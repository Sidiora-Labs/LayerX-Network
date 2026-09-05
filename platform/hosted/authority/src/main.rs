use layerx_client::lni::handshake::{perform, HandshakeConfig};
use layerx_client::lni::refusal::decode_core_refusal;
use layerx_client::lni::schema::{decode_envelope, encode_envelope, Capability, Envelope, Version};
use layerx_client::lni::transport::{ConnectionGate, FrameTransport, Limits, Uds};
use layerx_platform_authority::{
    authorized_batch_by_activity, hex, parse_replica_evidence, receipt_locator, EvidenceRefusal,
};
use layerx_proof::inclusion::SequencerAuthorization;
use layerx_wire::limits::STATE_COMMITMENT_PROTOCOL_VERSION as PROTOCOL_VERSION;
use rustls::pki_types::{CertificateDer, PrivateKeyDer};
use rustls::server::WebPkiClientVerifier;
use rustls::{RootCertStore, ServerConfig, ServerConnection, StreamOwned};
use std::collections::BTreeMap;
use std::env;
use std::fs;
use std::io::{Read, Write};
use std::net::{Ipv6Addr, SocketAddr, TcpListener, TcpStream};
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};
use std::sync::Arc;
use std::thread;
use std::time::Duration;
use subtle::ConstantTimeEq;
use zeroize::{Zeroize, Zeroizing};

const MAX_REQUEST_BYTES: usize = 16 * 1024;
const MAX_REPLICA_RESPONSE_BYTES: usize = 256 * 1024;
const MAX_TLS_FILE_BYTES: u64 = 64 * 1024;
const IO_TIMEOUT: Duration = Duration::from_secs(8);
const REPLICA_TIMEOUT: Duration = Duration::from_secs(8);
const MAX_CONNECTIONS: usize = 128;
const MAX_LNI_CONNECTIONS: usize = 16;
const LNI_FRAME_BYTES: usize = 1_212_416;
const RECEIPT_LOOKUP_REQUEST: u16 = 5;
const RECEIPT_LOOKUP_RESPONSE: u16 = 6;
const ERROR_RESPONSE: u16 = 25;
const ZERO_HEX32: &str = "0000000000000000000000000000000000000000000000000000000000000000";
static ACTIVE_CONNECTIONS: AtomicUsize = AtomicUsize::new(0);
static CORRELATION: AtomicU64 = AtomicU64::new(1);

const USAGE: &str = "layerx-receipt-authority serves verified authorised-batch facts over TLS.

Routes (GET only):
  /livez
  /readyz                                              {ready, network_id, wire_version}
  /v1/authorized-batches/by-activity/{activity_id}     bearer required
  /internal/v1/activities/{activity_id}/authority      bearer required
  /v1/batches/{batch_id}/receipt-authority?receipt_digest={digest}
                                                       bearer required, relayed to the replica unchanged

