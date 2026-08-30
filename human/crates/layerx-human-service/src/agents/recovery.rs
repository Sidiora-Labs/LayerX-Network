//! Receipt-verified managed-agent key rotation and recovery journeys.

use std::collections::BTreeSet;
use std::fmt::{Display, Formatter};

use layerx_types::ids::Did;
use layerx_types::verify::VerificationLevel;
use serde::{Deserialize, Serialize};
use sha2::{Digest as _, Sha256};

use crate::audit::{
    AuditChain, AuditError, AuditEvent, SecurityChangeKind, StepUpEvidence as AuditStepUpEvidence,
};
use crate::notify::{AgentId, Dispatcher, Event, EventId, NotifyError};
use crate::store::{EvidenceRef, PrincipalId, PrincipalScope, RowKey, StoreError, Table};
use crate::trace::TraceId;

const RECORD_VERSION: u8 = 1;
const RECORD_PREFIX: &str = "agent-key-change-";
const EVENT_PREFIX: &str = "agent-key-event-";
const REQUEST_DOMAIN: &[u8] = b"layerx-human/agent-key-change-request/v1\0";
const RECEIPT_DOMAIN: &[u8] = b"layerx-human/agent-key-change-receipt/v1\0";
const EVENT_DOMAIN: &[u8] = b"layerx-human/agent-key-change-event/v1\0";

/// Copy key for an ordinary agent-key rotation challenge.
pub const ROTATION_DELAY_COPY_KEY: &str = "agent.keys.rotation.challenge-delay";
/// Copy key for an agent recovery challenge.
pub const RECOVERY_DELAY_COPY_KEY: &str = "agent.keys.recovery.challenge-delay";
/// Copy key explaining that a separate rotation remains open in the protocol.
pub const ROTATION_COMPETITION_COPY_KEY: &str = "agent.keys.competing-rotation-open";

/// Which protocol path is changing the managed agent's primary key.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum AgentKeyChangeKind {
    Rotation,
    Recovery,
}

impl AgentKeyChangeKind {
    const fn code(self) -> u8 {
        match self {
            Self::Rotation => 1,
            Self::Recovery => 2,
        }
    }

    const fn delay_copy_key(self) -> &'static str {
        match self {
            Self::Rotation => ROTATION_DELAY_COPY_KEY,
            Self::Recovery => RECOVERY_DELAY_COPY_KEY,
        }
    }
}

/// Exact protocol delay rendered in a familiar unit without rounding.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ChallengeDelay {
    seconds: u64,
    label: String,
}

impl ChallengeDelay {
    fn new(seconds: u64) -> Result<Self, AgentRecoveryError> {
        if seconds == 0 {
            return Err(AgentRecoveryError::InvalidProtocolEvidence);
        }
        let (value, unit) = if seconds.is_multiple_of(86_400) {
            (seconds / 86_400, "day")
        } else if seconds.is_multiple_of(3_600) {
            (seconds / 3_600, "hour")
        } else if seconds.is_multiple_of(60) {
            (seconds / 60, "minute")
        } else {
            (seconds, "second")
        };
        let suffix = if value == 1 { "" } else { "s" };
        Ok(Self {
            seconds,
            label: format!("{value} {unit}{suffix}"),
        })
    }

    /// Returns the exact protocol delay in seconds.
    #[must_use]
    pub const fn seconds(&self) -> u64 {
        self.seconds
    }
}

impl Display for ChallengeDelay {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(&self.label)
    }
}

/// Honest public stage of a protocol key-change challenge.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum AgentKeyChangeStage {
    ChallengeOpen,
    ReadyToCommit,
    Effective,
    Lapsed,
    Vetoed,
}

impl AgentKeyChangeStage {
    const fn code(self) -> u8 {
        match self {
            Self::ChallengeOpen => 1,
            Self::ReadyToCommit => 2,
            Self::Effective => 3,
            Self::Lapsed => 4,
            Self::Vetoed => 5,
        }
    }
}

/// The state-proven protocol result for the tracked key change.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ProtocolKeyChangeState {
    ChallengeOpen,
    ReadyToCommit,
    Effective,
    Lapsed,
    Vetoed,
}

impl From<ProtocolKeyChangeState> for AgentKeyChangeStage {
    fn from(value: ProtocolKeyChangeState) -> Self {
        match value {
            ProtocolKeyChangeState::ChallengeOpen => Self::ChallengeOpen,
            ProtocolKeyChangeState::ReadyToCommit => Self::ReadyToCommit,
            ProtocolKeyChangeState::Effective => Self::Effective,
            ProtocolKeyChangeState::Lapsed => Self::Lapsed,
            ProtocolKeyChangeState::Vetoed => Self::Vetoed,
        }
    }
}

/// A separate rotation the protocol keeps open while recovery proceeds.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CompetingRotation {
    pub pending_public_key: [u8; 32],
    pub effective_at: u64,
    pub lapse_at: u64,
    pub effective_sequence: u64,
    pub state: ProtocolKeyChangeState,
}

/// Receipt-backed acknowledgement of a protocol rotation or recovery start.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ProtocolKeyChangeEvidence {
    pub kind: AgentKeyChangeKind,
    pub did: Did,
    pub recovery_authority: Did,
    pub previous_public_key: [u8; 32],
    pub pending_public_key: [u8; 32],
    pub effective_at: u64,
    pub lapse_at: u64,
    pub effective_sequence: Option<u64>,
    pub observed_at: u64,
    pub observed_sequence: u64,
    pub verification_level: VerificationLevel,
    pub receipt_digest: [u8; 32],
    pub competing_rotation: Option<CompetingRotation>,
}

