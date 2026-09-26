use std::collections::BTreeMap;
use std::io::{self, Read, Write};
use std::net::{IpAddr, Ipv4Addr, Ipv6Addr, Shutdown, SocketAddr, TcpListener, TcpStream};
use std::panic::{catch_unwind, AssertUnwindSafe};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver, SyncSender, TrySendError};
use std::sync::{Arc, Mutex};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

const READ_CHUNK: usize = 1_024;
const DRAIN_BYTES: usize = 65_536;
const DRAIN_TIME: Duration = Duration::from_millis(100);

/// The bounds every connection is held to.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Limits {
    pub workers: usize,
    pub queue: usize,
    pub request_line_bytes: usize,
    pub header_bytes: usize,
    pub header_count: usize,
    pub read_timeout: Duration,
    pub write_timeout: Duration,
}

impl Default for Limits {
    fn default() -> Self {
        Self {
            workers: 16,
            queue: 64,
            request_line_bytes: 8_192,
            header_bytes: 16_384,
            header_count: 64,
            read_timeout: Duration::from_secs(10),
            write_timeout: Duration::from_secs(10),
        }
    }
}

impl Limits {
    fn validate(&self) -> io::Result<()> {
        if self.workers == 0
            || self.queue == 0
            || self.request_line_bytes < 16
            || self.header_bytes == 0
            || self.header_count == 0
            || self.read_timeout.is_zero()
            || self.write_timeout.is_zero()
        {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "server limits must be positive",
            ));
        }
        Ok(())
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Ord, PartialOrd, Hash)]
pub enum Route {
    Health,
    Search,
    Fetch,
    Content,
}

