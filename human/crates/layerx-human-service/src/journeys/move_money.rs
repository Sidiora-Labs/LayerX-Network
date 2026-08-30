//! Durable internal movement journeys over deterministic routing and receipt-gated execution.

use std::collections::BTreeSet;
use std::fmt::{Display, Formatter};

use layerx_agent_api::identity::{AgentDid, AuthorityRef};
use layerx_intents::compile;
use layerx_proof::receipt::canonical_protocol_facts;
use layerx_sdk::Client as AgentClient;
use layerx_types::payload::ModuleRegistry;
use layerx_types::result::{KnownResult, ResultCode};
use serde::{Deserialize, Serialize};
use sha2::{Digest as _, Sha256};

use crate::custody::{CustodySigner, KeyId, Operation};
use crate::notify::JourneyId;
use crate::store::{PrincipalScope, RowKey, StoreError, Table};
use crate::trace::TraceId;

use super::{
    AgentBoundary, ChangeSurface, JourneyEngine, JourneyError, JourneyLeg, JourneyPhase,
    JourneyPlan, JourneyState, LimitRefusal, LimitSource, Mechanism, MovementTerm, Route,
    RouteError, RouteRequest, RouteResolver,
};

const RECORD_VERSION: u8 = 1;
const RECORD_PREFIX: &str = "move-";
const PLAN_DOMAIN: &[u8] = b"layerx-human-move-plan/v1";
const TEXT_LIMIT: usize = 256;
const IRREVERSIBILITY: &str =
    "Once committed, every completed step is final and cannot be cancelled.";

/// Per-leg agent-contract and signing context bound before commitment.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct MoveLegExecution {
    action_key: [u8; 32],
    actor: AgentDid,
    authority: AuthorityRef,
    account_sequence: u64,
    not_before: u64,
    not_after: u64,
    fee_ceiling: u128,
}

impl MoveLegExecution {
    /// Constructs complete execution context for one resolved leg.
    ///
    /// # Errors
    ///
    /// Refuses a zero action key or an inverted validity interval.
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        action_key: [u8; 32],
        actor: AgentDid,
        authority: AuthorityRef,
        account_sequence: u64,
        not_before: u64,
        not_after: u64,
        fee_ceiling: u128,
    ) -> Result<Self, MoveJourneyError> {
        if action_key == [0; 32]
            || not_after < not_before
            || actor.as_str().len() > 512
            || authority.as_str().len() > 512
            || actor.as_str().chars().any(char::is_control)
            || authority.as_str().chars().any(char::is_control)
        {
            return Err(MoveJourneyError::InvalidPlan);
        }
        Ok(Self {
            action_key,
            actor,
            authority,
            account_sequence,
            not_before,
            not_after,
            fee_ceiling,
        })
    }

    /// Returns the stable economic action key.
    #[must_use]
    pub const fn action_key(&self) -> [u8; 32] {
        self.action_key
    }

    /// Returns the maximum fee authorised for this leg.
    #[must_use]
    pub const fn fee_ceiling(&self) -> u128 {
        self.fee_ceiling
    }

    /// Returns the validated fields used by the canonical provider wire codec.
    #[must_use]
    pub fn to_wire_parts(&self) -> ([u8; 32], &AgentDid, &AuthorityRef, u64, u64, u64, u128) {
        (
            self.action_key,
            &self.actor,
            &self.authority,
            self.account_sequence,
            self.not_before,
            self.not_after,
            self.fee_ceiling,
        )
    }

    /// Reconstructs wire fields through the plan validator.
    #[allow(clippy::too_many_arguments)]
    pub fn from_wire_parts(
        action_key: [u8; 32],
        actor: AgentDid,
        authority: AuthorityRef,
        account_sequence: u64,
        not_before: u64,
        not_after: u64,
        fee_ceiling: u128,
    ) -> Result<Self, MoveJourneyError> {
        Self::new(
            action_key,
            actor,
            authority,
            account_sequence,
            not_before,
            not_after,
            fee_ceiling,
        )
    }
}

/// Plain review facts shown before any effect is committed.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct MoveQuote {
    amount: u128,
    asset_label: String,
    fee_estimate: u128,
    fee_ceiling: u128,
    arrival_expectation: String,
    irreversibility: String,
    plain_language: String,
}

impl MoveQuote {
    /// Returns the requested amount in exact base units.
    #[must_use]
    pub const fn amount(&self) -> u128 {
        self.amount
    }

    /// Returns the bounded human asset label.
    #[must_use]
    pub fn asset_label(&self) -> &str {
        &self.asset_label
    }

    /// Returns the pre-commit fee estimate.
    #[must_use]
    pub const fn fee_estimate(&self) -> u128 {
        self.fee_estimate
    }

