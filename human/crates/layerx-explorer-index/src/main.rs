#![forbid(unsafe_code)]

use std::env;
use std::fs;
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use layerx_agentd::read::LayerxdProgramBalanceReader;
use layerx_client::head::Head;
use layerx_explorer_index::programs::{
    ExplorerProgram, VerifiedProgramInterfaceMetadata,
};
use layerx_explorer_index::{Indexer, ProtocolProgramIngestor};
use layerx_programs::{
    hex, BuildPlan, DeploymentJournal, DeploymentProof, DeploymentRecord, JournalReadAuthority,
    ObservedHead, ProgramId, ProgramLifecycle, ProtocolDeploymentVerifier, Registry, RegistryError,
    ReproducibleBuild, SourceStatus, UpgradePolicy,
};
use serde_json::Value;

const HEADER_LIMIT: usize = 16 * 1024;

#[derive(Clone)]
struct FileJournal {
    root: PathBuf,
}

impl DeploymentJournal for FileJournal {
    fn canonical_record(&self, digest: [u8; 32]) -> Result<Vec<u8>, RegistryError> {
        fs::read(
            self.root
                .join(format!("{}.deployment", hex::encode(&digest))),
        )
        .map_err(|_| RegistryError::JournalUnavailable)
    }

    fn observed_head(&self) -> Result<ObservedHead, RegistryError> {
        let text = fs::read_to_string(self.root.join("head"))
            .map_err(|_| RegistryError::JournalUnavailable)?;
        let (sequence, observed_at) = text
            .trim()
            .split_once('\t')
            .ok_or(RegistryError::JournalUnavailable)?;
        Ok(ObservedHead {
            sequence: sequence
                .parse()
                .map_err(|_| RegistryError::JournalUnavailable)?,
            observed_at: observed_at
                .parse()
                .map_err(|_| RegistryError::JournalUnavailable)?,
        })
    }
}

struct Config {
    listen: String,
    bearer: String,
    node_endpoint: String,
    node_bearer: String,
    authority_endpoint: String,
    authority_bearer: String,
    authority_replica_id: [u8; 32],
    sequencer_trust_history: PathBuf,
    staleness_ms: u64,
    journal: FileJournal,
    verified_source_store: PathBuf,
    probe_program: ProgramId,
    observed_sealed_batch: u64,
    finalised_checkpoint: [u8; 32],
}

fn required(name: &str) -> Result<String, String> {
    env::var(name)
        .ok()
        .filter(|value| !value.is_empty())
        .ok_or_else(|| format!("{name} is required"))
}

fn parse_u64(name: &str) -> Result<u64, String> {
    required(name)?
        .parse()
        .map_err(|_| format!("{name} must be an unsigned integer"))
}

fn parse_digest(name: &str) -> Result<[u8; 32], String> {
    hex::decode_digest(&required(name)?).map_err(|error| format!("{name} is invalid: {error}"))
}

fn config() -> Result<Config, String> {
    let listen = required("LAYERX_EXPLORER_PROGRAM_LISTEN")?;
    let bearer = required("LAYERX_EXPLORER_PROGRAM_BEARER_TOKEN")?;
    let node_bearer = required("LAYERX_EXPLORER_NODE_BEARER_TOKEN")?;
    let authority_bearer = required("LAYERX_EXPLORER_AUTHORITY_BEARER_TOKEN")?;
    if !listen.starts_with("127.0.0.1:")
        || bearer.len() < 32
        || node_bearer.len() < 32
        || authority_bearer.len() < 32
        || bearer == node_bearer
        || bearer == authority_bearer
    {
        return Err("explorer program reads require loopback and distinct credentials".to_owned());
    }
    let staleness_ms = parse_u64("LAYERX_EXPLORER_PROGRAM_MAX_STALENESS_MS")?;
    if staleness_ms == 0 {
        return Err("explorer staleness bound is non-canonical".to_owned());
    }
    Ok(Config {
        listen,
        bearer,
        node_endpoint: required("LAYERX_EXPLORER_NODE_ENDPOINT")?,
        node_bearer,
        authority_endpoint: required("LAYERX_EXPLORER_AUTHORITY_ENDPOINT")?,
        authority_bearer,
        authority_replica_id: parse_digest("LAYERX_EXPLORER_AUTHORITY_REPLICA_ID")?,
        sequencer_trust_history: PathBuf::from(required(
            "LAYERX_EXPLORER_SEQUENCER_TRUST_HISTORY",
        )?),
        staleness_ms,
        journal: FileJournal {
            root: PathBuf::from(required("LAYERX_EXPLORER_DEPLOYMENT_JOURNAL")?),
        },
        verified_source_store: PathBuf::from(required(
            "LAYERX_EXPLORER_VERIFIED_SOURCE_STORE",
        )?),
        probe_program: ProgramId::new(parse_digest("LAYERX_EXPLORER_PROGRAM_PROBE_ID")?)
            .map_err(|error| format!("LAYERX_EXPLORER_PROGRAM_PROBE_ID is invalid: {error}"))?,
        observed_sealed_batch: parse_u64("LAYERX_EXPLORER_OBSERVED_SEALED_BATCH")?,
        finalised_checkpoint: parse_digest("LAYERX_EXPLORER_FINALISED_CHECKPOINT")?,
    })
}

