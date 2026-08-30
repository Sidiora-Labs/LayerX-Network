//! Authenticated approval operations over the policy hold registry.

use std::collections::BTreeMap;
use std::sync::Mutex;

use layerx_agent_api::prepare::{Disclosure, Prepared};
use sha2::{Digest as _, Sha256};

use crate::budget::{self, BudgetLimiter, ReleaseKind};
use crate::policy::approval::{
    ApprovalError as RegistryError, ApprovalRegistry, ApprovalSnapshot, ApprovalState, ApproverId,
    ReleasedApproval,
};
use crate::store::{Store, TenantId};

mod events;
mod expiry;

pub use events::{
    ApprovalEmission, ApprovalEventError, ApprovalEventKind, ApprovalEvents, ApprovalLifecycle,
};
pub use expiry::{ApprovalExpiry, ApprovalExpiryError, DecisionKey};

pub const APPROVAL_ENFORCEMENT_NOTICE: &str =
    "daemon-enforced restriction; confers no protocol authority; bypassing layerx-agentd bypasses this restriction";

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ApprovalEnforcement {
    DaemonOnly,
}

/// Human-readable reason attached to the policy hold.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct HoldReason {
    pub code: &'static str,
    pub message: &'static str,
}

/// Tenant-scoped record returned by list and get.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ApprovalRecord {
    pub approval_id: [u8; 32],
    pub tenant: TenantId,
    pub held_activity: Disclosure,
    pub canonical_bytes_digest: [u8; 32],
    pub hold_reason: HoldReason,
    pub created_at_sequence: u64,
    pub expires_at_sequence: u64,
    pub state: ApprovalState,
    pub submission_ref: Option<[u8; 32]>,
    pub enforcement: ApprovalEnforcement,
    pub authority_notice: &'static str,
}

/// Bounded deterministic approval page.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ApprovalPage {
    pub approvals: Vec<ApprovalRecord>,
    pub next_cursor: Option<[u8; 32]>,
}

/// Total result vocabulary for approval decisions.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ApprovalOutcome {
    Granted,
    Rejected,
    Expired,
    Defective,
    AlreadyDecided,
    Conflict,
}

/// Result returned to the requesting agent.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ApprovalDecision {
    pub outcome: ApprovalOutcome,
    pub submission_ref: Option<[u8; 32]>,
    pub winning_outcome: Option<ApprovalOutcome>,
    pub enforcement: ApprovalEnforcement,
    pub authority_notice: &'static str,
}

/// One preparation held under its owning tenant and approval identity.
#[derive(Clone)]
struct QueuedSubmission {
    tenant: TenantId,
    approval_id: [u8; 32],
    prepared: Prepared,
}

/// A real pre-signing submission queue containing the exact approved preparation.
#[derive(Default)]
pub struct ApprovalSubmissionQueue {
    queued: Mutex<BTreeMap<[u8; 32], QueuedSubmission>>,
}

impl ApprovalSubmissionQueue {
    pub(crate) fn matches_released_decision(
        &self,
        tenant: &TenantId,
        approval_id: [u8; 32],
        submission_ref: [u8; 32],
        disclosure_digest: [u8; 32],
    ) -> Result<bool, ApprovalOutcome> {
        let queued = self.queued.lock().map_err(|_| ApprovalOutcome::Conflict)?;
        Ok(queued.get(&submission_ref).is_some_and(|record| {
            &record.tenant == tenant
                && record.approval_id == approval_id
                && record.prepared.disclosure.canonical_digest == disclosure_digest
        }))
    }

