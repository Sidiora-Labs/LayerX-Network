#[allow(dead_code)]
mod support;

use std::collections::{BTreeMap, BTreeSet};
use std::fs;

use ed25519_dalek::{Signer as _, SigningKey, Verifier as _};
use layerx_agentd::budget::{
    create_protocol_budget, reserve, BudgetCreationError, BudgetKind, BudgetLimiter,
    BudgetPipeline, BudgetRequest, CoreBudgetReceipt, LimitConfig, LimitId, LimitRefusal,
    LimitScope, LocalLimit, ReservationRequest,
};
use layerx_agentd::identity::{
    register, CoreIdentity, IdentityError, IdentityResolver, ProtocolAuthority,
};
use layerx_agentd::session::{
    close, invalidate_on_revocation, open, InvalidationReason, OpenRequest, RevocationEvent,
    SessionId, SessionRegistry, Token,
};
use layerx_agentd::sign::ProvisionedSessionKey;
use layerx_agentd::store::{Store, TenantId};
use layerx_crypto::local::LocalSigner;
use layerx_crypto::session::IssuedSessionKey;
use layerx_crypto::signer::Signer as _;
use layerx_human_service::agents::{
    AgentControlAgent, AgentControlError, AgentControlProfile, AgentControlState, AgentControls,
    AppLimitEvidence, AppLimitRequest, CapabilityProvision, LimitBacking, LimitChangeRequest,
    LimitEnforcement, ProtocolAction, ProtocolEvidence, SessionControlAdapter, APP_LIMIT_COPY_KEY,
    APP_LIMIT_EXPLANATION, PAUSE_CONSEQUENCE, PAUSE_CONSEQUENCE_COPY_KEY, PROTOCOL_LIMIT_COPY_KEY,
};
use layerx_human_service::agents::{
    AgentCreationContract, AgentEvidence, AgentFailure, SessionProvision,
};
use layerx_human_service::custody::{
    AgentContractError, AgentSessionContract, AgentSessionProvision, ProtocolIdentitySnapshot,
    ProvisionEvidence, RevocationEvidence, RevocationReason, RotationEvidence, RotationObservation,
    RotationSubmission, SessionEntropySource, SessionKeyEntropy, SessionKeyError,
    SessionKeyProvisioner, SessionPolicy, SessionTarget, SuspensionEvidence,
};
use layerx_human_service::store::PrincipalId;
use layerx_intents::{DisclosureCheck, IntentKind};
use layerx_proof::receipt::{verify, AuthorizedBatch};
use layerx_types::account::AccountId;
use layerx_types::ids::{AssetId, Did};
use layerx_types::intent::PurposeHash;
use layerx_types::payload::{ActivityType, ModuleId, ModuleRegistration, ModuleRegistry};
use layerx_types::verify::VerificationLevel;
use sha2::{Digest as _, Sha256};

use support::directory;

const ASSET: [u8; 32] = [0x22; 32];
const AGENT_ID: [u8; 32] = [0x31; 32];

struct IdentityBoundary(CoreIdentity);

impl IdentityResolver for IdentityBoundary {
    fn resolve(&mut self, _did: &Did) -> Result<Option<CoreIdentity>, IdentityError> {
        Ok(Some(self.0.clone()))
    }
}

struct InstalledSession {
    _issued: IssuedSessionKey,
    _signer: ProvisionedSessionKey,
    token: Token,
}

struct ReceiptPipeline {
    receipt: CoreBudgetReceipt,
}

impl BudgetPipeline for ReceiptPipeline {
    fn submit_budget(
        &mut self,
        _request: &BudgetRequest,
    ) -> Result<CoreBudgetReceipt, BudgetCreationError> {
        Ok(self.receipt.clone())
    }
}

/// In-process agent contract over the actual agentd identity, session,
/// revocation, protocol-budget, local-limiter, and durable store code paths.
struct RealAgentLayer {
    root: std::path::PathBuf,
    store: Store,
    sessions: SessionRegistry,
    tenant: TenantId,
    principal: PrincipalId,
    did: Did,
    primary_public_key: [u8; 32],
    protocol_identity: [u8; 32],
    revocation_sequence: u64,
    now: u64,
    core_sequence: u64,
    installed: BTreeMap<[u8; 32], InstalledSession>,
    protocol_cache: BTreeMap<[u8; 32], ProtocolEvidence>,
    protocol_limit_effects: usize,
    app_limit_effects: usize,
    app_limiter: Option<(BudgetLimiter, LimitId)>,
    local_limit: Option<LocalLimit>,
}

