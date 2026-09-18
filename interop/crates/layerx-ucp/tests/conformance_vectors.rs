//! UCP conformance harness. The vectors are the first-party suite under
//! `interop/specs/conformance/ucp`, which the gateway deployment pins by
//! identifier, vector count and digest; every record is run through the
//! production `Capability`, `PaymentHandler`, `MerchantProfile`,
//! `UcpIdempotencyKey` and `CheckoutStatus` types.

use std::collections::BTreeMap;

use layerx_interop_gateway::adapter::{AdapterId, ConformanceSuite, PinnedSpec, SpecVersion};
use layerx_ucp::{
    interop_ucp, ucp_adapter_descriptor, Capability, CheckoutStatus, MerchantProfile,
    PaymentHandler, UcpError, UcpIdempotencyKey, UCP_CHECKOUT_SPEC_SHA256,
};
use sha2::{Digest as _, Sha256};

const UCP_VERSION: &str = "2026-04-08";
const SUITE: &str = "layerx-ucp-conformance-v1";
const CHECKOUT_CAPABILITY: &str = "dev.ucp.shopping.checkout";
const ORDER_CAPABILITY: &str = "dev.ucp.shopping.order";

const CHECKOUT_STATUS: &str = include_str!("../../../specs/conformance/ucp/checkout-status.json");
const IDEMPOTENCY_KEYS: &str = include_str!("../../../specs/conformance/ucp/idempotency-keys.json");
const MERCHANT_PROFILES: &str =
    include_str!("../../../specs/conformance/ucp/merchant-profiles.json");
const CAPABILITIES: &str = include_str!("../../../specs/conformance/ucp/capabilities.json");
const PAYMENT_HANDLERS: &str = include_str!("../../../specs/conformance/ucp/payment-handlers.json");

/// Every vector file of the suite, by its path relative to the suite directory.
/// The order is the sorted order the deployment renderer walks, so the digest
/// below is the digest `interop/deploy/gateway/render.py` pins.
const SUITE_FILES: [(&str, &str); 5] = [
    ("capabilities.json", CAPABILITIES),
    ("checkout-status.json", CHECKOUT_STATUS),
    ("idempotency-keys.json", IDEMPOTENCY_KEYS),
    ("merchant-profiles.json", MERCHANT_PROFILES),
    ("payment-handlers.json", PAYMENT_HANDLERS),
];

fn records(suite: &str, source: &str) -> Vec<serde_json::Value> {
    let records: Vec<serde_json::Value> =
        serde_json::from_str(suite).unwrap_or_else(|error| panic!("{source}: {error}"));
    assert!(
        !records.is_empty(),
        "{source}: a suite that carries no vector is not a conformance suite"
    );
    records
}

fn field<'a>(record: &'a serde_json::Value, name: &str, source: &str) -> &'a str {
    record[name]
        .as_str()
        .unwrap_or_else(|| panic!("{source}: every vector declares {name}"))
}

fn valid(record: &serde_json::Value, source: &str) -> bool {
    record["valid"]
        .as_bool()
        .unwrap_or_else(|| panic!("{source}: every vector declares whether it must be accepted"))
}

fn refusal(record: &serde_json::Value, source: &str) -> UcpError {
    match field(record, "refusal", source) {
        "InvalidProfile" => UcpError::InvalidProfile,
        "CapabilityUnavailable" => UcpError::CapabilityUnavailable,
        "PaymentHandlerUnavailable" => UcpError::PaymentHandlerUnavailable,
        "PaymentHandlerMismatch" => UcpError::PaymentHandlerMismatch,
        "InvalidIdempotencyKey" => UcpError::InvalidIdempotencyKey,
        "InvalidCheckout" => UcpError::InvalidCheckout,
        "InvalidOrder" => UcpError::InvalidOrder,
        other => panic!("{source}: {other} is not a UCP refusal"),
    }
}

fn handler_of(record: &serde_json::Value, source: &str) -> Result<PaymentHandler, UcpError> {
    PaymentHandler::new(
        field(record, "id", source),
        field(record, "version", source),
        field(record, "spec", source),
        field(record, "schema", source),
    )
}

/// The suite digest, over each vector file's path and bytes in sorted path
/// order: the exact rule `interop/deploy/gateway/render.py` applies to the same
/// files, so the declared suite is the suite this harness runs.
fn suite_digest() -> [u8; 32] {
    let mut hasher = Sha256::new();
    for (path, content) in SUITE_FILES {
        hasher.update(path.as_bytes());
        hasher.update([0]);
        hasher.update(content.as_bytes());
        hasher.update([0]);
    }
    hasher.finalize().into()
}

