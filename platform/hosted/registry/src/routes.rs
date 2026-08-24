//! The registry routes the developer CLI calls.
//!
//! Reading a program returns the registry projection only after the deployment
//! receipt behind its latest version has been re-verified against the
//! canonical journal. Verifying a program's source rebuilds mirrored source in
//! the pinned toolchain environment and compares the rebuilt artifact with the
//! registered on-chain code hash, so a mismatch is reported as a mismatch and
//! never as a verified source.

use std::collections::{BTreeMap, BTreeSet, HashMap};

use layerx_programs::{
    hex, programs_source_verification, BuildPlan, BuildRefusal, DeploymentRecord,
    JournalReadAuthority, LifecycleReceipt, ObservedHead, ProgramId, ProgramLifecycle, Registry,
    RegistryError, RegistryVersion, ReproducibleBuild, SourceArchive, SourceStatus, SourceVerifier,
    UpgradePolicy, VerifiedProgramBalanceRead, VerifiedRegistryRead, WindDownStateAccess,
};
use layerx_programs_protocol_adapter::ProtocolProgramStateRead;
use serde_json::{json, Value};
use sha2::{Digest as _, Sha256};

use crate::builder::HermeticBuilder;
use crate::journal::FileDeploymentJournal;
use crate::mirror::{MirrorRefusal, SourceMirror};
use crate::node_state::NodeProgramStateSource;
use crate::program_state::FileProgramStateJournal;
use crate::verified::{VerifiedSource, VerifiedSourceStore};
use crate::Config;

const IDEMPOTENCY_DOMAIN: &[u8] = b"LayerX/platform/registry/idempotency/v1\0";
const REQUEST_DOMAIN: &[u8] = b"LayerX/platform/registry/source-request/v1\0";
const MAX_IDEMPOTENCY_RECORDS: usize = 4_096;
const MAX_CHANGE_PAGES: usize = 1_024;
const ROUTE_PREFIX: &str = "/v1/programs/registry/";

/// One parsed request.
#[derive(Clone, Debug)]
pub struct Request {
    pub method: String,
    pub path: String,
    pub headers: HashMap<String, String>,
    pub body: Vec<u8>,
}

/// One rendered response.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Response {
    pub status: u16,
    pub body: String,
}

#[derive(Clone, Debug)]
struct Completed {
    request_digest: [u8; 32],
    status: u16,
    body: String,
    at: u64,
}

/// The hosted registry. It owns the durable evidence the routes answer from
/// and never answers a read from its own projection alone.
pub struct Registrar {
    registry: Registry,
    journal: FileDeploymentJournal,
    program_state: FileProgramStateJournal,
    node_state: NodeProgramStateSource,
    mirror: SourceMirror,
    verified: VerifiedSourceStore,
    verifier: SourceVerifier<HermeticBuilder>,
    staleness_seconds: u64,
    balance_reads: BTreeMap<ProgramId, VerifiedProgramBalanceRead>,
    idempotency: BTreeMap<String, Completed>,
}

impl Registrar {
    /// Opens every durable store and rebuilds the registry projection from the
    /// canonical journal and the completed rebuilds recorded beside it.
    ///
    /// # Errors
    ///
    /// Returns unusable directories, a corrupt journal, an inadmissible
    /// declared build environment and stored verifications that no longer
    /// decode.
    pub fn open(config: &Config, now: u64) -> Result<Self, String> {
        let builder = HermeticBuilder::new(
            config.workspace.clone(),
            config.builder_image_digest,
            config.builder_path.clone(),
            config.build_timeout_seconds,
        )?;
        let verifier = SourceVerifier::new(builder, config.attempts)
            .map_err(|refused| format!("the build pipeline is not admissible: {refused}"))?;
        if config.staleness_seconds == 0 {
            return Err("a registry read freshness bound is required".to_owned());
        }
        let mut registrar = Self {
            registry: Registry::new(),
            journal: FileDeploymentJournal::open(config.journal.clone())?,
            program_state: FileProgramStateJournal::open(config.journal.join("program-state"))?,
            node_state: NodeProgramStateSource::connect(
                &config.node_endpoint,
                config.node_authorization.clone(),
                &config.receipt_authority_endpoint,
                config.receipt_authority_authorization.clone(),
                config.receipt_authority_replica_id,
                config.sequencer_id,
                config.sequencer_public_key,
                config.sequencer_first_batch,
                config.sequencer_last_batch,
            )?,
            mirror: SourceMirror::open(config.mirror.clone())?,
            verified: VerifiedSourceStore::open(config.verified.clone())?,
            verifier,
            staleness_seconds: config.staleness_seconds,
            balance_reads: BTreeMap::new(),
            idempotency: BTreeMap::new(),
        };
        registrar.rebuild()?;
        registrar.synchronize_protocol_state(None, now)?;
        Ok(registrar)
    }

