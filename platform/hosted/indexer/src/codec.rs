//! Small exact codecs shared by the ingesters: hex, base64, big-endian
//! 256-bit words rendered as decimal, and quantity parsing.

use std::fmt::Write as _;

use crate::IndexError;

/// Lowercase hex of `bytes` without a prefix.
#[must_use]
pub fn hex(bytes: &[u8]) -> String {
    let mut text = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        let _ = write!(text, "{byte:02x}");
    }
    text
}

/// Lowercase `0x`-prefixed hex of `bytes`.
#[must_use]
pub fn hex0x(bytes: &[u8]) -> String {
    format!("0x{}", hex(bytes))
}

/// Decodes hex with or without a `0x` prefix. An odd digit count is refused.
///
/// # Errors
/// Refuses non-hex digits and odd lengths.
pub fn unhex(text: &str) -> Result<Vec<u8>, IndexError> {
    let digits = text
        .strip_prefix("0x")
        .or_else(|| text.strip_prefix("0X"))
        .unwrap_or(text);
    if digits.len() % 2 != 0 {
        return Err(IndexError::Decode(format!("odd-length hex: {text}")));
    }
    let bytes = digits.as_bytes();
    let mut output = Vec::with_capacity(bytes.len() / 2);
    for pair in bytes.chunks_exact(2) {
        let high = nibble(pair[0])?;
        let low = nibble(pair[1])?;
        output.push((high << 4) | low);
    }
    Ok(output)
}

fn nibble(digit: u8) -> Result<u8, IndexError> {
    match digit {
        b'0'..=b'9' => Ok(digit - b'0'),
        b'a'..=b'f' => Ok(digit - b'a' + 10),
        b'A'..=b'F' => Ok(digit - b'A' + 10),
        _ => Err(IndexError::Decode("non-hex digit".to_owned())),
    }
}

/// Decodes exactly `N` bytes of hex.
///
/// # Errors
/// Refuses malformed hex and any other length.
pub fn unhex_fixed<const N: usize>(text: &str) -> Result<[u8; N], IndexError> {
    unhex(text)?
        .try_into()
        .map_err(|_| IndexError::Decode(format!("expected {N} bytes of hex")))
}

/// Parses an Ethereum JSON-RPC quantity (`0x`-prefixed, no leading zeros
/// beyond `0x0`) into a `u64`.
///
/// # Errors
/// Refuses anything that is not a canonical `u64` quantity.
pub fn quantity_u64(text: &str) -> Result<u64, IndexError> {
    let digits = text
        .strip_prefix("0x")
        .filter(|digits| !digits.is_empty() && digits.len() <= 16)
        .ok_or_else(|| IndexError::Decode(format!("invalid quantity {text}")))?;
    u64::from_str_radix(digits, 16)
        .map_err(|_| IndexError::Decode(format!("invalid quantity {text}")))
}

/// Renders a `u64` as an Ethereum JSON-RPC quantity.
#[must_use]
pub fn to_quantity(value: u64) -> String {
    format!("0x{value:x}")
}

/// Parses a `0x` quantity of up to 256 bits into a big-endian word.
///
/// # Errors
/// Refuses non-hex text and values wider than 256 bits.
pub fn quantity_word(text: &str) -> Result<[u8; 32], IndexError> {
    let digits = text
        .strip_prefix("0x")
        .filter(|digits| !digits.is_empty() && digits.len() <= 64)
        .ok_or_else(|| IndexError::Decode(format!("invalid quantity {text}")))?;
    let padded = format!("{digits:0>64}");
    unhex_fixed::<32>(&padded)
}

/// Renders an unsigned big-endian integer of any width as decimal.
#[must_use]
pub fn be_decimal(bytes: &[u8]) -> String {
    let mut digits: Vec<u8> = bytes
        .iter()
        .copied()
        .skip_while(|byte| *byte == 0)
        .collect();
    if digits.is_empty() {
        return "0".to_owned();
    }
    let mut output = Vec::new();
    while !digits.is_empty() {
        let mut remainder: u32 = 0;
        let mut quotient = Vec::with_capacity(digits.len());
        for byte in &digits {
            let accumulator = (remainder << 8) | u32::from(*byte);
            let digit = accumulator / 10;
            remainder = accumulator % 10;
            if !(quotient.is_empty() && digit == 0) {
                quotient.push(u8::try_from(digit).unwrap_or(u8::MAX));
            }
        }
        output.push(b'0' + u8::try_from(remainder).unwrap_or(0));
        digits = quotient;
    }
    output.reverse();
    String::from_utf8(output).unwrap_or_default()
}

/// Renders a two's-complement big-endian signed integer as decimal.
#[must_use]
pub fn be_signed_decimal(bytes: &[u8]) -> String {
    if bytes.first().is_some_and(|byte| byte & 0x80 != 0) {
        let mut magnitude: Vec<u8> = bytes.iter().map(|byte| !byte).collect();
        for byte in magnitude.iter_mut().rev() {
            let (next, overflow) = byte.overflowing_add(1);
            *byte = next;
            if !overflow {
                break;
            }
        }
        format!("-{}", be_decimal(&magnitude))
    } else {
        be_decimal(bytes)
    }
}

