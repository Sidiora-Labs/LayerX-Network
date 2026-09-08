use layerx_agentd::human::HumanPeer;
use layerx_agentd::human_runtime::{HumanAuthorityBoundary, RemoteHumanAuthority};
use layerx_platform_authority::{hex, receipt_locator};
use layerx_types::ids::Did;
use serde_json::{json, Value};
use std::fs;
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

fn must<T, E: std::fmt::Debug>(result: Result<T, E>) -> T {
    result.unwrap_or_else(|e| panic!("{e:?}"))
}

fn write(path: &Path, bytes: &[u8]) {
    must(fs::write(path, bytes));
    must(fs::set_permissions(path, fs::Permissions::from_mode(0o600)));
}

fn openssl(root: &Path, arguments: &[&str]) {
    let result = must(
        Command::new("openssl")
            .current_dir(root)
            .args(arguments)
            .output(),
    );
    assert!(
        result.status.success(),
        "openssl failed: {}",
        String::from_utf8_lossy(&result.stderr)
    );
}

struct Server {
    child: Child,
    root: PathBuf,
}
impl Drop for Server {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

fn prepare_root() -> PathBuf {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../../qual-logs/hm2");
    must(fs::create_dir_all(&root));
    let mut nonce = [0_u8; 8];
    must(getrandom::fill(&mut nonce));
    let root = must(fs::canonicalize(root)).join(format!(
        "human-tls-{}-{}",
        std::process::id(),
        hex::encode(&nonce)
    ));
    must(fs::create_dir(&root));
    must(fs::set_permissions(
        &root,
        fs::Permissions::from_mode(0o700),
    ));
    let state = root.join("state");
    must(fs::create_dir(&state));
    must(fs::set_permissions(
        &state,
        fs::Permissions::from_mode(0o700),
    ));
    openssl(
        &root,
        &[
            "req",
            "-x509",
            "-newkey",
            "rsa:2048",
            "-nodes",
            "-keyout",
            "server.key",
            "-out",
            "server.pem",
            "-days",
            "1",
            "-subj",
            "/CN=localhost",
            "-addext",
            "subjectAltName=DNS:localhost,IP:127.0.0.1",
            "-addext",
            "basicConstraints=critical,CA:TRUE",
        ],
    );
    openssl(
        &root,
        &[
            "x509",
            "-in",
            "server.pem",
            "-outform",
            "DER",
            "-out",
            "server.der",
        ],
    );
    openssl(
        &root,
        &[
            "pkcs8",
            "-topk8",
            "-nocrypt",
            "-in",
            "server.key",
            "-outform",
            "DER",
            "-out",
            "key.der",
        ],
    );
    root
}

fn seed(root: &Path) {
    let state = root.join("state");
    let fixture: Value = must(serde_json::from_str(include_str!(
        "fixtures/real-program-deploy-receipt.json"
    )));
    let text = |key: &str| {
        fixture[key]
            .as_str()
            .unwrap_or_else(|| panic!("fixture field {key}"))
    };
    let receipt = must(hex::decode(text("receipt_hex")));
    let locator = must(receipt_locator(&receipt));
    let activity = hex::encode(&locator.activity_id);
    let reference =
        json!({"activity_id": activity, "receipt_digest": hex::encode(&locator.receipt_digest)});
    let key_policy = json!({"policy_revision":1,"required_delay_seconds":10,"maximum_delay_seconds":20,"effective_sequence":1,"evidence":reference});
    let policy = json!({"principals":[{"tenant":"tenant space","principal":"principal","account_id":hex::encode(&[1;32]),"asset_id":hex::encode(&[2;32]),"activities":[activity],"budgets":[hex::encode(&[4;32])],"maximum_age_seconds":60,"maximum_age_sequences":100,"identities":[{"did":"did:layerx:test","authorities":[{"kind":"primary_key","id":hex::encode(&[5;32])}],"revocation_sequence":1,"frozen":false,"evidence":reference,"capabilities":[],"rotation":key_policy,"recovery":key_policy}]}]});
    write(
        &root.join("policy.json"),
        &must(serde_json::to_vec(&policy)),
    );
    write(
        &root.join("modules.json"),
        br#"{"modules":[{"module":9,"ordinals":[1,7]}]}"#,
    );
    write(
        &root.join("human.token"),
        b"human-test-token-0000000000000000000000",
    );
    write(
        &root.join("service.token"),
        b"service-test-token-00000000000000000000",
    );
    write(
        &root.join("replica.token"),
        b"replica-test-token-00000000000000000000",
    );
    let mut encoder = layerx_wire::encode::Encoder::new(64);
    must(encoder.structure_header(0x4d50));
    must(encoder.u32(0));
    must(encoder.u32(1));
    must(encoder.u8(0));
    must(encoder.bytes(&[], 1024));
    let proof = encoder.finish();
    let record = json!({"receipt_hex": text("receipt_hex"), "replica_document": {
        "authority_replica_id": hex::encode(&[9;32]), "sequencer_public_key": text("sequencer_public_key_hex"),
        "batch_evidence": {"header_hex":text("header_hex"),"header_signature":text("header_signature_hex"),"receipt_proof_hex":hex::encode(&proof)}
    }});
    write(
        &state.join(format!("{activity}.json")),
        &must(serde_json::to_vec(&record)),
    );
}

fn launch(root: PathBuf) -> (Server, std::net::SocketAddr) {
    let state = root.join("state");
    let fixture: Value = must(serde_json::from_str(include_str!(
        "fixtures/real-program-deploy-receipt.json"
    )));
    let text = |key: &str| {
        fixture[key]
            .as_str()
            .unwrap_or_else(|| panic!("fixture field {key}"))
    };
    let listener = must(TcpListener::bind("127.0.0.1:0"));
    let address = must(listener.local_addr());
    drop(listener);
    let output = must(fs::File::create(root.join("server.log")));
    let child = must(
        Command::new(env!("CARGO_BIN_EXE_layerx-receipt-authority"))
            .env_clear()
            .env("PATH", std::env::var_os("PATH").unwrap_or_default())
            .env("LAYERX_AUTHORITY_LISTEN", address.to_string())
            .env("LAYERX_AUTHORITY_TLS_CERT_DER", root.join("server.der"))
            .env("LAYERX_AUTHORITY_TLS_KEY_DER", root.join("key.der"))
            .env("LAYERX_AUTHORITY_TOKEN_FILES", root.join("service.token"))
            .env("LAYERX_AUTHORITY_REPLICA_URL", "http://127.0.0.1:1")
            .env(
                "LAYERX_AUTHORITY_REPLICA_BEARER_TOKEN_FILE",
                root.join("replica.token"),
            )
            .env("LAYERX_AUTHORITY_REPLICA_ID", hex::encode(&[9; 32]))
            .env("LAYERX_AUTHORITY_LNI_SOCKET", root.join("absent.sock"))
            .env("LAYERX_AUTHORITY_PROTOCOL_NETWORK_ID", "7332")
            .env("LAYERX_AUTHORITY_NETWORK_ID", "human-authority-test")
            .env("LAYERX_AUTHORITY_SEQUENCER_ID", text("sequencer_id_hex"))
            .env(
                "LAYERX_AUTHORITY_SEQUENCER_PUBLIC_KEY",
                text("sequencer_public_key_hex"),
            )
            .env("LAYERX_AUTHORITY_FIRST_BATCH", "1")
            .env("LAYERX_AUTHORITY_LAST_BATCH", "100")
            .env(
                "LAYERX_AUTHORITY_HUMAN_AGENT_TOKEN_FILE",
                root.join("human.token"),
            )
            .env("LAYERX_AUTHORITY_HUMAN_AGENT_TENANT", "tenant space")
            .env("LAYERX_AUTHORITY_HUMAN_AGENT_PRINCIPAL", "principal")
            .env(
                "LAYERX_AUTHORITY_PRINCIPAL_POLICY_FILE",
                root.join("policy.json"),
            )
            .env(
                "LAYERX_AUTHORITY_MODULE_REGISTRY_FILE",
                root.join("modules.json"),
            )
            .env("LAYERX_AUTHORITY_CORE_CLOCK_HORIZON", "100")
            .env("LAYERX_AUTHORITY_STATE_ROOT", &state)
            .stdout(Stdio::null())
            .stderr(output)
            .spawn(),
    );
    let mut server = Server { child, root };
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        assert!(
            must(server.child.try_wait()).is_none(),
            "authority exited; see {}/server.log",
            server.root.display()
        );
        if TcpStream::connect(address).is_ok() {
            break;
        }
        assert!(Instant::now() < deadline, "authority startup timed out");
        std::thread::sleep(Duration::from_millis(20));
    }
    (server, address)
}

