//! Durable, receipt-gated human identity onboarding.

use std::fmt::{Display, Formatter};

use layerx_agent_api::error::RequestId;
use layerx_agent_api::idempotency::{BodyDigest, IdempotentMutation, Key};
use layerx_agent_api::identity::{AgentDid, AuthorityRef, ContractError};
use layerx_agent_api::prepare::{IdempotencyRef, PayloadBytes, PrepareRequest, TimestampBound};
use layerx_agent_api::track::TrackedSubmission;
use layerx_agent_api::{Amount, Sequence, SubmissionState, TimestampSeconds};
use layerx_intents::{
    compile, CompileError, DidRegistration, DisclosureCheck, DisclosureCheckError, Intent,
    IntentError, IntentKind, RecoveryRegistration,
};
use layerx_proof::receipt::{verify, AuthorizedBatch, VerificationFailure};
use layerx_sdk::{Call, Client as AgentClient, SdkError, SubmissionOutcome};
use layerx_types::ids::Did;
use layerx_types::intent::{ApprovalThreshold, PublicKey, RecoveryRoot};
use layerx_types::payload::ModuleRegistry;
use serde::{Deserialize, Serialize};
use sha2::{Digest as _, Sha256};

use crate::audit::{AuditChain, AuditError, AuditEvent, IdentityEvent};
use crate::custody::{CustodyError, KeyClass, KeyId, Keystore, Operation as CustodyOperation};
use crate::journeys::{JourneyEngine, JourneyKind, JourneyLeg, JourneyPlan};
use crate::notify::JourneyId;
use crate::store::{EvidenceRef, PrincipalScope, RowKey, StoreError, Table};
use crate::trace::TraceId;

const JOURNEY_ROW: &str = "onboarding-journey";
const APPLICATION_EVIDENCE_ROW: &str = "onboarding-application-identity";
const KEY_EVIDENCE_ROW: &str = "onboarding-custody-key";
const DID_RECEIPT_ROW: &str = "onboarding-did-receipt";
const RECOVERY_RECEIPT_ROW: &str = "onboarding-recovery-receipt";
const PRIMARY_KEY_ID: &str = "human-primary";
const RECORD_VERSION: u8 = 1;
const APPLICATION_EVIDENCE_DOMAIN: &[u8] = b"layerx-human-onboarding-application/v1";
const ACTION_KEY_DOMAIN: &[u8] = b"layerx-human-onboarding-action/v1";
const PREPARE_DIGEST_DOMAIN: &[u8] = b"layerx-human-agent-prepare/v1";
const DID_OPERATION: u8 = 1;
const RECOVERY_OPERATION: u8 = 3;
const TEXT_LIMIT: usize = 256;

/// The two protocol mutations in the onboarding journey.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub enum ProtocolStage {
    DidRegistration,
    RecoveryRegistration,
}

impl ProtocolStage {
    const fn code(self) -> u8 {
        match self {
            Self::DidRegistration => 1,
            Self::RecoveryRegistration => 2,
        }
    }

    const fn expected_operation(self) -> u8 {
        match self {
            Self::DidRegistration => DID_OPERATION,
            Self::RecoveryRegistration => RECOVERY_OPERATION,
        }
    }
}

/// Every durable onboarding stage, including local and protocol work.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum OnboardingStage {
    ApplicationIdentity,
    CustodyKey,
    DidRegistration,
    RecoveryRegistration,
}

/// The upstream dependency that prevented a queued action from advancing.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub enum Dependency {
    AgentLayer,
    Core,
}

/// Honest state of one onboarding stage. Local completion is deliberately
/// distinct from receipt verification.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum StageState {
    GettingReady,
    LocalComplete,
    Queued { unavailable: Option<Dependency> },
    Sending,
    Processing,
    StillChecking,
    ReceiptVerified,
    Refused { result_code: Option<i32> },
}

/// Evidence class exposed to Technical details.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum EvidenceClass {
    LocalJourneyState,
    CustodyKey,
    LayerxReceipt,
    SubmissionRecord,
}

/// The exact verification claim attached to an evidence reference.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum EvidenceVerification {
    Unverified,
    ReceiptVerified,
}

/// A stable reference to evidence persisted in the principal store.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct EvidencePointer {
    row: RowKey,
    class: EvidenceClass,
    verification: EvidenceVerification,
}

impl EvidencePointer {
    /// Returns the durable evidence identifier.
    #[must_use]
    pub const fn row(&self) -> &RowKey {
        &self.row
    }

    /// Returns the evidence class.
    #[must_use]
    pub const fn class(&self) -> EvidenceClass {
        self.class
    }

    /// Returns only the level established by local verification.
    #[must_use]
    pub const fn verification(&self) -> EvidenceVerification {
        self.verification
    }
}

/// Public state for one fixed onboarding stage.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct StageStatus {
    stage: OnboardingStage,
    state: StageState,
    evidence: Vec<EvidencePointer>,
}

impl StageStatus {
    #[must_use]
    pub const fn stage(&self) -> OnboardingStage {
        self.stage
    }

    #[must_use]
    pub const fn state(&self) -> StageState {
        self.state
    }

    #[must_use]
    pub fn evidence(&self) -> &[EvidencePointer] {
        &self.evidence
    }
}

/// Overall journey state. `ActiveRecoveryPending` means the DID receipt has
/// made the account active, while required recovery registration is unfinished.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum OnboardingState {
    GettingReady,
    Queued,
    Sending,
    Processing,
    StillChecking,
    ActiveRecoveryPending,
    Complete,
    Refused,
}

/// Receipt-gated public projection of the durable journey.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct OnboardingStatus {
    state: OnboardingState,
    account_active: bool,
    recovery_challenge_delay_secs: u64,
    stages: Vec<StageStatus>,
}

impl OnboardingStatus {
    #[must_use]
    pub const fn state(&self) -> OnboardingState {
        self.state
    }

    /// This can become true only after the DID receipt passes proof verification.
    #[must_use]
    pub const fn account_active(&self) -> bool {
        self.account_active
    }

    /// Protocol configuration that will delay any later recovery exercise.
    #[must_use]
    pub const fn recovery_challenge_delay_secs(&self) -> u64 {
        self.recovery_challenge_delay_secs
    }

    #[must_use]
    pub fn stages(&self) -> &[StageStatus] {
        &self.stages
    }
}

