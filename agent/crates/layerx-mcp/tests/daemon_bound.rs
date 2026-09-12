use std::collections::BTreeSet;
use std::fs;
use std::io::{Cursor, Read as _, Write as _};
use std::net::TcpListener;
use std::os::unix::fs::{MetadataExt as _, PermissionsExt as _};
use std::os::unix::net::UnixStream;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::thread;
use std::time::Duration;

use layerx_agentd::budget::{LimitConfig, LimitId, LimitScope};
use layerx_agentd::capability::{Capability, CapabilityDimensions, CapabilityId, RateCeiling};
use layerx_agentd::enrolment::{
    self, BindingMode, BindingPublisher, DaemonSurface, EnrolmentError, EnrolmentRequest,
};
use layerx_agentd::identity::{
    register, CoreIdentity, IdentityError, IdentityRecord, IdentityResolver, ProtocolAuthority,
};
use layerx_agentd::session::{open, OpenRequest, SessionId, SessionRegistry};
use layerx_agentd::store::{Store, TenantId};
use layerx_mcp::binding::{Binding, BindingError};
use layerx_mcp::catalogue;
use layerx_mcp::listener::{Listener, ListenerConfig, ListenerError};
use layerx_mcp::server::{catalogue as served, DeploymentMode, ToolKind};
use layerx_mcp::stdio::Session;
use layerx_types::ids::Did;
use layerx_types::verify::VerificationLevel;
use serde_json::{json, Value};

static NEXT_DIRECTORY: AtomicU64 = AtomicU64::new(1);

const SESSION_ID: &str = "0707070707070707070707070707070707070707070707070707070707070707";
const CAPABILITY_ID: &str = "0909090909090909090909090909090909090909090909090909090909090909";
const OBSERVED_SEQUENCE: u64 = 120;

fn directory(label: &str) -> PathBuf {
    let sequence = NEXT_DIRECTORY.fetch_add(1, Ordering::Relaxed);
    let base = std::env::temp_dir();
    let canonical = fs::canonicalize(&base).unwrap_or(base);
    let root = canonical.join(format!(
        "layerx-mcp-daemon-{label}-{}-{sequence}",
        std::process::id()
    ));
    fs::create_dir_all(&root).unwrap_or_else(|error| panic!("root {label}: {error}"));
    fs::set_permissions(&root, fs::Permissions::from_mode(0o700))
        .unwrap_or_else(|error| panic!("root mode {label}: {error}"));
    root
}

fn secret(root: &Path, name: &str, value: &str) -> PathBuf {
    let path = root.join(name);
    fs::write(&path, value).unwrap_or_else(|error| panic!("secret {name}: {error}"));
    fs::set_permissions(&path, fs::Permissions::from_mode(0o600))
        .unwrap_or_else(|error| panic!("secret mode {name}: {error}"));
    path
}

type Mutation<'a> = &'a dyn Fn(&mut serde_json::Map<String, Value>);

fn hex(bytes: &[u8]) -> String {
    const DIGITS: &[u8; 16] = b"0123456789abcdef";
    let mut output = String::with_capacity(bytes.len().saturating_mul(2));
    for byte in bytes {
        output.push(char::from(DIGITS[usize::from(byte >> 4)]));
        output.push(char::from(DIGITS[usize::from(byte & 0x0f)]));
    }
    output
}

struct BoundaryIdentity(CoreIdentity);

impl IdentityResolver for BoundaryIdentity {
    fn resolve(&mut self, _did: &Did) -> Result<Option<CoreIdentity>, IdentityError> {
        Ok(Some(self.0.clone()))
    }
}

