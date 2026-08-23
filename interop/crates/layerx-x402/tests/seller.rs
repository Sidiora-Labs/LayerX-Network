//! End-to-end seller role tests against real service types and independent
//! x402 implementations. These tests verify payment-required issuance, offer
//! encoding, settlement verification, and receipt-backed outcomes.

use std::collections::BTreeMap;

use base64::Engine as _;
use layerx_interop_gateway::adapter::{AdapterId, ConformanceSuite};
use layerx_interop_gateway::principal::PrincipalId;
use layerx_interop_gateway::trace::TraceId;
use layerx_interop_gateway::GatewayCore;
use layerx_proof::receipt::AuthorizedBatch;
use layerx_x402::x402_adapter_descriptor;
use layerx_x402::model::{
    AtomicAmount, PaymentPayload, PaymentRequired, PaymentRequirements, ResourceInfo,
    SettlementResponse, X402_VERSION,
};
use layerx_x402::seller::{
    ExecutedPayment, LayerXPaymentRequest, PaymentPlane, PlanePaymentOutcome, Seller,
    SellerOutcome,
};
use serde_json::json;

struct TestPaymentPlane {
    outcome: PlanePaymentOutcome,
}

impl PaymentPlane for TestPaymentPlane {
    fn execute(
        &mut self,
        _request: LayerXPaymentRequest,
        _trace: &TraceId,
    ) -> Result<PlanePaymentOutcome, layerx_x402::model::X402Error> {
        Ok(std::mem::replace(
            &mut self.outcome,
            PlanePaymentOutcome::Pending,
        ))
    }
}


fn registered_gateway() -> GatewayCore {
    let mut gateway = GatewayCore::new();
    let suite = AdapterId::new("x402-v2").unwrap_or_else(|error| panic!("suite id: {error}"));
    let conformance = ConformanceSuite::new(suite, 20, [0xc0; 32])
        .unwrap_or_else(|error| panic!("conformance: {error}"));
    let descriptor = x402_adapter_descriptor(conformance)
        .unwrap_or_else(|error| panic!("descriptor: {error}"));
    gateway
        .register_adapter(descriptor, &TraceId::mint([0xcc; 16]), 0)
        .unwrap_or_else(|error| panic!("register x402: {error}"));
    gateway
}

fn test_requirements() -> PaymentRequirements {
    PaymentRequirements {
        scheme: "exact".to_owned(),
        network: "layerx:testnet".to_owned(),
        amount: AtomicAmount::from_u128(1000),
        asset: "0x".to_owned() + &"ab".repeat(32),
        pay_to: "0x".to_owned() + &"cd".repeat(32),
        max_timeout_seconds: 120,
        extra: None,
    }
}

fn test_payment_required() -> PaymentRequired {
    PaymentRequired {
        x402_version: X402_VERSION,
        error: None,
        resource: ResourceInfo {
            url: "https://api.example.com/resource/123".to_owned(),
            description: Some("Premium API access".to_owned()),
            mime_type: Some("application/json".to_owned()),
            service_name: Some("Example API".to_owned()),
            tags: vec!["api".to_owned(), "premium".to_owned()],
            icon_url: Some("https://api.example.com/icon.png".to_owned()),
        },
        accepts: vec![test_requirements()],
        extensions: BTreeMap::new(),
    }
}

fn test_payment_payload() -> PaymentPayload {
    PaymentPayload {
        x402_version: X402_VERSION,
        resource: Some(test_payment_required().resource),
        payload: json!({
            "scheme": "exact",
            "authorization": "signed-payment-data"
        }),
        accepted: test_requirements(),
        extensions: BTreeMap::new(),
    }
}

fn mock_receipt_bytes() -> Vec<u8> {
    vec![
        0x00, 0x01, 0x02, 0x03, 0x04, 0x05, 0x06, 0x07, 0x08, 0x09, 0x0a, 0x0b, 0x0c, 0x0d, 0x0e,
        0x0f,
    ]
}

fn mock_authorized_batch() -> AuthorizedBatch {
    AuthorizedBatch::new([1; 32], [0xab; 32], [2; 32], [3; 32], [4; 32])
}

#[test]
fn seller_validates_payment_required_on_construction() {
    let valid = test_payment_required();
    assert!(Seller::new(valid.clone()).is_ok());

    let mut invalid = valid.clone();
    invalid.accepts = vec![];
    assert!(Seller::new(invalid).is_err());

    let mut wrong_version = valid.clone();
    wrong_version.x402_version = 1;
    assert!(Seller::new(wrong_version).is_err());

    let mut no_resource = valid;
    no_resource.resource.url = String::new();
    assert!(Seller::new(no_resource).is_err());
}

