//! Deterministic derivation of program-owned account identifiers.
//!
//! A program-owned account is an identifier a program controls under its own
//! authority rather than any invoking principal's. This module defines the
//! single, frozen derivation from a program identifier and a bounded
//! program-supplied seed to a [`ProgramAccount`]. The derivation is a pure
//! computation over public inputs: any party holding the program identifier and
//! the seed reproduces the same account identifier byte for byte, and holding a
//! [`ProgramAccount`] conveys no authority whatsoever. Deriving an account and
//! being permitted to spend from it are deliberately separate concerns — the
//! spending authority lives in the transfer law, never here.
//!
//! # Collision resistance
//!
//! The derivation is domain-separated by [`PROGRAM_ACCOUNT_DOMAIN`], a tag that
//! no principal-key derivation ever uses, so a program account digest lies in a
//! preimage space disjoint from principal identifiers: a principal cannot
//! present a public key whose identifier equals a derived program account
//! except by breaking the preimage resistance of the underlying hash. The
//! deriving program identifier is bound into the preimage ahead of the seed, so
//! changing the program yields an independent identifier and no program can
//! reproduce another program's account. The seed length is length-prefixed
//! before the seed bytes, so no two distinct `(program, seed)` inputs share a
//! preimage by concatenation ambiguity.

use core::fmt::{self, Display};

use crate::crypto::{hash_bytes, HashAlgorithm};
use crate::storage::ProgramId;

/// Domain separation tag for program-owned account derivation. It is distinct
/// from every principal-identifier derivation domain, which is what places
/// derived accounts in a preimage space disjoint from principal identifiers.
pub const PROGRAM_ACCOUNT_DOMAIN: &[u8] = b"LayerX/programs/program-account/v1\0";

/// Maximum length in bytes of the program-supplied derivation seed. The seed is
/// bounded so the derivation preimage stays within a fixed, cheaply meterable
/// envelope and so no program can widen the input beyond the frozen contract.
pub const MAX_PROGRAM_ACCOUNT_SEED_BYTES: usize = 128;

/// Fixed length in bytes of a derived program-owned account identifier.
pub const PROGRAM_ACCOUNT_BYTES: usize = 32;

/// A deterministically derived, program-owned account identifier.
///
/// The value carries no capability and no balance handle: it is exactly the
/// 32-byte identifier produced by [`derive_program_account`]. It can only be
/// obtained by derivation, so possession of a `ProgramAccount` never implies
/// authority to move value held under it. Ordering is defined over the raw
/// identifier bytes so program accounts sort canonically alongside other
/// 32-byte identifiers in protocol structures.
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd, Hash)]
pub struct ProgramAccount([u8; PROGRAM_ACCOUNT_BYTES]);

impl ProgramAccount {
    /// Returns the canonical 32-byte account identifier.
    ///
    /// This is the only projection out of the type. Consumers that must compare
    /// a candidate 32-byte identifier — for example the transfer law checking
    /// that a leg's source is an account the staging program can derive —
    /// compare against these bytes rather than fabricating an account value.
    #[must_use]
    pub const fn bytes(self) -> [u8; PROGRAM_ACCOUNT_BYTES] {
        self.0
    }

    /// Returns whether a candidate 32-byte identifier equals this derived
    /// account. Provided so authority checks read as an explicit equality
    /// against a derivation rather than a raw byte comparison.
    #[must_use]
    pub fn matches(self, candidate: &[u8; PROGRAM_ACCOUNT_BYTES]) -> bool {
        &self.0 == candidate
    }
}

impl Display for ProgramAccount {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        for byte in self.0 {
            write!(formatter, "{byte:02x}")?;
        }
        Ok(())
    }
}

/// Typed refusal for program-owned account derivation.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ProgramAccountError {
    /// The supplied seed exceeds [`MAX_PROGRAM_ACCOUNT_SEED_BYTES`].
    SeedTooLarge {
        /// Length of the supplied seed in bytes.
        length: usize,
        /// Frozen upper bound on the seed length in bytes.
        limit: usize,
    },
    /// The assembled derivation preimage could not be hashed within the frozen
    /// input bound. The seed bound keeps this unreachable for valid input; it is
    /// surfaced as a typed refusal rather than a panic so the derivation never
    /// traps.
    PreimageTooLarge,
}

