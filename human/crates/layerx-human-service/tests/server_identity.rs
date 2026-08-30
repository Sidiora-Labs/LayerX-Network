use layerx_human_service::auth::OperationDigest;
use layerx_human_service::server::schema::ApiSchema;
use layerx_human_service::server::IdentityProjector;
use serde_json::json;

#[test]
fn session_open_requires_real_device_metadata_before_minting_identifier() {
    let device = IdentityProjector::mint_session_device(&json!({
        "assertion_id": "asr_01j2h5p0yb2c4d6e8f0g2h4j6k",
        "device": { "label": "LayerX web app", "platform": "web" }
    }))
    .unwrap_or_else(|error| panic!("device: {error:?}"));
    assert!(device.device_id().starts_with("dev_"));
    assert_eq!(device.label(), "LayerX web app");
    assert_eq!(device.platform(), "web");

    let missing = IdentityProjector::mint_session_device(&json!({
        "assertion_id": "asr_01j2h5p0yb2c4d6e8f0g2h4j6k"
    }))
    .expect_err("missing metadata must fail closed");
    assert_eq!(missing.status, 400);
    assert_eq!(missing.field.as_deref(), Some("device"));
}

#[test]
fn embedded_schema_admits_additive_device_metadata() {
    let schema = ApiSchema::v1().unwrap_or_else(|error| panic!("schema: {error}"));
    let operation = schema
        .operation("session.open")
        .unwrap_or_else(|| panic!("session.open operation"));
    let decoded = schema
        .decode_request(
            operation,
            Some(json!({
                "assertion_id": "asr_01j2h5p0yb2c4d6e8f0g2h4j6k",
                "device": { "label": "LayerX web app", "platform": "web" }
            })),
        )
        .unwrap_or_else(|error| panic!("request: {error}"));
    assert_eq!(decoded["device"]["platform"], "web");
}

#[test]
fn operation_digest_schema_codec_is_strict_and_round_trips() {
    let digest = OperationDigest::new([0xab; 32]);
    let encoded = digest.to_schema();
    assert_eq!(encoded, format!("opd_{}", "ab".repeat(32)));
    let parsed = OperationDigest::parse_schema(&encoded)
        .unwrap_or_else(|error| panic!("operation digest: {error}"));
    assert_eq!(parsed, digest);
    assert!(OperationDigest::parse_schema(&encoded.to_uppercase()).is_err());
    assert!(OperationDigest::parse_schema("opd_ab").is_err());
}
