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

/// Opaque reconciliation result issued only after protocol evidence succeeds.
///
/// No public constructor exists. While the canonical budget record/key schema
/// is unavailable, reconciliation issues no value of this type.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ReconciliationState {
    last_verified_receipt: Option<[u8; 32]>,
    protocol_consumed: u128,
    local_before: u128,
    local_after: u128,
    divergence: Option<i128>,
    window_start_sequence: u64,
    window_end_sequence: u64,
    remaining: u128,
    observed_head_sequence: u64,
}

impl ReconciliationState {
    /// Returns the protocol remaining amount from this opaque verified result.
    #[must_use]
    pub const fn remaining(&self) -> u128 {
        self.remaining
    }

    /// Returns the signed head sequence which anchored this result.
    #[must_use]
    pub const fn observed_head_sequence(&self) -> u64 {
        self.observed_head_sequence
    }

    /// Returns the corrected local amount from this opaque verified result.
    #[must_use]
    pub const fn local_after(&self) -> u128 {
        self.local_after
    }

    pub(crate) const fn last_verified_receipt(&self) -> Option<[u8; 32]> {
        self.last_verified_receipt
    }

    pub(crate) const fn protocol_consumed(&self) -> u128 {
        self.protocol_consumed
    }

    pub(crate) const fn local_before(&self) -> u128 {
        self.local_before
    }

    pub(crate) const fn divergence(&self) -> Option<i128> {
        self.divergence
    }

    pub(crate) const fn window_start_sequence(&self) -> u64 {
        self.window_start_sequence
    }

    pub(crate) const fn window_end_sequence(&self) -> u64 {
        self.window_end_sequence
    }
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
    protocol: &ProtocolBudgetState,
    receipts: &[SpendReceiptEvidence],
    verifier: &EvidenceAuthority,
) -> Result<ReconciliationState, ReconcileError> {
    let mut replay = EvidenceAuthority::receipt_replay_guard();
    for receipt in receipts {
        let verified_receipt = verifier
            .verify_receipt(&receipt.evidence)
            .map_err(|_| ReconcileError::UnverifiedReceipt)?;
        if verified_receipt.activity_id() != receipt.expected_activity_id {
            return Err(ReconcileError::ReceiptActivityMismatch);
        }
        replay.admit(&verified_receipt).map_err(map_replay_error)?;
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
