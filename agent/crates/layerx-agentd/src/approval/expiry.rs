//! Durable approval expiry, idempotency and concurrency arbitration.

use std::path::Path;
use std::sync::{Arc, Mutex};

use crate::budget::{self, BudgetLimiter, ReleaseKind};
use crate::policy::approval::{ApprovalSnapshot, ApprovalState};
use crate::store::{ObjectKind, Store, TenantId, TenantKey};

use super::{decision, ApprovalDecision, ApprovalOutcome};

const MAGIC: &[u8; 5] = b"LXAP1";
const KEY_PREFIX: &[u8] = b"approval-v1:";

/// Required idempotency key for one approve or reject operation.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DecisionKey(String);

impl DecisionKey {
    /// Constructs a non-empty bounded idempotency key.
    ///
    /// # Errors
    ///
    /// Refuses empty values, values longer than 255 bytes, and embedded NUL bytes.
    pub fn new(value: impl Into<String>) -> Result<Self, ApprovalExpiryError> {
        let value = value.into();
        if value.is_empty() || value.len() > 255 || value.as_bytes().contains(&0) {
            return Err(ApprovalExpiryError::InvalidIdempotencyKey);
        }
        Ok(Self(value))
    }

    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// File-backed approval state machine used across process restarts.
pub struct ApprovalExpiry {
    store: Arc<Mutex<Store>>,
}

impl ApprovalExpiry {
    pub(crate) fn repeated(
        &self,
        tenant: &TenantId,
        approval_id: [u8; 32],
        idempotency_key: &DecisionKey,
    ) -> Result<Option<ApprovalDecision>, ApprovalExpiryError> {
        let store = self.store.lock().map_err(|_| ApprovalExpiryError::Store)?;
        let key = storage_key(tenant, approval_id)?;
        let Some(value) = store.get(&key) else {
            return Ok(None);
        };
        let persisted = decode(value.bytes())?;
        match (persisted.outcome, persisted.idempotency_key.as_deref()) {
            (Some(outcome), Some(key)) if key == idempotency_key.as_str() => {
                Ok(Some(ApprovalDecision {
                    outcome,
                    submission_ref: persisted.submission_ref,
                    winning_outcome: None,
                    enforcement: super::ApprovalEnforcement::DaemonOnly,
                    authority_notice: super::APPROVAL_ENFORCEMENT_NOTICE,
                }))
            }
            (Some(outcome), _) => Ok(Some(super::conflict(outcome))),
            (None, _) => Ok(None),
        }
    }

    pub(crate) fn persist_prepared_decision(
        &self,
        key: TenantKey,
        bytes: Vec<u8>,
    ) -> Result<(), ApprovalExpiryError> {
        self.store
            .lock()
            .map_err(|_| ApprovalExpiryError::Store)?
            .put_local(key, bytes)
            .map_err(|_| ApprovalExpiryError::Store)
    }

    /// Opens the real daemon store at `root`.
    ///
    /// # Errors
    ///
    /// Returns a storage failure when the store cannot be opened or migrated.
    pub fn open(root: impl AsRef<Path>) -> Result<Self, ApprovalExpiryError> {
        Ok(Self {
            store: Arc::new(Mutex::new(
                Store::open(root).map_err(|_| ApprovalExpiryError::Store)?,
            )),
        })
    }

    /// Uses the daemon's sole durable store owner.
    #[must_use]
    pub fn from_shared_store(store: Arc<Mutex<Store>>) -> Self {
        Self { store }
    }

    pub(crate) fn observe(
        &self,
        snapshot: &ApprovalSnapshot,
        current_sequence: u64,
        limiter: &BudgetLimiter,
    ) -> Result<ApprovalState, ApprovalExpiryError> {
        let mut store = self.store.lock().map_err(|_| ApprovalExpiryError::Store)?;
        let key = storage_key(&snapshot.context.tenant, snapshot.context.request_id)?;
        let mut persisted = match store.get(&key) {
            Some(value) => decode(value.bytes())?,
            None => PersistedApproval::pending(snapshot.expires_at_sequence),
        };
        if persisted.expires_at_sequence != snapshot.expires_at_sequence {
            return Err(ApprovalExpiryError::ExpiryMismatch);
        }
        if persisted.outcome.is_none() && current_sequence >= persisted.expires_at_sequence {
            persisted.outcome = Some(ApprovalOutcome::Expired);
            store
                .put_local(key, encode(&persisted)?)
                .map_err(|_| ApprovalExpiryError::Store)?;
            drop(store);
            budget::release(
                limiter,
                snapshot.context.request_id,
                ReleaseKind::Expired,
                current_sequence,
            )
            .map_err(|_| ApprovalExpiryError::Reservation)?;
            return Ok(ApprovalState::Expired);
        }
        if store.get(&key).is_none() {
            store
                .put_local(key, encode(&persisted)?)
                .map_err(|_| ApprovalExpiryError::Store)?;
        }
        Ok(state_for(persisted.outcome, snapshot.state))
    }

