//! Deterministic hash primitives covering sha256, keccak256 and blake3.

use core::fmt::{self, Display};

/// Maximum input length per hash call to bound resource consumption.
pub const MAX_HASH_INPUT_BYTES: u64 = 1_048_576;

/// Supported deterministic hash algorithms.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HashAlgorithm {
    Sha256,
    Keccak256,
    Blake3,
}

impl HashAlgorithm {
    /// Returns the algorithm for the given identifier, refusing unknown values.
    pub const fn from_identifier(id: u32) -> Result<Self, HashRefusal> {
        match id {
            1 => Ok(Self::Sha256),
            2 => Ok(Self::Keccak256),
            3 => Ok(Self::Blake3),
            _ => Err(HashRefusal::UnknownAlgorithm { id }),
        }
    }

    /// Returns the fuel cost coefficient per input byte for metering.
    #[must_use]
    pub const fn fuel_per_byte(self) -> u64 {
        match self {
            Self::Sha256 => 2,
            Self::Keccak256 => 3,
            Self::Blake3 => 1,
        }
    }

    /// Returns the fixed output length in bytes.
    #[must_use]
    pub const fn output_length(self) -> usize {
        match self {
            Self::Sha256 | Self::Keccak256 | Self::Blake3 => 32,
        }
    }

    const fn as_str(self) -> &'static str {
        match self {
            Self::Sha256 => "sha256",
            Self::Keccak256 => "keccak256",
            Self::Blake3 => "blake3",
        }
    }
}

impl Display for HashAlgorithm {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "{}", self.as_str())
    }
}

/// Typed refusal for hash operations.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HashRefusal {
    /// Unknown algorithm identifier.
    UnknownAlgorithm { id: u32 },
    /// Input exceeds the per-call length bound.
    InputTooLong { length: u64, limit: u64 },
}

impl Display for HashRefusal {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::UnknownAlgorithm { id } => write!(formatter, "unknown hash algorithm {id}"),
            Self::InputTooLong { length, limit } => {
                write!(
                    formatter,
                    "hash input length {length} exceeds limit {limit}"
                )
            }
        }
    }
}

impl core::error::Error for HashRefusal {}

/// Computes a deterministic hash digest using the specified algorithm.
///
/// # Errors
///
/// Returns a refusal when the input exceeds the length bound.
pub fn hash_bytes(
    algorithm: HashAlgorithm,
    input: &[u8],
) -> Result<[u8; 32], HashRefusal> {
    let length = u64::try_from(input.len()).unwrap_or(u64::MAX);
    if length > MAX_HASH_INPUT_BYTES {
        return Err(HashRefusal::InputTooLong {
            length,
            limit: MAX_HASH_INPUT_BYTES,
        });
    }
    Ok(match algorithm {
        HashAlgorithm::Sha256 => sha256(input),
        HashAlgorithm::Keccak256 => keccak256(input),
        HashAlgorithm::Blake3 => blake3(input),
    })
}

fn sha256(input: &[u8]) -> [u8; 32] {
    use sha2::Digest;
    let mut hasher = sha2::Sha256::new();
    hasher.update(input);
    hasher.finalize().into()
}

fn keccak256(input: &[u8]) -> [u8; 32] {
    use sha3::Digest;
    let mut hasher = sha3::Keccak256::new();
    hasher.update(input);
    hasher.finalize().into()
}

fn blake3(input: &[u8]) -> [u8; 32] {
    blake3::hash(input).into()
}

#[cfg(test)]
mod tests {
    use super::{hash_bytes, HashAlgorithm, HashRefusal, MAX_HASH_INPUT_BYTES};

    #[test]
    fn sha256_empty_input() {
        let result = hash_bytes(HashAlgorithm::Sha256, b"");
        assert!(result.is_ok());
        let digest = result.unwrap();
        assert_eq!(digest.len(), 32);
        assert_eq!(
            &digest[..],
            &[
                0xe3, 0xb0, 0xc4, 0x42, 0x98, 0xfc, 0x1c, 0x14, 0x9a, 0xfb, 0xf4, 0xc8, 0x99, 0x6f,
                0xb9, 0x24, 0x27, 0xae, 0x41, 0xe4, 0x64, 0x9b, 0x93, 0x4c, 0xa4, 0x95, 0x99, 0x1b,
                0x78, 0x52, 0xb8, 0x55
            ]
        );
    }

    #[test]
    fn sha256_abc() {
        let result = hash_bytes(HashAlgorithm::Sha256, b"abc");
        assert!(result.is_ok());
        let digest = result.unwrap();
        assert_eq!(
            &digest[..],
            &[
                0xba, 0x78, 0x16, 0xbf, 0x8f, 0x01, 0xcf, 0xea, 0x41, 0x41, 0x40, 0xde, 0x5d, 0xae,
                0x22, 0x23, 0xb0, 0x03, 0x61, 0xa3, 0x96, 0x17, 0x7a, 0x9c, 0xb4, 0x10, 0xff, 0x61,
                0xf2, 0x00, 0x15, 0xad
            ]
        );
    }

