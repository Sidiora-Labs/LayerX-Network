//! Managed-agent pause, resume, and spend-limit controls.

use std::collections::BTreeMap;
use std::fmt::{Display, Formatter};

use layerx_intents::{
    compile, BudgetCreate, CompileError, DisclosureCheck, DisclosureCheckError, Intent,
    IntentError, IntentKind,
};
use layerx_proof::receipt::{verify, VerificationFailure};
use layerx_types::account::AccountId;
use layerx_types::amount::Amount;
use layerx_types::ids::{AssetId, Did};
use layerx_types::intent::{BudgetId, PeriodLength, PurposeHash, RolloverPolicy, TimestampSeconds};
use layerx_types::payload::{ModuleId, ModuleRegistry};
use layerx_types::verify::VerificationLevel;
use sha2::{Digest as _, Sha256};

use crate::custody::{
    AgentSessionContract, KeyId, RevocationOutcome, SessionEntropySource, SessionKeyError,
    SessionKeyProvisioner, SessionLease, SessionLeaseState,
};
use crate::store::PrincipalId;

use super::{AgentCreationContract, AgentFailure, CreationStage, ProtocolAction, ProtocolEvidence};

const ACTION_DOMAIN: &[u8] = b"layerx-human/agent-control-action/v1";
const BUDGET_DOMAIN: &[u8] = b"layerx-human/agent-limit-budget/v1";
const APP_EVIDENCE_DOMAIN: &[u8] = b"layerx-human/app-limit-evidence/v1";

/// Copy-catalog key for the reversible pause consequence sentence.
pub const PAUSE_CONSEQUENCE_COPY_KEY: &str = "agent.pause.consequence";
/// Copy-catalog key for a receipt-backed protocol budget.
pub const PROTOCOL_LIMIT_COPY_KEY: &str = "agent.limit.protocol-enforced";
/// Copy-catalog key for a restriction enforced by this plane.
pub const APP_LIMIT_COPY_KEY: &str = "agent.limit.app-enforced";
/// Mandatory honest description carried beside every local restriction.
pub const APP_LIMIT_EXPLANATION: &str = "This limit is enforced by the app and agent service. Bypassing them bypasses it; it is not a protocol guarantee.";
/// Plain consequence sentence named by [`PAUSE_CONSEQUENCE_COPY_KEY`].
pub const PAUSE_CONSEQUENCE: &str =
    "The agent stops acting now. You can resume it later after its authority is restored.";

/// Runtime authority used for the current spend limit.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum LimitEnforcement {
    Protocol,
    App,
}

impl LimitEnforcement {
    /// Returns the exact public contract value.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Protocol => "protocol",
            Self::App => "app",
        }
    }

    /// Returns the mandatory copy-catalog key.
    #[must_use]
    pub const fn copy_key(self) -> &'static str {
        match self {
            Self::Protocol => PROTOCOL_LIMIT_COPY_KEY,
            Self::App => APP_LIMIT_COPY_KEY,
        }
    }
}

/// Initial backing for one managed agent's spend limit.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum LimitBacking {
    Protocol { active_budget_id: [u8; 32] },
    App,
}

impl LimitBacking {
    const fn enforcement(self) -> LimitEnforcement {
        match self {
            Self::Protocol { .. } => LimitEnforcement::Protocol,
            Self::App => LimitEnforcement::App,
        }
    }

    const fn budget_id(self) -> Option<[u8; 32]> {
        match self {
            Self::Protocol { active_budget_id } => Some(active_budget_id),
            Self::App => None,
        }
    }
}

/// Immutable authority and budget facts for one managed agent.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AgentControlProfile {
    pub principal: PrincipalId,
    pub agent_id: [u8; 32],
    pub did: Did,
    pub owner: AccountId,
    pub budget_account: AccountId,
    pub asset: AssetId,
    pub purpose: PurposeHash,
    pub period_seconds: u64,
    pub budget_lifetime_seconds: u64,
    pub freshness_seconds: u64,
    pub initial_limit_evidence_digest: [u8; 32],
}

impl AgentControlProfile {
    fn validate(&self) -> Result<(), AgentControlError> {
        if self.agent_id == [0; 32]
            || self.asset.bytes() == [0; 32]
            || self.purpose.bytes() == [0; 32]
            || self.period_seconds == 0
            || self.budget_lifetime_seconds == 0
            || self.freshness_seconds == 0
            || self.owner == self.budget_account
        {
            return Err(AgentControlError::InvalidProfile);
        }
        Ok(())
    }
}

