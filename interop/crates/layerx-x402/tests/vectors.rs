//! x402 v2 reference test vectors and conformance harness. These vectors verify
//! wire format compatibility, canonical encoding, bounds enforcement, and
//! interoperability with independent x402 implementations.

use std::collections::BTreeMap;

use base64::engine::general_purpose::STANDARD;
use base64::Engine as _;
use layerx_x402::model::{
    AtomicAmount, PaymentPayload, PaymentRequired, PaymentRequirements, ResourceInfo,
    SettlementResponse, X402Error, X402_VERSION,
};
use layerx_x402::transport::{
    decode_payment_payload, decode_payment_required, decode_settlement, encode_payment_payload,
    encode_payment_required, encode_settlement, TransportKind, TransportValue,
};
use serde_json::json;

struct Vector {
    name: &'static str,
    valid: bool,
    json: serde_json::Value,
}

fn payment_required_vectors() -> Vec<Vector> {
    vec![
        Vector {
            name: "minimal_valid_payment_required",
            valid: true,
            json: json!({
                "x402Version": 2,
                "resource": {
                    "url": "https://api.example.com/resource"
                },
                "accepts": [{
                    "scheme": "exact",
                    "network": "layerx:testnet",
                    "amount": "1000",
                    "asset": "0x1234567890abcdef1234567890abcdef1234567890abcdef1234567890abcdef",
                    "payTo": "0xfedcba0987654321fedcba0987654321fedcba0987654321fedcba0987654321",
                    "maxTimeoutSeconds": 120
                }]
            }),
        },
        Vector {
            name: "payment_required_with_all_optional_fields",
            valid: true,
            json: json!({
                "x402Version": 2,
                "error": "Unauthorized access",
                "resource": {
                    "url": "https://api.example.com/resource",
                    "description": "Premium content",
                    "mimeType": "application/json",
                    "serviceName": "API Service",
                    "tags": ["api", "premium"],
                    "iconUrl": "https://api.example.com/icon.png"
                },
                "accepts": [{
                    "scheme": "exact",
                    "network": "layerx:mainnet",
                    "amount": "5000",
                    "asset": "0xabcdef1234567890abcdef1234567890abcdef1234567890abcdef1234567890",
                    "payTo": "0x1234567890abcdef1234567890abcdef1234567890abcdef1234567890abcdef",
                    "maxTimeoutSeconds": 300,
                    "extra": {"customField": "value"}
                }],
                "extensions": {}
            }),
        },
        Vector {
            name: "payment_required_with_multiple_accepts",
            valid: true,
            json: json!({
                "x402Version": 2,
                "resource": {
                    "url": "https://service.example/data"
                },
                "accepts": [
                    {
                        "scheme": "exact",
                        "network": "layerx:testnet",
                        "amount": "100",
                        "asset": "0x".to_owned() + &"11".repeat(32),
                        "payTo": "0x".to_owned() + &"22".repeat(32),
                        "maxTimeoutSeconds": 60
                    },
                    {
                        "scheme": "402lxp",
                        "network": "layerx:mainnet",
                        "amount": "200",
                        "asset": "0x".to_owned() + &"33".repeat(32),
                        "payTo": "0x".to_owned() + &"44".repeat(32),
                        "maxTimeoutSeconds": 90
                    }
                ]
            }),
        },
        Vector {
            name: "payment_required_wrong_version",
            valid: false,
            json: json!({
                "x402Version": 1,
                "resource": {
                    "url": "https://api.example.com/resource"
                },
                "accepts": [{
                    "scheme": "exact",
                    "network": "layerx:testnet",
                    "amount": "1000",
                    "asset": "0x".to_owned() + &"ab".repeat(32),
                    "payTo": "0x".to_owned() + &"cd".repeat(32),
                    "maxTimeoutSeconds": 120
                }]
            }),
        },
        Vector {
            name: "payment_required_empty_accepts",
            valid: false,
            json: json!({
                "x402Version": 2,
                "resource": {
                    "url": "https://api.example.com/resource"
                },
                "accepts": []
            }),
        },
        Vector {
            name: "payment_required_zero_amount",
            valid: false,
            json: json!({
                "x402Version": 2,
                "resource": {
                    "url": "https://api.example.com/resource"
                },
                "accepts": [{
                    "scheme": "exact",
                    "network": "layerx:testnet",
                    "amount": "0",
                    "asset": "0x".to_owned() + &"ab".repeat(32),
                    "payTo": "0x".to_owned() + &"cd".repeat(32),
                    "maxTimeoutSeconds": 120
                }]
            }),
        },
        Vector {
            name: "payment_required_negative_amount",
            valid: false,
            json: json!({
                "x402Version": 2,
                "resource": {
                    "url": "https://api.example.com/resource"
                },
                "accepts": [{
                    "scheme": "exact",
                    "network": "layerx:testnet",
                    "amount": "-100",
                    "asset": "0x".to_owned() + &"ab".repeat(32),
                    "payTo": "0x".to_owned() + &"cd".repeat(32),
                    "maxTimeoutSeconds": 120
                }]
            }),
        },
        Vector {
            name: "payment_required_empty_url",
            valid: false,
            json: json!({
                "x402Version": 2,
                "resource": {
                    "url": ""
                },
                "accepts": [{
                    "scheme": "exact",
                    "network": "layerx:testnet",
                    "amount": "1000",
                    "asset": "0x".to_owned() + &"ab".repeat(32),
                    "payTo": "0x".to_owned() + &"cd".repeat(32),
                    "maxTimeoutSeconds": 120
                }]
            }),
        },
    ]
}

