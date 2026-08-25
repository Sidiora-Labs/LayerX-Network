mod support;

use layerx_agentd::protocol_evidence::{
    EvidenceAuthority, ReceiptEvidenceError, StateEvidenceError, VerifierPolicyError,
};
use layerx_agentd::boot::GateError;

use support::{
    StateHeaderIdentity, TestAuthorityPolicy, TestAuthorityRecord,
};

struct PolicyIdentity {
    signing_seed: [u8; 32],
    protocol_version: u16,
    network_id: u32,
    epoch: u64,
    first_batch: u64,
    last_batch: u64,
    revoked_at_batch: Option<u64>,
    handshake_signing_seed: [u8; 32],
    handshake_batch: u64,
}

fn verifier(identity: PolicyIdentity) -> EvidenceAuthority {
    let mut records = vec![TestAuthorityRecord {
        signing_seed: identity.signing_seed,
        epoch: identity.epoch,
        first_batch: identity.first_batch,
        last_batch: identity.last_batch,
        revoked_at_batch: identity.revoked_at_batch,
    }];
    if identity.handshake_signing_seed != identity.signing_seed {
        records.push(TestAuthorityRecord {
            signing_seed: identity.handshake_signing_seed,
            epoch: identity.epoch,
            first_batch: identity.handshake_batch,
            last_batch: identity.handshake_batch,
            revoked_at_batch: None,
        });
    }
    support::evidence_authority(TestAuthorityPolicy {
        protocol_version: identity.protocol_version,
        network_id: identity.network_id,
        records: &records,
        handshake_signing_seed: identity.handshake_signing_seed,
        handshake_batch: identity.handshake_batch,
    })
}

fn state(identity: StateHeaderIdentity) -> layerx_agentd::protocol_evidence::RawStateEvidence {
    support::raw_budget_state_with(10, 90, 1, 100, 50, identity)
}

#[test]
fn trusted_policy_issues_state_evidence_only_for_its_exact_identity() {
    let raw = support::raw_budget_state(10, 90, 1, 100, 50);
    let verified = support::evidence_verifier()
        .verify_state(&raw)
        .unwrap_or_else(|error| panic!("trusted evidence: {error:?}"));
    assert_eq!(
        verified.level(),
        layerx_types::verify::VerificationLevel::STATE_PROVEN
    );
    assert_eq!(verified.observed_head_sequence(), 50);
}

#[test]
fn a_caller_cannot_substitute_a_policy_after_the_daemon_handshake() {
    assert_eq!(
        support::try_evidence_authority(TestAuthorityPolicy {
            protocol_version: 1,
            network_id: 42,
            records: &[TestAuthorityRecord {
                signing_seed: [0x4a; 32],
                epoch: 2,
                first_batch: 7,
                last_batch: 7,
                revoked_at_batch: None,
            }],
            handshake_signing_seed: [0x5a; 32],
            handshake_batch: 7,
        }),
        Err(GateError::Evidence(VerifierPolicyError::HandshakeKey))
    );
}

#[test]
fn an_attacker_signed_header_cannot_supply_its_own_trust_key() {
    let attacker = state(StateHeaderIdentity {
        signing_seed: [0x5a; 32],
        protocol_version: 1,
        network_id: 42,
        epoch: 2,
        batch_number: 7,
    });
    assert_eq!(
        verifier(PolicyIdentity {
            signing_seed: [0x4a; 32],
            protocol_version: 1,
            network_id: 42,
            epoch: 2,
            first_batch: 7,
            last_batch: 7,
            revoked_at_batch: None,
            handshake_signing_seed: [0x4a; 32],
            handshake_batch: 7,
        })
        .verify_state(&attacker),
        Err(StateEvidenceError::Policy(
            VerifierPolicyError::UnknownSequencer
        ))
    );
}

#[test]
fn cross_network_and_cross_protocol_headers_are_refused_before_signature_use() {
    let cross_network = state(StateHeaderIdentity {
        signing_seed: [0x4a; 32],
        protocol_version: 1,
        network_id: 43,
        epoch: 2,
        batch_number: 7,
    });
    assert_eq!(
        verifier(PolicyIdentity {
            signing_seed: [0x4a; 32],
            protocol_version: 1,
            network_id: 42,
            epoch: 2,
            first_batch: 7,
            last_batch: 7,
            revoked_at_batch: None,
            handshake_signing_seed: [0x4a; 32],
            handshake_batch: 7,
        })
        .verify_state(&cross_network),
        Err(StateEvidenceError::Policy(VerifierPolicyError::Network))
    );

    let cross_protocol = state(StateHeaderIdentity {
        signing_seed: [0x4a; 32],
        protocol_version: 2,
        network_id: 42,
        epoch: 2,
        batch_number: 7,
    });
    assert_eq!(
        verifier(PolicyIdentity {
            signing_seed: [0x4a; 32],
            protocol_version: 1,
            network_id: 42,
            epoch: 2,
            first_batch: 7,
            last_batch: 7,
            revoked_at_batch: None,
            handshake_signing_seed: [0x4a; 32],
            handshake_batch: 7,
        })
        .verify_state(&cross_protocol),
        Err(StateEvidenceError::Policy(
            VerifierPolicyError::ProtocolVersion
        ))
    );
}

#[test]
fn a_header_from_an_unconfigured_epoch_cannot_reuse_an_authorised_key() {
    let wrong_epoch = state(StateHeaderIdentity {
        signing_seed: [0x4a; 32],
        protocol_version: 1,
        network_id: 42,
        epoch: 3,
        batch_number: 7,
    });
    assert_eq!(
        verifier(PolicyIdentity {
            signing_seed: [0x4a; 32],
            protocol_version: 1,
            network_id: 42,
            epoch: 2,
            first_batch: 7,
            last_batch: 7,
            revoked_at_batch: None,
            handshake_signing_seed: [0x4a; 32],
            handshake_batch: 7,
        })
        .verify_state(&wrong_epoch),
        Err(StateEvidenceError::Policy(VerifierPolicyError::Epoch))
    );
}

#[test]
fn revoked_and_out_of_range_keys_cannot_issue_evidence() {
    let raw = state(StateHeaderIdentity {
        signing_seed: [0x4a; 32],
        protocol_version: 1,
        network_id: 42,
        epoch: 2,
        batch_number: 7,
    });
    assert_eq!(
        verifier(PolicyIdentity {
            signing_seed: [0x4a; 32],
            protocol_version: 1,
            network_id: 42,
            epoch: 2,
            first_batch: 7,
            last_batch: 7,
            revoked_at_batch: Some(7),
            handshake_signing_seed: [0x3a; 32],
            handshake_batch: 7,
        })
        .verify_state(&raw),
        Err(StateEvidenceError::Policy(VerifierPolicyError::Revoked))
    );
    assert_eq!(
        verifier(PolicyIdentity {
            signing_seed: [0x4a; 32],
            protocol_version: 1,
            network_id: 42,
            epoch: 2,
            first_batch: 8,
            last_batch: 9,
            revoked_at_batch: None,
            handshake_signing_seed: [0x3a; 32],
            handshake_batch: 7,
        })
        .verify_state(&raw),
        Err(StateEvidenceError::Policy(
            VerifierPolicyError::BatchRange
        ))
    );
}

#[test]
fn receipt_batch_id_must_equal_the_independent_signed_header_digest() {
    let raw = support::raw_receipt([0x71; 32], 0, 25);
    assert_eq!(
        support::evidence_verifier().verify_receipt(&raw),
        Err(ReceiptEvidenceError::BatchIdentity)
    );
}
