use std::fs;
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::thread;
use std::time::Duration;

use ed25519_dalek::{Signer as _, SigningKey};
use layerx_client::evidence::{EvidenceError, RootSelector};
use layerx_client::head::Head;
use layerx_client::lni::framing::{read_frame, write_frame};
use layerx_client::lni::schema::{decode_envelope, encode_envelope, Envelope, Version};
use layerx_client::lni::transport::{ConnectionGate, Limits, Uds};
use layerx_client::read::{
    account, balance, history, module_state, HistoryCursor, ReadContext, ReadError, Requested,
};
use layerx_proof::inclusion::SequencerAuthorization;
use layerx_proof::merkle::{build_proof, Proof};
use layerx_types::verify::VerificationLevel;
use layerx_wire::encode::Encoder;
use layerx_wire::hash::batch_header_digest;
use layerx_wire::limits::PROTOCOL_VERSION;

static NEXT_SOCKET: AtomicU64 = AtomicU64::new(1);

struct SocketPath(PathBuf);

impl SocketPath {
    fn new(label: &str) -> Self {
        let sequence = NEXT_SOCKET.fetch_add(1, Ordering::Relaxed);
        Self(std::env::temp_dir().join(format!(
            "layerx-reads-{label}-{}-{sequence}.sock",
            std::process::id()
        )))
    }
}

impl Drop for SocketPath {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.0);
    }
}

fn limits() -> Limits {
    Limits {
        maximum_frame_bytes: 1024 * 1024,
        maximum_connections: 1,
        maximum_streams: 4,
        maximum_queued_bytes: 2 * 1024 * 1024,
        deadline: Duration::from_secs(2),
    }
}

fn header_bytes(state_root: [u8; 32], activity_root: [u8; 32], sequencer_id: [u8; 32]) -> Vec<u8> {
    let mut encoder = Encoder::new(354);
    assert!(encoder
        .structure_header_version(0x1701, PROTOCOL_VERSION)
        .is_ok());
    assert!(encoder.u8(15).is_ok());
    assert!(encoder.tag(1, 15).is_ok());
    assert!(encoder.u16(PROTOCOL_VERSION).is_ok());
    assert!(encoder.tag(2, 15).is_ok());
    assert!(encoder.u32(77).is_ok());
    for (field, value) in [(3, 2_u64), (4, 7), (5, 10), (6, 12)] {
        assert!(encoder.tag(field, 15).is_ok());
        assert!(encoder.u64(value).is_ok());
    }
    for (field, value) in [
        (7, [1; 32]),
        (8, state_root),
        (9, activity_root),
        (10, [2; 32]),
        (11, [3; 32]),
        (12, [4; 32]),
        (13, [5; 32]),
    ] {
        assert!(encoder.tag(field, 15).is_ok());
        assert!(encoder.bytes(&value, 32).is_ok());
    }
    assert!(encoder.tag(14, 15).is_ok());
    assert!(encoder.u64(1_000).is_ok());
    assert!(encoder.tag(15, 15).is_ok());
    assert!(encoder.bytes(&sequencer_id, 32).is_ok());
    encoder.finish()
}

fn state_fixture(
    asserted_level: u8,
) -> (Vec<u8>, Vec<u8>, SequencerAuthorization, [u8; 32], [u8; 32]) {
    let account = [0x21; 32];
    let asset = [0x31; 32];
    let mut balance_leaf = Vec::with_capacity(80);
    balance_leaf.extend_from_slice(&account);
    balance_leaf.extend_from_slice(&asset);
    balance_leaf.extend_from_slice(&750_u128.to_be_bytes());
    let other = [0x99; 80];
    let leaves = [balance_leaf.as_slice(), other.as_slice()];
    let (proof, root) = match build_proof(&leaves, 0) {
        Ok(result) => result,
        Err(error) => panic!("state proof failed: {error:?}"),
    };
    let key = SigningKey::from_bytes(&[0x41; 32]);
    let sequencer_id = key.verifying_key().to_bytes();
    let header = header_bytes(root, [0x51; 32], sequencer_id);
    let digest = match batch_header_digest(&header) {
        Ok(digest) => digest,
        Err(error) => panic!("header digest failed: {error:?}"),
    };
    let signature = key.sign(&digest).to_bytes();
    let proof_material = encode_proof(asserted_level, root, &proof, &header, &signature);
    let authorization = SequencerAuthorization::new(sequencer_id, sequencer_id, 7, 7);
    (balance_leaf, proof_material, authorization, account, asset)
}

