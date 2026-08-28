//! Bounded HTTP/1.1 framing for the hosted registry service.

use std::collections::HashMap;
use std::io::{Read, Write};

use crate::routes::{Request, Response};

const MAX_REQUEST: usize = 32 * 1024 * 1024;
const READ_CHUNK: usize = 8 * 1024;

/// Reads one bounded request from an accepted connection.
///
/// # Errors
///
/// Refuses oversized, truncated and malformed requests.
pub fn parse_request(stream: &mut impl Read) -> Result<Request, String> {
    let mut bytes = Vec::new();
    let mut chunk = [0_u8; READ_CHUNK];
    let header_end = loop {
        let count = stream.read(&mut chunk).map_err(|error| error.to_string())?;
        if count == 0 || bytes.len().saturating_add(count) > MAX_REQUEST {
            return Err("invalid request size".to_owned());
        }
        bytes.extend_from_slice(&chunk[..count]);
        if let Some(position) = bytes.windows(4).position(|window| window == b"\r\n\r\n") {
            break position.saturating_add(4);
        }
    };
    let source =
        std::str::from_utf8(&bytes[..header_end]).map_err(|_| "invalid headers".to_owned())?;
    let mut lines = source.split("\r\n");
    let mut start = lines
        .next()
        .ok_or_else(|| "missing request line".to_owned())?
        .split_whitespace();
    let method = start
        .next()
        .ok_or_else(|| "missing method".to_owned())?
        .to_owned();
    let path = start
        .next()
        .ok_or_else(|| "missing path".to_owned())?
        .split('?')
        .next()
        .unwrap_or_default()
        .to_owned();
    let version = start.next().ok_or_else(|| "missing HTTP version".to_owned())?;
    if version != "HTTP/1.1" || start.next().is_some() || !method.bytes().all(|byte| byte.is_ascii_uppercase()) {
        return Err("invalid request line".to_owned());
    }
    let mut headers = HashMap::new();
    let mut content_length = 0_usize;
    for line in lines.filter(|line| !line.is_empty()) {
        let (raw_name, value) = line
            .split_once(':')
            .ok_or_else(|| "invalid header".to_owned())?;
        if raw_name != raw_name.trim() {
            return Err("invalid header name whitespace".to_owned());
        }
        let name = raw_name.to_ascii_lowercase();
        let value = value.trim().to_owned();
        if name.is_empty()
            || !name.bytes().all(|byte| byte.is_ascii_alphanumeric() || byte == b'-')
            || value.bytes().any(|byte| byte.is_ascii_control() && byte != b'\t')
            || headers.contains_key(&name)
        {
            return Err("duplicate or invalid header".to_owned());
        }
        if name == "transfer-encoding" {
            return Err("transfer encoding is not accepted".to_owned());
        }
        if name == "content-length" {
            if value.is_empty()
                || !value.bytes().all(|byte| byte.is_ascii_digit())
                || (value.len() > 1 && value.starts_with('0'))
            {
                return Err("invalid content length".to_owned());
            }
            content_length = value
                .parse()
                .map_err(|_| "invalid content length".to_owned())?;
        }
        headers.insert(name, value);
    }
    let body_end = header_end
        .checked_add(content_length)
        .ok_or_else(|| "invalid request size".to_owned())?;
    if body_end > MAX_REQUEST {
        return Err("invalid request size".to_owned());
    }
    if matches!(method.as_str(), "POST" | "PUT" | "PATCH")
        && !headers.contains_key("content-length")
    {
        return Err("content length is required".to_owned());
    }
    while bytes.len() < body_end {
        let count = stream.read(&mut chunk).map_err(|error| error.to_string())?;
        if count == 0 {
            return Err("truncated body".to_owned());
        }
        bytes.extend_from_slice(&chunk[..count]);
    }
    if bytes.len() != body_end {
        return Err("trailing request bytes are not accepted".to_owned());
    }
    Ok(Request {
        method,
        path,
        headers,
        body: bytes[header_end..body_end].to_vec(),
    })
}

/// Writes one response and closes the connection.
///
/// # Errors
///
/// Returns the transport error that prevented the response from being sent.
pub fn write_response(stream: &mut impl Write, response: &Response) -> std::io::Result<()> {
    let reason = match response.status {
        200 => "OK",
        400 => "Bad Request",
        401 => "Unauthorized",
        403 => "Forbidden",
        404 => "Not Found",
        405 => "Method Not Allowed",
        409 => "Conflict",
        413 => "Content Too Large",
        422 => "Unprocessable Content",
        502 => "Bad Gateway",
        503 => "Service Unavailable",
        _ => "Internal Server Error",
    };
    write!(
        stream,
        "HTTP/1.1 {} {reason}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nCache-Control: no-store\r\nConnection: close\r\n\r\n{}",
        response.status,
        response.body.len(),
        response.body
    )
}

#[cfg(test)]
mod tests {
    use super::parse_request;
    use std::io::Write as _;
    use std::net::{TcpListener, TcpStream};
    use std::thread;

    fn parse(bytes: &'static [u8]) -> Result<crate::Request, String> {
        let listener = TcpListener::bind("127.0.0.1:0")
            .unwrap_or_else(|error| panic!("listener fixture failed: {error}"));
        let address = listener.local_addr()
            .unwrap_or_else(|error| panic!("listener address failed: {error}"));
        let writer = thread::spawn(move || {
            let mut stream = TcpStream::connect(address)
                .unwrap_or_else(|error| panic!("fixture connection failed: {error}"));
            stream.write_all(bytes)
                .unwrap_or_else(|error| panic!("fixture write failed: {error}"));
        });
        let (mut stream, _) = listener.accept()
            .unwrap_or_else(|error| panic!("fixture accept failed: {error}"));
        let parsed = parse_request(&mut stream);
        writer.join().unwrap_or_else(|_| panic!("fixture writer panicked"));
        parsed
    }

    #[test]
    fn refuses_duplicate_content_length_and_transfer_encoding() {
        assert!(parse(b"POST /__registry/sources HTTP/1.1\r\nContent-Length: 0\r\nContent-Length: 0\r\n\r\n").is_err());
        assert!(parse(b"POST /__registry/sources HTTP/1.1\r\nTransfer-Encoding: chunked\r\n\r\n0\r\n\r\n").is_err());
    }

    #[test]
    fn accepts_one_canonical_bounded_request() {
        let request = parse(b"POST /__registry/head HTTP/1.1\r\nAuthorization: Bearer secret\r\nContent-Length: 0\r\n\r\n")
            .unwrap_or_else(|error| panic!("canonical request refused: {error}"));
        assert_eq!(request.path, "/__registry/head");
    }
}
