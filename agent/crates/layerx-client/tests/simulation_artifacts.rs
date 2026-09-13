use ed25519_dalek::{Signer as _, SigningKey};
use layerx_client::lni::simulate::{
    simulation_boundary_id, simulation_evidence_digest, verify_simulation, SimulateError,
    SimulatedExecution, SimulationEvidence,
};
use layerx_wire::hash::receipt_digest;
use layerx_wire::receipt::{decode, encode_unsigned};

const DOCUMENT: &str =
    include_str!("../../../../platform/sdk/conformance/fixtures/receipt-programs-executed-v4.json");

fn bytes(field: &str) -> Vec<u8> {
    let marker = format!("\"{field}\": \"");
    let (_, rest) = DOCUMENT
        .split_once(&marker)
        .unwrap_or_else(|| panic!("{field}"));
    let value = rest.split('"').next().unwrap_or_else(|| panic!("{field}"));
    value
        .as_bytes()
        .chunks_exact(2)
        .map(|pair| {
            u8::from_str_radix(
                std::str::from_utf8(pair).unwrap_or_else(|error| panic!("{error}")),
                16,
            )
            .unwrap_or_else(|error| panic!("{error}"))
        })
        .collect()
}

#[test]
fn simulation_requires_every_committed_artifact_even_when_the_field_is_empty() {
    let receipt =
        decode(&bytes("canonical_receipt_hex")).unwrap_or_else(|error| panic!("{error:?}"));
    let protocol = receipt
        .protocol()
        .unwrap_or_else(|| panic!("protocol receipt"));
    let signer = SigningKey::from_bytes(&[0x31; 32]);
    let public = signer.verifying_key().to_bytes();
    let unsigned = encode_unsigned(&receipt).unwrap_or_else(|error| panic!("{error:?}"));
    let digest = receipt_digest(&unsigned).unwrap_or_else(|error| panic!("{error:?}"));
    let mut canonical = unsigned;
    assert_eq!(canonical.pop(), Some(0));
    canonical.push(1);
    canonical.extend_from_slice(&64_u32.to_be_bytes());
    canonical.extend_from_slice(&signer.sign(&digest).to_bytes());
    let execution = SimulatedExecution {
        activity_id: protocol.activity_id(),
        receipt: canonical,
        terminal_payload: bytes("terminal_payload_hex"),
        call_graph: bytes("call_graph_hex"),
    };
    let mut evidence = SimulationEvidence {
        boundary_id: simulation_boundary_id(&public),
        activity_id: protocol.activity_id(),
        previous_state_root: protocol.previous_state_root(),
        hypothetical_state_root: protocol.resulting_state_root(),
        observed_sequence: protocol.global_sequence(),
        observed_at: protocol.timestamp(),
        public_key: public,
        signature: [0; 64],
    };
    evidence.signature = signer
        .sign(&simulation_evidence_digest(&evidence))
        .to_bytes();
    assert!(verify_simulation(execution.clone(), evidence, protocol.activity_id(), public).is_ok());
    let mut missing = execution.clone();
    missing.terminal_payload.clear();
    assert_eq!(
        verify_simulation(missing, evidence, protocol.activity_id(), public),
        Err(SimulateError::ArtifactMismatch)
    );
    let mut missing = execution;
    missing.call_graph.clear();
    assert_eq!(
        verify_simulation(missing, evidence, protocol.activity_id(), public),
        Err(SimulateError::ArtifactMismatch)
    );
}
