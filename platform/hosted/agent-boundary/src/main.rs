mod artifacts;
mod deployment;
mod head_attestation;
#[cfg(test)]
mod head_attestation_tests;
#[cfg(test)]
mod lifecycle_tests;

use layerx_client::lni::handshake::{perform, Handshake, HandshakeConfig};
use layerx_client::lni::program_read::{
    read_program, ProgramReadContext, ProgramReadError, ProgramReadResult,
};
use layerx_client::lni::schema::{encode_envelope, Capability, Envelope, Version};
use layerx_client::lni::simulate::{simulate, SimulateContext, SimulateError};
use layerx_client::lni::transport::{ConnectionGate, FrameTransport, Limits, TransportError, Uds};
use layerx_client::receipt::{
    lookup_authenticated, AuthenticatedLookup, AuthenticatedLookupContext, ReceiptError,
    ReceiptWaitMode,
};
use layerx_client::submit::{submit_signed, Submission, SubmissionContext, SubmitError};
use layerx_proof::receipt::verify_sequencer_signature;
use layerx_types::payload::{ActivityType, ModuleId, ModuleRegistration, ModuleRegistry};
use layerx_types::program_call::NativeProgramCall;
use layerx_types::program_lifecycle::{
    NativeProgramDeploy, NativeProgramUpgrade, NativeProgramWindDown,
};
use layerx_types::result::{ResultCode, Retriability};
use layerx_wire::activity::{decode_signed, encode_signed, Activity};
use layerx_wire::hash::activity_id;
use layerx_wire::limits::STATE_COMMITMENT_PROTOCOL_VERSION as PROTOCOL_VERSION;
use rustls::pki_types::{CertificateDer, PrivateKeyDer, PrivatePkcs8KeyDer};
use rustls::server::WebPkiClientVerifier;
use rustls::{RootCertStore, ServerConfig, ServerConnection, StreamOwned};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;
use std::env;
use std::fs;
use std::io::{Read, Write};
use std::net::{SocketAddr, TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Condvar, Mutex, PoisonError};
use std::thread;
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use subtle::ConstantTimeEq;
use zeroize::{Zeroize, Zeroizing};

const SERVICE: &str = "agent-boundary";
const MAX_ACTIVITY_BYTES: usize = 1_048_576;
const MAX_REQUEST_BYTES: usize = MAX_ACTIVITY_BYTES + 16 * 1024;
const MAX_RELAY_BYTES: usize = 4 * 1024 * 1024;
const MAX_ARTIFACT_RESPONSE_BYTES: usize = 4 * MAX_ACTIVITY_BYTES + 16 * 1024;
const CONNECT_TIMEOUT: Duration = Duration::from_secs(3);
const IO_TIMEOUT: Duration = Duration::from_secs(15);
const MAX_CONNECTIONS: usize = 128;
const LNI_FRAME_BYTES: usize = 1_212_416;
const LNI_CONNECTIONS: usize = 4;
const ERROR_RESPONSE_TAG: u16 = 25;
const MAX_MODULES: usize = 9;
const MAX_ORDINALS: usize = 64;
const DEFAULT_ORDINALS: u16 = 16;
static ACTIVE_CONNECTIONS: AtomicUsize = AtomicUsize::new(0);

struct NodeEndpoint {
    port: u16,
}

struct Config {
    listen: SocketAddr,
    tls: Arc<ServerConfig>,
    gateway_token: Zeroizing<String>,
    registry_token: Zeroizing<String>,
    webhook_token: Zeroizing<String>,
    lni_socket: PathBuf,
    lni_deadline: Duration,
    node: NodeEndpoint,
    node_token: Zeroizing<String>,
    state_dir: PathBuf,
    protocol_network_id: u32,
    network_name: String,
    registry: ModuleRegistry,
    receipt_wait: Duration,
    gate: ConnectionGate,
    sessions: SessionPool,
    key_locks: Mutex<BTreeMap<String, Arc<Mutex<()>>>>,
}

struct Session {
    transport: Uds,
    handshake: Handshake,
    next_correlation: u64,
}

struct SessionSlot {
    current: Option<Session>,
    previous: Option<Handshake>,
}

struct SessionPool {
    slots: Vec<Mutex<SessionSlot>>,
    state: Mutex<SessionPoolState>,
    changed: Condvar,
}

struct SessionPoolState {
    busy: Vec<bool>,
}

#[derive(Clone, Copy, Eq, PartialEq)]
enum SessionUse {
    Interactive,
    ReceiptWait,
}

struct SessionLease<'a> {
    pool: &'a SessionPool,
    index: usize,
}

impl SessionPool {
    fn new() -> Self {
        Self {
            slots: (0..LNI_CONNECTIONS)
                .map(|_| {
                    Mutex::new(SessionSlot {
                        current: None,
                        previous: None,
                    })
                })
                .collect(),
            state: Mutex::new(SessionPoolState {
                busy: vec![false; LNI_CONNECTIONS],
            }),
            changed: Condvar::new(),
        }
    }

    fn acquire(&self, usage: SessionUse) -> SessionLease<'_> {
        let mut state = self.state.lock().unwrap_or_else(PoisonError::into_inner);
        loop {
            let first = usize::from(usage == SessionUse::ReceiptWait);
            if let Some(index) = (first..state.busy.len()).find(|index| !state.busy[*index]) {
                state.busy[index] = true;
                return SessionLease { pool: self, index };
            }
            state = self
                .changed
                .wait(state)
                .unwrap_or_else(PoisonError::into_inner);
        }
    }
}

impl Drop for SessionLease<'_> {
    fn drop(&mut self) {
        let mut state = self
            .pool
            .state
            .lock()
            .unwrap_or_else(PoisonError::into_inner);
        state.busy[self.index] = false;
        self.pool.changed.notify_all();
    }
}

impl Session {
    fn correlation(&mut self) -> u64 {
        let value = self.next_correlation;
        self.next_correlation = self.next_correlation.wrapping_add(1).max(1);
        value
    }
}

#[derive(Clone, Copy, Eq, PartialEq)]
enum Plane {
    Gateway,
    Registry,
    Webhook,
}

#[derive(Clone, Copy, Eq, PartialEq)]
enum Route {
    Activities,
    ProgramCall,
    ProgramDeploy,
    ProgramUpgrade,
    ProgramWindDown,
}

