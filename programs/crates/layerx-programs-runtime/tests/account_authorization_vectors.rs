//! Shared native account-authorization vectors against the Rust reference.

use layerx_programs_runtime::transfer::verify_authorization_root;
use serde::Deserialize;
use std::fs;
use std::path::PathBuf;

#[derive(Debug, Deserialize)]
struct AuthorizationVector {
    name: String,
    encoded: String,
    root: String,
    accept: bool,
}

fn hex_bytes(name: &str, value: &str) -> Vec<u8> {
    hex::decode(value).unwrap_or_else(|error| panic!("{name}: {error:?}"))
}

#[test]
fn shared_native_account_authorization_vectors() {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../fixtures/pay5/account-authorization-vectors.json");
    let raw =
        fs::read_to_string(&path).unwrap_or_else(|error| panic!("{}: {error}", path.display()));
    let vectors: Vec<AuthorizationVector> =
        serde_json::from_str(&raw).unwrap_or_else(|error| panic!("{}: {error}", path.display()));
    assert_eq!(vectors.len(), 515, "{}", path.display());
    for vector in vectors {
        let encoded = hex_bytes(&vector.name, &vector.encoded);
        let root = hex_bytes(&vector.name, &vector.root);
        let root: [u8; 32] = root
            .try_into()
            .unwrap_or_else(|bytes: Vec<u8>| panic!("{}: root len {}", vector.name, bytes.len()));
        let accepted = verify_authorization_root(&encoded, root).is_ok();
        assert_eq!(accepted, vector.accept, "{}", vector.name);
    }
}
