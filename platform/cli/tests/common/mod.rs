//! Shared harness for the platform CLI command suite.
//!
//! Every test drives the real `layerx` binary as a child process against an
//! isolated configuration file and the in-memory mock credential store, so no
//! test can read or write a developer's real keychain or configuration.
#![allow(dead_code, clippy::missing_panics_doc, clippy::missing_errors_doc)]

use std::io::{Read as _, Write as _};
use std::net::{TcpListener, TcpStream};
#[cfg(unix)]
use std::os::unix::fs::OpenOptionsExt as _;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Output, Stdio};
use std::sync::atomic::{AtomicU32, Ordering};
use std::time::{Duration, Instant};

use serde_json::Value;

static SEQUENCE: AtomicU32 = AtomicU32::new(0);
const EMULATOR_SEQUENCER_SEED: [u8; 32] = [0x42; 32];

fn scratch(prefix: &str) -> PathBuf {
    let sequence = SEQUENCE.fetch_add(1, Ordering::Relaxed);
    std::env::temp_dir().join(format!(
        "layerx-cli-{prefix}-{}-{sequence}",
        std::process::id()
    ))
}

fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

/// An isolated CLI invocation environment.
///
/// Holds a private on-disk configuration file and pins the credential store to
/// the in-memory mock, so credential-touching commands run without a real
/// operating-system keychain and never disturb the machine's real state.
pub struct Cli {
    config: PathBuf,
}

impl Cli {
    #[must_use]
    pub fn new() -> Self {
        Self {
            config: scratch("config").with_extension("json"),
        }
    }

    #[must_use]
    pub fn config_path(&self) -> &Path {
        &self.config
    }

    #[must_use]
    pub fn config_contents(&self) -> String {
        std::fs::read_to_string(&self.config).unwrap_or_default()
    }

    fn command(&self, arguments: &[&str]) -> Command {
        let mut command = Command::new(env!("CARGO_BIN_EXE_layerx"));
        command
            .args(arguments)
            .env("LAYERX_CONFIG", &self.config)
            .env("LAYERX_CREDENTIAL_STORE", "mock")
            .env("LAYERX_REPO_ROOT", repo_root())
            .env_remove("XDG_CONFIG_HOME");
        command
    }

    /// Point the active `emulator` profile at a live emulator endpoint.
    pub fn bind_emulator(&self, endpoint: &str) -> Output {
        let trust_anchor = emulator_trust_anchor();
        self.run(&[
            "--json",
            "environment",
            "use",
            "emulator",
            "--endpoint",
            endpoint,
            "--network-id",
            "402",
            "--sequencer-trust-anchor",
            &trust_anchor,
        ])
    }

    #[must_use]
    pub fn run(&self, arguments: &[&str]) -> Output {
        match self.command(arguments).output() {
            Ok(output) => output,
            Err(error) => panic!("the layerx CLI should start: {error}"),
        }
    }

    #[must_use]
    pub fn run_with_stdin(&self, arguments: &[&str], stdin: &str) -> Output {
        let mut child = match self
            .command(arguments)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
        {
            Ok(child) => child,
            Err(error) => panic!("the layerx CLI should start: {error}"),
        };
        match child.stdin.take() {
            Some(mut pipe) => {
                if let Err(error) = pipe.write_all(stdin.as_bytes()) {
                    panic!("the layerx CLI should accept standard input: {error}");
                }
            }
            None => panic!("the layerx CLI should expose a standard input pipe"),
        }
        match child.wait_with_output() {
            Ok(output) => output,
            Err(error) => panic!("the layerx CLI should finish: {error}"),
        }
    }
}

impl Default for Cli {
    fn default() -> Self {
        Self::new()
    }
}

impl Drop for Cli {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.config);
    }
}

/// A live emulator process bound to an ephemeral loopback port.
pub struct Emulator {
    child: Child,
    endpoint: String,
    sequencer_seed: PathBuf,
}

