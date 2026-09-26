use serde_json::json;

use crate::config::{ConfigError, StationConfig};
use crate::quote::{keccak, Address, Word};
use crate::rpc::{bytes, hex, read, JsonRpc, RpcFault};

pub const PAX_BASE_UNITS: u128 = 1_000_000_000_000_000_000;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PriceError {
    Unavailable,
    Malformed,
    MissingRate,
    InvalidRate,
    StaleRate,
    OutsideSpread,
    Overflow,
    ZeroGas,
}
impl std::fmt::Display for PriceError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "governed rate refused: {self:?}")
    }
}
impl std::error::Error for PriceError {}

/// The paymaster's governed rate: Sidiora base units per whole Paxeer coin,
/// and the chain time at which that rate was set.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct GovernedRate {
    pub rate: u128,
    pub updated_at: u64,
}

pub trait PriceSource {
    /// # Errors
    /// Refuses an unavailable, malformed, missing or stale governed rate; never substitutes one.
    fn governed_rate(&self) -> Result<GovernedRate, PriceError>;
}

pub struct PaymasterRateSource<R> {
    rpc: R,
    paymaster: Address,
}
impl<R: JsonRpc> PaymasterRateSource<R> {
    #[must_use]
    pub const fn new(rpc: R, paymaster: Address) -> Self {
        Self { rpc, paymaster }
    }

    fn call(&self, signature: &[u8]) -> Result<Word, RpcFault> {
        let result: String = read(
            &self.rpc,
            "eth_call",
            json!([{"to":hex(&self.paymaster),"data":hex(&keccak(signature)[..4])},"latest"]),
        )?;
        bytes(&result)?.try_into().map_err(|_| RpcFault::Malformed)
    }
}
impl<R: JsonRpc> PriceSource for PaymasterRateSource<R> {
    fn governed_rate(&self) -> Result<GovernedRate, PriceError> {
        let updated_at = match self.call(b"rateUpdatedAt()") {
            Ok(value) => uint(value).ok_or(PriceError::Malformed)?,
            Err(RpcFault::Rejected { .. }) => return Err(PriceError::MissingRate),
            Err(fault) => return Err(transport(fault)),
        };
        let updated_at = u64::try_from(updated_at).map_err(|_| PriceError::Malformed)?;
        match self.call(b"currentRate()") {
            Ok(value) => Ok(GovernedRate {
                rate: uint(value).ok_or(PriceError::Overflow)?,
                updated_at,
            }),
            Err(RpcFault::Rejected { .. }) if updated_at == 0 => Err(PriceError::MissingRate),
            Err(RpcFault::Rejected { .. }) => Err(PriceError::StaleRate),
            Err(fault) => Err(transport(fault)),
        }
    }
}

fn transport(fault: RpcFault) -> PriceError {
    if fault == RpcFault::Malformed {
        PriceError::Malformed
    } else {
        PriceError::Unavailable
    }
}

fn uint(value: Word) -> Option<u128> {
    if value[..16] != [0; 16] {
        return None;
    }
    Some(u128::from_be_bytes(value[16..].try_into().ok()?))
}

fn ceil_div(value: u128, divisor: u128) -> Result<u128, PriceError> {
    (value / divisor)
        .checked_add(u128::from(!value.is_multiple_of(divisor)))
        .ok_or(PriceError::Overflow)
}

#[derive(Clone, Debug)]
pub struct Pricing {
    config: StationConfig,
}
impl Pricing {
    /// # Errors
    /// Refuses policy that exceeds the paymaster's rate constraints.
    pub fn new(config: &StationConfig) -> Result<Self, ConfigError> {
        config.validate()?;
        Ok(Self {
            config: config.clone(),
        })
    }

    /// # Errors
    /// Refuses missing, invalid, stale or overflowing rates and a margin outside the spread.
    pub fn quote(&self, rate: &GovernedRate, gas_cost: u128, now: u64) -> Result<u128, PriceError> {
        let expected = self.expected(rate, gas_cost, now)?;
        let amount = ceil_div(
            expected
                .checked_mul(10_000 + u128::from(self.config.margin_bps))
                .ok_or(PriceError::Overflow)?,
            10_000,
        )?;
        self.check_spread(expected, amount)?;
        Ok(amount)
    }

    /// # Errors
    /// Refuses an implied rate outside the configured spread or an unusable governed rate.
    pub fn validate_amount(
        &self,
        rate: &GovernedRate,
        gas_cost: u128,
        amount: u128,
        now: u64,
    ) -> Result<(), PriceError> {
        self.check_spread(self.expected(rate, gas_cost, now)?, amount)
    }

    fn expected(&self, rate: &GovernedRate, gas_cost: u128, now: u64) -> Result<u128, PriceError> {
        if gas_cost == 0 {
            return Err(PriceError::ZeroGas);
        }
        if rate.rate == 0 {
            return Err(PriceError::InvalidRate);
        }
        if rate.updated_at == 0 {
            return Err(PriceError::MissingRate);
        }
        if rate.updated_at > now || now - rate.updated_at > self.config.max_rate_age {
            return Err(PriceError::StaleRate);
        }
        ceil_div(
            gas_cost
                .checked_mul(rate.rate)
                .ok_or(PriceError::Overflow)?,
            PAX_BASE_UNITS,
        )
    }

