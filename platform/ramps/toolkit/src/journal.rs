use std::collections::BTreeMap;
use std::fs::{File, OpenOptions};
use std::io::{BufRead as _, BufReader, Read as _, Write as _};
use std::path::Path;

use serde::{Deserialize, Serialize};
use sha2::{Digest as _, Sha256};

use crate::{AggregateStatus, RampError, RampOrder, RampPresentation, EXTERNAL_CUSTODY_LABEL};

const JOURNAL_DOMAIN: &[u8] = b"LXP/market-maker-ramp/journal/v1\0";
const MAX_JOURNAL_RECORD_BYTES: usize = 4 * 1024 * 1024;

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum WorkflowStage {
    CompliancePending,
    ManualReview,
    ComplianceRefused,
    AwaitingExternalCredit,
    AwaitingLayerxPayment,
    ProviderSubmissionPlanned,
    ProviderSubmittedUnknown,
    ProviderPending,
    ProviderSettled,
    ProviderRefused,
    ProviderReversed,
    LayerxSubmissionPlanned,
    LayerxSubmittedUnknown,
    LayerxPending,
    LayerxRefused,
    LayerxVerified,
    ReversalPending,
    Reversed,
    Done,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct TransitionEvidence {
    pub provider_operation_id: Option<String>,
    pub provider_evidence_digest: Option<[u8; 32]>,
    pub activity_id: Option<[u8; 32]>,
    pub canonical_activity: Option<Vec<u8>>,
    pub receipt_digest: Option<[u8; 32]>,
    pub refusal_code: Option<String>,
    pub retry_at: Option<u64>,
}

impl TransitionEvidence {
    #[must_use]
    pub const fn empty() -> Self {
        Self {
            provider_operation_id: None,
            provider_evidence_digest: None,
            activity_id: None,
            canonical_activity: None,
            receipt_digest: None,
            refusal_code: None,
            retry_at: None,
        }
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum Event {
    OrderCreated {
        order: RampOrder,
    },
    LeaseAcquired {
        order_digest: [u8; 32],
        worker_id: String,
        expires_at: u64,
    },
    Transition {
        order_digest: [u8; 32],
        expected: WorkflowStage,
        next: WorkflowStage,
        evidence: TransitionEvidence,
    },
    ProviderCallbackApplied {
        order_digest: [u8; 32],
        callback_id: String,
        provider_sequence: u64,
        evidence_digest: [u8; 32],
        expected: WorkflowStage,
        next: WorkflowStage,
        evidence: TransitionEvidence,
    },
    PaxeerPlanned {
        idempotency_key: [u8; 32],
        asset: [u8; 32],
        amount: u128,
    },
    PaxeerObserved {
        idempotency_key: [u8; 32],
        operation_id: String,
        transaction_hash: [u8; 32],
        stage: String,
        block_hash: Option<[u8; 32]>,
        confirmations: u64,
    },
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
struct RecordBody {
    sequence: u64,
    previous_hash: [u8; 32],
    recorded_at: u64,
    event: Event,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
struct Record {
    sequence: u64,
    previous_hash: [u8; 32],
    recorded_at: u64,
    event: Event,
    record_hash: [u8; 32],
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct OrderSnapshot {
    pub order: RampOrder,
    pub stage: WorkflowStage,
    pub evidence: TransitionEvidence,
    pub lease: Option<(String, u64)>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PaxeerSnapshot {
    pub idempotency_key: [u8; 32],
    pub asset: [u8; 32],
    pub amount: u128,
    pub operation_id: Option<String>,
    pub transaction_hash: Option<[u8; 32]>,
    pub stage: String,
    pub block_hash: Option<[u8; 32]>,
    pub confirmations: u64,
}

impl OrderSnapshot {
    #[must_use]
    pub fn presentation(&self) -> RampPresentation {
        let status = match self.stage {
            WorkflowStage::ManualReview => AggregateStatus::ManualReview,
            WorkflowStage::ComplianceRefused
            | WorkflowStage::ProviderRefused
            | WorkflowStage::LayerxRefused => AggregateStatus::Refused,
            WorkflowStage::ProviderSubmittedUnknown | WorkflowStage::LayerxSubmittedUnknown => {
                AggregateStatus::Unknown
            }
            WorkflowStage::ProviderSubmissionPlanned | WorkflowStage::LayerxSubmissionPlanned => {
                AggregateStatus::Unknown
            }
            WorkflowStage::ProviderReversed
            | WorkflowStage::ReversalPending
            | WorkflowStage::Reversed => AggregateStatus::Reversed,
            WorkflowStage::Done => AggregateStatus::Done,
            WorkflowStage::CompliancePending
            | WorkflowStage::AwaitingExternalCredit
            | WorkflowStage::AwaitingLayerxPayment
            | WorkflowStage::ProviderPending
            | WorkflowStage::ProviderSettled
            | WorkflowStage::LayerxPending
            | WorkflowStage::LayerxVerified => AggregateStatus::Pending,
        };
        let refusal_code = if matches!(
            status,
            AggregateStatus::Refused | AggregateStatus::ManualReview | AggregateStatus::Reversed
        ) {
            self.evidence.refusal_code.clone()
        } else {
            None
        };
        let retry_at = if matches!(status, AggregateStatus::Pending | AggregateStatus::Unknown) {
            self.evidence.retry_at
        } else {
            None
        };
        RampPresentation {
            external_custody_label: EXTERNAL_CUSTODY_LABEL,
            status,
            order_digest: self.order.order_digest,
            activity_id: self.evidence.activity_id,
            receipt_digest: self.evidence.receipt_digest,
            provider_evidence_digest: self.evidence.provider_evidence_digest,
            refusal_code,
            retry_at,
        }
    }
}

pub struct Journal {
    _lock: LockClaim,
    file: File,
    next_sequence: u64,
    head: [u8; 32],
    orders: BTreeMap<[u8; 32], OrderSnapshot>,
    order_ids: BTreeMap<String, [u8; 32]>,
    callbacks: BTreeMap<String, ([u8; 32], u64, [u8; 32])>,
    provider_sequences: BTreeMap<[u8; 32], u64>,
    paxeer: BTreeMap<[u8; 32], PaxeerSnapshot>,
}

impl Journal {
    pub fn open(path: impl AsRef<Path>) -> Result<Self, RampError> {
        let path = path.as_ref();
        let lock = LockClaim::acquire(path)?;
        let reader = open_journal(path)?;
        require_private_file(&reader)?;
        let mut journal = Self {
            _lock: lock,
            file: reader.try_clone().map_err(|_| RampError::Journal)?,
            next_sequence: 0,
            head: [0; 32],
            orders: BTreeMap::new(),
            order_ids: BTreeMap::new(),
            callbacks: BTreeMap::new(),
            provider_sequences: BTreeMap::new(),
            paxeer: BTreeMap::new(),
        };
        let mut reader = BufReader::new(reader);
        loop {
            let mut line = Vec::new();
            let read = reader
                .by_ref()
                .take((MAX_JOURNAL_RECORD_BYTES + 1) as u64)
                .read_until(b'\n', &mut line)
                .map_err(|_| RampError::Journal)?;
            if read == 0 {
                break;
            }
            if line.len() > MAX_JOURNAL_RECORD_BYTES || line.last() != Some(&b'\n') {
                return Err(RampError::Journal);
            }
            line.pop();
            if line.is_empty() {
                return Err(RampError::Journal);
            }
            let record: Record = serde_json::from_slice(&line).map_err(|_| RampError::Journal)?;
            if record.sequence != journal.next_sequence || record.previous_hash != journal.head {
                return Err(RampError::Journal);
            }
            let body = RecordBody {
                sequence: record.sequence,
                previous_hash: record.previous_hash,
                recorded_at: record.recorded_at,
                event: record.event.clone(),
            };
            if record.record_hash != record_digest(&body)? {
                return Err(RampError::Journal);
            }
            journal.apply(&record.event)?;
            journal.next_sequence = journal
                .next_sequence
                .checked_add(1)
                .ok_or(RampError::Journal)?;
            journal.head = record.record_hash;
        }
        Ok(journal)
    }

    pub fn create_order(&mut self, order: RampOrder, now: u64) -> Result<OrderSnapshot, RampError> {
        order.validate_bound()?;
        if let Some(existing) = self.order_ids.get(&order.order_id) {
            return if *existing == order.order_digest {
                self.orders.get(existing).cloned().ok_or(RampError::Journal)
            } else {
                Err(RampError::Conflict)
            };
        }
        self.append(
            Event::OrderCreated {
                order: order.clone(),
            },
            now,
        )?;
        self.orders
            .get(&order.order_digest)
            .cloned()
            .ok_or(RampError::Journal)
    }

    #[must_use]
    pub fn order(&self, digest: &[u8; 32]) -> Option<&OrderSnapshot> {
        self.orders.get(digest)
    }

    #[must_use]
    pub fn order_by_id(&self, order_id: &str) -> Option<&OrderSnapshot> {
        self.order_ids
            .get(order_id)
            .and_then(|digest| self.orders.get(digest))
    }

    #[must_use]
    pub fn orders(&self) -> Vec<OrderSnapshot> {
        self.orders.values().cloned().collect()
    }

    pub fn acquire_lease(
        &mut self,
        order_digest: [u8; 32],
        worker_id: &str,
        now: u64,
        lease_seconds: u64,
    ) -> Result<(), RampError> {
        if !safe_identifier(worker_id) || lease_seconds == 0 {
            return Err(RampError::InvalidOrder);
        }
        let snapshot = self
            .orders
            .get(&order_digest)
            .ok_or(RampError::InvalidOrder)?;
        if snapshot
            .lease
            .as_ref()
            .is_some_and(|(owner, expiry)| *expiry > now && owner != worker_id)
        {
            return Err(RampError::LeaseHeld);
        }
        self.append(
            Event::LeaseAcquired {
                order_digest,
                worker_id: worker_id.to_owned(),
                expires_at: now.saturating_add(lease_seconds),
            },
            now,
        )
    }

    pub fn transition(
        &mut self,
        order_digest: [u8; 32],
        expected: WorkflowStage,
        next: WorkflowStage,
        evidence: TransitionEvidence,
        worker_id: &str,
        now: u64,
    ) -> Result<(), RampError> {
        if !allowed(expected, next) {
            return Err(RampError::IllegalTransition);
        }
        let snapshot = self
            .orders
            .get(&order_digest)
            .ok_or(RampError::InvalidOrder)?;
        if snapshot.stage != expected {
            return Err(RampError::Conflict);
        }
        if !snapshot
            .lease
            .as_ref()
            .is_some_and(|(owner, expires_at)| owner == worker_id && *expires_at > now)
        {
            return Err(RampError::LeaseHeld);
        }
        validate_resulting_evidence(next, &snapshot.evidence, &evidence)?;
        if evidence_conflicts(&snapshot.evidence, &evidence) {
            return Err(RampError::Conflict);
        }
        if next == WorkflowStage::Done {
            let mut complete = snapshot.evidence.clone();
            merge_evidence(&mut complete, &evidence);
            if completion_missing(&complete) {
                return Err(RampError::IllegalTransition);
            }
        }
        self.append(
            Event::Transition {
                order_digest,
                expected,
                next,
                evidence,
            },
            now,
        )
    }

    pub fn apply_provider_callback(
        &mut self,
        order_digest: [u8; 32],
        callback_id: &str,
        provider_sequence: u64,
        evidence_digest: [u8; 32],
        expected: WorkflowStage,
        next: WorkflowStage,
        evidence: TransitionEvidence,
        now: u64,
    ) -> Result<bool, RampError> {
        if !safe_identifier(callback_id) || provider_sequence == 0 || evidence_digest == [0; 32] {
            return Err(RampError::Provider);
        }
        if let Some(existing) = self.callbacks.get(callback_id) {
            return if existing == &(order_digest, provider_sequence, evidence_digest) {
                Ok(false)
            } else {
                Err(RampError::Conflict)
            };
        }
        if self
            .provider_sequences
            .get(&order_digest)
            .is_some_and(|latest| provider_sequence <= *latest)
        {
            return Err(RampError::Conflict);
        }
        if !allowed(expected, next) {
            return Err(RampError::IllegalTransition);
        }
        let snapshot = self
            .orders
            .get(&order_digest)
            .ok_or(RampError::InvalidOrder)?;
        if snapshot.stage != expected {
            return Err(RampError::Conflict);
        }
        validate_resulting_evidence(next, &snapshot.evidence, &evidence)?;
        if evidence_conflicts(&snapshot.evidence, &evidence) {
            return Err(RampError::Conflict);
        }
        self.append(
            Event::ProviderCallbackApplied {
                order_digest,
                callback_id: callback_id.to_owned(),
                provider_sequence,
                evidence_digest,
                expected,
                next,
                evidence,
            },
            now,
        )?;
        Ok(true)
    }

    pub fn observe_paxeer(
        &mut self,
        idempotency_key: [u8; 32],
        operation_id: &str,
        transaction_hash: [u8; 32],
        stage: &str,
        block_hash: Option<[u8; 32]>,
        confirmations: u64,
        now: u64,
    ) -> Result<(), RampError> {
        if !safe_identifier(operation_id) || transaction_hash == [0; 32] || !safe_identifier(stage)
        {
            return Err(RampError::Paxeer);
        }
        let existing = self.paxeer.get(&idempotency_key).ok_or(RampError::Paxeer)?;
        if existing
            .operation_id
            .as_ref()
            .is_some_and(|value| value != operation_id)
            || existing
                .transaction_hash
                .is_some_and(|value| value != transaction_hash)
        {
            return Err(RampError::Conflict);
        }
        self.append(
            Event::PaxeerObserved {
                idempotency_key,
                operation_id: operation_id.to_owned(),
                transaction_hash,
                stage: stage.to_owned(),
                block_hash,
                confirmations,
            },
            now,
        )
    }

    pub fn plan_paxeer(
        &mut self,
        idempotency_key: [u8; 32],
        asset: [u8; 32],
        amount: u128,
        now: u64,
    ) -> Result<(), RampError> {
        if idempotency_key == [0; 32] || asset == [0; 32] || amount == 0 {
            return Err(RampError::Paxeer);
        }
        if let Some(existing) = self.paxeer.get(&idempotency_key) {
            return if existing.asset == asset && existing.amount == amount {
                Ok(())
            } else {
                Err(RampError::Conflict)
            };
        }
        self.append(
            Event::PaxeerPlanned {
                idempotency_key,
                asset,
                amount,
            },
            now,
        )
    }

    #[must_use]
    pub fn paxeer(&self, idempotency_key: &[u8; 32]) -> Option<&PaxeerSnapshot> {
        self.paxeer.get(idempotency_key)
    }

    fn append(&mut self, event: Event, recorded_at: u64) -> Result<(), RampError> {
        let next_sequence = self
            .next_sequence
            .checked_add(1)
            .ok_or(RampError::Journal)?;
        let body = RecordBody {
            sequence: self.next_sequence,
            previous_hash: self.head,
            recorded_at,
            event: event.clone(),
        };
        let record_hash = record_digest(&body)?;
        let record = Record {
            sequence: body.sequence,
            previous_hash: body.previous_hash,
            recorded_at: body.recorded_at,
            event: body.event,
            record_hash,
        };
        let mut bytes = serde_json::to_vec(&record).map_err(|_| RampError::Journal)?;
        if bytes.len().saturating_add(1) > MAX_JOURNAL_RECORD_BYTES {
            return Err(RampError::Journal);
        }
        bytes.push(b'\n');
        self.file
            .write_all(&bytes)
            .map_err(|_| RampError::Journal)?;
        self.file.sync_data().map_err(|_| RampError::Journal)?;
        self.apply(&event)?;
        self.next_sequence = next_sequence;
        self.head = record_hash;
        Ok(())
    }

    fn apply(&mut self, event: &Event) -> Result<(), RampError> {
        match event {
            Event::OrderCreated { order } => {
                order.validate_bound().map_err(|_| RampError::Journal)?;
                if self.orders.contains_key(&order.order_digest)
                    || self.order_ids.contains_key(&order.order_id)
                {
                    return Err(RampError::Journal);
                }
                self.order_ids
                    .insert(order.order_id.clone(), order.order_digest);
                self.orders.insert(
                    order.order_digest,
                    OrderSnapshot {
                        order: order.clone(),
                        stage: WorkflowStage::CompliancePending,
                        evidence: TransitionEvidence::empty(),
                        lease: None,
                    },
                );
            }
            Event::LeaseAcquired {
                order_digest,
                worker_id,
                expires_at,
            } => {
                if !safe_identifier(worker_id) || *expires_at == 0 {
                    return Err(RampError::Journal);
                }
                let snapshot = self
                    .orders
                    .get_mut(order_digest)
                    .ok_or(RampError::Journal)?;
                snapshot.lease = Some((worker_id.clone(), *expires_at));
            }
            Event::Transition {
                order_digest,
                expected,
                next,
                evidence,
            } => {
                let snapshot = self
                    .orders
                    .get_mut(order_digest)
                    .ok_or(RampError::Journal)?;
                if snapshot.stage != *expected
                    || !allowed(*expected, *next)
                    || validate_resulting_evidence(*next, &snapshot.evidence, evidence).is_err()
                    || evidence_conflicts(&snapshot.evidence, evidence)
                {
                    return Err(RampError::Journal);
                }
                if *next == WorkflowStage::Done {
                    let mut complete = snapshot.evidence.clone();
                    merge_evidence(&mut complete, evidence);
                    if completion_missing(&complete) {
                        return Err(RampError::Journal);
                    }
                }
                snapshot.stage = *next;
                merge_evidence(&mut snapshot.evidence, evidence);
            }
            Event::ProviderCallbackApplied {
                order_digest,
                callback_id,
                provider_sequence,
                evidence_digest,
                expected,
                next,
                evidence,
            } => {
                if !safe_identifier(callback_id)
                    || *provider_sequence == 0
                    || *evidence_digest == [0; 32]
                {
                    return Err(RampError::Journal);
                }
                if !self.orders.contains_key(order_digest)
                    || self
                        .callbacks
                        .insert(
                            callback_id.clone(),
                            (*order_digest, *provider_sequence, *evidence_digest),
                        )
                        .is_some()
                {
                    return Err(RampError::Journal);
                }
                if self
                    .provider_sequences
                    .insert(*order_digest, *provider_sequence)
                    .is_some_and(|previous| previous >= *provider_sequence)
                {
                    return Err(RampError::Journal);
                }
                let snapshot = self
                    .orders
                    .get_mut(order_digest)
                    .ok_or(RampError::Journal)?;
                if snapshot.stage != *expected
                    || !allowed(*expected, *next)
                    || validate_resulting_evidence(*next, &snapshot.evidence, evidence).is_err()
                    || evidence_conflicts(&snapshot.evidence, evidence)
                {
                    return Err(RampError::Journal);
                }
                snapshot.stage = *next;
                merge_evidence(&mut snapshot.evidence, evidence);
            }
            Event::PaxeerPlanned {
                idempotency_key,
                asset,
                amount,
            } => {
                if *idempotency_key == [0; 32] || *asset == [0; 32] || *amount == 0 {
                    return Err(RampError::Journal);
                }
                if self
                    .paxeer
                    .insert(
                        *idempotency_key,
                        PaxeerSnapshot {
                            idempotency_key: *idempotency_key,
                            asset: *asset,
                            amount: *amount,
                            operation_id: None,
                            transaction_hash: None,
                            stage: "submission_planned".to_owned(),
                            block_hash: None,
                            confirmations: 0,
                        },
                    )
                    .is_some()
                {
                    return Err(RampError::Journal);
                }
            }
            Event::PaxeerObserved {
                idempotency_key,
                operation_id,
                transaction_hash,
                stage,
                block_hash,
                confirmations,
            } => {
                if !safe_identifier(operation_id)
                    || !safe_identifier(stage)
                    || *transaction_hash == [0; 32]
                {
                    return Err(RampError::Journal);
                }
                let snapshot = self
                    .paxeer
                    .get_mut(idempotency_key)
                    .ok_or(RampError::Journal)?;
                if snapshot
                    .operation_id
                    .as_ref()
                    .is_some_and(|value| value != operation_id)
                    || snapshot
                        .transaction_hash
                        .is_some_and(|value| value != *transaction_hash)
                {
                    return Err(RampError::Journal);
                }
                snapshot.operation_id = Some(operation_id.clone());
                snapshot.transaction_hash = Some(*transaction_hash);
                snapshot.stage.clone_from(stage);
                snapshot.block_hash = *block_hash;
                snapshot.confirmations = *confirmations;
            }
        }
        Ok(())
    }
}

struct LockClaim {
    path: Option<std::path::PathBuf>,
    _file: File,
}

impl LockClaim {
    fn acquire(journal: &Path) -> Result<Self, RampError> {
        let mut path = journal.as_os_str().to_owned();
        path.push(".writer-lock");
        let path = std::path::PathBuf::from(path);
        let file = claim_lock(&path)?;
        file.sync_data().map_err(|_| RampError::Journal)?;
        Ok(Self {
            path: if cfg!(unix) { None } else { Some(path) },
            _file: file,
        })
    }
}

impl Drop for LockClaim {
    fn drop(&mut self) {
        if let Some(path) = &self.path {
            if let Err(error) = std::fs::remove_file(path) {
                eprintln!("ramp journal writer lock retained: {error}");
            }
        }
    }
}

#[cfg(unix)]
fn claim_lock(path: &Path) -> Result<File, RampError> {
    use std::os::unix::fs::OpenOptionsExt as _;
    let file = OpenOptions::new()
        .create(true)
        .read(true)
        .write(true)
        .mode(0o600)
        .open(path)
        .map_err(|_| RampError::Journal)?;
    require_private_file(&file)?;
    rustix::fs::flock(&file, rustix::fs::FlockOperation::NonBlockingLockExclusive)
        .map_err(|_| RampError::Journal)?;
    Ok(file)
}

#[cfg(not(unix))]
fn claim_lock(path: &Path) -> Result<File, RampError> {
    OpenOptions::new()
        .create_new(true)
        .write(true)
        .open(path)
        .map_err(|_| RampError::Journal)
}

#[cfg(unix)]
fn open_journal(path: &Path) -> Result<File, RampError> {
    use std::os::unix::fs::OpenOptionsExt as _;
    OpenOptions::new()
        .create(true)
        .read(true)
        .append(true)
        .mode(0o600)
        .open(path)
        .map_err(|_| RampError::Journal)
}

#[cfg(not(unix))]
fn open_journal(path: &Path) -> Result<File, RampError> {
    OpenOptions::new()
        .create(true)
        .read(true)
        .append(true)
        .open(path)
        .map_err(|_| RampError::Journal)
}

fn record_digest(body: &RecordBody) -> Result<[u8; 32], RampError> {
    let bytes = serde_json::to_vec(body).map_err(|_| RampError::Journal)?;
    let mut hasher = Sha256::new();
    hasher.update(JOURNAL_DOMAIN);
    hasher.update(bytes);
    Ok(hasher.finalize().into())
}

fn merge_evidence(target: &mut TransitionEvidence, value: &TransitionEvidence) {
    if value.provider_operation_id.is_some() {
        target
            .provider_operation_id
            .clone_from(&value.provider_operation_id);
    }
    if value.provider_evidence_digest.is_some() {
        target.provider_evidence_digest = value.provider_evidence_digest;
    }
    if value.activity_id.is_some() {
        target.activity_id = value.activity_id;
    }
    if value.canonical_activity.is_some() {
        target
            .canonical_activity
            .clone_from(&value.canonical_activity);
    }
    if value.receipt_digest.is_some() {
        target.receipt_digest = value.receipt_digest;
    }
    if value.refusal_code.is_some() {
        target.refusal_code.clone_from(&value.refusal_code);
    }
    if value.retry_at.is_some() {
        target.retry_at = value.retry_at;
    }
}

fn evidence_conflicts(existing: &TransitionEvidence, incoming: &TransitionEvidence) -> bool {
    existing
        .provider_operation_id
        .as_ref()
        .zip(incoming.provider_operation_id.as_ref())
        .is_some_and(|(current, next)| current != next && !current.starts_with("idempotency:"))
        || existing
            .activity_id
            .zip(incoming.activity_id)
            .is_some_and(|(current, next)| current != next)
        || existing
            .canonical_activity
            .as_ref()
            .zip(incoming.canonical_activity.as_ref())
            .is_some_and(|(current, next)| current != next)
        || existing
            .receipt_digest
            .zip(incoming.receipt_digest)
            .is_some_and(|(current, next)| current != next)
}

fn completion_missing(evidence: &TransitionEvidence) -> bool {
    evidence.activity_id.is_none()
        || evidence.canonical_activity.is_none()
        || evidence.receipt_digest.is_none()
        || evidence.provider_operation_id.is_none()
        || evidence.provider_evidence_digest.is_none()
}

fn safe_identifier(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 128
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.' | b':'))
}

fn validate_resulting_evidence(
    stage: WorkflowStage,
    existing: &TransitionEvidence,
    incoming: &TransitionEvidence,
) -> Result<(), RampError> {
    let mut evidence = existing.clone();
    merge_evidence(&mut evidence, incoming);
    let provider_operation = evidence
        .provider_operation_id
        .as_deref()
        .is_some_and(safe_identifier);
    let provider_digest = evidence
        .provider_evidence_digest
        .is_some_and(|value| value != [0; 32]);
    let activity = evidence.activity_id.is_some_and(|value| value != [0; 32]);
    let canonical_activity = evidence
        .canonical_activity
        .as_ref()
        .is_some_and(|value| !value.is_empty() && value.len() <= 1024 * 1024);
    let receipt = evidence
        .receipt_digest
        .is_some_and(|value| value != [0; 32]);
    let refusal = evidence
        .refusal_code
        .as_deref()
        .is_some_and(safe_identifier);
    let valid = match stage {
        WorkflowStage::ProviderSubmissionPlanned
        | WorkflowStage::ProviderSubmittedUnknown
        | WorkflowStage::ProviderPending => {
            provider_operation && evidence.retry_at.is_some_and(|retry| retry != 0)
        }
        WorkflowStage::ProviderSettled | WorkflowStage::ProviderReversed => {
            provider_operation && provider_digest
        }
        WorkflowStage::ProviderRefused => provider_operation && refusal,
        WorkflowStage::LayerxSubmittedUnknown | WorkflowStage::LayerxPending => {
            activity && canonical_activity
        }
        WorkflowStage::LayerxSubmissionPlanned => activity && canonical_activity,
        WorkflowStage::LayerxVerified | WorkflowStage::Done => {
            activity && canonical_activity && receipt
        }
        WorkflowStage::ComplianceRefused | WorkflowStage::LayerxRefused => refusal,
        _ => true,
    };
    if valid {
        Ok(())
    } else {
        Err(RampError::IllegalTransition)
    }
}

const fn allowed(from: WorkflowStage, to: WorkflowStage) -> bool {
    use WorkflowStage as S;
    matches!(
        (from, to),
        (
            S::CompliancePending,
            S::ManualReview
                | S::ComplianceRefused
                | S::AwaitingExternalCredit
                | S::AwaitingLayerxPayment
        ) | (
            S::ManualReview,
            S::ComplianceRefused
                | S::AwaitingExternalCredit
                | S::AwaitingLayerxPayment
                | S::ProviderSubmittedUnknown
                | S::ProviderPending
                | S::ProviderSettled
                | S::ProviderRefused
                | S::ProviderReversed
                | S::ManualReview
        ) | (
            S::AwaitingExternalCredit,
            S::ProviderSubmissionPlanned
                | S::ProviderPending
                | S::ProviderSettled
                | S::ProviderRefused
        ) | (
            S::ProviderSubmissionPlanned,
            S::ProviderSubmittedUnknown
                | S::ProviderPending
                | S::ProviderSettled
                | S::ProviderRefused
                | S::ManualReview
        ) | (
            S::ProviderSubmittedUnknown,
            S::ProviderSubmittedUnknown
                | S::ProviderPending
                | S::ProviderSettled
                | S::ProviderRefused
                | S::ProviderReversed
                | S::ManualReview
        ) | (
            S::ProviderPending,
            S::ProviderPending
                | S::ProviderSettled
                | S::ProviderRefused
                | S::ProviderReversed
                | S::ManualReview
        ) | (
            S::ProviderSettled,
            S::LayerxSubmissionPlanned | S::LayerxPending | S::ProviderReversed
        ) | (
            S::AwaitingLayerxPayment,
            S::LayerxSubmissionPlanned | S::LayerxPending | S::LayerxVerified | S::LayerxRefused
        ) | (
            S::LayerxSubmissionPlanned,
            S::LayerxSubmittedUnknown | S::LayerxPending | S::LayerxVerified | S::LayerxRefused
        ) | (
            S::LayerxSubmittedUnknown,
            S::LayerxSubmittedUnknown | S::LayerxPending | S::LayerxVerified | S::LayerxRefused
        ) | (
            S::LayerxPending,
            S::LayerxPending | S::LayerxVerified | S::LayerxRefused
        ) | (
            S::LayerxVerified,
            S::ProviderSubmissionPlanned
                | S::ProviderPending
                | S::ProviderSettled
                | S::ProviderRefused
                | S::Done
        ) | (S::ProviderSettled, S::Done)
            | (S::Done, S::ProviderReversed | S::ReversalPending)
            | (S::ProviderReversed, S::ReversalPending | S::Reversed)
            | (S::ReversalPending, S::ReversalPending | S::Reversed)
    )
}

#[cfg(unix)]
fn require_private_file(file: &File) -> Result<(), RampError> {
    use std::os::unix::fs::PermissionsExt as _;
    if file
        .metadata()
        .map_err(|_| RampError::Journal)?
        .permissions()
        .mode()
        & 0o077
        != 0
    {
        return Err(RampError::Journal);
    }
    Ok(())
}

#[cfg(not(unix))]
fn require_private_file(_file: &File) -> Result<(), RampError> {
    Ok(())
}
