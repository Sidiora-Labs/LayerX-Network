use std::time::Duration;

use layerx_agent_api::idempotency::{BodyDigest, IdempotentMutation, Key};
use layerx_agent_api::identity::{
    AgentDid, AuthorityRef, ClientId, ExplicitSet, PolicyVersion, SessionContext, SessionOpen,
    TenantId,
};
use layerx_agent_api::prepare::{CanonicalBytes, IdempotencyRef};
use layerx_agent_api::read::{
    AccountRef, BalanceValue, BatchRef, CheckpointRef, Freshness, RelativeTo, VerifiedRead,
};
use layerx_agent_api::track::{EvidenceRef, SubmissionRef, TrackedSubmission};
use layerx_agent_api::verify::Level;
use layerx_agent_api::{Amount, ContractVersion, Sequence, SubmissionState, TimestampSeconds};
use layerx_client::client::{ClientConfig, ReconnectPolicy};
use layerx_client::lni::handshake::HandshakeConfig;
use layerx_client::lni::schema::Version;
use layerx_client::lni::transport::Limits;
use layerx_sdk::approval::{
    ApprovalApproveRequest, ApprovalContractError, ApprovalDecisionOutcome, ApprovalEventKind,
    ApprovalGetRequest, ApprovalId, ApprovalListRequest, ApprovalRejectRequest, ApprovalState,
    DecisionKey, CONTRACT_INTRODUCED, ENFORCEMENT_NOTICE,
};
use layerx_sdk::{Client, Deployment, Operation, SdkError, SubmissionOutcome, GUARANTEES};
use layerx_types::result::ResultCode;

fn direct_config() -> ClientConfig {
    ClientConfig {
        endpoint: "/tmp/layerx-node.sock".into(),
        handshake: HandshakeConfig {
            built_interface_version: Version::V1_0,
            expected_protocol_version: 1,
            expected_network_id: 7,
        },
        limits: Limits {
            maximum_frame_bytes: 1_048_576,
            maximum_connections: 1,
            maximum_streams: 16,
            maximum_queued_bytes: 2_097_152,
            deadline: Duration::from_secs(5),
        },
        reconnect: ReconnectPolicy {
            maximum_attempts: 3,
            base_delay: Duration::from_millis(1),
            maximum_delay: Duration::from_millis(5),
            jitter_percent: 10,
        },
    }
}

fn context() -> SessionContext {
    SessionContext::new(
        TenantId::new("tenant-a").unwrap_or_else(|error| panic!("tenant: {error:?}")),
        AgentDid::new("did:layerx:agent-a").unwrap_or_else(|error| panic!("agent: {error:?}")),
        AuthorityRef::new("authority-a").unwrap_or_else(|error| panic!("authority: {error:?}")),
        ExplicitSet::allow(vec![]),
        TimestampSeconds(100),
        ClientId::new("sdk").unwrap_or_else(|error| panic!("client: {error:?}")),
        PolicyVersion::new("policy-v1").unwrap_or_else(|error| panic!("policy: {error:?}")),
    )
    .unwrap_or_else(|error| panic!("context: {error:?}"))
}

fn mutation<T>(operation: T) -> IdempotentMutation<T> {
    IdempotentMutation {
        request_id: layerx_agent_api::error::RequestId(7),
        key: Key::new([9; 32]).unwrap_or_else(|error| panic!("key: {error:?}")),
        body_digest: BodyDigest([8; 32]),
        operation,
    }
}

fn freshness() -> Freshness {
    Freshness {
        chain_head: Sequence(20),
        latest_sealed_batch: BatchRef::new("batch-19")
            .unwrap_or_else(|error| panic!("batch: {error:?}")),
        latest_finalised_checkpoint: CheckpointRef::new("checkpoint-18")
            .unwrap_or_else(|error| panic!("checkpoint: {error:?}")),
        value_sequence: Sequence(17),
        relative_to: RelativeTo::Batch(
            BatchRef::new("batch-19").unwrap_or_else(|error| panic!("batch: {error:?}")),
        ),
    }
}

