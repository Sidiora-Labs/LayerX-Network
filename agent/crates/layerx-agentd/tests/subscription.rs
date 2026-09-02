use std::collections::BTreeSet;
use std::fs;
use std::os::unix::fs::MetadataExt;
use std::os::unix::net::UnixListener;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{mpsc, Arc, RwLock};
use std::thread;
use std::time::Duration;

use layerx_agent_api::identity::{
    ActivityType, AgentDid, Asset, CapabilityId, Counterparty, ExplicitSet, TenantId as ApiTenantId,
};
use layerx_agent_api::read::{AccountRef, ModuleRef};
use layerx_agent_api::subscription::{
    Cursor as ApiCursor, CursorAcknowledgement, DeliveryTarget, SubscriptionCreate,
    SubscriptionFilter, SubscriptionId, SubscriptionScope, SubscriptionTarget, TenantObject,
};
use layerx_agent_api::Sequence;
use layerx_agentd::events::outbound::{
    deliver as deliver_outbound_unbound, deliver_shared_authorized as deliver_outbound_authorized,
    Authenticator, Endpoint, OutboundError, PeerIdentity, StopSignal,
};
use layerx_agentd::events::subscription::{
    Cursor, Store as SubscriptionStore, SubscriptionError, Termination,
};
use layerx_agentd::events::{
    backfill, backfill_authorized, deliver, deliver_authorized, health, health_authorized, ingest,
    CoreEvent, DeliveryEngine, DeliveryError, EventAttributes, EventIngestor, RetryPolicy,
};
use layerx_agentd::identity::{
    register, CoreIdentity, IdentityError, IdentityRecord, IdentityResolver, ProtocolAuthority,
};
use layerx_agentd::session::{close, open, OpenRequest, SessionId, SessionRegistry, Token};
use layerx_agentd::store::{ObjectKind, Store, TenantId, TenantKey};
use layerx_agentd::tenant::{AuthorizationError, Operation, TenantObservability};
use layerx_client::lni::framing::read_frame;
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

fn test_directory(name: &str) -> PathBuf {
    let sequence = NEXT_DIRECTORY.fetch_add(1, Ordering::Relaxed);
    std::env::temp_dir().join(format!(
        "layerx-agentd-subscription-{name}-{}-{sequence}",
        std::process::id()
    ))
}

fn durable_tenant(name: &str) -> TenantId {
    match TenantId::new(name) {
        Ok(value) => value,
        Err(error) => panic!("durable tenant must be valid: {error}"),
    }
}

fn api_tenant(name: &str) -> ApiTenantId {
    match ApiTenantId::new(name) {
        Ok(value) => value,
        Err(error) => panic!("API tenant must be valid: {error:?}"),
    }
}

fn agent(name: &str) -> AgentDid {
    match AgentDid::new(name) {
        Ok(value) => value,
        Err(error) => panic!("agent must be valid: {error:?}"),
    }
}

fn capability(name: &str) -> CapabilityId {
    match CapabilityId::new(name) {
        Ok(value) => value,
        Err(error) => panic!("capability must be valid: {error:?}"),
    }
}

fn subscription_id(name: &str) -> SubscriptionId {
    match SubscriptionId::new(name) {
        Ok(value) => value,
        Err(error) => panic!("subscription identifier must be valid: {error:?}"),
    }
}

fn target(name: &str) -> DeliveryTarget {
    match DeliveryTarget::new(name) {
        Ok(value) => value,
        Err(error) => panic!("delivery target must be valid: {error:?}"),
    }
}

fn scope(tenant: &str, agent_name: &str, capability_name: &str) -> SubscriptionScope {
    SubscriptionScope {
        tenant: api_tenant(tenant),
        agent: agent(agent_name),
        capability: capability(capability_name),
    }
}

fn tenant_object<T>(tenant: &str, value: T) -> TenantObject<T> {
    TenantObject {
        tenant: api_tenant(tenant),
        value,
    }
}

fn text<T, E: std::fmt::Debug>(result: Result<T, E>, label: &str) -> T {
    match result {
        Ok(value) => value,
        Err(error) => panic!("{label} must be valid: {error:?}"),
    }
}

