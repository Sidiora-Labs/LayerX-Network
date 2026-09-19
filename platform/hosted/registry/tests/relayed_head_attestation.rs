//! The registry's sequencer discovery proof when no local LNI socket is
//! configured: the attestation arrives through the node boundary's
//! registry-plane relay and is verified under the independently verified
//! sequencer key before any proof field is published.

use layerx_client::lni::head_attestation::ProgramDiscoveryHead;
use layerx_platform_registry::head_attestation::{
    attach_discovery_proof, verified_discovery_proof, DiscoveryProofFields, ExpectedDiscoveryHead,
};
use layerx_platform_registry::{HeadAuthority, NodeProgramStateSource};
use layerx_programs::hex;
use layerx_programs::ProtocolDeploymentVerifier;
use serde_json::{json, Value};
use std::fmt::Debug;
use std::fs;
use std::io::{Read as _, Write as _};
use std::net::{TcpListener, TcpStream};
use std::os::unix::fs::PermissionsExt as _;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

const VECTOR: &str = include_str!("../../../../tests/vectors/native-head-attestation.json");
const NETWORK_ID: u32 = 7_332;
const PROTOCOL_VERSION: u16 = 3;
const NODE_TOKEN: &str = "registry-node-plane-token";
const AUTHORITY_TOKEN: &str = "independent-authority-token";
const AUTHORITY_ENDPOINT: &str = "http://127.0.0.1:1";
const LOOPBACK_CA: &[u8] = b"loopback http never opens a TLS session";
const PEER_WAIT: Duration = Duration::from_secs(20);
static NEXT_ROOT: AtomicU64 = AtomicU64::new(1);

fn must<T, E: Debug>(result: Result<T, E>, what: &str) -> T {
    result.unwrap_or_else(|error| panic!("{what}: {error:?}"))
}

fn vector() -> Value {
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
    must(hex::decode(&vector_hex(name)), name)
}

fn vector_fixed<const N: usize>(name: &str) -> [u8; N] {
    vector_bytes(name)
        .try_into()
        .unwrap_or_else(|_| panic!("vector field {name} has another length"))
}

fn vector_number<T: std::str::FromStr>(name: &str) -> T
where
    T::Err: Debug,
{
    must(vector_text(name).parse(), name)
}

fn verified_head() -> ExpectedDiscoveryHead {
    ExpectedDiscoveryHead {
        head: ProgramDiscoveryHead {
            program_id: vector_fixed("program_id"),
            version: vector_number("version"),
            code_hash: vector_fixed("code_hash"),
            abi_version: vector_number("abi_version"),
            observed_sequence: vector_number("observed_sequence"),
            observed_at: vector_number("observed_at"),
            valid_through: vector_number("valid_through"),
            state_root: vector_fixed("state_root"),
        },
        head_receipt_digest: vector_fixed("head_receipt_digest"),
    }
}

fn staleness() -> u64 {
    vector_number::<u64>("valid_through") - vector_number::<u64>("observed_at")
}

fn authority() -> HeadAuthority {
    HeadAuthority {
        sequencer_public_key: vector_fixed("sequencer_public_key"),
        protocol_version: PROTOCOL_VERSION,
        network_id: NETWORK_ID,
    }
}

fn relay_document(payload: &[u8], proof: &[u8]) -> String {
    format!(
        "{{\"payload_hex\":\"{}\",\"proof_hex\":\"{}\"}}",
        hex::encode(payload),
        hex::encode(proof)
    )
}

fn unpublished_document() -> Value {
    let expected = verified_head();
    json!({
        "program_id": hex::encode(&expected.head.program_id),
        "state_root": hex::encode(&expected.head.state_root),
        "latest_version": expected.head.version,
    })
}

struct Root(PathBuf);

impl Root {
    fn create() -> Self {
        let path = std::env::temp_dir().join(format!(
            "layerx-registry-relayed-head-{}-{}",
            std::process::id(),
            NEXT_ROOT.fetch_add(1, Ordering::Relaxed)
        ));
        must(fs::create_dir_all(&path), "relay test root");
        must(
            fs::set_permissions(&path, fs::Permissions::from_mode(0o700)),
            "relay test root mode",
        );
        Self(path)
    }
}

