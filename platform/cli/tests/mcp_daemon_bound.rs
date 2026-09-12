use std::collections::BTreeSet;
use std::fs;
use std::io::{Read as _, Write as _};
use std::net::TcpListener;
use std::os::unix::fs::PermissionsExt as _;
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};
use std::sync::atomic::{AtomicU32, Ordering};
use std::thread;
use std::time::Duration;

use layerx_agentd::budget::{LimitConfig, LimitId, LimitScope};
use layerx_agentd::capability::{Capability, CapabilityDimensions, CapabilityId, RateCeiling};
use layerx_agentd::enrolment::{
    self, BindingMode, BindingPublisher, DaemonSurface, EnrolmentRequest, PublishedBinding,
};
use layerx_agentd::identity::{
    register, CoreIdentity, IdentityError, IdentityResolver, ProtocolAuthority,
};
use layerx_agentd::session::{SessionId, SessionRegistry};
use layerx_agentd::store::{Store, TenantId};
use layerx_mcp::server::{catalogue, ToolKind};
use layerx_types::ids::Did;
use layerx_types::verify::VerificationLevel;
use serde_json::Value;

static SEQUENCE: AtomicU32 = AtomicU32::new(0);
const OBSERVED_SEQUENCE: u64 = 120;

struct BoundaryIdentity(CoreIdentity);

impl IdentityResolver for BoundaryIdentity {
    fn resolve(&mut self, _did: &Did) -> Result<Option<CoreIdentity>, IdentityError> {
        Ok(Some(self.0.clone()))
    }
}

/// Creates one canonical, operator-private root for a single installation journey.
fn isolated(label: &str) -> PathBuf {
    let sequence = SEQUENCE.fetch_add(1, Ordering::Relaxed);
    let base = std::env::temp_dir();
    let canonical = fs::canonicalize(&base).unwrap_or(base);
    let root = canonical.join(format!(
        "layerx-install-daemon-{label}-{}-{sequence}",
        std::process::id()
    ));
    fs::create_dir_all(&root).unwrap_or_else(|error| panic!("root {label}: {error}"));
    fs::set_permissions(&root, fs::Permissions::from_mode(0o700))
        .unwrap_or_else(|error| panic!("root mode {label}: {error}"));
    root
}

/// Enrols the agent daemon exactly as a running daemon does: a real capability, a real identity
/// carrying its grant, a real session opened under that grant, and the binding document the
/// served path opens, written beside the CLI configuration.
fn enrol(root: &Path, endpoint: &str, bearer: &str) -> PublishedBinding {
    let mut store =
        Store::open(root.join("store")).unwrap_or_else(|error| panic!("store: {error}"));
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
        .persist(&mut store)
        .unwrap_or_else(|error| panic!("capability persist: {error:?}"));
    let mut boundary = BoundaryIdentity(CoreIdentity {
        canonical_bytes: b"model-identity".to_vec(),
        head_sequence: 10,
        revocation_sequence: 1,
        verification_level: VerificationLevel::STATE_PROVEN,
        frozen: false,
        authorities: vec![ProtocolAuthority::CapabilityGrant(capability.id.0)],
    });
    let identity = register(
        &mut store,
        tenant,
        Did::new(b"did:layerx:model").unwrap_or_else(|error| panic!("DID: {error:?}")),
        &mut boundary,
    )
    .unwrap_or_else(|error| panic!("identity: {error:?}"));
    let surface = DaemonSurface::new(endpoint, bearer.to_owned(), [0xcc; 32])
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
        scopes: catalogue()
            .iter()
            .map(|tool| tool.required_scope.to_owned())
            .collect(),
        expiry_sequence: 300,
        opening_client: "mcp".to_owned(),
        policy_version: "policy-v1".to_owned(),
        core_sequence: 50,
    };
    enrolment::enrol(&mut store, &mut sessions, &identity, request, &publisher)
        .unwrap_or_else(|error| panic!("enrol: {error}"))
}

fn layerx(root: &Path) -> Command {
    let mut command = Command::new(env!("CARGO_BIN_EXE_layerx"));
    command
        .env("LAYERX_CONFIG", root.join("config.json"))
        .env("LAYERX_INSTALL_ROOT", root)
        .env_remove("LAYERX_CREDENTIAL_STORE")
        .env_remove("LAYERX_GATEWAY_KEY_ID");
    command
}

fn install(root: &Path, arguments: &[&str]) -> Output {
    layerx(root)
        .args(["--json", "install", "mcp"])
        .args(arguments)
        .output()
        .unwrap_or_else(|error| panic!("real layerx executable should start: {error}"))
}

