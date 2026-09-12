use std::collections::BTreeMap;
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc,
};
use std::thread::JoinHandle;

use super::{command, fs, path, Duration, RedisProcess, TcpListener};
use layerx_platform_internal::{
    events::{Kind, ProducerCredential, Service},
    http,
    producer::{Client, Outbox},
    tls::{Origin, Upstream},
};
use native_tls::Identity;
use rustls::pki_types::{CertificateDer, PrivateKeyDer, PrivatePkcs8KeyDer};
use rustls::{RootCertStore, ServerConfig, ServerConnection, StreamOwned};
use zeroize::Zeroizing;

struct Listener {
    port: u16,
    stop: Arc<AtomicBool>,
    join: Option<JoinHandle<()>>,
}
impl Listener {
    fn start(
        tls: Arc<ServerConfig>,
        handler: impl Fn(&http::Request) -> http::Response + Send + 'static,
    ) -> Self {
        Self::start_on(tls, 0, handler)
    }
    fn start_on(
        tls: Arc<ServerConfig>,
        port: u16,
        handler: impl Fn(&http::Request) -> http::Response + Send + 'static,
    ) -> Self {
        let listener =
            TcpListener::bind(("127.0.0.1", port)).unwrap_or_else(|error| panic!("{error}"));
        let port = listener
            .local_addr()
            .unwrap_or_else(|error| panic!("{error}"))
            .port();
        listener
            .set_nonblocking(true)
            .unwrap_or_else(|error| panic!("{error}"));
        let stop = Arc::new(AtomicBool::new(false));
        let stopped = Arc::clone(&stop);
        let join = std::thread::spawn(move || {
            while !stopped.load(Ordering::Acquire) {
                match listener.accept() {
                    Ok((tcp, _)) => {
                        tcp.set_read_timeout(Some(Duration::from_secs(3)))
                            .unwrap_or_else(|error| panic!("{error}"));
                        tcp.set_write_timeout(Some(Duration::from_secs(3)))
                            .unwrap_or_else(|error| panic!("{error}"));
                        let connection = ServerConnection::new(Arc::clone(&tls))
                            .unwrap_or_else(|error| panic!("{error}"));
                        let mut stream = StreamOwned::new(connection, tcp);
                        if let Ok(mut request) = http::parse_client_request(&mut stream) {
                            request.peer_verified = stream
                                .conn
                                .peer_certificates()
                                .is_some_and(|certs| !certs.is_empty());
                            let _ = http::write_response(&mut stream, &handler(&request));
                        }
                    }
                    Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                        std::thread::sleep(Duration::from_millis(5));
                    }
                    Err(error) => panic!("{error}"),
                }
            }
        });
        Self {
            port,
            stop,
            join: Some(join),
        }
    }
}
impl Drop for Listener {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Release);
        if let Some(join) = self.join.take() {
            assert!(join.join().is_ok());
        }
    }
}

fn tls(redis: &RedisProcess) -> (Arc<ServerConfig>, Identity) {
    let directory = &redis.directory;
    command(
        "openssl",
        &[
            "req",
            "-new",
            "-newkey",
            "rsa:2048",
            "-nodes",
            "-keyout",
            path(&directory.join("client.key")),
            "-out",
            path(&directory.join("client.csr")),
            "-subj",
            "/CN=producer",
        ],
    );
    fs::write(
        directory.join("client.ext"),
        "basicConstraints=critical,CA:FALSE\nextendedKeyUsage=clientAuth\n",
    )
    .unwrap_or_else(|error| panic!("{error}"));
    command(
        "openssl",
        &[
            "x509",
            "-req",
            "-in",
            path(&directory.join("client.csr")),
            "-CA",
            path(&directory.join("server.pem")),
            "-CAkey",
            path(&directory.join("server.key")),
            "-CAcreateserial",
            "-out",
            path(&directory.join("client.pem")),
            "-days",
            "1",
            "-extfile",
            path(&directory.join("client.ext")),
        ],
    );
    command(
        "openssl",
        &[
            "pkcs12",
            "-export",
            "-inkey",
            path(&directory.join("client.key")),
            "-in",
            path(&directory.join("client.pem")),
            "-out",
            path(&directory.join("client.p12")),
            "-passout",
            "pass:integration-only",
        ],
    );
    command(
        "openssl",
        &[
            "pkcs8",
            "-topk8",
            "-nocrypt",
            "-in",
            path(&directory.join("server.key")),
            "-outform",
            "DER",
            "-out",
            path(&directory.join("server-key.der")),
        ],
    );
    let certificate = CertificateDer::from(
        fs::read(directory.join("server.der")).unwrap_or_else(|error| panic!("{error}")),
    );
    let mut roots = RootCertStore::empty();
    roots
        .add(certificate.clone())
        .unwrap_or_else(|error| panic!("{error}"));
    let verifier = rustls::server::WebPkiClientVerifier::builder(Arc::new(roots))
        .allow_unauthenticated()
        .build()
        .unwrap_or_else(|error| panic!("{error}"));
    let key = PrivateKeyDer::from(PrivatePkcs8KeyDer::from(
        fs::read(directory.join("server-key.der")).unwrap_or_else(|error| panic!("{error}")),
    ));
    let configuration = ServerConfig::builder()
        .with_client_cert_verifier(verifier)
        .with_single_cert(vec![certificate], key)
        .unwrap_or_else(|error| panic!("{error}"));
    let identity = Identity::from_pkcs12(
        &fs::read(directory.join("client.p12")).unwrap_or_else(|error| panic!("{error}")),
        "integration-only",
    )
    .unwrap_or_else(|error| panic!("{error}"));
    (Arc::new(configuration), identity)
}

