//! Fail-closed restart recovery for durable submissions and spend accounting.

use crate::budget::{self, PersistedReceipt, ProtocolBudgetState, RestartAccounting, RestartError};
use crate::capability::{self, Ceiling, CeilingError, ReceiptApplication as CeilingReceipt};
use crate::protocol_evidence::EvidenceAuthority;
use crate::store::{ObjectKind, Store, StoreError, TenantId};

use super::{Outbox, OutboxError, SubmissionState};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct UnknownCeilingReservation {
    pub id: [u8; 32],
    pub expected_activity_id: [u8; 32],
    pub amount: u128,
    pub expiry_sequence: u64,
}

pub struct RecoveryInputs<'a> {
    pub verifier: EvidenceAuthority,
    pub unknown_budget_ids: &'a [[u8; 32]],
    pub budget_receipts: &'a [PersistedReceipt],
    pub protocol_budget: ProtocolBudgetState,
    pub ceiling_maximum: u128,
    pub ceiling_receipts: &'a [CeilingReceipt],
    pub unknown_ceiling_reservations: &'a [UnknownCeilingReservation],
    pub current_sequence: u64,
}

pub struct RecoveredOutbox {
    pub outbox: Outbox,
    pub queued_for_transmission: Vec<[u8; 32]>,
    pub awaiting_receipt_resolution: Vec<[u8; 32]>,
    pub budget_accounting: RestartAccounting,
    pub ceiling: Ceiling,
    recovery_complete: bool,
}

impl RecoveredOutbox {
    /// Admits writes only once recovery and every spend control have reconciled.
    ///
    /// # Errors
    ///
    /// Returns `WritesBlocked` while recovery is incomplete or budget or ceiling accounting is
    /// unreconciled, and `Ceiling` wrapping `Poisoned` or `Overflow` raised by the ceiling snapshot.
    pub fn require_write_ready(&self) -> Result<(), RecoveryError> {
        if !self.recovery_complete || !self.budget_accounting.reconciled {
            return Err(RecoveryError::WritesBlocked);
        }
        let snapshot = self.ceiling.snapshot().map_err(RecoveryError::Ceiling)?;
        if !snapshot.reconciled {
            return Err(RecoveryError::WritesBlocked);
        }
        Ok(())
    }
}

#[derive(Debug)]
pub enum RecoveryError {
    Outbox(OutboxError),
    Store(StoreError),
    Budget(RestartError),
    Ceiling(CeilingError),
    Corrupt,
    WritesBlocked,
}

impl From<OutboxError> for RecoveryError {
    fn from(value: OutboxError) -> Self {
        Self::Outbox(value)
    }
}

impl From<StoreError> for RecoveryError {
    fn from(value: StoreError) -> Self {
        Self::Store(value)
    }
}

/// Restores every durable outbox record and reconstructs spend controls before writes.
///
/// # Errors
///
/// Returns `Budget` from restart accounting, `Ceiling` when the ceiling cannot be rebuilt or an
/// unknown reservation cannot be re-held, `Corrupt` for an object identifier that is not 32 bytes
/// or a record restored as `Prepared` or `Signed`, and `Outbox` or `Store` from each restore.
pub fn recover(
    store: &mut Store,
    tenant: &TenantId,
    inputs: &RecoveryInputs<'_>,
) -> Result<RecoveredOutbox, RecoveryError> {
    let budget_accounting = budget::rebuild(
        store,
        tenant,
        inputs.unknown_budget_ids,
        inputs.budget_receipts,
        inputs.protocol_budget.clone(),
        &inputs.verifier,
    )
    .map_err(RecoveryError::Budget)?;
    let ceiling = Ceiling::rebuild(
        inputs.ceiling_maximum,
        inputs.verifier.clone(),
        inputs.ceiling_receipts,
    )
    .map_err(RecoveryError::Ceiling)?;
    for reservation in inputs.unknown_ceiling_reservations {
        capability::consume(
            &ceiling,
            reservation.id,
            reservation.expected_activity_id,
            reservation.amount,
            reservation.expiry_sequence,
            inputs.current_sequence,
        )
        .map_err(RecoveryError::Ceiling)?;
        ceiling
            .mark_unknown(reservation.id)
            .map_err(RecoveryError::Ceiling)?;
    }

    let mut outbox = Outbox::default();
    let mut queued_for_transmission = Vec::new();
    let mut awaiting_receipt_resolution = Vec::new();
    let identifiers = store.list_object_ids(tenant, ObjectKind::Outbox);
    for identifier in identifiers {
        let submission_id: [u8; 32] = identifier
            .as_slice()
            .try_into()
            .map_err(|_| RecoveryError::Corrupt)?;
        outbox.restore(store, tenant.clone(), submission_id)?;
        let state = outbox
            .status(submission_id)
            .map(|status| status.state)
            .ok_or(RecoveryError::Corrupt)?;
        match state {
            SubmissionState::Queued => queued_for_transmission.push(submission_id),
            SubmissionState::Submitted | SubmissionState::Acknowledged => {
                outbox.transition(
                    store,
                    submission_id,
                    SubmissionState::Unknown,
                    "restart made transport outcome indeterminate",
                    None,
                )?;
                awaiting_receipt_resolution.push(submission_id);
            }
            SubmissionState::Unknown => awaiting_receipt_resolution.push(submission_id),
            SubmissionState::Executed
            | SubmissionState::Failed
            | SubmissionState::Expired
            | SubmissionState::Superseded => {}
            SubmissionState::Prepared | SubmissionState::Signed => {
                return Err(RecoveryError::Corrupt);
            }
        }
    }
    queued_for_transmission.sort_unstable();
    awaiting_receipt_resolution.sort_unstable();
    Ok(RecoveredOutbox {
        outbox,
        queued_for_transmission,
        awaiting_receipt_resolution,
        budget_accounting,
        ceiling,
        recovery_complete: true,
    })
}