/// Persists one real capability and registers one real identity carrying its grant.
fn daemon_records(store: &mut Store) -> (Capability, IdentityRecord) {
    let tenant = TenantId::new("tenant-a").unwrap_or_else(|error| panic!("tenant: {error}"));
    let capability = Capability::new(
        CapabilityId([9; 32]),
        tenant.clone(),
        CapabilityDimensions {
            activity_types: BTreeSet::from([7]),
            counterparties: BTreeSet::from([[2; 32]]),
            assets: BTreeSet::from([[3; 32]]),
            amount_ceiling: 100,
            rate_ceiling: RateCeiling {
                maximum_uses: 2,
                window_sequences: 10,
            },
            purposes: BTreeSet::from(["service-payment".to_owned()]),
            expiry_sequence: 400,
        },
    )
    .unwrap_or_else(|error| panic!("capability: {error:?}"));
    capability
        .persist(store)
        .unwrap_or_else(|error| panic!("capability persist: {error:?}"));
    let authority = ProtocolAuthority::CapabilityGrant(capability.id.0);
    let mut boundary = BoundaryIdentity(CoreIdentity {
        canonical_bytes: b"model-identity".to_vec(),
        head_sequence: 10,
        revocation_sequence: 1,
        verification_level: VerificationLevel::STATE_PROVEN,
        frozen: false,
        authorities: vec![authority],
    });
    let identity = register(
        store,
        tenant,
        Did::new(b"did:layerx:model").unwrap_or_else(|error| panic!("DID: {error:?}")),
        &mut boundary,
    )
    .unwrap_or_else(|error| panic!("identity: {error:?}"));
    (capability, identity)
}

/// Persists one real capability and one real open session, returning its bearer token.
fn enrol(root: &Path) -> [u8; 32] {
    let mut store =
        Store::open(root.join("store")).unwrap_or_else(|error| panic!("store: {error}"));
    let (capability, identity) = daemon_records(&mut store);
    let request = OpenRequest {
        session_id: SessionId([7; 32]),
        token_id: [8; 32],
        tenant: identity.tenant().clone(),
        agent: identity.did().clone(),
        authority: ProtocolAuthority::CapabilityGrant(capability.id.0),
        permitted_activity_types: BTreeSet::from([7]),
        scopes: served()
            .iter()
            .map(|tool| tool.required_scope.to_owned())
            .collect(),
        expiry_sequence: 300,
        opening_client: "mcp".to_owned(),
        policy_version: "policy-v1".to_owned(),
    };
    let mut sessions = SessionRegistry::default();
    let credential = open(&mut store, &mut sessions, &identity, request, 50)
        .unwrap_or_else(|error| panic!("session: {error:?}"))
        .credential();
    credential.token_id()
}

fn document(root: &Path, endpoint: &str, mode: &str, listener: Option<&str>) -> String {
    let token = enrol(root);
    let token_file = secret(root, "session-token", &hex(&token));
    let bearer_file = secret(root, "agent-bearer", &"b".repeat(48));
    let mut value = json!({
        "mode": mode,
        "tenant": "tenant-a",
        "store": root.join("store").display().to_string(),
        "audit_root": root.join("audit").display().to_string(),
        "session_id": SESSION_ID,
        "session_token_file": token_file.display().to_string(),
        "session_generation": 1,
        "capability_id": CAPABILITY_ID,
        "core_sequence": 50,
        "deadline_ms": 5_000,
        "agent": {
            "endpoint": endpoint,
            "bearer_file": bearer_file.display().to_string(),
            "probe_program": "cc".repeat(32),
        },
        "limit": {
            "id": "0a".repeat(16),
            "name": "mcp",
            "scope": "tenant",
            "scope_id": "01".repeat(32),
            "ceiling": "1000",
            "consumed": "0",
        },
    });
    if let (Some(socket), Some(fields)) = (listener, value.as_object_mut()) {
        let metadata = fs::metadata(root).unwrap_or_else(|error| panic!("root metadata: {error}"));
        fields.insert(
            "listener".to_owned(),
            json!({
                "socket": socket,
                "owner_uid": metadata.uid(),
                "owner_gid": metadata.gid(),
                "mode": "660",
                "admitted_uids": [metadata.uid()],
            }),
        );
    }
    serde_json::to_string(&value).unwrap_or_else(|error| panic!("document: {error}"))
}

fn write_document(root: &Path, body: &str) -> PathBuf {
    let path = root.join("binding.json");
    fs::write(&path, body).unwrap_or_else(|error| panic!("binding: {error}"));
    path
}

