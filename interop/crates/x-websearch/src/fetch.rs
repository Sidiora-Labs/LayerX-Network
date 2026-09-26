use std::io::{self, Read, Write};
use std::net::{IpAddr, Ipv4Addr, SocketAddr, TcpStream, ToSocketAddrs as _};
use std::time::{Duration, Instant};

use crate::canonical::{self, CanonicalError, ContentKind};
use crate::config::{
    FetchLimits, BODY_LIMIT_BYTES, CONNECT_TIMEOUT_LIMIT_MS, MAX_URL_BYTES, REDIRECT_LIMIT,
    TOTAL_TIMEOUT_LIMIT_MS,
};
use crate::content::{ContentStore, MAX_CONTENT_BYTES};
use crate::extract::{self, ExtractError};
use crate::robots::{Robots, RobotsCache};
use crate::server::{QueryError, Request, Response};

/// The media types a fetch asks for.
pub const ACCEPT: &str = "text/html, text/plain, application/json";

const MAX_RESPONSE_HEAD: usize = 65_536;
const MAX_CHUNK_LINE: usize = 1_024;
const READ_CHUNK: usize = 16_384;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum FetchError {
    InvalidLimits,
    InvalidUrl,
    UnsupportedScheme,
    Resolve,
    ForbiddenDestination,
    RobotsDisallowed,
    RobotsUnavailable,
    Connect,
    ConnectTimeout,
    Timeout,
    Tls,
    Transport,
    MalformedResponse,
    UnsupportedEncoding,
    Status(u16),
    TooManyRedirects,
    BodyTooLarge,
    ContentTooLarge,
    Extract(ExtractError),
    Canonical(CanonicalError),
}

impl FetchError {
    #[must_use]
    pub const fn code(self) -> &'static str {
        match self {
            Self::InvalidLimits => "invalid_fetch_limits",
            Self::InvalidUrl => "invalid_url",
            Self::UnsupportedScheme => "unsupported_scheme",
            Self::Resolve => "resolve_failed",
            Self::ForbiddenDestination => "forbidden_destination",
            Self::RobotsDisallowed => "robots_disallowed",
            Self::RobotsUnavailable => "robots_unavailable",
            Self::Connect => "connect_failed",
            Self::ConnectTimeout => "connect_timeout",
            Self::Timeout => "fetch_timeout",
            Self::Tls => "tls_failed",
            Self::Transport => "transport_failed",
            Self::MalformedResponse => "malformed_response",
            Self::UnsupportedEncoding => "unsupported_encoding",
            Self::Status(_) => "upstream_status",
            Self::TooManyRedirects => "too_many_redirects",
            Self::BodyTooLarge => "body_too_large",
            Self::ContentTooLarge => "content_too_large",
            Self::Extract(error) => error.code(),
            Self::Canonical(error) => error.code(),
        }
    }

    /// The status a `GET /fetch` refusal carries.
    #[must_use]
    pub const fn status(self) -> u16 {
        match self {
            Self::InvalidLimits => 500,
            Self::InvalidUrl | Self::UnsupportedScheme => 400,
            Self::ForbiddenDestination | Self::RobotsDisallowed => 403,
            Self::ConnectTimeout | Self::Timeout => 504,
            Self::Extract(ExtractError::MissingMediaType | ExtractError::UnsupportedMediaType) => {
                415
            }
            Self::Extract(ExtractError::Canonical(_)) | Self::Canonical(_) => 422,
            Self::Resolve
            | Self::RobotsUnavailable
            | Self::Connect
            | Self::Tls
            | Self::Transport
            | Self::MalformedResponse
            | Self::UnsupportedEncoding
            | Self::Status(_)
            | Self::TooManyRedirects
            | Self::BodyTooLarge
            | Self::ContentTooLarge => 502,
        }
    }
}

impl std::fmt::Display for FetchError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Status(status) => write!(f, "upstream answered {status}"),
            _ => f.write_str(self.code()),
        }
    }
}

impl std::error::Error for FetchError {}

/// An absolute http or https URL.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Url {
    pub secure: bool,
    pub host: String,
    pub port: u16,
    pub target: String,
}

fn scheme_of(text: &str) -> Option<(&str, &str)> {
    let (scheme, rest) = text.split_once(':')?;
    let mut bytes = scheme.bytes();
    let valid = bytes.next().is_some_and(|byte| byte.is_ascii_alphabetic())
        && bytes.all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'+' | b'-' | b'.'));
    valid.then_some((scheme, rest))
}

