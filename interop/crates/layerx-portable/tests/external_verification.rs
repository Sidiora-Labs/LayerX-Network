//! Tests for verifying external mandates and receipts through adapters.
//!
//! These tests prove that external protocol evidence is bound to exact
//! presentation bytes, pinned specifications, and conformance suite digests.

use layerx_interop_gateway::adapter::{
    AdapterDescriptor, AdapterId, ConformanceSuite, PinnedSpec, SpecVersion,
};
use layerx_portable::{
    verify_external_evidence, ExternalEvidenceKind, ExternalEvidenceVerifier, ExternalPresentation,
    ExternalPresentationError, ExternalVerificationError,
};

struct MockMandateVerifier {
    descriptor: AdapterDescriptor,
}

impl MockMandateVerifier {
    fn new() -> Self {
        let adapter_id = AdapterId::new("test-adapter").expect("valid adapter id");
        let protocol_id = AdapterId::new("test-protocol").expect("valid protocol id");
        let spec_version = SpecVersion::parse("1.0.0").expect("valid spec version");
        let spec_document_digest = [1u8; 32];
        let spec = PinnedSpec::new(protocol_id, spec_version, spec_document_digest)
            .expect("valid pinned spec");
        let suite_id = AdapterId::new("test-suite-v1").expect("valid suite id");
        let conformance =
            ConformanceSuite::new(suite_id, 10, [2u8; 32]).expect("valid conformance suite");
        let descriptor = AdapterDescriptor::new(adapter_id, spec, conformance);
        Self { descriptor }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct VerifiedMandate {
    payload_size: usize,
}

#[derive(Clone, Debug, Eq, PartialEq)]
enum MockVerificationError {
    InvalidSignature,
    ExpiredTimestamp,
}

impl std::fmt::Display for MockVerificationError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::InvalidSignature => formatter.write_str("invalid signature"),
            Self::ExpiredTimestamp => formatter.write_str("expired timestamp"),
        }
    }
}

impl std::error::Error for MockVerificationError {}

impl ExternalEvidenceVerifier<()> for MockMandateVerifier {
    type Verified = VerifiedMandate;
    type Error = MockVerificationError;

    fn descriptor(&self) -> &AdapterDescriptor {
        &self.descriptor
    }

    fn evidence_kind(&self) -> ExternalEvidenceKind {
        ExternalEvidenceKind::Mandate
    }

    fn media_type(&self) -> &str {
        "application/test-mandate+json"
    }

    fn verify(&self, payload: &[u8], _context: &()) -> Result<Self::Verified, Self::Error> {
        if payload.len() < 10 {
            return Err(MockVerificationError::InvalidSignature);
        }
        if payload[0] == 0xFF {
            return Err(MockVerificationError::ExpiredTimestamp);
        }
        Ok(VerifiedMandate {
            payload_size: payload.len(),
        })
    }
}

#[test]
fn verify_external_mandate_with_matching_presentation() {
    let verifier = MockMandateVerifier::new();
    let payload = b"valid-mandate-payload-data";
    let presentation = ExternalPresentation::new(
        "test-adapter",
        "test-protocol",
        "1.0.0",
        ExternalEvidenceKind::Mandate,
        "application/test-mandate+json",
        payload,
    )
    .expect("valid presentation");

    let result = verify_external_evidence(&verifier, &presentation, &());
    assert!(
        result.is_ok(),
        "Valid mandate must verify successfully: {result:?}"
    );

    let verified = result.expect("verification succeeds");
    assert_eq!(verified.adapter(), "test-adapter");
    assert_eq!(verified.protocol(), "test-protocol");
    assert_eq!(verified.spec_version(), "1.0.0");
    assert_eq!(verified.kind(), ExternalEvidenceKind::Mandate);
    assert_eq!(verified.media_type(), "application/test-mandate+json");
    assert_eq!(verified.conformance_suite(), "test-suite-v1");
    assert_eq!(verified.conformance_vector_count(), 10);
    assert_eq!(verified.verified().payload_size, payload.len());
}

#[test]
fn reject_adapter_id_mismatch() {
    let verifier = MockMandateVerifier::new();
    let payload = b"valid-mandate-payload-data";
    let presentation = ExternalPresentation::new(
        "wrong-adapter",
        "test-protocol",
        "1.0.0",
        ExternalEvidenceKind::Mandate,
        "application/test-mandate+json",
        payload,
    )
    .expect("valid presentation");

    let result = verify_external_evidence(&verifier, &presentation, &());
    match result {
        Err(ExternalVerificationError::DescriptorMismatch) => {}
        other => panic!("Must reject adapter mismatch, got {other:?}"),
    }
}