struct LoadedRegistry {
    registry: Registry,
    interfaces: Vec<VerifiedProgramInterfaceMetadata>,
}

fn load_registry(
    root: &Path,
    verified_source_store: &Path,
    verifier: &ProtocolDeploymentVerifier,
) -> Result<LoadedRegistry, String> {
    let mut paths = fs::read_dir(root)
        .map_err(|error| format!("deployment journal is unavailable: {error}"))?
        .map(|entry| entry.map(|value| value.path()))
        .collect::<Result<Vec<_>, _>>()
        .map_err(|error| format!("deployment journal is unreadable: {error}"))?;
    paths.retain(|path| path.extension().is_some_and(|value| value == "admission"));
    paths.sort();
    let mut registry = Registry::new();
    let mut interfaces = Vec::new();
    for path in paths {
        let bytes = fs::read(&path)
            .map_err(|error| format!("{} is unreadable: {error}", path.display()))?;
        let proof = DeploymentProof::decode(&bytes)
            .map_err(|error| format!("{} is corrupt: {error}", path.display()))?;
        let evidence = verifier
            .verify_historical_deployment(&proof)
            .map_err(|error| format!("{} is unverified: {error}", path.display()))?;
        let expected = hex::encode(&evidence.receipt_digest());
        if path.file_stem().and_then(|value| value.to_str()) != Some(expected.as_str()) {
            return Err(format!("{} is filed under the wrong receipt", path.display()));
        }
        let record_path = root.join(format!("{expected}.deployment"));
        let record = DeploymentRecord::decode(
            &fs::read(&record_path)
                .map_err(|error| format!("{} is unreadable: {error}", record_path.display()))?,
        )
        .map_err(|error| format!("{} is corrupt: {error}", record_path.display()))?;
        record
            .validate()
            .map_err(|error| format!("{} is inadmissible: {error}", record_path.display()))?;
        if &record != evidence.record() {
            return Err(format!("{} disagrees with protocol evidence", record_path.display()));
        }
        if let Some(interface) = VerifiedProgramInterfaceMetadata::from_deployment(&evidence) {
            interfaces.push(interface);
        }
        registry
            .record_verified_deployment(&evidence)
            .map_err(|error| format!("verified deployment replay failed: {error}"))?;
    }
    if registry.program_ids().is_empty() {
        return Err("deployment journal contains no verified admissions".to_owned());
    }
    replay_verified_sources(verified_source_store, &mut registry)?;
    Ok(LoadedRegistry {
        registry,
        interfaces,
    })
}

