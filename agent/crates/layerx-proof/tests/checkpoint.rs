use std::fs;
use std::path::{Path, PathBuf};

use k256::ecdsa::{Signature, SigningKey};
use layerx_crypto::secp256k1;
use layerx_proof::checkpoint::{
    checkpoint_id, verify_certificate, verify_declared_certificate, Attestation, Certificate,
    Checkpoint, CheckpointError, GuarantorKey, SettlementDomain,
};
use layerx_proof::settlement::{
    declared, declared_domain, maximum_attestation_delay_ms, DeclaredDomain,
    DECLARED_CHECKPOINT_SETTLEMENT,
};
use layerx_types::settlement::{
    DeclaredCheckpointSettlement, SettlementError, CHECKPOINT_SETTLEMENT_PATH,
};
use layerx_types::vectors::checkpoint::{
    load_checkpoint_vectors, CheckpointOutcome, CheckpointRejection, CheckpointVector,
    CHECKPOINT_VECTOR_CASES,
};
use layerx_types::verify::VerificationLevel;
use layerx_wire::encode::Encoder;
use layerx_wire::hash::checkpoint_attestation_digest;
use layerx_wire::limits::PROTOCOL_VERSION;

fn header_bytes() -> Vec<u8> {
    let mut encoder = Encoder::new(354);
    assert_eq!(
        encoder.structure_header_version(0x1701, PROTOCOL_VERSION),
        Ok(())
    );
    assert_eq!(encoder.u8(15), Ok(()));
    for field in 1..=15 {
        assert_eq!(encoder.tag(field, 15), Ok(()));
        match field {
            1 => assert_eq!(encoder.u16(PROTOCOL_VERSION), Ok(())),
            2 => assert_eq!(encoder.u32(42), Ok(())),
            3 => assert_eq!(encoder.u64(7), Ok(())),
            4 => assert_eq!(encoder.u64(8), Ok(())),
            5 => assert_eq!(encoder.u64(11), Ok(())),
            6 => assert_eq!(encoder.u64(19), Ok(())),
            7..=13 => assert_eq!(encoder.bytes(&[field; 32], 32), Ok(())),
            14 => assert_eq!(encoder.u64(1_000), Ok(())),
            15 => assert_eq!(encoder.bytes(&[15; 32], 32), Ok(())),
            _ => panic!("unreachable header field"),
        }
    }
    encoder.finish()
}

fn key(value: u8) -> (SigningKey, [u8; 33], [u8; 32]) {
    let mut scalar = [0_u8; 32];
    scalar[31] = value;
    let signing = SigningKey::from_bytes((&scalar).into())
        .unwrap_or_else(|error| panic!("invalid signing key: {error}"));
    let encoded = signing.verifying_key().to_encoded_point(true);
    let public_key: [u8; 33] = encoded
        .as_bytes()
        .try_into()
        .unwrap_or_else(|_| panic!("invalid compressed key width"));
    let mut id = [0_u8; 32];
    id[0] = value;
    (signing, public_key, id)
}

fn repository_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../..")
}

fn attestation(
    checkpoint_id: [u8; 32],
    guarantor_id: [u8; 32],
    signing_key: &SigningKey,
) -> Attestation {
    attestation_at(
        checkpoint_id,
        guarantor_id,
        signing_key,
        1_000 + u64::from(guarantor_id[0]),
    )
}

