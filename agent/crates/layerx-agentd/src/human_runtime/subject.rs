use super::{
    hex, query, HumanOperationError, HumanPeer, ModuleRegistry, ObjectKind, Store, TenantId,
    TenantKey,
};
use crate::human::HumanSubject;
use crate::store::StorageClass;
use layerx_types::{account::AccountId, ids::Did};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::{BTreeMap, BTreeSet};
use std::sync::{Arc, Mutex};

const KEY: &[u8] = b"human-subject-v1";

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Binding {
    version: u8,
    uid: u32,
    transport_tenant: String,
    transport_principal: String,
    principal: String,
    owner: String,
    account: String,
    asset: [u8; 32],
}

pub(super) fn principal_query(peer: &HumanPeer) -> String {
    let Some(subject) = &peer.subject else {
        return query(&peer.principal);
    };
    let mut params = format!(
        "{}&subject_principal={}&owner_did={}&owner_account={}&asset_id={}",
        query(&subject.transport_principal),
        query(&peer.principal),
        query(&subject.owner),
        query(&subject.account),
        hex(&subject.asset)
    );
    if let Some(registration) = &subject.registration {
        params.push_str("&registration=");
        params.push_str(&hex(registration));
    }
    params
}

pub(super) fn verify_context(peer: &HumanPeer, value: &Value) -> Result<(), HumanOperationError> {
    let subject = peer.subject.as_ref().ok_or(HumanOperationError::Refused)?;
    let account = AccountId::parse(&subject.account).map_err(|_| HumanOperationError::Refused)?;
    let account_id = layerx_wire::hash::account_id_for_protocol(&account, 3)
        .map_err(|_| HumanOperationError::Refused)?;
    if value["subject_principal"].as_str() != Some(peer.principal.as_str())
        || value["tenant"].as_str() != Some(subject.transport_tenant.as_str())
        || value["agent_tenant"].as_str() != Some(peer.tenant.as_str())
        || value["owner_did"].as_str() != Some(subject.owner.as_str())
        || value["account_id"].as_str() != Some(hex(&account_id).as_str())
        || value["asset_id"].as_str() != Some(hex(&subject.asset).as_str())
        || value["verification_level"].as_str() != Some("checkpoint_finalised")
        || value["observed_head_sequence"]
            .as_u64()
            .is_none_or(|sequence| sequence == 0)
        || value["checkpoint_digest"]
            .as_str()
            .and_then(super::digest_from_hex)
            .is_none_or(|digest| digest == [0; 32])
    {
        return Err(HumanOperationError::Refused);
    }
    Ok(())
}

fn binding(peer: &HumanPeer) -> Result<Binding, HumanOperationError> {
    let scope = peer.subject.as_ref().ok_or(HumanOperationError::Refused)?;
    let namespace =
        layerx_identity_binding::subject_namespace(&scope.transport_tenant, &peer.principal)
            .map_err(|_| HumanOperationError::Refused)?;
    let did = Did::new(scope.owner.as_bytes()).map_err(|_| HumanOperationError::Refused)?;
    let account = AccountId::parse(&scope.account).map_err(|_| HumanOperationError::Refused)?;
    if namespace != peer.tenant
        || scope.registration.is_some()
        || scope.asset == [0; 32]
        || !account.canonical().starts_with(&format!(
            "agent:{}:",
            std::str::from_utf8(did.as_bytes()).map_err(|_| HumanOperationError::Refused)?
        ))
    {
        return Err(HumanOperationError::Refused);
    }
    Ok(Binding {
        version: 1,
        uid: peer.uid,
        transport_tenant: scope.transport_tenant.clone(),
        transport_principal: scope.transport_principal.clone(),
        principal: peer.principal.clone(),
        owner: scope.owner.clone(),
        account: scope.account.clone(),
        asset: scope.asset,
    })
}

pub(super) fn retain(store: &mut Store, peer: &HumanPeer) -> Result<(), HumanOperationError> {
    let value = binding(peer)?;
    let tenant = TenantId::new(peer.tenant.clone()).map_err(|_| HumanOperationError::Refused)?;
    let key = TenantKey::new(tenant, ObjectKind::Configuration, KEY)
        .map_err(|_| HumanOperationError::Refused)?;
    if let Some(existing) = store.get(&key) {
        if existing.class() != StorageClass::LocalOnly {
            return Err(HumanOperationError::Refused);
        }
        let old: Binding =
            serde_json::from_slice(existing.bytes()).map_err(|_| HumanOperationError::Refused)?;
        if old.version != 1
            || old.uid != value.uid
            || old.transport_tenant != value.transport_tenant
            || old.transport_principal != value.transport_principal
            || old.principal != value.principal
            || old.owner != value.owner
        {
            return Err(HumanOperationError::Refused);
        }
        return Ok(());
    }
    store
        .put_local(
            key,
            serde_json::to_vec(&value).map_err(|_| HumanOperationError::Refused)?,
        )
        .map_err(|_| HumanOperationError::Unavailable)
}

