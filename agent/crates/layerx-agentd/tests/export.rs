use ed25519_dalek::{Signer as _, SigningKey as EdSigningKey};
use k256::ecdsa::{Signature, SigningKey};
use layerx_crypto::secp256k1;
use layerx_agentd::export::build;
use layerx_proof::checkpoint::{checkpoint_id, Attestation, Certificate, Checkpoint, GuarantorKey};
use layerx_proof::export::{
    verify as verify_export, CheckpointFact, DerivedAggregate, ExportVerificationError,
    InclusionFact, InclusionKind, OfflineExport, ReceiptFact,
};
use layerx_proof::inclusion::SequencerAuthorization;
use layerx_proof::merkle::build_proof;
use layerx_proof::receipt::{verify_outcome, AuthorizedBatch};
use layerx_wire::encode::Encoder;
use layerx_wire::hash::{batch_header_digest, checkpoint_attestation_digest, receipt_digest};

fn receipt_bytes(signature: Option<[u8; 64]>) -> Vec<u8> {
    let mut encoder = Encoder::new(4096);
    assert_eq!(encoder.structure_header(0x5201), Ok(()));
    assert_eq!(encoder.u16(1), Ok(()));
    assert_eq!(encoder.bytes(&[1; 32], 32), Ok(()));
    assert_eq!(encoder.u64(9), Ok(()));
    assert_eq!(encoder.bytes(&[2; 32], 32), Ok(()));
    assert_eq!(encoder.bytes(&[3; 32], 32), Ok(()));
    assert_eq!(encoder.bytes(&[8; 32], 32), Ok(()));
    assert_eq!(encoder.i32(0), Ok(()));
    assert_eq!(encoder.sequence_length(0, 512), Ok(()));
    assert_eq!(encoder.u128(1), Ok(()));
    assert_eq!(encoder.bytes(&[4; 32], 32), Ok(()));
    assert_eq!(encoder.u16(1), Ok(()));
    assert_eq!(encoder.u32(1), Ok(()));
    assert_eq!(encoder.u32(1), Ok(()));
    assert_eq!(encoder.u8(1), Ok(()));
    assert_eq!(encoder.bytes(&[5; 32], 32), Ok(()));
    assert_eq!(encoder.u128(25), Ok(()));
    assert_eq!(encoder.bytes(&[6; 32], 32), Ok(()));
    assert_eq!(encoder.u128(100), Ok(()));
    assert_eq!(encoder.u128(75), Ok(()));
    assert_eq!(encoder.u64(1), Ok(()));
    assert_eq!(encoder.bytes(&[7; 32], 32), Ok(()));
    assert_eq!(encoder.u128(10), Ok(()));
    assert_eq!(encoder.u128(35), Ok(()));
    assert_eq!(encoder.bytes(&[9; 32], 32), Ok(()));
    assert_eq!(encoder.bytes(&[10; 32], 32), Ok(()));
    assert_eq!(encoder.bytes(&[11; 32], 32), Ok(()));
    assert_eq!(encoder.u64(1_000), Ok(()));
    assert_eq!(encoder.u8(u8::from(signature.is_some())), Ok(()));
    if let Some(value) = signature {
        assert_eq!(encoder.bytes(&value, 64), Ok(()));
    }
    encoder.finish()
}

fn header_bytes(state_root: [u8; 32], activity_root: [u8; 32], sequencer_id: [u8; 32]) -> Vec<u8> {
    let mut encoder = Encoder::new(354);
    assert_eq!(encoder.structure_header(0x1701), Ok(()));
    assert_eq!(encoder.u8(15), Ok(()));
    for field in 1..=15 {
        assert_eq!(encoder.tag(field, 15), Ok(()));
        match field {
            1 => assert_eq!(encoder.u16(1), Ok(())),
            2 => assert_eq!(encoder.u32(42), Ok(())),
            3 => assert_eq!(encoder.u64(7), Ok(())),
            4 => assert_eq!(encoder.u64(8), Ok(())),
            5 => assert_eq!(encoder.u64(11), Ok(())),
            6 => assert_eq!(encoder.u64(19), Ok(())),
            7 => assert_eq!(encoder.bytes(&[7; 32], 32), Ok(())),
            8 => assert_eq!(encoder.bytes(&state_root, 32), Ok(())),
            9 => assert_eq!(encoder.bytes(&activity_root, 32), Ok(())),
            10 => assert_eq!(encoder.bytes(&[10; 32], 32), Ok(())),
            11 => assert_eq!(encoder.bytes(&[11; 32], 32), Ok(())),
            12 => assert_eq!(encoder.bytes(&[12; 32], 32), Ok(())),
            13 => assert_eq!(encoder.bytes(&[13; 32], 32), Ok(())),
            14 => assert_eq!(encoder.u64(1_000), Ok(())),
            15 => assert_eq!(encoder.bytes(&sequencer_id, 32), Ok(())),
            _ => panic!("unreachable header field"),
        }
    }
    encoder.finish()
}