#[test]
fn reject_protocol_id_mismatch() {
    let verifier = MockMandateVerifier::new();
    let payload = b"valid-mandate-payload-data";
    let presentation = ExternalPresentation::new(
        "test-adapter",
        "wrong-protocol",
        "1.0.0",
        ExternalEvidenceKind::Mandate,
        "application/test-mandate+json",
        payload,
    )
    .expect("valid presentation");

    let result = verify_external_evidence(&verifier, &presentation, &());
    match result {
        Err(ExternalVerificationError::DescriptorMismatch) => {}
        other => panic!("Must reject protocol mismatch, got {other:?}"),
    }
}

#[test]
fn reject_spec_version_mismatch() {
    let verifier = MockMandateVerifier::new();
    let payload = b"valid-mandate-payload-data";
    let presentation = ExternalPresentation::new(
        "test-adapter",
        "test-protocol",
        "2.0.0",
        ExternalEvidenceKind::Mandate,
        "application/test-mandate+json",
        payload,
    )
    .expect("valid presentation");

    let result = verify_external_evidence(&verifier, &presentation, &());
    match result {
        Err(ExternalVerificationError::DescriptorMismatch) => {}
        other => panic!("Must reject version mismatch, got {other:?}"),
    }
}

#[test]
fn reject_evidence_kind_mismatch() {
    let verifier = MockMandateVerifier::new();
    let payload = b"valid-mandate-payload-data";
    let presentation = ExternalPresentation::new(
        "test-adapter",
        "test-protocol",
        "1.0.0",
        ExternalEvidenceKind::Receipt,
        "application/test-mandate+json",
        payload,
    )
    .expect("valid presentation");

    let result = verify_external_evidence(&verifier, &presentation, &());
    match result {
        Err(ExternalVerificationError::EvidenceKindMismatch) => {}
        other => panic!("Must reject evidence kind mismatch, got {other:?}"),
    }
}

#[test]
fn reject_media_type_mismatch() {
    let verifier = MockMandateVerifier::new();
    let payload = b"valid-mandate-payload-data";
    let presentation = ExternalPresentation::new(
        "test-adapter",
        "test-protocol",
        "1.0.0",
        ExternalEvidenceKind::Mandate,
        "application/wrong-media-type+json",
        payload,
    )
    .expect("valid presentation");

    let result = verify_external_evidence(&verifier, &presentation, &());
    match result {
        Err(ExternalVerificationError::MediaTypeMismatch) => {}
        other => panic!("Must reject media type mismatch, got {other:?}"),
    }
}

#[test]
fn preserve_adapter_verification_error() {
    let verifier = MockMandateVerifier::new();
    let payload = b"short";
    let presentation = ExternalPresentation::new(
        "test-adapter",
        "test-protocol",
        "1.0.0",
        ExternalEvidenceKind::Mandate,
        "application/test-mandate+json",
        payload,
    )
    .expect("valid presentation");

    let result = verify_external_evidence(&verifier, &presentation, &());
    match result {
        Err(ExternalVerificationError::Adapter(MockVerificationError::InvalidSignature)) => {}
        other => panic!("Must preserve adapter error, got {other:?}"),
    }
}

#[test]
fn evidence_digest_changes_with_payload() {
    let verifier = MockMandateVerifier::new();
    let payload1 = b"mandate-payload-one";
    let payload2 = b"mandate-payload-two";

    let presentation1 = ExternalPresentation::new(
        "test-adapter",
        "test-protocol",
        "1.0.0",
        ExternalEvidenceKind::Mandate,
        "application/test-mandate+json",
        payload1,
    )
    .expect("valid presentation");

    let presentation2 = ExternalPresentation::new(
        "test-adapter",
        "test-protocol",
        "1.0.0",
        ExternalEvidenceKind::Mandate,
        "application/test-mandate+json",
        payload2,
    )
    .expect("valid presentation");

    let verified1 =
        verify_external_evidence(&verifier, &presentation1, &()).expect("verification 1 succeeds");
    let verified2 =
        verify_external_evidence(&verifier, &presentation2, &()).expect("verification 2 succeeds");

    assert_ne!(
        verified1.evidence_digest(),
        verified2.evidence_digest(),
        "Evidence digest must change with payload"
    );
}

