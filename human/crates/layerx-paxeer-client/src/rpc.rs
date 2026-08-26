use std::io::{Read, Write};
use std::net::{TcpStream, ToSocketAddrs as _};
use std::sync::Arc;
use std::time::Duration;

use rustls::pki_types::{CertificateDer, ServerName};
use rustls::{ClientConfig, ClientConnection, RootCertStore, StreamOwned};

use crate::json::{self, Json};

const MAXIMUM_HEADER_BYTES: usize = 32 * 1024;
const MAXIMUM_BODY_BYTES: usize = 2 * 1024 * 1024;
const MAXIMUM_WIRE_BYTES: usize = 4 * 1024 * 1024;
const MAXIMUM_CHUNKS: usize = 4_096;

/// Authentication policy for one Paxeer endpoint.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum EndpointTransport {
    /// Production TLS authenticated by an explicitly configured trust anchor.
    PinnedTls { trust_anchor_der: Vec<u8> },
    /// Plaintext transport restricted to an explicit loopback emulator.
    LocalEmulator,
}

/// One declared Paxeer JSON-RPC endpoint with its trust and network binding.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct EndpointConfig {
    pub url: String,
    pub request_timeout: Duration,
    pub transport: EndpointTransport,
    pub expected_chain_id: u64,
}

/// Typed reason one endpoint could not serve one request.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum EndpointFault {
    UnsupportedUrl,
    InsecureTransport,
    InvalidTrustAnchor,
    Authentication { detail: String },
    Connect { detail: String },
    Transport { detail: String },
    Http { status: u16 },
    ResponseTooLarge,
    AmbiguousFraming,
    MalformedResponse,
    Rpc { code: i64, message: String },
    ChainMismatch { expected: u64, actual: u64 },
    InconsistentObservation,
    UnexpectedValue { detail: String },
}

/// One endpoint's failure, naming the endpoint it came from.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct EndpointFailure {
    pub url: String,
    pub fault: EndpointFault,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum Scheme {
    Http,
    Https,
}

pub(crate) struct Target {
    scheme: Scheme,
    host: String,
    port: u16,
    path: String,
}

pub(crate) fn validate_endpoint(endpoint: &EndpointConfig) -> Result<Target, EndpointFault> {
    let target = parse_url(&endpoint.url).ok_or(EndpointFault::UnsupportedUrl)?;
    match (&endpoint.transport, target.scheme) {
        (EndpointTransport::LocalEmulator, Scheme::Http) if loopback(&target.host) => {}
        (EndpointTransport::PinnedTls { trust_anchor_der }, Scheme::Https) => {
            trust_roots(trust_anchor_der)?;
        }
        _ => return Err(EndpointFault::InsecureTransport),
    }
    if endpoint.expected_chain_id == 0 {
        return Err(EndpointFault::UnexpectedValue {
            detail: "expected chain identifier is zero".to_owned(),
        });
    }
    Ok(target)
}

pub(crate) fn canonical_endpoint_identity(
    endpoint: &EndpointConfig,
) -> Result<String, EndpointFault> {
    let target = validate_endpoint(endpoint)?;
    let host = target.host.parse::<std::net::IpAddr>().map_or_else(
        |_| target.host.trim_end_matches('.').to_ascii_lowercase(),
        |address| address.to_string(),
    );
    let scheme = match target.scheme {
        Scheme::Http => "http",
        Scheme::Https => "https",
    };
    Ok(format!(
        "{scheme}://{host}:{}{}",
        target.port, target.path
    ))
}

fn trust_roots(trust_anchor_der: &[u8]) -> Result<RootCertStore, EndpointFault> {
    if trust_anchor_der.is_empty() {
        return Err(EndpointFault::InvalidTrustAnchor);
    }
    let mut roots = RootCertStore::empty();
    roots
        .add(CertificateDer::from(trust_anchor_der.to_vec()))
        .map_err(|_| EndpointFault::InvalidTrustAnchor)?;
    Ok(roots)
}