fn replay_verified_sources(root: &Path, registry: &mut Registry) -> Result<(), String> {
    let mut paths = fs::read_dir(root)
        .map_err(|error| format!("verified source store is unavailable: {error}"))?
        .map(|entry| entry.map(|value| value.path()))
        .collect::<Result<Vec<_>, _>>()
        .map_err(|error| format!("verified source store is unreadable: {error}"))?;
    paths.retain(|path| path.extension().is_some_and(|value| value == "verified"));
    paths.sort();
    for path in paths {
        let bytes = fs::read(&path)
            .map_err(|error| format!("{} is unreadable: {error}", path.display()))?;
        let document: Value = serde_json::from_slice(&bytes)
            .map_err(|_| format!("{} is corrupt", path.display()))?;
        let program = document["program"]
            .as_str()
            .and_then(|value| hex::decode_digest(value).ok())
            .and_then(|value| ProgramId::new(value).ok())
            .ok_or_else(|| format!("{} has an invalid program", path.display()))?;
        let version = document["version"]
            .as_u64()
            .and_then(|value| u32::try_from(value).ok())
            .filter(|value| *value != 0)
            .ok_or_else(|| format!("{} has an invalid version", path.display()))?;
        let expected_name = format!("{}-{version}", hex::encode(&program.bytes()));
        if path.file_stem().and_then(|value| value.to_str()) != Some(expected_name.as_str()) {
            return Err(format!("{} is filed under the wrong program version", path.display()));
        }
        let source_uri = document["source_uri"]
            .as_str()
            .filter(|value| !value.is_empty())
            .ok_or_else(|| format!("{} has an invalid source URI", path.display()))?;
        let source_digest = document["source_digest"]
            .as_str()
            .and_then(|value| hex::decode_digest(value).ok())
            .ok_or_else(|| format!("{} has an invalid source digest", path.display()))?;
        let artifact_digest = document["artifact_digest"]
            .as_str()
            .and_then(|value| hex::decode_digest(value).ok())
            .ok_or_else(|| format!("{} has an invalid artifact digest", path.display()))?;
        let plan = document["plan"]
            .as_str()
            .ok_or_else(|| format!("{} has no build plan", path.display()))
            .and_then(|value| {
                BuildPlan::parse(value)
                    .map_err(|error| format!("{} has an invalid build plan: {error}", path.display()))
            })?;
        let build = ReproducibleBuild::from_record(
            source_uri.to_owned(),
            source_digest,
            plan.environment,
            artifact_digest,
        )
        .map_err(|error| format!("{} is inadmissible: {error}", path.display()))?;
        match registry.verify_source(program, version, &build) {
            Ok(SourceStatus::Verified { .. }) => {}
            Ok(SourceStatus::Mismatch { .. } | SourceStatus::Unpublished) => {
                return Err(format!("{} does not reproduce registered code", path.display()));
            }
            Err(error) => {
                return Err(format!("{} is not bound to registry state: {error}", path.display()));
            }
        }
    }
    Ok(())
}

fn now_ms() -> Result<u64, String> {
    let value = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|_| "system time precedes the Unix epoch".to_owned())?;
    u64::try_from(value.as_millis()).map_err(|_| "system time is out of range".to_owned())
}

fn lifecycle(value: ProgramLifecycle) -> &'static str {
    match value {
        ProgramLifecycle::Active => "active",
        ProgramLifecycle::Deprecated => "deprecated",
        ProgramLifecycle::Tombstoned => "tombstoned",
    }
}

fn upgrade_policy(value: UpgradePolicy) -> String {
    match value {
        UpgradePolicy::Immutable => "{\"kind\":\"immutable\"}".to_owned(),
        UpgradePolicy::Authority(authority) => format!(
            "{{\"kind\":\"upgradeable\",\"authority\":\"{}\"}}",
            hex::encode(&authority),
        ),
    }
}

fn source_status(value: &SourceStatus) -> String {
    match value {
        SourceStatus::Unpublished => "{\"status\":\"unpublished\"}".to_owned(),
        SourceStatus::Verified {
            source_digest,
            environment_digest,
        } => format!(
            "{{\"status\":\"verified\",\"source_digest\":\"{}\",\"environment_digest\":\"{}\"}}",
            hex::encode(source_digest),
            hex::encode(environment_digest),
        ),
        SourceStatus::Mismatch {
            expected,
            reproduced,
        } => format!(
            "{{\"status\":\"mismatch\",\"expected\":\"{}\",\"reproduced\":\"{}\"}}",
            hex::encode(expected),
            hex::encode(reproduced),
        ),
    }
}

