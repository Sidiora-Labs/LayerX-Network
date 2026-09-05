mod seal;
mod store;

use rustls::pki_types::{CertificateDer, PrivateKeyDer, PrivatePkcs8KeyDer};
use rustls::server::WebPkiClientVerifier;
use rustls::{RootCertStore, ServerConfig, ServerConnection, StreamOwned};
use seal::{hex, sha256_hex, StoreKey};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::env;
use std::fs;
use std::io::{Read, Write};
use std::net::{SocketAddr, TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use store::{Principal, Store, StoredSession};
use subtle::ConstantTimeEq;
use zeroize::{Zeroize, Zeroizing};

const MAX_REQUEST_BYTES: usize = 16 * 1024;
const IO_TIMEOUT: Duration = Duration::from_secs(8);
const MAX_CONNECTIONS: usize = 128;
const MAX_SIGNER_KEYS: usize = 128;
const MAX_AUDIENCES: usize = 32;
const MAX_SESSION_TTL_SECONDS: u64 = 30 * 86_400;
const SESSION_ID_BYTES: usize = 16;
const SESSION_SECRET_BYTES: usize = 32;
const CSRF_BYTES: usize = 32;
const MIN_SERVICE_TOKEN_BYTES: usize = 16;
static ACTIVE_CONNECTIONS: AtomicUsize = AtomicUsize::new(0);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Service {
    Gateway,
    Webhooks,
    Dashboard,
    Faucet,
    Testnet,
    Ramp,
    Provisioning,
}

impl Service {
    const ALL: [Self; 7] = [
        Self::Gateway,
        Self::Webhooks,
        Self::Dashboard,
        Self::Faucet,
        Self::Testnet,
        Self::Ramp,
        Self::Provisioning,
    ];

    const fn name(self) -> &'static str {
        match self {
            Self::Gateway => "gateway",
            Self::Webhooks => "webhooks",
            Self::Dashboard => "dashboard",
            Self::Faucet => "faucet",
            Self::Testnet => "testnet",
            Self::Ramp => "ramp",
            Self::Provisioning => "provisioning",
        }
    }
}

struct ServiceToken {
    service: Service,
    token: Zeroizing<String>,
}

struct Config {
    listen: SocketAddr,
    tls: Arc<ServerConfig>,
    state_dir: PathBuf,
    store_key: StoreKey,
    service_tokens: Vec<ServiceToken>,
    default_ttl_seconds: u64,
}

struct Shared {
    config: Config,
    store: Mutex<Store>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct IntrospectRequest {
    token: String,
    #[serde(default)]
    audience: Option<String>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct PrincipalRequest {
    sub: String,
    allowed_signer_public_keys: Vec<String>,
    #[serde(default)]
    account: Option<String>,
    #[serde(default)]
    audiences: Vec<String>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct SessionRequest {
    sub: String,
    #[serde(default)]
    ttl_seconds: Option<u64>,
}

#[derive(Serialize)]
struct GatewayShape<'a> {
    active: bool,
    sub: &'a str,
    allowed_signer_public_keys: &'a [String],
}

#[derive(Serialize)]
struct DeveloperShape<'a> {
    active: bool,
    sub: &'a str,
    csrf_token: &'a str,
}

#[derive(Serialize)]
struct SubjectShape<'a> {
    active: bool,
    sub: &'a str,
}

#[derive(Serialize)]
struct RampShape<'a> {
    active: bool,
    principal_id: &'a str,
    account: &'a str,
    audience: &'a str,
    expires_at: u64,
}

#[derive(Serialize)]
struct PrincipalResponse<'a> {
    sub: &'a str,
    allowed_signer_public_keys: &'a [String],
    account: Option<&'a str>,
    audiences: &'a [String],
}

#[derive(Serialize)]
struct SessionResponse<'a> {
    session_id: &'a str,
    sub: &'a str,
    token: &'a str,
    csrf_token: &'a str,
    expires_at: u64,
}

#[derive(Serialize)]
struct RevocationResponse<'a> {
    session_id: &'a str,
    revoked: bool,
    revoked_at: u64,
}

struct Request {
    method: String,
    path: String,
    headers: BTreeMap<String, String>,
    body: Vec<u8>,
}

impl Drop for Request {
    fn drop(&mut self) {
        for value in self.headers.values_mut() {
            value.zeroize();
        }
        self.body.zeroize();
    }
}

