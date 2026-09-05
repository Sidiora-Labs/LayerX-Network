#[allow(dead_code)]
mod support;

use std::collections::BTreeMap;
use std::fmt::Debug;
use std::fs;
use std::os::unix::fs::PermissionsExt as _;
use std::sync::Arc;
use std::thread;
use std::time::Duration;

use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use base64::Engine as _;
use ciborium::value::Value as CborValue;
use ed25519_dalek::{Signer as _, SigningKey};
use layerx_human_service::auth::{AuthConfig, Passkeys, RateLimit};
use layerx_human_service::server::schema::ApiSchema;
use layerx_human_service::server::{
    default_component_limits, AuthorizationGrantPolicy, ComponentServerConfig, HumanApiComponents,
    HumanComponentServer, IdentityServices, PrivilegedHumanComponents, ProvisionedAccount,
    ProvisionedAccounts, ScopedRequest, UnixComponents,
};
use serde_json::{json, Value};
use sha2::{Digest as _, Sha256};

use support::{directory, install_and_open, retention_uniform, tenancy};

const RP_ID: &str = "id.layerx.example";
const ORIGIN: &str = "https://id.layerx.example";
const ACCOUNT_ID: &str = "act_00112233445566778899aabbccddeeff";
const EMAIL: &str = "mara@example.com";
const PUBLIC_TRACE: &str = "trc_00112233445566778899aabbccddeeff";
const AUTHORIZE_TRACE: &str = "trc_ffeeddccbbaa99887766554433221100";
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
) -> layerx_human_service::server::BackendResponse {
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

fn digest_field(digest: &mut Sha256, value: &[u8]) {
    digest.update(u64::try_from(value.len()).unwrap_or(u64::MAX).to_be_bytes());
    digest.update(value);
}

fn authorized_request_digest(
    operation: &layerx_human_service::server::schema::Operation,
    destination: &str,
    path_parameters: &BTreeMap<String, String>,
    body: &Value,
    trace: &str,
) -> [u8; 32] {
    let mut digest = Sha256::new();
    digest.update(b"layerx-human/authorized-operation/v1\0");
    digest_field(&mut digest, operation.name.as_bytes());
    digest_field(&mut digest, operation.method.as_bytes());
    digest_field(&mut digest, destination.as_bytes());
    for (name, value) in path_parameters {
        digest_field(&mut digest, name.as_bytes());
        digest_field(&mut digest, value.as_bytes());
    }
    let body = required(serde_json::to_vec(body), "encode authorized body");
    digest_field(&mut digest, &body);
    digest_field(&mut digest, b"");
    digest_field(&mut digest, trace.as_bytes());
    digest.finalize().into()
}

#[test]
fn real_component_boundary_registers_asserts_opens_and_authorizes_session() {
    let store_root = directory("identity-component-store");
    let map = tenancy(&[(ACCOUNT_ID, "tenant-mara")]);
    let (store, _) = install_and_open(&store_root, &map, retention_uniform(86_400));
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
    let mut authenticator = SoftwareAuthenticator::new();

    let registration = public_call(
        &client,
        &schema,
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
        &client,
        &schema,
        "passkey.register.finish",
        BTreeMap::from([("registration_id".to_owned(), registration_id.to_owned())]),
        json!({ "credential": credential }),
    );

    let assertion = public_call(
        &client,
        &schema,
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
        &client,
        &schema,
        "passkey.assert.finish",
        BTreeMap::from([("assertion_id".to_owned(), assertion_id.to_owned())]),
        json!({ "credential": assertion_credential }),
    );
    let mut opened = public_call(
        &client,
        &schema,
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

    let list_operation = schema
        .operation("session.list")
        .unwrap_or_else(|| panic!("session.list operation"));
    let list_path = BTreeMap::new();
    let list_body = json!({});
    let disclosure_digest = Sha256::digest(required(
        serde_json::to_vec(&list_body),
        "encode session list disclosure",
    ))
    .into();
    let request_digest = authorized_request_digest(
        list_operation,
        "/v1/sessions",
        &list_path,
        &list_body,
        AUTHORIZE_TRACE,
    );
    let principal = required(
        client.authorize(
            list_operation,
            layerx_human_service::server::backend::SessionCredentials {
                access_token: &session.access_token,
                csrf_token: None,
                intended_destination: "/v1/sessions",
                refresh: false,
                request_digest,
                disclosure_digest,
                path_parameters: &list_path,
                body: &list_body,
                idempotency_key: None,
            },
            AUTHORIZE_TRACE,
        ),
        "component authorization",
    );
    assert_eq!(principal.principal.as_str(), ACCOUNT_ID);
    let listed = required(
        client.execute(ScopedRequest {
            operation: list_operation,
            principal: Some(principal),
            path_parameters: list_path,
            body: list_body,
            idempotency_key: None,
            trace: AUTHORIZE_TRACE.to_owned(),
        }),
        "list sessions",
    );
    assert_eq!(listed.result["sessions"][0]["current"], true);

    verify_principal_route(&socket_path, &session.access_token);

    shutdown.request();
    let served = server
        .join()
        .unwrap_or_else(|_| panic!("component server panicked"));
    required(served, "component server");
    thread::sleep(Duration::from_millis(1));
}

fn verify_principal_route(socket: &std::path::Path, access_token: &str) {
    use layerx_human_service::server::{HttpConfig, PrincipalLimits, Router};
    use std::io::{Read as _, Write as _};
    let backend = Arc::new(required(
        UnixComponents::new(socket, default_component_limits()),
        "principal client",
    ));
    let router = Arc::new(required(
        Router::new(
            backend,
            required(PrincipalLimits::new(100, 60, 100), "limits"),
            HttpConfig {
                maximum_header_bytes: 32768,
                maximum_body_bytes: 1048576,
                allowed_origin: ORIGIN.to_owned(),
                service_version: "test".to_owned(),
            },
        ),
        "principal router",
    ));
    for (credential, expected_status) in [(access_token, 200), ("invalid-session", 401), ("", 401)]
    {
        let (mut client, mut server) =
            required(std::os::unix::net::UnixStream::pair(), "HTTP pair");
        let shared = Arc::clone(&router);
        let worker = thread::spawn(move || {
            required(
                shared.serve_one(&mut server, "principal-test"),
                "route principal",
            )
        });
        let cookie = if credential.is_empty() {
            String::new()
        } else {
            format!("Cookie: __Host-layerx_access={credential}\r\n")
        };
        required(write!(client, "GET /internal/v1/principal HTTP/1.1\r\nHost: id.layerx.example\r\n{cookie}X-LayerX-Principal: another-principal\r\nContent-Length: 0\r\n\r\n"), "principal request");
        let mut response = String::new();
        required(client.read_to_string(&mut response), "principal response");
        worker.join().unwrap_or_else(|_| panic!("principal worker"));
        assert!(
            response.starts_with(&format!("HTTP/1.1 {expected_status}")),
            "{response}"
        );
        assert!(response
            .to_ascii_lowercase()
            .contains("cache-control: no-store"));
        if expected_status == 200 {
            let (_, body) = response
                .split_once("\r\n\r\n")
                .unwrap_or_else(|| panic!("HTTP body"));
            let body: Value = required(serde_json::from_str(body), "principal JSON");
            assert_eq!(body["result"], json!({"active":true,"sub":ACCOUNT_ID}));
        }
    }
}
