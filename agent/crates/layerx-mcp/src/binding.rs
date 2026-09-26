//! Operator-declared binding that turns daemon-owned records into one served protocol session.

use std::fmt;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use layerx_agentd::budget::{BudgetLimiter, LimitConfig, LimitId, LimitScope};
use layerx_agentd::capability::CapabilityId;
use layerx_agentd::config::read_protected_source;
use layerx_agentd::policy::approval::ApprovalRegistry;
use layerx_agentd::prepare::PreparationLifecycle;
use layerx_agentd::session::{SessionCredential, SessionId, SessionRegistry};
use layerx_agentd::session_control::SessionControl;
use layerx_agentd::store::{Store, TenantId};
use serde_json::{Map, Value};
use zeroize::Zeroizing;

use crate::approval::ApprovalPolicy;
use crate::boundary::{AgentSurface, BoundaryRefusal, ProgramReads, ToolBoundary};
use crate::listener::ListenerConfig;
use crate::server::{DeploymentMode, ReadOnly, Server, ToolDefinition, WebBoundary, WebRoute};
use crate::stdio::{Bound, Session};
use crate::tools::web::{ExactTerms, GrantTerms, WebConfig, WebPayer, WebPayerError, WebToolError};

const MAX_DOCUMENT_BYTES: usize = 65_536;
const MAX_SECRET_BYTES: usize = 4_096;
const MAX_TEXT_BYTES: usize = 255;
const MAX_ADMITTED_PEERS: usize = 64;

const BINDING_KEYS: [&str; 12] = [
    "mode",
    "tenant",
    "store",
    "audit_root",
    "session_id",
    "session_token_file",
    "session_generation",
    "capability_id",
    "core_sequence",
    "deadline_ms",
    "agent",
    "limit",
];
const AGENT_KEYS: [&str; 3] = ["endpoint", "bearer_file", "probe_program"];
const LIMIT_KEYS: [&str; 6] = ["id", "name", "scope", "scope_id", "ceiling", "consumed"];
const LISTENER_KEYS: [&str; 5] = ["socket", "owner_uid", "owner_gid", "mode", "admitted_uids"];
const WEB_KEYS: [&str; 6] = [
    "endpoint",
    "network",
    "sequencer_public_key",
    "timeout_ms",
    "pending_attempts",
    "approval_threshold",
];

/// Typed refusal of one binding document. It never echoes secret material.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum BindingError {
    Unreadable(String),
    Malformed(String),
    Refused(String),
}

impl BindingError {
    /// Renders the refusal for an operator without exposing file contents.
    #[must_use]
    pub fn detail(&self) -> String {
        match self {
            Self::Unreadable(reason) => format!("the binding document is unreadable: {reason}"),
            Self::Malformed(reason) => format!("the binding document is malformed: {reason}"),
            Self::Refused(reason) => format!("the daemon refused the binding: {reason}"),
        }
    }
}

fn malformed(reason: impl Into<String>) -> BindingError {
    BindingError::Malformed(reason.into())
}

fn closed(object: &Map<String, Value>, accepted: &[&str], scope: &str) -> Result<(), BindingError> {
    for key in object.keys() {
        if !accepted.contains(&key.as_str()) {
            return Err(malformed(format!("{scope} field {key} is not accepted")));
        }
    }
    Ok(())
}

fn object<'a>(
    parent: &'a Map<String, Value>,
    field: &str,
) -> Result<&'a Map<String, Value>, BindingError> {
    parent
        .get(field)
        .and_then(Value::as_object)
        .ok_or_else(|| malformed(format!("field {field} must be an object")))
}

fn text<'a>(parent: &'a Map<String, Value>, field: &str) -> Result<&'a str, BindingError> {
    let value = parent
        .get(field)
        .and_then(Value::as_str)
        .ok_or_else(|| malformed(format!("field {field} must be a string")))?;
    if value.is_empty() || value.len() > MAX_TEXT_BYTES {
        return Err(malformed(format!(
            "field {field} must be 1 to {MAX_TEXT_BYTES} bytes"
        )));
    }
    Ok(value)
}

