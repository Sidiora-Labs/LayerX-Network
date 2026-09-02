use std::collections::BTreeSet;
use std::fs;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};

use layerx_agentd::events::subscription::Termination;
use layerx_agentd::identity::{
    register, CoreIdentity, IdentityError, IdentityResolver, ProtocolAuthority,
};
use layerx_agentd::session::{
    invalidate_on_revocation, open, InvalidationReason, OpenRequest, PendingActivity,
    PreparationState, RevocationEvent, RevokedEvent, SessionError, SessionId, SessionRef,
    SessionRegistry, Token,
};
use layerx_agentd::store::{Store, TenantId};
use layerx_types::ids::Did;
use layerx_types::verify::VerificationLevel;

static NEXT_DIRECTORY: AtomicU64 = AtomicU64::new(1);

struct BoundaryIdentity(CoreIdentity);

impl IdentityResolver for BoundaryIdentity {
    fn resolve(&mut self, _did: &Did) -> Result<Option<CoreIdentity>, IdentityError> {
        Ok(Some(self.0.clone()))
    }
}

fn root() -> PathBuf {
    let sequence = NEXT_DIRECTORY.fetch_add(1, Ordering::Relaxed);
    std::env::temp_dir().join(format!(
        "layerx-agentd-revocation-{}-{sequence}",
        std::process::id()
    ))
}

fn tenant(name: &str) -> TenantId {
    TenantId::new(name).unwrap_or_else(|error| panic!("tenant: {error}"))
}

fn did(value: &[u8]) -> Did {
    Did::new(value).unwrap_or_else(|error| panic!("DID: {error:?}"))
}

fn open_session(
    store: &mut Store,
    sessions: &mut SessionRegistry,
    tenant_name: &str,
    agent: &[u8],
    session_byte: u8,
    authority_byte: u8,
) -> Token {
    let did = did(agent);
    let tenant = tenant(tenant_name);
    let mut boundary = BoundaryIdentity(CoreIdentity {
        canonical_bytes: b"identity".to_vec(),
        head_sequence: 1,
        revocation_sequence: 1,
        verification_level: VerificationLevel::STATE_PROVEN,
        frozen: false,
        authorities: vec![ProtocolAuthority::SessionKey([authority_byte; 32])],
    });
    let identity = register(store, tenant.clone(), did.clone(), &mut boundary)
        .unwrap_or_else(|error| panic!("identity: {error:?}"));
    let request = OpenRequest {
        session_id: SessionId([session_byte; 32]),
        token_id: [session_byte.wrapping_add(1); 32],
        tenant,
        agent: did,
        authority: ProtocolAuthority::SessionKey([authority_byte; 32]),
        permitted_activity_types: BTreeSet::from([7]),
        scopes: BTreeSet::from(["prepare".to_owned()]),
        expiry_sequence: 100,
        opening_client: "sdk".to_owned(),
        policy_version: "v1".to_owned(),
    };
    open(store, sessions, &identity, request, 1)
        .unwrap_or_else(|error| panic!("session: {error:?}"))
}

fn setup(store: &mut Store) -> (Did, SessionRegistry, Token) {
    let mut sessions = SessionRegistry::default();
    let token = open_session(
        store,
        &mut sessions,
        "tenant-a",
        b"did:layerx:revoked",
        1,
        3,
    );
    (did(b"did:layerx:revoked"), sessions, token)
}

fn restore(store: &Store, tenants: &[&str]) -> SessionRegistry {
    let mut sessions = SessionRegistry::default();
    for name in tenants {
        sessions
            .restore_tenant(store, &tenant(name))
            .unwrap_or_else(|error| panic!("restore {name}: {error:?}"));
    }
    sessions
}

