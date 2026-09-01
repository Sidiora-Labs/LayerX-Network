use ed25519_dalek::{Signer as _, SigningKey};
use layerx_proof::inclusion::{
    verify_activity, verify_state, InclusionError, SequencerAuthorization,
};
use layerx_proof::merkle::{build_proof, MerkleError, Proof};
use layerx_types::verify::VerificationLevel;
use layerx_wire::encode::Encoder;
use layerx_wire::hash::batch_header_digest;
use layerx_wire::limits::PROTOCOL_VERSION;

fn header_bytes(
    batch_number: u64,
    resulting_state_root: [u8; 32],
    activity_merkle_root: [u8; 32],
    sequencer_id: [u8; 32],
) -> Vec<u8> {
    let mut encoder = Encoder::new(354);
    assert_eq!(
        encoder.structure_header_version(0x1701, PROTOCOL_VERSION),
        Ok(())
    );
    assert_eq!(encoder.u8(15), Ok(()));
    let fields: [(u8, Vec<u8>); 15] = [
        (1, PROTOCOL_VERSION.to_be_bytes().to_vec()),
        (2, 42_u32.to_be_bytes().to_vec()),
        (3, 2_u64.to_be_bytes().to_vec()),
        (4, batch_number.to_be_bytes().to_vec()),
        (5, 9_u64.to_be_bytes().to_vec()),
        (6, 10_u64.to_be_bytes().to_vec()),
        (7, [1; 32].to_vec()),
        (8, resulting_state_root.to_vec()),
        (9, activity_merkle_root.to_vec()),
        (10, [2; 32].to_vec()),
        (11, [3; 32].to_vec()),
        (12, [4; 32].to_vec()),
        (13, [5; 32].to_vec()),
        (14, 1_000_u64.to_be_bytes().to_vec()),
        (15, sequencer_id.to_vec()),
    ];
    for (field, value) in fields {
        assert_eq!(encoder.tag(field, 15), Ok(()));
        match field {
            1 => assert_eq!(
                encoder.u16(u16::from_be_bytes([value[0], value[1]])),
                Ok(())
            ),
            2 => assert_eq!(
                encoder.u32(u32::from_be_bytes([value[0], value[1], value[2], value[3]])),
                Ok(())
            ),
            3..=6 | 14 => assert_eq!(
                encoder.u64(u64::from_be_bytes(
                    value
                        .as_slice()
                        .try_into()
                        .unwrap_or_else(|_| panic!("invalid u64 test field")),
                )),
                Ok(())
            ),
            _ => assert_eq!(encoder.bytes(&value, 32), Ok(())),
        }
    }
    let bytes = encoder.finish();
    assert_eq!(bytes.len(), 354);
    bytes
}

fn sign_header(bytes: &[u8], key: &SigningKey) -> [u8; 64] {
    let digest =
        batch_header_digest(bytes).unwrap_or_else(|error| panic!("header hash failed: {error:?}"));
    key.sign(&digest).to_bytes()
}

#[test]
fn raises_only_the_level_each_proof_establishes() {
    let key = SigningKey::from_bytes(&[7; 32]);
    let sequencer_id = key.verifying_key().to_bytes();
    let activities: [&[u8]; 3] = [b"activity-a", b"activity-b", b"activity-c"];
    let states: [&[u8]; 2] = [b"state-a", b"state-b"];
    let (activity_proof, activity_root) = build_proof(&activities, 1)
        .unwrap_or_else(|error| panic!("activity proof failed: {error:?}"));
    let (state_proof, state_root) =
        build_proof(&states, 0).unwrap_or_else(|error| panic!("state proof failed: {error:?}"));
    let header = header_bytes(7, state_root, activity_root, sequencer_id);
    let signature = sign_header(&header, &key);
    let authorization = SequencerAuthorization::new(sequencer_id, sequencer_id, 7, 9);

    let activity = verify_activity(
        activities[1],
        &activity_proof,
        &header,
        &signature,
        &authorization,
    )
    .unwrap_or_else(|error| panic!("activity inclusion failed: {error:?}"));
    assert_eq!(activity.level(), VerificationLevel::BATCH_INCLUDED);

    let state = verify_state(
        states[0],
        &state_proof,
        &state_root,
        &header,
        &signature,
        &authorization,
    )
    .unwrap_or_else(|error| panic!("state inclusion failed: {error:?}"));
    assert_eq!(state.level(), VerificationLevel::STATE_PROVEN);
}

#[test]
fn rejects_truncated_swapped_and_unauthorised_evidence() {
    let key = SigningKey::from_bytes(&[7; 32]);
    let sequencer_id = key.verifying_key().to_bytes();
    let activities: [&[u8]; 3] = [b"activity-a", b"activity-b", b"activity-c"];
    let states: [&[u8]; 2] = [b"state-a", b"state-b"];
    let (proof, activity_root) = build_proof(&activities, 1)
        .unwrap_or_else(|error| panic!("activity proof failed: {error:?}"));
    let (_, state_root) =
        build_proof(&states, 0).unwrap_or_else(|error| panic!("state proof failed: {error:?}"));
    let header = header_bytes(7, state_root, activity_root, sequencer_id);
    let signature = sign_header(&header, &key);
    let authorization = SequencerAuthorization::new(sequencer_id, sequencer_id, 7, 9);

    let mut truncated = proof.siblings().to_vec();
    let _ = truncated.pop();
    assert_eq!(
        Proof::new(proof.leaf_index(), proof.leaf_count(), truncated),
        Err(MerkleError::PathLength {
            expected: proof.siblings().len(),
            actual: proof.siblings().len() - 1,
        })
    );

    let swapped = header_bytes(7, state_root, [99; 32], sequencer_id);
    let swapped_signature = sign_header(&swapped, &key);
    assert!(matches!(
        verify_activity(
            activities[1],
            &proof,
            &swapped,
            &swapped_signature,
            &authorization,
        ),
        Err(InclusionError::Merkle(MerkleError::RootMismatch))
    ));

    let wrong_batch = SequencerAuthorization::new(sequencer_id, sequencer_id, 8, 9);
    assert_eq!(
        verify_activity(activities[1], &proof, &header, &signature, &wrong_batch),
        Err(InclusionError::BatchNumber)
    );

    let wrong_identity = SequencerAuthorization::new([55; 32], sequencer_id, 7, 9);
    assert_eq!(
        verify_activity(activities[1], &proof, &header, &signature, &wrong_identity,),
        Err(InclusionError::SequencerIdentity)
    );
}