fn unsigned(parent: &Map<String, Value>, field: &str) -> Result<u64, BindingError> {
    parent
        .get(field)
        .and_then(Value::as_u64)
        .ok_or_else(|| malformed(format!("field {field} must be an unsigned integer")))
}

fn unsigned32(parent: &Map<String, Value>, field: &str) -> Result<u32, BindingError> {
    u32::try_from(unsigned(parent, field)?)
        .map_err(|_| malformed(format!("field {field} is outside its unsigned range")))
}

fn wide(parent: &Map<String, Value>, field: &str) -> Result<u128, BindingError> {
    text(parent, field)?
        .parse::<u128>()
        .map_err(|_| malformed(format!("field {field} must be a decimal unsigned integer")))
}

fn digest<const N: usize>(
    parent: &Map<String, Value>,
    field: &str,
) -> Result<[u8; N], BindingError> {
    let value = text(parent, field)?;
    let expected = N.saturating_mul(2);
    if value.len() != expected {
        return Err(malformed(format!(
            "field {field} must be {N} hexadecimal bytes"
        )));
    }
    let mut bytes = [0_u8; N];
    for (index, byte) in bytes.iter_mut().enumerate() {
        let start = index.checked_mul(2).ok_or_else(|| malformed("overflow"))?;
        let end = start.checked_add(2).ok_or_else(|| malformed("overflow"))?;
        let pair = value
            .get(start..end)
            .ok_or_else(|| malformed(format!("field {field} is truncated")))?;
        *byte = u8::from_str_radix(pair, 16)
            .map_err(|_| malformed(format!("field {field} is not hexadecimal")))?;
    }
    Ok(bytes)
}

fn absolute(parent: &Map<String, Value>, field: &str) -> Result<PathBuf, BindingError> {
    let path = PathBuf::from(text(parent, field)?);
    if path.is_absolute() {
        Ok(path)
    } else {
        Err(malformed(format!("field {field} must be an absolute path")))
    }
}

/// The daemon surface one session reaches after authorization.
#[derive(Clone, Debug, Eq, PartialEq)]
struct AgentBinding {
    endpoint: String,
    bearer_file: PathBuf,
    probe_program: String,
}

/// The search sidecar a session with the web scopes pays through.
#[derive(Clone, Debug, Eq, PartialEq)]
struct WebBinding {
    endpoint: String,
    network: String,
    sequencer_public_key: [u8; 32],
    timeout: Duration,
    pending_attempts: u8,
    approval_threshold: u128,
}

/// The payer and approval registry the host attaches for web spends; the binding document
/// never carries key material.
#[derive(Clone)]
pub struct WebAuthority {
    approvals: Arc<ApprovalRegistry>,
    payer: Arc<Mutex<Box<dyn WebPayer + Send>>>,
}

impl WebAuthority {
    /// Pairs the registry approvers decide web holds in with the payer that signs web spends.
    #[must_use]
    pub fn new(approvals: Arc<ApprovalRegistry>, payer: Box<dyn WebPayer + Send>) -> Self {
        Self {
            approvals,
            payer: Arc::new(Mutex::new(payer)),
        }
    }
}

impl fmt::Debug for WebAuthority {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("WebAuthority(<attached>)")
    }
}

impl PartialEq for WebAuthority {
    fn eq(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.approvals, &other.approvals) && Arc::ptr_eq(&self.payer, &other.payer)
    }
}

impl Eq for WebAuthority {}

struct SharedPayer(Arc<Mutex<Box<dyn WebPayer + Send>>>);

impl WebPayer for SharedPayer {
    fn grant(&mut self, terms: &GrantTerms) -> Result<Vec<u8>, WebPayerError> {
        self.0
            .lock()
            .map_err(|_| WebPayerError::Unavailable)?
            .grant(terms)
    }

    fn pay(&mut self, terms: &ExactTerms) -> Result<Vec<u8>, WebPayerError> {
        self.0
            .lock()
            .map_err(|_| WebPayerError::Unavailable)?
            .pay(terms)
    }
}

