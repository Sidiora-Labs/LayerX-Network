use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use layerx_agentd::identity::{
    register, CoreIdentity, IdentityError, IdentityRecord, IdentityResolver, ProtocolAuthority,
};
use layerx_agentd::session::{
    close, invalidate_on_revocation, open, InvalidationReason, OpenRequest, PendingActivity,
    RevocationEvent, SessionId, SessionRef, SessionRegistry, Token,
};
use layerx_agentd::store::{Store, TenantId};
use layerx_agentd::tenant::{
    resolve, AuthorizationError, AuthorizationOutcome, ObjectOwner, Operation, OperationClass,
    RequestContext, Surface, TenantObservability,
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

struct Fixture {
    store: Store,
    registry: SessionRegistry,
    identity: IdentityRecord,
    tenant: TenantId,
    agent: Did,
}

impl Fixture {
    fn open(&mut self, id: u8, authority: u8) -> Token {
        self.open_scoped(id, authority, scopes())
    }

    fn open_scoped(&mut self, id: u8, authority: u8, scopes: BTreeSet<String>) -> Token {
        open(
            &mut self.store,
            &mut self.registry,
            &self.identity,
            OpenRequest {
                session_id: SessionId([id; 32]),
                token_id: [id.wrapping_add(1); 32],
                tenant: self.tenant.clone(),
                agent: self.agent.clone(),
                authority: ProtocolAuthority::SessionKey([authority; 32]),
                permitted_activity_types: BTreeSet::from([7]),
                scopes,
                expiry_sequence: 100,
                opening_client: "tenant-resolve-suite".to_owned(),
                policy_version: "policy-v1".to_owned(),
            },
            10,
        )
        .unwrap_or_else(|error| panic!("open session {id}: {error:?}"))
    }

    fn close(&mut self, id: u8) {
        close(
            &mut self.store,
            &mut self.registry,
            &self.tenant,
            SessionId([id; 32]),
        )
        .unwrap_or_else(|error| panic!("close session {id}: {error:?}"));
    }

    fn revoke_authority(&mut self, authority: u8, observed_sequence: u64) -> Vec<SessionRef> {
        let mut activities: [PendingActivity; 0] = [];
        invalidate_on_revocation(
            &mut self.store,
            &mut self.registry,
            &mut activities,
            &RevocationEvent {
                did: self.agent.clone(),
                authority: Some(ProtocolAuthority::SessionKey([authority; 32])),
                reason: InvalidationReason::SessionKeyRevoked,
                observed_sequence,
            },
        )
        .unwrap_or_else(|error| panic!("revocation: {error:?}"))
        .invalidated_sessions
    }

    fn reload(&mut self) {
        let mut registry = SessionRegistry::default();
        registry
            .restore_tenant(&self.store, &self.tenant)
            .unwrap_or_else(|error| panic!("restore: {error:?}"));
        self.registry = registry;
    }

    fn owner(&self) -> ObjectOwner {
        ObjectOwner {
            tenant: self.tenant.clone(),
            agent: Some(self.agent.clone()),
        }
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

fn scopes() -> BTreeSet<String> {
    OperationClass::ALL
        .iter()
        .map(|class| class.scope().to_owned())
        .collect()
}

fn fixture(root: &Path) -> Fixture {
    let mut store = Store::open(root).unwrap_or_else(|error| panic!("store: {error}"));
    let tenant = tenant_id("tenant-a");
    let agent = did(b"did:layerx:tenant-a:agent");
    let mut boundary = BoundaryIdentity(CoreIdentity {
        canonical_bytes: b"core-identity".to_vec(),
        head_sequence: 10,
        revocation_sequence: 1,
        verification_level: VerificationLevel::STATE_PROVEN,
        frozen: false,
        authorities: vec![
            ProtocolAuthority::SessionKey([4; 32]),
            ProtocolAuthority::SessionKey([5; 32]),
        ],
    });
    let identity = register(&mut store, tenant.clone(), agent.clone(), &mut boundary)
        .unwrap_or_else(|error| panic!("register identity: {error:?}"));
    Fixture {
        store,
        registry: SessionRegistry::default(),
        identity,
        tenant,
        agent,
    }
}

fn request(surface: Surface, owner: ObjectOwner) -> RequestContext {
    RequestContext {
        surface,
        operation: Operation::ReadBalance,
        core_sequence: 20,
        supplied_header_tenant: Some(tenant_id("spoof-header")),
        supplied_body_tenant: Some(tenant_id("spoof-body")),
        target_owner: Some(owner),
    }
}

fn operation_request(
    operation: Operation,
    class: OperationClass,
    owner: ObjectOwner,
) -> RequestContext {
    let surface = match class {
        OperationClass::Subscribe => Surface::Subscription,
        OperationClass::Export => Surface::Export,
        OperationClass::Prepare | OperationClass::Write => Surface::Mcp,
        OperationClass::Approve => Surface::RustSdk,
        OperationClass::Read => Surface::Contract,
    };
    RequestContext {
        surface,
        operation,
        core_sequence: 20,
        supplied_header_tenant: Some(tenant_id("spoof-header")),
        supplied_body_tenant: Some(tenant_id("spoof-body")),
        target_owner: Some(owner),
    }
}

fn gated_operations() -> Vec<(Operation, OperationClass)> {
    Operation::ALL
        .iter()
        .copied()
        .filter_map(|operation| {
            OperationClass::for_operation(operation).map(|class| (operation, class))
        })
        .collect()
}

#[test]
fn spoofed_request_tenants_are_ignored_on_every_surface() {
    let root = directory("surfaces");
    let mut fixture = fixture(&root);
    let token = fixture.open(1, 4);
    let mut observability = TenantObservability::default();
    for surface in Surface::ALL {
        let resolved = resolve(
            &token,
            &fixture.registry,
            &request(surface, fixture.owner()),
            &mut observability,
        )
        .unwrap_or_else(|error| panic!("resolve {surface:?}: {error:?}"));
        assert_eq!(resolved.tenant, fixture.tenant);
        assert_eq!(resolved.agent, fixture.agent);
        assert_eq!(resolved.session_id, SessionId([1; 32]));
        assert_eq!(resolved.surface, surface);
    }
    assert_eq!(observability.audit().len(), Surface::ALL.len());
    assert_eq!(observability.traces().len(), Surface::ALL.len());
    assert_eq!(observability.metrics().len(), Surface::ALL.len());
    assert!(observability.audit().iter().all(|entry| {
        entry.tenant == fixture.tenant && entry.outcome == AuthorizationOutcome::Allowed
    }));
    let correlation = observability.audit()[0].token_correlation;
    assert_ne!(correlation, token.token_id());
    assert!(observability
        .audit()
        .iter()
        .all(|entry| entry.token_correlation == correlation));
    let audit_debug = format!("{:?}", observability.audit()[0]);
    assert!(audit_debug.contains("[REDACTED]"));
    assert!(!audit_debug.contains(&format!("{correlation:?}")));
    assert!(!audit_debug.contains(&format!("{:?}", token.token_id())));
    assert!(observability
        .traces()
        .iter()
        .all(|trace| trace.tenant == fixture.tenant));
    assert!(observability
        .metrics()
        .keys()
        .all(|metric| metric.tenant == fixture.tenant));
    let _ = fs::remove_dir_all(root);
}

#[test]
fn cross_tenant_target_and_replayed_token_use_one_non_enumerating_refusal() {
    let root = directory("cross-tenant");
    let mut fixture = fixture(&root);
    let token = fixture.open(1, 4);
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
        resolve(&token, &fixture.registry, &cross_tenant, &mut observability),
        Err(AuthorizationError::NotAuthorized)
    );
    assert_eq!(
        resolve(
            &token,
            &fixture.registry,
            &absent_in_tenant,
            &mut observability
        ),
        Err(AuthorizationError::NotAuthorized)
    );
    assert_eq!(observability.audit().len(), 2);
    assert!(observability.audit().iter().all(|entry| {
        entry.tenant == fixture.tenant && entry.outcome == AuthorizationOutcome::NotAuthorized
    }));
    let _ = fs::remove_dir_all(root);
}

#[test]
fn wrong_agent_scope_and_expiry_are_explicit_and_audited() {
    let root = directory("refusals");
    let mut fixture = fixture(&root);
    let token = fixture.open_scoped(1, 4, BTreeSet::from(["read".to_owned()]));
    let mut observability = TenantObservability::default();
    let mut wrong_agent = request(
        Surface::Contract,
        ObjectOwner {
            tenant: fixture.tenant.clone(),
            agent: Some(did(b"did:layerx:tenant-a:other")),
        },
    );
    assert_eq!(
        resolve(&token, &fixture.registry, &wrong_agent, &mut observability),
        Err(AuthorizationError::NotAuthorized)
    );

    wrong_agent.target_owner = None;
    wrong_agent.operation = Operation::Submit;
    assert_eq!(
        resolve(&token, &fixture.registry, &wrong_agent, &mut observability),
        Err(AuthorizationError::ScopeDenied)
    );
    wrong_agent.operation = Operation::ReadBalance;
    wrong_agent.core_sequence = 100;
    assert_eq!(
        resolve(&token, &fixture.registry, &wrong_agent, &mut observability),
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
        .all(|entry| entry.tenant == fixture.tenant));
    let _ = fs::remove_dir_all(root);
}

#[test]
fn every_generated_token_gated_operation_refuses_closed_revoked_and_restarted_sessions() {
    let root = directory("entry-points");
    let mut fixture = fixture(&root);
    let operations = gated_operations();
    assert_eq!(operations.len(), Operation::ALL.len() - 2);
    assert_eq!(
        OperationClass::for_operation(Operation::AgentRegister),
        None
    );
    assert_eq!(OperationClass::for_operation(Operation::SessionOpen), None);
    for class in OperationClass::ALL {
        assert!(operations.iter().any(|(_, actual)| *actual == class));
    }
    let closed = fixture.open(1, 4);
    let revoked = fixture.open(2, 5);
    let restarted = fixture.open(3, 4);
    let live = fixture.open(4, 4);
    let mut admitted = TenantObservability::default();
    for (operation, class) in &operations {
        for token in [&closed, &revoked, &restarted, &live] {
            let resolved = resolve(
                token,
                &fixture.registry,
                &operation_request(*operation, *class, fixture.owner()),
                &mut admitted,
            )
            .unwrap_or_else(|error| panic!("{operation:?} before any change: {error:?}"));
            assert_eq!(resolved.tenant, fixture.tenant);
        }
    }
    assert_eq!(admitted.audit().len(), operations.len() * 4);
    assert!(admitted
        .audit()
        .iter()
        .all(|entry| entry.outcome == AuthorizationOutcome::Allowed));

    fixture.close(1);
    assert_eq!(
        fixture.revoke_authority(5, 30),
        vec![SessionRef::new(fixture.tenant.clone(), SessionId([2; 32]))]
    );
    fixture.close(3);
    fixture.reload();
    assert_eq!(
        fixture
            .registry
            .generation(&fixture.tenant, SessionId([1; 32])),
        Some(2)
    );
    assert_eq!(
        fixture
            .registry
            .generation(&fixture.tenant, SessionId([2; 32])),
        Some(2)
    );
    assert_eq!(
        fixture
            .registry
            .generation(&fixture.tenant, SessionId([3; 32])),
        Some(2)
    );
    assert_eq!(
        fixture
            .registry
            .generation(&fixture.tenant, SessionId([4; 32])),
        Some(1)
    );

    let mut refused = TenantObservability::default();
    for (operation, class) in &operations {
        for (label, token) in [
            ("closed", &closed),
            ("revoked", &revoked),
            ("restarted", &restarted),
        ] {
            assert_eq!(
                resolve(
                    token,
                    &fixture.registry,
                    &operation_request(*operation, *class, fixture.owner()),
                    &mut refused,
                )
                .err(),
                Some(AuthorizationError::Revoked),
                "{label} session must be refused at {operation:?}"
            );
        }
        let resolved = resolve(
            &live,
            &fixture.registry,
            &operation_request(*operation, *class, fixture.owner()),
            &mut refused,
        )
        .unwrap_or_else(|error| panic!("{operation:?} live session: {error:?}"));
        assert_eq!(resolved.session_id, SessionId([4; 32]));
    }
    assert_eq!(refused.audit().len(), operations.len() * 4);
    assert_eq!(
        refused
            .audit()
            .iter()
            .filter(|entry| entry.outcome == AuthorizationOutcome::Revoked)
            .count(),
        operations.len() * 3
    );
    assert_eq!(
        refused
            .audit()
            .iter()
            .filter(|entry| entry.outcome == AuthorizationOutcome::Allowed)
            .count(),
        operations.len()
    );
    assert!(refused
        .audit()
        .iter()
        .all(|entry| entry.tenant == fixture.tenant));
    let _ = fs::remove_dir_all(root);
}

#[test]
fn bootstrap_operations_are_explicitly_outside_the_existing_token_gate() {
    let root = directory("bootstrap-operations");
    let mut fixture = fixture(&root);
    let token = fixture.open(1, 4);
    let mut observability = TenantObservability::default();
    for operation in [Operation::AgentRegister, Operation::SessionOpen] {
        assert_eq!(OperationClass::for_operation(operation), None);
        let mut context = request(Surface::RustSdk, fixture.owner());
        context.operation = operation;
        assert_eq!(
            resolve(&token, &fixture.registry, &context, &mut observability),
            Err(AuthorizationError::InvalidRequest)
        );
    }
    assert_eq!(observability.audit().len(), 2);
    assert!(observability
        .audit()
        .iter()
        .all(|entry| entry.outcome == AuthorizationOutcome::InvalidRequest));
    let _ = fs::remove_dir_all(root);
}

#[test]
fn revocation_is_reported_before_expiry_for_every_generated_gated_operation() {
    let root = directory("revoked-before-expired");
    let mut fixture = fixture(&root);
    let closed = fixture.open(1, 4);
    let live = fixture.open(2, 4);
    fixture.close(1);
    let mut observability = TenantObservability::default();
    for (operation, class) in gated_operations() {
        let mut expired = operation_request(operation, class, fixture.owner());
        expired.core_sequence = 100;
        assert_eq!(
            resolve(&closed, &fixture.registry, &expired, &mut observability).err(),
            Some(AuthorizationError::Revoked),
            "{operation:?}"
        );
        assert_eq!(
            resolve(&live, &fixture.registry, &expired, &mut observability).err(),
            Some(AuthorizationError::Expired),
            "{operation:?}"
        );
    }
    assert_ne!(AuthorizationError::Revoked, AuthorizationError::Expired);
    assert_eq!(
        observability
            .audit()
            .iter()
            .filter(|entry| entry.outcome == AuthorizationOutcome::Revoked)
            .count(),
        gated_operations().len()
    );
    let _ = fs::remove_dir_all(root);
}

#[test]
fn an_unloaded_registry_refuses_every_generated_gated_operation_until_reload() {
    let root = directory("unloaded");
    let mut fixture = fixture(&root);
    let token = fixture.open(1, 4);
    fixture.registry = SessionRegistry::default();
    let mut observability = TenantObservability::default();
    for (operation, class) in gated_operations() {
        assert_eq!(
            resolve(
                &token,
                &fixture.registry,
                &operation_request(operation, class, fixture.owner()),
                &mut observability,
            )
            .err(),
            Some(AuthorizationError::Revoked),
            "{operation:?}"
        );
    }
    fixture.reload();
    assert_eq!(
        fixture
            .registry
            .generation(&fixture.tenant, SessionId([1; 32])),
        Some(1)
    );
    for (operation, class) in gated_operations() {
        let resolved = resolve(
            &token,
            &fixture.registry,
            &operation_request(operation, class, fixture.owner()),
            &mut observability,
        )
        .unwrap_or_else(|error| panic!("{operation:?} after reload: {error:?}"));
        assert_eq!(resolved.session_id, SessionId([1; 32]));
    }
    assert_eq!(observability.audit().len(), gated_operations().len() * 2);
    let _ = fs::remove_dir_all(root);
}
