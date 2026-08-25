//! Receipt-gated, crash-resumable Paxeer deposit journey.

use std::fmt::{Display, Formatter};

use layerx_agent_api::identity::{AgentDid, AuthorityRef, ContractError};
use layerx_paxeer_client::{
    account_address, ChainSignal, CustodyFault, DepositFailure, DepositProof, FinalityReport,
    FinalityStage, ProofFault, TransactionHash,
};
use layerx_sdk::Client as AgentClient;
use layerx_types::account::AccountId;
use layerx_types::amount::Amount;
use layerx_types::ids::AssetId;
use layerx_types::intent::{EvmAddress, NetworkId};
use layerx_types::payload::ModuleRegistry;
use serde::{Deserialize, Serialize};
use sha2::{Digest as _, Sha256};

use super::{
    AgentBoundary, AgentBoundaryError, JourneyEngine, JourneyError, JourneyLeg, JourneyPlan,
    JourneyState, ReceiptMaterial,
};
use crate::binding::{BindingError, BindingJourney, BindingState};
use crate::custody::{CustodySigner, KeyId, Operation};
use crate::notify::JourneyId;
use crate::store::{PrincipalScope, RowKey, StoreError, Table};
use crate::trace::TraceId;

const RECORD_VERSION: u8 = 3;
const RECORD_PREFIX: &str = "deposit-journey-";
const NOTIFICATION_PREFIX: &str = "deposit-notification-";
const WALLET_ACTION_DOMAIN: &[u8] = b"layerx-human-deposit-wallet/v1\0";
const PLAN_DIGEST_DOMAIN: &[u8] = b"layerx-human-deposit-plan/v2\0";
const CREDIT_PLAN_DOMAIN: &[u8] = b"layerx-human-deposit-credit-plan/v1\0";

/// Agent preparation facts for the one bridge credit leg.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DepositAgentPlan {
    pub actor: AgentDid,
    pub authority: AuthorityRef,
    pub account_sequence: u64,
    pub not_before: u64,
    pub not_after: u64,
    pub fee_limit: u128,
    pub custody_key: KeyId,
}

/// Immutable request for a deposit. The active binding is rechecked before
/// this plan is persisted and the beneficiary is derived from `recipient`.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DepositPlan {
    pub journey_id: JourneyId,
    pub idempotency_key: [u8; 32],
    pub wallet: EvmAddress,
    /// Existing wallet-binding scope; this is not the Paxeer RPC chain id.
    pub network: NetworkId,
    /// Full Paxeer EVM chain identity used by RPC and wallet custody.
    pub paxeer_chain_id: u64,
    pub layerx_network: NetworkId,
    pub layerx_protocol_version: u16,
    pub vault: EvmAddress,
    pub asset: AssetId,
    pub amount: Amount,
    pub recipient: AccountId,
    pub reserve: AccountId,
    pub currency: String,
    pub agent: DepositAgentPlan,
}

/// Exact wallet request. A production wallet adapter must deduplicate on
/// `action_key`; retrying it after an acknowledgement-gap crash must return
/// the original transaction rather than open a second signing ceremony.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct WalletCustodyRequest {
    pub action_key: [u8; 32],
    pub wallet: EvmAddress,
    pub chain_id: u64,
    pub vault: EvmAddress,
    pub asset: AssetId,
    pub beneficiary: [u8; 32],
    pub amount: Amount,
}

/// Result of the one user-controlled custody transaction.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum WalletCustodyOutcome {
    Submitted(TransactionHash),
    Rejected,
    Failed,
}

/// A stable external-boundary failure. Unavailable is retryable; corrupt
/// responses are never converted into economic success.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DepositBoundaryError {
    Unavailable,
    ContractViolation,
}

/// Real Paxeer/wallet operations consumed by the durable state machine.
pub trait DepositRuntime {
    /// Opens or resolves the custody signing request under its stable key.
    ///
    /// # Errors
    ///
    /// Returns an unavailable or contract-violation boundary result.
    fn submit_custody(
        &mut self,
        request: &WalletCustodyRequest,
    ) -> Result<WalletCustodyOutcome, DepositBoundaryError>;

    /// Polls the real Paxeer transaction and finality endpoints.
    ///
    /// # Errors
    ///
    /// Returns an unavailable or contract-violation boundary result.
    fn poll_finality(
        &mut self,
        transaction: TransactionHash,
    ) -> Result<FinalityReport, DepositBoundaryError>;

    /// Obtains authentic Paxeer-published root material and reconstructs the
    /// verifier-owned custody proof. Repeating this call is read-only and must
    /// preserve the same canonical deposit nullifier.
    ///
    /// # Errors
    ///
    /// Returns the exact typed custody, finality, checkpoint, or proof failure.
    fn obtain_proof(
        &mut self,
        transaction: TransactionHash,
    ) -> Result<DepositProof, DepositFailure>;
}

/// Agent boundary extension that reads the independently authorized receipt
/// material needed for deposit-specific amount/account verification.
pub trait DepositAgentBoundary: AgentBoundary {
    /// Reads the exact receipt plus independent core batch authority.
    ///
    /// # Errors
    ///
    /// Returns a typed agent-boundary failure without synthesizing evidence.
    fn credit_receipt(
        &mut self,
        action_key: [u8; 32],
        activity_id: [u8; 32],
    ) -> Result<ReceiptMaterial, AgentBoundaryError>;
}

/// Honest expectation attached to a finality stall while the journey keeps
/// running server-side.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct FinalityDelay {
    pub stalled_polls: u64,
    pub threshold: u64,
    pub stalled_for_seconds: u64,
    pub delayed_after_seconds: u64,
}

/// Terminal typed failure states exposed to clients.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DepositFailureKind {
    BindingUnavailable,
    WalletRejected,
    CustodyFailed,
    ReorgDisplaced { requeued: bool },
    ProofUnavailable,
    CreditRefused,
    LegacyProofSchema,
}

/// User-facing deposit timeline. Protocol machinery is represented as the
/// familiar wallet, confirming, crediting, and done stages.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum DepositStage {
    WaitingForWallet,
    ConfirmingPaxeer {
        transaction: TransactionHash,
        confirmations: u64,
        required: u64,
    },
    CreditingLayerX,
    Done,
    Failed(DepositFailureKind),
}

/// Explicit replay identity retained for current and legacy deposit history.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", content = "value", rename_all = "kebab-case")]
pub enum DepositProofIdentity {
    CanonicalNullifier([u8; 32]),
    LegacyProofCommitment([u8; 32]),
}

/// Joined custody/proof/credit evidence retained for Activity.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct DepositActivity {
    pub custody_transaction: [u8; 32],
    pub proof_identity: DepositProofIdentity,
    pub credit_activity_id: [u8; 32],
    pub credit_receipt_digest: [u8; 32],
}

/// Durable terminal notification outbox entry.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct DepositNotification {
    pub journey_id: String,
    pub completed: bool,
    pub failure: Option<String>,
    pub deep_link: String,
    pub created_at: u64,
}

/// Receipt-grounded public status. `balance_delta` is deliberately absent:
/// Human never projects a pending or locally inferred `LayerX` balance.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DepositStatus {
    journey_id: JourneyId,
    stage: DepositStage,
    in_flight_amount: Option<u128>,
    delay: Option<FinalityDelay>,
    activity: Option<DepositActivity>,
}

