//! Durable, receipt-gated managed-agent creation.

use std::collections::BTreeSet;
use std::fmt::{Display, Formatter};

use layerx_intents::{
    compile, BudgetCreate, BudgetFund, CompileError, DidRegistration, DisclosureCheck,
    DisclosureCheckError, Intent, IntentError, IntentKind, RecoveryRegistration,
};
use layerx_proof::receipt::{verify, AuthorizedBatch, VerificationFailure};
use layerx_types::account::AccountId;
use layerx_types::amount::Amount;
use layerx_types::ids::{AssetId, Did, IdempotencyKey};
use layerx_types::intent::{
    ApprovalThreshold, BudgetId, PeriodLength, PublicKey, PurposeHash, RecoveryRoot,
    RolloverPolicy, TimestampSeconds,
};
use layerx_types::payload::{ActivityType, ModuleRegistry};
use layerx_types::verify::VerificationLevel;
use serde::{Deserialize, Serialize};
use sha2::{Digest as _, Sha256};

use crate::custody::{CustodyError, KeyClass, KeyId, Keystore};
use crate::store::{PrincipalScope, RowKey, StoreError, Table};

const RECORD_VERSION: u8 = 1;
const ACTION_DOMAIN: &[u8] = b"layerx-human/agent-create/action/v1";
const AGENT_EVIDENCE_DOMAIN: &[u8] = b"layerx-human/agent-create/agent-evidence/v1";
const CONFIG_DOMAIN: &[u8] = b"layerx-human/agent-create/purpose-config/v1";
const ID_DOMAIN: &[u8] = b"layerx-human/agent-create/id/v1";
const NAME_LIMIT: usize = 80;
const PURPOSE_LIMIT: usize = 64;

/// The exact user-facing create form. Infrastructure identifiers and recovery
/// policy are supplied by authenticated service configuration, not requested
/// from the human.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CreateAgentRequest {
    name: String,
    purpose: String,
    monthly_spend: u128,
}

impl CreateAgentRequest {
    /// Creates the three-field managed-agent request.
    ///
    /// # Errors
    ///
    /// Refuses empty or oversized text and a zero monthly spend limit.
    pub fn new(
        name: impl Into<String>,
        purpose: impl Into<String>,
        monthly_spend: u128,
    ) -> Result<Self, AgentCreationError> {
        let name = name.into();
        let purpose = purpose.into();
        if name.is_empty()
            || name.len() > NAME_LIMIT
            || purpose.is_empty()
            || purpose.len() > PURPOSE_LIMIT
            || monthly_spend == 0
        {
            return Err(AgentCreationError::InvalidRequest);
        }
        Ok(Self {
            name,
            purpose,
            monthly_spend,
        })
    }

    #[must_use]
    pub fn name(&self) -> &str {
        &self.name
    }

    #[must_use]
    pub fn purpose(&self) -> &str {
        &self.purpose
    }

    #[must_use]
    pub const fn monthly_spend(&self) -> u128 {
        self.monthly_spend
    }
}

/// Authenticated system facts that are deliberately absent from the create
/// form. The human recovery commitment is reused so recovery names the human.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CreationContext {
    pub idempotency_key: [u8; 32],
    pub owner_account: String,
    pub human_recovery_root: [u8; 32],
    pub recovery_threshold: u16,
    pub network_id: u32,
    pub protocol_time: u64,
}

impl CreationContext {
    fn validate(&self) -> Result<(), AgentCreationError> {
        if self.idempotency_key == [0; 32]
            || self.human_recovery_root == [0; 32]
            || self.recovery_threshold == 0
            || self.network_id == 0
            || self.protocol_time == 0
            || AccountId::parse(&self.owner_account).is_err()
        {
            return Err(AgentCreationError::InvalidContext);
        }
        Ok(())
    }
}

/// Configuration-owned purpose templates. No purpose-to-authority mapping is
/// compiled into service code.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct PurposePresetCatalog {
    version: String,
    presets: Vec<PurposePreset>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
struct PurposePreset {
    id: String,
    activity_types: Vec<u32>,
    counterparties: Vec<[u8; 32]>,
    assets: Vec<[u8; 32]>,
    amount_ceiling: u128,
    rate_maximum_uses: u64,
    rate_window_sequences: u64,
    purposes: Vec<String>,
    expiry_sequence: u64,
    session_scopes: Vec<String>,
    session_lifetime_seconds: u64,
    budget_asset: [u8; 32],
    budget_period_seconds: u64,
    budget_expiry_seconds: u64,
    initial_funding: u128,
}

