use super::{read_http_message, NodeEndpoint, NodeFailure, CONNECT_TIMEOUT};
use std::io::{self, Read, Write};
use std::net::TcpStream;
use std::time::{Duration, Instant};

struct DeadlineStream {
    stream: TcpStream,
    deadline: Instant,
}

impl DeadlineStream {
    fn remaining(&self) -> io::Result<Duration> {
        self.deadline
            .checked_duration_since(Instant::now())
            .filter(|remaining| !remaining.is_zero())
            .ok_or_else(|| io::Error::new(io::ErrorKind::TimedOut, "Comet response deadline"))
    }
}

impl Read for DeadlineStream {
    fn read(&mut self, bytes: &mut [u8]) -> io::Result<usize> {
        self.stream.set_read_timeout(Some(self.remaining()?))?;
        self.stream.read(bytes)
    }
}

impl Write for DeadlineStream {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        self.stream.set_write_timeout(Some(self.remaining()?))?;
        self.stream.write(bytes)
    }

    fn flush(&mut self) -> io::Result<()> {
        self.stream.set_write_timeout(Some(self.remaining()?))?;
        self.stream.flush()
    }
}

pub(super) fn request(
    node: &NodeEndpoint,
    path: &str,
    body: Option<&[u8]>,
    maximum: usize,
    deadline: Instant,
) -> Result<Vec<u8>, NodeFailure> {
    let remaining = deadline
        .checked_duration_since(Instant::now())
        .filter(|remaining| !remaining.is_zero())
        .ok_or(NodeFailure::Unreachable)?;
    let stream = TcpStream::connect_timeout(&node.address, CONNECT_TIMEOUT.min(remaining))
        .map_err(|_| NodeFailure::Unreachable)?;
    let mut stream = DeadlineStream { stream, deadline };
    let method = if body.is_some() { "POST" } else { "GET" };
    let body = body.unwrap_or_default();
    write!(stream, "{method} {path} HTTP/1.1\r\nHost: {}\r\nContent-Type: application/json\r\nAccept: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n", node.address, body.len())
        .map_err(|_| NodeFailure::Unreachable)?;
    stream.write_all(body).map_err(|_| NodeFailure::Unreachable)?;
    stream.flush().map_err(|_| NodeFailure::Unreachable)?;
    let mut response = read_http_message(&mut stream, maximum, true)
        .map_err(|_| NodeFailure::Invalid)?;
    let mut start = response.headers.get("").ok_or(NodeFailure::Invalid)?.split_whitespace();
    if start.next() != Some("HTTP/1.1") || start.next() != Some("200")
        || Instant::now() >= deadline
    {
        return Err(NodeFailure::Invalid);
    }
    Ok(std::mem::take(&mut response.body))
}
