use std::fs;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};

use layerx_agent_api::identity::{
    ActivityType, AgentDid, Asset, CapabilityId, Counterparty, ExplicitSet, TenantId as ApiTenantId,
};
use layerx_agent_api::read::{AccountRef, ModuleRef};
use layerx_agent_api::subscription::{
    Cursor as ApiCursor, CursorAcknowledgement, DeduplicationId, DeliveryTarget,
    SubscriptionCreate, SubscriptionFilter, SubscriptionId, SubscriptionScope, SubscriptionTarget,
    TenantObject,
};
use layerx_agent_api::Sequence;
use layerx_agentd::events::subscription::Store as SubscriptionStore;
use layerx_agentd::events::{
    backfill, deliver, health, ingest, CoreEvent, DeliveryEngine, DeliveryError, DeliveryItem,
    DeliveryPhase, EventAttributes, EventIngestor, RetryPolicy, CONSUMER_DEDUPLICATION_OBLIGATION,
};
use layerx_agentd::store::{Store, TenantId};
use layerx_types::result::ResultCode;
use layerx_types::verify::VerificationLevel;

static NEXT_DIRECTORY: AtomicU64 = AtomicU64::new(1);

fn test_directory(name: &str) -> PathBuf {
    let sequence = NEXT_DIRECTORY.fetch_add(1, Ordering::Relaxed);
    std::env::temp_dir().join(format!(
        "layerx-agentd-delivery-{name}-{}-{sequence}",
        std::process::id()
    ))
}

fn durable_tenant() -> TenantId {
    match TenantId::new("tenant-a") {
        Ok(value) => value,
        Err(error) => panic!("durable tenant must be valid: {error}"),
    }
}

fn api_tenant() -> ApiTenantId {
    match ApiTenantId::new("tenant-a") {
        Ok(value) => value,
        Err(error) => panic!("API tenant must be valid: {error:?}"),
    }
}

fn text<T, E: std::fmt::Debug>(result: Result<T, E>, label: &str) -> T {
    match result {
        Ok(value) => value,
        Err(error) => panic!("{label} must be valid: {error:?}"),
    }
}

fn scope() -> SubscriptionScope {
    SubscriptionScope {
        tenant: api_tenant(),
        agent: text(AgentDid::new("agent-a"), "agent"),
        capability: text(CapabilityId::new("capability-a"), "capability"),
    }
}

fn id() -> SubscriptionId {
    text(SubscriptionId::new("subscription-a"), "subscription")
}

fn target() -> SubscriptionTarget {
    SubscriptionTarget {
        scope: scope(),
        subscription_id: id(),
    }
}

fn filter() -> SubscriptionFilter {
    SubscriptionFilter {
        agents: ExplicitSet::allow(vec![TenantObject {
            tenant: api_tenant(),
            value: text(AgentDid::new("agent-a"), "agent"),
        }]),
        accounts: ExplicitSet::allow(vec![TenantObject {
            tenant: api_tenant(),
            value: text(AccountRef::new("account-a"), "account"),
        }]),
        activity_types: ExplicitSet::allow(vec![ActivityType(9)]),
        modules: ExplicitSet::allow(vec![TenantObject {
            tenant: api_tenant(),
            value: text(ModuleRef::new("asset"), "module"),
        }]),
        assets: ExplicitSet::allow(vec![TenantObject {
            tenant: api_tenant(),
            value: text(Asset::new("LXR"), "asset"),
        }]),
        counterparties: ExplicitSet::allow(vec![TenantObject {
            tenant: api_tenant(),
            value: text(Counterparty::new("counterparty-a"), "counterparty"),
        }]),
        result_classes: ExplicitSet::allow(vec![ResultCode::from_raw(0)]),
    }
}

fn core_event(sequence: u64) -> CoreEvent {
    core_event_for(sequence, "agent-a")
}

fn core_event_for(sequence: u64, agent: &str) -> CoreEvent {
    CoreEvent {
        global_sequence: sequence,
        canonical_bytes: vec![0x45, u8::try_from(sequence).unwrap_or(0), 0x56],
        receipt_reference: None,
        receipt_verification_level: VerificationLevel::UNVERIFIED,
        attributes: EventAttributes {
            agent: agent.to_owned(),
            account: "account-a".to_owned(),
            activity_type: 9,
            module: "asset".to_owned(),
            asset: "LXR".to_owned(),
            counterparty: "counterparty-a".to_owned(),
            result_code: 0,
        },
    }
}

fn retry_policy(maximum_attempts: u32) -> RetryPolicy {
    RetryPolicy {
        base_delay_ms: 10,
        maximum_delay_ms: 80,
        jitter_percent: 25,
        maximum_attempts,
    }
}

fn engine(
    root: &PathBuf,
    sequences: &[u64],
    initial_sequence: u64,
    subscription_start: u64,
    live_start: u64,
    capacity: usize,
    attempts: u32,
) -> DeliveryEngine {
    let events: Vec<CoreEvent> = sequences
        .iter()
        .map(|sequence| core_event(*sequence))
        .collect();
    engine_with_events(
        root,
        &events,
        initial_sequence,
        subscription_start,
        live_start,
        capacity,
        attempts,
    )
}

