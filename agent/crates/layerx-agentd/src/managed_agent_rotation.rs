use super::*;
use crate::identity::{CoreIdentity, ProtocolAuthority};
use crate::session::SessionRegistry;
use layerx_crypto::rotation::{OwnerRotation, OwnerRotationState};
use layerx_proof::receipt::{
    verify_native_owner_outcome, AuthorizedBatch, NativeOwnerOutcomeContext,
};
use layerx_types::ids::Did;
use layerx_types::payload::{ActivityType, ModuleRegistry};

pub(crate) struct Projection<'a> {
    pub custody_key: &'a str,
    pub signed_activity: &'a [u8],
    pub evidence: FinalizationEvidence,
    pub identity: &'a CoreIdentity,
    pub authority: &'a AuthorizedBatch,
    pub registry: &'a ModuleRegistry,
}

pub(crate) enum Prepared {
    Replay(HumanResponse),
    Commit {
        event: RevocationEvent,
        updates: Vec<(crate::store::TenantKey, Vec<u8>)>,
        response: HumanResponse,
    },
}

pub(crate) fn prepare(
    store: &Store,
    sessions: &SessionRegistry,
    tenant: &TenantId,
    agent_id: &str,
    request: &Projection<'_>,
) -> Result<Prepared, HumanOperationError> {
    let evidence = request.evidence;
    if request.custody_key.is_empty()
        || request.custody_key.len() > 256
        || request.custody_key.bytes().any(|b| b.is_ascii_control())
        || request.signed_activity.len() > 16_384
        || evidence.action_key == [0; 32]
        || evidence.activity_id == [0; 32]
        || evidence.receipt_digest == [0; 32]
        || evidence.observed_sequence == 0
        || !(4..=5).contains(&evidence.verification)
        || evidence.finalized_at == 0
    {
        return Err(HumanOperationError::Refused);
    }
    let mut operation = vec![43];
    operation.extend_from_slice(&(request.custody_key.len() as u64).to_be_bytes());
    operation.extend_from_slice(request.custody_key.as_bytes());
    operation.extend_from_slice(request.signed_activity);
    let binding = finalization_digest(agent_id, &operation, evidence)?;
    let replay_key = action_record_key(tenant, evidence.action_key)?;
    if let Some(row) = store.get(&replay_key) {
        if row.class() != StorageClass::LocalOnly
            || row.bytes().get(..32) != Some(binding.as_slice())
        {
            return Err(HumanOperationError::Refused);
        }
        return Ok(Prepared::Replay(
            HumanResponse::new(row.bytes()[32..].to_vec())
                .map_err(|_| HumanOperationError::Refused)?,
        ));
    }
    let mut agent = load_agent(store, tenant, agent_id)?;
    if agent.state == 4 || evidence.finalized_at < agent.updated_at {
        return Err(HumanOperationError::Refused);
    }
    let did = Did::new(agent.agent_did.as_bytes()).map_err(|_| HumanOperationError::Refused)?;
    let activity = layerx_wire::activity::decode_signed(request.signed_activity, request.registry)
        .map_err(|_| HumanOperationError::Refused)?;
    let OwnerRotation::Commit(commit) =
        OwnerRotation::from_activity(&activity).map_err(|_| HumanOperationError::Refused)?
    else {
        return Err(HumanOperationError::Refused);
    };
    if commit.network_id != agent.context.network_id
        || commit.consent.owner != did
        || commit.consent.current_public_key != agent.context.custody_public_key
        || commit.consent.action_key != evidence.action_key
    {
        return Err(HumanOperationError::Refused);
    }
    let stored = crate::receipt::serve(
        store,
        tenant.clone(),
        crate::receipt::ReceiptLookupKey::Idempotency(evidence.action_key),
    )
    .map_err(|_| HumanOperationError::Refused)?;
    let verified = verify_native_owner_outcome(
        &stored.canonical_bytes,
        request.authority,
        &NativeOwnerOutcomeContext {
            canonical_activity: request.signed_activity,
            actor: did.as_bytes(),
            action_key: evidence.action_key,
            activity_type: ActivityType::new(ModuleId::Governance, 2)
                .map_err(|_| HumanOperationError::Refused)?,
            owner_public_key: agent.context.custody_public_key,
            network_id: agent.context.network_id,
        },
    )
    .map_err(|_| HumanOperationError::Refused)?;
    let protocol = verified
        .receipt()
        .protocol()
        .ok_or(HumanOperationError::Refused)?;
    if stored.metadata.verification_level.wire_rank() < evidence.verification
        || stored.metadata.activity_id != evidence.activity_id
        || protocol.activity_id() != evidence.activity_id
        || protocol.result_code() != 0
        || protocol.global_sequence() != evidence.observed_sequence
        || <[u8; 32]>::from(Sha256::digest(&stored.canonical_bytes)) != evidence.receipt_digest
    {
        return Err(HumanOperationError::Refused);
    }
    let owner_id = layerx_wire::hash::did_id_for_protocol(&did, 3)
        .map_err(|_| HumanOperationError::Refused)?;
    let mut expected = b"LXOR1".to_vec();
    expected.extend_from_slice(&owner_id);
    expected.extend_from_slice(&commit.consent.current_public_key);
    expected.extend_from_slice(&commit.consent.pending_public_key);
    expected.extend_from_slice(&evidence.observed_sequence.to_be_bytes());
    expected.extend_from_slice(&commit.consent.announcement);
    let effects: Vec<_> = protocol
        .effects()
        .iter()
        .filter(|effect| effect.event_type() == 0x7142)
        .collect();
    if effects.len() != 1
        || effects[0].kind() != 3
        || effects[0].module_id() != 7
        || effects[0].monetary()
        || effects[0].body() != expected
    {
        return Err(HumanOperationError::Refused);
    }
    let state = OwnerRotationState::decode(&request.identity.canonical_bytes, &did)
        .map_err(|_| HumanOperationError::Refused)?;
    if request.identity.verification_level < VerificationLevel::CHECKPOINT_FINALISED
        || request.identity.frozen
        || request.identity.head_sequence < evidence.observed_sequence
        || state.primary_public_key != commit.consent.pending_public_key
        || state.pending_public_key.is_some()
        || state.revocation_sequence != evidence.observed_sequence
        || request.identity.revocation_sequence != state.revocation_sequence
        || state.observed_sequence > request.identity.head_sequence
        || !request
            .identity
            .authorities
            .contains(&ProtocolAuthority::PrimaryKey(state.primary_public_key))
    {
        return Err(HumanOperationError::Refused);
    }
    let session = sessions
        .get(tenant, SessionId(agent.session_id))
        .ok_or(HumanOperationError::Refused)?;
    if session.request.agent != did
        || session.request.token_id != agent.session_token_id
        || session.generation != agent.session_generation
    {
        return Err(HumanOperationError::Refused);
    }
    if session.open {
        agent.session_generation = agent
            .session_generation
            .checked_add(1)
            .ok_or(HumanOperationError::Refused)?;
    }
    agent.context.actor = agent.agent_did.clone();
    agent.context.custody_key = request.custody_key.to_owned();
    agent.context.custody_public_key = state.primary_public_key;
    agent.context.primary_authority = hex(&state.primary_public_key);
    agent.updated_at = evidence.finalized_at;
    agent.verified_evidence.push(evidence.receipt_digest);
    agent.validate()?;
    let response = journey_response(&agent, 1, evidence)?;
    let mut replay = binding.to_vec();
    replay.extend_from_slice(response.bytes());
    let aggregate_key = agent_key(tenant, agent_id)?;
    let mut updates = related_generations(store, sessions, &did, &aggregate_key)?;
    updates.push((aggregate_key, encode(&agent)?));
    updates.push((replay_key, replay));
    Ok(Prepared::Commit {
        event: RevocationEvent {
            did,
            authority: None,
            reason: InvalidationReason::PrimaryKeyRotated,
            observed_sequence: evidence.observed_sequence,
        },
        updates,
        response,
    })
}

