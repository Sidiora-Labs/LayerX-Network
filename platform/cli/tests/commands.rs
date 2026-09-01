//! Machine-readable output and validation coverage for every CLI command.
//!
//! These tests do not require the emulator's higher-level gateway routes. They
//! prove that each command emits the `ok`/`kind`/`message`/`data` envelope under
//! `--json`, renders a human presentation without it, and refuses malformed
//! input with the machine-readable error envelope instead of a panic.

mod common;

use std::path::PathBuf;
use std::sync::atomic::{AtomicU32, Ordering};

use common::{envelope, error_envelope, string_field, Cli};
use serde_json::Value;

static SEQUENCE: AtomicU32 = AtomicU32::new(0);

fn scratch_dir(prefix: &str) -> PathBuf {
    let sequence = SEQUENCE.fetch_add(1, Ordering::Relaxed);
    std::env::temp_dir().join(format!(
        "layerx-cli-cmd-{prefix}-{}-{sequence}",
        std::process::id()
    ))
}

fn write_scratch_file(prefix: &str, contents: &str) -> PathBuf {
    let path = scratch_dir(prefix).with_extension("dat");
    if let Err(error) = std::fs::write(&path, contents) {
        panic!("scratch file should be writable: {error}");
    }
    path
}

fn assert_success_envelope(output: &std::process::Output, kind: &str) -> Value {
    assert!(
        output.status.success(),
        "command should succeed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let value = envelope(output);
    assert_eq!(value.pointer("/ok").and_then(Value::as_bool), Some(true));
    assert_eq!(string_field(&value, "/kind"), kind);
    assert!(value.pointer("/message").and_then(Value::as_str).is_some());
    value
}

fn assert_command_failed(output: &std::process::Output) {
    assert!(!output.status.success());
    let value = error_envelope(output);
    assert_eq!(value.pointer("/ok").and_then(Value::as_bool), Some(false));
    assert_eq!(
        value.pointer("/error/code").and_then(Value::as_str),
        Some("command_failed")
    );
}

#[test]
fn new_scaffolds_a_deterministic_program_project() {
    let cli = Cli::new();
    let parent = scratch_dir("scaffold");
    if let Err(error) = std::fs::create_dir_all(&parent) {
        panic!("scaffold parent should be creatable: {error}");
    }
    let parent_text = parent.to_string_lossy().into_owned();
    let output = cli.run(&["--json", "new", "counter", "--directory", &parent_text]);
    let value = assert_success_envelope(&output, "project.created");
    assert_eq!(string_field(&value, "/data/kind"), "rust-program");

    let project = parent.join("counter");
    for file in ["Cargo.toml", "LayerX.toml", "src/lib.rs", ".gitignore"] {
        assert!(project.join(file).exists(), "scaffold should create {file}");
    }
    let _ = std::fs::remove_dir_all(&parent);
}

#[test]
fn environment_list_reports_the_default_profile() {
    let cli = Cli::new();
    let output = cli.run(&["--json", "environment", "list"]);
    let value = assert_success_envelope(&output, "environment.list");
    let profiles = match value.pointer("/data").and_then(Value::as_array) {
        Some(profiles) => profiles,
        None => panic!("environment list should be an array: {value}"),
    };
    assert!(profiles
        .iter()
        .any(|profile| profile.get("name").and_then(Value::as_str) == Some("emulator")));
}

#[test]
fn environment_use_requires_endpoint_and_network_together() {
    let cli = Cli::new();
    let output = cli.run(&[
        "--json",
        "environment",
        "use",
        "testnet",
        "--endpoint",
        "https://testnet.example",
    ]);
    assert_command_failed(&output);
}

#[test]
fn environment_use_rejects_unknown_environment_names() {
    let cli = Cli::new();
    let output = cli.run(&[
        "--json",
        "environment",
        "use",
        "staging",
        "--endpoint",
        "https://staging.example",
        "--network-id",
        "7",
    ]);
    assert_command_failed(&output);
}

#[test]
fn key_lifecycle_metadata_persists_in_configuration() {
    let cli = Cli::new();
    assert_success_envelope(
        &cli.run(&["--json", "key", "create", "alpha"]),
        "key.created",
    );
    assert_success_envelope(
        &cli.run(&["--json", "key", "create", "beta"]),
        "key.created",
    );

    let listed = assert_success_envelope(&cli.run(&["--json", "key", "list"]), "key.list");
    let names: Vec<&str> = match listed.pointer("/data").and_then(Value::as_array) {
        Some(entries) => entries
            .iter()
            .filter_map(|entry| entry.get("name").and_then(Value::as_str))
            .collect(),
        None => panic!("key list should be an array: {listed}"),
    };
    assert_eq!(names, vec!["alpha", "beta"]);

    assert_success_envelope(
        &cli.run(&["--json", "key", "default", "beta"]),
        "key.default",
    );
    let shown =
        assert_success_envelope(&cli.run(&["--json", "key", "show", "beta"]), "key.metadata");
    assert_eq!(
        shown.pointer("/data/default").and_then(Value::as_bool),
        Some(true)
    );
}

#[test]
fn deleting_an_unknown_key_is_refused() {
    let cli = Cli::new();
    assert_command_failed(&cli.run(&["--json", "key", "delete", "ghost"]));
}

#[test]
fn auth_status_reports_no_token_for_a_fresh_environment() {
    let cli = Cli::new();
    let output = cli.run(&["--json", "auth", "status", "--environment", "testnet"]);
    let value = assert_success_envelope(&output, "auth.status");
    assert_eq!(
        value.pointer("/data/configured").and_then(Value::as_bool),
        Some(false)
    );
}

#[test]
fn payment_rejects_a_zero_amount() {
    let cli = Cli::new();
    let output = cli.run(&[
        "--json",
        "payment",
        "test",
        "--from",
        "agent:one",
        "--to",
        "agent:two",
        "--currency",
        "USD",
        "--amount",
        "0",
        "--idempotency-key",
        "idem-0000000000000000",
    ]);
    assert_command_failed(&output);
}

#[test]
fn payment_rejects_a_short_idempotency_key() {
    let cli = Cli::new();
    let output = cli.run(&[
        "--json",
        "payment",
        "test",
        "--from",
        "agent:one",
        "--to",
        "agent:two",
        "--currency",
        "USD",
        "--amount",
        "100",
        "--idempotency-key",
        "short",
    ]);
    assert_command_failed(&output);
}

#[test]
fn receipt_get_rejects_an_unsafe_identifier() {
    let cli = Cli::new();
    let output = cli.run(&["--json", "receipt", "get", "bad id"]);
    assert_command_failed(&output);
}

#[test]
fn receipt_verify_refuses_a_forged_receipt_locally() {
    let cli = Cli::new();
    let zeros = "0".repeat(64);
    let receipt = write_scratch_file("receipt", "00");
    let receipt_text = receipt.to_string_lossy().into_owned();
    let output = cli.run(&[
        "--json",
        "receipt",
        "verify",
        "--receipt",
        &receipt_text,
        "--batch-id",
        &zeros,
        "--asset",
        &zeros,
        "--previous-state-root",
        &zeros,
        "--resulting-state-root",
        &zeros,
        "--sequencer-public-key",
        &zeros,
    ]);
    assert_command_failed(&output);
    let _ = std::fs::remove_file(&receipt);
}

#[test]
fn program_build_reports_a_missing_manifest() {
    let cli = Cli::new();
    let missing = scratch_dir("manifest").join("Cargo.toml");
    let missing_text = missing.to_string_lossy().into_owned();
    let output = cli.run(&[
        "--json",
        "program",
        "build",
        "--manifest-path",
        &missing_text,
    ]);
    assert_command_failed(&output);
}

#[test]
fn program_deploy_reports_a_missing_artifact() {
    let cli = Cli::new();
    let missing = scratch_dir("artifact").join("program.wasm");
    let missing_text = missing.to_string_lossy().into_owned();
    let output = cli.run(&[
        "--json",
        "program",
        "deploy",
        &missing_text,
        "--idempotency-key",
        "idem-1111111111111111",
    ]);
    assert_command_failed(&output);
}

#[test]
fn program_registry_get_rejects_an_unsafe_identifier() {
    let cli = Cli::new();
    let output = cli.run(&["--json", "program", "registry", "get", "not a program"]);
    assert_command_failed(&output);
}

#[test]
fn program_registry_verify_source_rejects_a_malformed_digest() {
    let cli = Cli::new();
    let output = cli.run(&[
        "--json",
        "program",
        "registry",
        "verify-source",
        "program-1",
        "--source-uri",
        "https://example.com/source.tar.gz",
        "--source-digest",
        "not-hex",
        "--idempotency-key",
        "idem-2222222222222222",
    ]);
    assert_command_failed(&output);
}

#[test]
fn json_and_human_output_diverge_for_the_same_command() {
    let cli = Cli::new();
    let machine = cli.run(&["--json", "environment", "list"]);
    assert!(machine.status.success());
    assert!(serde_json::from_slice::<Value>(&machine.stdout).is_ok());

    let human = cli.run(&["environment", "list"]);
    assert!(human.status.success());
    assert!(
        serde_json::from_slice::<Value>(&human.stdout).is_err(),
        "human output should not be a bare JSON object"
    );
}

#[test]
fn unknown_subcommands_are_rejected() {
    let cli = Cli::new();
    let output = cli.run(&["nonsense"]);
    assert!(!output.status.success());
}
