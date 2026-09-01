//! End-to-end command coverage against a live emulator.
//!
//! Each test boots the real emulator over the production transition function on
//! an ephemeral loopback port and drives the CLI against it, proving that
//! environment selection, key management and account funding round-trip through
//! the same gateway surface a developer would use.

mod common;

use common::{envelope, error_envelope, string_field, Cli, Emulator};
use serde_json::Value;

#[test]
fn environment_selection_points_at_the_live_emulator() {
    let emulator = Emulator::start();
    let cli = Cli::new();
    assert!(cli.bind_emulator(emulator.endpoint()).status.success());

    let current = cli.run(&["--json", "environment", "current"]);
    assert!(current.status.success());
    let value = envelope(&current);
    assert_eq!(string_field(&value, "/data/name"), "emulator");
    assert_eq!(string_field(&value, "/data/endpoint"), emulator.endpoint());
    assert_eq!(
        value.pointer("/data/network_id").and_then(Value::as_u64),
        Some(402)
    );
}

#[test]
fn account_is_created_and_read_back_through_the_emulator() {
    let emulator = Emulator::start();
    let cli = Cli::new();
    assert!(cli.bind_emulator(emulator.endpoint()).status.success());

    let created_key = cli.run(&["--json", "key", "create", "alpha"]);
    assert!(created_key.status.success());
    let did = string_field(&envelope(&created_key), "/data/did").to_owned();

    let created = cli.run(&[
        "--json",
        "account",
        "create",
        "--key",
        "alpha",
        "--initial-amount",
        "1000000",
    ]);
    assert!(
        created.status.success(),
        "account create should succeed: {}",
        String::from_utf8_lossy(&created.stderr)
    );
    let value = envelope(&created);
    assert_eq!(string_field(&value, "/kind"), "account.created");
    assert_eq!(
        string_field(&value, "/data/account"),
        format!("agent:{did}:main")
    );
    assert_eq!(string_field(&value, "/data/funding"), "emulator-prefund");

    let fetched = cli.run(&["--json", "account", "get", "--did", &did]);
    assert!(
        fetched.status.success(),
        "account get should succeed: {}",
        String::from_utf8_lossy(&fetched.stderr)
    );
    let account = envelope(&fetched);
    assert_eq!(
        string_field(&account, "/data/account/name"),
        format!("agent:{did}:main")
    );
    assert_eq!(
        account
            .pointer("/data/account/balance_lo")
            .and_then(Value::as_u64),
        Some(1_000_000)
    );
}

#[test]
fn reading_a_missing_account_is_a_machine_readable_refusal() {
    let emulator = Emulator::start();
    let cli = Cli::new();
    assert!(cli.bind_emulator(emulator.endpoint()).status.success());

    let output = cli.run(&["--json", "account", "get", "--did", "did:layerx:absent"]);
    assert!(!output.status.success());
    let value = error_envelope(&output);
    assert_eq!(value.pointer("/ok").and_then(Value::as_bool), Some(false));
    assert_eq!(
        value.pointer("/error/code").and_then(Value::as_str),
        Some("command_failed")
    );
}

#[test]
fn human_and_machine_output_agree_on_the_active_environment() {
    let emulator = Emulator::start();
    let cli = Cli::new();
    assert!(cli.bind_emulator(emulator.endpoint()).status.success());

    let human = cli.run(&["environment", "current"]);
    assert!(human.status.success());
    let rendered = String::from_utf8_lossy(&human.stdout);
    assert!(
        rendered.contains("emulator"),
        "human output should name the active environment: {rendered}"
    );
    // The human presentation is not JSON; only the --json form is machine-readable.
    assert!(serde_json::from_slice::<Value>(&human.stdout).is_err());
}

