//! Destructive, step-up-bound managed-agent archive with retained evidence.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt::{Display, Formatter};

use layerx_types::ids::{AssetId, Did};
use layerx_types::verify::VerificationLevel;
use serde::{Deserialize, Serialize};
use sha2::{Digest as _, Sha256};

use crate::audit::{
    AuditChain, AuditError, AuditEvent, NotificationChannel, NotificationClass, SecurityChangeKind,
    StepUpEvidence as AuditStepUpEvidence,
};
use crate::auth::{
    AccessDecision, AuthError, AuthorizationRequest, OperationClass, OperationDigest, Passkeys,
    StepUpEvidence,
};
use crate::custody::{
    AgentContractError, AgentSessionContract, RevocationOutcome, RevocationReason,
    SessionEntropySource, SessionKeyError, SessionKeyProvisioner,
};
use crate::notify::{AgentId, NotifyError};
use crate::store::{EvidenceRef, PrincipalId, PrincipalScope, RowKey, StoreError, Table};
use crate::trace::TraceId;

use super::{ReclaimStage, ReclaimStatus};

const RECORD_VERSION: u8 = 1;
const RECORD_PREFIX: &str = "agent-archive-";
const NOTIFICATION_PREFIX: &str = "agent-archive-notification-";
const DIGEST_DOMAIN: &[u8] = b"layerx-human/agent-archive/v1\0";

/// Destructive confirmation tone required by both shells.
pub const ARCHIVE_CONFIRMATION_TONE: &str = "danger";
/// Exact irreversible consequence shown before the typed-name control.
pub const ARCHIVE_IRREVERSIBILITY_NOTICE: &str =
    "Archiving is permanent. This agent can never act or be resumed again.";
/// Product action named by the archive contract.
pub const ARCHIVE_ACTION_LABEL: &str = "Archive agent";

/// One exact verified balance before or after legitimate reclaim movements.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct AgentBalance {
    pub asset: AssetId,
    pub amount: u128,
}

/// Agent-layer proof that all value was disposed through receipt-backed
/// reclaim movements and that the current protocol balance is zero.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct FundsDispositionEvidence {
    pub before: Vec<AgentBalance>,
    pub after: Vec<AgentBalance>,
    pub protocol_state_digest: [u8; 32],
    pub observed_sequence: u64,
    pub observed_at: u64,
    pub verification_level: VerificationLevel,
}

/// Real agent contract required by archive in addition to session authority.
pub trait ArchiveAgentContract: AgentSessionContract {
    /// Returns proof-backed balances before and after completed reclaim work.
    ///
    /// # Errors
    ///
    /// Returns a typed agent-contract refusal without assuming funds are zero.
    fn funds_disposition(
        &mut self,
        principal: &PrincipalId,
        did: &Did,
        agent_id: &AgentId,
    ) -> Result<FundsDispositionEvidence, AgentContractError>;
}

/// Boundary consumed by the durable archive journey.
pub trait ArchiveBoundary {
    /// Reads verified disposition evidence from the real agent contract.
    ///
    /// # Errors
    ///
    /// Returns a typed boundary failure without advancing archive state.
    fn funds_disposition(
        &mut self,
        principal: &PrincipalId,
        did: &Did,
        agent_id: &AgentId,
    ) -> Result<FundsDispositionEvidence, ArchiveError>;

    /// Permanently removes daemon and protocol session authority.
    ///
    /// # Errors
    ///
    /// Returns a typed failure with the retry identity preserved by the journey.
    fn archive_authority(
        &mut self,
        principal: &PrincipalId,
        did: &Did,
        requested_at: u64,
    ) -> Result<RevocationOutcome, ArchiveError>;
}

/// Concrete archive bridge over the production session-key provisioner.
pub struct SessionArchiveAdapter<C: ArchiveAgentContract, E: SessionEntropySource> {
    sessions: SessionKeyProvisioner<C, E>,
}