fn attestation_at(
    checkpoint_id: [u8; 32],
    guarantor_id: [u8; 32],
    signing_key: &SigningKey,
    attested_at_ms: u64,
) -> Attestation {
    let settlement_contract = [0x55; 20];
    let mut message = [0_u8; 189];
    message[..2].copy_from_slice(&PROTOCOL_VERSION.to_be_bytes());
    message[2..6].copy_from_slice(&42_u32.to_be_bytes());
    message[6..14].copy_from_slice(&31_337_u64.to_be_bytes());
    message[14..34].copy_from_slice(&settlement_contract);
    message[34..42].copy_from_slice(&7_u64.to_be_bytes());
    message[42..74].copy_from_slice(&checkpoint_id);
    message[74..106].copy_from_slice(&checkpoint_id);
    message[106..138].copy_from_slice(&guarantor_id);
    message[138..146].copy_from_slice(&8_u64.to_be_bytes());
    message[146..178].copy_from_slice(&[12; 32]);
    message[178] = 1;
    message[179] = 1;
    message[180] = 0x1f;
    message[181..].copy_from_slice(&attested_at_ms.to_be_bytes());
    let digest = checkpoint_attestation_digest(&message)
        .unwrap_or_else(|error| panic!("attestation hash failed: {error:?}"));
    let (signature, recovery_id): (Signature, _) = signing_key
        .sign_prehash_recoverable(&digest)
        .unwrap_or_else(|error| panic!("attestation signing failed: {error}"));
    let signer = secp256k1::evm_address(
        signing_key
            .verifying_key()
            .to_encoded_point(true)
            .as_bytes(),
    )
    .unwrap_or_else(|error| panic!("attestation signer: {error:?}"));
    Attestation::new(
        PROTOCOL_VERSION,
        42,
        31_337,
        settlement_contract,
        7,
        checkpoint_id,
        checkpoint_id,
        guarantor_id,
        8,
        [12; 32],
        true,
        true,
        0x1f,
        attested_at_ms,
        signer,
        signature.to_bytes().into(),
        27 + u8::from(recovery_id),
    )
}

fn vector_certificate(vector: &CheckpointVector, domain: &DeclaredDomain) -> Certificate {
    let attestations = vector
        .attestations
        .iter()
        .map(|attestation| {
            Attestation::new(
                vector.header.protocol_version,
                vector.header.network_id,
                domain.settlement().paxeer_chain_id(),
                domain.settlement().settlement_contract(),
                vector.header.epoch,
                vector.expected_digest,
                vector.expected_digest,
                attestation.guarantor_id,
                vector.header.batch_number,
                vector.header.data_availability_root,
                attestation.replayed,
                attestation.data_possessed,
                attestation.availability_class_mask,
                attestation.attested_at_ms,
                attestation.signer,
                attestation.signature,
                attestation.signature_v,
            )
        })
        .collect();
    Certificate::new(
        Checkpoint::new(vector.header.bytes.clone(), vector.validity_proof.clone()),
        attestations,
        vector.threshold,
        None,
    )
}

fn fixture_with_settlement(
    settlement_reference: Option<Vec<u8>>,
) -> (Certificate, Vec<GuarantorKey>, [u8; 32]) {
    let checkpoint = Checkpoint::new(header_bytes(), b"PROOF".to_vec());
    let identifier = checkpoint_id(&checkpoint)
        .unwrap_or_else(|error| panic!("checkpoint hash failed: {error:?}"));
    let mut attestations = Vec::new();
    let mut keys = Vec::new();
    for value in 1..=3 {
        let (signing, public, id) = key(value);
        attestations.push(attestation(identifier, id, &signing));
        keys.push(GuarantorKey::new(id, public, true));
    }
    (
        Certificate::new(checkpoint, attestations, 2, settlement_reference),
        keys,
        identifier,
    )
}

fn fixture() -> (Certificate, Vec<GuarantorKey>, [u8; 32]) {
    fixture_with_settlement(None)
}

fn settlement_domain() -> SettlementDomain {
    SettlementDomain::new(31_337, [0x55; 20])
}