/// Required recovery policy registered as part of onboarding.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct RecoveryPolicy {
    root: RecoveryRoot,
    threshold: ApprovalThreshold,
    challenge_delay_secs: u64,
}

impl RecoveryPolicy {
    /// Constructs a non-empty recovery policy with an explicit later-exercise delay.
    ///
    /// # Errors
    ///
    /// Refuses a zero recovery commitment or zero challenge delay.
    pub fn new(
        root: RecoveryRoot,
        threshold: ApprovalThreshold,
        challenge_delay_secs: u64,
    ) -> Result<Self, OnboardingError> {
        if root.is_zero() || challenge_delay_secs == 0 {
            return Err(OnboardingError::InvalidRecoveryPolicy);
        }
        Ok(Self {
            root,
            threshold,
            challenge_delay_secs,
        })
    }

    #[must_use]
    pub const fn root(self) -> RecoveryRoot {
        self.root
    }

    #[must_use]
    pub const fn threshold(self) -> ApprovalThreshold {
        self.threshold
    }

    #[must_use]
    pub const fn challenge_delay_secs(self) -> u64 {
        self.challenge_delay_secs
    }
}

/// Stable inputs for the one onboarding journey owned by a principal.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct OnboardingStart {
    idempotency_key: [u8; 32],
    did: Did,
    recovery: RecoveryPolicy,
}

impl OnboardingStart {
    /// Binds one non-zero caller idempotency key to one DID and recovery policy.
    ///
    /// # Errors
    ///
    /// Refuses the reserved all-zero idempotency key.
    pub fn new(
        idempotency_key: [u8; 32],
        did: Did,
        recovery: RecoveryPolicy,
    ) -> Result<Self, OnboardingError> {
        if idempotency_key == [0; 32] {
            return Err(OnboardingError::InvalidIdempotencyKey);
        }
        Ok(Self {
            idempotency_key,
            did,
            recovery,
        })
    }
}

/// Core-produced values needed by the agent contract's existing prepare call.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AgentPreparationContext {
    actor: AgentDid,
    authority: AuthorityRef,
    account_sequence: u64,
    not_before: u64,
    not_after: u64,
    fee_limit: u128,
}

impl AgentPreparationContext {
    /// Creates complete core-produced preparation inputs for the agent contract.
    ///
    /// # Errors
    ///
    /// Refuses oversize labels and an inverted timestamp bound.
    pub fn new(
        actor: AgentDid,
        authority: AuthorityRef,
        account_sequence: u64,
        not_before: u64,
        not_after: u64,
        fee_limit: u128,
    ) -> Result<Self, OnboardingError> {
        if actor.as_str().len() > TEXT_LIMIT
            || authority.as_str().len() > TEXT_LIMIT
            || not_after < not_before
        {
            return Err(OnboardingError::InvalidAgentContext);
        }
        Ok(Self {
            actor,
            authority,
            account_sequence,
            not_before,
            not_after,
            fee_limit,
        })
    }
}

/// One typed onboarding intent wrapped in the existing agent prepare contract.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AgentPrepareAction {
    stage: ProtocolStage,
    action_key: [u8; 32],
    intent: Intent,
    disclosure: DisclosureCheck,
    call: Call<IdempotentMutation<PrepareRequest>>,
}

impl AgentPrepareAction {
    #[must_use]
    pub const fn stage(&self) -> ProtocolStage {
        self.stage
    }

    #[must_use]
    pub const fn action_key(&self) -> [u8; 32] {
        self.action_key
    }

    #[must_use]
    pub const fn intent(&self) -> &Intent {
        &self.intent
    }

    /// Independently decoded payload evidence retained for the signer boundary.
    #[must_use]
    pub const fn disclosure(&self) -> &DisclosureCheck {
        &self.disclosure
    }

    #[must_use]
    pub const fn call(&self) -> &Call<IdempotentMutation<PrepareRequest>> {
        &self.call
    }
}

/// Agent/core result used to advance one durable protocol stage.
pub enum AgentStageOutcome<'a> {
    Unavailable {
        dependency: Dependency,
    },
    Tracked {
        submission: &'a TrackedSubmission,
    },
    Executed {
        submission: &'a TrackedSubmission,
        activity_id: [u8; 32],
        receipt_bytes: &'a [u8],
        authorised_batch: &'a AuthorizedBatch,
    },
}

/// One update is bound to the exact deterministic action key it answers for.
pub struct AgentStageUpdate<'a> {
    pub stage: ProtocolStage,
    pub action_key: [u8; 32],
    pub outcome: AgentStageOutcome<'a>,
}

/// Material returned under Technical details and included by audit export.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ActivationEvidence {
    event: IdentityEvent,
    pointer: EvidencePointer,
    canonical_receipt: Vec<u8>,
    recovery_challenge_delay_secs: Option<u64>,
}

impl ActivationEvidence {
    #[must_use]
    pub const fn event(&self) -> IdentityEvent {
        self.event
    }

    #[must_use]
    pub const fn pointer(&self) -> &EvidencePointer {
        &self.pointer
    }

    #[must_use]
    pub fn canonical_receipt(&self) -> &[u8] {
        &self.canonical_receipt
    }

