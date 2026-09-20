//! The native state-witness surface a forced exit is proven against.
//!
//! Forced-exit evidence itself is [`crate::ForcedExitMaterial`]: the witness
//! bytes, the finalized batch they open under and the account authority's
//! signature over the recipient.

pub use layerx_proof::state_witness::{AccountPath, StateProofError, StateWitness};
