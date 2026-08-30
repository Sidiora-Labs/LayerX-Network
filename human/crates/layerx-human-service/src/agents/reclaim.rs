//! Receipt-gated return of funds from managed agents.

use std::fmt::{Display, Formatter};

use layerx_agent_api::identity::{AgentDid, AuthorityRef, ContractError};
use layerx_intents::compile;
use layerx_proof::receipt::{verify, VerificationFailure};
use layerx_sdk::Client as AgentClient;
use layerx_types::account::AccountId;
use layerx_types::amount::Amount;
use layerx_types::ids::{AssetId, IdempotencyKey};
use layerx_types::intent::{BudgetId, Sequence};
use layerx_types::payload::ModuleRegistry;
use serde::{Deserialize, Serialize};
use sha2::{Digest as _, Sha256};

use crate::activity::{ActivityKind, Feed, FeedError, FundsDisposition};
use crate::custody::{CustodySigner, KeyId, Operation};
use crate::journeys::{
    AgentBoundary, AgentBoundaryError, BudgetRoute, Endpoint, JourneyEngine, JourneyError,
    JourneyLeg, JourneyPlan, JourneyState, Mechanism, MovementTerm, PayerGrantRoute,
    ReceiptMaterial, Relationship, RouteError, RouteRequest, RouteResolver, SendRoute,
};
use crate::notify::JourneyId;
use crate::store::{PrincipalScope, RowKey, StoreError, Table};
use crate::trace::TraceId;

const RECORD_VERSION: u8 = 1;
const RECORD_PREFIX: &str = "agent-reclaim-";
const PLAN_DOMAIN: &[u8] = b"layerx-human-agent-reclaim/v1\0";

/// The only three protocol-authorized ways to return value from an agent.
/// There is deliberately no raw key, sweep, withdrawal, or custody variant.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ReclaimMechanism {
    /// Return value from a protocol budget through its revocation-aware
    /// defunding operation.
    BudgetDefund {
        budget_account: AccountId,
        budget_id: BudgetId,
        revocation_sequence: Sequence,
    },
    /// Let the agent authorize an ordinary protocol transfer to its owner.
    AgentAuthorized(SendRoute),
    /// Let the owner receive under a previously registered payer grant.
    ReceiveUnderPayerGrant(PayerGrantRoute),
}

impl ReclaimMechanism {
    /// Returns the exact protocol mechanism selected for this return.
    #[must_use]
    pub const fn mechanism(&self) -> Mechanism {
        match self {
            Self::BudgetDefund { .. } => Mechanism::BudgetDefund,
            Self::AgentAuthorized(_) => Mechanism::Send,
            Self::ReceiveUnderPayerGrant(_) => Mechanism::ReceiveUnderPayerGrant,
        }
    }
}

/// Complete agent-contract context for one reclaim activity.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ReclaimAgentContext {
    pub label: String,
    pub actor: AgentDid,
    pub authority: AuthorityRef,
    pub custody_key: KeyId,
    pub account_sequence: u64,
    pub not_before: u64,
    pub not_after: u64,
    pub fee_limit: u128,
}

/// Immutable request to take money back from one agent.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ReclaimRequest {
    pub journey_id: JourneyId,
    pub idempotency_key: [u8; 32],
    pub owner: AccountId,
    pub agent_account: AccountId,
    pub asset: AssetId,
    pub amount: Amount,
    pub mechanism: ReclaimMechanism,
    pub agent: ReclaimAgentContext,
}

/// One independently receipt-verified result shown on the agent and retained
/// by the unified activity feed.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct ReclaimResult {
    activity_id: [u8; 32],
    receipt_digest: [u8; 32],
    asset: [u8; 32],
    amount: u128,
    fee_charged: u128,
}

impl ReclaimResult {
    #[must_use]
    pub const fn activity_id(&self) -> [u8; 32] {
        self.activity_id
    }

    #[must_use]
    pub const fn receipt_digest(&self) -> [u8; 32] {
        self.receipt_digest
    }

    #[must_use]
    pub const fn asset(&self) -> [u8; 32] {
        self.asset
    }

