use super::{
    action_record_key, agent_key, decode, encode, HumanOperationError as Error, HumanResponse,
    Input, ManagedAgent, ObjectKind, Sha256, StorageClass, Store, TenantId, Wire, PREFIX,
};
use crate::budget::{NativeBudgetBinding, NativeBudgetScope};
use crate::store::{key, TenantKey};
use sha2::Digest;

const HISTORY_PREFIX: &[u8] = b"managed-agent-budget-v1:";
const HISTORY_DOMAIN: &[u8] = b"layerx-agentd/managed-agent-budget-history/v1\0";
const MAX_SNAPSHOT: usize = 1_048_576;

struct History {
    agent: ManagedAgent,
    action: [u8; 32],
    request: [u8; 32],
}

fn agents(store: &Store, tenant: &TenantId) -> Result<Vec<ManagedAgent>, Error> {
    let mut agents = Vec::new();
    for id in store.list_object_ids(tenant, ObjectKind::Configuration) {
        if !id.starts_with(PREFIX) {
            continue;
        }
        let object =
            key(tenant.clone(), ObjectKind::Configuration, id).map_err(|_| Error::Refused)?;
        let value = store.get(&object).ok_or(Error::Unavailable)?;
        if value.class() != StorageClass::LocalOnly {
            return Err(Error::Refused);
        }
        let agent = decode(value.bytes())?;
        if agent_key(tenant, &agent.agent_id)? != object {
            return Err(Error::Refused);
        }
        agents.push(agent);
    }
    Ok(agents)
}

fn history_key(tenant: &TenantId, budget: [u8; 32], bytes: &[u8]) -> Result<TenantKey, Error> {
    let mut digest = Sha256::new();
    digest.update(HISTORY_DOMAIN);
    digest.update(bytes);
    let mut id = HISTORY_PREFIX.to_vec();
    id.extend_from_slice(&budget);
    id.extend_from_slice(&digest.finalize());
    key(tenant.clone(), ObjectKind::Configuration, id).map_err(|_| Error::Refused)
}

fn history_bytes(history: &History) -> Result<Vec<u8>, Error> {
    let snapshot = encode(&history.agent)?;
    if snapshot.is_empty() || snapshot.len() > MAX_SNAPSHOT {
        return Err(Error::Refused);
    }
    let mut bytes = Wire::new();
    bytes.u8(1);
    bytes.fixed(&history.action);
    bytes.fixed(&history.request);
    bytes.u32(u32::try_from(snapshot.len()).map_err(|_| Error::Refused)?);
    bytes.fixed(&snapshot);
    Ok(bytes.0)
}

fn histories(store: &Store, tenant: &TenantId) -> Result<Vec<History>, Error> {
    let mut histories = Vec::new();
    for id in store.list_object_ids(tenant, ObjectKind::Configuration) {
        if !id.starts_with(HISTORY_PREFIX) {
            continue;
        }
        let object =
            key(tenant.clone(), ObjectKind::Configuration, id).map_err(|_| Error::Refused)?;
        let value = store.get(&object).ok_or(Error::Unavailable)?;
        if value.class() != StorageClass::LocalOnly {
            return Err(Error::Refused);
        }
        let history = decode_history(value.bytes())?;
        if history_key(tenant, history.agent.active_budget_id, value.bytes())? != object {
            return Err(Error::Refused);
        }
        let action = store
            .get(&action_record_key(tenant, history.action)?)
            .ok_or(Error::Refused)?;
        if action.class() != StorageClass::LocalOnly
            || action.bytes().len() <= 32
            || action.bytes()[..32] != history.request
        {
            return Err(Error::Refused);
        }
        HumanResponse::new(action.bytes()[32..].to_vec()).map_err(|_| Error::Refused)?;
        histories.push(history);
    }
    Ok(histories)
}

fn decode_history(bytes: &[u8]) -> Result<History, Error> {
    let mut input = Input { bytes, offset: 0 };
    if input.u8()? != 1 {
        return Err(Error::Refused);
    }
    let action = input.fixed()?;
    let request = input.fixed()?;
    let length = usize::try_from(input.u32()?).map_err(|_| Error::Refused)?;
    if action == [0; 32] || request == [0; 32] || length == 0 || length > MAX_SNAPSHOT {
        return Err(Error::Refused);
    }
    let history = History {
        agent: decode(input.bytes(length)?)?,
        action,
        request,
    };
    if input.offset != bytes.len() || history_bytes(&history)? != bytes {
        return Err(Error::Refused);
    }
    Ok(history)
}

