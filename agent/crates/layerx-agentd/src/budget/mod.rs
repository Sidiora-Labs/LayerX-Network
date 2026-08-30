//! Protocol-backed and explicitly local spending limits.

#[path = "reconcile.rs"]
mod accounting;
mod create;
#[path = "divergence.rs"]
mod divergence_reporting;
#[path = "hold.rs"]
mod recovery;
#[path = "reserve.rs"]
mod reservations;

pub use accounting::{
    LocalAccounting, ProtocolBudgetState, ReconcileError, ReconciliationState, SpendReceiptEvidence,
};
pub use create::{
    create_protocol_budget, BudgetCreationError, BudgetKind, BudgetPipeline, BudgetRequest,
    CoreBudgetReceipt, LocalLimit, ProtocolBudget,
};
pub use divergence_reporting::{BudgetDivergenceAlert, BudgetHealth, DivergenceAuditRecord};
pub use recovery::{
    PersistedReceipt, RestartAccounting, RestartError, UnknownOutcome, UnknownReservation,
};
pub use reservations::{
    BudgetLimiter, BudgetReservation, DurableBudgetReservation, LimitConfig, LimitId, LimitRefusal,
    LimitScope, ReleaseKind, ReservationRequest,
};

/// Reconciles local budget cache state against verified protocol evidence.
///
/// # Errors
///
/// Returns `ProtocolStateSchemaUnavailable` after authenticating the included
/// candidate state because core defines no canonical budget record/key schema.
/// Receipt verification, activity binding, and replay rejection happen before
/// that fail-closed result.
pub fn reconcile(
    local: &mut LocalAccounting,
    protocol: &ProtocolBudgetState,
    receipts: &[SpendReceiptEvidence],
    verifier: &crate::protocol_evidence::EvidenceAuthority,
) -> Result<ReconciliationState, ReconcileError> {
    accounting::reconcile_state(local, protocol, receipts, verifier)
}

/// Atomically reserves against every applicable scope.
///
/// # Errors
///
/// Returns `InvalidRequest` for a zero amount, an expiry at or before the current sequence or
/// an empty limit list, `UnknownLimit` for an unconfigured scope, `Exceeded` when consumed plus
/// held plus the request passes a ceiling, `Arithmetic` on overflow, or `Poisoned`.
pub fn reserve(
    limiter: &BudgetLimiter,
    request: &ReservationRequest,
) -> Result<BudgetReservation, LimitRefusal> {
    reservations::reserve_all(limiter, request)
}

/// Deterministically releases or consumes one reservation.
///
/// # Errors
///
/// Returns `Arithmetic` when an `Executed` release overflows a limit's consumed total, or
/// `Poisoned` when the limiter lock is poisoned; a reservation no limit holds is `Ok(false)`.
pub fn release(
    limiter: &BudgetLimiter,
    reservation_id: [u8; 32],
    kind: ReleaseKind,
    current_sequence: u64,
) -> Result<bool, LimitRefusal> {
    reservations::release_all(limiter, reservation_id, kind, current_sequence)
}

/// Restores canonical durable reservations before the limiter is made ready.
pub fn restore(
    limiter: &BudgetLimiter,
    reservations: &[DurableBudgetReservation],
) -> Result<(), LimitRefusal> {
    reservations::restore_all(limiter, reservations)
}

/// Persists a reservation whose submission outcome is unknown.
///
/// # Errors
///
/// Returns `RestartError::Store` for the I/O or `SizeOverflow` failure raised while the store
/// is written to disk; the in-memory entry is rolled back so nothing is half-persisted.
pub fn hold_unknown(
    store: &mut crate::store::Store,
    reservation: &UnknownReservation,
) -> Result<(), RestartError> {
    recovery::persist_unknown(store, reservation)
}

/// Rebuilds held and consumed accounting before writes are admitted.
///
/// # Errors
///
/// Returns typed verification, activity-identity, and replay failures before
/// rebuilding receipt consumption. The resulting accounting remains blocked by
/// `ProtocolStateSchemaUnavailable` because core defines no canonical budget
/// record/key schema. Returns `Corrupt` for malformed reservations, `Arithmetic`
/// on overflow, and `Store` for storage failures.
pub fn rebuild(
    store: &crate::store::Store,
    tenant: &crate::store::TenantId,
    unknown_ids: &[[u8; 32]],
    receipts: &[PersistedReceipt],
    protocol: &ProtocolBudgetState,
    verifier: &crate::protocol_evidence::EvidenceAuthority,
) -> Result<RestartAccounting, RestartError> {
    recovery::rebuild_accounting(store, tenant, unknown_ids, receipts, protocol, verifier)
}

/// Raises an explicit alert for a local/protocol mismatch.
#[must_use]
pub fn divergence_alert(
    state: &ReconciliationState,
    local_ceiling: u128,
) -> Option<BudgetDivergenceAlert> {
    divergence_reporting::build_alert(state, local_ceiling)
}