    pub(crate) fn decide(
        &self,
        snapshot: &ApprovalSnapshot,
        current_sequence: u64,
        idempotency_key: &DecisionKey,
        intended: ApprovalOutcome,
        submission_ref: Option<[u8; 32]>,
        limiter: &BudgetLimiter,
    ) -> Result<DecisionResolution, ApprovalExpiryError> {
        let observed = self.observe(snapshot, current_sequence, limiter)?;
        if observed == ApprovalState::Expired {
            return Ok(DecisionResolution::Expired);
        }
        let mut store = self.store.lock().map_err(|_| ApprovalExpiryError::Store)?;
        let key = storage_key(&snapshot.context.tenant, snapshot.context.request_id)?;
        let mut persisted = store
            .get(&key)
            .map(|value| decode(value.bytes()))
            .transpose()?
            .unwrap_or_else(|| PersistedApproval::pending(snapshot.expires_at_sequence));
        if let Some(winner) = persisted.outcome {
            if persisted.idempotency_key.as_deref() == Some(idempotency_key.as_str()) {
                return Ok(DecisionResolution::Repeat(ApprovalDecision {
                    outcome: winner,
                    submission_ref: persisted.submission_ref,
                    winning_outcome: None,
                    enforcement: super::ApprovalEnforcement::DaemonOnly,
                    authority_notice: super::APPROVAL_ENFORCEMENT_NOTICE,
                }));
            }
            return Ok(DecisionResolution::Conflict(winner));
        }
        persisted.idempotency_key = Some(idempotency_key.as_str().to_owned());
        persisted.outcome = Some(intended);
        persisted.submission_ref = submission_ref;
        let bytes = encode(&persisted)?;
        if intended == ApprovalOutcome::Granted {
            return Ok(DecisionResolution::WinnerPrepared(key, bytes));
        }
        store
            .put_local(key, bytes)
            .map_err(|_| ApprovalExpiryError::Store)?;
        Ok(DecisionResolution::Winner)
    }

