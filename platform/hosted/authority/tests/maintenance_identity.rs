use ed25519_dalek::{Signer as _, SigningKey};
use layerx_platform_authority::{
    authorized_batch_by_activity, hex, parse_replica_evidence, BatchEvidence,
    BatchIdentityEvidence, EvidenceRefusal,
};
use layerx_proof::inclusion::{InclusionError, SequencerAuthorization};
use layerx_proof::merkle::{build_proof, encode_proof, Proof};
use layerx_wire::hash::{batch_header_digest, program_execution_batch_id, receipt_digest};
use layerx_wire::receipt::{decode, decode_batch_header, encode_unsigned};
use std::fmt::Debug;

#[path = "../../../../agent/crates/layerx-wire/tests/support/maintenance.rs"]
mod maintenance;

fn must<T, E: Debug>(value: Result<T, E>) -> T {
    value.unwrap_or_else(|error| panic!("{error:?}"))
}

fn fixture() -> (Vec<u8>, BatchEvidence, SequencerAuthorization) {
    let document: serde_json::Value = must(serde_json::from_str(include_str!(
        "fixtures/real-program-deploy-receipt.json"
    )));
    let field = |name: &str| {
        must(hex::decode(
            document[name].as_str().unwrap_or_else(|| panic!("field")),
        ))
    };
    let mut bytes = field("receipt_hex");
    let mut header_bytes = field("header_hex");
    let original = must(decode_batch_header(&header_bytes));
    header_bytes[41..49].copy_from_slice(&(original.last_sequence() + 1).to_be_bytes());
    let header = must(decode_batch_header(&header_bytes));
    let id = must(program_execution_batch_id(
        header.previous_state_root(),
        header.activity_merkle_root(),
        header.first_sequence(),
        header.last_sequence() - 1,
        header.batch_number(),
    ));
    let old = must(decode(&bytes))
        .protocol()
        .unwrap_or_else(|| panic!("protocol"))
        .batch_id();
    let offset = bytes
        .windows(32)
        .position(|value| value == old)
        .unwrap_or_else(|| panic!("batch"));
    bytes[offset..offset + 32].copy_from_slice(&id);
    let key = SigningKey::from_bytes(&[41; 32]);
    let unsigned = must(encode_unsigned(&must(decode(&bytes))));
    let signature = key.sign(&must(receipt_digest(&unsigned))).to_bytes();
    let length = bytes.len();
    bytes[length - 64..].copy_from_slice(&signature);
    let maintenance = maintenance::maintenance_bytes(&header);
    let leaves = [&bytes[..], &maintenance[..]];
    let (receipt_proof, root) = must(build_proof(&leaves, 0));
    let (maintenance_proof, _) = must(build_proof(&leaves, 1));
    let root_offset = header_bytes
        .windows(32)
        .position(|value| value == header.receipt_merkle_root())
        .unwrap_or_else(|| panic!("root"));
    header_bytes[root_offset..root_offset + 32].copy_from_slice(&root);
    let header_signature = key
        .sign(&must(batch_header_digest(&header_bytes)))
        .to_bytes();
    let evidence = BatchEvidence {
        header: header_bytes,
        header_signature,
        receipt_proof: encode_proof(&receipt_proof),
        batch_identity: BatchIdentityEvidence::OccupancyMaintenanceV2 {
            receipt: maintenance,
            proof: encode_proof(&maintenance_proof),
            activity_receipts: Vec::new(),
        },
    };
    let authorization = SequencerAuthorization::new(
        header.sequencer_id(),
        key.verifying_key().to_bytes(),
        1,
        u64::MAX,
    );
    (bytes, evidence, authorization)
}

fn verify(
    bytes: &[u8],
    evidence: &BatchEvidence,
    authorization: &SequencerAuthorization,
) -> Result<layerx_platform_authority::AuthorityFacts, EvidenceRefusal> {
    let receipt = must(decode(bytes));
    authorized_batch_by_activity(
        receipt
            .protocol()
            .unwrap_or_else(|| panic!("protocol"))
            .activity_id(),
        bytes,
        evidence,
        authorization,
    )
}