impl RealAgentLayer {
    fn new(
        root: std::path::PathBuf,
        principal: PrincipalId,
        did: Did,
        primary_public_key: [u8; 32],
    ) -> Self {
        fs::create_dir_all(&root).unwrap_or_else(|error| panic!("agent root: {error}"));
        Self {
            store: Store::open(&root).unwrap_or_else(|error| panic!("agent store: {error}")),
            sessions: SessionRegistry::default(),
            tenant: TenantId::new("tenant-controls")
                .unwrap_or_else(|error| panic!("tenant: {error}")),
            principal,
            protocol_identity: Sha256::digest(did.as_bytes()).into(),
            did,
            primary_public_key,
            revocation_sequence: 1,
            now: 1_000,
            core_sequence: 10,
            installed: BTreeMap::new(),
            protocol_cache: BTreeMap::new(),
            protocol_limit_effects: 0,
            app_limit_effects: 0,
            app_limiter: None,
            local_limit: None,
            root,
        }
    }

    fn require_scope(&self, principal: &PrincipalId, did: &Did) -> Result<(), AgentContractError> {
        if principal == &self.principal && did == &self.did {
            Ok(())
        } else {
            Err(AgentContractError::Refused("wrong principal"))
        }
    }

    fn daemon_open(&self, grant_id: [u8; 32]) -> bool {
        self.sessions
            .get(SessionId(grant_id))
            .is_some_and(|record| record.open)
    }

    fn authorizes(&self, grant_id: [u8; 32], scope: &str) -> bool {
        self.installed.get(&grant_id).is_some_and(|installed| {
            installed
                .token
                .authorize(&self.tenant, &self.did, scope, self.core_sequence)
                .is_ok()
        })
    }

    fn app_refuses(&self, amount: u128) -> bool {
        let Some((limiter, id)) = &self.app_limiter else {
            return false;
        };
        matches!(
            reserve(
                limiter,
                &ReservationRequest {
                    id: [0x91; 32],
                    amount,
                    expiry_sequence: self.core_sequence.saturating_add(10),
                    current_sequence: self.core_sequence,
                    applicable_limits: vec![*id],
                },
            ),
            Err(LimitRefusal::Exceeded { .. })
        )
    }
}

impl Drop for RealAgentLayer {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.root);
    }
}

impl AgentSessionContract for RealAgentLayer {
    fn identity_snapshot(
        &mut self,
        principal: &PrincipalId,
        did: &Did,
    ) -> Result<ProtocolIdentitySnapshot, AgentContractError> {
        self.require_scope(principal, did)?;
        Ok(ProtocolIdentitySnapshot {
            protocol_identity: self.protocol_identity,
            primary_public_key: self.primary_public_key,
            revocation_sequence: self.revocation_sequence,
            protocol_time: self.now,
            core_sequence: self.core_sequence,
            verification_level: VerificationLevel::STATE_PROVEN,
        })
    }

