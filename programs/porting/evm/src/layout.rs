//! Solidity storage-slot semantics mapped onto namespaced program storage.
//!
//! The EVM addresses storage as one flat `2^256` word array per contract, so
//! Solidity derives an address for every composite variable with `keccak256`.
//! `LayerX` addresses storage as a byte-keyed map inside a namespace that is
//! already fixed to `(program, principal)` before guest code runs. This module
//! keeps both views: the exact EVM slot derivation, so an existing deployment's
//! state can be read cell-for-cell, and the collapsed key a ported program uses
//! once the principal is carried by the namespace instead of a mapping key.

use crate::error::PortRefusal;
use crate::keccak::keccak256;
use crate::value::{Address, Word};

/// The declared form of one Solidity state variable.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum StateVariable {
    /// A value type occupying its declared slot.
    Value {
        /// The declaration-order slot index.
        slot: u64,
    },
    /// A `mapping(K => V)` rooted at its declared slot.
    Mapping {
        /// The declaration-order slot index.
        slot: u64,
    },
    /// A dynamic array rooted at its declared slot.
    Array {
        /// The declaration-order slot index.
        slot: u64,
    },
}

impl StateVariable {
    /// Returns the declaration-order slot index of the variable.
    #[must_use]
    pub const fn slot(self) -> u64 {
        match self {
            Self::Value { slot } | Self::Mapping { slot } | Self::Array { slot } => slot,
        }
    }
}

/// Returns the slot address of a value-typed variable, which is its index.
#[must_use]
pub fn value_slot(slot: u64) -> Word {
    Word::from_u64(slot)
}

/// Returns `keccak256(key . slot)`, the EVM address of `mapping[key]`.
#[must_use]
pub fn mapping_slot(slot: u64, key: Word) -> Word {
    let mut preimage = [0u8; 64];
    preimage[..32].copy_from_slice(&key.bytes());
    preimage[32..].copy_from_slice(&Word::from_u64(slot).bytes());
    Word::from_bytes(keccak256(&preimage))
}

/// Returns the EVM address of a nested `mapping` value, applying the mapping
/// rule once per key from the outermost inwards.
///
/// # Errors
///
/// Refuses an empty key path, which does not name a value.
pub fn nested_mapping_slot(slot: u64, keys: &[Word]) -> Result<Word, PortRefusal> {
    let Some((first, rest)) = keys.split_first() else {
        return Err(PortRefusal::ArgumentCountMismatch);
    };
    let mut address = mapping_slot(slot, *first);
    for key in rest {
        let mut preimage = [0u8; 64];
        preimage[..32].copy_from_slice(&key.bytes());
        preimage[32..].copy_from_slice(&address.bytes());
        address = Word::from_bytes(keccak256(&preimage));
    }
    Ok(address)
}

/// Returns `keccak256(slot) + index`, the EVM address of `array[index]`.
#[must_use]
pub fn array_slot(slot: u64, index: u64) -> Word {
    let root = keccak256(&Word::from_u64(slot).bytes());
    Word::from_bytes(root).add_scalar(index)
}

/// Returns the address of a struct member at `offset` words from `base`.
#[must_use]
pub fn member_slot(base: Word, offset: u64) -> Word {
    base.add_scalar(offset)
}

/// Returns the namespaced-storage key a ported program uses for an EVM slot.
///
/// The key is the slot address itself: `LayerX` keys are arbitrary bounded byte
/// strings, so preserving the EVM address keeps an exported state dump
/// importable without re-deriving anything.
#[must_use]
pub fn storage_key(slot: Word) -> [u8; 32] {
    slot.bytes()
}

/// Returns the key a ported program uses for `mapping(address => V)` that is
/// only ever indexed by `msg.sender`.
///
/// Namespaced storage is already partitioned by principal, so the mapping key
/// carries no information the runtime has not already fixed. The mapping
/// collapses to its declared slot and the `keccak256` derivation disappears
/// from the hot path entirely.
#[must_use]
pub fn caller_indexed_key(slot: u64) -> [u8; 32] {
    storage_key(value_slot(slot))
}

/// Returns the key a ported program uses for state that is not caller-indexed
/// and therefore belongs in the shared namespace.
///
/// A value slot, a constant, or any mapping that does not collapse onto
/// `msg.sender` reaches the program-shared namespace `(program)` instead of
/// the principal-scoped namespace `(program, principal)`.
#[must_use]
pub fn shared_key(slot: Word) -> [u8; 32] {
    storage_key(slot)
}

/// One cell of an exported EVM state dump and the namespaced-storage cell it
/// becomes after the port.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct MigrationCell {
    /// The address the value occupies in the EVM contract's slot space.
    pub evm_slot: [u8; 32],
    /// The key the value occupies in `LayerX` namespaced storage.
    pub layerx_key: [u8; 32],
    /// The principal whose namespace holds the ported cell.
    pub principal: [u8; 32],
}

/// Builds the import plan for a `mapping(address => V)` collapsed onto the
/// per-principal namespace: one cell per holder, naming the EVM slot to read
/// and the namespaced key to write.
///
/// # Errors
///
/// Refuses the reserved zero principal, which owns no namespace.
pub fn caller_indexed_import(
    slot: u64,
    holders: &[(Address, [u8; 32])],
) -> Result<Vec<MigrationCell>, PortRefusal> {
    let mut plan = Vec::with_capacity(holders.len());
    for (holder, principal) in holders {
        if principal == &[0u8; 32] {
            return Err(PortRefusal::OutOfRange);
        }
        plan.push(MigrationCell {
            evm_slot: mapping_slot(slot, holder.word()).bytes(),
            layerx_key: caller_indexed_key(slot),
            principal: *principal,
        });
    }
    Ok(plan)
}
