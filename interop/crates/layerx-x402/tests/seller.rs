//! End-to-end seller role tests against real service types and independent
//! x402 implementations. These tests verify payment-required issuance, offer
//! encoding, settlement verification, and receipt-backed outcomes.

use std::collections::BTreeMap;

use base64::Engine as _;
use layerx_interop_gateway::adapter::{AdapterId, ConformanceSuite};
use layerx_interop_gateway::principal::PrincipalId;
use layerx_interop_gateway::trace::TraceId;
use layerx_interop_gateway::GatewayCore;
use layerx_x402::model::{
    account_identifiers, AtomicAmount, PaymentPayload, PaymentRequired, PaymentRequirements,
    ResourceInfo, SettlementResponse, X402_VERSION,
};
use layerx_x402::seller::{
    LayerXPaymentRequest, PaymentPlane, PlanePaymentOutcome, Seller, SellerOutcome,
};
use layerx_x402::x402_adapter_descriptor;
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
    let descriptor =
        x402_adapter_descriptor(conformance).unwrap_or_else(|error| panic!("descriptor: {error}"));
    gateway
        .register_adapter(descriptor, &TraceId::mint([0xcc; 16]), 0)
        .unwrap_or_else(|error| panic!("register x402: {error}"));
    gateway
}

const PAYEE_ACCOUNT: &str = "agent:did:layerx:test-resource:main";
const CURRENCY: &str = "LXP";

fn account_id(account: &str) -> [u8; 32] {
    account_identifiers(account)
        .unwrap_or_else(|error| panic!("{account} has account identifiers: {error}"))[0]
}

fn pay_to(account: &str) -> String {
    const DIGITS: &[u8; 16] = b"0123456789abcdef";
    let mut text = String::with_capacity(66);
    text.push_str("0x");
    for byte in account_id(account) {
        text.push(char::from(DIGITS[usize::from(byte >> 4)]));
        text.push(char::from(DIGITS[usize::from(byte & 0x0f)]));
    }
    text
}