#[test]
fn daemon_and_direct_node_shapes_publish_identical_guarantees_and_calls() {
    let daemon = Client::daemon(
        "/run/layerx/agentd.sock",
        ContractVersion { major: 1, minor: 2 },
    )
    .unwrap_or_else(|error| panic!("daemon: {error:?}"));
    let direct =
        Client::direct_node(direct_config()).unwrap_or_else(|error| panic!("direct: {error:?}"));
    assert_eq!(daemon.deployment(), Deployment::Daemon);
    assert_eq!(direct.deployment(), Deployment::DirectNode);
    assert_eq!(daemon.guarantees(), GUARANTEES);
    assert_eq!(daemon.guarantees(), direct.guarantees());

    let request = mutation(SessionOpen(context()));
    let daemon_call = daemon.session_open(request.clone());
    let direct_call = direct.session_open(request);
    assert_eq!(daemon_call.operation(), Operation::SessionOpen);
    assert_eq!(daemon_call.operation(), direct_call.operation());
    assert_eq!(daemon_call.request(), direct_call.request());
    assert_eq!(daemon_call.request().key.bytes(), [9; 32]);
    assert!(daemon_call.operation().mutating());
}

#[test]
fn operation_catalogue_covers_the_complete_contract_surface() {
    assert_eq!(Operation::ALL.len(), 46);
    let names: std::collections::BTreeSet<_> = Operation::ALL
        .iter()
        .map(|operation| operation.name())
        .collect();
    for required in [
        "agent.register",
        "session.open",
        "capability.create",
        "budget.create",
        "project",
        "prepare",
        "sign",
        "submit",
        "track",
        "read.balance",
        "read.proof_bundle",
        "availability.fetch",
        "subscription.create",
        "approval.list",
        "approval.get",
        "approval.approve",
        "approval.reject",
        "program.discover",
        "program.interface",
        "program.simulate",
        "program.call",
        "program.receipt",
        "program.activity",
    ] {
        assert!(names.contains(required), "missing SDK operation {required}");
    }
    assert!(Operation::ALL
        .iter()
        .filter(|operation| operation.mutating())
        .all(|operation| !matches!(operation, Operation::Track | Operation::Wait)));
    assert!(Operation::ProgramCall.mutating());
    assert!(!Operation::ProgramDiscover.mutating());
    assert!(!Operation::ProgramInterface.mutating());
    assert!(!Operation::ProgramSimulate.mutating());
    assert!(!Operation::ProgramReceipt.mutating());
    assert!(!Operation::ProgramActivity.mutating());
}

#[test]
fn approval_module_matches_contract_1_1_operations_events_and_outcomes() {
    assert_eq!(CONTRACT_INTRODUCED, (1, 1));
    assert!(ENFORCEMENT_NOTICE.contains("confers no protocol authority"));
    assert_eq!(
        ApprovalState::ALL
            .iter()
            .map(|value| value.name())
            .collect::<Vec<_>>(),
        ["Held", "Granted", "Rejected", "Expired", "Defective"]
    );
    assert_eq!(
        ApprovalDecisionOutcome::ALL
            .iter()
            .map(|value| value.name())
            .collect::<Vec<_>>(),
        [
            "Granted",
            "Rejected",
            "Expired",
            "Defective",
            "AlreadyDecided",
            "Conflict",
        ]
    );
    assert_eq!(
        ApprovalEventKind::ALL
            .iter()
            .map(|value| value.name())
            .collect::<Vec<_>>(),
        ["Created", "Granted", "Rejected", "Expired", "Defective"]
    );

    let tenant = TenantId::new("tenant-a").unwrap_or_else(|error| panic!("tenant: {error:?}"));
    let approval_id = ApprovalId::new([7; 32]);
    let key = DecisionKey::new("decision-7").unwrap_or_else(|error| panic!("key: {error:?}"));
    let client = Client::daemon(
        "/run/layerx/agentd.sock",
        ContractVersion { major: 1, minor: 1 },
    )
    .unwrap_or_else(|error| panic!("daemon: {error:?}"));
    assert_eq!(
        client
            .approval_list(
                ApprovalListRequest::new(tenant.clone(), None, 50)
                    .unwrap_or_else(|error| panic!("list: {error:?}")),
            )
            .operation(),
        Operation::ApprovalList
    );
    assert_eq!(
        client
            .approval_get(ApprovalGetRequest {
                tenant: tenant.clone(),
                approval_id,
            })
            .operation(),
        Operation::ApprovalGet
    );
    assert_eq!(
        client
            .approval_approve(ApprovalApproveRequest {
                tenant: tenant.clone(),
                approval_id,
                idempotency_key: key.clone(),
            })
            .operation(),
        Operation::ApprovalApprove
    );
    assert_eq!(
        client
            .approval_reject(
                ApprovalRejectRequest::new(tenant, approval_id, key, "not expected")
                    .unwrap_or_else(|error| panic!("reject: {error:?}")),
            )
            .operation(),
        Operation::ApprovalReject
    );
    assert!(Operation::ApprovalApprove.mutating());
    assert!(Operation::ApprovalReject.mutating());
    assert_eq!(
        ApprovalListRequest::new(
            TenantId::new("tenant-a").unwrap_or_else(|error| panic!("tenant: {error:?}")),
            None,
            0,
        ),
        Err(ApprovalContractError::InvalidPageLimit)
    );
}

