#![forbid(unsafe_code)]
//! Porting kit carrying an `EVM` contract onto the `LayerX` programs ABI.
//!
//! The kit keeps three things exact and refuses to fake the rest. Storage slot
//! addresses stay byte-identical, so an exported contract state dump imports
//! cell for cell. Event topics and four-byte selectors stay byte-identical, so
//! an existing indexer or client keeps matching. Everything the `EVM` model
//! assumes and `LayerX` does not provide - a contract-held balance, a clock,
//! ambient authority over another account's funds - is refused by name at
//! translation time instead of being emulated into something that looks like
//! the original but no longer means the same thing.
//!
//! The reference port in [`reference`] is a complete, runnable program: it
//! emits a real deterministic module, deploys through the real lifecycle, is
//! rebuilt from published source through the real reproducible-build pipeline
//! and executes under the real metered executor.

pub mod error;
pub mod hash;
pub mod keccak;
pub mod layout;
pub mod monetary;
pub mod qualify;
pub mod reference;
pub mod semantics;
pub mod shared_supply;
pub mod value;
pub mod wasm;

pub use error::PortRefusal;
pub use layout::{
    array_slot, caller_indexed_import, caller_indexed_key, mapping_slot, member_slot,
    nested_mapping_slot, shared_key, storage_key, value_slot, MigrationCell, StateVariable,
};
pub use monetary::{
    translate_all, ProgramAccountTransferPlan, Transfer402Plan, TranslatedValueFlow, ValueFlow,
};
pub use qualify::{
    build_plan, deploy_and_verify, execute_has_valid_key, execute_purchase,
    execute_remaining_periods, import_state, published_source, settle, source_archive,
    validated_module, AbsentReceipts, DeployedLock, Invocation, PortBuildRunner, Publication,
};
pub use reference::{LockTerms, PublicLockPort};
pub use semantics::{
    external_call, CallRequest, EventAbi, FailureMapping, MethodAbi, RuntimeOutcome,
};
pub use value::{Address, Word};

/// Identifies the `EVM` porting kit and the ABI version it targets.
#[must_use]
pub const fn programs_porting_evm() -> &'static str {
    "programs/porting/evm targeting layerx_v1 ABI version 1"
}
