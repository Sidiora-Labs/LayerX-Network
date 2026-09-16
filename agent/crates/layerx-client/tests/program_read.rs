use ed25519_dalek::{Signer as _, SigningKey};
use layerx_client::lni::program_read::{
    read_program, ProgramReadContext, ProgramReadError, PROGRAM_READ_REQUEST_TAG,
    PROGRAM_READ_RESPONSE_TAG,
};
use layerx_client::lni::schema::{decode_envelope, encode_envelope, Envelope, Version};
use layerx_client::lni::simulate::{
    encode_simulation_evidence, encode_simulation_payload, simulation_boundary_id,
    simulation_evidence_digest, SimulatedExecution, SimulationEvidence,
};
use layerx_client::lni::transport::{FrameTransport, TransportError};
use layerx_types::payload::{ActivityType, ModuleId, ModuleRegistration, ModuleRegistry};
use layerx_types::result::KnownResult;
use layerx_wire::hash::receipt_digest;
use layerx_wire::receipt::{decode, encode_unsigned};

const DOCUMENT: &str =
    include_str!("../../../../platform/sdk/conformance/fixtures/receipt-programs-executed-v4.json");

fn bytes(field: &str) -> Vec<u8> {
    let marker = format!("\"{field}\": \"");
    let (_, rest) = DOCUMENT
        .split_once(&marker)
        .unwrap_or_else(|| panic!("missing fixture field {field}"));
    let value = rest
        .split('"')
        .next()
        .unwrap_or_else(|| panic!("unterminated fixture field {field}"));
    value
        .as_bytes()
        .chunks_exact(2)
        .map(|pair| {
            let text = std::str::from_utf8(pair)
                .unwrap_or_else(|error| panic!("fixture field is not UTF-8: {error}"));
            u8::from_str_radix(text, 16)
                .unwrap_or_else(|error| panic!("fixture field is not hex: {error}"))
        })
        .collect()
}

fn registry() -> ModuleRegistry {
    let call = ActivityType::new(ModuleId::Programs, 3)
        .unwrap_or_else(|error| panic!("ProgramCall type failed: {error:?}"));
    let registration = ModuleRegistration::new(ModuleId::Programs, &[call])
        .unwrap_or_else(|error| panic!("Programs registration failed: {error:?}"));
    ModuleRegistry::new(&[registration])
        .unwrap_or_else(|error| panic!("registry failed: {error:?}"))
}

struct Scripted {
    response: Option<Vec<u8>>,
    sent: Vec<Vec<u8>>,
    receives: u8,
}

impl Scripted {
    fn new(response: Vec<u8>) -> Self {
        Self {
            response: Some(response),
            sent: Vec::new(),
            receives: 0,
        }
    }
}

impl FrameTransport for Scripted {
    fn send(&mut self, canonical_envelope: &[u8]) -> Result<(), TransportError> {
        self.sent.push(canonical_envelope.to_vec());
        Ok(())
    }

    fn receive(&mut self) -> Result<Vec<u8>, TransportError> {
        self.receives = self.receives.saturating_add(1);
        self.response.take().ok_or(TransportError::PeerShutdown)
    }
}

fn signed_execution() -> (Vec<u8>, SimulatedExecution, SimulationEvidence) {
    let signed_activity = bytes("signed_activity_hex");
    let receipt = decode(&bytes("canonical_receipt_hex"))
        .unwrap_or_else(|error| panic!("fixture receipt failed: {error:?}"));
    let protocol = receipt
        .protocol()
        .unwrap_or_else(|| panic!("fixture is not a protocol receipt"));
    let signer = SigningKey::from_bytes(&[0x31; 32]);
    let public_key = signer.verifying_key().to_bytes();
    let unsigned =
        encode_unsigned(&receipt).unwrap_or_else(|error| panic!("unsigned receipt: {error:?}"));
    let digest =
        receipt_digest(&unsigned).unwrap_or_else(|error| panic!("receipt digest: {error:?}"));
    let mut canonical_receipt = unsigned;
    assert_eq!(canonical_receipt.pop(), Some(0));
    canonical_receipt.push(1);
    canonical_receipt.extend_from_slice(&64_u32.to_be_bytes());
    canonical_receipt.extend_from_slice(&signer.sign(&digest).to_bytes());
    let execution = SimulatedExecution {
        activity_id: protocol.activity_id(),
        receipt: canonical_receipt,
        terminal_payload: bytes("terminal_payload_hex"),
        call_graph: bytes("call_graph_hex"),
    };
    let mut evidence = SimulationEvidence {
        boundary_id: simulation_boundary_id(&public_key),
        activity_id: protocol.activity_id(),
        previous_state_root: protocol.previous_state_root(),
        hypothetical_state_root: protocol.resulting_state_root(),
        observed_sequence: protocol.global_sequence(),
        observed_at: protocol.timestamp(),
        public_key,
        signature: [0; 64],
    };
    evidence.signature = signer
        .sign(&simulation_evidence_digest(&evidence))
        .to_bytes();
    (signed_activity, execution, evidence)
}