fn suite_vector_count() -> u64 {
    SUITE_FILES
        .iter()
        .map(|(path, content)| records(content, path).len() as u64)
        .sum()
}

#[test]
fn ucp_adapter_declares_versioned_spec_and_conformance() {
    let vector_count = suite_vector_count();
    let version = SpecVersion::parse("20260408").unwrap_or_else(|error| panic!("version: {error}"));
    let adapter_id = AdapterId::new("ucp").unwrap_or_else(|error| panic!("adapter id: {error}"));
    let spec = PinnedSpec::new(adapter_id.clone(), version, UCP_CHECKOUT_SPEC_SHA256)
        .unwrap_or_else(|error| panic!("spec: {error}"));
    let conformance = ConformanceSuite::new(
        AdapterId::new(SUITE).unwrap_or_else(|error| panic!("conformance id: {error}")),
        vector_count,
        suite_digest(),
    )
    .unwrap_or_else(|error| panic!("conformance: {error}"));

    let descriptor = ucp_adapter_descriptor(spec, conformance)
        .unwrap_or_else(|error| panic!("descriptor: {error}"));

    assert_eq!(descriptor.id(), &adapter_id);
    assert_eq!(descriptor.spec().version().as_str(), "20260408");
    assert_eq!(descriptor.conformance().vector_count(), vector_count);
    assert_eq!(descriptor.conformance().suite().as_str(), SUITE);
    assert_ne!(descriptor.conformance().suite_digest(), [0; 32]);
}

#[test]
fn merchant_profile_validation_refuses_invalid_urls() {
    let source = "interop/specs/conformance/ucp/merchant-profiles.json";
    for record in records(MERCHANT_PROFILES, source) {
        let name = field(&record, "name", source);
        let handler = handler_of(&record["payment_handler"], source)
            .unwrap_or_else(|error| panic!("{name}: payment handler: {error}"));
        let profile = MerchantProfile::layerx(field(&record, "rest_endpoint", source), handler);
        if valid(&record, source) {
            let profile = profile.unwrap_or_else(|error| panic!("{name}: {error}"));
            assert_eq!(
                profile.version(),
                field(&record, "expected_version", source),
                "{name}: profile revision"
            );
            let declared: Vec<&str> = record["expected_capabilities"]
                .as_array()
                .unwrap_or_else(|| panic!("{name}: every valid profile declares its capabilities"))
                .iter()
                .map(|value| {
                    value
                        .as_str()
                        .unwrap_or_else(|| panic!("{name}: capability names are strings"))
                })
                .collect();
            let published: Vec<&str> = profile
                .capabilities()
                .iter()
                .map(Capability::name)
                .collect();
            assert_eq!(published, declared, "{name}: published capabilities");
            assert_eq!(profile.payment_handlers().len(), 1, "{name}: one handler");
        } else {
            assert_eq!(profile.err(), Some(refusal(&record, source)), "{name}");
        }
    }
}

#[test]
fn capability_names_follow_reverse_domain_convention() {
    let source = "interop/specs/conformance/ucp/capabilities.json";
    let mut covered = Vec::new();
    for record in records(CAPABILITIES, source) {
        let name = field(&record, "name", source);
        let declared = field(&record, "capability", source);
        let capability = Capability::new(
            declared,
            field(&record, "version", source),
            field(&record, "spec", source),
            field(&record, "schema", source),
        );
        if valid(&record, source) {
            let capability = capability.unwrap_or_else(|error| panic!("{name}: {error}"));
            let prefix = field(&record, "reverse_domain_prefix", source);
            assert!(
                declared.starts_with(prefix),
                "{name}: {declared} does not follow the {prefix} convention"
            );
            assert_eq!(capability.name(), declared, "{name}");
            assert_eq!(
                capability.version(),
                field(&record, "version", source),
                "{name}"
            );
            assert_eq!(capability.spec(), field(&record, "spec", source), "{name}");
            assert_eq!(
                capability.schema(),
                field(&record, "schema", source),
                "{name}"
            );
            covered.push(declared.to_owned());
        } else {
            assert_eq!(capability.err(), Some(refusal(&record, source)), "{name}");
        }
    }
    for capability in [CHECKOUT_CAPABILITY, ORDER_CAPABILITY] {
        assert!(
            covered.iter().any(|declared| declared == capability),
            "{source}: {capability} is not covered"
        );
    }
}