    pub(crate) fn settle_verified(
        &self,
        tenant: &TenantId,
        idempotency_key: [u8; 32],
        result_code: i32,
        current_sequence: u64,
        store: &mut Store,
        limiter: &BudgetLimiter,
    ) -> Result<bool, ApprovalOperationError> {
        let identity = idempotency_key
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect::<String>();
        let mut queued = self
            .queued
            .lock()
            .map_err(|_| ApprovalOperationError::Registry(RegistryError::Unavailable))?;
        let selected = queued.iter().find_map(|(reference, record)| {
            (&record.tenant == tenant
                && record.prepared.disclosure.idempotency_key.as_str() == identity)
                .then_some((*reference, record.approval_id))
        });
        let Some((reference, approval_id)) = selected else {
            return Ok(false);
        };
        let key = crate::policy::approval::released_storage_key(tenant, approval_id)
            .map_err(ApprovalOperationError::Registry)?;
        if !store
            .remove_local(&key)
            .map_err(|_| ApprovalOperationError::Registry(RegistryError::Unavailable))?
        {
            return Err(ApprovalOperationError::Registry(
                RegistryError::CorruptRecord,
            ));
        }
        budget::release(
            limiter,
            approval_id,
            if result_code == 0 {
                ReleaseKind::Executed
            } else {
                ReleaseKind::Failed
            },
            current_sequence,
        )
        .map_err(ApprovalOperationError::Reservation)?;
        queued.remove(&reference);
        Ok(true)
    }

    pub(crate) fn authorize_submit(
        &self,
        tenant: &TenantId,
        preparation_ref: &str,
        canonical_bytes: &[u8],
        release_ref: Option<[u8; 32]>,
    ) -> Result<(), ApprovalOutcome> {
        let queued = self.queued.lock().map_err(|_| ApprovalOutcome::Conflict)?;
        match release_ref {
            Some(reference) => match queued.get(&reference) {
                Some(record)
                    if &record.tenant == tenant
                        && record.prepared.preparation_ref.as_str() == preparation_ref
                        && record.prepared.unsigned_canonical_bytes.as_bytes()
                            == canonical_bytes =>
                {
                    Ok(())
                }
                _ => Err(ApprovalOutcome::Conflict),
            },
            None if queued.values().any(|record| {
                &record.tenant == tenant
                    && record.prepared.preparation_ref.as_str() == preparation_ref
                    && record.prepared.unsigned_canonical_bytes.as_bytes() == canonical_bytes
            }) =>
            {
                Err(ApprovalOutcome::Conflict)
            }
            None => Ok(()),
        }
    }

    pub(crate) fn restore(&self, records: Vec<ReleasedApproval>) -> Result<(), ApprovalOutcome> {
        let mut queued = self.queued.lock().map_err(|_| ApprovalOutcome::Conflict)?;
        let mut restored = queued.clone();
        for record in records {
            if Self::reference(&record.tenant, record.approval_id, &record.prepared)
                != record.submission_ref
                || restored
                    .insert(
                        record.submission_ref,
                        QueuedSubmission {
                            tenant: record.tenant,
                            approval_id: record.approval_id,
                            prepared: record.prepared,
                        },
                    )
                    .is_some()
            {
                return Err(ApprovalOutcome::Conflict);
            }
        }
        *queued = restored;
        Ok(())
    }

    fn reference(tenant: &TenantId, approval_id: [u8; 32], prepared: &Prepared) -> [u8; 32] {
        let mut hasher = Sha256::new();
        hasher.update(b"layerx-approved-preparation-v1");
        hasher.update(tenant.as_str().as_bytes());
        hasher.update(approval_id);
        hasher.update(prepared.unsigned_canonical_bytes.as_bytes());
        hasher.finalize().into()
    }

    fn release(
        &self,
        tenant: TenantId,
        approval_id: [u8; 32],
        prepared: Prepared,
    ) -> Result<[u8; 32], ApprovalOutcome> {
        let submission_ref = Self::reference(&tenant, approval_id, &prepared);
        let mut queued = self.queued.lock().map_err(|_| ApprovalOutcome::Conflict)?;
        match queued.get(&submission_ref) {
            Some(stored)
                if stored.tenant == tenant
                    && stored.approval_id == approval_id
                    && stored.prepared == prepared =>
            {
                Ok(submission_ref)
            }
            Some(_) => Err(ApprovalOutcome::Conflict),
            None => {
                queued.insert(
                    submission_ref,
                    QueuedSubmission {
                        tenant,
                        approval_id,
                        prepared,
                    },
                );
                Ok(submission_ref)
            }
        }
    }

