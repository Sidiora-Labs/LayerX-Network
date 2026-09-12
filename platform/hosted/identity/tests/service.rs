use native_tls::{Certificate, TlsConnector};
use std::collections::BTreeMap;
use std::fmt::Write as _;
use std::fs;
use std::io::{BufRead, BufReader, Read, Write};
use std::net::TcpStream;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicU64, Ordering};
use std::thread;
use std::time::Duration;

const SERVICES: [&str; 9] = [
    "gateway",
    "registry",
    "webhooks",
    "dashboard",
    "faucet",
    "testnet",
    "ramp",
    "provisioning",
    "registrar",
];
const INTROSPECTING_SERVICES: [&str; 6] = [
    "gateway",
    "webhooks",
    "dashboard",
    "faucet",
    "testnet",
    "ramp",
];
const REGISTRAR_SUB: &str = "did:key:z6mkregistrar-principal";
const REGISTRAR_ACCOUNT: &str = "agent:did:key:z6mkregistrar-principal:main";
const SERVICE_NOT_PERMITTED: &str =
    "{\"error\":{\"code\":\"service_not_permitted\",\"retry\":\"never\"}}";
const SIGNER_KEY: &str = "1f2e3d4c5b6a79880123456789abcdef1f2e3d4c5b6a79880123456789abcdef";
const OTHER_SIGNER_KEY: &str = "0a1b2c3d4e5f60718293a4b5c6d7e8f90a1b2c3d4e5f60718293a4b5c6d7e8f9";
const SUB: &str = "did:key:z6mkbeta-principal_1";
const ALPHA_SUB: &str = "did:key:z6mkalpha-principal";
const BRAVO_SUB: &str = "did:key:z6mkbravo-principal";
const ACCOUNT: &str = "agent:did:key:z6mkbeta-principal_1:main";

struct Fixture {
    root: PathBuf,
    ca_der: Vec<u8>,
}

struct Server {
    child: Child,
    port: u16,
    state_dir: PathBuf,
}

struct Reply {
    status: u16,
    headers: BTreeMap<String, String>,
    body: String,
}

impl Drop for Server {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

fn openssl(args: &[&str]) {
    let output = Command::new("openssl")
        .args(args)
        .output()
        .unwrap_or_else(|error| panic!("openssl {}: {error}", args.join(" ")));
    assert!(
        output.status.success(),
        "openssl {} failed: {}",
        args.join(" "),
        String::from_utf8_lossy(&output.stderr)
    );
}

fn fixture(name: &str) -> Fixture {
    static NEXT: AtomicU64 = AtomicU64::new(0);
    let root = Path::new(env!("CARGO_TARGET_TMPDIR")).join(format!(
        "identity-{name}-{}-{}",
        std::process::id(),
        NEXT.fetch_add(1, Ordering::Relaxed)
    ));
    let _ = fs::remove_dir_all(&root);
    fs::create_dir_all(&root).unwrap_or_else(|error| panic!("fixture root: {error}"));
    issue_certificates(&root);
    let tokens = root.join("tokens");
    fs::create_dir_all(&tokens).unwrap_or_else(|error| panic!("tokens: {error}"));
    for service in SERVICES {
        fs::write(tokens.join(service), format!("{}\n", token_for(service)))
            .unwrap_or_else(|error| panic!("token: {error}"));
    }
    fs::write(root.join("store.key"), "beta-store-key-0123456789abcdef\n")
        .unwrap_or_else(|error| panic!("store key: {error}"));
    let ca_der = fs::read(root.join("ca.der")).unwrap_or_else(|error| panic!("ca der: {error}"));
    Fixture { root, ca_der }
}

fn issue_certificates(root: &Path) {
    let ca_key = root.join("ca.key");
    let ca_crt = root.join("ca.crt");
    let ca_der = root.join("ca.der");
    openssl(&[
        "genpkey",
        "-algorithm",
        "EC",
        "-pkeyopt",
        "ec_paramgen_curve:P-256",
        "-out",
        &ca_key.to_string_lossy(),
    ]);
    openssl(&[
        "req",
        "-x509",
        "-new",
        "-key",
        &ca_key.to_string_lossy(),
        "-days",
        "2",
        "-sha256",
        "-subj",
        "/O=LayerX beta/CN=LayerX beta internal CA",
        "-addext",
        "basicConstraints=critical,CA:TRUE,pathlen:0",
        "-addext",
        "keyUsage=critical,keyCertSign,cRLSign",
        "-out",
        &ca_crt.to_string_lossy(),
    ]);
    openssl(&[
        "x509",
        "-in",
        &ca_crt.to_string_lossy(),
        "-outform",
        "DER",
        "-out",
        &ca_der.to_string_lossy(),
    ]);
    issue_server_certificate(root, &ca_key, &ca_crt);
}

fn issue_server_certificate(root: &Path, ca_key: &Path, ca_crt: &Path) {
    let key = root.join("server.key");
    let csr = root.join("server.csr");
    let crt = root.join("server.crt");
    let ext = root.join("server.ext");
    openssl(&[
        "genpkey",
        "-algorithm",
        "EC",
        "-pkeyopt",
        "ec_paramgen_curve:P-256",
        "-out",
        &key.to_string_lossy(),
    ]);
    openssl(&[
        "req",
        "-new",
        "-key",
        &key.to_string_lossy(),
        "-subj",
        "/O=LayerX beta/CN=layerx-identity",
        "-out",
        &csr.to_string_lossy(),
    ]);
    fs::write(
        &ext,
        "basicConstraints=CA:FALSE\nkeyUsage=critical,digitalSignature\nextendedKeyUsage=serverAuth\nsubjectAltName=DNS:localhost,IP:127.0.0.1\n",
    )
    .unwrap_or_else(|error| panic!("ext: {error}"));
    openssl(&[
        "x509",
        "-req",
        "-in",
        &csr.to_string_lossy(),
        "-CA",
        &ca_crt.to_string_lossy(),
        "-CAkey",
        &ca_key.to_string_lossy(),
        "-CAcreateserial",
        "-days",
        "2",
        "-sha256",
        "-extfile",
        &ext.to_string_lossy(),
        "-out",
        &crt.to_string_lossy(),
    ]);
    openssl(&[
        "x509",
        "-in",
        &crt.to_string_lossy(),
        "-outform",
        "DER",
        "-out",
        &root.join("server.crt.der").to_string_lossy(),
    ]);
    openssl(&[
        "pkcs8",
        "-topk8",
        "-nocrypt",
        "-in",
        &key.to_string_lossy(),
        "-outform",
        "DER",
        "-out",
        &root.join("server.key.der").to_string_lossy(),
    ]);
}

fn token_for(service: &str) -> String {
    format!("{service}-service-token-0123456789abcdef")
}

impl Fixture {
    fn command(&self, state_dir: &Path) -> Command {
        let mut command = Command::new(env!("CARGO_BIN_EXE_layerx-identity"));
        command
            .env_clear()
            .env("LAYERX_IDENTITY_LISTEN", "127.0.0.1:0")
            .env(
                "LAYERX_IDENTITY_TLS_CERT_DER",
                self.root.join("server.crt.der"),
            )
            .env(
                "LAYERX_IDENTITY_TLS_KEY_DER",
                self.root.join("server.key.der"),
            )
            .env("LAYERX_IDENTITY_STATE_DIR", state_dir)
            .env(
                "LAYERX_IDENTITY_SERVICE_TOKENS_DIR",
                self.root.join("tokens"),
            )
            .env(
                "LAYERX_IDENTITY_STORE_KEY_FILE",
                self.root.join("store.key"),
            )
            .env("LAYERX_IDENTITY_SESSION_TTL_SECONDS", "3600")
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::piped());
        command
    }