fn success(output: &Output) -> Value {
    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    serde_json::from_slice(&output.stdout).unwrap_or_else(|error| panic!("stdout json: {error}"))
}

/// Answers the daemon's verified balance read for any authorized loopback caller.
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

fn strings(value: Option<&Value>) -> Vec<String> {
    value
        .and_then(Value::as_array)
        .unwrap_or_else(|| panic!("string array absent"))
        .iter()
        .map(|item| {
            item.as_str()
                .unwrap_or_else(|| panic!("string expected"))
                .to_owned()
        })
        .collect()
}

#[test]
fn a_fresh_install_serves_the_catalogue_through_the_daemon_without_a_gateway_key() {
    let root = isolated("journey");
    let bearer = "d".repeat(48);
    let endpoint = agent_daemon(bearer.clone());
    let published = enrol(&root, &endpoint, &bearer);
    let binding_path = root.join("mcp").join("binding.json");
    assert_eq!(published.binding, binding_path);

    let first = success(&install(&root, &["--host", "layerx"]));
    assert_eq!(first.pointer("/ok").and_then(Value::as_bool), Some(true));
    assert_eq!(
        first.pointer("/kind").and_then(Value::as_str),
        Some("install.mcp")
    );
    let data = first
        .pointer("/data")
        .unwrap_or_else(|| panic!("data absent"));
    assert_eq!(
        data.pointer("/authorization").and_then(Value::as_str),
        Some("agent-daemon")
    );
    assert_eq!(
        data.pointer("/deployment_mode").and_then(Value::as_str),
        Some("full")
    );
    assert_eq!(
        data.pointer("/daemon_binding/path").and_then(Value::as_str),
        binding_path.to_str()
    );
    assert_eq!(
        data.pointer("/daemon_binding/tenant")
            .and_then(Value::as_str),
        Some("tenant-a")
    );
    assert_eq!(
        data.pointer("/daemon_binding/agent_endpoint")
            .and_then(Value::as_str),
        Some(endpoint.as_str())
    );
    assert_eq!(
        data.pointer("/daemon_binding/session_generation")
            .and_then(Value::as_u64),
        Some(published.session_generation)
    );
    assert_eq!(
        strings(data.pointer("/server/args")),
        vec![
            "mcp".to_owned(),
            "serve".to_owned(),
            "--daemon-binding".to_owned(),
            binding_path.display().to_string(),
        ]
    );
    assert_eq!(
        data.pointer("/server/env")
            .and_then(Value::as_object)
            .map(serde_json::Map::len),
        Some(0)
    );
    assert!(data.get("credentials").is_none());
    assert!(data.get("environment").is_none());
    assert!(data.get("account_binding").is_none());
    assert_eq!(
        data.pointer("/tools")
            .and_then(Value::as_array)
            .map(Vec::len),
        Some(catalogue().len())
    );
    assert_eq!(
        data.pointer("/changed").and_then(Value::as_bool),
        Some(true)
    );
    let rendered = serde_json::to_string(&first).unwrap_or_else(|error| panic!("render: {error}"));
    assert!(!rendered.contains(&bearer));
    assert!(!rendered.contains("LAYERX_GATEWAY_KEY_ID"));

    let registration = fs::read_to_string(root.join("mcp.json"))
        .unwrap_or_else(|error| panic!("registration file: {error}"));
    assert!(!registration.contains(&bearer));
    let registered: Value =
        serde_json::from_str(&registration).unwrap_or_else(|error| panic!("mcp.json: {error}"));
    assert_eq!(
        strings(registered.pointer("/mcpServers/layerx/args")),
        strings(data.pointer("/server/args"))
    );
    assert_eq!(
        registered
            .pointer("/mcpServers/layerx/env")
            .and_then(Value::as_object)
            .map(serde_json::Map::len),
        Some(0)
    );

    let second = success(&install(&root, &["--host", "layerx"]));
    assert_eq!(
        second.pointer("/data/changed").and_then(Value::as_bool),
        Some(false)
    );

    let mut server = layerx(&root)
        .args(["mcp", "serve", "--daemon-binding"])
        .arg(&binding_path)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap_or_else(|error| panic!("installed server should start: {error}"));
    let program = "cc".repeat(32);
    {
        let mut stdin = server
            .stdin
            .take()
            .unwrap_or_else(|| panic!("server stdin absent"));
        let requests = [
            "{\"jsonrpc\":\"2.0\",\"id\":1,\"method\":\"initialize\"}".to_owned(),
            "{\"jsonrpc\":\"2.0\",\"id\":2,\"method\":\"tools/list\"}".to_owned(),
            "{\"jsonrpc\":\"2.0\",\"method\":\"notifications/initialized\"}".to_owned(),
            format!(
                "{{\"jsonrpc\":\"2.0\",\"id\":3,\"method\":\"tools/call\",\"params\":{{\"name\":\"balance.get\",\"arguments\":{{\"program\":\"{program}\"}}}}}}"
            ),
        ];
        for request in requests {
            writeln!(stdin, "{request}").unwrap_or_else(|error| panic!("request: {error}"));
        }
    }
    let output = server
        .wait_with_output()
        .unwrap_or_else(|error| panic!("server exit: {error}"));
    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let responses = String::from_utf8_lossy(&output.stdout)
        .lines()
        .map(|line| {
            serde_json::from_str::<Value>(line).unwrap_or_else(|error| panic!("response: {error}"))
        })
        .collect::<Vec<Value>>();
    assert_eq!(responses.len(), 3);
    assert_eq!(
        responses[0]
            .pointer("/result/_meta/layerx~1binding")
            .and_then(Value::as_str),
        Some("agent-daemon")
    );
    assert_eq!(
        responses[0]
            .pointer("/result/_meta/layerx~1deployment_mode")
            .and_then(Value::as_str),
        Some("full")
    );
    assert_eq!(
        responses[1]
            .pointer("/result/tools")
            .and_then(Value::as_array)
            .map(Vec::len),
        Some(catalogue().len())
    );
    assert_eq!(
        responses[2]
            .pointer("/result/isError")
            .and_then(Value::as_bool),
        Some(false)
    );
    assert_eq!(
        responses[2]
            .pointer("/result/structuredContent/result/program")
            .and_then(Value::as_str),
        Some(program.as_str())
    );
    assert_eq!(
        responses[2]
            .pointer("/result/structuredContent/result/freshness/observed_sequence")
            .and_then(Value::as_u64),
        Some(OBSERVED_SEQUENCE)
    );
    let _ = fs::remove_dir_all(root);
}