#[test]
fn payment_test_quotes_and_commits_once_through_the_live_emulator() {
    let emulator = Emulator::start();
    let cli = Cli::new();
    assert!(cli.bind_emulator(emulator.endpoint()).status.success());

    let seed = "42".repeat(32);
    let imported = cli.run_with_stdin(
        &[
            "--json",
            "key",
            "import",
            "move-source",
            "--did",
            "did:layerx:move-source",
        ],
        &seed,
    );
    assert!(
        imported.status.success(),
        "source import should succeed: {}",
        String::from_utf8_lossy(&imported.stderr)
    );
    let destination_key = cli.run(&[
        "--json",
        "key",
        "create",
        "move-destination",
        "--did",
        "did:layerx:move-destination",
    ]);
    assert!(destination_key.status.success());
    let source_account = cli.run(&[
        "--json",
        "account",
        "create",
        "--key",
        "move-source",
        "--initial-amount",
        "1000",
    ]);
    assert!(source_account.status.success());
    let destination_account = cli.run(&[
        "--json",
        "account",
        "create",
        "--key",
        "move-destination",
        "--initial-amount",
        "0",
    ]);
    assert!(destination_account.status.success());

    let payment = cli.run(&[
        "--json",
        "payment",
        "test",
        "--from",
        "agent:did:layerx:move-source:main",
        "--to",
        "agent:did:layerx:move-destination:main",
        "--currency",
        "LXP",
        "--amount",
        "250",
        "--idempotency-key",
        "cli-payment-test-0001",
    ]);
    assert!(
        payment.status.success(),
        "payment should succeed: {}",
        String::from_utf8_lossy(&payment.stderr)
    );
    let payment = envelope(&payment);
    assert_eq!(string_field(&payment, "/kind"), "payment.started");
    assert_eq!(string_field(&payment, "/data/journey/result/state"), "done");
    assert!(
        payment
            .pointer("/data/journey/result/evidence/0/verification")
            .and_then(Value::as_str)
            == Some("receipt-verified")
    );

    let source = envelope(&cli.run(&[
        "--json",
        "account",
        "get",
        "--did",
        "did:layerx:move-source",
    ]));
    let destination = envelope(&cli.run(&[
        "--json",
        "account",
        "get",
        "--did",
        "did:layerx:move-destination",
    ]));
    assert_eq!(
        source
            .pointer("/data/account/balance_lo")
            .and_then(Value::as_u64),
        Some(750)
    );
    assert_eq!(
        destination
            .pointer("/data/account/balance_lo")
            .and_then(Value::as_u64),
        Some(250)
    );
}

#[cfg(any(target_os = "linux", target_os = "android", target_vendor = "apple"))]
mod bootstrap {
    use std::fmt::Write as _;
    use std::fs;
    use std::os::unix::fs::{MetadataExt as _, PermissionsExt as _};
    use std::path::{Path, PathBuf};
    use std::process::{Command, Output, Stdio};
    use std::sync::atomic::{AtomicU32, Ordering};
    use std::time::Instant;

    use serde_json::Value;

    use crate::common::{
        emulator_trust_anchor, envelope, error_envelope, http_get, string_field, Emulator,
    };

    const BOOTSTRAP_MARKER: &str = "<!-- layerx:bootstrap-sequence -->";
    const LAYERX: &str = env!("CARGO_BIN_EXE_layerx");
    static PROFILE_SEQUENCE: AtomicU32 = AtomicU32::new(0);

    struct Profile {
        root: PathBuf,
    }

    impl Profile {
        fn new(prefix: &str) -> Self {
            let sequence = PROFILE_SEQUENCE.fetch_add(1, Ordering::Relaxed);
            let root = std::env::temp_dir().join(format!(
                "layerx-cli-profile-{prefix}-{}-{sequence}",
                std::process::id()
            ));
            if let Err(error) = fs::create_dir_all(&root) {
                panic!(
                    "profile directory {} should be creatable: {error}",
                    root.display()
                );
            }
            Self { root }
        }

        fn config(&self) -> PathBuf {
            self.root.join("config.json")
        }

        fn seed_file(&self) -> PathBuf {
            self.root.join("emulator").join("sequencer.seed")
        }

        fn anchor_file(&self) -> PathBuf {
            self.root.join("emulator").join("sequencer.anchor")
        }

        fn write(&self, name: &str, contents: &str) -> PathBuf {
            let path = self.root.join(name);
            if let Err(error) = fs::write(&path, contents) {
                panic!("{} should be writable: {error}", path.display());
            }
            path
        }

        fn command_with_config(&self, config: &Path, arguments: &[&str]) -> Command {
            let mut command = Command::new(LAYERX);
            command
                .args(arguments)
                .env("HOME", &self.root)
                .env("LAYERX_CONFIG", config)
                .env("LAYERX_CREDENTIAL_STORE", "mock")
                .env_remove("XDG_CONFIG_HOME")
                .env_remove("LAYERX_INSTALL_ROOT");
            command
        }

        fn command(&self, arguments: &[&str]) -> Command {
            self.command_with_config(&self.config(), arguments)
        }

        fn run(&self, arguments: &[&str]) -> Output {
            match self.command(arguments).output() {
                Ok(output) => output,
                Err(error) => panic!("layerx should be runnable: {error}"),
            }
        }