    fn boot_refusal(&self, state_dir: &Path) -> String {
        let output = self
            .command(state_dir)
            .output()
            .unwrap_or_else(|error| panic!("run: {error}"));
        assert_eq!(
            output.status.code(),
            Some(2),
            "layerx-identity must refuse to boot"
        );
        String::from_utf8_lossy(&output.stderr).into_owned()
    }

    fn spawn(&self, state_dir: &Path) -> Server {
        let mut child = self
            .command(state_dir)
            .spawn()
            .unwrap_or_else(|error| panic!("spawn: {error}"));
        let stderr = child.stderr.take().unwrap_or_else(|| panic!("stderr pipe"));
        let mut reader = BufReader::new(stderr);
        let mut line = String::new();
        let port = loop {
            line.clear();
            let count = reader
                .read_line(&mut line)
                .unwrap_or_else(|error| panic!("stderr: {error}"));
            assert!(count > 0, "layerx-identity exited before listening");
            if let Some(address) = line
                .trim()
                .strip_prefix("layerx-identity listening on ")
                .and_then(|rest| rest.strip_suffix(" with TLS"))
            {
                break address
                    .rsplit_once(':')
                    .and_then(|(_, port)| port.parse::<u16>().ok())
                    .unwrap_or_else(|| panic!("listen address {address}"));
            }
        };
        thread::spawn(move || {
            let mut sink = String::new();
            while reader.read_line(&mut sink).unwrap_or(0) > 0 {
                sink.clear();
            }
        });
        Server {
            child,
            port,
            state_dir: state_dir.to_path_buf(),
        }
    }

    fn request(
        &self,
        server: &Server,
        method: &str,
        path: &str,
        bearer: Option<&str>,
        body: Option<&str>,
    ) -> Reply {
        self.request_with_headers(server, method, path, bearer, body, &[])
    }

    fn request_with_headers(
        &self,
        server: &Server,
        method: &str,
        path: &str,
        bearer: Option<&str>,
        body: Option<&str>,
        extra: &[(&str, &str)],
    ) -> Reply {
        let ca = Certificate::from_der(&self.ca_der).unwrap_or_else(|error| panic!("ca: {error}"));
        let connector = TlsConnector::builder()
            .add_root_certificate(ca)
            .min_protocol_version(Some(native_tls::Protocol::Tlsv12))
            .build()
            .unwrap_or_else(|error| panic!("connector: {error}"));
        let tcp = TcpStream::connect(("127.0.0.1", server.port))
            .unwrap_or_else(|error| panic!("connect: {error}"));
        tcp.set_read_timeout(Some(Duration::from_secs(10)))
            .unwrap_or_else(|error| panic!("timeout: {error}"));
        let mut stream = connector
            .connect("localhost", tcp)
            .unwrap_or_else(|error| panic!("tls: {error}"));
        let body = body.unwrap_or_default();
        let mut head = format!("{method} {path} HTTP/1.1\r\nHost: localhost\r\n");
        if let Some(token) = bearer {
            let _ = write!(head, "Authorization: Bearer {token}\r\n");
        }
        if !body.is_empty() {
            head.push_str("Content-Type: application/json\r\n");
        }
        for (name, value) in extra {
            let _ = write!(head, "{name}: {value}\r\n");
        }
        let _ = write!(
            head,
            "Content-Length: {}\r\nConnection: close\r\n\r\n{body}",
            body.len()
        );
        stream
            .write_all(head.as_bytes())
            .unwrap_or_else(|error| panic!("write: {error}"));
        let mut raw = Vec::new();
        stream
            .read_to_end(&mut raw)
            .unwrap_or_else(|error| panic!("read: {error}"));
        parse_reply(&raw)
    }
}

fn parse_reply(raw: &[u8]) -> Reply {
    let split = raw
        .windows(4)
        .position(|window| window == b"\r\n\r\n")
        .unwrap_or_else(|| panic!("no header terminator in {}", String::from_utf8_lossy(raw)));
    let head = std::str::from_utf8(&raw[..split]).unwrap_or_else(|error| panic!("head: {error}"));
    let mut lines = head.split("\r\n");
    let status_line = lines.next().unwrap_or_default();
    let status = status_line
        .split_whitespace()
        .nth(1)
        .and_then(|value| value.parse::<u16>().ok())
        .unwrap_or_else(|| panic!("status line {status_line}"));
    let mut headers = BTreeMap::new();
    for line in lines {
        let (name, value) = line
            .split_once(':')
            .unwrap_or_else(|| panic!("header {line}"));
        let previous = headers.insert(name.to_ascii_lowercase(), value.trim().to_owned());
        assert!(previous.is_none(), "duplicate header {name}");
    }
    let length = headers
        .get("content-length")
        .and_then(|value| value.parse::<usize>().ok())
        .unwrap_or_else(|| panic!("content length"));
    let body = &raw[split + 4..];
    assert_eq!(body.len(), length, "body must match Content-Length");
    assert_eq!(
        headers.get("content-type").map(String::as_str),
        Some("application/json")
    );
    assert_eq!(
        headers.get("cache-control").map(String::as_str),
        Some("no-store")
    );
    assert_eq!(headers.get("connection").map(String::as_str), Some("close"));
    assert!(!headers.contains_key("transfer-encoding"));
    Reply {
        status,
        headers,
        body: String::from_utf8_lossy(body).into_owned(),
    }
}

fn json(reply: &Reply) -> serde_json::Value {
    serde_json::from_str(&reply.body).unwrap_or_else(|error| panic!("json {}: {error}", reply.body))
}

