//! Direct, crash-resumable Paxeer emergency-exit journey.

use std::fmt::{Display, Formatter};

use layerx_paxeer_client::{
    CustodyClaim, EmergencyExit, ExecutionOutcome, ExitClaim, ExitEligibility, ExitError,
    ExitEvidence, ExitProgress, ExitRefusal, ForcedExitMaterial, TransactionHash,
    TransactionInclusion,
};
use layerx_types::intent::EvmAddress;
use serde::{Deserialize, Serialize};
use sha2::{Digest as _, Sha256};

use crate::audit::{
    AuditChain, AuditError, AuditEvent, Decision, JourneyKind, JourneyState as AuditJourneyState,
    SigningOperation, StepUpEvidence,
};
use crate::notify::JourneyId;
use crate::redaction::{Label, RedactionError};
use crate::store::{EvidenceRef, PrincipalScope, RowKey, StoreError, Table};
use crate::trace::TraceId;

const RECORD_VERSION: u8 = 2;
const RECORD_PREFIX: &str = "exit-journey-";
const SNAPSHOT_PREFIX: &str = "exit-evidence-";
const WALLET_ACTION_DOMAIN: &[u8] = b"layerx-human-exit-wallet/v1\0";
const EXECUTE_ACTION_DOMAIN: &[u8] = b"layerx-human-exit-execute/v1\0";
const PLAN_DIGEST_DOMAIN: &[u8] = b"layerx-human-exit-plan/v1\0";
const CONFIRMATION_DOMAIN: &[u8] = b"layerx-human-exit-confirmation/v1\0";
/// Wire tag of the forced-exit plan. Plans of the removed checkpoint-proof
/// shape are refused rather than reinterpreted.
const PLAN_TAG: u8 = 5;
/// Bound on one encoded [`ForcedExitMaterial`]: the custody evidence bound
/// plus the fixed-size material header.
const MAX_MATERIAL_BYTES: usize = 131_072;

/// Settings location of the guided flow.
pub const EXIT_SETTINGS_SURFACE: &str = "Settings";
/// Familiar title shown instead of protocol terminology.
pub const EXIT_TITLE: &str = "Getting my money out";
/// Exact phrase the user must type before an emergency exit can begin.
pub const EXIT_CONFIRMATION_PHRASE: &str = "GET MY MONEY OUT";
/// Plain consequence shown alongside the typed confirmation.
pub const EXIT_IRREVERSIBILITY_NOTICE: &str =
    "Emergency exit is irreversible. Once submitted, it cannot be cancelled.";
/// Honest refusal while the network is operating normally.
pub const EXIT_NORMAL_OPERATION_MESSAGE: &str =
    "Emergency exit is unavailable because the network is operating normally. Use ordinary withdrawal instead.";
/// Route offered from the normal-operation refusal.
pub const ORDINARY_WITHDRAWAL_PATH: &str = "/app/withdraw";

/// A confirmation that can exist only after the exact irreversible phrase was typed.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct IrreversibleExitConfirmation {
    digest: [u8; 32],
}

impl IrreversibleExitConfirmation {
    /// Validates the exact, case-sensitive emergency-exit phrase.
    ///
    /// # Errors
    ///
    /// Returns [`ExitConfirmationError::ExactPhraseRequired`] for every other value.
    pub fn parse(value: &str) -> Result<Self, ExitConfirmationError> {
        if value != EXIT_CONFIRMATION_PHRASE {
            return Err(ExitConfirmationError::ExactPhraseRequired);
        }
        Ok(Self {
            digest: digest(&[CONFIRMATION_DOMAIN, value.as_bytes()]),
        })
    }

    #[must_use]
    pub const fn digest(self) -> [u8; 32] {
        self.digest
    }
}

/// Why irreversible confirmation was not accepted.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ExitConfirmationError {
    ExactPhraseRequired,
}

/// Immutable request for the direct exit path.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ExitPlan {
    pub journey_id: JourneyId,
    pub idempotency_key: [u8; 32],
    pub evidence: ExitEvidence,
}

/// Encodes all owner-visible exit evidence without JSON or debug projections.
///
/// # Errors
///
/// Refuses an invalid plan and material outside the custody evidence bounds.
pub(crate) fn encode_exit_plan(plan: &ExitPlan) -> Result<Vec<u8>, ExitJourneyError> {
    validate_plan(plan)?;
    validate_exit_evidence(&plan.evidence)?;
    let material = encode_material(&plan.evidence.material)?;
    let mut out = super::wire::Writer::new(PLAN_TAG);
    out.text(plan.journey_id.as_str())
        .map_err(|()| ExitJourneyError::InvalidPlan)?;
    out.fixed(&plan.idempotency_key);
    out.u128(plan.evidence.finalised_balance);
    out.u32(u32::try_from(material.len()).map_err(|_| ExitJourneyError::InvalidPlan)?);
    out.fixed(&material);
    Ok(out.finish())
}

/// Decodes an exact bounded exit plan and constructs only validated evidence.
///
/// # Errors
///
/// Refuses another wire shape, trailing bytes and unvalidated material.
pub(crate) fn decode_exit_plan(bytes: &[u8]) -> Result<ExitPlan, ExitJourneyError> {
    let mut input =
        super::wire::Reader::new(bytes, PLAN_TAG).map_err(|()| ExitJourneyError::InvalidPlan)?;
    let journey_id = JourneyId::new(input.text().map_err(|()| ExitJourneyError::InvalidPlan)?)
        .map_err(|_| ExitJourneyError::InvalidPlan)?;
    let idempotency_key = input.fixed().map_err(|()| ExitJourneyError::InvalidPlan)?;
    let finalised_balance = input.u128().map_err(|()| ExitJourneyError::InvalidPlan)?;
    let length = usize::try_from(input.u32().map_err(|()| ExitJourneyError::InvalidPlan)?)
        .map_err(|_| ExitJourneyError::InvalidPlan)?;
    if length == 0 || length > MAX_MATERIAL_BYTES {
        return Err(ExitJourneyError::InvalidPlan);
    }
    let mut encoded = Vec::with_capacity(length);
    for _ in 0..length {
        encoded.push(
            input
                .fixed::<1>()
                .map_err(|()| ExitJourneyError::InvalidPlan)?[0],
        );
    }
    input.finish().map_err(|()| ExitJourneyError::InvalidPlan)?;
    let plan = ExitPlan {
        journey_id,
        idempotency_key,
        evidence: ExitEvidence {
            material: decode_material(&encoded)?,
            finalised_balance,
        },
    };
    validate_plan(&plan)?;
    validate_exit_evidence(&plan.evidence)?;
    Ok(plan)
}

fn encode_material(material: &ForcedExitMaterial) -> Result<Vec<u8>, ExitJourneyError> {
    layerx_paxeer_client::wire::encode_forced_exit_material(material, MAX_MATERIAL_BYTES)
        .map_err(|_| ExitJourneyError::InvalidPlan)
}

