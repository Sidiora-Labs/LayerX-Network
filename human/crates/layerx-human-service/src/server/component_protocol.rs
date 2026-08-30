use std::collections::{BTreeMap, BTreeSet};

use serde::Deserialize;
use serde_json::{json, Value};
use sha2::{Digest as _, Sha256};
use zeroize::Zeroize;

use crate::trace::TraceId;

use super::backend::{
    ApiFailure, BackendResponse, PrincipalContext, Readiness, SessionSecrets,
    COMPONENT_PROTOCOL_VERSION,
};
use super::schema::Operation;

const OPERATION_LIMIT: usize = 128;
const TRACE_LIMIT: usize = 128;
const DESTINATION_LIMIT: usize = 2_048;
const SECRET_LIMIT: usize = 4_096;
const SESSION_LIMIT: usize = 255;
const PARAMETER_COUNT_LIMIT: usize = 32;
const PARAMETER_LIMIT: usize = 512;

#[derive(Deserialize)]
#[serde(tag = "kind", deny_unknown_fields)]
pub(super) enum ComponentRequest {
    #[serde(rename = "session.authorize")]
    Authorize {
        version: u64,
        operation: String,
        access_token: String,
        csrf_token: Option<String>,
        intended_destination: String,
        refresh: bool,
        request_digest: String,
        disclosure_digest: String,
        path_parameters: BTreeMap<String, String>,
        body: Value,
        idempotency_key: Option<String>,
        trace: String,
    },
    #[serde(rename = "human-api.execute")]
    Execute {
        version: u64,
        component: String,
        operation: String,
        principal: Option<WirePrincipal>,
        path_parameters: BTreeMap<String, String>,
        body: Value,
        idempotency_key: Option<String>,
        trace: String,
    },
    #[serde(rename = "readiness")]
    Readiness { version: u64, trace: String },
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct WirePrincipal {
    pub principal_id: String,
    pub tenant_id: String,
    pub session_id: String,
    pub capability: String,
    pub request_digest: String,
    pub disclosure_digest: String,
    pub operation: String,
    pub destination: String,
    pub trace: String,
    pub issued_at: u64,
    pub expires_at: u64,
    pub refresh_token: Option<String>,
    pub refresh_csrf: Option<String>,
}

impl ComponentRequest {
    pub(super) fn validate(&self) -> Result<(), ApiFailure> {
        match self {
            Self::Authorize {
                version,
                operation,
                access_token,
                csrf_token,
                intended_destination,
                request_digest,
                disclosure_digest,
                path_parameters,
                body,
                idempotency_key,
                trace,
                ..
            } => {
                valid_version(*version)?;
                valid_operation(operation)?;
                valid_secret(access_token)?;
                if let Some(csrf) = csrf_token {
                    valid_secret(csrf)?;
                }
                if intended_destination.is_empty()
                    || intended_destination.len() > DESTINATION_LIMIT
                    || !intended_destination.starts_with('/')
                    || intended_destination.contains(['\0', '\r', '\n'])
                {
                    return Err(ApiFailure::invalid_request(Some("intended_destination")));
                }
                parse_digest(request_digest, "request_digest")?;
                parse_digest(disclosure_digest, "disclosure_digest")?;
                validate_parameters(path_parameters)?;
                if idempotency_key
                    .as_ref()
                    .is_some_and(|value| !valid_idempotency(value))
                {
                    return Err(ApiFailure::invalid_request(Some("idempotency_key")));
                }
                if json_digest(body)? != parse_digest(disclosure_digest, "disclosure_digest")? {
                    return Err(ApiFailure::unauthenticated());
                }
                valid_trace(trace)
            }
            Self::Execute {
                version,
                component,
                operation,
                principal,
                path_parameters,
                idempotency_key,
                trace,
                ..
            } => {
                valid_version(*version)?;
                valid_operation(operation)?;
                if component.is_empty()
                    || component.len() > OPERATION_LIMIT
                    || !component.bytes().all(|byte| {
                        byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'-'
                    })
                {
                    return Err(ApiFailure::invalid_request(Some("component")));
                }
                if let Some(principal) = principal {
                    if principal.principal_id.is_empty()
                        || principal.principal_id.len() > SESSION_LIMIT
                        || principal.tenant_id.is_empty()
                        || principal.tenant_id.len() > SESSION_LIMIT
                        || principal.session_id.is_empty()
                        || principal.session_id.len() > SESSION_LIMIT
                    {
                        return Err(ApiFailure::invalid_request(Some("principal")));
                    }
                    valid_secret(&principal.capability)?;
                    parse_digest(&principal.request_digest, "request_digest")?;
                    parse_digest(&principal.disclosure_digest, "disclosure_digest")?;
                    valid_operation(&principal.operation)?;
                    if principal.destination.is_empty()
                        || principal.destination.len() > DESTINATION_LIMIT
                        || !principal.destination.starts_with('/')
                        || principal.destination.starts_with("//")
                    {
                        return Err(ApiFailure::invalid_request(Some("principal")));
                    }
                    valid_trace(&principal.trace)?;
                    if principal.expires_at <= principal.issued_at
                        || principal.expires_at.saturating_sub(principal.issued_at) > 60
                        || principal.refresh_token.is_some() != principal.refresh_csrf.is_some()
                    {
                        return Err(ApiFailure::invalid_request(Some("principal")));
                    }
                    if let Some(token) = &principal.refresh_token {
                        valid_secret(token)?;
                    }
                    if let Some(csrf) = &principal.refresh_csrf {
                        valid_secret(csrf)?;
                    }
                }
                validate_parameters(path_parameters)?;
                if idempotency_key
                    .as_ref()
                    .is_some_and(|value| !valid_idempotency(value))
                {
                    return Err(ApiFailure::invalid_request(Some("idempotency_key")));
                }
                valid_trace(trace)
            }
            Self::Readiness { version, trace } => {
                valid_version(*version)?;
                valid_trace(trace)
            }
        }
    }