/// True when every byte is zero.
#[must_use]
pub fn is_zero(bytes: &[u8]) -> bool {
    bytes.iter().all(|byte| *byte == 0)
}

/// Strict RFC 4648 base64 decoding with padding.
///
/// # Errors
/// Refuses non-alphabet characters, bad padding and non-canonical trailing
/// bits.
pub fn base64_decode(text: &str) -> Result<Vec<u8>, IndexError> {
    let bytes = text.as_bytes();
    if bytes.len() % 4 != 0 {
        return Err(IndexError::Decode(
            "base64 length is not a multiple of four".to_owned(),
        ));
    }
    let mut output = Vec::with_capacity(bytes.len() / 4 * 3);
    for (index, quad) in bytes.chunks_exact(4).enumerate() {
        let last = index + 1 == bytes.len() / 4;
        let mut values = [0_u8; 4];
        let mut padding = 0;
        for (position, character) in quad.iter().enumerate() {
            values[position] = match character {
                b'A'..=b'Z' => character - b'A',
                b'a'..=b'z' => character - b'a' + 26,
                b'0'..=b'9' => character - b'0' + 52,
                b'+' => 62,
                b'/' => 63,
                b'=' if last && position >= 2 => {
                    padding += 1;
                    0
                }
                _ => return Err(IndexError::Decode("invalid base64".to_owned())),
            };
            if padding > 0 && *character != b'=' {
                return Err(IndexError::Decode("invalid base64 padding".to_owned()));
            }
        }
        let word = (u32::from(values[0]) << 18)
            | (u32::from(values[1]) << 12)
            | (u32::from(values[2]) << 6)
            | u32::from(values[3]);
        let [_, first, second, third] = word.to_be_bytes();
        output.push(first);
        if padding < 2 {
            output.push(second);
        } else if second != 0 {
            return Err(IndexError::Decode("non-canonical base64".to_owned()));
        }
        if padding < 1 {
            output.push(third);
        } else if third != 0 {
            return Err(IndexError::Decode("non-canonical base64".to_owned()));
        }
    }
    Ok(output)
}

/// Decodes `%XX` escapes in a URL path segment or query value; `+` stays
/// literal in paths and becomes a space in queries when `query` is set.
///
/// # Errors
/// Refuses truncated escapes and non-UTF-8 results.
pub fn percent_decode(text: &str, query: bool) -> Result<String, IndexError> {
    let bytes = text.as_bytes();
    let mut output = Vec::with_capacity(bytes.len());
    let mut index = 0;
    while index < bytes.len() {
        match bytes[index] {
            b'%' => {
                let pair = bytes
                    .get(index + 1..index + 3)
                    .ok_or_else(|| IndexError::Decode("truncated percent escape".to_owned()))?;
                output.push((nibble(pair[0])? << 4) | nibble(pair[1])?);
                index += 3;
            }
            b'+' if query => {
                output.push(b' ');
                index += 1;
            }
            byte => {
                output.push(byte);
                index += 1;
            }
        }
    }
    String::from_utf8(output).map_err(|_| IndexError::Decode("escape is not UTF-8".to_owned()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decimal_rendering_is_exact_for_wide_words() {
        assert_eq!(be_decimal(&[0; 32]), "0");
        assert_eq!(be_decimal(&[0x01, 0x00]), "256");
        let max = [0xff_u8; 32];
        assert_eq!(
            be_decimal(&max),
            "115792089237316195423570985008687907853269984665640564039457584007913129639935"
        );
        assert_eq!(be_signed_decimal(&max), "-1");
        let mut minus_two = [0xff_u8; 32];
        minus_two[31] = 0xfe;
        assert_eq!(be_signed_decimal(&minus_two), "-2");
        assert_eq!(be_decimal(&u128::MAX.to_be_bytes()), u128::MAX.to_string());
    }

    #[test]
    fn base64_is_strict() {
        assert_eq!(base64_decode("YW1vdW50").unwrap_or_default(), b"amount");
        assert_eq!(
            base64_decode("cmVjaXBpZW50").unwrap_or_default(),
            b"recipient"
        );
        assert_eq!(base64_decode("ZGVub20=").unwrap_or_default(), b"denom");
        assert!(base64_decode("amount").is_err());
        assert!(base64_decode("ZGVub21=").is_err());
    }

    #[test]
    fn quantities_and_hex_round_trip() {
        assert_eq!(quantity_u64("0x64").unwrap_or_default(), 100);
        assert!(quantity_u64("64").is_err());
        assert_eq!(to_quantity(100), "0x64");
        assert_eq!(unhex("0x0a0B").unwrap_or_default(), vec![10, 11]);
        assert!(unhex("0x0").is_err());
        let word = quantity_word("0xde0b6b3a7640000").unwrap_or([0; 32]);
        assert_eq!(be_decimal(&word), "1000000000000000000");
        assert_eq!(
            percent_decode("factory%2Fpax1%2Fx", false).unwrap_or_default(),
            "factory/pax1/x"
        );
    }
}