/// Replays the exact response `layerx-agentd` writes for its verified program balance route
/// (`agent/crates/layerx-agentd/src/main.rs`), for one bearer, on loopback.
fn agent_daemon(bearer: String) -> String {
    let listener =
        TcpListener::bind("127.0.0.1:0").unwrap_or_else(|error| panic!("agent listener: {error}"));
    let endpoint = listener
        .local_addr()
        .unwrap_or_else(|error| panic!("agent address: {error}"))
        .to_string();
    thread::spawn(move || loop {
        let Ok((mut stream, _)) = listener.accept() else {
            return;
        };
        let mut request = Vec::new();
        let mut chunk = [0_u8; 1_024];
        while !request.windows(4).any(|window| window == b"\r\n\r\n") {
            match stream.read(&mut chunk) {
                Ok(0) | Err(_) => break,
                Ok(count) => request.extend_from_slice(&chunk[..count]),
            }
        }
        let text = String::from_utf8_lossy(&request).into_owned();
        let authorized = text
            .lines()
            .any(|header| header.strip_prefix("Authorization: Bearer ") == Some(bearer.as_str()));
        let path = text
            .lines()
            .next()
            .and_then(|line| line.split_ascii_whitespace().nth(1))
            .unwrap_or_default()
            .to_owned();
        let (status, body) = if authorized {
            match path
                .strip_prefix("/v1/programs/")
                .and_then(|value| value.strip_suffix("/balances"))
            {
                Some(program) => (200, balances(program)),
                None => (404, "{\"error\":\"not_found\"}".to_owned()),
            }
        } else {
            (401, "{\"error\":\"unauthorized\"}".to_owned())
        };
        let reason = if status < 300 { "OK" } else { "Refused" };
        let header = format!(
            "HTTP/1.1 {status} {reason}\r\nContent-Type: application/json\r\nCache-Control: no-store\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
            body.len()
        );
        let _ = stream
            .write_all(header.as_bytes())
            .and_then(|()| stream.write_all(body.as_bytes()))
            .and_then(|()| stream.flush());
    });
    endpoint
}

fn balances(program: &str) -> String {
    format!(
        "{{\"program\":\"{program}\",\"lifecycle\":\"active\",\"accounts\":[{{\"account\":\"{}\",\"asset\":\"{}\",\"amount\":\"7\",\"frozen\":false}}],\"freshness\":{{\"observed_sequence\":{OBSERVED_SEQUENCE},\"observed_at\":1,\"receipt_digest\":\"{}\",\"state_root\":\"{}\",\"valid_through\":400}}}}",
        "11".repeat(32),
        "22".repeat(32),
        "33".repeat(32),
        "44".repeat(32)
    )
}

fn exchange(
    session: &mut Session<layerx_mcp::boundary::ProgramReads>,
    requests: &[Value],
) -> Vec<Value> {
    let mut input = String::new();
    for request in requests {
        input.push_str(
            &serde_json::to_string(request).unwrap_or_else(|error| panic!("request: {error}")),
        );
        input.push('\n');
    }
    let mut reader = Cursor::new(input.into_bytes());
    let mut writer = Vec::new();
    session
        .serve(&mut reader, &mut writer)
        .unwrap_or_else(|error| panic!("serve: {error}"));
    String::from_utf8_lossy(&writer)
        .lines()
        .map(|line| {
            serde_json::from_str::<Value>(line).unwrap_or_else(|error| panic!("response: {error}"))
        })
        .collect()
}

#[test]
fn the_served_catalogue_is_the_daemon_catalogue_and_every_tool_is_fully_described() {
    let full = catalogue::surface(DeploymentMode::Full);
    assert_eq!(full.len(), 21);
    assert_eq!(full.as_slice(), served());
    let read_only = catalogue::surface(DeploymentMode::ReadOnly);
    assert!(!read_only.is_empty());
    assert!(read_only.iter().all(|tool| tool.kind == ToolKind::Read));
    assert_eq!(
        read_only.len(),
        served()
            .iter()
            .filter(|tool| tool.kind == ToolKind::Read)
            .count()
    );
    for tool in served() {
        let listing = catalogue::listing(*tool)
            .unwrap_or_else(|| panic!("tool {} has no listing", tool.name));
        assert_eq!(
            listing.pointer("/name").and_then(Value::as_str),
            Some(tool.name)
        );
        assert!(catalogue::description(tool.name).is_some());
        let schema = catalogue::input_schema(tool.name)
            .unwrap_or_else(|| panic!("tool {} has no schema", tool.name));
        assert_eq!(
            schema
                .pointer("/additionalProperties")
                .and_then(Value::as_bool),
            Some(false)
        );
        assert_eq!(
            listing
                .pointer("/annotations/readOnlyHint")
                .and_then(Value::as_bool),
            Some(tool.kind == ToolKind::Read)
        );
        assert_eq!(
            listing
                .pointer("/_meta/layerx~1scope")
                .and_then(Value::as_str),
            Some(tool.required_scope)
        );
    }
}

