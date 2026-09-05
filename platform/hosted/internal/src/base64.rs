//! Standard padded base64 as the developer plane exchanges keys, messages and
//! signatures.

const ALPHABET: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";

/// Encodes bytes as standard base64 with padding.
#[must_use]
pub fn encode(bytes: &[u8]) -> String {
    let mut out = String::with_capacity(bytes.len().div_ceil(3) * 4);
    for chunk in bytes.chunks(3) {
        let first = chunk[0];
        let second = chunk.get(1).copied().unwrap_or(0);
        let third = chunk.get(2).copied().unwrap_or(0);
        out.push(char::from(ALPHABET[usize::from(first >> 2)]));
        out.push(char::from(
            ALPHABET[usize::from(((first & 0x03) << 4) | (second >> 4))],
        ));
        if chunk.len() > 1 {
            out.push(char::from(
                ALPHABET[usize::from(((second & 0x0f) << 2) | (third >> 6))],
            ));
        } else {
            out.push('=');
        }
        if chunk.len() > 2 {
            out.push(char::from(ALPHABET[usize::from(third & 0x3f)]));
        } else {
            out.push('=');
        }
    }
    out
}

/// Decodes standard padded base64, refusing whitespace, missing padding and
/// non-canonical trailing bits.
#[must_use]
pub fn decode(value: &str) -> Option<Vec<u8>> {
    let bytes = value.as_bytes();
    if !bytes.len().is_multiple_of(4) {
        return None;
    }
    let mut out = Vec::with_capacity(bytes.len() / 4 * 3);
    for (index, chunk) in bytes.chunks(4).enumerate() {
        let padding = chunk.iter().rev().take_while(|byte| **byte == b'=').count();
        if padding > 2
            || (padding != 0 && index + 1 != bytes.len() / 4)
            || chunk[..4 - padding].contains(&b'=')
        {
            return None;
        }
        let mut accumulator = 0_u32;
        for byte in &chunk[..4 - padding] {
            accumulator = (accumulator << 6) | u32::from(sextet(*byte)?);
        }
        accumulator <<= match padding {
            0 => 0,
            1 => 6,
            _ => 12,
        };
        let decoded = accumulator.to_be_bytes();
        match padding {
            0 => out.extend_from_slice(&decoded[1..4]),
            1 => {
                if decoded[3] != 0 {
                    return None;
                }
                out.extend_from_slice(&decoded[1..3]);
            }
            _ => {
                if decoded[2] != 0 || decoded[3] != 0 {
                    return None;
                }
                out.push(decoded[1]);
            }
        }
    }
    Some(out)
}

fn sextet(byte: u8) -> Option<u8> {
    match byte {
        b'A'..=b'Z' => Some(byte - b'A'),
        b'a'..=b'z' => Some(byte - b'a' + 26),
        b'0'..=b'9' => Some(byte - b'0' + 52),
        b'+' => Some(62),
        b'/' => Some(63),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::{decode, encode};

    #[test]
    fn round_trips_every_padding_length() {
        for length in 0..=66 {
            let bytes: Vec<u8> = (0..length)
                .map(|index| u8::try_from(index).unwrap_or(0) ^ 0x5a)
                .collect();
            let encoded = encode(&bytes);
            assert!(encoded.len().is_multiple_of(4));
            assert_eq!(decode(&encoded).as_deref(), Some(bytes.as_slice()));
        }
        assert_eq!(encode(b"Man"), "TWFu");
        assert_eq!(encode(b"Ma"), "TWE=");
        assert_eq!(encode(b"M"), "TQ==");
    }

    #[test]
    fn rejects_non_canonical_input() {
        assert_eq!(decode("TWE"), None);
        assert_eq!(decode("TW=E"), None);
        assert_eq!(decode("TWF="), None);
        assert_eq!(decode("TR=="), None);
        assert_eq!(decode("TQ=\n"), None);
        assert_eq!(decode("===="), None);
        assert_eq!(decode(""), Some(Vec::new()));
    }
}
