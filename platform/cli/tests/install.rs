use std::path::PathBuf;
use std::process::{Command, Output};
use std::sync::atomic::{AtomicU32, Ordering};

static SEQUENCE: AtomicU32 = AtomicU32::new(0);

fn isolated(label: &str) -> (PathBuf, PathBuf) {
    let sequence = SEQUENCE.fetch_add(1, Ordering::Relaxed);
    let root = std::env::temp_dir().join(format!(
        "layerx-install-{label}-{}-{sequence}",
        std::process::id()
    ));
    (root.join("config.json"), root)
}

fn run(label: &str, arguments: &[&str]) -> Output {
    let (config, root) = isolated(label);
    let output = Command::new(env!("CARGO_BIN_EXE_layerx"))
        .args(arguments)
        .env("LAYERX_CONFIG", config)
        .env("LAYERX_INSTALL_ROOT", &root)
        .env_remove("LAYERX_CREDENTIAL_STORE")
        .output()
        .unwrap_or_else(|error| panic!("real layerx executable should start: {error}"));
    let _ = std::fs::remove_dir_all(root);
    output
}

fn error(output: &Output) -> String {
    assert!(!output.status.success());
    String::from_utf8_lossy(&output.stderr).into_owned()
}

#[test]
fn production_installation_never_falls_back_to_emulator_routes() {
    let source = "11".repeat(32);
    let asset = "22".repeat(32);
    let output = run(
        "emulator",
        &[
            "--json",
            "install",
            "mcp",
            "--host",
            "layerx",
            "--source-account",
            &source,
            "--asset",
            &asset,
        ],
    );
    assert!(error(&output).contains("hosted testnet or production gateway"));
}

#[test]
fn undocumented_runtime_alias_is_rejected_before_credentials_are_touched() {
    let source = "11".repeat(32);
    let asset = "22".repeat(32);
    let output = run(
        "host",
        &[
            "--json",
            "install",
            "mcp",
            "--host",
            "claude",
            "--source-account",
            &source,
            "--asset",
            &asset,
        ],
    );
    assert!(error(&output).contains("claude-code"));
}

#[test]
fn payment_installation_requires_a_fixed_real_source_and_asset() {
    let output = run(
        "binding",
        &["--json", "install", "a2a", "--environment", "testnet"],
    );
    assert!(error(&output).contains("--source-account"));
}