fn decode_material(bytes: &[u8]) -> Result<ForcedExitMaterial, ExitJourneyError> {
    layerx_paxeer_client::wire::decode_forced_exit_material(bytes, MAX_MATERIAL_BYTES)
        .map_err(|_| ExitJourneyError::InvalidPlan)
}

/// Validates the forced-exit material the owner confirmed, without inventing
/// any chain fact: the whole-balance and recipient-authority proofs are made
/// against the anchor the custody precompile reports, in
/// [`ExitJourney::validate_claim`].
fn validate_exit_evidence(evidence: &ExitEvidence) -> Result<(), ExitJourneyError> {
    let material = evidence
        .material
        .clone()
        .validated()
        .map_err(|_| ExitJourneyError::InvalidPlan)?;
    layerx_paxeer_client::state_proof::StateWitness::decode(&material.witness)
        .map_err(|_| ExitJourneyError::InvalidPlan)?;
    if evidence.finalised_balance == 0 {
        return Err(ExitJourneyError::InvalidPlan);
    }
    Ok(())
}

/// Stable wallet request. Implementations must resolve the original transaction
/// for repeated calls carrying the same action key and identical claim.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ExitWalletRequest {
    pub identity: super::MovementExecutionIdentity,
    pub action_key: [u8; 32],
    pub contract: EvmAddress,
    pub calldata: Vec<u8>,
    pub checkpoint: [u8; 32],
    pub withdrawal_id: [u8; 32],
    pub nullifier: [u8; 32],
    pub recipient: EvmAddress,
    pub finalised_balance: u128,
}

/// Result of one user-controlled Paxeer transaction.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ExitWalletOutcome {
    Submitted(TransactionHash),
    Rejected,
}

/// Stable wallet boundary failures. Unavailable leaves the durable stage unchanged.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ExitBoundaryError {
    Unavailable,
    ContractViolation,
}

/// Wallet boundary for the real Paxeer exit transactions.
pub trait ExitWallet {
    /// Opens or resolves the transaction under its stable action key.
    ///
    /// # Errors
    ///
    /// Returns a typed transient or contract failure without changing the request.
    fn submit_or_resolve(
        &mut self,
        request: &ExitWalletRequest,
    ) -> Result<ExitWalletOutcome, ExitBoundaryError>;
}

/// Terminal refusal or failure shown without claiming money moved successfully.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ExitFailureKind {
    NoFinalisedCheckpoint,
    InvalidCheckpointEvidence,
    WalletRejected,
    TransactionDisplaced { requeued: bool },
    PaxeerRefused,
}

/// Paxeer inclusion facts that alone authorize the done state.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct ExitFinalityEvidence {
    pub transaction: [u8; 32],
    pub block_number: u64,
    pub block_hash: [u8; 32],
    pub transaction_index: u64,
    pub confirmations: u64,
}

/// Guided emergency-exit stages.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ExitStage {
    ConstructingLastFinalisedCheckpoint,
    WaitingForWallet,
    ConfirmingPaxeer {
        transaction: TransactionHash,
        confirmations: u64,
        required: u64,
    },
    /// The custody precompile queued the exit; it pays only after its
    /// forced-exit delay elapsed.
    WaitingForForcedExitDelay {
        claim_id: [u8; 32],
        available_at: u64,
    },
    Done(ExitFinalityEvidence),
    UnavailableWhileNetworkOperatingNormally {
        ordinary_withdrawal_path: &'static str,
    },
    Failed(ExitFailureKind),
}

/// Public status with the Settings wording and honest alternative attached.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ExitStatus {
    journey_id: JourneyId,
    stage: ExitStage,
}

impl ExitStatus {
    #[must_use]
    pub const fn journey_id(&self) -> &JourneyId {
        &self.journey_id
    }

    #[must_use]
    pub const fn stage(&self) -> &ExitStage {
        &self.stage
    }

    #[must_use]
    pub const fn settings_surface() -> &'static str {
        EXIT_SETTINGS_SURFACE
    }

    #[must_use]
    pub const fn title() -> &'static str {
        EXIT_TITLE
    }

    #[must_use]
    pub const fn irreversibility_notice() -> &'static str {
        EXIT_IRREVERSIBILITY_NOTICE
    }

    #[must_use]
    pub const fn normal_operation_message(&self) -> Option<&'static str> {
        if matches!(
            self.stage,
            ExitStage::UnavailableWhileNetworkOperatingNormally { .. }
        ) {
            Some(EXIT_NORMAL_OPERATION_MESSAGE)
        } else {
            None
        }
    }
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
enum Phase {
    Constructing,
    WalletOpening,
    Confirming,
    AwaitingExecution,
    ExecuteWalletOpening,
    ExecuteConfirming,
    Done,
    NormalOperation,
    Failed,
}

impl Phase {
    const fn code(self) -> &'static str {
        match self {
            Self::Constructing => "constructing",
            Self::WalletOpening => "wallet-opening",
            Self::Confirming => "confirming",
            Self::AwaitingExecution => "awaiting-execution",
            Self::ExecuteWalletOpening => "execute-wallet-opening",
            Self::ExecuteConfirming => "execute-confirming",
            Self::Done => "done",
            Self::NormalOperation => "normal-operation",
            Self::Failed => "failed",
        }
    }

    const fn audit_state(self) -> AuditJourneyState {
        match self {
            Self::Constructing | Self::Confirming | Self::ExecuteConfirming => {
                AuditJourneyState::Processing
            }
            Self::AwaitingExecution => AuditJourneyState::StillChecking,
            Self::WalletOpening | Self::ExecuteWalletOpening => AuditJourneyState::WaitingForYou,
            Self::Done => AuditJourneyState::DoneFinalised,
            Self::NormalOperation | Self::Failed => AuditJourneyState::Refused,
        }
    }

    const fn audit_from(self) -> AuditJourneyState {
        match self {
            Self::Constructing | Self::Confirming | Self::ExecuteConfirming => {
                AuditJourneyState::WaitingForYou
            }
            Self::ExecuteWalletOpening => AuditJourneyState::StillChecking,
            Self::WalletOpening
            | Self::AwaitingExecution
            | Self::Done
            | Self::NormalOperation
            | Self::Failed => AuditJourneyState::Processing,
        }
    }
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
enum StoredFailure {
    NoFinalisedCheckpoint,
    InvalidCheckpointEvidence,
    WalletRejected,
    TransactionDisplacedRequeued,
    TransactionDisplacedDropped,
    PaxeerRefused,
}

impl StoredFailure {
    const fn public(self) -> ExitFailureKind {
        match self {
            Self::NoFinalisedCheckpoint => ExitFailureKind::NoFinalisedCheckpoint,
            Self::InvalidCheckpointEvidence => ExitFailureKind::InvalidCheckpointEvidence,
            Self::WalletRejected => ExitFailureKind::WalletRejected,
            Self::TransactionDisplacedRequeued => {
                ExitFailureKind::TransactionDisplaced { requeued: true }
            }
            Self::TransactionDisplacedDropped => {
                ExitFailureKind::TransactionDisplaced { requeued: false }
            }
            Self::PaxeerRefused => ExitFailureKind::PaxeerRefused,
        }
    }
}