fn matches_live(history: &ManagedAgent, live: &ManagedAgent) -> bool {
    history.agent_id == live.agent_id
        && history.agent_did == live.agent_did
        && history.created_at == live.created_at
        && history.context.network_id == live.context.network_id
        && history.context.owner_account == live.context.owner_account
        && history.context.budget_asset == live.context.budget_asset
        && history.context.purpose_hash == live.context.purpose_hash
        && history.updated_at <= live.updated_at
        && history.active_budget_id != live.active_budget_id
}

pub(super) fn scope(
    store: &Store,
    tenant: &TenantId,
    budget: [u8; 32],
) -> Result<NativeBudgetScope, Error> {
    if budget == [0; 32] {
        return Err(Error::Refused);
    }
    let agents = agents(store, tenant)?;
    let mut selected = None;
    for agent in &agents {
        if agent.active_budget_id == budget {
            if selected.is_some() {
                return Err(Error::Refused);
            }
            selected = Some(agent_scope(agent, agent.state == 1)?);
        }
    }
    for history in histories(store, tenant)? {
        if history.agent.active_budget_id != budget {
            continue;
        }
        let mut live = agents
            .iter()
            .filter(|agent| agent.agent_id == history.agent.agent_id);
        let agent = live.next().ok_or(Error::Refused)?;
        if selected.is_some() || live.next().is_some() || !matches_live(&history.agent, agent) {
            return Err(Error::Refused);
        }
        selected = Some(agent_scope(&history.agent, false)?);
    }
    selected.ok_or(Error::Refused)
}

pub(super) fn replacement(
    store: &Store,
    tenant: &TenantId,
    previous: &ManagedAgent,
    current: &ManagedAgent,
    action: [u8; 32],
    request: [u8; 32],
) -> Result<Option<(TenantKey, Vec<u8>)>, Error> {
    if previous.active_budget_id == current.active_budget_id {
        return Ok(None);
    }
    if action == [0; 32] || request == [0; 32] || !matches_live(previous, current) {
        return Err(Error::Refused);
    }
    for agent in agents(store, tenant)? {
        if agent.agent_id != previous.agent_id
            && (agent.active_budget_id == previous.active_budget_id
                || agent.active_budget_id == current.active_budget_id)
        {
            return Err(Error::Refused);
        }
    }
    for history in histories(store, tenant)? {
        if history.agent.active_budget_id == previous.active_budget_id
            || history.agent.active_budget_id == current.active_budget_id
        {
            return Err(Error::Refused);
        }
    }
    let history = History {
        agent: previous.clone(),
        action,
        request,
    };
    let bytes = history_bytes(&history)?;
    Ok(Some((
        history_key(tenant, previous.active_budget_id, &bytes)?,
        bytes,
    )))
}

fn agent_scope(agent: &ManagedAgent, write_enabled: bool) -> Result<NativeBudgetScope, Error> {
    let context = &agent.context;
    let budget_id = agent.active_budget_id;
    let owner = layerx_types::account::AccountId::parse(&context.owner_account)
        .map_err(|_| Error::Refused)?;
    let account = layerx_types::account::AccountId::parse(&format!(
        "agent:{}:budget:{}",
        context.actor,
        layerx_programs::hex::encode(&budget_id)
    ))
    .map_err(|_| Error::Refused)?;
    let period = context
        .budget_period_seconds
        .checked_mul(1000)
        .ok_or(Error::Refused)?;
    let start = context
        .period_start
        .checked_mul(1000)
        .ok_or(Error::Refused)?;
    let lifetime = context
        .budget_expiry_seconds
        .checked_mul(1000)
        .ok_or(Error::Refused)?;
    Ok(NativeBudgetScope {
        network_id: context.network_id,
        write_enabled,
        binding: NativeBudgetBinding {
            budget_id,
            owner_account: layerx_wire::hash::account_id_for_protocol(&owner, 3)
                .map_err(|_| Error::Refused)?,
            budget_account: layerx_wire::hash::account_id_for_protocol(&account, 3)
                .map_err(|_| Error::Refused)?,
            asset: context.budget_asset,
            owner_did: layerx_types::ids::Did::new(context.actor.as_bytes())
                .map_err(|_| Error::Refused)?,
            owner_public_key: context.custody_public_key,
            period_start_ms: start,
            period_length_ms: period,
            expiry_ms: start.checked_add(lifetime).ok_or(Error::Refused)?,
        },
        maximum: agent.monthly_limit.min(context.amount_ceiling),
        maximum_lifetime_ms: lifetime,
    })
}
