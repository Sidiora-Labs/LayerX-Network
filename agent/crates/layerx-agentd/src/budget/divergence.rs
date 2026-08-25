//! Explicit reporting and conservative enforcement for budget divergence.

use super::accounting::ReconciliationState;

/// Health exposed while a budget divergence remains open.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct BudgetHealth {
    pub ready_for_writes: bool,
    pub divergence_open: bool,
}

/// Audit evidence retained for a detected local/protocol mismatch.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct DivergenceAuditRecord {
    pub local_consumed: u128,
    pub protocol_consumed: u128,
    pub last_verified_receipt: Option<[u8; 32]>,
    pub observed_head_sequence: u64,
    pub window_start_sequence: u64,
    pub window_end_sequence: u64,
}

/// Open divergence with the allowance safe to enforce until resolution.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct BudgetDivergenceAlert {
    pub audit: DivergenceAuditRecord,
    pub health: BudgetHealth,
    pub enforced_consumed: u128,
    pub enforced_remaining: u128,
}

/// Builds a visible alert and applies the more restrictive of the two figures.
pub(crate) fn build_alert(
    state: &ReconciliationState,
    local_ceiling: u128,
) -> Option<BudgetDivergenceAlert> {
    state.divergence()?;
    let enforced_consumed = state.local_before().max(state.protocol_consumed());
    let local_remaining = local_ceiling.saturating_sub(state.local_before());
    let enforced_remaining = state.remaining().min(local_remaining);
    Some(BudgetDivergenceAlert {
        audit: DivergenceAuditRecord {
            local_consumed: state.local_before(),
            protocol_consumed: state.protocol_consumed(),
            last_verified_receipt: state.last_verified_receipt(),
            observed_head_sequence: state.observed_head_sequence(),
            window_start_sequence: state.window_start_sequence(),
            window_end_sequence: state.window_end_sequence(),
        },
        health: BudgetHealth {
            ready_for_writes: false,
            divergence_open: true,
        },
        enforced_consumed,
        enforced_remaining,
    })
}
