use std::collections::BTreeMap;
use std::fmt::{Debug, Formatter};
use std::path::{Path, PathBuf};
use std::time::Duration;

use layerx_client::lni::transport::{ConnectionGate, FrameTransport, Limits, Uds};
use serde_json::{json, Map, Value};
use zeroize::{Zeroize, Zeroizing};

use crate::store::{AgentTenantId, PrincipalId};

use super::schema::Operation;

/// Stable structured error returned on every human-api failure.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ApiFailure {
    pub status: u16,
    pub code: String,
    pub copy_key: String,
    pub retry: String,
    pub retry_after_ms: Option<u64>,
    pub field: Option<String>,
}

impl ApiFailure {
    #[must_use]
    pub fn invalid_request(field: Option<&str>) -> Self {
        Self {
            status: 400,
            code: "invalid-request".to_owned(),
            copy_key: "error.request.invalid".to_owned(),
            retry: "final".to_owned(),
            retry_after_ms: None,
            field: field.map(str::to_owned),
        }
    }

    #[must_use]
    pub fn unauthenticated() -> Self {
        Self {
            status: 401,
            code: "unauthenticated".to_owned(),
            copy_key: "error.session.required".to_owned(),
            retry: "structural".to_owned(),
            retry_after_ms: None,
            field: None,
        }
    }

    #[must_use]
    pub fn session_expired() -> Self {
        Self {
            status: 401,
            code: "session-expired".to_owned(),
            copy_key: "error.session.expired".to_owned(),
            retry: "structural".to_owned(),
            retry_after_ms: None,
            field: None,
        }
    }

    #[must_use]
    pub fn forbidden() -> Self {
        Self {
            status: 403,
            code: "forbidden".to_owned(),
            copy_key: "error.request.forbidden".to_owned(),
            retry: "final".to_owned(),
            retry_after_ms: None,
            field: None,
        }
    }

    #[must_use]
    pub fn not_found() -> Self {
        Self {
            status: 404,
            code: "not-found".to_owned(),
            copy_key: "error.route.not-found".to_owned(),
            retry: "final".to_owned(),
            retry_after_ms: None,
            field: None,
        }
    }

    #[must_use]
    pub fn rate_limited(retry_after_ms: u64) -> Self {
        Self {
            status: 429,
            code: "rate-limited".to_owned(),
            copy_key: "error.rate-limited".to_owned(),
            retry: "retriable-after".to_owned(),
            retry_after_ms: Some(retry_after_ms),
            field: None,
        }
    }

    #[must_use]
    pub fn unavailable() -> Self {
        Self {
            status: 503,
            code: "unavailable".to_owned(),
            copy_key: "error.service.unavailable".to_owned(),
            retry: "retriable".to_owned(),
            retry_after_ms: None,
            field: None,
        }
    }

    #[must_use]
    pub fn upstream_degraded() -> Self {
        Self {
            status: 503,
            code: "upstream-degraded".to_owned(),
            copy_key: "error.upstream.degraded".to_owned(),
            retry: "retriable".to_owned(),
            retry_after_ms: None,
            field: None,
        }
    }

    #[must_use]
    pub fn envelope(&self) -> Value {
        let mut error = Map::new();
        error.insert("code".to_owned(), Value::String(self.code.clone()));
        error.insert("copy_key".to_owned(), Value::String(self.copy_key.clone()));
        error.insert("retry".to_owned(), Value::String(self.retry.clone()));
        if let Some(retry_after_ms) = self.retry_after_ms {
            error.insert("retry_after_ms".to_owned(), Value::from(retry_after_ms));
        }
        if let Some(field) = &self.field {
            error.insert("field".to_owned(), Value::String(field.clone()));
        }
        Value::Object(error)
    }
}

/// Credentials extracted from protected browser cookies.
pub struct SessionCredentials<'request> {
    pub access_token: &'request str,
    pub csrf_token: Option<&'request str>,
    pub intended_destination: &'request str,
    pub refresh: bool,
    pub request_digest: [u8; 32],
    pub disclosure_digest: [u8; 32],
    pub path_parameters: &'request BTreeMap<String, String>,
    pub body: &'request Value,
    pub idempotency_key: Option<&'request str>,
}

/// The principal and agent tenancy authenticated by the real session service.
pub struct PrincipalContext {
    pub principal: PrincipalId,
    pub tenant: AgentTenantId,
    pub session_id: String,
    capability: Zeroizing<String>,
    request_digest: [u8; 32],
    disclosure_digest: [u8; 32],
    operation: String,
    destination: String,
    trace: String,
    issued_at: u64,
    expires_at: u64,
    refresh_token: Option<Zeroizing<String>>,
    refresh_csrf: Option<Zeroizing<String>>,
}