impl<C: ArchiveAgentContract, E: SessionEntropySource> SessionArchiveAdapter<C, E> {
    #[must_use]
    pub const fn new(sessions: SessionKeyProvisioner<C, E>) -> Self {
        Self { sessions }
    }

    #[must_use]
    pub const fn sessions(&self) -> &SessionKeyProvisioner<C, E> {
        &self.sessions
    }

    #[must_use]
    pub const fn sessions_mut(&mut self) -> &mut SessionKeyProvisioner<C, E> {
        &mut self.sessions
    }
}

impl<C: ArchiveAgentContract, E: SessionEntropySource> ArchiveBoundary
    for SessionArchiveAdapter<C, E>
{
    fn funds_disposition(
        &mut self,
        principal: &PrincipalId,
        did: &Did,
        agent_id: &AgentId,
    ) -> Result<FundsDispositionEvidence, ArchiveError> {
        Ok(self
            .sessions
            .contract_mut()
            .funds_disposition(principal, did, agent_id)?)
    }

    fn archive_authority(
        &mut self,
        principal: &PrincipalId,
        did: &Did,
        requested_at: u64,
    ) -> Result<RevocationOutcome, ArchiveError> {
        Ok(self.sessions.archive(principal, did, requested_at)?)
    }
}

/// Immutable request. History references are retained read-only and pinned by
/// exportable archive audit evidence.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ArchiveRequest {
    pub idempotency_key: [u8; 32],
    pub agent_id: AgentId,
    pub agent_name: String,
    pub did: Did,
    pub history: Vec<EvidenceRef>,
}

/// Public destructive journey stage.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ArchiveStage {
    AwaitingConfirmation,
    StoppingWork,
    Archived,
}

/// Read-only archive status. There is deliberately no resume or delete action.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ArchiveStatus {
    agent_id: AgentId,
    agent_name: String,
    stage: ArchiveStage,
    history_entries: usize,
    suspension_receipt_digest: Option<[u8; 32]>,
    revocation_receipt_digest: Option<[u8; 32]>,
}

impl ArchiveStatus {
    #[must_use]
    pub const fn agent_id(&self) -> &AgentId {
        &self.agent_id
    }

    #[must_use]
    pub fn agent_name(&self) -> &str {
        &self.agent_name
    }

    #[must_use]
    pub const fn stage(&self) -> ArchiveStage {
        self.stage
    }

    #[must_use]
    pub const fn history_entries(&self) -> usize {
        self.history_entries
    }

    #[must_use]
    pub const fn irreversible(&self) -> bool {
        matches!(self.stage, ArchiveStage::Archived)
    }

    #[must_use]
    pub const fn suspension_receipt_digest(&self) -> Option<[u8; 32]> {
        self.suspension_receipt_digest
    }

    #[must_use]
    pub const fn revocation_receipt_digest(&self) -> Option<[u8; 32]> {
        self.revocation_receipt_digest
    }
}

/// One immutable archived history or receipt row.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ArchivedHistoryEntry {
    table: Table,
    key: RowKey,
    written_at: u64,
    bytes: Vec<u8>,
}

impl ArchivedHistoryEntry {
    #[must_use]
    pub const fn table(&self) -> Table {
        self.table
    }

    #[must_use]
    pub const fn key(&self) -> &RowKey {
        &self.key
    }

    #[must_use]
    pub const fn written_at(&self) -> u64 {
        self.written_at
    }