impl DepositStatus {
    #[must_use]
    pub const fn journey_id(&self) -> &JourneyId {
        &self.journey_id
    }

    #[must_use]
    pub const fn stage(&self) -> &DepositStage {
        &self.stage
    }

    /// Amount visible only in the in-flight section, never as spendable balance.
    #[must_use]
    pub const fn in_flight_amount(&self) -> Option<u128> {
        self.in_flight_amount
    }

    #[must_use]
    pub const fn delay(&self) -> Option<FinalityDelay> {
        self.delay
    }

    #[must_use]
    pub const fn activity(&self) -> Option<&DepositActivity> {
        self.activity.as_ref()
    }
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
enum Phase {
    Ready,
    WalletOpening,
    Confirming,
    Proving,
    Crediting,
    Done,
    Failed,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
enum StoredFailure {
    BindingUnavailable,
    WalletRejected,
    CustodyFailed,
    ReorgDisplacedRequeued,
    ReorgDisplacedDropped,
    ProofUnavailable,
    CreditRefused,
    LegacyProofSchema,
}

impl StoredFailure {
    const fn public(self) -> DepositFailureKind {
        match self {
            Self::BindingUnavailable => DepositFailureKind::BindingUnavailable,
            Self::WalletRejected => DepositFailureKind::WalletRejected,
            Self::CustodyFailed => DepositFailureKind::CustodyFailed,
            Self::ReorgDisplacedRequeued => DepositFailureKind::ReorgDisplaced { requeued: true },
            Self::ReorgDisplacedDropped => DepositFailureKind::ReorgDisplaced { requeued: false },
            Self::ProofUnavailable => DepositFailureKind::ProofUnavailable,
            Self::CreditRefused => DepositFailureKind::CreditRefused,
            Self::LegacyProofSchema => DepositFailureKind::LegacyProofSchema,
        }
    }

    const fn label(self) -> &'static str {
        match self {
            Self::BindingUnavailable => "binding-unavailable",
            Self::WalletRejected => "wallet-rejected",
            Self::CustodyFailed => "custody-failed",
            Self::ReorgDisplacedRequeued => "reorg-displaced-requeued",
            Self::ReorgDisplacedDropped => "reorg-displaced",
            Self::ProofUnavailable => "proof-unavailable",
            Self::CreditRefused => "credit-refused",
            Self::LegacyProofSchema => "legacy-proof-schema",
        }
    }
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
struct StoredDelay {
    stalled_polls: u64,
    threshold: u64,
    stalled_for_seconds: u64,
    delayed_after_seconds: u64,
}

impl StoredDelay {
    const fn public(self) -> FinalityDelay {
        FinalityDelay {
            stalled_polls: self.stalled_polls,
            threshold: self.threshold,
            stalled_for_seconds: self.stalled_for_seconds,
            delayed_after_seconds: self.delayed_after_seconds,
        }
    }
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
enum RecordSchema {
    Current,
    LegacyV1,
    LegacyV2,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
struct Record {
    version: u8,
    schema: RecordSchema,
    journey_id: String,
    idempotency_key: [u8; 32],
    plan_digest: [u8; 32],
    wallet: [u8; 20],
    binding_network_id: u32,
    paxeer_chain_id: Option<u64>,
    layerx_network_id: Option<u32>,
    layerx_protocol_version: Option<u16>,
    binding_receipt_digest: [u8; 32],
    vault: [u8; 20],
    asset: [u8; 32],
    amount: u128,
    recipient: String,
    reserve: String,
    currency: String,
    actor: String,
    authority: String,
    account_sequence: u64,
    not_before: u64,
    not_after: u64,
    fee_limit: u128,
    custody_key: String,
    wallet_action_key: [u8; 32],
    credit_plan_key: [u8; 32],
    credit_journey_id: String,
    phase: Phase,
    transaction: Option<[u8; 32]>,
    confirmations: u64,
    required: u64,
    delay: Option<StoredDelay>,
    deposit_nullifier: Option<[u8; 32]>,
    legacy_proof_commitment: Option<[u8; 32]>,
    legacy_phase: Option<Phase>,
    activity: Option<DepositActivity>,
    failure: Option<StoredFailure>,
    started_at: u64,
    updated_at: u64,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(untagged)]
enum LegacyDepositActivity {
    V1(LegacyDepositActivityV1),
    V2(LegacyDepositActivityV2),
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
struct LegacyDepositActivityV1 {
    custody_transaction: [u8; 32],
    proof_commitment: [u8; 32],
    credit_activity_id: [u8; 32],
    credit_receipt_digest: [u8; 32],
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
struct LegacyDepositActivityV2 {
    custody_transaction: [u8; 32],
    deposit_nullifier: [u8; 32],
    credit_activity_id: [u8; 32],
    credit_receipt_digest: [u8; 32],
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
struct LegacyRecord {
    version: u8,
    journey_id: String,
    idempotency_key: [u8; 32],
    plan_digest: [u8; 32],
    wallet: [u8; 20],
    network_id: u32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    layerx_network_id: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    layerx_protocol_version: Option<u16>,
    binding_receipt_digest: [u8; 32],
    vault: [u8; 20],
    asset: [u8; 32],
    amount: u128,
    recipient: String,
    reserve: String,
    currency: String,
    actor: String,
    authority: String,
    account_sequence: u64,
    not_before: u64,
    not_after: u64,
    fee_limit: u128,
    custody_key: String,
    wallet_action_key: [u8; 32],
    credit_plan_key: [u8; 32],
    credit_journey_id: String,
    phase: Phase,
    transaction: Option<[u8; 32]>,
    confirmations: u64,
    required: u64,
    delay: Option<StoredDelay>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    proof_commitment: Option<[u8; 32]>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    deposit_nullifier: Option<[u8; 32]>,
    activity: Option<LegacyDepositActivity>,
    failure: Option<StoredFailure>,
    started_at: u64,
    updated_at: u64,
}

/// Durable deposit state machine.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DepositJourney {
    record: Record,
}

impl DepositJourney {
    /// Rechecks the real receipt-backed wallet binding and persists the full
    /// immutable request before any wallet effect can occur.
    ///
    /// # Errors
    ///
    /// Returns typed binding, idempotency, validation, and store failures.
    pub fn start(
        scope: &mut PrincipalScope<'_>,
        binding: &BindingJourney,
        plan: &DepositPlan,
        now: u64,
    ) -> Result<Self, DepositJourneyError> {
        validate_plan(plan)?;
        let row = record_row(plan.idempotency_key)?;
        let digest = plan_digest(plan);
        if let Some(existing) = scope.get(Table::Journeys, &row) {
            let record = decode(existing.bytes())?;
            if record.journey_id != plan.journey_id.as_str()
                || (record.schema == RecordSchema::Current && record.plan_digest != digest)
            {
                return Err(DepositJourneyError::IdempotencyConflict);
            }
            let journey = Self { record };
            if journey.record.schema != RecordSchema::Current {
                journey.persist(scope)?;
            }
            return Ok(journey);
        }
        let active = match binding.state(scope)? {
            BindingState::Active(active) | BindingState::Rebinding { active, .. } => active,
            BindingState::Unbound | BindingState::Binding { .. } => {
                return Err(DepositJourneyError::BindingUnavailable)
            }
        };
        if active.address() != plan.wallet.bytes() || active.network_id() != plan.network.value() {
            return Err(DepositJourneyError::BindingUnavailable);
        }
        let wallet_action_key = derive_key(WALLET_ACTION_DOMAIN, &plan.idempotency_key);
        let credit_plan_key = derive_key(CREDIT_PLAN_DOMAIN, &plan.idempotency_key);
        let credit_journey_id = credit_journey_id(credit_plan_key)?;
        let record = Record {
            version: RECORD_VERSION,
            schema: RecordSchema::Current,
            journey_id: plan.journey_id.as_str().to_owned(),
            idempotency_key: plan.idempotency_key,
            plan_digest: digest,
            wallet: plan.wallet.bytes(),
            binding_network_id: plan.network.value(),
            paxeer_chain_id: Some(plan.paxeer_chain_id),
            layerx_network_id: Some(plan.layerx_network.value()),
            layerx_protocol_version: Some(plan.layerx_protocol_version),
            binding_receipt_digest: active.receipt_digest(),
            vault: plan.vault.bytes(),
            asset: plan.asset.bytes(),
            amount: plan.amount.value(),
            recipient: plan.recipient.canonical().to_owned(),
            reserve: plan.reserve.canonical().to_owned(),
            currency: plan.currency.clone(),
            actor: plan.agent.actor.as_str().to_owned(),
            authority: plan.agent.authority.as_str().to_owned(),
            account_sequence: plan.agent.account_sequence,
            not_before: plan.agent.not_before,
            not_after: plan.agent.not_after,
            fee_limit: plan.agent.fee_limit,
            custody_key: plan.agent.custody_key.as_str().to_owned(),
            wallet_action_key,
            credit_plan_key,
            credit_journey_id,
            phase: Phase::Ready,
            transaction: None,
            confirmations: 0,
            required: 0,
            delay: None,
            deposit_nullifier: None,
            legacy_proof_commitment: None,
            legacy_phase: None,
            activity: None,
            failure: None,
            started_at: now,
            updated_at: now,
        };
        let journey = Self { record };
        journey.persist(scope)?;
        Ok(journey)
    }