#[test]
fn revocation_cancels_only_unsubmitted_work_and_persists_session_invalidation() {
    let path = root();
    let mut store = Store::open(&path).unwrap_or_else(|error| panic!("store: {error}"));
    let (did, mut sessions, token) = setup(&mut store);
    let tenant = tenant("tenant-a");
    assert_eq!(sessions.generation(&tenant, SessionId([1; 32])), Some(1));
    assert_eq!(
        token.authorize(&sessions, &tenant, &did, "prepare", 8),
        Ok(SessionId([1; 32]))
    );
    let mut activities = vec![
        PendingActivity {
            session: SessionRef::new(tenant.clone(), SessionId([1; 32])),
            state: PreparationState::Prepared,
            cancelled: false,
            resolution_continues: false,
        },
        PendingActivity {
            session: SessionRef::new(tenant.clone(), SessionId([1; 32])),
            state: PreparationState::Executed,
            cancelled: false,
            resolution_continues: false,
        },
    ];
    let report = invalidate_on_revocation(
        &mut store,
        &mut sessions,
        &mut activities,
        &RevocationEvent {
            did: did.clone(),
            authority: Some(ProtocolAuthority::SessionKey([3; 32])),
            reason: InvalidationReason::SessionKeyRevoked,
            observed_sequence: 8,
        },
    )
    .unwrap_or_else(|error| panic!("invalidation: {error:?}"));
    assert_eq!(report.cancelled_preparations, 1);
    assert_eq!(report.executed_untouched, 1);
    assert_eq!(
        report.invalidated_sessions,
        vec![SessionRef::new(tenant.clone(), SessionId([1; 32]))]
    );
    assert!(activities[0].cancelled);
    assert!(!activities[1].cancelled);
    assert!(!sessions
        .get(&tenant, SessionId([1; 32]))
        .is_some_and(|record| record.open));
    assert_eq!(sessions.generation(&tenant, SessionId([1; 32])), Some(2));
    assert_eq!(
        token.authorize(&sessions, &tenant, &did, "prepare", 8),
        Err(SessionError::Revoked)
    );
    assert_eq!(
        token.authorize(&sessions, &tenant, &did, "prepare", 100),
        Err(SessionError::Revoked)
    );
    drop(sessions);
    drop(store);

    let store = Store::open(&path).unwrap_or_else(|error| panic!("reopen: {error}"));
    assert_eq!(
        token.authorize(&SessionRegistry::default(), &tenant, &did, "prepare", 8),
        Err(SessionError::Revoked)
    );
    let restored = restore(&store, &["tenant-a"]);
    assert_eq!(restored.generation(&tenant, SessionId([1; 32])), Some(2));
    assert!(!restored
        .get(&tenant, SessionId([1; 32]))
        .is_some_and(|record| record.open));
    assert_eq!(
        token.authorize(&restored, &tenant, &did, "prepare", 8),
        Err(SessionError::Revoked)
    );
    assert_eq!(
        restored.authenticate(&token.credential()),
        Err(SessionError::Revoked)
    );
    let _ = fs::remove_dir_all(path);
}

#[test]
fn unknown_submission_remains_owned_for_receipt_resolution() {
    let path = root();
    let mut store = Store::open(&path).unwrap_or_else(|error| panic!("store: {error}"));
    let (did, mut sessions, _token) = setup(&mut store);
    let tenant = tenant("tenant-a");
    let mut activities = [PendingActivity {
        session: SessionRef::new(tenant, SessionId([1; 32])),
        state: PreparationState::Unknown,
        cancelled: false,
        resolution_continues: false,
    }];
    let report = invalidate_on_revocation(
        &mut store,
        &mut sessions,
        &mut activities,
        &RevocationEvent {
            did,
            authority: None,
            reason: InvalidationReason::PrimaryKeyRotated,
            observed_sequence: 9,
        },
    )
    .unwrap_or_else(|error| panic!("invalidation: {error:?}"));
    assert_eq!(report.unresolved_left_for_resolution, 1);
    assert!(!activities[0].cancelled);
    assert!(activities[0].resolution_continues);
    let _ = fs::remove_dir_all(path);
}

