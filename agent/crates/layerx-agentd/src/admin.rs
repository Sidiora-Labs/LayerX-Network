//! Audited operator controls that cannot mutate protocol-derived facts.

use std::path::Path;

use layerx_agent_api::subscription::{SubscriptionRecord, SubscriptionTarget};
use layerx_types::verify::VerificationLevel;
use sha2::{Digest, Sha256};

use crate::audit::{AuditError, Log};
use crate::budget::{
    self, BudgetDivergenceAlert, LocalAccounting, ProtocolBudgetState, ReconcileError,
    ReconciliationState, SpendReceiptEvidence,
};
use crate::events::subscription::{Continuity, Store as SubscriptionStore, SubscriptionError};
use crate::finality::{self, FinalityError, VerificationProgress, WaitResult};
use crate::outbox::{
    self, Outbox, ReceiptLookup, SubmissionState, SubmissionStatus, UnknownResolution,
    UnknownResolutionError,
};
use crate::prepare::PrepareRequest;
use crate::store::TenantId;

/// Stable operator commands exposed by the daemon.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum OperatorCommand {
    InspectUnknown([u8; 32]),
    ResolveUnknown([u8; 32]),
    InspectStalledSubscription([u8; 32]),
    ResumeStalledSubscription([u8; 32]),
    InspectBudgetDivergence([u8; 32]),
    ReconcileBudgetDivergence([u8; 32]),
    InspectVerificationBacklog([u8; 32]),
    RetryVerification([u8; 32]),
    SubmitActivity([u8; 32]),
    AttemptProtectedMutation {
        target: [u8; 32],
        mutation: ProtectedMutation,
    },
}

impl OperatorCommand {
    const fn code(self) -> u8 {
        match self {
            Self::InspectUnknown(_) => 1,
            Self::ResolveUnknown(_) => 2,
            Self::InspectStalledSubscription(_) => 3,
            Self::ResumeStalledSubscription(_) => 4,
            Self::InspectBudgetDivergence(_) => 5,
            Self::ReconcileBudgetDivergence(_) => 6,
            Self::InspectVerificationBacklog(_) => 7,
            Self::RetryVerification(_) => 8,
            Self::SubmitActivity(_) => 9,
            Self::AttemptProtectedMutation { mutation, .. } => 32 + mutation.code(),
        }
    }

    const fn target(self) -> [u8; 32] {
        match self {
            Self::InspectUnknown(target)
            | Self::ResolveUnknown(target)
            | Self::InspectStalledSubscription(target)
            | Self::ResumeStalledSubscription(target)
            | Self::InspectBudgetDivergence(target)
            | Self::ReconcileBudgetDivergence(target)
            | Self::InspectVerificationBacklog(target)
            | Self::RetryVerification(target)
            | Self::SubmitActivity(target)
            | Self::AttemptProtectedMutation { target, .. } => target,
        }
    }
}

/// Operations deliberately absent from the operator command catalogue.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ProtectedMutation {
    MarkUnknownExecuted,
    ReplaceReceipt,
    RaiseVerificationLevel,
    RewriteAuditEntry,
    ReplaceProtocolValue,
}

impl ProtectedMutation {
    const fn code(self) -> u8 {
        match self {
            Self::MarkUnknownExecuted => 1,
            Self::ReplaceReceipt => 2,
            Self::RaiseVerificationLevel => 3,
            Self::RewriteAuditEntry => 4,
            Self::ReplaceProtocolValue => 5,
        }
    }
}

/// Execution routes are constrained to existing non-authoritative subsystems.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ActionPlan {
    InspectOnly,
    ReceiptLookupAndExactResend,
    ResumeDaemonLocalSubscription,
    ReconcileFromVerifiedCoreEvidence,
    RetryEvidenceVerification,
    OrdinaryClientWrite([ClientWriteStage; 4]),
}

/// Mandatory stages for every new activity, regardless of who requested it.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ClientWriteStage {
    Prepare,
    Policy,
    Sign,
    Submit,
}