impl Debug for PrincipalContext {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("PrincipalContext")
            .field("principal", &self.principal)
            .field("tenant", &self.tenant)
            .field("session_id", &self.session_id)
            .field("capability", &"[REDACTED]")
            .field("request_digest", &self.request_digest)
            .field("disclosure_digest", &self.disclosure_digest)
            .field("operation", &self.operation)
            .field("destination", &self.destination)
            .field("trace", &self.trace)
            .field("issued_at", &self.issued_at)
            .field("expires_at", &self.expires_at)
            .field(
                "refresh_token",
                &self.refresh_token.as_ref().map(|_| "[REDACTED]"),
            )
            .finish()
    }
}

impl PrincipalContext {
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn authorized(
        principal: PrincipalId,
        tenant: AgentTenantId,
        session_id: String,
        capability: String,
        request_digest: [u8; 32],
        disclosure_digest: [u8; 32],
        operation: String,
        destination: String,
        trace: String,
        issued_at: u64,
        expires_at: u64,
    ) -> Result<Self, ApiFailure> {
        if session_id.is_empty()
            || capability.len() != 43
            || operation.is_empty()
            || !destination.starts_with('/')
            || destination.starts_with("//")
            || trace.is_empty()
            || trace.len() > 255
            || expires_at <= issued_at
            || expires_at.saturating_sub(issued_at) > 60
        {
            return Err(ApiFailure::upstream_degraded());
        }
        Ok(Self {
            principal,
            tenant,
            session_id,
            capability: Zeroizing::new(capability),
            request_digest,
            disclosure_digest,
            operation,
            destination,
            trace,
            issued_at,
            expires_at,
            refresh_token: None,
            refresh_csrf: None,
        })
    }

    pub(crate) fn with_refresh(mut self, token: String, csrf: String) -> Result<Self, ApiFailure> {
        if token.is_empty() || token.len() > 4_096 || csrf.is_empty() || csrf.len() > 4_096 {
            return Err(ApiFailure::unauthenticated());
        }
        self.refresh_token = Some(Zeroizing::new(token));
        self.refresh_csrf = Some(Zeroizing::new(csrf));
        Ok(self)
    }

    pub(crate) fn capability(&self) -> &str {
        self.capability.as_str()
    }
    pub(crate) const fn request_digest(&self) -> [u8; 32] {
        self.request_digest
    }
    pub(crate) const fn disclosure_digest(&self) -> [u8; 32] {
        self.disclosure_digest
    }
    pub(crate) fn operation(&self) -> &str {
        &self.operation
    }
    pub(crate) fn destination(&self) -> &str {
        &self.destination
    }
    pub(crate) fn trace(&self) -> &str {
        &self.trace
    }
    pub(crate) const fn issued_at(&self) -> u64 {
        self.issued_at
    }
    pub(crate) const fn expires_at(&self) -> u64 {
        self.expires_at
    }
    pub(crate) fn refresh_credentials(&self) -> Option<(&str, &str)> {
        Some((
            self.refresh_token.as_ref()?.as_str(),
            self.refresh_csrf.as_ref()?.as_str(),
        ))
    }
}

/// One schema-decoded request after session authentication and path binding.
pub struct ScopedRequest<'operation> {
    pub operation: &'operation Operation,
    pub principal: Option<PrincipalContext>,
    pub path_parameters: BTreeMap<String, String>,
    pub body: Value,
    pub idempotency_key: Option<String>,
    pub trace: String,
}

/// New browser secrets returned only as protected cookies, never in JSON.
pub struct SessionSecrets {
    pub access_token: String,
    pub refresh_token: String,
    pub csrf_token: String,
    pub access_max_age_seconds: u64,
    pub refresh_max_age_seconds: u64,
}

impl Debug for SessionSecrets {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("SessionSecrets")
            .field("access_token", &"[REDACTED]")
            .field("refresh_token", &"[REDACTED]")
            .field("csrf_token", &"[REDACTED]")
            .field("access_max_age_seconds", &self.access_max_age_seconds)
            .field("refresh_max_age_seconds", &self.refresh_max_age_seconds)
            .finish()
    }
}

impl Drop for SessionSecrets {
    fn drop(&mut self) {
        self.access_token.zeroize();
        self.refresh_token.zeroize();
        self.csrf_token.zeroize();
    }
}

