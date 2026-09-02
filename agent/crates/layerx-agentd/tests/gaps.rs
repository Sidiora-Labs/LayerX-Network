use std::collections::BTreeSet;
use std::fs;
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::thread;
use std::time::Duration;

use layerx_agent_api::identity::{AgentDid, CapabilityId, ExplicitSet, TenantId as ApiTenantId};
use layerx_agent_api::subscription::{
    Cursor as ApiCursor, DeliveryTarget, SubscriptionCreate, SubscriptionFilter, SubscriptionId,
    SubscriptionScope, SubscriptionTarget,
};
use layerx_agent_api::Sequence;
use layerx_agentd::events::gap::{
    admit, admit_authorized, apply_backfill, apply_backfill_authorized, attempt_backfill, detect,
    detect_authorized, enforce_retention, enforce_retention_authorized, BackfillFailure,
    BackfillReport, BackfillResolution, GapError, Retention, Truncated,
};
use layerx_agentd::events::subscription::{
    Continuity, Store as SubscriptionStore, SubscriptionError,
};
use layerx_agentd::events::{
    backfill, ingest, CoreEvent, DeliveryEngine, DeliveryError, EventAttributes, EventIngestor,
    RetryPolicy,
};
use layerx_agentd::identity::{
    register, CoreIdentity, IdentityError, IdentityRecord, IdentityResolver, ProtocolAuthority,
};
use layerx_agentd::session::{close, open, OpenRequest, SessionId, SessionRegistry, Token};
use layerx_agentd::store::{Store, TenantId};
use layerx_agentd::tenant::{AuthorizationError, TenantObservability};
use layerx_client::head::Head;
use layerx_client::lni::framing::{read_frame, write_frame};
use layerx_client::lni::schema::{decode_envelope, encode_envelope, Envelope, Version};
use layerx_client::lni::transport::{ConnectionGate, Limits, Uds};
use layerx_client::stream::{Cursor as ClientCursor, StreamConfig};
use layerx_types::ids::Did;
use layerx_types::verify::VerificationLevel;

static NEXT_PATH: AtomicU64 = AtomicU64::new(1);

struct BoundaryIdentity(CoreIdentity);

impl IdentityResolver for BoundaryIdentity {
    fn resolve(&mut self, _did: &Did) -> Result<Option<CoreIdentity>, IdentityError> {
        Ok(Some(self.0.clone()))
    }
}

struct SocketPath(PathBuf);

impl SocketPath {
    fn new(label: &str) -> Self {
        let sequence = NEXT_PATH.fetch_add(1, Ordering::Relaxed);
        Self(std::env::temp_dir().join(format!(
            "layerx-agentd-gap-{label}-{}-{sequence}.sock",
            std::process::id()
        )))
    }
}

impl Drop for SocketPath {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.0);
    }
}

fn test_directory(name: &str) -> PathBuf {
    let sequence = NEXT_PATH.fetch_add(1, Ordering::Relaxed);
    std::env::temp_dir().join(format!(
        "layerx-agentd-gap-{name}-{}-{sequence}",
        std::process::id()
    ))
}

fn text<T, E: std::fmt::Debug>(result: Result<T, E>, label: &str) -> T {
    match result {
        Ok(value) => value,
        Err(error) => panic!("{label} must be valid: {error:?}"),
    }
}

fn durable_tenant() -> TenantId {
    text(TenantId::new("tenant-a"), "durable tenant")
}

fn scope() -> SubscriptionScope {
    SubscriptionScope {
        tenant: text(ApiTenantId::new("tenant-a"), "API tenant"),
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
        agents: ExplicitSet::deny_all(),
        accounts: ExplicitSet::deny_all(),
        activity_types: ExplicitSet::deny_all(),
        modules: ExplicitSet::deny_all(),
        assets: ExplicitSet::deny_all(),
        counterparties: ExplicitSet::deny_all(),
        result_classes: ExplicitSet::deny_all(),
    }
}