/// One idempotent spend-limit request.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct LimitChangeRequest {
    pub idempotency_key: [u8; 32],
    pub monthly_limit: u128,
}

impl LimitChangeRequest {
    /// Constructs a non-zero exact-integer limit request.
    ///
    /// # Errors
    ///
    /// Refuses the reserved zero idempotency key and a zero limit.
    pub fn new(idempotency_key: [u8; 32], monthly_limit: u128) -> Result<Self, AgentControlError> {
        if idempotency_key == [0; 32] || monthly_limit == 0 {
            return Err(AgentControlError::InvalidLimit);
        }
        Ok(Self {
            idempotency_key,
            monthly_limit,
        })
    }
}

/// Exact daemon-side change request. It cannot claim protocol enforcement.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AppLimitRequest {
    pub action_key: [u8; 32],
    pub agent_id: [u8; 32],
    pub asset: [u8; 32],
    pub ceiling: u128,
}

/// Durable agent-layer evidence for an app-enforced restriction.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AppLimitEvidence {
    pub action_key: [u8; 32],
    pub agent_id: [u8; 32],
    pub asset: [u8; 32],
    pub ceiling: u128,
    pub observed_sequence: u64,
    pub configuration_digest: [u8; 32],
}

impl AppLimitEvidence {
    /// Computes the exact evidence digest expected from the agent contract.
    #[must_use]
    pub fn expected_digest(&self) -> [u8; 32] {
        digest(&[
            APP_EVIDENCE_DOMAIN,
            &self.action_key,
            &self.agent_id,
            &self.asset,
            &self.ceiling.to_be_bytes(),
            &self.observed_sequence.to_be_bytes(),
        ])
    }
}

/// Agent contracts used by the concrete session-control adapter add one
/// explicitly local restriction operation to the existing real session and
/// typed-protocol seams.
pub trait AgentControlAgent: AgentSessionContract + AgentCreationContract {
    /// Installs or replaces one daemon-side restriction.
    ///
    /// # Errors
    ///
    /// Returns typed unavailability or refusal without calling it protocol-backed.
    fn apply_app_limit(
        &mut self,
        request: AppLimitRequest,
    ) -> Result<AppLimitEvidence, AgentFailure>;
}

/// High-level boundary consumed by [`AgentControls`].
pub trait AgentControlContract {
    fn current_authority(&self, principal: &PrincipalId, did: &Did) -> Option<SessionLease>;

    /// Removes the named agent's operating authority.
    ///
    /// # Errors
    ///
    /// Returns typed session or agent-contract evidence failures.
    fn pause_authority(
        &mut self,
        principal: &PrincipalId,
        did: &Did,
        requested_at: u64,
    ) -> Result<RevocationOutcome, AgentControlError>;

    /// Restores the named agent through a fresh verified session.
    ///
    /// # Errors
    ///
    /// Returns typed session or agent-contract evidence failures.
    fn resume_authority(
        &mut self,
        principal: &PrincipalId,
        did: &Did,
    ) -> Result<SessionLease, AgentControlError>;

    /// Submits one compiler-verified protocol budget change.
    ///
    /// # Errors
    ///
    /// Returns typed unavailability or refusal from the agent boundary.
    fn submit_protocol_limit(
        &mut self,
        action: ProtocolAction,
    ) -> Result<ProtocolEvidence, AgentControlError>;

    /// Applies one explicitly local app restriction.
    ///
    /// # Errors
    ///
    /// Returns typed unavailability or refusal from the agent boundary.
    fn apply_app_limit(
        &mut self,
        request: AppLimitRequest,
    ) -> Result<AppLimitEvidence, AgentControlError>;
}

/// Concrete bridge from Human controls to the receipt-verified session
/// provisioner and the versioned agent contract.
pub struct SessionControlAdapter<C: AgentControlAgent, E: SessionEntropySource> {
    sessions: SessionKeyProvisioner<C, E>,
}

impl<C: AgentControlAgent, E: SessionEntropySource> SessionControlAdapter<C, E> {
    #[must_use]
    pub const fn new(sessions: SessionKeyProvisioner<C, E>) -> Self {
        Self { sessions }
    }

    #[must_use]
    pub const fn sessions(&self) -> &SessionKeyProvisioner<C, E> {
        &self.sessions
    }

    #[must_use]
    pub const fn sessions_mut(&mut self) -> &mut SessionKeyProvisioner<C, E> {
        &mut self.sessions
    }
}