#[test]
fn reject_invalid_adapter_id() {
    let payload = b"valid-mandate-payload-data";
    let result = ExternalPresentation::new(
        "INVALID ID",
        "test-protocol",
        "1.0.0",
        ExternalEvidenceKind::Mandate,
        "application/test-mandate+json",
        payload,
    );
    match result {
        Err(ExternalPresentationError::InvalidAdapter) => {}
        other => panic!("Must reject invalid adapter id, got {other:?}"),
    }
}

#[test]
fn reject_invalid_protocol_id() {
    let payload = b"valid-mandate-payload-data";
    let result = ExternalPresentation::new(
        "test-adapter",
        "",
        "1.0.0",
        ExternalEvidenceKind::Mandate,
        "application/test-mandate+json",
        payload,
    );
    match result {
        Err(ExternalPresentationError::InvalidProtocol) => {}
        other => panic!("Must reject invalid protocol id, got {other:?}"),
    }
}

#[test]
fn reject_unpinned_version() {
    let payload = b"valid-mandate-payload-data";
    let result = ExternalPresentation::new(
        "test-adapter",
        "test-protocol",
        "1.x",
        ExternalEvidenceKind::Mandate,
        "application/test-mandate+json",
        payload,
    );
    match result {
        Err(ExternalPresentationError::UnpinnedVersion) => {}
        other => panic!("Must reject unpinned version, got {other:?}"),
    }
}

#[test]
fn reject_empty_media_type() {
    let payload = b"valid-mandate-payload-data";
    let result = ExternalPresentation::new(
        "test-adapter",
        "test-protocol",
        "1.0.0",
        ExternalEvidenceKind::Mandate,
        "",
        payload,
    );
    match result {
        Err(ExternalPresentationError::InvalidMediaType) => {}
        other => panic!("Must reject empty media type, got {other:?}"),
    }
}

#[test]
fn reject_media_type_with_control_characters() {
    let payload = b"valid-mandate-payload-data";
    let result = ExternalPresentation::new(
        "test-adapter",
        "test-protocol",
        "1.0.0",
        ExternalEvidenceKind::Mandate,
        "application/test\x00+json",
        payload,
    );
    match result {
        Err(ExternalPresentationError::InvalidMediaType) => {}
        other => panic!("Must reject media type with control chars, got {other:?}"),
    }
}

#[test]
fn reject_empty_payload() {
    let result = ExternalPresentation::new(
        "test-adapter",
        "test-protocol",
        "1.0.0",
        ExternalEvidenceKind::Mandate,
        "application/test-mandate+json",
        b"",
    );
    match result {
        Err(ExternalPresentationError::PayloadBounds) => {}
        other => panic!("Must reject empty payload, got {other:?}"),
    }
}

#[test]
fn reject_oversized_payload() {
    let oversized_payload = vec![0u8; 2_097_153];
    let result = ExternalPresentation::new(
        "test-adapter",
        "test-protocol",
        "1.0.0",
        ExternalEvidenceKind::Mandate,
        "application/test-mandate+json",
        &oversized_payload,
    );
    match result {
        Err(ExternalPresentationError::PayloadBounds) => {}
        other => panic!("Must reject oversized payload, got {other:?}"),
    }
}

#[test]
fn verified_external_evidence_binds_all_inputs() {
    let verifier = MockMandateVerifier::new();
    let payload = b"mandate-payload-complete-binding-test";
    let presentation = ExternalPresentation::new(
        "test-adapter",
        "test-protocol",
        "1.0.0",
        ExternalEvidenceKind::Mandate,
        "application/test-mandate+json",
        payload,
    )
    .expect("valid presentation");

    let verified =
        verify_external_evidence(&verifier, &presentation, &()).expect("verification succeeds");

    assert_eq!(verified.adapter(), presentation.adapter());
    assert_eq!(verified.protocol(), presentation.protocol());
    assert_eq!(verified.spec_version(), presentation.spec_version());
    assert_eq!(verified.kind(), presentation.kind());
    assert_eq!(verified.media_type(), presentation.media_type());
    assert_eq!(
        verified.spec_document_digest(),
        verifier.descriptor().spec().document_digest()
    );
    assert_eq!(
        verified.conformance_suite(),
        verifier.descriptor().conformance().suite().as_str()
    );
    assert_eq!(
        verified.conformance_vector_count(),
        verifier.descriptor().conformance().vector_count()
    );
    assert_eq!(
        verified.conformance_suite_digest(),
        verifier.descriptor().conformance().suite_digest()
    );
}