#[test]
fn reads_never_present_unverified_or_below_requested_values() {
    let balance = BalanceValue {
        account: AccountRef::new("account-a").unwrap_or_else(|error| panic!("account: {error:?}")),
        asset: layerx_agent_api::identity::Asset::new("LXP")
            .unwrap_or_else(|error| panic!("asset: {error:?}")),
        amount: Amount(9_007_199_254_740_993),
        canonical_state: CanonicalBytes::new(b"core-state".to_vec())
            .unwrap_or_else(|error| panic!("state: {error:?}")),
    };
    let unverified = VerifiedRead::new(balance.clone(), Level::Unverified, freshness());
    assert_eq!(
        Client::accept_verified_read(Level::SequencerSigned, unverified),
        Err(SdkError::UnverifiedRead)
    );
    let below = VerifiedRead::new(balance.clone(), Level::SequencerSigned, freshness());
    assert_eq!(
        Client::accept_verified_read(Level::StateProven, below),
        Err(SdkError::VerificationBelowRequested {
            requested: Level::StateProven,
            achieved: Level::SequencerSigned,
        })
    );
    let proven = VerifiedRead::new(balance, Level::StateProven, freshness());
    assert!(Client::accept_verified_read(Level::StateProven, proven).is_ok());
}

fn tracked(state: SubmissionState, evidence: Vec<EvidenceRef>, level: Level) -> TrackedSubmission {
    TrackedSubmission {
        submission_ref: SubmissionRef::new("submission-a")
            .unwrap_or_else(|error| panic!("submission: {error:?}")),
        state,
        evidence,
        verification_level: level,
        transitions: Vec::new(),
    }
}

#[test]
fn unknown_and_future_protocol_codes_remain_lossless() {
    let unknown = Client::submission_outcome(tracked(
        SubmissionState::Unknown,
        Vec::new(),
        Level::Unverified,
    ));
    assert!(matches!(unknown, Ok(SubmissionOutcome::Unknown(_))));

    let future = ResultCode::from_raw(-77_777);
    let failed = Client::submission_outcome(tracked(
        SubmissionState::Failed { result: future },
        Vec::new(),
        Level::Unverified,
    ));
    assert!(matches!(
        failed,
        Ok(SubmissionOutcome::Failed { result, .. }) if result.raw() == -77_777
    ));

    let executed_without_proof = Client::submission_outcome(tracked(
        SubmissionState::Executed {
            receipt_ref: layerx_agent_api::track::ReceiptRef::new("receipt-a")
                .unwrap_or_else(|error| panic!("receipt: {error:?}")),
        },
        Vec::new(),
        Level::Unverified,
    ));
    assert_eq!(
        executed_without_proof,
        Err(SdkError::ExecutedWithoutEvidence)
    );
}

#[test]
fn signer_surface_contains_no_key_export_and_offline_verifier_is_real() {
    let source = include_str!("../src/lib.rs");
    let signer_start = source
        .find("pub mod signer")
        .unwrap_or_else(|| panic!("signer module missing"));
    let signer_end = source[signer_start..]
        .find("pub enum Deployment")
        .map_or_else(
            || panic!("signer module end missing"),
            |offset| signer_start + offset,
        );
    let signer_source = &source[signer_start..signer_end];
    assert!(!signer_source.contains("private_key"));
    assert!(!signer_source.contains("secret"));
    assert!(signer_source.contains("trait ExternalSigner"));

    let empty = layerx_proof::export::OfflineExport {
        receipts: Vec::new(),
        inclusions: Vec::new(),
        checkpoints: Vec::new(),
        derived_aggregates: Vec::new(),
    };
    let report = Client::verify_offline(
        &empty,
        layerx_proof::checkpoint::SettlementDomain::new(31_337, [0x55; 20]),
    )
        .unwrap_or_else(|error| panic!("empty export has no false fact: {error:?}"));
    assert_eq!(report.verified_receipts, 0);
    assert!(!report.derived_aggregates_are_protocol_facts);

    let key = IdempotencyRef::new("caller-owned-key")
        .unwrap_or_else(|error| panic!("idempotency: {error:?}"));
    assert_eq!(key.as_str(), "caller-owned-key");
}
