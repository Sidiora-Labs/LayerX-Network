//! Crash-resumable execution of receipt-gated journey legs.

use std::collections::BTreeSet;
use std::fmt::{Display, Formatter};

use layerx_agent_api::error::RequestId;
use layerx_agent_api::idempotency::{BodyDigest, IdempotentMutation, Key};
use layerx_agent_api::identity::{AgentDid, AuthorityRef, ContractError};
use layerx_agent_api::prepare::{
    IdempotencyRef, PayloadBytes, PreparationRef, PrepareRequest, TimestampBound,
};
use layerx_agent_api::submit::{SignatureBytes, SubmitRequest};
use layerx_agent_api::track::{SubmissionRef, TrackRequest, TrackedSubmission};
use layerx_agent_api::{Amount, Sequence, TimestampSeconds};
use layerx_crypto::disclosure::Disclosure;
use layerx_intents::{compile, CompileError, DisclosureCheck, DisclosureCheckError, Intent};
use layerx_proof::receipt::{verify_outcome, AuthorizedBatch, VerificationFailure};
use layerx_sdk::{Call, Client as AgentClient, SdkError, SubmissionOutcome};
use layerx_types::payload::{ActivityType, ModuleRegistry, PayloadError};
use layerx_types::verify::VerificationLevel;
use serde::{Deserialize, Serialize};
use sha2::{Digest as _, Sha256};

use crate::custody::{
    CustodyError, CustodySigner, KeyId, Operation, SignAuthorization, SignRequest, StepUpEvidence,
};
use crate::notify::JourneyId;
use crate::store::{PrincipalScope, RowKey, StoreError, Table};
use crate::trace::TraceId;

const RECORD_VERSION: u8 = 1;
const MAXIMUM_LEGS: usize = 32;
const TEXT_LIMIT: usize = 256;
const JOURNEY_PREFIX: &str = "journey-";
const STREAM_PREFIX: &str = "jstream-";
const NOTIFY_PREFIX: &str = "jnotify-";
const PREPARE_DIGEST_DOMAIN: &[u8] = b"layerx-human-journey-prepare/v1";
const SUBMIT_DIGEST_DOMAIN: &[u8] = b"layerx-human-journey-submit/v1";
const PLAN_DIGEST_DOMAIN: &[u8] = b"layerx-human-journey-plan/v1";

/// Complete core-produced inputs for one use of the agent prepare operation.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct JourneyLeg {
    intent: Intent,
    action_key: [u8; 32],
    actor: AgentDid,
    authority: AuthorityRef,
    account_sequence: u64,
    not_before: u64,
    not_after: u64,
    fee_limit: u128,
}

impl JourneyLeg {
    /// Binds one typed intent to its stable agent-layer action identity and
    /// complete preparation context.
    ///
    /// # Errors
    ///
    /// Refuses the reserved zero action key, oversized references, and an
    /// inverted timestamp interval.
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        intent: Intent,
        action_key: [u8; 32],
        actor: AgentDid,
        authority: AuthorityRef,
        account_sequence: u64,
        not_before: u64,
        not_after: u64,
        fee_limit: u128,
    ) -> Result<Self, JourneyError> {
        if action_key == [0; 32]
            || actor.as_str().len() > TEXT_LIMIT
            || authority.as_str().len() > TEXT_LIMIT
            || not_after < not_before
        {
            return Err(JourneyError::InvalidPlan);
        }
        Ok(Self {
            intent,
            action_key,
            actor,
            authority,
            account_sequence,
            not_before,
            not_after,
            fee_limit,
        })
    }
}

/// One immutable movement plan. The caller idempotency key owns the durable
/// record; each leg has a separate stable economic idempotency key.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum JourneyKind {
    Onboarding,
    WalletBinding,
    Deposit,
    Withdraw,
    Exit,
    Move,
    AgentCreate,
    AgentFund,
    AgentPause,
    AgentRetire,
}

impl JourneyKind {
    const fn code(self) -> u8 {
        match self {
            Self::Onboarding => 1,
            Self::WalletBinding => 2,
            Self::Deposit => 3,
            Self::Withdraw => 4,
            Self::Exit => 5,
            Self::Move => 6,
            Self::AgentCreate => 7,
            Self::AgentFund => 8,
            Self::AgentPause => 9,
            Self::AgentRetire => 10,
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct JourneyPlan {
    journey_id: JourneyId,
    kind: JourneyKind,
    idempotency_key: [u8; 32],
    custody_key: KeyId,
    signing_operation: Operation,
    legs: Vec<JourneyLeg>,
}

impl JourneyPlan {
    /// Constructs a bounded non-empty plan with unique per-leg action keys.
    ///
    /// # Errors
    ///
    /// Refuses a zero caller key, no legs, too many legs, or reused action keys.
    pub fn new(
        journey_id: JourneyId,
        kind: JourneyKind,
        idempotency_key: [u8; 32],
        custody_key: KeyId,
        signing_operation: Operation,
        legs: Vec<JourneyLeg>,
    ) -> Result<Self, JourneyError> {
        let mut action_keys = BTreeSet::new();
        if idempotency_key == [0; 32]
            || legs.is_empty()
            || legs.len() > MAXIMUM_LEGS
            || legs.iter().any(|leg| !action_keys.insert(leg.action_key))
        {
            return Err(JourneyError::InvalidPlan);
        }
        Ok(Self {
            journey_id,
            kind,
            idempotency_key,
            custody_key,
            signing_operation,
            legs,
        })
    }
}

/// Exact preparation returned by an agent-layer implementation. The structured
/// disclosure is retained only in memory and is re-derived by an idempotent
/// repeated prepare after a service restart.
#[derive(Clone, Debug)]
pub struct AgentPreparation {
    pub preparation_ref: PreparationRef,
    pub unsigned_canonical_bytes: Vec<u8>,
    pub signing_preimage: Vec<u8>,
    pub disclosure: Disclosure,
    pub actor: AgentDid,
    pub authority: AuthorityRef,
    pub account_sequence: u64,
    pub not_before: u64,
    pub not_after: u64,
    pub fee_limit: u128,
    pub activity_type: ActivityType,
    pub payload: Vec<u8>,
    pub payload_hash: [u8; 32],
    pub idempotency_key: [u8; 32],
}

/// Canonical receipt bytes, independently supplied batch authority, and the
/// verification rank authenticated by the agent boundary. The rank is not
/// inferred from receipt bytes and cannot replace local receipt verification.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ReceiptMaterial {
    pub canonical_bytes: Vec<u8>,
    pub authorised_batch: AuthorizedBatch,
    pub verification_level: VerificationLevel,
}

/// Receipt evidence retained by a completed journey leg. This exposes only
/// the immutable facts a higher-level journey needs for operation-specific
/// verification; it cannot be used to advance or rewrite the engine.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct VerifiedLegEvidence {
    pub action_key: [u8; 32],
    pub activity_id: [u8; 32],
    pub canonical_receipt: Vec<u8>,
    pub receipt_digest: [u8; 32],
    pub authorised_batch: AuthorizedBatch,
}

/// Result of the receipt-only resolution operation.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ReceiptLookup {
    Absent,
    Found(ReceiptMaterial),
}