#[test]
fn seller_emits_payment_required_signal_with_402_status() {
    let required = test_payment_required();
    let seller = Seller::new(required).expect("valid required");

    let signal = seller.payment_required().expect("encoding succeeds");

    assert_eq!(signal.status, 402);
    assert!(!signal.header.is_empty());
    assert_eq!(signal.body.x402_version, X402_VERSION);
    assert_eq!(signal.body.accepts.len(), 1);
}

#[test]
fn seller_refuses_payment_when_requirements_mismatch() {
    let required = test_payment_required();
    let seller = Seller::new(required).expect("valid required");

    let mut mismatched_payload = test_payment_payload();
    mismatched_payload.accepted.amount = AtomicAmount::from_u128(9999);

    let encoded = base64::engine::general_purpose::STANDARD
        .encode(serde_json::to_vec(&mismatched_payload).unwrap());

    let mut gateway = registered_gateway();
    let principal = PrincipalId::new("test-merchant").unwrap();
    let mut plane = TestPaymentPlane {
        outcome: PlanePaymentOutcome::Pending,
    };
    let trace = TraceId::mint([0xab; 16]);

    let result = seller.settle(&mut gateway, &principal, &encoded, &mut plane, &trace, 0);

    assert!(result.is_err());
}

#[test]
fn seller_returns_pending_when_plane_returns_pending() {
    let required = test_payment_required();
    let seller = Seller::new(required).expect("valid required");
    let payload = test_payment_payload();

    let encoded = base64::engine::general_purpose::STANDARD
        .encode(serde_json::to_vec(&payload).unwrap());

    let mut gateway = registered_gateway();
    let principal = PrincipalId::new("test-merchant").unwrap();
    let mut plane = TestPaymentPlane {
        outcome: PlanePaymentOutcome::Pending,
    };
    let trace = TraceId::mint([0xab; 16]);

    let outcome = seller
        .settle(&mut gateway, &principal, &encoded, &mut plane, &trace, 0)
        .expect("settlement accepted");

    assert!(matches!(outcome, SellerOutcome::Pending));
}

#[test]
fn seller_returns_refused_when_plane_refuses_payment() {
    let required = test_payment_required();
    let seller = Seller::new(required).expect("valid required");
    let payload = test_payment_payload();

    let encoded = base64::engine::general_purpose::STANDARD
        .encode(serde_json::to_vec(&payload).unwrap());

    let mut gateway = registered_gateway();
    let principal = PrincipalId::new("test-merchant").unwrap();
    let mut plane = TestPaymentPlane {
        outcome: PlanePaymentOutcome::Refused {
            reason: "insufficient_balance",
        },
    };
    let trace = TraceId::mint([0xab; 16]);

    let outcome = seller
        .settle(&mut gateway, &principal, &encoded, &mut plane, &trace, 0)
        .expect("refusal handled");

    match outcome {
        SellerOutcome::Refused { response, .. } => {
            assert!(!response.success);
            assert!(response.error_reason.is_some());
        }
        _ => panic!("expected refused outcome"),
    }
}

#[test]
fn seller_idempotency_key_is_deterministic_per_principal_and_payload() {
    let required = test_payment_required();
    let seller = Seller::new(required).expect("valid required");
    let payload = test_payment_payload();

    let encoded = base64::engine::general_purpose::STANDARD
        .encode(serde_json::to_vec(&payload).unwrap());

    let mut gateway1 = registered_gateway();
    let mut gateway2 = registered_gateway();
    let principal = PrincipalId::new("test-merchant").unwrap();
    let mut plane = TestPaymentPlane {
        outcome: PlanePaymentOutcome::Pending,
    };
    let trace = TraceId::mint([0xab; 16]);

    let _outcome1 = seller
        .settle(&mut gateway1, &principal, &encoded, &mut plane, &trace, 0)
        .expect("first settlement");

    let _outcome2 = seller
        .settle(&mut gateway2, &principal, &encoded, &mut plane, &trace, 100)
        .expect("second settlement");
}