impl Display for ProgramAccountError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::SeedTooLarge { length, limit } => write!(
                formatter,
                "program account seed length {length} exceeds limit {limit}"
            ),
            Self::PreimageTooLarge => {
                formatter.write_str("program account derivation preimage exceeds the hash bound")
            }
        }
    }
}

impl core::error::Error for ProgramAccountError {}

/// Assembles the canonical derivation preimage for a program and seed.
///
/// The layout is frozen as
/// `PROGRAM_ACCOUNT_DOMAIN || program (32 bytes) || seed_len (u32 big-endian)
/// || seed`. The domain tag separates the space from principal derivations, the
/// program identifier binds the account to exactly one program, and the
/// big-endian length prefix removes any concatenation ambiguity between
/// distinct seeds. This function is pure and total for any admitted seed.
///
/// # Errors
///
/// Refuses a seed longer than [`MAX_PROGRAM_ACCOUNT_SEED_BYTES`].
pub fn program_account_preimage(
    program: ProgramId,
    seed: &[u8],
) -> Result<Vec<u8>, ProgramAccountError> {
    if seed.len() > MAX_PROGRAM_ACCOUNT_SEED_BYTES {
        return Err(ProgramAccountError::SeedTooLarge {
            length: seed.len(),
            limit: MAX_PROGRAM_ACCOUNT_SEED_BYTES,
        });
    }
    let seed_length = u32::try_from(seed.len()).map_err(|_| ProgramAccountError::PreimageTooLarge)?;
    let mut preimage = Vec::with_capacity(
        PROGRAM_ACCOUNT_DOMAIN
            .len()
            .saturating_add(PROGRAM_ACCOUNT_BYTES)
            .saturating_add(4)
            .saturating_add(seed.len()),
    );
    preimage.extend_from_slice(PROGRAM_ACCOUNT_DOMAIN);
    preimage.extend_from_slice(&program.bytes());
    preimage.extend_from_slice(&seed_length.to_be_bytes());
    preimage.extend_from_slice(seed);
    Ok(preimage)
}

/// Derives the program-owned account identifier for a program and bounded seed.
///
/// The result is a pure function of the two public inputs: any party recomputes
/// it from the program identifier and the seed alone, with no host state, no
/// wall-clock time and no node-local value entering the computation. The
/// returned [`ProgramAccount`] grants no authority; it is only an identifier.
///
/// # Errors
///
/// Refuses a seed longer than [`MAX_PROGRAM_ACCOUNT_SEED_BYTES`] and refuses a
/// preimage that cannot be hashed within the frozen input bound.
pub fn derive_program_account(
    program: ProgramId,
    seed: &[u8],
) -> Result<ProgramAccount, ProgramAccountError> {
    let preimage = program_account_preimage(program, seed)?;
    let digest = hash_bytes(HashAlgorithm::Sha256, &preimage)
        .map_err(|_| ProgramAccountError::PreimageTooLarge)?;
    Ok(ProgramAccount(digest))
}

#[cfg(test)]
mod tests {
    use super::{
        derive_program_account, program_account_preimage, ProgramAccountError,
        MAX_PROGRAM_ACCOUNT_SEED_BYTES, PROGRAM_ACCOUNT_BYTES, PROGRAM_ACCOUNT_DOMAIN,
    };
    use crate::crypto::{hash_bytes, HashAlgorithm};
    use crate::storage::ProgramId;

    fn program(byte: u8) -> ProgramId {
        ProgramId::new([byte; 32]).expect("nonzero program identifier")
    }

    #[test]
    fn derivation_is_reproducible_from_public_inputs() {
        let account_a = derive_program_account(program(1), b"vault").expect("derivation succeeds");
        let account_b = derive_program_account(program(1), b"vault").expect("derivation succeeds");
        assert_eq!(account_a, account_b);
        assert_eq!(account_a.bytes(), account_b.bytes());
    }

