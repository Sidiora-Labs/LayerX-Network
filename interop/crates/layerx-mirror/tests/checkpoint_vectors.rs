// The mirror publisher recomputes checkpoint identity through
// layerx_wire::hash::checkpoint_id and finalises certificates through
// layerx_proof certificate verification. Both must agree with the shared
// cross-language checkpoint vectors and the declared settlement configuration.

use std::path::{Path, PathBuf};

use layerx_proof::checkpoint::{
    checkpoint_id as certificate_checkpoint_id, verify_declared_certificate, Attestation,
    Certificate, Checkpoint, CheckpointError,
};
use layerx_proof::settlement::{declared, declared_domain, maximum_attestation_delay_ms};
use layerx_types::vectors::checkpoint::{
    load_checkpoint_vectors, CheckpointOutcome, CheckpointRejection, CHECKPOINT_VECTOR_CASES,
};
use layerx_wire::hash::{checkpoint_id, Domain};

fn repository_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../..")
}

#[test]
fn mirror_checkpoint_identity_and_freshness_follow_the_shared_vectors() {
    let settlement = declared().unwrap_or_else(|error| panic!("declared settlement: {error}"));
    assert_eq!(
        Domain::CheckpointCertificate.tag(),
        settlement.checkpoint_certificate_domain()
    );
    let maximum_delay_ms = maximum_attestation_delay_ms()
        .unwrap_or_else(|error| panic!("declared delay unavailable: {error}"));
    let vectors = load_checkpoint_vectors(&repository_root())
        .unwrap_or_else(|error| panic!("checkpoint vectors failed to load: {error:?}"));
    assert_eq!(vectors.len(), CHECKPOINT_VECTOR_CASES.len());
    for vector in &vectors {
        let name = vector.case_name.as_str();
        assert!(
            vector
                .header
                .bytes
                .starts_with(settlement.header_encoding_prefix()),
            "{name}: header prefix"
        );
        assert_eq!(
            checkpoint_id(&vector.header.bytes, &vector.validity_proof),
            Ok(vector.expected_digest),
            "{name}: mirror checkpoint identity"
        );
        let domain = declared_domain(&vector.settlement_domain)
            .unwrap_or_else(|error| panic!("{name}: domain undeclared: {error}"));
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
        let certificate = Certificate::new(
            Checkpoint::new(vector.header.bytes.clone(), vector.validity_proof.clone()),
            attestations,
            vector.threshold,
            None,
        );
        assert_eq!(
            certificate_checkpoint_id(certificate.checkpoint()),
            Ok(vector.expected_digest),
            "{name}"
        );
        let result = verify_declared_certificate(
            &certificate,
            &vector.settlement_domain,
            &vector.expected_digest,
            None,
        );
        let first = &vector.attestations[0];
        match vector.outcome {
            CheckpointOutcome::Accept => {
                let report =
                    result.unwrap_or_else(|error| panic!("{name}: expected accept, got {error:?}"));
                assert_eq!(report.achieved, vector.attestations.len());
                assert_eq!(
                    report.evidence().checkpoint_id(),
                    Some(vector.expected_digest)
                );
            }
            CheckpointOutcome::Reject(CheckpointRejection::NotYetValid) => assert_eq!(
                result,
                Err(CheckpointError::AttestationNotYetValid {
                    guarantor_id: first.guarantor_id,
                    attested_at_ms: first.attested_at_ms,
                    header_timestamp_ms: vector.header.timestamp_ms,
                }),
                "{name}"
            ),
            CheckpointOutcome::Reject(CheckpointRejection::Expired) => assert_eq!(
                result,
                Err(CheckpointError::AttestationExpired {
                    guarantor_id: first.guarantor_id,
                    attested_at_ms: first.attested_at_ms,
                    deadline_ms: vector.header.timestamp_ms + maximum_delay_ms,
                }),
                "{name}"
            ),
        }
    }
}
