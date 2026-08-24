use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::io::{Read, Write};
use std::net::{SocketAddr, TcpStream, ToSocketAddrs};
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::Duration;

use rustls::pki_types::{CertificateDer, ServerName};
use rustls::{ClientConfig, ClientConnection, RootCertStore, StreamOwned};
use serde::Deserialize;
use serde_json::{json, Value};
use zeroize::{Zeroize, Zeroizing};

use crate::MigrationError;

const MAX_ENDPOINTS: usize = 8;
const MAX_HEADER_BYTES: usize = 32 * 1024;
const DEFAULT_RETRY_SECONDS: u64 = 5;
static REQUEST_SEQUENCE: AtomicU64 = AtomicU64::new(1);

/// One authenticated HTTPS JSON-RPC endpoint. Credentials are read from the
/// named file and never accepted inline or retained in debug output.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq)]
pub struct RpcEndpointConfig {
    pub url: String,
    pub ca_certificate_der: PathBuf,
    pub bearer_token_file: PathBuf,
}

/// Strict majority policy and transport bounds for source-chain RPC reads.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq)]
pub struct RpcQuorumConfig {
    pub endpoints: Vec<RpcEndpointConfig>,
    pub quorum: usize,
    pub connect_timeout_ms: u64,
    pub request_timeout_ms: u64,
    pub maximum_response_bytes: usize,
}

struct Endpoint {
    host: String,
    port: u16,
    path: String,
    authorization: Zeroizing<String>,
    tls: Arc<ClientConfig>,
}

pub(crate) struct RpcCluster {
    endpoints: Vec<Endpoint>,
    quorum: usize,
    connect_timeout: Duration,
    request_timeout: Duration,
    maximum_response_bytes: usize,
}

impl RpcCluster {
    pub(crate) fn new(config: &RpcQuorumConfig) -> Result<Self, MigrationError> {
        if config.endpoints.len() < 2
            || config.endpoints.len() > MAX_ENDPOINTS
            || config.quorum < 2
            || config.quorum > config.endpoints.len()
            || config.quorum.saturating_mul(2) <= config.endpoints.len()
            || !(100..=30_000).contains(&config.connect_timeout_ms)
            || !(100..=120_000).contains(&config.request_timeout_ms)
            || !(1024..=16 * 1024 * 1024).contains(&config.maximum_response_bytes)
        {
            return Err(MigrationError::Configuration);
        }
        let endpoints = config
            .endpoints
            .iter()
            .map(Endpoint::new)
            .collect::<Result<Vec<_>, _>>()?;
        let identities: BTreeSet<_> = endpoints
            .iter()
            .map(|endpoint| {
                (
                    endpoint.host.as_str(),
                    endpoint.port,
                    endpoint.path.as_str(),
                )
            })
            .collect();
        if identities.len() != endpoints.len() {
            return Err(MigrationError::Configuration);
        }
        Ok(Self {
            endpoints,
            quorum: config.quorum,
            connect_timeout: Duration::from_millis(config.connect_timeout_ms),
            request_timeout: Duration::from_millis(config.request_timeout_ms),
            maximum_response_bytes: config.maximum_response_bytes,
        })
    }

    pub(crate) fn call(&self, method: &str, parameters: Value) -> Result<Value, MigrationError> {
        if method.is_empty()
            || method.len() > 128
            || !method
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || byte == b'_')
        {
            return Err(MigrationError::Configuration);
        }
        let request_id = REQUEST_SEQUENCE.fetch_add(1, Ordering::Relaxed);
        if request_id == 0 {
            return Err(MigrationError::RpcUnavailable);
        }
        let body = serde_json::to_vec(&json!({
            "jsonrpc": "2.0",
            "id": request_id,
            "method": method,
            "params": parameters
        }))
        .map_err(|_| MigrationError::RpcUnavailable)?;
        if body.len() > 256 * 1024 {
            return Err(MigrationError::Configuration);
        }
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
                    let canonical = serde_json::to_vec(&value)
                        .map_err(|_| MigrationError::RpcResponseMismatch)?;
                    let entry = matching.entry(canonical).or_insert((0, value));
                    entry.0 = entry.0.saturating_add(1);
                }
                Err(MigrationError::RpcRateLimited {
                    retry_after_seconds,
                }) => {
                    rate_limit = Some(rate_limit.map_or(retry_after_seconds, |existing: u64| {
                        existing.max(retry_after_seconds)
                    }));
                }
                Err(MigrationError::RpcResponseMismatch) => {
                    malformed = malformed.saturating_add(1);
                }
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
            return Err(MigrationError::RpcDivergence);
        }
        if malformed >= self.quorum {
            return Err(MigrationError::RpcResponseMismatch);
        }
        if let Some(retry_after_seconds) = rate_limit {
            return Err(MigrationError::RpcRateLimited {
                retry_after_seconds,
            });
        }
        Err(MigrationError::RpcUnavailable)
    }
}

