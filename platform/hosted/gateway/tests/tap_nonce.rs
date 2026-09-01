use layerx_platform_gateway::store::{
    RedisEndpoint, RedisStore, TapCredentialRecord, TapNonceConsumption,
};
use native_tls::Certificate;
use std::fs;
use std::net::{TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::thread;
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use zeroize::Zeroizing;

struct RedisProcess {
    child: Child,
    directory: PathBuf,
    endpoint: RedisEndpoint,
    certificate: Certificate,
}

impl RedisProcess {
    fn start() -> Self {
        let unique = format!(
            "layerx-tap-redis-{}-{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap_or_default()
                .as_nanos()
        );
        let directory = std::env::temp_dir().join(unique);
        fs::create_dir(&directory)
            .unwrap_or_else(|error| panic!("test Redis directory must be created: {error}"));
        let certificate_pem = directory.join("server.pem");
        let certificate_der = directory.join("server.der");
        let private_key = directory.join("server.key");
        command(
            "openssl",
            &[
                "req",
                "-x509",
                "-newkey",
                "rsa:2048",
                "-nodes",
                "-keyout",
                path(&private_key),
                "-out",
                path(&certificate_pem),
                "-days",
                "1",
                "-subj",
                "/CN=localhost",
                "-addext",
                "subjectAltName=DNS:localhost",
            ],
        );
        command(
            "openssl",
            &[
                "x509",
                "-in",
                path(&certificate_pem),
                "-outform",
                "DER",
                "-out",
                path(&certificate_der),
            ],
        );
        let listener = TcpListener::bind("127.0.0.1:0")
            .unwrap_or_else(|error| panic!("test port must be allocated: {error}"));
        let port = listener
            .local_addr()
            .unwrap_or_else(|error| panic!("test port must resolve: {error}"))
            .port();
        drop(listener);
        let acl = directory.join("users.acl");
        fs::write(
            &acl,
            "user default off\nuser tap on >tap-secret ~* &* +@all\n",
        )
        .unwrap_or_else(|error| panic!("test Redis ACL must be written: {error}"));
        let config = directory.join("redis.conf");
        fs::write(
            &config,
            format!(
                "bind 127.0.0.1\nport 0\ntls-port {port}\ntls-cert-file {}\ntls-key-file {}\ntls-ca-cert-file {}\ntls-auth-clients no\naclfile {}\nappendonly yes\nappendfsync always\ndir {}\nprotected-mode yes\n",
                path(&certificate_pem),
                path(&private_key),
                path(&certificate_pem),
                path(&acl),
                path(&directory),
            ),
        )
        .unwrap_or_else(|error| panic!("test Redis config must be written: {error}"));
        let child = Command::new("redis-server")
            .arg(&config)
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .unwrap_or_else(|error| panic!("real Redis server must start: {error}"));
        let endpoint = RedisEndpoint::parse(&format!("rediss://localhost:{port}"))
            .unwrap_or_else(|error| panic!("test Redis endpoint must parse: {error}"));
        let certificate = Certificate::from_der(
            &fs::read(&certificate_der)
                .unwrap_or_else(|error| panic!("test certificate must be read: {error}")),
        )
        .unwrap_or_else(|error| panic!("test certificate must parse: {error}"));
        for _ in 0..100 {
            if TcpStream::connect(("127.0.0.1", port)).is_ok() {
                return Self {
                    child,
                    directory,
                    endpoint,
                    certificate,
                };
            }
            thread::sleep(Duration::from_millis(20));
        }
        panic!("real Redis server did not become reachable")
    }

    fn store(&self) -> RedisStore {
        RedisStore::new(
            self.endpoint.clone(),
            self.certificate.clone(),
            Zeroizing::new("tap".to_owned()),
            Zeroizing::new("tap-secret".to_owned()),
        )
    }
}

impl Drop for RedisProcess {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
        let _ = fs::remove_dir_all(&self.directory);
    }
}

fn command(program: &str, arguments: &[&str]) {
    let output = Command::new(program)
        .args(arguments)
        .output()
        .unwrap_or_else(|error| panic!("{program} must run: {error}"));
    assert!(
        output.status.success(),
        "{program} failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
}

fn path(value: &Path) -> &str {
    value
        .to_str()
        .unwrap_or_else(|| panic!("test path must be UTF-8"))
}

#[test]
fn exact_pending_retry_survives_reconstruction_but_altered_nonce_reuse_is_replay() {
    let redis = RedisProcess::start();
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    let record = TapCredentialRecord {
        principal_digest: "11".repeat(32),
        key_id: "tap-registry-key-7".to_owned(),
        layerx_agent: "22".repeat(32),
        trusted_agent_id: "trusted-agent-7".to_owned(),
        trusted_agent_domain: "https://agent.example".to_owned(),
        intent: "pay".to_owned(),
        evidence_digest: "33".repeat(32),
        activity_id: Some("44".repeat(32)),
        signer_public_key: "22".repeat(32),
        target_authority: "shop.example".to_owned(),
        target_path: "/checkout".to_owned(),
        operation_identity: "55".repeat(32),
        credential_expires_at: now + 300,
    };
    let first = redis
        .store()
        .consume_tap_nonce(
            &record.key_id,
            "nonce-across-service-restart",
            &record,
            now,
            now + 360,
            "tap-nonce-first-request",
        )
        .unwrap_or_else(|error| panic!("first nonce consumption must persist: {error}"));
    let TapNonceConsumption::Consumed { binding_digest } = first else {
        panic!("first nonce consumption must not be a replay")
    };

    let reconstructed = redis.store();
    assert_eq!(
        reconstructed
            .consume_tap_nonce(
                &record.key_id,
                "nonce-across-service-restart",
                &record,
                now + 1,
                now + 360,
                "tap-nonce-second-request",
            )
            .unwrap_or_else(|error| panic!("replay decision must come from Redis: {error}")),
        TapNonceConsumption::AlreadyConsumed {
            binding_digest: binding_digest.clone()
        }
    );
    let mut altered_operation = record.clone();
    altered_operation.operation_identity = "66".repeat(32);
    assert_eq!(
        reconstructed
            .consume_tap_nonce(
                &altered_operation.key_id,
                "nonce-across-service-restart",
                &altered_operation,
                now + 2,
                now + 360,
                "tap-nonce-altered-operation",
            )
            .unwrap_or_else(|error| panic!("altered operation must reach replay state: {error}")),
        TapNonceConsumption::Replay
    );
    let mut altered_target = record.clone();
    altered_target.target_path = "/other-checkout".to_owned();
    assert_eq!(
        reconstructed
            .consume_tap_nonce(
                &altered_target.key_id,
                "nonce-across-service-restart",
                &altered_target,
                now + 3,
                now + 360,
                "tap-nonce-altered-target",
            )
            .unwrap_or_else(|error| panic!("altered target must reach replay state: {error}")),
        TapNonceConsumption::Replay
    );
    assert_eq!(
        reconstructed
            .tap_binding(&binding_digest)
            .unwrap_or_else(|error| panic!("durable TAP binding must be readable: {error}")),
        Some(record)
    );
}
