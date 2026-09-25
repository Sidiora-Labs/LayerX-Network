use crate::config::{ConfigError, StationConfig};
use crate::quote::Address;
use std::collections::BTreeMap;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PolicyRefusal {
    PerQuote,
    PerAccount,
    PerInterval,
    BalanceFloor,
    InvalidAmount,
    ClockRegression,
}
impl std::fmt::Display for PolicyRefusal {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "quote policy refused: {self:?}")
    }
}
impl std::error::Error for PolicyRefusal {}

pub struct QuotePolicy {
    config: StationConfig,
    interval: Option<u64>,
    last_time: u64,
    accounts: BTreeMap<Address, u128>,
    total: u128,
    reserved_pax: u128,
}
impl QuotePolicy {
    /// # Errors
    /// Refuses inconsistent policy configuration.
    pub fn new(config: &StationConfig) -> Result<Self, ConfigError> {
        config.validate()?;
        Ok(Self {
            config: config.clone(),
            interval: None,
            last_time: 0,
            accounts: BTreeMap::new(),
            total: 0,
            reserved_pax: 0,
        })
    }

    /// # Errors
    /// Names the budget or balance floor that would be exceeded. Failed checks consume no budget.
    pub fn reserve(
        &mut self,
        account: Address,
        amount: u128,
        gas_cost: u128,
        balance: u128,
        now: u64,
    ) -> Result<(), PolicyRefusal> {
        if amount == 0 || gas_cost == 0 || account == [0; 20] {
            return Err(PolicyRefusal::InvalidAmount);
        }
        if now < self.last_time {
            return Err(PolicyRefusal::ClockRegression);
        }
        let interval = now / self.config.interval_seconds;
        let same_interval = self.interval == Some(interval);
        let prior_account = if same_interval {
            self.accounts.get(&account).copied().unwrap_or(0)
        } else {
            0
        };
        let prior_total = if same_interval { self.total } else { 0 };
        if amount > self.config.per_quote_limit {
            return Err(PolicyRefusal::PerQuote);
        }
        let account_total = prior_account
            .checked_add(amount)
            .filter(|value| *value <= self.config.per_account_limit)
            .ok_or(PolicyRefusal::PerAccount)?;
        let total = prior_total
            .checked_add(amount)
            .filter(|value| *value <= self.config.per_interval_limit)
            .ok_or(PolicyRefusal::PerInterval)?;
        let reserved = self
            .reserved_pax
            .checked_add(gas_cost)
            .ok_or(PolicyRefusal::BalanceFloor)?;
        if balance
            .checked_sub(reserved)
            .is_none_or(|remaining| remaining < self.config.balance_floor)
        {
            return Err(PolicyRefusal::BalanceFloor);
        }
        if !same_interval {
            self.accounts.clear();
        }
        self.interval = Some(interval);
        self.last_time = now;
        self.accounts.insert(account, account_total);
        self.total = total;
        self.reserved_pax = reserved;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::tests::config;
    #[test]
    fn every_limit_and_atomic_refusal() -> Result<(), Box<dyn std::error::Error>> {
        let mut policy = QuotePolicy::new(&config())?;
        assert_eq!(
            policy.reserve([1; 20], 3_000_001, 1, 1000, 1000),
            Err(PolicyRefusal::PerQuote)
        );
        assert_eq!(policy.reserve([1; 20], 2_000_000, 1, 1000, 1000), Ok(()));
        assert_eq!(
            policy.reserve([1; 20], 2_000_001, 1, 1000, 1000),
            Err(PolicyRefusal::PerAccount)
        );
        assert_eq!(policy.reserve([1; 20], 2_000_000, 1, 1000, 1000), Ok(()));
        assert_eq!(policy.reserve([2; 20], 3_000_000, 1, 1000, 1000), Ok(()));
        assert_eq!(
            policy.reserve([3; 20], 1_000_001, 1, 1000, 1000),
            Err(PolicyRefusal::PerInterval)
        );
        assert_eq!(policy.reserve([3; 20], 1_000_000, 1, 1000, 1000), Ok(()));
        assert_eq!(policy.reserve([1; 20], 3_000_000, 1, 1000, 1060), Ok(()));
        assert_eq!(
            policy.reserve([4; 20], 1, 1, 1000, 1059),
            Err(PolicyRefusal::ClockRegression)
        );
        Ok(())
    }
    #[test]
    fn balance_floor_accounts_for_outstanding_promises() -> Result<(), Box<dyn std::error::Error>> {
        let mut policy = QuotePolicy::new(&config())?;
        assert_eq!(
            policy.reserve([1; 20], 1, 1, 99, 1000),
            Err(PolicyRefusal::BalanceFloor)
        );
        assert_eq!(
            policy.reserve([1; 20], 1, 1, 100, 1000),
            Err(PolicyRefusal::BalanceFloor)
        );
        assert_eq!(policy.reserve([1; 20], 1, 1, 101, 1000), Ok(()));
        assert_eq!(
            policy.reserve([1; 20], 1, 1, 101, 1060),
            Err(PolicyRefusal::BalanceFloor)
        );
        assert_eq!(
            policy.reserve([1; 20], 1, u128::MAX, u128::MAX, 1060),
            Err(PolicyRefusal::BalanceFloor)
        );
        assert_eq!(
            policy.reserve([1; 20], 0, 1, 1000, 1060),
            Err(PolicyRefusal::InvalidAmount)
        );
        Ok(())
    }
}