fn payment_payload_vectors() -> Vec<Vector> {
    vec![
        Vector {
            name: "minimal_valid_payment_payload",
            valid: true,
            json: json!({
                "x402Version": 2,
                "payload": {"authorization": "signed-payment"},
                "accepted": {
                    "scheme": "exact",
                    "network": "layerx:testnet",
                    "amount": "1500",
                    "asset": "0x".to_owned() + &"aa".repeat(32),
                    "payTo": "0x".to_owned() + &"bb".repeat(32),
                    "maxTimeoutSeconds": 180
                }
            }),
        },
        Vector {
            name: "payment_payload_with_resource",
            valid: true,
            json: json!({
                "x402Version": 2,
                "resource": {
                    "url": "https://service.example/content",
                    "description": "Paid content"
                },
                "payload": {"scheme": "exact", "data": "payment-data"},
                "accepted": {
                    "scheme": "exact",
                    "network": "layerx:mainnet",
                    "amount": "2500",
                    "asset": "0x".to_owned() + &"cc".repeat(32),
                    "payTo": "0x".to_owned() + &"dd".repeat(32),
                    "maxTimeoutSeconds": 240
                }
            }),
        },
        Vector {
            name: "payment_payload_with_extensions",
            valid: true,
            json: json!({
                "x402Version": 2,
                "payload": {"authorization": "signed"},
                "accepted": {
                    "scheme": "402lxp",
                    "network": "layerx:testnet",
                    "amount": "500",
                    "asset": "0x".to_owned() + &"ee".repeat(32),
                    "payTo": "0x".to_owned() + &"ff".repeat(32),
                    "maxTimeoutSeconds": 90
                },
                "extensions": {
                    "custom": {
                        "info": {"key": "value"},
                        "schema": {"type": "object"}
                    }
                }
            }),
        },
        Vector {
            name: "payment_payload_wrong_version",
            valid: false,
            json: json!({
                "x402Version": 3,
                "payload": {"authorization": "signed"},
                "accepted": {
                    "scheme": "exact",
                    "network": "layerx:testnet",
                    "amount": "1000",
                    "asset": "0x".to_owned() + &"ab".repeat(32),
                    "payTo": "0x".to_owned() + &"cd".repeat(32),
                    "maxTimeoutSeconds": 120
                }
            }),
        },
        Vector {
            name: "payment_payload_non_object_payload",
            valid: false,
            json: json!({
                "x402Version": 2,
                "payload": "not-an-object",
                "accepted": {
                    "scheme": "exact",
                    "network": "layerx:testnet",
                    "amount": "1000",
                    "asset": "0x".to_owned() + &"ab".repeat(32),
                    "payTo": "0x".to_owned() + &"cd".repeat(32),
                    "maxTimeoutSeconds": 120
                }
            }),
        },
    ]
}

