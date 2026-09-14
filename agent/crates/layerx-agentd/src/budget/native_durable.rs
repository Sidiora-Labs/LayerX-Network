use super::{NativeBudgetError as Error, NativeBudgetOutcome};
use crate::protocol_evidence::{RawActivityReceiptEvidence, RawReceiptEvidence};
use crate::store::{ObjectKind, StorageClass, Store, TenantId, TenantKey};

const MAX_BYTES: usize = 16_777_216;

#[derive(serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct StoredOutcome {
    version: u8,
    activity: Vec<u8>,
    receipt: Vec<u8>,
    proof: Vec<u8>,
    header: Vec<u8>,
    signature: Vec<u8>,
}

fn key(tenant: &TenantId, id: [u8; 32]) -> Result<TenantKey, Error> {
    TenantKey::new(
        tenant.clone(),
        ObjectKind::Budget,
        [b"native-budget-terminal-v1:".as_slice(), &id].concat(),
    )
    .map_err(|_| Error::Store)
}

pub(crate) fn persist_native_outcome(
    store: &mut Store,
    tenant: &TenantId,
    outcome: &NativeBudgetOutcome,
) -> Result<(), Error> {
    let raw = outcome.proof.receipt();
    let stored = StoredOutcome {
        version: 1,
        activity: outcome.proof.canonical_activity().to_vec(),
        receipt: raw.canonical_receipt().to_vec(),
        proof: layerx_proof::merkle::encode_proof(raw.proof()),
        header: raw.canonical_header().to_vec(),
        signature: raw.header_signature().to_vec(),
    };
    let encoded = serde_json::to_vec(&stored).map_err(|_| Error::Store)?;
    if encoded.len() > MAX_BYTES {
        return Err(Error::Store);
    }
    let key = key(tenant, outcome.idempotency_key)?;
    if let Some(previous) = store.get(&key) {
        return if previous.class() == StorageClass::LocalOnly && previous.bytes() == encoded {
            Ok(())
        } else {
            Err(Error::Store)
        };
    }
    store.put_local(key, encoded).map_err(|_| Error::Store)
}

pub(super) fn outcome(
    store: &Store,
    tenant: &TenantId,
    id: [u8; 32],
) -> Result<RawActivityReceiptEvidence, Error> {
    let value = store.get(&key(tenant, id)?).ok_or(Error::Store)?;
    if value.class() != StorageClass::LocalOnly || value.bytes().len() > MAX_BYTES {
        return Err(Error::Store);
    }
    let stored: StoredOutcome = serde_json::from_slice(value.bytes()).map_err(|_| Error::Store)?;
    if stored.version != 1 {
        return Err(Error::Store);
    }
    let proof = layerx_proof::merkle::decode_proof(&stored.proof).map_err(|_| Error::Store)?;
    let signature = stored.signature.try_into().map_err(|_| Error::Store)?;
    Ok(RawActivityReceiptEvidence::from_signed_inclusion(
        stored.activity,
        RawReceiptEvidence::new(stored.receipt, proof, stored.header, signature),
    ))
}