impl Drop for Root {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn deployment_verifier(root: &Root) -> ProtocolDeploymentVerifier {
    let mut history = b"LayerX/sequencer-trust-history/v1\0".to_vec();
    history.extend_from_slice(&1_u16.to_be_bytes());
    history.extend_from_slice(&0_u16.to_be_bytes());
    history.extend_from_slice(&PROTOCOL_VERSION.to_be_bytes());
    history.extend_from_slice(&NETWORK_ID.to_be_bytes());
    history.extend_from_slice(&1_u64.to_be_bytes());
    history.extend_from_slice(&[0x51; 32]);
    history.extend_from_slice(&authority().sequencer_public_key);
    history.extend_from_slice(&1_u64.to_be_bytes());
    history.extend_from_slice(&u64::MAX.to_be_bytes());
    history.extend_from_slice(&[0; 9]);
    let path = root.0.join("trust-history");
    must(fs::write(&path, &history), "trust history");
    must(
        fs::set_permissions(&path, fs::Permissions::from_mode(0o600)),
        "trust history mode",
    );
    must(
        ProtocolDeploymentVerifier::from_protected_history(&path, staleness()),
        "trust history verifier",
    )
}

/// One request the registry sent to the node boundary endpoint.
#[derive(Debug, Eq, PartialEq)]
struct Asked {
    request_line: String,
    authorization: String,
}

/// A loopback peer at the registry's node endpoint answering with the node
/// boundary's exact relay documents and refusals.
struct BoundaryPeer {
    endpoint: String,
    worker: JoinHandle<Vec<Asked>>,
}

impl BoundaryPeer {
    fn answering(answers: Vec<(u16, String)>) -> Self {
        let listener = must(TcpListener::bind("127.0.0.1:0"), "boundary peer");
        let endpoint = format!(
            "http://127.0.0.1:{}",
            must(listener.local_addr(), "boundary peer address").port()
        );
        must(listener.set_nonblocking(true), "boundary peer mode");
        let worker = thread::spawn(move || {
            answers
                .into_iter()
                .map(|(status, body)| answer(&accept(&listener), status, &body))
                .collect()
        });
        Self { endpoint, worker }
    }

    fn asked(self) -> Vec<Asked> {
        self.worker
            .join()
            .unwrap_or_else(|_| panic!("boundary peer panicked"))
    }
}

fn accept(listener: &TcpListener) -> TcpStream {
    let deadline = Instant::now() + PEER_WAIT;
    loop {
        match listener.accept() {
            Ok((stream, _)) => {
                must(stream.set_nonblocking(false), "boundary stream mode");
                must(
                    stream.set_read_timeout(Some(PEER_WAIT)),
                    "boundary stream timeout",
                );
                return stream;
            }
            Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                assert!(
                    Instant::now() < deadline,
                    "the registry never asked the node boundary"
                );
                thread::sleep(Duration::from_millis(5));
            }
            Err(error) => panic!("boundary accept: {error}"),
        }
    }
}

fn answer(mut stream: &TcpStream, status: u16, body: &str) -> Asked {
    let mut head = Vec::new();
    let mut byte = [0_u8; 1];
    while !head.ends_with(b"\r\n\r\n") {
        must(stream.read_exact(&mut byte), "boundary request head");
        head.push(byte[0]);
        assert!(
            head.len() <= 16 * 1024,
            "boundary request head is unbounded"
        );
    }
    let head = must(String::from_utf8(head), "boundary request text");
    let mut lines = head.split("\r\n");
    let request_line = lines.next().unwrap_or_default().to_owned();
    let authorization = lines
        .filter_map(|line| line.split_once(':'))
        .find(|(name, _)| name.eq_ignore_ascii_case("authorization"))
        .map(|(_, value)| value.trim().to_owned())
        .unwrap_or_default();
    let reason = if status == 200 { "OK" } else { "Refused" };
    must(
        stream.write_all(
            format!(
                "HTTP/1.1 {status} {reason}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                body.len()
            )
            .as_bytes(),
        ),
        "boundary response",
    );
    Asked {
        request_line,
        authorization,
    }
}

fn unbound_endpoint() -> String {
    let listener = must(TcpListener::bind("127.0.0.1:0"), "unbound endpoint");
    format!(
        "http://127.0.0.1:{}",
        must(listener.local_addr(), "unbound endpoint address").port()
    )
}

fn source(root: &Root, endpoint: &str) -> NodeProgramStateSource {
    must(
        NodeProgramStateSource::connect(
            endpoint,
            NODE_TOKEN.to_owned(),
            LOOPBACK_CA,
            AUTHORITY_ENDPOINT,
            AUTHORITY_TOKEN.to_owned(),
            [0x61; 32],
            deployment_verifier(root),
        ),
        "node state source",
    )
}

fn relayed(endpoint: &str, authority: &HeadAuthority) -> Result<DiscoveryProofFields, String> {
    let root = Root::create();
    verified_discovery_proof(
        None,
        &source(&root, endpoint),
        &verified_head(),
        staleness(),
        authority,
    )
}

fn published(endpoint: &str, authority: &HeadAuthority) -> Value {
    let mut document = unpublished_document();
    let _ = relayed(endpoint, authority)
        .and_then(|fields| attach_discovery_proof(&mut document, &fields));
    document
}

