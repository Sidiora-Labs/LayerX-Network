//! Real-process probe for the beta node: performs the LNI handshake with
//! `layerx-client`, reads an account balance, and talks to the supervisor
//! socket. Every result is printed as one JSON line on stdout.

use std::collections::BTreeMap;
use std::fmt::Write as _;
use std::io::{BufRead, BufReader, Read, Write};
use std::os::unix::fs::OpenOptionsExt;
use std::os::unix::net::UnixStream;
use std::path::PathBuf;
use std::process::ExitCode;
use std::time::Duration;

use layerx_client::client::{Client, ClientConfig, ReconnectPolicy};
use layerx_client::lni::handshake::HandshakeConfig;
use layerx_client::lni::schema::Version;
use layerx_client::lni::transport::Limits;
use layerx_proof::inclusion::SequencerAuthorization;
use layerx_types::account::AccountId;
use layerx_types::verify::VerificationLevel;

const FRAME_BYTES: usize = 1_212_416;

fn usage() -> ExitCode {
    eprintln!(
        "usage: layerx-node-probe handshake --socket PATH --network-id N\n       layerx-node-probe balance --socket PATH --network-id N --account NAME --asset HEX64\n       layerx-node-probe supervisor --socket PATH --request reset|status"
    );
    ExitCode::from(2)
}

fn options(arguments: &[String]) -> Result<BTreeMap<String, String>, String> {
    let mut parsed = BTreeMap::new();
    let mut index = 0;
    while index < arguments.len() {
        let key = &arguments[index];
        let Some(name) = key.strip_prefix("--") else {
            return Err(format!("unexpected argument {key}"));
        };
        let Some(value) = arguments.get(index + 1) else {
            return Err(format!("missing value for --{name}"));
        };
        parsed.insert(name.to_owned(), value.clone());
        index += 2;
    }
    Ok(parsed)
}

fn required<'a>(parsed: &'a BTreeMap<String, String>, name: &str) -> Result<&'a str, String> {
    parsed
        .get(name)
        .map(String::as_str)
        .ok_or_else(|| format!("--{name} is required"))
}

fn hex(bytes: &[u8]) -> String {
    let mut text = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        let _ = write!(text, "{byte:02x}");
    }
    text
}

fn hex32(text: &str) -> Result<[u8; 32], String> {
    if text.len() != 64 {
        return Err("expected 64 hex characters".to_owned());
    }
    let mut out = [0u8; 32];
    for (index, chunk) in text.as_bytes().chunks(2).enumerate() {
        let pair = std::str::from_utf8(chunk).map_err(|error| error.to_string())?;
        out[index] = u8::from_str_radix(pair, 16).map_err(|error| error.to_string())?;
    }
    Ok(out)
}

fn connect(socket: &str, network_id: u32) -> Result<Client, String> {
    Client::connect(ClientConfig {
        endpoint: PathBuf::from(socket),
        handshake: HandshakeConfig {
            built_interface_version: Version::V1_3,
            expected_protocol_version: layerx_wire::limits::STATE_COMMITMENT_PROTOCOL_VERSION,
            expected_network_id: network_id,
        },
        limits: Limits {
            maximum_frame_bytes: FRAME_BYTES,
            maximum_connections: 4,
            maximum_streams: 32,
            maximum_queued_bytes: 4 * FRAME_BYTES,
            deadline: Duration::from_secs(5),
        },
        reconnect: ReconnectPolicy {
            maximum_attempts: 3,
            base_delay: Duration::from_millis(50),
            maximum_delay: Duration::from_millis(500),
            jitter_percent: 10,
        },
    })
    .map_err(|error| format!("lni connect failed: {error:?}"))
}

fn handshake(parsed: &BTreeMap<String, String>) -> Result<String, String> {
    let socket = required(parsed, "socket")?;
    let network_id: u32 = required(parsed, "network-id")?
        .parse()
        .map_err(|error| format!("--network-id: {error}"))?;
    let client = connect(socket, network_id)?;
    let node = client.handshake().node();
    let capabilities: Vec<String> = client
        .handshake()
        .capabilities()
        .available()
        .iter()
        .map(|capability| format!("{capability:?}"))
        .collect();
    Ok(format!(
        "{{\"network_id\":{},\"protocol_version\":{},\"interface_version\":\"{}.{}\",\"role\":\"{:?}\",\"chain_head_sequence\":{},\"latest_sealed_batch\":{},\"sequencer_public_key\":\"{}\",\"capabilities\":[{}]}}",
        node.network_id,
        node.protocol_version,
        node.interface_version.major,
        node.interface_version.minor,
        node.role,
        node.chain_head_sequence,
        node.latest_sealed_batch,
        hex(&node.authorised_sequencer_key),
        capabilities
            .iter()
            .map(|name| format!("\"{name}\""))
            .collect::<Vec<_>>()
            .join(",")
    ))
}

