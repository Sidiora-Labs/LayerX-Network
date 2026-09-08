use layerx_wire::encode::Encoder;
fn encode_program_receipt(
    activity_id: [u8; 32],
    batch_id: [u8; 32],
    batch_number: u64,
    timestamp: u64,
    resulting_state_root: [u8; 32],
    activity_root: [u8; 32],
    signature: Option<[u8; 64]>,
) -> Vec<u8> {
    let mut encoder = Encoder::new(4_096);
    assert_eq!(encoder.structure_header_version(0x5201, 2), Ok(()));
    assert_eq!(encoder.u16(2), Ok(()));
    assert_eq!(encoder.bytes(&activity_id, 32), Ok(()));
    assert_eq!(encoder.u64(batch_number), Ok(()));
    assert_eq!(encoder.bytes(&[0x11; 32], 32), Ok(()));
    assert_eq!(encoder.bytes(&resulting_state_root, 32), Ok(()));
    assert_eq!(encoder.bytes(&activity_root, 32), Ok(()));
    assert_eq!(encoder.i32(0), Ok(()));
    assert_eq!(encoder.sequence_length(0, 512), Ok(()));
    assert_eq!(encoder.u128(0), Ok(()));
    assert_eq!(encoder.bytes(&batch_id, 32), Ok(()));
    assert_eq!(encoder.u16(9), Ok(()));
    assert_eq!(encoder.u32(2), Ok(()));
    assert_eq!(encoder.u32(0), Ok(()));
    assert_eq!(encoder.u8(0), Ok(()));
    assert_eq!(encoder.bytes(&[0; 32], 32), Ok(()));
    assert_eq!(encoder.u128(0), Ok(()));
    assert_eq!(encoder.bytes(&[0; 32], 32), Ok(()));
    assert_eq!(encoder.u128(0), Ok(()));
    assert_eq!(encoder.u128(0), Ok(()));
    assert_eq!(encoder.u64(0), Ok(()));
    assert_eq!(encoder.bytes(&[0; 32], 32), Ok(()));
    assert_eq!(encoder.u128(0), Ok(()));
    assert_eq!(encoder.u128(0), Ok(()));
    assert_eq!(encoder.bytes(&[0; 32], 32), Ok(()));
    assert_eq!(encoder.bytes(&[0x13; 32], 32), Ok(()));
    assert_eq!(encoder.bytes(&[0x14; 32], 32), Ok(()));
    assert_eq!(encoder.u64(timestamp), Ok(()));
    assert_eq!(encoder.u8(u8::from(signature.is_some())), Ok(()));
    if let Some(signature) = signature {
        assert_eq!(encoder.bytes(&signature, 64), Ok(()));
    }
    encoder.finish()
}