    #[must_use]
    pub const fn amount(&self) -> u128 {
        self.amount
    }

    #[must_use]
    pub const fn fee_charged(&self) -> u128 {
        self.fee_charged
    }
}

/// Product state for the return journey.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ReclaimStage {
    GettingReady,
    Sending,
    Processing,
    StillChecking,
    Done,
    Refused,
}

/// Receipt-gated status shown on the managed agent.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ReclaimStatus {
    journey_id: JourneyId,
    agent: String,
    stage: ReclaimStage,
    result: Option<ReclaimResult>,
}

impl ReclaimStatus {
    #[must_use]
    pub const fn journey_id(&self) -> &JourneyId {
        &self.journey_id
    }

    #[must_use]
    pub fn agent(&self) -> &str {
        &self.agent
    }

    #[must_use]
    pub const fn stage(&self) -> ReclaimStage {
        self.stage
    }

    #[must_use]
    pub const fn result(&self) -> Option<&ReclaimResult> {
        self.result.as_ref()
    }

    /// Stable internal-movement vocabulary used in APIs, logs, and copy.
    #[must_use]
    pub const fn movement_term() -> MovementTerm {
        MovementTerm::Return
    }

    /// Familiar product action used for this protocol return.
    #[must_use]
    pub const fn user_action() -> &'static str {
        MovementTerm::user_action()
    }
}

/// Agent boundary extension used only to re-read the exact material already
/// verified by the journey engine. It cannot submit another effect.
pub trait ReclaimAgentBoundary: AgentBoundary {
    /// Returns immutable receipt material under the original action identity.
    ///
    /// # Errors
    ///
    /// Returns a typed agent error without manufacturing receipt evidence.
    fn reclaim_receipt(
        &mut self,
        action_key: [u8; 32],
        expected_activity_id: [u8; 32],
    ) -> Result<ReceiptMaterial, AgentBoundaryError>;
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
struct Record {
    version: u8,
    journey_id: String,
    idempotency_key: [u8; 32],
    plan_digest: [u8; 32],
    agent: String,
    asset: [u8; 32],
    amount: u128,
    fee_limit: u128,
    mechanism: u8,
    result: Option<ReclaimResult>,
    activity_projected: bool,
    refused: bool,
    started_at: u64,
    updated_at: u64,
}

/// Durable reclaim orchestrator. Callers supply only one closed reclaim
/// mechanism; the signing operation and typed intent are selected internally.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Reclaim {
    record: Record,
    engine: JourneyEngine,
}

impl Reclaim {
    /// Resolves the requested return, persists its immutable owner record, and
    /// starts the generic receipt-gated engine under the same stable key.
    ///
    /// # Errors
    ///
    /// Refuses invalid labels, route widening, conflicting idempotency reuse,
    /// intent compilation, or durable-store failures.
    pub fn start(
        scope: &mut PrincipalScope<'_>,
        request: &ReclaimRequest,
        registry: &ModuleRegistry,
        now: u64,
    ) -> Result<Self, ReclaimError> {
        validate_request(request)?;
        let route = reclaim_route(request)?;
        let leg = route.legs().first().ok_or(ReclaimError::RouteWidened)?;
        if route.legs().len() != 1
            || leg.term() != MovementTerm::Return
            || leg.mechanism() != request.mechanism.mechanism()
            || leg.term().is_custody_boundary()
        {
            return Err(ReclaimError::RouteWidened);
        }
        let compiled = compile(leg.intent(), registry)?;
        let digest = request_digest(request, compiled.payload_hash());
        let row = record_row(request.idempotency_key)?;
        let record = if let Some(existing) = scope.get(Table::Journeys, &row) {
            let record = decode(existing.bytes())?;
            if record.plan_digest != digest || record.journey_id != request.journey_id.as_str() {
                return Err(ReclaimError::IdempotencyConflict);
            }
            record
        } else {
            let record = Record {
                version: RECORD_VERSION,
                journey_id: request.journey_id.as_str().to_owned(),
                idempotency_key: request.idempotency_key,
                plan_digest: digest,
                agent: request.agent.label.clone(),
                asset: request.asset.bytes(),
                amount: request.amount.value(),
                fee_limit: request.agent.fee_limit,
                mechanism: mechanism_code(&request.mechanism),
                result: None,
                activity_projected: false,
                refused: false,
                started_at: now,
                updated_at: now,
            };
            persist_record(scope, &record)?;
            record
        };
        let journey_leg = JourneyLeg::new(
            leg.intent().clone(),
            request.idempotency_key,
            request.agent.actor.clone(),
            request.agent.authority.clone(),
            request.agent.account_sequence,
            request.agent.not_before,
            request.agent.not_after,
            request.agent.fee_limit,
        )?;
        let plan = JourneyPlan::new(
            request.journey_id.clone(),
            crate::journeys::JourneyKind::AgentRetire,
            request.idempotency_key,
            request.agent.custody_key.clone(),
            Operation::ProtocolMutation,
            vec![journey_leg],
        )?;
        let engine = JourneyEngine::start(scope, &plan, registry, now)?;
        Ok(Self { record, engine })
    }

