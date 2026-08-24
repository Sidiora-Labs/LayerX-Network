//! Strict authenticated HTTPS JSON-RPC quorum and broadcast transport.

use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::io::{Read, Write};
use std::net::{SocketAddr, TcpStream, ToSocketAddrs};
use std::os::unix::fs::PermissionsExt;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::Duration;

use rustls::pki_types::{CertificateDer, ServerName};
use rustls::{ClientConfig, ClientConnection, RootCertStore, StreamOwned};
use serde::Deserialize;
use serde_json::{json, Value};
use zeroize::{Zeroize, Zeroizing};

const MAX_ENDPOINTS: usize = 8;
const MAX_HEADER_BYTES: usize = 32 * 1024;
const MAX_REQUEST_BYTES: usize = 512 * 1024;
const DEFAULT_RETRY_SECONDS: u64 = 5;
static REQUEST_SEQUENCE: AtomicU64 = AtomicU64::new(1);

/// Authenticated endpoint. Tokens are file-backed secrets, never inline config.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct RpcEndpointConfig {
    pub url: String,
    pub ca_certificate_der: PathBuf,
    pub bearer_token_file: PathBuf,
    /// Operator-audited backend identity; aliases of one backend must not be
    /// counted as independent quorum members.
    pub independent_backend: String,
}