fn encode_proof(
    asserted_level: u8,
    root: [u8; 32],
    proof: &Proof,
    header: &[u8],
    signature: &[u8; 64],
) -> Vec<u8> {
    let mut bytes = Vec::new();
    bytes.push(asserted_level);
    bytes.extend_from_slice(&root);
    bytes.extend_from_slice(&proof.leaf_index().to_be_bytes());
    bytes.extend_from_slice(&proof.leaf_count().to_be_bytes());
    let sibling_count = match u8::try_from(proof.siblings().len()) {
        Ok(count) => count,
        Err(error) => panic!("sibling count failed: {error}"),
    };
    bytes.push(sibling_count);
    for sibling in proof.siblings() {
        bytes.extend_from_slice(sibling);
    }
    let header_length = match u32::try_from(header.len()) {
        Ok(length) => length,
        Err(error) => panic!("header length failed: {error}"),
    };
    bytes.extend_from_slice(&header_length.to_be_bytes());
    bytes.extend_from_slice(header);
    bytes.extend_from_slice(signature);
    bytes
}

fn context(requested: VerificationLevel, authorization: SequencerAuthorization) -> ReadContext {
    let root_selector = if requested >= VerificationLevel::CHECKPOINT_FINALISED {
        RootSelector::Checkpoint([0x71; 32])
    } else {
        RootSelector::Latest
    };
    ReadContext {
        interface_version: Version::V1_2,
        correlation_id: 44,
        expected_protocol_version: PROTOCOL_VERSION,
        expected_network_id: 77,
        requested: Requested::new(requested),
        head: Head {
            chain_sequence: 12,
            sealed_batch: 7,
            finalised_checkpoint: [0x71; 32],
        },
        handshake_sequencer_key: authorization.public_key(),
        sequencer_authorization: authorization,
        root_selector,
    }
}

fn request(stream: &mut UnixStream, expected_tag: u16) -> (u64, Vec<u8>) {
    let frame = match read_frame(stream, 1024 * 1024) {
        Ok(frame) => frame,
        Err(error) => panic!("request frame failed: {error:?}"),
    };
    let envelope = match decode_envelope(&frame) {
        Ok(envelope) => envelope,
        Err(error) => panic!("request envelope failed: {error:?}"),
    };
    assert_eq!(envelope.message_tag, expected_tag);
    (envelope.correlation_id, envelope.canonical_payload.to_vec())
}

fn respond(stream: &mut UnixStream, tag: u16, correlation_id: u64, payload: &[u8], proof: &[u8]) {
    let response = match encode_envelope(Envelope {
        version: Version::V1_0,
        message_tag: tag,
        correlation_id,
        canonical_payload: payload,
        proof_material: proof,
    }) {
        Ok(response) => response,
        Err(error) => panic!("response encoding failed: {error:?}"),
    };
    if let Err(error) = write_frame(stream, &response, 1024 * 1024) {
        panic!("response write failed: {error:?}");
    }
}

#[test]
fn legacy_generic_balance_proof_never_establishes_state() {
    let socket = SocketPath::new("levels");
    let listener = match UnixListener::bind(&socket.0) {
        Ok(listener) => listener,
        Err(error) => panic!("listener failed: {error}"),
    };
    let (leaf, proof, authorization, account_id, asset_id) = state_fixture(5);
    let server_leaf = leaf.clone();
    let server_proof = proof.clone();
    let server = thread::spawn(move || {
        for expected_rank in [3_u8, 4] {
            let (mut stream, _) = match listener.accept() {
                Ok(connection) => connection,
                Err(error) => panic!("accept failed: {error}"),
            };
            let (correlation, selector) = request(&mut stream, 7);
            assert_eq!(selector.last().copied(), Some(expected_rank));
            respond(&mut stream, 8, correlation, &server_leaf, &server_proof);
        }
    });
    let gate = ConnectionGate::new(1);
    {
        let mut transport = match Uds::connect(&socket.0, &gate, limits()) {
            Ok(transport) => transport,
            Err(error) => panic!("state-proven connection failed: {error:?}"),
        };
        assert_eq!(
            balance(
                &mut transport,
                account_id,
                asset_id,
                context(VerificationLevel::STATE_PROVEN, authorization),
            ),
            Err(ReadError::ProductionEvidence(EvidenceError::Malformed))
        );
    }
    {
        let mut transport = match Uds::connect(&socket.0, &gate, limits()) {
            Ok(transport) => transport,
            Err(error) => panic!("checkpoint connection failed: {error:?}"),
        };
        assert_eq!(
            balance(
                &mut transport,
                account_id,
                asset_id,
                context(VerificationLevel::CHECKPOINT_FINALISED, authorization),
            ),
            Err(ReadError::ProductionEvidence(EvidenceError::Malformed))
        );
    }
    assert!(server.join().is_ok(), "level node panicked");
}

