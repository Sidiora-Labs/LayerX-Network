mod artifacts;

use layerx_client::lni::handshake::{perform, Handshake, HandshakeConfig};
use layerx_client::lni::refusal::decode_core_refusal;
use layerx_client::lni::schema::{decode_envelope, encode_envelope, Capability, Envelope, Version};
use layerx_client::lni::transport::{ConnectionGate, FrameTransport, Limits, Uds};
use layerx_client::submit::{submit_signed, Submission, SubmissionContext, SubmitError};
use layerx_proof::receipt::verify_sequencer_signature;
use layerx_types::intent::ProgramCall;
use layerx_types::payload::{ActivityType, ModuleId, ModuleRegistration, ModuleRegistry};
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
use std::sync::{Arc, Mutex, PoisonError};
use std::thread;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};
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
const RECEIPT_POLL_INTERVAL: Duration = Duration::from_millis(50);
const RECEIPT_LOOKUP_REQUEST_TAG: u16 = 5;
const RECEIPT_LOOKUP_RESPONSE_TAG: u16 = 6;
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
    session: Mutex<Option<Session>>,
    key_locks: Mutex<BTreeMap<String, Arc<Mutex<()>>>>,
}

struct Session {
    transport: Uds,
    handshake: Handshake,
    next_correlation: u64,
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
}

#[derive(Clone, Copy, Eq, PartialEq)]
enum Route {
    Activities,
    ProgramCall,
}

impl Route {
    const fn name(self) -> &'static str {
        match self {
            Self::Activities => "activities",
            Self::ProgramCall => "programs_call",
        }
    }

    const fn success_state(self) -> &'static str {
        match self {
            Self::Activities => "completed",
            Self::ProgramCall => "executed",
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
}

impl LniFailure {
    fn response(&self) -> Response {
        match self {
            Self::Unavailable(detail) => {
                eprintln!("{SERVICE}: node unavailable: {detail}");
                refusal(503, "node_unavailable", Some(5))
            }
            Self::Transport(detail) => {
                eprintln!("{SERVICE}: node transport lost: {detail}");
                refusal(503, "node_transport_lost", Some(5))
            }
        }
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
        session: Mutex::new(None),
        key_locks: Mutex::new(BTreeMap::new()),
    })
}

fn lni_limits(config: &Config) -> Limits {
    Limits {
        maximum_frame_bytes: LNI_FRAME_BYTES,
        maximum_connections: LNI_CONNECTIONS,
        maximum_streams: 1,
        maximum_queued_bytes: LNI_FRAME_BYTES,
        deadline: config.lni_deadline,
    }
}

fn open_session(config: &Config) -> Result<Session, LniFailure> {
    let mut transport = Uds::connect(&config.lni_socket, &config.gate, lni_limits(config))
        .map_err(|error| LniFailure::Unavailable(format!("{error:?}")))?;
    let expected = HandshakeConfig {
        built_interface_version: Version::V1_3,
        expected_protocol_version: PROTOCOL_VERSION,
        expected_network_id: config.protocol_network_id,
    };
    let handshake = perform(&mut transport, &expected, None)
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
    let mut guard = config
        .session
        .lock()
        .unwrap_or_else(PoisonError::into_inner);
    if guard.is_none() {
        *guard = Some(open_session(config)?);
    }
    let Some(session) = guard.as_mut() else {
        return Err(LniFailure::Unavailable("session is absent".to_owned()));
    };
    let result = operation(session);
    if let Err(LniFailure::Transport(_)) = &result {
        *guard = None;
    }
    result
}

