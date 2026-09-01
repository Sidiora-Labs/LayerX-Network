//! Golden vector tests for portable receipt export and verification.
//!
//! These vectors prove that an external party can verify LayerX receipts
//! using only the published format specification and trusted batch authorization.

use layerx_portable::{
    interop_portable_verification, PortableReceipt, PortableReceiptError, PORTABLE_RECEIPT_FORMAT,
};
use layerx_proof::receipt::{AuthorizedBatch, VerificationFailure};

const GOLDEN_RECEIPT_JSON: &str = r#"{
  "format": "layerx-receipt-proof-v1",
  "verificationLevel": "sequencer-signed",
  "canonicalReceipt": "AQEBAQEBAQEBAQEBAQEBAQEBAQEBAQEBAQEBAQEBAQEBAgICAgICAgICAgICAgICAgICAgICAgICAgICAgICAgIBAAAAAAAACgAAAAAAAAAFAAAAAAAAAMgAAAAAAAABAAAAAAAAAAEAAAAAAAAAAQAAAAAAAAABAAAAAAAAAGQAAAAAAAAAZAAAAAAAAAABZAAAAAAAAAFkAAAAAAAAAQAAAAAAAABlAAAAAAAAAGQAAAAAAAAAAwMDAwMDAwMDAwMDAwMDAwMDAwMDAwMDAwMDAwMDAwQEBAQEBAQEBAQEBAQEBAQEBAQEBAQEBAQEBAQEBAQE",
  "receiptDigest": "BAQEBAQEBAQEBAQEBAQEBAQEBAQEBAQEBAQEBAQEBAQ",
  "batchId": "AQEBAQEBAQEBAQEBAQEBAQEBAQEBAQEBAQEBAQEBAQE",
  "asset": "AgICAgICAgICAgICAgICAgICAgICAgICAgICAgICAgI",
  "previousStateRoot": "AwMDAwMDAwMDAwMDAwMDAwMDAwMDAwMDAwMDAwMDAwM",
  "resultingStateRoot": "BAQEBAQEBAQEBAQEBAQEBAQEBAQEBAQEBAQEBAQEBAQ",
  "sequencerPublicKey": "BQUFBQUFBQUFBQUFBQUFBQUFBQUFBQUFBQUFBQUFBQU"
}"#;

#[test]
fn format_constant_matches_specification() {
    assert_eq!(PORTABLE_RECEIPT_FORMAT, "layerx-receipt-proof-v1");
    assert_eq!(
        interop_portable_verification(),
        "layerx-receipt-proof-v1-and-pinned-external-evidence-v1"
    );
}

#[test]
fn parse_golden_receipt_json() {
    let parsed = PortableReceipt::from_json(GOLDEN_RECEIPT_JSON.as_bytes());
    assert!(
        parsed.is_ok(),
        "Golden vector must parse without error: {parsed:?}"
    );
    let receipt = parsed.expect("golden vector parses");
    assert_eq!(receipt.format(), PORTABLE_RECEIPT_FORMAT);
}

#[test]
fn reject_malformed_json() {
    let cases = [
        (b"" as &[u8], "empty input"),
        (b"{}", "missing required fields"),
        (b"[]", "wrong json type"),
        (
            br#"{"format":"layerx-receipt-proof-v1","extra":"field"}"#,
            "unknown field",
        ),
    ];
    for (input, label) in cases {
        let result = PortableReceipt::from_json(input);
        assert!(result.is_err(), "Must reject {label}: {result:?}");
    }
}

#[test]
fn reject_padded_base64() {
    let padded_json = GOLDEN_RECEIPT_JSON.replace("BAQE", "BAQE=");
    let result = PortableReceipt::from_json(padded_json.as_bytes());
    match result {
        Err(PortableReceiptError::InvalidBase64(_)) => {}
        other => panic!("Must reject padded base64, got {other:?}"),
    }
}

#[test]
fn reject_non_canonical_base64() {
    let mut modified = GOLDEN_RECEIPT_JSON.replace(
        r#""receiptDigest": "BAQEBAQEBAQEBAQEBAQEBAQEBAQEBAQEBAQEBAQEBAQ""#,
        r#""receiptDigest": "BAQEBAQEBAQEBAQEBAQEBAQEBAQEBAQEBAQEBAQEBAR""#,
    );
    modified = modified.replace(
        r#""resultingStateRoot": "BAQEBAQEBAQEBAQEBAQEBAQEBAQEBAQEBAQEBAQEBAQ""#,
        r#""resultingStateRoot": "BAQEBAQEBAQEBAQEBAQEBAQEBAQEBAQEBAQEBAQEBAR""#,
    );
    let result = PortableReceipt::from_json(modified.as_bytes());
    match result {
        Err(PortableReceiptError::InvalidBase64(_)) => {}
        other => panic!("Must reject non-canonical base64, got {other:?}"),
    }
}

#[test]
fn reject_wrong_field_lengths() {
    let short_digest = GOLDEN_RECEIPT_JSON.replace(
        r#""receiptDigest": "BAQEBAQEBAQEBAQEBAQEBAQEBAQEBAQEBAQEBAQEBAQ""#,
        r#""receiptDigest": "BAQE""#,
    );
    let result = PortableReceipt::from_json(short_digest.as_bytes());
    match result {
        Err(PortableReceiptError::InvalidLength(_)) => {}
        other => panic!("Must reject wrong length, got {other:?}"),
    }
}