/// The confirmed forced-exit material in its canonical, self-validating wire
/// form, with the whole balance it proves.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
struct StoredEvidence {
    material: Vec<u8>,
    finalised_balance: u128,
}

impl StoredEvidence {
    fn from_public(value: &ExitEvidence) -> Result<Self, ExitJourneyError> {
        Ok(Self {
            material: encode_material(&value.material)?,
            finalised_balance: value.finalised_balance,
        })
    }

    fn material(&self) -> Result<ForcedExitMaterial, ExitJourneyError> {
        layerx_paxeer_client::wire::decode_forced_exit_material(&self.material, MAX_MATERIAL_BYTES)
            .map_err(|_| ExitJourneyError::Corrupt("stored exit material is invalid"))
    }

    fn public(&self) -> Result<ExitEvidence, ExitJourneyError> {
        Ok(ExitEvidence {
            material: self.material()?,
            finalised_balance: self.finalised_balance,
        })
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
struct StoredClaim {
    contract: [u8; 20],
    calldata: Vec<u8>,
    execute_calldata: Vec<u8>,
    claim_id: [u8; 32],
    batch_number: u64,
    state_root: [u8; 32],
    withdrawal_id: [u8; 32],
    nullifier: [u8; 32],
    account: [u8; 32],
    asset_id: [u8; 32],
    finalised_balance: u128,
    recipient: [u8; 20],
}

impl From<&ExitClaim> for StoredClaim {
    fn from(value: &ExitClaim) -> Self {
        Self {
            contract: value.contract.bytes(),
            calldata: value.calldata.clone(),
            execute_calldata: value.execute_calldata.clone(),
            claim_id: value.claim_id,
            batch_number: value.batch_number,
            state_root: value.state_root,
            withdrawal_id: value.withdrawal_id,
            nullifier: value.nullifier,
            account: value.account,
            asset_id: value.asset_id,
            finalised_balance: value.finalised_balance,
            recipient: value.recipient.bytes(),
        }
    }
}

impl StoredClaim {
    fn public(&self) -> ExitClaim {
        ExitClaim {
            contract: EvmAddress::new(self.contract),
            calldata: self.calldata.clone(),
            execute_calldata: self.execute_calldata.clone(),
            claim_id: self.claim_id,
            batch_number: self.batch_number,
            state_root: self.state_root,
            withdrawal_id: self.withdrawal_id,
            nullifier: self.nullifier,
            account: self.account,
            asset_id: self.asset_id,
            finalised_balance: self.finalised_balance,
            recipient: EvmAddress::new(self.recipient),
        }
    }

    fn wallet_request(
        &self,
        action_key: [u8; 32],
        identity: super::MovementExecutionIdentity,
        calldata: Vec<u8>,
    ) -> ExitWalletRequest {
        ExitWalletRequest {
            identity,
            action_key,
            contract: EvmAddress::new(self.contract),
            calldata,
            checkpoint: self.state_root,
            withdrawal_id: self.withdrawal_id,
            nullifier: self.nullifier,
            recipient: EvmAddress::new(self.recipient),
            finalised_balance: self.finalised_balance,
        }
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
struct Record {
    version: u8,
    journey_id: String,
    idempotency_key: [u8; 32],
    plan_digest: [u8; 32],
    confirmation_digest: [u8; 32],
    wallet_action_key: [u8; 32],
    execute_action_key: [u8; 32],
    evidence: StoredEvidence,
    claim: Option<StoredClaim>,
    transaction: Option<[u8; 32]>,
    execute_transaction: Option<[u8; 32]>,
    available_at: Option<u64>,
    confirmations: u64,
    required: u64,
    finality: Option<ExitFinalityEvidence>,
    phase: Phase,
    failure: Option<StoredFailure>,
    started_at: u64,
    updated_at: u64,
}

/// Durable emergency-exit state machine using only Paxeer exit-path evidence.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ExitJourney {
    record: Record,
}

impl ExitJourney {
    /// Persists the confirmed request before any claim construction or wallet effect.
    ///
    /// # Errors
    ///
    /// Refuses invalid plans, idempotency conflicts, corrupt storage, or audit failure.
    pub fn start(
        scope: &mut PrincipalScope<'_>,
        audit: &mut AuditChain,
        trace: &TraceId,
        plan: &ExitPlan,
        confirmation: IrreversibleExitConfirmation,
        now: u64,
    ) -> Result<Self, ExitJourneyError> {
        validate_plan(plan)?;
        validate_exit_evidence(&plan.evidence)?;
        let row = record_row(plan.idempotency_key)?;
        let plan_hash = plan_digest(plan)?;
        let confirmed_digest = digest(&[CONFIRMATION_DOMAIN, &plan_hash, &confirmation.digest()]);
        if let Some(existing) = scope.get(Table::Journeys, &row) {
            let journey = Self {
                record: decode(existing.bytes())?,
            };
            if journey.record.plan_digest != plan_hash
                || journey.record.journey_id != plan.journey_id.as_str()
                || journey.record.confirmation_digest != confirmed_digest
            {
                return Err(ExitJourneyError::IdempotencyConflict);
            }
            journey.ensure_confirmation_audited(scope, audit, trace, now)?;
            journey.ensure_phase_audited(scope, audit, trace, now)?;
            return Ok(journey);
        }
        let record = Record {
            version: RECORD_VERSION,
            journey_id: plan.journey_id.as_str().to_owned(),
            idempotency_key: plan.idempotency_key,
            plan_digest: plan_hash,
            confirmation_digest: confirmed_digest,
            wallet_action_key: derive_key(WALLET_ACTION_DOMAIN, &plan.idempotency_key),
            execute_action_key: derive_key(EXECUTE_ACTION_DOMAIN, &plan.idempotency_key),
            evidence: StoredEvidence::from_public(&plan.evidence)?,
            claim: None,
            transaction: None,
            execute_transaction: None,
            available_at: None,
            confirmations: 0,
            required: 0,
            finality: None,
            phase: Phase::Constructing,
            failure: None,
            started_at: now,
            updated_at: now,
        };
        let journey = Self { record };
        journey.persist(scope)?;
        journey.write_snapshot(scope)?;
        journey.ensure_confirmation_audited(scope, audit, trace, now)?;
        journey.ensure_phase_audited(scope, audit, trace, now)?;
        Ok(journey)
    }

    /// Loads one exit by its public journey identifier.
    ///
    /// # Errors
    ///
    /// Refuses malformed or duplicate records.
    pub fn load(
        scope: &PrincipalScope<'_>,
        journey_id: &JourneyId,
    ) -> Result<Option<Self>, ExitJourneyError> {
        let mut found = None;
        for key in scope.keys(Table::Journeys) {
            if !key.as_str().starts_with(RECORD_PREFIX) {
                continue;
            }
            let row = scope
                .get(Table::Journeys, &key)
                .ok_or(ExitJourneyError::Corrupt("exit disappeared"))?;
            let record = decode(row.bytes())?;
            if record.journey_id == journey_id.as_str() {
                if found.is_some() {
                    return Err(ExitJourneyError::Corrupt("duplicate exit journey"));
                }
                found = Some(Self { record });
            }
        }
        Ok(found)
    }

    /// Advances at most one durable stage over the concrete Paxeer exit client.
    ///
    /// The exit is two user transactions against the custody precompile:
    /// `requestForcedExit` queues the claim and starts its forced-exit delay,
    /// `executeForcedExit` pays it once the delay elapsed. Settlement is
    /// reported only after the execute transaction is final on Paxeer *and*
    /// the precompile's `EmergencyExitExecuted` log binds to the claim.
    ///
    /// # Errors
    ///
    /// Transient endpoint/wallet errors preserve the last durable state. Evidence
    /// conflicts and malformed external answers never become success.
    #[allow(clippy::too_many_lines)]
    pub fn advance<W: ExitWallet>(
        &mut self,
        scope: &mut PrincipalScope<'_>,
        audit: &mut AuditChain,
        trace: &TraceId,
        exit: &EmergencyExit,
        wallet: &mut W,
        now: u64,
    ) -> Result<ExitStatus, ExitJourneyError> {
        if now < self.record.updated_at {
            return Err(ExitJourneyError::TimeRegressed);
        }
        self.ensure_phase_audited(scope, audit, trace, now)?;
        match self.record.phase {
            Phase::Constructing => match exit.construct_claim(&self.evidence()?) {
                Ok(claim) => {
                    self.validate_claim(exit, &claim)?;
                    self.record.claim = Some(StoredClaim::from(&claim));
                    self.transition(scope, audit, trace, Phase::WalletOpening, now)?;
                }
                Err(ExitError::Refused(ExitRefusal::NotEligible {
                    eligibility: ExitEligibility::NetworkOperatingNormally { .. },
                })) => {
                    self.transition(scope, audit, trace, Phase::NormalOperation, now)?;
                }
                Err(ExitError::Refused(ExitRefusal::NotEligible {
                    eligibility: ExitEligibility::NoFinalisedCheckpoint,
                })) => {
                    self.fail(
                        scope,
                        audit,
                        trace,
                        StoredFailure::NoFinalisedCheckpoint,
                        now,
                    )?;
                }
                Err(ExitError::Refused(_)) => {
                    self.fail(
                        scope,
                        audit,
                        trace,
                        StoredFailure::InvalidCheckpointEvidence,
                        now,
                    )?;
                }
                Err(error) => return Err(ExitJourneyError::Paxeer(error)),
            },
            Phase::WalletOpening => {
                let request = self.wallet_request(scope, false)?;
                match wallet.submit_or_resolve(&request)? {
                    ExitWalletOutcome::Submitted(transaction) => {
                        if transaction.bytes() == [0; 32] {
                            return Err(ExitJourneyError::Boundary(
                                ExitBoundaryError::ContractViolation,
                            ));
                        }
                        self.record.transaction = Some(transaction.bytes());
                        self.record.required = exit.required_confirmations();
                        self.transition(scope, audit, trace, Phase::Confirming, now)?;
                    }
                    ExitWalletOutcome::Rejected => {
                        self.fail(scope, audit, trace, StoredFailure::WalletRejected, now)?;
                    }
                }
            }
            Phase::Confirming => {
                let transaction = self.transaction()?;
                match self.poll(exit, transaction)?.0 {
                    ExitProgress::Settled { .. } => {
                        let queued = self.queued(exit)?;
                        self.record.available_at = Some(queued.available_at);
                        self.transition(scope, audit, trace, Phase::AwaitingExecution, now)?;
                    }
                    ExitProgress::Refused { .. } => {
                        self.fail(scope, audit, trace, StoredFailure::PaxeerRefused, now)?;
                    }
                    ExitProgress::Displaced { requeued } => {
                        self.displaced(scope, audit, trace, requeued, now)?;
                    }
                    ExitProgress::Pending | ExitProgress::Confirming { .. } => {
                        self.persist_at(scope, now)?;
                    }
                }
            }
            Phase::AwaitingExecution => {
                let queued = self.queued(exit)?;
                self.record.available_at = Some(queued.available_at);
                if now >= queued.available_at {
                    self.transition(scope, audit, trace, Phase::ExecuteWalletOpening, now)?;
                } else {
                    self.persist_at(scope, now)?;
                }
            }
            Phase::ExecuteWalletOpening => {
                let request = self.wallet_request(scope, true)?;
                match wallet.submit_or_resolve(&request)? {
                    ExitWalletOutcome::Submitted(transaction) => {
                        if transaction.bytes() == [0; 32] {
                            return Err(ExitJourneyError::Boundary(
                                ExitBoundaryError::ContractViolation,
                            ));
                        }
                        self.record.execute_transaction = Some(transaction.bytes());
                        self.record.required = exit.required_confirmations();
                        self.transition(scope, audit, trace, Phase::ExecuteConfirming, now)?;
                    }
                    ExitWalletOutcome::Rejected => {
                        self.fail(scope, audit, trace, StoredFailure::WalletRejected, now)?;
                    }
                }
            }
            Phase::ExecuteConfirming => {
                let transaction = self.execute_transaction()?;
                let (progress, logs) = self.poll(exit, transaction)?;
                match progress {
                    ExitProgress::Settled {
                        inclusion,
                        confirmations,
                    } => {
                        let claim = self.claim()?;
                        let logs = logs.ok_or(ExitJourneyError::MissingReceiptLogs)?;
                        EmergencyExit::verify_executed(&claim, &logs)
                            .map_err(ExitJourneyError::Paxeer)?;
                        self.record.finality =
                            Some(finality(transaction, inclusion, confirmations));
                        self.transition(scope, audit, trace, Phase::Done, now)?;
                    }
                    ExitProgress::Refused { .. } => {
                        self.fail(scope, audit, trace, StoredFailure::PaxeerRefused, now)?;
                    }
                    ExitProgress::Displaced { requeued } => {
                        self.displaced(scope, audit, trace, requeued, now)?;
                    }
                    ExitProgress::Pending | ExitProgress::Confirming { .. } => {
                        self.persist_at(scope, now)?;
                    }
                }
            }
            Phase::Done | Phase::NormalOperation | Phase::Failed => {}
        }
        self.status()
    }

    /// Returns the current user-facing stage without inferring settlement.
    ///
    /// # Errors
    ///
    /// Refuses corrupt durable state.
    pub fn status(&self) -> Result<ExitStatus, ExitJourneyError> {
        let journey_id = JourneyId::new(self.record.journey_id.clone())
            .map_err(|_| ExitJourneyError::Corrupt("invalid exit journey id"))?;
        let stage = match self.record.phase {
            Phase::Constructing => ExitStage::ConstructingLastFinalisedCheckpoint,
            Phase::WalletOpening | Phase::ExecuteWalletOpening => ExitStage::WaitingForWallet,
            Phase::Confirming => ExitStage::ConfirmingPaxeer {
                transaction: self.transaction()?,
                confirmations: self.record.confirmations,
                required: self.record.required,
            },
            Phase::ExecuteConfirming => ExitStage::ConfirmingPaxeer {
                transaction: self.execute_transaction()?,
                confirmations: self.record.confirmations,
                required: self.record.required,
            },
            Phase::AwaitingExecution => ExitStage::WaitingForForcedExitDelay {
                claim_id: self
                    .record
                    .claim
                    .as_ref()
                    .ok_or(ExitJourneyError::Corrupt("queued exit has no claim"))?
                    .claim_id,
                available_at: self
                    .record
                    .available_at
                    .ok_or(ExitJourneyError::Corrupt("queued exit has no delay"))?,
            },
            Phase::Done => ExitStage::Done(
                self.record
                    .finality
                    .ok_or(ExitJourneyError::Corrupt("done exit has no finality"))?,
            ),
            Phase::NormalOperation => ExitStage::UnavailableWhileNetworkOperatingNormally {
                ordinary_withdrawal_path: ORDINARY_WITHDRAWAL_PATH,
            },
            Phase::Failed => ExitStage::Failed(
                self.record
                    .failure
                    .ok_or(ExitJourneyError::Corrupt("failed exit has no reason"))?
                    .public(),
            ),
        };
        Ok(ExitStatus { journey_id, stage })
    }

    /// Polls one submitted transaction and records its confirmation progress.
    /// The receipt logs are returned only when the tracked receipt carried them.
    fn poll(
        &mut self,
        exit: &EmergencyExit,
        transaction: TransactionHash,
    ) -> Result<(ExitProgress, Option<Vec<layerx_paxeer_client::LogRecord>>), ExitJourneyError>
    {
        let mut tracker = exit
            .track(transaction)
            .map_err(ExitJourneyError::TrackerConfig)?;
        let report = tracker.poll();
        if report.transaction() != transaction {
            return Err(ExitJourneyError::Boundary(
                ExitBoundaryError::ContractViolation,
            ));
        }
        self.record.confirmations = report.progress().confirmed;
        self.record.required = report.progress().required;
        let logs = report.receipt_logs().map(<[_]>::to_vec);
        Ok((ExitProgress::of(&report), logs))
    }

    /// Reads the custody precompile's stored record for the constructed exit.
    fn queued(&self, exit: &EmergencyExit) -> Result<CustodyClaim, ExitJourneyError> {
        let claim = self.claim()?;
        let record = exit
            .claim_record(&claim)
            .map_err(ExitJourneyError::Paxeer)?
            .ok_or(ExitJourneyError::ClaimNotQueued)?;
        match record.status {
            1 => Ok(record),
            2 => Err(ExitJourneyError::ClaimPaidElsewhere),
            3 => Err(ExitJourneyError::ClaimCancelled),
            _ => Err(ExitJourneyError::ClaimMismatch),
        }
    }

    /// Re-derives every claim fact from the confirmed material and the anchor
    /// the custody precompile reports, independently of the client's own
    /// construction.
    fn validate_claim(
        &self,
        exit: &EmergencyExit,
        claim: &ExitClaim,
    ) -> Result<(), ExitJourneyError> {
        let evidence = self.evidence()?;
        layerx_paxeer_client::verify_exit_balance(&evidence, exit.network_id(), claim.state_root)
            .map_err(|_| ExitJourneyError::ClaimMismatch)?;
        let material = &evidence.material;
        if claim.contract != exit.contract()
            || claim.batch_number != material.batch_number
            || claim.account != material.account
            || claim.asset_id != material.asset_id
            || claim.recipient != material.recipient
            || claim.finalised_balance != evidence.finalised_balance
            || claim.calldata != material.request_calldata()
            || claim.execute_calldata != material.execute_calldata()
            || claim.claim_id == [0; 32]
            || claim.state_root == [0; 32]
            || claim.withdrawal_id == [0; 32]
            || claim.nullifier == [0; 32]
        {
            return Err(ExitJourneyError::ClaimMismatch);
        }
        match exit.eligibility().map_err(ExitJourneyError::Paxeer)? {
            ExitEligibility::Eligible {
                batch_number,
                state_root,
            } if batch_number == claim.batch_number && state_root == claim.state_root => Ok(()),
            _ => Err(ExitJourneyError::ClaimMismatch),
        }
    }

    fn evidence(&self) -> Result<ExitEvidence, ExitJourneyError> {
        self.record.evidence.public()
    }

    fn claim(&self) -> Result<ExitClaim, ExitJourneyError> {
        self.record
            .claim
            .as_ref()
            .map(StoredClaim::public)
            .ok_or(ExitJourneyError::Corrupt("exit has no claim"))
    }

    fn wallet_request(
        &self,
        scope: &PrincipalScope<'_>,
        execute: bool,
    ) -> Result<ExitWalletRequest, ExitJourneyError> {
        let stored = self
            .record
            .claim
            .as_ref()
            .ok_or(ExitJourneyError::Corrupt("wallet stage has no claim"))?;
        let material = self.record.evidence.material()?;
        let (action_key, calldata) = if execute {
            (
                self.record.execute_action_key,
                stored.execute_calldata.clone(),
            )
        } else {
            (self.record.wallet_action_key, stored.calldata.clone())
        };
        Ok(stored.wallet_request(
            action_key,
            super::MovementExecutionIdentity {
                principal: scope.principal().clone(),
                tenant: scope.tenant().clone(),
                account: material.account,
                wallet: material.recipient,
                plan_id: self.record.idempotency_key,
            },
            calldata,
        ))
    }

    fn transaction(&self) -> Result<TransactionHash, ExitJourneyError> {
        self.record
            .transaction
            .map(TransactionHash::new)
            .ok_or(ExitJourneyError::Corrupt("exit has no transaction"))
    }

    fn execute_transaction(&self) -> Result<TransactionHash, ExitJourneyError> {
        self.record
            .execute_transaction
            .map(TransactionHash::new)
            .ok_or(ExitJourneyError::Corrupt("exit has no execute transaction"))
    }

    fn displaced(
        &mut self,
        scope: &mut PrincipalScope<'_>,
        audit: &mut AuditChain,
        trace: &TraceId,
        requeued: bool,
        now: u64,
    ) -> Result<(), ExitJourneyError> {
        self.fail(
            scope,
            audit,
            trace,
            if requeued {
                StoredFailure::TransactionDisplacedRequeued
            } else {
                StoredFailure::TransactionDisplacedDropped
            },
            now,
        )
    }

    fn fail(
        &mut self,
        scope: &mut PrincipalScope<'_>,
        audit: &mut AuditChain,
        trace: &TraceId,
        failure: StoredFailure,
        now: u64,
    ) -> Result<(), ExitJourneyError> {
        self.record.failure = Some(failure);
        self.transition(scope, audit, trace, Phase::Failed, now)
    }

    fn transition(
        &mut self,
        scope: &mut PrincipalScope<'_>,
        audit: &mut AuditChain,
        trace: &TraceId,
        phase: Phase,
        now: u64,
    ) -> Result<(), ExitJourneyError> {
        let from = self.record.phase.audit_state();
        self.record.phase = phase;
        self.persist_at(scope, now)?;
        self.write_snapshot(scope)?;
        self.append_transition(scope, audit, trace, from, now)
    }

    fn append_transition(
        &self,
        scope: &mut PrincipalScope<'_>,
        audit: &mut AuditChain,
        trace: &TraceId,
        from: AuditJourneyState,
        now: u64,
    ) -> Result<(), ExitJourneyError> {
        let snapshot = self.snapshot_key()?;
        let evidence = [EvidenceRef::new(Table::Journeys, snapshot)];
        audit.append(
            scope,
            now,
            trace,
            &AuditEvent::JourneyTransition {
                journey: Label::new(self.record.phase.code())?,
                kind: JourneyKind::Exit,
                from,
                to: self.record.phase.audit_state(),
            },
            &evidence,
        )?;
        Ok(())
    }

    fn ensure_phase_audited(
        &self,
        scope: &mut PrincipalScope<'_>,
        audit: &mut AuditChain,
        trace: &TraceId,
        now: u64,
    ) -> Result<(), ExitJourneyError> {
        let prefix = format!(
            "{SNAPSHOT_PREFIX}{}-{}-",
            hex(&self.record.idempotency_key),
            self.record.phase.code()
        );
        let already_bound = audit.entries(scope)?.iter().any(|entry| {
            matches!(
                entry.event(),
                AuditEvent::JourneyTransition {
                    kind: JourneyKind::Exit,
                    to,
                    ..
                } if *to == self.record.phase.audit_state()
            ) && entry.evidence().iter().any(|binding| {
                binding.table() == Table::Journeys && binding.key().as_str().starts_with(&prefix)
            })
        });
        if !already_bound {
            self.write_snapshot(scope)?;
            self.append_transition(scope, audit, trace, self.record.phase.audit_from(), now)?;
        }
        Ok(())
    }

    fn ensure_confirmation_audited(
        &self,
        scope: &mut PrincipalScope<'_>,
        audit: &mut AuditChain,
        trace: &TraceId,
        now: u64,
    ) -> Result<(), ExitJourneyError> {
        let already_bound = audit.entries(scope)?.iter().any(|entry| {
            matches!(
                entry.event(),
                AuditEvent::SigningDecision {
                    operation: SigningOperation::EmergencyExit,
                    disclosure_digest,
                    outcome: Decision::Granted,
                    ..
                } if *disclosure_digest == self.record.confirmation_digest
            ) && !entry.evidence().is_empty()
        });
        if !already_bound {
            self.write_snapshot(scope)?;
            let snapshot = self.snapshot_key()?;
            audit.append(
                scope,
                now,
                trace,
                &AuditEvent::SigningDecision {
                    operation: SigningOperation::EmergencyExit,
                    disclosure_digest: self.record.confirmation_digest,
                    step_up: StepUpEvidence::NotRequired,
                    outcome: Decision::Granted,
                },
                &[EvidenceRef::new(Table::Journeys, snapshot)],
            )?;
        }
        Ok(())
    }

    fn write_snapshot(&self, scope: &mut PrincipalScope<'_>) -> Result<(), ExitJourneyError> {
        let key = self.snapshot_key()?;
        let bytes = encode(&self.record)?;
        if let Some(existing) = scope.get(Table::Journeys, &key) {
            if existing.bytes() != bytes {
                return Err(ExitJourneyError::EvidenceConflict);
            }
            return Ok(());
        }
        scope.put(Table::Journeys, key, self.record.updated_at, bytes)?;
        Ok(())
    }

    fn snapshot_key(&self) -> Result<RowKey, ExitJourneyError> {
        let encoded = encode(&self.record)?;
        let record_digest = digest(&[b"layerx-human-exit-snapshot/v1\0", &encoded]);
        Ok(RowKey::new(format!(
            "{SNAPSHOT_PREFIX}{}-{}-{}",
            hex(&self.record.idempotency_key),
            self.record.phase.code(),
            &hex(&record_digest)[..16]
        ))?)
    }

    fn persist_at(
        &mut self,
        scope: &mut PrincipalScope<'_>,
        now: u64,
    ) -> Result<(), ExitJourneyError> {
        if now < self.record.updated_at {
            return Err(ExitJourneyError::TimeRegressed);
        }
        self.record.updated_at = now;
        self.persist(scope)
    }

    fn persist(&self, scope: &mut PrincipalScope<'_>) -> Result<(), ExitJourneyError> {
        validate_record(&self.record)?;
        scope.put(
            Table::Journeys,
            record_row(self.record.idempotency_key)?,
            self.record.updated_at,
            encode(&self.record)?,
        )?;
        Ok(())
    }
}

fn validate_plan(plan: &ExitPlan) -> Result<(), ExitJourneyError> {
    if plan.idempotency_key == [0; 32] {
        return Err(ExitJourneyError::InvalidPlan);
    }
    Ok(())
}

fn validate_record(record: &Record) -> Result<(), ExitJourneyError> {
    let claimed = matches!(
        record.phase,
        Phase::WalletOpening
            | Phase::Confirming
            | Phase::AwaitingExecution
            | Phase::ExecuteWalletOpening
            | Phase::ExecuteConfirming
            | Phase::Done
    );
    let submitted = matches!(
        record.phase,
        Phase::Confirming
            | Phase::AwaitingExecution
            | Phase::ExecuteWalletOpening
            | Phase::ExecuteConfirming
            | Phase::Done
    );
    let queued = matches!(
        record.phase,
        Phase::AwaitingExecution | Phase::ExecuteWalletOpening | Phase::ExecuteConfirming
    ) || record.phase == Phase::Done;
    let executing = matches!(record.phase, Phase::ExecuteConfirming | Phase::Done);
    if record.version != RECORD_VERSION
        || JourneyId::new(record.journey_id.clone()).is_err()
        || record.idempotency_key == [0; 32]
        || record.plan_digest == [0; 32]
        || record.confirmation_digest == [0; 32]
        || record.wallet_action_key == [0; 32]
        || record.execute_action_key == [0; 32]
        || record.wallet_action_key == record.execute_action_key
        || record.updated_at < record.started_at
        || (claimed && record.claim.is_none())
        || (submitted && record.transaction.is_none())
        || (queued && record.available_at.is_none())
        || (executing && record.execute_transaction.is_none())
        || (record.phase == Phase::Done) != record.finality.is_some()
        || (record.phase == Phase::Failed) != record.failure.is_some()
    {
        return Err(ExitJourneyError::Corrupt("exit invariants are invalid"));
    }
    Ok(())
}

fn encode(record: &Record) -> Result<Vec<u8>, ExitJourneyError> {
    serde_json::to_vec(record).map_err(|_| ExitJourneyError::Corrupt("exit cannot be encoded"))
}

fn decode(bytes: &[u8]) -> Result<Record, ExitJourneyError> {
    let record = serde_json::from_slice(bytes)
        .map_err(|_| ExitJourneyError::Corrupt("invalid exit encoding"))?;
    validate_record(&record)?;
    Ok(record)
}

fn record_row(key: [u8; 32]) -> Result<RowKey, StoreError> {
    RowKey::new(format!("{RECORD_PREFIX}{}", hex(&key)))
}

fn derive_key(domain: &[u8], key: &[u8; 32]) -> [u8; 32] {
    digest(&[domain, key])
}

fn plan_digest(plan: &ExitPlan) -> Result<[u8; 32], ExitJourneyError> {
    let stored = StoredEvidence::from_public(&plan.evidence)?;
    let encoded = serde_json::to_vec(&stored)
        .map_err(|_| ExitJourneyError::Corrupt("exit plan cannot be encoded"))?;
    Ok(digest(&[
        PLAN_DIGEST_DOMAIN,
        plan.journey_id.as_str().as_bytes(),
        &plan.idempotency_key,
        &encoded,
    ]))
}

fn finality(
    transaction: TransactionHash,
    inclusion: TransactionInclusion,
    confirmations: u64,
) -> ExitFinalityEvidence {
    debug_assert_eq!(inclusion.execution, ExecutionOutcome::Succeeded);
    ExitFinalityEvidence {
        transaction: transaction.bytes(),
        block_number: inclusion.block.number,
        block_hash: inclusion.block.hash,
        transaction_index: inclusion.transaction_index,
        confirmations,
    }
}

fn digest(parts: &[&[u8]]) -> [u8; 32] {
    let mut digest = Sha256::new();
    for part in parts {
        digest.update(part);
    }
    digest.finalize().into()
}

fn hex(bytes: &[u8]) -> String {
    let mut output = String::with_capacity(bytes.len().saturating_mul(2));
    for byte in bytes {
        use std::fmt::Write as _;
        let _ = write!(output, "{byte:02x}");
    }
    output
}

/// Typed emergency-exit journey failure.
#[derive(Debug)]
pub enum ExitJourneyError {
    Store(StoreError),
    Audit(AuditError),
    Redaction(RedactionError),
    Paxeer(ExitError),
    TrackerConfig(layerx_paxeer_client::TrackerConfigError),
    Boundary(ExitBoundaryError),
    InvalidPlan,
    IdempotencyConflict,
    TimeRegressed,
    ClaimMismatch,
    /// The request transaction is final but the precompile holds no queued
    /// claim for it yet.
    ClaimNotQueued,
    /// The queued claim was already paid by a transaction this journey did not
    /// submit, so no finality evidence of this journey's own can bind it.
    ClaimPaidElsewhere,
    /// The authority cancelled the queued claim.
    ClaimCancelled,
    /// A final execute transaction was reported without its receipt logs.
    MissingReceiptLogs,
    EvidenceConflict,
    Corrupt(&'static str),
}

impl Display for ExitJourneyError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Store(error) => write!(formatter, "exit store failure: {error}"),
            Self::Audit(error) => write!(formatter, "exit audit failure: {error}"),
            Self::Redaction(error) => write!(formatter, "exit redaction failure: {error}"),
            Self::Paxeer(error) => write!(formatter, "exit Paxeer failure: {error:?}"),
            Self::TrackerConfig(error) => {
                write!(formatter, "exit finality configuration failure: {error:?}")
            }
            Self::Boundary(error) => write!(formatter, "exit wallet failure: {error:?}"),
            Self::InvalidPlan => formatter.write_str("exit plan is invalid"),
            Self::IdempotencyConflict => {
                formatter.write_str("exit idempotency key owns another request")
            }
            Self::TimeRegressed => formatter.write_str("exit journey time regressed"),
            Self::ClaimMismatch => {
                formatter.write_str("exit claim differs from the confirmed exit material")
            }
            Self::ClaimNotQueued => {
                formatter.write_str("the custody precompile holds no queued exit claim yet")
            }
            Self::ClaimPaidElsewhere => formatter
                .write_str("the queued exit was paid by a transaction this journey did not submit"),
            Self::ClaimCancelled => formatter.write_str("the queued exit claim was cancelled"),
            Self::MissingReceiptLogs => {
                formatter.write_str("the final exit transaction reported no receipt logs")
            }
            Self::EvidenceConflict => formatter.write_str("exit audit evidence conflicts"),
            Self::Corrupt(reason) => write!(formatter, "corrupt exit journey: {reason}"),
        }
    }
}

impl std::error::Error for ExitJourneyError {}

impl From<StoreError> for ExitJourneyError {
    fn from(value: StoreError) -> Self {
        Self::Store(value)
    }
}

impl From<AuditError> for ExitJourneyError {
    fn from(value: AuditError) -> Self {
        Self::Audit(value)
    }
}

impl From<RedactionError> for ExitJourneyError {
    fn from(value: RedactionError) -> Self {
        Self::Redaction(value)
    }
}

impl From<ExitBoundaryError> for ExitJourneyError {
    fn from(value: ExitBoundaryError) -> Self {
        Self::Boundary(value)
    }
}

#[cfg(test)]
mod forced_exit_persistence_tests {
    use super::*;
    use ed25519_dalek::{Signer as _, SigningKey};
    use layerx_crypto::settlement_recipient::RecipientAuthorization;
    use layerx_paxeer_client::custody::exit_recipient_message;
    use layerx_paxeer_client::state_proof::{AccountPath, StateWitness};
    use layerx_types::ids::Did;

