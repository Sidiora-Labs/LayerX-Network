//! Bounded blocking HTTP/1.1 client for the indexer's three upstreams: the
//! relay/archive sync API, Paxeer EVM JSON-RPC and CometBFT JSON-RPC.
//!
//! The framing rules follow `layerx-paxeer-verifier`'s hand-rolled client:
//! exactly one of `Content-Length` or chunked encoding, bounded headers,
//! bounded bodies, `Connection: close`. HTTPS is authenticated by an
//! explicitly configured trust anchor; plaintext is admitted for loopback
//! endpoints, or for in-cluster endpoints only when the operator opts in.

use std::io::{Read, Write};
use std::net::{TcpStream, ToSocketAddrs as _};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::Duration;

use rustls::pki_types::{CertificateDer, ServerName};
use rustls::{ClientConfig, ClientConnection, RootCertStore, StreamOwned};
use serde_json::Value;

use crate::IndexError;

const MAXIMUM_HEADER_BYTES: usize = 32 * 1024;
const MAXIMUM_BODY_BYTES: usize = 64 * 1024 * 1024;
const MAXIMUM_CHUNKS: usize = 1 << 20;

/// How one endpoint is authenticated.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum Security {
    /// HTTPS authenticated by exactly this DER trust anchor.
    PinnedTls(Vec<u8>),
    /// Plaintext HTTP; admitted for loopback hosts, and for any host only
    /// when `allow_remote` is set by the operator.
    Plaintext { allow_remote: bool },
}

/// One upstream origin plus base path.
#[derive(Clone, Debug)]
pub struct Endpoint {
    https: bool,
    host: String,
    port: u16,
    base: String,
    security: Security,
    timeout: Duration,
    tls: Option<Arc<ClientConfig>>,
}

static REQUEST_ID: AtomicU64 = AtomicU64::new(1);

impl Endpoint {
    /// Parses `http(s)://host[:port][/base]` and binds its security policy.
    ///
    /// # Errors
    /// Refuses credentials, queries, fragments, a scheme that does not match
    /// the security policy, and remote plaintext without opt-in.
    pub fn parse(url: &str, security: Security, timeout: Duration) -> Result<Self, IndexError> {
        let (https, rest, default_port) = if let Some(rest) = url.strip_prefix("https://") {
            (true, rest, 443)
        } else if let Some(rest) = url.strip_prefix("http://") {
            (false, rest, 80)
        } else {
            return Err(IndexError::Config(format!("{url} is not http(s)")));
        };
        if rest.is_empty()
            || rest.contains(['#', '@', '?'])
            || rest
                .bytes()
                .any(|byte| byte.is_ascii_whitespace() || byte.is_ascii_control())
        {
            return Err(IndexError::Config(format!("{url} is not a bare origin")));
        }
        let (authority, base) = match rest.find('/') {
            Some(index) => rest.split_at(index),
            None => (rest, ""),
        };
        let (host, port) = match authority.rsplit_once(':') {
            Some((host, port)) if !host.contains(':') => (
                host,
                port.parse::<u16>()
                    .ok()
                    .filter(|port| *port != 0)
                    .ok_or_else(|| IndexError::Config(format!("{url} has an invalid port")))?,
            ),
            Some(_) => return Err(IndexError::Config(format!("{url} host is ambiguous"))),
            None => (authority, default_port),
        };
        if host.is_empty() {
            return Err(IndexError::Config(format!("{url} has no host")));
        }
        let tls = match (&security, https) {
            (Security::PinnedTls(anchor), true) => {
                let _ = rustls::crypto::ring::default_provider().install_default();
                let mut roots = RootCertStore::empty();
                roots
                    .add(CertificateDer::from(anchor.clone()))
                    .map_err(|error| IndexError::Config(format!("trust anchor: {error}")))?;
                Some(Arc::new(
                    ClientConfig::builder()
                        .with_root_certificates(roots)
                        .with_no_client_auth(),
                ))
            }
            (Security::Plaintext { allow_remote }, false) => {
                if !allow_remote && !loopback(host) {
                    return Err(IndexError::Config(format!(
                        "{url} is remote plaintext; configure a trust anchor or opt in"
                    )));
                }
                None
            }
            _ => {
                return Err(IndexError::Config(format!(
                    "{url} scheme does not match its security policy"
                )))
            }
        };
        Ok(Self {
            https,
            host: host.to_ascii_lowercase(),
            port,
            base: base.trim_end_matches('/').to_owned(),
            security,
            timeout,
            tls,
        })
    }