    pub(super) fn zeroize(&mut self) {
        match self {
            Self::Authorize {
                operation,
                access_token,
                csrf_token,
                intended_destination,
                request_digest,
                disclosure_digest,
                path_parameters,
                body,
                idempotency_key,
                trace,
                ..
            } => {
                operation.zeroize();
                access_token.zeroize();
                if let Some(value) = csrf_token {
                    value.zeroize();
                }
                intended_destination.zeroize();
                request_digest.zeroize();
                disclosure_digest.zeroize();
                for (mut name, mut value) in std::mem::take(path_parameters) {
                    name.zeroize();
                    value.zeroize();
                }
                zeroize_value(body);
                if let Some(value) = idempotency_key {
                    value.zeroize();
                }
                trace.zeroize();
            }
            Self::Execute {
                component,
                operation,
                principal,
                path_parameters,
                body,
                idempotency_key,
                trace,
                ..
            } => {
                component.zeroize();
                operation.zeroize();
                if let Some(principal) = principal {
                    principal.principal_id.zeroize();
                    principal.tenant_id.zeroize();
                    principal.session_id.zeroize();
                    principal.capability.zeroize();
                    principal.request_digest.zeroize();
                    principal.disclosure_digest.zeroize();
                    principal.operation.zeroize();
                    principal.destination.zeroize();
                    principal.trace.zeroize();
                    if let Some(value) = &mut principal.refresh_token {
                        value.zeroize();
                    }
                    if let Some(value) = &mut principal.refresh_csrf {
                        value.zeroize();
                    }
                }
                for (mut name, mut value) in std::mem::take(path_parameters) {
                    name.zeroize();
                    value.zeroize();
                }
                zeroize_value(body);
                if let Some(value) = idempotency_key {
                    value.zeroize();
                }
                trace.zeroize();
            }
            Self::Readiness { trace, .. } => trace.zeroize(),
        }
    }
}

pub(super) fn validate_execute(
    operation: &Operation,
    path_parameters: &BTreeMap<String, String>,
    idempotency_key: Option<&str>,
) -> Result<(), ApiFailure> {
    let declared = operation
        .path
        .split('/')
        .filter_map(|segment| {
            segment
                .strip_prefix('{')
                .and_then(|value| value.strip_suffix('}'))
        })
        .collect::<BTreeSet<_>>();
    let supplied = path_parameters
        .keys()
        .map(String::as_str)
        .collect::<BTreeSet<_>>();
    if declared != supplied {
        return Err(ApiFailure::invalid_request(Some("path_parameters")));
    }
    match (operation.idempotency, idempotency_key) {
        (true, Some(value)) if valid_idempotency(value) => Ok(()),
        (false, None) => Ok(()),
        _ => Err(ApiFailure::invalid_request(Some("idempotency_key"))),
    }
}

pub(super) fn encode_authorized(context: &PrincipalContext) -> Result<Vec<u8>, ApiFailure> {
    serde_json::to_vec(&json!({
        "version": COMPONENT_PROTOCOL_VERSION,
        "ok": true,
        "result": {
            "principal_id": context.principal.as_str(),
            "tenant_id": context.tenant.as_str(),
            "session_id": context.session_id.as_str(),
            "capability": context.capability(),
            "request_digest": hex(&context.request_digest()),
            "disclosure_digest": hex(&context.disclosure_digest()),
            "operation": context.operation(),
            "destination": context.destination(),
            "trace": context.trace(),
            "issued_at": context.issued_at(),
            "expires_at": context.expires_at(),
            "refresh_token": context.refresh_credentials().map(|value| value.0),
            "refresh_csrf": context.refresh_credentials().map(|value| value.1)
        }
    }))
    .map_err(|_| ApiFailure::upstream_degraded())
}

pub(super) fn encode_backend(response: &BackendResponse) -> Result<Vec<u8>, ApiFailure> {
    let mut object = serde_json::Map::new();
    object.insert(
        "version".to_owned(),
        Value::from(COMPONENT_PROTOCOL_VERSION),
    );
    object.insert("ok".to_owned(), Value::Bool(true));
    object.insert("result".to_owned(), response.result.clone());
    if let Some(session) = response.session.as_ref() {
        object.insert("session".to_owned(), session_value(session));
    }
    let mut value = Value::Object(object);
    let encoded = serde_json::to_vec(&value).map_err(|_| ApiFailure::upstream_degraded());
    zeroize_value(&mut value);
    encoded
}

pub(super) fn encode_readiness(readiness: Readiness) -> Result<Vec<u8>, ApiFailure> {
    serde_json::to_vec(&json!({
        "version": COMPONENT_PROTOCOL_VERSION,
        "ok": true,
        "result": {
            "human_service": readiness.human_service.as_str(),
            "custody": readiness.custody.as_str(),
            "agent": readiness.agent.as_str(),
            "core": readiness.core.as_str(),
            "paxeer": readiness.paxeer.as_str()
        }
    }))
    .map_err(|_| ApiFailure::upstream_degraded())
}

pub(super) fn encode_failure(failure: &ApiFailure) -> Result<Vec<u8>, ApiFailure> {
    let mut error = failure.envelope();
    let object = error
        .as_object_mut()
        .ok_or_else(ApiFailure::upstream_degraded)?;
    object.insert("status".to_owned(), Value::from(failure.status));
    serde_json::to_vec(&json!({
        "version": COMPONENT_PROTOCOL_VERSION,
        "ok": false,
        "error": error
    }))
    .map_err(|_| ApiFailure::upstream_degraded())
}

fn session_value(session: &SessionSecrets) -> Value {
    json!({
        "access_token": session.access_token.as_str(),
        "refresh_token": session.refresh_token.as_str(),
        "csrf_token": session.csrf_token.as_str(),
        "access_max_age_seconds": session.access_max_age_seconds,
        "refresh_max_age_seconds": session.refresh_max_age_seconds
    })
}

fn valid_version(version: u64) -> Result<(), ApiFailure> {
    if version == COMPONENT_PROTOCOL_VERSION {
        Ok(())
    } else {
        Err(ApiFailure::invalid_request(Some("version")))
    }
}

fn valid_operation(value: &str) -> Result<(), ApiFailure> {
    if !value.is_empty()
        && value.len() <= OPERATION_LIMIT
        && value.bytes().all(|byte| {
            byte.is_ascii_lowercase() || byte.is_ascii_digit() || matches!(byte, b'.' | b'-')
        })
    {
        Ok(())
    } else {
        Err(ApiFailure::invalid_request(Some("operation")))
    }
}

fn valid_secret(value: &str) -> Result<(), ApiFailure> {
    if !value.is_empty()
        && value.len() <= SECRET_LIMIT
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.' | b'~'))
    {
        Ok(())
    } else {
        Err(ApiFailure::invalid_request(None))
    }
}

fn valid_trace(value: &str) -> Result<(), ApiFailure> {
    if value.len() <= TRACE_LIMIT && TraceId::parse(value).is_ok() {
        Ok(())
    } else {
        Err(ApiFailure::invalid_request(Some("trace")))
    }
}

fn valid_idempotency(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= SESSION_LIMIT
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.' | b':'))
}

pub(super) fn json_digest(value: &Value) -> Result<[u8; 32], ApiFailure> {
    let encoded = serde_json::to_vec(value).map_err(|_| ApiFailure::invalid_request(None))?;
    Ok(Sha256::digest(encoded).into())
}

pub(super) fn authorized_request_digest(
    operation: &Operation,
    destination: &str,
    path_parameters: &BTreeMap<String, String>,
    body: &Value,
    idempotency_key: Option<&str>,
    trace: &str,
) -> Result<[u8; 32], ApiFailure> {
    let mut digest = Sha256::new();
    digest.update(b"layerx-human/authorized-operation/v1\0");
    digest_field(&mut digest, operation.name.as_bytes());
    digest_field(&mut digest, operation.method.as_bytes());
    digest_field(&mut digest, destination.as_bytes());
    for (name, value) in path_parameters {
        digest_field(&mut digest, name.as_bytes());
        digest_field(&mut digest, value.as_bytes());
    }
    let body = serde_json::to_vec(body).map_err(|_| ApiFailure::invalid_request(None))?;
    digest_field(&mut digest, &body);
    digest_field(&mut digest, idempotency_key.unwrap_or_default().as_bytes());
    digest_field(&mut digest, trace.as_bytes());
    Ok(digest.finalize().into())
}

fn digest_field(digest: &mut Sha256, value: &[u8]) {
    digest.update(u64::try_from(value.len()).unwrap_or(u64::MAX).to_be_bytes());
    digest.update(value);
}

pub(super) fn parse_digest(value: &str, field: &str) -> Result<[u8; 32], ApiFailure> {
    if value.len() != 64 {
        return Err(ApiFailure::invalid_request(Some(field)));
    }
    let mut digest = [0_u8; 32];
    for (index, pair) in value.as_bytes().chunks_exact(2).enumerate() {
        let high = nibble(pair[0]).ok_or_else(|| ApiFailure::invalid_request(Some(field)))?;
        let low = nibble(pair[1]).ok_or_else(|| ApiFailure::invalid_request(Some(field)))?;
        digest[index] = high << 4 | low;
    }
    Ok(digest)
}

fn nibble(value: u8) -> Option<u8> {
    match value {
        b'0'..=b'9' => Some(value - b'0'),
        b'a'..=b'f' => Some(value - b'a' + 10),
        _ => None,
    }
}

pub(super) fn hex(value: &[u8; 32]) -> String {
    const DIGITS: &[u8; 16] = b"0123456789abcdef";
    let mut output = String::with_capacity(64);
    for byte in value {
        output.push(char::from(DIGITS[usize::from(byte >> 4)]));
        output.push(char::from(DIGITS[usize::from(byte & 0x0f)]));
    }
    output
}

fn validate_parameters(path_parameters: &BTreeMap<String, String>) -> Result<(), ApiFailure> {
    if path_parameters.len() > PARAMETER_COUNT_LIMIT
        || path_parameters.iter().any(|(name, value)| {
            name.is_empty()
                || name.len() > PARAMETER_LIMIT
                || value.is_empty()
                || value.len() > PARAMETER_LIMIT
                || name.bytes().any(|byte| {
                    !(byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'_')
                })
                || value
                    .bytes()
                    .any(|byte| byte.is_ascii_control() || matches!(byte, b'/' | b'\\'))
        })
    {
        Err(ApiFailure::invalid_request(Some("path_parameters")))
    } else {
        Ok(())
    }
}

fn zeroize_value(value: &mut Value) {
    match value {
        Value::String(text) => text.zeroize(),
        Value::Array(entries) => entries.iter_mut().for_each(zeroize_value),
        Value::Object(entries) => entries.values_mut().for_each(zeroize_value),
        Value::Null | Value::Bool(_) | Value::Number(_) => {}
    }
}
