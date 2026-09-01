use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use layerx_agent_api::identity::{
    ActivityType, AgentDid, Asset, CapabilityId as ApiCapabilityId, Counterparty, ExplicitSet,
    TenantId as ApiTenantId,
};
use layerx_agent_api::read::{AccountRef, ModuleRef};
use layerx_agent_api::subscription::{
    Cursor as ApiCursor, CursorAcknowledgement, DeliveryTarget, SubscriptionCreate,
    SubscriptionFilter, SubscriptionId, SubscriptionScope, SubscriptionTarget, TenantObject,
};
use layerx_agent_api::Sequence;
use layerx_agentd::approval::{
    ApprovalEmission, ApprovalEventKind, ApprovalEvents, ApprovalLifecycle,
    APPROVAL_ENFORCEMENT_NOTICE,
};
use layerx_agentd::audit::{
    export, Coverage, Decision, EventClass, EvidenceStore, Log, PayloadEvidence, Query,
};
use layerx_agentd::events::gap::{
    admit, apply_backfill, detect, BackfillReport, BackfillResolution, RecoveredEvent,
};
use layerx_agentd::events::subscription::Store as SubscriptionStore;
use layerx_agentd::events::{
    backfill, deliver, DeliveryEngine, DeliveryError, DeliveryItem, EventIngestor, RetryPolicy,
};
use layerx_agentd::session::SessionId;
use layerx_agentd::store::{Store, TenantId};
use layerx_agentd::tenant::{Config, RedactionPolicy, Retention};
use layerx_proof::checkpoint::SettlementDomain;
use layerx_types::ids::Did;
use layerx_types::result::ResultCode;
use layerx_types::verify::VerificationLevel;
use sha2::{Digest as _, Sha256};

static NEXT_DIRECTORY: AtomicU64 = AtomicU64::new(1);

fn directory() -> PathBuf {
    let sequence = NEXT_DIRECTORY.fetch_add(1, Ordering::Relaxed);
    std::env::temp_dir().join(format!(
        "layerx-approval-events-{}-{sequence}",
        std::process::id()
    ))
}

fn tenant() -> TenantId {
    TenantId::new("tenant-a").unwrap_or_else(|error| panic!("tenant: {error}"))
}

fn api_tenant() -> ApiTenantId {
    ApiTenantId::new("tenant-a").unwrap_or_else(|error| panic!("API tenant: {error:?}"))
}

fn agent() -> Did {
    Did::new(b"did:layerx:approval-events").unwrap_or_else(|error| panic!("agent: {error:?}"))
}

fn lifecycle(kind: ApprovalEventKind, id: u8, observed_at_ms: u64) -> ApprovalLifecycle {
    ApprovalLifecycle {
        tenant: tenant(),
        agent: agent(),
        session: SessionId([2; 32]),
        capability: [3; 32],
        policy_version: "policy-v4".to_owned(),
        approval_id: [id; 32],
        canonical_digest: Sha256::digest(format!("approval-{id}").as_bytes()).into(),
        activity_type: 7,
        asset: "LXP".to_owned(),
        kind,
        principal: matches!(
            kind,
            ApprovalEventKind::Granted | ApprovalEventKind::Rejected
        )
        .then(|| "human:operator".to_owned()),
        observed_at_ms,
    }
}

fn scope() -> SubscriptionScope {
    SubscriptionScope {
        tenant: api_tenant(),
        agent: AgentDid::new("did:layerx:approval-events")
            .unwrap_or_else(|error| panic!("API agent: {error:?}")),
        capability: ApiCapabilityId::new("approval-events")
            .unwrap_or_else(|error| panic!("API capability: {error:?}")),
    }
}

fn target() -> SubscriptionTarget {
    SubscriptionTarget {
        scope: scope(),
        subscription_id: SubscriptionId::new("approval-lifecycle")
            .unwrap_or_else(|error| panic!("subscription: {error:?}")),
    }
}

