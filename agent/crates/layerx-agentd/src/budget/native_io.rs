use super::{
    NativeAccountCandidate, NativeBudgetBinding, NativeBudgetCandidate, NativeBudgetError as Error,
    NativeBudgetRecoveryEvidence,
};
use crate::protocol_evidence::{EvidenceAuthority, RawActivityReceiptEvidence, RawReceiptEvidence};
use crate::store::{ObjectKind, StorageClass, Store, TenantId, TenantKey};
use layerx_client::{
    evidence::{CheckpointSelector, ProofBundleSelector, VerifiedProofBundle},
    read::{HistoryKind, HistoryProof},
    Client,
};
use layerx_types::{payload::ModuleRegistry, verify::VerificationLevel};
use layerx_wire::native_budget::BudgetRecord;
use layerx_wire::receipt::{decode, decode_batch_header};

const BASELINE_PREFIX: &[u8] = b"native-budget-anchor-v1:";
const MAX_BASELINE_BYTES: usize = 16_777_216;

fn key(tenant: &TenantId, id: [u8; 32]) -> Result<TenantKey, Error> {
    TenantKey::new(
        tenant.clone(),
        ObjectKind::Budget,
        [BASELINE_PREFIX, &id].concat(),
    )
    .map_err(|_| Error::Store)
}

pub(super) fn baseline(
    store: &Store,
    tenant: &TenantId,
    id: [u8; 32],
) -> Result<Option<NativeBudgetCandidate>, Error> {
    store
        .get(&key(tenant, id)?)
        .map(|value| {
            if value.class() != StorageClass::LocalOnly || value.bytes().len() > MAX_BASELINE_BYTES
            {
                return Err(Error::Store);
            }
            serde_json::from_slice(value.bytes()).map_err(|_| Error::Store)
        })
        .transpose()
}

pub(super) fn retain_baseline(
    store: &mut Store,
    tenant: &TenantId,
    id: [u8; 32],
    value: &NativeBudgetCandidate,
) -> Result<(), Error> {
    if baseline(store, tenant, id)?.is_some() {
        return Err(Error::Baseline);
    }
    let encoded = serde_json::to_vec(value).map_err(|_| Error::Store)?;
    if encoded.len() > MAX_BASELINE_BYTES {
        return Err(Error::Store);
    }
    store
        .put_local(key(tenant, id)?, encoded)
        .map_err(|_| Error::Store)
}

pub(super) fn advance_anchor(
    store: &mut Store,
    tenant: &TenantId,
    id: [u8; 32],
    current: &NativeBudgetCandidate,
) -> Result<(), Error> {
    let encoded = serde_json::to_vec(current).map_err(|_| Error::Store)?;
    if encoded.len() > MAX_BASELINE_BYTES {
        return Err(Error::Store);
    }
    store
        .put_local(key(tenant, id)?, encoded)
        .map_err(|_| Error::Store)
}

#[derive(Clone, Copy)]
pub(super) struct TerminalRequest<'a> {
    pub id: [u8; 32],
    pub exact: &'a [u8],
    pub maximum_sequence: u64,
}

pub(super) fn terminal(
    client: &mut Client,
    authority: &EvidenceAuthority,
    binding: &NativeBudgetBinding,
    store: &Store,
    tenant: &TenantId,
    request: TerminalRequest<'_>,
) -> Result<super::NativeBudgetOutcome, Error> {
    let TerminalRequest {
        id,
        exact,
        maximum_sequence,
    } = request;
    let raw = super::native_durable::outcome(store, tenant, id)?;
    if raw.canonical_activity() != exact {
        return Err(Error::Activity);
    }
    let outcome = authority.restore_native_outcome(binding, &raw)?;
    if outcome.idempotency_key != id || outcome.sequence > maximum_sequence {
        return Err(Error::Receipt);
    }
    let header =
        decode_batch_header(raw.receipt().canonical_header()).map_err(|_| Error::Receipt)?;
    let checkpoint = client
        .checkpoint_evidence(CheckpointSelector::Batch(header.batch_number()), 6400)
        .map_err(|_| Error::Checkpoint)?;
    if checkpoint.canonical_header() != raw.receipt().canonical_header() {
        return Err(Error::Checkpoint);
    }
    Ok(outcome)
}

fn account(
    client: &mut Client,
    id: [u8; 32],
    correlation: u64,
    authorization: layerx_proof::inclusion::SequencerAuthorization,
) -> Result<NativeAccountCandidate, Error> {
    let value = client
        .account(
            id,
            VerificationLevel::CHECKPOINT_FINALISED,
            correlation,
            authorization,
        )
        .map_err(|_| Error::AccountProof)?;
    Ok(NativeAccountCandidate {
        canonical_value: value.canonical_bytes().to_vec(),
        proof_material: value.proof_material().to_vec(),
    })
}

