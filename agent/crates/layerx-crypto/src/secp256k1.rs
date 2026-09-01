//! Compact low-S secp256k1 verification for Paxeer-facing certificates.

use k256::ecdsa::{signature::hazmat::PrehashVerifier as _, RecoveryId, Signature, VerifyingKey};
use sha3::{Digest as _, Keccak256};

use crate::{SignatureMessage, VerifyError};

/// Verifies a canonical compact secp256k1 signature over the scoped core digest.
///
/// # Errors
///
/// Returns `BadSignature` for invalid SEC1 keys, zero or non-reduced scalars,
/// high-S malleable signatures, and failed verification equations.
pub fn verify(
    public_key: &[u8],
    signature: &[u8; 64],
    message: SignatureMessage<'_>,
) -> Result<(), VerifyError> {
    verify_digest(public_key, signature, &message.digest())
}

/// Verifies a canonical compact secp256k1 signature over an already
/// domain-separated 32-byte core digest.
///
/// # Errors
///
/// Returns `BadSignature` for invalid SEC1 keys, zero or non-reduced scalars,
/// high-S malleable signatures, and failed verification equations.
pub fn verify_digest(
    public_key: &[u8],
    signature: &[u8; 64],
    digest: &[u8; 32],
) -> Result<(), VerifyError> {
    if public_key.len() != 33 && public_key.len() != 65 {
        return Err(VerifyError::BadSignature);
    }
    let signature = Signature::from_slice(signature).map_err(|_| VerifyError::BadSignature)?;
    if signature.normalize_s().is_some() {
        return Err(VerifyError::BadSignature);
    }
    let verifying_key =
        VerifyingKey::from_sec1_bytes(public_key).map_err(|_| VerifyError::BadSignature)?;
    verifying_key
        .verify_prehash(digest, &signature)
        .map_err(|_| VerifyError::BadSignature)
}

/// Derives the canonical EVM account from a SEC1 secp256k1 public key.
///
/// # Errors
///
/// Returns `BadSignature` when the public key is not a valid compressed or
/// uncompressed secp256k1 point.
pub fn evm_address(public_key: &[u8]) -> Result<[u8; 20], VerifyError> {
    if public_key.len() != 33 && public_key.len() != 65 {
        return Err(VerifyError::BadSignature);
    }
    let verifying_key =
        VerifyingKey::from_sec1_bytes(public_key).map_err(|_| VerifyError::BadSignature)?;
    let encoded = verifying_key.to_encoded_point(false);
    let digest = Keccak256::digest(&encoded.as_bytes()[1..]);
    let mut address = [0_u8; 20];
    address.copy_from_slice(&digest[12..]);
    Ok(address)
}

/// Verifies that an EVM-compatible recovery byte selects the supplied public
/// key for a canonical compact signature and digest.
///
/// # Errors
///
/// Returns `BadSignature` for a non-EVM recovery byte, malformed or high-S
/// signature, invalid public key, or a recovered key mismatch.
pub fn verify_recoverable_digest(
    public_key: &[u8],
    signature: &[u8; 64],
    recovery_v: u8,
    digest: &[u8; 32],
) -> Result<(), VerifyError> {
    let recovery_id = recovery_v
        .checked_sub(27)
        .and_then(|value| RecoveryId::try_from(value).ok())
        .filter(|value| u8::from(*value) <= 1)
        .ok_or(VerifyError::BadSignature)?;
    let signature = Signature::from_slice(signature).map_err(|_| VerifyError::BadSignature)?;
    if signature.normalize_s().is_some() {
        return Err(VerifyError::BadSignature);
    }
    let expected =
        VerifyingKey::from_sec1_bytes(public_key).map_err(|_| VerifyError::BadSignature)?;
    let recovered = VerifyingKey::recover_from_prehash(digest, &signature, recovery_id)
        .map_err(|_| VerifyError::BadSignature)?;
    if recovered != expected {
        return Err(VerifyError::BadSignature);
    }
    Ok(())
}
