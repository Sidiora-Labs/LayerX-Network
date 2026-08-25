//! Serialised capability-ceiling reservations and receipt-only consumption.

use std::collections::BTreeMap;
use std::sync::Mutex;

use crate::protocol_evidence::{EvidenceAuthority, RawReceiptEvidence};

/// One held amount awaiting a verified terminal receipt.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Reservation {
    pub id: [u8; 32],
    pub amount: u128,
    pub expiry_sequence: u64,
    pub unknown: bool,
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct State {
    consumed: u128,
    reservations: BTreeMap<[u8; 32], Reservation>,
    reconciled: bool,
}

/// Thread-safe capability ceiling.
#[derive(Debug)]
pub struct Ceiling {
    maximum: u128,
    verifier: EvidenceAuthority,
    state: Mutex<State>,
}

impl Ceiling {
    #[must_use]
    pub fn new(maximum: u128, verifier: EvidenceAuthority) -> Self {
        Self {
            maximum,
            verifier,
            state: Mutex::new(State {
                consumed: 0,
                reservations: BTreeMap::new(),
                reconciled: true,
            }),
        }
    }

    /// Rebuilds consumed value from persisted verified receipts only.
    ///
    /// # Errors
    ///
    /// Returns `UnverifiedReceipt` for any unverified receipt, `Overflow` when the
    /// executed amounts do not sum, and `Exceeded` when the rebuilt total passes the
    /// ceiling.
    pub fn rebuild(
        maximum: u128,
        verifier: EvidenceAuthority,
        receipts: &[ReceiptApplication],
    ) -> Result<Self, CeilingError> {
        let mut consumed = 0_u128;
        for receipt in receipts {
            let verified = verifier
                .verify_receipt(&receipt.evidence)
                .map_err(|_| CeilingError::UnverifiedReceipt)?;
            if verified.result_code() == 0 {
                let amount = verified.amount();
                consumed = consumed.checked_add(amount).ok_or(CeilingError::Overflow)?;
            }
        }
        if consumed > maximum {
            return Err(CeilingError::Exceeded);
        }
        Ok(Self {
            maximum,
            verifier,
            state: Mutex::new(State {
                consumed,
                reservations: BTreeMap::new(),
                reconciled: true,
            }),
        })
    }

    /// Applies only a verified terminal receipt; failure consumes nothing.
    ///
    /// # Errors
    ///
    /// Returns `UnverifiedReceipt` for an unverified receipt, `MissingReservation`
    /// when no held reservation matches, `AmountMismatch` when the executed amount
    /// differs from the held amount, `Overflow` when consumption does not sum, and
    /// `Poisoned` when the state lock is poisoned.
    pub fn apply_receipt(&self, receipt: &ReceiptApplication) -> Result<(), CeilingError> {
        let verified = self
            .verifier
            .verify_receipt(&receipt.evidence)
            .map_err(|_| CeilingError::UnverifiedReceipt)?;
        let mut state = self.state.lock().map_err(|_| CeilingError::Poisoned)?;
        let reservation = state
            .reservations
            .get(&receipt.reservation_id)
            .ok_or(CeilingError::MissingReservation)?;
        let updated_consumed = if verified.result_code() == 0 {
            let amount = verified.amount();
            if amount != reservation.amount {
                return Err(CeilingError::AmountMismatch);
            }
            state
                .consumed
                .checked_add(amount)
                .ok_or(CeilingError::Overflow)?
        } else {
            state.consumed
        };
        state.reservations.remove(&receipt.reservation_id);
        state.consumed = updated_consumed;
        Ok(())
    }

    /// Cancels a reservation only while its activity is known not to have been submitted.
    ///
    /// # Errors
    ///
    /// Returns `MissingReservation` when the identifier is not held, `Indeterminate` when the
    /// reservation has crossed into unknown outcome state, and `Poisoned` for a poisoned lock.
    pub fn cancel_unsubmitted(&self, id: [u8; 32]) -> Result<(), CeilingError> {
        let mut state = self.state.lock().map_err(|_| CeilingError::Poisoned)?;
        if state
            .reservations
            .get(&id)
            .ok_or(CeilingError::MissingReservation)?
            .unknown
        {
            return Err(CeilingError::Indeterminate);
        }
        state.reservations.remove(&id);
        Ok(())
    }

