//! The Solana side of the relayer.
//!
//! Solana deposits into the Paxeer X custody program are observed through
//! [`rpc::SolanaRpc`], turned into inbound attestations by
//! [`observe::observe_deposits`] and submitted to the precompile as `bridgeIn`
//! by the relayer's existing inbound path, under the same journal stream shape
//! the Ethereum chains use.
//!
//! Solana identities enter a digest as 20-byte handles, exactly as
//! `bridge/ATTESTATION-SOLANA.md` specifies: the handle of a 32-byte key is the
//! last 20 bytes of its keccak256, the inbound txHash is keccak256 of the
//! 64-byte transaction signature, and Solana's chain id on the Paxeer side is
//! the reserved constant [`SOLANA_CHAIN_ID`].

pub mod observe;
pub mod rpc;

use std::fmt;

use sha3::{Digest as _, Keccak256};

/// Solana's chain id on the Paxeer side: the ASCII bytes of SOLANA, left-padded
/// to eight bytes and read big-endian (`0x0000534f4c414e41`), far above every
/// EIP-155 chain id in use.
pub const SOLANA_CHAIN_ID: u64 = 91_600_046_870_081;

/// The width of a handle, and of every address inside an attestation preimage.
pub const HANDLE_BYTES: usize = 20;

/// The longest base58 key or signature text the relayer decodes: a 64-byte
/// signature needs at most 88 characters.
const MAX_BASE58_CHARACTERS: usize = 128;

/// The longest base58 instruction data the relayer decodes: a whole 1232-byte
/// transaction needs at most 1683 characters.
const MAX_BASE58_DATA_CHARACTERS: usize = 1700;

const BASE58_ALPHABET: &[u8; 58] = b"123456789ABCDEFGHJKLMNPQRSTUVWXYZabcdefghijkmnopqrstuvwxyz";
const BASE64_ALPHABET: &[u8; 64] =
    b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";

/// A base58 or base64 text that does not encode the bytes it must.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct CodecError;

impl fmt::Display for CodecError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("invalid base58 or base64 text")
    }
}

impl std::error::Error for CodecError {}

/// The last 20 bytes of keccak256 of a 32-byte Solana key.
#[must_use]
pub fn handle(key: &[u8; 32]) -> [u8; HANDLE_BYTES] {
    let digest = Keccak256::digest(key);
    let mut out = [0_u8; HANDLE_BYTES];
    out.copy_from_slice(&digest[32 - HANDLE_BYTES..]);
    out
}

/// The inbound txHash of a Solana deposit: keccak256 of the 64-byte signature
/// of the transaction that carried it.
#[must_use]
pub fn inbound_tx_hash(signature: &[u8; 64]) -> [u8; 32] {
    Keccak256::digest(signature).into()
}

/// Bitcoin-alphabet base58, the encoding of every Solana key and signature.
#[must_use]
pub fn base58_encode(bytes: &[u8]) -> String {
    let zeros = bytes.iter().take_while(|byte| **byte == 0).count();
    let mut digits: Vec<u8> = Vec::with_capacity(bytes.len() * 138 / 100 + 1);
    for byte in &bytes[zeros..] {
        let mut carry = u32::from(*byte);
        for digit in &mut digits {
            carry += u32::from(*digit) << 8;
            *digit = low_byte(carry % 58);
            carry /= 58;
        }
        while carry > 0 {
            digits.push(low_byte(carry % 58));
            carry /= 58;
        }
    }
    let mut out = String::with_capacity(zeros + digits.len());
    out.extend(std::iter::repeat_n('1', zeros));
    for digit in digits.iter().rev() {
        out.push(char::from(BASE58_ALPHABET[usize::from(*digit)]));
    }
    out
}

/// Decodes base58 text.
///
/// # Errors
///
/// Refuses empty or oversized text and any character outside the alphabet.
pub fn base58_decode(text: &str) -> Result<Vec<u8>, CodecError> {
    if text.is_empty() || text.len() > MAX_BASE58_CHARACTERS {
        return Err(CodecError);
    }
    decode_base58(text)
}

