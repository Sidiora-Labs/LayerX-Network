//! Reference port demonstrating shared state: a CosmWasm order book.
//!
//! The contract tracks open orders that every trader can read and match,
//! demonstrating shared state mapped onto the program-shared namespace.

use crate::error::PortRefusal;
use crate::storage::{item_key, map_key, StateBinding};

/// The global order count, shared across all traders.
pub const ORDER_COUNT: &str = "order_count";
/// The order book map, shared across all traders.
pub const ORDERS: &str = "orders";
/// Per-trader position, principal-scoped.
pub const POSITIONS: &str = "positions";

/// An order book entry visible to every trader.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Order {
    /// The order identifier.
    pub id: u64,
    /// Whether the order is buying or selling.
    pub is_buy: bool,
    /// The price per unit.
    pub price: u128,
    /// The remaining quantity.
    pub quantity: u64,
}

/// A trader's position, principal-scoped.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Position {
    /// The trader's buy-side quantity.
    pub buy_quantity: u64,
    /// The trader's sell-side quantity.
    pub sell_quantity: u64,
}

/// Demonstrates the storage bindings for a shared order book.
pub struct OrderBook;

impl OrderBook {
    /// Returns the binding for the global order count.
    #[must_use]
    pub fn order_count_binding() -> StateBinding {
        StateBinding::Shared
    }

    /// Returns the binding for the order book map.
    #[must_use]
    pub fn orders_binding() -> StateBinding {
        StateBinding::Shared
    }

    /// Returns the binding for per-trader positions.
    #[must_use]
    pub fn positions_binding() -> StateBinding {
        StateBinding::SenderIndexed
    }

    /// Returns the storage key for the global order count in shared namespace.
    ///
    /// # Errors
    ///
    /// Refuses invalid namespace or key construction.
    pub fn order_count_key() -> Result<Vec<u8>, PortRefusal> {
        Self::order_count_binding().layerx_key(ORDER_COUNT, &[])
    }

    /// Returns the storage key for one order in the shared order book.
    ///
    /// # Errors
    ///
    /// Refuses invalid namespace or key construction.
    pub fn order_key(order_id: u64) -> Result<Vec<u8>, PortRefusal> {
        let id_bytes = order_id.to_le_bytes();
        map_key(ORDERS, &id_bytes)
    }

    /// Returns the storage key for one trader's position in principal-scoped
    /// namespace.
    ///
    /// # Errors
    ///
    /// Refuses invalid namespace or key construction.
    pub fn position_key() -> Result<Vec<u8>, PortRefusal> {
        Self::positions_binding().layerx_key(POSITIONS, &[])
    }

    /// Returns whether the order count addresses shared namespace.
    #[must_use]
    pub fn order_count_is_shared() -> bool {
        Self::order_count_binding().shared()
    }

    /// Returns whether the order book addresses shared namespace.
    #[must_use]
    pub fn orders_is_shared() -> bool {
        Self::orders_binding().shared()
    }

    /// Returns whether positions address shared namespace.
    #[must_use]
    pub fn positions_is_shared() -> bool {
        Self::positions_binding().shared()
    }

    /// Returns whether the order count is portable.
    #[must_use]
    pub fn order_count_portable() -> bool {
        Self::order_count_binding().portable()
    }

    /// Returns whether the order book is portable.
    #[must_use]
    pub fn orders_portable() -> bool {
        Self::orders_binding().portable()
    }

    /// Returns whether positions are portable.
    #[must_use]
    pub fn positions_portable() -> bool {
        Self::positions_binding().portable()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn order_count_is_shared() {
        assert_eq!(OrderBook::order_count_binding(), StateBinding::Shared);
        assert!(OrderBook::order_count_is_shared());
        assert!(OrderBook::order_count_portable());
    }

    #[test]
    fn orders_are_shared() {
        assert_eq!(OrderBook::orders_binding(), StateBinding::Shared);
        assert!(OrderBook::orders_is_shared());
        assert!(OrderBook::orders_portable());
    }

    #[test]
    fn positions_are_sender_indexed() {
        assert_eq!(OrderBook::positions_binding(), StateBinding::SenderIndexed);
        assert!(!OrderBook::positions_is_shared());
        assert!(OrderBook::positions_portable());
    }

    #[test]
    fn order_count_key_derivation() {
        let key = OrderBook::order_count_key().unwrap();
        // Item key is the namespace verbatim
        assert_eq!(key, ORDER_COUNT.as_bytes());
    }

    #[test]
    fn order_key_derivation() {
        let key = OrderBook::order_key(42).unwrap();
        // Map key is length-prefixed namespace + key
        assert!(!key.is_empty());
        // Verify it uses map_key composition
        let expected = map_key(ORDERS, &42u64.to_le_bytes()).unwrap();
        assert_eq!(key, expected);
    }

    #[test]
    fn position_key_collapses() {
        let key = OrderBook::position_key().unwrap();
        // SenderIndexed collapses onto the map prefix
        // which is length-prefixed namespace
        assert!(!key.is_empty());
    }

    #[test]
    fn all_bindings_portable() {
        // All three bindings are now portable, including shared state
        assert!(OrderBook::order_count_portable());
        assert!(OrderBook::orders_portable());
        assert!(OrderBook::positions_portable());
    }

    #[test]
    fn shared_binding_addresses_shared_namespace() {
        // Shared bindings explicitly indicate shared namespace
        let binding = StateBinding::Shared;
        assert!(binding.shared());
        assert!(binding.portable());
        
        // And produce valid keys
        let key = binding.layerx_key(ORDER_COUNT, &[]).unwrap();
        assert_eq!(key, ORDER_COUNT.as_bytes());
    }
}
