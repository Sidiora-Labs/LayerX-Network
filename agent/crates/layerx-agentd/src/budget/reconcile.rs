//! Receipt- and protocol-state-authoritative budget reconciliation.

use crate::protocol_evidence::{
    EvidenceAuthority, RawReceiptEvidence, RawStateEvidence, ReceiptReplayError,
};

/// State inclusion candidate for a protocol budget.
///
/// The verified leaf must carry the core budget module record stored under
/// [`budget_state_key`]; any other leaf leaves reconciliation unavailable.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ProtocolBudgetState {
    pub evidence: RawStateEvidence,
}

/// Core module identifier whose state holds canonical budget records.
pub const BUDGET_MODULE_ID: u16 = 3;

const BUDGET_STATE_KEY_PREFIX: &[u8] = b"budget:";
const BUDGET_RECORD_FIXED_BYTES: usize = 278;
const BUDGET_MAX_DELEGATES: usize = 16;

/// Returns the exact core state key under which a budget record is stored.
#[must_use]
pub fn budget_state_key(budget_id: [u8; 32]) -> Vec<u8> {
    let mut key = Vec::with_capacity(BUDGET_STATE_KEY_PREFIX.len() + budget_id.len());
    key.extend_from_slice(BUDGET_STATE_KEY_PREFIX);
    key.extend_from_slice(&budget_id);
    key
}

/// Canonical core budget record decoded from verified module state bytes.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ProtocolBudgetRecord {
    pub budget_id: [u8; 32],
    pub owner: [u8; 32],
    pub budget_account: [u8; 32],
    pub asset_id: [u8; 32],
    pub purpose_hash: [u8; 32],
    pub per_period_limit: u128,
    pub configured_period_limit: u128,
    pub carry_cap: u128,
    pub spent_this_period: u128,
    pub carried: u128,
    pub period_length: u64,
    pub period_start: u64,
    pub expiry: u64,
    pub revocation_sequence: u64,
    pub rollover_policy: u8,
    pub closed: bool,
    pub revoked: bool,
    pub delegates: Vec<[u8; 32]>,
    pub source_account: Option<[u8; 32]>,
}

impl ProtocolBudgetRecord {
    /// Decodes the exact core budget record encoding.
    ///
    /// # Errors
    ///
    /// Returns `ProtocolStateSchemaUnavailable` when the bytes are not one canonical
    /// core budget record.
    pub fn decode(bytes: &[u8]) -> Result<Self, ReconcileError> {
        let unavailable = ReconcileError::ProtocolStateSchemaUnavailable;
        if bytes.len() < BUDGET_RECORD_FIXED_BYTES
            || bytes[0] != 0
            || (bytes[1] != 1 && bytes[1] != 2)
            || bytes[275] > 1
            || bytes[276] > 1
        {
            return Err(unavailable);
        }
        let native_source = bytes[1] == 2;
        let delegate_count = usize::from(bytes[277]);
        let expected =
            BUDGET_RECORD_FIXED_BYTES + delegate_count * 32 + if native_source { 32 } else { 0 };
        if delegate_count > BUDGET_MAX_DELEGATES || bytes.len() != expected {
            return Err(unavailable);
        }
        let fixed = |offset: usize| -> [u8; 32] {
            let mut value = [0_u8; 32];
            value.copy_from_slice(&bytes[offset..offset + 32]);
            value
        };
        let u128_at = |offset: usize| -> u128 {
            let mut value = [0_u8; 16];
            value.copy_from_slice(&bytes[offset..offset + 16]);
            u128::from_be_bytes(value)
        };
        let u64_at = |offset: usize| -> u64 {
            let mut value = [0_u8; 8];
            value.copy_from_slice(&bytes[offset..offset + 8]);
            u64::from_be_bytes(value)
        };
        let mut delegates = Vec::with_capacity(delegate_count);
        for index in 0..delegate_count {
            let entry = fixed(BUDGET_RECORD_FIXED_BYTES + index * 32);
            if delegates.last().is_some_and(|previous| *previous >= entry) {
                return Err(unavailable);
            }
            delegates.push(entry);
        }
        let record = Self {
            budget_id: fixed(2),
            owner: fixed(34),
            budget_account: fixed(66),
            asset_id: fixed(98),
            purpose_hash: fixed(130),
            per_period_limit: u128_at(162),
            configured_period_limit: u128_at(178),
            carry_cap: u128_at(194),
            spent_this_period: u128_at(210),
            carried: u128_at(226),
            period_length: u64_at(242),
            period_start: u64_at(250),
            expiry: u64_at(258),
            revocation_sequence: u64_at(266),
            rollover_policy: bytes[274],
            closed: bytes[275] != 0,
            revoked: bytes[276] != 0,
            delegates,
            source_account: native_source.then(|| fixed(bytes.len() - 32)),
        };
        if record.budget_id == [0; 32]
            || record.period_length == 0
            || record.spent_this_period > record.per_period_limit
            || record
                .period_start
                .checked_add(record.period_length)
                .is_none()
        {
            return Err(unavailable);
        }
        Ok(record)
    }