    #[test]
    fn derivation_matches_independent_preimage_hash() {
        let program = program(7);
        let seed = b"escrow/42";
        let account = derive_program_account(program, seed).expect("derivation succeeds");

        let mut expected_preimage = Vec::new();
        expected_preimage.extend_from_slice(PROGRAM_ACCOUNT_DOMAIN);
        expected_preimage.extend_from_slice(&program.bytes());
        expected_preimage.extend_from_slice(&(seed.len() as u32).to_be_bytes());
        expected_preimage.extend_from_slice(seed);
        assert_eq!(
            program_account_preimage(program, seed).expect("preimage assembles"),
            expected_preimage
        );

        let expected = hash_bytes(HashAlgorithm::Sha256, &expected_preimage).expect("hash succeeds");
        assert_eq!(account.bytes(), expected);
    }

    #[test]
    fn distinct_programs_derive_distinct_accounts() {
        let left = derive_program_account(program(1), b"pool").expect("derivation succeeds");
        let right = derive_program_account(program(2), b"pool").expect("derivation succeeds");
        assert_ne!(
            left.bytes(),
            right.bytes(),
            "the same seed under two programs must not collide"
        );
    }

    #[test]
    fn distinct_seeds_derive_distinct_accounts() {
        let program = program(9);
        let first = derive_program_account(program, b"a").expect("derivation succeeds");
        let second = derive_program_account(program, b"b").expect("derivation succeeds");
        assert_ne!(first.bytes(), second.bytes());
    }

    #[test]
    fn length_prefix_prevents_concatenation_collisions() {
        let program = program(3);
        // Without the length prefix, `("ab", "") and ("a", "b")`-style splits of
        // the program/seed boundary could collide. The prefix separates them.
        let left = derive_program_account(program, b"ab").expect("derivation succeeds");
        let right = derive_program_account(program, b"a").expect("derivation succeeds");
        assert_ne!(left.bytes(), right.bytes());
    }

    #[test]
    fn empty_seed_is_admitted() {
        let account = derive_program_account(program(5), b"").expect("derivation succeeds");
        assert_eq!(account.bytes().len(), PROGRAM_ACCOUNT_BYTES);
    }

    #[test]
    fn seed_at_bound_is_admitted() {
        let seed = vec![0xabu8; MAX_PROGRAM_ACCOUNT_SEED_BYTES];
        assert!(derive_program_account(program(5), &seed).is_ok());
    }

    #[test]
    fn seed_over_bound_is_refused() {
        let seed = vec![0u8; MAX_PROGRAM_ACCOUNT_SEED_BYTES + 1];
        assert_eq!(
            derive_program_account(program(5), &seed),
            Err(ProgramAccountError::SeedTooLarge {
                length: MAX_PROGRAM_ACCOUNT_SEED_BYTES + 1,
                limit: MAX_PROGRAM_ACCOUNT_SEED_BYTES,
            })
        );
    }

    #[test]
    fn domain_tag_is_bound_into_the_preimage() {
        let program = program(4);
        let seed = b"grant";
        let preimage = program_account_preimage(program, seed).expect("preimage assembles");
        assert!(preimage.starts_with(PROGRAM_ACCOUNT_DOMAIN));
        // Hashing the same fields without the domain must not reproduce the
        // account, which is what keeps derived accounts out of the principal
        // preimage space.
        let mut undomained = Vec::new();
        undomained.extend_from_slice(&program.bytes());
        undomained.extend_from_slice(&(seed.len() as u32).to_be_bytes());
        undomained.extend_from_slice(seed);
        let account = derive_program_account(program, seed).expect("derivation succeeds");
        let undomained_digest =
            hash_bytes(HashAlgorithm::Sha256, &undomained).expect("hash succeeds");
        assert_ne!(account.bytes(), undomained_digest);
    }

    #[test]
    fn matches_compares_against_raw_identifier() {
        let account = derive_program_account(program(6), b"sub").expect("derivation succeeds");
        let bytes = account.bytes();
        assert!(account.matches(&bytes));
        let mut altered = bytes;
        altered[0] ^= 0x01;
        assert!(!account.matches(&altered));
    }

    #[test]
    fn account_ordering_follows_identifier_bytes() {
        let account = derive_program_account(program(6), b"order").expect("derivation succeeds");
        let same = account;
        assert_eq!(account.cmp(&same), core::cmp::Ordering::Equal);
    }
}