fn filter() -> SubscriptionFilter {
    SubscriptionFilter {
        agents: ExplicitSet::allow(vec![TenantObject {
            tenant: api_tenant(),
            value: AgentDid::new("did:layerx:approval-events")
                .unwrap_or_else(|error| panic!("filter agent: {error:?}")),
        }]),
        accounts: ExplicitSet::allow(vec![TenantObject {
            tenant: api_tenant(),
            value: AccountRef::new("tenant-a").unwrap_or_else(|error| panic!("account: {error:?}")),
        }]),
        activity_types: ExplicitSet::allow(vec![ActivityType(7)]),
        modules: ExplicitSet::allow(vec![TenantObject {
            tenant: api_tenant(),
            value: ModuleRef::new("approval").unwrap_or_else(|error| panic!("module: {error:?}")),
        }]),
        assets: ExplicitSet::allow(vec![TenantObject {
            tenant: api_tenant(),
            value: Asset::new("LXP").unwrap_or_else(|error| panic!("asset: {error:?}")),
        }]),
        counterparties: ExplicitSet::allow(vec![TenantObject {
            tenant: api_tenant(),
            value: Counterparty::new("human-approval")
                .unwrap_or_else(|error| panic!("counterparty: {error:?}")),
        }]),
        result_classes: ExplicitSet::allow(vec![
            ResultCode::from_raw(0),
            ResultCode::from_raw(-1),
            ResultCode::from_raw(-2),
        ]),
    }
}

fn config() -> Config {
    Config {
        tenant: tenant(),
        policy_version: "policy-v4".to_owned(),
        redaction: RedactionPolicy::Standard,
        retention: Retention {
            events: 100,
            audit: 100,
            receipts: 100,
        },
        verification_default: VerificationLevel::STATE_PROVEN,
        approval_required_for: BTreeSet::from([7]),
    }
}

fn emit_lifecycles(
    ingestor: &mut EventIngestor,
    audit: &mut Log,
    coverage: &mut Coverage,
    lifecycles: &[ApprovalLifecycle],
) -> Vec<ApprovalEmission> {
    lifecycles
        .iter()
        .map(|event| {
            ApprovalEvents::emit(ingestor, audit, coverage, event)
                .unwrap_or_else(|error| panic!("emit: {error}"))
        })
        .collect()
}

fn subscriptions_restored_after_gap(durable: Store, recovered_bytes: Vec<u8>) -> SubscriptionStore {
    let mut subscriptions = SubscriptionStore::open(durable, tenant())
        .unwrap_or_else(|error| panic!("subscriptions: {error}"));
    subscriptions
        .create(
            target().subscription_id,
            SubscriptionCreate {
                scope: scope(),
                filter: filter(),
                start: ApiCursor(Sequence(0)),
                delivery_target: DeliveryTarget::new("uds://approval-consumer")
                    .unwrap_or_else(|error| panic!("target: {error:?}")),
            },
        )
        .unwrap_or_else(|error| panic!("create: {error}"));
    let gap = detect(&mut subscriptions, &target(), 1, 2)
        .unwrap_or_else(|error| panic!("detect gap: {error}"))
        .unwrap_or_else(|| panic!("gap missing"));
    assert!(admit(&subscriptions, &target()).is_err());
    let report = BackfillReport::Recovered {
        gap,
        events: vec![RecoveredEvent {
            global_sequence: 1,
            canonical_bytes: recovered_bytes,
        }],
    };
    assert!(matches!(
        apply_backfill(&mut subscriptions, &target(), gap, &report),
        Ok(BackfillResolution::Restored)
    ));
    assert!(admit(&subscriptions, &target()).is_ok());
    subscriptions
}

fn delivery_engine(subscriptions: SubscriptionStore) -> DeliveryEngine {
    DeliveryEngine::open(
        subscriptions,
        target(),
        4,
        8,
        RetryPolicy {
            base_delay_ms: 10,
            maximum_delay_ms: 100,
            jitter_percent: 0,
            maximum_attempts: 3,
        },
    )
    .unwrap_or_else(|error| panic!("delivery: {error}"))
}

fn refill_backfill(delivery: &mut DeliveryEngine, label: &str) {
    match backfill(delivery) {
        Ok(_) | Err(DeliveryError::Backpressure { .. }) => {}
        Err(error) => panic!("{label}: {error}"),
    }
}