fn settlement_response_vectors() -> Vec<Vector> {
    vec![
        Vector {
            name: "successful_settlement",
            valid: true,
            json: json!({
                "success": true,
                "payer": "0x".to_owned() + &"12".repeat(32),
                "transaction": "lxp:".to_owned() + &"ab".repeat(32),
                "network": "layerx:mainnet",
                "amount": "1000",
                "extensions": {
                    "layerx": {
                        "receipt": STANDARD.encode(vec![0u8; 64]),
                        "receiptDigest": "ab".repeat(32),
                        "verificationLevel": "sequencer-signed"
                    }
                }
            }),
        },
        Vector {
            name: "pending_settlement",
            valid: true,
            json: json!({
                "success": false,
                "errorReason": "settlement_pending",
                "transaction": "pending:ref-123",
                "network": "layerx:testnet"
            }),
        },
        Vector {
            name: "refused_settlement",
            valid: true,
            json: json!({
                "success": false,
                "errorReason": "insufficient_balance",
                "transaction": "",
                "network": "layerx:testnet"
            }),
        },
        Vector {
            name: "settlement_success_with_error_reason",
            valid: false,
            json: json!({
                "success": true,
                "errorReason": "should-not-be-here",
                "transaction": "lxp:".to_owned() + &"ab".repeat(32),
                "network": "layerx:testnet",
                "amount": "500"
            }),
        },
        Vector {
            name: "settlement_failed_without_error_reason",
            valid: false,
            json: json!({
                "success": false,
                "transaction": "",
                "network": "layerx:testnet"
            }),
        },
    ]
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
        let serialized = serde_json::to_string(&atomic).expect("serialization");
        let deserialized: AtomicAmount =
            serde_json::from_str(&serialized).expect("deserialization");
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
            "expected {} to be invalid but parsed successfully",
            value
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
        let parsed = AtomicAmount::parse(string).expect("parsing");
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
            pay_to: "0x".to_owned() + &"bb".repeat(32),
            max_timeout_seconds: 100,
            extra: None,
        }],
        extensions: BTreeMap::new(),
    };

    let encoded = encode_payment_required(TransportKind::Http, &required).expect("encoding");

    let TransportValue::HttpHeader { name, value } = encoded else {
        panic!("expected HTTP header");
    };

    assert_eq!(name, "PAYMENT-REQUIRED");
    let decoded = STANDARD.decode(value.as_bytes()).expect("base64");
    let parsed: PaymentRequired = serde_json::from_slice(&decoded).expect("json");
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
            pay_to: "0x".to_owned() + &"bb".repeat(32),
            max_timeout_seconds: 100,
            extra: None,
        }],
        extensions: BTreeMap::new(),
    };

    let encoded = encode_payment_required(TransportKind::Mcp, &required).expect("encoding");

    let TransportValue::Json(value) = encoded else {
        panic!("expected JSON");
    };

    let parsed: PaymentRequired = serde_json::from_value(value).expect("parsing");
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
        pay_to: "0x".to_owned() + &"cd".repeat(32),
        max_timeout_seconds: 60,
        extra: None,
    };
    assert!(valid.validate().is_ok());
    assert!(valid.layerx_facts().is_ok());

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
            pay_to: "0x".to_owned() + &"cd".repeat(32),
            max_timeout_seconds: 300,
            extra: Some(json!({"custom": "field"})),
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
        let encoded = encode_payment_required(transport, &original).expect("encoding");
        let decoded = decode_payment_required(transport, &encoded).expect("decoding");
        assert_eq!(decoded, original);
    }
}