#[test]
fn arguments_outside_the_declared_shape_never_reach_the_daemon() {
    let program = "cc".repeat(32);
    assert!(catalogue::validate("balance.get", &json!({"program": program})).is_ok());
    assert!(catalogue::validate(
        "history.list",
        &json!({"account": "11".repeat(32), "limit": "32"})
    )
    .is_ok());
    let cases: [(&str, Value); 8] = [
        ("balance.get", json!({"program": program, "extra": "1"})),
        ("balance.get", json!({})),
        ("balance.get", json!({"program": 7})),
        ("balance.get", json!({"program": "zz".repeat(32)})),
        (
            "history.list",
            json!({"account": "11".repeat(32), "limit": "0"}),
        ),
        (
            "history.list",
            json!({"account": "11".repeat(32), "limit": "257"}),
        ),
        (
            "activity.wait",
            json!({"submission_ref": "ref-1", "timeout_ms": "600001"}),
        ),
        (
            "token.create",
            json!({
                "symbol": "LXP",
                "decimals": "19",
                "supply": "10",
                "idempotency_key": "11".repeat(32),
            }),
        ),
    ];
    for (tool, arguments) in cases {
        let refusal = catalogue::validate(tool, &arguments);
        assert!(refusal.is_err(), "{tool} accepted {arguments}");
        let detail = refusal
            .err()
            .map(|error| error.detail())
            .unwrap_or_default();
        assert!(!detail.is_empty());
    }
    assert!(catalogue::validate("balance.get", &json!(["program"])).is_err());
}

#[test]
fn a_binding_document_is_closed_complete_and_narrowable() {
    let root = directory("binding");
    let body = document(
        &root,
        "127.0.0.1:9440",
        "full",
        Some("/run/layerx/mcp.sock"),
    );
    let path = write_document(&root, &body);
    let mut binding = Binding::open(&path).unwrap_or_else(|error| panic!("open: {error:?}"));
    assert_eq!(binding.mode(), DeploymentMode::Full);
    assert_eq!(binding.deadline(), Duration::from_millis(5_000));
    let declared = binding
        .listener()
        .unwrap_or_else(|| panic!("listener absent"))
        .clone();
    assert_eq!(declared.endpoint, PathBuf::from("/run/layerx/mcp.sock"));
    assert_eq!(declared.mode, 0o660);
    binding.restrict_to_read_only();
    assert_eq!(binding.mode(), DeploymentMode::ReadOnly);

    let parsed: Value =
        serde_json::from_str(&body).unwrap_or_else(|error| panic!("reparse: {error}"));
    let mutate = |change: &dyn Fn(&mut serde_json::Map<String, Value>)| -> BindingError {
        let mut copy = parsed.clone();
        if let Some(fields) = copy.as_object_mut() {
            change(fields);
        }
        let text = serde_json::to_string(&copy).unwrap_or_else(|error| panic!("encode: {error}"));
        Binding::parse(&text).err().unwrap_or_else(|| {
            panic!("a mutated binding document was accepted");
        })
    };
    let changes: [Mutation; 6] = [
        &|fields: &mut serde_json::Map<String, Value>| {
            fields.insert("unexpected".to_owned(), json!("1"));
        },
        &|fields: &mut serde_json::Map<String, Value>| {
            fields.remove("capability_id");
        },
        &|fields: &mut serde_json::Map<String, Value>| {
            fields.insert("mode".to_owned(), json!("permissive"));
        },
        &|fields: &mut serde_json::Map<String, Value>| {
            fields.insert("deadline_ms".to_owned(), json!(0));
        },
        &|fields: &mut serde_json::Map<String, Value>| {
            fields.insert("store".to_owned(), json!("relative/store"));
        },
        &|fields: &mut serde_json::Map<String, Value>| {
            fields.insert("session_id".to_owned(), json!("07"));
        },
    ];
    for change in changes {
        let refusal = mutate(change);
        assert!(matches!(refusal, BindingError::Malformed(_)));
        assert!(!refusal.detail().is_empty());
    }
    assert!(Binding::parse("[]").is_err());
    assert!(Binding::open(Path::new("relative.json")).is_err());
    let _ = fs::remove_dir_all(root);
}