#[test]
fn revocation_leaves_sibling_and_cross_tenant_sessions_at_their_generation() {
    let path = root();
    let mut store = Store::open(&path).unwrap_or_else(|error| panic!("store: {error}"));
    let (did, mut sessions, token) = setup(&mut store);
    let sibling = open_session(
        &mut store,
        &mut sessions,
        "tenant-a",
        b"did:layerx:sibling",
        5,
        6,
    );
    let other = open_session(
        &mut store,
        &mut sessions,
        "tenant-b",
        b"did:layerx:other",
        7,
        3,
    );
    let mut activities: [PendingActivity; 0] = [];
    let report = invalidate_on_revocation(
        &mut store,
        &mut sessions,
        &mut activities,
        &RevocationEvent {
            did: did.clone(),
            authority: Some(ProtocolAuthority::SessionKey([3; 32])),
            reason: InvalidationReason::SessionKeyRevoked,
            observed_sequence: 8,
        },
    )
    .unwrap_or_else(|error| panic!("invalidation: {error:?}"));
    assert_eq!(
        report.invalidated_sessions,
        vec![SessionRef::new(tenant("tenant-a"), SessionId([1; 32]))]
    );
    let tenant_a = self::tenant("tenant-a");
    let tenant_b = self::tenant("tenant-b");
    let sibling_did = self::did(b"did:layerx:sibling");
    let other_did = self::did(b"did:layerx:other");
    for registry in [sessions, {
        drop(store);
        let store = Store::open(&path).unwrap_or_else(|error| panic!("reopen: {error}"));
        restore(&store, &["tenant-a", "tenant-b"])
    }] {
        assert_eq!(registry.generation(&tenant_a, SessionId([1; 32])), Some(2));
        assert_eq!(registry.generation(&tenant_a, SessionId([5; 32])), Some(1));
        assert_eq!(registry.generation(&tenant_b, SessionId([7; 32])), Some(1));
        assert_eq!(registry.open_count(), 2);
        assert_eq!(
            token.authorize(&registry, &tenant_a, &did, "prepare", 9),
            Err(SessionError::Revoked)
        );
        assert_eq!(
            sibling.authorize(&registry, &tenant_a, &sibling_did, "prepare", 9),
            Ok(SessionId([5; 32]))
        );
        assert_eq!(
            other.authorize(&registry, &tenant_b, &other_did, "prepare", 9),
            Ok(SessionId([7; 32]))
        );
        assert_eq!(
            registry.authenticate(&sibling.credential()),
            Ok(sibling.clone())
        );
        assert_eq!(
            registry.authenticate(&other.credential()),
            Ok(other.clone())
        );
    }
    let _ = fs::remove_dir_all(path);
}

#[test]
fn authority_revocation_is_global_for_the_same_did_and_authority_but_session_ids_remain_tenant_qualified(
) {
    let path = root();
    let mut store = Store::open(&path).unwrap_or_else(|error| panic!("store: {error}"));
    let mut sessions = SessionRegistry::default();
    let did = did(b"did:layerx:shared");
    let token_a = open_session(
        &mut store,
        &mut sessions,
        "tenant-a",
        b"did:layerx:shared",
        1,
        3,
    );
    let token_b = open_session(
        &mut store,
        &mut sessions,
        "tenant-b",
        b"did:layerx:shared",
        1,
        3,
    );
    let mut activities: [PendingActivity; 0] = [];
    let report = invalidate_on_revocation(
        &mut store,
        &mut sessions,
        &mut activities,
        &RevocationEvent {
            did: did.clone(),
            authority: Some(ProtocolAuthority::SessionKey([3; 32])),
            reason: InvalidationReason::SessionKeyRevoked,
            observed_sequence: 8,
        },
    )
    .unwrap_or_else(|error| panic!("invalidation: {error:?}"));
    assert_eq!(
        report.invalidated_sessions,
        vec![
            SessionRef::new(tenant("tenant-a"), SessionId([1; 32])),
            SessionRef::new(tenant("tenant-b"), SessionId([1; 32])),
        ]
    );
    assert_eq!(
        token_a.authorize(&sessions, &tenant("tenant-a"), &did, "prepare", 9),
        Err(SessionError::Revoked)
    );
    assert_eq!(
        token_b.authorize(&sessions, &tenant("tenant-b"), &did, "prepare", 9),
        Err(SessionError::Revoked)
    );
    let _ = fs::remove_dir_all(path);
}