impl Endpoint {
    fn new(config: &RpcEndpointConfig) -> Result<Self, MigrationError> {
        let (mut host, port, path) = parse_url(&config.url)?;
        host.make_ascii_lowercase();
        let mut token = fs::read_to_string(&config.bearer_token_file)
            .map_err(|_| MigrationError::Configuration)?;
        while matches!(token.as_bytes().last(), Some(b'\r' | b'\n')) {
            token.pop();
        }
        if token.is_empty()
            || token.len() > 4096
            || token.bytes().any(|byte| byte <= b' ' || byte == b'\x7f')
        {
            token.zeroize();
            return Err(MigrationError::Configuration);
        }
        let certificate =
            fs::read(&config.ca_certificate_der).map_err(|_| MigrationError::Configuration)?;
        let mut roots = RootCertStore::empty();
        roots
            .add(CertificateDer::from(certificate))
            .map_err(|_| MigrationError::Configuration)?;
        let tls = ClientConfig::builder()
            .with_root_certificates(roots)
            .with_no_client_auth();
        Ok(Self {
            host,
            port,
            path,
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
    ) -> Result<Value, MigrationError> {
        let addresses: Vec<SocketAddr> = (self.host.as_str(), self.port)
            .to_socket_addrs()
            .map_err(|_| MigrationError::RpcUnavailable)?
            .take(8)
            .collect();
        if addresses.is_empty() {
            return Err(MigrationError::RpcUnavailable);
        }
        for address in addresses {
            let Ok(tcp) = TcpStream::connect_timeout(&address, connect_timeout) else {
                continue;
            };
            tcp.set_read_timeout(Some(request_timeout))
                .map_err(|_| MigrationError::RpcUnavailable)?;
            tcp.set_write_timeout(Some(request_timeout))
                .map_err(|_| MigrationError::RpcUnavailable)?;
            let server_name = ServerName::try_from(self.host.clone())
                .map_err(|_| MigrationError::Configuration)?;
            let connection = ClientConnection::new(Arc::clone(&self.tls), server_name)
                .map_err(|_| MigrationError::RpcUnavailable)?;
            let mut stream = StreamOwned::new(connection, tcp);
            write!(
                stream,
                "POST {} HTTP/1.1\r\nHost: {}\r\nAuthorization: Bearer {}\r\nAccept: application/json\r\nContent-Type: application/json\r\nUser-Agent: LayerX-Migrate/1\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                self.path,
                authority(&self.host, self.port),
                self.authorization.as_str(),
                body.len()
            )
            .map_err(|_| MigrationError::RpcUnavailable)?;
            stream
                .write_all(body)
                .and_then(|()| stream.flush())
                .map_err(|_| MigrationError::RpcUnavailable)?;
            return read_json_rpc(&mut stream, maximum_response_bytes, request_id);
        }
        Err(MigrationError::RpcUnavailable)
    }
}

fn parse_url(value: &str) -> Result<(String, u16, String), MigrationError> {
    if value.is_empty()
        || value.len() > 2048
        || value.bytes().any(|byte| byte <= b' ' || byte == b'\x7f')
    {
        return Err(MigrationError::Configuration);
    }
    let rest = value
        .strip_prefix("https://")
        .ok_or(MigrationError::Configuration)?;
    let (authority, path) = rest
        .split_once('/')
        .map_or((rest, "/".to_owned()), |(authority, path)| {
            (authority, format!("/{path}"))
        });
    if authority.is_empty()
        || authority.contains(['@', '?', '#', '\\'])
        || path.contains(['#', '\\'])
    {
        return Err(MigrationError::Configuration);
    }
    let (host, port) = authority.rsplit_once(':').map_or_else(
        || Ok::<_, MigrationError>((authority.to_owned(), 443)),
        |(host, port)| {
            Ok((
                host.to_owned(),
                port.parse::<u16>()
                    .map_err(|_| MigrationError::Configuration)?,
            ))
        },
    )?;
    if !valid_dns(&host) {
        return Err(MigrationError::Configuration);
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
) -> Result<Value, MigrationError> {
    let mut response = Vec::with_capacity(4096);
    let mut buffer = [0_u8; 4096];
    let header_end = loop {
        let read = stream
            .read(&mut buffer)
            .map_err(|_| MigrationError::RpcUnavailable)?;
        if read == 0 || response.len().saturating_add(read) > maximum_response_bytes {
            return Err(MigrationError::RpcResponseMismatch);
        }
        response.extend_from_slice(&buffer[..read]);
        if let Some(position) = response.windows(4).position(|window| window == b"\r\n\r\n") {
            if position.saturating_add(4) > MAX_HEADER_BYTES {
                return Err(MigrationError::RpcResponseMismatch);
            }
            break position.saturating_add(4);
        }
    };
    let header = std::str::from_utf8(&response[..header_end])
        .map_err(|_| MigrationError::RpcResponseMismatch)?;
    let mut lines = header.split("\r\n");
    let status_line = lines.next().ok_or(MigrationError::RpcResponseMismatch)?;
    let mut status_parts = status_line.split_whitespace();
    if !matches!(status_parts.next(), Some("HTTP/1.1" | "HTTP/1.0")) {
        return Err(MigrationError::RpcResponseMismatch);
    }
    let status = status_parts
        .next()
        .ok_or(MigrationError::RpcResponseMismatch)?
        .parse::<u16>()
        .map_err(|_| MigrationError::RpcResponseMismatch)?;
    let mut headers = BTreeMap::new();
    for line in lines.filter(|line| !line.is_empty()) {
        let (name, value) = line
            .split_once(':')
            .ok_or(MigrationError::RpcResponseMismatch)?;
        let name = name.trim().to_ascii_lowercase();
        if name.is_empty() || headers.contains_key(&name) {
            return Err(MigrationError::RpcResponseMismatch);
        }
        headers.insert(name, value.trim().to_owned());
    }
    if status == 429 {
        let retry_after_seconds = headers
            .get("retry-after")
            .and_then(|value| value.parse::<u64>().ok())
            .unwrap_or(DEFAULT_RETRY_SECONDS)
            .clamp(1, 3600);
        return Err(MigrationError::RpcRateLimited {
            retry_after_seconds,
        });
    }
    if !(200..300).contains(&status)
        || !headers
            .get("content-type")
            .is_some_and(|value| value.to_ascii_lowercase().starts_with("application/json"))
    {
        return Err(MigrationError::RpcUnavailable);
    }
    let mut wire_body = response[header_end..].to_vec();
    while wire_body.len().saturating_add(header_end) < maximum_response_bytes {
        let read = stream
            .read(&mut buffer)
            .map_err(|_| MigrationError::RpcUnavailable)?;
        if read == 0 {
            break;
        }
        wire_body.extend_from_slice(&buffer[..read]);
    }
    if wire_body.len().saturating_add(header_end) > maximum_response_bytes {
        return Err(MigrationError::RpcResponseMismatch);
    }
    let body = match headers.get("transfer-encoding") {
        Some(value) if value.eq_ignore_ascii_case("chunked") => decode_chunked(&wire_body)?,
        Some(_) => return Err(MigrationError::RpcResponseMismatch),
        None => {
            let length = headers
                .get("content-length")
                .ok_or(MigrationError::RpcResponseMismatch)?
                .parse::<usize>()
                .map_err(|_| MigrationError::RpcResponseMismatch)?;
            if length != wire_body.len() {
                return Err(MigrationError::RpcResponseMismatch);
            }
            wire_body
        }
    };
    let envelope: Value =
        serde_json::from_slice(&body).map_err(|_| MigrationError::RpcResponseMismatch)?;
    if envelope.get("jsonrpc").and_then(Value::as_str) != Some("2.0")
        || envelope.get("id").and_then(Value::as_u64) != Some(request_id)
    {
        return Err(MigrationError::RpcResponseMismatch);
    }
    if let Some(error) = envelope.get("error") {
        let code = error.get("code").and_then(Value::as_i64);
        if matches!(code, Some(-32005 | -32016 | -32429)) {
            return Err(MigrationError::RpcRateLimited {
                retry_after_seconds: DEFAULT_RETRY_SECONDS,
            });
        }
        return Err(MigrationError::RpcUnavailable);
    }
    envelope
        .get("result")
        .cloned()
        .ok_or(MigrationError::RpcResponseMismatch)
}

fn decode_chunked(wire: &[u8]) -> Result<Vec<u8>, MigrationError> {
    let mut offset = 0_usize;
    let mut body = Vec::new();
    loop {
        let line_end = wire[offset..]
            .windows(2)
            .position(|window| window == b"\r\n")
            .map(|position| offset.saturating_add(position))
            .ok_or(MigrationError::RpcResponseMismatch)?;
        let line = std::str::from_utf8(&wire[offset..line_end])
            .map_err(|_| MigrationError::RpcResponseMismatch)?;
        let length = usize::from_str_radix(
            line.split(';')
                .next()
                .ok_or(MigrationError::RpcResponseMismatch)?,
            16,
        )
        .map_err(|_| MigrationError::RpcResponseMismatch)?;
        offset = line_end.saturating_add(2);
        if length == 0 {
            if wire.get(offset..offset.saturating_add(2)) != Some(b"\r\n") {
                return Err(MigrationError::RpcResponseMismatch);
            }
            return Ok(body);
        }
        let end = offset
            .checked_add(length)
            .ok_or(MigrationError::RpcResponseMismatch)?;
        let chunk = wire
            .get(offset..end)
            .ok_or(MigrationError::RpcResponseMismatch)?;
        body.extend_from_slice(chunk);
        if wire.get(end..end.saturating_add(2)) != Some(b"\r\n") {
            return Err(MigrationError::RpcResponseMismatch);
        }
        offset = end.saturating_add(2);
    }
}
