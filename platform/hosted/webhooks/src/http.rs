//! Bounded HTTP/1.1 request and response framing shared by hosted services.

use std::collections::BTreeMap;
use std::io::{Read, Write};

use crate::error::WebhookError;

/// Largest request the control surface reads.
pub const MAXIMUM_REQUEST_BYTES: usize = 128 * 1024;
/// One parsed request.
#[derive(Clone, Debug)]
pub struct Request {
    /// Request method in upper case.
    pub method: String,
    /// Request path without its query string.
    pub path: String,
    /// Raw query string without the leading question mark.
    pub query: String,
    /// Header names lowercased.
    pub headers: BTreeMap<String, String>,
    /// Exact request body bytes.
    pub body: Vec<u8>,
}

impl Request {
    /// Borrows one header by its lowercase name.
    #[must_use]
    pub fn header(&self, name: &str) -> Option<&str> {
        self.headers.get(name).map(String::as_str)
    }

    /// Borrows one query parameter value.
    #[must_use]
    pub fn parameter(&self, name: &str) -> Option<&str> {
        self.query.split('&').find_map(|pair| {
            let (key, value) = pair.split_once('=')?;
            (key == name).then_some(value)
        })
    }

    /// Returns the non-empty path segments.
    #[must_use]
    pub fn segments(&self) -> Vec<&str> {
        self.path
            .split('/')
            .filter(|segment| !segment.is_empty())
            .collect()
    }
}

/// One reply.
#[derive(Clone, Debug)]
pub struct Reply {
    /// HTTP status.
    pub status: u16,
    /// JSON body.
    pub body: String,
    /// Retry timing carried on a refusal that is worth retrying.
    pub retry_after: Option<u64>,
}

impl Reply {
    /// Builds a JSON reply.
    #[must_use]
    pub fn json(status: u16, body: String) -> Self {
        Self {
            status,
            body,
            retry_after: None,
        }
    }

    /// Builds a typed refusal carrying explicit retry timing.
    #[must_use]
    pub fn refusal(status: u16, code: &str, retry_after: Option<u64>) -> Self {
        let timing = retry_after.map_or_else(
            || "\"never\"".to_owned(),
            |seconds| format!("\"after\",\"retry_after_seconds\":{seconds}"),
        );
        Self {
            status,
            body: format!("{{\"error\":{{\"code\":\"{code}\",\"retry\":{timing}}}}}"),
            retry_after,
        }
    }
}

/// Reads one request from the stream.
///
/// # Errors
/// Returns [`WebhookError::InvalidRequest`] for a malformed or oversized request
/// and [`WebhookError::Io`] when the stream fails.
pub fn read_request(stream: &mut impl Read) -> Result<Request, WebhookError> {
    let mut bytes = Vec::new();
    let mut chunk = [0_u8; 4096];
    let header_end = loop {
        let count = stream.read(&mut chunk)?;
        if count == 0 || bytes.len().saturating_add(count) > MAXIMUM_REQUEST_BYTES {
            return Err(WebhookError::InvalidRequest);
        }
        bytes.extend_from_slice(&chunk[..count]);
        if let Some(position) = bytes.windows(4).position(|window| window == b"\r\n\r\n") {
            break position.saturating_add(4);
        }
    };
    let source =
        std::str::from_utf8(&bytes[..header_end]).map_err(|_| WebhookError::InvalidRequest)?;
    let mut lines = source.split("\r\n");
    let mut first = lines
        .next()
        .ok_or(WebhookError::InvalidRequest)?
        .split_whitespace();
    let method = first
        .next()
        .ok_or(WebhookError::InvalidRequest)?
        .to_ascii_uppercase();
    let target = first.next().ok_or(WebhookError::InvalidRequest)?;
    if first.next() != Some("HTTP/1.1")
        || first.next().is_some()
        || !target.starts_with('/')
        || target.contains(['#', '\\', '\0'])
    {
        return Err(WebhookError::InvalidRequest);
    }
    let (path, query) = target.split_once('?').unwrap_or((target, ""));
    let path = path.to_owned();
    let query = query.to_owned();
    let mut headers = BTreeMap::new();
    let mut content_length = 0_usize;
    for line in lines.filter(|line| !line.is_empty()) {
        let (name, value) = line.split_once(':').ok_or(WebhookError::InvalidRequest)?;
        let name = name.trim().to_ascii_lowercase();
        let value = value.trim().to_owned();
        if name.is_empty()
            || headers.contains_key(&name)
            || name
                .bytes()
                .any(|byte| !(byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'-'))
            || value.contains(['\r', '\n', '\0'])
            || name == "transfer-encoding"
        {
            return Err(WebhookError::InvalidRequest);
        }
        if name == "content-length" {
            content_length = value.parse().map_err(|_| WebhookError::InvalidRequest)?;
        }
        headers.insert(name, value);
    }
    if !headers.contains_key("host") {
        return Err(WebhookError::InvalidRequest);
    }
    let total = header_end.saturating_add(content_length);
    if total > MAXIMUM_REQUEST_BYTES {
        return Err(WebhookError::InvalidRequest);
    }
    while bytes.len() < total {
        let count = stream.read(&mut chunk)?;
        if count == 0 {
            return Err(WebhookError::InvalidRequest);
        }
        bytes.extend_from_slice(&chunk[..count]);
    }
    if bytes.len() != total {
        return Err(WebhookError::InvalidRequest);
    }
    Ok(Request {
        method,
        path,
        query,
        headers,
        body: bytes
            .get(header_end..total)
            .ok_or(WebhookError::InvalidRequest)?
            .to_vec(),
    })
}

/// Writes one reply and closes the connection.
///
/// # Errors
/// Returns [`WebhookError::Io`] when the stream fails.
pub fn write_reply(stream: &mut impl Write, reply: &Reply) -> Result<(), WebhookError> {
    let reason = match reply.status {
        200 => "OK",
        201 => "Created",
        202 => "Accepted",
        400 => "Bad Request",
        401 => "Unauthorized",
        403 => "Forbidden",
        409 => "Conflict",
        410 => "Gone",
        413 => "Payload Too Large",
        422 => "Unprocessable Content",
        429 => "Too Many Requests",
        503 => "Service Unavailable",
        _ => "Not Found",
    };
    let retry = reply
        .retry_after
        .map_or_else(String::new, |seconds| format!("Retry-After: {seconds}\r\n"));
    write!(
        stream,
        "HTTP/1.1 {} {reason}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nCache-Control: no-store\r\nX-Content-Type-Options: nosniff\r\n{retry}Connection: close\r\n\r\n{}",
        reply.status,
        reply.body.len(),
        reply.body
    )?;
    stream.flush()?;
    Ok(())
}