    /// Loads one reclaim with its independently validated engine state.
    ///
    /// # Errors
    ///
    /// Refuses corrupt, duplicate, or orphaned reclaim records.
    pub fn load(
        scope: &PrincipalScope<'_>,
        journey_id: &JourneyId,
    ) -> Result<Option<Self>, ReclaimError> {
        let mut found = None;
        for key in scope.keys(Table::Journeys) {
            if !key.as_str().starts_with(RECORD_PREFIX) {
                continue;
            }
            let row = scope
                .get(Table::Journeys, &key)
                .ok_or(ReclaimError::Corrupt("reclaim disappeared"))?;
            let record = decode(row.bytes())?;
            if record.journey_id == journey_id.as_str() {
                if found.is_some() {
                    return Err(ReclaimError::Corrupt("duplicate reclaim journey"));
                }
                let engine = JourneyEngine::load(scope, journey_id)?
                    .ok_or(ReclaimError::Corrupt("reclaim engine missing"))?;
                found = Some(Self { record, engine });
            }
        }
        Ok(found)
    }

    /// Advances at most one durable stage. Unknown outcomes stay on the
    /// engine's receipt-lookup-only path. Completion is withheld until the
    /// exact receipt is independently reverified and durably projected into
    /// Activity.
    ///
    /// # Errors
    ///
    /// Returns typed engine, receipt, feed, or store failures without
    /// resubmitting an unknown economic effect.
    #[allow(clippy::too_many_arguments)]
    pub async fn advance<A: ReclaimAgentBoundary>(
        &mut self,
        scope: &mut PrincipalScope<'_>,
        agent_contract: &AgentClient,
        agent: &mut A,
        custody: &CustodySigner,
        registry: &ModuleRegistry,
        trace: &TraceId,
        now: u64,
    ) -> Result<ReclaimStatus, ReclaimError> {
        if now < self.record.updated_at {
            return Err(ReclaimError::TimeRegressed);
        }
        let status = self
            .engine
            .advance(scope, agent_contract, agent, custody, registry, trace, now)
            .await?;
        match status.state() {
            JourneyState::Done => {
                if self.record.result.is_none() {
                    let evidence = self
                        .engine
                        .verified_leg_evidence(0)?
                        .ok_or(ReclaimError::Corrupt("verified reclaim receipt missing"))?;
                    let material = agent
                        .reclaim_receipt(evidence.action_key, evidence.activity_id)
                        .map_err(ReclaimError::Agent)?;
                    if material.canonical_bytes != evidence.canonical_receipt {
                        return Err(ReclaimError::ReceiptMismatch);
                    }
                    let verified = verify(&material.canonical_bytes, &material.authorised_batch)?;
                    let protocol = verified
                        .receipt()
                        .protocol()
                        .ok_or(ReclaimError::ReceiptMismatch)?;
                    if protocol.activity_id() != evidence.activity_id
                        || protocol.asset() != self.record.asset
                        || protocol.amount() != self.record.amount
                        || protocol.fee_charged() > self.record.fee_limit
                    {
                        return Err(ReclaimError::ReceiptMismatch);
                    }
                    self.record.result = Some(ReclaimResult {
                        activity_id: protocol.activity_id(),
                        receipt_digest: evidence.receipt_digest,
                        asset: protocol.asset(),
                        amount: protocol.amount(),
                        fee_charged: protocol.fee_charged(),
                    });
                    self.persist_at(scope, now)?;
                }
                self.project_activity(scope, None, now)?;
            }
            JourneyState::Refused => {
                self.record.refused = true;
                self.persist_at(scope, now)?;
                self.project_activity(scope, Some(FundsDisposition::NoMoneyLeft), now)?;
            }
            JourneyState::GettingReady
            | JourneyState::Sending
            | JourneyState::Processing
            | JourneyState::StillChecking => {}
        }
        self.status()
    }