impl Route {
    const fn name(self) -> &'static str {
        match self {
            Self::Activities => "activities",
            Self::ProgramCall => "programs_call",
            Self::ProgramDeploy => "programs_deploy",
            Self::ProgramUpgrade => "programs_upgrade",
            Self::ProgramWindDown => "programs_wind_down",
        }
    }

    const fn success_state(self) -> &'static str {
        match self {
            Self::Activities => "completed",
            Self::ProgramCall
            | Self::ProgramDeploy
            | Self::ProgramUpgrade
            | Self::ProgramWindDown => "executed",
        }
    }

    const fn program_ordinal(self) -> Option<u16> {
        match self {
            Self::Activities => None,
            Self::ProgramDeploy => Some(1),
            Self::ProgramUpgrade => Some(2),
            Self::ProgramCall => Some(3),
            Self::ProgramWindDown => Some(7),
        }
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ModuleFile {
    modules: Vec<ModuleDeclaration>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ModuleDeclaration {
    module: u16,
    ordinals: Vec<u16>,
}

#[derive(Clone, Serialize, Deserialize, PartialEq, Eq)]
struct Refusal {
    status: u16,
    code: String,
    retry_after: Option<u64>,
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct JournalRecord {
    idempotency_key: String,
    route: String,
    request_digest: String,
    activity_id: String,
    program_id: Option<String>,
    signed_activity: String,
    state: String,
    attempts: u32,
    refusal: Option<Refusal>,
    receipt: Option<String>,
    result_code: Option<i32>,
    #[serde(default)]
    program_execution: Option<artifacts::StoredExecution>,
    #[serde(default)]
    lifecycle_sequencer_key: Option<String>,
    #[serde(default)]
    receipt_sequencer_key: Option<String>,
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

enum LniFailure {
    Unavailable(String),
    Transport(String),
    Undelivered(String),
}

impl LniFailure {
    const fn retires_session(&self) -> bool {
        matches!(self, Self::Transport(_) | Self::Undelivered(_))
    }

    fn response(&self) -> Response {
        match self {
            Self::Unavailable(detail) => {
                eprintln!("{SERVICE}: node unavailable: {detail}");
                refusal(503, "node_unavailable", Some(5))
            }
            Self::Transport(detail) | Self::Undelivered(detail) => {
                eprintln!("{SERVICE}: node transport lost: {detail}");
                refusal(503, "node_transport_lost", Some(5))
            }
        }
    }
}

struct SubmissionTransport<'a> {
    inner: &'a mut Uds,
    send_failure: Option<TransportError>,
}

impl FrameTransport for SubmissionTransport<'_> {
    fn send(&mut self, canonical_envelope: &[u8]) -> Result<(), TransportError> {
        let sent = self.inner.send(canonical_envelope);
        if let Err(error) = sent {
            self.send_failure = Some(error);
        }
        sent
    }

    fn receive(&mut self) -> Result<Vec<u8>, TransportError> {
        self.inner.receive()
    }
}

enum Lookup {
    Absent,
    Present {
        receipt: Vec<u8>,
        result_code: i32,
        module_id: u16,
        sequencer_public_key: [u8; 32],
    },
}

enum SubmitOutcome {
    Acknowledged,
    Refused(Refusal),
}

struct Decoded {
    activity_id: [u8; 32],
    signer_public_key: [u8; 32],
    program_id: Option<[u8; 32]>,
}

fn hex(bytes: &[u8]) -> String {
    const DIGITS: &[u8; 16] = b"0123456789abcdef";
    let mut text = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        text.push(char::from(DIGITS[usize::from(byte >> 4)]));
        text.push(char::from(DIGITS[usize::from(byte & 15)]));
    }
    text
}

fn decode_hex(text: &str, maximum: usize) -> Result<Vec<u8>, String> {
    if !text.len().is_multiple_of(2) || text.len() / 2 > maximum {
        return Err("hex text has an invalid length".to_owned());
    }
    let mut bytes = Vec::with_capacity(text.len() / 2);
    let digits = text.as_bytes();
    for pair in digits.chunks(2) {
        let text = std::str::from_utf8(pair).map_err(|_| "hex text is not ASCII".to_owned())?;
        bytes.push(u8::from_str_radix(text, 16).map_err(|_| "hex digit is invalid".to_owned())?);
    }
    Ok(bytes)
}

fn is_hex32(text: &str) -> bool {
    text.len() == 64 && text.bytes().all(|byte| byte.is_ascii_hexdigit())
}

fn parse_hex32(text: &str) -> Option<[u8; 32]> {
    if !is_hex32(text) {
        return None;
    }
    let bytes = decode_hex(text, 32).ok()?;
    bytes.try_into().ok()
}

fn sha256_hex(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}

fn valid_identifier(value: &str, max: usize) -> bool {
    !value.is_empty()
        && value.len() <= max
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.' | b':'))
}

fn snake_case(name: &str) -> String {
    let mut text = String::with_capacity(name.len() + 8);
    for (index, character) in name.chars().enumerate() {
        if character.is_ascii_uppercase() {
            if index != 0 {
                text.push('_');
            }
            text.push(character.to_ascii_lowercase());
        } else {
            text.push(character);
        }
    }
    text
}

fn read_secret(path_variable: &str) -> Result<Zeroizing<String>, String> {
    let path = env::var(path_variable).map_err(|_| format!("{path_variable} is required"))?;
    let mut value = fs::read_to_string(path).map_err(|error| error.to_string())?;
    while matches!(value.as_bytes().last(), Some(b'\n' | b'\r')) {
        value.pop();
    }
    if value.is_empty() || value.len() > 4096 {
        value.zeroize();
        return Err(format!("{path_variable} does not contain a bounded secret"));
    }
    Ok(Zeroizing::new(value))
}

fn parse_u64(name: &str, default: u64) -> Result<u64, String> {
    env::var(name).map_or(Ok(default), |value| {
        value
            .parse::<u64>()
            .map_err(|_| format!("{name} must be an integer"))
    })
}

fn server_tls_config() -> Result<Arc<ServerConfig>, String> {
    rustls::crypto::ring::default_provider()
        .install_default()
        .map_err(|_| "failed to install TLS crypto provider".to_owned())?;
    let certificate_path = env::var("LAYERX_AGENT_BOUNDARY_TLS_CERT_DER")
        .map_err(|_| "LAYERX_AGENT_BOUNDARY_TLS_CERT_DER is required")?;
    let key_path = env::var("LAYERX_AGENT_BOUNDARY_TLS_KEY_DER")
        .map_err(|_| "LAYERX_AGENT_BOUNDARY_TLS_KEY_DER is required")?;
    let certificate = CertificateDer::from(fs::read(certificate_path).map_err(|e| e.to_string())?);
    let key = PrivateKeyDer::from(PrivatePkcs8KeyDer::from(
        fs::read(key_path).map_err(|e| e.to_string())?,
    ));
    let builder = ServerConfig::builder();
    let config = match env::var("LAYERX_AGENT_BOUNDARY_CLIENT_CA_DER") {
        Ok(client_ca_path) => {
            let client_ca =
                CertificateDer::from(fs::read(client_ca_path).map_err(|e| e.to_string())?);
            let mut roots = RootCertStore::empty();
            roots
                .add(client_ca)
                .map_err(|_| "client CA certificate is invalid".to_owned())?;
            let verifier = WebPkiClientVerifier::builder(roots.into())
                .allow_unauthenticated()
                .build()
                .map_err(|error| error.to_string())?;
            builder
                .with_client_cert_verifier(verifier)
                .with_single_cert(vec![certificate], key)
        }
        Err(_) => builder
            .with_no_client_auth()
            .with_single_cert(vec![certificate], key),
    }
    .map_err(|error| error.to_string())?;
    Ok(Arc::new(config))
}

fn module_registry() -> Result<ModuleRegistry, String> {
    let declarations = match env::var("LAYERX_AGENT_BOUNDARY_MODULE_REGISTRY_FILE") {
        Ok(path) => {
            let file: ModuleFile =
                serde_json::from_slice(&fs::read(path).map_err(|error| error.to_string())?)
                    .map_err(|error| format!("module registry file is invalid: {error}"))?;
            file.modules
        }
        Err(_) => (1..=9)
            .map(|module| ModuleDeclaration {
                module,
                ordinals: (1..=DEFAULT_ORDINALS).collect(),
            })
            .collect(),
    };
    if declarations.is_empty() || declarations.len() > MAX_MODULES {
        return Err("module registry must declare between one and nine modules".to_owned());
    }
    let mut registrations = Vec::with_capacity(declarations.len());
    for declaration in declarations {
        let module = ModuleId::from_u16(declaration.module)
            .map_err(|_| format!("module {} is unknown", declaration.module))?;
        if declaration.ordinals.is_empty() || declaration.ordinals.len() > MAX_ORDINALS {
            return Err(format!(
                "module {} declares no ordinals",
                declaration.module
            ));
        }
        let mut types = Vec::with_capacity(declaration.ordinals.len());
        for ordinal in declaration.ordinals {
            types.push(
                ActivityType::new(module, ordinal)
                    .map_err(|_| format!("ordinal {ordinal} is invalid"))?,
            );
        }
        registrations.push(
            ModuleRegistration::new(module, &types)
                .map_err(|_| format!("module {} registration is invalid", declaration.module))?,
        );
    }
    ModuleRegistry::new(&registrations).map_err(|_| "module registry is invalid".to_owned())
}

fn node_endpoint(value: &str) -> Result<NodeEndpoint, String> {
    let rest = value.strip_prefix("http://127.0.0.1:").ok_or_else(|| {
        "LAYERX_AGENT_BOUNDARY_NODE_URL must be http://127.0.0.1:<port>".to_owned()
    })?;
    let port = rest
        .strip_suffix('/')
        .unwrap_or(rest)
        .parse::<u16>()
        .map_err(|_| "LAYERX_AGENT_BOUNDARY_NODE_URL port is invalid".to_owned())?;
    if port == 0 {
        return Err("LAYERX_AGENT_BOUNDARY_NODE_URL port is invalid".to_owned());
    }
    Ok(NodeEndpoint { port })
}

fn config() -> Result<Config, String> {
    let listen = env::var("LAYERX_AGENT_BOUNDARY_LISTEN")
        .unwrap_or_else(|_| "0.0.0.0:9446".to_owned())
        .parse::<SocketAddr>()
        .map_err(|_| "LAYERX_AGENT_BOUNDARY_LISTEN must be a socket address".to_owned())?;
    let gateway_token = read_secret("LAYERX_AGENT_BOUNDARY_GATEWAY_TOKEN_FILE")?;
    let registry_token = read_secret("LAYERX_AGENT_BOUNDARY_REGISTRY_TOKEN_FILE")?;
    if gateway_token
        .as_bytes()
        .ct_eq(registry_token.as_bytes())
        .unwrap_u8()
        == 1
    {
        return Err("gateway and registry bearer tokens must be distinct".to_owned());
    }
    let webhook_token = read_secret("LAYERX_AGENT_BOUNDARY_WEBHOOK_TOKEN_FILE")?;
    if [gateway_token.as_str(), registry_token.as_str()]
        .iter()
        .any(|token| token.as_bytes().ct_eq(webhook_token.as_bytes()).unwrap_u8() == 1)
    {
        return Err("webhook bearer token must be distinct from gateway and registry".to_owned());
    }
    let lni_socket = PathBuf::from(
        env::var("LAYERX_AGENT_BOUNDARY_LNI_SOCKET")
            .map_err(|_| "LAYERX_AGENT_BOUNDARY_LNI_SOCKET is required")?,
    );
    let lni_deadline_ms = parse_u64("LAYERX_AGENT_BOUNDARY_LNI_DEADLINE_MS", 10_000)?;
    if !(1..=60_000).contains(&lni_deadline_ms) {
        return Err("LAYERX_AGENT_BOUNDARY_LNI_DEADLINE_MS must be within 1..=60000".to_owned());
    }
    let receipt_wait_ms = parse_u64("LAYERX_AGENT_BOUNDARY_RECEIPT_WAIT_MS", 5_000)?;
    if !(1..=60_000).contains(&receipt_wait_ms) {
        return Err("LAYERX_AGENT_BOUNDARY_RECEIPT_WAIT_MS must be within 1..=60000".to_owned());
    }
    let protocol_network_id = env::var("LAYERX_AGENT_BOUNDARY_PROTOCOL_NETWORK_ID")
        .map_err(|_| "LAYERX_AGENT_BOUNDARY_PROTOCOL_NETWORK_ID is required".to_owned())?
        .parse::<u32>()
        .map_err(|_| "LAYERX_AGENT_BOUNDARY_PROTOCOL_NETWORK_ID must be a u32".to_owned())?;
    if protocol_network_id == 0 {
        return Err("LAYERX_AGENT_BOUNDARY_PROTOCOL_NETWORK_ID must be non-zero".to_owned());
    }
    let network_name = env::var("LAYERX_AGENT_BOUNDARY_NETWORK_ID")
        .map_err(|_| "LAYERX_AGENT_BOUNDARY_NETWORK_ID is required".to_owned())?;
    if !valid_identifier(&network_name, 64) {
        return Err("LAYERX_AGENT_BOUNDARY_NETWORK_ID is not a canonical identifier".to_owned());
    }
    let state_dir = PathBuf::from(
        env::var("LAYERX_AGENT_BOUNDARY_STATE_DIR")
            .map_err(|_| "LAYERX_AGENT_BOUNDARY_STATE_DIR is required")?,
    );
    fs::create_dir_all(state_dir.join("journal")).map_err(|error| error.to_string())?;
    fs::create_dir_all(state_dir.join("activities")).map_err(|error| error.to_string())?;
    Ok(Config {
        listen,
        tls: server_tls_config()?,
        gateway_token,
        registry_token,
        webhook_token,
        lni_socket,
        lni_deadline: Duration::from_millis(lni_deadline_ms),
        node: node_endpoint(
            &env::var("LAYERX_AGENT_BOUNDARY_NODE_URL")
                .map_err(|_| "LAYERX_AGENT_BOUNDARY_NODE_URL is required")?,
        )?,
        node_token: read_secret("LAYERX_AGENT_BOUNDARY_NODE_BEARER_TOKEN_FILE")?,
        state_dir,
        protocol_network_id,
        network_name,
        registry: module_registry()?,
        receipt_wait: Duration::from_millis(receipt_wait_ms),
        gate: ConnectionGate::new(LNI_CONNECTIONS),
        sessions: SessionPool::new(),
        key_locks: Mutex::new(BTreeMap::new()),
    })
}

fn lni_limits(config: &Config, slot: usize) -> Limits {
    Limits {
        maximum_frame_bytes: LNI_FRAME_BYTES,
        maximum_connections: LNI_CONNECTIONS,
        maximum_streams: 1,
        maximum_queued_bytes: LNI_FRAME_BYTES,
        deadline: if slot == 0 {
            config.lni_deadline
        } else {
            config.lni_deadline.min(config.receipt_wait)
        },
    }
}

fn open_session(
    config: &Config,
    slot: usize,
    previous: Option<&Handshake>,
) -> Result<Session, LniFailure> {
    let mut transport = Uds::connect(&config.lni_socket, &config.gate, lni_limits(config, slot))
        .map_err(|error| LniFailure::Unavailable(format!("{error:?}")))?;
    let expected = HandshakeConfig {
        built_interface_version: Version::V1_7,
        expected_protocol_version: PROTOCOL_VERSION,
        expected_network_id: config.protocol_network_id,
    };
    let handshake = perform(&mut transport, &expected, previous)
        .map_err(|error| LniFailure::Unavailable(format!("{error:?}")))?;
    for capability in [
        Capability::NodeInfo,
        Capability::Submit,
        Capability::ReceiptLookup,
    ] {
        if !handshake.capabilities().contains(capability) {
            return Err(LniFailure::Unavailable(format!(
                "node does not advertise {capability:?}"
            )));
        }
    }
    let seed = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|elapsed| u64::try_from(elapsed.as_nanos()).unwrap_or(u64::MAX))
        .unwrap_or(1)
        .max(1);
    Ok(Session {
        transport,
        handshake,
        next_correlation: seed,
    })
}