    fn provision_session(
        &mut self,
        request: AgentSessionProvision,
    ) -> Result<ProvisionEvidence, AgentContractError> {
        let (principal, did, issued, scopes, policy_version, secret) = request.into_parts();
        self.require_scope(&principal, &did)?;
        let grant_id = issued.grant_id;
        let session_public_key = issued.session_public_key;
        let signer = ProvisionedSessionKey::new(secret.into_seed(), issued.clone())
            .map_err(|_| AgentContractError::Refused("session key mismatch"))?;
        let authorities = self
            .installed
            .keys()
            .copied()
            .map(ProtocolAuthority::SessionKey)
            .chain(std::iter::once(ProtocolAuthority::SessionKey(grant_id)))
            .collect();
        let mut resolver = IdentityBoundary(CoreIdentity {
            canonical_bytes: self.protocol_identity.to_vec(),
            head_sequence: self.core_sequence,
            verification_level: VerificationLevel::STATE_PROVEN,
            frozen: false,
            authorities,
        });
        let identity = register(
            &mut self.store,
            self.tenant.clone(),
            self.did.clone(),
            &mut resolver,
        )
        .map_err(|_| AgentContractError::Refused("identity registration failed"))?;
        let token = open(
            &mut self.store,
            &mut self.sessions,
            &identity,
            OpenRequest {
                session_id: SessionId(grant_id),
                token_id: Sha256::digest(grant_id).into(),
                tenant: self.tenant.clone(),
                agent: self.did.clone(),
                authority: ProtocolAuthority::SessionKey(grant_id),
                permitted_activity_types: issued
                    .permitted_activity_types
                    .iter()
                    .map(|activity| activity.ordinal())
                    .collect::<BTreeSet<_>>(),
                scopes: scopes.into_iter().collect(),
                expiry_sequence: issued.expires_at,
                opening_client: "layerx-human-service".to_owned(),
                policy_version,
            },
            self.core_sequence,
        )
        .map_err(|_| AgentContractError::Refused("daemon session open failed"))?;
        self.installed.insert(
            grant_id,
            InstalledSession {
                _issued: issued,
                _signer: signer,
                token,
            },
        );
        let mut evidence = ProvisionEvidence {
            grant_id,
            session_public_key,
            daemon_session_id: grant_id,
            protocol_sequence: self.core_sequence,
            observed_at: self.now,
            verification_level: VerificationLevel::BATCH_INCLUDED,
            receipt_digest: [0; 32],
        };
        evidence.receipt_digest = evidence.expected_digest();
        Ok(evidence)
    }

    fn suspend_permissions(
        &mut self,
        target: &SessionTarget,
        reason: RevocationReason,
        requested_at: u64,
    ) -> Result<SuspensionEvidence, AgentContractError> {
        self.require_scope(&target.principal, &target.agent_did)?;
        close(
            &mut self.store,
            &mut self.sessions,
            SessionId(target.daemon_session_id),
        )
        .map_err(|_| AgentContractError::Refused("daemon close failed"))?;
        self.now = self.now.max(requested_at);
        let mut evidence = SuspensionEvidence {
            grant_id: target.grant_id,
            daemon_session_id: target.daemon_session_id,
            reason,
            observed_at: self.now,
            receipt_digest: [0; 32],
        };
        evidence.receipt_digest = evidence.expected_digest();
        Ok(evidence)
    }

    fn revoke_protocol_session(
        &mut self,
        target: &SessionTarget,
        reason: RevocationReason,
        requested_at: u64,
    ) -> Result<RevocationEvidence, AgentContractError> {
        self.require_scope(&target.principal, &target.agent_did)?;
        self.core_sequence = self.core_sequence.saturating_add(1);
        self.revocation_sequence = self.revocation_sequence.saturating_add(1);
        invalidate_on_revocation(
            &mut self.store,
            &mut self.sessions,
            &mut [],
            &RevocationEvent {
                did: self.did.clone(),
                authority: Some(ProtocolAuthority::SessionKey(target.grant_id)),
                reason: InvalidationReason::SessionKeyRevoked,
                observed_sequence: self.core_sequence,
            },
        )
        .map_err(|_| AgentContractError::Refused("protocol invalidation failed"))?;
        self.now = self.now.max(requested_at).saturating_add(1);
        let mut evidence = RevocationEvidence {
            grant_id: target.grant_id,
            reason,
            observed_sequence: self.core_sequence,
            observed_at: self.now,
            verification_level: VerificationLevel::BATCH_INCLUDED,
            receipt_digest: [0; 32],
        };
        evidence.receipt_digest = evidence.expected_digest();
        Ok(evidence)
    }

    fn announce_rotation(
        &mut self,
        _submission: RotationSubmission,
    ) -> Result<RotationEvidence, AgentContractError> {
        Err(AgentContractError::Refused(
            "rotation is outside this contract",
        ))
    }

    fn rotation_observation(
        &mut self,
        _principal: &PrincipalId,
        _did: &Did,
    ) -> Result<RotationObservation, AgentContractError> {
        Err(AgentContractError::Refused(
            "rotation is outside this contract",
        ))
    }
}

