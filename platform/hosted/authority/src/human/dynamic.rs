use super::{
    budget_state, digest, hex, json, native_checkpoint, native_identity_state, native_u64, refusal,
    session_membership, unavailable, value, Authority, Config, EvidenceReference, Human, Identity,
    KeyPolicy, PrincipalPolicy, Response, ScopeBinding, Verified,
};
use layerx_identity_binding::{Binding, Client, Config as BindingConfig};
use layerx_types::{
    account::AccountId,
    payload::{ActivityType, ModuleId, ModuleRegistration, ModuleRegistry},
};
use layerx_wire::activity::{decode_signed, encode_signed, encode_unsigned, Activity};
use layerx_wire::hash::{activity_id, payload_hash, Domain};
use std::collections::BTreeMap;
use std::path::PathBuf;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

pub(super) fn requested(params: &BTreeMap<String, String>) -> bool {
    params.keys().any(|key| {
        matches!(
            key.as_str(),
            "subject_principal"
                | "owner_did"
                | "owner_account"
                | "asset_id"
                | "registration"
                | "signed_activity"
        )
    })
}

pub(super) fn binding_client(tenant: &str) -> Result<Option<Client>, String> {
    let names = [
        "LAYERX_AUTHORITY_IDENTITY_BINDING_SOCKET",
        "LAYERX_AUTHORITY_IDENTITY_BINDING_UID",
        "LAYERX_AUTHORITY_IDENTITY_BINDING_GID",
    ];
    if names.iter().all(|name| std::env::var_os(name).is_none()) {
        return Ok(None);
    }
    let values = names.map(|name| {
        std::env::var(name)
            .map_err(|_| "identity binding configuration is incomplete or invalid".to_owned())
    });
    let [socket, uid, gid] = values;
    let (socket, uid, gid) = (socket?, uid?, gid?);
    Client::new(
        BindingConfig {
            socket: PathBuf::from(socket),
            tenant: tenant.to_owned(),
            peer_uid: uid.parse().map_err(|_| "invalid identity binding UID")?,
            peer_gid: gid.parse().map_err(|_| "invalid identity binding GID")?,
            deadline: Duration::from_secs(5),
        },
        layerx_client::runtime_clock::RuntimeClock::from_environment()
            .map_err(|_| "identity binding clock unavailable")?,
    )
    .map(Some)
    .map_err(|_| "identity binding configuration is invalid".to_owned())
}

struct Subject {
    binding: Binding,
    did: String,
    account: AccountId,
    account_id: [u8; 32],
    asset: [u8; 32],
}

fn subject(human: &Human, params: &BTreeMap<String, String>) -> Result<Subject, Response> {
    let refused = || refusal(403, "subject_not_bound", None);
    let principal = params.get("subject_principal").ok_or_else(refused)?;
    let did = params.get("owner_did").ok_or_else(refused)?;
    let account = AccountId::parse(params.get("owner_account").ok_or_else(refused)?)
        .map_err(|_| refused())?;
    let asset =
        hex::decode32(params.get("asset_id").ok_or_else(refused)?).map_err(|_| refused())?;
    let binding = human
        .binding
        .as_ref()
        .ok_or_else(|| unavailable("identity_binding_unconfigured"))?
        .lookup(principal)
        .map_err(|_| refused())?;
    if binding.tenant() != human.tenant
        || binding.principal() != principal
        || binding.did().as_bytes() != did.as_bytes()
        || asset == [0; 32]
        || ![
            format!("agent:{did}:main"),
            format!("agent:{did}:asset:{}", hex::encode(&asset)),
        ]
        .iter()
        .any(|name| name == account.canonical())
    {
        return Err(refused());
    }
    let account_id =
        layerx_wire::hash::account_id_for_protocol(&account, 3).map_err(|_| refused())?;
    Ok(Subject {
        binding,
        did: did.clone(),
        account,
        account_id,
        asset,
    })
}

struct Current {
    state: Vec<u8>,
    head: u64,
    timestamp: u64,
    checkpoint: [u8; 32],
    authorization: layerx_proof::inclusion::SequencerAuthorization,
}

