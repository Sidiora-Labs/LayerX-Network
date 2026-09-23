//! `0x`-prefixed hexadecimal as Ethereum JSON-RPC speaks it.

use std::fmt;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct HexError;

impl fmt::Display for HexError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("malformed hexadecimal")
    }
}

impl std::error::Error for HexError {}

const DIGITS: &[u8; 16] = b"0123456789abcdef";

/// Lowercase hexadecimal without a prefix.
#[must_use]
pub fn encode(bytes: &[u8]) -> String {
    let mut output = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        output.push(char::from(DIGITS[usize::from(byte >> 4)]));
        output.push(char::from(DIGITS[usize::from(byte & 0x0f)]));
    }
    output
}

/// Lowercase hexadecimal with the `0x` prefix.
#[must_use]
pub fn prefixed(bytes: &[u8]) -> String {
    format!("0x{}", encode(bytes))
}

fn digit(value: u8) -> Option<u8> {
    match value {
        b'0'..=b'9' => Some(value - b'0'),
        b'a'..=b'f' => Some(value - b'a' + 10),
        b'A'..=b'F' => Some(value - b'A' + 10),
        _ => None,
    }
}

/// Decodes `0x`-prefixed, even-length hexadecimal data.
///
/// # Errors
///
/// Refuses a missing prefix, an odd length or a non-hexadecimal digit.
pub fn decode(value: &str) -> Result<Vec<u8>, HexError> {
    let digits = value
        .strip_prefix("0x")
        .or_else(|| value.strip_prefix("0X"))
        .ok_or(HexError)?;
    if digits.len() % 2 != 0 {
        return Err(HexError);
    }
    digits
        .as_bytes()
        .chunks_exact(2)
        .map(|pair| Ok((digit(pair[0]).ok_or(HexError)? << 4) | digit(pair[1]).ok_or(HexError)?))
        .collect()
}

/// Decodes exactly `N` bytes of `0x`-prefixed hexadecimal.
///
/// # Errors
///
/// Refuses malformed hexadecimal or any other length.
pub fn fixed<const N: usize>(value: &str) -> Result<[u8; N], HexError> {
    decode(value)?.try_into().map_err(|_| HexError)
}

/// A JSON-RPC quantity: `0x` followed by the minimal lowercase digits.
#[must_use]
pub fn quantity(value: u64) -> String {
    format!("0x{value:x}")
}

/// A JSON-RPC quantity that fits in 128 bits.
///
/// # Errors
///
/// Refuses a missing prefix, no digits, or a value over `u128::MAX`.
pub fn parse_quantity_u128(value: &str) -> Result<u128, HexError> {
    let digits = value.strip_prefix("0x").ok_or(HexError)?;
    if digits.is_empty() || digits.len() > 32 {
        return Err(HexError);
    }
    u128::from_str_radix(digits, 16).map_err(|_| HexError)
}

/// A JSON-RPC quantity that fits in 64 bits.
///
/// # Errors
///
/// Refuses a missing prefix, no digits, or a value over `u64::MAX`.
pub fn parse_quantity(value: &str) -> Result<u64, HexError> {
    u64::try_from(parse_quantity_u128(value)?).map_err(|_| HexError)
}

/// Serde adapter storing a fixed byte array as `0x` hexadecimal.
pub mod fixed_serde {
    use serde::{Deserialize as _, Deserializer, Serializer};

    /// # Errors
    ///
    /// Propagates the serializer's error.
    pub fn serialize<S: Serializer, const N: usize>(
        value: &[u8; N],
        serializer: S,
    ) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(&super::prefixed(value))
    }

    /// # Errors
    ///
    /// Refuses malformed hexadecimal or a wrong length.
    pub fn deserialize<'de, D: Deserializer<'de>, const N: usize>(
        deserializer: D,
    ) -> Result<[u8; N], D::Error> {
        let text = String::deserialize(deserializer)?;
        super::fixed(&text).map_err(serde::de::Error::custom)
    }
}

/// Serde adapter storing variable bytes as `0x` hexadecimal.
pub mod bytes_serde {
    use serde::{Deserialize as _, Deserializer, Serializer};

    /// # Errors
    ///
    /// Propagates the serializer's error.
    pub fn serialize<S: Serializer>(value: &[u8], serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(&super::prefixed(value))
    }

    /// # Errors
    ///
    /// Refuses malformed hexadecimal.
    pub fn deserialize<'de, D: Deserializer<'de>>(deserializer: D) -> Result<Vec<u8>, D::Error> {
        let text = String::deserialize(deserializer)?;
        super::decode(&text).map_err(serde::de::Error::custom)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hexadecimal_round_trips_and_refuses_malformed_input() {
        assert_eq!(prefixed(&[0x00, 0xab, 0xff]), "0x00abff");
        assert_eq!(decode("0x00ABff"), Ok(vec![0x00, 0xab, 0xff]));
        assert_eq!(decode("00ab"), Err(HexError));
        assert_eq!(decode("0xabc"), Err(HexError));
        assert_eq!(decode("0xzz"), Err(HexError));
        assert_eq!(fixed::<2>("0x0102"), Ok([1, 2]));
        assert_eq!(fixed::<3>("0x0102"), Err(HexError));
    }

    #[test]
    fn quantities_are_minimal_and_bounded() {
        assert_eq!(quantity(0), "0x0");
        assert_eq!(quantity(255), "0xff");
        assert_eq!(parse_quantity("0x78"), Ok(120));
        assert_eq!(parse_quantity("0x"), Err(HexError));
        assert_eq!(parse_quantity("78"), Err(HexError));
        assert_eq!(parse_quantity("0x10000000000000000"), Err(HexError));
        assert_eq!(parse_quantity_u128("0x10000000000000000"), Ok(1_u128 << 64));
    }
}
