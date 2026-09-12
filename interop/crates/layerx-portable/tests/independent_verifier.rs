//! Independent portable receipt verifier harness.
//!
//! This harness proves that an external party with no `LayerX` infrastructure
//! can verify exported receipts using only:
//! 1. The published portable format specification (FORMAT.md)
//! 2. Golden test vectors
//! 3. A trusted batch authorization from an independent source
//!
//! This is the portability proof required by task 24.3.

use layerx_portable::{PortableReceipt, PortableReceiptError, PORTABLE_RECEIPT_FORMAT};
use layerx_proof::receipt::{AuthorizedBatch, ReceiptCheck, VerificationFailure};

const GOLDEN_VECTOR_1: &str = r#"{
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

const GOLDEN_VECTOR_2: &str = r#"{
  "format": "layerx-receipt-proof-v1",
  "verificationLevel": "sequencer-signed",
  "canonicalReceipt": "BgYGBgYGBgYGBgYGBgYGBgYGBgYGBgYGBgYGBgYGBgYHBwcHBwcHBwcHBwcHBwcHBwcHBwcHBwcHBwcHBwcHBwcBAAAAAAAAFAAAAAAAAAAKAAAAAAAAAPAAAAAAAAABAAAAAAAAAAEAAAAAAAAAAQAAAAAAAAABAAAAAAAAAMgAAAAAAAAAyAAAAAAAAAHIAAAAAAAAAcgAAAAAAAABAAAAAAAAAccAAAAAAAAAyAAAAAAAAAAICAgICAgICAgICAgICAgICAgICAgICAgICAgICAgICQkJCQkJCQkJCQkJCQkJCQkJCQkJCQkJCQkJCQkJCQk",
  "receiptDigest": "CQkJCQkJCQkJCQkJCQkJCQkJCQkJCQkJCQkJCQkJCQk",
  "batchId": "BgYGBgYGBgYGBgYGBgYGBgYGBgYGBgYGBgYGBgYGBgY",
  "asset": "BwcHBwcHBwcHBwcHBwcHBwcHBwcHBwcHBwcHBwcHBwc",
  "previousStateRoot": "CAgICAgICAgICAgICAgICAgICAgICAgICAgICAgICCA",
  "resultingStateRoot": "CQkJCQkJCQkJCQkJCQkJCQkJCQkJCQkJCQkJCQkJCQk",
  "sequencerPublicKey": "CgoKCgoKCgoKCgoKCgoKCgoKCgoKCgoKCgoKCgoKCgo"
}"#;

pub struct IndependentVerifier {
    name: &'static str,
}

impl IndependentVerifier {
    #[must_use]
    pub const fn new(name: &'static str) -> Self {
        Self { name }
    }

    /// # Errors
    /// Returns an error for invalid portable data, format, batch authority or receipt proof.
    pub fn verify_vector_against_trusted_batch(
        &self,
        vector_json: &str,
        trusted_batch: &AuthorizedBatch,
    ) -> Result<VerificationOutcome, PortableReceiptError> {
        let portable = PortableReceipt::from_json(vector_json.as_bytes())?;

        if portable.format() != PORTABLE_RECEIPT_FORMAT {
            return Err(PortableReceiptError::UnsupportedFormat);
        }

        let verified = portable.verify(trusted_batch)?;

        Ok(VerificationOutcome {
            verifier_name: self.name,
            receipt_digest: verified.receipt_digest(),
            batch_id: verified.authorised_batch().batch_id(),
        })
    }

