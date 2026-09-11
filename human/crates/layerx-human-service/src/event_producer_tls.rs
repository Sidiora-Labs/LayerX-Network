use crate::store::PrincipalStore;
use layerx_platform_internal::{
    events::{Kind, ProducerCredential, Service},
    http,
    producer::{Client, Health, Outbox},
};
use std::collections::BTreeMap;
use std::fmt::Debug;
use std::fs;
use std::os::unix::fs::PermissionsExt as _;
use std::sync::Arc;
use std::sync::Mutex;
use std::thread;
use std::time::Duration;
use zeroize::Zeroizing;

use crate::auth::{AuthConfig, Passkeys, RateLimit};
use crate::server::schema::ApiSchema;
use crate::server::{
    default_component_limits, AuthorizationGrantPolicy, ComponentServerConfig, HumanApiComponents,
    HumanComponentServer, IdentityServices, PrivilegedHumanComponents, ProvisionedAccount,
    ProvisionedAccounts, ScopedRequest, UnixComponents,
};
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use base64::Engine as _;
use ciborium::value::Value as CborValue;
use ed25519_dalek::{Signer as _, SigningKey};
use serde_json::{json, Value};
use sha2::{Digest as _, Sha256};

const RP_ID: &str = "id.layerx.example";
const ORIGIN: &str = "https://id.layerx.example";
const ACCOUNT_ID: &str = "act_00112233445566778899aabbccddeeff";
const EMAIL: &str = "mara@example.com";
const PUBLIC_TRACE: &str = "trc_00112233445566778899aabbccddeeff";
const FLAG_UP: u8 = 1 << 0;
const FLAG_UV: u8 = 1 << 2;
const FLAG_AT: u8 = 1 << 6;

fn required<T, E: Debug>(result: Result<T, E>, label: &str) -> T {
    result.unwrap_or_else(|error| panic!("{label}: {error:?}"))
}

fn auth_config() -> AuthConfig {
    AuthConfig {
        rp_id: RP_ID.to_owned(),
        rp_name: "LayerX".to_owned(),
        origin: ORIGIN.to_owned(),
        ceremony_ttl_secs: 300,
        assertion_ttl_secs: 60,
        session_ttl_secs: 300,
        refresh_ttl_secs: 3_600,
        step_up_ttl_secs: 60,
        rate_limit: RateLimit {
            attempts: 100,
            window_secs: 60,
        },
    }
}

struct SoftwareAuthenticator {
    signing_key: SigningKey,
    credential_id: Vec<u8>,
    counter: u32,
    user_handle: Option<String>,
}

impl SoftwareAuthenticator {
    fn new() -> Self {
        let mut seed = [0_u8; 32];
        required(getrandom::fill(&mut seed), "authenticator entropy");
        let mut credential_id = vec![0_u8; 32];
        required(
            getrandom::fill(&mut credential_id),
            "credential identifier entropy",
        );
        Self {
            signing_key: SigningKey::from_bytes(&seed),
            credential_id,
            counter: 0,
            user_handle: None,
        }
    }

    fn register(&mut self, ceremony: &str) -> String {
        let options = decode_ceremony(ceremony);
        let challenge = required_text(&options, "/challenge");
        self.user_handle = Some(required_text(&options, "/user/id").to_owned());
        let client_data = client_data("webauthn.create", challenge);
        encode_response(&json!({
            "id": URL_SAFE_NO_PAD.encode(&self.credential_id),
            "transports": ["internal"],
            "attestationObject": URL_SAFE_NO_PAD.encode(self.attestation_object()),
            "clientDataJSON": URL_SAFE_NO_PAD.encode(client_data),
        }))
    }