#[test]
fn maintained_batch_requires_signed_maintenance_and_explicit_selection() {
    let (bytes, evidence, authorization) = fixture();
    let facts = must(verify(&bytes, &evidence, &authorization));
    assert_eq!(
        facts.batch_id,
        must(decode(&bytes))
            .protocol()
            .unwrap_or_else(|| panic!("protocol"))
            .batch_id()
    );
    let mut changed = evidence.clone();
    changed.batch_identity = BatchIdentityEvidence::Historical;
    assert_eq!(
        verify(&bytes, &changed, &authorization),
        Err(EvidenceRefusal::BatchIdentity)
    );
    let mut changed = evidence.clone();
    changed.header_signature[0] ^= 1;
    assert_eq!(
        verify(&bytes, &changed, &authorization),
        Err(EvidenceRefusal::Inclusion(InclusionError::HeaderSignature))
    );
    let mut changed = evidence.clone();
    if let BatchIdentityEvidence::OccupancyMaintenanceV2 { receipt, .. } =
        &mut changed.batch_identity
    {
        receipt[45] ^= 1;
    }
    assert!(matches!(
        verify(&bytes, &changed, &authorization),
        Err(EvidenceRefusal::Inclusion(InclusionError::Merkle(_)))
    ));
    let mut changed = evidence.clone();
    if let BatchIdentityEvidence::OccupancyMaintenanceV2 { proof, .. } = &mut changed.batch_identity
    {
        *proof = encode_proof(&must(Proof::new(0, 1, vec![])));
    }
    assert!(verify(&bytes, &changed, &authorization).is_err());
}

fn native_proof(proof_bytes: &[u8]) -> Vec<u8> {
    let proof = must(layerx_proof::merkle::decode_proof(proof_bytes));
    let mut encoder = layerx_wire::encode::Encoder::new(2048);
    must(encoder.structure_header(0x4d50));
    must(encoder.u32(proof.leaf_index()));
    must(encoder.u32(proof.leaf_count()));
    must(encoder.u8(must(u8::try_from(proof.siblings().len()))));
    must(encoder.bytes(&proof.siblings().concat(), 1024));
    encoder.finish()
}

#[test]
fn replica_maintenance_document_is_explicit_and_closed() {
    let (bytes, evidence, authorization) = fixture();
    let BatchIdentityEvidence::OccupancyMaintenanceV2 { receipt, proof, .. } =
        &evidence.batch_identity
    else {
        panic!("maintenance")
    };
    let mut document = serde_json::json!({ "authority_replica_id": hex::encode(&[7;32]), "sequencer_public_key": hex::encode(&authorization.public_key()), "batch_evidence": { "header_hex": hex::encode(&evidence.header), "header_signature": hex::encode(&evidence.header_signature), "receipt_proof_hex": hex::encode(&native_proof(&evidence.receipt_proof)), "batch_identity": { "kind": "occupancy_maintenance_v2", "receipt_hex": hex::encode(receipt), "receipt_proof_hex": hex::encode(&native_proof(proof)) } } });
    let parsed = must(parse_replica_evidence(
        &must(serde_json::to_vec(&document)),
        [7; 32],
        authorization.public_key(),
    ));
    assert_eq!(parsed, evidence);
    assert!(verify(&bytes, &parsed, &authorization).is_ok());
    for value in [
        serde_json::Value::Null,
        serde_json::json!({"kind":"unknown"}),
        serde_json::json!({"kind":"occupancy_maintenance_v2"}),
        serde_json::json!({"kind":"historical", "activity_count":1}),
    ] {
        document["batch_evidence"]["batch_identity"] = value;
        assert!(parse_replica_evidence(
            &must(serde_json::to_vec(&document)),
            [7; 32],
            authorization.public_key()
        )
        .is_err());
    }
}

fn reseal(bytes: &[u8], evidence: &mut BatchEvidence) {
    let BatchIdentityEvidence::OccupancyMaintenanceV2 { receipt, proof, .. } =
        &mut evidence.batch_identity
    else {
        panic!("maintenance")
    };
    let leaves = [bytes, receipt.as_slice()];
    let (activity_proof, root) = must(build_proof(&leaves, 0));
    *proof = encode_proof(&must(build_proof(&leaves, 1)).0);
    evidence.receipt_proof = encode_proof(&activity_proof);
    let header = must(decode_batch_header(&evidence.header));
    let offset = evidence
        .header
        .windows(32)
        .position(|value| value == header.receipt_merkle_root())
        .unwrap_or_else(|| panic!("root"));
    evidence.header[offset..offset + 32].copy_from_slice(&root);
    evidence.header_signature = SigningKey::from_bytes(&[41; 32])
        .sign(&must(batch_header_digest(&evidence.header)))
        .to_bytes();
}

