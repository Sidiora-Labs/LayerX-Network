#![forbid(unsafe_code)]

use std::collections::{BTreeMap, BTreeSet};
use std::env;
use std::fs;
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::sync::{mpsc, Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use layerx_agentd::audit::Redacted;
use layerx_agentd::budget::{LimitConfig, LimitId, LimitScope};
use layerx_agentd::capability::CapabilityId;
use layerx_agentd::enrolment::{
    self, BindingMode, BindingPublisher, DaemonSurface, EnrolmentRequest,
};
use layerx_agentd::human::{HumanListenerConfig, HumanPeer, HumanUnixServer};
use layerx_agentd::human_runtime::{
    HumanAuthorityBoundary, ProductionHumanOperations, RemoteHumanAuthority, UnifiedAgentOwner,
};
use layerx_agentd::identity::{self, CoreIdentity, IdentityError, IdentityResolver};
use layerx_agentd::read::{
    LayerxdProgramBalanceReader, NativeReadRoute, ProgramAuthority, ProgramBalanceRead,
    ProgramBalanceReadRoute,
};
use layerx_agentd::session::{SessionId, SessionRegistry};
use layerx_agentd::session_keys::SessionKeyRegistry;
use layerx_agentd::store::{Store, TenantId};
use layerx_client::client::{ClientConfig, ReconnectPolicy};
use layerx_client::lni::handshake::HandshakeConfig;
use layerx_client::lni::schema::Version;
use layerx_client::lni::transport::Limits;
use layerx_client::Client;
use layerx_programs::{
    hex, DeploymentProof, DeploymentRecord, ProgramId, ProgramLifecycle,
    ProtocolDeploymentVerifier, Registry,
};
use layerx_types::ids::Did;

mod human_owner_mode;
mod human_peer_config;

const HEADER_LIMIT: usize = 16 * 1024;

struct Config {
    listen: String,
    bearer: String,
    node_endpoint: String,
    node_bearer: String,
    authority_endpoint: String,
    authority_bearer: String,
    authority_ca_der: Vec<u8>,
    authority_replica_id: [u8; 32],
    sequencer_trust_history: String,
    staleness_ms: u64,
    deployment_journal: String,
    probe_program: ProgramId,
}

fn optional(name: &str) -> Option<String> {
    env::var(name).ok().filter(|value| !value.is_empty())
}

fn required(name: &str) -> Result<String, String> {
    optional(name).ok_or_else(|| format!("{name} is required"))
}

fn absolute_path(name: &str) -> Result<PathBuf, String> {
    let path = PathBuf::from(required(name)?);
    if path.is_absolute() {
        Ok(path)
    } else {
        Err(format!("{name} must be an absolute path"))
    }
}

fn read_ca(name: &str) -> Result<Vec<u8>, String> {
    let bytes = fs::read(required(name)?).map_err(|_| format!("{name} is unreadable"))?;
    if bytes.is_empty() {
        return Err(format!("{name} is empty"));
    }
    Ok(bytes)
}

fn parse_u64(name: &str) -> Result<u64, String> {
    required(name)?
        .parse()
        .map_err(|_| format!("{name} must be an unsigned integer"))
}

fn parse_digest(name: &str) -> Result<[u8; 32], String> {
    hex::decode_digest(&required(name)?).map_err(|error| format!("{name} is invalid: {error}"))
}

fn parse_hex<const N: usize>(name: &str) -> Result<[u8; N], String> {
    let value = required(name)?;
    if value.len() != N * 2 {
        return Err(format!("{name} has the wrong width"));
    }
    let mut bytes = [0; N];
    for (index, byte) in bytes.iter_mut().enumerate() {
        *byte = u8::from_str_radix(&value[index * 2..index * 2 + 2], 16)
            .map_err(|_| format!("{name} is not hexadecimal"))?;
    }
    Ok(bytes)
}

fn human_peers() -> Result<BTreeMap<u32, (String, String)>, String> {
    human_peer_config::parse(&required("LAYERX_AGENT_HUMAN_PEERS")?)
        .map_err(|error| error.to_string())
}

fn verified_limit() -> Result<LimitConfig, String> {
    let scope_bytes = parse_hex("LAYERX_AGENT_HUMAN_LIMIT_SCOPE_ID")?;
    let scope = match required("LAYERX_AGENT_HUMAN_LIMIT_SCOPE")?.as_str() {
        "tenant" => LimitScope::Tenant(scope_bytes),
        "agent" => LimitScope::Agent(scope_bytes),
        "session" => LimitScope::Session(scope_bytes),
        "capability" => LimitScope::Capability(scope_bytes),
        "counterparty" => LimitScope::Counterparty(scope_bytes),
        _ => return Err("human limit scope is invalid".to_owned()),
    };
    Ok(LimitConfig {
        id: LimitId(parse_hex("LAYERX_AGENT_HUMAN_LIMIT_ID")?),
        name: required("LAYERX_AGENT_HUMAN_LIMIT_NAME")?,
        scope,
        ceiling: required("LAYERX_AGENT_HUMAN_LIMIT_CEILING")?
            .parse()
            .map_err(|_| "human limit ceiling is invalid")?,
        consumed: required("LAYERX_AGENT_HUMAN_LIMIT_CONSUMED")?
            .parse()
            .map_err(|_| "human limit consumed is invalid")?,
    })
}

struct McpEnrolment {
    root: PathBuf,
    audit_root: PathBuf,
    peer_uid: u32,
    did: Did,
    request: EnrolmentRequest,
    limit: LimitConfig,
    deadline: Duration,
    mode: BindingMode,
}

struct McpBoot {
    surface: DaemonSurface,
    enrolment: McpEnrolment,
}

struct ResolvedIdentity {
    did: Did,
    observation: CoreIdentity,
}

impl IdentityResolver for ResolvedIdentity {
    fn resolve(&mut self, did: &Did) -> Result<Option<CoreIdentity>, IdentityError> {
        if did == &self.did {
            Ok(Some(self.observation.clone()))
        } else {
            Ok(None)
        }
    }
}

fn mcp_activity_types() -> Result<BTreeSet<u16>, String> {
    let mut values = BTreeSet::new();
    for entry in required("LAYERX_AGENT_MCP_ACTIVITY_TYPES")?.split(',') {
        let value = entry
            .trim()
            .parse()
            .map_err(|_| "LAYERX_AGENT_MCP_ACTIVITY_TYPES lists an invalid activity type")?;
        if !values.insert(value) {
            return Err("LAYERX_AGENT_MCP_ACTIVITY_TYPES repeats an activity type".to_owned());
        }
    }
    Ok(values)
}

fn mcp_scopes() -> Result<BTreeSet<String>, String> {
    let mut values = BTreeSet::new();
    for entry in required("LAYERX_AGENT_MCP_SCOPES")?.split(',') {
        let value = entry.trim();
        if value.is_empty() {
            return Err("LAYERX_AGENT_MCP_SCOPES lists an empty scope".to_owned());
        }
        if !values.insert(value.to_owned()) {
            return Err("LAYERX_AGENT_MCP_SCOPES repeats a scope".to_owned());
        }
    }
    Ok(values)
}

fn mcp_limit() -> Result<LimitConfig, String> {
    let scope_bytes = parse_hex("LAYERX_AGENT_MCP_LIMIT_SCOPE_ID")?;
    let scope = match required("LAYERX_AGENT_MCP_LIMIT_SCOPE")?.as_str() {
        "tenant" => LimitScope::Tenant(scope_bytes),
        "agent" => LimitScope::Agent(scope_bytes),
        "session" => LimitScope::Session(scope_bytes),
        "capability" => LimitScope::Capability(scope_bytes),
        "counterparty" => LimitScope::Counterparty(scope_bytes),
        _ => return Err("LAYERX_AGENT_MCP_LIMIT_SCOPE is invalid".to_owned()),
    };
    Ok(LimitConfig {
        id: LimitId(parse_hex("LAYERX_AGENT_MCP_LIMIT_ID")?),
        name: required("LAYERX_AGENT_MCP_LIMIT_NAME")?,
        scope,
        ceiling: required("LAYERX_AGENT_MCP_LIMIT_CEILING")?
            .parse()
            .map_err(|_| "LAYERX_AGENT_MCP_LIMIT_CEILING is invalid")?,
        consumed: required("LAYERX_AGENT_MCP_LIMIT_CONSUMED")?
            .parse()
            .map_err(|_| "LAYERX_AGENT_MCP_LIMIT_CONSUMED is invalid")?,
    })
}

/// Reads the model context protocol enrolment this daemon publishes at boot, when one is
/// configured. `LAYERX_AGENT_MCP_BINDING_ROOT` selects the binding directory and every other
/// key is then required.
fn mcp_enrolment() -> Result<Option<McpEnrolment>, String> {
    if optional("LAYERX_AGENT_MCP_BINDING_ROOT").is_none() {
        return Ok(None);
    }
    let mode = match required("LAYERX_AGENT_MCP_MODE")?.as_str() {
        "full" => BindingMode::Full,
        "read-only" => BindingMode::ReadOnly,
        _ => return Err("LAYERX_AGENT_MCP_MODE is invalid".to_owned()),
    };
    Ok(Some(McpEnrolment {
        root: absolute_path("LAYERX_AGENT_MCP_BINDING_ROOT")?,
        audit_root: absolute_path("LAYERX_AGENT_MCP_AUDIT_ROOT")?,
        peer_uid: required("LAYERX_AGENT_MCP_PEER_UID")?
            .parse()
            .map_err(|_| "LAYERX_AGENT_MCP_PEER_UID is invalid")?,
        did: Did::new(required("LAYERX_AGENT_MCP_AGENT_DID")?.as_bytes())
            .map_err(|_| "LAYERX_AGENT_MCP_AGENT_DID is invalid")?,
        request: EnrolmentRequest {
            session_id: SessionId(parse_digest("LAYERX_AGENT_MCP_SESSION_ID")?),
            capability_id: CapabilityId(parse_digest("LAYERX_AGENT_MCP_CAPABILITY_ID")?),
            permitted_activity_types: mcp_activity_types()?,
            scopes: mcp_scopes()?,
            expiry_sequence: parse_u64("LAYERX_AGENT_MCP_EXPIRY_SEQUENCE")?,
            opening_client: required("LAYERX_AGENT_MCP_OPENING_CLIENT")?,
            policy_version: required("LAYERX_AGENT_MCP_POLICY_VERSION")?,
            core_sequence: parse_u64("LAYERX_AGENT_MCP_CORE_SEQUENCE")?,
        },
        limit: mcp_limit()?,
        deadline: Duration::from_millis(parse_u64("LAYERX_AGENT_MCP_DEADLINE_MS")?),
        mode,
    }))
}

fn mcp_boot(config: &Config, enrolment: McpEnrolment) -> Result<McpBoot, String> {
    let surface = DaemonSurface::new(
        &config.listen,
        config.bearer.clone(),
        config.probe_program.bytes(),
    )
    .map_err(|error| format!("the MCP daemon surface is invalid: {error}"))?;
    Ok(McpBoot { surface, enrolment })
}

/// Publishes the binding document one model context protocol session reads.
///
/// The agent identity is resolved through the configured human authority, registered against
/// the daemon store, and enrolled into a capability-grant session whose token reaches disk only
/// inside the operator-protected files beside the document. A daemon that already opened the
/// configured session keeps the document it published rather than opening a second session.
fn publish_mcp_binding(
    authority: &mut RemoteHumanAuthority,
    peers: &BTreeMap<u32, (String, String)>,
    shared_store: &Arc<Mutex<Store>>,
    store_path: &Path,
    boot: McpBoot,
) -> Result<(), String> {
    let McpBoot {
        surface,
        enrolment: configured,
    } = boot;
    let (principal, tenant) = peers
        .get(&configured.peer_uid)
        .ok_or("LAYERX_AGENT_MCP_PEER_UID names no configured human peer")?;
    let peer = HumanPeer {
        uid: configured.peer_uid,
        principal: principal.clone(),
        tenant: tenant.clone(),
    };
    let tenant_id = TenantId::new(tenant.clone())
        .map_err(|error| format!("the MCP peer tenant is invalid: {error:?}"))?;
    let observation = authority
        .core_identity(&peer, &configured.did)
        .map_err(|error| format!("the MCP agent identity is unverified: {error:?}"))?;
    let publisher = BindingPublisher::new(
        configured.root,
        store_path.to_path_buf(),
        configured.audit_root,
        surface,
        configured.limit,
        configured.deadline,
        configured.mode,
    )
    .map_err(|error| format!("the MCP binding publisher is invalid: {error}"))?;
    let mut resolver = ResolvedIdentity {
        did: configured.did.clone(),
        observation,
    };
    let mut store = shared_store
        .lock()
        .map_err(|_| "the agent store is unavailable".to_owned())?;
    let identity = identity::register(&mut store, tenant_id.clone(), configured.did, &mut resolver)
        .map_err(|error| format!("the MCP agent identity is unusable: {error:?}"))?;
    let mut sessions = SessionRegistry::default();
    sessions
        .restore_tenant(&store, &tenant_id)
        .map_err(|error| format!("the agent sessions are unrestorable: {error:?}"))?;
    let session_id = configured.request.session_id;
    if sessions.get(&tenant_id, session_id).is_some() {
        let bound_session = enrolment::published_session(&publisher.binding_path())
            .map_err(|error| format!("the published MCP binding is unusable: {error}"))?;
        if bound_session == Some(session_id) {
            return Ok(());
        }
        return Err(
            "LAYERX_AGENT_MCP_SESSION_ID names an open session without its binding document"
                .to_owned(),
        );
    }
    enrolment::enrol(
        &mut store,
        &mut sessions,
        &identity,
        configured.request,
        &publisher,
    )
    .map_err(|error| format!("MCP enrolment failed: {error}"))?;
    Ok(())
}

fn human_lni_limits(deadline: Duration) -> Result<Limits, String> {
    Limits {
        maximum_frame_bytes: required("LAYERX_AGENT_HUMAN_MAX_FRAME_BYTES")?
            .parse()
            .map_err(|_| "human max frame is invalid")?,
        maximum_connections: required("LAYERX_AGENT_HUMAN_MAX_CONNECTIONS")?
            .parse()
            .map_err(|_| "human max connections is invalid")?,
        maximum_streams: required("LAYERX_AGENT_HUMAN_MAX_STREAMS")?
            .parse()
            .map_err(|_| "human max streams is invalid")?,
        maximum_queued_bytes: required("LAYERX_AGENT_HUMAN_MAX_QUEUED_BYTES")?
            .parse()
            .map_err(|_| "human max queue is invalid")?,
        deadline,
    }
    .validate()
    .map_err(|_| "human LNI limits are invalid".to_owned())
}

fn connect_human_node(node_path: PathBuf, node_limits: Limits) -> Result<Client, String> {
    let human_protocol_version = required("LAYERX_AGENT_HUMAN_PROTOCOL_VERSION")?
        .parse()
        .map_err(|_| "human protocol version is invalid")?;
    if !layerx_wire::limits::protocol_version_uses_occupancy(human_protocol_version) {
        return Err("human protocol version is not the current beta protocol".to_owned());
    }
    Client::connect(ClientConfig {
        endpoint: node_path,
        handshake: HandshakeConfig {
            built_interface_version: Version::V1_3,
            expected_protocol_version: human_protocol_version,
            expected_network_id: required("LAYERX_AGENT_HUMAN_NETWORK_ID")?
                .parse()
                .map_err(|_| "human network id is invalid")?,
        },
        limits: node_limits,
        reconnect: ReconnectPolicy {
            maximum_attempts: required("LAYERX_AGENT_HUMAN_RECONNECT_ATTEMPTS")?
                .parse()
                .map_err(|_| "human reconnect attempts are invalid")?,
            base_delay: Duration::from_millis(parse_u64("LAYERX_AGENT_HUMAN_RECONNECT_BASE_MS")?),
            maximum_delay: Duration::from_millis(parse_u64("LAYERX_AGENT_HUMAN_RECONNECT_MAX_MS")?),
            jitter_percent: required("LAYERX_AGENT_HUMAN_RECONNECT_JITTER_PERCENT")?
                .parse()
                .map_err(|_| "human reconnect jitter is invalid")?,
        },
    })
    .map_err(|error| format!("human node LNI is unavailable: {error:?}"))
}

fn connect_human_authority(
    deadline: Duration,
    peers: &BTreeMap<u32, (String, String)>,
) -> Result<RemoteHumanAuthority, String> {
    let authority = RemoteHumanAuthority::connect(
        &required("LAYERX_AGENT_HUMAN_AUTHORITY_ENDPOINT")?,
        required("LAYERX_AGENT_HUMAN_AUTHORITY_BEARER")?,
        deadline,
        required("LAYERX_AGENT_HUMAN_AUTHORITY_MAX_BYTES")?
            .parse()
            .map_err(|_| "human authority bound is invalid")?,
        &read_ca("LAYERX_AGENT_HUMAN_AUTHORITY_CA_DER")?,
    )
    .map_err(|error| format!("human authority is invalid: {error:?}"))?;
    for (uid, (principal, tenant)) in peers {
        authority
            .registry(&HumanPeer {
                uid: *uid,
                principal: principal.clone(),
                tenant: tenant.clone(),
            })
            .map_err(|error| format!("human authority readiness failed: {error:?}"))?;
    }
    Ok(authority)
}

fn start_human_owner(mcp: Option<McpBoot>) -> Result<mpsc::Receiver<Result<(), String>>, String> {
    let peers = human_peers()?;
    let deadline = Duration::from_millis(parse_u64("LAYERX_AGENT_HUMAN_DEADLINE_MS")?);
    let human_limits = human_lni_limits(deadline)?;
    let node_limits = Limits {
        maximum_frame_bytes: human_limits
            .maximum_frame_bytes
            .max(layerx_client::evidence::MINIMUM_FINALITY_FRAME_BYTES),
        ..human_limits
    };
    let node_path = PathBuf::from(required("LAYERX_AGENT_HUMAN_NODE_LNI")?);
    let store_path = PathBuf::from(required("LAYERX_AGENT_HUMAN_STORE")?);
    let socket_path = PathBuf::from(required("LAYERX_AGENT_HUMAN_SOCKET")?);
    let session_key_root = PathBuf::from(required("LAYERX_AGENT_HUMAN_SESSION_KEY_ROOT")?);
    let session_secret_path =
        PathBuf::from(required("LAYERX_AGENT_HUMAN_SESSION_OPERATOR_SECRET_FILE")?);
    if !node_path.is_absolute()
        || !store_path.is_absolute()
        || !socket_path.is_absolute()
        || !session_key_root.is_absolute()
        || !session_secret_path.is_absolute()
    {
        return Err("human daemon paths must be absolute".to_owned());
    }
    let node = connect_human_node(node_path, node_limits)?;
    let mut authority = connect_human_authority(deadline, &peers)?;
    let shared_store =
        Arc::new(Mutex::new(Store::open(&store_path).map_err(|error| {
            format!("human store is unavailable: {error}")
        })?));
    if let Some(boot) = mcp {
        publish_mcp_binding(&mut authority, &peers, &shared_store, &store_path, boot)?;
    }
    let operations = ProductionHumanOperations::new(
        authority,
        node,
        Arc::clone(&shared_store),
        &peers,
        required("LAYERX_AGENT_HUMAN_MAX_PAYLOAD_BYTES")?
            .parse()
            .map_err(|_| "human payload bound is invalid")?,
        parse_u64("LAYERX_AGENT_HUMAN_TIMESTAMP_SPAN")?,
    )
    .map_err(|error| format!("human operations are invalid: {error:?}"))?;
    let socket_uid = required("LAYERX_AGENT_HUMAN_SOCKET_UID")?
        .parse()
        .map_err(|_| "human socket uid is invalid")?;
    let operator_secret = layerx_agentd::config::read_protected_source(&session_secret_path, 4096)
        .map_err(|error| format!("human session operator secret is unavailable: {error:?}"))?;
    let session_keys = SessionKeyRegistry::open(
        session_key_root,
        operator_secret,
        required("LAYERX_AGENT_HUMAN_NETWORK_ID")?
            .parse()
            .map_err(|_| "human network id is invalid")?,
        socket_uid,
    )
    .map_err(|error| format!("human session key registry is invalid: {error:?}"))?;
    let owner = UnifiedAgentOwner::new(
        operations,
        shared_store,
        &peers,
        vec![verified_limit()?],
        session_keys,
    )
    .map_err(|error| format!("human owner is invalid: {error:?}"))?;
    let server = HumanUnixServer::bind(
        HumanListenerConfig {
            endpoint: socket_path,
            owner_uid: socket_uid,
            owner_gid: required("LAYERX_AGENT_HUMAN_SOCKET_GID")?
                .parse()
                .map_err(|_| "human socket gid is invalid")?,
            mode: u32::from_str_radix(&required("LAYERX_AGENT_HUMAN_SOCKET_MODE")?, 8)
                .map_err(|_| "human socket mode is invalid")?,
            maximum_frame_bytes: human_limits.maximum_frame_bytes,
            deadline,
            peers,
        },
        owner,
    )
    .map_err(|error| format!("human listener is invalid: {error:?}"))?;
    let (sender, receiver) = mpsc::sync_channel(1);
    thread::Builder::new()
        .name("layerx-agent-human".to_owned())
        .spawn(move || {
            let _ = sender.send(
                server
                    .serve()
                    .map_err(|error| format!("human listener stopped: {error:?}")),
            );
        })
        .map_err(|error| format!("human listener thread failed: {error}"))?;
    Ok(receiver)
}

fn config() -> Result<Config, String> {
    let listen = required("LAYERX_AGENT_PROGRAM_LISTEN")?;
    let bearer = required("LAYERX_AGENT_PROGRAM_BEARER_TOKEN")?;
    let node_bearer = required("LAYERX_AGENT_NODE_BEARER_TOKEN")?;
    let authority_bearer = required("LAYERX_AGENT_AUTHORITY_BEARER_TOKEN")?;
    if !listen.starts_with("127.0.0.1:")
        || bearer.len() < 32
        || node_bearer.len() < 32
        || authority_bearer.len() < 32
        || bearer == node_bearer
        || bearer == authority_bearer
    {
        return Err(
            "agent program reads require loopback and distinct bounded credentials".to_owned(),
        );
    }
    let staleness_ms = parse_u64("LAYERX_AGENT_PROGRAM_MAX_STALENESS_MS")?;
    if staleness_ms == 0 {
        return Err("agent staleness bound is non-canonical".to_owned());
    }
    Ok(Config {
        listen,
        bearer,
        node_endpoint: required("LAYERX_AGENT_NODE_ENDPOINT")?,
        node_bearer,
        authority_endpoint: required("LAYERX_AGENT_AUTHORITY_ENDPOINT")?,
        authority_bearer,
        authority_ca_der: read_ca("LAYERX_AGENT_AUTHORITY_CA_DER")?,
        authority_replica_id: parse_digest("LAYERX_AGENT_AUTHORITY_REPLICA_ID")?,
        sequencer_trust_history: required("LAYERX_AGENT_SEQUENCER_TRUST_HISTORY")?,
        staleness_ms,
        deployment_journal: required("LAYERX_AGENT_DEPLOYMENT_JOURNAL")?,
        probe_program: ProgramId::new(parse_digest("LAYERX_AGENT_PROGRAM_PROBE_ID")?)
            .map_err(|error| format!("LAYERX_AGENT_PROGRAM_PROBE_ID is invalid: {error}"))?,
    })
}

fn load_registry(root: &Path, verifier: &ProtocolDeploymentVerifier) -> Result<Registry, String> {
    let mut paths = fs::read_dir(root)
        .map_err(|error| format!("deployment journal is unavailable: {error}"))?
        .map(|entry| entry.map(|value| value.path()))
        .collect::<Result<Vec<_>, _>>()
        .map_err(|error| format!("deployment journal is unreadable: {error}"))?;
    paths.retain(|path| path.extension().is_some_and(|value| value == "admission"));
    paths.sort();
    let mut registry = Registry::new();
    for path in paths {
        let bytes = fs::read(&path)
            .map_err(|error| format!("{} is unreadable: {error}", path.display()))?;
        let proof = DeploymentProof::decode(&bytes)
            .map_err(|error| format!("{} is corrupt: {error}", path.display()))?;
        let evidence = verifier
            .verify_historical_deployment(&proof)
            .map_err(|error| format!("{} is unverified: {error}", path.display()))?;
        let expected = hex::encode(&evidence.receipt_digest());
        if path.file_stem().and_then(|value| value.to_str()) != Some(expected.as_str()) {
            return Err(format!(
                "{} is filed under the wrong receipt",
                path.display()
            ));
        }
        let record_path = root.join(format!("{expected}.deployment"));
        let record = DeploymentRecord::decode(
            &fs::read(&record_path)
                .map_err(|error| format!("{} is unreadable: {error}", record_path.display()))?,
        )
        .map_err(|error| format!("{} is corrupt: {error}", record_path.display()))?;
        record
            .validate()
            .map_err(|error| format!("{} is inadmissible: {error}", record_path.display()))?;
        if &record != evidence.record() {
            return Err(format!(
                "{} disagrees with protocol evidence",
                record_path.display()
            ));
        }
        registry
            .record_verified_deployment(&evidence)
            .map_err(|error| format!("verified deployment replay failed: {error}"))?;
    }
    if registry.program_ids().is_empty() {
        return Err("deployment journal contains no verified admissions".to_owned());
    }
    Ok(registry)
}

fn now_ms() -> Result<u64, String> {
    let value = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|_| "system time precedes the Unix epoch".to_owned())?;
    u64::try_from(value.as_millis()).map_err(|_| "system time is out of range".to_owned())
}