pub(super) fn current(
    client: &mut Client,
    authority: &EvidenceAuthority,
    binding: &NativeBudgetBinding,
) -> Result<NativeBudgetCandidate, Error> {
    client.reconnect().map_err(|_| Error::Unavailable)?;
    let head = client.head();
    let authorization = authority.native_authorization(head.sealed_batch)?;
    let checkpoint = client
        .checkpoint_evidence(
            CheckpointSelector::Identifier(head.finalised_checkpoint),
            6100,
        )
        .map_err(|_| Error::Checkpoint)?;
    let header =
        decode_batch_header(checkpoint.canonical_header()).map_err(|_| Error::Checkpoint)?;
    if header.last_sequence() != head.chain_sequence || header.batch_number() != head.sealed_batch {
        return Err(Error::Checkpoint);
    }
    let key = [b"budget:".as_slice(), &binding.budget_id].concat();
    let module = client
        .module_state(
            3,
            &key,
            VerificationLevel::CHECKPOINT_FINALISED,
            6101,
            authorization,
        )
        .map_err(|_| Error::StateProof)?;
    let record = BudgetRecord::decode(module.canonical_bytes()).map_err(|_| Error::Record)?;
    let owner = account(client, binding.owner_account, 6102, authorization)?;
    let owner_key =
        layerx_proof::state::decode_account_value(binding.owner_account, &owner.canonical_value)
            .map_err(|_| Error::AccountProof)?
            .authority_key
            .ok_or(Error::Binding)?;
    let owner_history = if owner_key == binding.owner_public_key {
        None
    } else {
        let did = layerx_wire::hash::did_id_for_protocol(&binding.owner_did, 3)
            .map_err(|_| Error::Binding)?;
        let key = [&[0x0a][..], &did].concat();
        let value = client
            .module_state(
                7,
                &key,
                VerificationLevel::CHECKPOINT_FINALISED,
                6105,
                authorization,
            )
            .map_err(|_| Error::StateProof)?;
        Some(super::NativeOwnerHistoryCandidate {
            canonical_record: value.canonical_bytes().to_vec(),
            proof_material: value.proof_material().to_vec(),
        })
    };
    let budget = account(client, binding.budget_account, 6103, authorization)?;
    let source = record
        .source
        .filter(|id| *id != binding.owner_account)
        .map(|id| account(client, id, 6104, authorization))
        .transpose()?;
    Ok(NativeBudgetCandidate {
        canonical_record: module.canonical_bytes().to_vec(),
        module_proof_material: module.proof_material().to_vec(),
        owner,
        owner_history,
        account: budget,
        source,
        canonical_header: checkpoint.canonical_header().to_vec(),
        checkpoint_id: head.finalised_checkpoint,
    })
}

pub(super) fn history(
    client: &mut Client,
    authority: &EvidenceAuthority,
    registry: &ModuleRegistry,
    baseline: NativeBudgetCandidate,
    mut current: NativeBudgetCandidate,
) -> Result<NativeBudgetRecoveryEvidence, Error> {
    if baseline.canonical_header == current.canonical_header && current.owner_history.is_none() {
        current.owner_history.clone_from(&baseline.owner_history);
    }
    let first = decode_batch_header(&baseline.canonical_header).map_err(|_| Error::Baseline)?;
    let last = decode_batch_header(&current.canonical_header).map_err(|_| Error::Checkpoint)?;
    let mut history = Vec::new();
    let mut maintenance = Vec::new();
    let length = last
        .last_sequence()
        .checked_sub(first.last_sequence())
        .ok_or(Error::History)?;
    if length > 4096 {
        return Err(Error::History);
    }
    if length == 0 {
        return Ok(NativeBudgetRecoveryEvidence {
            baseline,
            current,
            history,
            maintenance,
        });
    }
    for batch in first
        .batch_number()
        .checked_add(1)
        .ok_or(Error::Arithmetic)?..=last.batch_number()
    {
        let header = client
            .batch_header(batch, 6200)
            .map_err(|_| Error::History)?;
        let authorization = authority.native_authorization(batch)?;
        let mut cursor = None;
        loop {
            let page = client
                .history(
                    header.header.first_sequence(),
                    header.header.last_sequence(),
                    256,
                    cursor,
                    VerificationLevel::BATCH_INCLUDED,
                    6201,
                    authorization,
                )
                .map_err(|_| Error::History)?;
            for item in page.items {
                if item.kind != HistoryKind::Receipt {
                    return Err(Error::History);
                }
                let proof =
                    HistoryProof::decode(item.proof_material()).map_err(|_| Error::History)?;
                if proof.header != header.canonical_bytes() {
                    return Err(Error::History);
                }
                if item.global_sequence == header.header.last_sequence() {
                    layerx_wire::batch_maintenance::decode_maintenance(item.canonical_bytes())
                        .map_err(|_| Error::History)?
                        .verify_header(&header.header)
                        .map_err(|_| Error::History)?;
                    maintenance.push(RawReceiptEvidence::new(
                        item.canonical_bytes().to_vec(),
                        proof.proof,
                        proof.header,
                        proof.header_signature,
                    ));
                    continue;
                }
                history.push(activity_entry(client, registry, &item, proof)?);
            }
            cursor = page.cursor;
            if cursor.is_none() {
                break;
            }
        }
    }
    owner_history_for_interval(client, authority, &mut current, &history)?;
    Ok(NativeBudgetRecoveryEvidence {
        baseline,
        current,
        history,
        maintenance,
    })
}