pub(super) fn restore_peers(
    store: &Store,
    configured: &BTreeMap<u32, (String, String)>,
) -> Result<Vec<HumanPeer>, HumanOperationError> {
    let mut output = configured
        .iter()
        .map(|(uid, (principal, tenant))| HumanPeer {
            uid: *uid,
            principal: principal.clone(),
            tenant: tenant.clone(),
            subject: None,
        })
        .collect::<Vec<_>>();
    let mut tenants = output
        .iter()
        .map(|peer| peer.tenant.clone())
        .collect::<BTreeSet<_>>();
    for tenant in store.tenant_ids_for_kind(ObjectKind::Configuration) {
        let key = TenantKey::new(tenant.clone(), ObjectKind::Configuration, KEY)
            .map_err(|_| HumanOperationError::Refused)?;
        let Some(stored) = store.get(&key) else {
            continue;
        };
        if stored.class() != StorageClass::LocalOnly || !tenants.insert(tenant.as_str().to_owned())
        {
            return Err(HumanOperationError::Refused);
        }
        let value: Binding =
            serde_json::from_slice(stored.bytes()).map_err(|_| HumanOperationError::Refused)?;
        if value.version != 1
            || configured.get(&value.uid)
                != Some(&(
                    value.transport_principal.clone(),
                    value.transport_tenant.clone(),
                ))
        {
            return Err(HumanOperationError::Refused);
        }
        let peer = HumanPeer {
            uid: value.uid,
            tenant: tenant.as_str().to_owned(),
            principal: value.principal,
            subject: Some(HumanSubject {
                transport_tenant: value.transport_tenant,
                transport_principal: value.transport_principal,
                owner: value.owner,
                account: value.account,
                asset: value.asset,
                registration: None,
            }),
        };
        binding(&peer)?;
        output.push(peer);
    }
    Ok(output)
}

pub(super) fn for_did(
    shared: &Arc<Mutex<Store>>,
    peer: &HumanPeer,
    did: &Did,
    registry: &ModuleRegistry,
) -> Result<HumanPeer, HumanOperationError> {
    let Some(scope) = &peer.subject else {
        return Ok(peer.clone());
    };
    if did.as_bytes() == scope.owner.as_bytes() {
        return Ok(peer.clone());
    }
    let tenant = TenantId::new(peer.tenant.clone()).map_err(|_| HumanOperationError::Refused)?;
    let store = shared
        .lock()
        .map_err(|_| HumanOperationError::Unavailable)?;
    let mut matching = None;
    for id in store.list_object_ids(&tenant, ObjectKind::Outbox) {
        if id.len() != 32 {
            continue;
        }
        let identifier: [u8; 32] = id
            .as_slice()
            .try_into()
            .map_err(|_| HumanOperationError::Refused)?;
        let mut outbox = crate::outbox::Outbox::default();
        outbox
            .restore(&store, tenant.clone(), identifier)
            .map_err(|_| HumanOperationError::Refused)?;
        let status = outbox
            .status(identifier)
            .ok_or(HumanOperationError::Refused)?;
        if status.state != crate::outbox::SubmissionState::Executed || status.evidence.is_none() {
            continue;
        }
        let key = TenantKey::new(tenant.clone(), ObjectKind::PreparedActivity, id)
            .map_err(|_| HumanOperationError::Refused)?;
        let bytes = store.get(&key).ok_or(HumanOperationError::Refused)?.bytes();
        let activity = layerx_wire::activity::decode_signed(bytes, registry)
            .map_err(|_| HumanOperationError::Refused)?;
        if layerx_wire::activity::encode_signed(&activity)
            .map_err(|_| HumanOperationError::Refused)?
            != bytes
            || layerx_wire::hash::activity_id(&activity)
                .map_err(|_| HumanOperationError::Refused)?
                != status.activity_id
            || activity.idempotency_key() != identifier
        {
            return Err(HumanOperationError::Refused);
        }
        if activity.activity_type().module() != layerx_types::payload::ModuleId::Governance
            || activity.activity_type().ordinal() != 1
            || activity.actor_did() != scope.owner.as_bytes()
        {
            continue;
        }
        let Ok(registration) =
            layerx_crypto::onboarding::SponsoredRegistration::decode(activity.payload())
        else {
            continue;
        };
        if registration.consent.target != *did {
            continue;
        }
        registration
            .validate_outer(&activity)
            .map_err(|_| HumanOperationError::Refused)?;
        if matching.replace(bytes.to_vec()).is_some() {
            return Err(HumanOperationError::Refused);
        }
    }
    let mut bound = peer.clone();
    bound
        .subject
        .as_mut()
        .ok_or(HumanOperationError::Refused)?
        .registration = Some(matching.ok_or(HumanOperationError::Refused)?);
    Ok(bound)
}

pub(super) fn for_activity(
    shared: &Arc<Mutex<Store>>,
    peer: &HumanPeer,
    bytes: &[u8],
    registry: &ModuleRegistry,
) -> Result<HumanPeer, HumanOperationError> {
    if peer.subject.is_none() {
        return Ok(peer.clone());
    }
    let activity = layerx_wire::activity::decode_signed(bytes, registry)
        .map_err(|_| HumanOperationError::Refused)?;
    let did = Did::new(activity.actor_did()).map_err(|_| HumanOperationError::Refused)?;
    for_did(shared, peer, &did, registry)
}

pub(super) fn begin_transmission(
    outbox: &mut crate::outbox::Outbox,
    store: &mut Store,
    identifier: [u8; 32],
) -> Result<Vec<u8>, HumanOperationError> {
    let bytes = outbox
        .bytes_for_transmission(identifier)
        .map_err(|_| HumanOperationError::Refused)?
        .to_vec();
    outbox
        .transition(
            store,
            identifier,
            crate::outbox::SubmissionState::Submitted,
            "transmission started",
            None,
        )
        .map_err(|_| HumanOperationError::Unavailable)?;
    Ok(bytes)
}

#[cfg(test)]
mod tests;
