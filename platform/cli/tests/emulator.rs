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

    let output = cli.run(&[
        "--json",
        "account",
        "get",
        "--did",
        "did:layerx:absent",
    ]);
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