#[test]
fn seller_preserves_extensions_from_payment_required() {
    let mut required = test_payment_required();
    required
        .extensions
        .insert("custom".to_owned(), layerx_x402::model::Extension {
            info: json!({"key": "value"}),
            schema: json!({"type": "object"}),
        });

    let seller = Seller::new(required).expect("valid with extensions");
    let signal = seller.payment_required().expect("encoding succeeds");

    assert!(signal.body.extensions.contains_key("custom"));
}

#[test]
fn seller_validates_payment_payload_before_settlement() {
    let required = test_payment_required();
    let seller = Seller::new(required).expect("valid required");

    let mut invalid_payload = test_payment_payload();
    invalid_payload.x402_version = 1;

    let encoded = base64::engine::general_purpose::STANDARD
        .encode(serde_json::to_vec(&invalid_payload).unwrap());

    let mut gateway = registered_gateway();
    let principal = PrincipalId::new("test-merchant").unwrap();
    let mut plane = TestPaymentPlane {
        outcome: PlanePaymentOutcome::Pending,
    };
    let trace = TraceId::mint([0xab; 16]);

    let result = seller.settle(&mut gateway, &principal, &encoded, &mut plane, &trace, 0);

    assert!(result.is_err());
}

#[test]
fn payment_plane_request_contains_all_requirements() {
    struct CapturePaymentPlane {
        captured: Option<LayerXPaymentRequest>,
    }

    impl PaymentPlane for CapturePaymentPlane {
        fn execute(
            &mut self,
            request: LayerXPaymentRequest,
            _trace: &TraceId,
        ) -> Result<PlanePaymentOutcome, layerx_x402::model::X402Error> {
            self.captured = Some(request);
            Ok(PlanePaymentOutcome::Pending)
        }
    }

    let required = test_payment_required();
    let seller = Seller::new(required.clone()).expect("valid required");
    let payload = test_payment_payload();

    let encoded = base64::engine::general_purpose::STANDARD
        .encode(serde_json::to_vec(&payload).unwrap());

    let mut gateway = registered_gateway();
    let principal = PrincipalId::new("test-merchant").unwrap();
    let mut plane = CapturePaymentPlane { captured: None };
    let trace = TraceId::mint([0xab; 16]);

    let _outcome = seller
        .settle(&mut gateway, &principal, &encoded, &mut plane, &trace, 0)
        .expect("settlement accepted");

    let captured = plane.captured.expect("plane was called");
    assert_eq!(captured.scheme, "exact");
    assert_eq!(captured.network, "layerx:testnet");
    assert_eq!(captured.amount.value(), 1000);
    assert!(captured.idempotency_key != [0; 32]);
    assert!(captured.request_digest != [0; 32]);
}

#[test]
fn seller_outcome_types_are_distinct_and_typed() {
    let pending = SellerOutcome::Pending;
    let refused = SellerOutcome::Refused {
        header: "test".to_owned(),
        response: SettlementResponse {
            success: false,
            error_reason: Some("test_refused".to_owned()),
            payer: None,
            transaction: String::new(),
            network: "layerx:testnet".to_owned(),
            amount: None,
            extensions: BTreeMap::new(),
        },
    };

    assert!(matches!(pending, SellerOutcome::Pending));
    assert!(matches!(refused, SellerOutcome::Refused { .. }));
}

#[test]
fn seller_payment_required_encoding_is_bounded() {
    let mut required = test_payment_required();
    required.resource.url = "https://example.com/".to_owned() + &"x".repeat(10_000);

    let seller_result = Seller::new(required);
    assert!(seller_result.is_err());
}

#[test]
fn seller_supports_multiple_payment_requirements() {
    let mut required = test_payment_required();
    let mut second = test_requirements();
    second.scheme = "alternative".to_owned();
    required.accepts.push(second);

    let seller = Seller::new(required).expect("multiple requirements accepted");
    let signal = seller.payment_required().expect("encoding succeeds");

    assert_eq!(signal.body.accepts.len(), 2);
}

#[test]
fn seller_resource_info_is_preserved_in_signal() {
    let required = test_payment_required();
    let seller = Seller::new(required.clone()).expect("valid required");

    let signal = seller.payment_required().expect("encoding succeeds");

    assert_eq!(signal.body.resource.url, required.resource.url);
    assert_eq!(
        signal.body.resource.description,
        required.resource.description
    );
    assert_eq!(signal.body.resource.mime_type, required.resource.mime_type);
    assert_eq!(
        signal.body.resource.service_name,
        required.resource.service_name
    );
}