    fn check_spread(&self, expected: u128, amount: u128) -> Result<(), PriceError> {
        let spread = u128::from(self.config.spread_bps);
        let lower = ceil_div(
            expected
                .checked_mul(10_000 - spread)
                .ok_or(PriceError::Overflow)?,
            10_000,
        )?;
        let upper = expected
            .checked_mul(10_000 + spread)
            .ok_or(PriceError::Overflow)?
            / 10_000;
        if amount < lower || amount > upper {
            return Err(PriceError::OutsideSpread);
        }
        Ok(())
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use crate::config::tests::config;
    use crate::rpc::{ConfiguredRpc, Exchange};
    use serde_json::Value;

    pub(crate) const RATE: GovernedRate = GovernedRate {
        rate: 3_114_000,
        updated_at: 1000,
    };

    struct RecordedPaymaster {
        paymaster: Address,
        responses: Value,
    }
    impl Exchange for RecordedPaymaster {
        fn request(&self, _: &str, method: &str, params: &Value) -> Result<Value, RpcFault> {
            assert_eq!(method, "eth_call");
            assert_eq!(params[0]["to"], hex(&self.paymaster));
            assert_eq!(params[1], "latest");
            let name = ["currentRate()", "rateUpdatedAt()"]
                .into_iter()
                .find(|name| params[0]["data"] == hex(&keccak(name.as_bytes())[..4]))
                .ok_or(RpcFault::Malformed)?;
            let response = &self.responses[name];
            if response["fault"] == "unavailable" {
                return Err(RpcFault::Unavailable);
            }
            if let Some(code) = response["error"]["code"].as_i64() {
                return Err(RpcFault::Rejected { code });
            }
            Ok(response["result"].clone())
        }
    }
    fn source(case: &str) -> Result<PaymasterRateSource<impl JsonRpc>, Box<dyn std::error::Error>> {
        let recorded: Value =
            serde_json::from_str(include_str!("../tests/fixtures/paymaster_rate.json"))?;
        let config = config();
        let rpc = ConfiguredRpc::new(
            &config,
            RecordedPaymaster {
                paymaster: config.paymaster,
                responses: recorded[case].clone(),
            },
        )?;
        Ok(PaymasterRateSource::new(rpc, config.paymaster))
    }

    #[test]
    fn paymaster_rate_read_from_current_rate_and_update_time(
    ) -> Result<(), Box<dyn std::error::Error>> {
        assert_eq!(source("fresh")?.governed_rate(), Ok(RATE));
        Ok(())
    }

    #[test]
    fn paymaster_revert_reads_as_stale_or_missing() -> Result<(), Box<dyn std::error::Error>> {
        assert_eq!(source("stale")?.governed_rate(), Err(PriceError::StaleRate));
        assert_eq!(
            source("missing")?.governed_rate(),
            Err(PriceError::MissingRate)
        );
        Ok(())
    }

    #[test]
    fn paymaster_unreadable_responses_refused() -> Result<(), Box<dyn std::error::Error>> {
        assert_eq!(
            source("unavailable")?.governed_rate(),
            Err(PriceError::Unavailable)
        );
        assert_eq!(source("short")?.governed_rate(), Err(PriceError::Malformed));
        assert_eq!(source("wide")?.governed_rate(), Err(PriceError::Overflow));
        Ok(())
    }

    #[test]
    fn pricing_from_governed_rate() -> Result<(), Box<dyn std::error::Error>> {
        let pricing = Pricing::new(&config())?;
        assert_eq!(pricing.quote(&RATE, PAX_BASE_UNITS, 1000), Ok(3_145_140));
        let mut aged = RATE;
        aged.updated_at = 700;
        assert_eq!(pricing.quote(&aged, PAX_BASE_UNITS, 1000), Ok(3_145_140));
        let mut cfg = config();
        cfg.margin_bps = 0;
        let unit = GovernedRate {
            rate: 1,
            updated_at: 1000,
        };
        assert_eq!(Pricing::new(&cfg)?.quote(&unit, 1, 1000), Ok(1));
        Ok(())
    }

    #[test]
    fn missing_invalid_and_stale_rates_refused() -> Result<(), Box<dyn std::error::Error>> {
        let pricing = Pricing::new(&config())?;
        let mut missing = RATE;
        missing.updated_at = 0;
        assert_eq!(
            pricing.quote(&missing, 1, 1000),
            Err(PriceError::MissingRate)
        );
        let mut invalid = RATE;
        invalid.rate = 0;
        assert_eq!(
            pricing.quote(&invalid, 1, 1000),
            Err(PriceError::InvalidRate)
        );
        for updated_at in [699, 1001] {
            let mut stale = RATE;
            stale.updated_at = updated_at;
            assert_eq!(pricing.quote(&stale, 1, 1000), Err(PriceError::StaleRate));
        }
        Ok(())
    }

    #[test]
    fn zero_gas_and_overflow_refused() -> Result<(), Box<dyn std::error::Error>> {
        let pricing = Pricing::new(&config())?;
        assert_eq!(pricing.quote(&RATE, 0, 1000), Err(PriceError::ZeroGas));
        assert_eq!(
            pricing.quote(&RATE, u128::MAX, 1000),
            Err(PriceError::Overflow)
        );
        Ok(())
    }

    #[test]
    fn amounts_outside_spread_refused() -> Result<(), Box<dyn std::error::Error>> {
        let pricing = Pricing::new(&config())?;
        for amount in [2_958_299, 3_269_701] {
            assert_eq!(
                pricing.validate_amount(&RATE, PAX_BASE_UNITS, amount, 1000),
                Err(PriceError::OutsideSpread)
            );
        }
        for amount in [2_958_300, 3_269_700] {
            assert_eq!(
                pricing.validate_amount(&RATE, PAX_BASE_UNITS, amount, 1000),
                Ok(())
            );
        }
        let unit = GovernedRate {
            rate: 1,
            updated_at: 1000,
        };
        assert_eq!(
            pricing.quote(&unit, 1, 1000),
            Err(PriceError::OutsideSpread)
        );
        Ok(())
    }
}