/// Strict-majority policy and finite transport bounds.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct RpcQuorumConfig {
    pub endpoints: Vec<RpcEndpointConfig>,
    pub quorum: usize,
    pub connect_timeout_ms: u64,
    pub request_timeout_ms: u64,
    pub maximum_response_bytes: usize,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum RpcError {
    Configuration,
    Unavailable,
    RateLimited { retry_after_seconds: u64 },
    Divergence,
    ResponseMismatch,
    Rejected { code: i64, message: String },
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum BroadcastResult {
    Accepted,
    Unknown,
}

struct Endpoint {
    host: String,
    port: u16,
    path: String,
    backend: String,
    authorization: Zeroizing<String>,
    tls: Arc<ClientConfig>,
}

/// Production read quorum. Exact result JSON must match on a strict majority;
/// endpoint errors can never vote for data.
pub struct RpcCluster {
    endpoints: Vec<Endpoint>,
    quorum: usize,
    connect_timeout: Duration,
    request_timeout: Duration,
    maximum_response_bytes: usize,
}

impl RpcCluster {
    pub fn new(config: &RpcQuorumConfig) -> Result<Self, RpcError> {
        if config.endpoints.len() < 2
            || config.endpoints.len() > MAX_ENDPOINTS
            || config.quorum < 2
            || config.quorum > config.endpoints.len()
            || config.quorum.saturating_mul(2) <= config.endpoints.len()
            || !(100..=30_000).contains(&config.connect_timeout_ms)
            || !(100..=120_000).contains(&config.request_timeout_ms)
            || !(1024..=16 * 1024 * 1024).contains(&config.maximum_response_bytes)
        {
            return Err(RpcError::Configuration);
        }
        let endpoints = config
            .endpoints
            .iter()
            .map(Endpoint::new)
            .collect::<Result<Vec<_>, _>>()?;
        let network_identities: BTreeSet<_> = endpoints
            .iter()
            .map(|endpoint| (endpoint.host.as_str(), endpoint.port))
            .collect();
        let backend_identities: BTreeSet<_> = endpoints
            .iter()
            .map(|endpoint| endpoint.backend.as_str())
            .collect();
        if network_identities.len() != endpoints.len()
            || backend_identities.len() != endpoints.len()
        {
            return Err(RpcError::Configuration);
        }
        Ok(Self {
            endpoints,
            quorum: config.quorum,
            connect_timeout: Duration::from_millis(config.connect_timeout_ms),
            request_timeout: Duration::from_millis(config.request_timeout_ms),
            maximum_response_bytes: config.maximum_response_bytes,
        })
    }

    /// Returns one byte-identical strict-majority result.
    pub fn call(&self, method: &str, parameters: Value) -> Result<Value, RpcError> {
        validate_method(method)?;
        let request_id = next_id()?;
        let body = request_body(method, parameters, request_id)?;
        let mut matching: BTreeMap<Vec<u8>, (usize, Value)> = BTreeMap::new();
        let mut rate_limit = None;
        let mut responses = 0_usize;
        let mut malformed = 0_usize;
        for endpoint in &self.endpoints {
            match endpoint.request(
                &body,
                self.connect_timeout,
                self.request_timeout,
                self.maximum_response_bytes,
                request_id,
            ) {
                Ok(value) => {
                    responses = responses.saturating_add(1);
                    let canonical = canonical_json(&value)?;
                    let entry = matching.entry(canonical).or_insert((0, value));
                    entry.0 = entry.0.saturating_add(1);
                }
                Err(RpcError::RateLimited {
                    retry_after_seconds,
                }) => {
                    rate_limit = Some(rate_limit.map_or(retry_after_seconds, |existing: u64| {
                        existing.max(retry_after_seconds)
                    }));
                }
                Err(RpcError::ResponseMismatch) => malformed = malformed.saturating_add(1),
                Err(_) => {}
            }
        }
        if let Some((_, value)) = matching
            .into_values()
            .find(|(count, _)| *count >= self.quorum)
        {
            return Ok(value);
        }
        if responses >= self.quorum {
            return Err(RpcError::Divergence);
        }
        if malformed >= self.quorum {
            return Err(RpcError::ResponseMismatch);
        }
        if let Some(retry_after_seconds) = rate_limit {
            return Err(RpcError::RateLimited {
                retry_after_seconds,
            });
        }
        Err(RpcError::Unavailable)
    }

    /// Broadcasts already signed immutable bytes to every independent endpoint.
    /// At least one exact expected identity is acceptance. An absence of any
    /// conclusive response is `Unknown`, never permission to replace the
    /// transaction. A strict majority of the same deterministic refusal is
    /// returned as permanent rejection.
    pub fn broadcast(
        &self,
        method: &str,
        parameters: Value,
        expected_identity: &str,
    ) -> Result<BroadcastResult, RpcError> {
        validate_method(method)?;
        if expected_identity.is_empty() || expected_identity.len() > 256 {
            return Err(RpcError::Configuration);
        }
        let request_id = next_id()?;
        let body = request_body(method, parameters, request_id)?;
        let mut refusals: BTreeMap<(i64, String), usize> = BTreeMap::new();
        let mut mismatched_successes = 0_usize;
        for endpoint in &self.endpoints {
            match endpoint.request(
                &body,
                self.connect_timeout,
                self.request_timeout,
                self.maximum_response_bytes,
                request_id,
            ) {
                Ok(value) => {
                    if value.as_str() == Some(expected_identity) {
                        return Ok(BroadcastResult::Accepted);
                    }
                    mismatched_successes = mismatched_successes.saturating_add(1);
                }
                Err(RpcError::Rejected { code, message }) => {
                    let count = refusals.entry((code, message)).or_default();
                    *count = count.saturating_add(1);
                }
                Err(_) => {}
            }
        }
        if let Some(((code, message), _)) = refusals
            .into_iter()
            .find(|(_, count)| *count >= self.quorum)
        {
            return Err(RpcError::Rejected { code, message });
        }
        if mismatched_successes >= self.quorum {
            return Err(RpcError::ResponseMismatch);
        }
        Ok(BroadcastResult::Unknown)
    }
}

fn canonical_json(value: &Value) -> Result<Vec<u8>, RpcError> {
    fn encode(value: &Value, output: &mut Vec<u8>) -> Result<(), RpcError> {
        match value {
            Value::Null | Value::Bool(_) | Value::Number(_) | Value::String(_) => {
                serde_json::to_writer(output, value).map_err(|_| RpcError::ResponseMismatch)?;
            }
            Value::Array(values) => {
                output.push(b'[');
                for (index, value) in values.iter().enumerate() {
                    if index != 0 {
                        output.push(b',');
                    }
                    encode(value, output)?;
                }
                output.push(b']');
            }
            Value::Object(values) => {
                output.push(b'{');
                let mut entries = values.iter().collect::<Vec<_>>();
                entries.sort_unstable_by(|(left, _), (right, _)| left.cmp(right));
                for (index, (key, value)) in entries.into_iter().enumerate() {
                    if index != 0 {
                        output.push(b',');
                    }
                    serde_json::to_writer(&mut *output, key)
                        .map_err(|_| RpcError::ResponseMismatch)?;
                    output.push(b':');
                    encode(value, output)?;
                }
                output.push(b'}');
            }
        }
        Ok(())
    }

    let mut output = Vec::new();
    encode(value, &mut output)?;
    Ok(output)
}

impl Endpoint {
    fn new(config: &RpcEndpointConfig) -> Result<Self, RpcError> {
        if !config.ca_certificate_der.is_absolute() || !config.bearer_token_file.is_absolute() {
            return Err(RpcError::Configuration);
        }
        let (mut host, port, path) = parse_url(&config.url)?;
        host.make_ascii_lowercase();
        let mut backend = config.independent_backend.trim().to_ascii_lowercase();
        if backend.is_empty()
            || backend.len() > 128
            || backend
                .bytes()
                .any(|byte| !(byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.')))
        {
            backend.zeroize();
            return Err(RpcError::Configuration);
        }
        let metadata =
            fs::metadata(&config.bearer_token_file).map_err(|_| RpcError::Configuration)?;
        if !metadata.is_file() || metadata.permissions().mode() & 0o037 != 0 {
            return Err(RpcError::Configuration);
        }
        let mut token =
            fs::read_to_string(&config.bearer_token_file).map_err(|_| RpcError::Configuration)?;
        while matches!(token.as_bytes().last(), Some(b'\r' | b'\n')) {
            token.pop();
        }
        if token.is_empty()
            || token.len() > 4096
            || token.bytes().any(|byte| byte <= b' ' || byte == b'\x7f')
        {
            token.zeroize();
            return Err(RpcError::Configuration);
        }
        let certificate =
            fs::read(&config.ca_certificate_der).map_err(|_| RpcError::Configuration)?;
        let mut roots = RootCertStore::empty();
        roots
            .add(CertificateDer::from(certificate))
            .map_err(|_| RpcError::Configuration)?;
        let tls = ClientConfig::builder()
            .with_root_certificates(roots)
            .with_no_client_auth();
        Ok(Self {
            host,
            port,
            path,
            backend,
            authorization: Zeroizing::new(token),
            tls: Arc::new(tls),
        })
    }

    fn request(
        &self,
        body: &[u8],
        connect_timeout: Duration,
        request_timeout: Duration,
        maximum_response_bytes: usize,
        request_id: u64,
    ) -> Result<Value, RpcError> {
        let addresses: Vec<SocketAddr> = (self.host.as_str(), self.port)
            .to_socket_addrs()
            .map_err(|_| RpcError::Unavailable)?
            .take(8)
            .collect();
        if addresses.is_empty() {
            return Err(RpcError::Unavailable);
        }
        for address in addresses {
            let Ok(tcp) = TcpStream::connect_timeout(&address, connect_timeout) else {
                continue;
            };
            tcp.set_read_timeout(Some(request_timeout))
                .and_then(|()| tcp.set_write_timeout(Some(request_timeout)))
                .map_err(|_| RpcError::Unavailable)?;
            let server_name =
                ServerName::try_from(self.host.clone()).map_err(|_| RpcError::Configuration)?;
            let connection = ClientConnection::new(Arc::clone(&self.tls), server_name)
                .map_err(|_| RpcError::Unavailable)?;
            let mut stream = StreamOwned::new(connection, tcp);
            write!(
                stream,
                "POST {} HTTP/1.1\r\nHost: {}\r\nAuthorization: Bearer {}\r\nAccept: application/json\r\nContent-Type: application/json\r\nUser-Agent: LayerX-Mirror/2\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                self.path,
                authority(&self.host, self.port),
                self.authorization.as_str(),
                body.len()
            )
            .map_err(|_| RpcError::Unavailable)?;
            stream
                .write_all(body)
                .and_then(|()| stream.flush())
                .map_err(|_| RpcError::Unavailable)?;
            return read_json_rpc(&mut stream, maximum_response_bytes, request_id);
        }
        Err(RpcError::Unavailable)
    }
}

fn validate_method(method: &str) -> Result<(), RpcError> {
    if method.is_empty()
        || method.len() > 128
        || !method
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'_')
    {
        Err(RpcError::Configuration)
    } else {
        Ok(())
    }
}

fn next_id() -> Result<u64, RpcError> {
    let request_id = REQUEST_SEQUENCE.fetch_add(1, Ordering::Relaxed);
    (request_id != 0)
        .then_some(request_id)
        .ok_or(RpcError::Unavailable)
}

fn request_body(method: &str, parameters: Value, request_id: u64) -> Result<Vec<u8>, RpcError> {
    let body = serde_json::to_vec(&json!({
        "jsonrpc": "2.0",
        "id": request_id,
        "method": method,
        "params": parameters
    }))
    .map_err(|_| RpcError::Configuration)?;
    if body.len() > MAX_REQUEST_BYTES {
        Err(RpcError::Configuration)
    } else {
        Ok(body)
    }
}

fn parse_url(value: &str) -> Result<(String, u16, String), RpcError> {
    if value.is_empty()
        || value.len() > 2048
        || value.bytes().any(|byte| byte <= b' ' || byte == b'\x7f')
    {
        return Err(RpcError::Configuration);
    }
    let rest = value
        .strip_prefix("https://")
        .ok_or(RpcError::Configuration)?;
    let (authority, path) = rest
        .split_once('/')
        .map_or((rest, "/".to_owned()), |(authority, path)| {
            (authority, format!("/{path}"))
        });
    if authority.is_empty()
        || authority.contains(['@', '?', '#', '\\'])
        || path.contains(['#', '\\'])
    {
        return Err(RpcError::Configuration);
    }
    let (host, port) = authority.rsplit_once(':').map_or_else(
        || Ok::<_, RpcError>((authority.to_owned(), 443)),
        |(host, port)| {
            Ok((
                host.to_owned(),
                port.parse::<u16>().map_err(|_| RpcError::Configuration)?,
            ))
        },
    )?;
    if !valid_dns(&host) || port == 0 {
        return Err(RpcError::Configuration);
    }
    Ok((host, port, path))
}

fn valid_dns(value: &str) -> bool {
    value.len() <= 253
        && value.split('.').all(|label| {
            !label.is_empty()
                && label.len() <= 63
                && !label.starts_with('-')
                && !label.ends_with('-')
                && label
                    .bytes()
                    .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-')
        })
}

fn authority(host: &str, port: u16) -> String {
    if port == 443 {
        host.to_owned()
    } else {
        format!("{host}:{port}")
    }
}

fn read_json_rpc(
    stream: &mut impl Read,
    maximum_response_bytes: usize,
    request_id: u64,
) -> Result<Value, RpcError> {
    let mut response = Vec::with_capacity(4096);
    let mut buffer = [0_u8; 4096];
    let header_end = loop {
        let read = stream
            .read(&mut buffer)
            .map_err(|_| RpcError::Unavailable)?;
        if read == 0 || response.len().saturating_add(read) > maximum_response_bytes {
            return Err(RpcError::ResponseMismatch);
        }
        response.extend_from_slice(&buffer[..read]);
        if let Some(position) = response.windows(4).position(|window| window == b"\r\n\r\n") {
            if position.saturating_add(4) > MAX_HEADER_BYTES {
                return Err(RpcError::ResponseMismatch);
            }
            break position.saturating_add(4);
        }
    };
    let header =
        std::str::from_utf8(&response[..header_end]).map_err(|_| RpcError::ResponseMismatch)?;
    let mut lines = header.split("\r\n");
    let status_line = lines.next().ok_or(RpcError::ResponseMismatch)?;
    let mut status_parts = status_line.split_whitespace();
    if !matches!(status_parts.next(), Some("HTTP/1.1" | "HTTP/1.0")) {
        return Err(RpcError::ResponseMismatch);
    }
    let status = status_parts
        .next()
        .ok_or(RpcError::ResponseMismatch)?
        .parse::<u16>()
        .map_err(|_| RpcError::ResponseMismatch)?;
    let mut headers = BTreeMap::new();
    for line in lines.filter(|line| !line.is_empty()) {
        let (name, value) = line.split_once(':').ok_or(RpcError::ResponseMismatch)?;
        let name = name.trim().to_ascii_lowercase();
        if name.is_empty() || headers.contains_key(&name) {
            return Err(RpcError::ResponseMismatch);
        }
        headers.insert(name, value.trim().to_owned());
    }
    if status == 429 {
        let retry_after_seconds = headers
            .get("retry-after")
            .and_then(|value| value.parse::<u64>().ok())
            .unwrap_or(DEFAULT_RETRY_SECONDS)
            .clamp(1, 3600);
        return Err(RpcError::RateLimited {
            retry_after_seconds,
        });
    }
    if !(200..300).contains(&status)
        || !headers
            .get("content-type")
            .is_some_and(|value| value.to_ascii_lowercase().starts_with("application/json"))
    {
        return Err(RpcError::Unavailable);
    }
    let mut wire_body = response[header_end..].to_vec();
    while wire_body.len().saturating_add(header_end) < maximum_response_bytes {
        let read = stream
            .read(&mut buffer)
            .map_err(|_| RpcError::Unavailable)?;
        if read == 0 {
            break;
        }
        wire_body.extend_from_slice(&buffer[..read]);
    }
    if wire_body.len().saturating_add(header_end) > maximum_response_bytes {
        return Err(RpcError::ResponseMismatch);
    }
    let body = match headers.get("transfer-encoding") {
        Some(value) if value.eq_ignore_ascii_case("chunked") => decode_chunked(&wire_body)?,
        Some(_) => return Err(RpcError::ResponseMismatch),
        None => {
            let length = headers
                .get("content-length")
                .ok_or(RpcError::ResponseMismatch)?
                .parse::<usize>()
                .map_err(|_| RpcError::ResponseMismatch)?;
            if length != wire_body.len() {
                return Err(RpcError::ResponseMismatch);
            }
            wire_body
        }
    };
    let envelope: Value = serde_json::from_slice(&body).map_err(|_| RpcError::ResponseMismatch)?;
    if envelope.get("jsonrpc").and_then(Value::as_str) != Some("2.0")
        || envelope.get("id").and_then(Value::as_u64) != Some(request_id)
    {
        return Err(RpcError::ResponseMismatch);
    }
    if let Some(error) = envelope.get("error") {
        let code = error
            .get("code")
            .and_then(Value::as_i64)
            .ok_or(RpcError::ResponseMismatch)?;
        if matches!(code, -32005 | -32016 | -32429) {
            return Err(RpcError::RateLimited {
                retry_after_seconds: DEFAULT_RETRY_SECONDS,
            });
        }
        let message = error
            .get("message")
            .and_then(Value::as_str)
            .unwrap_or("rpc refused request");
        let message = message.chars().take(256).collect();
        return Err(RpcError::Rejected { code, message });
    }
    envelope
        .get("result")
        .cloned()
        .ok_or(RpcError::ResponseMismatch)
}

fn decode_chunked(wire: &[u8]) -> Result<Vec<u8>, RpcError> {
    let mut offset = 0_usize;
    let mut body = Vec::new();
    loop {
        let line_end = wire
            .get(offset..)
            .ok_or(RpcError::ResponseMismatch)?
            .windows(2)
            .position(|window| window == b"\r\n")
            .map(|position| offset.saturating_add(position))
            .ok_or(RpcError::ResponseMismatch)?;
        let line =
            std::str::from_utf8(&wire[offset..line_end]).map_err(|_| RpcError::ResponseMismatch)?;
        let length = usize::from_str_radix(
            line.split(';').next().ok_or(RpcError::ResponseMismatch)?,
            16,
        )
        .map_err(|_| RpcError::ResponseMismatch)?;
        offset = line_end.saturating_add(2);
        if length == 0 {
            if wire.get(offset..offset.saturating_add(2)) != Some(b"\r\n") {
                return Err(RpcError::ResponseMismatch);
            }
            return Ok(body);
        }
        let end = offset
            .checked_add(length)
            .ok_or(RpcError::ResponseMismatch)?;
        body.extend_from_slice(wire.get(offset..end).ok_or(RpcError::ResponseMismatch)?);
        if wire.get(end..end.saturating_add(2)) != Some(b"\r\n") {
            return Err(RpcError::ResponseMismatch);
        }
        offset = end.saturating_add(2);
    }
}