    /// Returns the current status without exposing a completion before both
    /// receipt verification and Activity projection are durable.
    ///
    /// # Errors
    ///
    /// Refuses corrupt public identifiers or inconsistent terminal state.
    pub fn status(&self) -> Result<ReclaimStatus, ReclaimError> {
        let journey_id = JourneyId::new(self.record.journey_id.clone())
            .map_err(|_| ReclaimError::Corrupt("invalid reclaim journey id"))?;
        let engine = self.engine.status()?;
        let stage = if self.record.refused && self.record.activity_projected {
            ReclaimStage::Refused
        } else if self.record.result.is_some() && self.record.activity_projected {
            ReclaimStage::Done
        } else {
            match engine.state() {
                JourneyState::GettingReady => ReclaimStage::GettingReady,
                JourneyState::Sending => ReclaimStage::Sending,
                JourneyState::Processing | JourneyState::Done | JourneyState::Refused => {
                    ReclaimStage::Processing
                }
                JourneyState::StillChecking => ReclaimStage::StillChecking,
            }
        };
        Ok(ReclaimStatus {
            journey_id,
            agent: self.record.agent.clone(),
            stage,
            result: self.record.result.clone(),
        })
    }

    /// The only custody operation this orchestrator can request. Its public
    /// API never accepts an operation or raw agent key from a caller.
    #[must_use]
    pub const fn signing_operation() -> Operation {
        Operation::ProtocolMutation
    }

    fn project_activity(
        &mut self,
        scope: &mut PrincipalScope<'_>,
        refusal: Option<FundsDisposition>,
        now: u64,
    ) -> Result<(), ReclaimError> {
        if self.record.activity_projected {
            return Ok(());
        }
        let journey_id = JourneyId::new(self.record.journey_id.clone())
            .map_err(|_| ReclaimError::Corrupt("invalid reclaim journey id"))?;
        let events = JourneyEngine::stream_events(scope, &journey_id)?;
        let progress = events
            .last()
            .ok_or(ReclaimError::Corrupt("reclaim progress missing"))?;
        let status = self.engine.status()?;
        let _ = Feed::record_journey(
            scope,
            ActivityKind::Movement,
            &status,
            progress,
            Some(self.record.agent.clone()),
            refusal,
            now,
        )?;
        self.record.activity_projected = true;
        self.persist_at(scope, now)
    }

    fn persist_at(&mut self, scope: &mut PrincipalScope<'_>, now: u64) -> Result<(), ReclaimError> {
        if now < self.record.updated_at {
            return Err(ReclaimError::TimeRegressed);
        }
        self.record.updated_at = now;
        persist_record(scope, &self.record)
    }
}

fn validate_request(request: &ReclaimRequest) -> Result<(), ReclaimError> {
    if request.idempotency_key == [0; 32]
        || request.amount == Amount::ZERO
        || request.agent.label.is_empty()
        || request.agent.label.len() > 128
        || !request
            .agent
            .label
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b':' | b'.'))
        || request.agent.not_after < request.agent.not_before
    {
        return Err(ReclaimError::InvalidRequest);
    }
    let protocol_key = match &request.mechanism {
        ReclaimMechanism::BudgetDefund { .. } => request.idempotency_key,
        ReclaimMechanism::AgentAuthorized(route) => route.idempotency_key.bytes(),
        ReclaimMechanism::ReceiveUnderPayerGrant(route) => route.idempotency_key.bytes(),
    };
    if protocol_key != request.idempotency_key {
        return Err(ReclaimError::IdempotencyConflict);
    }
    Ok(())
}