    fn assert(&mut self, ceremony: &str) -> String {
        let options = decode_ceremony(ceremony);
        let challenge = required_text(&options, "/challenge");
        self.counter = self.counter.saturating_add(1);
        let authenticator_data = self.authenticator_data(self.counter, false);
        let client_data = client_data("webauthn.get", challenge);
        let client_hash = Sha256::digest(&client_data);
        let mut signed = Vec::with_capacity(authenticator_data.len() + client_hash.len());
        signed.extend_from_slice(&authenticator_data);
        signed.extend_from_slice(&client_hash);
        let signature = self.signing_key.sign(&signed).to_bytes();
        encode_response(&json!({
            "id": URL_SAFE_NO_PAD.encode(&self.credential_id),
            "authenticatorData": URL_SAFE_NO_PAD.encode(authenticator_data),
            "signature": URL_SAFE_NO_PAD.encode(signature),
            "clientDataJSON": URL_SAFE_NO_PAD.encode(client_data),
            "userHandle": self.user_handle,
        }))
    }

    fn attestation_object(&self) -> Vec<u8> {
        let map = CborValue::Map(vec![
            (
                CborValue::Text("fmt".to_owned()),
                CborValue::Text("none".to_owned()),
            ),
            (
                CborValue::Text("attStmt".to_owned()),
                CborValue::Map(Vec::new()),
            ),
            (
                CborValue::Text("authData".to_owned()),
                CborValue::Bytes(self.authenticator_data(0, true)),
            ),
        ]);
        let mut bytes = Vec::new();
        required(
            ciborium::ser::into_writer(&map, &mut bytes),
            "encode attestation",
        );
        bytes
    }

    fn authenticator_data(&self, counter: u32, attested: bool) -> Vec<u8> {
        let mut bytes = Vec::new();
        bytes.extend_from_slice(&Sha256::digest(RP_ID.as_bytes()));
        bytes.push(FLAG_UP | FLAG_UV | if attested { FLAG_AT } else { 0 });
        bytes.extend_from_slice(&counter.to_be_bytes());
        if attested {
            bytes.extend_from_slice(&[0_u8; 16]);
            let length = u16::try_from(self.credential_id.len())
                .unwrap_or_else(|_| panic!("credential identifier too long"));
            bytes.extend_from_slice(&length.to_be_bytes());
            bytes.extend_from_slice(&self.credential_id);
            bytes.extend_from_slice(&self.cose_public_key());
        }
        bytes
    }

    fn cose_public_key(&self) -> Vec<u8> {
        let map = CborValue::Map(vec![
            (CborValue::Integer(1.into()), CborValue::Integer(1.into())),
            (
                CborValue::Integer(3.into()),
                CborValue::Integer((-8).into()),
            ),
            (
                CborValue::Integer((-1).into()),
                CborValue::Integer(6.into()),
            ),
            (
                CborValue::Integer((-2).into()),
                CborValue::Bytes(self.signing_key.verifying_key().to_bytes().to_vec()),
            ),
        ]);
        let mut bytes = Vec::new();
        required(
            ciborium::ser::into_writer(&map, &mut bytes),
            "encode public key",
        );
        bytes
    }
}

fn decode_ceremony(value: &str) -> Value {
    let bytes = required(URL_SAFE_NO_PAD.decode(value), "decode ceremony");
    required(serde_json::from_slice(&bytes), "parse ceremony")
}

fn required_text<'value>(value: &'value Value, pointer: &str) -> &'value str {
    value
        .pointer(pointer)
        .and_then(Value::as_str)
        .unwrap_or_else(|| panic!("missing ceremony field {pointer}"))
}

fn encode_response(value: &Value) -> String {
    URL_SAFE_NO_PAD.encode(required(serde_json::to_vec(value), "serialize response"))
}

fn client_data(kind: &str, challenge: &str) -> Vec<u8> {
    required(
        serde_json::to_vec(&json!({
            "type": kind,
            "challenge": challenge,
            "origin": ORIGIN,
            "crossOrigin": false,
        })),
        "encode client data",
    )
}

fn public_call(
    client: &UnixComponents,
    schema: &ApiSchema,
    operation: &str,
    path_parameters: BTreeMap<String, String>,
    body: Value,
) -> crate::server::BackendResponse {
    let operation = schema
        .operation(operation)
        .unwrap_or_else(|| panic!("operation {operation}"));
    let idempotency_key = operation
        .idempotency
        .then(|| format!("identity-boundary-{}", operation.name.replace('.', "-")));
    required(
        client.execute(ScopedRequest {
            operation,
            principal: None,
            path_parameters,
            body,
            idempotency_key,
            trace: PUBLIC_TRACE.to_owned(),
        }),
        "public component call",
    )
}