fn engine_with_events(
    root: &PathBuf,
    events: &[CoreEvent],
    initial_sequence: u64,
    subscription_start: u64,
    live_start: u64,
    capacity: usize,
    attempts: u32,
) -> DeliveryEngine {
    let tenant = durable_tenant();
    let durable = match Store::open(root) {
        Ok(value) => value,
        Err(error) => panic!("store open failed: {error}"),
    };
    let mut ingestor = match EventIngestor::open(durable, tenant.clone(), 64, initial_sequence) {
        Ok(value) => value,
        Err(error) => panic!("ingestor open failed: {error}"),
    };
    for event in events {
        if let Err(error) = ingest(&mut ingestor, event.clone()) {
            panic!("event {} ingest failed: {error}", event.global_sequence);
        }
    }
    let durable = ingestor.into_store();
    let mut subscriptions = match SubscriptionStore::open(durable, tenant) {
        Ok(value) => value,
        Err(error) => panic!("subscription store open failed: {error}"),
    };
    let create = SubscriptionCreate {
        scope: scope(),
        filter: filter(),
        start: ApiCursor(Sequence(subscription_start)),
        delivery_target: text(DeliveryTarget::new("uds://consumer-a"), "target"),
    };
    if let Err(error) = subscriptions.create(id(), create) {
        panic!("subscription create failed: {error}");
    }
    match DeliveryEngine::open(
        subscriptions,
        target(),
        live_start,
        capacity,
        retry_policy(attempts),
    ) {
        Ok(value) => value,
        Err(error) => panic!("delivery engine open failed: {error}"),
    }
}

fn acknowledge(engine: &mut DeliveryEngine, cursor: ApiCursor) {
    let acknowledgement = CursorAcknowledgement {
        scope: scope(),
        subscription_id: id(),
        cursor,
    };
    if let Err(error) = engine.acknowledge(&acknowledgement) {
        panic!("cursor acknowledgement failed: {error}");
    }
}

#[test]
fn loaded_seam_is_ordered_observable_and_has_no_duplicate() {
    let root = test_directory("loaded-seam");
    let sequences: Vec<u64> = (0..9).collect();
    let mut delivery = engine(&root, &sequences, 0, 0, 5, 3, 4);
    assert!(matches!(
        backfill(&mut delivery),
        Err(DeliveryError::Backpressure {
            capacity: 3,
            lag_sequences: 9,
        })
    ));

    let first = match deliver(&mut delivery) {
        Ok(Some(item)) => item,
        other => panic!("first delivery missing: {other:?}"),
    };
    let repeated = match deliver(&mut delivery) {
        Ok(Some(item)) => item,
        other => panic!("repeated delivery missing: {other:?}"),
    };
    assert_eq!(
        first, repeated,
        "unaccepted delivery must be retried exactly"
    );

    let mut order = Vec::new();
    let mut steps = 0_u32;
    loop {
        steps += 1;
        assert!(steps < 40, "delivery did not terminate");
        match backfill(&mut delivery) {
            Ok(_) | Err(DeliveryError::Backpressure { .. }) => {}
            Err(error) => panic!("history pump failed: {error}"),
        }
        let Some(item) = (match deliver(&mut delivery) {
            Ok(value) => value,
            Err(error) => panic!("delivery attempt failed: {error}"),
        }) else {
            break;
        };
        match &item {
            DeliveryItem::Event(event) => {
                let expected_dedup =
                    DeduplicationId::from_event_identity(event.delivery.event_identity);
                assert_eq!(event.delivery.deduplication_id, expected_dedup);
                assert_eq!(
                    event.delivery.event_bytes,
                    core_event(event.global_sequence).canonical_bytes
                );
                order.push((Some(event.phase), event.global_sequence));
            }
            DeliveryItem::BackfillComplete(transition) => {
                assert_eq!(transition.live_starts_at, 5);
                order.push((None, transition.live_starts_at));
            }
        }
        let accepted = match delivery.accept_front(1_000 + u64::from(steps)) {
            Ok(value) => value,
            Err(error) => panic!("front acceptance failed: {error}"),
        };
        assert_eq!(accepted, item);
        if let DeliveryItem::Event(event) = accepted {
            acknowledge(&mut delivery, event.delivery.cursor);
        }
    }

    let expected = vec![
        (Some(DeliveryPhase::Backfill), 0),
        (Some(DeliveryPhase::Backfill), 1),
        (Some(DeliveryPhase::Backfill), 2),
        (Some(DeliveryPhase::Backfill), 3),
        (Some(DeliveryPhase::Backfill), 4),
        (None, 5),
        (Some(DeliveryPhase::Live), 5),
        (Some(DeliveryPhase::Live), 6),
        (Some(DeliveryPhase::Live), 7),
        (Some(DeliveryPhase::Live), 8),
    ];
    assert_eq!(order, expected);
    assert!(delivery.seam_confirmed());
    let snapshot = health(&delivery).unwrap_or_else(|error| panic!("health: {error}"));
    assert_eq!(snapshot.lag_sequences, 0);
    assert!(CONSUMER_DEDUPLICATION_OBLIGATION.contains("must deduplicate"));
    let _ = fs::remove_dir_all(root);
}

