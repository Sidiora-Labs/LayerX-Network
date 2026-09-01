//! End-to-end buyer role tests against real service types. These tests verify
//! offer parsing, payment construction through typed plane paths, extension
//! echoing, receipt capture, and evidence-backed settlement verification.

use std::collections::BTreeMap;

use base64::engine::general_purpose::STANDARD;
use base64::Engine as _;
use layerx_interop_gateway::trace::TraceId;
use layerx_proof::receipt::AuthorizedBatch;
use layerx_x402::buyer::{
    BuiltPayment, Buyer, BuyerPaymentPlane, PaymentBuildRequest, SupportedKind,
};
use layerx_x402::model::{
    AtomicAmount, PaymentPayload, PaymentRequired, PaymentRequirements, ResourceInfo,
    SettlementResponse, X402Error, X402_VERSION,
};
use serde_json::{json, Value};

struct TestBuyerPlane {
    payload: Value,
}

impl BuyerPaymentPlane for TestBuyerPlane {
    fn construct(&mut self, _request: PaymentBuildRequest) -> Result<Value, X402Error> {
        Ok(self.payload.clone())
    }
}

fn test_supported() -> Vec<SupportedKind> {
    vec![
        SupportedKind {
            scheme: "exact".to_owned(),
            network: "layerx:testnet".to_owned(),
        },
        SupportedKind {
            scheme: "402lxp".to_owned(),
            network: "layerx:mainnet".to_owned(),
        },
    ]
}

fn test_requirements() -> PaymentRequirements {
    PaymentRequirements {
        scheme: "exact".to_owned(),
        network: "layerx:testnet".to_owned(),
        amount: AtomicAmount::from_u128(500),
        asset: "0x".to_owned() + &"12".repeat(32),
        pay_to: "0x".to_owned() + &"34".repeat(32),
        max_timeout_seconds: 90,
        extra: None,
    }
}

fn test_payment_required() -> PaymentRequired {
    PaymentRequired {
        x402_version: X402_VERSION,
        error: None,
        resource: ResourceInfo {
            url: "https://service.example/content".to_owned(),
            description: Some("Protected content".to_owned()),
            mime_type: Some("application/json".to_owned()),
            service_name: Some("Content Service".to_owned()),
            tags: vec!["content".to_owned()],
            icon_url: None,
        },
        accepts: vec![test_requirements()],
        extensions: BTreeMap::new(),
    }
}

fn encode_payment_required(required: &PaymentRequired) -> String {
    STANDARD.encode(serde_json::to_vec(required).unwrap())
}

fn mock_settlement_response() -> SettlementResponse {
    let mut extensions = BTreeMap::new();
    extensions.insert(
        "layerx".to_owned(),
        json!({
            "receipt": STANDARD.encode(mock_receipt_bytes()),
            "receiptDigest": "ab".repeat(32),
            "verificationLevel": "sequencer-signed"
        }),
    );

    SettlementResponse {
        success: true,
        error_reason: None,
        payer: Some("0x".to_owned() + &"56".repeat(32)),
        transaction: format!("lxp:{}", "ab".repeat(32)),
        network: "layerx:testnet".to_owned(),
        amount: Some(AtomicAmount::from_u128(500)),
        extensions,
    }
}

fn mock_receipt_bytes() -> Vec<u8> {
    vec![
        0x00, 0x01, 0x02, 0x03, 0x04, 0x05, 0x06, 0x07, 0x08, 0x09, 0x0a, 0x0b, 0x0c, 0x0d, 0x0e,
        0x0f,
    ]
}

fn mock_authorized_batch() -> AuthorizedBatch {
    AuthorizedBatch::new([1; 32], [0x12; 32], [2; 32], [3; 32], [4; 32])
}

#[test]
fn buyer_validates_supported_kinds_on_construction() {
    let valid = test_supported();
    assert!(Buyer::new(valid).is_ok());

    assert!(Buyer::new(vec![]).is_err());

    let duplicate = vec![
        SupportedKind {
            scheme: "exact".to_owned(),
            network: "layerx:testnet".to_owned(),
        },
        SupportedKind {
            scheme: "exact".to_owned(),
            network: "layerx:testnet".to_owned(),
        },
    ];
    assert!(Buyer::new(duplicate).is_err());

    let too_many = (0..100)
        .map(|i| SupportedKind {
            scheme: format!("scheme{i}"),
            network: "layerx:testnet".to_owned(),
        })
        .collect();
    assert!(Buyer::new(too_many).is_err());
}

