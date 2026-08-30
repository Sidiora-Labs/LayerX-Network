use std::collections::BTreeSet;
use std::fs;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};

use layerx_agentd::identity::{
    register, CoreIdentity, IdentityError, IdentityResolver, ProtocolAuthority,
};
use layerx_agentd::session::{close, open, OpenRequest, SessionError, SessionId, SessionRegistry};
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

fn directory(label: &str) -> PathBuf {
    let sequence = NEXT_DIRECTORY.fetch_add(1, Ordering::Relaxed);
    std::env::temp_dir().join(format!(
        "layerx-agentd-session-{label}-{}-{sequence}",
        std::process::id()
    ))
}

fn tenant(value: &str) -> TenantId {
    TenantId::new(value).unwrap_or_else(|error| panic!("tenant invalid: {error}"))
}

fn did(value: &[u8]) -> Did {
    Did::new(value).unwrap_or_else(|error| panic!("DID invalid: {error:?}"))
}

fn identity(
    store: &mut Store,
    tenant_id: TenantId,
    agent: Did,
) -> layerx_agentd::identity::IdentityRecord {
    let mut boundary = BoundaryIdentity(CoreIdentity {
        canonical_bytes: b"proven-identity".to_vec(),
        head_sequence: 10,
        revocation_sequence: 1,
        verification_level: VerificationLevel::STATE_PROVEN,
        frozen: false,
        authorities: vec![ProtocolAuthority::SessionKey([4; 32])],
    });
    register(store, tenant_id, agent, &mut boundary)
        .unwrap_or_else(|error| panic!("identity registration failed: {error:?}"))
}

fn request(id: u8, tenant_id: TenantId, agent: Did) -> OpenRequest {
    OpenRequest {
        session_id: SessionId([id; 32]),
        token_id: [id.wrapping_add(20); 32],
        tenant: tenant_id,
        agent,
        authority: ProtocolAuthority::SessionKey([4; 32]),
        permitted_activity_types: BTreeSet::from([7_u16, 9]),
        scopes: BTreeSet::from(["prepare".to_owned(), "read".to_owned()]),
        expiry_sequence: 100,
        opening_client: "sdk-rust".to_owned(),
        policy_version: "policy-v3".to_owned(),
    }
}

#[test]
fn session_open_binds_every_dimension_and_token_is_daemon_only() {
    let root = directory("open");
    let mut store = Store::open(&root).unwrap_or_else(|error| panic!("open failed: {error}"));
    let tenant_id = tenant("tenant-a");
    let agent = did(b"did:layerx:a");
    let identity = identity(&mut store, tenant_id.clone(), agent.clone());
    let mut registry = SessionRegistry::default();
    let token = open(
        &mut store,
        &mut registry,
        &identity,
        request(1, tenant_id.clone(), agent.clone()),
        10,
    )
    .unwrap_or_else(|error| panic!("session open failed: {error:?}"));
    assert_eq!(
        token.authorize(&tenant_id, &agent, "prepare", 11),
        Ok(SessionId([1; 32]))
    );
    assert_eq!(
        token.authorize(&tenant_id, &agent, "submit", 11),
        Err(SessionError::ScopeDenied)
    );
    assert_eq!(
        token.authorize(&tenant("tenant-b"), &agent, "prepare", 11),
        Err(SessionError::WrongPrincipal)
    );
    assert_eq!(
        token.authorize(&tenant_id, &agent, "prepare", 100),
        Err(SessionError::Expired)
    );
    let record = registry
        .get(SessionId([1; 32]))
        .unwrap_or_else(|| panic!("session missing"));
    assert_eq!(
        record.request.authority,
        ProtocolAuthority::SessionKey([4; 32])
    );
    let _ = fs::remove_dir_all(root);
}

#[test]
fn absent_protocol_authority_and_incomplete_requests_are_refused() {
    let root = directory("refuse");
    let mut store = Store::open(&root).unwrap_or_else(|error| panic!("open failed: {error}"));
    let tenant_id = tenant("tenant-a");
    let agent = did(b"did:layerx:a");
    let identity = identity(&mut store, tenant_id.clone(), agent.clone());
    let mut registry = SessionRegistry::default();
    let mut missing_authority = request(1, tenant_id.clone(), agent.clone());
    missing_authority.authority = ProtocolAuthority::SessionKey([99; 32]);
    assert!(matches!(
        open(&mut store, &mut registry, &identity, missing_authority, 10),
        Err(SessionError::AuthorityMissing)
    ));
    let mut missing_scope = request(2, tenant_id, agent);
    missing_scope.scopes.clear();
    assert!(matches!(
        open(&mut store, &mut registry, &identity, missing_scope, 10),
        Err(SessionError::MissingField("scopes"))
    ));
    assert_eq!(registry.open_count(), 0);
    let _ = fs::remove_dir_all(root);
}

#[test]
fn closing_one_concurrent_session_leaves_sibling_state_untouched() {
    let root = directory("independent");
    let mut store = Store::open(&root).unwrap_or_else(|error| panic!("open failed: {error}"));
    let tenant_id = tenant("tenant-a");
    let agent = did(b"did:layerx:a");
    let identity = identity(&mut store, tenant_id.clone(), agent.clone());
    let mut registry = SessionRegistry::default();
    for id in [1_u8, 2] {
        open(
            &mut store,
            &mut registry,
            &identity,
            request(id, tenant_id.clone(), agent.clone()),
            10,
        )
        .unwrap_or_else(|error| panic!("session {id} open failed: {error:?}"));
    }
    if let Err(error) = close(&mut store, &mut registry, SessionId([1; 32])) {
        panic!("close failed: {error:?}");
    }
    assert!(!registry
        .get(SessionId([1; 32]))
        .is_some_and(|record| record.open));
    assert!(registry
        .get(SessionId([2; 32]))
        .is_some_and(|record| record.open));
    assert_eq!(registry.open_count(), 1);
    let _ = fs::remove_dir_all(root);
}