struct Response {
    status: u16,
    body: String,
    retry_after: Option<u64>,
}

struct ActiveSession {
    principal: Principal,
    csrf_token: Zeroizing<String>,
    expires_at: u64,
}

fn unix_seconds() -> Result<u64, String> {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_secs())
        .map_err(|_| "system clock precedes Unix epoch".to_owned())
}

fn valid_sub(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 128
        && value.bytes().all(|byte| {
            byte.is_ascii_lowercase()
                || byte.is_ascii_digit()
                || matches!(byte, b'-' | b'_' | b'.' | b':')
        })
}

fn valid_identifier(value: &str, max: usize) -> bool {
    !value.is_empty()
        && value.len() <= max
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.' | b':'))
}

fn valid_hex(value: &str, bytes: usize) -> bool {
    value.len() == bytes * 2
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

fn valid_signer_keys(keys: &[String]) -> bool {
    keys.len() <= MAX_SIGNER_KEYS
        && keys.iter().all(|key| valid_hex(key, 32))
        && keys
            .iter()
            .enumerate()
            .all(|(index, key)| !keys[..index].contains(key))
}

fn read_secret_file(path: &Path) -> Result<Zeroizing<String>, String> {
    let mut value =
        fs::read_to_string(path).map_err(|error| format!("{}: {error}", path.display()))?;
    while matches!(value.as_bytes().last(), Some(b'\n' | b'\r')) {
        value.pop();
    }
    if value.is_empty() || value.len() > 4096 || value.bytes().any(|byte| byte.is_ascii_control()) {
        value.zeroize();
        return Err(format!(
            "{} does not contain a bounded secret",
            path.display()
        ));
    }
    Ok(Zeroizing::new(value))
}

fn read_secret(path_variable: &str) -> Result<Zeroizing<String>, String> {
    let path = env::var(path_variable).map_err(|_| format!("{path_variable} is required"))?;
    read_secret_file(Path::new(&path))
}

fn parse_u64(name: &str, default: u64) -> Result<u64, String> {
    env::var(name).map_or(Ok(default), |value| {
        value
            .parse::<u64>()
            .map_err(|_| format!("{name} must be an integer"))
    })
}

fn service_tokens(directory: &Path) -> Result<Vec<ServiceToken>, String> {
    let entries = fs::read_dir(directory)
        .map_err(|error| format!("LAYERX_IDENTITY_SERVICE_TOKENS_DIR: {error}"))?;
    for entry in entries {
        let entry = entry.map_err(|error| error.to_string())?;
        let name = entry.file_name();
        let name = name.to_string_lossy();
        if name.starts_with("..") {
            continue;
        }
        if !Service::ALL.iter().any(|service| service.name() == name) {
            return Err(format!(
                "service token file {name} names no calling service"
            ));
        }
    }
    let mut tokens = Vec::with_capacity(Service::ALL.len());
    for service in Service::ALL {
        let token = read_secret_file(&directory.join(service.name()))
            .map_err(|error| format!("service token for {}: {error}", service.name()))?;
        if token.len() < MIN_SERVICE_TOKEN_BYTES {
            return Err(format!(
                "service token for {} must be at least {MIN_SERVICE_TOKEN_BYTES} bytes",
                service.name()
            ));
        }
        if tokens
            .iter()
            .any(|existing: &ServiceToken| existing.token.as_str() == token.as_str())
        {
            return Err(format!(
                "service token for {} duplicates another service",
                service.name()
            ));
        }
        tokens.push(ServiceToken { service, token });
    }
    Ok(tokens)
}

fn server_tls_config() -> Result<Arc<ServerConfig>, String> {
    rustls::crypto::ring::default_provider()
        .install_default()
        .map_err(|_| "failed to install TLS crypto provider".to_owned())?;
    let certificate_path =
        env::var("LAYERX_IDENTITY_TLS_CERT_DER").map_err(|_| "TLS certificate is required")?;
    let key_path =
        env::var("LAYERX_IDENTITY_TLS_KEY_DER").map_err(|_| "TLS private key is required")?;
    let certificate = CertificateDer::from(fs::read(certificate_path).map_err(|e| e.to_string())?);
    let key = PrivateKeyDer::from(PrivatePkcs8KeyDer::from(
        fs::read(key_path).map_err(|e| e.to_string())?,
    ));
    let builder = ServerConfig::builder();
    let config = match env::var("LAYERX_IDENTITY_CLIENT_CA_DER") {
        Ok(path) => {
            let mut roots = RootCertStore::empty();
            roots
                .add(CertificateDer::from(
                    fs::read(path).map_err(|error| error.to_string())?,
                ))
                .map_err(|error| error.to_string())?;
            let verifier = WebPkiClientVerifier::builder(Arc::new(roots))
                .build()
                .map_err(|error| error.to_string())?;
            builder.with_client_cert_verifier(verifier)
        }
        Err(_) => builder.with_no_client_auth(),
    }
    .with_single_cert(vec![certificate], key)
    .map_err(|error| error.to_string())?;
    Ok(Arc::new(config))
}

fn config() -> Result<Config, String> {
    let listen = env::var("LAYERX_IDENTITY_LISTEN")
        .unwrap_or_else(|_| "0.0.0.0:9443".to_owned())
        .parse::<SocketAddr>()
        .map_err(|_| "LAYERX_IDENTITY_LISTEN must be a socket address".to_owned())?;
    let state_dir = PathBuf::from(
        env::var("LAYERX_IDENTITY_STATE_DIR")
            .map_err(|_| "LAYERX_IDENTITY_STATE_DIR is required")?,
    );
    let tokens_dir = PathBuf::from(
        env::var("LAYERX_IDENTITY_SERVICE_TOKENS_DIR")
            .map_err(|_| "LAYERX_IDENTITY_SERVICE_TOKENS_DIR is required")?,
    );
    let default_ttl_seconds = parse_u64("LAYERX_IDENTITY_SESSION_TTL_SECONDS", 86_400)?;
    if default_ttl_seconds == 0 || default_ttl_seconds > MAX_SESSION_TTL_SECONDS {
        return Err(format!(
            "LAYERX_IDENTITY_SESSION_TTL_SECONDS must be between 1 and {MAX_SESSION_TTL_SECONDS}"
        ));
    }
    let store_secret = read_secret("LAYERX_IDENTITY_STORE_KEY_FILE")?;
    Ok(Config {
        listen,
        tls: server_tls_config()?,
        state_dir,
        store_key: StoreKey::derive(store_secret.as_bytes()),
        service_tokens: service_tokens(&tokens_dir)?,
        default_ttl_seconds,
    })
}

fn read_http_message(stream: &mut impl Read, maximum: usize) -> Result<Request, String> {
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
    })
}