/// One component result plus optional cookie rotation metadata.
#[derive(Debug)]
pub struct BackendResponse {
    pub result: Value,
    pub session: Option<SessionSecrets>,
}

/// Redacted readiness state; explanatory strings never cross the public boundary.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ComponentState {
    Ready,
    Degraded,
    Unavailable,
}

impl ComponentState {
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Ready => "ready",
            Self::Degraded => "degraded",
            Self::Unavailable => "unavailable",
        }
    }
}

/// Readiness of every trust boundary the human plane depends on.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Readiness {
    pub human_service: ComponentState,
    pub custody: ComponentState,
    pub agent: ComponentState,
    pub core: ComponentState,
    pub paxeer: ComponentState,
}

impl Readiness {
    #[must_use]
    pub const fn ready(self) -> bool {
        matches!(self.human_service, ComponentState::Ready)
            && matches!(self.custody, ComponentState::Ready)
            && matches!(self.agent, ComponentState::Ready)
            && matches!(self.core, ComponentState::Ready)
            && matches!(self.paxeer, ComponentState::Ready)
    }

    #[must_use]
    pub fn redacted(self) -> Value {
        json!({
            "ready": self.ready(),
            "components": {
                "human_service": self.human_service.as_str(),
                "custody": self.custody.as_str(),
                "agent": self.agent.as_str(),
                "core": self.core.as_str(),
                "paxeer": self.paxeer.as_str()
            }
        })
    }
}

/// The only product boundary the HTTP router can invoke. Implementations own the
/// real custody, journey, approval, activity, notification, explorer and agent services.
pub trait HumanApiComponents: Send + Sync + 'static {
    /// Authenticates and tenant-binds a session using the durable passkey session service.
    fn authorize(
        &self,
        operation: &Operation,
        credentials: SessionCredentials<'_>,
        trace: &str,
    ) -> Result<PrincipalContext, ApiFailure>;

    /// Dispatches one already schema-decoded request into its real owning component.
    fn execute(&self, request: ScopedRequest<'_>) -> Result<BackendResponse, ApiFailure>;

    /// Reads redacted component readiness without exposing endpoints or failure details.
    fn readiness(&self, trace: &str) -> Result<Readiness, ApiFailure>;
}

/// Production bounded adapter to the process that owns the concrete human services.
/// It uses the same finite framed Unix boundary as the agent/core client and carries
/// principal plus agent tenancy on every authenticated request.
pub struct UnixComponents {
    endpoint: PathBuf,
    gate: ConnectionGate,
    limits: Limits,
}

impl UnixComponents {
    /// Creates a production component boundary only with non-zero finite limits.
    ///
    /// # Errors
    ///
    /// Refuses an empty endpoint or any disabled transport bound.
    pub fn new(endpoint: impl AsRef<Path>, limits: Limits) -> Result<Self, ApiFailure> {
        let endpoint = endpoint.as_ref();
        if endpoint.as_os_str().is_empty() || !endpoint.is_absolute() {
            return Err(ApiFailure::unavailable());
        }
        let limits = limits.validate().map_err(|_| ApiFailure::unavailable())?;
        Ok(Self {
            endpoint: endpoint.to_path_buf(),
            gate: ConnectionGate::new(limits.maximum_connections),
            limits,
        })
    }

    fn round_trip(&self, mut request: Value) -> Result<Value, ApiFailure> {
        let mut connection = Uds::connect(&self.endpoint, &self.gate, self.limits)
            .map_err(|_| ApiFailure::unavailable())?;
        let encoded = serde_json::to_vec(&request);
        zeroize_value(&mut request);
        let mut request_bytes = encoded.map_err(|_| ApiFailure::invalid_request(None))?;
        let send_result = connection.send(&request_bytes);
        request_bytes.zeroize();
        send_result.map_err(|_| ApiFailure::unavailable())?;
        let mut response = connection
            .receive()
            .map_err(|_| ApiFailure::unavailable())?;
        let decoded = serde_json::from_slice(&response);
        response.zeroize();
        let mut value: Value = decoded.map_err(|_| ApiFailure::upstream_degraded())?;
        let ok = value
            .as_object()
            .and_then(|object| object.get("ok"))
            .and_then(Value::as_bool);
        match ok {
            Some(true) => Ok(value),
            Some(false) => {
                let failure = value.as_object().and_then(|object| object.get("error"));
                let parsed = parse_failure(failure);
                zeroize_value(&mut value);
                Err(parsed?)
            }
            None => {
                zeroize_value(&mut value);
                Err(ApiFailure::upstream_degraded())
            }
        }
    }
}