fn provision(fixture: &Fixture, server: &Server) -> (String, String, String) {
    let principal = fixture.request(
        server,
        "POST",
        "/v1/principals",
        Some(&token_for("provisioning")),
        Some(&format!(
            "{{\"tenant\":\"beta\",\"sub\":\"{SUB}\",\"allowed_signer_public_keys\":[\"{SIGNER_KEY}\"],\"account\":\"{ACCOUNT}\",\"audiences\":[\"ramp-reference\"]}}"
        )),
    );
    assert_eq!(principal.status, 200, "{}", principal.body);
    assert_eq!(
        principal.body,
        format!(
            "{{\"tenant\":\"beta\",\"sub\":\"{SUB}\",\"allowed_signer_public_keys\":[\"{SIGNER_KEY}\"],\"account\":\"{ACCOUNT}\",\"audiences\":[\"ramp-reference\"]}}"
        )
    );
    let session = fixture.request(
        server,
        "POST",
        "/v1/sessions",
        Some(&token_for("provisioning")),
        Some(&format!("{{\"sub\":\"{SUB}\"}}")),
    );
    assert_eq!(session.status, 200, "{}", session.body);
    let value = json(&session);
    let token = value["token"].as_str().unwrap_or_default().to_owned();
    let csrf = value["csrf_token"].as_str().unwrap_or_default().to_owned();
    let session_id = value["session_id"].as_str().unwrap_or_default().to_owned();
    assert_eq!(value["sub"], SUB);
    assert_eq!(value["tenant"], "beta");
    assert!(value["expires_at"].as_u64().unwrap_or_default() > 0);
    assert_eq!(session_id.len(), 32);
    assert_eq!(csrf.len(), 64);
    assert_eq!(token, format!("ses_{session_id}.{}", &token[37..]));
    assert_eq!(token.len(), 4 + 32 + 1 + 64);
    (session_id, token, csrf)
}

fn create_principal(fixture: &Fixture, server: &Server, body: &serde_json::Value) -> Reply {
    fixture.request(
        server,
        "POST",
        "/v1/principals",
        Some(&token_for("provisioning")),
        Some(&body.to_string()),
    )
}

fn create_session(fixture: &Fixture, server: &Server, body: &serde_json::Value) -> Reply {
    fixture.request(
        server,
        "POST",
        "/v1/sessions",
        Some(&token_for("provisioning")),
        Some(&body.to_string()),
    )
}

fn cross_tenant_claim() -> serde_json::Value {
    serde_json::json!({
        "tenant": "bravo",
        "sub": ALPHA_SUB,
        "allowed_signer_public_keys": [OTHER_SIGNER_KEY]
    })
}

fn provision_two_tenants(fixture: &Fixture, server: &Server) {
    for (tenant, sub, key) in [
        ("alpha", ALPHA_SUB, SIGNER_KEY),
        ("bravo", BRAVO_SUB, OTHER_SIGNER_KEY),
    ] {
        let reply = create_principal(
            fixture,
            server,
            &serde_json::json!({
                "tenant": tenant,
                "sub": sub,
                "allowed_signer_public_keys": [key],
                "account": format!("agent:{sub}:main"),
                "audiences": ["ramp-reference"]
            }),
        );
        assert_eq!(reply.status, 200, "{}", reply.body);
        assert_eq!(json(&reply)["tenant"], tenant);
        assert_eq!(json(&reply)["sub"], sub);
    }
}

fn assert_tenant_binding(fixture: &Fixture, server: &Server, token: &str, sub: &str, key: &str) {
    let gateway = introspect(fixture, server, "gateway", "/v1/sessions/introspect", token);
    assert_eq!(
        gateway.body,
        format!("{{\"active\":true,\"sub\":\"{sub}\",\"allowed_signer_public_keys\":[\"{key}\"]}}")
    );
    let ramp = introspect(fixture, server, "ramp", "/v1/introspect", token);
    assert_eq!(json(&ramp)["principal_id"], sub);
    assert_eq!(json(&ramp)["account"], format!("agent:{sub}:main"));
}

fn introspect(fixture: &Fixture, server: &Server, service: &str, path: &str, token: &str) -> Reply {
    let body = if service == "ramp" {
        format!("{{\"token\":\"{token}\",\"audience\":\"ramp-reference\"}}")
    } else {
        format!("{{\"token\":\"{token}\"}}")
    };
    let reply = fixture.request(server, "POST", path, Some(&token_for(service)), Some(&body));
    assert_eq!(reply.status, 200, "{service} {path}: {}", reply.body);
    reply
}

fn assert_shapes(fixture: &Fixture, server: &Server, token: &str, csrf: &str, expires_at: u64) {
    let gateway = introspect(fixture, server, "gateway", "/v1/sessions/introspect", token);
    assert_eq!(
        gateway.body,
        format!("{{\"active\":true,\"sub\":\"{SUB}\",\"allowed_signer_public_keys\":[\"{SIGNER_KEY}\"]}}")
    );
    for developer in ["webhooks", "dashboard"] {
        let reply = introspect(fixture, server, developer, "/v1/sessions/introspect", token);
        assert_eq!(
            reply.body,
            format!("{{\"active\":true,\"sub\":\"{SUB}\",\"csrf_token\":\"{csrf}\"}}")
        );
    }
    for subject in ["faucet", "testnet"] {
        let reply = introspect(fixture, server, subject, "/v1/introspect", token);
        assert_eq!(reply.body, format!("{{\"active\":true,\"sub\":\"{SUB}\"}}"));
    }
    let ramp = introspect(fixture, server, "ramp", "/v1/introspect", token);
    assert_eq!(
        ramp.body,
        format!(
            "{{\"active\":true,\"principal_id\":\"{SUB}\",\"account\":\"{ACCOUNT}\",\"audience\":\"ramp-reference\",\"expires_at\":{expires_at}}}"
        )
    );
}

fn assert_inactive(fixture: &Fixture, server: &Server, token: &str) {
    let gateway = introspect(fixture, server, "gateway", "/v1/sessions/introspect", token);
    assert_eq!(
        gateway.body,
        "{\"active\":false,\"sub\":\"\",\"allowed_signer_public_keys\":[]}"
    );
    for developer in ["webhooks", "dashboard"] {
        let reply = introspect(fixture, server, developer, "/v1/introspect", token);
        assert_eq!(
            reply.body,
            "{\"active\":false,\"sub\":\"\",\"csrf_token\":\"\"}"
        );
    }
    for subject in ["faucet", "testnet"] {
        let reply = introspect(fixture, server, subject, "/v1/sessions/introspect", token);
        assert_eq!(reply.body, "{\"active\":false,\"sub\":\"\"}");
    }
    let ramp = introspect(fixture, server, "ramp", "/v1/introspect", token);
    assert_eq!(
        ramp.body,
        "{\"active\":false,\"principal_id\":\"\",\"account\":\"\",\"audience\":\"ramp-reference\",\"expires_at\":0}"
    );
}

