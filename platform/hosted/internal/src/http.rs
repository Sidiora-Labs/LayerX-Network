//! Bounded HTTP/1.1 over a blocking TLS stream in the hosted house style:
//! one request per connection, exact bodies, refusal envelopes and
//! `Connection: close`.

use serde::Serialize;
use std::collections::BTreeMap;
use std::io::{Read, Write};
use std::net::{SocketAddr, TcpListener, TcpStream};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use std::thread;
use std::time::Duration;

use rustls::{ServerConfig, ServerConnection, StreamOwned};
use subtle::ConstantTimeEq;

/// Largest request a service reads, headers and body together.
pub const MAX_REQUEST_BYTES: usize = 96 * 1024;
/// Socket read and write timeout.
pub const IO_TIMEOUT: Duration = Duration::from_secs(8);
/// Concurrent connections a service serves.
pub const MAX_CONNECTIONS: usize = 128;

static ACTIVE_CONNECTIONS: AtomicUsize = AtomicUsize::new(0);

/// A parsed request.
pub struct Request {
    pub method: String,
    pub path: String,
    pub headers: BTreeMap<String, String>,
    pub body: Vec<u8>,
    /// True when the TLS peer presented a certificate the configured client
    /// CA verified.
    pub peer_verified: bool,
}

impl Request {
    /// Returns the bearer token when the request carries exactly one.
    #[must_use]
    pub fn bearer(&self) -> Option<&str> {
        let value = self.headers.get("authorization")?;
        let (scheme, token) = value.split_once(' ')?;
        if !scheme.eq_ignore_ascii_case("bearer") {
            return None;
        }
        let token = token.trim();
        (!token.is_empty() && token.len() <= 4096).then_some(token)
    }

    /// Constant-time comparison of the presented bearer token.
    #[must_use]
    pub fn bearer_matches(&self, expected: &str) -> bool {
        self.bearer().is_some_and(|presented| {
            presented.as_bytes().ct_eq(expected.as_bytes()).unwrap_u8() == 1
        })
    }

    /// True when the body is declared as JSON.
    #[must_use]
    pub fn json_body(&self) -> bool {
        self.headers
            .get("content-type")
            .is_some_and(|value| value.starts_with("application/json"))
    }
}

/// A response to write.
pub struct Response {
    pub status: u16,
    pub body: String,
    pub retry_after: Option<u64>,
}

/// A 200 with the given JSON body.
#[must_use]
pub fn ok(body: String) -> Response {
    Response {
        status: 200,
        body,
        retry_after: None,
    }
}

/// Serializes a value as a JSON response with the given status.
#[must_use]
pub fn json<T: Serialize>(status: u16, value: &T) -> Response {
    serde_json::to_string(value).map_or_else(
        |_| refusal(500, "serialization_failed", None),
        |body| Response {
            status,
            body,
            retry_after: None,
        },
    )
}

/// The hosted refusal envelope.
#[must_use]
pub fn refusal(status: u16, code: &str, retry_after: Option<u64>) -> Response {
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

/// Reads one bounded HTTP/1.1 message: no transfer encoding, no duplicate
/// headers, a body exactly as long as `Content-Length`.
///
/// # Errors
/// Returns a description when the message is malformed or exceeds `maximum`.
pub fn read_http_message(stream: &mut impl Read, maximum: usize) -> Result<Request, String> {
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
        peer_verified: false,
    })
}

/// Parses a client request line on top of [`read_http_message`]: HTTP/1.1
/// only, no query string, a Host header.
///
/// # Errors
/// Returns a description when the request is malformed.
pub fn parse_client_request(stream: &mut impl Read) -> Result<Request, String> {
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
    parts
        .next()
        .ok_or_else(|| "request target is missing".to_owned())?
        .clone_into(&mut request.path);
    if parts.next() != Some("HTTP/1.1")
        || parts.next().is_some()
        || request.path.contains('?')
        || !request.path.starts_with('/')
    {
        return Err("request line is invalid".to_owned());
    }
    if !request.headers.contains_key("host") {
        return Err("HTTP/1.1 Host header is required".to_owned());
    }
    Ok(request)
}