fn balance(parsed: &BTreeMap<String, String>) -> Result<String, String> {
    let socket = required(parsed, "socket")?;
    let network_id: u32 = required(parsed, "network-id")?
        .parse()
        .map_err(|error| format!("--network-id: {error}"))?;
    let account = AccountId::parse(required(parsed, "account")?)
        .map_err(|error| format!("--account: {error:?}"))?;
    let asset = hex32(required(parsed, "asset")?)?;
    let mut client = connect(socket, network_id)?;
    let node = client.handshake().node();
    let account_id = layerx_wire::hash::account_id_for_protocol(&account, node.protocol_version)
        .map_err(|error| format!("account id: {error:?}"))?;
    let sequencer_key = node.authorised_sequencer_key;
    let sealed = node.latest_sealed_batch.max(1);
    let authorization = SequencerAuthorization::new(sequencer_key, sequencer_key, 1, sealed);
    let read = client
        .balance(
            account_id,
            asset,
            VerificationLevel::UNVERIFIED,
            1,
            authorization,
        )
        .map_err(|error| format!("balance read failed: {error:?}"))?;
    Ok(format!(
        "{{\"account\":\"{}\",\"account_id\":\"{}\",\"asset\":\"{}\",\"balance\":\"{}\",\"achieved\":\"{:?}\",\"global_sequence\":{}}}",
        account.canonical(),
        hex(&read.account),
        hex(&read.asset),
        read.amount.value(),
        read.achieved(),
        read.freshness().global_sequence
    ))
}

fn supervisor(parsed: &BTreeMap<String, String>) -> Result<String, String> {
    let socket = required(parsed, "socket")?;
    let request = required(parsed, "request")?;
    if request != "reset" && request != "status" {
        return Err("--request must be reset or status".to_owned());
    }
    let mut stream = UnixStream::connect(socket)
        .map_err(|error| format!("supervisor connect failed: {error}"))?;
    stream
        .set_read_timeout(Some(Duration::from_secs(330)))
        .map_err(|error| error.to_string())?;
    stream
        .write_all(format!("{request}\n").as_bytes())
        .map_err(|error| format!("supervisor write failed: {error}"))?;
    let mut reply = String::new();
    BufReader::new(stream)
        .read_line(&mut reply)
        .map_err(|error| format!("supervisor read failed: {error}"))?;
    let reply = reply.trim_end().to_owned();
    if reply.is_empty() {
        return Err("supervisor closed without a reply".to_owned());
    }
    Ok(reply)
}

fn write_send(parsed: &BTreeMap<String, String>) -> Result<String, String> {
    let network_id = required(parsed, "network-id")?
        .parse::<u32>()
        .map_err(|error| error.to_string())?;
    let seed_file = std::fs::OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_NONBLOCK | libc::O_NOFOLLOW)
        .open(required(parsed, "seed-file")?)
        .map_err(|error| error.to_string())?;
    let metadata = seed_file.metadata().map_err(|error| error.to_string())?;
    if !metadata.is_file() || metadata.len() != 32 {
        return Err("test seed must be a 32-byte regular file".to_owned());
    }
    let mut seed_bytes = Vec::new();
    seed_file
        .take(33)
        .read_to_end(&mut seed_bytes)
        .map_err(|error| error.to_string())?;
    let seed: [u8; 32] = seed_bytes
        .try_into()
        .map_err(|_| "test seed must be 32 bytes".to_owned())?;
    let source_did = layerx_platform_core::treasury_did(&seed);
    let actor = layerx_types::ids::Did::new(source_did.as_bytes())
        .map_err(|error| format!("actor: {error:?}"))?;
    let mut client = connect(required(parsed, "socket")?, network_id)?;
    let state = client
        .preparation_state(&actor, 1)
        .map_err(|error| format!("preparation: {error:?}"))?;
    let signed = layerx_platform_core::build_send(
        &seed,
        &layerx_platform_core::SendRequest {
            network_id,
            source_did,
            destination_did: required(parsed, "destination-did")?.to_owned(),
            asset: hex32(required(parsed, "asset")?)?,
            amount: 1,
            account_sequence: state
                .account_sequence
                .checked_add(1)
                .ok_or("sequence exhausted")?,
            idempotency_key: [0x61; 32],
            not_before_ms: state.protocol_timestamp,
            expires_at_ms: state
                .protocol_timestamp
                .checked_add(300_000)
                .ok_or("timestamp exhausted")?,
            fee_limit: 1000,
        },
    )?;
    let mut output = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(required(parsed, "output")?)
        .map_err(|error| error.to_string())?;
    output
        .write_all(&signed.canonical)
        .map_err(|error| error.to_string())?;
    output.sync_all().map_err(|error| error.to_string())?;
    Ok(hex(&signed.activity_id))
}

fn main() -> ExitCode {
    let arguments: Vec<String> = std::env::args().skip(1).collect();
    let Some(command) = arguments.first() else {
        return usage();
    };
    let parsed = match options(&arguments[1..]) {
        Ok(parsed) => parsed,
        Err(error) => {
            eprintln!("layerx-node-probe: {error}");
            return usage();
        }
    };
    let outcome = match command.as_str() {
        "handshake" => handshake(&parsed),
        "balance" => balance(&parsed),
        "supervisor" => supervisor(&parsed),
        "write-send" => write_send(&parsed),
        _ => return usage(),
    };
    match outcome {
        Ok(line) => {
            println!("{line}");
            ExitCode::SUCCESS
        }
        Err(error) => {
            eprintln!("layerx-node-probe: {error}");
            ExitCode::FAILURE
        }
    }
}