fn owner_history_for_interval(
    client: &mut Client,
    authority: &EvidenceAuthority,
    current: &mut NativeBudgetCandidate,
    history: &[RawActivityReceiptEvidence],
) -> Result<(), Error> {
    if current.owner_history.is_some() {
        return Ok(());
    }
    let record = BudgetRecord::decode(&current.canonical_record).map_err(|_| Error::Record)?;
    let owner =
        layerx_proof::state::decode_account_value(record.owner, &current.owner.canonical_value)
            .map_err(|_| Error::AccountProof)?;
    let name = std::str::from_utf8(&owner.name).map_err(|_| Error::Binding)?;
    let did = layerx_types::ids::Did::new(
        name.strip_prefix("agent:")
            .and_then(|value| value.strip_suffix(":main"))
            .ok_or(Error::Binding)?
            .as_bytes(),
    )
    .map_err(|_| Error::Binding)?;
    let did = layerx_wire::hash::did_id_for_protocol(&did, 3).map_err(|_| Error::Binding)?;
    let mut changed = false;
    for entry in history {
        let decoded = decode(entry.receipt().canonical_receipt()).map_err(|_| Error::Receipt)?;
        let receipt = decoded.protocol().ok_or(Error::Receipt)?;
        changed |= receipt.module_id() == 7
            && receipt.result_code() == 0
            && receipt.effects().iter().any(|effect| {
                effect.event_type() == 0x7142 && effect.body().get(5..37) == Some(did.as_slice())
            });
    }
    if !changed {
        return Ok(());
    }
    let header = decode_batch_header(&current.canonical_header).map_err(|_| Error::Checkpoint)?;
    let authorization = authority.native_authorization(header.batch_number())?;
    let key = [&[0x0a][..], &did].concat();
    let value = client
        .module_state(
            7,
            &key,
            VerificationLevel::CHECKPOINT_FINALISED,
            6106,
            authorization,
        )
        .map_err(|_| Error::StateProof)?;
    current.owner_history = Some(super::NativeOwnerHistoryCandidate {
        canonical_record: value.canonical_bytes().to_vec(),
        proof_material: value.proof_material().to_vec(),
    });
    Ok(())
}

fn activity_entry(
    client: &mut Client,
    registry: &ModuleRegistry,
    item: &layerx_client::read::HistoryItem,
    proof: HistoryProof,
) -> Result<RawActivityReceiptEvidence, Error> {
    let receipt = decode(item.canonical_bytes()).map_err(|_| Error::Receipt)?;
    let id = receipt.protocol().ok_or(Error::Receipt)?.activity_id();
    let activity = client
        .proof_bundle(ProofBundleSelector::Activity(id), 6202, registry)
        .map_err(|_| Error::Activity)?;
    let VerifiedProofBundle::Activity {
        canonical_bytes,
        activity_id,
        signed_header,
        ..
    } = activity
    else {
        return Err(Error::Activity);
    };
    if activity_id != id || signed_header.canonical_bytes != proof.header {
        return Err(Error::Activity);
    }
    Ok(RawActivityReceiptEvidence::from_signed_inclusion(
        canonical_bytes,
        RawReceiptEvidence::new(
            item.canonical_bytes().to_vec(),
            proof.proof,
            proof.header,
            proof.header_signature,
        ),
    ))
}

/// Retrieves the actual finalized native proof bundle and bounded complete receipt history.
/// # Errors
/// Refuses unavailable state, checkpoint or activity evidence; returned bytes require reconciliation.
pub fn retrieve(
    client: &mut Client,
    authority: &EvidenceAuthority,
    binding: &NativeBudgetBinding,
    baseline: Option<NativeBudgetCandidate>,
    registry: &ModuleRegistry,
) -> Result<NativeBudgetRecoveryEvidence, Error> {
    let current = current(client, authority, binding)?;
    let baseline = baseline.unwrap_or_else(|| current.clone());
    history(client, authority, registry, baseline, current)
}