    /// Loads one deposit by its public journey identifier.
    ///
    /// # Errors
    ///
    /// Refuses corrupt or duplicate records.
    pub fn load(
        scope: &PrincipalScope<'_>,
        journey_id: &JourneyId,
    ) -> Result<Option<Self>, DepositJourneyError> {
        let mut found = None;
        for key in scope.keys(Table::Journeys) {
            if !key.as_str().starts_with(RECORD_PREFIX) {
                continue;
            }
            let row = scope
                .get(Table::Journeys, &key)
                .ok_or(DepositJourneyError::Corrupt("deposit disappeared"))?;
            let record = decode(row.bytes())?;
            if record.journey_id == journey_id.as_str() {
                if found.is_some() {
                    return Err(DepositJourneyError::Corrupt("duplicate deposit journey"));
                }
                found = Some(Self { record });
            }
        }
        Ok(found)
    }

    /// Advances at most one durable stage. Every side effect is preceded by a
    /// persisted state and retried only under its original idempotency key.
    ///
    /// # Errors
    ///
    /// Returns transient boundary and durable invariant errors without
    /// manufacturing a success state.
    #[allow(clippy::too_many_arguments, clippy::too_many_lines)]
    pub async fn advance<R: DepositRuntime, A: DepositAgentBoundary>(
        &mut self,
        scope: &mut PrincipalScope<'_>,
        runtime: &mut R,
        agent_contract: &AgentClient,
        agent: &mut A,
        custody: &CustodySigner,
        registry: &ModuleRegistry,
        trace: &TraceId,
        now: u64,
    ) -> Result<DepositStatus, DepositJourneyError> {
        if now < self.record.updated_at {
            return Err(DepositJourneyError::TimeRegressed);
        }
        if self.record.schema != RecordSchema::Current {
            self.persist(scope)?;
        }
        match self.record.phase {
            Phase::Ready => self.transition(scope, Phase::WalletOpening, now)?,
            Phase::WalletOpening => {
                let request = self.wallet_request();
                match runtime.submit_custody(&request)? {
                    WalletCustodyOutcome::Submitted(transaction) => {
                        if transaction.bytes() == [0; 32] {
                            return Err(DepositJourneyError::Boundary(
                                DepositBoundaryError::ContractViolation,
                            ));
                        }
                        self.record.transaction = Some(transaction.bytes());
                        self.transition(scope, Phase::Confirming, now)?;
                    }
                    WalletCustodyOutcome::Rejected => {
                        self.fail(scope, StoredFailure::WalletRejected, now)?;
                    }
                    WalletCustodyOutcome::Failed => {
                        self.fail(scope, StoredFailure::CustodyFailed, now)?;
                    }
                }
            }
            Phase::Confirming => {
                let transaction = self.transaction()?;
                let report = runtime.poll_finality(transaction)?;
                if report.transaction() != transaction {
                    return Err(DepositJourneyError::Boundary(
                        DepositBoundaryError::ContractViolation,
                    ));
                }
                self.record.confirmations = report.progress().confirmed;
                self.record.required = report.progress().required;
                self.record.delay = delay(&report.signal());
                match report.stage() {
                    FinalityStage::Final { .. } => {
                        self.transition(scope, Phase::Proving, now)?;
                    }
                    FinalityStage::Displaced { requeued, .. } => {
                        self.fail(
                            scope,
                            if requeued {
                                StoredFailure::ReorgDisplacedRequeued
                            } else {
                                StoredFailure::ReorgDisplacedDropped
                            },
                            now,
                        )?;
                    }
                    FinalityStage::Announced
                    | FinalityStage::Missing { .. }
                    | FinalityStage::Pooled { .. }
                    | FinalityStage::Confirming { .. } => self.persist_at(scope, now)?,
                }
            }
            Phase::Proving => {
                let proof = match runtime.obtain_proof(self.transaction()?) {
                    Ok(proof) => proof,
                    Err(DepositFailure::ProofUnavailable(ProofFault::NotFinal { .. })) => {
                        self.transition(scope, Phase::Confirming, now)?;
                        return self.status();
                    }
                    Err(DepositFailure::CustodyFailed(CustodyFault::Displaced {
                        requeued,
                        ..
                    })) => {
                        self.fail(
                            scope,
                            if requeued {
                                StoredFailure::ReorgDisplacedRequeued
                            } else {
                                StoredFailure::ReorgDisplacedDropped
                            },
                            now,
                        )?;
                        return self.status();
                    }
                    Err(DepositFailure::CustodyFailed(_)) => {
                        self.fail(scope, StoredFailure::CustodyFailed, now)?;
                        return self.status();
                    }
                    Err(DepositFailure::ProofUnavailable(_)) => {
                        self.fail(scope, StoredFailure::ProofUnavailable, now)?;
                        return self.status();
                    }
                    Err(DepositFailure::CreditRefused(_)) => {
                        self.fail(scope, StoredFailure::CreditRefused, now)?;
                        return self.status();
                    }
                };
                self.validate_proof(&proof)?;
                self.record.deposit_nullifier = Some(proof.nullifier());
                let inner = match self.credit_plan(&proof) {
                    Ok(plan) => plan,
                    Err(DepositJourneyError::Deposit(DepositFailure::CreditRefused(_))) => {
                        self.fail(scope, StoredFailure::CreditRefused, now)?;
                        return self.status();
                    }
                    Err(error) => return Err(error),
                };
                let _ = JourneyEngine::start(scope, &inner, registry, now)?;
                self.transition(scope, Phase::Crediting, now)?;
            }
            Phase::Crediting => {
                let inner_id = JourneyId::new(self.record.credit_journey_id.clone())
                    .map_err(|_| DepositJourneyError::Corrupt("invalid credit journey id"))?;
                let mut inner = JourneyEngine::load(scope, &inner_id)?
                    .ok_or(DepositJourneyError::Corrupt("credit journey missing"))?;
                let state = match inner
                    .advance(scope, agent_contract, agent, custody, registry, trace, now)
                    .await
                {
                    Ok(status) => status.state(),
                    Err(JourneyError::Agent(AgentBoundaryError::Unavailable)) => {
                        return Err(DepositJourneyError::Agent(AgentBoundaryError::Unavailable));
                    }
                    Err(
                        error @ (JourneyError::Store(_)
                        | JourneyError::Custody(_)
                        | JourneyError::TimeRegressed),
                    ) => {
                        return Err(DepositJourneyError::Journey(error));
                    }
                    Err(_) => {
                        self.fail(scope, StoredFailure::CreditRefused, now)?;
                        return self.status();
                    }
                };
                if state == JourneyState::Refused {
                    self.fail(scope, StoredFailure::CreditRefused, now)?;
                } else if state == JourneyState::Done {
                    let evidence = inner
                        .verified_leg_evidence(0)?
                        .ok_or(DepositJourneyError::Corrupt("credit receipt missing"))?;
                    let material = match agent
                        .credit_receipt(evidence.action_key, evidence.activity_id)
                    {
                        Ok(material) => material,
                        Err(AgentBoundaryError::Unavailable) => {
                            return Err(DepositJourneyError::Agent(
                                AgentBoundaryError::Unavailable,
                            ));
                        }
                        Err(AgentBoundaryError::Refused | AgentBoundaryError::CorruptResponse) => {
                            self.fail(scope, StoredFailure::CreditRefused, now)?;
                            return self.status();
                        }
                    };
                    if material.canonical_bytes != evidence.canonical_receipt {
                        return Err(DepositJourneyError::Corrupt(
                            "agent receipt changed after verification",
                        ));
                    }
                    let proof = runtime
                        .obtain_proof(self.transaction()?)
                        .map_err(DepositJourneyError::Deposit)?;
                    self.validate_proof(&proof)?;
                    let reserve = self.reserve()?;
                    let recipient = self.recipient()?;
                    if proof
                        .accept_credit(
                            &evidence.canonical_receipt,
                            &material.authorised_batch,
                            evidence.activity_id,
                            &reserve,
                            &recipient,
                        )
                        .is_err()
                    {
                        self.fail(scope, StoredFailure::CreditRefused, now)?;
                        return self.status();
                    }
                    self.record.activity = Some(DepositActivity {
                        custody_transaction: self.transaction()?.bytes(),
                        proof_identity: DepositProofIdentity::CanonicalNullifier(
                            proof.nullifier(),
                        ),
                        credit_activity_id: evidence.activity_id,
                        credit_receipt_digest: evidence.receipt_digest,
                    });
                    self.transition(scope, Phase::Done, now)?;
                    self.write_notification(scope, now)?;
                }
            }
            Phase::Done | Phase::Failed => self.write_notification(scope, now)?,
        }
        self.status()
    }

