use std::collections::BTreeMap;
use std::io::{Read, Write};
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

use serde_json::{json, Value};
use subtle::ConstantTimeEq;
use zeroize::Zeroize;

use crate::trace::TraceId;

use super::backend::{
    ApiFailure, BackendResponse, HumanApiComponents, PrincipalContext, ScopedRequest,
    SessionCredentials, SessionSecrets,
};
use super::limits::PrincipalLimits;
use super::schema::{ApiSchema, Operation};

const ACCESS_COOKIE: &str = "__Host-layerx_access";
const REFRESH_COOKIE: &str = "__Host-layerx_refresh";
const CSRF_COOKIE: &str = "__Host-layerx_csrf";

/// Finite HTTP parsing and browser-origin policy.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct HttpConfig {
    pub maximum_header_bytes: usize,
    pub maximum_body_bytes: usize,
    pub allowed_origin: String,
    pub service_version: String,
}

impl HttpConfig {
    /// Refuses disabled bounds or a non-HTTPS browser origin.
    ///
    /// # Errors
    ///
    /// Returns a startup failure before the service binds.
    pub fn validate(self) -> Result<Self, ApiFailure> {
        let origin_authority = self.allowed_origin.strip_prefix("https://");
        if self.maximum_header_bytes == 0
            || self.maximum_body_bytes == 0
            || origin_authority.is_none_or(str::is_empty)
            || origin_authority.is_some_and(|authority| {
                authority.bytes().any(|byte| {
                    byte.is_ascii_whitespace()
                        || byte.is_ascii_control()
                        || matches!(byte, b'/' | b'?' | b'#' | b'\\')
                })
            })
            || self.allowed_origin.ends_with('/')
            || self.service_version.is_empty()
        {
            return Err(ApiFailure::unavailable());
        }
        Ok(self)
    }
}

/// One bounded synchronous HTTPS request router. Business actions can only cross
/// the supplied production component boundary after schema and session admission.
pub struct Router<B: HumanApiComponents> {
    schema: ApiSchema,
    backend: Arc<B>,
    limits: PrincipalLimits,
    config: HttpConfig,
}

impl<B: HumanApiComponents> Router<B> {
    /// Creates a router from the embedded schema and explicit finite policies.
    ///
    /// # Errors
    ///
    /// Refuses malformed embedded schema or invalid HTTP policy.
    pub fn new(
        backend: Arc<B>,
        limits: PrincipalLimits,
        config: HttpConfig,
    ) -> Result<Self, ApiFailure> {
        let schema = ApiSchema::v1().map_err(|_| ApiFailure::unavailable())?;
        Ok(Self {
            schema,
            backend,
            limits,
            config: config.validate()?,
        })
    }

    #[must_use]
    pub const fn schema(&self) -> &ApiSchema {
        &self.schema
    }

    /// Reads, routes and writes exactly one HTTP request on an established TLS stream.
    ///
    /// # Errors
    ///
    /// Returns only connection I/O failures; application failures are structured JSON.
    pub fn serve_one<S: Read + Write>(
        &self,
        stream: &mut S,
        public_rate_key: &str,
    ) -> std::io::Result<()> {
        let request = match HttpRequest::read(stream, &self.config) {
            Ok(request) => request,
            Err(failure) => {
                let trace = match mint_trace(None) {
                    Ok(trace) | Err(trace) => trace,
                };
                return write_response(stream, error_response(&trace, failure));
            }
        };
        let response = self.handle(request, public_rate_key);
        write_response(stream, response)
    }