fn filter(tenant: &str, agent_name: &str) -> SubscriptionFilter {
    SubscriptionFilter {
        agents: ExplicitSet::allow(vec![tenant_object(tenant, agent(agent_name))]),
        accounts: ExplicitSet::allow(vec![tenant_object(
            tenant,
            text(AccountRef::new("account-7"), "account"),
        )]),
        activity_types: ExplicitSet::allow(vec![ActivityType(9)]),
        modules: ExplicitSet::allow(vec![tenant_object(
            tenant,
            text(ModuleRef::new("asset"), "module"),
        )]),
        assets: ExplicitSet::allow(vec![tenant_object(
            tenant,
            text(Asset::new("LXR"), "asset"),
        )]),
        counterparties: ExplicitSet::allow(vec![tenant_object(
            tenant,
            text(Counterparty::new("counterparty-4"), "counterparty"),
        )]),
        result_classes: ExplicitSet::allow(vec![ResultCode::from_raw(0)]),
    }
}

fn request(scope: SubscriptionScope, filter: SubscriptionFilter, start: u64) -> SubscriptionCreate {
    SubscriptionCreate {
        scope,
        filter,
        start: ApiCursor(Sequence(start)),
        delivery_target: target("uds://consumer-a"),
    }
}

fn subscription_target(scope: &SubscriptionScope, id: &SubscriptionId) -> SubscriptionTarget {
    SubscriptionTarget {
        scope: scope.clone(),
        subscription_id: id.clone(),
    }
}

fn subscription_key(tenant: &TenantId, id: &SubscriptionId) -> TenantKey {
    text(
        TenantKey::new(
            tenant.clone(),
            ObjectKind::Subscription,
            id.as_str().as_bytes().to_vec(),
        ),
        "subscription key",
    )
}

fn rewrite_as_legacy_unbound(
    durable: &mut Store,
    tenant: &TenantId,
    scope: &SubscriptionScope,
    id: &SubscriptionId,
) {
    let key = subscription_key(tenant, id);
    let mut bytes = durable
        .get(&key)
        .map(|stored| stored.bytes().to_vec())
        .unwrap_or_else(|| panic!("subscription record missing"));
    assert_eq!(&bytes[..4], b"LXS2");
    let binding_offset = 4 + [
        id.as_str(),
        scope.tenant.as_str(),
        scope.agent.as_str(),
        scope.capability.as_str(),
    ]
    .iter()
    .map(|value| 4 + value.len())
    .sum::<usize>();
    assert_eq!(bytes[binding_offset], 0);
    bytes.remove(binding_offset);
    bytes[..4].copy_from_slice(b"LXSB");
    text(durable.put_local(key, bytes), "legacy subscription put");
}

#[test]
fn restart_resumes_from_acknowledged_cursor_and_remembers_deliveries() {
    let root = test_directory("restart");
    let tenant_id = durable_tenant("tenant-a");
    let subscription_scope = scope("tenant-a", "agent-a", "capability-a");
    let id = subscription_id("subscription-a");
    let target = subscription_target(&subscription_scope, &id);
    let durable = match Store::open(&root) {
        Ok(value) => value,
        Err(error) => panic!("store open failed: {error}"),
    };
    let mut subscriptions = match SubscriptionStore::open(durable, tenant_id.clone()) {
        Ok(value) => value,
        Err(error) => panic!("subscription store open failed: {error}"),
    };
    let created = match subscriptions.create(
        id.clone(),
        request(
            subscription_scope.clone(),
            filter("tenant-a", "agent-a"),
            10,
        ),
    ) {
        Ok(value) => value,
        Err(error) => panic!("subscription create failed: {error}"),
    };
    assert_eq!(created.filter, filter("tenant-a", "agent-a"));
    if let Err(error) = subscriptions.mark_delivered(&target, Cursor(11)) {
        panic!("cursor 11 delivery failed: {error}");
    }
    if let Err(error) = subscriptions.mark_delivered(&target, Cursor(12)) {
        panic!("cursor 12 delivery failed: {error}");
    }
    let acknowledged = CursorAcknowledgement {
        scope: subscription_scope.clone(),
        subscription_id: id.clone(),
        cursor: ApiCursor(Sequence(11)),
    };
    if let Err(error) = subscriptions.acknowledge(&acknowledged) {
        panic!("cursor acknowledgement failed: {error}");
    }
    drop(subscriptions);

    let durable = match Store::open(&root) {
        Ok(value) => value,
        Err(error) => panic!("store restart failed: {error}"),
    };
    let mut restarted = match SubscriptionStore::open(durable, tenant_id) {
        Ok(value) => value,
        Err(error) => panic!("subscription restart failed: {error}"),
    };
    assert!(matches!(restarted.resume_cursor(&target), Ok(Cursor(11))));
    assert_eq!(restarted.list(&subscription_scope).len(), 1);
    if let Err(error) = restarted.mark_delivered(&target, Cursor(12)) {
        panic!("unacknowledged cursor was not redeliverable after restart: {error}");
    }
    let delivered_before_restart = CursorAcknowledgement {
        scope: subscription_scope.clone(),
        subscription_id: id.clone(),
        cursor: ApiCursor(Sequence(12)),
    };
    if let Err(error) = restarted.acknowledge(&delivered_before_restart) {
        panic!("durable delivered cursor was forgotten: {error}");
    }
    let never_delivered = CursorAcknowledgement {
        scope: subscription_scope,
        subscription_id: id,
        cursor: ApiCursor(Sequence(13)),
    };
    assert!(matches!(
        restarted.acknowledge(&never_delivered),
        Err(SubscriptionError::CursorNeverDelivered { cursor: Cursor(13) })
    ));
    let _ = fs::remove_dir_all(root);
}