    /// Answers one request at the supplied wall-clock second.
    pub fn route(&mut self, request: &Request, now: u64) -> Response {
        match (request.method.as_str(), request.path.as_str()) {
            ("GET", "/healthz") => Response {
                status: 200,
                body: json!({"status": "ready", "service": "program-registry"}).to_string(),
            },
            ("POST", "/__registry/deployments") => self.ingest_deployment(&request.body),
            ("POST", "/__registry/head") => self.ingest_head(&request.body),
            ("POST", "/__registry/sources") => self.ingest_source(&request.body),
            (
                _,
                "/healthz" | "/__registry/deployments" | "/__registry/head" | "/__registry/sources",
            ) => refusal(
                405,
                "method_not_allowed",
                "method is not supported for this route",
            ),
            _ => self.program_route(request, now),
        }
    }

    /// Reconciles the hosted cache with the authenticated state and receipt
    /// authority owned by the production node that commits activities. The
    /// cursor is advanced only after every affected program has been resolved
    /// at the same current head, independently receipt-checked, replayed and
    /// persisted.
    pub fn synchronize_protocol_state(
        &mut self,
        requested: Option<ProgramId>,
        now: u64,
    ) -> Result<(), String> {
        if now == 0 {
            return Err("program-state synchronization requires an observed time".to_owned());
        }
        let prior_cursor = self.program_state.cursor()?;
        let mut complete = prior_cursor;
        let mut programs = BTreeSet::new();
        let mut caught_up = false;
        let mut feed_head = 0_u64;
        for _ in 0..MAX_CHANGE_PAGES {
            let (notices, next, scanned, current) = self.node_state.changes(complete)?;
            programs.extend(notices.into_iter().map(|notice| notice.program));
            if next == complete && !current {
                return Err("node program-state change feed made no progress".to_owned());
            }
            complete = next;
            if scanned < feed_head {
                return Err("node program-state scan head regressed".to_owned());
            }
            feed_head = scanned;
            if current {
                caught_up = true;
                break;
            }
        }
        if !caught_up {
            return Err("node program-state change feed exceeded its page bound".to_owned());
        }
        if prior_cursor.sequence == 0 || self.balance_reads.is_empty() {
            programs.extend(self.registry.program_ids());
        }
        if let Some(program) = requested {
            programs.insert(program);
        }
        let current_head = self.node_state.current_head()?;
        if complete.sequence > feed_head || feed_head > current_head.freshness.observed_sequence {
            return Err(
                "program-state feed is ahead of the independently verified head".to_owned(),
            );
        }
        let mut registry = self.registry.clone();
        let mut staged = Vec::with_capacity(programs.len());
        for program in programs {
            let entry = registry
                .entry_for_wind_down(program)
                .map_err(|error| format!("program-state registry lookup refused: {error}"))?;
            let abi = entry.versions.last().map(|version| version.abi_version);
            if abi == Some(1) && entry.value_accounts.is_empty() {
                continue;
            }
            if abi != Some(2) {
                return Err("program value accounts require the frozen ABI-two protocol".to_owned());
            }
            let record = self.node_state.program_state(program, current_head)?;
            let state = ProtocolProgramStateRead::restore_verified(
                &record.bytes,
                &mut registry,
                record.receipt,
                current_head,
                now,
                self.staleness_seconds,
            )
            .map_err(|error| format!("protocol program-state adapter refused: {error:?}"))?;
            if state.program() != program {
                return Err("node program-state record changed program identity".to_owned());
            }
            staged.push(state);
        }

        for state in &staged {
            self.program_state.store(state)?;
        }
        self.program_state.advance(complete)?;
        let mut balance_reads = self.balance_reads.clone();
        for state in staged {
            let balances = state.into_balances();
            balance_reads.insert(balances.program(), balances);
        }
        self.registry = registry;
        self.balance_reads = balance_reads;
        Ok(())
    }