        fn run_with_config(&self, config: &Path, arguments: &[&str]) -> Output {
            match self.command_with_config(config, arguments).output() {
                Ok(output) => output,
                Err(error) => panic!("layerx should be runnable: {error}"),
            }
        }
    }

    impl Drop for Profile {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.root);
        }
    }

    struct BackgroundProcesses {
        pids: Vec<String>,
    }

    impl BackgroundProcesses {
        fn from_pid_file(path: &Path) -> Self {
            let pids = fs::read_to_string(path)
                .unwrap_or_default()
                .lines()
                .map(str::trim)
                .filter(|line| !line.is_empty())
                .map(str::to_owned)
                .collect();
            Self { pids }
        }
    }

    impl Drop for BackgroundProcesses {
        fn drop(&mut self) {
            for pid in &self.pids {
                let _ = Command::new("kill").arg(pid).status();
            }
        }
    }

    fn hex(bytes: &[u8]) -> String {
        let mut encoded = String::with_capacity(bytes.len() * 2);
        for byte in bytes {
            let _ = write!(encoded, "{byte:02x}");
        }
        encoded
    }

    fn read_trimmed(path: &Path) -> String {
        match fs::read_to_string(path) {
            Ok(contents) => contents.trim().to_owned(),
            Err(error) => panic!("{} should be readable: {error}", path.display()),
        }
    }

    fn read_seed(path: &Path) -> (String, [u8; 32]) {
        let seed_hex = read_trimmed(path);
        assert_eq!(
            seed_hex.len(),
            64,
            "seed file should hold 64 hex characters"
        );
        let mut bytes = [0_u8; 32];
        for (slot, chunk) in bytes.iter_mut().zip(seed_hex.as_bytes().chunks(2)) {
            let text = std::str::from_utf8(chunk).unwrap_or("zz");
            *slot = match u8::from_str_radix(text, 16) {
                Ok(byte) => byte,
                Err(error) => panic!("seed file should be hex: {error}"),
            };
        }
        (seed_hex, bytes)
    }

    fn derived_anchor(seed: &[u8; 32]) -> String {
        hex(&ed25519_dalek::SigningKey::from_bytes(seed)
            .verifying_key()
            .to_bytes())
    }

    fn other_anchor() -> String {
        derived_anchor(&[0x24; 32])
    }

    fn assert_owner_only(path: &Path, expected_mode: u32) {
        let metadata = match fs::symlink_metadata(path) {
            Ok(metadata) => metadata,
            Err(error) => panic!("{} should exist: {error}", path.display()),
        };
        assert_eq!(
            metadata.mode() & 0o777,
            expected_mode,
            "{} should have mode {expected_mode:o}",
            path.display()
        );
        assert_eq!(
            metadata.uid(),
            rustix::process::geteuid().as_raw(),
            "{} should be owned by the current user",
            path.display()
        );
    }

    fn assert_no_seed_material(
        label: &str,
        streams: &[&[u8]],
        seed_hex: &str,
        seed_bytes: &[u8; 32],
    ) {
        let upper = seed_hex.to_ascii_uppercase();
        for stream in streams {
            let text = String::from_utf8_lossy(stream);
            assert!(
                !text.contains(seed_hex),
                "{label} leaked the seed as hex: {text}"
            );
            assert!(
                !text.contains(&upper),
                "{label} leaked the seed as upper-case hex: {text}"
            );
            assert!(
                !stream
                    .windows(seed_bytes.len())
                    .any(|window| window == seed_bytes),
                "{label} leaked raw seed bytes"
            );
        }
    }

    fn assert_refused(output: &Output, code: &str) -> String {
        assert!(
            !output.status.success(),
            "command should be refused: {}",
            String::from_utf8_lossy(&output.stdout)
        );
        let error = error_envelope(output);
        assert_eq!(string_field(&error, "/error/code"), code);
        let detail = string_field(&error, "/error/detail").to_owned();
        assert!(
            detail.starts_with(&format!("{code}: ")),
            "expected {code}, got {detail}"
        );
        detail
    }

    fn published_bootstrap_sequence() -> String {
        let path = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("..")
            .join("docs")
            .join("content")
            .join("install.md");
        let document = match fs::read_to_string(&path) {
            Ok(document) => document,
            Err(error) => panic!("{} should be readable: {error}", path.display()),
        };
        assert_eq!(
            document
                .lines()
                .filter(|line| *line == BOOTSTRAP_MARKER)
                .count(),
            1,
            "install.md should carry exactly one bootstrap marker"
        );
        let mut lines = document
            .lines()
            .skip_while(|line| *line != BOOTSTRAP_MARKER)
            .skip(1);
        assert!(
            lines.next().is_some_and(|line| line.starts_with("```")),
            "the bootstrap marker should sit immediately above a fenced block"
        );
        let mut sequence = Vec::new();
        loop {
            match lines.next() {
                Some("```") => break,
                Some(line) => sequence.push(line),
                None => panic!("the bootstrap block is not closed"),
            }
        }
        assert!(
            !sequence.is_empty(),
            "the bootstrap block should not be empty"
        );
        sequence.join("\n")
    }

    fn flag_value(sequence: &str, flag: &str) -> String {
        let mut words = sequence.split_whitespace().skip_while(|word| *word != flag);
        match words.nth(1) {
            Some(value) => value.to_owned(),
            None => panic!("the published sequence should pass {flag}"),
        }
    }

    fn assert_emulator_reachable(endpoint: &str, network_id: u64, anchor: &str) {
        let health = match http_get(endpoint, "/healthz") {
            Ok(health) => health,
            Err(error) => panic!("emulator at {endpoint} should be reachable: {error}"),
        };
        assert!(
            health.contains("\"status\":\"ready\""),
            "emulator should be ready: {health}"
        );
        let identity = match http_get(endpoint, "/v1/sequencer") {
            Ok(identity) => identity,
            Err(error) => panic!("emulator at {endpoint} should advertise its identity: {error}"),
        };
        assert!(
            identity.contains(&format!("\"network_id\":{network_id}")),
            "emulator should advertise network id {network_id}: {identity}"
        );
        assert!(
            identity.contains(&format!("\"sequencer_public_key\":\"{anchor}\"")),
            "emulator identity should match the published anchor: {identity}"
        );
    }

    fn assert_current_environment(home: &Path, endpoint: &str, network_id: u64, anchor: &str) {
        let output = Command::new(LAYERX)
            .args(["--json", "environment", "current"])
            .env_clear()
            .env("HOME", home)
            .output();
        let output = match output {
            Ok(output) => output,
            Err(error) => panic!("layerx should be runnable: {error}"),
        };
        assert!(
            output.status.success(),
            "environment current should succeed: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        let value = envelope(&output);
        assert_eq!(string_field(&value, "/kind"), "environment.current");
        assert_eq!(string_field(&value, "/data/name"), "emulator");
        assert_eq!(string_field(&value, "/data/endpoint"), endpoint);
        assert_eq!(
            value.pointer("/data/network_id").and_then(Value::as_u64),
            Some(network_id)
        );
        assert_eq!(string_field(&value, "/data/sequencer_trust_anchor"), anchor);
    }

    #[test]
    fn emulator_up_requires_the_sequencer_seed_file() {
        let profile = Profile::new("up-without-seed");
        let output = profile.run(&["emulator", "up", "--listen", "127.0.0.1:0"]);
        assert!(
            !output.status.success(),
            "emulator up must not start without a seed file"
        );
        let stderr = String::from_utf8_lossy(&output.stderr);
        assert!(
            stderr.contains("--sequencer-seed-file"),
            "the refusal should name the missing seed file: {stderr}"
        );
    }

    #[test]
    fn provision_writes_an_owner_only_seed_and_prints_only_paths_and_the_anchor() {
        let profile = Profile::new("provision-json");
        let output = profile.run(&["--json", "emulator", "provision"]);
        assert!(
            output.status.success(),
            "provision should succeed: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        let value = envelope(&output);
        assert_eq!(string_field(&value, "/kind"), "emulator.provisioned");
        assert_eq!(
            Path::new(string_field(&value, "/data/sequencer_seed_file")),
            profile.seed_file()
        );
        assert_eq!(
            Path::new(string_field(&value, "/data/sequencer_trust_anchor_file")),
            profile.anchor_file()
        );
        assert_owner_only(&profile.root.join("emulator"), 0o700);
        assert_owner_only(&profile.seed_file(), 0o600);
        assert_owner_only(&profile.anchor_file(), 0o600);
        let (seed_hex, seed_bytes) = read_seed(&profile.seed_file());
        let anchor = derived_anchor(&seed_bytes);
        assert_eq!(string_field(&value, "/data/sequencer_trust_anchor"), anchor);
        assert_eq!(read_trimmed(&profile.anchor_file()), anchor);
        let json = value.to_string();
        assert_no_seed_material(
            "provision --json",
            &[&output.stdout, &output.stderr, json.as_bytes()],
            &seed_hex,
            &seed_bytes,
        );

        let text_profile = Profile::new("provision-text");
        let text = text_profile.run(&["emulator", "provision"]);
        assert!(
            text.status.success(),
            "provision should succeed: {}",
            String::from_utf8_lossy(&text.stderr)
        );
        let (seed_hex, seed_bytes) = read_seed(&text_profile.seed_file());
        assert_no_seed_material(
            "provision",
            &[&text.stdout, &text.stderr],
            &seed_hex,
            &seed_bytes,
        );
        let stdout = String::from_utf8_lossy(&text.stdout);
        assert!(
            stdout.contains(&derived_anchor(&seed_bytes)),
            "text output should show the anchor"
        );
        assert!(
            stdout.contains(&text_profile.seed_file().display().to_string()),
            "text output should show the seed path"
        );
    }

    #[test]
    fn provision_refuses_to_overwrite_without_force() {
        let profile = Profile::new("provision-force");
        let first = profile.run(&["--json", "emulator", "provision"]);
        assert!(first.status.success(), "first provision should succeed");
        let seed_before = read_trimmed(&profile.seed_file());
        let anchor_before = read_trimmed(&profile.anchor_file());

        let refused = profile.run(&["--json", "emulator", "provision"]);
        let detail = assert_refused(&refused, "sequencer_seed_exists");
        assert!(
            detail.contains("--force"),
            "the refusal should name --force: {detail}"
        );
        assert_eq!(read_trimmed(&profile.seed_file()), seed_before);
        assert_eq!(read_trimmed(&profile.anchor_file()), anchor_before);
        assert_no_seed_material(
            "refused provision",
            &[&refused.stdout, &refused.stderr],
            &seed_before,
            &read_seed(&profile.seed_file()).1,
        );

        let forced = profile.run(&["--json", "emulator", "provision", "--force"]);
        assert!(
            forced.status.success(),
            "provision --force should replace the identity: {}",
            String::from_utf8_lossy(&forced.stderr)
        );
        let (seed_after, seed_bytes) = read_seed(&profile.seed_file());
        assert_ne!(
            seed_after, seed_before,
            "--force should generate a new seed"
        );
        assert_eq!(
            read_trimmed(&profile.anchor_file()),
            derived_anchor(&seed_bytes)
        );
        assert_eq!(
            string_field(&envelope(&forced), "/data/sequencer_trust_anchor"),
            derived_anchor(&seed_bytes)
        );
        assert_owner_only(&profile.seed_file(), 0o600);
        assert_no_seed_material(
            "provision --force",
            &[&forced.stdout, &forced.stderr],
            &seed_after,
            &seed_bytes,
        );
    }

    #[test]
    fn concurrent_forced_provisions_publish_one_consistent_identity_pair() {
        let profile = Profile::new("provision-concurrent-force");
        let first = profile.run(&["--json", "emulator", "provision"]);
        assert!(first.status.success(), "initial provision should succeed");

        let mut children = Vec::new();
        for _ in 0..6 {
            let child = profile
                .command(&["--json", "emulator", "provision", "--force"])
                .stdout(Stdio::piped())
                .stderr(Stdio::piped())
                .spawn();
            children.push(match child {
                Ok(child) => child,
                Err(error) => panic!("concurrent provision should start: {error}"),
            });
        }
        for child in children {
            let output = match child.wait_with_output() {
                Ok(output) => output,
                Err(error) => panic!("concurrent provision should finish: {error}"),
            };
            assert!(
                output.status.success(),
                "concurrent provision should succeed: {}",
                String::from_utf8_lossy(&output.stderr)
            );
        }

        let (_, seed_bytes) = read_seed(&profile.seed_file());
        assert_eq!(
            read_trimmed(&profile.anchor_file()),
            derived_anchor(&seed_bytes),
            "the published anchor must derive from the atomically published seed"
        );
        assert_owner_only(&profile.root.join("emulator"), 0o700);
        assert_owner_only(&profile.seed_file(), 0o600);
        assert_owner_only(&profile.anchor_file(), 0o600);
    }

    #[test]
    fn provision_refuses_a_symlinked_profile_ancestor() {
        let profile = Profile::new("provision-symlink-ancestor");
        let real = profile.root.join("real-profile");
        if let Err(error) = fs::create_dir(&real) {
            panic!("real profile should be creatable: {error}");
        }
        let linked = profile.root.join("linked-profile");
        if let Err(error) = std::os::unix::fs::symlink(&real, &linked) {
            panic!("profile ancestor should be linkable: {error}");
        }
        let config = linked.join("config.json");
        let output = profile.run_with_config(&config, &["--json", "emulator", "provision"]);
        let detail = assert_refused(&output, "profile_directory_unavailable");
        assert!(detail.contains("symlink ancestor"), "{detail}");
        assert!(
            !real.join("emulator").exists(),
            "a symlinked profile must not receive an identity"
        );
    }

    #[test]
    fn provision_recovers_a_stale_private_identity_stage() {
        let profile = Profile::new("provision-stale-stage");
        let stale = profile.root.join(".emulator-stage-interrupted");
        if let Err(error) = fs::create_dir(&stale) {
            panic!("stale stage should be creatable: {error}");
        }
        if let Err(error) = fs::set_permissions(&stale, fs::Permissions::from_mode(0o700)) {
            panic!("stale stage should be protectable: {error}");
        }

        let output = profile.run(&["--json", "emulator", "provision"]);
        assert!(
            output.status.success(),
            "provision should recover the stale stage: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        assert!(!stale.exists(), "the stale stage should be removed");
        let (_, seed_bytes) = read_seed(&profile.seed_file());
        assert_eq!(
            read_trimmed(&profile.anchor_file()),
            derived_anchor(&seed_bytes)
        );
    }

    #[test]
    fn environment_use_binds_the_anchor_file_after_verifying_the_advertised_identity() {
        let emulator = Emulator::start();
        let profile = Profile::new("use-anchor-file");
        let anchor = emulator_trust_anchor();
        let anchor_file = profile.write("published.anchor", &format!("{anchor}\n"));
        let output = profile.run(&[
            "--json",
            "environment",
            "use",
            "emulator",
            "--endpoint",
            emulator.endpoint(),
            "--network-id",
            "402",
            "--sequencer-trust-anchor-file",
            &anchor_file.display().to_string(),
        ]);
        assert!(
            output.status.success(),
            "binding a matching anchor should succeed: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        let value = envelope(&output);
        assert_eq!(string_field(&value, "/kind"), "environment.selected");
        assert_eq!(string_field(&value, "/data/sequencer_trust_anchor"), anchor);
        assert_eq!(
            string_field(&value, "/data/sequencer_identity/sequencer_public_key"),
            anchor
        );
        assert_eq!(
            value
                .pointer("/data/sequencer_identity/network_id")
                .and_then(Value::as_u64),
            Some(402)
        );
        let current = envelope(&profile.run(&["--json", "environment", "current"]));
        assert_eq!(string_field(&current, "/data/name"), "emulator");
        assert_eq!(
            string_field(&current, "/data/endpoint"),
            emulator.endpoint()
        );
        assert_eq!(
            string_field(&current, "/data/sequencer_trust_anchor"),
            anchor
        );
    }

    #[test]
    fn environment_use_reverifies_a_stored_emulator_identity_before_selection() {
        let emulator = Emulator::start();
        let profile = Profile::new("use-stored-identity");
        let anchor_file = profile.write("published.anchor", &emulator_trust_anchor());
        let bound = profile.run(&[
            "--json",
            "environment",
            "use",
            "emulator",
            "--endpoint",
            emulator.endpoint(),
            "--network-id",
            "402",
            "--sequencer-trust-anchor-file",
            &anchor_file.display().to_string(),
        ]);
        assert!(bound.status.success(), "initial binding should succeed");

        let mut configuration: Value = match fs::read_to_string(profile.config()) {
            Ok(source) => match serde_json::from_str(&source) {
                Ok(configuration) => configuration,
                Err(error) => panic!("configuration should be JSON: {error}"),
            },
            Err(error) => panic!("configuration should be readable: {error}"),
        };
        let stored_anchor = configuration
            .pointer_mut("/environments/emulator/sequencer_trust_anchor")
            .unwrap_or_else(|| panic!("emulator anchor should be stored"));
        *stored_anchor = Value::String(other_anchor());
        let tampered = match serde_json::to_string_pretty(&configuration) {
            Ok(tampered) => format!("{tampered}\n"),
            Err(error) => panic!("configuration should encode: {error}"),
        };
        if let Err(error) = fs::write(profile.config(), &tampered) {
            panic!("configuration should be writable: {error}");
        }

        let selected = profile.run(&["--json", "environment", "use", "emulator"]);
        let detail = assert_refused(&selected, "sequencer_trust_anchor_mismatch");
        assert!(
            detail.contains("configured sequencer trust anchor"),
            "{detail}"
        );
        assert_eq!(
            fs::read_to_string(profile.config()).unwrap_or_default(),
            tampered,
            "a failed live re-verification must not save the configuration"
        );
    }

    #[test]
    fn environment_use_refuses_an_anchor_that_disagrees_with_the_advertised_identity() {
        let emulator = Emulator::start();
        let profile = Profile::new("use-anchor-mismatch");
        let anchor_file = profile.write("other.anchor", &other_anchor());
        let from_file = profile.run(&[
            "--json",
            "environment",
            "use",
            "emulator",
            "--endpoint",
            emulator.endpoint(),
            "--network-id",
            "402",
            "--sequencer-trust-anchor-file",
            &anchor_file.display().to_string(),
        ]);
        let detail = assert_refused(&from_file, "sequencer_trust_anchor_mismatch");
        assert!(detail.contains("--sequencer-trust-anchor-file"), "{detail}");
        assert!(detail.contains(&emulator_trust_anchor()), "{detail}");
        assert!(
            !profile.config().exists(),
            "a refused binding must not be saved"
        );

        let literal = profile.run(&[
            "--json",
            "environment",
            "use",
            "emulator",
            "--endpoint",
            emulator.endpoint(),
            "--network-id",
            "402",
            "--sequencer-trust-anchor",
            &other_anchor(),
        ]);
        let detail = assert_refused(&literal, "sequencer_trust_anchor_mismatch");
        assert!(detail.contains("--sequencer-trust-anchor "), "{detail}");
        assert!(
            !profile.config().exists(),
            "a refused binding must not be saved"
        );
    }

    #[test]
    fn environment_use_refuses_a_network_id_that_disagrees_with_the_advertised_identity() {
        let emulator = Emulator::start();
        let profile = Profile::new("use-network-mismatch");
        let anchor_file = profile.write("published.anchor", &emulator_trust_anchor());
        let output = profile.run(&[
            "--json",
            "environment",
            "use",
            "emulator",
            "--endpoint",
            emulator.endpoint(),
            "--network-id",
            "403",
            "--sequencer-trust-anchor-file",
            &anchor_file.display().to_string(),
        ]);
        let detail = assert_refused(&output, "network_id_mismatch");
        assert!(detail.contains("--network-id 403"), "{detail}");
        assert!(detail.contains("402"), "{detail}");
        assert!(
            !profile.config().exists(),
            "a refused binding must not be saved"
        );
    }

    #[test]
    fn environment_use_names_each_missing_bootstrap_input() {
        let profile = Profile::new("use-missing-inputs");
        let anchor_file = profile.write("published.anchor", &other_anchor());
        let anchor_file = anchor_file.display().to_string();
        let endpoint = "http://127.0.0.1:9";

        let detail = assert_refused(
            &profile.run(&[
                "--json",
                "environment",
                "use",
                "emulator",
                "--endpoint",
                endpoint,
                "--network-id",
                "402",
            ]),
            "environment_input_missing",
        );
        assert!(
            detail.starts_with(
                "environment_input_missing: --sequencer-trust-anchor-file is required"
            ),
            "{detail}"
        );

        let detail = assert_refused(
            &profile.run(&[
                "--json",
                "environment",
                "use",
                "emulator",
                "--sequencer-trust-anchor-file",
                &anchor_file,
            ]),
            "environment_input_missing",
        );
        assert!(
            detail
                .starts_with("environment_input_missing: --endpoint and --network-id are required"),
            "{detail}"
        );

        let detail = assert_refused(
            &profile.run(&[
                "--json",
                "environment",
                "use",
                "emulator",
                "--endpoint",
                endpoint,
                "--sequencer-trust-anchor-file",
                &anchor_file,
            ]),
            "environment_input_missing",
        );
        assert!(
            detail.starts_with("environment_input_missing: --network-id is required"),
            "{detail}"
        );

        assert_refused(
            &profile.run(&[
                "--json",
                "environment",
                "use",
                "emulator",
                "--endpoint",
                endpoint,
                "--network-id",
                "402",
                "--sequencer-trust-anchor",
                &other_anchor(),
                "--sequencer-trust-anchor-file",
                &anchor_file,
            ]),
            "sequencer_trust_anchor_conflict",
        );

        assert_refused(
            &profile.run(&["--json", "environment", "use", "emulator"]),
            "sequencer_trust_anchor_unbound",
        );
        assert!(
            !profile.config().exists(),
            "a refused selection must not be saved"
        );
    }

    #[test]
    fn environment_use_refuses_an_empty_or_malformed_anchor() {
        let profile = Profile::new("use-anchor-content");
        let empty = profile.write("empty.anchor", "");
        let blank = profile.write("blank.anchor", " \n\t\n");
        let malformed = profile.write("malformed.anchor", "zz");
        let missing = profile.root.join("absent.anchor");
        let cases: [(&str, &Path, &str); 4] = [
            (
                "--sequencer-trust-anchor-file",
                &empty,
                "sequencer_trust_anchor_empty",
            ),
            (
                "--sequencer-trust-anchor-file",
                &blank,
                "sequencer_trust_anchor_empty",
            ),
            (
                "--sequencer-trust-anchor-file",
                &malformed,
                "sequencer_trust_anchor_malformed",
            ),
            (
                "--sequencer-trust-anchor-file",
                &missing,
                "sequencer_trust_anchor_unreadable",
            ),
        ];
        for (flag, path, code) in cases {
            let path = path.display().to_string();
            let detail = assert_refused(
                &profile.run(&[
                    "--json",
                    "environment",
                    "use",
                    "emulator",
                    "--endpoint",
                    "http://127.0.0.1:9",
                    "--network-id",
                    "402",
                    flag,
                    &path,
                ]),
                code,
            );
            assert!(detail.contains(flag) || detail.contains(&path), "{detail}");
        }
        let detail = assert_refused(
            &profile.run(&[
                "--json",
                "environment",
                "use",
                "emulator",
                "--endpoint",
                "http://127.0.0.1:9",
                "--network-id",
                "402",
                "--sequencer-trust-anchor",
                "",
            ]),
            "sequencer_trust_anchor_empty",
        );
        assert!(detail.contains("--sequencer-trust-anchor "), "{detail}");
        assert!(
            !profile.config().exists(),
            "a refused binding must not be saved"
        );
    }

    #[test]
    fn clean_bootstrap() {
        let sequence = published_bootstrap_sequence();
        let endpoint = flag_value(&sequence, "--endpoint");
        let network_id = match flag_value(&sequence, "--network-id").parse::<u64>() {
            Ok(network_id) => network_id,
            Err(error) => panic!("the published network id should be numeric: {error}"),
        };
        let profile = Profile::new("clean-bootstrap");
        let bin = profile.root.join("bin");
        if let Err(error) = fs::create_dir_all(&bin) {
            panic!("{} should be creatable: {error}", bin.display());
        }
        if let Err(error) = std::os::unix::fs::symlink(LAYERX, bin.join("layerx")) {
            panic!("layerx should be linkable onto PATH: {error}");
        }
        let pid_file = profile.root.join("emulator.pid");
        let transcript_path = profile.root.join("transcript.log");
        let script = format!(
            "set -euo pipefail\ntrap 'jobs -p >\"{}\"' EXIT\n{sequence}\n",
            pid_file.display()
        );
        let path = format!(
            "{}:{}",
            bin.display(),
            std::env::var("PATH").unwrap_or_default()
        );
        let stdout = match fs::File::create(&transcript_path) {
            Ok(file) => file,
            Err(error) => panic!("transcript should be creatable: {error}"),
        };
        let stderr = match stdout.try_clone() {
            Ok(file) => file,
            Err(error) => panic!("transcript handle should be cloneable: {error}"),
        };
        let started = Instant::now();
        let status = Command::new("bash")
            .arg("-c")
            .arg(&script)
            .env_clear()
            .env("HOME", &profile.root)
            .env("PATH", &path)
            .stdin(Stdio::null())
            .stdout(Stdio::from(stdout))
            .stderr(Stdio::from(stderr))
            .status();
        let elapsed = started.elapsed();
        let _emulator = BackgroundProcesses::from_pid_file(&pid_file);
        let transcript = fs::read(&transcript_path).unwrap_or_default();
        let status = match status {
            Ok(status) => status,
            Err(error) => panic!("bash should run the published sequence: {error}"),
        };
        assert!(
            status.success(),
            "the published bootstrap sequence should succeed: {}",
            String::from_utf8_lossy(&transcript)
        );

        let emulator_directory = profile.root.join(".config").join("layerx").join("emulator");
        let seed_file = emulator_directory.join("sequencer.seed");
        let anchor_file = emulator_directory.join("sequencer.anchor");
        assert_owner_only(&emulator_directory, 0o700);
        assert_owner_only(&seed_file, 0o600);
        let (seed_hex, seed_bytes) = read_seed(&seed_file);
        let anchor = read_trimmed(&anchor_file);
        assert_eq!(anchor, derived_anchor(&seed_bytes));
        assert_no_seed_material(
            "bootstrap transcript",
            &[&transcript],
            &seed_hex,
            &seed_bytes,
        );
        assert_emulator_reachable(&endpoint, network_id, &anchor);
        assert_current_environment(&profile.root, &endpoint, network_id, &anchor);
        println!("clean_bootstrap elapsed_ms={}", elapsed.as_millis());
    }
}