#[test]
fn buyer_refuses_unsupported_offer() {
    let supported = test_supported();
    let buyer = Buyer::new(supported).expect("valid supported");

    let mut required = test_payment_required();
    required.accepts[0].scheme = "unsupported".to_owned();

    let encoded = encode_payment_required(&required);
    let mut plane = TestBuyerPlane {
        payload: json!({"scheme": "exact"}),
    };
    let trace = TraceId::mint([0xab; 16]);

    let result = buyer.build_payment(&encoded, [1; 32], &mut plane, &trace);

    assert!(result.is_err());
}

#[test]
fn buyer_selects_first_supported_offer_in_seller_order() {
    let supported = test_supported();
    let buyer = Buyer::new(supported).expect("valid supported");

    let mut required = test_payment_required();
    let unsupported = PaymentRequirements {
        scheme: "unsupported".to_owned(),
        network: "layerx:testnet".to_owned(),
        amount: AtomicAmount::from_u128(100),
        asset: "0x".to_owned() + &"aa".repeat(32),
        pay_to: "0x".to_owned() + &"bb".repeat(32),
        max_timeout_seconds: 60,
        extra: None,
    };
    let exact_supported = test_requirements();

    required.accepts = vec![unsupported, exact_supported.clone()];

    let encoded = encode_payment_required(&required);
    let mut plane = TestBuyerPlane {
        payload: json!({"authorization": "test"}),
    };
    let trace = TraceId::mint([0xab; 16]);

    let payment = buyer
        .build_payment(&encoded, [1; 32], &mut plane, &trace)
        .expect("payment built");

    assert_eq!(payment.payload.accepted.scheme, "exact");
    assert_eq!(payment.payload.accepted.network, "layerx:testnet");
}

#[test]
fn buyer_echoes_required_extensions_byte_for_value() {
    let supported = test_supported();
    let buyer = Buyer::new(supported).expect("valid supported");

    let mut required = test_payment_required();
    required.extensions.insert(
        "custom".to_owned(),
        layerx_x402::model::Extension {
            info: json!({"key": "value", "number": 42}),
            schema: json!({"type": "object"}),
        },
    );

    let encoded = encode_payment_required(&required);
    let mut plane = TestBuyerPlane {
        payload: json!({"authorization": "test"}),
    };
    let trace = TraceId::mint([0xab; 16]);

    let payment = buyer
        .build_payment(&encoded, [1; 32], &mut plane, &trace)
        .expect("payment built");

    assert_eq!(payment.payload.extensions, required.extensions);
}

#[test]
fn buyer_refuses_zero_idempotency_key() {
    let supported = test_supported();
    let buyer = Buyer::new(supported).expect("valid supported");

    let required = test_payment_required();
    let encoded = encode_payment_required(&required);
    let mut plane = TestBuyerPlane {
        payload: json!({"authorization": "test"}),
    };
    let trace = TraceId::mint([0xab; 16]);

    let result = buyer.build_payment(&encoded, [0; 32], &mut plane, &trace);

    assert!(result.is_err());
}

#[test]
fn buyer_validates_payment_required_header_before_parsing() {
    let supported = test_supported();
    let buyer = Buyer::new(supported).expect("valid supported");

    let invalid_header = "not-valid-base64!";
    let mut plane = TestBuyerPlane {
        payload: json!({"authorization": "test"}),
    };
    let trace = TraceId::mint([0xab; 16]);

    let result = buyer.build_payment(invalid_header, [1; 32], &mut plane, &trace);

    assert!(result.is_err());
}

#[test]
fn buyer_refuses_wrong_x402_version() {
    let supported = test_supported();
    let buyer = Buyer::new(supported).expect("valid supported");

    let mut required = test_payment_required();
    required.x402_version = 1;

    let encoded = encode_payment_required(&required);
    let mut plane = TestBuyerPlane {
        payload: json!({"authorization": "test"}),
    };
    let trace = TraceId::mint([0xab; 16]);

    let result = buyer.build_payment(&encoded, [1; 32], &mut plane, &trace);

    assert!(result.is_err());
}

#[test]
fn buyer_includes_resource_info_in_built_payment() {
    let supported = test_supported();
    let buyer = Buyer::new(supported).expect("valid supported");

    let required = test_payment_required();
    let encoded = encode_payment_required(&required);
    let mut plane = TestBuyerPlane {
        payload: json!({"authorization": "test"}),
    };
    let trace = TraceId::mint([0xab; 16]);

    let payment = buyer
        .build_payment(&encoded, [1; 32], &mut plane, &trace)
        .expect("payment built");

    assert!(payment.payload.resource.is_some());
    assert_eq!(payment.payload.resource.unwrap().url, required.resource.url);
}

