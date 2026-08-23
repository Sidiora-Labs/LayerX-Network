//! Deterministic signature verification and public-key recovery primitives.

use core::fmt::{self, Display};

use ed25519_dalek::{Signature as Ed25519Signature, Verifier, VerifyingKey as Ed25519PublicKey};
use k256::ecdsa::{
    RecoveryId, Signature as Secp256k1Signature, VerifyingKey as Secp256k1VerifyingKey,
};

type VerifyingKey = Secp256k1VerifyingKey;

/// Fixed size of an Ed25519 public key in bytes.
pub const ED25519_PUBLIC_KEY_BYTES: usize = 32;
/// Fixed size of an Ed25519 signature in bytes.
pub const ED25519_SIGNATURE_BYTES: usize = 64;
/// Fixed size of a secp256k1 compressed public key in bytes.
pub const SECP256K1_COMPRESSED_PUBLIC_KEY_BYTES: usize = 33;
/// Fixed size of a secp256k1 uncompressed public key in bytes.
pub const SECP256K1_UNCOMPRESSED_PUBLIC_KEY_BYTES: usize = 65;
/// Fixed size of a secp256k1 signature in bytes (r || s).
pub const SECP256K1_SIGNATURE_BYTES: usize = 64;
/// Maximum size of a message digest for signature verification.
pub const MAX_MESSAGE_DIGEST_BYTES: usize = 64;

/// Enumeration of supported signature algorithms.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[repr(u32)]
pub enum SignatureAlgorithm {
    /// Ed25519 signature verification.
    Ed25519 = 1,
    /// secp256k1 signature verification (ECDSA).
    Secp256k1Verify = 2,
    /// secp256k1 public key recovery from signature.
    Secp256k1Recover = 3,
}

impl SignatureAlgorithm {
    /// Decodes the algorithm from its numeric identifier.
    ///
    /// # Errors
    ///
    /// Returns `InvalidAlgorithm` for unknown algorithm identifiers.
    pub const fn decode(raw: u32) -> Result<Self, SignatureRefusal> {
        match raw {
            1 => Ok(Self::Ed25519),
            2 => Ok(Self::Secp256k1Verify),
            3 => Ok(Self::Secp256k1Recover),
            _ => Err(SignatureRefusal::InvalidAlgorithm),
        }
    }

    /// Returns the metering coefficient for this algorithm.
    #[must_use]
    pub const fn fuel_coefficient(self) -> u64 {
        match self {
            Self::Ed25519 => 2_000,
            Self::Secp256k1Verify => 3_000,
            Self::Secp256k1Recover => 3_500,
        }
    }
}

impl Display for SignatureAlgorithm {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Ed25519 => write!(f, "ed25519"),
            Self::Secp256k1Verify => write!(f, "secp256k1-verify"),
            Self::Secp256k1Recover => write!(f, "secp256k1-recover"),
        }
    }
}

/// Typed refusal for signature verification operations.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SignatureRefusal {
    /// The algorithm identifier is not recognized.
    InvalidAlgorithm,
    /// The message digest length is invalid for the operation.
    InvalidMessageLength,
    /// The public key is malformed or has invalid length.
    MalformedPublicKey,
    /// The signature is malformed or has invalid length.
    MalformedSignature,
    /// The recovery identifier is out of range for secp256k1.
    InvalidRecoveryId,
    /// The signature verification failed.
    VerificationFailed,
    /// Public key recovery failed.
    RecoveryFailed,
}

impl Display for SignatureRefusal {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidAlgorithm => write!(f, "invalid signature algorithm identifier"),
            Self::InvalidMessageLength => write!(f, "invalid message digest length"),
            Self::MalformedPublicKey => write!(f, "malformed public key"),
            Self::MalformedSignature => write!(f, "malformed signature"),
            Self::InvalidRecoveryId => write!(f, "invalid recovery identifier"),
            Self::VerificationFailed => write!(f, "signature verification failed"),
            Self::RecoveryFailed => write!(f, "public key recovery failed"),
        }
    }
}

impl std::error::Error for SignatureRefusal {}

/// Verifies an Ed25519 signature with constant-time execution.
///
/// # Errors
///
/// Returns `SignatureRefusal` for malformed inputs or verification failure.
pub fn verify_ed25519(
    message: &[u8],
    public_key: &[u8],
    signature: &[u8],
) -> Result<(), SignatureRefusal> {
    if message.len() > MAX_MESSAGE_DIGEST_BYTES {
        return Err(SignatureRefusal::InvalidMessageLength);
    }
    if public_key.len() != ED25519_PUBLIC_KEY_BYTES {
        return Err(SignatureRefusal::MalformedPublicKey);
    }
    if signature.len() != ED25519_SIGNATURE_BYTES {
        return Err(SignatureRefusal::MalformedSignature);
    }

    ed25519_verify_impl(message, public_key, signature)
}

