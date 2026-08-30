#[allow(dead_code)]
mod support;

use std::collections::BTreeMap;
use std::fs;

use layerx_agent_api::identity::{ActivityType, AgentDid, Asset, AuthorityRef, ExplicitSet};
use layerx_agent_api::prepare::{
    CanonicalBytes, DisclosedAmount, Disclosure, IdempotencyRef, PreparationRef, Prepared,
    SigningPreimage,
};
use layerx_agent_api::track::{EvidenceRef, SubmissionRef, SubmissionState, TrackedSubmission};
use layerx_agent_api::verify::Level;
use layerx_agent_api::{Amount, TimestampSeconds};
use layerx_agentd::approval::{
    ApprovalEventKind as AgentEventKind, ApprovalEvents, ApprovalExpiry, ApprovalLifecycle,
    ApprovalOutcome, ApprovalService, ApprovalSubmissionQueue, DecisionKey, DecisionRequest,
};
use layerx_agentd::audit::{Coverage, Log};
use layerx_agentd::budget::BudgetLimiter;
use layerx_agentd::capability::CapabilityId;
use layerx_agentd::events::EventIngestor;
use layerx_agentd::policy::approval::{
    hold, ApprovalContext, ApprovalRegistry, ApprovalState, ApproverId,
};
use layerx_agentd::session::SessionId;
use layerx_agentd::store::{Store as AgentStore, TenantId};
use layerx_human_service::approvals::{
    AgentApprovalRecord, AgentApprovalState, ApprovalBoundary, ApprovalBoundaryError,
    ApprovalEvent, Inbox, InboxState, VerifiedBudgetAfter,
};
use layerx_human_service::audit::AuditChain;
use layerx_human_service::notify::{DetailLevel, Dispatcher};
use layerx_human_service::trace::TraceId;
use layerx_types::ids::Did;
use sha2::{Digest as _, Sha256};

fn tenant() -> TenantId {
    TenantId::new("tenant-inbox").unwrap_or_else(|error| panic!("tenant: {error}"))
}

fn prepared(id: u8, expiry: u64) -> Prepared {
    let bytes = format!("agent-generated-held-movement-{id}").into_bytes();
    let digest = Sha256::digest(&bytes).into();
    let counterparty = AgentDid::new("did:layerx:merchant")
        .unwrap_or_else(|error| panic!("counterparty: {error:?}"));
    Prepared {
        preparation_ref: PreparationRef::new(format!("preparation-{id}"))
            .unwrap_or_else(|error| panic!("preparation: {error:?}")),
        unsigned_canonical_bytes: CanonicalBytes::new(bytes)
            .unwrap_or_else(|error| panic!("canonical bytes: {error:?}")),
        signing_preimage: SigningPreimage::new(vec![id; 32])
            .unwrap_or_else(|error| panic!("preimage: {error:?}")),
        disclosure: Disclosure {
            canonical_digest: digest,
            activity_type: ActivityType(5),
            actor: AgentDid::new("did:layerx:inbox-agent")
                .unwrap_or_else(|error| panic!("actor: {error:?}")),
            authority: AuthorityRef::new("session-key")
                .unwrap_or_else(|error| panic!("authority: {error:?}")),
            counterparties: ExplicitSet::allow(vec![counterparty.clone()]),
            amounts: ExplicitSet::allow(vec![DisclosedAmount {
                counterparty,
                amount: Amount(25),
            }]),
            asset: Asset::new("LXP").unwrap_or_else(|error| panic!("asset: {error:?}")),
            fee_limit: Amount(2),
            expiry: TimestampSeconds(expiry),
            idempotency_key: IdempotencyRef::new(format!("movement-{id}"))
                .unwrap_or_else(|error| panic!("idempotency: {error:?}")),
        },
        expiry: TimestampSeconds(expiry),
    }
}

fn context(id: u8) -> ApprovalContext {
    ApprovalContext {
        tenant: tenant(),
        agent: Did::new(b"did:layerx:inbox-agent")
            .unwrap_or_else(|error| panic!("agent: {error:?}")),
        session: SessionId([2; 32]),
        capability: CapabilityId([3; 32]),
        policy_version: "policy-inbox-v1".to_owned(),
        request_id: [id; 32],
    }
}

struct AgentdBoundary<'a> {
    service: ApprovalService<'a>,
    queue: &'a ApprovalSubmissionQueue,
    tenant: TenantId,
    released: BTreeMap<[u8; 32], [u8; 32]>,
}

