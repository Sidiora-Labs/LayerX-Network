//! x402 v2 conformance harness. The vectors are the first-party suite under
//! `interop/specs/conformance/x402`, which the gateway deployment pins by
//! vector count and digest, and they verify wire format compatibility,
//! canonical encoding, bounds enforcement, and interoperability with
//! independent x402 implementations.

use std::collections::BTreeMap;

use base64::engine::general_purpose::STANDARD;
use base64::Engine as _;
use layerx_x402::model::{
    account_identifiers, AtomicAmount, LayerXTerms, PaymentPayload, PaymentRequired,
    PaymentRequirements, ResourceInfo, SettlementResponse, X402Error, X402_VERSION,
};
use layerx_x402::transport::{
    decode_payment_required, encode_payment_required, TransportKind, TransportValue,
};
use serde_json::{json, Value};

const PAYMENT_REQUIRED_SUITE: &str =
    include_str!("../../../specs/conformance/x402/payment-required.json");
const PAYMENT_PAYLOAD_SUITE: &str =
    include_str!("../../../specs/conformance/x402/payment-payload.json");
const SETTLEMENT_RESPONSE_SUITE: &str =
    include_str!("../../../specs/conformance/x402/settlement-response.json");

const PAYEE_ACCOUNT: &str = "agent:did:layerx:api-seller:main";
const OTHER_ACCOUNT: &str = "agent:did:layerx:data-seller:main";
const CURRENCY: &str = "LXP";

fn account_id(account: &str, derivation: usize) -> [u8; 32] {
    account_identifiers(account)
        .unwrap_or_else(|error| panic!("{account} has account identifiers: {error}"))[derivation]
}

fn hex32(bytes: &[u8; 32]) -> String {
    const DIGITS: &[u8; 16] = b"0123456789abcdef";
    let mut text = String::with_capacity(66);
    text.push_str("0x");
    for byte in bytes {
        text.push(char::from(DIGITS[usize::from(byte >> 4)]));
        text.push(char::from(DIGITS[usize::from(byte & 0x0f)]));
    }
    text
}

fn pay_to(account: &str) -> String {
    hex32(&account_id(account, 0))
}

fn layerx_terms(account: &str) -> Value {
    json!({"layerx": {"commitment": "executed", "account": account, "currency": CURRENCY}})
}

struct Vector {
    name: String,
    valid: bool,
    json: serde_json::Value,
}

fn suite_vectors(suite: &str, source: &str) -> Vec<Vector> {
    let records: Vec<serde_json::Value> =
        serde_json::from_str(suite).unwrap_or_else(|error| panic!("{source}: {error}"));
    assert!(
        !records.is_empty(),
        "{source}: a suite that carries no vector is not a conformance suite"
    );
    records
        .into_iter()
        .map(|record| {
            let name = record["name"]
                .as_str()
                .unwrap_or_else(|| panic!("{source}: every vector is named"))
                .to_owned();
            let valid = record["valid"].as_bool().unwrap_or_else(|| {
                panic!("{name}: every vector declares whether it must validate")
            });
            let json = record
                .get("document")
                .cloned()
                .unwrap_or_else(|| panic!("{name}: every vector carries its wire document"));
            Vector { name, valid, json }
        })
        .collect()
}

fn payment_required_vectors() -> Vec<Vector> {
    suite_vectors(
        PAYMENT_REQUIRED_SUITE,
        "interop/specs/conformance/x402/payment-required.json",
    )
}

fn payment_payload_vectors() -> Vec<Vector> {
    suite_vectors(
        PAYMENT_PAYLOAD_SUITE,
        "interop/specs/conformance/x402/payment-payload.json",
    )
}

fn settlement_response_vectors() -> Vec<Vector> {
    suite_vectors(
        SETTLEMENT_RESPONSE_SUITE,
        "interop/specs/conformance/x402/settlement-response.json",
    )
}

