use std::collections::BTreeSet;
use std::fs;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};

use layerx_agentd::identity::{
    register, CoreIdentity, IdentityError, IdentityResolver, ProtocolAuthority,
};
use layerx_agentd::session::{open, OpenRequest, SessionId, SessionRegistry, Token};
use layerx_agentd::store::{Store, TenantId};
use layerx_agentd::tenant::{
    resolve, AuthorizationError, AuthorizationOutcome, ObjectOwner, RequestContext, Surface,
    TenantObservability,
};
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
        "layerx-tenant-resolve-{label}-{}-{sequence}",
        std::process::id()
    ))
}

fn tenant_id(value: &str) -> TenantId {
    TenantId::new(value).unwrap_or_else(|error| panic!("tenant id {value}: {error}"))
}

fn did(value: &[u8]) -> Did {
    Did::new(value).unwrap_or_else(|error| panic!("did: {error:?}"))
}

fn token(root: &PathBuf) -> (Token, TenantId, Did) {
    let mut store = Store::open(root).unwrap_or_else(|error| panic!("store: {error}"));
    let tenant = tenant_id("tenant-a");
    let agent = did(b"did:layerx:tenant-a:agent");
    let mut boundary = BoundaryIdentity(CoreIdentity {
        canonical_bytes: b"core-identity".to_vec(),
        head_sequence: 10,
        revocation_sequence: 1,
        verification_level: VerificationLevel::STATE_PROVEN,
        frozen: false,
        authorities: vec![ProtocolAuthority::SessionKey([4; 32])],
    });
    let identity = register(&mut store, tenant.clone(), agent.clone(), &mut boundary)
        .unwrap_or_else(|error| panic!("register identity: {error:?}"));
    let mut registry = SessionRegistry::default();
    let token = open(
        &mut store,
        &mut registry,
        &identity,
        OpenRequest {
            session_id: SessionId([1; 32]),
            token_id: [2; 32],
            tenant: tenant.clone(),
            agent: agent.clone(),
            authority: ProtocolAuthority::SessionKey([4; 32]),
            permitted_activity_types: BTreeSet::from([7]),
            scopes: BTreeSet::from(["read".to_owned(), "export".to_owned()]),
            expiry_sequence: 100,
            opening_client: "tenant-resolve-suite".to_owned(),
            policy_version: "policy-v1".to_owned(),
        },
        10,
    )
    .unwrap_or_else(|error| panic!("open session: {error:?}"));
    (token, tenant, agent)
}

fn request(surface: Surface, owner: ObjectOwner) -> RequestContext {
    RequestContext {
        surface,
        operation: "read".to_owned(),
        core_sequence: 20,
        supplied_header_tenant: Some(tenant_id("spoof-header")),
        supplied_body_tenant: Some(tenant_id("spoof-body")),
        target_owner: Some(owner),
    }
}

#[test]
fn spoofed_request_tenants_are_ignored_on_every_surface() {
    let root = directory("surfaces");
    let (token, tenant, agent) = token(&root);
    let mut observability = TenantObservability::default();
    let surfaces = [
        Surface::Contract,
        Surface::RustSdk,
        Surface::TypeScriptSdk,
        Surface::PythonSdk,
        Surface::Mcp,
        Surface::Subscription,
        Surface::Export,
    ];
    for surface in surfaces {
        let resolved = resolve(
            &token,
            &request(
                surface,
                ObjectOwner {
                    tenant: tenant.clone(),
                    agent: Some(agent.clone()),
                },
            ),
            &mut observability,
        )
        .unwrap_or_else(|error| panic!("resolve {surface:?}: {error:?}"));
        assert_eq!(resolved.tenant, tenant);
        assert_eq!(resolved.agent, agent);
        assert_eq!(resolved.surface, surface);
    }
    assert_eq!(observability.audit().len(), surfaces.len());
    assert_eq!(observability.traces().len(), surfaces.len());
    assert_eq!(observability.metrics().len(), surfaces.len());
    assert!(observability
        .audit()
        .iter()
        .all(|entry| entry.tenant == tenant && entry.outcome == AuthorizationOutcome::Allowed));
    assert!(observability
        .traces()
        .iter()
        .all(|trace| trace.tenant == tenant));
    assert!(observability
        .metrics()
        .keys()
        .all(|metric| metric.tenant == tenant));
    let _ = fs::remove_dir_all(root);
}

#[test]
fn cross_tenant_target_and_replayed_token_use_one_non_enumerating_refusal() {
    let root = directory("cross-tenant");
    let (token, tenant, _agent) = token(&root);
    let mut observability = TenantObservability::default();
    let cross_tenant = request(
        Surface::Mcp,
        ObjectOwner {
            tenant: tenant_id("tenant-b"),
            agent: None,
        },
    );
    let absent_in_tenant = request(
        Surface::Mcp,
        ObjectOwner {
            tenant: tenant_id("tenant-b"),
            agent: Some(did(b"did:layerx:missing")),
        },
    );
    assert_eq!(
        resolve(&token, &cross_tenant, &mut observability),
        Err(AuthorizationError::NotAuthorized)
    );
    assert_eq!(
        resolve(&token, &absent_in_tenant, &mut observability),
        Err(AuthorizationError::NotAuthorized)
    );
    assert_eq!(observability.audit().len(), 2);
    assert!(observability.audit().iter().all(|entry| {
        entry.tenant == tenant && entry.outcome == AuthorizationOutcome::NotAuthorized
    }));
    let _ = fs::remove_dir_all(root);
}

#[test]
fn wrong_agent_scope_and_expiry_are_explicit_and_audited() {
    let root = directory("refusals");
    let (token, tenant, _agent) = token(&root);
    let mut observability = TenantObservability::default();
    let mut wrong_agent = request(
        Surface::Contract,
        ObjectOwner {
            tenant: tenant.clone(),
            agent: Some(did(b"did:layerx:tenant-a:other")),
        },
    );
    assert_eq!(
        resolve(&token, &wrong_agent, &mut observability),
        Err(AuthorizationError::NotAuthorized)
    );

    wrong_agent.target_owner = None;
    wrong_agent.operation = "submit".to_owned();
    assert_eq!(
        resolve(&token, &wrong_agent, &mut observability),
        Err(AuthorizationError::ScopeDenied)
    );
    wrong_agent.operation = "read".to_owned();
    wrong_agent.core_sequence = 100;
    assert_eq!(
        resolve(&token, &wrong_agent, &mut observability),
        Err(AuthorizationError::Expired)
    );
    assert_eq!(
        observability
            .audit()
            .iter()
            .map(|entry| entry.outcome)
            .collect::<Vec<_>>(),
        vec![
            AuthorizationOutcome::NotAuthorized,
            AuthorizationOutcome::ScopeDenied,
            AuthorizationOutcome::Expired
        ]
    );
    assert!(observability
        .audit()
        .iter()
        .all(|entry| entry.tenant == tenant));
    let _ = fs::remove_dir_all(root);
}