    /// Returns the current stage without inferring a core balance.
    ///
    /// # Errors
    ///
    /// Refuses corrupt identifiers and phase records.
    pub fn status(&self) -> Result<DepositStatus, DepositJourneyError> {
        let journey_id = JourneyId::new(self.record.journey_id.clone())
            .map_err(|_| DepositJourneyError::Corrupt("invalid deposit journey id"))?;
        let transaction = self.record.transaction.map(TransactionHash::new);
        let stage = match self.record.phase {
            Phase::Ready | Phase::WalletOpening => DepositStage::WaitingForWallet,
            Phase::Confirming | Phase::Proving => DepositStage::ConfirmingPaxeer {
                transaction: transaction.ok_or(DepositJourneyError::Corrupt(
                    "confirming without transaction",
                ))?,
                confirmations: self.record.confirmations,
                required: self.record.required,
            },
            Phase::Crediting => DepositStage::CreditingLayerX,
            Phase::Done => DepositStage::Done,
            Phase::Failed => DepositStage::Failed(
                self.record
                    .failure
                    .ok_or(DepositJourneyError::Corrupt("failed without reason"))?
                    .public(),
            ),
        };
        Ok(DepositStatus {
            journey_id,
            stage,
            in_flight_amount: if matches!(self.record.phase, Phase::Done | Phase::Failed) {
                None
            } else {
                Some(self.record.amount)
            },
            delay: self.record.delay.map(StoredDelay::public),
            activity: self.record.activity.clone(),
        })
    }

    /// Reads the terminal notification queued for delivery, if any.
    ///
    /// # Errors
    ///
    /// Refuses malformed durable notification data.
    pub fn notification(
        scope: &PrincipalScope<'_>,
        journey_id: &JourneyId,
    ) -> Result<Option<DepositNotification>, DepositJourneyError> {
        let key = notification_row(journey_id)?;
        scope
            .get(Table::Notifications, &key)
            .map_or(Ok(None), |row| {
                serde_json::from_slice(row.bytes())
                    .map(Some)
                    .map_err(|_| DepositJourneyError::Corrupt("invalid deposit notification"))
            })
    }

    fn wallet_request(&self) -> WalletCustodyRequest {
        let chain_id = self
            .record
            .paxeer_chain_id
            .unwrap_or_else(|| unreachable!("active current deposit has a Paxeer chain"));
        WalletCustodyRequest {
            action_key: self.record.wallet_action_key,
            wallet: EvmAddress::new(self.record.wallet),
            chain_id,
            vault: EvmAddress::new(self.record.vault),
            asset: AssetId::new(self.record.asset),
            beneficiary: account_address(&self.recipient_unchecked()),
            amount: Amount::from_u128(self.record.amount),
        }
    }

    fn credit_plan(&self, proof: &DepositProof) -> Result<JourneyPlan, DepositJourneyError> {
        let recipient = self.recipient()?;
        let reserve = self.reserve()?;
        let intent = proof
            .credit_intent(&reserve, &recipient)
            .map_err(DepositJourneyError::Deposit)?;
        let leg = JourneyLeg::new(
            intent,
            proof.idempotency_key().bytes(),
            AgentDid::new(self.record.actor.clone())?,
            AuthorityRef::new(self.record.authority.clone())?,
            self.record.account_sequence,
            self.record.not_before,
            self.record.not_after,
            self.record.fee_limit,
        )?;
        JourneyPlan::new(
            JourneyId::new(self.record.credit_journey_id.clone())
                .map_err(|_| DepositJourneyError::Corrupt("invalid credit journey id"))?,
            self.record.credit_plan_key,
            KeyId::new(self.record.custody_key.clone())
                .map_err(|_| DepositJourneyError::Corrupt("invalid custody key"))?,
            Operation::ProtocolMutation,
            vec![leg],
        )
        .map_err(Into::into)
    }