#[test]
fn health_routes_answer_without_a_service_token() {
    let fixture = fixture("health");
    let server = fixture.spawn(&fixture.root.join("state"));
    let live = fixture.request(&server, "GET", "/livez", None, None);
    assert_eq!(live.status, 200);
    assert_eq!(live.body, "{\"status\":\"live\",\"service\":\"identity\"}");
    let ready = fixture.request(&server, "GET", "/readyz", None, None);
    assert_eq!(ready.status, 200);
    assert_eq!(
        ready.body,
        "{\"status\":\"ready\",\"service\":\"identity\"}"
    );
    assert!(server.state_dir.join("ready.marker").exists());
    let missing = fixture.request(&server, "GET", "/v1/unknown", None, None);
    assert_eq!(missing.status, 404);
    assert_eq!(
        missing.body,
        "{\"error\":{\"code\":\"not_found\",\"retry\":\"never\"}}"
    );
    let forwarded = fixture.request_with_headers(
        &server,
        "POST",
        "/v1/introspect",
        Some(&token_for("faucet")),
        Some("{\"token\":\"x\"}"),
        &[("X-Forwarded-For", "10.0.0.1")],
    );
    assert_eq!(forwarded.status, 400);
    assert!(forwarded.body.contains("untrusted_identity_header"));
}

#[test]
fn readiness_fails_when_the_store_is_not_writable() {
    let fixture = fixture("readiness");
    let state = fixture.root.join("state");
    let server = fixture.spawn(&state);
    assert_eq!(
        fixture
            .request(&server, "GET", "/readyz", None, None)
            .status,
        200
    );
    fs::remove_dir_all(&state).unwrap_or_else(|error| panic!("remove state: {error}"));
    let broken = fixture.request(&server, "GET", "/readyz", None, None);
    assert_eq!(broken.status, 503);
    assert_eq!(
        broken.body,
        "{\"error\":{\"code\":\"store_unavailable\",\"retry\":\"after\",\"retry_after_seconds\":5}}"
    );
    assert_eq!(
        broken.headers.get("retry-after").map(String::as_str),
        Some("5")
    );
    fs::create_dir_all(&state).unwrap_or_else(|error| panic!("recreate state: {error}"));
    let replaced = fixture.request(&server, "GET", "/readyz", None, None);
    assert_eq!(replaced.status, 503);
    assert_eq!(replaced.body, broken.body);
    assert_eq!(
        replaced.headers.get("retry-after").map(String::as_str),
        Some("5")
    );
    drop(server);
    let restarted = fixture.spawn(&state);
    let recovered = fixture.request(&restarted, "GET", "/readyz", None, None);
    assert_eq!(recovered.status, 200);
    assert_eq!(
        recovered.body,
        "{\"status\":\"ready\",\"service\":\"identity\"}"
    );
    assert!(restarted.state_dir.join("ready.marker").exists());
}

#[test]
fn every_introspection_shape_matches_its_consumer() {
    let fixture = fixture("shapes");
    let server = fixture.spawn(&fixture.root.join("state"));
    let (_, token, csrf) = provision(&fixture, &server);
    let ramp = introspect(&fixture, &server, "ramp", "/v1/introspect", &token);
    let expires_at = json(&ramp)["expires_at"].as_u64().unwrap_or_default();
    assert!(expires_at > 0);
    assert_shapes(&fixture, &server, &token, &csrf, expires_at);
    let other_audience = fixture.request(
        &server,
        "POST",
        "/v1/introspect",
        Some(&token_for("ramp")),
        Some(&format!("{{\"token\":\"{token}\",\"audience\":\"other\"}}")),
    );
    assert_eq!(other_audience.status, 200);
    assert_eq!(
        other_audience.body,
        "{\"active\":false,\"principal_id\":\"\",\"account\":\"\",\"audience\":\"other\",\"expires_at\":0}"
    );
    let no_audience = fixture.request(
        &server,
        "POST",
        "/v1/introspect",
        Some(&token_for("ramp")),
        Some(&format!("{{\"token\":\"{token}\"}}")),
    );
    assert_eq!(no_audience.status, 400);
    assert!(no_audience.body.contains("audience_required"));
    let stray_audience = fixture.request(
        &server,
        "POST",
        "/v1/introspect",
        Some(&token_for("faucet")),
        Some(&format!("{{\"token\":\"{token}\",\"audience\":\"x\"}}")),
    );
    assert_eq!(stray_audience.status, 400);
    let unknown_field = fixture.request(
        &server,
        "POST",
        "/v1/introspect",
        Some(&token_for("faucet")),
        Some(&format!("{{\"token\":\"{token}\",\"extra\":1}}")),
    );
    assert_eq!(unknown_field.status, 400);
    let (session_id, secret) = token
        .trim_start_matches("ses_")
        .split_once('.')
        .unwrap_or_default();
    let mut wrong_secret = secret.to_owned();
    wrong_secret.replace_range(0..1, if secret.starts_with('0') { "1" } else { "0" });
    assert_inactive(
        &fixture,
        &server,
        &format!("ses_{session_id}.{wrong_secret}"),
    );
    assert_inactive(&fixture, &server, "ses_00000000000000000000000000000000.0000000000000000000000000000000000000000000000000000000000000000");
    assert_inactive(&fixture, &server, "not-a-session-token");
}

fn assert_read_services_cannot_provision(fixture: &Fixture, server: &Server, session_id: &str) {
    for service in INTROSPECTING_SERVICES {
        let principal = fixture.request(
            server,
            "POST",
            "/v1/principals",
            Some(&token_for(service)),
            Some(&format!(
                "{{\"tenant\":\"beta\",\"sub\":\"{SUB}\",\"allowed_signer_public_keys\":[]}}"
            )),
        );
        assert_eq!(
            principal.status, 403,
            "{service} must not provision principals"
        );
        let session = fixture.request(
            server,
            "POST",
            "/v1/sessions",
            Some(&token_for(service)),
            Some(&format!("{{\"sub\":\"{SUB}\"}}")),
        );
        assert_eq!(session.status, 403, "{service} must not mint sessions");
        let revoke = fixture.request(
            server,
            "DELETE",
            &format!("/v1/sessions/{session_id}"),
            Some(&token_for(service)),
            None,
        );
        assert_eq!(revoke.status, 403, "{service} must not revoke sessions");
    }
}