fn directory(label: &str) -> std::path::PathBuf {
    std::env::temp_dir().join(format!("r29-{label}-{}", std::process::id()))
}
fn retention_uniform(seconds: u64) -> crate::store::RetentionPolicy {
    let period = crate::store::RetentionPeriod::new(seconds);
    crate::store::RetentionPolicy {
        journeys: period,
        notifications: period,
        audit: period,
        telemetry: period,
        cache: period,
    }
}
fn tenancy() -> crate::store::TenancyMap {
    required(
        crate::store::TenancyMap::new([(
            required(crate::store::PrincipalId::new(ACCOUNT_ID), "principal"),
            required(crate::store::AgentTenantId::new("tenant-mara"), "tenant"),
        )]),
        "tenancy",
    )
}

#[path = "../../../../platform/hosted/internal/src/producer_test_support.rs"]
mod transport;

#[test]
fn human_journey_and_approval_cross_tls_and_recover_after_sink_loss() {
    let store_root = directory("identity-component-store");
    let map = tenancy();
    let digest = required(map.install(&store_root), "install");
    let store = required(
        crate::store::PrincipalStore::open(&store_root, retention_uniform(86_400), digest),
        "store",
    );
    let passkeys = required(Passkeys::new(auth_config()), "passkeys");
    let account = required(
        ProvisionedAccount::new(ACCOUNT_ID, EMAIL, "Mara", "Primary passkey"),
        "provisioned account",
    );
    let accounts = required(ProvisionedAccounts::new([account]), "account directory");
    let services = IdentityServices::new(accounts);
    let components = required(
        PrivilegedHumanComponents::new(
            store,
            passkeys,
            services,
            AuthorizationGrantPolicy {
                lifetime_seconds: 30,
                maximum_outstanding: 32,
            },
        ),
        "privileged components",
    );
    let socket_root = directory("identity-component-socket");
    required(fs::create_dir_all(&socket_root), "socket directory");
    required(
        fs::set_permissions(&socket_root, fs::Permissions::from_mode(0o700)),
        "socket permissions",
    );
    let socket_path = socket_root.join("human.sock");
    let bound = required(
        HumanComponentServer::new(Arc::new(components)).bind(ComponentServerConfig {
            socket_path: socket_path.clone(),
            allowed_uid: rustix::process::getuid().as_raw(),
            worker_count: 1,
            queue_capacity: 1,
            limits: default_component_limits(),
        }),
        "bind component server",
    );
    let shutdown = bound.shutdown();
    let server = thread::spawn(move || bound.run());
    let client = required(
        UnixComponents::new(&socket_path, default_component_limits()),
        "component client",
    );
    let schema = required(ApiSchema::v1(), "schema");
    let session = open_session(&client, &schema);
    verify_delivery(&socket_path, &session.access_token);
    shutdown.request();
    required(
        server.join().unwrap_or_else(|_| panic!("component thread")),
        "component server",
    );
}

fn open_session(
    client: &UnixComponents,
    schema: &ApiSchema,
) -> crate::server::backend::SessionSecrets {
    let mut authenticator = SoftwareAuthenticator::new();

    let registration = public_call(
        client,
        schema,
        "passkey.register.begin",
        BTreeMap::new(),
        json!({ "account_id": ACCOUNT_ID }),
    );
    let registration_id = registration.result["registration_id"]
        .as_str()
        .unwrap_or_else(|| panic!("registration identifier"));
    assert!(registration_id.starts_with("reg_"));
    let credential = authenticator.register(
        registration.result["ceremony"]
            .as_str()
            .unwrap_or_else(|| panic!("registration ceremony")),
    );
    public_call(
        client,
        schema,
        "passkey.register.finish",
        BTreeMap::from([("registration_id".to_owned(), registration_id.to_owned())]),
        json!({ "credential": credential }),
    );

    let assertion = public_call(
        client,
        schema,
        "passkey.assert.begin",
        BTreeMap::new(),
        json!({ "email": EMAIL }),
    );
    let assertion_id = assertion.result["assertion_id"]
        .as_str()
        .unwrap_or_else(|| panic!("assertion identifier"));
    let assertion_credential = authenticator.assert(
        assertion.result["ceremony"]
            .as_str()
            .unwrap_or_else(|| panic!("assertion ceremony")),
    );
    public_call(
        client,
        schema,
        "passkey.assert.finish",
        BTreeMap::from([("assertion_id".to_owned(), assertion_id.to_owned())]),
        json!({ "credential": assertion_credential }),
    );
    let mut opened = public_call(
        client,
        schema,
        "session.open",
        BTreeMap::new(),
        json!({
            "assertion_id": assertion_id,
            "device": { "label": "LayerX web app", "platform": "web" }
        }),
    );
    let session = opened
        .session
        .take()
        .unwrap_or_else(|| panic!("protected session secrets"));
    assert_eq!(opened.result["device"]["platform"], "web");

    session
}