/// Decodes base58 instruction data, which may be empty.
///
/// # Errors
///
/// Refuses text longer than a whole transaction could need and any character
/// outside the alphabet.
pub fn base58_data(text: &str) -> Result<Vec<u8>, CodecError> {
    if text.len() > MAX_BASE58_DATA_CHARACTERS {
        return Err(CodecError);
    }
    decode_base58(text)
}

fn decode_base58(text: &str) -> Result<Vec<u8>, CodecError> {
    let mut bytes: Vec<u8> = Vec::with_capacity(text.len());
    for character in text.bytes() {
        let mut carry = u32::from(base58_digit(character)?);
        for byte in &mut bytes {
            carry += u32::from(*byte) * 58;
            *byte = low_byte(carry);
            carry >>= 8;
        }
        while carry > 0 {
            bytes.push(low_byte(carry));
            carry >>= 8;
        }
    }
    let zeros = text
        .bytes()
        .take_while(|character| *character == b'1')
        .count();
    let mut out = vec![0_u8; zeros];
    out.extend(bytes.iter().rev());
    Ok(out)
}

/// Decodes base58 text that must carry exactly `N` bytes.
///
/// # Errors
///
/// Refuses invalid text and any other length.
pub fn base58_fixed<const N: usize>(text: &str) -> Result<[u8; N], CodecError> {
    base58_decode(text)?.try_into().map_err(|_| CodecError)
}

/// Standard padded base64, the encoding of account data and wire transactions.
#[must_use]
pub fn base64_encode(bytes: &[u8]) -> String {
    let mut out = String::with_capacity(bytes.len().div_ceil(3) * 4);
    for chunk in bytes.chunks(3) {
        let mut word = [0_u8; 4];
        word[1..=chunk.len()].copy_from_slice(chunk);
        let value = u32::from_be_bytes(word);
        for position in 0..4 {
            if position <= chunk.len() {
                let index = (value >> (18 - 6 * position)) & 0x3f;
                out.push(char::from(BASE64_ALPHABET[usize::from(low_byte(index))]));
            } else {
                out.push('=');
            }
        }
    }
    out
}

/// Decodes standard padded base64.
///
/// # Errors
///
/// Refuses text whose length is not a multiple of four, padding anywhere but
/// the end, any character outside the alphabet and non-zero trailing bits.
pub fn base64_decode(text: &str) -> Result<Vec<u8>, CodecError> {
    let bytes = text.as_bytes();
    if !bytes.len().is_multiple_of(4) {
        return Err(CodecError);
    }
    let chunks = bytes.len() / 4;
    let mut out = Vec::with_capacity(chunks * 3);
    for (index, chunk) in bytes.chunks_exact(4).enumerate() {
        let padding = chunk.iter().rev().take_while(|byte| **byte == b'=').count();
        if padding > 2 || (padding > 0 && index + 1 != chunks) {
            return Err(CodecError);
        }
        let mut value = 0_u32;
        for (position, character) in chunk.iter().enumerate() {
            let digit = if position >= 4 - padding {
                0
            } else {
                base64_digit(*character)?
            };
            value = (value << 6) | u32::from(digit);
        }
        let [_, first, second, third] = value.to_be_bytes();
        match padding {
            0 => out.extend_from_slice(&[first, second, third]),
            1 if third == 0 => out.extend_from_slice(&[first, second]),
            2 if second == 0 && third == 0 => out.push(first),
            _ => return Err(CodecError),
        }
    }
    Ok(out)
}

fn low_byte(value: u32) -> u8 {
    value.to_le_bytes()[0]
}

fn base58_digit(character: u8) -> Result<u8, CodecError> {
    BASE58_ALPHABET
        .iter()
        .position(|candidate| *candidate == character)
        .and_then(|position| u8::try_from(position).ok())
        .ok_or(CodecError)
}