fn program_json(program: &ExplorerProgram) -> String {
    let versions = program
        .versions
        .iter()
        .map(|version| {
            format!(
                "{{\"version\":\"{}\",\"code_hash\":\"{}\",\"abi_version\":\"{}\",\"interface_digest\":{},\"source\":{}}}",
                version.number,
                hex::encode(&version.code_hash),
                version.abi_version,
                version
                    .interface_digest
                    .map(|digest| format!("\"{}\"", hex::encode(&digest)))
                    .unwrap_or_else(|| "null".to_owned()),
                source_status(&version.source),
            )
        })
        .collect::<Vec<_>>()
        .join(",");
    let accounts = program
        .value_accounts
        .iter()
        .map(|account| {
            format!(
                "{{\"account\":\"{}\",\"asset\":\"{}\",\"balance\":\"{}\",\"frozen\":{}}}",
                hex::encode(&account.account),
                hex::encode(&account.asset),
                account.balance,
                account.frozen
            )
        })
        .collect::<Vec<_>>()
        .join(",");
    format!(
        "{{\"program\":\"{}\",\"upgrade_policy\":{},\"lifecycle\":\"{}\",\"versions\":[{}],\"value_accounts\":[{}],\"observed_sequence\":\"{}\",\"observed_at\":\"{}\",\"receipt_digest\":\"{}\",\"state_root\":\"{}\"}}",
        hex::encode(&program.identifier),
        upgrade_policy(program.upgrade_policy),
        lifecycle(program.lifecycle),
        versions,
        accounts,
        program.balance_observed_sequence,
        program.balance_observed_at,
        hex::encode(&program.balance_receipt_digest),
        hex::encode(&program.balance_state_root)
    )
}

fn response(stream: &mut TcpStream, status: u16, body: &str) -> Result<(), String> {
    let reason = if status < 300 { "OK" } else { "Refused" };
    let header = format!(
        "HTTP/1.1 {status} {reason}\r\nContent-Type: application/json\r\nCache-Control: no-store\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
        body.len()
    );
    stream
        .write_all(header.as_bytes())
        .and_then(|()| stream.write_all(body.as_bytes()))
        .map_err(|error| format!("explorer response failed: {error}"))
}

fn refresh_program(
    config: &Config,
    loaded: &LoadedRegistry,
    ingestor: &mut ProtocolProgramIngestor,
    index: &mut Indexer,
    program: ProgramId,
    now: u64,
) -> Result<(), String> {
    let authority = JournalReadAuthority::new(&config.journal, now, config.staleness_ms)
        .map_err(|error| format!("registry authority is unavailable: {error}"))?;
    let read = loaded
        .registry
        .read(program, &authority)
        .map_err(|error| format!("registry read is unavailable: {error}"))?;
    ingestor
        .ingest(index, read, &loaded.interfaces, now)
        .map_err(|error| format!("program ingest failed: {error:?}"))?;
    Ok(())
}