fn with_session<T>(
    config: &Config,
    operation: impl FnOnce(&mut Session) -> Result<T, LniFailure>,
) -> Result<T, LniFailure> {
    with_session_for(config, SessionUse::Interactive, operation)
}

fn with_session_for<T>(
    config: &Config,
    usage: SessionUse,
    operation: impl FnOnce(&mut Session) -> Result<T, LniFailure>,
) -> Result<T, LniFailure> {
    let lease = config.sessions.acquire(usage);
    let mut slot = config.sessions.slots[lease.index]
        .lock()
        .unwrap_or_else(PoisonError::into_inner);
    if slot.current.is_none() {
        let opened = open_session(config, lease.index, slot.previous.as_ref())?;
        slot.previous = Some(opened.handshake.clone());
        slot.current = Some(opened);
    }
    let Some(session) = slot.current.as_mut() else {
        return Err(LniFailure::Unavailable("session is absent".to_owned()));
    };
    let result = operation(session);
    if result.as_ref().is_err_and(LniFailure::retires_session) {
        if let Some(retired) = slot.current.take() {
            slot.previous = Some(retired.handshake);
        }
    }
    result
}

fn lookup_receipt(
    session: &mut Session,
    activity: [u8; 32],
    wait_mode: ReceiptWaitMode,
) -> Result<Lookup, LniFailure> {
    let sequencer_public_key = session.handshake.node().authorised_sequencer_key;
    let context = AuthenticatedLookupContext {
        interface_version: session.handshake.node().interface_version,
        correlation_id: session.correlation(),
        sequencer_public_key,
        wait_mode,
    };
    match lookup_authenticated(&mut session.transport, activity, context) {
        Ok(AuthenticatedLookup::Absent | AuthenticatedLookup::TimedOut) => Ok(Lookup::Absent),
        Ok(AuthenticatedLookup::Verified(receipt)) => Ok(Lookup::Present {
            receipt: receipt.canonical_bytes().to_vec(),
            result_code: receipt.result_code().raw(),
            module_id: receipt.module_id(),
            sequencer_public_key,
        }),
        Err(
            error @ (ReceiptError::CoreRefusal { .. }
            | ReceiptError::UnavailableCapability
            | ReceiptError::InterfaceVersion(_)),
        ) => Err(LniFailure::Unavailable(format!(
            "receipt lookup refused: {error:?}"
        ))),
        Err(error) => Err(LniFailure::Transport(format!(
            "receipt lookup failed: {error:?}"
        ))),
    }
}

fn result_refusal(result: ResultCode) -> Refusal {
    let code = result.known().map_or_else(
        || format!("protocol_result_{}", result.raw().unsigned_abs()),
        |known| snake_case(&format!("{known:?}")),
    );
    match result.retriability() {
        Retriability::Retriable => Refusal {
            status: 409,
            code,
            retry_after: Some(5),
        },
        Retriability::Terminal => Refusal {
            status: 422,
            code,
            retry_after: None,
        },
    }
}

fn terminal(status: u16, code: &str) -> Refusal {
    Refusal {
        status,
        code: code.to_owned(),
        retry_after: None,
    }
}

fn withdrawal_admission(
    config: &Config,
    session: &mut Session,
    signed: &[u8],
) -> Result<Option<Refusal>, LniFailure> {
    use layerx_client::payments::SnapshotContext;
    use layerx_client::withdrawal::WithdrawalConfigurationError;

    let Ok(activity) = decode_signed(signed, &config.registry) else {
        return Ok(Some(terminal(400, "malformed_activity")));
    };
    if activity.activity_type().module() != ModuleId::Asset
        || activity.activity_type().ordinal() != 9
    {
        return Ok(None);
    }
    let correlation_id = session.next_correlation;
    session.next_correlation = correlation_id
        .checked_add(3)
        .ok_or_else(|| LniFailure::Unavailable("withdrawal correlation exhausted".to_owned()))?;
    let context = SnapshotContext {
        interface_version: session.handshake.node().interface_version,
        correlation_id,
        minimum_sequence: session.handshake.node().chain_head_sequence,
    };
    match layerx_client::withdrawal::registry(
        &mut session.transport,
        &activity,
        context,
        config.protocol_network_id,
    ) {
        Ok(_) => Ok(None),
        Err(WithdrawalConfigurationError::Unsupported) => {
            Ok(Some(terminal(422, "asset_ordinal_reserved")))
        }
        Err(WithdrawalConfigurationError::InvalidActivity) => {
            Ok(Some(terminal(400, "invalid_asset_activity")))
        }
        Err(WithdrawalConfigurationError::AssetUnavailable) => {
            Ok(Some(terminal(422, "withdrawal_asset_unavailable")))
        }
        Err(WithdrawalConfigurationError::Unavailable) => Err(LniFailure::Unavailable(
            "committed withdrawal configuration is unavailable".to_owned(),
        )),
    }
}

fn submit_activity(
    config: &Config,
    session: &mut Session,
    decoded: &Decoded,
    attempt: u32,
    signed: &[u8],
) -> Result<SubmitOutcome, LniFailure> {
    if let Some(refusal) = withdrawal_admission(config, session, signed)? {
        return Ok(SubmitOutcome::Refused(refusal));
    }
    let node = session.handshake.node();
    let context = SubmissionContext {
        interface_version: node.interface_version,
        protocol_version: node.protocol_version,
        network_id: node.network_id,
        correlation_id: session.correlation(),
        signer_public_key: decoded.signer_public_key,
        attempt,
    };
    let mut transport = SubmissionTransport {
        inner: &mut session.transport,
        send_failure: None,
    };
    let submission = submit_signed(&mut transport, &config.registry, context, signed);
    let send_failure = transport.send_failure;
    match submission {
        Ok(Submission::Acknowledged(acknowledgement)) => {
            if acknowledgement.activity_id() != decoded.activity_id {
                return Err(LniFailure::Transport(
                    "acknowledgement names a different activity".to_owned(),
                ));
            }
            Ok(SubmitOutcome::Acknowledged)
        }
        Ok(Submission::Unknown(_)) => Err(send_failure.map_or_else(
            || LniFailure::Transport("submission outcome is indeterminate".to_owned()),
            |error| {
                LniFailure::Undelivered(format!(
                    "submission frame was not written to the node: {error:?}"
                ))
            },
        )),
        Err(SubmitError::CoreRefusal { result, .. }) => {
            Ok(SubmitOutcome::Refused(result_refusal(result)))
        }
        Err(SubmitError::Wire(_) | SubmitError::Envelope(_)) => {
            Ok(SubmitOutcome::Refused(terminal(400, "malformed_activity")))
        }
        Err(SubmitError::SignatureLength(_) | SubmitError::Signature(_)) => {
            Ok(SubmitOutcome::Refused(terminal(422, "bad_signature")))
        }
        Err(SubmitError::ProtocolVersion { .. }) => {
            Ok(SubmitOutcome::Refused(terminal(422, "version_unsupported")))
        }
        Err(SubmitError::Network { .. }) => {
            Ok(SubmitOutcome::Refused(terminal(422, "wrong_network")))
        }
        Err(SubmitError::UnavailableCapability) => Err(LniFailure::Unavailable(
            "node does not accept submissions".to_owned(),
        )),
        Err(SubmitError::Disconnected) => Err(LniFailure::Transport(
            "node disconnected before the submission".to_owned(),
        )),
    }
}

fn journal_path(config: &Config, key_digest: &str) -> PathBuf {
    config
        .state_dir
        .join("journal")
        .join(format!("{key_digest}.json"))
}

