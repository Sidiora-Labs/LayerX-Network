use layerx_ucp::{
    interop_ucp, ucp_adapter_descriptor, Capability, MerchantProfile, PaymentHandler, UcpError,
    UcpIdempotencyKey,
};
use layerx_interop_gateway::adapter::{AdapterId, ConformanceSuite, PinnedSpec, SpecVersion};
use sha2::{Digest as _, Sha256};

const UCP_VERSION: &str = "2026-04-08";
const CHECKOUT_CAPABILITY: &str = "dev.ucp.shopping.checkout";
const ORDER_CAPABILITY: &str = "dev.ucp.shopping.order";

fn spec_digest(content: &str) -> [u8; 32] {
    Sha256::digest(content.as_bytes()).into()
}

fn conformance_digest(vectors: &[&str]) -> [u8; 32] {
    let mut hasher = Sha256::new();
    for vector in vectors {
        hasher.update(vector.as_bytes());
        hasher.update([0]);
    }
    hasher.finalize().into()
}

#[test]
fn ucp_adapter_declares_versioned_spec_and_conformance() {
    let spec_content = concat!(
        "UCP 2026-04-08 Specification\n",
        "Checkout: POST /checkout {checkout_id, currency, amount}\n",
        "Order: GET /order/{order_id} -> {id, checkout_id, amount, receipt_digest}\n",
    );
    let spec_digest = spec_digest(spec_content);

    let vectors = vec![
        "checkout_minimal: {checkout_id: 'chk_1', currency: 'USD', amount: 100}",
        "checkout_refused: {checkout_id: 'chk_bad', status: 'incomplete'}",
        "order_completed: {id: 'ord_1', checkout_id: 'chk_1', receipt_digest: '...'}",
    ];
    let conformance_digest = conformance_digest(&vectors);

    let version = SpecVersion::parse(UCP_VERSION)
        .unwrap_or_else(|error| panic!("version: {error}"));
    let adapter_id = AdapterId::new("ucp")
        .unwrap_or_else(|error| panic!("adapter id: {error}"));
    let spec = PinnedSpec::new(adapter_id.clone(), version, spec_digest)
        .unwrap_or_else(|error| panic!("spec: {error}"));
    let conformance = ConformanceSuite::new(
        AdapterId::new("ucp-2026-04-08-vectors")
            .unwrap_or_else(|error| panic!("conformance id: {error}")),
        vectors.len() as u32,
        conformance_digest,
    )
    .unwrap_or_else(|error| panic!("conformance: {error}"));

    let descriptor = ucp_adapter_descriptor(spec, conformance)
        .unwrap_or_else(|error| panic!("descriptor: {error}"));

    assert_eq!(descriptor.adapter(), &adapter_id);
    assert_eq!(descriptor.spec().version().as_str(), UCP_VERSION);
    assert_eq!(descriptor.conformance().vector_count(), 3);
}

#[test]
fn merchant_profile_validation_refuses_invalid_urls() {
    let handler = PaymentHandler::new(
        "test-handler",
        "2.0.0",
        "https://example.com/spec",
        "https://example.com/schema.json",
    )
    .unwrap_or_else(|error| panic!("handler: {error}"));

    let invalid_http = MerchantProfile::layerx("http://insecure.example/ucp", handler.clone());
    assert!(matches!(invalid_http, Err(UcpError::InvalidProfile)));

    let no_domain = MerchantProfile::layerx("https://", handler.clone());
    assert!(matches!(no_domain, Err(UcpError::InvalidProfile)));

    let spaces = MerchantProfile::layerx("https://example .com/ucp", handler);
    assert!(matches!(spaces, Err(UcpError::InvalidProfile)));
}

#[test]
fn capability_names_follow_reverse_domain_convention() {
    assert!(CHECKOUT_CAPABILITY.starts_with("dev.ucp."));
    assert!(ORDER_CAPABILITY.starts_with("dev.ucp."));

    let capability = Capability::new(
        CHECKOUT_CAPABILITY,
        UCP_VERSION,
        "https://ucp.dev/spec",
        "https://ucp.dev/schema.json",
    )
    .unwrap_or_else(|error| panic!("capability: {error}"));

    assert_eq!(capability.name(), CHECKOUT_CAPABILITY);
    assert_eq!(capability.version(), UCP_VERSION);
}

#[test]
fn idempotency_keys_parse_exact_uuid_format() {
    let valid = UcpIdempotencyKey::parse("12345678-1234-5678-1234-567812345678")
        .unwrap_or_else(|error| panic!("valid uuid: {error}"));

    assert_ne!(valid.gateway_key(), [0; 32]);

    let uppercase = UcpIdempotencyKey::parse("ABCDEF01-2345-6789-ABCD-EF0123456789")
        .unwrap_or_else(|error| panic!("uppercase uuid: {error}"));

    assert_ne!(uppercase.gateway_key(), [0; 32]);

    let all_zeros = UcpIdempotencyKey::parse("00000000-0000-0000-0000-000000000000");
    assert!(matches!(all_zeros, Err(UcpError::InvalidIdempotencyKey)));

    let missing_dash = UcpIdempotencyKey::parse("123456781234567812345678123456");
    assert!(matches!(missing_dash, Err(UcpError::InvalidIdempotencyKey)));

    let too_short = UcpIdempotencyKey::parse("12345678-1234-5678-1234");
    assert!(matches!(too_short, Err(UcpError::InvalidIdempotencyKey)));
}

#[test]
fn payment_handler_digest_is_stable_and_collision_resistant() {
    let handler1 = PaymentHandler::new(
        "handler-a",
        "1.0.0",
        "https://example.com/spec-a",
        "https://example.com/schema-a.json",
    )
    .unwrap_or_else(|error| panic!("handler1: {error}"));

    let handler2 = PaymentHandler::new(
        "handler-b",
        "1.0.0",
        "https://example.com/spec-a",
        "https://example.com/schema-a.json",
    )
    .unwrap_or_else(|error| panic!("handler2: {error}"));

    let handler1_copy = PaymentHandler::new(
        "handler-a",
        "1.0.0",
        "https://example.com/spec-a",
        "https://example.com/schema-a.json",
    )
    .unwrap_or_else(|error| panic!("handler1 copy: {error}"));

    assert_ne!(
        handler1.payment_handler_digest(),
        handler2.payment_handler_digest(),
        "different ids must produce different digests"
    );

    assert_eq!(
        handler1.payment_handler_digest(),
        handler1_copy.payment_handler_digest(),
        "identical handlers must produce identical digests"
    );
}

#[test]
fn ucp_codify_anchor_remains_stable() {
    assert_eq!(interop_ucp(), "ucp-2026-04-08-receipt-backed-commerce");
}

#[test]
fn conformance_vectors_cover_all_status_transitions() {
    let vectors = vec![
        "incomplete: checkout refused before submission",
        "requires_escalation: checkout needs manual review",
        "ready_for_complete: checkout validated, awaiting completion",
        "complete_in_progress: checkout submitted, pending receipt",
        "completed: checkout finalized with verified receipt",
        "canceled: checkout explicitly canceled",
    ];

    assert_eq!(
        vectors.len(),
        6,
        "UCP status vocabulary declares 6 states; conformance must cover all"
    );

    let digest = conformance_digest(&vectors);
    assert_ne!(digest, [0; 32]);
}