#[test]
fn stalled_consumer_keeps_the_seam_and_event_under_bounded_retry() {
    let root = test_directory("stall");
    let mut delivery = engine(&root, &[0, 1], 0, 0, 1, 1, 2);
    assert!(matches!(
        backfill(&mut delivery),
        Err(DeliveryError::Backpressure { capacity: 1, .. })
    ));
    let first = match deliver(&mut delivery) {
        Ok(Some(DeliveryItem::Event(event))) => event,
        other => panic!("backfill event missing: {other:?}"),
    };
    let retry_one = match delivery.fail_front(100, "consumer unavailable") {
        Ok(value) => value,
        Err(error) => panic!("first retry failed: {error}"),
    };
    let retry_two = match delivery.fail_front(200, "consumer still unavailable") {
        Ok(value) => value,
        Err(error) => panic!("second retry failed: {error}"),
    };
    assert_eq!(retry_one.attempt, 1);
    assert_eq!(retry_two.attempt, 2);
    assert!((10..=80).contains(&retry_one.delay_ms));
    assert!((10..=80).contains(&retry_two.delay_ms));
    assert!(matches!(
        delivery.fail_front(300, "still unavailable"),
        Err(DeliveryError::RetryExhausted { attempts: 2 })
    ));
    let snapshot = health(&delivery).unwrap_or_else(|error| panic!("health: {error}"));
    assert!(snapshot.lagging);
    assert_eq!(snapshot.failure_count, 2);
    assert!(matches!(
        deliver(&mut delivery),
        Ok(Some(DeliveryItem::Event(ref event))) if *event == first
    ));
    if let Err(error) = delivery.accept_front(400) {
        panic!("event acceptance failed: {error}");
    }
    acknowledge(&mut delivery, first.delivery.cursor);

    assert!(matches!(
        backfill(&mut delivery),
        Err(DeliveryError::Backpressure { capacity: 1, .. })
    ));
    let seam = match deliver(&mut delivery) {
        Ok(Some(DeliveryItem::BackfillComplete(value))) => value,
        other => panic!("seam marker missing: {other:?}"),
    };
    assert_eq!(seam.live_starts_at, 1);
    assert_eq!(delivery.buffered_len(), 1);
    if let Err(error) = delivery.fail_front(500, "consumer stalled at seam") {
        panic!("seam retry failed: {error}");
    }
    assert!(matches!(
        deliver(&mut delivery),
        Ok(Some(DeliveryItem::BackfillComplete(value))) if value == seam
    ));
    if let Err(error) = delivery.accept_front(600) {
        panic!("seam acceptance failed: {error}");
    }
    assert!(delivery.seam_confirmed());
    match backfill(&mut delivery) {
        Ok(_) => {}
        Err(error) => panic!("live pump failed: {error}"),
    }
    assert!(matches!(
        deliver(&mut delivery),
        Ok(Some(DeliveryItem::Event(event)))
            if event.global_sequence == 1 && event.phase == DeliveryPhase::Live
    ));
    let _ = fs::remove_dir_all(root);
}

#[test]
fn start_before_filter_subject_genesis_transitions_before_first_live_match() {
    let root = test_directory("pre-genesis");
    let events: Vec<CoreEvent> = (0..7)
        .map(|sequence| core_event_for(sequence, if sequence < 5 { "agent-b" } else { "agent-a" }))
        .collect();
    let mut delivery = engine_with_events(&root, &events, 0, 0, 5, 2, 3);
    assert!(matches!(
        backfill(&mut delivery),
        Err(DeliveryError::Backpressure { capacity: 2, .. })
    ));
    let transition = match deliver(&mut delivery) {
        Ok(Some(DeliveryItem::BackfillComplete(value))) => value,
        other => panic!("pre-genesis transition missing: {other:?}"),
    };
    assert_eq!(transition.live_starts_at, 5);
    assert_eq!(transition.resume_cursor, ApiCursor(Sequence(0)));
    if let Err(error) = delivery.accept_front(1) {
        panic!("transition acceptance failed: {error}");
    }
    let first_match = match deliver(&mut delivery) {
        Ok(Some(DeliveryItem::Event(value))) => value,
        other => panic!("first matching event missing: {other:?}"),
    };
    assert_eq!(first_match.global_sequence, 5);
    assert_eq!(first_match.phase, DeliveryPhase::Live);
    assert_eq!(first_match.delivery.cursor, ApiCursor(Sequence(6)));
    if let Err(error) = delivery.accept_front(2) {
        panic!("first live event acceptance failed: {error}");
    }
    acknowledge(&mut delivery, first_match.delivery.cursor);
    match backfill(&mut delivery) {
        Ok(_) => {}
        Err(error) => panic!("second live pump failed: {error}"),
    }
    assert!(matches!(
        deliver(&mut delivery),
        Ok(Some(DeliveryItem::Event(event)))
            if event.global_sequence == 6 && event.phase == DeliveryPhase::Live
    ));
    let _ = fs::remove_dir_all(root);
}