fn encode_header(
    batch_number: u64,
    timestamp: u64,
    resulting_state_root: [u8; 32],
    activity_root: [u8; 32],
    receipt_root: [u8; 32],
    sequencer_id: [u8; 32],
    epoch: u64,
) -> Vec<u8> {
    let mut encoder = Encoder::new(354);
    assert_eq!(encoder.structure_header_version(0x1701, 2), Ok(()));
    assert_eq!(encoder.u8(15), Ok(()));
    assert_eq!(encoder.tag(1, 15), Ok(()));
    assert_eq!(encoder.u16(2), Ok(()));
    assert_eq!(encoder.tag(2, 15), Ok(()));
    assert_eq!(encoder.u32(42), Ok(()));
    assert_eq!(encoder.tag(3, 15), Ok(()));
    assert_eq!(encoder.u64(epoch), Ok(()));
    assert_eq!(encoder.tag(4, 15), Ok(()));
    assert_eq!(encoder.u64(batch_number), Ok(()));
    assert_eq!(encoder.tag(5, 15), Ok(()));
    assert_eq!(encoder.u64(batch_number), Ok(()));
    assert_eq!(encoder.tag(6, 15), Ok(()));
    assert_eq!(encoder.u64(batch_number), Ok(()));
    assert_eq!(encoder.tag(7, 15), Ok(()));
    assert_eq!(encoder.bytes(&[0x11; 32], 32), Ok(()));
    assert_eq!(encoder.tag(8, 15), Ok(()));
    assert_eq!(encoder.bytes(&resulting_state_root, 32), Ok(()));
    assert_eq!(encoder.tag(9, 15), Ok(()));
    assert_eq!(encoder.bytes(&activity_root, 32), Ok(()));
    assert_eq!(encoder.tag(10, 15), Ok(()));
    assert_eq!(encoder.bytes(&receipt_root, 32), Ok(()));
    assert_eq!(encoder.tag(11, 15), Ok(()));
    assert_eq!(encoder.bytes(&[0x15; 32], 32), Ok(()));
    assert_eq!(encoder.tag(12, 15), Ok(()));
    assert_eq!(encoder.bytes(&[0x16; 32], 32), Ok(()));
    assert_eq!(encoder.tag(13, 15), Ok(()));
    assert_eq!(encoder.bytes(&[0x17; 32], 32), Ok(()));
    assert_eq!(encoder.tag(14, 15), Ok(()));
    assert_eq!(encoder.u64(timestamp), Ok(()));
    assert_eq!(encoder.tag(15, 15), Ok(()));
    assert_eq!(encoder.bytes(&sequencer_id, 32), Ok(()));
    encoder.finish()
}
use base64::{engine::general_purpose::STANDARD, Engine as _};
use ed25519_dalek::{Signer as _, SigningKey};
use layerx_human_security_provider::RecoveryReceipt;
use layerx_proof::merkle::{build_proof, encode_proof};
use layerx_wire::hash::{batch_header_digest, execution_batch_id, receipt_digest};

pub fn evidence() -> (Vec<u8>, RecoveryReceipt) {
    let key = SigningKey::from_bytes(&[37; 32]);
    let public = key.verifying_key().to_bytes();
    let activity = [42; 32];
    let batch = execution_batch_id([0x11; 32], activity, 1, 1).unwrap();
    let timestamp = 1_700_000_000_000;
    let unsigned = encode_program_receipt(activity, batch, 1, timestamp, [23; 32], [24; 32], None);
    let signature = key.sign(&receipt_digest(&unsigned).unwrap()).to_bytes();
    let receipt = encode_program_receipt(
        activity,
        batch,
        1,
        timestamp,
        [23; 32],
        [24; 32],
        Some(signature),
    );
    let (proof, root) = build_proof(&[&receipt], 0).unwrap();
    let header = encode_header(1, timestamp, [23; 32], [24; 32], root, public, 2);
    let header_signature = key.sign(&batch_header_digest(&header).unwrap()).to_bytes();
    let mut history = b"LayerX/sequencer-trust-history/v1\0".to_vec();
    history.extend_from_slice(&1u16.to_be_bytes());
    history.extend_from_slice(&0u16.to_be_bytes());
    history.extend_from_slice(&2u16.to_be_bytes());
    history.extend_from_slice(&42u32.to_be_bytes());
    history.extend_from_slice(&2u64.to_be_bytes());
    history.extend_from_slice(&public);
    history.extend_from_slice(&public);
    history.extend_from_slice(&1u64.to_be_bytes());
    history.extend_from_slice(&100u64.to_be_bytes());
    history.push(0);
    history.extend_from_slice(&0u64.to_be_bytes());
    (
        history,
        RecoveryReceipt {
            version: 1,
            principal: "alice".into(),
            evidence_id: "recovery-1".into(),
            canonical_receipt: STANDARD.encode(receipt),
            receipt_proof: STANDARD.encode(encode_proof(&proof)),
            header: STANDARD.encode(header),
            header_signature: STANDARD.encode(header_signature),
        },
    )
}