impl Route {
    #[must_use]
    pub const fn is_paid(self) -> bool {
        !matches!(self, Self::Health)
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum QueryError {
    Malformed,
    Duplicate,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Request {
    pub method: String,
    pub route: Route,
    pub path: String,
    pub query: Option<String>,
    pub headers: Vec<(String, String)>,
    pub digest: Option<[u8; 32]>,
    pub peer: SocketAddr,
}

impl Request {
    /// The value of a header, matched without regard to case.
    #[must_use]
    pub fn header(&self, name: &str) -> Option<&str> {
        self.headers
            .iter()
            .find(|(candidate, _)| candidate.eq_ignore_ascii_case(name))
            .map(|(_, value)| value.as_str())
    }

    /// The percent-decoded value of a query parameter.
    ///
    /// # Errors
    /// Refuses malformed percent-encoding, invalid UTF-8 and a parameter given
    /// more than once.
    pub fn query_param(&self, name: &str) -> Result<Option<String>, QueryError> {
        let Some(query) = self.query.as_deref() else {
            return Ok(None);
        };
        let mut found = None;
        for pair in query.split('&').filter(|pair| !pair.is_empty()) {
            let (key, value) = pair.split_once('=').unwrap_or((pair, ""));
            if percent_decode(key)? == name {
                if found.is_some() {
                    return Err(QueryError::Duplicate);
                }
                found = Some(percent_decode(value)?);
            }
        }
        Ok(found)
    }
}

fn percent_decode(text: &str) -> Result<String, QueryError> {
    let bytes = text.as_bytes();
    let mut decoded = Vec::with_capacity(bytes.len());
    let mut index = 0;
    while index < bytes.len() {
        match bytes[index] {
            b'%' => {
                let high = bytes.get(index + 1).and_then(|byte| hex_value(*byte));
                let low = bytes.get(index + 2).and_then(|byte| hex_value(*byte));
                let (Some(high), Some(low)) = (high, low) else {
                    return Err(QueryError::Malformed);
                };
                decoded.push((high << 4) | low);
                index += 3;
            }
            b'+' => {
                decoded.push(b' ');
                index += 1;
            }
            byte => {
                decoded.push(byte);
                index += 1;
            }
        }
    }
    String::from_utf8(decoded).map_err(|_| QueryError::Malformed)
}

const fn hex_value(byte: u8) -> Option<u8> {
    match byte {
        b'0'..=b'9' => Some(byte - b'0'),
        b'a'..=b'f' => Some(byte - b'a' + 10),
        b'A'..=b'F' => Some(byte - b'A' + 10),
        _ => None,
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Response {
    pub status: u16,
    pub content_type: String,
    pub headers: Vec<(String, String)>,
    pub body: Vec<u8>,
}

impl Response {
    #[must_use]
    pub fn new(status: u16, content_type: &str, body: Vec<u8>) -> Self {
        Self {
            status,
            content_type: content_type.to_owned(),
            headers: Vec::new(),
            body,
        }
    }

    #[must_use]
    pub fn json(status: u16, body: Vec<u8>) -> Self {
        Self::new(status, "application/json", body)
    }

    /// A JSON refusal carrying a machine-readable code.
    #[must_use]
    pub fn error(status: u16, code: &str) -> Self {
        Self::json(
            status,
            serde_json::json!({ "error": code })
                .to_string()
                .into_bytes(),
        )
    }

    #[must_use]
    pub fn with_header(mut self, name: &str, value: &str) -> Self {
        self.headers.push((name.to_owned(), value.to_owned()));
        self
    }
}

type Handler = Box<dyn Fn(&Request) -> Response + Send + Sync>;
type AttestationHandler = Box<dyn Fn(u64) -> Response + Send + Sync>;

/// The path prefix of the signature-exchange route
/// `GET /attestations/<request id>`.
pub const ATTESTATION_PATH: &str = "/attestations/";

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RouteError {
    BuiltIn,
    AlreadySet,
}

impl std::fmt::Display for RouteError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::BuiltIn => f.write_str("the health route is built in"),
            Self::AlreadySet => f.write_str("the route already has a handler"),
        }
    }
}

impl std::error::Error for RouteError {}

/// The handlers for the paid routes. The health route is built in; a paid
/// route with no handler answers 503. The signature-exchange route answers
/// 404 until an attestor sets its handler.
#[derive(Default)]
pub struct RouteTable {
    handlers: BTreeMap<Route, Handler>,
    attestations: Option<AttestationHandler>,
}

impl RouteTable {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// # Errors
    /// Refuses the built-in health route and a route that already has a handler.
    pub fn set(
        &mut self,
        route: Route,
        handler: impl Fn(&Request) -> Response + Send + Sync + 'static,
    ) -> Result<(), RouteError> {
        if route == Route::Health {
            return Err(RouteError::BuiltIn);
        }
        if self.handlers.contains_key(&route) {
            return Err(RouteError::AlreadySet);
        }
        self.handlers.insert(route, Box::new(handler));
        Ok(())
    }

    /// Sets the handler of `GET /attestations/<request id>`.
    ///
    /// # Errors
    /// Refuses a table whose exchange route already has a handler.
    pub fn set_attestations(
        &mut self,
        handler: impl Fn(u64) -> Response + Send + Sync + 'static,
    ) -> Result<(), RouteError> {
        if self.attestations.is_some() {
            return Err(RouteError::AlreadySet);
        }
        self.attestations = Some(Box::new(handler));
        Ok(())
    }

    fn dispatch_attestation(&self, request_id: u64) -> Response {
        let Some(handler) = &self.attestations else {
            return Response::error(404, "not_found");
        };
        catch_unwind(AssertUnwindSafe(|| handler(request_id)))
            .unwrap_or_else(|_| Response::error(500, "internal_error"))
    }

    #[must_use]
    pub fn dispatch(&self, request: &Request) -> Response {
        if request.route == Route::Health {
            return Response::json(200, b"{\"status\":\"ok\"}".to_vec());
        }
        let Some(handler) = self.handlers.get(&request.route) else {
            return Response::error(503, "route_unavailable");
        };
        catch_unwind(AssertUnwindSafe(|| handler(request)))
            .unwrap_or_else(|_| Response::error(500, "internal_error"))
    }
}

pub struct Server {
    listener: TcpListener,
    limits: Limits,
    routes: Arc<RouteTable>,
}

impl Server {
    /// # Errors
    /// Refuses zero limits and returns the bind error.
    pub fn bind(address: SocketAddr, limits: Limits, routes: RouteTable) -> io::Result<Self> {
        limits.validate()?;
        Ok(Self {
            listener: TcpListener::bind(address)?,
            limits,
            routes: Arc::new(routes),
        })
    }

    /// # Errors
    /// Returns the listener's error.
    pub fn local_addr(&self) -> io::Result<SocketAddr> {
        self.listener.local_addr()
    }

    /// Serves until the listener fails.
    ///
    /// # Errors
    /// Returns the error that stopped the server.
    pub fn run(self) -> io::Result<()> {
        self.spawn()?.wait()
    }

    /// Starts the acceptor and the worker pool on their own threads.
    ///
    /// # Errors
    /// Returns an error when the address or a thread cannot be obtained.
    pub fn spawn(self) -> io::Result<RunningServer> {
        let address = self.listener.local_addr()?;
        let stop = Arc::new(AtomicBool::new(false));
        let (sender, receiver) = mpsc::sync_channel::<TcpStream>(self.limits.queue);
        let receiver = Arc::new(Mutex::new(receiver));
        let mut workers = Vec::with_capacity(self.limits.workers);
        for index in 0..self.limits.workers {
            let receiver = Arc::clone(&receiver);
            let routes = Arc::clone(&self.routes);
            let limits = self.limits;
            workers.push(
                thread::Builder::new()
                    .name(format!("x-websearch-worker-{index}"))
                    .spawn(move || work(&receiver, &limits, &routes))?,
            );
        }
        let acceptor_stop = Arc::clone(&stop);
        let limits = self.limits;
        let listener = self.listener;
        let acceptor = thread::Builder::new()
            .name("x-websearch-acceptor".to_owned())
            .spawn(move || accept(&listener, &sender, &acceptor_stop, &limits))?;
        Ok(RunningServer {
            address,
            stop,
            acceptor,
            workers,
        })
    }
}

pub struct RunningServer {
    address: SocketAddr,
    stop: Arc<AtomicBool>,
    acceptor: JoinHandle<io::Result<()>>,
    workers: Vec<JoinHandle<()>>,
}

impl RunningServer {
    #[must_use]
    pub const fn local_addr(&self) -> SocketAddr {
        self.address
    }

    /// Stops accepting, lets the workers finish the queued connections and
    /// joins every thread.
    ///
    /// # Errors
    /// Returns the acceptor's error or a thread that panicked.
    pub fn shutdown(self) -> io::Result<()> {
        self.stop.store(true, Ordering::SeqCst);
        let wake = match self.address.ip() {
            IpAddr::V4(ip) if ip.is_unspecified() => {
                SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), self.address.port())
            }
            IpAddr::V6(ip) if ip.is_unspecified() => {
                SocketAddr::new(IpAddr::V6(Ipv6Addr::LOCALHOST), self.address.port())
            }
            _ => self.address,
        };
        drop(TcpStream::connect_timeout(&wake, Duration::from_secs(1)));
        self.wait()
    }