fn parse_client_request(stream: &mut impl Read) -> Result<Request, String> {
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
    if parts.next() != Some("HTTP/1.1") || parts.next().is_some() || request.path.contains('?') {
        return Err("request line is invalid".to_owned());
    }
    if !request.headers.contains_key("host") {
        return Err("HTTP/1.1 Host header is required".to_owned());
    }
    Ok(request)
}

fn resolve_service(tokens: &[ServiceToken], presented: &str) -> Option<Service> {
    let mut matched = None;
    for candidate in tokens {
        let equal = candidate
            .token
            .as_bytes()
            .ct_eq(presented.as_bytes())
            .unwrap_u8()
            == 1;
        if equal {
            matched = Some(candidate.service);
        }
    }
    matched
}

fn authenticate_service(config: &Config, request: &Request) -> Result<Service, Response> {
    let Some(token) = request
        .headers
        .get("authorization")
        .and_then(|value| value.strip_prefix("Bearer "))
    else {
        return Err(refusal(401, "service_token_required", None));
    };
    if token.is_empty() || token.len() > 4096 {
        return Err(refusal(401, "service_token_required", None));
    }
    resolve_service(&config.service_tokens, token)
        .ok_or_else(|| refusal(401, "service_token_required", None))
}

fn parse_session_token(token: &str) -> Option<(&str, &str)> {
    let rest = token.strip_prefix("ses_")?;
    let (id, secret) = rest.split_once('.')?;
    (valid_hex(id, SESSION_ID_BYTES) && valid_hex(secret, SESSION_SECRET_BYTES))
        .then_some((id, secret))
}