#[test]
fn signed_maintenance_sequence_and_root_mismatches_are_refused() {
    let (bytes, evidence, authorization) = fixture();
    let mut changed = evidence.clone();
    if let BatchIdentityEvidence::OccupancyMaintenanceV2 { receipt, .. } =
        &mut changed.batch_identity
    {
        let sequence_offset = b"LXP/programs/occupancy-receipt/v2\0".len() + 8;
        receipt[sequence_offset + 7] ^= 1;
    }
    reseal(&bytes, &mut changed);
    assert_eq!(
        verify(&bytes, &changed, &authorization),
        Err(EvidenceRefusal::BatchIdentity)
    );
    let mut changed = evidence.clone();
    if let BatchIdentityEvidence::OccupancyMaintenanceV2 { receipt, .. } =
        &mut changed.batch_identity
    {
        let end = receipt.len();
        receipt[end - 1] ^= 1;
    }
    reseal(&bytes, &mut changed);
    assert_eq!(
        verify(&bytes, &changed, &authorization),
        Err(EvidenceRefusal::BatchIdentity)
    );
    let mut changed = evidence;
    changed.header[48] ^= 1;
    reseal(&bytes, &mut changed);
    assert_eq!(
        verify(&bytes, &changed, &authorization),
        Err(EvidenceRefusal::SequenceRange)
    );
}

#[test]
fn maintained_previous_root_and_cross_batch_leaf_are_refused() {
    let (bytes, evidence, authorization) = fixture();
    for previous in [true, false] {
        let mut changed = evidence.clone();
        if let BatchIdentityEvidence::OccupancyMaintenanceV2 { receipt, .. } =
            &mut changed.batch_identity
        {
            let offset = if previous {
                receipt.len() - 64
            } else {
                b"LXP/programs/occupancy-receipt/v2\0".len() + 7
            };
            receipt[offset] ^= 1;
        }
        assert!(matches!(
            verify(&bytes, &changed, &authorization),
            Err(EvidenceRefusal::Inclusion(InclusionError::Merkle(_)))
        ));
        reseal(&bytes, &mut changed);
        assert_eq!(
            verify(&bytes, &changed, &authorization),
            Err(if previous {
                EvidenceRefusal::Receipt(layerx_proof::receipt::ReceiptCheck::ResultingStateRoot)
            } else {
                EvidenceRefusal::BatchIdentity
            })
        );
    }
}

#[test]
fn historical_document_cannot_select_maintained_outcome() {
    let document: serde_json::Value = must(serde_json::from_str(include_str!(
        "fixtures/real-program-deploy-receipt.json"
    )));
    let field = |name: &str| {
        must(hex::decode(
            document[name].as_str().unwrap_or_else(|| panic!("field")),
        ))
    };
    let (_, maintained, _) = fixture();
    let historical = field("receipt_hex");
    let mut evidence = BatchEvidence {
        header: field("header_hex"),
        header_signature: must(field("header_signature_hex").try_into()),
        receipt_proof: encode_proof(&must(Proof::new(0, 1, vec![]))),
        batch_identity: BatchIdentityEvidence::Historical,
    };
    let header = must(decode_batch_header(&evidence.header));
    let authorization = SequencerAuthorization::new(
        header.sequencer_id(),
        must(field("sequencer_public_key_hex").try_into()),
        1,
        u64::MAX,
    );
    let facts = must(verify(&historical, &evidence, &authorization));
    let decoded = must(decode(&historical));
    let protocol = decoded.protocol().unwrap_or_else(|| panic!("protocol"));
    let header = must(decode_batch_header(&evidence.header));
    assert_eq!(
        facts.batch_id,
        must(layerx_wire::hash::execution_batch_id(
            header.previous_state_root(),
            protocol.activity_id(),
            protocol.global_sequence(),
            header.batch_number(),
        ))
    );
    evidence.batch_identity = maintained.batch_identity;
    assert!(matches!(
        verify(&historical, &evidence, &authorization),
        Err(EvidenceRefusal::Inclusion(InclusionError::Merkle(_)))
    ));
}