fn parse_url(url: &str) -> Option<Target> {
    let (scheme, rest, default_port) = if let Some(rest) = url.strip_prefix("https://") {
        (Scheme::Https, rest, 443)
    } else if let Some(rest) = url.strip_prefix("http://") {
        (Scheme::Http, rest, 80)
    } else {
        return None;
    };
    if rest.is_empty()
        || rest.contains('#')
        || rest.contains('@')
        || rest.contains('?')
        || rest.bytes().any(|byte| byte.is_ascii_whitespace())
    {
        return None;
    }
    let (authority, path) = match rest.find('/') {
        Some(index) => rest.split_at(index),
        None => (rest, "/"),
    };
    if authority.contains('[')
        || authority.contains(']')
        || authority.matches(':').count() > 1
    {
        return None;
    }
    let (host, port) = match authority.rfind(':') {
        Some(index) => {
            let host = authority.get(..index)?;
            let port = authority.get(index.saturating_add(1)..)?.parse().ok()?;
            (host, port)
        }
        None => (authority, default_port),
    };
    if host.is_empty() || port == 0 {
        return None;
    }
    Some(Target {
        scheme,
        host: host.to_owned(),
        port,
        path: path.to_owned(),
    })
}

fn loopback(host: &str) -> bool {
    host.eq_ignore_ascii_case("localhost")
        || host
            .parse::<std::net::IpAddr>()
            .is_ok_and(|address| address.is_loopback())
}

/// Issues one JSON-RPC request against one configured endpoint.
///
/// # Errors
///
/// Returns the endpoint's typed transport, framing, HTTP or RPC fault.
pub fn raw_call(
    endpoint: &EndpointConfig,
    method: &str,
    params: &[Json],
) -> Result<Json, EndpointFailure> {
    let result = (|| {
        let chain = call_target(endpoint, "eth_chainId", &[])?;
        require_chain(endpoint, &chain)?;
        if method == "eth_chainId" {
            return Ok(chain);
        }
        call_target(endpoint, method, params)
    })();
    result.map_err(|fault| EndpointFailure {
        url: endpoint.url.clone(),
        fault,
    })
}

fn require_chain(endpoint: &EndpointConfig, value: &Json) -> Result<(), EndpointFault> {
    let actual = hex_quantity(value)?;
    if actual != endpoint.expected_chain_id {
        return Err(EndpointFault::ChainMismatch {
            expected: endpoint.expected_chain_id,
            actual,
        });
    }
    Ok(())
}

fn hex_quantity(value: &Json) -> Result<u64, EndpointFault> {
    let text = value
        .as_text()
        .ok_or_else(|| EndpointFault::UnexpectedValue {
            detail: "chain identifier is not text".to_owned(),
        })?;
    let digits = text
        .strip_prefix("0x")
        .filter(|digits| !digits.is_empty())
        .ok_or_else(|| EndpointFault::UnexpectedValue {
            detail: "chain identifier is not a hex quantity".to_owned(),
        })?;
    u64::from_str_radix(digits, 16).map_err(|_| EndpointFault::UnexpectedValue {
        detail: "chain identifier is out of range".to_owned(),
    })
}

fn call_target(
    endpoint: &EndpointConfig,
    method: &str,
    params: &[Json],
) -> Result<Json, EndpointFault> {
    let target = validate_endpoint(endpoint)?;
    let body = envelope(method, params);
    let response = exchange(&target, endpoint, &body)?;
    let (status, payload) = split_response(&response)?;
    if status != 200 {
        return Err(EndpointFault::Http { status });
    }
    let value = json::parse(&payload).map_err(|_| EndpointFault::MalformedResponse)?;
    if let Some(error) = value.member("error") {
        if !error.is_null() {
            let code = error.member("code").and_then(Json::as_integer).unwrap_or(0);
            let message = error
                .member("message")
                .and_then(Json::as_text)
                .unwrap_or("")
                .to_owned();
            return Err(EndpointFault::Rpc { code, message });
        }
    }
    value
        .member("result")
        .cloned()
        .ok_or(EndpointFault::MalformedResponse)
}

fn envelope(method: &str, params: &[Json]) -> String {
    Json::Object(vec![
        ("jsonrpc".to_owned(), Json::Text("2.0".to_owned())),
        ("id".to_owned(), Json::Number("1".to_owned())),
        ("method".to_owned(), Json::Text(method.to_owned())),
        ("params".to_owned(), Json::Array(params.to_vec())),
    ])
    .render()
}