    /// Returns the hard fee ceiling across all legs.
    #[must_use]
    pub const fn fee_ceiling(&self) -> u128 {
        self.fee_ceiling
    }

    /// Returns the configured arrival expectation.
    #[must_use]
    pub fn arrival_expectation(&self) -> &str {
        &self.arrival_expectation
    }

    /// Returns the plain commitment consequence.
    #[must_use]
    pub fn irreversibility(&self) -> &str {
        &self.irreversibility
    }

    /// Returns the complete protocol-free review sentence.
    #[must_use]
    pub fn plain_language(&self) -> &str {
        &self.plain_language
    }
}

/// Immutable resolved movement submitted by the Human service.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct MovePlan {
    journey_id: JourneyId,
    idempotency_key: [u8; 32],
    custody_key: KeyId,
    operation: Operation,
    request: RouteRequest,
    route: Route,
    executions: Vec<MoveLegExecution>,
    quote: MoveQuote,
}

impl MovePlan {
    /// Canonically encodes the complete immutable plan without debug or serde forms.
    #[must_use]
    pub fn canonical_encode(&self) -> Vec<u8> {
        let mut out = vec![1];
        wire_text(&mut out, self.journey_id.as_str());
        out.extend(self.idempotency_key);
        wire_text(&mut out, self.custody_key.as_str());
        wire_text(&mut out, self.operation.label());
        let route = self.request.canonical_encode();
        out.extend((route.len() as u32).to_be_bytes());
        out.extend(route);
        out.extend((self.executions.len() as u16).to_be_bytes());
        for execution in &self.executions {
            let (k, a, r, s, b, e, f) = execution.to_wire_parts();
            out.extend(k);
            wire_text(&mut out, a.as_str());
            wire_text(&mut out, r.as_str());
            out.extend(s.to_be_bytes());
            out.extend(b.to_be_bytes());
            out.extend(e.to_be_bytes());
            out.extend(f.to_be_bytes());
        }
        out.extend(self.quote.fee_estimate.to_be_bytes());
        wire_text(&mut out, &self.quote.asset_label);
        wire_text(&mut out, &self.quote.arrival_expectation);
        out
    }

    /// Decodes bounded canonical bytes and reconstructs every owner through validators.
    pub fn canonical_decode(bytes: &[u8]) -> Result<Self, MoveJourneyError> {
        if bytes.is_empty() || bytes.len() > 1_048_576 {
            return Err(MoveJourneyError::InvalidPlan);
        }
        let mut r = MoveWire::new(bytes);
        if r.u8()? != 1 {
            return Err(MoveJourneyError::InvalidPlan);
        }
        let journey_id = JourneyId::new(r.text(128)?).map_err(|_| MoveJourneyError::InvalidPlan)?;
        let idempotency_key = r.array()?;
        let custody_key = KeyId::new(r.text(128)?).map_err(|_| MoveJourneyError::InvalidPlan)?;
        let operation = Operation::from_label(&r.text(64)?).ok_or(MoveJourneyError::InvalidPlan)?;
        let route_len = r.u32()? as usize;
        if route_len == 0 || route_len > 262_144 {
            return Err(MoveJourneyError::InvalidPlan);
        }
        let request =
            RouteRequest::canonical_decode(r.take(route_len)?).map_err(MoveJourneyError::Route)?;
        let count = r.u16()? as usize;
        if count == 0 || count > 64 {
            return Err(MoveJourneyError::InvalidPlan);
        }
        let mut executions = Vec::with_capacity(count);
        for _ in 0..count {
            executions.push(MoveLegExecution::from_wire_parts(
                r.array()?,
                AgentDid::new(r.text(512)?).map_err(|_| MoveJourneyError::InvalidPlan)?,
                AuthorityRef::new(r.text(512)?).map_err(|_| MoveJourneyError::InvalidPlan)?,
                r.u64()?,
                r.u64()?,
                r.u64()?,
                r.u128()?,
            )?);
        }
        let fee_estimate = r.u128()?;
        let asset_label = r.text(TEXT_LIMIT)?;
        let arrival_expectation = r.text(TEXT_LIMIT)?;
        if !r.done() {
            return Err(MoveJourneyError::InvalidPlan);
        }
        let plan = Self::from_wire_parts(
            journey_id,
            idempotency_key,
            custody_key,
            operation,
            request,
            executions,
            fee_estimate,
            asset_label,
            arrival_expectation,
        )?;
        if plan.canonical_encode() != bytes {
            return Err(MoveJourneyError::InvalidPlan);
        }
        Ok(plan)
    }
    /// Returns every owner field needed for canonical wire encoding.
    #[must_use]
    pub fn to_wire_parts(
        &self,
    ) -> (
        &JourneyId,
        [u8; 32],
        &KeyId,
        Operation,
        &RouteRequest,
        &[MoveLegExecution],
        u128,
        &str,
        &str,
    ) {
        (
            &self.journey_id,
            self.idempotency_key,
            &self.custody_key,
            self.operation,
            &self.request,
            &self.executions,
            self.quote.fee_estimate,
            &self.quote.asset_label,
            &self.quote.arrival_expectation,
        )
    }

