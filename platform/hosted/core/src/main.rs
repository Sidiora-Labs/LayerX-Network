mod program_accounts;
mod program_lifecycle;
mod public_reads;

use layerx_client::client::{Client, ClientConfig, ReconnectPolicy};
use layerx_client::lni::handshake::{perform, Handshake, HandshakeConfig};
use layerx_client::lni::refusal::decode_core_refusal;
use layerx_client::lni::schema::{decode_envelope, encode_envelope, Capability, Envelope, Version};
use layerx_client::lni::simulate::SimulateError;
use layerx_client::lni::transport::{ConnectionGate, FrameTransport, Limits, Uds};
use layerx_client::read::ReadError;
use layerx_client::submit::{Submission, SubmitError};
use layerx_platform_core::{
    asset_registry, build_send_with_signer, did_for_public_key, fixed_hex, hex_decode, hex_encode,
    main_account, SendError, SendRequest, SocketSigner, TreasurySigner as _,
};
use layerx_proof::inclusion::SequencerAuthorization;
use layerx_proof::receipt::{verify_outcome, AuthorizedBatch};
use layerx_proof::state::decode_account_value;
use layerx_types::payload::{ActivityType, ModuleId, ModuleRegistration, ModuleRegistry};
use layerx_types::program_call::NativeProgramCall;
use layerx_types::verify::VerificationLevel;
use rustls::pki_types::{CertificateDer, PrivateKeyDer, PrivatePkcs8KeyDer};
use rustls::server::WebPkiClientVerifier;
use rustls::{RootCertStore, ServerConfig, ServerConnection, StreamOwned};
use serde::Deserialize;
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;
use std::env;
use std::fs;
use std::io::{BufRead, BufReader, Read, Write};
use std::net::{SocketAddr, TcpListener, TcpStream};
use std::os::unix::fs::{DirBuilderExt, OpenOptionsExt};
use std::os::unix::net::UnixStream;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};
use subtle::ConstantTimeEq;
use zeroize::{Zeroize, Zeroizing};

const MAX_REQUEST_BYTES: usize = 4 * 1024 * 1024;
const MAX_RELAY_BYTES: usize = 4 * 1024 * 1024;
const IO_TIMEOUT: Duration = Duration::from_secs(8);
const RESET_TIMEOUT: Duration = Duration::from_secs(180);
const MAX_CONNECTIONS: usize = 128;
const LNI_FRAME_BYTES: usize = 1_212_416;
const LNI_DEADLINE: Duration = Duration::from_secs(5);
const WIRE_VERSION: &str = "3";
const RECEIPT_LOOKUP_REQUEST_TAG: u16 = 5;
const RECEIPT_LOOKUP_RESPONSE_TAG: u16 = 6;
const ERROR_RESPONSE_TAG: u16 = 25;
const MAX_JOURNAL_BYTES: u64 = 16 * 1024 * 1024;
const MAX_REQUESTS_PER_CONNECTION: usize = 128;

static ACTIVE_CONNECTIONS: AtomicUsize = AtomicUsize::new(0);
static TRACE: AtomicU64 = AtomicU64::new(1);

