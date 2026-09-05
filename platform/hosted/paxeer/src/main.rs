use rustls::pki_types::{CertificateDer, PrivateKeyDer, PrivatePkcs8KeyDer};
use rustls::{ServerConfig, ServerConnection, StreamOwned};
use serde_json::Value;
use std::collections::BTreeMap;
use std::env;
use std::fs;
use std::io::{Read, Write};
use std::net::{IpAddr, SocketAddr, TcpListener, TcpStream};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use std::thread;
use std::time::Duration;
use zeroize::Zeroize;

const MAX_BODY_BYTES: usize = 2 * 1024 * 1024;
const MAX_HEADER_BYTES: usize = 16 * 1024;
const MAX_REQUEST_BYTES: usize = MAX_BODY_BYTES + MAX_HEADER_BYTES;
const MAX_NODE_RESPONSE_BYTES: usize = 8 * 1024 * 1024;
const MAX_METHOD_BYTES: usize = 64;
const CONNECT_TIMEOUT: Duration = Duration::from_secs(3);
const CLIENT_IO_TIMEOUT: Duration = Duration::from_secs(20);
const NODE_IO_TIMEOUT: Duration = Duration::from_secs(30);
const MAX_CONNECTIONS: usize = 128;
static ACTIVE_CONNECTIONS: AtomicUsize = AtomicUsize::new(0);

const ALLOWED_PREFIXES: [&str; 1] = ["eth_"];
const ALLOWED_METHODS: [&str; 2] = ["net_version", "web3_clientVersion"];
const DENIED_METHODS: [&str; 8] = [
    "eth_accounts",
    "eth_coinbase",
    "eth_sendTransaction",
    "eth_sign",
    "eth_signTransaction",
    "eth_signTypedData",
    "eth_signTypedData_v4",
    "eth_mining",
];

#[derive(Clone)]
struct NodeEndpoint {
    address: SocketAddr,
    path: String,
}

impl NodeEndpoint {
    fn parse(value: &str) -> Result<Self, String> {
        let rest = value
            .strip_prefix("http://")
            .ok_or_else(|| "LAYERX_PAXEER_NODE_URL must use plain http".to_owned())?;
        let (authority, path) = rest.split_once('/').map_or((rest, "/"), |(host, tail)| {
            (host, if tail.is_empty() { "/" } else { tail })
        });
        if authority.is_empty()
            || authority.contains(['@', '?', '#', '\\'])
            || path.contains(['?', '#', '\\'])
        {
            return Err("LAYERX_PAXEER_NODE_URL is not canonical".to_owned());
        }
        let path = if path == "/" {
            "/".to_owned()
        } else {
            format!("/{path}")
        };
        let (host, port) = authority
            .rsplit_once(':')
            .ok_or_else(|| "LAYERX_PAXEER_NODE_URL must name a port".to_owned())?;
        let port = port
            .parse::<u16>()
            .map_err(|_| "LAYERX_PAXEER_NODE_URL port is invalid".to_owned())?;
        let ip = host
            .parse::<IpAddr>()
            .map_err(|_| "LAYERX_PAXEER_NODE_URL must use a literal loopback address".to_owned())?;
        if !ip.is_loopback() || port == 0 {
            return Err("LAYERX_PAXEER_NODE_URL must point at a loopback port".to_owned());
        }
        Ok(Self {
            address: SocketAddr::new(ip, port),
            path,
        })
    }
}

struct Config {
    listen: SocketAddr,
    tls: Arc<ServerConfig>,
    node: NodeEndpoint,
    chain_id: u64,
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
    retry_after: Option<u64>,
}

enum NodeFailure {
    Unreachable,
    Invalid,
}

fn parse_u64(name: &str) -> Result<u64, String> {
    let value = env::var(name).map_err(|_| format!("{name} is required"))?;
    value
        .parse::<u64>()
        .map_err(|_| format!("{name} must be an integer"))
}