#[test]
fn all_payment_required_vectors_validate_correctly() {
    for vector in payment_required_vectors() {
        let parsed: Result<PaymentRequired, _> = serde_json::from_value(vector.json.clone());
        match (parsed, vector.valid) {
            (Ok(required), true) => {
                let validation = required.validate();
                assert!(
                    validation.is_ok(),
                    "{}: expected valid, got {:?}",
                    vector.name,
                    validation.err()
                );
            }
            (Ok(required), false) => {
                let validation = required.validate();
                assert!(
                    validation.is_err(),
                    "{}: expected invalid but validation passed",
                    vector.name
                );
            }
            (Err(_), false) => {}
            (Err(error), true) => {
                panic!("{}: parsing failed: {}", vector.name, error);
            }
        }
    }
}

#[test]
fn all_payment_payload_vectors_validate_correctly() {
    for vector in payment_payload_vectors() {
        let parsed: Result<PaymentPayload, _> = serde_json::from_value(vector.json.clone());
        match (parsed, vector.valid) {
            (Ok(payload), true) => {
                let validation = payload.validate();
                assert!(
                    validation.is_ok(),
                    "{}: expected valid, got {:?}",
                    vector.name,
                    validation.err()
                );
            }
            (Ok(payload), false) => {
                let validation = payload.validate();
                assert!(
                    validation.is_err(),
                    "{}: expected invalid but validation passed",
                    vector.name
                );
            }
            (Err(_), false) => {}
            (Err(error), true) => {
                panic!("{}: parsing failed: {}", vector.name, error);
            }
        }
    }
}

#[test]
fn all_settlement_response_vectors_validate_correctly() {
    for vector in settlement_response_vectors() {
        let parsed: Result<SettlementResponse, _> = serde_json::from_value(vector.json.clone());
        match (parsed, vector.valid) {
            (Ok(response), true) => {
                let validation = response.validate_wire();
                assert!(
                    validation.is_ok(),
                    "{}: expected valid, got {:?}",
                    vector.name,
                    validation.err()
                );
            }
            (Ok(response), false) => {
                let validation = response.validate_wire();
                assert!(
                    validation.is_err(),
                    "{}: expected invalid but validation passed",
                    vector.name
                );
            }
            (Err(_), false) => {}
            (Err(error), true) => {
                panic!("{}: parsing failed: {}", vector.name, error);
            }
        }
    }
}

#[test]
fn atomic_amount_canonical_encoding_round_trips() {
    let amounts = vec![
        0u128,
        1,
        100,
        1_000,
        1_000_000,
        1_000_000_000_000_000_000,
        u128::MAX,
    ];

    for amount in amounts {
        let atomic = AtomicAmount::from_u128(amount);
        let serialized = serde_json::to_string(&atomic)
            .unwrap_or_else(|error| panic!("serialization: {error:?}"));
        let deserialized: AtomicAmount = serde_json::from_str(&serialized)
            .unwrap_or_else(|error| panic!("deserialization: {error:?}"));
        assert_eq!(deserialized.value(), amount);
    }
}

#[test]
fn atomic_amount_refuses_non_canonical_strings() {
    let overflow = "9".repeat(40);
    let invalid = vec![
        "",
        "-100",
        "1.5",
        "1e10",
        "01",
        "00",
        " 100",
        "100 ",
        "abc",
        overflow.as_str(),
    ];

    for value in invalid {
        let result = AtomicAmount::parse(value);
        assert!(
            result.is_err(),
            "expected {value} to be invalid but parsed successfully"
        );
    }
}

#[test]
fn atomic_amount_accepts_canonical_strings() {
    let valid = vec![
        ("0", 0u128),
        ("1", 1),
        ("100", 100),
        ("1000000", 1_000_000),
        ("340282366920938463463374607431768211455", u128::MAX),
    ];

    for (string, expected) in valid {
        let parsed =
            AtomicAmount::parse(string).unwrap_or_else(|error| panic!("parsing: {error:?}"));
        assert_eq!(parsed.value(), expected);
    }
}

