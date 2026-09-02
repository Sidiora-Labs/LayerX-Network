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
    invalidate_on_revocation, open, InvalidationReason, OpenRequest, RevocationEvent,
    SessionCredential, SessionError, SessionId, SessionRecord, SessionRef, SessionRegistry,
};
use layerx_agentd::session_control::SessionControl;
use layerx_agentd::store::{Store, TenantId};
use layerx_mcp::server::{
    catalogue, DaemonGate, InvocationOutcome, Server, ServerError, ToolDefinition, ToolKind,
    REQUIRED_DAEMON_GATES,
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

fn directory(label: &str) -> std::path::PathBuf {
    let sequence = NEXT_DIRECTORY.fetch_add(1, Ordering::Relaxed);
    std::env::temp_dir().join(format!(
        "layerx-mcp-scope-{label}-{}-{sequence}",
        std::process::id()
    ))
}

fn tenant(name: &str) -> TenantId {
    TenantId::new(name).unwrap_or_else(|error| panic!("tenant: {error}"))
}

fn session_control(store: Store, sessions: SessionRegistry) -> SessionControl {
    let budgets = BudgetLimiter::new(vec![LimitConfig {
        id: LimitId([9; 16]),
        name: "mcp-limit".to_owned(),
        scope: LimitScope::Tenant([1; 32]),
        ceiling: 1_000,
        consumed: 0,
    }])
    .unwrap_or_else(|error| panic!("limiter: {error:?}"));
    SessionControl::new(
        Arc::new(Mutex::new(store)),
        sessions,
        Arc::new(PreparationLifecycle::default()),
        Arc::new(budgets),
    )
}

fn open_session(
    store: &mut Store,
    sessions: &mut SessionRegistry,
    session: &SessionRecord,
) -> SessionCredential {
    let mut boundary = BoundaryIdentity(CoreIdentity {
        canonical_bytes: b"model-identity".to_vec(),
        head_sequence: 10,
        revocation_sequence: 1,
        verification_level: VerificationLevel::STATE_PROVEN,
        frozen: false,
        authorities: vec![session.request.authority.clone()],
    });
    let identity = register(
        store,
        session.request.tenant.clone(),
        session.request.agent.clone(),
        &mut boundary,
    )
    .unwrap_or_else(|error| panic!("identity: {error:?}"));
    open(store, sessions, &identity, session.request.clone(), 50)
        .unwrap_or_else(|error| panic!("session: {error:?}"))
        .credential()
}

fn bind_control(
    root: &Path,
    session: &SessionRecord,
    capability: &Capability,
) -> (SessionControl, SessionCredential) {
    let mut store =
        Store::open(root.join("store")).unwrap_or_else(|error| panic!("store: {error}"));
    capability
        .persist(&mut store)
        .unwrap_or_else(|error| panic!("capability persist: {error:?}"));
    let mut sessions = SessionRegistry::default();
    let credential = open_session(&mut store, &mut sessions, session);
    (session_control(store, sessions), credential)
}

fn restart(root: &Path, tenant: &TenantId) -> SessionControl {
    let store = Store::open(root.join("store")).unwrap_or_else(|error| panic!("reopen: {error}"));
    let mut sessions = SessionRegistry::default();
    sessions
        .restore_tenant(&store, tenant)
        .unwrap_or_else(|error| panic!("restore: {error:?}"));
    session_control(store, sessions)
}

fn execute(
    server: &mut Server,
    core_sequence: u64,
    tool: &ToolDefinition,
    arguments: Vec<u8>,
) -> Result<(), ServerError> {
    match tool.kind {
        ToolKind::Read => server.execute_read(core_sequence, tool.name, arguments, |_| {
            ((), InvocationOutcome::Completed)
        }),
        ToolKind::Write => server.execute_committed(core_sequence, tool.name, arguments, |_| {
            ((), InvocationOutcome::Completed)
        }),
    }
}

fn records(scopes: &[&str]) -> (SessionRecord, Capability) {
    let tenant = tenant("tenant-a");
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
    let session = SessionRecord {
        request: OpenRequest {
            session_id: SessionId([7; 32]),
            token_id: [8; 32],
            tenant,
            agent: Did::new(b"did:layerx:model").unwrap_or_else(|error| panic!("DID: {error:?}")),
            authority: ProtocolAuthority::CapabilityGrant(capability.id.0),
            permitted_activity_types: BTreeSet::from([7]),
            scopes: scopes.iter().map(|scope| (*scope).to_owned()).collect(),
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
fn every_catalogue_tool_has_a_canonical_operation_and_reauthorizes_at_route_and_completion() {
    let root = directory("operation-mapping");
    let scopes = catalogue()
        .iter()
        .map(|tool| tool.required_scope)
        .collect::<Vec<_>>();
    let (session, capability) = records(&scopes);
    let (control, credential) = bind_control(&root, &session, &capability);
    let mut server = Server::bind(control, credential, capability.id, 50, &root)
        .unwrap_or_else(|error| panic!("bind: {error:?}"));
    assert_eq!(server.tools(), catalogue());
    for tool in catalogue() {
        let checked = match tool.kind {
            ToolKind::Read => {
                server.execute_read(50, tool.name, b"arguments".to_vec(), |invocation| {
                    assert_eq!(invocation.tool(), *tool);
                    ((), InvocationOutcome::Completed)
                })
            }
            ToolKind::Write => {
                server.execute_committed(50, tool.name, b"arguments".to_vec(), |invocation| {
                    assert_eq!(invocation.tool(), *tool);
                    ((), InvocationOutcome::Completed)
                })
            }
        };
        checked.unwrap_or_else(|error| panic!("execute {}: {error:?}", tool.name));
        let crossed = match tool.kind {
            ToolKind::Read => {
                server.execute_committed(50, tool.name, b"arguments".to_vec(), |_| {
                    ((), InvocationOutcome::Completed)
                })
            }
            ToolKind::Write => server.execute_read(50, tool.name, b"arguments".to_vec(), |_| {
                ((), InvocationOutcome::Completed)
            }),
        };
        assert!(
            matches!(crossed, Err(ServerError::ToolAbsent)),
            "{} must not run through the other tool kind",
            tool.name
        );
    }
    assert_eq!(
        usize::try_from(server.audit_entries()).ok(),
        Some(catalogue().len() * 2)
    );
    let _ = fs::remove_dir_all(root);
}

#[test]
fn server_exposes_only_the_exact_bound_scope_and_separates_read_from_write() {
    let root = directory("filter");
    let (session, capability) = records(&["read:balance", "write:prepare"]);
    let (control, credential) = bind_control(&root, &session, &capability);
    let server = Server::bind(control, credential, capability.id, 50, &root)
        .unwrap_or_else(|error| panic!("bind: {error:?}"));
    assert_eq!(server.binding().tenant().as_str(), "tenant-a");
    assert_eq!(server.binding().session_id(), SessionId([7; 32]));
    assert_eq!(server.binding().capability_id(), capability.id);
    assert_eq!(server.binding().generation(), 1);
    assert_eq!(server.binding().scopes().len(), 2);
    assert_eq!(server.tools().len(), 2);
    assert_eq!(
        server
            .tool("balance.get")
            .unwrap_or_else(|| panic!("balance absent"))
            .kind,
        ToolKind::Read
    );
    assert_eq!(
        server
            .tool("activity.prepare")
            .unwrap_or_else(|| panic!("prepare absent"))
            .kind,
        ToolKind::Write
    );
    assert!(server.tool("history.list").is_none());
    assert!(server.tool("activity.submit").is_none());
    let binding_debug = format!("{:?}", server.binding());
    assert!(binding_debug.contains("[REDACTED]"));
    assert!(!binding_debug.contains(&format!("{:?}", [8_u8; 32])));
    let _ = fs::remove_dir_all(root);
}

#[test]
fn every_reachable_tool_is_an_audited_daemon_invocation_with_all_gates() {
    let root = directory("route");
    let (session, capability) = records(&["read:balance"]);
    let (control, credential) = bind_control(&root, &session, &capability);
    let mut server = Server::bind(control, credential, capability.id, 50, &root)
        .unwrap_or_else(|error| panic!("bind: {error:?}"));
    assert!(matches!(
        server.execute_committed(50, "activity.submit", b"untrusted".to_vec(), |_| (
            (),
            InvocationOutcome::Completed
        )),
        Err(ServerError::ToolAbsent)
    ));
    let digest = server
        .execute_read(
            50,
            "balance.get",
            b"schema-validated-arguments".to_vec(),
            |invocation| {
                assert_eq!(invocation.gates(), REQUIRED_DAEMON_GATES);
                assert_eq!(
                    invocation.gates(),
                    [
                        DaemonGate::Policy,
                        DaemonGate::Capability,
                        DaemonGate::Budget,
                        DaemonGate::RateLimit,
                        DaemonGate::Audit,
                    ]
                );
                assert_eq!(invocation.invocation_id(), 0);
                assert_eq!(invocation.arguments(), b"schema-validated-arguments");
                (invocation.arguments_digest(), InvocationOutcome::Completed)
            },
        )
        .unwrap_or_else(|error| panic!("execute: {error:?}"));
    assert_ne!(digest, [0; 32]);
    assert_eq!(server.audit_entries(), 3);
    let _ = fs::remove_dir_all(root);
}

#[test]
fn binding_refuses_unlinked_cross_tenant_expired_empty_and_unloaded_authority() {
    let root = directory("refuse");
    let (session, capability) = records(&["read:balance"]);
    let mut unlinked = session.clone();
    unlinked.request.authority = ProtocolAuthority::CapabilityGrant([1; 32]);
    let (control, credential) = bind_control(&root.join("unlinked"), &unlinked, &capability);
    assert!(matches!(
        Server::bind(
            control,
            credential,
            capability.id,
            50,
            &root.join("unlinked-audit")
        ),
        Err(ServerError::CapabilityMismatch)
    ));
    let mut cross_tenant = capability.clone();
    cross_tenant.tenant = tenant("tenant-b");
    let (control, credential) = bind_control(&root.join("cross"), &session, &cross_tenant);
    assert!(matches!(
        Server::bind(
            control,
            credential,
            cross_tenant.id,
            50,
            &root.join("cross-audit")
        ),
        Err(ServerError::MissingCapability)
    ));
    let (control, credential) = bind_control(&root.join("expired"), &session, &capability);
    assert!(matches!(
        Server::bind(
            control,
            credential,
            capability.id,
            150,
            &root.join("expired-audit")
        ),
        Err(ServerError::ExpiredAuthority)
    ));
    let (empty, capability) = records(&["not-a-tool-scope"]);
    let (control, credential) = bind_control(&root.join("empty"), &empty, &capability);
    assert!(matches!(
        Server::bind(
            control,
            credential,
            capability.id,
            50,
            &root.join("empty-audit")
        ),
        Err(ServerError::NoScope)
    ));
    let (control, credential) = bind_control(&root.join("unloaded"), &session, &capability);
    let store = control.store();
    drop(control);
    let store = Arc::try_unwrap(store)
        .unwrap_or_else(|_| panic!("store is still shared"))
        .into_inner()
        .unwrap_or_else(|error| panic!("store lock: {error}"));
    let unloaded = session_control(store, SessionRegistry::default());
    assert!(matches!(
        Server::bind(
            unloaded,
            credential,
            capability.id,
            50,
            &root.join("unloaded-audit")
        ),
        Err(ServerError::RevokedSession)
    ));
    let _ = fs::remove_dir_all(root);
}

#[test]
fn bound_server_refuses_every_tool_once_its_session_is_closed_or_revoked() {
    let root = directory("revoked");
    let (session, capability) = records(&["read:balance", "write:prepare"]);
    let (control, credential) = bind_control(&root, &session, &capability);
    let mut server = Server::bind(
        control.clone(),
        credential.clone(),
        capability.id,
        50,
        &root,
    )
    .unwrap_or_else(|error| panic!("bind: {error:?}"));
    assert_eq!(server.binding().generation(), 1);
    let tools = server.tools().to_vec();
    assert_eq!(tools.len(), 2);
    for tool in &tools {
        execute(&mut server, 50, tool, b"arguments".to_vec())
            .unwrap_or_else(|error| panic!("live execute {}: {error:?}", tool.name));
    }

    let closing = control.clone();
    let tenant = session.request.tenant.clone();
    assert!(matches!(
        server.execute_read(50, "balance.get", b"pending".to_vec(), |_| {
            closing
                .close(&tenant, SessionId([7; 32]), 50)
                .unwrap_or_else(|error| panic!("close: {error:?}"));
            ((), InvocationOutcome::Completed)
        }),
        Err(ServerError::RevokedSession)
    ));
    {
        let registry = control.registry();
        let sessions = registry
            .read()
            .unwrap_or_else(|error| panic!("session registry: {error}"));
        assert_eq!(sessions.generation(&tenant, SessionId([7; 32])), Some(2));
        assert!(matches!(
            sessions.authenticate(&credential),
            Err(SessionError::Revoked)
        ));
    }
    for tool in &tools {
        assert!(
            matches!(
                execute(&mut server, 50, tool, b"arguments".to_vec()),
                Err(ServerError::RevokedSession)
            ),
            "{} must refuse a closed session",
            tool.name
        );
    }
    assert_eq!(
        usize::try_from(server.audit_entries()).ok(),
        Some(tools.len() * 3 + 1)
    );
    assert!(matches!(
        Server::bind(
            control.clone(),
            credential.clone(),
            capability.id,
            50,
            &root.join("rebind")
        ),
        Err(ServerError::RevokedSession)
    ));
    drop(server);
    drop(control);

    let restored = restart(&root, &tenant);
    {
        let registry = restored.registry();
        let sessions = registry
            .read()
            .unwrap_or_else(|error| panic!("restored registry: {error}"));
        assert_eq!(sessions.generation(&tenant, SessionId([7; 32])), Some(2));
        let closed = sessions
            .get(&tenant, SessionId([7; 32]))
            .unwrap_or_else(|| panic!("closed session missing"));
        assert!(!closed.open);
        assert!(matches!(
            sessions.authenticate_bearer(&tenant, SessionId([7; 32]), [8; 32]),
            Err(SessionError::Revoked)
        ));
    }
    assert!(matches!(
        Server::bind(
            restored,
            credential,
            capability.id,
            50,
            &root.join("restart")
        ),
        Err(ServerError::RevokedSession)
    ));
    let _ = fs::remove_dir_all(root);
}

#[test]
fn authority_revocation_refuses_the_bound_server_and_leaves_a_sibling_session_live() {
    let root = directory("authority-revoked");
    let (session, capability) = records(&["read:balance"]);
    let (control, credential) = bind_control(&root, &session, &capability);
    let mut sibling = session.clone();
    sibling.request.session_id = SessionId([11; 32]);
    sibling.request.token_id = [12; 32];
    sibling.request.agent =
        Did::new(b"did:layerx:sibling").unwrap_or_else(|error| panic!("DID: {error:?}"));
    let sibling_credential = {
        let registry = control.registry();
        let mut sessions = registry
            .write()
            .unwrap_or_else(|error| panic!("session registry: {error}"));
        let store = control.store();
        let mut store = store
            .lock()
            .unwrap_or_else(|error| panic!("store: {error}"));
        open_session(&mut store, &mut sessions, &sibling)
    };
    let mut server = Server::bind(control.clone(), credential, capability.id, 50, &root)
        .unwrap_or_else(|error| panic!("bind: {error:?}"));
    let mut sibling_server = Server::bind(
        control.clone(),
        sibling_credential,
        capability.id,
        50,
        &root.join("sibling"),
    )
    .unwrap_or_else(|error| panic!("sibling bind: {error:?}"));
    let revoking = control.clone();
    let mut report = None;
    assert!(matches!(
        server.execute_read(60, "balance.get", b"pending".to_vec(), |_| {
            let registry = revoking.registry();
            let mut sessions = registry
                .write()
                .unwrap_or_else(|error| panic!("session registry: {error}"));
            let store = revoking.store();
            let mut store = store
                .lock()
                .unwrap_or_else(|error| panic!("store: {error}"));
            report = Some(
                invalidate_on_revocation(
                    &mut store,
                    &mut sessions,
                    &mut [],
                    &RevocationEvent {
                        did: session.request.agent.clone(),
                        authority: Some(session.request.authority.clone()),
                        reason: InvalidationReason::CapabilityGrantRevoked,
                        observed_sequence: 60,
                    },
                )
                .unwrap_or_else(|error| panic!("revocation: {error:?}")),
            );
            ((), InvocationOutcome::Completed)
        }),
        Err(ServerError::RevokedSession)
    ));
    let report = report.unwrap_or_else(|| panic!("revocation report missing"));
    assert_eq!(
        report.invalidated_sessions,
        vec![SessionRef::new(
            session.request.tenant.clone(),
            SessionId([7; 32])
        )]
    );
    assert!(matches!(
        server.execute_read(60, "balance.get", b"arguments".to_vec(), |_| (
            (),
            InvocationOutcome::Completed
        )),
        Err(ServerError::RevokedSession)
    ));
    sibling_server
        .execute_read(60, "balance.get", b"arguments".to_vec(), |_| {
            ((), InvocationOutcome::Completed)
        })
        .unwrap_or_else(|error| panic!("sibling execute: {error:?}"));
    let registry = control.registry();
    let sessions = registry
        .read()
        .unwrap_or_else(|error| panic!("session registry: {error}"));
    assert_eq!(
        sessions.generation(&session.request.tenant, SessionId([7; 32])),
        Some(2)
    );
    assert_eq!(
        sessions.generation(&session.request.tenant, SessionId([11; 32])),
        Some(1)
    );
    let _ = fs::remove_dir_all(root);
}

#[test]
fn a_committed_write_is_linearized_against_a_concurrent_close() {
    let root = directory("linearized");
    let (session, capability) = records(&["read:balance", "write:prepare"]);
    let (control, credential) = bind_control(&root, &session, &capability);
    let mut server = Server::bind(control.clone(), credential, capability.id, 50, &root)
        .unwrap_or_else(|error| panic!("bind: {error:?}"));
    let tenant = session.request.tenant.clone();
    let (started_tx, started_rx) = std::sync::mpsc::channel();
    let (closed_tx, closed_rx) = std::sync::mpsc::channel();
    let closing = control.clone();
    let closing_tenant = tenant.clone();
    let closer = std::thread::spawn(move || {
        started_rx
            .recv()
            .unwrap_or_else(|error| panic!("start coordination: {error}"));
        closing
            .close(&closing_tenant, SessionId([7; 32]), 50)
            .unwrap_or_else(|error| panic!("close: {error:?}"));
        closed_tx
            .send(())
            .unwrap_or_else(|error| panic!("close coordination: {error}"));
    });
    server
        .execute_committed(50, "activity.prepare", b"validated".to_vec(), |_| {
            started_tx
                .send(())
                .unwrap_or_else(|error| panic!("start signal: {error}"));
            assert!(
                closed_rx
                    .recv_timeout(std::time::Duration::from_millis(200))
                    .is_err(),
                "close must wait for the committed write to finish"
            );
            ((), InvocationOutcome::Completed)
        })
        .unwrap_or_else(|error| panic!("committed execute: {error:?}"));
    closed_rx
        .recv()
        .unwrap_or_else(|error| panic!("close completion: {error}"));
    assert!(closer.join().is_ok(), "closer panicked");
    assert!(matches!(
        server.execute_committed(50, "activity.prepare", b"validated".to_vec(), |_| (
            (),
            InvocationOutcome::Completed
        )),
        Err(ServerError::RevokedSession)
    ));
    let registry = control.registry();
    let sessions = registry
        .read()
        .unwrap_or_else(|error| panic!("session registry: {error}"));
    assert_eq!(sessions.generation(&tenant, SessionId([7; 32])), Some(2));
    let _ = fs::remove_dir_all(root);
}