fn response(
    execution: &SimulatedExecution,
    evidence: &SimulationEvidence,
    correlation_id: u64,
) -> Vec<u8> {
    let payload = encode_simulation_payload(execution)
        .unwrap_or_else(|error| panic!("simulation payload failed: {error:?}"));
    let proof = encode_simulation_evidence(evidence);
    encode_envelope(Envelope {
        version: Version::V1_6,
        message_tag: PROGRAM_READ_RESPONSE_TAG,
        correlation_id,
        canonical_payload: &payload,
        proof_material: &proof,
    })
    .unwrap_or_else(|error| panic!("program-read response failed: {error:?}"))
}

#[test]
fn program_read_sends_one_exact_snapshot_bound_activity_and_verifies_evidence() {
    let (signed_activity, execution, evidence) = signed_execution();
    let correlation_id = 71;
    let minimum_sequence = evidence.observed_sequence;
    let expected_root = evidence.previous_state_root;
    let mut transport = Scripted::new(response(&execution, &evidence, correlation_id));
    let result = read_program(
        &mut transport,
        &registry(),
        &signed_activity,
        ProgramReadContext {
            interface_version: Version::V1_6,
            sequencer_public_key: evidence.public_key,
            correlation_id,
            minimum_sequence,
            expected_state_root: Some(expected_root),
        },
    )
    .unwrap_or_else(|error| panic!("program read failed: {error:?}"));
    assert_eq!(result.execution, execution);
    assert_eq!(result.evidence, evidence);
    assert_eq!(result.snapshot.minimum_sequence, minimum_sequence);
    assert_eq!(
        result.snapshot.observed_sequence,
        evidence.observed_sequence
    );
    assert_eq!(result.snapshot.state_root, expected_root);
    assert_eq!(transport.sent.len(), 1);
    assert_eq!(transport.receives, 1);

    let request = decode_envelope(&transport.sent[0])
        .unwrap_or_else(|error| panic!("request envelope failed: {error:?}"));
    assert_eq!(request.message_tag, PROGRAM_READ_REQUEST_TAG);
    assert!(request.proof_material.is_empty());
    assert_eq!(&request.canonical_payload[..2], &1_u16.to_be_bytes());
    assert_eq!(
        &request.canonical_payload[2..10],
        &minimum_sequence.to_be_bytes()
    );
    assert_eq!(request.canonical_payload[10], 1);
    assert_eq!(&request.canonical_payload[11..43], &expected_root);
    assert_eq!(
        &request.canonical_payload[43..47],
        &u32::try_from(signed_activity.len())
            .unwrap_or_default()
            .to_be_bytes()
    );
    assert_eq!(&request.canonical_payload[47..], signed_activity);
}

#[test]
fn program_read_preserves_typed_snapshot_refusals_without_retrying() {
    let (signed_activity, _, evidence) = signed_execution();
    for (raw, expected) in [
        (
            KnownResult::ProjectionStale.raw(),
            ProgramReadError::SnapshotStale,
        ),
        (
            KnownResult::ContextMismatch.raw(),
            ProgramReadError::SnapshotMismatch,
        ),
    ] {
        let correlation_id = u64::from(raw.unsigned_abs());
        let mut refusal = vec![7];
        refusal.extend_from_slice(&raw.to_be_bytes());
        let response = encode_envelope(Envelope {
            version: Version::V1_6,
            message_tag: 25,
            correlation_id,
            canonical_payload: &refusal,
            proof_material: &[],
        })
        .unwrap_or_else(|error| panic!("refusal envelope failed: {error:?}"));
        let mut transport = Scripted::new(response);
        assert_eq!(
            read_program(
                &mut transport,
                &registry(),
                &signed_activity,
                ProgramReadContext {
                    interface_version: Version::V1_6,
                    sequencer_public_key: evidence.public_key,
                    correlation_id,
                    minimum_sequence: 0,
                    expected_state_root: None,
                },
            ),
            Err(expected)
        );
        assert_eq!(transport.sent.len(), 1);
        assert_eq!(transport.receives, 1);
    }
}
