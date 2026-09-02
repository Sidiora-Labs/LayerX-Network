use std::cell::Cell;
use std::collections::BTreeSet;
use std::fs;
use std::path::Path;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

use layerx_agentd::budget::{BudgetLimiter, LimitConfig, LimitId, LimitScope};
use layerx_agentd::capability::{Capability, CapabilityDimensions, CapabilityId, RateCeiling};
use layerx_agentd::identity::{
    register, CoreIdentity, IdentityError, IdentityResolver, ProtocolAuthority,
};
use layerx_agentd::prepare::PreparationLifecycle;
use layerx_agentd::session::{
    open, OpenRequest, SessionCredential, SessionId, SessionRecord, SessionRegistry,
};
use layerx_agentd::session_control::SessionControl;
use layerx_agentd::store::{Store, TenantId};
use layerx_mcp::server::{
    catalogue, DeploymentMode, InvocationOutcome, ReadOnly, Server, ServerError, ToolKind,
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

fn control(
    root: &Path,
    session: &SessionRecord,
    capability: &Capability,
) -> (SessionControl, SessionCredential) {
    let mut store =
        Store::open(root.join("store")).unwrap_or_else(|error| panic!("store: {error}"));
    capability
        .persist(&mut store)
        .unwrap_or_else(|error| panic!("capability persist: {error:?}"));
    let mut boundary = BoundaryIdentity(CoreIdentity {
        canonical_bytes: b"model-identity".to_vec(),
        head_sequence: 10,
        revocation_sequence: 1,
        verification_level: VerificationLevel::STATE_PROVEN,
        frozen: false,
        authorities: vec![session.request.authority.clone()],
    });
    let identity = register(
        &mut store,
        session.request.tenant.clone(),
        session.request.agent.clone(),
        &mut boundary,
    )
    .unwrap_or_else(|error| panic!("identity: {error:?}"));
    let mut sessions = SessionRegistry::default();
    let credential = open(
        &mut store,
        &mut sessions,
        &identity,
        session.request.clone(),
        50,
    )
    .unwrap_or_else(|error| panic!("session: {error:?}"))
    .credential();
    let budgets = BudgetLimiter::new(vec![LimitConfig {
        id: LimitId([9; 16]),
        name: "mcp-limit".to_owned(),
        scope: LimitScope::Tenant([1; 32]),
        ceiling: 1_000,
        consumed: 0,
    }])
    .unwrap_or_else(|error| panic!("limiter: {error:?}"));
    (
        SessionControl::new(
            Arc::new(Mutex::new(store)),
            sessions,
            Arc::new(PreparationLifecycle::default()),
            Arc::new(budgets),
        ),
        credential,
    )
}

fn directory(label: &str) -> std::path::PathBuf {
    let sequence = NEXT_DIRECTORY.fetch_add(1, Ordering::Relaxed);
    std::env::temp_dir().join(format!(
        "layerx-mcp-readonly-{label}-{}-{sequence}",
        std::process::id()
    ))
}

fn records() -> (SessionRecord, Capability) {
    let tenant = TenantId::new("tenant-a").unwrap_or_else(|error| panic!("tenant: {error}"));
    let capability = Capability::new(
        CapabilityId([9; 32]),
        tenant.clone(),
        CapabilityDimensions {
            activity_types: BTreeSet::from([7]),
            counterparties: BTreeSet::from([[2; 32]]),
            assets: BTreeSet::from([[3; 32]]),
            amount_ceiling: 100,
            rate_ceiling: RateCeiling {
                maximum_uses: 2,
                window_sequences: 10,
            },
            purposes: BTreeSet::from(["service-payment".to_owned()]),
            expiry_sequence: 200,
        },
    )
    .unwrap_or_else(|error| panic!("capability: {error:?}"));
    let scopes = catalogue()
        .iter()
        .map(|tool| tool.required_scope.to_owned())
        .collect();
    let session = SessionRecord {
        request: OpenRequest {
            session_id: SessionId([7; 32]),
            token_id: [8; 32],
            tenant,
            agent: Did::new(b"did:layerx:model").unwrap_or_else(|error| panic!("DID: {error:?}")),
            authority: ProtocolAuthority::CapabilityGrant(capability.id.0),
            permitted_activity_types: BTreeSet::from([7]),
            scopes,
            expiry_sequence: 150,
            opening_client: "mcp".to_owned(),
            policy_version: "policy-v1".to_owned(),
        },
        open: true,
        sequence: 0,
        budget_reserved: 0,
        subscription_cursor: 0,
        generation: 1,
        retired_token_ids: BTreeSet::new(),
    };
    (session, capability)
}

#[test]
fn read_only_capability_and_catalogue_expose_no_write_surface() {
    let root = directory("catalogue");
    let (session, capability) = records();
    let (control, credential) = control(&root, &session, &capability);
    let server = ReadOnly::bind(control, credential, capability.id, 50, &root)
        .unwrap_or_else(|error| panic!("bind: {error:?}"));
    let declaration = server.capability_declaration();
    assert_eq!(declaration.mode, DeploymentMode::ReadOnly);
    assert_eq!(declaration.write_tools, 0);
    assert!(!declaration.mutations_reachable);
    assert!(declaration.read_tools > 0);
    assert!(server
        .tools()
        .iter()
        .all(|tool| tool.kind == ToolKind::Read && tool.mutation == "none"));
    assert_eq!(server.binding().generation(), 1);
    let _ = fs::remove_dir_all(root);
}

#[test]
fn write_tools_are_absent_through_every_model_reachable_shape() {
    let root = directory("shapes");
    let (session, capability) = records();
    let (control, credential) = control(&root, &session, &capability);
    let mut server = ReadOnly::bind(control, credential, capability.id, 50, &root)
        .unwrap_or_else(|error| panic!("bind: {error:?}"));
    let write_tools: Vec<_> = catalogue()
        .iter()
        .filter(|tool| tool.kind == ToolKind::Write)
        .collect();
    assert!(!write_tools.is_empty());
    for write in write_tools {
        assert!(!server.tools().iter().any(|tool| tool.name == write.name));
        for name in [
            write.name.to_owned(),
            format!("{}?cursor=write", write.name),
            format!("resources/{}", write.name),
            format!("errors/retry/{}", write.name),
        ] {
            assert!(matches!(
                server.execute_authorized(50, &name, b"untrusted".to_vec(), |_| (
                    (),
                    InvocationOutcome::Completed
                )),
                Err(ServerError::ToolAbsent)
            ));
        }
    }
    assert_eq!(server.audit_entries(), 0);
    let _ = fs::remove_dir_all(root);
}

#[test]
fn read_only_mode_never_invokes_a_write_executor() {
    let full_root = directory("full");
    let read_root = directory("read");
    let (session, capability) = records();
    let (control, credential) = control(&full_root, &session, &capability);
    let mut full = Server::bind(
        control.clone(),
        credential.clone(),
        capability.id,
        50,
        &full_root,
    )
    .unwrap_or_else(|error| panic!("full bind: {error:?}"));
    let full_declaration = full.capability_declaration();
    assert_eq!(full_declaration.mode, DeploymentMode::Full);
    assert!(full_declaration.write_tools > 0);
    assert!(full_declaration.mutations_reachable);
    full.execute_committed(50, "activity.submit", b"validated write".to_vec(), |_| {
        ((), InvocationOutcome::Completed)
    })
    .unwrap_or_else(|error| panic!("full execute: {error:?}"));

    let mut read_only = ReadOnly::bind(control, credential, capability.id, 50, &read_root)
        .unwrap_or_else(|error| panic!("read-only bind: {error:?}"));
    let invoked = Cell::new(false);
    assert!(matches!(
        read_only.execute_authorized(50, "activity.submit", b"validated write".to_vec(), |_| {
            invoked.set(true);
            ((), InvocationOutcome::Completed)
        }),
        Err(ServerError::ToolAbsent)
    ));
    assert!(!invoked.get());
    assert_eq!(read_only.audit_entries(), 0);
    let _ = fs::remove_dir_all(full_root);
    let _ = fs::remove_dir_all(read_root);
}

#[test]
fn write_only_scope_cannot_construct_a_read_only_server() {
    let root = directory("write-only");
    let (mut session, capability) = records();
    session.request.scopes = BTreeSet::from(["write:submit".to_owned()]);
    let (control, credential) = control(&root, &session, &capability);
    assert!(matches!(
        ReadOnly::bind(control, credential, capability.id, 50, &root.join("audit")),
        Err(ServerError::NoScope)
    ));
    let _ = fs::remove_dir_all(root);
}

#[test]
fn read_only_server_refuses_every_read_once_its_session_is_closed() {
    let root = directory("revoked");
    let (session, capability) = records();
    let (control, credential) = control(&root, &session, &capability);
    let mut server = ReadOnly::bind(
        control.clone(),
        credential.clone(),
        capability.id,
        50,
        &root,
    )
    .unwrap_or_else(|error| panic!("bind: {error:?}"));
    assert_eq!(server.binding().generation(), 1);
    let reads = server.tools().to_vec();
    assert!(!reads.is_empty());
    server
        .execute_authorized(50, "balance.get", b"arguments".to_vec(), |_| {
            ((), InvocationOutcome::Completed)
        })
        .unwrap_or_else(|error| panic!("execute: {error:?}"));
    control
        .close(&session.request.tenant, SessionId([7; 32]), 50)
        .unwrap_or_else(|error| panic!("close: {error:?}"));
    for read in &reads {
        assert!(
            matches!(
                server.execute_authorized(50, read.name, b"arguments".to_vec(), |_| (
                    (),
                    InvocationOutcome::Completed
                )),
                Err(ServerError::RevokedSession)
            ),
            "{} must refuse a closed session",
            read.name
        );
    }
    assert_eq!(
        usize::try_from(server.audit_entries()).ok(),
        Some(2 + reads.len())
    );
    assert!(matches!(
        ReadOnly::bind(control, credential, capability.id, 50, &root.join("rebind")),
        Err(ServerError::RevokedSession)
    ));
    let _ = fs::remove_dir_all(root);
}