impl HumanApiComponents for UnixComponents {
    fn authorize(
        &self,
        operation: &Operation,
        credentials: SessionCredentials<'_>,
        trace: &str,
    ) -> Result<PrincipalContext, ApiFailure> {
        let mut response = self.round_trip(json!({
            "version": COMPONENT_PROTOCOL_VERSION,
            "kind": "session.authorize",
            "operation": operation.name.as_str(),
            "access_token": credentials.access_token,
            "csrf_token": credentials.csrf_token,
            "intended_destination": credentials.intended_destination,
            "refresh": credentials.refresh,
            "request_digest": hex(&credentials.request_digest),
            "disclosure_digest": hex(&credentials.disclosure_digest),
            "path_parameters": credentials.path_parameters,
            "body": credentials.body,
            "idempotency_key": credentials.idempotency_key,
            "trace": trace
        }))?;
        let parsed = (|| {
            let result = response
                .get("result")
                .and_then(Value::as_object)
                .ok_or_else(ApiFailure::upstream_degraded)?;
            let principal = result
                .get("principal_id")
                .and_then(Value::as_str)
                .ok_or_else(ApiFailure::upstream_degraded)
                .and_then(|value| {
                    PrincipalId::new(value).map_err(|_| ApiFailure::upstream_degraded())
                })?;
            let tenant = result
                .get("tenant_id")
                .and_then(Value::as_str)
                .ok_or_else(ApiFailure::upstream_degraded)
                .and_then(|value| {
                    AgentTenantId::new(value).map_err(|_| ApiFailure::upstream_degraded())
                })?;
            let session_id = result
                .get("session_id")
                .and_then(Value::as_str)
                .filter(|value| !value.is_empty() && value.len() <= 255)
                .ok_or_else(ApiFailure::upstream_degraded)?
                .to_owned();
            let capability = bounded_secret(result, "capability")?;
            let request_digest = digest(result, "request_digest")?;
            let disclosure_digest = digest(result, "disclosure_digest")?;
            let operation_name = bounded_result_text(result, "operation", 128)?;
            let destination = bounded_result_text(result, "destination", 2_048)?;
            let response_trace = bounded_result_text(result, "trace", 255)?;
            let issued_at = result
                .get("issued_at")
                .and_then(Value::as_u64)
                .ok_or_else(ApiFailure::upstream_degraded)?;
            let expires_at = result
                .get("expires_at")
                .and_then(Value::as_u64)
                .filter(|expires| *expires > issued_at)
                .ok_or_else(ApiFailure::upstream_degraded)?;
            if request_digest != credentials.request_digest
                || disclosure_digest != credentials.disclosure_digest
                || operation_name != operation.name
                || destination != credentials.intended_destination
                || response_trace != trace
            {
                return Err(ApiFailure::upstream_degraded());
            }
            PrincipalContext::authorized(
                principal,
                tenant,
                session_id,
                capability,
                request_digest,
                disclosure_digest,
                operation_name,
                destination,
                response_trace,
                issued_at,
                expires_at,
            )
        })();
        zeroize_value(&mut response);
        parsed
    }

    fn execute(&self, request: ScopedRequest<'_>) -> Result<BackendResponse, ApiFailure> {
        let component = component_owner(&request.operation.name)?;
        let principal = request.principal.as_ref().map(|context| {
            json!({
                "principal_id": context.principal.as_str(),
                "tenant_id": context.tenant.as_str(),
                "session_id": context.session_id.as_str(),
                "capability": context.capability.as_str(),
                "request_digest": hex(&context.request_digest),
                "disclosure_digest": hex(&context.disclosure_digest),
                "operation": context.operation.as_str(),
                "destination": context.destination.as_str(),
                "trace": context.trace.as_str(),
                "issued_at": context.issued_at,
                "expires_at": context.expires_at
            })
        });
        let mut response = self.round_trip(json!({
            "version": COMPONENT_PROTOCOL_VERSION,
            "kind": "human-api.execute",
            "component": component,
            "operation": request.operation.name.as_str(),
            "principal": principal,
            "path_parameters": request.path_parameters,
            "body": request.body,
            "idempotency_key": request.idempotency_key,
            "trace": request.trace
        }))?;
        let parsed = (|| {
            let object = response
                .as_object_mut()
                .ok_or_else(ApiFailure::upstream_degraded)?;
            let result = object
                .remove("result")
                .ok_or_else(ApiFailure::upstream_degraded)?;
            let session = if let Some(mut value) = object.remove("session") {
                let parsed = parse_session(&value);
                zeroize_value(&mut value);
                Some(parsed?)
            } else {
                None
            };
            Ok(BackendResponse { result, session })
        })();
        zeroize_value(&mut response);
        parsed
    }

