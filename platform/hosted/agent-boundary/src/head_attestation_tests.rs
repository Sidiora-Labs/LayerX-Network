use super::{
    route, Config, NodeEndpoint, Request, Response, SessionPool, LNI_CONNECTIONS, PROTOCOL_VERSION,
};
use layerx_client::lni::framing::{read_frame, write_frame};
use layerx_client::lni::handshake::{encode_node_info, NodeInfo, NodeRole};
use layerx_client::lni::head_attestation::{
    decode_program_head_attestation, encode_program_head_attest_request,
    PROGRAM_HEAD_ATTEST_REQUEST_TAG, PROGRAM_HEAD_ATTEST_RESPONSE_TAG,
};
use layerx_client::lni::schema::{decode_envelope, encode_envelope, Envelope, Version};
use layerx_client::lni::transport::ConnectionGate;
use std::collections::BTreeMap;
use std::os::unix::net::UnixListener;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::thread::{self, JoinHandle};
use std::time::Duration;
use zeroize::Zeroizing;

const VECTOR: &str = include_str!("../../../../tests/vectors/native-head-attestation.json");
const NETWORK_ID: u32 = 7_332;
const FRAME_BYTES: usize = 1_212_416;
const GATEWAY_TOKEN: &str = "gateway-plane-token";
const REGISTRY_TOKEN: &str = "registry-plane-token";
const WEBHOOK_TOKEN: &str = "webhook-plane-token";
const NODE_INFO_REQUEST_TAG: u16 = 1;
const NODE_INFO_RESPONSE_TAG: u16 = 2;
const PROJECTION_STALE: i32 = -903;
const UNKNOWN_FIELD: i32 = -7;
static NEXT_ROOT: AtomicU64 = AtomicU64::new(1);

fn must<T, E: std::fmt::Debug>(result: Result<T, E>, what: &str) -> T {
    result.unwrap_or_else(|error| panic!("{what}: {error:?}"))
}

fn vector() -> serde_json::Value {
    must(serde_json::from_str(VECTOR), "head attestation vector")
}

fn vector_text(name: &str) -> String {
    vector()[name]
        .as_str()
        .unwrap_or_else(|| panic!("vector omits {name}"))
        .to_owned()
}

fn vector_hex(name: &str) -> String {
    vector_text(name)
        .strip_prefix("0x")
        .unwrap_or_else(|| panic!("vector field {name} is not hexadecimal"))
        .to_owned()
}

fn vector_bytes(name: &str) -> Vec<u8> {
    must(super::decode_hex(&vector_hex(name), 4096), name)
}

fn vector_fixed(name: &str) -> [u8; 32] {
    vector_bytes(name)
        .try_into()
        .unwrap_or_else(|_| panic!("vector field {name} is not thirty-two bytes"))
}

fn vector_number(name: &str) -> u64 {
    must(vector_text(name).parse(), name)
}

fn staleness() -> u64 {
    vector_number("valid_through") - vector_number("observed_at")
}

fn target() -> String {
    format!(
        "/internal/v1/programs/{}/head-attestation?staleness_ms={}",
        vector_hex("program_id"),
        staleness()
    )
}

struct Root(PathBuf);

impl Root {
    fn new() -> Self {
        let path = std::env::temp_dir().join(format!(
            "layerx-boundary-head-attestation-{}-{}",
            std::process::id(),
            NEXT_ROOT.fetch_add(1, Ordering::Relaxed)
        ));
        must(std::fs::create_dir_all(&path), "test root");
        Self(path)
    }

    fn socket(&self) -> PathBuf {
        self.0.join("lni.sock")
    }
}