fn connect(
    target: &Target,
    timeout: Duration,
    loopback_only: bool,
) -> Result<TcpStream, EndpointFault> {
    let addresses = (target.host.as_str(), target.port)
        .to_socket_addrs()
        .map_err(|error| EndpointFault::Connect {
            detail: error.to_string(),
        })?
        .collect::<Vec<_>>();
    if loopback_only && addresses.iter().any(|address| !address.ip().is_loopback()) {
        return Err(EndpointFault::InsecureTransport);
    }
    let mut last = None;
    for address in addresses {
        match TcpStream::connect_timeout(&address, timeout) {
            Ok(stream) => return Ok(stream),
            Err(error) => last = Some(error),
        }
    }
    Err(EndpointFault::Connect {
        detail: last.map_or_else(
            || "no resolved addresses".to_owned(),
            |error| error.to_string(),
        ),
    })
}

fn exchange(
    target: &Target,
    endpoint: &EndpointConfig,
    body: &str,
) -> Result<Vec<u8>, EndpointFault> {
    let loopback_only = matches!(&endpoint.transport, EndpointTransport::LocalEmulator);
    let mut tcp = connect(target, endpoint.request_timeout, loopback_only)?;
    tcp.set_read_timeout(Some(endpoint.request_timeout))
        .map_err(transport)?;
    tcp.set_write_timeout(Some(endpoint.request_timeout))
        .map_err(transport)?;
    match &endpoint.transport {
        EndpointTransport::LocalEmulator => exchange_stream(&mut tcp, target, body),
        EndpointTransport::PinnedTls { trust_anchor_der } => {
            let roots = trust_roots(trust_anchor_der)?;
            let configuration = ClientConfig::builder()
                .with_root_certificates(roots)
                .with_no_client_auth();
            let server_name = ServerName::try_from(target.host.clone())
                .map_err(|_| EndpointFault::InvalidTrustAnchor)?;
            let connection = ClientConnection::new(Arc::new(configuration), server_name)
                .map_err(|error| EndpointFault::Authentication {
                    detail: error.to_string(),
                })?;
            let mut tls = StreamOwned::new(connection, tcp);
            exchange_stream(&mut tls, target, body)
        }
    }
}

fn transport(error: std::io::Error) -> EndpointFault {
    EndpointFault::Transport {
        detail: error.to_string(),
    }
}

fn exchange_stream<S: Read + Write>(
    stream: &mut S,
    target: &Target,
    body: &str,
) -> Result<Vec<u8>, EndpointFault> {
    let request = format!(
        "POST {} HTTP/1.1\r\nHost: {}:{}\r\nContent-Type: application/json\r\nAccept: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
        target.path,
        target.host,
        target.port,
        body.len(),
        body
    );
    stream.write_all(request.as_bytes()).map_err(transport)?;
    stream.flush().map_err(transport)?;
    let mut response = Vec::new();
    let mut block = [0_u8; 8_192];
    loop {
        let read = stream.read(&mut block).map_err(transport)?;
        if read == 0 {
            break;
        }
        if response.len().saturating_add(read) > MAXIMUM_WIRE_BYTES {
            return Err(EndpointFault::ResponseTooLarge);
        }
        response.extend_from_slice(&block[..read]);
        if !response.windows(4).any(|window| window == b"\r\n\r\n")
            && response.len() > MAXIMUM_HEADER_BYTES
        {
            return Err(EndpointFault::ResponseTooLarge);
        }
    }
    Ok(response)
}