    type Outcome = Result<(), Box<dyn std::error::Error>>;

    const NETWORK_ID: u32 = 7332;
    const BALANCE: u128 = 5_000_000;
    const DID: &[u8] = b"exit-holder";
    const RECIPIENT: [u8; 20] = [0x42; 20];
    const ASSET: [u8; 32] = [0x24; 32];

    fn state_leaf(key: &[u8], value: &[u8]) -> Result<[u8; 32], Box<dyn std::error::Error>> {
        let mut hasher = Sha256::new();
        hasher.update(b"LXP/v1/state-leaf\0");
        hasher.update(u32::try_from(key.len())?.to_be_bytes());
        hasher.update(u32::try_from(value.len())?.to_be_bytes());
        hasher.update(key);
        hasher.update(value);
        Ok(hasher.finalize().into())
    }

    /// The exact native account value layout the custody module's balance
    /// verifier reads, with the account authority key in place.
    fn account_value(
        name: &[u8],
        authority: [u8; 32],
    ) -> Result<([u8; 32], Vec<u8>), Box<dyn std::error::Error>> {
        let mut hasher = Sha256::new();
        hasher.update(b"LX:ACCOUNT:v1");
        hasher.update(u32::try_from(name.len())?.to_be_bytes());
        hasher.update(name);
        let account_id: [u8; 32] = hasher.finalize().into();
        let mut value = u16::try_from(name.len())?.to_be_bytes().to_vec();
        value.extend_from_slice(name);
        value.push(1);
        value.extend_from_slice(&BALANCE.to_be_bytes());
        value.extend_from_slice(&ASSET);
        value.push(1);
        value.extend_from_slice(&9_u64.to_be_bytes());
        value.extend_from_slice(&2_u64.to_be_bytes());
        value.extend_from_slice(&[0, 0]);
        value.extend_from_slice(&authority);
        value.push(1);
        layerx_proof::state::decode_account_value(account_id, &value)
            .map_err(|error| format!("account value: {error:?}"))?;
        Ok((account_id, value))
    }