#[test]
fn wrong_service_tokens_are_refused() {
    let fixture = fixture("wrong-service");
    let server = fixture.spawn(&fixture.root.join("state"));
    let (session_id, token, _) = provision(&fixture, &server);
    let body = format!("{{\"token\":\"{token}\"}}");
    let missing = fixture.request(&server, "POST", "/v1/introspect", None, Some(&body));
    assert_eq!(missing.status, 401);
    assert_eq!(
        missing.body,
        "{\"error\":{\"code\":\"service_token_required\",\"retry\":\"never\"}}"
    );
    let unknown = fixture.request(
        &server,
        "POST",
        "/v1/sessions/introspect",
        Some("gateway-service-token-0123456789abcdeg"),
        Some(&body),
    );
    assert_eq!(unknown.status, 401);
    for (service, path) in [
        ("provisioning", "/v1/sessions/introspect"),
        ("provisioning", "/v1/introspect"),
        ("registrar", "/v1/sessions/introspect"),
        ("registrar", "/v1/introspect"),
    ] {
        let refused = fixture.request(
            &server,
            "POST",
            path,
            Some(&token_for(service)),
            Some(&body),
        );
        assert_eq!(
            refused.status, 403,
            "{service} must not introspect at {path}"
        );
        assert_eq!(refused.body, SERVICE_NOT_PERMITTED);
    }
    assert_read_services_cannot_provision(&fixture, &server, &session_id);
    let registrar_session = fixture.request(
        &server,
        "POST",
        "/v1/sessions",
        Some(&token_for("registrar")),
        Some(&format!("{{\"sub\":\"{SUB}\"}}")),
    );
    assert_eq!(
        registrar_session.status, 403,
        "the registrar must not mint sessions"
    );
    assert_eq!(registrar_session.body, SERVICE_NOT_PERMITTED);
    let registrar_revoke = fixture.request(
        &server,
        "DELETE",
        &format!("/v1/sessions/{session_id}"),
        Some(&token_for("registrar")),
        None,
    );
    assert_eq!(
        registrar_revoke.status, 403,
        "the registrar must not revoke sessions"
    );
    assert_eq!(registrar_revoke.body, SERVICE_NOT_PERMITTED);
    let still_active = introspect(&fixture, &server, "faucet", "/v1/introspect", &token);
    assert_eq!(
        still_active.body,
        format!("{{\"active\":true,\"sub\":\"{SUB}\"}}")
    );
    let unknown_principal = fixture.request(
        &server,
        "POST",
        "/v1/sessions",
        Some(&token_for("provisioning")),
        Some("{\"sub\":\"did:key:nobody\"}"),
    );
    assert_eq!(unknown_principal.status, 404);
    let invalid_sub = fixture.request(
        &server,
        "POST",
        "/v1/principals",
        Some(&token_for("provisioning")),
        Some("{\"tenant\":\"beta\",\"sub\":\"Upper:Case\",\"allowed_signer_public_keys\":[]}"),
    );
    assert_eq!(invalid_sub.status, 400);
    let invalid_key = fixture.request(
        &server,
        "POST",
        "/v1/principals",
        Some(&token_for("provisioning")),
        Some("{\"tenant\":\"beta\",\"sub\":\"did:key:other\",\"allowed_signer_public_keys\":[\"abc\"]}"),
    );
    assert_eq!(invalid_key.status, 400);
}

fn assert_registrar_principal_and_mint_refusals(
    fixture: &Fixture,
    server: &Server,
    state: &Path,
    registrar: &str,
) {
    let principal = format!(
        "{{\"tenant\":\"beta\",\"sub\":\"{REGISTRAR_SUB}\",\"allowed_signer_public_keys\":[\"{SIGNER_KEY}\"],\"account\":\"{REGISTRAR_ACCOUNT}\",\"audiences\":[\"ramp-reference\"]}}"
    );
    let created = fixture.request(
        server,
        "POST",
        "/v1/principals",
        Some(registrar),
        Some(&principal),
    );
    assert_eq!(created.status, 200, "{}", created.body);
    assert_eq!(created.body, principal);
    let conflict = fixture.request(
        server,
        "POST",
        "/v1/principals",
        Some(registrar),
        Some(
            &serde_json::json!({
                "tenant": "rival",
                "sub": REGISTRAR_SUB,
                "allowed_signer_public_keys": [OTHER_SIGNER_KEY]
            })
            .to_string(),
        ),
    );
    assert_eq!(conflict.status, 409, "{}", conflict.body);
    assert_eq!(
        conflict.body,
        "{\"error\":{\"code\":\"subject_tenant_conflict\",\"retry\":\"never\"}}"
    );
    for body in [
        format!("{{\"sub\":\"{REGISTRAR_SUB}\"}}"),
        format!("{{\"tenant\":\"beta\",\"sub\":\"{REGISTRAR_SUB}\"}}"),
        format!("{{\"sub\":\"{REGISTRAR_SUB}\",\"ttl_seconds\":60}}"),
    ] {
        let minted = fixture.request(
            server,
            "POST",
            "/v1/sessions",
            Some(registrar),
            Some(&body),
        );
        assert_eq!(minted.status, 403, "{}", minted.body);
        assert_eq!(minted.body, SERVICE_NOT_PERMITTED);
    }
    let journal = fs::read_to_string(state.join("journal.log")).unwrap_or_default();
    assert!(
        journal.contains("\"Principal\"") && journal.contains(REGISTRAR_SUB),
        "the registrar's principal reaches the journal: {journal}"
    );
    assert!(
        !journal.contains("\"Session\""),
        "the registrar's refused mints leave no session record: {journal}"
    );
}

