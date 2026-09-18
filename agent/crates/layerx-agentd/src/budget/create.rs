//! Budget creation through the ordinary canonical write pipeline.

use layerx_types::payload::{ActivityType, ModuleId, ModuleRegistry};
use layerx_wire::decode::Decoder;

use super::accounting::{ProtocolBudgetRecord, ProtocolBudgetState};
use crate::protocol_evidence::{EvidenceAuthority, RawReceiptEvidence};
use crate::sign::VerifiedSubmission;
use crate::store::{Store, TenantId};

const LOCAL_BYPASS_STATEMENT: &str =
    "daemon-enforced only; bypassing layerx-agentd bypasses this limit";
const BUDGET_CREATE_WIRE_TAG: u16 = 0x4201;
const BUDGET_CREATE_FIELD_COUNT: u16 = 10;
const BUDGET_CREATE_ORDINAL: u16 = 1;
const PROTOCOL_ENFORCEMENT: &str = "protocol-enforced";

/// Protocol object offered for a spending limit.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum BudgetKind {
    ProtocolBudget,
    CapabilityGrant,
}

/// Complete input to a protocol-enforced limit creation activity.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct BudgetRequest {
    pub tenant: TenantId,
    pub request_id: [u8; 32],
    pub kind: BudgetKind,
    pub asset: [u8; 32],
    pub ceiling: u128,
    pub expiry_sequence: u64,
    pub canonical_activity: Vec<u8>,
    pub verified_submission: Option<VerifiedSubmission>,
}

/// Raw core receipt returned for the submitted creation activity.
///
/// This boundary supplies no object identifier; the identifier is read from
/// the canonical activity by the rule core keys the record with, then
/// confirmed from proven module state.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CoreBudgetReceipt {
    pub evidence: RawReceiptEvidence,
}

/// The mandatory exact-submission, raw-evidence and proven-state seam.
pub trait BudgetPipeline {
    /// Submits the byte-identical verifier-bound budget activity and returns raw core evidence.
    ///
    /// # Errors
    ///
    /// Returns `Submission` when the exact signed canonical activity does not reach core and
    /// produce a receipt.
    fn submit_budget(
        &mut self,
        request: &BudgetRequest,
    ) -> Result<CoreBudgetReceipt, BudgetCreationError>;

    /// Reads the proven budget module state stored under `budget_state_key(budget_id)`.
    ///
    /// # Errors
    ///
    /// Returns `CreatedBudgetUnconfirmed` when the node serves no proven state for the key.
    fn budget_state(
        &mut self,
        budget_id: [u8; 32],
    ) -> Result<ProtocolBudgetState, BudgetCreationError>;
}

/// Object-naming fields of one canonical budget-create payload.
///
/// Core keys the created record by the payload's own `budget_id`
/// (`lx_budget_create_decode` copies it and `budget_execute_create` saves the
/// record under `lx_budget_state_key(budget_id)`), so the identifier is read
/// from the exact bytes that were signed, never from the receipt.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct BudgetCreateIdentity {
    pub budget_id: [u8; 32],
    pub owner: [u8; 32],
    pub budget_account: [u8; 32],
    pub asset: [u8; 32],
    pub per_period_limit: u128,
    pub period_length: u64,
    pub expiry: u64,
}

/// Decodes the budget identity from a signed canonical budget-create activity.
///
/// # Errors
///
/// Returns `NotBudgetCreation` when the bytes are not one canonical signed
/// activity of type budget/create carrying a well-formed create payload.
pub fn budget_create_identity(
    canonical_activity: &[u8],
    registry: &ModuleRegistry,
) -> Result<BudgetCreateIdentity, BudgetCreationError> {
    let not_creation = BudgetCreationError::NotBudgetCreation;
    let activity = layerx_wire::activity::decode_signed(canonical_activity, registry)
        .map_err(|_| not_creation)?;
    let create =
        ActivityType::new(ModuleId::Budget, BUDGET_CREATE_ORDINAL).map_err(|_| not_creation)?;
    if activity.activity_type() != create {
        return Err(not_creation);
    }
    let mut decoder = Decoder::new(activity.payload(), 0);
    if decoder.u16().map_err(|_| not_creation)? != BUDGET_CREATE_WIRE_TAG
        || decoder.u16().map_err(|_| not_creation)? != BUDGET_CREATE_FIELD_COUNT
    {
        return Err(not_creation);
    }
    let budget_id = fixed32(&mut decoder)?;
    let owner = fixed32(&mut decoder)?;
    let budget_account = fixed32(&mut decoder)?;
    let asset = fixed32(&mut decoder)?;
    let per_period_limit = decoder.u128().map_err(|_| not_creation)?;
    let period_length = decoder.u64().map_err(|_| not_creation)?;
    let _rollover = decoder.u8().map_err(|_| not_creation)?;
    let _carry_cap = decoder.u128().map_err(|_| not_creation)?;
    let _purpose = fixed32(&mut decoder)?;
    let expiry = decoder.u64().map_err(|_| not_creation)?;
    decoder.finish().map_err(|_| not_creation)?;
    if budget_id == [0; 32] {
        return Err(not_creation);
    }
    Ok(BudgetCreateIdentity {
        budget_id,
        owner,
        budget_account,
        asset,
        per_period_limit,
        period_length,
        expiry,
    })
}

fn fixed32(decoder: &mut Decoder<'_>) -> Result<[u8; 32], BudgetCreationError> {
    decoder
        .fixed(32)
        .map_err(|_| BudgetCreationError::NotBudgetCreation)?
        .try_into()
        .map_err(|_| BudgetCreationError::NotBudgetCreation)
}

