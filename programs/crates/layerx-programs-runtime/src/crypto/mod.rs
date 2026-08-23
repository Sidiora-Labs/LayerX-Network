//! Deterministic cryptographic primitives for guest programs.

pub mod hash;

pub use hash::{hash_bytes, HashAlgorithm};