/// The daemon-backed boundary one opened session executes against: verified program reads,
/// and the web route in front of them when the session carries a web scope.
pub enum DaemonBoundary {
    Reads(ProgramReads),
    Web(Box<WebBoundary<ProgramReads>>),
}

impl ToolBoundary for DaemonBoundary {
    fn execute(
        &mut self,
        tool: ToolDefinition,
        arguments: &Value,
    ) -> Result<Value, BoundaryRefusal> {
        match self {
            Self::Reads(reads) => reads.execute(tool, arguments),
            Self::Web(web) => web.execute(tool, arguments),
        }
    }

    fn observed_sequence(&mut self) -> Result<u64, BoundaryRefusal> {
        match self {
            Self::Reads(reads) => reads.observed_sequence(),
            Self::Web(web) => web.observed_sequence(),
        }
    }
}

/// One complete, validated binding document.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Binding {
    mode: DeploymentMode,
    tenant: String,
    store: PathBuf,
    audit_root: PathBuf,
    session_id: [u8; 32],
    session_token_file: PathBuf,
    session_generation: u64,
    capability_id: [u8; 32],
    core_sequence: u64,
    deadline: Duration,
    agent: AgentBinding,
    limit: LimitConfig,
    listener: Option<ListenerConfig>,
    web: Option<WebBinding>,
    web_authority: Option<WebAuthority>,
}

impl Binding {
    /// Reads and validates one binding document from an operator-protected file.
    ///
    /// # Errors
    ///
    /// Refuses a relative path, an unreadable or oversized document, and any document that is
    /// not a complete, closed binding.
    pub fn open(path: &Path) -> Result<Self, BindingError> {
        if !path.is_absolute() {
            return Err(BindingError::Unreadable(
                "the binding path must be absolute".to_owned(),
            ));
        }
        let metadata = fs::symlink_metadata(path)
            .map_err(|error| BindingError::Unreadable(error.kind().to_string()))?;
        if !metadata.is_file() {
            return Err(BindingError::Unreadable(
                "the binding path is not a regular file".to_owned(),
            ));
        }
        let length = usize::try_from(metadata.len()).unwrap_or(usize::MAX);
        if length > MAX_DOCUMENT_BYTES {
            return Err(BindingError::Unreadable(format!(
                "the binding document exceeds {MAX_DOCUMENT_BYTES} bytes"
            )));
        }
        let document = fs::read_to_string(path)
            .map_err(|error| BindingError::Unreadable(error.kind().to_string()))?;
        Self::parse(&document)
    }

