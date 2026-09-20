//! Receipt-gated, indefinitely durable withdrawal and Paxeer claim journey.

use std::fmt::{Display, Formatter};

use layerx_agent_api::identity::{AgentDid, AuthorityRef, ContractError};
use layerx_intents::{BridgeWithdrawRequest, Intent, IntentKind};
use layerx_paxeer_client::{
    CancellationEvidence, CancelledFundsDisposition, ClaimProgress, CommittedWithdrawalDebit,
    DebitExpectation, DebitFault, FinalityStage, PaxeerFundsDisposition, PayoutEvidence,
    ProtocolDebitDisposition, SubmittedWithdrawalClaim, TransactionHash, WithdrawalBoundary,
    WithdrawalError, WithdrawalMaterial,
};
use layerx_proof::receipt::AuthorizedBatch;
use layerx_sdk::Client as AgentClient;
use layerx_types::account::AccountId;
use layerx_types::amount::Amount;
use layerx_types::ids::{AssetId, CheckpointId, IdempotencyKey};
use layerx_types::intent::{EvmAddress, NetworkId};
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

const RECORD_VERSION: u8 = 5;
const STATE_PREFIX: &str = "withdraw-state-";
const PIN_PREFIX: &str = "withdraw-pin-";
const PLAN_DIGEST_DOMAIN: &[u8] = b"layerx-human-withdraw-plan/v2\0";
const DEBIT_PLAN_DOMAIN: &[u8] = b"layerx-human-withdraw-debit-plan/v2\0";
const DEBIT_ACTION_DOMAIN: &[u8] = b"layerx-human-withdraw-debit/v1\0";
const CLAIM_ACTION_DOMAIN: &[u8] = b"layerx-human-withdraw-claim/v1\0";
const PAYOUT_ACTION_DOMAIN: &[u8] = b"layerx-human-withdraw-payout/v1\0";

/// Bound of one encoded [`WithdrawalMaterial`]: the version and tag bytes, the
/// three length-prefixed evidence parts at the custody evidence bound and the
/// sequencer header signature.
pub(crate) const MATERIAL_WIRE_BYTES: usize =
    2 + 3 * (4 + layerx_paxeer_client::custody::MAX_EVIDENCE_BYTES) + 64;

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
    pub layerx_protocol_version: u16,
    pub request_anchor: CheckpointId,
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

/// Encodes the complete withdrawal plan in canonical provider-wire order.
pub(crate) fn encode_withdrawal_plan(
    plan: &WithdrawalPlan,
) -> Result<Vec<u8>, WithdrawalJourneyError> {
    validate_plan(plan)?;
    let mut out = super::wire::Writer::new(6);
    out.text(plan.journey_id.as_str())
        .map_err(|()| WithdrawalJourneyError::InvalidPlan)?;
    out.fixed(&plan.idempotency_key);
    out.u32(plan.network.value());
    out.u16(plan.layerx_protocol_version);
    out.fixed(&plan.request_anchor.bytes());
    out.text(plan.owner.canonical())
        .map_err(|()| WithdrawalJourneyError::InvalidPlan)?;
    out.text(plan.withdrawals_account.canonical())
        .map_err(|()| WithdrawalJourneyError::InvalidPlan)?;
    out.fixed(&plan.payout_address.bytes());
    out.fixed(&plan.asset.bytes());
    out.u128(plan.amount.value());
    out.text(&plan.currency)
        .map_err(|()| WithdrawalJourneyError::InvalidPlan)?;
    out.u64(plan.settlement.checkpoint_interval_seconds);
    out.u64(plan.settlement.paxeer_block_seconds);
    out.u64(plan.settlement.required_confirmations);
    out.u64(plan.reminder_interval_seconds);
    out.text(plan.agent.actor.as_str())
        .map_err(|()| WithdrawalJourneyError::InvalidPlan)?;
    out.text(plan.agent.authority.as_str())
        .map_err(|()| WithdrawalJourneyError::InvalidPlan)?;
    out.u64(plan.agent.account_sequence);
    out.u64(plan.agent.not_before);
    out.u64(plan.agent.not_after);
    out.u128(plan.agent.fee_limit);
    out.text(plan.agent.custody_key.as_str())
        .map_err(|()| WithdrawalJourneyError::InvalidPlan)?;
    Ok(out.finish())
}