#[test]
fn transported_registration_never_invents_paxeer_chain_finality() {
    let (certificate, keys, identifier) = fixture();
    let finalised = verify_certificate(&certificate, &keys, &identifier, settlement_domain(), None)
        .unwrap_or_else(|error| panic!("finalised certificate failed: {error:?}"));
    assert_eq!(finalised.achieved, 3);
    assert_eq!(finalised.required, 2);
    assert_eq!(finalised.protocol_version(), PROTOCOL_VERSION);
    assert_eq!(finalised.network_id(), 42);
    assert_eq!(finalised.batch_number(), 8);
    assert_eq!(finalised.first_sequence(), 11);
    assert_eq!(finalised.last_sequence(), 19);
    assert_eq!(finalised.data_availability_root(), [12; 32]);
    assert_eq!(finalised.record_roots().activity, [9; 32]);
    assert_eq!(finalised.record_roots().receipt, [10; 32]);
    assert_eq!(finalised.record_roots().event, [11; 32]);
    assert_eq!(finalised.record_roots().oracle, [13; 32]);
    assert_eq!(finalised.resulting_state_root(), [8; 32]);
    assert_eq!(finalised.level(), VerificationLevel::CHECKPOINT_FINALISED);
    assert_eq!(finalised.evidence().checkpoint_id(), Some(identifier));
    assert_eq!(finalised.evidence().settlement_reference(), None);

    let (anchored, anchored_keys, anchored_identifier) =
        fixture_with_settlement(Some(b"paxeer-registered-1".to_vec()));
    let registered_report = verify_certificate(
        &anchored,
        &anchored_keys,
        &anchored_identifier,
        settlement_domain(),
        Some(b"paxeer-registered-1"),
    )
    .unwrap_or_else(|error| panic!("registered certificate failed: {error:?}"));
    assert_eq!(registered_report.achieved, 3);
    assert_eq!(registered_report.required, 2);
    assert_eq!(
        registered_report.level(),
        VerificationLevel::CHECKPOINT_FINALISED
    );
    assert_ne!(
        registered_report.level(),
        VerificationLevel::SETTLEMENT_ANCHORED
    );
    assert_eq!(
        registered_report.evidence().settlement_reference(),
        Some(b"paxeer-registered-1".as_slice())
    );
    assert_eq!(
        verify_certificate(
            &anchored,
            &anchored_keys,
            &anchored_identifier,
            settlement_domain(),
            Some(b"different-reference"),
        ),
        Err(CheckpointError::Settlement)
    );
}

#[test]
fn rejects_threshold_duplicate_membership_signature_and_identifier_failures() {
    let (certificate, keys, identifier) = fixture();
    let (signing, _, first_id) = key(1);
    let duplicate = attestation(identifier, first_id, &signing);
    let duplicate_certificate = Certificate::new(
        Checkpoint::new(header_bytes(), b"PROOF".to_vec()),
        vec![duplicate.clone(), duplicate],
        2,
        None,
    );
    assert_eq!(
        verify_certificate(
            &duplicate_certificate,
            &keys,
            &identifier,
            settlement_domain(),
            None
        ),
        Err(CheckpointError::DuplicateSigner(first_id))
    );

    let (outsider_signing, _, outsider_id) = key(9);
    let outsider = Certificate::new(
        Checkpoint::new(header_bytes(), b"PROOF".to_vec()),
        vec![attestation(identifier, outsider_id, &outsider_signing)],
        1,
        None,
    );
    assert_eq!(
        verify_certificate(&outsider, &keys, &identifier, settlement_domain(), None),
        Err(CheckpointError::SignerMembership(outsider_id))
    );

    let one_signature = Certificate::new(
        Checkpoint::new(header_bytes(), b"PROOF".to_vec()),
        vec![attestation(identifier, first_id, &signing)],
        2,
        None,
    );
    assert_eq!(
        verify_certificate(
            &one_signature,
            &keys,
            &identifier,
            settlement_domain(),
            None
        ),
        Err(CheckpointError::Threshold {
            achieved: 1,
            required: 2,
        })
    );

    let bad_signature = Certificate::new(
        Checkpoint::new(header_bytes(), b"PROOF".to_vec()),
        vec![Attestation::new(
            PROTOCOL_VERSION,
            42,
            31_337,
            [0x55; 20],
            7,
            identifier,
            identifier,
            first_id,
            8,
            [12; 32],
            true,
            true,
            0x1f,
            1_001,
            [1; 20],
            [1; 64],
            27,
        )],
        1,
        None,
    );
    assert_eq!(
        verify_certificate(
            &bad_signature,
            &keys,
            &identifier,
            settlement_domain(),
            None
        ),
        Err(CheckpointError::Signature(first_id))
    );

    assert_eq!(
        verify_certificate(&certificate, &keys, &[99; 32], settlement_domain(), None),
        Err(CheckpointError::CheckpointIdentifier)
    );
    assert_eq!(
        verify_certificate(
            &certificate,
            &keys,
            &identifier,
            SettlementDomain::new(31_338, [0x55; 20]),
            None,
        ),
        Err(CheckpointError::CheckpointFields)
    );
    assert_eq!(
        verify_certificate(
            &certificate,
            &keys,
            &identifier,
            SettlementDomain::new(31_337, [0x56; 20]),
            None,
        ),
        Err(CheckpointError::CheckpointFields)
    );
    assert_eq!(
        verify_certificate(
            &certificate,
            &keys,
            &identifier,
            SettlementDomain::new(0, [0; 20]),
            None,
        ),
        Err(CheckpointError::Settlement)
    );
}