fn host_valid(host: &str) -> bool {
    if let Some(inner) = host.strip_prefix('[') {
        return inner
            .strip_suffix(']')
            .is_some_and(|literal| literal.parse::<std::net::Ipv6Addr>().is_ok());
    }
    !host.is_empty()
        && host.len() <= 253
        && host
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'.' | b'_'))
}

fn remove_dot_segments(path: &str) -> String {
    let segments: Vec<&str> = path.split('/').collect();
    let mut output: Vec<&str> = Vec::with_capacity(segments.len());
    for (index, segment) in segments.iter().enumerate().skip(1) {
        let last = index + 1 == segments.len();
        match *segment {
            "." => {
                if last {
                    output.push("");
                }
            }
            ".." => {
                output.pop();
                if last {
                    output.push("");
                }
            }
            other => output.push(other),
        }
    }
    format!("/{}", output.join("/"))
}

impl Url {
    /// # Errors
    /// Refuses a scheme other than http and https, user information, a
    /// malformed host or port, whitespace, control and non-ASCII bytes and a
    /// URL longer than 2048 bytes.
    pub fn parse(text: &str) -> Result<Self, FetchError> {
        if text.len() > MAX_URL_BYTES
            || !text.is_ascii()
            || text
                .bytes()
                .any(|byte| byte.is_ascii_control() || byte == b' ')
        {
            return Err(FetchError::InvalidUrl);
        }
        let text = text.split('#').next().unwrap_or_default();
        let (scheme, rest) = scheme_of(text).ok_or(FetchError::InvalidUrl)?;
        let secure = match scheme.to_ascii_lowercase().as_str() {
            "http" => false,
            "https" => true,
            _ => return Err(FetchError::UnsupportedScheme),
        };
        let rest = rest.strip_prefix("//").ok_or(FetchError::InvalidUrl)?;
        let end = rest.find(['/', '?']).unwrap_or(rest.len());
        let (authority, tail) = rest.split_at(end);
        if authority.contains('@') {
            return Err(FetchError::InvalidUrl);
        }
        let (host, port) = match authority.rfind(':') {
            Some(colon) if !authority[colon..].contains(']') => {
                (&authority[..colon], Some(&authority[colon + 1..]))
            }
            _ => (authority, None),
        };
        let host = host.to_ascii_lowercase();
        if !host_valid(&host) {
            return Err(FetchError::InvalidUrl);
        }
        let default_port = if secure { 443 } else { 80 };
        let port = match port {
            None | Some("") => default_port,
            Some(digits) => digits
                .parse::<u16>()
                .ok()
                .filter(|port| *port != 0 && digits.bytes().all(|byte| byte.is_ascii_digit()))
                .ok_or(FetchError::InvalidUrl)?,
        };
        let target = if tail.is_empty() {
            "/".to_owned()
        } else if tail.starts_with('?') {
            format!("/{tail}")
        } else {
            tail.to_owned()
        };
        Ok(Self {
            secure,
            host,
            port,
            target,
        })
    }

    #[must_use]
    pub const fn scheme(&self) -> &'static str {
        if self.secure {
            "https"
        } else {
            "http"
        }
    }

    const fn default_port(&self) -> bool {
        (self.secure && self.port == 443) || (!self.secure && self.port == 80)
    }

    /// The host with the port unless it is the scheme's default.
    #[must_use]
    pub fn authority(&self) -> String {
        if self.default_port() {
            self.host.clone()
        } else {
            format!("{}:{}", self.host, self.port)
        }
    }

    /// Scheme, host and port: the key robots.txt is cached under.
    #[must_use]
    pub fn origin(&self) -> String {
        format!("{}://{}:{}", self.scheme(), self.host, self.port)
    }

    /// The host as name resolution and TLS take it, without brackets.
    #[must_use]
    pub fn bare_host(&self) -> &str {
        self.host.trim_start_matches('[').trim_end_matches(']')
    }

    /// Resolves a reference, such as a Location header, against this URL.
    ///
    /// # Errors
    /// Refuses a reference that does not resolve to a valid http or https
    /// URL.
    pub fn join(&self, reference: &str) -> Result<Self, FetchError> {
        let reference = reference.trim_matches([' ', '\t']);
        let reference = reference.split('#').next().unwrap_or_default();
        if scheme_of(reference).is_some() {
            return Self::parse(reference);
        }
        if reference.starts_with("//") {
            return Self::parse(&format!("{}:{reference}", self.scheme()));
        }
        let path = self.target.split('?').next().unwrap_or("/");
        let target = if reference.is_empty() {
            self.target.clone()
        } else if reference.starts_with('/') {
            reference.to_owned()
        } else if reference.starts_with('?') {
            format!("{path}{reference}")
        } else {
            let directory = path.rfind('/').map_or("/", |slash| &path[..=slash]);
            format!("{directory}{reference}")
        };
        let (target_path, query) = match target.split_once('?') {
            Some((target_path, query)) => (target_path, Some(query)),
            None => (target.as_str(), None),
        };
        let mut normalised = remove_dot_segments(target_path);
        if let Some(query) = query {
            normalised.push('?');
            normalised.push_str(query);
        }
        Self::parse(&format!(
            "{}://{}{normalised}",
            self.scheme(),
            self.authority()
        ))
    }
}