fn activity_index_path(config: &Config, activity: &str) -> PathBuf {
    config.state_dir.join("activities").join(activity)
}

fn write_atomic(path: &Path, bytes: &[u8]) -> Result<(), String> {
    let parent = path
        .parent()
        .ok_or_else(|| "journal path has no parent".to_owned())?;
    let name = path
        .file_name()
        .and_then(|name| name.to_str())
        .ok_or_else(|| "journal path has no name".to_owned())?;
    let temporary = parent.join(format!("{name}.tmp"));
    {
        let mut file = fs::File::create(&temporary).map_err(|error| error.to_string())?;
        file.write_all(bytes).map_err(|error| error.to_string())?;
        file.sync_all().map_err(|error| error.to_string())?;
    }
    fs::rename(&temporary, path).map_err(|error| error.to_string())?;
    fs::File::open(parent)
        .and_then(|directory| directory.sync_all())
        .map_err(|error| error.to_string())
}

fn load_record(config: &Config, key_digest: &str) -> Result<Option<JournalRecord>, String> {
    let path = journal_path(config, key_digest);
    match fs::read(&path) {
        Ok(bytes) => serde_json::from_slice(&bytes)
            .map(Some)
            .map_err(|error| format!("journal record is invalid: {error}")),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(error.to_string()),
    }
}

fn store_record(config: &Config, key_digest: &str, record: &JournalRecord) -> Result<(), String> {
    let bytes = serde_json::to_vec(record).map_err(|error| error.to_string())?;
    write_atomic(&journal_path(config, key_digest), &bytes)?;
    write_atomic(
        &activity_index_path(config, &record.activity_id),
        key_digest.as_bytes(),
    )
}

fn key_lock(config: &Config, key_digest: &str) -> Arc<Mutex<()>> {
    let mut locks = config
        .key_locks
        .lock()
        .unwrap_or_else(PoisonError::into_inner);
    locks.retain(|_, lock| Arc::strong_count(lock) > 1);
    Arc::clone(locks.entry(key_digest.to_owned()).or_default())
}

fn decode_activity(config: &Config, route: Route, body: &[u8]) -> Result<Decoded, Response> {
    let activity: Activity = decode_signed(body, &config.registry)
        .map_err(|_| refusal(400, "malformed_activity", None))?;
    if encode_signed(&activity).map_or(true, |canonical| canonical != body) {
        return Err(refusal(400, "non_canonical_activity", None));
    }
    if activity.protocol_version() != PROTOCOL_VERSION {
        return Err(refusal(422, "version_unsupported", None));
    }
    if activity.network_id() != config.protocol_network_id {
        return Err(refusal(422, "wrong_network", None));
    }
    let signer_public_key: [u8; 32] = activity
        .authority()
        .try_into()
        .map_err(|_| refusal(422, "unsupported_authority", None))?;
    let activity_id =
        activity_id(&activity).map_err(|_| refusal(400, "malformed_activity", None))?;
    let is_program_call = activity.activity_type().module() == ModuleId::Programs
        && activity.activity_type().ordinal() == 3;
    if let Some(ordinal) = route.program_ordinal() {
        if activity.activity_type().module() != ModuleId::Programs
            || activity.activity_type().ordinal() != ordinal
        {
            return Err(refusal(
                400,
                if route == Route::ProgramCall {
                    "not_program_call"
                } else {
                    "wrong_program_operation"
                },
                None,
            ));
        }
    }
    if activity.activity_type().module() == ModuleId::Programs {
        validate_program_lifecycle(activity.activity_type().ordinal(), activity.payload())?;
    }
    let program_id = if is_program_call {
        let call = NativeProgramCall::decode(activity.payload())
            .map_err(|_| refusal(400, "malformed_program_call", None))?;
        Some(call.callee().bytes())
    } else {
        None
    };
    Ok(Decoded {
        activity_id,
        signer_public_key,
        program_id,
    })
}

fn validate_program_lifecycle(ordinal: u16, payload: &[u8]) -> Result<(), Response> {
    let (wasm, expected_hash) = match ordinal {
        1 => {
            let deploy = NativeProgramDeploy::decode(payload)
                .map_err(|_| refusal(400, "malformed_program_deploy", None))?;
            (deploy.wasm, deploy.new_hash)
        }
        2 => {
            let upgrade = NativeProgramUpgrade::decode(payload)
                .map_err(|_| refusal(400, "malformed_program_upgrade", None))?;
            (upgrade.wasm, upgrade.new_hash)
        }
        7 => {
            return NativeProgramWindDown::decode(payload)
                .map(|_| ())
                .map_err(|_| refusal(400, "malformed_program_wind_down", None))
        }
        _ => return Ok(()),
    };
    if !wasm.starts_with(b"\0asm\x01\0\0\0") {
        return Err(refusal(400, "malformed_program_wasm", None));
    }
    let digest: [u8; 32] = Sha256::digest(wasm).into();
    if digest != expected_hash {
        return Err(refusal(400, "program_payload_hash_mismatch", None));
    }
    Ok(())
}

struct ReceiptMetadata {
    result_code: i32,
    state_version: u64,
    state_root: [u8; 32],
}

fn verified_receipt_metadata(record: &JournalRecord) -> Result<ReceiptMetadata, String> {
    let key = record
        .receipt_sequencer_key
        .as_deref()
        .or(record.lifecycle_sequencer_key.as_deref())
        .or_else(|| {
            record
                .program_execution
                .as_ref()
                .map(|execution| execution.sequencer_public_key.as_str())
        })
        .and_then(parse_hex32)
        .ok_or_else(|| "missing receipt sequencer key".to_owned())?;
    let receipt = artifacts::canonical_hex(
        record.receipt.as_deref().unwrap_or_default(),
        MAX_ACTIVITY_BYTES,
    )?;
    let verified =
        verify_sequencer_signature(&receipt, key).map_err(|error| format!("{error:?}"))?;
    let protocol = verified
        .protocol()
        .ok_or_else(|| "missing protocol receipt".to_owned())?;
    if hex(&protocol.activity_id()) != record.activity_id
        || Some(protocol.result_code()) != record.result_code
        || protocol.protocol_version() != PROTOCOL_VERSION
    {
        return Err("receipt journal binding mismatch".to_owned());
    }
    Ok(ReceiptMetadata {
        result_code: protocol.result_code(),
        state_version: protocol.global_sequence(),
        state_root: protocol.resulting_state_root(),
    })
}

fn outcome_response(record: &JournalRecord) -> Response {
    let metadata = match verified_receipt_metadata(record) {
        Ok(metadata) => metadata,
        Err(detail) => {
            eprintln!("{SERVICE}: {detail}");
            return refusal(503, "receipt_invalid", Some(5));
        }
    };
    let receipt = record.receipt.clone().unwrap_or_default();
    let (terminal_payload, call_graph) =
        record
            .program_execution
            .as_ref()
            .map_or(("", ""), |execution| {
                (
                    execution.terminal_payload.as_str(),
                    execution.call_graph.as_str(),
                )
            });
    let route = match record.route.as_str() {
        "programs_call" => Route::ProgramCall,
        "programs_deploy" => Route::ProgramDeploy,
        "programs_upgrade" => Route::ProgramUpgrade,
        "programs_wind_down" => Route::ProgramWindDown,
        _ => Route::Activities,
    };
    let state = if record.result_code == Some(0) {
        route.success_state()
    } else {
        "refused"
    };
    ok(serde_json::json!({
        "result": {
            "state": state,
            "activity_id": record.activity_id,
            "receipt": receipt,
            "terminal_payload": terminal_payload,
            "call_graph": call_graph,
            "result_code": metadata.result_code,
            "state_version": metadata.state_version.to_string(),
            "state_root": hex(&metadata.state_root),
        }
    })
    .to_string())
}

fn refusal_response(stored: &Refusal) -> Response {
    refusal(stored.status, &stored.code, stored.retry_after)
}

fn unknown_response(activity: &str) -> Response {
    Response {
        status: 202,
        body: format!(
            "{{\"state\":\"unknown\",\"activity_id\":\"{activity}\",\"retry\":\"after\",\"retry_after_seconds\":2}}"
        ),
        retry_after: Some(2),
    }
}

fn fetch_program_execution(
    config: &Config,
    receipt: &[u8],
    activity_id: [u8; 32],
    program_id: [u8; 32],
    signed_activity: &[u8],
    sequencer_key: [u8; 32],
) -> Result<artifacts::StoredExecution, Response> {
    let invalid = || refusal(503, "program_artifacts_invalid", Some(5));
    let (batch_id, digest) = artifacts::locator(receipt).map_err(|_| invalid())?;
    let evidence = relay_route(
        config,
        &format!(
            "/v1/batches/{}/receipt-authority?receipt_digest={}",
            hex(&batch_id),
            hex(&digest)
        ),
    );
    if evidence.status != 200 {
        return Err(refusal(503, "program_artifacts_unavailable", Some(5)));
    }
    let authority: artifacts::AuthorityDocument =
        serde_json::from_str(&evidence.body).map_err(|_| invalid())?;
    if authority.sequencer_public_key != hex(&sequencer_key) {
        return Err(invalid());
    }
    let decoded = layerx_wire::receipt::decode(receipt).map_err(|_| invalid())?;
    let protocol = decoded.protocol().ok_or_else(invalid)?;
    let (terminal_payload, call_graph) =
        if protocol.result_code() < 0 && protocol.program_outcome().is_none() {
            (String::new(), String::new())
        } else {
            let answer = relay_bounded(
                config,
                &format!(
                    "/v1/programs/activities/{}/artifacts?receipt_digest={}",
                    hex(&activity_id),
                    hex(&digest)
                ),
                MAX_ARTIFACT_RESPONSE_BYTES,
            );
            if answer.status != 200 {
                return Err(refusal(503, "program_artifacts_unavailable", Some(5)));
            }
            let document =
                artifacts::document(&answer.body, activity_id, digest).map_err(|_| invalid())?;
            (document.terminal_payload, document.call_graph)
        };
    let stored = artifacts::StoredExecution {
        version: 1,
        sequencer_public_key: hex(&sequencer_key),
        evidence: authority.batch_evidence,
        terminal_payload,
        call_graph,
    };
    let activity = decode_signed(signed_activity, &config.registry).map_err(|_| invalid())?;
    let call = NativeProgramCall::decode(activity.payload()).map_err(|_| invalid())?;
    if layerx_wire::hash::activity_id(&activity).map_err(|_| invalid())? != activity_id
        || call.callee().bytes() != program_id
    {
        return Err(invalid());
    }
    let payload_hash = layerx_wire::hash::payload_hash(&activity).map_err(|_| invalid())?;
    artifacts::verify(
        &stored,
        receipt,
        artifacts::ExpectedCall {
            activity_id,
            program_id,
            payload_hash,
            guest_abi_version: call.guest_abi,
            actor_did: activity.actor_did(),
        },
        config.protocol_network_id,
    )
    .map_err(|detail| {
        eprintln!("{SERVICE}: {detail}");
        invalid()
    })?;
    Ok(stored)
}

