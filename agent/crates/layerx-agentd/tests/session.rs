use std::collections::BTreeSet;
use std::fs;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};

use layerx_agentd::identity::{
    register, CoreIdentity, IdentityError, IdentityRecord, IdentityResolver, ProtocolAuthority,
};
use layerx_agentd::session::{
    close, open, restrict_scope, OpenRequest, SessionCredential, SessionError, SessionId,
    SessionRegistry, Token,
};
use layerx_agentd::store::{ObjectKind, Store, TenantId, TenantKey};
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

fn identity(store: &mut Store, tenant_id: TenantId, agent: Did) -> IdentityRecord {
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

fn open_session(
    store: &mut Store,
    registry: &mut SessionRegistry,
    identity: &IdentityRecord,
    id: u8,
    tenant_id: &TenantId,
    agent: &Did,
) -> Token {
    open(
        store,
        registry,
        identity,
        request(id, tenant_id.clone(), agent.clone()),
        10,
    )
    .unwrap_or_else(|error| panic!("session {id} open failed: {error:?}"))
}

fn reload(store: &Store, tenants: &[&TenantId]) -> SessionRegistry {
    let mut registry = SessionRegistry::default();
    for tenant_id in tenants {
        registry
            .restore_tenant(store, tenant_id)
            .unwrap_or_else(|error| panic!("restore failed: {error:?}"));
    }
    registry
}

fn session_key(tenant_id: &TenantId, id: u8) -> TenantKey {
    TenantKey::new(tenant_id.clone(), ObjectKind::Session, [id; 32].to_vec())
        .unwrap_or_else(|error| panic!("session key invalid: {error}"))
}

fn stored_bytes(store: &Store, tenant_id: &TenantId, id: u8) -> Vec<u8> {
    store
        .get(&session_key(tenant_id, id))
        .map(|value| value.bytes().to_vec())
        .unwrap_or_else(|| panic!("session {id} record missing"))
}

fn length(value: usize) -> u16 {
    u16::try_from(value).unwrap_or_else(|error| panic!("length prefix: {error}"))
}

fn legacy_record_bytes(request: &OpenRequest) -> Vec<u8> {
    let mut bytes = Vec::new();
    bytes.extend_from_slice(b"LXSR02");
    bytes.extend_from_slice(&request.session_id.0);
    bytes.extend_from_slice(&request.token_id);
    bytes.extend_from_slice(&request.expiry_sequence.to_be_bytes());
    bytes.push(1);
    bytes.extend_from_slice(&0_u64.to_be_bytes());
    bytes.extend_from_slice(&0_u128.to_be_bytes());
    bytes.extend_from_slice(&0_u64.to_be_bytes());
    for text in [&request.opening_client, &request.policy_version] {
        bytes.extend_from_slice(&length(text.len()).to_be_bytes());
        bytes.extend_from_slice(text.as_bytes());
    }
    let agent = request.agent.as_bytes();
    bytes.extend_from_slice(&length(agent.len()).to_be_bytes());
    bytes.extend_from_slice(agent);
    let ProtocolAuthority::SessionKey(authority) = &request.authority else {
        panic!("request authority must be a session key");
    };
    bytes.push(2);
    bytes.extend_from_slice(authority);
    bytes.extend_from_slice(&length(request.permitted_activity_types.len()).to_be_bytes());
    for value in &request.permitted_activity_types {
        bytes.extend_from_slice(&value.to_be_bytes());
    }
    bytes.extend_from_slice(&length(request.scopes.len()).to_be_bytes());
    for scope in &request.scopes {
        bytes.extend_from_slice(&length(scope.len()).to_be_bytes());
        bytes.extend_from_slice(scope.as_bytes());
    }
    bytes
}

#[test]
fn session_open_binds_every_dimension_and_token_is_daemon_only() {
    let root = directory("open");
    let mut store = Store::open(&root).unwrap_or_else(|error| panic!("open failed: {error}"));
    let tenant_id = tenant("tenant-a");
    let agent = did(b"did:layerx:a");
    let identity = identity(&mut store, tenant_id.clone(), agent.clone());
    let mut registry = SessionRegistry::default();
    let token = open_session(&mut store, &mut registry, &identity, 1, &tenant_id, &agent);
    assert_eq!(token.generation(), 1);
    assert_eq!(registry.generation(&tenant_id, SessionId([1; 32])), Some(1));
    assert_eq!(registry.generation(&tenant_id, SessionId([2; 32])), None);
    let raw_bearer_debug = format!("{:?}", [21_u8; 32]);
    let credential_debug = format!("{:?}", token.credential());
    let token_debug = format!("{token:?}");
    assert!(credential_debug.contains("[REDACTED]"));
    assert!(token_debug.contains("[REDACTED]"));
    assert!(!credential_debug.contains(&raw_bearer_debug));
    assert!(!token_debug.contains(&raw_bearer_debug));
    assert_eq!(
        token.authorize(&registry, &tenant_id, &agent, "prepare", 11),
        Ok(SessionId([1; 32]))
    );
    assert_eq!(
        token.authorize(&registry, &tenant_id, &agent, "submit", 11),
        Err(SessionError::ScopeDenied)
    );
    assert_eq!(
        token.authorize(&registry, &tenant("tenant-b"), &agent, "prepare", 11),
        Err(SessionError::WrongPrincipal)
    );
    assert_eq!(
        token.authorize(&registry, &tenant_id, &agent, "prepare", 100),
        Err(SessionError::Expired)
    );
    assert_eq!(
        registry.authenticate(&token.credential()),
        Ok(token.clone())
    );
    assert_eq!(
        registry.authenticate_bearer(&tenant_id, SessionId([1; 32]), [21; 32]),
        Ok(token)
    );
    assert_eq!(
        registry.authenticate(&SessionCredential::new(
            tenant_id.clone(),
            SessionId([1; 32]),
            [22; 32],
            1,
        )),
        Err(SessionError::Revoked)
    );
    assert_eq!(
        registry.authenticate(&SessionCredential::new(
            tenant_id.clone(),
            SessionId([2; 32]),
            [21; 32],
            1,
        )),
        Err(SessionError::NotFound)
    );
    let record = registry
        .get(&tenant_id, SessionId([1; 32]))
        .unwrap_or_else(|| panic!("session missing"));
    assert_eq!(
        record.request.authority,
        ProtocolAuthority::SessionKey([4; 32])
    );
    assert_eq!(record.generation, 1);
    let record_debug = format!("{record:?}");
    assert!(record_debug.contains("[REDACTED]"));
    assert!(!record_debug.contains(&raw_bearer_debug));
    assert!(stored_bytes(&store, &tenant_id, 1).starts_with(b"LXSR04"));
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
fn a_recorded_session_id_is_never_reopened_even_after_restart() {
    let root = directory("reopen");
    let mut store = Store::open(&root).unwrap_or_else(|error| panic!("open failed: {error}"));
    let tenant_id = tenant("tenant-a");
    let agent = did(b"did:layerx:a");
    let identity = identity(&mut store, tenant_id.clone(), agent.clone());
    let mut registry = SessionRegistry::default();
    let token = open_session(&mut store, &mut registry, &identity, 1, &tenant_id, &agent);
    assert_eq!(
        open(
            &mut store,
            &mut registry,
            &identity,
            request(1, tenant_id.clone(), agent.clone()),
            10
        ),
        Err(SessionError::IdentityMismatch)
    );
    close(&mut store, &mut registry, &tenant_id, SessionId([1; 32]))
        .unwrap_or_else(|error| panic!("close failed: {error:?}"));
    assert_eq!(registry.generation(&tenant_id, SessionId([1; 32])), Some(2));
    assert_eq!(
        open(
            &mut store,
            &mut registry,
            &identity,
            request(1, tenant_id.clone(), agent.clone()),
            10
        ),
        Err(SessionError::IdentityMismatch)
    );
    drop(registry);
    let mut fresh = SessionRegistry::default();
    assert_eq!(
        open(
            &mut store,
            &mut fresh,
            &identity,
            request(1, tenant_id.clone(), agent.clone()),
            10
        ),
        Err(SessionError::IdentityMismatch)
    );
    assert_eq!(fresh.open_count(), 0);
    let restored = reload(&store, &[&tenant_id]);
    assert_eq!(restored.generation(&tenant_id, SessionId([1; 32])), Some(2));
    assert_eq!(
        token.authorize(&restored, &tenant_id, &agent, "read", 11),
        Err(SessionError::Revoked)
    );
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
    let first = open_session(&mut store, &mut registry, &identity, 1, &tenant_id, &agent);
    let second = open_session(&mut store, &mut registry, &identity, 2, &tenant_id, &agent);
    if let Err(error) = close(&mut store, &mut registry, &tenant_id, SessionId([1; 32])) {
        panic!("close failed: {error:?}");
    }
    assert!(!registry
        .get(&tenant_id, SessionId([1; 32]))
        .is_some_and(|record| record.open));
    assert!(registry
        .get(&tenant_id, SessionId([2; 32]))
        .is_some_and(|record| record.open));
    assert_eq!(registry.open_count(), 1);
    assert_eq!(registry.generation(&tenant_id, SessionId([1; 32])), Some(2));
    assert_eq!(registry.generation(&tenant_id, SessionId([2; 32])), Some(1));
    assert_eq!(
        first.authorize(&registry, &tenant_id, &agent, "read", 11),
        Err(SessionError::Revoked)
    );
    assert_eq!(
        second.authorize(&registry, &tenant_id, &agent, "read", 11),
        Ok(SessionId([2; 32]))
    );
    assert_eq!(
        close(&mut store, &mut registry, &tenant_id, SessionId([1; 32])),
        Err(SessionError::AlreadyClosed)
    );
    assert_eq!(registry.generation(&tenant_id, SessionId([1; 32])), Some(2));
    let _ = fs::remove_dir_all(root);
}

#[test]
fn closed_session_token_is_revoked_not_expired_before_and_after_restart() {
    let root = directory("closed");
    let mut store = Store::open(&root).unwrap_or_else(|error| panic!("open failed: {error}"));
    let tenant_id = tenant("tenant-a");
    let agent = did(b"did:layerx:a");
    let identity = identity(&mut store, tenant_id.clone(), agent.clone());
    let mut registry = SessionRegistry::default();
    let token = open_session(&mut store, &mut registry, &identity, 1, &tenant_id, &agent);
    close(&mut store, &mut registry, &tenant_id, SessionId([1; 32]))
        .unwrap_or_else(|error| panic!("close failed: {error:?}"));
    assert_eq!(
        token.authorize(&registry, &tenant_id, &agent, "read", 11),
        Err(SessionError::Revoked)
    );
    assert_eq!(
        token.authorize(&registry, &tenant_id, &agent, "read", 100),
        Err(SessionError::Revoked)
    );
    assert_ne!(SessionError::Revoked, SessionError::Expired);
    drop(registry);
    drop(store);

    let store = Store::open(&root).unwrap_or_else(|error| panic!("reopen failed: {error}"));
    assert_eq!(
        token.authorize(&SessionRegistry::default(), &tenant_id, &agent, "read", 11),
        Err(SessionError::Revoked)
    );
    let restored = reload(&store, &[&tenant_id]);
    assert_eq!(restored.generation(&tenant_id, SessionId([1; 32])), Some(2));
    assert!(!restored
        .get(&tenant_id, SessionId([1; 32]))
        .is_some_and(|record| record.open));
    assert_eq!(
        token.authorize(&restored, &tenant_id, &agent, "read", 11),
        Err(SessionError::Revoked)
    );
    assert_eq!(
        restored.authenticate(&token.credential()),
        Err(SessionError::Revoked)
    );
    let _ = fs::remove_dir_all(root);
}

#[test]
fn restart_reloads_the_generation_before_an_open_session_token_is_accepted() {
    let root = directory("restart");
    let mut store = Store::open(&root).unwrap_or_else(|error| panic!("open failed: {error}"));
    let tenant_id = tenant("tenant-a");
    let agent = did(b"did:layerx:a");
    let identity = identity(&mut store, tenant_id.clone(), agent.clone());
    let mut registry = SessionRegistry::default();
    let token = open_session(&mut store, &mut registry, &identity, 1, &tenant_id, &agent);
    drop(registry);
    drop(store);

    let store = Store::open(&root).unwrap_or_else(|error| panic!("reopen failed: {error}"));
    assert_eq!(
        token.authorize(&SessionRegistry::default(), &tenant_id, &agent, "read", 11),
        Err(SessionError::Revoked)
    );
    let restored = reload(&store, &[&tenant_id]);
    assert_eq!(restored.generation(&tenant_id, SessionId([1; 32])), Some(1));
    assert_eq!(
        token.authorize(&restored, &tenant_id, &agent, "read", 11),
        Ok(SessionId([1; 32]))
    );
    assert_eq!(restored.authenticate(&token.credential()), Ok(token));
    let _ = fs::remove_dir_all(root);
}

#[test]
fn scope_narrowing_advances_the_generation_and_refuses_every_earlier_token() {
    let root = directory("scope");
    let mut store = Store::open(&root).unwrap_or_else(|error| panic!("open failed: {error}"));
    let tenant_id = tenant("tenant-a");
    let agent = did(b"did:layerx:a");
    let identity = identity(&mut store, tenant_id.clone(), agent.clone());
    let mut registry = SessionRegistry::default();
    let first = open_session(&mut store, &mut registry, &identity, 1, &tenant_id, &agent);
    assert_eq!(
        restrict_scope(
            &mut store,
            &mut registry,
            &tenant_id,
            SessionId([1; 32]),
            [31; 32],
            BTreeSet::from(["prepare".to_owned(), "read".to_owned(), "submit".to_owned()]),
            BTreeSet::from([7_u16, 9]),
        ),
        Err(SessionError::ScopeDenied)
    );
    assert_eq!(
        restrict_scope(
            &mut store,
            &mut registry,
            &tenant_id,
            SessionId([1; 32]),
            [31; 32],
            BTreeSet::new(),
            BTreeSet::from([7_u16]),
        ),
        Err(SessionError::ScopeDenied)
    );
    assert_eq!(
        restrict_scope(
            &mut store,
            &mut registry,
            &tenant_id,
            SessionId([2; 32]),
            [31; 32],
            BTreeSet::from(["read".to_owned()]),
            BTreeSet::from([7_u16]),
        ),
        Err(SessionError::NotFound)
    );
    assert_eq!(registry.generation(&tenant_id, SessionId([1; 32])), Some(1));
    assert_eq!(
        first.authorize(&registry, &tenant_id, &agent, "prepare", 11),
        Ok(SessionId([1; 32]))
    );

    let narrowed = restrict_scope(
        &mut store,
        &mut registry,
        &tenant_id,
        SessionId([1; 32]),
        [31; 32],
        BTreeSet::from(["read".to_owned()]),
        BTreeSet::from([7_u16]),
    )
    .unwrap_or_else(|error| panic!("scope narrowing failed: {error:?}"));
    assert_eq!(narrowed.generation(), 2);
    assert_eq!(narrowed.token_id(), [31; 32]);
    assert_eq!(registry.generation(&tenant_id, SessionId([1; 32])), Some(2));
    assert_eq!(
        first.authorize(&registry, &tenant_id, &agent, "read", 11),
        Err(SessionError::Revoked)
    );
    assert_eq!(
        narrowed.authorize(&registry, &tenant_id, &agent, "read", 11),
        Ok(SessionId([1; 32]))
    );
    assert_eq!(
        narrowed.authorize(&registry, &tenant_id, &agent, "prepare", 11),
        Err(SessionError::ScopeDenied)
    );
    assert_eq!(
        registry.authenticate(&first.credential()),
        Err(SessionError::Revoked)
    );
    assert_eq!(
        registry.authenticate(&narrowed.credential()),
        Ok(narrowed.clone())
    );
    let record = registry
        .get(&tenant_id, SessionId([1; 32]))
        .unwrap_or_else(|| panic!("session missing"));
    assert!(record.open);
    assert_eq!(record.request.scopes, BTreeSet::from(["read".to_owned()]));
    assert_eq!(
        record.request.permitted_activity_types,
        BTreeSet::from([7_u16])
    );
    assert_eq!(record.retired_token_ids, BTreeSet::from([[21; 32]]));
    drop(registry);

    let mut restored = reload(&store, &[&tenant_id]);
    assert_eq!(restored.generation(&tenant_id, SessionId([1; 32])), Some(2));
    assert_eq!(
        first.authorize(&restored, &tenant_id, &agent, "read", 11),
        Err(SessionError::Revoked)
    );
    assert_eq!(
        narrowed.authorize(&restored, &tenant_id, &agent, "read", 11),
        Ok(SessionId([1; 32]))
    );
    assert_eq!(
        restrict_scope(
            &mut store,
            &mut restored,
            &tenant_id,
            SessionId([1; 32]),
            [21; 32],
            BTreeSet::from(["read".to_owned()]),
            BTreeSet::from([7_u16]),
        ),
        Err(SessionError::TokenReuse)
    );
    assert_eq!(
        restored.authenticate_bearer(&tenant_id, SessionId([1; 32]), [21; 32]),
        Err(SessionError::Revoked)
    );
    assert_eq!(restored.authenticate(&narrowed.credential()), Ok(narrowed));
    let _ = fs::remove_dir_all(root);
}

#[test]
fn closing_under_one_tenant_cannot_alter_generations_under_another() {
    let root = directory("tenants");
    let mut store = Store::open(&root).unwrap_or_else(|error| panic!("open failed: {error}"));
    let tenant_a = tenant("tenant-a");
    let tenant_b = tenant("tenant-b");
    let agent_a = did(b"did:layerx:a");
    let agent_b = did(b"did:layerx:b");
    let identity_a = identity(&mut store, tenant_a.clone(), agent_a.clone());
    let identity_b = identity(&mut store, tenant_b.clone(), agent_b.clone());
    let mut registry = SessionRegistry::default();
    let token_a = open_session(
        &mut store,
        &mut registry,
        &identity_a,
        1,
        &tenant_a,
        &agent_a,
    );
    let token_b = open_session(
        &mut store,
        &mut registry,
        &identity_b,
        1,
        &tenant_b,
        &agent_b,
    );
    close(&mut store, &mut registry, &tenant_a, SessionId([1; 32]))
        .unwrap_or_else(|error| panic!("close failed: {error:?}"));
    assert_eq!(registry.generation(&tenant_a, SessionId([1; 32])), Some(2));
    assert_eq!(registry.generation(&tenant_b, SessionId([1; 32])), Some(1));
    assert_eq!(
        token_a.authorize(&registry, &tenant_a, &agent_a, "read", 11),
        Err(SessionError::Revoked)
    );
    assert_eq!(
        token_b.authorize(&registry, &tenant_b, &agent_b, "read", 11),
        Ok(SessionId([1; 32]))
    );
    assert_eq!(
        token_b.authorize(&registry, &tenant_a, &agent_b, "read", 11),
        Err(SessionError::WrongPrincipal)
    );
    drop(registry);

    let restored = reload(&store, &[&tenant_a, &tenant_b]);
    assert_eq!(restored.generation(&tenant_a, SessionId([1; 32])), Some(2));
    assert_eq!(restored.generation(&tenant_b, SessionId([1; 32])), Some(1));
    assert_eq!(
        token_b.authorize(&restored, &tenant_b, &agent_b, "read", 11),
        Ok(SessionId([1; 32]))
    );
    let only_b = reload(&store, &[&tenant_b]);
    assert_eq!(only_b.generation(&tenant_a, SessionId([1; 32])), None);
    assert_eq!(only_b.generation(&tenant_b, SessionId([1; 32])), Some(1));
    assert_eq!(
        token_a.authorize(&only_b, &tenant_a, &agent_a, "read", 11),
        Err(SessionError::Revoked)
    );
    assert_eq!(
        token_b.authorize(&only_b, &tenant_b, &agent_b, "read", 11),
        Ok(SessionId([1; 32]))
    );
    let _ = fs::remove_dir_all(root);
}

#[test]
fn legacy_records_load_at_the_first_generation_and_are_rewritten_on_change() {
    let root = directory("legacy");
    let mut store = Store::open(&root).unwrap_or_else(|error| panic!("open failed: {error}"));
    let tenant_id = tenant("tenant-a");
    let agent = did(b"did:layerx:a");
    let legacy = request(1, tenant_id.clone(), agent.clone());
    store
        .put_local(session_key(&tenant_id, 1), legacy_record_bytes(&legacy))
        .unwrap_or_else(|error| panic!("legacy put failed: {error}"));
    let mut registry = reload(&store, &[&tenant_id]);
    assert_eq!(registry.generation(&tenant_id, SessionId([1; 32])), Some(1));
    let record = registry
        .get(&tenant_id, SessionId([1; 32]))
        .unwrap_or_else(|| panic!("legacy session missing"));
    assert!(record.open);
    assert_eq!(record.request, legacy);
    let token = registry
        .authenticate_bearer(&tenant_id, SessionId([1; 32]), [21; 32])
        .unwrap_or_else(|error| panic!("legacy token: {error:?}"));
    assert_eq!(token.generation(), 1);
    assert_eq!(
        token.authorize(&registry, &tenant_id, &agent, "read", 11),
        Ok(SessionId([1; 32]))
    );
    assert!(stored_bytes(&store, &tenant_id, 1).starts_with(b"LXSR02"));

    close(&mut store, &mut registry, &tenant_id, SessionId([1; 32]))
        .unwrap_or_else(|error| panic!("close failed: {error:?}"));
    assert!(stored_bytes(&store, &tenant_id, 1).starts_with(b"LXSR04"));
    assert_eq!(
        token.authorize(&registry, &tenant_id, &agent, "read", 11),
        Err(SessionError::Revoked)
    );
    let restored = reload(&store, &[&tenant_id]);
    assert_eq!(restored.generation(&tenant_id, SessionId([1; 32])), Some(2));
    assert_eq!(
        token.authorize(&restored, &tenant_id, &agent, "read", 11),
        Err(SessionError::Revoked)
    );
    let _ = fs::remove_dir_all(root);
}

#[test]
fn generation_exhaustion_is_typed_and_does_not_mutate_or_persist_the_session() {
    let root = directory("generation-exhaustion");
    let mut store = Store::open(&root).unwrap_or_else(|error| panic!("open failed: {error}"));
    let tenant_id = tenant("tenant-a");
    let agent = did(b"did:layerx:a");
    let identity = identity(&mut store, tenant_id.clone(), agent);
    let mut registry = SessionRegistry::default();
    open_session(
        &mut store,
        &mut registry,
        &identity,
        1,
        &tenant_id,
        identity.did(),
    );
    let mut bytes = stored_bytes(&store, &tenant_id, 1);
    bytes[111..119].copy_from_slice(&u64::MAX.to_be_bytes());
    store
        .put_local(session_key(&tenant_id, 1), bytes.clone())
        .unwrap_or_else(|error| panic!("put failed: {error}"));
    let mut restored = reload(&store, &[&tenant_id]);
    assert_eq!(
        close(&mut store, &mut restored, &tenant_id, SessionId([1; 32])),
        Err(SessionError::GenerationExhausted)
    );
    assert_eq!(
        restored.generation(&tenant_id, SessionId([1; 32])),
        Some(u64::MAX)
    );
    assert!(restored
        .get(&tenant_id, SessionId([1; 32]))
        .is_some_and(|record| record.open));
    assert_eq!(stored_bytes(&store, &tenant_id, 1), bytes);
    let _ = fs::remove_dir_all(root);
}

#[test]
fn a_zero_generation_record_is_refused_rather_than_defaulted() {
    let root = directory("zero-generation");
    let mut store = Store::open(&root).unwrap_or_else(|error| panic!("open failed: {error}"));
    let tenant_id = tenant("tenant-a");
    let agent = did(b"did:layerx:a");
    let identity = identity(&mut store, tenant_id.clone(), agent.clone());
    let mut registry = SessionRegistry::default();
    open_session(&mut store, &mut registry, &identity, 1, &tenant_id, &agent);
    let mut bytes = stored_bytes(&store, &tenant_id, 1);
    assert_eq!(&bytes[111..119], &1_u64.to_be_bytes());
    bytes[111..119].copy_from_slice(&0_u64.to_be_bytes());
    store
        .put_local(session_key(&tenant_id, 1), bytes)
        .unwrap_or_else(|error| panic!("put failed: {error}"));
    let mut fresh = SessionRegistry::default();
    assert_eq!(
        fresh.restore_tenant(&store, &tenant_id),
        Err(SessionError::MissingField("generation"))
    );
    assert_eq!(fresh.generation(&tenant_id, SessionId([1; 32])), None);
    assert_eq!(fresh.open_count(), 0);
    let _ = fs::remove_dir_all(root);
}

#[test]
fn an_unknown_record_version_is_refused() {
    let root = directory("unknown-version");
    let mut store = Store::open(&root).unwrap_or_else(|error| panic!("open failed: {error}"));
    let tenant_id = tenant("tenant-a");
    let agent = did(b"did:layerx:a");
    let mut bytes = legacy_record_bytes(&request(1, tenant_id.clone(), agent));
    bytes[..6].copy_from_slice(b"LXSR99");
    store
        .put_local(session_key(&tenant_id, 1), bytes)
        .unwrap_or_else(|error| panic!("put failed: {error}"));
    let mut fresh = SessionRegistry::default();
    assert_eq!(
        fresh.restore_tenant(&store, &tenant_id),
        Err(SessionError::MissingField("record_version"))
    );
    assert_eq!(fresh.generation(&tenant_id, SessionId([1; 32])), None);
    let _ = fs::remove_dir_all(root);
}

#[test]
fn a_failed_tenant_restore_leaves_every_existing_and_candidate_session_untouched() {
    let root = directory("atomic-restore");
    let mut store = Store::open(&root).unwrap_or_else(|error| panic!("open failed: {error}"));
    let tenant_a = tenant("tenant-a");
    let tenant_b = tenant("tenant-b");
    let agent_a = did(b"did:layerx:a");
    let agent_b = did(b"did:layerx:b");
    let identity_a = identity(&mut store, tenant_a.clone(), agent_a.clone());
    let identity_b = identity(&mut store, tenant_b.clone(), agent_b.clone());
    let mut source = SessionRegistry::default();
    open_session(&mut store, &mut source, &identity_a, 1, &tenant_a, &agent_a);
    open_session(&mut store, &mut source, &identity_a, 2, &tenant_a, &agent_a);
    open_session(&mut store, &mut source, &identity_b, 1, &tenant_b, &agent_b);
    let mut corrupt = stored_bytes(&store, &tenant_a, 2);
    corrupt[..6].copy_from_slice(b"LXSR99");
    store
        .put_local(session_key(&tenant_a, 2), corrupt)
        .unwrap_or_else(|error| panic!("corrupt put failed: {error}"));

    let mut target = SessionRegistry::default();
    target
        .restore_tenant(&store, &tenant_b)
        .unwrap_or_else(|error| panic!("tenant-b restore failed: {error:?}"));
    assert_eq!(
        target.restore_tenant(&store, &tenant_a),
        Err(SessionError::MissingField("record_version"))
    );
    assert_eq!(target.open_count(), 1);
    assert_eq!(target.generation(&tenant_a, SessionId([1; 32])), None);
    assert_eq!(target.generation(&tenant_a, SessionId([2; 32])), None);
    assert_eq!(target.generation(&tenant_b, SessionId([1; 32])), Some(1));
    assert!(target
        .get(&tenant_b, SessionId([1; 32]))
        .is_some_and(|record| record.open));
    let _ = fs::remove_dir_all(root);
}