/// One tracked agent result. Executed is not accepted without matching receipt
/// material and independently verified evidence.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AgentObservation {
    pub submission: TrackedSubmission,
    pub activity_id: [u8; 32],
    pub receipt: Option<ReceiptMaterial>,
}

/// Stable failures of the real agent boundary.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum AgentBoundaryError {
    Unavailable,
    Refused,
    CorruptResponse,
}

/// The existing prepare, submit, track and receipt-lookup operations used by
/// the engine. Production adapters execute these typed SDK calls; tests bind
/// the same contract to the real `layerx-agentd` implementation.
pub trait AgentBoundary {
    /// Prepares canonical unsigned activity bytes under the request action key.
    ///
    /// # Errors
    ///
    /// Returns a stable boundary error when the real agent cannot prepare the activity.
    fn prepare(
        &mut self,
        call: &Call<IdempotentMutation<PrepareRequest>>,
    ) -> Result<AgentPreparation, AgentBoundaryError>;

    /// Submits signed activity bytes under the original action key.
    ///
    /// # Errors
    ///
    /// Returns a stable boundary error when the real agent cannot accept or observe the submit.
    fn submit(
        &mut self,
        call: &Call<IdempotentMutation<SubmitRequest>>,
        signer_public_key: [u8; 32],
    ) -> Result<AgentObservation, AgentBoundaryError>;

    /// Tracks an acknowledged submission without creating a new economic effect.
    ///
    /// # Errors
    ///
    /// Returns a stable boundary error when the real agent cannot report the submission state.
    fn track(&mut self, call: &Call<TrackRequest>) -> Result<AgentObservation, AgentBoundaryError>;

    /// Looks up receipt evidence by the original action key and expected activity identity.
    ///
    /// # Errors
    ///
    /// Returns a stable boundary error when the receipt lookup cannot be completed.
    fn receipt_by_idempotency_key(
        &mut self,
        idempotency_key: [u8; 32],
        expected_activity_id: [u8; 32],
    ) -> Result<ReceiptLookup, AgentBoundaryError>;
}

/// Durable phase of one economic leg.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum JourneyPhase {
    Compiled,
    Preparing,
    Prepared,
    Signed,
    Submitted,
    StillChecking,
    ReceiptVerified,
    Refused,
}

/// Honest user-facing state of the full journey.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum JourneyState {
    GettingReady,
    Sending,
    Processing,
    StillChecking,
    Done,
    Refused,
}

/// Public receipt-backed journey status.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct JourneyStatus {
    journey_id: JourneyId,
    state: JourneyState,
    current_leg: usize,
    phases: Vec<JourneyPhase>,
    receipt_digests: Vec<Option<[u8; 32]>>,
    receipt_material: Vec<Option<Vec<u8>>>,
    receipt_authorities: Vec<Option<AuthorizedBatch>>,
    refusal_codes: Vec<Option<i32>>,
}

impl JourneyStatus {
    #[must_use]
    pub const fn journey_id(&self) -> &JourneyId {
        &self.journey_id
    }

    #[must_use]
    pub const fn state(&self) -> JourneyState {
        self.state
    }

    #[must_use]
    pub const fn current_leg(&self) -> usize {
        self.current_leg
    }

    #[must_use]
    pub fn phases(&self) -> &[JourneyPhase] {
        &self.phases
    }

    #[must_use]
    pub fn receipt_digests(&self) -> &[Option<[u8; 32]>] {
        &self.receipt_digests
    }

    #[must_use]
    pub fn receipt_material(&self) -> &[Option<Vec<u8>>] {
        &self.receipt_material
    }
    #[must_use]
    pub fn receipt_authorities(&self) -> &[Option<AuthorizedBatch>] {
        &self.receipt_authorities
    }

    /// Returns the exact protocol result for each refused leg.
    #[must_use]
    pub fn refusal_codes(&self) -> &[Option<i32>] {
        &self.refusal_codes
    }
}

/// One durable event delivered to both the resumable stream outbox and the
/// notification-service outbox under the same stable transition identity.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct JourneyProgress {
    sequence: u64,
    journey_id: String,
    leg: usize,
    phase: JourneyPhase,
    observed_at: u64,
}

impl JourneyProgress {
    #[must_use]
    pub const fn sequence(&self) -> u64 {
        self.sequence
    }

    #[must_use]
    pub fn journey_id(&self) -> &str {
        &self.journey_id
    }

    #[must_use]
    pub const fn leg(&self) -> usize {
        self.leg
    }

    #[must_use]
    pub const fn phase(&self) -> JourneyPhase {
        self.phase
    }

    #[must_use]
    pub const fn observed_at(&self) -> u64 {
        self.observed_at
    }

