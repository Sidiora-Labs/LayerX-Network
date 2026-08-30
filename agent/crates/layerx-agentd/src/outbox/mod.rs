//! Durable exact-byte submission outbox and explicit state machine.

use std::collections::BTreeMap;

use crate::protocol_evidence::VerifiedReceiptEvidence;
use crate::sign::VerifiedSubmission;
use crate::store::{ObjectKind, Store, StoreError, TenantId, TenantKey};

#[path = "unknown.rs"]
mod resolution;
#[path = "recover.rs"]
mod restart;

pub use resolution::{
    resolve_unknown, ReceiptLookup, ResendObservation, ResolutionObservation, UnknownAge,
    UnknownBoundaryError, UnknownResolution, UnknownResolutionError,
};
pub use restart::{
    recover, RecoveredOutbox, RecoveryError, RecoveryInputs, UnknownCeilingReservation,
};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SubmissionState {
    Prepared,
    Signed,
    Queued,
    Submitted,
    Acknowledged,
    Unknown,
    Executed,
    Failed,
    Expired,
    Superseded,
}

impl SubmissionState {
    #[must_use]
    pub const fn terminal(self) -> bool {
        matches!(
            self,
            Self::Executed | Self::Failed | Self::Expired | Self::Superseded
        )
    }

    const fn code(self) -> u8 {
        match self {
            Self::Prepared => 1,
            Self::Signed => 2,
            Self::Queued => 3,
            Self::Submitted => 4,
            Self::Acknowledged => 5,
            Self::Unknown => 6,
            Self::Executed => 7,
            Self::Failed => 8,
            Self::Expired => 9,
            Self::Superseded => 10,
        }
    }