    fn program_route(&mut self, request: &Request, now: u64) -> Response {
        let Some(rest) = request.path.strip_prefix(ROUTE_PREFIX) else {
            return refusal(404, "not_found", "route does not exist");
        };
        match rest.split_once('/') {
            None if request.method == "GET" => self.read(rest, now),
            Some((program, "source")) if request.method == "POST" => {
                self.verify(program, request, now)
            }
            None | Some((_, "source")) => refusal(
                405,
                "method_not_allowed",
                "method is not supported for this route",
            ),
            Some(_) => refusal(404, "not_found", "route does not exist"),
        }
    }

    fn read(&mut self, program: &str, now: u64) -> Response {
        let Some(program) = program_id(program) else {
            return refusal(
                400,
                "invalid_argument",
                "program id must be thirty-two hexadecimal-encoded bytes",
            );
        };
        if self.registry.latest_version(program).is_err() {
            return refusal(404, "not_found", "program is not registered");
        }
        if let Err(error) = self.synchronize_protocol_state(Some(program), now) {
            return refusal(503, "protocol_state_unavailable", &error);
        }
        let authority = match JournalReadAuthority::new(&self.journal, now, self.staleness_seconds)
        {
            Ok(authority) => authority,
            Err(error) => return refusal(503, "read_unverifiable", &error.to_string()),
        };
        match self.registry.read(program, &authority) {
            Ok(read) => self.render_read(&read, now),
            Err(RegistryError::UnknownProgram | RegistryError::UnknownVersion) => {
                refusal(404, "not_found", "program is not registered")
            }
            Err(error @ RegistryError::StaleRead) => refusal(503, "stale_read", &error.to_string()),
            Err(error) => refusal(502, "unverified_read", &error.to_string()),
        }
    }

    fn render_read(&self, read: &VerifiedRegistryRead, now: u64) -> Response {
        let abi = read
            .entry
            .versions
            .last()
            .map(|version| version.abi_version);
        if abi == Some(1) && read.entry.value_accounts.is_empty() {
            return Response {
                status: 200,
                body: registry_read_json(read, None).to_string(),
            };
        }
        if abi != Some(2) {
            return refusal(
                502,
                "balance_protocol_unsupported",
                "program value accounts require the frozen ABI-two account protocol",
            );
        }
        let Some(balances) = self.balance_reads.get(&read.entry.program) else {
            return refusal(
                503,
                "balance_read_unavailable",
                "a current receipt-proven program balance read is not available",
            );
        };
        let freshness = balances.freshness();
        if now < freshness.observed_at
            || now.saturating_sub(freshness.observed_at) > self.staleness_seconds
            || freshness.observed_sequence < read.freshness.observed_sequence
        {
            return refusal(
                503,
                "stale_balance_read",
                "the program balance proof is not current at the observed registry head",
            );
        }
        let bindings_match = balances.bindings().len() == read.entry.value_accounts.len()
            && read.entry.value_accounts.iter().all(|binding| {
                balances
                    .bindings()
                    .iter()
                    .any(|candidate| candidate == binding)
            });
        if balances.program() != read.entry.program
            || balances.lifecycle() != read.entry.lifecycle
            || !bindings_match
        {
            return refusal(
                502,
                "balance_registry_mismatch",
                "the current balance proof does not match the receipt-verified registry record",
            );
        }
        Response {
            status: 200,
            body: registry_read_json(read, Some(balances)).to_string(),
        }
    }