fn verify_lifecycle_record(config: &Config, record: &JournalRecord) -> Result<(), String> {
    let key = record
        .lifecycle_sequencer_key
        .as_deref()
        .and_then(parse_hex32)
        .ok_or_else(|| "missing lifecycle sequencer key".to_owned())?;
    let signed = artifacts::canonical_hex(&record.signed_activity, MAX_ACTIVITY_BYTES)?;
    let activity =
        decode_signed(&signed, &config.registry).map_err(|error| format!("{error:?}"))?;
    let id = activity_id(&activity).map_err(|error| format!("{error:?}"))?;
    let receipt_bytes = artifacts::canonical_hex(
        record.receipt.as_deref().unwrap_or_default(),
        MAX_ACTIVITY_BYTES,
    )?;
    let receipt =
        verify_sequencer_signature(&receipt_bytes, key).map_err(|error| format!("{error:?}"))?;
    let protocol = receipt
        .protocol()
        .ok_or_else(|| "missing protocol receipt".to_owned())?;
    if activity.protocol_version() != PROTOCOL_VERSION
        || activity.network_id() != config.protocol_network_id
        || activity.activity_type().module() != ModuleId::Programs
        || !matches!(activity.activity_type().ordinal(), 1 | 2 | 7)
        || record.request_digest != sha256_hex(&signed)
        || record.activity_id != hex(&id)
        || protocol.activity_id() != id
        || protocol.protocol_version() != PROTOCOL_VERSION
        || protocol.module_id() != 9
        || protocol.module_version() != 4
        || protocol.operation() != 0
        || protocol.program_outcome().is_some()
        || Some(protocol.result_code()) != record.result_code
    {
        return Err("lifecycle receipt binding mismatch".into());
    }
    Ok(())
}

fn completed_response(config: &Config, record: &JournalRecord) -> Response {
    if matches!(
        record.route.as_str(),
        "programs_deploy" | "programs_upgrade" | "programs_wind_down"
    ) && verify_lifecycle_record(config, record).is_err()
    {
        return refusal(503, "lifecycle_receipt_invalid", Some(5));
    }
    if let Some(program_id) = &record.program_id {
        let checked = (|| {
            let stored = record
                .program_execution
                .as_ref()
                .ok_or_else(|| "missing artifacts".to_owned())?;
            let receipt = artifacts::canonical_hex(
                record.receipt.as_deref().unwrap_or_default(),
                MAX_ACTIVITY_BYTES,
            )?;
            let activity_id =
                parse_hex32(&record.activity_id).ok_or_else(|| "invalid activity".to_owned())?;
            let program_id = parse_hex32(program_id).ok_or_else(|| "invalid program".to_owned())?;
            let signed = artifacts::canonical_hex(&record.signed_activity, MAX_ACTIVITY_BYTES)?;
            let activity =
                decode_signed(&signed, &config.registry).map_err(|error| format!("{error:?}"))?;
            let actual_id =
                layerx_wire::hash::activity_id(&activity).map_err(|error| format!("{error:?}"))?;
            let call = NativeProgramCall::decode(activity.payload())
                .map_err(|error| format!("{error:?}"))?;
            if actual_id != activity_id || call.callee().bytes() != program_id {
                return Err("journal program identity mismatch".into());
            }
            let decoded =
                layerx_wire::receipt::decode(&receipt).map_err(|error| format!("{error:?}"))?;
            if decoded
                .protocol()
                .map(layerx_wire::receipt::ProtocolReceipt::result_code)
                != record.result_code
            {
                return Err("journal result mismatch".to_owned());
            }
            artifacts::verify(
                stored,
                &receipt,
                artifacts::ExpectedCall {
                    activity_id,
                    program_id,
                    payload_hash: layerx_wire::hash::payload_hash(&activity)
                        .map_err(|error| format!("{error:?}"))?,
                    guest_abi_version: call.guest_abi,
                    actor_did: activity.actor_did(),
                },
                config.protocol_network_id,
            )
        })();
        if let Err(detail) = checked {
            eprintln!("{SERVICE}: {detail}");
            return refusal(503, "program_artifacts_invalid", Some(5));
        }
    }
    outcome_response(record)
}

fn complete_record(
    config: &Config,
    key_digest: &str,
    record: &mut JournalRecord,
    receipt: &[u8],
    result_code: i32,
    sequencer_public_key: [u8; 32],
) -> Response {
    if let Some(program_text) = record.program_id.as_deref() {
        let Some(program_id) = parse_hex32(program_text) else {
            return refusal(503, "persistence_invalid", Some(5));
        };
        let Some(activity_id) = parse_hex32(&record.activity_id) else {
            return refusal(503, "persistence_invalid", Some(5));
        };
        let Ok(signed_activity) =
            artifacts::canonical_hex(&record.signed_activity, MAX_ACTIVITY_BYTES)
        else {
            return refusal(503, "persistence_invalid", Some(5));
        };
        match fetch_program_execution(
            config,
            receipt,
            activity_id,
            program_id,
            &signed_activity,
            sequencer_public_key,
        ) {
            Ok(execution) => record.program_execution = Some(execution),
            Err(response) => return response,
        }
    }
    "completed".clone_into(&mut record.state);
    record.receipt = Some(hex(receipt));
    record.result_code = Some(result_code);
    record.receipt_sequencer_key = Some(hex(&sequencer_public_key));
    if matches!(
        record.route.as_str(),
        "programs_deploy" | "programs_upgrade" | "programs_wind_down"
    ) {
        record.lifecycle_sequencer_key = Some(hex(&sequencer_public_key));
        if verify_lifecycle_record(config, record).is_err() {
            return refusal(503, "lifecycle_receipt_invalid", Some(5));
        }
    }
    if store_record(config, key_digest, record).is_err() {
        return refusal(503, "persistence_unavailable", Some(5));
    }
    outcome_response(record)
}

fn await_receipt(
    config: &Config,
    activity: [u8; 32],
    wait: Duration,
) -> Result<Lookup, LniFailure> {
    let (usage, wait_mode) = if wait.is_zero() {
        (SessionUse::Interactive, ReceiptWaitMode::Immediate)
    } else {
        (SessionUse::ReceiptWait, ReceiptWaitMode::Durable)
    };
    with_session_for(config, usage, |session| {
        lookup_receipt(session, activity, wait_mode)
    })
}

fn resolve_record(
    config: &Config,
    key_digest: &str,
    record: &mut JournalRecord,
    decoded: &Decoded,
    signed: &[u8],
) -> Response {
    if record.attempts > 0 {
        let wait = if record.state == "acknowledged" {
            config.receipt_wait
        } else {
            Duration::ZERO
        };
        match await_receipt(config, decoded.activity_id, wait) {
            Ok(Lookup::Present {
                receipt,
                result_code,
                sequencer_public_key,
                ..
            }) => {
                return complete_record(
                    config,
                    key_digest,
                    record,
                    &receipt,
                    result_code,
                    sequencer_public_key,
                )
            }
            Ok(Lookup::Absent) => return unknown_response(&record.activity_id),
            Err(failure) => return failure.response(),
        }
    }
    "submitting".clone_into(&mut record.state);
    record.attempts = record.attempts.saturating_add(1);
    let attempt = record.attempts;
    if store_record(config, key_digest, record).is_err() {
        return refusal(503, "persistence_unavailable", Some(5));
    }
    let outcome = with_session(config, |session| {
        submit_activity(config, session, decoded, attempt, signed)
    });
    match outcome {
        Ok(SubmitOutcome::Acknowledged) => {
            "acknowledged".clone_into(&mut record.state);
            if store_record(config, key_digest, record).is_err() {
                return refusal(503, "persistence_unavailable", Some(5));
            }
            match await_receipt(config, decoded.activity_id, config.receipt_wait) {
                Ok(Lookup::Present {
                    receipt,
                    result_code,
                    sequencer_public_key,
                    ..
                }) => complete_record(
                    config,
                    key_digest,
                    record,
                    &receipt,
                    result_code,
                    sequencer_public_key,
                ),
                Ok(Lookup::Absent) | Err(_) => unknown_response(&record.activity_id),
            }
        }
        Ok(SubmitOutcome::Refused(stored)) => {
            "refused".clone_into(&mut record.state);
            record.refusal = Some(stored.clone());
            if store_record(config, key_digest, record).is_err() {
                return refusal(503, "persistence_unavailable", Some(5));
            }
            refusal_response(&stored)
        }
        Err(LniFailure::Transport(_)) => unknown_response(&record.activity_id),
        Err(failure @ (LniFailure::Unavailable(_) | LniFailure::Undelivered(_))) => {
            failure.response()
        }
    }
}

