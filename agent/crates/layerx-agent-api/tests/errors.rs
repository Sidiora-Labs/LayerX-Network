use std::collections::BTreeSet;

use layerx_agent_api::error::{ApiError, ErrorClass, RequestId};
use layerx_agent_api::idempotency::{
    classify_repeat, BodyDigest, IdempotencyOutcome, IdempotentMutation, Key,
};
use layerx_agent_api::verify::{ApiSuccess, Level, VerificationStatus};
use layerx_types::error::ErrorClass as LayerErrorClass;
use layerx_types::verify::VerificationLevel;

const SCHEMA: &str = include_str!("../../../schema/agent-api/errors.kvx");

#[test]
fn api_error_taxonomy_is_total_and_unavailable_capability_is_distinct() {
    let unique: BTreeSet<ErrorClass> = ErrorClass::ALL.iter().copied().collect();
    assert_eq!(unique.len(), ErrorClass::ALL.len());
    let layer_classes = [
        LayerErrorClass::TransportFailure,
        LayerErrorClass::Deadline,
        LayerErrorClass::ProtocolIncompatibility,
        LayerErrorClass::UnavailableCapability,
        LayerErrorClass::CoreRejection,
        LayerErrorClass::VerificationFailure,
        LayerErrorClass::PolicyRefusal,
        LayerErrorClass::CapabilityRefusal,
        LayerErrorClass::BudgetRefusal,
        LayerErrorClass::RateLimit,
        LayerErrorClass::InternalFault,
    ];
    for class in layer_classes {
        assert!(unique.contains(&class.into()));
    }

    let unavailable = ApiError::unavailable_capability(RequestId(9), "availability_fetch")
        .unwrap_or_else(|error| panic!("unavailable error: {error:?}"));
    assert_eq!(unavailable.class, ErrorClass::UnavailableCapability);
    assert_eq!(unavailable.protocol_result_code, None);
    assert!(unavailable.reason.as_str().contains("availability_fetch"));
    assert_ne!(unavailable.class, ErrorClass::CapabilityRefusal);
}

#[test]
fn every_declared_mutation_has_the_idempotent_envelope() {
    let mut mutation_count = 0_u32;
    let mut in_mutation = false;
    for raw in SCHEMA.lines() {
        let line = raw.trim();
        if line.starts_with("[mutation.") {
            mutation_count += 1;
            in_mutation = true;
        } else if line.starts_with('[') {
            in_mutation = false;
        } else if in_mutation && line.starts_with("envelope") {
            assert_eq!(line, "envelope = \"IdempotentMutation\"");
            in_mutation = false;
        }
    }
    assert_eq!(mutation_count, 18);

    let key = Key::new([7; 32]).unwrap_or_else(|error| panic!("key: {error:?}"));
    let wrapped = IdempotentMutation {
        request_id: RequestId(5),
        key,
        body_digest: BodyDigest([1; 32]),
        operation: "submit",
    };
    assert_eq!(wrapped.key.bytes(), [7; 32]);
    assert!(Key::new([0; 32]).is_err());
}

#[test]
fn repeats_return_original_and_changed_bodies_conflict() {
    let original = BodyDigest([1; 32]);
    assert_eq!(
        classify_repeat(original, original, "original-result"),
        IdempotencyOutcome::RepeatedOriginal("original-result")
    );
    assert_eq!(
        classify_repeat(original, BodyDigest([2; 32]), "must-not-replace"),
        IdempotencyOutcome::Conflict {
            original_body: original,
            repeated_body: BodyDigest([2; 32]),
        }
    );
    assert!(SCHEMA.contains("repeat_same_body = \"return_original_result\""));
    assert!(SCHEMA.contains("repeat_different_body = \"IdempotencyConflict\""));
}

#[test]
fn verification_lattice_and_success_status_are_explicit() {
    let levels = [
        Level::Unverified,
        Level::SequencerSigned,
        Level::BatchIncluded,
        Level::StateProven,
        Level::CheckpointFinalised,
        Level::SettlementAnchored,
    ];
    assert!(levels.windows(2).all(|pair| pair[0] < pair[1]));
    assert_eq!(
        Level::from(VerificationLevel::UNVERIFIED),
        Level::Unverified
    );
    assert_eq!(
        Level::from(VerificationLevel::SETTLEMENT_ANCHORED),
        Level::SettlementAnchored
    );

    let success = ApiSuccess {
        request_id: RequestId(12),
        value: "balance",
        verification_status: VerificationStatus::Achieved(Level::StateProven),
    };
    assert_eq!(
        success.verification_status,
        VerificationStatus::Achieved(Level::StateProven)
    );
    assert!(SCHEMA.contains("required = [\"request_id\",\"value\",\"verification_status\"]"));
    assert!(SCHEMA.contains("shortfall is never silently downgraded"));
}