fn current(
    config: &Config,
    subject: &Subject,
    policy: &PrincipalPolicy,
) -> Result<Current, Response> {
    let refused = || unavailable("subject_checkpoint_proof_unavailable");
    let mut session = budget_state::Session::open(config).map_err(|()| refused())?;
    let key = budget_state::identity_key(&subject.did).map_err(|()| refused())?;
    let state = session.module(7, &key).map_err(|()| refused())?;
    let account = session
        .account(subject.account_id)
        .map_err(|()| refused())?;
    let account = account.account();
    if state.len() != 223
        || &state[..5] != b"LXGI1"
        || state[5..37] != key
        || native_u64(&state, 69)? == 0
        || native_u64(&state, 215)? > session.context.head.chain_sequence
        || account.name != subject.account.canonical().as_bytes()
        || account.kind != 1
        || account.frozen
        || account.asset_id() != subject.asset
        || account.authority_key.as_ref().map(<[u8; 32]>::as_slice) != Some(&state[37..69])
    {
        return Err(refused());
    }
    let header = layerx_wire::receipt::decode_batch_header(session.checkpoint.canonical_header())
        .map_err(|_| refused())?;
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|_| refused())?
        .as_millis();
    if now
        .checked_sub(u128::from(header.timestamp_ms()))
        .is_none_or(|age| age > u128::from(policy.maximum_age_seconds) * 1000)
    {
        return Err(unavailable("subject_checkpoint_stale"));
    }
    session.unchanged().map_err(|()| refused())?;
    Ok(Current {
        state,
        head: session.context.head.chain_sequence,
        timestamp: header.timestamp_ms(),
        checkpoint: session.context.head.finalised_checkpoint,
        authorization: session.context.sequencer_authorization,
    })
}

fn registry(human: &Human) -> Result<ModuleRegistry, Response> {
    let (registry, _) = super::read_registry(&human.registry_path)?;
    let modules = registry
        .modules
        .iter()
        .map(|module| {
            let id = ModuleId::from_u16(module.module).map_err(|_| ())?;
            let types = module
                .ordinals
                .iter()
                .map(|ordinal| ActivityType::new(id, *ordinal).map_err(|_| ()))
                .collect::<Result<Vec<_>, _>>()?;
            ModuleRegistration::new(id, &types).map_err(|_| ())
        })
        .collect::<Result<Vec<_>, _>>()
        .map_err(|()| unavailable("module_registry_invalid"))?;
    ModuleRegistry::new(&modules).map_err(|_| unavailable("module_registry_invalid"))
}

fn signed(bytes: &[u8], registry: &ModuleRegistry, network: u32) -> Result<Activity, Response> {
    let refused = || refusal(403, "original_activity_invalid", None);
    let activity = decode_signed(bytes, registry).map_err(|_| refused())?;
    if activity.protocol_version() != 3
        || activity.network_id() != network
        || encode_signed(&activity).map_err(|_| refused())? != bytes
        || payload_hash(&activity).map_err(|_| refused())? != activity.payload_hash()
    {
        return Err(refused());
    }
    let key: [u8; 32] = activity.authority().try_into().map_err(|_| refused())?;
    let signature: [u8; 64] = activity
        .signature()
        .ok_or_else(refused)?
        .try_into()
        .map_err(|_| refused())?;
    let unsigned = encode_unsigned(&activity).map_err(|_| refused())?;
    let message =
        layerx_crypto::SignatureMessage::new(Domain::SignaturePreimage, 3, network, &unsigned)
            .map_err(|_| refused())?;
    layerx_crypto::ed25519::verify(&key, &signature, message).map_err(|_| refused())?;
    Ok(activity)
}

fn held<'a>(evidence: &'a [Verified], activity: &Activity) -> Result<&'a Verified, Response> {
    let identifier =
        activity_id(activity).map_err(|_| refusal(403, "original_activity_invalid", None))?;
    evidence
        .iter()
        .find(|item| item.facts.activity_id == identifier)
        .ok_or_else(|| unavailable("original_activity_evidence_unavailable"))
}

fn identity_at<'a>(
    evidence: &'a [Verified],
    did: &str,
    sequence: u64,
) -> Result<(&'a Verified, Vec<u8>), Response> {
    evidence
        .iter()
        .rev()
        .filter(|item| item.facts.global_sequence <= sequence)
        .find_map(|item| {
            native_identity_state(item, did)
                .ok()
                .map(|state| (item, state))
        })
        .ok_or_else(|| unavailable("identity_state_proof_unavailable"))
}

fn registration_binding(
    bytes: &[u8],
    registry: &ModuleRegistry,
    network: u32,
    owner: &str,
    target: &str,
) -> Result<(Activity, layerx_crypto::onboarding::SponsoredRegistration), Response> {
    let activity = signed(bytes, registry, network)?;
    let registration = layerx_crypto::onboarding::SponsoredRegistration::decode(activity.payload())
        .map_err(|_| refusal(403, "child_not_bound", None))?;
    registration
        .validate_outer(&activity)
        .map_err(|_| refusal(403, "child_not_bound", None))?;
    if activity.actor_did() != owner.as_bytes()
        || registration.consent.target.as_bytes() != target.as_bytes()
    {
        return Err(refusal(403, "child_not_bound", None));
    }
    Ok((activity, registration))
}

