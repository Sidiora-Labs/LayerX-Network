use std::fs;
use std::io::{Read, Write};
use std::os::unix::net::UnixListener;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::thread;
use std::time::Duration;

use ed25519_dalek::{Signer as _, SigningKey};
use layerx_client::client::{Client, ClientConfig, ReconnectPolicy};
use layerx_client::lni::framing::{read_frame, write_frame};
use layerx_client::lni::handshake::{encode_node_info, HandshakeConfig, NodeInfo, NodeRole};
use layerx_client::lni::schema::{decode_envelope, encode_envelope, Envelope, Version};
use layerx_client::lni::transport::{ConnectionGate, Limits, Uds};
use layerx_client::submit::{
    submit_signed, Submission, SubmissionContext, SubmitError, UnknownCause,
};
use layerx_crypto::SignatureMessage;
use layerx_types::payload::{ActivityType, ModuleId, ModuleRegistration, ModuleRegistry};
use layerx_wire::encode::Encoder;
use layerx_wire::hash::Domain;
use layerx_wire::limits::PROTOCOL_VERSION;

static NEXT_SOCKET: AtomicU64 = AtomicU64::new(1);
const IDEMPOTENCY_KEY: [u8; 32] = [0x81; 32];

struct SocketPath(PathBuf);

impl SocketPath {
    fn new(label: &str) -> Self {
        let sequence = NEXT_SOCKET.fetch_add(1, Ordering::Relaxed);
        Self(std::env::temp_dir().join(format!(
            "layerx-submit-{label}-{}-{sequence}.sock",
            std::process::id()
        )))
    }
}

impl Drop for SocketPath {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.0);
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

