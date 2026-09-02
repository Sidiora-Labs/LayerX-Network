use std::collections::BTreeMap;

use layerx_types::ids::Did;

use crate::session::{SessionError, SessionId, SessionRegistry, Token};
use crate::store::TenantId;

pub use layerx_agent_api::Operation;

pub mod delete;
#[path = "tenant/errors.rs"]
mod errors;

pub use delete::{
    record_legal_audit, DeletionError, DeletionReport, LegalAuditClass, LegalAuditRecord,
    LegalRetention,
};
#[path = "tenant/isolation.rs"]
mod isolation;

pub use errors::{
    BoundedMetricKey, BoundedMetrics, ErrorClass, InternalError, MetricKind, MetricLabel,
    NormalizedError, SanitizedTrace, TIMING_MITIGATION,
};
pub use isolation::{
    ChannelBinding, ChannelKind, Config, IsolationError, RedactionPolicy, Retention, SignerBinding,
    SignerMaterial, TenantIsolation,
};

macro_rules! enumerated {
    (@count) => { 0_usize };
    (@count $head:ident $($tail:ident)*) => { 1_usize + enumerated!(@count $($tail)*) };
    ($(#[$meta:meta])* $name:ident { $($variant:ident),+ $(,)? }) => {
        $(#[$meta])*
        pub enum $name {
            $($variant),+
        }

        impl $name {
            /// Every variant, in declaration order.
            pub const ALL: [Self; enumerated!(@count $($variant)+)] = [$(Self::$variant),+];
        }
    };
}

/// Produces the only public error, trace, and metric representation for an internal failure.
pub fn normalize_error(
    error: &InternalError,
    tenant: &TenantId,
    surface: Surface,
    metrics: &mut BoundedMetrics,
) -> NormalizedError {
    errors::normalize(error, tenant, surface, metrics)
}

/// Deletes one tenant atomically under an explicit legal-retention policy.
///
/// # Errors
///
/// Returns `InvalidDeletionId` for an all-zero identifier, `InvalidRetention` when
/// a named legal audit is absent or not local-only, and `CorruptLegalAudit` when a
/// retained record cannot be decoded; store failures propagate.
pub fn delete_tenant_data(
    store: &mut crate::store::Store,
    tenant: &TenantId,
    policy: &LegalRetention,
    current_sequence: u64,
    deletion_id: [u8; 16],
) -> Result<DeletionReport, DeletionError> {
    delete::delete_tenant(store, tenant, policy, current_sequence, deletion_id)
}

enumerated! {
    /// Every public surface that must use the same authenticated tenant resolution.
    #[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
    Surface {
        Contract,
        RustSdk,
        TypeScriptSdk,
        PythonSdk,
        Mcp,
        Subscription,
        Export,
    }
}

/// Every class of token-gated operation dispatched through [`resolve`].
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub enum OperationClass {
    Read,
    Subscribe,
    Prepare,
    Export,
    Approve,
    Write,
}

impl OperationClass {
    pub const ALL: [Self; 6] = [
        Self::Read,
        Self::Subscribe,
        Self::Prepare,
        Self::Export,
        Self::Approve,
        Self::Write,
    ];

    /// Classifies the generated Agent API operation inventory. Bootstrap operations that cannot
    /// carry a pre-existing token are explicitly excluded. This exhaustive match makes adding a
    /// schema operation a compile-time authorization decision.
    #[must_use]
    pub const fn for_operation(operation: Operation) -> Option<Self> {
        match operation {
            Operation::AgentRegister | Operation::SessionOpen => None,
            Operation::ApprovalApprove
            | Operation::ApprovalGet
            | Operation::ApprovalList
            | Operation::ApprovalReject => Some(Self::Approve),
            Operation::ExportOffline => Some(Self::Export),
            Operation::Prepare => Some(Self::Prepare),
            Operation::SubscriptionAcknowledge
            | Operation::SubscriptionCreate
            | Operation::SubscriptionDelete
            | Operation::SubscriptionHealth
            | Operation::SubscriptionList
            | Operation::SubscriptionPause
            | Operation::SubscriptionResume => Some(Self::Subscribe),
            Operation::AvailabilityFetch
            | Operation::BudgetList
            | Operation::BudgetReconciliation
            | Operation::CapabilityList
            | Operation::ProgramActivity
            | Operation::ProgramDiscover
            | Operation::ProgramInterface
            | Operation::ProgramReceipt
            | Operation::ProgramSimulate
            | Operation::Project
            | Operation::ReadAccount
            | Operation::ReadBalance
            | Operation::ReadBatch
            | Operation::ReadCheckpoint
            | Operation::ReadHistory
            | Operation::ReadModuleState
            | Operation::ReadProofBundle
            | Operation::SessionList => Some(Self::Read),
            Operation::BudgetCreate
            | Operation::BudgetFund
            | Operation::BudgetRevoke
            | Operation::CapabilityAttenuate
            | Operation::CapabilityCreate
            | Operation::CapabilityRevoke
            | Operation::ProgramCall
            | Operation::SessionClose
            | Operation::SessionRefresh
            | Operation::Sign
            | Operation::Submit
            | Operation::Track
            | Operation::Wait => Some(Self::Write),
        }
    }

    /// Returns the server-owned scope set for an operation. Broad class scopes are canonical;
    /// established MCP tool scopes remain explicit aliases and cannot be selected by a request.
    #[must_use]
    pub const fn authorized_scopes(operation: Operation) -> &'static [&'static str] {
        match operation {
            Operation::ReadBalance => &["read", "read:balance"],
            Operation::ReadHistory => &["read", "read:history"],
            Operation::ProgramDiscover
            | Operation::ProgramInterface
            | Operation::ProgramActivity => &["read", "program:read"],
            Operation::ProgramReceipt => &["read", "program:read", "read:receipt"],
            Operation::ProgramSimulate => &["read", "program:simulate"],
            Operation::ProgramCall => &["write", "program:call"],
            Operation::ReadCheckpoint => &["read", "read:checkpoint"],
            Operation::ReadProofBundle => &["read", "read:proof"],
            Operation::AvailabilityFetch => &["read", "read:availability"],
            Operation::Prepare => &["prepare", "write:prepare", "write:disclose"],
            Operation::Sign => &["write", "write:sign"],
            Operation::Submit => &["write", "write:submit"],
            Operation::Track => &["write", "write:track"],
            operation => match Self::for_operation(operation) {
                Some(Self::Read) => &["read"],
                Some(Self::Subscribe) => &["subscribe"],
                Some(Self::Prepare) => &["prepare"],
                Some(Self::Export) => &["export"],
                Some(Self::Approve) => &["approve"],
                Some(Self::Write) => &["write"],
                None => &[],
            },
        }
    }

    /// Returns the scope a token must carry for this operation class.
    #[must_use]
    pub const fn scope(self) -> &'static str {
        match self {
            Self::Read => "read",
            Self::Subscribe => "subscribe",
            Self::Prepare => "prepare",
            Self::Export => "export",
            Self::Approve => "approve",
            Self::Write => "write",
        }
    }
}

