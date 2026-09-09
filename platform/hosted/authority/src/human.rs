use super::{by_activity, hex, json, protected, refusal, Config, Request, Response};
use layerx_platform_authority::{
    authorized_batch_by_activity, parse_replica_evidence, receipt_locator, AuthorityFacts,
};
use layerx_proof::inclusion::SequencerAuthorization;
use layerx_wire::receipt::{decode, decode_batch_header};
use serde::{Deserialize, Serialize};
use serde_json::{json as value, Value};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet};
use std::fs::{self, File, OpenOptions};
use std::io::Write;
use std::os::unix::fs::{MetadataExt, OpenOptionsExt};
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use subtle::ConstantTimeEq;
use zeroize::Zeroizing;

const MAX_FILE: u64 = 16 * 1024 * 1024;

#[derive(Clone, Deserialize)]
#[serde(deny_unknown_fields)]
struct Policy {
    principals: Vec<PrincipalPolicy>,
}

#[derive(Clone, Deserialize)]
#[serde(deny_unknown_fields)]
struct PrincipalPolicy {
    tenant: String,
    principal: String,
    account_id: String,
    asset_id: String,
    activities: Vec<String>,
    budgets: Vec<String>,
    maximum_age_seconds: u64,
    maximum_age_sequences: u64,
    identities: Vec<Identity>,
}

#[derive(Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct EvidenceReference {
    activity_id: String,
    receipt_digest: String,
}

#[derive(Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct Identity {
    did: String,
    authorities: Vec<Authority>,
    revocation_sequence: u64,
    frozen: bool,
    evidence: EvidenceReference,
    capabilities: Vec<ScopeBinding>,
    rotation: KeyPolicy,
    recovery: KeyPolicy,
}

#[derive(Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct Authority {
    kind: String,
    id: String,
}

#[derive(Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct ScopeBinding {
    authority: String,
    action_key: String,
    capability_id: String,
    activity_types: Vec<u16>,
    counterparties: Vec<String>,
    assets: Vec<String>,
    amount_ceiling: String,
    expiry_sequence: u64,
    enforceable_dimensions: Vec<String>,
    evidence: EvidenceReference,
}