fn verify_delivery(socket: &std::path::Path, access_token: &str) {
    let root = directory("human-event-tls");
    let tls = transport::Tls::new(&root);
    let human = human_listener(&tls, socket);
    let source = |kind, port| human_source(&tls, &root, human.port, access_token, kind, port);
    let journey = source(Kind::Journey, 0);
    let approval = source(Kind::Approval, 0);
    let mut webhook = transport::Webhooks::start(
        &root,
        &[("JOURNEY", journey.port), ("APPROVAL", approval.port)],
    );
    let client = || {
        required(
            Client::new(
                BTreeMap::from([
                    (
                        "journey".to_owned(),
                        tls.upstream(journey.port, "producer-token"),
                    ),
                    (
                        "approval".to_owned(),
                        tls.upstream(approval.port, "producer-token"),
                    ),
                ]),
                tls.upstream(webhook.port, "notification-token"),
            ),
            "producer client",
        )
    };
    let event_root = root.join("producer");
    let (outbox, retention, digest) = event_store(&event_root);
    let store = Arc::clone(&outbox.store);
    let health = Arc::clone(&outbox.health);
    let producer = client();
    let journey = qualify_journey(&producer, outbox.as_ref(), &mut webhook, journey, &source);
    let second =
        required(outbox.pending(), "approval pending").unwrap_or_else(|| panic!("approval"));
    assert_eq!(second.observation.kind, "approval");
    let mut foreign = second.observation.clone();
    foreign.principal = Some("foreign-principal".to_owned());
    assert_eq!(
        required(
            tls.upstream(approval.port, "producer-token").post(
                "/internal/v1/observe",
                &required(foreign.encode(), "foreign encoding")
            ),
            "foreign response"
        )
        .status,
        403
    );
    let approval_port = approval.port;
    drop(approval);
    required(
        producer.spawn(Arc::downgrade(&outbox), Arc::clone(&health)),
        "worker",
    );
    let deadline = std::time::Instant::now() + Duration::from_secs(36);
    while health.ready() && std::time::Instant::now() < deadline {
        thread::sleep(Duration::from_millis(100));
    }
    assert!(!outbox.ready());
    assert_eq!(
        required(outbox.pending(), "sink loss queue"),
        Some(second.clone())
    );
    let approval = source(Kind::Approval, approval_port);
    let deadline = std::time::Instant::now() + Duration::from_secs(10);
    while required(outbox.pending(), "recovery queue").is_some()
        && std::time::Instant::now() < deadline
    {
        thread::sleep(Duration::from_millis(100));
    }
    assert!(required(outbox.pending(), "delivered").is_none());
    assert!(outbox.ready());
    let response = required(
        tls.upstream(approval.port, "consumer-token")
            .get(&format!("/internal/v1/events/{}", second.observation.id)),
        "read approval",
    );
    assert_eq!(response.status, 200);
    let record: layerx_platform_internal::events::Record =
        required(serde_json::from_slice(&response.body), "approval record");
    assert_eq!(record, second.observation.record(ACCOUNT_ID.to_owned()));
    drop(outbox);
    thread::sleep(Duration::from_secs(2));
    drop(store);
    let reopened = super::HumanOutbox {
        store: Arc::new(Mutex::new(required(
            PrincipalStore::open(&event_root, retention, digest),
            "reopened producer",
        ))),
        health,
    };
    assert!(required(reopened.pending(), "durable acknowledgements").is_none());
    drop(reopened);
    drop(approval);
    drop(journey);
    drop(webhook);
    drop(human);
}