impl AgentCreationContract for RealAgentLayer {
    fn submit_protocol(
        &mut self,
        action: ProtocolAction,
    ) -> Result<ProtocolEvidence, AgentFailure> {
        if let Some(existing) = self.protocol_cache.get(&action.action_key) {
            return Ok(existing.clone());
        }
        if !matches!(action.intent.kind(), IntentKind::BudgetCreate(_))
            || DisclosureCheck::verify(&action.intent, &action.compiled)
                != Ok(action.disclosure.clone())
            || action.compiled.payload().as_bytes() != action.disclosure.canonical_payload()
        {
            return Err(AgentFailure::Refused("typed limit intent mismatch"));
        }
        let receipt = protocol_receipt(action.action_key);
        verify(&receipt.receipt_bytes, &receipt.authorized_batch)
            .map_err(|_| AgentFailure::Refused("receipt verification failed"))?;
        let activity_digest = Sha256::digest(action.compiled.payload().as_bytes());
        let activity_signer = SigningKey::from_bytes(&[0x72; 32]);
        let activity_signature = activity_signer.sign(&activity_digest);
        activity_signer
            .verifying_key()
            .verify(&activity_digest, &activity_signature)
            .map_err(|_| AgentFailure::Refused("activity signature failed"))?;
        let mut pipeline = ReceiptPipeline {
            receipt: CoreBudgetReceipt {
                object_id: action.action_key,
                evidence: support::raw_receipt_evidence(
                    receipt.receipt_bytes.clone(),
                    receipt.authorized_batch,
                    11,
                    &SigningKey::from_bytes(&[0x84; 32]),
                ),
            },
        };
        let receipt_signer = SigningKey::from_bytes(&[0x84; 32]);
        create_protocol_budget(
            &mut self.store,
            &BudgetRequest {
                tenant: self.tenant.clone(),
                request_id: action.action_key,
                kind: BudgetKind::ProtocolBudget,
                asset: ASSET,
                ceiling: decoded_limit(action.intent.kind())?,
                expiry_sequence: 50_000,
                canonical_activity: action.compiled.payload().as_bytes().to_vec(),
                signature: activity_signature.to_bytes(),
            },
            &support::evidence_verifier(&receipt_signer),
            &mut pipeline,
        )
        .map_err(|_| AgentFailure::Refused("protocol budget change failed"))?;
        self.protocol_limit_effects = self.protocol_limit_effects.saturating_add(1);
        self.protocol_cache
            .insert(action.action_key, receipt.clone());
        Ok(receipt)
    }

    fn provision_session(
        &mut self,
        _request: SessionProvision,
    ) -> Result<AgentEvidence, AgentFailure> {
        Err(AgentFailure::Refused(
            "creation session path is not a control",
        ))
    }

    fn narrow_capability(
        &mut self,
        _request: CapabilityProvision,
    ) -> Result<AgentEvidence, AgentFailure> {
        Err(AgentFailure::Refused(
            "capability creation is not a control",
        ))
    }
}

impl AgentControlAgent for RealAgentLayer {
    fn apply_app_limit(
        &mut self,
        request: AppLimitRequest,
    ) -> Result<AppLimitEvidence, AgentFailure> {
        let mut id_bytes = [0_u8; 16];
        id_bytes.copy_from_slice(&request.action_key[..16]);
        let id = LimitId(id_bytes);
        let local = LocalLimit::new(
            self.tenant.clone(),
            request.action_key,
            request.asset,
            request.ceiling,
        );
        let limiter = BudgetLimiter::new(vec![LimitConfig {
            id,
            name: "managed agent monthly app limit".to_owned(),
            scope: LimitScope::Agent(request.agent_id),
            ceiling: request.ceiling,
            consumed: 0,
        }])
        .map_err(|_| AgentFailure::Refused("app limit configuration failed"))?;
        self.local_limit = Some(local);
        self.app_limiter = Some((limiter, id));
        self.app_limit_effects = self.app_limit_effects.saturating_add(1);
        let mut evidence = AppLimitEvidence {
            action_key: request.action_key,
            agent_id: request.agent_id,
            asset: request.asset,
            ceiling: request.ceiling,
            observed_sequence: self.core_sequence,
            configuration_digest: [0; 32],
        };
        evidence.configuration_digest = evidence.expected_digest();
        Ok(evidence)
    }
}