    /// Blocks until the acceptor stops and the workers drain.
    ///
    /// # Errors
    /// Returns the acceptor's error or a thread that panicked.
    pub fn wait(self) -> io::Result<()> {
        let panicked = || io::Error::other("server thread panicked");
        let accepted = self.acceptor.join().map_err(|_| panicked())?;
        for worker in self.workers {
            worker.join().map_err(|_| panicked())?;
        }
        accepted
    }
}

fn accept(
    listener: &TcpListener,
    sender: &SyncSender<TcpStream>,
    stop: &AtomicBool,
    limits: &Limits,
) -> io::Result<()> {
    for incoming in listener.incoming() {
        if stop.load(Ordering::SeqCst) {
            return Ok(());
        }
        let stream = match incoming {
            Ok(stream) => stream,
            Err(error) if error.kind() == io::ErrorKind::Interrupted => continue,
            Err(error)
                if matches!(
                    error.kind(),
                    io::ErrorKind::ConnectionAborted | io::ErrorKind::ConnectionReset
                ) =>
            {
                continue
            }
            Err(error) => return Err(error),
        };
        match sender.try_send(stream) {
            Ok(()) => {}
            Err(TrySendError::Full(mut stream)) => {
                if stream.set_write_timeout(Some(limits.write_timeout)).is_ok() {
                    finish(&mut stream, &Response::error(503, "server_busy"));
                }
            }
            Err(TrySendError::Disconnected(_)) => {
                return Err(io::Error::other("worker pool stopped"))
            }
        }
    }
    Ok(())
}