fn lookup_session(
    shared: &Shared,
    token: &str,
    now: u64,
) -> Result<Option<ActiveSession>, Response> {
    let Some((session_id, secret)) = parse_session_token(token) else {
        return Ok(None);
    };
    let presented = Zeroizing::new(sha256_hex(secret.as_bytes()));
    let store = shared
        .store
        .lock()
        .map_err(|_| refusal(503, "store_unavailable", Some(5)))?;
    store
        .check_available()
        .map_err(|_| refusal(503, "store_unavailable", Some(5)))?;
    let placeholder = StoredSession {
        session_id: String::new(),
        principal: String::new(),
        token_digest: "0".repeat(64),
        csrf_digest: String::new(),
        csrf_sealed: String::new(),
        issued_at: 0,
        expires_at: 0,
        revoked_at: None,
    };
    let found = store.session(session_id);
    let session = found.unwrap_or(&placeholder);
    let digest_matches = session
        .token_digest
        .as_bytes()
        .ct_eq(presented.as_bytes())
        .unwrap_u8()
        == 1;
    if found.is_none()
        || !digest_matches
        || session.revoked_at.is_some()
        || session.expires_at <= now
    {
        return Ok(None);
    }
    let Some(principal) = store.principal(&session.principal) else {
        return Ok(None);
    };
    let csrf = shared
        .config
        .store_key
        .open(&session.csrf_sealed)
        .map_err(|_| refusal(503, "store_unavailable", Some(5)))?;
    let csrf_token = Zeroizing::new(
        String::from_utf8(csrf.to_vec()).map_err(|_| refusal(503, "store_unavailable", Some(5)))?,
    );
    if sha256_hex(csrf_token.as_bytes())
        .as_bytes()
        .ct_eq(session.csrf_digest.as_bytes())
        .unwrap_u8()
        != 1
    {
        return Err(refusal(503, "store_unavailable", Some(5)));
    }
    Ok(Some(ActiveSession {
        principal: principal.clone(),
        csrf_token,
        expires_at: session.expires_at,
    }))
}

fn serialize<T: Serialize>(value: &T) -> Response {
    serde_json::to_string(value).map_or_else(
        |_| refusal(503, "encoding_failed", Some(5)),
        |body| Response {
            status: 200,
            body,
            retry_after: None,
        },
    )
}

fn introspect(shared: &Shared, service: Service, request: &Request) -> Response {
    if request.headers.get("content-type").map(String::as_str) != Some("application/json") {
        return refusal(400, "content_type_required", None);
    }
    let body: IntrospectRequest = match serde_json::from_slice(&request.body) {
        Ok(body) => body,
        Err(_) => return refusal(400, "invalid_argument", None),
    };
    let IntrospectRequest { token, audience } = body;
    let body = Zeroizing::new(token);
    let audience = match (service, audience) {
        (Service::Ramp, Some(audience)) if valid_identifier(&audience, 128) => Some(audience),
        (Service::Ramp, _) => return refusal(400, "audience_required", None),
        (_, None) => None,
        (_, Some(_)) => return refusal(400, "invalid_argument", None),
    };
    if body.is_empty() || body.len() > 4096 {
        return refusal(400, "invalid_argument", None);
    }
    let Ok(now) = unix_seconds() else {
        return refusal(503, "clock_unavailable", Some(5));
    };
    let session = match lookup_session(shared, &body, now) {
        Ok(session) => session,
        Err(response) => return response,
    };
    introspection_shape(service, session.as_ref(), audience)
}

fn introspection_shape(
    service: Service,
    session: Option<&ActiveSession>,
    audience: Option<String>,
) -> Response {
    match service {
        Service::Gateway => session.as_ref().map_or_else(
            || {
                serialize(&GatewayShape {
                    active: false,
                    sub: "",
                    allowed_signer_public_keys: &[],
                })
            },
            |active| {
                serialize(&GatewayShape {
                    active: true,
                    sub: &active.principal.sub,
                    allowed_signer_public_keys: &active.principal.allowed_signer_public_keys,
                })
            },
        ),
        Service::Webhooks | Service::Dashboard => session.as_ref().map_or_else(
            || {
                serialize(&DeveloperShape {
                    active: false,
                    sub: "",
                    csrf_token: "",
                })
            },
            |active| {
                serialize(&DeveloperShape {
                    active: true,
                    sub: &active.principal.sub,
                    csrf_token: &active.csrf_token,
                })
            },
        ),
        Service::Faucet | Service::Testnet => session.as_ref().map_or_else(
            || {
                serialize(&SubjectShape {
                    active: false,
                    sub: "",
                })
            },
            |active| {
                serialize(&SubjectShape {
                    active: true,
                    sub: &active.principal.sub,
                })
            },
        ),
        Service::Ramp => {
            let audience = audience.unwrap_or_default();
            let bound = session.as_ref().and_then(|active| {
                let account = active.principal.account.as_deref()?;
                active
                    .principal
                    .audiences
                    .iter()
                    .any(|granted| granted == &audience)
                    .then_some((active, account))
            });
            bound.map_or_else(
                || {
                    serialize(&RampShape {
                        active: false,
                        principal_id: "",
                        account: "",
                        audience: &audience,
                        expires_at: 0,
                    })
                },
                |(active, account)| {
                    serialize(&RampShape {
                        active: true,
                        principal_id: &active.principal.sub,
                        account,
                        audience: &audience,
                        expires_at: active.expires_at,
                    })
                },
            )
        }
        Service::Provisioning => refusal(403, "service_not_permitted", None),
    }
}

