//! Receipt- and protocol-state-authoritative budget reconciliation.

use crate::protocol_evidence::{
    verify_receipt, verify_state_evidence, RawReceiptEvidence, RawStateEvidence,
};

const BUDGET_STATE_MAGIC: &[u8; 4] = b"LXBS";

/// Raw proof-bearing protocol budget state for one deterministic window.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ProtocolBudgetState {
    pub evidence: RawStateEvidence,
}

/// Raw receipt evidence applied to one declared protocol budget window.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SpendReceiptEvidence {
    pub window_start_sequence: u64,
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
    UnverifiedReceipt,
    ReceiptFromOtherWindow,
    Arithmetic,
}

/// Reconciles the cache to verified protocol state and records any correction.
pub(crate) fn reconcile_state(
    local: &mut LocalAccounting,
    protocol: ProtocolBudgetState,
    receipts: &[SpendReceiptEvidence],
) -> Result<ReconciliationState, ReconcileError> {
    let protocol = verify_protocol_budget_state(&protocol)?;
    let mut receipt_total = 0_u128;
    let mut last_receipt = None;
    for receipt in receipts {
        let verified = verify_receipt(&receipt.evidence)
            .map_err(|_| ReconcileError::UnverifiedReceipt)?;
        if receipt.window_start_sequence != protocol.window_start_sequence {
            return Err(ReconcileError::ReceiptFromOtherWindow);
        }
        if verified.global_sequence() < protocol.window_start_sequence
            || verified.global_sequence() > protocol.window_end_sequence
            || verified.global_sequence() > protocol.observed_head_sequence
        {
            return Err(ReconcileError::ReceiptFromOtherWindow);
        }
        if verified.result_code() == 0 {
            receipt_total = receipt_total
                .checked_add(verified.amount())
                .ok_or(ReconcileError::Arithmetic)?;
            last_receipt = Some(verified.receipt_ref());
        }
    }
    if receipt_total > protocol.consumed {
        return Err(ReconcileError::Arithmetic);
    }
    let local_before = local.consumed;
    let divergence = if local_before == protocol.consumed {
        None
    } else {
        let protocol_value =
            i128::try_from(protocol.consumed).map_err(|_| ReconcileError::Arithmetic)?;
        let local_value = i128::try_from(local_before).map_err(|_| ReconcileError::Arithmetic)?;
        Some(protocol_value - local_value)
    };
    local.consumed = protocol.consumed;
    local.window_start_sequence = protocol.window_start_sequence;
    local.last_receipt = last_receipt;
    Ok(ReconciliationState {
        last_verified_receipt: last_receipt,
        protocol_consumed: protocol.consumed,
        local_before,
        local_after: local.consumed,
        divergence,
        window_start_sequence: protocol.window_start_sequence,
        window_end_sequence: protocol.window_end_sequence,
        remaining: protocol.remaining,
        observed_head_sequence: protocol.observed_head_sequence,
    })
}

#[derive(Clone, Copy)]
pub(crate) struct DecodedBudgetState {
    pub(crate) consumed: u128,
    pub(crate) remaining: u128,
    pub(crate) window_start_sequence: u64,
    pub(crate) window_end_sequence: u64,
    pub(crate) observed_head_sequence: u64,
}

pub(crate) fn verify_protocol_budget_state(
    state: &ProtocolBudgetState,
) -> Result<DecodedBudgetState, ReconcileError> {
    let verified = verify_state_evidence(&state.evidence)
        .map_err(|_| ReconcileError::UnverifiedProtocolState)?;
    decode_budget_state(
        verified.canonical_state(),
        verified.observed_head_sequence(),
    )
}

fn decode_budget_state(
    bytes: &[u8],
    observed_head_sequence: u64,
) -> Result<DecodedBudgetState, ReconcileError> {
    if bytes.len() != 52 || &bytes[..4] != BUDGET_STATE_MAGIC {
        return Err(ReconcileError::UnverifiedProtocolState);
    }
    let consumed = u128::from_be_bytes(
        bytes[4..20]
            .try_into()
            .map_err(|_| ReconcileError::UnverifiedProtocolState)?,
    );
    let remaining = u128::from_be_bytes(
        bytes[20..36]
            .try_into()
            .map_err(|_| ReconcileError::UnverifiedProtocolState)?,
    );
    let window_start_sequence = u64::from_be_bytes(
        bytes[36..44]
            .try_into()
            .map_err(|_| ReconcileError::UnverifiedProtocolState)?,
    );
    let window_end_sequence = u64::from_be_bytes(
        bytes[44..52]
            .try_into()
            .map_err(|_| ReconcileError::UnverifiedProtocolState)?,
    );
    if window_start_sequence == 0
        || window_end_sequence < window_start_sequence
        || observed_head_sequence < window_start_sequence
        || consumed.checked_add(remaining).is_none()
    {
        return Err(ReconcileError::UnverifiedProtocolState);
    }
    Ok(DecodedBudgetState {
        consumed,
        remaining,
        window_start_sequence,
        window_end_sequence,
        observed_head_sequence,
    })
}