fn lifecycle(value: ProgramLifecycle) -> &'static str {
    match value {
        ProgramLifecycle::Active => "active",
        ProgramLifecycle::Deprecated => "deprecated",
        ProgramLifecycle::Tombstoned => "tombstoned",
    }
}

fn balance_json(read: &ProgramBalanceRead) -> String {
    let accounts = read
        .accounts
        .iter()
        .map(|account| {
            format!(
                "{{\"account\":\"{}\",\"asset\":\"{}\",\"amount\":\"{}\",\"frozen\":{}}}",
                hex::encode(&account.account),
                hex::encode(&account.asset),
                account.amount,
                account.frozen
            )
        })
        .collect::<Vec<_>>()
        .join(",");
    format!(
        "{{\"program\":\"{}\",\"lifecycle\":\"{}\",\"accounts\":[{}],\"freshness\":{{\"observed_sequence\":{},\"observed_at\":{},\"receipt_digest\":\"{}\",\"state_root\":\"{}\",\"valid_through\":{}}}}}",
        hex::encode(&read.program),
        lifecycle(read.lifecycle),
        accounts,
        read.freshness.observed_sequence,
        read.freshness.observed_at,
        hex::encode(&read.freshness.receipt_digest),
        hex::encode(&read.freshness.state_root),
        read.freshness.valid_through
    )
}