fn random_hex(bytes: usize) -> Result<Zeroizing<String>, String> {
    let mut random = Zeroizing::new(vec![0_u8; bytes]);
    getrandom::fill(&mut random).map_err(|_| "entropy is unavailable".to_owned())?;
    Ok(Zeroizing::new(hex(&random)))
}

fn create_principal(shared: &Shared, request: &Request) -> Response {
    if request.headers.get("content-type").map(String::as_str) != Some("application/json") {
        return refusal(400, "content_type_required", None);
    }
    let body: PrincipalRequest = match serde_json::from_slice(&request.body) {
        Ok(body) => body,
        Err(_) => return refusal(400, "invalid_argument", None),
    };
    if !valid_sub(&body.sub)
        || !valid_signer_keys(&body.allowed_signer_public_keys)
        || body
            .account
            .as_deref()
            .is_some_and(|account| !valid_identifier(account, 512))
        || body.audiences.len() > MAX_AUDIENCES
        || body
            .audiences
            .iter()
            .any(|audience| !valid_identifier(audience, 128))
        || body
            .audiences
            .iter()
            .enumerate()
            .any(|(index, audience)| body.audiences[..index].contains(audience))
    {
        return refusal(400, "invalid_argument", None);
    }
    let principal = Principal {
        sub: body.sub,
        allowed_signer_public_keys: body.allowed_signer_public_keys,
        account: body.account,
        audiences: body.audiences,
    };
    let Ok(mut store) = shared.store.lock() else {
        return refusal(503, "store_unavailable", Some(5));
    };
    if store.put_principal(principal.clone()).is_err() {
        return refusal(503, "store_unavailable", Some(5));
    }
    serialize(&PrincipalResponse {
        sub: &principal.sub,
        allowed_signer_public_keys: &principal.allowed_signer_public_keys,
        account: principal.account.as_deref(),
        audiences: &principal.audiences,
    })
}

fn create_session(shared: &Shared, request: &Request) -> Response {
    if request.headers.get("content-type").map(String::as_str) != Some("application/json") {
        return refusal(400, "content_type_required", None);
    }
    let body: SessionRequest = match serde_json::from_slice(&request.body) {
        Ok(body) => body,
        Err(_) => return refusal(400, "invalid_argument", None),
    };
    let ttl = body
        .ttl_seconds
        .unwrap_or(shared.config.default_ttl_seconds);
    if !valid_sub(&body.sub) || ttl == 0 || ttl > MAX_SESSION_TTL_SECONDS {
        return refusal(400, "invalid_argument", None);
    }
    let Ok(now) = unix_seconds() else {
        return refusal(503, "clock_unavailable", Some(5));
    };
    let (Ok(session_id), Ok(secret), Ok(csrf_token)) = (
        random_hex(SESSION_ID_BYTES),
        random_hex(SESSION_SECRET_BYTES),
        random_hex(CSRF_BYTES),
    ) else {
        return refusal(503, "entropy_unavailable", Some(5));
    };
    let Ok(csrf_sealed) = shared.config.store_key.seal(csrf_token.as_bytes()) else {
        return refusal(503, "entropy_unavailable", Some(5));
    };
    let session = StoredSession {
        session_id: session_id.to_string(),
        principal: body.sub.clone(),
        token_digest: sha256_hex(secret.as_bytes()),
        csrf_digest: sha256_hex(csrf_token.as_bytes()),
        csrf_sealed,
        issued_at: now,
        expires_at: now.saturating_add(ttl),
        revoked_at: None,
    };
    let Ok(mut store) = shared.store.lock() else {
        return refusal(503, "store_unavailable", Some(5));
    };
    if store.principal(&body.sub).is_none() {
        return refusal(404, "principal_not_found", None);
    }
    let expires_at = session.expires_at;
    if let Err(error) = store.put_session(session) {
        return if error.contains("bound") {
            refusal(429, "session_bound_reached", Some(60))
        } else {
            refusal(503, "store_unavailable", Some(5))
        };
    }
    let token = Zeroizing::new(format!("ses_{}.{}", session_id.as_str(), secret.as_str()));
    serialize(&SessionResponse {
        session_id: &session_id,
        sub: &body.sub,
        token: &token,
        csrf_token: &csrf_token,
        expires_at,
    })
}

