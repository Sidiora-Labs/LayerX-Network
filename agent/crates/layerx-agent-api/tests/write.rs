use layerx_agent_api::identity::{ActivityType, AgentDid, Asset, AuthorityRef, ExplicitSet};
use layerx_agent_api::prepare::{
    CanonicalBytes, Disclosure, IdempotencyRef, PreparationRef, Prepared, SigningPreimage,
};
use layerx_agent_api::track::{
    ReceiptRef, SubmissionRef, SubmissionState, TrackedSubmission, WaitRequest, WaitResult,
};
use layerx_agent_api::verify::Level;
use layerx_agent_api::{Amount, TimestampSeconds};

const SCHEMA: &str = include_str!("../../../schema/agent-api/write.kvx");

fn required<T>(result: Result<T, layerx_agent_api::identity::ContractError>) -> T {
    result.unwrap_or_else(|error| panic!("valid contract value: {error:?}"))
}

#[test]
fn preparation_binds_exact_bytes_preimage_disclosure_and_expiry() {
    let digest = [7; 32];
    let prepared = Prepared {
        preparation_ref: required(PreparationRef::new("prep-7")),
        unsigned_canonical_bytes: required(CanonicalBytes::new(vec![1, 2, 3])),
        signing_preimage: required(SigningPreimage::new(vec![4, 5, 6])),
        disclosure: Disclosure {
            canonical_digest: digest,
            activity_type: ActivityType(9),
            actor: required(AgentDid::new("did:layerx:actor")),
            authority: required(AuthorityRef::new("owner-1")),
            counterparties: ExplicitSet::allow(vec![required(AgentDid::new("did:layerx:payee"))]),
            amounts: ExplicitSet::deny_all(),
            asset: required(Asset::new("LXP")),
            fee_limit: Amount(3),
            expiry: TimestampSeconds(100),
            idempotency_key: required(IdempotencyRef::new("idem-7")),
        },
        expiry: TimestampSeconds(100),
    };
    assert_eq!(prepared.disclosure.canonical_digest, digest);
    assert_eq!(prepared.expiry, prepared.disclosure.expiry);
    assert!(SCHEMA.contains("origin = \"decoded_from_unsigned_canonical_bytes\""));
}

#[test]
fn state_machine_is_exact_and_unknown_is_a_value() {
    let states = [
        SubmissionState::Prepared,
        SubmissionState::Signed,
        SubmissionState::Queued,
        SubmissionState::Submitted,
        SubmissionState::Acknowledged,
        SubmissionState::Unknown,
        SubmissionState::Executed {
            receipt_ref: required(ReceiptRef::new("receipt-1")),
        },
        SubmissionState::Failed {
            result: layerx_types::result::KnownResult::BadSignature.into(),
        },
        SubmissionState::Expired,
    ];
    assert_eq!(
        states.iter().map(SubmissionState::name).collect::<Vec<_>>(),
        [
            "prepared",
            "signed",
            "queued",
            "submitted",
            "acknowledged",
            "unknown",
            "executed",
            "failed",
            "expired",
        ]
    );
    assert!(SCHEMA.contains("Unknown.semantics = \"first_class_terminal_pending\""));
}

#[test]
fn executed_state_cannot_exist_without_a_receipt_reference() {
    assert!(ReceiptRef::new("").is_err());
    let executed = SubmissionState::Executed {
        receipt_ref: required(ReceiptRef::new("receipt-9")),
    };
    let SubmissionState::Executed { receipt_ref } = executed else {
        panic!("executed variant expected");
    };
    assert_eq!(receipt_ref.as_str(), "receipt-9");
    assert!(SCHEMA.contains("Executed.required = [\"receipt_ref\"]"));
}

#[test]
fn wait_returns_actual_level_even_when_deadline_elapsed() {
    let submission_ref = required(SubmissionRef::new("submission-1"));
    let request = WaitRequest {
        submission_ref: submission_ref.clone(),
        requested_verification_level: Level::CheckpointFinalised,
        deadline: TimestampSeconds(50),
    };
    let tracked = TrackedSubmission {
        submission_ref,
        state: SubmissionState::Acknowledged,
        evidence: Vec::new(),
        verification_level: Level::SequencerSigned,
        transitions: Vec::new(),
    };
    let result = WaitResult {
        submission: tracked,
        actual_verification_level: Level::SequencerSigned,
        deadline_elapsed: true,
    };
    assert!(result.deadline_elapsed);
    assert!(result.actual_verification_level < request.requested_verification_level);
}
