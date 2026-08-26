use layerx_ap2::{ap2_adapter_descriptor, AP2_SPEC_COMMIT, AP2_SPEC_SHA256};
use layerx_interop_gateway::adapter::{AdapterError, AdapterId, ConformanceSuite};

const VENDORED_SPEC: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../specs/vendor/ap2/specification.md"
);

fn conformance() -> ConformanceSuite {
    let suite = AdapterId::new("ap2-v1-local-matrix")
        .unwrap_or_else(|error| panic!("conformance suite identifier: {error}"));
    ConformanceSuite::new(suite, 1, [0x11; 32])
        .unwrap_or_else(|error| panic!("conformance suite: {error}"))
}

#[test]
fn vendored_ap2_specification_matches_the_compiled_pin() {
    let document = std::fs::read(VENDORED_SPEC).unwrap_or_else(|error| {
        panic!("vendored AP2 specification for commit {AP2_SPEC_COMMIT} must exist: {error}")
    });
    let descriptor = ap2_adapter_descriptor(conformance())
        .unwrap_or_else(|error| panic!("descriptor: {error}"));
    assert_eq!(descriptor.spec().document_digest(), AP2_SPEC_SHA256);
    descriptor
        .spec()
        .verify_document(&document)
        .unwrap_or_else(|error| {
            panic!("vendored AP2 specification does not match the compiled pin: {error}")
        });
}

#[test]
fn altered_ap2_specification_bytes_are_refused() {
    let mut document = std::fs::read(VENDORED_SPEC)
        .unwrap_or_else(|error| panic!("vendored AP2 specification must exist: {error}"));
    document.push(b'\n');
    let descriptor = ap2_adapter_descriptor(conformance())
        .unwrap_or_else(|error| panic!("descriptor: {error}"));
    assert_eq!(
        descriptor.spec().verify_document(&document),
        Err(AdapterError::DocumentDigestMismatch)
    );
}