#[test]
fn payment_facts_cross_real_tls_and_refuse_foreign_principals_without_losing_the_outbox() {
    use layerx_platform_gateway::gateway_principal;
    let _ = rustls::crypto::ring::default_provider().install_default();
    let redis = RedisProcess::start();
    let (tls, identity) = tls(&redis);
    let store = Arc::new(redis.store());
    let (key, secret) = issue_payment_key(&store);
    let principal_store = Arc::clone(&store);
    let principal = Listener::start(Arc::clone(&tls), move |request| {
        if request.method != "GET" || request.path != "/internal/v1/principal" {
            return http::refusal(404, "not_found", None);
        }
        request
            .headers
            .get("authorization")
            .ok_or(layerx_platform_gateway::AccessError::Unauthenticated)
            .and_then(|authorization| gateway_principal(&principal_store, authorization))
            .map_or_else(
                |_| http::refusal(401, "api_key_required", None),
                |value| http::json(200, &value),
            )
    });
    let upstream = |port, token: &str| {
        Upstream::new(
            Origin::parse(&format!("https://localhost:{port}"))
                .unwrap_or_else(|error| panic!("{error}")),
            redis.certificate.clone(),
            Some(identity.clone()),
            Some(Zeroizing::new(token.to_owned())),
        )
    };
    let credentials = BTreeMap::from([
        (
            "payment-principal".to_owned(),
            Zeroizing::new(format!("payment-key:{secret}")),
        ),
        (
            "foreign-principal".to_owned(),
            Zeroizing::new(format!("payment-key:{secret}")),
        ),
    ]);
    let sink = event_source(
        Arc::clone(&tls),
        upstream(principal.port, "unused-principal-bearer"),
        credentials.clone(),
        &redis.directory.join("events"),
        0,
    );
    let pending = enqueue_payment(&store, &key);
    let client = Client::new(
        BTreeMap::from([("payment".to_owned(), upstream(sink.port, "producer-token"))]),
        upstream(sink.port, "notification-token"),
    )
    .unwrap_or_else(|error| panic!("{error}"));
    assert_eq!(client.deliver(&pending), Ok(true));
    assert_eq!(client.deliver(&pending), Ok(true));
    let response = upstream(sink.port, "consumer-token")
        .get(&format!("/internal/v1/events/{}", pending.observation.id))
        .unwrap_or_else(|error| panic!("{error:?}"));
    assert_eq!(response.status, 200);
    let record: layerx_platform_internal::events::Record =
        serde_json::from_slice(&response.body).unwrap_or_else(|error| panic!("{error}"));
    assert_eq!(
        record,
        pending.observation.record("payment-principal".to_owned())
    );
    assert_foreign(&upstream(sink.port, "producer-token"), &pending.observation);
    let sink_port = sink.port;
    drop(sink);
    assert!(client.deliver(&pending).is_err());
    assert!(client.step(store.as_ref(), &store.producer_health).is_err());
    std::thread::sleep(Duration::from_secs(31));
    assert!(!store.producer_health.ready());
    assert_eq!(
        store.pending().unwrap_or_else(|error| panic!("{error}")),
        Some(pending.clone())
    );
    let sink = event_source(
        Arc::clone(&tls),
        upstream(principal.port, "unused-principal-bearer"),
        credentials,
        &redis.directory.join("events"),
        sink_port,
    );
    assert!(client.step(store.as_ref(), &store.producer_health).is_ok());
    assert!(store.producer_health.ready());
    let mut observed = pending;
    observed.observed = true;
    assert_eq!(
        store.pending().unwrap_or_else(|error| panic!("{error}")),
        Some(observed)
    );
    drop(sink);
    drop(principal);
}

