use std::collections::{BTreeMap, BTreeSet};

use serde::Deserialize;
use serde_json::{json, Value};
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
    pub authorization: String,
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
                    valid_secret(&principal.authorization)?;
                }
                if path_parameters.len() > PARAMETER_COUNT_LIMIT
                    || path_parameters.iter().any(|(name, value)| {
                        name.is_empty()
                            || name.len() > PARAMETER_LIMIT
                            || value.is_empty()
                            || value.len() > PARAMETER_LIMIT
                            || name.bytes().any(|byte| {
                                !(byte.is_ascii_lowercase()
                                    || byte.is_ascii_digit()
                                    || byte == b'_')
                            })
                            || value
                                .bytes()
                                .any(|byte| byte.is_ascii_control() || matches!(byte, b'/' | b'\\'))
                    })
                {
                    return Err(ApiFailure::invalid_request(Some("path_parameters")));
                }
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
                trace,
                ..
            } => {
                operation.zeroize();
                access_token.zeroize();
                if let Some(value) = csrf_token {
                    value.zeroize();
                }
                intended_destination.zeroize();
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
                    principal.authorization.zeroize();
                }
                for (name, value) in path_parameters {
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
    let supplied = path_parameters.keys().map(String::as_str).collect::<BTreeSet<_>>();
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
            "authorization": context.authorization.as_str()
        }
    }))
    .map_err(|_| ApiFailure::upstream_degraded())
}

pub(super) fn encode_backend(response: &BackendResponse) -> Result<Vec<u8>, ApiFailure> {
    let mut object = serde_json::Map::new();
    object.insert("version".to_owned(), Value::from(COMPONENT_PROTOCOL_VERSION));
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
        && value.bytes().all(|byte| {
            byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.' | b'~')
        })
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
        && value.bytes().all(|byte| {
            byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.' | b':')
        })
}

fn zeroize_value(value: &mut Value) {
    match value {
        Value::String(text) => text.zeroize(),
        Value::Array(entries) => entries.iter_mut().for_each(zeroize_value),
        Value::Object(entries) => entries.values_mut().for_each(zeroize_value),
        Value::Null | Value::Bool(_) | Value::Number(_) => {}
    }
}