pub const ORDINARY_CLIENT_WRITE: [ClientWriteStage; 4] = [
    ClientWriteStage::Prepare,
    ClientWriteStage::Policy,
    ClientWriteStage::Sign,
    ClientWriteStage::Submit,
];

/// Published metadata for one supported operator command.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct CommandDescriptor {
    pub name: &'static str,
    pub target: &'static str,
    pub action: &'static str,
    pub protocol_mutating: bool,
}

const COMMANDS: [CommandDescriptor; 9] = [
    CommandDescriptor {
        name: "unknown.inspect",
        target: "unknown submission",
        action: "read daemon-local outbox status",
        protocol_mutating: false,
    },
    CommandDescriptor {
        name: "unknown.resolve",
        target: "unknown submission",
        action: "look up a verified receipt or resend identical bytes",
        protocol_mutating: false,
    },
    CommandDescriptor {
        name: "subscription.inspect",
        target: "stalled subscription",
        action: "read daemon-local delivery state",
        protocol_mutating: false,
    },
    CommandDescriptor {
        name: "subscription.resume",
        target: "stalled subscription",
        action: "resume daemon-local delivery from its durable cursor",
        protocol_mutating: false,
    },
    CommandDescriptor {
        name: "budget.inspect",
        target: "budget divergence",
        action: "read verified reconciliation evidence",
        protocol_mutating: false,
    },
    CommandDescriptor {
        name: "budget.reconcile",
        target: "budget divergence",
        action: "rebuild a local cache from verified core evidence",
        protocol_mutating: false,
    },
    CommandDescriptor {
        name: "verification.inspect",
        target: "verification backlog",
        action: "read requested and observed verification levels",
        protocol_mutating: false,
    },
    CommandDescriptor {
        name: "verification.retry",
        target: "verification backlog",
        action: "retry evidence verification without assigning a level",
        protocol_mutating: false,
    },
    CommandDescriptor {
        name: "activity.submit",
        target: "new activity",
        action: "enter the ordinary client prepare, policy, sign and submit path",
        protocol_mutating: false,
    },
];

/// Returns the complete supported command catalogue.
#[must_use]
pub const fn commands() -> &'static [CommandDescriptor] {
    &COMMANDS
}

/// Proves that a command can only select a non-authoritative execution route.
///
/// # Errors
///
/// Refuses every attempted receipt, verification-level, audit, outcome, or protocol-value edit.
pub const fn assert_non_mutating(command: &OperatorCommand) -> Result<ActionPlan, AdminError> {
    match command {
        OperatorCommand::InspectUnknown(_)
        | OperatorCommand::InspectStalledSubscription(_)
        | OperatorCommand::InspectBudgetDivergence(_)
        | OperatorCommand::InspectVerificationBacklog(_) => Ok(ActionPlan::InspectOnly),
        OperatorCommand::ResolveUnknown(_) => Ok(ActionPlan::ReceiptLookupAndExactResend),
        OperatorCommand::ResumeStalledSubscription(_) => {
            Ok(ActionPlan::ResumeDaemonLocalSubscription)
        }
        OperatorCommand::ReconcileBudgetDivergence(_) => {
            Ok(ActionPlan::ReconcileFromVerifiedCoreEvidence)
        }
        OperatorCommand::RetryVerification(_) => Ok(ActionPlan::RetryEvidenceVerification),
        OperatorCommand::SubmitActivity(_) => {
            Ok(ActionPlan::OrdinaryClientWrite(ORDINARY_CLIENT_WRITE))
        }
        OperatorCommand::AttemptProtectedMutation { mutation, .. } => {
            Err(AdminError::ProtectedMutation(*mutation))
        }
    }
}

/// Authenticated identity and correlation for one operator attempt.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct OperatorContext {
    pub operator_id: String,
    pub request_id: [u8; 32],
}

