use std::collections::BTreeMap;

use layerx_client::evidence::{EvidenceError, FinalityEvidenceCandidate};

const FINALITY_VECTOR: &str = include_str!("../../../../tests/vectors/finality_evidence_v1.vec");

fn fields() -> BTreeMap<&'static str, &'static str> {
    FINALITY_VECTOR
        .lines()
        .filter_map(|line| line.split_once('='))
        .collect()
}

fn hex(encoded: &str) -> Vec<u8> {
    assert_eq!(encoded.len() & 1, 0, "odd-length finality vector");
    encoded
        .as_bytes()
        .chunks_exact(2)
        .map(|pair| {
            let high = (pair[0] as char)
                .to_digit(16)
                .unwrap_or_else(|| panic!("invalid finality vector hex"));
            let low = (pair[1] as char)
                .to_digit(16)
                .unwrap_or_else(|| panic!("invalid finality vector hex"));
            u8::try_from((high << 4) | low)
                .unwrap_or_else(|_| panic!("finality vector byte overflow"))
        })
        .collect()
}

fn vector() -> (u16, u32, Vec<u8>, Vec<u8>) {
    let values = fields();
    assert_eq!(values.get("version"), Some(&"1"));
    let protocol = values
        .get("protocol_version")
        .unwrap_or_else(|| panic!("missing protocol version"))
        .parse()
        .unwrap_or_else(|error| panic!("invalid protocol version: {error}"));
    let network = values
        .get("network_id")
        .unwrap_or_else(|| panic!("missing network id"))
        .parse()
        .unwrap_or_else(|error| panic!("invalid network id: {error}"));
    let checkpoint = hex(values
        .get("checkpoint_payload")
        .unwrap_or_else(|| panic!("missing checkpoint payload")));
    let context = hex(values
        .get("finality_proof")
        .unwrap_or_else(|| panic!("missing finality proof")));
    (protocol, network, checkpoint, context)
}

#[test]
fn accepts_the_exact_c_typed_cp1_cx1_fixture() {
    let (protocol, network, checkpoint, context) = vector();
    FinalityEvidenceCandidate::from_exact_bytes(checkpoint, context, protocol, network)
        .unwrap_or_else(|error| panic!("C CP1/CX1 vector refused: {error:?}"));
}

#[test]
fn rejects_corrupt_or_cross_record_finality_bytes() {
    let (protocol, network, checkpoint, context) = vector();

    let mut malformed_checkpoint = checkpoint.clone();
    malformed_checkpoint[0] ^= 1;
    assert!(matches!(
        FinalityEvidenceCandidate::from_exact_bytes(
            malformed_checkpoint,
            context.clone(),
            protocol,
            network,
        ),
        Err(EvidenceError::Malformed)
    ));

    let mut malformed_context = context.clone();
    malformed_context[0] ^= 1;
    assert!(matches!(
        FinalityEvidenceCandidate::from_exact_bytes(
            checkpoint.clone(),
            malformed_context,
            protocol,
            network,
        ),
        Err(EvidenceError::Malformed)
    ));

    let mut mismatched_checkpoint = checkpoint.clone();
    *mismatched_checkpoint
        .last_mut()
        .unwrap_or_else(|| panic!("empty checkpoint fixture")) ^= 1;
    assert!(matches!(
        FinalityEvidenceCandidate::from_exact_bytes(
            mismatched_checkpoint,
            context.clone(),
            protocol,
            network,
        ),
        Err(EvidenceError::Registration)
    ));

    let mut mismatched_context = context.clone();
    *mismatched_context
        .last_mut()
        .unwrap_or_else(|| panic!("empty context fixture")) ^= 1;
    assert!(matches!(
        FinalityEvidenceCandidate::from_exact_bytes(
            checkpoint.clone(),
            mismatched_context,
            protocol,
            network,
        ),
        Err(EvidenceError::Registration)
    ));

    assert!(matches!(
        FinalityEvidenceCandidate::from_exact_bytes(
            checkpoint.clone(),
            context.clone(),
            protocol,
            network + 1,
        ),
        Err(EvidenceError::NetworkMismatch)
    ));
    assert!(matches!(
        FinalityEvidenceCandidate::from_exact_bytes(checkpoint, context, protocol + 1, network,),
        Err(EvidenceError::NetworkMismatch)
    ));
}
