use crate::pay_timing;
use native_tls::{Certificate, Identity, TlsConnector, TlsStream};
use std::collections::BTreeMap;
use std::io::{Read, Write};
use std::net::{IpAddr, TcpStream, ToSocketAddrs};
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant};
use zeroize::Zeroize;

const CONNECT_TIMEOUT: Duration = Duration::from_secs(3);
const IO_TIMEOUT: Duration = Duration::from_secs(8);
const MAX_HEADERS: usize = 32 * 1024;
const MAX_RESPONSE: usize = 8 * 1024 * 1024;
const MAX_IDLE_CONNECTIONS_PER_ENDPOINT: usize = 8;

#[derive(Clone)]
pub struct Endpoint {
    pub host: String,
    pub port: u16,
    pub base_path: String,
}

impl Endpoint {
    /// # Errors
    /// Refuses noncanonical HTTPS endpoints or invalid DNS names and ports.
    pub fn parse(value: &str) -> Result<Self, String> {
        let rest = value
            .strip_prefix("https://")
            .ok_or_else(|| "component endpoint must use HTTPS".to_owned())?;
        let (authority, path) = rest
            .split_once('/')
            .map_or((rest, ""), |(authority, path)| (authority, path));
        if authority.is_empty()
            || authority.contains(['@', '?', '#', '\\'])
            || path.contains(['?', '#', '\\'])
        {
            return Err("component endpoint is not canonical".to_owned());
        }
        let (host, port) = authority.rsplit_once(':').map_or_else(
            || Ok::<_, String>((authority.to_owned(), 443)),
            |(host, port)| {
                Ok((
                    host.to_owned(),
                    port.parse::<u16>()
                        .map_err(|_| "component endpoint port is invalid".to_owned())?,
                ))
            },
        )?;
        if host.is_empty() || host.parse::<IpAddr>().is_ok() {
            return Err("component TLS endpoint must use a DNS name".to_owned());
        }
        let base_path = if path.is_empty() {
            String::new()
        } else {
            format!("/{}", path.trim_end_matches('/'))
        };
        Ok(Self {
            host,
            port,
            base_path,
        })
    }

    fn authority(&self) -> String {
        if self.port == 443 {
            self.host.clone()
        } else {
            format!("{}:{}", self.host, self.port)
        }
    }
}

pub struct Client {
    ca: Certificate,
    identity: Identity,
    connector: OnceLock<Result<TlsConnector, String>>,
    idle: Mutex<BTreeMap<String, Vec<TlsStream<TcpStream>>>>,
}

pub struct OutboundRequest<'a> {
    pub method: &'a str,
    pub path: &'a str,
    pub idempotency: Option<&'a str>,
    pub content_type: &'a str,
    pub body: &'a [u8],
}

impl Client {
    #[must_use]
    pub fn new(ca: Certificate, identity: Identity) -> Self {
        Self {
            ca,
            identity,
            connector: OnceLock::new(),
            idle: Mutex::new(BTreeMap::new()),
        }
    }

    fn connector(&self) -> Result<&TlsConnector, String> {
        match self.connector.get_or_init(|| {
            TlsConnector::builder()
                .add_root_certificate(self.ca.clone())
                .identity(self.identity.clone())
                .min_protocol_version(Some(native_tls::Protocol::Tlsv12))
                .build()
                .map_err(|error| error.to_string())
        }) {
            Ok(connector) => Ok(connector),
            Err(error) => Err(error.clone()),
        }
    }

    fn take_idle(&self, pool_key: &str) -> Result<Option<TlsStream<TcpStream>>, String> {
        self.idle
            .lock()
            .map_err(|_| "gateway HTTP connection pool is unavailable".to_owned())
            .map(|mut idle| idle.get_mut(pool_key).and_then(Vec::pop))
    }

    fn retain_idle(&self, pool_key: &str, stream: TlsStream<TcpStream>) -> Result<(), String> {
        let mut idle = self
            .idle
            .lock()
            .map_err(|_| "gateway HTTP connection pool is unavailable".to_owned())?;
        let connections = idle.entry(pool_key.to_owned()).or_default();
        if connections.len() < MAX_IDLE_CONNECTIONS_PER_ENDPOINT {
            connections.push(stream);
        }
        Ok(())
    }

    /// # Errors
    /// Refuses requests outside the configured bounds and TLS or HTTP failures.
    pub fn request(
        &self,
        endpoint: &Endpoint,
        bearer: &str,
        request: &OutboundRequest<'_>,
    ) -> Result<UpstreamResponse, String> {
        self.request_authorized(endpoint, &format!("Bearer {bearer}"), request)
    }

    /// Sends one bounded request with the supplied authorization value.
    ///
    /// # Errors
    /// Refuses requests outside the configured bounds and TLS or HTTP failures.
    pub fn request_authorized(
        &self,
        endpoint: &Endpoint,
        authorization: &str,
        request: &OutboundRequest<'_>,
    ) -> Result<UpstreamResponse, String> {
        self.request_authorized_traced(endpoint, authorization, request, None)
    }