fn enqueue_journey(scope: &mut crate::store::PrincipalScope<'_>) {
    use crate::journeys::{JourneyEngine, JourneyKind, JourneyLeg, JourneyPlan};
    use layerx_types::intent::{
        AuthorizationSignature, ContextHash, NetworkId, ProtocolVersion, PublicKey,
        SendAuthorization, SendAuthorizationKind, Sequence, TimestampSeconds,
    };
    use layerx_types::payload::{ActivityType, ModuleId, ModuleRegistration, ModuleRegistry};
    let send = required(
        layerx_intents::LxpSend::new(
            required(
                layerx_types::account::AccountId::parse("agent:did:layerx:alice:main"),
                "sender",
            ),
            required(
                layerx_types::account::AccountId::parse("agent:did:layerx:recipient:main"),
                "recipient",
            ),
            layerx_types::ids::AssetId::new([0x33; 32]),
            layerx_types::amount::Amount::from_u128(1),
            Sequence::from_u64(7),
            layerx_types::ids::IdempotencyKey::new([0x21; 32]),
            TimestampSeconds::from_u64(1010),
            ContextHash::new([0x55; 32]),
            SendAuthorization::new(
                SendAuthorizationKind::Owner,
                PublicKey::new([0x66; 32]),
                AuthorizationSignature::new([0x77; 64]),
            ),
            required(NetworkId::new(77), "network"),
            required(
                ProtocolVersion::new(layerx_wire::limits::PROTOCOL_VERSION),
                "protocol",
            ),
        ),
        "send intent",
    );
    let leg = required(
        JourneyLeg::new(
            layerx_intents::Intent::v1(layerx_intents::IntentKind::LxpSend(send)),
            [0x21; 32],
            required(
                layerx_agent_api::identity::AgentDid::new("did:layerx:alice"),
                "actor",
            ),
            required(
                layerx_agent_api::identity::AuthorityRef::new("custody-human-primary"),
                "authority",
            ),
            7,
            995,
            1010,
            7,
        ),
        "journey leg",
    );
    let plan = required(
        JourneyPlan::new(
            required(
                crate::notify::JourneyId::new("jrn_producertls"),
                "journey id",
            ),
            JourneyKind::Move,
            [0x31; 32],
            required(crate::custody::KeyId::new("human-primary"), "custody key"),
            crate::custody::Operation::ProtocolMutation,
            vec![leg],
        ),
        "journey plan",
    );
    let module = required(
        ModuleRegistration::new(
            ModuleId::Asset,
            &[
                required(ActivityType::new(ModuleId::Asset, 5), "send type"),
                required(ActivityType::new(ModuleId::Asset, 6), "receive type"),
            ],
        ),
        "asset module",
    );
    let registry = required(ModuleRegistry::new(&[module]), "registry");
    required(
        JourneyEngine::start(scope, &plan, &registry, 100),
        "journey transition",
    );
}

fn human_source(
    tls: &transport::Tls,
    root: &std::path::Path,
    human_port: u16,
    access_token: &str,
    kind: Kind,
    port: u16,
) -> transport::Listener {
    let service = required(
        Service::open(
            kind,
            tls.upstream(human_port, "unused"),
            BTreeMap::from([(
                ACCOUNT_ID.to_owned(),
                Zeroizing::new(access_token.to_owned()),
            )]),
            Zeroizing::new("consumer-token".to_owned()),
            &root.join(kind.singular()),
        )
        .and_then(|service| {
            service.with_producers(vec![ProducerCredential {
                token: Zeroizing::new("producer-token".to_owned()),
                allow_principal_digest: false,
            }])
        }),
        "source",
    );
    transport::Listener::start(Arc::clone(&tls.config), port, move |stream| {
        if let Ok(mut request) = http::parse_client_request(stream) {
            request.peer_verified = stream
                .conn
                .peer_certificates()
                .is_some_and(|certs| !certs.is_empty());
            let _ = http::write_response(stream, &service.route(&request));
        }
    })
}

