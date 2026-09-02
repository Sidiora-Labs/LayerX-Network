use std::collections::BTreeSet;
use std::fs;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

use layerx_agent_api::prepare::CanonicalBytes;
use layerx_agent_api::track::{
    EvidenceRef, ReceiptRef, SubmissionRef, SubmissionState, TrackedSubmission,
};
use layerx_agent_api::verify::Level;
use layerx_agentd::budget::{BudgetLimiter, LimitConfig, LimitId, LimitScope};
use layerx_agentd::capability::{Capability, CapabilityDimensions, CapabilityId, RateCeiling};
use layerx_agentd::identity::{
    register, CoreIdentity, IdentityError, IdentityResolver, ProtocolAuthority,
};
use layerx_agentd::prepare::PreparationLifecycle;
use layerx_agentd::session::{open, OpenRequest, SessionId, SessionRegistry};
use layerx_agentd::session_control::SessionControl;
use layerx_agentd::store::{Store, TenantId};
use layerx_mcp::server::{Server, ServerError};
use layerx_mcp::tools::write::{
    execute, track, FailureClass, StageFailure, VerifiedReceipt, WriteOutcome, WriteStage,
    WriteToolError, WriteTranscript, ORDINARY_WRITE_STAGES,
};
use layerx_types::ids::Did;
use layerx_types::result::ResultCode;
use layerx_types::verify::VerificationLevel;

static NEXT_DIRECTORY: AtomicU64 = AtomicU64::new(1);

struct BoundaryIdentity(CoreIdentity);

impl IdentityResolver for BoundaryIdentity {
    fn resolve(&mut self, _did: &Did) -> Result<Option<CoreIdentity>, IdentityError> {
        Ok(Some(self.0.clone()))
    }
}

fn directory(label: &str) -> std::path::PathBuf {
    let sequence = NEXT_DIRECTORY.fetch_add(1, Ordering::Relaxed);
    std::env::temp_dir().join(format!(
        "layerx-mcp-write-{label}-{}-{sequence}",
        std::process::id()
    ))
}

fn server(root: &std::path::Path) -> (Server, SessionControl, TenantId) {
    let tenant = TenantId::new("tenant-a").unwrap_or_else(|error| panic!("tenant: {error}"));
    let capability = Capability::new(
        CapabilityId([9; 32]),
        tenant.clone(),
        CapabilityDimensions {
            activity_types: BTreeSet::from([7]),
            counterparties: BTreeSet::from([[2; 32]]),
            assets: BTreeSet::from([[3; 32]]),
            amount_ceiling: 100,
            rate_ceiling: RateCeiling {
                maximum_uses: 2,
                window_sequences: 10,
            },
            purposes: BTreeSet::from(["service-payment".to_owned()]),
            expiry_sequence: 200,
        },
    )
    .unwrap_or_else(|error| panic!("capability: {error:?}"));
    let mut store =
        Store::open(root.join("store")).unwrap_or_else(|error| panic!("store: {error}"));
    capability
        .persist(&mut store)
        .unwrap_or_else(|error| panic!("capability persist: {error:?}"));
    let agent = Did::new(b"did:layerx:model").unwrap_or_else(|error| panic!("DID: {error:?}"));
    let mut boundary = BoundaryIdentity(CoreIdentity {
        canonical_bytes: b"model-identity".to_vec(),
        head_sequence: 10,
        revocation_sequence: 1,
        verification_level: VerificationLevel::STATE_PROVEN,
        frozen: false,
        authorities: vec![ProtocolAuthority::CapabilityGrant(capability.id.0)],
    });
    let identity = register(&mut store, tenant.clone(), agent.clone(), &mut boundary)
        .unwrap_or_else(|error| panic!("identity: {error:?}"));
    let mut sessions = SessionRegistry::default();
    let token = open(
        &mut store,
        &mut sessions,
        &identity,
        OpenRequest {
            session_id: SessionId([7; 32]),
            token_id: [8; 32],
            tenant,
            agent,
            authority: ProtocolAuthority::CapabilityGrant(capability.id.0),
            permitted_activity_types: BTreeSet::from([7]),
            scopes: BTreeSet::from(["write:submit".to_owned(), "write:track".to_owned()]),
            expiry_sequence: 150,
            opening_client: "mcp".to_owned(),
            policy_version: "policy-v1".to_owned(),
        },
        50,
    )
    .unwrap_or_else(|error| panic!("session: {error:?}"));
    let budgets = BudgetLimiter::new(vec![LimitConfig {
        id: LimitId([9; 16]),
        name: "mcp-limit".to_owned(),
        scope: LimitScope::Tenant([1; 32]),
        ceiling: 1_000,
        consumed: 0,
    }])
    .unwrap_or_else(|error| panic!("limiter: {error:?}"));
    let control = SessionControl::new(
        Arc::new(Mutex::new(store)),
        sessions,
        Arc::new(PreparationLifecycle::default()),
        Arc::new(budgets),
    );
    let bound_tenant = token.tenant().clone();
    let server = Server::bind(control.clone(), token.credential(), capability.id, 50, root)
        .unwrap_or_else(|error| panic!("bind: {error:?}"));
    (server, control, bound_tenant)
}