    /// Stable cursor consumed by the Human stream contract.
    #[must_use]
    pub fn cursor(&self) -> String {
        format!("{}:{:020}", self.journey_id, self.sequence)
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
struct StoredPreparation {
    actor: String,
    authority: String,
    account_sequence: u64,
    not_before: u64,
    not_after: u64,
    fee_limit: u128,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
struct SignedEvidence {
    preparation_ref: String,
    canonical_digest: [u8; 32],
    signature: Vec<u8>,
    signer_public_key: [u8; 32],
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
struct LegRecord {
    action_key: [u8; 32],
    activity_type: u32,
    expected_operation: u8,
    payload: Vec<u8>,
    payload_hash: [u8; 32],
    preparation: StoredPreparation,
    phase: JourneyPhase,
    prepared_ref: Option<String>,
    prepared_digest: Option<[u8; 32]>,
    signed: Option<SignedEvidence>,
    submission_ref: Option<String>,
    activity_id: Option<[u8; 32]>,
    receipt: Option<Vec<u8>>,
    receipt_digest: Option<[u8; 32]>,
    #[serde(default)]
    receipt_authority: Option<StoredAuthorizedBatch>,
    refusal_code: Option<i32>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
struct StoredAuthorizedBatch {
    batch_id: [u8; 32],
    asset: [u8; 32],
    previous_state_root: [u8; 32],
    resulting_state_root: [u8; 32],
    sequencer_public_key: [u8; 32],
}
impl StoredAuthorizedBatch {
    fn from_public(value: &AuthorizedBatch) -> Self {
        Self {
            batch_id: value.batch_id(),
            asset: value.asset(),
            previous_state_root: value.previous_state_root(),
            resulting_state_root: value.resulting_state_root(),
            sequencer_public_key: value.sequencer_public_key(),
        }
    }
    fn public(&self) -> AuthorizedBatch {
        AuthorizedBatch::new(
            self.batch_id,
            self.asset,
            self.previous_state_root,
            self.resulting_state_root,
            self.sequencer_public_key,
        )
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
struct TransitionRecord {
    sequence: u64,
    leg: usize,
    phase: JourneyPhase,
    observed_at: u64,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
struct JourneyRecord {
    version: u8,
    journey_id: String,
    kind: JourneyKind,
    idempotency_key: [u8; 32],
    plan_digest: [u8; 32],
    custody_key: String,
    signing_operation: String,
    current_leg: usize,
    started_at: u64,
    updated_at: u64,
    legs: Vec<LegRecord>,
    transitions: Vec<TransitionRecord>,
}

/// Durable state machine for a multi-leg Human journey.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct JourneyEngine {
    record: JourneyRecord,
}

impl JourneyEngine {
    /// Returns protocol identities only for a leg whose receipt has been
    /// independently verified. Pending submissions are intentionally hidden.
    pub fn verified_identity(&self, leg: usize) -> Option<(&str, [u8; 32])> {
        let leg = self.record.legs.get(leg)?;
        if leg.phase != JourneyPhase::ReceiptVerified || leg.receipt_digest.is_none() {
            return None;
        }
        Some((leg.submission_ref.as_deref()?, leg.activity_id?))
    }

    /// Re-obtains and validates the agent preparation for a persisted Prepared
    /// leg, exposing only the disclosure digest needed to bridge fresh
    /// authentication into custody. No signature or submission is produced.
    pub fn prepared_disclosure_digest(
        &self,
        agent_contract: &AgentClient,
        agent: &mut dyn AgentBoundary,
        registry: &ModuleRegistry,
    ) -> Result<[u8; 32], JourneyError> {
        let index = self.record.current_leg;
        if self.record.legs.get(index).map(|leg| leg.phase) != Some(JourneyPhase::Prepared) {
            return Err(JourneyError::Corrupt("journey leg is not prepared"));
        }
        let call = self.prepare_call(agent_contract, index)?;
        let prepared = agent.prepare(&call)?;
        self.validate_preparation(index, &prepared, registry)?;
        prepared
            .disclosure
            .audit_digest()
            .map_err(|_| JourneyError::PreparationMismatch)
    }
    /// Compiles and independently verifies every typed intent, then atomically
    /// persists the immutable plan before any agent-layer effect is possible.
    /// Repeating the caller idempotency key returns the original journey.
    ///
    /// # Errors
    ///
    /// Refuses a conflicting repeated key, intent/compiler/disclosure defects,
    /// or a durable-store failure.
    pub fn start(
        scope: &mut PrincipalScope<'_>,
        plan: &JourneyPlan,
        registry: &ModuleRegistry,
        now: u64,
    ) -> Result<Self, JourneyError> {
        let row = journey_row(plan.idempotency_key)?;
        let compiled = compile_plan(plan, registry)?;
        let plan_digest = plan_digest(plan, &compiled);
        if let Some(existing) = scope.get(Table::Journeys, &row) {
            let record = decode_record(existing.bytes())?;
            if record.plan_digest != plan_digest || record.journey_id != plan.journey_id.as_str() {
                return Err(JourneyError::IdempotencyConflict);
            }
            return Ok(Self { record });
        }
        let transitions = vec![TransitionRecord {
            sequence: 1,
            leg: 0,
            phase: JourneyPhase::Compiled,
            observed_at: now,
        }];
        let record = JourneyRecord {
            version: RECORD_VERSION,
            journey_id: plan.journey_id.as_str().to_owned(),
            kind: plan.kind,
            idempotency_key: plan.idempotency_key,
            plan_digest,
            custody_key: plan.custody_key.as_str().to_owned(),
            signing_operation: plan.signing_operation.label().to_owned(),
            current_leg: 0,
            started_at: now,
            updated_at: now,
            legs: compiled,
            transitions,
        };
        validate_record(&record)?;
        let engine = Self { record };
        engine.persist(scope)?;
        engine.repair_events(scope)?;
        Ok(engine)
    }

    /// Loads a journey by its public identifier from the authenticated
    /// principal scope.
    ///
    /// # Errors
    ///
    /// Refuses malformed records, duplicate identifiers, and storage failures.
    pub fn load(
        scope: &PrincipalScope<'_>,
        journey_id: &JourneyId,
    ) -> Result<Option<Self>, JourneyError> {
        let mut found = None;
        for key in scope.keys(Table::Journeys) {
            if !key.as_str().starts_with(JOURNEY_PREFIX) {
                continue;
            }
            let row = scope
                .get(Table::Journeys, &key)
                .ok_or(JourneyError::Corrupt("journey disappeared while loading"))?;
            let record = decode_record(row.bytes())?;
            if record.journey_id == journey_id.as_str() {
                if found.is_some() {
                    return Err(JourneyError::Corrupt("duplicate journey identifier"));
                }
                found = Some(Self { record });
            }
        }
        Ok(found)
    }

    /// Lists all journeys in an authenticated principal scope in stable
    /// newest-first order.
    ///
    /// # Errors
    ///
    /// Refuses malformed records or duplicate public identifiers.
    pub fn list(scope: &PrincipalScope<'_>) -> Result<Vec<Self>, JourneyError> {
        let mut journeys = Vec::new();
        let mut identifiers = BTreeSet::new();
        for key in scope.keys(Table::Journeys) {
            if !key.as_str().starts_with(JOURNEY_PREFIX) {
                continue;
            }
            let row = scope
                .get(Table::Journeys, &key)
                .ok_or(JourneyError::Corrupt("journey disappeared while listing"))?;
            let record = decode_record(row.bytes())?;
            if !identifiers.insert(record.journey_id.clone()) {
                return Err(JourneyError::Corrupt("duplicate journey identifier"));
            }
            journeys.push(Self { record });
        }
        journeys.sort_by(|left, right| {
            right
                .record
                .updated_at
                .cmp(&left.record.updated_at)
                .then_with(|| right.record.journey_id.cmp(&left.record.journey_id))
        });
        Ok(journeys)
    }

    #[must_use]
    pub const fn started_at(&self) -> u64 {
        self.record.started_at
    }

    #[must_use]
    pub const fn updated_at(&self) -> u64 {
        self.record.updated_at
    }

    #[must_use]
    pub const fn kind(&self) -> JourneyKind {
        self.record.kind
    }

    /// Advances at most one durable phase. Every external call is preceded by
    /// a durable phase and is repeatable under the same action key after a
    /// crash. An unknown submission takes the receipt-lookup-only branch.
    ///
    /// # Errors
    ///
    /// Returns typed agent, custody, receipt, contract, SDK, and storage
    /// failures without advancing the next leg.
    #[allow(clippy::too_many_arguments)]
    pub async fn advance(
        &mut self,
        scope: &mut PrincipalScope<'_>,
        agent_contract: &AgentClient,
        agent: &mut dyn AgentBoundary,
        custody: &CustodySigner,
        registry: &ModuleRegistry,
        trace: &TraceId,
        now: u64,
    ) -> Result<JourneyStatus, JourneyError> {
        self.advance_authorized(
            scope,
            agent_contract,
            agent,
            custody,
            registry,
            trace,
            None,
            now,
        )
        .await
    }

    /// Advances one phase with optional fresh step-up evidence for a sensitive
    /// signing operation. Ordinary protocol mutations use [`Self::advance`].
    ///
    /// # Errors
    ///
    /// Returns the same typed failures as [`Self::advance`], including custody
    /// refusal when a sensitive operation lacks matching fresh evidence.
    #[allow(clippy::too_many_arguments)]
    pub async fn advance_authorized(
        &mut self,
        scope: &mut PrincipalScope<'_>,
        agent_contract: &AgentClient,
        agent: &mut dyn AgentBoundary,
        custody: &CustodySigner,
        registry: &ModuleRegistry,
        trace: &TraceId,
        step_up: Option<&StepUpEvidence>,
        now: u64,
    ) -> Result<JourneyStatus, JourneyError> {
        self.repair_events(scope)?;
        if self.terminal() {
            return self.status();
        }
        if now < self.record.updated_at {
            return Err(JourneyError::TimeRegressed);
        }
        let leg_index = self.record.current_leg;
        let phase = self.record.legs[leg_index].phase;
        match phase {
            JourneyPhase::Compiled => {
                self.transition(scope, leg_index, JourneyPhase::Preparing, now)?;
            }
            JourneyPhase::Preparing => {
                let call = self.prepare_call(agent_contract, leg_index)?;
                let prepared = agent.prepare(&call)?;
                self.validate_preparation(leg_index, &prepared, registry)?;
                let leg = &mut self.record.legs[leg_index];
                leg.prepared_ref = Some(prepared.preparation_ref.as_str().to_owned());
                leg.prepared_digest =
                    Some(Sha256::digest(&prepared.unsigned_canonical_bytes).into());
                self.transition(scope, leg_index, JourneyPhase::Prepared, now)?;
            }
            JourneyPhase::Prepared => {
                let call = self.prepare_call(agent_contract, leg_index)?;
                let prepared = agent.prepare(&call)?;
                self.validate_preparation(leg_index, &prepared, registry)?;
                let operation = operation_from_label(&self.record.signing_operation)?;
                let key = KeyId::new(self.record.custody_key.clone())?;
                let principal = scope.principal().clone();
                let signature = custody
                    .sign_in_scope(
                        scope,
                        SignRequest::new(
                            &principal,
                            &key,
                            trace,
                            SignAuthorization::new(operation, step_up),
                            &prepared.unsigned_canonical_bytes,
                            &prepared.disclosure,
                            now,
                        ),
                    )
                    .await?;
                let signed = SignedEvidence {
                    preparation_ref: prepared.preparation_ref.as_str().to_owned(),
                    canonical_digest: Sha256::digest(&prepared.unsigned_canonical_bytes).into(),
                    signature: signature.signature().to_vec(),
                    signer_public_key: signature.signer_public_key(),
                };
                self.record.legs[leg_index].signed = Some(signed);
                self.transition(scope, leg_index, JourneyPhase::Signed, now)?;
            }
            JourneyPhase::Signed => {
                let (call, signer_public_key) = self.submit_call(agent_contract, leg_index)?;
                let observation = agent.submit(&call, signer_public_key)?;
                self.apply_observation(scope, leg_index, observation, now)?;
            }
            JourneyPhase::Submitted => {
                let submission_ref = self.record.legs[leg_index].submission_ref.as_ref().ok_or(
                    JourneyError::Corrupt("submitted leg has no submission reference"),
                )?;
                let call = agent_contract.track(TrackRequest {
                    submission_ref: SubmissionRef::new(submission_ref.clone())?,
                });
                let observation = agent.track(&call)?;
                self.apply_observation(scope, leg_index, observation, now)?;
            }
            JourneyPhase::StillChecking => {
                let leg = &self.record.legs[leg_index];
                let activity_id = leg.activity_id.ok_or(JourneyError::Corrupt(
                    "unknown leg has no activity identity",
                ))?;
                match agent.receipt_by_idempotency_key(leg.action_key, activity_id)? {
                    ReceiptLookup::Absent => {}
                    ReceiptLookup::Found(receipt) => {
                        self.verify_and_complete(scope, leg_index, activity_id, &receipt, now)?;
                    }
                }
            }
            JourneyPhase::ReceiptVerified | JourneyPhase::Refused => {}
        }
        self.repair_events(scope)?;
        self.status()
    }

    /// Returns the current state without claiming more than persisted receipt evidence.
    ///
    /// # Errors
    ///
    /// Returns an invariant error when the stored journey identifier is corrupt.
    pub fn status(&self) -> Result<JourneyStatus, JourneyError> {
        let journey_id = JourneyId::new(self.record.journey_id.clone())
            .map_err(|_| JourneyError::Corrupt("invalid journey identifier"))?;
        let phases = self.record.legs.iter().map(|leg| leg.phase).collect();
        let receipt_digests = self
            .record
            .legs
            .iter()
            .map(|leg| leg.receipt_digest)
            .collect();
        let refusal_codes = self
            .record
            .legs
            .iter()
            .map(|leg| leg.refusal_code)
            .collect();
        let receipt_material = self
            .record
            .legs
            .iter()
            .map(|leg| leg.receipt.clone())
            .collect();
        let receipt_authorities = self
            .record
            .legs
            .iter()
            .map(|leg| {
                leg.receipt_authority
                    .as_ref()
                    .map(StoredAuthorizedBatch::public)
            })
            .collect();
        Ok(JourneyStatus {
            journey_id,
            state: self.state(),
            current_leg: self.record.current_leg,
            phases,
            receipt_digests,
            receipt_material,
            receipt_authorities,
            refusal_codes,
        })
    }

    /// Returns the immutable receipt evidence for a verified leg.
    ///
    /// # Errors
    ///
    /// Refuses an out-of-range leg or a corrupt partially verified record.
    pub fn verified_leg_evidence(
        &self,
        index: usize,
    ) -> Result<Option<VerifiedLegEvidence>, JourneyError> {
        let leg = self
            .record
            .legs
            .get(index)
            .ok_or(JourneyError::Corrupt("receipt leg is outside the plan"))?;
        if leg.phase != JourneyPhase::ReceiptVerified {
            return Ok(None);
        }
        Ok(Some(VerifiedLegEvidence {
            action_key: leg.action_key,
            activity_id: leg
                .activity_id
                .ok_or(JourneyError::Corrupt("verified leg has no activity"))?,
            canonical_receipt: leg
                .receipt
                .clone()
                .ok_or(JourneyError::Corrupt("verified leg has no receipt"))?,
            receipt_digest: leg
                .receipt_digest
                .ok_or(JourneyError::Corrupt("verified leg has no receipt digest"))?,
            authorised_batch: leg
                .receipt_authority
                .as_ref()
                .ok_or(JourneyError::Corrupt(
                    "verified leg has no receipt authority",
                ))?
                .public(),
        }))
    }

    /// Reads ordered progress events from the resumable stream outbox.
    ///
    /// # Errors
    ///
    /// Returns a storage or decoding error when the durable stream cannot be read safely.
    pub fn stream_events(
        scope: &PrincipalScope<'_>,
        journey_id: &JourneyId,
    ) -> Result<Vec<JourneyProgress>, JourneyError> {
        events(scope, STREAM_PREFIX, journey_id)
    }

    /// Reads ordered stage-boundary events queued durably for the notification service.
    ///
    /// # Errors
    ///
    /// Returns a storage or decoding error when the notification outbox cannot be read safely.
    pub fn notification_events(
        scope: &PrincipalScope<'_>,
        journey_id: &JourneyId,
    ) -> Result<Vec<JourneyProgress>, JourneyError> {
        events(scope, NOTIFY_PREFIX, journey_id)
    }

    fn state(&self) -> JourneyState {
        if self
            .record
            .legs
            .iter()
            .any(|leg| leg.phase == JourneyPhase::Refused)
        {
            return JourneyState::Refused;
        }
        if self
            .record
            .legs
            .iter()
            .all(|leg| leg.phase == JourneyPhase::ReceiptVerified)
        {
            return JourneyState::Done;
        }
        match self.record.legs[self.record.current_leg].phase {
            JourneyPhase::Compiled | JourneyPhase::Preparing | JourneyPhase::Prepared => {
                JourneyState::GettingReady
            }
            JourneyPhase::Signed => JourneyState::Sending,
            JourneyPhase::Submitted | JourneyPhase::ReceiptVerified => JourneyState::Processing,
            JourneyPhase::StillChecking => JourneyState::StillChecking,
            JourneyPhase::Refused => JourneyState::Refused,
        }
    }

    fn terminal(&self) -> bool {
        matches!(self.state(), JourneyState::Done | JourneyState::Refused)
    }

    fn prepare_call(
        &self,
        agent: &AgentClient,
        index: usize,
    ) -> Result<Call<IdempotentMutation<PrepareRequest>>, JourneyError> {
        let leg = self
            .record
            .legs
            .get(index)
            .ok_or(JourneyError::Corrupt("current leg is outside the plan"))?;
        let request = PrepareRequest {
            protocol_activity_type: leg.activity_type,
            actor: AgentDid::new(leg.preparation.actor.clone())?,
            authority: AuthorityRef::new(leg.preparation.authority.clone())?,
            account_sequence: Sequence(leg.preparation.account_sequence),
            timestamp_bound: TimestampBound {
                not_before: TimestampSeconds(leg.preparation.not_before),
                not_after: TimestampSeconds(leg.preparation.not_after),
            }
            .validate()?,
            idempotency_key: IdempotencyRef::new(hex(&leg.action_key))?,
            fee_limit: Amount(leg.preparation.fee_limit),
            payload: PayloadBytes::new(leg.payload.clone())?,
            payload_hash: leg.payload_hash,
        };
        let digest = prepare_digest(&request);
        Ok(agent.prepare(mutation(leg.action_key, digest, request)?))
    }

    fn submit_call(
        &self,
        agent: &AgentClient,
        index: usize,
    ) -> Result<(Call<IdempotentMutation<SubmitRequest>>, [u8; 32]), JourneyError> {
        let leg = self
            .record
            .legs
            .get(index)
            .ok_or(JourneyError::Corrupt("current leg is outside the plan"))?;
        let signed = leg.signed.as_ref().ok_or(JourneyError::Corrupt(
            "signed leg has no signature evidence",
        ))?;
        let request = SubmitRequest {
            preparation_ref: PreparationRef::new(signed.preparation_ref.clone())?,
            signature: SignatureBytes::new(signed.signature.clone())?,
            approval_release_ref: None,
        };
        let digest = submit_digest(&request, signed.signer_public_key);
        Ok((
            agent.submit(mutation(leg.action_key, digest, request)?),
            signed.signer_public_key,
        ))
    }

    fn validate_preparation(
        &self,
        index: usize,
        prepared: &AgentPreparation,
        registry: &ModuleRegistry,
    ) -> Result<(), JourneyError> {
        let leg = self
            .record
            .legs
            .get(index)
            .ok_or(JourneyError::Corrupt("current leg is outside the plan"))?;
        let canonical_digest: [u8; 32] = Sha256::digest(&prepared.unsigned_canonical_bytes).into();
        if prepared.preparation_ref.as_str().len() > TEXT_LIMIT
            || prepared.signing_preimage.is_empty()
            || prepared.actor.as_str() != leg.preparation.actor
            || prepared.authority.as_str() != leg.preparation.authority
            || prepared.account_sequence != leg.preparation.account_sequence
            || prepared.not_before != leg.preparation.not_before
            || prepared.not_after != leg.preparation.not_after
            || prepared.fee_limit != leg.preparation.fee_limit
            || prepared.activity_type.value() != leg.activity_type
            || prepared.payload != leg.payload
            || prepared.payload_hash != leg.payload_hash
            || prepared.idempotency_key != leg.action_key
            || leg
                .prepared_ref
                .as_ref()
                .is_some_and(|value| value != prepared.preparation_ref.as_str())
            || leg
                .prepared_digest
                .is_some_and(|value| value != canonical_digest)
        {
            return Err(JourneyError::PreparationMismatch);
        }
        let reencoded = prepared
            .disclosure
            .reencode()
            .map_err(|_| JourneyError::PreparationMismatch)?;
        if reencoded != prepared.unsigned_canonical_bytes {
            return Err(JourneyError::PreparationMismatch);
        }
        let _ = registry;
        if prepared.disclosure.activity_type.value() != leg.activity_type
            || prepared.disclosure.idempotency_key != leg.action_key
            || prepared.disclosure.fee_limit != leg.preparation.fee_limit
        {
            return Err(JourneyError::PreparationMismatch);
        }
        Ok(())
    }

    fn apply_observation(
        &mut self,
        scope: &mut PrincipalScope<'_>,
        index: usize,
        observation: AgentObservation,
        now: u64,
    ) -> Result<(), JourneyError> {
        if observation.activity_id == [0; 32] {
            return Err(JourneyError::ObservationMismatch);
        }
        let leg = &self.record.legs[index];
        if leg
            .submission_ref
            .as_ref()
            .is_some_and(|stored| stored != observation.submission.submission_ref.as_str())
            || leg
                .activity_id
                .is_some_and(|stored| stored != observation.activity_id)
        {
            return Err(JourneyError::ObservationMismatch);
        }
        self.record.legs[index].submission_ref =
            Some(observation.submission.submission_ref.as_str().to_owned());
        self.record.legs[index].activity_id = Some(observation.activity_id);
        match AgentClient::submission_outcome(observation.submission)? {
            SubmissionOutcome::Unknown(_) => {
                if observation.receipt.is_some() {
                    return Err(JourneyError::ObservationMismatch);
                }
                self.transition(scope, index, JourneyPhase::StillChecking, now)
            }
            SubmissionOutcome::Executed(_) => {
                let receipt = observation.receipt.ok_or(JourneyError::ReceiptRequired)?;
                self.verify_and_complete(scope, index, observation.activity_id, &receipt, now)
            }
            SubmissionOutcome::Failed { result, .. } => {
                self.record.legs[index].refusal_code = Some(result.raw());
                self.transition(scope, index, JourneyPhase::Refused, now)
            }
            SubmissionOutcome::Expired(_) => {
                self.record.legs[index].refusal_code = None;
                self.transition(scope, index, JourneyPhase::Refused, now)
            }
            SubmissionOutcome::Pending(_) => {
                if observation.receipt.is_some() {
                    return Err(JourneyError::ObservationMismatch);
                }
                self.transition(scope, index, JourneyPhase::Submitted, now)
            }
        }
    }

    fn verify_and_complete(
        &mut self,
        scope: &mut PrincipalScope<'_>,
        index: usize,
        expected_activity_id: [u8; 32],
        material: &ReceiptMaterial,
        now: u64,
    ) -> Result<(), JourneyError> {
        let verified = verify_outcome(&material.canonical_bytes, &material.authorised_batch)?;
        let receipt = verified
            .receipt()
            .protocol()
            .ok_or(JourneyError::ReceiptShape)?;
        let leg = &self.record.legs[index];
        if receipt.activity_id() != expected_activity_id
            || receipt.operation() != leg.expected_operation
        {
            return Err(JourneyError::ReceiptMismatch);
        }
        let canonical = verified.canonical_bytes().to_vec();
        let digest: [u8; 32] = Sha256::digest(&canonical).into();
        if self.record.legs.iter().enumerate().any(|(other, leg)| {
            other != index
                && (leg.receipt_digest == Some(digest)
                    || leg.activity_id == Some(expected_activity_id))
        }) {
            return Err(JourneyError::ReceiptReuse);
        }
        let leg = &mut self.record.legs[index];
        if leg.receipt_digest.is_some_and(|stored| stored != digest)
            || leg
                .receipt
                .as_ref()
                .is_some_and(|stored| stored != &canonical)
        {
            return Err(JourneyError::ReceiptMismatch);
        }
        leg.receipt = Some(canonical);
        leg.receipt_digest = Some(digest);
        leg.receipt_authority = Some(StoredAuthorizedBatch::from_public(
            &material.authorised_batch,
        ));
        leg.refusal_code = None;
        if now < self.record.updated_at {
            return Err(JourneyError::TimeRegressed);
        }
        if self.record.legs[index].phase != JourneyPhase::ReceiptVerified {
            self.record.legs[index].phase = JourneyPhase::ReceiptVerified;
            self.record.updated_at = now;
            let sequence = u64::try_from(self.record.transitions.len())
                .map_err(|_| JourneyError::Corrupt("too many journey transitions"))?
                .checked_add(1)
                .ok_or(JourneyError::Corrupt("journey transition overflow"))?;
            self.record.transitions.push(TransitionRecord {
                sequence,
                leg: index,
                phase: JourneyPhase::ReceiptVerified,
                observed_at: now,
            });
        }
        if index.saturating_add(1) < self.record.legs.len() {
            self.record.current_leg = index.saturating_add(1);
        }
        self.persist(scope)
    }

    fn transition(
        &mut self,
        scope: &mut PrincipalScope<'_>,
        index: usize,
        phase: JourneyPhase,
        now: u64,
    ) -> Result<(), JourneyError> {
        if now < self.record.updated_at {
            return Err(JourneyError::TimeRegressed);
        }
        if self.record.legs[index].phase == phase {
            return Ok(());
        }
        self.record.legs[index].phase = phase;
        self.record.updated_at = now;
        let sequence = u64::try_from(self.record.transitions.len())
            .map_err(|_| JourneyError::Corrupt("too many journey transitions"))?
            .checked_add(1)
            .ok_or(JourneyError::Corrupt("journey transition overflow"))?;
        self.record.transitions.push(TransitionRecord {
            sequence,
            leg: index,
            phase,
            observed_at: now,
        });
        self.persist(scope)
    }

    fn persist(&self, scope: &mut PrincipalScope<'_>) -> Result<(), JourneyError> {
        validate_record(&self.record)?;
        let bytes = serde_json::to_vec(&self.record)
            .map_err(|_| JourneyError::Corrupt("journey cannot be encoded"))?;
        scope.put(
            Table::Journeys,
            journey_row(self.record.idempotency_key)?,
            self.record.updated_at,
            bytes,
        )?;
        Ok(())
    }

    fn repair_events(&self, scope: &mut PrincipalScope<'_>) -> Result<(), JourneyError> {
        for transition in &self.record.transitions {
            let progress = JourneyProgress {
                sequence: transition.sequence,
                journey_id: self.record.journey_id.clone(),
                leg: transition.leg,
                phase: transition.phase,
                observed_at: transition.observed_at,
            };
            let bytes = serde_json::to_vec(&progress)
                .map_err(|_| JourneyError::Corrupt("progress cannot be encoded"))?;
            put_exact(
                scope,
                event_row(STREAM_PREFIX, &self.record.journey_id, transition.sequence)?,
                transition.observed_at,
                &bytes,
            )?;
            put_exact(
                scope,
                event_row(NOTIFY_PREFIX, &self.record.journey_id, transition.sequence)?,
                transition.observed_at,
                &bytes,
            )?;
            put_stream_progress(scope, &progress, transition.observed_at)?;
        }
        Ok(())
    }
}

fn compile_plan(
    plan: &JourneyPlan,
    registry: &ModuleRegistry,
) -> Result<Vec<LegRecord>, JourneyError> {
    plan.legs
        .iter()
        .map(|leg| {
            let compiled = compile(&leg.intent, registry)?;
            let disclosure = DisclosureCheck::verify(&leg.intent, &compiled)?;
            let expected_operation = u8::try_from(compiled.activity_type().ordinal())
                .map_err(|_| JourneyError::InvalidPlan)?;
            Ok(LegRecord {
                action_key: leg.action_key,
                activity_type: compiled.activity_type().value(),
                expected_operation,
                payload: disclosure.canonical_payload().to_vec(),
                payload_hash: disclosure.payload_hash(),
                preparation: StoredPreparation {
                    actor: leg.actor.as_str().to_owned(),
                    authority: leg.authority.as_str().to_owned(),
                    account_sequence: leg.account_sequence,
                    not_before: leg.not_before,
                    not_after: leg.not_after,
                    fee_limit: leg.fee_limit,
                },
                phase: JourneyPhase::Compiled,
                prepared_ref: None,
                prepared_digest: None,
                signed: None,
                submission_ref: None,
                activity_id: None,
                receipt: None,
                receipt_digest: None,
                receipt_authority: None,
                refusal_code: None,
            })
        })
        .collect()
}

fn validate_record(record: &JourneyRecord) -> Result<(), JourneyError> {
    if record.version != RECORD_VERSION
        || JourneyId::new(record.journey_id.clone()).is_err()
        || record.idempotency_key == [0; 32]
        || record.plan_digest == [0; 32]
        || record.legs.is_empty()
        || record.legs.len() > MAXIMUM_LEGS
        || record.current_leg >= record.legs.len()
        || record.updated_at < record.started_at
        || record.transitions.is_empty()
    {
        return Err(JourneyError::Corrupt("journey invariants are invalid"));
    }
    let mut keys = BTreeSet::new();
    for (index, leg) in record.legs.iter().enumerate() {
        if leg.action_key == [0; 32]
            || !keys.insert(leg.action_key)
            || ActivityType::from_u32(leg.activity_type).is_err()
            || leg.expected_operation == 0
            || leg.payload.is_empty()
            || leg.preparation.not_after < leg.preparation.not_before
            || leg.preparation.actor.is_empty()
            || leg.preparation.authority.is_empty()
            || leg.preparation.actor.len() > TEXT_LIMIT
            || leg.preparation.authority.len() > TEXT_LIMIT
            || leg.prepared_ref.as_ref().is_some_and(String::is_empty)
            || leg.signed.as_ref().is_some_and(|value| {
                value.signature.len() != 64
                    || value.preparation_ref.is_empty()
                    || value.signer_public_key == [0; 32]
            })
            || leg.submission_ref.as_ref().is_some_and(String::is_empty)
            || leg.phase == JourneyPhase::ReceiptVerified
                && (leg.receipt.is_none()
                    || leg.receipt_digest.is_none()
                    || leg.activity_id.is_none()
                    || leg.submission_ref.is_none())
            || leg.phase != JourneyPhase::ReceiptVerified
                && (leg.receipt.is_some() || leg.receipt_digest.is_some())
            || index > record.current_leg && leg.phase != JourneyPhase::Compiled
            || index < record.current_leg && leg.phase != JourneyPhase::ReceiptVerified
            || index == record.current_leg
                && leg.phase == JourneyPhase::ReceiptVerified
                && index.saturating_add(1) < record.legs.len()
        {
            return Err(JourneyError::Corrupt("journey leg invariants are invalid"));
        }
    }
    for (offset, transition) in record.transitions.iter().enumerate() {
        let expected = u64::try_from(offset)
            .map_err(|_| JourneyError::Corrupt("transition index overflow"))?
            .saturating_add(1);
        if transition.sequence != expected
            || transition.leg >= record.legs.len()
            || transition.observed_at < record.started_at
        {
            return Err(JourneyError::Corrupt(
                "journey transition invariants are invalid",
            ));
        }
    }
    Ok(())
}

fn decode_record(bytes: &[u8]) -> Result<JourneyRecord, JourneyError> {
    let record: JourneyRecord = serde_json::from_slice(bytes)
        .map_err(|_| JourneyError::Corrupt("invalid journey encoding"))?;
    validate_record(&record)?;
    Ok(record)
}

fn plan_digest(plan: &JourneyPlan, legs: &[LegRecord]) -> [u8; 32] {
    let mut digest = Sha256::new();
    digest.update(PLAN_DIGEST_DOMAIN);
    hash_text(&mut digest, plan.journey_id.as_str());
    digest.update([plan.kind.code()]);
    digest.update(plan.idempotency_key);
    hash_text(&mut digest, plan.custody_key.as_str());
    hash_text(&mut digest, plan.signing_operation.label());
    for leg in legs {
        digest.update(leg.action_key);
        digest.update(leg.activity_type.to_be_bytes());
        digest.update(leg.payload_hash);
        digest.update(Sha256::digest(&leg.payload));
        hash_text(&mut digest, &leg.preparation.actor);
        hash_text(&mut digest, &leg.preparation.authority);
        digest.update(leg.preparation.account_sequence.to_be_bytes());
        digest.update(leg.preparation.not_before.to_be_bytes());
        digest.update(leg.preparation.not_after.to_be_bytes());
        digest.update(leg.preparation.fee_limit.to_be_bytes());
    }
    digest.finalize().into()
}

fn prepare_digest(request: &PrepareRequest) -> [u8; 32] {
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

fn submit_digest(request: &SubmitRequest, signer_public_key: [u8; 32]) -> [u8; 32] {
    let mut digest = Sha256::new();
    digest.update(SUBMIT_DIGEST_DOMAIN);
    hash_text(&mut digest, request.preparation_ref.as_str());
    digest.update(Sha256::digest(request.signature.as_bytes()));
    digest.update(signer_public_key);
    match request.approval_release_ref {
        Some(reference) => {
            digest.update([1]);
            digest.update(reference);
        }
        None => digest.update([0]),
    }
    digest.finalize().into()
}

fn mutation<T>(
    action_key: [u8; 32],
    body_digest: [u8; 32],
    operation: T,
) -> Result<IdempotentMutation<T>, JourneyError> {
    let mut request = [0_u8; 8];
    request.copy_from_slice(&action_key[..8]);
    Ok(IdempotentMutation {
        request_id: RequestId(u64::from_be_bytes(request)),
        key: Key::new(action_key)?,
        body_digest: BodyDigest(body_digest),
        operation,
    })
}

fn operation_from_label(label: &str) -> Result<Operation, JourneyError> {
    match label {
        "protocol-mutation" => Ok(Operation::ProtocolMutation),
        "approval-decision" => Ok(Operation::ApprovalDecision),
        "security-settings" => Ok(Operation::SecuritySettings),
        "secret-reveal" => Ok(Operation::SecretReveal),
        "withdrawal" => Ok(Operation::Withdrawal),
        "emergency-exit" => Ok(Operation::EmergencyExit),
        "wallet-rebinding" => Ok(Operation::WalletRebinding),
        "agent-archive" => Ok(Operation::AgentArchive),
        _ => Err(JourneyError::Corrupt("unknown signing operation")),
    }
}

fn journey_row(idempotency_key: [u8; 32]) -> Result<RowKey, JourneyError> {
    Ok(RowKey::new(format!(
        "{JOURNEY_PREFIX}{}",
        hex(&idempotency_key)
    ))?)
}

fn event_row(prefix: &str, journey_id: &str, sequence: u64) -> Result<RowKey, JourneyError> {
    Ok(RowKey::new(format!(
        "{prefix}{}-{sequence:020}",
        journey_id.strip_prefix("jrn_").unwrap_or(journey_id)
    ))?)
}

fn events(
    scope: &PrincipalScope<'_>,
    prefix: &str,
    journey_id: &JourneyId,
) -> Result<Vec<JourneyProgress>, JourneyError> {
    let key_prefix = format!(
        "{prefix}{}-",
        journey_id
            .as_str()
            .strip_prefix("jrn_")
            .unwrap_or(journey_id.as_str())
    );
    let mut output = scope
        .keys(Table::Journeys)
        .into_iter()
        .filter(|key| key.as_str().starts_with(&key_prefix))
        .map(|key| {
            let row = scope
                .get(Table::Journeys, &key)
                .ok_or(JourneyError::Corrupt("progress disappeared while listing"))?;
            serde_json::from_slice::<JourneyProgress>(row.bytes())
                .map_err(|_| JourneyError::Corrupt("invalid progress encoding"))
        })
        .collect::<Result<Vec<_>, _>>()?;
    output.sort_by_key(JourneyProgress::sequence);
    Ok(output)
}

fn put_exact(
    scope: &mut PrincipalScope<'_>,
    key: RowKey,
    now: u64,
    bytes: &[u8],
) -> Result<(), JourneyError> {
    if let Some(existing) = scope.get(Table::Journeys, &key) {
        return if existing.bytes() == bytes {
            Ok(())
        } else {
            Err(JourneyError::EvidenceConflict)
        };
    }
    scope.put(Table::Journeys, key, now, bytes.to_vec())?;
    Ok(())
}

fn put_stream_progress(
    scope: &mut PrincipalScope<'_>,
    progress: &JourneyProgress,
    now: u64,
) -> Result<(), JourneyError> {
    let source = format!("journey:{}:{}", progress.journey_id(), progress.sequence());
    let source_digest: [u8; 32] = Sha256::digest(source.as_bytes()).into();
    let source_key = RowKey::new(format!("stream-source-{}", hex(&source_digest)))?;
    if scope.get(Table::Stream, &source_key).is_some() {
        return Ok(());
    }
    let head_key = RowKey::new("stream-head")?;
    let sequence = scope
        .get(Table::Stream, &head_key)
        .map(|row| {
            let bytes: [u8; 8] = row
                .bytes()
                .try_into()
                .map_err(|_| JourneyError::Corrupt("invalid stream head"))?;
            Ok::<u64, JourneyError>(u64::from_be_bytes(bytes))
        })
        .transpose()?
        .unwrap_or(0)
        .checked_add(1)
        .ok_or(JourneyError::SequenceOverflow)?;
    let event = serde_json::json!({"sequence":sequence,"source":source,"kind":"journey-progress","observed_at":now,"payload":{}});
    let event_bytes = serde_json::to_vec(&event)
        .map_err(|_| JourneyError::Corrupt("stream event cannot be encoded"))?;
    scope.put(
        Table::Stream,
        RowKey::new(format!("stream-event-{sequence:016x}"))?,
        now,
        event_bytes,
    )?;
    scope.put(
        Table::Stream,
        source_key,
        now,
        sequence.to_be_bytes().to_vec(),
    )?;
    scope.put(
        Table::Stream,
        head_key,
        now,
        sequence.to_be_bytes().to_vec(),
    )?;
    Ok(())
}

fn hash_text(digest: &mut Sha256, value: &str) {
    digest.update(u32::try_from(value.len()).unwrap_or(u32::MAX).to_be_bytes());
    digest.update(value.as_bytes());
}

fn hex(bytes: &[u8]) -> String {
    let mut output = String::with_capacity(bytes.len().saturating_mul(2));
    for byte in bytes {
        use std::fmt::Write as _;
        let _ = write!(output, "{byte:02x}");
    }
    output
}

/// Typed journey failure. No error variant is rendered as an economic result.
#[derive(Debug)]
pub enum JourneyError {
    Store(StoreError),
    Custody(CustodyError),
    Contract(ContractError),
    Compile(CompileError),
    Disclosure(DisclosureCheckError),
    Payload(PayloadError),
    Verification(VerificationFailure),
    Sdk(SdkError),
    Agent(AgentBoundaryError),
    InvalidPlan,
    IdempotencyConflict,
    TimeRegressed,
    PreparationMismatch,
    ObservationMismatch,
    ReceiptRequired,
    ReceiptShape,
    ReceiptMismatch,
    ReceiptReuse,
    EvidenceConflict,
    SequenceOverflow,
    Corrupt(&'static str),
}

impl Display for JourneyError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Store(error) => write!(formatter, "journey store failure: {error}"),
            Self::Custody(error) => write!(formatter, "journey custody failure: {error}"),
            Self::Contract(error) => write!(formatter, "agent contract failure: {error:?}"),
            Self::Compile(error) => write!(formatter, "journey compile failure: {error:?}"),
            Self::Disclosure(error) => write!(formatter, "journey disclosure failure: {error:?}"),
            Self::Payload(error) => write!(formatter, "journey payload failure: {error:?}"),
            Self::Verification(error) => write!(formatter, "journey receipt failure: {error:?}"),
            Self::Sdk(error) => write!(formatter, "agent SDK failure: {error:?}"),
            Self::Agent(error) => write!(formatter, "agent boundary failure: {error:?}"),
            Self::InvalidPlan => formatter.write_str("journey plan is invalid"),
            Self::IdempotencyConflict => {
                formatter.write_str("idempotency key owns another journey")
            }
            Self::TimeRegressed => formatter.write_str("journey time regressed"),
            Self::PreparationMismatch => {
                formatter.write_str("agent preparation differs from the typed intent")
            }
            Self::ObservationMismatch => formatter.write_str("agent observation names another leg"),
            Self::ReceiptRequired => formatter.write_str("executed leg lacks receipt material"),
            Self::ReceiptShape => formatter.write_str("receipt is not a protocol receipt"),
            Self::ReceiptMismatch => formatter.write_str("receipt does not prove the current leg"),
            Self::ReceiptReuse => {
                formatter.write_str("one receipt cannot complete two journey legs")
            }
            Self::EvidenceConflict => formatter.write_str("durable journey evidence conflicts"),
            Self::SequenceOverflow => formatter.write_str("journey event sequence overflowed"),
            Self::Corrupt(reason) => write!(formatter, "corrupt journey: {reason}"),
        }
    }
}

impl std::error::Error for JourneyError {}

impl From<StoreError> for JourneyError {
    fn from(value: StoreError) -> Self {
        Self::Store(value)
    }
}
impl From<CustodyError> for JourneyError {
    fn from(value: CustodyError) -> Self {
        Self::Custody(value)
    }
}
impl From<ContractError> for JourneyError {
    fn from(value: ContractError) -> Self {
        Self::Contract(value)
    }
}
impl From<CompileError> for JourneyError {
    fn from(value: CompileError) -> Self {
        Self::Compile(value)
    }
}
impl From<DisclosureCheckError> for JourneyError {
    fn from(value: DisclosureCheckError) -> Self {
        Self::Disclosure(value)
    }
}
impl From<PayloadError> for JourneyError {
    fn from(value: PayloadError) -> Self {
        Self::Payload(value)
    }
}
impl From<VerificationFailure> for JourneyError {
    fn from(value: VerificationFailure) -> Self {
        Self::Verification(value)
    }
}
impl From<SdkError> for JourneyError {
    fn from(value: SdkError) -> Self {
        Self::Sdk(value)
    }
}
impl From<AgentBoundaryError> for JourneyError {
    fn from(value: AgentBoundaryError) -> Self {
        Self::Agent(value)
    }
}
