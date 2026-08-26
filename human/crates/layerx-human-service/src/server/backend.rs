use std::collections::BTreeMap;
use std::fmt::{Debug, Formatter};
use std::path::{Path, PathBuf};
use std::time::Duration;

use layerx_client::lni::transport::{ConnectionGate, FrameTransport, Limits, Uds};
use serde_json::{json, Map, Value};
use zeroize::Zeroize;

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
}

/// The principal and agent tenancy authenticated by the real session service.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PrincipalContext {
    pub principal: PrincipalId,
    pub tenant: AgentTenantId,
    pub session_id: String,
}

/// One schema-decoded request after session authentication and path binding.
pub struct ScopedRequest<'operation> {
    pub operation: &'operation Operation,
    pub principal: Option<&'operation PrincipalContext>,
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
        !matches!(self.human_service, ComponentState::Unavailable)
            && !matches!(self.custody, ComponentState::Unavailable)
            && !matches!(self.agent, ComponentState::Unavailable)
            && !matches!(self.core, ComponentState::Unavailable)
            && !matches!(self.paxeer, ComponentState::Unavailable)
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
        let mut response = connection.receive().map_err(|_| ApiFailure::unavailable())?;
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
                let failure = value
                    .as_object()
                    .and_then(|object| object.get("error"));
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
            "kind": "session.authorize",
            "operation": operation.name.as_str(),
            "access_token": credentials.access_token,
            "csrf_token": credentials.csrf_token,
            "intended_destination": credentials.intended_destination,
            "refresh": credentials.refresh,
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
            Ok(PrincipalContext {
                principal,
                tenant,
                session_id,
            })
        })();
        zeroize_value(&mut response);
        parsed
    }

    fn execute(&self, request: ScopedRequest<'_>) -> Result<BackendResponse, ApiFailure> {
        let component = component_owner(&request.operation.name);
        let principal = request.principal.map(|context| {
            json!({
                "principal_id": context.principal.as_str(),
                "tenant_id": context.tenant.as_str(),
                "session_id": context.session_id.as_str()
            })
        });
        let mut response = self.round_trip(json!({
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
            "kind": "readiness",
            "trace": trace
        }))?;
        let parsed = (|| {
            let result = response
                .get("result")
                .and_then(Value::as_object)
                .ok_or_else(ApiFailure::upstream_degraded)?;
            Ok(Readiness {
                human_service: ComponentState::Ready,
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

fn zeroize_value(value: &mut Value) {
    match value {
        Value::String(text) => text.zeroize(),
        Value::Array(entries) => entries.iter_mut().for_each(zeroize_value),
        Value::Object(entries) => entries.values_mut().for_each(zeroize_value),
        Value::Null | Value::Bool(_) | Value::Number(_) => {}
    }
}

fn component_owner(operation: &str) -> &'static str {
    if operation == "account.balance" {
        return "agent";
    }
    let root = operation.split('.').next().unwrap_or_default();
    match root {
        "account" | "authenticator" | "passkey" | "profile" | "security" | "session"
        | "stepup" => "custody",
        "binding" => "custody",
        "deposit" | "exit" | "journey" | "move" | "withdraw" => "journeys",
        "agent" => "agents",
        "approval" => "approvals",
        "activity" | "evidence" => "activity-explorer",
        "notification" | "stream" => "notifications",
        "onboarding" => "onboarding",
        "support" => "support",
        "home" => "home",
        "version" => "service",
        _ => "agent",
    }
}

fn parse_session(value: &Value) -> Result<SessionSecrets, ApiFailure> {
    let object = value.as_object().ok_or_else(ApiFailure::upstream_degraded)?;
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
        || !matches!(retry.as_str(), "retriable" | "retriable-after" | "structural" | "final")
        || !copy_key.bytes().all(|byte| {
            byte.is_ascii_lowercase()
                || byte.is_ascii_digit()
                || matches!(byte, b'.' | b'_' | b'-')
        })
    {
        return Err(ApiFailure::upstream_degraded());
    }
    let retry_after_ms = object.get("retry_after_ms").and_then(Value::as_u64);
    if (code == "rate-limited" || retry == "retriable-after") && retry_after_ms.is_none() {
        return Err(ApiFailure::upstream_degraded());
    }
    let field = object.get("field").and_then(Value::as_str).map(str::to_owned);
    if field.as_ref().is_some_and(|value| value.is_empty() || value.len() > 255) {
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