#[test]
fn embedded_settlement_is_the_declared_document() {
    let on_disk = fs::read_to_string(repository_root().join(CHECKPOINT_SETTLEMENT_PATH))
        .unwrap_or_else(|error| panic!("declared settlement unreadable: {error}"));
    assert_eq!(on_disk, DECLARED_CHECKPOINT_SETTLEMENT);
    let parsed = DeclaredCheckpointSettlement::parse(&on_disk)
        .unwrap_or_else(|error| panic!("declared settlement invalid: {error}"));
    let embedded =
        declared().unwrap_or_else(|error| panic!("embedded settlement invalid: {error}"));
    assert_eq!(*embedded, parsed);
    assert_eq!(
        maximum_attestation_delay_ms(),
        Ok(parsed.finality_policy().maximum_attestation_delay_seconds() * 1_000)
    );
    assert_eq!(
        declared_domain("undeclared").map(|domain| domain.name().to_owned()),
        Err(SettlementError::UnknownDomain("undeclared".to_owned()))
    );
    let vectors = declared_domain("vectors")
        .unwrap_or_else(|error| panic!("vectors domain undeclared: {error}"));
    assert_eq!(vectors.network_id(), 42);
    assert_eq!(vectors.settlement().paxeer_chain_id(), 31_337);
    assert_eq!(vectors.guarantor_set().len(), 3);
    assert_eq!(
        vectors.certificate_threshold(),
        parsed.finality_policy().certificate_threshold()
    );
}

#[test]
fn checkpoint_vectors_apply_the_declared_freshness_window() {
    let vectors = load_checkpoint_vectors(&repository_root())
        .unwrap_or_else(|error| panic!("checkpoint vectors failed to load: {error:?}"));
    assert_eq!(vectors.len(), CHECKPOINT_VECTOR_CASES.len());
    let maximum_delay_ms = maximum_attestation_delay_ms()
        .unwrap_or_else(|error| panic!("declared delay unavailable: {error}"));
    for vector in &vectors {
        let domain = declared_domain(&vector.settlement_domain)
            .unwrap_or_else(|error| panic!("{}: domain undeclared: {error}", vector.case_name));
        let certificate = vector_certificate(vector, &domain);
        assert_eq!(
            checkpoint_id(certificate.checkpoint()),
            Ok(vector.expected_digest),
            "{}",
            vector.case_name
        );
        let declared_result = verify_declared_certificate(
            &certificate,
            &vector.settlement_domain,
            &vector.expected_digest,
            None,
        );
        let generic_result = verify_certificate(
            &certificate,
            domain.guarantor_set(),
            &vector.expected_digest,
            domain.settlement(),
            None,
        );
        assert_eq!(declared_result, generic_result, "{}", vector.case_name);
        let first = &vector.attestations[0];
        match vector.outcome {
            CheckpointOutcome::Accept => {
                let report = declared_result.unwrap_or_else(|error| {
                    panic!("{}: expected accept, got {error:?}", vector.case_name)
                });
                assert_eq!(report.achieved, vector.attestations.len());
                assert_eq!(report.required, vector.threshold);
                assert_eq!(report.level(), VerificationLevel::CHECKPOINT_FINALISED);
                assert_eq!(report.batch_number(), vector.header.batch_number);
                assert_eq!(report.network_id(), vector.header.network_id);
                assert_eq!(
                    report.resulting_state_root(),
                    vector.header.resulting_state_root
                );
                assert_eq!(
                    report.evidence().checkpoint_id(),
                    Some(vector.expected_digest)
                );
            }
            CheckpointOutcome::Reject(CheckpointRejection::NotYetValid) => assert_eq!(
                declared_result,
                Err(CheckpointError::AttestationNotYetValid {
                    guarantor_id: first.guarantor_id,
                    attested_at_ms: first.attested_at_ms,
                    header_timestamp_ms: vector.header.timestamp_ms,
                }),
                "{}",
                vector.case_name
            ),
            CheckpointOutcome::Reject(CheckpointRejection::Expired) => assert_eq!(
                declared_result,
                Err(CheckpointError::AttestationExpired {
                    guarantor_id: first.guarantor_id,
                    attested_at_ms: first.attested_at_ms,
                    deadline_ms: vector.header.timestamp_ms + maximum_delay_ms,
                }),
                "{}",
                vector.case_name
            ),
        }
    }
}