impl PurposePresetCatalog {
    /// Parses and validates operator-owned JSON configuration.
    ///
    /// # Errors
    ///
    /// Refuses malformed JSON, duplicate presets, and any open or zero bound.
    pub fn from_json(bytes: &[u8]) -> Result<Self, AgentCreationError> {
        let catalog: Self =
            serde_json::from_slice(bytes).map_err(|_| AgentCreationError::InvalidPresetConfig)?;
        catalog.validate()?;
        Ok(catalog)
    }

    #[must_use]
    pub fn version(&self) -> &str {
        &self.version
    }

    fn preset(&self, id: &str) -> Option<&PurposePreset> {
        self.presets.iter().find(|preset| preset.id == id)
    }

    fn validate(&self) -> Result<(), AgentCreationError> {
        if self.version.is_empty() || self.presets.is_empty() {
            return Err(AgentCreationError::InvalidPresetConfig);
        }
        let mut ids = BTreeSet::new();
        for preset in &self.presets {
            let unique =
                |values: &[String]| values.iter().collect::<BTreeSet<_>>().len() == values.len();
            let activities = preset
                .activity_types
                .iter()
                .copied()
                .collect::<BTreeSet<_>>();
            let counterparties = preset
                .counterparties
                .iter()
                .copied()
                .collect::<BTreeSet<_>>();
            let assets = preset.assets.iter().copied().collect::<BTreeSet<_>>();
            if preset.id.is_empty()
                || !ids.insert(&preset.id)
                || preset.activity_types.is_empty()
                || activities.len() != preset.activity_types.len()
                || preset
                    .activity_types
                    .iter()
                    .any(|value| ActivityType::from_u32(*value).is_err())
                || preset.counterparties.is_empty()
                || counterparties.len() != preset.counterparties.len()
                || preset.assets.is_empty()
                || assets.len() != preset.assets.len()
                || preset.amount_ceiling == 0
                || preset.rate_maximum_uses == 0
                || preset.rate_window_sequences == 0
                || preset.purposes.is_empty()
                || preset.purposes.iter().any(String::is_empty)
                || preset.expiry_sequence == 0
                || preset.session_scopes.is_empty()
                || preset.session_scopes.iter().any(String::is_empty)
                || preset.session_lifetime_seconds == 0
                || preset.budget_asset == [0; 32]
                || preset.budget_period_seconds == 0
                || preset.budget_expiry_seconds == 0
                || preset.initial_funding == 0
                || !unique(&preset.purposes)
                || !unique(&preset.session_scopes)
            {
                return Err(AgentCreationError::InvalidPresetConfig);
            }
        }
        Ok(())
    }
}

/// Receipt-gated orchestration stages in dependency order.
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
pub enum CreationStage {
    Custody,
    DidRegistration,
    RecoveryRegistration,
    SessionProvision,
    CapabilityNarrowing,
    BudgetCreation,
    BudgetFunding,
}

impl CreationStage {
    const ALL: [Self; 7] = [
        Self::Custody,
        Self::DidRegistration,
        Self::RecoveryRegistration,
        Self::SessionProvision,
        Self::CapabilityNarrowing,
        Self::BudgetCreation,
        Self::BudgetFunding,
    ];

    const fn code(self) -> u8 {
        match self {
            Self::Custody => 1,
            Self::DidRegistration => 2,
            Self::RecoveryRegistration => 3,
            Self::SessionProvision => 4,
            Self::CapabilityNarrowing => 5,
            Self::BudgetCreation => 6,
            Self::BudgetFunding => 7,
        }
    }

    const fn operation(self) -> Option<u8> {
        match self {
            Self::DidRegistration | Self::BudgetCreation => Some(1),
            Self::RecoveryRegistration => Some(3),
            Self::BudgetFunding => Some(2),
            Self::Custody | Self::SessionProvision | Self::CapabilityNarrowing => None,
        }
    }
}

/// Honest per-stage state. Local custody is not called verified until the DID
/// receipt proves registration of the generated public key.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub enum StageState {
    Pending,
    LocalComplete,
    Unavailable,
    Refused,
    ReceiptVerified,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CreationState {
    GettingReady,
    Partial,
    Active,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CreationStatus {
    pub agent_id: [u8; 32],
    pub did: Did,
    pub state: CreationState,
    pub stages: Vec<(CreationStage, StageState)>,
}

/// Typed protocol submission. The agent contract receives the semantic intent,
/// compiler output, and disclosure proof together; callers cannot supply raw
/// node payload bytes.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ProtocolAction {
    pub stage: CreationStage,
    pub action_key: [u8; 32],
    pub intent: Intent,
    pub compiled: layerx_intents::CompiledIntent,
    pub disclosure: DisclosureCheck,
}