    fn verify(&mut self, program: &str, request: &Request, now: u64) -> Response {
        let Some(program) = program_id(program) else {
            return refusal(
                400,
                "invalid_argument",
                "program id must be thirty-two hexadecimal-encoded bytes",
            );
        };
        let Some(key) = request.headers.get("idempotency-key") else {
            return refusal(
                400,
                "idempotency_key_required",
                "source verification requires an Idempotency-Key header",
            );
        };
        if !valid_idempotency_key(key) {
            return refusal(
                400,
                "invalid_argument",
                "idempotency key must be 16-128 ASCII letters, digits, dashes, or underscores",
            );
        }
        let Some((source_uri, source_digest)) = source_request(&request.body) else {
            return refusal(
                400,
                "invalid_argument",
                "request must carry source_uri and a thirty-two byte hexadecimal source_digest",
            );
        };
        let scoped = scoped_key(program, key);
        let digest = request_digest(program, &source_uri, &source_digest);
        if let Some(record) = self.idempotency.get(&scoped) {
            if record.request_digest != digest {
                return refusal(
                    409,
                    "idempotency_conflict",
                    "idempotency key was already used for a different request",
                );
            }
            return Response {
                status: record.status,
                body: record.body.clone(),
            };
        }
        let response = self.reproduce(program, &source_uri, source_digest);
        if response.status != 503 {
            self.remember(scoped, digest, &response, now);
        }
        response
    }

    fn reproduce(&mut self, program: ProgramId, uri: &str, source_digest: [u8; 32]) -> Response {
        let version = match self.registry.latest_version(program) {
            Ok(version) => version,
            Err(error) => return refusal(404, "not_found", &error.to_string()),
        };
        let mirrored = match self.mirror.fetch(uri, source_digest) {
            Ok(mirrored) => mirrored,
            Err(refused @ MirrorRefusal::NotMirrored) => {
                return refusal(404, "source_not_mirrored", &refused.to_string())
            }
            Err(refused) => return refusal(422, "source_unverifiable", &refused.to_string()),
        };
        let build = match self.verifier.reproduce(&mirrored.source, &mirrored.plan) {
            Ok(build) => build,
            Err(refused) => return rebuild_refusal(&refused),
        };
        let status = match self.registry.verify_source(program, version, &build) {
            Ok(status) => status,
            Err(error) => return refusal(404, "not_found", &error.to_string()),
        };
        if let Err(error) = self.verified.record(&VerifiedSource {
            program,
            version,
            source_uri: build.source_uri.clone(),
            source_digest: build.source_digest,
            artifact_digest: build.artifact_digest,
            plan: mirrored.plan.clone(),
        }) {
            return refusal(503, "persistence_unavailable", &error);
        }
        verification_response(program, version, &build, status)
    }

    fn remember(
        &mut self,
        scoped: String,
        request_digest: [u8; 32],
        response: &Response,
        now: u64,
    ) {
        if self.idempotency.len() >= MAX_IDEMPOTENCY_RECORDS {
            let oldest = self
                .idempotency
                .iter()
                .min_by_key(|(_, record)| record.at)
                .map(|(key, _)| key.clone());
            if let Some(key) = oldest {
                self.idempotency.remove(&key);
            }
        }
        self.idempotency.insert(
            scoped,
            Completed {
                request_digest,
                status: response.status,
                body: response.body.clone(),
                at: now,
            },
        );
    }