#[test]
fn real_tls_client_and_durable_refusals() {
    if std::env::var_os("LAYERX_HUMAN_TLS_CHILD").is_some() {
        drive_client();
        return;
    }
    let root = prepare_root();
    seed(&root);
    let (server, address) = launch(root);
    let result = must(
        Command::new(must(std::env::current_exe()))
            .args([
                "--exact",
                "real_tls_client_and_durable_refusals",
                "--nocapture",
            ])
            .env("LAYERX_HUMAN_TLS_CHILD", address.to_string())
            .env("LAYERX_HUMAN_TLS_ROOT", &server.root)
            .env("SSL_CERT_FILE", server.root.join("server.pem"))
            .env("SSL_CERT_DIR", &server.root)
            .env_remove("HTTPS_PROXY")
            .env_remove("HTTP_PROXY")
            .env_remove("ALL_PROXY")
            .output(),
    );
    write(
        &server.root.join("client.log"),
        &[result.stdout, result.stderr].concat(),
    );
    assert!(
        result.status.success(),
        "real client failed; see {}/client.log",
        server.root.display()
    );
}

fn wire_status(root: &Path, path: &str) -> u16 {
    wire_status_at(
        root,
        &must(std::env::var("LAYERX_HUMAN_TLS_CHILD")),
        path,
        "human-test-token-0000000000000000000000",
    )
}

