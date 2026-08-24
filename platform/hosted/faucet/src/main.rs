use native_tls::{Certificate, TlsConnector};
use rustls::pki_types::{CertificateDer, PrivateKeyDer, PrivatePkcs8KeyDer};
use rustls::{ServerConfig, ServerConnection, StreamOwned};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;
use std::env;
use std::fs;
use std::io::{Read, Write};
use std::net::{IpAddr, SocketAddr, TcpListener, TcpStream, ToSocketAddrs};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use std::thread;
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use subtle::ConstantTimeEq;
use zeroize::{Zeroize, Zeroizing};

const MAX_REQUEST_BYTES: usize = 16 * 1024;
const MAX_RESPONSE_BYTES: usize = 64 * 1024;
const CONNECT_TIMEOUT: Duration = Duration::from_secs(3);
const IO_TIMEOUT: Duration = Duration::from_secs(8);
const AUDIT_ATTEMPTS: usize = 8;
const MAX_CONNECTIONS: usize = 128;
static ACTIVE_CONNECTIONS: AtomicUsize = AtomicUsize::new(0);

#[derive(Clone)]
struct Endpoint {
    host: String,
    port: u16,
    path: String,
}

impl Endpoint {
    fn parse(value: &str, scheme: &str) -> Result<Self, String> {
        let prefix = format!("{scheme}://");
        let rest = value
            .strip_prefix(&prefix)
            .ok_or_else(|| format!("endpoint must use {scheme}"))?;
        let (authority, path) = rest.split_once('/').map_or((rest, "/"), |(host, tail)| {
            (host, if tail.is_empty() { "/" } else { tail })
        });
        if authority.is_empty()
            || authority.contains(['@', '?', '#', '\\'])
            || path.contains(['?', '#', '\\'])
        {
            return Err("endpoint is not canonical".to_owned());
        }
        let path = if path == "/" {
            "/".to_owned()
        } else {
            format!("/{path}")
        };
        let default_port = if scheme == "rediss" { 6379 } else { 443 };
        let (host, port) = authority.rsplit_once(':').map_or_else(
            || Ok::<_, String>((authority.to_owned(), default_port)),
            |(host, port)| {
                let port = port
                    .parse::<u16>()
                    .map_err(|_| "endpoint port is invalid".to_owned())?;
                Ok((host.to_owned(), port))
            },
        )?;
        if host.is_empty() || host.parse::<IpAddr>().is_ok() {
            return Err("TLS endpoint must use a DNS name".to_owned());
        }
        Ok(Self { host, port, path })
    }

    fn authority(&self) -> String {
        if self.port == 443 {
            self.host.clone()
        } else {
            format!("{}:{}", self.host, self.port)
        }
    }
}

struct Config {
    listen: SocketAddr,
    tls: Arc<ServerConfig>,
    outbound_ca: Certificate,
    identity: Endpoint,
    identity_service_token: Zeroizing<String>,
    funding: Endpoint,
    funding_admin_token: Zeroizing<String>,
    redis: Endpoint,
    redis_username: Zeroizing<String>,
    redis_password: Zeroizing<String>,
    identity_limit: u64,
    address_limit: u64,
    network_limit: u64,
    network_request_limit: u64,
    network_request_window_seconds: u64,
    window_seconds: u64,
    idempotency_seconds: u64,
    amount: u64,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ClaimRequest {
    did: String,
    public_key: String,
}

#[derive(Deserialize)]
struct SessionResponse {
    active: bool,
    sub: String,
}

#[derive(Serialize)]
struct FundingRequest<'a> {
    funding_id: &'a str,
    did: &'a str,
    public_key: &'a str,
    amount: u64,
}

#[derive(Deserialize)]
struct FundingResponse {
    funding_id: String,
    state: String,
    #[serde(default)]
    transaction_id: Option<String>,
}

struct Request {
    method: String,
    path: String,
    headers: BTreeMap<String, String>,
    body: Vec<u8>,
}

impl Drop for Request {
    fn drop(&mut self) {
        for value in self.headers.values_mut() {
            value.zeroize();
        }
        self.body.zeroize();
    }
}

struct Response {
    status: u16,
    body: String,
    retry_after: Option<u64>,
}

enum Reservation {
    Reserved { funding_id: String },
    Funded { body: String },
    Pending { funding_id: String },
    Conflict,
    Quota(&'static str),
}

enum FundingResult {
    Funded(String),
    Rejected,
    Unknown,
}

fn unix_seconds() -> Result<u64, String> {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_secs())
        .map_err(|_| "system clock precedes Unix epoch".to_owned())
}

fn sha256(parts: &[&str]) -> String {
    let mut digest = Sha256::new();
    for part in parts {
        digest.update(part.as_bytes());
        digest.update([0]);
    }
    format!("{digest:x}")
}