fn subscriptions(root: &PathBuf, start: u64) -> SubscriptionStore {
    let durable = match Store::open(root) {
        Ok(value) => value,
        Err(error) => panic!("store open failed: {error}"),
    };
    let mut subscriptions = match SubscriptionStore::open(durable, durable_tenant()) {
        Ok(value) => value,
        Err(error) => panic!("subscription store open failed: {error}"),
    };
    let create = SubscriptionCreate {
        scope: scope(),
        filter: filter(),
        start: ApiCursor(Sequence(start)),
        delivery_target: text(DeliveryTarget::new("uds://consumer-a"), "target"),
    };
    if let Err(error) = subscriptions.create(id(), create) {
        panic!("subscription create failed: {error}");
    }
    subscriptions
}

fn session_identity(store: &mut Store) -> IdentityRecord {
    let mut boundary = BoundaryIdentity(CoreIdentity {
        canonical_bytes: b"gap-session-identity".to_vec(),
        head_sequence: 10,
        revocation_sequence: 1,
        verification_level: VerificationLevel::STATE_PROVEN,
        frozen: false,
        authorities: vec![ProtocolAuthority::SessionKey([4; 32])],
    });
    text(
        register(
            store,
            durable_tenant(),
            text(Did::new(b"agent-a"), "agent DID"),
            &mut boundary,
        ),
        "session identity",
    )
}

fn authorized_subscriptions(
    root: &PathBuf,
    start: u64,
) -> (SubscriptionStore, Store, SessionRegistry, Token) {
    let mut session_store = text(Store::open(root.join("sessions")), "session store");
    let identity = session_identity(&mut session_store);
    let mut sessions = SessionRegistry::default();
    let token = text(
        open(
            &mut session_store,
            &mut sessions,
            &identity,
            OpenRequest {
                session_id: SessionId([1; 32]),
                token_id: [2; 32],
                tenant: durable_tenant(),
                agent: text(Did::new(b"agent-a"), "agent DID"),
                authority: ProtocolAuthority::SessionKey([4; 32]),
                permitted_activity_types: BTreeSet::from([9]),
                scopes: BTreeSet::from(["subscribe".to_owned()]),
                expiry_sequence: 100,
                opening_client: "gap-suite".to_owned(),
                policy_version: "policy-v1".to_owned(),
            },
            10,
        ),
        "session",
    );
    let durable = text(Store::open(root.join("events")), "event store");
    let mut subscriptions = text(
        SubscriptionStore::open(durable, durable_tenant()),
        "subscription store",
    );
    text(
        subscriptions.create_authorized(
            &sessions,
            &token,
            &mut TenantObservability::default(),
            10,
            id(),
            SubscriptionCreate {
                scope: scope(),
                filter: filter(),
                start: ApiCursor(Sequence(start)),
                delivery_target: text(DeliveryTarget::new("uds://consumer-a"), "target"),
            },
        ),
        "authorized subscription",
    );
    (subscriptions, session_store, sessions, token)
}

fn limits() -> Limits {
    Limits {
        maximum_frame_bytes: 1024 * 1024,
        maximum_connections: 1,
        maximum_streams: 2,
        maximum_queued_bytes: 1024 * 1024,
        deadline: Duration::from_secs(2),
    }
}

fn stream_config() -> StreamConfig {
    StreamConfig {
        interface_version: Version::V1_0,
        correlation_id: 77,
        maximum_buffered_events: 4,
        maximum_buffered_bytes: 1024,
        maximum_heartbeats_per_poll: 2,
    }
}

fn head() -> Head {
    Head {
        chain_sequence: 20,
        sealed_batch: 2,
        finalised_checkpoint: [0x71; 32],
    }
}

fn receive_subscription(stream: &mut UnixStream) -> ClientCursor {
    let frame = match read_frame(stream, 1024 * 1024) {
        Ok(value) => value,
        Err(error) => panic!("subscription frame failed: {error:?}"),
    };
    let envelope = match decode_envelope(&frame) {
        Ok(value) => value,
        Err(error) => panic!("subscription envelope failed: {error:?}"),
    };
    assert_eq!(envelope.message_tag, 21);
    let Some(cursor) = envelope.canonical_payload.get(..48) else {
        panic!("subscription cursor missing")
    };
    match ClientCursor::decode(cursor) {
        Ok(value) => value,
        Err(error) => panic!("subscription cursor invalid: {error:?}"),
    }
}