fn decoded_limit(intent: &IntentKind) -> Result<u128, AgentFailure> {
    let IntentKind::BudgetCreate(value) = intent else {
        return Err(AgentFailure::Refused("limit intent missing"));
    };
    Ok(value.per_period_limit().value())
}

struct RealEntropy;

impl SessionEntropySource for RealEntropy {
    fn next_session_entropy(&mut self) -> Result<SessionKeyEntropy, SessionKeyError> {
        let mut seed = [0_u8; 32];
        getrandom::fill(&mut seed).map_err(|_| SessionKeyError::EntropyUnavailable)?;
        SessionKeyEntropy::new(seed)
    }
}

fn principal() -> PrincipalId {
    PrincipalId::new("alice").unwrap_or_else(|error| panic!("principal: {error}"))
}

fn did() -> Did {
    Did::new(b"did:layerx:controlled").unwrap_or_else(|error| panic!("controlled DID: {error:?}"))
}

fn activity(module: ModuleId, ordinal: u16) -> ActivityType {
    ActivityType::new(module, ordinal).unwrap_or_else(|error| panic!("activity: {error:?}"))
}

fn registry() -> ModuleRegistry {
    let asset = ModuleRegistration::new(ModuleId::Asset, &[activity(ModuleId::Asset, 5)])
        .unwrap_or_else(|error| panic!("asset registry: {error:?}"));
    let budget = ModuleRegistration::new(ModuleId::Budget, &[activity(ModuleId::Budget, 1)])
        .unwrap_or_else(|error| panic!("budget registry: {error:?}"));
    ModuleRegistry::new(&[asset, budget])
        .unwrap_or_else(|error| panic!("module registry: {error:?}"))
}

fn session_policy() -> SessionPolicy {
    SessionPolicy::new(
        1_000,
        100,
        3,
        vec!["prepare".to_owned(), "submit".to_owned()],
        "managed-agent-controls-v1",
    )
    .unwrap_or_else(|error| panic!("session policy: {error}"))
}

fn profile() -> AgentControlProfile {
    AgentControlProfile {
        principal: principal(),
        agent_id: AGENT_ID,
        did: did(),
        owner: AccountId::parse("agent:did:layerx:alice:main")
            .unwrap_or_else(|error| panic!("owner: {error:?}")),
        budget_account: AccountId::parse("agent:did:layerx:controlled:budget:31313131")
            .unwrap_or_else(|error| panic!("budget account: {error:?}")),
        asset: AssetId::new(ASSET),
        purpose: PurposeHash::new([0x51; 32]),
        period_seconds: 2_592_000,
        budget_lifetime_seconds: 31_536_000,
        freshness_seconds: 5,
        initial_limit_evidence_digest: [0x52; 32],
    }
}

type Controls = AgentControls<SessionControlAdapter<RealAgentLayer, RealEntropy>>;

fn controls(label: &str, backing: LimitBacking) -> (Controls, [u8; 32]) {
    let root = directory(label);
    let primary_public = LocalSigner::new([0x11; 32]).public_key();
    let layer = RealAgentLayer::new(root, principal(), did(), primary_public);
    let mut sessions = SessionKeyProvisioner::new(layer, RealEntropy, session_policy(), registry());
    let lease = sessions
        .provision(&principal(), &did(), vec![activity(ModuleId::Asset, 5)])
        .unwrap_or_else(|error| panic!("initial session: {error}"));
    let controls = AgentControls::new(
        SessionControlAdapter::new(sessions),
        profile(),
        1_000,
        backing,
        1_000,
    )
    .unwrap_or_else(|error| panic!("controls: {error}"));
    (controls, lease.grant_id)
}