#[test]
fn the_registrar_creates_principals_and_only_provisioning_mints_their_sessions() {
    let fixture = fixture("registrar");
    let state = fixture.root.join("state");
    let server = fixture.spawn(&state);
    let registrar = token_for("registrar");
    assert_registrar_principal_and_mint_refusals(&fixture, &server, &state, &registrar);
    let session = fixture.request(
        &server,
        "POST",
        "/v1/sessions",
        Some(&token_for("provisioning")),
        Some(&format!("{{\"sub\":\"{REGISTRAR_SUB}\"}}")),
    );
    assert_eq!(session.status, 200, "{}", session.body);
    let value = json(&session);
    assert_eq!(value["tenant"], "beta");
    assert_eq!(value["sub"], REGISTRAR_SUB);
    let token = value["token"].as_str().unwrap_or_default().to_owned();
    let session_id = value["session_id"].as_str().unwrap_or_default().to_owned();
    let gateway = introspect(
        &fixture,
        &server,
        "gateway",
        "/v1/sessions/introspect",
        &token,
    );
    assert_eq!(
        gateway.body,
        format!(
            "{{\"active\":true,\"sub\":\"{REGISTRAR_SUB}\",\"allowed_signer_public_keys\":[\"{SIGNER_KEY}\"]}}"
        )
    );
    let ramp = introspect(&fixture, &server, "ramp", "/v1/introspect", &token);
    assert_eq!(json(&ramp)["principal_id"], REGISTRAR_SUB);
    assert_eq!(json(&ramp)["account"], REGISTRAR_ACCOUNT);
    for path in ["/v1/sessions/introspect", "/v1/introspect"] {
        let refused = fixture.request(
            &server,
            "POST",
            path,
            Some(&registrar),
            Some(&format!("{{\"token\":\"{token}\"}}")),
        );
        assert_eq!(refused.status, 403, "{}", refused.body);
        assert_eq!(refused.body, SERVICE_NOT_PERMITTED);
    }
    let revoke = fixture.request(
        &server,
        "DELETE",
        &format!("/v1/sessions/{session_id}"),
        Some(&registrar),
        None,
    );
    assert_eq!(revoke.status, 403, "{}", revoke.body);
    assert_eq!(revoke.body, SERVICE_NOT_PERMITTED);
    let still_active = introspect(&fixture, &server, "faucet", "/v1/introspect", &token);
    assert_eq!(
        still_active.body,
        format!("{{\"active\":true,\"sub\":\"{REGISTRAR_SUB}\"}}")
    );
    let revoked = fixture.request(
        &server,
        "DELETE",
        &format!("/v1/sessions/{session_id}"),
        Some(&token_for("provisioning")),
        None,
    );
    assert_eq!(revoked.status, 200, "{}", revoked.body);
    assert_inactive(&fixture, &server, &token);
}

#[test]
fn boot_requires_a_distinct_registrar_token() {
    let fixture = fixture("registrar-token");
    let state = fixture.root.join("state");
    let registrar = fixture.root.join("tokens").join("registrar");
    fs::remove_file(&registrar).unwrap_or_else(|error| panic!("remove registrar token: {error}"));
    let missing = fixture.boot_refusal(&state);
    assert!(
        missing.contains("service token for registrar"),
        "boot names the missing registrar token: {missing}"
    );
    fs::write(&registrar, format!("{}\n", token_for("provisioning")))
        .unwrap_or_else(|error| panic!("write registrar token: {error}"));
    let duplicate = fixture.boot_refusal(&state);
    assert!(
        duplicate.contains("service token for registrar duplicates another service"),
        "boot refuses a registrar token equal to the provisioning token: {duplicate}"
    );
    fs::write(&registrar, format!("{}\n", token_for("registrar")))
        .unwrap_or_else(|error| panic!("restore registrar token: {error}"));
    let server = fixture.spawn(&state);
    let ready = fixture.request(&server, "GET", "/readyz", None, None);
    assert_eq!(ready.status, 200, "{}", ready.body);
}

#[test]
fn revoked_sessions_introspect_inactive() {
    let fixture = fixture("revocation");
    let server = fixture.spawn(&fixture.root.join("state"));
    let (session_id, token, csrf) = provision(&fixture, &server);
    let ramp = introspect(&fixture, &server, "ramp", "/v1/introspect", &token);
    let expires_at = json(&ramp)["expires_at"].as_u64().unwrap_or_default();
    assert_shapes(&fixture, &server, &token, &csrf, expires_at);
    let revoked = fixture.request(
        &server,
        "DELETE",
        &format!("/v1/sessions/{session_id}"),
        Some(&token_for("provisioning")),
        None,
    );
    assert_eq!(revoked.status, 200, "{}", revoked.body);
    let first = json(&revoked);
    assert_eq!(first["session_id"], session_id);
    assert_eq!(first["revoked"], true);
    let revoked_at = first["revoked_at"].as_u64().unwrap_or_default();
    assert!(revoked_at > 0);
    assert_inactive(&fixture, &server, &token);
    let again = fixture.request(
        &server,
        "DELETE",
        &format!("/v1/sessions/{session_id}"),
        Some(&token_for("provisioning")),
        None,
    );
    assert_eq!(again.status, 200);
    assert_eq!(json(&again)["revoked_at"].as_u64(), Some(revoked_at));
    let unknown = fixture.request(
        &server,
        "DELETE",
        "/v1/sessions/ffffffffffffffffffffffffffffffff",
        Some(&token_for("provisioning")),
        None,
    );
    assert_eq!(unknown.status, 404);
    let malformed = fixture.request(
        &server,
        "DELETE",
        "/v1/sessions/../snapshot.json",
        Some(&token_for("provisioning")),
        None,
    );
    assert_eq!(malformed.status, 404);
}

#[test]
fn expired_sessions_introspect_inactive() {
    let fixture = fixture("expiry");
    let server = fixture.spawn(&fixture.root.join("state"));
    provision(&fixture, &server);
    let short = fixture.request(
        &server,
        "POST",
        "/v1/sessions",
        Some(&token_for("provisioning")),
        Some(&format!("{{\"sub\":\"{SUB}\",\"ttl_seconds\":1}}")),
    );
    assert_eq!(short.status, 200, "{}", short.body);
    let value = json(&short);
    let token = value["token"].as_str().unwrap_or_default().to_owned();
    let expires_at = value["expires_at"].as_u64().unwrap_or_default();
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|duration| duration.as_secs())
        .unwrap_or_default();
    assert!(
        expires_at >= now && expires_at <= now + 2,
        "ttl_seconds bounds expires_at"
    );
    thread::sleep(Duration::from_secs(2));
    assert_inactive(&fixture, &server, &token);
    let zero = fixture.request(
        &server,
        "POST",
        "/v1/sessions",
        Some(&token_for("provisioning")),
        Some(&format!("{{\"sub\":\"{SUB}\",\"ttl_seconds\":0}}")),
    );
    assert_eq!(zero.status, 400);
    let too_long = fixture.request(
        &server,
        "POST",
        "/v1/sessions",
        Some(&token_for("provisioning")),
        Some(&format!("{{\"sub\":\"{SUB}\",\"ttl_seconds\":2592001}}")),
    );
    assert_eq!(too_long.status, 400);
}