fn assert_daemon_handshake(initialize: &Value, listed: &Value) {
    assert_eq!(
        initialize
            .pointer("/result/_meta/layerx~1binding")
            .and_then(Value::as_str),
        Some("agent-daemon")
    );
    assert_eq!(
        initialize
            .pointer("/result/_meta/layerx~1deployment_mode")
            .and_then(Value::as_str),
        Some("full")
    );
    let reads = initialize
        .pointer("/result/_meta/layerx~1read_tools")
        .and_then(Value::as_u64)
        .unwrap_or_default();
    let writes = initialize
        .pointer("/result/_meta/layerx~1write_tools")
        .and_then(Value::as_u64)
        .unwrap_or_default();
    assert_eq!(reads.saturating_add(writes), 21);
    let tools = listed
        .pointer("/result/tools")
        .and_then(Value::as_array)
        .unwrap_or_else(|| panic!("tools absent"));
    assert_eq!(tools.len(), 21);
}

fn assert_daemon_read(read: &Value, program: &str) {
    assert_eq!(read.pointer("/id").and_then(Value::as_u64), Some(3));
    assert_eq!(
        read.pointer("/result/isError").and_then(Value::as_bool),
        Some(false)
    );
    assert_eq!(
        read.pointer("/result/structuredContent/result/program")
            .and_then(Value::as_str),
        Some(program)
    );
    assert_eq!(
        read.pointer("/result/structuredContent/result/lifecycle")
            .and_then(Value::as_str),
        Some("active")
    );
    assert_eq!(
        read.pointer("/result/structuredContent/result/freshness/observed_sequence")
            .and_then(Value::as_u64),
        Some(OBSERVED_SEQUENCE)
    );
    assert_eq!(
        read.pointer("/result/structuredContent/result/balances/0/amount")
            .and_then(Value::as_str),
        Some("7")
    );
}

fn assert_refused_shapes(responses: &[Value]) {
    let refused_arguments = &responses[0];
    assert_eq!(
        refused_arguments
            .pointer("/result/isError")
            .and_then(Value::as_bool),
        Some(true)
    );
    assert_eq!(
        refused_arguments
            .pointer("/result/structuredContent/stage")
            .and_then(Value::as_str),
        Some("arguments")
    );
    let unserved = &responses[1];
    assert_eq!(
        unserved.pointer("/result/isError").and_then(Value::as_bool),
        Some(true)
    );
    assert_eq!(
        unserved
            .pointer("/result/structuredContent/stage")
            .and_then(Value::as_str),
        Some("daemon")
    );
    assert_eq!(
        unserved
            .pointer("/result/structuredContent/state")
            .and_then(Value::as_str),
        Some("refused")
    );
    assert_eq!(
        responses[2].pointer("/error/code").and_then(Value::as_i64),
        Some(-32602)
    );
}