/// Decodes and validates one canonical withdrawal plan with no trailing data.
pub(crate) fn decode_withdrawal_plan(
    bytes: &[u8],
) -> Result<WithdrawalPlan, WithdrawalJourneyError> {
    let mut input =
        super::wire::Reader::new(bytes, 6).map_err(|()| WithdrawalJourneyError::InvalidPlan)?;
    let plan = WithdrawalPlan {
        journey_id: JourneyId::new(
            input
                .text()
                .map_err(|()| WithdrawalJourneyError::InvalidPlan)?,
        )
        .map_err(|_| WithdrawalJourneyError::InvalidPlan)?,
        idempotency_key: input
            .fixed()
            .map_err(|()| WithdrawalJourneyError::InvalidPlan)?,
        network: NetworkId::new(
            input
                .u32()
                .map_err(|()| WithdrawalJourneyError::InvalidPlan)?,
        )
        .map_err(|_| WithdrawalJourneyError::InvalidPlan)?,
        layerx_protocol_version: input
            .u16()
            .map_err(|()| WithdrawalJourneyError::InvalidPlan)?,
        request_anchor: CheckpointId::new(
            input
                .fixed()
                .map_err(|()| WithdrawalJourneyError::InvalidPlan)?,
        ),
        owner: AccountId::parse(
            &input
                .text()
                .map_err(|()| WithdrawalJourneyError::InvalidPlan)?,
        )
        .map_err(|_| WithdrawalJourneyError::InvalidPlan)?,
        withdrawals_account: AccountId::parse(
            &input
                .text()
                .map_err(|()| WithdrawalJourneyError::InvalidPlan)?,
        )
        .map_err(|_| WithdrawalJourneyError::InvalidPlan)?,
        payout_address: EvmAddress::new(
            input
                .fixed()
                .map_err(|()| WithdrawalJourneyError::InvalidPlan)?,
        ),
        asset: AssetId::new(
            input
                .fixed()
                .map_err(|()| WithdrawalJourneyError::InvalidPlan)?,
        ),
        amount: Amount::from_u128(
            input
                .u128()
                .map_err(|()| WithdrawalJourneyError::InvalidPlan)?,
        ),
        currency: input
            .text()
            .map_err(|()| WithdrawalJourneyError::InvalidPlan)?,
        settlement: SettlementConfig {
            checkpoint_interval_seconds: input
                .u64()
                .map_err(|()| WithdrawalJourneyError::InvalidPlan)?,
            paxeer_block_seconds: input
                .u64()
                .map_err(|()| WithdrawalJourneyError::InvalidPlan)?,
            required_confirmations: input
                .u64()
                .map_err(|()| WithdrawalJourneyError::InvalidPlan)?,
        },
        reminder_interval_seconds: input
            .u64()
            .map_err(|()| WithdrawalJourneyError::InvalidPlan)?,
        agent: decode_withdrawal_agent(&mut input)?,
    };
    input
        .finish()
        .map_err(|()| WithdrawalJourneyError::InvalidPlan)?;
    validate_plan(&plan)?;
    Ok(plan)
}

fn decode_withdrawal_agent(
    input: &mut super::wire::Reader<'_>,
) -> Result<WithdrawalAgentPlan, WithdrawalJourneyError> {
    Ok(WithdrawalAgentPlan {
        actor: AgentDid::new(
            input
                .text()
                .map_err(|()| WithdrawalJourneyError::InvalidPlan)?,
        )
        .map_err(|_| WithdrawalJourneyError::InvalidPlan)?,
        authority: AuthorityRef::new(
            input
                .text()
                .map_err(|()| WithdrawalJourneyError::InvalidPlan)?,
        )
        .map_err(|_| WithdrawalJourneyError::InvalidPlan)?,
        account_sequence: input
            .u64()
            .map_err(|()| WithdrawalJourneyError::InvalidPlan)?,
        not_before: input
            .u64()
            .map_err(|()| WithdrawalJourneyError::InvalidPlan)?,
        not_after: input
            .u64()
            .map_err(|()| WithdrawalJourneyError::InvalidPlan)?,
        fee_limit: input
            .u128()
            .map_err(|()| WithdrawalJourneyError::InvalidPlan)?,
        custody_key: KeyId::new(
            input
                .text()
                .map_err(|()| WithdrawalJourneyError::InvalidPlan)?,
        )
        .map_err(|_| WithdrawalJourneyError::InvalidPlan)?,
    })
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
}