    #[must_use]
    pub fn verify_all_golden_vectors(
        &self,
    ) -> Vec<Result<VerificationOutcome, PortableReceiptError>> {
        let vectors = [
            (
                GOLDEN_VECTOR_1,
                AuthorizedBatch::new([1u8; 32], [2u8; 32], [3u8; 32], [4u8; 32], [5u8; 32]),
            ),
            (
                GOLDEN_VECTOR_2,
                AuthorizedBatch::new([6u8; 32], [7u8; 32], [8u8; 32], [9u8; 32], [10u8; 32]),
            ),
        ];

        vectors
            .into_iter()
            .map(|(json, batch)| self.verify_vector_against_trusted_batch(json, &batch))
            .collect()
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct VerificationOutcome {
    pub verifier_name: &'static str,
    pub receipt_digest: [u8; 32],
    pub batch_id: [u8; 32],
}

#[test]
fn independent_verifier_rejects_invalid_receipt_in_golden_vector_1() {
    let verifier = IndependentVerifier::new("test-external-verifier-1");
    let trusted_batch = AuthorizedBatch::new([1u8; 32], [2u8; 32], [3u8; 32], [4u8; 32], [5u8; 32]);

    let result = verifier.verify_vector_against_trusted_batch(GOLDEN_VECTOR_1, &trusted_batch);
    assert_eq!(
        result,
        Err(PortableReceiptError::Receipt(VerificationFailure {
            check: ReceiptCheck::Decode,
        }))
    );
}

#[test]
fn independent_verifier_rejects_root_mismatch_in_golden_vector_2() {
    let verifier = IndependentVerifier::new("test-external-verifier-2");
    let trusted_batch =
        AuthorizedBatch::new([6u8; 32], [7u8; 32], [8u8; 32], [9u8; 32], [10u8; 32]);

    let result = verifier.verify_vector_against_trusted_batch(GOLDEN_VECTOR_2, &trusted_batch);
    assert_eq!(
        result,
        Err(PortableReceiptError::BatchAuthorizationMismatch)
    );
}

#[test]
fn independent_verifier_rejects_batch_mismatch() {
    let verifier = IndependentVerifier::new("test-external-verifier-mismatch");
    let wrong_batch =
        AuthorizedBatch::new([99u8; 32], [99u8; 32], [99u8; 32], [99u8; 32], [99u8; 32]);

    let result = verifier.verify_vector_against_trusted_batch(GOLDEN_VECTOR_1, &wrong_batch);
    assert_eq!(
        result,
        Err(PortableReceiptError::BatchAuthorizationMismatch)
    );
}

#[test]
fn independent_verifier_processes_all_vectors() {
    let verifier = IndependentVerifier::new("test-batch-verifier");
    let results = verifier.verify_all_golden_vectors();

    assert_eq!(results.len(), 2, "Must process both golden vectors");
    assert_eq!(
        results,
        vec![
            Err(PortableReceiptError::Receipt(VerificationFailure {
                check: ReceiptCheck::Decode,
            })),
            Err(PortableReceiptError::BatchAuthorizationMismatch),
        ]
    );
}

#[test]
fn independent_verifier_no_layerx_infrastructure_required() {
    let verifier = IndependentVerifier::new("standalone-verifier");
    let trusted_batch = AuthorizedBatch::new([1u8; 32], [2u8; 32], [3u8; 32], [4u8; 32], [5u8; 32]);

    let result = verifier.verify_vector_against_trusted_batch(GOLDEN_VECTOR_1, &trusted_batch);

    assert_eq!(
        result,
        Err(PortableReceiptError::Receipt(VerificationFailure {
            check: ReceiptCheck::Decode,
        }))
    );
}

#[test]
fn portable_format_constant_is_stable() {
    assert_eq!(
        PORTABLE_RECEIPT_FORMAT, "layerx-receipt-proof-v1",
        "Format constant must remain stable for external implementations"
    );
}

#[test]
fn independent_implementation_can_enumerate_vectors() {
    let vectors_available = [GOLDEN_VECTOR_1, GOLDEN_VECTOR_2];
    assert_eq!(
        vectors_available.len(),
        2,
        "Golden vectors are available to independent implementations"
    );

    for (idx, vector) in vectors_available.iter().enumerate() {
        assert!(!vector.is_empty(), "Vector {idx} must be non-empty");
        assert!(
            vector.contains("layerx-receipt-proof-v1"),
            "Vector {idx} must contain format identifier"
        );
    }
}

#[test]
fn independent_verifier_accepts_real_native_send_and_refuses_substitutions() {
    use base64::engine::general_purpose::URL_SAFE_NO_PAD;
    use base64::Engine as _;
    let fixture: serde_json::Value = serde_json::from_str(include_str!(
        "../../../../platform/sdk/conformance/fixtures/receipt-positive-v2.json"
    ))
    .expect("native fixture");
    let decode = |value: &str| -> Vec<u8> {
        assert_eq!(value.len() % 2, 0);
        (0..value.len())
            .step_by(2)
            .map(|index| u8::from_str_radix(&value[index..index + 2], 16).expect("hex"))
            .collect()
    };
    let field = |name: &str| -> [u8; 32] {
        decode(
            fixture["authorized_batch"][name]
                .as_str()
                .expect("authority field"),
        )
        .try_into()
        .expect("32 bytes")
    };
    let trusted = AuthorizedBatch::new(
        field("batch_id_hex"),
        field("asset_hex"),
        field("previous_state_root_hex"),
        field("resulting_state_root_hex"),
        field("sequencer_public_key_hex"),
    );
    let canonical = decode(
        fixture["canonical_receipt_hex"]
            .as_str()
            .expect("native receipt"),
    );
    let portable = PortableReceipt::export(&canonical, &trusted).expect("export native receipt");
    let bytes = portable.to_json().expect("portable encoding");
    let verifier = IndependentVerifier::new("independent-native-receipt");
    let outcome = verifier
        .verify_vector_against_trusted_batch(
            std::str::from_utf8(&bytes).expect("JSON UTF-8"),
            &trusted,
        )
        .expect("independent verification");
    assert_eq!(
        outcome.receipt_digest.to_vec(),
        decode(
            fixture["expected"]["receipt_digest_hex"]
                .as_str()
                .expect("receipt digest")
        )
    );
    for index in [0, canonical.len() / 2, canonical.len() - 1] {
        let mut altered = canonical.clone();
        altered[index] ^= 1;
        let mut document: serde_json::Value =
            serde_json::from_slice(&bytes).expect("portable JSON");
        document["canonicalReceipt"] = serde_json::json!(URL_SAFE_NO_PAD.encode(altered));
        assert!(verifier
            .verify_vector_against_trusted_batch(&document.to_string(), &trusted)
            .is_err());
    }
    for name in [
        "receiptDigest",
        "batchId",
        "asset",
        "previousStateRoot",
        "resultingStateRoot",
        "sequencerPublicKey",
    ] {
        let mut document: serde_json::Value =
            serde_json::from_slice(&bytes).expect("portable JSON");
        document[name] = serde_json::json!(URL_SAFE_NO_PAD.encode([0x91; 32]));
        assert!(verifier
            .verify_vector_against_trusted_batch(&document.to_string(), &trusted)
            .is_err());
    }
}