#[test]
fn the_stdio_transport_serves_the_daemon_catalogue_without_a_seed_or_a_gateway() {
    let root = directory("stdio");
    let endpoint = agent_daemon("b".repeat(48));
    let body = document(&root, &endpoint, "full", None);
    let path = write_document(&root, &body);
    let binding = Binding::open(&path).unwrap_or_else(|error| panic!("open: {error:?}"));
    let mut session = binding
        .open_session()
        .unwrap_or_else(|error| panic!("open session: {}", error.detail()));
    let program = "cc".repeat(32);
    let responses = exchange(
        &mut session,
        &[
            json!({"jsonrpc": "2.0", "id": 1, "method": "initialize"}),
            json!({"jsonrpc": "2.0", "id": 2, "method": "tools/list"}),
            json!({"jsonrpc": "2.0", "method": "notifications/initialized"}),
            json!({
                "jsonrpc": "2.0",
                "id": 3,
                "method": "tools/call",
                "params": {"name": "balance.get", "arguments": {"program": program}},
            }),
            json!({
                "jsonrpc": "2.0",
                "id": 4,
                "method": "tools/call",
                "params": {"name": "balance.get", "arguments": {"program": program, "nope": "1"}},
            }),
            json!({
                "jsonrpc": "2.0",
                "id": 5,
                "method": "tools/call",
                "params": {"name": "history.list", "arguments": {"account": "11".repeat(32), "limit": "8"}},
            }),
            json!({
                "jsonrpc": "2.0",
                "id": 6,
                "method": "tools/call",
                "params": {"name": "gateway.post", "arguments": {}},
            }),
        ],
    );
    assert_eq!(responses.len(), 6);
    assert_daemon_handshake(&responses[0], &responses[1]);
    assert_daemon_read(&responses[2], &program);
    assert_refused_shapes(&responses[3..]);
    let _ = fs::remove_dir_all(root);
}

#[test]
fn a_read_only_binding_never_exposes_a_write_tool() {
    let root = directory("readonly");
    let endpoint = agent_daemon("b".repeat(48));
    let body = document(&root, &endpoint, "read-only", None);
    let path = write_document(&root, &body);
    let binding = Binding::open(&path).unwrap_or_else(|error| panic!("open: {error:?}"));
    assert_eq!(binding.mode(), DeploymentMode::ReadOnly);
    let mut session = binding
        .open_session()
        .unwrap_or_else(|error| panic!("open session: {}", error.detail()));
    let responses = exchange(
        &mut session,
        &[
            json!({"jsonrpc": "2.0", "id": 1, "method": "tools/list"}),
            json!({
                "jsonrpc": "2.0",
                "id": 2,
                "method": "tools/call",
                "params": {
                    "name": "wallet.send",
                    "arguments": {
                        "destination": "11".repeat(32),
                        "asset": "22".repeat(32),
                        "amount": "1",
                        "idempotency_key": "33".repeat(32),
                    },
                },
            }),
        ],
    );
    let listed = responses[0]
        .pointer("/result/tools")
        .and_then(Value::as_array)
        .unwrap_or_else(|| panic!("tools absent"));
    assert_eq!(
        listed.len(),
        served()
            .iter()
            .filter(|tool| tool.kind == ToolKind::Read)
            .count()
    );
    assert!(listed.iter().all(|tool| {
        tool.pointer("/annotations/readOnlyHint")
            .and_then(Value::as_bool)
            == Some(true)
    }));
    assert_eq!(
        responses[1].pointer("/error/code").and_then(Value::as_i64),
        Some(-32602)
    );
    let _ = fs::remove_dir_all(root);
}

