use layerx_types::intent::ProgramId;
use layerx_types::payload::{ModuleId, ModuleRegistry};
use layerx_types::program_call::{NativeProgramCall, Resources};
use layerx_wire::activity::decode_signed;
use serde::Deserialize;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Budget {
    fuel: String,
    fee_limit: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct NativeFields {
    guest_abi: u16,
    entrypoint: String,
    capabilities_hex: String,
    access_declaration_hex: String,
    response_capacity: u32,
    resources: [String; 7],
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Request {
    payload_encoding: String,
    program_id: String,
    calldata: String,
    budget: Budget,
    signed_activity: String,
    native_call: NativeFields,
}

fn bytes(value: &str, maximum: usize) -> Result<Vec<u8>, String> {
    if !value
        .bytes()
        .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
    {
        return Err("native call hexadecimal is not canonical".to_owned());
    }
    if value.len() / 2 > maximum {
        return Err("native call hexadecimal exceeds its bound".to_owned());
    }
    super::hex_decode(value)
}

fn decimal<T: std::str::FromStr + ToString>(value: &str) -> Result<T, String> {
    let parsed: T = value
        .parse()
        .map_err(|_| "native call decimal is invalid")?;
    if parsed.to_string() != value {
        return Err("native call decimal is not canonical".to_owned());
    }
    Ok(parsed)
}

type BoundNativeCall = (Vec<u8>, [u8; 32]);

pub(super) fn parse_json(
    body: &[u8],
    registry: &ModuleRegistry,
) -> Result<Option<BoundNativeCall>, String> {
    let value: serde_json::Value =
        serde_json::from_slice(body).map_err(|_| "program call body is invalid")?;
    if value.get("payload_encoding").is_none() {
        return Ok(None);
    }
    let request: Request =
        serde_json::from_slice(body).map_err(|_| "native call body is invalid")?;
    if request.payload_encoding != "native-v1" {
        return Err("native call encoding is unsupported".to_owned());
    }
    let program_id: [u8; 32] = bytes(&request.program_id, 32)?
        .try_into()
        .map_err(|_| "native program id is invalid")?;
    let calldata = bytes(&request.calldata, 1_048_576)?;
    let capabilities = bytes(&request.native_call.capabilities_hex, 65_535)?;
    let access = bytes(&request.native_call.access_declaration_hex, 1_048_576)?;
    let mut resources = [0; 7];
    for (destination, source) in resources.iter_mut().zip(&request.native_call.resources) {
        *destination = decimal(source)?;
    }
    let fee_limit: u128 = decimal(&request.budget.fee_limit)?;
    let fuel: u64 = decimal(&request.budget.fuel)?;
    if fuel == 0 || fuel != resources[0] {
        return Err("native call resource binding is invalid".to_owned());
    }
    let native = NativeProgramCall {
        program_id: ProgramId::new(program_id),
        guest_abi: request.native_call.guest_abi,
        entrypoint: request.native_call.entrypoint.as_bytes(),
        calldata: &calldata,
        capabilities: &capabilities,
        access_declaration: &access,
        response_capacity: request.native_call.response_capacity,
        resources: Resources(resources),
    };
    let expected = native
        .encode()
        .map_err(|_| "native call payload is invalid")?;
    let signed = bytes(&request.signed_activity, 1_048_576)?;
    let activity =
        decode_signed(&signed, registry).map_err(|_| "native signed activity is invalid")?;
    let actual_program = from_activity(&activity)?;
    if activity.fee_limit() != fee_limit
        || activity.payload() != expected
        || actual_program != program_id
    {
        return Err("native signed activity does not match request".to_owned());
    }
    Ok(Some((signed, program_id)))
}

pub(super) fn from_activity(
    activity: &layerx_wire::activity::Activity,
) -> Result<[u8; 32], String> {
    if activity.protocol_version() != 3
        || activity.activity_type().module() != ModuleId::Programs
        || activity.activity_type().ordinal() != 3
    {
        return Err("native activity scope is invalid".to_owned());
    }
    let call = NativeProgramCall::decode(activity.payload())
        .map_err(|_| "native program payload is invalid")?;
    Ok(call.program_id.bytes())
}

#[cfg(test)]
mod tests {
    use super::*;
    use layerx_types::payload::{ActivityType, ModuleRegistration};

    fn fixture() -> Result<(serde_json::Value, ModuleRegistry), Box<dyn std::error::Error>> {
        let vector: serde_json::Value = serde_json::from_str(include_str!(
            "../../sdk/conformance/fixtures/native-program-call-v3.json"
        ))?;
        let registry = ModuleRegistry::new(&[ModuleRegistration::new(
            ModuleId::Programs,
            &[ActivityType::new(ModuleId::Programs, 3).map_err(|_| "activity type invalid")?],
        )
        .map_err(|_| "registration invalid")?])
        .map_err(|_| "registry invalid")?;
        let body = serde_json::json!({
            "payload_encoding":"native-v1", "program_id":"11".repeat(32), "calldata":"",
            "budget":{"fuel":"1000000","fee_limit":"1000"}, "signed_activity":vector["signed_activity_hex"],
            "native_call":{"guest_abi":1,"entrypoint":"layerx_call","capabilities_hex":"0000",
                "access_declaration_hex":super::super::hex_encode(b"LayerX/programs/access-declaration/v1\0\0"),
                "response_capacity":16,"resources":vector["resources"]}
        });
        Ok((body, registry))
    }

    #[test]
    fn exact_native_json_and_scope_bindings() -> Result<(), Box<dyn std::error::Error>> {
        let (body, registry) = fixture()?;
        let encoded = serde_json::to_vec(&body)?;
        let verified = parse_json(&encoded, &registry)?.ok_or("native request not selected")?;
        assert_eq!(verified.1, [0x11; 32]);
        for (path, changed) in [
            ("/budget/fee_limit", serde_json::json!("999")),
            ("/budget/fuel", serde_json::json!("999")),
            ("/native_call/guest_abi", serde_json::json!(2)),
            ("/native_call/entrypoint", serde_json::json!("other")),
            ("/native_call/capabilities_hex", serde_json::json!("0001")),
            (
                "/native_call/access_declaration_hex",
                serde_json::json!("00"),
            ),
            ("/native_call/response_capacity", serde_json::json!(17)),
            ("/native_call/resources/1", serde_json::json!("999")),
            ("/program_id", serde_json::json!("22".repeat(32))),
            ("/calldata", serde_json::json!("00")),
        ] {
            let mut altered = body.clone();
            *altered.pointer_mut(path).ok_or("fixture path missing")? = changed;
            assert!(
                parse_json(&serde_json::to_vec(&altered)?, &registry).is_err(),
                "{path}"
            );
        }
        let mut extra = body.clone();
        extra["capabilities"] = serde_json::json!([]);
        assert!(parse_json(&serde_json::to_vec(&extra)?, &registry).is_err());
        let duplicate =
            String::from_utf8(encoded)?.replacen('{', "{\"payload_encoding\":\"native-v1\",", 1);
        assert!(parse_json(duplicate.as_bytes(), &registry).is_err());
        Ok(())
    }
    #[test]
    fn native_json_and_octet_routes_retain_original_signed_bytes(
    ) -> Result<(), Box<dyn std::error::Error>> {
        let (body, registry) = fixture()?;
        let encoded = serde_json::to_vec(&body)?;
        let (signed, program_id) =
            parse_json(&encoded, &registry)?.ok_or("native request missing")?;
        for (content_type, body) in [
            ("application/json", encoded),
            ("application/octet-stream", signed.clone()),
        ] {
            let request = super::super::Request {
                method: "POST".to_owned(),
                path: "/v1/programs/call".to_owned(),
                content_type: content_type.to_owned(),
                idempotency_key: None,
                body,
            };
            let decoded = super::super::decode_program_activity(&request)?;
            assert_eq!(decoded.signed, signed);
            assert_eq!(decoded.program_id, program_id);
            assert_eq!(decoded.protocol_version, 3);
        }
        let legacy = serde_json::json!({"program_id":body["program_id"], "calldata":"", "budget":body["budget"],
            "capabilities":[], "signed_activity":body["signed_activity"]});
        let request = super::super::Request {
            method: "POST".to_owned(),
            path: "/v1/programs/call".to_owned(),
            content_type: "application/json".to_owned(),
            idempotency_key: None,
            body: serde_json::to_vec(&legacy)?,
        };
        assert!(super::super::decode_program_activity(&request).is_err());
        Ok(())
    }
}