fn expected_request() -> Asked {
    Asked {
        request_line: format!(
            "GET /internal/v1/programs/{}/head-attestation?staleness_ms={} HTTP/1.1",
            vector_hex("program_id"),
            staleness()
        ),
        authorization: format!("Bearer {NODE_TOKEN}"),
    }
}

#[test]
fn relayed_vector_is_published_with_the_proof_fields() {
    let peer = BoundaryPeer::answering(vec![(
        200,
        relay_document(&vector_bytes("payload"), &vector_bytes("proof")),
    )]);
    let document = published(&peer.endpoint, &authority());
    assert_eq!(peer.asked(), vec![expected_request()]);
    assert_eq!(document["receipt_digest"], json!(vector_hex("digest")));
    assert_eq!(
        document["discovery_public_key"],
        json!(vector_hex("sequencer_public_key"))
    );
    assert_eq!(
        document["discovery_signature"],
        json!(vector_hex("signature"))
    );
    assert_eq!(
        document["latest_version"],
        json!(vector_number::<u32>("version"))
    );
}

#[test]
fn tampered_or_foreign_relayed_attestations_publish_no_proof() {
    let payload = vector_bytes("payload");
    let proof = vector_bytes("proof");
    let mut documents = Vec::new();
    for offset in [34, 38, 70, 72, 96, payload.len() - 1] {
        let mut tampered = payload.clone();
        tampered[offset] ^= 1;
        documents.push(relay_document(&tampered, &proof));
    }
    for offset in [0, 32, proof.len() - 1] {
        let mut tampered = proof.clone();
        tampered[offset] ^= 1;
        documents.push(relay_document(&payload, &tampered));
    }
    documents.push(relay_document(&payload[..payload.len() - 1], &proof));
    documents.push(relay_document(&payload, &proof[..proof.len() - 1]));
    documents.push(format!("{{\"payload_hex\":\"{}\"}}", hex::encode(&payload)));
    documents.push("{}".to_owned());
    for document in documents {
        let peer = BoundaryPeer::answering(vec![(200, document.clone())]);
        let result = relayed(&peer.endpoint, &authority());
        assert_eq!(peer.asked(), vec![expected_request()]);
        assert!(result.is_err(), "{document} was accepted");
        let peer = BoundaryPeer::answering(vec![(200, document)]);
        assert_eq!(
            published(&peer.endpoint, &authority()),
            unpublished_document()
        );
        peer.asked();
    }

    let mut foreign = authority();
    foreign.sequencer_public_key[0] ^= 1;
    let peer = BoundaryPeer::answering(vec![(200, relay_document(&payload, &proof))]);
    assert_eq!(published(&peer.endpoint, &foreign), unpublished_document());
    assert_eq!(peer.asked(), vec![expected_request()]);

    let mut other_head = verified_head();
    other_head.head.state_root[0] ^= 1;
    let peer = BoundaryPeer::answering(vec![(200, relay_document(&payload, &proof))]);
    let root = Root::create();
    assert!(verified_discovery_proof(
        None,
        &source(&root, &peer.endpoint),
        &other_head,
        staleness(),
        &authority(),
    )
    .is_err());
    peer.asked();
}

#[test]
fn unavailable_boundary_or_node_publishes_no_proof() {
    for (status, code, retry_after) in [
        (503, "node_unavailable", Some(5)),
        (503, "node_transport_lost", Some(5)),
        (503, "capability_unavailable", Some(60)),
        (503, "head_stale", Some(1)),
        (404, "program_not_registered", None),
        (403, "entitlement_denied", None),
        (401, "unauthorized", None),
    ] {
        let body = retry_after
            .map_or_else(
                || json!({"error": {"code": code, "retry": "never"}}),
                |seconds: u64| {
                    json!({"error": {"code": code, "retry": "after", "retry_after_seconds": seconds}})
                },
            )
            .to_string();
        let peer = BoundaryPeer::answering(vec![(status, body.clone())]);
        let refusal = relayed(&peer.endpoint, &authority());
        assert_eq!(peer.asked(), vec![expected_request()]);
        let refusal = refusal
            .err()
            .unwrap_or_else(|| panic!("{code} published a proof"));
        assert!(refusal.contains(code), "{refusal}");
        let peer = BoundaryPeer::answering(vec![(status, body)]);
        assert_eq!(
            published(&peer.endpoint, &authority()),
            unpublished_document()
        );
        peer.asked();
    }

    let peer = BoundaryPeer::answering(vec![(
        503,
        relay_document(&vector_bytes("payload"), &vector_bytes("proof")),
    )]);
    assert_eq!(
        published(&peer.endpoint, &authority()),
        unpublished_document()
    );
    peer.asked();

    assert_eq!(
        published(&unbound_endpoint(), &authority()),
        unpublished_document()
    );
    assert!(relayed(&unbound_endpoint(), &authority()).is_err());
}