fn response(stream: &mut UnixStream, tag: u16, payload: &[u8]) {
    let encoded = match encode_envelope(Envelope {
        version: Version::V1_0,
        message_tag: tag,
        correlation_id: 77,
        canonical_payload: payload,
        proof_material: &[],
    }) {
        Ok(value) => value,
        Err(error) => panic!("response encoding failed: {error:?}"),
    };
    if let Err(error) = write_frame(stream, &encoded, 1024 * 1024) {
        panic!("response write failed: {error:?}");
    }
}

fn event(stream: &mut UnixStream, sequence: u64, canonical_bytes: &[u8]) {
    let mut payload = sequence.to_be_bytes().to_vec();
    payload.extend_from_slice(canonical_bytes);
    response(stream, 22, &payload);
}

fn gap_response(stream: &mut UnixStream, first: u64, last: u64) {
    let mut payload = first.to_be_bytes().to_vec();
    payload.extend_from_slice(&last.to_be_bytes());
    response(stream, 23, &payload);
}

fn recovered_event(sequence: u64, bytes: Vec<u8>) -> CoreEvent {
    CoreEvent {
        global_sequence: sequence,
        canonical_bytes: bytes,
        receipt_reference: None,
        receipt_verification_level: VerificationLevel::UNVERIFIED,
        attributes: EventAttributes {
            agent: "agent-a".to_owned(),
            account: "account-a".to_owned(),
            activity_type: 9,
            module: "asset".to_owned(),
            asset: "LXR".to_owned(),
            counterparty: "counterparty-a".to_owned(),
            result_code: 0,
        },
    }
}

#[test]
fn skipped_sequence_is_backfilled_over_the_real_stream_before_unblocking() {
    let root = test_directory("recovered");
    let mut subscriptions = subscriptions(&root, 11);
    let gap = match detect(&mut subscriptions, &target(), 11, 13) {
        Ok(Some(value)) => value,
        other => panic!("gap was not detected: {other:?}"),
    };
    assert!(
        matches!(admit(&subscriptions, &target()), Err(GapError::Blocked(value)) if value == gap)
    );

    let socket = SocketPath::new("recovered");
    let listener = match UnixListener::bind(&socket.0) {
        Ok(value) => value,
        Err(error) => panic!("listener bind failed: {error}"),
    };
    let server = thread::spawn(move || {
        let (mut stream, _) = match listener.accept() {
            Ok(value) => value,
            Err(error) => panic!("accept failed: {error}"),
        };
        assert_eq!(receive_subscription(&mut stream).next_sequence(), 11);
        event(&mut stream, 11, b"core-event-eleven");
        event(&mut stream, 12, b"core-event-twelve");
    });
    let gate = ConnectionGate::new(1);
    let mut transport = match Uds::connect(&socket.0, &gate, limits()) {
        Ok(value) => value,
        Err(error) => panic!("core connection failed: {error:?}"),
    };
    let report = attempt_backfill(&mut transport, gap, head(), stream_config(), 2);
    assert!(server.join().is_ok(), "core stream server panicked");
    let recovered = match &report {
        BackfillReport::Recovered { events, .. } => events.clone(),
        other @ BackfillReport::Incomplete { .. } => {
            panic!("backfill did not recover: {other:?}")
        }
    };

    let durable = subscriptions.into_durable();
    let mut ingestor = match EventIngestor::open(durable, durable_tenant(), 2, 11) {
        Ok(value) => value,
        Err(error) => panic!("ingestor open failed: {error}"),
    };
    for recovered in recovered {
        if let Err(error) = ingest(
            &mut ingestor,
            recovered_event(recovered.global_sequence, recovered.canonical_bytes),
        ) {
            panic!("recovered event persistence failed: {error}");
        }
    }
    let mut subscriptions = match SubscriptionStore::open(ingestor.into_store(), durable_tenant()) {
        Ok(value) => value,
        Err(error) => panic!("subscription reopen failed: {error}"),
    };
    assert!(matches!(
        apply_backfill(&mut subscriptions, &target(), gap, &report),
        Ok(BackfillResolution::Restored)
    ));
    assert!(matches!(
        subscriptions.continuity(&target()),
        Ok(Continuity::Healthy)
    ));
    assert!(admit(&subscriptions, &target()).is_ok());

    drop(subscriptions);
    let durable = match Store::open(&root) {
        Ok(value) => value,
        Err(error) => panic!("store restart failed: {error}"),
    };
    let restarted = match SubscriptionStore::open(durable, durable_tenant()) {
        Ok(value) => value,
        Err(error) => panic!("subscription restart failed: {error}"),
    };
    assert!(matches!(
        restarted.continuity(&target()),
        Ok(Continuity::Healthy)
    ));
    let _ = fs::remove_dir_all(root);
}

