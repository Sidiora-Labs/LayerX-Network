use super::{
    decode_activity, encode_envelope, hex, ok, parse_hex32, refusal, refusal_response,
    submit_activity, with_session, Config, Envelope, FrameTransport, LniFailure, Request, Response,
    Route, SubmitOutcome, ERROR_RESPONSE_TAG, MAX_ACTIVITY_BYTES,
};
use layerx_client::lni::refusal::decode_core_refusal;
use layerx_client::lni::schema::decode_envelope;

pub(super) fn submit(config: &Config, request: &Request, route: Route) -> Response {
    if request.headers.get("content-type").map(String::as_str) != Some("application/octet-stream")
        || request.body.is_empty()
        || request.body.len() > MAX_ACTIVITY_BYTES
    {
        return refusal(400, "invalid_deployment_body", None);
    }
    let decoded = match decode_activity(config, route, &request.body) {
        Ok(decoded) => decoded,
        Err(response) => return response,
    };
    match with_session(config, |session| {
        submit_activity(config, session, &decoded, 1, &request.body)
    }) {
        Ok(SubmitOutcome::Acknowledged) => Response {
            status: 202,
            body: serde_json::json!({"activity_id": hex(&decoded.activity_id)}).to_string(),
            retry_after: None,
        },
        Ok(SubmitOutcome::Refused(value)) => refusal_response(&value),
        Err(error) => error.response(),
    }
}

pub(super) fn proof(config: &Config, activity: &str) -> Response {
    let Some(id) = parse_hex32(activity).filter(|id| *id != [0; 32]) else {
        return refusal(400, "invalid_activity_id", None);
    };
    let result = with_session(config, |session| {
        let mut payload = vec![0, 1, 4];
        payload.extend_from_slice(&id);
        let correlation_id = session.correlation();
        let version = session.handshake.node().interface_version;
        let request = encode_envelope(Envelope {
            version,
            message_tag: 16,
            correlation_id,
            canonical_payload: &payload,
            proof_material: &[],
        })
        .map_err(|error| LniFailure::Unavailable(format!("{error:?}")))?;
        session
            .transport
            .send(&request)
            .map_err(|error| LniFailure::Transport(format!("{error:?}")))?;
        let bytes = session
            .transport
            .receive()
            .map_err(|error| LniFailure::Transport(format!("{error:?}")))?;
        let response =
            decode_envelope(&bytes).map_err(|error| LniFailure::Transport(format!("{error:?}")))?;
        if response.version != version
            || response.correlation_id != correlation_id
            || !response.proof_material.is_empty()
        {
            return Err(LniFailure::Transport(
                "deployment response binding mismatch".to_owned(),
            ));
        }
        match response.message_tag {
            17 if !response.canonical_payload.is_empty() => Ok(ok(
                serde_json::json!({"proof_hex": hex(response.canonical_payload)}).to_string(),
            )),
            ERROR_RESPONSE_TAG => {
                let value = decode_core_refusal(response.canonical_payload)
                    .ok_or_else(|| LniFailure::Transport("malformed proof refusal".to_owned()))?;
                Ok(Response {
                    status: 503,
                    body: serde_json::json!({"native_result": value.result.raw()}).to_string(),
                    retry_after: None,
                })
            }
            _ => Err(LniFailure::Transport(
                "unexpected proof response tag".to_owned(),
            )),
        }
    });
    result.unwrap_or_else(|error| error.response())
}

pub(super) fn route(
    config: &Config,
    request: &Request,
    path: &str,
    query: Option<&str>,
    plane: super::Plane,
) -> Option<Response> {
    if !path.starts_with("/internal/v1/deployment-proof/")
        && !matches!(
            path,
            "/internal/v1/programs/deploy" | "/internal/v1/programs/upgrade"
        )
    {
        return None;
    }
    if plane != super::Plane::Registry {
        return Some(refusal(403, "entitlement_denied", None));
    }
    if query.is_some() {
        return Some(refusal(404, "not_found", None));
    }
    Some(match (request.method.as_str(), path) {
        ("POST", "/internal/v1/programs/deploy") => submit(config, request, Route::ProgramDeploy),
        ("POST", "/internal/v1/programs/upgrade") => submit(config, request, Route::ProgramUpgrade),
        ("GET", target) if target.starts_with("/internal/v1/deployment-proof/") => {
            proof(config, &target["/internal/v1/deployment-proof/".len()..])
        }
        _ => refusal(404, "not_found", None),
    })
}
