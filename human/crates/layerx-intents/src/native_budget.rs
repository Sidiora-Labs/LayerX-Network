use layerx_types::account::AccountId;
use layerx_wire::{decode::Decoder, encode::Encoder};

use crate::{IntentError, IntentErrorReason, IntentField};

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct NativeBudgetCreate {
    pub budget_id: [u8; 32],
    pub budget_account: AccountId,
    pub asset: [u8; 32],
    pub purpose: [u8; 32],
    pub per_period_limit: u128,
    pub carry_cap: u128,
    pub initial_amount: u128,
    pub period_length_ms: u64,
    pub period_start_ms: u64,
    pub expiry_ms: u64,
    pub revocation_sequence: u64,
    pub rollover: u8,
    pub source_account: AccountId,
    pub source_sequence: u64,
}

impl NativeBudgetCreate {
    /// # Errors
    /// Refuses invalid native BudgetCreate fields or a noncanonical budget account.
    pub fn payload(&self) -> Result<Vec<u8>, IntentError> {
        let invalid_wire = |_| invalid();
        if self.budget_id == [0; 32] || self.asset == [0; 32] || self.purpose == [0; 32]
            || self.per_period_limit == 0 || self.initial_amount == 0
            || self.period_length_ms == 0
            || self.expiry_ms <= self.period_start_ms || !(1..=2).contains(&self.rollover)
            || (self.rollover == 1 && self.carry_cap != 0) || self.source_sequence == u64::MAX
        {
            return Err(invalid());
        }
        let source = self.source_account.as_str();
        let owner = source.strip_suffix(":main").or_else(|| source.strip_suffix(&format!(":asset:{}", hex(&self.asset))))
            .ok_or_else(invalid)?;
        if self.budget_account.as_str() != format!("{owner}:budget:{}", hex(&self.budget_id)) {
            return Err(invalid());
        }
        let mut encoded = Encoder::new(251);
        encoded.u16(2).map_err(invalid_wire)?;
        encoded.fixed(&self.budget_id).map_err(invalid_wire)?;
        encoded.fixed(&crate::canonical::account_id_for_protocol(&self.budget_account, 3).map_err(invalid_wire)?).map_err(invalid_wire)?;
        encoded.fixed(&self.asset).map_err(invalid_wire)?;
        encoded.fixed(&self.purpose).map_err(invalid_wire)?;
        encoded.u128(self.per_period_limit).map_err(invalid_wire)?;
        encoded.u128(self.carry_cap).map_err(invalid_wire)?;
        encoded.u128(self.initial_amount).map_err(invalid_wire)?;
        encoded.u64(self.period_length_ms).map_err(invalid_wire)?;
        encoded.u64(self.period_start_ms).map_err(invalid_wire)?;
        encoded.u64(self.expiry_ms).map_err(invalid_wire)?;
        encoded.u64(self.revocation_sequence).map_err(invalid_wire)?;
        encoded.u8(self.rollover).map_err(invalid_wire)?;
        encoded.fixed(&crate::canonical::account_id_for_protocol(&self.source_account, 3).map_err(invalid_wire)?).map_err(invalid_wire)?;
        encoded.u64(self.source_sequence).map_err(invalid_wire)?;
        Ok(encoded.finish())
    }

    /// # Errors
    /// Refuses any field differing from the original typed funding and account request.
    pub fn verify_payload(&self, bytes: &[u8]) -> Result<(), IntentError> {
        let wire = |_| invalid();
        let mut decoded = Decoder::new(bytes, 0);
        if decoded.u16().map_err(wire)? != 2
            || decoded.fixed(32).map_err(wire)? != self.budget_id
            || decoded.fixed(32).map_err(wire)? != crate::canonical::account_id_for_protocol(&self.budget_account, 3).map_err(wire)?
            || decoded.fixed(32).map_err(wire)? != self.asset
            || decoded.fixed(32).map_err(wire)? != self.purpose
            || decoded.u128().map_err(wire)? != self.per_period_limit
            || decoded.u128().map_err(wire)? != self.carry_cap
            || decoded.u128().map_err(wire)? != self.initial_amount
            || decoded.u64().map_err(wire)? != self.period_length_ms
            || decoded.u64().map_err(wire)? != self.period_start_ms
            || decoded.u64().map_err(wire)? != self.expiry_ms
            || decoded.u64().map_err(wire)? != self.revocation_sequence
            || decoded.u8().map_err(wire)? != self.rollover
            || decoded.fixed(32).map_err(wire)? != crate::canonical::account_id_for_protocol(&self.source_account, 3).map_err(wire)?
            || decoded.u64().map_err(wire)? != self.source_sequence
        { return Err(invalid()); }
        decoded.finish().map_err(wire)?;
        if self.payload()? != bytes { return Err(invalid()); }
        Ok(())
    }
}

fn invalid() -> IntentError {
    IntentError { field: IntentField::Budget, reason: IntentErrorReason::InvalidCanonicalEncoding }
}

fn hex(bytes: &[u8; 32]) -> String {
    const DIGITS: &[u8; 16] = b"0123456789abcdef";
    let mut value = String::with_capacity(64);
    for byte in bytes { value.push(char::from(DIGITS[usize::from(byte >> 4)])); value.push(char::from(DIGITS[usize::from(byte & 15)])); }
    value
}
