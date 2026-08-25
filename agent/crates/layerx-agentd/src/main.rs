#![forbid(unsafe_code)]

use std::env;
use std::fs;
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::path::Path;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use layerx_agentd::read::{
    LayerxdProgramBalanceReader, ProgramBalanceRead, ProgramBalanceReadRoute,
};
use layerx_programs::{
    hex, DeploymentProof, DeploymentRecord, ProgramId, ProgramLifecycle,
    ProtocolDeploymentVerifier, Registry,
};

const HEADER_LIMIT: usize = 16 * 1024;

struct Config {
    listen: String,
    bearer: String,
    node_endpoint: String,
    node_bearer: String,
    authority_endpoint: String,
    authority_bearer: String,
    authority_replica_id: [u8; 32],
    protocol_version: u16,
    network_id: u32,
    epoch: u64,
    sequencer_id: [u8; 32],
    sequencer_public_key: [u8; 32],
    first_batch: u64,
    last_batch: u64,
    revoked_from_batch: Option<u64>,
    staleness_ms: u64,
    deployment_journal: String,
    probe_program: ProgramId,
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

fn parse_optional_u64(name: &str) -> Result<Option<u64>, String> {
    env::var(name).map_or(Ok(None), |value| {
        value
            .parse()
            .map(Some)
            .map_err(|_| format!("{name} must be an unsigned integer"))
    })
}

fn parse_digest(name: &str) -> Result<[u8; 32], String> {
    hex::decode_digest(&required(name)?).map_err(|error| format!("{name} is invalid: {error}"))
}

fn config() -> Result<Config, String> {
    let listen = required("LAYERX_AGENT_PROGRAM_LISTEN")?;
    let bearer = required("LAYERX_AGENT_PROGRAM_BEARER_TOKEN")?;
    let node_bearer = required("LAYERX_AGENT_NODE_BEARER_TOKEN")?;
    let authority_bearer = required("LAYERX_AGENT_AUTHORITY_BEARER_TOKEN")?;
    if !listen.starts_with("127.0.0.1:")
        || bearer.len() < 32
        || node_bearer.len() < 32
        || authority_bearer.len() < 32
        || bearer == node_bearer
        || bearer == authority_bearer
    {
        return Err(
            "agent program reads require loopback and distinct bounded credentials".to_owned(),
        );
    }
    let first_batch = parse_u64("LAYERX_AGENT_SEQUENCER_FIRST_BATCH")?;
    let last_batch = parse_u64("LAYERX_AGENT_SEQUENCER_LAST_BATCH")?;
    let protocol_version = u16::try_from(parse_u64("LAYERX_AGENT_PROTOCOL_VERSION")?)
        .map_err(|_| "LAYERX_AGENT_PROTOCOL_VERSION is out of range".to_owned())?;
    let network_id = u32::try_from(parse_u64("LAYERX_AGENT_NETWORK_ID")?)
        .map_err(|_| "LAYERX_AGENT_NETWORK_ID is out of range".to_owned())?;
    let epoch = parse_u64("LAYERX_AGENT_EPOCH")?;
    let revoked_from_batch = parse_optional_u64("LAYERX_AGENT_SEQUENCER_REVOKED_FROM_BATCH")?;
    let staleness_ms = parse_u64("LAYERX_AGENT_PROGRAM_MAX_STALENESS_MS")?;
    if !matches!(protocol_version, 1 | 2)
        || network_id == 0
        || epoch == 0
        || first_batch == 0
        || last_batch < first_batch
        || revoked_from_batch.is_some_and(|batch| batch == 0 || batch <= first_batch)
        || staleness_ms == 0
    {
        return Err("agent authority range and staleness bound are non-canonical".to_owned());
    }
    Ok(Config {
        listen,
        bearer,
        node_endpoint: required("LAYERX_AGENT_NODE_ENDPOINT")?,
        node_bearer,
        authority_endpoint: required("LAYERX_AGENT_AUTHORITY_ENDPOINT")?,
        authority_bearer,
        authority_replica_id: parse_digest("LAYERX_AGENT_AUTHORITY_REPLICA_ID")?,
        protocol_version,
        network_id,
        epoch,
        sequencer_id: parse_digest("LAYERX_AGENT_SEQUENCER_ID")?,
        sequencer_public_key: parse_digest("LAYERX_AGENT_SEQUENCER_PUBLIC_KEY")?,
        first_batch,
        last_batch,
        revoked_from_batch,
        staleness_ms,
        deployment_journal: required("LAYERX_AGENT_DEPLOYMENT_JOURNAL")?,
        probe_program: ProgramId::new(parse_digest("LAYERX_AGENT_PROGRAM_PROBE_ID")?)
            .map_err(|error| format!("LAYERX_AGENT_PROGRAM_PROBE_ID is invalid: {error}"))?,
    })
}

fn load_registry(
    root: &Path,
    verifier: &ProtocolDeploymentVerifier,
) -> Result<Registry, String> {
    let mut paths = fs::read_dir(root)
        .map_err(|error| format!("deployment journal is unavailable: {error}"))?
        .map(|entry| entry.map(|value| value.path()))
        .collect::<Result<Vec<_>, _>>()
        .map_err(|error| format!("deployment journal is unreadable: {error}"))?;
    paths.retain(|path| path.extension().is_some_and(|value| value == "admission"));
    paths.sort();
    let mut registry = Registry::new();
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
        registry
            .record_verified_deployment(&evidence)
            .map_err(|error| format!("verified deployment replay failed: {error}"))?;
    }
    if registry.program_ids().is_empty() {
        return Err("deployment journal contains no verified admissions".to_owned());
    }
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

fn balance_json(read: &ProgramBalanceRead) -> String {
    let accounts = read
        .accounts
        .iter()
        .map(|account| {
            format!(
                "{{\"account\":\"{}\",\"asset\":\"{}\",\"amount\":\"{}\",\"frozen\":{}}}",
                hex::encode(&account.account),
                hex::encode(&account.asset),
                account.amount,
                account.frozen
            )
        })
        .collect::<Vec<_>>()
        .join(",");
    format!(
        "{{\"program\":\"{}\",\"lifecycle\":\"{}\",\"accounts\":[{}],\"freshness\":{{\"observed_sequence\":{},\"observed_at\":{},\"receipt_digest\":\"{}\",\"state_root\":\"{}\",\"valid_through\":{}}}}}",
        hex::encode(&read.program),
        lifecycle(read.lifecycle),
        accounts,
        read.freshness.observed_sequence,
        read.freshness.observed_at,
        hex::encode(&read.freshness.receipt_digest),
        hex::encode(&read.freshness.state_root),
        read.freshness.valid_through
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
        .map_err(|error| format!("agent response failed: {error}"))
}

fn serve_connection(
    stream: &mut TcpStream,
    bearer: &str,
    probe_program: ProgramId,
    route: &mut ProgramBalanceReadRoute,
) -> Result<(), String> {
    let mut bytes = [0_u8; HEADER_LIMIT];
    let mut length = 0_usize;
    while length < bytes.len() && !bytes[..length].windows(4).any(|value| value == b"\r\n\r\n") {
        let count = stream
            .read(&mut bytes[length..])
            .map_err(|error| format!("agent request failed: {error}"))?;
        if count == 0 {
            return Err("agent request ended before its headers".to_owned());
        }
        length += count;
    }
    if !bytes[..length].windows(4).any(|value| value == b"\r\n\r\n") {
        return response(stream, 431, "{\"error\":\"headers_too_large\"}");
    }
    let request = std::str::from_utf8(&bytes[..length])
        .map_err(|_| "agent request headers are not UTF-8".to_owned())?;
    let line = request
        .lines()
        .next()
        .ok_or_else(|| "agent request omitted its request line".to_owned())?;
    let mut parts = line.split_ascii_whitespace();
    let method = parts.next().unwrap_or_default();
    let path = parts.next().unwrap_or_default();
    if parts.next() != Some("HTTP/1.1") || parts.next().is_some() || method != "GET" {
        return response(stream, 400, "{\"error\":\"invalid_request\"}");
    }
    let authorized = request
        .lines()
        .any(|header| header.strip_prefix("Authorization: Bearer ") == Some(bearer));
    if !authorized {
        return response(stream, 401, "{\"error\":\"unauthorized\"}");
    }
    if path == "/healthz" {
        return match route.read(probe_program, now_ms()?) {
            Ok(_) => response(stream, 200, "{\"ready\":true}"),
            Err(_) => response(stream, 503, "{\"ready\":false}"),
        };
    }
    let Some(program_text) = path
        .strip_prefix("/v1/programs/")
        .and_then(|value| value.strip_suffix("/balances"))
    else {
        return response(stream, 404, "{\"error\":\"not_found\"}");
    };
    let program = hex::decode_digest(program_text)
        .ok()
        .and_then(|bytes| ProgramId::new(bytes).ok());
    let Some(program) = program else {
        return response(stream, 400, "{\"error\":\"invalid_program\"}");
    };
    let read = route
        .read(program, now_ms()?)
        .map_err(|error| format!("current program state is unavailable: {error:?}"));
    match read {
        Ok(read) => response(stream, 200, &balance_json(&read)),
        Err(_) => response(stream, 503, "{\"error\":\"program_state_unavailable\"}"),
    }
}

fn serve(config: Config) -> Result<(), String> {
    let verifier = ProtocolDeploymentVerifier::new(
        config.protocol_version,
        config.network_id,
        config.epoch,
        config.sequencer_id,
        config.sequencer_public_key,
        config.first_batch,
        config.last_batch,
        config.revoked_from_batch,
        config.staleness_ms,
    )
    .map_err(|error| format!("agent deployment verifier is invalid: {error}"))?;
    let registry = load_registry(Path::new(&config.deployment_journal), &verifier)?;
    let reader = LayerxdProgramBalanceReader::connect(
        &config.node_endpoint,
        config.node_bearer,
        &config.authority_endpoint,
        config.authority_bearer,
        config.authority_replica_id,
        config.protocol_version,
        config.network_id,
        config.epoch,
        config.sequencer_id,
        config.sequencer_public_key,
        config.first_batch,
        config.last_batch,
        config.revoked_from_batch,
        registry,
        config.staleness_ms,
    )
    .map_err(|error| format!("agent protocol reader configuration failed: {error:?}"))?;
    let mut route = ProgramBalanceReadRoute::new(reader);
    route
        .read(config.probe_program, now_ms()?)
        .map_err(|error| format!("agent protocol reader is not ready: {error:?}"))?;
    let listener = TcpListener::bind(&config.listen)
        .map_err(|error| format!("agent program listener failed: {error}"))?;
    for incoming in listener.incoming() {
        let mut stream = incoming.map_err(|error| format!("agent accept failed: {error}"))?;
        stream
            .set_read_timeout(Some(Duration::from_secs(10)))
            .and_then(|()| stream.set_write_timeout(Some(Duration::from_secs(10))))
            .map_err(|error| format!("agent connection timeout setup failed: {error}"))?;
        let _ = serve_connection(
            &mut stream,
            &config.bearer,
            config.probe_program,
            &mut route,
        );
    }
    Ok(())
}

fn main() {
    if let Err(error) = config().and_then(serve) {
        eprintln!("layerx-agentd: {error}");
        std::process::exit(2);
    }
}