fn server_tls_config() -> Result<Arc<ServerConfig>, String> {
    rustls::crypto::ring::default_provider()
        .install_default()
        .map_err(|_| "failed to install TLS crypto provider".to_owned())?;
    let certificate_path = env::var("LAYERX_PAXEER_BOUNDARY_TLS_CERT_DER")
        .map_err(|_| "TLS certificate is required")?;
    let key_path = env::var("LAYERX_PAXEER_BOUNDARY_TLS_KEY_DER")
        .map_err(|_| "TLS private key is required")?;
    let certificate = CertificateDer::from(fs::read(certificate_path).map_err(|e| e.to_string())?);
    let key = PrivateKeyDer::from(PrivatePkcs8KeyDer::from(
        fs::read(key_path).map_err(|e| e.to_string())?,
    ));
    let config = ServerConfig::builder()
        .with_no_client_auth()
        .with_single_cert(vec![certificate], key)
        .map_err(|error| error.to_string())?;
    Ok(Arc::new(config))
}

fn config() -> Result<Config, String> {
    let listen = env::var("LAYERX_PAXEER_BOUNDARY_LISTEN")
        .unwrap_or_else(|_| "0.0.0.0:9443".to_owned())
        .parse::<SocketAddr>()
        .map_err(|_| "LAYERX_PAXEER_BOUNDARY_LISTEN must be a socket address".to_owned())?;
    let chain_id = parse_u64("LAYERX_PAXEER_CHAIN_ID")?;
    if chain_id == 0 {
        return Err("LAYERX_PAXEER_CHAIN_ID must be positive".to_owned());
    }
    Ok(Config {
        listen,
        tls: server_tls_config()?,
        node: NodeEndpoint::parse(
            &env::var("LAYERX_PAXEER_NODE_URL")
                .map_err(|_| "LAYERX_PAXEER_NODE_URL is required")?,
        )?,
        chain_id,
    })
}

