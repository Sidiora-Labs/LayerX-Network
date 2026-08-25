//! Proof-gated submission finality augmentation and deadline-bounded waiting.

use layerx_proof::checkpoint::{
    verify_certificate, Certificate, CheckpointError, GuarantorKey, SettlementDomain,
};
use layerx_proof::inclusion::{
    verify_activity, verify_state, InclusionError, SequencerAuthorization,
};
use layerx_proof::merkle::Proof;
use layerx_types::verify::VerificationLevel;

use crate::receipt::{self, ReceiptLookupKey, ReceiptStoreError};
use crate::store::{ObjectKind, Store, StoreError, TenantId, TenantKey};

const RECORD_MAGIC: &[u8; 4] = b"LXFA";

/// Activity and state evidence carried by one signed batch.
pub struct InclusionBundle<'a> {
    pub activity_bytes: &'a [u8],
    pub activity_proof: &'a Proof,
    pub state_leaf_bytes: &'a [u8],
    pub state_proof: &'a Proof,
    pub named_resulting_state_root: [u8; 32],
    pub header_bytes: &'a [u8],
    pub header_signature: [u8; 64],
    pub authorization: &'a SequencerAuthorization,
}

/// Optional checkpoint evidence that may raise a state-proven receipt.
pub struct CheckpointBundle<'a> {
    pub certificate: &'a Certificate,
    pub bonded_set: &'a [GuarantorKey],
    pub registered_checkpoint_id: [u8; 32],
    pub expected_settlement_domain: SettlementDomain,
    pub registered_settlement_reference: Option<&'a [u8]>,
}

/// Verified evidence retained alongside, but never inside, original receipt bytes.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct FinalityRecord {
    pub idempotency_key: [u8; 32],
    pub verification_level: VerificationLevel,
    pub activity_proof: Vec<u8>,
    pub state_proof: Vec<u8>,
    pub checkpoint_id: Option<[u8; 32]>,
    pub guarantor_signatures_achieved: Option<usize>,
    pub guarantor_threshold: Option<usize>,
    pub settlement_reference: Option<Vec<u8>>,
}

#[derive(Debug)]
pub enum FinalityError {
    Receipt(ReceiptStoreError),
    Inclusion(InclusionError),
    Checkpoint(CheckpointError),
    Store(StoreError),
    InvalidDeadline,
    ProgressUnavailable,
    Arithmetic,
    Corrupt,
}

impl From<ReceiptStoreError> for FinalityError {
    fn from(value: ReceiptStoreError) -> Self {
        Self::Receipt(value)
    }
}

impl From<StoreError> for FinalityError {
    fn from(value: StoreError) -> Self {
        Self::Store(value)
    }
}

/// Verifies and records only evidence-supported finality without modifying the receipt.
///
/// # Errors
///
/// Returns a receipt error when the receipt is absent or corrupt, `Inclusion` or
/// `Checkpoint` when the supplied evidence fails verification, `Arithmetic` when a
/// proof cannot be encoded, and `Corrupt` when the re-read receipt differs from
/// the raised one.
pub fn augment(
    durable: &mut Store,
    tenant: TenantId,
    idempotency_key: [u8; 32],
    inclusion: &InclusionBundle<'_>,
    checkpoint: Option<&CheckpointBundle<'_>>,
) -> Result<FinalityRecord, FinalityError> {
    let receipt_before = receipt::serve(
        durable,
        tenant.clone(),
        ReceiptLookupKey::Idempotency(idempotency_key),
    )?;
    verify_activity(
        inclusion.activity_bytes,
        inclusion.activity_proof,
        inclusion.header_bytes,
        &inclusion.header_signature,
        inclusion.authorization,
    )
    .map_err(FinalityError::Inclusion)?;
    verify_state(
        inclusion.state_leaf_bytes,
        inclusion.state_proof,
        &inclusion.named_resulting_state_root,
        inclusion.header_bytes,
        &inclusion.header_signature,
        inclusion.authorization,
    )
    .map_err(FinalityError::Inclusion)?;

    let mut record = FinalityRecord {
        idempotency_key,
        verification_level: VerificationLevel::STATE_PROVEN,
        activity_proof: encode_proof(inclusion.activity_proof)?,
        state_proof: encode_proof(inclusion.state_proof)?,
        checkpoint_id: None,
        guarantor_signatures_achieved: None,
        guarantor_threshold: None,
        settlement_reference: None,
    };
    if let Some(checkpoint) = checkpoint {
        let report = verify_certificate(
            checkpoint.certificate,
            checkpoint.bonded_set,
            &checkpoint.registered_checkpoint_id,
            checkpoint.expected_settlement_domain,
            checkpoint.registered_settlement_reference,
        )
        .map_err(FinalityError::Checkpoint)?;
        record.verification_level = report.level();
        record.checkpoint_id = report.evidence().checkpoint_id();
        record.guarantor_signatures_achieved = Some(report.achieved);
        record.guarantor_threshold = Some(report.required);
        record.settlement_reference = report.evidence().settlement_reference().map(<[u8]>::to_vec);
    }

    durable.put_local(
        record_key(tenant.clone(), idempotency_key)?,
        encode_record(&record)?,
    )?;
    let metadata = receipt::raise_verification_level(
        durable,
        tenant.clone(),
        idempotency_key,
        &receipt_before.canonical_bytes,
        record.verification_level,
    )?;
    let receipt_after = receipt::serve(
        durable,
        tenant,
        ReceiptLookupKey::Idempotency(idempotency_key),
    )?;
    if receipt_after.canonical_bytes != receipt_before.canonical_bytes
        || receipt_after.metadata.verification_level != metadata.verification_level
    {
        return Err(FinalityError::Corrupt);
    }
    Ok(record)
}