/// Verifies a secp256k1 ECDSA signature with constant-time execution.
///
/// # Errors
///
/// Returns `SignatureRefusal` for malformed inputs or verification failure.
pub fn verify_secp256k1(
    message_digest: &[u8],
    public_key: &[u8],
    signature: &[u8],
) -> Result<(), SignatureRefusal> {
    if message_digest.len() != 32 {
        return Err(SignatureRefusal::InvalidMessageLength);
    }
    if public_key.len() != SECP256K1_COMPRESSED_PUBLIC_KEY_BYTES
        && public_key.len() != SECP256K1_UNCOMPRESSED_PUBLIC_KEY_BYTES
    {
        return Err(SignatureRefusal::MalformedPublicKey);
    }
    if signature.len() != SECP256K1_SIGNATURE_BYTES {
        return Err(SignatureRefusal::MalformedSignature);
    }

    secp256k1_verify_impl(message_digest, public_key, signature)
}

/// Recovers a secp256k1 public key from a signature and message digest.
///
/// # Errors
///
/// Returns `SignatureRefusal` for malformed inputs or recovery failure.
pub fn recover_secp256k1(
    message_digest: &[u8],
    signature: &[u8],
    recovery_id: u8,
) -> Result<[u8; SECP256K1_UNCOMPRESSED_PUBLIC_KEY_BYTES], SignatureRefusal> {
    if message_digest.len() != 32 {
        return Err(SignatureRefusal::InvalidMessageLength);
    }
    if signature.len() != SECP256K1_SIGNATURE_BYTES {
        return Err(SignatureRefusal::MalformedSignature);
    }
    if recovery_id > 3 {
        return Err(SignatureRefusal::InvalidRecoveryId);
    }

    secp256k1_recover_impl(message_digest, signature, recovery_id)
}

/// Constant-time Ed25519 verification implementation.
///
/// Uses ed25519-dalek with constant-time execution. The verification path does not
/// branch on secret-bearing inputs and uses no floating-point operations.
///
/// # Errors
///
/// Returns `MalformedPublicKey` for invalid public keys, `MalformedSignature` for
/// invalid signatures, or `VerificationFailed` when verification fails.
fn ed25519_verify_impl(
    message: &[u8],
    public_key: &[u8],
    signature: &[u8],
) -> Result<(), SignatureRefusal> {
    let public_key_array: [u8; ED25519_PUBLIC_KEY_BYTES] = public_key
        .try_into()
        .map_err(|_| SignatureRefusal::MalformedPublicKey)?;

    let verifying_key = Ed25519PublicKey::from_bytes(&public_key_array)
        .map_err(|_| SignatureRefusal::MalformedPublicKey)?;

    let signature_array: [u8; ED25519_SIGNATURE_BYTES] = signature
        .try_into()
        .map_err(|_| SignatureRefusal::MalformedSignature)?;

    let signature = Ed25519Signature::from_bytes(&signature_array);

    verifying_key
        .verify(message, &signature)
        .map_err(|_| SignatureRefusal::VerificationFailed)
}

/// Constant-time secp256k1 ECDSA verification implementation.
///
/// Uses k256 with constant-time execution. Accepts both compressed (33 bytes) and
/// uncompressed (65 bytes) public keys. The verification path uses constant-time
/// field arithmetic with no floating-point operations.
///
/// # Errors
///
/// Returns `MalformedPublicKey` for invalid public keys, `MalformedSignature` for
/// invalid signatures, or `VerificationFailed` when verification fails.
fn secp256k1_verify_impl(
    message_digest: &[u8],
    public_key: &[u8],
    signature: &[u8],
) -> Result<(), SignatureRefusal> {
    let signature_array: [u8; SECP256K1_SIGNATURE_BYTES] = signature
        .try_into()
        .map_err(|_| SignatureRefusal::MalformedSignature)?;

    let signature = Secp256k1Signature::from_slice(&signature_array)
        .map_err(|_| SignatureRefusal::MalformedSignature)?;

    let verifying_key = if public_key.len() == SECP256K1_COMPRESSED_PUBLIC_KEY_BYTES {
        Secp256k1VerifyingKey::from_sec1_bytes(public_key)
            .map_err(|_| SignatureRefusal::MalformedPublicKey)?
    } else if public_key.len() == SECP256K1_UNCOMPRESSED_PUBLIC_KEY_BYTES {
        Secp256k1VerifyingKey::from_sec1_bytes(public_key)
            .map_err(|_| SignatureRefusal::MalformedPublicKey)?
    } else {
        return Err(SignatureRefusal::MalformedPublicKey);
    };

    use k256::ecdsa::signature::Verifier;
    verifying_key
        .verify(message_digest, &signature)
        .map_err(|_| SignatureRefusal::VerificationFailed)
}

