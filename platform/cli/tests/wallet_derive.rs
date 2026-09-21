mod common;

use std::os::unix::fs::PermissionsExt as _;

use common::{envelope, error_envelope, Cli};
use serde_json::Value;

const FIXTURE: &str = include_str!("../../sdk/conformance/fixtures/account-derivation-v1.json");

fn fixture() -> Value {
    serde_json::from_str(FIXTURE).expect("the shared derivation fixture is JSON")
}

#[test]
fn derive_reads_the_phrase_from_stdin_and_matches_the_shared_fixture() {
    let cli = Cli::new();
    let fixture = fixture();
    for vector in fixture["mnemonic_vectors"].as_array().expect("vectors") {
        if vector["passphrase"] != "" {
            continue;
        }
        for account in vector["accounts"].as_array().expect("accounts") {
            let index = account["index"].as_u64().expect("index").to_string();
            let output = cli.run_with_stdin(
                &["--json", "wallet", "derive", "--index", &index],
                &format!("{}\n", vector["mnemonic"].as_str().expect("mnemonic")),
            );
            assert!(
                output.status.success(),
                "{}",
                String::from_utf8_lossy(&output.stderr)
            );
            let result = envelope(&output);
            assert_eq!(result["kind"], "wallet.derived");
            assert_eq!(result["data"]["evm_address"], account["evm_address"]);
            assert_eq!(result["data"]["did"], account["did"]);
            assert_eq!(result["data"]["evm_path"], account["evm_path"]);
            assert_eq!(result["data"]["layerx_path"], account["layerx_path"]);
            assert!(result["data"].get("private_keys_file").is_none());
        }
    }
}

#[test]
fn derive_reads_files_and_exports_keys_only_to_an_owner_only_file() {
    let cli = Cli::new();
    let fixture = fixture();
    let vector = &fixture["mnemonic_vectors"][1];
    let account = &vector["accounts"][1];
    let directory = tempfile::tempdir().expect("temporary directory");
    let phrase = directory.path().join("phrase");
    let passphrase = directory.path().join("passphrase");
    let keys = directory.path().join("keys.json");
    std::fs::write(&phrase, vector["mnemonic"].as_str().expect("mnemonic")).expect("phrase");
    std::fs::write(
        &passphrase,
        format!("{}\n", vector["passphrase"].as_str().expect("passphrase")),
    )
    .expect("passphrase");
    let arguments = [
        "--json",
        "wallet",
        "derive",
        "--index",
        "1",
        "--mnemonic-file",
        phrase.to_str().expect("path"),
        "--passphrase-file",
        passphrase.to_str().expect("path"),
        "--export-private-keys",
        keys.to_str().expect("path"),
    ];
    let output = cli.run(&arguments);
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let result = envelope(&output);
    assert_eq!(result["data"]["evm_address"], account["evm_address"]);
    assert_eq!(result["data"]["did"], account["did"]);
    let mode = std::fs::metadata(&keys)
        .expect("key file")
        .permissions()
        .mode();
    assert_eq!(mode & 0o777, 0o600);
    let exported: Value =
        serde_json::from_str(&std::fs::read_to_string(&keys).expect("key file")).expect("JSON");
    assert_eq!(exported["did"], account["did"]);
    let seed = exported["layerx_seed"].as_str().expect("seed");
    let evm = exported["evm_private_key"].as_str().expect("EVM key");
    assert_eq!(seed.len(), 64);
    assert_eq!(evm.len(), 64);
    let printed = format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(!printed.contains(seed));
    assert!(!printed.contains(evm));

    let again = cli.run(&arguments);
    assert!(!again.status.success());
    assert_eq!(
        error_envelope(&again)["error"]["code"],
        "private_key_export_refused"
    );
}

#[test]
fn derive_refuses_bad_phrases_argv_phrases_and_bind_without_an_endpoint() {
    let cli = Cli::new();
    let bad = cli.run_with_stdin(&["--json", "wallet", "derive"], "abandon abandon abandon\n");
    assert!(!bad.status.success());
    assert_eq!(error_envelope(&bad)["error"]["code"], "invalid_mnemonic");

    let argv = cli.run_with_stdin(
        &["--json", "wallet", "derive", "abandon abandon abandon"],
        "",
    );
    assert!(!argv.status.success());

    let unbound = cli.run_with_stdin(
        &["--json", "wallet", "derive", "--bind"],
        "abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon about\n",
    );
    assert!(!unbound.status.success());
    assert_eq!(
        error_envelope(&unbound)["error"]["code"],
        "bind_requires_rpc"
    );
}