#[test]
fn an_explicit_binding_path_narrows_the_installed_surface_to_read_only() {
    let root = isolated("read-only");
    let bearer = "e".repeat(48);
    let endpoint = agent_daemon(bearer.clone());
    let published = enrol(&root, &endpoint, &bearer);
    let relocated = root.join("elsewhere");
    fs::create_dir_all(&relocated).unwrap_or_else(|error| panic!("relocated: {error}"));
    let binding_path = relocated.join("binding.json");
    fs::copy(&published.binding, &binding_path).unwrap_or_else(|error| panic!("copy: {error}"));
    let installed = success(&install(
        &root,
        &[
            "--host",
            "layerx",
            "--read-only",
            "--daemon-binding",
            binding_path
                .to_str()
                .unwrap_or_else(|| panic!("binding path is UTF-8")),
        ],
    ));
    let data = installed
        .pointer("/data")
        .unwrap_or_else(|| panic!("data absent"));
    assert_eq!(
        data.pointer("/deployment_mode").and_then(Value::as_str),
        Some("read-only")
    );
    assert_eq!(
        data.pointer("/daemon_binding/declared_mode")
            .and_then(Value::as_str),
        Some("full")
    );
    assert_eq!(
        data.pointer("/daemon_binding/path").and_then(Value::as_str),
        binding_path.to_str()
    );
    let arguments = strings(data.pointer("/server/args"));
    assert_eq!(arguments.last().map(String::as_str), Some("--read-only"));
    assert_eq!(arguments.get(3).map(String::as_str), binding_path.to_str());
    let tools = data
        .pointer("/tools")
        .and_then(Value::as_array)
        .unwrap_or_else(|| panic!("tools absent"));
    assert_eq!(
        tools.len(),
        catalogue()
            .iter()
            .filter(|tool| tool.kind == ToolKind::Read)
            .count()
    );
    let _ = fs::remove_dir_all(root);
}

#[test]
fn installation_refuses_before_any_registration_when_the_daemon_has_not_enrolled() {
    let root = isolated("unenrolled");
    let output = install(&root, &["--host", "layerx"]);
    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr).into_owned();
    let expected = root.join("mcp").join("binding.json").display().to_string();
    assert!(stderr.contains(&expected), "stderr: {stderr}");
    assert!(
        stderr.contains("agent-daemon enrolment"),
        "stderr: {stderr}"
    );
    assert!(!root.join("mcp.json").exists());
    let _ = fs::remove_dir_all(root);
}