fn drain_delivered(delivery: &mut DeliveryEngine) -> Vec<Vec<u8>> {
    let mut delivered = Vec::new();
    refill_backfill(delivery, "backfill");
    while let Some(item) =
        deliver(delivery).unwrap_or_else(|error| panic!("delivery attempt: {error}"))
    {
        let accepted = delivery
            .accept_front(1_000)
            .unwrap_or_else(|error| panic!("accept: {error}"));
        assert_eq!(accepted, item);
        if let DeliveryItem::Event(event) = accepted {
            delivered.push(event.delivery.event_bytes.clone());
            delivery
                .acknowledge(&CursorAcknowledgement {
                    scope: scope(),
                    subscription_id: target().subscription_id,
                    cursor: event.delivery.cursor,
                })
                .unwrap_or_else(|error| panic!("acknowledge: {error}"));
        }
        refill_backfill(delivery, "continued backfill");
    }
    delivered
}

fn assert_digest_evidence_exported(audit_path: &Path, lifecycles: &[ApprovalLifecycle]) {
    let exported = export(
        audit_path,
        &config(),
        Query {
            tenant: tenant(),
            agent: None,
            from_observed_at_ms: None,
            through_observed_at_ms: None,
        },
        &EvidenceStore::new(tenant()),
        SettlementDomain::new(31_337, [0x55; 20]),
    )
    .unwrap_or_else(|error| panic!("export: {error}"));
    assert_eq!(exported.entries.len(), 4);
    for (entry, lifecycle) in exported.entries.iter().zip(lifecycles) {
        assert_eq!(entry.entry.class, EventClass::PolicyDecision);
        assert_eq!(
            entry.entry.submitted_bytes,
            Some(PayloadEvidence::Digest(lifecycle.canonical_digest))
        );
        assert!(entry.entry.protocol_authority.is_none());
        assert!(entry.entry.reason.as_str().contains("daemon-enforced"));
        assert!(entry
            .entry
            .reason
            .as_str()
            .contains("no protocol authority"));
    }
    assert_eq!(
        exported
            .entries
            .iter()
            .map(|entry| entry.entry.decision)
            .collect::<Vec<_>>(),
        vec![
            Decision::Requested,
            Decision::Allowed,
            Decision::Refused,
            Decision::Failed,
        ]
    );
}

#[test]
fn approval_lifecycle_stream_resumes_after_gap_and_exports_digest_evidence() {
    let root = directory();
    fs::create_dir_all(&root).unwrap_or_else(|error| panic!("root: {error}"));
    let durable = Store::open(&root).unwrap_or_else(|error| panic!("store: {error}"));
    let mut ingestor = EventIngestor::open(durable, tenant(), 16, 0)
        .unwrap_or_else(|error| panic!("ingestor: {error}"));
    let mut audit = Log::open(&root, &tenant()).unwrap_or_else(|error| panic!("audit: {error}"));
    let audit_path = audit.path().to_path_buf();
    let mut coverage = Coverage::default();
    let lifecycles = [
        lifecycle(ApprovalEventKind::Created, 1, 100),
        lifecycle(ApprovalEventKind::Granted, 1, 101),
        lifecycle(ApprovalEventKind::Rejected, 2, 102),
        lifecycle(ApprovalEventKind::Expired, 3, 103),
    ];
    let emissions = emit_lifecycles(&mut ingestor, &mut audit, &mut coverage, &lifecycles);
    assert_eq!(ingestor.watermark().next_expected, 4);
    assert_eq!(coverage.count(EventClass::PolicyDecision), 4);
    assert!(emissions
        .iter()
        .all(|event| event.enforcement_notice == APPROVAL_ENFORCEMENT_NOTICE));
    drop(audit);

    let subscriptions = subscriptions_restored_after_gap(
        ingestor.into_store(),
        emissions[1].canonical_event_bytes.clone(),
    );
    let mut delivery = delivery_engine(subscriptions);
    let delivered = drain_delivered(&mut delivery);
    assert_eq!(
        delivered,
        emissions
            .iter()
            .map(|event| event.canonical_event_bytes.clone())
            .collect::<Vec<_>>()
    );

    assert_digest_evidence_exported(&audit_path, &lifecycles);
    let _ = fs::remove_dir_all(root);
}
