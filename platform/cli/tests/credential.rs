//! Secrets live in the credential store, never in plaintext configuration.
//!
//! These tests exercise the real key and token management commands and assert
//! that private key seeds and API tokens are accepted by the credential store
//! yet never appear in the on-disk configuration file or on any output stream.

mod common;

use common::{envelope, error_envelope, string_field, Cli};
use ed25519_dalek::SigningKey;
use serde_json::Value;

const SEED_HEX: &str = "1111111111111111111111111111111111111111111111111111111111111111";
const TOKEN: &str = "layerx-testnet-token-abcdef0123456789";

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}

fn expected_public_key() -> String {
    let mut seed = [0_u8; 32];
    for byte in &mut seed {
        *byte = 0x11;
    }
    let signing = SigningKey::from_bytes(&seed);
    hex(&signing.verifying_key().to_bytes())
}

#[test]
fn imported_seed_is_never_written_to_configuration() {
    let cli = Cli::new();
    let output = cli.run_with_stdin(&["--json", "key", "import", "alpha"], SEED_HEX);
    assert!(
        output.status.success(),
        "import should succeed: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    let value = envelope(&output);
    assert_eq!(string_field(&value, "/kind"), "key.imported");
    let public_key = expected_public_key();
    assert_eq!(string_field(&value, "/data/public_key"), public_key);
    assert_eq!(
        string_field(&value, "/data/did"),
        format!("did:layerx:{public_key}")
    );

    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        !stdout.contains(SEED_HEX),
        "the private seed must never be echoed to standard output"
    );

    let config = cli.config_contents();
    assert!(
        !config.contains(SEED_HEX),
        "the private seed must never be written to plaintext configuration"
    );
    let parsed: Value = match serde_json::from_str(&config) {
        Ok(value) => value,
        Err(error) => panic!("configuration should be JSON: {error}; config={config}"),
    };
    let metadata = match parsed.pointer("/keys/alpha") {
        Some(value) => value,
        None => panic!("configuration should record key metadata: {config}"),
    };
    let object = match metadata.as_object() {
        Some(object) => object,
        None => panic!("key metadata should be an object: {metadata}"),
    };
    let mut fields: Vec<&String> = object.keys().collect();
    fields.sort();
    assert_eq!(
        fields,
        vec!["did", "public_key"],
        "configuration must carry only public key metadata"
    );
}

#[cfg(unix)]
#[test]
fn configuration_file_is_owner_only() {
    use std::os::unix::fs::PermissionsExt as _;

    let cli = Cli::new();
    let output = cli.run(&["--json", "key", "create", "alpha"]);
    assert!(output.status.success());

    let metadata = match std::fs::metadata(cli.config_path()) {
        Ok(metadata) => metadata,
        Err(error) => panic!("configuration should exist after key creation: {error}"),
    };
    assert_eq!(
        metadata.permissions().mode() & 0o777,
        0o600,
        "configuration must be readable only by its owner"
    );
}

#[test]
fn created_key_reports_its_secret_storage_and_hides_the_secret() {
    let cli = Cli::new();
    let created = cli.run(&["--json", "key", "create", "alpha"]);
    assert!(created.status.success());
    let public_key = string_field(&envelope(&created), "/data/public_key").to_owned();

    let shown = cli.run(&["--json", "key", "show", "alpha"]);
    assert!(shown.status.success());
    let value = envelope(&shown);
    assert_eq!(
        string_field(&value, "/data/secret_storage"),
        "operating-system-credential-store"
    );
    assert_eq!(string_field(&value, "/data/public_key"), public_key);
    let rendered = String::from_utf8_lossy(&shown.stdout);
    assert!(
        !rendered.contains("seed") && !rendered.contains("private"),
        "key metadata must not surface secret material"
    );
}

#[test]
fn api_token_is_never_written_to_configuration() {
    let cli = Cli::new();
    // Materialise a configuration file first so the assertion inspects a real file.
    assert!(cli.run(&["--json", "key", "create", "alpha"]).status.success());

    let output = cli.run_with_stdin(
        &["--json", "auth", "set", "--environment", "testnet"],
        TOKEN,
    );
    assert!(
        output.status.success(),
        "auth set should succeed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let value = envelope(&output);
    assert_eq!(string_field(&value, "/kind"), "auth.saved");
    assert_eq!(
        string_field(&value, "/data/secret_storage"),
        "operating-system-credential-store"
    );

    assert!(
        !String::from_utf8_lossy(&output.stdout).contains(TOKEN),
        "the API token must never be echoed to standard output"
    );
    assert!(
        !cli.config_contents().contains(TOKEN),
        "the API token must never be written to plaintext configuration"
    );
}

#[test]
fn duplicate_key_names_are_refused_with_a_machine_readable_error() {
    let cli = Cli::new();
    assert!(cli.run(&["--json", "key", "create", "alpha"]).status.success());

    let output = cli.run(&["--json", "key", "create", "alpha"]);
    assert!(!output.status.success());
    let value = error_envelope(&output);
    assert_eq!(value.pointer("/ok").and_then(Value::as_bool), Some(false));
    assert_eq!(
        value.pointer("/error/code").and_then(Value::as_str),
        Some("command_failed")
    );
}