    /// The configured security policy.
    #[must_use]
    pub const fn security(&self) -> &Security {
        &self.security
    }

    /// GETs `path` (relative to the base path) and returns status and body.
    ///
    /// # Errors
    /// Returns [`IndexError::Source`] on transport or framing failure.
    pub fn get(&self, path: &str) -> Result<(u16, Vec<u8>), IndexError> {
        self.exchange("GET", path, &[])
    }

    /// GETs `path` and parses a 200 JSON body.
    ///
    /// # Errors
    /// Refuses a non-200 status or a non-JSON body.
    pub fn get_json(&self, path: &str) -> Result<Value, IndexError> {
        let (status, body) = self.get(path)?;
        if status != 200 {
            return Err(IndexError::Source(format!("GET {path} answered {status}")));
        }
        serde_json::from_slice(&body)
            .map_err(|error| IndexError::Decode(format!("GET {path}: {error}")))
    }

    /// Issues one JSON-RPC 2.0 call and returns its `result`.
    ///
    /// # Errors
    /// Refuses transport failures, non-200 answers, an answer bound to a
    /// different request, and returns the upstream's own error.
    pub fn rpc(&self, method: &str, params: &Value) -> Result<Value, IndexError> {
        let id = REQUEST_ID.fetch_add(1, Ordering::Relaxed);
        let body = serde_json::json!({
            "jsonrpc": "2.0",
            "id": id,
            "method": method,
            "params": params,
        })
        .to_string();
        let (status, answer) = self.exchange("POST", "", body.as_bytes())?;
        if status != 200 {
            return Err(IndexError::Source(format!(
                "{method} answered HTTP {status}"
            )));
        }
        let value: Value = serde_json::from_slice(&answer)
            .map_err(|error| IndexError::Decode(format!("{method}: {error}")))?;
        if value.get("id").and_then(Value::as_u64) != Some(id) {
            return Err(IndexError::Source(format!(
                "{method} answer is for another request"
            )));
        }
        if let Some(error) = value.get("error").filter(|error| !error.is_null()) {
            return Err(IndexError::Source(format!("{method} refused: {error}")));
        }
        value
            .get("result")
            .cloned()
            .ok_or_else(|| IndexError::Decode(format!("{method} answer has no result")))
    }

    fn exchange(
        &self,
        method: &str,
        path: &str,
        body: &[u8],
    ) -> Result<(u16, Vec<u8>), IndexError> {
        if path
            .bytes()
            .any(|byte| byte.is_ascii_whitespace() || byte.is_ascii_control())
        {
            return Err(IndexError::Source("request path is invalid".to_owned()));
        }
        let target = if self.base.is_empty() && path.is_empty() {
            "/".to_owned()
        } else {
            format!("{}{path}", self.base)
        };
        let loopback_only = matches!(
            self.security,
            Security::Plaintext {
                allow_remote: false
            }
        );
        let tcp = connect(&self.host, self.port, self.timeout, loopback_only)?;
        tcp.set_read_timeout(Some(self.timeout))
            .map_err(|error| IndexError::Source(error.to_string()))?;
        tcp.set_write_timeout(Some(self.timeout))
            .map_err(|error| IndexError::Source(error.to_string()))?;
        let host = if (self.https && self.port == 443) || (!self.https && self.port == 80) {
            self.host.clone()
        } else {
            format!("{}:{}", self.host, self.port)
        };
        let head = format!(
            "{method} {target} HTTP/1.1\r\nHost: {host}\r\nAccept: application/json\r\nContent-Type: application/json\r\nUser-Agent: layerx-indexer/1\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
            body.len()
        );
        let raw = if let Some(tls) = &self.tls {
            let name = ServerName::try_from(self.host.clone())
                .map_err(|_| IndexError::Config("TLS server name is invalid".to_owned()))?;
            let connection = ClientConnection::new(Arc::clone(tls), name)
                .map_err(|error| IndexError::Source(error.to_string()))?;
            let mut stream = StreamOwned::new(connection, tcp);
            round_trip(&mut stream, head.as_bytes(), body)?
        } else {
            let mut stream = tcp;
            round_trip(&mut stream, head.as_bytes(), body)?
        };
        split_response(&raw)
    }
}