fn base64_digit(character: u8) -> Result<u8, CodecError> {
    BASE64_ALPHABET
        .iter()
        .position(|candidate| *candidate == character)
        .and_then(|position| u8::try_from(position).ok())
        .ok_or(CodecError)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::hex;

    #[test]
    fn the_chain_id_is_the_padded_ascii_of_solana() {
        let mut word = [0_u8; 8];
        word[2..].copy_from_slice(b"SOLANA");
        assert_eq!(SOLANA_CHAIN_ID, u64::from_be_bytes(word));
    }

    #[test]
    fn handles_match_the_pinned_vectors() {
        let sidiora = base58_fixed::<32>("5w3wVdJaESaJKyLmStM6Hv9UyUkmZ1b9DLQquAqqpump")
            .unwrap_or_else(|error| panic!("mint: {error}"));
        assert_eq!(
            hex::prefixed(&handle(&sidiora)),
            "0x9232467d43fd9edd0bf250fe24ca0c9380b3ca86"
        );
        let wrapped = base58_fixed::<32>("So11111111111111111111111111111111111111112")
            .unwrap_or_else(|error| panic!("mint: {error}"));
        assert_eq!(
            hex::prefixed(&handle(&wrapped)),
            "0xcf996523b5d068a26f0aa8a116602fe5033ee3a1"
        );
        let authority = base58_fixed::<32>("GxxA9Cs9v5pAGVsaCe2jjDrtmieeBijcY4S5HHTY8Vq6")
            .unwrap_or_else(|error| panic!("authority: {error}"));
        assert_eq!(
            hex::prefixed(&handle(&authority)),
            "0x334121a65b47bd45c3f6381537d9180e98e445bc"
        );
    }

    #[test]
    fn base58_round_trips_keys_signatures_and_leading_zeros() {
        let system = base58_fixed::<32>("11111111111111111111111111111111")
            .unwrap_or_else(|error| panic!("system: {error}"));
        assert_eq!(system, [0; 32]);
        assert_eq!(base58_encode(&system), "11111111111111111111111111111111");
        let program = "A7SZbByPYuHpunZ9pyMDMrhMYvK44ANT1AVqb8U1FpM9";
        let decoded = base58_fixed::<32>(program).unwrap_or_else(|error| panic!("{error}"));
        assert_eq!(
            hex::prefixed(&decoded),
            "0x875f91f2b7ddd837d8584366fa8d7a8c73905b2212713a4826d4a89faa8d40fc"
        );
        assert_eq!(base58_encode(&decoded), program);
        let mut signature = [0_u8; 64];
        signature[2..].fill(0xab);
        let text = base58_encode(&signature);
        assert!(text.starts_with("11"));
        assert_eq!(base58_fixed::<64>(&text), Ok(signature));
    }

    #[test]
    fn base58_refuses_foreign_characters_and_wrong_lengths() {
        assert_eq!(base58_decode(""), Err(CodecError));
        assert_eq!(base58_decode("0OIl"), Err(CodecError));
        assert_eq!(base58_data(""), Ok(Vec::new()));
        assert_eq!(base58_data(&"2".repeat(1701)), Err(CodecError));
        assert_eq!(
            base58_fixed::<32>("So1111111111111111111111111111111111111111"),
            Err(CodecError)
        );
    }

    #[test]
    fn base64_round_trips_and_refuses_malformed_padding() {
        for length in 0..8 {
            let bytes: Vec<u8> = (0..length).map(|index| index * 37 + 1).collect();
            let text = base64_encode(&bytes);
            assert_eq!(base64_decode(&text), Ok(bytes));
        }
        assert_eq!(base64_encode(b"PXBR"), "UFhCUg==");
        assert_eq!(base64_decode("UFhCUg=="), Ok(b"PXBR".to_vec()));
        assert_eq!(base64_decode("UFhCUg="), Err(CodecError));
        assert_eq!(base64_decode("UF=CUg=="), Err(CodecError));
        assert_eq!(base64_decode("UFhCUh=="), Err(CodecError));
        assert_eq!(base64_decode("UFhC*g=="), Err(CodecError));
    }
}