    /// Reconstructs provider wire fields through route and plan validation.
    #[allow(clippy::too_many_arguments)]
    pub fn from_wire_parts(
        journey_id: JourneyId,
        idempotency_key: [u8; 32],
        custody_key: KeyId,
        operation: Operation,
        request: RouteRequest,
        executions: Vec<MoveLegExecution>,
        fee_estimate: u128,
        asset_label: String,
        arrival_expectation: String,
    ) -> Result<Self, MoveJourneyError> {
        Self::new(
            journey_id,
            idempotency_key,
            custody_key,
            operation,
            request,
            executions,
            fee_estimate,
            asset_label,
            arrival_expectation,
        )
    }
    /// Rebinds an unchanged reviewed route to the commit idempotency key.
    /// This is restricted to the privileged server composition; all economic
    /// fields and per-leg action keys remain exactly those returned by the
    /// trusted movement planner.
    pub(crate) fn with_idempotency_key(
        mut self,
        idempotency_key: [u8; 32],
    ) -> Result<Self, MoveJourneyError> {
        if idempotency_key == [0; 32] {
            return Err(MoveJourneyError::InvalidPlan);
        }
        self.idempotency_key = idempotency_key;
        Ok(self)
    }

    /// Resolves the complete route and derives the review and aggregate fee ceiling.
    ///
    /// # Errors
    ///
    /// Refuses custody-boundary routes, invalid text, mismatched leg context,
    /// duplicate action keys, an estimate above the hard ceiling, or any
    /// resolver refusal. No partial route is retained.
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        journey_id: JourneyId,
        idempotency_key: [u8; 32],
        custody_key: KeyId,
        operation: Operation,
        request: RouteRequest,
        executions: Vec<MoveLegExecution>,
        fee_estimate: u128,
        asset_label: impl Into<String>,
        arrival_expectation: impl Into<String>,
    ) -> Result<Self, MoveJourneyError> {
        let route = RouteResolver::resolve(&request)?;
        if idempotency_key == [0; 32]
            || route.legs().is_empty()
            || route.legs().len() != executions.len()
            || route
                .legs()
                .iter()
                .any(|leg| leg.term().is_custody_boundary())
        {
            return Err(MoveJourneyError::InvalidPlan);
        }
        let mut action_keys = BTreeSet::new();
        if executions
            .iter()
            .any(|execution| !action_keys.insert(execution.action_key))
        {
            return Err(MoveJourneyError::InvalidPlan);
        }
        let fee_ceiling = executions.iter().try_fold(0_u128, |total, leg| {
            total
                .checked_add(leg.fee_ceiling)
                .ok_or(MoveJourneyError::InvalidPlan)
        })?;
        let asset_label = asset_label.into();
        let arrival_expectation = arrival_expectation.into();
        let irreversibility = IRREVERSIBILITY.to_owned();
        if fee_estimate > fee_ceiling
            || !valid_text(&asset_label)
            || !valid_text(&arrival_expectation)
            || !valid_text(&irreversibility)
        {
            return Err(MoveJourneyError::InvalidPlan);
        }
        let amount = request.amount.value();
        let plain_language = review_text(
            &route,
            amount,
            &asset_label,
            fee_estimate,
            fee_ceiling,
            &arrival_expectation,
            &irreversibility,
        );
        if plain_language.len() > TEXT_LIMIT.saturating_mul(5) {
            return Err(MoveJourneyError::InvalidPlan);
        }
        Ok(Self {
            journey_id,
            idempotency_key,
            custody_key,
            operation,
            request,
            route,
            executions,
            quote: MoveQuote {
                amount,
                asset_label,
                fee_estimate,
                fee_ceiling,
                arrival_expectation,
                irreversibility,
                plain_language,
            },
        })
    }

    /// Returns the deterministic plain-language review.
    #[must_use]
    pub const fn quote(&self) -> &MoveQuote {
        &self.quote
    }

    /// Returns the ordered resolved route.
    #[must_use]
    pub const fn route(&self) -> &Route {
        &self.route
    }
}

