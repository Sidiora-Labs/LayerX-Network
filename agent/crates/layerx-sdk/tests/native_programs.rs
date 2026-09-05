use layerx_sdk::programs::NativeProgramCallRequest;
use layerx_types::payload::{ActivityType, ModuleId, ModuleRegistration, ModuleRegistry};
use layerx_types::program_call::NativeProgramCall;

fn bytes(value: &str) -> Result<Vec<u8>, std::num::ParseIntError> {
    value
        .as_bytes()
        .chunks_exact(2)
        .map(|pair| {
            let text = String::from_utf8_lossy(pair);
            u8::from_str_radix(&text, 16)
        })
        .collect()
}

#[test]
fn native_request_binds_real_signed_fixture() -> Result<(), Box<dyn std::error::Error>> {
    let fixture: serde_json::Value = serde_json::from_str(include_str!(
        "../../../../platform/sdk/conformance/fixtures/native-program-call-v3.json"
    ))?;
    let signed = bytes(
        fixture["signed_activity_hex"]
            .as_str()
            .ok_or("signed fixture missing")?,
    )?;
    let payload = bytes(
        fixture["payload_hex"]
            .as_str()
            .ok_or("payload fixture missing")?,
    )?;
    let native = NativeProgramCall::decode(&payload).map_err(|_| "invalid fixture")?;
    let registry = ModuleRegistry::new(&[ModuleRegistration::new(
        ModuleId::Programs,
        &[ActivityType::new(ModuleId::Programs, 3).map_err(|_| "activity type invalid")?],
    )
    .map_err(|_| "registration invalid")?])
    .map_err(|_| "registry invalid")?;
    let bound = NativeProgramCallRequest::new(&registry, native, 1000, &signed)
        .map_err(|_| "binding failed")?;
    assert_eq!(bound.signed_activity(), signed);
    assert_eq!(
        bound.bound_activity_id().to_vec(),
        bytes(fixture["activity_id_hex"].as_str().ok_or("id missing")?)?
    );
    assert!(NativeProgramCallRequest::new(&registry, native, 999, &signed).is_err());
    assert!(NativeProgramCallRequest::new(
        &registry,
        NativeProgramCall {
            response_capacity: 17,
            ..native
        },
        1000,
        &signed
    )
    .is_err());
    Ok(())
}