fn guarantor_key(value: u8) -> (SigningKey, [u8; 33], [u8; 32]) {
    let mut scalar = [0_u8; 32];
    scalar[31] = value;
    let signing = SigningKey::from_bytes((&scalar).into())
        .unwrap_or_else(|error| panic!("guarantor key: {error}"));
    let encoded = signing.verifying_key().to_encoded_point(true);
    let public = encoded
        .as_bytes()
        .try_into()
        .unwrap_or_else(|_| panic!("public key width"));
    let mut identifier = [0_u8; 32];
    identifier[0] = value;
    (signing, public, identifier)
}

fn attestation(checkpoint: [u8; 32], guarantor_id: [u8; 32], key: &SigningKey) -> Attestation {
    let settlement_contract = [0x55; 20];
    let mut message = [0_u8; 189];
    message[..2].copy_from_slice(&1_u16.to_be_bytes());
    message[2..6].copy_from_slice(&42_u32.to_be_bytes());
    message[6..14].copy_from_slice(&31_337_u64.to_be_bytes());
    message[14..34].copy_from_slice(&settlement_contract);
    message[34..42].copy_from_slice(&7_u64.to_be_bytes());
    message[42..74].copy_from_slice(&checkpoint);
    message[74..106].copy_from_slice(&checkpoint);
    message[106..138].copy_from_slice(&guarantor_id);
    message[138..146].copy_from_slice(&8_u64.to_be_bytes());
    message[146..178].copy_from_slice(&[12; 32]);
    message[178] = 1;
    message[179] = 1;
    message[180] = 0x1f;
    message[181..].copy_from_slice(&(1_000 + u64::from(guarantor_id[0])).to_be_bytes());
    let digest = checkpoint_attestation_digest(&message)
        .unwrap_or_else(|error| panic!("attestation digest: {error:?}"));
    let (signature, recovery_id): (Signature, _) = key
        .sign_prehash_recoverable(&digest)
        .unwrap_or_else(|error| panic!("attestation signature: {error}"));
    let signer = secp256k1::evm_address(
        key.verifying_key().to_encoded_point(true).as_bytes(),
    )
    .unwrap_or_else(|error| panic!("attestation signer: {error:?}"));
    Attestation::new(
        1,
        42,
        31_337,
        settlement_contract,
        7,
        checkpoint,
        checkpoint,
        guarantor_id,
        8,
        [12; 32],
        true,
        true,
        0x1f,
        1_000 + u64::from(guarantor_id[0]),
        signer,
        signature.to_bytes().into(),
        27 + u8::from(recovery_id),
    )
}