    /// Propagates the ingress trace identifier unchanged across the boundary.
    ///
    /// # Errors
    /// Refuses invalid traces, requests outside bounds and TLS or HTTP failures.
    pub fn request_authorized_traced(
        &self,
        endpoint: &Endpoint,
        authorization: &str,
        request: &OutboundRequest<'_>,
        trace: Option<&str>,
    ) -> Result<UpstreamResponse, String> {
        let total_started = Instant::now();
        let path = request.path;
        let body = request.body;
        if !path.starts_with('/') || path.contains(['?', '#', '\\']) || body.len() > MAX_RESPONSE {
            return Err("outbound request exceeds its boundary".to_owned());
        }
        if authorization.is_empty()
            || authorization.len() > 4096
            || authorization
                .bytes()
                .any(|byte| matches!(byte, b'\r' | b'\n' | 0))
        {
            return Err("outbound authorization exceeds its boundary".to_owned());
        }
        if trace.is_some_and(|value| {
            value.is_empty()
                || value.len() > 64
                || value.bytes().any(|byte| matches!(byte, b'\r' | b'\n' | 0))
        }) {
            return Err("outbound trace exceeds its boundary".to_owned());
        }
        let connector_started = Instant::now();
        let connector = self.connector()?;
        pay_timing("gateway.http.connector", connector_started);
        let pool_key = format!("{}:{}", endpoint.host, endpoint.port);
        let pool_started = Instant::now();
        let pooled = self.take_idle(&pool_key)?;
        pay_timing("gateway.http.pool", pool_started);
        if let Some(mut stream) = pooled {
            let exchange_started = Instant::now();
            let result = exchange(&mut stream, endpoint, authorization, request, trace);
            pay_timing("gateway.http.exchange", exchange_started);
            if result
                .as_ref()
                .is_ok_and(|response| !response.connection_close)
            {
                self.retain_idle(&pool_key, stream)?;
            }
            pay_timing("gateway.http.total", total_started);
            return result;
        }
        let resolve_started = Instant::now();
        let addresses = (endpoint.host.as_str(), endpoint.port)
            .to_socket_addrs()
            .map_err(|error| error.to_string())?;
        pay_timing("gateway.http.resolve", resolve_started);
        let mut last_error = None;
        for address in addresses.take(8) {
            let connect_started = Instant::now();
            match TcpStream::connect_timeout(&address, CONNECT_TIMEOUT) {
                Ok(tcp) => {
                    pay_timing("gateway.http.tcp_connect", connect_started);
                    tcp.set_nodelay(true).map_err(|error| error.to_string())?;
                    tcp.set_read_timeout(Some(IO_TIMEOUT))
                        .map_err(|error| error.to_string())?;
                    tcp.set_write_timeout(Some(IO_TIMEOUT))
                        .map_err(|error| error.to_string())?;
                    let tls_started = Instant::now();
                    let mut stream = connector
                        .connect(&endpoint.host, tcp)
                        .map_err(|error| error.to_string())?;
                    pay_timing("gateway.http.tls_handshake", tls_started);
                    let exchange_started = Instant::now();
                    let result = exchange(&mut stream, endpoint, authorization, request, trace);
                    pay_timing("gateway.http.exchange", exchange_started);
                    if result
                        .as_ref()
                        .is_ok_and(|response| !response.connection_close)
                    {
                        self.retain_idle(&pool_key, stream)?;
                    }
                    pay_timing("gateway.http.total", total_started);
                    return result;
                }
                Err(error) => {
                    pay_timing("gateway.http.tcp_connect", connect_started);
                    last_error = Some(error);
                }
            }
        }
        Err(last_error.map_or_else(
            || "component endpoint did not resolve".to_owned(),
            |error| error.to_string(),
        ))
    }
}

pub struct IncomingRequest {
    pub method: String,
    pub path: String,
    pub headers: BTreeMap<String, String>,
    pub body: Vec<u8>,
}

impl Drop for IncomingRequest {
    fn drop(&mut self) {
        for value in self.headers.values_mut() {
            value.zeroize();
        }
        self.body.zeroize();
    }
}

pub struct OutgoingResponse {
    pub status: u16,
    pub body: Vec<u8>,
    pub retry_after: Option<u64>,
}

pub struct UpstreamResponse {
    pub status: u16,
    pub content_type: String,
    pub body: Vec<u8>,
    connection_close: bool,
}