fn revoke_session(shared: &Shared, session_id: &str) -> Response {
    if !valid_hex(session_id, SESSION_ID_BYTES) {
        return refusal(404, "session_not_found", None);
    }
    let Ok(now) = unix_seconds() else {
        return refusal(503, "clock_unavailable", Some(5));
    };
    let Ok(mut store) = shared.store.lock() else {
        return refusal(503, "store_unavailable", Some(5));
    };
    match store.revoke_session(session_id, now) {
        Ok(Some(revoked_at)) => serialize(&RevocationResponse {
            session_id,
            revoked: true,
            revoked_at,
        }),
        Ok(None) => refusal(404, "session_not_found", None),
        Err(_) => refusal(503, "store_unavailable", Some(5)),
    }
}

fn readiness(shared: &Shared) -> Response {
    let Ok(store) = shared.store.lock() else {
        return refusal(503, "store_unavailable", Some(5));
    };
    match store.probe_writable() {
        Ok(()) => ok("{\"status\":\"ready\",\"service\":\"identity\"}".to_owned()),
        Err(_) => refusal(503, "store_unavailable", Some(5)),
    }
}

fn route(shared: &Shared, request: &Request) -> Response {
    if request.method == "GET" && request.path == "/livez" {
        return ok("{\"status\":\"live\",\"service\":\"identity\"}".to_owned());
    }
    if request.method == "GET" && request.path == "/readyz" {
        return readiness(shared);
    }
    if request.headers.contains_key("forwarded")
        || request.headers.contains_key("x-forwarded-for")
        || request.headers.contains_key("x-real-ip")
        || request.headers.contains_key("x-layerx-client-ip")
        || request.headers.contains_key("x-layerx-principal")
    {
        return refusal(400, "untrusted_identity_header", None);
    }
    let known_route = matches!(
        (request.method.as_str(), request.path.as_str()),
        (
            "POST",
            "/v1/sessions/introspect" | "/v1/introspect" | "/v1/principals" | "/v1/sessions"
        )
    ) || (request.method == "DELETE"
        && request.path.starts_with("/v1/sessions/"));
    if !known_route {
        return refusal(404, "not_found", None);
    }
    let service = match authenticate_service(&shared.config, request) {
        Ok(service) => service,
        Err(response) => return response,
    };
    match (request.method.as_str(), request.path.as_str()) {
        ("POST", "/v1/sessions/introspect" | "/v1/introspect") => {
            if service == Service::Provisioning {
                return refusal(403, "service_not_permitted", None);
            }
            introspect(shared, service, request)
        }
        ("POST", "/v1/principals") => {
            if service != Service::Provisioning {
                return refusal(403, "service_not_permitted", None);
            }
            create_principal(shared, request)
        }
        ("POST", "/v1/sessions") => {
            if service != Service::Provisioning {
                return refusal(403, "service_not_permitted", None);
            }
            create_session(shared, request)
        }
        ("DELETE", path) => {
            if service != Service::Provisioning {
                return refusal(403, "service_not_permitted", None);
            }
            let session_id = path.strip_prefix("/v1/sessions/").unwrap_or_default();
            revoke_session(shared, session_id)
        }
        _ => refusal(404, "not_found", None),
    }
}

fn ok(body: String) -> Response {
    Response {
        status: 200,
        body,
        retry_after: None,
    }
}

