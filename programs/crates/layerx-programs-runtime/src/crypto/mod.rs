//! Deterministic cryptographic and wide-integer primitives.

pub mod bigint;
pub mod hash;
pub mod signature;

pub use hash::{hash_bytes, HashAlgorithm, HashRefusal};
pub use signature::{
    recover_secp256k1, verify_ed25519, verify_secp256k1, SignatureAlgorithm, SignatureRefusal,
    ED25519_PUBLIC_KEY_BYTES, ED25519_SIGNATURE_BYTES, MAX_MESSAGE_DIGEST_BYTES,
    SECP256K1_COMPRESSED_PUBLIC_KEY_BYTES, SECP256K1_SIGNATURE_BYTES,
    SECP256K1_UNCOMPRESSED_PUBLIC_KEY_BYTES,
};