impl<C: AgentControlAgent, E: SessionEntropySource> AgentControlContract
    for SessionControlAdapter<C, E>
{
    fn current_authority(&self, principal: &PrincipalId, did: &Did) -> Option<SessionLease> {
        self.sessions.session(principal, did).cloned()
    }

    fn pause_authority(
        &mut self,
        principal: &PrincipalId,
        did: &Did,
        requested_at: u64,
    ) -> Result<RevocationOutcome, AgentControlError> {
        Ok(self.sessions.pause(principal, did, requested_at)?)
    }

    fn resume_authority(
        &mut self,
        principal: &PrincipalId,
        did: &Did,
    ) -> Result<SessionLease, AgentControlError> {
        Ok(self.sessions.resume(principal, did)?)
    }

    fn submit_protocol_limit(
        &mut self,
        action: ProtocolAction,
    ) -> Result<ProtocolEvidence, AgentControlError> {
        Ok(self.sessions.contract_mut().submit_protocol(action)?)
    }

    fn apply_app_limit(
        &mut self,
        request: AppLimitRequest,
    ) -> Result<AppLimitEvidence, AgentControlError> {
        Ok(self.sessions.contract_mut().apply_app_limit(request)?)
    }
}

/// Honest visible lifecycle state.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum AgentControlState {
    Active,
    Paused,
}

impl AgentControlState {
    #[must_use]
    pub const fn copy_key(self) -> &'static str {
        match self {
            Self::Active => "agent.state.active",
            Self::Paused => "agent.state.paused",
        }
    }
}

/// The spend-limit portion of the public agent record.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PresentedLimit {
    pub monthly: u128,
    pub enforcement: LimitEnforcement,
    pub enforcement_copy_key: &'static str,
    pub explanation: &'static str,
    pub evidence_digest: [u8; 32],
    pub verification_level: VerificationLevel,
}

/// Fresh public state returned by every control operation.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AgentControlView {
    pub agent_id: [u8; 32],
    pub state: AgentControlState,
    pub state_copy_key: &'static str,
    pub pause_consequence_copy_key: Option<&'static str>,
    pub pause_consequence: Option<&'static str>,
    pub limit: PresentedLimit,
    pub updated_at: u64,
    pub fresh_until: u64,
    pub authority_evidence: Vec<[u8; 32]>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct CompletedLimitChange {
    request: LimitChangeRequest,
    view: AgentControlView,
}

struct LimitChangeEvidence {
    digest: [u8; 32],
    level: VerificationLevel,
    replacement_budget: Option<[u8; 32]>,
}

/// Receipt-gated managed-agent controls. Protocol-backed changes always pass
/// through `layerx-intents`; local restrictions have a structurally distinct
/// app-enforced result and cannot acquire protocol wording.
pub struct AgentControls<B: AgentControlContract> {
    boundary: B,
    profile: AgentControlProfile,
    state: AgentControlState,
    limit: PresentedLimit,
    active_budget_id: Option<[u8; 32]>,
    updated_at: u64,
    authority_evidence: Vec<[u8; 32]>,
    completed_limit_changes: BTreeMap<[u8; 32], CompletedLimitChange>,
}

impl<B: AgentControlContract> AgentControls<B> {
    /// Binds one active receipt-verified agent to its initial presented limit.
    ///
    /// # Errors
    ///
    /// Refuses invalid profile data, missing live authority, and unbacked
    /// protocol limit identifiers.
    pub fn new(
        boundary: B,
        profile: AgentControlProfile,
        monthly_limit: u128,
        backing: LimitBacking,
        observed_at: u64,
    ) -> Result<Self, AgentControlError> {
        profile.validate()?;
        if monthly_limit == 0
            || observed_at == 0
            || matches!(backing, LimitBacking::Protocol { active_budget_id } if active_budget_id == [0; 32])
            || matches!(backing, LimitBacking::Protocol { .. })
                && profile.initial_limit_evidence_digest == [0; 32]
        {
            return Err(AgentControlError::InvalidLimit);
        }
        let authority = boundary
            .current_authority(&profile.principal, &profile.did)
            .ok_or(AgentControlError::AuthorityMissing)?;
        if !matches!(authority.state, SessionLeaseState::Active)
            || authority.provision_receipt_digest == [0; 32]
        {
            return Err(AgentControlError::AuthorityMissing);
        }
        let enforcement = backing.enforcement();
        let explanation = match enforcement {
            LimitEnforcement::Protocol => {
                "This limit is enforced by the protocol budget named by the verified receipt."
            }
            LimitEnforcement::App => APP_LIMIT_EXPLANATION,
        };
        let (limit_evidence, limit_level) = match enforcement {
            LimitEnforcement::Protocol => (
                profile.initial_limit_evidence_digest,
                VerificationLevel::BATCH_INCLUDED,
            ),
            LimitEnforcement::App => ([0; 32], VerificationLevel::UNVERIFIED),
        };
        Ok(Self {
            boundary,
            profile,
            state: AgentControlState::Active,
            limit: PresentedLimit {
                monthly: monthly_limit,
                enforcement,
                enforcement_copy_key: enforcement.copy_key(),
                explanation,
                evidence_digest: limit_evidence,
                verification_level: limit_level,
            },
            active_budget_id: backing.budget_id(),
            updated_at: observed_at,
            authority_evidence: vec![authority.provision_receipt_digest],
            completed_limit_changes: BTreeMap::new(),
        })
    }