    /// Validates one binding document.
    ///
    /// # Errors
    ///
    /// Refuses a document that is not a JSON object, carries an unaccepted field, omits a
    /// required field, or carries a value outside its declared shape.
    pub fn parse(document: &str) -> Result<Self, BindingError> {
        if document.len() > MAX_DOCUMENT_BYTES {
            return Err(malformed(format!(
                "the binding document exceeds {MAX_DOCUMENT_BYTES} bytes"
            )));
        }
        let value: Value = serde_json::from_str(document)
            .map_err(|error| malformed(format!("it is not valid JSON: {error}")))?;
        let root = value
            .as_object()
            .ok_or_else(|| malformed("the binding document is not a JSON object"))?;
        let mut accepted = BINDING_KEYS.to_vec();
        accepted.push("listener");
        accepted.push("web");
        closed(root, &accepted, "binding")?;
        let mode = match text(root, "mode")? {
            "full" => DeploymentMode::Full,
            "read-only" => DeploymentMode::ReadOnly,
            _ => return Err(malformed("field mode must be full or read-only")),
        };
        let deadline_ms = unsigned(root, "deadline_ms")?;
        if deadline_ms == 0 {
            return Err(malformed("field deadline_ms must be positive"));
        }
        let agent = object(root, "agent")?;
        closed(agent, &AGENT_KEYS, "agent")?;
        let limits = object(root, "limit")?;
        closed(limits, &LIMIT_KEYS, "limit")?;
        let scope_id = digest::<32>(limits, "scope_id")?;
        let scope = match text(limits, "scope")? {
            "tenant" => LimitScope::Tenant(scope_id),
            "agent" => LimitScope::Agent(scope_id),
            "session" => LimitScope::Session(scope_id),
            "capability" => LimitScope::Capability(scope_id),
            "counterparty" => LimitScope::Counterparty(scope_id),
            _ => return Err(malformed("field limit.scope names no known limit scope")),
        };
        let listener = match root.get("listener") {
            Some(declared) => Some(listener_config(declared, deadline_ms)?),
            None => None,
        };
        let web = match root.get("web") {
            Some(declared) => Some(web_binding(declared)?),
            None => None,
        };
        Ok(Self {
            mode,
            tenant: text(root, "tenant")?.to_owned(),
            store: absolute(root, "store")?,
            audit_root: absolute(root, "audit_root")?,
            session_id: digest::<32>(root, "session_id")?,
            session_token_file: absolute(root, "session_token_file")?,
            session_generation: unsigned(root, "session_generation")?,
            capability_id: digest::<32>(root, "capability_id")?,
            core_sequence: unsigned(root, "core_sequence")?,
            deadline: Duration::from_millis(deadline_ms),
            agent: AgentBinding {
                endpoint: text(agent, "endpoint")?.to_owned(),
                bearer_file: absolute(agent, "bearer_file")?,
                probe_program: text(agent, "probe_program")?.to_owned(),
            },
            limit: LimitConfig {
                id: LimitId(digest::<16>(limits, "id")?),
                name: text(limits, "name")?.to_owned(),
                scope,
                ceiling: wide(limits, "ceiling")?,
                consumed: wide(limits, "consumed")?,
            },
            listener,
            web,
            web_authority: None,
        })
    }

    /// Returns the deployment mode this binding declares.
    #[must_use]
    pub const fn mode(&self) -> DeploymentMode {
        self.mode
    }

    /// Returns the transport deadline this binding declares.
    #[must_use]
    pub const fn deadline(&self) -> Duration {
        self.deadline
    }

    /// Returns the tenant whose daemon records this binding serves.
    #[must_use]
    pub fn tenant(&self) -> &str {
        &self.tenant
    }

    /// Returns the agent store this binding opens.
    #[must_use]
    pub fn store(&self) -> &Path {
        &self.store
    }

    /// Returns the revocation generation the bound session token was minted under.
    #[must_use]
    pub const fn session_generation(&self) -> u64 {
        self.session_generation
    }

    /// Returns the loopback endpoint of the agent daemon that authorizes every tool call.
    #[must_use]
    pub fn agent_endpoint(&self) -> &str {
        &self.agent.endpoint
    }

    /// Narrows a full binding to its read-only surface. It never widens a read-only binding.
    pub fn restrict_to_read_only(&mut self) {
        self.mode = DeploymentMode::ReadOnly;
    }

    /// Returns the listener configuration a socket deployment declares, if it declares one.
    #[must_use]
    pub const fn listener(&self) -> Option<&ListenerConfig> {
        self.listener.as_ref()
    }

    /// Attaches the payer and approval registry a session with the web scopes spends through.
    pub fn attach_web(&mut self, authority: WebAuthority) {
        self.web_authority = Some(authority);
    }