fn response(stream: &mut TcpStream, status: u16, body: &str) -> Result<(), String> {
    let reason = if status < 300 { "OK" } else { "Refused" };
    let header = format!(
        "HTTP/1.1 {status} {reason}\r\nContent-Type: application/json\r\nCache-Control: no-store\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
        body.len()
    );
    stream
        .write_all(header.as_bytes())
        .and_then(|()| stream.write_all(body.as_bytes()))
        .map_err(|error| format!("agent response failed: {error}"))
}

fn serve_connection(
    stream: &mut TcpStream,
    bearer: &str,
    probe_program: ProgramId,
    route: &mut ProgramBalanceReadRoute,
    native: &mut Option<NativeReadRoute>,
) -> Result<(), String> {
    let mut bytes = [0_u8; HEADER_LIMIT];
    let mut length = 0_usize;
    while length < bytes.len() && !bytes[..length].windows(4).any(|value| value == b"\r\n\r\n") {
        let count = stream
            .read(&mut bytes[length..])
            .map_err(|error| format!("agent request failed: {error}"))?;
        if count == 0 {
            return Err("agent request ended before its headers".to_owned());
        }
        length += count;
    }
    if !bytes[..length].windows(4).any(|value| value == b"\r\n\r\n") {
        return response(stream, 431, "{\"error\":\"headers_too_large\"}");
    }
    let request = std::str::from_utf8(&bytes[..length])
        .map_err(|_| "agent request headers are not UTF-8".to_owned())?;
    let line = request
        .lines()
        .next()
        .ok_or_else(|| "agent request omitted its request line".to_owned())?;
    let mut parts = line.split_ascii_whitespace();
    let method = parts.next().unwrap_or_default();
    let path = parts.next().unwrap_or_default();
    if parts.next() != Some("HTTP/1.1") || parts.next().is_some() || method != "GET" {
        return response(stream, 400, "{\"error\":\"invalid_request\"}");
    }
    let authorized = request
        .lines()
        .any(|header| header.strip_prefix("Authorization: Bearer ") == Some(bearer));
    if !authorized {
        return response(stream, 401, "{\"error\":\"unauthorized\"}");
    }
    if path == "/healthz" {
        return match route.read(probe_program, now_ms()?) {
            Ok(_) => response(stream, 200, "{\"ready\":true}"),
            Err(_) => response(stream, 503, "{\"ready\":false}"),
        };
    }
    if path.starts_with("/v1/reads/") {
        let Some(reader) = native.as_mut() else {
            return response(stream, 404, "{\"error\":\"not_found\"}");
        };
        return match reader.read(path) {
            Ok(value) => response(stream, 200, &value.to_string()),
            Err(
                layerx_agentd::read::NativeReadError::InvalidRequest
                | layerx_agentd::read::NativeReadError::CursorMismatch,
            ) => response(stream, 400, "{\"error\":\"invalid_read\"}"),
            Err(layerx_agentd::read::NativeReadError::ResultTooLarge) => {
                response(stream, 413, "{\"error\":\"read_too_large\"}")
            }
            Err(_) => response(stream, 503, "{\"error\":\"verified_read_unavailable\"}"),
        };
    }
    let Some(program_text) = path
        .strip_prefix("/v1/programs/")
        .and_then(|value| value.strip_suffix("/balances"))
    else {
        return response(stream, 404, "{\"error\":\"not_found\"}");
    };
    let program = hex::decode_digest(program_text)
        .ok()
        .and_then(|bytes| ProgramId::new(bytes).ok());
    let Some(program) = program else {
        return response(stream, 400, "{\"error\":\"invalid_program\"}");
    };
    let read = route
        .read(program, now_ms()?)
        .map_err(|error| format!("current program state is unavailable: {error:?}"));
    match read {
        Ok(read) => response(stream, 200, &balance_json(&read)),
        Err(_) => response(stream, 503, "{\"error\":\"program_state_unavailable\"}"),
    }
}