fn exchange(
    stream: &mut TlsStream<TcpStream>,
    endpoint: &Endpoint,
    authorization: &str,
    request: &OutboundRequest<'_>,
    trace: Option<&str>,
) -> Result<UpstreamResponse, String> {
    let idempotency = request
        .idempotency
        .map_or_else(String::new, |key| format!("Idempotency-Key: {key}\r\n"));
    let trace = trace.map_or_else(String::new, |value| format!("X-Trace-Id: {value}\r\n"));
    let mut outbound = zeroize::Zeroizing::new(Vec::new());
    write!(
        outbound,
        "{} {}{} HTTP/1.1\r\nHost: {}\r\nAuthorization: {authorization}\r\nAccept: application/json\r\nContent-Type: {}\r\n{idempotency}{trace}Content-Length: {}\r\nConnection: keep-alive\r\n\r\n",
        request.method,
        endpoint.base_path,
        request.path,
        endpoint.authority(),
        request.content_type,
        request.body.len()
    )
    .map_err(|error| error.to_string())?;
    outbound.extend_from_slice(request.body);
    stream
        .write_all(&outbound)
        .map_err(|error| error.to_string())?;
    stream.flush().map_err(|error| error.to_string())?;
    read_response(stream)
}

/// # Errors
/// Refuses malformed, truncated or oversized HTTP requests and read failures.
pub fn read_request(stream: &mut impl Read, maximum: usize) -> Result<IncomingRequest, String> {
    let (start, headers, body) = read_message(stream, maximum)?;
    let mut parts = start.split_whitespace();
    let method = parts
        .next()
        .ok_or_else(|| "request method is missing".to_owned())?;
    let path = parts
        .next()
        .ok_or_else(|| "request target is missing".to_owned())?;
    if parts.next() != Some("HTTP/1.1")
        || parts.next().is_some()
        || !path.starts_with('/')
        || path.contains(['?', '#', '\\'])
        || !headers.contains_key("host")
    {
        return Err("request line is invalid".to_owned());
    }
    Ok(IncomingRequest {
        method: method.to_owned(),
        path: path.to_owned(),
        headers,
        body,
    })
}

fn read_response(stream: &mut impl Read) -> Result<UpstreamResponse, String> {
    let (start, headers, body) = read_message(stream, MAX_RESPONSE)?;
    let mut parts = start.split_whitespace();
    if parts.next() != Some("HTTP/1.1") {
        return Err("component response must use HTTP/1.1".to_owned());
    }
    let status = parts
        .next()
        .ok_or_else(|| "component response status is missing".to_owned())?
        .parse::<u16>()
        .map_err(|_| "component response status is invalid".to_owned())?;
    let content_type = headers.get("content-type").cloned().unwrap_or_default();
    let connection_close = headers
        .get("connection")
        .is_some_and(|value| value.eq_ignore_ascii_case("close"));
    Ok(UpstreamResponse {
        status,
        content_type,
        body,
        connection_close,
    })
}

type HttpMessage = (String, BTreeMap<String, String>, Vec<u8>);

fn read_message(stream: &mut impl Read, maximum: usize) -> Result<HttpMessage, String> {
    let mut bytes = Vec::with_capacity(2048);
    let mut chunk = [0_u8; 2048];
    let header_end = loop {
        let count = stream.read(&mut chunk).map_err(|error| error.to_string())?;
        if count == 0 || bytes.len().saturating_add(count) > maximum {
            return Err("HTTP message is empty or exceeds its bound".to_owned());
        }
        bytes.extend_from_slice(&chunk[..count]);
        if let Some(position) = bytes.windows(4).position(|window| window == b"\r\n\r\n") {
            if position + 4 > MAX_HEADERS {
                return Err("HTTP headers exceed their bound".to_owned());
            }
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
        if name.is_empty() || headers.contains_key(&name) {
            return Err("duplicate or empty HTTP header".to_owned());
        }
        let value = value.trim().to_owned();
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
    Ok((
        start,
        headers,
        bytes[header_end..header_end + content_length].to_vec(),
    ))
}

/// # Errors
/// Returns an error when writing or flushing the response fails.
pub fn write_response(stream: &mut impl Write, response: &OutgoingResponse) -> Result<(), String> {
    write_response_connection(stream, response, false)
}

/// Writes a response with the bounded connection lifecycle selected by the server.
///
/// # Errors
/// Returns an error when writing or flushing the response fails.
pub fn write_response_connection(
    stream: &mut impl Write,
    response: &OutgoingResponse,
    keep_alive: bool,
) -> Result<(), String> {
    let reason = match response.status {
        200 => "OK",
        201 => "Created",
        202 => "Accepted",
        400 => "Bad Request",
        401 => "Unauthorized",
        403 => "Forbidden",
        404 => "Not Found",
        409 => "Conflict",
        415 => "Unsupported Media Type",
        429 => "Too Many Requests",
        503 => "Service Unavailable",
        _ => "Error",
    };
    let retry = response
        .retry_after
        .map_or_else(String::new, |value| format!("Retry-After: {value}\r\n"));
    let connection = if keep_alive { "keep-alive" } else { "close" };
    write!(
        stream,
        "HTTP/1.1 {} {reason}\r\nContent-Type: application/json\r\nCache-Control: no-store\r\nX-Content-Type-Options: nosniff\r\n{retry}Content-Length: {}\r\nConnection: {connection}\r\n\r\n",
        response.status,
        response.body.len()
    )
    .map_err(|error| error.to_string())?;
    stream
        .write_all(&response.body)
        .map_err(|error| error.to_string())?;
    stream.flush().map_err(|error| error.to_string())
}