#[test]
fn freshness_window_is_header_relative_and_closed_on_both_ends() {
    let maximum_delay_ms = maximum_attestation_delay_ms()
        .unwrap_or_else(|error| panic!("declared delay unavailable: {error}"));
    let checkpoint = Checkpoint::new(header_bytes(), b"PROOF".to_vec());
    let identifier = checkpoint_id(&checkpoint)
        .unwrap_or_else(|error| panic!("checkpoint hash failed: {error:?}"));
    let mut keys = Vec::new();
    for value in 1..=3 {
        let (_, public, id) = key(value);
        keys.push(GuarantorKey::new(id, public, true));
    }
    let (signing, _, first_id) = key(1);
    let certificate_at = |attested_at_ms: u64| {
        Certificate::new(
            checkpoint.clone(),
            vec![attestation_at(
                identifier,
                first_id,
                &signing,
                attested_at_ms,
            )],
            1,
            None,
        )
    };
    for attested_at_ms in [1_000, 1_000 + maximum_delay_ms] {
        let report = verify_certificate(
            &certificate_at(attested_at_ms),
            &keys,
            &identifier,
            settlement_domain(),
            None,
        )
        .unwrap_or_else(|error| panic!("boundary {attested_at_ms} rejected: {error:?}"));
        assert_eq!(report.achieved, 1);
    }
    assert_eq!(
        verify_certificate(
            &certificate_at(999),
            &keys,
            &identifier,
            settlement_domain(),
            None
        ),
        Err(CheckpointError::AttestationNotYetValid {
            guarantor_id: first_id,
            attested_at_ms: 999,
            header_timestamp_ms: 1_000,
        })
    );
    assert_eq!(
        verify_certificate(
            &certificate_at(1_001 + maximum_delay_ms),
            &keys,
            &identifier,
            settlement_domain(),
            None
        ),
        Err(CheckpointError::AttestationExpired {
            guarantor_id: first_id,
            attested_at_ms: 1_001 + maximum_delay_ms,
            deadline_ms: 1_000 + maximum_delay_ms,
        })
    );
}

#[test]
fn declared_domain_verification_rejects_foreign_contracts_and_weak_thresholds() {
    let (certificate, _, identifier) = fixture();
    assert_eq!(
        verify_declared_certificate(&certificate, "vectors", &identifier, None),
        Err(CheckpointError::CheckpointFields)
    );
    let weak = Certificate::new(
        certificate.checkpoint().clone(),
        certificate.attestations().to_vec(),
        1,
        None,
    );
    assert_eq!(
        verify_declared_certificate(&weak, "vectors", &identifier, None),
        Err(CheckpointError::Threshold {
            achieved: 1,
            required: 2,
        })
    );
    assert_eq!(
        verify_declared_certificate(&certificate, "undeclared", &identifier, None),
        Err(CheckpointError::Configuration(
            SettlementError::UnknownDomain("undeclared".to_owned())
        ))
    );
}