impl std::fmt::Display for Url {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}://{}{}", self.scheme(), self.authority(), self.target)
    }
}

const fn shared_or_reserved_v4(address: Ipv4Addr) -> bool {
    let octets = address.octets();
    octets[0] == 0 || (octets[0] == 100 && (octets[1] & 0xc0) == 64)
}

/// Whether a resolved address may be dialled: loopback only when
/// `allow_loopback` is set; private, link-local, multicast, unspecified and
/// broadcast never.
#[must_use]
pub fn destination_permitted(address: IpAddr, allow_loopback: bool) -> bool {
    match address {
        IpAddr::V4(address) => {
            if address.is_loopback() {
                return allow_loopback;
            }
            !(address.is_private()
                || address.is_link_local()
                || address.is_multicast()
                || address.is_unspecified()
                || address.is_broadcast()
                || shared_or_reserved_v4(address))
        }
        IpAddr::V6(address) => {
            if let Some(mapped) = address.to_ipv4_mapped() {
                return destination_permitted(IpAddr::V4(mapped), allow_loopback);
            }
            if address.is_loopback() {
                return allow_loopback;
            }
            let first = address.segments()[0];
            !(address.is_multicast()
                || address.is_unspecified()
                || (first & 0xfe00) == 0xfc00
                || (first & 0xffc0) == 0xfe80)
        }
    }
}

enum Stream {
    Plain(TcpStream),
    Tls(Box<native_tls::TlsStream<TcpStream>>),
}

impl Stream {
    fn socket(&self) -> &TcpStream {
        match self {
            Self::Plain(stream) => stream,
            Self::Tls(stream) => stream.get_ref(),
        }
    }
}

impl Read for Stream {
    fn read(&mut self, buffer: &mut [u8]) -> io::Result<usize> {
        match self {
            Self::Plain(stream) => stream.read(buffer),
            Self::Tls(stream) => stream.read(buffer),
        }
    }
}

impl Write for Stream {
    fn write(&mut self, buffer: &[u8]) -> io::Result<usize> {
        match self {
            Self::Plain(stream) => stream.write(buffer),
            Self::Tls(stream) => stream.write(buffer),
        }
    }

    fn flush(&mut self) -> io::Result<()> {
        match self {
            Self::Plain(stream) => stream.flush(),
            Self::Tls(stream) => stream.flush(),
        }
    }
}

fn remaining(deadline: Instant) -> Result<Duration, FetchError> {
    let remaining = deadline.saturating_duration_since(Instant::now());
    if remaining.is_zero() {
        Err(FetchError::Timeout)
    } else {
        Ok(remaining)
    }
}

struct Wire {
    stream: Stream,
    buffer: Vec<u8>,
    start: usize,
    deadline: Instant,
}

impl Wire {
    fn available(&self) -> &[u8] {
        &self.buffer[self.start..]
    }

