use std::fs;
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::thread::{self, JoinHandle};
use std::time::Duration;

use ed25519_dalek::{Signer as _, SigningKey};
use layerx_client::client::ReconnectPolicy;
use layerx_client::lni::framing::{read_frame, write_frame};
use layerx_client::lni::schema::{decode_envelope, encode_envelope, Envelope, Version};
use layerx_client::lni::transport::{ConnectionGate, Limits, Uds};
use layerx_client::receipt::{
    lookup, resolve_unknown, Lookup, LookupContext, ReceiptError, ReceiptSelector, Resolution,
};
use layerx_client::submit::{submit_signed, Submission, SubmissionContext, Unknown};
use layerx_crypto::SignatureMessage;
use layerx_proof::receipt::AuthorizedBatch;
use layerx_types::payload::{ActivityType, ModuleId, ModuleRegistration, ModuleRegistry};
use layerx_types::verify::VerificationLevel;
use layerx_wire::activity::decode_signed;
use layerx_wire::encode::Encoder;
use layerx_wire::hash::{activity_id, receipt_digest, Domain};
use layerx_wire::limits::PROTOCOL_VERSION;

static NEXT_SOCKET: AtomicU64 = AtomicU64::new(1);
const IDEMPOTENCY_KEY: [u8; 32] = [0x81; 32];

struct SocketPath(PathBuf);