#[test]
fn authority_revocation_batch_failure_changes_no_session_or_generation() {
    let path = root();
    let mut store = Store::open(&path).unwrap_or_else(|error| panic!("store: {error}"));
    let mut sessions = SessionRegistry::default();
    let did = did(b"did:layerx:atomic-revocation");
    let first = open_session(
        &mut store,
        &mut sessions,
        "tenant-a",
        b"did:layerx:atomic-revocation",
        1,
        3,
    );
    let second = open_session(
        &mut store,
        &mut sessions,
        "tenant-b",
        b"did:layerx:atomic-revocation",
        2,
        3,
    );
    fs::create_dir(path.join("store.bin.tmp"))
        .unwrap_or_else(|error| panic!("block temporary store file: {error}"));
    let mut activities: [PendingActivity; 0] = [];
    assert!(matches!(
        invalidate_on_revocation(
            &mut store,
            &mut sessions,
            &mut activities,
            &RevocationEvent {
                did: did.clone(),
                authority: Some(ProtocolAuthority::SessionKey([3; 32])),
                reason: InvalidationReason::SessionKeyRevoked,
                observed_sequence: 8,
            },
        ),
        Err(SessionError::Store(_))
    ));
    for (tenant, session, token) in [
        (tenant("tenant-a"), SessionId([1; 32]), first),
        (tenant("tenant-b"), SessionId([2; 32]), second),
    ] {
        assert_eq!(sessions.generation(&tenant, session), Some(1));
        assert!(sessions
            .get(&tenant, session)
            .is_some_and(|record| record.open));
        assert_eq!(
            token.authorize(&sessions, &tenant, &did, "prepare", 9),
            Ok(session)
        );
    }
    fs::remove_dir(path.join("store.bin.tmp"))
        .unwrap_or_else(|error| panic!("unblock temporary store file: {error}"));
    drop(store);
    let store = Store::open(&path).unwrap_or_else(|error| panic!("reopen: {error}"));
    let restored = restore(&store, &["tenant-a", "tenant-b"]);
    assert_eq!(
        restored.generation(&tenant("tenant-a"), SessionId([1; 32])),
        Some(1)
    );
    assert_eq!(
        restored.generation(&tenant("tenant-b"), SessionId([2; 32])),
        Some(1)
    );
    assert_eq!(restored.open_count(), 2);
    let _ = fs::remove_dir_all(path);
}

#[test]
fn revocation_stops_the_in_flight_stream_with_a_typed_event() {
    let path = root();
    let mut store = Store::open(&path).unwrap_or_else(|error| panic!("store: {error}"));
    let (did, mut sessions, token) = setup(&mut store);
    let stop = sessions
        .revocation_stop(&token)
        .unwrap_or_else(|error| panic!("revocation stop: {error:?}"));
    assert_eq!(stop.reason(), None);
    let mut activities: [PendingActivity; 0] = [];
    invalidate_on_revocation(
        &mut store,
        &mut sessions,
        &mut activities,
        &RevocationEvent {
            did: did.clone(),
            authority: Some(ProtocolAuthority::SessionKey([3; 32])),
            reason: InvalidationReason::SessionKeyRevoked,
            observed_sequence: 8,
        },
    )
    .unwrap_or_else(|error| panic!("invalidation: {error:?}"));
    assert_eq!(
        token.boundary(&sessions),
        Err(RevokedEvent {
            tenant: tenant("tenant-a"),
            session_id: SessionId([1; 32]),
            token_generation: 1,
            current_generation: Some(2),
            open: false,
        })
    );
    assert_eq!(stop.reason(), Some(Termination::SessionRevoked));
    assert_eq!(
        token.authorize(&sessions, &tenant("tenant-a"), &did, "prepare", 9),
        Err(SessionError::Revoked)
    );
    let _ = fs::remove_dir_all(path);
}