    fn rebuild(&mut self) -> Result<(), String> {
        let mut registry = Registry::new();
        registry
            .replay_journal(&self.journal.records()?)
            .map_err(|error| {
                format!("the canonical deployment journal is not replayable: {error}")
            })?;
        for record in self.verified.records()? {
            let build = ReproducibleBuild::from_record(
                record.source_uri.clone(),
                record.source_digest,
                record.plan.environment.clone(),
                record.artifact_digest,
            )
            .map_err(|error| format!("a stored verification is not admissible: {error}"))?;
            match registry.verify_source(record.program, record.version, &build) {
                Ok(_) | Err(RegistryError::UnknownProgram | RegistryError::UnknownVersion) => {}
                Err(error) => {
                    return Err(format!("a stored verification is not replayable: {error}"))
                }
            }
        }
        self.program_state.audit()?;
        self.registry = registry;
        self.balance_reads.clear();
        Ok(())
    }

    fn ingest_deployment(&mut self, body: &[u8]) -> Response {
        let Some(bytes) = field(body, "record_hex").and_then(|text| hex::decode(&text).ok()) else {
            return refusal(
                400,
                "invalid_argument",
                "request must carry the canonical record as record_hex",
            );
        };
        let record = match DeploymentRecord::decode(&bytes) {
            Ok(record) => record,
            Err(error) => return refusal(400, "invalid_argument", &error.to_string()),
        };
        let digest = match self.journal.append(&record) {
            Ok(digest) => digest,
            Err(error) => return refusal(503, "persistence_unavailable", &error),
        };
        if let Err(error) = self.rebuild() {
            let mut rollback = self.journal.discard(digest).err();
            if rollback.is_none() {
                rollback = self.rebuild().err();
            }
            let detail = rollback.map_or_else(
                || format!("{error}; the journal entry was rolled back"),
                |failure| format!("{error}; the journal entry could not be rolled back: {failure}"),
            );
            return refusal(409, "registry_conflict", &detail);
        }
        Response {
            status: 200,
            body: json!({
                "recorded": true,
                "program_id": hex::encode(&record.program.bytes()),
                "version": record.version,
                "deployment_receipt_digest": hex::encode(&digest),
            })
            .to_string(),
        }
    }

    fn ingest_head(&mut self, body: &[u8]) -> Response {
        let Ok(document) = serde_json::from_slice::<Value>(body) else {
            return refusal(400, "invalid_argument", "request body is not JSON");
        };
        let (Some(sequence), Some(observed_at)) = (
            document["sequence"].as_u64(),
            document["observed_at"].as_u64(),
        ) else {
            return refusal(
                400,
                "invalid_argument",
                "request must carry sequence and observed_at",
            );
        };
        let head = ObservedHead {
            sequence,
            observed_at,
        };
        match self.journal.refresh_head(head) {
            Ok(()) => Response {
                status: 200,
                body: json!({"observed": true, "sequence": sequence, "observed_at": observed_at})
                    .to_string(),
            },
            Err(error) => refusal(400, "invalid_argument", &error),
        }
    }

    fn ingest_source(&mut self, body: &[u8]) -> Response {
        let Ok(document) = serde_json::from_slice::<Value>(body) else {
            return refusal(400, "invalid_argument", "request body is not JSON");
        };
        let (Some(uri), Some(plan), Some(archive)) = (
            document["source_uri"].as_str(),
            document["plan"].as_str(),
            document["archive_hex"]
                .as_str()
                .and_then(|text| hex::decode(text).ok()),
        ) else {
            return refusal(
                400,
                "invalid_argument",
                "request must carry source_uri, plan and archive_hex",
            );
        };
        let archive = match SourceArchive::decode(&archive) {
            Ok(archive) => archive,
            Err(error) => return refusal(400, "invalid_argument", &error.to_string()),
        };
        let plan = match BuildPlan::parse(plan) {
            Ok(plan) => plan,
            Err(error) => return refusal(400, "invalid_argument", &error.to_string()),
        };
        match self.mirror.publish(uri, &plan, &archive) {
            Ok(digest) => Response {
                status: 200,
                body: json!({
                    "mirrored": true,
                    "source_uri": uri,
                    "source_digest": hex::encode(&digest),
                })
                .to_string(),
            },
            Err(error) => refusal(400, "invalid_argument", &error),
        }
    }
}