#[test]
fn legacy_subscription_without_a_session_generation_is_durably_quarantined() {
    let root = test_directory("legacy-session-binding");
    let tenant_id = durable_tenant("tenant-a");
    let subscription_scope = scope("tenant-a", "agent-a", "capability-a");
    let id = subscription_id("legacy-subscription");
    let target = subscription_target(&subscription_scope, &id);
    let durable = text(Store::open(&root), "store open");
    let mut subscriptions = text(
        SubscriptionStore::open(durable, tenant_id.clone()),
        "subscription store open",
    );
    text(
        subscriptions.create(
            id.clone(),
            request(subscription_scope.clone(), filter("tenant-a", "agent-a"), 0),
        ),
        "subscription create",
    );
    let mut durable = subscriptions.into_durable();
    rewrite_as_legacy_unbound(&mut durable, &tenant_id, &subscription_scope, &id);
    drop(durable);

    let restarted = text(
        SubscriptionStore::open(text(Store::open(&root), "store restart"), tenant_id.clone()),
        "legacy migration",
    );
    assert_eq!(
        text(restarted.termination(&target), "legacy termination"),
        Some(Termination::SessionRevoked)
    );
    assert!(matches!(
        restarted.get(&target),
        Err(SubscriptionError::NotFound)
    ));
    assert!(restarted.list(&subscription_scope).is_empty());
    let migrated = restarted
        .durable()
        .get(&subscription_key(&tenant_id, &id))
        .map(|stored| stored.bytes().to_vec())
        .unwrap_or_else(|| panic!("migrated subscription missing"));
    assert!(migrated.starts_with(b"LXS2"));
    drop(restarted);

    let restarted_again = text(
        SubscriptionStore::open(text(Store::open(&root), "store second restart"), tenant_id),
        "migrated restart",
    );
    assert_eq!(
        text(restarted_again.termination(&target), "migrated termination",),
        Some(Termination::SessionRevoked)
    );
    assert!(matches!(
        restarted_again.get(&target),
        Err(SubscriptionError::NotFound)
    ));
    let _ = fs::remove_dir_all(root);
}

#[test]
fn tenant_and_agent_scope_are_applied_before_any_filter() {
    let root = test_directory("scope");
    let tenant_id = durable_tenant("tenant-a");
    let subscription_scope = scope("tenant-a", "agent-a", "capability-a");
    let durable = match Store::open(&root) {
        Ok(value) => value,
        Err(error) => panic!("store open failed: {error}"),
    };
    let mut subscriptions = match SubscriptionStore::open(durable, tenant_id.clone()) {
        Ok(value) => value,
        Err(error) => panic!("subscription store open failed: {error}"),
    };

    let mut cross_tenant = filter("tenant-a", "agent-a");
    cross_tenant.accounts = ExplicitSet::allow(vec![tenant_object(
        "tenant-b",
        text(AccountRef::new("secret-account"), "account"),
    )]);
    assert!(matches!(
        subscriptions.create(
            subscription_id("cross-tenant"),
            request(subscription_scope.clone(), cross_tenant, 0),
        ),
        Err(SubscriptionError::InvalidFilter)
    ));

    let same_tenant_other_agent = filter("tenant-a", "agent-b");
    assert!(matches!(
        subscriptions.create(
            subscription_id("cross-agent"),
            request(subscription_scope.clone(), same_tenant_other_agent, 0),
        ),
        Err(SubscriptionError::InvalidFilter)
    ));
    assert!(subscriptions
        .into_durable()
        .list_object_ids(&tenant_id, ObjectKind::Subscription)
        .is_empty());
    let _ = fs::remove_dir_all(root);
}