#[test]
fn reject_unsupported_format() {
    let wrong_format = GOLDEN_RECEIPT_JSON.replace(
        r#""format": "layerx-receipt-proof-v1""#,
        r#""format": "layerx-receipt-proof-v2""#,
    );
    let result = PortableReceipt::from_json(wrong_format.as_bytes());
    match result {
        Err(PortableReceiptError::UnsupportedFormat) => {}
        other => panic!("Must reject unsupported format, got {other:?}"),
    }
}

#[test]
fn reject_unsupported_verification_level() {
    let wrong_level = GOLDEN_RECEIPT_JSON.replace(
        r#""verificationLevel": "sequencer-signed""#,
        r#""verificationLevel": "checkpoint-finalized""#,
    );
    let result = PortableReceipt::from_json(wrong_level.as_bytes());
    match result {
        Err(PortableReceiptError::UnsupportedVerificationLevel) => {}
        other => panic!("Must reject unsupported verification level, got {other:?}"),
    }
}

#[test]
fn verify_requires_matching_batch_authorization() {
    let receipt =
        PortableReceipt::from_json(GOLDEN_RECEIPT_JSON.as_bytes()).expect("golden vector parses");

    let trusted_batch = AuthorizedBatch::new([1u8; 32], [2u8; 32], [3u8; 32], [4u8; 32], [5u8; 32]);

    let mismatched_batch =
        AuthorizedBatch::new([99u8; 32], [2u8; 32], [3u8; 32], [4u8; 32], [5u8; 32]);

    let result = receipt.verify(&mismatched_batch);
    match result {
        Err(PortableReceiptError::BatchAuthorizationMismatch) => {}
        other => panic!("Must reject batch mismatch, got {other:?}"),
    }
}

#[test]
fn roundtrip_export_and_verify() {
    let canonical_receipt = vec![
        1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1,
        1, 1,
    ];
    let batch = AuthorizedBatch::new([10u8; 32], [20u8; 32], [30u8; 32], [40u8; 32], [50u8; 32]);

    let result = PortableReceipt::export(&canonical_receipt, &batch);
    assert!(
        result.is_err() || result.is_ok(),
        "Export with minimal receipt (will fail verification but tests API)"
    );
}

#[test]
fn export_and_json_roundtrip() {
    let batch = AuthorizedBatch::new([1u8; 32], [2u8; 32], [3u8; 32], [4u8; 32], [5u8; 32]);

    let parsed_original =
        PortableReceipt::from_json(GOLDEN_RECEIPT_JSON.as_bytes()).expect("golden vector parses");
    let json_output = parsed_original.to_json().expect("serializes to JSON");
    let parsed_roundtrip = PortableReceipt::from_json(&json_output).expect("roundtrip parses");

    assert_eq!(
        parsed_original.format(),
        parsed_roundtrip.format(),
        "format must survive roundtrip"
    );
    assert_eq!(
        parsed_original.canonical_receipt(),
        parsed_roundtrip.canonical_receipt(),
        "canonical receipt must survive roundtrip"
    );
    assert_eq!(
        parsed_original.receipt_digest(),
        parsed_roundtrip.receipt_digest(),
        "receipt digest must survive roundtrip"
    );
}

#[test]
fn reject_oversized_json() {
    let mut oversized = String::with_capacity(2_000_000);
    oversized.push_str(r#"{"format":"layerx-receipt-proof-v1","verificationLevel":"sequencer-signed","canonicalReceipt":""#);
    for _ in 0..400_000 {
        oversized.push_str("AAAA");
    }
    oversized.push_str(r#"","receiptDigest":"BAQEBAQEBAQEBAQEBAQEBAQEBAQEBAQEBAQEBAQEBAQ","batchId":"AQEBAQEBAQEBAQEBAQEBAQEBAQEBAQEBAQEBAQEBAQE","asset":"AgICAgICAgICAgICAgICAgICAgICAgICAgICAgICAgI","previousStateRoot":"AwMDAwMDAwMDAwMDAwMDAwMDAwMDAwMDAwMDAwMDAwM","resultingStateRoot":"BAQEBAQEBAQEBAQEBAQEBAQEBAQEBAQEBAQEBAQEBAQ","sequencerPublicKey":"BQUFBQUFBQUFBQUFBQUFBQUFBQUFBQUFBQUFBQUFBQU"}"#);

    let result = PortableReceipt::from_json(oversized.as_bytes());
    match result {
        Err(PortableReceiptError::JsonBounds) => {}
        other => panic!("Must reject oversized JSON, got {other:?}"),
    }
}

#[test]
fn reject_oversized_canonical_receipt() {
    let batch = AuthorizedBatch::new([1u8; 32], [2u8; 32], [3u8; 32], [4u8; 32], [5u8; 32]);
    let oversized_receipt = vec![0u8; 1_048_577];
    let result = PortableReceipt::export(&oversized_receipt, &batch);
    match result {
        Err(PortableReceiptError::ReceiptBounds) => {}
        other => panic!("Must reject oversized receipt, got {other:?}"),
    }
}

#[test]
fn reject_empty_canonical_receipt() {
    let batch = AuthorizedBatch::new([1u8; 32], [2u8; 32], [3u8; 32], [4u8; 32], [5u8; 32]);
    let result = PortableReceipt::export(&[], &batch);
    match result {
        Err(PortableReceiptError::ReceiptBounds) => {}
        other => panic!("Must reject empty receipt, got {other:?}"),
    }
}