/// Canonical receipt and independently obtained batch authority returned by
/// the real agent/core submission path.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ProtocolEvidence {
    pub action_key: [u8; 32],
    pub activity_id: [u8; 32],
    pub receipt_bytes: Vec<u8>,
    pub authorized_batch: AuthorizedBatch,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SessionProvision {
    pub action_key: [u8; 32],
    pub did: Did,
    pub activity_types: Vec<ActivityType>,
    pub daemon_scopes: Vec<String>,
    pub expires_at: u64,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CapabilityProvision {
    pub action_key: [u8; 32],
    pub did: Did,
    pub capability_id: [u8; 32],
    pub activity_types: BTreeSet<u16>,
    pub counterparties: BTreeSet<[u8; 32]>,
    pub assets: BTreeSet<[u8; 32]>,
    pub amount_ceiling: u128,
    pub rate_maximum_uses: u64,
    pub rate_window_sequences: u64,
    pub purposes: BTreeSet<String>,
    pub expiry_sequence: u64,
}

/// Agent-layer evidence for joined session and narrowed-capability operations.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AgentEvidence {
    pub action_key: [u8; 32],
    pub object_id: [u8; 32],
    pub observed_sequence: u64,
    pub verification_level: VerificationLevel,
    pub receipt_digest: [u8; 32],
}