impl OperatorContext {
    /// Creates a bounded, non-empty operator identity.
    ///
    /// # Errors
    ///
    /// Rejects an empty, oversized, or NUL-bearing identity.
    pub fn new(operator_id: impl Into<String>, request_id: [u8; 32]) -> Result<Self, AdminError> {
        let operator_id = operator_id.into();
        if operator_id.is_empty() || operator_id.len() > 256 || operator_id.as_bytes().contains(&0)
        {
            return Err(AdminError::InvalidOperator);
        }
        Ok(Self {
            operator_id,
            request_id,
        })
    }
}

/// One operator-visible stalled-subscription snapshot.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct StalledSubscription {
    pub record: SubscriptionRecord,
    pub continuity: Continuity,
}

/// One bounded verification retry item. Levels are observations, never operator assignments.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct VerificationBacklog {
    pub idempotency_key: [u8; 32],
    pub observed: VerificationLevel,
    pub requested: VerificationLevel,
    pub queued_at_ms: u64,
    pub attempts: u32,
}

/// A new-activity request that can only be handed to the ordinary client write path.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ClientWritePlan {
    request: PrepareRequest,
}

impl ClientWritePlan {
    #[must_use]
    #[allow(clippy::unused_self)]
    pub const fn stages(&self) -> [ClientWriteStage; 4] {
        ORDINARY_CLIENT_WRITE
    }

    #[must_use]
    pub fn into_request(self) -> PrepareRequest {
        self.request
    }
}

/// Tenant-bound operator entry point. The audit log is private and append-only.
pub struct Surface {
    audit: Log,
}

impl Surface {
    /// Opens the exact tenant's tamper-evident operator audit chain.
    ///
    /// # Errors
    ///
    /// Returns an audit error if the chain is absent, corrupt, or cannot be opened durably.
    pub fn open(root: impl AsRef<Path>, tenant: &TenantId) -> Result<Self, AdminError> {
        Ok(Self {
            audit: Log::open(root, tenant).map_err(AdminError::Audit)?,
        })
    }

    #[must_use]
    pub fn audit_path(&self) -> &Path {
        self.audit.path()
    }

    #[must_use]
    pub const fn audit_entries(&self) -> u64 {
        self.audit.entries()
    }

    /// Audits an attempt before selecting its constrained execution route.
    ///
    /// # Errors
    ///
    /// Fails closed if audit persistence fails or the request is a protected mutation.
    pub fn dispatch(
        &mut self,
        context: &OperatorContext,
        command: OperatorCommand,
    ) -> Result<ActionPlan, AdminError> {
        let payload = audit_payload(context, command)?;
        let (_, result) = self
            .audit
            .before_operation(&payload, || assert_non_mutating(&command))
            .map_err(AdminError::Audit)?;
        result
    }

    /// Returns an unknown outbox status without granting transition authority.
    ///
    /// # Errors
    ///
    /// Returns `InvalidOperator` or `Audit` when the attempt cannot be audited, and
    /// `NotUnknown` when the submission is absent or is not in the `Unknown` state.
    pub fn inspect_unknown(
        &mut self,
        context: &OperatorContext,
        outbox: &Outbox,
        submission_id: [u8; 32],
    ) -> Result<SubmissionStatus, AdminError> {
        self.dispatch(context, OperatorCommand::InspectUnknown(submission_id))?;
        let status = outbox
            .status(submission_id)
            .filter(|status| status.state == SubmissionState::Unknown)
            .cloned()
            .ok_or(AdminError::NotUnknown)?;
        Ok(status)
    }

    /// Runs the existing receipt-only unknown resolver after durable audit.
    ///
    /// # Errors
    ///
    /// Returns `InvalidOperator` or `Audit` when the attempt cannot be audited, then
    /// `UnknownResolution` for an unknown submission id, a non-`Unknown` state, an
    /// observation time before the first sighting, or a failed durable backoff write.
    pub fn resolve_unknown<B: ReceiptLookup>(
        &mut self,
        context: &OperatorContext,
        outbox: &mut Outbox,
        store: &mut crate::store::Store,
        submission_id: [u8; 32],
        observed_at_ms: u64,
        boundary: &mut B,
    ) -> Result<UnknownResolution, AdminError> {
        self.dispatch(context, OperatorCommand::ResolveUnknown(submission_id))?;
        outbox::resolve_unknown(outbox, store, submission_id, observed_at_ms, boundary)
            .map_err(AdminError::UnknownResolution)
    }