impl Emulator {
    #[must_use]
    pub fn start() -> Self {
        let port = free_port();
        let listen = format!("127.0.0.1:{port}");
        let endpoint = format!("http://{listen}");
        let sequencer_seed = scratch("emulator-sequencer-seed");
        let mut seed_options = std::fs::OpenOptions::new();
        seed_options.write(true).create_new(true);
        #[cfg(unix)]
        seed_options.mode(0o600);
        let mut seed_file = seed_options
            .open(&sequencer_seed)
            .unwrap_or_else(|error| panic!("the emulator seed should be created: {error}"));
        seed_file
            .write_all(&EMULATOR_SEQUENCER_SEED)
            .unwrap_or_else(|error| panic!("the emulator seed should be written: {error}"));
        seed_file
            .sync_all()
            .unwrap_or_else(|error| panic!("the emulator seed should be durable: {error}"));
        let child = match Command::new(env!("CARGO_BIN_EXE_layerx"))
            .args(["emulator", "up", "--listen", &listen])
            .arg("--sequencer-seed-file")
            .arg(&sequencer_seed)
            .env_remove("LAYERX_CONFIG")
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
        {
            Ok(child) => child,
            Err(error) => panic!("the emulator should start: {error}"),
        };
        let emulator = Self {
            child,
            endpoint,
            sequencer_seed,
        };
        emulator.wait_until_ready();
        emulator
    }

    #[must_use]
    pub fn endpoint(&self) -> &str {
        &self.endpoint
    }

    fn wait_until_ready(&self) {
        let deadline = Instant::now() + Duration::from_secs(30);
        while Instant::now() < deadline {
            if let Ok(response) = http_get(&self.endpoint, "/healthz") {
                if response.contains("\"status\":\"ready\"") {
                    return;
                }
            }
            std::thread::sleep(Duration::from_millis(50));
        }
        panic!("the emulator never became ready at {}", self.endpoint);
    }
}

impl Drop for Emulator {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
        let _ = std::fs::remove_file(&self.sequencer_seed);
    }
}

pub fn emulator_trust_anchor() -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let public_key = ed25519_dalek::SigningKey::from_bytes(&EMULATOR_SEQUENCER_SEED)
        .verifying_key()
        .to_bytes();
    let mut encoded = String::with_capacity(public_key.len() * 2);
    for byte in public_key {
        encoded.push(char::from(HEX[usize::from(byte >> 4)]));
        encoded.push(char::from(HEX[usize::from(byte & 0x0f)]));
    }
    encoded
}

fn free_port() -> u16 {
    match TcpListener::bind("127.0.0.1:0") {
        Ok(listener) => match listener.local_addr() {
            Ok(address) => address.port(),
            Err(error) => panic!("a loopback port should be inspectable: {error}"),
        },
        Err(error) => panic!("a loopback port should be reservable: {error}"),
    }
}

pub fn http_get(endpoint: &str, path: &str) -> Result<String, String> {
    let authority = endpoint
        .strip_prefix("http://")
        .ok_or_else(|| "endpoint must be loopback http".to_string())?;
    let mut stream =
        TcpStream::connect(authority).map_err(|error| format!("connect failed: {error}"))?;
    let request = format!("GET {path} HTTP/1.1\r\nHost: {authority}\r\nConnection: close\r\n\r\n");
    stream
        .write_all(request.as_bytes())
        .map_err(|error| format!("write failed: {error}"))?;
    let mut response = String::new();
    stream
        .read_to_string(&mut response)
        .map_err(|error| format!("read failed: {error}"))?;
    Ok(response)
}

/// Parse the CLI's machine-readable success envelope from standard output.
#[must_use]
pub fn envelope(output: &Output) -> Value {
    match serde_json::from_slice(&output.stdout) {
        Ok(value) => value,
        Err(error) => panic!(
            "the CLI should emit a JSON success envelope: {error}; stdout={}",
            String::from_utf8_lossy(&output.stdout)
        ),
    }
}

/// Parse the CLI's machine-readable error envelope from standard error.
#[must_use]
pub fn error_envelope(output: &Output) -> Value {
    match serde_json::from_slice(&output.stderr) {
        Ok(value) => value,
        Err(error) => panic!(
            "the CLI should emit a JSON error envelope: {error}; stderr={}",
            String::from_utf8_lossy(&output.stderr)
        ),
    }
}

/// Fetch a string field from a JSON value, failing the test if it is absent.
#[must_use]
pub fn string_field<'a>(value: &'a Value, pointer: &str) -> &'a str {
    match value.pointer(pointer).and_then(Value::as_str) {
        Some(text) => text,
        None => panic!("expected a string at {pointer} in {value}"),
    }
}