fn wire_status_at(root: &Path, address: &str, path: &str, token: &str) -> u16 {
    let certificate = must(native_tls::Certificate::from_pem(&must(fs::read(
        root.join("server.pem"),
    ))));
    let connector = must(
        native_tls::TlsConnector::builder()
            .add_root_certificate(certificate)
            .build(),
    );
    let tcp = must(TcpStream::connect(address));
    must(tcp.set_read_timeout(Some(Duration::from_secs(5))));
    let mut tls = must(connector.connect("localhost", tcp));
    must(write!(tls, "GET {path} HTTP/1.1\r\nHost: localhost\r\nAuthorization: Bearer {token}\r\nConnection: close\r\n\r\n"));
    let mut response = String::new();
    must(tls.read_to_string(&mut response));
    must(
        response
            .split_whitespace()
            .nth(1)
            .unwrap_or_else(|| panic!("status absent"))
            .parse(),
    )
}

#[test]
fn real_server_wire_statuses_remain_distinct() {
    let root = prepare_root();
    seed(&root);
    let (server, address) = launch(root);
    let address = address.to_string();
    let status = |path: &str| {
        wire_status_at(
            &server.root,
            &address,
            path,
            "human-test-token-0000000000000000000000",
        )
    };
    let fixture: Value = must(serde_json::from_str(include_str!(
        "fixtures/real-program-deploy-receipt.json"
    )));
    let receipt = must(hex::decode(
        fixture["receipt_hex"]
            .as_str()
            .unwrap_or_else(|| panic!("receipt")),
    ));
    let activity = hex::encode(&must(receipt_locator(&receipt)).activity_id);
    let base = "tenant=tenant%20space&principal=principal";
    assert_eq!(status(&format!("/v1/agent/registry?{base}")), 200);
    assert_eq!(
        status(&format!(
            "/v1/agent/authorized-batch?{base}&activity_id={activity}"
        )),
        200
    );
    assert_eq!(status(&format!("/v1/agent/core-clock?{base}")), 503);
    assert_eq!(status(&format!("/v1/agent/balance-context?{base}")), 503);
    assert_eq!(
        status(&format!(
            "/v1/agent/identity?{base}&did=did%3Alayerx%3Atest"
        )),
        503
    );
    assert_eq!(status(&format!("/v1/agent/capability-scope?{base}&did=did%3Alayerx%3Atest&authority={}&action_key={}&capability_id={}", hex::encode(&[5;32]), hex::encode(&[6;32]), hex::encode(&[7;32]))), 403);
    assert_eq!(
        status(&format!(
            "/v1/agent/budget-state?{base}&budget_id={}",
            hex::encode(&[4; 32])
        )),
        503
    );
    assert_eq!(
        status(&format!(
            "/v1/agent/key-policy?{base}&did=did%3Alayerx%3Atest&recovery=false"
        )),
        503
    );
    assert_eq!(
        status("/v1/agent/registry?tenant=other&principal=principal"),
        403
    );
    assert_eq!(
        wire_status_at(
            &server.root,
            &address,
            &format!("/v1/agent/registry?{base}"),
            "service-test-token-00000000000000000000"
        ),
        401
    );
    must(fs::rename(
        server.root.join("policy.json"),
        server.root.join("policy.hidden"),
    ));
    assert_eq!(status(&format!("/v1/agent/registry?{base}")), 503);
    must(fs::rename(
        server.root.join("policy.hidden"),
        server.root.join("policy.json"),
    ));
    assert_eq!(status(&format!("/v1/agent/registry?{base}")), 200);
    write(&server.root.join("policy.json"), br#"{"principals":[]}"#);
    assert_eq!(status(&format!("/v1/agent/registry?{base}")), 503);
}

fn drive_client() {
    use layerx_agentd::human::HumanOperationError;
    let endpoint = format!("https://{}", must(std::env::var("LAYERX_HUMAN_TLS_CHILD")));
    let root = PathBuf::from(must(std::env::var("LAYERX_HUMAN_TLS_ROOT")));
    let mut client = must(RemoteHumanAuthority::connect(
        &endpoint,
        "human-test-token-0000000000000000000000".to_owned(),
        Duration::from_secs(5),
        1_048_576,
    ));
    let peer = HumanPeer {
        uid: 1,
        tenant: "tenant space".to_owned(),
        principal: "principal".to_owned(),
    };
    must(client.registry(&peer));
    assert_eq!(
        wire_status(
            &root,
            "/v1/agent/core-clock?tenant=tenant%20space&principal=principal"
        ),
        503
    );
    assert_eq!(
        wire_status(&root, "/v1/agent/registry?tenant=other&principal=principal"),
        403
    );
    assert_eq!(wire_status(&root, &format!("/v1/agent/capability-scope?tenant=tenant%20space&principal=principal&did=did%3Alayerx%3Atest&authority={}&action_key={}&capability_id={}", hex::encode(&[5;32]), hex::encode(&[6;32]), hex::encode(&[7;32]))), 403);
    let fixture: Value = must(serde_json::from_str(include_str!(
        "fixtures/real-program-deploy-receipt.json"
    )));
    let receipt = must(hex::decode(
        fixture["receipt_hex"]
            .as_str()
            .unwrap_or_else(|| panic!("receipt")),
    ));
    let locator = must(receipt_locator(&receipt));
    assert_eq!(
        must(client.authorized_batch(&peer, locator.activity_id)).batch_id(),
        locator.batch_id
    );
    assert!(matches!(
        client.lease_attestation(&peer),
        Err(HumanOperationError::Refused)
    ));
    assert!(matches!(
        client.balance_context(&peer),
        Err(HumanOperationError::Refused)
    ));
    let did = must(Did::new(b"did:layerx:test"));
    assert!(client.core_identity(&peer, &did).is_err());
    assert!(matches!(
        client.capability_scope(&peer, &did, [5; 32], [6; 32], [7; 32]),
        Err(HumanOperationError::Refused)
    ));
    assert!(matches!(
        client.budget_state(&peer, [4; 32]),
        Err(HumanOperationError::Refused)
    ));
    assert!(matches!(
        client.key_rotation_policy(&peer, &did, false),
        Err(HumanOperationError::Refused)
    ));
    let other = HumanPeer {
        tenant: "other".to_owned(),
        ..peer.clone()
    };
    assert!(client.registry(&other).is_err());
    must(fs::rename(
        root.join("policy.json"),
        root.join("policy.hidden"),
    ));
    assert!(client.registry(&peer).is_err());
    assert_eq!(
        wire_status(
            &root,
            "/v1/agent/registry?tenant=tenant%20space&principal=principal"
        ),
        503
    );
    must(fs::rename(
        root.join("policy.hidden"),
        root.join("policy.json"),
    ));
    must(client.registry(&peer));
    write(&root.join("policy.json"), br#"{"principals":[]}"#);
    assert!(client.registry(&peer).is_err());
}