    fn handle(&self, mut request: HttpRequest, public_rate_key: &str) -> HttpResponse {
        let trace = match mint_trace(request.header("x-layerx-trace")) {
            Ok(trace) => trace,
            Err(trace) => return error_response(&trace, ApiFailure::unavailable()),
        };
        if request.method == "GET" && request.path == "/livez" {
            return success_response(
                200,
                &trace,
                json!({ "live": true, "service": "layerx-human-service" }),
                Vec::new(),
            );
        }
        if request.method == "GET" && request.path == "/readyz" {
            return match self.backend.readiness(trace.as_str()) {
                Ok(readiness) => success_response(
                    if readiness.ready() { 200 } else { 503 },
                    &trace,
                    readiness.redacted(),
                    Vec::new(),
                ),
                Err(failure) => error_response(&trace, failure),
            };
        }
        let matched = match self.schema.route(&request.method, &request.path) {
            Ok(Some(matched)) => matched,
            Ok(None) => return error_response(&trace, ApiFailure::not_found()),
            Err(_) => return error_response(&trace, ApiFailure::invalid_request(None)),
        };
        let operation = matched.operation;
        if operation.name == "version" {
            if let Err(failure) = self.limits.admit(public_rate_key, unix_seconds()) {
                return error_response(&trace, failure);
            }
            let (major, minor) = self.schema.version();
            return success_response(
                200,
                &trace,
                json!({
                    "schema": { "major": major, "minor": minor },
                    "service": self.config.service_version.as_str()
                }),
                Vec::new(),
            );
        }
        if operation.mutates()
            && !operation.is_public_bootstrap()
            && !same_origin(request.header("origin"), &self.config.allowed_origin)
        {
            return error_response(&trace, ApiFailure::forbidden());
        }
        let cookies = match parse_cookies(request.header("cookie")) {
            Ok(cookies) => cookies,
            Err(failure) => return error_response(&trace, failure),
        };
        let principal = if operation.is_public_bootstrap() {
            if let Err(failure) = self.limits.admit(public_rate_key, unix_seconds()) {
                return error_response(&trace, failure);
            }
            None
        } else {
            let credential_name = if operation.uses_refresh_cookie() {
                REFRESH_COOKIE
            } else {
                ACCESS_COOKIE
            };
            let Some(access_token) = cookies.get(credential_name) else {
                return error_response(&trace, ApiFailure::unauthenticated());
            };
            let csrf_cookie = cookies.get(CSRF_COOKIE).map(String::as_str);
            if operation.mutates()
                && !csrf_matches(csrf_cookie, request.header("x-layerx-csrf"))
            {
                return error_response(&trace, ApiFailure::forbidden());
            }
            let context = match self.backend.authorize(
                operation,
                SessionCredentials {
                    access_token,
                    csrf_token: csrf_cookie,
                    intended_destination: &request.path,
                    refresh: operation.uses_refresh_cookie(),
                },
                trace.as_str(),
            ) {
                Ok(context) => context,
                Err(failure) => return error_response(&trace, failure),
            };
            if let Err(failure) = self.limits.admit(context.principal.as_str(), unix_seconds()) {
                return error_response(&trace, failure);
            }
            Some(context)
        };
        let idempotency_key = match idempotency_key(operation, request.header("idempotency-key")) {
            Ok(key) => key,
            Err(failure) => return error_response(&trace, failure),
        };
        let body = match request.json_body(operation.request != "Empty") {
            Ok(body) => body,
            Err(failure) => return error_response(&trace, failure),
        };
        let body = match self.schema.decode_request(operation, body) {
            Ok(body) => body,
            Err(error) => {
                let field = error
                    .detail()
                    .split_whitespace()
                    .next()
                    .filter(|value| value.starts_with("request."));
                return error_response(&trace, ApiFailure::invalid_request(field));
            }
        };
        let clear_session = should_clear_session(operation, principal.as_ref(), &matched.path_parameters);
        let response = self.backend.execute(ScopedRequest {
            operation,
            principal: principal.as_ref(),
            path_parameters: matched.path_parameters,
            body,
            idempotency_key,
            trace: trace.as_str().to_owned(),
        });
        match response {
            Ok(BackendResponse { result, session }) => {
                if self.schema.encode_response(operation, &result).is_err() {
                    return error_response(&trace, ApiFailure::upstream_degraded());
                }
                let mut headers = session.map_or_else(Vec::new, session_cookie_headers);
                if clear_session {
                    headers.extend(clear_session_headers());
                }
                success_response(success_status(operation), &trace, result, headers)
            }
            Err(failure) => error_response(&trace, failure),
        }
    }
}