fn fields(encoder: &mut Encoder, public_key: &[u8; 32]) {
    assert!(encoder.tag(1, 12).is_ok());
    assert!(encoder.u16(PROTOCOL_VERSION).is_ok());
    assert!(encoder.tag(2, 12).is_ok());
    assert!(encoder.u32(77).is_ok());
    assert!(encoder.tag(3, 12).is_ok());
    assert!(encoder.u32(0x0001_0001).is_ok());
    assert!(encoder.tag(4, 12).is_ok());
    assert!(encoder.bytes(b"did:layerx:submitter", 255).is_ok());
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

fn signed_activity() -> (Vec<u8>, [u8; 32]) {
    let key = SigningKey::from_bytes(&[0x31; 32]);
    let public_key = key.verifying_key().to_bytes();
    let mut unsigned = Encoder::new(4096);
    assert!(unsigned
        .structure_header_version(0x1001, PROTOCOL_VERSION)
        .is_ok());
    assert!(unsigned.u8(11).is_ok());
    fields(&mut unsigned, &public_key);
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
    fields(&mut signed, &public_key);
    assert!(signed.tag(12, 12).is_ok());
    assert!(signed.bytes(&signature, 128).is_ok());
    (signed.finish(), public_key)
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

fn context(public_key: [u8; 32], attempt: u32) -> SubmissionContext {
    context_at(Version::V1_0, public_key, attempt)
}

fn context_at(interface_version: Version, public_key: [u8; 32], attempt: u32) -> SubmissionContext {
    SubmissionContext {
        interface_version,
        protocol_version: PROTOCOL_VERSION,
        network_id: 77,
        correlation_id: 42,
        signer_public_key: public_key,
        attempt,
    }
}

#[test]
fn retries_and_reconnects_preserve_exact_signed_bytes_under_fragmentation() {
    let (signed, public_key) = signed_activity();
    let response_bytes = signed.clone();
    let decoded = layerx_wire::activity::decode_signed(&signed, &registry())
        .unwrap_or_else(|error| panic!("activity decode failed: {error:?}"));
    let response_id = layerx_wire::hash::activity_id(&decoded)
        .unwrap_or_else(|error| panic!("activity id failed: {error:?}"));
    let socket = SocketPath::new("exact");
    let listener = match UnixListener::bind(&socket.0) {
        Ok(listener) => listener,
        Err(error) => panic!("listener failed: {error}"),
    };
    let server = thread::spawn(move || {
        let mut observed = Vec::new();
        for attempt in 1..=3 {
            let (mut stream, _) = match listener.accept() {
                Ok(connection) => connection,
                Err(error) => panic!("accept failed: {error}"),
            };
            let frame = match read_frame(&mut stream, 1024 * 1024) {
                Ok(frame) => frame,
                Err(error) => panic!("request frame failed: {error:?}"),
            };
            let envelope = match decode_envelope(&frame) {
                Ok(envelope) => envelope,
                Err(error) => panic!("request envelope failed: {error:?}"),
            };
            assert_eq!(envelope.message_tag, 3);
            observed.push(envelope.canonical_payload.to_vec());
            if attempt == 3 {
                let response = match encode_envelope(Envelope {
                    version: Version::V1_0,
                    message_tag: 4,
                    correlation_id: envelope.correlation_id,
                    canonical_payload: &response_bytes,
                    proof_material: &response_id,
                }) {
                    Ok(response) => response,
                    Err(error) => panic!("response encoding failed: {error:?}"),
                };
                let length = match u32::try_from(response.len()) {
                    Ok(length) => length,
                    Err(error) => panic!("response length failed: {error}"),
                };
                let mut framed = length.to_be_bytes().to_vec();
                framed.extend_from_slice(&response);
                for chunk in framed.chunks(3) {
                    if let Err(error) = stream.write_all(chunk) {
                        panic!("fragmented response failed: {error}");
                    }
                }
            }
        }
        observed
    });

    let gate = ConnectionGate::new(1);
    for attempt in 1..=3 {
        let mut transport = match Uds::connect(&socket.0, &gate, limits()) {
            Ok(transport) => transport,
            Err(error) => panic!("connection {attempt} failed: {error:?}"),
        };
        let outcome = match submit_signed(
            &mut transport,
            &registry(),
            context_at(Version::V1_3, public_key, attempt),
            &signed,
        ) {
            Ok(outcome) => outcome,
            Err(error) => panic!("submission {attempt} failed before transmission: {error:?}"),
        };
        if attempt < 3 {
            let Submission::Unknown(unknown) = outcome else {
                panic!("lost response was not unknown");
            };
            assert_eq!(unknown.idempotency_key(), IDEMPOTENCY_KEY);
            assert_eq!(unknown.attempt(), attempt);
            assert!(matches!(unknown.cause(), UnknownCause::Transport(_)));
        } else {
            let Submission::Acknowledged(acknowledgement) = outcome else {
                panic!("complete response was not acknowledged");
            };
            assert_eq!(acknowledgement.admission_bytes(), signed);
            assert_eq!(acknowledgement.core_evidence(), response_id);
        }
    }
    let Ok(observed) = server.join() else {
        panic!("submission server panicked");
    };
    assert_eq!(observed, vec![signed.clone(), signed.clone(), signed]);
}

#[test]
fn mismatched_submit_acknowledgement_is_indeterminate() {
    let (signed, public_key) = signed_activity();
    let response_bytes = signed.clone();
    let decoded = layerx_wire::activity::decode_signed(&signed, &registry())
        .unwrap_or_else(|error| panic!("activity decode failed: {error:?}"));
    let response_id = layerx_wire::hash::activity_id(&decoded)
        .unwrap_or_else(|error| panic!("activity id failed: {error:?}"));
    let socket = SocketPath::new("mismatched-ack");
    let listener =
        UnixListener::bind(&socket.0).unwrap_or_else(|error| panic!("listener failed: {error}"));
    let server = thread::spawn(move || {
        for mismatch in 0..2 {
            let (mut stream, _) = listener
                .accept()
                .unwrap_or_else(|error| panic!("accept failed: {error}"));
            let request = read_frame(&mut stream, 1024 * 1024)
                .unwrap_or_else(|error| panic!("request frame failed: {error:?}"));
            let request = decode_envelope(&request)
                .unwrap_or_else(|error| panic!("request envelope failed: {error:?}"));
            let wrong_id = [0xee; 32];
            let response = encode_envelope(Envelope {
                version: Version::V1_3,
                message_tag: 4,
                correlation_id: request.correlation_id,
                canonical_payload: if mismatch == 0 {
                    &[0xac]
                } else {
                    &response_bytes
                },
                proof_material: if mismatch == 0 {
                    &response_id
                } else {
                    &wrong_id
                },
            })
            .unwrap_or_else(|error| panic!("response encoding failed: {error:?}"));
            let mut framed = u32::try_from(response.len())
                .unwrap_or_else(|error| panic!("response length failed: {error}"))
                .to_be_bytes()
                .to_vec();
            framed.extend_from_slice(&response);
            stream
                .write_all(&framed)
                .unwrap_or_else(|error| panic!("response failed: {error}"));
        }
    });
    let gate = ConnectionGate::new(1);
    for attempt in 1..=2 {
        let mut transport = Uds::connect(&socket.0, &gate, limits())
            .unwrap_or_else(|error| panic!("connection failed: {error:?}"));
        let outcome = submit_signed(
            &mut transport,
            &registry(),
            context_at(Version::V1_3, public_key, attempt),
            &signed,
        )
        .unwrap_or_else(|error| panic!("submission failed: {error:?}"));
        let Submission::Unknown(unknown) = outcome else {
            panic!("mismatched acknowledgement was accepted");
        };
        assert_eq!(unknown.cause(), UnknownCause::IndeterminateResponse);
    }
    server
        .join()
        .unwrap_or_else(|_| panic!("submission server panicked"));
}

#[test]
fn legacy_submit_acknowledgement_preserves_broad_v1_evidence() {
    let (signed, public_key) = signed_activity();
    let socket = SocketPath::new("legacy-ack");
    let listener =
        UnixListener::bind(&socket.0).unwrap_or_else(|error| panic!("listener failed: {error}"));
    let server = thread::spawn(move || {
        let (mut stream, _) = listener
            .accept()
            .unwrap_or_else(|error| panic!("accept failed: {error}"));
        let request = read_frame(&mut stream, 1024 * 1024)
            .unwrap_or_else(|error| panic!("request frame failed: {error:?}"));
        let request = decode_envelope(&request)
            .unwrap_or_else(|error| panic!("request envelope failed: {error:?}"));
        let response = encode_envelope(Envelope {
            version: Version::V1_0,
            message_tag: 4,
            correlation_id: request.correlation_id,
            canonical_payload: &[0xac],
            proof_material: &[0xed],
        })
        .unwrap_or_else(|error| panic!("response encoding failed: {error:?}"));
        let mut framed = u32::try_from(response.len())
            .unwrap_or_else(|error| panic!("response length failed: {error}"))
            .to_be_bytes()
            .to_vec();
        framed.extend_from_slice(&response);
        stream
            .write_all(&framed)
            .unwrap_or_else(|error| panic!("response failed: {error}"));
    });
    let gate = ConnectionGate::new(1);
    let mut transport = Uds::connect(&socket.0, &gate, limits())
        .unwrap_or_else(|error| panic!("connection failed: {error:?}"));
    let outcome = submit_signed(&mut transport, &registry(), context(public_key, 1), &signed)
        .unwrap_or_else(|error| panic!("legacy submission failed: {error:?}"));
    let Submission::Acknowledged(acknowledgement) = outcome else {
        panic!("legacy acknowledgement was not preserved");
    };
    assert_eq!(acknowledgement.admission_bytes(), [0xac]);
    assert_eq!(acknowledgement.core_evidence(), [0xed]);
    server
        .join()
        .unwrap_or_else(|_| panic!("submission server panicked"));
}

#[test]
fn high_level_client_refuses_legacy_submit_without_durable_capability() {
    let socket = SocketPath::new("legacy-client-gate");
    let listener =
        UnixListener::bind(&socket.0).unwrap_or_else(|error| panic!("listener failed: {error}"));
    let server = thread::spawn(move || {
        let (mut stream, _) = listener
            .accept()
            .unwrap_or_else(|error| panic!("accept failed: {error}"));
        let request = read_frame(&mut stream, 1024 * 1024)
            .unwrap_or_else(|error| panic!("request frame failed: {error:?}"));
        let request = decode_envelope(&request)
            .unwrap_or_else(|error| panic!("request envelope failed: {error:?}"));
        assert_eq!(request.version, Version::V1_0);
        assert_eq!(request.message_tag, 1);
        let node = NodeInfo {
            interface_version: Version::V1_2,
            protocol_version: PROTOCOL_VERSION,
            network_id: 77,
            role: NodeRole::Sequencer,
            chain_head_sequence: 9,
            latest_sealed_batch: 8,
            latest_finalised_checkpoint: [7; 32],
            authorised_sequencer_key: [6; 32],
            advertised_capabilities: vec!["node_info".to_owned(), "submit".to_owned()],
        };
        let payload = encode_node_info(&node)
            .unwrap_or_else(|error| panic!("node info encoding failed: {error:?}"));
        let response = encode_envelope(Envelope {
            version: Version::V1_2,
            message_tag: 2,
            correlation_id: 0,
            canonical_payload: &payload,
            proof_material: &[],
        })
        .unwrap_or_else(|error| panic!("response encoding failed: {error:?}"));
        write_frame(&mut stream, &response, 1024 * 1024)
            .unwrap_or_else(|error| panic!("response failed: {error:?}"));
        stream
            .set_read_timeout(Some(Duration::from_millis(250)))
            .unwrap_or_else(|error| panic!("read timeout failed: {error}"));
        let mut byte = [0_u8; 1];
        match stream.read(&mut byte) {
            Ok(0) => false,
            Ok(_) => true,
            Err(error)
                if matches!(
                    error.kind(),
                    std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut
                ) =>
            {
                false
            }
            Err(error) => panic!("submit observation failed: {error}"),
        }
    });
    let mut client = Client::connect(ClientConfig {
        endpoint: socket.0.clone(),
        handshake: HandshakeConfig {
            built_interface_version: Version::V1_3,
            expected_protocol_version: PROTOCOL_VERSION,
            expected_network_id: 77,
        },
        limits: limits(),
        reconnect: ReconnectPolicy {
            maximum_attempts: 1,
            base_delay: Duration::from_millis(1),
            maximum_delay: Duration::from_millis(1),
            jitter_percent: 0,
        },
    })
    .unwrap_or_else(|error| panic!("client connection failed: {error:?}"));
    let (signed, public_key) = signed_activity();
    assert!(matches!(
        client.submit_signed(&registry(), public_key, 42, 1, &signed),
        Err(SubmitError::UnavailableCapability)
    ));
    drop(client);
    let observed_submit = server
        .join()
        .unwrap_or_else(|_| panic!("legacy server panicked"));
    assert!(!observed_submit);
}

#[test]
fn invalid_signature_fails_before_any_submission_bytes_are_sent() {
    let socket = SocketPath::new("invalid");
    let listener = match UnixListener::bind(&socket.0) {
        Ok(listener) => listener,
        Err(error) => panic!("listener failed: {error}"),
    };
    let server = thread::spawn(move || {
        let (mut stream, _) = match listener.accept() {
            Ok(connection) => connection,
            Err(error) => panic!("accept failed: {error}"),
        };
        let mut observed = Vec::new();
        if let Err(error) = stream.read_to_end(&mut observed) {
            panic!("read failed: {error}");
        }
        observed
    });
    let (mut signed, public_key) = signed_activity();
    let Some(last) = signed.last_mut() else {
        panic!("signed activity unexpectedly empty");
    };
    *last ^= 1;
    let gate = ConnectionGate::new(1);
    let mut transport = match Uds::connect(&socket.0, &gate, limits()) {
        Ok(transport) => transport,
        Err(error) => panic!("connection failed: {error:?}"),
    };
    assert!(matches!(
        submit_signed(&mut transport, &registry(), context(public_key, 1), &signed),
        Err(SubmitError::Signature(_))
    ));
    drop(transport);
    let Ok(observed) = server.join() else {
        panic!("signature server panicked");
    };
    assert!(observed.is_empty());
}
