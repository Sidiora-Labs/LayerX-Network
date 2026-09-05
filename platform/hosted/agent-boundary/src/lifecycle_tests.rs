use super::{artifacts, validate_program_lifecycle};

fn fixture(name: &str) -> Vec<u8> {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../../platform/sdk/conformance/fixtures")
        .join(format!("native-program-{name}-v3.json"));
    let source = std::fs::read_to_string(&path)
        .unwrap_or_else(|error| panic!("{}: {error}", path.display()));
    let document: serde_json::Value =
        serde_json::from_str(&source).unwrap_or_else(|error| panic!("fixture JSON: {error}"));
    let payload = document["payload_hex"]
        .as_str()
        .unwrap_or_else(|| panic!("fixture omits payload_hex"));
    artifacts::canonical_hex(payload, super::MAX_ACTIVITY_BYTES)
        .unwrap_or_else(|error| panic!("fixture hexadecimal: {error}"))
}

#[test]
fn native_c_lifecycle_vectors_are_admitted() {
    for (name, ordinal) in [
        ("deploy", 1),
        ("upgrade", 2),
        ("wind-down-route", 7),
        ("wind-down-deprecate", 7),
        ("wind-down-tombstone", 7),
        ("wind-down-exit", 7),
    ] {
        let payload = fixture(name);
        assert!(
            validate_program_lifecycle(ordinal, &payload).is_ok(),
            "{name}"
        );
        let mut trailing = payload;
        trailing.push(0);
        assert!(
            validate_program_lifecycle(ordinal, &trailing).is_err(),
            "{name} trailing byte"
        );
    }
}

#[test]
fn lifecycle_code_hash_substitution_refuses_before_submit() {
    for (name, ordinal) in [("deploy", 1), ("upgrade", 2)] {
        let mut payload = fixture(name);
        payload[68] ^= 1;
        let refused = validate_program_lifecycle(ordinal, &payload)
            .err()
            .unwrap_or_else(|| panic!("{name} substituted hash was accepted"));
        assert_eq!(refused.status, 400);
        assert!(refused.body.contains("program_payload_hash_mismatch"));
        payload = fixture(name);
        payload[35] = 1;
        let refused = validate_program_lifecycle(ordinal, &payload)
            .err()
            .unwrap_or_else(|| panic!("{name} reserved field was accepted"));
        assert_eq!(refused.status, 400);
        assert!(refused.body.contains("malformed_program_"));
    }
}

#[test]
fn idempotency_receipts_require_signed_activity_and_sequencer_bindings() {
    let document: serde_json::Value = serde_json::from_str(include_str!(
        "../../../sdk/conformance/fixtures/receipt-programs-positive-v3.json"
    ))
    .unwrap_or_else(|error| panic!("receipt fixture JSON: {error}"));
    let receipt = document["canonical_receipt_hex"]
        .as_str()
        .unwrap_or_else(|| panic!("fixture omits receipt"));
    let signer = document["authorized_batch"]["sequencer_public_key_hex"]
        .as_str()
        .and_then(super::parse_hex32)
        .unwrap_or_else(|| panic!("fixture omits sequencer key"));
    let bytes = artifacts::canonical_hex(receipt, super::MAX_ACTIVITY_BYTES)
        .unwrap_or_else(|error| panic!("receipt hexadecimal: {error}"));
    let decoded = layerx_wire::receipt::decode(&bytes)
        .unwrap_or_else(|error| panic!("receipt decoding: {error:?}"));
    let activity_id = super::hex(
        &decoded
            .protocol()
            .unwrap_or_else(|| panic!("fixture omits protocol receipt"))
            .activity_id(),
    );
    let response = serde_json::json!({"activity_id":activity_id,"receipt":receipt}).to_string();
    assert!(super::verify_idempotency_receipt(&response, signer, Some(&activity_id)).is_ok());
    assert!(super::verify_idempotency_receipt(&response, signer, Some(&"00".repeat(32))).is_err());
    let mut wrong_signer = signer;
    wrong_signer[0] ^= 1;
    assert!(
        super::verify_idempotency_receipt(&response, wrong_signer, Some(&activity_id)).is_err()
    );
    let mut corrupt = bytes;
    let last = corrupt
        .last_mut()
        .unwrap_or_else(|| panic!("empty receipt"));
    *last ^= 1;
    let response =
        serde_json::json!({"activity_id":activity_id,"receipt":super::hex(&corrupt)}).to_string();
    assert!(super::verify_idempotency_receipt(&response, signer, Some(&activity_id)).is_err());
}
