use ed25519_dalek::{Signer as _, SigningKey as EdSigningKey};
use k256::ecdsa::{Signature, SigningKey};
use layerx_agentd::read::{
    checkpoint, proof_bundle, CheckpointReadError, ProofBundleKind, ProofBundleRequest,
};
use layerx_crypto::secp256k1;
use layerx_proof::checkpoint::{
    checkpoint_id, Attestation, Certificate, Checkpoint, CheckpointError, GuarantorKey,
    SettlementDomain,
};
use layerx_proof::inclusion::SequencerAuthorization;
use layerx_proof::merkle::{build_proof, Proof};
use layerx_types::verify::VerificationLevel;
use layerx_wire::encode::Encoder;
use layerx_wire::hash::{batch_header_digest, checkpoint_attestation_digest};

fn header_bytes(state_root: [u8; 32], activity_root: [u8; 32], sequencer_id: [u8; 32]) -> Vec<u8> {
    let mut encoder = Encoder::new(354);
    assert_eq!(
        encoder.structure_header_version(0x1701, layerx_wire::limits::PROTOCOL_VERSION),
        Ok(())
    );
    assert_eq!(encoder.u8(15), Ok(()));
    for field in 1..=15 {
        assert_eq!(encoder.tag(field, 15), Ok(()));
        match field {
            1 => assert_eq!(encoder.u16(layerx_wire::limits::PROTOCOL_VERSION), Ok(())),
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

fn settlement_domain() -> SettlementDomain {
    SettlementDomain::new(31_337, [0x55; 20])
}

fn guarantor_key(value: u8) -> (SigningKey, [u8; 33], [u8; 32]) {
    let mut scalar = [0_u8; 32];
    scalar[31] = value;
    let signing = SigningKey::from_bytes((&scalar).into())
        .unwrap_or_else(|error| panic!("signing key: {error}"));
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
    message[..2].copy_from_slice(&layerx_wire::limits::PROTOCOL_VERSION.to_be_bytes());
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
    let signer = secp256k1::evm_address(key.verifying_key().to_encoded_point(true).as_bytes())
        .unwrap_or_else(|error| panic!("attestation signer: {error:?}"));
    Attestation::new(
        layerx_wire::limits::PROTOCOL_VERSION,
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

fn certificate(
    header: &[u8],
    signers: u8,
    threshold: usize,
    settlement: Option<Vec<u8>>,
    duplicate: bool,
) -> (Certificate, Vec<GuarantorKey>, [u8; 32]) {
    let body = Checkpoint::new(header.to_vec(), b"PROOF".to_vec());
    let identifier =
        checkpoint_id(&body).unwrap_or_else(|error| panic!("checkpoint identifier: {error:?}"));
    let mut attestations = Vec::new();
    let mut keys = Vec::new();
    for value in 1..=signers {
        let (signing, public, guarantor_id) = guarantor_key(value);
        attestations.push(attestation(identifier, guarantor_id, &signing));
        keys.push(GuarantorKey::new(guarantor_id, public, true));
    }
    if duplicate {
        attestations.push(attestations[0].clone());
    }
    (
        Certificate::new(body, attestations, threshold, settlement),
        keys,
        identifier,
    )
}

struct InclusionFixture {
    state: Vec<u8>,
    state_proof: Proof,
    state_root: [u8; 32],
    header: Vec<u8>,
    header_signature: [u8; 64],
    authorization: SequencerAuthorization,
}

fn inclusion_fixture() -> InclusionFixture {
    let state = b"state-leaf".to_vec();
    let activity = b"activity-leaf".to_vec();
    let (state_proof, state_root) = build_proof(&[state.as_slice()], 0)
        .unwrap_or_else(|error| panic!("state proof: {error:?}"));
    let (_, activity_root) = build_proof(&[activity.as_slice()], 0)
        .unwrap_or_else(|error| panic!("activity proof: {error:?}"));
    let key = EdSigningKey::from_bytes(&[7; 32]);
    let sequencer_id = key.verifying_key().to_bytes();
    let header = header_bytes(state_root, activity_root, sequencer_id);
    let digest =
        batch_header_digest(&header).unwrap_or_else(|error| panic!("header digest: {error:?}"));
    InclusionFixture {
        state,
        state_proof,
        state_root,
        header,
        header_signature: key.sign(&digest).to_bytes(),
        authorization: SequencerAuthorization::new(sequencer_id, sequencer_id, 8, 8),
    }
}

#[test]
fn verified_checkpoint_derives_commitments_signers_and_registration_without_anchor_escalation() {
    let fixture = inclusion_fixture();
    let (certificate, bonded, identifier) = certificate(
        &fixture.header,
        3,
        2,
        Some(b"paxeer-anchor".to_vec()),
        false,
    );
    let served = checkpoint(
        &certificate,
        &bonded,
        identifier,
        settlement_domain(),
        Some(b"paxeer-anchor"),
        true,
    )
    .unwrap_or_else(|error| panic!("checkpoint: {error:?}"));
    assert_eq!(served.checkpoint_id, identifier);
    assert_eq!(served.header_bytes, fixture.header);
    assert_eq!(served.commitments.batch_number, 8);
    assert_eq!(served.commitments.resulting_state_root, fixture.state_root);
    assert_eq!(served.commitments.availability_root, [12; 32]);
    assert_eq!(served.guarantor_signatures.len(), 3);
    assert_eq!(served.achieved, 3);
    assert_eq!(served.threshold, 2);
    assert_eq!(
        served.settlement_reference.as_deref(),
        Some(b"paxeer-anchor".as_slice())
    );
    assert_eq!(
        served.verification_level,
        VerificationLevel::CHECKPOINT_FINALISED
    );

    let bundle = proof_bundle(&ProofBundleRequest {
        kind: ProofBundleKind::State,
        canonical_leaf_bytes: &fixture.state,
        proof: &fixture.state_proof,
        named_root: fixture.state_root,
        header_bytes: &fixture.header,
        header_signature: fixture.header_signature,
        authorization: &fixture.authorization,
    })
    .unwrap_or_else(|error| panic!("proof bundle: {error:?}"));
    assert_eq!(bundle.named_root, fixture.state_root);
    assert_eq!(bundle.batch_number, 8);
    assert_eq!(bundle.verification_level, VerificationLevel::STATE_PROVEN);
}

#[test]
fn unavailable_subthreshold_duplicate_and_settlement_mismatch_are_refused() {
    let fixture = inclusion_fixture();
    let (valid, bonded, identifier) = certificate(&fixture.header, 2, 2, None, false);
    assert_eq!(
        checkpoint(
            &valid,
            &bonded,
            identifier,
            settlement_domain(),
            None,
            false
        ),
        Err(CheckpointReadError::AvailabilityUnavailable {
            checkpoint_id: identifier
        })
    );

    let (subthreshold, keys, identifier) = certificate(&fixture.header, 1, 2, None, false);
    assert!(matches!(
        checkpoint(
            &subthreshold,
            &keys,
            identifier,
            settlement_domain(),
            None,
            true
        ),
        Err(CheckpointReadError::Certificate(
            CheckpointError::Threshold {
                achieved: 1,
                required: 2
            }
        ))
    ));

    let (duplicate, keys, identifier) = certificate(&fixture.header, 1, 2, None, true);
    assert!(matches!(
        checkpoint(
            &duplicate,
            &keys,
            identifier,
            settlement_domain(),
            None,
            true
        ),
        Err(CheckpointReadError::Certificate(
            CheckpointError::DuplicateSigner(_)
        ))
    ));

    let (settlement, keys, identifier) =
        certificate(&fixture.header, 2, 2, Some(b"registered".to_vec()), false);
    assert_eq!(
        checkpoint(
            &settlement,
            &keys,
            identifier,
            settlement_domain(),
            Some(b"mismatch"),
            true,
        ),
        Err(CheckpointReadError::Certificate(
            CheckpointError::Settlement
        ))
    );
}
