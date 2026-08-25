#[allow(dead_code)]
mod support;

use std::collections::{BTreeMap, BTreeSet};
use std::fs;

use ed25519_dalek::{Signer as _, SigningKey};
use layerx_agentd::budget::{
    create_protocol_budget, BudgetCreationError, BudgetKind, BudgetPipeline, BudgetRequest,
    CoreBudgetReceipt,
};
use layerx_agentd::capability::{
    assert_narrowing, Capability, CapabilityDimensions, CapabilityId, Dimension, ProtocolScope,
    RateCeiling,
};
use layerx_agentd::identity::{
    register, CoreIdentity, IdentityError, IdentityResolver, ProtocolAuthority,
};
use layerx_agentd::session::{open, OpenRequest, SessionId, SessionRegistry};
use layerx_agentd::sign::ProvisionedSessionKey;
use layerx_agentd::store::{ObjectKind, Store, TenantId, TenantKey};
use layerx_crypto::local::LocalSigner;
use layerx_crypto::session::{issue_session_key, SessionKeyRequest};
use layerx_crypto::signer::Signer as _;
use layerx_human_service::agents::{
    AgentCreationContract, AgentEvidence, AgentFailure, CapabilityProvision, CreateAgentRequest,
    CreationContext, CreationJourney, CreationStage, CreationState, ProtocolAction,
    ProtocolEvidence, PurposePresetCatalog, SessionProvision, StageState,
};
use layerx_human_service::custody::{EnvelopeKms, Keystore};
use layerx_human_service::store::{PrincipalStore, TenancyDigest};
use layerx_intents::{DisclosureCheck, IntentKind};
use layerx_proof::receipt::{verify, AuthorizedBatch};
use layerx_types::payload::{ActivityType, ModuleId, ModuleRegistration, ModuleRegistry};
use layerx_types::verify::VerificationLevel;
use serde_json::json;
use sha2::{Digest as _, Sha256};

use support::{directory, install_and_open, principal, retention_uniform, tenancy};

const NETWORK_ID: u32 = 77;
const CORE_SEQUENCE: u64 = 10;

struct Fixture {
    root: std::path::PathBuf,
    store: PrincipalStore,
    digest: TenancyDigest,
    keystore: Keystore,
    registry: ModuleRegistry,
}

impl Fixture {
    fn new(label: &str) -> Self {
        let root = directory(label);
        fs::create_dir_all(&root).unwrap_or_else(|error| panic!("fixture root: {error}"));
        let map = tenancy(&[("alice", "tenant-alice")]);
        let (store, digest) =
            install_and_open(&root.join("human-store"), &map, retention_uniform(100));
        let kms_secret = root.join("kms-root");
        fs::write(&kms_secret, [0x42; 64]).unwrap_or_else(|error| panic!("KMS secret: {error}"));
        let kms = EnvelopeKms::new("kms://agent-create", kms_secret)
            .unwrap_or_else(|error| panic!("KMS: {error}"));
        let keystore = Keystore::open_development(root.join("custody"), NETWORK_ID, kms)
            .unwrap_or_else(|error| panic!("keystore: {error}"));
        let governance = ModuleRegistration::new(
            ModuleId::Governance,
            &[
                activity(ModuleId::Governance, 1),
                activity(ModuleId::Governance, 3),
            ],
        )
        .unwrap_or_else(|error| panic!("governance registry: {error:?}"));
        let budget = ModuleRegistration::new(
            ModuleId::Budget,
            &[activity(ModuleId::Budget, 1), activity(ModuleId::Budget, 2)],
        )
        .unwrap_or_else(|error| panic!("budget registry: {error:?}"));
        let registry = ModuleRegistry::new(&[governance, budget])
            .unwrap_or_else(|error| panic!("module registry: {error:?}"));
        Self {
            root,
            store,
            digest,
            keystore,
            registry,
        }
    }

    fn reopen(&self) -> PrincipalStore {
        PrincipalStore::open(
            self.root.join("human-store"),
            retention_uniform(100),
            self.digest,
        )
        .unwrap_or_else(|error| panic!("reopen human store: {error}"))
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.root);
    }
}