fn submission(state: SubmissionState, level: Level) -> TrackedSubmission {
    TrackedSubmission {
        submission_ref: SubmissionRef::new("submission-1")
            .unwrap_or_else(|error| panic!("submission ref: {error:?}")),
        state,
        evidence: vec![EvidenceRef {
            kind: "receipt".to_owned(),
            digest: [4; 32],
        }],
        verification_level: level,
        transitions: Vec::new(),
    }
}

fn receipt() -> VerifiedReceipt {
    VerifiedReceipt {
        receipt_ref: ReceiptRef::new("receipt-1")
            .unwrap_or_else(|error| panic!("receipt ref: {error:?}")),
        canonical_receipt: CanonicalBytes::new(b"exact-core-receipt".to_vec())
            .unwrap_or_else(|error| panic!("receipt bytes: {error:?}")),
        verification_level: Level::BatchIncluded,
        evidence_ids: vec![[4; 32]],
    }
}

#[test]
fn refused_policy_is_a_typed_stage_error_and_is_audited() {
    let root = directory("policy");
    let (mut server, _control, _tenant) = server(&root);
    let failure = StageFailure {
        stage: WriteStage::Policy,
        class: FailureClass::Refused,
        protocol_result_code: None,
    };
    let result = execute(
        &mut server,
        50,
        b"validated-request".to_vec(),
        |_| WriteTranscript {
            stages: ORDINARY_WRITE_STAGES[..3].to_vec(),
            submission: Err(failure),
            receipt: None,
        },
        0,
    );
    assert!(matches!(result, Err(WriteToolError::Stage(found)) if found == failure));
    assert_eq!(server.audit_entries(), 2);
    let _ = fs::remove_dir_all(root);
}

#[test]
fn unknown_is_never_reported_as_success_and_late_receipt_resolves_through_track() {
    let root = directory("unknown");
    let (mut server, _control, _tenant) = server(&root);
    let unknown = execute(
        &mut server,
        50,
        b"validated-request".to_vec(),
        |_| WriteTranscript {
            stages: ORDINARY_WRITE_STAGES.to_vec(),
            submission: Ok(submission(SubmissionState::Unknown, Level::SequencerSigned)),
            receipt: None,
        },
        4_000,
    )
    .unwrap_or_else(|error| panic!("unknown: {error:?}"));
    assert!(matches!(
        unknown,
        WriteOutcome::Unknown { age_ms: 4_000, .. }
    ));

    let executed = track(
        &mut server,
        50,
        b"submission-1".to_vec(),
        |_| WriteTranscript {
            stages: vec![WriteStage::Track],
            submission: Ok(submission(
                SubmissionState::Executed {
                    receipt_ref: ReceiptRef::new("receipt-1")
                        .unwrap_or_else(|error| panic!("receipt ref: {error:?}")),
                },
                Level::BatchIncluded,
            )),
            receipt: Some(receipt()),
        },
        9_000,
    )
    .unwrap_or_else(|error| panic!("late receipt: {error:?}"));
    assert!(matches!(executed, WriteOutcome::Executed { .. }));
    assert_eq!(server.audit_entries(), 4);
    let _ = fs::remove_dir_all(root);
}