#[test]
fn pause_revokes_real_authority_promptly_and_resume_is_receipt_gated() {
    let (mut controls, first_grant) = controls(
        "agent-controls-authority",
        LimitBacking::Protocol {
            active_budget_id: AGENT_ID,
        },
    );
    assert!(controls
        .boundary()
        .sessions()
        .contract()
        .authorizes(first_grant, "prepare"));

    let paused = controls
        .pause(1_005)
        .unwrap_or_else(|error| panic!("pause: {error}"));
    assert_eq!(paused.state, AgentControlState::Paused);
    assert_eq!(
        paused.pause_consequence_copy_key,
        Some(PAUSE_CONSEQUENCE_COPY_KEY)
    );
    assert_eq!(paused.pause_consequence, Some(PAUSE_CONSEQUENCE));
    let outcome_lease = controls
        .boundary()
        .sessions()
        .session(&principal(), &did())
        .unwrap_or_else(|| panic!("paused lease missing"));
    let latency = match outcome_lease.state {
        layerx_human_service::custody::SessionLeaseState::Revoked {
            suspended_at,
            revoked_at,
            ..
        } => revoked_at.saturating_sub(suspended_at),
        _ => panic!("pause did not revoke the session"),
    };
    assert!(latency <= session_policy().maximum_revocation_latency_seconds());
    assert!(!controls
        .boundary()
        .sessions()
        .contract()
        .daemon_open(first_grant));
    let resumed = controls
        .resume(1_007)
        .unwrap_or_else(|error| panic!("resume: {error}"));
    assert_eq!(resumed.state, AgentControlState::Active);
    assert!(resumed.pause_consequence.is_none());
    let replacement = controls
        .boundary()
        .sessions()
        .session(&principal(), &did())
        .unwrap_or_else(|| panic!("replacement lease missing"));
    assert_ne!(replacement.grant_id, first_grant);
    assert!(controls
        .boundary()
        .sessions()
        .contract()
        .authorizes(replacement.grant_id, "submit"));
}

#[test]
fn protocol_and_app_limit_changes_keep_their_real_enforcement_labels() {
    let (mut protocol, _) = controls(
        "agent-controls-protocol-limit",
        LimitBacking::Protocol {
            active_budget_id: AGENT_ID,
        },
    );
    let request = LimitChangeRequest::new([0x61; 32], 750)
        .unwrap_or_else(|error| panic!("protocol request: {error}"));
    let changed = protocol
        .change_limit(&registry(), request, 1_002)
        .unwrap_or_else(|error| panic!("protocol limit: {error}"));
    assert_eq!(changed.limit.monthly, 750);
    assert_eq!(changed.limit.enforcement, LimitEnforcement::Protocol);
    assert_eq!(changed.limit.enforcement_copy_key, PROTOCOL_LIMIT_COPY_KEY);
    assert!(changed.limit.verification_level >= VerificationLevel::SEQUENCER_SIGNED);
    assert_eq!(
        protocol
            .boundary()
            .sessions()
            .contract()
            .protocol_limit_effects,
        1
    );
    let repeated = protocol
        .change_limit(&registry(), request, 1_003)
        .unwrap_or_else(|error| panic!("repeat protocol limit: {error}"));
    assert_eq!(repeated, changed);
    assert_eq!(
        protocol
            .boundary()
            .sessions()
            .contract()
            .protocol_limit_effects,
        1
    );

    let (mut app, _) = controls("agent-controls-app-limit", LimitBacking::App);
    let changed = app
        .change_limit(
            &registry(),
            LimitChangeRequest::new([0x71; 32], 100)
                .unwrap_or_else(|error| panic!("app request: {error}")),
            1_002,
        )
        .unwrap_or_else(|error| panic!("app limit: {error}"));
    assert_eq!(changed.limit.enforcement, LimitEnforcement::App);
    assert_eq!(changed.limit.enforcement_copy_key, APP_LIMIT_COPY_KEY);
    assert_eq!(changed.limit.explanation, APP_LIMIT_EXPLANATION);
    assert!(!changed.limit.explanation.contains("protocol-enforced"));
    assert!(changed
        .limit
        .explanation
        .contains("not a protocol guarantee"));
    let layer = app.boundary().sessions().contract();
    assert_eq!(layer.app_limit_effects, 1);
    assert!(layer.app_refuses(101));
    let local = layer
        .local_limit
        .as_ref()
        .unwrap_or_else(|| panic!("local limit missing"));
    assert_eq!(local.enforcement, "daemon-enforced");
    assert!(local
        .bypass_statement
        .contains("bypassing layerx-agentd bypasses"));

    assert!(matches!(
        app.view(1_008),
        Err(AgentControlError::Stale { .. })
    ));
}

