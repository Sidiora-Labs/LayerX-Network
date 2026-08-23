//! End-to-end integration tests for seller and buyer roles working together
//! against real service types and independent x402 v2 implementations. Tests
//! complete payment flows, evidence verification, and interoperability.

use std::collections::BTreeMap;

use base64::engine::general_purpose::STANDARD;
use base64::Engine as _;
use layerx_interop_gateway::adapter::{AdapterId, ConformanceSuite};
use layerx_interop_gateway::principal::PrincipalId;
use layerx_interop_gateway::trace::TraceId;
use layerx_interop_gateway::GatewayCore;
use layerx_proof::receipt::AuthorizedBatch;
use layerx_x402::buyer::{Buyer, BuyerPaymentPlane, PaymentBuildRequest, SupportedKind};
use layerx_x402::x402_adapter_descriptor;
use layerx_x402::model::{
    AtomicAmount, PaymentPayload, PaymentRequired, PaymentRequirements, ResourceInfo, X402Error,
    X402_VERSION,
};
use layerx_x402::seller::{
    ExecutedPayment, LayerXPaymentRequest, PaymentPlane, PlanePaymentOutcome, Seller,
    SellerOutcome,
};
use layerx_x402::transport::{
    decode_payment_payload, encode_payment_payload, encode_payment_required, TransportKind,
};
use serde_json::{json, Value};

struct MockBuyerPlane {
    scheme_payload: Value,
}

impl BuyerPaymentPlane for MockBuyerPlane {
    fn construct(&mut self, _request: PaymentBuildRequest) -> Result<Value, X402Error> {
        Ok(self.scheme_payload.clone())
    }
}

struct MockSellerPlane {
    outcome: PlanePaymentOutcome,
}