impl ProtocolKeyChangeEvidence {
    /// Recomputes the receipt binding the agent layer must return.
    #[must_use]
    pub fn expected_digest(&self) -> [u8; 32] {
        let mut digest = Sha256::new();
        digest.update(RECEIPT_DOMAIN);
        digest.update([self.kind.code()]);
        hash_bytes(&mut digest, self.did.as_bytes());
        hash_bytes(&mut digest, self.recovery_authority.as_bytes());
        digest.update(self.previous_public_key);
        digest.update(self.pending_public_key);
        digest.update(self.effective_at.to_be_bytes());
        digest.update(self.lapse_at.to_be_bytes());
        hash_optional_u64(&mut digest, self.effective_sequence);
        digest.update(self.observed_at.to_be_bytes());
        digest.update(self.observed_sequence.to_be_bytes());
        digest.update([self.verification_level.wire_rank()]);
        hash_competition(&mut digest, self.competing_rotation.as_ref());
        digest.finalize().into()
    }
}

/// State-proven observation used to advance an existing challenge.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ProtocolKeyChangeObservation {
    pub kind: AgentKeyChangeKind,
    pub did: Did,
    pub previous_public_key: [u8; 32],
    pub pending_public_key: [u8; 32],
    pub primary_public_key: [u8; 32],
    pub superseded_public_key: Option<[u8; 32]>,
    pub effective_at: u64,
    pub lapse_at: u64,
    pub effective_sequence: Option<u64>,
    pub state: ProtocolKeyChangeState,
    pub observed_at: u64,
    pub observed_sequence: u64,
    pub verification_level: VerificationLevel,
    pub competing_rotation: Option<CompetingRotation>,
}

/// Immutable service request. The authority digest names the fresh human
/// ceremony and the history references keep the stable agent timeline pinned.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AgentKeyChangeRequest {
    pub idempotency_key: [u8; 32],
    pub agent_id: AgentId,
    pub did: Did,
    pub human_recovery_authority: Did,
    pub authority_evidence_digest: [u8; 32],
    pub history: Vec<EvidenceRef>,
}

/// The exact contract an agent-layer implementation must satisfy.
pub trait AgentRecoveryBoundary {
    /// Starts one idempotent protocol challenge under the registered human
    /// recovery authority.
    ///
    /// # Errors
    ///
    /// Returns a typed protocol refusal or the existing competing rotation.
    fn begin_key_change(
        &mut self,
        principal: &PrincipalId,
        kind: AgentKeyChangeKind,
        request: &AgentKeyChangeRequest,
    ) -> Result<ProtocolKeyChangeEvidence, AgentRecoveryBoundaryError>;

    /// Reads the proof-backed protocol state for an existing challenge.
    ///
    /// # Errors
    ///
    /// Returns a typed boundary failure without advancing local state.
    fn observe_key_change(
        &mut self,
        principal: &PrincipalId,
        kind: AgentKeyChangeKind,
        did: &Did,
    ) -> Result<ProtocolKeyChangeObservation, AgentRecoveryBoundaryError>;
}

/// Stable failures at the agent-layer recovery boundary.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum AgentRecoveryBoundaryError {
    Unavailable,
    Refused(&'static str),
    CompetingRotation(CompetingRotation),
}

impl Display for AgentRecoveryBoundaryError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Unavailable => formatter.write_str("agent recovery contract is unavailable"),
            Self::Refused(reason) => write!(formatter, "agent recovery contract refused: {reason}"),
            Self::CompetingRotation(_) => {
                formatter.write_str("the protocol already has an open key rotation")
            }
        }
    }
}

impl std::error::Error for AgentRecoveryBoundaryError {}

/// Public status preserving the agent identity, history, and protocol facts.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AgentKeyChallenge {
    agent_id: AgentId,
    did: Did,
    kind: AgentKeyChangeKind,
    stage: AgentKeyChangeStage,
    challenge_delay: ChallengeDelay,
    delay_copy_key: &'static str,
    ready_at: u64,
    lapse_at: u64,
    primary_public_key: [u8; 32],
    pending_public_key: [u8; 32],
    superseded_public_key: Option<[u8; 32]>,
    superseded_key_usable_before_sequence: Option<u64>,
    announcement_receipt_digest: [u8; 32],
    verification_level: VerificationLevel,
    observed_at: u64,
    observed_sequence: u64,
    history: Vec<EvidenceRef>,
    competing_rotation: Option<CompetingRotation>,
}

impl AgentKeyChallenge {
    #[must_use]
    pub const fn agent_id(&self) -> &AgentId {
        &self.agent_id
    }

    #[must_use]
    pub const fn did(&self) -> &Did {
        &self.did
    }

    #[must_use]
    pub const fn kind(&self) -> AgentKeyChangeKind {
        self.kind
    }

    #[must_use]
    pub const fn stage(&self) -> AgentKeyChangeStage {
        self.stage
    }

    #[must_use]
    pub const fn challenge_delay(&self) -> &ChallengeDelay {
        &self.challenge_delay
    }