#[test]
fn unrecoverable_core_gap_remains_durably_blocked() {
    let root = test_directory("unrecoverable");
    let mut subscriptions = subscriptions(&root, 11);
    let gap = match detect(&mut subscriptions, &target(), 11, 13) {
        Ok(Some(value)) => value,
        other => panic!("gap was not detected: {other:?}"),
    };
    let socket = SocketPath::new("unrecoverable");
    let listener = match UnixListener::bind(&socket.0) {
        Ok(value) => value,
        Err(error) => panic!("listener bind failed: {error}"),
    };
    let server = thread::spawn(move || {
        let (mut stream, _) = match listener.accept() {
            Ok(value) => value,
            Err(error) => panic!("accept failed: {error}"),
        };
        let _ = receive_subscription(&mut stream);
        gap_response(&mut stream, 11, 12);
    });
    let gate = ConnectionGate::new(1);
    let mut transport = match Uds::connect(&socket.0, &gate, limits()) {
        Ok(value) => value,
        Err(error) => panic!("core connection failed: {error:?}"),
    };
    let report = attempt_backfill(&mut transport, gap, head(), stream_config(), 2);
    assert!(server.join().is_ok(), "core stream server panicked");
    assert!(matches!(
        &report,
        BackfillReport::Incomplete {
            failure: BackfillFailure::CoreReportedGap {
                missing_first: 11,
                missing_last: 12,
            },
            ..
        }
    ));
    assert!(matches!(
        apply_backfill(&mut subscriptions, &target(), gap, &report),
        Ok(BackfillResolution::StillBlocked {
            recovered_through: None,
        })
    ));
    assert!(matches!(
        subscriptions.continuity(&target()),
        Ok(Continuity::GapBlocked {
            backfill_attempted: true,
            recovered_through: None,
            ..
        })
    ));

    let retry = RetryPolicy {
        base_delay_ms: 10,
        maximum_delay_ms: 20,
        jitter_percent: 0,
        maximum_attempts: 2,
    };
    let mut delivery = match DeliveryEngine::open(subscriptions, target(), 13, 2, retry) {
        Ok(value) => value,
        Err(error) => panic!("delivery engine open failed: {error}"),
    };
    assert!(matches!(
        backfill(&mut delivery),
        Err(DeliveryError::ContinuityBlocked)
    ));
    let _ = fs::remove_dir_all(root);
}