fn pay_timing(stage: &str, started: Instant) {
    if std::env::var_os("LAYERX_PAY_TIMING").is_some() {
        eprintln!(
            "pay_timing stage={stage} duration_us={}",
            started.elapsed().as_micros()
        );
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Plane {
    Core,
    Admin,
}

struct Config {
    listen: SocketAddr,
    admin_listen: SocketAddr,
    tls: Arc<ServerConfig>,
    admin_tls: Arc<ServerConfig>,
    lni_socket: PathBuf,
    network_id: u32,
    node: NodeEndpoint,
    node_token: Zeroizing<String>,
    receipt_events_token: Option<Zeroizing<String>>,
    replica: NodeEndpoint,
    replica_token: Zeroizing<String>,
    admin_token: Zeroizing<String>,
    treasury: SocketSigner,
    treasury_did: String,
    treasury_asset: [u8; 32],
    sequencer_id: [u8; 32],
    supervisor_socket: PathBuf,
    state_dir: PathBuf,
    fee_limit: u128,
    receipt_deadline: Duration,
    admin_lock: Mutex<()>,
    journal_lock: Mutex<()>,
}

#[derive(Clone)]
struct NodeEndpoint {
    host: String,
    port: u16,
}

struct Request {
    method: String,
    path: String,
    query: Option<String>,
    headers: BTreeMap<String, String>,
    body: Vec<u8>,
}

struct Response {
    status: u16,
    body: String,
    retry_after: Option<u64>,
}

#[derive(Deserialize, serde::Serialize)]
#[serde(deny_unknown_fields)]
struct FundingCommand {
    funding_id: String,
    did: String,
    public_key: String,
    amount: u64,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ActivityBody {
    activity: String,
}

#[derive(Deserialize, serde::Serialize)]
struct JournalEntry {
    request_digest: String,
    status: u16,
    body: String,
    retry_after: Option<u64>,
}

#[derive(Deserialize, serde::Serialize)]
#[serde(deny_unknown_fields)]
struct PreparedFunding {
    canonical: Vec<u8>,
    activity_id: [u8; 32],
    signer_public_key: [u8; 32],
}

struct ReceiptFacts {
    activity_id: [u8; 32],
    batch_id: [u8; 32],
    global_sequence: u64,
    result_code: i32,
    state_root: [u8; 32],
    canonical: Vec<u8>,
}

fn read_secret(path_variable: &str) -> Result<Zeroizing<String>, String> {
    let path = env::var(path_variable).map_err(|_| format!("{path_variable} is required"))?;
    let mut value = fs::read_to_string(path).map_err(|error| error.to_string())?;
    while matches!(value.as_bytes().last(), Some(b'\n' | b'\r')) {
        value.pop();
    }
    if value.is_empty() || value.len() > 4096 {
        value.zeroize();
        return Err(format!("{path_variable} does not contain a bounded secret"));
    }
    Ok(Zeroizing::new(value))
}

fn required(name: &str) -> Result<String, String> {
    env::var(name).map_err(|_| format!("{name} is required"))
}

fn parse_listen(name: &str, default: &str) -> Result<SocketAddr, String> {
    env::var(name)
        .unwrap_or_else(|_| default.to_owned())
        .parse::<SocketAddr>()
        .map_err(|_| format!("{name} must be a socket address"))
}

fn parse_u64(name: &str, default: u64) -> Result<u64, String> {
    env::var(name).map_or(Ok(default), |value| {
        value
            .parse::<u64>()
            .map_err(|_| format!("{name} must be an integer"))
    })
}

fn install_provider() -> Result<(), String> {
    if rustls::crypto::CryptoProvider::get_default().is_some() {
        return Ok(());
    }
    rustls::crypto::ring::default_provider()
        .install_default()
        .map_err(|_| "failed to install TLS crypto provider".to_owned())
}

fn server_tls_config(
    certificate_variable: &str,
    key_variable: &str,
    client_ca: Option<&[u8]>,
) -> Result<Arc<ServerConfig>, String> {
    install_provider()?;
    let certificate = CertificateDer::from(
        fs::read(required(certificate_variable)?).map_err(|error| error.to_string())?,
    );
    let key = PrivateKeyDer::from(PrivatePkcs8KeyDer::from(
        fs::read(required(key_variable)?).map_err(|error| error.to_string())?,
    ));
    let builder = ServerConfig::builder();
    let config = match client_ca {
        Some(ca) => {
            let mut roots = RootCertStore::empty();
            roots
                .add(CertificateDer::from(ca.to_vec()))
                .map_err(|error| error.to_string())?;
            let verifier = WebPkiClientVerifier::builder(Arc::new(roots))
                .allow_unauthenticated()
                .build()
                .map_err(|error| error.to_string())?;
            builder.with_client_cert_verifier(verifier)
        }
        None => builder.with_no_client_auth(),
    }
    .with_single_cert(vec![certificate], key)
    .map_err(|error| error.to_string())?;
    Ok(Arc::new(config))
}

fn parse_node_url(value: &str) -> Result<NodeEndpoint, String> {
    let rest = value
        .strip_prefix("http://")
        .ok_or_else(|| "LAYERX_CORE_NODE_URL must use plaintext http on loopback".to_owned())?;
    let authority = rest.trim_end_matches('/');
    if authority.contains(['/', '?', '#', '@', '\\']) {
        return Err("LAYERX_CORE_NODE_URL must not carry a path".to_owned());
    }
    let (host, port) = authority
        .rsplit_once(':')
        .ok_or_else(|| "LAYERX_CORE_NODE_URL must carry a port".to_owned())?;
    let port = port
        .parse::<u16>()
        .map_err(|_| "LAYERX_CORE_NODE_URL port is invalid".to_owned())?;
    if host != "127.0.0.1" && host != "localhost" {
        return Err("LAYERX_CORE_NODE_URL must address the loopback listener".to_owned());
    }
    Ok(NodeEndpoint {
        host: host.to_owned(),
        port,
    })
}

fn config() -> Result<Config, String> {
    let client_ca = match env::var("LAYERX_CORE_CLIENT_CA_DER") {
        Ok(path) => Some(fs::read(path).map_err(|error| error.to_string())?),
        Err(_) => None,
    };
    let network_id = required("LAYERX_CORE_NETWORK_ID")?
        .parse::<u32>()
        .map_err(|_| "LAYERX_CORE_NETWORK_ID must be a 32-bit integer".to_owned())?;
    if network_id == 0 {
        return Err("LAYERX_CORE_NETWORK_ID must be non-zero".to_owned());
    }
    let treasury =
        SocketSigner::connect(Path::new(&required("LAYERX_CORE_TREASURY_SIGNER_SOCKET")?))
            .map_err(|error| format!("LAYERX_CORE_TREASURY_SIGNER_SOCKET: {error}"))?;
    let treasury_asset = fixed_hex::<32>(
        "LAYERX_CORE_TREASURY_ASSET",
        &required("LAYERX_CORE_TREASURY_ASSET")?,
    )?;
    if treasury_asset == [0; 32] {
        return Err("LAYERX_CORE_TREASURY_ASSET must be non-zero".to_owned());
    }
    let sequencer_id = fixed_hex::<32>(
        "LAYERX_CORE_SEQUENCER_ID",
        &required("LAYERX_CORE_SEQUENCER_ID")?,
    )?;
    let state_dir = PathBuf::from(required("LAYERX_CORE_STATE_DIR")?);
    let mut journal = fs::DirBuilder::new();
    journal.recursive(true).mode(0o700);
    journal
        .create(state_dir.join("journal"))
        .map_err(|error| format!("LAYERX_CORE_STATE_DIR is unusable: {error}"))?;
    Ok(Config {
        listen: parse_listen("LAYERX_CORE_LISTEN", "0.0.0.0:9443")?,
        admin_listen: parse_listen("LAYERX_CORE_ADMIN_LISTEN", "0.0.0.0:9444")?,
        tls: server_tls_config(
            "LAYERX_CORE_TLS_CERT_DER",
            "LAYERX_CORE_TLS_KEY_DER",
            client_ca.as_deref(),
        )?,
        admin_tls: server_tls_config(
            "LAYERX_CORE_ADMIN_TLS_CERT_DER",
            "LAYERX_CORE_ADMIN_TLS_KEY_DER",
            None,
        )?,
        lni_socket: PathBuf::from(required("LAYERX_CORE_LNI_SOCKET")?),
        network_id,
        node: parse_node_url(&required("LAYERX_CORE_NODE_URL")?)?,
        node_token: read_secret("LAYERX_CORE_NODE_BEARER_TOKEN_FILE")?,
        receipt_events_token: env::var_os("LAYERX_CORE_RECEIPT_EVENTS_TOKEN_FILE")
            .map(|_| read_secret("LAYERX_CORE_RECEIPT_EVENTS_TOKEN_FILE"))
            .transpose()?,
        replica: parse_node_url(&required("LAYERX_CORE_REPLICA_URL")?)?,
        replica_token: read_secret("LAYERX_CORE_REPLICA_BEARER_TOKEN_FILE")?,
        admin_token: read_secret("LAYERX_CORE_ADMIN_TOKEN_FILE")?,
        treasury_did: did_for_public_key(&treasury.public_key()),
        treasury,
        treasury_asset,
        sequencer_id,
        supervisor_socket: PathBuf::from(required("LAYERX_CORE_SUPERVISOR_SOCKET")?),
        state_dir,
        fee_limit: u128::from(parse_u64("LAYERX_CORE_FEE_LIMIT", 1_000)?),
        receipt_deadline: Duration::from_millis(parse_u64(
            "LAYERX_CORE_RECEIPT_DEADLINE_MS",
            15_000,
        )?),
        admin_lock: Mutex::new(()),
        journal_lock: Mutex::new(()),
    })
}

fn read_http_message(stream: &mut impl Read, maximum: usize) -> Result<Request, String> {
    let mut bytes = Vec::with_capacity(2048);
    let mut chunk = [0_u8; 4096];
    let header_end = loop {
        let count = stream.read(&mut chunk).map_err(|error| error.to_string())?;
        if count == 0 || bytes.len().saturating_add(count) > maximum {
            return Err("HTTP message is empty or exceeds its bound".to_owned());
        }
        bytes.extend_from_slice(&chunk[..count]);
        if let Some(position) = bytes.windows(4).position(|window| window == b"\r\n\r\n") {
            break position + 4;
        }
        if bytes.len() > 16 * 1024 {
            return Err("HTTP headers exceed their bound".to_owned());
        }
    };
    let source = std::str::from_utf8(&bytes[..header_end])
        .map_err(|_| "HTTP headers are not UTF-8".to_owned())?;
    let mut lines = source.split("\r\n");
    let first = lines
        .next()
        .ok_or_else(|| "HTTP start line is missing".to_owned())?
        .to_owned();
    let mut headers = BTreeMap::new();
    headers.insert(String::new(), first);
    let mut content_length = 0_usize;
    for line in lines.filter(|line| !line.is_empty()) {
        let (name, value) = line
            .split_once(':')
            .ok_or_else(|| "HTTP header is malformed".to_owned())?;
        let name = name.trim().to_ascii_lowercase();
        let value = value.trim().to_owned();
        if headers.contains_key(&name) {
            return Err("duplicate HTTP header".to_owned());
        }
        if name == "transfer-encoding" {
            return Err("transfer-encoded messages are not accepted".to_owned());
        }
        if name == "content-length" {
            content_length = value
                .parse::<usize>()
                .map_err(|_| "content length is invalid".to_owned())?;
        }
        headers.insert(name, value);
    }
    if header_end.saturating_add(content_length) > maximum {
        return Err("HTTP body exceeds its bound".to_owned());
    }
    while bytes.len() < header_end + content_length {
        let count = stream.read(&mut chunk).map_err(|error| error.to_string())?;
        if count == 0 || bytes.len().saturating_add(count) > maximum {
            return Err("HTTP body is truncated or exceeds its bound".to_owned());
        }
        bytes.extend_from_slice(&chunk[..count]);
    }
    Ok(Request {
        method: String::new(),
        path: String::new(),
        query: None,
        headers,
        body: bytes[header_end..header_end + content_length].to_vec(),
    })
}

fn valid_query(query: &str) -> bool {
    !query.is_empty()
        && query.len() <= 512
        && query.bytes().all(|byte| {
            byte.is_ascii_alphanumeric() || matches!(byte, b'=' | b'&' | b'-' | b'_' | b'.')
        })
}

fn parse_client_request(stream: &mut impl Read) -> Result<Request, String> {
    let mut request = read_http_message(stream, MAX_REQUEST_BYTES)?;
    let start = request
        .headers
        .remove("")
        .ok_or_else(|| "request line is missing".to_owned())?;
    let mut parts = start.split_whitespace();
    parts
        .next()
        .ok_or_else(|| "request method is missing".to_owned())?
        .clone_into(&mut request.method);
    let target = parts
        .next()
        .ok_or_else(|| "request target is missing".to_owned())?
        .to_owned();
    if parts.next() != Some("HTTP/1.1") || parts.next().is_some() {
        return Err("request line is invalid".to_owned());
    }
    match target.split_once('?') {
        Some((path, query)) => {
            if !valid_query(query) {
                return Err("request query is invalid".to_owned());
            }
            path.clone_into(&mut request.path);
            request.query = Some(query.to_owned());
        }
        None => request.path = target,
    }
    if !request.path.starts_with('/') || request.path.contains(['#', '\\', ' ']) {
        return Err("request path is invalid".to_owned());
    }
    if !request.headers.contains_key("host") {
        return Err("HTTP/1.1 Host header is required".to_owned());
    }
    Ok(request)
}

fn refusal(status: u16, code: &str, retry_after: Option<u64>) -> Response {
    let retry = if retry_after.is_some() {
        "after"
    } else {
        "never"
    };
    let body = retry_after.map_or_else(
        || serde_json::json!({ "error": { "code": code, "retry": retry } }),
        |seconds| serde_json::json!({ "error": { "code": code, "retry": retry, "retry_after_seconds": seconds } }),
    );
    Response {
        status,
        body: body.to_string(),
        retry_after,
    }
}

fn json_response(status: u16, value: &serde_json::Value) -> Response {
    Response {
        status,
        body: value.to_string(),
        retry_after: None,
    }
}

fn next_trace() -> String {
    format!("core-{}", TRACE.fetch_add(1, Ordering::AcqRel))
}

fn success(result: &serde_json::Value) -> Response {
    json_response(
        200,
        &serde_json::json!({ "ok": true, "result": result, "trace": next_trace() }),
    )
}

fn write_response(
    stream: &mut impl Write,
    response: &Response,
    keep_alive: bool,
) -> Result<(), String> {
    let reason = match response.status {
        200 => "OK",
        202 => "Accepted",
        400 => "Bad Request",
        401 => "Unauthorized",
        404 => "Not Found",
        405 => "Method Not Allowed",
        409 => "Conflict",
        422 => "Unprocessable Entity",
        429 => "Too Many Requests",
        _ => "Service Unavailable",
    };
    let retry = response.retry_after.map_or(String::new(), |seconds| {
        format!("Retry-After: {seconds}\r\n")
    });
    let connection = if keep_alive { "keep-alive" } else { "close" };
    write!(
        stream,
        "HTTP/1.1 {} {reason}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nCache-Control: no-store\r\n{retry}Connection: {connection}\r\n\r\n{}",
        response.status,
        response.body.len(),
        response.body
    )
    .map_err(|error| error.to_string())
}

fn lni_limits() -> Limits {
    Limits {
        maximum_frame_bytes: LNI_FRAME_BYTES,
        maximum_connections: 4,
        maximum_streams: 1,
        maximum_queued_bytes: 4 * 1024 * 1024,
        deadline: LNI_DEADLINE,
    }
}

fn handshake_config(config: &Config) -> HandshakeConfig {
    HandshakeConfig {
        built_interface_version: Version::V1_5,
        expected_protocol_version: layerx_wire::limits::STATE_COMMITMENT_PROTOCOL_VERSION,
        expected_network_id: config.network_id,
    }
}

fn connect_client(config: &Config) -> Result<Client, String> {
    Client::connect(ClientConfig {
        endpoint: config.lni_socket.clone(),
        handshake: handshake_config(config),
        limits: lni_limits(),
        reconnect: ReconnectPolicy {
            maximum_attempts: 1,
            base_delay: Duration::from_millis(100),
            maximum_delay: Duration::from_secs(1),
            jitter_percent: 0,
        },
    })
    .map_err(|error| format!("LNI connection failed: {error:?}"))
}

fn connect_raw(config: &Config) -> Result<(Uds, Handshake), String> {
    connect_raw_with_deadline(config, LNI_DEADLINE)
}

fn connect_raw_with_deadline(
    config: &Config,
    deadline: Duration,
) -> Result<(Uds, Handshake), String> {
    let gate = ConnectionGate::new(1);
    let mut limits = lni_limits();
    limits.deadline = deadline.min(limits.deadline);
    let mut transport = Uds::connect(&config.lni_socket, &gate, limits)
        .map_err(|error| format!("LNI connection failed: {error:?}"))?;
    let handshake = perform(&mut transport, &handshake_config(config), None)
        .map_err(|error| format!("LNI handshake failed: {error:?}"))?;
    Ok((transport, handshake))
}

fn lookup_receipt_bytes(
    transport: &mut Uds,
    handshake: &Handshake,
    activity_id: [u8; 32],
    correlation_id: u64,
    wait_publication: bool,
) -> Result<Option<Vec<u8>>, String> {
    let mut selector = Vec::with_capacity(37);
    selector.push(1);
    selector.extend_from_slice(&activity_id);
    lookup_receipt_selector(
        transport,
        handshake,
        selector,
        correlation_id,
        wait_publication,
    )
}

fn lookup_receipt_selector(
    transport: &mut Uds,
    handshake: &Handshake,
    mut selector: Vec<u8>,
    correlation_id: u64,
    wait_publication: bool,
) -> Result<Option<Vec<u8>>, String> {
    if !handshake.capabilities().contains(Capability::ReceiptLookup) {
        return Err("receipt_lookup capability is unavailable".to_owned());
    }
    if wait_publication {
        if handshake.node().interface_version.minor < 5 {
            return Err("receipt publication wait requires LNI minor 5".to_owned());
        }
        selector.push(1);
    }
    let request = encode_envelope(Envelope {
        version: handshake.node().interface_version,
        message_tag: RECEIPT_LOOKUP_REQUEST_TAG,
        correlation_id,
        canonical_payload: &selector,
        proof_material: &[],
    })
    .map_err(|error| format!("receipt lookup encoding failed: {error:?}"))?;
    transport
        .send(&request)
        .map_err(|error| format!("receipt lookup send failed: {error:?}"))?;
    let response_bytes = transport
        .receive()
        .map_err(|error| format!("receipt lookup receive failed: {error:?}"))?;
    let response = decode_envelope(&response_bytes)
        .map_err(|error| format!("receipt lookup response is malformed: {error:?}"))?;
    if response.version.major != handshake.node().interface_version.major
        || !response.proof_material.is_empty()
        || response.correlation_id != correlation_id
    {
        return Err("receipt lookup response correlation mismatch".to_owned());
    }
    if response.message_tag == ERROR_RESPONSE_TAG {
        let refusal = decode_core_refusal(response.canonical_payload)
            .ok_or_else(|| "receipt lookup refusal is malformed".to_owned())?;
        return Err(format!(
            "receipt lookup refused: class {} result {}",
            refusal.class,
            refusal.result.raw()
        ));
    }
    if response.message_tag != RECEIPT_LOOKUP_RESPONSE_TAG {
        return Err("receipt lookup response has an unexpected tag".to_owned());
    }
    if response.canonical_payload.is_empty() {
        return Ok(None);
    }
    Ok(Some(response.canonical_payload.to_vec()))
}

fn receipt_facts(bytes: &[u8], sequencer_key: [u8; 32]) -> Result<ReceiptFacts, String> {
    let decoded = layerx_wire::receipt::decode(bytes)
        .map_err(|error| format!("receipt does not decode: {error:?}"))?;
    let protocol = decoded
        .protocol()
        .ok_or_else(|| "receipt is not a protocol receipt".to_owned())?;
    let authorised = AuthorizedBatch::new(
        protocol.batch_id(),
        protocol.asset(),
        protocol.previous_state_root(),
        protocol.resulting_state_root(),
        sequencer_key,
    );
    if protocol.module_id() == 9 && protocol.operation() == 0 {
        program_lifecycle::verify_receipt(bytes, &authorised, protocol.activity_id())?;
        return Ok(ReceiptFacts {
            activity_id: protocol.activity_id(),
            batch_id: protocol.batch_id(),
            global_sequence: protocol.global_sequence(),
            result_code: protocol.result_code(),
            state_root: protocol.resulting_state_root(),
            canonical: bytes.to_vec(),
        });
    }
    let verified = verify_outcome(bytes, &authorised)
        .map_err(|error| format!("receipt verification failed: {error:?}"))?;
    let receipt = verified
        .receipt()
        .protocol()
        .ok_or_else(|| "verified receipt is not a protocol receipt".to_owned())?;
    Ok(ReceiptFacts {
        activity_id: receipt.activity_id(),
        batch_id: receipt.batch_id(),
        global_sequence: receipt.global_sequence(),
        result_code: receipt.result_code(),
        state_root: receipt.resulting_state_root(),
        canonical: verified.canonical_bytes().to_vec(),
    })
}

fn await_receipt(
    config: &Config,
    activity_id: [u8; 32],
    deadline: Duration,
) -> Result<Option<ReceiptFacts>, String> {
    let total_started = Instant::now();
    let connect_started = Instant::now();
    let (mut transport, handshake) = if deadline.is_zero() {
        connect_raw(config)?
    } else {
        connect_raw_with_deadline(config, deadline)?
    };
    pay_timing("core.receipt.connect_handshake", connect_started);
    let wait_started = Instant::now();
    let bytes = lookup_receipt_bytes(
        &mut transport,
        &handshake,
        activity_id,
        1,
        !deadline.is_zero(),
    )?;
    pay_timing("core.receipt.publication_wait", wait_started);
    let Some(bytes) = bytes else {
        return Ok(None);
    };
    let verify_started = Instant::now();
    let facts = receipt_facts(&bytes, handshake.node().authorised_sequencer_key)?;
    pay_timing("core.receipt.verify", verify_started);
    if facts.activity_id != activity_id {
        return Err("receipt names another activity".to_owned());
    }
    pay_timing("core.receipt.total", total_started);
    Ok(Some(facts))
}

fn receipt_result(facts: &ReceiptFacts) -> serde_json::Value {
    serde_json::json!({
        "state": if facts.result_code == 0 { "completed" } else { "refused" },
        "activity_id": hex_encode(&facts.activity_id),
        "batch_id": hex_encode(&facts.batch_id),
        "global_sequence": facts.global_sequence,
        "result_code": facts.result_code,
        "state_root": hex_encode(&facts.state_root),
        "receipt": hex_encode(&facts.canonical),
    })
}

fn signer_key(authority: &[u8]) -> Option<[u8; 32]> {
    match authority.len() {
        32 => authority.try_into().ok(),
        33 if authority[0] == 1 => authority[1..].try_into().ok(),
        _ => None,
    }
}

const ASSET_SUBMISSION_ORDINALS: [u16; 10] = [1, 2, 3, 4, 5, 6, 7, 8, 10, 11];
const PROGRAM_SUBMISSION_ORDINALS: [u16; 6] = [1, 2, 3, 5, 6, 7];

fn registry_with_asset_ordinals(asset_ordinals: &[u16]) -> Result<ModuleRegistry, String> {
    let asset_operations = asset_ordinals
        .iter()
        .copied()
        .map(|ordinal| ActivityType::new(ModuleId::Asset, ordinal))
        .collect::<Result<Vec<_>, _>>()
        .map_err(|error| format!("asset activity: {error:?}"))?;
    let program_operations = PROGRAM_SUBMISSION_ORDINALS
        .map(|ordinal| ActivityType::new(ModuleId::Programs, ordinal))
        .into_iter()
        .collect::<Result<Vec<_>, _>>()
        .map_err(|error| format!("program activity: {error:?}"))?;
    let asset = ModuleRegistration::new(ModuleId::Asset, &asset_operations)
        .map_err(|error| format!("asset registration: {error:?}"))?;
    let programs = ModuleRegistration::new(ModuleId::Programs, &program_operations)
        .map_err(|error| format!("program registration: {error:?}"))?;
    ModuleRegistry::new(&[asset, programs]).map_err(|error| format!("module registry: {error:?}"))
}

fn submission_registry() -> Result<ModuleRegistry, String> {
    registry_with_asset_ordinals(&ASSET_SUBMISSION_ORDINALS)
}

fn submission_decode_registry() -> Result<ModuleRegistry, String> {
    let mut ordinals = ASSET_SUBMISSION_ORDINALS.to_vec();
    ordinals.push(9);
    ordinals.sort_unstable();
    registry_with_asset_ordinals(&ordinals)
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum SubmissionValidationError {
    AssetOrdinalReserved,
    InvalidAssetActivity,
    InvalidProgramCall,
    InvalidProgramAccountOperation,
    InvalidProgramLifecycle,
}

impl SubmissionValidationError {
    fn response(self) -> Response {
        match self {
            Self::AssetOrdinalReserved => refusal(422, "asset_ordinal_reserved", None),
            Self::InvalidAssetActivity => refusal(400, "invalid_asset_activity", None),
            Self::InvalidProgramCall => refusal(400, "invalid_program_call", None),
            Self::InvalidProgramAccountOperation => {
                refusal(400, "invalid_program_account_operation", None)
            }
            Self::InvalidProgramLifecycle => refusal(400, "invalid_program_lifecycle", None),
        }
    }
}

fn validate_submission_payload(
    canonical: &[u8],
    activity: &layerx_wire::activity::Activity,
    registry: &ModuleRegistry,
) -> Result<(), SubmissionValidationError> {
    let module = activity.activity_type().module();
    let ordinal = activity.activity_type().ordinal();
    if module == ModuleId::Asset {
        if ordinal == 9 {
            return Err(SubmissionValidationError::AssetOrdinalReserved);
        }
        let unsigned = layerx_wire::activity::encode_unsigned(activity)
            .map_err(|_| SubmissionValidationError::InvalidAssetActivity)?;
        layerx_crypto::disclosure::bind(&unsigned, registry)
            .map(|_| ())
            .map_err(|_| SubmissionValidationError::InvalidAssetActivity)?;
    } else if module == ModuleId::Programs {
        if ordinal == 3 {
            NativeProgramCall::decode(activity.payload())
                .map_err(|_| SubmissionValidationError::InvalidProgramCall)?;
        } else if matches!(ordinal, 5 | 6) {
            let unsigned = layerx_wire::activity::encode_unsigned(activity)
                .map_err(|_| SubmissionValidationError::InvalidProgramAccountOperation)?;
            layerx_crypto::disclosure::bind(&unsigned, registry)
                .map_err(|_| SubmissionValidationError::InvalidProgramAccountOperation)?;
            program_accounts::validate(ordinal, activity.payload())
                .map_err(|()| SubmissionValidationError::InvalidProgramAccountOperation)?;
        } else {
            program_lifecycle::validate(canonical, registry, ordinal)
                .map_err(|_| SubmissionValidationError::InvalidProgramLifecycle)?;
        }
    }
    Ok(())
}

fn submit_activity(
    config: &Config,
    canonical: &[u8],
    program_ordinal: Option<u16>,
) -> Result<Response, Response> {
    let total_started = Instant::now();
    let validate_started = Instant::now();
    let decode_registry =
        submission_decode_registry().map_err(|_| refusal(503, "registry_unavailable", Some(5)))?;
    let activity = layerx_wire::activity::decode_signed(canonical, &decode_registry)
        .map_err(|_| refusal(400, "invalid_activity", None))?;
    if let Some(ordinal) = program_ordinal {
        if activity.activity_type().module() != ModuleId::Programs
            || activity.activity_type().ordinal() != ordinal
            || activity.protocol_version() != 3
        {
            return Err(refusal(400, "program_route_mismatch", None));
        }
    }
    validate_submission_payload(canonical, &activity, &decode_registry)
        .map_err(SubmissionValidationError::response)?;
    let registry =
        submission_registry().map_err(|_| refusal(503, "registry_unavailable", Some(5)))?;
    pay_timing("core.submit.validate", validate_started);
    let signer = signer_key(activity.authority())
        .ok_or_else(|| refusal(400, "authority_unsupported", None))?;
    let connect_started = Instant::now();
    let mut client = connect_client(config).map_err(|error| {
        eprintln!("layerx-core-boundary: {error}");
        refusal(503, "node_unavailable", Some(5))
    })?;
    pay_timing("core.submit.connect_handshake", connect_started);
    let admission_started = Instant::now();
    let submission = client
        .submit_signed(&registry, signer, 1, 1, canonical)
        .map_err(|error| match error {
            SubmitError::CoreRefusal { class, result } => {
                eprintln!(
                    "layerx-core-boundary: submission refused class {class} result {}",
                    result.raw()
                );
                refusal(422, "submission_refused", None)
            }
            SubmitError::UnavailableCapability => refusal(503, "capability_unavailable", Some(30)),
            SubmitError::Disconnected => refusal(503, "node_unavailable", Some(5)),
            _ => refusal(400, "invalid_activity", None),
        })?;
    pay_timing("core.submit.admission", admission_started);
    drop(client);
    let activity_id = match submission {
        Submission::Acknowledged(acknowledgement) => acknowledgement.activity_id(),
        Submission::Unknown(unknown) => unknown.activity_id(),
    };
    let receipt_started = Instant::now();
    let result = match await_receipt(config, activity_id, config.receipt_deadline) {
        Ok(Some(facts)) => {
            if program_ordinal.is_some_and(|ordinal| matches!(ordinal, 1 | 2 | 7)) {
                Ok(success(&serde_json::json!({
                    "state": if facts.result_code == 0 { "completed" } else { "refused" },
                    "activity_id": hex_encode(&facts.activity_id), "receipt": hex_encode(&facts.canonical),
                    "terminal_payload": "", "call_graph": "",
                })))
            } else {
                Ok(success(&receipt_result(&facts)))
            }
        }
        Ok(None) => Ok(json_response(
            202,
            &serde_json::json!({
                "ok": true,
                "result": { "state": "pending", "activity_id": hex_encode(&activity_id) },
                "trace": next_trace(),
            }),
        )),
        Err(error) => {
            eprintln!("layerx-core-boundary: {error}");
            Err(refusal(503, "receipt_unavailable", Some(5)))
        }
    };
    pay_timing("core.submit.receipt", receipt_started);
    pay_timing("core.submit.total", total_started);
    result
}

fn simulate_activity(config: &Config, canonical: &[u8]) -> Result<Response, Response> {
    let registry =
        submission_registry().map_err(|_| refusal(503, "registry_unavailable", Some(5)))?;
    let activity = layerx_wire::activity::decode_signed(canonical, &registry)
        .map_err(|_| refusal(400, "invalid_activity", None))?;
    if activity.activity_type().module() != ModuleId::Programs
        || activity.activity_type().ordinal() != 3
    {
        return Err(refusal(400, "not_program_call", None));
    }
    let call = NativeProgramCall::decode(activity.payload())
        .map_err(|_| refusal(400, "invalid_program_call", None))?;
    let expected_activity_id = layerx_wire::hash::activity_id(&activity)
        .map_err(|_| refusal(400, "invalid_activity", None))?;
    let mut client = connect_client(config).map_err(|error| {
        eprintln!("layerx-core-boundary: {error}");
        refusal(503, "node_unavailable", Some(5))
    })?;
    let sequencer_key = client.handshake().node().authorised_sequencer_key;
    let simulation = client
        .simulate(&registry, canonical, 1)
        .map_err(|error| match error {
            SimulateError::CoreRefusal { class, result } => {
                eprintln!(
                    "layerx-core-boundary: simulation refused class {class} result {}",
                    result.raw()
                );
                if class == 3 {
                    refusal(503, "capability_unavailable", Some(30))
                } else {
                    refusal(422, "simulation_refused", None)
                }
            }
            SimulateError::UnavailableCapability | SimulateError::InterfaceVersion(_) => {
                refusal(503, "capability_unavailable", Some(30))
            }
            SimulateError::Disconnected | SimulateError::Transport(_) => {
                refusal(503, "node_unavailable", Some(5))
            }
            SimulateError::MalformedRequest => refusal(400, "invalid_activity", None),
            other => {
                eprintln!("layerx-core-boundary: simulation unverifiable: {other:?}");
                refusal(503, "node_unavailable", Some(5))
            }
        })?;
    drop(client);
    if simulation.execution.activity_id != expected_activity_id {
        return Err(refusal(503, "node_unavailable", Some(5)));
    }
    let receipt = layerx_proof::receipt::verify_sequencer_signature(
        &simulation.execution.receipt,
        sequencer_key,
    )
    .map_err(|_| refusal(503, "node_unavailable", Some(5)))?;
    let protocol = receipt
        .protocol()
        .ok_or_else(|| refusal(503, "node_unavailable", Some(5)))?;
    let state = if protocol.result_code() == 0 {
        "simulated"
    } else {
        "refused"
    };
    Ok(success(&serde_json::json!({
        "committed": false,
        "execution": {
            "state": state,
            "activity_id": hex_encode(&simulation.execution.activity_id),
            "program_id": hex_encode(&call.callee().bytes()),
            "result_code": protocol.result_code(),
            "receipt": hex_encode(&simulation.execution.receipt),
            "terminal_payload": hex_encode(&simulation.execution.terminal_payload),
            "call_graph": hex_encode(&simulation.execution.call_graph),
        },
        "simulation_evidence": {
            "boundary_id": hex_encode(&simulation.evidence.boundary_id),
            "activity_id": hex_encode(&simulation.evidence.activity_id),
            "previous_state_root": hex_encode(&simulation.evidence.previous_state_root),
            "hypothetical_state_root": hex_encode(&simulation.evidence.hypothetical_state_root),
            "observed_sequence": simulation.evidence.observed_sequence.to_string(),
            "observed_at": simulation.evidence.observed_at.to_string(),
            "committed": false,
            "public_key": hex_encode(&simulation.evidence.public_key),
            "signature": hex_encode(&simulation.evidence.signature),
        }
    })))
}

fn simulate_route(config: &Config, request: &Request) -> Response {
    let canonical = match request.headers.get("content-type").map(String::as_str) {
        Some("application/octet-stream") => request.body.clone(),
        Some("application/json") => {
            let Ok(body) = serde_json::from_slice::<ActivityBody>(&request.body) else {
                return refusal(400, "invalid_argument", None);
            };
            match hex_decode(&body.activity) {
                Ok(bytes) => bytes,
                Err(_) => return refusal(400, "invalid_argument", None),
            }
        }
        _ => return refusal(400, "content_type_required", None),
    };
    if canonical.is_empty() || canonical.len() > LNI_FRAME_BYTES {
        return refusal(400, "invalid_argument", None);
    }
    match simulate_activity(config, &canonical) {
        Ok(response) | Err(response) => response,
    }
}

fn activities_route(config: &Config, request: &Request) -> Response {
    let total_started = Instant::now();
    let decode_started = Instant::now();
    let ordinal = program_lifecycle::ordinal(&request.path)
        .or_else(|| (request.path == "/v1/programs/call").then_some(3));
    if program_lifecycle::ordinal(&request.path).is_some()
        && request.headers.get("content-type").map(String::as_str)
            != Some("application/octet-stream")
    {
        return refusal(415, "activity_content_type_required", None);
    }
    let canonical = match request.headers.get("content-type").map(String::as_str) {
        Some("application/octet-stream") => request.body.clone(),
        Some("application/json") => {
            let Ok(body) = serde_json::from_slice::<ActivityBody>(&request.body) else {
                return refusal(400, "invalid_argument", None);
            };
            match hex_decode(&body.activity) {
                Ok(bytes) => bytes,
                Err(_) => return refusal(400, "invalid_argument", None),
            }
        }
        _ => return refusal(400, "content_type_required", None),
    };
    if canonical.is_empty() || canonical.len() > 1_048_576 {
        return refusal(400, "invalid_argument", None);
    }
    if ordinal.is_some() {
        let Some(key) = request.headers.get("idempotency-key") else {
            return refusal(400, "idempotency_key_required", None);
        };
        let Ok(registry) = submission_registry() else {
            return refusal(503, "registry_unavailable", Some(5));
        };
        let Ok(activity) = layerx_wire::activity::decode_signed(&canonical, &registry) else {
            return refusal(400, "invalid_activity", None);
        };
        if key != &hex_encode(&activity.idempotency_key()) {
            return refusal(409, "protocol_idempotency_mismatch", None);
        }
    }
    pay_timing("core.activity.decode", decode_started);
    let submit_started = Instant::now();
    let response = match submit_activity(config, &canonical, ordinal) {
        Ok(response) | Err(response) => response,
    };
    pay_timing("core.activity.submit", submit_started);
    pay_timing("core.activity.total", total_started);
    response
}

fn receipt_route(config: &Config, activity_hex: &str) -> Response {
    let Ok(activity_id) = fixed_hex::<32>("activity id", activity_hex) else {
        return refusal(400, "invalid_argument", None);
    };
    if activity_hex.bytes().any(|byte| byte.is_ascii_uppercase()) {
        return refusal(400, "invalid_argument", None);
    }
    match await_receipt(config, activity_id, Duration::ZERO) {
        Ok(Some(facts)) => success(&receipt_result(&facts)),
        Ok(None) => refusal(404, "not_found", None),
        Err(error) => {
            eprintln!("layerx-core-boundary: {error}");
            refusal(503, "node_unavailable", Some(5))
        }
    }
}

fn program_idempotency_receipt(config: &Config, key: &str) -> Response {
    #[derive(Deserialize)]
    #[serde(deny_unknown_fields)]
    struct Document {
        activity_id: String,
        receipt: String,
    }
    if key.len() != 64
        || key
            .bytes()
            .any(|byte| !byte.is_ascii_digit() && !(b'a'..=b'f').contains(&byte))
    {
        return refusal(400, "invalid_idempotency_key", None);
    }
    let target = format!("/v1/programs/receipts/by-idempotency/{key}");
    let body = match node_get(&config.node, &config.node_token, &target) {
        Ok((200, body)) => body,
        Ok((404, _)) => return refusal(404, "receipt_not_found", None),
        _ => return refusal(503, "receipt_unavailable", Some(5)),
    };
    let document: Document = match serde_json::from_slice(&body) {
        Ok(value) => value,
        Err(_) => return refusal(502, "receipt_invalid", None),
    };
    let bytes = match hex_decode(&document.receipt) {
        Ok(value)
            if !value.is_empty()
                && value.len() <= 1_048_576
                && hex_encode(&value) == document.receipt =>
        {
            value
        }
        _ => return refusal(502, "receipt_invalid", None),
    };
    let Ok((_, handshake)) = connect_raw(config) else {
        return refusal(503, "node_unavailable", Some(5));
    };
    let Ok(receipt) = layerx_proof::receipt::verify_sequencer_signature(
        &bytes,
        handshake.node().authorised_sequencer_key,
    ) else {
        return refusal(502, "receipt_invalid", None);
    };
    let Some(protocol) = receipt.protocol() else {
        return refusal(502, "receipt_invalid", None);
    };
    if protocol.protocol_version() != 3
        || protocol.module_id() != 9
        || protocol.module_version() != 4
        || document.activity_id != hex_encode(&protocol.activity_id())
    {
        return refusal(502, "receipt_invalid", None);
    }
    success(&serde_json::json!({"activity_id": document.activity_id, "receipt": document.receipt}))
}

fn readiness(config: &Config) -> Response {
    match connect_client(config) {
        Ok(client) => {
            let zero = "0".repeat(64);
            let target = format!("/v1/batches/{zero}/receipt-authority?receipt_digest={zero}");
            if !matches!(
                node_get(&config.replica, &config.replica_token, &target),
                Ok((200 | 404, _))
            ) {
                return refusal(503, "replica_unavailable", Some(5));
            }
            let Ok(_guard) = config.journal_lock.lock() else {
                return refusal(503, "journal_unavailable", Some(5));
            };
            if journal_probe(&config.state_dir.join("journal/ready")).is_err() {
                return refusal(503, "journal_unavailable", Some(5));
            }
            let node = client.handshake().node();
            json_response(
                200,
                &serde_json::json!({
                    "ready": true,
                    "network_id": node.network_id.to_string(),
                    "wire_version": WIRE_VERSION,
                    "synchronous_receipts": true,
                    "state_snapshot": true,
                }),
            )
        }
        Err(error) => {
            eprintln!("layerx-core-boundary: readiness: {error}");
            refusal(503, "node_unavailable", Some(5))
        }
    }
}

fn sequencer_route(config: &Config) -> Response {
    match connect_client(config) {
        Ok(client) => {
            let node = client.handshake().node();
            success(&serde_json::json!({
                "network_id": node.network_id,
                "sequencer_public_key": hex_encode(&node.authorised_sequencer_key),
                "chain_head_sequence": node.chain_head_sequence,
                "latest_sealed_batch": node.latest_sealed_batch,
            }))
        }
        Err(error) => {
            eprintln!("layerx-core-boundary: {error}");
            refusal(503, "node_unavailable", Some(5))
        }
    }
}

fn read_relay_response(stream: &mut TcpStream) -> Result<(u16, Vec<u8>), String> {
    let mut bytes = Vec::with_capacity(4096);
    let mut chunk = [0_u8; 4096];
    loop {
        let count = stream.read(&mut chunk).map_err(|error| error.to_string())?;
        if count == 0 {
            break;
        }
        if bytes.len().saturating_add(count) > MAX_RELAY_BYTES {
            return Err("node response exceeds its bound".to_owned());
        }
        bytes.extend_from_slice(&chunk[..count]);
    }
    let header_end = bytes
        .windows(4)
        .position(|window| window == b"\r\n\r\n")
        .ok_or_else(|| "node response has no header terminator".to_owned())?;
    let head = std::str::from_utf8(&bytes[..header_end])
        .map_err(|_| "node response headers are not UTF-8".to_owned())?;
    let status = head
        .split_whitespace()
        .nth(1)
        .and_then(|value| value.parse::<u16>().ok())
        .ok_or_else(|| "node response status is invalid".to_owned())?;
    let mut body = bytes[header_end + 4..].to_vec();
    for line in head.split("\r\n").skip(1) {
        if let Some((name, value)) = line.split_once(':') {
            if name.trim().eq_ignore_ascii_case("content-length") {
                let length = value
                    .trim()
                    .parse::<usize>()
                    .map_err(|_| "node content length is invalid".to_owned())?;
                if length > body.len() {
                    return Err("node response body is truncated".to_owned());
                }
                body.truncate(length);
            }
        }
    }
    Ok((status, body))
}

fn node_get(node: &NodeEndpoint, token: &str, target: &str) -> Result<(u16, Vec<u8>), String> {
    TcpStream::connect((node.host.as_str(), node.port))
        .and_then(|mut stream| {
            stream.set_read_timeout(Some(IO_TIMEOUT))?;
            stream.set_write_timeout(Some(IO_TIMEOUT))?;
            write!(stream, "GET {target} HTTP/1.1\r\nHost: {}:{}\r\nAuthorization: Bearer {token}\r\nAccept: application/json\r\nConnection: close\r\n\r\n", node.host, node.port)?;
            Ok(stream)
        })
        .map_err(|error| error.to_string())
        .and_then(|mut stream| read_relay_response(&mut stream))
}

fn relay_route(config: &Config, request: &Request) -> Response {
    let target = request.query.as_ref().map_or_else(
        || request.path.clone(),
        |query| format!("{}?{query}", request.path),
    );
    let relayed = node_get(&config.node, &config.node_token, &target);
    match relayed {
        Ok((status @ (200 | 404 | 503), body)) => {
            let Ok(body) = String::from_utf8(body) else {
                return refusal(503, "node_unavailable", Some(5));
            };
            if serde_json::from_str::<serde_json::Value>(&body).is_err() {
                return refusal(503, "node_unavailable", Some(5));
            }
            Response {
                status,
                body,
                retry_after: None,
            }
        }
        Ok((status, _)) => {
            eprintln!("layerx-core-boundary: node answered {status} for {target}");
            refusal(503, "node_unavailable", Some(5))
        }
        Err(error) => {
            eprintln!("layerx-core-boundary: relay {target}: {error}");
            refusal(503, "node_unavailable", Some(5))
        }
    }
}

fn wrapped_relay_route(config: &Config, request: &Request) -> Response {
    let response = relay_route(config, request);
    if response.status == 200 {
        serde_json::from_str(&response.body).map_or_else(
            |_| refusal(503, "node_unavailable", Some(5)),
            |value| success(&value),
        )
    } else {
        response
    }
}

fn is_hex64(value: &str) -> bool {
    value.len() == 64 && value.bytes().all(|byte| byte.is_ascii_hexdigit())
}

fn relay_target(path: &str) -> bool {
    let segments: Vec<&str> = path.trim_start_matches('/').split('/').collect();
    match segments.as_slice() {
        ["v1", "protocol", "account-state", "head"]
        | ["v1", "programs", "account-state", "changes"] => true,
        ["v1", "receipts" | "programs", id, "account-state"]
        | ["v1", "batches", id, "receipt-authority"] => is_hex64(id),
        _ => false,
    }
}

fn unavailable_capability(path: &str) -> bool {
    path == "/v1/accounts"
        || path == "/v1/programs/registry"
        || path.starts_with("/v1/programs/registry/")
        || path.starts_with("/v1/programs/activities/")
        || path.starts_with("/v1/programs/receipts/by-idempotency/")
}

fn core_route(config: &Config, request: &Request) -> Response {
    public_reads::route(config, request).unwrap_or_else(|| protocol_route(config, request))
}

fn protocol_route(config: &Config, request: &Request) -> Response {
    let method = request.method.as_str();
    let path = request.path.as_str();
    if let Some(key) = path.strip_prefix("/v1/programs/receipts/by-idempotency/") {
        return if method != "GET" {
            refusal(405, "method_not_allowed", None)
        } else if request.query.is_some() {
            refusal(400, "invalid_request", None)
        } else {
            program_idempotency_receipt(config, key)
        };
    }
    if unavailable_capability(path) {
        return refusal(503, "capability_unavailable", Some(3600));
    }
    if relay_target(path) {
        return if method == "GET" {
            relay_route(config, request)
        } else {
            refusal(405, "method_not_allowed", None)
        };
    }
    if request.query.is_some() {
        return refusal(400, "invalid_request", None);
    }
    match (method, path) {
        ("GET", "/livez") => json_response(200, &serde_json::json!({ "live": true })),
        ("GET", "/readyz") => readiness(config),
        ("GET", "/v1/sequencer") => sequencer_route(config),
        (
            "POST",
            "/v1/activities"
            | "/v1/programs/call"
            | "/v1/programs/deploy"
            | "/v1/programs/upgrade"
            | "/v1/programs/wind-down",
        ) => {
            if let Some(ordinal) = program_lifecycle::ordinal(path) {
                if request.headers.get("content-type").map(String::as_str)
                    != Some("application/octet-stream")
                {
                    return refusal(415, "activity_content_type_required", None);
                }
                let Some(key) = request.headers.get("idempotency-key") else {
                    return refusal(400, "idempotency_key_required", None);
                };
                let Ok(registry) = submission_registry() else {
                    return refusal(503, "registry_unavailable", Some(5));
                };
                let Ok(activity) = layerx_wire::activity::decode_signed(&request.body, &registry)
                else {
                    return refusal(400, "invalid_activity", None);
                };
                if activity.activity_type().module() != ModuleId::Programs
                    || activity.activity_type().ordinal() != ordinal
                    || activity.protocol_version() != 3
                {
                    return refusal(400, "program_route_mismatch", None);
                }
                if program_lifecycle::validate(&request.body, &registry, ordinal).is_err() {
                    return refusal(400, "invalid_program_lifecycle", None);
                }
                if key != &hex_encode(&activity.idempotency_key()) {
                    return refusal(409, "protocol_idempotency_mismatch", None);
                }
            }
            stateful(config, "activities", request, || {
                activities_route(config, request)
            })
        }
        ("POST", "/v1/programs/simulate") => simulate_route(config, request),
        ("GET", "/v1/state") => wrapped_relay_route(
            config,
            &Request {
                method: "GET".to_owned(),
                path: "/v1/protocol/account-state/head".to_owned(),
                query: None,
                headers: BTreeMap::new(),
                body: Vec::new(),
            },
        ),
        ("GET", other) if other.starts_with("/v1/receipts/") => {
            receipt_route(config, &other["/v1/receipts/".len()..])
        }
        (
            _,
            "/livez"
            | "/readyz"
            | "/v1/sequencer"
            | "/v1/activities"
            | "/v1/programs/call"
            | "/v1/programs/deploy"
            | "/v1/programs/upgrade"
            | "/v1/programs/wind-down"
            | "/v1/programs/simulate"
            | "/v1/state",
        ) => refusal(405, "method_not_allowed", None),
        _ => refusal(404, "not_found", None),
    }
}

fn valid_key(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 128
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.' | b':'))
}

fn journal_path(config: &Config, scope: &str, key: &str) -> PathBuf {
    let mut digest = Sha256::new();
    digest.update(scope.as_bytes());
    digest.update([0]);
    digest.update(key.as_bytes());
    config
        .state_dir
        .join("journal")
        .join(format!("{}.json", hex_encode(&digest.finalize())))
}

fn request_digest(request: &Request) -> String {
    let mut digest = Sha256::new();
    digest.update(request.method.as_bytes());
    digest.update([0]);
    digest.update(request.path.as_bytes());
    digest.update([0]);
    digest.update(&request.body);
    hex_encode(&digest.finalize())
}

fn journal_read(path: &Path) -> Result<Option<JournalEntry>, String> {
    match fs::read(path) {
        Ok(bytes) => {
            if bytes.len() as u64 > MAX_JOURNAL_BYTES {
                return Err("journal entry exceeds its bound".to_owned());
            }
            if let Ok(entry) = serde_json::from_slice::<JournalEntry>(&bytes) {
                return Ok(Some(entry));
            }
            let mut latest = None;
            for record in bytes.split_inclusive(|byte| *byte == b'\n') {
                if !record.ends_with(b"\n") {
                    break;
                }
                let record = &record[..record.len() - 1];
                if record.is_empty() {
                    return Err("journal entry is corrupt: empty record".to_owned());
                }
                latest = Some(
                    serde_json::from_slice::<JournalEntry>(record)
                        .map_err(|error| format!("journal entry is corrupt: {error}"))?,
                );
            }
            latest
                .map(Some)
                .ok_or_else(|| "journal entry is corrupt: no complete record".to_owned())
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(error.to_string()),
    }
}

fn journal_write(path: &Path, entry: &JournalEntry) -> Result<(), String> {
    let mut bytes = serde_json::to_vec(entry).map_err(|error| error.to_string())?;
    bytes.push(b'\n');
    let mut file = fs::OpenOptions::new()
        .read(true)
        .create(true)
        .append(true)
        .mode(0o600)
        .open(path)
        .map_err(|error| error.to_string())?;
    let mut prior = Vec::new();
    file.read_to_end(&mut prior)
        .map_err(|error| error.to_string())?;
    if prior.len() as u64 > MAX_JOURNAL_BYTES {
        return Err("journal entry exceeds its bound".to_owned());
    }
    if !prior.is_empty() && !prior.ends_with(b"\n") {
        if serde_json::from_slice::<JournalEntry>(&prior).is_ok() {
            bytes.insert(0, b'\n');
        } else {
            let committed = prior
                .iter()
                .rposition(|byte| *byte == b'\n')
                .map_or(0, |position| position + 1);
            if committed == 0 {
                return Err("journal entry is corrupt: no complete record".to_owned());
            }
            file.set_len(committed as u64)
                .map_err(|error| error.to_string())?;
            prior.truncate(committed);
        }
    }
    let prior_len = prior.len() as u64;
    if prior_len.saturating_add(bytes.len() as u64) > MAX_JOURNAL_BYTES {
        return Err("journal entry exceeds its bound".to_owned());
    }
    file.write_all(&bytes).map_err(|error| error.to_string())?;
    file.sync_data().map_err(|error| error.to_string())?;
    if prior_len == 0 {
        let directory = path
            .parent()
            .ok_or_else(|| "journal path has no parent".to_owned())?;
        fs::File::open(directory)
            .and_then(|handle| handle.sync_all())
            .map_err(|error| error.to_string())?;
    }
    Ok(())
}

fn journal_probe(path: &Path) -> Result<(), String> {
    let mut file = fs::OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(true)
        .mode(0o600)
        .open(path)
        .map_err(|error| error.to_string())?;
    file.write_all(b"ready\n")
        .map_err(|error| error.to_string())?;
    file.sync_data().map_err(|error| error.to_string())
}

fn stateful(
    config: &Config,
    scope: &str,
    request: &Request,
    execute: impl FnOnce() -> Response,
) -> Response {
    let total_started = Instant::now();
    let timed = scope == "activities";
    let Some(key) = request.headers.get("idempotency-key") else {
        return if scope == "activities" {
            execute()
        } else {
            refusal(400, "idempotency_key_required", None)
        };
    };
    if !valid_key(key) {
        return refusal(400, "invalid_idempotency_key", None);
    }
    let lock_started = Instant::now();
    let Ok(_guard) = config.journal_lock.lock() else {
        return refusal(503, "journal_unavailable", Some(5));
    };
    if timed {
        pay_timing("core.journal.lock", lock_started);
    }
    let path = journal_path(config, scope, key);
    let digest = request_digest(request);
    let read_started = Instant::now();
    let prior = journal_read(&path);
    if timed {
        pay_timing("core.journal.read", read_started);
    }
    match prior {
        Ok(Some(entry)) if entry.request_digest == digest => {
            if (entry.status == 202 && program_lifecycle::ordinal(&request.path).is_some())
                || (scope == "fund"
                    && (entry.status == 202
                        || entry.status == 409
                        || entry.status >= 500
                        || entry.retry_after.is_some()))
            {
                let response = execute();
                if journal_write(
                    &path,
                    &JournalEntry {
                        request_digest: digest,
                        status: response.status,
                        body: response.body.clone(),
                        retry_after: response.retry_after,
                    },
                )
                .is_err()
                {
                    return refusal(503, "journal_unavailable", Some(5));
                }
                return response;
            }
            return Response {
                status: entry.status,
                body: entry.body,
                retry_after: entry.retry_after,
            };
        }
        Ok(Some(_)) => return refusal(409, "idempotency_conflict", None),
        Ok(None) => {}
        Err(error) => {
            eprintln!("layerx-core-boundary: journal: {error}");
            return refusal(503, "journal_unavailable", Some(5));
        }
    }
    stateful_execute_new(&path, digest, scope, timed, total_started, execute)
}

fn stateful_execute_new(
    path: &Path,
    digest: String,
    scope: &str,
    timed: bool,
    total_started: Instant,
    execute: impl FnOnce() -> Response,
) -> Response {
    let pending = if scope == "activities" {
        json_response(
            202,
            &serde_json::json!({"ok": true, "result": {"state": "pending"}}),
        )
    } else {
        refusal(409, "outcome_unknown", Some(5))
    };
    let pending_started = Instant::now();
    if journal_write(
        path,
        &JournalEntry {
            request_digest: digest.clone(),
            status: pending.status,
            body: pending.body,
            retry_after: pending.retry_after,
        },
    )
    .is_err()
    {
        return refusal(503, "journal_unavailable", Some(5));
    }
    if timed {
        pay_timing("core.journal.pending_commit", pending_started);
    }
    let execute_started = Instant::now();
    let response = execute();
    if timed {
        pay_timing("core.journal.execute", execute_started);
    }
    let final_started = Instant::now();
    if let Err(error) = journal_write(
        path,
        &JournalEntry {
            request_digest: digest,
            status: response.status,
            body: response.body.clone(),
            retry_after: response.retry_after,
        },
    ) {
        eprintln!("layerx-core-boundary: journal: {error}");
        return refusal(503, "journal_unavailable", Some(5));
    }
    if timed {
        pay_timing("core.journal.final_commit", final_started);
        pay_timing("core.journal.total", total_started);
    }
    response
}

fn admin_authorized(config: &Config, request: &Request) -> bool {
    request
        .headers
        .get("authorization")
        .and_then(|value| value.strip_prefix("Bearer "))
        .is_some_and(|token| {
            token.len() == config.admin_token.len()
                && token.as_bytes().ct_eq(config.admin_token.as_bytes()).into()
        })
}

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |elapsed| {
            u64::try_from(elapsed.as_millis()).unwrap_or(u64::MAX)
        })
}

fn send_idempotency(key: &str) -> [u8; 32] {
    let mut digest = Sha256::new();
    digest.update(b"layerx-core-fund\0");
    digest.update(key.as_bytes());
    digest.finalize().into()
}

fn send_refusal(error: SendError) -> Response {
    match error {
        SendError::Signer(reason) => {
            eprintln!("layerx-core-boundary: treasury signer: {reason}");
            refusal(503, "treasury_signer_unavailable", Some(5))
        }
        SendError::Invalid(reason) => {
            eprintln!("layerx-core-boundary: send construction: {reason}");
            refusal(422, "send_unbuildable", None)
        }
    }
}

fn treasury_sequence(config: &Config, client: &mut Client, amount: u128) -> Result<u64, Response> {
    let treasury = main_account(&config.treasury_did)
        .map_err(|_| refusal(503, "treasury_unavailable", Some(60)))?;
    let authorization = SequencerAuthorization::new(
        config.sequencer_id,
        client.handshake().node().authorised_sequencer_key,
        1,
        u64::MAX,
    );
    let value = client
        .account(treasury, VerificationLevel::STATE_PROVEN, 2, authorization)
        .map_err(|error| match error {
            ReadError::CoreRefusal { class, result } => {
                eprintln!(
                    "layerx-core-boundary: treasury read refused class {class} result {}",
                    result.raw()
                );
                refusal(422, "treasury_account_unavailable", Some(60))
            }
            ReadError::UnavailableCapability => refusal(503, "capability_unavailable", Some(30)),
            other => {
                eprintln!("layerx-core-boundary: treasury read failed: {other:?}");
                refusal(422, "treasury_account_unavailable", Some(60))
            }
        })?;
    let account = decode_account_value(treasury, value.canonical_bytes())
        .map_err(|_| refusal(422, "treasury_account_unavailable", Some(60)))?;
    let holds_amount = account
        .asset
        .is_some_and(|held| held.asset_id == config.treasury_asset && held.balance >= amount);
    if !holds_amount {
        return Err(refusal(422, "insufficient_treasury_balance", Some(60)));
    }
    Ok(account.next_sequence)
}

fn fund(config: &Config, request: &Request, key: &str) -> Response {
    let Ok(command) = serde_json::from_slice::<FundingCommand>(&request.body) else {
        return refusal(400, "invalid_argument", None);
    };
    if !valid_key(&command.funding_id)
        || !command.did.starts_with("did:")
        || command.did.len() > 512
        || !is_hex64(&command.public_key)
        || command.did != format!("did:layerx:{}", command.public_key.to_ascii_lowercase())
        || command.amount == 0
        || command.did == config.treasury_did
        || main_account(&command.did).is_err()
    {
        return refusal(400, "invalid_argument", None);
    }
    match fund_send(config, &command, key) {
        Ok(response) | Err(response) => response,
    }
}

fn fund_send(config: &Config, command: &FundingCommand, key: &str) -> Result<Response, Response> {
    let mut client = connect_client(config).map_err(|error| {
        eprintln!("layerx-core-boundary: {error}");
        refusal(503, "node_unavailable", Some(5))
    })?;
    let stage_path = journal_path(config, "fund-canonical", key);
    let command_bytes =
        serde_json::to_vec(command).map_err(|_| refusal(503, "journal_unavailable", Some(5)))?;
    let command_digest = hex_encode(&Sha256::digest(&command_bytes));
    let staged =
        journal_read(&stage_path).map_err(|_| refusal(503, "journal_unavailable", Some(5)))?;
    let signed = if let Some(entry) = staged {
        if entry.request_digest != command_digest {
            return Err(refusal(409, "idempotency_conflict", None));
        }
        serde_json::from_str::<PreparedFunding>(&entry.body)
            .map_err(|_| refusal(503, "journal_unavailable", Some(5)))?
    } else {
        let amount = u128::from(command.amount);
        let actor = layerx_types::ids::Did::new(config.treasury_did.as_bytes())
            .map_err(|_| refusal(503, "treasury_unavailable", Some(60)))?;
        let identity_sequence = client
            .preparation_state(&actor, 3)
            .map_err(|error| {
                eprintln!("layerx-core-boundary: treasury preparation failed: {error:?}");
                refusal(503, "treasury_identity_unavailable", Some(5))
            })?
            .account_sequence;
        let sequence = treasury_sequence(config, &mut client, amount)?;
        let now = now_ms();
        let signed = build_send_with_signer(
            &config.treasury,
            identity_sequence,
            &SendRequest {
                network_id: config.network_id,
                source_did: config.treasury_did.clone(),
                destination_did: command.did.clone(),
                asset: config.treasury_asset,
                amount,
                account_sequence: sequence,
                idempotency_key: send_idempotency(key),
                not_before_ms: now.saturating_sub(60_000),
                expires_at_ms: now.saturating_add(300_000),
                fee_limit: config.fee_limit,
            },
        )
        .map_err(send_refusal)?;
        let staged = PreparedFunding {
            canonical: signed.canonical,
            activity_id: signed.activity_id,
            signer_public_key: signed.signer_public_key,
        };
        journal_write(
            &stage_path,
            &JournalEntry {
                request_digest: command_digest,
                status: 202,
                body: serde_json::to_string(&staged)
                    .map_err(|_| refusal(503, "journal_unavailable", Some(5)))?,
                retry_after: Some(5),
            },
        )
        .map_err(|_| refusal(503, "journal_unavailable", Some(5)))?;
        staged
    };
    if let Ok(Some(facts)) = await_receipt(config, signed.activity_id, Duration::ZERO) {
        return funding_receipt_response(command, &facts);
    }
    let (registry, _) =
        asset_registry().map_err(|_| refusal(503, "registry_unavailable", Some(5)))?;
    let submission = client
        .submit_signed(&registry, signed.signer_public_key, 3, 1, &signed.canonical)
        .map_err(|error| match error {
            SubmitError::CoreRefusal { class, result } => {
                eprintln!(
                    "layerx-core-boundary: funding submission refused class {class} result {}",
                    result.raw()
                );
                refusal(422, "submission_refused", None)
            }
            SubmitError::UnavailableCapability => refusal(503, "capability_unavailable", Some(30)),
            SubmitError::Disconnected => refusal(503, "node_unavailable", Some(5)),
            other => {
                eprintln!("layerx-core-boundary: funding submission failed: {other:?}");
                refusal(422, "send_unbuildable", None)
            }
        })?;
    drop(client);
    let activity_id = match submission {
        Submission::Acknowledged(acknowledgement) => acknowledgement.activity_id(),
        Submission::Unknown(unknown) => unknown.activity_id(),
    };
    let transaction_id = hex_encode(&activity_id);
    match await_receipt(config, activity_id, config.receipt_deadline) {
        Ok(Some(facts)) => funding_receipt_response(command, &facts),
        Ok(None) => Ok(json_response(
            202,
            &serde_json::json!({
                "funding_id": command.funding_id,
                "state": "pending",
                "transaction_id": transaction_id,
            }),
        )),
        Err(error) => {
            eprintln!("layerx-core-boundary: {error}");
            Err(refusal(503, "receipt_unavailable", Some(5)))
        }
    }
}

fn funding_receipt_response(
    command: &FundingCommand,
    facts: &ReceiptFacts,
) -> Result<Response, Response> {
    if facts.result_code != 0 {
        return Err(refusal(422, "send_refused", None));
    }
    Ok(json_response(
        200,
        &serde_json::json!({
            "funding_id": command.funding_id,
            "state": "funded",
            "transaction_id": hex_encode(&facts.activity_id),
        }),
    ))
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct SupervisorReply {
    state: Option<String>,
    reset_id: Option<String>,
    error: Option<SupervisorError>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct SupervisorError {
    code: String,
    retry: String,
    retry_after_seconds: Option<u64>,
}

fn reset(config: &Config) -> Response {
    let outcome = UnixStream::connect(&config.supervisor_socket)
        .and_then(|mut stream| {
            stream.set_read_timeout(Some(RESET_TIMEOUT))?;
            stream.set_write_timeout(Some(IO_TIMEOUT))?;
            stream.write_all(b"reset\n")?;
            stream.flush()?;
            let mut line = String::new();
            BufReader::new(stream).take(4096).read_line(&mut line)?;
            Ok(line)
        })
        .map_err(|error| error.to_string());
    let line = match outcome {
        Ok(line) => line,
        Err(error) => {
            eprintln!("layerx-core-boundary: supervisor: {error}");
            return refusal(503, "supervisor_unavailable", Some(30));
        }
    };
    let answer = line.trim_end_matches(['\r', '\n']);
    match serde_json::from_str::<SupervisorReply>(answer) {
        Ok(SupervisorReply {
            state: Some(state),
            reset_id: Some(reset_id),
            error: None,
        }) if state == "reset" && valid_key(&reset_id) => json_response(
            200,
            &serde_json::json!({ "state": "reset", "reset_id": reset_id }),
        ),
        Ok(SupervisorReply {
            state: None,
            reset_id: None,
            error: Some(error),
        }) if valid_key(&error.code) => {
            eprintln!(
                "layerx-core-boundary: supervisor refused the reset: {}",
                error.code
            );
            let retry_after = if error.retry == "after" {
                Some(error.retry_after_seconds.unwrap_or(30))
            } else {
                None
            };
            refusal(503, &error.code, retry_after)
        }
        _ => {
            eprintln!("layerx-core-boundary: supervisor answered {answer:?}");
            refusal(503, "reset_failed", Some(30))
        }
    }
}

fn admin_result(mut response: Response) -> Response {
    if response.status >= 500 {
        response.status = 422;
    }
    response
}

fn admin_route(config: &Config, request: &Request) -> Response {
    match (request.method.as_str(), request.path.as_str()) {
        ("GET", "/livez") => return json_response(200, &serde_json::json!({ "live": true })),
        ("GET", "/readyz") => return readiness(config),
        _ => {}
    }
    if !admin_authorized(config, request) {
        return refusal(401, "unauthorized", None);
    }
    if request.query.is_some() {
        return refusal(400, "invalid_request", None);
    }
    if request.method != "POST" {
        return match request.path.as_str() {
            "/admin/v1/testnet/fund" | "/admin/v1/testnet/reset" => {
                refusal(405, "method_not_allowed", None)
            }
            _ => refusal(404, "not_found", None),
        };
    }
    if request.headers.get("content-type").map(String::as_str) != Some("application/json") {
        return refusal(400, "content_type_required", None);
    }
    let Some(key) = request.headers.get("idempotency-key").cloned() else {
        return refusal(400, "idempotency_key_required", None);
    };
    if !valid_key(&key) {
        return refusal(400, "invalid_idempotency_key", None);
    }
    let Ok(_guard) = config.admin_lock.lock() else {
        return refusal(503, "admin_unavailable", Some(5));
    };
    match request.path.as_str() {
        "/admin/v1/testnet/fund" => stateful(config, "fund", request, || {
            admin_result(fund(config, request, &key))
        }),
        "/admin/v1/testnet/reset" => {
            if request.body != b"{}" {
                return refusal(400, "invalid_argument", None);
            }
            stateful(config, "reset", request, || admin_result(reset(config)))
        }
        _ => refusal(404, "not_found", None),
    }
}

fn handle_connection(config: &Arc<Config>, plane: Plane, tcp: TcpStream) -> Result<(), String> {
    tcp.set_nodelay(true).map_err(|error| error.to_string())?;
    tcp.set_read_timeout(Some(IO_TIMEOUT))
        .map_err(|error| error.to_string())?;
    tcp.set_write_timeout(Some(IO_TIMEOUT))
        .map_err(|error| error.to_string())?;
    let tls = match plane {
        Plane::Core => &config.tls,
        Plane::Admin => &config.admin_tls,
    };
    let connection = ServerConnection::new(Arc::clone(tls)).map_err(|error| error.to_string())?;
    let mut stream = StreamOwned::new(connection, tcp);
    for request_number in 0..MAX_REQUESTS_PER_CONNECTION {
        let request = match parse_client_request(&mut stream) {
            Ok(request) => request,
            Err(_) if request_number == 0 => {
                write_response(&mut stream, &refusal(400, "invalid_request", None), false)?;
                break;
            }
            Err(_) => break,
        };
        let keep_alive = request_number + 1 < MAX_REQUESTS_PER_CONNECTION
            && request
                .headers
                .get("connection")
                .is_none_or(|value| !value.eq_ignore_ascii_case("close"));
        let response = match plane {
            Plane::Core => core_route(config, &request),
            Plane::Admin => {
                let mut response = admin_route(config, &request);
                if response.status >= 500 && request.path != "/readyz" && request.path != "/livez" {
                    response.status = 422;
                }
                response
            }
        };
        write_response(&mut stream, &response, keep_alive)?;
        stream.flush().map_err(|error| error.to_string())?;
        if !keep_alive {
            break;
        }
    }
    stream.conn.send_close_notify();
    let _ = stream.flush();
    Ok(())
}

struct ConnectionPermit;

impl ConnectionPermit {
    fn acquire() -> Option<Self> {
        ACTIVE_CONNECTIONS
            .fetch_update(Ordering::AcqRel, Ordering::Acquire, |active| {
                (active < MAX_CONNECTIONS).then_some(active + 1)
            })
            .ok()
            .map(|_| Self)
    }
}

impl Drop for ConnectionPermit {
    fn drop(&mut self) {
        ACTIVE_CONNECTIONS.fetch_sub(1, Ordering::AcqRel);
    }
}

fn serve(config: &Arc<Config>, plane: Plane, listener: &TcpListener) {
    for connection in listener.incoming() {
        match connection {
            Ok(stream) => {
                let Some(permit) = ConnectionPermit::acquire() else {
                    continue;
                };
                let shared = Arc::clone(config);
                thread::spawn(move || {
                    let _permit = permit;
                    if let Err(error) = handle_connection(&shared, plane, stream) {
                        eprintln!("layerx-core-boundary connection failed: {error}");
                    }
                });
            }
            Err(error) => eprintln!("layerx-core-boundary accept failed: {error}"),
        }
    }
}

fn platform_core(config: Config) -> Result<(), String> {
    let listener = TcpListener::bind(config.listen).map_err(|error| error.to_string())?;
    let admin = TcpListener::bind(config.admin_listen).map_err(|error| error.to_string())?;
    let config = Arc::new(config);
    eprintln!("layerx-core-boundary listening with TLS on the core and admin planes");
    let admin_config = Arc::clone(&config);
    let admin_thread = thread::spawn(move || serve(&admin_config, Plane::Admin, &admin));
    serve(&config, Plane::Core, &listener);
    admin_thread
        .join()
        .map_err(|_| "admin listener thread panicked".to_owned())
}

fn main() {
    if let Err(error) = config().and_then(platform_core) {
        eprintln!("layerx-core-boundary: {error}");
        std::process::exit(2);
    }
}

#[cfg(test)]
mod admission_tests {
    use super::{
        submission_decode_registry, submission_registry, validate_submission_payload,
        SubmissionValidationError, ASSET_SUBMISSION_ORDINALS, PROGRAM_SUBMISSION_ORDINALS,
    };
    use ed25519_dalek::{Signer, SigningKey};
    use layerx_platform_core::{build_send, treasury_did, SendRequest};
    use layerx_types::activity::{Authority, EnvelopeBuilder, Signature, TimestampBound};
    use layerx_types::amount::Amount;
    use layerx_types::ids::{Did, IdempotencyKey};
    use layerx_types::payload::{ActivityType, ModuleId, Payload};

    const ACTOR: &str = "did:layerx:alice";
    const ASSET_FIXTURES: [(u16, &str); 7] = [
        (
            1,
            include_str!("../../../../agent/crates/layerx-crypto/tests/fixtures/payments/1-1.hex"),
        ),
        (
            4,
            include_str!("../../../../agent/crates/layerx-crypto/tests/fixtures/payments/1-4.hex"),
        ),
        (
            6,
            include_str!("../../../../agent/crates/layerx-crypto/tests/fixtures/payments/1-6.hex"),
        ),
        (
            7,
            include_str!("../../../../agent/crates/layerx-crypto/tests/fixtures/payments/1-7.hex"),
        ),
        (
            8,
            include_str!("../../../../agent/crates/layerx-crypto/tests/fixtures/payments/1-8.hex"),
        ),
        (
            10,
            include_str!("../../../../agent/crates/layerx-crypto/tests/fixtures/payments/1-10.hex"),
        ),
        (
            11,
            include_str!("../../../../agent/crates/layerx-crypto/tests/fixtures/payments/1-11.hex"),
        ),
    ];
    const PROGRAM_PAYMENT_FIXTURES: [(u16, &str); 2] = [
        (
            5,
            include_str!("../../../../agent/crates/layerx-crypto/tests/fixtures/payments/9-5.hex"),
        ),
        (
            6,
            include_str!("../../../../agent/crates/layerx-crypto/tests/fixtures/payments/9-6.hex"),
        ),
    ];

    fn required<T, E: std::fmt::Debug>(result: Result<T, E>) -> T {
        result.unwrap_or_else(|error| panic!("{error:?}"))
    }

    fn hex(value: &str) -> Vec<u8> {
        value
            .trim()
            .as_bytes()
            .chunks_exact(2)
            .map(|pair| {
                let digits = std::str::from_utf8(pair).unwrap_or_else(|error| panic!("{error}"));
                u8::from_str_radix(digits, 16).unwrap_or_else(|error| panic!("{error}"))
            })
            .collect()
    }

    fn signed(module: ModuleId, ordinal: u16, payload_bytes: &[u8], actor: &str) -> Vec<u8> {
        let key = SigningKey::from_bytes(&[42; 32]);
        let registry = required(submission_decode_registry());
        let activity_type = required(ActivityType::new(module, ordinal));
        let payload = required(Payload::new(&registry, activity_type, payload_bytes));
        let payload_hash = required(layerx_wire::hash::payload_hash_for(&payload));
        let mut builder = EnvelopeBuilder::new();
        required(
            builder
                .protocol_version(layerx_wire::limits::STATE_COMMITMENT_PROTOCOL_VERSION)
                .and_then(|value| value.network_id(17))
                .and_then(|value| value.activity_type(activity_type))
                .and_then(|value| value.actor_did(required(Did::new(actor.as_bytes()))))
                .and_then(|value| {
                    value.authority(required(Authority::owner(&key.verifying_key().to_bytes())))
                })
                .and_then(|value| value.account_sequence(7))
                .and_then(|value| value.timestamp_bound(required(TimestampBound::new(1, u64::MAX))))
                .and_then(|value| value.idempotency_key(IdempotencyKey::new([0x71; 32])))
                .and_then(|value| value.fee_limit(Amount::from_u128(1_000_000)))
                .and_then(|value| value.payload_hash(payload_hash))
                .and_then(|value| value.payload(payload)),
        );
        let unsigned = required(builder.build());
        let preimage = required(layerx_wire::sign::preimage_unsigned(&unsigned));
        let signature = key.sign(preimage.as_bytes()).to_bytes();
        required(layerx_wire::activity::encode_signed_envelope(
            &unsigned.attach_signature(required(Signature::new(&signature))),
        ))
    }

    fn validate(canonical: &[u8]) -> Result<(), SubmissionValidationError> {
        let registry = required(submission_decode_registry());
        let activity = required(layerx_wire::activity::decode_signed(canonical, &registry));
        validate_submission_payload(canonical, &activity, &registry)
    }

    #[test]
    fn submission_registry_declares_only_executable_asset_and_program_types() {
        let registry = required(submission_registry());
        for ordinal in ASSET_SUBMISSION_ORDINALS {
            assert!(registry.declares(required(ActivityType::new(ModuleId::Asset, ordinal))));
        }
        for ordinal in PROGRAM_SUBMISSION_ORDINALS {
            assert!(registry.declares(required(ActivityType::new(ModuleId::Programs, ordinal))));
        }
        for ordinal in [9, 12] {
            assert!(!registry.declares(required(ActivityType::new(ModuleId::Asset, ordinal))));
        }
    }

    #[test]
    fn every_native_asset_payload_is_strictly_admitted_and_malformed_bytes_are_refused() {
        for (ordinal, fixture) in ASSET_FIXTURES {
            let payload = hex(fixture);
            let canonical = signed(ModuleId::Asset, ordinal, &payload, ACTOR);
            assert_eq!(validate(&canonical), Ok(()), "Asset ordinal {ordinal}");
            let malformed = signed(
                ModuleId::Asset,
                ordinal,
                &payload[..payload.len() - 1],
                ACTOR,
            );
            assert_eq!(
                validate(&malformed),
                Err(SubmissionValidationError::InvalidAssetActivity),
                "Asset ordinal {ordinal} malformed body"
            );
        }

        let source_seed = [9; 32];
        let source_did = treasury_did(&source_seed);
        let send = required(build_send(
            &source_seed,
            &SendRequest {
                network_id: 17,
                source_did: source_did.clone(),
                destination_did: treasury_did(&[10; 32]),
                asset: [3; 32],
                amount: 1,
                account_sequence: 7,
                idempotency_key: [0x71; 32],
                not_before_ms: 1,
                expires_at_ms: u64::MAX,
                fee_limit: 1_000_000,
            },
        ));
        assert_eq!(validate(&send.canonical), Ok(()), "Asset ordinal 5");
        let registry = required(submission_decode_registry());
        let activity = required(layerx_wire::activity::decode_signed(
            &send.canonical,
            &registry,
        ));
        let malformed_payload = &activity.payload()[..activity.payload().len() - 1];
        let malformed = signed(ModuleId::Asset, 5, malformed_payload, &source_did);
        assert_eq!(
            validate(&malformed),
            Err(SubmissionValidationError::InvalidAssetActivity)
        );
    }

    #[test]
    fn asset_pause_and_unpause_are_admitted_and_malformed_bodies_are_refused() {
        let registry = required(submission_registry());
        let mut payload = vec![0x00, 0x01];
        payload.extend_from_slice(&[7; 32]);
        for ordinal in [2u16, 3u16] {
            assert!(registry.declares(required(ActivityType::new(ModuleId::Asset, ordinal))));
            assert_eq!(
                validate(&signed(ModuleId::Asset, ordinal, &payload, ACTOR)),
                Ok(()),
                "Asset ordinal {ordinal}"
            );
            assert_eq!(
                validate(&signed(
                    ModuleId::Asset,
                    ordinal,
                    &payload[..payload.len() - 1],
                    ACTOR,
                )),
                Err(SubmissionValidationError::InvalidAssetActivity),
                "Asset ordinal {ordinal} truncated body"
            );
            let mut unsupported_version = payload.clone();
            unsupported_version[1] = 2;
            assert_eq!(
                validate(&signed(
                    ModuleId::Asset,
                    ordinal,
                    &unsupported_version,
                    ACTOR
                )),
                Err(SubmissionValidationError::InvalidAssetActivity),
                "Asset ordinal {ordinal} unsupported payload version"
            );
            let mut zero_asset = payload.clone();
            zero_asset[2..].fill(0);
            assert_eq!(
                validate(&signed(ModuleId::Asset, ordinal, &zero_asset, ACTOR)),
                Err(SubmissionValidationError::InvalidAssetActivity),
                "Asset ordinal {ordinal} zero asset id"
            );
        }
    }

    #[test]
    fn reserved_asset_ordinal_and_program_payment_malformations_are_typed() {
        let reserved = signed(ModuleId::Asset, 9, &[], ACTOR);
        assert_eq!(
            validate(&reserved),
            Err(SubmissionValidationError::AssetOrdinalReserved)
        );
        for (ordinal, fixture) in PROGRAM_PAYMENT_FIXTURES {
            let payload = hex(fixture);
            assert_eq!(
                validate(&signed(ModuleId::Programs, ordinal, &payload, ACTOR)),
                Ok(())
            );
            assert_eq!(
                validate(&signed(
                    ModuleId::Programs,
                    ordinal,
                    &payload[..payload.len() - 1],
                    ACTOR,
                )),
                Err(SubmissionValidationError::InvalidProgramAccountOperation)
            );
        }
    }
}

#[cfg(test)]
mod journal_tests {
    use super::{journal_read, journal_write, JournalEntry, TRACE};
    use std::fs;
    use std::io::Write;
    use std::sync::atomic::Ordering;

    fn entry(status: u16, body: &str) -> JournalEntry {
        JournalEntry {
            request_digest: "request".to_owned(),
            status,
            body: body.to_owned(),
            retry_after: None,
        }
    }

    #[test]
    fn append_journal_recovers_last_complete_transition_and_legacy_record() {
        let path = std::env::temp_dir().join(format!(
            "layerx-core-journal-{}-{}.json",
            std::process::id(),
            TRACE.fetch_add(1, Ordering::AcqRel)
        ));
        journal_write(&path, &entry(202, "pending"))
            .unwrap_or_else(|error| panic!("pending journal: {error}"));
        journal_write(&path, &entry(200, "complete"))
            .unwrap_or_else(|error| panic!("complete journal: {error}"));
        let mut file = fs::OpenOptions::new()
            .append(true)
            .open(&path)
            .unwrap_or_else(|error| panic!("open journal: {error}"));
        file.write_all(b"{\"request_digest\":")
            .unwrap_or_else(|error| panic!("partial journal: {error}"));
        file.sync_data()
            .unwrap_or_else(|error| panic!("sync partial journal: {error}"));
        let recovered = journal_read(&path)
            .unwrap_or_else(|error| panic!("read journal: {error}"))
            .unwrap_or_else(|| panic!("journal is missing"));
        assert_eq!(recovered.status, 200);
        assert_eq!(recovered.body, "complete");
        journal_write(&path, &entry(200, "reconciled"))
            .unwrap_or_else(|error| panic!("repair torn tail: {error}"));
        let reconciled = journal_read(&path)
            .unwrap_or_else(|error| panic!("read repaired journal: {error}"))
            .unwrap_or_else(|| panic!("repaired journal is missing"));
        assert_eq!(reconciled.body, "reconciled");

        let legacy = serde_json::to_vec(&entry(202, "legacy"))
            .unwrap_or_else(|error| panic!("legacy journal: {error}"));
        fs::write(&path, legacy).unwrap_or_else(|error| panic!("write legacy journal: {error}"));
        journal_write(&path, &entry(200, "migrated"))
            .unwrap_or_else(|error| panic!("migrate journal: {error}"));
        let migrated = journal_read(&path)
            .unwrap_or_else(|error| panic!("read migrated journal: {error}"))
            .unwrap_or_else(|| panic!("migrated journal is missing"));
        assert_eq!(migrated.status, 200);
        assert_eq!(migrated.body, "migrated");
        fs::remove_file(&path).unwrap_or_else(|error| panic!("remove journal: {error}"));
    }
}

#[cfg(test)]
mod fund_refusal_tests {
    use super::{admin_result, send_refusal, Response};
    use layerx_platform_core::SendError;

    fn code(response: &Response) -> String {
        let document: serde_json::Value = serde_json::from_str(&response.body)
            .unwrap_or_else(|error| panic!("refusal body {}: {error}", response.body));
        document["error"]["code"]
            .as_str()
            .unwrap_or_else(|| panic!("refusal body {}", response.body))
            .to_owned()
    }

    #[test]
    fn a_treasury_signer_outage_is_a_retryable_refusal_the_admin_plane_delivers_as_422() {
        let refused = send_refusal(SendError::Signer(
            "treasury signer socket /run/layerx/node/treasury-signer.sock is not available"
                .to_owned(),
        ));
        assert_eq!(refused.status, 503);
        assert_eq!(refused.retry_after, Some(5));
        assert_eq!(code(&refused), "treasury_signer_unavailable");
        let delivered = admin_result(refused);
        assert_eq!(
            delivered.status, 422,
            "every admin 5xx is delivered as 422: {}",
            delivered.body
        );
        assert_eq!(delivered.retry_after, Some(5));
        assert_eq!(code(&delivered), "treasury_signer_unavailable");
    }

    #[test]
    fn an_unbuildable_send_is_a_terminal_refusal_on_both_planes() {
        let refused = send_refusal(SendError::Invalid(
            "amount exceeds the fee limit".to_owned(),
        ));
        assert_eq!(refused.status, 422);
        assert_eq!(refused.retry_after, None);
        assert_eq!(code(&refused), "send_unbuildable");
        let delivered = admin_result(refused);
        assert_eq!(delivered.status, 422);
        assert_eq!(delivered.retry_after, None);
        assert_eq!(code(&delivered), "send_unbuildable");
    }
}