    fn fill(&mut self) -> Result<usize, FetchError> {
        let mut chunk = [0_u8; READ_CHUNK];
        loop {
            let left = remaining(self.deadline)?;
            self.stream
                .socket()
                .set_read_timeout(Some(left))
                .map_err(|_| FetchError::Transport)?;
            match self.stream.read(&mut chunk) {
                Ok(count) => {
                    self.buffer.extend_from_slice(&chunk[..count]);
                    return Ok(count);
                }
                Err(error) if error.kind() == io::ErrorKind::Interrupted => {}
                Err(error)
                    if matches!(
                        error.kind(),
                        io::ErrorKind::WouldBlock | io::ErrorKind::TimedOut
                    ) =>
                {
                    return Err(FetchError::Timeout)
                }
                Err(_) => return Err(FetchError::Transport),
            }
        }
    }

    fn line(&mut self, maximum: usize) -> Result<String, FetchError> {
        loop {
            if let Some(end) = self.available().windows(2).position(|pair| pair == b"\r\n") {
                let line = std::str::from_utf8(&self.available()[..end])
                    .map_err(|_| FetchError::MalformedResponse)?
                    .to_owned();
                self.start += end + 2;
                return Ok(line);
            }
            if self.available().len() > maximum || self.fill()? == 0 {
                return Err(FetchError::MalformedResponse);
            }
        }
    }

    fn head(&mut self) -> Result<String, FetchError> {
        loop {
            if let Some(end) = self
                .available()
                .windows(4)
                .position(|window| window == b"\r\n\r\n")
            {
                let head = std::str::from_utf8(&self.available()[..end])
                    .map_err(|_| FetchError::MalformedResponse)?
                    .to_owned();
                self.start += end + 4;
                return Ok(head);
            }
            if self.available().len() > MAX_RESPONSE_HEAD || self.fill()? == 0 {
                return Err(FetchError::MalformedResponse);
            }
        }
    }

    fn take(&mut self, count: usize) -> Result<Vec<u8>, FetchError> {
        while self.available().len() < count {
            if self.fill()? == 0 {
                return Err(FetchError::MalformedResponse);
            }
        }
        let bytes = self.available()[..count].to_vec();
        self.start += count;
        Ok(bytes)
    }

    fn until_close(&mut self, limit: usize) -> Result<Vec<u8>, FetchError> {
        loop {
            if self.available().len() > limit {
                return Err(FetchError::BodyTooLarge);
            }
            if self.fill()? == 0 {
                let bytes = self.available().to_vec();
                self.start = self.buffer.len();
                return Ok(bytes);
            }
        }
    }

    fn chunked(&mut self, limit: usize) -> Result<Vec<u8>, FetchError> {
        let mut body = Vec::new();
        loop {
            let line = self.line(MAX_CHUNK_LINE)?;
            let size = line.split(';').next().unwrap_or_default().trim();
            if size.is_empty() || size.len() > 16 {
                return Err(FetchError::MalformedResponse);
            }
            let size =
                usize::from_str_radix(size, 16).map_err(|_| FetchError::MalformedResponse)?;
            if size == 0 {
                while !self.line(MAX_RESPONSE_HEAD)?.is_empty() {}
                return Ok(body);
            }
            if body.len().saturating_add(size) > limit {
                return Err(FetchError::BodyTooLarge);
            }
            body.extend_from_slice(&self.take(size)?);
            if self.take(2)? != b"\r\n" {
                return Err(FetchError::MalformedResponse);
            }
        }
    }
}

/// One HTTP response: the status, the headers and, for a 2xx status, the
/// body.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct HttpResponse {
    pub status: u16,
    pub headers: Vec<(String, String)>,
    pub body: Vec<u8>,
}

impl HttpResponse {
    #[must_use]
    pub fn header(&self, name: &str) -> Option<&str> {
        self.headers
            .iter()
            .find(|(candidate, _)| candidate.eq_ignore_ascii_case(name))
            .map(|(_, value)| value.as_str())
    }
}

