use super::{
    action_record_key, agent_key, decode, encode, finalization_digest, response_agent,
    FinalizationEvidence, HumanOperationError as Error, HumanResponse, ManagedAgent, StorageClass,
    Store, TenantId,
};
use crate::outbox::Outbox;
use crate::receipt::{ReceiptLookupKey, ServedReceipt};
use crate::store::{key, ObjectKind};
use layerx_crypto::disclosure::DisclosedNativeBudgetAmend;
use layerx_types::payload::{ActivityType, ModuleId, ModuleRegistration, ModuleRegistry};
use layerx_types::verify::VerificationLevel;
use layerx_wire::activity::{decode_signed, encode_signed, encode_unsigned, Activity};
use layerx_wire::hash::{activity_id, payload_hash, Domain};
use layerx_wire::receipt::{decode as decode_receipt, ProtocolReceipt};
use sha2::{Digest, Sha256};

pub(super) fn applies(
    store: &Store,
    tenant: &TenantId,
    evidence: FinalizationEvidence,
) -> Result<bool, Error> {
    let served = match crate::receipt::serve(
        store,
        tenant.clone(),
        ReceiptLookupKey::Idempotency(evidence.action_key),
    ) {
        Ok(value) => value,
        Err(crate::receipt::ReceiptStoreError::Missing) => return Ok(false),
        Err(_) => return Err(Error::Refused),
    };
    let receipt = decode_receipt(&served.canonical_bytes).map_err(|_| Error::Refused)?;
    Ok(receipt.protocol().is_some_and(|protocol| {
        protocol.protocol_version() == 3
            && protocol.module_id() == ModuleId::Budget as u16
            && protocol.operation() == 0
    }))
}

fn source(
    store: &Store,
    tenant: &TenantId,
    evidence: FinalizationEvidence,
) -> Result<Activity, Error> {
    for kind in [ObjectKind::Outbox, ObjectKind::PreparedActivity] {
        let object =
            key(tenant.clone(), kind, evidence.action_key.to_vec()).map_err(|_| Error::Refused)?;
        if store.get(&object).ok_or(Error::Unavailable)?.class() != StorageClass::LocalOnly {
            return Err(Error::Refused);
        }
    }
    let mut outbox = Outbox::default();
    outbox
        .restore(store, tenant.clone(), evidence.action_key)
        .map_err(|_| Error::Refused)?;
    let status = outbox.status(evidence.action_key).ok_or(Error::Refused)?;
    if status.submission_id != evidence.action_key || status.activity_id != evidence.activity_id {
        return Err(Error::Refused);
    }
    let bytes = outbox
        .exact_signed_bytes(evidence.action_key)
        .map_err(|_| Error::Refused)?;
    let kind = ActivityType::new(ModuleId::Budget, 3).map_err(|_| Error::Refused)?;
    let registration =
        ModuleRegistration::new(ModuleId::Budget, &[kind]).map_err(|_| Error::Refused)?;
    let registry = ModuleRegistry::new(&[registration]).map_err(|_| Error::Refused)?;
    let activity = decode_signed(bytes, &registry).map_err(|_| Error::Refused)?;
    if encode_signed(&activity).map_err(|_| Error::Refused)? != bytes
        || activity.protocol_version() != 3
        || activity.activity_type() != kind
        || activity.idempotency_key() != evidence.action_key
        || activity_id(&activity).map_err(|_| Error::Refused)? != evidence.activity_id
        || activity.payload_hash() != payload_hash(&activity).map_err(|_| Error::Refused)?
    {
        return Err(Error::Refused);
    }
    Ok(activity)
}

fn bind_policy(
    agent: &ManagedAgent,
    activity: &Activity,
    (amount, currency, budget): (u128, &str, [u8; 32]),
) -> Result<(), Error> {
    let context = &agent.context;
    if amount == 0
        || amount > context.amount_ceiling
        || amount < agent.spent
        || currency != agent.currency
        || currency != context.currency
        || budget != agent.active_budget_id
        || budget == [0; 32]
        || context.budget_asset == [0; 32]
        || context.purpose_hash == [0; 32]
        || activity.network_id() != context.network_id
        || activity.actor_did() != context.actor.as_bytes()
        || activity.authority() != context.custody_public_key
    {
        return Err(Error::Refused);
    }
    let expiry_ms = context
        .period_start
        .checked_add(context.budget_expiry_seconds)
        .and_then(|value| value.checked_mul(1_000))
        .ok_or(Error::Refused)?;
    let expected = DisclosedNativeBudgetAmend {
        budget_id: budget,
        per_period_limit: amount,
        carry_cap: 0,
        expiry_ms,
        rollover: 1,
    };
    expected
        .verify_payload(activity.payload())
        .map_err(|_| Error::Refused)?;
    if activity.timestamp_bound().not_before >= expiry_ms {
        return Err(Error::Refused);
    }
    let signature = activity
        .signature()
        .ok_or(Error::Refused)?
        .try_into()
        .map_err(|_| Error::Refused)?;
    let mut digest = Sha256::new();
    digest.update(Domain::SignaturePreimage.tag());
    digest.update(encode_unsigned(activity).map_err(|_| Error::Refused)?);
    layerx_crypto::ed25519::verify_digest(
        &context.custody_public_key,
        &signature,
        &digest.finalize().into(),
    )
    .map_err(|_| Error::Refused)
}