#[test]
fn payment_required_http_transport_encoding_is_base64_json() {
    let required = PaymentRequired {
        x402_version: X402_VERSION,
        error: None,
        resource: ResourceInfo {
            url: "https://test.example/resource".to_owned(),
            description: None,
            mime_type: None,
            service_name: None,
            tags: vec![],
            icon_url: None,
        },
        accepts: vec![PaymentRequirements {
            scheme: "exact".to_owned(),
            network: "layerx:testnet".to_owned(),
            amount: AtomicAmount::from_u128(750),
            asset: "0x".to_owned() + &"aa".repeat(32),
            pay_to: pay_to(PAYEE_ACCOUNT),
            max_timeout_seconds: 100,
            extra: Some(layerx_terms(PAYEE_ACCOUNT)),
        }],
        extensions: BTreeMap::new(),
    };

    let encoded = encode_payment_required(TransportKind::Http, &required)
        .unwrap_or_else(|error| panic!("encoding: {error:?}"));

    let TransportValue::HttpHeader { name, value } = encoded else {
        panic!("expected HTTP header");
    };

    assert_eq!(name, "PAYMENT-REQUIRED");
    let decoded = STANDARD
        .decode(value.as_bytes())
        .unwrap_or_else(|error| panic!("base64: {error:?}"));
    let parsed: PaymentRequired =
        serde_json::from_slice(&decoded).unwrap_or_else(|error| panic!("json: {error:?}"));
    assert_eq!(parsed.x402_version, X402_VERSION);
}

#[test]
fn payment_required_mcp_transport_encoding_is_json() {
    let required = PaymentRequired {
        x402_version: X402_VERSION,
        error: None,
        resource: ResourceInfo {
            url: "https://test.example/resource".to_owned(),
            description: None,
            mime_type: None,
            service_name: None,
            tags: vec![],
            icon_url: None,
        },
        accepts: vec![PaymentRequirements {
            scheme: "exact".to_owned(),
            network: "layerx:testnet".to_owned(),
            amount: AtomicAmount::from_u128(750),
            asset: "0x".to_owned() + &"aa".repeat(32),
            pay_to: pay_to(PAYEE_ACCOUNT),
            max_timeout_seconds: 100,
            extra: Some(layerx_terms(PAYEE_ACCOUNT)),
        }],
        extensions: BTreeMap::new(),
    };

    let encoded = encode_payment_required(TransportKind::Mcp, &required)
        .unwrap_or_else(|error| panic!("encoding: {error:?}"));

    let TransportValue::Json(value) = encoded else {
        panic!("expected JSON");
    };

    let parsed: PaymentRequired =
        serde_json::from_value(value).unwrap_or_else(|error| panic!("parsing: {error:?}"));
    assert_eq!(parsed, required);
}

#[test]
fn resource_info_validates_url_format() {
    let valid = ResourceInfo {
        url: "https://api.example.com/resource".to_owned(),
        description: None,
        mime_type: None,
        service_name: None,
        tags: vec![],
        icon_url: None,
    };
    assert!(valid.validate().is_ok());

    let no_protocol = ResourceInfo {
        url: "api.example.com/resource".to_owned(),
        description: None,
        mime_type: None,
        service_name: None,
        tags: vec![],
        icon_url: None,
    };
    assert!(no_protocol.validate().is_err());

    let with_newline = ResourceInfo {
        url: "https://api.example.com/resource\nmalicious".to_owned(),
        description: None,
        mime_type: None,
        service_name: None,
        tags: vec![],
        icon_url: None,
    };
    assert!(with_newline.validate().is_err());
}