fn assert_foreign(
    source: &Upstream,
    observation: &layerx_platform_internal::producer::Observation,
) {
    use layerx_platform_internal::secret::sha256_hex;
    let mut foreign = observation.clone();
    foreign.principal_digest = Some(sha256_hex(b"foreign-principal"));
    let response = source
        .post(
            "/internal/v1/observe",
            &foreign.encode().unwrap_or_else(|error| panic!("{error}")),
        )
        .unwrap_or_else(|error| panic!("{error:?}"));
    assert_eq!(response.status, 403);
}

fn issue_payment_key(
    store: &layerx_platform_gateway::store::RedisStore,
) -> (layerx_platform_gateway::store::KeyRecord, String) {
    let secret = format!("lxp_live_{}", "12".repeat(32));
    let key = layerx_platform_gateway::store::KeyRecord {
        key_id: "payment-key".to_owned(),
        principal_digest: layerx_platform_internal::secret::sha256_hex(b"payment-principal"),
        salt: "payment-salt".to_owned(),
        secret_digest: layerx_platform_gateway::gateway_digest(&[
            b"gateway-key-v1",
            b"payment-salt",
            secret.as_bytes(),
        ]),
        signer_public_key: "11".repeat(32),
        scopes: "receipt:read".to_owned(),
        quota_requests: 100,
        quota_window_seconds: 60,
        epoch: 1,
        disabled: false,
    };
    store
        .issue_key(&key, "issue-payment-key")
        .unwrap_or_else(|error| panic!("{error}"));
    (key, secret)
}

fn enqueue_payment(
    store: &layerx_platform_gateway::store::RedisStore,
    key: &layerx_platform_gateway::store::KeyRecord,
) -> layerx_platform_internal::producer::Pending {
    use layerx_platform_gateway::store::{Completion, ReservationRequest};
    use layerx_platform_internal::secret::{hex, unhex};
    let document: serde_json::Value =
        serde_json::from_str(include_str!("../fixtures/maintained-authority.json"))
            .unwrap_or_else(|error| panic!("{error}"));
    let receipt_hex = document["receipt_hex"]
        .as_str()
        .unwrap_or_else(|| panic!("receipt missing"));
    let bytes = unhex(receipt_hex).unwrap_or_else(|| panic!("receipt encoding"));
    let decoded = layerx_wire::receipt::decode(&bytes).unwrap_or_else(|error| panic!("{error:?}"));
    let receipt = decoded
        .protocol()
        .unwrap_or_else(|| panic!("protocol receipt"));
    let resource = hex(&receipt.activity_id());
    store
        .reserve(
            key,
            ReservationRequest {
                idempotency_scope: "tls-payment",
                request_digest: "tls-payment-request",
                now: 100,
                retention_seconds: 3600,
                activity_id: &resource,
                protocol_idempotency_key: "tls-idempotency",
                principal_digest: &key.principal_digest,
                audit_event: "reserved-tls-payment",
                continuation: "",
            },
        )
        .unwrap_or_else(|error| panic!("{error}"));
    store
        .complete_verified(Completion {
            idempotency_scope: "tls-payment",
            request_digest: "tls-payment-request",
            state: "completed",
            response_hex: "7b7d",
            receipt_hex,
            activity_id: Some(&resource),
            principal_digest: &key.principal_digest,
            audit_event: "verified-tls-payment",
        })
        .unwrap_or_else(|error| panic!("{error}"));
    store
        .pending()
        .unwrap_or_else(|error| panic!("{error}"))
        .unwrap_or_else(|| panic!("pending missing"))
}

fn event_source(
    tls: Arc<ServerConfig>,
    upstream: Upstream,
    credentials: BTreeMap<String, Zeroizing<String>>,
    directory: &std::path::Path,
    port: u16,
) -> Listener {
    let service = Service::open(
        Kind::Payment,
        upstream,
        credentials,
        Zeroizing::new("consumer-token".to_owned()),
        directory,
    )
    .and_then(|service| {
        service.with_producers(vec![ProducerCredential {
            token: Zeroizing::new("producer-token".to_owned()),
            allow_principal_digest: true,
        }])
    })
    .unwrap_or_else(|error| panic!("{error}"));
    Listener::start_on(tls, port, move |request| service.route(request))
}