    #[must_use]
    pub fn bytes(&self) -> &[u8] {
        &self.bytes
    }
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
enum StoredPhase {
    AwaitingConfirmation,
    Revoking,
    Archived,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
struct StoredEvidenceRef {
    table: u8,
    key: String,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
struct Record {
    version: u8,
    request_digest: [u8; 32],
    idempotency_key: [u8; 32],
    agent_id: String,
    agent_name: String,
    did: Vec<u8>,
    disposition_digest: [u8; 32],
    reclaim_receipts: Vec<[u8; 32]>,
    history: Vec<StoredEvidenceRef>,
    phase: StoredPhase,
    step_up_digest: Option<[u8; 32]>,
    requested_at: Option<u64>,
    suspended_at: Option<u64>,
    revoked_at: Option<u64>,
    suspension_receipt_digest: Option<[u8; 32]>,
    revocation_receipt_digest: Option<[u8; 32]>,
    notification_written: bool,
    created_at: u64,
    updated_at: u64,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
struct ArchiveNotification {
    class: String,
    agent_id: String,
    message: String,
    deep_link: String,
    action_copy_key: String,
    created_at: u64,
}

/// Durable destructive archive journey.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ArchiveJourney {
    record: Record,
}

impl ArchiveJourney {
    /// Verifies current zero balances and exact completed reclaim receipts,
    /// then persists a confirmation-bound archive candidate.
    ///
    /// # Errors
    ///
    /// Refuses funds that remain, missing or mismatched reclaim receipts,
    /// unverified state, foreign history, and conflicting idempotency reuse.
    pub fn start<B: ArchiveBoundary>(
        scope: &mut PrincipalScope<'_>,
        boundary: &mut B,
        request: &ArchiveRequest,
        completed_reclaims: &[ReclaimStatus],
        now: u64,
    ) -> Result<Self, ArchiveError> {
        validate_request(scope, request)?;
        let request_digest = request_digest(request);
        let row = record_row(request.idempotency_key)?;
        if let Some(existing) = scope.get(Table::Journeys, &row) {
            let record = decode(existing.bytes())?;
            if record.request_digest != request_digest {
                return Err(ArchiveError::IdempotencyConflict);
            }
            return Ok(Self { record });
        }
        let principal = scope.principal().clone();
        let evidence = boundary.funds_disposition(&principal, &request.did, &request.agent_id)?;
        validate_disposition(&evidence)?;
        let remaining = evidence
            .after
            .iter()
            .copied()
            .filter(|balance| balance.amount != 0)
            .collect::<Vec<_>>();
        if !remaining.is_empty() {
            return Err(ArchiveError::FundsRemain(remaining));
        }
        let reclaim_receipts =
            validate_reclaims(&request.agent_name, &evidence.before, completed_reclaims)?;
        let disposition_digest = disposition_digest(&evidence, &reclaim_receipts);
        let record = Record {
            version: RECORD_VERSION,
            request_digest,
            idempotency_key: request.idempotency_key,
            agent_id: request.agent_id.as_str().to_owned(),
            agent_name: request.agent_name.clone(),
            did: request.did.as_bytes().to_vec(),
            disposition_digest,
            reclaim_receipts,
            history: request.history.iter().map(stored_reference).collect(),
            phase: StoredPhase::AwaitingConfirmation,
            step_up_digest: None,
            requested_at: None,
            suspended_at: None,
            revoked_at: None,
            suspension_receipt_digest: None,
            revocation_receipt_digest: None,
            notification_written: false,
            created_at: now,
            updated_at: now,
        };
        persist(scope, &record)?;
        Ok(Self { record })
    }

    /// Loads this principal's archive journey by retry identity.
    ///
    /// # Errors
    ///
    /// Refuses malformed or contradictory durable state.
    pub fn load(
        scope: &PrincipalScope<'_>,
        idempotency_key: [u8; 32],
    ) -> Result<Option<Self>, ArchiveError> {
        scope
            .get(Table::Journeys, &record_row(idempotency_key)?)
            .map_or(Ok(None), |row| {
                Ok(Some(Self {
                    record: decode(row.bytes())?,
                }))
            })
    }

    /// Digest a fresh passkey ceremony must confirm for this exact archive.
    #[must_use]
    pub fn operation_digest(&self) -> OperationDigest {
        OperationDigest::new(self.record.request_digest)
    }