fn idempotency_key(
    operation: &Operation,
    supplied: Option<&str>,
) -> Result<Option<String>, ApiFailure> {
    if !operation.idempotency {
        return Ok(None);
    }
    let key = supplied
        .filter(|value| !value.is_empty() && value.len() <= 255)
        .filter(|value| {
            value
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.' | b':'))
        })
        .ok_or_else(|| ApiFailure::invalid_request(Some("Idempotency-Key")))?;
    Ok(Some(key.to_owned()))
}

fn same_origin(origin: Option<&str>, allowed: &str) -> bool {
    origin.is_some_and(|value| bool::from(value.as_bytes().ct_eq(allowed.as_bytes())))
}

fn csrf_matches(cookie: Option<&str>, header: Option<&str>) -> bool {
    match (cookie, header) {
        (Some(cookie), Some(header)) if cookie.len() == header.len() => {
            bool::from(cookie.as_bytes().ct_eq(header.as_bytes()))
        }
        _ => false,
    }
}

fn should_clear_session(
    operation: &Operation,
    principal: Option<&PrincipalContext>,
    path_parameters: &BTreeMap<String, String>,
) -> bool {
    if matches!(
        operation.name.as_str(),
        "session.revoke-all" | "security.session.revoke-all"
    ) {
        return true;
    }
    operation.name == "session.revoke"
        && principal.is_some_and(|context| {
            path_parameters
                .get("session_id")
                .is_some_and(|session| session == &context.session_id)
        })
}

fn success_status(operation: &Operation) -> u16 {
    match operation.name.as_str() {
        "account.create" | "agent.create" | "deposit.start" | "session.open"
        | "support.create" => 201,
        _ => 200,
    }
}

fn mint_trace(inbound: Option<&str>) -> Result<TraceId, TraceId> {
    if let Some(inbound) = inbound {
        if let Ok(trace) = TraceId::parse(inbound) {
            return Ok(trace);
        }
    }
    let mut entropy = [0_u8; 16];
    if getrandom::fill(&mut entropy).is_err() {
        return Err(TraceId::mint([0_u8; 16]));
    }
    Ok(TraceId::mint(entropy))
}

fn unix_seconds() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |duration| duration.as_secs())
}

struct HttpRequest {
    method: String,
    path: String,
    headers: BTreeMap<String, String>,
    body: Vec<u8>,
}

impl HttpRequest {
    fn read(stream: &mut impl Read, config: &HttpConfig) -> Result<Self, ApiFailure> {
        let mut received = Vec::new();
        let header_end = loop {
            if received.len() >= config.maximum_header_bytes {
                return Err(ApiFailure::invalid_request(None));
            }
            let mut block = [0_u8; 4096];
            let read = stream.read(&mut block).map_err(|_| ApiFailure::invalid_request(None))?;
            if read == 0 {
                return Err(ApiFailure::invalid_request(None));
            }
            received.extend_from_slice(&block[..read]);
            if let Some(end) = received.windows(4).position(|window| window == b"\r\n\r\n") {
                if end + 4 > config.maximum_header_bytes {
                    return Err(ApiFailure::invalid_request(None));
                }
                break end + 4;
            }
        };
        let header_text = std::str::from_utf8(&received[..header_end])
            .map_err(|_| ApiFailure::invalid_request(None))?;
        let mut lines = header_text[..header_text.len().saturating_sub(4)].split("\r\n");
        let request_line = lines.next().ok_or_else(|| ApiFailure::invalid_request(None))?;
        let mut request_parts = request_line.split(' ');
        let method = request_parts.next().unwrap_or_default();
        let target = request_parts.next().unwrap_or_default();
        let version = request_parts.next().unwrap_or_default();
        if request_parts.next().is_some()
            || !matches!(method, "DELETE" | "GET" | "PATCH" | "POST" | "PUT")
            || version != "HTTP/1.1"
        {
            return Err(ApiFailure::invalid_request(None));
        }
        if target.is_empty()
            || !target.starts_with('/')
            || target.contains('?')
            || target.contains('#')
        {
            return Err(ApiFailure::invalid_request(None));
        }
        let method = method.to_owned();
        let path = target.to_owned();
        let mut headers = BTreeMap::new();
        for line in lines {
            let (name, value) = line
                .split_once(':')
                .ok_or_else(|| ApiFailure::invalid_request(None))?;
            let name = name.trim().to_ascii_lowercase();
            let value = value.trim();
            if name.is_empty()
                || !name.bytes().all(header_name_byte)
                || value.bytes().any(|byte| byte.is_ascii_control() && byte != b'\t')
                || headers.insert(name, value.to_owned()).is_some()
            {
                return Err(ApiFailure::invalid_request(None));
            }
        }
        if headers.contains_key("transfer-encoding") || !headers.contains_key("host") {
            return Err(ApiFailure::invalid_request(None));
        }
        if headers.get("host").is_none_or(String::is_empty) {
            return Err(ApiFailure::invalid_request(None));
        }
        let content_length = headers
            .get("content-length")
            .map(|value| value.parse::<usize>())
            .transpose()
            .map_err(|_| ApiFailure::invalid_request(None))?
            .unwrap_or(0);
        if content_length > config.maximum_body_bytes {
            return Err(ApiFailure::invalid_request(None));
        }
        let mut body = received.split_off(header_end);
        if body.len() > content_length {
            return Err(ApiFailure::invalid_request(None));
        }
        while body.len() < content_length {
            let remaining = content_length - body.len();
            let mut block = [0_u8; 4096];
            let wanted = remaining.min(block.len());
            let read = stream
                .read(&mut block[..wanted])
                .map_err(|_| ApiFailure::invalid_request(None))?;
            if read == 0 {
                return Err(ApiFailure::invalid_request(None));
            }
            body.extend_from_slice(&block[..read]);
        }
        if !body.is_empty()
            && headers.get("content-type").is_none_or(|value| {
                value
                    .split(';')
                    .next()
                    .is_none_or(|media| media.trim() != "application/json")
            })
        {
            return Err(ApiFailure::invalid_request(Some("Content-Type")));
        }
        Ok(Self {
            method,
            path,
            headers,
            body,
        })
    }