    fn validate_proof(&self, proof: &DepositProof) -> Result<(), DepositJourneyError> {
        let recipient = self.recipient()?;
        if proof.transaction() != self.transaction()?
            || proof.vault().bytes() != self.record.vault
            || Some(proof.chain_id()) != self.record.paxeer_chain_id
            || Some(proof.network_id()) != self.record.layerx_network_id
            || Some(proof.protocol_version()) != self.record.layerx_protocol_version
            || proof.custody().payer.bytes() != self.record.wallet
            || proof.custody().asset.bytes() != self.record.asset
            || proof.custody().amount.value() != self.record.amount
            || proof.custody().beneficiary != account_address(&recipient)
            || self
                .record
                .deposit_nullifier
                .is_some_and(|stored| stored != proof.nullifier())
        {
            return Err(DepositJourneyError::ProofMismatch);
        }
        Ok(())
    }

    fn transaction(&self) -> Result<TransactionHash, DepositJourneyError> {
        self.record
            .transaction
            .map(TransactionHash::new)
            .ok_or(DepositJourneyError::Corrupt("deposit has no transaction"))
    }

    fn recipient_unchecked(&self) -> AccountId {
        AccountId::parse(&self.record.recipient)
            .unwrap_or_else(|_| unreachable!("validated record account"))
    }

    fn recipient(&self) -> Result<AccountId, DepositJourneyError> {
        AccountId::parse(&self.record.recipient)
            .map_err(|_| DepositJourneyError::Corrupt("invalid deposit recipient"))
    }

    fn reserve(&self) -> Result<AccountId, DepositJourneyError> {
        AccountId::parse(&self.record.reserve)
            .map_err(|_| DepositJourneyError::Corrupt("invalid deposit reserve"))
    }

    fn transition(
        &mut self,
        scope: &mut PrincipalScope<'_>,
        phase: Phase,
        now: u64,
    ) -> Result<(), DepositJourneyError> {
        self.record.phase = phase;
        self.persist_at(scope, now)
    }

    fn fail(
        &mut self,
        scope: &mut PrincipalScope<'_>,
        failure: StoredFailure,
        now: u64,
    ) -> Result<(), DepositJourneyError> {
        self.record.failure = Some(failure);
        self.record.phase = Phase::Failed;
        self.persist_at(scope, now)?;
        self.write_notification(scope, now)
    }

    fn persist_at(
        &mut self,
        scope: &mut PrincipalScope<'_>,
        now: u64,
    ) -> Result<(), DepositJourneyError> {
        if now < self.record.updated_at {
            return Err(DepositJourneyError::TimeRegressed);
        }
        self.record.updated_at = now;
        self.persist(scope)
    }

    fn persist(&self, scope: &mut PrincipalScope<'_>) -> Result<(), DepositJourneyError> {
        validate_record(&self.record)?;
        let bytes = serde_json::to_vec(&self.record)
            .map_err(|_| DepositJourneyError::Corrupt("deposit cannot be encoded"))?;
        scope.put(
            Table::Journeys,
            record_row(self.record.idempotency_key)?,
            self.record.updated_at,
            bytes,
        )?;
        Ok(())
    }