    fn from_code(code: u8) -> Result<Self, OutboxError> {
        match code {
            1 => Ok(Self::Prepared),
            2 => Ok(Self::Signed),
            3 => Ok(Self::Queued),
            4 => Ok(Self::Submitted),
            5 => Ok(Self::Acknowledged),
            6 => Ok(Self::Unknown),
            7 => Ok(Self::Executed),
            8 => Ok(Self::Failed),
            9 => Ok(Self::Expired),
            10 => Ok(Self::Superseded),
            _ => Err(OutboxError::Corrupt),
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ReceiptEvidence {
    receipt_ref: [u8; 32],
}

impl ReceiptEvidence {
    #[must_use]
    pub const fn receipt_ref(self) -> [u8; 32] {
        self.receipt_ref
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct StateTransition {
    pub from: SubmissionState,
    pub to: SubmissionState,
    pub cause: String,
    pub receipt: Option<ReceiptEvidence>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SubmissionStatus {
    pub submission_id: [u8; 32],
    pub state: SubmissionState,
    pub activity_id: [u8; 32],
    pub evidence: Option<ReceiptEvidence>,
    pub transitions: Vec<StateTransition>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct OutboxRecord {
    tenant: TenantId,
    status: SubmissionStatus,
    signed_canonical_bytes: Vec<u8>,
}

#[derive(Default)]
pub struct Outbox {
    records: BTreeMap<[u8; 32], OutboxRecord>,
}

impl Outbox {
    /// Durably queues exact verified bytes before they can be obtained for transport.
    ///
    /// # Errors
    ///
    /// Returns `Duplicate` for an already-tracked submission identifier, `IdempotencyMismatch` when
    /// the signed audit key differs from it, `Corrupt` when the record exceeds the `u32` length
    /// prefixes, or `Store` when the three durable records cannot be written atomically.
    pub fn enqueue(
        &mut self,
        store: &mut Store,
        tenant: TenantId,
        submission_id: [u8; 32],
        verified: VerifiedSubmission,
    ) -> Result<(), OutboxError> {
        if self.records.contains_key(&submission_id) {
            return Err(OutboxError::Duplicate);
        }
        if verified.idempotency_key() != submission_id {
            return Err(OutboxError::IdempotencyMismatch);
        }
        let activity_id = verified.activity_id();
        let signed_canonical_bytes = verified.into_exact_bytes();
        let transitions = vec![
            StateTransition {
                from: SubmissionState::Prepared,
                to: SubmissionState::Signed,
                cause: "exact signature verified".to_owned(),
                receipt: None,
            },
            StateTransition {
                from: SubmissionState::Signed,
                to: SubmissionState::Queued,
                cause: "durable outbox record created".to_owned(),
                receipt: None,
            },
        ];
        let record = OutboxRecord {
            tenant: tenant.clone(),
            status: SubmissionStatus {
                submission_id,
                state: SubmissionState::Queued,
                activity_id,
                evidence: None,
                transitions,
            },
            signed_canonical_bytes: signed_canonical_bytes.clone(),
        };
        store
            .record_submission(
                tenant,
                submission_id.to_vec(),
                signed_canonical_bytes,
                encode_record(&record)?,
            )
            .map_err(OutboxError::Store)?;
        self.records.insert(submission_id, record);
        Ok(())
    }

    /// Restores one durable record by its tenant-scoped idempotency key.
    ///
    /// # Errors
    ///
    /// Returns `NotFound` when no durable outbox record exists, `Corrupt` for a missing signed-bytes
    /// record, undecodable state or a record naming a different submission, and `Store` when either
    /// tenant key cannot be built.
    pub fn restore(
        &mut self,
        store: &Store,
        tenant: TenantId,
        submission_id: [u8; 32],
    ) -> Result<(), OutboxError> {
        let outbox_key = TenantKey::new(tenant.clone(), ObjectKind::Outbox, submission_id.to_vec())
            .map_err(OutboxError::Store)?;
        let signed_key = TenantKey::new(
            tenant.clone(),
            ObjectKind::PreparedActivity,
            submission_id.to_vec(),
        )
        .map_err(OutboxError::Store)?;
        let encoded = store.get(&outbox_key).ok_or(OutboxError::NotFound)?;
        let signed = store.get(&signed_key).ok_or(OutboxError::Corrupt)?;
        let mut record = decode_record(encoded.bytes(), tenant)?;
        if record.status.submission_id != submission_id {
            return Err(OutboxError::Corrupt);
        }
        record.signed_canonical_bytes = signed.bytes().to_vec();
        self.records.insert(submission_id, record);
        Ok(())
    }

    /// Returns exact stored bytes only after the queued record is durable.
    ///
    /// # Errors
    ///
    /// Returns `NotFound` for a submission this outbox never enqueued or restored, and `NotQueued`
    /// once it has left `Queued` for any later state.
    pub fn bytes_for_transmission(&self, submission_id: [u8; 32]) -> Result<&[u8], OutboxError> {
        let record = self
            .records
            .get(&submission_id)
            .ok_or(OutboxError::NotFound)?;
        if record.status.state != SubmissionState::Queued {
            return Err(OutboxError::NotQueued);
        }
        Ok(&record.signed_canonical_bytes)
    }

    /// Returns the exact durable signed activity at any later lifecycle state.
    pub fn exact_signed_bytes(&self, submission_id: [u8; 32]) -> Result<&[u8], OutboxError> {
        self.records
            .get(&submission_id)
            .map(|record| record.signed_canonical_bytes.as_slice())
            .ok_or(OutboxError::NotFound)
    }

    /// Applies one legal state change and durably records it with its cause.
    ///
    /// # Errors
    ///
    /// Returns `EmptyCause` for a blank cause, `NotFound` for an untracked submission,
    /// `InvalidTransition` for a move `legal_transition` forbids, `SuccessWithoutVerifiedReceipt`
    /// for `Executed` without verified evidence, `ReceiptMismatch` for evidence naming another
    /// activity, and `Corrupt` or `Store` when the updated record cannot be encoded or persisted.
    pub fn transition(
        &mut self,
        store: &mut Store,
        submission_id: [u8; 32],
        to: SubmissionState,
        cause: impl Into<String>,
        receipt: Option<VerifiedReceiptEvidence>,
    ) -> Result<SubmissionStatus, OutboxError> {
        let cause = cause.into();
        if cause.is_empty() {
            return Err(OutboxError::EmptyCause);
        }
        let current = self
            .records
            .get(&submission_id)
            .cloned()
            .ok_or(OutboxError::NotFound)?;
        let from = current.status.state;
        if !legal_transition(from, to) {
            return Err(OutboxError::InvalidTransition { from, to });
        }
        if to == SubmissionState::Executed && receipt.is_none() {
            return Err(OutboxError::SuccessWithoutVerifiedReceipt);
        }
        if receipt
            .as_ref()
            .is_some_and(|evidence| evidence.activity_id() != current.status.activity_id)
        {
            return Err(OutboxError::ReceiptMismatch);
        }
        let receipt = receipt.map(|evidence| ReceiptEvidence {
            receipt_ref: evidence.receipt_ref(),
        });
        let mut updated = current;
        updated.status.state = to;
        updated.status.evidence = receipt;
        updated.status.transitions.push(StateTransition {
            from,
            to,
            cause,
            receipt,
        });
        let key = TenantKey::new(
            updated.tenant.clone(),
            ObjectKind::Outbox,
            submission_id.to_vec(),
        )
        .map_err(OutboxError::Store)?;
        store
            .put_local(key, encode_record(&updated)?)
            .map_err(OutboxError::Store)?;
        let status = updated.status.clone();
        self.records.insert(submission_id, updated);
        Ok(status)
    }

    #[must_use]
    pub fn status(&self, submission_id: [u8; 32]) -> Option<&SubmissionStatus> {
        self.records
            .get(&submission_id)
            .map(|record| &record.status)
    }
}

#[derive(Debug)]
pub enum OutboxError {
    Duplicate,
    IdempotencyMismatch,
    NotFound,
    NotQueued,
    EmptyCause,
    Corrupt,
    SuccessWithoutVerifiedReceipt,
    ReceiptMismatch,
    InvalidTransition {
        from: SubmissionState,
        to: SubmissionState,
    },
    Store(StoreError),
}

fn legal_transition(from: SubmissionState, to: SubmissionState) -> bool {
    matches!(
        (from, to),
        (
            SubmissionState::Queued,
            SubmissionState::Submitted | SubmissionState::Expired | SubmissionState::Superseded,
        ) | (
            SubmissionState::Submitted,
            SubmissionState::Acknowledged | SubmissionState::Unknown,
        ) | (
            SubmissionState::Acknowledged,
            SubmissionState::Unknown | SubmissionState::Executed | SubmissionState::Failed,
        ) | (
            SubmissionState::Unknown,
            SubmissionState::Executed | SubmissionState::Failed | SubmissionState::Superseded,
        )
    )
}

fn encode_record(record: &OutboxRecord) -> Result<Vec<u8>, OutboxError> {
    let mut bytes = Vec::new();
    bytes.extend_from_slice(b"LXOB");
    bytes.push(2);
    bytes.extend_from_slice(&record.status.submission_id);
    bytes.push(record.status.state.code());
    bytes.extend_from_slice(&record.status.activity_id);
    encode_receipt(&mut bytes, record.status.evidence);
    push_u32(&mut bytes, record.status.transitions.len())?;
    for transition in &record.status.transitions {
        bytes.push(transition.from.code());
        bytes.push(transition.to.code());
        push_bytes(&mut bytes, transition.cause.as_bytes())?;
        encode_receipt(&mut bytes, transition.receipt);
    }
    Ok(bytes)
}

fn decode_record(bytes: &[u8], tenant: TenantId) -> Result<OutboxRecord, OutboxError> {
    let mut decoder = RecordDecoder { bytes, offset: 0 };
    if decoder.take(4)? != b"LXOB" || decoder.u8()? != 2 {
        return Err(OutboxError::Corrupt);
    }
    let submission_id = decoder.fixed()?;
    let state = SubmissionState::from_code(decoder.u8()?)?;
    let activity_id = decoder.fixed()?;
    let evidence = decoder.receipt()?;
    let transition_count = decoder.u32()? as usize;
    if transition_count > 1_024 {
        return Err(OutboxError::Corrupt);
    }
    let mut transitions = Vec::with_capacity(transition_count);
    for _ in 0..transition_count {
        let from = SubmissionState::from_code(decoder.u8()?)?;
        let to = SubmissionState::from_code(decoder.u8()?)?;
        let cause =
            String::from_utf8(decoder.bytes()?.to_vec()).map_err(|_| OutboxError::Corrupt)?;
        let receipt = decoder.receipt()?;
        transitions.push(StateTransition {
            from,
            to,
            cause,
            receipt,
        });
    }
    if decoder.offset != bytes.len() {
        return Err(OutboxError::Corrupt);
    }
    Ok(OutboxRecord {
        tenant,
        status: SubmissionStatus {
            submission_id,
            state,
            activity_id,
            evidence,
            transitions,
        },
        signed_canonical_bytes: Vec::new(),
    })
}

fn encode_receipt(bytes: &mut Vec<u8>, receipt: Option<ReceiptEvidence>) {
    match receipt {
        Some(receipt) => {
            bytes.push(1);
            bytes.extend_from_slice(&receipt.receipt_ref);
        }
        None => bytes.push(0),
    }
}

fn push_u32(bytes: &mut Vec<u8>, value: usize) -> Result<(), OutboxError> {
    let value = u32::try_from(value).map_err(|_| OutboxError::Corrupt)?;
    bytes.extend_from_slice(&value.to_be_bytes());
    Ok(())
}

fn push_bytes(bytes: &mut Vec<u8>, value: &[u8]) -> Result<(), OutboxError> {
    push_u32(bytes, value.len())?;
    bytes.extend_from_slice(value);
    Ok(())
}

struct RecordDecoder<'a> {
    bytes: &'a [u8],
    offset: usize,
}

impl<'a> RecordDecoder<'a> {
    fn take(&mut self, length: usize) -> Result<&'a [u8], OutboxError> {
        let end = self
            .offset
            .checked_add(length)
            .ok_or(OutboxError::Corrupt)?;
        let value = self
            .bytes
            .get(self.offset..end)
            .ok_or(OutboxError::Corrupt)?;
        self.offset = end;
        Ok(value)
    }

    fn u8(&mut self) -> Result<u8, OutboxError> {
        Ok(*self.take(1)?.first().ok_or(OutboxError::Corrupt)?)
    }

    fn u32(&mut self) -> Result<u32, OutboxError> {
        let mut value = [0; 4];
        value.copy_from_slice(self.take(4)?);
        Ok(u32::from_be_bytes(value))
    }

    fn fixed(&mut self) -> Result<[u8; 32], OutboxError> {
        let mut value = [0; 32];
        value.copy_from_slice(self.take(32)?);
        Ok(value)
    }

    fn bytes(&mut self) -> Result<&'a [u8], OutboxError> {
        let length = self.u32()? as usize;
        if length > 1_048_576 {
            return Err(OutboxError::Corrupt);
        }
        self.take(length)
    }

    fn receipt(&mut self) -> Result<Option<ReceiptEvidence>, OutboxError> {
        match self.u8()? {
            0 => Ok(None),
            1 => Ok(Some(ReceiptEvidence {
                receipt_ref: self.fixed()?,
            })),
            _ => Err(OutboxError::Corrupt),
        }
    }
}
