#[path = "support/producer_redis.rs"]
mod producer_redis;
#[path = "../../../../programs/crates/layerx-programs-registry/tests/support/mod.rs"]
pub mod support;
#[path = "../../internal/src/producer_test_support.rs"]
mod transport;

use layerx_platform_internal::{
    events::{Kind, ProducerCredential, Service},
    gateway_http, http,
    principal::PrincipalClient,
    producer::{Client, Outbox},
    secret::sha256_hex,
};
use layerx_platform_registry::event_producer::ProgramOutbox;
use std::collections::BTreeMap;
use std::fs;
use std::path::Path;
use std::sync::Arc;
use std::time::Duration;
use zeroize::Zeroizing;

struct IdentityServer(std::process::Child);
impl Drop for IdentityServer {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

fn identity(root: &Path, tls: &transport::Tls) -> (IdentityServer, PrincipalClient) {
    let tokens = root.join("identity-tokens");
    fs::create_dir_all(&tokens).unwrap_or_else(|error| panic!("tokens directory: {error:?}"));
    for service in [
        "gateway",
        "registry",
        "webhooks",
        "dashboard",
        "faucet",
        "testnet",
        "ramp",
        "provisioning",
    ] {
        fs::write(tokens.join(service), format!("{service}-integration-token"))
            .unwrap_or_else(|error| panic!("service token: {error:?}"));
    }
    fs::write(root.join("store-key"), "identity-integration-store-key")
        .unwrap_or_else(|error| panic!("store key: {error:?}"));
    let listener = std::net::TcpListener::bind("127.0.0.1:0")
        .unwrap_or_else(|error| panic!("port: {error:?}"));
    let port = listener
        .local_addr()
        .unwrap_or_else(|error| panic!("address: {error:?}"))
        .port();
    drop(listener);
    let child = std::process::Command::new(
        std::env::var_os("LAYERX_TEST_IDENTITY_BIN")
            .unwrap_or_else(|| panic!("identity binary required")),
    )
    .env_clear()
    .env("LAYERX_IDENTITY_LISTEN", format!("127.0.0.1:{port}"))
    .env("LAYERX_IDENTITY_TLS_CERT_DER", root.join("cert.der"))
    .env("LAYERX_IDENTITY_TLS_KEY_DER", root.join("key.der"))
    .env("LAYERX_IDENTITY_CLIENT_CA_DER", root.join("cert.der"))
    .env("LAYERX_IDENTITY_STATE_DIR", root.join("identity-state"))
    .env("LAYERX_IDENTITY_SERVICE_TOKENS_DIR", tokens)
    .env("LAYERX_IDENTITY_STORE_KEY_FILE", root.join("store-key"))
    .stdout(std::process::Stdio::null())
    .stderr(std::process::Stdio::null())
    .spawn()
    .unwrap_or_else(|error| panic!("identity spawn: {error:?}"));
    let mut server = IdentityServer(child);
    for _ in 0..100 {
        assert!(server
            .0
            .try_wait()
            .unwrap_or_else(|error| panic!("identity status: {error:?}"))
            .is_none());
        if tls
            .upstream(port, "registry-integration-token")
            .get("/livez")
            .is_ok_and(|reply| reply.status == 200)
        {
            break;
        }
        std::thread::sleep(Duration::from_millis(50));
    }
    let endpoint = gateway_http::Endpoint::parse(&format!("https://localhost:{port}"))
        .unwrap_or_else(|error| panic!("identity endpoint: {error:?}"));
    let client = gateway_http::Client::new(tls.ca.clone(), tls.identity.clone());
    for (path, body, key) in [
        ("/v1/principals", serde_json::json!({"sub":"program-principal", "allowed_signer_public_keys":["ab".repeat(32)]}).to_string(), None),
        ("/v1/publication-keys", serde_json::json!({"sub":"program-principal", "revoked":false}).to_string(), Some("program-key:publication-integration")),
    ] {
        let response = client.request_with_principal(&endpoint, "Bearer provisioning-integration-token", &gateway_http::OutboundRequest { method:"POST", path, idempotency:None, content_type:"application/json", body:body.as_bytes() }, None, key).unwrap_or_else(|error| panic!("provisioning response: {error:?}"));
        assert_eq!(response.status, 200);
    }
    (
        server,
        PrincipalClient::new(
            client,
            endpoint,
            Zeroizing::new("registry-integration-token".to_owned()),
        ),
    )
}

fn gateway(
    tls: &transport::Tls,
    store: Arc<layerx_platform_gateway::store::RedisStore>,
) -> transport::Listener {
    transport::Listener::start(Arc::clone(&tls.config), 0, move |stream| {
        if let Ok(request) = http::parse_client_request(stream) {
            let response = request
                .headers
                .get("authorization")
                .ok_or(layerx_platform_gateway::AccessError::Unauthenticated)
                .and_then(|authorization| {
                    layerx_platform_gateway::gateway_principal(&store, authorization)
                })
                .map_or_else(
                    |_| http::refusal(401, "api_key_required", None),
                    |value| http::json(200, &value),
                );
            let _ = http::write_response(stream, &response);
        }
    })
}

fn publication() -> String {
    let fixture = support::deploy_fixture(
        support::WASM_V1,
        layerx_programs_runtime::UpgradePolicy::Authority(support::AUTHORITY),
        70,
        1_700_000_070,
    );
    let verifier = support::verifier_for_fixture(&fixture, 70, 100, None, 1000);
    let evidence = verifier
        .verify_deployment(&fixture.proof, support::NOW)
        .unwrap_or_else(|error| panic!("verified publication: {error:?}"));
    serde_json::json!({"program_id":layerx_programs::hex::encode(&evidence.program().bytes()), "lifecycle":"active", "versions":[{"version":evidence.version(), "code_hash":layerx_programs::hex::encode(&evidence.code_hash()), "deployment_receipt_digest":layerx_programs::hex::encode(&evidence.receipt_digest())}]}).to_string()
}

#[test]
fn registry_publication_uses_identity_and_delivers_over_tls_with_durable_recovery() {
    let root = std::env::temp_dir().join(format!("registry-producer-tls-{}", std::process::id()));
    let tls = transport::Tls::new(&root);
    let (_identity, identity) = identity(&root, &tls);
    let principal = identity
        .resolve("program-key:publication-integration")
        .unwrap_or_else(|error| panic!("identity resolution: {error:?}"));
    assert_eq!(principal, sha256_hex(b"program-principal"));
    assert!(identity.resolve("unknown-publication-key").is_err());
    let redis = producer_redis::RedisProcess::start();
    let store = Arc::new(redis.store());
    let secret = format!("lxp_live_{}", "12".repeat(32));
    store
        .issue_key(
            &layerx_platform_gateway::store::KeyRecord {
                key_id: "program-key".to_owned(),
                principal_digest: principal.clone(),
                salt: "program-salt".to_owned(),
                secret_digest: layerx_platform_gateway::gateway_digest(&[
                    b"gateway-key-v1",
                    b"program-salt",
                    secret.as_bytes(),
                ]),
                signer_public_key: "ab".repeat(32),
                scopes: "program:read".to_owned(),
                quota_requests: 1000,
                quota_window_seconds: 60,
                epoch: 1,
                disabled: false,
            },
            "program-issue",
        )
        .unwrap_or_else(|error| panic!("gateway key: {error:?}"));
    let gateway = gateway(&tls, store);
    qualify_delivery(&tls, &root, gateway.port, &secret, &principal);
}

fn source(
    tls: &transport::Tls,
    root: &Path,
    gateway_port: u16,
    secret: &str,
    port: u16,
) -> transport::Listener {
    let service = Service::open(
        Kind::Program,
        tls.upstream(gateway_port, "unused"),
        BTreeMap::from([(
            "program-principal".to_owned(),
            Zeroizing::new(format!("program-key:{secret}")),
        )]),
        Zeroizing::new("consumer-token".to_owned()),
        &root.join("source"),
    )
    .and_then(|service| {
        service.with_producers(vec![ProducerCredential {
            token: Zeroizing::new("producer-token".to_owned()),
            allow_principal_digest: true,
        }])
    })
    .unwrap_or_else(|error| panic!("program source: {error:?}"));
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

fn qualify_delivery(
    tls: &transport::Tls,
    root: &Path,
    gateway_port: u16,
    secret: &str,
    principal: &str,
) {
    let source = |port| source(tls, root, gateway_port, secret, port);
    let sink = source(0);
    let mut webhook = transport::Webhooks::start(root, &[("PROGRAM", sink.port)]);
    let client = Client::new(
        BTreeMap::from([(
            "program".to_owned(),
            tls.upstream(sink.port, "producer-token"),
        )]),
        tls.upstream(webhook.port, "notification-token"),
    )
    .unwrap_or_else(|error| panic!("producer: {error:?}"));
    let outbox = ProgramOutbox::new(&root.join("registry"));
    outbox
        .enqueue_publication(&publication(), principal, support::NOW)
        .unwrap_or_else(|error| panic!("publication enqueue: {error:?}"));
    let first = outbox
        .pending()
        .unwrap_or_else(|error| panic!("pending: {error:?}"))
        .unwrap_or_else(|| panic!("publication"));
    let mut foreign = first.observation.clone();
    foreign.principal_digest = Some(sha256_hex(b"foreign-principal"));
    assert_eq!(
        tls.upstream(sink.port, "producer-token")
            .post(
                "/internal/v1/observe",
                &foreign
                    .encode()
                    .unwrap_or_else(|error| panic!("foreign encoding: {error:?}"))
            )
            .unwrap_or_else(|error| panic!("refusal: {error:?}"))
            .status,
        403
    );
    let port = sink.port;
    drop(sink);
    assert!(client.step(&outbox, &outbox.health).is_err());
    std::thread::sleep(Duration::from_secs(31));
    assert!(!outbox.health.ready());
    assert_eq!(
        outbox
            .pending()
            .unwrap_or_else(|error| panic!("queue retained: {error:?}")),
        Some(first.clone())
    );
    let sink = source(port);
    client
        .step(&outbox, &outbox.health)
        .unwrap_or_else(|error| panic!("recovered observation: {error:?}"));
    assert!(outbox.health.ready());
    let observed = outbox
        .pending()
        .unwrap_or_else(|error| panic!("observed: {error:?}"))
        .unwrap_or_else(|| panic!("notification"));
    assert!(observed.observed);
    webhook.stop();
    assert!(client.step(&outbox, &outbox.health).is_err());
    drop(outbox);
    let outbox = ProgramOutbox::new(&root.join("registry"));
    assert_eq!(
        outbox
            .pending()
            .unwrap_or_else(|error| panic!("restart pending: {error:?}")),
        Some(observed.clone())
    );
    webhook.restart();
    assert_eq!(client.deliver(&observed), Ok(false));
    assert_eq!(client.deliver(&observed), Ok(false));
    client
        .step(&outbox, &outbox.health)
        .unwrap_or_else(|error| panic!("notification ack: {error:?}"));
    assert!(outbox
        .pending()
        .unwrap_or_else(|error| panic!("delivered: {error:?}"))
        .is_none());
    let reply = tls
        .upstream(sink.port, "consumer-token")
        .get(&format!("/internal/v1/events/{}", first.observation.id))
        .unwrap_or_else(|error| panic!("event read: {error:?}"));
    assert_eq!(reply.status, 200);
    let record: layerx_platform_internal::events::Record =
        serde_json::from_slice(&reply.body).unwrap_or_else(|error| panic!("record: {error:?}"));
    assert_eq!(
        record,
        first.observation.record("program-principal".to_owned())
    );
}
