use layerx_interop_gateway::adapter::{AdapterError, AdapterId, PinnedSpec, SpecVersion};
use layerx_ucp::{
    UCP_CHECKOUT_SCHEMA_SHA256, UCP_CHECKOUT_SPEC_SHA256, UCP_ORDER_SCHEMA_SHA256,
    UCP_ORDER_SPEC_SHA256, UCP_REST_SCHEMA_SHA256,
};

const VENDOR_ROOT: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../../specs/vendor/ucp");

fn pin(digest: [u8; 32]) -> PinnedSpec {
    let protocol =
        AdapterId::new("ucp").unwrap_or_else(|error| panic!("protocol identifier: {error}"));
    let version = SpecVersion::parse("2026.04.08")
        .unwrap_or_else(|error| panic!("pinned version: {error}"));
    PinnedSpec::new(protocol, version, digest).unwrap_or_else(|error| panic!("pin: {error}"))
}

fn verify_vendored(file: &str, digest: [u8; 32]) {
    let path = format!("{VENDOR_ROOT}/{file}");
    let document = std::fs::read(&path)
        .unwrap_or_else(|error| panic!("vendored UCP document {file} must exist: {error}"));
    pin(digest).verify_document(&document).unwrap_or_else(|error| {
        panic!("vendored UCP document {file} does not match the compiled pin: {error}")
    });
}

#[test]
fn vendored_ucp_documents_match_the_compiled_pins() {
    verify_vendored("specification-checkout.html", UCP_CHECKOUT_SPEC_SHA256);
    verify_vendored("specification-order.html", UCP_ORDER_SPEC_SHA256);
    verify_vendored("checkout.schema.json", UCP_CHECKOUT_SCHEMA_SHA256);
    verify_vendored("order.schema.json", UCP_ORDER_SCHEMA_SHA256);
    verify_vendored("rest.openapi.json", UCP_REST_SCHEMA_SHA256);
}

#[test]
fn altered_ucp_document_bytes_are_refused() {
    let path = format!("{VENDOR_ROOT}/checkout.schema.json");
    let mut document = std::fs::read(path)
        .unwrap_or_else(|error| panic!("vendored UCP checkout schema must exist: {error}"));
    document.push(b'\n');
    assert_eq!(
        pin(UCP_CHECKOUT_SCHEMA_SHA256).verify_document(&document),
        Err(AdapterError::DocumentDigestMismatch)
    );
}
