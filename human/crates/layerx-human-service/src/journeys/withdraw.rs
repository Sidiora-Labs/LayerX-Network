//! Receipt-gated, indefinitely durable withdrawal and Paxeer claim journey.

use std::fmt::{Display, Formatter};

use layerx_agent_api::identity::{AgentDid, AuthorityRef, ContractError};
use layerx_intents::{BridgeWithdrawRequest, Intent, IntentKind};
use layerx_paxeer_client::{
    account_address, CancellationEvidence, CancelledFundsDisposition, ChallengeHold, ChallengeKind,
    CheckpointProof, ClaimProgress, CommittedWithdrawalDebit, DebitExpectation, DebitFault,
    FinalityStage, PaxeerFundsDisposition, PayoutEvidence, ProtocolDebitDisposition,
    SubmittedWithdrawalClaim, TransactionHash, WithdrawalAttestation, WithdrawalBoundary,
    WithdrawalError,
};
use layerx_proof::receipt::AuthorizedBatch;
use layerx_sdk::Client as AgentClient;
use layerx_types::account::AccountId;
use layerx_types::amount::Amount;
use layerx_types::ids::{AssetId, IdempotencyKey};
use layerx_types::intent::{EvmAddress, NetworkId, WithdrawalId};
use layerx_types::payload::ModuleRegistry;
use serde::{Deserialize, Serialize};
use sha2::{Digest as _, Sha256};

use super::{
    AgentBoundary, AgentBoundaryError, JourneyEngine, JourneyError, JourneyLeg, JourneyPlan,
    JourneyState, ReceiptLookup,
};
use crate::custody::{CustodySigner, KeyId, Operation, StepUpEvidence};
use crate::notify::JourneyId;
use crate::store::{AuditDisposition, EvidenceRef, PrincipalScope, RowKey, StoreError, Table};
use crate::trace::TraceId;

const RECORD_VERSION: u8 = 1;
const STATE_PREFIX: &str = "withdraw-state-";
const PIN_PREFIX: &str = "withdraw-pin-";
const PLAN_DIGEST_DOMAIN: &[u8] = b"layerx-human-withdraw-plan/v1\0";
const DEBIT_PLAN_DOMAIN: &[u8] = b"layerx-human-withdraw-debit-plan/v1\0";
const DEBIT_ACTION_DOMAIN: &[u8] = b"layerx-human-withdraw-debit/v1\0";
const CLAIM_ACTION_DOMAIN: &[u8] = b"layerx-human-withdraw-claim/v1\0";
const PAYOUT_ACTION_DOMAIN: &[u8] = b"layerx-human-withdraw-payout/v1\0";
const CANCEL_ACTION_DOMAIN: &[u8] = b"layerx-human-withdraw-cancel/v1\0";

/// Agent preparation facts for the protocol withdrawal debit.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct WithdrawalAgentPlan {
    pub actor: AgentDid,
    pub authority: AuthorityRef,
    pub account_sequence: u64,
    pub not_before: u64,
    pub not_after: u64,
    pub fee_limit: u128,
    pub custody_key: KeyId,
}

/// Declared timing inputs used to explain the settlement wait.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct SettlementConfig {
    pub checkpoint_interval_seconds: u64,
    pub paxeer_block_seconds: u64,
    pub required_confirmations: u64,
}

impl SettlementConfig {
    /// Computes the displayed wait solely from declared configuration.
    ///
    /// # Errors
    ///
    /// Refuses zero values and arithmetic overflow.
    pub fn expectation(self) -> Result<SettlementExpectation, WithdrawalJourneyError> {
        if self.checkpoint_interval_seconds == 0
            || self.paxeer_block_seconds == 0
            || self.required_confirmations == 0
        {
            return Err(WithdrawalJourneyError::InvalidPlan);
        }
        let confirmation_seconds = self
            .paxeer_block_seconds
            .checked_mul(self.required_confirmations)
            .ok_or(WithdrawalJourneyError::InvalidPlan)?;
        let expected_seconds = self
            .checkpoint_interval_seconds
            .checked_add(confirmation_seconds)
            .ok_or(WithdrawalJourneyError::InvalidPlan)?;
        Ok(SettlementExpectation {
            expected_seconds,
            checkpoint_interval_seconds: self.checkpoint_interval_seconds,
            paxeer_block_seconds: self.paxeer_block_seconds,
            required_confirmations: self.required_confirmations,
        })
    }
}

/// Honest settlement expectation and the declared inputs behind it.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct SettlementExpectation {
    pub expected_seconds: u64,
    pub checkpoint_interval_seconds: u64,
    pub paxeer_block_seconds: u64,
    pub required_confirmations: u64,
}

/// Immutable withdrawal request. All economic action keys are derived from
/// the caller idempotency key and survive service restarts unchanged.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct WithdrawalPlan {
    pub journey_id: JourneyId,
    pub idempotency_key: [u8; 32],
    pub network: NetworkId,
    pub withdrawal_id: WithdrawalId,
    pub owner: AccountId,
    pub withdrawals_account: AccountId,
    pub payout_address: EvmAddress,
    pub asset: AssetId,
    pub amount: Amount,
    pub currency: String,
    pub settlement: SettlementConfig,
    pub reminder_interval_seconds: u64,
    pub agent: WithdrawalAgentPlan,
}

/// The only truthful cancellation promise after the `LayerX` debit commits.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CancellationPolicy {
    CannotCancelAfterCommitCompleteOnly,
}

/// Economic purpose of one real Paxeer transaction.
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub enum PaxeerAction {
    QueueClaim,
    FinalisePayout,
    CancelChallengedPayout,
}

/// Exact wallet or permissionless transaction request. An adapter must bind
/// `action_key` durably before broadcast and return the original transaction
/// when the same request is recovered after an acknowledgement gap.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct WithdrawalTransactionRequest {
    pub action_key: [u8; 32],
    pub action: PaxeerAction,
    pub target: EvmAddress,
    pub calldata: Vec<u8>,
}

/// Real transaction-boundary result. Unknown is never permission to resubmit.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PaxeerActionOutcome {
    Submitted(TransactionHash),
    Unknown,
}

/// Stable production-boundary failures.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum WithdrawalBoundaryError {
    Unavailable,
    ContractViolation,
}

/// Core proof and Paxeer transaction operations consumed by the state machine.
pub trait WithdrawalRuntime {
    /// Returns a real finalised checkpoint proof, or `None` while settlement is pending.
    ///
    /// # Errors
    ///
    /// Returns a stable boundary failure without manufacturing proof material.
    fn checkpoint_proof(
        &mut self,
        debit: &DebitExpectation,
    ) -> Result<Option<CheckpointProof>, WithdrawalBoundaryError>;

    /// Broadcasts or resolves a transaction under its stable action key.
    ///
    /// # Errors
    ///
    /// Returns unavailable when outcome is unknown; a retry with the same key
    /// must resolve, never create a second transaction.
    fn submit_or_resolve(
        &mut self,
        request: &WithdrawalTransactionRequest,
    ) -> Result<PaxeerActionOutcome, WithdrawalBoundaryError>;

    /// Read-only recovery for an action whose broadcast outcome was unknown.
    ///
    /// # Errors
    ///
    /// Returns a stable boundary failure and never broadcasts.
    fn lookup(
        &mut self,
        action_key: [u8; 32],
    ) -> Result<Option<TransactionHash>, WithdrawalBoundaryError>;
}

/// Durable reminder for a claim that still needs its one wallet action.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct WithdrawalReminder {
    pub sequence: u64,
    pub journey_id: JourneyId,
    pub created_at: u64,
    pub deep_link: String,
}