/// Exact wallet or permissionless transaction request. An adapter must bind
/// `action_key` durably before broadcast and return the original transaction
/// when the same request is recovered after an acknowledgement gap.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct WithdrawalTransactionRequest {
    pub signed_transaction: Option<Vec<u8>>,
    pub identity: super::MovementExecutionIdentity,
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
    /// Persists the verified request-to-activity identity before settlement.
    /// # Errors
    /// Refuses unavailable storage or a conflicting receipt mapping.
    fn bind_debit(
        &mut self,
        identity: &super::MovementExecutionIdentity,
        debit: &CommittedWithdrawalDebit,
    ) -> Result<(), WithdrawalBoundaryError>;

    /// Verifies a wallet signature against the exact persisted claim request
    /// and returns the complete transaction bytes authorised by that signature.
    /// # Errors
    /// Refuses a signature that does not authorize the exact withdrawal transaction.
    fn verify_claim_signature(
        &mut self,
        request: &WithdrawalTransactionRequest,
        signature: &[u8],
    ) -> Result<Vec<u8>, WithdrawalBoundaryError>;

    /// Returns the real withdrawal material the custody precompile takes - the
    /// receipt, its inclusion path, the batch header and the sequencer
    /// signature - or `None` while settlement is pending.
    ///
    /// # Errors
    ///
    /// Returns a stable boundary failure without manufacturing evidence.
    fn withdrawal_material(
        &mut self,
        debit: &DebitExpectation,
    ) -> Result<Option<WithdrawalMaterial>, WithdrawalBoundaryError>;

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
    WaitingForSettlement { expectation: SettlementExpectation },
    ReadyToClaim,
    ClaimSubmitting,
    WaitingForChallengeWindow { available_at: u64, observed_at: u64 },
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
    withdrawal_id: Option<[u8; 32]>,
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
    pub const fn withdrawal_id(&self) -> Option<[u8; 32]> {
        self.withdrawal_id
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
    ReadyToFinalise,
    PayoutSubmitting,
    PayoutStillChecking,
    PayoutConfirming,
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

/// Persisted custody withdrawal material in the canonical privilege-boundary
/// wire form, so a restored journey reconstructs exactly the evidence the
/// precompile accepted.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
struct StoredMaterial {
    wire: Vec<u8>,
}

impl StoredMaterial {
    fn from_public(value: &WithdrawalMaterial) -> Result<Self, WithdrawalJourneyError> {
        Ok(Self {
            wire: layerx_paxeer_client::wire::encode_withdrawal_material(
                value,
                MATERIAL_WIRE_BYTES,
            )
            .map_err(|_| WithdrawalJourneyError::Corrupt("withdrawal material is unencodable"))?,
        })
    }

    fn public(&self) -> Result<WithdrawalMaterial, WithdrawalJourneyError> {
        layerx_paxeer_client::wire::decode_withdrawal_material(&self.wire, MATERIAL_WIRE_BYTES)
            .map_err(|_| WithdrawalJourneyError::Corrupt("withdrawal material is invalid"))
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
    observed_head: u64,
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
    layerx_protocol_version: u16,
    request_anchor: [u8; 32],
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
    phase: Phase,
    debit_activity_id: Option<[u8; 32]>,
    debit_receipt: Option<Vec<u8>>,
    debit_batch: Option<StoredBatch>,
    debit_receipt_reference: Option<[u8; 32]>,
    material: Option<StoredMaterial>,
    claim_transaction: Option<[u8; 32]>,
    claim_id: Option<[u8; 32]>,
    claim_available_at: Option<u64>,
    payout_transaction: Option<[u8; 32]>,
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
    /// Verifies and submits the user's external claim signature exactly once.
    /// An unknown broadcast outcome moves immediately to lookup-only recovery.
    /// # Errors
    /// Refuses invalid journey state, changed claim authorization, or rejected settlement submission.
    pub fn claim_external_signature<R: WithdrawalRuntime>(
        &mut self,
        scope: &mut PrincipalScope<'_>,
        runtime: &mut R,
        boundary: &WithdrawalBoundary,
        signature: &[u8],
        now: u64,
    ) -> Result<WithdrawalStatus, WithdrawalJourneyError> {
        if boundary.protocol_version() != self.record.layerx_protocol_version {
            return Err(WithdrawalJourneyError::InvalidPlan);
        }
        if now < self.record.updated_at {
            return Err(WithdrawalJourneyError::TimeRegressed);
        }
        if self.record.phase != Phase::ClaimReady {
            return Err(WithdrawalJourneyError::ClaimNotReady);
        }
        let mut request = self.claim_request(scope, boundary)?;
        let signed = runtime.verify_claim_signature(&request, signature)?;
        if signed.is_empty() || signed == request.calldata {
            return Err(WithdrawalJourneyError::Boundary(
                WithdrawalBoundaryError::ContractViolation,
            ));
        }
        request.signed_transaction = Some(signed);
        match runtime.submit_or_resolve(&request) {
            Ok(PaxeerActionOutcome::Submitted(transaction)) => {
                if transaction.bytes() == [0; 32] {
                    return Err(WithdrawalJourneyError::Boundary(
                        WithdrawalBoundaryError::ContractViolation,
                    ));
                }
                self.record.claim_transaction = Some(transaction.bytes());
                self.transition(scope, Phase::ClaimConfirming, now)?;
            }
            Ok(PaxeerActionOutcome::Unknown) | Err(WithdrawalBoundaryError::Unavailable) => {
                self.transition(scope, Phase::ClaimStillChecking, now)?;
            }
            Err(error) => return Err(error.into()),
        }
        self.status()
    }

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
            layerx_protocol_version: plan.layerx_protocol_version,
            request_anchor: plan.request_anchor.bytes(),
            withdrawal_id: [0; 32],
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
            phase: Phase::Processing,
            debit_activity_id: None,
            debit_receipt: None,
            debit_batch: None,
            debit_receipt_reference: None,
            material: None,
            claim_transaction: None,
            claim_id: None,
            claim_available_at: None,
            payout_transaction: None,
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

    /// Re-obtains the exact disclosure for a prepared protocol debit.
    /// Returns `None` until the debit reaches the prepared phase and after it
    /// has already been signed, so fresh step-up evidence is bound only at the
    /// signing boundary.
    /// # Errors
    /// Refuses unavailable or inconsistent prepared debit evidence.
    pub fn prepared_debit_disclosure_digest(
        &self,
        scope: &PrincipalScope<'_>,
        agent_contract: &AgentClient,
        agent: &mut dyn AgentBoundary,
        registry: &ModuleRegistry,
    ) -> Result<Option<[u8; 32]>, WithdrawalJourneyError> {
        let id = JourneyId::new(self.record.debit_journey_id.clone())
            .map_err(|_| WithdrawalJourneyError::Corrupt("invalid debit journey id"))?;
        let Some(engine) = JourneyEngine::load(scope, &id)? else {
            return Ok(None);
        };
        let status = engine.status()?;
        if status.phases().get(status.current_leg()) != Some(&super::JourneyPhase::Prepared) {
            return Ok(None);
        }
        engine
            .prepared_disclosure_digest(agent_contract, agent, registry)
            .map(Some)
            .map_err(WithdrawalJourneyError::from)
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
        if boundary.protocol_version() != self.record.layerx_protocol_version {
            return Err(WithdrawalJourneyError::InvalidPlan);
        }
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
                    runtime.bind_debit(
                        &super::MovementExecutionIdentity {
                            principal: scope.principal().clone(),
                            tenant: scope.tenant().clone(),
                            account: expectation.account,
                            wallet: expectation.recipient,
                            plan_id: self.record.idempotency_key,
                        },
                        &committed,
                    )?;
                    self.record.withdrawal_id = committed.expectation().activity_id;
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
                if let Some(material) = runtime.withdrawal_material(&expectation)? {
                    let claim =
                        boundary.construct_claim(self.committed_debit()?, material.clone())?;
                    if claim.debit().receipt_reference() != self.debit_receipt_reference()? {
                        return Err(WithdrawalJourneyError::EvidenceConflict);
                    }
                    self.record.material = Some(StoredMaterial::from_public(&material)?);
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
                if let Some(transaction) = runtime.lookup(self.record.claim_action_key)? {
                    self.record.claim_transaction = Some(transaction.bytes());
                    self.transition(scope, Phase::ClaimConfirming, now)?;
                    return self.status();
                }
                let request = self.claim_request(scope, boundary)?;
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
                    let submitted =
                        boundary.accept_submission(self.restored_claim(boundary)?, &report)?;
                    self.record.claim_id = Some(submitted.claim_id());
                    self.record.claim_available_at = Some(submitted.available_at());
                    self.transition(scope, Phase::ClaimQueued, now)?;
                }
            }
            Phase::ClaimQueued => {
                let submitted = self.submitted_claim(boundary)?;
                match boundary.progress(&submitted)? {
                    ClaimProgress::WaitingForChallengeWindow { .. } => {}
                    ClaimProgress::ReadyToFinalise { .. } => {
                        self.transition(scope, Phase::ReadyToFinalise, now)?;
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
                        let cancellation = boundary.verify_cancellation(&submitted)?;
                        if cancellation.disposition != disposition
                            || cancellation.debit_receipt_reference
                                != self.debit_receipt_reference()?
                        {
                            return Err(WithdrawalJourneyError::EvidenceConflict);
                        }
                        self.record.cancellation =
                            Some(StoredCancellation::from_public(&cancellation));
                        self.transition(scope, Phase::Cancelled, now)?;
                    }
                }
            }
            Phase::ReadyToFinalise => {
                self.transition(scope, Phase::PayoutSubmitting, now)?;
            }
            Phase::PayoutSubmitting => {
                let request = self.payout_request(scope, boundary)?;
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
            withdrawal_id: self.record.debit_activity_id,
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
            CheckpointId::new(self.record.request_anchor),
            u64::try_from(self.record.fee_limit)
                .map_err(|_| WithdrawalJourneyError::InvalidPlan)?,
            self.owner()?,
            self.withdrawals_account()?,
            EvmAddress::new(self.record.payout_address),
            AssetId::new(self.record.asset),
            Amount::from_u128(self.record.amount),
            IdempotencyKey::new(self.record.debit_action_key),
        )?;
        let leg = JourneyLeg::new(
            Intent::v2(IntentKind::BridgeWithdrawRequest(intent)),
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
            super::engine::JourneyKind::Withdraw,
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
            withdrawal_id: activity_id,
            account: layerx_paxeer_client::account_address_for_protocol(
                &self.owner()?,
                self.record.layerx_protocol_version,
            )
            .map_err(|_| WithdrawalJourneyError::InvalidPlan)?,
            withdrawals_account: layerx_paxeer_client::account_address_for_protocol(
                &self.withdrawals_account()?,
                self.record.layerx_protocol_version,
            )
            .map_err(|_| WithdrawalJourneyError::InvalidPlan)?,
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
        let material = self
            .record
            .material
            .as_ref()
            .ok_or(WithdrawalJourneyError::Corrupt(
                "withdrawal material absent",
            ))?
            .public()?;
        Ok(boundary.construct_claim(self.committed_debit()?, material)?)
    }

    /// The same claim, rebuilt for a withdrawal already on the custody ledger.
    /// Its nullifier is reserved, consumed or cancelled by then, so the
    /// admission gates of a new claim cannot apply; the custody state is proved
    /// instead by the restore, progress, payout and cancellation checks.
    fn restored_claim(
        &self,
        boundary: &WithdrawalBoundary,
    ) -> Result<layerx_paxeer_client::WithdrawalClaim, WithdrawalJourneyError> {
        let material = self
            .record
            .material
            .as_ref()
            .ok_or(WithdrawalJourneyError::Corrupt(
                "withdrawal material absent",
            ))?
            .public()?;
        Ok(boundary.restore_claim(self.committed_debit()?, material)?)
    }

    fn submitted_claim(
        &self,
        boundary: &WithdrawalBoundary,
    ) -> Result<SubmittedWithdrawalClaim, WithdrawalJourneyError> {
        let transaction = self.claim_transaction()?;
        let report =
            final_report(boundary, transaction)?.ok_or(WithdrawalJourneyError::EvidencePending)?;
        let submitted = boundary.restore_submission(self.restored_claim(boundary)?, &report)?;
        if self.record.claim_id != Some(submitted.claim_id())
            || self.record.claim_available_at != Some(submitted.available_at())
        {
            return Err(WithdrawalJourneyError::EvidenceConflict);
        }
        Ok(submitted)
    }

    fn claim_request(
        &self,
        scope: &PrincipalScope<'_>,
        boundary: &WithdrawalBoundary,
    ) -> Result<WithdrawalTransactionRequest, WithdrawalJourneyError> {
        let claim = self.claim(boundary)?;
        Ok(WithdrawalTransactionRequest {
            signed_transaction: None,
            identity: super::MovementExecutionIdentity {
                principal: scope.principal().clone(),
                tenant: scope.tenant().clone(),
                account: layerx_paxeer_client::account_address_for_protocol(
                    &self.owner()?,
                    self.record.layerx_protocol_version,
                )
                .map_err(|_| WithdrawalJourneyError::InvalidPlan)?,
                wallet: EvmAddress::new(self.record.payout_address),
                plan_id: self.record.idempotency_key,
            },
            action_key: self.record.claim_action_key,
            action: PaxeerAction::QueueClaim,
            target: claim.contract(),
            calldata: claim.calldata().to_vec(),
        })
    }

    fn payout_request(
        &self,
        scope: &PrincipalScope<'_>,
        boundary: &WithdrawalBoundary,
    ) -> Result<WithdrawalTransactionRequest, WithdrawalJourneyError> {
        let submitted = self.submitted_claim(boundary)?;
        Ok(WithdrawalTransactionRequest {
            signed_transaction: None,
            identity: super::MovementExecutionIdentity {
                principal: scope.principal().clone(),
                tenant: scope.tenant().clone(),
                account: layerx_paxeer_client::account_address_for_protocol(
                    &self.owner()?,
                    self.record.layerx_protocol_version,
                )
                .map_err(|_| WithdrawalJourneyError::InvalidPlan)?,
                wallet: EvmAddress::new(self.record.payout_address),
                plan_id: self.record.idempotency_key,
            },
            action_key: self.record.payout_action_key,
            action: PaxeerAction::FinalisePayout,
            target: boundary.custody_precompile(),
            calldata: submitted.finalise_calldata(),
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
            observed_head: value.observed_head,
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
            observed_head: self.observed_head,
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

fn validate_plan(plan: &WithdrawalPlan) -> Result<(), WithdrawalJourneyError> {
    if !matches!(plan.layerx_protocol_version, 2 | 3)
        || plan.idempotency_key == [0; 32]
        || plan.request_anchor.bytes() == [0; 32]
        || plan.agent.fee_limit > u128::from(u64::MAX)
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

fn invalid_debit_state(record: &Record) -> bool {
    matches!(record.phase, Phase::Processing)
        && (record.debit_receipt.is_some()
            || record.debit_activity_id.is_some()
            || record.debit_batch.is_some()
            || record.debit_receipt_reference.is_some())
        || !matches!(record.phase, Phase::Processing)
            && (record.debit_receipt.is_none()
                || record.debit_batch.is_none()
                || record.debit_activity_id.is_none()
                || record.debit_receipt_reference.is_none())
}

fn validate_record(record: &Record) -> Result<(), WithdrawalJourneyError> {
    if record.version != RECORD_VERSION
        || !matches!(record.layerx_protocol_version, 2 | 3)
        || JourneyId::new(record.journey_id.clone()).is_err()
        || record.idempotency_key == [0; 32]
        || record.plan_digest == [0; 32]
        || record.debit_plan_key == [0; 32]
        || record.debit_action_key == [0; 32]
        || record.claim_action_key == [0; 32]
        || record.payout_action_key == [0; 32]
        || record.network_id == 0
        || record.request_anchor == [0; 32]
        || record.fee_limit > u128::from(u64::MAX)
        || record.withdrawal_id != record.debit_activity_id.unwrap_or([0; 32])
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
        || invalid_debit_state(record)
        || matches!(
            record.phase,
            Phase::ClaimReady
                | Phase::ClaimSubmitting
                | Phase::ClaimStillChecking
                | Phase::ClaimConfirming
                | Phase::ClaimQueued
                | Phase::ReadyToFinalise
                | Phase::PayoutSubmitting
                | Phase::PayoutStillChecking
                | Phase::PayoutConfirming
                | Phase::Paid
                | Phase::Cancelled
        ) && record.material.is_none()
        || matches!(
            record.phase,
            Phase::ClaimConfirming
                | Phase::ClaimQueued
                | Phase::ReadyToFinalise
                | Phase::PayoutSubmitting
                | Phase::PayoutStillChecking
                | Phase::PayoutConfirming
                | Phase::Paid
                | Phase::Cancelled
        ) && record.claim_transaction.is_none()
        || matches!(
            record.phase,
            Phase::ClaimQueued
                | Phase::ReadyToFinalise
                | Phase::PayoutSubmitting
                | Phase::PayoutStillChecking
                | Phase::PayoutConfirming
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
    digest.update(plan.layerx_protocol_version.to_be_bytes());
    digest.update(plan.request_anchor.bytes());
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

#[cfg(test)]
mod material_persistence_tests {
    use super::*;

    const RECEIPT: &[u8] =
        include_bytes!("../../../../../tests/fixtures/asset/bound-native-withdrawal/receipt");
    const PROOF: &[u8] =
        include_bytes!("../../../../../tests/fixtures/asset/bound-native-withdrawal/receipt.proof");
    const HEADER: &[u8] =
        include_bytes!("../../../../../tests/fixtures/asset/bound-native-withdrawal/header");
    const HEADER_SIGNATURE: &[u8] = include_bytes!(
        "../../../../../tests/fixtures/asset/bound-native-withdrawal/header.signature"
    );

    fn fixture_material() -> Result<WithdrawalMaterial, Box<dyn std::error::Error>> {
        Ok(WithdrawalMaterial {
            receipt: RECEIPT.to_vec(),
            proof: PROOF.to_vec(),
            header: HEADER.to_vec(),
            header_signature: HEADER_SIGNATURE.try_into()?,
        }
        .validated()
        .map_err(|error| format!("real withdrawal material: {error:?}"))?)
    }

    #[test]
    fn stored_material_round_trips_the_real_node_withdrawal_evidence(
    ) -> Result<(), Box<dyn std::error::Error>> {
        let material = fixture_material()?;
        let stored = StoredMaterial::from_public(&material)?;
        let bytes = serde_json::to_vec(&stored)?;
        let restored: StoredMaterial = serde_json::from_slice(&bytes)?;
        let restored = restored.public()?;
        assert_eq!(restored, material);
        assert_eq!(
            restored.finalise_calldata(),
            material.finalise_calldata(),
            "restored material must finalise exactly the submitted claim"
        );
        assert_ne!(restored.request_calldata(), restored.finalise_calldata());
        Ok(())
    }

    #[test]
    fn stored_material_refuses_evidence_that_was_altered_at_rest(
    ) -> Result<(), Box<dyn std::error::Error>> {
        let mut stored = StoredMaterial::from_public(&fixture_material()?)?;
        let last = stored
            .wire
            .len()
            .checked_sub(1)
            .ok_or("encoded material is empty")?;
        stored.wire[last] ^= 0x01;
        let altered = stored.public()?;
        assert_ne!(&altered.header_signature[..], HEADER_SIGNATURE);
        stored.wire.truncate(last);
        assert!(matches!(
            stored.public(),
            Err(WithdrawalJourneyError::Corrupt(_))
        ));
        Ok(())
    }
}
