#![cfg_attr(not(feature = "std"), no_std)]

extern crate alloc;

mod export_globals;
pub mod gas_metering;
mod stack_limiter;

pub use export_globals::export_mutable_globals;
pub use parity_wasm;
pub use stack_limiter::inject as inject_stack_limiter;