    /// Returns the exact preparation released by approval.
    #[must_use]
    pub fn prepared(&self, submission_ref: [u8; 32]) -> Option<Prepared> {
        self.queued.lock().ok().and_then(|queued| {
            queued
                .get(&submission_ref)
                .map(|stored| stored.prepared.clone())
        })
    }

    /// Number of preparations waiting for the ordinary sign-and-submit path.
    #[must_use]
    pub fn len(&self) -> usize {
        self.queued.lock().map_or(0, |queued| queued.len())
    }

    /// Whether no approved preparation is queued.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.queued.lock().map_or(true, |queued| queued.is_empty())
    }
}

/// Authenticated identity of exactly one approve or reject decision.
pub struct DecisionRequest<'a> {
    pub tenant: &'a TenantId,
    pub approval_id: [u8; 32],
    pub idempotency_key: &'a DecisionKey,
    pub approver: ApproverId,
    pub current_sequence: u64,
}

/// Tenant-authenticated list/get/approve/reject service.
pub struct ApprovalService<'a> {
    registry: &'a ApprovalRegistry,
    limiter: &'a BudgetLimiter,
    expiry: &'a ApprovalExpiry,
}

impl<'a> ApprovalService<'a> {
    #[must_use]
    pub const fn new(
        registry: &'a ApprovalRegistry,
        limiter: &'a BudgetLimiter,
        expiry: &'a ApprovalExpiry,
    ) -> Self {
        Self {
            registry,
            limiter,
            expiry,
        }
    }

    /// Lists only holds belonging to the authenticated tenant.
    ///
    /// # Errors
    ///
    /// Refuses an invalid page bound or an unavailable approval registry.
    pub fn list(
        &self,
        tenant: &TenantId,
        cursor: Option<[u8; 32]>,
        page_limit: usize,
        current_sequence: u64,
    ) -> Result<ApprovalPage, ApprovalOperationError> {
        if page_limit == 0 || page_limit > 100 {
            return Err(ApprovalOperationError::InvalidPageLimit);
        }
        let mut snapshots = self
            .registry
            .list_scoped(tenant, current_sequence)
            .map_err(ApprovalOperationError::Registry)?;
        snapshots.sort_by_key(|snapshot| snapshot.context.request_id);
        let mut records = Vec::new();
        for mut snapshot in snapshots {
            if cursor.is_some_and(|value| snapshot.context.request_id <= value) {
                continue;
            }
            snapshot.state = self
                .expiry
                .observe(&snapshot, current_sequence, self.limiter)
                .map_err(ApprovalOperationError::Durability)?;
            records.push(record(snapshot));
        }
        let next_cursor = (records.len() > page_limit).then(|| records[page_limit - 1].approval_id);
        records.truncate(page_limit);
        Ok(ApprovalPage {
            approvals: records,
            next_cursor,
        })
    }

    /// Gets one hold without revealing whether another tenant owns its identifier.
    ///
    /// # Errors
    ///
    /// Returns `NotFound` for absent and cross-tenant identifiers, or the registry failure.
    pub fn get(
        &self,
        tenant: &TenantId,
        approval_id: [u8; 32],
        current_sequence: u64,
    ) -> Result<ApprovalRecord, ApprovalOperationError> {
        let mut snapshot = self
            .registry
            .get_scoped(tenant, approval_id, current_sequence)
            .map_err(ApprovalOperationError::Registry)?;
        snapshot.state = self
            .expiry
            .observe(&snapshot, current_sequence, self.limiter)
            .map_err(ApprovalOperationError::Durability)?;
        Ok(record(snapshot))
    }