impl SocketPath {
    fn new(label: &str) -> Self {
        let sequence = NEXT_SOCKET.fetch_add(1, Ordering::Relaxed);
        Self(std::env::temp_dir().join(format!(
            "layerx-receipt-{label}-{}-{sequence}.sock",
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

fn registry() -> ModuleRegistry {
    let activity = match ActivityType::new(ModuleId::Asset, 1) {
        Ok(activity) => activity,
        Err(error) => panic!("activity type rejected: {error:?}"),
    };
    let registration = match ModuleRegistration::new(ModuleId::Asset, &[activity]) {
        Ok(registration) => registration,
        Err(error) => panic!("registration rejected: {error:?}"),
    };
    match ModuleRegistry::new(&[registration]) {
        Ok(registry) => registry,
        Err(error) => panic!("registry rejected: {error:?}"),
    }
}

fn activity_fields(encoder: &mut Encoder, public_key: &[u8; 32]) {
    assert!(encoder.tag(1, 12).is_ok());
    assert!(encoder.u16(PROTOCOL_VERSION).is_ok());
    assert!(encoder.tag(2, 12).is_ok());
    assert!(encoder.u32(77).is_ok());
    assert!(encoder.tag(3, 12).is_ok());
    assert!(encoder.u32(0x0001_0001).is_ok());
    assert!(encoder.tag(4, 12).is_ok());
    assert!(encoder.bytes(b"did:layerx:receipt", 255).is_ok());
    assert!(encoder.tag(5, 12).is_ok());
    assert!(encoder.bytes(public_key, 524_288).is_ok());
    assert!(encoder.tag(6, 12).is_ok());
    assert!(encoder.u64(9).is_ok());
    assert!(encoder.tag(7, 12).is_ok());
    assert!(encoder.u64(10).is_ok());
    assert!(encoder.u64(100).is_ok());
    assert!(encoder.tag(8, 12).is_ok());
    assert!(encoder.bytes(&IDEMPOTENCY_KEY, 32).is_ok());
    assert!(encoder.tag(9, 12).is_ok());
    assert!(encoder.u128(1000).is_ok());
    assert!(encoder.tag(10, 12).is_ok());
    assert!(encoder.bytes(&[0x91; 32], 32).is_ok());
    assert!(encoder.tag(11, 12).is_ok());
    assert!(encoder.bytes(&[0x42, 0x43], 524_288).is_ok());
}

fn signed_activity() -> (Vec<u8>, [u8; 32], [u8; 32]) {
    let key = SigningKey::from_bytes(&[0x31; 32]);
    let public_key = key.verifying_key().to_bytes();
    let mut unsigned = Encoder::new(4096);
    assert!(unsigned
        .structure_header_version(0x1001, PROTOCOL_VERSION)
        .is_ok());
    assert!(unsigned.u8(11).is_ok());
    activity_fields(&mut unsigned, &public_key);
    let unsigned = unsigned.finish();
    let message =
        match SignatureMessage::new(Domain::SignaturePreimage, PROTOCOL_VERSION, 77, &unsigned) {
            Ok(message) => message,
            Err(error) => panic!("signature scope rejected: {error:?}"),
        };
    let signature = key.sign(&message.digest()).to_bytes();
    let mut signed = Encoder::new(4096);
    assert!(signed
        .structure_header_version(0x1001, PROTOCOL_VERSION)
        .is_ok());
    assert!(signed.u8(12).is_ok());
    activity_fields(&mut signed, &public_key);
    assert!(signed.tag(12, 12).is_ok());
    assert!(signed.bytes(&signature, 128).is_ok());
    let bytes = signed.finish();
    let decoded = match decode_signed(&bytes, &registry()) {
        Ok(decoded) => decoded,
        Err(error) => panic!("signed activity rejected: {error:?}"),
    };
    let identifier = match activity_id(&decoded) {
        Ok(identifier) => identifier,
        Err(error) => panic!("activity identifier failed: {error:?}"),
    };
    (bytes, public_key, identifier)
}

fn receipt(activity_identifier: [u8; 32]) -> (Vec<u8>, AuthorizedBatch) {
    let key = SigningKey::from_bytes(&[0x63; 32]);
    let encode = |signature: Option<[u8; 64]>| {
        let mut encoder = Encoder::new(4096);
        assert!(encoder
            .structure_header_version(0x5201, PROTOCOL_VERSION)
            .is_ok());
        assert!(encoder.u16(PROTOCOL_VERSION).is_ok());
        assert!(encoder.bytes(&activity_identifier, 32).is_ok());
        assert!(encoder.u64(9).is_ok());
        assert!(encoder.bytes(&[2; 32], 32).is_ok());
        assert!(encoder.bytes(&[3; 32], 32).is_ok());
        assert!(encoder.bytes(&[8; 32], 32).is_ok());
        assert!(encoder.i32(0).is_ok());
        assert!(encoder.sequence_length(0, 512).is_ok());
        assert!(encoder.u128(1).is_ok());
        assert!(encoder.bytes(&[4; 32], 32).is_ok());
        assert!(encoder.u16(1).is_ok());
        assert!(encoder.u32(1).is_ok());
        assert!(encoder.u32(1).is_ok());
        assert!(encoder.u8(1).is_ok());
        assert!(encoder.bytes(&[5; 32], 32).is_ok());
        assert!(encoder.u128(25).is_ok());
        assert!(encoder.bytes(&[6; 32], 32).is_ok());
        assert!(encoder.u128(100).is_ok());
        assert!(encoder.u128(75).is_ok());
        assert!(encoder.u64(1).is_ok());
        assert!(encoder.bytes(&[7; 32], 32).is_ok());
        assert!(encoder.u128(10).is_ok());
        assert!(encoder.u128(35).is_ok());
        assert!(encoder.bytes(&[9; 32], 32).is_ok());
        assert!(encoder.bytes(&[10; 32], 32).is_ok());
        assert!(encoder.bytes(&[11; 32], 32).is_ok());
        assert!(encoder.u64(1_000).is_ok());
        assert!(encoder.u8(u8::from(signature.is_some())).is_ok());
        if let Some(value) = signature {
            assert!(encoder.bytes(&value, 64).is_ok());
        }
        encoder.finish()
    };
    let unsigned = encode(None);
    let digest = match receipt_digest(&unsigned) {
        Ok(digest) => digest,
        Err(error) => panic!("receipt digest failed: {error:?}"),
    };
    let signed = encode(Some(key.sign(&digest).to_bytes()));
    let authorised = AuthorizedBatch::new(
        [4; 32],
        [5; 32],
        [2; 32],
        [3; 32],
        key.verifying_key().to_bytes(),
    );
    (signed, authorised)
}

struct ReceivedEnvelope {
    correlation_id: u64,
    canonical_payload: Vec<u8>,
}

fn receive_envelope(stream: &mut UnixStream, expected_tag: u16) -> ReceivedEnvelope {
    let frame = match read_frame(stream, 1024 * 1024) {
        Ok(frame) => frame,
        Err(error) => panic!("request frame failed: {error:?}"),
    };
    let decoded = match decode_envelope(&frame) {
        Ok(decoded) => decoded,
        Err(error) => panic!("request envelope failed: {error:?}"),
    };
    assert_eq!(decoded.message_tag, expected_tag);
    ReceivedEnvelope {
        correlation_id: decoded.correlation_id,
        canonical_payload: decoded.canonical_payload.to_vec(),
    }
}

fn send_receipt(stream: &mut UnixStream, correlation_id: u64, receipt: &[u8]) {
    let response = match encode_envelope(Envelope {
        version: Version::V1_0,
        message_tag: 6,
        correlation_id,
        canonical_payload: receipt,
        proof_material: &[],
    }) {
        Ok(response) => response,
        Err(error) => panic!("receipt response encoding failed: {error:?}"),
    };
    if let Err(error) = write_frame(stream, &response, 1024 * 1024) {
        panic!("receipt response failed: {error:?}");
    }
}

fn spawn_resolution_node(
    socket: &SocketPath,
    responses: Vec<Vec<u8>>,
) -> JoinHandle<(Vec<u8>, Vec<Vec<u8>>)> {
    let listener = match UnixListener::bind(&socket.0) {
        Ok(listener) => listener,
        Err(error) => panic!("listener failed: {error}"),
    };
    thread::spawn(move || {
        let (mut submission, _) = match listener.accept() {
            Ok(connection) => connection,
            Err(error) => panic!("submission accept failed: {error}"),
        };
        let submitted = receive_envelope(&mut submission, 3).canonical_payload;
        drop(submission);
        let (mut resolution, _) = match listener.accept() {
            Ok(connection) => connection,
            Err(error) => panic!("resolution accept failed: {error}"),
        };
        let mut selectors = Vec::new();
        for response in responses {
            let request = receive_envelope(&mut resolution, 5);
            selectors.push(request.canonical_payload);
            send_receipt(&mut resolution, request.correlation_id, &response);
        }
        (submitted, selectors)
    })
}

fn unknown(socket: &SocketPath, signed: &[u8], public_key: [u8; 32]) -> Unknown {
    let gate = ConnectionGate::new(1);
    let mut transport = match Uds::connect(&socket.0, &gate, limits()) {
        Ok(transport) => transport,
        Err(error) => panic!("submission connection failed: {error:?}"),
    };
    let context = SubmissionContext {
        interface_version: Version::V1_0,
        protocol_version: PROTOCOL_VERSION,
        network_id: 77,
        correlation_id: 1,
        signer_public_key: public_key,
        attempt: 1,
    };
    let outcome = match submit_signed(&mut transport, &registry(), context, signed) {
        Ok(outcome) => outcome,
        Err(error) => panic!("submission failed before transmission: {error:?}"),
    };
    let Submission::Unknown(unknown) = outcome else {
        panic!("lost submission response was not unknown");
    };
    unknown
}

fn policy(maximum_attempts: u8) -> ReconnectPolicy {
    ReconnectPolicy {
        maximum_attempts,
        base_delay: Duration::from_millis(1),
        maximum_delay: Duration::from_millis(4),
        jitter_percent: 10,
    }
}

#[test]
fn resolves_a_receipt_that_appears_late_without_resubmitting() {
    let socket = SocketPath::new("late");
    let (signed, public_key, identifier) = signed_activity();
    let (receipt_bytes, authorised) = receipt(identifier);
    let server = spawn_resolution_node(&socket, vec![Vec::new(), receipt_bytes.clone()]);
    let unknown = unknown(&socket, &signed, public_key);
    assert_eq!(unknown.retry_bytes(), signed);
    let gate = ConnectionGate::new(1);
    let mut transport = match Uds::connect(&socket.0, &gate, limits()) {
        Ok(transport) => transport,
        Err(error) => panic!("resolution connection failed: {error:?}"),
    };
    let resolution = match resolve_unknown(
        &mut transport,
        &unknown,
        LookupContext {
            interface_version: Version::V1_0,
            correlation_id: 10,
            authorised_batch: authorised,
        },
        policy(2),
    ) {
        Ok(resolution) => resolution,
        Err(error) => panic!("late receipt resolution failed: {error:?}"),
    };
    let Resolution::Resolved(verified) = resolution else {
        panic!("late receipt remained unknown");
    };
    assert_eq!(verified.canonical_bytes(), receipt_bytes);
    assert_eq!(verified.level(), VerificationLevel::SEQUENCER_SIGNED);
    let Ok((submitted, selectors)) = server.join() else {
        panic!("late receipt node panicked");
    };
    assert_eq!(submitted, signed);
    assert_eq!(selectors, vec![selector_bytes(2), selector_bytes(2)]);
}

#[test]
fn receipt_absence_through_deadline_remains_unknown() {
    let socket = SocketPath::new("absent");
    let (signed, public_key, identifier) = signed_activity();
    let (_, authorised) = receipt(identifier);
    let server = spawn_resolution_node(&socket, vec![Vec::new(), Vec::new(), Vec::new()]);
    let unknown = unknown(&socket, &signed, public_key);
    let gate = ConnectionGate::new(1);
    let mut transport = match Uds::connect(&socket.0, &gate, limits()) {
        Ok(transport) => transport,
        Err(error) => panic!("resolution connection failed: {error:?}"),
    };
    let resolution = match resolve_unknown(
        &mut transport,
        &unknown,
        LookupContext {
            interface_version: Version::V1_0,
            correlation_id: 20,
            authorised_batch: authorised,
        },
        policy(3),
    ) {
        Ok(resolution) => resolution,
        Err(error) => panic!("absence resolution failed: {error:?}"),
    };
    let Resolution::Unknown(still_unknown) = resolution else {
        panic!("absent receipt resolved");
    };
    assert_eq!(still_unknown.resolution_attempts(), 3);
    assert_eq!(still_unknown.retry_bytes(), signed);
    let Ok((submitted, selectors)) = server.join() else {
        panic!("absence node panicked");
    };
    assert_eq!(submitted, signed);
    assert_eq!(selectors.len(), 3);
}

#[test]
fn refuses_a_verified_receipt_for_a_different_activity() {
    let socket = SocketPath::new("mismatch");
    let (signed, public_key, identifier) = signed_activity();
    let mut wrong_identifier = identifier;
    wrong_identifier[0] ^= 1;
    let (wrong_receipt, authorised) = receipt(wrong_identifier);
    let server = spawn_resolution_node(&socket, vec![wrong_receipt]);
    let unknown = unknown(&socket, &signed, public_key);
    let gate = ConnectionGate::new(1);
    let mut transport = match Uds::connect(&socket.0, &gate, limits()) {
        Ok(transport) => transport,
        Err(error) => panic!("resolution connection failed: {error:?}"),
    };
    assert_eq!(
        resolve_unknown(
            &mut transport,
            &unknown,
            LookupContext {
                interface_version: Version::V1_0,
                correlation_id: 30,
                authorised_batch: authorised,
            },
            policy(1),
        ),
        Err(ReceiptError::ActivityMismatch {
            expected: identifier,
            actual: wrong_identifier,
        })
    );
    let Ok((submitted, selectors)) = server.join() else {
        panic!("mismatch node panicked");
    };
    assert_eq!(submitted, signed);
    assert_eq!(selectors.len(), 1);
}

#[test]
fn lookup_supports_activity_id_idempotency_key_and_sequence() {
    let socket = SocketPath::new("selectors");
    let listener = match UnixListener::bind(&socket.0) {
        Ok(listener) => listener,
        Err(error) => panic!("listener failed: {error}"),
    };
    let (_, _, identifier) = signed_activity();
    let (receipt_bytes, authorised) = receipt(identifier);
    let server_receipt = receipt_bytes.clone();
    let server = thread::spawn(move || {
        let (mut stream, _) = match listener.accept() {
            Ok(connection) => connection,
            Err(error) => panic!("lookup accept failed: {error}"),
        };
        let mut selectors = Vec::new();
        for _ in 0..3 {
            let request = receive_envelope(&mut stream, 5);
            selectors.push(request.canonical_payload);
            send_receipt(&mut stream, request.correlation_id, &server_receipt);
        }
        selectors
    });
    let gate = ConnectionGate::new(1);
    let mut transport = match Uds::connect(&socket.0, &gate, limits()) {
        Ok(transport) => transport,
        Err(error) => panic!("lookup connection failed: {error:?}"),
    };
    let selectors = [
        ReceiptSelector::ActivityId(identifier),
        ReceiptSelector::IdempotencyKey {
            idempotency_key: IDEMPOTENCY_KEY,
            expected_activity_id: identifier,
        },
        ReceiptSelector::GlobalSequence(9),
    ];
    for (index, selector) in selectors.into_iter().enumerate() {
        let lookup = match lookup(
            &mut transport,
            selector,
            LookupContext {
                interface_version: Version::V1_0,
                correlation_id: 100 + u64::try_from(index).unwrap_or_default(),
                authorised_batch: authorised,
            },
        ) {
            Ok(lookup) => lookup,
            Err(error) => panic!("selector lookup failed: {error:?}"),
        };
        let Lookup::Verified(verified) = lookup else {
            panic!("selector lookup returned absence");
        };
        assert_eq!(verified.canonical_bytes(), receipt_bytes);
    }
    let Ok(observed) = server.join() else {
        panic!("selector node panicked");
    };
    assert_eq!(observed[0][0], 1);
    assert_eq!(observed[1][0], 2);
    assert_eq!(observed[2], selector_sequence(9));
}

fn selector_bytes(tag: u8) -> Vec<u8> {
    let mut bytes = vec![tag];
    bytes.extend_from_slice(&IDEMPOTENCY_KEY);
    bytes
}

fn selector_sequence(sequence: u64) -> Vec<u8> {
    let mut bytes = vec![3];
    bytes.extend_from_slice(&sequence.to_be_bytes());
    bytes
}