fn reclaim_route(request: &ReclaimRequest) -> Result<crate::journeys::Route, ReclaimError> {
    let relationship = match &request.mechanism {
        ReclaimMechanism::BudgetDefund {
            budget_account: _,
            budget_id,
            revocation_sequence,
        } => Relationship::ManagedBudget(BudgetRoute {
            budget_id: *budget_id,
            idempotency_key: IdempotencyKey::new(request.idempotency_key),
            revocation_sequence: *revocation_sequence,
            create: None,
        }),
        ReclaimMechanism::AgentAuthorized(route) => Relationship::AgentAuthorized(*route),
        ReclaimMechanism::ReceiveUnderPayerGrant(route) => Relationship::PayerGrant(*route),
    };
    let source = match &request.mechanism {
        ReclaimMechanism::BudgetDefund { budget_account, .. } => {
            Endpoint::AgentBudget(budget_account.clone())
        }
        ReclaimMechanism::AgentAuthorized(_) | ReclaimMechanism::ReceiveUnderPayerGrant(_) => {
            Endpoint::Agent(request.agent_account.clone())
        }
    };
    RouteResolver::resolve(&RouteRequest {
        source,
        destination: Endpoint::Human(request.owner.clone()),
        relationship,
        asset: request.asset,
        amount: request.amount,
    })
    .map_err(Into::into)
}

fn request_digest(request: &ReclaimRequest, payload_hash: [u8; 32]) -> [u8; 32] {
    let mut digest = Sha256::new();
    digest.update(PLAN_DOMAIN);
    digest.update(request.idempotency_key);
    digest.update(payload_hash);
    digest.update(request.asset.bytes());
    digest.update(request.amount.value().to_be_bytes());
    hash_text(&mut digest, request.journey_id.as_str());
    hash_text(&mut digest, request.owner.canonical());
    hash_text(&mut digest, request.agent_account.canonical());
    hash_text(&mut digest, &request.agent.label);
    hash_text(&mut digest, request.agent.actor.as_str());
    hash_text(&mut digest, request.agent.authority.as_str());
    hash_text(&mut digest, request.agent.custody_key.as_str());
    digest.update(request.agent.account_sequence.to_be_bytes());
    digest.update(request.agent.not_before.to_be_bytes());
    digest.update(request.agent.not_after.to_be_bytes());
    digest.update(request.agent.fee_limit.to_be_bytes());
    digest.update([mechanism_code(&request.mechanism)]);
    digest.finalize().into()
}

fn hash_text(digest: &mut Sha256, value: &str) {
    digest.update(u32::try_from(value.len()).unwrap_or(u32::MAX).to_be_bytes());
    digest.update(value.as_bytes());
}

const fn mechanism_code(mechanism: &ReclaimMechanism) -> u8 {
    match mechanism {
        ReclaimMechanism::BudgetDefund { .. } => 1,
        ReclaimMechanism::AgentAuthorized(_) => 2,
        ReclaimMechanism::ReceiveUnderPayerGrant(_) => 3,
    }
}

fn record_row(idempotency_key: [u8; 32]) -> Result<RowKey, StoreError> {
    RowKey::new(format!("{RECORD_PREFIX}{}", hex(&idempotency_key)))
}

fn persist_record(scope: &mut PrincipalScope<'_>, record: &Record) -> Result<(), ReclaimError> {
    validate_record(record)?;
    let bytes = serde_json::to_vec(record)
        .map_err(|_| ReclaimError::Corrupt("reclaim cannot be encoded"))?;
    scope.put(
        Table::Journeys,
        record_row(record.idempotency_key)?,
        record.updated_at,
        bytes,
    )?;
    Ok(())
}

fn decode(bytes: &[u8]) -> Result<Record, ReclaimError> {
    let record: Record = serde_json::from_slice(bytes)
        .map_err(|_| ReclaimError::Corrupt("invalid reclaim encoding"))?;
    validate_record(&record)?;
    Ok(record)
}

