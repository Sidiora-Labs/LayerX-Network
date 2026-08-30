use std::fs;
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::thread;
use std::time::Duration;

use layerx_agentd::read::{history, HistoryLimits, HistoryReadError};
use layerx_client::evidence::RootSelector;
use layerx_client::head::Head;
use layerx_client::lni::framing::{read_frame, write_frame};
use layerx_client::lni::schema::{decode_envelope, encode_envelope, Envelope, Version};
use layerx_client::lni::transport::{ConnectionGate, Limits, Uds};
use layerx_client::read::{ReadContext, Requested};
use layerx_proof::inclusion::SequencerAuthorization;
use layerx_types::verify::VerificationLevel;
use sha2::{Digest, Sha256};

static NEXT_SOCKET: AtomicU64 = AtomicU64::new(1);

struct SocketPath(PathBuf);

impl SocketPath {
    fn new() -> Self {
        let sequence = NEXT_SOCKET.fetch_add(1, Ordering::Relaxed);
        Self(std::env::temp_dir().join(format!(
            "layerx-agentd-history-{}-{sequence}.sock",
            std::process::id()
        )))
    }
}

impl Drop for SocketPath {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.0);
    }
}

fn transport_limits() -> Limits {
    Limits {
        maximum_frame_bytes: 1024 * 1024,
        maximum_connections: 1,
        maximum_streams: 4,
        maximum_queued_bytes: 2 * 1024 * 1024,
        deadline: Duration::from_secs(2),
    }
}

fn history_limits(oldest: u64) -> HistoryLimits {
    HistoryLimits {
        maximum_items: 3,
        maximum_response_bytes: 20,
        oldest_available_sequence: oldest,
    }
}

fn context(head: u64) -> ReadContext {
    ReadContext {
        interface_version: Version::V1_2,
        correlation_id: 91,
        expected_protocol_version: 1,
        expected_network_id: 77,
        requested: Requested::new(VerificationLevel::UNVERIFIED),
        head: Head {
            chain_sequence: head,
            sealed_batch: 7,
            finalised_checkpoint: [0x71; 32],
        },
        sequencer_authorization: SequencerAuthorization::new([1; 32], [2; 32], 0, u64::MAX),
        handshake_sequencer_key: [2; 32],
        root_selector: RootSelector::Latest,
    }
}

fn request(stream: &mut UnixStream, expected_start: u64) -> u64 {
    let frame =
        read_frame(stream, 1024 * 1024).unwrap_or_else(|error| panic!("request frame: {error:?}"));
    let envelope =
        decode_envelope(&frame).unwrap_or_else(|error| panic!("request envelope: {error:?}"));
    assert_eq!(envelope.message_tag, 9);
    assert_eq!(
        envelope.canonical_payload.get(..8),
        Some(expected_start.to_be_bytes().as_slice())
    );
    envelope.correlation_id
}

fn respond(stream: &mut UnixStream, tag: u16, correlation: u64, payload: &[u8], proof: &[u8]) {
    let envelope = encode_envelope(Envelope {
        version: Version::V1_0,
        message_tag: tag,
        correlation_id: correlation,
        canonical_payload: payload,
        proof_material: proof,
    })
    .unwrap_or_else(|error| panic!("response envelope: {error:?}"));
    write_frame(stream, &envelope, 1024 * 1024)
        .unwrap_or_else(|error| panic!("response frame: {error:?}"));
}

fn item(stream: &mut UnixStream, correlation: u64, sequence: u64, bytes: &[u8]) {
    let mut metadata = vec![match sequence % 3 {
        0 => 1,
        1 => 2,
        _ => 3,
    }];
    metadata.extend_from_slice(&sequence.to_be_bytes());
    respond(stream, 10, correlation, bytes, &metadata);
}

#[test]
fn bounded_cursor_survives_restart_and_excludes_concurrent_appends() {
    let socket = SocketPath::new();
    let listener =
        UnixListener::bind(&socket.0).unwrap_or_else(|error| panic!("listener: {error}"));
    let server = thread::spawn(move || {
        let (mut first, _) = listener
            .accept()
            .unwrap_or_else(|error| panic!("first accept: {error}"));
        let correlation = request(&mut first, 10);
        item(&mut first, correlation, 10, b"activity10");
        item(&mut first, correlation, 11, b"receipt-11");
        item(&mut first, correlation, 12, b"event--012");
        respond(&mut first, 11, correlation, &13_u64.to_be_bytes(), &[]);
        drop(first);

        let (mut resumed, _) = listener
            .accept()
            .unwrap_or_else(|error| panic!("resumed accept: {error}"));
        let correlation = request(&mut resumed, 12);
        item(&mut resumed, correlation, 12, b"event--012");
        respond(&mut resumed, 11, correlation, &13_u64.to_be_bytes(), &[]);
    });

    let gate = ConnectionGate::new(1);
    let mut first_transport = Uds::connect(&socket.0, &gate, transport_limits())
        .unwrap_or_else(|error| panic!("first connect: {error:?}"));
    let first = history(
        &mut first_transport,
        10,
        12,
        None,
        history_limits(10),
        context(20),
    )
    .unwrap_or_else(|error| panic!("first page: {error:?}"));
    assert_eq!(first.items.len(), 2);
    assert_eq!(first.response_bytes, 20);
    assert_eq!(first.items[0].canonical_bytes(), b"activity10");
    assert_eq!(first.items[1].canonical_bytes(), b"receipt-11");
    let cursor = first.cursor.unwrap_or_else(|| panic!("cursor missing"));
    assert_eq!(cursor.next_sequence, 12);
    assert!(matches!(
        history(
            &mut first_transport,
            10,
            12,
            Some(cursor),
            history_limits(13),
            context(20),
        ),
        Err(HistoryReadError::PrunedRange {
            requested: 12,
            oldest: 13
        })
    ));
    assert!(matches!(
        history(
            &mut first_transport,
            10,
            12,
            Some(cursor),
            history_limits(10),
            context(21),
        ),
        Err(HistoryReadError::CursorMismatch)
    ));
    drop(first_transport);

    let mut resumed_transport = Uds::connect(&socket.0, &gate, transport_limits())
        .unwrap_or_else(|error| panic!("resumed connect: {error:?}"));
    let resumed = history(
        &mut resumed_transport,
        10,
        12,
        Some(cursor),
        history_limits(10),
        context(20),
    )
    .unwrap_or_else(|error| panic!("resumed page: {error:?}"));
    assert_eq!(resumed.items.len(), 1);
    assert_eq!(resumed.items[0].global_sequence, 12);
    assert_eq!(resumed.items[0].canonical_bytes(), b"event--012");
    assert!(resumed.cursor.is_none());

    let original_hash: [u8; 32] = Sha256::digest(b"event--012").into();
    let served_hash: [u8; 32] = Sha256::digest(resumed.items[0].canonical_bytes()).into();
    assert_eq!(served_hash, original_hash);
    assert!(server.join().is_ok(), "history node failed");
}