fn child(
    config: &Config,
    human: &Human,
    subject: &Subject,
    target: &str,
    encoded: Option<&String>,
    evidence: &[Verified],
) -> Result<(), Response> {
    if target == subject.did {
        if encoded.is_some() {
            return Err(refusal(400, "unexpected_registration", None));
        }
        return Ok(());
    }
    let bytes = hex::decode(encoded.ok_or_else(|| refusal(403, "child_not_bound", None))?)
        .map_err(|_| refusal(403, "child_not_bound", None))?;
    let (activity, registration) = registration_binding(
        &bytes,
        &registry(human)?,
        config.protocol_network_id,
        &subject.did,
        target,
    )?;
    let item = held(evidence, &activity)?;
    native_checkpoint(item)?;
    let target_state = native_identity_state(item, target)?;
    let (sponsor_item, sponsor_state) = identity_at(
        evidence,
        &subject.did,
        item.facts
            .global_sequence
            .checked_sub(1)
            .ok_or_else(|| refusal(403, "child_not_bound", None))?,
    )?;
    super::native_complete_suffix(sponsor_item, evidence)?;
    if target_state[37..69] != registration.consent.target_public_key
        || sponsor_state[37..69] != activity.authority()[..]
    {
        return Err(refusal(403, "child_not_bound", None));
    }
    super::native_complete_suffix(item, evidence)
}

fn key_policy(
    state: &[u8],
    reference: &EvidenceReference,
    sequence: u64,
    recovery: bool,
) -> Result<KeyPolicy, Response> {
    let (revision, delay, maximum) = if recovery {
        (
            native_u64(state, 175)?,
            native_u64(state, 183)?,
            native_u64(state, 191)?,
        )
    } else {
        (
            native_u64(state, 167)?,
            native_u64(state, 199)?.div_ceil(1000),
            native_u64(state, 207)? / 1000,
        )
    };
    Ok(KeyPolicy {
        policy_revision: revision,
        required_delay_seconds: delay,
        maximum_delay_seconds: maximum,
        effective_sequence: sequence,
        evidence: reference.clone(),
    })
}

fn identity(item: &Verified, did: &str) -> Result<Identity, Response> {
    let state = native_identity_state(item, did)?;
    let reference = EvidenceReference {
        activity_id: hex::encode(&item.facts.activity_id),
        receipt_digest: hex::encode(&item.receipt_digest),
    };
    Ok(Identity {
        did: did.to_owned(),
        authorities: vec![Authority {
            kind: "primary_key".to_owned(),
            id: hex::encode(&state[37..69]),
        }],
        revocation_sequence: native_u64(&state, 69)?,
        frozen: false,
        rotation: key_policy(&state, &reference, item.facts.global_sequence, false)?,
        recovery: key_policy(&state, &reference, item.facts.global_sequence, true)?,
        evidence: reference,
        capabilities: Vec::new(),
    })
}

fn validate_parameters(name: &str, params: &BTreeMap<String, String>) -> Result<(), Response> {
    let specific: &[&str] = match name {
        "subject-context" | "registry" | "balance-context" | "core-clock" => &[],
        "authorized-batch" => &["activity_id", "signed_activity"],
        "identity" => &["did"],
        "key-policy" => &["did", "recovery"],
        "capability-scope" => &["did", "authority", "action_key", "capability_id"],
        "budget-state" => &["budget_id"],
        _ => return Err(refusal(404, "not_found", None)),
    };
    let base = [
        "tenant",
        "principal",
        "subject_principal",
        "owner_did",
        "owner_account",
        "asset_id",
    ];
    if base
        .iter()
        .chain(specific)
        .any(|key| !params.contains_key(*key))
        || params.keys().any(|key| {
            !base.contains(&key.as_str())
                && !specific.contains(&key.as_str())
                && key != "registration"
        })
        || (params.contains_key("registration")
            && !matches!(
                name,
                "identity" | "key-policy" | "capability-scope" | "authorized-batch"
            ))
    {
        return Err(refusal(400, "invalid_query", None));
    }
    Ok(())
}

fn context(subject: &Subject, current: &Current) -> Response {
    json(
        200,
        &value!({"tenant":subject.binding.tenant(),"subject_principal":subject.binding.principal(),"agent_tenant":subject.binding.agent_tenant(),"owner_did":subject.did,"account_id":hex::encode(&subject.account_id),"asset_id":hex::encode(&subject.asset),"verification_level":"checkpoint_finalised","observed_head_sequence":current.head,"checkpoint_digest":hex::encode(&current.checkpoint)}),
    )
}

