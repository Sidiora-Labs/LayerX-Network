//! Strict Ed25519 verification matching the core's canonicality rules.

use ed25519_dalek::{Signature, VerifyingKey};

use crate::{SignatureMessage, VerifyError};

const FIELD_PRIME: [u8; 32] = [
    0xed, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff,
    0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0x7f,
];

fn little_endian_less(left: &[u8; 32], right: &[u8; 32]) -> bool {
    for index in (0..32).rev() {
        if left[index] < right[index] {
            return true;
        }
        if left[index] > right[index] {
            return false;
        }
    }
    false
}

pub(crate) fn public_key_is_canonical(public_key: &[u8; 32]) -> bool {
    let mut y = *public_key;
    y[31] &= 0x7f;
    if y.iter().all(|byte| *byte == 0) || (y[0] == 1 && y[1..].iter().all(|byte| *byte == 0)) {
        return false;
    }
    little_endian_less(&y, &FIELD_PRIME)
}

/// Verifies one strict canonical Ed25519 signature over the scoped core digest.
///
/// # Errors
///
/// Returns `BadSignature` for non-canonical, weak, high-order, non-reduced, or
/// mathematically invalid keys and signatures.
pub fn verify(
    public_key: &[u8; 32],
    signature: &[u8; 64],
    message: SignatureMessage<'_>,
) -> Result<(), VerifyError> {
    verify_digest(public_key, signature, &message.digest())
}

/// Verifies a strict canonical Ed25519 signature over an already
/// domain-separated 32-byte core digest.
///
/// # Errors
///
/// Returns `BadSignature` for non-canonical, weak, high-order, non-reduced, or
/// mathematically invalid keys and signatures.
pub fn verify_digest(
    public_key: &[u8; 32],
    signature: &[u8; 64],
    digest: &[u8; 32],
) -> Result<(), VerifyError> {
    if !public_key_is_canonical(public_key) {
        return Err(VerifyError::BadSignature);
    }
    let verifying_key =
        VerifyingKey::from_bytes(public_key).map_err(|_| VerifyError::BadSignature)?;
    if verifying_key.is_weak() {
        return Err(VerifyError::BadSignature);
    }
    let signature = Signature::from_bytes(signature);
    verifying_key
        .verify_strict(digest, &signature)
        .map_err(|_| VerifyError::BadSignature)
}
