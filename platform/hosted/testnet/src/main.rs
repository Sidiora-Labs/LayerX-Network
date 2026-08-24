use layerx_platform_testnet::{
    platform_testnet, PendingRelease, LXP_WIRE_PROTOCOL_VERSION, TESTNET_NETWORK_ID,
};
use native_tls::{Certificate, TlsConnector};
use rustls::pki_types::{CertificateDer, PrivateKeyDer, PrivatePkcs8KeyDer};
use rustls::{ServerConfig, ServerConnection, StreamOwned};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::env;
use std::fs;
use std::io::{Read, Write};
use std::net::{SocketAddr, TcpListener, TcpStream, ToSocketAddrs};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use std::thread;
use std::time::Duration;
use subtle::ConstantTimeEq;
use zeroize::{Zeroize, Zeroizing};

const MAX_MESSAGE: usize = 64 * 1024;
const CONNECT_TIMEOUT: Duration = Duration::from_secs(3);
const IO_TIMEOUT: Duration = Duration::from_secs(8);
const MAX_CONNECTIONS: usize = 128;
static ACTIVE_CONNECTIONS: AtomicUsize = AtomicUsize::new(0);

#[derive(Clone)]
struct Endpoint {
    host: String,
    port: u16,
    path: String,
}

impl Endpoint {
    fn parse(value: &str) -> Result<Self, String> {
        let rest = value
            .strip_prefix("https://")
            .ok_or_else(|| "component endpoint must use HTTPS".to_owned())?;
        let (authority, tail) = rest.split_once('/').unwrap_or((rest, ""));
        if authority.is_empty()
            || authority.contains(['@', '?', '#', '\\'])
            || tail.contains(['?', '#', '\\'])
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
        if host.is_empty() {
            return Err("component endpoint host is missing".to_owned());
        }
        Ok(Self {
            host,
            port,
            path: if tail.is_empty() {
                String::new()
            } else {
                format!("/{tail}")
            },
        })
    }

    fn with_path(&self, path: &str) -> Self {
        Self {
            host: self.host.clone(),
            port: self.port,
            path: format!("{}{path}", self.path),
        }
    }

    fn authority(&self) -> String {
        if self.port == 443 {
            self.host.clone()
        } else {
            format!("{}:{}", self.host, self.port)
        }
    }
}

struct Config {
    public_listen: SocketAddr,
    admin_listen: SocketAddr,
    tls: Arc<ServerConfig>,
    outbound_ca: Certificate,
    package_semver: String,
    pending_package_semver: String,
    pending_wire_version: u16,
    core: Endpoint,
    core_admin: Endpoint,
    gateway: Endpoint,
    paxeer: Endpoint,
    backend_admin_token: Zeroizing<String>,
    inbound_admin_token: Zeroizing<String>,
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
    body: Vec<u8>,
}

#[derive(Serialize)]
struct ComponentStatus {
    name: &'static str,
    state: &'static str,
}

#[derive(Serialize)]
struct PublicStatus<'a> {
    service: &'static str,
    state: &'static str,
    package_semver: &'a str,
    lxp_wire_protocol_version: u16,
    network_id: u32,
    components: Vec<ComponentStatus>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct FundingCommand {
    funding_id: String,
    did: String,
    public_key: String,
    amount: u64,
}

fn read_secret(variable: &str) -> Result<Zeroizing<String>, String> {
    let path = env::var(variable).map_err(|_| format!("{variable} is required"))?;
    let mut secret = fs::read_to_string(path).map_err(|error| error.to_string())?;
    while matches!(secret.as_bytes().last(), Some(b'\n' | b'\r')) {
        secret.pop();
    }
    if secret.is_empty() || secret.len() > 4096 {
        secret.zeroize();
        return Err(format!("{variable} is empty or exceeds its bound"));
    }
    Ok(Zeroizing::new(secret))
}