    /// Reads an exact-scope subscription only when it is paused or continuity-blocked.
    ///
    /// # Errors
    ///
    /// Returns `InvalidOperator` or `Audit` when the attempt cannot be audited,
    /// `Subscription` carrying `NotFound` for an absent, terminated or out-of-scope target,
    /// and `NotStalled` when the record is neither paused nor continuity-blocked.
    pub fn inspect_stalled_subscription(
        &mut self,
        context: &OperatorContext,
        subscriptions: &SubscriptionStore,
        target: &SubscriptionTarget,
    ) -> Result<StalledSubscription, AdminError> {
        let target_id = target_digest(target.subscription_id.as_str().as_bytes());
        self.dispatch(
            context,
            OperatorCommand::InspectStalledSubscription(target_id),
        )?;
        let record = subscriptions
            .get(target)
            .map_err(AdminError::Subscription)?;
        let continuity = subscriptions
            .continuity(target)
            .map_err(AdminError::Subscription)?;
        if !record.paused && continuity == Continuity::Healthy {
            return Err(AdminError::NotStalled);
        }
        Ok(StalledSubscription { record, continuity })
    }

    /// Resumes only daemon-local delivery; gap and truncation state remain enforced.
    ///
    /// # Errors
    ///
    /// Returns `InvalidOperator` or `Audit` when the attempt cannot be audited, or
    /// `Subscription` carrying `NotFound` for an absent, terminated or out-of-scope target
    /// and `Durable` when the unpaused record cannot be written back.
    pub fn resume_stalled_subscription(
        &mut self,
        context: &OperatorContext,
        subscriptions: &mut SubscriptionStore,
        target: &SubscriptionTarget,
    ) -> Result<SubscriptionRecord, AdminError> {
        let target_id = target_digest(target.subscription_id.as_str().as_bytes());
        self.dispatch(
            context,
            OperatorCommand::ResumeStalledSubscription(target_id),
        )?;
        subscriptions
            .resume(target)
            .map_err(AdminError::Subscription)
    }

    /// Audits and returns the already-derived divergence evidence unchanged.
    ///
    /// # Errors
    ///
    /// Returns `InvalidOperator` or `Audit` when the attempt cannot be audited; the supplied
    /// alert is never re-derived, so no divergence-specific failure exists here.
    pub fn inspect_budget_divergence(
        &mut self,
        context: &OperatorContext,
        budget_id: [u8; 32],
        alert: BudgetDivergenceAlert,
    ) -> Result<BudgetDivergenceAlert, AdminError> {
        self.dispatch(context, OperatorCommand::InspectBudgetDivergence(budget_id))?;
        Ok(alert)
    }

    /// Rebuilds only the local budget cache from verified core evidence and receipts.
    ///
    /// # Errors
    ///
    /// Returns `InvalidOperator` or `Audit` when the attempt cannot be audited, or
    /// `BudgetReconciliation` for unverified protocol state, an unverified receipt, a receipt
    /// from another spend window, or accounting overflow.
    pub fn reconcile_budget_divergence(
        &mut self,
        context: &OperatorContext,
        budget_id: [u8; 32],
        local: &mut LocalAccounting,
        protocol: ProtocolBudgetState,
        receipts: &[SpendReceiptEvidence],
    ) -> Result<ReconciliationState, AdminError> {
        self.dispatch(
            context,
            OperatorCommand::ReconcileBudgetDivergence(budget_id),
        )?;
        budget::reconcile(local, protocol, receipts).map_err(AdminError::BudgetReconciliation)
    }