/// Renders one refusal in the platform's refusal envelope.
#[must_use]
pub fn refusal(status: u16, code: &str, detail: &str) -> Response {
    Response {
        status,
        body: json!({"error": {"code": code, "retry": "never", "detail": detail}}).to_string(),
    }
}

fn rebuild_refusal(refused: &BuildRefusal) -> Response {
    match refused {
        BuildRefusal::SandboxUnavailable { reason } => refusal(503, "builder_unavailable", reason),
        BuildRefusal::BuilderFailed { reason } => refusal(422, "build_failed", reason),
        BuildRefusal::NondeterministicBuild { .. } => {
            refusal(422, "build_not_reproducible", &refused.to_string())
        }
        _ => refusal(422, "source_unverifiable", &refused.to_string()),
    }
}

fn verification_response(
    program: ProgramId,
    version: u32,
    build: &ReproducibleBuild,
    status: SourceStatus,
) -> Response {
    let verified = matches!(status, SourceStatus::Verified { .. });
    let outcome = json!({
        "program_id": hex::encode(&program.bytes()),
        "version": version,
        "source_uri": build.source_uri,
        "source_digest": hex::encode(&build.source_digest),
        "environment_digest": hex::encode(&build.environment_digest),
        "reproduced_artifact_digest": hex::encode(&build.artifact_digest),
        "source": source_json(status),
        "pipeline": programs_source_verification(),
    });
    if verified {
        return Response {
            status: 200,
            body: outcome.to_string(),
        };
    }
    Response {
        status: 409,
        body: json!({
            "error": {
                "code": "source_mismatch",
                "retry": "never",
                "detail": "the rebuilt artifact does not hash to the registered code hash",
            },
            "verification": outcome,
        })
        .to_string(),
    }
}

fn registry_read_json(
    read: &VerifiedRegistryRead,
    balances: Option<&VerifiedProgramBalanceRead>,
) -> Value {
    let lifecycle = balances.map_or(read.entry.lifecycle, VerifiedProgramBalanceRead::lifecycle);
    let value_accounts = balances.map_or_else(
        || {
            json!({
                "status": "account-incapable-abi1",
                "accounts": [],
            })
        },
        |balances| {
            json!({
                "status": "current",
                "lifecycle": lifecycle_name(balances.lifecycle()),
                "accounts": balances.value_accounts().iter().map(|account| json!({
                    "account_id": hex::encode(&account.account_id),
                    "asset_id": hex::encode(&account.asset_id),
                    "balance": account.balance.to_string(),
                    "frozen": account.frozen,
                })).collect::<Vec<Value>>(),
                "receipt": {
                    "receipt_digest": hex::encode(&balances.receipt_digest()),
                    "state_root": hex::encode(&balances.state_root()),
                    "observed_sequence": balances.freshness().observed_sequence,
                    "observed_at": balances.freshness().observed_at,
                    "verification": "account-primary-and-state-proof-verified",
                },
            })
        },
    );
    json!({
        "program_id": hex::encode(&read.entry.program.bytes()),
        "upgrade_policy": policy_json(read.entry.upgrade_policy),
        "lifecycle": lifecycle_name(lifecycle),
        "latest_version": read.entry.versions.last().map(|version| version.number),
        "versions": read
            .entry
            .versions
            .iter()
            .map(version_json)
            .collect::<Vec<Value>>(),
        "lifecycle_history": read
            .entry
            .lifecycle_history
            .iter()
            .map(lifecycle_json)
            .collect::<Vec<Value>>(),
        "exit_routes": read
            .entry
            .exit_routes
            .iter()
            .map(|route| json!({
                "seed_hex": hex::encode(&route.seed),
                "account_id": hex::encode(&route.account_id),
                "asset_id": hex::encode(&route.asset_id),
                "destination": hex::encode(&route.destination),
            }))
            .collect::<Vec<Value>>(),
        "value_accounts": value_accounts,
        "receipt": {
            "deployment_receipt_digest": hex::encode(&read.receipt_digest),
            "observed_sequence": read.freshness.observed_sequence,
            "observed_at": read.freshness.observed_at,
            "verification": "receipt-verified",
        },
    })
}

