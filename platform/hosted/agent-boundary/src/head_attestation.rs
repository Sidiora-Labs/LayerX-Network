use super::{
    hex, ok, parse_hex32, refusal, refusal_response, result_refusal, with_session, Config,
    LniFailure, Plane, Request, Response,
};
use layerx_client::lni::head_attestation::{
    attest_program_head, encode_program_head_attestation, ProgramHeadAttestContext,
    ProgramHeadAttestError,
};
use layerx_client::lni::schema::Capability;

const PREFIX: &str = "/internal/v1/programs/";
const SUFFIX: &str = "/head-attestation";
const STALENESS_QUERY: &str = "staleness_ms=";

fn staleness_ms(query: Option<&str>) -> Option<u64> {
    let value = query?.strip_prefix(STALENESS_QUERY)?;
    if value.is_empty()
        || value.len() > 20
        || value.starts_with('0')
        || !value.bytes().all(|byte| byte.is_ascii_digit())
    {
        return None;
    }
    value.parse::<u64>().ok()
}

fn attest(config: &Config, program_id: [u8; 32], staleness_ms: u64) -> Response {
    let outcome = with_session(config, |session| {
        if !session
            .handshake
            .capabilities()
            .contains(Capability::ProgramHeadAttest)
        {
            return Ok(Err(refusal(503, "capability_unavailable", Some(60))));
        }
        let context = ProgramHeadAttestContext {
            interface_version: session.handshake.node().interface_version,
            sequencer_public_key: session.handshake.node().authorised_sequencer_key,
            correlation_id: session.correlation(),
            program_id,
            staleness_ms,
        };
        match attest_program_head(&mut session.transport, context) {
            Ok(attestation) => Ok(Ok(attestation)),
            Err(ProgramHeadAttestError::HeadStale) => Ok(Err(refusal(503, "head_stale", Some(1)))),
            Err(ProgramHeadAttestError::UnknownProgram) => {
                Ok(Err(refusal(404, "program_not_registered", None)))
            }
            Err(ProgramHeadAttestError::CoreRefusal { class, result }) => {
                if class == 3 {
                    Ok(Err(refusal(503, "capability_unavailable", Some(60))))
                } else {
                    Ok(Err(refusal_response(&result_refusal(result))))
                }
            }
            Err(ProgramHeadAttestError::MalformedRequest) => Ok(Err(refusal(
                400,
                "malformed_head_attestation_request",
                None,
            ))),
            Err(
                ProgramHeadAttestError::UnavailableCapability
                | ProgramHeadAttestError::InterfaceVersion(_),
            ) => Ok(Err(refusal(503, "capability_unavailable", Some(60)))),
            Err(error) => Err(LniFailure::Transport(format!(
                "program head attestation failed: {error:?}"
            ))),
        }
    });
    let attestation = match outcome {
        Ok(Ok(attestation)) => attestation,
        Ok(Err(response)) => return response,
        Err(failure) => return failure.response(),
    };
    let (payload, proof) = encode_program_head_attestation(&attestation);
    ok(serde_json::json!({"payload_hex": hex(&payload), "proof_hex": hex(&proof)}).to_string())
}

pub(super) fn route(
    config: &Config,
    request: &Request,
    path: &str,
    query: Option<&str>,
    plane: Plane,
) -> Option<Response> {
    let program = path.strip_prefix(PREFIX)?.strip_suffix(SUFFIX)?;
    if plane != Plane::Registry {
        return Some(refusal(403, "entitlement_denied", None));
    }
    if request.method != "GET" {
        return Some(refusal(404, "not_found", None));
    }
    if program.bytes().any(|byte| byte.is_ascii_uppercase()) {
        return Some(refusal(400, "invalid_program_id", None));
    }
    let Some(program_id) = parse_hex32(program).filter(|id| *id != [0; 32]) else {
        return Some(refusal(400, "invalid_program_id", None));
    };
    let Some(staleness_ms) = staleness_ms(query) else {
        return Some(refusal(400, "invalid_staleness_ms", None));
    };
    Some(attest(config, program_id, staleness_ms))
}