fn parse_head(head: &str) -> Result<(u16, Vec<(String, String)>), FetchError> {
    let mut lines = head.split("\r\n");
    let status_line = lines.next().unwrap_or_default();
    let mut parts = status_line.splitn(3, ' ');
    let version = parts.next().unwrap_or_default();
    let status = parts.next().unwrap_or_default();
    if !matches!(version, "HTTP/1.1" | "HTTP/1.0")
        || status.len() != 3
        || !status.bytes().all(|byte| byte.is_ascii_digit())
    {
        return Err(FetchError::MalformedResponse);
    }
    let status: u16 = status.parse().map_err(|_| FetchError::MalformedResponse)?;
    let mut headers: Vec<(String, String)> = Vec::new();
    for line in lines {
        if line.starts_with([' ', '\t']) {
            return Err(FetchError::MalformedResponse);
        }
        let (name, value) = line.split_once(':').ok_or(FetchError::MalformedResponse)?;
        if name.is_empty() || name.bytes().any(|byte| byte <= b' ') {
            return Err(FetchError::MalformedResponse);
        }
        let value = value.trim_matches([' ', '\t']);
        let repeated = headers.iter().find(|(existing, _)| {
            existing.eq_ignore_ascii_case(name)
                && [
                    "content-length",
                    "transfer-encoding",
                    "content-type",
                    "location",
                ]
                .contains(&name.to_ascii_lowercase().as_str())
        });
        match repeated {
            Some((_, existing)) if existing == value => {}
            Some(_) => return Err(FetchError::MalformedResponse),
            None => headers.push((name.to_owned(), value.to_owned())),
        }
    }
    Ok((status, headers))
}

/// The outbound HTTP client: `std::net::TcpStream`, with native-tls for https.
pub struct HttpClient {
    tls: native_tls::TlsConnector,
    connect_timeout: Duration,
}

impl HttpClient {
    /// # Errors
    /// Refuses a zero connect timeout and a TLS connector that cannot be
    /// built.
    pub fn new(connect_timeout: Duration) -> Result<Self, FetchError> {
        if connect_timeout.is_zero() {
            return Err(FetchError::InvalidLimits);
        }
        let tls = native_tls::TlsConnector::new().map_err(|_| FetchError::Tls)?;
        Ok(Self {
            tls,
            connect_timeout,
        })
    }

    fn connect(
        &self,
        url: &Url,
        address: SocketAddr,
        deadline: Instant,
    ) -> Result<Stream, FetchError> {
        let left = remaining(deadline)?;
        let limited = self.connect_timeout < left;
        let socket = TcpStream::connect_timeout(&address, self.connect_timeout.min(left)).map_err(
            |error| match error.kind() {
                io::ErrorKind::TimedOut | io::ErrorKind::WouldBlock if limited => {
                    FetchError::ConnectTimeout
                }
                io::ErrorKind::TimedOut | io::ErrorKind::WouldBlock => FetchError::Timeout,
                _ => FetchError::Connect,
            },
        )?;
        let left = remaining(deadline)?;
        socket
            .set_write_timeout(Some(left))
            .and_then(|()| socket.set_read_timeout(Some(left)))
            .map_err(|_| FetchError::Transport)?;
        if !url.secure {
            return Ok(Stream::Plain(socket));
        }
        self.tls
            .connect(url.bare_host(), socket)
            .map(|stream| Stream::Tls(Box::new(stream)))
            .map_err(|_| {
                if Instant::now() >= deadline {
                    FetchError::Timeout
                } else {
                    FetchError::Tls
                }
            })
    }

    /// Sends one GET to an already checked address and reads the response;
    /// the body is read only for a 2xx status and never beyond `body_limit`.
    ///
    /// # Errors
    /// Returns the connect, TLS, transport, time, framing and size refusals.
    pub fn get(
        &self,
        url: &Url,
        address: SocketAddr,
        deadline: Instant,
        body_limit: usize,
        headers: &[(&str, &str)],
    ) -> Result<HttpResponse, FetchError> {
        let mut stream = self.connect(url, address, deadline)?;
        let mut request = format!(
            "GET {} HTTP/1.1\r\nHost: {}\r\nUser-Agent: {}/{}\r\nAccept-Encoding: identity\r\nConnection: close\r\n",
            url.target,
            url.authority(),
            crate::robots::USER_AGENT_TOKEN,
            env!("CARGO_PKG_VERSION"),
        );
        for (name, value) in headers {
            request.push_str(name);
            request.push_str(": ");
            request.push_str(value);
            request.push_str("\r\n");
        }
        request.push_str("\r\n");
        stream
            .write_all(request.as_bytes())
            .and_then(|()| stream.flush())
            .map_err(|error| match error.kind() {
                io::ErrorKind::WouldBlock | io::ErrorKind::TimedOut => FetchError::Timeout,
                _ => FetchError::Transport,
            })?;
        let mut wire = Wire {
            stream,
            buffer: Vec::with_capacity(READ_CHUNK),
            start: 0,
            deadline,
        };
        let (status, headers) = loop {
            let (status, headers) = parse_head(&wire.head()?)?;
            if !(100..200).contains(&status) {
                break (status, headers);
            }
        };
        let mut response = HttpResponse {
            status,
            headers,
            body: Vec::new(),
        };
        if !(200..300).contains(&status) {
            return Ok(response);
        }
        if response
            .header("content-encoding")
            .is_some_and(|encoding| !encoding.eq_ignore_ascii_case("identity"))
        {
            return Err(FetchError::UnsupportedEncoding);
        }
        response.body = if let Some(encoding) = response.header("transfer-encoding") {
            if !encoding.eq_ignore_ascii_case("chunked") {
                return Err(FetchError::UnsupportedEncoding);
            }
            wire.chunked(body_limit)?
        } else if let Some(length) = response.header("content-length") {
            if length.is_empty() || !length.bytes().all(|byte| byte.is_ascii_digit()) {
                return Err(FetchError::MalformedResponse);
            }
            let length: usize = length.parse().map_err(|_| FetchError::BodyTooLarge)?;
            if length > body_limit {
                return Err(FetchError::BodyTooLarge);
            }
            wire.take(length)?
        } else {
            wire.until_close(body_limit)?
        };
        Ok(response)
    }
}