/// Successfully created protocol-backed budget, confirmed from proven core state.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ProtocolBudget {
    object_id: [u8; 32],
    kind: BudgetKind,
    receipt_bytes: Vec<u8>,
    enforcement: &'static str,
    record: ProtocolBudgetRecord,
    observed_head_sequence: u64,
}

impl ProtocolBudget {
    #[must_use]
    pub const fn object_id(&self) -> [u8; 32] {
        self.object_id
    }

    #[must_use]
    pub const fn kind(&self) -> BudgetKind {
        self.kind
    }

    #[must_use]
    pub fn receipt_bytes(&self) -> &[u8] {
        &self.receipt_bytes
    }

    #[must_use]
    pub const fn enforcement(&self) -> &'static str {
        self.enforcement
    }

    /// Returns the canonical core record the confirmation decoded.
    #[must_use]
    pub const fn record(&self) -> &ProtocolBudgetRecord {
        &self.record
    }

    /// Returns the signed head sequence that anchored the confirming state.
    #[must_use]
    pub const fn observed_head_sequence(&self) -> u64 {
        self.observed_head_sequence
    }
}

/// Honest daemon-only limit.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct LocalLimit {
    pub tenant: TenantId,
    pub id: [u8; 32],
    pub asset: [u8; 32],
    pub ceiling: u128,
    pub enforcement: &'static str,
    pub bypass_statement: &'static str,
}

impl LocalLimit {
    #[must_use]
    pub fn new(tenant: TenantId, id: [u8; 32], asset: [u8; 32], ceiling: u128) -> Self {
        Self {
            tenant,
            id,
            asset,
            ceiling,
            enforcement: "daemon-enforced",
            bypass_statement: LOCAL_BYPASS_STATEMENT,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum BudgetCreationError {
    EmptyActivity,
    InvalidLimit,
    ActivityBindingUnavailable,
    ActivityBindingMismatch,
    NotBudgetCreation,
    IdentityMismatch,
    Submission,
    UnverifiedReceipt,
    ReceiptActivityMismatch,
    CoreRejected,
    CreatedBudgetUnconfirmed,
}

/// Offers a verifier-bound signed canonical limit activity to core and confirms
/// the created object from proven module state.
///
/// # Errors
///
/// Refuses a zero ceiling or expiry sequence, missing or substituted verifier
/// binding, an activity that is not a canonical budget creation, a payload
/// whose limit, expiry or asset differ from the request, a receipt for any
/// activity other than the exact canonical verified submission, and an
/// unverified or rejected receipt. After a successful receipt the budget
/// identifier is read from the payload by core's own keying rule and the
/// record under `budget_state_key` must verify, decode and match the request;
/// otherwise `CreatedBudgetUnconfirmed` is returned and nothing is cached.
pub fn create_protocol_budget(
    _store: &mut Store,
    request: &BudgetRequest,
    registry: &ModuleRegistry,
    verifier: &EvidenceAuthority,
    pipeline: &mut dyn BudgetPipeline,
) -> Result<ProtocolBudget, BudgetCreationError> {
    if request.ceiling == 0 || request.expiry_sequence == 0 {
        return Err(BudgetCreationError::InvalidLimit);
    }
    if request.canonical_activity.is_empty() {
        return Err(BudgetCreationError::EmptyActivity);
    }
    let submission = request
        .verified_submission
        .as_ref()
        .ok_or(BudgetCreationError::ActivityBindingUnavailable)?;
    if submission.exact_bytes() != request.canonical_activity.as_slice()
        || submission.idempotency_key() != request.request_id
    {
        return Err(BudgetCreationError::ActivityBindingMismatch);
    }
    let identity = budget_create_identity(submission.exact_bytes(), registry)?;
    if identity.per_period_limit != request.ceiling
        || identity.expiry != request.expiry_sequence
        || identity.asset != request.asset
    {
        return Err(BudgetCreationError::IdentityMismatch);
    }
    let receipt = pipeline.submit_budget(request)?;
    let verified_receipt = verifier
        .verify_receipt(&receipt.evidence)
        .map_err(|_| BudgetCreationError::UnverifiedReceipt)?;
    if verified_receipt.activity_id() != submission.activity_id() {
        return Err(BudgetCreationError::ReceiptActivityMismatch);
    }
    if verified_receipt.result_code() != 0 {
        return Err(BudgetCreationError::CoreRejected);
    }
    let state = pipeline.budget_state(identity.budget_id)?;
    let verified_state = verifier
        .verify_state(&state.evidence)
        .map_err(|_| BudgetCreationError::CreatedBudgetUnconfirmed)?;
    let record = ProtocolBudgetRecord::decode(verified_state.canonical_state())
        .map_err(|_| BudgetCreationError::CreatedBudgetUnconfirmed)?;
    if record.budget_id != identity.budget_id
        || record.owner != identity.owner
        || record.budget_account != identity.budget_account
        || record.asset_id != request.asset
        || record.per_period_limit != request.ceiling
        || record.expiry != request.expiry_sequence
        || record.closed
        || record.revoked
    {
        return Err(BudgetCreationError::CreatedBudgetUnconfirmed);
    }
    Ok(ProtocolBudget {
        object_id: record.budget_id,
        kind: request.kind,
        receipt_bytes: verified_receipt.canonical_receipt().to_vec(),
        enforcement: PROTOCOL_ENFORCEMENT,
        observed_head_sequence: verified_state.observed_head_sequence(),
        record,
    })
}