#[test]
fn payment_requirements_validates_layerx_network_format() {
    let valid = PaymentRequirements {
        scheme: "exact".to_owned(),
        network: "layerx:testnet".to_owned(),
        amount: AtomicAmount::from_u128(100),
        asset: "0x".to_owned() + &"ab".repeat(32),
        pay_to: pay_to(PAYEE_ACCOUNT),
        max_timeout_seconds: 60,
        extra: Some(layerx_terms(PAYEE_ACCOUNT)),
    };
    assert!(valid.validate().is_ok());
    assert!(valid.layerx_facts().is_ok());
    assert_eq!(
        valid.layerx_terms(),
        Ok(LayerXTerms {
            account: PAYEE_ACCOUNT.to_owned(),
            currency: CURRENCY.to_owned(),
        })
    );

    let wrong_namespace = PaymentRequirements {
        scheme: "exact".to_owned(),
        network: "ethereum:mainnet".to_owned(),
        amount: AtomicAmount::from_u128(100),
        asset: "0x".to_owned() + &"ab".repeat(32),
        pay_to: "0x".to_owned() + &"cd".repeat(32),
        max_timeout_seconds: 60,
        extra: None,
    };
    assert!(wrong_namespace.validate().is_ok());
    assert!(wrong_namespace.layerx_facts().is_err());

    let no_separator = PaymentRequirements {
        scheme: "exact".to_owned(),
        network: "layerxtestnet".to_owned(),
        amount: AtomicAmount::from_u128(100),
        asset: "0x".to_owned() + &"ab".repeat(32),
        pay_to: "0x".to_owned() + &"cd".repeat(32),
        max_timeout_seconds: 60,
        extra: None,
    };
    assert!(no_separator.validate().is_err());
}

#[test]
fn wire_encoding_round_trip_preserves_all_fields() {
    let original = PaymentRequired {
        x402_version: X402_VERSION,
        error: Some("Custom error message".to_owned()),
        resource: ResourceInfo {
            url: "https://api.example.com/protected".to_owned(),
            description: Some("Protected resource".to_owned()),
            mime_type: Some("application/json".to_owned()),
            service_name: Some("API Service".to_owned()),
            tags: vec!["api".to_owned(), "protected".to_owned()],
            icon_url: Some("https://api.example.com/icon.png".to_owned()),
        },
        accepts: vec![PaymentRequirements {
            scheme: "exact".to_owned(),
            network: "layerx:mainnet".to_owned(),
            amount: AtomicAmount::from_u128(12345),
            asset: "0x".to_owned() + &"ab".repeat(32),
            pay_to: pay_to(PAYEE_ACCOUNT),
            max_timeout_seconds: 300,
            extra: Some(json!({
                "custom": "field",
                "layerx": {
                    "commitment": "executed",
                    "account": PAYEE_ACCOUNT,
                    "currency": CURRENCY
                }
            })),
        }],
        extensions: {
            let mut map = BTreeMap::new();
            map.insert(
                "test".to_owned(),
                layerx_x402::model::Extension {
                    info: json!({"value": 123}),
                    schema: json!({"type": "number"}),
                },
            );
            map
        },
    };

    for transport in [TransportKind::Http, TransportKind::Mcp, TransportKind::A2a] {
        let encoded = encode_payment_required(transport, &original)
            .unwrap_or_else(|error| panic!("encoding: {error:?}"));
        let decoded = decode_payment_required(transport, &encoded)
            .unwrap_or_else(|error| panic!("decoding: {error:?}"));
        assert_eq!(decoded, original);
    }
}

fn layerx_offer(pay_to: String, extra: Option<Value>) -> PaymentRequirements {
    PaymentRequirements {
        scheme: "exact".to_owned(),
        network: "layerx:testnet".to_owned(),
        amount: AtomicAmount::from_u128(100),
        asset: "0x".to_owned() + &"ab".repeat(32),
        pay_to,
        max_timeout_seconds: 60,
        extra,
    }
}

#[test]
fn layerx_offer_without_quote_terms_is_refused() {
    let missing = layerx_offer(pay_to(PAYEE_ACCOUNT), None);
    assert_eq!(missing.validate(), Err(X402Error::ProfileMissing));
    assert_eq!(missing.layerx_terms(), Err(X402Error::ProfileMissing));

    let commitment_only = layerx_offer(
        pay_to(PAYEE_ACCOUNT),
        Some(json!({"layerx": {"commitment": "executed"}})),
    );
    assert_eq!(commitment_only.validate(), Err(X402Error::ProfileMissing));

    let account_only = layerx_offer(
        pay_to(PAYEE_ACCOUNT),
        Some(json!({"layerx": {"account": PAYEE_ACCOUNT}})),
    );
    assert_eq!(account_only.validate(), Err(X402Error::ProfileMissing));

    let currency_only = layerx_offer(
        pay_to(PAYEE_ACCOUNT),
        Some(json!({"layerx": {"currency": CURRENCY}})),
    );
    assert_eq!(currency_only.validate(), Err(X402Error::ProfileMissing));
}

