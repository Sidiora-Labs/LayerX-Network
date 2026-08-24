#![forbid(unsafe_code)]

use std::env;
use std::fs;
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use layerx_agentd::read::LayerxdProgramBalanceReader;
use layerx_client::head::Head;
use layerx_explorer_index::programs::ExplorerProgram;
use layerx_explorer_index::{Indexer, ProtocolProgramIngestor};
use layerx_programs::{
    hex, DeploymentJournal, DeploymentRecord, JournalReadAuthority, ObservedHead, ProgramId,
    ProgramLifecycle, Registry, RegistryError,
};

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
    sequencer_id: [u8; 32],
    sequencer_public_key: [u8; 32],
    first_batch: u64,
    last_batch: u64,
    staleness_ms: u64,
    journal: FileJournal,
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
    let first_batch = parse_u64("LAYERX_EXPLORER_SEQUENCER_FIRST_BATCH")?;
    let last_batch = parse_u64("LAYERX_EXPLORER_SEQUENCER_LAST_BATCH")?;
    let staleness_ms = parse_u64("LAYERX_EXPLORER_PROGRAM_MAX_STALENESS_MS")?;
    if first_batch == 0 || last_batch < first_batch || staleness_ms == 0 {
        return Err("explorer authority range and staleness bound are non-canonical".to_owned());
    }
    Ok(Config {
        listen,
        bearer,
        node_endpoint: required("LAYERX_EXPLORER_NODE_ENDPOINT")?,
        node_bearer,
        authority_endpoint: required("LAYERX_EXPLORER_AUTHORITY_ENDPOINT")?,
        authority_bearer,
        authority_replica_id: parse_digest("LAYERX_EXPLORER_AUTHORITY_REPLICA_ID")?,
        sequencer_id: parse_digest("LAYERX_EXPLORER_SEQUENCER_ID")?,
        sequencer_public_key: parse_digest("LAYERX_EXPLORER_SEQUENCER_PUBLIC_KEY")?,
        first_batch,
        last_batch,
        staleness_ms,
        journal: FileJournal {
            root: PathBuf::from(required("LAYERX_EXPLORER_DEPLOYMENT_JOURNAL")?),
        },
        probe_program: ProgramId::new(parse_digest("LAYERX_EXPLORER_PROGRAM_PROBE_ID")?)
            .map_err(|error| format!("LAYERX_EXPLORER_PROGRAM_PROBE_ID is invalid: {error}"))?,
        observed_sealed_batch: parse_u64("LAYERX_EXPLORER_OBSERVED_SEALED_BATCH")?,
        finalised_checkpoint: parse_digest("LAYERX_EXPLORER_FINALISED_CHECKPOINT")?,
    })
}

fn load_registry(root: &Path) -> Result<Registry, String> {
    let mut paths = fs::read_dir(root)
        .map_err(|error| format!("deployment journal is unavailable: {error}"))?
        .map(|entry| entry.map(|value| value.path()))
        .collect::<Result<Vec<_>, _>>()
        .map_err(|error| format!("deployment journal is unreadable: {error}"))?;
    paths.retain(|path| path.extension().is_some_and(|value| value == "deployment"));
    paths.sort();
    let mut records = Vec::with_capacity(paths.len());
    for path in paths {
        let bytes = fs::read(&path)
            .map_err(|error| format!("{} is unreadable: {error}", path.display()))?;
        let record = DeploymentRecord::decode(&bytes)
            .map_err(|error| format!("{} is corrupt: {error}", path.display()))?;
        record
            .validate()
            .map_err(|error| format!("{} is inadmissible: {error}", path.display()))?;
        let expected = hex::encode(&record.digest());
        if path.file_stem().and_then(|value| value.to_str()) != Some(expected.as_str()) {
            return Err(format!(
                "{} is filed under the wrong digest",
                path.display()
            ));
        }
        records.push(record);
    }
    if records.is_empty() {
        return Err("deployment journal contains no canonical records".to_owned());
    }
    let mut registry = Registry::new();
    registry
        .replay_journal(&records)
        .map_err(|error| format!("deployment journal replay failed: {error}"))?;
    Ok(registry)
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

fn program_json(program: &ExplorerProgram) -> String {
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
        "{{\"program\":\"{}\",\"lifecycle\":\"{}\",\"value_accounts\":[{}],\"observed_sequence\":{},\"observed_at\":{},\"receipt_digest\":\"{}\",\"state_root\":\"{}\"}}",
        hex::encode(&program.identifier),
        lifecycle(program.lifecycle),
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
    registry: &Registry,
    ingestor: &mut ProtocolProgramIngestor,
    index: &mut Indexer,
    program: ProgramId,
    now: u64,
) -> Result<(), String> {
    let authority = JournalReadAuthority::new(&config.journal, now, config.staleness_ms)
        .map_err(|error| format!("registry authority is unavailable: {error}"))?;
    let read = registry
        .read(program, &authority)
        .map_err(|error| format!("registry read is unavailable: {error}"))?;
    ingestor
        .ingest(index, read, now)
        .map_err(|error| format!("program ingest failed: {error:?}"))?;
    Ok(())
}

fn serve_connection(
    stream: &mut TcpStream,
    config: &Config,
    registry: &Registry,
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
            registry,
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
    let now = now_ms()?;
    if refresh_program(config, registry, ingestor, index, program, now).is_err() {
        return response(stream, 503, "{\"error\":\"program_state_unavailable\"}");
    }
    match index.program(program.bytes()).value {
        Some(program) => response(stream, 200, &program_json(&program)),
        None => response(stream, 503, "{\"error\":\"program_state_unavailable\"}"),
    }
}

fn serve(config: Config) -> Result<(), String> {
    let registry = load_registry(&config.journal.root)?;
    let reader = LayerxdProgramBalanceReader::connect(
        &config.node_endpoint,
        config.node_bearer.clone(),
        &config.authority_endpoint,
        config.authority_bearer.clone(),
        config.authority_replica_id,
        config.sequencer_id,
        config.sequencer_public_key,
        config.first_batch,
        config.last_batch,
        registry.clone(),
        config.staleness_ms,
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
    let probe = registry
        .read(config.probe_program, &authority)
        .map_err(|error| format!("explorer registry probe failed: {error}"))?;
    ingestor
        .ingest(&mut index, probe, now)
        .map_err(|error| format!("explorer protocol probe failed: {error:?}"))?;
    let listener = TcpListener::bind(&config.listen)
        .map_err(|error| format!("explorer program listener failed: {error}"))?;
    for incoming in listener.incoming() {
        let mut stream = incoming.map_err(|error| format!("explorer accept failed: {error}"))?;
        stream
            .set_read_timeout(Some(Duration::from_secs(10)))
            .and_then(|()| stream.set_write_timeout(Some(Duration::from_secs(10))))
            .map_err(|error| format!("explorer connection timeout setup failed: {error}"))?;
        let _ = serve_connection(&mut stream, &config, &registry, &mut ingestor, &mut index);
    }
    Ok(())
}

fn main() {
    if let Err(error) = config().and_then(serve) {
        eprintln!("layerx-explorer-index: {error}");
        std::process::exit(2);
    }
}