    #[must_use]
    pub const fn boundary(&self) -> &B {
        &self.boundary
    }

    #[must_use]
    pub const fn boundary_mut(&mut self) -> &mut B {
        &mut self.boundary
    }

    /// Returns the current record only inside its declared freshness bound.
    ///
    /// # Errors
    ///
    /// Refuses stale state instead of silently serving an old authority claim.
    pub fn view(&self, now: u64) -> Result<AgentControlView, AgentControlError> {
        let fresh_until = self
            .updated_at
            .checked_add(self.profile.freshness_seconds)
            .ok_or(AgentControlError::TimeOverflow)?;
        if now > fresh_until {
            return Err(AgentControlError::Stale {
                last_verified_at: self.updated_at,
                freshness_seconds: self.profile.freshness_seconds,
            });
        }
        Ok(self.build_view(fresh_until))
    }

    /// Promptly suspends daemon permissions, revokes the protocol session, and
    /// exposes pause as a reversible action with its exact consequence.
    ///
    /// # Errors
    ///
    /// Returns the typed authority or freshness refusal without claiming pause.
    pub fn pause(&mut self, requested_at: u64) -> Result<AgentControlView, AgentControlError> {
        if self.state == AgentControlState::Paused {
            return self.view(requested_at);
        }
        let outcome = self.boundary.pause_authority(
            &self.profile.principal,
            &self.profile.did,
            requested_at,
        )?;
        if outcome.revoked_at < requested_at
            || outcome.suspension_receipt_digest == [0; 32]
            || outcome.revocation_receipt_digest == [0; 32]
        {
            return Err(AgentControlError::EvidenceConflict);
        }
        self.state = AgentControlState::Paused;
        self.updated_at = outcome.revoked_at;
        self.authority_evidence
            .push(outcome.suspension_receipt_digest);
        self.authority_evidence
            .push(outcome.revocation_receipt_digest);
        self.view(outcome.revoked_at)
    }

    /// Restores authority only after a fresh protocol grant and daemon session
    /// have produced their receipt-backed lease.
    ///
    /// # Errors
    ///
    /// Returns the typed session or evidence refusal without presenting Active.
    pub fn resume(&mut self, observed_at: u64) -> Result<AgentControlView, AgentControlError> {
        if self.state == AgentControlState::Active {
            return self.view(observed_at);
        }
        let lease = self
            .boundary
            .resume_authority(&self.profile.principal, &self.profile.did)?;
        if !matches!(lease.state, SessionLeaseState::Active)
            || lease.provision_receipt_digest == [0; 32]
            || lease.issued_at > observed_at
        {
            return Err(AgentControlError::EvidenceConflict);
        }
        self.state = AgentControlState::Active;
        self.updated_at = observed_at;
        self.authority_evidence.push(lease.provision_receipt_digest);
        self.view(observed_at)
    }