    /// Releases exactly the held prepared activity into the ordinary submission queue.
    ///
    /// # Errors
    ///
    /// Returns registry failures that cannot be represented by a typed decision outcome.
    pub fn approve(
        &self,
        request: DecisionRequest<'_>,
        current_prepared: &Prepared,
        submissions: &ApprovalSubmissionQueue,
    ) -> Result<ApprovalDecision, ApprovalOperationError> {
        let DecisionRequest {
            tenant,
            approval_id,
            idempotency_key,
            approver,
            current_sequence,
        } = request;
        if let Some(decision) = self
            .expiry
            .repeated(tenant, approval_id, idempotency_key)
            .map_err(ApprovalOperationError::Durability)?
        {
            return Ok(decision);
        }
        let snapshot = self
            .registry
            .get_scoped(tenant, approval_id, current_sequence)
            .map_err(ApprovalOperationError::Registry)?;
        let digest = canonical_digest(snapshot.prepared.unsigned_canonical_bytes.as_bytes());
        let intended = if current_prepared != &snapshot.prepared
            || digest != snapshot.prepared.disclosure.canonical_digest
        {
            ApprovalOutcome::Defective
        } else {
            ApprovalOutcome::Granted
        };
        let submission_ref = (intended == ApprovalOutcome::Granted)
            .then(|| ApprovalSubmissionQueue::reference(tenant, approval_id, &snapshot.prepared));
        let decision_record = match self
            .expiry
            .decide(
                &snapshot,
                current_sequence,
                idempotency_key,
                intended,
                submission_ref,
                self.limiter,
            )
            .map_err(ApprovalOperationError::Durability)?
        {
            expiry::DecisionResolution::Winner => None,
            expiry::DecisionResolution::WinnerPrepared(key, bytes) => Some((key, bytes)),
            expiry::DecisionResolution::Repeat(decision) => return Ok(decision),
            expiry::DecisionResolution::Conflict(winner) => {
                return Ok(conflict(winner));
            }
            expiry::DecisionResolution::Expired => {
                return Ok(decision(ApprovalOutcome::Expired, None));
            }
        };
        let claimed = match self
            .registry
            .claim_scoped(tenant, approval_id, current_sequence)
        {
            Ok(claimed) => claimed,
            Err(error) => return outcome_or_error(error),
        };
        if intended == ApprovalOutcome::Defective {
            self.registry
                .complete_claim(
                    approval_id,
                    ApprovalState::Defective,
                    approver,
                    "held_preparation_changed_after_approval_hold",
                    None,
                    None,
                )
                .map_err(ApprovalOperationError::Registry)?;
            return Ok(decision(ApprovalOutcome::Defective, None));
        }
        let queued_submission_ref =
            match submissions.release(tenant.clone(), approval_id, claimed.prepared.clone()) {
                Ok(reference) => reference,
                Err(outcome) => {
                    self.registry
                        .abort_claim(approval_id)
                        .map_err(ApprovalOperationError::Registry)?;
                    return Ok(decision(outcome, None));
                }
            };
        let fallback_decision_record = (!self.registry.has_durable_store())
            .then(|| decision_record.clone())
            .flatten();
        self.registry
            .complete_claim(
                approval_id,
                ApprovalState::Approved,
                approver,
                "approver_released_exact_preparation",
                Some(queued_submission_ref),
                decision_record,
            )
            .map_err(ApprovalOperationError::Registry)?;
        if let Some((key, bytes)) = fallback_decision_record {
            self.expiry
                .persist_prepared_decision(key, bytes)
                .map_err(ApprovalOperationError::Durability)?;
        }
        debug_assert_eq!(submission_ref, Some(queued_submission_ref));
        Ok(decision(
            ApprovalOutcome::Granted,
            Some(queued_submission_ref),
        ))
    }