fn tls_config() -> Result<Arc<ServerConfig>, String> {
    rustls::crypto::ring::default_provider()
        .install_default()
        .map_err(|_| "failed to install TLS crypto provider".to_owned())?;
    let cert = fs::read(
        env::var("LAYERX_TESTNET_TLS_CERT_DER")
            .map_err(|_| "LAYERX_TESTNET_TLS_CERT_DER is required")?,
    )
    .map_err(|error| error.to_string())?;
    let key = fs::read(
        env::var("LAYERX_TESTNET_TLS_KEY_DER")
            .map_err(|_| "LAYERX_TESTNET_TLS_KEY_DER is required")?,
    )
    .map_err(|error| error.to_string())?;
    let config = ServerConfig::builder()
        .with_no_client_auth()
        .with_single_cert(
            vec![CertificateDer::from(cert)],
            PrivateKeyDer::from(PrivatePkcs8KeyDer::from(key)),
        )
        .map_err(|error| error.to_string())?;
    Ok(Arc::new(config))
}

fn config() -> Result<Config, String> {
    let testnet = platform_testnet();
    let pending_package_semver = env::var("LAYERX_PENDING_PACKAGE_SEMVER")
        .map_err(|_| "LAYERX_PENDING_PACKAGE_SEMVER is required")?;
    let pending_wire_version = env::var("LAYERX_PENDING_WIRE_PROTOCOL_VERSION")
        .map_err(|_| "LAYERX_PENDING_WIRE_PROTOCOL_VERSION is required")?
        .parse::<u16>()
        .map_err(|_| "pending wire protocol version is invalid".to_owned())?;
    testnet
        .validate(&PendingRelease {
            package_semver: pending_package_semver.clone(),
            wire_protocol_version: pending_wire_version,
        })
        .map_err(str::to_owned)?;
    Ok(Config {
        public_listen: env::var("LAYERX_TESTNET_PUBLIC_LISTEN")
            .unwrap_or_else(|_| "0.0.0.0:9443".to_owned())
            .parse::<SocketAddr>()
            .map_err(|_| "public listen address is invalid".to_owned())?,
        admin_listen: env::var("LAYERX_TESTNET_ADMIN_LISTEN")
            .unwrap_or_else(|_| "0.0.0.0:9444".to_owned())
            .parse::<SocketAddr>()
            .map_err(|_| "admin listen address is invalid".to_owned())?,
        tls: tls_config()?,
        outbound_ca: Certificate::from_der(
            &fs::read(
                env::var("LAYERX_OUTBOUND_CA_DER")
                    .map_err(|_| "LAYERX_OUTBOUND_CA_DER is required")?,
            )
            .map_err(|error| error.to_string())?,
        )
        .map_err(|error| error.to_string())?,
        package_semver: testnet.package_semver,
        pending_package_semver,
        pending_wire_version,
        core: Endpoint::parse(
            &env::var("LAYERX_TESTNET_CORE_URL")
                .map_err(|_| "LAYERX_TESTNET_CORE_URL is required")?,
        )?,
        core_admin: Endpoint::parse(
            &env::var("LAYERX_TESTNET_CORE_ADMIN_URL")
                .map_err(|_| "LAYERX_TESTNET_CORE_ADMIN_URL is required")?,
        )?,
        gateway: Endpoint::parse(
            &env::var("LAYERX_TESTNET_GATEWAY_URL")
                .map_err(|_| "LAYERX_TESTNET_GATEWAY_URL is required")?,
        )?,
        paxeer: Endpoint::parse(
            &env::var("LAYERX_TESTNET_PAXEER_URL")
                .map_err(|_| "LAYERX_TESTNET_PAXEER_URL is required")?,
        )?,
        backend_admin_token: read_secret("LAYERX_TESTNET_BACKEND_ADMIN_TOKEN_FILE")?,
        inbound_admin_token: read_secret("LAYERX_TESTNET_CONTROL_ADMIN_TOKEN_FILE")?,
    })
}

fn connect(endpoint: &Endpoint) -> Result<TcpStream, String> {
    let mut last = None;
    for address in (endpoint.host.as_str(), endpoint.port)
        .to_socket_addrs()
        .map_err(|error| error.to_string())?
        .take(8)
    {
        match TcpStream::connect_timeout(&address, CONNECT_TIMEOUT) {
            Ok(stream) => {
                stream
                    .set_read_timeout(Some(IO_TIMEOUT))
                    .map_err(|error| error.to_string())?;
                stream
                    .set_write_timeout(Some(IO_TIMEOUT))
                    .map_err(|error| error.to_string())?;
                return Ok(stream);
            }
            Err(error) => last = Some(error),
        }
    }
    Err(last.map_or_else(
        || "component did not resolve".to_owned(),
        |error| error.to_string(),
    ))
}