fn activity(module: ModuleId, ordinal: u16) -> ActivityType {
    ActivityType::new(module, ordinal)
        .unwrap_or_else(|error| panic!("activity {module:?}/{ordinal}: {error:?}"))
}

fn catalog(purpose: &str, ceiling: u128) -> PurposePresetCatalog {
    let counterparty = [17_u8; 32];
    let asset = [34_u8; 32];
    let config = json!({
        "version": "operator-policy-2026-08",
        "presets": [{
            "id": purpose,
            "activity_types": [activity(ModuleId::Asset, 5).value()],
            "counterparties": [counterparty],
            "assets": [asset],
            "amount_ceiling": ceiling,
            "rate_maximum_uses": 12,
            "rate_window_sequences": 50,
            "purposes": [purpose],
            "expiry_sequence": 50_000,
            "session_scopes": ["prepare", "submit"],
            "session_lifetime_seconds": 5_000,
            "budget_asset": asset,
            "budget_period_seconds": 2_592_000,
            "budget_expiry_seconds": 31_536_000,
            "initial_funding": 250
        }]
    });
    PurposePresetCatalog::from_json(
        &serde_json::to_vec(&config).unwrap_or_else(|error| panic!("config JSON: {error}")),
    )
    .unwrap_or_else(|error| panic!("catalog: {error}"))
}

fn request(purpose: &str) -> CreateAgentRequest {
    CreateAgentRequest::new("Treasury helper", purpose, 1_000)
        .unwrap_or_else(|error| panic!("request: {error}"))
}

fn context(marker: u8) -> CreationContext {
    CreationContext {
        idempotency_key: [marker; 32],
        owner_account: "agent:did:layerx:alice:main".to_owned(),
        human_recovery_root: [0x51; 32],
        recovery_threshold: 1,
        network_id: NETWORK_ID,
        protocol_time: 1_000,
    }
}

struct IdentityBoundary(CoreIdentity);

impl IdentityResolver for IdentityBoundary {
    fn resolve(
        &mut self,
        _did: &layerx_types::ids::Did,
    ) -> Result<Option<CoreIdentity>, IdentityError> {
        Ok(Some(self.0.clone()))
    }
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

/// In-process implementation over the real agentd identity/session,
/// capability, budget and tenant-store code paths. Failure gates act at the
/// contract boundary before effects; releasing one resumes the real operation.
struct InProcessAgentLayer {
    store: Store,
    tenant: TenantId,
    sessions: SessionRegistry,
    fail_once: Option<CreationStage>,
    protocol_cache: BTreeMap<[u8; 32], ProtocolEvidence>,
    agent_cache: BTreeMap<[u8; 32], AgentEvidence>,
    effects: BTreeMap<CreationStage, usize>,
    last_capability: Option<CapabilityProvision>,
}

impl InProcessAgentLayer {
    fn new(root: &std::path::Path, fail_once: Option<CreationStage>) -> Self {
        Self {
            store: Store::open(root).unwrap_or_else(|error| panic!("agent store: {error}")),
            tenant: TenantId::new("tenant-alice").unwrap_or_else(|error| panic!("tenant: {error}")),
            sessions: SessionRegistry::default(),
            fail_once,
            protocol_cache: BTreeMap::new(),
            agent_cache: BTreeMap::new(),
            effects: BTreeMap::new(),
            last_capability: None,
        }
    }

    fn boundary(&mut self, stage: CreationStage) -> Result<(), AgentFailure> {
        if self.fail_once == Some(stage) {
            self.fail_once = None;
            Err(AgentFailure::Unavailable)
        } else {
            Ok(())
        }
    }

    fn effect(&mut self, stage: CreationStage) {
        *self.effects.entry(stage).or_default() += 1;
    }

    fn count(&self, stage: CreationStage) -> usize {
        self.effects.get(&stage).copied().unwrap_or_default()
    }

