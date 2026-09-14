use std::fmt::Debug;

use ed25519_dalek::SigningKey;
use layerx_human_service::agents::{AgentFailure, ProtocolEvidence};
use layerx_human_service::server::agent_creation::ProductionAgentCreation;
use layerx_intents::canonical;
use layerx_proof::receipt::{verify_sequencer_signature, AuthorizedBatch};
use layerx_types::payload::{ActivityType, ModuleId, ModuleRegistration, ModuleRegistry};
use layerx_types::verify::VerificationLevel;

fn checked<T, E: Debug>(value: Result<T, E>) -> T {
    value.unwrap_or_else(|error| panic!("original native lifecycle evidence: {error:?}"))
}

fn evidence() -> ProtocolEvidence {
    let signed = include_bytes!(
        "../../../../tests/fixtures/authority/native-sessions/authentication/grant.activity"
    );
    let receipt = include_bytes!(
        "../../../../tests/fixtures/authority/native-sessions/authentication/grant.receipt"
    );
    let sequencer = *include_bytes!("../../../../tests/fixtures/authority/native-sessions/authentication/grant.sequencer-public");
    let kind = checked(ActivityType::new(ModuleId::Governance, 5));
    let registry = checked(ModuleRegistry::new(&[checked(ModuleRegistration::new(
        ModuleId::Governance,
        &[kind],
    ))]));
    let original = checked(canonical::decode_signed_activity(signed, &registry));
    let authenticated = checked(verify_sequencer_signature(receipt, sequencer));
    let protocol = authenticated
        .protocol()
        .unwrap_or_else(|| panic!("missing native receipt"));
    let public = SigningKey::from_bytes(&[0x11; 32])
        .verifying_key()
        .to_bytes();
    let mut actor = String::from("did:layerx:");
    for byte in public {
        use std::fmt::Write as _;
        checked(write!(&mut actor, "{byte:02x}"));
    }
    ProtocolEvidence {
        signed_activity: signed.to_vec(),
        owner_public_key: public,
        actor: actor.into_bytes(),
        network_id: 77,
        action_key: original.idempotency_key(),
        activity_id: *include_bytes!(
            "../../../../tests/fixtures/authority/native-sessions/authentication/grant.activity-id"
        ),
        receipt_bytes: receipt.to_vec(),
        authorized_batch: AuthorizedBatch::new(
            protocol.batch_id(),
            protocol.asset(),
            protocol.previous_state_root(),
            protocol.resulting_state_root(),
            sequencer,
        ),
        verification_level: VerificationLevel::SEQUENCER_SIGNED,
    }
}

#[test]
fn native_lifecycle_binds_distinct_action_and_activity_without_inventing_finality() {
    let evidence = evidence();
    let kind = checked(ActivityType::new(ModuleId::Governance, 5));
    assert_ne!(evidence.action_key, evidence.activity_id);
    let original = checked(evidence.bound_activity(kind));
    assert_eq!(original.idempotency_key(), evidence.action_key);
    let verified = checked(evidence.verify_outcome(kind));
    assert_eq!(verified.level(), VerificationLevel::SEQUENCER_SIGNED);
    assert_eq!(
        verified
            .receipt()
            .protocol()
            .unwrap_or_else(|| panic!("missing protocol"))
            .operation(),
        0
    );
    assert!(matches!(
        ProductionAgentCreation::finalization_evidence(&evidence, ModuleId::Governance, 5, 1000),
        Err(AgentFailure::Refused(
            "lifecycle receipt is not checkpoint-finalized"
        ))
    ));
    let mutations: &[fn(&mut ProtocolEvidence)] = &[
        |value| value.activity_id = value.action_key,
        |value| value.action_key = value.activity_id,
        |value| value.actor[0] ^= 1,
        |value| value.owner_public_key[0] ^= 1,
        |value| value.network_id += 1,
        |value| value.signed_activity[12] ^= 1,
    ];
    for mutate in mutations {
        let mut changed = evidence.clone();
        mutate(&mut changed);
        assert!(changed.bound_activity(kind).is_err());
        assert!(changed.verify_outcome(kind).is_err());
    }
    assert!(evidence
        .bound_activity(checked(ActivityType::new(ModuleId::Governance, 6)))
        .is_err());
}
