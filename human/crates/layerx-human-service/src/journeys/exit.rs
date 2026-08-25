//! Direct, crash-resumable Paxeer emergency-exit journey.

use std::fmt::{Display, Formatter};

use layerx_paxeer_client::{
    EmergencyExit, ExecutionOutcome, ExitClaim, ExitEligibility, ExitError, ExitEvidence,
    ExitProgress, ExitRefusal, GuarantorAttestation, TransactionHash, TransactionInclusion,
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

const RECORD_VERSION: u8 = 1;
const RECORD_PREFIX: &str = "exit-journey-";
const SNAPSHOT_PREFIX: &str = "exit-evidence-";
const WALLET_ACTION_DOMAIN: &[u8] = b"layerx-human-exit-wallet/v1\0";
const PLAN_DIGEST_DOMAIN: &[u8] = b"layerx-human-exit-plan/v1\0";
const CONFIRMATION_DOMAIN: &[u8] = b"layerx-human-exit-confirmation/v1\0";

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

/// Stable wallet request. Implementations must resolve the original transaction
/// for repeated calls carrying the same action key and identical claim.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ExitWalletRequest {
    pub action_key: [u8; 32],
    pub contract: EvmAddress,
    pub calldata: Vec<u8>,
    pub checkpoint: [u8; 32],
    pub withdrawal_id: [u8; 32],
    pub nullifier: [u8; 32],
    pub recipient: EvmAddress,
    pub finalised_balance: u128,
}

/// Result of the one user-controlled Paxeer transaction.
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

