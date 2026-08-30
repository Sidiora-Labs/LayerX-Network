//! Verified, byte-preserving receipt storage and protocol-code classification.

use layerx_proof::receipt::{verify_outcome, AuthorizedBatch, ReceiptCheck};
use layerx_types::result::{KnownResult, ResultCode, ResultDomain, Retriability};
use layerx_types::verify::VerificationLevel;
use sha2::{Digest, Sha256};

use crate::store::{ObjectKind, Store, StoreError, TenantId, TenantKey};

const METADATA_MAGIC: &[u8; 4] = b"LXRM";

/// One of the three durable receipt indexes.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ReceiptLookupKey {
    Activity([u8; 32]),
    Idempotency([u8; 32]),
    GlobalSequence(u64),
}

/// Lossless protocol rejection classification.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ResultClassification {
    pub code: ResultCode,
    pub canonical: Option<KnownResult>,
    pub domain: ResultDomain,
    pub retriability: Retriability,
    pub retry_permitted: bool,
}

/// Metadata recorded only after local proof verification succeeds.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ReceiptMetadata {
    pub activity_id: [u8; 32],
    pub idempotency_key: [u8; 32],
    pub global_sequence: u64,
    pub verification_level: VerificationLevel,
    pub result: ResultClassification,
}

/// Exact stored bytes coupled to their achieved evidence and protocol result.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ServedReceipt {
    pub canonical_bytes: Vec<u8>,
    pub metadata: ReceiptMetadata,
}

#[derive(Debug)]
pub enum ReceiptStoreError {
    Verification(ReceiptCheck),
    Store(StoreError),
    Corrupt,
    Missing,
}

impl From<StoreError> for ReceiptStoreError {
    fn from(value: StoreError) -> Self {
        Self::Store(value)
    }
}

/// Classifies one exact protocol code without replacing its numeric value.
#[must_use]
pub const fn classify(code: ResultCode) -> ResultClassification {
    let retriability = code.retriability();
    ResultClassification {
        code,
        canonical: code.known(),
        domain: code.domain(),
        retriability,
        retry_permitted: code.raw() != 0 && matches!(retriability, Retriability::Retriable),
    }
}

/// Verifies through `layerx-proof`, then atomically stores exact bytes under
/// activity, idempotency, and global-sequence indexes.
///
/// # Errors
///
/// Returns the failed `layerx-proof` check when verification refuses the receipt, `Corrupt` when
/// the verified receipt carries no protocol section, and the store failure when an index key is
/// invalid or a durable index already holds conflicting bytes.
pub fn store(
    durable: &mut Store,
    tenant: TenantId,
    idempotency_key: [u8; 32],
    receipt_bytes: &[u8],
    authorised: &AuthorizedBatch,
) -> Result<ReceiptMetadata, ReceiptStoreError> {
    let verified = verify_outcome(receipt_bytes, authorised)
        .map_err(|failure| ReceiptStoreError::Verification(failure.check))?;
    let protocol = verified
        .receipt()
        .protocol()
        .ok_or(ReceiptStoreError::Corrupt)?;
    let metadata = ReceiptMetadata {
        activity_id: protocol.activity_id(),
        idempotency_key,
        global_sequence: protocol.global_sequence(),
        verification_level: verified.level(),
        result: classify(ResultCode::from_raw(protocol.result_code())),
    };
    let digest: [u8; 32] = Sha256::digest(verified.canonical_bytes()).into();
    let indexes = [
        lookup_key(
            tenant.clone(),
            ReceiptLookupKey::Activity(metadata.activity_id),
        )?,
        lookup_key(
            tenant.clone(),
            ReceiptLookupKey::Idempotency(metadata.idempotency_key),
        )?,
        lookup_key(
            tenant.clone(),
            ReceiptLookupKey::GlobalSequence(metadata.global_sequence),
        )?,
    ];
    durable.record_verified_receipt(
        &indexes,
        verified.canonical_bytes(),
        metadata_key(tenant, digest)?,
        encode_metadata(metadata),
    )?;
    Ok(metadata)
}

/// Serves the exact core-produced receipt bytes through any durable index.
///
/// # Errors
///
/// Returns `Missing` when the index holds no receipt, and `Corrupt` when the paired metadata
/// record is absent or does not decode.
pub fn serve(
    durable: &Store,
    tenant: TenantId,
    lookup: ReceiptLookupKey,
) -> Result<ServedReceipt, ReceiptStoreError> {
    let index = lookup_key(tenant.clone(), lookup)?;
    let bytes = durable
        .get(&index)
        .ok_or(ReceiptStoreError::Missing)?
        .bytes()
        .to_vec();
    let digest: [u8; 32] = Sha256::digest(&bytes).into();
    let metadata = durable
        .get(&metadata_key(tenant, digest)?)
        .ok_or(ReceiptStoreError::Corrupt)
        .and_then(|value| decode_metadata(value.bytes()))?;
    Ok(ServedReceipt {
        canonical_bytes: bytes,
        metadata,
    })
}