fn related_generations(
    store: &Store,
    sessions: &SessionRegistry,
    did: &Did,
    primary: &crate::store::TenantKey,
) -> Result<Vec<(crate::store::TenantKey, Vec<u8>)>, HumanOperationError> {
    let mut updates = Vec::new();
    for tenant in store.tenant_ids_for_kind(ObjectKind::Configuration) {
        for object_id in store.list_object_ids(&tenant, ObjectKind::Configuration) {
            if !object_id.starts_with(PREFIX) {
                continue;
            }
            let object_key = key(tenant.clone(), ObjectKind::Configuration, object_id)
                .map_err(|_| HumanOperationError::Refused)?;
            if &object_key == primary {
                continue;
            }
            let value = store.get(&object_key).ok_or(HumanOperationError::Refused)?;
            if value.class() != StorageClass::LocalOnly {
                return Err(HumanOperationError::Refused);
            }
            let mut other = decode(value.bytes())?;
            if other.agent_did.as_bytes() != did.as_bytes() {
                continue;
            }
            let session = sessions
                .get(&tenant, SessionId(other.session_id))
                .ok_or(HumanOperationError::Refused)?;
            if session.request.agent != *did
                || session.request.token_id != other.session_token_id
                || session.generation != other.session_generation
            {
                return Err(HumanOperationError::Refused);
            }
            if session.open {
                other.session_generation = other
                    .session_generation
                    .checked_add(1)
                    .ok_or(HumanOperationError::Refused)?;
                updates.push((object_key, encode(&other)?));
            }
        }
    }
    Ok(updates)
}