    /// Marks an indeterminate outcome; it remains held across expiry.
    ///
    /// # Errors
    ///
    /// Returns `MissingReservation` when no reservation holds the identifier and
    /// `Poisoned` when the state lock is poisoned.
    pub fn mark_unknown(&self, id: [u8; 32]) -> Result<(), CeilingError> {
        let mut state = self.state.lock().map_err(|_| CeilingError::Poisoned)?;
        let reservation = state
            .reservations
            .get_mut(&id)
            .ok_or(CeilingError::MissingReservation)?;
        reservation.unknown = true;
        Ok(())
    }

    /// Releases only expired reservations whose outcome is not unknown.
    ///
    /// # Errors
    ///
    /// Returns `Poisoned` when the state lock is poisoned.
    pub fn release_expired(&self, current_sequence: u64) -> Result<usize, CeilingError> {
        let mut state = self.state.lock().map_err(|_| CeilingError::Poisoned)?;
        let before = state.reservations.len();
        state
            .reservations
            .retain(|_, value| value.unknown || current_sequence < value.expiry_sequence);
        Ok(before - state.reservations.len())
    }

    /// Returns the ceiling totals observed under one lock acquisition.
    ///
    /// # Errors
    ///
    /// Returns `Overflow` when the held amounts do not sum and `Poisoned` when the
    /// state lock is poisoned.
    pub fn snapshot(&self) -> Result<CeilingSnapshot, CeilingError> {
        let state = self.state.lock().map_err(|_| CeilingError::Poisoned)?;
        let held = state
            .reservations
            .values()
            .try_fold(0_u128, |total, value| total.checked_add(value.amount))
            .ok_or(CeilingError::Overflow)?;
        Ok(CeilingSnapshot {
            maximum: self.maximum,
            consumed: state.consumed,
            held,
            reservations: state.reservations.len(),
            reconciled: state.reconciled,
        })
    }
}

/// Raw boundary receipt paired with the reservation it is expected to settle.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ReceiptApplication {
    pub reservation_id: [u8; 32],
    pub evidence: RawReceiptEvidence,
}

/// Atomic ceiling snapshot.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct CeilingSnapshot {
    pub maximum: u128,
    pub consumed: u128,
    pub held: u128,
    pub reservations: usize,
    pub reconciled: bool,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CeilingError {
    ZeroAmount,
    Expired,
    Duplicate,
    Exceeded,
    Unreconciled,
    UnverifiedReceipt,
    MissingReservation,
    Indeterminate,
    AmountMismatch,
    Overflow,
    Poisoned,
}

pub(crate) fn reserve(
    ceiling: &Ceiling,
    reservation_id: [u8; 32],
    amount: u128,
    expiry_sequence: u64,
    current_sequence: u64,
) -> Result<Reservation, CeilingError> {
    if amount == 0 {
        return Err(CeilingError::ZeroAmount);
    }
    if expiry_sequence <= current_sequence {
        return Err(CeilingError::Expired);
    }
    let mut state = ceiling.state.lock().map_err(|_| CeilingError::Poisoned)?;
    if !state.reconciled {
        return Err(CeilingError::Unreconciled);
    }
    if state.reservations.contains_key(&reservation_id) {
        return Err(CeilingError::Duplicate);
    }
    let held = state
        .reservations
        .values()
        .try_fold(0_u128, |total, value| total.checked_add(value.amount))
        .ok_or(CeilingError::Overflow)?;
    let projected = state
        .consumed
        .checked_add(held)
        .and_then(|value| value.checked_add(amount))
        .ok_or(CeilingError::Overflow)?;
    if projected > ceiling.maximum {
        return Err(CeilingError::Exceeded);
    }
    let reservation = Reservation {
        id: reservation_id,
        amount,
        expiry_sequence,
        unknown: false,
    };
    state
        .reservations
        .insert(reservation_id, reservation.clone());
    Ok(reservation)
}