fn simulate_route(config: &Config, request: &Request) -> Response {
    if request.headers.get("content-type").map(String::as_str) != Some("application/octet-stream") {
        return refusal(400, "content_type_required", None);
    }
    if request.body.is_empty() || request.body.len() > MAX_ACTIVITY_BYTES {
        return refusal(400, "invalid_activity_length", None);
    }
    let decoded = match decode_activity(config, Route::ProgramCall, &request.body) {
        Ok(decoded) => decoded,
        Err(response) => return response,
    };
    let Some(program_id) = decoded.program_id else {
        return refusal(400, "not_program_call", None);
    };
    let outcome = with_session(config, |session| {
        if !session
            .handshake
            .capabilities()
            .contains(Capability::Simulate)
        {
            return Ok(Err(refusal(503, "capability_unavailable", Some(60))));
        }
        let context = SimulateContext {
            interface_version: session.handshake.node().interface_version,
            sequencer_public_key: session.handshake.node().authorised_sequencer_key,
            correlation_id: session.correlation(),
        };
        match simulate(
            &mut session.transport,
            &config.registry,
            &request.body,
            context,
        ) {
            Ok(simulation) => Ok(Ok(simulation)),
            Err(SimulateError::Transport(error)) => {
                Err(LniFailure::Transport(format!("{error:?}")))
            }
            Err(SimulateError::CoreRefusal { class, result }) => {
                if class == 3 {
                    return Ok(Err(refusal(503, "capability_unavailable", Some(60))));
                }
                Ok(Err(refusal_response(&result_refusal(result))))
            }
            Err(SimulateError::MalformedRequest) => {
                Ok(Err(refusal(400, "malformed_activity", None)))
            }
            Err(SimulateError::UnavailableCapability | SimulateError::InterfaceVersion(_)) => {
                Ok(Err(refusal(503, "capability_unavailable", Some(60))))
            }
            Err(error) => Err(LniFailure::Transport(format!("{error:?}"))),
        }
    });
    let simulation = match outcome {
        Ok(Ok(simulation)) => simulation,
        Ok(Err(response)) => return response,
        Err(failure) => return failure.response(),
    };
    if simulation.execution.activity_id != decoded.activity_id {
        return LniFailure::Transport("simulation names a different activity".to_owned())
            .response();
    }
    let Ok(receipt) = verify_sequencer_signature(
        &simulation.execution.receipt,
        simulation.evidence.public_key,
    ) else {
        return LniFailure::Transport("simulated receipt is not sequencer-signed".to_owned())
            .response();
    };
    let Some(protocol) = receipt.protocol() else {
        return LniFailure::Transport("simulated receipt is not a protocol receipt".to_owned())
            .response();
    };
    let state = if protocol.result_code() == 0 {
        "simulated"
    } else {
        "refused"
    };
    simulation_response(&simulation, &program_id, protocol.result_code(), state)
}

fn simulation_response(
    simulation: &layerx_client::lni::simulate::Simulation,
    program_id: &[u8; 32],
    result_code: i32,
    state: &str,
) -> Response {
    let document = serde_json::json!({
        "result": {
            "committed": false,
            "execution": {
                "state": state,
                "activity_id": hex(&simulation.execution.activity_id),
                "program_id": hex(program_id),
                "result_code": result_code,
                "receipt": hex(&simulation.execution.receipt),
                "terminal_payload": hex(&simulation.execution.terminal_payload),
                "call_graph": hex(&simulation.execution.call_graph),
            },
            "simulation_evidence": {
                "boundary_id": hex(&simulation.evidence.boundary_id),
                "activity_id": hex(&simulation.evidence.activity_id),
                "previous_state_root": hex(&simulation.evidence.previous_state_root),
                "hypothetical_state_root": hex(&simulation.evidence.hypothetical_state_root),
                "observed_sequence": simulation.evidence.observed_sequence.to_string(),
                "observed_at": simulation.evidence.observed_at.to_string(),
                "committed": false,
                "public_key": hex(&simulation.evidence.public_key),
                "signature": hex(&simulation.evidence.signature),
            }
        }
    });
    ok(document.to_string())
}

struct ProgramReadFreshness {
    minimum_sequence: u64,
    expected_state_root: Option<[u8; 32]>,
}

fn program_read_freshness(request: &Request) -> Result<ProgramReadFreshness, Response> {
    let minimum_sequence =
        request
            .headers
            .get("layerx-minimum-sequence")
            .map_or(Ok(0), |value| {
                if value.is_empty()
                    || (value.len() > 1 && value.starts_with('0'))
                    || !value.bytes().all(|byte| byte.is_ascii_digit())
                {
                    return Err(refusal(400, "invalid_minimum_sequence", None));
                }
                value
                    .parse::<u64>()
                    .map_err(|_| refusal(400, "invalid_minimum_sequence", None))
            })?;
    let expected_state_root = request
        .headers
        .get("layerx-expected-state-root")
        .map(|value| {
            if value.bytes().any(|byte| byte.is_ascii_uppercase()) {
                return Err(refusal(400, "invalid_expected_state_root", None));
            }
            parse_hex32(value)
                .filter(|root| *root != [0; 32])
                .ok_or_else(|| refusal(400, "invalid_expected_state_root", None))
        })
        .transpose()?;
    Ok(ProgramReadFreshness {
        minimum_sequence,
        expected_state_root,
    })
}

fn program_read_response(
    result: &ProgramReadResult,
    program_id: &[u8; 32],
    result_code: i32,
) -> Response {
    let state = if result_code == 0 { "read" } else { "refused" };
    let document = serde_json::json!({
        "result": {
            "committed": false,
            "read_only": true,
            "execution": {
                "state": state,
                "activity_id": hex(&result.execution.activity_id),
                "program_id": hex(program_id),
                "result_code": result_code,
                "receipt": hex(&result.execution.receipt),
                "receipt_kind": "hypothetical",
                "terminal_payload": hex(&result.execution.terminal_payload),
                "call_graph": hex(&result.execution.call_graph),
            },
            "simulation_evidence": {
                "boundary_id": hex(&result.evidence.boundary_id),
                "activity_id": hex(&result.evidence.activity_id),
                "previous_state_root": hex(&result.evidence.previous_state_root),
                "hypothetical_state_root": hex(&result.evidence.hypothetical_state_root),
                "observed_sequence": result.evidence.observed_sequence.to_string(),
                "observed_at": result.evidence.observed_at.to_string(),
                "committed": false,
                "public_key": hex(&result.evidence.public_key),
                "signature": hex(&result.evidence.signature),
            },
            "snapshot": {
                "minimum_sequence": result.snapshot.minimum_sequence.to_string(),
                "observed_sequence": result.snapshot.observed_sequence.to_string(),
                "state_root": hex(&result.snapshot.state_root),
                "verification": "sequencer_signed_snapshot",
            }
        }
    });
    ok(document.to_string())
}

fn program_read_route(config: &Config, request: &Request) -> Response {
    if request.headers.get("content-type").map(String::as_str) != Some("application/octet-stream") {
        return refusal(400, "content_type_required", None);
    }
    if request.body.is_empty() || request.body.len() > MAX_ACTIVITY_BYTES {
        return refusal(400, "invalid_activity_length", None);
    }
    let freshness = match program_read_freshness(request) {
        Ok(freshness) => freshness,
        Err(response) => return response,
    };
    let decoded = match decode_activity(config, Route::ProgramCall, &request.body) {
        Ok(decoded) => decoded,
        Err(response) => return response,
    };
    let Some(program_id) = decoded.program_id else {
        return refusal(400, "not_program_call", None);
    };
    let outcome = with_session(config, |session| {
        if !session
            .handshake
            .capabilities()
            .contains(Capability::ProgramRead)
        {
            return Ok(Err(refusal(503, "capability_unavailable", Some(60))));
        }
        let context = ProgramReadContext {
            interface_version: session.handshake.node().interface_version,
            sequencer_public_key: session.handshake.node().authorised_sequencer_key,
            correlation_id: session.correlation(),
            minimum_sequence: freshness.minimum_sequence,
            expected_state_root: freshness.expected_state_root,
        };
        match read_program(
            &mut session.transport,
            &config.registry,
            &request.body,
            context,
        ) {
            Ok(result) => Ok(Ok(result)),
            Err(ProgramReadError::SnapshotStale) => {
                Ok(Err(refusal(409, "snapshot_stale", Some(1))))
            }
            Err(ProgramReadError::SnapshotMismatch) => {
                Ok(Err(refusal(409, "snapshot_mismatch", None)))
            }
            Err(ProgramReadError::CoreRefusal { class, result }) => {
                if class == 3 {
                    Ok(Err(refusal(503, "capability_unavailable", Some(60))))
                } else {
                    Ok(Err(refusal_response(&result_refusal(result))))
                }
            }
            Err(ProgramReadError::CanonicalActivity | ProgramReadError::MalformedRequest) => {
                Ok(Err(refusal(400, "malformed_activity", None)))
            }
            Err(
                ProgramReadError::UnavailableCapability | ProgramReadError::InterfaceVersion(_),
            ) => Ok(Err(refusal(503, "capability_unavailable", Some(60)))),
            Err(error) => Err(LniFailure::Transport(format!(
                "program read failed: {error:?}"
            ))),
        }
    });
    let result = match outcome {
        Ok(Ok(result)) => result,
        Ok(Err(response)) => return response,
        Err(failure) => return failure.response(),
    };
    if result.execution.activity_id != decoded.activity_id {
        return LniFailure::Transport("program read names a different activity".to_owned())
            .response();
    }
    let Ok(receipt) =
        verify_sequencer_signature(&result.execution.receipt, result.evidence.public_key)
    else {
        return LniFailure::Transport("program read receipt is not sequencer-signed".to_owned())
            .response();
    };
    let Some(protocol) = receipt.protocol() else {
        return LniFailure::Transport("program read receipt is not a protocol receipt".to_owned())
            .response();
    };
    program_read_response(&result, &program_id, protocol.result_code())
}