#[test]
fn state_survives_a_restart() {
    let fixture = fixture("restart");
    let state = fixture.root.join("state");
    let (session_id, token, csrf, expires_at, revoked_token) = {
        let server = fixture.spawn(&state);
        let (session_id, token, csrf) = provision(&fixture, &server);
        let ramp = introspect(&fixture, &server, "ramp", "/v1/introspect", &token);
        let expires_at = json(&ramp)["expires_at"].as_u64().unwrap_or_default();
        let second = fixture.request(
            &server,
            "POST",
            "/v1/sessions",
            Some(&token_for("provisioning")),
            Some(&format!("{{\"sub\":\"{SUB}\"}}")),
        );
        assert_eq!(second.status, 200);
        let second = json(&second);
        let revoked = fixture.request(
            &server,
            "DELETE",
            &format!(
                "/v1/sessions/{}",
                second["session_id"].as_str().unwrap_or_default()
            ),
            Some(&token_for("provisioning")),
            None,
        );
        assert_eq!(revoked.status, 200);
        (
            session_id,
            token,
            csrf,
            expires_at,
            second["token"].as_str().unwrap_or_default().to_owned(),
        )
    };
    let snapshot = fs::read_to_string(state.join("snapshot.json")).unwrap_or_default();
    let journal = fs::read_to_string(state.join("journal.log")).unwrap_or_default();
    let secret = token.rsplit('.').next().unwrap_or_default();
    assert!(
        !snapshot.contains(secret) && !journal.contains(secret),
        "token secrets stay out of the store"
    );
    assert!(
        !snapshot.contains(&csrf) && !journal.contains(&csrf),
        "csrf tokens are sealed at rest"
    );
    assert!(
        journal.contains(&session_id),
        "the journal holds the session before restart"
    );
    let server = fixture.spawn(&state);
    assert_shapes(&fixture, &server, &token, &csrf, expires_at);
    assert_inactive(&fixture, &server, &revoked_token);
    let compacted = fs::read_to_string(state.join("journal.log")).unwrap_or_default();
    assert!(
        compacted.is_empty(),
        "restart compacts the journal into the snapshot"
    );
    let snapshot = fs::read_to_string(state.join("snapshot.json")).unwrap_or_default();
    assert!(snapshot.contains(&session_id));
    assert!(!snapshot.contains(secret) && !snapshot.contains(&csrf));
    let revoked_now = fixture.request(
        &server,
        "DELETE",
        &format!("/v1/sessions/{session_id}"),
        Some(&token_for("provisioning")),
        None,
    );
    assert_eq!(revoked_now.status, 200);
    drop(server);
    let server = fixture.spawn(&state);
    assert_inactive(&fixture, &server, &token);
}

#[test]
fn principal_tenant_is_required_bounded_and_echoed() {
    let fixture = fixture("tenant");
    let server = fixture.spawn(&fixture.root.join("state"));
    for tenant in [
        None,
        Some(serde_json::json!("")),
        Some(serde_json::json!("a".repeat(129))),
        Some(serde_json::json!("Upper")),
        Some(serde_json::json!("a:b")),
        Some(serde_json::json!("a/b")),
        Some(serde_json::json!("a b")),
        Some(serde_json::json!("é")),
        Some(serde_json::json!("a\n")),
        Some(serde_json::json!(42)),
    ] {
        let mut body = serde_json::json!({"sub": SUB, "allowed_signer_public_keys": [SIGNER_KEY]});
        if let Some(tenant) = tenant {
            body["tenant"] = tenant;
        }
        let reply = fixture.request(
            &server,
            "POST",
            "/v1/principals",
            Some(&token_for("provisioning")),
            Some(&body.to_string()),
        );
        assert_eq!(reply.status, 400, "{}", reply.body);
    }
    for (index, tenant) in ["beta-tenant_1.prod".to_owned(), "a".repeat(128)]
        .into_iter()
        .enumerate()
    {
        let sub = format!("{SUB}-{index}");
        let body = serde_json::json!({"tenant": tenant, "sub": sub, "allowed_signer_public_keys": [SIGNER_KEY]});
        let reply = fixture.request(
            &server,
            "POST",
            "/v1/principals",
            Some(&token_for("provisioning")),
            Some(&body.to_string()),
        );
        assert_eq!(reply.status, 200, "{}", reply.body);
        assert_eq!(json(&reply)["tenant"], tenant);
        assert_eq!(json(&reply)["sub"], sub);
    }
    let claimed = serde_json::json!({
        "tenant": "rival",
        "sub": format!("{SUB}-0"),
        "allowed_signer_public_keys": [OTHER_SIGNER_KEY]
    });
    let conflict = fixture.request(
        &server,
        "POST",
        "/v1/principals",
        Some(&token_for("provisioning")),
        Some(&claimed.to_string()),
    );
    assert_eq!(conflict.status, 409, "{}", conflict.body);
    assert_eq!(
        conflict.body,
        "{\"error\":{\"code\":\"subject_tenant_conflict\",\"retry\":\"never\"}}"
    );
}
#[test]
fn a_second_tenant_cannot_claim_a_bound_subject() {
    let fixture = fixture("tenant-claim");
    let server = fixture.spawn(&fixture.root.join("state"));
    provision_two_tenants(&fixture, &server);
    let refused = create_principal(&fixture, &server, &cross_tenant_claim());
    assert_eq!(refused.status, 409, "{}", refused.body);
    assert_eq!(
        refused.body,
        "{\"error\":{\"code\":\"subject_tenant_conflict\",\"retry\":\"never\"}}"
    );
    let alpha = create_session(&fixture, &server, &serde_json::json!({"sub": ALPHA_SUB}));
    assert_eq!(alpha.status, 200, "{}", alpha.body);
    assert_eq!(json(&alpha)["tenant"], "alpha");
    let bravo = create_session(&fixture, &server, &serde_json::json!({"sub": BRAVO_SUB}));
    assert_eq!(bravo.status, 200, "{}", bravo.body);
    assert_eq!(json(&bravo)["tenant"], "bravo");
    assert_tenant_binding(
        &fixture,
        &server,
        json(&alpha)["token"].as_str().unwrap_or_default(),
        ALPHA_SUB,
        SIGNER_KEY,
    );
    assert_tenant_binding(
        &fixture,
        &server,
        json(&bravo)["token"].as_str().unwrap_or_default(),
        BRAVO_SUB,
        OTHER_SIGNER_KEY,
    );
}

