use std::error::Error;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicU64, Ordering};

static NEXT_DIRECTORY: AtomicU64 = AtomicU64::new(0);

const KEY_FILE: &str = "LAYERX_EXPLORER_READ_KEY_FILE";
const ENDPOINT: &str = "LAYERX_EXPLORER_READ_ENDPOINT";
const CA_DER: &str = "LAYERX_EXPLORER_READ_CA_DER";
const SEQUENCER_KEY_FILE: &str = "LAYERX_EXPLORER_READ_SEQUENCER_PUBLIC_KEY_FILE";
const NETWORK_ID: &str = "LAYERX_EXPLORER_READ_NETWORK_ID";
const FEE_LIMIT: &str = "LAYERX_EXPLORER_READ_FEE_LIMIT";
const NAMING_PROGRAM: &str = "LAYERX_EXPLORER_NAMING_PROGRAM";

struct Directory(PathBuf);

impl Directory {
    fn create() -> Result<Self, Box<dyn Error>> {
        let path = std::env::temp_dir().join(format!(
            "explorer-read-principal-config-{}-{}",
            std::process::id(),
            NEXT_DIRECTORY.fetch_add(1, Ordering::Relaxed),
        ));
        fs::create_dir(&path)?;
        Ok(Self(path))
    }

    fn file(&self, name: &str, bytes: &[u8]) -> Result<PathBuf, Box<dyn Error>> {
        let path = self.0.join(name);
        fs::write(&path, bytes)?;
        Ok(path)
    }
}