    /// Opens the daemon-bound protocol session this binding describes.
    ///
    /// The bearer and session token are read from their operator-protected files only here,
    /// and no signing seed is read on this path. A full session that carries a web scope is
    /// served the web tools through the declared sidecar and the attached web authority.
    ///
    /// # Errors
    ///
    /// Refuses an unavailable store, an unrestorable session registry, an invalid limit set,
    /// unreadable protected secrets, refused daemon authority, an invalid agent surface, and a
    /// web scope without the web section or without an attached web authority.
    pub fn open_session(&self) -> Result<Session<DaemonBoundary>, BindingError> {
        let tenant = TenantId::new(self.tenant.clone())
            .map_err(|error| malformed(format!("field tenant is invalid: {error}")))?;
        let store = Store::open(&self.store).map_err(|error| {
            BindingError::Refused(format!("the agent store is unavailable: {error}"))
        })?;
        let mut sessions = SessionRegistry::default();
        sessions.restore_tenant(&store, &tenant).map_err(|error| {
            BindingError::Refused(format!("the agent sessions are unrestorable: {error:?}"))
        })?;
        let limiter = BudgetLimiter::new(vec![self.limit.clone()]).map_err(|error| {
            BindingError::Refused(format!("the configured limit is invalid: {error:?}"))
        })?;
        let control = SessionControl::new(
            Arc::new(Mutex::new(store)),
            sessions,
            Arc::new(PreparationLifecycle::default()),
            Arc::new(limiter),
        );
        let token = self.session_token()?;
        let credential = SessionCredential::new(
            tenant,
            SessionId(self.session_id),
            *token,
            self.session_generation,
        );
        let capability = CapabilityId(self.capability_id);
        let (bound, route) = match self.mode {
            DeploymentMode::Full => {
                let server = Server::bind(
                    control,
                    credential,
                    capability,
                    self.core_sequence,
                    &self.audit_root,
                )
                .map_err(|error| {
                    BindingError::Refused(format!("the daemon binding was refused: {error:?}"))
                })?;
                let route = if server.serves_web() {
                    Some(self.web_route(&server)?)
                } else {
                    None
                };
                (Bound::Full(Box::new(server)), route)
            }
            DeploymentMode::ReadOnly => ReadOnly::bind(
                control,
                credential,
                capability,
                self.core_sequence,
                &self.audit_root,
            )
            .map(|server| (Bound::ReadOnly(Box::new(server)), None))
            .map_err(|error| {
                BindingError::Refused(format!(
                    "the read-only daemon binding was refused: {error:?}"
                ))
            })?,
        };
        let surface = AgentSurface::new(
            &self.agent.endpoint,
            self.agent_bearer()?,
            &self.agent.probe_program,
            self.deadline,
        )
        .map_err(|refusal| {
            BindingError::Refused(format!(
                "the agent daemon surface is invalid: {}",
                refusal.detail()
            ))
        })?;
        let reads = ProgramReads::new(surface);
        let boundary = match (&bound, route) {
            (Bound::Full(server), Some(route)) => {
                DaemonBoundary::Web(Box::new(server.route_web(reads, route).map_err(
                    |error| BindingError::Refused(format!("the web route was refused: {error:?}")),
                )?))
            }
            _ => DaemonBoundary::Reads(reads),
        };
        Ok(Session::new(bound, boundary))
    }

    fn web_route(&self, server: &Server) -> Result<WebRoute, BindingError> {
        let web = self.web.as_ref().ok_or_else(|| {
            BindingError::Refused(
                "field web is absent while the bound session carries a web scope".to_owned(),
            )
        })?;
        let authority = self.web_authority.as_ref().ok_or_else(|| {
            BindingError::Refused(
                "no web payer and approval registry are attached for the web scope".to_owned(),
            )
        })?;
        let payer_did = std::str::from_utf8(server.binding().agent().as_bytes())
            .map_err(|_| BindingError::Refused("the bound agent DID is not UTF-8".to_owned()))?;
        let config = WebConfig::new(
            &web.endpoint,
            payer_did,
            &web.network,
            web.sequencer_public_key,
            web.timeout,
            web.pending_attempts,
        )
        .map_err(|error| match error {
            WebToolError::Configuration(field) => {
                malformed(format!("field web.{field} is invalid"))
            }
            other => malformed(format!("field web is invalid: {other:?}")),
        })?;
        Ok(WebRoute::new(
            config,
            Arc::clone(&authority.approvals),
            ApprovalPolicy {
                amount_threshold: web.approval_threshold,
            },
            Box::new(SharedPayer(Arc::clone(&authority.payer))),
        ))
    }

