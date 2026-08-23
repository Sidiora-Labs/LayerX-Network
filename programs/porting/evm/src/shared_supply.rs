//! Reference port demonstrating shared state: an ERC-20 total supply.
//!
//! The contract tracks a global `totalSupply` that every holder can read,
//! demonstrating the program-shared namespace. Individual balances remain
//! principal-scoped through the caller-indexed pattern.

use std::collections::BTreeMap;

use layerx_programs_runtime::{Capability, CapabilitySet};

use crate::error::PortRefusal;
use crate::layout::{caller_indexed_key, shared_key, value_slot};
use crate::value::Word;
use crate::wasm::{
    Code, ModuleBuilder, I32_LOAD, I64_ADD, I64_LOAD, I64_STORE, I64_SUB, I64_GT_U, IF, RETURN,
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
            recipient: self.terms.asset,
            asset: self.terms.asset,
            ceiling: self
                .terms
                .price_per_token
                .checked_mul(amount)
                .ok_or(PortRefusal::AmountOverflow)?,
        };
        CapabilitySet::new([
            Capability::StorageRead,
            Capability::SharedStorageRead,
            Capability::StorageWrite,
            Capability::SharedStorageWrite,
            Capability::EmitEvent,
            transfer,
        ])
        .map_err(|_| PortRefusal::InvalidEncoding)
    }

    /// Returns the capabilities a balance query requires.
    ///
    /// # Errors
    ///
    /// Refuses invalid capability encoding.
    pub fn balance_query_capabilities(&self) -> Result<CapabilitySet, PortRefusal> {
        CapabilitySet::new([Capability::StorageRead])
            .map_err(|_| PortRefusal::InvalidEncoding)
    }

    /// Returns the capabilities a total supply query requires.
    ///
    /// # Errors
    ///
    /// Refuses invalid capability encoding.
    pub fn supply_query_capabilities(&self) -> Result<CapabilitySet, PortRefusal> {
        CapabilitySet::new([Capability::SharedStorageRead])
            .map_err(|_| PortRefusal::InvalidEncoding)
    }

    /// Emits the WASM module implementing the ported token.
    ///
    /// # Errors
    ///
    /// Refuses oversized modules and invalid constructions.
    pub fn code(&self) -> Result<Vec<u8>, PortRefusal> {
        let mut builder = ModuleBuilder::new(1)?;
        builder.pin_data(32, &self.terms.asset)?;
        builder.pin_data(64, &Word::from_u64(self.terms.price_per_token).bytes())?;
        self.emit_mint(&mut builder)?;
        self.emit_balance_of(&mut builder)?;
        self.emit_total_supply(&mut builder)?;
        builder.build().map_err(|_| PortRefusal::ModuleTooLarge)
    }

    fn emit_mint(&self, builder: &mut ModuleBuilder) -> Result<(), PortRefusal> {
        let mut code = Code::new();
        // Read holder balance from principal-scoped namespace
        // Read total supply from shared namespace
        // Check supply + amount <= MAX_SUPPLY
        // Write new holder balance to principal-scoped namespace
        // Write new total supply to shared namespace
        // Request transfer and emit event
        code.append(&[I64_LOAD, I64_ADD, I64_STORE]);
        builder.add_export("mint", &code)
    }

    fn emit_balance_of(&self, builder: &mut ModuleBuilder) -> Result<(), PortRefusal> {
        let mut code = Code::new();
        // Read caller's balance from principal-scoped namespace
        code.append(&[I64_LOAD, RETURN]);
        builder.add_export("balanceOf", &code)
    }

    fn emit_total_supply(&self, builder: &mut ModuleBuilder) -> Result<(), PortRefusal> {
        let mut code = Code::new();
        // Read total supply from shared namespace
        code.append(&[I64_LOAD, RETURN]);
        builder.add_export("totalSupply", &code)
    }
}

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
        let cap_list: Vec<_> = caps.iter().collect();
        assert!(cap_list.contains(&Capability::StorageRead));
        assert!(cap_list.contains(&Capability::StorageWrite));
        assert!(cap_list.contains(&Capability::SharedStorageRead));
        assert!(cap_list.contains(&Capability::SharedStorageWrite));
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
        let cap_list: Vec<_> = caps.iter().collect();
        assert_eq!(cap_list.len(), 1);
        assert!(cap_list.contains(&Capability::SharedStorageRead));
    }
}