#[test]
fn pause_resume_delete_and_cross_scope_non_disclosure_are_durable() {
    let root = test_directory("lifecycle");
    let tenant_id = durable_tenant("tenant-a");
    let subscription_scope = scope("tenant-a", "agent-a", "capability-a");
    let id = subscription_id("subscription-a");
    let target = subscription_target(&subscription_scope, &id);
    let durable = match Store::open(&root) {
        Ok(value) => value,
        Err(error) => panic!("store open failed: {error}"),
    };
    let mut subscriptions = match SubscriptionStore::open(durable, tenant_id.clone()) {
        Ok(value) => value,
        Err(error) => panic!("subscription store open failed: {error}"),
    };
    if let Err(error) = subscriptions.create(
        id.clone(),
        request(subscription_scope.clone(), filter("tenant-a", "agent-a"), 4),
    ) {
        panic!("subscription create failed: {error}");
    }
    let paused = match subscriptions.pause(&target) {
        Ok(value) => value,
        Err(error) => panic!("pause failed: {error}"),
    };
    assert!(paused.paused);
    assert!(matches!(
        subscriptions.mark_delivered(&target, Cursor(5)),
        Err(SubscriptionError::Paused)
    ));

    let wrong_scope = subscription_target(&scope("tenant-b", "agent-a", "capability-a"), &id);
    assert!(matches!(
        subscriptions.get(&wrong_scope),
        Err(SubscriptionError::NotFound)
    ));
    drop(subscriptions);

    let durable = match Store::open(&root) {
        Ok(value) => value,
        Err(error) => panic!("store restart failed: {error}"),
    };
    let mut restarted = match SubscriptionStore::open(durable, tenant_id.clone()) {
        Ok(value) => value,
        Err(error) => panic!("subscription restart failed: {error}"),
    };
    assert!(matches!(restarted.get(&target), Ok(record) if record.paused));
    let resumed = match restarted.resume(&target) {
        Ok(value) => value,
        Err(error) => panic!("resume failed: {error}"),
    };
    assert!(!resumed.paused);
    if let Err(error) = restarted.delete(&target) {
        panic!("delete failed: {error}");
    }
    drop(restarted);

    let durable = match Store::open(&root) {
        Ok(value) => value,
        Err(error) => panic!("final store restart failed: {error}"),
    };
    let final_store = match SubscriptionStore::open(durable, tenant_id) {
        Ok(value) => value,
        Err(error) => panic!("final subscription restart failed: {error}"),
    };
    assert!(final_store.list(&subscription_scope).is_empty());
    assert!(matches!(
        final_store.get(&target),
        Err(SubscriptionError::NotFound)
    ));
    let _ = fs::remove_dir_all(root);
}

fn session_identity(store: &mut Store) -> IdentityRecord {
    let mut boundary = BoundaryIdentity(CoreIdentity {
        canonical_bytes: b"subscription-identity".to_vec(),
        head_sequence: 10,
        revocation_sequence: 1,
        verification_level: VerificationLevel::STATE_PROVEN,
        frozen: false,
        authorities: vec![ProtocolAuthority::SessionKey([4; 32])],
    });
    text(
        register(
            store,
            durable_tenant("tenant-a"),
            text(Did::new(b"agent-a"), "agent DID"),
            &mut boundary,
        ),
        "identity",
    )
}

fn session(
    store: &mut Store,
    sessions: &mut SessionRegistry,
    identity: &IdentityRecord,
    id: u8,
) -> Token {
    text(
        open(
            store,
            sessions,
            identity,
            OpenRequest {
                session_id: SessionId([id; 32]),
                token_id: [id.wrapping_add(1); 32],
                tenant: durable_tenant("tenant-a"),
                agent: text(Did::new(b"agent-a"), "agent DID"),
                authority: ProtocolAuthority::SessionKey([4; 32]),
                permitted_activity_types: BTreeSet::from([9]),
                scopes: BTreeSet::from(["subscribe".to_owned()]),
                expiry_sequence: 100,
                opening_client: "subscription-suite".to_owned(),
                policy_version: "policy-v1".to_owned(),
            },
            10,
        ),
        "session",
    )
}