    fn evidence(stage: CreationStage, action_key: [u8; 32], object_id: [u8; 32]) -> AgentEvidence {
        let mut evidence = AgentEvidence {
            action_key,
            object_id,
            observed_sequence: CORE_SEQUENCE,
            verification_level: VerificationLevel::BATCH_INCLUDED,
            receipt_digest: [0; 32],
        };
        evidence.receipt_digest = evidence.expected_digest(stage);
        evidence
    }
}

impl AgentCreationContract for InProcessAgentLayer {
    fn submit_protocol(
        &mut self,
        action: ProtocolAction,
    ) -> Result<ProtocolEvidence, AgentFailure> {
        if let Some(existing) = self.protocol_cache.get(&action.action_key) {
            return Ok(existing.clone());
        }
        self.boundary(action.stage)?;
        if DisclosureCheck::verify(&action.intent, &action.compiled)
            != Ok(action.disclosure.clone())
            || action.compiled.payload().as_bytes() != action.disclosure.canonical_payload()
            || !intent_matches(action.stage, action.intent.kind())
        {
            return Err(AgentFailure::Refused("typed intent mismatch"));
        }
        let receipt = receipt(action.stage, action.action_key);
        verify(&receipt.receipt_bytes, &receipt.authorized_batch)
            .map_err(|_| AgentFailure::Refused("core receipt verification failed"))?;
        match action.stage {
            CreationStage::BudgetCreation => {
                let receipt_signing_seed: [u8; 32] = Sha256::digest(
                    [b"agent-create-receipt".as_slice(), &action.action_key].concat(),
                )
                .into();
                let receipt_signer = SigningKey::from_bytes(&receipt_signing_seed);
                let mut pipeline = ReceiptPipeline {
                    receipt: CoreBudgetReceipt {
                        object_id: action.action_key,
                        evidence: support::raw_receipt_evidence(
                            receipt.receipt_bytes.clone(),
                            receipt.authorized_batch,
                            CORE_SEQUENCE,
                            &receipt_signer,
                        ),
                    },
                };
                create_protocol_budget(
                    &mut self.store,
                    &BudgetRequest {
                        tenant: self.tenant.clone(),
                        request_id: action.action_key,
                        kind: BudgetKind::ProtocolBudget,
                        asset: [0x22; 32],
                        ceiling: 1_000,
                        expiry_sequence: 50_000,
                        canonical_activity: action.compiled.payload().as_bytes().to_vec(),
                        signature: [0x73; 64],
                    },
                    &support::evidence_verifier(&receipt_signer),
                    &mut pipeline,
                )
                .map_err(|_| AgentFailure::Refused("protocol budget creation failed"))?;
            }
            CreationStage::BudgetFunding => {
                let key = TenantKey::new(
                    self.tenant.clone(),
                    ObjectKind::Receipt,
                    action.action_key.to_vec(),
                )
                .map_err(|_| AgentFailure::Refused("fund receipt key failed"))?;
                self.store
                    .put_core_cache(key, receipt.receipt_bytes.clone())
                    .map_err(|_| AgentFailure::Refused("fund receipt persistence failed"))?;
            }
            CreationStage::DidRegistration | CreationStage::RecoveryRegistration => {}
            CreationStage::Custody
            | CreationStage::SessionProvision
            | CreationStage::CapabilityNarrowing => {
                return Err(AgentFailure::Refused("non-protocol stage"));
            }
        }
        let receipt_key = TenantKey::new(
            self.tenant.clone(),
            ObjectKind::Receipt,
            action.action_key.to_vec(),
        )
        .map_err(|_| AgentFailure::Refused("protocol receipt key failed"))?;
        self.store
            .put_core_cache(receipt_key, receipt.receipt_bytes.clone())
            .map_err(|_| AgentFailure::Refused("protocol receipt persistence failed"))?;
        self.effect(action.stage);
        self.protocol_cache
            .insert(action.action_key, receipt.clone());
        Ok(receipt)
    }