/// Source of independently observed finality progress for deadline-bounded waits.
pub trait VerificationProgress {
    /// Returns the independently observed level for the submission at that instant.
    ///
    /// # Errors
    ///
    /// Returns `ProgressUnavailable` when the source cannot observe the submission at
    /// `observed_at_ms`.
    fn level_at(
        &mut self,
        idempotency_key: [u8; 32],
        observed_at_ms: u64,
    ) -> Result<VerificationLevel, FinalityError>;
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct WaitResult {
    pub requested: VerificationLevel,
    pub reached: VerificationLevel,
    pub observed_at_ms: u64,
    pub deadline_elapsed: bool,
}

/// Polls until the declared level is observed or the explicit deadline is reached.
///
/// # Errors
///
/// Returns `InvalidDeadline` when the deadline precedes the start or the poll
/// interval is zero, `Arithmetic` when the next poll instant overflows, and
/// propagates the progress-source failure.
pub fn wait_for_level<P: VerificationProgress>(
    progress: &mut P,
    idempotency_key: [u8; 32],
    requested: VerificationLevel,
    started_at_ms: u64,
    deadline_ms: u64,
    poll_interval_ms: u64,
) -> Result<WaitResult, FinalityError> {
    if deadline_ms < started_at_ms || poll_interval_ms == 0 {
        return Err(FinalityError::InvalidDeadline);
    }
    let mut observed_at_ms = started_at_ms;
    loop {
        let reached = progress.level_at(idempotency_key, observed_at_ms)?;
        if reached >= requested {
            return Ok(WaitResult {
                requested,
                reached,
                observed_at_ms,
                deadline_elapsed: false,
            });
        }
        if observed_at_ms == deadline_ms {
            return Ok(WaitResult {
                requested,
                reached,
                observed_at_ms,
                deadline_elapsed: true,
            });
        }
        observed_at_ms = observed_at_ms
            .checked_add(poll_interval_ms)
            .ok_or(FinalityError::Arithmetic)?
            .min(deadline_ms);
    }
}

fn record_key(tenant: TenantId, idempotency_key: [u8; 32]) -> Result<TenantKey, StoreError> {
    let mut object_id = b"finality:".to_vec();
    object_id.extend_from_slice(&idempotency_key);
    TenantKey::new(tenant, ObjectKind::Configuration, object_id)
}

fn encode_proof(proof: &Proof) -> Result<Vec<u8>, FinalityError> {
    let count = u32::try_from(proof.siblings().len()).map_err(|_| FinalityError::Arithmetic)?;
    let mut bytes = Vec::new();
    bytes.extend_from_slice(&proof.leaf_index().to_be_bytes());
    bytes.extend_from_slice(&proof.leaf_count().to_be_bytes());
    bytes.extend_from_slice(&count.to_be_bytes());
    for sibling in proof.siblings() {
        bytes.extend_from_slice(sibling);
    }
    Ok(bytes)
}

fn encode_record(record: &FinalityRecord) -> Result<Vec<u8>, FinalityError> {
    let mut bytes = Vec::new();
    bytes.extend_from_slice(RECORD_MAGIC);
    bytes.extend_from_slice(&record.idempotency_key);
    bytes.push(record.verification_level.wire_rank());
    push_bytes(&mut bytes, &record.activity_proof)?;
    push_bytes(&mut bytes, &record.state_proof)?;
    match record.checkpoint_id {
        Some(identifier) => {
            bytes.push(1);
            bytes.extend_from_slice(&identifier);
            let achieved = u32::try_from(record.guarantor_signatures_achieved.unwrap_or(0))
                .map_err(|_| FinalityError::Arithmetic)?;
            let threshold = u32::try_from(record.guarantor_threshold.unwrap_or(0))
                .map_err(|_| FinalityError::Arithmetic)?;
            bytes.extend_from_slice(&achieved.to_be_bytes());
            bytes.extend_from_slice(&threshold.to_be_bytes());
        }
        None => bytes.push(0),
    }
    match &record.settlement_reference {
        Some(reference) => {
            bytes.push(1);
            push_bytes(&mut bytes, reference)?;
        }
        None => bytes.push(0),
    }
    Ok(bytes)
}

fn push_bytes(output: &mut Vec<u8>, value: &[u8]) -> Result<(), FinalityError> {
    let length = u32::try_from(value.len()).map_err(|_| FinalityError::Arithmetic)?;
    output.extend_from_slice(&length.to_be_bytes());
    output.extend_from_slice(value);
    Ok(())
}