    #[must_use]
    pub const fn delay_copy_key(&self) -> &'static str {
        self.delay_copy_key
    }

    #[must_use]
    pub const fn ready_at(&self) -> u64 {
        self.ready_at
    }

    #[must_use]
    pub const fn lapse_at(&self) -> u64 {
        self.lapse_at
    }

    #[must_use]
    pub const fn primary_public_key(&self) -> [u8; 32] {
        self.primary_public_key
    }

    #[must_use]
    pub const fn pending_public_key(&self) -> [u8; 32] {
        self.pending_public_key
    }

    #[must_use]
    pub const fn superseded_public_key(&self) -> Option<[u8; 32]> {
        self.superseded_public_key
    }

    #[must_use]
    pub const fn superseded_key_usable_before_sequence(&self) -> Option<u64> {
        self.superseded_key_usable_before_sequence
    }

    #[must_use]
    pub const fn announcement_receipt_digest(&self) -> [u8; 32] {
        self.announcement_receipt_digest
    }

    #[must_use]
    pub const fn verification_level(&self) -> VerificationLevel {
        self.verification_level
    }

    #[must_use]
    pub const fn observed_at(&self) -> u64 {
        self.observed_at
    }

    #[must_use]
    pub const fn observed_sequence(&self) -> u64 {
        self.observed_sequence
    }

    #[must_use]
    pub fn history(&self) -> &[EvidenceRef] {
        &self.history
    }

    #[must_use]
    pub const fn competing_rotation(&self) -> Option<&CompetingRotation> {
        self.competing_rotation.as_ref()
    }

    #[must_use]
    pub const fn competition_copy_key(&self) -> Option<&'static str> {
        if self.competing_rotation.is_some() {
            Some(ROTATION_COMPETITION_COPY_KEY)
        } else {
            None
        }
    }
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
enum StoredKind {
    Rotation,
    Recovery,
}

impl From<AgentKeyChangeKind> for StoredKind {
    fn from(value: AgentKeyChangeKind) -> Self {
        match value {
            AgentKeyChangeKind::Rotation => Self::Rotation,
            AgentKeyChangeKind::Recovery => Self::Recovery,
        }
    }
}