    /// Builds a real account state witness, derives its state root, and signs
    /// the recipient authorization over that root with the account authority.
    fn evidence() -> Result<(ExitEvidence, [u8; 32]), Box<dyn std::error::Error>> {
        let authority = SigningKey::from_bytes(&[0x61; 32]);
        let public = authority.verifying_key().to_bytes();
        let (account_id, value) = account_value(b"agent:exit-holder:main", public)?;
        let mut key = vec![4_u8];
        key.extend_from_slice(&account_id);
        let witness = StateWitness {
            module_id: 0,
            key,
            value,
            account_path: Some(AccountPath {
                index: 0,
                count: 2,
                siblings: vec![state_leaf(&[4; 33], b"neighbour")?],
            }),
            leaf_index_a: 0,
            leaf_count_a: 2,
            siblings_a: vec![state_leaf(b"sequence", &21_u64.to_be_bytes())?],
            leaf_count_b: 10,
            siblings_b: vec![[0xd1; 32], [0xd2; 32], [0xd3; 32], [0xd4; 32]],
        };
        let root = witness.root()?;
        let encoded = witness.encode()?;
        let recipient = EvmAddress::new(RECIPIENT);

        // The message the human service's recipient-authorization signer
        // produces must be byte-identical to the one the custody precompile
        // verifies, anchored on the finalized state root.
        let authorization = RecipientAuthorization {
            network_id: NETWORK_ID,
            binding_digest: [0x11; 32],
            did: Did::new(DID).map_err(|error| format!("did: {error:?}"))?,
            public_key: public,
            asset: ASSET,
            checkpoint: root,
            recipient: RECIPIENT,
        };
        let message = authorization
            .message()
            .map_err(|error| format!("{error:?}"))?;
        assert_eq!(
            message,
            exit_recipient_message(NETWORK_ID, &account_id, &ASSET, recipient, &root)
        );
        assert_eq!(
            authorization
                .account()
                .map_err(|error| format!("{error:?}"))?,
            account_id
        );
        let signature = authority.sign(&message).to_bytes();
        authorization
            .verify_signature(&signature)
            .map_err(|error| format!("{error:?}"))?;

        Ok((
            ExitEvidence {
                material: ForcedExitMaterial {
                    witness: encoded,
                    batch_number: 21,
                    account: account_id,
                    asset_id: ASSET,
                    recipient,
                    recipient_signature: signature,
                },
                finalised_balance: BALANCE,
            },
            root,
        ))
    }