    /// Returns the allowance still spendable in the current period.
    #[must_use]
    pub const fn remaining(&self) -> u128 {
        self.per_period_limit.saturating_sub(self.spent_this_period)
    }

    /// Returns the exclusive end sequence of the current period window.
    #[must_use]
    pub const fn window_end_sequence(&self) -> u64 {
        self.period_start.saturating_add(self.period_length)
    }
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
/// No public constructor exists; a value is issued only after the verified leaf
/// decoded as one canonical core budget record.
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

/// Verifies receipt identity and state inclusion, then rebuilds the local cache
/// from the decoded canonical budget record.
pub(crate) fn reconcile_state(
    local: &mut LocalAccounting,
    protocol: &ProtocolBudgetState,
    receipts: &[SpendReceiptEvidence],
    verifier: &EvidenceAuthority,
) -> Result<ReconciliationState, ReconcileError> {
    let mut replay = EvidenceAuthority::receipt_replay_guard();
    let mut last_verified_receipt = local.last_receipt;
    for receipt in receipts {
        let verified_receipt = verifier
            .verify_receipt(&receipt.evidence)
            .map_err(|_| ReconcileError::UnverifiedReceipt)?;
        if verified_receipt.activity_id() != receipt.expected_activity_id {
            return Err(ReconcileError::ReceiptActivityMismatch);
        }
        replay.admit(&verified_receipt).map_err(map_replay_error)?;
        last_verified_receipt = Some(verified_receipt.activity_id());
    }
    let verified = verifier
        .verify_state(&protocol.evidence)
        .map_err(|_| ReconcileError::UnverifiedProtocolState)?;
    let record = ProtocolBudgetRecord::decode(verified.canonical_state())?;
    let protocol_consumed = record.spent_this_period;
    let local_before = local.consumed;
    let divergence = if local_before == protocol_consumed {
        None
    } else if local_before > protocol_consumed {
        Some(i128::try_from(local_before - protocol_consumed).unwrap_or(i128::MAX))
    } else {
        Some(
            i128::try_from(protocol_consumed - local_before)
                .map_or(i128::MIN, |difference| -difference),
        )
    };
    let state = ReconciliationState {
        last_verified_receipt,
        protocol_consumed,
        local_before,
        local_after: protocol_consumed,
        divergence,
        window_start_sequence: record.period_start,
        window_end_sequence: record.window_end_sequence(),
        remaining: record.remaining(),
        observed_head_sequence: verified.observed_head_sequence(),
    };
    local.consumed = protocol_consumed;
    local.window_start_sequence = record.period_start;
    local.last_receipt = last_verified_receipt;
    Ok(state)
}

const fn map_replay_error(error: ReceiptReplayError) -> ReconcileError {
    match error {
        ReceiptReplayError::DuplicateReceipt => ReconcileError::DuplicateReceipt,
        ReceiptReplayError::DuplicateActivity => ReconcileError::DuplicateActivity,
    }
}
