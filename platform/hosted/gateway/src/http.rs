use native_tls::{Certificate, Identity, TlsConnector};
use std::collections::BTreeMap;
use std::io::{Read, Write};
use std::net::{IpAddr, TcpStream, ToSocketAddrs};
use std::time::Duration;
use zeroize::Zeroize;

const CONNECT_TIMEOUT: Duration = Duration::from_secs(3);
const IO_TIMEOUT: Duration = Duration::from_secs(8);
const MAX_HEADERS: usize = 32 * 1024;
const MAX_RESPONSE: usize = 512 * 1024;

#[derive(Clone)]
pub struct Endpoint {
    pub host: String,
    pub port: u16,
    pub base_path: String,
}

impl Endpoint {
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
}

impl Client {
    pub fn new(ca: Certificate, identity: Identity) -> Self {
        Self { ca, identity }
    }

    pub fn request(
        &self,
        endpoint: &Endpoint,
        method: &str,
        path: &str,
        bearer: &str,
        idempotency: Option<&str>,
        content_type: &str,
        body: &[u8],
    ) -> Result<UpstreamResponse, String> {
        if !path.starts_with('/') || path.contains(['?', '#', '\\']) || body.len() > MAX_RESPONSE {
            return Err("outbound request exceeds its boundary".to_owned());
        }
        let connector = TlsConnector::builder()
            .add_root_certificate(self.ca.clone())
            .identity(self.identity.clone())
            .min_protocol_version(Some(native_tls::Protocol::Tlsv12))
            .build()
            .map_err(|error| error.to_string())?;
        let mut last_error = None;
        for address in (endpoint.host.as_str(), endpoint.port)
            .to_socket_addrs()
            .map_err(|error| error.to_string())?
            .take(8)
        {
            match TcpStream::connect_timeout(&address, CONNECT_TIMEOUT) {
                Ok(tcp) => {
                    tcp.set_read_timeout(Some(IO_TIMEOUT))
                        .map_err(|error| error.to_string())?;
                    tcp.set_write_timeout(Some(IO_TIMEOUT))
                        .map_err(|error| error.to_string())?;
                    let mut stream = connector
                        .connect(&endpoint.host, tcp)
                        .map_err(|error| error.to_string())?;
                    let idempotency = idempotency
                        .map_or_else(String::new, |key| format!("Idempotency-Key: {key}\r\n"));
                    write!(
                        stream,
                        "{method} {}{path} HTTP/1.1\r\nHost: {}\r\nAuthorization: Bearer {bearer}\r\nAccept: application/json\r\nContent-Type: {content_type}\r\n{idempotency}Content-Length: {}\r\nConnection: close\r\n\r\n",
                        endpoint.base_path,
                        endpoint.authority(),
                        body.len()
                    )
                    .map_err(|error| error.to_string())?;
                    stream.write_all(body).map_err(|error| error.to_string())?;
                    stream.flush().map_err(|error| error.to_string())?;
                    return read_response(&mut stream);
                }
                Err(error) => last_error = Some(error),
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
}

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
    Ok(UpstreamResponse {
        status,
        content_type,
        body,
    })
}

fn read_message(
    stream: &mut impl Read,
    maximum: usize,
) -> Result<(String, BTreeMap<String, String>, Vec<u8>), String> {
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

pub fn write_response(stream: &mut impl Write, response: &OutgoingResponse) -> Result<(), String> {
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
    write!(
        stream,
        "HTTP/1.1 {} {reason}\r\nContent-Type: application/json\r\nCache-Control: no-store\r\nX-Content-Type-Options: nosniff\r\n{retry}Content-Length: {}\r\nConnection: close\r\n\r\n",
        response.status,
        response.body.len()
    )
    .map_err(|error| error.to_string())?;
    stream
        .write_all(&response.body)
        .map_err(|error| error.to_string())?;
    stream.flush().map_err(|error| error.to_string())
}
