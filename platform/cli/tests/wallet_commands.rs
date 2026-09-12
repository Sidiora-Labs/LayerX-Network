mod common;

use common::{envelope, error_envelope, Cli, Emulator};

#[test]
fn real_wallet_creation_listing_and_balance() {
    let cli = Cli::new();
    let emulator = Emulator::start();
    assert!(cli.bind_emulator(emulator.endpoint()).status.success());
    let output = cli.run(&[
        "--json",
        "wallet",
        "create",
        "alice",
        "--did",
        "did:layerx:alice",
    ]);
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let result = envelope(&output);
    assert_eq!(result["kind"], "wallet.created");
    assert_eq!(result["data"]["did"], "did:layerx:alice");
    let listed = cli.run(&["--json", "wallet", "list"]);
    assert!(listed.status.success());
    assert_eq!(envelope(&listed)["data"][0]["did"], "did:layerx:alice");
    let balances = cli.run(&["--json", "wallet", "balance"]);
    assert!(
        balances.status.success(),
        "{}",
        String::from_utf8_lossy(&balances.stderr)
    );
    assert_eq!(
        envelope(&balances)["data"]["accounts"][0]["name"],
        "agent:did:layerx:alice:main"
    );
    let refusal = cli.run(&[
        "--json",
        "wallet",
        "send",
        "--to",
        "did:layerx:bob",
        "--asset",
        &format!("01{}", "00".repeat(31)),
        "--amount",
        "1",
    ]);
    assert!(!refusal.status.success());
    assert_eq!(
        error_envelope(&refusal)["error"]["code"],
        "identity_sequence_unavailable"
    );
    let balances_after = cli.run(&["--json", "wallet", "balance"]);
    assert_eq!(
        envelope(&balances_after)["data"],
        envelope(&balances)["data"]
    );
}

#[test]
fn wallet_import_uses_real_credential_storage() {
    let cli = Cli::new();
    let result = cli.run_with_stdin(
        &[
            "--json",
            "wallet",
            "import",
            "imported",
            "--did",
            "did:layerx:imported",
        ],
        &"11".repeat(32),
    );
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
    assert_eq!(envelope(&result)["data"]["did"], "did:layerx:imported");
    let output = cli.run(&["--json", "wallet", "list"]);
    assert!(output.status.success());
    assert!(!String::from_utf8_lossy(&output.stdout).contains(&"11".repeat(32)));
}

#[test]
fn unavailable_rpc_asset_methods_fail_closed() {
    let cli = Cli::new();
    let output = cli.run(&["--json", "--rpc", "http://127.0.0.1:1/rpc", "token", "list"]);
    assert!(!output.status.success());
    assert_eq!(
        error_envelope(&output)["error"]["code"],
        "rpc_method_unavailable"
    );
}

#[test]
fn unpublished_registration_and_history_preserve_local_keys() {
    let cli = Cli::new();
    let before = cli.run(&["--json", "wallet", "list"]);
    assert!(before.status.success());
    let create = cli.run(&[
        "--json",
        "--rpc",
        "http://127.0.0.1:1/rpc",
        "wallet",
        "create",
        "unpublished",
        "--did",
        "did:layerx:unpublished",
    ]);
    assert!(!create.status.success());
    assert_eq!(
        error_envelope(&create)["error"]["code"],
        "wallet_registration_unavailable"
    );
    let history = cli.run(&[
        "--json",
        "wallet",
        "history",
        "--did",
        "did:layerx:unpublished",
    ]);
    assert!(!history.status.success());
    assert_eq!(
        error_envelope(&history)["error"]["code"],
        "wallet_history_unavailable"
    );
    let after = cli.run(&["--json", "wallet", "list"]);
    assert!(after.status.success());
    assert_eq!(envelope(&before)["data"], envelope(&after)["data"]);
}