    /// Changes the active spend limit through its declared authority. A
    /// protocol-backed limit compiles a replacement budget intent and accepts
    /// it only after independent receipt verification; an app-backed limit
    /// never enters that path and remains explicitly app-enforced.
    ///
    /// # Errors
    ///
    /// Refuses conflicting idempotency keys, invalid limits, compiler or
    /// disclosure failures, unverified receipts, and malformed app evidence.
    pub fn change_limit(
        &mut self,
        registry: &ModuleRegistry,
        request: LimitChangeRequest,
        observed_at: u64,
    ) -> Result<AgentControlView, AgentControlError> {
        if let Some(completed) = self.completed_limit_changes.get(&request.idempotency_key) {
            return if completed.request == request {
                Ok(completed.view.clone())
            } else {
                Err(AgentControlError::IdempotencyConflict)
            };
        }
        if observed_at < self.updated_at {
            return Err(AgentControlError::NonMonotonicObservation);
        }
        let action_key = control_action_key(self.profile.agent_id, request);
        let evidence = match self.limit.enforcement {
            LimitEnforcement::Protocol => {
                self.change_protocol_limit(registry, request, action_key, observed_at)?
            }
            LimitEnforcement::App => self.change_app_limit(request, action_key)?,
        };
        self.limit.monthly = request.monthly_limit;
        self.limit.evidence_digest = evidence.digest;
        self.limit.verification_level = evidence.level;
        self.active_budget_id = evidence.replacement_budget.or(self.active_budget_id);
        self.updated_at = observed_at;
        let view = self.view(observed_at)?;
        self.completed_limit_changes.insert(
            request.idempotency_key,
            CompletedLimitChange {
                request,
                view: view.clone(),
            },
        );
        Ok(view)
    }

    fn change_protocol_limit(
        &mut self,
        registry: &ModuleRegistry,
        request: LimitChangeRequest,
        action_key: [u8; 32],
        observed_at: u64,
    ) -> Result<LimitChangeEvidence, AgentControlError> {
        let previous = self
            .active_budget_id
            .ok_or(AgentControlError::EvidenceConflict)?;
        let replacement = digest(&[BUDGET_DOMAIN, &previous, &action_key]);
        let intent = Intent::v1(IntentKind::BudgetCreate(BudgetCreate::new(
            BudgetId::new(replacement),
            self.profile.owner.clone(),
            self.profile.budget_account.clone(),
            self.profile.asset,
            Amount::from_u128(request.monthly_limit),
            PeriodLength::new(self.profile.period_seconds)
                .map_err(|_| AgentControlError::InvalidProfile)?,
            RolloverPolicy::None,
            Amount::ZERO,
            self.profile.purpose,
            TimestampSeconds::from_u64(
                observed_at
                    .checked_add(self.profile.budget_lifetime_seconds)
                    .ok_or(AgentControlError::TimeOverflow)?,
            ),
        )?));
        let compiled = compile(&intent, registry)?;
        let disclosure = DisclosureCheck::verify(&intent, &compiled)?;
        let evidence = self.boundary.submit_protocol_limit(ProtocolAction {
            stage: CreationStage::BudgetCreation,
            action_key,
            intent,
            compiled,
            disclosure,
            custody_key: KeyId::new(format!("agent-{}", short_hex(&self.profile.agent_id)))
                .map_err(|_| AgentControlError::InvalidProfile)?,
            started_at: observed_at,
        })?;
        if evidence.action_key != action_key || evidence.activity_id != action_key {
            return Err(AgentControlError::EvidenceConflict);
        }
        let verified = verify(&evidence.receipt_bytes, &evidence.authorized_batch)?;
        let protocol = verified
            .receipt()
            .protocol()
            .ok_or(AgentControlError::EvidenceConflict)?;
        if protocol.activity_id() != action_key
            || protocol.module_id() != ModuleId::Budget as u16
            || protocol.operation() != 1
        {
            return Err(AgentControlError::EvidenceConflict);
        }
        if evidence.verification_level < VerificationLevel::CHECKPOINT_FINALISED
            || evidence.verification_level < verified.level()
        {
            return Err(AgentControlError::EvidenceConflict);
        }
        Ok(LimitChangeEvidence {
            digest: Sha256::digest(verified.canonical_bytes()).into(),
            level: evidence.verification_level,
            replacement_budget: Some(replacement),
        })
    }

    fn change_app_limit(
        &mut self,
        request: LimitChangeRequest,
        action_key: [u8; 32],
    ) -> Result<LimitChangeEvidence, AgentControlError> {
        let evidence = self.boundary.apply_app_limit(AppLimitRequest {
            action_key,
            agent_id: self.profile.agent_id,
            asset: self.profile.asset.bytes(),
            ceiling: request.monthly_limit,
        })?;
        if evidence.action_key != action_key
            || evidence.agent_id != self.profile.agent_id
            || evidence.asset != self.profile.asset.bytes()
            || evidence.ceiling != request.monthly_limit
            || evidence.observed_sequence == 0
            || evidence.configuration_digest != evidence.expected_digest()
        {
            return Err(AgentControlError::EvidenceConflict);
        }
        Ok(LimitChangeEvidence {
            digest: evidence.configuration_digest,
            level: VerificationLevel::UNVERIFIED,
            replacement_budget: None,
        })
    }

