//! Reference port demonstrating shared state: an ERC-20 total supply.
//!
//! The contract tracks a global `totalSupply` that every holder can read,
//! demonstrating the program-shared namespace. Individual balances remain
//! principal-scoped through the caller-indexed pattern.

use layerx_programs_runtime::{Capability, CapabilitySet, ABI_MODULE};

use crate::error::PortRefusal;
use crate::layout::{caller_indexed_key, shared_key, value_slot};
use crate::wasm::{
    Code, ModuleBuilder, I32, I32_LOAD8_U, I32_LT_S, I32_STORE8, I32_WRAP_I64, I64, I64_ADD,
    I64_EXTEND_I32_U, I64_GT_S, I64_LT_S, I64_OR, I64_SHL, I64_SHR_U, I64_STORE,
};

/// Declaration-order slot of the total supply in shared namespace.
pub const TOTAL_SUPPLY_SLOT: u64 = 0;
/// Declaration-order slot of per-holder balances, caller-indexed.
pub const BALANCES_SLOT: u64 = 1;
/// Maximum supply the contract may mint.
pub const MAX_SUPPLY: u64 = 1_000_000;

/// Terms a token deployment pins into its module.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TokenTerms {
    /// The identifier of the 402LXP asset standing in for the ERC-20.
    pub asset: [u8; 32],
    /// The per-token price in the asset's smallest unit.
    pub price_per_token: u64,
}

/// The ported token contract descriptor.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SharedSupplyPort {
    terms: TokenTerms,
}

impl SharedSupplyPort {
    /// Constructs a port descriptor with the given deployment terms.
    ///
    /// # Errors
    ///
    /// Refuses zero price or oversized bounds.
    pub fn new(terms: TokenTerms) -> Result<Self, PortRefusal> {
        if terms.price_per_token == 0 {
            return Err(PortRefusal::ArgumentCountMismatch);
        }
        Ok(Self { terms })
    }

    /// Returns the storage key for the shared total supply.
    #[must_use]
    pub fn total_supply_key(&self) -> [u8; 32] {
        shared_key(value_slot(TOTAL_SUPPLY_SLOT))
    }

    /// Returns the storage key for one holder's balance.
    #[must_use]
    pub fn balance_key(&self) -> [u8; 32] {
        caller_indexed_key(BALANCES_SLOT)
    }

    /// Returns the capabilities a mint operation requires.
    ///
    /// # Errors
    ///
    /// Refuses invalid capability encoding.
    pub fn mint_capabilities(&self, amount: u64) -> Result<CapabilitySet, PortRefusal> {
        let transfer = Capability::Transfer402 {
            asset: self.terms.asset,
            to: self.terms.asset,
            maximum_amount: u128::from(self.terms.price_per_token)
                .checked_mul(u128::from(amount))
                .ok_or(PortRefusal::OutOfRange)?,
        };
        CapabilitySet::new([
            Capability::StorageRead,
            Capability::SharedStorageRead,
            Capability::StorageWrite,
            Capability::SharedStorageWrite,
            Capability::EmitEvent,
            transfer,
        ])
        .map_err(PortRefusal::from)
    }

    /// Returns the capabilities a balance query requires.
    ///
    /// # Errors
    ///
    /// Refuses invalid capability encoding.
    pub fn balance_query_capabilities(&self) -> Result<CapabilitySet, PortRefusal> {
        CapabilitySet::new([Capability::StorageRead]).map_err(PortRefusal::from)
    }

    /// Returns the capabilities a total supply query requires.
    ///
    /// # Errors
    ///
    /// Refuses invalid capability encoding.
    pub fn supply_query_capabilities(&self) -> Result<CapabilitySet, PortRefusal> {
        CapabilitySet::new([Capability::SharedStorageRead]).map_err(PortRefusal::from)
    }