/// Persists a newly observed proof-verified receipt exactly once, or validates
/// that an existing durable receipt is the same observation without lowering
/// any independently augmented finality level.
///
/// # Errors
///
/// Returns the underlying proof or store error for a first observation and
/// `Corrupt` when an existing index conflicts with the verified ingress.
pub fn store_verified_if_absent(
    durable: &mut Store,
    tenant: TenantId,
    idempotency_key: [u8; 32],
    receipt_bytes: &[u8],
    authorised: &AuthorizedBatch,
) -> Result<ServedReceipt, ReceiptStoreError> {
    match serve(
        durable,
        tenant.clone(),
        ReceiptLookupKey::Idempotency(idempotency_key),
    ) {
        Ok(existing) => {
            let verified = verify_outcome(receipt_bytes, authorised)
                .map_err(|failure| ReceiptStoreError::Verification(failure.check))?;
            let protocol = verified
                .receipt()
                .protocol()
                .ok_or(ReceiptStoreError::Corrupt)?;
            if existing.canonical_bytes != verified.canonical_bytes()
                || existing.metadata.activity_id != protocol.activity_id()
                || existing.metadata.idempotency_key != idempotency_key
                || existing.metadata.global_sequence != protocol.global_sequence()
                || existing.metadata.result
                    != classify(ResultCode::from_raw(protocol.result_code()))
                || existing.metadata.verification_level < verified.level()
            {
                return Err(ReceiptStoreError::Corrupt);
            }
            Ok(existing)
        }
        Err(ReceiptStoreError::Missing) => {
            let metadata = store(
                durable,
                tenant.clone(),
                idempotency_key,
                receipt_bytes,
                authorised,
            )?;
            let served = serve(
                durable,
                tenant,
                ReceiptLookupKey::Idempotency(idempotency_key),
            )?;
            if served.canonical_bytes != receipt_bytes || served.metadata != metadata {
                return Err(ReceiptStoreError::Corrupt);
            }
            Ok(served)
        }
        Err(error) => Err(error),
    }
}

pub(crate) fn raise_verification_level(
    durable: &mut Store,
    tenant: TenantId,
    idempotency_key: [u8; 32],
    expected_receipt_bytes: &[u8],
    achieved: VerificationLevel,
) -> Result<ReceiptMetadata, ReceiptStoreError> {
    let served = serve(
        durable,
        tenant.clone(),
        ReceiptLookupKey::Idempotency(idempotency_key),
    )?;
    if served.canonical_bytes != expected_receipt_bytes {
        return Err(ReceiptStoreError::Corrupt);
    }
    let mut metadata = served.metadata;
    if achieved > metadata.verification_level {
        metadata.verification_level = achieved;
        let digest: [u8; 32] = Sha256::digest(expected_receipt_bytes).into();
        durable.put_local(metadata_key(tenant, digest)?, encode_metadata(metadata))?;
    }
    Ok(metadata)
}

fn lookup_key(tenant: TenantId, lookup: ReceiptLookupKey) -> Result<TenantKey, StoreError> {
    let mut object_id = match lookup {
        ReceiptLookupKey::Activity(_) => b"receipt:activity:".to_vec(),
        ReceiptLookupKey::Idempotency(_) => b"receipt:idempotency:".to_vec(),
        ReceiptLookupKey::GlobalSequence(_) => b"receipt:sequence:".to_vec(),
    };
    match lookup {
        ReceiptLookupKey::Activity(value) | ReceiptLookupKey::Idempotency(value) => {
            object_id.extend_from_slice(&value);
        }
        ReceiptLookupKey::GlobalSequence(value) => {
            object_id.extend_from_slice(&value.to_be_bytes());
        }
    }
    TenantKey::new(tenant, ObjectKind::Receipt, object_id)
}

fn metadata_key(tenant: TenantId, digest: [u8; 32]) -> Result<TenantKey, StoreError> {
    let mut object_id = b"receipt-metadata:".to_vec();
    object_id.extend_from_slice(&digest);
    TenantKey::new(tenant, ObjectKind::Configuration, object_id)
}

fn encode_metadata(metadata: ReceiptMetadata) -> Vec<u8> {
    let mut bytes = Vec::with_capacity(81);
    bytes.extend_from_slice(METADATA_MAGIC);
    bytes.extend_from_slice(&metadata.activity_id);
    bytes.extend_from_slice(&metadata.idempotency_key);
    bytes.extend_from_slice(&metadata.global_sequence.to_be_bytes());
    bytes.push(metadata.verification_level.wire_rank());
    bytes.extend_from_slice(&metadata.result.code.raw().to_be_bytes());
    bytes
}

fn decode_metadata(bytes: &[u8]) -> Result<ReceiptMetadata, ReceiptStoreError> {
    if bytes.len() != 81 || &bytes[..4] != METADATA_MAGIC {
        return Err(ReceiptStoreError::Corrupt);
    }
    let mut activity_id = [0_u8; 32];
    activity_id.copy_from_slice(&bytes[4..36]);
    let mut idempotency_key = [0_u8; 32];
    idempotency_key.copy_from_slice(&bytes[36..68]);
    let mut sequence = [0_u8; 8];
    sequence.copy_from_slice(&bytes[68..76]);
    let verification_level = match bytes[76] {
        1 => VerificationLevel::SEQUENCER_SIGNED,
        2 => VerificationLevel::BATCH_INCLUDED,
        3 => VerificationLevel::STATE_PROVEN,
        4 => VerificationLevel::CHECKPOINT_FINALISED,
        5 => VerificationLevel::SETTLEMENT_ANCHORED,
        _ => return Err(ReceiptStoreError::Corrupt),
    };
    let mut result = [0_u8; 4];
    result.copy_from_slice(&bytes[77..81]);
    Ok(ReceiptMetadata {
        activity_id,
        idempotency_key,
        global_sequence: u64::from_be_bytes(sequence),
        verification_level,
        result: classify(ResultCode::from_raw(i32::from_be_bytes(result))),
    })
}
