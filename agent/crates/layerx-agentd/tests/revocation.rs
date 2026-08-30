use std::collections::BTreeSet;
use std::fs;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};

use layerx_agentd::identity::{
    register, CoreIdentity, IdentityError, IdentityResolver, ProtocolAuthority,
};
use layerx_agentd::session::{
    invalidate_on_revocation, open, InvalidationReason, OpenRequest, PendingActivity,
    PreparationState, RevocationEvent, SessionId, SessionRegistry,
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

fn setup(store: &mut Store) -> (Did, SessionRegistry) {
    let did = Did::new(b"did:layerx:revoked").unwrap_or_else(|error| panic!("DID: {error:?}"));
    let tenant = TenantId::new("tenant-a").unwrap_or_else(|error| panic!("tenant: {error}"));
    let mut boundary = BoundaryIdentity(CoreIdentity {
        canonical_bytes: b"identity".to_vec(),
        head_sequence: 1,
        revocation_sequence: 1,
        verification_level: VerificationLevel::STATE_PROVEN,
        frozen: false,
        authorities: vec![ProtocolAuthority::SessionKey([3; 32])],
    });
    let identity = register(store, tenant.clone(), did.clone(), &mut boundary)
        .unwrap_or_else(|error| panic!("identity: {error:?}"));
    let mut sessions = SessionRegistry::default();
    let request = OpenRequest {
        session_id: SessionId([1; 32]),
        token_id: [2; 32],
        tenant,
        agent: did.clone(),
        authority: ProtocolAuthority::SessionKey([3; 32]),
        permitted_activity_types: BTreeSet::from([7]),
        scopes: BTreeSet::from(["prepare".to_owned()]),
        expiry_sequence: 100,
        opening_client: "sdk".to_owned(),
        policy_version: "v1".to_owned(),
    };
    open(store, &mut sessions, &identity, request, 1)
        .unwrap_or_else(|error| panic!("session: {error:?}"));
    (did, sessions)
}

#[test]
fn revocation_cancels_only_unsubmitted_work_and_persists_session_invalidation() {
    let path = root();
    let mut store = Store::open(&path).unwrap_or_else(|error| panic!("store: {error}"));
    let (did, mut sessions) = setup(&mut store);
    let mut activities = vec![
        PendingActivity {
            session_id: SessionId([1; 32]),
            state: PreparationState::Prepared,
            cancelled: false,
            resolution_continues: false,
        },
        PendingActivity {
            session_id: SessionId([1; 32]),
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
            did,
            authority: Some(ProtocolAuthority::SessionKey([3; 32])),
            reason: InvalidationReason::SessionKeyRevoked,
            observed_sequence: 8,
        },
    )
    .unwrap_or_else(|error| panic!("invalidation: {error:?}"));
    assert_eq!(report.cancelled_preparations, 1);
    assert_eq!(report.executed_untouched, 1);
    assert!(activities[0].cancelled);
    assert!(!activities[1].cancelled);
    assert!(!sessions
        .get(SessionId([1; 32]))
        .is_some_and(|record| record.open));
    drop(store);
    assert!(
        Store::open(&path).is_ok(),
        "persisted invalidation store must reopen"
    );
    let _ = fs::remove_dir_all(path);
}

#[test]
fn unknown_submission_remains_owned_for_receipt_resolution() {
    let path = root();
    let mut store = Store::open(&path).unwrap_or_else(|error| panic!("store: {error}"));
    let (did, mut sessions) = setup(&mut store);
    let mut activities = [PendingActivity {
        session_id: SessionId([1; 32]),
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