fn validate_record(record: &Record) -> Result<(), ReclaimError> {
    if record.version != RECORD_VERSION
        || JourneyId::new(record.journey_id.clone()).is_err()
        || record.idempotency_key == [0; 32]
        || record.plan_digest == [0; 32]
        || record.agent.is_empty()
        || record.asset == [0; 32]
        || record.amount == 0
        || !matches!(record.mechanism, 1..=3)
        || record.updated_at < record.started_at
        || record.activity_projected && !(record.result.is_some() || record.refused)
        || record.result.is_some() && record.refused
        || record.result.as_ref().is_some_and(|result| {
            result.activity_id == [0; 32]
                || result.receipt_digest == [0; 32]
                || result.asset != record.asset
                || result.amount != record.amount
                || result.fee_charged > record.fee_limit
        })
    {
        return Err(ReclaimError::Corrupt("reclaim invariants are invalid"));
    }
    Ok(())
}

fn hex(bytes: &[u8]) -> String {
    let mut output = String::with_capacity(bytes.len().saturating_mul(2));
    for byte in bytes {
        use std::fmt::Write as _;
        let _ = write!(output, "{byte:02x}");
    }
    output
}

/// Typed reclaim failure. No error path exposes a raw key sweep or claims a
/// receipt-backed result before verification.
#[derive(Debug)]
pub enum ReclaimError {
    Store(StoreError),
    Contract(ContractError),
    Route(RouteError),
    Compile(layerx_intents::CompileError),
    Journey(JourneyError),
    Agent(AgentBoundaryError),
    Receipt(VerificationFailure),
    Feed(FeedError),
    InvalidRequest,
    IdempotencyConflict,
    RouteWidened,
    ReceiptMismatch,
    TimeRegressed,
    Corrupt(&'static str),
}

impl Display for ReclaimError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Store(error) => write!(formatter, "reclaim store failure: {error}"),
            Self::Contract(error) => write!(formatter, "reclaim contract failure: {error:?}"),
            Self::Route(error) => write!(formatter, "reclaim route failure: {error}"),
            Self::Compile(error) => write!(formatter, "reclaim compilation failure: {error:?}"),
            Self::Journey(error) => write!(formatter, "reclaim journey failure: {error}"),
            Self::Agent(error) => write!(formatter, "reclaim agent failure: {error:?}"),
            Self::Receipt(error) => write!(formatter, "reclaim receipt failure: {error:?}"),
            Self::Feed(error) => write!(formatter, "reclaim activity failure: {error}"),
            Self::InvalidRequest => formatter.write_str("reclaim request is invalid"),
            Self::IdempotencyConflict => formatter.write_str("reclaim idempotency key conflicts"),
            Self::RouteWidened => formatter.write_str("reclaim route is not exactly one return"),
            Self::ReceiptMismatch => {
                formatter.write_str("reclaim receipt differs from the request")
            }
            Self::TimeRegressed => formatter.write_str("reclaim time regressed"),
            Self::Corrupt(reason) => write!(formatter, "corrupt reclaim: {reason}"),
        }
    }
}

impl std::error::Error for ReclaimError {}

impl From<StoreError> for ReclaimError {
    fn from(value: StoreError) -> Self {
        Self::Store(value)
    }
}

impl From<ContractError> for ReclaimError {
    fn from(value: ContractError) -> Self {
        Self::Contract(value)
    }
}

impl From<RouteError> for ReclaimError {
    fn from(value: RouteError) -> Self {
        Self::Route(value)
    }
}

impl From<layerx_intents::CompileError> for ReclaimError {
    fn from(value: layerx_intents::CompileError) -> Self {
        Self::Compile(value)
    }
}

impl From<JourneyError> for ReclaimError {
    fn from(value: JourneyError) -> Self {
        Self::Journey(value)
    }
}

impl From<VerificationFailure> for ReclaimError {
    fn from(value: VerificationFailure) -> Self {
        Self::Receipt(value)
    }
}

impl From<FeedError> for ReclaimError {
    fn from(value: FeedError) -> Self {
        Self::Feed(value)
    }
}