fn submit_route(config: &Config, request: &Request, route: Route) -> Response {
    if request.headers.get("content-type").map(String::as_str) != Some("application/octet-stream") {
        return refusal(400, "content_type_required", None);
    }
    if request.body.is_empty() || request.body.len() > MAX_ACTIVITY_BYTES {
        return refusal(400, "invalid_activity_length", None);
    }
    let Some(idempotency) = request.headers.get("idempotency-key") else {
        return refusal(400, "idempotency_key_required", None);
    };
    if !valid_identifier(idempotency, 128) {
        return refusal(400, "invalid_idempotency_key", None);
    }
    let decoded = match decode_activity(config, route, &request.body) {
        Ok(decoded) => decoded,
        Err(response) => return response,
    };
    let key_digest = sha256_hex(idempotency.as_bytes());
    let request_digest = sha256_hex(&request.body);
    let lock = key_lock(config, &key_digest);
    let _guard = lock.lock().unwrap_or_else(PoisonError::into_inner);
    let Ok(existing) = load_record(config, &key_digest) else {
        return refusal(503, "persistence_unavailable", Some(5));
    };
    let mut record = match existing {
        Some(mut record) => {
            if record
                .request_digest
                .as_bytes()
                .ct_eq(request_digest.as_bytes())
                .unwrap_u8()
                != 1
                || record.route != route.name()
            {
                return refusal(409, "idempotency_conflict", None);
            }
            if record.activity_id != hex(&decoded.activity_id)
                || record.signed_activity != hex(&request.body)
            {
                return refusal(503, "persistence_invalid", Some(5));
            }
            record.program_id = decoded.program_id.map(|id| hex(&id));
            match record.state.as_str() {
                "completed"
                    if record.program_id.is_none() || record.program_execution.is_some() =>
                {
                    return completed_response(config, &record);
                }
                "completed" if record.attempts == 0 => {
                    return refusal(503, "persistence_invalid", Some(5))
                }
                "refused" => {
                    return record.refusal.as_ref().map_or_else(
                        || refusal(503, "persistence_invalid", Some(5)),
                        refusal_response,
                    )
                }
                _ => record,
            }
        }
        None => JournalRecord {
            idempotency_key: idempotency.clone(),
            route: route.name().to_owned(),
            request_digest,
            activity_id: hex(&decoded.activity_id),
            program_id: decoded.program_id.map(|id| hex(&id)),
            signed_activity: hex(&request.body),
            state: "submitting".to_owned(),
            attempts: 0,
            refusal: None,
            receipt: None,
            result_code: None,
            program_execution: None,
            lifecycle_sequencer_key: None,
            receipt_sequencer_key: None,
        },
    };
    resolve_record(config, &key_digest, &mut record, &decoded, &request.body)
}

fn receipt_route(config: &Config, activity_text: &str, plane: Plane) -> Response {
    let Some(activity) = parse_hex32(activity_text) else {
        return refusal(400, "invalid_activity_id", None);
    };
    match with_session(config, |session| {
        lookup_receipt(session, activity, ReceiptWaitMode::Immediate)
    }) {
        Ok(Lookup::Present { receipt, .. }) => {
            let body = serde_json::json!({"activity_id": hex(&activity), "receipt": hex(&receipt)});
            if plane == Plane::Gateway {
                ok(serde_json::json!({"result": body}).to_string())
            } else {
                ok(body.to_string())
            }
        }
        Ok(Lookup::Absent) => refusal(404, "receipt_not_found", None),
        Err(failure) => failure.response(),
    }
}