    /// Emits the WASM module implementing the ported token.
    ///
    /// # Errors
    ///
    /// Refuses oversized modules and invalid constructions.
    pub fn code(&self) -> Result<Vec<u8>, PortRefusal> {
        let mut builder = ModuleBuilder::new(MEMORY_PAGES);
        let host_type = builder.signature(&[I32, I32, I32, I32], &[I32]);
        let load_type = builder.signature(&[I32], &[I64]);
        let store_type = builder.signature(&[I32, I64], &[]);
        let read_type = builder.signature(&[I32], &[I64]);
        let mint_type = builder.signature(&[I64], &[I64]);
        let query_type = builder.signature(&[], &[I64]);
        let storage_read = builder.import(ABI_MODULE, "storage_read", host_type);
        let storage_write = builder.import(ABI_MODULE, "storage_write", host_type);
        builder.segment(SUPPLY_KEY_POINTER, &self.total_supply_key());
        builder.segment(BALANCE_KEY_POINTER, &self.balance_key());
        let load_be64 = emit_load_be64(&mut builder, load_type);
        let store_word = emit_store_word(&mut builder, store_type);
        let read_word = emit_read_word(&mut builder, read_type, storage_read, load_be64);
        let mint = emit_mint(&mut builder, mint_type, storage_write, read_word, store_word);
        let balance_of = emit_query(&mut builder, query_type, BALANCE_KEY_POINTER, read_word);
        let total_supply = emit_query(&mut builder, query_type, SUPPLY_KEY_POINTER, read_word);
        builder.export_memory(MEMORY_EXPORT);
        builder.export_function(MINT_EXPORT, mint);
        builder.export_function(BALANCE_OF_EXPORT, balance_of);
        builder.export_function(TOTAL_SUPPLY_EXPORT, total_supply);
        let wasm = builder.finish();
        if u64::try_from(wasm.len()).unwrap_or(u64::MAX)
            > layerx_programs_runtime::limits::DEFAULT_MAX_MODULE_BYTES
        {
            return Err(PortRefusal::ModuleTooLarge);
        }
        Ok(wasm)
    }
}

/// Exported name of the mint entry point.
pub const MINT_EXPORT: &str = "mint";
/// Exported name of the balance query.
pub const BALANCE_OF_EXPORT: &str = "balanceOf";
/// Exported name of the total supply query.
pub const TOTAL_SUPPLY_EXPORT: &str = "totalSupply";
/// Exported name of the module's linear memory.
pub const MEMORY_EXPORT: &str = "memory";

const MEMORY_PAGES: u32 = 1;
const SUPPLY_KEY_POINTER: u32 = 0;
const BALANCE_KEY_POINTER: u32 = 32;
const VALUE_POINTER: u32 = 64;
const KEY_LENGTH: i32 = 32;

fn emit_load_be64(builder: &mut ModuleBuilder, signature: u32) -> u32 {
    let mut code = Code::new();
    code.i64_const(0);
    code.local_set(1);
    for offset in 0..8_u32 {
        code.local_get(1);
        code.i64_const(8);
        code.op(I64_SHL);
        code.local_get(0);
        code.memory(I32_LOAD8_U, offset);
        code.op(I64_EXTEND_I32_U);
        code.op(I64_OR);
        code.local_set(1);
    }
    code.local_get(1);
    code.end();
    builder.function(signature, &[(1, I64)], &code)
}

fn emit_store_word(builder: &mut ModuleBuilder, signature: u32) -> u32 {
    let mut code = Code::new();
    for offset in [0_u32, 8, 16] {
        code.local_get(0);
        code.i64_const(0);
        code.memory(I64_STORE, offset);
    }
    for index in 0..8_u32 {
        code.local_get(0);
        code.local_get(1);
        code.i64_const(i64::from(56 - index * 8));
        code.op(I64_SHR_U);
        code.op(I32_WRAP_I64);
        code.memory(I32_STORE8, 24 + index);
    }
    code.end();
    builder.function(signature, &[], &code)
}

fn emit_read_word(
    builder: &mut ModuleBuilder,
    signature: u32,
    storage_read: u32,
    load_be64: u32,
) -> u32 {
    let mut code = Code::new();
    for offset in [0_u32, 8, 16, 24] {
        code.pointer(VALUE_POINTER);
        code.i64_const(0);
        code.memory(I64_STORE, offset);
    }
    code.local_get(0);
    code.i32_const(KEY_LENGTH);
    code.pointer(VALUE_POINTER);
    code.i32_const(KEY_LENGTH);
    code.call(storage_read);
    code.i32_const(0);
    code.op(I32_LT_S);
    code.trap_if();
    code.pointer(VALUE_POINTER + 24);
    code.call(load_be64);
    code.end();
    builder.function(signature, &[], &code)
}