#[test]
fn layerx_pay_to_must_derive_from_the_advertised_account() {
    for derivation in 0..2 {
        let bound = layerx_offer(
            hex32(&account_id(PAYEE_ACCOUNT, derivation)),
            Some(layerx_terms(PAYEE_ACCOUNT)),
        );
        assert_eq!(bound.validate(), Ok(()));
        assert_eq!(
            bound.layerx_terms().map(|terms| terms.account),
            Ok(PAYEE_ACCOUNT.to_owned())
        );
    }

    let redirected = layerx_offer(pay_to(OTHER_ACCOUNT), Some(layerx_terms(PAYEE_ACCOUNT)));
    assert_eq!(redirected.validate(), Err(X402Error::ProfileMismatch));
    assert_eq!(redirected.layerx_terms(), Err(X402Error::ProfileMismatch));

    let unbindable = layerx_offer(
        "0x".to_owned() + &"cd".repeat(32),
        Some(layerx_terms(PAYEE_ACCOUNT)),
    );
    assert_eq!(unbindable.validate(), Err(X402Error::ProfileMismatch));
}

#[test]
fn layerx_quote_terms_follow_the_account_and_currency_grammar() {
    let payee = pay_to(PAYEE_ACCOUNT);
    let refused = [
        json!({"layerx": {"account": "did:layerx:api-seller", "currency": CURRENCY}}),
        json!({"layerx": {"account": "agent::main", "currency": CURRENCY}}),
        json!({"layerx": {"account": "agent:did:layerx:api-seller:asset:lxp:main", "currency": CURRENCY}}),
        json!({"layerx": {"account": "agent:DID:layerx:api-seller:main", "currency": CURRENCY}}),
        json!({"layerx": {"account": PAYEE_ACCOUNT, "currency": "lxp"}}),
        json!({"layerx": {"account": PAYEE_ACCOUNT, "currency": ""}}),
        json!({"layerx": {"account": PAYEE_ACCOUNT, "currency": "LXP-TESTNET"}}),
        json!({"layerx": {"account": PAYEE_ACCOUNT, "currency": 1}}),
        json!({"layerx": {"account": 1, "currency": CURRENCY}}),
        json!({"layerx": "terms"}),
    ];
    for extra in refused {
        let offer = layerx_offer(payee.clone(), Some(extra.clone()));
        assert_eq!(
            offer.validate(),
            Err(X402Error::ProfileMismatch),
            "{extra} is not a layerx profile"
        );
    }
}

#[test]
fn offers_outside_layerx_carry_no_quote_terms_but_keep_the_grammar() {
    let external = PaymentRequirements {
        scheme: "exact".to_owned(),
        network: "eip155:84532".to_owned(),
        amount: AtomicAmount::from_u128(10_000),
        asset: "0x036CbD53842c5426634e7929541eC2318f3dCF7e".to_owned(),
        pay_to: "0x209693Bc6afc0C5328bA36FaF03C514EF312287C".to_owned(),
        max_timeout_seconds: 60,
        extra: Some(json!({"name": "USDC", "version": "2"})),
    };
    assert_eq!(external.validate(), Ok(()));
    assert_eq!(external.layerx_terms(), Err(X402Error::ProfileMissing));
    assert_eq!(external.layerx_facts(), Err(X402Error::UnsupportedOffer));

    let malformed = PaymentRequirements {
        extra: Some(json!({"layerx": {"account": "agent:main", "currency": CURRENCY}})),
        ..external
    };
    assert_eq!(malformed.validate(), Err(X402Error::ProfileMismatch));
}