fn refusal(status: u16, code: &str, retry_after: Option<u64>) -> Response {
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

fn write_response(stream: &mut impl Write, response: &Response) -> Result<(), String> {
    let reason = match response.status {
        200 => "OK",
        400 => "Bad Request",
        401 => "Unauthorized",
        403 => "Forbidden",
        404 => "Not Found",
        429 => "Too Many Requests",
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

fn handle_connection(shared: &Arc<Shared>, tcp: TcpStream) -> Result<(), String> {
    tcp.set_read_timeout(Some(IO_TIMEOUT))
        .map_err(|error| error.to_string())?;
    tcp.set_write_timeout(Some(IO_TIMEOUT))
        .map_err(|error| error.to_string())?;
    let connection =
        ServerConnection::new(Arc::clone(&shared.config.tls)).map_err(|e| e.to_string())?;
    let mut stream = StreamOwned::new(connection, tcp);
    let response = parse_client_request(&mut stream).map_or_else(
        |_| refusal(400, "invalid_request", None),
        |request| route(shared, &request),
    );
    write_response(&mut stream, &response)?;
    stream.flush().map_err(|error| error.to_string())?;
    stream.conn.send_close_notify();
    let _ = stream.conn.write_tls(&mut stream.sock);
    Ok(())
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

fn platform_identity(config: Config) -> Result<(), String> {
    let store = Store::open(&config.state_dir)?;
    store.probe_writable()?;
    let listener = TcpListener::bind(config.listen).map_err(|error| error.to_string())?;
    let bound = listener.local_addr().map_err(|error| error.to_string())?;
    let shared = Arc::new(Shared {
        config,
        store: Mutex::new(store),
    });
    eprintln!("layerx-identity listening on {bound} with TLS");
    for connection in listener.incoming() {
        match connection {
            Ok(stream) => {
                let Some(permit) = ConnectionPermit::acquire() else {
                    continue;
                };
                let shared = Arc::clone(&shared);
                thread::spawn(move || {
                    let _permit = permit;
                    if let Err(error) = handle_connection(&shared, stream) {
                        eprintln!("layerx-identity connection failed: {error}");
                    }
                });
            }
            Err(error) => eprintln!("layerx-identity accept failed: {error}"),
        }
    }
    Ok(())
}

fn main() {
    if let Err(error) = config().and_then(platform_identity) {
        eprintln!("layerx-identity: {error}");
        std::process::exit(2);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tokens() -> Vec<ServiceToken> {
        Service::ALL
            .iter()
            .map(|service| ServiceToken {
                service: *service,
                token: Zeroizing::new(format!("{}-token-0123456789abcdef", service.name())),
            })
            .collect()
    }

    #[test]
    fn service_resolution_is_exact() {
        let tokens = tokens();
        assert_eq!(
            resolve_service(&tokens, "gateway-token-0123456789abcdef"),
            Some(Service::Gateway)
        );
        assert_eq!(
            resolve_service(&tokens, "provisioning-token-0123456789abcdef"),
            Some(Service::Provisioning)
        );
        assert_eq!(
            resolve_service(&tokens, "gateway-token-0123456789abcde"),
            None
        );
        assert_eq!(
            resolve_service(&tokens, "gateway-token-0123456789abcdef "),
            None
        );
        assert_eq!(resolve_service(&tokens, ""), None);
    }

    #[test]
    fn session_token_form_is_strict() {
        let id = "0".repeat(32);
        let secret = "f".repeat(64);
        assert_eq!(
            parse_session_token(&format!("ses_{id}.{secret}")),
            Some((id.as_str(), secret.as_str()))
        );
        assert_eq!(parse_session_token(&format!("ses_{id}{secret}")), None);
        assert_eq!(parse_session_token(&format!("tok_{id}.{secret}")), None);
        assert_eq!(
            parse_session_token(&format!("ses_{id}.{}", "F".repeat(64))),
            None
        );
        assert_eq!(
            parse_session_token(&format!("ses_{}.{secret}", "0".repeat(31))),
            None
        );
    }

    #[test]
    fn subject_and_key_validation_matches_the_gateway_rules() {
        assert!(valid_sub("did:key:z6mkabc-1_2.3"));
        assert!(!valid_sub("did:key:Z6MK"));
        assert!(!valid_sub(""));
        assert!(!valid_sub(&"a".repeat(129)));
        assert!(valid_signer_keys(&["ab".repeat(32)]));
        assert!(!valid_signer_keys(&["AB".repeat(32)]));
        assert!(!valid_signer_keys(&["ab".repeat(32), "ab".repeat(32)]));
        assert!(!valid_signer_keys(&vec![
            "ab".repeat(32);
            MAX_SIGNER_KEYS + 1
        ]));
        assert!(valid_signer_keys(&[]));
    }

    #[test]
    fn introspection_shapes_serialize_exactly() {
        let keys = vec!["ab".repeat(32)];
        let gateway = serialize(&GatewayShape {
            active: true,
            sub: "did:key:alpha",
            allowed_signer_public_keys: &keys,
        });
        assert_eq!(
            gateway.body,
            format!(
                "{{\"active\":true,\"sub\":\"did:key:alpha\",\"allowed_signer_public_keys\":[\"{}\"]}}",
                "ab".repeat(32)
            )
        );
        let developer = serialize(&DeveloperShape {
            active: false,
            sub: "",
            csrf_token: "",
        });
        assert_eq!(
            developer.body,
            "{\"active\":false,\"sub\":\"\",\"csrf_token\":\"\"}"
        );
        let subject = serialize(&SubjectShape {
            active: true,
            sub: "did:key:alpha",
        });
        assert_eq!(subject.body, "{\"active\":true,\"sub\":\"did:key:alpha\"}");
        let ramp = serialize(&RampShape {
            active: true,
            principal_id: "alpha",
            account: "agent:did:key:alpha:main",
            audience: "ramp-reference",
            expires_at: 42,
        });
        assert_eq!(
            ramp.body,
            "{\"active\":true,\"principal_id\":\"alpha\",\"account\":\"agent:did:key:alpha:main\",\"audience\":\"ramp-reference\",\"expires_at\":42}"
        );
    }

    #[test]
    fn refusal_bodies_follow_the_hosted_contract() {
        let never = refusal(401, "service_token_required", None);
        assert_eq!(
            never.body,
            "{\"error\":{\"code\":\"service_token_required\",\"retry\":\"never\"}}"
        );
        let after = refusal(503, "store_unavailable", Some(5));
        assert_eq!(
            after.body,
            "{\"error\":{\"code\":\"store_unavailable\",\"retry\":\"after\",\"retry_after_seconds\":5}}"
        );
        let mut bytes = Vec::new();
        write_response(&mut bytes, &after).unwrap_or_else(|error| panic!("write: {error}"));
        let text = String::from_utf8(bytes).unwrap_or_default();
        assert!(text
            .starts_with("HTTP/1.1 503 Service Unavailable\r\nContent-Type: application/json\r\n"));
        assert!(text.contains("\r\nRetry-After: 5\r\n"));
        assert!(text.contains("\r\nCache-Control: no-store\r\n"));
        assert!(text.contains("\r\nConnection: close\r\n\r\n{"));
    }

    #[test]
    fn request_parser_rejects_unbounded_and_ambiguous_messages() {
        let mut good =
            &b"POST /v1/introspect HTTP/1.1\r\nHost: identity\r\nContent-Length: 2\r\n\r\n{}"[..];
        let request =
            parse_client_request(&mut good).unwrap_or_else(|error| panic!("parse: {error}"));
        assert_eq!(request.method, "POST");
        assert_eq!(request.path, "/v1/introspect");
        assert_eq!(request.body, b"{}");
        let mut chunked = &b"POST /v1/introspect HTTP/1.1\r\nHost: identity\r\nTransfer-Encoding: chunked\r\n\r\n"[..];
        assert!(parse_client_request(&mut chunked).is_err());
        let mut duplicate = &b"GET /livez HTTP/1.1\r\nHost: a\r\nHost: b\r\n\r\n"[..];
        assert!(parse_client_request(&mut duplicate).is_err());
        let mut no_host = &b"GET /livez HTTP/1.1\r\n\r\n"[..];
        assert!(parse_client_request(&mut no_host).is_err());
        let mut query = &b"GET /livez?x=1 HTTP/1.1\r\nHost: a\r\n\r\n"[..];
        assert!(parse_client_request(&mut query).is_err());
        let oversized = format!(
            "POST /v1/introspect HTTP/1.1\r\nHost: a\r\nContent-Length: {MAX_REQUEST_BYTES}\r\n\r\n"
        );
        let mut oversized = oversized.as_bytes();
        assert!(parse_client_request(&mut oversized).is_err());
    }
}