fn valid_identifier(value: &str, max: usize) -> bool {
    !value.is_empty()
        && value.len() <= max
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.' | b':'))
}

fn valid_did(value: &str) -> bool {
    value.starts_with("did:") && valid_identifier(value, 512)
}

fn valid_hex32(value: &str) -> bool {
    value.len() == 64 && value.bytes().all(|byte| byte.is_ascii_hexdigit())
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

fn parse_u64(name: &str, default: u64) -> Result<u64, String> {
    env::var(name).map_or(Ok(default), |value| {
        value
            .parse::<u64>()
            .map_err(|_| format!("{name} must be an integer"))
    })
}

fn server_tls_config() -> Result<Arc<ServerConfig>, String> {
    rustls::crypto::ring::default_provider()
        .install_default()
        .map_err(|_| "failed to install TLS crypto provider".to_owned())?;
    let certificate_path =
        env::var("LAYERX_FAUCET_TLS_CERT_DER").map_err(|_| "TLS certificate is required")?;
    let key_path =
        env::var("LAYERX_FAUCET_TLS_KEY_DER").map_err(|_| "TLS private key is required")?;
    let certificate = CertificateDer::from(fs::read(certificate_path).map_err(|e| e.to_string())?);
    let key = PrivateKeyDer::from(PrivatePkcs8KeyDer::from(
        fs::read(key_path).map_err(|e| e.to_string())?,
    ));
    let config = ServerConfig::builder()
        .with_no_client_auth()
        .with_single_cert(vec![certificate], key)
        .map_err(|error| error.to_string())?;
    Ok(Arc::new(config))
}

fn config() -> Result<Config, String> {
    let window_seconds = parse_u64("LAYERX_FAUCET_WINDOW_SECONDS", 86_400)?;
    let idempotency_seconds = parse_u64("LAYERX_FAUCET_IDEMPOTENCY_SECONDS", 604_800)?;
    let amount = parse_u64("LAYERX_FAUCET_CLAIM_AMOUNT", 1_000_000)?;
    if window_seconds == 0 || idempotency_seconds < window_seconds || amount == 0 {
        return Err(
            "window and claim amount must be positive and idempotency retention must cover the window"
                .to_owned(),
        );
    }
    let listen = env::var("LAYERX_FAUCET_LISTEN")
        .unwrap_or_else(|_| "0.0.0.0:9443".to_owned())
        .parse::<SocketAddr>()
        .map_err(|_| "LAYERX_FAUCET_LISTEN must be a socket address".to_owned())?;
    Ok(Config {
        listen,
        tls: server_tls_config()?,
        outbound_ca: Certificate::from_der(
            &fs::read(
                env::var("LAYERX_OUTBOUND_CA_DER")
                    .map_err(|_| "LAYERX_OUTBOUND_CA_DER is required")?,
            )
            .map_err(|error| error.to_string())?,
        )
        .map_err(|error| error.to_string())?,
        identity: Endpoint::parse(
            &env::var("LAYERX_IDENTITY_INTROSPECTION_URL")
                .map_err(|_| "LAYERX_IDENTITY_INTROSPECTION_URL is required")?,
            "https",
        )?,
        identity_service_token: read_secret("LAYERX_IDENTITY_SERVICE_TOKEN_FILE")?,
        funding: Endpoint::parse(
            &env::var("LAYERX_TESTNET_FUNDING_URL")
                .map_err(|_| "LAYERX_TESTNET_FUNDING_URL is required")?,
            "https",
        )?,
        funding_admin_token: read_secret("LAYERX_TESTNET_ADMIN_TOKEN_FILE")?,
        redis: Endpoint::parse(
            &env::var("LAYERX_FAUCET_REDIS_URL")
                .map_err(|_| "LAYERX_FAUCET_REDIS_URL is required")?,
            "rediss",
        )?,
        redis_username: read_secret("LAYERX_FAUCET_REDIS_USERNAME_FILE")?,
        redis_password: read_secret("LAYERX_FAUCET_REDIS_PASSWORD_FILE")?,
        identity_limit: parse_u64("LAYERX_FAUCET_IDENTITY_LIMIT", 10_000_000)?,
        address_limit: parse_u64("LAYERX_FAUCET_ADDRESS_LIMIT", 10_000_000)?,
        network_limit: parse_u64("LAYERX_FAUCET_NETWORK_LIMIT", 50_000_000)?,
        network_request_limit: parse_u64("LAYERX_FAUCET_NETWORK_REQUEST_LIMIT", 60)?,
        network_request_window_seconds: parse_u64(
            "LAYERX_FAUCET_NETWORK_REQUEST_WINDOW_SECONDS",
            60,
        )?,
        window_seconds,
        idempotency_seconds,
        amount,
    })
}

fn connect_tcp(endpoint: &Endpoint) -> Result<TcpStream, String> {
    let addresses = (endpoint.host.as_str(), endpoint.port)
        .to_socket_addrs()
        .map_err(|error| error.to_string())?;
    let mut last_error = None;
    for address in addresses.take(8) {
        match TcpStream::connect_timeout(&address, CONNECT_TIMEOUT) {
            Ok(stream) => {
                stream
                    .set_read_timeout(Some(IO_TIMEOUT))
                    .map_err(|error| error.to_string())?;
                stream
                    .set_write_timeout(Some(IO_TIMEOUT))
                    .map_err(|error| error.to_string())?;
                return Ok(stream);
            }
            Err(error) => last_error = Some(error),
        }
    }
    Err(last_error.map_or_else(
        || "endpoint did not resolve".to_owned(),
        |error| error.to_string(),
    ))
}

fn https_request(
    ca: &Certificate,
    endpoint: &Endpoint,
    method: &str,
    bearer: &str,
    idempotency_key: Option<&str>,
    body: &[u8],
) -> Result<(u16, Vec<u8>), String> {
    let connector = TlsConnector::builder()
        .add_root_certificate(ca.clone())
        .min_protocol_version(Some(native_tls::Protocol::Tlsv12))
        .build()
        .map_err(|error| error.to_string())?;
    let tcp = connect_tcp(endpoint)?;
    let mut stream = connector
        .connect(&endpoint.host, tcp)
        .map_err(|error| error.to_string())?;
    let idempotency =
        idempotency_key.map_or(String::new(), |key| format!("Idempotency-Key: {key}\r\n"));
    write!(
        stream,
        "{method} {} HTTP/1.1\r\nHost: {}\r\nAuthorization: Bearer {bearer}\r\nContent-Type: application/json\r\nAccept: application/json\r\n{idempotency}Content-Length: {}\r\nConnection: close\r\n\r\n",
        endpoint.path,
        endpoint.authority(),
        body.len()
    )
    .map_err(|error| error.to_string())?;
    stream.write_all(body).map_err(|error| error.to_string())?;
    stream.flush().map_err(|error| error.to_string())?;
    read_http_response(&mut stream)
}

fn read_http_response(stream: &mut impl Read) -> Result<(u16, Vec<u8>), String> {
    let mut request = read_http_message(stream, MAX_RESPONSE_BYTES)?;
    let first = request
        .headers
        .get("")
        .ok_or_else(|| "upstream response is missing status".to_owned())?;
    let mut parts = first.split_whitespace();
    if parts.next() != Some("HTTP/1.1") {
        return Err("upstream must use HTTP/1.1".to_owned());
    }
    let status = parts
        .next()
        .ok_or_else(|| "upstream status is missing".to_owned())?
        .parse::<u16>()
        .map_err(|_| "upstream status is invalid".to_owned())?;
    Ok((status, std::mem::take(&mut request.body)))
}

fn read_http_message(stream: &mut impl Read, maximum: usize) -> Result<Request, String> {
    let mut bytes = Vec::with_capacity(2048);
    let mut chunk = [0_u8; 2048];
    let header_end = loop {
        let count = stream.read(&mut chunk).map_err(|error| error.to_string())?;
        if count == 0 || bytes.len().saturating_add(count) > maximum {
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
        headers,
        body: bytes[header_end..header_end + content_length].to_vec(),
    })
}

fn parse_client_request(stream: &mut impl Read) -> Result<Request, String> {
    let mut request = read_http_message(stream, MAX_REQUEST_BYTES)?;
    let start = request
        .headers
        .remove("")
        .ok_or_else(|| "request line is missing".to_owned())?;
    let mut parts = start.split_whitespace();
    request.method = parts
        .next()
        .ok_or_else(|| "request method is missing".to_owned())?
        .to_owned();
    request.path = parts
        .next()
        .ok_or_else(|| "request target is missing".to_owned())?
        .to_owned();
    if parts.next() != Some("HTTP/1.1") || parts.next().is_some() || request.path.contains('?') {
        return Err("request line is invalid".to_owned());
    }
    if !request.headers.contains_key("host") {
        return Err("HTTP/1.1 Host header is required".to_owned());
    }
    Ok(request)
}

fn authenticate(config: &Config, request: &Request) -> Result<String, Response> {
    let Some(token) = request
        .headers
        .get("authorization")
        .and_then(|value| value.strip_prefix("Bearer "))
    else {
        return Err(refusal(401, "identity_required", None));
    };
    if token.is_empty() || token.len() > 4096 {
        return Err(refusal(401, "identity_required", None));
    }
    let body = Zeroizing::new(
        serde_json::to_vec(&serde_json::json!({ "token": token }))
            .map_err(|_| refusal(503, "identity_unavailable", Some(10)))?,
    );
    let service_token = config.identity_service_token.as_str();
    let (status, response) = https_request(
        &config.outbound_ca,
        &config.identity,
        "POST",
        service_token,
        None,
        &body,
    )
    .map_err(|_| refusal(503, "identity_unavailable", Some(10)))?;
    if status != 200 {
        return Err(refusal(401, "identity_required", None));
    }
    let response = Zeroizing::new(response);
    let session: SessionResponse = serde_json::from_slice(&response)
        .map_err(|_| refusal(503, "identity_unavailable", Some(10)))?;
    if !session.active || !valid_identifier(&session.sub, 512) {
        return Err(refusal(401, "identity_required", None));
    }
    Ok(session.sub)
}

enum Resp {
    Simple(String),
    Bulk(Option<Vec<u8>>),
    Integer(i64),
    Array(Vec<Resp>),
}

fn redis_command(config: &Config, arguments: &[&str]) -> Result<Resp, String> {
    let connector = TlsConnector::builder()
        .add_root_certificate(config.outbound_ca.clone())
        .min_protocol_version(Some(native_tls::Protocol::Tlsv12))
        .build()
        .map_err(|error| error.to_string())?;
    let tcp = connect_tcp(&config.redis)?;
    let mut stream = connector
        .connect(&config.redis.host, tcp)
        .map_err(|error| error.to_string())?;
    write_resp_command(
        &mut stream,
        &[
            "AUTH",
            config.redis_username.as_str(),
            config.redis_password.as_str(),
        ],
    )?;
    match read_resp(&mut stream, 0)? {
        Resp::Simple(value) if value == "OK" => {}
        _ => return Err("Redis authentication failed".to_owned()),
    }
    write_resp_command(&mut stream, arguments)?;
    read_resp(&mut stream, 0)
}

fn write_resp_command(stream: &mut impl Write, arguments: &[&str]) -> Result<(), String> {
    write!(stream, "*{}\r\n", arguments.len()).map_err(|error| error.to_string())?;
    for argument in arguments {
        write!(stream, "${}\r\n", argument.len()).map_err(|error| error.to_string())?;
        stream
            .write_all(argument.as_bytes())
            .map_err(|error| error.to_string())?;
        stream
            .write_all(b"\r\n")
            .map_err(|error| error.to_string())?;
    }
    stream.flush().map_err(|error| error.to_string())
}

fn read_resp(stream: &mut impl Read, depth: usize) -> Result<Resp, String> {
    if depth > 4 {
        return Err("Redis response nesting exceeds its bound".to_owned());
    }
    let mut prefix = [0_u8; 1];
    stream.read_exact(&mut prefix).map_err(|e| e.to_string())?;
    let line = read_resp_line(stream)?;
    match prefix[0] {
        b'+' => Ok(Resp::Simple(line)),
        b'-' => Err("Redis returned an error".to_owned()),
        b':' => line
            .parse::<i64>()
            .map(Resp::Integer)
            .map_err(|_| "Redis integer is invalid".to_owned()),
        b'$' => {
            let length = line
                .parse::<i64>()
                .map_err(|_| "Redis bulk length is invalid".to_owned())?;
            if length == -1 {
                return Ok(Resp::Bulk(None));
            }
            let length =
                usize::try_from(length).map_err(|_| "Redis bulk length is invalid".to_owned())?;
            if length > MAX_RESPONSE_BYTES {
                return Err("Redis bulk response exceeds its bound".to_owned());
            }
            let mut value = vec![0_u8; length];
            stream.read_exact(&mut value).map_err(|e| e.to_string())?;
            let mut end = [0_u8; 2];
            stream.read_exact(&mut end).map_err(|e| e.to_string())?;
            if end != *b"\r\n" {
                return Err("Redis bulk response is malformed".to_owned());
            }
            Ok(Resp::Bulk(Some(value)))
        }
        b'*' => {
            let length = line
                .parse::<usize>()
                .map_err(|_| "Redis array length is invalid".to_owned())?;
            if length > 32 {
                return Err("Redis array exceeds its bound".to_owned());
            }
            let mut values = Vec::with_capacity(length);
            for _ in 0..length {
                values.push(read_resp(stream, depth + 1)?);
            }
            Ok(Resp::Array(values))
        }
        _ => Err("Redis response prefix is invalid".to_owned()),
    }
}

fn read_resp_line(stream: &mut impl Read) -> Result<String, String> {
    let mut value = Vec::new();
    loop {
        let mut byte = [0_u8; 1];
        stream.read_exact(&mut byte).map_err(|e| e.to_string())?;
        if byte[0] == b'\r' {
            stream.read_exact(&mut byte).map_err(|e| e.to_string())?;
            if byte[0] != b'\n' {
                return Err("Redis line is malformed".to_owned());
            }
            break;
        }
        if value.len() >= 4096 {
            return Err("Redis line exceeds its bound".to_owned());
        }
        value.push(byte[0]);
    }
    String::from_utf8(value).map_err(|_| "Redis line is not UTF-8".to_owned())
}

fn resp_text(value: &Resp) -> Option<String> {
    match value {
        Resp::Simple(value) => Some(value.clone()),
        Resp::Bulk(Some(value)) => String::from_utf8(value.clone()).ok(),
        Resp::Integer(value) => Some(value.to_string()),
        Resp::Bulk(None) | Resp::Array(_) => None,
    }
}

const NETWORK_ADMISSION_SCRIPT: &str = r#"
local used = redis.call('INCR', KEYS[1])
if used == 1 then redis.call('EXPIRE', KEYS[1], ARGV[1]) end
if used > tonumber(ARGV[2]) then return {'rate_limited'} end
return {'admitted'}
"#;

fn admit_network(config: &Config, peer: IpAddr) -> Result<bool, String> {
    if config.network_request_limit == 0 || config.network_request_window_seconds == 0 {
        return Err("network request admission bounds must be positive".to_owned());
    }
    let now = unix_seconds()?;
    let window = now / config.network_request_window_seconds;
    let key = format!("faucet:admission:{window}:{}", sha256(&[&peer.to_string()]));
    let response = redis_command(
        config,
        &[
            "EVAL",
            NETWORK_ADMISSION_SCRIPT,
            "1",
            &key,
            &config.network_request_window_seconds.to_string(),
            &config.network_request_limit.to_string(),
        ],
    )?;
    let Resp::Array(values) = response else {
        return Err("network admission response is invalid".to_owned());
    };
    Ok(values.first().and_then(resp_text).as_deref() == Some("admitted"))
}

const RESERVE_SCRIPT: &str = r#"
local current = redis.call('GET', KEYS[6]) or ''
if current ~= ARGV[10] then return {'audit_retry'} end
local existing = redis.call('HGET', KEYS[1], 'digest')
if existing then
  return {'existing', existing, redis.call('HGET', KEYS[1], 'state') or '', redis.call('HGET', KEYS[1], 'funding_id') or '', redis.call('HGET', KEYS[1], 'response') or ''}
end
local identity = tonumber(redis.call('GET', KEYS[2]) or '0')
local address = tonumber(redis.call('GET', KEYS[3]) or '0')
local network = tonumber(redis.call('GET', KEYS[4]) or '0')
local quota = ''
if identity + tonumber(ARGV[2]) > tonumber(ARGV[3]) then quota = 'identity_quota'
elseif address + tonumber(ARGV[2]) > tonumber(ARGV[4]) then quota = 'address_quota'
elseif network + tonumber(ARGV[2]) > tonumber(ARGV[5]) then quota = 'network_quota' end
if quota ~= '' then
  redis.call('XADD', KEYS[5], '*', 'event', ARGV[9], 'result', quota, 'chain', ARGV[11])
  redis.call('SET', KEYS[6], ARGV[11])
  return {'quota', quota}
end
redis.call('INCRBY', KEYS[2], ARGV[2]); redis.call('EXPIRE', KEYS[2], ARGV[6])
redis.call('INCRBY', KEYS[3], ARGV[2]); redis.call('EXPIRE', KEYS[3], ARGV[6])
redis.call('INCRBY', KEYS[4], ARGV[2]); redis.call('EXPIRE', KEYS[4], ARGV[6])
redis.call('HSET', KEYS[1], 'digest', ARGV[1], 'state', 'reserved', 'funding_id', ARGV[8], 'identity_key', KEYS[2], 'address_key', KEYS[3], 'network_key', KEYS[4], 'amount', ARGV[2])
redis.call('EXPIRE', KEYS[1], ARGV[7])
redis.call('XADD', KEYS[5], '*', 'event', ARGV[9], 'result', 'reserved', 'funding_id', ARGV[8], 'chain', ARGV[11])
redis.call('SET', KEYS[6], ARGV[11])
return {'reserved', ARGV[8]}
"#;

fn reserve(
    config: &Config,
    idempotency: &str,
    digest: &str,
    identity: &str,
    address: &str,
    peer: &str,
) -> Result<Reservation, String> {
    let now = unix_seconds()?;
    let window = now / config.window_seconds;
    let retry = config.window_seconds - now % config.window_seconds;
    let identity_key = format!("faucet:quota:{window}:identity:{}", sha256(&[identity]));
    let address_key = format!("faucet:quota:{window}:address:{}", sha256(&[address]));
    let network_key = format!("faucet:quota:{window}:network:{}", sha256(&[peer]));
    let idem_key = format!("faucet:idem:{}", sha256(&[idempotency]));
    let funding_id = sha256(&[idempotency, digest]);
    let event = sha256(&[&now.to_string(), &idem_key, digest, peer]);
    for _ in 0..AUDIT_ATTEMPTS {
        let head = match redis_command(config, &["GET", "faucet:audit:head"])? {
            Resp::Bulk(Some(value)) => {
                String::from_utf8(value).map_err(|_| "audit head is invalid".to_owned())?
            }
            Resp::Bulk(None) => String::new(),
            _ => return Err("audit head response is invalid".to_owned()),
        };
        let chain = sha256(&[&head, &event]);
        let result = redis_command(
            config,
            &[
                "EVAL",
                RESERVE_SCRIPT,
                "6",
                &idem_key,
                &identity_key,
                &address_key,
                &network_key,
                "faucet:audit",
                "faucet:audit:head",
                digest,
                &config.amount.to_string(),
                &config.identity_limit.to_string(),
                &config.address_limit.to_string(),
                &config.network_limit.to_string(),
                &retry.to_string(),
                &config.idempotency_seconds.to_string(),
                &funding_id,
                &event,
                &head,
                &chain,
            ],
        )?;
        let Resp::Array(values) = result else {
            return Err("reservation response is invalid".to_owned());
        };
        let tag = values.first().and_then(resp_text).unwrap_or_default();
        match tag.as_str() {
            "audit_retry" => continue,
            "reserved" => return Ok(Reservation::Reserved { funding_id }),
            "quota" => {
                return Ok(Reservation::Quota(
                    match values.get(1).and_then(resp_text).as_deref() {
                        Some("identity_quota") => "identity_quota",
                        Some("address_quota") => "address_quota",
                        _ => "network_quota",
                    },
                ))
            }
            "existing" => {
                let existing_digest = values.get(1).and_then(resp_text).unwrap_or_default();
                if existing_digest
                    .as_bytes()
                    .ct_eq(digest.as_bytes())
                    .unwrap_u8()
                    != 1
                {
                    return Ok(Reservation::Conflict);
                }
                return Ok(match values.get(2).and_then(resp_text).as_deref() {
                    Some("funded") => Reservation::Funded {
                        body: values.get(4).and_then(resp_text).unwrap_or_default(),
                    },
                    _ => Reservation::Pending {
                        funding_id: values.get(3).and_then(resp_text).unwrap_or_default(),
                    },
                });
            }
            _ => return Err("reservation state is invalid".to_owned()),
        }
    }
    Err("audit head remained contended".to_owned())
}

const COMPLETE_SCRIPT: &str = r#"
if redis.call('HGET', KEYS[1], 'digest') ~= ARGV[1] or redis.call('HGET', KEYS[1], 'funding_id') ~= ARGV[2] then return {'conflict'} end
if redis.call('HGET', KEYS[1], 'state') == 'funded' then return {'funded'} end
redis.call('HSET', KEYS[1], 'state', 'funded', 'response', ARGV[3])
redis.call('XADD', KEYS[2], '*', 'event', ARGV[4], 'result', 'funded', 'funding_id', ARGV[2])
return {'funded'}
"#;

const ROLLBACK_SCRIPT: &str = r#"
if redis.call('HGET', KEYS[1], 'digest') ~= ARGV[1] or redis.call('HGET', KEYS[1], 'funding_id') ~= ARGV[2] or redis.call('HGET', KEYS[1], 'state') ~= 'reserved' then return {'unchanged'} end
local amount = tonumber(redis.call('HGET', KEYS[1], 'amount') or '0')
for index = 3, 5 do
  local key = redis.call('HGET', KEYS[1], ARGV[index])
  if key and redis.call('EXISTS', key) == 1 then
    local remaining = redis.call('DECRBY', key, amount)
    if remaining <= 0 then redis.call('DEL', key) end
  end
end
redis.call('DEL', KEYS[1])
redis.call('XADD', KEYS[2], '*', 'event', ARGV[6], 'result', 'rejected', 'funding_id', ARGV[2])
return {'rolled_back'}
"#;

fn complete(
    config: &Config,
    idempotency: &str,
    digest: &str,
    funding_id: &str,
    body: &str,
) -> Result<(), String> {
    let idem_key = format!("faucet:idem:{}", sha256(&[idempotency]));
    let event = sha256(&["funded", funding_id, digest]);
    let response = redis_command(
        config,
        &[
            "EVAL",
            COMPLETE_SCRIPT,
            "2",
            &idem_key,
            "faucet:audit",
            digest,
            funding_id,
            body,
            &event,
        ],
    )?;
    match response {
        Resp::Array(values) if values.first().and_then(resp_text).as_deref() == Some("funded") => {
            Ok(())
        }
        _ => Err("funding completion conflicted with durable state".to_owned()),
    }
}

fn rollback(
    config: &Config,
    idempotency: &str,
    digest: &str,
    funding_id: &str,
) -> Result<(), String> {
    let idem_key = format!("faucet:idem:{}", sha256(&[idempotency]));
    let event = sha256(&["rejected", funding_id, digest]);
    let response = redis_command(
        config,
        &[
            "EVAL",
            ROLLBACK_SCRIPT,
            "2",
            &idem_key,
            "faucet:audit",
            digest,
            funding_id,
            "identity_key",
            "address_key",
            "network_key",
            &event,
        ],
    )?;
    match response {
        Resp::Array(values)
            if matches!(
                values.first().and_then(resp_text).as_deref(),
                Some("rolled_back" | "unchanged")
            ) =>
        {
            Ok(())
        }
        _ => Err("funding rollback conflicted with durable state".to_owned()),
    }
}

fn fund(config: &Config, claim: &ClaimRequest, funding_id: &str) -> FundingResult {
    let Ok(body) = serde_json::to_vec(&FundingRequest {
        funding_id,
        did: &claim.did,
        public_key: &claim.public_key,
        amount: config.amount,
    }) else {
        return FundingResult::Unknown;
    };
    let Ok((status, response)) = https_request(
        &config.outbound_ca,
        &config.funding,
        "POST",
        config.funding_admin_token.as_str(),
        Some(funding_id),
        &body,
    ) else {
        return FundingResult::Unknown;
    };
    if (400..500).contains(&status) {
        return FundingResult::Rejected;
    }
    if status != 200 {
        return FundingResult::Unknown;
    }
    let Ok(result) = serde_json::from_slice::<FundingResponse>(&response) else {
        return FundingResult::Unknown;
    };
    if result.funding_id != funding_id || result.state != "funded" {
        return FundingResult::Unknown;
    }
    let response_body = serde_json::json!({
        "funded": true,
        "funding_id": funding_id,
        "transaction_id": result.transaction_id,
        "amount": config.amount.to_string(),
        "network": "layerx-testnet"
    });
    FundingResult::Funded(response_body.to_string())
}

fn route(config: &Config, request: &Request, peer: IpAddr) -> Response {
    if request.method == "GET" && request.path == "/livez" {
        return ok("{\"status\":\"live\",\"service\":\"faucet\"}".to_owned());
    }
    if request.method == "GET" && request.path == "/readyz" {
        return match redis_command(config, &["PING"]) {
            Ok(Resp::Simple(value)) if value == "PONG" => {
                ok("{\"status\":\"ready\",\"service\":\"faucet\"}".to_owned())
            }
            _ => refusal(503, "dependency_unavailable", Some(10)),
        };
    }
    if request.method != "POST" || request.path != "/v1/faucet/claims" {
        return refusal(404, "not_found", None);
    }
    if request.headers.get("content-type").map(String::as_str) != Some("application/json") {
        return refusal(400, "content_type_required", None);
    }
    if request.headers.contains_key("forwarded")
        || request.headers.contains_key("x-forwarded-for")
        || request.headers.contains_key("x-real-ip")
        || request.headers.contains_key("x-layerx-client-ip")
        || request.headers.contains_key("x-layerx-principal")
    {
        return refusal(400, "untrusted_identity_header", None);
    }
    match admit_network(config, peer) {
        Ok(true) => {}
        Ok(false) => {
            return refusal(
                429,
                "network_request_rate",
                Some(config.network_request_window_seconds),
            )
        }
        Err(_) => return refusal(503, "persistence_unavailable", Some(10)),
    }
    let Some(idempotency) = request.headers.get("idempotency-key") else {
        return refusal(400, "idempotency_key_required", None);
    };
    if !valid_identifier(idempotency, 128) {
        return refusal(400, "invalid_idempotency_key", None);
    }
    let identity = match authenticate(config, request) {
        Ok(identity) => identity,
        Err(response) => return response,
    };
    let claim: ClaimRequest = match serde_json::from_slice(&request.body) {
        Ok(claim) => claim,
        Err(_) => return refusal(400, "invalid_argument", None),
    };
    if !valid_did(&claim.did) || !valid_hex32(&claim.public_key) {
        return refusal(400, "invalid_argument", None);
    }
    let digest = sha256(&[
        &identity,
        &claim.did,
        &claim.public_key,
        &config.amount.to_string(),
    ]);
    let reservation = match reserve(
        config,
        idempotency,
        &digest,
        &identity,
        &claim.public_key,
        &peer.to_string(),
    ) {
        Ok(reservation) => reservation,
        Err(_) => return refusal(503, "persistence_unavailable", Some(10)),
    };
    match reservation {
        Reservation::Funded { body } => ok(body),
        Reservation::Pending { funding_id } => match fund(config, &claim, &funding_id) {
            FundingResult::Funded(body) => {
                if complete(config, idempotency, &digest, &funding_id, &body).is_ok() {
                    ok(body)
                } else {
                    pending()
                }
            }
            FundingResult::Rejected => {
                if rollback(config, idempotency, &digest, &funding_id).is_ok() {
                    refusal(503, "funding_rejected", Some(10))
                } else {
                    pending()
                }
            }
            FundingResult::Unknown => pending(),
        },
        Reservation::Conflict => refusal(409, "idempotency_conflict", None),
        Reservation::Quota(code) => refusal(
            429,
            code,
            unix_seconds()
                .ok()
                .map(|now| config.window_seconds - now % config.window_seconds),
        ),
        Reservation::Reserved { funding_id } => match fund(config, &claim, &funding_id) {
            FundingResult::Funded(body) => {
                if complete(config, idempotency, &digest, &funding_id, &body).is_ok() {
                    ok(body)
                } else {
                    pending()
                }
            }
            FundingResult::Rejected => {
                if rollback(config, idempotency, &digest, &funding_id).is_ok() {
                    refusal(503, "funding_rejected", Some(10))
                } else {
                    pending()
                }
            }
            FundingResult::Unknown => pending(),
        },
    }
}

fn ok(body: String) -> Response {
    Response {
        status: 200,
        body,
        retry_after: None,
    }
}

fn pending() -> Response {
    Response {
        status: 202,
        body: "{\"state\":\"still_checking\",\"retry\":\"after\",\"retry_after_seconds\":10}"
            .to_owned(),
        retry_after: Some(10),
    }
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

fn write_response(stream: &mut impl Write, response: &Response) -> Result<(), String> {
    let reason = match response.status {
        200 => "OK",
        202 => "Accepted",
        400 => "Bad Request",
        401 => "Unauthorized",
        404 => "Not Found",
        409 => "Conflict",
        429 => "Too Many Requests",
        _ => "Service Unavailable",
    };
    let retry = response.retry_after.map_or(String::new(), |seconds| {
        format!("Retry-After: {seconds}\r\n")
    });
    write!(
        stream,
        "HTTP/1.1 {} {reason}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nCache-Control: no-store\r\n{retry}Connection: close\r\n\r\n{}",
        response.status,
        response.body.len(),
        response.body
    )
    .map_err(|error| error.to_string())
}

fn handle_connection(config: &Arc<Config>, tcp: TcpStream, peer: SocketAddr) -> Result<(), String> {
    tcp.set_read_timeout(Some(IO_TIMEOUT))
        .map_err(|error| error.to_string())?;
    tcp.set_write_timeout(Some(IO_TIMEOUT))
        .map_err(|error| error.to_string())?;
    let connection = ServerConnection::new(Arc::clone(&config.tls)).map_err(|e| e.to_string())?;
    let mut stream = StreamOwned::new(connection, tcp);
    let response = parse_client_request(&mut stream).map_or_else(
        |_| refusal(400, "invalid_request", None),
        |request| route(config, &request, peer.ip()),
    );
    write_response(&mut stream, &response)
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

fn platform_faucet(config: Config) -> Result<(), String> {
    let listener = TcpListener::bind(config.listen).map_err(|error| error.to_string())?;
    let config = Arc::new(config);
    eprintln!("layerx-faucet listening with TLS");
    for connection in listener.incoming() {
        match connection {
            Ok(stream) => {
                let Some(permit) = ConnectionPermit::acquire() else {
                    continue;
                };
                let peer = match stream.peer_addr() {
                    Ok(peer) => peer,
                    Err(error) => {
                        eprintln!("layerx-faucet rejected a connection: {error}");
                        continue;
                    }
                };
                let shared = Arc::clone(&config);
                thread::spawn(move || {
                    let _permit = permit;
                    if let Err(error) = handle_connection(&shared, stream, peer) {
                        eprintln!("layerx-faucet connection failed: {error}");
                    }
                });
            }
            Err(error) => eprintln!("layerx-faucet accept failed: {error}"),
        }
    }
    Ok(())
}

fn main() {
    if let Err(error) = config().and_then(platform_faucet) {
        eprintln!("layerx-faucet: {error}");
        std::process::exit(2);
    }
}