fn work(receiver: &Mutex<Receiver<TcpStream>>, limits: &Limits, routes: &RouteTable) {
    loop {
        let next = match receiver.lock() {
            Ok(guard) => guard.recv(),
            Err(_) => return,
        };
        let Ok(stream) = next else { return };
        serve(stream, limits, routes);
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum ReadFailure {
    Closed,
    Timeout,
    LineTooLong,
    HeadersTooLarge,
    Malformed,
}

fn serve(mut stream: TcpStream, limits: &Limits, routes: &RouteTable) {
    if stream
        .set_write_timeout(Some(limits.write_timeout))
        .is_err()
    {
        return;
    }
    let Ok(peer) = stream.peer_addr() else { return };
    let response = match read_head(&mut stream, limits) {
        Ok((head, trailing)) => match parse_request(&head, peer, limits) {
            Ok(_) if trailing => Response::error(400, "unexpected_bytes"),
            Ok(Parsed::Routed(request)) if request.method == "GET" => routes.dispatch(&request),
            Ok(Parsed::Attestation { method, request_id }) if method == "GET" => {
                routes.dispatch_attestation(request_id)
            }
            Ok(_) => Response::error(405, "method_not_allowed").with_header("Allow", "GET"),
            Err(response) => response,
        },
        Err(ReadFailure::Closed) => return,
        Err(ReadFailure::Timeout) => Response::error(408, "request_timeout"),
        Err(ReadFailure::LineTooLong) => Response::error(414, "request_line_too_long"),
        Err(ReadFailure::HeadersTooLarge) => Response::error(431, "headers_too_large"),
        Err(ReadFailure::Malformed) => Response::error(400, "malformed_request"),
    };
    finish(&mut stream, &response);
}

fn find(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    haystack
        .windows(needle.len())
        .position(|window| window == needle)
}

fn read_head(stream: &mut TcpStream, limits: &Limits) -> Result<(Vec<u8>, bool), ReadFailure> {
    let deadline = Instant::now() + limits.read_timeout;
    let mut buffer = Vec::with_capacity(READ_CHUNK);
    let mut chunk = [0_u8; READ_CHUNK];
    loop {
        let line_end = find(&buffer, b"\r\n");
        match line_end {
            None if buffer.len() > limits.request_line_bytes + 1 => {
                return Err(ReadFailure::LineTooLong)
            }
            Some(end) if end > limits.request_line_bytes => return Err(ReadFailure::LineTooLong),
            Some(end) => {
                if let Some(head_end) = find(&buffer[end..], b"\r\n\r\n") {
                    let total = end + head_end + 4;
                    if head_end > limits.header_bytes {
                        return Err(ReadFailure::HeadersTooLarge);
                    }
                    let trailing = buffer.len() > total;
                    buffer.truncate(total);
                    return Ok((buffer, trailing));
                }
                if buffer.len() - end > limits.header_bytes + 4 {
                    return Err(ReadFailure::HeadersTooLarge);
                }
            }
            None => {}
        }
        let remaining = deadline.saturating_duration_since(Instant::now());
        if remaining.is_zero() {
            return Err(ReadFailure::Timeout);
        }
        stream
            .set_read_timeout(Some(remaining))
            .map_err(|_| ReadFailure::Malformed)?;
        match stream.read(&mut chunk) {
            Ok(0) if buffer.is_empty() => return Err(ReadFailure::Closed),
            Ok(0) => return Err(ReadFailure::Malformed),
            Ok(count) => buffer.extend_from_slice(&chunk[..count]),
            Err(error)
                if matches!(
                    error.kind(),
                    io::ErrorKind::WouldBlock | io::ErrorKind::TimedOut
                ) =>
            {
                return Err(ReadFailure::Timeout)
            }
            Err(error) if error.kind() == io::ErrorKind::Interrupted => {}
            Err(_) => return Err(ReadFailure::Closed),
        }
    }
}

const fn token_byte(byte: u8) -> bool {
    byte.is_ascii_alphanumeric()
        || matches!(
            byte,
            b'!' | b'#'
                | b'$'
                | b'%'
                | b'&'
                | b'\''
                | b'*'
                | b'+'
                | b'-'
                | b'.'
                | b'^'
                | b'_'
                | b'`'
                | b'|'
                | b'~'
        )
}

fn token(text: &str) -> bool {
    !text.is_empty() && text.bytes().all(token_byte)
}

fn header_value(text: &str) -> bool {
    text.bytes()
        .all(|byte| byte == b'\t' || (b' '..=b'~').contains(&byte))
}

fn route_of(path: &str) -> Result<(Route, Option<[u8; 32]>), Response> {
    match path {
        "/health" => Ok((Route::Health, None)),
        "/search" => Ok((Route::Search, None)),
        "/fetch" => Ok((Route::Fetch, None)),
        _ => {
            let Some(digest) = path.strip_prefix("/content/") else {
                return Err(Response::error(404, "not_found"));
            };
            crate::config::parse_hex32(digest)
                .filter(|_| !digest.starts_with("0x"))
                .map(|digest| (Route::Content, Some(digest)))
                .ok_or_else(|| Response::error(400, "malformed_digest"))
        }
    }
}

/// A parsed request: one for a routed resource, or one for the
/// signature-exchange route with its decimal request id.
enum Parsed {
    Routed(Request),
    Attestation { method: String, request_id: u64 },
}

fn attestation_id(path: &str) -> Option<Result<u64, Response>> {
    let id = path.strip_prefix(ATTESTATION_PATH)?;
    let canonical = !id.is_empty()
        && id.bytes().all(|byte| byte.is_ascii_digit())
        && (id == "0" || !id.starts_with('0'));
    Some(
        canonical
            .then(|| id.parse().ok())
            .flatten()
            .ok_or_else(|| Response::error(400, "malformed_request_id")),
    )
}

fn parse_request(head: &[u8], peer: SocketAddr, limits: &Limits) -> Result<Parsed, Response> {
    let malformed = || Response::error(400, "malformed_request");
    let text = std::str::from_utf8(head).map_err(|_| malformed())?;
    let mut lines = text
        .strip_suffix("\r\n\r\n")
        .ok_or_else(malformed)?
        .split("\r\n");
    let request_line = lines.next().ok_or_else(malformed)?;
    let mut parts = request_line.split(' ');
    let (Some(method), Some(target), Some(version), None) =
        (parts.next(), parts.next(), parts.next(), parts.next())
    else {
        return Err(malformed());
    };
    if !token(method)
        || !matches!(version, "HTTP/1.1" | "HTTP/1.0")
        || !target.starts_with('/')
        || target.contains('#')
        || !target.bytes().all(|byte| (b'!'..=b'~').contains(&byte))
    {
        return Err(malformed());
    }
    let mut headers: Vec<(String, String)> = Vec::new();
    for line in lines {
        let (name, value) = line.split_once(':').ok_or_else(malformed)?;
        let value = value.trim_matches([' ', '\t']);
        if !token(name) || !header_value(value) {
            return Err(malformed());
        }
        if headers
            .iter()
            .any(|(existing, _)| existing.eq_ignore_ascii_case(name))
        {
            return Err(Response::error(400, "duplicate_header"));
        }
        if headers.len() == limits.header_count {
            return Err(Response::error(431, "headers_too_large"));
        }
        headers.push((name.to_owned(), value.to_owned()));
    }
    let has = |wanted: &str| {
        headers
            .iter()
            .find(|(name, _)| name.eq_ignore_ascii_case(wanted))
            .map(|(_, value)| value.as_str())
    };
    if version == "HTTP/1.1" && has("Host").is_none_or(str::is_empty) {
        return Err(Response::error(400, "missing_host"));
    }
    if has("Transfer-Encoding").is_some()
        || has("Content-Length").is_some_and(|length| length != "0")
    {
        return Err(Response::error(400, "body_not_accepted"));
    }
    let (path, query) = match target.split_once('?') {
        Some((path, query)) => (path, Some(query.to_owned())),
        None => (target, None),
    };
    if let Some(request_id) = attestation_id(path) {
        return Ok(Parsed::Attestation {
            method: method.to_owned(),
            request_id: request_id?,
        });
    }
    let (route, digest) = route_of(path)?;
    Ok(Parsed::Routed(Request {
        method: method.to_owned(),
        route,
        path: path.to_owned(),
        query,
        headers,
        digest,
        peer,
    }))
}

const fn reason(status: u16) -> &'static str {
    match status {
        200 => "OK",
        400 => "Bad Request",
        402 => "Payment Required",
        403 => "Forbidden",
        404 => "Not Found",
        405 => "Method Not Allowed",
        408 => "Request Timeout",
        409 => "Conflict",
        413 => "Content Too Large",
        414 => "URI Too Long",
        415 => "Unsupported Media Type",
        422 => "Unprocessable Content",
        429 => "Too Many Requests",
        431 => "Request Header Fields Too Large",
        500 => "Internal Server Error",
        502 => "Bad Gateway",
        503 => "Service Unavailable",
        504 => "Gateway Timeout",
        _ => "",
    }
}

const RESERVED_HEADERS: [&str; 4] = [
    "content-type",
    "content-length",
    "connection",
    "transfer-encoding",
];

fn encode(response: &Response) -> Option<Vec<u8>> {
    let phrase = reason(response.status);
    let headers_valid = response.headers.iter().all(|(name, value)| {
        token(name)
            && header_value(value)
            && !RESERVED_HEADERS.contains(&name.to_ascii_lowercase().as_str())
    });
    if phrase.is_empty() || !headers_valid || !header_value(&response.content_type) {
        return None;
    }
    let mut head = format!(
        "HTTP/1.1 {} {phrase}\r\nContent-Type: {}\r\nContent-Length: {}\r\nConnection: close\r\nCache-Control: no-store\r\n",
        response.status,
        response.content_type,
        response.body.len()
    );
    for (name, value) in &response.headers {
        head.push_str(name);
        head.push_str(": ");
        head.push_str(value);
        head.push_str("\r\n");
    }
    head.push_str("\r\n");
    let mut bytes = head.into_bytes();
    bytes.extend_from_slice(&response.body);
    Some(bytes)
}

fn finish(stream: &mut TcpStream, response: &Response) {
    let bytes = encode(response)
        .unwrap_or_else(|| encode(&Response::error(500, "invalid_response")).unwrap_or_default());
    if stream
        .write_all(&bytes)
        .and_then(|()| stream.flush())
        .is_err()
    {
        return;
    }
    if stream.shutdown(Shutdown::Write).is_err() {
        return;
    }
    let deadline = Instant::now() + DRAIN_TIME;
    let mut drained = 0;
    let mut chunk = [0_u8; READ_CHUNK];
    while drained < DRAIN_BYTES {
        let remaining = deadline.saturating_duration_since(Instant::now());
        if remaining.is_zero() || stream.set_read_timeout(Some(remaining)).is_err() {
            return;
        }
        match stream.read(&mut chunk) {
            Ok(0) | Err(_) => return,
            Ok(count) => drained += count,
        }
    }
}