fn version_json(version: &RegistryVersion) -> Value {
    json!({
        "version": version.number,
        "code_hash": hex::encode(&version.code_hash),
        "abi_version": version.abi_version,
        "deployment_receipt_digest": hex::encode(&version.deployment_receipt_digest),
        "source": source_json(version.source),
    })
}

fn source_json(status: SourceStatus) -> Value {
    match status {
        SourceStatus::Unpublished => json!({"status": "unpublished"}),
        SourceStatus::Verified {
            source_digest,
            environment_digest,
        } => json!({
            "status": "verified",
            "source_digest": hex::encode(&source_digest),
            "environment_digest": hex::encode(&environment_digest),
            "pipeline": programs_source_verification(),
        }),
        SourceStatus::Mismatch {
            expected,
            reproduced,
        } => json!({
            "status": "mismatch",
            "expected_code_hash": hex::encode(&expected),
            "reproduced_artifact_digest": hex::encode(&reproduced),
        }),
    }
}

fn lifecycle_json(receipt: &LifecycleReceipt) -> Value {
    let state_access = match receipt.wind_down.state_access {
        WindDownStateAccess::ReadOnly => "read-only",
    };
    json!({
        "prior": lifecycle_name(receipt.prior),
        "current": lifecycle_name(receipt.current),
        "authority": hex::encode(&receipt.authority),
        "effective_sequence": receipt.effective_sequence,
        "live_value_accounts": receipt.live_value_accounts,
        "wind_down": {
            "exit_program": hex::encode(&receipt.wind_down.exit_program),
            "deadline": receipt.wind_down.deadline,
            "state_access": state_access,
        },
    })
}

fn policy_json(policy: UpgradePolicy) -> Value {
    match policy {
        UpgradePolicy::Immutable => json!({"kind": "immutable"}),
        UpgradePolicy::Authority(authority) => {
            json!({"kind": "upgradeable", "authority": hex::encode(&authority)})
        }
    }
}

const fn lifecycle_name(lifecycle: ProgramLifecycle) -> &'static str {
    match lifecycle {
        ProgramLifecycle::Active => "active",
        ProgramLifecycle::Deprecated => "deprecated",
        ProgramLifecycle::Tombstoned => "tombstoned",
    }
}

fn program_id(text: &str) -> Option<ProgramId> {
    hex::decode_digest(text)
        .ok()
        .and_then(|bytes| ProgramId::new(bytes).ok())
}

fn source_request(body: &[u8]) -> Option<(String, [u8; 32])> {
    let document: Value = serde_json::from_slice(body).ok()?;
    let uri = document["source_uri"].as_str()?.to_owned();
    let digest = hex::decode_digest(document["source_digest"].as_str()?).ok()?;
    Some((uri, digest))
}

fn field(body: &[u8], name: &str) -> Option<String> {
    let document: Value = serde_json::from_slice(body).ok()?;
    Some(document[name].as_str()?.to_owned())
}

fn valid_idempotency_key(value: &str) -> bool {
    (16..=128).contains(&value.len())
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_'))
}

fn scoped_key(program: ProgramId, key: &str) -> String {
    let digest: [u8; 32] =
        Sha256::digest([IDEMPOTENCY_DOMAIN, &program.bytes(), b"\0", key.as_bytes()].concat())
            .into();
    hex::encode(&digest)
}

fn request_digest(program: ProgramId, uri: &str, source_digest: &[u8; 32]) -> [u8; 32] {
    Sha256::digest(
        [
            REQUEST_DOMAIN,
            &program.bytes(),
            b"\0",
            uri.as_bytes(),
            b"\0",
            source_digest,
        ]
        .concat(),
    )
    .into()
}
