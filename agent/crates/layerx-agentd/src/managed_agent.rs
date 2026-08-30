//! Durable tenant-owned projection of agents created through the Human plane.

use layerx_types::payload::ModuleId;
use layerx_types::verify::VerificationLevel;
use layerx_wire::receipt::decode as decode_receipt;
use sha2::{Digest, Sha256};

use crate::human::{HumanAgentLifecycleSeed, HumanOperationError, HumanResponse};
use crate::store::{key, ObjectKind, StorageClass, Store, TenantId};

const PREFIX: &[u8] = b"managed-agent-v1:";
const VERSION: u8 = 3;
const MAX_AGENTS_PER_PAGE: u8 = 100;
const MAX_EVIDENCE: usize = 64;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct FinalizationEvidence {
    pub action_key: [u8; 32],
    pub activity_id: [u8; 32],
    pub receipt_digest: [u8; 32],
    pub observed_sequence: u64,
    pub verification: u8,
    pub finalized_at: u64,
}
impl From<crate::human::HumanFinalizationEvidence> for FinalizationEvidence {
    fn from(value: crate::human::HumanFinalizationEvidence) -> Self {
        Self {
            action_key: value.action_key,
            activity_id: value.activity_id,
            receipt_digest: value.receipt_digest,
            observed_sequence: value.observed_sequence,
            verification: value.verification,
            finalized_at: value.finalized_at,
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ManagedAgent {
    pub agent_id: String,
    pub name: String,
    pub purpose: String,
    pub state: u8,
    pub monthly_limit: u128,
    pub currency: String,
    pub period_start: u64,
    pub period_end: u64,
    pub spent: u128,
    pub created_at: u64,
    pub updated_at: u64,
    pub verified_evidence: Vec<[u8; 32]>,
    pub context: HumanAgentLifecycleSeed,
    pub agent_did: String,
    pub session_id: [u8; 32],
    pub session_token_id: [u8; 32],
    pub protocol_grant_id: [u8; 32],
    pub active_budget_id: [u8; 32],
}

impl ManagedAgent {
    pub fn from_creation(
        seed: &HumanAgentLifecycleSeed,
        installed_agent: &str,
        session_id: [u8; 32],
        session_token_id: [u8; 32],
    ) -> Result<Self, HumanOperationError> {
        if installed_agent.is_empty()
            || session_id == [0; 32]
            || session_token_id == [0; 32]
            || seed.agent_id.is_empty()
            || seed.name.is_empty()
            || seed.purpose.is_empty()
            || seed.currency.is_empty()
            || seed.monthly_limit == 0
            || seed.period_end <= seed.period_start
            || seed.updated_at < seed.created_at
            || seed.verified_evidence.is_empty()
            || seed.verified_evidence.len() > MAX_EVIDENCE
            || seed.verified_evidence.iter().any(|value| *value == [0; 32])
        {
            return Err(HumanOperationError::Refused);
        }
        validate_context(seed)?;
        let active_budget_id = parse_agent_digest(&seed.agent_id)?;
        Ok(Self {
            agent_id: seed.agent_id.clone(),
            name: seed.name.clone(),
            purpose: seed.purpose.clone(),
            state: 1,
            monthly_limit: seed.monthly_limit,
            currency: seed.currency.clone(),
            period_start: seed.period_start,
            period_end: seed.period_end,
            spent: 0,
            created_at: seed.created_at,
            updated_at: seed.updated_at,
            verified_evidence: seed.verified_evidence.clone(),
            context: seed.clone(),
            agent_did: installed_agent.to_owned(),
            session_id,
            session_token_id,
            protocol_grant_id: seed.protocol_grant_id,
            active_budget_id,
        })
    }

    fn validate(&self) -> Result<(), HumanOperationError> {
        if self.active_budget_id == [0; 32] {
            return Err(HumanOperationError::Refused);
        }
        if self.agent_id.is_empty()
            || self.agent_did.is_empty()
            || self.session_id == [0; 32]
            || self.session_token_id == [0; 32]
            || self.name.is_empty()
            || self.purpose.is_empty()
            || self.currency.is_empty()
            || self.state > 4
            || self.monthly_limit == 0
            || self.period_end <= self.period_start
            || self.updated_at < self.created_at
            || self.spent > self.monthly_limit
            || self.verified_evidence.is_empty()
            || self.verified_evidence.len() > MAX_EVIDENCE
            || self.verified_evidence.iter().any(|value| *value == [0; 32])
            || self.context.agent_id != self.agent_id
        {
            return Err(HumanOperationError::Refused);
        }
        validate_context(&self.context)?;
        Ok(())
    }
}

pub fn publish_creation(
    store: &mut Store,
    tenant: &TenantId,
    agent: ManagedAgent,
) -> Result<(), HumanOperationError> {
    agent.validate()?;
    let object_key = agent_key(tenant, &agent.agent_id)?;
    if let Some(existing) = store.get(&object_key) {
        if existing.class() != StorageClass::LocalOnly || decode(existing.bytes())? != agent {
            return Err(HumanOperationError::Refused);
        }
        return Ok(());
    }
    store
        .put_local(object_key, encode(&agent)?)
        .map_err(|_| HumanOperationError::Unavailable)
}
pub fn publish_creation_with_companion(
    store: &mut Store,
    tenant: &TenantId,
    agent: ManagedAgent,
    companion: crate::store::TenantKey,
    bytes: Vec<u8>,
) -> Result<(), HumanOperationError> {
    agent.validate()?;
    let object_key = agent_key(tenant, &agent.agent_id)?;
    if let Some(existing) = store.get(&object_key) {
        if existing.class() != StorageClass::LocalOnly || decode(existing.bytes())? != agent {
            return Err(HumanOperationError::Refused);
        }
        store
            .put_local(companion, bytes)
            .map_err(|_| HumanOperationError::Unavailable)?;
    } else {
        store
            .update_local_with_companion(object_key, encode(&agent)?, companion, bytes)
            .map_err(|_| HumanOperationError::Unavailable)?;
    }
    Ok(())
}

pub fn validate_tenant(store: &Store, tenant: &TenantId) -> Result<(), HumanOperationError> {
    for object_id in store.list_object_ids(tenant, ObjectKind::Configuration) {
        if !object_id.starts_with(PREFIX) {
            continue;
        }
        let object_key = key(tenant.clone(), ObjectKind::Configuration, object_id)
            .map_err(|_| HumanOperationError::Refused)?;
        let value = store
            .get(&object_key)
            .ok_or(HumanOperationError::Unavailable)?;
        if value.class() != StorageClass::LocalOnly {
            return Err(HumanOperationError::Refused);
        }
        let agent = decode(value.bytes())?;
        if agent_key(tenant, &agent.agent_id)? != object_key {
            return Err(HumanOperationError::Refused);
        }
    }
    Ok(())
}

pub fn get(
    store: &Store,
    tenant: &TenantId,
    agent_id: &str,
) -> Result<HumanResponse, HumanOperationError> {
    let object_key = agent_key(tenant, agent_id)?;
    let value = store.get(&object_key).ok_or(HumanOperationError::Refused)?;
    if value.class() != StorageClass::LocalOnly {
        return Err(HumanOperationError::Refused);
    }
    response_agent(&decode(value.bytes())?)
}
pub fn context(
    store: &Store,
    tenant: &TenantId,
    agent_id: &str,
) -> Result<HumanResponse, HumanOperationError> {
    let object = store
        .get(&agent_key(tenant, agent_id)?)
        .ok_or(HumanOperationError::Refused)?;
    if object.class() != StorageClass::LocalOnly {
        return Err(HumanOperationError::Refused);
    }
    let agent = decode(object.bytes())?;
    let mut out = Wire::new();
    encode_context(&mut out, &agent.context)?;
    out.text(&agent.agent_did)?;
    out.fixed(&agent.session_id);
    out.fixed(&agent.session_token_id);
    out.fixed(&agent.protocol_grant_id);
    out.fixed(&agent.active_budget_id);
    out.u8(agent.state);
    out.u128(agent.monthly_limit);
    out.u128(agent.spent);
    out.u64(agent.updated_at);
    out.finish()
}

pub fn record_observation(
    store: &mut Store,
    tenant: &TenantId,
    class: u8,
    digest: [u8; 32],
    bytes: Vec<u8>,
) -> Result<(), HumanOperationError> {
    if digest == [0; 32] || bytes.is_empty() {
        return Err(HumanOperationError::Refused);
    }
    let mut id = b"managed-agent-observation-v1:".to_vec();
    id.push(class);
    id.extend_from_slice(&digest);
    let record = key(tenant.clone(), ObjectKind::Configuration, id)
        .map_err(|_| HumanOperationError::Refused)?;
    if let Some(existing) = store.get(&record) {
        if existing.class() != StorageClass::LocalOnly || existing.bytes() != bytes {
            return Err(HumanOperationError::Refused);
        }
        return Ok(());
    }
    store
        .put_local(record, bytes)
        .map_err(|_| HumanOperationError::Unavailable)
}
fn budget_observation(
    store: &Store,
    tenant: &TenantId,
    digest: [u8; 32],
    budget: [u8; 32],
    minimum_sequence: u64,
    require_zero: bool,
) -> Result<(), HumanOperationError> {
    if digest == [0; 32] {
        return Err(HumanOperationError::Refused);
    }
    let mut id = b"managed-agent-observation-v1:".to_vec();
    id.push(1);
    id.extend_from_slice(&digest);
    let record = key(tenant.clone(), ObjectKind::Configuration, id)
        .map_err(|_| HumanOperationError::Refused)?;
    let bytes = store
        .get(&record)
        .ok_or(HumanOperationError::Unavailable)?
        .bytes();
    if bytes.len() != 209
        || bytes[..32] != budget
        || bytes[49..81] != digest
        || bytes[48] < 4
        || bytes[48] > 5
        || bytes[81..113] == [0; 32]
        || bytes[113..145] == [0; 32]
        || bytes[177..209] == [0; 32]
    {
        return Err(HumanOperationError::Refused);
    }
    let observed = u64::from_be_bytes(
        bytes[40..48]
            .try_into()
            .map_err(|_| HumanOperationError::Refused)?,
    );
    let age = u64::from_be_bytes(
        bytes[145..153]
            .try_into()
            .map_err(|_| HumanOperationError::Refused)?,
    );
    let maximum = u64::from_be_bytes(
        bytes[153..161]
            .try_into()
            .map_err(|_| HumanOperationError::Refused)?,
    );
    let remaining = u128::from_be_bytes(
        bytes[161..177]
            .try_into()
            .map_err(|_| HumanOperationError::Refused)?,
    );
    if observed < minimum_sequence
        || maximum == 0
        || age > maximum
        || (require_zero && remaining != 0)
    {
        return Err(HumanOperationError::Refused);
    }
    Ok(())
}

fn key_policy_observation(
    store: &Store,
    tenant: &TenantId,
    digest: [u8; 32],
    did: &str,
    recovery: bool,
    delay: u64,
    ready_at: u64,
    finalized_at: u64,
) -> Result<(), HumanOperationError> {
    let mut id = b"managed-agent-observation-v1:".to_vec();
    id.push(2);
    id.extend_from_slice(&digest);
    let record = key(tenant.clone(), ObjectKind::Configuration, id)
        .map_err(|_| HumanOperationError::Refused)?;
    let bytes = store
        .get(&record)
        .ok_or(HumanOperationError::Unavailable)?
        .bytes();
    let mut input = Input { bytes, offset: 0 };
    if input.text()? != did || input.u8()? != u8::from(recovery) {
        return Err(HumanOperationError::Refused);
    }
    let revision = input.u64()?;
    let required = input.u64()?;
    let maximum = input.u64()?;
    let effective = input.u64()?;
    let observed = input.u64()?;
    let verification = input.u8()?;
    let observed_digest = input.fixed()?;
    let checkpoint = input.fixed()?;
    let age = input.u64()?;
    let maximum_age = input.u64()?;
    if input.offset != bytes.len()
        || revision == 0
        || delay != required
        || delay > maximum
        || ready_at
            != finalized_at
                .checked_add(delay)
                .ok_or(HumanOperationError::Refused)?
        || effective == 0
        || observed < effective
        || verification < 4
        || observed_digest != digest
        || checkpoint == [0; 32]
        || maximum_age == 0
        || age > maximum_age
    {
        return Err(HumanOperationError::Refused);
    }
    Ok(())
}

fn require_session_observation(
    store: &Store,
    tenant: &TenantId,
    agent_id: &str,
    digest: [u8; 32],
    resume: bool,
) -> Result<(), HumanOperationError> {
    let mut id = b"managed-agent-observation-v1:".to_vec();
    id.push(3);
    id.extend_from_slice(&digest);
    let record = key(tenant.clone(), ObjectKind::Configuration, id)
        .map_err(|_| HumanOperationError::Refused)?;
    let bytes = store
        .get(&record)
        .ok_or(HumanOperationError::Unavailable)?
        .bytes();
    let mut input = Input { bytes, offset: 0 };
    if input.text()? != agent_id {
        return Err(HumanOperationError::Refused);
    }
    let did = input.text()?;
    let session = input.fixed()?;
    let token = input.fixed()?;
    let open = input.u8()?;
    let action = input.fixed()?;
    let observed = input.fixed()?;
    if input.offset != bytes.len()
        || observed != digest
        || open != u8::from(resume)
        || action == [0; 32]
    {
        return Err(HumanOperationError::Refused);
    }
    let agent = load_agent(store, tenant, agent_id)?;
    if agent.agent_did != did || agent.session_id != session || agent.session_token_id != token {
        return Err(HumanOperationError::Refused);
    }
    Ok(())
}
pub fn finalize_control(
    store: &mut Store,
    tenant: &TenantId,
    agent_id: &str,
    resume: bool,
    session_observation: [u8; 32],
    evidence: FinalizationEvidence,
) -> Result<HumanResponse, HumanOperationError> {
    require_session_observation(store, tenant, agent_id, session_observation, resume)?;
    let mut operation = vec![26, u8::from(resume)];
    operation.extend_from_slice(&session_observation);
    finalize_agent(
        store,
        tenant,
        agent_id,
        evidence,
        6,
        &operation,
        |agent| {
            if resume {
                if agent.state != 2 {
                    return Err(HumanOperationError::Refused);
                }
                agent.state = 1;
            } else {
                if agent.state != 1 {
                    return Err(HumanOperationError::Refused);
                }
                agent.state = 2;
            }
            Ok(())
        },
        response_agent,
    )
}

pub fn finalize_limit(
    store: &mut Store,
    tenant: &TenantId,
    agent_id: &str,
    monthly_limit: u128,
    currency: &str,
    replacement_budget_id: [u8; 32],
    evidence: FinalizationEvidence,
) -> Result<HumanResponse, HumanOperationError> {
    if monthly_limit == 0 || currency.is_empty() || replacement_budget_id == [0; 32] {
        return Err(HumanOperationError::Refused);
    }
    let mut body = Vec::new();
    body.push(27);
    body.extend_from_slice(&monthly_limit.to_be_bytes());
    body.extend_from_slice(currency.as_bytes());
    body.extend_from_slice(&replacement_budget_id);
    finalize_agent(
        store,
        tenant,
        agent_id,
        evidence,
        1,
        &body,
        |agent| {
            if agent.state == 4 || agent.currency != currency || agent.spent > monthly_limit {
                return Err(HumanOperationError::Refused);
            }
            agent.monthly_limit = monthly_limit;
            agent.active_budget_id = replacement_budget_id;
            Ok(())
        },
        response_agent,
    )
}

pub fn finalize_journey(
    store: &mut Store,
    tenant: &TenantId,
    agent_id: &str,
    kind: u8,
    amount: u128,
    currency: &str,
    delay: u64,
    ready_at: u64,
    pre_observation: [u8; 32],
    post_observation: [u8; 32],
    evidence: FinalizationEvidence,
) -> Result<HumanResponse, HumanOperationError> {
    if kind > 2
        || (kind == 0 && (amount == 0 || currency.is_empty() || delay != 0 || ready_at != 0))
        || (kind != 0
            && (amount != 0
                || !currency.is_empty()
                || delay == 0
                || ready_at <= evidence.finalized_at))
    {
        return Err(HumanOperationError::Refused);
    }
    if kind == 0 && pre_observation == post_observation {
        return Err(HumanOperationError::Refused);
    }
    if kind == 0 {
        let agent = load_agent(store, tenant, agent_id)?;
        budget_observation(
            store,
            tenant,
            pre_observation,
            agent.active_budget_id,
            0,
            false,
        )?;
        budget_observation(
            store,
            tenant,
            post_observation,
            agent.active_budget_id,
            evidence.observed_sequence,
            false,
        )?
    } else {
        if pre_observation == [0; 32] || post_observation != [0; 32] {
            return Err(HumanOperationError::Refused);
        }
        let agent = load_agent(store, tenant, agent_id)?;
        key_policy_observation(
            store,
            tenant,
            pre_observation,
            &agent.agent_did,
            kind == 2,
            delay,
            ready_at,
            evidence.finalized_at,
        )?
    }
    let mut body = Vec::new();
    body.push(28);
    body.push(kind);
    body.extend_from_slice(&amount.to_be_bytes());
    body.extend_from_slice(currency.as_bytes());
    body.extend_from_slice(&delay.to_be_bytes());
    body.extend_from_slice(&ready_at.to_be_bytes());
    body.extend_from_slice(&pre_observation);
    body.extend_from_slice(&post_observation);
    finalize_agent(
        store,
        tenant,
        agent_id,
        evidence,
        match kind {
            0 => 3,
            1 | 2 => 2,
            _ => return Err(HumanOperationError::Refused),
        },
        &body,
        |agent| {
            if agent.state == 4 || (kind == 0 && agent.currency != currency) {
                return Err(HumanOperationError::Refused);
            }
            Ok(())
        },
        |agent| {
            if kind == 0 {
                journey_response(agent, kind, evidence)
            } else {
                challenge_response(agent, kind, delay, ready_at, evidence)
            }
        },
    )
}

pub fn finalize_archive(
    store: &mut Store,
    tenant: &TenantId,
    agent_id: &str,
    confirm_name: &str,
    pre_observation: [u8; 32],
    post_observation: [u8; 32],
    session_observation: [u8; 32],
    evidence: FinalizationEvidence,
) -> Result<HumanResponse, HumanOperationError> {
    if pre_observation == post_observation {
        return Err(HumanOperationError::Refused);
    }
    let current = load_agent(store, tenant, agent_id)?;
    budget_observation(
        store,
        tenant,
        pre_observation,
        current.active_budget_id,
        0,
        false,
    )?;
    budget_observation(
        store,
        tenant,
        post_observation,
        current.active_budget_id,
        evidence.observed_sequence,
        true,
    )?;
    require_session_observation(store, tenant, agent_id, session_observation, false)?;
    let mut body = Vec::new();
    body.push(29);
    body.extend_from_slice(confirm_name.as_bytes());
    body.extend_from_slice(&pre_observation);
    body.extend_from_slice(&post_observation);
    body.extend_from_slice(&session_observation);
    finalize_agent(
        store,
        tenant,
        agent_id,
        evidence,
        6,
        &body,
        |agent| {
            if agent.state == 4 || agent.name != confirm_name {
                return Err(HumanOperationError::Refused);
            }
            agent.state = 4;
            Ok(())
        },
        |agent| journey_response(agent, 3, evidence),
    )
}
fn load_agent(
    store: &Store,
    tenant: &TenantId,
    agent_id: &str,
) -> Result<ManagedAgent, HumanOperationError> {
    let value = store
        .get(&agent_key(tenant, agent_id)?)
        .ok_or(HumanOperationError::Refused)?;
    if value.class() != StorageClass::LocalOnly {
        return Err(HumanOperationError::Refused);
    }
    decode(value.bytes())
}

pub fn session_coordinates(
    store: &Store,
    tenant: &TenantId,
    agent_id: &str,
) -> Result<(String, [u8; 32], [u8; 32]), HumanOperationError> {
    let agent = load_agent(store, tenant, agent_id)?;
    Ok((agent.agent_did, agent.session_id, agent.session_token_id))
}
pub fn protocol_grant(
    store: &Store,
    tenant: &TenantId,
    agent_id: &str,
) -> Result<[u8; 32], HumanOperationError> {
    let agent = load_agent(store, tenant, agent_id)?;
    if agent.protocol_grant_id == [0; 32] {
        Err(HumanOperationError::Refused)
    } else {
        Ok(agent.protocol_grant_id)
    }
}
fn session_bytes(
    agent: &ManagedAgent,
    action: [u8; 32],
    open: bool,
) -> Result<(HumanResponse, [u8; 32]), HumanOperationError> {
    if action == [0; 32] {
        return Err(HumanOperationError::Refused);
    }
    let mut out = Wire::new();
    out.text(&agent.agent_id)?;
    out.text(&agent.agent_did)?;
    out.fixed(&agent.session_id);
    out.fixed(&agent.session_token_id);
    out.u8(u8::from(open));
    out.fixed(&action);
    let digest: [u8; 32] = Sha256::digest(
        [
            b"layerx-agentd/session-observation/v1\0".as_slice(),
            out.0.as_slice(),
        ]
        .concat(),
    )
    .into();
    out.fixed(&digest);
    Ok((out.finish()?, digest))
}
pub fn record_session_observation(
    store: &mut Store,
    tenant: &TenantId,
    agent_id: &str,
    action: [u8; 32],
    open: bool,
    create: bool,
) -> Result<HumanResponse, HumanOperationError> {
    let agent = load_agent(store, tenant, agent_id)?;
    let (response, digest) = session_bytes(&agent, action, open)?;
    let mut id = b"managed-agent-observation-v1:".to_vec();
    id.push(3);
    id.extend_from_slice(&digest);
    let record = key(tenant.clone(), ObjectKind::Configuration, id)
        .map_err(|_| HumanOperationError::Refused)?;
    if let Some(existing) = store.get(&record) {
        if existing.class() != StorageClass::LocalOnly || existing.bytes() != response.bytes() {
            return Err(HumanOperationError::Refused);
        }
        return Ok(response);
    }
    if !create {
        return Err(HumanOperationError::Refused);
    }
    store
        .put_local(record, response.bytes().to_vec())
        .map_err(|_| HumanOperationError::Unavailable)?;
    Ok(response)
}
pub fn prepare_session_observation(
    store: &Store,
    tenant: &TenantId,
    agent_id: &str,
    action: [u8; 32],
    open: bool,
) -> Result<(HumanResponse, crate::store::TenantKey, Vec<u8>), HumanOperationError> {
    let agent = load_agent(store, tenant, agent_id)?;
    let (response, digest) = session_bytes(&agent, action, open)?;
    let mut id = b"managed-agent-observation-v1:".to_vec();
    id.push(3);
    id.extend_from_slice(&digest);
    let record = key(tenant.clone(), ObjectKind::Configuration, id)
        .map_err(|_| HumanOperationError::Refused)?;
    if store.get(&record).is_some() {
        return Err(HumanOperationError::Refused);
    }
    let bytes = response.bytes().to_vec();
    Ok((response, record, bytes))
}
pub fn bind_session(
    store: &mut Store,
    tenant: &TenantId,
    agent_id: &str,
    session: [u8; 32],
    token: [u8; 32],
    grant: [u8; 32],
    action: [u8; 32],
) -> Result<HumanResponse, HumanOperationError> {
    if session == [0; 32] || token == [0; 32] {
        return Err(HumanOperationError::Refused);
    }
    let mut agent = load_agent(store, tenant, agent_id)?;
    agent.session_id = session;
    agent.session_token_id = token;
    agent.protocol_grant_id = grant;
    agent.context.protocol_grant_id = grant;
    let (response, digest) = session_bytes(&agent, action, true)?;
    let mut id = b"managed-agent-observation-v1:".to_vec();
    id.push(3);
    id.extend_from_slice(&digest);
    let record = key(tenant.clone(), ObjectKind::Configuration, id)
        .map_err(|_| HumanOperationError::Refused)?;
    if let Some(existing) = store.get(&record) {
        if existing.class() != StorageClass::LocalOnly || existing.bytes() != response.bytes() {
            return Err(HumanOperationError::Refused);
        }
        let current = load_agent(store, tenant, agent_id)?;
        if current.session_id != session || current.session_token_id != token {
            return Err(HumanOperationError::Refused);
        }
        return Ok(response);
    }
    store
        .update_local_with_companion(
            agent_key(tenant, agent_id)?,
            encode(&agent)?,
            record,
            response.bytes().to_vec(),
        )
        .map_err(|_| HumanOperationError::Unavailable)?;
    Ok(response)
}

fn finalize_agent<F, R>(
    store: &mut Store,
    tenant: &TenantId,
    agent_id: &str,
    evidence: FinalizationEvidence,
    expected_operation: u8,
    operation: &[u8],
    mutate: F,
    response: R,
) -> Result<HumanResponse, HumanOperationError>
where
    F: FnOnce(&mut ManagedAgent) -> Result<(), HumanOperationError>,
    R: FnOnce(&ManagedAgent) -> Result<HumanResponse, HumanOperationError>,
{
    if evidence.action_key == [0; 32]
        || evidence.activity_id != evidence.action_key
        || evidence.receipt_digest == [0; 32]
        || evidence.observed_sequence == 0
        || evidence.verification < 4
        || evidence.verification > 5
        || evidence.finalized_at == 0
    {
        return Err(HumanOperationError::Refused);
    }
    let aggregate_key = agent_key(tenant, agent_id)?;
    let action_key = action_record_key(tenant, evidence.action_key)?;
    let request_digest = finalization_digest(agent_id, operation, evidence);
    if let Some(saved) = store.get(&action_key) {
        if saved.class() != StorageClass::LocalOnly
            || saved.bytes().len() < 32
            || saved.bytes()[..32] != request_digest
        {
            return Err(HumanOperationError::Refused);
        }
        return HumanResponse::new(saved.bytes()[32..].to_vec())
            .map_err(|_| HumanOperationError::Refused);
    }
    let served = crate::receipt::serve(
        store,
        tenant.clone(),
        crate::receipt::ReceiptLookupKey::Idempotency(evidence.action_key),
    )
    .map_err(|error| match error {
        crate::receipt::ReceiptStoreError::Missing => HumanOperationError::Unavailable,
        _ => HumanOperationError::Refused,
    })?;
    let receipt =
        decode_receipt(&served.canonical_bytes).map_err(|_| HumanOperationError::Refused)?;
    let protocol = receipt.protocol().ok_or(HumanOperationError::Refused)?;
    let digest: [u8; 32] = Sha256::digest(&served.canonical_bytes).into();
    if protocol.module_id() != ModuleId::Governance as u16
        || protocol.operation() != expected_operation
        || served.metadata.activity_id != evidence.activity_id
        || served.metadata.idempotency_key != evidence.action_key
        || served.metadata.global_sequence != evidence.observed_sequence
        || served.metadata.result.code.raw() != 0
        || served.metadata.verification_level < VerificationLevel::CHECKPOINT_FINALISED
        || served.metadata.verification_level.wire_rank() != evidence.verification
        || digest != evidence.receipt_digest
    {
        return Err(HumanOperationError::Refused);
    }
    let value = store
        .get(&aggregate_key)
        .ok_or(HumanOperationError::Refused)?;
    if value.class() != StorageClass::LocalOnly {
        return Err(HumanOperationError::Refused);
    }
    let mut agent = decode(value.bytes())?;
    if evidence.finalized_at < agent.updated_at {
        return Err(HumanOperationError::Refused);
    }
    mutate(&mut agent)?;
    agent.updated_at = evidence.finalized_at;
    agent.verified_evidence.push(evidence.receipt_digest);
    agent.validate()?;
    let response = response(&agent)?;
    let mut action = Vec::with_capacity(32 + response.bytes().len());
    action.extend_from_slice(&request_digest);
    action.extend_from_slice(response.bytes());
    store
        .update_local_with_companion(aggregate_key, encode(&agent)?, action_key, action)
        .map_err(|_| HumanOperationError::Unavailable)?;
    Ok(response)
}

fn action_record_key(
    tenant: &TenantId,
    action: [u8; 32],
) -> Result<crate::store::TenantKey, HumanOperationError> {
    let mut id = b"managed-agent-action-v1:".to_vec();
    id.extend_from_slice(&action);
    key(tenant.clone(), ObjectKind::Idempotency, id).map_err(|_| HumanOperationError::Refused)
}
fn finalization_digest(agent: &str, operation: &[u8], evidence: FinalizationEvidence) -> [u8; 32] {
    let mut h = Sha256::new();
    h.update(b"layerx-agentd/managed-agent-finalize/v1\0");
    h.update((agent.len() as u32).to_be_bytes());
    h.update(agent.as_bytes());
    h.update((operation.len() as u32).to_be_bytes());
    h.update(operation);
    h.update(evidence.action_key);
    h.update(evidence.activity_id);
    h.update(evidence.receipt_digest);
    h.update(evidence.observed_sequence.to_be_bytes());
    h.update([evidence.verification]);
    h.update(evidence.finalized_at.to_be_bytes());
    h.finalize().into()
}
fn journey_response(
    agent: &ManagedAgent,
    kind: u8,
    evidence: FinalizationEvidence,
) -> Result<HumanResponse, HumanOperationError> {
    let label = match kind {
        0 => "reclaim",
        1 => "rotate",
        2 => "recover",
        3 => "archive",
        _ => return Err(HumanOperationError::Refused),
    };
    let mut out = Wire::new();
    out.text(&format!("jrn_{}", hex(&evidence.action_key)))?;
    out.text(label)?;
    out.u8(2);
    out.u8(1);
    out.text("receipt-finalized")?;
    out.text("agent.journey.receipt-finalized")?;
    out.u8(2);
    out.u8(1);
    out.text(&hex(&evidence.receipt_digest))?;
    out.text(label)?;
    out.u8(evidence.verification);
    out.text(&evidence.finalized_at.to_string())?;
    out.text(&agent.updated_at.to_string())?;
    out.u8(1);
    out.text(&hex(&evidence.receipt_digest))?;
    out.text(label)?;
    out.u8(evidence.verification);
    out.finish()
}
fn challenge_response(
    agent: &ManagedAgent,
    kind: u8,
    delay: u64,
    ready_at: u64,
    evidence: FinalizationEvidence,
) -> Result<HumanResponse, HumanOperationError> {
    let mut out = Wire::new();
    out.text(&agent.agent_id)?;
    out.u8(kind - 1);
    out.u64(delay);
    out.text(&ready_at.to_string())?;
    out.u8(1);
    out.text(&hex(&evidence.receipt_digest))?;
    out.text(if kind == 1 { "rotate" } else { "recover" })?;
    out.u8(evidence.verification);
    out.finish()
}

pub fn list(
    store: &Store,
    tenant: &TenantId,
    cursor: Option<[u8; 32]>,
    limit: u8,
) -> Result<HumanResponse, HumanOperationError> {
    if limit == 0 || limit > MAX_AGENTS_PER_PAGE {
        return Err(HumanOperationError::Refused);
    }
    let mut agents = Vec::new();
    for object_id in store.list_object_ids(tenant, ObjectKind::Configuration) {
        if !object_id.starts_with(PREFIX) {
            continue;
        }
        let object_key = key(tenant.clone(), ObjectKind::Configuration, object_id)
            .map_err(|_| HumanOperationError::Refused)?;
        let value = store
            .get(&object_key)
            .ok_or(HumanOperationError::Unavailable)?;
        if value.class() != StorageClass::LocalOnly {
            return Err(HumanOperationError::Refused);
        }
        agents.push(decode(value.bytes())?);
    }
    let start = match cursor {
        None => 0,
        Some(cursor) => agents
            .iter()
            .position(|agent| cursor_for(&agent.agent_id) == cursor)
            .map(|index| index + 1)
            .ok_or(HumanOperationError::Refused)?,
    };
    let end = start.saturating_add(usize::from(limit)).min(agents.len());
    let page = &agents[start..end];
    let mut out = Wire::new();
    out.u8(u8::try_from(page.len()).map_err(|_| HumanOperationError::Refused)?);
    for agent in page {
        encode_response_agent(&mut out, agent)?;
    }
    if end < agents.len() {
        out.u8(1);
        out.fixed(&cursor_for(&agents[end - 1].agent_id));
    } else {
        out.u8(0);
    }
    out.finish()
}

fn agent_key(
    tenant: &TenantId,
    agent_id: &str,
) -> Result<crate::store::TenantKey, HumanOperationError> {
    if agent_id.is_empty() || agent_id.len() > 512 {
        return Err(HumanOperationError::Refused);
    }
    let mut object_id = Vec::with_capacity(PREFIX.len() + agent_id.len());
    object_id.extend_from_slice(PREFIX);
    object_id.extend_from_slice(agent_id.as_bytes());
    key(tenant.clone(), ObjectKind::Configuration, object_id)
        .map_err(|_| HumanOperationError::Refused)
}

fn cursor_for(agent_id: &str) -> [u8; 32] {
    Sha256::digest(
        [
            b"layerx-agentd/managed-agent-cursor/v1\0".as_slice(),
            agent_id.as_bytes(),
        ]
        .concat(),
    )
    .into()
}

fn response_agent(agent: &ManagedAgent) -> Result<HumanResponse, HumanOperationError> {
    let mut out = Wire::new();
    encode_response_agent(&mut out, agent)?;
    out.finish()
}
fn encode_response_agent(out: &mut Wire, agent: &ManagedAgent) -> Result<(), HumanOperationError> {
    agent.validate()?;
    out.text(&agent.agent_id)?;
    out.text(&agent.name)?;
    out.text(&agent.purpose)?;
    out.u8(agent.state);
    out.u128(agent.monthly_limit);
    out.text(&agent.currency)?;
    out.u8(0);
    out.text(&agent.period_start.to_string())?;
    out.text(&agent.period_end.to_string())?;
    out.u128(agent.spent);
    out.u128(agent.monthly_limit - agent.spent);
    out.u8(3);
    out.text(&agent.created_at.to_string())?;
    out.text(&agent.updated_at.to_string())?;
    out.u8(u8::try_from(agent.verified_evidence.len()).map_err(|_| HumanOperationError::Refused)?);
    for evidence in &agent.verified_evidence {
        out.text(&hex(evidence))?;
        out.text("agent-creation")?;
        out.u8(3);
    }
    Ok(())
}

fn encode(agent: &ManagedAgent) -> Result<Vec<u8>, HumanOperationError> {
    let mut out = Wire::new();
    out.u8(VERSION);
    out.text(&agent.agent_id)?;
    out.text(&agent.name)?;
    out.text(&agent.purpose)?;
    out.u8(agent.state);
    out.u128(agent.monthly_limit);
    out.text(&agent.currency)?;
    out.u64(agent.period_start);
    out.u64(agent.period_end);
    out.u128(agent.spent);
    out.u64(agent.created_at);
    out.u64(agent.updated_at);
    out.u8(u8::try_from(agent.verified_evidence.len()).map_err(|_| HumanOperationError::Refused)?);
    for value in &agent.verified_evidence {
        out.fixed(value)
    }
    encode_context(&mut out, &agent.context)?;
    out.text(&agent.agent_did)?;
    out.fixed(&agent.session_id);
    out.fixed(&agent.session_token_id);
    out.fixed(&agent.protocol_grant_id);
    out.fixed(&agent.active_budget_id);
    Ok(out.0)
}
fn decode(bytes: &[u8]) -> Result<ManagedAgent, HumanOperationError> {
    let mut input = Input { bytes, offset: 0 };
    if input.u8()? != VERSION {
        return Err(HumanOperationError::Refused);
    }
    let agent = ManagedAgent {
        agent_id: input.text()?,
        name: input.text()?,
        purpose: input.text()?,
        state: input.u8()?,
        monthly_limit: input.u128()?,
        currency: input.text()?,
        period_start: input.u64()?,
        period_end: input.u64()?,
        spent: input.u128()?,
        created_at: input.u64()?,
        updated_at: input.u64()?,
        verified_evidence: {
            let count = usize::from(input.u8()?);
            if count == 0 || count > MAX_EVIDENCE {
                return Err(HumanOperationError::Refused);
            }
            let mut values = Vec::with_capacity(count);
            for _ in 0..count {
                values.push(input.fixed()?)
            }
            values
        },
        context: decode_context(&mut input)?,
        agent_did: input.text()?,
        session_id: input.fixed()?,
        session_token_id: input.fixed()?,
        protocol_grant_id: input.fixed()?,
        active_budget_id: input.fixed()?,
    };
    if input.offset != bytes.len() {
        return Err(HumanOperationError::Refused);
    }
    agent.validate()?;
    Ok(agent)
}

struct Wire(Vec<u8>);
impl Wire {
    fn new() -> Self {
        Self(Vec::new())
    }
    fn u8(&mut self, value: u8) {
        self.0.push(value)
    }
    fn u16(&mut self, value: u16) {
        self.0.extend_from_slice(&value.to_be_bytes())
    }
    fn u32(&mut self, value: u32) {
        self.0.extend_from_slice(&value.to_be_bytes())
    }
    fn u64(&mut self, value: u64) {
        self.0.extend_from_slice(&value.to_be_bytes())
    }
    fn u128(&mut self, value: u128) {
        self.0.extend_from_slice(&value.to_be_bytes())
    }
    fn fixed(&mut self, value: &[u8]) {
        self.0.extend_from_slice(value)
    }
    fn text(&mut self, value: &str) -> Result<(), HumanOperationError> {
        if value.is_empty() || value.len() > 4096 {
            return Err(HumanOperationError::Refused);
        }
        self.0.extend_from_slice(
            &u32::try_from(value.len())
                .map_err(|_| HumanOperationError::Refused)?
                .to_be_bytes(),
        );
        self.fixed(value.as_bytes());
        Ok(())
    }
    fn finish(self) -> Result<HumanResponse, HumanOperationError> {
        HumanResponse::new(self.0).map_err(|_| HumanOperationError::Refused)
    }
}
struct Input<'a> {
    bytes: &'a [u8],
    offset: usize,
}
impl Input<'_> {
    fn take<const N: usize>(&mut self) -> Result<[u8; N], HumanOperationError> {
        let end = self
            .offset
            .checked_add(N)
            .ok_or(HumanOperationError::Refused)?;
        let value = self
            .bytes
            .get(self.offset..end)
            .ok_or(HumanOperationError::Refused)?
            .try_into()
            .map_err(|_| HumanOperationError::Refused)?;
        self.offset = end;
        Ok(value)
    }
    fn u8(&mut self) -> Result<u8, HumanOperationError> {
        Ok(self.take::<1>()?[0])
    }
    fn u16(&mut self) -> Result<u16, HumanOperationError> {
        Ok(u16::from_be_bytes(self.take()?))
    }
    fn u32(&mut self) -> Result<u32, HumanOperationError> {
        Ok(u32::from_be_bytes(self.take()?))
    }
    fn u64(&mut self) -> Result<u64, HumanOperationError> {
        Ok(u64::from_be_bytes(self.take()?))
    }
    fn u128(&mut self) -> Result<u128, HumanOperationError> {
        Ok(u128::from_be_bytes(self.take()?))
    }
    fn fixed(&mut self) -> Result<[u8; 32], HumanOperationError> {
        self.take()
    }
    fn text(&mut self) -> Result<String, HumanOperationError> {
        let length = u32::from_be_bytes(self.take()?) as usize;
        if length == 0 || length > 4096 {
            return Err(HumanOperationError::Refused);
        }
        let end = self
            .offset
            .checked_add(length)
            .ok_or(HumanOperationError::Refused)?;
        let value = std::str::from_utf8(
            self.bytes
                .get(self.offset..end)
                .ok_or(HumanOperationError::Refused)?,
        )
        .map_err(|_| HumanOperationError::Refused)?
        .to_owned();
        self.offset = end;
        Ok(value)
    }
}
fn hex(value: &[u8]) -> String {
    const DIGITS: &[u8; 16] = b"0123456789abcdef";
    let mut out = String::with_capacity(value.len() * 2);
    for byte in value {
        out.push(char::from(DIGITS[usize::from(byte >> 4)]));
        out.push(char::from(DIGITS[usize::from(byte & 15)]));
    }
    out
}
fn parse_agent_digest(value: &str) -> Result<[u8; 32], HumanOperationError> {
    let encoded = value
        .strip_prefix("agt_")
        .ok_or(HumanOperationError::Refused)?;
    if encoded.len() != 64 {
        return Err(HumanOperationError::Refused);
    }
    let mut out = [0u8; 32];
    for (index, pair) in encoded.as_bytes().chunks_exact(2).enumerate() {
        let digit = |b| match b {
            b'0'..=b'9' => Some(b - b'0'),
            b'a'..=b'f' => Some(b - b'a' + 10),
            _ => None,
        };
        out[index] = (digit(pair[0]).ok_or(HumanOperationError::Refused)? << 4)
            | digit(pair[1]).ok_or(HumanOperationError::Refused)?
    }
    if out == [0; 32] {
        return Err(HumanOperationError::Refused);
    }
    Ok(out)
}
fn validate_context(c: &HumanAgentLifecycleSeed) -> Result<(), HumanOperationError> {
    if c.actor.is_empty()
        || c.primary_authority.is_empty()
        || c.custody_key.is_empty()
        || c.custody_public_key == [0; 32]
        || c.owner_account.is_empty()
        || c.budget_account.is_empty()
        || c.budget_asset == [0; 32]
        || c.purpose_hash == [0; 32]
        || c.recovery_root == [0; 32]
        || c.recovery_threshold == 0
        || c.capability_id == [0; 32]
        || c.activity_types.is_empty()
        || c.counterparties.is_empty()
        || c.assets.is_empty()
        || c.amount_ceiling == 0
        || c.rate_maximum_uses == 0
        || c.rate_window_sequences == 0
        || c.purposes.is_empty()
        || c.capability_expiry_sequence == 0
        || c.session_scopes.is_empty()
        || c.session_expiry_unix_seconds == 0
        || c.budget_period_seconds == 0
        || c.budget_expiry_seconds == 0
        || c.initial_funding == 0
        || c.network_id == 0
        || c.creation_receipt_roots.is_empty()
    {
        return Err(HumanOperationError::Refused);
    }
    Ok(())
}
fn encode_context(out: &mut Wire, c: &HumanAgentLifecycleSeed) -> Result<(), HumanOperationError> {
    out.text(&c.agent_id)?;
    out.text(&c.name)?;
    out.text(&c.purpose)?;
    out.text(&c.currency)?;
    out.u128(c.monthly_limit);
    out.u64(c.period_start);
    out.u64(c.period_end);
    out.u64(c.created_at);
    out.u64(c.updated_at);
    fixeds(out, &c.verified_evidence)?;
    out.text(&c.actor)?;
    out.text(&c.primary_authority)?;
    out.text(&c.custody_key)?;
    out.fixed(&c.custody_public_key);
    out.text(&c.owner_account)?;
    out.text(&c.budget_account)?;
    out.fixed(&c.budget_asset);
    out.fixed(&c.purpose_hash);
    out.fixed(&c.recovery_root);
    out.u16(c.recovery_threshold);
    out.fixed(&c.capability_id);
    u32s(out, &c.activity_types)?;
    fixeds(out, &c.counterparties)?;
    fixeds(out, &c.assets)?;
    out.u128(c.amount_ceiling);
    out.u64(c.rate_maximum_uses);
    out.u64(c.rate_window_sequences);
    texts(out, &c.purposes)?;
    out.u64(c.capability_expiry_sequence);
    texts(out, &c.session_scopes)?;
    out.u64(c.session_expiry_unix_seconds);
    out.fixed(&c.protocol_grant_id);
    out.u64(c.budget_period_seconds);
    out.u64(c.budget_expiry_seconds);
    out.u128(c.initial_funding);
    out.u32(c.network_id);
    fixeds(out, &c.creation_receipt_roots)
}
pub fn lifecycle_publish_digest(
    c: &HumanAgentLifecycleSeed,
) -> Result<[u8; 32], HumanOperationError> {
    let mut wire = Wire::new();
    encode_context(&mut wire, c)?;
    let mut digest = Sha256::new();
    digest.update(b"layerx-human-agent-lifecycle-publish/v1");
    digest.update(&wire.0);
    Ok(digest.finalize().into())
}
fn decode_context(i: &mut Input) -> Result<HumanAgentLifecycleSeed, HumanOperationError> {
    Ok(HumanAgentLifecycleSeed {
        agent_id: i.text()?,
        name: i.text()?,
        purpose: i.text()?,
        currency: i.text()?,
        monthly_limit: i.u128()?,
        period_start: i.u64()?,
        period_end: i.u64()?,
        created_at: i.u64()?,
        updated_at: i.u64()?,
        verified_evidence: read_fixeds(i, 64)?,
        actor: i.text()?,
        primary_authority: i.text()?,
        custody_key: i.text()?,
        custody_public_key: i.fixed()?,
        owner_account: i.text()?,
        budget_account: i.text()?,
        budget_asset: i.fixed()?,
        purpose_hash: i.fixed()?,
        recovery_root: i.fixed()?,
        recovery_threshold: i.u16()?,
        capability_id: i.fixed()?,
        activity_types: read_u32s(i, 256)?,
        counterparties: read_fixeds(i, 256)?,
        assets: read_fixeds(i, 256)?,
        amount_ceiling: i.u128()?,
        rate_maximum_uses: i.u64()?,
        rate_window_sequences: i.u64()?,
        purposes: read_texts(i, 64)?,
        capability_expiry_sequence: i.u64()?,
        session_scopes: read_texts(i, 64)?,
        session_expiry_unix_seconds: i.u64()?,
        protocol_grant_id: i.fixed()?,
        budget_period_seconds: i.u64()?,
        budget_expiry_seconds: i.u64()?,
        initial_funding: i.u128()?,
        network_id: i.u32()?,
        creation_receipt_roots: read_fixeds(i, 64)?,
    })
}
fn fixeds(out: &mut Wire, v: &[[u8; 32]]) -> Result<(), HumanOperationError> {
    out.u16(u16::try_from(v.len()).map_err(|_| HumanOperationError::Refused)?);
    for x in v {
        out.fixed(x)
    }
    Ok(())
}
fn u32s(out: &mut Wire, v: &[u32]) -> Result<(), HumanOperationError> {
    out.u16(u16::try_from(v.len()).map_err(|_| HumanOperationError::Refused)?);
    for x in v {
        out.u32(*x)
    }
    Ok(())
}
fn texts(out: &mut Wire, v: &[String]) -> Result<(), HumanOperationError> {
    out.u16(u16::try_from(v.len()).map_err(|_| HumanOperationError::Refused)?);
    for x in v {
        out.text(x)?
    }
    Ok(())
}
fn read_fixeds(i: &mut Input, max: usize) -> Result<Vec<[u8; 32]>, HumanOperationError> {
    let n = usize::from(i.u16()?);
    if n == 0 || n > max {
        return Err(HumanOperationError::Refused);
    }
    let mut v = Vec::with_capacity(n);
    for _ in 0..n {
        v.push(i.fixed()?)
    }
    Ok(v)
}
fn read_u32s(i: &mut Input, max: usize) -> Result<Vec<u32>, HumanOperationError> {
    let n = usize::from(i.u16()?);
    if n == 0 || n > max {
        return Err(HumanOperationError::Refused);
    }
    let mut v = Vec::with_capacity(n);
    for _ in 0..n {
        v.push(i.u32()?)
    }
    Ok(v)
}
fn read_texts(i: &mut Input, max: usize) -> Result<Vec<String>, HumanOperationError> {
    let n = usize::from(i.u16()?);
    if n == 0 || n > max {
        return Err(HumanOperationError::Refused);
    }
    let mut v = Vec::with_capacity(n);
    for _ in 0..n {
        v.push(i.text()?)
    }
    Ok(v)
}
