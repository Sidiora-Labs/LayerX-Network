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
use layerx_agentd::session_control::{SessionControl, SessionControlError};
use layerx_agentd::store::{Store, TenantId};
use layerx_agentd::tenant::{AuthorizationError, Operation, Surface};
use layerx_mcp::server::{
    catalogue, InvocationOutcome, Server, ServerError, ToolKind, FAUCET_REQUEST,
    REQUIRED_DAEMON_GATES,
};
use layerx_types::ids::Did;
use layerx_types::verify::VerificationLevel;

static NEXT_DIRECTORY: AtomicU64 = AtomicU64::new(1);

const CLAIM: &[u8] =
    br#"{"did":"did:layerx:model","public_key":"abababababababababababababababababababababababababababababababab"}"#;

struct BoundaryIdentity(CoreIdentity);

impl IdentityResolver for BoundaryIdentity {
    fn resolve(&mut self, _did: &Did) -> Result<Option<CoreIdentity>, IdentityError> {
        Ok(Some(self.0.clone()))
    }
}

fn directory(label: &str) -> std::path::PathBuf {
    let sequence = NEXT_DIRECTORY.fetch_add(1, Ordering::Relaxed);
    std::env::temp_dir().join(format!(
        "layerx-mcp-faucet-{label}-{}-{sequence}",
        std::process::id()
    ))
}

fn records(scopes: &[&str]) -> (SessionRecord, Capability) {
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

fn bind(root: &Path, scopes: &[&str]) -> Server {
    let (session, capability) = records(scopes);
    let (control, credential) = control(root, &session, &capability);
    Server::bind(control, credential, capability.id, 50, root)
        .unwrap_or_else(|error| panic!("bind: {error:?}"))
}

#[test]
fn the_faucet_tool_is_served_through_the_daemon_path_under_its_own_scope() {
    let root = directory("served");
    let mut server = bind(&root, &[FAUCET_REQUEST.required_scope]);
    assert_eq!(server.tools(), [FAUCET_REQUEST]);
    let tool = server
        .tool("faucet.request")
        .unwrap_or_else(|| panic!("faucet tool absent"));
    assert_eq!(tool, FAUCET_REQUEST);
    assert_eq!(tool.kind, ToolKind::Write);
    assert_eq!(tool.required_scope, "write:faucet:claim");
    assert_ne!(tool.mutation, "none");
    assert!(catalogue().contains(&FAUCET_REQUEST));
    let declaration = server.capability_declaration();
    assert_eq!(declaration.write_tools, 1);
    assert!(declaration.mutations_reachable);

    let observed = Cell::new(0_u32);
    let claimed = server
        .execute_committed(50, "faucet.request", CLAIM.to_vec(), |invocation| {
            observed.set(observed.get() + 1);
            assert_eq!(invocation.tool(), FAUCET_REQUEST);
            assert_eq!(invocation.arguments(), CLAIM);
            assert_eq!(invocation.gates(), REQUIRED_DAEMON_GATES);
            ("funded", InvocationOutcome::Completed)
        })
        .unwrap_or_else(|error| panic!("faucet claim: {error:?}"));
    assert_eq!(claimed, "funded");
    assert_eq!(observed.get(), 1);
    assert_eq!(server.audit_entries(), 2);
    let _ = fs::remove_dir_all(root);
}

#[test]
fn a_refused_faucet_claim_stays_a_refusal_in_the_daemon_audit() {
    let root = directory("refused");
    let mut server = bind(&root, &[FAUCET_REQUEST.required_scope]);
    let refused = server
        .execute_committed(50, "faucet.request", CLAIM.to_vec(), |_| {
            ("signer_not_bound", InvocationOutcome::Refused)
        })
        .unwrap_or_else(|error| panic!("faucet claim: {error:?}"));
    assert_eq!(refused, "signer_not_bound");
    assert_eq!(server.audit_entries(), 2);
    let _ = fs::remove_dir_all(root);
}

#[test]
fn the_faucet_scope_reaches_no_other_catalogue_tool() {
    let root = directory("isolated");
    let mut server = bind(&root, &[FAUCET_REQUEST.required_scope]);
    for tool in catalogue() {
        if tool.name == FAUCET_REQUEST.name {
            continue;
        }
        assert!(server.tool(tool.name).is_none(), "{} is served", tool.name);
        let refused = match tool.kind {
            ToolKind::Read => server.execute_read(50, tool.name, CLAIM.to_vec(), |_| {
                ((), InvocationOutcome::Completed)
            }),
            ToolKind::Write => server.execute_committed(50, tool.name, CLAIM.to_vec(), |_| {
                ((), InvocationOutcome::Completed)
            }),
        };
        assert!(
            matches!(refused, Err(ServerError::ToolAbsent)),
            "{} must not run under the faucet scope",
            tool.name
        );
    }
    let _ = fs::remove_dir_all(root);
}

#[test]
fn a_session_without_the_faucet_scope_is_never_offered_the_faucet_tool() {
    let root = directory("unscoped");
    let mut server = bind(&root, &["read:balance", "write:submit"]);
    assert!(server.tool("faucet.request").is_none());
    assert!(!server
        .tools()
        .iter()
        .any(|tool| tool.name == FAUCET_REQUEST.name));
    let ran = Cell::new(false);
    let refused = server.execute_committed(50, "faucet.request", CLAIM.to_vec(), |_| {
        ran.set(true);
        ((), InvocationOutcome::Completed)
    });
    assert!(matches!(refused, Err(ServerError::ToolAbsent)));
    assert!(!ran.get());
    let _ = fs::remove_dir_all(root);
}

#[test]
fn the_daemon_refuses_a_faucet_claim_the_session_no_longer_authorizes() {
    let root = directory("expired");
    let mut server = bind(&root, &[FAUCET_REQUEST.required_scope]);
    let ran = Cell::new(false);
    let refused = server.execute_committed(151, "faucet.request", CLAIM.to_vec(), |_| {
        ran.set(true);
        ((), InvocationOutcome::Completed)
    });
    assert!(
        matches!(refused, Err(ServerError::ExpiredAuthority)),
        "{refused:?}"
    );
    assert!(!ran.get());
    assert_eq!(server.audit_entries(), 1);
    let _ = fs::remove_dir_all(root);
}

#[test]
fn the_faucet_scope_authorizes_only_the_faucet_operation_at_the_agentd_boundary() {
    let root = directory("boundary");
    let (session, capability) = records(&[FAUCET_REQUEST.required_scope]);
    let (control, credential) = control(&root, &session, &capability);
    let permit = control
        .authorize(&credential, Operation::FaucetClaim, Surface::Mcp, 50, None)
        .unwrap_or_else(|error| panic!("faucet authorization: {error:?}"));
    assert_eq!(permit.credential(), credential);
    assert_eq!(permit.principal().surface, Surface::Mcp);
    for operation in [
        Operation::Submit,
        Operation::Sign,
        Operation::Prepare,
        Operation::Track,
        Operation::Wait,
        Operation::BudgetFund,
        Operation::ReadBalance,
    ] {
        let refused = control.authorize(&credential, operation, Surface::Mcp, 50, None);
        assert!(
            matches!(
                refused,
                Err(SessionControlError::Authorization(
                    AuthorizationError::ScopeDenied
                ))
            ),
            "{} was authorized by the faucet scope",
            operation.name()
        );
    }
    let _ = fs::remove_dir_all(root);
}
