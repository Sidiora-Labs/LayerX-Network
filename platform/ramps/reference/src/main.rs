#![forbid(unsafe_code)]

use std::collections::BTreeMap;
use std::fs;
use std::io::{Read as _, Write as _};
use std::net::{TcpListener, TcpStream};
use std::path::PathBuf;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use layerx_ramp_toolkit::clients::{
    parse_hex32, ActivityConfig, ComplianceClient, Endpoint, IdentityClient, LayerxClient,
    MutualTlsClient, MutualTlsFiles, PaxeerCustodyClient, ProviderCallback, ProviderClient,
    SecretFile,
};
use layerx_ramp_toolkit::engine::{InventoryRebalancer, RampEngine};
use layerx_ramp_toolkit::journal::{Journal, WorkflowStage};
use layerx_ramp_toolkit::{
    platform_ramp_toolkit, CreateOrder, OperatorIdentity, QuoteTerms, RampError, RampOrder,
    EXTERNAL_CUSTODY_LABEL,
};
use layerx_types::payload::{ActivityType, ModuleId, ModuleRegistration, ModuleRegistry};
use layerx_paxeer_client::{
    ChainSignal, EndpointConfig, EndpointSignal, EndpointTransport, FinalityTracker,
    TrackerConfig, TransactionHash,
};
use native_tls::{Identity, TlsAcceptor, TlsStream};
use serde::{Deserialize, Serialize};
use serde_json::json;
use subtle::ConstantTimeEq as _;