    /// Checks the exact typed name, authorizes the destructive operation with
    /// fresh passkey evidence, and permanently retires daemon and protocol
    /// authority. Post-effect retries repair durable audit/notification state
    /// without repeating the external effect.
    ///
    /// # Errors
    ///
    /// Returns typed confirmation, authentication, authority, evidence,
    /// storage, audit, or notification failures.
    #[allow(clippy::too_many_arguments)]
    pub fn confirm<B: ArchiveBoundary>(
        &mut self,
        scope: &mut PrincipalScope<'_>,
        passkeys: &Passkeys,
        access_token: &str,
        csrf_token: &str,
        step_up: &StepUpEvidence,
        confirm_name: &str,
        boundary: &mut B,
        trace: &TraceId,
        now: u64,
    ) -> Result<ArchiveStatus, ArchiveError> {
        if confirm_name != self.record.agent_name {
            return Err(ArchiveError::ConfirmationMismatch);
        }
        if now < self.record.updated_at {
            return Err(ArchiveError::TimeRegressed);
        }
        if self.record.step_up_digest.is_none() {
            match passkeys.authorize(
                scope,
                access_token,
                Some(csrf_token),
                &AuthorizationRequest {
                    operation: OperationClass::AgentArchive,
                    digest: Some(self.operation_digest()),
                    step_up: Some(step_up),
                    intended_destination: "/app/agents",
                },
                now,
            )? {
                AccessDecision::Authorized(_) => {}
                AccessDecision::Reauthenticate { .. } => {
                    return Err(ArchiveError::ReauthenticationRequired)
                }
            }
            self.record.step_up_digest = Some(step_up.confirms().bytes());
            self.record.requested_at = Some(now);
            self.record.phase = StoredPhase::Revoking;
            self.record.updated_at = now;
            persist(scope, &self.record)?;
        }
        if self.record.phase == StoredPhase::Revoking {
            let principal = scope.principal().clone();
            let did =
                Did::new(&self.record.did).map_err(|_| ArchiveError::Corrupt("invalid DID"))?;
            let requested_at = self
                .record
                .requested_at
                .ok_or(ArchiveError::Corrupt("archive request time missing"))?;
            let outcome = boundary.archive_authority(&principal, &did, requested_at)?;
            validate_outcome(&outcome, requested_at)?;
            self.record.suspended_at = Some(outcome.suspended_at);
            self.record.revoked_at = Some(outcome.revoked_at);
            self.record.suspension_receipt_digest = Some(outcome.suspension_receipt_digest);
            self.record.revocation_receipt_digest = Some(outcome.revocation_receipt_digest);
            self.record.phase = StoredPhase::Archived;
            self.record.updated_at = outcome.revoked_at.max(now);
            persist(scope, &self.record)?;
        }
        self.repair_evidence(scope, trace, self.record.updated_at.max(now))?;
        self.status()
    }

    /// Returns the honest irreversible status.
    ///
    /// # Errors
    ///
    /// Refuses corrupt identifiers and contradictory terminal evidence.
    pub fn status(&self) -> Result<ArchiveStatus, ArchiveError> {
        validate_record(&self.record)?;
        Ok(ArchiveStatus {
            agent_id: AgentId::new(self.record.agent_id.clone())?,
            agent_name: self.record.agent_name.clone(),
            stage: match self.record.phase {
                StoredPhase::AwaitingConfirmation => ArchiveStage::AwaitingConfirmation,
                StoredPhase::Revoking => ArchiveStage::StoppingWork,
                StoredPhase::Archived => ArchiveStage::Archived,
            },
            history_entries: self.record.history.len(),
            suspension_receipt_digest: self.record.suspension_receipt_digest,
            revocation_receipt_digest: self.record.revocation_receipt_digest,
        })
    }

    /// Reads retained history and receipt bytes without exposing a write path.
    ///
    /// # Errors
    ///
    /// Refuses pre-archive access and missing or altered retained rows.
    pub fn history(
        &self,
        scope: &PrincipalScope<'_>,
    ) -> Result<Vec<ArchivedHistoryEntry>, ArchiveError> {
        if self.record.phase != StoredPhase::Archived {
            return Err(ArchiveError::NotArchived);
        }
        self.record
            .history
            .iter()
            .map(|stored| {
                let reference = evidence_reference(stored)?;
                let row = scope
                    .get(reference.table(), reference.key())
                    .ok_or(ArchiveError::HistoryMissing)?;
                Ok(ArchivedHistoryEntry {
                    table: reference.table(),
                    key: reference.key().clone(),
                    written_at: row.written_at(),
                    bytes: row.bytes().to_vec(),
                })
            })
            .collect()
    }