    fn header(&self, name: &str) -> Option<&str> {
        self.headers.get(name).map(String::as_str)
    }

    fn json_body(&mut self, required: bool) -> Result<Option<Value>, ApiFailure> {
        if self.body.is_empty() {
            return if required {
                Err(ApiFailure::invalid_request(None))
            } else {
                Ok(None)
            };
        }
        let decoded = serde_json::from_slice(&self.body);
        self.body.zeroize();
        decoded
            .map(Some)
            .map_err(|_| ApiFailure::invalid_request(None))
    }
}

impl Drop for HttpRequest {
    fn drop(&mut self) {
        self.body.zeroize();
        self.path.zeroize();
        for value in self.headers.values_mut() {
            value.zeroize();
        }
    }
}

struct Cookies(BTreeMap<String, String>);

impl Cookies {
    fn get(&self, name: &str) -> Option<&String> {
        self.0.get(name)
    }
}

impl Drop for Cookies {
    fn drop(&mut self) {
        for value in self.0.values_mut() {
            value.zeroize();
        }
    }
}

fn parse_cookies(value: Option<&str>) -> Result<Cookies, ApiFailure> {
    let mut cookies = BTreeMap::new();
    let Some(value) = value else {
        return Ok(Cookies(cookies));
    };
    if value.len() > 16_384 {
        return Err(ApiFailure::invalid_request(None));
    }
    for cookie in value.split(';') {
        let (name, value) = cookie
            .trim()
            .split_once('=')
            .ok_or_else(|| ApiFailure::invalid_request(None))?;
        if !matches!(name, ACCESS_COOKIE | REFRESH_COOKIE | CSRF_COOKIE) {
            continue;
        }
        if value.is_empty()
            || value.len() > 4096
            || !value.bytes().all(cookie_value_byte)
            || cookies.insert(name.to_owned(), value.to_owned()).is_some()
        {
            return Err(ApiFailure::invalid_request(None));
        }
    }
    Ok(Cookies(cookies))
}

const fn cookie_value_byte(byte: u8) -> bool {
    byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.' | b'~')
}