fn artifact() -> OfflineExport {
    let receipt_key = EdSigningKey::from_bytes(&[3; 32]);
    let receipt_unsigned = receipt_bytes(None);
    let receipt_hash =
        receipt_digest(&receipt_unsigned).unwrap_or_else(|error| panic!("receipt hash: {error:?}"));
    let receipt_bytes = receipt_bytes(Some(receipt_key.sign(&receipt_hash).to_bytes()));
    let authorised_batch = AuthorizedBatch::new(
        [4; 32],
        [5; 32],
        [2; 32],
        [3; 32],
        receipt_key.verifying_key().to_bytes(),
    );
    let verified_receipt = verify_outcome(&receipt_bytes, &authorised_batch)
        .unwrap_or_else(|error| panic!("receipt: {error:?}"));
    let receipt_digest = verified_receipt
        .evidence()
        .receipt_digest()
        .unwrap_or_else(|| panic!("receipt digest absent"));

    let state = b"state-leaf".to_vec();
    let activity = b"activity-leaf".to_vec();
    let (state_proof, state_root) = build_proof(&[state.as_slice()], 0)
        .unwrap_or_else(|error| panic!("state proof: {error:?}"));
    let (_, activity_root) = build_proof(&[activity.as_slice()], 0)
        .unwrap_or_else(|error| panic!("activity proof: {error:?}"));
    let sequencer_key = EdSigningKey::from_bytes(&[7; 32]);
    let sequencer_id = sequencer_key.verifying_key().to_bytes();
    let header = header_bytes(state_root, activity_root, sequencer_id);
    let header_hash =
        batch_header_digest(&header).unwrap_or_else(|error| panic!("header hash: {error:?}"));
    let header_signature = sequencer_key.sign(&header_hash).to_bytes();
    let authorization = SequencerAuthorization::new(sequencer_id, sequencer_id, 8, 8);

    let checkpoint = Checkpoint::new(header.clone(), b"PROOF".to_vec());
    let checkpoint_identifier =
        checkpoint_id(&checkpoint).unwrap_or_else(|error| panic!("checkpoint id: {error:?}"));
    let mut attestations = Vec::new();
    let mut bonded_set = Vec::new();
    for value in 1..=2 {
        let (key, public, guarantor_id) = guarantor_key(value);
        attestations.push(attestation(checkpoint_identifier, guarantor_id, &key));
        bonded_set.push(GuarantorKey::new(guarantor_id, public, true));
    }

    OfflineExport {
        receipts: vec![ReceiptFact {
            statement: "activity receipt was produced by the authorised sequencer".to_owned(),
            canonical_receipt_bytes: receipt_bytes,
            authorised_batch,
            expected_receipt_digest: receipt_digest,
        }],
        inclusions: vec![InclusionFact {
            statement: "state leaf is included under the signed batch root".to_owned(),
            kind: InclusionKind::State,
            canonical_leaf_bytes: state,
            proof: state_proof,
            named_root: state_root,
            canonical_header_bytes: header,
            header_signature,
            sequencer_authorization: authorization,
        }],
        checkpoints: vec![CheckpointFact {
            statement: "bonded guarantors finalised the batch checkpoint".to_owned(),
            certificate: Certificate::new(checkpoint, attestations, 2, None),
            bonded_set,
            registered_checkpoint_id: checkpoint_identifier,
            registered_settlement_reference: None,
            availability_obtained: true,
        }],
        derived_aggregates: vec![DerivedAggregate {
            label: "local spend summary, not a protocol fact".to_owned(),
            rendered_value: "25".to_owned(),
            contributing_receipt_digests: vec![receipt_digest],
        }],
    }
}

#[test]
fn layerx_proof_verifies_complete_export_with_no_daemon_node_or_network() {
    let built = build(artifact()).unwrap_or_else(|error| panic!("build: {error:?}"));
    assert_eq!(built.local_verification.verified_receipts, 1);
    assert_eq!(built.local_verification.verified_inclusions, 1);
    assert_eq!(built.local_verification.verified_checkpoints, 1);
    assert!(
        !built
            .local_verification
            .derived_aggregates_are_protocol_facts
    );

    let offline = built.artifact;
    let third_party =
        verify_export(&offline).unwrap_or_else(|error| panic!("offline verification: {error:?}"));
    assert_eq!(third_party.verified_receipts, 1);
    assert_eq!(third_party.achieved_levels.len(), 3);
}

#[test]
fn hostile_export_changes_and_unknown_aggregate_contributors_fail() {
    let mut changed = artifact();
    changed.receipts[0].expected_receipt_digest[0] ^= 1;
    assert_eq!(
        verify_export(&changed),
        Err(ExportVerificationError::ReceiptDigest { index: 0 })
    );

    let mut unknown_contributor = artifact();
    unknown_contributor.derived_aggregates[0].contributing_receipt_digests[0] = [0x99; 32];
    assert!(matches!(
        verify_export(&unknown_contributor),
        Err(ExportVerificationError::UnknownAggregateContributor {
            aggregate: 0,
            digest
        }) if digest == [0x99; 32]
    ));
}