    #[test]
    fn keccak256_empty_input() {
        let result = hash_bytes(HashAlgorithm::Keccak256, b"");
        assert!(result.is_ok());
        let digest = result.unwrap();
        assert_eq!(digest.len(), 32);
        assert_eq!(
            &digest[..],
            &[
                0xc5, 0xd2, 0x46, 0x01, 0x86, 0xf7, 0x23, 0x3c, 0x92, 0x7e, 0x7d, 0xb2, 0xdc, 0xc7,
                0x03, 0xc0, 0xe5, 0x00, 0xb6, 0x53, 0xca, 0x82, 0x27, 0x3b, 0x7b, 0xfa, 0xd8, 0x04,
                0x5d, 0x85, 0xa4, 0x70
            ]
        );
    }

    #[test]
    fn keccak256_abc() {
        let result = hash_bytes(HashAlgorithm::Keccak256, b"abc");
        assert!(result.is_ok());
        let digest = result.unwrap();
        assert_eq!(
            &digest[..],
            &[
                0x4e, 0x03, 0x65, 0x7a, 0xea, 0x45, 0xa9, 0x4f, 0xc7, 0xd4, 0x7b, 0xa8, 0x26, 0xc8,
                0xd6, 0x67, 0xc0, 0xd1, 0xe6, 0xe3, 0x3a, 0x64, 0xa0, 0x36, 0xec, 0x44, 0xf5, 0x8f,
                0xa1, 0x2d, 0x6c, 0x45
            ]
        );
    }

    #[test]
    fn blake3_empty_input() {
        let result = hash_bytes(HashAlgorithm::Blake3, b"");
        assert!(result.is_ok());
        let digest = result.unwrap();
        assert_eq!(digest.len(), 32);
        assert_eq!(
            &digest[..],
            &[
                0xaf, 0x13, 0x49, 0xb9, 0xf5, 0xf9, 0xa1, 0xa6, 0xa0, 0x40, 0x4d, 0xea, 0x36, 0xdc,
                0xc9, 0x49, 0x9b, 0xcb, 0x25, 0xc9, 0xad, 0xc1, 0x12, 0xb7, 0xcc, 0x9a, 0x93, 0xca,
                0xe4, 0x1f, 0x32, 0x62
            ]
        );
    }

    #[test]
    fn blake3_abc() {
        let result = hash_bytes(HashAlgorithm::Blake3, b"abc");
        assert!(result.is_ok());
        let digest = result.unwrap();
        assert_eq!(
            &digest[..],
            &[
                0x64, 0x37, 0xb3, 0xac, 0x38, 0x46, 0x51, 0x33, 0xff, 0xb6, 0x3b, 0x75, 0x27, 0x3a,
                0x8d, 0xb5, 0x48, 0xc5, 0x58, 0x46, 0x5d, 0x79, 0xdb, 0x03, 0xfd, 0x35, 0x9c, 0x6c,
                0xd5, 0xbd, 0x9d, 0x85
            ]
        );
    }

    #[test]
    fn unknown_algorithm_refuses() {
        assert_eq!(
            HashAlgorithm::from_identifier(0),
            Err(HashRefusal::UnknownAlgorithm { id: 0 })
        );
        assert_eq!(
            HashAlgorithm::from_identifier(999),
            Err(HashRefusal::UnknownAlgorithm { id: 999 })
        );
    }

    #[test]
    fn input_length_bound_is_enforced() {
        let oversized = vec![0u8; (MAX_HASH_INPUT_BYTES + 1) as usize];
        for algorithm in [
            HashAlgorithm::Sha256,
            HashAlgorithm::Keccak256,
            HashAlgorithm::Blake3,
        ] {
            assert_eq!(
                hash_bytes(algorithm, &oversized),
                Err(HashRefusal::InputTooLong {
                    length: MAX_HASH_INPUT_BYTES + 1,
                    limit: MAX_HASH_INPUT_BYTES
                })
            );
        }
    }

    #[test]
    fn fuel_coefficients_reflect_real_cost() {
        assert_eq!(HashAlgorithm::Sha256.fuel_per_byte(), 2);
        assert_eq!(HashAlgorithm::Keccak256.fuel_per_byte(), 3);
        assert_eq!(HashAlgorithm::Blake3.fuel_per_byte(), 1);
    }

    #[test]
    fn output_lengths_are_correct() {
        assert_eq!(HashAlgorithm::Sha256.output_length(), 32);
        assert_eq!(HashAlgorithm::Keccak256.output_length(), 32);
        assert_eq!(HashAlgorithm::Blake3.output_length(), 32);
    }
}