#[derive(Clone)]
struct ReceiptFields {
    activity_id: [u8; 32],
    previous_state_root: [u8; 32],
    resulting_state_root: [u8; 32],
    batch_id: [u8; 32],
}

fn protocol_receipt(action_key: [u8; 32]) -> ProtocolEvidence {
    let previous_state_root = [0x81; 32];
    let fields = ReceiptFields {
        activity_id: action_key,
        previous_state_root,
        resulting_state_root: [0x82; 32],
        batch_id: support::execution_batch_id(previous_state_root, action_key, 11),
    };
    let signer = SigningKey::from_bytes(&[0x84; 32]);
    let unsigned = encode_receipt(&fields, None);
    let mut hasher = Sha256::new();
    hasher.update(b"LXP/v1/receipt\0");
    hasher.update(&unsigned);
    let signature = signer.sign(&<[u8; 32]>::from(hasher.finalize()));
    let receipt_bytes = encode_receipt(&fields, Some(signature.to_bytes()));
    ProtocolEvidence {
        action_key,
        activity_id: action_key,
        receipt_bytes,
        authorized_batch: AuthorizedBatch::new(
            fields.batch_id,
            ASSET,
            fields.previous_state_root,
            fields.resulting_state_root,
            signer.verifying_key().to_bytes(),
        ),
    }
}

fn encode_receipt(fields: &ReceiptFields, signature: Option<[u8; 64]>) -> Vec<u8> {
    let mut bytes = Vec::new();
    push_u16(&mut bytes, 1);
    push_u16(&mut bytes, 0x5201);
    push_u16(&mut bytes, 1);
    push_bytes(&mut bytes, &fields.activity_id);
    push_u64(&mut bytes, 11);
    push_bytes(&mut bytes, &fields.previous_state_root);
    push_bytes(&mut bytes, &fields.resulting_state_root);
    push_bytes(&mut bytes, &[0x85; 32]);
    bytes.extend_from_slice(&0_i32.to_be_bytes());
    bytes.extend_from_slice(&0_u32.to_be_bytes());
    bytes.extend_from_slice(&1_u128.to_be_bytes());
    push_bytes(&mut bytes, &fields.batch_id);
    push_u16(&mut bytes, ModuleId::Budget as u16);
    bytes.extend_from_slice(&1_u32.to_be_bytes());
    bytes.extend_from_slice(&1_u32.to_be_bytes());
    bytes.push(1);
    push_bytes(&mut bytes, &ASSET);
    bytes.extend_from_slice(&1_u128.to_be_bytes());
    push_bytes(&mut bytes, &[0x86; 32]);
    bytes.extend_from_slice(&10_u128.to_be_bytes());
    bytes.extend_from_slice(&9_u128.to_be_bytes());
    push_u64(&mut bytes, 1);
    push_bytes(&mut bytes, &[0x87; 32]);
    bytes.extend_from_slice(&20_u128.to_be_bytes());
    bytes.extend_from_slice(&21_u128.to_be_bytes());
    push_bytes(&mut bytes, &[0x88; 32]);
    push_bytes(&mut bytes, &[0x89; 32]);
    push_bytes(&mut bytes, &[0x8a; 32]);
    push_u64(&mut bytes, 1_002);
    bytes.push(u8::from(signature.is_some()));
    if let Some(signature) = signature {
        push_bytes(&mut bytes, &signature);
    }
    bytes
}

fn push_u16(bytes: &mut Vec<u8>, value: u16) {
    bytes.extend_from_slice(&value.to_be_bytes());
}

fn push_u64(bytes: &mut Vec<u8>, value: u64) {
    bytes.extend_from_slice(&value.to_be_bytes());
}

fn push_bytes(bytes: &mut Vec<u8>, value: &[u8]) {
    let length = u32::try_from(value.len()).unwrap_or_else(|_| panic!("field overflow"));
    bytes.extend_from_slice(&length.to_be_bytes());
    bytes.extend_from_slice(value);
}
