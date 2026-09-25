use crate::config::{ConfigError, StationConfig};
use crate::quote::{keccak, Address};

pub const ORACLE: Address = [
    0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0x10, 0x08,
];

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
        write!(f, "oracle price refused: {self:?}")
    }
}
impl std::error::Error for PriceError {}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct OracleRate {
    pub denom: String,
    pub exchange_rate: String,
    pub last_update: String,
    pub last_update_timestamp: i64,
}

pub trait OracleTransport {
    /// # Errors
    /// Returns an unavailable or malformed response; never substitutes a rate.
    fn eth_call(&self, address: Address, calldata: &[u8]) -> Result<Vec<u8>, PriceError>;
}

pub trait PriceSource {
    /// # Errors
    /// Refuses unavailable or malformed oracle data.
    fn exchange_rates(&self) -> Result<Vec<OracleRate>, PriceError>;
}

pub struct OraclePriceSource<T> {
    transport: T,
}
impl<T: OracleTransport> OraclePriceSource<T> {
    #[must_use]
    pub const fn new(transport: T) -> Self {
        Self { transport }
    }
}
impl<T: OracleTransport> PriceSource for OraclePriceSource<T> {
    fn exchange_rates(&self) -> Result<Vec<OracleRate>, PriceError> {
        let selector = keccak(b"getExchangeRates()");
        decode_exchange_rates(&self.transport.eth_call(ORACLE, &selector[..4])?)
    }
}

fn abi_word(bytes: &[u8], start: usize) -> Result<&[u8], PriceError> {
    bytes
        .get(start..start.checked_add(32).ok_or(PriceError::Malformed)?)
        .ok_or(PriceError::Malformed)
}
fn abi_offset(bytes: &[u8], start: usize) -> Result<usize, PriceError> {
    let raw = abi_word(bytes, start)?;
    if raw[..24].iter().any(|b| *b != 0) {
        return Err(PriceError::Malformed);
    }
    let mut value = [0; 8];
    value.copy_from_slice(&raw[24..]);
    usize::try_from(u64::from_be_bytes(value)).map_err(|_| PriceError::Malformed)
}
fn offset(base: usize, relative: usize) -> Result<usize, PriceError> {
    if !relative.is_multiple_of(32) {
        return Err(PriceError::Malformed);
    }
    base.checked_add(relative).ok_or(PriceError::Malformed)
}
fn abi_string(bytes: &[u8], base: usize, head: usize) -> Result<String, PriceError> {
    let start = offset(base, abi_offset(bytes, head)?)?;
    let length = abi_offset(bytes, start)?;
    let data = start.checked_add(32).ok_or(PriceError::Malformed)?;
    let end = data.checked_add(length).ok_or(PriceError::Malformed)?;
    let raw = bytes.get(data..end).ok_or(PriceError::Malformed)?;
    String::from_utf8(raw.to_vec()).map_err(|_| PriceError::Malformed)
}

/// # Errors
/// Refuses invalid ABI offsets, lengths, strings and signed timestamps.
pub fn decode_exchange_rates(bytes: &[u8]) -> Result<Vec<OracleRate>, PriceError> {
    if bytes.len() > 1_048_576 || !bytes.len().is_multiple_of(32) || abi_offset(bytes, 0)? != 32 {
        return Err(PriceError::Malformed);
    }
    let count = abi_offset(bytes, 32)?;
    if count > bytes.len() / 32 {
        return Err(PriceError::Malformed);
    }
    let mut rates = Vec::with_capacity(count);
    for index in 0..count {
        let pair = offset(64, abi_offset(bytes, 64 + index * 32)?)?;
        let denom = abi_string(bytes, pair, pair)?;
        let rate = offset(
            pair,
            abi_offset(bytes, pair.checked_add(32).ok_or(PriceError::Malformed)?)?,
        )?;
        let raw = abi_word(bytes, rate.checked_add(64).ok_or(PriceError::Malformed)?)?;
        let mut timestamp = [0; 8];
        timestamp.copy_from_slice(&raw[24..]);
        let timestamp = i64::from_be_bytes(timestamp);
        let extension = if timestamp < 0 { 255 } else { 0 };
        if raw[..24].iter().any(|byte| *byte != extension) {
            return Err(PriceError::Malformed);
        }
        rates.push(OracleRate {
            denom,
            exchange_rate: abi_string(bytes, rate, rate)?,
            last_update: abi_string(
                bytes,
                rate,
                rate.checked_add(32).ok_or(PriceError::Malformed)?,
            )?,
            last_update_timestamp: timestamp,
        });
    }
    Ok(rates)
}