fn upstream(
    ca: &Certificate,
    endpoint: &Endpoint,
    method: &str,
    bearer: Option<&str>,
    idempotency: Option<&str>,
    body: &[u8],
) -> Result<Response, String> {
    let connector = TlsConnector::builder()
        .add_root_certificate(ca.clone())
        .min_protocol_version(Some(native_tls::Protocol::Tlsv12))
        .build()
        .map_err(|error| error.to_string())?;
    let tcp = connect(endpoint)?;
    let mut stream = connector
        .connect(&endpoint.host, tcp)
        .map_err(|error| error.to_string())?;
    let authorization = bearer.map_or(String::new(), |token| {
        format!("Authorization: Bearer {token}\r\n")
    });
    let idempotency =
        idempotency.map_or(String::new(), |key| format!("Idempotency-Key: {key}\r\n"));
    write!(
        stream,
        "{method} {} HTTP/1.1\r\nHost: {}\r\nAccept: application/json\r\nContent-Type: application/json\r\n{authorization}{idempotency}Content-Length: {}\r\nConnection: close\r\n\r\n",
        endpoint.path,
        endpoint.authority(),
        body.len()
    )
    .map_err(|error| error.to_string())?;
    stream.write_all(body).map_err(|error| error.to_string())?;
    stream.flush().map_err(|error| error.to_string())?;
    let mut message = read_message(&mut stream)?;
    let start = message
        .headers
        .get("")
        .ok_or_else(|| "component response has no status".to_owned())?;
    let mut parts = start.split_whitespace();
    if parts.next() != Some("HTTP/1.1") {
        return Err("component response is not HTTP/1.1".to_owned());
    }
    let status = parts
        .next()
        .ok_or_else(|| "component status is missing".to_owned())?
        .parse::<u16>()
        .map_err(|_| "component status is invalid".to_owned())?;
    Ok(Response {
        status,
        body: std::mem::take(&mut message.body),
    })
}