fn loopback(host: &str) -> bool {
    host.eq_ignore_ascii_case("localhost")
        || host
            .trim_start_matches('[')
            .trim_end_matches(']')
            .parse::<std::net::IpAddr>()
            .is_ok_and(|address| address.is_loopback())
}

fn connect(
    host: &str,
    port: u16,
    timeout: Duration,
    loopback_only: bool,
) -> Result<TcpStream, IndexError> {
    let addresses: Vec<_> = (host, port)
        .to_socket_addrs()
        .map_err(|error| IndexError::Source(format!("{host}: {error}")))?
        .collect();
    if loopback_only && addresses.iter().any(|address| !address.ip().is_loopback()) {
        return Err(IndexError::Source(format!(
            "{host} does not resolve to loopback"
        )));
    }
    let mut last = None;
    for address in addresses {
        match TcpStream::connect_timeout(&address, timeout) {
            Ok(stream) => return Ok(stream),
            Err(error) => last = Some(error),
        }
    }
    Err(IndexError::Source(last.map_or_else(
        || format!("{host} resolved to no address"),
        |error| format!("{host}: {error}"),
    )))
}

fn round_trip<S: Read + Write>(
    stream: &mut S,
    head: &[u8],
    body: &[u8],
) -> Result<Vec<u8>, IndexError> {
    stream
        .write_all(head)
        .and_then(|()| stream.write_all(body))
        .and_then(|()| stream.flush())
        .map_err(|error| IndexError::Source(error.to_string()))?;
    let mut response = Vec::new();
    let mut block = [0_u8; 16 * 1024];
    loop {
        let read = match stream.read(&mut block) {
            Ok(read) => read,
            Err(error) if error.kind() == std::io::ErrorKind::UnexpectedEof => 0,
            Err(error) => return Err(IndexError::Source(error.to_string())),
        };
        if read == 0 {
            break;
        }
        if response.len().saturating_add(read) > MAXIMUM_BODY_BYTES + MAXIMUM_HEADER_BYTES {
            return Err(IndexError::Source("response exceeds its bound".to_owned()));
        }
        response.extend_from_slice(&block[..read]);
    }
    Ok(response)
}

/// Splits one complete HTTP/1.1 response into status and exact body.
///
/// # Errors
/// Refuses ambiguous framing, truncation and oversized parts.
pub fn split_response(response: &[u8]) -> Result<(u16, Vec<u8>), IndexError> {
    let malformed = || IndexError::Source("malformed HTTP response".to_owned());
    let ambiguous = || IndexError::Source("ambiguous HTTP framing".to_owned());
    let boundary = response
        .windows(4)
        .position(|window| window == b"\r\n\r\n")
        .ok_or_else(malformed)?;
    if boundary > MAXIMUM_HEADER_BYTES {
        return Err(IndexError::Source(
            "HTTP headers exceed their bound".to_owned(),
        ));
    }
    let headers = std::str::from_utf8(&response[..boundary]).map_err(|_| malformed())?;
    let raw_body = &response[boundary + 4..];
    let mut lines = headers.split("\r\n");
    let mut status_parts = lines.next().ok_or_else(malformed)?.split_ascii_whitespace();
    if status_parts.next() != Some("HTTP/1.1") {
        return Err(malformed());
    }
    let status: u16 = status_parts
        .next()
        .and_then(|code| code.parse().ok())
        .ok_or_else(malformed)?;
    let mut content_length = None;
    let mut chunked = None;
    for line in lines {
        let (name, value) = line.split_once(':').ok_or_else(malformed)?;
        let value = value.trim();
        if name.eq_ignore_ascii_case("content-length") {
            if content_length.is_some() {
                return Err(ambiguous());
            }
            content_length = Some(value.parse::<usize>().map_err(|_| malformed())?);
        } else if name.eq_ignore_ascii_case("transfer-encoding") {
            if chunked.is_some() {
                return Err(ambiguous());
            }
            chunked = Some(value.eq_ignore_ascii_case("chunked"));
        }
    }
    let body = match (content_length, chunked) {
        (Some(length), None) => {
            if length > MAXIMUM_BODY_BYTES || raw_body.len() != length {
                return Err(ambiguous());
            }
            raw_body.to_vec()
        }
        (None, Some(true)) => decode_chunked(raw_body)?,
        _ => return Err(ambiguous()),
    };
    Ok((status, body))
}