/// A fetched page: the requested URL, where it was finally read from, the
/// canonical text and media type, the canonical bytes and their digest.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct FetchedPage {
    pub url: String,
    pub final_url: String,
    pub media_type: String,
    pub text: String,
    pub canonical: Vec<u8>,
    pub digest: [u8; 32],
}

/// Fetches pages under the configured limits, destination rules and
/// robots.txt.
pub struct Fetcher {
    limits: FetchLimits,
    client: HttpClient,
    robots: RobotsCache,
}

const fn redirect(status: u16) -> bool {
    matches!(status, 301 | 302 | 303 | 307 | 308)
}

impl Fetcher {
    /// # Errors
    /// Refuses limits beyond a 3 second connect limit, a 10 second total
    /// limit, a 2 MiB body and three redirects, and zero limits.
    pub fn new(limits: FetchLimits) -> Result<Self, FetchError> {
        let valid = (1..=CONNECT_TIMEOUT_LIMIT_MS).contains(&limits.connect_timeout_ms)
            && (limits.connect_timeout_ms..=TOTAL_TIMEOUT_LIMIT_MS)
                .contains(&limits.total_timeout_ms)
            && (1..=BODY_LIMIT_BYTES).contains(&limits.max_body_bytes)
            && limits.max_redirects <= REDIRECT_LIMIT;
        if !valid {
            return Err(FetchError::InvalidLimits);
        }
        Ok(Self {
            client: HttpClient::new(limits.connect_timeout())?,
            limits,
            robots: RobotsCache::new(),
        })
    }

    #[must_use]
    pub const fn limits(&self) -> &FetchLimits {
        &self.limits
    }

    fn body_limit(&self) -> usize {
        usize::try_from(self.limits.max_body_bytes).unwrap_or(usize::MAX)
    }

    /// Resolves the host and refuses the URL when any resolved address is a
    /// forbidden destination.
    ///
    /// # Errors
    /// Refuses an unresolvable host and a forbidden destination.
    pub fn destination(&self, url: &Url) -> Result<SocketAddr, FetchError> {
        let addresses: Vec<SocketAddr> = (url.bare_host(), url.port)
            .to_socket_addrs()
            .map_err(|_| FetchError::Resolve)?
            .collect();
        let first = *addresses.first().ok_or(FetchError::Resolve)?;
        if addresses
            .iter()
            .any(|address| !destination_permitted(address.ip(), self.limits.allow_loopback))
        {
            return Err(FetchError::ForbiddenDestination);
        }
        Ok(first)
    }

