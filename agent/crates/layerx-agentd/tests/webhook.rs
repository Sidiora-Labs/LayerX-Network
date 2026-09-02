use std::fs;
use std::os::unix::fs::MetadataExt;
use std::os::unix::net::UnixListener;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc;
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
    deliver as deliver_outbound, Authenticator, Endpoint, EndpointFailure, OutboundError,
    PeerIdentity, StopSignal, VerifiedItem,
};
use layerx_agentd::events::subscription::{Store as SubscriptionStore, Termination};
use layerx_agentd::events::{
    health as event_health, ingest, CoreEvent, DeliveryEngine, DeliveryItem, EventAttributes,
    EventIngestor, RetryPolicy,
};
use layerx_agentd::store::{Store, TenantId};
use layerx_client::lni::framing::{read_frame, write_frame};
use layerx_types::result::ResultCode;
use layerx_types::verify::VerificationLevel;

static NEXT_PATH: AtomicU64 = AtomicU64::new(1);
const KEY: [u8; 32] = [0x8a; 32];

struct SocketPath(PathBuf);

impl SocketPath {
    fn new(label: &str) -> Self {
        let sequence = NEXT_PATH.fetch_add(1, Ordering::Relaxed);
        Self(std::env::temp_dir().join(format!(
            "layerx-agentd-webhook-{label}-{}-{sequence}.sock",
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
        "layerx-agentd-webhook-{name}-{}-{sequence}",
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

fn api_tenant() -> ApiTenantId {
    text(ApiTenantId::new("tenant-a"), "API tenant")
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

fn engine(root: &PathBuf) -> DeliveryEngine {
    let durable = match Store::open(root) {
        Ok(value) => value,
        Err(error) => panic!("store open failed: {error}"),
    };
    let mut ingestor = match EventIngestor::open(durable, durable_tenant(), 2, 0) {
        Ok(value) => value,
        Err(error) => panic!("ingestor open failed: {error}"),
    };
    let event = CoreEvent {
        global_sequence: 0,
        canonical_bytes: b"exact-core-event-zero".to_vec(),
        receipt_reference: Some([0x51; 32]),
        receipt_verification_level: VerificationLevel::SEQUENCER_SIGNED,
        attributes: EventAttributes {
            agent: "agent-a".to_owned(),
            account: "account-a".to_owned(),
            activity_type: 9,
            module: "asset".to_owned(),
            asset: "LXR".to_owned(),
            counterparty: "counterparty-a".to_owned(),
            result_code: 0,
        },
    };
    if let Err(error) = ingest(&mut ingestor, event) {
        panic!("event ingest failed: {error}");
    }
    let mut subscriptions = match SubscriptionStore::open(ingestor.into_store(), durable_tenant()) {
        Ok(value) => value,
        Err(error) => panic!("subscription store open failed: {error}"),
    };
    let create = SubscriptionCreate {
        scope: scope(),
        filter: filter(),
        start: ApiCursor(Sequence(0)),
        delivery_target: text(DeliveryTarget::new("uds://receiver"), "target"),
    };
    if let Err(error) = subscriptions.create(id(), create) {
        panic!("subscription create failed: {error}");
    }
    let retry = RetryPolicy {
        base_delay_ms: 10,
        maximum_delay_ms: 100,
        jitter_percent: 20,
        maximum_attempts: 4,
    };
    match DeliveryEngine::open(subscriptions, target(), 1, 2, retry) {
        Ok(value) => value,
        Err(error) => panic!("delivery engine open failed: {error}"),
    }
}

fn peer_for_socket(path: &PathBuf) -> PeerIdentity {
    let metadata = match fs::metadata(path) {
        Ok(value) => value,
        Err(error) => panic!("socket metadata failed: {error}"),
    };
    PeerIdentity {
        uid: metadata.uid(),
        gid: metadata.gid(),
    }
}

fn endpoint(path: &PathBuf, peer: PeerIdentity, deadline_ms: u64) -> Endpoint {
    match Endpoint::new(
        path,
        peer,
        4096,
        Duration::from_millis(deadline_ms),
        Duration::from_millis(2),
    ) {
        Ok(value) => value,
        Err(error) => panic!("endpoint configuration failed: {error}"),
    }
}

fn authenticator() -> Authenticator {
    match Authenticator::new("receiver-key-v1", KEY) {
        Ok(value) => value,
        Err(error) => panic!("authenticator failed: {error}"),
    }
}

#[test]
fn unreachable_endpoint_schedules_retry_and_preserves_health_cursor() {
    let root = test_directory("unreachable");
    let socket = SocketPath::new("missing");
    let metadata = match fs::metadata(std::env::temp_dir()) {
        Ok(value) => value,
        Err(error) => panic!("temporary directory metadata failed: {error}"),
    };
    let peer = PeerIdentity {
        uid: metadata.uid(),
        gid: metadata.gid(),
    };
    let endpoint = endpoint(&socket.0, peer, 30);
    let mut delivery = engine(&root);
    assert!(matches!(
        deliver_outbound(
            &mut delivery,
            &endpoint,
            &authenticator(),
            &StopSignal::active(),
            100,
        ),
        Err(OutboundError::RetryScheduled {
            failure: EndpointFailure::Unreachable(_),
            ..
        })
    ));
    let health = event_health(&delivery).unwrap_or_else(|error| panic!("event health: {error}"));
    assert_eq!(health.acknowledged_cursor.0, 0);
    assert_eq!(health.lag_sequences, 1);
    assert_eq!(health.failure_count, 1);
    assert!(health.lagging);
    assert!(health.last_error.is_some());
    let _ = fs::remove_dir_all(root);
}

#[test]
fn slow_endpoint_receives_byte_identical_retry_and_bound_event_evidence() {
    let root = test_directory("slow");
    let socket = SocketPath::new("slow");
    let listener = match UnixListener::bind(&socket.0) {
        Ok(value) => value,
        Err(error) => panic!("listener bind failed: {error}"),
    };
    let peer = peer_for_socket(&socket.0);
    let server = thread::spawn(move || {
        let auth = authenticator();
        let (mut first, _) = match listener.accept() {
            Ok(value) => value,
            Err(error) => panic!("first accept failed: {error}"),
        };
        let first_frame = match read_frame(&mut first, 4096) {
            Ok(value) => value,
            Err(error) => panic!("first frame failed: {error:?}"),
        };
        let verified = match auth.verify(&first_frame, peer) {
            Ok(value) => value,
            Err(error) => panic!("first authentication failed: {error}"),
        };
        match &verified.item {
            VerifiedItem::Event {
                global_sequence,
                receipt_reference,
                event_bytes,
                ..
            } => {
                assert_eq!(*global_sequence, 0);
                assert!(receipt_reference.is_some());
                assert_eq!(event_bytes, b"exact-core-event-zero");
            }
            other @ VerifiedItem::BackfillComplete { .. } => {
                panic!("unexpected outbound item: {other:?}")
            }
        }
        thread::sleep(Duration::from_millis(90));
        drop(first);

        let (mut second, _) = match listener.accept() {
            Ok(value) => value,
            Err(error) => panic!("second accept failed: {error}"),
        };
        let second_frame = match read_frame(&mut second, 4096) {
            Ok(value) => value,
            Err(error) => panic!("second frame failed: {error:?}"),
        };
        assert_eq!(second_frame, first_frame);
        let verified_again = match auth.verify(&second_frame, peer) {
            Ok(value) => value,
            Err(error) => panic!("retry authentication failed: {error}"),
        };
        assert_eq!(verified_again.binding, verified.binding);
        let acknowledgement = auth.acknowledgement(verified_again.binding);
        if let Err(error) = write_frame(&mut second, &acknowledgement, 4096) {
            panic!("acknowledgement failed: {error:?}");
        }
    });

    let endpoint = endpoint(&socket.0, peer, 30);
    let mut delivery = engine(&root);
    let stop = StopSignal::active();
    assert!(matches!(
        deliver_outbound(&mut delivery, &endpoint, &authenticator(), &stop, 100,),
        Err(OutboundError::RetryScheduled {
            failure: EndpointFailure::Timeout | EndpointFailure::Protocol,
            ..
        })
    ));
    thread::sleep(Duration::from_millis(90));
    let receipt = match deliver_outbound(&mut delivery, &endpoint, &authenticator(), &stop, 300) {
        Ok(value) => value,
        Err(error) => panic!("retry delivery failed: {error}"),
    };
    assert!(server.join().is_ok(), "receiver server panicked");
    let cursor = match receipt.item {
        DeliveryItem::Event(event) => event.delivery.cursor,
        other @ DeliveryItem::BackfillComplete(_) => {
            panic!("unexpected receipt item: {other:?}")
        }
    };
    let acknowledgement = CursorAcknowledgement {
        scope: scope(),
        subscription_id: id(),
        cursor,
    };
    if let Err(error) = delivery.acknowledge(&acknowledgement) {
        panic!("consumer cursor acknowledgement failed: {error}");
    }
    let health = event_health(&delivery).unwrap_or_else(|error| panic!("event health: {error}"));
    assert_eq!(health.acknowledged_cursor.0, 1);
    assert_eq!(health.lag_sequences, 0);
    assert_eq!(health.failure_count, 1);
    assert_eq!(health.last_delivery_at_ms, Some(300));
    let _ = fs::remove_dir_all(root);
}

#[test]
fn capability_revocation_midflight_cancels_ack_wait_and_persists_stop() {
    let root = test_directory("revoked");
    let socket = SocketPath::new("revoked");
    let listener = match UnixListener::bind(&socket.0) {
        Ok(value) => value,
        Err(error) => panic!("listener bind failed: {error}"),
    };
    let peer = peer_for_socket(&socket.0);
    let (received_tx, received_rx) = mpsc::channel();
    let server = thread::spawn(move || {
        let auth = authenticator();
        let (mut stream, _) = match listener.accept() {
            Ok(value) => value,
            Err(error) => panic!("accept failed: {error}"),
        };
        let frame = match read_frame(&mut stream, 4096) {
            Ok(value) => value,
            Err(error) => panic!("frame failed: {error:?}"),
        };
        let verified = match auth.verify(&frame, peer) {
            Ok(value) => value,
            Err(error) => panic!("authentication failed: {error}"),
        };
        assert!(
            received_tx.send(()).is_ok(),
            "revocation coordination failed"
        );
        thread::sleep(Duration::from_millis(100));
        let acknowledgement = auth.acknowledgement(verified.binding);
        let _ = write_frame(&mut stream, &acknowledgement, 4096);
    });

    let mut delivery = engine(&root);
    let endpoint = endpoint(&socket.0, peer, 500);
    let stop = StopSignal::active();
    let stopper = stop.clone();
    let revoker = thread::spawn(move || {
        assert!(
            received_rx.recv().is_ok(),
            "receiver never observed delivery"
        );
        stopper.stop(Termination::CapabilityRevoked);
    });
    assert!(matches!(
        deliver_outbound(&mut delivery, &endpoint, &authenticator(), &stop, 100,),
        Err(OutboundError::Stopped(Termination::CapabilityRevoked))
    ));
    assert!(revoker.join().is_ok(), "revoker panicked");
    assert!(server.join().is_ok(), "receiver server panicked");
    assert_eq!(delivery.buffered_len(), 0);
    assert!(matches!(
        deliver_outbound(&mut delivery, &endpoint, &authenticator(), &stop, 200,),
        Err(OutboundError::Stopped(Termination::CapabilityRevoked))
    ));
    let subscriptions = delivery.into_subscriptions();
    assert!(matches!(
        subscriptions.termination(&target()),
        Ok(Some(Termination::CapabilityRevoked))
    ));
    assert!(subscriptions.get(&target()).is_err());
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
        restarted.termination(&target()),
        Ok(Some(Termination::CapabilityRevoked))
    ));
    let _ = fs::remove_dir_all(root);
}

#[test]
fn deletion_prevents_connection_and_retains_tombstone_audit() {
    let root = test_directory("deleted");
    let socket = SocketPath::new("deleted");
    let listener = match UnixListener::bind(&socket.0) {
        Ok(value) => value,
        Err(error) => panic!("listener bind failed: {error}"),
    };
    if let Err(error) = listener.set_nonblocking(true) {
        panic!("listener nonblocking failed: {error}");
    }
    let peer = peer_for_socket(&socket.0);
    let endpoint = endpoint(&socket.0, peer, 100);
    let mut delivery = engine(&root);
    let stop = StopSignal::active();
    stop.stop(Termination::Deleted);
    assert!(matches!(
        deliver_outbound(&mut delivery, &endpoint, &authenticator(), &stop, 100,),
        Err(OutboundError::Stopped(Termination::Deleted))
    ));
    assert!(
        matches!(listener.accept(), Err(error) if error.kind() == std::io::ErrorKind::WouldBlock)
    );
    let subscriptions = delivery.into_subscriptions();
    assert!(subscriptions.list(&scope()).is_empty());
    assert!(subscriptions.get(&target()).is_err());
    assert!(matches!(
        subscriptions.termination(&target()),
        Ok(Some(Termination::Deleted))
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
        restarted.termination(&target()),
        Ok(Some(Termination::Deleted))
    ));
    let _ = fs::remove_dir_all(root);
}