#[test]
fn the_protocol_socket_refuses_every_unprotected_endpoint_and_serves_an_admitted_peer() {
    let root = directory("listener");
    let endpoint = agent_daemon("b".repeat(48));
    let socket = root.join("mcp.sock");
    let body = document(
        &root,
        &endpoint,
        "full",
        Some(&socket.display().to_string()),
    );
    let path = write_document(&root, &body);
    let binding = Binding::open(&path).unwrap_or_else(|error| panic!("open: {error:?}"));
    let configuration = binding
        .listener()
        .cloned()
        .unwrap_or_else(|| panic!("listener absent"));

    let relative = ListenerConfig {
        endpoint: PathBuf::from("mcp.sock"),
        ..configuration.clone()
    };
    assert_eq!(
        Listener::bind(relative).err(),
        Some(ListenerError::RelativeEndpoint)
    );
    let broad = ListenerConfig {
        mode: 0o666,
        ..configuration.clone()
    };
    assert_eq!(
        Listener::bind(broad).err(),
        Some(ListenerError::ModeTooBroad)
    );
    let unpeered = ListenerConfig {
        admitted_uids: Vec::new(),
        ..configuration.clone()
    };
    assert_eq!(
        Listener::bind(unpeered).err(),
        Some(ListenerError::NoAdmittedPeer)
    );
    let immediate = ListenerConfig {
        deadline: Duration::from_millis(0),
        ..configuration.clone()
    };
    assert_eq!(
        Listener::bind(immediate).err(),
        Some(ListenerError::ZeroDeadline)
    );
    let open_parent = root.join("open");
    fs::create_dir_all(&open_parent).unwrap_or_else(|error| panic!("open parent: {error}"));
    fs::set_permissions(&open_parent, fs::Permissions::from_mode(0o777))
        .unwrap_or_else(|error| panic!("open parent mode: {error}"));
    let world = ListenerConfig {
        endpoint: open_parent.join("mcp.sock"),
        ..configuration.clone()
    };
    assert_eq!(
        Listener::bind(world).err(),
        Some(ListenerError::ParentUnowned)
    );
    let occupied = root.join("occupied.sock");
    fs::write(&occupied, b"").unwrap_or_else(|error| panic!("occupied: {error}"));
    let taken = ListenerConfig {
        endpoint: occupied,
        ..configuration.clone()
    };
    assert_eq!(
        Listener::bind(taken).err(),
        Some(ListenerError::EndpointExists)
    );

    let listener = Listener::bind(configuration).unwrap_or_else(|error| {
        panic!("bind: {}", error.detail());
    });
    let mut session = binding
        .open_session()
        .unwrap_or_else(|error| panic!("open session: {}", error.detail()));
    thread::spawn(move || {
        let _ = listener.serve(&mut session);
    });
    let mut client = connect(&socket);
    client
        .write_all(b"{\"jsonrpc\":\"2.0\",\"id\":1,\"method\":\"tools/list\"}\n")
        .and_then(|()| client.flush())
        .unwrap_or_else(|error| panic!("client write: {error}"));
    client
        .shutdown(std::net::Shutdown::Write)
        .unwrap_or_else(|error| panic!("client shutdown: {error}"));
    let mut answer = String::new();
    client
        .read_to_string(&mut answer)
        .unwrap_or_else(|error| panic!("client read: {error}"));
    let response: Value = serde_json::from_str(answer.trim())
        .unwrap_or_else(|error| panic!("client response {answer}: {error}"));
    assert_eq!(
        response
            .pointer("/result/tools")
            .and_then(Value::as_array)
            .map(Vec::len),
        Some(21)
    );
    let _ = fs::remove_dir_all(root);
}

fn mode_of(path: &Path) -> u32 {
    fs::symlink_metadata(path)
        .unwrap_or_else(|error| panic!("metadata {}: {error}", path.display()))
        .mode()
        & 0o777
}

fn assert_published_binding(
    root: &Path,
    bearer: &str,
    issued: &enrolment::PublishedBinding,
) -> String {
    assert!(issued.created);
    assert_eq!(issued.binding, root.join("mcp").join("binding.json"));
    assert_eq!(issued.session_id, SessionId([0x0c; 32]));
    assert_eq!(mode_of(&root.join("mcp")), 0o700);
    assert_eq!(mode_of(&issued.binding), 0o600);
    assert_eq!(mode_of(&issued.session_token_file), 0o600);
    assert_eq!(mode_of(&issued.daemon_bearer_file), 0o600);
    let body =
        fs::read_to_string(&issued.binding).unwrap_or_else(|error| panic!("binding body: {error}"));
    assert!(!body.contains(bearer));
    let written: Value =
        serde_json::from_str(&body).unwrap_or_else(|error| panic!("binding json: {error}"));
    assert_eq!(
        written
            .pointer("/session_token_file")
            .and_then(Value::as_str),
        issued.session_token_file.to_str()
    );
    assert_eq!(
        written
            .pointer("/agent/bearer_file")
            .and_then(Value::as_str),
        issued.daemon_bearer_file.to_str()
    );
    assert_eq!(
        written.pointer("/session_id").and_then(Value::as_str),
        Some("0c".repeat(32).as_str())
    );

    body
}