fn test_requirements() -> PaymentRequirements {
    PaymentRequirements {
        scheme: "exact".to_owned(),
        network: "layerx:beta".to_owned(),
        amount: AtomicAmount::from_u128(1000),
        asset: "0x".to_owned() + &"ab".repeat(32),
        pay_to: pay_to(PAYEE_ACCOUNT),
        max_timeout_seconds: 120,
        extra: Some(json!({
            "layerx": {"commitment": "executed", "account": PAYEE_ACCOUNT, "currency": CURRENCY}
        })),
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
    let seller = Seller::new(required).unwrap_or_else(|error| panic!("valid required: {error:?}"));

    let signal = seller
        .payment_required()
        .unwrap_or_else(|error| panic!("encoding succeeds: {error:?}"));

    assert_eq!(signal.status, 402);
    assert!(!signal.header.is_empty());
    assert_eq!(signal.body.x402_version, X402_VERSION);
    assert_eq!(signal.body.accepts.len(), 1);
}

#[test]
fn seller_refuses_payment_when_requirements_mismatch() {
    let required = test_payment_required();
    let seller = Seller::new(required).unwrap_or_else(|error| panic!("valid required: {error:?}"));

    let mut mismatched_payload = test_payment_payload();
    mismatched_payload.accepted.amount = AtomicAmount::from_u128(9999);

    let encoded = base64::engine::general_purpose::STANDARD.encode(
        serde_json::to_vec(&mismatched_payload)
            .unwrap_or_else(|error| panic!("test input: {error:?}")),
    );

    let mut gateway = registered_gateway();
    let principal =
        PrincipalId::new("test-merchant").unwrap_or_else(|error| panic!("test input: {error:?}"));
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
    let seller = Seller::new(required).unwrap_or_else(|error| panic!("valid required: {error:?}"));
    let payload = test_payment_payload();

    let encoded = base64::engine::general_purpose::STANDARD.encode(
        serde_json::to_vec(&payload).unwrap_or_else(|error| panic!("test input: {error:?}")),
    );

    let mut gateway = registered_gateway();
    let principal =
        PrincipalId::new("test-merchant").unwrap_or_else(|error| panic!("test input: {error:?}"));
    let mut plane = TestPaymentPlane {
        outcome: PlanePaymentOutcome::Pending,
    };
    let trace = TraceId::mint([0xab; 16]);

    let outcome = seller
        .settle(&mut gateway, &principal, &encoded, &mut plane, &trace, 0)
        .unwrap_or_else(|error| panic!("settlement accepted: {error:?}"));

    assert!(matches!(outcome, SellerOutcome::Pending));
}

#[test]
fn seller_returns_refused_when_plane_refuses_payment() {
    let required = test_payment_required();
    let seller = Seller::new(required).unwrap_or_else(|error| panic!("valid required: {error:?}"));
    let payload = test_payment_payload();

    let encoded = base64::engine::general_purpose::STANDARD.encode(
        serde_json::to_vec(&payload).unwrap_or_else(|error| panic!("test input: {error:?}")),
    );

    let mut gateway = registered_gateway();
    let principal =
        PrincipalId::new("test-merchant").unwrap_or_else(|error| panic!("test input: {error:?}"));
    let mut plane = TestPaymentPlane {
        outcome: PlanePaymentOutcome::Refused {
            reason: "insufficient_balance",
        },
    };
    let trace = TraceId::mint([0xab; 16]);

    let outcome = seller
        .settle(&mut gateway, &principal, &encoded, &mut plane, &trace, 0)
        .unwrap_or_else(|error| panic!("refusal handled: {error:?}"));

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
    let seller = Seller::new(required).unwrap_or_else(|error| panic!("valid required: {error:?}"));
    let payload = test_payment_payload();

    let encoded = base64::engine::general_purpose::STANDARD.encode(
        serde_json::to_vec(&payload).unwrap_or_else(|error| panic!("test input: {error:?}")),
    );

    let mut gateway1 = registered_gateway();
    let mut gateway2 = registered_gateway();
    let principal =
        PrincipalId::new("test-merchant").unwrap_or_else(|error| panic!("test input: {error:?}"));
    let mut plane = TestPaymentPlane {
        outcome: PlanePaymentOutcome::Pending,
    };
    let trace = TraceId::mint([0xab; 16]);

    let _outcome1 = seller
        .settle(&mut gateway1, &principal, &encoded, &mut plane, &trace, 0)
        .unwrap_or_else(|error| panic!("first settlement: {error:?}"));

    let _outcome2 = seller
        .settle(&mut gateway2, &principal, &encoded, &mut plane, &trace, 100)
        .unwrap_or_else(|error| panic!("second settlement: {error:?}"));
}

#[test]
fn seller_preserves_extensions_from_payment_required() {
    let mut required = test_payment_required();
    required.extensions.insert(
        "custom".to_owned(),
        layerx_x402::model::Extension {
            info: json!({"key": "value"}),
            schema: json!({"type": "object"}),
        },
    );

    let seller =
        Seller::new(required).unwrap_or_else(|error| panic!("valid with extensions: {error:?}"));
    let signal = seller
        .payment_required()
        .unwrap_or_else(|error| panic!("encoding succeeds: {error:?}"));

    assert!(signal.body.extensions.contains_key("custom"));
}

#[test]
fn seller_validates_payment_payload_before_settlement() {
    let required = test_payment_required();
    let seller = Seller::new(required).unwrap_or_else(|error| panic!("valid required: {error:?}"));

    let mut invalid_payload = test_payment_payload();
    invalid_payload.x402_version = 1;

    let encoded = base64::engine::general_purpose::STANDARD.encode(
        serde_json::to_vec(&invalid_payload)
            .unwrap_or_else(|error| panic!("test input: {error:?}")),
    );

    let mut gateway = registered_gateway();
    let principal =
        PrincipalId::new("test-merchant").unwrap_or_else(|error| panic!("test input: {error:?}"));
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
    let seller =
        Seller::new(required.clone()).unwrap_or_else(|error| panic!("valid required: {error:?}"));
    let payload = test_payment_payload();

    let encoded = base64::engine::general_purpose::STANDARD.encode(
        serde_json::to_vec(&payload).unwrap_or_else(|error| panic!("test input: {error:?}")),
    );

    let mut gateway = registered_gateway();
    let principal =
        PrincipalId::new("test-merchant").unwrap_or_else(|error| panic!("test input: {error:?}"));
    let mut plane = CapturePaymentPlane { captured: None };
    let trace = TraceId::mint([0xab; 16]);

    let _outcome = seller
        .settle(&mut gateway, &principal, &encoded, &mut plane, &trace, 0)
        .unwrap_or_else(|error| panic!("settlement accepted: {error:?}"));

    let captured = plane.captured.unwrap_or_else(|| panic!("plane was called"));
    assert_eq!(captured.scheme, "exact");
    assert_eq!(captured.network, "layerx:beta");
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
            network: "layerx:beta".to_owned(),
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

    let seller = Seller::new(required)
        .unwrap_or_else(|error| panic!("multiple requirements accepted: {error:?}"));
    let signal = seller
        .payment_required()
        .unwrap_or_else(|error| panic!("encoding succeeds: {error:?}"));

    assert_eq!(signal.body.accepts.len(), 2);
}

#[test]
fn seller_resource_info_is_preserved_in_signal() {
    let required = test_payment_required();
    let seller =
        Seller::new(required.clone()).unwrap_or_else(|error| panic!("valid required: {error:?}"));

    let signal = seller
        .payment_required()
        .unwrap_or_else(|error| panic!("encoding succeeds: {error:?}"));

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

const ASSET_HEX: &str = "abababababababababababababababababababababababababababababababab";
const OTHER_ASSET_HEX: &str = "cdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcd";

fn hex_id(id: [u8; 32]) -> String {
    const DIGITS: &[u8; 16] = b"0123456789abcdef";
    let mut text = String::with_capacity(64);
    for byte in id {
        text.push(char::from(DIGITS[usize::from(byte >> 4)]));
        text.push(char::from(DIGITS[usize::from(byte & 0x0f)]));
    }
    text
}

fn asset_account(did: &str, asset_hex: &str) -> String {
    format!("agent:{did}:asset:{asset_hex}")
}

fn asset_offer(account: &str, pay_to: String) -> PaymentRequirements {
    PaymentRequirements {
        pay_to,
        extra: Some(json!({
            "layerx": {"commitment": "executed", "account": account, "currency": "SID"}
        })),
        ..test_requirements()
    }
}

fn required_with(offer: PaymentRequirements) -> PaymentRequired {
    PaymentRequired {
        accepts: vec![offer],
        ..test_payment_required()
    }
}

#[test]
fn seller_accepts_a_per_asset_payee_account_for_the_offered_asset() {
    let account = asset_account("did:layerx:test-resource", ASSET_HEX);
    let identifiers = account_identifiers(&account)
        .unwrap_or_else(|error| panic!("asset account identifiers: {error}"));
    for (index, identifier) in identifiers.into_iter().enumerate() {
        let offer = asset_offer(&account, format!("0x{}", hex_id(identifier)));
        assert_eq!(offer.validate(), Ok(()), "derivation {index}");
        assert_eq!(
            offer
                .layerx_terms()
                .map(|terms| (terms.account, terms.currency)),
            Ok((account.clone(), "SID".to_owned())),
            "derivation {index}"
        );
        assert!(Seller::new(required_with(offer.clone())).is_ok());

        let bare = PaymentRequirements {
            asset: ASSET_HEX.to_owned(),
            pay_to: hex_id(identifier),
            ..offer
        };
        assert_eq!(
            bare.validate(),
            Ok(()),
            "unprefixed asset, derivation {index}"
        );
    }

    let main_account = "agent:did:layerx:test-resource:main";
    let main_offer = asset_offer(main_account, pay_to(main_account));
    assert_eq!(main_offer.validate(), Ok(()));
}

#[test]
fn seller_refuses_every_other_per_asset_payee_form() {
    let did = "did:layerx:test-resource";
    let accepted = asset_account(did, ASSET_HEX);

    let other_asset = asset_account(did, OTHER_ASSET_HEX);
    let refused_accounts = [
        other_asset.clone(),
        asset_account(did, &ASSET_HEX.to_uppercase()),
        asset_account(did, &format!("0x{ASSET_HEX}")),
        asset_account(did, &ASSET_HEX[..62]),
        asset_account(did, &format!("{ASSET_HEX}ab")),
        asset_account(did, ""),
        asset_account("", ASSET_HEX),
        asset_account(&format!("{did}:asset:{ASSET_HEX}"), ASSET_HEX),
        asset_account(&format!("{did}:"), ASSET_HEX),
        asset_account("did:layerx:Test-Resource", ASSET_HEX),
        format!("agent:{did}:asset:{ASSET_HEX}:main"),
        format!("{did}:asset:{ASSET_HEX}"),
        format!("agent:{did}:assets:{ASSET_HEX}"),
        "agent:did:layerx:api-seller:asset:lxp:main".to_owned(),
    ];
    for account in &refused_accounts {
        let offer = asset_offer(account, pay_to(&accepted));
        assert_eq!(
            offer.validate(),
            Err(layerx_x402::model::X402Error::ProfileMismatch),
            "{account}"
        );
        assert!(Seller::new(required_with(offer)).is_err(), "{account}");
    }

    let bound_to_other = asset_offer(&other_asset, pay_to(&other_asset));
    assert_eq!(
        bound_to_other.validate(),
        Err(layerx_x402::model::X402Error::ProfileMismatch)
    );

    let redirected = asset_offer(&accepted, pay_to(PAYEE_ACCOUNT));
    assert_eq!(
        redirected.validate(),
        Err(layerx_x402::model::X402Error::ProfileMismatch)
    );
    let main_payee = asset_offer(PAYEE_ACCOUNT, pay_to(&accepted));
    assert_eq!(
        main_payee.validate(),
        Err(layerx_x402::model::X402Error::ProfileMismatch)
    );

    let unparsed_asset = PaymentRequirements {
        asset: "lxp".to_owned(),
        ..asset_offer(&accepted, pay_to(&accepted))
    };
    assert!(unparsed_asset.validate().is_err());
    assert!(Seller::new(required_with(unparsed_asset)).is_err());

    let external = PaymentRequirements {
        network: "eip155:8453".to_owned(),
        ..asset_offer(&accepted, pay_to(&accepted))
    };
    assert_eq!(
        external.validate(),
        Err(layerx_x402::model::X402Error::ProfileMismatch)
    );
    assert_eq!(
        external.layerx_facts(),
        Err(layerx_x402::model::X402Error::UnsupportedOffer)
    );
}