    fn build_view(&self, fresh_until: u64) -> AgentControlView {
        let paused = self.state == AgentControlState::Paused;
        AgentControlView {
            agent_id: self.profile.agent_id,
            state: self.state,
            state_copy_key: self.state.copy_key(),
            pause_consequence_copy_key: paused.then_some(PAUSE_CONSEQUENCE_COPY_KEY),
            pause_consequence: paused.then_some(PAUSE_CONSEQUENCE),
            limit: self.limit.clone(),
            updated_at: self.updated_at,
            fresh_until,
            authority_evidence: self.authority_evidence.clone(),
        }
    }
}

fn control_action_key(agent_id: [u8; 32], request: LimitChangeRequest) -> [u8; 32] {
    digest(&[
        ACTION_DOMAIN,
        &agent_id,
        &request.idempotency_key,
        &request.monthly_limit.to_be_bytes(),
    ])
}

fn digest(parts: &[&[u8]]) -> [u8; 32] {
    let mut hasher = Sha256::new();
    for part in parts {
        hasher.update(part);
    }
    hasher.finalize().into()
}

/// Typed control refusal.
#[derive(Debug)]
pub enum AgentControlError {
    InvalidProfile,
    InvalidLimit,
    AuthorityMissing,
    EvidenceConflict,
    IdempotencyConflict,
    NonMonotonicObservation,
    TimeOverflow,
    Stale {
        last_verified_at: u64,
        freshness_seconds: u64,
    },
    Session(SessionKeyError),
    Agent(AgentFailure),
    Intent(IntentError),
    Compile(CompileError),
    Disclosure(DisclosureCheckError),
    Receipt(VerificationFailure),
}

impl Display for AgentControlError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::InvalidProfile => formatter.write_str("agent control profile is invalid"),
            Self::InvalidLimit => formatter.write_str("agent spend limit is invalid"),
            Self::AuthorityMissing => formatter.write_str("agent operating authority is missing"),
            Self::EvidenceConflict => formatter.write_str("agent control evidence did not match"),
            Self::IdempotencyConflict => formatter.write_str("limit request key was reused"),
            Self::NonMonotonicObservation => {
                formatter.write_str("agent observation moved backwards")
            }
            Self::TimeOverflow => formatter.write_str("agent control time overflowed"),
            Self::Stale {
                last_verified_at,
                freshness_seconds,
            } => write!(
                formatter,
                "agent state is stale after {last_verified_at} plus {freshness_seconds} seconds"
            ),
            Self::Session(error) => write!(formatter, "{error}"),
            Self::Agent(AgentFailure::Unavailable) => {
                formatter.write_str("agent contract is unavailable")
            }
            Self::Agent(AgentFailure::Refused(reason)) => {
                write!(formatter, "agent contract refused: {reason}")
            }
            Self::Intent(error) => write!(formatter, "intent refused: {error:?}"),
            Self::Compile(error) => write!(formatter, "intent compilation refused: {error:?}"),
            Self::Disclosure(error) => write!(formatter, "intent disclosure refused: {error:?}"),
            Self::Receipt(error) => write!(formatter, "receipt verification failed: {error:?}"),
        }
    }
}

impl std::error::Error for AgentControlError {}

impl From<SessionKeyError> for AgentControlError {
    fn from(value: SessionKeyError) -> Self {
        Self::Session(value)
    }
}

impl From<AgentFailure> for AgentControlError {
    fn from(value: AgentFailure) -> Self {
        Self::Agent(value)
    }
}

impl From<IntentError> for AgentControlError {
    fn from(value: IntentError) -> Self {
        Self::Intent(value)
    }
}

impl From<CompileError> for AgentControlError {
    fn from(value: CompileError) -> Self {
        Self::Compile(value)
    }
}

impl From<DisclosureCheckError> for AgentControlError {
    fn from(value: DisclosureCheckError) -> Self {
        Self::Disclosure(value)
    }
}

impl From<VerificationFailure> for AgentControlError {
    fn from(value: VerificationFailure) -> Self {
        Self::Receipt(value)
    }
}

fn short_hex(bytes: &[u8; 32]) -> String {
    const DIGITS: &[u8; 16] = b"0123456789abcdef";
    bytes[..8]
        .iter()
        .flat_map(|byte| {
            [
                DIGITS[(byte >> 4) as usize] as char,
                DIGITS[(byte & 15) as usize] as char,
            ]
        })
        .collect()
}
