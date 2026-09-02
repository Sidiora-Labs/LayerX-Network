//! One-tenant, one-scope-set MCP server with daemon-only routing.

use std::collections::BTreeSet;
use std::fmt;
use std::path::Path;

use layerx_agentd::audit::{AuditError, Log};
use layerx_agentd::capability::{Capability, CapabilityError, CapabilityId};
use layerx_agentd::identity::ProtocolAuthority;
use layerx_agentd::session::{SessionCredential, SessionError, SessionId, SessionRecord};
use layerx_agentd::session_control::{OperationPermit, SessionControl, SessionControlError};
use layerx_agentd::store::TenantId;
use layerx_agentd::tenant::{AuthorizationError, ObjectOwner, Operation, Surface};
use layerx_types::ids::Did;
use sha2::{Digest, Sha256};

pub use crate::readonly::ReadOnly;

const MAX_ARGUMENT_BYTES: usize = 1_048_576;
const MAX_TOOL_NAME_BYTES: usize = 128;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ToolKind {
    Read,
    Write,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DeploymentMode {
    Full,
    ReadOnly,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct CapabilityDeclaration {
    pub mode: DeploymentMode,
    pub read_tools: usize,
    pub write_tools: usize,
    pub mutations_reachable: bool,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ToolDefinition {
    pub name: &'static str,
    pub kind: ToolKind,
    pub required_scope: &'static str,
    pub mutation: &'static str,
    pub evidence: &'static str,
}

const TOOL_CATALOGUE: [ToolDefinition; 11] = [
    ToolDefinition {
        name: "balance.get",
        kind: ToolKind::Read,
        required_scope: "read:balance",
        mutation: "none",
        evidence: "core bytes, verification level, freshness",
    },
    ToolDefinition {
        name: "history.list",
        kind: ToolKind::Read,
        required_scope: "read:history",
        mutation: "none",
        evidence: "core records, verification level, stable cursor, freshness",
    },
    ToolDefinition {
        name: "receipt.get",
        kind: ToolKind::Read,
        required_scope: "read:receipt",
        mutation: "none",
        evidence: "core receipt bytes and verification level",
    },
    ToolDefinition {
        name: "checkpoint.get",
        kind: ToolKind::Read,
        required_scope: "read:checkpoint",
        mutation: "none",
        evidence: "checkpoint certificate and verification level",
    },
    ToolDefinition {
        name: "proof.get",
        kind: ToolKind::Read,
        required_scope: "read:proof",
        mutation: "none",
        evidence: "proof bundle and verification level",
    },
    ToolDefinition {
        name: "availability.get",
        kind: ToolKind::Read,
        required_scope: "read:availability",
        mutation: "none",
        evidence: "verified chunks and attributed availability failures",
    },
    ToolDefinition {
        name: "activity.prepare",
        kind: ToolKind::Write,
        required_scope: "write:prepare",
        mutation: "daemon-local preparation only",
        evidence: "canonical bytes and bound disclosure",
    },
    ToolDefinition {
        name: "activity.disclose",
        kind: ToolKind::Write,
        required_scope: "write:disclose",
        mutation: "none",
        evidence: "disclosure decoded from canonical bytes",
    },
    ToolDefinition {
        name: "activity.sign",
        kind: ToolKind::Write,
        required_scope: "write:sign",
        mutation: "daemon-local signed preparation",
        evidence: "signature binding evidence",
    },
    ToolDefinition {
        name: "activity.submit",
        kind: ToolKind::Write,
        required_scope: "write:submit",
        mutation: "core submission through the ordinary daemon path",
        evidence: "verified receipt or honest non-terminal state",
    },
    ToolDefinition {
        name: "activity.track",
        kind: ToolKind::Write,
        required_scope: "write:track",
        mutation: "daemon-local receipt resolution state",
        evidence: "verified receipt or honest non-terminal state",
    },
];

#[must_use]
pub const fn catalogue() -> &'static [ToolDefinition] {
    &TOOL_CATALOGUE
}

/// Authority fixed for the lifetime of one server instance.
#[derive(Clone, Eq, PartialEq)]
pub struct ScopeBinding {
    tenant: TenantId,
    agent: Did,
    session_id: SessionId,
    capability_id: CapabilityId,
    scopes: BTreeSet<String>,
    generation: u64,
}

impl fmt::Debug for ScopeBinding {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("ScopeBinding")
            .field("tenant", &self.tenant)
            .field("agent", &self.agent)
            .field("session_id", &self.session_id)
            .field("capability_id", &self.capability_id)
            .field("scopes", &self.scopes)
            .field("generation", &self.generation)
            .field("credential", &"[REDACTED]")
            .finish()
    }
}

impl ScopeBinding {
    fn derive(
        session: &SessionRecord,
        capability: &Capability,
        core_sequence: u64,
    ) -> Result<Self, ServerError> {
        if !session.open {
            return Err(ServerError::ClosedSession);
        }
        if session.request.tenant != capability.tenant {
            return Err(ServerError::TenantMismatch);
        }
        if session.request.authority != ProtocolAuthority::CapabilityGrant(capability.id.0) {
            return Err(ServerError::CapabilityMismatch);
        }
        if core_sequence >= session.request.expiry_sequence
            || core_sequence >= capability.dimensions.expiry_sequence
        {
            return Err(ServerError::ExpiredAuthority);
        }
        if session.request.scopes.is_empty() {
            return Err(ServerError::NoScope);
        }
        let permitted = session
            .request
            .permitted_activity_types
            .iter()
            .all(|activity| capability.dimensions.activity_types.contains(activity));
        if !permitted {
            return Err(ServerError::CapabilityMismatch);
        }
        let scopes = session
            .request
            .scopes
            .iter()
            .filter(|scope| {
                TOOL_CATALOGUE
                    .iter()
                    .any(|tool| tool.required_scope == scope.as_str())
            })
            .cloned()
            .collect::<BTreeSet<_>>();
        if scopes.is_empty() {
            return Err(ServerError::NoScope);
        }
        Ok(Self {
            tenant: session.request.tenant.clone(),
            agent: session.request.agent.clone(),
            session_id: session.request.session_id,
            capability_id: capability.id,
            scopes,
            generation: session.generation,
        })
    }

    #[must_use]
    pub const fn tenant(&self) -> &TenantId {
        &self.tenant
    }

    #[must_use]
    pub const fn session_id(&self) -> SessionId {
        self.session_id
    }

    #[must_use]
    pub const fn capability_id(&self) -> CapabilityId {
        self.capability_id
    }

    #[must_use]
    pub const fn scopes(&self) -> &BTreeSet<String> {
        &self.scopes
    }

    /// Returns the session revocation generation this binding was derived under.
    #[must_use]
    pub const fn generation(&self) -> u64 {
        self.generation
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DaemonGate {
    Policy,
    Capability,
    Budget,
    RateLimit,
    Audit,
}

pub const REQUIRED_DAEMON_GATES: [DaemonGate; 5] = [
    DaemonGate::Policy,
    DaemonGate::Capability,
    DaemonGate::Budget,
    DaemonGate::RateLimit,
    DaemonGate::Audit,
];

/// The only request produced by the MCP server. A daemon must consume it.
#[derive(Clone, Eq, PartialEq)]
pub struct DaemonInvocation {
    server_binding: [u8; 32],
    invocation_id: u64,
    tool: ToolDefinition,
    arguments: Vec<u8>,
    arguments_digest: [u8; 32],
    gates: [DaemonGate; 5],
}

impl fmt::Debug for DaemonInvocation {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("DaemonInvocation")
            .field("server_binding", &"[REDACTED]")
            .field("invocation_id", &self.invocation_id)
            .field("tool", &self.tool)
            .field("arguments", &"[REDACTED]")
            .field("arguments_digest", &self.arguments_digest)
            .field("gates", &self.gates)
            .finish()
    }
}

impl DaemonInvocation {
    #[must_use]
    pub const fn invocation_id(&self) -> u64 {
        self.invocation_id
    }

    #[must_use]
    pub const fn tool(&self) -> ToolDefinition {
        self.tool
    }

    #[must_use]
    pub fn arguments(&self) -> &[u8] {
        &self.arguments
    }

    #[must_use]
    pub const fn arguments_digest(&self) -> [u8; 32] {
        self.arguments_digest
    }

    #[must_use]
    pub const fn gates(&self) -> [DaemonGate; 5] {
        self.gates
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum InvocationOutcome {
    Completed,
    Refused,
    Unknown,
    Failed,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct InvocationRecord {
    pub invocation_id: u64,
    pub arguments_digest: [u8; 32],
    pub outcome: InvocationOutcome,
}

/// A server has no boundary handle: it can only create daemon invocations.
pub struct Server {
    control: SessionControl,
    credential: SessionCredential,
    binding: ScopeBinding,
    binding_digest: [u8; 32],
    tools: Vec<ToolDefinition>,
    audit: Log,
    next_invocation_id: u64,
    mode: DeploymentMode,
}

impl Server {
    /// Binds one exact bearer credential to the shared daemon session authority and its
    /// persisted capability. The credential is consumed so the server does not create an
    /// unnecessary bearer copy.
    ///
    /// # Errors
    ///
    /// Refuses missing, closed, expired, cross-tenant, unlinked, or scope-empty authority.
    pub fn bind(
        control: SessionControl,
        credential: SessionCredential,
        capability_id: CapabilityId,
        core_sequence: u64,
        audit_root: impl AsRef<Path>,
    ) -> Result<Self, ServerError> {
        Self::bind_for_mode(
            control,
            credential,
            capability_id,
            core_sequence,
            audit_root,
            DeploymentMode::Full,
        )
    }

    pub(crate) fn bind_for_mode(
        control: SessionControl,
        credential: SessionCredential,
        capability_id: CapabilityId,
        core_sequence: u64,
        audit_root: impl AsRef<Path>,
        mode: DeploymentMode,
    ) -> Result<Self, ServerError> {
        let registry_handle = control.registry();
        let registry = registry_handle
            .read()
            .map_err(|_| ServerError::AuthorizationUnavailable)?;
        let _token = registry.authenticate(&credential).map_err(map_session)?;
        let session = registry
            .get(credential.tenant(), credential.session_id())
            .ok_or(ServerError::MissingSession)?;
        let store_handle = control.store();
        let store = store_handle
            .lock()
            .map_err(|_| ServerError::AuthorizationUnavailable)?;
        let capability = Capability::restore(&store, session.request.tenant.clone(), capability_id)
            .map_err(ServerError::Capability)?
            .ok_or(ServerError::MissingCapability)?;
        let binding = ScopeBinding::derive(session, &capability, core_sequence)?;
        let tools = TOOL_CATALOGUE
            .iter()
            .filter(|tool| {
                binding.scopes.contains(tool.required_scope)
                    && (mode == DeploymentMode::Full || tool.kind == ToolKind::Read)
            })
            .copied()
            .collect::<Vec<_>>();
        if tools.is_empty() {
            return Err(ServerError::NoScope);
        }
        let binding_digest = binding_digest(&binding, mode);
        let audit = Log::open(audit_root, binding.tenant()).map_err(ServerError::Audit)?;
        drop(store);
        drop(registry);
        Ok(Self {
            control,
            credential,
            binding,
            binding_digest,
            tools,
            audit,
            next_invocation_id: 0,
            mode,
        })
    }

    #[must_use]
    pub const fn binding(&self) -> &ScopeBinding {
        &self.binding
    }

    #[must_use]
    pub fn tools(&self) -> &[ToolDefinition] {
        &self.tools
    }

    #[must_use]
    pub fn capability_declaration(&self) -> CapabilityDeclaration {
        let read_tools = self
            .tools
            .iter()
            .filter(|tool| tool.kind == ToolKind::Read)
            .count();
        let write_tools = self.tools.len() - read_tools;
        CapabilityDeclaration {
            mode: self.mode,
            read_tools,
            write_tools,
            mutations_reachable: write_tools != 0,
        }
    }

    #[must_use]
    pub fn tool(&self, name: &str) -> Option<ToolDefinition> {
        self.tools.iter().find(|tool| tool.name == name).copied()
    }

    /// Constructs an invocation only after the common daemon resolver authorizes the exact
    /// credential and arms its exact-generation stop signal.
    fn route(
        &mut self,
        core_sequence: u64,
        name: &str,
        arguments: Vec<u8>,
    ) -> Result<(DaemonInvocation, OperationPermit), ServerError> {
        if name.is_empty()
            || name.len() > MAX_TOOL_NAME_BYTES
            || name.as_bytes().contains(&0)
            || arguments.len() > MAX_ARGUMENT_BYTES
        {
            return Err(ServerError::InvalidInvocation);
        }
        let arguments_digest: [u8; 32] = Sha256::digest(&arguments).into();
        let tool = self.tool(name);
        let authorization = tool.map(|definition| self.authorize_tool(core_sequence, definition));
        let decision = match &authorization {
            Some(Ok(_)) => 1,
            None => 2,
            Some(Err(_)) => 3,
        };
        let attempt = invocation_audit(
            self.binding_digest,
            self.next_invocation_id,
            name,
            arguments_digest,
            decision,
            0,
        )?;
        self.audit
            .before_operation(&attempt, || ())
            .map_err(ServerError::Audit)?;
        let tool = tool.ok_or(ServerError::ToolAbsent)?;
        let permit = authorization.ok_or(ServerError::ToolAbsent)??;
        let invocation_id = self.next_invocation_id;
        self.next_invocation_id = self
            .next_invocation_id
            .checked_add(1)
            .ok_or(ServerError::Arithmetic)?;
        Ok((
            DaemonInvocation {
                server_binding: self.binding_digest,
                invocation_id,
                tool,
                arguments,
                arguments_digest,
                gates: REQUIRED_DAEMON_GATES,
            },
            permit,
        ))
    }

    /// Records the daemon's typed result and consumes the pending invocation.
    ///
    /// # Errors
    ///
    /// Refuses an invocation from a different server and fails on audit persistence errors.
    fn complete(
        &mut self,
        invocation: &DaemonInvocation,
        outcome: InvocationOutcome,
    ) -> Result<InvocationRecord, ServerError> {
        if invocation.server_binding != self.binding_digest {
            return Err(ServerError::WrongServer);
        }
        let completion = invocation_audit(
            self.binding_digest,
            invocation.invocation_id,
            invocation.tool.name,
            invocation.arguments_digest,
            1,
            outcome_code(outcome),
        )?;
        self.audit
            .before_operation(&completion, || ())
            .map_err(ServerError::Audit)?;
        Ok(InvocationRecord {
            invocation_id: invocation.invocation_id,
            arguments_digest: invocation.arguments_digest,
            outcome,
        })
    }

    /// Runs a non-mutating tool and reauthorizes at the result-release boundary.
    pub fn execute_read<T, F>(
        &mut self,
        core_sequence: u64,
        name: &str,
        arguments: Vec<u8>,
        executor: F,
    ) -> Result<T, ServerError>
    where
        F: FnOnce(&DaemonInvocation) -> (T, InvocationOutcome),
    {
        if self
            .tool(name)
            .is_some_and(|tool| tool.kind != ToolKind::Read || tool.mutation != "none")
        {
            return Err(ServerError::ToolAbsent);
        }
        let (invocation, permit) = self.route(core_sequence, name, arguments)?;
        let (result, outcome) = executor(&invocation);
        permit.boundary(&self.control).map_err(map_control)?;
        self.complete(&invocation, outcome)?;
        Ok(result)
    }

    /// Runs a write tool only while the shared daemon session read permit linearizes its actual
    /// externally visible effect against close, revocation, and scope restriction.
    pub fn execute_committed<T, F>(
        &mut self,
        core_sequence: u64,
        name: &str,
        arguments: Vec<u8>,
        executor: F,
    ) -> Result<T, ServerError>
    where
        F: FnOnce(&DaemonInvocation) -> (T, InvocationOutcome),
    {
        if self
            .tool(name)
            .is_some_and(|tool| tool.kind != ToolKind::Write)
        {
            return Err(ServerError::ToolAbsent);
        }
        let (invocation, permit) = self.route(core_sequence, name, arguments)?;
        let control = self.control.clone();
        let (result, outcome) = permit
            .commit(&control, || Ok(executor(&invocation)))
            .map_err(map_control)?;
        self.complete(&invocation, outcome)?;
        Ok(result)
    }

    #[must_use]
    pub const fn audit_entries(&self) -> u64 {
        self.audit.entries()
    }

    fn authorize_tool(
        &self,
        core_sequence: u64,
        tool: ToolDefinition,
    ) -> Result<OperationPermit, ServerError> {
        let operation = tool_operation(tool.name).ok_or(ServerError::InvalidInvocation)?;
        self.control
            .authorize(
                &self.credential,
                operation,
                Surface::Mcp,
                core_sequence,
                Some(ObjectOwner {
                    tenant: self.binding.tenant.clone(),
                    agent: Some(self.binding.agent.clone()),
                }),
            )
            .map_err(map_control)
    }
}

fn tool_operation(name: &str) -> Option<Operation> {
    match name {
        "balance.get" => Some(Operation::ReadBalance),
        "history.list" => Some(Operation::ReadHistory),
        "receipt.get" => Some(Operation::ProgramReceipt),
        "checkpoint.get" => Some(Operation::ReadCheckpoint),
        "proof.get" => Some(Operation::ReadProofBundle),
        "availability.get" => Some(Operation::AvailabilityFetch),
        "activity.prepare" | "activity.disclose" => Some(Operation::Prepare),
        "activity.sign" => Some(Operation::Sign),
        "activity.submit" => Some(Operation::Submit),
        "activity.track" => Some(Operation::Track),
        _ => None,
    }
}

#[derive(Debug)]
pub enum ServerError {
    MissingSession,
    MissingCapability,
    ClosedSession,
    RevokedSession,
    TenantMismatch,
    CapabilityMismatch,
    ExpiredAuthority,
    NoScope,
    InvalidInvocation,
    ToolAbsent,
    WrongServer,
    AuthorizationUnavailable,
    Arithmetic,
    Capability(CapabilityError),
    Audit(AuditError),
}

fn binding_digest(binding: &ScopeBinding, mode: DeploymentMode) -> [u8; 32] {
    let mut hasher = Sha256::new();
    hasher.update(binding.tenant.as_str().as_bytes());
    hasher.update(binding.agent.as_bytes());
    hasher.update(binding.session_id.0);
    hasher.update(binding.generation.to_be_bytes());
    hasher.update(binding.capability_id.0);
    hasher.update([match mode {
        DeploymentMode::Full => 1,
        DeploymentMode::ReadOnly => 2,
    }]);
    for scope in &binding.scopes {
        hasher.update((scope.len() as u64).to_be_bytes());
        hasher.update(scope.as_bytes());
    }
    hasher.finalize().into()
}

const fn map_authorization(error: AuthorizationError) -> ServerError {
    match error {
        AuthorizationError::Revoked => ServerError::RevokedSession,
        AuthorizationError::Expired => ServerError::ExpiredAuthority,
        AuthorizationError::ScopeDenied => ServerError::ToolAbsent,
        AuthorizationError::NotAuthorized | AuthorizationError::InvalidRequest => {
            ServerError::CapabilityMismatch
        }
    }
}

fn map_session(error: SessionError) -> ServerError {
    match error {
        SessionError::Expired => ServerError::ExpiredAuthority,
        SessionError::ScopeDenied => ServerError::ToolAbsent,
        SessionError::Revoked | SessionError::NotFound | SessionError::AlreadyClosed => {
            ServerError::RevokedSession
        }
        SessionError::IdentityMismatch
        | SessionError::AuthorityMissing
        | SessionError::WrongPrincipal => ServerError::CapabilityMismatch,
        SessionError::MissingField(_)
        | SessionError::GenerationExhausted
        | SessionError::TokenReuse
        | SessionError::TokenHistoryExhausted
        | SessionError::Store(_) => ServerError::AuthorizationUnavailable,
    }
}

fn map_control(error: SessionControlError) -> ServerError {
    match error {
        SessionControlError::Authorization(error) => map_authorization(error),
        SessionControlError::Session(error) => map_session(error),
        SessionControlError::Lifecycle(_)
        | SessionControlError::Human(_)
        | SessionControlError::Unavailable => ServerError::AuthorizationUnavailable,
    }
}

fn invocation_audit(
    binding: [u8; 32],
    invocation_id: u64,
    name: &str,
    arguments_digest: [u8; 32],
    decision: u8,
    outcome: u8,
) -> Result<Vec<u8>, ServerError> {
    let name_length = u16::try_from(name.len()).map_err(|_| ServerError::InvalidInvocation)?;
    let mut payload = Vec::with_capacity(82 + name.len());
    payload.extend_from_slice(b"LXMI");
    payload.push(1);
    payload.extend_from_slice(&binding);
    payload.extend_from_slice(&invocation_id.to_be_bytes());
    payload.extend_from_slice(&name_length.to_be_bytes());
    payload.extend_from_slice(name.as_bytes());
    payload.extend_from_slice(&arguments_digest);
    payload.push(decision);
    payload.push(outcome);
    Ok(payload)
}

const fn outcome_code(outcome: InvocationOutcome) -> u8 {
    match outcome {
        InvocationOutcome::Completed => 1,
        InvocationOutcome::Refused => 2,
        InvocationOutcome::Unknown => 3,
        InvocationOutcome::Failed => 4,
    }
}