Environment:
  LAYERX_AUTHORITY_LISTEN                    listen address, default 0.0.0.0:9445
  LAYERX_AUTHORITY_TLS_CERT_DER              server certificate (DER)
  LAYERX_AUTHORITY_TLS_KEY_DER               server private key (PKCS#8 DER)
  LAYERX_AUTHORITY_CLIENT_CA_DER             optional; when set a presented client certificate must chain to it
  LAYERX_AUTHORITY_TOKEN_FILES               colon-separated files, one bearer token each (gateway, registry, webhooks)
  LAYERX_AUTHORITY_REPLICA_URL               loopback http://127.0.0.1:PORT of the independent receipt-authority replica
  LAYERX_AUTHORITY_REPLICA_BEARER_TOKEN_FILE bearer token the replica requires
  LAYERX_AUTHORITY_REPLICA_ID                64-hex replica identity every evidence document must carry
  LAYERX_AUTHORITY_LNI_SOCKET                LNI unix socket used as the receipt source (ReceiptLookup by activity id);
                                             this service takes receipt bytes from the LNI and never an HTTP receipt URL
  LAYERX_AUTHORITY_PROTOCOL_NETWORK_ID       numeric protocol network id expected in the LNI handshake
  LAYERX_AUTHORITY_NETWORK_ID                deployment network identifier echoed in every answer
  LAYERX_AUTHORITY_WIRE_VERSION              wire version echoed in every answer, must be the built protocol version (default 3)
  LAYERX_AUTHORITY_SEQUENCER_ID              64-hex sequencer identity pinned for header verification
  LAYERX_AUTHORITY_SEQUENCER_PUBLIC_KEY      64-hex sequencer public key pinned for header and receipt signatures
  LAYERX_AUTHORITY_FIRST_BATCH               first authorised batch number
  LAYERX_AUTHORITY_LAST_BATCH                last authorised batch number
";

struct Config {
    listen: SocketAddr,
    tls: Arc<ServerConfig>,
    tokens: Vec<Zeroizing<String>>,
    replica_address: SocketAddr,
    replica_host: String,
    replica_token: Zeroizing<String>,
    replica_id: [u8; 32],
    lni_socket: PathBuf,
    lni_gate: ConnectionGate,
    protocol_network_id: u32,
    network_id: String,
    wire_version: String,
    authorization: SequencerAuthorization,
    sequencer_public_key: [u8; 32],
}

struct Request {
    method: String,
    path: String,
    query: Option<String>,
    headers: BTreeMap<String, String>,
}

struct Response {
    status: u16,
    body: Vec<u8>,
    retry_after: Option<u64>,
}

enum ReplicaAnswer {
    Status(u16, Vec<u8>),
    Unavailable,
}

fn valid_identifier(value: &str, max: usize) -> bool {
    !value.is_empty()
        && value.len() <= max
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.'))
}

fn read_secret_file(path: &str, name: &str) -> Result<Zeroizing<String>, String> {
    let mut value = fs::read_to_string(path).map_err(|error| format!("{name}: {error}"))?;
    while matches!(value.as_bytes().last(), Some(b'\n' | b'\r')) {
        value.pop();
    }
    if value.is_empty()
        || value.len() > 4096
        || value.bytes().any(|byte| !(0x21..=0x7e).contains(&byte))
    {
        value.zeroize();
        return Err(format!(
            "{name} does not contain a bounded printable secret"
        ));
    }
    Ok(Zeroizing::new(value))
}

fn read_secret(variable: &str) -> Result<Zeroizing<String>, String> {
    let path = env::var(variable).map_err(|_| format!("{variable} is required"))?;
    read_secret_file(&path, variable)
}

fn read_bounded(variable: &str) -> Result<Vec<u8>, String> {
    let path = env::var(variable).map_err(|_| format!("{variable} is required"))?;
    let metadata = fs::metadata(&path).map_err(|error| format!("{variable}: {error}"))?;
    if !metadata.is_file() || metadata.len() == 0 || metadata.len() > MAX_TLS_FILE_BYTES {
        return Err(format!("{variable} is not a bounded regular file"));
    }
    fs::read(&path).map_err(|error| format!("{variable}: {error}"))
}

fn parse_hex32(variable: &str) -> Result<[u8; 32], String> {
    let value = env::var(variable).map_err(|_| format!("{variable} is required"))?;
    hex::decode32(&value).map_err(|_| format!("{variable} must be 64 hexadecimal characters"))
}

fn parse_u64(variable: &str) -> Result<u64, String> {
    env::var(variable)
        .map_err(|_| format!("{variable} is required"))?
        .parse::<u64>()
        .map_err(|_| format!("{variable} must be an integer"))
}

fn loopback_http(endpoint: &str) -> Option<(String, SocketAddr)> {
    let authority = endpoint.strip_prefix("http://")?;
    let authority = authority.strip_suffix('/').unwrap_or(authority);
    if authority.contains(['/', '?', '#', '@', '\\']) {
        return None;
    }
    let (host, port) = authority.rsplit_once(':')?;
    let port = port.parse::<u16>().ok().filter(|port| *port != 0)?;
    let address = match host {
        "127.0.0.1" | "localhost" => SocketAddr::from(([127, 0, 0, 1], port)),
        "[::1]" => SocketAddr::from((Ipv6Addr::LOCALHOST, port)),
        _ => return None,
    };
    Some((format!("{host}:{port}"), address))
}

fn server_tls_config() -> Result<Arc<ServerConfig>, String> {
    rustls::crypto::ring::default_provider()
        .install_default()
        .map_err(|_| "failed to install the authority TLS provider".to_owned())?;
    let certificate = CertificateDer::from(read_bounded("LAYERX_AUTHORITY_TLS_CERT_DER")?);
    let private_key = PrivateKeyDer::try_from(read_bounded("LAYERX_AUTHORITY_TLS_KEY_DER")?)
        .map_err(|_| "LAYERX_AUTHORITY_TLS_KEY_DER is not a PKCS#8 private key".to_owned())?;
    let builder = ServerConfig::builder();
    let config = if env::var_os("LAYERX_AUTHORITY_CLIENT_CA_DER").is_some() {
        let client_ca = CertificateDer::from(read_bounded("LAYERX_AUTHORITY_CLIENT_CA_DER")?);
        let mut roots = RootCertStore::empty();
        roots
            .add(client_ca)
            .map_err(|_| "LAYERX_AUTHORITY_CLIENT_CA_DER is not a CA certificate".to_owned())?;
        let verifier = WebPkiClientVerifier::builder(roots.into())
            .allow_unauthenticated()
            .build()
            .map_err(|_| "authority client certificate verifier is invalid".to_owned())?;
        builder
            .with_client_cert_verifier(verifier)
            .with_single_cert(vec![certificate], private_key)
    } else {
        builder
            .with_no_client_auth()
            .with_single_cert(vec![certificate], private_key)
    }
    .map_err(|_| "authority TLS identity is invalid".to_owned())?;
    Ok(Arc::new(config))
}

fn config() -> Result<Config, String> {
    let listen = env::var("LAYERX_AUTHORITY_LISTEN")
        .unwrap_or_else(|_| "0.0.0.0:9445".to_owned())
        .parse::<SocketAddr>()
        .map_err(|_| "LAYERX_AUTHORITY_LISTEN must be a socket address".to_owned())?;
    let tls = server_tls_config()?;
    let token_files = env::var("LAYERX_AUTHORITY_TOKEN_FILES")
        .map_err(|_| "LAYERX_AUTHORITY_TOKEN_FILES is required")?;
    let mut tokens = Vec::new();
    for path in token_files.split(':').filter(|path| !path.is_empty()) {
        tokens.push(read_secret_file(path, "LAYERX_AUTHORITY_TOKEN_FILES")?);
    }
    if tokens.is_empty() {
        return Err("LAYERX_AUTHORITY_TOKEN_FILES names no token file".to_owned());
    }
    let (replica_host, replica_address) = env::var("LAYERX_AUTHORITY_REPLICA_URL")
        .ok()
        .as_deref()
        .and_then(loopback_http)
        .ok_or_else(|| {
            "LAYERX_AUTHORITY_REPLICA_URL must be a loopback http://host:port endpoint".to_owned()
        })?;
    let replica_token = read_secret("LAYERX_AUTHORITY_REPLICA_BEARER_TOKEN_FILE")?;
    let replica_id = parse_hex32("LAYERX_AUTHORITY_REPLICA_ID")?;
    if replica_id == [0; 32] {
        return Err("LAYERX_AUTHORITY_REPLICA_ID must not be zero".to_owned());
    }
    let lni_socket = env::var("LAYERX_AUTHORITY_LNI_SOCKET")
        .map(PathBuf::from)
        .map_err(|_| "LAYERX_AUTHORITY_LNI_SOCKET is required".to_owned())?;
    if !lni_socket.is_absolute() {
        return Err("LAYERX_AUTHORITY_LNI_SOCKET must be an absolute path".to_owned());
    }
    let protocol_network_id = parse_u64("LAYERX_AUTHORITY_PROTOCOL_NETWORK_ID")?;
    let protocol_network_id = u32::try_from(protocol_network_id)
        .ok()
        .filter(|value| *value != 0)
        .ok_or_else(|| {
            "LAYERX_AUTHORITY_PROTOCOL_NETWORK_ID must be a non-zero 32-bit integer".to_owned()
        })?;
    let network_id = env::var("LAYERX_AUTHORITY_NETWORK_ID")
        .map_err(|_| "LAYERX_AUTHORITY_NETWORK_ID is required")?;
    let wire_version =
        env::var("LAYERX_AUTHORITY_WIRE_VERSION").unwrap_or_else(|_| PROTOCOL_VERSION.to_string());
    if !valid_identifier(&network_id, 64) || !valid_identifier(&wire_version, 32) {
        return Err(
            "LAYERX_AUTHORITY_NETWORK_ID or LAYERX_AUTHORITY_WIRE_VERSION is invalid".to_owned(),
        );
    }
    if wire_version.parse::<u16>().ok() != Some(PROTOCOL_VERSION) {
        return Err("LAYERX_AUTHORITY_WIRE_VERSION is not the built protocol version".to_owned());
    }
    let sequencer_id = parse_hex32("LAYERX_AUTHORITY_SEQUENCER_ID")?;
    let sequencer_public_key = parse_hex32("LAYERX_AUTHORITY_SEQUENCER_PUBLIC_KEY")?;
    if sequencer_id == [0; 32] || sequencer_public_key == [0; 32] {
        return Err("sequencer identity and public key must not be zero".to_owned());
    }
    let first_batch = parse_u64("LAYERX_AUTHORITY_FIRST_BATCH")?;
    let last_batch = parse_u64("LAYERX_AUTHORITY_LAST_BATCH")?;
    if first_batch == 0 || last_batch < first_batch {
        return Err(
            "LAYERX_AUTHORITY_FIRST_BATCH..LAYERX_AUTHORITY_LAST_BATCH is not a batch range"
                .to_owned(),
        );
    }
    Ok(Config {
        listen,
        tls,
        tokens,
        replica_address,
        replica_host,
        replica_token,
        replica_id,
        lni_socket,
        lni_gate: ConnectionGate::new(MAX_LNI_CONNECTIONS),
        protocol_network_id,
        network_id,
        wire_version,
        authorization: SequencerAuthorization::new(
            sequencer_id,
            sequencer_public_key,
            first_batch,
            last_batch,
        ),
        sequencer_public_key,
    })
}

fn read_http_message(stream: &mut impl Read) -> Result<Request, String> {
    let mut bytes = Vec::with_capacity(2048);
    let mut chunk = [0_u8; 2048];
    let header_end = loop {
        let count = stream.read(&mut chunk).map_err(|error| error.to_string())?;
        if count == 0 || bytes.len().saturating_add(count) > MAX_REQUEST_BYTES {
            return Err("HTTP message is empty or exceeds its bound".to_owned());
        }
        bytes.extend_from_slice(&chunk[..count]);
        if let Some(position) = bytes.windows(4).position(|window| window == b"\r\n\r\n") {
            break position + 4;
        }
    };
    let source = std::str::from_utf8(&bytes[..header_end])
        .map_err(|_| "HTTP headers are not UTF-8".to_owned())?;
    let mut lines = source.split("\r\n");
    let start = lines
        .next()
        .ok_or_else(|| "HTTP start line is missing".to_owned())?
        .to_owned();
    let mut headers = BTreeMap::new();
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
    if content_length != 0 {
        return Err("request bodies are not accepted".to_owned());
    }
    if bytes.len() != header_end {
        return Err("request carries unexpected bytes".to_owned());
    }
    let mut parts = start.split_whitespace();
    let method = parts
        .next()
        .ok_or_else(|| "request method is missing".to_owned())?
        .to_owned();
    let target = parts
        .next()
        .ok_or_else(|| "request target is missing".to_owned())?;
    if parts.next() != Some("HTTP/1.1") || parts.next().is_some() || !target.starts_with('/') {
        return Err("request line is invalid".to_owned());
    }
    if !headers.contains_key("host") {
        return Err("HTTP/1.1 Host header is required".to_owned());
    }
    let (path, query) = target
        .split_once('?')
        .map_or((target.to_owned(), None), |(path, query)| {
            (path.to_owned(), Some(query.to_owned()))
        });
    if path.contains('#')
        || query
            .as_deref()
            .is_some_and(|query| query.contains(['?', '#']))
    {
        return Err("request target is invalid".to_owned());
    }
    Ok(Request {
        method,
        path,
        query,
        headers,
    })
}

fn json(status: u16, value: &serde_json::Value) -> Response {
    Response {
        status,
        body: value.to_string().into_bytes(),
        retry_after: None,
    }
}

fn refusal(status: u16, code: &str, retry_after: Option<u64>) -> Response {
    let body = retry_after.map_or_else(
        || serde_json::json!({ "error": { "code": code, "retry": "never" } }),
        |seconds| {
            serde_json::json!({ "error": { "code": code, "retry": "after", "retry_after_seconds": seconds } })
        },
    );
    Response {
        status,
        body: body.to_string().into_bytes(),
        retry_after,
    }
}

fn write_response(stream: &mut impl Write, response: &Response) -> Result<(), String> {
    let reason = match response.status {
        200 => "OK",
        400 => "Bad Request",
        401 => "Unauthorized",
        404 => "Not Found",
        405 => "Method Not Allowed",
        502 => "Bad Gateway",
        _ => "Service Unavailable",
    };
    let retry = response.retry_after.map_or(String::new(), |seconds| {
        format!("Retry-After: {seconds}\r\n")
    });
    let head = format!(
        "HTTP/1.1 {} {reason}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nCache-Control: no-store\r\n{retry}Connection: close\r\n\r\n",
        response.status,
        response.body.len()
    );
    stream
        .write_all(head.as_bytes())
        .and_then(|()| stream.write_all(&response.body))
        .and_then(|()| stream.flush())
        .map_err(|error| error.to_string())
}

fn authenticate(config: &Config, request: &Request) -> Result<(), Response> {
    let Some(presented) = request
        .headers
        .get("authorization")
        .and_then(|value| value.strip_prefix("Bearer "))
    else {
        return Err(refusal(401, "identity_required", None));
    };
    if presented.is_empty() || presented.len() > 4096 {
        return Err(refusal(401, "identity_required", None));
    }
    let accepted = config.tokens.iter().any(|token| {
        token.len() == presented.len() && bool::from(token.as_bytes().ct_eq(presented.as_bytes()))
    });
    if accepted {
        Ok(())
    } else {
        Err(refusal(401, "identity_required", None))
    }
}

fn replica_get(config: &Config, path: &str) -> ReplicaAnswer {
    match replica_exchange(config, path) {
        Ok(answer) => answer,
        Err(error) => {
            eprintln!("layerx-receipt-authority replica GET failed: {error}");
            ReplicaAnswer::Unavailable
        }
    }
}

fn replica_exchange(config: &Config, path: &str) -> Result<ReplicaAnswer, String> {
    let mut stream = TcpStream::connect_timeout(&config.replica_address, REPLICA_TIMEOUT)
        .map_err(|error| format!("connect: {error}"))?;
    stream
        .set_read_timeout(Some(REPLICA_TIMEOUT))
        .and_then(|()| stream.set_write_timeout(Some(REPLICA_TIMEOUT)))
        .map_err(|error| format!("timeout: {error}"))?;
    let request = format!(
        "GET {path} HTTP/1.1\r\nHost: {}\r\nAuthorization: Bearer {}\r\nAccept: application/json\r\nConnection: close\r\n\r\n",
        config.replica_host,
        config.replica_token.as_str()
    );
    stream
        .write_all(request.as_bytes())
        .map_err(|error| format!("write: {error}"))?;
    let mut raw = Vec::new();
    let mut chunk = [0_u8; 4096];
    loop {
        let count = stream
            .read(&mut chunk)
            .map_err(|error| format!("read: {error}"))?;
        if count == 0 {
            break;
        }
        raw.extend_from_slice(&chunk[..count]);
        if raw.len() > MAX_REPLICA_RESPONSE_BYTES {
            return Err("response exceeds the replica document bound".to_owned());
        }
    }
    let head_end = raw
        .windows(4)
        .position(|window| window == b"\r\n\r\n")
        .ok_or_else(|| "response lacks a header terminator".to_owned())?;
    let head = std::str::from_utf8(&raw[..head_end])
        .map_err(|_| "response head is not UTF-8".to_owned())?;
    let mut lines = head.split("\r\n");
    let status_line = lines
        .next()
        .ok_or_else(|| "response lacks a status line".to_owned())?;
    let status = status_line
        .strip_prefix("HTTP/1.1 ")
        .or_else(|| status_line.strip_prefix("HTTP/1.0 "))
        .and_then(|rest| rest.split(' ').next())
        .and_then(|code| code.parse::<u16>().ok())
        .filter(|code| (100..=599).contains(code))
        .ok_or_else(|| "response status line is malformed".to_owned())?;
    let mut content_length = None;
    for line in lines {
        let (name, value) = line
            .split_once(':')
            .ok_or_else(|| "response header is malformed".to_owned())?;
        if name.trim().eq_ignore_ascii_case("content-length") {
            let length = value
                .trim()
                .parse::<usize>()
                .map_err(|_| "response content length is malformed".to_owned())?;
            if content_length.replace(length).is_some() {
                return Err("response repeats content length".to_owned());
            }
        }
        if name.trim().eq_ignore_ascii_case("transfer-encoding") {
            return Err("response uses a transfer encoding".to_owned());
        }
    }
    let body = raw[head_end + 4..].to_vec();
    match content_length {
        Some(length) if length == body.len() => Ok(ReplicaAnswer::Status(status, body)),
        Some(_) => Err("response body length disagrees with its header".to_owned()),
        None if body.is_empty() => Ok(ReplicaAnswer::Status(status, body)),
        None => Err("response body lacks a content length".to_owned()),
    }
}

enum ReceiptSource {
    Found(Vec<u8>),
    Unknown,
    Unavailable(String),
    KeyMismatch,
}

fn lookup_receipt(config: &Config, activity_id: [u8; 32]) -> ReceiptSource {
    let limits = Limits {
        maximum_frame_bytes: LNI_FRAME_BYTES,
        maximum_connections: MAX_LNI_CONNECTIONS,
        maximum_streams: 1,
        maximum_queued_bytes: LNI_FRAME_BYTES,
        deadline: IO_TIMEOUT,
    };
    let mut transport = match Uds::connect(&config.lni_socket, &config.lni_gate, limits) {
        Ok(transport) => transport,
        Err(error) => return ReceiptSource::Unavailable(format!("LNI connect: {error:?}")),
    };
    let expected = HandshakeConfig {
        built_interface_version: Version::V1_3,
        expected_protocol_version: PROTOCOL_VERSION,
        expected_network_id: config.protocol_network_id,
    };
    let handshake = match perform(&mut transport, &expected, None) {
        Ok(handshake) => handshake,
        Err(error) => return ReceiptSource::Unavailable(format!("LNI handshake: {error:?}")),
    };
    if !handshake.capabilities().contains(Capability::ReceiptLookup) {
        return ReceiptSource::Unavailable("LNI does not advertise receipt_lookup".to_owned());
    }
    if handshake.node().authorised_sequencer_key != config.sequencer_public_key {
        return ReceiptSource::KeyMismatch;
    }
    let mut selector = Vec::with_capacity(33);
    selector.push(1);
    selector.extend_from_slice(&activity_id);
    let correlation_id = CORRELATION.fetch_add(1, Ordering::AcqRel);
    let request = match encode_envelope(Envelope {
        version: Version::V1_3,
        message_tag: RECEIPT_LOOKUP_REQUEST,
        correlation_id,
        canonical_payload: &selector,
        proof_material: &[],
    }) {
        Ok(request) => request,
        Err(error) => return ReceiptSource::Unavailable(format!("LNI encode: {error:?}")),
    };
    if let Err(error) = transport.send(&request) {
        return ReceiptSource::Unavailable(format!("LNI send: {error:?}"));
    }
    let response = match transport.receive() {
        Ok(response) => response,
        Err(error) => return ReceiptSource::Unavailable(format!("LNI receive: {error:?}")),
    };
    let envelope = match decode_envelope(&response) {
        Ok(envelope) => envelope,
        Err(error) => return ReceiptSource::Unavailable(format!("LNI decode: {error:?}")),
    };
    if envelope.version.major != Version::V1_3.major || envelope.correlation_id != correlation_id {
        return ReceiptSource::Unavailable(
            "LNI response changed version or correlation".to_owned(),
        );
    }
    match envelope.message_tag {
        RECEIPT_LOOKUP_RESPONSE if envelope.proof_material.is_empty() => {
            if envelope.canonical_payload.is_empty() {
                ReceiptSource::Unknown
            } else {
                ReceiptSource::Found(envelope.canonical_payload.to_vec())
            }
        }
        ERROR_RESPONSE => ReceiptSource::Unavailable(format!(
            "LNI refused the lookup: {:?}",
            decode_core_refusal(envelope.canonical_payload)
        )),
        other => ReceiptSource::Unavailable(format!("LNI answered message tag {other}")),
    }
}

fn evidence_refusal(refusal_kind: &EvidenceRefusal) -> Response {
    eprintln!("layerx-receipt-authority refused replica evidence: {refusal_kind:?}");
    refusal(502, "evidence_refused", None)
}

fn by_activity(config: &Config, requested: &str) -> Response {
    let Ok(activity_id) = hex::decode32(requested) else {
        return refusal(400, "invalid_activity_id", None);
    };
    if activity_id == [0; 32] {
        return refusal(400, "invalid_activity_id", None);
    }
    let receipt = match lookup_receipt(config, activity_id) {
        ReceiptSource::Found(receipt) => receipt,
        ReceiptSource::Unknown => return refusal(404, "unknown_activity", None),
        ReceiptSource::KeyMismatch => {
            eprintln!("layerx-receipt-authority: LNI sequencer key differs from the pinned key");
            return refusal(502, "sequencer_key_mismatch", None);
        }
        ReceiptSource::Unavailable(reason) => {
            eprintln!("layerx-receipt-authority receipt source unavailable: {reason}");
            return refusal(503, "receipt_source_unavailable", Some(5));
        }
    };
    let locator = match receipt_locator(&receipt) {
        Ok(locator) => locator,
        Err(error) => return evidence_refusal(&error),
    };
    if locator.activity_id != activity_id {
        return evidence_refusal(&EvidenceRefusal::ActivityMismatch);
    }
    let path = format!(
        "/v1/batches/{}/receipt-authority?receipt_digest={}",
        hex::encode(&locator.batch_id),
        hex::encode(&locator.receipt_digest)
    );
    let document = match replica_get(config, &path) {
        ReplicaAnswer::Status(200, body) => body,
        ReplicaAnswer::Status(404, _) => {
            return refusal(503, "replica_evidence_unavailable", Some(1));
        }
        ReplicaAnswer::Status(status, _) => {
            eprintln!("layerx-receipt-authority replica answered HTTP {status}");
            return refusal(503, "replica_unavailable", Some(5));
        }
        ReplicaAnswer::Unavailable => return refusal(503, "replica_unavailable", Some(5)),
    };
    let evidence =
        match parse_replica_evidence(&document, config.replica_id, config.sequencer_public_key) {
            Ok(evidence) => evidence,
            Err(error) => return evidence_refusal(&error),
        };
    match authorized_batch_by_activity(activity_id, &receipt, &evidence, &config.authorization) {
        Ok(facts) => json(
            200,
            &serde_json::json!({
                "activity_id": requested,
                "batch_id": hex::encode(&facts.batch_id),
                "asset": hex::encode(&facts.asset),
                "previous_state_root": hex::encode(&facts.previous_state_root),
                "resulting_state_root": hex::encode(&facts.resulting_state_root),
                "sequencer_public_key": hex::encode(&facts.sequencer_public_key),
                "network_id": config.network_id,
                "wire_version": config.wire_version,
            }),
        ),
        Err(error) => evidence_refusal(&error),
    }
}

fn relay(config: &Config, batch_id: &str, query: Option<&str>) -> Response {
    let Some(digest) = query.and_then(|query| query.strip_prefix("receipt_digest=")) else {
        return refusal(400, "invalid_request", None);
    };
    if !hex::is_hex32(batch_id) || !hex::is_hex32(digest) {
        return refusal(400, "invalid_request", None);
    }
    let path = format!("/v1/batches/{batch_id}/receipt-authority?receipt_digest={digest}");
    match replica_get(config, &path) {
        ReplicaAnswer::Status(status @ (200 | 404), body) => Response {
            status,
            body,
            retry_after: None,
        },
        ReplicaAnswer::Status(status, _) => {
            eprintln!("layerx-receipt-authority replica answered HTTP {status}");
            refusal(503, "replica_unavailable", Some(5))
        }
        ReplicaAnswer::Unavailable => refusal(503, "replica_unavailable", Some(5)),
    }
}

fn replica_answers(config: &Config) -> bool {
    let path = format!("/v1/batches/{ZERO_HEX32}/receipt-authority?receipt_digest={ZERO_HEX32}");
    matches!(
        replica_get(config, &path),
        ReplicaAnswer::Status(200 | 404, _)
    )
}

fn readiness(config: &Config) -> Response {
    let ready = replica_answers(config);
    let body = serde_json::json!({
        "ready": ready,
        "network_id": config.network_id,
        "wire_version": config.wire_version,
    });
    let mut response = json(if ready { 200 } else { 503 }, &body);
    if !ready {
        response.retry_after = Some(5);
    }
    response
}

fn route(config: &Config, request: &Request) -> Response {
    if request.method != "GET" {
        return refusal(405, "method_not_allowed", None);
    }
    let path = request.path.as_str();
    if path == "/livez" {
        return json(200, &serde_json::json!({ "live": true }));
    }
    if path == "/readyz" {
        return readiness(config);
    }
    if let Some(batch_id) = path
        .strip_prefix("/v1/batches/")
        .and_then(|rest| rest.strip_suffix("/receipt-authority"))
    {
        return match authenticate(config, request) {
            Ok(()) => relay(config, batch_id, request.query.as_deref()),
            Err(response) => response,
        };
    }
    if request.query.is_some() {
        return refusal(404, "not_found", None);
    }
    let activity = path
        .strip_prefix("/v1/authorized-batches/by-activity/")
        .or_else(|| {
            path.strip_prefix("/internal/v1/activities/")
                .and_then(|rest| rest.strip_suffix("/authority"))
        });
    match activity {
        Some(activity) => match authenticate(config, request) {
            Ok(()) => by_activity(config, activity),
            Err(response) => response,
        },
        None => refusal(404, "not_found", None),
    }
}

fn handle_connection(config: &Arc<Config>, tcp: TcpStream) -> Result<(), String> {
    tcp.set_read_timeout(Some(IO_TIMEOUT))
        .map_err(|error| error.to_string())?;
    tcp.set_write_timeout(Some(IO_TIMEOUT))
        .map_err(|error| error.to_string())?;
    let connection =
        ServerConnection::new(Arc::clone(&config.tls)).map_err(|error| error.to_string())?;
    let mut stream = StreamOwned::new(connection, tcp);
    let response = read_http_message(&mut stream).map_or_else(
        |_| refusal(400, "invalid_request", None),
        |request| route(config, &request),
    );
    write_response(&mut stream, &response)?;
    stream.conn.send_close_notify();
    let _ = stream.conn.write_tls(&mut stream.sock);
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

fn serve(config: Config) -> Result<(), String> {
    let listener = TcpListener::bind(config.listen).map_err(|error| error.to_string())?;
    let config = Arc::new(config);
    eprintln!(
        "layerx-receipt-authority listening with TLS on {}",
        config.listen
    );
    for connection in listener.incoming() {
        match connection {
            Ok(stream) => {
                let Some(permit) = ConnectionPermit::acquire() else {
                    continue;
                };
                let shared = Arc::clone(&config);
                thread::spawn(move || {
                    let _permit = permit;
                    if let Err(error) = handle_connection(&shared, stream) {
                        eprintln!("layerx-receipt-authority connection failed: {error}");
                    }
                });
            }
            Err(error) => eprintln!("layerx-receipt-authority accept failed: {error}"),
        }
    }
    Ok(())
}

fn main() {
    let arguments: Vec<String> = env::args().skip(1).collect();
    if arguments
        .iter()
        .any(|argument| argument == "--help" || argument == "-h")
    {
        print!("{USAGE}");
        return;
    }
    if !arguments.is_empty() {
        eprint!("layerx-receipt-authority accepts no arguments\n\n{USAGE}");
        std::process::exit(2);
    }
    if let Err(error) = config().and_then(serve) {
        eprintln!("layerx-receipt-authority: {error}");
        std::process::exit(2);
    }
}