fn event_store(
    event_root: &std::path::Path,
) -> (
    Arc<super::HumanOutbox>,
    crate::store::RetentionPolicy,
    crate::store::TenancyDigest,
) {
    let map = tenancy();
    let digest = required(map.install(event_root), "event tenancy");
    let retention = retention_uniform(86400);
    let store = Arc::new(Mutex::new(required(
        PrincipalStore::open(event_root, retention, digest),
        "event store",
    )));
    {
        let mut store = store
            .lock()
            .unwrap_or_else(|error| panic!("store lock: {error:?}"));
        let mut scope = required(
            store.principal(&required(
                crate::store::PrincipalId::new(ACCOUNT_ID),
                "principal",
            )),
            "scope",
        );
        enqueue_journey(&mut scope);
        let journal = crate::server::stream_journal::StreamJournal::new(
            [1; 32],
            layerx_proof::checkpoint::SettlementDomain::new(31337, [2; 20]),
        );
        required(journal.append(&mut scope, "approval:r29:pending", "approval-created", 102, json!({"approval":{"approval_id":"approval-r29", "agent_id":"agent-r29", "state":"pending", "created_at":102}})), "approval transition");
    }
    let health = Arc::new(Health::default());
    let outbox = Arc::new(super::HumanOutbox {
        store: Arc::clone(&store),
        health: Arc::clone(&health),
    });
    (outbox, retention, digest)
}

fn qualify_journey(
    producer: &Client,
    outbox: &super::HumanOutbox,
    webhook: &mut transport::Webhooks,
    journey: transport::Listener,
    source: &impl Fn(Kind, u16) -> transport::Listener,
) -> transport::Listener {
    let first = required(outbox.pending(), "pending").unwrap_or_else(|| panic!("journey pending"));
    assert_eq!(first.observation.kind, "journey");
    let mut foreign = first.clone();
    foreign.observation.principal = Some("foreign-principal".to_owned());
    foreign.body = required(foreign.observation.encode(), "foreign journey encoding");
    assert_eq!(
        producer.deliver(&foreign),
        Err("event observation refused: 403".to_owned())
    );
    let journey_port = journey.port;
    drop(journey);
    assert!(producer.step(outbox, &outbox.health).is_err());
    thread::sleep(Duration::from_secs(31));
    assert!(!outbox.ready());
    assert_eq!(
        required(outbox.pending(), "journey sink loss queue"),
        Some(first.clone())
    );
    let journey = source(Kind::Journey, journey_port);
    assert_eq!(producer.deliver(&first), Ok(true));
    assert_eq!(producer.deliver(&first), Ok(true));
    required(producer.step(outbox, &outbox.health), "observe ack");
    assert!(outbox.ready());
    let observed =
        required(outbox.pending(), "observed").unwrap_or_else(|| panic!("pending notification"));
    webhook.stop();
    assert!(producer.deliver(&observed).is_err());
    assert_eq!(
        required(outbox.pending(), "retained notification"),
        Some(observed.clone())
    );
    webhook.restart();
    assert_eq!(producer.deliver(&observed), Ok(false));
    assert_eq!(producer.deliver(&observed), Ok(false));
    required(
        outbox.acknowledge(&observed.observation.id, false),
        "notification ack",
    );
    journey
}

fn human_listener(tls: &transport::Tls, socket: &std::path::Path) -> transport::Listener {
    use crate::server::{HttpConfig, PrincipalLimits, Router};
    let router = Arc::new(required(
        Router::new(
            Arc::new(required(
                UnixComponents::new(socket, default_component_limits()),
                "backend",
            )),
            required(PrincipalLimits::new(10000, 60, 100), "limits"),
            HttpConfig {
                maximum_header_bytes: 32768,
                maximum_body_bytes: 1_048_576,
                allowed_origin: ORIGIN.to_owned(),
                service_version: "integration".to_owned(),
            },
        ),
        "router",
    ));
    transport::Listener::start(Arc::clone(&tls.config), 0, move |stream| {
        let _ = router.serve_one(stream, "producer-integration");
    })
}