fn read_http_message(stream: &mut impl Read, maximum: usize) -> Result<Request, String> {
    let mut bytes = Vec::with_capacity(4096);
    let mut chunk = [0_u8; 8192];
    let header_end = loop {
        let count = stream.read(&mut chunk).map_err(|error| error.to_string())?;
        if count == 0 || bytes.len().saturating_add(count) > maximum {
            return Err("HTTP message is empty or exceeds its bound".to_owned());
        }
        bytes.extend_from_slice(&chunk[..count]);
        if let Some(position) = bytes.windows(4).position(|window| window == b"\r\n\r\n") {
            break position + 4;
        }
        if bytes.len() > MAX_HEADER_BYTES {
            return Err("HTTP headers exceed their bound".to_owned());
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
    if bytes.len() != header_end + content_length {
        return Err("HTTP message carries trailing bytes".to_owned());
    }
    Ok(Request {
        method: String::new(),
        path: String::new(),
        headers,
        body: bytes[header_end..].to_vec(),
    })
}

fn parse_client_request(stream: &mut impl Read) -> Result<Request, String> {
    let mut request = read_http_message(stream, MAX_REQUEST_BYTES)?;
    let start = request
        .headers
        .remove("")
        .ok_or_else(|| "request line is missing".to_owned())?;
    let mut parts = start.split_whitespace();
    request.method = parts
        .next()
        .ok_or_else(|| "request method is missing".to_owned())?
        .to_owned();
    request.path = parts
        .next()
        .ok_or_else(|| "request target is missing".to_owned())?
        .to_owned();
    if parts.next() != Some("HTTP/1.1") || parts.next().is_some() || request.path.contains('?') {
        return Err("request line is invalid".to_owned());
    }
    if !request.headers.contains_key("host") {
        return Err("HTTP/1.1 Host header is required".to_owned());
    }
    Ok(request)
}

fn node_request(node: &NodeEndpoint, body: &[u8]) -> Result<(u16, Vec<u8>), NodeFailure> {
    let mut stream = TcpStream::connect_timeout(&node.address, CONNECT_TIMEOUT)
        .map_err(|_| NodeFailure::Unreachable)?;
    stream
        .set_read_timeout(Some(NODE_IO_TIMEOUT))
        .map_err(|_| NodeFailure::Unreachable)?;
    stream
        .set_write_timeout(Some(NODE_IO_TIMEOUT))
        .map_err(|_| NodeFailure::Unreachable)?;
    write!(
        stream,
        "POST {} HTTP/1.1\r\nHost: {}\r\nContent-Type: application/json\r\nAccept: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
        node.path,
        node.address,
        body.len()
    )
    .map_err(|_| NodeFailure::Unreachable)?;
    stream
        .write_all(body)
        .map_err(|_| NodeFailure::Unreachable)?;
    stream.flush().map_err(|_| NodeFailure::Unreachable)?;
    let mut response =
        read_http_message(&mut stream, MAX_NODE_RESPONSE_BYTES).map_err(|_| NodeFailure::Invalid)?;
    let first = response.headers.get("").ok_or(NodeFailure::Invalid)?;
    let mut parts = first.split_whitespace();
    if parts.next() != Some("HTTP/1.1") {
        return Err(NodeFailure::Invalid);
    }
    let status = parts
        .next()
        .ok_or(NodeFailure::Invalid)?
        .parse::<u16>()
        .map_err(|_| NodeFailure::Invalid)?;
    Ok((status, std::mem::take(&mut response.body)))
}

fn rpc_id_valid(id: &Value) -> bool {
    match id {
        Value::Null | Value::Number(_) => true,
        Value::String(text) => text.len() <= 256,
        Value::Bool(_) | Value::Array(_) | Value::Object(_) => false,
    }
}

fn method_allowed(method: &str) -> bool {
    if method.len() > MAX_METHOD_BYTES
        || !method
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'_')
        || DENIED_METHODS.contains(&method)
    {
        return false;
    }
    ALLOWED_METHODS.contains(&method)
        || ALLOWED_PREFIXES
            .iter()
            .any(|prefix| method.starts_with(prefix))
}

fn rpc_error(id: &Value, code: i64, message: &str) -> Response {
    let body = serde_json::json!({ "jsonrpc": "2.0", "id": id, "error": { "code": code, "message": message } });
    Response {
        status: 200,
        body: body.to_string().into_bytes(),
        retry_after: None,
    }
}

fn relay(config: &Config, request: &Request) -> Response {
    if request.headers.get("content-type").map(String::as_str) != Some("application/json") {
        return refusal(400, "content_type_required", None);
    }
    if request.body.len() > MAX_BODY_BYTES {
        return refusal(413, "body_too_large", None);
    }
    let Ok(document) = serde_json::from_slice::<Value>(&request.body) else {
        return refusal(400, "invalid_json", None);
    };
    let Value::Object(members) = &document else {
        return refusal(400, "batch_not_supported", None);
    };
    let id = members.get("id").cloned().unwrap_or(Value::Null);
    if !rpc_id_valid(&id)
        || members.get("jsonrpc").and_then(Value::as_str) != Some("2.0")
        || !members
            .get("params")
            .is_none_or(|params| params.is_array() || params.is_object())
        || members.keys().any(|key| {
            !matches!(key.as_str(), "jsonrpc" | "id" | "method" | "params")
        })
    {
        return rpc_error(&id, -32600, "invalid request");
    }
    let Some(method) = members.get("method").and_then(Value::as_str) else {
        return rpc_error(&id, -32600, "invalid request");
    };
    if !method_allowed(method) {
        return rpc_error(&id, -32601, "method is not relayed by the boundary");
    }
    match node_request(&config.node, &request.body) {
        Ok((200, body)) => match serde_json::from_slice::<Value>(&body) {
            Ok(Value::Object(reply))
                if reply.get("jsonrpc").and_then(Value::as_str) == Some("2.0")
                    && reply.get("id").unwrap_or(&Value::Null) == &id
                    && (reply.contains_key("result") ^ reply.contains_key("error")) =>
            {
                Response {
                    status: 200,
                    body,
                    retry_after: None,
                }
            }
            _ => refusal(502, "node_response_invalid", Some(5)),
        },
        Ok(_) => refusal(502, "node_response_invalid", Some(5)),
        Err(NodeFailure::Unreachable) => refusal(503, "node_unavailable", Some(5)),
        Err(NodeFailure::Invalid) => refusal(502, "node_response_invalid", Some(5)),
    }
}

fn node_chain_id(config: &Config) -> Result<u64, NodeFailure> {
    let body = b"{\"jsonrpc\":\"2.0\",\"id\":1,\"method\":\"eth_chainId\",\"params\":[]}";
    let (status, payload) = node_request(&config.node, body)?;
    if status != 200 {
        return Err(NodeFailure::Invalid);
    }
    let document = serde_json::from_slice::<Value>(&payload).map_err(|_| NodeFailure::Invalid)?;
    let result = document
        .get("result")
        .and_then(Value::as_str)
        .ok_or(NodeFailure::Invalid)?;
    let digits = result
        .strip_prefix("0x")
        .filter(|digits| !digits.is_empty() && digits.len() <= 16)
        .ok_or(NodeFailure::Invalid)?;
    u64::from_str_radix(digits, 16).map_err(|_| NodeFailure::Invalid)
}

fn readiness(config: &Config) -> Response {
    match node_chain_id(config) {
        Ok(chain_id) if chain_id == config.chain_id => ok(format!(
            "{{\"status\":\"ready\",\"service\":\"paxeer-boundary\",\"chain_id\":{chain_id}}}"
        )),
        Ok(_) => refusal(503, "chain_id_mismatch", Some(10)),
        Err(NodeFailure::Unreachable) => refusal(503, "node_unavailable", Some(5)),
        Err(NodeFailure::Invalid) => refusal(503, "node_response_invalid", Some(5)),
    }
}

fn route(config: &Config, request: &Request) -> Response {
    if request.method == "GET" && request.path == "/livez" {
        return ok("{\"status\":\"live\",\"service\":\"paxeer-boundary\"}".to_owned());
    }
    if request.method == "GET" && request.path == "/readyz" {
        return readiness(config);
    }
    if request.method == "POST" && request.path == "/" {
        return relay(config, request);
    }
    refusal(404, "not_found", None)
}

fn ok(body: String) -> Response {
    Response {
        status: 200,
        body: body.into_bytes(),
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
        body: body.to_string().into_bytes(),
        retry_after,
    }
}

fn write_response(stream: &mut impl Write, response: &Response) -> Result<(), String> {
    let reason = match response.status {
        200 => "OK",
        400 => "Bad Request",
        404 => "Not Found",
        413 => "Content Too Large",
        502 => "Bad Gateway",
        _ => "Service Unavailable",
    };
    let retry = response.retry_after.map_or(String::new(), |seconds| {
        format!("Retry-After: {seconds}\r\n")
    });
    write!(
        stream,
        "HTTP/1.1 {} {reason}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nCache-Control: no-store\r\n{retry}Connection: close\r\n\r\n",
        response.status,
        response.body.len()
    )
    .map_err(|error| error.to_string())?;
    stream
        .write_all(&response.body)
        .map_err(|error| error.to_string())?;
    stream.flush().map_err(|error| error.to_string())
}

fn handle_connection(config: &Arc<Config>, tcp: TcpStream) -> Result<(), String> {
    tcp.set_read_timeout(Some(CLIENT_IO_TIMEOUT))
        .map_err(|error| error.to_string())?;
    tcp.set_write_timeout(Some(CLIENT_IO_TIMEOUT))
        .map_err(|error| error.to_string())?;
    let connection = ServerConnection::new(Arc::clone(&config.tls)).map_err(|e| e.to_string())?;
    let mut stream = StreamOwned::new(connection, tcp);
    let response = parse_client_request(&mut stream).map_or_else(
        |_| refusal(400, "invalid_request", None),
        |request| route(config, &request),
    );
    write_response(&mut stream, &response)?;
    stream.conn.send_close_notify();
    let _ = stream.flush();
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

fn paxeer_boundary(config: Config) -> Result<(), String> {
    let listener = TcpListener::bind(config.listen).map_err(|error| error.to_string())?;
    let config = Arc::new(config);
    eprintln!(
        "layerx-paxeer-boundary listening with TLS for chain {}",
        config.chain_id
    );
    for connection in listener.incoming() {
        match connection {
            Ok(stream) => {
                let Some(permit) = ConnectionPermit::acquire() else {
                    continue;
                };
                let shared = Arc::clone(&config);
                thread::spawn(move || {
                    let _permit = permit;
                    if let Err(error) = handle_connection(&shared, stream) {
                        eprintln!("layerx-paxeer-boundary connection failed: {error}");
                    }
                });
            }
            Err(error) => eprintln!("layerx-paxeer-boundary accept failed: {error}"),
        }
    }
    Ok(())
}

fn main() {
    if let Err(error) = config().and_then(paxeer_boundary) {
        eprintln!("layerx-paxeer-boundary: {error}");
        std::process::exit(2);
    }
}
