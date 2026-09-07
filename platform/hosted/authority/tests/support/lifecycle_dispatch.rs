use super::{
    decode, hex, verify_authorized_receipt, AuthorizedBatch, EvidenceRefusal, ReceiptCheck,
};
use layerx_proof::receipt::verify_outcome;
use std::fmt::Debug;

fn must<T, E: Debug>(result: Result<T, E>) -> T {
    result.unwrap_or_else(|error| panic!("fixture verification: {error:?}"))
}

fn state_fixture() -> (Vec<u8>, AuthorizedBatch) {
    let document: serde_json::Value = must(serde_json::from_str(include_str!(
        "../fixtures/real-program-deploy-receipt.json"
    )));
    let bytes = must(hex::decode(
        document["receipt_hex"]
            .as_str()
            .unwrap_or_else(|| panic!("receipt")),
    ));
    let signer = must(hex::decode32(
        document["sequencer_public_key_hex"]
            .as_str()
            .unwrap_or_else(|| panic!("signer")),
    ));
    let receipt = must(decode(&bytes));
    let protocol = receipt.protocol().unwrap_or_else(|| panic!("protocol"));
    let field = |name: &str| {
        document[name]
            .as_str()
            .unwrap_or_else(|| panic!("evidence field"))
    };
    let header = must(hex::decode(field("header_hex")));
    let signature: [u8; 64] = must(must(hex::decode(field("header_signature_hex"))).try_into());
    let authorization = layerx_proof::inclusion::SequencerAuthorization::new(
        must(hex::decode32(field("sequencer_id_hex"))),
        signer,
        1,
        u64::MAX,
    );
    let siblings = document["proof_siblings"]
        .as_array()
        .unwrap_or_else(|| panic!("proof siblings"))
        .iter()
        .map(|value| {
            must(hex::decode32(
                value.as_str().unwrap_or_else(|| panic!("sibling")),
            ))
        })
        .collect();
    let index = must(u32::try_from(
        document["proof_index"]
            .as_u64()
            .unwrap_or_else(|| panic!("index")),
    ));
    let count = must(u32::try_from(
        document["proof_count"]
            .as_u64()
            .unwrap_or_else(|| panic!("count")),
    ));
    let proof = must(layerx_proof::merkle::Proof::new(index, count, siblings));
    let inclusion = must(layerx_proof::inclusion::verify_receipt(
        &bytes,
        &proof,
        &header,
        &signature,
        &authorization,
    ));
    let verified_header = inclusion.header().header();
    assert_eq!(verified_header.network_id(), 7332);
    assert_eq!(verified_header.protocol_version(), 3);
    let batch_id = must(layerx_wire::hash::receipt_execution_batch_id(
        protocol,
        verified_header,
    ));
    let evidence = super::BatchEvidence {
        batch_identity: super::BatchIdentityEvidence::Historical,
        header: header.clone(),
        header_signature: signature,
        receipt_proof: layerx_proof::merkle::encode_proof(&proof),
    };
    let facts = must(super::authorized_batch_by_activity(
        protocol.activity_id(),
        &bytes,
        &evidence,
        &authorization,
    ));
    assert_eq!(facts.batch_id, batch_id);
    verify_historical_documents(&bytes, &evidence, &authorization);
    let mut tampered = evidence;
    tampered.header_signature[0] ^= 1;
    assert_eq!(
        super::authorized_batch_by_activity(
            protocol.activity_id(),
            &bytes,
            &tampered,
            &authorization
        ),
        Err(EvidenceRefusal::Inclusion(
            layerx_proof::inclusion::InclusionError::HeaderSignature
        ))
    );
    let authority = AuthorizedBatch::new(
        batch_id,
        protocol.asset(),
        verified_header.previous_state_root(),
        verified_header.resulting_state_root(),
        signer,
    );
    (bytes, authority)
}

fn verify_historical_documents(
    bytes: &[u8],
    evidence: &super::BatchEvidence,
    authorization: &layerx_proof::inclusion::SequencerAuthorization,
) {
    let receipt = must(decode(bytes));
    let protocol = receipt.protocol().unwrap_or_else(|| panic!("protocol"));
    let verified_header = must(layerx_wire::receipt::decode_batch_header(&evidence.header));
    let batch_id = protocol.batch_id();
    let proof = must(layerx_proof::merkle::decode_proof(&evidence.receipt_proof));
    let signer = authorization.public_key();
    assert_eq!(
        batch_id,
        must(layerx_wire::hash::execution_batch_id(
            verified_header.previous_state_root(),
            protocol.activity_id(),
            protocol.global_sequence(),
            verified_header.batch_number(),
        ))
    );
    let mut encoder = layerx_wire::encode::Encoder::new(2048);
    must(encoder.structure_header(0x4d50));
    must(encoder.u32(proof.leaf_index()));
    must(encoder.u32(proof.leaf_count()));
    must(encoder.u8(must(u8::try_from(proof.siblings().len()))));
    must(encoder.bytes(&proof.siblings().concat(), 1024));
    let mut replica_document = serde_json::json!({
        "authority_replica_id": hex::encode(&[7; 32]),
        "sequencer_public_key": hex::encode(&signer),
        "batch_evidence": {
            "header_hex": hex::encode(&evidence.header),
            "header_signature": hex::encode(&evidence.header_signature),
            "receipt_proof_hex": hex::encode(&encoder.finish()),
        }
    });
    for explicit in [false, true] {
        if explicit {
            replica_document["batch_evidence"]["batch_identity"] =
                serde_json::json!({"kind": "historical"});
        }
        let parsed = must(super::parse_replica_evidence(
            &must(serde_json::to_vec(&replica_document)),
            [7; 32],
            signer,
        ));
        assert_eq!(&parsed, evidence);
        assert_eq!(
            must(super::authorized_batch_by_activity(
                protocol.activity_id(),
                bytes,
                &parsed,
                authorization,
            ))
            .batch_id,
            batch_id
        );
    }
}