impl ApprovalBoundary for AgentdBoundary<'_> {
    fn approval(
        &mut self,
        approval_id: [u8; 32],
        at_sequence: u64,
    ) -> Result<AgentApprovalRecord, ApprovalBoundaryError> {
        let record = self
            .service
            .get(&self.tenant, approval_id, at_sequence)
            .map_err(|_| ApprovalBoundaryError::Unavailable)?;
        let state = match record.state {
            ApprovalState::AwaitingApproval => AgentApprovalState::AwaitingApproval,
            ApprovalState::Approved => AgentApprovalState::Approved {
                submission_ref: *self
                    .released
                    .get(&approval_id)
                    .ok_or(ApprovalBoundaryError::Corrupt)?,
            },
            ApprovalState::Rejected => AgentApprovalState::Rejected,
            ApprovalState::Expired => AgentApprovalState::Expired,
            ApprovalState::Defective => AgentApprovalState::Defective,
        };
        Ok(AgentApprovalRecord {
            approval_id,
            held_activity: record.held_activity,
            canonical_bytes_digest: record.canonical_bytes_digest,
            hold_reason_code: record.hold_reason.code.to_owned(),
            hold_reason: record.hold_reason.message.to_owned(),
            created_at_sequence: record.created_at_sequence,
            expires_at_sequence: record.expires_at_sequence,
            state,
        })
    }

    fn verified_budget_after(
        &mut self,
        hold: &AgentApprovalRecord,
        at_sequence: u64,
    ) -> Result<VerifiedBudgetAfter, ApprovalBoundaryError> {
        let account = [0x41; 32];
        let asset: [u8; 32] = Sha256::digest(hold.held_activity.asset.as_str().as_bytes()).into();
        let mut canonical = Vec::with_capacity(80);
        canonical.extend_from_slice(&account);
        canonical.extend_from_slice(&asset);
        canonical.extend_from_slice(&975_u128.to_be_bytes());
        let authority =
            support::evidence_verifier(&ed25519_dalek::SigningKey::from_bytes(&[0x84; 32]));
        let state = authority
            .verify_state(&support::raw_state_leaf(canonical, at_sequence))
            .map_err(|_| ApprovalBoundaryError::VerificationFailed)?;
        let canonical: [u8; 80] = state
            .canonical_state()
            .try_into()
            .map_err(|_| ApprovalBoundaryError::Corrupt)?;
        if state.level() != layerx_types::verify::VerificationLevel::STATE_PROVEN
            || canonical[..32] != account
            || canonical[32..64] != asset
        {
            return Err(ApprovalBoundaryError::VerificationFailed);
        }
        let remaining = u128::from_be_bytes(
            canonical[64..]
                .try_into()
                .map_err(|_| ApprovalBoundaryError::Corrupt)?,
        );
        let mut evidence = Sha256::new();
        evidence.update(canonical);
        evidence.update(state.observed_head_sequence().to_be_bytes());
        Ok(VerifiedBudgetAfter {
            remaining,
            level: Level::StateProven,
            evidence_digest: evidence.finalize().into(),
            observed_at_sequence: state.observed_head_sequence(),
        })
    }

    fn track_released(
        &mut self,
        submission_ref: [u8; 32],
    ) -> Result<TrackedSubmission, ApprovalBoundaryError> {
        if self.queue.prepared(submission_ref).is_none() {
            return Err(ApprovalBoundaryError::Corrupt);
        }
        Ok(TrackedSubmission {
            submission_ref: SubmissionRef::new(hex(submission_ref))
                .map_err(|_| ApprovalBoundaryError::Corrupt)?,
            state: SubmissionState::Queued,
            evidence: vec![EvidenceRef {
                kind: "approval-release".to_owned(),
                digest: submission_ref,
            }],
            verification_level: Level::SequencerSigned,
            transitions: Vec::new(),
        })
    }
}

fn hex(bytes: [u8; 32]) -> String {
    use std::fmt::Write as _;

    let mut output = String::with_capacity(64);
    for byte in bytes {
        let _ = write!(output, "{byte:02x}");
    }
    output
}

fn emit(
    ingestor: &mut EventIngestor,
    audit: &mut Log,
    coverage: &mut Coverage,
    kind: AgentEventKind,
    id: u8,
    observed_at: u64,
) -> ApprovalEvent {
    let lifecycle = ApprovalLifecycle {
        tenant: tenant(),
        agent: Did::new(b"did:layerx:inbox-agent")
            .unwrap_or_else(|error| panic!("stream agent: {error:?}")),
        session: SessionId([2; 32]),
        capability: [3; 32],
        policy_version: "policy-inbox-v1".to_owned(),
        approval_id: [id; 32],
        canonical_digest: prepared(id, 100).disclosure.canonical_digest,
        activity_type: 5,
        asset: "LXP".to_owned(),
        kind,
        principal: matches!(kind, AgentEventKind::Granted | AgentEventKind::Rejected)
            .then(|| "human-alice".to_owned()),
        observed_at_ms: observed_at,
    };
    let emission = ApprovalEvents::emit(ingestor, audit, coverage, &lifecycle)
        .unwrap_or_else(|error| panic!("real agent approval event: {error}"));
    ApprovalEvent::decode_agent_stream(&emission.canonical_event_bytes, observed_at)
        .unwrap_or_else(|error| panic!("decode agent approval event: {error}"))
}