#[test]
fn idempotency_keys_parse_exact_uuid_format() {
    let source = "interop/specs/conformance/ucp/idempotency-keys.json";
    let mut gateway_keys = Vec::new();
    for record in records(IDEMPOTENCY_KEYS, source) {
        let name = field(&record, "name", source);
        let parsed = UcpIdempotencyKey::parse(field(&record, "key", source));
        if valid(&record, source) {
            let key = parsed.unwrap_or_else(|error| panic!("{name}: {error}"));
            assert_ne!(key.gateway_key(), [0; 32], "{name}");
            gateway_keys.push(key.gateway_key());
        } else {
            assert_eq!(parsed.err(), Some(refusal(&record, source)), "{name}");
        }
    }
    let distinct: std::collections::BTreeSet<[u8; 32]> = gateway_keys.iter().copied().collect();
    assert_eq!(
        distinct.len(),
        gateway_keys.len(),
        "{source}: distinct idempotency keys must not collide"
    );
}

#[test]
fn payment_handler_digest_is_stable_and_collision_resistant() {
    let source = "interop/specs/conformance/ucp/payment-handlers.json";
    let mut groups: BTreeMap<String, Vec<[u8; 32]>> = BTreeMap::new();
    for record in records(PAYMENT_HANDLERS, source) {
        let name = field(&record, "name", source);
        let handler = handler_of(&record, source);
        if valid(&record, source) {
            let handler = handler.unwrap_or_else(|error| panic!("{name}: {error}"));
            assert_eq!(handler.id(), field(&record, "id", source), "{name}");
            assert_eq!(
                handler.version(),
                field(&record, "version", source),
                "{name}"
            );
            groups
                .entry(field(&record, "digest_group", source).to_owned())
                .or_default()
                .push(handler.payment_handler_digest());
        } else {
            assert_eq!(handler.err(), Some(refusal(&record, source)), "{name}");
        }
    }
    assert!(
        groups.len() > 1,
        "{source}: collision resistance needs more than one handler"
    );
    for (group, digests) in &groups {
        let first = digests
            .first()
            .unwrap_or_else(|| panic!("{source}: {group} carries no handler"));
        for digest in digests {
            assert_eq!(
                digest, first,
                "{source}: identical handlers must produce identical digests"
            );
        }
    }
    let distinct: std::collections::BTreeSet<[u8; 32]> = groups
        .values()
        .filter_map(|digests| digests.first().copied())
        .collect();
    assert_eq!(
        distinct.len(),
        groups.len(),
        "{source}: different handlers must produce different digests"
    );
}

#[test]
fn conformance_vectors_cover_all_status_transitions() {
    let source = "interop/specs/conformance/ucp/checkout-status.json";
    let mut covered = Vec::new();
    for record in records(CHECKOUT_STATUS, source) {
        let declared = field(&record, "status", source);
        let status = match declared {
            "incomplete" => CheckoutStatus::Incomplete,
            "requires_escalation" => CheckoutStatus::RequiresEscalation,
            "ready_for_complete" => CheckoutStatus::ReadyForComplete,
            "complete_in_progress" => CheckoutStatus::CompleteInProgress,
            "completed" => CheckoutStatus::Completed,
            "canceled" => CheckoutStatus::Canceled,
            other => panic!("{source}: {other} is not a UCP checkout status"),
        };
        assert!(
            !field(&record, "meaning", source).is_empty(),
            "{source}: {declared} carries no meaning"
        );
        assert!(
            !covered.contains(&status),
            "{source}: {declared} is declared twice"
        );
        covered.push(status);
    }
    for status in [
        CheckoutStatus::Incomplete,
        CheckoutStatus::RequiresEscalation,
        CheckoutStatus::ReadyForComplete,
        CheckoutStatus::CompleteInProgress,
        CheckoutStatus::Completed,
        CheckoutStatus::Canceled,
    ] {
        assert!(
            covered.contains(&status),
            "{source}: the UCP status vocabulary is not fully covered: {status:?}"
        );
    }
    assert_eq!(
        covered.len(),
        6,
        "UCP status vocabulary declares 6 states; conformance must cover all"
    );
}

#[test]
fn ucp_codify_anchor_remains_stable() {
    assert_eq!(interop_ucp(), "ucp-2026-04-08-receipt-backed-commerce");
}

#[test]
fn the_vendored_revision_is_the_declared_capability_revision() {
    assert_eq!(UCP_VERSION, "2026-04-08");
    let capability = Capability::new(
        CHECKOUT_CAPABILITY,
        UCP_VERSION,
        "https://ucp.dev/2026-04-08/specification/checkout",
        "https://ucp.dev/2026-04-08/schemas/shopping/checkout.json",
    )
    .unwrap_or_else(|error| panic!("capability: {error}"));
    assert_eq!(capability.version(), UCP_VERSION);
}