#[test]
fn buyer_payment_header_is_base64_encoded_json() {
    let supported = test_supported();
    let buyer = Buyer::new(supported).expect("valid supported");

    let required = test_payment_required();
    let encoded = encode_payment_required(&required);
    let mut plane = TestBuyerPlane {
        payload: json!({"authorization": "test"}),
    };
    let trace = TraceId::mint([0xab; 16]);

    let payment = buyer
        .build_payment(&encoded, [1; 32], &mut plane, &trace)
        .expect("payment built");

    let decoded = STANDARD
        .decode(payment.header.as_bytes())
        .expect("valid base64");
    let parsed: PaymentPayload = serde_json::from_slice(&decoded).expect("valid payment payload");

    assert_eq!(parsed.x402_version, X402_VERSION);
    assert_eq!(parsed.accepted, test_requirements());
}

#[test]
fn buyer_plane_request_contains_all_requirements() {
    struct CaptureBuyerPlane {
        captured: Option<PaymentBuildRequest>,
    }

    impl BuyerPaymentPlane for CaptureBuyerPlane {
        fn construct(&mut self, request: PaymentBuildRequest) -> Result<Value, X402Error> {
            self.captured = Some(request);
            Ok(json!({"test": "payload"}))
        }
    }

    let supported = test_supported();
    let buyer = Buyer::new(supported).expect("valid supported");

    let required = test_payment_required();
    let encoded = encode_payment_required(&required);
    let mut plane = CaptureBuyerPlane { captured: None };
    let trace = TraceId::mint([0xab; 16]);

    let _payment = buyer
        .build_payment(&encoded, [5; 32], &mut plane, &trace)
        .expect("payment built");

    let captured = plane.captured.expect("plane was called");
    assert_eq!(captured.requirements, test_requirements());
    assert_eq!(captured.idempotency_key, [5; 32]);
}

#[test]
fn buyer_refuses_non_object_scheme_payload() {
    let supported = test_supported();
    let buyer = Buyer::new(supported).expect("valid supported");

    let required = test_payment_required();
    let encoded = encode_payment_required(&required);
    let mut plane = TestBuyerPlane {
        payload: json!("not-an-object"),
    };
    let trace = TraceId::mint([0xab; 16]);

    let result = buyer.build_payment(&encoded, [1; 32], &mut plane, &trace);

    assert!(result.is_err());
}

#[test]
fn buyer_built_payment_preserves_idempotency_key() {
    let supported = test_supported();
    let buyer = Buyer::new(supported).expect("valid supported");

    let required = test_payment_required();
    let encoded = encode_payment_required(&required);
    let mut plane = TestBuyerPlane {
        payload: json!({"authorization": "test"}),
    };
    let trace = TraceId::mint([0xab; 16]);

    let key = [7; 32];
    let payment = buyer
        .build_payment(&encoded, key, &mut plane, &trace)
        .expect("payment built");

    assert_eq!(payment.idempotency_key, key);
}

#[test]
fn buyer_capture_refuses_failed_settlement_as_success() {
    let payment = BuiltPayment {
        header: "test".to_owned(),
        payload: PaymentPayload {
            x402_version: X402_VERSION,
            resource: None,
            payload: json!({"test": "data"}),
            accepted: test_requirements(),
            extensions: BTreeMap::new(),
        },
        idempotency_key: [1; 32],
    };

    let failed = SettlementResponse {
        success: false,
        error_reason: Some("payment_refused".to_owned()),
        payer: None,
        transaction: String::new(),
        network: "layerx:testnet".to_owned(),
        amount: None,
        extensions: BTreeMap::new(),
    };

    let encoded = STANDARD.encode(serde_json::to_vec(&failed).unwrap());
    let batch = mock_authorized_batch();
    let trace = TraceId::mint([0xab; 16]);

    let result = Buyer::capture_settlement(&encoded, &payment, &batch, &trace);

    assert!(result.is_err());
}

