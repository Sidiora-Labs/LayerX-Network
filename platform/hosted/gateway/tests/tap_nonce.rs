use layerx_platform_gateway::store::{
    RedisEndpoint, RedisStore, TapCredentialRecord, TapNonceConsumption,
};
use native_tls::Certificate;
use std::fmt::Write as _;
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
        let endpoint = RedisEndpoint::parse(&format!("rediss://localhost:{port}"))
            .unwrap_or_else(|error| panic!("test Redis endpoint must parse: {error}"));
        let certificate = Certificate::from_der(
            &fs::read(&certificate_der)
                .unwrap_or_else(|error| panic!("test certificate must be read: {error}")),
        )
        .unwrap_or_else(|error| panic!("test certificate must parse: {error}"));
        let child = Command::new("redis-server")
            .arg(&config)
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .unwrap_or_else(|error| panic!("real Redis server must start: {error}"));
        let process = Self {
            child,
            directory,
            endpoint,
            certificate,
        };
        for _ in 0..100 {
            if TcpStream::connect(("127.0.0.1", port)).is_ok() {
                return process;
            }
            thread::sleep(Duration::from_millis(20));
        }
        redis_unreachable(process)
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

#[test]
fn principal_binding_comes_only_from_the_authenticated_durable_key_record() {
    use layerx_platform_gateway::store::KeyRecord;
    use layerx_platform_gateway::{authenticate_gateway_key, gateway_digest, PrincipalId};
    let redis = RedisProcess::start();
    let store = redis.store();
    let principal = PrincipalId::new("principal-one".to_owned())
        .unwrap_or_else(|error| panic!("principal: {error:?}"));
    let foreign = PrincipalId::new("principal-two".to_owned())
        .unwrap_or_else(|error| panic!("principal: {error:?}"));
    let digest = |principal: &PrincipalId| {
        principal
            .audit_digest()
            .iter()
            .fold(String::new(), |mut text, byte| {
                write!(text, "{byte:02x}")
                    .unwrap_or_else(|error| panic!("writing to String cannot fail: {error}"));
                text
            })
    };
    let secret = format!("lxp_live_{}", "a".repeat(64));
    let record = KeyRecord {
        key_id: "principal-key-one".to_owned(),
        principal_digest: digest(&principal),
        salt: "salt-one".to_owned(),
        secret_digest: gateway_digest(&[b"gateway-key-v1", b"salt-one", secret.as_bytes()]),
        signer_public_key: "11".repeat(32),
        scopes: "receipt:read".to_owned(),
        quota_requests: 100,
        quota_window_seconds: 60,
        epoch: 1,
        disabled: false,
    };
    store
        .issue_key(&record, "principal-key-issued")
        .unwrap_or_else(|error| panic!("issue: {error}"));
    let credential = format!("LayerX-Key {}:{secret}", record.key_id);
    let authenticated = authenticate_gateway_key(&store, &credential)
        .unwrap_or_else(|error| panic!("authenticate: {error:?}"));
    assert_eq!(authenticated.principal_digest, digest(&principal));
    assert_ne!(authenticated.principal_digest, digest(&foreign));
    assert!(authenticate_gateway_key(&store, "").is_err());
    assert!(authenticate_gateway_key(&store, &format!("Bearer {secret}")).is_err());
    assert!(authenticate_gateway_key(
        &store,
        &format!("LayerX-Key {}:lxp_live_{}", record.key_id, "b".repeat(64))
    )
    .is_err());
    assert!(store
        .revoke_key(
            &record.key_id,
            &record.principal_digest,
            "principal-key-revoked"
        )
        .unwrap_or_else(|error| panic!("revoke: {error}")));
    assert!(authenticate_gateway_key(&store, &credential).is_err());
}

fn redis_unreachable(mut process: RedisProcess) -> ! {
    let _ = process.child.kill();
    process
        .child
        .wait()
        .unwrap_or_else(|error| panic!("test Redis child must be reaped: {error}"));
    panic!("real Redis server did not become reachable")
}

