//! Receipt-only resolution for submissions whose transport outcome is unknown.

use sha2::{Digest, Sha256};

use crate::protocol_evidence::{EvidenceAuthority, RawReceiptEvidence};
use crate::store::{ObjectKind, Store, StoreError, TenantId, TenantKey};

use super::{Outbox, OutboxError, SubmissionState};

const STATE_MAGIC: &[u8; 4] = b"LXUR";
const INITIAL_BACKOFF_MS: u64 = 1_000;
const MAX_BACKOFF_MS: u64 = 60_000;

/// Caller-clocked age and retry state for one unresolved submission.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct UnknownAge {
    pub first_observed_at_ms: u64,
    pub age_ms: u64,
    pub attempt_count: u32,
    pub next_attempt_at_ms: u64,
}

/// A stable failure classification for receipt lookup and exact-byte resend.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum UnknownBoundaryError {
    Unavailable,
    InvalidResponse,
}

/// The only boundary operations unknown-state resolution is permitted to use.
pub trait ReceiptLookup {
    /// Looks up terminal receipt evidence already recorded under one idempotency key.
    ///
    /// # Errors
    ///
    /// Returns `Unavailable` when the node cannot be reached and `InvalidResponse` for a reply that
    /// is not decodable receipt evidence; a reachable node holding no receipt returns `Ok(None)`.
    fn receipt_by_idempotency_key(
        &mut self,
        idempotency_key: [u8; 32],
    ) -> Result<Option<RawReceiptEvidence>, UnknownBoundaryError>;

