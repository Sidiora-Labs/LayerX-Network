//! Receipt- and protocol-state-authoritative budget reconciliation.

use crate::protocol_evidence::{
    EvidenceAuthority, RawReceiptEvidence, RawStateEvidence, ReceiptReplayError,
};

/// Uninterpreted state inclusion candidate for a protocol budget.
///
/// Core does not currently define or produce a canonical budget record key and
/// value schema, so inclusion of these bytes never authorizes reconciliation.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ProtocolBudgetState {
    pub evidence: RawStateEvidence,
}

/// Raw receipt evidence applied to one declared protocol budget window.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SpendReceiptEvidence {
    pub expected_activity_id: [u8; 32],
    pub evidence: RawReceiptEvidence,
}

/// Rebuildable local cache, never authority.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct LocalAccounting {
    pub consumed: u128,
    pub window_start_sequence: u64,
    pub last_receipt: Option<[u8; 32]>,
}

/// Observable reconciliation result including the corrected divergence.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ReconciliationState {
    pub last_verified_receipt: Option<[u8; 32]>,
    pub protocol_consumed: u128,
    pub local_before: u128,
    pub local_after: u128,
    pub divergence: Option<i128>,
    pub window_start_sequence: u64,
    pub window_end_sequence: u64,
    pub remaining: u128,
    pub observed_head_sequence: u64,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ReconcileError {
    UnverifiedProtocolState,
    ProtocolStateSchemaUnavailable,
    UnverifiedReceipt,
    DuplicateReceipt,
    DuplicateActivity,
    ReceiptActivityMismatch,
}

/// Verifies receipt identity and state inclusion, then refuses reconciliation
/// because no canonical protocol budget state schema exists.
pub(crate) fn reconcile_state(
    local: &mut LocalAccounting,
    protocol: ProtocolBudgetState,
    receipts: &[SpendReceiptEvidence],
    verifier: &EvidenceAuthority,
) -> Result<ReconciliationState, ReconcileError> {
    let mut replay = verifier.receipt_replay_guard();
    for receipt in receipts {
        let verified = verifier
            .verify_receipt(&receipt.evidence)
            .map_err(|_| ReconcileError::UnverifiedReceipt)?;
        if verified.activity_id() != receipt.expected_activity_id {
            return Err(ReconcileError::ReceiptActivityMismatch);
        }
        replay.admit(&verified).map_err(map_replay_error)?;
    }
    let _ = local;
    let _verified = verifier
        .verify_state(&protocol.evidence)
        .map_err(|_| ReconcileError::UnverifiedProtocolState)?;
    Err(ReconcileError::ProtocolStateSchemaUnavailable)
}

const fn map_replay_error(error: ReceiptReplayError) -> ReconcileError {
    match error {
        ReceiptReplayError::DuplicateReceipt => ReconcileError::DuplicateReceipt,
        ReceiptReplayError::DuplicateActivity => ReconcileError::DuplicateActivity,
    }
}