fn core_event(sequence: u64) -> CoreEvent {
    CoreEvent {
        global_sequence: sequence,
        canonical_bytes: format!("core-event-{sequence}").into_bytes(),
        receipt_reference: None,
        receipt_verification_level: VerificationLevel::UNVERIFIED,
        attributes: EventAttributes {
            agent: "agent-a".to_owned(),
            account: "account-7".to_owned(),
            activity_type: 9,
            module: "asset".to_owned(),
            asset: "LXR".to_owned(),
            counterparty: "counterparty-4".to_owned(),
            result_code: 0,
        },
    }
}

#[test]
fn closed_session_terminates_the_in_flight_stream_with_a_typed_revoked_event() {
    let root = test_directory("revoked-session");
    let tenant_id = durable_tenant("tenant-a");
    let subscription_scope = scope("tenant-a", "agent-a", "capability-a");
    let id = subscription_id("subscription-a");
    let target = subscription_target(&subscription_scope, &id);

    let mut session_store = text(Store::open(root.join("sessions")), "session store");
    let identity = session_identity(&mut session_store);
    let mut sessions = SessionRegistry::default();
    let token = session(&mut session_store, &mut sessions, &identity, 1);
    let sibling = session(&mut session_store, &mut sessions, &identity, 2);

    let events = text(Store::open(root.join("events")), "event store");
    let mut ingestor = text(
        EventIngestor::open(events, tenant_id.clone(), 64, 0),
        "ingestor",
    );
    for sequence in 0..3 {
        text(ingest(&mut ingestor, core_event(sequence)), "ingest");
    }
    let mut subscriptions = text(
        SubscriptionStore::open(ingestor.into_store(), tenant_id.clone()),
        "subscription store",
    );
    let mut observability = TenantObservability::default();
    text(
        subscriptions.create_authorized(
            &sessions,
            &token,
            &mut observability,
            11,
            id.clone(),
            request(subscription_scope.clone(), filter("tenant-a", "agent-a"), 0),
        ),
        "create",
    );
    assert!(subscriptions.list(&subscription_scope).is_empty());
    assert!(matches!(
        subscriptions.get(&target),
        Err(SubscriptionError::AuthorizationRequired)
    ));
    assert!(matches!(
        subscriptions.mark_delivered(&target, Cursor(1)),
        Err(SubscriptionError::AuthorizationRequired)
    ));
    assert!(matches!(
        subscriptions.acknowledge(&CursorAcknowledgement {
            scope: subscription_scope.clone(),
            subscription_id: id.clone(),
            cursor: ApiCursor(Sequence(0)),
        }),
        Err(SubscriptionError::AuthorizationRequired)
    ));
    assert!(matches!(
        subscriptions.pause(&target),
        Err(SubscriptionError::AuthorizationRequired)
    ));
    assert!(matches!(
        subscriptions.resume(&target),
        Err(SubscriptionError::AuthorizationRequired)
    ));
    assert!(matches!(
        subscriptions.delete(&target),
        Err(SubscriptionError::AuthorizationRequired)
    ));
    assert!(matches!(
        subscriptions.resume_cursor(&target),
        Err(SubscriptionError::AuthorizationRequired)
    ));
    assert!(matches!(
        subscriptions.continuity(&target),
        Err(SubscriptionError::AuthorizationRequired)
    ));
    assert!(matches!(
        subscriptions.block_gap(&target, 1, 2),
        Err(SubscriptionError::AuthorizationRequired)
    ));
    assert!(matches!(
        subscriptions.record_backfill(&target, Some(1)),
        Err(SubscriptionError::AuthorizationRequired)
    ));
    assert!(matches!(
        subscriptions.clear_gap(&target),
        Err(SubscriptionError::AuthorizationRequired)
    ));
    assert!(matches!(
        subscriptions.mark_truncated(&target, 0, 1, 0),
        Err(SubscriptionError::AuthorizationRequired)
    ));
    assert_eq!(
        text(
            subscriptions.list_authorized(
                &sessions,
                &token,
                &mut observability,
                11,
                &subscription_scope,
            ),
            "authorized list",
        ),
        vec![text(
            subscriptions.health_authorized(&sessions, &token, &mut observability, 11, &target,),
            "authorized health",
        )]
    );
    text(
        subscriptions.pause_authorized(&sessions, &token, &mut observability, 11, &target),
        "authorized pause",
    );
    text(
        subscriptions.resume_authorized(&sessions, &token, &mut observability, 11, &target),
        "authorized resume",
    );
    text(
        subscriptions.acknowledge_authorized(
            &sessions,
            &token,
            &mut observability,
            11,
            &CursorAcknowledgement {
                scope: subscription_scope.clone(),
                subscription_id: id.clone(),
                cursor: ApiCursor(Sequence(0)),
            },
        ),
        "authorized acknowledgement",
    );
    let disposable_id = subscription_id("subscription-disposable");
    let disposable_target = subscription_target(&subscription_scope, &disposable_id);
    text(
        subscriptions.create_authorized(
            &sessions,
            &token,
            &mut observability,
            11,
            disposable_id,
            request(subscription_scope.clone(), filter("tenant-a", "agent-a"), 0),
        ),
        "disposable create",
    );
    text(
        subscriptions.delete_authorized(
            &sessions,
            &token,
            &mut observability,
            11,
            &disposable_target,
        ),
        "authorized delete",
    );
    let retry_policy = RetryPolicy {
        base_delay_ms: 10,
        maximum_delay_ms: 100,
        jitter_percent: 20,
        maximum_attempts: 4,
    };
    assert!(matches!(
        DeliveryEngine::open(subscriptions, target.clone(), 3, 8, retry_policy,),
        Err(DeliveryError::AuthorizationRequired)
    ));
    let subscriptions = text(
        SubscriptionStore::open(
            text(Store::open(root.join("events")), "event store reopen"),
            tenant_id.clone(),
        ),
        "subscription store reopen",
    );
    let mut engine = text(
        DeliveryEngine::open_authorized(
            subscriptions,
            target.clone(),
            3,
            8,
            retry_policy,
            &mut sessions,
            token.clone(),
            &mut observability,
            11,
        ),
        "engine",
    );
    assert!(matches!(
        health(&engine),
        Err(DeliveryError::AuthorizationRequired)
    ));
    text(
        health_authorized(&mut engine, &sessions, 11, &mut observability),
        "authorized delivery health",
    );
    assert!(matches!(
        backfill(&mut engine),
        Err(DeliveryError::AuthorizationRequired)
    ));
    assert!(matches!(
        deliver(&mut engine),
        Err(DeliveryError::AuthorizationRequired)
    ));
    assert!(matches!(
        engine.accept_front(1),
        Err(DeliveryError::AuthorizationRequired)
    ));
    assert!(matches!(
        engine.acknowledge(&CursorAcknowledgement {
            scope: subscription_scope.clone(),
            subscription_id: id.clone(),
            cursor: ApiCursor(Sequence(0)),
        }),
        Err(DeliveryError::AuthorizationRequired)
    ));
    text(
        backfill_authorized(&mut engine, &sessions, 11, &mut observability),
        "backfill",
    );
    assert_eq!(engine.buffered_len(), 4);
    assert!(matches!(
        deliver_authorized(&mut engine, &sessions, 11, &mut observability),
        Ok(Some(_))
    ));
    assert!(matches!(
        engine.authorize_boundary(&sessions, Operation::ReadBalance, 11, &mut observability,),
        Err(DeliveryError::Authorization(
            AuthorizationError::InvalidRequest
        ))
    ));
    assert_eq!(
        engine.authorization_stop().and_then(|stop| stop.reason()),
        None
    );

    let metadata = text(
        fs::metadata(std::env::temp_dir()),
        "temporary directory metadata",
    );
    let socket_path = root.join("receiver.sock");
    let listener = text(UnixListener::bind(&socket_path), "receiver listener");
    let endpoint = text(
        Endpoint::new(
            socket_path,
            PeerIdentity {
                uid: metadata.uid(),
                gid: metadata.gid(),
            },
            4096,
            Duration::from_millis(500),
            Duration::from_millis(2),
        ),
        "endpoint",
    );
    let authenticator = text(
        Authenticator::new("receiver-key-v1", [0x8a; 32]),
        "authenticator",
    );
    assert!(matches!(
        deliver_outbound_unbound(
            &mut engine,
            &endpoint,
            &authenticator,
            &StopSignal::active(),
            99,
        ),
        Err(OutboundError::Delivery(
            DeliveryError::AuthorizationRequired
        ))
    ));
    let (received_tx, received_rx) = mpsc::channel();
    let (closed_tx, closed_rx) = mpsc::channel();
    let receiver = thread::spawn(move || {
        let (mut stream, _) = listener
            .accept()
            .unwrap_or_else(|error| panic!("receiver accept: {error}"));
        let frame = read_frame(&mut stream, 4096)
            .unwrap_or_else(|error| panic!("receiver frame: {error:?}"));
        assert!(!frame.is_empty());
        received_tx
            .send(())
            .unwrap_or_else(|error| panic!("receiver coordination: {error}"));
        closed_rx
            .recv()
            .unwrap_or_else(|error| panic!("close completion coordination: {error}"));
    });
    let sessions = Arc::new(RwLock::new(sessions));
    let revocation_sessions = Arc::clone(&sessions);
    let revocation_tenant = tenant_id.clone();
    let revoker = thread::spawn(move || {
        received_rx
            .recv()
            .unwrap_or_else(|error| panic!("revocation coordination: {error}"));
        let mut registry = revocation_sessions
            .write()
            .unwrap_or_else(|error| panic!("revocation registry: {error}"));
        close(
            &mut session_store,
            &mut registry,
            &revocation_tenant,
            SessionId([1; 32]),
        )
        .unwrap_or_else(|error| panic!("close: {error:?}"));
        drop(registry);
        closed_tx
            .send(())
            .unwrap_or_else(|error| panic!("close completion signal: {error}"));
    });
    assert!(matches!(
        deliver_outbound_authorized(
            &mut engine,
            &sessions,
            &endpoint,
            &authenticator,
            11,
            &mut observability,
            100,
        ),
        Err(OutboundError::Stopped(Termination::SessionRevoked))
    ));
    assert!(revoker.join().is_ok(), "revoker panicked");
    assert!(receiver.join().is_ok(), "receiver panicked");
    assert_eq!(engine.buffered_len(), 0);
    assert_eq!(
        engine.authorization_stop().and_then(|stop| stop.reason()),
        Some(Termination::SessionRevoked)
    );
    {
        let registry = sessions
            .read()
            .unwrap_or_else(|error| panic!("session registry: {error}"));
        assert!(matches!(
            deliver_authorized(&mut engine, &registry, 11, &mut observability),
            Err(DeliveryError::Revoked)
        ));
    }
    assert!(matches!(
        deliver_outbound_authorized(
            &mut engine,
            &sessions,
            &endpoint,
            &authenticator,
            11,
            &mut observability,
            200,
        ),
        Err(OutboundError::Stopped(Termination::SessionRevoked))
    ));
    assert_eq!(engine.buffered_len(), 0);

    let sessions = sessions
        .read()
        .unwrap_or_else(|error| panic!("session registry: {error}"));
    assert_eq!(
        sibling.authorize(&sessions, &tenant_id, identity.did(), "subscribe", 11),
        Ok(SessionId([2; 32]))
    );
    assert_eq!(sessions.generation(&tenant_id, SessionId([2; 32])), Some(1));

    let subscriptions = engine.into_subscriptions();
    assert_eq!(
        text(subscriptions.termination(&target), "termination"),
        Some(Termination::SessionRevoked)
    );
    assert!(matches!(
        subscriptions.get(&target),
        Err(SubscriptionError::AuthorizationRequired)
    ));
    assert!(matches!(
        subscriptions.health_authorized(&sessions, &token, &mut observability, 11, &target),
        Err(SubscriptionError::Authorization(
            AuthorizationError::Revoked
        ))
    ));
    assert!(matches!(
        subscriptions.health_authorized(&sessions, &sibling, &mut observability, 11, &target),
        Err(SubscriptionError::Authorization(
            AuthorizationError::NotAuthorized
        ))
    ));
    drop(subscriptions);

    let restarted = text(
        SubscriptionStore::open(
            text(Store::open(root.join("events")), "event store restart"),
            tenant_id,
        ),
        "subscription store restart",
    );
    assert_eq!(
        text(restarted.termination(&target), "termination after restart"),
        Some(Termination::SessionRevoked)
    );
    assert!(matches!(
        restarted.get(&target),
        Err(SubscriptionError::AuthorizationRequired)
    ));
    assert!(matches!(
        restarted.health_authorized(&sessions, &token, &mut observability, 11, &target),
        Err(SubscriptionError::Authorization(
            AuthorizationError::Revoked
        ))
    ));
    let _ = fs::remove_dir_all(root);
}