#[test]
fn terminal_protocol_rejection_names_track_stage_and_exact_result_code() {
    let root = directory("rejection");
    let (mut server, _control, _tenant) = server(&root);
    let result_code = ResultCode::from_raw(-400);
    let result = execute(
        &mut server,
        50,
        b"validated-request".to_vec(),
        |_| WriteTranscript {
            stages: ORDINARY_WRITE_STAGES.to_vec(),
            submission: Ok(submission(
                SubmissionState::Failed {
                    result: result_code,
                },
                Level::SequencerSigned,
            )),
            receipt: None,
        },
        0,
    );
    assert!(matches!(
        result,
        Err(WriteToolError::Stage(StageFailure {
            stage: WriteStage::Track,
            class: FailureClass::Protocol,
            protocol_result_code: Some(found),
        })) if found == result_code
    ));
    let _ = fs::remove_dir_all(root);
}

#[test]
fn executed_without_matching_verified_receipt_is_never_success() {
    let root = directory("receipt-gate");
    let (mut server, _control, _tenant) = server(&root);
    let result = execute(
        &mut server,
        50,
        b"validated-request".to_vec(),
        |_| WriteTranscript {
            stages: ORDINARY_WRITE_STAGES.to_vec(),
            submission: Ok(submission(
                SubmissionState::Executed {
                    receipt_ref: ReceiptRef::new("receipt-1")
                        .unwrap_or_else(|error| panic!("receipt ref: {error:?}")),
                },
                Level::BatchIncluded,
            )),
            receipt: None,
        },
        0,
    );
    assert!(matches!(
        result,
        Err(WriteToolError::SuccessWithoutVerifiedReceipt)
    ));
    let _ = fs::remove_dir_all(root);
}

#[test]
fn closed_session_refuses_submit_and_track_with_a_typed_revocation_before_any_transcript() {
    let root = directory("revoked");
    let (mut server, control, tenant) = server(&root);
    control
        .close(&tenant, SessionId([7; 32]), 50)
        .unwrap_or_else(|error| panic!("close: {error:?}"));
    let mut executed = false;
    let result = execute(
        &mut server,
        50,
        b"validated-request".to_vec(),
        |_| {
            executed = true;
            WriteTranscript {
                stages: ORDINARY_WRITE_STAGES.to_vec(),
                submission: Ok(submission(SubmissionState::Unknown, Level::SequencerSigned)),
                receipt: None,
            }
        },
        0,
    );
    assert!(matches!(
        result,
        Err(WriteToolError::Server(ServerError::RevokedSession))
    ));
    let tracked = track(
        &mut server,
        50,
        b"submission-1".to_vec(),
        |_| {
            executed = true;
            WriteTranscript {
                stages: vec![WriteStage::Track],
                submission: Ok(submission(
                    SubmissionState::Executed {
                        receipt_ref: ReceiptRef::new("receipt-1")
                            .unwrap_or_else(|error| panic!("receipt ref: {error:?}")),
                    },
                    Level::BatchIncluded,
                )),
                receipt: Some(receipt()),
            }
        },
        0,
    );
    assert!(matches!(
        tracked,
        Err(WriteToolError::Server(ServerError::RevokedSession))
    ));
    assert!(!executed);
    assert_eq!(server.audit_entries(), 2);
    let _ = fs::remove_dir_all(root);
}