fn wire_text(out: &mut Vec<u8>, value: &str) {
    out.extend((value.len() as u16).to_be_bytes());
    out.extend(value.as_bytes())
}
struct MoveWire<'a> {
    bytes: &'a [u8],
    at: usize,
}
impl<'a> MoveWire<'a> {
    fn new(bytes: &'a [u8]) -> Self {
        Self { bytes, at: 0 }
    }
    fn take(&mut self, n: usize) -> Result<&'a [u8], MoveJourneyError> {
        let end = self
            .at
            .checked_add(n)
            .ok_or(MoveJourneyError::InvalidPlan)?;
        let value = self
            .bytes
            .get(self.at..end)
            .ok_or(MoveJourneyError::InvalidPlan)?;
        self.at = end;
        Ok(value)
    }
    fn array<const N: usize>(&mut self) -> Result<[u8; N], MoveJourneyError> {
        self.take(N)?
            .try_into()
            .map_err(|_| MoveJourneyError::InvalidPlan)
    }
    fn u8(&mut self) -> Result<u8, MoveJourneyError> {
        Ok(self.array::<1>()?[0])
    }
    fn u16(&mut self) -> Result<u16, MoveJourneyError> {
        Ok(u16::from_be_bytes(self.array()?))
    }
    fn u32(&mut self) -> Result<u32, MoveJourneyError> {
        Ok(u32::from_be_bytes(self.array()?))
    }
    fn u64(&mut self) -> Result<u64, MoveJourneyError> {
        Ok(u64::from_be_bytes(self.array()?))
    }
    fn u128(&mut self) -> Result<u128, MoveJourneyError> {
        Ok(u128::from_be_bytes(self.array()?))
    }
    fn text(&mut self, max: usize) -> Result<String, MoveJourneyError> {
        let n = self.u16()? as usize;
        if n == 0 || n > max {
            return Err(MoveJourneyError::InvalidPlan);
        }
        let bytes = self.take(n)?;
        let value = std::str::from_utf8(bytes).map_err(|_| MoveJourneyError::InvalidPlan)?;
        if value.chars().any(char::is_control) {
            return Err(MoveJourneyError::InvalidPlan);
        }
        Ok(value.to_owned())
    }
    fn done(&self) -> bool {
        self.at == self.bytes.len()
    }
}

/// Result of real policy, budget, capability and protocol preflight.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum MoveAuthorization {
    Allowed,
    Refused(LimitRefusal),
}

/// Honest public state of a committed move.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum MoveStage {
    Committed,
    Moving,
    StillChecking,
    Done,
    Refused,
}

/// Ordered immutable receipt reference for one completed leg.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct MoveReceiptReference {
    leg: usize,
    activity_id: [u8; 32],
    digest: [u8; 32],
}

impl MoveReceiptReference {
    #[must_use]
    pub const fn leg(&self) -> usize {
        self.leg
    }

    #[must_use]
    pub const fn activity_id(&self) -> [u8; 32] {
        self.activity_id
    }

    #[must_use]
    pub const fn digest(&self) -> [u8; 32] {
        self.digest
    }

    /// Returns the stable content-addressed receipt reference.
    #[must_use]
    pub fn reference(&self) -> String {
        format!("receipt-{}", hex(&self.digest))
    }
}

/// Receipt-backed progress for one ordered route leg.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct MoveLegProgress {
    index: usize,
    term: MovementTerm,
    mechanism: Mechanism,
    phase: JourneyPhase,
    receipt: Option<MoveReceiptReference>,
    actual_amount: Option<u128>,
    actual_fee: Option<u128>,
}

impl MoveLegProgress {
    #[must_use]
    pub const fn index(&self) -> usize {
        self.index
    }

    #[must_use]
    pub const fn term(&self) -> MovementTerm {
        self.term
    }

    #[must_use]
    pub const fn mechanism(&self) -> Mechanism {
        self.mechanism
    }

    #[must_use]
    pub const fn phase(&self) -> JourneyPhase {
        self.phase
    }

    #[must_use]
    pub const fn receipt(&self) -> Option<&MoveReceiptReference> {
        self.receipt.as_ref()
    }

    #[must_use]
    pub const fn actual_amount(&self) -> Option<u128> {
        self.actual_amount
    }

    #[must_use]
    pub const fn actual_fee(&self) -> Option<u128> {
        self.actual_fee
    }
}

/// Public movement status. Estimates disappear once receipt-backed actuals exist.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct MoveStatus {
    journey_id: JourneyId,
    stage: MoveStage,
    current_leg: usize,
    fee_estimate: Option<u128>,
    fee_ceiling: u128,
    actual_amount: Option<u128>,
    actual_fees: Option<u128>,
    legs: Vec<MoveLegProgress>,
    receipts: Vec<MoveReceiptReference>,
    refusal: Option<LimitRefusal>,
}

impl MoveStatus {
    #[must_use]
    pub const fn journey_id(&self) -> &JourneyId {
        &self.journey_id
    }

    #[must_use]
    pub const fn stage(&self) -> MoveStage {
        self.stage
    }

    #[must_use]
    pub const fn current_leg(&self) -> usize {
        self.current_leg
    }

    #[must_use]
    pub const fn fee_estimate(&self) -> Option<u128> {
        self.fee_estimate
    }

