//! Proof-gated submission finality augmentation and deadline-bounded waiting.

use layerx_client::evidence::{VerifiedCheckpoint, VerifiedProofBundle};
use layerx_proof::inclusion::{
    verify_activity, verify_receipt, InclusionError, SequencerAuthorization,
};
use layerx_proof::merkle::Proof;
use layerx_proof::receipt::verify_sequencer_signature;
use layerx_types::payload::ModuleRegistry;
use layerx_types::verify::VerificationLevel;
use layerx_wire::activity::{decode_signed, encode_signed};
use layerx_wire::hash::activity_id;
use layerx_wire::receipt::Receipt;

use crate::receipt::{self, ReceiptLookupKey, ReceiptStoreError};
use crate::store::{ObjectKind, Store, StoreError, TenantId, TenantKey};

const RECORD_MAGIC: &[u8; 4] = b"LXFA";

/// Activity and exact receipt evidence carried by one signed batch.
pub struct InclusionBundle<'a> {
    pub registry: &'a ModuleRegistry,
    pub activity_bytes: &'a [u8],
    pub activity_proof: &'a Proof,
    pub receipt_bytes: &'a [u8],
    pub receipt_proof: &'a Proof,
    pub header_bytes: &'a [u8],
    pub header_signature: [u8; 64],
    pub authorization: &'a SequencerAuthorization,
}

/// Verified evidence retained alongside, but never inside, original receipt bytes.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct FinalityRecord {
    pub idempotency_key: [u8; 32],
    pub verification_level: VerificationLevel,
    pub activity_proof: Vec<u8>,
    pub receipt_proof: Vec<u8>,
    pub checkpoint_id: Option<[u8; 32]>,
    pub guarantor_signatures_achieved: Option<usize>,
    pub guarantor_threshold: Option<usize>,
    pub settlement_reference: Option<Vec<u8>>,
}

#[derive(Debug)]
pub enum FinalityError {
    Receipt(ReceiptStoreError),
    Inclusion(InclusionError),
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
/// `Arithmetic` when a proof cannot be encoded, and `Corrupt` when the re-read
/// receipt differs from the raised one. Checkpoint finality is deliberately
/// unavailable on this raw-byte path; only [`augment_verified`] can consume a
/// node-authority-accepted [`VerifiedCheckpoint`].
pub fn augment(
    durable: &mut Store,
    tenant: TenantId,
    idempotency_key: [u8; 32],
    inclusion: &InclusionBundle<'_>,
) -> Result<FinalityRecord, FinalityError> {
    let receipt_before = receipt::serve(
        durable,
        tenant.clone(),
        ReceiptLookupKey::Idempotency(idempotency_key),
    )?;
    let activity = decode_signed(inclusion.activity_bytes, inclusion.registry)
        .map_err(|_| FinalityError::Corrupt)?;
    if encode_signed(&activity).map_err(|_| FinalityError::Corrupt)? != inclusion.activity_bytes
        || activity_id(&activity).map_err(|_| FinalityError::Corrupt)?
            != receipt_before.metadata.activity_id
    {
        return Err(FinalityError::Corrupt);
    }
    verify_activity(
        inclusion.activity_bytes,
        inclusion.activity_proof,
        inclusion.header_bytes,
        &inclusion.header_signature,
        inclusion.authorization,
    )
    .map_err(FinalityError::Inclusion)?;
    if inclusion.receipt_bytes != receipt_before.canonical_bytes {
        return Err(FinalityError::Corrupt);
    }
    let receipt_value = verify_sequencer_signature(
        inclusion.receipt_bytes,
        inclusion.authorization.public_key(),
    )
    .map_err(|_| FinalityError::Corrupt)?;
    let Receipt::Protocol(protocol_receipt) = receipt_value else {
        return Err(FinalityError::Corrupt);
    };
    if protocol_receipt.activity_id() != receipt_before.metadata.activity_id
        || protocol_receipt.global_sequence() != receipt_before.metadata.global_sequence
        || protocol_receipt.result_code() != receipt_before.metadata.result.code.raw()
    {
        return Err(FinalityError::Corrupt);
    }
    verify_receipt(
        inclusion.receipt_bytes,
        inclusion.receipt_proof,
        inclusion.header_bytes,
        &inclusion.header_signature,
        inclusion.authorization,
    )
    .map_err(FinalityError::Inclusion)?;

    let record = FinalityRecord {
        idempotency_key,
        verification_level: VerificationLevel::BATCH_INCLUDED,
        activity_proof: encode_proof(inclusion.activity_proof)?,
        receipt_proof: encode_proof(inclusion.receipt_proof)?,
        checkpoint_id: None,
        guarantor_signatures_achieved: None,
        guarantor_threshold: None,
        settlement_reference: None,
    };
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

/// Persists finality already established by the production LNI verifier.
///
/// The activity and exact stored receipt must have independently verified
/// inclusion under byte-identical signed-header evidence. A checkpoint raises
/// the receipt only when its verified certificate covers that same header.
pub fn augment_verified(
    durable: &mut Store,
    tenant: TenantId,
    idempotency_key: [u8; 32],
    activity: &VerifiedProofBundle,
    receipt_bundle: &VerifiedProofBundle,
    checkpoint: Option<&VerifiedCheckpoint>,
) -> Result<FinalityRecord, FinalityError> {
    let receipt_before = receipt::serve(
        durable,
        tenant.clone(),
        ReceiptLookupKey::Idempotency(idempotency_key),
    )?;
    let (
        VerifiedProofBundle::Activity {
            activity_id,
            proof: activity_proof,
            signed_header: activity_header,
            ..
        },
        VerifiedProofBundle::Receipt {
            canonical_bytes: receipt_bytes,
            activity_id: receipt_activity_id,
            proof: receipt_proof,
            signed_header: receipt_header,
        },
    ) = (activity, receipt_bundle)
    else {
        return Err(FinalityError::Corrupt);
    };
    if activity_id != receipt_activity_id
        || *activity_id != receipt_before.metadata.activity_id
        || receipt_bytes != &receipt_before.canonical_bytes
        || !activity_header.same_evidence(receipt_header)
    {
        return Err(FinalityError::Corrupt);
    }
    let mut record = FinalityRecord {
        idempotency_key,
        verification_level: VerificationLevel::BATCH_INCLUDED,
        activity_proof: encode_proof(activity_proof)?,
        receipt_proof: encode_proof(receipt_proof)?,
        checkpoint_id: None,
        guarantor_signatures_achieved: None,
        guarantor_threshold: None,
        settlement_reference: None,
    };
    if let Some(checkpoint) = checkpoint {
        let report = checkpoint.report();
        if report.level() != VerificationLevel::CHECKPOINT_FINALISED
            || checkpoint.canonical_header() != activity_header.canonical_bytes
        {
            return Err(FinalityError::Corrupt);
        }
        record.verification_level = VerificationLevel::CHECKPOINT_FINALISED;
        record.checkpoint_id = report.evidence().checkpoint_id();
        record.guarantor_signatures_achieved = Some(report.achieved);
        record.guarantor_threshold = Some(report.required);
        // Registration bytes are retained, but never relabelled as a Paxeer
        // settlement anchor without a separate live-chain finality verifier.
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
    push_bytes(&mut bytes, &record.receipt_proof)?;
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