    fn session_token(&self) -> Result<Zeroizing<[u8; 32]>, BindingError> {
        let encoded = protected_text(&self.session_token_file, "session_token_file")?;
        let mut bytes = Zeroizing::new([0_u8; 32]);
        if encoded.len() != 64 {
            return Err(malformed(
                "field session_token_file does not hold 32 hexadecimal bytes",
            ));
        }
        for (index, byte) in bytes.iter_mut().enumerate() {
            let start = index.checked_mul(2).ok_or_else(|| malformed("overflow"))?;
            let end = start.checked_add(2).ok_or_else(|| malformed("overflow"))?;
            let pair = encoded
                .get(start..end)
                .ok_or_else(|| malformed("field session_token_file is truncated"))?;
            *byte = u8::from_str_radix(pair, 16)
                .map_err(|_| malformed("field session_token_file is not hexadecimal"))?;
        }
        Ok(bytes)
    }

    fn agent_bearer(&self) -> Result<String, BindingError> {
        let bearer = protected_text(&self.agent.bearer_file, "agent.bearer_file")?;
        Ok(bearer.as_str().to_owned())
    }
}

fn protected_text(path: &Path, field: &str) -> Result<Zeroizing<String>, BindingError> {
    let bytes = read_protected_source(path, MAX_SECRET_BYTES).map_err(|error| {
        BindingError::Unreadable(format!("field {field} is unusable: {error:?}"))
    })?;
    let decoded = Zeroizing::new(
        String::from_utf8(bytes)
            .map_err(|_| malformed(format!("field {field} does not hold UTF-8")))?,
    );
    let trimmed = Zeroizing::new(decoded.trim().to_owned());
    if trimmed.is_empty() {
        return Err(malformed(format!("field {field} names an empty secret")));
    }
    Ok(trimmed)
}

fn listener_config(declared: &Value, deadline_ms: u64) -> Result<ListenerConfig, BindingError> {
    let listener = declared
        .as_object()
        .ok_or_else(|| malformed("field listener must be an object"))?;
    closed(listener, &LISTENER_KEYS, "listener")?;
    let mode = u32::from_str_radix(text(listener, "mode")?, 8)
        .map_err(|_| malformed("field listener.mode must be an octal mode"))?;
    let admitted = listener
        .get("admitted_uids")
        .and_then(Value::as_array)
        .ok_or_else(|| malformed("field listener.admitted_uids must be an array"))?;
    if admitted.is_empty() || admitted.len() > MAX_ADMITTED_PEERS {
        return Err(malformed(format!(
            "field listener.admitted_uids must name 1 to {MAX_ADMITTED_PEERS} peers"
        )));
    }
    let mut admitted_uids = Vec::with_capacity(admitted.len());
    for entry in admitted {
        let uid = entry
            .as_u64()
            .and_then(|value| u32::try_from(value).ok())
            .ok_or_else(|| malformed("field listener.admitted_uids holds a non-uid entry"))?;
        if !admitted_uids.contains(&uid) {
            admitted_uids.push(uid);
        }
    }
    Ok(ListenerConfig {
        endpoint: absolute(listener, "socket")?,
        owner_uid: unsigned32(listener, "owner_uid")?,
        owner_gid: unsigned32(listener, "owner_gid")?,
        mode,
        admitted_uids,
        deadline: Duration::from_millis(deadline_ms),
    })
}

fn web_binding(declared: &Value) -> Result<WebBinding, BindingError> {
    let web = declared
        .as_object()
        .ok_or_else(|| malformed("field web must be an object"))?;
    closed(web, &WEB_KEYS, "web")?;
    let pending_attempts = u8::try_from(unsigned(web, "pending_attempts")?)
        .map_err(|_| malformed("field web.pending_attempts is outside its unsigned range"))?;
    Ok(WebBinding {
        endpoint: text(web, "endpoint")?.to_owned(),
        network: text(web, "network")?.to_owned(),
        sequencer_public_key: digest::<32>(web, "sequencer_public_key")?,
        timeout: Duration::from_millis(unsigned(web, "timeout_ms")?),
        pending_attempts,
        approval_threshold: wide(web, "approval_threshold")?,
    })
}
