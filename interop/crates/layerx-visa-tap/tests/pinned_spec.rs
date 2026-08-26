use layerx_interop_gateway::adapter::{AdapterError, AdapterId, PinnedSpec, SpecVersion};
use layerx_visa_tap::{VISA_TAP_SPEC_COMMIT, VISA_TAP_SPEC_SHA256};

const VENDORED_SPEC: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../specs/vendor/visa-tap/README.md"
);

fn pin() -> PinnedSpec {
    let protocol = AdapterId::new("visa-tap")
        .unwrap_or_else(|error| panic!("protocol identifier: {error}"));
    let version =
        SpecVersion::parse("1").unwrap_or_else(|error| panic!("pinned version: {error}"));
    PinnedSpec::new(protocol, version, VISA_TAP_SPEC_SHA256)
        .unwrap_or_else(|error| panic!("pin: {error}"))
}

#[test]
fn vendored_visa_tap_document_matches_the_compiled_pin() {
    let document = std::fs::read(VENDORED_SPEC).unwrap_or_else(|error| {
        panic!(
            "vendored Visa TAP document for commit {VISA_TAP_SPEC_COMMIT} must exist: {error}"
        )
    });
    pin().verify_document(&document).unwrap_or_else(|error| {
        panic!("vendored Visa TAP document does not match the compiled pin: {error}")
    });
}

#[test]
fn altered_visa_tap_document_bytes_are_refused() {
    let mut document = std::fs::read(VENDORED_SPEC)
        .unwrap_or_else(|error| panic!("vendored Visa TAP document must exist: {error}"));
    document.push(b'\n');
    assert_eq!(
        pin().verify_document(&document),
        Err(AdapterError::DocumentDigestMismatch)
    );
}