fn split_response(response: &[u8]) -> Result<(u16, String), EndpointFault> {
    let boundary = response
        .windows(4)
        .position(|window| window == b"\r\n\r\n")
        .ok_or(EndpointFault::MalformedResponse)?;
    if boundary > MAXIMUM_HEADER_BYTES {
        return Err(EndpointFault::ResponseTooLarge);
    }
    let header_bytes = response
        .get(..boundary)
        .ok_or(EndpointFault::MalformedResponse)?;
    let raw_body = response
        .get(boundary.saturating_add(4)..)
        .ok_or(EndpointFault::MalformedResponse)?;
    let headers =
        std::str::from_utf8(header_bytes).map_err(|_| EndpointFault::MalformedResponse)?;
    let mut lines = headers.split("\r\n");
    let status_line = lines.next().ok_or(EndpointFault::MalformedResponse)?;
    let mut status_parts = status_line.split_ascii_whitespace();
    if status_parts.next() != Some("HTTP/1.1") {
        return Err(EndpointFault::MalformedResponse);
    }
    let status = status_parts
        .next()
        .and_then(|code| code.parse().ok())
        .ok_or(EndpointFault::MalformedResponse)?;
    if status_parts.next().is_none() {
        return Err(EndpointFault::MalformedResponse);
    }
    let mut content_length = None;
    let mut transfer_encoding = None;
    for line in lines {
        let (name, value) = line
            .split_once(':')
            .ok_or(EndpointFault::MalformedResponse)?;
        let value = value.trim();
        if name.eq_ignore_ascii_case("content-length") {
            if content_length.is_some() {
                return Err(EndpointFault::AmbiguousFraming);
            }
            content_length = Some(
                value
                    .parse::<usize>()
                    .map_err(|_| EndpointFault::MalformedResponse)?,
            );
        } else if name.eq_ignore_ascii_case("transfer-encoding") {
            if transfer_encoding.is_some() {
                return Err(EndpointFault::AmbiguousFraming);
            }
            transfer_encoding = Some(value);
        }
    }
    if content_length.is_some() && transfer_encoding.is_some() {
        return Err(EndpointFault::AmbiguousFraming);
    }
    let body_bytes = match (content_length, transfer_encoding) {
        (Some(length), None) => {
            if length > MAXIMUM_BODY_BYTES {
                return Err(EndpointFault::ResponseTooLarge);
            }
            if raw_body.len() != length {
                return Err(EndpointFault::AmbiguousFraming);
            }
            raw_body.to_vec()
        }
        (None, Some(value)) if value.eq_ignore_ascii_case("chunked") => {
            decode_chunked(raw_body)?
        }
        _ => return Err(EndpointFault::AmbiguousFraming),
    };
    String::from_utf8(body_bytes)
        .map_err(|_| EndpointFault::MalformedResponse)
        .map(|payload| (status, payload))
}

fn decode_chunked(raw: &[u8]) -> Result<Vec<u8>, EndpointFault> {
    let mut output = Vec::new();
    let mut rest = raw;
    let mut chunks = 0_usize;
    loop {
        chunks = chunks.saturating_add(1);
        if chunks > MAXIMUM_CHUNKS {
            return Err(EndpointFault::ResponseTooLarge);
        }
        let line_end = rest
            .windows(2)
            .position(|window| window == b"\r\n")
            .ok_or(EndpointFault::MalformedResponse)?;
        let size_text = rest
            .get(..line_end)
            .and_then(|bytes| std::str::from_utf8(bytes).ok())
            .ok_or(EndpointFault::MalformedResponse)?;
        if size_text.is_empty() || size_text.contains(';') {
            return Err(EndpointFault::AmbiguousFraming);
        }
        let size = usize::from_str_radix(size_text, 16)
            .map_err(|_| EndpointFault::MalformedResponse)?;
        rest = rest
            .get(line_end.saturating_add(2)..)
            .ok_or(EndpointFault::MalformedResponse)?;
        if size == 0 {
            return if rest == b"\r\n" {
                Ok(output)
            } else {
                Err(EndpointFault::AmbiguousFraming)
            };
        }
        if size > MAXIMUM_BODY_BYTES.saturating_sub(output.len()) {
            return Err(EndpointFault::ResponseTooLarge);
        }
        let chunk = rest.get(..size).ok_or(EndpointFault::MalformedResponse)?;
        output.extend_from_slice(chunk);
        rest = rest
            .get(size..)
            .ok_or(EndpointFault::MalformedResponse)?;
        rest = rest
            .strip_prefix(b"\r\n")
            .ok_or(EndpointFault::MalformedResponse)?;
    }
}