/// Untrusted request metadata and the trusted owner loaded for its target.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RequestContext {
    pub surface: Surface,
    pub operation: Operation,
    pub core_sequence: u64,
    pub supplied_header_tenant: Option<TenantId>,
    pub supplied_body_tenant: Option<TenantId>,
    pub target_owner: Option<ObjectOwner>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ObjectOwner {
    pub tenant: TenantId,
    pub agent: Option<Did>,
}

/// Principal claims copied only from an authenticated daemon token.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ResolvedPrincipal {
    pub tenant: TenantId,
    pub agent: Did,
    pub session_id: SessionId,
    pub surface: Surface,
}

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub enum AuthorizationOutcome {
    Allowed,
    NotAuthorized,
    ScopeDenied,
    Expired,
    Revoked,
    InvalidRequest,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum AuthorizationError {
    NotAuthorized,
    ScopeDenied,
    Expired,
    Revoked,
    InvalidRequest,
}

impl AuthorizationError {
    const fn outcome(&self) -> AuthorizationOutcome {
        match self {
            Self::NotAuthorized => AuthorizationOutcome::NotAuthorized,
            Self::ScopeDenied => AuthorizationOutcome::ScopeDenied,
            Self::Expired => AuthorizationOutcome::Expired,
            Self::Revoked => AuthorizationOutcome::Revoked,
            Self::InvalidRequest => AuthorizationOutcome::InvalidRequest,
        }
    }
}

#[derive(Clone, Eq, PartialEq)]
pub struct TenantAuditEntry {
    pub tenant: TenantId,
    /// Domain-separated SHA-256 correlation digest, never the raw bearer identifier.
    pub token_correlation: [u8; 32],
    pub surface: Surface,
    pub operation: String,
    pub outcome: AuthorizationOutcome,
}

impl std::fmt::Debug for TenantAuditEntry {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("TenantAuditEntry")
            .field("tenant", &self.tenant)
            .field("token_correlation", &"[REDACTED]")
            .field("surface", &self.surface)
            .field("operation", &self.operation)
            .field("outcome", &self.outcome)
            .finish()
    }
}