fn serve(config: Config) -> Result<(), String> {
    let mcp = mcp_enrolment()?
        .map(|enrolment| mcp_boot(&config, enrolment))
        .transpose()?;
    let mut native = mcp
        .as_ref()
        .map(|boot| {
            let limits = human_lni_limits(boot.enrolment.deadline)?;
            let limits = Limits {
                maximum_frame_bytes: limits
                    .maximum_frame_bytes
                    .max(layerx_client::evidence::MINIMUM_FINALITY_FRAME_BYTES),
                ..limits
            };
            let client = connect_human_node(
                PathBuf::from(required("LAYERX_AGENT_HUMAN_NODE_LNI")?),
                limits,
            )?;
            NativeReadRoute::new(
                client,
                boot.enrolment.did.clone(),
                config.bearer.clone(),
                Instant::now,
            )
            .map_err(|error| format!("native read route is invalid: {error:?}"))
        })
        .transpose()?;
    let human = start_human_owner(mcp)?;
    let verifier = ProtocolDeploymentVerifier::from_protected_history(
        Path::new(&config.sequencer_trust_history),
        config.staleness_ms,
    )
    .map_err(|error| format!("agent deployment verifier is invalid: {error}"))?;
    let registry = load_registry(Path::new(&config.deployment_journal), &verifier)?;
    let reader = LayerxdProgramBalanceReader::connect(
        &config.node_endpoint,
        config.node_bearer,
        ProgramAuthority {
            endpoint: &config.authority_endpoint,
            authorization: config.authority_bearer,
            replica_id: config.authority_replica_id,
            ca_der: &config.authority_ca_der,
        },
        verifier,
        registry,
    )
    .map_err(|error| format!("agent protocol reader configuration failed: {error:?}"))?;
    let mut route = ProgramBalanceReadRoute::new(reader);
    route
        .read(config.probe_program, now_ms()?)
        .map_err(|error| format!("agent protocol reader is not ready: {error:?}"))?;
    let listener = TcpListener::bind(&config.listen)
        .map_err(|error| format!("agent program listener failed: {error}"))?;
    listener
        .set_nonblocking(true)
        .map_err(|error| format!("agent program listener nonblocking setup failed: {error}"))?;
    loop {
        match human.try_recv() {
            Ok(result) => return result,
            Err(mpsc::TryRecvError::Disconnected) => {
                return Err("human listener terminated without status".to_owned())
            }
            Err(mpsc::TryRecvError::Empty) => {}
        }
        let mut stream = match listener.accept() {
            Ok((stream, _)) => stream,
            Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                thread::sleep(Duration::from_millis(25));
                continue;
            }
            Err(error) => return Err(format!("agent accept failed: {error}")),
        };
        stream
            .set_read_timeout(Some(Duration::from_secs(10)))
            .and_then(|()| stream.set_write_timeout(Some(Duration::from_secs(10))))
            .map_err(|error| format!("agent connection timeout setup failed: {error}"))?;
        let _ = serve_connection(
            &mut stream,
            &config.bearer,
            config.probe_program,
            &mut route,
            &mut native,
        );
    }
}

fn main() {
    if let Err(error) = human_owner_mode::run() {
        eprintln!("layerx-agentd: {}", Redacted::boot_diagnostic(&error));
        std::process::exit(2);
    }
}