fn read_message(stream: &mut impl Read) -> Result<Request, String> {
    let mut bytes = Vec::with_capacity(2048);
    let mut chunk = [0_u8; 2048];
    let header_end = loop {
        let count = stream.read(&mut chunk).map_err(|error| error.to_string())?;
        if count == 0 || bytes.len().saturating_add(count) > MAX_MESSAGE {
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
    let start = lines
        .next()
        .ok_or_else(|| "HTTP start line is missing".to_owned())?
        .to_owned();
    let mut headers = BTreeMap::new();
    headers.insert(String::new(), start);
    let mut content_length = 0_usize;
    for line in lines.filter(|line| !line.is_empty()) {
        let (name, value) = line
            .split_once(':')
            .ok_or_else(|| "HTTP header is malformed".to_owned())?;
        let name = name.trim().to_ascii_lowercase();
        if headers.contains_key(&name) || name == "transfer-encoding" {
            return Err("duplicate or transfer-encoded header is rejected".to_owned());
        }
        let value = value.trim().to_owned();
        if name == "content-length" {
            content_length = value
                .parse::<usize>()
                .map_err(|_| "content length is invalid".to_owned())?;
        }
        headers.insert(name, value);
    }
    if header_end.saturating_add(content_length) > MAX_MESSAGE {
        return Err("HTTP body exceeds its bound".to_owned());
    }
    while bytes.len() < header_end + content_length {
        let count = stream.read(&mut chunk).map_err(|error| error.to_string())?;
        if count == 0 || bytes.len().saturating_add(count) > MAX_MESSAGE {
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

fn client_request(stream: &mut impl Read) -> Result<Request, String> {
    let mut request = read_message(stream)?;
    let start = request
        .headers
        .remove("")
        .ok_or_else(|| "request line is missing".to_owned())?;
    let mut parts = start.split_whitespace();
    request.method = parts.next().unwrap_or_default().to_owned();
    request.path = parts.next().unwrap_or_default().to_owned();
    if parts.next() != Some("HTTP/1.1")
        || parts.next().is_some()
        || request.method.is_empty()
        || !request.path.starts_with('/')
        || request.path.contains('?')
    {
        return Err("request line is invalid".to_owned());
    }
    if !request.headers.contains_key("host") {
        return Err("HTTP/1.1 Host header is required".to_owned());
    }
    Ok(request)
}

fn probe(ca: &Certificate, endpoint: &Endpoint) -> bool {
    matches!(
        upstream(ca, &endpoint.with_path("/readyz"), "GET", None, None, &[]),
        Ok(Response { status: 200, .. })
    )
}

fn status(config: &Config) -> PublicStatus<'_> {
    let core = probe(&config.outbound_ca, &config.core);
    let gateway = probe(&config.outbound_ca, &config.gateway);
    let paxeer = probe(&config.outbound_ca, &config.paxeer);
    let release = config.package_semver == config.pending_package_semver
        && config.pending_wire_version == LXP_WIRE_PROTOCOL_VERSION;
    let state = if core && gateway && paxeer && release {
        "ready"
    } else {
        "degraded"
    };
    PublicStatus {
        service: "layerx-hosted-testnet",
        state,
        package_semver: &config.package_semver,
        lxp_wire_protocol_version: LXP_WIRE_PROTOCOL_VERSION,
        network_id: TESTNET_NETWORK_ID,
        components: vec![
            ComponentStatus {
                name: "testnet",
                state: if release { "ready" } else { "degraded" },
            },
            ComponentStatus {
                name: "gateway",
                state: if gateway { "ready" } else { "unavailable" },
            },
            ComponentStatus {
                name: "core",
                state: if core { "ready" } else { "unavailable" },
            },
            ComponentStatus {
                name: "paxeer",
                state: if paxeer { "ready" } else { "unavailable" },
            },
        ],
    }
}

fn public_route(config: &Config, request: &Request) -> Response {
    if request.method != "GET" {
        return json_response(404, serde_json::json!({ "error": { "code": "not_found" } }));
    }
    match request.path.as_str() {
        "/livez" => json_response(200, serde_json::json!({ "status": "live" })),
        "/readyz" => {
            let value = status(config);
            json_response(if value.state == "ready" { 200 } else { 503 }, value)
        }
        "/v1/status" => json_response(200, status(config)),
        "/v1/parameters" => json_response(
            200,
            serde_json::json!({
                "network": "layerx-testnet",
                "network_id": TESTNET_NETWORK_ID,
                "package_semver": config.package_semver,
                "lxp_wire_protocol_version": LXP_WIRE_PROTOCOL_VERSION,
                "reset_schedule": "09:00 UTC on the first Tuesday of every month"
            }),
        ),
        _ => json_response(404, serde_json::json!({ "error": { "code": "not_found" } })),
    }
}

fn admin_authorized(config: &Config, request: &Request) -> bool {
    let Some(token) = request
        .headers
        .get("authorization")
        .and_then(|value| value.strip_prefix("Bearer "))
    else {
        return false;
    };
    token.len() == config.inbound_admin_token.len()
        && token
            .as_bytes()
            .ct_eq(config.inbound_admin_token.as_bytes())
            .unwrap_u8()
            == 1
}

fn valid_key(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 128
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.' | b':'))
}

fn admin_route(config: &Config, request: &Request) -> Response {
    if !admin_authorized(config, request) {
        return json_response(
            401,
            serde_json::json!({ "error": { "code": "unauthorized" } }),
        );
    }
    if request.headers.get("content-type").map(String::as_str) != Some("application/json") {
        return json_response(
            400,
            serde_json::json!({ "error": { "code": "content_type_required" } }),
        );
    }
    let Some(idempotency) = request.headers.get("idempotency-key") else {
        return json_response(
            400,
            serde_json::json!({ "error": { "code": "idempotency_key_required" } }),
        );
    };
    if !valid_key(idempotency) {
        return json_response(
            400,
            serde_json::json!({ "error": { "code": "invalid_idempotency_key" } }),
        );
    }
    let endpoint = match (request.method.as_str(), request.path.as_str()) {
        ("POST", "/admin/v1/testnet/fund") => {
            let Ok(command) = serde_json::from_slice::<FundingCommand>(&request.body) else {
                return json_response(
                    400,
                    serde_json::json!({ "error": { "code": "invalid_argument" } }),
                );
            };
            if !valid_key(&command.funding_id)
                || !command.did.starts_with("did:")
                || command.did.len() > 512
                || command.public_key.len() != 64
                || !command
                    .public_key
                    .bytes()
                    .all(|byte| byte.is_ascii_hexdigit())
                || command.amount == 0
            {
                return json_response(
                    400,
                    serde_json::json!({ "error": { "code": "invalid_argument" } }),
                );
            }
            config.core_admin.with_path("/admin/v1/testnet/fund")
        }
        ("POST", "/admin/v1/testnet/reset") => {
            if request.body != b"{}" {
                return json_response(
                    400,
                    serde_json::json!({ "error": { "code": "invalid_argument" } }),
                );
            }
            config.core_admin.with_path("/admin/v1/testnet/reset")
        }
        _ => return json_response(404, serde_json::json!({ "error": { "code": "not_found" } })),
    };
    match upstream(
        &config.outbound_ca,
        &endpoint,
        "POST",
        Some(config.backend_admin_token.as_str()),
        Some(idempotency),
        &request.body,
    ) {
        Ok(response) if response.status == 200 || response.status == 202 => response,
        Ok(response) if (400..500).contains(&response.status) => response,
        _ => json_response(
            503,
            serde_json::json!({ "error": { "code": "core_unavailable", "retry": "after" } }),
        ),
    }
}

fn json_response(status: u16, value: impl Serialize) -> Response {
    match serde_json::to_vec(&value) {
        Ok(body) => Response { status, body },
        Err(_) => Response {
            status: 500,
            body: b"{\"error\":{\"code\":\"serialization_failure\"}}".to_vec(),
        },
    }
}

fn write_response(stream: &mut impl Write, response: &Response) -> Result<(), String> {
    let reason = match response.status {
        200 => "OK",
        202 => "Accepted",
        400 => "Bad Request",
        401 => "Unauthorized",
        404 => "Not Found",
        _ => "Service Unavailable",
    };
    write!(
        stream,
        "HTTP/1.1 {} {reason}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nCache-Control: no-store\r\nConnection: close\r\n\r\n",
        response.status,
        response.body.len()
    )
    .map_err(|error| error.to_string())?;
    stream
        .write_all(&response.body)
        .map_err(|error| error.to_string())
}

fn serve(listener: TcpListener, config: Arc<Config>, admin: bool) -> Result<(), String> {
    for connection in listener.incoming() {
        match connection {
            Ok(tcp) => {
                let Some(permit) = ConnectionPermit::acquire() else {
                    continue;
                };
                tcp.set_read_timeout(Some(IO_TIMEOUT))
                    .map_err(|e| e.to_string())?;
                tcp.set_write_timeout(Some(IO_TIMEOUT))
                    .map_err(|e| e.to_string())?;
                let shared = Arc::clone(&config);
                thread::spawn(move || {
                    let _permit = permit;
                    let result = (|| -> Result<(), String> {
                        let connection = ServerConnection::new(Arc::clone(&shared.tls))
                            .map_err(|error| error.to_string())?;
                        let mut stream = StreamOwned::new(connection, tcp);
                        let response = client_request(&mut stream).map_or_else(
                            |_| {
                                json_response(
                                    400,
                                    serde_json::json!({ "error": { "code": "invalid_request" } }),
                                )
                            },
                            |request| {
                                if admin {
                                    admin_route(&shared, &request)
                                } else {
                                    public_route(&shared, &request)
                                }
                            },
                        );
                        write_response(&mut stream, &response)
                    })();
                    if let Err(error) = result {
                        eprintln!("layerx-testnet-control connection failed: {error}");
                    }
                });
            }
            Err(error) => eprintln!("layerx-testnet-control accept failed: {error}"),
        }
    }
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

fn run(config: Config) -> Result<(), String> {
    let public = TcpListener::bind(config.public_listen).map_err(|error| error.to_string())?;
    let admin = TcpListener::bind(config.admin_listen).map_err(|error| error.to_string())?;
    let config = Arc::new(config);
    let admin_config = Arc::clone(&config);
    thread::spawn(move || {
        if let Err(error) = serve(admin, admin_config, true) {
            eprintln!("layerx-testnet-control admin listener failed: {error}");
        }
    });
    eprintln!("layerx-testnet-control public and private TLS listeners started");
    serve(public, config, false)
}

fn main() {
    if let Err(error) = config().and_then(run) {
        eprintln!("layerx-testnet-control: {error}");
        std::process::exit(2);
    }
}