    fn readiness(&self, trace: &str) -> Result<Readiness, ApiFailure> {
        let mut response = self.round_trip(json!({
            "version": COMPONENT_PROTOCOL_VERSION,
            "kind": "readiness",
            "trace": trace
        }))?;
        let parsed = (|| {
            let result = response
                .get("result")
                .and_then(Value::as_object)
                .ok_or_else(ApiFailure::upstream_degraded)?;
            Ok(Readiness {
                human_service: parse_component_state(result.get("human_service"))?,
                custody: parse_component_state(result.get("custody"))?,
                agent: parse_component_state(result.get("agent"))?,
                core: parse_component_state(result.get("core"))?,
                paxeer: parse_component_state(result.get("paxeer"))?,
            })
        })();
        zeroize_value(&mut response);
        parsed
    }
}

fn bounded_secret(result: &Map<String, Value>, name: &str) -> Result<String, ApiFailure> {
    bounded_result_text(result, name, 4_096).and_then(|value| {
        if value.len() < 43
            || !value
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_'))
        {
            Err(ApiFailure::upstream_degraded())
        } else {
            Ok(value)
        }
    })
}

fn bounded_result_text(
    result: &Map<String, Value>,
    name: &str,
    maximum: usize,
) -> Result<String, ApiFailure> {
    result
        .get(name)
        .and_then(Value::as_str)
        .filter(|value| !value.is_empty() && value.len() <= maximum)
        .map(str::to_owned)
        .ok_or_else(ApiFailure::upstream_degraded)
}

fn digest(result: &Map<String, Value>, name: &str) -> Result<[u8; 32], ApiFailure> {
    let text = result
        .get(name)
        .and_then(Value::as_str)
        .filter(|value| value.len() == 64)
        .ok_or_else(ApiFailure::upstream_degraded)?;
    let mut output = [0_u8; 32];
    for (index, pair) in text.as_bytes().chunks_exact(2).enumerate() {
        output[index] =
            hex_nibble(pair[0]).and_then(|high| hex_nibble(pair[1]).map(|low| high << 4 | low))?;
    }
    Ok(output)
}

fn hex_nibble(value: u8) -> Result<u8, ApiFailure> {
    match value {
        b'0'..=b'9' => Ok(value - b'0'),
        b'a'..=b'f' => Ok(value - b'a' + 10),
        _ => Err(ApiFailure::upstream_degraded()),
    }
}

fn hex(value: &[u8; 32]) -> String {
    const DIGITS: &[u8; 16] = b"0123456789abcdef";
    let mut output = String::with_capacity(64);
    for byte in value {
        output.push(char::from(DIGITS[usize::from(byte >> 4)]));
        output.push(char::from(DIGITS[usize::from(byte & 0x0f)]));
    }
    output
}

fn zeroize_value(value: &mut Value) {
    match value {
        Value::String(text) => text.zeroize(),
        Value::Array(entries) => entries.iter_mut().for_each(zeroize_value),
        Value::Object(entries) => entries.values_mut().for_each(zeroize_value),
        Value::Null | Value::Bool(_) | Value::Number(_) => {}
    }
}

pub(super) fn component_owner(operation: &str) -> Result<&'static str, ApiFailure> {
    if operation == "account.balance" {
        return Ok("agent");
    }
    let root = operation.split('.').next().unwrap_or_default();
    match root {
        "account" | "authenticator" | "passkey" | "profile" | "security" | "session" | "stepup" => {
            Ok("custody")
        }
        "binding" => Ok("custody"),
        "deposit" | "exit" | "journey" | "move" | "withdraw" => Ok("journeys"),
        "agent" => Ok("agents"),
        "approval" => Ok("approvals"),
        "activity" | "evidence" => Ok("activity-explorer"),
        "notification" | "stream" => Ok("notifications"),
        "onboarding" => Ok("onboarding"),
        "support" => Ok("support"),
        "home" => Ok("home"),
        "version" => Ok("service"),
        _ => Err(ApiFailure::not_found()),
    }
}

pub(crate) const COMPONENT_PROTOCOL_VERSION: u64 = 1;