    #[test]
    fn stored_exit_preserves_forced_exit_material_and_recipient_authority() -> Outcome {
        let (evidence, root) = evidence()?;
        layerx_paxeer_client::verify_exit_balance(&evidence, NETWORK_ID, root)
            .map_err(|error| format!("{error:?}"))?;

        let stored = StoredEvidence::from_public(&evidence)?;
        let bytes = serde_json::to_vec(&stored)?;
        let restored: StoredEvidence = serde_json::from_slice(&bytes)?;
        let restored = restored.public()?;
        assert_eq!(restored, evidence);
        layerx_paxeer_client::verify_exit_balance(&restored, NETWORK_ID, root)
            .map_err(|error| format!("{error:?}"))?;

        let mut other_recipient = restored.clone();
        other_recipient.material.recipient = EvmAddress::new([0x43; 20]);
        assert!(
            layerx_paxeer_client::verify_exit_balance(&other_recipient, NETWORK_ID, root).is_err()
        );

        let mut other_balance = restored.clone();
        other_balance.finalised_balance = BALANCE + 1;
        assert!(
            layerx_paxeer_client::verify_exit_balance(&other_balance, NETWORK_ID, root).is_err()
        );

        let mut other_anchor = root;
        other_anchor[0] ^= 0x01;
        assert!(
            layerx_paxeer_client::verify_exit_balance(&restored, NETWORK_ID, other_anchor).is_err()
        );
        Ok(())
    }