const fn header_name_byte(byte: u8) -> bool {
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

struct HttpResponse {
    status: u16,
    trace: String,
    body: Vec<u8>,
    headers: Vec<(&'static str, String)>,
}

impl Drop for HttpResponse {
    fn drop(&mut self) {
        self.body.zeroize();
        self.trace.zeroize();
        for (_, value) in &mut self.headers {
            value.zeroize();
        }
    }
}

fn success_response(
    status: u16,
    trace: &TraceId,
    result: Value,
    headers: Vec<(&'static str, String)>,
) -> HttpResponse {
    response(status, trace, json!({ "ok": true, "result": result, "trace": trace.as_str() }), headers)
}

fn error_response(trace: &TraceId, failure: ApiFailure) -> HttpResponse {
    response(
        failure.status,
        trace,
        json!({ "ok": false, "error": failure.envelope(), "trace": trace.as_str() }),
        Vec::new(),
    )
}

fn response(
    status: u16,
    trace: &TraceId,
    mut envelope: Value,
    headers: Vec<(&'static str, String)>,
) -> HttpResponse {
    let encoded = serde_json::to_vec(&envelope);
    zeroize_json(&mut envelope);
    let body = match encoded {
        Ok(body) => body,
        Err(_) => format!(
            "{{\"ok\":false,\"error\":{{\"code\":\"unavailable\",\"copy_key\":\"error.service.unavailable\",\"retry\":\"retriable\"}},\"trace\":\"{}\"}}",
            trace.as_str()
        )
        .into_bytes(),
    };
    HttpResponse {
        status,
        trace: trace.as_str().to_owned(),
        body,
        headers,
    }
}

fn zeroize_json(value: &mut Value) {
    match value {
        Value::String(text) => text.zeroize(),
        Value::Array(entries) => entries.iter_mut().for_each(zeroize_json),
        Value::Object(entries) => entries.values_mut().for_each(zeroize_json),
        Value::Null | Value::Bool(_) | Value::Number(_) => {}
    }
}

fn session_cookie_headers(session: SessionSecrets) -> Vec<(&'static str, String)> {
    vec![
        (
            "Set-Cookie",
            protected_cookie(
                ACCESS_COOKIE,
                &session.access_token,
                session.access_max_age_seconds,
                true,
            ),
        ),
        (
            "Set-Cookie",
            protected_cookie(
                REFRESH_COOKIE,
                &session.refresh_token,
                session.refresh_max_age_seconds,
                true,
            ),
        ),
        (
            "Set-Cookie",
            protected_cookie(
                CSRF_COOKIE,
                &session.csrf_token,
                session.refresh_max_age_seconds,
                false,
            ),
        ),
    ]
}

fn protected_cookie(name: &str, value: &str, max_age: u64, http_only: bool) -> String {
    format!(
        "{name}={value}; Path=/; Max-Age={max_age}; Secure; SameSite=Strict{}",
        if http_only { "; HttpOnly" } else { "" }
    )
}

fn clear_session_headers() -> Vec<(&'static str, String)> {
    [ACCESS_COOKIE, REFRESH_COOKIE, CSRF_COOKIE]
        .into_iter()
        .map(|name| {
            (
                "Set-Cookie",
                format!(
                    "{name}=; Path=/; Max-Age=0; Secure; SameSite=Strict{}",
                    if name == CSRF_COOKIE { "" } else { "; HttpOnly" }
                ),
            )
        })
        .collect()
}

fn write_response(stream: &mut impl Write, response: HttpResponse) -> std::io::Result<()> {
    write!(
        stream,
        "HTTP/1.1 {} {}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nCache-Control: no-store\r\nX-Content-Type-Options: nosniff\r\nX-LayerX-Trace: {}\r\nConnection: close\r\n",
        response.status,
        reason(response.status),
        response.body.len(),
        response.trace
    )?;
    for (name, value) in &response.headers {
        write!(stream, "{name}: {value}\r\n")?;
    }
    stream.write_all(b"\r\n")?;
    stream.write_all(&response.body)?;
    stream.flush()
}

const fn reason(status: u16) -> &'static str {
    match status {
        200 => "OK",
        201 => "Created",
        400 => "Bad Request",
        401 => "Unauthorized",
        403 => "Forbidden",
        404 => "Not Found",
        409 => "Conflict",
        429 => "Too Many Requests",
        503 => "Service Unavailable",
        _ => "Error",
    }
}