impl Drop for Directory {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn certificate(directory: &Directory) -> Result<PathBuf, Box<dyn Error>> {
    let ca = directory.0.join("ca.der");
    let output = Command::new("openssl")
        .args([
            "req",
            "-x509",
            "-newkey",
            "rsa:2048",
            "-nodes",
            "-subj",
            "/CN=explorer-read-principal-config",
            "-addext",
            "basicConstraints=critical,CA:TRUE",
            "-addext",
            "keyUsage=critical,keyCertSign,cRLSign",
            "-days",
            "1",
            "-outform",
            "DER",
            "-out",
        ])
        .arg(&ca)
        .arg("-keyout")
        .arg(directory.0.join("key.pem"))
        .output()?;
    assert!(output.status.success(), "CA certificate generation failed");
    Ok(ca)
}

fn explorer(directory: &Directory, ca: &Path) -> Command {
    let mut command = Command::new(env!("CARGO_BIN_EXE_layerx-explorer-index"));
    command
        .env_clear()
        .env("LAYERX_EXPLORER_PROGRAM_LISTEN", "127.0.0.1:0")
        .env("LAYERX_EXPLORER_PROGRAM_BEARER_TOKEN", "e".repeat(32))
        .env("LAYERX_EXPLORER_NODE_BEARER_TOKEN", "n".repeat(32))
        .env("LAYERX_EXPLORER_AUTHORITY_BEARER_TOKEN", "a".repeat(32))
        .env("LAYERX_EXPLORER_PROGRAM_MAX_STALENESS_MS", "1000")
        .env("LAYERX_EXPLORER_NODE_ENDPOINT", "https://localhost:1")
        .env("LAYERX_EXPLORER_AUTHORITY_ENDPOINT", "https://localhost:2")
        .env("LAYERX_EXPLORER_AUTHORITY_CA_DER", ca)
        .env("LAYERX_EXPLORER_AUTHORITY_REPLICA_ID", "11".repeat(32))
        .env(
            "LAYERX_EXPLORER_SEQUENCER_TRUST_HISTORY",
            directory.0.join("history"),
        )
        .env(
            "LAYERX_EXPLORER_DEPLOYMENT_JOURNAL",
            directory.0.join("journal"),
        )
        .env(
            "LAYERX_EXPLORER_VERIFIED_SOURCE_STORE",
            directory.0.join("verified"),
        )
        .env("LAYERX_EXPLORER_PROGRAM_PROBE_ID", "22".repeat(32))
        .env("LAYERX_EXPLORER_OBSERVED_SEALED_BATCH", "1")
        .env("LAYERX_EXPLORER_FINALISED_CHECKPOINT", "33".repeat(32));
    command
}

fn stderr(command: &mut Command) -> Result<String, Box<dyn Error>> {
    let output = command.output()?;
    assert_eq!(output.status.code(), Some(2));
    assert!(output.stdout.is_empty());
    Ok(String::from_utf8(output.stderr)?.trim().to_owned())
}

fn refusal(command: &mut Command, expected: &str) -> Result<(), Box<dyn Error>> {
    assert_eq!(
        stderr(command)?,
        format!("layerx-explorer-index: {expected}")
    );
    Ok(())
}

#[test]
fn every_read_principal_input_refuses_startup_by_name() -> Result<(), Box<dyn Error>> {
    let directory = Directory::create()?;
    let ca = certificate(&directory)?;
    let command = || explorer(&directory, &ca);
    refusal(&mut command(), &format!("{KEY_FILE} is required"))?;
    refusal(
        command().env(KEY_FILE, directory.0.join("missing.key")),
        &format!("{KEY_FILE} is unreadable"),
    )?;
    for (name, bytes, error) in [
        ("empty.key", Vec::new(), "is empty"),
        ("blank.key", b"\n".to_vec(), "is empty"),
        (
            "oversized.key",
            vec![b'0'; 257],
            "exceeds the key file size limit",
        ),
        (
            "malformed.key",
            b"not a seed".to_vec(),
            "must contain a hexadecimal ed25519 seed",
        ),
        (
            "short.key",
            "07".repeat(31).into_bytes(),
            "must contain a hexadecimal ed25519 seed",
        ),
    ] {
        let path = directory.file(name, &bytes)?;
        refusal(
            command().env(KEY_FILE, path),
            &format!("{KEY_FILE} {error}"),
        )?;
    }
    let key = directory.file("read.key", format!("{}\n", "07".repeat(32)).as_bytes())?;
    let keyed = || {
        let mut command = command();
        command.env(KEY_FILE, &key);
        command
    };
    refusal(&mut keyed(), &format!("{ENDPOINT} is required"))?;
    refusal(
        keyed().env(ENDPOINT, "https://localhost:3"),
        &format!("{CA_DER} is required"),
    )?;
    refusal(
        keyed()
            .env(ENDPOINT, "https://localhost:3")
            .env(CA_DER, directory.0.join("missing.der")),
        &format!("{CA_DER} is unreadable"),
    )?;
    refusal(
        keyed().env(ENDPOINT, "http://localhost:3").env(CA_DER, &ca),
        &format!("{ENDPOINT} must be https://<host>:<port>"),
    )?;
    let reachable = || {
        let mut command = keyed();
        command
            .env(ENDPOINT, "https://localhost:3")
            .env(CA_DER, &ca);
        command
    };
    refusal(
        &mut reachable(),
        &format!("{SEQUENCER_KEY_FILE} is required"),
    )?;
    refusal(
        reachable().env(SEQUENCER_KEY_FILE, directory.0.join("missing.pub")),
        &format!("{SEQUENCER_KEY_FILE} is unreadable"),
    )?;
    let malformed = directory.file("malformed.pub", b"sequencer")?;
    assert!(
        stderr(reachable().env(SEQUENCER_KEY_FILE, malformed))?.starts_with(&format!(
            "layerx-explorer-index: {SEQUENCER_KEY_FILE} is invalid: "
        ))
    );
    let sequencer = directory.file("sequencer.pub", "44".repeat(32).as_bytes())?;
    let trusted = || {
        let mut command = reachable();
        command.env(SEQUENCER_KEY_FILE, &sequencer);
        command
    };
    refusal(&mut trusted(), &format!("{NETWORK_ID} is required"))?;
    for value in ["0", "-1", "network", "4294967296"] {
        refusal(
            trusted().env(NETWORK_ID, value),
            &format!("{NETWORK_ID} must be a nonzero unsigned integer"),
        )?;
    }
    refusal(
        trusted().env(NETWORK_ID, "7"),
        &format!("{FEE_LIMIT} is required"),
    )?;
    refusal(
        trusted().env(NETWORK_ID, "7").env(FEE_LIMIT, "-1"),
        &format!("{FEE_LIMIT} must be an unsigned integer"),
    )?;
    refusal(
        trusted().env(NETWORK_ID, "7").env(FEE_LIMIT, "0"),
        &format!("{NAMING_PROGRAM} is required"),
    )?;
    assert!(stderr(
        trusted()
            .env(NETWORK_ID, "7")
            .env(FEE_LIMIT, "0")
            .env(NAMING_PROGRAM, "naming")
    )?
    .starts_with(&format!(
        "layerx-explorer-index: {NAMING_PROGRAM} is invalid: "
    )));
    let complete = stderr(
        trusted()
            .env(NETWORK_ID, "7")
            .env(FEE_LIMIT, "0")
            .env(NAMING_PROGRAM, "55".repeat(32)),
    )?;
    assert!(!complete.contains("LAYERX_EXPLORER_READ_"), "{complete}");
    assert!(!complete.contains(NAMING_PROGRAM), "{complete}");
    Ok(())
}
