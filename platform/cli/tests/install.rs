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
            "a2a",
            "--source-account",
            &source,
            "--asset",
            &asset,
        ],
    );
    assert!(error(&output).contains("hosted testnet or production gateway"));
}

#[test]
fn undocumented_runtime_alias_is_rejected_before_the_binding_is_read() {
    let output = run("host", &["--json", "install", "mcp", "--host", "claude"]);
    let detail = error(&output);
    assert!(detail.contains("claude-code"));
    assert!(!detail.contains("binding.json"));
}

#[test]
fn mcp_installation_takes_no_gateway_key_or_payment_flags() {
    for flag in ["--key", "--environment", "--source-account", "--asset"] {
        let output = run(
            "flags",
            &[
                "--json", "install", "mcp", "--host", "layerx", flag, "value",
            ],
        );
        assert!(error(&output).contains(flag), "{flag} was accepted");
    }
    for flag in ["--token-stdin", "--rotate"] {
        let output = run(
            "switches",
            &["--json", "install", "mcp", "--host", "layerx", flag],
        );
        assert!(error(&output).contains(flag), "{flag} was accepted");
    }
}

#[test]
fn mcp_installation_refuses_without_the_daemon_binding_document() {
    let output = run(
        "unenrolled",
        &["--json", "install", "mcp", "--host", "layerx"],
    );
    let detail = error(&output);
    assert!(detail.contains("binding.json"));
    assert!(detail.contains("agent-daemon enrolment"));
}

#[test]
fn payment_installation_requires_a_fixed_real_source_and_asset() {
    let output = run(
        "binding",
        &["--json", "install", "a2a", "--environment", "testnet"],
    );
    assert!(error(&output).contains("--source-account"));
}