    /// Recovers a known hold and expires it if its deadline elapsed while offline.
    ///
    /// # Errors
    ///
    /// Returns storage, encoding, expiry-consistency, or reservation-release failures.
    pub fn recover(
        &self,
        tenant: &TenantId,
        approval_id: [u8; 32],
        expires_at_sequence: u64,
        current_sequence: u64,
        limiter: &BudgetLimiter,
    ) -> Result<ApprovalDecision, ApprovalExpiryError> {
        let mut store = self.store.lock().map_err(|_| ApprovalExpiryError::Store)?;
        let key = storage_key(tenant, approval_id)?;
        let Some(value) = store.get(&key) else {
            return Err(ApprovalExpiryError::NotFound);
        };
        let mut persisted = decode(value.bytes())?;
        if persisted.expires_at_sequence != expires_at_sequence {
            return Err(ApprovalExpiryError::ExpiryMismatch);
        }
        if persisted.outcome.is_none() && current_sequence >= expires_at_sequence {
            persisted.outcome = Some(ApprovalOutcome::Expired);
            store
                .put_local(key, encode(&persisted)?)
                .map_err(|_| ApprovalExpiryError::Store)?;
            drop(store);
            budget::release(limiter, approval_id, ReleaseKind::Expired, current_sequence)
                .map_err(|_| ApprovalExpiryError::Reservation)?;
            return Ok(decision(ApprovalOutcome::Expired, None));
        }
        let outcome = persisted.outcome.unwrap_or(ApprovalOutcome::AlreadyDecided);
        Ok(decision(outcome, persisted.submission_ref))
    }
}

pub(crate) enum DecisionResolution {
    Winner,
    WinnerPrepared(TenantKey, Vec<u8>),
    Repeat(ApprovalDecision),
    Conflict(ApprovalOutcome),
    Expired,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ApprovalExpiryError {
    InvalidIdempotencyKey,
    Store,
    Corrupt,
    NotFound,
    ExpiryMismatch,
    Reservation,
}

struct PersistedApproval {
    expires_at_sequence: u64,
    idempotency_key: Option<String>,
    outcome: Option<ApprovalOutcome>,
    submission_ref: Option<[u8; 32]>,
}

impl PersistedApproval {
    const fn pending(expires_at_sequence: u64) -> Self {
        Self {
            expires_at_sequence,
            idempotency_key: None,
            outcome: None,
            submission_ref: None,
        }
    }
}

fn storage_key(tenant: &TenantId, approval_id: [u8; 32]) -> Result<TenantKey, ApprovalExpiryError> {
    let mut object_id = Vec::with_capacity(KEY_PREFIX.len() + approval_id.len());
    object_id.extend_from_slice(KEY_PREFIX);
    object_id.extend_from_slice(&approval_id);
    TenantKey::new(tenant.clone(), ObjectKind::Idempotency, object_id)
        .map_err(|_| ApprovalExpiryError::Store)
}

fn encode(record: &PersistedApproval) -> Result<Vec<u8>, ApprovalExpiryError> {
    let key = record.idempotency_key.as_deref().unwrap_or_default();
    let key_length = u8::try_from(key.len()).map_err(|_| ApprovalExpiryError::Corrupt)?;
    let mut bytes = Vec::with_capacity(MAGIC.len() + 43 + key.len());
    bytes.extend_from_slice(MAGIC);
    bytes.extend_from_slice(&record.expires_at_sequence.to_be_bytes());
    bytes.push(outcome_code(record.outcome));
    bytes.push(key_length);
    bytes.extend_from_slice(key.as_bytes());
    match record.submission_ref {
        Some(reference) => {
            bytes.push(1);
            bytes.extend_from_slice(&reference);
        }
        None => bytes.push(0),
    }
    Ok(bytes)
}

fn decode(bytes: &[u8]) -> Result<PersistedApproval, ApprovalExpiryError> {
    if bytes.len() < MAGIC.len() + 11 || &bytes[..MAGIC.len()] != MAGIC {
        return Err(ApprovalExpiryError::Corrupt);
    }
    let mut offset = MAGIC.len();
    let expires_at_sequence = u64::from_be_bytes(
        bytes[offset..offset + 8]
            .try_into()
            .map_err(|_| ApprovalExpiryError::Corrupt)?,
    );
    offset += 8;
    let outcome = decode_outcome(bytes[offset])?;
    offset += 1;
    let key_length = usize::from(bytes[offset]);
    offset += 1;
    if bytes.len() < offset + key_length + 1 {
        return Err(ApprovalExpiryError::Corrupt);
    }
    let idempotency_key = if key_length == 0 {
        None
    } else {
        let key = std::str::from_utf8(&bytes[offset..offset + key_length])
            .map_err(|_| ApprovalExpiryError::Corrupt)?;
        Some(key.to_owned())
    };
    offset += key_length;
    let submission_ref = match bytes[offset] {
        0 if bytes.len() == offset + 1 => None,
        1 if bytes.len() == offset + 33 => Some(
            bytes[offset + 1..]
                .try_into()
                .map_err(|_| ApprovalExpiryError::Corrupt)?,
        ),
        _ => return Err(ApprovalExpiryError::Corrupt),
    };
    Ok(PersistedApproval {
        expires_at_sequence,
        idempotency_key,
        outcome,
        submission_ref,
    })
}

const fn outcome_code(outcome: Option<ApprovalOutcome>) -> u8 {
    match outcome {
        None => 0,
        Some(ApprovalOutcome::Granted) => 1,
        Some(ApprovalOutcome::Rejected) => 2,
        Some(ApprovalOutcome::Expired) => 3,
        Some(ApprovalOutcome::Defective) => 4,
        Some(ApprovalOutcome::AlreadyDecided) => 5,
        Some(ApprovalOutcome::Conflict) => 6,
    }
}

const fn decode_outcome(code: u8) -> Result<Option<ApprovalOutcome>, ApprovalExpiryError> {
    match code {
        0 => Ok(None),
        1 => Ok(Some(ApprovalOutcome::Granted)),
        2 => Ok(Some(ApprovalOutcome::Rejected)),
        3 => Ok(Some(ApprovalOutcome::Expired)),
        4 => Ok(Some(ApprovalOutcome::Defective)),
        5 => Ok(Some(ApprovalOutcome::AlreadyDecided)),
        6 => Ok(Some(ApprovalOutcome::Conflict)),
        _ => Err(ApprovalExpiryError::Corrupt),
    }
}

const fn state_for(outcome: Option<ApprovalOutcome>, fallback: ApprovalState) -> ApprovalState {
    match outcome {
        Some(ApprovalOutcome::Granted) => ApprovalState::Approved,
        Some(ApprovalOutcome::Rejected) => ApprovalState::Rejected,
        Some(ApprovalOutcome::Expired) => ApprovalState::Expired,
        Some(ApprovalOutcome::Defective) => ApprovalState::Defective,
        _ => fallback,
    }
}