/// Constant-time secp256k1 public key recovery implementation.
///
/// Uses k256 with constant-time execution to recover the public key from a signature
/// and message digest. Returns the uncompressed (65-byte) public key. The recovery
/// path uses constant-time field arithmetic with no floating-point operations.
///
/// # Errors
///
/// Returns `MalformedSignature` for invalid signatures, `InvalidRecoveryId` for
/// out-of-range recovery identifiers, or `RecoveryFailed` when recovery fails.
fn secp256k1_recover_impl(
    message_digest: &[u8],
    signature: &[u8],
    recovery_id: u8,
) -> Result<[u8; SECP256K1_UNCOMPRESSED_PUBLIC_KEY_BYTES], SignatureRefusal> {
    let signature_array: [u8; SECP256K1_SIGNATURE_BYTES] = signature
        .try_into()
        .map_err(|_| SignatureRefusal::MalformedSignature)?;

    let signature = Secp256k1Signature::from_slice(&signature_array)
        .map_err(|_| SignatureRefusal::MalformedSignature)?;

    let recovery_id =
        RecoveryId::try_from(recovery_id).map_err(|_| SignatureRefusal::InvalidRecoveryId)?;

    let recovered_key = VerifyingKey::recover_from_prehash(message_digest, &signature, recovery_id)
        .map_err(|_| SignatureRefusal::RecoveryFailed)?;

    let encoded = recovered_key.to_encoded_point(false);
    let bytes = encoded.as_bytes();

    if bytes.len() != SECP256K1_UNCOMPRESSED_PUBLIC_KEY_BYTES {
        return Err(SignatureRefusal::RecoveryFailed);
    }

    let mut result = [0u8; SECP256K1_UNCOMPRESSED_PUBLIC_KEY_BYTES];
    result.copy_from_slice(bytes);
    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn algorithm_decode_roundtrip() {
        assert_eq!(
            SignatureAlgorithm::decode(1).ok(),
            Some(SignatureAlgorithm::Ed25519)
        );
        assert_eq!(
            SignatureAlgorithm::decode(2).ok(),
            Some(SignatureAlgorithm::Secp256k1Verify)
        );
        assert_eq!(
            SignatureAlgorithm::decode(3).ok(),
            Some(SignatureAlgorithm::Secp256k1Recover)
        );
        assert_eq!(
            SignatureAlgorithm::decode(99).err(),
            Some(SignatureRefusal::InvalidAlgorithm)
        );
    }

    #[test]
    fn ed25519_rejects_malformed_inputs() {
        let message = [0u8; 32];
        let public_key = [0u8; ED25519_PUBLIC_KEY_BYTES];
        let signature = [0u8; ED25519_SIGNATURE_BYTES];

        assert_eq!(
            verify_ed25519(&message, &public_key[..31], &signature).err(),
            Some(SignatureRefusal::MalformedPublicKey)
        );
        assert_eq!(
            verify_ed25519(&message, &public_key, &signature[..63]).err(),
            Some(SignatureRefusal::MalformedSignature)
        );
        assert_eq!(
            verify_ed25519(&[0u8; MAX_MESSAGE_DIGEST_BYTES + 1], &public_key, &signature).err(),
            Some(SignatureRefusal::InvalidMessageLength)
        );
    }

    #[test]
    fn secp256k1_verify_rejects_malformed_inputs() {
        let digest = [0u8; 32];
        let public_key = [0u8; SECP256K1_COMPRESSED_PUBLIC_KEY_BYTES];
        let signature = [0u8; SECP256K1_SIGNATURE_BYTES];

        assert_eq!(
            verify_secp256k1(&digest[..31], &public_key, &signature).err(),
            Some(SignatureRefusal::InvalidMessageLength)
        );
        assert_eq!(
            verify_secp256k1(&digest, &public_key[..32], &signature).err(),
            Some(SignatureRefusal::MalformedPublicKey)
        );
        assert_eq!(
            verify_secp256k1(&digest, &public_key, &signature[..63]).err(),
            Some(SignatureRefusal::MalformedSignature)
        );
    }

    #[test]
    fn secp256k1_recover_rejects_malformed_inputs() {
        let digest = [0u8; 32];
        let signature = [0u8; SECP256K1_SIGNATURE_BYTES];

        assert_eq!(
            recover_secp256k1(&digest[..31], &signature, 0).err(),
            Some(SignatureRefusal::InvalidMessageLength)
        );
        assert_eq!(
            recover_secp256k1(&digest, &signature[..63], 0).err(),
            Some(SignatureRefusal::MalformedSignature)
        );
        assert_eq!(
            recover_secp256k1(&digest, &signature, 4).err(),
            Some(SignatureRefusal::InvalidRecoveryId)
        );
    }

    #[test]
    fn fuel_coefficients_are_ordered() {
        let ed25519 = SignatureAlgorithm::Ed25519.fuel_coefficient();
        let verify = SignatureAlgorithm::Secp256k1Verify.fuel_coefficient();
        let recover = SignatureAlgorithm::Secp256k1Recover.fuel_coefficient();

        assert!(ed25519 < verify);
        assert!(verify < recover);
    }
}
