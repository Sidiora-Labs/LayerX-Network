//! Budget creation through the ordinary canonical write pipeline.

use crate::protocol_evidence::{EvidenceAuthority, RawReceiptEvidence};
use crate::sign::VerifiedSubmission;
use crate::store::{Store, TenantId};

const LOCAL_BYPASS_STATEMENT: &str =
    "daemon-enforced only; bypassing layerx-agentd bypasses this limit";

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
/// This boundary supplies no object identifier; only verified canonical effects
/// may issue one.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CoreBudgetReceipt {
    pub evidence: RawReceiptEvidence,
}

/// The mandatory exact-submission and raw-evidence seam.
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
}

/// Successfully created protocol-backed budget.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ProtocolBudget {
    object_id: [u8; 32],
    kind: BudgetKind,
    receipt_bytes: Vec<u8>,
    enforcement: &'static str,
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
    Submission,
    UnverifiedReceipt,
    ReceiptActivityMismatch,
    CoreRejected,
    ProtocolObjectEffectUnavailable,
}

/// Offers a verifier-bound signed canonical limit activity to core.
///
/// # Errors
///
/// Refuses a zero ceiling or expiry sequence, missing or substituted verifier
/// binding, a receipt for any activity other than the exact canonical verified
/// submission, and an unverified or rejected receipt. The current receipt schema
/// carries no created budget object effect, so a successful activity still fails
/// closed without caching an object ID.
pub fn create_protocol_budget(
    _store: &mut Store,
    request: &BudgetRequest,
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
    Err(BudgetCreationError::ProtocolObjectEffectUnavailable)
}