#[test]
fn sessions_belong_to_the_tenant_of_their_principal() {
    let fixture = fixture("tenant-sessions");
    let state = fixture.root.join("state");
    let server = fixture.spawn(&state);
    provision_two_tenants(&fixture, &server);
    for (tenant, sub) in [("bravo", ALPHA_SUB), ("alpha", BRAVO_SUB)] {
        let reply = create_session(
            &fixture,
            &server,
            &serde_json::json!({"tenant": tenant, "sub": sub}),
        );
        assert_eq!(
            reply.status, 404,
            "{tenant} must not mint a session for {sub}"
        );
        assert_eq!(
            reply.body,
            "{\"error\":{\"code\":\"principal_not_found\",\"retry\":\"never\"}}"
        );
    }
    for tenant in ["Alpha", "al:pha", &"a".repeat(129)] {
        let reply = create_session(
            &fixture,
            &server,
            &serde_json::json!({"tenant": tenant, "sub": ALPHA_SUB}),
        );
        assert_eq!(reply.status, 400, "{tenant} is not a tenant name");
    }
    let scoped = create_session(
        &fixture,
        &server,
        &serde_json::json!({"tenant": "alpha", "sub": ALPHA_SUB}),
    );
    assert_eq!(scoped.status, 200, "{}", scoped.body);
    assert_eq!(json(&scoped)["tenant"], "alpha");
    let alpha_token = json(&scoped)["token"]
        .as_str()
        .unwrap_or_default()
        .to_owned();
    let resolved = create_session(&fixture, &server, &serde_json::json!({"sub": BRAVO_SUB}));
    assert_eq!(resolved.status, 200, "{}", resolved.body);
    assert_eq!(json(&resolved)["tenant"], "bravo");
    let bravo_token = json(&resolved)["token"]
        .as_str()
        .unwrap_or_default()
        .to_owned();
    assert_tenant_binding(&fixture, &server, &alpha_token, ALPHA_SUB, SIGNER_KEY);
    assert_tenant_binding(&fixture, &server, &bravo_token, BRAVO_SUB, OTHER_SIGNER_KEY);
    drop(server);

    let server = fixture.spawn(&state);
    let snapshot = fs::read_to_string(state.join("snapshot.json")).unwrap_or_default();
    assert!(
        snapshot.contains(&format!("\"alpha\":{{\"{ALPHA_SUB}\":"))
            && snapshot.contains(&format!("\"bravo\":{{\"{BRAVO_SUB}\":")),
        "the snapshot keys principals by tenant then subject: {snapshot}"
    );
    assert_tenant_binding(&fixture, &server, &alpha_token, ALPHA_SUB, SIGNER_KEY);
    assert_tenant_binding(&fixture, &server, &bravo_token, BRAVO_SUB, OTHER_SIGNER_KEY);
    let still_refused = create_principal(&fixture, &server, &cross_tenant_claim());
    assert_eq!(still_refused.status, 409, "{}", still_refused.body);
    assert_tenant_binding(&fixture, &server, &alpha_token, ALPHA_SUB, SIGNER_KEY);
}

#[test]
fn registry_resolver_authenticates_and_retains_authority_across_restart() {
    use sha2::{Digest, Sha256};
    let fixture = fixture("publication-resolver");
    let state_dir = fixture.root.join("state");
    let server = fixture.spawn(&state_dir);
    let _ = provision(&fixture, &server);
    let key = "publication-key:resolver-integration-only";
    let headers = [("LayerX-Key", key)];
    let resolve = |server: &Server, token: Option<&str>, body: Option<&str>| {
        fixture.request_with_headers(
            server,
            "GET",
            "/internal/v1/principal",
            token,
            body,
            &headers,
        )
    };
    assert_eq!(resolve(&server, None, None).status, 401);
    assert_eq!(
        resolve(&server, Some(&token_for("gateway")), None).status,
        403
    );
    assert_eq!(
        resolve(&server, Some(&token_for("registry")), None).status,
        404
    );
    let body = format!("{{\"sub\":\"{SUB}\",\"revoked\":false}}");
    let bind = fixture.request_with_headers(
        &server,
        "POST",
        "/v1/publication-keys",
        Some(&token_for("provisioning")),
        Some(&body),
        &headers,
    );
    assert_eq!(bind.status, 200);
    assert_eq!(
        resolve(
            &server,
            Some(&token_for("registry")),
            Some("{\"principal_digest\":\"foreign\"}")
        )
        .status,
        400
    );
    let expected = format!("{:x}", Sha256::digest(SUB.as_bytes()));
    assert_eq!(
        json(&resolve(&server, Some(&token_for("registry")), None))["result"]["principal_digest"],
        expected
    );
    registry_client_resolves(&fixture, &server, key, &expected);
    drop(server);
    let server = fixture.spawn(&state_dir);
    assert_eq!(
        json(&resolve(&server, Some(&token_for("registry")), None))["result"]["principal_digest"],
        expected
    );
    let revoke = format!("{{\"sub\":\"{SUB}\",\"revoked\":true}}");
    assert_eq!(
        fixture
            .request_with_headers(
                &server,
                "POST",
                "/v1/publication-keys",
                Some(&token_for("provisioning")),
                Some(&revoke),
                &headers
            )
            .status,
        200
    );
    assert_eq!(
        resolve(&server, Some(&token_for("registry")), None).status,
        404
    );
    assert_eq!(
        fixture
            .request_with_headers(
                &server,
                "POST",
                "/v1/publication-keys",
                Some(&token_for("provisioning")),
                Some(&body),
                &headers
            )
            .status,
        409
    );
    drop(server);
    let server = fixture.spawn(&state_dir);
    assert_eq!(
        resolve(&server, Some(&token_for("registry")), None).status,
        404
    );
}

fn registry_client_resolves(fixture: &Fixture, server: &Server, key: &str, expected: &str) {
    use layerx_platform_internal::gateway_http::{Client, Endpoint};
    use layerx_platform_internal::principal::PrincipalClient;
    use native_tls::Identity;
    use zeroize::Zeroizing;
    openssl(&[
        "pkcs12",
        "-export",
        "-inkey",
        &fixture.root.join("server.key").to_string_lossy(),
        "-in",
        &fixture.root.join("server.crt").to_string_lossy(),
        "-out",
        &fixture.root.join("client.p12").to_string_lossy(),
        "-passout",
        "pass:integration-only",
    ]);
    let identity = Identity::from_pkcs12(
        &fs::read(fixture.root.join("client.p12")).unwrap_or_else(|error| panic!("{error}")),
        "integration-only",
    )
    .unwrap_or_else(|error| panic!("{error}"));
    let client = PrincipalClient::new(
        Client::new(
            Certificate::from_der(&fixture.ca_der).unwrap_or_else(|error| panic!("{error}")),
            identity,
        ),
        Endpoint::parse(&format!("https://localhost:{}", server.port))
            .unwrap_or_else(|error| panic!("{error}")),
        Zeroizing::new(token_for("registry")),
    );
    assert_eq!(client.resolve(key).as_deref(), Ok(expected));
    assert!(client.resolve("unknown-publication-key").is_err());
}
