use layerx_interop_gateway::adapter::{AdapterError, AdapterId, ConformanceSuite};
use layerx_x402::{x402_adapter_descriptor, X402_SPEC_COMMIT, X402_SPEC_SHA256};

const VENDORED_SPEC: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../specs/vendor/x402/x402-specification-v2.md"
);

fn conformance() -> ConformanceSuite {
    let suite = AdapterId::new("x402-v2-local-matrix")
        .unwrap_or_else(|error| panic!("conformance suite identifier: {error}"));
    ConformanceSuite::new(suite, 1, [0x11; 32])
        .unwrap_or_else(|error| panic!("conformance suite: {error}"))
}

#[test]
fn vendored_x402_specification_matches_the_compiled_pin() {
    let document = std::fs::read(VENDORED_SPEC).unwrap_or_else(|error| {
        panic!("vendored x402 specification for commit {X402_SPEC_COMMIT} must exist: {error}")
    });
    let descriptor = x402_adapter_descriptor(conformance())
        .unwrap_or_else(|error| panic!("descriptor: {error}"));
    assert_eq!(descriptor.spec().document_digest(), X402_SPEC_SHA256);
    descriptor
        .spec()
        .verify_document(&document)
        .unwrap_or_else(|error| {
            panic!("vendored x402 specification does not match the compiled pin: {error}")
        });
}

#[test]
fn altered_x402_specification_bytes_are_refused() {
    let mut document = std::fs::read(VENDORED_SPEC)
        .unwrap_or_else(|error| panic!("vendored x402 specification must exist: {error}"));
    document.push(b'\n');
    let descriptor = x402_adapter_descriptor(conformance())
        .unwrap_or_else(|error| panic!("descriptor: {error}"));
    assert_eq!(
        descriptor.spec().verify_document(&document),
        Err(AdapterError::DocumentDigestMismatch)
    );
}