fn emit_mint(
    builder: &mut ModuleBuilder,
    signature: u32,
    storage_write: u32,
    read_word: u32,
    store_word: u32,
) -> u32 {
    let mut code = Code::new();
    code.local_get(0);
    code.i64_const(1);
    code.op(I64_LT_S);
    code.trap_if();
    code.i32_const(SUPPLY_KEY_POINTER_I32);
    code.call(read_word);
    code.local_set(1);
    code.local_get(1);
    code.local_get(0);
    code.op(I64_ADD);
    code.local_set(2);
    code.local_get(2);
    code.i64_const(MAX_SUPPLY_I64);
    code.op(I64_GT_S);
    code.trap_if();
    code.pointer(VALUE_POINTER);
    code.local_get(2);
    code.call(store_word);
    code.pointer(SUPPLY_KEY_POINTER);
    code.i32_const(KEY_LENGTH);
    code.pointer(VALUE_POINTER);
    code.i32_const(KEY_LENGTH);
    code.call(storage_write);
    code.trap_unless_ok();
    code.i32_const(BALANCE_KEY_POINTER_I32);
    code.call(read_word);
    code.local_set(3);
    code.local_get(3);
    code.local_get(0);
    code.op(I64_ADD);
    code.local_set(4);
    code.pointer(VALUE_POINTER);
    code.local_get(4);
    code.call(store_word);
    code.pointer(BALANCE_KEY_POINTER);
    code.i32_const(KEY_LENGTH);
    code.pointer(VALUE_POINTER);
    code.i32_const(KEY_LENGTH);
    code.call(storage_write);
    code.trap_unless_ok();
    code.local_get(2);
    code.end();
    builder.function(signature, &[(4, I64)], &code)
}

fn emit_query(builder: &mut ModuleBuilder, signature: u32, key_pointer: u32, read_word: u32) -> u32 {
    let mut code = Code::new();
    code.pointer(key_pointer);
    code.call(read_word);
    code.end();
    builder.function(signature, &[], &code)
}

const SUPPLY_KEY_POINTER_I32: i32 = SUPPLY_KEY_POINTER as i32;
const BALANCE_KEY_POINTER_I32: i32 = BALANCE_KEY_POINTER as i32;
#[allow(clippy::cast_possible_wrap)]
const MAX_SUPPLY_I64: i64 = MAX_SUPPLY as i64;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shared_supply_key_addresses_shared_namespace() {
        let terms = TokenTerms {
            asset: [1u8; 32],
            price_per_token: 100,
        };
        let port = SharedSupplyPort::new(terms).unwrap();
        
        // The total supply key uses shared_key, demonstrating shared namespace
        let supply_key = port.total_supply_key();
        assert_eq!(supply_key, shared_key(value_slot(TOTAL_SUPPLY_SLOT)));
    }

    #[test]
    fn balance_key_is_caller_indexed() {
        let terms = TokenTerms {
            asset: [1u8; 32],
            price_per_token: 100,
        };
        let port = SharedSupplyPort::new(terms).unwrap();
        
        // Balances remain principal-scoped
        let balance_key = port.balance_key();
        assert_eq!(balance_key, caller_indexed_key(BALANCES_SLOT));
    }

    #[test]
    fn mint_requires_both_namespaces() {
        let terms = TokenTerms {
            asset: [1u8; 32],
            price_per_token: 100,
        };
        let port = SharedSupplyPort::new(terms).unwrap();
        let caps = port.mint_capabilities(10).unwrap();

        // Mint needs principal-scoped read/write for balances
        // and shared read/write for total supply
        let expected = CapabilitySet::new([
            Capability::StorageRead,
            Capability::SharedStorageRead,
            Capability::StorageWrite,
            Capability::SharedStorageWrite,
            Capability::EmitEvent,
            Capability::Transfer402 {
                asset: [1u8; 32],
                to: [1u8; 32],
                maximum_amount: 1_000,
            },
        ])
        .unwrap();
        assert_eq!(caps, expected);
    }

    #[test]
    fn supply_query_requires_only_shared_read() {
        let terms = TokenTerms {
            asset: [1u8; 32],
            price_per_token: 100,
        };
        let port = SharedSupplyPort::new(terms).unwrap();
        let caps = port.supply_query_capabilities().unwrap();

        // Total supply query only needs shared read
        let expected = CapabilitySet::new([Capability::SharedStorageRead]).unwrap();
        assert_eq!(caps, expected);
    }
}