    fn provision_session(
        &mut self,
        request: SessionProvision,
    ) -> Result<AgentEvidence, AgentFailure> {
        if let Some(existing) = self.agent_cache.get(&request.action_key) {
            return Ok(existing.clone());
        }
        self.boundary(CreationStage::SessionProvision)?;
        let session_seed: [u8; 32] =
            Sha256::digest([b"layerx-agent-session/v1".as_slice(), &request.action_key].concat())
                .into();
        let session_public_key = LocalSigner::new(session_seed).public_key();
        let issued = issue_session_key(&SessionKeyRequest {
            grantor: Sha256::digest(request.did.as_bytes()).into(),
            session_public_key,
            not_before: 1_000,
            expires_at: Some(request.expires_at),
            permitted_activity_types: request.activity_types.clone(),
            revocation_sequence: Some(1),
        })
        .map_err(|_| AgentFailure::Refused("protocol session issue failed"))?;
        let _signer = ProvisionedSessionKey::new(session_seed, issued.clone())
            .map_err(|_| AgentFailure::Refused("session signer provisioning failed"))?;
        let authority = ProtocolAuthority::SessionKey(issued.grant_id);
        let mut resolver = IdentityBoundary(CoreIdentity {
            canonical_bytes: request.did.as_bytes().to_vec(),
            head_sequence: CORE_SEQUENCE,
            verification_level: VerificationLevel::STATE_PROVEN,
            frozen: false,
            authorities: vec![authority.clone()],
        });
        let identity = register(
            &mut self.store,
            self.tenant.clone(),
            request.did.clone(),
            &mut resolver,
        )
        .map_err(|_| AgentFailure::Refused("agent identity registration failed"))?;
        let token_id: [u8; 32] = Sha256::digest(request.action_key).into();
        let token = open(
            &mut self.store,
            &mut self.sessions,
            &identity,
            OpenRequest {
                session_id: SessionId(issued.grant_id),
                token_id,
                tenant: self.tenant.clone(),
                agent: request.did.clone(),
                authority,
                permitted_activity_types: request
                    .activity_types
                    .iter()
                    .map(|activity| activity.ordinal())
                    .collect(),
                scopes: request.daemon_scopes.iter().cloned().collect(),
                expiry_sequence: request.expires_at,
                opening_client: "layerx-human-service".to_owned(),
                policy_version: "operator-policy-2026-08".to_owned(),
            },
            CORE_SEQUENCE,
        )
        .map_err(|_| AgentFailure::Refused("agent session open failed"))?;
        token
            .authorize(&self.tenant, &request.did, "prepare", CORE_SEQUENCE)
            .map_err(|_| AgentFailure::Refused("session scope verification failed"))?;
        let evidence = Self::evidence(
            CreationStage::SessionProvision,
            request.action_key,
            issued.grant_id,
        );
        self.effect(CreationStage::SessionProvision);
        self.agent_cache
            .insert(request.action_key, evidence.clone());
        Ok(evidence)
    }

    fn narrow_capability(
        &mut self,
        request: CapabilityProvision,
    ) -> Result<AgentEvidence, AgentFailure> {
        if let Some(existing) = self.agent_cache.get(&request.action_key) {
            return Ok(existing.clone());
        }
        self.boundary(CreationStage::CapabilityNarrowing)?;
        self.last_capability = Some(request.clone());
        let capability = Capability::new(
            CapabilityId(request.capability_id),
            self.tenant.clone(),
            CapabilityDimensions {
                activity_types: request.activity_types.clone(),
                counterparties: request.counterparties.clone(),
                assets: request.assets.clone(),
                amount_ceiling: request.amount_ceiling,
                rate_ceiling: RateCeiling {
                    maximum_uses: request.rate_maximum_uses,
                    window_sequences: request.rate_window_sequences,
                },
                purposes: request.purposes.clone(),
                expiry_sequence: request.expiry_sequence,
            },
        )
        .map_err(|_| AgentFailure::Refused("capability construction failed"))?;
        let scope = ProtocolScope {
            activity_types: request.activity_types,
            counterparties: request.counterparties,
            assets: request.assets,
            amount_ceiling: request.amount_ceiling,
            expires_at_sequence: request.expiry_sequence,
            enforceable_dimensions: [
                Dimension::ActivityType,
                Dimension::Counterparty,
                Dimension::Asset,
                Dimension::Amount,
                Dimension::Expiry,
            ]
            .into_iter()
            .collect(),
        };
        let binding = assert_narrowing(
            &capability,
            ProtocolAuthority::CapabilityGrant(request.capability_id),
            &scope,
        )
        .map_err(|_| AgentFailure::Refused("capability is wider than protocol authority"))?;
        if !binding.enabled {
            return Err(AgentFailure::Refused("capability binding disabled"));
        }
        capability
            .persist(&mut self.store)
            .map_err(|_| AgentFailure::Refused("capability persistence failed"))?;
        let restored = Capability::restore(
            &self.store,
            self.tenant.clone(),
            CapabilityId(request.capability_id),
        )
        .map_err(|_| AgentFailure::Refused("capability restore failed"))?;
        if restored != Some(capability) {
            return Err(AgentFailure::Refused("capability restore mismatch"));
        }
        let evidence = Self::evidence(
            CreationStage::CapabilityNarrowing,
            request.action_key,
            request.capability_id,
        );
        self.effect(CreationStage::CapabilityNarrowing);
        self.agent_cache
            .insert(request.action_key, evidence.clone());
        Ok(evidence)
    }
}

struct CrashAfterEffect<'a> {
    inner: &'a mut InProcessAgentLayer,
    stage: CreationStage,
    fired: bool,
}