impl From<StoredKind> for AgentKeyChangeKind {
    fn from(value: StoredKind) -> Self {
        match value {
            StoredKind::Rotation => Self::Rotation,
            StoredKind::Recovery => Self::Recovery,
        }
    }
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
enum StoredStage {
    ChallengeOpen,
    ReadyToCommit,
    Effective,
    Lapsed,
    Vetoed,
}

impl From<AgentKeyChangeStage> for StoredStage {
    fn from(value: AgentKeyChangeStage) -> Self {
        match value {
            AgentKeyChangeStage::ChallengeOpen => Self::ChallengeOpen,
            AgentKeyChangeStage::ReadyToCommit => Self::ReadyToCommit,
            AgentKeyChangeStage::Effective => Self::Effective,
            AgentKeyChangeStage::Lapsed => Self::Lapsed,
            AgentKeyChangeStage::Vetoed => Self::Vetoed,
        }
    }
}

impl From<StoredStage> for AgentKeyChangeStage {
    fn from(value: StoredStage) -> Self {
        match value {
            StoredStage::ChallengeOpen => Self::ChallengeOpen,
            StoredStage::ReadyToCommit => Self::ReadyToCommit,
            StoredStage::Effective => Self::Effective,
            StoredStage::Lapsed => Self::Lapsed,
            StoredStage::Vetoed => Self::Vetoed,
        }
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
struct StoredCompetition {
    pending_public_key: [u8; 32],
    effective_at: u64,
    lapse_at: u64,
    effective_sequence: u64,
    state: StoredStage,
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
    did: Vec<u8>,
    recovery_authority: Vec<u8>,
    authority_evidence_digest: [u8; 32],
    kind: StoredKind,
    previous_public_key: [u8; 32],
    pending_public_key: [u8; 32],
    primary_public_key: [u8; 32],
    superseded_public_key: Option<[u8; 32]>,
    announced_at: u64,
    challenge_delay_seconds: u64,
    effective_at: u64,
    lapse_at: u64,
    effective_sequence: Option<u64>,
    stage: StoredStage,
    announcement_receipt_digest: [u8; 32],
    verification_rank: u8,
    observed_at: u64,
    observed_sequence: u64,
    history: Vec<StoredEvidenceRef>,
    competing_rotation: Option<StoredCompetition>,
    created_at: u64,
    updated_at: u64,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
struct EventRecord {
    version: u8,
    agent_id: String,
    did: Vec<u8>,
    kind: StoredKind,
    stage: StoredStage,
    announcement_receipt_digest: [u8; 32],
    competing_rotation: Option<StoredCompetition>,
}

/// Durable managed-agent rotation and recovery service.
pub struct AgentRecovery<B: AgentRecoveryBoundary> {
    boundary: B,
}

impl<B: AgentRecoveryBoundary> AgentRecovery<B> {
    #[must_use]
    pub const fn new(boundary: B) -> Self {
        Self { boundary }
    }

    #[must_use]
    pub const fn boundary(&self) -> &B {
        &self.boundary
    }

    #[must_use]
    pub const fn boundary_mut(&mut self) -> &mut B {
        &mut self.boundary
    }

    /// Starts or resumes an ordinary primary-key rotation.
    ///
    /// # Errors
    ///
    /// Refuses invalid history, conflicting retry identity, unverified
    /// protocol evidence, or a protocol-declared competing rotation.
    pub fn rotate(
        &mut self,
        scope: &mut PrincipalScope<'_>,
        request: &AgentKeyChangeRequest,
        trace: &TraceId,
        now: u64,
    ) -> Result<AgentKeyChallenge, AgentRecoveryError> {
        self.start(scope, AgentKeyChangeKind::Rotation, request, trace, now)
    }

    /// Starts or resumes recovery under the registered human authority.
    ///
    /// # Errors
    ///
    /// Refuses invalid history, conflicting retry identity, unverified
    /// protocol evidence, or a protocol refusal.
    pub fn recover(
        &mut self,
        scope: &mut PrincipalScope<'_>,
        request: &AgentKeyChangeRequest,
        trace: &TraceId,
        now: u64,
    ) -> Result<AgentKeyChallenge, AgentRecoveryError> {
        self.start(scope, AgentKeyChangeKind::Recovery, request, trace, now)
    }

    /// Reconciles one stored challenge exclusively from state-proven protocol
    /// facts and emits any newly observed security event exactly once.
    ///
    /// # Errors
    ///
    /// Refuses missing, corrupt, regressed, or inconsistent observations.
    pub fn reconcile(
        &mut self,
        scope: &mut PrincipalScope<'_>,
        idempotency_key: [u8; 32],
        trace: &TraceId,
        now: u64,
    ) -> Result<AgentKeyChallenge, AgentRecoveryError> {
        let row = record_row(idempotency_key)?;
        let stored = scope
            .get(Table::Journeys, &row)
            .ok_or(AgentRecoveryError::NotFound)?;
        let mut record = decode(stored.bytes())?;
        if now < record.updated_at {
            return Err(AgentRecoveryError::TimeRegressed);
        }
        let did =
            Did::new(&record.did).map_err(|_| AgentRecoveryError::Corrupt("invalid agent DID"))?;
        let kind = AgentKeyChangeKind::from(record.kind);
        let principal = scope.principal().clone();
        let observation = self.boundary.observe_key_change(&principal, kind, &did)?;
        validate_observation(&record, &observation)?;
        record.primary_public_key = observation.primary_public_key;
        record.superseded_public_key = observation.superseded_public_key;
        record.stage = AgentKeyChangeStage::from(observation.state).into();
        record.verification_rank = observation.verification_level.wire_rank();
        record.observed_at = observation.observed_at;
        record.observed_sequence = observation.observed_sequence;
        record.competing_rotation = observation
            .competing_rotation
            .as_ref()
            .map(stored_competition);
        record.updated_at = now.max(observation.observed_at);
        persist(scope, &record)?;
        Self::repair_event(scope, &record, trace, record.updated_at)?;
        status(&record)
    }

    /// Loads a challenge without consulting or changing protocol state.
    ///
    /// # Errors
    ///
    /// Refuses corrupt durable state.
    pub fn load(
        scope: &PrincipalScope<'_>,
        idempotency_key: [u8; 32],
    ) -> Result<Option<AgentKeyChallenge>, AgentRecoveryError> {
        scope
            .get(Table::Journeys, &record_row(idempotency_key)?)
            .map_or(Ok(None), |row| status(&decode(row.bytes())?).map(Some))
    }

    fn start(
        &mut self,
        scope: &mut PrincipalScope<'_>,
        kind: AgentKeyChangeKind,
        request: &AgentKeyChangeRequest,
        trace: &TraceId,
        now: u64,
    ) -> Result<AgentKeyChallenge, AgentRecoveryError> {
        validate_request(scope, request)?;
        let request_digest = request_digest(kind, request);
        let row = record_row(request.idempotency_key)?;
        if let Some(existing) = scope.get(Table::Journeys, &row) {
            let record = decode(existing.bytes())?;
            if record.request_digest != request_digest || record.kind != kind.into() {
                return Err(AgentRecoveryError::IdempotencyConflict);
            }
            Self::repair_event(scope, &record, trace, now.max(record.updated_at))?;
            return status(&record);
        }
        let principal = scope.principal().clone();
        let evidence = self.boundary.begin_key_change(&principal, kind, request)?;
        validate_start(kind, request, &evidence)?;
        let record = Record {
            version: RECORD_VERSION,
            request_digest,
            idempotency_key: request.idempotency_key,
            agent_id: request.agent_id.as_str().to_owned(),
            did: request.did.as_bytes().to_vec(),
            recovery_authority: request.human_recovery_authority.as_bytes().to_vec(),
            authority_evidence_digest: request.authority_evidence_digest,
            kind: kind.into(),
            previous_public_key: evidence.previous_public_key,
            pending_public_key: evidence.pending_public_key,
            primary_public_key: evidence.previous_public_key,
            superseded_public_key: None,
            announced_at: evidence.observed_at,
            challenge_delay_seconds: evidence.effective_at.saturating_sub(evidence.observed_at),
            effective_at: evidence.effective_at,
            lapse_at: evidence.lapse_at,
            effective_sequence: evidence.effective_sequence,
            stage: StoredStage::ChallengeOpen,
            announcement_receipt_digest: evidence.receipt_digest,
            verification_rank: evidence.verification_level.wire_rank(),
            observed_at: evidence.observed_at,
            observed_sequence: evidence.observed_sequence,
            history: request.history.iter().map(stored_reference).collect(),
            competing_rotation: evidence.competing_rotation.as_ref().map(stored_competition),
            created_at: now,
            updated_at: now.max(evidence.observed_at),
        };
        persist(scope, &record)?;
        Self::repair_event(scope, &record, trace, record.updated_at)?;
        status(&record)
    }

    fn repair_event(
        scope: &mut PrincipalScope<'_>,
        record: &Record,
        trace: &TraceId,
        now: u64,
    ) -> Result<(), AgentRecoveryError> {
        let digest = event_digest(record);
        let event_row = event_row(digest)?;
        let event_record = EventRecord {
            version: RECORD_VERSION,
            agent_id: record.agent_id.clone(),
            did: record.did.clone(),
            kind: record.kind,
            stage: record.stage,
            announcement_receipt_digest: record.announcement_receipt_digest,
            competing_rotation: record.competing_rotation.clone(),
        };
        put_exact(
            scope,
            Table::Journeys,
            event_row.clone(),
            now,
            serde_json::to_vec(&event_record)
                .map_err(|_| AgentRecoveryError::Corrupt("key event cannot encode"))?,
        )?;
        let event_reference = EvidenceRef::new(Table::Journeys, event_row);
        let mut evidence = vec![event_reference.clone()];
        evidence.extend(
            record
                .history
                .iter()
                .map(evidence_reference)
                .collect::<Result<Vec<_>, _>>()?,
        );
        let mut audit = AuditChain::open(scope)?;
        let already_audited = audit.entries(scope)?.iter().any(|entry| {
            entry.evidence().iter().any(|binding| {
                binding.table() == event_reference.table() && binding.key() == event_reference.key()
            })
        });
        if !already_audited {
            audit.append(
                scope,
                now,
                trace,
                &AuditEvent::SecurityChange {
                    change: match AgentKeyChangeKind::from(record.kind) {
                        AgentKeyChangeKind::Rotation => SecurityChangeKind::KeyRotation,
                        AgentKeyChangeKind::Recovery => SecurityChangeKind::RecoveryInitiated,
                    },
                    step_up: AuditStepUpEvidence::Fresh {
                        ceremony_digest: record.authority_evidence_digest,
                    },
                },
                &evidence,
            )?;
        }
        let event_id = EventId::new(format!("evt_{}", hex(digest)))?;
        let event = match AgentKeyChangeKind::from(record.kind) {
            AgentKeyChangeKind::Rotation => Event::SecurityKeyRotation { event_id },
            AgentKeyChangeKind::Recovery => Event::SecurityRecovery { event_id },
        };
        Dispatcher::dispatch(scope, &mut audit, now, trace, &event)?;
        Ok(())
    }
}

fn validate_request(
    scope: &PrincipalScope<'_>,
    request: &AgentKeyChangeRequest,
) -> Result<(), AgentRecoveryError> {
    if request.idempotency_key == [0; 32]
        || request.authority_evidence_digest == [0; 32]
        || request.did == request.human_recovery_authority
        || request.history.is_empty()
    {
        return Err(AgentRecoveryError::InvalidRequest);
    }
    let mut unique = BTreeSet::new();
    for reference in &request.history {
        if !unique.insert((reference.table(), reference.key().clone()))
            || scope.get(reference.table(), reference.key()).is_none()
        {
            return Err(AgentRecoveryError::HistoryMissing);
        }
    }
    Ok(())
}

fn validate_start(
    kind: AgentKeyChangeKind,
    request: &AgentKeyChangeRequest,
    evidence: &ProtocolKeyChangeEvidence,
) -> Result<(), AgentRecoveryError> {
    let delay = evidence
        .effective_at
        .checked_sub(evidence.observed_at)
        .ok_or(AgentRecoveryError::InvalidProtocolEvidence)?;
    let lapse = evidence
        .lapse_at
        .checked_sub(evidence.effective_at)
        .ok_or(AgentRecoveryError::InvalidProtocolEvidence)?;
    ChallengeDelay::new(delay)?;
    if evidence.kind != kind
        || evidence.did != request.did
        || evidence.recovery_authority != request.human_recovery_authority
        || evidence.previous_public_key == [0; 32]
        || evidence.pending_public_key == [0; 32]
        || evidence.previous_public_key == evidence.pending_public_key
        || delay != lapse
        || evidence.observed_sequence == 0
        || evidence.verification_level < VerificationLevel::BATCH_INCLUDED
        || evidence.receipt_digest != evidence.expected_digest()
        || (kind == AgentKeyChangeKind::Rotation
            && evidence
                .effective_sequence
                .is_none_or(|sequence| sequence == 0))
        || (kind == AgentKeyChangeKind::Recovery && evidence.effective_sequence.is_some())
    {
        return Err(AgentRecoveryError::InvalidProtocolEvidence);
    }
    if let Some(competing) = &evidence.competing_rotation {
        validate_competition(competing, evidence.observed_at)?;
    }
    Ok(())
}

fn validate_observation(
    record: &Record,
    observation: &ProtocolKeyChangeObservation,
) -> Result<(), AgentRecoveryError> {
    let kind = AgentKeyChangeKind::from(record.kind);
    let next_stage = AgentKeyChangeStage::from(observation.state);
    if observation.kind != kind
        || observation.did.as_bytes() != record.did
        || observation.previous_public_key != record.previous_public_key
        || observation.pending_public_key != record.pending_public_key
        || observation.effective_at != record.effective_at
        || observation.lapse_at != record.lapse_at
        || observation.effective_sequence != record.effective_sequence
        || observation.observed_at < record.observed_at
        || observation.observed_sequence < record.observed_sequence
        || observation.verification_level < VerificationLevel::STATE_PROVEN
        || observation.verification_level < verification_level(record.verification_rank)?
        || !valid_transition(AgentKeyChangeStage::from(record.stage), next_stage)
        || (kind == AgentKeyChangeKind::Rotation
            && observation.state == ProtocolKeyChangeState::Vetoed)
    {
        return Err(AgentRecoveryError::InvalidProtocolObservation);
    }
    match observation.state {
        ProtocolKeyChangeState::ChallengeOpen | ProtocolKeyChangeState::ReadyToCommit => {
            if observation.primary_public_key != record.previous_public_key
                || observation.superseded_public_key.is_some()
            {
                return Err(AgentRecoveryError::InvalidProtocolObservation);
            }
        }
        ProtocolKeyChangeState::Effective => {
            if observation.primary_public_key != record.pending_public_key
                || observation.superseded_public_key != Some(record.previous_public_key)
            {
                return Err(AgentRecoveryError::InvalidProtocolObservation);
            }
        }
        ProtocolKeyChangeState::Lapsed | ProtocolKeyChangeState::Vetoed => {
            if observation.primary_public_key != record.previous_public_key {
                return Err(AgentRecoveryError::InvalidProtocolObservation);
            }
        }
    }
    if let Some(competing) = &observation.competing_rotation {
        validate_competition(competing, observation.observed_at)?;
    }
    Ok(())
}

fn validate_competition(
    competing: &CompetingRotation,
    observed_at: u64,
) -> Result<(), AgentRecoveryError> {
    if competing.pending_public_key == [0; 32]
        || competing.effective_at == 0
        || competing.lapse_at <= competing.effective_at
        || competing.effective_sequence == 0
        || (matches!(competing.state, ProtocolKeyChangeState::ChallengeOpen)
            && observed_at >= competing.effective_at)
        || (matches!(competing.state, ProtocolKeyChangeState::ReadyToCommit)
            && (observed_at < competing.effective_at || observed_at > competing.lapse_at))
    {
        return Err(AgentRecoveryError::InvalidProtocolObservation);
    }
    Ok(())
}

fn status(record: &Record) -> Result<AgentKeyChallenge, AgentRecoveryError> {
    validate_record(record)?;
    let kind = AgentKeyChangeKind::from(record.kind);
    Ok(AgentKeyChallenge {
        agent_id: AgentId::new(record.agent_id.clone())?,
        did: Did::new(&record.did).map_err(|_| AgentRecoveryError::Corrupt("invalid agent DID"))?,
        kind,
        stage: AgentKeyChangeStage::from(record.stage),
        challenge_delay: ChallengeDelay::new(record.challenge_delay_seconds)?,
        delay_copy_key: kind.delay_copy_key(),
        ready_at: record.effective_at,
        lapse_at: record.lapse_at,
        primary_public_key: record.primary_public_key,
        pending_public_key: record.pending_public_key,
        superseded_public_key: record.superseded_public_key,
        superseded_key_usable_before_sequence: (kind == AgentKeyChangeKind::Rotation)
            .then_some(record.effective_sequence)
            .flatten(),
        announcement_receipt_digest: record.announcement_receipt_digest,
        verification_level: verification_level(record.verification_rank)?,
        observed_at: record.observed_at,
        observed_sequence: record.observed_sequence,
        history: record
            .history
            .iter()
            .map(evidence_reference)
            .collect::<Result<Vec<_>, _>>()?,
        competing_rotation: record.competing_rotation.as_ref().map(competing_rotation),
    })
}

fn validate_record(record: &Record) -> Result<(), AgentRecoveryError> {
    let kind = AgentKeyChangeKind::from(record.kind);
    if record.version != RECORD_VERSION
        || record.request_digest == [0; 32]
        || record.idempotency_key == [0; 32]
        || record.authority_evidence_digest == [0; 32]
        || record.previous_public_key == [0; 32]
        || record.pending_public_key == [0; 32]
        || record.previous_public_key == record.pending_public_key
        || record.challenge_delay_seconds == 0
        || record.effective_at
            != record
                .announced_at
                .saturating_add(record.challenge_delay_seconds)
        || record.lapse_at
            != record
                .effective_at
                .saturating_add(record.challenge_delay_seconds)
        || record.observed_sequence == 0
        || record.announcement_receipt_digest == [0; 32]
        || record.history.is_empty()
        || record.updated_at < record.created_at
        || (kind == AgentKeyChangeKind::Rotation
            && record
                .effective_sequence
                .is_none_or(|sequence| sequence == 0))
        || (kind == AgentKeyChangeKind::Recovery && record.effective_sequence.is_some())
    {
        return Err(AgentRecoveryError::Corrupt("invalid key-change record"));
    }
    let level = verification_level(record.verification_rank)?;
    if level < VerificationLevel::BATCH_INCLUDED {
        return Err(AgentRecoveryError::Corrupt(
            "key-change evidence is not verified",
        ));
    }
    let stage = AgentKeyChangeStage::from(record.stage);
    match stage {
        AgentKeyChangeStage::ChallengeOpen | AgentKeyChangeStage::ReadyToCommit => {
            if record.primary_public_key != record.previous_public_key
                || record.superseded_public_key.is_some()
            {
                return Err(AgentRecoveryError::Corrupt(
                    "open key-change state is inconsistent",
                ));
            }
        }
        AgentKeyChangeStage::Effective => {
            if record.primary_public_key != record.pending_public_key
                || record.superseded_public_key != Some(record.previous_public_key)
            {
                return Err(AgentRecoveryError::Corrupt(
                    "effective key-change state is inconsistent",
                ));
            }
        }
        AgentKeyChangeStage::Lapsed | AgentKeyChangeStage::Vetoed => {
            if record.primary_public_key != record.previous_public_key {
                return Err(AgentRecoveryError::Corrupt(
                    "closed key-change state is inconsistent",
                ));
            }
        }
    }
    let history = record
        .history
        .iter()
        .map(evidence_reference)
        .collect::<Result<Vec<_>, _>>()?;
    let request = AgentKeyChangeRequest {
        idempotency_key: record.idempotency_key,
        agent_id: AgentId::new(record.agent_id.clone())?,
        did: Did::new(&record.did).map_err(|_| AgentRecoveryError::Corrupt("invalid agent DID"))?,
        human_recovery_authority: Did::new(&record.recovery_authority)
            .map_err(|_| AgentRecoveryError::Corrupt("invalid recovery authority DID"))?,
        authority_evidence_digest: record.authority_evidence_digest,
        history,
    };
    if request.did == request.human_recovery_authority
        || request_digest(kind, &request) != record.request_digest
    {
        return Err(AgentRecoveryError::Corrupt(
            "key-change request binding is inconsistent",
        ));
    }
    if let Some(competing) = record.competing_rotation.as_ref().map(competing_rotation) {
        validate_competition(&competing, record.observed_at)?;
    }
    Ok(())
}

const fn valid_transition(from: AgentKeyChangeStage, to: AgentKeyChangeStage) -> bool {
    match from {
        AgentKeyChangeStage::ChallengeOpen => true,
        AgentKeyChangeStage::ReadyToCommit => !matches!(to, AgentKeyChangeStage::ChallengeOpen),
        AgentKeyChangeStage::Effective => matches!(to, AgentKeyChangeStage::Effective),
        AgentKeyChangeStage::Lapsed => matches!(to, AgentKeyChangeStage::Lapsed),
        AgentKeyChangeStage::Vetoed => matches!(to, AgentKeyChangeStage::Vetoed),
    }
}

fn request_digest(kind: AgentKeyChangeKind, request: &AgentKeyChangeRequest) -> [u8; 32] {
    let mut digest = Sha256::new();
    digest.update(REQUEST_DOMAIN);
    digest.update([kind.code()]);
    digest.update(request.idempotency_key);
    hash_bytes(&mut digest, request.agent_id.as_str().as_bytes());
    hash_bytes(&mut digest, request.did.as_bytes());
    hash_bytes(&mut digest, request.human_recovery_authority.as_bytes());
    digest.update(request.authority_evidence_digest);
    for reference in &request.history {
        digest.update([table_code(reference.table())]);
        hash_bytes(&mut digest, reference.key().as_str().as_bytes());
    }
    digest.finalize().into()
}

fn event_digest(record: &Record) -> [u8; 32] {
    let mut digest = Sha256::new();
    digest.update(EVENT_DOMAIN);
    digest.update(record.request_digest);
    digest.update([AgentKeyChangeKind::from(record.kind).code()]);
    digest.update([AgentKeyChangeStage::from(record.stage).code()]);
    hash_stored_competition(&mut digest, record.competing_rotation.as_ref());
    digest.finalize().into()
}

fn persist(scope: &mut PrincipalScope<'_>, record: &Record) -> Result<(), AgentRecoveryError> {
    scope.put(
        Table::Journeys,
        record_row(record.idempotency_key)?,
        record.updated_at,
        serde_json::to_vec(record)
            .map_err(|_| AgentRecoveryError::Corrupt("key-change record cannot encode"))?,
    )?;
    Ok(())
}

fn decode(bytes: &[u8]) -> Result<Record, AgentRecoveryError> {
    let record: Record = serde_json::from_slice(bytes)
        .map_err(|_| AgentRecoveryError::Corrupt("key-change record cannot decode"))?;
    validate_record(&record)?;
    Ok(record)
}

fn put_exact(
    scope: &mut PrincipalScope<'_>,
    table: Table,
    key: RowKey,
    now: u64,
    bytes: Vec<u8>,
) -> Result<(), AgentRecoveryError> {
    if let Some(existing) = scope.get(table, &key) {
        if existing.bytes() != bytes {
            return Err(AgentRecoveryError::Corrupt("key event conflicts"));
        }
        return Ok(());
    }
    scope.put(table, key, now, bytes)?;
    Ok(())
}

fn record_row(idempotency_key: [u8; 32]) -> Result<RowKey, AgentRecoveryError> {
    Ok(RowKey::new(format!(
        "{RECORD_PREFIX}{}",
        hex(idempotency_key)
    ))?)
}

fn event_row(digest: [u8; 32]) -> Result<RowKey, AgentRecoveryError> {
    Ok(RowKey::new(format!("{EVENT_PREFIX}{}", hex(digest)))?)
}

fn stored_reference(reference: &EvidenceRef) -> StoredEvidenceRef {
    StoredEvidenceRef {
        table: table_code(reference.table()),
        key: reference.key().as_str().to_owned(),
    }
}

fn evidence_reference(stored: &StoredEvidenceRef) -> Result<EvidenceRef, AgentRecoveryError> {
    Ok(EvidenceRef::new(
        table_from_code(stored.table)?,
        RowKey::new(stored.key.clone())?,
    ))
}

const fn table_code(table: Table) -> u8 {
    match table {
        Table::Journeys => 1,
        Table::Notifications => 2,
        Table::Support => 6,
        Table::Telemetry => 4,
        Table::Cache => 5,
        Table::Stream => 7,
    }
}

fn table_from_code(code: u8) -> Result<Table, AgentRecoveryError> {
    match code {
        1 => Ok(Table::Journeys),
        2 => Ok(Table::Notifications),
        6 => Ok(Table::Support),
        4 => Ok(Table::Telemetry),
        5 => Ok(Table::Cache),
        7 => Ok(Table::Stream),
        _ => Err(AgentRecoveryError::Corrupt("invalid evidence table")),
    }
}

fn stored_competition(value: &CompetingRotation) -> StoredCompetition {
    StoredCompetition {
        pending_public_key: value.pending_public_key,
        effective_at: value.effective_at,
        lapse_at: value.lapse_at,
        effective_sequence: value.effective_sequence,
        state: AgentKeyChangeStage::from(value.state).into(),
    }
}

fn competing_rotation(value: &StoredCompetition) -> CompetingRotation {
    CompetingRotation {
        pending_public_key: value.pending_public_key,
        effective_at: value.effective_at,
        lapse_at: value.lapse_at,
        effective_sequence: value.effective_sequence,
        state: match AgentKeyChangeStage::from(value.state) {
            AgentKeyChangeStage::ChallengeOpen => ProtocolKeyChangeState::ChallengeOpen,
            AgentKeyChangeStage::ReadyToCommit => ProtocolKeyChangeState::ReadyToCommit,
            AgentKeyChangeStage::Effective => ProtocolKeyChangeState::Effective,
            AgentKeyChangeStage::Lapsed => ProtocolKeyChangeState::Lapsed,
            AgentKeyChangeStage::Vetoed => ProtocolKeyChangeState::Vetoed,
        },
    }
}

fn verification_level(rank: u8) -> Result<VerificationLevel, AgentRecoveryError> {
    match rank {
        0 => Ok(VerificationLevel::UNVERIFIED),
        1 => Ok(VerificationLevel::SEQUENCER_SIGNED),
        2 => Ok(VerificationLevel::BATCH_INCLUDED),
        3 => Ok(VerificationLevel::STATE_PROVEN),
        4 => Ok(VerificationLevel::CHECKPOINT_FINALISED),
        5 => Ok(VerificationLevel::SETTLEMENT_ANCHORED),
        _ => Err(AgentRecoveryError::Corrupt("invalid verification rank")),
    }
}

fn hash_bytes(digest: &mut Sha256, bytes: &[u8]) {
    digest.update(u64::try_from(bytes.len()).unwrap_or(u64::MAX).to_be_bytes());
    digest.update(bytes);
}

fn hash_optional_u64(digest: &mut Sha256, value: Option<u64>) {
    match value {
        Some(value) => {
            digest.update([1]);
            digest.update(value.to_be_bytes());
        }
        None => digest.update([0]),
    }
}

fn hash_competition(digest: &mut Sha256, value: Option<&CompetingRotation>) {
    match value {
        Some(value) => {
            digest.update([1]);
            digest.update(value.pending_public_key);
            digest.update(value.effective_at.to_be_bytes());
            digest.update(value.lapse_at.to_be_bytes());
            digest.update(value.effective_sequence.to_be_bytes());
            digest.update([AgentKeyChangeStage::from(value.state).code()]);
        }
        None => digest.update([0]),
    }
}

fn hash_stored_competition(digest: &mut Sha256, value: Option<&StoredCompetition>) {
    match value {
        Some(value) => {
            digest.update([1]);
            digest.update(value.pending_public_key);
            digest.update(value.effective_at.to_be_bytes());
            digest.update(value.lapse_at.to_be_bytes());
            digest.update(value.effective_sequence.to_be_bytes());
            digest.update([AgentKeyChangeStage::from(value.state).code()]);
        }
        None => digest.update([0]),
    }
}

fn hex(bytes: [u8; 32]) -> String {
    use std::fmt::Write as _;

    let mut output = String::with_capacity(64);
    for byte in bytes {
        let _ = write!(&mut output, "{byte:02x}");
    }
    output
}

/// Typed managed-agent key-change failure.
#[derive(Debug)]
pub enum AgentRecoveryError {
    InvalidRequest,
    HistoryMissing,
    IdempotencyConflict,
    NotFound,
    TimeRegressed,
    InvalidProtocolEvidence,
    InvalidProtocolObservation,
    Corrupt(&'static str),
    Boundary(AgentRecoveryBoundaryError),
    Store(StoreError),
    Audit(AuditError),
    Notify(NotifyError),
}

impl Display for AgentRecoveryError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::InvalidRequest => formatter.write_str("invalid agent key-change request"),
            Self::HistoryMissing => formatter.write_str("agent history evidence is missing"),
            Self::IdempotencyConflict => {
                formatter.write_str("agent key-change retry identity conflicts")
            }
            Self::NotFound => formatter.write_str("agent key-change journey was not found"),
            Self::TimeRegressed => formatter.write_str("agent key-change time regressed"),
            Self::InvalidProtocolEvidence => {
                formatter.write_str("agent key-change evidence did not verify")
            }
            Self::InvalidProtocolObservation => {
                formatter.write_str("agent key-change observation did not verify")
            }
            Self::Corrupt(reason) => write!(formatter, "corrupt agent key-change state: {reason}"),
            Self::Boundary(error) => write!(formatter, "{error}"),
            Self::Store(error) => write!(formatter, "agent key-change store failure: {error}"),
            Self::Audit(error) => write!(formatter, "agent key-change audit failure: {error}"),
            Self::Notify(error) => {
                write!(formatter, "agent key-change notification failure: {error}")
            }
        }
    }
}

impl std::error::Error for AgentRecoveryError {}

impl From<AgentRecoveryBoundaryError> for AgentRecoveryError {
    fn from(value: AgentRecoveryBoundaryError) -> Self {
        Self::Boundary(value)
    }
}

impl From<StoreError> for AgentRecoveryError {
    fn from(value: StoreError) -> Self {
        Self::Store(value)
    }
}

impl From<AuditError> for AgentRecoveryError {
    fn from(value: AuditError) -> Self {
        Self::Audit(value)
    }
}

impl From<NotifyError> for AgentRecoveryError {
    fn from(value: NotifyError) -> Self {
        Self::Notify(value)
    }
}