fn lookup_receipt(session: &mut Session, activity: [u8; 32]) -> Result<Lookup, LniFailure> {
    let correlation_id = session.correlation();
    let mut selector = Vec::with_capacity(33);
    selector.push(1);
    selector.extend_from_slice(&activity);
    let request = encode_envelope(Envelope {
        version: Version::V1_3,
        message_tag: RECEIPT_LOOKUP_REQUEST_TAG,
        correlation_id,
        canonical_payload: &selector,
        proof_material: &[],
    })
    .map_err(|error| LniFailure::Unavailable(format!("{error:?}")))?;
    session
        .transport
        .send(&request)
        .map_err(|error| LniFailure::Transport(format!("{error:?}")))?;
    let response = session
        .transport
        .receive()
        .map_err(|error| LniFailure::Transport(format!("{error:?}")))?;
    let response =
        decode_envelope(&response).map_err(|error| LniFailure::Transport(format!("{error:?}")))?;
    if response.correlation_id != correlation_id {
        return Err(LniFailure::Transport(
            "receipt lookup correlation mismatch".to_owned(),
        ));
    }
    if response.message_tag == ERROR_RESPONSE_TAG {
        let refusal = decode_core_refusal(response.canonical_payload);
        return Err(LniFailure::Unavailable(format!(
            "receipt lookup refused: {refusal:?}"
        )));
    }
    if response.message_tag != RECEIPT_LOOKUP_RESPONSE_TAG {
        return Err(LniFailure::Transport(
            "receipt lookup answered with an unexpected message".to_owned(),
        ));
    }
    if response.canonical_payload.is_empty() {
        return Ok(Lookup::Absent);
    }
    let sequencer_key = session.handshake.node().authorised_sequencer_key;
    let receipt = verify_sequencer_signature(response.canonical_payload, sequencer_key)
        .map_err(|error| LniFailure::Unavailable(format!("receipt is not authentic: {error:?}")))?;
    let Some(protocol) = receipt.protocol() else {
        return Err(LniFailure::Unavailable(
            "receipt is not a protocol receipt".to_owned(),
        ));
    };
    if protocol.activity_id() != activity {
        return Err(LniFailure::Unavailable(
            "receipt names a different activity".to_owned(),
        ));
    }
    Ok(Lookup::Present {
        receipt: response.canonical_payload.to_vec(),
        result_code: protocol.result_code(),
        module_id: protocol.module_id(),
        sequencer_public_key: sequencer_key,
    })
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

fn submit_activity(
    config: &Config,
    session: &mut Session,
    decoded: &Decoded,
    attempt: u32,
    signed: &[u8],
) -> Result<SubmitOutcome, LniFailure> {
    let node = session.handshake.node();
    let context = SubmissionContext {
        interface_version: Version::V1_3,
        protocol_version: node.protocol_version,
        network_id: node.network_id,
        correlation_id: session.correlation(),
        signer_public_key: decoded.signer_public_key,
        attempt,
    };
    match submit_signed(&mut session.transport, &config.registry, context, signed) {
        Ok(Submission::Acknowledged(acknowledgement)) => {
            if acknowledgement.activity_id() != decoded.activity_id {
                return Err(LniFailure::Transport(
                    "acknowledgement names a different activity".to_owned(),
                ));
            }
            Ok(SubmitOutcome::Acknowledged)
        }
        Ok(Submission::Unknown(_)) => Err(LniFailure::Transport(
            "submission outcome is indeterminate".to_owned(),
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
    if route == Route::ProgramCall && !is_program_call {
        return Err(refusal(400, "not_program_call", None));
    }
    let program_id = if is_program_call {
        let call = ProgramCall::from_canonical_payload(activity.payload())
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

fn outcome_response(record: &JournalRecord) -> Response {
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
    let route = if record.route == Route::ProgramCall.name() {
        Route::ProgramCall
    } else {
        Route::Activities
    };
    let state = if record.result_code == Some(0) {
        route.success_state()
    } else {
        "refused"
    };
    ok(format!(
        "{{\"result\":{{\"state\":\"{state}\",\"activity_id\":\"{}\",\"receipt\":\"{receipt}\",\"terminal_payload\":\"{terminal_payload}\",\"call_graph\":\"{call_graph}\"}}}}",
        record.activity_id
    ))
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
    artifacts::verify(
        &stored,
        receipt,
        activity_id,
        program_id,
        config.protocol_network_id,
    )
    .map_err(|detail| {
        eprintln!("{SERVICE}: {detail}");
        invalid()
    })?;
    Ok(stored)
}

fn completed_response(config: &Config, record: &JournalRecord) -> Response {
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
            let call = ProgramCall::from_canonical_payload(activity.payload())
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
                activity_id,
                program_id,
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
        match fetch_program_execution(
            config,
            receipt,
            activity_id,
            program_id,
            sequencer_public_key,
        ) {
            Ok(execution) => record.program_execution = Some(execution),
            Err(response) => return response,
        }
    }
    "completed".clone_into(&mut record.state);
    record.receipt = Some(hex(receipt));
    record.result_code = Some(result_code);
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
    let deadline = Instant::now() + wait;
    loop {
        let lookup = with_session(config, |session| lookup_receipt(session, activity))?;
        if matches!(lookup, Lookup::Present { .. }) || Instant::now() >= deadline {
            return Ok(lookup);
        }
        thread::sleep(RECEIPT_POLL_INTERVAL);
    }
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
        Err(failure @ LniFailure::Unavailable(_)) => failure.response(),
    }
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
        },
    };
    resolve_record(config, &key_digest, &mut record, &decoded, &request.body)
}

fn receipt_route(config: &Config, activity_text: &str) -> Response {
    let Some(activity) = parse_hex32(activity_text) else {
        return refusal(400, "invalid_activity_id", None);
    };
    match with_session(config, |session| lookup_receipt(session, activity)) {
        Ok(Lookup::Present { receipt, .. }) => ok(format!(
            "{{\"activity_id\":\"{}\",\"receipt\":\"{}\"}}",
            hex(&activity),
            hex(&receipt)
        )),
        Ok(Lookup::Absent) => refusal(404, "receipt_not_found", None),
        Err(failure) => failure.response(),
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
        match with_session(config, |session| lookup_receipt(session, activity)) {
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
    let session = match open_session(config) {
        Ok(session) => session,
        Err(failure) => {
            *config
                .session
                .lock()
                .unwrap_or_else(PoisonError::into_inner) = None;
            return failure.response();
        }
    };
    let node = session.handshake.node();
    if node.network_id != config.protocol_network_id {
        return refusal(503, "wrong_network", Some(5));
    }
    let address = SocketAddr::from(([127, 0, 0, 1], config.node.port));
    if TcpStream::connect_timeout(&address, CONNECT_TIMEOUT).is_err() {
        return refusal(503, "node_unavailable", Some(5));
    }
    ok(format!(
        "{{\"ready\":true,\"network_id\":\"{}\",\"wire_version\":\"{}\",\"synchronous_receipts\":true,\"state_snapshot\":true}}",
        config.network_name, node.protocol_version
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
        if plane != Plane::Registry {
            return refusal(403, "entitlement_denied", None);
        }
        if request.method != "GET" || query.is_some() {
            return refusal(404, "not_found", None);
        }
        return receipt_route(config, activity);
    }
    if !path.starts_with("/v1/") || query.is_some() {
        return refusal(404, "not_found", None);
    }
    let gateway_path = matches!(
        path,
        "/v1/activities" | "/v1/programs/call" | "/v1/programs/simulate"
    ) || path
        .strip_prefix("/v1/receipts/")
        .or_else(|| path.strip_prefix("/v1/programs/activities/"))
        .is_some_and(|activity| !activity.contains('/'));
    if !gateway_path {
        return refusal(404, "not_found", None);
    }
    if plane != Plane::Gateway {
        return refusal(403, "entitlement_denied", None);
    }
    match (request.method.as_str(), path) {
        ("POST", "/v1/activities") => submit_route(config, request, Route::Activities),
        ("POST", "/v1/programs/call") => submit_route(config, request, Route::ProgramCall),
        ("POST", "/v1/programs/simulate") => refusal(503, "capability_unavailable", Some(60)),
        ("GET", target) => {
            if let Some(activity) = target.strip_prefix("/v1/receipts/") {
                receipt_route(config, activity)
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