impl CrashAfterEffect<'_> {
    fn crash<T>(&mut self, stage: CreationStage, value: T) -> Result<T, AgentFailure> {
        if self.stage == stage && !self.fired {
            self.fired = true;
            Err(AgentFailure::Unavailable)
        } else {
            Ok(value)
        }
    }
}

impl AgentCreationContract for CrashAfterEffect<'_> {
    fn submit_protocol(
        &mut self,
        action: ProtocolAction,
    ) -> Result<ProtocolEvidence, AgentFailure> {
        let stage = action.stage;
        let evidence = self.inner.submit_protocol(action)?;
        self.crash(stage, evidence)
    }

    fn provision_session(
        &mut self,
        request: SessionProvision,
    ) -> Result<AgentEvidence, AgentFailure> {
        let evidence = self.inner.provision_session(request)?;
        self.crash(CreationStage::SessionProvision, evidence)
    }

    fn narrow_capability(
        &mut self,
        request: CapabilityProvision,
    ) -> Result<AgentEvidence, AgentFailure> {
        let evidence = self.inner.narrow_capability(request)?;
        self.crash(CreationStage::CapabilityNarrowing, evidence)
    }
}

fn intent_matches(stage: CreationStage, intent: &IntentKind) -> bool {
    matches!(
        (stage, intent),
        (
            CreationStage::DidRegistration,
            IntentKind::DidRegistration(_)
        ) | (
            CreationStage::RecoveryRegistration,
            IntentKind::RecoveryRegistration(_)
        ) | (CreationStage::BudgetCreation, IntentKind::BudgetCreate(_))
            | (CreationStage::BudgetFunding, IntentKind::BudgetFund(_))
    )
}

#[derive(Clone)]
struct ReceiptFields {
    activity_id: [u8; 32],
    previous_state_root: [u8; 32],
    resulting_state_root: [u8; 32],
    batch_id: [u8; 32],
    asset: [u8; 32],
    operation: u8,
    module: u16,
}