#[test]
#[allow(clippy::too_many_lines)]
fn real_agent_holds_drive_live_inbox_counts_notifications_and_honest_lifecycle() {
    let directory = support::directory("approval-inbox");
    let agent_root = directory.join("agent");
    fs::create_dir_all(&agent_root).unwrap_or_else(|error| panic!("agent root: {error}"));
    let registry = ApprovalRegistry::default();
    let limiter =
        BudgetLimiter::new(Vec::new()).unwrap_or_else(|error| panic!("limiter: {error:?}"));
    let expiry = ApprovalExpiry::open(&agent_root)
        .unwrap_or_else(|error| panic!("approval expiry: {error:?}"));
    let service = ApprovalService::new(&registry, &limiter, &expiry);
    let queue = ApprovalSubmissionQueue::default();

    for (id, expires) in [(1_u8, 50_u64), (2, 50), (3, 15)] {
        hold(&registry, context(id), prepared(id, 100), 10, expires)
            .unwrap_or_else(|error| panic!("real agent hold {id}: {error:?}"));
    }

    let stream_root = agent_root.join("stream");
    fs::create_dir_all(&stream_root).unwrap_or_else(|error| panic!("stream root: {error}"));
    let stream_store =
        AgentStore::open(&stream_root).unwrap_or_else(|error| panic!("agent event store: {error}"));
    let mut ingestor = EventIngestor::open(stream_store, tenant(), 16, 10)
        .unwrap_or_else(|error| panic!("agent event ingestor: {error}"));
    let mut agent_audit =
        Log::open(&stream_root, &tenant()).unwrap_or_else(|error| panic!("agent audit: {error}"));
    let mut coverage = Coverage::default();

    let map = support::tenancy(&[("alice", "tenant-inbox")]);
    let (mut store, _) = support::install_and_open(
        &directory.join("human"),
        &map,
        support::retention_uniform(1_000),
    );
    let mut scope = store
        .principal(&support::principal("alice"))
        .unwrap_or_else(|error| panic!("principal scope: {error}"));
    let mut audit = AuditChain::open(&scope).unwrap_or_else(|error| panic!("audit: {error}"));
    let trace = TraceId::mint([7; 16]);
    let mut notification_preferences = Dispatcher::preferences(&scope)
        .unwrap_or_else(|error| panic!("notification preferences: {error}"));
    notification_preferences.set_detail(DetailLevel::Full);
    Dispatcher::update_preferences(&mut scope, 9, &notification_preferences)
        .unwrap_or_else(|error| panic!("full notification detail: {error}"));
    let mut boundary = AgentdBoundary {
        service,
        queue: &queue,
        tenant: tenant(),
        released: BTreeMap::new(),
    };
    let mut inbox = Inbox::new(10, 3).unwrap_or_else(|error| panic!("inbox: {error}"));
    inbox
        .consume(
            &[emit(
                &mut ingestor,
                &mut agent_audit,
                &mut coverage,
                AgentEventKind::Created,
                1,
                10,
            )],
            &mut boundary,
            &mut scope,
            &mut audit,
            &trace,
        )
        .unwrap_or_else(|error| panic!("created event: {error}"));
    let pending = inbox
        .snapshot(10)
        .unwrap_or_else(|error| panic!("snapshot: {error}"));
    assert_eq!(pending.awaiting_count(), 1);
    assert_eq!(pending.items()[0].amount(), 25);
    assert_eq!(pending.items()[0].budget_after().remaining, 975);
    assert_eq!(pending.items()[0].budget_after().level, Level::StateProven);
    assert_eq!(pending.items()[0].remaining(12), 38);
    let deliveries = Dispatcher::deliveries(&scope)
        .unwrap_or_else(|error| panic!("notification deliveries: {error}"));
    assert!(deliveries.iter().all(|delivery| {
        delivery
            .deep_link()
            .contains(pending.items()[0].approval_id().as_str())
    }));
    assert!(deliveries
        .iter()
        .all(|delivery| delivery.payload().contains("agt_")));
    assert!(deliveries
        .iter()
        .all(|delivery| delivery.payload().contains("25")));

    let held = prepared(1, 100);
    let decision = boundary
        .service
        .approve(
            DecisionRequest {
                tenant: &tenant(),
                approval_id: [1; 32],
                idempotency_key: &DecisionKey::new("approve-one")
                    .unwrap_or_else(|error| panic!("decision key: {error:?}")),
                approver: ApproverId::new("human-alice")
                    .unwrap_or_else(|error| panic!("approver: {error:?}")),
                current_sequence: 11,
            },
            &held,
            &queue,
        )
        .unwrap_or_else(|error| panic!("agent approve: {error:?}"));
    assert_eq!(decision.outcome, ApprovalOutcome::Granted);
    let released = decision
        .submission_ref
        .unwrap_or_else(|| panic!("approved hold lacked release reference"));
    boundary.released.insert([1; 32], released);
    inbox
        .consume(
            &[emit(
                &mut ingestor,
                &mut agent_audit,
                &mut coverage,
                AgentEventKind::Granted,
                1,
                11,
            )],
            &mut boundary,
            &mut scope,
            &mut audit,
            &trace,
        )
        .unwrap_or_else(|error| panic!("approved event: {error}"));
    let approved = inbox
        .snapshot(11)
        .unwrap_or_else(|error| panic!("approved: {error}"));
    assert_eq!(approved.awaiting_count(), 0);
    assert!(matches!(
        approved.items()[0].state(),
        InboxState::Approved { .. }
    ));

    inbox
        .consume(
            &[emit(
                &mut ingestor,
                &mut agent_audit,
                &mut coverage,
                AgentEventKind::Created,
                2,
                12,
            )],
            &mut boundary,
            &mut scope,
            &mut audit,
            &trace,
        )
        .unwrap_or_else(|error| panic!("second created event: {error}"));
    assert_eq!(
        inbox
            .snapshot(12)
            .unwrap_or_else(|error| panic!("second pending: {error}"))
            .awaiting_count(),
        1
    );
    let rejected = boundary
        .service
        .reject(DecisionRequest {
            tenant: &tenant(),
            approval_id: [2; 32],
            idempotency_key: &DecisionKey::new("reject-two")
                .unwrap_or_else(|error| panic!("decision key: {error:?}")),
            approver: ApproverId::new("human-alice")
                .unwrap_or_else(|error| panic!("approver: {error:?}")),
            current_sequence: 13,
        })
        .unwrap_or_else(|error| panic!("agent reject: {error:?}"));
    assert_eq!(rejected.outcome, ApprovalOutcome::Rejected);
    inbox
        .consume(
            &[emit(
                &mut ingestor,
                &mut agent_audit,
                &mut coverage,
                AgentEventKind::Rejected,
                2,
                13,
            )],
            &mut boundary,
            &mut scope,
            &mut audit,
            &trace,
        )
        .unwrap_or_else(|error| panic!("rejected event: {error}"));
    inbox
        .consume(
            &[emit(
                &mut ingestor,
                &mut agent_audit,
                &mut coverage,
                AgentEventKind::Created,
                3,
                14,
            )],
            &mut boundary,
            &mut scope,
            &mut audit,
            &trace,
        )
        .unwrap_or_else(|error| panic!("third created event: {error}"));
    assert_eq!(
        inbox
            .snapshot(14)
            .unwrap_or_else(|error| panic!("third pending: {error}"))
            .awaiting_count(),
        1
    );
    inbox
        .consume(
            &[emit(
                &mut ingestor,
                &mut agent_audit,
                &mut coverage,
                AgentEventKind::Expired,
                3,
                15,
            )],
            &mut boundary,
            &mut scope,
            &mut audit,
            &trace,
        )
        .unwrap_or_else(|error| panic!("expired event: {error}"));
    let resolved = inbox
        .snapshot(15)
        .unwrap_or_else(|error| panic!("resolved: {error}"));
    assert_eq!(resolved.awaiting_count(), 0);
    assert!(resolved.items().iter().any(|item| {
        matches!(item.state(), InboxState::Rejected)
            && item.state().nothing_moved() == Some("Nothing moved.")
    }));
    let all_deliveries = Dispatcher::deliveries(&scope)
        .unwrap_or_else(|error| panic!("all notification deliveries: {error}"));
    assert_eq!(all_deliveries.len(), 9, "each created hold notified once");
    assert!(resolved.items().iter().any(|item| {
        matches!(item.state(), InboxState::Expired)
            && !item.state().can_approve()
            && item.state().nothing_moved() == Some("Nothing moved.")
    }));
    assert!(
        inbox.snapshot(19).is_err(),
        "stale counts must not be served"
    );
    let _ = fs::remove_dir_all(directory);
}