    fn repair_evidence(
        &mut self,
        scope: &mut PrincipalScope<'_>,
        trace: &TraceId,
        now: u64,
    ) -> Result<(), ArchiveError> {
        if self.record.phase != StoredPhase::Archived {
            return Ok(());
        }
        if !self.record.notification_written {
            self.record.notification_written = true;
            self.record.updated_at = self.record.updated_at.max(now);
            persist(scope, &self.record)?;
        }
        let row = record_row(self.record.idempotency_key)?;
        let mut archive_evidence = vec![EvidenceRef::new(Table::Journeys, row)];
        for stored in &self.record.history {
            archive_evidence.push(evidence_reference(stored)?);
        }
        let step_up_digest = self
            .record
            .step_up_digest
            .ok_or(ArchiveError::Corrupt("archive step-up evidence missing"))?;
        let mut audit = AuditChain::open(scope)?;
        if !audit.entries(scope)?.iter().any(|entry| {
            matches!(
                entry.event(),
                AuditEvent::SecurityChange {
                    change: SecurityChangeKind::AgentArchive,
                    step_up: AuditStepUpEvidence::Fresh { ceremony_digest },
                } if *ceremony_digest == step_up_digest
            )
        }) {
            audit.append(
                scope,
                now,
                trace,
                &AuditEvent::SecurityChange {
                    change: SecurityChangeKind::AgentArchive,
                    step_up: AuditStepUpEvidence::Fresh {
                        ceremony_digest: step_up_digest,
                    },
                },
                &archive_evidence,
            )?;
        }
        let notification_key = notification_row(self.record.idempotency_key)?;
        let notification = ArchiveNotification {
            class: "security".to_owned(),
            agent_id: self.record.agent_id.clone(),
            message: format!(
                "Agent '{}' was permanently archived. Review this action if you did not make it.",
                self.record.agent_name
            ),
            deep_link: format!("/app/agents/{}", self.record.agent_id),
            action_copy_key: "notification.action.review-agent".to_owned(),
            created_at: self
                .record
                .revoked_at
                .ok_or(ArchiveError::Corrupt("archive revocation time missing"))?,
        };
        if let Some(existing) = scope.get(Table::Notifications, &notification_key) {
            let decoded: ArchiveNotification = serde_json::from_slice(existing.bytes())
                .map_err(|_| ArchiveError::Corrupt("archive notification is invalid"))?;
            if decoded != notification {
                return Err(ArchiveError::Corrupt("archive notification conflicts"));
            }
        } else {
            let bytes = serde_json::to_vec(&notification)
                .map_err(|_| ArchiveError::Corrupt("archive notification cannot encode"))?;
            scope.put(Table::Notifications, notification_key.clone(), now, bytes)?;
        }
        let entries = audit.entries(scope)?;
        if !entries.iter().any(|entry| {
            matches!(
                entry.event(),
                AuditEvent::NotificationDispatch {
                    class: NotificationClass::Security,
                    channel: NotificationChannel::InApp,
                }
            ) && entry.evidence().iter().any(|evidence| {
                evidence.table() == Table::Notifications && evidence.key() == &notification_key
            })
        }) {
            audit.append(
                scope,
                now,
                trace,
                &AuditEvent::NotificationDispatch {
                    class: NotificationClass::Security,
                    channel: NotificationChannel::InApp,
                },
                &[EvidenceRef::new(Table::Notifications, notification_key)],
            )?;
        }
        Ok(())
    }
}

fn validate_request(
    scope: &PrincipalScope<'_>,
    request: &ArchiveRequest,
) -> Result<(), ArchiveError> {
    if request.idempotency_key == [0; 32]
        || request.agent_name.is_empty()
        || request.agent_name.len() > 128
        || request.agent_name.trim() != request.agent_name
        || request.history.is_empty()
    {
        return Err(ArchiveError::InvalidRequest);
    }
    let mut unique = BTreeSet::new();
    for reference in &request.history {
        if !unique.insert((reference.table(), reference.key().clone()))
            || scope.get(reference.table(), reference.key()).is_none()
        {
            return Err(ArchiveError::HistoryMissing);
        }
    }
    Ok(())
}

fn validate_disposition(evidence: &FundsDispositionEvidence) -> Result<(), ArchiveError> {
    if evidence.protocol_state_digest == [0; 32]
        || evidence.observed_sequence == 0
        || evidence.observed_at == 0
        || evidence.verification_level < VerificationLevel::BATCH_INCLUDED
        || evidence.before.is_empty()
        || evidence.after.is_empty()
    {
        return Err(ArchiveError::UnverifiedDisposition);
    }
    let before = balances(&evidence.before)?;
    let after = balances(&evidence.after)?;
    if before.keys().ne(after.keys()) {
        return Err(ArchiveError::UnverifiedDisposition);
    }
    for (asset, amount_after) in after {
        if amount_after
            > *before
                .get(&asset)
                .ok_or(ArchiveError::UnverifiedDisposition)?
        {
            return Err(ArchiveError::UnverifiedDisposition);
        }
    }
    Ok(())
}

fn balances(values: &[AgentBalance]) -> Result<BTreeMap<[u8; 32], u128>, ArchiveError> {
    let mut output = BTreeMap::new();
    for value in values {
        if value.asset.bytes() == [0; 32]
            || output.insert(value.asset.bytes(), value.amount).is_some()
        {
            return Err(ArchiveError::UnverifiedDisposition);
        }
    }
    Ok(output)
}

fn validate_reclaims(
    agent_name: &str,
    before: &[AgentBalance],
    completed: &[ReclaimStatus],
) -> Result<Vec<[u8; 32]>, ArchiveError> {
    let expected = balances(before)?;
    let mut moved = BTreeMap::<[u8; 32], u128>::new();
    let mut receipts = BTreeSet::new();
    for status in completed {
        if status.stage() != ReclaimStage::Done || status.agent() != agent_name {
            return Err(ArchiveError::ReclaimMismatch);
        }
        let result = status.result().ok_or(ArchiveError::ReclaimMismatch)?;
        let total = moved.entry(result.asset()).or_default();
        *total = total
            .checked_add(result.amount())
            .ok_or(ArchiveError::ReclaimMismatch)?;
        if !receipts.insert(result.receipt_digest()) {
            return Err(ArchiveError::ReclaimMismatch);
        }
    }
    let required = expected
        .into_iter()
        .filter(|(_, amount)| *amount != 0)
        .collect::<BTreeMap<_, _>>();
    if moved != required {
        return Err(ArchiveError::ReclaimMismatch);
    }
    Ok(receipts.into_iter().collect())
}

fn disposition_digest(evidence: &FundsDispositionEvidence, receipts: &[[u8; 32]]) -> [u8; 32] {
    let mut digest = Sha256::new();
    digest.update(DIGEST_DOMAIN);
    digest.update(evidence.protocol_state_digest);
    digest.update(evidence.observed_sequence.to_be_bytes());
    digest.update(evidence.observed_at.to_be_bytes());
    for balance in &evidence.before {
        digest.update(balance.asset.bytes());
        digest.update(balance.amount.to_be_bytes());
    }
    for balance in &evidence.after {
        digest.update(balance.asset.bytes());
        digest.update(balance.amount.to_be_bytes());
    }
    for receipt in receipts {
        digest.update(receipt);
    }
    digest.finalize().into()
}

fn validate_outcome(outcome: &RevocationOutcome, requested_at: u64) -> Result<(), ArchiveError> {
    if outcome.reason != RevocationReason::Archived
        || outcome.requested_at != requested_at
        || outcome.suspended_at < requested_at
        || outcome.revoked_at < outcome.suspended_at
        || outcome.latency_seconds != outcome.revoked_at.saturating_sub(outcome.suspended_at)
        || outcome.suspension_receipt_digest == [0; 32]
        || outcome.revocation_receipt_digest == [0; 32]
    {
        return Err(ArchiveError::AuthorityEvidenceMismatch);
    }
    Ok(())
}

fn request_digest(request: &ArchiveRequest) -> [u8; 32] {
    let mut digest = Sha256::new();
    digest.update(DIGEST_DOMAIN);
    digest.update(request.idempotency_key);
    digest.update(request.agent_id.as_str().as_bytes());
    digest.update(request.agent_name.as_bytes());
    digest.update(request.did.as_bytes());
    for reference in &request.history {
        digest.update([table_code(reference.table())]);
        digest.update(reference.key().as_str().as_bytes());
    }
    digest.finalize().into()
}

fn table_code(table: Table) -> u8 {
    match table {
        Table::Journeys => 1,
        Table::Notifications => 2,
        Table::Support => 6,
        Table::Telemetry => 4,
        Table::Cache => 5,
        Table::Stream => 7,
    }
}

fn table_from_code(code: u8) -> Result<Table, ArchiveError> {
    match code {
        1 => Ok(Table::Journeys),
        2 => Ok(Table::Notifications),
        6 => Ok(Table::Support),
        4 => Ok(Table::Telemetry),
        5 => Ok(Table::Cache),
        7 => Ok(Table::Stream),
        _ => Err(ArchiveError::Corrupt("archive evidence table is invalid")),
    }
}

fn stored_reference(reference: &EvidenceRef) -> StoredEvidenceRef {
    StoredEvidenceRef {
        table: table_code(reference.table()),
        key: reference.key().as_str().to_owned(),
    }
}

fn evidence_reference(stored: &StoredEvidenceRef) -> Result<EvidenceRef, ArchiveError> {
    Ok(EvidenceRef::new(
        table_from_code(stored.table)?,
        RowKey::new(stored.key.clone())?,
    ))
}

fn record_row(idempotency_key: [u8; 32]) -> Result<RowKey, StoreError> {
    RowKey::new(format!("{RECORD_PREFIX}{}", hex(&idempotency_key)))
}

fn notification_row(idempotency_key: [u8; 32]) -> Result<RowKey, StoreError> {
    RowKey::new(format!("{NOTIFICATION_PREFIX}{}", hex(&idempotency_key)))
}

fn persist(scope: &mut PrincipalScope<'_>, record: &Record) -> Result<(), ArchiveError> {
    validate_record(record)?;
    let bytes = serde_json::to_vec(record)
        .map_err(|_| ArchiveError::Corrupt("archive record cannot encode"))?;
    scope.put(
        Table::Journeys,
        record_row(record.idempotency_key)?,
        record.updated_at,
        bytes,
    )?;
    Ok(())
}

fn decode(bytes: &[u8]) -> Result<Record, ArchiveError> {
    let record: Record = serde_json::from_slice(bytes)
        .map_err(|_| ArchiveError::Corrupt("archive record is invalid"))?;
    validate_record(&record)?;
    Ok(record)
}

fn validate_record(record: &Record) -> Result<(), ArchiveError> {
    let terminal = record.phase == StoredPhase::Archived;
    if record.version != RECORD_VERSION
        || record.request_digest == [0; 32]
        || record.idempotency_key == [0; 32]
        || AgentId::new(record.agent_id.clone()).is_err()
        || record.agent_name.is_empty()
        || Did::new(&record.did).is_err()
        || record.disposition_digest == [0; 32]
        || record.history.is_empty()
        || record.updated_at < record.created_at
        || matches!(record.phase, StoredPhase::Revoking | StoredPhase::Archived)
            && (record.step_up_digest.is_none() || record.requested_at.is_none())
        || terminal
            && (record.suspended_at.is_none()
                || record.revoked_at.is_none()
                || record.suspension_receipt_digest.is_none()
                || record.revocation_receipt_digest.is_none())
        || !terminal
            && (record.notification_written
                || record.suspended_at.is_some()
                || record.revoked_at.is_some()
                || record.suspension_receipt_digest.is_some()
                || record.revocation_receipt_digest.is_some())
    {
        return Err(ArchiveError::Corrupt("archive invariants are invalid"));
    }
    Ok(())
}

fn hex(bytes: &[u8]) -> String {
    let mut output = String::with_capacity(bytes.len().saturating_mul(2));
    for byte in bytes {
        use std::fmt::Write as _;
        let _ = write!(output, "{byte:02x}");
    }
    output
}

/// Typed archive refusal. No error can present a partially archived agent as
/// complete or offer a resume action.
#[derive(Debug)]
pub enum ArchiveError {
    Store(StoreError),
    Auth(AuthError),
    Audit(AuditError),
    Notify(NotifyError),
    Session(SessionKeyError),
    Agent(AgentContractError),
    FundsRemain(Vec<AgentBalance>),
    InvalidRequest,
    UnverifiedDisposition,
    ReclaimMismatch,
    ConfirmationMismatch,
    ReauthenticationRequired,
    IdempotencyConflict,
    AuthorityEvidenceMismatch,
    TimeRegressed,
    NotArchived,
    HistoryMissing,
    Corrupt(&'static str),
}

impl Display for ArchiveError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Store(error) => write!(formatter, "archive store failure: {error}"),
            Self::Auth(error) => write!(formatter, "archive authentication failure: {error}"),
            Self::Audit(error) => write!(formatter, "archive audit failure: {error}"),
            Self::Notify(error) => write!(formatter, "archive notification failure: {error}"),
            Self::Session(error) => write!(formatter, "archive session failure: {error}"),
            Self::Agent(error) => write!(formatter, "archive agent failure: {error}"),
            Self::FundsRemain(_) => formatter.write_str("archive needs funds disposition"),
            Self::InvalidRequest => formatter.write_str("archive request is invalid"),
            Self::UnverifiedDisposition => {
                formatter.write_str("archive funds disposition is not verified")
            }
            Self::ReclaimMismatch => {
                formatter.write_str("archive reclaim receipts do not dispose the balances")
            }
            Self::ConfirmationMismatch => {
                formatter.write_str("archive typed name does not match the agent")
            }
            Self::ReauthenticationRequired => {
                formatter.write_str("archive requires reauthentication")
            }
            Self::IdempotencyConflict => formatter.write_str("archive retry key conflicts"),
            Self::AuthorityEvidenceMismatch => {
                formatter.write_str("archive authority evidence is invalid")
            }
            Self::TimeRegressed => formatter.write_str("archive time regressed"),
            Self::NotArchived => formatter.write_str("agent history is not archived yet"),
            Self::HistoryMissing => formatter.write_str("archive history evidence is missing"),
            Self::Corrupt(reason) => write!(formatter, "corrupt archive: {reason}"),
        }
    }
}

impl std::error::Error for ArchiveError {}

impl From<StoreError> for ArchiveError {
    fn from(value: StoreError) -> Self {
        Self::Store(value)
    }
}

impl From<AuthError> for ArchiveError {
    fn from(value: AuthError) -> Self {
        Self::Auth(value)
    }
}

impl From<AuditError> for ArchiveError {
    fn from(value: AuditError) -> Self {
        Self::Audit(value)
    }
}

impl From<NotifyError> for ArchiveError {
    fn from(value: NotifyError) -> Self {
        Self::Notify(value)
    }
}

impl From<SessionKeyError> for ArchiveError {
    fn from(value: SessionKeyError) -> Self {
        Self::Session(value)
    }
}

impl From<AgentContractError> for ArchiveError {
    fn from(value: AgentContractError) -> Self {
        Self::Agent(value)
    }
}