    /// Finalizes rejection and releases the hold's reservation deterministically.
    ///
    /// # Errors
    ///
    /// Returns registry or reservation failures that prevent a final rejection.
    pub fn reject(
        &self,
        request: DecisionRequest<'_>,
    ) -> Result<ApprovalDecision, ApprovalOperationError> {
        let DecisionRequest {
            tenant,
            approval_id,
            idempotency_key,
            approver,
            current_sequence,
        } = request;
        let snapshot = self
            .registry
            .get_scoped(tenant, approval_id, current_sequence)
            .map_err(ApprovalOperationError::Registry)?;
        match self
            .expiry
            .decide(
                &snapshot,
                current_sequence,
                idempotency_key,
                ApprovalOutcome::Rejected,
                None,
                self.limiter,
            )
            .map_err(ApprovalOperationError::Durability)?
        {
            expiry::DecisionResolution::Winner => {}
            expiry::DecisionResolution::WinnerPrepared(_, _) => {
                return Err(ApprovalOperationError::Durability(
                    ApprovalExpiryError::Corrupt,
                ))
            }
            expiry::DecisionResolution::Repeat(decision) => return Ok(decision),
            expiry::DecisionResolution::Conflict(winner) => {
                return Ok(conflict(winner));
            }
            expiry::DecisionResolution::Expired => {
                return Ok(decision(ApprovalOutcome::Expired, None));
            }
        }
        match self
            .registry
            .claim_scoped(tenant, approval_id, current_sequence)
        {
            Ok(_) => {}
            Err(error) => return outcome_or_error(error),
        }
        if let Err(error) = budget::release(
            self.limiter,
            approval_id,
            ReleaseKind::Failed,
            current_sequence,
        ) {
            self.registry
                .abort_claim(approval_id)
                .map_err(ApprovalOperationError::Registry)?;
            return Err(ApprovalOperationError::Reservation(error));
        }
        self.registry
            .complete_claim(
                approval_id,
                ApprovalState::Rejected,
                approver,
                "approver_rejected_and_released_reservation",
                None,
                None,
            )
            .map_err(ApprovalOperationError::Registry)?;
        Ok(decision(ApprovalOutcome::Rejected, None))
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ApprovalOperationError {
    InvalidPageLimit,
    Registry(RegistryError),
    Reservation(crate::budget::LimitRefusal),
    Durability(ApprovalExpiryError),
}

fn record(snapshot: ApprovalSnapshot) -> ApprovalRecord {
    ApprovalRecord {
        approval_id: snapshot.context.request_id,
        tenant: snapshot.context.tenant,
        held_activity: snapshot.prepared.disclosure.clone(),
        canonical_bytes_digest: canonical_digest(
            snapshot.prepared.unsigned_canonical_bytes.as_bytes(),
        ),
        hold_reason: HoldReason {
            code: "policy_approval_required",
            message: "Policy requires human approval before submission",
        },
        created_at_sequence: snapshot.created_at_sequence,
        expires_at_sequence: snapshot.expires_at_sequence,
        state: snapshot.state,
        submission_ref: snapshot.submission_ref,
        enforcement: ApprovalEnforcement::DaemonOnly,
        authority_notice: APPROVAL_ENFORCEMENT_NOTICE,
    }
}

fn outcome_or_error(error: RegistryError) -> Result<ApprovalDecision, ApprovalOperationError> {
    match error {
        RegistryError::AlreadyDecided(ApprovalState::Expired) => {
            Ok(decision(ApprovalOutcome::Expired, None))
        }
        RegistryError::AlreadyDecided(_) => Ok(decision(ApprovalOutcome::AlreadyDecided, None)),
        RegistryError::Defective => Ok(decision(ApprovalOutcome::Defective, None)),
        RegistryError::DecisionConflict | RegistryError::DisclosureChanged => {
            Ok(decision(ApprovalOutcome::Conflict, None))
        }
        other => Err(ApprovalOperationError::Registry(other)),
    }
}

const fn decision(outcome: ApprovalOutcome, submission_ref: Option<[u8; 32]>) -> ApprovalDecision {
    ApprovalDecision {
        outcome,
        submission_ref,
        winning_outcome: None,
        enforcement: ApprovalEnforcement::DaemonOnly,
        authority_notice: APPROVAL_ENFORCEMENT_NOTICE,
    }
}

const fn conflict(winner: ApprovalOutcome) -> ApprovalDecision {
    ApprovalDecision {
        outcome: ApprovalOutcome::Conflict,
        submission_ref: None,
        winning_outcome: Some(winner),
        enforcement: ApprovalEnforcement::DaemonOnly,
        authority_notice: APPROVAL_ENFORCEMENT_NOTICE,
    }
}

fn canonical_digest(bytes: &[u8]) -> [u8; 32] {
    let mut hasher = Sha256::new();
    hasher.update(bytes);
    hasher.finalize().into()
}