    fn robots_for(
        &self,
        url: &Url,
        address: SocketAddr,
        deadline: Instant,
    ) -> Result<Robots, FetchError> {
        let origin = url.origin();
        if let Some(robots) = self.robots.get(&origin) {
            return Ok(robots);
        }
        let mut current = Url {
            target: "/robots.txt".to_owned(),
            ..url.clone()
        };
        let mut address = address;
        let mut redirects = 0_u8;
        let robots = loop {
            let response = self
                .client
                .get(
                    &current,
                    address,
                    deadline,
                    self.body_limit(),
                    &[("Accept", "text/plain")],
                )
                .map_err(robots_failure)?;
            match response.status {
                200..=299 => {
                    let text = std::str::from_utf8(&response.body)
                        .map_err(|_| FetchError::RobotsUnavailable)?;
                    break Robots::parse(text);
                }
                status if redirect(status) => {
                    if redirects >= self.limits.max_redirects {
                        return Err(FetchError::RobotsUnavailable);
                    }
                    redirects += 1;
                    let location = response
                        .header("location")
                        .ok_or(FetchError::RobotsUnavailable)?;
                    current = current
                        .join(location)
                        .map_err(|_| FetchError::RobotsUnavailable)?;
                    address = self.destination(&current)?;
                }
                400..=499 => break Robots::allow_all(),
                _ => return Err(FetchError::RobotsUnavailable),
            }
        };
        self.robots.insert(&origin, robots.clone());
        Ok(robots)
    }

    /// Fetches a URL: destination and robots.txt checks before every
    /// request, at most the configured redirects each checked again, the
    /// body limit and the time limits, then extraction and the canonical
    /// bytes with the requested URL as the payload.
    ///
    /// # Errors
    /// Returns the refusal that stopped the fetch.
    pub fn fetch(&self, url: &str) -> Result<FetchedPage, FetchError> {
        let deadline = Instant::now() + self.limits.total_timeout();
        let mut current = Url::parse(url)?;
        let mut redirects = 0_u8;
        loop {
            let address = self.destination(&current)?;
            if !self
                .robots_for(&current, address, deadline)?
                .allows(&current.target)
            {
                return Err(FetchError::RobotsDisallowed);
            }
            let response = self.client.get(
                &current,
                address,
                deadline,
                self.body_limit(),
                &[("Accept", ACCEPT)],
            )?;
            match response.status {
                200..=299 => {
                    let extracted =
                        extract::extract(response.header("content-type"), &response.body)
                            .map_err(FetchError::Extract)?;
                    let canonical = canonical::canonical_bytes(
                        ContentKind::Fetch,
                        url.as_bytes(),
                        &extracted.media_type,
                        &extracted.text,
                    )
                    .map_err(FetchError::Canonical)?;
                    if canonical.len() > MAX_CONTENT_BYTES {
                        return Err(FetchError::ContentTooLarge);
                    }
                    return Ok(FetchedPage {
                        url: url.to_owned(),
                        final_url: current.to_string(),
                        media_type: extracted.media_type,
                        text: extracted.text,
                        digest: canonical::content_digest(&canonical),
                        canonical,
                    });
                }
                status if redirect(status) => {
                    if redirects >= self.limits.max_redirects {
                        return Err(FetchError::TooManyRedirects);
                    }
                    redirects += 1;
                    let location = response
                        .header("location")
                        .ok_or(FetchError::MalformedResponse)?;
                    current = current.join(location)?;
                }
                status => return Err(FetchError::Status(status)),
            }
        }
    }
}

const fn robots_failure(error: FetchError) -> FetchError {
    match error {
        FetchError::Timeout | FetchError::ConnectTimeout | FetchError::ForbiddenDestination => {
            error
        }
        _ => FetchError::RobotsUnavailable,
    }
}

/// The `GET /fetch?url=` resource: fetches the page, writes its canonical
/// bytes to the content store and answers with the text and its digest.
#[must_use]
pub fn fetch_route(fetcher: &Fetcher, store: &ContentStore, request: &Request) -> Response {
    let url = match request.query_param("url") {
        Ok(Some(url)) if !url.is_empty() => url,
        Ok(_) => return Response::error(400, "missing_url"),
        Err(QueryError::Duplicate) => return Response::error(400, "duplicate_url"),
        Err(QueryError::Malformed) => return Response::error(400, "malformed_query"),
    };
    let page = match fetcher.fetch(&url) {
        Ok(page) => page,
        Err(error) => return Response::error(error.status(), error.code()),
    };
    if store.put(&page.canonical).is_err() {
        return Response::error(500, "content_store_error");
    }
    Response::json(
        200,
        serde_json::json!({
            "url": page.url,
            "final_url": page.final_url,
            "media_type": page.media_type,
            "digest": canonical::digest_hex(&page.digest),
            "length": page.text.len(),
            "text": page.text,
        })
        .to_string()
        .into_bytes(),
    )
}