const MAX_REQUEST_BYTES: usize = 1024 * 1024;
const MAX_HEADERS: usize = 32 * 1024;
const MAX_CONNECTIONS: usize = 128;

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct Config {
    listen: String,
    journal_path: PathBuf,
    worker_id: String,
    lease_seconds: u64,
    reconcile_seconds: u64,
    operator: OperatorIdentity,
    quotes: Vec<QuoteTerms>,
    server_identity_pkcs12: PathBuf,
    server_identity_password_file: PathBuf,
    client_tls: ClientTls,
    identity: IdentityConfig,
    compliance: ComplianceConfig,
    provider: ProviderConfig,
    layerx: LayerxConfig,
    paxeer: PaxeerConfig,
    provider_callback_public_key: String,
    operator_control_token_file: PathBuf,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct ClientTls {
    ca_pem: PathBuf,
    identity_pkcs12: PathBuf,
    identity_password_file: PathBuf,
    timeout_seconds: u64,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct IdentityConfig {
    endpoint: String,
    service_token_file: PathBuf,
    audience: String,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct ComplianceConfig {
    endpoint: String,
    service_token_file: PathBuf,
    public_key: String,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct ProviderConfig {
    endpoint: String,
    credential_file: PathBuf,
    settlement_path: String,
    status_path: String,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct LayerxConfig {
    gateway_endpoint: String,
    receipt_authority_endpoint: String,
    signer_endpoint: String,
    gateway_key_file: PathBuf,
    authority_token_file: PathBuf,
    signer_token_file: PathBuf,
    actor_did: String,
    protocol_version: u16,
    network_id: u32,
    fee_limit: u128,
    signer_public_key: String,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct PaxeerConfig {
    custody_endpoint: String,
    custody_credential_file: PathBuf,
    broadcast_path: String,
    status_path: String,
    operator_account: String,
    wallet_address: String,
    vault_id: String,
    signer_key_handle: String,
    rpc_endpoints: Vec<String>,
    rpc_trust_anchor_der: PathBuf,
    rpc_chain_id: u64,
    rpc_minimum_agreement: usize,
    required_confirmations: u64,
    poll_cadence_seconds: u64,
    delayed_after_polls: u64,
}

struct State {
    journal: Mutex<Journal>,
    quotes: BTreeMap<String, QuoteTerms>,
    operator: OperatorIdentity,
    identity: IdentityClient,
    compliance: ComplianceClient,
    provider: ProviderClient,
    layerx: LayerxClient,
    paxeer: PaxeerCustodyClient,
    paxeer_tracker_config: TrackerConfig,
    paxeer_trackers: Mutex<BTreeMap<[u8; 32], FinalityTracker>>,
    registry: ModuleRegistry,
    worker_id: String,
    lease_seconds: u64,
    provider_callback_public_key: [u8; 32],
    operator_control_token: String,
}

struct ConnectionGate {
    active: AtomicUsize,
    maximum: usize,
}

impl ConnectionGate {
    const fn new(maximum: usize) -> Self {
        Self {
            active: AtomicUsize::new(0),
            maximum,
        }
    }

    fn try_acquire(self: &Arc<Self>) -> Option<ConnectionPermit> {
        self.active
            .fetch_update(Ordering::AcqRel, Ordering::Acquire, |active| {
                (active < self.maximum).then_some(active + 1)
            })
            .ok()
            .map(|_| ConnectionPermit(Arc::clone(self)))
    }
}

struct ConnectionPermit(Arc<ConnectionGate>);

impl Drop for ConnectionPermit {
    fn drop(&mut self) {
        self.0.active.fetch_sub(1, Ordering::AcqRel);
    }
}

fn main() {
    if let Err(error) = run() {
        eprintln!("layerx-reference-ramp refused startup: {error}");
        std::process::exit(1);
    }
}

fn run() -> Result<(), String> {
    let config_path = std::env::args_os()
        .nth(1)
        .ok_or_else(|| "usage: layerx-reference-ramp CONFIG.json".to_owned())?;
    let config: Config = serde_json::from_slice(
        &fs::read(config_path).map_err(|error| format!("read config: {error}"))?,
    )
    .map_err(|error| format!("parse config: {error}"))?;
    validate_config(&config)?;
    let acceptor = server_acceptor(&config)?;
    let state = Arc::new(build_state(&config)?);
    let reconcile_state = Arc::clone(&state);
    let cadence = Duration::from_secs(config.reconcile_seconds);
    thread::Builder::new()
        .name("ramp-reconciler".to_owned())
        .spawn(move || reconcile_loop(reconcile_state, cadence))
        .map_err(|error| format!("start reconciler: {error}"))?;
    let listener = TcpListener::bind(&config.listen)
        .map_err(|error| format!("bind {}: {error}", config.listen))?;
    let connection_gate = Arc::new(ConnectionGate::new(MAX_CONNECTIONS));
    println!("{}", platform_ramp_toolkit());
    println!("{EXTERNAL_CUSTODY_LABEL}");
    for stream in listener.incoming() {
        let stream = match stream {
            Ok(stream) => stream,
            Err(error) => {
                eprintln!("ramp accept failed: {error}");
                continue;
            }
        };
        let Some(permit) = connection_gate.try_acquire() else {
            eprintln!("ramp connection limit reached");
            continue;
        };
        let state = Arc::clone(&state);
        let acceptor = acceptor.clone();
        if thread::Builder::new()
            .name("ramp-request".to_owned())
            .spawn(move || {
                let _permit = permit;
                serve(stream, &acceptor, &state);
            })
            .is_err()
        {
            eprintln!("ramp request worker unavailable");
        }
    }
    Ok(())
}

fn validate_config(config: &Config) -> Result<(), String> {
    if config.worker_id.is_empty()
        || config.worker_id.len() > 128
        || config.lease_seconds == 0
        || config.reconcile_seconds == 0
        || config.client_tls.timeout_seconds == 0
        || config.lease_seconds
            <= config
                .client_tls
                .timeout_seconds
                .saturating_mul(5)
        || config.quotes.is_empty()
        || config.paxeer.rpc_endpoints.is_empty()
        || config.paxeer.rpc_chain_id == 0
        || config.paxeer.rpc_minimum_agreement < 2
        || config.paxeer.rpc_minimum_agreement > config.paxeer.rpc_endpoints.len()
        || config.paxeer.required_confirmations == 0
        || config.paxeer.poll_cadence_seconds == 0
        || config.paxeer.delayed_after_polls == 0
        || config.paxeer.operator_account != config.operator.account
        || config.layerx.protocol_version != 1
        || config.layerx.network_id == 0
        || config.layerx.fee_limit == 0
        || config.identity.audience.is_empty()
        || !valid_service_path(&config.provider.settlement_path)
        || !valid_service_path(&config.provider.status_path)
        || !valid_service_path(&config.paxeer.broadcast_path)
        || !valid_service_path(&config.paxeer.status_path)
        || !valid_opaque(&config.paxeer.wallet_address)
        || !valid_opaque(&config.paxeer.vault_id)
        || !valid_opaque(&config.paxeer.signer_key_handle)
        || config
            .operator
            .account
            .strip_prefix("agent:")
            .and_then(|account| account.strip_suffix(":main"))
            != Some(config.layerx.actor_did.as_str())
    {
        return Err("invalid worker, timing or quote configuration".to_owned());
    }
    config
        .operator
        .validate()
        .map_err(|_| "operator identity rejected".to_owned())?;
    for quote in &config.quotes {
        quote
            .validate(now())
            .map_err(|_| format!("quote {} rejected", quote.quote_id))?;
    }
    Ok(())
}

fn valid_service_path(path: &str) -> bool {
    path.starts_with('/')
        && path.len() <= 512
        && !path.contains(['?', '#', '\\'])
        && !path.split('/').any(|segment| matches!(segment, "." | ".."))
}

fn valid_opaque(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 512
        && !value
            .bytes()
            .any(|byte| byte.is_ascii_control() || matches!(byte, b'"' | b'\'' | b'\\'))
}

fn server_acceptor(config: &Config) -> Result<TlsAcceptor, String> {
    let identity = SecretFile::new(&config.server_identity_pkcs12)
        .map_err(|_| "invalid server identity file".to_owned())?
        .read()
        .map_err(|_| "server identity unavailable".to_owned())?;
    let password = secret_text(&config.server_identity_password_file)?;
    let identity = Identity::from_pkcs12(&identity, &password)
        .map_err(|_| "server identity rejected".to_owned())?;
    TlsAcceptor::builder(identity)
        .min_protocol_version(Some(native_tls::Protocol::Tlsv12))
        .build()
        .map_err(|_| "server TLS configuration rejected".to_owned())
}

fn build_state(config: &Config) -> Result<State, String> {
    let tls = MutualTlsFiles {
        ca_pem: config.client_tls.ca_pem.clone(),
        identity_pkcs12: SecretFile::new(&config.client_tls.identity_pkcs12)
            .map_err(|_| "invalid client identity".to_owned())?,
        identity_password: SecretFile::new(&config.client_tls.identity_password_file)
            .map_err(|_| "invalid client identity password".to_owned())?,
    };
    let timeout = Duration::from_secs(config.client_tls.timeout_seconds);
    let client = || {
        MutualTlsClient::new(&tls, timeout).map_err(|_| "mTLS client rejected".to_owned())
    };
    let identity = IdentityClient {
        http: client()?,
        endpoint: Endpoint::parse(&config.identity.endpoint)
            .map_err(|_| "identity endpoint rejected".to_owned())?,
        service_token: secret_text(&config.identity.service_token_file)?,
        audience: config.identity.audience.clone(),
    };
    let compliance = ComplianceClient {
        http: client()?,
        endpoint: Endpoint::parse(&config.compliance.endpoint)
            .map_err(|_| "compliance endpoint rejected".to_owned())?,
        service_token: secret_text(&config.compliance.service_token_file)?,
        verifying_key: configured_key(&config.compliance.public_key, "compliance")?,
    };
    let provider = ProviderClient {
        http: client()?,
        endpoint: Endpoint::parse(&config.provider.endpoint)
            .map_err(|_| "provider endpoint rejected".to_owned())?,
        credential: secret_text(&config.provider.credential_file)?,
        settlement_path: config.provider.settlement_path.clone(),
        status_path: config.provider.status_path.clone(),
    };
    let layerx = LayerxClient {
        http: client()?,
        gateway: Endpoint::parse(&config.layerx.gateway_endpoint)
            .map_err(|_| "gateway endpoint rejected".to_owned())?,
        receipt_authority: Endpoint::parse(&config.layerx.receipt_authority_endpoint)
            .map_err(|_| "receipt authority endpoint rejected".to_owned())?,
        signer: Endpoint::parse(&config.layerx.signer_endpoint)
            .map_err(|_| "signer endpoint rejected".to_owned())?,
        gateway_key: secret_text(&config.layerx.gateway_key_file)?,
        authority_token: secret_text(&config.layerx.authority_token_file)?,
        signer_token: secret_text(&config.layerx.signer_token_file)?,
        activity: ActivityConfig {
            actor_did: config.layerx.actor_did.as_bytes().to_vec(),
            protocol_version: config.layerx.protocol_version,
            network_id: config.layerx.network_id,
            fee_limit: config.layerx.fee_limit,
            signer_public_key: configured_key(&config.layerx.signer_public_key, "LayerX signer")?,
        },
    };
    let paxeer = PaxeerCustodyClient {
        http: client()?,
        endpoint: Endpoint::parse(&config.paxeer.custody_endpoint)
            .map_err(|_| "Paxeer custody endpoint rejected".to_owned())?,
        credential: secret_text(&config.paxeer.custody_credential_file)?,
        broadcast_path: config.paxeer.broadcast_path.clone(),
        status_path: config.paxeer.status_path.clone(),
        operator_account: config.paxeer.operator_account.clone(),
        wallet_address: config.paxeer.wallet_address.clone(),
        vault_id: config.paxeer.vault_id.clone(),
        signer_key_handle: config.paxeer.signer_key_handle.clone(),
    };
    let rpc_trust_anchor_der = fs::read(&config.paxeer.rpc_trust_anchor_der)
        .map_err(|_| "Paxeer RPC trust anchor is unavailable".to_owned())?;
    if rpc_trust_anchor_der.is_empty() {
        return Err("Paxeer RPC trust anchor is empty".to_owned());
    }
    let paxeer_tracker_config = TrackerConfig {
        endpoints: config
            .paxeer
            .rpc_endpoints
            .iter()
            .map(|url| EndpointConfig {
                url: url.clone(),
                request_timeout: timeout,
                transport: EndpointTransport::PinnedTls {
                    trust_anchor_der: rpc_trust_anchor_der.clone(),
                },
                expected_chain_id: config.paxeer.rpc_chain_id,
            })
            .collect(),
        minimum_endpoint_agreement: config.paxeer.rpc_minimum_agreement,
        required_confirmations: config.paxeer.required_confirmations,
        poll_cadence: Duration::from_secs(config.paxeer.poll_cadence_seconds),
        delayed_after_polls: config.paxeer.delayed_after_polls,
    };
    let send = ActivityType::new(ModuleId::Asset, 5)
        .map_err(|_| "asset send activity rejected".to_owned())?;
    let receive = ActivityType::new(ModuleId::Asset, 6)
        .map_err(|_| "asset receive activity rejected".to_owned())?;
    let registration = ModuleRegistration::new(ModuleId::Asset, &[send, receive])
        .map_err(|_| "asset registry rejected".to_owned())?;
    let registry = ModuleRegistry::new(&[registration])
        .map_err(|_| "module registry rejected".to_owned())?;
    let mut quotes = BTreeMap::new();
    for quote in &config.quotes {
        if quotes.insert(quote.quote_id.clone(), quote.clone()).is_some() {
            return Err("duplicate quote id".to_owned());
        }
    }
    Ok(State {
        journal: Mutex::new(
            Journal::open(&config.journal_path).map_err(|_| "journal rejected".to_owned())?,
        ),
        quotes,
        operator: config.operator.clone(),
        identity,
        compliance,
        provider,
        layerx,
        paxeer,
        paxeer_tracker_config,
        paxeer_trackers: Mutex::new(BTreeMap::new()),
        registry,
        worker_id: config.worker_id.clone(),
        lease_seconds: config.lease_seconds,
        provider_callback_public_key: configured_key(
            &config.provider_callback_public_key,
            "provider callback",
        )?,
        operator_control_token: secret_text(&config.operator_control_token_file)?,
    })
}

fn configured_key(value: &str, label: &str) -> Result<[u8; 32], String> {
    let key = parse_hex32(value).map_err(|_| format!("{label} key rejected"))?;
    if key == [0; 32] {
        return Err(format!("{label} key rejected"));
    }
    Ok(key)
}

fn secret_text(path: &PathBuf) -> Result<String, String> {
    let bytes = SecretFile::new(path)
        .map_err(|_| format!("secret file {} rejected", path.display()))?
        .read()
        .map_err(|_| format!("secret file {} unavailable", path.display()))?;
    let value = std::str::from_utf8(&bytes)
        .map_err(|_| format!("secret file {} is not text", path.display()))?
        .trim_end_matches(['\r', '\n'])
        .to_owned();
    if value.is_empty() {
        return Err(format!("secret file {} is empty", path.display()));
    }
    if value.len() > 4096 || value.bytes().any(|byte| matches!(byte, 0 | b'\r' | b'\n')) {
        return Err(format!("secret file {} is not a bounded credential", path.display()));
    }
    Ok(value)
}

fn serve(stream: TcpStream, acceptor: &TlsAcceptor, state: &State) {
    let mut tls = match acceptor.accept(stream) {
        Ok(tls) => tls,
        Err(_) => return,
    };
    let response = read_request(&mut tls)
        .and_then(|request| route(state, request))
        .unwrap_or_else(|response| response);
    let _ = tls.write_all(&response.encode());
    let _ = tls.flush();
}

struct Request {
    method: String,
    path: String,
    headers: BTreeMap<String, String>,
    body: Vec<u8>,
}

struct Response {
    status: u16,
    body: Vec<u8>,
}

impl Response {
    fn encode(&self) -> Vec<u8> {
        let reason = match self.status {
            200 => "OK",
            201 => "Created",
            202 => "Accepted",
            400 => "Bad Request",
            401 => "Unauthorized",
            403 => "Forbidden",
            404 => "Not Found",
            409 => "Conflict",
            _ => "Service Unavailable",
        };
        let mut output = format!(
            "HTTP/1.1 {} {}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
            self.status,
            reason,
            self.body.len()
        )
        .into_bytes();
        output.extend_from_slice(&self.body);
        output
    }
}

fn read_request(stream: &mut TlsStream<TcpStream>) -> Result<Request, Response> {
    stream
        .get_ref()
        .set_read_timeout(Some(Duration::from_secs(10)))
        .map_err(|_| error(503, "request_timeout"))?;
    let mut bytes = Vec::new();
    let mut chunk = [0_u8; 4096];
    let boundary;
    loop {
        let read = stream
            .read(&mut chunk)
            .map_err(|_| error(400, "request_read_failed"))?;
        if read == 0 {
            return Err(error(400, "request_incomplete"));
        }
        bytes.extend_from_slice(&chunk[..read]);
        if bytes.len() > MAX_HEADERS {
            return Err(error(400, "headers_too_large"));
        }
        if let Some(found) = bytes.windows(4).position(|window| window == b"\r\n\r\n") {
            boundary = found;
            break;
        }
    }
    let headers = std::str::from_utf8(&bytes[..boundary])
        .map_err(|_| error(400, "headers_invalid"))?;
    let mut lines = headers.split("\r\n");
    let request_line = lines.next().ok_or_else(|| error(400, "request_invalid"))?;
    let mut parts = request_line.split_ascii_whitespace();
    let method = parts.next().ok_or_else(|| error(400, "request_invalid"))?;
    let path = parts.next().ok_or_else(|| error(400, "request_invalid"))?;
    if parts.next() != Some("HTTP/1.1") || parts.next().is_some() || !path.starts_with('/') {
        return Err(error(400, "request_invalid"));
    }
    let mut map = BTreeMap::new();
    for line in lines {
        let (name, value) = line
            .split_once(':')
            .ok_or_else(|| error(400, "headers_invalid"))?;
        let name = name.to_ascii_lowercase();
        if map.insert(name, value.trim().to_owned()).is_some() {
            return Err(error(400, "duplicate_header"));
        }
    }
    let length = map
        .get("content-length")
        .map_or(Ok(0), |value| value.parse::<usize>())
        .map_err(|_| error(400, "content_length_invalid"))?;
    if length > MAX_REQUEST_BYTES {
        return Err(error(400, "body_too_large"));
    }
    let body_offset = boundary.saturating_add(4);
    while bytes.len().saturating_sub(body_offset) < length {
        let read = stream
            .read(&mut chunk)
            .map_err(|_| error(400, "request_read_failed"))?;
        if read == 0 || bytes.len().saturating_add(read) > MAX_HEADERS + MAX_REQUEST_BYTES {
            return Err(error(400, "request_incomplete"));
        }
        bytes.extend_from_slice(&chunk[..read]);
    }
    Ok(Request {
        method: method.to_owned(),
        path: path.to_owned(),
        headers: map,
        body: bytes[body_offset..body_offset + length].to_vec(),
    })
}

fn route(state: &State, request: Request) -> Result<Response, Response> {
    if request.method == "GET" && request.path == "/livez" {
        return Ok(ok(json!({ "live": true })));
    }
    if request.method == "GET" && request.path == "/readyz" {
        let ready = state.journal.lock().is_ok();
        let body = json!({
            "ready": ready,
            "external_custody": true,
            "provider_contract": layerx_ramp_toolkit::PROVIDER_CONTRACT_VERSION,
            "compliance_contract": layerx_ramp_toolkit::COMPLIANCE_CONTRACT_VERSION,
            "paxeer_contract": layerx_ramp_toolkit::PAXEER_CONTRACT_VERSION
        });
        return Ok(if ready {
            ok(body)
        } else {
            json_response(503, body)
        });
    }
    if request.method == "POST" && request.path == "/v1/orders" {
        let principal = authenticate(state, &request)?;
        let create: CreateOrder =
            serde_json::from_slice(&request.body).map_err(|_| error(400, "order_invalid"))?;
        let mut journal = state
            .journal
            .lock()
            .map_err(|_| error(503, "journal_unavailable"))?;
        if let Some(existing) = journal.order_by_id(&create.order_id) {
            if existing.order.customer != principal
                || existing.order.quote.quote_id != create.quote_id
                || existing.order.payer_grant != create.payer_grant
            {
                return Err(error(409, "order_id_conflict"));
            }
            return Ok(created(&existing.presentation()));
        }
        let quote = state
            .quotes
            .get(&create.quote_id)
            .cloned()
            .ok_or_else(|| error(404, "quote_not_found"))?;
        let order = RampOrder::bind(create, quote, principal, state.operator.clone(), now())
            .map_err(map_error)?;
        let snapshot = journal
            .create_order(order, now())
            .map_err(map_error)?;
        return Ok(created(&snapshot.presentation()));
    }
    if request.method == "POST" && request.path == "/v1/provider-callbacks" {
        let callback: ProviderCallback = serde_json::from_slice(&request.body)
            .map_err(|_| error(400, "callback_invalid"))?;
        let mut journal = state
            .journal
            .lock()
            .map_err(|_| error(503, "journal_unavailable"))?;
        let mut engine = engine(state, &mut journal);
        engine
            .provider_callback(&callback, &state.provider_callback_public_key, now())
            .map_err(map_error)?;
        return Ok(ok(json!({ "accepted": true })));
    }
    if let Some(digest) = request.path.strip_prefix("/v1/orders/") {
        if request.method != "GET" || digest.contains('/') {
            return Err(error(404, "not_found"));
        }
        let principal = authenticate(state, &request)?;
        let digest = parse_hex32(digest).map_err(|_| error(400, "order_digest_invalid"))?;
        let journal = state
            .journal
            .lock()
            .map_err(|_| error(503, "journal_unavailable"))?;
        let snapshot = journal.order(&digest).ok_or_else(|| error(404, "order_not_found"))?;
        if snapshot.order.customer != principal {
            return Err(error(404, "order_not_found"));
        }
        return Ok(ok(json!({
            "order_id": snapshot.order.order_id,
            "stage": snapshot.stage,
            "presentation": snapshot.presentation()
        })));
    }
    if request.method == "POST" && request.path == "/internal/v1/work" {
        require_operator(state, &request)?;
        let work: Work =
            serde_json::from_slice(&request.body).map_err(|_| error(400, "work_invalid"))?;
        let mut journal = state
            .journal
            .lock()
            .map_err(|_| error(503, "journal_unavailable"))?;
        let mut engine = engine(state, &mut journal);
        match work.action {
            WorkAction::Compliance => engine.evaluate_compliance(work.order_digest, now()),
            WorkAction::SubmitProvider => engine.submit_provider(work.order_digest, now()),
            WorkAction::ReconcileProvider => engine.reconcile_provider(work.order_digest, now()),
            WorkAction::SubmitLayerx => match work.account_sequence {
                Some(sequence) => engine.submit_layerx(work.order_digest, sequence, now()),
                None => Err(RampError::InvalidOrder),
            },
            WorkAction::ResolveLayerx => engine.resolve_layerx(work.order_digest, now()),
        }
        .map_err(map_error)?;
        let snapshot = journal
            .order(&work.order_digest)
            .ok_or_else(|| error(404, "order_not_found"))?;
        return Ok(ok(json!({
            "stage": snapshot.stage,
            "presentation": snapshot.presentation()
        })));
    }
    if request.method == "POST" && request.path == "/internal/v1/rebalances" {
        require_operator(state, &request)?;
        let action: Rebalance = serde_json::from_slice(&request.body)
            .map_err(|_| error(400, "rebalance_invalid"))?;
        let mut journal = state
            .journal
            .lock()
            .map_err(|_| error(503, "journal_unavailable"))?;
        let mut rebalancer = InventoryRebalancer {
            journal: &mut journal,
            custody: &state.paxeer,
        };
        return match action {
            Rebalance::Submit {
                asset,
                amount,
                idempotency_key,
            } => {
                let (operation_id, transaction) = rebalancer
                    .submit(asset, amount, idempotency_key, now())
                    .map_err(map_error)?;
                Ok(accepted(json!({
                    "operation_id": operation_id,
                    "transaction_hash": format!("0x{}", layerx_ramp_toolkit::clients::hex(&transaction.bytes())),
                    "status": "broadcast_unknown"
                })))
            }
            Rebalance::Poll {
                idempotency_key,
                operation_id,
                transaction_hash,
            } => {
                let transaction = TransactionHash::from_hex(&transaction_hash)
                    .map_err(|_| error(400, "transaction_hash_invalid"))?;
                let mut trackers = state
                    .paxeer_trackers
                    .lock()
                    .map_err(|_| error(503, "paxeer_tracker_unavailable"))?;
                if !trackers.contains_key(&idempotency_key) {
                    let tracker = FinalityTracker::new(
                        state.paxeer_tracker_config.clone(),
                        transaction,
                    )
                    .map_err(|_| error(503, "paxeer_tracker_unavailable"))?;
                    trackers.insert(idempotency_key, tracker);
                }
                let tracker = trackers
                    .get_mut(&idempotency_key)
                    .ok_or_else(|| error(503, "paxeer_tracker_unavailable"))?;
                if tracker.transaction() != transaction {
                    return Err(error(409, "rebalance_transaction_conflict"));
                }
                let report = rebalancer
                    .poll(idempotency_key, &operation_id, tracker, now())
                    .map_err(map_error)?;
                let persisted = journal
                    .paxeer(&idempotency_key)
                    .ok_or_else(|| error(404, "rebalance_not_found"))?;
                Ok(ok(json!({
                    "operation_id": operation_id,
                    "transaction_hash": transaction_hash,
                    "status": persisted.stage,
                    "confirmations": report.progress().confirmed,
                    "required_confirmations": report.progress().required,
                    "chain": chain_signal(&report.signal()),
                    "endpoints": endpoint_signal(&report.endpoint())
                })))
            }
            Rebalance::Reconcile { idempotency_key } => {
                let (operation_id, transaction) = rebalancer
                    .reconcile(idempotency_key, now())
                    .map_err(map_error)?;
                Ok(accepted(json!({
                    "operation_id": operation_id,
                    "transaction_hash": format!("0x{}", layerx_ramp_toolkit::clients::hex(&transaction.bytes())),
                    "status": "broadcast_unknown"
                })))
            }
        };
    }
    if let Some(idempotency) = request.path.strip_prefix("/internal/v1/rebalances/") {
        if request.method != "GET" || idempotency.contains('/') {
            return Err(error(404, "not_found"));
        }
        require_operator(state, &request)?;
        let idempotency =
            parse_hex32(idempotency).map_err(|_| error(400, "idempotency_key_invalid"))?;
        let journal = state
            .journal
            .lock()
            .map_err(|_| error(503, "journal_unavailable"))?;
        let snapshot = journal
            .paxeer(&idempotency)
            .ok_or_else(|| error(404, "rebalance_not_found"))?;
        return Ok(ok(json!({
            "idempotency_key": snapshot.idempotency_key,
            "asset": snapshot.asset,
            "amount": snapshot.amount,
            "operation_id": snapshot.operation_id.as_deref(),
            "transaction_hash": snapshot.transaction_hash.map(|hash| format!("0x{}", layerx_ramp_toolkit::clients::hex(&hash))),
            "status": &snapshot.stage,
            "block_hash": snapshot.block_hash.map(|hash| format!("0x{}", layerx_ramp_toolkit::clients::hex(&hash))),
            "confirmations": snapshot.confirmations
        })));
    }
    Err(error(404, "not_found"))
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Work {
    order_digest: [u8; 32],
    action: WorkAction,
    account_sequence: Option<u64>,
}

#[derive(Deserialize)]
#[serde(rename_all = "snake_case")]
enum WorkAction {
    Compliance,
    SubmitProvider,
    ReconcileProvider,
    SubmitLayerx,
    ResolveLayerx,
}

#[derive(Deserialize)]
#[serde(tag = "action", rename_all = "snake_case", deny_unknown_fields)]
enum Rebalance {
    Submit {
        asset: [u8; 32],
        amount: u128,
        idempotency_key: [u8; 32],
    },
    Reconcile {
        idempotency_key: [u8; 32],
    },
    Poll {
        idempotency_key: [u8; 32],
        operation_id: String,
        transaction_hash: String,
    },
}

fn authenticate(state: &State, request: &Request) -> Result<layerx_ramp_toolkit::AuthenticatedPrincipal, Response> {
    state
        .identity
        .authenticate(
            request
                .headers
                .get("authorization")
                .ok_or_else(|| error(401, "authentication_required"))?,
            now(),
        )
        .map_err(|_| error(401, "authentication_refused"))
}

fn require_operator(state: &State, request: &Request) -> Result<(), Response> {
    let presented = request
        .headers
        .get("authorization")
        .and_then(|value| value.strip_prefix("Bearer "))
        .ok_or_else(|| error(401, "operator_authentication_required"))?;
    if presented
        .as_bytes()
        .ct_eq(state.operator_control_token.as_bytes())
        .unwrap_u8()
        != 1
    {
        return Err(error(403, "operator_authentication_refused"));
    }
    Ok(())
}

fn engine<'a>(state: &'a State, journal: &'a mut Journal) -> RampEngine<'a> {
    RampEngine {
        journal,
        compliance: &state.compliance,
        provider: &state.provider,
        layerx: &state.layerx,
        registry: &state.registry,
        worker_id: &state.worker_id,
        lease_seconds: state.lease_seconds,
    }
}

fn reconcile_loop(state: Arc<State>, cadence: Duration) {
    loop {
        thread::sleep(cadence);
        let mut journal = match state.journal.lock() {
            Ok(journal) => journal,
            Err(_) => continue,
        };
        let due = journal.orders();
        for snapshot in due {
            let digest = snapshot.order.order_digest;
            let observed_at = now();
            if matches!(
                snapshot.stage,
                WorkflowStage::ProviderSubmissionPlanned
                    | WorkflowStage::ProviderSubmittedUnknown
                    | WorkflowStage::ProviderPending
                    | WorkflowStage::LayerxSubmissionPlanned
                    | WorkflowStage::LayerxSubmittedUnknown
                    | WorkflowStage::LayerxPending
            ) && snapshot
                .evidence
                .retry_at
                .is_some_and(|retry_at| retry_at > observed_at)
            {
                continue;
            }
            let mut worker = engine(&state, &mut journal);
            let result = match snapshot.stage {
                WorkflowStage::ProviderSubmissionPlanned
                | WorkflowStage::ProviderSubmittedUnknown
                | WorkflowStage::ProviderPending => {
                    worker.reconcile_provider(digest, observed_at)
                }
                WorkflowStage::LayerxSubmissionPlanned
                | WorkflowStage::LayerxSubmittedUnknown
                | WorkflowStage::LayerxPending => {
                    worker.resolve_layerx(digest, observed_at)
                }
                WorkflowStage::ProviderSettled | WorkflowStage::LayerxVerified => {
                    worker.finish_if_complete(digest, observed_at)
                }
                _ => Ok(()),
            };
            if result.is_err() {
                eprintln!("ramp reconciliation retained unresolved state");
            }
        }
    }
}

fn now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |duration| duration.as_secs())
}

fn chain_signal(signal: &ChainSignal) -> serde_json::Value {
    match signal {
        ChainSignal::Progressing => json!({ "state": "progressing" }),
        ChainSignal::Delayed {
            stalled_polls,
            threshold,
            stalled_for,
            delayed_after,
        } => json!({
            "state": "delayed",
            "stalled_polls": stalled_polls,
            "threshold": threshold,
            "stalled_for_seconds": stalled_for.as_secs(),
            "delayed_after_seconds": delayed_after.as_secs()
        }),
        ChainSignal::Unreachable { .. } => json!({ "state": "unreachable" }),
    }
}

fn endpoint_signal(signal: &EndpointSignal) -> serde_json::Value {
    match signal {
        EndpointSignal::Serving => json!({ "state": "serving" }),
        EndpointSignal::Degraded { failovers } => {
            json!({ "state": "degraded", "failover_count": failovers.len() })
        }
        EndpointSignal::Unreachable { .. } => json!({ "state": "unreachable" }),
    }
}

fn ok(value: impl Serialize) -> Response {
    json_response(200, value)
}

fn created(value: impl Serialize) -> Response {
    json_response(201, value)
}

fn accepted(value: impl Serialize) -> Response {
    json_response(202, value)
}

fn error(status: u16, code: &str) -> Response {
    json_response(status, json!({ "error": code }))
}

fn json_response(status: u16, value: impl Serialize) -> Response {
    match serde_json::to_vec(&value) {
        Ok(body) => Response { status, body },
        Err(_) => Response {
            status: 503,
            body: b"{\"error\":\"encoding_failed\"}".to_vec(),
        },
    }
}

fn map_error(error_value: RampError) -> Response {
    match error_value {
        RampError::InvalidOrder | RampError::InvalidPrincipal | RampError::OrderBinding => {
            error(400, "request_refused")
        }
        RampError::Conflict | RampError::IllegalTransition | RampError::LeaseHeld => {
            error(409, "operation_conflict")
        }
        RampError::PayerGrantRequired | RampError::Intent | RampError::ReceiptMismatch => {
            error(400, "layerx_binding_refused")
        }
        RampError::Compliance => error(503, "compliance_unavailable"),
        RampError::Provider => error(503, "provider_unavailable"),
        RampError::Layerx | RampError::Receipt(_) => error(503, "layerx_unavailable"),
        RampError::Paxeer => error(503, "paxeer_unavailable"),
        RampError::Journal | RampError::Configuration => error(503, "service_unavailable"),
    }
}

#[must_use]
pub const fn platform_reference_ramp() -> &'static str {
    "receipt-backed-reference-market-maker"
}

#[cfg(test)]
mod boundary_tests {
    use std::sync::atomic::Ordering;
    use std::sync::Arc;

    use super::ConnectionGate;

    #[test]
    fn connection_gate_refuses_work_above_the_bound_and_releases_capacity() {
        let gate = Arc::new(ConnectionGate::new(2));
        let Some(first) = gate.try_acquire() else {
            panic!("first permit was refused");
        };
        let Some(second) = gate.try_acquire() else {
            panic!("second permit was refused");
        };
        assert!(gate.try_acquire().is_none());
        drop(first);
        let Some(replacement) = gate.try_acquire() else {
            panic!("released capacity was not reusable");
        };
        assert_eq!(gate.active.load(Ordering::Acquire), 2);
        drop(second);
        drop(replacement);
        assert_eq!(gate.active.load(Ordering::Acquire), 0);
    }
}
