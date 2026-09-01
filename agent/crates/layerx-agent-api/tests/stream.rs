use layerx_agent_api::identity::{ActivityType, AgentDid, CapabilityId, ExplicitSet, TenantId};
use layerx_agent_api::subscription::{
    Cursor, DeduplicationId, Delivery, DeliveryTarget, EventDelivery, EventIdentity, GapNotice,
    ReceiptReference, SubscriptionCreate, SubscriptionFilter, SubscriptionScope, TenantObject,
    TruncationNotice,
};
use layerx_agent_api::Sequence;

const SCHEMA: &str = include_str!("../../../schema/agent-api/stream.kvx");

fn required<T>(result: Result<T, layerx_agent_api::identity::ContractError>) -> T {
    result.unwrap_or_else(|error| panic!("valid contract value: {error:?}"))
}

fn scope() -> SubscriptionScope {
    SubscriptionScope {
        tenant: required(TenantId::new("tenant-a")),
        agent: required(AgentDid::new("did:layerx:agent-a")),
        capability: required(CapabilityId::new("cap-a")),
    }
}

fn empty_filter() -> SubscriptionFilter {
    SubscriptionFilter {
        agents: ExplicitSet::deny_all(),
        accounts: ExplicitSet::deny_all(),
        activity_types: ExplicitSet::allow(vec![ActivityType(7)]),
        modules: ExplicitSet::deny_all(),
        assets: ExplicitSet::deny_all(),
        counterparties: ExplicitSet::deny_all(),
        result_classes: ExplicitSet::deny_all(),
    }
}

#[test]
fn subscription_requires_tenant_agent_capability_and_durable_cursor() {
    assert!(CapabilityId::new("").is_err());
    let create = SubscriptionCreate {
        scope: scope(),
        filter: empty_filter(),
        start: Cursor(Sequence(44)),
        delivery_target: required(DeliveryTarget::new("pull:consumer-a")),
    }
    .validate()
    .unwrap_or_else(|error| panic!("subscription: {error:?}"));
    assert_eq!(create.start, Cursor(Sequence(44)));
    assert!(SCHEMA.contains("required = [\"scope\",\"filter\",\"start\",\"delivery_target\"]"));
    assert!(SCHEMA.contains("last acknowledged cursor"));
}

#[test]
fn filter_cannot_name_an_object_outside_its_tenant() {
    let mut filter = empty_filter();
    filter.agents = ExplicitSet::allow(vec![TenantObject {
        tenant: required(TenantId::new("tenant-b")),
        value: required(AgentDid::new("did:layerx:agent-b")),
    }]);
    assert!(filter.validate_for(&scope()).is_err());
    assert!(SCHEMA.contains("[\"tenant_restriction\",\"capability_scope_restriction\",\"filter\"]"));
}

#[test]
fn event_delivery_has_identity_derived_deduplication_and_receipt_level() {
    let identity = EventIdentity::new([9; 32]);
    let delivery = EventDelivery::new(
        identity,
        vec![1, 2, 3],
        Cursor(Sequence(45)),
        ReceiptReference::None,
    )
    .unwrap_or_else(|error| panic!("delivery: {error:?}"));
    assert_eq!(
        delivery.deduplication_id,
        DeduplicationId::from_event_identity(identity)
    );
    assert!(SCHEMA.contains("Consumers must deduplicate on deduplication_id"));
    assert_eq!(delivery.cursor, Cursor(Sequence(45)));
}

#[test]
fn gap_and_truncation_are_first_class_delivery_barriers() {
    let gap = GapNotice {
        missing_first: Sequence(46),
        missing_last: Sequence(48),
        backfill_cursor: Cursor(Sequence(46)),
        backfill_attempted: true,
    }
    .validate()
    .unwrap_or_else(|error| panic!("gap: {error:?}"));
    let truncated = TruncationNotice {
        requested_first: Sequence(1),
        oldest_available: Sequence(20),
        resume_cursor: Cursor(Sequence(20)),
    };
    assert!(matches!(Delivery::Gap(gap), Delivery::Gap(_)));
    assert!(matches!(
        Delivery::Truncated(truncated),
        Delivery::Truncated(_)
    ));
    assert!(SCHEMA.contains("No later event may be delivered before successful backfill"));
}
