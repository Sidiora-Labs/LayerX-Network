//! Native Budget ceilings retain indeterminate spends across timestamp expiry.

use super::{CeilingError, CeilingSnapshot};
use crate::budget::{NativeBudgetOutcome, NativeBudgetReconciliation};
use std::collections::{BTreeMap, BTreeSet};

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct NativeReservation {
    pub id: [u8; 32],
    pub expected_activity_id: [u8; 32],
    pub amount: u128,
    pub expiry_ms: u64,
    pub unknown: bool,
}

#[derive(Clone, Debug)]
pub struct NativeCeiling {
    maximum: u128,
    reconciliation: NativeBudgetReconciliation,
    reservations: BTreeMap<[u8; 32], NativeReservation>,
    settled: BTreeSet<[u8; 32]>,
}

impl NativeCeiling {
    /// # Errors
    /// Refuses duplicate identities, unmatched amounts, arithmetic overflow and excess held spend.
    pub fn rebuild(
        maximum: u128,
        reconciliation: NativeBudgetReconciliation,
        reservations: &[NativeReservation],
    ) -> Result<Self, CeilingError> {
        if maximum == 0 || reconciliation.spent() > maximum {
            return Err(CeilingError::Exceeded);
        }
        let mut result = Self {
            maximum,
            reconciliation,
            reservations: BTreeMap::new(),
            settled: BTreeSet::new(),
        };
        let mut activities = BTreeSet::new();
        for reservation in reservations {
            if reservation.id == [0; 32]
                || reservation.expected_activity_id == [0; 32]
                || reservation.amount == 0
                || reservation.expiry_ms == 0
            {
                return Err(CeilingError::InvalidIdentity);
            }
            if result.reservations.contains_key(&reservation.id)
                || result.settled.contains(&reservation.id)
            {
                return Err(CeilingError::Duplicate);
            }
            if !activities.insert(reservation.expected_activity_id) {
                return Err(CeilingError::DuplicateActivity);
            }
            if let Some(outcome) = result
                .reconciliation
                .outcomes()
                .iter()
                .find(|value| value.activity_id() == reservation.expected_activity_id)
            {
                if outcome.amount() != reservation.amount {
                    return Err(CeilingError::AmountMismatch);
                }
                result.settled.insert(reservation.id);
            } else {
                // A durable submission is held even when its wall-clock deadline has passed.
                result
                    .reservations
                    .insert(reservation.id, reservation.clone());
            }
        }
        result.require_capacity(0)?;
        Ok(result)
    }

    fn require_capacity(&self, amount: u128) -> Result<(), CeilingError> {
        let held = self
            .held()?
            .checked_add(amount)
            .ok_or(CeilingError::Overflow)?;
        if self
            .reconciliation
            .spent()
            .checked_add(held)
            .ok_or(CeilingError::Overflow)?
            > self.maximum
            || (self.reconciliation.write_eligible() && held > self.reconciliation.remaining())
        {
            return Err(CeilingError::Exceeded);
        }
        Ok(())
    }

    fn held(&self) -> Result<u128, CeilingError> {
        self.reservations
            .values()
            .try_fold(0_u128, |value, reservation| {
                value.checked_add(reservation.amount)
            })
            .ok_or(CeilingError::Overflow)
    }

    /// # Errors
    /// Refuses expired or regressed clocks, reused identities and checked ceiling exhaustion.
    pub fn reserve(
        &mut self,
        reservation: NativeReservation,
        current_ms: u64,
        current_sequence: u64,
    ) -> Result<(), CeilingError> {
        self.authorize_amount(
            reservation.amount,
            reservation.expiry_ms,
            current_ms,
            current_sequence,
        )?;
        if reservation.id == [0; 32] || reservation.expected_activity_id == [0; 32] {
            return Err(CeilingError::InvalidIdentity);
        }
        if self.reservations.contains_key(&reservation.id) || self.settled.contains(&reservation.id)
        {
            return Err(CeilingError::Duplicate);
        }
        if self
            .reservations
            .values()
            .any(|value| value.expected_activity_id == reservation.expected_activity_id)
            || self
                .reconciliation
                .outcomes()
                .iter()
                .any(|value| value.activity_id() == reservation.expected_activity_id)
        {
            return Err(CeilingError::DuplicateActivity);
        }
        self.reservations.insert(reservation.id, reservation);
        Ok(())
    }