impl Drop for Root {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn config(root: &Root) -> Config {
    let tls = must(
        rustls::ServerConfig::builder_with_provider(Arc::new(
            rustls::crypto::ring::default_provider(),
        ))
        .with_safe_default_protocol_versions(),
        "TLS protocol versions",
    )
    .with_no_client_auth()
    .with_cert_resolver(Arc::new(rustls::server::ResolvesServerCertUsingSni::new()));
    Config {
        listen: must("127.0.0.1:0".parse(), "listen address"),
        tls: Arc::new(tls),
        gateway_token: Zeroizing::new(GATEWAY_TOKEN.to_owned()),
        registry_token: Zeroizing::new(REGISTRY_TOKEN.to_owned()),
        webhook_token: Zeroizing::new(WEBHOOK_TOKEN.to_owned()),
        lni_socket: root.socket(),
        lni_deadline: Duration::from_secs(5),
        node: NodeEndpoint { port: 1 },
        node_token: Zeroizing::new("node-token".to_owned()),
        state_dir: root.0.join("state"),
        protocol_network_id: NETWORK_ID,
        network_name: "layerx-head-attestation-test".to_owned(),
        registry: must(super::module_registry(), "module registry"),
        receipt_wait: Duration::from_secs(1),
        gate: ConnectionGate::new(LNI_CONNECTIONS),
        sessions: SessionPool::new(),
        key_locks: Mutex::new(BTreeMap::new()),
    }
}

fn request(method: &str, target: &str, bearer: Option<&str>) -> Request {
    let mut headers = BTreeMap::from([("host".to_owned(), "boundary".to_owned())]);
    if let Some(token) = bearer {
        headers.insert("authorization".to_owned(), format!("Bearer {token}"));
    }
    Request {
        method: method.to_owned(),
        path: target.to_owned(),
        headers,
        body: Vec::new(),
    }
}

fn error_code(response: &Response) -> String {
    let document: serde_json::Value = must(serde_json::from_str(&response.body), "refusal JSON");
    document["error"]["code"]
        .as_str()
        .unwrap_or_else(|| panic!("refusal omits its code: {}", response.body))
        .to_owned()
}

enum NodeAnswer {
    Attestation { payload: Vec<u8>, proof: Vec<u8> },
    Refusal { class: u8, result: i32 },
}

struct SentRequest {
    tag: u16,
    payload: Vec<u8>,
    proof: Vec<u8>,
}

fn node_info(sequencer_key: [u8; 32], capabilities: &[&str]) -> NodeInfo {
    NodeInfo {
        interface_version: Version::V1_7,
        protocol_version: PROTOCOL_VERSION,
        network_id: NETWORK_ID,
        role: NodeRole::Sequencer,
        chain_head_sequence: vector_number("observed_sequence"),
        latest_sealed_batch: 1,
        latest_finalised_checkpoint: [0; 32],
        authorised_sequencer_key: sequencer_key,
        advertised_capabilities: capabilities.iter().map(|name| (*name).to_owned()).collect(),
    }
}

/// Serves one LNI connection on the boundary's node socket: the startup
/// exchange, then one head attestation answer. Returns the request envelope
/// the boundary sent, when it sent one.
fn serve_node(
    root: &Root,
    info: NodeInfo,
    answer: Option<NodeAnswer>,
) -> JoinHandle<Option<SentRequest>> {
    let listener = must(UnixListener::bind(root.socket()), "node socket");
    thread::spawn(move || {
        let (mut stream, _) = must(listener.accept(), "node accept");
        let hello = must(read_frame(&mut stream, FRAME_BYTES), "startup frame");
        let hello = must(decode_envelope(&hello), "startup envelope");
        assert_eq!(hello.message_tag, NODE_INFO_REQUEST_TAG);
        let payload = must(encode_node_info(&info), "node info");
        let response = must(
            encode_envelope(Envelope {
                version: info.interface_version,
                message_tag: NODE_INFO_RESPONSE_TAG,
                correlation_id: 0,
                canonical_payload: &payload,
                proof_material: &[],
            }),
            "node info envelope",
        );
        must(
            write_frame(&mut stream, &response, FRAME_BYTES),
            "node info frame",
        );
        let answer = answer?;
        let frame = must(read_frame(&mut stream, FRAME_BYTES), "attestation request");
        let envelope = must(decode_envelope(&frame), "attestation request envelope");
        assert_eq!(envelope.version, info.interface_version);
        let refusal;
        let (tag, payload, proof): (u16, &[u8], &[u8]) = match &answer {
            NodeAnswer::Attestation { payload, proof } => {
                (PROGRAM_HEAD_ATTEST_RESPONSE_TAG, payload, proof)
            }
            NodeAnswer::Refusal { class, result } => {
                let mut bytes = vec![*class];
                bytes.extend_from_slice(&result.to_be_bytes());
                refusal = bytes;
                (super::ERROR_RESPONSE_TAG, &refusal, &[])
            }
        };
        let response = must(
            encode_envelope(Envelope {
                version: info.interface_version,
                message_tag: tag,
                correlation_id: envelope.correlation_id,
                canonical_payload: payload,
                proof_material: proof,
            }),
            "attestation answer envelope",
        );
        must(
            write_frame(&mut stream, &response, FRAME_BYTES),
            "attestation answer frame",
        );
        Some(SentRequest {
            tag: envelope.message_tag,
            payload: envelope.canonical_payload.to_vec(),
            proof: envelope.proof_material.to_vec(),
        })
    })
}

const ATTESTING: &[&str] = &[
    "node_info",
    "program_head_attest",
    "receipt_lookup",
    "submit",
];

fn relay(info: NodeInfo, answer: Option<NodeAnswer>) -> (Response, Option<SentRequest>) {
    let root = Root::new();
    let node = serve_node(&root, info, answer);
    let config = config(&root);
    let response = route(&config, &request("GET", &target(), Some(REGISTRY_TOKEN)));
    drop(config);
    let sent = node
        .join()
        .unwrap_or_else(|_| panic!("the node side of the relay panicked"));
    (response, sent)
}

#[test]
fn native_vector_is_relayed_byte_for_byte_over_the_registry_plane() {
    let (response, sent) = relay(
        node_info(vector_fixed("sequencer_public_key"), ATTESTING),
        Some(NodeAnswer::Attestation {
            payload: vector_bytes("payload"),
            proof: vector_bytes("proof"),
        }),
    );
    assert_eq!(response.status, 200, "{}", response.body);
    assert_eq!(
        response.body,
        format!(
            "{{\"payload_hex\":\"{}\",\"proof_hex\":\"{}\"}}",
            vector_hex("payload"),
            vector_hex("proof")
        )
    );
    let sent = sent.unwrap_or_else(|| panic!("no attestation request was sent"));
    assert_eq!(sent.tag, PROGRAM_HEAD_ATTEST_REQUEST_TAG);
    assert_eq!(
        sent.payload,
        encode_program_head_attest_request(&vector_fixed("program_id"), staleness()).to_vec()
    );
    assert!(sent.proof.is_empty());
    let document: serde_json::Value = must(serde_json::from_str(&response.body), "relay JSON");
    let attestation = must(
        decode_program_head_attestation(
            &must(
                super::decode_hex(document["payload_hex"].as_str().unwrap_or_default(), 4096),
                "relayed payload",
            ),
            &must(
                super::decode_hex(document["proof_hex"].as_str().unwrap_or_default(), 4096),
                "relayed proof",
            ),
            &vector_fixed("program_id"),
            staleness(),
            &vector_fixed("sequencer_public_key"),
        ),
        "relayed attestation under the sequencer key",
    );
    assert_eq!(attestation.digest, vector_fixed("digest"));
    assert_eq!(attestation.signature.to_vec(), vector_bytes("signature"));
}

#[test]
fn only_the_registry_plane_reaches_the_attestation_and_never_the_node_when_refused() {
    let root = Root::new();
    let listener = must(UnixListener::bind(root.socket()), "node socket");
    must(listener.set_nonblocking(true), "non-blocking node socket");
    let config = config(&root);
    let target = target();
    let unauthenticated = route(&config, &request("GET", &target, None));
    assert_eq!(unauthenticated.status, 401);
    assert_eq!(error_code(&unauthenticated), "identity_required");
    let unknown = route(&config, &request("GET", &target, Some("unknown-token")));
    assert_eq!(unknown.status, 401);
    for bearer in [GATEWAY_TOKEN, WEBHOOK_TOKEN] {
        let denied = route(&config, &request("GET", &target, Some(bearer)));
        assert_eq!(denied.status, 403, "{bearer}");
        assert_eq!(error_code(&denied), "entitlement_denied");
    }
    let posted = route(&config, &request("POST", &target, Some(REGISTRY_TOKEN)));
    assert_eq!(posted.status, 404);
    let program = vector_hex("program_id");
    for (target, code) in [
        (
            format!("/internal/v1/programs/{}/head-attestation?staleness_ms=60000", "0".repeat(64)),
            "invalid_program_id",
        ),
        (
            format!(
                "/internal/v1/programs/{}/head-attestation?staleness_ms=60000",
                "AB".repeat(32)
            ),
            "invalid_program_id",
        ),
        (
            "/internal/v1/programs/1111/head-attestation?staleness_ms=60000".to_owned(),
            "invalid_program_id",
        ),
        (
            format!("/internal/v1/programs/{program}/head-attestation"),
            "invalid_staleness_ms",
        ),
        (
            format!("/internal/v1/programs/{program}/head-attestation?staleness_ms=0"),
            "invalid_staleness_ms",
        ),
        (
            format!("/internal/v1/programs/{program}/head-attestation?staleness_ms=060000"),
            "invalid_staleness_ms",
        ),
        (
            format!("/internal/v1/programs/{program}/head-attestation?staleness_ms=60000&x=1"),
            "invalid_staleness_ms",
        ),
        (
            format!(
                "/internal/v1/programs/{program}/head-attestation?staleness_ms=184467440737095516160"
            ),
            "invalid_staleness_ms",
        ),
    ] {
        let refused = route(&config, &request("GET", &target, Some(REGISTRY_TOKEN)));
        assert_eq!(refused.status, 400, "{target}");
        assert_eq!(error_code(&refused), code, "{target}");
    }
    assert_eq!(
        listener.accept().err().map(|error| error.kind()),
        Some(std::io::ErrorKind::WouldBlock)
    );
}

#[test]
fn tampered_or_foreign_attestations_are_not_relayed() {
    for offset in [34, 38, 70, 72, 96] {
        let mut payload = vector_bytes("payload");
        payload[offset] ^= 1;
        let (response, _) = relay(
            node_info(vector_fixed("sequencer_public_key"), ATTESTING),
            Some(NodeAnswer::Attestation {
                payload,
                proof: vector_bytes("proof"),
            }),
        );
        assert_eq!(response.status, 503, "offset {offset}: {}", response.body);
        assert_eq!(error_code(&response), "node_transport_lost");
        assert!(!response.body.contains("payload_hex"));
    }
    let mut signature = vector_bytes("proof");
    signature[40] ^= 1;
    let (response, _) = relay(
        node_info(vector_fixed("sequencer_public_key"), ATTESTING),
        Some(NodeAnswer::Attestation {
            payload: vector_bytes("payload"),
            proof: signature,
        }),
    );
    assert_eq!(response.status, 503, "{}", response.body);
    assert!(!response.body.contains("payload_hex"));
    let mut foreign = vector_fixed("sequencer_public_key");
    foreign[0] ^= 1;
    let (response, _) = relay(
        node_info(foreign, ATTESTING),
        Some(NodeAnswer::Attestation {
            payload: vector_bytes("payload"),
            proof: vector_bytes("proof"),
        }),
    );
    assert_eq!(response.status, 503, "{}", response.body);
    assert!(!response.body.contains("payload_hex"));
}

#[test]
fn node_refusals_and_absence_answer_unavailable_without_a_proof() {
    let (stale, _) = relay(
        node_info(vector_fixed("sequencer_public_key"), ATTESTING),
        Some(NodeAnswer::Refusal {
            class: 4,
            result: PROJECTION_STALE,
        }),
    );
    assert_eq!(stale.status, 503, "{}", stale.body);
    assert_eq!(error_code(&stale), "head_stale");
    assert_eq!(stale.retry_after, Some(1));
    let (unregistered, _) = relay(
        node_info(vector_fixed("sequencer_public_key"), ATTESTING),
        Some(NodeAnswer::Refusal {
            class: 4,
            result: UNKNOWN_FIELD,
        }),
    );
    assert_eq!(unregistered.status, 404, "{}", unregistered.body);
    assert_eq!(error_code(&unregistered), "program_not_registered");
    let (unsupported, sent) = relay(
        node_info(
            vector_fixed("sequencer_public_key"),
            &["node_info", "receipt_lookup", "submit"],
        ),
        None,
    );
    assert_eq!(unsupported.status, 503, "{}", unsupported.body);
    assert_eq!(error_code(&unsupported), "capability_unavailable");
    assert!(sent.is_none());
    let root = Root::new();
    let config = config(&root);
    let absent = route(&config, &request("GET", &target(), Some(REGISTRY_TOKEN)));
    assert_eq!(absent.status, 503, "{}", absent.body);
    assert_eq!(error_code(&absent), "node_unavailable");
    for response in [&stale, &unregistered, &unsupported, &absent] {
        assert!(!response.body.contains("payload_hex"));
        assert!(!response.body.contains("proof_hex"));
    }
}
