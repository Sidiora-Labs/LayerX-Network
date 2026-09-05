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

const SERVICES: [&str; 7] = [
    "gateway",
    "webhooks",
    "dashboard",
    "faucet",
    "testnet",
    "ramp",
    "provisioning",
];
const SIGNER_KEY: &str = "1f2e3d4c5b6a79880123456789abcdef1f2e3d4c5b6a79880123456789abcdef";
const SUB: &str = "did:key:z6mkbeta-principal_1";
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
    fn spawn(&self, state_dir: &Path) -> Server {
        let mut child = Command::new(env!("CARGO_BIN_EXE_layerx-identity"))
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
            .stderr(Stdio::piped())
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
            "{{\"sub\":\"{SUB}\",\"allowed_signer_public_keys\":[\"{SIGNER_KEY}\"],\"account\":\"{ACCOUNT}\",\"audiences\":[\"ramp-reference\"]}}"
        )),
    );
    assert_eq!(principal.status, 200, "{}", principal.body);
    assert_eq!(
        principal.body,
        format!(
            "{{\"sub\":\"{SUB}\",\"allowed_signer_public_keys\":[\"{SIGNER_KEY}\"],\"account\":\"{ACCOUNT}\",\"audiences\":[\"ramp-reference\"]}}"
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
    assert!(value["expires_at"].as_u64().unwrap_or_default() > 0);
    assert_eq!(session_id.len(), 32);
    assert_eq!(csrf.len(), 64);
    assert_eq!(token, format!("ses_{session_id}.{}", &token[37..]));
    assert_eq!(token.len(), 4 + 32 + 1 + 64);
    (session_id, token, csrf)
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
    assert_eq!(
        fixture
            .request(&server, "GET", "/readyz", None, None)
            .status,
        200
    );
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
    let provisioning_introspect = fixture.request(
        &server,
        "POST",
        "/v1/sessions/introspect",
        Some(&token_for("provisioning")),
        Some(&body),
    );
    assert_eq!(provisioning_introspect.status, 403);
    assert_eq!(
        provisioning_introspect.body,
        "{\"error\":{\"code\":\"service_not_permitted\",\"retry\":\"never\"}}"
    );
    for service in [
        "gateway",
        "webhooks",
        "dashboard",
        "faucet",
        "testnet",
        "ramp",
    ] {
        let principal = fixture.request(
            &server,
            "POST",
            "/v1/principals",
            Some(&token_for(service)),
            Some(&format!(
                "{{\"sub\":\"{SUB}\",\"allowed_signer_public_keys\":[]}}"
            )),
        );
        assert_eq!(
            principal.status, 403,
            "{service} must not provision principals"
        );
        let session = fixture.request(
            &server,
            "POST",
            "/v1/sessions",
            Some(&token_for(service)),
            Some(&format!("{{\"sub\":\"{SUB}\"}}")),
        );
        assert_eq!(session.status, 403, "{service} must not mint sessions");
        let revoke = fixture.request(
            &server,
            "DELETE",
            &format!("/v1/sessions/{session_id}"),
            Some(&token_for(service)),
            None,
        );
        assert_eq!(revoke.status, 403, "{service} must not revoke sessions");
    }
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
        Some("{\"sub\":\"Upper:Case\",\"allowed_signer_public_keys\":[]}"),
    );
    assert_eq!(invalid_sub.status, 400);
    let invalid_key = fixture.request(
        &server,
        "POST",
        "/v1/principals",
        Some(&token_for("provisioning")),
        Some("{\"sub\":\"did:key:other\",\"allowed_signer_public_keys\":[\"abc\"]}"),
    );
    assert_eq!(invalid_key.status, 400);
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