fn receipt(
    store: &Store,
    tenant: &TenantId,
    evidence: FinalizationEvidence,
    activity: &Activity,
) -> Result<ServedReceipt, Error> {
    if evidence.action_key == [0; 32]
        || evidence.activity_id == [0; 32]
        || evidence.receipt_digest == [0; 32]
        || evidence.observed_sequence == 0
        || evidence.finalized_at == 0
        || evidence.verification < VerificationLevel::CHECKPOINT_FINALISED.wire_rank()
        || evidence.verification > VerificationLevel::SETTLEMENT_ANCHORED.wire_rank()
    {
        return Err(Error::Refused);
    }
    let served = crate::receipt::serve(
        store,
        tenant.clone(),
        ReceiptLookupKey::Idempotency(evidence.action_key),
    )
    .map_err(|_| Error::Refused)?;
    for lookup in [
        ReceiptLookupKey::Activity(evidence.activity_id),
        ReceiptLookupKey::GlobalSequence(evidence.observed_sequence),
    ] {
        if crate::receipt::serve(store, tenant.clone(), lookup).map_err(|_| Error::Refused)?
            != served
        {
            return Err(Error::Refused);
        }
    }
    let value = decode_receipt(&served.canonical_bytes).map_err(|_| Error::Refused)?;
    let protocol = value.protocol().ok_or(Error::Refused)?;
    let digest: [u8; 32] = Sha256::digest(&served.canonical_bytes).into();
    if served.metadata.activity_id != evidence.activity_id
        || served.metadata.idempotency_key != evidence.action_key
        || served.metadata.global_sequence != evidence.observed_sequence
        || served.metadata.result.code.raw() != 0
        || served.metadata.verification_level < VerificationLevel::CHECKPOINT_FINALISED
        || served.metadata.verification_level.wire_rank() != evidence.verification
        || digest != evidence.receipt_digest
        || protocol.protocol_version() != 3
        || protocol.module_id() != ModuleId::Budget as u16
        || protocol.module_version() != 1
        || protocol.activity_id() != evidence.activity_id
        || protocol.global_sequence() != evidence.observed_sequence
        || protocol.result_code() != 0
        || protocol.timestamp() == 0
        || protocol.timestamp() / 1_000 > evidence.finalized_at
        || !(activity.timestamp_bound().not_before..=activity.timestamp_bound().not_after)
            .contains(&protocol.timestamp())
        || protocol.fee_charged() > activity.fee_limit()
    {
        return Err(Error::Refused);
    }
    projection(protocol)?;
    Ok(served)
}

fn projection(protocol: &ProtocolReceipt) -> Result<(), Error> {
    if protocol.operation() != 0
        || protocol.asset() != [0; 32]
        || protocol.amount() != 0
        || protocol.from() != [0; 32]
        || protocol.to() != [0; 32]
        || protocol.debit_sequence() != 0
        || protocol.debit_balance_before() != 0
        || protocol.debit_balance_after() != 0
        || protocol.credit_balance_before() != 0
        || protocol.credit_balance_after() != 0
        || protocol.transfer_set_root() != [0; 32]
        || protocol.authorization_hash() != [0; 32]
        || protocol.context_hash() != [0; 32]
        || protocol.program_outcome().is_some()
        || protocol.total_units().is_some()
        || protocol
            .effects()
            .iter()
            .any(layerx_wire::receipt::Effect::monetary)
    {
        return Err(Error::Refused);
    }
    Ok(())
}

pub(super) fn finalize(
    store: &mut Store,
    tenant: &TenantId,
    agent_id: &str,
    request: (u128, &str, [u8; 32]),
    evidence: FinalizationEvidence,
) -> Result<HumanResponse, Error> {
    let activity = source(store, tenant, evidence)?;
    let served = receipt(store, tenant, evidence, &activity)?;
    let mut operation = vec![27, 3];
    operation.extend_from_slice(&request.0.to_be_bytes());
    operation.extend_from_slice(request.1.as_bytes());
    operation.extend_from_slice(&request.2);
    let digest = finalization_digest(agent_id, &operation, evidence)?;
    let action = action_record_key(tenant, evidence.action_key)?;
    if let Some(saved) = store.get(&action) {
        if saved.class() != StorageClass::LocalOnly
            || saved.bytes().len() <= 32
            || saved.bytes()[..32] != digest
        {
            return Err(Error::Refused);
        }
        return HumanResponse::new(saved.bytes()[32..].to_vec()).map_err(|_| Error::Refused);
    }
    let aggregate = agent_key(tenant, agent_id)?;
    let value = store.get(&aggregate).ok_or(Error::Refused)?;
    if value.class() != StorageClass::LocalOnly {
        return Err(Error::Refused);
    }
    let mut agent = decode(value.bytes())?;
    if agent.state == 4 || evidence.finalized_at < agent.updated_at {
        return Err(Error::Refused);
    }
    super::native_budget::scope(store, tenant, request.2)?;
    bind_policy(&agent, &activity, request)?;
    agent.monthly_limit = request.0;
    agent.updated_at = evidence.finalized_at;
    agent
        .verified_evidence
        .push(Sha256::digest(&served.canonical_bytes).into());
    agent.validate()?;
    let response = response_agent(&agent)?;
    let mut completed = digest.to_vec();
    completed.extend_from_slice(response.bytes());
    store
        .update_local_with_companion(aggregate, encode(&agent)?, action, completed)
        .map_err(|_| Error::Unavailable)?;
    Ok(response)
}