fn balance(
    human: &Human,
    policy: &PrincipalPolicy,
    subject: &Subject,
    current: &Current,
) -> Result<Response, Response> {
    let (registry, bytes) = super::read_registry(&human.registry_path)?;
    let asset = registry
        .assets
        .iter()
        .find(|asset| asset.asset == hex::encode(&subject.asset))
        .ok_or_else(|| unavailable("registry_currency_metadata_unavailable"))?;
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|_| unavailable("clock_unavailable"))?
        .as_millis();
    let age = now
        .checked_sub(u128::from(current.timestamp))
        .ok_or_else(|| unavailable("head_timestamp_in_future"))?;
    if age > u128::from(policy.maximum_age_seconds) * 1000 {
        return Err(unavailable("balance_evidence_stale"));
    }
    Ok(json(
        200,
        &value!({"account_id":hex::encode(&subject.account_id),"asset_id":hex::encode(&subject.asset),"currency":asset.currency,"decimals":asset.decimals,"symbol":asset.symbol,"registry_revision":hex::encode(&digest(&bytes)),"observed_at":current.timestamp.to_string(),"age_seconds":u64::try_from(age/1000).map_err(|_| unavailable("clock_unavailable"))?,"maximum_age_seconds":policy.maximum_age_seconds,"sequencer_id":hex::encode(&current.authorization.sequencer_id()),"sequencer_public_key":hex::encode(&current.authorization.public_key()),"first_batch_number":current.authorization.first_batch_number(),"last_batch_number":current.authorization.last_batch_number(),"checkpoint_digest":hex::encode(&current.checkpoint)}),
    ))
}

fn authority(
    config: &Config,
    human: &Human,
    subject: &Subject,
    params: &BTreeMap<String, String>,
    evidence: &[Verified],
) -> Result<Response, Response> {
    let bytes = hex::decode(&params["signed_activity"])
        .map_err(|_| refusal(403, "original_activity_invalid", None))?;
    let activity = signed(&bytes, &registry(human)?, config.protocol_network_id)?;
    if hex::encode(
        &activity_id(&activity).map_err(|_| refusal(403, "original_activity_invalid", None))?,
    ) != params["activity_id"]
    {
        return Err(refusal(403, "original_activity_invalid", None));
    }
    let actor = std::str::from_utf8(activity.actor_did())
        .map_err(|_| refusal(403, "original_activity_invalid", None))?;
    child(
        config,
        human,
        subject,
        actor,
        params.get("registration"),
        evidence,
    )?;
    let item = held(evidence, &activity)?;
    let (_, prior) = identity_at(
        evidence,
        actor,
        item.facts
            .global_sequence
            .checked_sub(1)
            .ok_or_else(|| refusal(403, "original_activity_invalid", None))?,
    )?;
    if prior[37..69] != activity.authority()[..] {
        return Err(refusal(403, "original_activity_authority_mismatch", None));
    }
    let receipt = layerx_wire::receipt::decode(
        &hex::decode(&item.record.receipt_hex)
            .map_err(|_| unavailable("state_evidence_refused"))?,
    )
    .map_err(|_| unavailable("state_evidence_refused"))?;
    let protocol = receipt
        .protocol()
        .ok_or_else(|| unavailable("state_evidence_refused"))?;
    let header = layerx_wire::receipt::decode_batch_header(&item.header)
        .map_err(|_| unavailable("state_evidence_refused"))?;
    if header.network_id() != activity.network_id()
        || protocol.protocol_version() != activity.protocol_version()
        || protocol.module_id() != activity.activity_type().module() as u16
    {
        return Err(refusal(403, "original_activity_receipt_mismatch", None));
    }
    super::selected_activity_authority(
        value!({"batch_id":hex::encode(&item.facts.batch_id),"asset":hex::encode(&item.facts.asset),"sequencer_public_key":hex::encode(&item.facts.sequencer_public_key)}),
        &item.record.receipt_hex,
    )
}