    #[must_use]
    pub const fn recovery_challenge_delay_secs(&self) -> Option<u64> {
        self.recovery_challenge_delay_secs
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
enum ProtocolState {
    Queued,
    Sending,
    Processing,
    StillChecking,
    Verified,
    Refused,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
struct StoredPreparation {
    actor: String,
    authority: String,
    account_sequence: u64,
    not_before: u64,
    not_after: u64,
    fee_limit: u128,
}

impl From<&AgentPreparationContext> for StoredPreparation {
    fn from(value: &AgentPreparationContext) -> Self {
        Self {
            actor: value.actor.as_str().to_owned(),
            authority: value.authority.as_str().to_owned(),
            account_sequence: value.account_sequence,
            not_before: value.not_before,
            not_after: value.not_after,
            fee_limit: value.fee_limit,
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
struct ProtocolRecord {
    state: ProtocolState,
    unavailable: Option<Dependency>,
    preparation: Option<StoredPreparation>,
    submission_ref: Option<String>,
    receipt_digest: Option<[u8; 32]>,
    activity_id: Option<[u8; 32]>,
    refusal_code: Option<i32>,
}

impl ProtocolRecord {
    const fn queued() -> Self {
        Self {
            state: ProtocolState::Queued,
            unavailable: None,
            preparation: None,
            submission_ref: None,
            receipt_digest: None,
            activity_id: None,
            refusal_code: None,
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
struct JourneyRecord {
    version: u8,
    idempotency_key: [u8; 32],
    did: Vec<u8>,
    recovery_root: [u8; 32],
    recovery_threshold: u16,
    recovery_challenge_delay_secs: u64,
    started_at: u64,
    updated_at: u64,
    public_key: Option<[u8; 32]>,
    did_registration: ProtocolRecord,
    recovery_registration: ProtocolRecord,
}

/// Durable onboarding state machine. It stores no private key material and has
/// no direct node path: protocol work leaves only as typed agent-contract calls.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct OnboardingJourney {
    record: JourneyRecord,
}

impl OnboardingJourney {
    #[allow(clippy::too_many_arguments)]
    pub fn start_durable_engine(
        &self,
        scope: &mut PrincipalScope<'_>,
        registry: &ModuleRegistry,
        actor: AgentDid,
        authority: AuthorityRef,
        account_sequence: u64,
        not_before: u64,
        not_after: u64,
        fee_limit: u128,
        now: u64,
    ) -> Result<JourneyEngine, OnboardingError> {
        let first_key = self.action_key(ProtocolStage::DidRegistration);
        let second_key = self.action_key(ProtocolStage::RecoveryRegistration);
        let legs = vec![
            JourneyLeg::new(
                self.intent(ProtocolStage::DidRegistration)?,
                first_key,
                actor.clone(),
                authority.clone(),
                account_sequence,
                not_before,
                not_after,
                fee_limit,
            )
            .map_err(|_| OnboardingError::InvalidAgentContext)?,
            JourneyLeg::new(
                self.intent(ProtocolStage::RecoveryRegistration)?,
                second_key,
                actor,
                authority,
                account_sequence
                    .checked_add(1)
                    .ok_or(OnboardingError::InvalidAgentContext)?,
                not_before,
                not_after,
                fee_limit,
            )
            .map_err(|_| OnboardingError::InvalidAgentContext)?,
        ];
        let journey_id = JourneyId::new(format!("jrn_{}", hex(&self.record.idempotency_key)))
            .map_err(|_| OnboardingError::InvalidIdempotencyKey)?;
        let plan = JourneyPlan::new(
            journey_id,
            JourneyKind::Onboarding,
            self.record.idempotency_key,
            primary_key_id()?,
            CustodyOperation::ProtocolMutation,
            legs,
        )
        .map_err(|_| OnboardingError::InvalidAgentContext)?;
        JourneyEngine::start(scope, &plan, registry, now)
            .map_err(|_| OnboardingError::InvalidAgentContext)
    }
    pub fn did(&self) -> Result<Did, OnboardingError> {
        Did::new(&self.record.did).map_err(|_| OnboardingError::CorruptJourney("invalid DID"))
    }
    /// Creates or returns the principal's one application identity journey.
    /// Repeating the same request converges; a conflicting request is refused.
    ///
    /// # Errors
    ///
    /// Refuses conflicting starts, corrupt existing state, or durable-store failures.
    pub fn start(
        scope: &mut PrincipalScope<'_>,
        request: &OnboardingStart,
        now: u64,
    ) -> Result<Self, OnboardingError> {
        if let Some(existing) = Self::load(scope)? {
            if existing.same_start(request) {
                return Ok(existing);
            }
            return Err(OnboardingError::IdempotencyConflict);
        }
        let record = JourneyRecord {
            version: RECORD_VERSION,
            idempotency_key: request.idempotency_key,
            did: request.did.as_bytes().to_vec(),
            recovery_root: request.recovery.root().bytes(),
            recovery_threshold: request.recovery.threshold().value(),
            recovery_challenge_delay_secs: request.recovery.challenge_delay_secs(),
            started_at: now,
            updated_at: now,
            public_key: None,
            did_registration: ProtocolRecord::queued(),
            recovery_registration: ProtocolRecord::queued(),
        };
        let mut journey = Self { record };
        let application = journey.application_evidence(scope);
        put_exact(scope, application_row()?, now, application.to_vec())?;
        journey.persist(scope, now)?;
        Ok(journey)
    }

    /// Loads and validates the principal's journey.
    ///
    /// # Errors
    ///
    /// Refuses corrupt state, missing application evidence, and storage failures.
    pub fn load(scope: &PrincipalScope<'_>) -> Result<Option<Self>, OnboardingError> {
        let Some(row) = scope.get(Table::Journeys, &journey_row()?) else {
            return Ok(None);
        };
        let record: JourneyRecord = serde_json::from_slice(row.bytes())
            .map_err(|_| OnboardingError::CorruptJourney("invalid journey encoding"))?;
        validate_record(&record)?;
        let journey = Self { record };
        let application = scope.get(Table::Journeys, &application_row()?).ok_or(
            OnboardingError::CorruptJourney("application identity evidence is missing"),
        )?;
        if application.bytes() != journey.application_evidence(scope) {
            return Err(OnboardingError::EvidenceConflict);
        }
        Ok(Some(journey))
    }

    /// Generates or rediscovers the stable human primary key. A crash after
    /// key persistence but before journey persistence converges through describe.
    ///
    /// # Errors
    ///
    /// Returns typed entropy, KMS, custody-record, evidence, and store failures.
    pub fn resume_local(
        &mut self,
        scope: &mut PrincipalScope<'_>,
        keystore: &Keystore,
        now: u64,
    ) -> Result<OnboardingStatus, OnboardingError> {
        let key_id = primary_key_id()?;
        let descriptor = match keystore.describe(scope.principal(), &key_id) {
            Ok(descriptor) => descriptor,
            Err(CustodyError::KeyNotFound) => {
                match keystore.create(scope.principal(), &key_id, KeyClass::HumanPrimary) {
                    Ok(public_key) => crate::custody::KeyDescriptor {
                        class: KeyClass::HumanPrimary,
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
        if descriptor.class != KeyClass::HumanPrimary {
            return Err(OnboardingError::CorruptJourney(
                "onboarding key has the wrong custody class",
            ));
        }
        if self
            .record
            .public_key
            .is_some_and(|stored| stored != descriptor.public_key)
        {
            return Err(OnboardingError::EvidenceConflict);
        }
        let key_evidence = encode_key_evidence(
            descriptor.public_key,
            keystore.network_id(),
            keystore.key_reference(),
        )?;
        put_exact(scope, key_evidence_row()?, now, key_evidence)?;
        self.record.public_key = Some(descriptor.public_key);
        self.record.updated_at = self.record.updated_at.max(now);
        self.persist(scope, now)?;
        Ok(self.status())
    }

    /// Returns the next eligible typed intent in the existing agent prepare
    /// contract. The first context is durable; duplicates always reproduce it.
    ///
    /// # Errors
    ///
    /// Refuses use before key generation and returns typed intent, disclosure,
    /// contract, or durable-store failures.
    pub fn prepare_agent_action(
        &mut self,
        scope: &mut PrincipalScope<'_>,
        registry: &ModuleRegistry,
        agent: &AgentClient,
        context: &AgentPreparationContext,
        now: u64,
    ) -> Result<Option<AgentPrepareAction>, OnboardingError> {
        let Some(stage) = self.next_action_stage()? else {
            return Ok(None);
        };
        let stored = self
            .stage(stage)
            .preparation
            .clone()
            .unwrap_or_else(|| context.into());
        if self.stage(stage).preparation.is_none() {
            self.stage_mut(stage).preparation = Some(stored.clone());
            self.record.updated_at = self.record.updated_at.max(now);
            self.persist(scope, now)?;
        }
        self.build_action(stage, registry, agent, &stored).map(Some)
    }

    /// Records an unavailable, pending, unknown, refused, or receipt-backed
    /// result from the agent contract without ever translating uncertainty to success.
    ///
    /// # Errors
    ///
    /// Refuses ineligible or mismatched updates, unverifiable receipts,
    /// conflicting evidence, audit failures, and durable-store failures.
    pub fn apply_agent_update(
        &mut self,
        scope: &mut PrincipalScope<'_>,
        audit: &mut AuditChain,
        trace: &TraceId,
        update: &AgentStageUpdate<'_>,
        now: u64,
    ) -> Result<OnboardingStatus, OnboardingError> {
        self.require_eligible(update.stage)?;
        if update.action_key != self.action_key(update.stage) {
            return Err(OnboardingError::ActionKeyMismatch);
        }
        match &update.outcome {
            AgentStageOutcome::Unavailable { dependency } => {
                let stage = self.stage_mut(update.stage);
                if stage.state != ProtocolState::Verified {
                    stage.state = ProtocolState::Queued;
                    stage.unavailable = Some(*dependency);
                }
                self.persist(scope, now)?;
            }
            AgentStageOutcome::Tracked { submission } => {
                self.apply_tracked(update.stage, submission)?;
                self.persist(scope, now)?;
            }
            AgentStageOutcome::Executed {
                submission,
                activity_id,
                receipt_bytes,
                authorised_batch,
            } => {
                self.apply_executed(
                    scope,
                    update.stage,
                    submission,
                    *activity_id,
                    receipt_bytes,
                    authorised_batch,
                    now,
                )?;
                self.ensure_activation_audit(scope, audit, trace, update.stage, now)?;
            }
        }
        Ok(self.status())
    }

    /// Returns the receipt material that established DID, key, and recovery
    /// activation. The DID receipt intentionally establishes both the DID and
    /// the primary key named by that registration.
    ///
    /// # Errors
    ///
    /// Refuses missing or altered evidence and durable-store failures.
    pub fn activation_evidence(
        &self,
        scope: &PrincipalScope<'_>,
    ) -> Result<Vec<ActivationEvidence>, OnboardingError> {
        let mut evidence = Vec::new();
        if self.did_verified() {
            let pointer = receipt_pointer(did_receipt_row()?);
            let bytes = evidence_bytes(scope, pointer.row())?;
            verify_stored_digest(&bytes, self.record.did_registration.receipt_digest)?;
            evidence.push(ActivationEvidence {
                event: IdentityEvent::DidRegistration,
                pointer: pointer.clone(),
                canonical_receipt: bytes.clone(),
                recovery_challenge_delay_secs: None,
            });
            evidence.push(ActivationEvidence {
                event: IdentityEvent::KeyActivation,
                pointer,
                canonical_receipt: bytes,
                recovery_challenge_delay_secs: None,
            });
        }
        if self.recovery_verified() {
            let pointer = receipt_pointer(recovery_receipt_row()?);
            let bytes = evidence_bytes(scope, pointer.row())?;
            verify_stored_digest(&bytes, self.record.recovery_registration.receipt_digest)?;
            evidence.push(ActivationEvidence {
                event: IdentityEvent::RecoveryRegistration,
                pointer,
                canonical_receipt: bytes,
                recovery_challenge_delay_secs: Some(self.record.recovery_challenge_delay_secs),
            });
        }
        Ok(evidence)
    }

    /// Projects durable internal state to an honest user-facing state matrix.
    #[must_use]
    pub fn status(&self) -> OnboardingStatus {
        let account_active = self.did_verified();
        OnboardingStatus {
            state: self.overall_state(),
            account_active,
            recovery_challenge_delay_secs: self.record.recovery_challenge_delay_secs,
            stages: vec![
                StageStatus {
                    stage: OnboardingStage::ApplicationIdentity,
                    state: StageState::LocalComplete,
                    evidence: vec![local_pointer(application_row_infallible())],
                },
                StageStatus {
                    stage: OnboardingStage::CustodyKey,
                    state: if self.record.public_key.is_some() {
                        StageState::LocalComplete
                    } else {
                        StageState::GettingReady
                    },
                    evidence: self.record.public_key.map_or_else(Vec::new, |_| {
                        vec![EvidencePointer {
                            row: key_evidence_row_infallible(),
                            class: EvidenceClass::CustodyKey,
                            verification: EvidenceVerification::Unverified,
                        }]
                    }),
                },
                self.protocol_status(
                    OnboardingStage::DidRegistration,
                    &self.record.did_registration,
                    did_receipt_row_infallible(),
                ),
                self.protocol_status(
                    OnboardingStage::RecoveryRegistration,
                    &self.record.recovery_registration,
                    recovery_receipt_row_infallible(),
                ),
            ],
        }
    }

    fn same_start(&self, request: &OnboardingStart) -> bool {
        self.record.idempotency_key == request.idempotency_key
            && self.record.did == request.did.as_bytes()
            && self.record.recovery_root == request.recovery.root().bytes()
            && self.record.recovery_threshold == request.recovery.threshold().value()
            && self.record.recovery_challenge_delay_secs == request.recovery.challenge_delay_secs()
    }

    fn application_evidence(&self, scope: &PrincipalScope<'_>) -> [u8; 32] {
        let mut digest = Sha256::new();
        digest.update(APPLICATION_EVIDENCE_DOMAIN);
        digest.update(scope.principal().as_str().as_bytes());
        digest.update(self.record.idempotency_key);
        digest.update(&self.record.did);
        digest.finalize().into()
    }

    fn persist(&mut self, scope: &mut PrincipalScope<'_>, now: u64) -> Result<(), OnboardingError> {
        self.record.updated_at = self.record.updated_at.max(now);
        let bytes = serde_json::to_vec(&self.record)
            .map_err(|_| OnboardingError::CorruptJourney("journey cannot be encoded"))?;
        scope.put(Table::Journeys, journey_row()?, now, bytes)?;
        Ok(())
    }

    fn stage(&self, stage: ProtocolStage) -> &ProtocolRecord {
        match stage {
            ProtocolStage::DidRegistration => &self.record.did_registration,
            ProtocolStage::RecoveryRegistration => &self.record.recovery_registration,
        }
    }

    fn stage_mut(&mut self, stage: ProtocolStage) -> &mut ProtocolRecord {
        match stage {
            ProtocolStage::DidRegistration => &mut self.record.did_registration,
            ProtocolStage::RecoveryRegistration => &mut self.record.recovery_registration,
        }
    }

    fn did_verified(&self) -> bool {
        self.record.did_registration.state == ProtocolState::Verified
    }

    fn recovery_verified(&self) -> bool {
        self.record.recovery_registration.state == ProtocolState::Verified
    }

    fn next_action_stage(&self) -> Result<Option<ProtocolStage>, OnboardingError> {
        if self.record.public_key.is_none() {
            return Err(OnboardingError::CustodyKeyRequired);
        }
        if self.record.did_registration.state != ProtocolState::Verified {
            return Ok(
                (self.record.did_registration.state == ProtocolState::Queued)
                    .then_some(ProtocolStage::DidRegistration),
            );
        }
        Ok(
            (self.record.recovery_registration.state == ProtocolState::Queued)
                .then_some(ProtocolStage::RecoveryRegistration),
        )
    }

    fn require_eligible(&self, stage: ProtocolStage) -> Result<(), OnboardingError> {
        if self.record.public_key.is_none()
            || stage == ProtocolStage::RecoveryRegistration && !self.did_verified()
        {
            return Err(OnboardingError::StageNotEligible);
        }
        if self.stage(stage).preparation.is_none() {
            return Err(OnboardingError::ActionNotPrepared);
        }
        Ok(())
    }

    fn action_key(&self, stage: ProtocolStage) -> [u8; 32] {
        let mut digest = Sha256::new();
        digest.update(ACTION_KEY_DOMAIN);
        digest.update(self.record.idempotency_key);
        digest.update([stage.code()]);
        digest.update(&self.record.did);
        digest.finalize().into()
    }

    fn intent(&self, stage: ProtocolStage) -> Result<Intent, OnboardingError> {
        let did = Did::new(&self.record.did)
            .map_err(|_| OnboardingError::CorruptJourney("stored DID is invalid"))?;
        let kind = match stage {
            ProtocolStage::DidRegistration => {
                let public_key = self
                    .record
                    .public_key
                    .ok_or(OnboardingError::CustodyKeyRequired)?;
                IntentKind::DidRegistration(DidRegistration::new(did, PublicKey::new(public_key))?)
            }
            ProtocolStage::RecoveryRegistration => {
                let threshold = ApprovalThreshold::new(self.record.recovery_threshold)
                    .map_err(|_| OnboardingError::CorruptJourney("stored threshold is invalid"))?;
                IntentKind::RecoveryRegistration(RecoveryRegistration::new(
                    did,
                    RecoveryRoot::new(self.record.recovery_root),
                    threshold,
                )?)
            }
        };
        Ok(Intent::v1(kind))
    }

    fn build_action(
        &self,
        stage: ProtocolStage,
        registry: &ModuleRegistry,
        agent: &AgentClient,
        stored: &StoredPreparation,
    ) -> Result<AgentPrepareAction, OnboardingError> {
        let intent = self.intent(stage)?;
        let compiled = compile(&intent, registry)?;
        let disclosure = DisclosureCheck::verify(&intent, &compiled)?;
        let action_key = self.action_key(stage);
        let request = PrepareRequest {
            protocol_activity_type: compiled.activity_type().value(),
            actor: AgentDid::new(stored.actor.clone())?,
            authority: AuthorityRef::new(stored.authority.clone())?,
            account_sequence: Sequence(stored.account_sequence),
            timestamp_bound: TimestampBound {
                not_before: TimestampSeconds(stored.not_before),
                not_after: TimestampSeconds(stored.not_after),
            }
            .validate()?,
            idempotency_key: IdempotencyRef::new(hex(&action_key))?,
            fee_limit: Amount(stored.fee_limit),
            payload: PayloadBytes::new(disclosure.canonical_payload().to_vec())?,
            payload_hash: disclosure.payload_hash(),
        };
        let body_digest = prepare_body_digest(&request);
        let mut request_bytes = [0_u8; 8];
        request_bytes.copy_from_slice(&action_key[..8]);
        let mutation = IdempotentMutation {
            request_id: RequestId(u64::from_be_bytes(request_bytes)),
            key: Key::new(action_key)?,
            body_digest: BodyDigest(body_digest),
            operation: request,
        };
        Ok(AgentPrepareAction {
            stage,
            action_key,
            intent,
            disclosure,
            call: agent.prepare(mutation),
        })
    }

    fn apply_tracked(
        &mut self,
        stage: ProtocolStage,
        submission: &TrackedSubmission,
    ) -> Result<(), OnboardingError> {
        let record = self.stage_mut(stage);
        bind_submission(record, submission)?;
        record.unavailable = None;
        match &submission.state {
            SubmissionState::Prepared
            | SubmissionState::Signed
            | SubmissionState::Queued
            | SubmissionState::Submitted => record.state = ProtocolState::Sending,
            SubmissionState::Acknowledged => record.state = ProtocolState::Processing,
            SubmissionState::Unknown => record.state = ProtocolState::StillChecking,
            SubmissionState::Executed { .. } => return Err(OnboardingError::ReceiptRequired),
            SubmissionState::Failed { result } => {
                record.state = ProtocolState::Refused;
                record.refusal_code = Some(result.raw());
            }
            SubmissionState::Expired => {
                record.state = ProtocolState::Refused;
                record.refusal_code = None;
            }
        }
        Ok(())
    }

    #[allow(clippy::too_many_arguments)]
    fn apply_executed(
        &mut self,
        scope: &mut PrincipalScope<'_>,
        stage: ProtocolStage,
        submission: &TrackedSubmission,
        expected_activity_id: [u8; 32],
        receipt_bytes: &[u8],
        authorised_batch: &AuthorizedBatch,
        now: u64,
    ) -> Result<(), OnboardingError> {
        match AgentClient::submission_outcome(submission.clone())? {
            SubmissionOutcome::Executed(_) => {}
            _ => return Err(OnboardingError::ReceiptRequired),
        }
        let verified = verify(receipt_bytes, authorised_batch)?;
        let protocol = verified
            .receipt()
            .protocol()
            .ok_or(OnboardingError::ReceiptShape)?;
        if protocol.activity_id() != expected_activity_id {
            return Err(OnboardingError::ReceiptActivityMismatch);
        }
        if protocol.operation() != stage.expected_operation() {
            return Err(OnboardingError::ReceiptOperationMismatch);
        }
        let digest: [u8; 32] = Sha256::digest(verified.canonical_bytes()).into();
        let other = match stage {
            ProtocolStage::DidRegistration => &self.record.recovery_registration,
            ProtocolStage::RecoveryRegistration => &self.record.did_registration,
        };
        if other.receipt_digest == Some(digest) || other.activity_id == Some(expected_activity_id) {
            return Err(OnboardingError::ReceiptReuse);
        }
        {
            let record = self.stage_mut(stage);
            bind_submission(record, submission)?;
            if record.receipt_digest.is_some_and(|stored| stored != digest)
                || record
                    .activity_id
                    .is_some_and(|stored| stored != expected_activity_id)
            {
                return Err(OnboardingError::EvidenceConflict);
            }
        }
        let row = receipt_row(stage)?;
        put_exact(scope, row, now, verified.canonical_bytes().to_vec())?;
        let record = self.stage_mut(stage);
        record.state = ProtocolState::Verified;
        record.unavailable = None;
        record.receipt_digest = Some(digest);
        record.activity_id = Some(expected_activity_id);
        record.refusal_code = None;
        self.persist(scope, now)
    }

    fn ensure_activation_audit(
        &self,
        scope: &mut PrincipalScope<'_>,
        audit: &mut AuditChain,
        trace: &TraceId,
        stage: ProtocolStage,
        now: u64,
    ) -> Result<(), OnboardingError> {
        let digest = self
            .stage(stage)
            .receipt_digest
            .ok_or(OnboardingError::ReceiptRequired)?;
        match stage {
            ProtocolStage::DidRegistration => {
                ensure_identity_event(
                    scope,
                    audit,
                    trace,
                    now,
                    IdentityEvent::DidRegistration,
                    digest,
                    &[EvidenceRef::new(Table::Journeys, did_receipt_row()?)],
                )?;
                ensure_identity_event(
                    scope,
                    audit,
                    trace,
                    now,
                    IdentityEvent::KeyActivation,
                    digest,
                    &[
                        EvidenceRef::new(Table::Journeys, key_evidence_row()?),
                        EvidenceRef::new(Table::Journeys, did_receipt_row()?),
                    ],
                )?;
            }
            ProtocolStage::RecoveryRegistration => ensure_identity_event(
                scope,
                audit,
                trace,
                now,
                IdentityEvent::RecoveryRegistration,
                digest,
                &[EvidenceRef::new(Table::Journeys, recovery_receipt_row()?)],
            )?,
        }
        Ok(())
    }

    fn protocol_status(
        &self,
        milestone: OnboardingStage,
        record: &ProtocolRecord,
        receipt_row: RowKey,
    ) -> StageStatus {
        let view_state =
            if milestone == OnboardingStage::RecoveryRegistration && !self.did_verified() {
                StageState::GettingReady
            } else {
                match record.state {
                    ProtocolState::Queued => StageState::Queued {
                        unavailable: record.unavailable,
                    },
                    ProtocolState::Sending => StageState::Sending,
                    ProtocolState::Processing => StageState::Processing,
                    ProtocolState::StillChecking => StageState::StillChecking,
                    ProtocolState::Verified => StageState::ReceiptVerified,
                    ProtocolState::Refused => StageState::Refused {
                        result_code: record.refusal_code,
                    },
                }
            };
        let evidence = if record.state == ProtocolState::Verified {
            vec![receipt_pointer(receipt_row)]
        } else if record.submission_ref.is_some() {
            vec![EvidencePointer {
                row: receipt_row,
                class: EvidenceClass::SubmissionRecord,
                verification: EvidenceVerification::Unverified,
            }]
        } else {
            Vec::new()
        };
        StageStatus {
            stage: milestone,
            state: view_state,
            evidence,
        }
    }

    fn overall_state(&self) -> OnboardingState {
        if self.record.public_key.is_none() {
            return OnboardingState::GettingReady;
        }
        if self.recovery_verified() {
            return OnboardingState::Complete;
        }
        let current = if self.did_verified() {
            &self.record.recovery_registration
        } else {
            &self.record.did_registration
        };
        match current.state {
            ProtocolState::Queued if self.did_verified() => OnboardingState::ActiveRecoveryPending,
            ProtocolState::Queued => OnboardingState::Queued,
            ProtocolState::Sending => OnboardingState::Sending,
            ProtocolState::Processing => OnboardingState::Processing,
            ProtocolState::StillChecking => OnboardingState::StillChecking,
            ProtocolState::Verified => OnboardingState::ActiveRecoveryPending,
            ProtocolState::Refused => OnboardingState::Refused,
        }
    }
}

fn validate_record(record: &JourneyRecord) -> Result<(), OnboardingError> {
    if record.version != RECORD_VERSION
        || record.idempotency_key == [0; 32]
        || Did::new(&record.did).is_err()
        || RecoveryRoot::new(record.recovery_root).is_zero()
        || ApprovalThreshold::new(record.recovery_threshold).is_err()
        || record.recovery_challenge_delay_secs == 0
        || record.updated_at < record.started_at
        || record
            .public_key
            .is_some_and(|public_key| public_key == [0; 32])
    {
        return Err(OnboardingError::CorruptJourney(
            "journey invariants are invalid",
        ));
    }
    validate_protocol_record(&record.did_registration)?;
    validate_protocol_record(&record.recovery_registration)?;
    if record.did_registration.state != ProtocolState::Verified
        && record.recovery_registration.state == ProtocolState::Verified
    {
        return Err(OnboardingError::CorruptJourney(
            "recovery verified before DID registration",
        ));
    }
    Ok(())
}

fn validate_protocol_record(record: &ProtocolRecord) -> Result<(), OnboardingError> {
    if record.state == ProtocolState::Verified
        && (record.receipt_digest.is_none()
            || record.activity_id.is_none()
            || record.submission_ref.is_none())
        || record.state != ProtocolState::Verified
            && (record.receipt_digest.is_some() || record.activity_id.is_some())
        || record
            .submission_ref
            .as_ref()
            .is_some_and(|value| value.is_empty() || value.len() > TEXT_LIMIT)
        || record.unavailable.is_some() && record.state != ProtocolState::Queued
    {
        return Err(OnboardingError::CorruptJourney(
            "protocol stage invariants are invalid",
        ));
    }
    Ok(())
}

fn bind_submission(
    record: &mut ProtocolRecord,
    submission: &TrackedSubmission,
) -> Result<(), OnboardingError> {
    let submitted = submission.submission_ref.as_str();
    if submitted.len() > TEXT_LIMIT
        || record
            .submission_ref
            .as_ref()
            .is_some_and(|stored| stored != submitted)
    {
        return Err(OnboardingError::SubmissionConflict);
    }
    record.submission_ref = Some(submitted.to_owned());
    Ok(())
}

fn ensure_identity_event(
    scope: &mut PrincipalScope<'_>,
    audit: &mut AuditChain,
    trace: &TraceId,
    now: u64,
    identity_event: IdentityEvent,
    receipt_digest: [u8; 32],
    evidence: &[EvidenceRef],
) -> Result<(), OnboardingError> {
    let entries = audit.entries(scope)?;
    for entry in entries {
        if let AuditEvent::IdentityLifecycle {
            event,
            receipt_digest: existing,
        } = entry.event()
        {
            if *event == identity_event {
                return if *existing == receipt_digest {
                    Ok(())
                } else {
                    Err(OnboardingError::AuditConflict)
                };
            }
        }
    }
    audit.append(
        scope,
        now,
        trace,
        &AuditEvent::IdentityLifecycle {
            event: identity_event,
            receipt_digest,
        },
        evidence,
    )?;
    Ok(())
}

fn prepare_body_digest(request: &PrepareRequest) -> [u8; 32] {
    let mut digest = Sha256::new();
    digest.update(PREPARE_DIGEST_DOMAIN);
    digest.update(request.protocol_activity_type.to_be_bytes());
    hash_text(&mut digest, request.actor.as_str());
    hash_text(&mut digest, request.authority.as_str());
    digest.update(request.account_sequence.get().to_be_bytes());
    digest.update(request.timestamp_bound.not_before.get().to_be_bytes());
    digest.update(request.timestamp_bound.not_after.get().to_be_bytes());
    hash_text(&mut digest, request.idempotency_key.as_str());
    digest.update(request.fee_limit.get().to_be_bytes());
    digest.update(request.payload_hash);
    digest.update(Sha256::digest(request.payload.as_bytes()));
    digest.finalize().into()
}

fn hash_text(digest: &mut Sha256, value: &str) {
    let length = u32::try_from(value.len()).unwrap_or(u32::MAX);
    digest.update(length.to_be_bytes());
    digest.update(value.as_bytes());
}

fn encode_key_evidence(
    public_key: [u8; 32],
    network_id: u32,
    key_reference: &str,
) -> Result<Vec<u8>, OnboardingError> {
    let reference_length = u16::try_from(key_reference.len())
        .map_err(|_| OnboardingError::CorruptJourney("KMS reference is too long"))?;
    let mut bytes = Vec::with_capacity(43_usize.saturating_add(key_reference.len()));
    bytes.extend_from_slice(b"LXOK");
    bytes.push(1);
    bytes.extend_from_slice(&network_id.to_be_bytes());
    bytes.extend_from_slice(&public_key);
    bytes.extend_from_slice(&reference_length.to_be_bytes());
    bytes.extend_from_slice(key_reference.as_bytes());
    Ok(bytes)
}

fn put_exact(
    scope: &mut PrincipalScope<'_>,
    key: RowKey,
    now: u64,
    bytes: Vec<u8>,
) -> Result<(), OnboardingError> {
    if let Some(existing) = scope.get(Table::Journeys, &key) {
        return if existing.bytes() == bytes {
            Ok(())
        } else {
            Err(OnboardingError::EvidenceConflict)
        };
    }
    scope.put(Table::Journeys, key, now, bytes)?;
    Ok(())
}

fn evidence_bytes(scope: &PrincipalScope<'_>, key: &RowKey) -> Result<Vec<u8>, OnboardingError> {
    scope
        .get(Table::Journeys, key)
        .map(|row| row.bytes().to_vec())
        .ok_or(OnboardingError::CorruptJourney(
            "activation evidence is missing",
        ))
}

fn verify_stored_digest(bytes: &[u8], expected: Option<[u8; 32]>) -> Result<(), OnboardingError> {
    if expected == Some(Sha256::digest(bytes).into()) {
        Ok(())
    } else {
        Err(OnboardingError::EvidenceConflict)
    }
}

fn local_pointer(row: RowKey) -> EvidencePointer {
    EvidencePointer {
        row,
        class: EvidenceClass::LocalJourneyState,
        verification: EvidenceVerification::Unverified,
    }
}

fn receipt_pointer(row: RowKey) -> EvidencePointer {
    EvidencePointer {
        row,
        class: EvidenceClass::LayerxReceipt,
        verification: EvidenceVerification::ReceiptVerified,
    }
}

fn receipt_row(stage: ProtocolStage) -> Result<RowKey, OnboardingError> {
    match stage {
        ProtocolStage::DidRegistration => did_receipt_row(),
        ProtocolStage::RecoveryRegistration => recovery_receipt_row(),
    }
}

fn journey_row() -> Result<RowKey, OnboardingError> {
    Ok(RowKey::new(JOURNEY_ROW)?)
}

fn application_row() -> Result<RowKey, OnboardingError> {
    Ok(RowKey::new(APPLICATION_EVIDENCE_ROW)?)
}

fn key_evidence_row() -> Result<RowKey, OnboardingError> {
    Ok(RowKey::new(KEY_EVIDENCE_ROW)?)
}

fn did_receipt_row() -> Result<RowKey, OnboardingError> {
    Ok(RowKey::new(DID_RECEIPT_ROW)?)
}

fn recovery_receipt_row() -> Result<RowKey, OnboardingError> {
    Ok(RowKey::new(RECOVERY_RECEIPT_ROW)?)
}

fn primary_key_id() -> Result<KeyId, OnboardingError> {
    Ok(KeyId::new(PRIMARY_KEY_ID)?)
}

fn application_row_infallible() -> RowKey {
    RowKey::new(APPLICATION_EVIDENCE_ROW).unwrap_or_else(|_| unreachable!())
}

fn key_evidence_row_infallible() -> RowKey {
    RowKey::new(KEY_EVIDENCE_ROW).unwrap_or_else(|_| unreachable!())
}

fn did_receipt_row_infallible() -> RowKey {
    RowKey::new(DID_RECEIPT_ROW).unwrap_or_else(|_| unreachable!())
}

fn recovery_receipt_row_infallible() -> RowKey {
    RowKey::new(RECOVERY_RECEIPT_ROW).unwrap_or_else(|_| unreachable!())
}

fn hex(bytes: &[u8]) -> String {
    let mut text = String::with_capacity(bytes.len().saturating_mul(2));
    for byte in bytes {
        use std::fmt::Write as _;
        let _ = write!(text, "{byte:02x}");
    }
    text
}

/// Typed onboarding refusal. Every error preserves the durable journey and no
/// variant can be mistaken for account activation.
#[derive(Debug)]
pub enum OnboardingError {
    Store(StoreError),
    Custody(CustodyError),
    Audit(AuditError),
    Intent(IntentError),
    Compile(CompileError),
    Disclosure(DisclosureCheckError),
    Contract(ContractError),
    Verification(VerificationFailure),
    Sdk(SdkError),
    InvalidRecoveryPolicy,
    InvalidIdempotencyKey,
    InvalidAgentContext,
    IdempotencyConflict,
    EntropyUnavailable,
    CustodyKeyRequired,
    StageNotEligible,
    ActionNotPrepared,
    ActionKeyMismatch,
    SubmissionConflict,
    ReceiptRequired,
    ReceiptShape,
    ReceiptActivityMismatch,
    ReceiptOperationMismatch,
    ReceiptReuse,
    EvidenceConflict,
    AuditConflict,
    CorruptJourney(&'static str),
}

impl Display for OnboardingError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Store(error) => write!(formatter, "onboarding store failure: {error}"),
            Self::Custody(error) => write!(formatter, "onboarding custody failure: {error}"),
            Self::Audit(error) => write!(formatter, "onboarding audit failure: {error}"),
            Self::Intent(error) => write!(formatter, "onboarding intent failure: {error:?}"),
            Self::Compile(error) => write!(formatter, "onboarding compile failure: {error:?}"),
            Self::Disclosure(error) => {
                write!(formatter, "onboarding disclosure failure: {error:?}")
            }
            Self::Contract(error) => write!(formatter, "agent contract refusal: {error:?}"),
            Self::Verification(error) => {
                write!(formatter, "receipt verification failed: {error:?}")
            }
            Self::Sdk(error) => write!(formatter, "agent SDK refusal: {error:?}"),
            Self::InvalidRecoveryPolicy => formatter.write_str("recovery policy is invalid"),
            Self::InvalidIdempotencyKey => formatter.write_str("idempotency key is invalid"),
            Self::InvalidAgentContext => {
                formatter.write_str("agent preparation context is invalid")
            }
            Self::IdempotencyConflict => {
                formatter.write_str("onboarding request conflicts with the existing journey")
            }
            Self::EntropyUnavailable => {
                formatter.write_str("key-generation entropy is unavailable")
            }
            Self::CustodyKeyRequired => formatter.write_str("custody key generation is incomplete"),
            Self::StageNotEligible => formatter.write_str("onboarding stage is not eligible"),
            Self::ActionNotPrepared => {
                formatter.write_str("agent action has not been durably prepared")
            }
            Self::ActionKeyMismatch => {
                formatter.write_str("agent update names another onboarding action")
            }
            Self::SubmissionConflict => {
                formatter.write_str("agent update names another submission")
            }
            Self::ReceiptRequired => {
                formatter.write_str("executed state requires verified receipt evidence")
            }
            Self::ReceiptShape => formatter.write_str("receipt is not a full protocol receipt"),
            Self::ReceiptActivityMismatch => formatter.write_str("receipt names another activity"),
            Self::ReceiptOperationMismatch => {
                formatter.write_str("receipt names another protocol operation")
            }
            Self::ReceiptReuse => {
                formatter.write_str("one receipt cannot activate two onboarding mutations")
            }
            Self::EvidenceConflict => formatter.write_str("durable onboarding evidence conflicts"),
            Self::AuditConflict => formatter.write_str("identity audit history conflicts"),
            Self::CorruptJourney(reason) => {
                write!(formatter, "corrupt onboarding journey: {reason}")
            }
        }
    }
}

impl std::error::Error for OnboardingError {}

impl From<StoreError> for OnboardingError {
    fn from(value: StoreError) -> Self {
        Self::Store(value)
    }
}

impl From<CustodyError> for OnboardingError {
    fn from(value: CustodyError) -> Self {
        Self::Custody(value)
    }
}

impl From<AuditError> for OnboardingError {
    fn from(value: AuditError) -> Self {
        Self::Audit(value)
    }
}

impl From<IntentError> for OnboardingError {
    fn from(value: IntentError) -> Self {
        Self::Intent(value)
    }
}

impl From<CompileError> for OnboardingError {
    fn from(value: CompileError) -> Self {
        Self::Compile(value)
    }
}

impl From<DisclosureCheckError> for OnboardingError {
    fn from(value: DisclosureCheckError) -> Self {
        Self::Disclosure(value)
    }
}

impl From<ContractError> for OnboardingError {
    fn from(value: ContractError) -> Self {
        Self::Contract(value)
    }
}

impl From<VerificationFailure> for OnboardingError {
    fn from(value: VerificationFailure) -> Self {
        Self::Verification(value)
    }
}

impl From<SdkError> for OnboardingError {
    fn from(value: SdkError) -> Self {
        Self::Sdk(value)
    }
}