    /// # Errors
    /// Refuses a signing request whose amount or timestamp cannot fit the reconciled ceiling.
    pub fn authorize_amount(
        &self,
        amount: u128,
        expiry_ms: u64,
        current_ms: u64,
        current_sequence: u64,
    ) -> Result<(), CeilingError> {
        if !self.reconciliation.write_eligible()
            || current_ms < self.reconciliation.timestamp_ms()
            || current_ms >= self.reconciliation.period_end_ms()
            || current_sequence != self.reconciliation.observed_sequence()
            || expiry_ms <= current_ms
            || expiry_ms > self.reconciliation.period_end_ms()
        {
            return Err(CeilingError::Expired);
        }
        if amount == 0 {
            return Err(CeilingError::ZeroAmount);
        }
        self.require_capacity(amount)
    }

    /// # Errors
    /// Refuses cancellation after dispatch or an absent reservation.
    pub fn cancel_unsubmitted(&mut self, id: [u8; 32]) -> Result<(), CeilingError> {
        if self
            .reservations
            .get(&id)
            .ok_or(CeilingError::MissingReservation)?
            .unknown
        {
            return Err(CeilingError::Indeterminate);
        }
        self.reservations.remove(&id);
        Ok(())
    }

    /// # Errors
    /// Refuses unknown identifiers; dispatch makes expiry insufficient to release a hold.
    pub fn mark_unknown(&mut self, id: [u8; 32]) -> Result<(), CeilingError> {
        self.reservations
            .get_mut(&id)
            .ok_or(CeilingError::MissingReservation)?
            .unknown = true;
        Ok(())
    }

    /// # Errors
    /// Refuses substituted authority, identity, period, regressed history or inconsistent outcomes.
    pub fn reconcile(
        &mut self,
        next: NativeBudgetReconciliation,
    ) -> Result<Vec<NativeBudgetOutcome>, CeilingError> {
        if !next.authenticates_owner_after(&self.reconciliation)
            || next.authority != self.reconciliation.authority
            || next.observed_sequence() < self.reconciliation.observed_sequence()
            || next.timestamp_ms() < self.reconciliation.timestamp_ms()
            || (next.binding.period_start_ms == self.reconciliation.binding.period_start_ms
                && next.spent() < self.reconciliation.spent())
        {
            return Err(CeilingError::Unreconciled);
        }
        let old: Vec<_> = self.reservations.values().cloned().collect();
        let mut rebuilt = Self::rebuild(self.maximum, next, &old)?;
        let outcomes = rebuilt
            .reconciliation
            .outcomes()
            .iter()
            .filter(|outcome| {
                old.iter()
                    .any(|reservation| reservation.expected_activity_id == outcome.activity_id())
            })
            .cloned()
            .collect();
        rebuilt.settled.extend(self.settled.iter());
        *self = rebuilt;
        Ok(outcomes)
    }

    /// # Errors
    /// Refuses arithmetic overflow in unresolved holds.
    pub fn snapshot(&self) -> Result<CeilingSnapshot, CeilingError> {
        Ok(CeilingSnapshot {
            maximum: self.maximum,
            consumed: self.reconciliation.spent(),
            held: self.held()?,
            reservations: self.reservations.len(),
            reconciled: true,
        })
    }

    #[must_use]
    pub fn reconciliation(&self) -> &NativeBudgetReconciliation {
        &self.reconciliation
    }
    #[must_use]
    pub fn reservation(&self, id: [u8; 32]) -> Option<&NativeReservation> {
        self.reservations.get(&id)
    }
}