/// Honest withdrawal timeline. `PaidOut` exists only with verified Paxeer evidence.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum WithdrawalStage {
    Processing,
    WaitingForSettlement {
        expectation: SettlementExpectation,
    },
    ReadyToClaim,
    ClaimSubmitting,
    WaitingForChallengeWindow {
        available_at: u64,
        observed_at: u64,
    },
    ChallengeHeld(ChallengeHold),
    ChallengeUpheldAwaitingCancellation {
        disposition: CancelledFundsDisposition,
    },
    ReadyToFinalise,
    VerifyingPayout,
    PaidOut(PayoutEvidence),
    Cancelled(CancellationEvidence),
}

/// Receipt- and chain-grounded public status.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct WithdrawalStatus {
    journey_id: JourneyId,
    stage: WithdrawalStage,
    cancellation_policy: CancellationPolicy,
    debit_receipt_reference: Option<[u8; 32]>,
    reminder_count: u64,
}

impl WithdrawalStatus {
    #[must_use]
    pub const fn journey_id(&self) -> &JourneyId {
        &self.journey_id
    }

    #[must_use]
    pub const fn stage(&self) -> &WithdrawalStage {
        &self.stage
    }

    #[must_use]
    pub const fn cancellation_policy(&self) -> CancellationPolicy {
        self.cancellation_policy
    }

    #[must_use]
    pub const fn debit_receipt_reference(&self) -> Option<[u8; 32]> {
        self.debit_receipt_reference
    }

    #[must_use]
    pub const fn reminder_count(&self) -> u64 {
        self.reminder_count
    }
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
enum Phase {
    Processing,
    WaitingSettlement,
    ClaimReady,
    ClaimSubmitting,
    ClaimStillChecking,
    ClaimConfirming,
    ClaimQueued,
    ChallengeHeld,
    ReadyToFinalise,
    PayoutSubmitting,
    PayoutStillChecking,
    PayoutConfirming,
    CancellationReady,
    CancellationSubmitting,
    CancellationStillChecking,
    CancellationConfirming,
    Paid,
    Cancelled,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
struct StoredBatch {
    batch_id: [u8; 32],
    asset: [u8; 32],
    previous_state_root: [u8; 32],
    resulting_state_root: [u8; 32],
    sequencer_public_key: [u8; 32],
}

impl StoredBatch {
    const fn from_authorized(value: &AuthorizedBatch) -> Self {
        Self {
            batch_id: value.batch_id(),
            asset: value.asset(),
            previous_state_root: value.previous_state_root(),
            resulting_state_root: value.resulting_state_root(),
            sequencer_public_key: value.sequencer_public_key(),
        }
    }