impl AgentEvidence {
    #[must_use]
    pub fn expected_digest(&self, stage: CreationStage) -> [u8; 32] {
        digest(&[
            AGENT_EVIDENCE_DOMAIN,
            &[stage.code()],
            &self.action_key,
            &self.object_id,
            &self.observed_sequence.to_be_bytes(),
            &[self.verification_level.wire_rank()],
        ])
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum AgentFailure {
    Unavailable,
    Refused(&'static str),
}

/// Versioned Human-to-agent creation seam. Implementations own prepare, sign,
/// submit, receipt retrieval, and the actual agentd installation operations.
/// Every method must durably deduplicate by `action_key` and return the same
/// evidence after either side restarts.
pub trait AgentCreationContract {
    /// Submits one compiler-verified protocol intent.
    ///
    /// # Errors
    ///
    /// Returns typed unavailability or refusal without claiming success.
    fn submit_protocol(&mut self, action: ProtocolAction)
        -> Result<ProtocolEvidence, AgentFailure>;

    /// Installs one bounded protocol session and daemon permission token.
    ///
    /// # Errors
    ///
    /// Returns typed unavailability or refusal without partial authority.
    fn provision_session(
        &mut self,
        request: SessionProvision,
    ) -> Result<AgentEvidence, AgentFailure>;

    /// Persists one capability proven no wider than core authority.
    ///
    /// # Errors
    ///
    /// Returns typed unavailability or refusal without enabling the capability.
    fn narrow_capability(
        &mut self,
        request: CapabilityProvision,
    ) -> Result<AgentEvidence, AgentFailure>;
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
struct StoredPreset {
    config_version: String,
    config_digest: [u8; 32],
    id: String,
    activity_types: Vec<u32>,
    counterparties: Vec<[u8; 32]>,
    assets: Vec<[u8; 32]>,
    amount_ceiling: u128,
    rate_maximum_uses: u64,
    rate_window_sequences: u64,
    purposes: Vec<String>,
    expiry_sequence: u64,
    session_scopes: Vec<String>,
    session_lifetime_seconds: u64,
    budget_asset: [u8; 32],
    budget_period_seconds: u64,
    budget_expiry_seconds: u64,
    initial_funding: u128,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
struct JourneyRecord {
    version: u8,
    agent_id: [u8; 32],
    idempotency_key: [u8; 32],
    name: String,
    monthly_spend: u128,
    did: Vec<u8>,
    owner_account: String,
    recovery_root: [u8; 32],
    recovery_threshold: u16,
    network_id: u32,
    started_at: u64,
    public_key: Option<[u8; 32]>,
    preset: StoredPreset,
    stages: Vec<StageRecord>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
struct StageRecord {
    stage: CreationStage,
    state: StageState,
    action_key: [u8; 32],
    evidence_digest: Option<[u8; 32]>,
    object_id: Option<[u8; 32]>,
}

/// Durable managed-agent creation state machine. Every retry uses the same
/// action keys, skips verified effects, and exposes partial state without
/// inventing an agent.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CreationJourney {
    record: JourneyRecord,
}

impl CreationJourney {
    /// Creates or rediscovers one idempotent journey.
    ///
    /// # Errors
    ///
    /// Refuses invalid inputs, unknown presets, conflicting retries, and store failures.
    pub fn start(
        scope: &mut PrincipalScope<'_>,
        request: &CreateAgentRequest,
        context: &CreationContext,
        catalog: &PurposePresetCatalog,
        now: u64,
    ) -> Result<Self, AgentCreationError> {
        context.validate()?;
        let preset = catalog
            .preset(request.purpose())
            .ok_or(AgentCreationError::UnknownPurpose)?;
        let agent_id = digest(&[
            ID_DOMAIN,
            scope.principal().as_str().as_bytes(),
            &context.network_id.to_be_bytes(),
            &context.idempotency_key,
        ]);
        if let Some(existing) = Self::load(scope, agent_id)? {
            if existing.record.idempotency_key == context.idempotency_key
                && existing.record.name == request.name
                && existing.record.monthly_spend == request.monthly_spend
                && existing.record.preset.id == request.purpose
            {
                return Ok(existing);
            }
            return Err(AgentCreationError::IdempotencyConflict);
        }
        let did = Did::new(format!("did:layerx:agent:{}", short_hex(&agent_id)).as_bytes())
            .map_err(|_| AgentCreationError::InvalidContext)?;
        let config_bytes = serde_json::to_vec(&(catalog.version.as_str(), preset))
            .map_err(|_| AgentCreationError::InvalidPresetConfig)?;
        let config_digest = digest(&[CONFIG_DOMAIN, &config_bytes]);
        let stored = StoredPreset {
            config_version: catalog.version.clone(),
            config_digest,
            id: preset.id.clone(),
            activity_types: preset.activity_types.clone(),
            counterparties: preset.counterparties.clone(),
            assets: preset.assets.clone(),
            amount_ceiling: preset.amount_ceiling,
            rate_maximum_uses: preset.rate_maximum_uses,
            rate_window_sequences: preset.rate_window_sequences,
            purposes: preset.purposes.clone(),
            expiry_sequence: preset.expiry_sequence,
            session_scopes: preset.session_scopes.clone(),
            session_lifetime_seconds: preset.session_lifetime_seconds,
            budget_asset: preset.budget_asset,
            budget_period_seconds: preset.budget_period_seconds,
            budget_expiry_seconds: preset.budget_expiry_seconds,
            initial_funding: preset.initial_funding,
        };
        let stages = CreationStage::ALL
            .into_iter()
            .map(|stage| StageRecord {
                stage,
                state: StageState::Pending,
                action_key: action_key(agent_id, stage),
                evidence_digest: None,
                object_id: None,
            })
            .collect();
        let journey = Self {
            record: JourneyRecord {
                version: RECORD_VERSION,
                agent_id,
                idempotency_key: context.idempotency_key,
                name: request.name.clone(),
                monthly_spend: request.monthly_spend,
                did: did.as_bytes().to_vec(),
                owner_account: context.owner_account.clone(),
                recovery_root: context.human_recovery_root,
                recovery_threshold: context.recovery_threshold,
                network_id: context.network_id,
                started_at: context.protocol_time,
                public_key: None,
                preset: stored,
                stages,
            },
        };
        journey.persist(scope, now)?;
        Ok(journey)
    }

    /// Loads one journey and validates its durable invariants.
    ///
    /// # Errors
    ///
    /// Refuses corrupt state, invalid identifiers, and store failures.
    pub fn load(
        scope: &PrincipalScope<'_>,
        agent_id: [u8; 32],
    ) -> Result<Option<Self>, AgentCreationError> {
        let key = journey_row(agent_id)?;
        let Some(row) = scope.get(Table::Journeys, &key) else {
            return Ok(None);
        };
        let record: JourneyRecord =
            serde_json::from_slice(row.bytes()).map_err(|_| AgentCreationError::CorruptJourney)?;
        validate_record(&record, agent_id)?;
        Ok(Some(Self { record }))
    }

    #[must_use]
    pub const fn agent_id(&self) -> [u8; 32] {
        self.record.agent_id
    }

    #[must_use]
    pub fn status(&self) -> CreationStatus {
        let active = self
            .record
            .stages
            .iter()
            .all(|stage| stage.state == StageState::ReceiptVerified);
        let touched = self
            .record
            .stages
            .iter()
            .any(|stage| stage.state != StageState::Pending);
        CreationStatus {
            agent_id: self.record.agent_id,
            did: self.did().unwrap_or_else(|_| unreachable!()),
            state: if active {
                CreationState::Active
            } else if touched {
                CreationState::Partial
            } else {
                CreationState::GettingReady
            },
            stages: self
                .record
                .stages
                .iter()
                .map(|stage| (stage.stage, stage.state))
                .collect(),
        }
    }

    /// Resumes from the first unverified stage. A boundary failure is persisted
    /// and returned as honest partial status; a later call retries the identical
    /// action key and never repeats a verified effect.
    ///
    /// # Errors
    ///
    /// Returns typed custody, intent, verification, evidence, and store failures.
    pub fn resume<C: AgentCreationContract>(
        &mut self,
        scope: &mut PrincipalScope<'_>,
        keystore: &Keystore,
        registry: &ModuleRegistry,
        agent: &mut C,
        now: u64,
    ) -> Result<CreationStatus, AgentCreationError> {
        self.ensure_custody(scope, keystore, now)?;
        for stage in CreationStage::ALL.into_iter().skip(1) {
            if self.stage(stage).state == StageState::ReceiptVerified {
                continue;
            }
            let result = match stage {
                CreationStage::DidRegistration
                | CreationStage::RecoveryRegistration
                | CreationStage::BudgetCreation
                | CreationStage::BudgetFunding => {
                    self.run_protocol(scope, registry, agent, stage, now)
                }
                CreationStage::SessionProvision => self.run_session(scope, agent, now),
                CreationStage::CapabilityNarrowing => self.run_capability(scope, agent, now),
                CreationStage::Custody => unreachable!(),
            };
            match result {
                Ok(()) => {}
                Err(AgentCreationError::Agent(failure)) => {
                    self.stage_mut(stage).state = match failure {
                        AgentFailure::Unavailable => StageState::Unavailable,
                        AgentFailure::Refused(_) => StageState::Refused,
                    };
                    self.persist(scope, now)?;
                    return Ok(self.status());
                }
                Err(error) => return Err(error),
            }
        }
        Ok(self.status())
    }

    fn ensure_custody(
        &mut self,
        scope: &mut PrincipalScope<'_>,
        keystore: &Keystore,
        now: u64,
    ) -> Result<(), AgentCreationError> {
        if self.record.public_key.is_some() {
            return Ok(());
        }
        let key_id = KeyId::new(format!("agent-{}", short_hex(&self.record.agent_id)))?;
        let descriptor = match keystore.describe(scope.principal(), &key_id) {
            Ok(value) => value,
            Err(CustodyError::KeyNotFound) => {
                match keystore.create(scope.principal(), &key_id, KeyClass::AgentPrimary) {
                    Ok(public_key) => crate::custody::KeyDescriptor {
                        class: KeyClass::AgentPrimary,
                        public_key,
                    },
                    Err(CustodyError::KeyExists) => {
                        keystore.describe(scope.principal(), &key_id)?
                    }
                    Err(error) => return Err(error.into()),
                }
            }
            Err(error) => return Err(error.into()),
        };
        if descriptor.class != KeyClass::AgentPrimary {
            return Err(AgentCreationError::EvidenceConflict);
        }
        self.record.public_key = Some(descriptor.public_key);
        self.stage_mut(CreationStage::Custody).state = StageState::LocalComplete;
        self.persist(scope, now)
    }

    fn run_protocol<C: AgentCreationContract>(
        &mut self,
        scope: &mut PrincipalScope<'_>,
        registry: &ModuleRegistry,
        agent: &mut C,
        stage: CreationStage,
        now: u64,
    ) -> Result<(), AgentCreationError> {
        let action = self.protocol_action(stage, registry)?;
        let action_key = action.action_key;
        let evidence = agent.submit_protocol(action)?;
        if evidence.action_key != action_key || evidence.activity_id != action_key {
            return Err(AgentCreationError::EvidenceConflict);
        }
        let verified = verify(&evidence.receipt_bytes, &evidence.authorized_batch)?;
        let protocol = verified
            .receipt()
            .protocol()
            .ok_or(AgentCreationError::EvidenceConflict)?;
        if protocol.activity_id() != evidence.activity_id
            || Some(protocol.operation()) != stage.operation()
        {
            return Err(AgentCreationError::EvidenceConflict);
        }
        let receipt_digest: [u8; 32] = Sha256::digest(verified.canonical_bytes()).into();
        put_exact(
            scope,
            evidence_row(self.record.agent_id, stage)?,
            now,
            verified.canonical_bytes().to_vec(),
        )?;
        let record = self.stage_mut(stage);
        if record
            .evidence_digest
            .is_some_and(|value| value != receipt_digest)
        {
            return Err(AgentCreationError::EvidenceConflict);
        }
        record.state = StageState::ReceiptVerified;
        record.evidence_digest = Some(receipt_digest);
        record.object_id = Some(evidence.activity_id);
        if stage == CreationStage::DidRegistration {
            self.stage_mut(CreationStage::Custody).state = StageState::ReceiptVerified;
            self.stage_mut(CreationStage::Custody).evidence_digest = Some(receipt_digest);
        }
        self.persist(scope, now)
    }

    fn run_session<C: AgentCreationContract>(
        &mut self,
        scope: &mut PrincipalScope<'_>,
        agent: &mut C,
        now: u64,
    ) -> Result<(), AgentCreationError> {
        let action_key = self.stage(CreationStage::SessionProvision).action_key;
        let activity_types = self
            .record
            .preset
            .activity_types
            .iter()
            .map(|value| {
                ActivityType::from_u32(*value).map_err(|_| AgentCreationError::InvalidPresetConfig)
            })
            .collect::<Result<Vec<_>, _>>()?;
        let evidence = agent.provision_session(SessionProvision {
            action_key,
            did: self.did()?,
            activity_types,
            daemon_scopes: self.record.preset.session_scopes.clone(),
            expires_at: self
                .record
                .started_at
                .checked_add(self.record.preset.session_lifetime_seconds)
                .ok_or(AgentCreationError::InvalidPresetConfig)?,
        })?;
        self.accept_agent_evidence(scope, CreationStage::SessionProvision, &evidence, now)
    }

    fn run_capability<C: AgentCreationContract>(
        &mut self,
        scope: &mut PrincipalScope<'_>,
        agent: &mut C,
        now: u64,
    ) -> Result<(), AgentCreationError> {
        let stage = CreationStage::CapabilityNarrowing;
        let request = CapabilityProvision {
            action_key: self.stage(stage).action_key,
            did: self.did()?,
            capability_id: digest(&[b"layerx-human/agent-capability/v1", &self.record.agent_id]),
            activity_types: self
                .record
                .preset
                .activity_types
                .iter()
                .map(|value| {
                    u16::try_from(value & 0xffff)
                        .map_err(|_| AgentCreationError::InvalidPresetConfig)
                })
                .collect::<Result<_, _>>()?,
            counterparties: self.record.preset.counterparties.iter().copied().collect(),
            assets: self.record.preset.assets.iter().copied().collect(),
            amount_ceiling: self
                .record
                .monthly_spend
                .min(self.record.preset.amount_ceiling),
            rate_maximum_uses: self.record.preset.rate_maximum_uses,
            rate_window_sequences: self.record.preset.rate_window_sequences,
            purposes: self.record.preset.purposes.iter().cloned().collect(),
            expiry_sequence: self.record.preset.expiry_sequence,
        };
        let evidence = agent.narrow_capability(request)?;
        self.accept_agent_evidence(scope, stage, &evidence, now)
    }

    fn accept_agent_evidence(
        &mut self,
        scope: &mut PrincipalScope<'_>,
        stage: CreationStage,
        evidence: &AgentEvidence,
        now: u64,
    ) -> Result<(), AgentCreationError> {
        if evidence.action_key != self.stage(stage).action_key
            || evidence.object_id == [0; 32]
            || evidence.observed_sequence == 0
            || evidence.verification_level < VerificationLevel::BATCH_INCLUDED
            || evidence.receipt_digest != evidence.expected_digest(stage)
        {
            return Err(AgentCreationError::EvidenceConflict);
        }
        let bytes = serde_json::to_vec(&(
            evidence.action_key,
            evidence.object_id,
            evidence.observed_sequence,
            evidence.verification_level.wire_rank(),
            evidence.receipt_digest,
        ))
        .map_err(|_| AgentCreationError::EvidenceConflict)?;
        put_exact(
            scope,
            evidence_row(self.record.agent_id, stage)?,
            now,
            bytes,
        )?;
        let record = self.stage_mut(stage);
        record.state = StageState::ReceiptVerified;
        record.evidence_digest = Some(evidence.receipt_digest);
        record.object_id = Some(evidence.object_id);
        self.persist(scope, now)
    }

    fn protocol_action(
        &self,
        stage: CreationStage,
        registry: &ModuleRegistry,
    ) -> Result<ProtocolAction, AgentCreationError> {
        let did = self.did()?;
        let intent = match stage {
            CreationStage::DidRegistration => {
                Intent::v1(IntentKind::DidRegistration(DidRegistration::new(
                    did,
                    PublicKey::new(
                        self.record
                            .public_key
                            .ok_or(AgentCreationError::CustodyRequired)?,
                    ),
                )?))
            }
            CreationStage::RecoveryRegistration => {
                Intent::v1(IntentKind::RecoveryRegistration(RecoveryRegistration::new(
                    did,
                    RecoveryRoot::new(self.record.recovery_root),
                    ApprovalThreshold::new(self.record.recovery_threshold)
                        .map_err(|_| AgentCreationError::InvalidContext)?,
                )?))
            }
            CreationStage::BudgetCreation => {
                Intent::v1(IntentKind::BudgetCreate(BudgetCreate::new(
                    BudgetId::new(self.record.agent_id),
                    self.owner_account()?,
                    self.budget_account()?,
                    AssetId::new(self.record.preset.budget_asset),
                    Amount::from_u128(self.record.monthly_spend),
                    PeriodLength::new(self.record.preset.budget_period_seconds)
                        .map_err(|_| AgentCreationError::InvalidPresetConfig)?,
                    RolloverPolicy::None,
                    Amount::ZERO,
                    PurposeHash::new(self.record.preset.config_digest),
                    TimestampSeconds::from_u64(
                        self.record
                            .started_at
                            .checked_add(self.record.preset.budget_expiry_seconds)
                            .ok_or(AgentCreationError::InvalidPresetConfig)?,
                    ),
                )?))
            }
            CreationStage::BudgetFunding => Intent::v1(IntentKind::BudgetFund(BudgetFund::new(
                BudgetId::new(self.record.agent_id),
                self.owner_account()?,
                self.budget_account()?,
                AssetId::new(self.record.preset.budget_asset),
                Amount::from_u128(self.record.preset.initial_funding),
                IdempotencyKey::new(self.stage(stage).action_key),
            )?)),
            CreationStage::Custody
            | CreationStage::SessionProvision
            | CreationStage::CapabilityNarrowing => return Err(AgentCreationError::InvalidStage),
        };
        let compiled = compile(&intent, registry)?;
        let disclosure = DisclosureCheck::verify(&intent, &compiled)?;
        Ok(ProtocolAction {
            stage,
            action_key: self.stage(stage).action_key,
            intent,
            compiled,
            disclosure,
        })
    }

    fn did(&self) -> Result<Did, AgentCreationError> {
        Did::new(&self.record.did).map_err(|_| AgentCreationError::CorruptJourney)
    }

    fn owner_account(&self) -> Result<AccountId, AgentCreationError> {
        AccountId::parse(&self.record.owner_account).map_err(|_| AgentCreationError::CorruptJourney)
    }

    fn budget_account(&self) -> Result<AccountId, AgentCreationError> {
        let did = std::str::from_utf8(&self.record.did)
            .map_err(|_| AgentCreationError::CorruptJourney)?;
        AccountId::parse(&format!(
            "agent:{did}:budget:{}",
            short_hex(&self.record.agent_id)
        ))
        .map_err(|_| AgentCreationError::CorruptJourney)
    }

    fn stage(&self, stage: CreationStage) -> &StageRecord {
        self.record
            .stages
            .iter()
            .find(|record| record.stage == stage)
            .unwrap_or_else(|| unreachable!())
    }

    fn stage_mut(&mut self, stage: CreationStage) -> &mut StageRecord {
        self.record
            .stages
            .iter_mut()
            .find(|record| record.stage == stage)
            .unwrap_or_else(|| unreachable!())
    }

    fn persist(&self, scope: &mut PrincipalScope<'_>, now: u64) -> Result<(), AgentCreationError> {
        let bytes =
            serde_json::to_vec(&self.record).map_err(|_| AgentCreationError::CorruptJourney)?;
        scope.put(
            Table::Journeys,
            journey_row(self.record.agent_id)?,
            now,
            bytes,
        )?;
        Ok(())
    }
}

fn validate_record(record: &JourneyRecord, expected: [u8; 32]) -> Result<(), AgentCreationError> {
    if record.version != RECORD_VERSION
        || record.agent_id != expected
        || record.idempotency_key == [0; 32]
        || record.did.is_empty()
        || record.stages.len() != CreationStage::ALL.len()
        || record
            .stages
            .iter()
            .zip(CreationStage::ALL)
            .any(|(record, expected_stage)| {
                record.stage != expected_stage
                    || record.action_key != action_key(expected, expected_stage)
                    || (record.state == StageState::ReceiptVerified
                        && record.evidence_digest.is_none())
            })
    {
        return Err(AgentCreationError::CorruptJourney);
    }
    Ok(())
}

fn action_key(agent_id: [u8; 32], stage: CreationStage) -> [u8; 32] {
    digest(&[ACTION_DOMAIN, &agent_id, &[stage.code()]])
}

fn journey_row(agent_id: [u8; 32]) -> Result<RowKey, AgentCreationError> {
    Ok(RowKey::new(format!(
        "agent-create-{}",
        short_hex(&agent_id)
    ))?)
}

fn evidence_row(agent_id: [u8; 32], stage: CreationStage) -> Result<RowKey, AgentCreationError> {
    Ok(RowKey::new(format!(
        "agent-create-{}-e{}",
        short_hex(&agent_id),
        stage.code()
    ))?)
}

fn put_exact(
    scope: &mut PrincipalScope<'_>,
    key: RowKey,
    now: u64,
    bytes: Vec<u8>,
) -> Result<(), AgentCreationError> {
    if let Some(existing) = scope.get(Table::Journeys, &key) {
        return if existing.bytes() == bytes {
            Ok(())
        } else {
            Err(AgentCreationError::EvidenceConflict)
        };
    }
    scope.put(Table::Journeys, key, now, bytes)?;
    Ok(())
}

fn digest(parts: &[&[u8]]) -> [u8; 32] {
    let mut hasher = Sha256::new();
    for part in parts {
        hasher.update(part);
    }
    hasher.finalize().into()
}

fn short_hex(bytes: &[u8; 32]) -> String {
    let mut text = String::with_capacity(32);
    for byte in &bytes[..16] {
        use std::fmt::Write as _;
        let _ = write!(text, "{byte:02x}");
    }
    text
}

/// Typed creation refusal. Agent failures are persisted as partial state by
/// `resume`; all other variants reject inconsistent local evidence.
#[derive(Debug)]
pub enum AgentCreationError {
    Store(StoreError),
    Custody(CustodyError),
    Intent(IntentError),
    Compile(CompileError),
    Disclosure(DisclosureCheckError),
    Verification(VerificationFailure),
    Agent(AgentFailure),
    InvalidRequest,
    InvalidContext,
    InvalidPresetConfig,
    UnknownPurpose,
    IdempotencyConflict,
    EntropyUnavailable,
    CustodyRequired,
    InvalidStage,
    EvidenceConflict,
    CorruptJourney,
}

impl Display for AgentCreationError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Store(error) => write!(formatter, "agent creation store failure: {error}"),
            Self::Custody(error) => write!(formatter, "agent creation custody failure: {error}"),
            Self::Intent(error) => write!(formatter, "agent creation intent failure: {error:?}"),
            Self::Compile(error) => write!(formatter, "agent creation compile failure: {error:?}"),
            Self::Disclosure(error) => write!(formatter, "agent disclosure failure: {error:?}"),
            Self::Verification(error) => write!(formatter, "agent receipt failure: {error:?}"),
            Self::Agent(error) => write!(formatter, "agent layer failure: {error:?}"),
            Self::InvalidRequest => formatter.write_str("agent request is invalid"),
            Self::InvalidContext => formatter.write_str("agent creation context is invalid"),
            Self::InvalidPresetConfig => {
                formatter.write_str("purpose preset configuration is invalid")
            }
            Self::UnknownPurpose => formatter.write_str("purpose is not configured"),
            Self::IdempotencyConflict => formatter.write_str("agent creation idempotency conflict"),
            Self::EntropyUnavailable => formatter.write_str("agent key entropy is unavailable"),
            Self::CustodyRequired => formatter.write_str("agent custody key is incomplete"),
            Self::InvalidStage => formatter.write_str("agent creation stage is invalid"),
            Self::EvidenceConflict => formatter.write_str("agent creation evidence conflicts"),
            Self::CorruptJourney => formatter.write_str("agent creation journey is corrupt"),
        }
    }
}

impl std::error::Error for AgentCreationError {}

impl From<StoreError> for AgentCreationError {
    fn from(value: StoreError) -> Self {
        Self::Store(value)
    }
}
impl From<CustodyError> for AgentCreationError {
    fn from(value: CustodyError) -> Self {
        Self::Custody(value)
    }
}
impl From<IntentError> for AgentCreationError {
    fn from(value: IntentError) -> Self {
        Self::Intent(value)
    }
}
impl From<CompileError> for AgentCreationError {
    fn from(value: CompileError) -> Self {
        Self::Compile(value)
    }
}
impl From<DisclosureCheckError> for AgentCreationError {
    fn from(value: DisclosureCheckError) -> Self {
        Self::Disclosure(value)
    }
}
impl From<VerificationFailure> for AgentCreationError {
    fn from(value: VerificationFailure) -> Self {
        Self::Verification(value)
    }
}
impl From<AgentFailure> for AgentCreationError {
    fn from(value: AgentFailure) -> Self {
        Self::Agent(value)
    }
}