    /// Audits and returns a genuine backlog observation without changing its levels.
    ///
    /// # Errors
    ///
    /// Returns `InvalidOperator` or `Audit` when the attempt cannot be audited, or
    /// `NotBacklogged` when the observed level already reaches the requested one.
    pub fn inspect_verification_backlog(
        &mut self,
        context: &OperatorContext,
        backlog: VerificationBacklog,
    ) -> Result<VerificationBacklog, AdminError> {
        self.dispatch(
            context,
            OperatorCommand::InspectVerificationBacklog(backlog.idempotency_key),
        )?;
        if backlog.observed >= backlog.requested {
            return Err(AdminError::NotBacklogged);
        }
        Ok(backlog)
    }

    /// Retries the normal evidence observer; the operator cannot supply the reached level.
    ///
    /// # Errors
    ///
    /// Returns `InvalidOperator` or `Audit` when the attempt cannot be audited, or
    /// `Verification` for a deadline before `started_at_ms`, a zero poll interval, poll-clock
    /// overflow, or the observer's own failure to report a level.
    pub fn retry_verification<P: VerificationProgress>(
        &mut self,
        context: &OperatorContext,
        progress: &mut P,
        backlog: VerificationBacklog,
        started_at_ms: u64,
        deadline_ms: u64,
        poll_interval_ms: u64,
    ) -> Result<WaitResult, AdminError> {
        self.dispatch(
            context,
            OperatorCommand::RetryVerification(backlog.idempotency_key),
        )?;
        finality::wait_for_level(
            progress,
            backlog.idempotency_key,
            backlog.requested,
            started_at_ms,
            deadline_ms,
            poll_interval_ms,
        )
        .map_err(AdminError::Verification)
    }

    /// Converts an operator request into the same complete input accepted from clients.
    ///
    /// # Errors
    ///
    /// Returns `InvalidOperator` or `Audit` when the attempt cannot be audited, or
    /// `RouteInvariant` if dispatch selected anything but the four-stage
    /// `OrdinaryClientWrite` plan.
    pub fn route_activity(
        &mut self,
        context: &OperatorContext,
        request: PrepareRequest,
    ) -> Result<ClientWritePlan, AdminError> {
        let idempotency_key = request.idempotency_key.bytes();
        let route = self.dispatch(context, OperatorCommand::SubmitActivity(idempotency_key))?;
        if route != ActionPlan::OrdinaryClientWrite(ORDINARY_CLIENT_WRITE) {
            return Err(AdminError::RouteInvariant);
        }
        Ok(ClientWritePlan { request })
    }
}

/// Failures are explicit and never carry the attempted replacement bytes.
#[derive(Debug)]
pub enum AdminError {
    InvalidOperator,
    Audit(AuditError),
    ProtectedMutation(ProtectedMutation),
    NotUnknown,
    NotStalled,
    NotBacklogged,
    UnknownResolution(UnknownResolutionError),
    Subscription(SubscriptionError),
    BudgetReconciliation(ReconcileError),
    Verification(FinalityError),
    RouteInvariant,
    Arithmetic,
}

fn audit_payload(
    context: &OperatorContext,
    command: OperatorCommand,
) -> Result<Vec<u8>, AdminError> {
    let operator = context.operator_id.as_bytes();
    if operator.is_empty() || operator.len() > 256 || operator.contains(&0) {
        return Err(AdminError::InvalidOperator);
    }
    let length = u16::try_from(operator.len()).map_err(|_| AdminError::Arithmetic)?;
    let mut payload = Vec::with_capacity(72 + operator.len());
    payload.extend_from_slice(b"LXOA");
    payload.push(1);
    payload.extend_from_slice(&length.to_be_bytes());
    payload.extend_from_slice(operator);
    payload.extend_from_slice(&context.request_id);
    payload.push(command.code());
    payload.extend_from_slice(&command.target());
    Ok(payload)
}

fn target_digest(value: &[u8]) -> [u8; 32] {
    Sha256::digest(value).into()
}