    const fn authorized(self) -> AuthorizedBatch {
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
struct StoredAttestation {
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

impl StoredAttestation {
    fn from_public(value: &WithdrawalAttestation) -> Self {
        Self {
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

    const fn public(&self) -> WithdrawalAttestation {
        WithdrawalAttestation {
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
struct StoredProof {
    checkpoint_hash: [u8; 32],
    state_root: [u8; 32],
    epoch: u64,
    batch_number: u64,
    data_availability_root: [u8; 32],
    leaf_index: u64,
    siblings: Vec<[u8; 32]>,
    attestations: Vec<StoredAttestation>,
}

impl StoredProof {
    fn from_public(value: &CheckpointProof) -> Self {
        Self {
            checkpoint_hash: value.checkpoint_hash,
            state_root: value.state_root,
            epoch: value.epoch,
            batch_number: value.batch_number,
            data_availability_root: value.data_availability_root,
            leaf_index: value.leaf_index,
            siblings: value.siblings.clone(),
            attestations: value
                .attestations
                .iter()
                .map(StoredAttestation::from_public)
                .collect(),
        }
    }

    fn public(&self) -> CheckpointProof {
        CheckpointProof {
            checkpoint_hash: self.checkpoint_hash,
            state_root: self.state_root,
            epoch: self.epoch,
            batch_number: self.batch_number,
            data_availability_root: self.data_availability_root,
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

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
struct StoredHold {
    kind: u8,
    evidence_hash: [u8; 32],
    raised_at: u64,
    window_closes_at: u64,
    observed_at: u64,
    window_elapsed: bool,
}

impl StoredHold {
    const fn from_public(value: ChallengeHold) -> Self {
        Self {
            kind: match value.kind {
                ChallengeKind::Fraud => 1,
                ChallengeKind::DataAvailability => 2,
                ChallengeKind::Equivocation => 3,
            },
            evidence_hash: value.evidence_hash,
            raised_at: value.raised_at,
            window_closes_at: value.window_closes_at,
            observed_at: value.observed_at,
            window_elapsed: value.window_elapsed,
        }
    }

    fn public(self) -> Result<ChallengeHold, WithdrawalJourneyError> {
        let kind = match self.kind {
            1 => ChallengeKind::Fraud,
            2 => ChallengeKind::DataAvailability,
            3 => ChallengeKind::Equivocation,
            _ => return Err(WithdrawalJourneyError::Corrupt("unknown challenge kind")),
        };
        Ok(ChallengeHold {
            kind,
            evidence_hash: self.evidence_hash,
            raised_at: self.raised_at,
            window_closes_at: self.window_closes_at,
            observed_at: self.observed_at,
            window_elapsed: self.window_elapsed,
            resolution_has_no_on_chain_deadline: true,
        })
    }
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
struct StoredInclusion {
    block_number: u64,
    block_hash: [u8; 32],
    transaction_index: u64,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
struct StoredPayout {
    debit_receipt_reference: [u8; 32],
    checkpoint_hash: [u8; 32],
    claim_id: [u8; 32],
    transaction: [u8; 32],
    inclusion: StoredInclusion,
    vault: [u8; 20],
    token: [u8; 20],
    asset: [u8; 32],
    recipient: [u8; 20],
    amount: u128,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
struct StoredCancellation {
    debit_receipt_reference: [u8; 32],
    checkpoint_hash: [u8; 32],
    claim_id: [u8; 32],
    transaction: [u8; 32],
    inclusion: StoredInclusion,
    vault: [u8; 20],
    asset: [u8; 32],
    amount: u128,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
struct Record {
    version: u8,
    sequence: u64,
    previous_digest: Option<[u8; 32]>,
    journey_id: String,
    idempotency_key: [u8; 32],
    plan_digest: [u8; 32],
    network_id: u32,
    withdrawal_id: [u8; 32],
    owner: String,
    withdrawals_account: String,
    payout_address: [u8; 20],
    asset: [u8; 32],
    amount: u128,
    currency: String,
    checkpoint_interval_seconds: u64,
    paxeer_block_seconds: u64,
    required_confirmations: u64,
    reminder_interval_seconds: u64,
    actor: String,
    authority: String,
    account_sequence: u64,
    not_before: u64,
    not_after: u64,
    fee_limit: u128,
    custody_key: String,
    debit_plan_key: [u8; 32],
    debit_action_key: [u8; 32],
    debit_journey_id: String,
    claim_action_key: [u8; 32],
    payout_action_key: [u8; 32],
    cancellation_action_key: [u8; 32],
    phase: Phase,
    debit_activity_id: Option<[u8; 32]>,
    debit_receipt: Option<Vec<u8>>,
    debit_batch: Option<StoredBatch>,
    debit_receipt_reference: Option<[u8; 32]>,
    proof: Option<StoredProof>,
    claim_transaction: Option<[u8; 32]>,
    claim_id: Option<[u8; 32]>,
    claim_available_at: Option<u64>,
    payout_transaction: Option<[u8; 32]>,
    cancellation_transaction: Option<[u8; 32]>,
    challenge_hold: Option<StoredHold>,
    challenged_disposition_vault: Option<[u8; 20]>,
    payout: Option<StoredPayout>,
    cancellation: Option<StoredCancellation>,
    reminder_count: u64,
    last_reminder_at: Option<u64>,
    started_at: u64,
    updated_at: u64,
}

/// Append-only, evidence-pinned withdrawal state machine.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct WithdrawalJourney {
    record: Record,
}

impl WithdrawalJourney {
    /// Persists immutable review facts before any debit can execute. Repeating
    /// the idempotency key returns the existing journey only when every plan
    /// fact agrees.
    ///
    /// # Errors
    ///
    /// Refuses invalid plans, conflicting keys, and durable-store failures.
    pub fn start(
        scope: &mut PrincipalScope<'_>,
        plan: &WithdrawalPlan,
        now: u64,
    ) -> Result<Self, WithdrawalJourneyError> {
        validate_plan(plan)?;
        let digest = plan_digest(plan);
        if let Some(existing) = latest_for_key(scope, plan.idempotency_key)? {
            if existing.plan_digest != digest || existing.journey_id != plan.journey_id.as_str() {
                return Err(WithdrawalJourneyError::IdempotencyConflict);
            }
            let journey = Self { record: existing };
            journey.ensure_pin(scope)?;
            return Ok(journey);
        }
        let debit_plan_key = derive_key(DEBIT_PLAN_DOMAIN, &plan.idempotency_key);
        let debit_journey_id = derived_journey_id("debit", debit_plan_key)?;
        let record = Record {
            version: RECORD_VERSION,
            sequence: 0,
            previous_digest: None,
            journey_id: plan.journey_id.as_str().to_owned(),
            idempotency_key: plan.idempotency_key,
            plan_digest: digest,
            network_id: plan.network.value(),
            withdrawal_id: plan.withdrawal_id.bytes(),
            owner: plan.owner.canonical().to_owned(),
            withdrawals_account: plan.withdrawals_account.canonical().to_owned(),
            payout_address: plan.payout_address.bytes(),
            asset: plan.asset.bytes(),
            amount: plan.amount.value(),
            currency: plan.currency.clone(),
            checkpoint_interval_seconds: plan.settlement.checkpoint_interval_seconds,
            paxeer_block_seconds: plan.settlement.paxeer_block_seconds,
            required_confirmations: plan.settlement.required_confirmations,
            reminder_interval_seconds: plan.reminder_interval_seconds,
            actor: plan.agent.actor.as_str().to_owned(),
            authority: plan.agent.authority.as_str().to_owned(),
            account_sequence: plan.agent.account_sequence,
            not_before: plan.agent.not_before,
            not_after: plan.agent.not_after,
            fee_limit: plan.agent.fee_limit,
            custody_key: plan.agent.custody_key.as_str().to_owned(),
            debit_plan_key,
            debit_action_key: derive_key(DEBIT_ACTION_DOMAIN, &plan.idempotency_key),
            debit_journey_id,
            claim_action_key: derive_key(CLAIM_ACTION_DOMAIN, &plan.idempotency_key),
            payout_action_key: derive_key(PAYOUT_ACTION_DOMAIN, &plan.idempotency_key),
            cancellation_action_key: derive_key(CANCEL_ACTION_DOMAIN, &plan.idempotency_key),
            phase: Phase::Processing,
            debit_activity_id: None,
            debit_receipt: None,
            debit_batch: None,
            debit_receipt_reference: None,
            proof: None,
            claim_transaction: None,
            claim_id: None,
            claim_available_at: None,
            payout_transaction: None,
            cancellation_transaction: None,
            challenge_hold: None,
            challenged_disposition_vault: None,
            payout: None,
            cancellation: None,
            reminder_count: 0,
            last_reminder_at: None,
            started_at: now,
            updated_at: now,
        };
        let journey = Self { record };
        journey.write_state(scope)?;
        journey.ensure_pin(scope)?;
        Ok(journey)
    }

    /// Loads the newest append-only state and repairs its permanent evidence pin.
    ///
    /// # Errors
    ///
    /// Refuses duplicate/corrupt state histories and storage failures.
    pub fn load(
        scope: &mut PrincipalScope<'_>,
        journey_id: &JourneyId,
    ) -> Result<Option<Self>, WithdrawalJourneyError> {
        let mut latest: Option<Record> = None;
        for key in scope.keys(Table::Journeys) {
            if !key.as_str().starts_with(STATE_PREFIX) {
                continue;
            }
            let row = scope
                .get(Table::Journeys, &key)
                .ok_or(WithdrawalJourneyError::Corrupt(
                    "withdrawal state disappeared",
                ))?;
            let record = decode(row.bytes())?;
            if record.journey_id != journey_id.as_str() {
                continue;
            }
            if latest
                .as_ref()
                .is_some_and(|value| value.sequence == record.sequence)
            {
                return Err(WithdrawalJourneyError::Corrupt(
                    "duplicate withdrawal state sequence",
                ));
            }
            if latest
                .as_ref()
                .is_none_or(|value| value.sequence < record.sequence)
            {
                latest = Some(record);
            }
        }
        let Some(record) = latest else {
            return Ok(None);
        };
        let journey = Self { record };
        journey.ensure_pin(scope)?;
        Ok(Some(journey))
    }

    /// Advances at most one durable stage. A transaction with unknown outcome
    /// moves to lookup-only recovery and is never resubmitted under uncertainty.
    ///
    /// # Errors
    ///
    /// Returns typed agent, proof, Paxeer, custody, and storage failures without
    /// manufacturing debit, claim, cancellation, or payout success.
    #[allow(clippy::too_many_arguments, clippy::too_many_lines)]
    pub async fn advance<A: AgentBoundary, R: WithdrawalRuntime>(
        &mut self,
        scope: &mut PrincipalScope<'_>,
        runtime: &mut R,
        boundary: &WithdrawalBoundary,
        agent_contract: &AgentClient,
        agent: &mut A,
        custody: &CustodySigner,
        registry: &ModuleRegistry,
        trace: &TraceId,
        step_up: Option<&StepUpEvidence>,
        now: u64,
    ) -> Result<WithdrawalStatus, WithdrawalJourneyError> {
        if now < self.record.updated_at {
            return Err(WithdrawalJourneyError::TimeRegressed);
        }
        self.ensure_pin(scope)?;
        match self.record.phase {
            Phase::Processing => {
                let plan = self.debit_plan()?;
                let id = JourneyId::new(self.record.debit_journey_id.clone())
                    .map_err(|_| WithdrawalJourneyError::Corrupt("invalid debit journey id"))?;
                let mut debit = JourneyEngine::load(scope, &id)?
                    .unwrap_or(JourneyEngine::start(scope, &plan, registry, now)?);
                let status = debit
                    .advance_authorized(
                        scope,
                        agent_contract,
                        agent,
                        custody,
                        registry,
                        trace,
                        step_up,
                        now,
                    )
                    .await?;
                if status.state() == JourneyState::Refused {
                    return Err(WithdrawalJourneyError::DebitRefused);
                }
                if status.state() == JourneyState::Done {
                    let evidence =
                        debit
                            .verified_leg_evidence(0)?
                            .ok_or(WithdrawalJourneyError::Corrupt(
                                "debit lacks receipt evidence",
                            ))?;
                    let material = match agent
                        .receipt_by_idempotency_key(evidence.action_key, evidence.activity_id)?
                    {
                        ReceiptLookup::Absent => return self.status(),
                        ReceiptLookup::Found(material) => material,
                    };
                    if material.canonical_bytes != evidence.canonical_receipt {
                        return Err(WithdrawalJourneyError::EvidenceConflict);
                    }
                    let expectation = self.debit_expectation(evidence.activity_id)?;
                    let committed = CommittedWithdrawalDebit::verify(
                        &material.canonical_bytes,
                        &material.authorised_batch,
                        expectation,
                    )?;
                    self.record.debit_activity_id = Some(evidence.activity_id);
                    self.record.debit_receipt = Some(material.canonical_bytes);
                    self.record.debit_batch =
                        Some(StoredBatch::from_authorized(&material.authorised_batch));
                    self.record.debit_receipt_reference = Some(committed.receipt_reference());
                    self.transition(scope, Phase::WaitingSettlement, now)?;
                }
            }
            Phase::WaitingSettlement => {
                let expectation = self.debit_expectation(self.debit_activity_id()?)?;
                if let Some(proof) = runtime.checkpoint_proof(&expectation)? {
                    let claim = boundary.construct_claim(self.committed_debit()?, proof.clone())?;
                    if claim.debit().receipt_reference() != self.debit_receipt_reference()? {
                        return Err(WithdrawalJourneyError::EvidenceConflict);
                    }
                    self.record.proof = Some(StoredProof::from_public(&proof));
                    self.transition(scope, Phase::ClaimReady, now)?;
                }
            }
            Phase::ClaimReady => {
                let due = self
                    .record
                    .last_reminder_at
                    .unwrap_or(self.record.updated_at)
                    .saturating_add(self.record.reminder_interval_seconds);
                if now >= due {
                    self.record.reminder_count = self.record.reminder_count.saturating_add(1);
                    self.record.last_reminder_at = Some(now);
                    self.persist_next(scope, now)?;
                }
            }
            Phase::ClaimSubmitting => {
                let request = self.claim_request(boundary)?;
                match runtime.submit_or_resolve(&request) {
                    Ok(PaxeerActionOutcome::Submitted(transaction)) => {
                        self.record.claim_transaction = Some(transaction.bytes());
                        self.transition(scope, Phase::ClaimConfirming, now)?;
                    }
                    Ok(PaxeerActionOutcome::Unknown)
                    | Err(WithdrawalBoundaryError::Unavailable) => {
                        self.transition(scope, Phase::ClaimStillChecking, now)?;
                    }
                    Err(error) => return Err(error.into()),
                }
            }
            Phase::ClaimStillChecking => {
                if let Some(transaction) = runtime.lookup(self.record.claim_action_key)? {
                    self.record.claim_transaction = Some(transaction.bytes());
                    self.transition(scope, Phase::ClaimConfirming, now)?;
                }
            }
            Phase::ClaimConfirming => {
                let transaction = self.claim_transaction()?;
                if let Some(report) = final_report(boundary, transaction)? {
                    let submitted = boundary.accept_submission(self.claim(boundary)?, &report)?;
                    self.record.claim_id = Some(submitted.claim_id());
                    self.record.claim_available_at = Some(submitted.available_at());
                    self.transition(scope, Phase::ClaimQueued, now)?;
                }
            }
            Phase::ClaimQueued | Phase::ChallengeHeld => {
                let submitted = self.submitted_claim(boundary)?;
                match boundary.progress(&submitted)? {
                    ClaimProgress::WaitingForChallengeWindow { .. } => {
                        if self.record.phase != Phase::ClaimQueued {
                            self.record.challenge_hold = None;
                            self.transition(scope, Phase::ClaimQueued, now)?;
                        }
                    }
                    ClaimProgress::ReadyToFinalise { .. } => {
                        self.record.challenge_hold = None;
                        self.transition(scope, Phase::ReadyToFinalise, now)?;
                    }
                    ClaimProgress::ChallengeHeld(hold) => {
                        self.record.challenge_hold = Some(StoredHold::from_public(hold));
                        self.transition(scope, Phase::ChallengeHeld, now)?;
                    }
                    ClaimProgress::ChallengeUpheldAwaitingCancellation { disposition } => {
                        self.record.challenged_disposition_vault =
                            Some(disposition_vault(disposition));
                        self.transition(scope, Phase::CancellationReady, now)?;
                    }
                    ClaimProgress::PaidAwaitingPayoutVerification => {
                        if self.record.payout_transaction.is_none() {
                            if let Some(transaction) =
                                runtime.lookup(self.record.payout_action_key)?
                            {
                                self.record.payout_transaction = Some(transaction.bytes());
                            } else {
                                return Err(WithdrawalJourneyError::EvidenceConflict);
                            }
                        }
                        self.transition(scope, Phase::PayoutConfirming, now)?;
                    }
                    ClaimProgress::Cancelled { disposition } => {
                        self.record.challenged_disposition_vault =
                            Some(disposition_vault(disposition));
                        if self.record.cancellation_transaction.is_none() {
                            if let Some(transaction) =
                                runtime.lookup(self.record.cancellation_action_key)?
                            {
                                self.record.cancellation_transaction = Some(transaction.bytes());
                            } else {
                                return Err(WithdrawalJourneyError::EvidenceConflict);
                            }
                        }
                        self.transition(scope, Phase::CancellationConfirming, now)?;
                    }
                }
            }
            Phase::ReadyToFinalise => {
                self.transition(scope, Phase::PayoutSubmitting, now)?;
            }
            Phase::PayoutSubmitting => {
                let request = self.payout_request(boundary)?;
                match runtime.submit_or_resolve(&request) {
                    Ok(PaxeerActionOutcome::Submitted(transaction)) => {
                        self.record.payout_transaction = Some(transaction.bytes());
                        self.transition(scope, Phase::PayoutConfirming, now)?;
                    }
                    Ok(PaxeerActionOutcome::Unknown)
                    | Err(WithdrawalBoundaryError::Unavailable) => {
                        self.transition(scope, Phase::PayoutStillChecking, now)?;
                    }
                    Err(error) => return Err(error.into()),
                }
            }
            Phase::PayoutStillChecking => {
                if let Some(transaction) = runtime.lookup(self.record.payout_action_key)? {
                    self.record.payout_transaction = Some(transaction.bytes());
                    self.transition(scope, Phase::PayoutConfirming, now)?;
                }
            }
            Phase::PayoutConfirming => {
                let transaction = self.payout_transaction()?;
                if let Some(report) = final_report(boundary, transaction)? {
                    let payout =
                        boundary.verify_payout(&self.submitted_claim(boundary)?, &report)?;
                    self.record.payout = Some(StoredPayout::from_public(&payout));
                    self.transition(scope, Phase::Paid, now)?;
                }
            }
            Phase::CancellationReady => {
                self.transition(scope, Phase::CancellationSubmitting, now)?;
            }
            Phase::CancellationSubmitting => {
                let request = self.cancellation_request(boundary)?;
                match runtime.submit_or_resolve(&request) {
                    Ok(PaxeerActionOutcome::Submitted(transaction)) => {
                        self.record.cancellation_transaction = Some(transaction.bytes());
                        self.transition(scope, Phase::CancellationConfirming, now)?;
                    }
                    Ok(PaxeerActionOutcome::Unknown)
                    | Err(WithdrawalBoundaryError::Unavailable) => {
                        self.transition(scope, Phase::CancellationStillChecking, now)?;
                    }
                    Err(error) => return Err(error.into()),
                }
            }
            Phase::CancellationStillChecking => {
                if let Some(transaction) = runtime.lookup(self.record.cancellation_action_key)? {
                    self.record.cancellation_transaction = Some(transaction.bytes());
                    self.transition(scope, Phase::CancellationConfirming, now)?;
                }
            }
            Phase::CancellationConfirming => {
                let transaction = self.cancellation_transaction()?;
                if let Some(report) = final_report(boundary, transaction)? {
                    let cancellation =
                        boundary.verify_cancellation(&self.submitted_claim(boundary)?, &report)?;
                    self.record.cancellation = Some(StoredCancellation::from_public(&cancellation));
                    self.transition(scope, Phase::Cancelled, now)?;
                }
            }
            Phase::Paid | Phase::Cancelled => {}
        }
        self.status()
    }

    /// Records the user's explicit claim action before the wallet can open.
    /// It is accepted exactly once for this withdrawal.
    ///
    /// # Errors
    ///
    /// Refuses an action before claim readiness or a second claim ceremony.
    pub fn request_claim(
        &mut self,
        scope: &mut PrincipalScope<'_>,
        now: u64,
    ) -> Result<WithdrawalStatus, WithdrawalJourneyError> {
        if self.record.phase != Phase::ClaimReady {
            return Err(WithdrawalJourneyError::ClaimNotReady);
        }
        self.transition(scope, Phase::ClaimSubmitting, now)?;
        self.status()
    }

    /// Returns the current evidence-grounded state without doing external work.
    ///
    /// # Errors
    ///
    /// Refuses malformed persisted evidence.
    pub fn status(&self) -> Result<WithdrawalStatus, WithdrawalJourneyError> {
        let journey_id = JourneyId::new(self.record.journey_id.clone())
            .map_err(|_| WithdrawalJourneyError::Corrupt("invalid withdrawal journey id"))?;
        let stage = match self.record.phase {
            Phase::Processing => WithdrawalStage::Processing,
            Phase::WaitingSettlement => WithdrawalStage::WaitingForSettlement {
                expectation: self.settlement_expectation()?,
            },
            Phase::ClaimReady => WithdrawalStage::ReadyToClaim,
            Phase::ClaimSubmitting | Phase::ClaimStillChecking | Phase::ClaimConfirming => {
                WithdrawalStage::ClaimSubmitting
            }
            Phase::ClaimQueued => WithdrawalStage::WaitingForChallengeWindow {
                available_at: self.claim_available_at()?,
                observed_at: self.record.updated_at,
            },
            Phase::ChallengeHeld => WithdrawalStage::ChallengeHeld(
                self.record
                    .challenge_hold
                    .ok_or(WithdrawalJourneyError::Corrupt("challenge hold absent"))?
                    .public()?,
            ),
            Phase::CancellationReady
            | Phase::CancellationSubmitting
            | Phase::CancellationStillChecking
            | Phase::CancellationConfirming => {
                WithdrawalStage::ChallengeUpheldAwaitingCancellation {
                    disposition: self.cancelled_disposition()?,
                }
            }
            Phase::ReadyToFinalise | Phase::PayoutSubmitting => WithdrawalStage::ReadyToFinalise,
            Phase::PayoutStillChecking | Phase::PayoutConfirming => {
                WithdrawalStage::VerifyingPayout
            }
            Phase::Paid => WithdrawalStage::PaidOut(
                self.record
                    .payout
                    .ok_or(WithdrawalJourneyError::Corrupt("payout evidence absent"))?
                    .public(),
            ),
            Phase::Cancelled => WithdrawalStage::Cancelled(
                self.record
                    .cancellation
                    .ok_or(WithdrawalJourneyError::Corrupt(
                        "cancellation evidence absent",
                    ))?
                    .public(),
            ),
        };
        Ok(WithdrawalStatus {
            journey_id,
            stage,
            cancellation_policy: CancellationPolicy::CannotCancelAfterCommitCompleteOnly,
            debit_receipt_reference: self.record.debit_receipt_reference,
            reminder_count: self.record.reminder_count,
        })
    }

    /// Reads all durable claim reminders in sequence order.
    ///
    /// # Errors
    ///
    /// Refuses corrupt state rows.
    pub fn reminders(
        scope: &PrincipalScope<'_>,
        journey_id: &JourneyId,
    ) -> Result<Vec<WithdrawalReminder>, WithdrawalJourneyError> {
        let mut records = Vec::new();
        for key in scope.keys(Table::Journeys) {
            if !key.as_str().starts_with(STATE_PREFIX) {
                continue;
            }
            let row = scope
                .get(Table::Journeys, &key)
                .ok_or(WithdrawalJourneyError::Corrupt(
                    "withdrawal state disappeared",
                ))?;
            let record = decode(row.bytes())?;
            if record.journey_id == journey_id.as_str() && record.last_reminder_at.is_some() {
                records.push(record);
            }
        }
        records.sort_by_key(|record| record.sequence);
        let mut previous = 0;
        let mut reminders = Vec::new();
        for record in records {
            if record.reminder_count > previous {
                reminders.push(WithdrawalReminder {
                    sequence: record.reminder_count,
                    journey_id: journey_id.clone(),
                    created_at: record
                        .last_reminder_at
                        .ok_or(WithdrawalJourneyError::Corrupt("reminder timestamp absent"))?,
                    deep_link: format!("/app/journeys/{}/claim", journey_id.as_str()),
                });
                previous = record.reminder_count;
            }
        }
        Ok(reminders)
    }

    fn debit_plan(&self) -> Result<JourneyPlan, WithdrawalJourneyError> {
        let intent = BridgeWithdrawRequest::new(
            WithdrawalId::new(self.record.withdrawal_id),
            self.owner()?,
            self.withdrawals_account()?,
            EvmAddress::new(self.record.payout_address),
            AssetId::new(self.record.asset),
            Amount::from_u128(self.record.amount),
            IdempotencyKey::new(self.record.debit_action_key),
        )?;
        let leg = JourneyLeg::new(
            Intent::v1(IntentKind::BridgeWithdrawRequest(intent)),
            self.record.debit_action_key,
            AgentDid::new(self.record.actor.clone())?,
            AuthorityRef::new(self.record.authority.clone())?,
            self.record.account_sequence,
            self.record.not_before,
            self.record.not_after,
            self.record.fee_limit,
        )?;
        Ok(JourneyPlan::new(
            JourneyId::new(self.record.debit_journey_id.clone())
                .map_err(|_| WithdrawalJourneyError::Corrupt("invalid debit journey id"))?,
            self.record.debit_plan_key,
            KeyId::new(self.record.custody_key.clone())
                .map_err(|_| WithdrawalJourneyError::Corrupt("invalid custody key"))?,
            Operation::Withdrawal,
            vec![leg],
        )?)
    }

    fn debit_expectation(
        &self,
        activity_id: [u8; 32],
    ) -> Result<DebitExpectation, WithdrawalJourneyError> {
        Ok(DebitExpectation {
            activity_id,
            network_id: self.record.network_id,
            withdrawal_id: self.record.withdrawal_id,
            account: account_address(&self.owner()?),
            withdrawals_account: account_address(&self.withdrawals_account()?),
            asset_id: self.record.asset,
            amount: self.record.amount,
            recipient: EvmAddress::new(self.record.payout_address),
        })
    }

    fn committed_debit(&self) -> Result<CommittedWithdrawalDebit, WithdrawalJourneyError> {
        let receipt = self
            .record
            .debit_receipt
            .as_ref()
            .ok_or(WithdrawalJourneyError::Corrupt("debit receipt absent"))?;
        let batch = self
            .record
            .debit_batch
            .ok_or(WithdrawalJourneyError::Corrupt("debit batch absent"))?
            .authorized();
        Ok(CommittedWithdrawalDebit::verify(
            receipt,
            &batch,
            self.debit_expectation(self.debit_activity_id()?)?,
        )?)
    }

    fn claim(
        &self,
        boundary: &WithdrawalBoundary,
    ) -> Result<layerx_paxeer_client::WithdrawalClaim, WithdrawalJourneyError> {
        let proof = self
            .record
            .proof
            .as_ref()
            .ok_or(WithdrawalJourneyError::Corrupt("checkpoint proof absent"))?
            .public();
        Ok(boundary.construct_claim(self.committed_debit()?, proof)?)
    }

    fn submitted_claim(
        &self,
        boundary: &WithdrawalBoundary,
    ) -> Result<SubmittedWithdrawalClaim, WithdrawalJourneyError> {
        let transaction = self.claim_transaction()?;
        let report =
            final_report(boundary, transaction)?.ok_or(WithdrawalJourneyError::EvidencePending)?;
        let submitted = boundary.restore_submission(self.claim(boundary)?, &report)?;
        if self.record.claim_id != Some(submitted.claim_id())
            || self.record.claim_available_at != Some(submitted.available_at())
        {
            return Err(WithdrawalJourneyError::EvidenceConflict);
        }
        Ok(submitted)
    }

    fn claim_request(
        &self,
        boundary: &WithdrawalBoundary,
    ) -> Result<WithdrawalTransactionRequest, WithdrawalJourneyError> {
        let claim = self.claim(boundary)?;
        Ok(WithdrawalTransactionRequest {
            action_key: self.record.claim_action_key,
            action: PaxeerAction::QueueClaim,
            target: claim.contract(),
            calldata: claim.calldata().to_vec(),
        })
    }

    fn payout_request(
        &self,
        boundary: &WithdrawalBoundary,
    ) -> Result<WithdrawalTransactionRequest, WithdrawalJourneyError> {
        let submitted = self.submitted_claim(boundary)?;
        Ok(WithdrawalTransactionRequest {
            action_key: self.record.payout_action_key,
            action: PaxeerAction::FinalisePayout,
            target: boundary.claims_contract(),
            calldata: submitted.finalise_calldata(),
        })
    }

    fn cancellation_request(
        &self,
        boundary: &WithdrawalBoundary,
    ) -> Result<WithdrawalTransactionRequest, WithdrawalJourneyError> {
        let submitted = self.submitted_claim(boundary)?;
        Ok(WithdrawalTransactionRequest {
            action_key: self.record.cancellation_action_key,
            action: PaxeerAction::CancelChallengedPayout,
            target: boundary.claims_contract(),
            calldata: submitted.cancellation_calldata(),
        })
    }

    fn settlement_expectation(&self) -> Result<SettlementExpectation, WithdrawalJourneyError> {
        SettlementConfig {
            checkpoint_interval_seconds: self.record.checkpoint_interval_seconds,
            paxeer_block_seconds: self.record.paxeer_block_seconds,
            required_confirmations: self.record.required_confirmations,
        }
        .expectation()
    }

    fn cancelled_disposition(&self) -> Result<CancelledFundsDisposition, WithdrawalJourneyError> {
        Ok(CancelledFundsDisposition {
            paxeer: PaxeerFundsDisposition::RetainedInVault {
                vault: EvmAddress::new(self.record.challenged_disposition_vault.ok_or(
                    WithdrawalJourneyError::Corrupt("challenged vault disposition absent"),
                )?),
                asset_id: self.record.asset,
                amount: self.record.amount,
            },
            layerx: ProtocolDebitDisposition::RemainsCommittedPendingProtocolRecovery {
                debit_receipt_reference: self.debit_receipt_reference()?,
            },
        })
    }

    fn owner(&self) -> Result<AccountId, WithdrawalJourneyError> {
        AccountId::parse(&self.record.owner)
            .map_err(|_| WithdrawalJourneyError::Corrupt("invalid withdrawal owner"))
    }

    fn withdrawals_account(&self) -> Result<AccountId, WithdrawalJourneyError> {
        AccountId::parse(&self.record.withdrawals_account)
            .map_err(|_| WithdrawalJourneyError::Corrupt("invalid withdrawals account"))
    }

    fn debit_activity_id(&self) -> Result<[u8; 32], WithdrawalJourneyError> {
        self.record
            .debit_activity_id
            .ok_or(WithdrawalJourneyError::Corrupt("debit activity absent"))
    }

    fn debit_receipt_reference(&self) -> Result<[u8; 32], WithdrawalJourneyError> {
        self.record
            .debit_receipt_reference
            .ok_or(WithdrawalJourneyError::Corrupt(
                "debit receipt reference absent",
            ))
    }

    fn claim_transaction(&self) -> Result<TransactionHash, WithdrawalJourneyError> {
        self.record
            .claim_transaction
            .map(TransactionHash::new)
            .ok_or(WithdrawalJourneyError::Corrupt("claim transaction absent"))
    }

    fn payout_transaction(&self) -> Result<TransactionHash, WithdrawalJourneyError> {
        self.record
            .payout_transaction
            .map(TransactionHash::new)
            .ok_or(WithdrawalJourneyError::Corrupt("payout transaction absent"))
    }

    fn cancellation_transaction(&self) -> Result<TransactionHash, WithdrawalJourneyError> {
        self.record
            .cancellation_transaction
            .map(TransactionHash::new)
            .ok_or(WithdrawalJourneyError::Corrupt(
                "cancellation transaction absent",
            ))
    }

    fn claim_available_at(&self) -> Result<u64, WithdrawalJourneyError> {
        self.record
            .claim_available_at
            .ok_or(WithdrawalJourneyError::Corrupt("claim availability absent"))
    }

    fn transition(
        &mut self,
        scope: &mut PrincipalScope<'_>,
        phase: Phase,
        now: u64,
    ) -> Result<(), WithdrawalJourneyError> {
        self.record.phase = phase;
        self.persist_next(scope, now)
    }

    fn persist_next(
        &mut self,
        scope: &mut PrincipalScope<'_>,
        now: u64,
    ) -> Result<(), WithdrawalJourneyError> {
        if now < self.record.updated_at {
            return Err(WithdrawalJourneyError::TimeRegressed);
        }
        let previous = record_digest(&self.record)?;
        self.record.sequence = self.record.sequence.saturating_add(1);
        self.record.previous_digest = Some(previous);
        self.record.updated_at = now;
        self.write_state(scope)?;
        self.ensure_pin(scope)
    }

    fn write_state(&self, scope: &mut PrincipalScope<'_>) -> Result<(), WithdrawalJourneyError> {
        validate_record(&self.record)?;
        let bytes = encode(&self.record)?;
        let key = state_row(self.record.idempotency_key, self.record.sequence)?;
        if let Some(existing) = scope.get(Table::Journeys, &key) {
            if existing.bytes() != bytes {
                return Err(WithdrawalJourneyError::EvidenceConflict);
            }
            return Ok(());
        }
        scope.put(Table::Journeys, key, self.record.updated_at, bytes)?;
        Ok(())
    }

    fn ensure_pin(&self, scope: &mut PrincipalScope<'_>) -> Result<(), WithdrawalJourneyError> {
        let pin = pin_row(self.record.idempotency_key, self.record.sequence)?;
        if scope.audit(&pin).is_some() {
            return Ok(());
        }
        let state = state_row(self.record.idempotency_key, self.record.sequence)?;
        scope.append_audit(
            pin,
            self.record.updated_at,
            record_digest(&self.record)?.to_vec(),
            AuditDisposition::Exportable {
                evidence: vec![EvidenceRef::new(Table::Journeys, state)],
            },
        )?;
        Ok(())
    }
}

impl StoredPayout {
    fn from_public(value: &PayoutEvidence) -> Self {
        Self {
            debit_receipt_reference: value.debit_receipt_reference,
            checkpoint_hash: value.checkpoint_hash,
            claim_id: value.claim_id,
            transaction: value.payout_transaction.bytes(),
            inclusion: StoredInclusion::from_public(value.payout_inclusion),
            vault: value.vault.bytes(),
            token: value.token.bytes(),
            asset: value.asset_id,
            recipient: value.recipient.bytes(),
            amount: value.amount,
        }
    }

    const fn public(self) -> PayoutEvidence {
        PayoutEvidence {
            debit_receipt_reference: self.debit_receipt_reference,
            checkpoint_hash: self.checkpoint_hash,
            claim_id: self.claim_id,
            payout_transaction: TransactionHash::new(self.transaction),
            payout_inclusion: self.inclusion.public(),
            vault: EvmAddress::new(self.vault),
            token: EvmAddress::new(self.token),
            asset_id: self.asset,
            recipient: EvmAddress::new(self.recipient),
            amount: self.amount,
        }
    }
}

impl StoredCancellation {
    fn from_public(value: &CancellationEvidence) -> Self {
        let PaxeerFundsDisposition::RetainedInVault {
            vault,
            asset_id,
            amount,
        } = value.disposition.paxeer;
        Self {
            debit_receipt_reference: value.debit_receipt_reference,
            checkpoint_hash: value.checkpoint_hash,
            claim_id: value.claim_id,
            transaction: value.cancellation_transaction.bytes(),
            inclusion: StoredInclusion::from_public(value.cancellation_inclusion),
            vault: vault.bytes(),
            asset: asset_id,
            amount,
        }
    }

    const fn public(self) -> CancellationEvidence {
        CancellationEvidence {
            debit_receipt_reference: self.debit_receipt_reference,
            checkpoint_hash: self.checkpoint_hash,
            claim_id: self.claim_id,
            cancellation_transaction: TransactionHash::new(self.transaction),
            cancellation_inclusion: self.inclusion.public(),
            disposition: CancelledFundsDisposition {
                paxeer: PaxeerFundsDisposition::RetainedInVault {
                    vault: EvmAddress::new(self.vault),
                    asset_id: self.asset,
                    amount: self.amount,
                },
                layerx: ProtocolDebitDisposition::RemainsCommittedPendingProtocolRecovery {
                    debit_receipt_reference: self.debit_receipt_reference,
                },
            },
        }
    }
}

impl StoredInclusion {
    const fn from_public(value: layerx_paxeer_client::TransactionInclusion) -> Self {
        Self {
            block_number: value.block.number,
            block_hash: value.block.hash,
            transaction_index: value.transaction_index,
        }
    }

    const fn public(self) -> layerx_paxeer_client::TransactionInclusion {
        layerx_paxeer_client::TransactionInclusion {
            block: layerx_paxeer_client::BlockRef {
                number: self.block_number,
                hash: self.block_hash,
            },
            transaction_index: self.transaction_index,
            execution: layerx_paxeer_client::ExecutionOutcome::Succeeded,
            deployed_contract: None,
        }
    }
}

fn final_report(
    boundary: &WithdrawalBoundary,
    transaction: TransactionHash,
) -> Result<Option<layerx_paxeer_client::FinalityReport>, WithdrawalJourneyError> {
    let mut tracker = boundary.track(transaction)?;
    let report = tracker.poll();
    match report.stage() {
        FinalityStage::Final { .. } => Ok(Some(report)),
        FinalityStage::Displaced { .. } => Err(WithdrawalJourneyError::TransactionDisplaced),
        FinalityStage::Announced
        | FinalityStage::Missing { .. }
        | FinalityStage::Pooled { .. }
        | FinalityStage::Confirming { .. } => Ok(None),
    }
}

fn disposition_vault(disposition: CancelledFundsDisposition) -> [u8; 20] {
    let PaxeerFundsDisposition::RetainedInVault { vault, .. } = disposition.paxeer;
    vault.bytes()
}

fn validate_plan(plan: &WithdrawalPlan) -> Result<(), WithdrawalJourneyError> {
    if plan.idempotency_key == [0; 32]
        || plan.withdrawal_id.is_zero()
        || plan.amount.value() == 0
        || plan.owner == plan.withdrawals_account
        || plan.payout_address.bytes() == [0; 20]
        || plan.reminder_interval_seconds == 0
        || plan.agent.not_after < plan.agent.not_before
        || plan.currency.is_empty()
        || plan.currency.len() > 12
        || !plan
            .currency
            .bytes()
            .all(|byte| byte.is_ascii_uppercase() || byte.is_ascii_digit())
    {
        return Err(WithdrawalJourneyError::InvalidPlan);
    }
    let _ = plan.settlement.expectation()?;
    Ok(())
}

fn validate_record(record: &Record) -> Result<(), WithdrawalJourneyError> {
    if record.version != RECORD_VERSION
        || JourneyId::new(record.journey_id.clone()).is_err()
        || record.idempotency_key == [0; 32]
        || record.plan_digest == [0; 32]
        || record.debit_plan_key == [0; 32]
        || record.debit_action_key == [0; 32]
        || record.claim_action_key == [0; 32]
        || record.payout_action_key == [0; 32]
        || record.cancellation_action_key == [0; 32]
        || record.network_id == 0
        || record.withdrawal_id == [0; 32]
        || record.payout_address == [0; 20]
        || record.asset == [0; 32]
        || record.amount == 0
        || record.updated_at < record.started_at
        || (record.sequence == 0) != record.previous_digest.is_none()
        || AccountId::parse(&record.owner).is_err()
        || AccountId::parse(&record.withdrawals_account).is_err()
        || AgentDid::new(record.actor.clone()).is_err()
        || AuthorityRef::new(record.authority.clone()).is_err()
        || KeyId::new(record.custody_key.clone()).is_err()
        || JourneyId::new(record.debit_journey_id.clone()).is_err()
        || (SettlementConfig {
            checkpoint_interval_seconds: record.checkpoint_interval_seconds,
            paxeer_block_seconds: record.paxeer_block_seconds,
            required_confirmations: record.required_confirmations,
        })
        .expectation()
        .is_err()
        || record.reminder_interval_seconds == 0
        || matches!(record.phase, Phase::Processing) && record.debit_receipt.is_some()
        || !matches!(record.phase, Phase::Processing)
            && (record.debit_receipt.is_none()
                || record.debit_batch.is_none()
                || record.debit_activity_id.is_none()
                || record.debit_receipt_reference.is_none())
        || matches!(
            record.phase,
            Phase::ClaimReady
                | Phase::ClaimSubmitting
                | Phase::ClaimStillChecking
                | Phase::ClaimConfirming
                | Phase::ClaimQueued
                | Phase::ChallengeHeld
                | Phase::ReadyToFinalise
                | Phase::PayoutSubmitting
                | Phase::PayoutStillChecking
                | Phase::PayoutConfirming
                | Phase::CancellationReady
                | Phase::CancellationSubmitting
                | Phase::CancellationStillChecking
                | Phase::CancellationConfirming
                | Phase::Paid
                | Phase::Cancelled
        ) && record.proof.is_none()
        || matches!(
            record.phase,
            Phase::ClaimConfirming
                | Phase::ClaimQueued
                | Phase::ChallengeHeld
                | Phase::ReadyToFinalise
                | Phase::PayoutSubmitting
                | Phase::PayoutStillChecking
                | Phase::PayoutConfirming
                | Phase::CancellationReady
                | Phase::CancellationSubmitting
                | Phase::CancellationStillChecking
                | Phase::CancellationConfirming
                | Phase::Paid
                | Phase::Cancelled
        ) && record.claim_transaction.is_none()
        || matches!(
            record.phase,
            Phase::ClaimQueued
                | Phase::ChallengeHeld
                | Phase::ReadyToFinalise
                | Phase::PayoutSubmitting
                | Phase::PayoutStillChecking
                | Phase::PayoutConfirming
                | Phase::CancellationReady
                | Phase::CancellationSubmitting
                | Phase::CancellationStillChecking
                | Phase::CancellationConfirming
                | Phase::Paid
                | Phase::Cancelled
        ) && (record.claim_id.is_none() || record.claim_available_at.is_none())
        || record.phase == Phase::Paid && record.payout.is_none()
        || record.phase == Phase::Cancelled && record.cancellation.is_none()
    {
        return Err(WithdrawalJourneyError::Corrupt(
            "withdrawal invariants are invalid",
        ));
    }
    Ok(())
}

fn latest_for_key(
    scope: &PrincipalScope<'_>,
    key: [u8; 32],
) -> Result<Option<Record>, WithdrawalJourneyError> {
    let prefix = format!("{STATE_PREFIX}{}-", hex(&key));
    let mut latest: Option<Record> = None;
    for row_key in scope.keys(Table::Journeys) {
        if !row_key.as_str().starts_with(&prefix) {
            continue;
        }
        let row = scope
            .get(Table::Journeys, &row_key)
            .ok_or(WithdrawalJourneyError::Corrupt(
                "withdrawal state disappeared",
            ))?;
        let record = decode(row.bytes())?;
        if latest
            .as_ref()
            .is_none_or(|value| value.sequence < record.sequence)
        {
            latest = Some(record);
        }
    }
    Ok(latest)
}

fn encode(record: &Record) -> Result<Vec<u8>, WithdrawalJourneyError> {
    serde_json::to_vec(record)
        .map_err(|_| WithdrawalJourneyError::Corrupt("withdrawal cannot be encoded"))
}

fn decode(bytes: &[u8]) -> Result<Record, WithdrawalJourneyError> {
    let record = serde_json::from_slice(bytes)
        .map_err(|_| WithdrawalJourneyError::Corrupt("invalid withdrawal encoding"))?;
    validate_record(&record)?;
    Ok(record)
}

fn record_digest(record: &Record) -> Result<[u8; 32], WithdrawalJourneyError> {
    Ok(Sha256::digest(encode(record)?).into())
}

fn state_row(key: [u8; 32], sequence: u64) -> Result<RowKey, StoreError> {
    RowKey::new(format!("{STATE_PREFIX}{}-{sequence:020}", hex(&key)))
}

fn pin_row(key: [u8; 32], sequence: u64) -> Result<RowKey, StoreError> {
    RowKey::new(format!("{PIN_PREFIX}{}-{sequence:020}", hex(&key)))
}

fn derive_key(domain: &[u8], input: &[u8; 32]) -> [u8; 32] {
    let mut digest = Sha256::new();
    digest.update(domain);
    digest.update(input);
    digest.finalize().into()
}

fn derived_journey_id(label: &str, key: [u8; 32]) -> Result<String, WithdrawalJourneyError> {
    let value = format!("jrn_{label}{}", &hex(&key)[..32]);
    JourneyId::new(value.clone())
        .map_err(|_| WithdrawalJourneyError::Corrupt("invalid derived journey id"))?;
    Ok(value)
}

fn plan_digest(plan: &WithdrawalPlan) -> [u8; 32] {
    let mut digest = Sha256::new();
    digest.update(PLAN_DIGEST_DOMAIN);
    hash_text(&mut digest, plan.journey_id.as_str());
    digest.update(plan.idempotency_key);
    digest.update(plan.network.value().to_be_bytes());
    digest.update(plan.withdrawal_id.bytes());
    hash_text(&mut digest, plan.owner.canonical());
    hash_text(&mut digest, plan.withdrawals_account.canonical());
    digest.update(plan.payout_address.bytes());
    digest.update(plan.asset.bytes());
    digest.update(plan.amount.value().to_be_bytes());
    hash_text(&mut digest, &plan.currency);
    digest.update(plan.settlement.checkpoint_interval_seconds.to_be_bytes());
    digest.update(plan.settlement.paxeer_block_seconds.to_be_bytes());
    digest.update(plan.settlement.required_confirmations.to_be_bytes());
    digest.update(plan.reminder_interval_seconds.to_be_bytes());
    hash_text(&mut digest, plan.agent.actor.as_str());
    hash_text(&mut digest, plan.agent.authority.as_str());
    digest.update(plan.agent.account_sequence.to_be_bytes());
    digest.update(plan.agent.not_before.to_be_bytes());
    digest.update(plan.agent.not_after.to_be_bytes());
    digest.update(plan.agent.fee_limit.to_be_bytes());
    hash_text(&mut digest, plan.agent.custody_key.as_str());
    digest.finalize().into()
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

/// Typed withdrawal journey failure. Transient errors never become economic results.
#[derive(Debug)]
pub enum WithdrawalJourneyError {
    Store(StoreError),
    Contract(ContractError),
    Intent(layerx_intents::IntentError),
    Journey(JourneyError),
    Agent(AgentBoundaryError),
    Boundary(WithdrawalBoundaryError),
    Paxeer(WithdrawalError),
    Debit(DebitFault),
    Tracker(layerx_paxeer_client::TrackerConfigError),
    InvalidPlan,
    IdempotencyConflict,
    TimeRegressed,
    ClaimNotReady,
    DebitRefused,
    EvidencePending,
    EvidenceConflict,
    TransactionDisplaced,
    Corrupt(&'static str),
}

impl Display for WithdrawalJourneyError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Store(error) => write!(formatter, "withdrawal store failure: {error}"),
            Self::Contract(error) => write!(formatter, "withdrawal contract failure: {error:?}"),
            Self::Intent(error) => write!(formatter, "withdrawal intent failure: {error:?}"),
            Self::Journey(error) => write!(formatter, "withdrawal debit failure: {error}"),
            Self::Agent(error) => write!(formatter, "withdrawal agent failure: {error:?}"),
            Self::Boundary(error) => write!(formatter, "withdrawal boundary failure: {error:?}"),
            Self::Paxeer(error) => write!(formatter, "withdrawal Paxeer failure: {error:?}"),
            Self::Debit(error) => write!(formatter, "withdrawal receipt failure: {error:?}"),
            Self::Tracker(error) => write!(formatter, "withdrawal tracker failure: {error:?}"),
            Self::InvalidPlan => formatter.write_str("withdrawal plan is invalid"),
            Self::IdempotencyConflict => {
                formatter.write_str("withdrawal idempotency key owns another request")
            }
            Self::TimeRegressed => formatter.write_str("withdrawal journey time regressed"),
            Self::ClaimNotReady => formatter.write_str("withdrawal claim is not ready"),
            Self::DebitRefused => formatter.write_str("withdrawal debit was refused"),
            Self::EvidencePending => formatter.write_str("withdrawal evidence is still pending"),
            Self::EvidenceConflict => formatter.write_str("withdrawal evidence conflicts"),
            Self::TransactionDisplaced => {
                formatter.write_str("withdrawal transaction was displaced")
            }
            Self::Corrupt(reason) => write!(formatter, "corrupt withdrawal journey: {reason}"),
        }
    }
}

impl std::error::Error for WithdrawalJourneyError {}

impl From<StoreError> for WithdrawalJourneyError {
    fn from(value: StoreError) -> Self {
        Self::Store(value)
    }
}

impl From<ContractError> for WithdrawalJourneyError {
    fn from(value: ContractError) -> Self {
        Self::Contract(value)
    }
}

impl From<layerx_intents::IntentError> for WithdrawalJourneyError {
    fn from(value: layerx_intents::IntentError) -> Self {
        Self::Intent(value)
    }
}

impl From<JourneyError> for WithdrawalJourneyError {
    fn from(value: JourneyError) -> Self {
        Self::Journey(value)
    }
}

impl From<AgentBoundaryError> for WithdrawalJourneyError {
    fn from(value: AgentBoundaryError) -> Self {
        Self::Agent(value)
    }
}

impl From<WithdrawalBoundaryError> for WithdrawalJourneyError {
    fn from(value: WithdrawalBoundaryError) -> Self {
        Self::Boundary(value)
    }
}

impl From<WithdrawalError> for WithdrawalJourneyError {
    fn from(value: WithdrawalError) -> Self {
        Self::Paxeer(value)
    }
}

impl From<DebitFault> for WithdrawalJourneyError {
    fn from(value: DebitFault) -> Self {
        Self::Debit(value)
    }
}

impl From<layerx_paxeer_client::TrackerConfigError> for WithdrawalJourneyError {
    fn from(value: layerx_paxeer_client::TrackerConfigError) -> Self {
        Self::Tracker(value)
    }
}