    /// Resends the stored signed bytes unchanged under their original idempotency key.
    ///
    /// # Errors
    ///
    /// Returns `Unavailable` when the node cannot be reached and `InvalidResponse` for an unusable
    /// reply; both leave the send indeterminate, so neither may be read as a refusal.
    fn resend_exact(
        &mut self,
        idempotency_key: [u8; 32],
        signed_canonical_bytes: &[u8],
    ) -> Result<(), UnknownBoundaryError>;
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ResolutionObservation {
    Backoff,
    ReceiptMissing,
    LookupUnavailable,
    UnverifiedReceipt,
    ExecutedReceipt,
    FailedReceipt,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ResendObservation {
    NotWarranted,
    Sent,
    Indeterminate,
}

/// Operator-visible result of one resolution pass.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct UnknownResolution {
    pub state: SubmissionState,
    pub age: UnknownAge,
    pub observation: ResolutionObservation,
    pub resend: ResendObservation,
}

#[derive(Debug)]
pub enum UnknownResolutionError {
    Outbox(OutboxError),
    Store(StoreError),
    NotUnknown,
    TimeRegressed,
    Arithmetic,
    Corrupt,
}

impl From<OutboxError> for UnknownResolutionError {
    fn from(value: OutboxError) -> Self {
        Self::Outbox(value)
    }
}

impl From<StoreError> for UnknownResolutionError {
    fn from(value: StoreError) -> Self {
        Self::Store(value)
    }
}

/// Resolves an unknown submission solely from a receipt lookup by idempotency key.
///
/// # Errors
///
/// Returns `Outbox(NotFound)` for an untracked submission and `NotUnknown` for a record in any
/// other state, `TimeRegressed`, `Arithmetic` or `Corrupt` from the persisted backoff record, and
/// `Store` for failed durable writes; lookup and resend failures surface as observations instead.
pub fn resolve_unknown<B: ReceiptLookup>(
    outbox: &mut Outbox,
    store: &mut Store,
    submission_id: [u8; 32],
    observed_at_ms: u64,
    verifier: &EvidenceAuthority,
    boundary: &mut B,
) -> Result<UnknownResolution, UnknownResolutionError> {
    let record = outbox
        .records
        .get(&submission_id)
        .cloned()
        .ok_or(OutboxError::NotFound)?;
    if record.status.state != SubmissionState::Unknown {
        return Err(UnknownResolutionError::NotUnknown);
    }

    let key = state_key(record.tenant.clone(), submission_id)?;
    let mut age = match store.get(&key) {
        Some(value) => decode_state(value.bytes(), observed_at_ms)?,
        None => UnknownAge {
            first_observed_at_ms: observed_at_ms,
            age_ms: 0,
            attempt_count: 0,
            next_attempt_at_ms: observed_at_ms,
        },
    };
    if observed_at_ms < age.first_observed_at_ms {
        return Err(UnknownResolutionError::TimeRegressed);
    }
    age.age_ms = observed_at_ms - age.first_observed_at_ms;
    if observed_at_ms < age.next_attempt_at_ms {
        return Ok(UnknownResolution {
            state: SubmissionState::Unknown,
            age,
            observation: ResolutionObservation::Backoff,
            resend: ResendObservation::NotWarranted,
        });
    }

    age.attempt_count = age
        .attempt_count
        .checked_add(1)
        .ok_or(UnknownResolutionError::Arithmetic)?;
    age.next_attempt_at_ms = observed_at_ms
        .checked_add(backoff_ms(submission_id, age.attempt_count))
        .ok_or(UnknownResolutionError::Arithmetic)?;
    store.put_local(key.clone(), encode_state(age))?;

    let Ok(receipt) = boundary.receipt_by_idempotency_key(submission_id) else {
        return Ok(UnknownResolution {
            state: SubmissionState::Unknown,
            age,
            observation: ResolutionObservation::LookupUnavailable,
            resend: ResendObservation::NotWarranted,
        });
    };

    if let Some(receipt) = receipt {
        let verified_receipt = match verifier.verify_receipt(&receipt) {
            Ok(candidate) if candidate.activity_id() == record.status.activity_id => candidate,
            Ok(_) | Err(_) => {
                return Ok(UnknownResolution {
                    state: SubmissionState::Unknown,
                    age,
                    observation: ResolutionObservation::UnverifiedReceipt,
                    resend: ResendObservation::NotWarranted,
                });
            }
        };
        let (state, observation, cause) = if verified_receipt.result_code() == 0 {
            (
                SubmissionState::Executed,
                ResolutionObservation::ExecutedReceipt,
                "verified receipt found by idempotency key",
            )
        } else {
            (
                SubmissionState::Failed,
                ResolutionObservation::FailedReceipt,
                "verified failure receipt found by idempotency key",
            )
        };
        outbox.transition(store, submission_id, state, cause, Some(verified_receipt))?;
        store.remove_local(&key)?;
        return Ok(UnknownResolution {
            state,
            age,
            observation,
            resend: ResendObservation::NotWarranted,
        });
    }

    let resend = match boundary.resend_exact(submission_id, &record.signed_canonical_bytes) {
        Ok(()) => ResendObservation::Sent,
        Err(_) => ResendObservation::Indeterminate,
    };
    Ok(UnknownResolution {
        state: SubmissionState::Unknown,
        age,
        observation: ResolutionObservation::ReceiptMissing,
        resend,
    })
}

fn state_key(tenant: TenantId, submission_id: [u8; 32]) -> Result<TenantKey, StoreError> {
    let mut object_id = b"unknown-resolution:".to_vec();
    object_id.extend_from_slice(&submission_id);
    TenantKey::new(tenant, ObjectKind::Configuration, object_id)
}

fn backoff_ms(submission_id: [u8; 32], attempt_count: u32) -> u64 {
    let exponent = attempt_count.saturating_sub(1).min(6);
    let base = INITIAL_BACKOFF_MS
        .saturating_mul(1_u64 << exponent)
        .min(MAX_BACKOFF_MS);
    let digest = Sha256::digest([submission_id.as_slice(), &attempt_count.to_be_bytes()].concat());
    let jitter_window = base / 4;
    let jitter = u64::from_be_bytes(digest[..8].try_into().unwrap_or([0; 8])) % (jitter_window + 1);
    base.saturating_add(jitter).min(MAX_BACKOFF_MS)
}

fn encode_state(age: UnknownAge) -> Vec<u8> {
    let mut bytes = Vec::with_capacity(24);
    bytes.extend_from_slice(STATE_MAGIC);
    bytes.extend_from_slice(&age.first_observed_at_ms.to_be_bytes());
    bytes.extend_from_slice(&age.attempt_count.to_be_bytes());
    bytes.extend_from_slice(&age.next_attempt_at_ms.to_be_bytes());
    bytes
}

fn decode_state(bytes: &[u8], observed_at_ms: u64) -> Result<UnknownAge, UnknownResolutionError> {
    if bytes.len() != 24 || &bytes[..4] != STATE_MAGIC {
        return Err(UnknownResolutionError::Corrupt);
    }
    let mut first = [0_u8; 8];
    first.copy_from_slice(&bytes[4..12]);
    let mut attempts = [0_u8; 4];
    attempts.copy_from_slice(&bytes[12..16]);
    let mut next = [0_u8; 8];
    next.copy_from_slice(&bytes[16..24]);
    let first_observed_at_ms = u64::from_be_bytes(first);
    if observed_at_ms < first_observed_at_ms {
        return Err(UnknownResolutionError::TimeRegressed);
    }
    Ok(UnknownAge {
        first_observed_at_ms,
        age_ms: observed_at_ms - first_observed_at_ms,
        attempt_count: u32::from_be_bytes(attempts),
        next_attempt_at_ms: u64::from_be_bytes(next),
    })
}