/// Wallet boundary for the real Paxeer exit transaction.
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
            Self::Done => "done",
            Self::NormalOperation => "normal-operation",
            Self::Failed => "failed",
        }
    }

    const fn audit_state(self) -> AuditJourneyState {
        match self {
            Self::Constructing | Self::Confirming => AuditJourneyState::Processing,
            Self::WalletOpening => AuditJourneyState::WaitingForYou,
            Self::Done => AuditJourneyState::DoneFinalised,
            Self::NormalOperation | Self::Failed => AuditJourneyState::Refused,
        }
    }

    const fn audit_from(self) -> AuditJourneyState {
        match self {
            Self::Constructing | Self::Confirming => AuditJourneyState::WaitingForYou,
            Self::WalletOpening | Self::Done | Self::NormalOperation | Self::Failed => {
                AuditJourneyState::Processing
            }
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

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
struct StoredAttestation {
    protocol_version: u16,
    network_id: u32,
    paxeer_chain_id: u64,
    settlement_contract: [u8; 20],
    epoch: u64,
    checkpoint_id: [u8; 32],
    checkpoint_hash: [u8; 32],
    guarantor_id: [u8; 32],
    batch_number: u64,
    data_availability_root: [u8; 32],
    replayed: bool,
    data_available: bool,
    availability_class_mask: u8,
    attested_at: u64,
    signer: [u8; 20],
    signature_r: [u8; 32],
    signature_s: [u8; 32],
    signature_v: u8,
}

impl From<&GuarantorAttestation> for StoredAttestation {
    fn from(value: &GuarantorAttestation) -> Self {
        Self {
            protocol_version: value.protocol_version,
            network_id: value.network_id,
            paxeer_chain_id: value.paxeer_chain_id,
            settlement_contract: value.settlement_contract.bytes(),
            epoch: value.epoch,
            checkpoint_id: value.checkpoint_id,
            checkpoint_hash: value.checkpoint_hash,
            guarantor_id: value.guarantor_id,
            batch_number: value.batch_number,
            data_availability_root: value.data_availability_root,
            replayed: value.replayed,
            data_available: value.data_available,
            availability_class_mask: value.availability_class_mask,
            attested_at: value.attested_at,
            signer: value.signer.bytes(),
            signature_r: value.signature_r,
            signature_s: value.signature_s,
            signature_v: value.signature_v,
        }
    }
}

impl StoredAttestation {
    const fn public(&self) -> GuarantorAttestation {
        GuarantorAttestation {
            protocol_version: self.protocol_version,
            network_id: self.network_id,
            paxeer_chain_id: self.paxeer_chain_id,
            settlement_contract: EvmAddress::new(self.settlement_contract),
            epoch: self.epoch,
            checkpoint_id: self.checkpoint_id,
            checkpoint_hash: self.checkpoint_hash,
            guarantor_id: self.guarantor_id,
            batch_number: self.batch_number,
            data_availability_root: self.data_availability_root,
            replayed: self.replayed,
            data_available: self.data_available,
            availability_class_mask: self.availability_class_mask,
            attested_at: self.attested_at,
            signer: EvmAddress::new(self.signer),
            signature_r: self.signature_r,
            signature_s: self.signature_s,
            signature_v: self.signature_v,
        }
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
struct StoredEvidence {
    account: [u8; 32],
    asset_id: [u8; 32],
    finalised_balance: u128,
    recipient: [u8; 20],
    leaf_index: u64,
    siblings: Vec<[u8; 32]>,
    attestations: Vec<StoredAttestation>,
}

impl From<&ExitEvidence> for StoredEvidence {
    fn from(value: &ExitEvidence) -> Self {
        Self {
            account: value.account,
            asset_id: value.asset_id,
            finalised_balance: value.finalised_balance,
            recipient: value.recipient.bytes(),
            leaf_index: value.leaf_index,
            siblings: value.siblings.clone(),
            attestations: value.attestations.iter().map(Into::into).collect(),
        }
    }
}

impl StoredEvidence {
    fn public(&self) -> ExitEvidence {
        ExitEvidence {
            account: self.account,
            asset_id: self.asset_id,
            finalised_balance: self.finalised_balance,
            recipient: EvmAddress::new(self.recipient),
            leaf_index: self.leaf_index,
            siblings: self.siblings.clone(),
            attestations: self
                .attestations
                .iter()
                .map(StoredAttestation::public)
                .collect(),
        }
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
struct StoredClaim {
    contract: [u8; 20],
    calldata: Vec<u8>,
    checkpoint: [u8; 32],
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
            checkpoint: value.checkpoint,
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
    fn wallet_request(&self, action_key: [u8; 32]) -> ExitWalletRequest {
        ExitWalletRequest {
            action_key,
            contract: EvmAddress::new(self.contract),
            calldata: self.calldata.clone(),
            checkpoint: self.checkpoint,
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
    evidence: StoredEvidence,
    claim: Option<StoredClaim>,
    transaction: Option<[u8; 32]>,
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
            evidence: StoredEvidence::from(&plan.evidence),
            claim: None,
            transaction: None,
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
            Phase::Constructing => match exit.construct_claim(&self.record.evidence.public()) {
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
                Err(ExitError::Endpoint(error)) => {
                    return Err(ExitJourneyError::Paxeer(ExitError::Endpoint(error)));
                }
                Err(ExitError::Contract { detail }) => {
                    return Err(ExitJourneyError::Paxeer(ExitError::Contract { detail }));
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
            },
            Phase::WalletOpening => {
                let request = self.wallet_request()?;
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
                match ExitProgress::of(&report) {
                    ExitProgress::Settled {
                        inclusion,
                        confirmations,
                    } => {
                        self.record.finality =
                            Some(finality(transaction, inclusion, confirmations));
                        self.transition(scope, audit, trace, Phase::Done, now)?;
                    }
                    ExitProgress::Refused { .. } => {
                        self.fail(scope, audit, trace, StoredFailure::PaxeerRefused, now)?;
                    }
                    ExitProgress::Displaced { requeued } => {
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
                        )?;
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
            Phase::WalletOpening => ExitStage::WaitingForWallet,
            Phase::Confirming => ExitStage::ConfirmingPaxeer {
                transaction: self.transaction()?,
                confirmations: self.record.confirmations,
                required: self.record.required,
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

    fn validate_claim(
        &self,
        exit: &EmergencyExit,
        claim: &ExitClaim,
    ) -> Result<(), ExitJourneyError> {
        let evidence = &self.record.evidence;
        if claim.contract != exit.contract()
            || claim.account != evidence.account
            || claim.asset_id != evidence.asset_id
            || claim.finalised_balance != evidence.finalised_balance
            || claim.recipient.bytes() != evidence.recipient
            || claim.calldata.is_empty()
            || claim.checkpoint == [0; 32]
            || claim.state_root == [0; 32]
            || claim.withdrawal_id == [0; 32]
            || claim.nullifier == [0; 32]
        {
            return Err(ExitJourneyError::ClaimMismatch);
        }
        Ok(())
    }

    fn wallet_request(&self) -> Result<ExitWalletRequest, ExitJourneyError> {
        self.record
            .claim
            .as_ref()
            .map(|claim| claim.wallet_request(self.record.wallet_action_key))
            .ok_or(ExitJourneyError::Corrupt("wallet stage has no claim"))
    }

    fn transaction(&self) -> Result<TransactionHash, ExitJourneyError> {
        self.record
            .transaction
            .map(TransactionHash::new)
            .ok_or(ExitJourneyError::Corrupt("exit has no transaction"))
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
    if record.version != RECORD_VERSION
        || JourneyId::new(record.journey_id.clone()).is_err()
        || record.idempotency_key == [0; 32]
        || record.plan_digest == [0; 32]
        || record.confirmation_digest == [0; 32]
        || record.wallet_action_key == [0; 32]
        || record.updated_at < record.started_at
        || (matches!(
            record.phase,
            Phase::WalletOpening | Phase::Confirming | Phase::Done
        ) && record.claim.is_none())
        || (matches!(record.phase, Phase::Confirming | Phase::Done) && record.transaction.is_none())
        || (record.phase == Phase::Done) != record.finality.is_some()
        || (record.phase == Phase::Failed) != record.failure.is_some()
        || (record.phase != Phase::Failed && record.failure.is_some())
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
    let stored = StoredEvidence::from(&plan.evidence);
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
                formatter.write_str("exit claim differs from the confirmed checkpoint evidence")
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
