use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Barrier;
use std::thread;

use crate::approval::{
    ApprovalExpiry, ApprovalOutcome, ApprovalService, ApprovalSubmissionQueue, DecisionKey,
    DecisionRequest,
};
use crate::budget::{reserve, BudgetLimiter, LimitConfig, LimitId, LimitScope, ReservationRequest};
use crate::capability::CapabilityId;
use crate::policy::approval::{hold, ApprovalContext, ApprovalRegistry, ApproverId};
use crate::session::SessionId;
use crate::store::TenantId;
use layerx_agent_api::identity::{ActivityType, AgentDid, Asset, AuthorityRef, ExplicitSet};
use layerx_agent_api::prepare::{
    CanonicalBytes, Disclosure, IdempotencyRef, PreparationRef, Prepared, SigningPreimage,
};
use layerx_agent_api::{Amount, TimestampSeconds};
use layerx_types::ids::Did;
use sha2::{Digest as _, Sha256};

struct Directory(PathBuf);

impl Directory {
    fn new(label: &str) -> Self {
        let root = std::env::temp_dir().join(format!(
            "layerx-approval-ordering-{label}-{}",
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&root).unwrap_or_else(|error| panic!("directory: {error}"));
        Self(root)
    }

    fn path(&self) -> &Path {
        &self.0
    }
}

impl Drop for Directory {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn tenant() -> TenantId {
    TenantId::new("tenant-approval-semantics").unwrap_or_else(|error| panic!("tenant: {error}"))
}

fn prepared(id: u8, expiry: u64) -> Prepared {
    let bytes = format!("durable-approval-preparation-{id}").into_bytes();
    let digest: [u8; 32] = Sha256::digest(&bytes).into();
    let actor = AgentDid::new("did:layerx:approval-semantics")
        .unwrap_or_else(|error| panic!("actor: {error:?}"));
    Prepared {
        preparation_ref: PreparationRef::new(format!("prepared-{id}"))
            .unwrap_or_else(|error| panic!("preparation: {error:?}")),
        unsigned_canonical_bytes: CanonicalBytes::new(bytes)
            .unwrap_or_else(|error| panic!("bytes: {error:?}")),
        signing_preimage: SigningPreimage::new(vec![id; 32])
            .unwrap_or_else(|error| panic!("preimage: {error:?}")),
        disclosure: Disclosure {
            canonical_digest: digest,
            activity_type: ActivityType(7),
            actor,
            authority: AuthorityRef::new("session-key")
                .unwrap_or_else(|error| panic!("authority: {error:?}")),
            counterparties: ExplicitSet::deny_all(),
            amounts: ExplicitSet::deny_all(),
            asset: Asset::new("LXP").unwrap_or_else(|error| panic!("asset: {error:?}")),
            fee_limit: Amount(2),
            expiry: TimestampSeconds(expiry),
            idempotency_key: IdempotencyRef::new(format!("activity-{id}"))
                .unwrap_or_else(|error| panic!("activity key: {error:?}")),
        },
        expiry: TimestampSeconds(expiry),
    }
}

fn context(id: u8) -> ApprovalContext {
    ApprovalContext {
        tenant: tenant(),
        agent: Did::new(b"did:layerx:approval-semantics")
            .unwrap_or_else(|error| panic!("agent: {error:?}")),
        session: SessionId([2; 32]),
        capability: CapabilityId([3; 32]),
        policy_version: "policy-v3".to_owned(),
        request_id: [id; 32],
    }
}

fn limiter() -> BudgetLimiter {
    BudgetLimiter::new(vec![LimitConfig {
        id: LimitId([4; 16]),
        name: "approval-semantics".to_owned(),
        scope: LimitScope::Tenant([5; 32]),
        ceiling: 1_000,
        consumed: 0,
    }])
    .unwrap_or_else(|error| panic!("limiter: {error:?}"))
}

fn reserve_hold(limiter: &BudgetLimiter, id: u8, expiry: u64) {
    reserve(
        limiter,
        &ReservationRequest {
            id: [id; 32],
            amount: 10,
            expiry_sequence: expiry,
            current_sequence: 10,
            applicable_limits: vec![LimitId([4; 16])],
        },
    )
    .unwrap_or_else(|error| panic!("reserve: {error:?}"));
}

fn key(value: &str) -> DecisionKey {
    DecisionKey::new(value).unwrap_or_else(|error| panic!("decision key: {error:?}"))
}

fn approver(value: &str) -> ApproverId {
    ApproverId::new(value).unwrap_or_else(|error| panic!("approver: {error:?}"))
}

#[test]
fn rejection_between_grant_prepare_and_persist_conflicts_with_durable_grant() {
    let directory = Directory::new("prepare-persist");
    let registry = ApprovalRegistry::default();
    let limiter = limiter();
    let expiry =
        ApprovalExpiry::open(directory.path()).unwrap_or_else(|error| panic!("expiry: {error:?}"));
    let queue = ApprovalSubmissionQueue::default();
    let held = prepared(14, 40);
    reserve_hold(&limiter, 14, 40);
    hold(&registry, context(14), held.clone(), 10, 40)
        .unwrap_or_else(|error| panic!("hold: {error:?}"));
    let prepared_barrier = Barrier::new(2);
    let rejection_barrier = Barrier::new(2);
    let (grant, rejection) = thread::scope(|scope| {
        let grant = scope.spawn(|| {
            let pause = || {
                prepared_barrier.wait();
                rejection_barrier.wait();
            };
            let mut service = ApprovalService::new(&registry, &limiter, &expiry);
            service.after_prepare = Some(&pause);
            service
                .approve(
                    DecisionRequest {
                        tenant: &tenant(),
                        approval_id: [14; 32],
                        idempotency_key: &key("grant-14"),
                        approver: approver("human:grant"),
                        current_sequence: 11,
                    },
                    &held,
                    &queue,
                )
                .unwrap_or_else(|error| panic!("grant: {error:?}"))
        });
        let rejection = scope.spawn(|| {
            prepared_barrier.wait();
            let entered = || {
                let locked = expiry.decision_is_locked();
                let pending = expiry.repeated(&tenant(), [14; 32], &key("grant-14"));
                rejection_barrier.wait();
                assert!(
                    locked,
                    "decision lock must span preparation through persistence"
                );
                assert_eq!(pending, Ok(None));
            };
            let mut service = ApprovalService::new(&registry, &limiter, &expiry);
            service.before_decision = Some(&entered);
            service
                .reject(DecisionRequest {
                    tenant: &tenant(),
                    approval_id: [14; 32],
                    idempotency_key: &key("reject-14"),
                    approver: approver("human:reject"),
                    current_sequence: 11,
                })
                .unwrap_or_else(|error| panic!("reject: {error:?}"))
        });
        (
            grant.join().unwrap_or_else(|_| panic!("grant thread")),
            rejection.join().unwrap_or_else(|_| panic!("reject thread")),
        )
    });
    assert_eq!(grant.outcome, ApprovalOutcome::Granted);
    assert_eq!(rejection.outcome, ApprovalOutcome::Conflict);
    assert_eq!(rejection.winning_outcome, Some(ApprovalOutcome::Granted));
    assert_eq!(queue.len(), 1);
    drop(expiry);
    let reopened =
        ApprovalExpiry::open(directory.path()).unwrap_or_else(|error| panic!("reopen: {error:?}"));
    let durable = reopened
        .recover(&tenant(), [14; 32], 40, 12, &limiter)
        .unwrap_or_else(|error| panic!("recover: {error:?}"));
    assert_eq!(durable.outcome, ApprovalOutcome::Granted);
    assert_eq!(durable.submission_ref, grant.submission_ref);
}

#[test]
fn stale_prepared_grant_cannot_overwrite_durable_rejection() {
    let directory = Directory::new("conditional-put");
    let registry = ApprovalRegistry::default();
    let limiter = limiter();
    let expiry =
        ApprovalExpiry::open(directory.path()).unwrap_or_else(|error| panic!("expiry: {error:?}"));
    let held = prepared(15, 40);
    reserve_hold(&limiter, 15, 40);
    hold(&registry, context(15), held, 10, 40).unwrap_or_else(|error| panic!("hold: {error:?}"));
    let snapshot = registry
        .get_scoped(&tenant(), [15; 32], 11)
        .unwrap_or_else(|error| panic!("snapshot: {error:?}"));
    let super::expiry::DecisionResolution::WinnerPrepared(prepared) = expiry
        .decide(
            &snapshot,
            11,
            &key("grant-15"),
            ApprovalOutcome::Granted,
            Some([15; 32]),
            &limiter,
        )
        .unwrap_or_else(|error| panic!("prepare: {error:?}"))
    else {
        panic!("grant not prepared");
    };
    let rejection = ApprovalService::new(&registry, &limiter, &expiry)
        .reject(DecisionRequest {
            tenant: &tenant(),
            approval_id: [15; 32],
            idempotency_key: &key("reject-15"),
            approver: approver("human:reject"),
            current_sequence: 11,
        })
        .unwrap_or_else(|error| panic!("reject: {error:?}"));
    assert_eq!(rejection.outcome, ApprovalOutcome::Rejected);
    assert_eq!(
        expiry.persist_prepared_decision(prepared),
        Err(super::ApprovalExpiryError::DecisionConflict)
    );
    drop(expiry);
    let reopened =
        ApprovalExpiry::open(directory.path()).unwrap_or_else(|error| panic!("reopen: {error:?}"));
    let durable = reopened
        .recover(&tenant(), [15; 32], 40, 12, &limiter)
        .unwrap_or_else(|error| panic!("recover: {error:?}"));
    assert_eq!(durable.outcome, ApprovalOutcome::Rejected);
    assert_eq!(durable.submission_ref, None);
}