    #[must_use]
    pub const fn fee_ceiling(&self) -> u128 {
        self.fee_ceiling
    }

    #[must_use]
    pub const fn actual_amount(&self) -> Option<u128> {
        self.actual_amount
    }

    #[must_use]
    pub const fn actual_fees(&self) -> Option<u128> {
        self.actual_fees
    }

    #[must_use]
    pub fn legs(&self) -> &[MoveLegProgress] {
        &self.legs
    }

    #[must_use]
    pub fn receipt_references(&self) -> &[MoveReceiptReference] {
        &self.receipts
    }

    #[must_use]
    pub const fn refusal(&self) -> Option<&LimitRefusal> {
        self.refusal.as_ref()
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
struct StoredLeg {
    term: u8,
    mechanism: u8,
    action_key: [u8; 32],
    fee_ceiling: u128,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
struct StoredMove {
    version: u8,
    journey_id: String,
    idempotency_key: [u8; 32],
    plan_digest: [u8; 32],
    expected_asset: [u8; 32],
    requested_amount: u128,
    fee_estimate: u128,
    fee_ceiling: u128,
    asset_label: String,
    arrival_expectation: String,
    irreversibility: String,
    legs: Vec<StoredLeg>,
}

/// Durable move-money facade over the generic journey engine.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct MoveJourney {
    record: StoredMove,
    engine: JourneyEngine,
}

impl MoveJourney {
    /// Commits an authorised review and persists the complete execution plan
    /// before the first agent-layer effect. Repetition returns the original
    /// journey and conflicting reuse is refused.
    ///
    /// # Errors
    ///
    /// Returns the typed preflight refusal, route/compile/engine failure, or a
    /// durable evidence conflict.
    pub fn commit(
        scope: &mut PrincipalScope<'_>,
        plan: &MovePlan,
        authorization: MoveAuthorization,
        registry: &ModuleRegistry,
        now: u64,
    ) -> Result<Self, MoveJourneyError> {
        if let MoveAuthorization::Refused(refusal) = authorization {
            return Err(MoveJourneyError::Refused(refusal));
        }
        let digest = move_plan_digest(plan, registry)?;
        let row = move_row(&plan.journey_id)?;
        let record = stored_move(plan, digest);
        if let Some(existing) = scope.get(Table::Journeys, &row) {
            let existing = decode_move(existing.bytes())?;
            if existing.plan_digest != digest || existing != record {
                return Err(MoveJourneyError::IdempotencyConflict);
            }
        }
        let journey_plan = engine_plan(plan)?;
        let engine = JourneyEngine::start(scope, &journey_plan, registry, now)?;
        let bytes = serde_json::to_vec(&record)
            .map_err(|_| MoveJourneyError::Corrupt("move record cannot be encoded"))?;
        scope.put(Table::Journeys, row, now, bytes)?;
        Ok(Self { record, engine })
    }

    /// Loads one committed movement from its principal scope.
    ///
    /// # Errors
    ///
    /// Refuses corrupt metadata or an orphaned engine record.
    pub fn load(
        scope: &PrincipalScope<'_>,
        journey_id: &JourneyId,
    ) -> Result<Option<Self>, MoveJourneyError> {
        let row = move_row(journey_id)?;
        let Some(stored) = scope.get(Table::Journeys, &row) else {
            return Ok(None);
        };
        let record = decode_move(stored.bytes())?;
        let engine = JourneyEngine::load(scope, journey_id)?
            .ok_or(MoveJourneyError::Corrupt("move engine is missing"))?;
        Ok(Some(Self { record, engine }))
    }

    /// Advances at most one receipt-gated durable engine phase.
    ///
    /// # Errors
    ///
    /// Propagates real agent-contract, custody, receipt and store failures.
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
    ) -> Result<MoveStatus, MoveJourneyError> {
        self.engine
            .advance(scope, agent_contract, agent, custody, registry, trace, now)
            .await?;
        self.status()
    }

    /// Derives public progress and actuals only from independently verified
    /// engine receipts.
    ///
    /// # Errors
    ///
    /// Refuses malformed receipt evidence, an asset mismatch, a charged fee
    /// above its committed ceiling, or corrupt stored metadata.
    pub fn status(&self) -> Result<MoveStatus, MoveJourneyError> {
        let engine_status = self.engine.status()?;
        if engine_status.phases().len() != self.record.legs.len() {
            return Err(MoveJourneyError::Corrupt("move leg count changed"));
        }
        let mut legs = Vec::with_capacity(self.record.legs.len());
        let mut receipts = Vec::new();
        let mut actual_fees = 0_u128;
        let mut final_amount = None;
        for (index, (stored, phase)) in self
            .record
            .legs
            .iter()
            .zip(engine_status.phases())
            .enumerate()
        {
            let evidence = self.engine.verified_leg_evidence(index)?;
            let (receipt, actual_amount, actual_fee) = if let Some(evidence) = evidence {
                if Sha256::digest(&evidence.canonical_receipt).as_slice() != evidence.receipt_digest
                {
                    return Err(MoveJourneyError::ReceiptMismatch);
                }
                let facts = canonical_protocol_facts(&evidence.canonical_receipt)
                    .map_err(|_| MoveJourneyError::ReceiptMismatch)?;
                if facts.result_code() != 0
                    || facts.asset() != self.record.expected_asset
                    || facts.fee_charged() > stored.fee_ceiling
                {
                    return Err(MoveJourneyError::ReceiptMismatch);
                }
                actual_fees = actual_fees
                    .checked_add(facts.fee_charged())
                    .ok_or(MoveJourneyError::ReceiptMismatch)?;
                final_amount = Some(facts.amount());
                let reference = MoveReceiptReference {
                    leg: index,
                    activity_id: evidence.activity_id,
                    digest: evidence.receipt_digest,
                };
                receipts.push(reference.clone());
                (
                    Some(reference),
                    Some(facts.amount()),
                    Some(facts.fee_charged()),
                )
            } else {
                (None, None, None)
            };
            legs.push(MoveLegProgress {
                index,
                term: term_from_code(stored.term)?,
                mechanism: mechanism_from_code(stored.mechanism)?,
                phase: *phase,
                receipt,
                actual_amount,
                actual_fee,
            });
        }
        let stage = match engine_status.state() {
            JourneyState::GettingReady => MoveStage::Committed,
            JourneyState::Sending | JourneyState::Processing => MoveStage::Moving,
            JourneyState::StillChecking => MoveStage::StillChecking,
            JourneyState::Done => MoveStage::Done,
            JourneyState::Refused => MoveStage::Refused,
        };
        let refusal = if stage == MoveStage::Refused {
            let code = engine_status
                .refusal_codes()
                .iter()
                .flatten()
                .copied()
                .next()
                .ok_or(MoveJourneyError::Corrupt("refused move has no result"))?;
            Some(refusal_from_result(code)?)
        } else {
            None
        };
        let done = stage == MoveStage::Done;
        if done && receipts.len() != self.record.legs.len() {
            return Err(MoveJourneyError::Corrupt(
                "completed move lacks ordered receipts",
            ));
        }
        Ok(MoveStatus {
            journey_id: JourneyId::new(self.record.journey_id.clone())
                .map_err(|_| MoveJourneyError::Corrupt("invalid move journey id"))?,
            stage,
            current_leg: engine_status.current_leg(),
            fee_estimate: (!done).then_some(self.record.fee_estimate),
            fee_ceiling: self.record.fee_ceiling,
            actual_amount: done.then_some(final_amount.unwrap_or(self.record.requested_amount)),
            actual_fees: done.then_some(actual_fees),
            legs,
            receipts,
            refusal,
        })
    }
}

fn stored_move(plan: &MovePlan, plan_digest: [u8; 32]) -> StoredMove {
    StoredMove {
        version: RECORD_VERSION,
        journey_id: plan.journey_id.as_str().to_owned(),
        idempotency_key: plan.idempotency_key,
        plan_digest,
        expected_asset: plan.request.asset.bytes(),
        requested_amount: plan.request.amount.value(),
        fee_estimate: plan.quote.fee_estimate,
        fee_ceiling: plan.quote.fee_ceiling,
        asset_label: plan.quote.asset_label.clone(),
        arrival_expectation: plan.quote.arrival_expectation.clone(),
        irreversibility: plan.quote.irreversibility.clone(),
        legs: plan
            .route
            .legs()
            .iter()
            .zip(&plan.executions)
            .map(|(route, execution)| StoredLeg {
                term: term_code(route.term()),
                mechanism: mechanism_code(route.mechanism()),
                action_key: execution.action_key,
                fee_ceiling: execution.fee_ceiling,
            })
            .collect(),
    }
}

fn engine_plan(plan: &MovePlan) -> Result<JourneyPlan, MoveJourneyError> {
    let legs = plan
        .route
        .legs()
        .iter()
        .zip(&plan.executions)
        .map(|(route, execution)| {
            JourneyLeg::new(
                route.intent().clone(),
                execution.action_key,
                execution.actor.clone(),
                execution.authority.clone(),
                execution.account_sequence,
                execution.not_before,
                execution.not_after,
                execution.fee_ceiling,
            )
        })
        .collect::<Result<Vec<_>, _>>()?;
    Ok(JourneyPlan::new(
        plan.journey_id.clone(),
        super::engine::JourneyKind::Move,
        plan.idempotency_key,
        plan.custody_key.clone(),
        plan.operation,
        legs,
    )?)
}

fn move_plan_digest(
    plan: &MovePlan,
    registry: &ModuleRegistry,
) -> Result<[u8; 32], MoveJourneyError> {
    let mut digest = Sha256::new();
    digest.update(PLAN_DOMAIN);
    hash_text(&mut digest, plan.journey_id.as_str());
    digest.update(plan.idempotency_key);
    hash_text(&mut digest, plan.custody_key.as_str());
    hash_text(&mut digest, plan.operation.label());
    digest.update(plan.request.asset.bytes());
    digest.update(plan.request.amount.to_be_bytes());
    digest.update(plan.quote.fee_estimate.to_be_bytes());
    digest.update(plan.quote.fee_ceiling.to_be_bytes());
    hash_text(&mut digest, &plan.quote.asset_label);
    hash_text(&mut digest, &plan.quote.arrival_expectation);
    hash_text(&mut digest, &plan.quote.irreversibility);
    for (route, execution) in plan.route.legs().iter().zip(&plan.executions) {
        let compiled = compile(route.intent(), registry)
            .map_err(|error| MoveJourneyError::Journey(JourneyError::from(error)))?;
        digest.update([term_code(route.term()), mechanism_code(route.mechanism())]);
        digest.update(execution.action_key);
        hash_text(&mut digest, execution.actor.as_str());
        hash_text(&mut digest, execution.authority.as_str());
        digest.update(execution.account_sequence.to_be_bytes());
        digest.update(execution.not_before.to_be_bytes());
        digest.update(execution.not_after.to_be_bytes());
        digest.update(execution.fee_ceiling.to_be_bytes());
        digest.update(compiled.activity_type().value().to_be_bytes());
        digest.update(compiled.payload_hash());
        digest.update(Sha256::digest(compiled.payload().as_bytes()));
    }
    Ok(digest.finalize().into())
}

fn refusal_from_result(raw: i32) -> Result<LimitRefusal, MoveJourneyError> {
    let result = ResultCode::from_raw(raw);
    let (source, limit, surface) = match result.known() {
        Some(KnownResult::BudgetPeriodCap) => (
            LimitSource::Budget,
            "the agent's spending limit".to_owned(),
            Some(ChangeSurface::Budget),
        ),
        Some(KnownResult::BudgetAllowanceExceeded | KnownResult::InsufficientBudgetFunds) => (
            LimitSource::Budget,
            "the agent's available budget".to_owned(),
            Some(ChangeSurface::Budget),
        ),
        Some(KnownResult::BudgetRevoked) => (
            LimitSource::Budget,
            "the agent's paused budget".to_owned(),
            Some(ChangeSurface::Budget),
        ),
        Some(
            KnownResult::AuthScope
            | KnownResult::AuthAllowance
            | KnownResult::GrantScopeViolation
            | KnownResult::UnauthorizedDelegate,
        ) => (
            LimitSource::Capability,
            "the agent's permitted actions".to_owned(),
            Some(ChangeSurface::Capability),
        ),
        Some(KnownResult::FeeLimit) => (
            LimitSource::Protocol,
            "the committed maximum fee".to_owned(),
            None,
        ),
        Some(KnownResult::InsufficientBalance) => (
            LimitSource::Protocol,
            "the available balance".to_owned(),
            None,
        ),
        _ => (LimitSource::Protocol, format!("protocol rule {raw}"), None),
    };
    LimitRefusal::new(source, limit, surface)
        .map_err(|_| MoveJourneyError::Corrupt("invalid refusal ownership"))
}

fn review_text(
    route: &Route,
    amount: u128,
    asset: &str,
    fee_estimate: u128,
    fee_ceiling: u128,
    arrival: &str,
    irreversibility: &str,
) -> String {
    let steps = route
        .legs()
        .iter()
        .map(|leg| match leg.mechanism() {
            Mechanism::BudgetCreate => "set up the agent's spending limit",
            Mechanism::BudgetFund => "move the money into the agent's budget",
            Mechanism::Send => "send the money to the recipient",
            Mechanism::BudgetDefund => "return the money from the agent's budget",
            Mechanism::ReceiveUnderPayerGrant => "receive the authorised payment",
            Mechanism::BridgeDepositCredit | Mechanism::BridgeWithdrawRequest => {
                "cross the wallet boundary"
            }
        })
        .collect::<Vec<_>>()
        .join(", then ");
    format!(
        "Move {amount} {asset}: {steps}. Estimated fees are {fee_estimate} {asset} and cannot exceed {fee_ceiling} {asset}. Expected arrival: {arrival}. {irreversibility}"
    )
}

fn valid_text(value: &str) -> bool {
    !value.trim().is_empty() && value.len() <= TEXT_LIMIT && !value.chars().any(char::is_control)
}

fn move_row(journey_id: &JourneyId) -> Result<RowKey, MoveJourneyError> {
    Ok(RowKey::new(format!(
        "{RECORD_PREFIX}{}",
        journey_id
            .as_str()
            .strip_prefix("jrn_")
            .unwrap_or(journey_id.as_str())
    ))?)
}

fn decode_move(bytes: &[u8]) -> Result<StoredMove, MoveJourneyError> {
    let record: StoredMove = serde_json::from_slice(bytes)
        .map_err(|_| MoveJourneyError::Corrupt("invalid move record encoding"))?;
    if record.version != RECORD_VERSION
        || JourneyId::new(record.journey_id.clone()).is_err()
        || record.idempotency_key == [0; 32]
        || record.plan_digest == [0; 32]
        || record.expected_asset == [0; 32]
        || record.requested_amount == 0
        || record.legs.is_empty()
        || record.fee_estimate > record.fee_ceiling
        || !valid_text(&record.asset_label)
        || !valid_text(&record.arrival_expectation)
        || !valid_text(&record.irreversibility)
    {
        return Err(MoveJourneyError::Corrupt(
            "move record invariants are invalid",
        ));
    }
    let mut keys = BTreeSet::new();
    let total_ceiling = record.legs.iter().try_fold(0_u128, |total, leg| {
        if leg.action_key == [0; 32]
            || !keys.insert(leg.action_key)
            || term_from_code(leg.term).is_err()
            || mechanism_from_code(leg.mechanism).is_err()
        {
            return None;
        }
        total.checked_add(leg.fee_ceiling)
    });
    if total_ceiling != Some(record.fee_ceiling) {
        return Err(MoveJourneyError::Corrupt("move leg metadata is invalid"));
    }
    Ok(record)
}

const fn term_code(value: MovementTerm) -> u8 {
    match value {
        MovementTerm::Deposit => 1,
        MovementTerm::Withdrawal => 2,
        MovementTerm::Fund => 3,
        MovementTerm::Allocate => 4,
        MovementTerm::Return => 5,
        MovementTerm::Transfer => 6,
    }
}

fn term_from_code(value: u8) -> Result<MovementTerm, MoveJourneyError> {
    match value {
        3 => Ok(MovementTerm::Fund),
        4 => Ok(MovementTerm::Allocate),
        5 => Ok(MovementTerm::Return),
        6 => Ok(MovementTerm::Transfer),
        _ => Err(MoveJourneyError::Corrupt("invalid internal movement term")),
    }
}

const fn mechanism_code(value: Mechanism) -> u8 {
    match value {
        Mechanism::BudgetCreate => 1,
        Mechanism::BudgetFund => 2,
        Mechanism::Send => 3,
        Mechanism::BudgetDefund => 4,
        Mechanism::ReceiveUnderPayerGrant => 5,
        Mechanism::BridgeDepositCredit => 6,
        Mechanism::BridgeWithdrawRequest => 7,
    }
}

fn mechanism_from_code(value: u8) -> Result<Mechanism, MoveJourneyError> {
    match value {
        1 => Ok(Mechanism::BudgetCreate),
        2 => Ok(Mechanism::BudgetFund),
        3 => Ok(Mechanism::Send),
        4 => Ok(Mechanism::BudgetDefund),
        5 => Ok(Mechanism::ReceiveUnderPayerGrant),
        _ => Err(MoveJourneyError::Corrupt(
            "invalid internal movement mechanism",
        )),
    }
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

/// Typed move failure. No error is rendered as completed movement.
#[derive(Debug)]
pub enum MoveJourneyError {
    Route(RouteError),
    Journey(JourneyError),
    Store(StoreError),
    Refused(LimitRefusal),
    InvalidPlan,
    IdempotencyConflict,
    ReceiptMismatch,
    Corrupt(&'static str),
}

impl Display for MoveJourneyError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Route(error) => write!(formatter, "move route failure: {error}"),
            Self::Journey(error) => write!(formatter, "move execution failure: {error}"),
            Self::Store(error) => write!(formatter, "move store failure: {error}"),
            Self::Refused(refusal) => formatter.write_str(&refusal.plain_language()),
            Self::InvalidPlan => formatter.write_str("move plan is invalid"),
            Self::IdempotencyConflict => {
                formatter.write_str("move idempotency key owns another plan")
            }
            Self::ReceiptMismatch => {
                formatter.write_str("move receipt differs from the committed route")
            }
            Self::Corrupt(reason) => write!(formatter, "corrupt move journey: {reason}"),
        }
    }
}

impl std::error::Error for MoveJourneyError {}

impl From<RouteError> for MoveJourneyError {
    fn from(value: RouteError) -> Self {
        Self::Route(value)
    }
}

impl From<JourneyError> for MoveJourneyError {
    fn from(value: JourneyError) -> Self {
        Self::Journey(value)
    }
}

impl From<StoreError> for MoveJourneyError {
    fn from(value: StoreError) -> Self {
        Self::Store(value)
    }
}
