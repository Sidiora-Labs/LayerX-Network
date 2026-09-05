//! Bounded secrets, identifiers and hex the two services share.

use sha2::{Digest, Sha256};
use std::env;
use std::fs;
use std::path::Path;
use std::time::{SystemTime, UNIX_EPOCH};
use zeroize::{Zeroize, Zeroizing};

/// Largest accepted secret, in bytes.
pub const MAX_SECRET_BYTES: usize = 4096;
/// Smallest accepted bearer token, in bytes.
pub const MIN_TOKEN_BYTES: usize = 16;

/// Reads a bounded secret from a file, trimming trailing line breaks.
///
/// # Errors
/// Returns a description when the file is unreadable, empty, oversized or
/// carries control bytes.
pub fn read_secret_file(path: &Path) -> Result<Zeroizing<String>, String> {
    let mut value =
        fs::read_to_string(path).map_err(|error| format!("{}: {error}", path.display()))?;
    while matches!(value.as_bytes().last(), Some(b'\n' | b'\r')) {
        value.pop();
    }
    if value.is_empty()
        || value.len() > MAX_SECRET_BYTES
        || value.bytes().any(|byte| byte.is_ascii_control())
    {
        value.zeroize();
        return Err(format!(
            "{} does not contain a bounded secret",
            path.display()
        ));
    }
    Ok(Zeroizing::new(value))
}

/// Reads the secret at the path named by an environment variable.
///
/// # Errors
/// Returns a description naming the variable when it is unset or the file is
/// not a bounded secret.
pub fn read_secret(path_variable: &str) -> Result<Zeroizing<String>, String> {
    let path = env::var(path_variable).map_err(|_| format!("{path_variable} is required"))?;
    read_secret_file(Path::new(&path))
}

/// Reads a bearer token file and requires the minimum token length.
///
/// # Errors
/// Returns a description naming the variable when the token is missing or
/// shorter than [`MIN_TOKEN_BYTES`].
pub fn read_token(path_variable: &str) -> Result<Zeroizing<String>, String> {
    let token = read_secret(path_variable)?;
    if token.len() < MIN_TOKEN_BYTES {
        return Err(format!(
            "{path_variable} must hold at least {MIN_TOKEN_BYTES} bytes"
        ));
    }
    Ok(token)
}

/// Reads a required environment variable.
///
/// # Errors
/// Returns a description naming the variable when it is unset or empty.
pub fn required_env(name: &str) -> Result<String, String> {
    env::var(name)
        .ok()
        .filter(|value| !value.is_empty())
        .ok_or_else(|| format!("{name} is required"))
}

/// Parses an optional integer environment variable.
///
/// # Errors
/// Returns a description when the value is not an integer.
pub fn env_u64(name: &str, default: u64) -> Result<u64, String> {
    env::var(name).map_or(Ok(default), |value| {
        value
            .parse::<u64>()
            .map_err(|_| format!("{name} must be an integer"))
    })
}

/// Seconds since the Unix epoch.
///
/// # Errors
/// Returns a description when the clock precedes the epoch.
pub fn unix_seconds() -> Result<u64, String> {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_secs())
        .map_err(|_| "system clock precedes Unix epoch".to_owned())
}

/// Returns true for a bounded identifier of ASCII letters, digits, `-`, `_`
/// and `.`, the alphabet the webhooks accept for source event ids and
/// idempotency keys.
#[must_use]
pub fn valid_identifier(value: &str, maximum: usize) -> bool {
    !value.is_empty()
        && value.len() <= maximum
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.'))
}

/// Returns true for a bounded token that may also carry `:`; the webhooks
/// subject and event identifier rule.
#[must_use]
pub fn valid_token(value: &str, maximum: usize) -> bool {
    !value.is_empty()
        && value.len() <= maximum
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.' | b':'))
}

/// Returns true for the hosted gateway principal rule: lowercase letters,
/// digits, `-`, `_`, `.` and `:` up to 128 bytes.
#[must_use]
pub fn valid_principal(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 128
        && value.bytes().all(|byte| {
            byte.is_ascii_lowercase()
                || byte.is_ascii_digit()
                || matches!(byte, b'-' | b'_' | b'.' | b':')
        })
}

/// Returns true for exactly `bytes` lowercase hex-encoded bytes.
#[must_use]
pub fn valid_hex(value: &str, bytes: usize) -> bool {
    value.len() == bytes * 2
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

/// Lowercase hex.
#[must_use]
pub fn hex(bytes: &[u8]) -> String {
    let mut out = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        out.push(char::from(b"0123456789abcdef"[usize::from(byte >> 4)]));
        out.push(char::from(b"0123456789abcdef"[usize::from(byte & 0x0f)]));
    }
    out
}

/// Decodes lowercase hex.
#[must_use]
pub fn unhex(value: &str) -> Option<Vec<u8>> {
    if !value.len().is_multiple_of(2) {
        return None;
    }
    let mut out = Vec::with_capacity(value.len() / 2);
    for pair in value.as_bytes().chunks(2) {
        let high = hex_nibble(pair[0])?;
        let low = hex_nibble(pair[1])?;
        out.push((high << 4) | low);
    }
    Some(out)
}

fn hex_nibble(byte: u8) -> Option<u8> {
    match byte {
        b'0'..=b'9' => Some(byte - b'0'),
        b'a'..=b'f' => Some(byte - b'a' + 10),
        _ => None,
    }
}

/// SHA-256 as lowercase hex.
#[must_use]
pub fn sha256_hex(value: &[u8]) -> String {
    hex(&Sha256::digest(value))
}

/// Fresh random bytes as lowercase hex.
///
/// # Errors
/// Returns a description when the entropy source is unavailable.
pub fn random_hex(bytes: usize) -> Result<Zeroizing<String>, String> {
    let mut random = Zeroizing::new(vec![0_u8; bytes]);
    getrandom::fill(&mut random).map_err(|_| "entropy is unavailable".to_owned())?;
    Ok(Zeroizing::new(hex(&random)))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn identifier_rules_match_the_developer_plane() {
        assert!(valid_identifier("register.whk-1_2", 128));
        assert!(!valid_identifier("a:b", 128));
        assert!(!valid_identifier("", 128));
        assert!(!valid_identifier(&"a".repeat(129), 128));
        assert!(valid_token("did:key:z6mk-1", 128));
        assert!(valid_principal("did:key:z6mkabc-1_2.3"));
        assert!(!valid_principal("did:key:Z6MK"));
        assert!(valid_hex(&"ab".repeat(32), 32));
        assert!(!valid_hex(&"AB".repeat(32), 32));
        assert_eq!(unhex("00abff"), Some(vec![0x00, 0xab, 0xff]));
        assert_eq!(unhex("abc"), None);
        assert_eq!(hex(&[0x00, 0xab, 0xff]), "00abff");
        assert_eq!(sha256_hex(b"").len(), 64);
    }
}