fn call_fixture() -> (Vec<u8>, AuthorizedBatch) {
    let document: serde_json::Value = must(serde_json::from_str(include_str!(
        "../../../../sdk/conformance/fixtures/receipt-programs-positive-v3.json"
    )));
    let bytes = must(hex::decode(
        document["canonical_receipt_hex"]
            .as_str()
            .unwrap_or_else(|| panic!("receipt")),
    ));
    let fields = &document["authorized_batch"];
    let field = |name: &str| {
        must(hex::decode32(
            fields[name]
                .as_str()
                .unwrap_or_else(|| panic!("authority field")),
        ))
    };
    let authority = AuthorizedBatch::new(
        field("batch_id_hex"),
        field("asset_hex"),
        field("previous_state_root_hex"),
        field("resulting_state_root_hex"),
        field("sequencer_public_key_hex"),
    );
    (bytes, authority)
}

fn module_offset(bytes: &[u8]) -> usize {
    let receipt = must(decode(bytes));
    let batch = receipt
        .protocol()
        .unwrap_or_else(|| panic!("protocol"))
        .batch_id();
    let positions: Vec<_> = bytes
        .windows(32)
        .enumerate()
        .filter_map(|(index, value)| (value == batch).then_some(index))
        .collect();
    assert_eq!(positions.len(), 1, "unique batch marker in real fixture");
    positions[0] + 32
}

#[test]
fn real_deploy_state_dispatch_preserves_generic_operation_refusal() {
    let (bytes, authority) = state_fixture();
    assert_eq!(verify_authorized_receipt(&bytes, &authority), Ok(()));
    let failure = verify_outcome(&bytes, &authority)
        .err()
        .unwrap_or_else(|| panic!("generic operation zero accepted"));
    assert_eq!(failure.check, ReceiptCheck::Operation);
    let wrong_roots = AuthorizedBatch::new(
        authority.batch_id(),
        authority.asset(),
        [0; 32],
        authority.resulting_state_root(),
        authority.sequencer_public_key(),
    );
    assert_eq!(
        verify_authorized_receipt(&bytes, &wrong_roots),
        Err(EvidenceRefusal::Receipt(ReceiptCheck::PreviousStateRoot))
    );
}

#[test]
fn malformed_state_module_version_and_signature_are_refused() {
    let (bytes, authority) = state_fixture();
    let offset = module_offset(&bytes);
    let mut wrong_module = bytes.clone();
    wrong_module[offset..offset + 2].copy_from_slice(&1_u16.to_be_bytes());
    assert_eq!(
        verify_authorized_receipt(&wrong_module, &authority),
        Err(EvidenceRefusal::Receipt(ReceiptCheck::Operation))
    );
    let mut wrong_version = bytes.clone();
    wrong_version[offset + 2..offset + 6].copy_from_slice(&3_u32.to_be_bytes());
    assert_eq!(
        verify_authorized_receipt(&wrong_version, &authority),
        Err(EvidenceRefusal::Receipt(ReceiptCheck::Module))
    );
    let mut bad_signature = bytes.clone();
    *bad_signature
        .last_mut()
        .unwrap_or_else(|| panic!("signature")) ^= 1;
    assert_eq!(
        verify_authorized_receipt(&bad_signature, &authority),
        Err(EvidenceRefusal::Receipt(ReceiptCheck::SequencerSignature))
    );
    assert_eq!(
        verify_authorized_receipt(&bytes[..bytes.len() - 1], &authority),
        Err(EvidenceRefusal::ReceiptDecode)
    );
}

#[test]
fn call_outcome_cannot_be_reclassified_as_state() {
    let (bytes, authority) = call_fixture();
    assert_eq!(verify_authorized_receipt(&bytes, &authority), Ok(()));
    assert!(verify_outcome(&bytes, &authority).is_ok());
    let offset = module_offset(&bytes);
    let mut state_with_outcome = bytes;
    state_with_outcome[offset + 10] = 0;
    assert_eq!(
        verify_authorized_receipt(&state_with_outcome, &authority),
        Err(EvidenceRefusal::Receipt(ReceiptCheck::ReceiptShape))
    );
}