#[test]
fn retention_expiry_marks_outage_truncated_and_survives_restart() {
    let root = test_directory("truncated");
    let mut subscriptions = subscriptions(&root, 10);
    let _ = detect(&mut subscriptions, &target(), 10, 15);
    let notice = match enforce_retention(
        &mut subscriptions,
        &target(),
        31,
        15,
        Retention {
            maximum_undelivered_sequences: 10,
        },
    ) {
        Ok(Some(value)) => value,
        other => panic!("retention did not truncate: {other:?}"),
    };
    assert_eq!(
        notice,
        Truncated {
            requested_from: 10,
            oldest_available: 21,
            lost_through: 20,
        }
    );
    assert!(matches!(
        admit(&subscriptions, &target()),
        Err(GapError::Truncated(value)) if value == notice
    ));
    drop(subscriptions);

    let durable = match Store::open(&root) {
        Ok(value) => value,
        Err(error) => panic!("store restart failed: {error}"),
    };
    let restarted = match SubscriptionStore::open(durable, durable_tenant()) {
        Ok(value) => value,
        Err(error) => panic!("subscription restart failed: {error}"),
    };
    assert!(matches!(
        restarted.continuity(&target()),
        Ok(Continuity::Truncated {
            requested_from: 10,
            oldest_available: 21,
            lost_through: 20,
        })
    ));
    let _ = fs::remove_dir_all(root);
}

#[test]
fn bound_gap_runtime_requires_the_common_resolver_and_refuses_after_revocation() {
    let root = test_directory("authorized");
    let (mut subscriptions, mut session_store, mut sessions, token) =
        authorized_subscriptions(&root, 11);
    let mut observability = TenantObservability::default();

    assert!(matches!(
        detect(&mut subscriptions, &target(), 11, 13),
        Err(GapError::Subscription(
            SubscriptionError::AuthorizationRequired
        ))
    ));
    let gap = match detect_authorized(
        &mut subscriptions,
        &sessions,
        &token,
        &mut observability,
        11,
        &target(),
        11,
        13,
    ) {
        Ok(Some(value)) => value,
        other => panic!("authorized gap was not detected: {other:?}"),
    };
    assert!(matches!(
        admit(&subscriptions, &target()),
        Err(GapError::Subscription(
            SubscriptionError::AuthorizationRequired
        ))
    ));
    assert!(matches!(
        admit_authorized(
            &subscriptions,
            &sessions,
            &token,
            &mut observability,
            11,
            &target(),
        ),
        Err(GapError::Blocked(value)) if value == gap
    ));
    let report = BackfillReport::Incomplete {
        gap,
        recovered: Vec::new(),
        failure: BackfillFailure::Incomplete,
    };
    assert!(matches!(
        apply_backfill(&mut subscriptions, &target(), gap, &report),
        Err(GapError::Subscription(
            SubscriptionError::AuthorizationRequired
        ))
    ));
    assert!(matches!(
        apply_backfill_authorized(
            &mut subscriptions,
            &sessions,
            &token,
            &mut observability,
            11,
            &target(),
            gap,
            &report,
        ),
        Ok(BackfillResolution::StillBlocked {
            recovered_through: None,
        })
    ));

    text(
        close(
            &mut session_store,
            &mut sessions,
            &durable_tenant(),
            SessionId([1; 32]),
        ),
        "session close",
    );
    assert!(matches!(
        admit_authorized(
            &subscriptions,
            &sessions,
            &token,
            &mut observability,
            11,
            &target(),
        ),
        Err(GapError::Subscription(SubscriptionError::Authorization(
            AuthorizationError::Revoked
        )))
    ));
    assert_eq!(observability.audit().len(), 4);

    let retention_root = root.join("retention");
    let (mut retention, _store, sessions, token) = authorized_subscriptions(&retention_root, 10);
    let mut retention_observability = TenantObservability::default();
    assert!(matches!(
        enforce_retention(
            &mut retention,
            &target(),
            31,
            15,
            Retention {
                maximum_undelivered_sequences: 10,
            },
        ),
        Err(GapError::Subscription(
            SubscriptionError::AuthorizationRequired
        ))
    ));
    assert!(matches!(
        enforce_retention_authorized(
            &mut retention,
            &sessions,
            &token,
            &mut retention_observability,
            11,
            &target(),
            31,
            15,
            Retention {
                maximum_undelivered_sequences: 10,
            },
        ),
        Ok(Some(Truncated {
            requested_from: 10,
            oldest_available: 21,
            lost_through: 20,
        }))
    ));
    assert_eq!(retention_observability.audit().len(), 1);
    let _ = fs::remove_dir_all(root);
}