#[test]
fn payment_outbox_commits_with_completion_and_retains_acknowledgements() {
    use layerx_platform_gateway::store::{Completion, KeyRecord, Reservation, ReservationRequest};
    use layerx_platform_internal::events::Fact;
    use layerx_platform_internal::producer::{event_id, Observation, Outbox, Pending};
    use layerx_platform_internal::secret::{hex, sha256_hex, unhex};

    let redis = RedisProcess::start();
    let store = redis.store();
    let document: serde_json::Value =
        serde_json::from_str(include_str!("fixtures/maintained-authority.json"))
            .unwrap_or_else(|error| panic!("receipt fixture: {error}"));
    let receipt_hex = document["receipt_hex"]
        .as_str()
        .unwrap_or_else(|| panic!("receipt fixture bytes missing"));
    let receipt_bytes = unhex(receipt_hex).unwrap_or_else(|| panic!("invalid receipt hex"));
    let decoded = layerx_wire::receipt::decode(&receipt_bytes)
        .unwrap_or_else(|error| panic!("receipt decode: {error:?}"));
    let receipt = decoded
        .protocol()
        .unwrap_or_else(|| panic!("protocol receipt required"));
    let resource = hex(&receipt.activity_id());
    let principal = sha256_hex(b"payment-principal");
    let key = KeyRecord {
        key_id: "payment-key".to_owned(),
        principal_digest: principal.clone(),
        salt: "payment-salt".to_owned(),
        secret_digest: sha256_hex(b"test-key"),
        signer_public_key: "11".repeat(32),
        scopes: "activity:submit".to_owned(),
        quota_requests: 100,
        quota_window_seconds: 60,
        epoch: 1,
        disabled: false,
    };
    store
        .issue_key(&key, "payment-key-issued")
        .unwrap_or_else(|error| panic!("key persistence: {error}"));
    let reserved = store
        .reserve(
            &key,
            ReservationRequest {
                idempotency_scope: "payment-operation",
                request_digest: "payment-request",
                now: 100,
                retention_seconds: 3600,
                activity_id: &resource,
                protocol_idempotency_key: "payment-idempotency",
                principal_digest: &principal,
                audit_event: "payment-reserved",
                continuation: "",
            },
        )
        .unwrap_or_else(|error| panic!("reservation: {error}"));
    assert!(matches!(reserved, Reservation::Reserved));
    let observation = Observation {
        kind: "payment".to_owned(),
        id: event_id("payment", &resource, 1),
        principal: None,
        principal_digest: Some(principal.clone()),
        resource: resource.clone(),
        sequence: 1,
        source_sequence: receipt.global_sequence(),
        occurred_at: receipt.timestamp(),
        facts: vec![Fact {
            name: "result_code".to_owned(),
            value: receipt.result_code().to_string(),
        }],
        activity_id: Some(resource.clone()),
        amount: Some(receipt.amount().to_string()),
        asset: Some(hex(&receipt.asset())),
    };
    let pending = Pending::new(observation).unwrap_or_else(|error| panic!("observation: {error}"));
    let completion = Completion {
        idempotency_scope: "payment-operation",
        request_digest: "payment-request",
        state: "completed",
        response_hex: "7b7d",
        receipt_hex,
        activity_id: Some(&resource),
        principal_digest: &principal,
        audit_event: "payment-completed",
    };
    assert!(store
        .complete_observed(
            Completion {
                principal_digest: "foreign",
                ..completion
            },
            &pending
        )
        .is_err());
    assert!(store
        .pending()
        .unwrap_or_else(|error| panic!("pending: {error}"))
        .is_none());
    store
        .complete_observed(completion, &pending)
        .unwrap_or_else(|error| panic!("atomic completion: {error}"));
    drop(store);
    assert_payment_restart(&redis, &pending, completion, receipt_hex);
}

fn assert_payment_restart(
    redis: &RedisProcess,
    pending: &layerx_platform_internal::producer::Pending,
    completion: layerx_platform_gateway::store::Completion<'_>,
    receipt_hex: &str,
) {
    use layerx_platform_internal::producer::{Outbox, Pending};
    let store = redis.store();
    assert_eq!(
        store
            .pending()
            .unwrap_or_else(|error| panic!("pending: {error}")),
        Some(pending.clone())
    );
    assert_eq!(
        store
            .operation("payment-operation")
            .unwrap_or_else(|error| panic!("operation: {error}"))
            .unwrap_or_else(|| panic!("operation missing"))
            .receipt,
        receipt_hex
    );
    assert!(store.acknowledge(&pending.observation.id, false).is_err());
    assert!(store.acknowledge(&"00".repeat(32), true).is_err());
    store
        .acknowledge(&pending.observation.id, true)
        .unwrap_or_else(|error| panic!("sink acknowledgement: {error}"));
    drop(store);
    let store = redis.store();
    let mut observed = pending.clone();
    observed.observed = true;
    assert_eq!(
        store
            .pending()
            .unwrap_or_else(|error| panic!("pending: {error}")),
        Some(observed)
    );
    store
        .acknowledge(&pending.observation.id, false)
        .unwrap_or_else(|error| panic!("webhook acknowledgement: {error}"));
    store
        .complete_observed(completion, pending)
        .unwrap_or_else(|error| panic!("identical completion retry: {error}"));
    assert!(store
        .pending()
        .unwrap_or_else(|error| panic!("pending: {error}"))
        .is_none());
    let mut changed = pending.observation.clone();
    "altered".clone_into(&mut changed.facts[0].value);
    let changed =
        Pending::new(changed).unwrap_or_else(|error| panic!("changed observation: {error}"));
    assert!(store.complete_observed(completion, &changed).is_err());
}

#[path = "support/payment_events.rs"]
mod payment_events;