fn assert_served_binding(root: &Path, issued: &enrolment::PublishedBinding, endpoint: &str) {
    let binding = Binding::open(&issued.binding).unwrap_or_else(|error| panic!("open: {error:?}"));
    assert_eq!(binding.mode(), DeploymentMode::Full);
    assert_eq!(binding.tenant(), "tenant-a");
    assert_eq!(binding.store(), root.join("store").as_path());
    assert_eq!(binding.session_generation(), issued.session_generation);
    assert_eq!(binding.agent_endpoint(), endpoint);
    let mut session = binding
        .open_session()
        .unwrap_or_else(|error| panic!("open session: {}", error.detail()));
    let program = "cc".repeat(32);
    let responses = exchange(
        &mut session,
        &[
            json!({"jsonrpc": "2.0", "id": 1, "method": "initialize"}),
            json!({"jsonrpc": "2.0", "id": 2, "method": "tools/list"}),
            json!({"jsonrpc": "2.0", "method": "notifications/initialized"}),
            json!({
                "jsonrpc": "2.0",
                "id": 3,
                "method": "tools/call",
                "params": {"name": "balance.get", "arguments": {"program": program}},
            }),
        ],
    );
    assert_eq!(responses.len(), 3);
    assert_daemon_handshake(&responses[0], &responses[1]);
    assert_daemon_read(&responses[2], &program);
}

#[test]
fn daemon_enrolment_writes_the_binding_the_served_path_opens() {
    let root = directory("enrolment");
    let bearer = "d".repeat(48);
    let endpoint = agent_daemon(bearer.clone());
    let mut store =
        Store::open(root.join("store")).unwrap_or_else(|error| panic!("store: {error}"));
    let (capability, identity) = daemon_records(&mut store);
    let surface = DaemonSurface::new(&endpoint, bearer.clone(), [0xcc; 32])
        .unwrap_or_else(|error| panic!("surface: {error}"));
    let publisher = BindingPublisher::new(
        root.join("mcp"),
        root.join("store"),
        root.join("audit"),
        surface,
        LimitConfig {
            id: LimitId([0x0a; 16]),
            name: "mcp".to_owned(),
            scope: LimitScope::Tenant([1; 32]),
            ceiling: 1_000,
            consumed: 0,
        },
        Duration::from_millis(5_000),
        BindingMode::Full,
    )
    .unwrap_or_else(|error| panic!("publisher: {error}"));
    let mut sessions = SessionRegistry::default();
    let request = EnrolmentRequest {
        session_id: SessionId([0x0c; 32]),
        capability_id: capability.id,
        permitted_activity_types: BTreeSet::from([7]),
        scopes: served()
            .iter()
            .map(|tool| tool.required_scope.to_owned())
            .collect(),
        expiry_sequence: 300,
        opening_client: "mcp".to_owned(),
        policy_version: "policy-v1".to_owned(),
        core_sequence: 50,
    };
    let issued = enrolment::enrol(
        &mut store,
        &mut sessions,
        &identity,
        request.clone(),
        &publisher,
    )
    .unwrap_or_else(|error| panic!("enrol: {error}"));
    let body = assert_published_binding(&root, &bearer, &issued);

    let repeated = enrolment::enrol(
        &mut store,
        &mut sessions,
        &identity,
        request.clone(),
        &publisher,
    );
    assert!(matches!(repeated, Err(EnrolmentError::Session(_))));
    let other = EnrolmentRequest {
        session_id: SessionId([0x0d; 32]),
        ..request
    };
    let displaced = enrolment::enrol(&mut store, &mut sessions, &identity, other, &publisher);
    assert!(
        matches!(displaced, Err(EnrolmentError::AlreadyPublished(ref path)) if *path == issued.binding)
    );
    assert_eq!(
        sessions
            .get(identity.tenant(), SessionId([0x0d; 32]))
            .map(|record| record.open),
        Some(false)
    );
    assert_eq!(
        fs::read_to_string(&issued.binding).unwrap_or_else(|error| panic!("binding body: {error}")),
        body
    );
    drop(store);

    assert_served_binding(&root, &issued, &endpoint);
    let _ = fs::remove_dir_all(root);
}

fn connect(socket: &Path) -> UnixStream {
    for _ in 0..200 {
        if let Ok(stream) = UnixStream::connect(socket) {
            stream
                .set_read_timeout(Some(Duration::from_secs(10)))
                .unwrap_or_else(|error| panic!("client timeout: {error}"));
            return stream;
        }
        thread::sleep(Duration::from_millis(25));
    }
    panic!("the protocol socket never accepted an admitted peer");
}