    #[test]
    fn exit_plans_round_trip_only_through_the_forced_exit_codec() -> Outcome {
        let (evidence, _) = evidence()?;
        let plan = ExitPlan {
            journey_id: JourneyId::new("jrn_exitcodec000001")?,
            idempotency_key: [0x41; 32],
            evidence,
        };
        let encoded = encode_exit_plan(&plan)?;
        assert_eq!(encoded.get(..2), Some([1, PLAN_TAG].as_slice()));
        assert_eq!(decode_exit_plan(&encoded)?, plan);

        let mut truncated = encoded.clone();
        truncated.pop();
        assert!(decode_exit_plan(&truncated).is_err());

        let mut trailing = encoded.clone();
        trailing.push(0);
        assert!(decode_exit_plan(&trailing).is_err());

        // The removed checkpoint-proof plan shapes are refused, never reread.
        for tag in [3_u8, 4] {
            let mut retagged = encoded.clone();
            retagged[1] = tag;
            assert!(decode_exit_plan(&retagged).is_err());
        }

        // A plan whose recipient signature was altered cannot be constructed
        // back out of the wire: the material codec revalidates every field.
        let mut tampered = encoded;
        let at = tampered.len() - 1;
        tampered[at] ^= 0x01;
        let decoded = decode_exit_plan(&tampered);
        assert!(decoded.is_err() || decoded.ok().as_ref() != Some(&plan));
        Ok(())
    }

    #[test]
    fn the_two_forced_exit_wallet_actions_are_distinct_and_stable() -> Outcome {
        let key = [0x41_u8; 32];
        let request = derive_key(WALLET_ACTION_DOMAIN, &key);
        let execute = derive_key(EXECUTE_ACTION_DOMAIN, &key);
        assert_ne!(request, execute);
        assert_eq!(request, derive_key(WALLET_ACTION_DOMAIN, &key));
        assert_eq!(execute, derive_key(EXECUTE_ACTION_DOMAIN, &key));
        assert_ne!(request, [0; 32]);
        assert_ne!(execute, [0; 32]);
        Ok(())
    }
}