fn decimal(raw: &str) -> Result<u128, PriceError> {
    if raw.is_empty() || raw.starts_with('.') || raw.ends_with('.') {
        return Err(PriceError::InvalidRate);
    }
    let mut value = 0_u128;
    let mut fraction = None;
    for byte in raw.bytes() {
        if byte == b'.' && fraction.is_none() {
            fraction = Some(0_u32);
            continue;
        }
        if !byte.is_ascii_digit() {
            return Err(PriceError::InvalidRate);
        }
        if let Some(count) = fraction.as_mut() {
            *count += 1;
            if *count > 18 {
                return Err(PriceError::InvalidRate);
            }
        }
        value = value
            .checked_mul(10)
            .and_then(|v| v.checked_add(u128::from(byte - b'0')))
            .ok_or(PriceError::Overflow)?;
    }
    value = value
        .checked_mul(10_u128.pow(18 - fraction.unwrap_or(0)))
        .ok_or(PriceError::Overflow)?;
    if value == 0 {
        return Err(PriceError::InvalidRate);
    }
    Ok(value)
}

fn rate(rates: &[OracleRate], denom: &str, now: u64, max_age: u64) -> Result<u128, PriceError> {
    let mut found = rates.iter().filter(|rate| rate.denom == denom);
    let rate = found.next().ok_or(PriceError::MissingRate)?;
    if found.next().is_some() {
        return Err(PriceError::InvalidRate);
    }
    let timestamp = u64::try_from(rate.last_update_timestamp).map_err(|_| PriceError::StaleRate)?;
    if timestamp == 0 || timestamp > now || now - timestamp > max_age {
        return Err(PriceError::StaleRate);
    }
    decimal(&rate.exchange_rate)
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
    /// Refuses policy that exceeds the paymaster's oracle constraints.
    pub fn new(config: &StationConfig) -> Result<Self, ConfigError> {
        config.validate()?;
        Ok(Self {
            config: config.clone(),
        })
    }

    /// # Errors
    /// Refuses missing, invalid, stale or overflowing prices and a margin outside the spread.
    pub fn quote(
        &self,
        rates: &[OracleRate],
        gas_cost: u128,
        now: u64,
    ) -> Result<u128, PriceError> {
        let expected = self.expected(rates, gas_cost, now)?;
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
    /// Refuses an implied rate outside the configured spread or invalid oracle data.
    pub fn validate_amount(
        &self,
        rates: &[OracleRate],
        gas_cost: u128,
        amount: u128,
        now: u64,
    ) -> Result<(), PriceError> {
        self.check_spread(self.expected(rates, gas_cost, now)?, amount)
    }

    fn expected(&self, rates: &[OracleRate], gas_cost: u128, now: u64) -> Result<u128, PriceError> {
        if gas_cost == 0 {
            return Err(PriceError::ZeroGas);
        }
        let sid = rate(rates, &self.config.sid_denom, now, self.config.max_rate_age)?;
        let pax = rate(rates, &self.config.pax_denom, now, self.config.max_rate_age)?;
        let sid_wei = ceil_div(gas_cost.checked_mul(pax).ok_or(PriceError::Overflow)?, sid)?;
        ceil_div(sid_wei, 1_000_000_000_000)
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
    use crate::quote::word;

    pub(crate) fn rates() -> Vec<OracleRate> {
        vec![
            OracleRate {
                denom: "usid".into(),
                exchange_rate: "1.000000000000000000".into(),
                last_update: "10".into(),
                last_update_timestamp: 1000,
            },
            OracleRate {
                denom: "uhpx".into(),
                exchange_rate: "2.000000000000000000".into(),
                last_update: "10".into(),
                last_update_timestamp: 1000,
            },
        ]
    }
    fn string(value: &str) -> Vec<u8> {
        let mut bytes = word(value.len() as u128).to_vec();
        bytes.extend_from_slice(value.as_bytes());
        bytes.resize(bytes.len().div_ceil(32) * 32, 0);
        bytes
    }
    fn encoded_rates() -> Vec<u8> {
        let mut pairs = Vec::new();
        for rate in rates() {
            let denom = string(&rate.denom);
            let value = string(&rate.exchange_rate);
            let updated = string(&rate.last_update);
            let pair = [
                word(64).to_vec(),
                word((64 + denom.len()) as u128).to_vec(),
                denom,
                word(96).to_vec(),
                word((96 + value.len()) as u128).to_vec(),
                word(1000).to_vec(),
                value,
                updated,
            ]
            .concat();
            pairs.push(pair);
        }
        [
            word(32).to_vec(),
            word(2).to_vec(),
            word(64).to_vec(),
            word((64 + pairs[0].len()) as u128).to_vec(),
            pairs.concat(),
        ]
        .concat()
    }
    #[test]
    fn oracle_abi_nested_tuples_and_malformed_inputs() {
        let bytes = encoded_rates();
        assert_eq!(decode_exchange_rates(&bytes), Ok(rates()));
        for length in 0..bytes.len() {
            assert!(decode_exchange_rates(&bytes[..length]).is_err());
        }
        let mut bad = bytes;
        bad[64] = 255;
        assert_eq!(decode_exchange_rates(&bad), Err(PriceError::Malformed));
        let abi: serde_json::Value =
            serde_json::from_str(include_str!("../../../../precompiles/oracle/abi.json"))
                .unwrap_or_else(|e| panic!("{e}"));
        assert_eq!(abi[0]["name"], "getExchangeRates");
        assert_eq!(abi[0]["inputs"].as_array().map(Vec::len), Some(0));
        assert_eq!(
            abi[0]["outputs"][0]["components"][1]["components"][2]["type"],
            "int64"
        );
    }
    #[test]
    fn pricing_and_all_refusals() -> Result<(), Box<dyn std::error::Error>> {
        let pricing = Pricing::new(&config())?;
        assert_eq!(
            pricing.quote(&rates(), 1_000_000_000_000_000_000, 1000),
            Ok(2_020_000)
        );
        assert_eq!(pricing.quote(&[], 1, 1000), Err(PriceError::MissingRate));
        assert_eq!(
            pricing.quote(&rates()[..1], 1, 1000),
            Err(PriceError::MissingRate)
        );
        for timestamp in [-1, 0, 699, 1001] {
            let mut raw = rates();
            raw[0].last_update_timestamp = timestamp;
            assert_eq!(pricing.quote(&raw, 1, 1000), Err(PriceError::StaleRate));
        }
        let mut raw = rates();
        raw[0].last_update_timestamp = 700;
        assert!(pricing.quote(&raw, 1_000_000_000_000_000_000, 1000).is_ok());
        for invalid in [
            "0",
            "-1",
            "1.2.3",
            "1e18",
            ".1",
            "1.",
            "1.0000000000000000001",
            "",
        ] {
            let mut raw = rates();
            raw[0].exchange_rate = invalid.into();
            assert_eq!(pricing.quote(&raw, 1, 1000), Err(PriceError::InvalidRate));
        }
        let mut raw = rates();
        raw.push(raw[0].clone());
        assert_eq!(pricing.quote(&raw, 1, 1000), Err(PriceError::InvalidRate));
        assert_eq!(pricing.quote(&rates(), 0, 1000), Err(PriceError::ZeroGas));
        assert_eq!(
            pricing.quote(&rates(), u128::MAX, 1000),
            Err(PriceError::Overflow)
        );
        for amount in [1_899_999, 2_100_001] {
            assert_eq!(
                pricing.validate_amount(&rates(), 1_000_000_000_000_000_000, amount, 1000),
                Err(PriceError::OutsideSpread)
            );
        }
        for amount in [1_900_000, 2_100_000] {
            assert_eq!(
                pricing.validate_amount(&rates(), 1_000_000_000_000_000_000, amount, 1000),
                Ok(())
            );
        }
        let mut cfg = config();
        cfg.margin_bps = 0;
        let mut raw = rates();
        raw[0].exchange_rate = "3".into();
        raw[1].exchange_rate = "1".into();
        assert_eq!(Pricing::new(&cfg)?.quote(&raw, 1, 1000), Ok(1));
        assert_eq!(pricing.quote(&raw, 1, 1000), Err(PriceError::OutsideSpread));
        Ok(())
    }
}
