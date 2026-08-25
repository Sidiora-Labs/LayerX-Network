//! Durable unknown reservations and fail-closed restart accounting.

use crate::protocol_evidence::{verify_receipt, RawReceiptEvidence};
use crate::store::{ObjectKind, Store, StoreError, TenantId, TenantKey};

use super::accounting::verify_protocol_budget_state;
use super::ProtocolBudgetState;

/// Reservation kept unavailable until a receipt resolves it.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct UnknownReservation {
    pub tenant: TenantId,
    pub id: [u8; 32],
    pub amount: u128,
    pub expiry_sequence: u64,
    pub resolved: Option<UnknownOutcome>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum UnknownOutcome {
    Executed,
    Failed,
}

/// Persisted verified receipt used to rebuild consumed accounting.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PersistedReceipt {
    pub evidence: RawReceiptEvidence,
}

/// Operator-visible restart accounting.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct RestartAccounting {
    pub protocol_consumed: u128,
    pub receipt_consumed: u128,
    pub held_unresolved: u128,
    pub unresolved_count: usize,
    pub reconciled: bool,
}

impl RestartAccounting {
    /// Refuses write admission until protocol and receipt accounting reconcile.
    ///
    /// # Errors
    ///
    /// Returns `RestartError::Unreconciled` whenever rebuilt receipt consumption disagrees
    /// with the protocol-reported consumed amount.
    pub fn require_write_ready(self) -> Result<(), RestartError> {
        if self.reconciled {
            Ok(())
        } else {
            Err(RestartError::Unreconciled)
        }
    }
}

#[derive(Debug)]
pub enum RestartError {
    Store(StoreError),
    Corrupt,
    UnverifiedProtocol,
    UnverifiedReceipt,
    Unreconciled,
    Arithmetic,
}

impl From<StoreError> for RestartError {
    fn from(value: StoreError) -> Self {
        Self::Store(value)
    }
}

pub(crate) fn persist_unknown(
    store: &mut Store,
    reservation: &UnknownReservation,
) -> Result<(), RestartError> {
    let key = key(reservation.tenant.clone(), reservation.id)?;
    store.put_local(key, encode(reservation))?;
    Ok(())
}

pub(crate) fn rebuild_accounting(
    store: &Store,
    tenant: &TenantId,
    unknown_ids: &[[u8; 32]],
    receipts: &[PersistedReceipt],
    protocol: ProtocolBudgetState,
) -> Result<RestartAccounting, RestartError> {
    let protocol = verify_protocol_budget_state(&protocol)
        .map_err(|_| RestartError::UnverifiedProtocol)?;
    let mut receipt_consumed = 0_u128;
    for receipt in receipts {
        let verified = verify_receipt(&receipt.evidence)
            .map_err(|_| RestartError::UnverifiedReceipt)?;
        if verified.global_sequence() < protocol.window_start_sequence
            || verified.global_sequence() > protocol.window_end_sequence
            || verified.global_sequence() > protocol.observed_head_sequence
        {
            return Err(RestartError::UnverifiedReceipt);
        }
        if verified.result_code() == 0 {
            receipt_consumed = receipt_consumed
                .checked_add(verified.amount())
                .ok_or(RestartError::Arithmetic)?;
        }
    }
    let mut held_unresolved = 0_u128;
    let mut unresolved_count = 0_usize;
    for id in unknown_ids {
        let storage_key = key(tenant.clone(), *id)?;
        let Some(value) = store.get(&storage_key) else {
            return Err(RestartError::Corrupt);
        };
        let reservation = decode(tenant, *id, value.bytes())?;
        if reservation.resolved.is_none() {
            held_unresolved = held_unresolved
                .checked_add(reservation.amount)
                .ok_or(RestartError::Arithmetic)?;
            unresolved_count += 1;
        }
    }
    Ok(RestartAccounting {
        protocol_consumed: protocol.consumed,
        receipt_consumed,
        held_unresolved,
        unresolved_count,
        reconciled: receipt_consumed == protocol.consumed,
    })
}

fn key(tenant: TenantId, id: [u8; 32]) -> Result<TenantKey, RestartError> {
    let mut object_id = b"unknown-budget:".to_vec();
    object_id.extend_from_slice(&id);
    Ok(TenantKey::new(tenant, ObjectKind::Budget, object_id)?)
}

fn encode(reservation: &UnknownReservation) -> Vec<u8> {
    let mut bytes = Vec::new();
    bytes.extend_from_slice(&reservation.amount.to_be_bytes());
    bytes.extend_from_slice(&reservation.expiry_sequence.to_be_bytes());
    bytes.push(match reservation.resolved {
        None => 0,
        Some(UnknownOutcome::Executed) => 1,
        Some(UnknownOutcome::Failed) => 2,
    });
    bytes
}

fn decode(
    tenant: &TenantId,
    id: [u8; 32],
    bytes: &[u8],
) -> Result<UnknownReservation, RestartError> {
    if bytes.len() != 25 {
        return Err(RestartError::Corrupt);
    }
    let mut amount = [0_u8; 16];
    amount.copy_from_slice(&bytes[..16]);
    let mut expiry = [0_u8; 8];
    expiry.copy_from_slice(&bytes[16..24]);
    let resolved = match bytes[24] {
        0 => None,
        1 => Some(UnknownOutcome::Executed),
        2 => Some(UnknownOutcome::Failed),
        _ => return Err(RestartError::Corrupt),
    };
    Ok(UnknownReservation {
        tenant: tenant.clone(),
        id,
        amount: u128::from_be_bytes(amount),
        expiry_sequence: u64::from_be_bytes(expiry),
        resolved,
    })
}