    fn write_notification(
        &self,
        scope: &mut PrincipalScope<'_>,
        now: u64,
    ) -> Result<(), DepositJourneyError> {
        if !matches!(self.record.phase, Phase::Done | Phase::Failed) {
            return Ok(());
        }
        let journey_id = JourneyId::new(self.record.journey_id.clone())
            .map_err(|_| DepositJourneyError::Corrupt("invalid deposit journey id"))?;
        let notification = DepositNotification {
            journey_id: self.record.journey_id.clone(),
            completed: self.record.phase == Phase::Done,
            failure: self.record.failure.map(|value| value.label().to_owned()),
            deep_link: format!("/app/journeys/{}", self.record.journey_id),
            created_at: now,
        };
        let bytes = serde_json::to_vec(&notification)
            .map_err(|_| DepositJourneyError::Corrupt("notification cannot be encoded"))?;
        let key = notification_row(&journey_id)?;
        if let Some(existing) = scope.get(Table::Notifications, &key) {
            let existing_notification: DepositNotification =
                serde_json::from_slice(existing.bytes())
                    .map_err(|_| DepositJourneyError::Corrupt("invalid deposit notification"))?;
            if existing_notification.completed != notification.completed
                || existing_notification.failure != notification.failure
                || existing_notification.deep_link != notification.deep_link
            {
                return Err(DepositJourneyError::EvidenceConflict);
            }
            return Ok(());
        }
        scope.put(Table::Notifications, key, now, bytes)?;
        Ok(())
    }
}

fn validate_plan(plan: &DepositPlan) -> Result<(), DepositJourneyError> {
    if plan.idempotency_key == [0; 32]
        || plan.amount.value() == 0
        || plan.paxeer_chain_id == 0
        || plan.layerx_protocol_version == 0
        || plan.currency.is_empty()
        || plan.currency.len() > 12
        || !plan
            .currency
            .bytes()
            .all(|byte| byte.is_ascii_uppercase() || byte.is_ascii_digit())
        || plan.agent.not_after < plan.agent.not_before
    {
        return Err(DepositJourneyError::InvalidPlan);
    }
    Ok(())
}

fn validate_record(record: &Record) -> Result<(), DepositJourneyError> {
    let schema_invalid = match record.schema {
        RecordSchema::Current => {
            record.paxeer_chain_id.is_none_or(|value| value == 0)
                || record.layerx_network_id.is_none_or(|value| value == 0)
                || record
                    .layerx_protocol_version
                    .is_none_or(|value| value == 0)
                || record.legacy_proof_commitment.is_some()
                || record.legacy_phase.is_some()
                || record.activity.as_ref().is_some_and(|activity| {
                    !matches!(
                        activity.proof_identity,
                        DepositProofIdentity::CanonicalNullifier(_)
                    )
                })
        }
        RecordSchema::LegacyV1 => {
            record.paxeer_chain_id.is_some()
                || record.layerx_network_id.is_some()
                || record.layerx_protocol_version.is_some()
                || record.deposit_nullifier.is_some()
                || !matches!(record.phase, Phase::Done | Phase::Failed)
                || record
                    .legacy_phase
                    .is_some_and(|phase| matches!(phase, Phase::Done | Phase::Failed))
                || (record.failure == Some(StoredFailure::LegacyProofSchema))
                    != record.legacy_phase.is_some()
                || record.activity.as_ref().is_some_and(|activity| {
                    !matches!(
                        activity.proof_identity,
                        DepositProofIdentity::LegacyProofCommitment(_)
                    )
                })
        }
        RecordSchema::LegacyV2 => {
            record.paxeer_chain_id.is_some()
                || record.layerx_network_id.is_none_or(|value| value == 0)
                || record
                    .layerx_protocol_version
                    .is_none_or(|value| value == 0)
                || record.legacy_proof_commitment.is_some()
                || !matches!(record.phase, Phase::Done | Phase::Failed)
                || record
                    .legacy_phase
                    .is_some_and(|phase| matches!(phase, Phase::Done | Phase::Failed))
                || (record.failure == Some(StoredFailure::LegacyProofSchema))
                    != record.legacy_phase.is_some()
                || record.activity.as_ref().is_some_and(|activity| {
                    !matches!(
                        activity.proof_identity,
                        DepositProofIdentity::CanonicalNullifier(_)
                    )
                })
        }
    };
    let terminal_identity_invalid = if record.phase == Phase::Done {
        match (record.schema, record.activity.as_ref()) {
            (
                RecordSchema::Current | RecordSchema::LegacyV2,
                Some(DepositActivity {
                    proof_identity: DepositProofIdentity::CanonicalNullifier(value),
                    ..
                }),
            ) => record.deposit_nullifier != Some(*value),
            (
                RecordSchema::LegacyV1,
                Some(DepositActivity {
                    proof_identity: DepositProofIdentity::LegacyProofCommitment(value),
                    ..
                }),
            ) => record.legacy_proof_commitment != Some(*value),
            _ => true,
        }
    } else {
        false
    };
    if record.version != RECORD_VERSION
        || JourneyId::new(record.journey_id.clone()).is_err()
        || record.idempotency_key == [0; 32]
        || record.plan_digest == [0; 32]
        || record.wallet_action_key == [0; 32]
        || record.credit_plan_key == [0; 32]
        || record.wallet == [0; 20]
        || record.vault == [0; 20]
        || record.binding_network_id == 0
        || record.amount == 0
        || schema_invalid
        || terminal_identity_invalid
        || record.updated_at < record.started_at
        || (matches!(
            record.phase,
            Phase::Confirming | Phase::Proving | Phase::Crediting | Phase::Done
        ) && record.transaction.is_none())
        || (record.phase == Phase::Done && record.activity.is_none())
        || (record.phase == Phase::Failed) != record.failure.is_some()
        || record.phase != Phase::Done && record.activity.is_some()
        || AccountId::parse(&record.recipient).is_err()
        || AccountId::parse(&record.reserve).is_err()
        || AgentDid::new(record.actor.clone()).is_err()
        || AuthorityRef::new(record.authority.clone()).is_err()
        || KeyId::new(record.custody_key.clone()).is_err()
        || JourneyId::new(record.credit_journey_id.clone()).is_err()
    {
        return Err(DepositJourneyError::Corrupt(
            "deposit invariants are invalid",
        ));
    }
    Ok(())
}

fn decode(bytes: &[u8]) -> Result<Record, DepositJourneyError> {
    #[derive(Deserialize)]
    struct Version {
        version: u8,
    }

    let version: Version = serde_json::from_slice(bytes)
        .map_err(|_| DepositJourneyError::Corrupt("invalid deposit encoding"))?;
    let record = match version.version {
        RECORD_VERSION => serde_json::from_slice(bytes)
            .map_err(|_| DepositJourneyError::Corrupt("invalid current deposit encoding"))?,
        1 | 2 => {
            let legacy: LegacyRecord = serde_json::from_slice(bytes)
                .map_err(|_| DepositJourneyError::Corrupt("invalid legacy deposit encoding"))?;
            migrate_legacy(legacy)?
        }
        _ => {
            return Err(DepositJourneyError::Corrupt(
                "unsupported deposit record version",
            ));
        }
    };
    validate_record(&record)?;
    Ok(record)
}

fn migrate_legacy(legacy: LegacyRecord) -> Result<Record, DepositJourneyError> {
    if !matches!(legacy.version, 1 | 2)
        || JourneyId::new(legacy.journey_id.clone()).is_err()
        || legacy.idempotency_key == [0; 32]
        || legacy.plan_digest == [0; 32]
        || legacy.wallet == [0; 20]
        || legacy.network_id == 0
        || legacy.vault == [0; 20]
        || legacy.amount == 0
        || legacy.wallet_action_key == [0; 32]
        || legacy.credit_plan_key == [0; 32]
        || legacy.updated_at < legacy.started_at
        || AccountId::parse(&legacy.recipient).is_err()
        || AccountId::parse(&legacy.reserve).is_err()
        || AgentDid::new(legacy.actor.clone()).is_err()
        || AuthorityRef::new(legacy.authority.clone()).is_err()
        || KeyId::new(legacy.custody_key.clone()).is_err()
        || JourneyId::new(legacy.credit_journey_id.clone()).is_err()
        || (matches!(
            legacy.phase,
            Phase::Confirming | Phase::Proving | Phase::Crediting | Phase::Done
        ) && legacy.transaction.is_none())
        || (legacy.phase == Phase::Done && legacy.activity.is_none())
        || (legacy.phase == Phase::Failed) != legacy.failure.is_some()
        || legacy.phase != Phase::Done && legacy.activity.is_some()
    {
        return Err(DepositJourneyError::Corrupt(
            "legacy deposit invariants are invalid",
        ));
    }
    if legacy.version == 1
        && (legacy.layerx_network_id.is_some()
            || legacy.layerx_protocol_version.is_some()
            || legacy.deposit_nullifier.is_some())
    {
        return Err(DepositJourneyError::Corrupt(
            "legacy v1 deposit contains later-schema fields",
        ));
    }
    if legacy.version == 2
        && (legacy.layerx_network_id.is_none_or(|value| value == 0)
            || legacy
                .layerx_protocol_version
                .is_none_or(|value| value == 0)
            || legacy.proof_commitment.is_some())
    {
        return Err(DepositJourneyError::Corrupt(
            "legacy v2 deposit fields are invalid",
        ));
    }
    if legacy.phase == Phase::Done {
        let consistent = match (legacy.version, legacy.activity.as_ref()) {
            (
                1,
                Some(LegacyDepositActivity::V1(LegacyDepositActivityV1 {
                    proof_commitment,
                    ..
                })),
            ) => legacy.proof_commitment == Some(*proof_commitment),
            (
                2,
                Some(LegacyDepositActivity::V2(LegacyDepositActivityV2 {
                    deposit_nullifier,
                    ..
                })),
            ) => legacy.deposit_nullifier == Some(*deposit_nullifier),
            _ => false,
        };
        if !consistent {
            return Err(DepositJourneyError::Corrupt(
                "legacy terminal deposit evidence is inconsistent",
            ));
        }
    }

    let schema = if legacy.version == 1 {
        RecordSchema::LegacyV1
    } else {
        RecordSchema::LegacyV2
    };
    let resumable = !matches!(legacy.phase, Phase::Done | Phase::Failed);
    let original_phase = legacy.phase;
    let activity = legacy
        .activity
        .map(|activity| migrate_legacy_activity(schema, activity))
        .transpose()?;
    Ok(Record {
        version: RECORD_VERSION,
        schema,
        journey_id: legacy.journey_id,
        idempotency_key: legacy.idempotency_key,
        plan_digest: legacy.plan_digest,
        wallet: legacy.wallet,
        binding_network_id: legacy.network_id,
        paxeer_chain_id: None,
        layerx_network_id: legacy.layerx_network_id,
        layerx_protocol_version: legacy.layerx_protocol_version,
        binding_receipt_digest: legacy.binding_receipt_digest,
        vault: legacy.vault,
        asset: legacy.asset,
        amount: legacy.amount,
        recipient: legacy.recipient,
        reserve: legacy.reserve,
        currency: legacy.currency,
        actor: legacy.actor,
        authority: legacy.authority,
        account_sequence: legacy.account_sequence,
        not_before: legacy.not_before,
        not_after: legacy.not_after,
        fee_limit: legacy.fee_limit,
        custody_key: legacy.custody_key,
        wallet_action_key: legacy.wallet_action_key,
        credit_plan_key: legacy.credit_plan_key,
        credit_journey_id: legacy.credit_journey_id,
        phase: if resumable { Phase::Failed } else { original_phase },
        transaction: legacy.transaction,
        confirmations: legacy.confirmations,
        required: legacy.required,
        delay: legacy.delay,
        deposit_nullifier: legacy.deposit_nullifier,
        legacy_proof_commitment: legacy.proof_commitment,
        legacy_phase: resumable.then_some(original_phase),
        activity: if resumable { None } else { activity },
        failure: if resumable {
            Some(StoredFailure::LegacyProofSchema)
        } else {
            legacy.failure
        },
        started_at: legacy.started_at,
        updated_at: legacy.updated_at,
    })
}

fn migrate_legacy_activity(
    schema: RecordSchema,
    activity: LegacyDepositActivity,
) -> Result<DepositActivity, DepositJourneyError> {
    match (schema, activity) {
        (
            RecordSchema::LegacyV1,
            LegacyDepositActivity::V1(LegacyDepositActivityV1 {
                custody_transaction,
                proof_commitment,
                credit_activity_id,
                credit_receipt_digest,
            }),
        ) => Ok(DepositActivity {
            custody_transaction,
            proof_identity: DepositProofIdentity::LegacyProofCommitment(proof_commitment),
            credit_activity_id,
            credit_receipt_digest,
        }),
        (
            RecordSchema::LegacyV2,
            LegacyDepositActivity::V2(LegacyDepositActivityV2 {
                custody_transaction,
                deposit_nullifier,
                credit_activity_id,
                credit_receipt_digest,
            }),
        ) => Ok(DepositActivity {
            custody_transaction,
            proof_identity: DepositProofIdentity::CanonicalNullifier(deposit_nullifier),
            credit_activity_id,
            credit_receipt_digest,
        }),
        _ => Err(DepositJourneyError::Corrupt(
            "legacy deposit activity schema is inconsistent",
        )),
    }
}

fn record_row(key: [u8; 32]) -> Result<RowKey, StoreError> {
    RowKey::new(format!("{RECORD_PREFIX}{}", hex(&key)))
}

fn notification_row(journey_id: &JourneyId) -> Result<RowKey, StoreError> {
    RowKey::new(format!("{NOTIFICATION_PREFIX}{}", journey_id.as_str()))
}

fn derive_key(domain: &[u8], input: &[u8; 32]) -> [u8; 32] {
    let mut digest = Sha256::new();
    digest.update(domain);
    digest.update(input);
    digest.finalize().into()
}

fn credit_journey_id(key: [u8; 32]) -> Result<String, DepositJourneyError> {
    let value = format!("jrn_credit{}", &hex(&key)[..32]);
    JourneyId::new(value.clone())
        .map_err(|_| DepositJourneyError::Corrupt("invalid credit journey id"))?;
    Ok(value)
}

fn plan_digest(plan: &DepositPlan) -> [u8; 32] {
    let mut digest = Sha256::new();
    digest.update(PLAN_DIGEST_DOMAIN);
    hash_text(&mut digest, plan.journey_id.as_str());
    digest.update(plan.idempotency_key);
    digest.update(plan.wallet.bytes());
    digest.update(plan.network.value().to_be_bytes());
    digest.update(plan.paxeer_chain_id.to_be_bytes());
    digest.update(plan.layerx_network.value().to_be_bytes());
    digest.update(plan.layerx_protocol_version.to_be_bytes());
    digest.update(plan.vault.bytes());
    digest.update(plan.asset.bytes());
    digest.update(plan.amount.value().to_be_bytes());
    hash_text(&mut digest, plan.recipient.canonical());
    hash_text(&mut digest, plan.reserve.canonical());
    hash_text(&mut digest, &plan.currency);
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

fn delay(signal: &ChainSignal) -> Option<StoredDelay> {
    match signal {
        ChainSignal::Delayed {
            stalled_polls,
            threshold,
            stalled_for,
            delayed_after,
        } => Some(StoredDelay {
            stalled_polls: *stalled_polls,
            threshold: *threshold,
            stalled_for_seconds: stalled_for.as_secs(),
            delayed_after_seconds: delayed_after.as_secs(),
        }),
        ChainSignal::Progressing | ChainSignal::Unreachable { .. } => None,
    }
}

fn hex(bytes: &[u8]) -> String {
    let mut output = String::with_capacity(bytes.len().saturating_mul(2));
    for byte in bytes {
        use std::fmt::Write as _;
        let _ = write!(output, "{byte:02x}");
    }
    output
}

/// Typed deposit journey error. Transient errors do not alter the last durable
/// state; terminal business failures are represented by `DepositStage::Failed`.
#[derive(Debug)]
pub enum DepositJourneyError {
    Store(StoreError),
    Binding(BindingError),
    Contract(ContractError),
    Journey(JourneyError),
    Agent(AgentBoundaryError),
    Boundary(DepositBoundaryError),
    Deposit(DepositFailure),
    InvalidPlan,
    BindingUnavailable,
    IdempotencyConflict,
    TimeRegressed,
    ProofMismatch,
    EvidenceConflict,
    Corrupt(&'static str),
}

impl Display for DepositJourneyError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Store(error) => write!(formatter, "deposit store failure: {error}"),
            Self::Binding(error) => write!(formatter, "deposit binding failure: {error}"),
            Self::Contract(error) => write!(formatter, "deposit agent contract failure: {error:?}"),
            Self::Journey(error) => write!(formatter, "deposit credit journey failure: {error}"),
            Self::Agent(error) => write!(formatter, "deposit agent boundary failure: {error:?}"),
            Self::Boundary(error) => {
                write!(formatter, "deposit Paxeer boundary failure: {error:?}")
            }
            Self::Deposit(error) => {
                write!(formatter, "deposit proof or receipt failure: {error:?}")
            }
            Self::InvalidPlan => formatter.write_str("deposit plan is invalid"),
            Self::BindingUnavailable => {
                formatter.write_str("a matching active wallet binding is required")
            }
            Self::IdempotencyConflict => {
                formatter.write_str("deposit idempotency key owns another request")
            }
            Self::TimeRegressed => formatter.write_str("deposit journey time regressed"),
            Self::ProofMismatch => {
                formatter.write_str("custody proof differs from the bound deposit request")
            }
            Self::EvidenceConflict => formatter.write_str("deposit terminal evidence conflicts"),
            Self::Corrupt(reason) => write!(formatter, "corrupt deposit journey: {reason}"),
        }
    }
}

impl std::error::Error for DepositJourneyError {}

impl From<StoreError> for DepositJourneyError {
    fn from(value: StoreError) -> Self {
        Self::Store(value)
    }
}
impl From<BindingError> for DepositJourneyError {
    fn from(value: BindingError) -> Self {
        Self::Binding(value)
    }
}
impl From<ContractError> for DepositJourneyError {
    fn from(value: ContractError) -> Self {
        Self::Contract(value)
    }
}
impl From<JourneyError> for DepositJourneyError {
    fn from(value: JourneyError) -> Self {
        Self::Journey(value)
    }
}
impl From<AgentBoundaryError> for DepositJourneyError {
    fn from(value: AgentBoundaryError) -> Self {
        Self::Agent(value)
    }
}
impl From<DepositBoundaryError> for DepositJourneyError {
    fn from(value: DepositBoundaryError) -> Self {
        Self::Boundary(value)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn legacy_record(version: u8, phase: Phase) -> LegacyRecord {
        let transaction = matches!(
            phase,
            Phase::Confirming | Phase::Proving | Phase::Crediting | Phase::Done
        )
        .then_some([7; 32]);
        let done = phase == Phase::Done;
        let failed = phase == Phase::Failed;
        let activity = done.then_some(if version == 1 {
            LegacyDepositActivity::V1(LegacyDepositActivityV1 {
                custody_transaction: [7; 32],
                proof_commitment: [8; 32],
                credit_activity_id: [9; 32],
                credit_receipt_digest: [10; 32],
            })
        } else {
            LegacyDepositActivity::V2(LegacyDepositActivityV2 {
                custody_transaction: [7; 32],
                deposit_nullifier: [8; 32],
                credit_activity_id: [9; 32],
                credit_receipt_digest: [10; 32],
            })
        });
        LegacyRecord {
            version,
            journey_id: "jrn_depositcrash".to_owned(),
            idempotency_key: [1; 32],
            plan_digest: [2; 32],
            wallet: [3; 20],
            network_id: 17,
            layerx_network_id: (version == 2).then_some(17),
            layerx_protocol_version: (version == 2).then_some(1),
            binding_receipt_digest: [4; 32],
            vault: [5; 20],
            asset: [6; 32],
            amount: 25,
            recipient: "agent:did:layerx:deposit-recipient:main".to_owned(),
            reserve: "system:paxeer-reserve".to_owned(),
            currency: "LXP".to_owned(),
            actor: "did:layerx:deposit-recipient".to_owned(),
            authority: "owner:deposit-owner".to_owned(),
            account_sequence: 5,
            not_before: 995,
            not_after: 1_010,
            fee_limit: 7,
            custody_key: "human-primary".to_owned(),
            wallet_action_key: [11; 32],
            credit_plan_key: [12; 32],
            credit_journey_id: credit_journey_id([12; 32])
                .unwrap_or_else(|error| panic!("credit journey: {error}")),
            phase,
            transaction,
            confirmations: 1,
            required: 1,
            delay: None,
            proof_commitment: (version == 1 && done).then_some([8; 32]),
            deposit_nullifier: (version == 2 && done).then_some([8; 32]),
            activity,
            failure: failed.then_some(StoredFailure::WalletRejected),
            started_at: 100,
            updated_at: 101,
        }
    }

    fn decode_legacy(record: &LegacyRecord) -> DepositJourney {
        let encoded = serde_json::to_vec(record)
            .unwrap_or_else(|error| panic!("legacy encoding: {error}"));
        DepositJourney {
            record: decode(&encoded).unwrap_or_else(|error| panic!("legacy decode: {error}")),
        }
    }

    #[test]
    fn terminal_v1_history_preserves_legacy_proof_identity() {
        let journey = decode_legacy(&legacy_record(1, Phase::Done));
        let status = journey
            .status()
            .unwrap_or_else(|error| panic!("legacy status: {error}"));
        assert_eq!(status.stage(), &DepositStage::Done);
        assert!(matches!(
            status.activity().map(|activity| &activity.proof_identity),
            Some(DepositProofIdentity::LegacyProofCommitment(value)) if value == &[8; 32]
        ));
        assert_eq!(journey.record.schema, RecordSchema::LegacyV1);
        assert_eq!(journey.record.paxeer_chain_id, None);
    }

    #[test]
    fn terminal_v1_failure_preserves_its_original_reason() {
        let journey = decode_legacy(&legacy_record(1, Phase::Failed));
        let status = journey
            .status()
            .unwrap_or_else(|error| panic!("legacy status: {error}"));
        assert_eq!(
            status.stage(),
            &DepositStage::Failed(DepositFailureKind::WalletRejected)
        );
        assert_eq!(journey.record.legacy_phase, None);
    }

    #[test]
    fn resumable_v1_is_explicitly_failed_without_chain_or_proof_reinterpretation() {
        let journey = decode_legacy(&legacy_record(1, Phase::Proving));
        let status = journey
            .status()
            .unwrap_or_else(|error| panic!("legacy status: {error}"));
        assert_eq!(
            status.stage(),
            &DepositStage::Failed(DepositFailureKind::LegacyProofSchema)
        );
        assert_eq!(journey.record.legacy_phase, Some(Phase::Proving));
        assert_eq!(journey.record.paxeer_chain_id, None);
        assert_eq!(journey.record.deposit_nullifier, None);
        let migrated = serde_json::to_vec(&journey.record)
            .unwrap_or_else(|error| panic!("migrated encoding: {error}"));
        let restarted = DepositJourney {
            record: decode(&migrated)
                .unwrap_or_else(|error| panic!("migrated restart: {error}")),
        };
        assert_eq!(
            restarted
                .status()
                .unwrap_or_else(|error| panic!("migrated status: {error}"))
                .stage(),
            &DepositStage::Failed(DepositFailureKind::LegacyProofSchema)
        );
    }

    #[test]
    fn terminal_v2_nullifier_history_is_preserved_but_chain_is_not_inferred() {
        let journey = decode_legacy(&legacy_record(2, Phase::Done));
        let status = journey
            .status()
            .unwrap_or_else(|error| panic!("legacy status: {error}"));
        assert!(matches!(
            status.activity().map(|activity| &activity.proof_identity),
            Some(DepositProofIdentity::CanonicalNullifier(value)) if value == &[8; 32]
        ));
        assert_eq!(journey.record.schema, RecordSchema::LegacyV2);
        assert_eq!(journey.record.paxeer_chain_id, None);
    }

    #[test]
    fn inconsistent_legacy_terminal_identity_is_corrupt_not_reinterpreted() {
        let mut legacy = legacy_record(1, Phase::Done);
        legacy.proof_commitment = Some([12; 32]);
        let encoded = serde_json::to_vec(&legacy)
            .unwrap_or_else(|error| panic!("legacy encoding: {error}"));
        assert!(matches!(
            decode(&encoded),
            Err(DepositJourneyError::Corrupt(
                "legacy terminal deposit evidence is inconsistent"
            ))
        ));
    }

    #[test]
    fn current_paxeer_chain_identity_preserves_values_above_u32() {
        let mut record = migrate_legacy(legacy_record(1, Phase::Failed))
            .unwrap_or_else(|error| panic!("legacy migration: {error}"));
        record.schema = RecordSchema::Current;
        record.paxeer_chain_id = Some(4_294_967_312);
        record.layerx_network_id = Some(17);
        record.layerx_protocol_version = Some(1);
        record.legacy_proof_commitment = None;
        record.legacy_phase = None;
        let encoded = serde_json::to_vec(&record)
            .unwrap_or_else(|error| panic!("current encoding: {error}"));
        let current = DepositJourney {
            record: decode(&encoded).unwrap_or_else(|error| panic!("current decode: {error}")),
        };
        assert_eq!(current.record.binding_network_id, 17);
        assert_eq!(current.record.paxeer_chain_id, Some(4_294_967_312));
        assert_eq!(current.wallet_request().chain_id, 4_294_967_312);
    }
}