/// Writes a response with the hosted headers.
///
/// # Errors
/// Returns a description when the stream write fails.
pub fn write_response(stream: &mut impl Write, response: &Response) -> Result<(), String> {
    let reason = match response.status {
        200 => "OK",
        201 => "Created",
        400 => "Bad Request",
        401 => "Unauthorized",
        403 => "Forbidden",
        404 => "Not Found",
        405 => "Method Not Allowed",
        409 => "Conflict",
        413 => "Payload Too Large",
        422 => "Unprocessable Entity",
        429 => "Too Many Requests",
        500 => "Internal Server Error",
        502 => "Bad Gateway",
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

fn handle_connection<H>(tls: &Arc<ServerConfig>, handler: &H, tcp: TcpStream) -> Result<(), String>
where
    H: Fn(&Request) -> Response,
{
    tcp.set_read_timeout(Some(IO_TIMEOUT))
        .map_err(|error| error.to_string())?;
    tcp.set_write_timeout(Some(IO_TIMEOUT))
        .map_err(|error| error.to_string())?;
    let connection = ServerConnection::new(Arc::clone(tls)).map_err(|error| error.to_string())?;
    let mut stream = StreamOwned::new(connection, tcp);
    let response = parse_client_request(&mut stream).map_or_else(
        |_| refusal(400, "invalid_request", None),
        |mut request| {
            request.peer_verified = stream
                .conn
                .peer_certificates()
                .is_some_and(|chain| !chain.is_empty());
            handler(&request)
        },
    );
    write_response(&mut stream, &response)?;
    stream.flush().map_err(|error| error.to_string())?;
    stream.conn.send_close_notify();
    let _ = stream.conn.write_tls(&mut stream.sock);
    Ok(())
}

/// Serves `handler` over TLS on `listen` until the listener fails. `name`
/// labels log lines; `on_bound` receives the bound address before accepting.
///
/// # Errors
/// Returns a description when the listener cannot bind.
pub fn serve<H>(
    name: &'static str,
    listen: SocketAddr,
    tls: &Arc<ServerConfig>,
    handler: H,
) -> Result<(), String>
where
    H: Fn(&Request) -> Response + Send + Sync + 'static,
{
    let listener = TcpListener::bind(listen).map_err(|error| error.to_string())?;
    let bound = listener.local_addr().map_err(|error| error.to_string())?;
    let handler = Arc::new(handler);
    eprintln!("{name} listening on {bound} with TLS");
    for connection in listener.incoming() {
        match connection {
            Ok(stream) => {
                let Some(permit) = ConnectionPermit::acquire() else {
                    continue;
                };
                let tls = Arc::clone(tls);
                let handler = Arc::clone(&handler);
                thread::spawn(move || {
                    let _permit = permit;
                    if let Err(error) = handle_connection(&tls, handler.as_ref(), stream) {
                        eprintln!("{name} connection failed: {error}");
                    }
                });
            }
            Err(error) => eprintln!("{name} accept failed: {error}"),
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;

    #[test]
    fn request_parser_rejects_unbounded_and_ambiguous_messages() {
        let mut plain = Cursor::new(
            b"POST /v1/signatures HTTP/1.1\r\nHost: kms\r\nAuthorization: Bearer abc\r\nContent-Type: application/json\r\nContent-Length: 2\r\n\r\n{}".to_vec(),
        );
        let request = parse_client_request(&mut plain).unwrap_or_else(|error| panic!("{error}"));
        assert_eq!(request.method, "POST");
        assert_eq!(request.path, "/v1/signatures");
        assert_eq!(request.body, b"{}");
        assert_eq!(request.bearer(), Some("abc"));
        assert!(request.bearer_matches("abc"));
        assert!(!request.bearer_matches("abd"));
        assert!(request.json_body());
        assert!(!request.peer_verified);

        let mut query = Cursor::new(b"GET /readyz?x=1 HTTP/1.1\r\nHost: kms\r\n\r\n".to_vec());
        assert!(parse_client_request(&mut query).is_err());
        let mut chunked = Cursor::new(
            b"POST /v1 HTTP/1.1\r\nHost: kms\r\nTransfer-Encoding: chunked\r\n\r\n".to_vec(),
        );
        assert!(parse_client_request(&mut chunked).is_err());
        let mut duplicate =
            Cursor::new(b"GET /readyz HTTP/1.1\r\nHost: kms\r\nHost: other\r\n\r\n".to_vec());
        assert!(parse_client_request(&mut duplicate).is_err());
        let mut no_host = Cursor::new(b"GET /readyz HTTP/1.1\r\n\r\n".to_vec());
        assert!(parse_client_request(&mut no_host).is_err());
        let mut oversized = Cursor::new(format!(
            "POST /v1 HTTP/1.1\r\nHost: kms\r\nContent-Length: {}\r\n\r\n",
            MAX_REQUEST_BYTES + 1
        ));
        assert!(parse_client_request(&mut oversized).is_err());
        let mut http10 = Cursor::new(b"GET /readyz HTTP/1.0\r\nHost: kms\r\n\r\n".to_vec());
        assert!(parse_client_request(&mut http10).is_err());
    }

    #[test]
    fn refusal_bodies_follow_the_hosted_contract() {
        let never = refusal(404, "unknown_key", None);
        assert_eq!(
            never.body,
            r#"{"error":{"code":"unknown_key","retry":"never"}}"#
        );
        let after = refusal(503, "dependency_unavailable", Some(5));
        assert_eq!(
            after.body,
            r#"{"error":{"code":"dependency_unavailable","retry":"after","retry_after_seconds":5}}"#
        );
        let mut buffer = Vec::new();
        write_response(&mut buffer, &after).unwrap_or_else(|error| panic!("{error}"));
        let text = String::from_utf8_lossy(&buffer);
        assert!(text.starts_with("HTTP/1.1 503 Service Unavailable\r\n"));
        assert!(text.contains("Content-Type: application/json\r\n"));
        assert!(text.contains("Cache-Control: no-store\r\n"));
        assert!(text.contains("Retry-After: 5\r\n"));
        assert!(text.contains("Connection: close\r\n\r\n"));
        let mut created = Vec::new();
        write_response(&mut created, &json(201, &serde_json::json!({"a": 1})))
            .unwrap_or_else(|error| panic!("{error}"));
        assert!(String::from_utf8_lossy(&created).starts_with("HTTP/1.1 201 Created\r\n"));
    }
}