fn idempotency_binding(config: &Config, key: &str) -> Result<(String, Option<String>), Response> {
    let Ok(record) = load_record(config, &sha256_hex(key.as_bytes())) else {
        return Err(refusal(503, "persistence_invalid", Some(5)));
    };
    if let Some(record) = record {
        let checked = (|| {
            let signed = artifacts::canonical_hex(&record.signed_activity, MAX_ACTIVITY_BYTES)?;
            let activity =
                decode_signed(&signed, &config.registry).map_err(|error| format!("{error:?}"))?;
            let id = activity_id(&activity).map_err(|error| format!("{error:?}"))?;
            if record.idempotency_key != key
                || record.activity_id != hex(&id)
                || record.request_digest != sha256_hex(&signed)
                || activity.activity_type().module() != ModuleId::Programs
                || activity.protocol_version() != PROTOCOL_VERSION
                || activity.network_id() != config.protocol_network_id
            {
                return Err("journal binding mismatch".to_owned());
            }
            Ok((hex(&activity.idempotency_key()), Some(hex(&id))))
        })();
        checked.map_err(|_| refusal(503, "persistence_invalid", Some(5)))
    } else if is_hex32(key) && !key.bytes().any(|byte| byte.is_ascii_uppercase()) {
        Ok((key.to_owned(), None))
    } else {
        Err(refusal(404, "receipt_not_found", None))
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct IdempotencyReceiptDocument {
    activity_id: String,
    receipt: String,
}

fn verify_idempotency_receipt(
    body: &str,
    signer: [u8; 32],
    expected_activity: Option<&str>,
) -> Result<IdempotencyReceiptDocument, String> {
    let document: IdempotencyReceiptDocument =
        serde_json::from_str(body).map_err(|error| error.to_string())?;
    let receipt = artifacts::canonical_hex(&document.receipt, MAX_ACTIVITY_BYTES)?;
    let verified =
        verify_sequencer_signature(&receipt, signer).map_err(|error| format!("{error:?}"))?;
    let protocol = verified
        .protocol()
        .ok_or_else(|| "not protocol receipt".to_owned())?;
    if protocol.module_id() != 9
        || protocol.module_version() != 4
        || protocol.protocol_version() != PROTOCOL_VERSION
        || document.activity_id != hex(&protocol.activity_id())
        || expected_activity.is_some_and(|expected| expected != document.activity_id)
    {
        return Err("receipt binding mismatch".into());
    }
    Ok(document)
}

fn idempotency_receipt_route(config: &Config, key: &str) -> Response {
    if !valid_identifier(key, 128) {
        return refusal(400, "invalid_idempotency_key", None);
    }
    let (protocol_key, expected_activity) = match idempotency_binding(config, key) {
        Ok(binding) => binding,
        Err(response) => return response,
    };
    let answer = relay_bounded(
        config,
        &format!("/v1/programs/receipts/by-idempotency/{protocol_key}"),
        MAX_RELAY_BYTES,
    );
    if answer.status == 404 {
        return refusal(404, "receipt_not_found", None);
    }
    if answer.status != 200 {
        return refusal(503, "receipt_unavailable", Some(5));
    }
    let Ok(signer) = with_session(config, |session| {
        Ok(session.handshake.node().authorised_sequencer_key)
    }) else {
        return refusal(503, "receipt_unavailable", Some(5));
    };
    match verify_idempotency_receipt(&answer.body, signer, expected_activity.as_deref()) {
        Ok(document) => ok(serde_json::json!({"result":{"activity_id":document.activity_id,"receipt":document.receipt}}).to_string()),
        Err(_) => refusal(502, "receipt_invalid", None),
    }
}

fn program_activity_route(config: &Config, activity_text: &str) -> Response {
    let Some(activity) = parse_hex32(activity_text) else {
        return refusal(400, "invalid_activity_id", None);
    };
    let activity_hex = hex(&activity);
    let key_digest = match fs::read_to_string(activity_index_path(config, &activity_hex)) {
        Ok(digest) if is_hex32(&digest) => digest,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            return refusal(404, "activity_not_journaled", None)
        }
        _ => return refusal(503, "persistence_unavailable", Some(5)),
    };
    let lock = key_lock(config, &key_digest);
    let _guard = lock.lock().unwrap_or_else(PoisonError::into_inner);
    let mut record = match load_record(config, &key_digest) {
        Ok(Some(record)) if record.activity_id == activity_hex => record,
        Ok(None) => return refusal(404, "activity_not_journaled", None),
        _ => return refusal(503, "persistence_unavailable", Some(5)),
    };
    let Ok(signed) = artifacts::canonical_hex(&record.signed_activity, MAX_ACTIVITY_BYTES) else {
        return refusal(503, "persistence_invalid", Some(5));
    };
    let decoded = match decode_activity(config, Route::ProgramCall, &signed) {
        Ok(decoded) => decoded,
        Err(response) => return response,
    };
    let Some(program_id) = decoded.program_id.map(|id| hex(&id)) else {
        return refusal(400, "not_program_call", None);
    };
    if decoded.activity_id != activity
        || record
            .program_id
            .as_ref()
            .is_some_and(|stored| stored != &program_id)
    {
        return refusal(503, "persistence_invalid", Some(5));
    }
    record.program_id = Some(program_id.clone());
    let response = if record.state == "completed" && record.program_execution.is_some() {
        completed_response(config, &record)
    } else {
        match with_session(config, |session| {
            lookup_receipt(session, activity, ReceiptWaitMode::Immediate)
        }) {
            Ok(Lookup::Present {
                receipt,
                result_code,
                module_id,
                sequencer_public_key,
            }) => {
                if module_id != 9 {
                    return refusal(400, "not_program_call", None);
                }
                complete_record(
                    config,
                    &key_digest,
                    &mut record,
                    &receipt,
                    result_code,
                    sequencer_public_key,
                )
            }
            Ok(Lookup::Absent) => return refusal(404, "receipt_not_found", None),
            Err(failure) => return failure.response(),
        }
    };
    if response.status != 200 {
        return response;
    }
    let mut document: serde_json::Value = match serde_json::from_str(&response.body) {
        Ok(document) => document,
        Err(_) => return refusal(503, "persistence_invalid", Some(5)),
    };
    document["result"]["program_id"] = serde_json::Value::String(program_id);
    if record.result_code == Some(0) {
        document["result"]["state"] = serde_json::Value::String("executed".into());
    }
    ok(document.to_string())
}

fn relay_path_allowed(path: &str, query: Option<&str>) -> bool {
    let digits = |value: &str| !value.is_empty() && value.bytes().all(|byte| byte.is_ascii_digit());
    if path == "/v1/protocol/account-state/head" {
        return query.is_none();
    }
    if let Some(rest) = path.strip_prefix("/v1/receipts/") {
        return query.is_none() && rest.strip_suffix("/account-state").is_some_and(is_hex32);
    }
    if path == "/v1/programs/account-state/changes" {
        return query
            .and_then(|query| query.strip_prefix("after_sequence="))
            .is_some_and(digits);
    }
    if let Some(rest) = path.strip_prefix("/v1/programs/") {
        return rest.strip_suffix("/account-state").is_some_and(is_hex32)
            && query
                .and_then(|query| query.strip_prefix("at="))
                .is_some_and(digits);
    }
    if let Some(rest) = path.strip_prefix("/v1/batches/") {
        return rest
            .strip_suffix("/receipt-authority")
            .is_some_and(is_hex32)
            && query
                .and_then(|query| query.strip_prefix("receipt_digest="))
                .is_some_and(is_hex32);
    }
    false
}

fn relay_route(config: &Config, target: &str) -> Response {
    relay_bounded(config, target, MAX_RELAY_BYTES)
}

fn relay_bounded(config: &Config, target: &str, maximum: usize) -> Response {
    let address = SocketAddr::from(([127, 0, 0, 1], config.node.port));
    let Ok(mut stream) = TcpStream::connect_timeout(&address, CONNECT_TIMEOUT) else {
        return refusal(503, "node_unavailable", Some(5));
    };
    if stream.set_read_timeout(Some(IO_TIMEOUT)).is_err()
        || stream.set_write_timeout(Some(IO_TIMEOUT)).is_err()
    {
        return refusal(503, "node_unavailable", Some(5));
    }
    let written = write!(
        stream,
        "GET {target} HTTP/1.1\r\nHost: 127.0.0.1:{}\r\nAuthorization: Bearer {}\r\nConnection: close\r\n\r\n",
        config.node.port,
        config.node_token.as_str()
    )
    .and_then(|()| stream.flush());
    if written.is_err() {
        return refusal(503, "node_unavailable", Some(5));
    }
    let Ok(mut upstream) = read_http_message(&mut stream, maximum) else {
        return refusal(503, "node_invalid", Some(5));
    };
    let status = upstream
        .headers
        .get("")
        .and_then(|line| {
            let mut parts = line.split_whitespace();
            (parts.next() == Some("HTTP/1.1"))
                .then(|| parts.next())
                .flatten()
        })
        .and_then(|code| code.parse::<u16>().ok());
    let Some(status) = status.filter(|status| (200..600).contains(status)) else {
        return refusal(503, "node_invalid", Some(5));
    };
    let Ok(body) = String::from_utf8(std::mem::take(&mut upstream.body)) else {
        return refusal(503, "node_invalid", Some(5));
    };
    Response {
        status,
        body,
        retry_after: None,
    }
}

fn readiness(config: &Config) -> Response {
    let protocol_version = match with_session(config, |session| {
        Ok(session.handshake.node().protocol_version)
    }) {
        Ok(version) => version,
        Err(failure) => return failure.response(),
    };
    let address = SocketAddr::from(([127, 0, 0, 1], config.node.port));
    if TcpStream::connect_timeout(&address, CONNECT_TIMEOUT).is_err() {
        return refusal(503, "node_unavailable", Some(5));
    }
    ok(format!(
        "{{\"ready\":true,\"network_id\":\"{}\",\"wire_version\":\"{}\",\"synchronous_receipts\":true,\"state_snapshot\":true}}",
        config.network_name, protocol_version
    ))
}

fn authenticate(config: &Config, request: &Request) -> Result<Plane, Response> {
    let Some(token) = request
        .headers
        .get("authorization")
        .and_then(|value| value.strip_prefix("Bearer "))
    else {
        return Err(refusal(401, "identity_required", None));
    };
    if token.is_empty() || token.len() > 4096 {
        return Err(refusal(401, "identity_required", None));
    }
    if token
        .as_bytes()
        .ct_eq(config.gateway_token.as_bytes())
        .unwrap_u8()
        == 1
    {
        return Ok(Plane::Gateway);
    }
    if token
        .as_bytes()
        .ct_eq(config.registry_token.as_bytes())
        .unwrap_u8()
        == 1
    {
        return Ok(Plane::Registry);
    }
    if token
        .as_bytes()
        .ct_eq(config.webhook_token.as_bytes())
        .unwrap_u8()
        == 1
    {
        return Ok(Plane::Webhook);
    }
    Err(refusal(401, "identity_required", None))
}

fn route(config: &Config, request: &Request) -> Response {
    let (path, query) = request
        .path
        .split_once('?')
        .map_or((request.path.as_str(), None), |(path, query)| {
            (path, Some(query))
        });
    if request.method == "GET" && path == "/livez" && query.is_none() {
        return ok(format!("{{\"status\":\"live\",\"service\":\"{SERVICE}\"}}"));
    }
    if request.method == "GET" && path == "/readyz" && query.is_none() {
        return readiness(config);
    }
    let plane = match authenticate(config, request) {
        Ok(plane) => plane,
        Err(response) => return response,
    };
    if let Some(response) = deployment::route(config, request, path, query, plane) {
        return response;
    }
    if let Some(response) = head_attestation::route(config, request, path, query, plane) {
        return response;
    }
    if relay_path_allowed(path, query) {
        if plane != Plane::Registry {
            return refusal(403, "entitlement_denied", None);
        }
        if request.method != "GET" {
            return refusal(404, "not_found", None);
        }
        return relay_route(config, &request.path);
    }
    if let Some(activity) = path.strip_prefix("/internal/v1/receipts/") {
        if !matches!(plane, Plane::Registry | Plane::Webhook) {
            return refusal(403, "entitlement_denied", None);
        }
        if request.method != "GET" || query.is_some() {
            return refusal(404, "not_found", None);
        }
        return receipt_route(config, activity, plane);
    }
    if !path.starts_with("/v1/") || query.is_some() {
        return refusal(404, "not_found", None);
    }
    let gateway_path = matches!(
        path,
        "/v1/activities"
            | "/v1/programs/call"
            | "/v1/programs/read"
            | "/v1/programs/simulate"
            | "/v1/programs/deploy"
            | "/v1/programs/upgrade"
            | "/v1/programs/wind-down"
    ) || path
        .strip_prefix("/v1/receipts/")
        .or_else(|| path.strip_prefix("/v1/programs/activities/"))
        .is_some_and(|activity| !activity.contains('/'));
    let gateway_path = gateway_path
        || path
            .strip_prefix("/v1/programs/receipts/by-idempotency/")
            .is_some_and(|key| !key.contains('/'));
    if !gateway_path {
        return refusal(404, "not_found", None);
    }
    if plane != Plane::Gateway {
        return refusal(403, "entitlement_denied", None);
    }
    match (request.method.as_str(), path) {
        ("POST", "/v1/activities") => submit_route(config, request, Route::Activities),
        ("POST", "/v1/programs/call") => submit_route(config, request, Route::ProgramCall),
        ("POST", "/v1/programs/deploy") => submit_route(config, request, Route::ProgramDeploy),
        ("POST", "/v1/programs/upgrade") => submit_route(config, request, Route::ProgramUpgrade),
        ("POST", "/v1/programs/wind-down") => submit_route(config, request, Route::ProgramWindDown),
        ("POST", "/v1/programs/read") => program_read_route(config, request),
        ("POST", "/v1/programs/simulate") => simulate_route(config, request),
        ("GET", target) => {
            if let Some(key) = target.strip_prefix("/v1/programs/receipts/by-idempotency/") {
                idempotency_receipt_route(config, key)
            } else if let Some(activity) = target.strip_prefix("/v1/receipts/") {
                receipt_route(config, activity, plane)
            } else if let Some(activity) = target.strip_prefix("/v1/programs/activities/") {
                program_activity_route(config, activity)
            } else {
                refusal(404, "not_found", None)
            }
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

fn read_http_message(stream: &mut impl Read, maximum: usize) -> Result<Request, String> {
    let mut bytes = Vec::with_capacity(2048);
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
    if parts.next() != Some("HTTP/1.1")
        || parts.next().is_some()
        || !request.path.starts_with('/')
        || request.path.contains(['#', '\\', ' '])
    {
        return Err("request line is invalid".to_owned());
    }
    if !request.headers.contains_key("host") {
        return Err("HTTP/1.1 Host header is required".to_owned());
    }
    Ok(request)
}

fn write_response(stream: &mut impl Write, response: &Response) -> Result<(), String> {
    let reason = match response.status {
        200 => "OK",
        202 => "Accepted",
        400 => "Bad Request",
        401 => "Unauthorized",
        403 => "Forbidden",
        404 => "Not Found",
        409 => "Conflict",
        422 => "Unprocessable Content",
        429 => "Too Many Requests",
        502 => "Bad Gateway",
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

fn handle_connection(config: &Arc<Config>, tcp: TcpStream) -> Result<(), String> {
    tcp.set_read_timeout(Some(IO_TIMEOUT))
        .map_err(|error| error.to_string())?;
    tcp.set_write_timeout(Some(IO_TIMEOUT))
        .map_err(|error| error.to_string())?;
    let connection = ServerConnection::new(Arc::clone(&config.tls)).map_err(|e| e.to_string())?;
    let mut stream = StreamOwned::new(connection, tcp);
    let response = parse_client_request(&mut stream).map_or_else(
        |_| refusal(400, "invalid_request", None),
        |request| route(config, &request),
    );
    write_response(&mut stream, &response)?;
    stream.flush().map_err(|error| error.to_string())
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

fn serve(config: Config) -> Result<(), String> {
    let listener = TcpListener::bind(config.listen).map_err(|error| error.to_string())?;
    let config = Arc::new(config);
    eprintln!("layerx-agent-boundary listening with TLS");
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
                        eprintln!("layerx-agent-boundary connection failed: {error}");
                    }
                });
            }
            Err(error) => eprintln!("layerx-agent-boundary accept failed: {error}"),
        }
    }
    Ok(())
}

fn main() {
    if let Err(error) = config().and_then(serve) {
        eprintln!("layerx-agent-boundary: {error}");
        std::process::exit(2);
    }
}