fn serve_connection(
    stream: &mut TcpStream,
    config: &Config,
    loaded: &LoadedRegistry,
    ingestor: &mut ProtocolProgramIngestor,
    index: &mut Indexer,
) -> Result<(), String> {
    let mut bytes = [0_u8; HEADER_LIMIT];
    let mut length = 0_usize;
    while length < bytes.len() && !bytes[..length].windows(4).any(|value| value == b"\r\n\r\n") {
        let count = stream
            .read(&mut bytes[length..])
            .map_err(|error| format!("explorer request failed: {error}"))?;
        if count == 0 {
            return Err("explorer request ended before its headers".to_owned());
        }
        length += count;
    }
    if !bytes[..length].windows(4).any(|value| value == b"\r\n\r\n") {
        return response(stream, 431, "{\"error\":\"headers_too_large\"}");
    }
    let request = std::str::from_utf8(&bytes[..length])
        .map_err(|_| "explorer request headers are not UTF-8".to_owned())?;
    let line = request
        .lines()
        .next()
        .ok_or_else(|| "explorer request omitted its request line".to_owned())?;
    let mut parts = line.split_ascii_whitespace();
    if parts.next() != Some("GET") {
        return response(stream, 400, "{\"error\":\"invalid_request\"}");
    }
    let path = parts.next().unwrap_or_default();
    if parts.next() != Some("HTTP/1.1") || parts.next().is_some() {
        return response(stream, 400, "{\"error\":\"invalid_request\"}");
    }
    if !request
        .lines()
        .any(|header| header.strip_prefix("Authorization: Bearer ") == Some(config.bearer.as_str()))
    {
        return response(stream, 401, "{\"error\":\"unauthorized\"}");
    }
    if path == "/healthz" {
        return match refresh_program(
            config,
            loaded,
            ingestor,
            index,
            config.probe_program,
            now_ms()?,
        ) {
            Ok(()) => response(stream, 200, "{\"ready\":true}"),
            Err(_) => response(stream, 503, "{\"ready\":false}"),
        };
    }
    let Some(program_text) = path.strip_prefix("/v1/programs/") else {
        return response(stream, 404, "{\"error\":\"not_found\"}");
    };
    let program = hex::decode_digest(program_text)
        .ok()
        .and_then(|bytes| ProgramId::new(bytes).ok());
    let Some(program) = program else {
        return response(stream, 400, "{\"error\":\"invalid_program\"}");
    };
    if !loaded.registry.program_ids().contains(&program) {
        return response(stream, 404, "{\"error\":\"not_found\"}");
    }
    let now = now_ms()?;
    if refresh_program(config, loaded, ingestor, index, program, now).is_err() {
        return response(stream, 503, "{\"error\":\"program_state_unavailable\"}");
    }
    match index.program(program.bytes()).value {
        Some(program) => response(stream, 200, &program_json(&program)),
        None => response(stream, 503, "{\"error\":\"program_state_unavailable\"}"),
    }
}

fn serve(config: Config) -> Result<(), String> {
    let verifier = ProtocolDeploymentVerifier::from_protected_history(
        &config.sequencer_trust_history,
        config.staleness_ms,
    )
    .map_err(|error| format!("explorer deployment verifier is invalid: {error}"))?;
    let loaded = load_registry(
        &config.journal.root,
        &config.verified_source_store,
        &verifier,
    )?;
    let reader = LayerxdProgramBalanceReader::connect(
        &config.node_endpoint,
        config.node_bearer.clone(),
        &config.authority_endpoint,
        config.authority_bearer.clone(),
        config.authority_replica_id,
        verifier,
        loaded.registry.clone(),
    )
    .map_err(|error| format!("explorer protocol reader configuration failed: {error:?}"))?;
    let head = config
        .journal
        .observed_head()
        .map_err(|error| format!("explorer head is unavailable: {error}"))?;
    let mut index = Indexer::new(Head {
        chain_sequence: head.sequence,
        sealed_batch: config.observed_sealed_batch,
        finalised_checkpoint: config.finalised_checkpoint,
    });
    let mut ingestor = ProtocolProgramIngestor::new(reader);
    let now = now_ms()?;
    let authority = JournalReadAuthority::new(&config.journal, now, config.staleness_ms)
        .map_err(|error| format!("explorer registry authority is unavailable: {error}"))?;
    let probe = loaded
        .registry
        .read(config.probe_program, &authority)
        .map_err(|error| format!("explorer registry probe failed: {error}"))?;
    ingestor
        .ingest(&mut index, probe, &loaded.interfaces, now)
        .map_err(|error| format!("explorer protocol probe failed: {error:?}"))?;
    let listener = TcpListener::bind(&config.listen)
        .map_err(|error| format!("explorer program listener failed: {error}"))?;
    for incoming in listener.incoming() {
        let mut stream = incoming.map_err(|error| format!("explorer accept failed: {error}"))?;
        stream
            .set_read_timeout(Some(Duration::from_secs(10)))
            .and_then(|()| stream.set_write_timeout(Some(Duration::from_secs(10))))
            .map_err(|error| format!("explorer connection timeout setup failed: {error}"))?;
        let _ = serve_connection(&mut stream, &config, &loaded, &mut ingestor, &mut index);
    }
    Ok(())
}

fn main() {
    if let Err(error) = config().and_then(serve) {
        eprintln!("layerx-explorer-index: {error}");
        std::process::exit(2);
    }
}