fn receipt(stage: CreationStage, action_key: [u8; 32]) -> ProtocolEvidence {
    let marker = stage as u8 + 11;
    let previous_state_root = [marker; 32];
    let fields = ReceiptFields {
        activity_id: action_key,
        previous_state_root,
        resulting_state_root: [marker.saturating_add(1); 32],
        batch_id: support::execution_batch_id(previous_state_root, action_key, CORE_SEQUENCE),
        asset: [0x22; 32],
        operation: match stage {
            CreationStage::DidRegistration | CreationStage::BudgetCreation => 1,
            CreationStage::BudgetFunding => 2,
            CreationStage::RecoveryRegistration => 3,
            _ => unreachable!(),
        },
        module: match stage {
            CreationStage::DidRegistration | CreationStage::RecoveryRegistration => {
                ModuleId::Governance as u16
            }
            CreationStage::BudgetCreation | CreationStage::BudgetFunding => ModuleId::Budget as u16,
            _ => unreachable!(),
        },
    };
    let signing_seed: [u8; 32] =
        Sha256::digest([b"agent-create-receipt".as_slice(), &action_key].concat()).into();
    let signer = SigningKey::from_bytes(&signing_seed);
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
            fields.asset,
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
    push_u64(&mut bytes, CORE_SEQUENCE);
    push_bytes(&mut bytes, &fields.previous_state_root);
    push_bytes(&mut bytes, &fields.resulting_state_root);
    push_bytes(&mut bytes, &[0x81; 32]);
    bytes.extend_from_slice(&0_i32.to_be_bytes());
    bytes.extend_from_slice(&0_u32.to_be_bytes());
    bytes.extend_from_slice(&1_u128.to_be_bytes());
    push_bytes(&mut bytes, &fields.batch_id);
    push_u16(&mut bytes, fields.module);
    bytes.extend_from_slice(&1_u32.to_be_bytes());
    bytes.extend_from_slice(&1_u32.to_be_bytes());
    bytes.push(fields.operation);
    push_bytes(&mut bytes, &fields.asset);
    bytes.extend_from_slice(&1_u128.to_be_bytes());
    push_bytes(&mut bytes, &[0x91; 32]);
    bytes.extend_from_slice(&10_u128.to_be_bytes());
    bytes.extend_from_slice(&9_u128.to_be_bytes());
    push_u64(&mut bytes, 1);
    push_bytes(&mut bytes, &[0x92; 32]);
    bytes.extend_from_slice(&20_u128.to_be_bytes());
    bytes.extend_from_slice(&21_u128.to_be_bytes());
    push_bytes(&mut bytes, &[0x93; 32]);
    push_bytes(&mut bytes, &[0x94; 32]);
    push_bytes(&mut bytes, &[0x95; 32]);
    push_u64(&mut bytes, 1_000);
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

fn push_bytes(output: &mut Vec<u8>, value: &[u8]) {
    let length = u32::try_from(value.len()).unwrap_or_else(|_| panic!("receipt field overflow"));
    output.extend_from_slice(&length.to_be_bytes());
    output.extend_from_slice(value);
}

const AGENT_STAGES: [CreationStage; 6] = [
    CreationStage::DidRegistration,
    CreationStage::RecoveryRegistration,
    CreationStage::SessionProvision,
    CreationStage::CapabilityNarrowing,
    CreationStage::BudgetCreation,
    CreationStage::BudgetFunding,
];

fn durable_kind(stage: CreationStage) -> ObjectKind {
    match stage {
        CreationStage::DidRegistration
        | CreationStage::RecoveryRegistration
        | CreationStage::BudgetFunding => ObjectKind::Receipt,
        CreationStage::SessionProvision => ObjectKind::Session,
        CreationStage::CapabilityNarrowing => ObjectKind::Capability,
        CreationStage::BudgetCreation => ObjectKind::Budget,
        CreationStage::Custody => unreachable!(),
    }
}

fn durable_count(root: &std::path::Path, stage: CreationStage) -> usize {
    let store = Store::open(root).unwrap_or_else(|error| panic!("inspect agent store: {error}"));
    let tenant =
        TenantId::new("tenant-alice").unwrap_or_else(|error| panic!("inspect tenant: {error}"));
    store.list_object_ids(&tenant, durable_kind(stage)).len()
}

#[test]
fn creation_is_receipt_gated_idempotent_and_uses_real_agentd_paths() {
    let mut fixture = Fixture::new("agent-create-complete");
    let alice = principal("alice");
    let mut scope = fixture
        .store
        .principal(&alice)
        .unwrap_or_else(|error| panic!("principal scope: {error}"));
    let mut journey = CreationJourney::start(
        &mut scope,
        &request("treasury"),
        &context(0x31),
        &catalog("treasury", 2_000),
        1,
    )
    .unwrap_or_else(|error| panic!("start: {error}"));
    assert_eq!(journey.status().state, CreationState::GettingReady);
    let mut agent = InProcessAgentLayer::new(&fixture.root.join("agent-store"), None);
    let active = journey
        .resume(
            &mut scope,
            &fixture.keystore,
            &fixture.registry,
            &mut agent,
            2,
        )
        .unwrap_or_else(|error| panic!("resume: {error}"));
    assert_eq!(active.state, CreationState::Active);
    assert!(active
        .stages
        .iter()
        .all(|(_, state)| *state == StageState::ReceiptVerified));
    for stage in AGENT_STAGES {
        assert_eq!(agent.count(stage), 1, "one effect for {stage:?}");
    }
    assert_eq!(agent.sessions.open_count(), 1);

    let agent_id = journey.agent_id();
    drop(scope);
    let mut reopened = fixture.reopen();
    let mut scope = reopened
        .principal(&alice)
        .unwrap_or_else(|error| panic!("reopened scope: {error}"));
    let mut recovered = CreationJourney::load(&scope, agent_id)
        .unwrap_or_else(|error| panic!("load: {error}"))
        .unwrap_or_else(|| panic!("journey missing"));
    let repeated = recovered
        .resume(
            &mut scope,
            &fixture.keystore,
            &fixture.registry,
            &mut agent,
            3,
        )
        .unwrap_or_else(|error| panic!("repeat resume: {error}"));
    assert_eq!(repeated.state, CreationState::Active);
    for stage in AGENT_STAGES {
        assert_eq!(agent.count(stage), 1, "no duplicate effect for {stage:?}");
    }
}

#[test]
fn every_agent_layer_failure_is_partial_and_resumes_after_service_restart() {
    for (index, failed_stage) in AGENT_STAGES.into_iter().enumerate() {
        let mut fixture = Fixture::new(&format!("agent-create-failure-{index}"));
        let alice = principal("alice");
        let mut scope = fixture
            .store
            .principal(&alice)
            .unwrap_or_else(|error| panic!("principal scope: {error}"));
        let mut journey = CreationJourney::start(
            &mut scope,
            &request("operations"),
            &context(u8::try_from(index + 1).unwrap_or_else(|_| unreachable!())),
            &catalog("operations", 1_500),
            10,
        )
        .unwrap_or_else(|error| panic!("start {failed_stage:?}: {error}"));
        let agent_root = fixture.root.join("agent-store");
        let mut agent = InProcessAgentLayer::new(&agent_root, Some(failed_stage));
        let partial = journey
            .resume(
                &mut scope,
                &fixture.keystore,
                &fixture.registry,
                &mut agent,
                11,
            )
            .unwrap_or_else(|error| panic!("partial {failed_stage:?}: {error}"));
        assert_eq!(partial.state, CreationState::Partial);
        assert_eq!(
            partial
                .stages
                .iter()
                .find(|(stage, _)| *stage == failed_stage)
                .map(|(_, state)| *state),
            Some(StageState::Unavailable)
        );
        assert_eq!(agent.count(failed_stage), 0, "failure precedes effect");

        let agent_id = journey.agent_id();
        drop(scope);
        let mut reopened = fixture.reopen();
        let mut scope = reopened
            .principal(&alice)
            .unwrap_or_else(|error| panic!("reopened scope: {error}"));
        let mut recovered = CreationJourney::load(&scope, agent_id)
            .unwrap_or_else(|error| panic!("load {failed_stage:?}: {error}"))
            .unwrap_or_else(|| panic!("journey missing for {failed_stage:?}"));
        let active = recovered
            .resume(
                &mut scope,
                &fixture.keystore,
                &fixture.registry,
                &mut agent,
                12,
            )
            .unwrap_or_else(|error| panic!("resume {failed_stage:?}: {error}"));
        assert_eq!(active.state, CreationState::Active);
        for stage in AGENT_STAGES {
            assert_eq!(
                agent.count(stage),
                1,
                "single {stage:?} after {failed_stage:?}"
            );
        }
    }
}

#[test]
fn crash_after_each_real_effect_replays_the_same_durable_object() {
    for (index, crashed_stage) in AGENT_STAGES.into_iter().enumerate() {
        let mut fixture = Fixture::new(&format!("agent-create-post-effect-{index}"));
        let alice = principal("alice");
        let mut scope = fixture
            .store
            .principal(&alice)
            .unwrap_or_else(|error| panic!("principal scope: {error}"));
        let mut journey = CreationJourney::start(
            &mut scope,
            &request("reconciliation"),
            &context(u8::try_from(index + 31).unwrap_or_else(|_| unreachable!())),
            &catalog("reconciliation", 1_500),
            20,
        )
        .unwrap_or_else(|error| panic!("start {crashed_stage:?}: {error}"));
        let agent_root = fixture.root.join("agent-store");
        let mut first_agent = InProcessAgentLayer::new(&agent_root, None);
        let mut crashing = CrashAfterEffect {
            inner: &mut first_agent,
            stage: crashed_stage,
            fired: false,
        };
        let partial = journey
            .resume(
                &mut scope,
                &fixture.keystore,
                &fixture.registry,
                &mut crashing,
                21,
            )
            .unwrap_or_else(|error| panic!("post-effect crash {crashed_stage:?}: {error}"));
        assert_eq!(partial.state, CreationState::Partial);
        assert_eq!(
            partial
                .stages
                .iter()
                .find(|(stage, _)| *stage == crashed_stage)
                .map(|(_, state)| *state),
            Some(StageState::Unavailable)
        );
        let objects_after_crash = durable_count(&agent_root, crashed_stage);
        assert!(objects_after_crash > 0);

        let agent_id = journey.agent_id();
        drop(scope);
        drop(first_agent);
        let mut reopened = fixture.reopen();
        let mut scope = reopened
            .principal(&alice)
            .unwrap_or_else(|error| panic!("reopened scope: {error}"));
        let mut recovered = CreationJourney::load(&scope, agent_id)
            .unwrap_or_else(|error| panic!("load {crashed_stage:?}: {error}"))
            .unwrap_or_else(|| panic!("journey missing for {crashed_stage:?}"));
        let next = AGENT_STAGES
            .iter()
            .position(|stage| *stage == crashed_stage)
            .and_then(|position| AGENT_STAGES.get(position + 1))
            .copied();
        let mut resumed_agent = InProcessAgentLayer::new(&agent_root, next);
        let resumed = recovered
            .resume(
                &mut scope,
                &fixture.keystore,
                &fixture.registry,
                &mut resumed_agent,
                22,
            )
            .unwrap_or_else(|error| panic!("post-crash resume {crashed_stage:?}: {error}"));
        assert_eq!(
            resumed
                .stages
                .iter()
                .find(|(stage, _)| *stage == crashed_stage)
                .map(|(_, state)| *state),
            Some(StageState::ReceiptVerified)
        );
        assert_eq!(
            durable_count(&agent_root, crashed_stage),
            objects_after_crash,
            "retry converges on one durable {crashed_stage:?} object"
        );
        let active = recovered
            .resume(
                &mut scope,
                &fixture.keystore,
                &fixture.registry,
                &mut resumed_agent,
                23,
            )
            .unwrap_or_else(|error| panic!("finish {crashed_stage:?}: {error}"));
        assert_eq!(active.state, CreationState::Active);
    }
}

#[test]
fn purpose_authority_comes_from_configuration_and_monthly_limit_only_narrows_it() {
    let catalog = catalog("vendor-payments", 750);
    assert_eq!(catalog.version(), "operator-policy-2026-08");
    assert!(CreateAgentRequest::new("helper", "unconfigured", 1).is_ok());

    let mut fixture = Fixture::new("agent-create-config");
    let alice = principal("alice");
    let mut scope = fixture
        .store
        .principal(&alice)
        .unwrap_or_else(|error| panic!("scope: {error}"));
    let Err(error) = CreationJourney::start(
        &mut scope,
        &request("unconfigured"),
        &context(0x61),
        &catalog,
        1,
    ) else {
        panic!("unconfigured purpose must be refused");
    };
    assert!(error.to_string().contains("purpose is not configured"));

    let mut configured = CreationJourney::start(
        &mut scope,
        &request("vendor-payments"),
        &context(0x62),
        &catalog,
        2,
    )
    .unwrap_or_else(|error| panic!("configured start: {error}"));
    let mut agent = InProcessAgentLayer::new(&fixture.root.join("agent-store"), None);
    let status = configured
        .resume(
            &mut scope,
            &fixture.keystore,
            &fixture.registry,
            &mut agent,
            3,
        )
        .unwrap_or_else(|error| panic!("configured resume: {error}"));
    assert_eq!(status.state, CreationState::Active);
    let capability = agent
        .last_capability
        .as_ref()
        .unwrap_or_else(|| panic!("configured capability missing"));
    assert_eq!(capability.amount_ceiling, 750);
    assert_eq!(
        capability.purposes,
        BTreeSet::from(["vendor-payments".to_owned()])
    );
}