fn capability(
    identity: &mut Identity,
    params: &BTreeMap<String, String>,
    evidence: &[Verified],
) -> Result<(), Response> {
    for item in evidence.iter().rev() {
        let Ok(state) = native_identity_state(item, &identity.did) else {
            continue;
        };
        let bytes = hex::decode(&item.record.receipt_hex)
            .map_err(|_| unavailable("capability_state_proof_unavailable"))?;
        let receipt = layerx_wire::receipt::decode(&bytes)
            .map_err(|_| unavailable("capability_state_proof_unavailable"))?;
        let protocol = receipt
            .protocol()
            .ok_or_else(|| unavailable("capability_state_proof_unavailable"))?;
        let summaries = protocol
            .effects()
            .iter()
            .filter(|effect| effect.module_id() == 7 && effect.event_type() == 0x7145)
            .collect::<Vec<_>>();
        if summaries.len() != 1 {
            continue;
        }
        let summary = summaries[0].body();
        if summary.len() != 209 || hex::encode(&summary[5..37]) != params["capability_id"] {
            continue;
        }
        if summary[69..101] != state[37..69]
            || hex::encode(&summary[69..101]) != params["authority"]
            || hex::encode(&summary[101..133]) != params["action_key"]
        {
            return Err(refusal(403, "capability_not_bound", None));
        }
        let minimum = u16::from_be_bytes([summary[181], summary[182]]);
        let maximum = u16::from_be_bytes([summary[183], summary[184]]);
        if minimum == 0 || maximum < minimum {
            return Err(refusal(403, "capability_not_bound", None));
        }
        identity.capabilities.push(ScopeBinding {
            authority: params["authority"].clone(),
            action_key: params["action_key"].clone(),
            capability_id: params["capability_id"].clone(),
            activity_types: (minimum..=maximum).collect(),
            counterparties: Vec::new(),
            assets: Vec::new(),
            amount_ceiling: "0".to_owned(),
            expiry_sequence: native_u64(summary, 165)?,
            enforceable_dimensions: Vec::new(),
            evidence: EvidenceReference {
                activity_id: hex::encode(&item.facts.activity_id),
                receipt_digest: hex::encode(&item.receipt_digest),
            },
        });
        return Ok(());
    }
    Err(refusal(403, "capability_not_bound", None))
}

pub(super) fn dispatch(
    config: &Config,
    human: &Human,
    policy: &PrincipalPolicy,
    name: &str,
    params: &BTreeMap<String, String>,
) -> Result<Response, Response> {
    validate_parameters(name, params)?;
    let subject = subject(human, params)?;
    let current = current(config, &subject, policy)?;
    if name == "subject-context" {
        return Ok(context(&subject, &current));
    }
    if name == "registry" {
        return super::registry(&human.registry_path);
    }
    if name == "balance-context" {
        return balance(human, policy, &subject, &current);
    }
    let mut evidence = human.evidence(config)?;
    for item in &mut evidence {
        item.checkpoint = super::super::checkpoint_header(config, item.facts.batch_number).ok();
    }
    let latest = evidence
        .last()
        .ok_or_else(|| unavailable("identity_head_evidence_incomplete"))?;
    let checkpoint = native_checkpoint(latest)?;
    if latest.last_sequence != current.head
        || checkpoint.report().evidence().checkpoint_id() != Some(current.checkpoint)
    {
        return Err(unavailable("identity_head_evidence_incomplete"));
    }
    let target = params
        .get("did")
        .map_or(subject.did.as_str(), String::as_str);
    if name == "authorized-batch" {
        return authority(config, human, &subject, params, &evidence);
    }
    child(
        config,
        human,
        &subject,
        target,
        params.get("registration"),
        &evidence,
    )?;
    let (anchor, _) = identity_at(&evidence, target, current.head)?;
    let mut identity = identity(anchor, target)?;
    let (state, authorities) = session_membership::current(&identity, anchor, &evidence, policy)?;
    if target == subject.did && state != current.state {
        return Err(unavailable("identity_head_evidence_incomplete"));
    }
    if name == "identity" {
        return Ok(json(
            200,
            &value!({"authorities":authorities,"canonical_core_bytes":hex::encode(&state),"head_sequence":current.head,"revocation_sequence":native_u64(&state,69)?,"frozen":false,"verification_level":"checkpoint_finalised"}),
        ));
    }
    if name == "capability-scope" {
        let id = &params["capability_id"];
        if !authorities
            .iter()
            .any(|authority| authority.kind == "session_key" && &authority.id == id)
        {
            return Err(refusal(403, "capability_not_bound", None));
        }
        capability(&mut identity, params, &evidence)?;
    }
    let mut bound = policy.clone();
    bound.account_id = hex::encode(&subject.account_id);
    bound.asset_id = hex::encode(&subject.asset);
    bound.identities = vec![identity];
    match name {
        "core-clock" => super::clock(&evidence, human.horizon),
        "budget-state" => budget_state::read_dynamic(config, &bound, &params["budget_id"]),
        _ => super::policy_route(name, params, &bound, &evidence),
    }
}

#[cfg(test)]
mod tests;