fn decode_chunked(raw: &[u8]) -> Result<Vec<u8>, IndexError> {
    let malformed = || IndexError::Source("malformed chunked body".to_owned());
    let mut output = Vec::new();
    let mut rest = raw;
    for _ in 0..MAXIMUM_CHUNKS {
        let line_end = rest
            .windows(2)
            .position(|window| window == b"\r\n")
            .ok_or_else(malformed)?;
        let size_text = std::str::from_utf8(&rest[..line_end]).map_err(|_| malformed())?;
        if size_text.is_empty() || size_text.contains(';') {
            return Err(malformed());
        }
        let size = usize::from_str_radix(size_text, 16).map_err(|_| malformed())?;
        rest = &rest[line_end + 2..];
        if size == 0 {
            return if rest == b"\r\n" {
                Ok(output)
            } else {
                Err(malformed())
            };
        }
        if size > MAXIMUM_BODY_BYTES.saturating_sub(output.len()) {
            return Err(IndexError::Source(
                "chunked body exceeds its bound".to_owned(),
            ));
        }
        output.extend_from_slice(rest.get(..size).ok_or_else(malformed)?);
        rest = rest
            .get(size..)
            .and_then(|tail| tail.strip_prefix(b"\r\n"))
            .ok_or_else(malformed)?;
    }
    Err(IndexError::Source("too many chunks".to_owned()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn endpoints_refuse_remote_plaintext_without_opt_in() {
        let timeout = Duration::from_secs(1);
        assert!(Endpoint::parse(
            "http://127.0.0.1:8545",
            Security::Plaintext {
                allow_remote: false
            },
            timeout
        )
        .is_ok());
        assert!(Endpoint::parse(
            "http://paxeer-node:8545",
            Security::Plaintext {
                allow_remote: false
            },
            timeout
        )
        .is_err());
        assert!(Endpoint::parse(
            "http://paxeer-node:8545",
            Security::Plaintext { allow_remote: true },
            timeout
        )
        .is_ok());
        assert!(Endpoint::parse(
            "https://paxeer-node",
            Security::Plaintext { allow_remote: true },
            timeout
        )
        .is_err());
        assert!(Endpoint::parse(
            "http://user@127.0.0.1",
            Security::Plaintext {
                allow_remote: false
            },
            timeout
        )
        .is_err());
    }

    #[test]
    fn framing_is_exact() {
        let framed = b"HTTP/1.1 200 OK\r\nContent-Length: 2\r\n\r\n{}";
        assert_eq!(
            split_response(framed).unwrap_or((0, Vec::new())),
            (200, b"{}".to_vec())
        );
        let chunked = b"HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\n\r\n2\r\n{}\r\n0\r\n\r\n";
        assert_eq!(
            split_response(chunked).unwrap_or((0, Vec::new())),
            (200, b"{}".to_vec())
        );
        let both = b"HTTP/1.1 200 OK\r\nContent-Length: 2\r\nTransfer-Encoding: chunked\r\n\r\n{}";
        assert!(split_response(both).is_err());
        let short = b"HTTP/1.1 200 OK\r\nContent-Length: 3\r\n\r\n{}";
        assert!(split_response(short).is_err());
    }
}