fn parse_session(value: &Value) -> Result<SessionSecrets, ApiFailure> {
    let object = value
        .as_object()
        .ok_or_else(ApiFailure::upstream_degraded)?;
    let secret = |name: &str| {
        object
            .get(name)
            .and_then(Value::as_str)
            .filter(|value| {
                !value.is_empty()
                    && value.len() <= 4096
                    && value.bytes().all(|byte| {
                        byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.' | b'~')
                    })
            })
            .map(str::to_owned)
            .ok_or_else(ApiFailure::upstream_degraded)
    };
    Ok(SessionSecrets {
        access_token: secret("access_token")?,
        refresh_token: secret("refresh_token")?,
        csrf_token: secret("csrf_token")?,
        access_max_age_seconds: object
            .get("access_max_age_seconds")
            .and_then(Value::as_u64)
            .ok_or_else(ApiFailure::upstream_degraded)?,
        refresh_max_age_seconds: object
            .get("refresh_max_age_seconds")
            .and_then(Value::as_u64)
            .ok_or_else(ApiFailure::upstream_degraded)?,
    })
}

fn parse_component_state(value: Option<&Value>) -> Result<ComponentState, ApiFailure> {
    match value.and_then(Value::as_str) {
        Some("ready") => Ok(ComponentState::Ready),
        Some("degraded") => Ok(ComponentState::Degraded),
        Some("unavailable") => Ok(ComponentState::Unavailable),
        _ => Err(ApiFailure::upstream_degraded()),
    }
}

fn parse_failure(value: Option<&Value>) -> Result<ApiFailure, ApiFailure> {
    let object = value
        .and_then(Value::as_object)
        .ok_or_else(ApiFailure::upstream_degraded)?;
    let status = object
        .get("status")
        .and_then(Value::as_u64)
        .and_then(|value| u16::try_from(value).ok())
        .filter(|value| (400..600).contains(value))
        .ok_or_else(ApiFailure::upstream_degraded)?;
    let text = |name: &str| {
        object
            .get(name)
            .and_then(Value::as_str)
            .filter(|value| !value.is_empty() && value.len() <= 255)
            .map(str::to_owned)
            .ok_or_else(ApiFailure::upstream_degraded)
    };
    let code = text("code")?;
    let copy_key = text("copy_key")?;
    let retry = text("retry")?;
    if !ERROR_CODES.contains(&code.as_str())
        || !matches!(
            retry.as_str(),
            "retriable" | "retriable-after" | "structural" | "final"
        )
        || !copy_key.bytes().all(|byte| {
            byte.is_ascii_lowercase() || byte.is_ascii_digit() || matches!(byte, b'.' | b'_' | b'-')
        })
    {
        return Err(ApiFailure::upstream_degraded());
    }
    let retry_after_ms = object.get("retry_after_ms").and_then(Value::as_u64);
    if (code == "rate-limited" || retry == "retriable-after") && retry_after_ms.is_none() {
        return Err(ApiFailure::upstream_degraded());
    }
    let field = object
        .get("field")
        .and_then(Value::as_str)
        .map(str::to_owned);
    if field
        .as_ref()
        .is_some_and(|value| value.is_empty() || value.len() > 255)
    {
        return Err(ApiFailure::upstream_degraded());
    }
    Ok(ApiFailure {
        status,
        code,
        copy_key,
        retry,
        retry_after_ms,
        field,
    })
}

const ERROR_CODES: &[&str] = &[
    "unauthenticated",
    "session-expired",
    "step-up-required",
    "forbidden",
    "not-found",
    "invalid-request",
    "conflict",
    "rate-limited",
    "cursor-expired",
    "unavailable",
    "upstream-degraded",
    "challenge-expired",
    "refused-by-policy",
    "refused-by-budget",
    "refused-by-capability",
    "refused-by-protocol",
    "refused-by-limit",
    "quote-expired",
    "wallet-not-bound",
    "exit-unavailable",
    "already-decided",
    "hold-expired",
    "hold-defective",
    "archive-needs-disposition",
    "confirmation-mismatch",
    "not-suppressible",
    "support-unavailable",
    "support-conversation-unknown",
    "support-message-unknown",
];

#[must_use]
pub const fn default_component_limits() -> Limits {
    Limits {
        maximum_frame_bytes: 1_048_576,
        maximum_connections: 128,
        maximum_streams: 128,
        maximum_queued_bytes: 8_388_608,
        deadline: Duration::from_secs(10),
    }
}