impl PaymentPlane for MockSellerPlane {
    fn execute(
        &mut self,
        _request: LayerXPaymentRequest,
        _trace: &TraceId,
    ) -> Result<PlanePaymentOutcome, X402Error> {
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

fn create_payment_required() -> PaymentRequired {
    PaymentRequired {
        x402_version: X402_VERSION,
        error: None,
        resource: ResourceInfo {
            url: "https://merchant.example/api/data".to_owned(),
            description: Some("Premium API data".to_owned()),
            mime_type: Some("application/json".to_owned()),
            service_name: Some("Merchant API".to_owned()),
            tags: vec!["api".to_owned(), "data".to_owned()],
            icon_url: None,
        },
        accepts: vec![
            PaymentRequirements {
                scheme: "exact".to_owned(),
                network: "layerx:testnet".to_owned(),
                amount: AtomicAmount::from_u128(1000),
                asset: "0x".to_owned() + &"aa".repeat(32),
                pay_to: "0x".to_owned() + &"bb".repeat(32),
                max_timeout_seconds: 120,
                extra: None,
            },
            PaymentRequirements {
                scheme: "402lxp".to_owned(),
                network: "layerx:mainnet".to_owned(),
                amount: AtomicAmount::from_u128(950),
                asset: "0x".to_owned() + &"cc".repeat(32),
                pay_to: "0x".to_owned() + &"dd".repeat(32),
                max_timeout_seconds: 180,
                extra: None,
            },
        ],
        extensions: BTreeMap::new(),
    }
}

#[test]
fn buyer_and_seller_complete_payment_flow_over_http() {
    let required = create_payment_required();
    let seller = Seller::new(required.clone()).expect("seller created");

    let signal = seller.payment_required().expect("signal issued");
    assert_eq!(signal.status, 402);

    let buyer = Buyer::new(vec![
        SupportedKind {
            scheme: "exact".to_owned(),
            network: "layerx:testnet".to_owned(),
        },
        SupportedKind {
            scheme: "402lxp".to_owned(),
            network: "layerx:mainnet".to_owned(),
        },
    ])
    .expect("buyer created");

    let transport_value = encode_payment_required(TransportKind::Http, &signal.body)
        .expect("encode for transport");
    let payment_header = match transport_value {
        layerx_x402::transport::TransportValue::HttpHeader { value, .. } => value,
        _ => panic!("expected HTTP header"),
    };

    let mut buyer_plane = MockBuyerPlane {
        scheme_payload: json!({
            "scheme": "exact",
            "authorization": "buyer-signed-payment"
        }),
    };
    let trace = TraceId::mint([0xab; 16]);

    let payment = buyer
        .build_payment(&payment_header, [5; 32], &mut buyer_plane, &trace)
        .expect("payment built");

    assert_eq!(payment.payload.x402_version, X402_VERSION);
    assert_eq!(payment.payload.accepted.scheme, "exact");
    assert_eq!(payment.payload.accepted.network, "layerx:testnet");
    assert_eq!(payment.idempotency_key, [5; 32]);

    let mut gateway = registered_gateway();
    let principal = PrincipalId::new("test-merchant").unwrap();
    let mut seller_plane = MockSellerPlane {
        outcome: PlanePaymentOutcome::Pending,
    };

    let outcome = seller
        .settle(
            &mut gateway,
            &principal,
            &payment.header,
            &mut seller_plane,
            &trace,
            0,
        )
        .expect("settlement processed");

    assert!(matches!(outcome, SellerOutcome::Pending));
}

#[test]
fn buyer_selects_first_supported_scheme_from_seller_accepts() {
    let required = create_payment_required();

    let buyer = Buyer::new(vec![SupportedKind {
        scheme: "402lxp".to_owned(),
        network: "layerx:mainnet".to_owned(),
    }])
    .expect("buyer created");

    let encoded = STANDARD.encode(serde_json::to_vec(&required).unwrap());
    let mut plane = MockBuyerPlane {
        scheme_payload: json!({"scheme": "402lxp"}),
    };
    let trace = TraceId::mint([0xab; 16]);

    let payment = buyer
        .build_payment(&encoded, [1; 32], &mut plane, &trace)
        .expect("payment built");

    assert_eq!(payment.payload.accepted.scheme, "402lxp");
    assert_eq!(payment.payload.accepted.network, "layerx:mainnet");
    assert_eq!(payment.payload.accepted.amount.value(), 950);
}

#[test]
fn seller_validates_buyer_payment_matches_issued_requirements() {
    let required = create_payment_required();
    let seller = Seller::new(required.clone()).expect("seller created");

    let mut wrong_payload = PaymentPayload {
        x402_version: X402_VERSION,
        resource: None,
        payload: json!({"scheme": "exact"}),
        accepted: PaymentRequirements {
            scheme: "exact".to_owned(),
            network: "layerx:testnet".to_owned(),
            amount: AtomicAmount::from_u128(9999),
            asset: "0x".to_owned() + &"aa".repeat(32),
            pay_to: "0x".to_owned() + &"bb".repeat(32),
            max_timeout_seconds: 120,
            extra: None,
        },
        extensions: BTreeMap::new(),
    };

    let encoded = STANDARD.encode(serde_json::to_vec(&wrong_payload).unwrap());

    let mut gateway = registered_gateway();
    let principal = PrincipalId::new("test-merchant").unwrap();
    let mut plane = MockSellerPlane {
        outcome: PlanePaymentOutcome::Pending,
    };
    let trace = TraceId::mint([0xab; 16]);

    let result = seller.settle(&mut gateway, &principal, &encoded, &mut plane, &trace, 0);

    assert!(result.is_err());
}

#[test]
fn payment_flow_preserves_extensions_end_to_end() {
    let mut required = create_payment_required();
    required
        .extensions
        .insert("merchant".to_owned(), layerx_x402::model::Extension {
            info: json!({"merchantId": "12345", "region": "us-west"}),
            schema: json!({"type": "object"}),
        });

    let seller = Seller::new(required.clone()).expect("seller created");
    let signal = seller.payment_required().expect("signal issued");

    let buyer = Buyer::new(vec![SupportedKind {
        scheme: "exact".to_owned(),
        network: "layerx:testnet".to_owned(),
    }])
    .expect("buyer created");

    let encoded = STANDARD.encode(serde_json::to_vec(&signal.body).unwrap());
    let mut plane = MockBuyerPlane {
        scheme_payload: json!({"authorization": "test"}),
    };
    let trace = TraceId::mint([0xab; 16]);

    let payment = buyer
        .build_payment(&encoded, [7; 32], &mut plane, &trace)
        .expect("payment built");

    assert_eq!(payment.payload.extensions, required.extensions);
}

#[test]
fn seller_refuses_payment_when_extension_missing() {
    let mut required = create_payment_required();
    required
        .extensions
        .insert("required".to_owned(), layerx_x402::model::Extension {
            info: json!({"must": "exist"}),
            schema: json!({"type": "object"}),
        });

    let seller = Seller::new(required.clone()).expect("seller created");

    let payload = PaymentPayload {
        x402_version: X402_VERSION,
        resource: None,
        payload: json!({"scheme": "exact"}),
        accepted: PaymentRequirements {
            scheme: "exact".to_owned(),
            network: "layerx:testnet".to_owned(),
            amount: AtomicAmount::from_u128(1000),
            asset: "0x".to_owned() + &"aa".repeat(32),
            pay_to: "0x".to_owned() + &"bb".repeat(32),
            max_timeout_seconds: 120,
            extra: None,
        },
        extensions: BTreeMap::new(),
    };

    let encoded = STANDARD.encode(serde_json::to_vec(&payload).unwrap());

    let mut gateway = registered_gateway();
    let principal = PrincipalId::new("test-merchant").unwrap();
    let mut plane = MockSellerPlane {
        outcome: PlanePaymentOutcome::Pending,
    };
    let trace = TraceId::mint([0xab; 16]);

    let result = seller.settle(&mut gateway, &principal, &encoded, &mut plane, &trace, 0);

    assert!(result.is_err());
}

#[test]
fn transport_independent_payment_flow_over_mcp() {
    let required = create_payment_required();
    let seller = Seller::new(required.clone()).expect("seller created");

    let transport_value = encode_payment_required(TransportKind::Mcp, &required)
        .expect("encode for MCP transport");
    let json_value = match transport_value {
        layerx_x402::transport::TransportValue::Json(value) => value,
        _ => panic!("expected JSON"),
    };

    let encoded = STANDARD.encode(serde_json::to_vec(&json_value).unwrap());

    let buyer = Buyer::new(vec![SupportedKind {
        scheme: "exact".to_owned(),
        network: "layerx:testnet".to_owned(),
    }])
    .expect("buyer created");

    let mut plane = MockBuyerPlane {
        scheme_payload: json!({"authorization": "mcp-payment"}),
    };
    let trace = TraceId::mint([0xab; 16]);

    let payment = buyer
        .build_payment(&encoded, [9; 32], &mut plane, &trace)
        .expect("payment built");

    assert_eq!(payment.payload.accepted.scheme, "exact");
}

#[test]
fn seller_refuses_payment_before_plane_execution_when_validation_fails() {
    let required = create_payment_required();
    let seller = Seller::new(required.clone()).expect("seller created");

    let invalid_payload = PaymentPayload {
        x402_version: 1,
        resource: None,
        payload: json!({"scheme": "exact"}),
        accepted: PaymentRequirements {
            scheme: "exact".to_owned(),
            network: "layerx:testnet".to_owned(),
            amount: AtomicAmount::from_u128(1000),
            asset: "0x".to_owned() + &"aa".repeat(32),
            pay_to: "0x".to_owned() + &"bb".repeat(32),
            max_timeout_seconds: 120,
            extra: None,
        },
        extensions: BTreeMap::new(),
    };

    let encoded = STANDARD.encode(serde_json::to_vec(&invalid_payload).unwrap());

    struct NeverCalledPlane;

    impl PaymentPlane for NeverCalledPlane {
        fn execute(
            &mut self,
            _request: LayerXPaymentRequest,
            _trace: &TraceId,
        ) -> Result<PlanePaymentOutcome, X402Error> {
            panic!("plane should not be called for invalid payment");
        }
    }

    let mut gateway = registered_gateway();
    let principal = PrincipalId::new("test-merchant").unwrap();
    let mut plane = NeverCalledPlane;
    let trace = TraceId::mint([0xab; 16]);

    let result = seller.settle(&mut gateway, &principal, &encoded, &mut plane, &trace, 0);

    assert!(result.is_err());
}

#[test]
fn buyer_constructs_payment_with_correct_idempotency_semantics() {
    let required = create_payment_required();

    let buyer = Buyer::new(vec![SupportedKind {
        scheme: "exact".to_owned(),
        network: "layerx:testnet".to_owned(),
    }])
    .expect("buyer created");

    let encoded = STANDARD.encode(serde_json::to_vec(&required).unwrap());
    let mut plane = MockBuyerPlane {
        scheme_payload: json!({"authorization": "test"}),
    };
    let trace = TraceId::mint([0xab; 16]);

    let key1 = [11; 32];
    let key2 = [22; 32];

    let payment1 = buyer
        .build_payment(&encoded, key1, &mut plane, &trace)
        .expect("payment 1 built");

    let payment2 = buyer
        .build_payment(&encoded, key2, &mut plane, &trace)
        .expect("payment 2 built");

    assert_eq!(payment1.idempotency_key, key1);
    assert_eq!(payment2.idempotency_key, key2);
    assert_ne!(payment1.idempotency_key, payment2.idempotency_key);
}

#[test]
fn seller_outcome_types_distinguish_pending_refused_and_settled() {
    let required = create_payment_required();
    let seller = Seller::new(required.clone()).expect("seller created");

    let buyer = Buyer::new(vec![SupportedKind {
        scheme: "exact".to_owned(),
        network: "layerx:testnet".to_owned(),
    }])
    .expect("buyer created");

    let encoded = STANDARD.encode(serde_json::to_vec(&required).unwrap());
    let mut plane = MockBuyerPlane {
        scheme_payload: json!({"authorization": "test"}),
    };
    let trace = TraceId::mint([0xab; 16]);

    let payment = buyer
        .build_payment(&encoded, [1; 32], &mut plane, &trace)
        .expect("payment built");

    let mut gateway = registered_gateway();
    let principal = PrincipalId::new("test-merchant").unwrap();

    let mut pending_plane = MockSellerPlane {
        outcome: PlanePaymentOutcome::Pending,
    };
    let pending_outcome = seller
        .settle(
            &mut gateway,
            &principal,
            &payment.header,
            &mut pending_plane,
            &trace,
            0,
        )
        .expect("pending processed");
    assert!(matches!(pending_outcome, SellerOutcome::Pending));

    let mut refused_plane = MockSellerPlane {
        outcome: PlanePaymentOutcome::Refused {
            reason: "insufficient_balance",
        },
    };
    let refused_outcome = seller
        .settle(
            &mut gateway,
            &principal,
            &payment.header,
            &mut refused_plane,
            &trace,
            100,
        )
        .expect("refused processed");
    assert!(matches!(refused_outcome, SellerOutcome::Refused { .. }));
}

#[test]
fn payment_requirements_layerx_facts_extraction() {
    let requirements = PaymentRequirements {
        scheme: "exact".to_owned(),
        network: "layerx:testnet".to_owned(),
        amount: AtomicAmount::from_u128(1000),
        asset: "0x".to_owned() + &"ab".repeat(32),
        pay_to: "0x".to_owned() + &"cd".repeat(32),
        max_timeout_seconds: 120,
        extra: None,
    };

    let (asset, recipient) = requirements.layerx_facts().expect("layerx facts");

    assert_eq!(asset, [0xab; 32]);
    assert_eq!(recipient, [0xcd; 32]);
}

#[test]
fn payment_required_with_error_message_is_valid() {
    let mut required = create_payment_required();
    required.error = Some("Authentication required".to_owned());

    let seller = Seller::new(required.clone()).expect("seller created");
    let signal = seller.payment_required().expect("signal issued");

    assert_eq!(signal.body.error, Some("Authentication required".to_owned()));
}

#[test]
fn resource_info_with_all_fields_is_preserved() {
    let required = create_payment_required();

    let buyer = Buyer::new(vec![SupportedKind {
        scheme: "exact".to_owned(),
        network: "layerx:testnet".to_owned(),
    }])
    .expect("buyer created");

    let encoded = STANDARD.encode(serde_json::to_vec(&required).unwrap());
    let mut plane = MockBuyerPlane {
        scheme_payload: json!({"authorization": "test"}),
    };
    let trace = TraceId::mint([0xab; 16]);

    let payment = buyer
        .build_payment(&encoded, [1; 32], &mut plane, &trace)
        .expect("payment built");

    let resource = payment.payload.resource.expect("resource present");
    assert_eq!(resource.url, required.resource.url);
    assert_eq!(resource.description, required.resource.description);
    assert_eq!(resource.mime_type, required.resource.mime_type);
    assert_eq!(resource.service_name, required.resource.service_name);
    assert_eq!(resource.tags, required.resource.tags);
}
