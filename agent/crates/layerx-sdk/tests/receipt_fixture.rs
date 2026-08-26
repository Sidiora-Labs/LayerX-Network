use std::fs;
use std::path::Path;

use layerx_proof::receipt::{canonical_protocol_facts, AuthorizedBatch, ReceiptCheck};
use layerx_sdk::production::verify_receipt;
use layerx_types::verify::VerificationLevel;

fn fixture_json() -> String {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../../platform/sdk/conformance/fixtures/receipt-positive-v1.json");
    fs::read_to_string(&path)
        .unwrap_or_else(|error| panic!("read {}: {error}", path.display()))
}

fn string_field(json: &str, key: &str) -> String {
    let marker = format!("\"{key}\": \"");
    let start = json
        .find(&marker)
        .unwrap_or_else(|| panic!("fixture field {key} missing"))
        + marker.len();
    let rest = &json[start..];
    let end = rest
        .find('"')
        .unwrap_or_else(|| panic!("fixture field {key} unterminated"));
    rest[..end].to_string()
}

fn number_field(json: &str, key: &str) -> u64 {
    let marker = format!("\"{key}\": ");
    let start = json
        .find(&marker)
        .unwrap_or_else(|| panic!("fixture field {key} missing"))
        + marker.len();
    let rest = &json[start..];
    let end = rest
        .find([',', '\n'])
        .unwrap_or_else(|| panic!("fixture field {key} unterminated"));
    rest[..end]
        .trim()
        .parse::<u64>()
        .unwrap_or_else(|error| panic!("fixture field {key}: {error}"))
}

fn u128_field(json: &str, key: &str) -> u128 {
    string_field(json, key)
        .parse::<u128>()
        .unwrap_or_else(|error| panic!("fixture field {key}: {error}"))
}

fn hex_bytes(value: &str) -> Vec<u8> {
    assert!(value.len() % 2 == 0, "odd hex length");
    (0..value.len() / 2)
        .map(|index| {
            u8::from_str_radix(&value[2 * index..2 * index + 2], 16)
                .unwrap_or_else(|error| panic!("fixture hex: {error}"))
        })
        .collect()
}

fn hex_32(json: &str, key: &str) -> [u8; 32] {
    hex_bytes(&string_field(json, key))
        .as_slice()
        .try_into()
        .unwrap_or_else(|error| panic!("fixture field {key} is not 32 bytes: {error}"))
}

fn authorised_batch(json: &str) -> AuthorizedBatch {
    AuthorizedBatch::new(
        hex_32(json, "batch_id_hex"),
        hex_32(json, "asset_hex"),
        hex_32(json, "previous_state_root_hex"),
        hex_32(json, "resulting_state_root_hex"),
        hex_32(json, "sequencer_public_key_hex"),
    )
}

#[test]
fn core_fixture_receipt_verifies_positively() {
    let json = fixture_json();
    let canonical = hex_bytes(&string_field(&json, "canonical_receipt_hex"));
    let verified = verify_receipt(&canonical, &authorised_batch(&json))
        .unwrap_or_else(|failure| panic!("canonical core receipt refused: {failure:?}"));
    assert_eq!(verified.level(), VerificationLevel::SEQUENCER_SIGNED);
    assert_eq!(verified.canonical_bytes(), canonical.as_slice());
    assert_eq!(
        verified.evidence().receipt_digest(),
        Some(hex_32(&json, "receipt_digest_hex"))
    );
    let protocol = verified
        .receipt()
        .protocol()
        .unwrap_or_else(|| panic!("verified receipt lost its protocol body"));
    assert_eq!(i64::from(protocol.result_code()), 0);
    assert_eq!(u64::from(protocol.protocol_version()), number_field(&json, "protocol_version"));
    assert_eq!(u64::from(protocol.operation()), number_field(&json, "operation"));
    assert_eq!(u64::from(protocol.module_id()), number_field(&json, "module_id"));
    assert_eq!(protocol.global_sequence(), number_field(&json, "global_sequence"));
    assert_eq!(protocol.timestamp(), number_field(&json, "timestamp_ms"));
    assert_eq!(protocol.amount(), u128_field(&json, "amount"));
    assert_eq!(protocol.fee_charged(), u128_field(&json, "fee_charged"));
    assert_eq!(protocol.debit_balance_before(), u128_field(&json, "from_balance_before"));
    assert_eq!(protocol.debit_balance_after(), u128_field(&json, "from_balance_after"));
    assert_eq!(protocol.credit_balance_before(), u128_field(&json, "to_balance_before"));
    assert_eq!(protocol.credit_balance_after(), u128_field(&json, "to_balance_after"));
    assert_eq!(protocol.activity_id(), hex_32(&json, "activity_id_hex"));
    assert_eq!(protocol.from(), hex_32(&json, "from_hex"));
    assert_eq!(protocol.to(), hex_32(&json, "to_hex"));
    assert_eq!(protocol.batch_id(), hex_32(&json, "batch_id_hex"));
    assert_eq!(protocol.asset(), hex_32(&json, "asset_hex"));
    assert_eq!(protocol.previous_state_root(), hex_32(&json, "previous_state_root_hex"));
    assert_eq!(protocol.resulting_state_root(), hex_32(&json, "resulting_state_root_hex"));
    let facts = canonical_protocol_facts(&canonical)
        .unwrap_or_else(|failure| panic!("canonical facts refused: {failure:?}"));
    assert_eq!(facts.result_code(), 0);
    assert_eq!(facts.amount(), u128_field(&json, "amount"));
    assert_eq!(facts.fee_charged(), u128_field(&json, "fee_charged"));
    assert_eq!(facts.asset(), hex_32(&json, "asset_hex"));
}

#[test]
fn core_fixture_receipt_byte_flip_fails() {
    let json = fixture_json();
    let mut mutated = hex_bytes(&string_field(&json, "canonical_receipt_hex"));
    let last = mutated.len() - 1;
    mutated[last] ^= 0x01;
    let failure = verify_receipt(&mutated, &authorised_batch(&json))
        .map(|_| ())
        .expect_err("mutated receipt verified; a flipped signature byte must fail");
    assert_eq!(failure.check, ReceiptCheck::SequencerSignature);
}
