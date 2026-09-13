mod required;
use required::Required;

use layerx_sdk::production::SecretBytes;
use layerx_sdk::program_lifecycle::{programs_module_registry, NativeProgramLifecycleRequest};
use layerx_sdk::programs::{
    AgentErrorClass, HttpProgramTransport, LayerXKeyCredential, ProgramOperationError,
    ProgramTransport, Retriability,
};
use layerx_types::program_lifecycle::NativeProgramDeploy;
use std::fs;

fn main() {
    let path = std::env::args()
        .nth(1)
        .required("private lifecycle request path");
    let value: serde_json::Value =
        serde_json::from_slice(&fs::read(path).required("private request"))
            .required("private request JSON");
    let bytes = |field: &str| {
        layerx_platform_core::hex_decode(value[field].as_str().required("canonical field"))
            .required("canonical bytes")
    };
    let signed = bytes("signed_activity");
    let payload = bytes("payload");
    let expected = bytes("receipt");
    let sequencer: [u8; 32] = bytes("sequencer").try_into().required("sequencer key");
    let request = NativeProgramLifecycleRequest::deploy(
        &programs_module_registry().required("production Programs registry"),
        NativeProgramDeploy::decode(&payload).required("real deployment"),
        &signed,
    )
    .required("canonical deployment binding");
    let credential = LayerXKeyCredential::new(
        value["key_id"].as_str().required("key identifier"),
        SecretBytes::new(
            value["key_secret"]
                .as_str()
                .required("key secret")
                .as_bytes(),
        )
        .required("protected credential"),
    )
    .required("gateway credential");
    let transport = HttpProgramTransport::connect(
        value["endpoint"].as_str().required("gateway endpoint"),
        Some(credential),
        sequencer,
        || {
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .ok()
                .and_then(|duration| u64::try_from(duration.as_millis()).ok())
                .ok_or(ProgramOperationError::Verification)
        },
    )
    .required("real TLS transport");
    let expected = layerx_proof::receipt::verify_sequencer_signature(&expected, sequencer)
        .required("expected signed refusal");
    let code = expected
        .protocol()
        .required("protocol receipt")
        .result_code();
    assert_ne!(code, 0);
    match transport.deploy(&request, request.bound_idempotency_key()) {
        Err(ProgramOperationError::Service(error)) => {
            assert_eq!(error.class, AgentErrorClass::PolicyRefusal);
            assert_eq!(error.retriability, Retriability::Terminal);
            assert_eq!(
                error.protocol_result_code.map(|result| result.raw()),
                Some(code)
            );
        }
        _ => panic!("signed refusal replay must remain a terminal policy refusal"),
    }
    let recovered = transport
        .lifecycle_receipt(&request)
        .required("authenticated refusal recovery");
    assert_eq!(recovered, expected);
    println!("signed lifecycle refusal and exact receipt recovery verified");
}