#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub struct MetricKey {
    pub tenant: TenantId,
    pub surface: Surface,
    pub outcome: AuthorizationOutcome,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TenantTrace {
    pub tenant: TenantId,
    pub surface: Surface,
    pub operation: String,
    pub outcome: AuthorizationOutcome,
}

/// Observable evidence emitted by the common tenant gate.
#[derive(Debug, Default)]
pub struct TenantObservability {
    audit: Vec<TenantAuditEntry>,
    metrics: BTreeMap<MetricKey, u64>,
    traces: Vec<TenantTrace>,
}

impl TenantObservability {
    #[must_use]
    pub fn audit(&self) -> &[TenantAuditEntry] {
        &self.audit
    }

    #[must_use]
    pub fn metrics(&self) -> &BTreeMap<MetricKey, u64> {
        &self.metrics
    }

    #[must_use]
    pub fn traces(&self) -> &[TenantTrace] {
        &self.traces
    }

    fn record(
        &mut self,
        token: &Token,
        surface: Surface,
        operation: &str,
        outcome: AuthorizationOutcome,
    ) {
        let tenant = token.tenant().clone();
        self.audit.push(TenantAuditEntry {
            tenant: tenant.clone(),
            token_correlation: token.audit_correlation(),
            surface,
            operation: operation.to_owned(),
            outcome,
        });
        let counter = self
            .metrics
            .entry(MetricKey {
                tenant: tenant.clone(),
                surface,
                outcome,
            })
            .or_default();
        *counter = counter.saturating_add(1);
        self.traces.push(TenantTrace {
            tenant,
            surface,
            operation: operation.to_owned(),
            outcome,
        });
    }
}

/// Resolves the tenant and agent exclusively from the authenticated token against the current
/// session registry view.
///
/// # Errors
///
/// Returns `InvalidRequest` for an empty, oversized, or NUL-bearing operation,
/// `ScopeDenied`, `Expired` or `Revoked` from the token authorization, and
/// `NotAuthorized` for any other session failure or a target owned by another principal.
pub fn resolve(
    token: &Token,
    sessions: &SessionRegistry,
    request: &RequestContext,
    observability: &mut TenantObservability,
) -> Result<ResolvedPrincipal, AuthorizationError> {
    let authorized_scopes = OperationClass::authorized_scopes(request.operation);
    if authorized_scopes.is_empty() {
        let error = AuthorizationError::InvalidRequest;
        observability.record(
            token,
            request.surface,
            request.operation.name(),
            error.outcome(),
        );
        return Err(error);
    }
    let authorization =
        token.authorize_any_scope(sessions, authorized_scopes, request.core_sequence);
    let session_id = match authorization {
        Ok(session_id) => session_id,
        Err(failure) => {
            let error = match failure {
                SessionError::ScopeDenied => AuthorizationError::ScopeDenied,
                SessionError::Expired => AuthorizationError::Expired,
                SessionError::Revoked => AuthorizationError::Revoked,
                _ => AuthorizationError::NotAuthorized,
            };
            observability.record(
                token,
                request.surface,
                request.operation.name(),
                error.outcome(),
            );
            return Err(error);
        }
    };
    if let Some(owner) = &request.target_owner {
        if let Err(error) = require_owner(token.tenant(), token.agent(), owner) {
            observability.record(
                token,
                request.surface,
                request.operation.name(),
                error.outcome(),
            );
            return Err(error);
        }
    }
    observability.record(
        token,
        request.surface,
        request.operation.name(),
        AuthorizationOutcome::Allowed,
    );
    Ok(ResolvedPrincipal {
        tenant: token.tenant().clone(),
        agent: token.agent().clone(),
        session_id,
        surface: request.surface,
    })
}

/// Applies the same non-enumerating owner check to every target surface.
///
/// # Errors
///
/// Returns `NotAuthorized` when the owner tenant differs or a named owner agent is
/// not the caller.
pub fn require_owner(
    tenant: &TenantId,
    agent: &Did,
    owner: &ObjectOwner,
) -> Result<(), AuthorizationError> {
    if &owner.tenant != tenant || owner.agent.as_ref().is_some_and(|value| value != agent) {
        Err(AuthorizationError::NotAuthorized)
    } else {
        Ok(())
    }
}