#[test]
fn unverified_account_bytes_remain_opaque_and_module_reads_fail_closed() {
    let socket = SocketPath::new("opaque");
    let listener = match UnixListener::bind(&socket.0) {
        Ok(listener) => listener,
        Err(error) => panic!("listener failed: {error}"),
    };
    let (_, _, authorization, _, _) = state_fixture(0);
    let server = thread::spawn(move || {
        let (mut stream, _) = match listener.accept() {
            Ok(connection) => connection,
            Err(error) => panic!("accept failed: {error}"),
        };
        let (correlation, _) = request(&mut stream, 7);
        respond(&mut stream, 8, correlation, b"account-core-bytes", &[]);
    });
    let gate = ConnectionGate::new(1);
    let mut transport = match Uds::connect(&socket.0, &gate, limits()) {
        Ok(transport) => transport,
        Err(error) => panic!("opaque connection failed: {error:?}"),
    };
    let read_context = context(VerificationLevel::UNVERIFIED, authorization);
    let account_value = match account(&mut transport, [1; 32], read_context) {
        Ok(value) => value,
        Err(error) => panic!("account read failed: {error:?}"),
    };
    assert_eq!(account_value.canonical_bytes(), b"account-core-bytes");
    assert_eq!(
        module_state(&mut transport, 6, b"position", read_context),
        Err(ReadError::UnavailableCapability)
    );
    assert!(server.join().is_ok(), "opaque node panicked");
}

#[test]
fn history_cursor_resumes_without_gap_or_repetition_and_preserves_bytes() {
    let socket = SocketPath::new("history");
    let listener = match UnixListener::bind(&socket.0) {
        Ok(listener) => listener,
        Err(error) => panic!("listener failed: {error}"),
    };
    let (_, _, authorization, _, _) = state_fixture(0);
    let server = thread::spawn(move || {
        let (mut stream, _) = match listener.accept() {
            Ok(connection) => connection,
            Err(error) => panic!("accept failed: {error}"),
        };
        let (correlation, first_selector) = request(&mut stream, 9);
        assert_eq!(&first_selector[..8], &10_u64.to_be_bytes());
        send_history_item(&mut stream, correlation, 10, b"activity-10");
        send_history_item(&mut stream, correlation, 11, b"activity-11");
        respond(&mut stream, 11, correlation, &12_u64.to_be_bytes(), &[]);
        let (correlation, second_selector) = request(&mut stream, 9);
        assert_eq!(&second_selector[..8], &12_u64.to_be_bytes());
        send_history_item(&mut stream, correlation, 12, b"activity-12");
        respond(&mut stream, 11, correlation, &13_u64.to_be_bytes(), &[]);
    });
    let gate = ConnectionGate::new(1);
    let mut transport = match Uds::connect(&socket.0, &gate, limits()) {
        Ok(transport) => transport,
        Err(error) => panic!("history connection failed: {error:?}"),
    };
    let read_context = context(VerificationLevel::UNVERIFIED, authorization);
    let first = match history(&mut transport, 10, 12, 2, None, read_context) {
        Ok(page) => page,
        Err(error) => panic!("first page failed: {error:?}"),
    };
    assert_eq!(first.items.len(), 2);
    assert_eq!(first.items[0].canonical_bytes(), b"activity-10");
    assert_eq!(first.items[1].canonical_bytes(), b"activity-11");
    let cursor: HistoryCursor = match first.cursor {
        Some(cursor) => cursor,
        None => panic!("first page omitted cursor"),
    };
    assert_eq!(cursor.next_sequence(), 12);
    let second = match history(&mut transport, 10, 12, 2, Some(cursor), read_context) {
        Ok(page) => page,
        Err(error) => panic!("second page failed: {error:?}"),
    };
    assert_eq!(second.items.len(), 1);
    assert_eq!(second.items[0].global_sequence, 12);
    assert_eq!(second.items[0].canonical_bytes(), b"activity-12");
    assert_eq!(second.cursor, None);
    assert!(server.join().is_ok(), "history node panicked");
}

#[test]
fn history_gap_is_explicit() {
    let socket = SocketPath::new("gap");
    let listener = match UnixListener::bind(&socket.0) {
        Ok(listener) => listener,
        Err(error) => panic!("listener failed: {error}"),
    };
    let (_, _, authorization, _, _) = state_fixture(0);
    let server = thread::spawn(move || {
        let (mut stream, _) = match listener.accept() {
            Ok(connection) => connection,
            Err(error) => panic!("accept failed: {error}"),
        };
        let (correlation, _) = request(&mut stream, 9);
        send_history_item(&mut stream, correlation, 11, b"skipped-ten");
    });
    let gate = ConnectionGate::new(1);
    let mut transport = match Uds::connect(&socket.0, &gate, limits()) {
        Ok(transport) => transport,
        Err(error) => panic!("gap connection failed: {error:?}"),
    };
    assert_eq!(
        history(
            &mut transport,
            10,
            12,
            2,
            None,
            context(VerificationLevel::UNVERIFIED, authorization),
        ),
        Err(ReadError::HistoryGap {
            expected: 10,
            actual: 11,
        })
    );
    assert!(server.join().is_ok(), "gap node panicked");
}

fn send_history_item(stream: &mut UnixStream, correlation_id: u64, sequence: u64, payload: &[u8]) {
    let mut metadata = vec![1];
    metadata.extend_from_slice(&sequence.to_be_bytes());
    respond(stream, 10, correlation_id, payload, &metadata);
}