#[test]
fn buyer_capture_refuses_missing_layerx_evidence() {
    let payment = BuiltPayment {
        header: "test".to_owned(),
        payload: PaymentPayload {
            x402_version: X402_VERSION,
            resource: None,
            payload: json!({"test": "data"}),
            accepted: test_requirements(),
            extensions: BTreeMap::new(),
        },
        idempotency_key: [1; 32],
    };

    let no_evidence = SettlementResponse {
        success: true,
        error_reason: None,
        payer: Some("0x".to_owned() + &"ab".repeat(32)),
        transaction: "test".to_owned(),
        network: "layerx:testnet".to_owned(),
        amount: Some(AtomicAmount::from_u128(500)),
        extensions: BTreeMap::new(),
    };

    let encoded = STANDARD.encode(serde_json::to_vec(&no_evidence).unwrap());
    let batch = mock_authorized_batch();
    let trace = TraceId::mint([0xab; 16]);

    let result = Buyer::capture_settlement(&encoded, &payment, &batch, &trace);

    assert!(result.is_err());
}

#[test]
fn buyer_capture_refuses_wrong_verification_level() {
    let payment = BuiltPayment {
        header: "test".to_owned(),
        payload: PaymentPayload {
            x402_version: X402_VERSION,
            resource: None,
            payload: json!({"test": "data"}),
            accepted: test_requirements(),
            extensions: BTreeMap::new(),
        },
        idempotency_key: [1; 32],
    };

    let mut extensions = BTreeMap::new();
    extensions.insert(
        "layerx".to_owned(),
        json!({
            "receipt": STANDARD.encode(mock_receipt_bytes()),
            "receiptDigest": "ab".repeat(32),
            "verificationLevel": "unverified"
        }),
    );

    let wrong_level = SettlementResponse {
        success: true,
        error_reason: None,
        payer: Some("0x".to_owned() + &"ab".repeat(32)),
        transaction: "test".to_owned(),
        network: "layerx:testnet".to_owned(),
        amount: Some(AtomicAmount::from_u128(500)),
        extensions,
    };

    let encoded = STANDARD.encode(serde_json::to_vec(&wrong_level).unwrap());
    let batch = mock_authorized_batch();
    let trace = TraceId::mint([0xab; 16]);

    let result = Buyer::capture_settlement(&encoded, &payment, &batch, &trace);

    assert!(result.is_err());
}

#[test]
fn buyer_capture_refuses_malformed_receipt() {
    let payment = BuiltPayment {
        header: "test".to_owned(),
        payload: PaymentPayload {
            x402_version: X402_VERSION,
            resource: None,
            payload: json!({"test": "data"}),
            accepted: test_requirements(),
            extensions: BTreeMap::new(),
        },
        idempotency_key: [1; 32],
    };

    let mut extensions = BTreeMap::new();
    extensions.insert(
        "layerx".to_owned(),
        json!({
            "receipt": "not-valid-base64!",
            "receiptDigest": "ab".repeat(32),
            "verificationLevel": "sequencer-signed"
        }),
    );

    let bad_receipt = SettlementResponse {
        success: true,
        error_reason: None,
        payer: Some("0x".to_owned() + &"ab".repeat(32)),
        transaction: "test".to_owned(),
        network: "layerx:testnet".to_owned(),
        amount: Some(AtomicAmount::from_u128(500)),
        extensions,
    };

    let encoded = STANDARD.encode(serde_json::to_vec(&bad_receipt).unwrap());
    let batch = mock_authorized_batch();
    let trace = TraceId::mint([0xab; 16]);

    let result = Buyer::capture_settlement(&encoded, &payment, &batch, &trace);

    assert!(result.is_err());
}

#[test]
fn supported_kind_equality_matches_both_scheme_and_network() {
    let kind1 = SupportedKind {
        scheme: "exact".to_owned(),
        network: "layerx:testnet".to_owned(),
    };
    let kind2 = SupportedKind {
        scheme: "exact".to_owned(),
        network: "layerx:testnet".to_owned(),
    };
    let kind3 = SupportedKind {
        scheme: "402lxp".to_owned(),
        network: "layerx:testnet".to_owned(),
    };

    assert_eq!(kind1, kind2);
    assert_ne!(kind1, kind3);
}

#[test]
fn buyer_validates_payment_payload_after_construction() {
    let supported = test_supported();
    let buyer = Buyer::new(supported).expect("valid supported");

    let required = test_payment_required();
    let encoded = encode_payment_required(&required);
    let mut plane = TestBuyerPlane {
        payload: json!({"valid": "object"}),
    };
    let trace = TraceId::mint([0xab; 16]);

    let payment = buyer
        .build_payment(&encoded, [1; 32], &mut plane, &trace)
        .expect("payment built");

    assert!(payment.payload.validate().is_ok());
}