#[derive(Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct KeyPolicy {
    policy_revision: u64,
    required_delay_seconds: u64,
    maximum_delay_seconds: u64,
    effective_sequence: u64,
    evidence: EvidenceReference,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Registry {
    schema_version: u16,
    assets: Vec<CurrencyMetadata>,
    modules: Vec<Module>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct CurrencyMetadata {
    asset: String,
    currency: String,
    decimals: u8,
    symbol: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Module {
    module: u16,
    ordinals: Vec<u16>,
}

#[derive(Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Record {
    receipt_hex: String,
    replica_document: Value,
}

struct Verified {
    record: Record,
    facts: AuthorityFacts,
    receipt_digest: [u8; 32],
    header: Vec<u8>,
    header_signature: [u8; 64],
    timestamp_ms: u64,
    last_sequence: u64,
    checkpoint: Option<layerx_client::evidence::VerifiedCheckpoint>,
}

pub(super) struct Human {
    token: Zeroizing<Vec<u8>>,
    tenant: String,
    principal: String,
    policy_path: PathBuf,
    policy_digest: Option<[u8; 32]>,
    policy: Option<Policy>,
    registry_path: PathBuf,
    state_root: PathBuf,
    horizon: u64,
    records: Mutex<()>,
}

fn required(name: &str) -> Result<String, String> {
    std::env::var(name).map_err(|_| format!("{name} is required"))
}

fn digest(bytes: &[u8]) -> [u8; 32] {
    Sha256::digest(bytes).into()
}

fn valid_digest(value: &str) -> bool {
    hex::decode32(value).is_ok_and(|id| id != [0; 32])
}

fn reference_valid(reference: &EvidenceReference, principal: &PrincipalPolicy) -> bool {
    valid_digest(&reference.receipt_digest) && principal.activities.contains(&reference.activity_id)
}

fn policy_valid(policy: &Policy) -> bool {
    let mut pairs = BTreeSet::new();
    policy.principals.iter().all(|p| {
        !p.tenant.is_empty()
            && !p.principal.is_empty()
            && pairs.insert((&p.tenant, &p.principal))
            && valid_digest(&p.account_id)
            && valid_digest(&p.asset_id)
            && p.maximum_age_seconds > 0
            && p.maximum_age_sequences > 0
            && p.activities
                .iter()
                .chain(&p.budgets)
                .all(|id| valid_digest(id))
            && unique(&p.activities)
            && unique(&p.budgets)
            && p.identities
                .iter()
                .map(|i| &i.did)
                .collect::<BTreeSet<_>>()
                .len()
                == p.identities.len()
            && p.identities.iter().all(|i| identity_valid(i, p))
    })
}

fn unique<T: Ord>(values: &[T]) -> bool {
    values.iter().collect::<BTreeSet<_>>().len() == values.len()
}

fn identity_valid(i: &Identity, p: &PrincipalPolicy) -> bool {
    !i.did.is_empty()
        && i.revocation_sequence > 0
        && !i.authorities.is_empty()
        && reference_valid(&i.evidence, p)
        && i.authorities.iter().all(|a| {
            valid_digest(&a.id)
                && matches!(
                    a.kind.as_str(),
                    "primary_key" | "session_key" | "capability_grant"
                )
        })
        && i.capabilities
            .iter()
            .map(|c| (&c.authority, &c.action_key, &c.capability_id))
            .collect::<BTreeSet<_>>()
            .len()
            == i.capabilities.len()
        && i.capabilities.iter().all(|c| {
            valid_digest(&c.authority)
                && valid_digest(&c.action_key)
                && valid_digest(&c.capability_id)
                && i.authorities.iter().any(|a| a.id == c.authority)
                && c.amount_ceiling.parse::<u128>().is_ok()
                && !c.amount_ceiling.is_empty()
                && c.amount_ceiling.bytes().all(|b| b.is_ascii_digit())
                && c.expiry_sequence > 0
                && reference_valid(&c.evidence, p)
                && c.assets
                    .iter()
                    .chain(&c.counterparties)
                    .all(|id| valid_digest(id))
                && c.enforceable_dimensions.iter().all(|d| {
                    matches!(
                        d.as_str(),
                        "activity_type"
                            | "counterparty"
                            | "asset"
                            | "amount"
                            | "rate"
                            | "purpose"
                            | "expiry"
                    )
                })
        })
        && [&i.rotation, &i.recovery].iter().all(|k| {
            k.policy_revision > 0
                && k.required_delay_seconds > 0
                && k.maximum_delay_seconds >= k.required_delay_seconds
                && k.effective_sequence > 0
                && reference_valid(&k.evidence, p)
        })
}

fn root_valid(path: &Path) -> Result<(), ()> {
    let metadata = fs::symlink_metadata(path).map_err(|_| ())?;
    if !path.is_absolute()
        || fs::canonicalize(path).map_err(|_| ())? != path
        || !metadata.is_dir()
        || metadata.mode() & 0o077 != 0
        || metadata.uid() != fs::metadata("/proc/self").map_err(|_| ())?.uid()
    {
        return Err(());
    }
    Ok(())
}

impl Human {
    pub(super) fn load(tokens: &[Zeroizing<String>]) -> Result<Option<Self>, String> {
        if std::env::var_os("LAYERX_AUTHORITY_HUMAN_AGENT_TOKEN_FILE").is_none() {
            let partial = [
                "HUMAN_AGENT_TENANT",
                "HUMAN_AGENT_PRINCIPAL",
                "PRINCIPAL_POLICY_FILE",
                "MODULE_REGISTRY_FILE",
                "CORE_CLOCK_HORIZON",
                "STATE_ROOT",
            ]
            .iter()
            .any(|suffix| std::env::var_os(format!("LAYERX_AUTHORITY_{suffix}")).is_some());
            return if partial {
                Err("incomplete human authority configuration".to_owned())
            } else {
                Ok(None)
            };
        }
        let path = PathBuf::from(required("LAYERX_AUTHORITY_HUMAN_AGENT_TOKEN_FILE")?);
        let token = Zeroizing::new(
            protected::read(&path, 4096)
                .map_err(|()| "human token file is unavailable or unprotected")?,
        );
        if token.len() < 32
            || !token.iter().all(|b| (0x21..=0x7e).contains(b))
            || tokens
                .iter()
                .any(|other| bool::from(other.as_bytes().ct_eq(&token)))
        {
            return Err("human token must be distinct and contain 32..4096 printable bytes without whitespace".to_owned());
        }
        let policy_path = PathBuf::from(required("LAYERX_AUTHORITY_PRINCIPAL_POLICY_FILE")?);
        let loaded = protected::read(&policy_path, MAX_FILE)
            .ok()
            .and_then(|bytes| {
                let policy: Policy = serde_json::from_slice(&bytes).ok()?;
                policy_valid(&policy).then(|| (digest(&bytes), policy))
            });
        let (policy_digest, policy) = loaded.map_or((None, None), |(d, p)| (Some(d), Some(p)));
        let horizon = required("LAYERX_AUTHORITY_CORE_CLOCK_HORIZON")?
            .parse::<u64>()
            .map_err(|_| "core clock horizon must be an integer")?;
        if horizon == 0 {
            return Err("core clock horizon must be positive".to_owned());
        }
        let state_root = PathBuf::from(required("LAYERX_AUTHORITY_STATE_ROOT")?);
        root_valid(&state_root)
            .map_err(|()| "state root must be an existing protected authority-owned directory")?;
        let tenant = required("LAYERX_AUTHORITY_HUMAN_AGENT_TENANT")?;
        let principal = required("LAYERX_AUTHORITY_HUMAN_AGENT_PRINCIPAL")?;
        if tenant.is_empty() || principal.is_empty() {
            return Err("human tenant and principal must be nonempty".to_owned());
        }
        Ok(Some(Self {
            token,
            tenant,
            principal,
            policy_path,
            policy_digest,
            policy,
            registry_path: PathBuf::from(required("LAYERX_AUTHORITY_MODULE_REGISTRY_FILE")?),
            state_root,
            horizon,
            records: Mutex::new(()),
        }))
    }

    fn principal(&self) -> Result<&PrincipalPolicy, Response> {
        let bytes = protected::read(&self.policy_path, MAX_FILE)
            .map_err(|()| unavailable("policy_unavailable"))?;
        if Some(digest(&bytes)) != self.policy_digest {
            return Err(unavailable("policy_changed_restart_required"));
        }
        self.policy
            .as_ref()
            .ok_or_else(|| unavailable("policy_unavailable"))?
            .principals
            .iter()
            .find(|p| p.tenant == self.tenant && p.principal == self.principal)
            .ok_or_else(|| refusal(404, "principal_not_provisioned", None))
    }

    pub(super) fn retain(
        &self,
        receipt: &[u8],
        document: &[u8],
        config: &Config,
    ) -> Result<(), ()> {
        let _guard = self.records.lock().map_err(|_| ())?;
        root_valid(&self.state_root)?;
        let record = Record {
            receipt_hex: hex::encode(receipt),
            replica_document: serde_json::from_slice(document).map_err(|_| ())?,
        };
        let verified = verify(record.clone(), &config.authorization, config)?;
        let path = self
            .state_root
            .join(format!("{}.json", hex::encode(&verified.facts.activity_id)));
        let bytes = serde_json::to_vec(&record).map_err(|_| ())?;
        if path.try_exists().map_err(|_| ())? {
            if protected::read(&path, MAX_FILE)? != bytes {
                return Err(());
            }
            return Ok(());
        }
        let temporary = self.state_root.join(".receipt.pending");
        let mut file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .open(&temporary)
            .map_err(|_| ())?;
        file.write_all(&bytes)
            .and_then(|()| file.sync_all())
            .map_err(|_| ())?;
        fs::rename(&temporary, &path).map_err(|_| ())?;
        File::open(&self.state_root)
            .and_then(|dir| dir.sync_all())
            .map_err(|_| ())
    }

    fn evidence(&self, config: &Config) -> Result<Vec<Verified>, Response> {
        let _guard = self
            .records
            .lock()
            .map_err(|_| unavailable("state_unavailable"))?;
        root_valid(&self.state_root).map_err(|()| unavailable("state_unavailable"))?;
        let mut verified = Vec::new();
        for entry in fs::read_dir(&self.state_root).map_err(|_| unavailable("state_unavailable"))? {
            let path = entry.map_err(|_| unavailable("state_unavailable"))?.path();
            let bytes =
                protected::read(&path, MAX_FILE).map_err(|()| unavailable("state_unavailable"))?;
            let record =
                serde_json::from_slice(&bytes).map_err(|_| unavailable("state_unavailable"))?;
            let item = verify(record, &config.authorization, config)
                .map_err(|()| unavailable("state_evidence_refused"))?;
            if path.file_name().and_then(|s| s.to_str())
                != Some(format!("{}.json", hex::encode(&item.facts.activity_id)).as_str())
            {
                return Err(unavailable("state_evidence_refused"));
            }
            verified.push(item);
        }
        verified.sort_by_key(|r| r.facts.global_sequence);
        if verified
            .windows(2)
            .any(|pair| pair[0].facts.global_sequence == pair[1].facts.global_sequence)
        {
            return Err(unavailable("state_sequence_conflict"));
        }
        Ok(verified)
    }
}

fn verify(
    record: Record,
    authorization: &SequencerAuthorization,
    config: &Config,
) -> Result<Verified, ()> {
    let receipt = hex::decode(&record.receipt_hex).map_err(|_| ())?;
    let locator = receipt_locator(&receipt).map_err(|_| ())?;
    let evidence = parse_replica_evidence(
        &serde_json::to_vec(&record.replica_document).map_err(|_| ())?,
        config.replica_id,
        config.sequencer_public_key,
    )
    .map_err(|_| ())?;
    let facts =
        authorized_batch_by_activity(locator.activity_id, &receipt, &evidence, authorization)
            .map_err(|_| ())?;
    let header = decode_batch_header(&evidence.header).map_err(|_| ())?;
    if header.network_id() != config.protocol_network_id {
        return Err(());
    }
    Ok(Verified {
        record,
        facts,
        checkpoint: None,
        receipt_digest: locator.receipt_digest,
        timestamp_ms: header.timestamp_ms(),
        last_sequence: header.last_sequence(),
        header: evidence.header,
        header_signature: evidence.header_signature,
    })
}

fn unavailable(code: &str) -> Response {
    refusal(503, code, Some(5))
}

fn query(source: Option<&str>) -> Result<BTreeMap<String, String>, Response> {
    let mut output = BTreeMap::new();
    for pair in source
        .ok_or_else(|| refusal(400, "query_required", None))?
        .split('&')
    {
        let (key, value) = pair
            .split_once('=')
            .ok_or_else(|| refusal(400, "invalid_query", None))?;
        let key = percent_decode(key)?;
        let value = percent_decode(value)?;
        if output.insert(key, value).is_some() {
            return Err(refusal(400, "duplicate_query", None));
        }
    }
    Ok(output)
}

fn percent_decode(source: &str) -> Result<String, Response> {
    let mut bytes = Vec::new();
    let mut offset = 0;
    while offset < source.len() {
        let byte = source.as_bytes()[offset];
        if byte == b'%' {
            let end = offset + 3;
            let encoded = source
                .get(offset + 1..end)
                .ok_or_else(|| refusal(400, "invalid_query", None))?;
            bytes.extend(hex::decode(encoded).map_err(|_| refusal(400, "invalid_query", None))?);
            offset = end;
        } else {
            bytes.push(byte);
            offset += 1;
        }
    }
    String::from_utf8(bytes).map_err(|_| refusal(400, "invalid_query", None))
}

pub(super) fn route(config: &Config, request: &Request) -> Response {
    dispatch(config, request).unwrap_or_else(|response| response)
}

fn dispatch(config: &Config, request: &Request) -> Result<Response, Response> {
    let human = config
        .human
        .as_ref()
        .ok_or_else(|| unavailable("human_authority_unconfigured"))?;
    let presented = request
        .headers
        .get("authorization")
        .and_then(|v| v.strip_prefix("Bearer "))
        .ok_or_else(|| refusal(401, "identity_required", None))?;
    if !bool::from(human.token.as_slice().ct_eq(presented.as_bytes())) {
        return Err(refusal(401, "identity_required", None));
    }
    let params = query(request.query.as_deref())?;
    if params.get("tenant") != Some(&human.tenant)
        || params.get("principal") != Some(&human.principal)
    {
        return Err(refusal(403, "principal_mismatch", None));
    }
    let p = human.principal()?;
    let name = request
        .path
        .strip_prefix("/v1/agent/")
        .ok_or_else(|| refusal(404, "not_found", None))?;
    let additional: &[&str] = match name {
        "registry" | "balance-context" | "core-clock" => &[],
        "authorized-batch" => &["activity_id"],
        "identity" => &["did"],
        "capability-scope" => &["did", "authority", "action_key", "capability_id"],
        "budget-state" => &["budget_id"],
        "key-policy" => &["did", "recovery"],
        _ => return Err(refusal(404, "not_found", None)),
    };
    if params.len() != additional.len() + 2
        || additional.iter().any(|key| !params.contains_key(*key))
    {
        return Err(refusal(400, "invalid_query", None));
    }
    if name == "registry" {
        return registry(&human.registry_path);
    }
    if name == "authorized-batch" {
        let activity = &params["activity_id"];
        authorize_activity(p, activity)?;
        if let Some(held) = human
            .evidence(config)?
            .iter()
            .find(|e| hex::encode(&e.facts.activity_id) == *activity)
        {
            return Ok(json(
                200,
                &value!({"batch_id": hex::encode(&held.facts.batch_id), "asset": hex::encode(&held.facts.asset), "previous_state_root": hex::encode(&held.facts.previous_state_root), "resulting_state_root": hex::encode(&held.facts.resulting_state_root), "sequencer_public_key": hex::encode(&held.facts.sequencer_public_key)}),
            ));
        }
        return Ok(by_activity(config, activity, false));
    }
    let mut evidence = human.evidence(config)?;
    if matches!(name, "identity" | "key-policy" | "capability-scope") {
        for item in &mut evidence {
            item.checkpoint = super::checkpoint_header(config, item.facts.batch_number).ok();
        }
    }
    match name {
        "core-clock" => clock(&evidence, human.horizon),
        "balance-context" => balance_context(p, &human.registry_path, &evidence, config),
        "budget-state" => budget(p, &params["budget_id"], &evidence),
        _ => policy_route(name, &params, p, &evidence),
    }
}

fn authorize_activity(p: &PrincipalPolicy, activity: &str) -> Result<(), Response> {
    if !valid_digest(activity) {
        return Err(refusal(400, "invalid_activity_id", None));
    }
    if !p.activities.iter().any(|id| id == activity) {
        return Err(refusal(403, "activity_not_bound", None));
    }
    Ok(())
}

fn budget(p: &PrincipalPolicy, id: &str, evidence: &[Verified]) -> Result<Response, Response> {
    if !p.budgets.iter().any(|bound| bound == id) {
        return Err(refusal(404, "budget_not_bound", None));
    }
    let mut response = unavailable("budget_revocation_and_checkpoint_evidence_unavailable");
    let mut body = serde_json::from_slice::<Value>(&response.body)
        .map_err(|_| unavailable("encoding_failed"))?;
    body["evidence"] = account_summary(p, evidence)?;
    response.body = body.to_string().into_bytes();
    Ok(response)
}

fn read_registry(path: &Path) -> Result<(Registry, Vec<u8>), Response> {
    let bytes =
        protected::read(path, MAX_FILE).map_err(|()| unavailable("module_registry_unavailable"))?;
    let registry: Registry =
        serde_json::from_slice(&bytes).map_err(|_| unavailable("module_registry_invalid"))?;
    let mut seen = BTreeSet::new();
    if registry.schema_version != 2
        || registry.assets.is_empty()
        || registry.assets.len() > 256
        || registry.assets.iter().any(|a| {
            !valid_digest(&a.asset)
                || a.asset != a.asset.to_ascii_lowercase()
                || a.currency.is_empty()
                || a.currency.len() > 32
                || a.currency.chars().any(char::is_control)
                || a.symbol.is_empty()
                || a.symbol.len() > 32
                || a.symbol.chars().any(char::is_control)
                || a.decimals > 38
        })
        || registry
            .assets
            .iter()
            .map(|a| &a.asset)
            .collect::<BTreeSet<_>>()
            .len()
            != registry.assets.len()
        || registry.modules.is_empty()
        || registry.modules.len() > 32
        || registry.modules.iter().any(|m| {
            !(1..=9).contains(&m.module)
                || !seen.insert(m.module)
                || m.ordinals.is_empty()
                || m.ordinals.len() > 64
                || m.ordinals.contains(&0)
                || m.ordinals.windows(2).any(|p| p[0] >= p[1])
        })
    {
        return Err(unavailable("module_registry_invalid"));
    }
    Ok((registry, bytes))
}

fn registry(path: &Path) -> Result<Response, Response> {
    let (registry, bytes) = read_registry(path)?;
    let modules: Vec<_> = registry.modules.iter().map(|m| value!({"module_id": m.module,
        "activity_types": m.ordinals.iter().map(|o| (u32::from(m.module) << 16) | u32::from(*o)).collect::<Vec<_>>() })).collect();
    Ok(json(
        200,
        &value!({"modules": modules, "revision": hex::encode(&digest(&bytes))}),
    ))
}

fn balance_context(
    p: &PrincipalPolicy,
    registry_path: &Path,
    evidence: &[Verified],
    config: &Config,
) -> Result<Response, Response> {
    let (registry, bytes) = read_registry(registry_path)?;
    let asset = registry
        .assets
        .iter()
        .find(|a| a.asset == p.asset_id)
        .ok_or_else(|| unavailable("registry_currency_metadata_unavailable"))?;
    let head = evidence
        .iter()
        .max_by_key(|e| e.last_sequence)
        .ok_or_else(|| unavailable("head_unavailable"))?;
    let now_ms = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_err(|_| unavailable("clock_unavailable"))?
        .as_millis();
    let age_ms = now_ms
        .checked_sub(u128::from(head.timestamp_ms))
        .ok_or_else(|| unavailable("head_timestamp_in_future"))?;
    if age_ms > u128::from(p.maximum_age_seconds) * 1000 {
        return Err(unavailable("balance_evidence_stale"));
    }
    Ok(json(
        200,
        &value!({
            "account_id": p.account_id, "asset_id": p.asset_id,
            "currency": asset.currency, "decimals": asset.decimals, "symbol": asset.symbol,
            "registry_revision": hex::encode(&digest(&bytes)),
            "observed_at": head.timestamp_ms.to_string(),
            "age_seconds": u64::try_from(age_ms / 1000).map_err(|_| unavailable("clock_unavailable"))?,
            "maximum_age_seconds": p.maximum_age_seconds,
            "sequencer_id": hex::encode(&config.sequencer_id),
            "sequencer_public_key": hex::encode(&config.sequencer_public_key),
            "first_batch_number": config.first_batch, "last_batch_number": config.last_batch,
            "evidence": account_summary(p, evidence)?
        }),
    ))
}

fn clock(evidence: &[Verified], horizon: u64) -> Result<Response, Response> {
    let mut headers = BTreeMap::new();
    for item in evidence {
        if let Some(previous) = headers.insert(item.last_sequence, item) {
            if previous.header != item.header || previous.header_signature != item.header_signature
            {
                return Err(unavailable("header_conflict"));
            }
        }
    }
    let mut latest = headers.values().rev();
    let upper = latest
        .next()
        .ok_or_else(|| unavailable("clock_anchors_unavailable"))?;
    let lower = latest
        .next()
        .ok_or_else(|| unavailable("clock_anchors_unavailable"))?;
    let (upper_ms, upper_sequence) = extrapolate(
        lower.last_sequence,
        lower.timestamp_ms,
        upper.last_sequence,
        upper.timestamp_ms,
        horizon,
    )
    .ok_or_else(|| unavailable("clock_anchors_invalid"))?;
    let canonical = serde_json::to_vec(&value!({"domain": "layerx-human/core-clock/v1", "lower_header": hex::encode(&lower.header), "lower_signature": hex::encode(&lower.header_signature), "head_header": hex::encode(&upper.header), "head_signature": hex::encode(&upper.header_signature), "horizon": horizon, "upper_unix_ms": upper_ms, "upper_sequence": upper_sequence})).map_err(|_| unavailable("encoding_failed"))?;
    Ok(json(
        200,
        &value!({"lower_unix_ms": lower.timestamp_ms, "lower_sequence": lower.last_sequence,
        "upper_unix_ms": upper_ms, "upper_sequence": upper_sequence, "observed_head_sequence": upper.last_sequence,
        "canonical_attestation": hex::encode(&canonical)}),
    ))
}

fn extrapolate(
    lower_sequence: u64,
    lower_ms: u64,
    head_sequence: u64,
    head_ms: u64,
    horizon: u64,
) -> Option<(u64, u64)> {
    if lower_sequence == 0 || horizon == 0 {
        return None;
    }
    let sequences = head_sequence
        .checked_sub(lower_sequence)
        .filter(|v| *v > 0)?;
    let millis = head_ms.checked_sub(lower_ms).filter(|v| *v > 0)?;
    let extension = u128::from(horizon)
        .checked_mul(u128::from(millis))?
        .checked_div(u128::from(sequences))?;
    let upper_ms = head_ms.checked_add(u64::try_from(extension).ok()?)?;
    if upper_ms <= head_ms {
        return None;
    }
    Some((upper_ms, head_sequence.checked_add(horizon)?))
}

fn account_summary(p: &PrincipalPolicy, evidence: &[Verified]) -> Result<Value, Response> {
    let account = hex::decode32(&p.account_id).map_err(|_| unavailable("policy_invalid"))?;
    let asset = hex::decode32(&p.asset_id).map_err(|_| unavailable("policy_invalid"))?;
    let mut balance = 0_u128;
    let mut account_digests = Vec::new();
    let mut observed_sequence = None;
    for item in evidence {
        let bytes = hex::decode(&item.record.receipt_hex)
            .map_err(|_| unavailable("state_evidence_refused"))?;
        let receipt = decode(&bytes).map_err(|_| unavailable("state_evidence_refused"))?;
        let receipt = receipt
            .protocol()
            .ok_or_else(|| unavailable("state_evidence_refused"))?;
        if receipt.asset() == asset && (receipt.from() == account || receipt.to() == account) {
            balance = if receipt.to() == account {
                receipt.credit_balance_after()
            } else {
                receipt.debit_balance_after()
            };
            account_digests.push(hex::encode(&item.receipt_digest));
            observed_sequence = Some(item.facts.global_sequence);
        }
    }
    let all: Vec<_> = evidence
        .iter()
        .map(|e| hex::encode(&e.receipt_digest))
        .collect();
    let canonical = serde_json::to_vec(
        &value!({"domain": "layerx-human/held-evidence/v1", "receipts": all, "checkpoints": []}),
    )
    .map_err(|_| unavailable("encoding_failed"))?;
    Ok(
        value!({"account_id": p.account_id, "asset_id": p.asset_id, "remaining": balance.to_string(),
        "observed_head_sequence": evidence.iter().map(|r| r.last_sequence).max(), "account_observed_sequence": observed_sequence,
        "receipt_digests": account_digests, "checkpoint_digests": [], "evidence_digest": hex::encode(&digest(&canonical))}),
    )
}

fn policy_route(
    name: &str,
    params: &BTreeMap<String, String>,
    p: &PrincipalPolicy,
    evidence: &[Verified],
) -> Result<Response, Response> {
    let identity = p
        .identities
        .iter()
        .find(|i| i.did == params["did"])
        .ok_or_else(|| refusal(404, "did_not_bound", None))?;
    let reference = match name {
        "identity" => &identity.evidence,
        "capability-scope" => {
            let c = identity
                .capabilities
                .iter()
                .find(|c| {
                    c.authority == params["authority"]
                        && c.action_key == params["action_key"]
                        && c.capability_id == params["capability_id"]
                })
                .ok_or_else(|| refusal(403, "capability_not_bound", None))?;
            &c.evidence
        }
        "key-policy" => match params["recovery"].as_str() {
            "true" => &identity.recovery.evidence,
            "false" => &identity.rotation.evidence,
            _ => return Err(refusal(400, "invalid_recovery", None)),
        },
        _ => return Err(refusal(404, "not_found", None)),
    };
    let item = evidence
        .iter()
        .find(|e| {
            hex::encode(&e.facts.activity_id) == reference.activity_id
                && hex::encode(&e.receipt_digest) == reference.receipt_digest
        })
        .ok_or_else(|| unavailable("policy_receipt_evidence_unavailable"))?;
    let head = evidence
        .iter()
        .map(|e| e.last_sequence)
        .max()
        .ok_or_else(|| unavailable("head_unavailable"))?;
    if head
        .checked_sub(item.facts.global_sequence)
        .is_none_or(|age| age > p.maximum_age_sequences)
    {
        return Err(unavailable("policy_evidence_stale"));
    }
    let checkpoint = if matches!(name, "identity" | "key-policy" | "capability-scope") {
        let checkpoint = native_checkpoint(item)?;
        Some(checkpoint)
    } else {
        None
    };
    if name == "key-policy" {
        return native_key_policy(
            params["recovery"] == "true",
            identity,
            item,
            evidence,
            p,
            checkpoint,
        );
    }
    if name == "capability-scope" {
        return native_capability_scope(identity, params, item, evidence, head);
    }
    if name == "identity" {
        let state = native_identity_state(item, &identity.did)?;
        let revision = native_u64(&state, 69)?;
        if identity.frozen
            || identity.revocation_sequence != revision
            || identity.authorities.len() != 1
            || identity.authorities[0].kind != "primary_key"
            || identity.authorities[0].id != hex::encode(&state[37..69])
        {
            return Err(unavailable("identity_state_proof_unavailable"));
        }
        native_complete_suffix(item, evidence)?;
        for later in evidence
            .iter()
            .filter(|e| e.facts.global_sequence > item.facts.global_sequence)
        {
            if native_identity_state(later, &identity.did).is_ok() {
                return Err(unavailable("identity_state_proof_unavailable"));
            }
        }
        return Ok(json(
            200,
            &value!({
                "authorities": identity.authorities,
                "canonical_core_bytes": hex::encode(&state),
                "head_sequence": head, "revocation_sequence": revision,
                "frozen": false, "verification_level": "checkpoint_finalised"
            }),
        ));
    }
    Err(unavailable(match name {
        "identity" => "identity_state_proof_unavailable",
        "capability-scope" => "capability_state_proof_unavailable",
        _ => "key_policy_checkpoint_evidence_unavailable",
    }))
}

fn native_capability_scope(
    identity: &Identity,
    params: &BTreeMap<String, String>,
    item: &Verified,
    evidence: &[Verified],
    head: u64,
) -> Result<Response, Response> {
    let refused = || unavailable("capability_state_proof_unavailable");
    let scope = identity
        .capabilities
        .iter()
        .find(|scope| {
            scope.authority == params["authority"]
                && scope.action_key == params["action_key"]
                && scope.capability_id == params["capability_id"]
        })
        .ok_or_else(refused)?;
    let state = native_identity_state(item, &identity.did)?;
    let bytes = hex::decode(&item.record.receipt_hex).map_err(|_| refused())?;
    let receipt = decode(&bytes).map_err(|_| refused())?;
    let protocol = receipt.protocol().ok_or_else(refused)?;
    let summaries: Vec<_> = protocol
        .effects()
        .iter()
        .filter(|effect| effect.module_id() == 7 && effect.event_type() == 0x7145)
        .collect();
    if summaries.len() != 1 || summaries[0].monetary() || summaries[0].kind() != 3 {
        return Err(refused());
    }
    let summary = summaries[0].body();
    if summary.len() != 209
        || &summary[..5] != b"LXGS2"
        || hex::encode(&summary[5..37]) != scope.capability_id
        || summary[37..69] != state[5..37]
        || hex::encode(&summary[69..101]) != scope.authority
        || summary[69..101] != state[37..69]
        || hex::encode(&summary[101..133]) != scope.action_key
        || summary[101..133] == [0; 32]
        || native_u64(summary, 165)? != scope.expiry_sequence
        || scope.expiry_sequence <= head
        || native_u64(summary, 173)? != 2
        || native_u64(summary, 201)? != native_u64(&state, 69)?
        || identity.revocation_sequence != native_u64(&state, 69)?
        || identity.frozen
        || !scope.counterparties.is_empty()
        || !scope.assets.is_empty()
        || scope.amount_ceiling != "0"
        || !scope.enforceable_dimensions.is_empty()
    {
        return Err(refused());
    }
    let minimum = u16::from_be_bytes([summary[181], summary[182]]);
    let maximum = u16::from_be_bytes([summary[183], summary[184]]);
    if minimum == 0
        || minimum > maximum
        || scope.activity_types != (minimum..=maximum).collect::<Vec<_>>()
    {
        return Err(refused());
    }
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_err(|_| refused())?
        .as_millis();
    if now < u128::from(native_u64(summary, 185)?) || now >= u128::from(native_u64(summary, 193)?) {
        return Err(refused());
    }
    native_complete_suffix(item, evidence)?;
    for later in evidence
        .iter()
        .filter(|later| later.facts.global_sequence > item.facts.global_sequence)
    {
        if let Ok(later_state) = native_identity_state(later, &identity.did) {
            if later_state[37..77] != state[37..77] || later_state[111..183] != state[111..183] {
                return Err(refused());
            }
        }
    }
    let checkpoint = native_checkpoint(item)?;
    Ok(json(
        200,
        &value!({
            "activity_types": scope.activity_types, "native_module_mask": 2,
            "counterparties": [], "assets": [], "amount_ceiling": "0",
            "expiry_sequence": scope.expiry_sequence, "action_key": scope.action_key,
            "capability_id": scope.capability_id, "authority": scope.authority,
            "enforceable_dimensions": [], "observed_sequence": head, "verification": 4,
            "evidence_digest": hex::encode(&item.receipt_digest),
            "canonical_core_bytes": hex::encode(summary),
            "checkpoint_digest": hex::encode(&checkpoint.report().evidence().checkpoint_id().ok_or_else(refused)?)
        }),
    ))
}

fn native_checkpoint(
    item: &Verified,
) -> Result<&layerx_client::evidence::VerifiedCheckpoint, Response> {
    let checkpoint = item
        .checkpoint
        .as_ref()
        .ok_or_else(|| unavailable("key_policy_checkpoint_evidence_unavailable"))?;
    if checkpoint.canonical_header() != item.header {
        return Err(unavailable("key_policy_checkpoint_evidence_unavailable"));
    }
    Ok(checkpoint)
}

fn native_key_policy(
    recovery: bool,
    identity: &Identity,
    item: &Verified,
    evidence: &[Verified],
    p: &PrincipalPolicy,
    checkpoint: Option<&layerx_client::evidence::VerifiedCheckpoint>,
) -> Result<Response, Response> {
    let head = evidence
        .iter()
        .map(|e| e.last_sequence)
        .max()
        .ok_or_else(|| unavailable("head_unavailable"))?;
    let policy = if recovery {
        &identity.recovery
    } else {
        &identity.rotation
    };
    let state = native_identity_state(item, &identity.did)?;
    let (revision_offset, delay_offset, maximum_offset) = if recovery {
        (175, 183, 191)
    } else {
        (167, 199, 207)
    };
    let revision = native_u64(&state, revision_offset)?;
    let delay = native_u64(&state, delay_offset)?;
    let maximum = native_u64(&state, maximum_offset)?;
    let (delay, maximum) = if recovery {
        (delay, maximum)
    } else {
        (delay.div_ceil(1000), maximum / 1000)
    };
    if revision == 0
        || delay == 0
        || maximum < delay
        || policy.policy_revision != revision
        || policy.required_delay_seconds != delay
        || policy.maximum_delay_seconds != maximum
        || policy.effective_sequence != item.facts.global_sequence
    {
        return Err(unavailable("key_policy_checkpoint_evidence_unavailable"));
    }
    native_complete_suffix(item, evidence)?;
    for later in evidence
        .iter()
        .filter(|e| e.facts.global_sequence > item.facts.global_sequence)
    {
        if let Ok(later_state) = native_identity_state(later, &identity.did) {
            if later_state[revision_offset..revision_offset + 8]
                != state[revision_offset..revision_offset + 8]
                || later_state[37..77] != state[37..77]
            {
                return Err(unavailable("key_policy_checkpoint_evidence_unavailable"));
            }
        }
    }
    Ok(json(
        200,
        &value!({
            "policy_revision": revision,
            "required_delay_seconds": delay,
            "maximum_delay_seconds": maximum,
            "effective_sequence": policy.effective_sequence,
            "observed_head_sequence": head,
            "verification": 4,
            "evidence_digest": hex::encode(&item.receipt_digest),
            "checkpoint_digest": hex::encode(&checkpoint
                .and_then(|c| c.report().evidence().checkpoint_id())
                .ok_or_else(|| unavailable("key_policy_checkpoint_evidence_unavailable"))?),
            "age_sequences": head - item.facts.global_sequence,
            "maximum_age_sequences": p.maximum_age_sequences
        }),
    ))
}

fn native_u64(state: &[u8], offset: usize) -> Result<u64, Response> {
    let bytes = state
        .get(offset..offset + 8)
        .and_then(|bytes| bytes.try_into().ok())
        .ok_or_else(|| unavailable("identity_state_proof_unavailable"))?;
    Ok(u64::from_be_bytes(bytes))
}

fn native_identity_state(item: &Verified, did: &str) -> Result<Vec<u8>, Response> {
    let refused = || unavailable("identity_state_proof_unavailable");
    if did.is_empty() || did.len() > 255 {
        return Err(refused());
    }
    let mut preimage = b"LXP/v1/did-id\0".to_vec();
    preimage.extend_from_slice(
        &u16::try_from(did.len())
            .map_err(|_| refused())?
            .to_be_bytes(),
    );
    preimage.extend_from_slice(did.as_bytes());
    let did_id = digest(&preimage);
    let bytes = hex::decode(&item.record.receipt_hex).map_err(|_| refused())?;
    let receipt = decode(&bytes).map_err(|_| refused())?;
    let receipt = receipt.protocol().ok_or_else(refused)?;
    if receipt.protocol_version() != 3
        || receipt.module_id() != 7
        || receipt.module_version() != 1
        || receipt.result_code() != 0
    {
        return Err(refused());
    }
    let mut states = receipt
        .effects()
        .iter()
        .filter(|e| e.module_id() == 7 && e.event_type() == 0x7110);
    let effect = states.next().ok_or_else(refused)?;
    let state = effect.body();
    if states.next().is_some()
        || effect.monetary()
        || state.len() != 223
        || &state[..5] != b"LXGI1"
        || state[5..37] != did_id
        || native_u64(state, 215)? != item.facts.global_sequence
        || native_u64(state, 69)? == 0
    {
        return Err(refused());
    }
    Ok(state.to_vec())
}

fn native_complete_suffix(start: &Verified, evidence: &[Verified]) -> Result<(), Response> {
    let refused = || unavailable("identity_state_proof_unavailable");
    let mut batches = BTreeMap::new();
    for item in evidence
        .iter()
        .filter(|e| e.facts.batch_number >= start.facts.batch_number)
    {
        batches
            .entry(item.facts.batch_number)
            .or_insert_with(Vec::new)
            .push(item);
    }
    let mut next_batch = start.facts.batch_number;
    let mut previous_root = None;
    for (number, items) in batches {
        if number != next_batch {
            return Err(refused());
        }
        next_batch = number.checked_add(1).ok_or_else(refused)?;
        let first = items.first().ok_or_else(refused)?;
        let header = decode_batch_header(&first.header).map_err(|_| refused())?;
        if previous_root.is_some_and(|root| root != header.previous_state_root()) {
            return Err(refused());
        }
        previous_root = Some(header.resulting_state_root());
        let begin = header.first_sequence().max(start.facts.global_sequence);
        let last = header.last_sequence();
        let maintenance = first
            .record
            .replica_document
            .get("batch_evidence")
            .and_then(|e| e.get("batch_identity"))
            .and_then(|e| e.get("kind"))
            .and_then(Value::as_str)
            == Some("occupancy_maintenance_v2");
        let end = if maintenance {
            last.checked_sub(1).ok_or_else(refused)?
        } else {
            last
        };
        let held: BTreeSet<_> = items.iter().map(|e| e.facts.global_sequence).collect();
        if end < begin
            || end - begin + 1 != held.range(begin..=end).count() as u64
            || items
                .iter()
                .any(|e| e.header != first.header || e.header_signature != first.header_signature)
        {
            return Err(refused());
        }
    }
    Ok(())
}

#[cfg(test)]
#[path = "../tests/support/human_unit.rs"]
mod tests;
