use layerx_intents::vectors::{security_batch_header, security_program_receipt};

use base64::{engine::general_purpose::STANDARD, Engine as _};
use ed25519_dalek::{Signer as _, SigningKey};
use layerx_human_security_provider::RecoveryReceipt;
use layerx_intents::canonical::{batch_header_digest, execution_batch_id, receipt_digest};
use layerx_proof::merkle::{build_proof, encode_proof};

pub fn evidence() -> (Vec<u8>, RecoveryReceipt) {
    let key = SigningKey::from_bytes(&[37; 32]);
    let public = key.verifying_key().to_bytes();
    let activity = [42; 32];
    let batch = execution_batch_id([0x11; 32], activity, 1, 1).unwrap();
    let timestamp = 1_700_000_000_000;
    let unsigned =
        security_program_receipt(activity, batch, 1, timestamp, [23; 32], [24; 32], None);
    let signature = key.sign(&receipt_digest(&unsigned).unwrap()).to_bytes();
    let receipt = security_program_receipt(
        activity,
        batch,
        1,
        timestamp,
        [23; 32],
        [24; 32],
        Some(signature),
    );
    let (proof, root) = build_proof(&[&receipt], 0).unwrap();
    let header = security_batch_header(1, timestamp, [23; 32], [24; 32], root, public, 2);
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
