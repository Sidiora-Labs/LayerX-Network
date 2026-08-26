#![forbid(unsafe_code)]
//! Porting kit carrying a `CosmWasm` contract onto the `LayerX` programs ABI.
//!
//! The kit keeps three things exact and refuses to fake the rest. Raw storage
//! keys stay byte-identical - the `cw-storage-plus` framing of an `Item` and of
//! a `Map` prefix is reproduced exactly - so an exported state dump can be
//! located key for key. `JSON` message and event names stay byte-identical, so
//! an existing client or indexer keeps matching. Everything the chain model
//! assumes and `LayerX` does not provide - a contract-held bank balance, a
//! contract that spends somebody else's allowance, a `Deps::querier` round trip
//! into another module, state shared across senders - is refused by name at
//! translation time instead of being emulated into something that looks like
//! the original but no longer means the same thing.
//!
//! `JSON` itself stops at the edge. A client still sends `{"donate":{...}}`
//! and a migration still reads the documents `cw-storage-plus` wrote, but the
//! running program moves canonically framed bytes, because a deterministic
//! module has no serde and no allocator to parse a document with.
//!
//! The reference port in [`reference`] is a complete, runnable program: it
//! emits a real deterministic module, deploys through the real lifecycle, is
//! rebuilt from published source through the real reproducible-build pipeline
//! and executes under the real metered executor.

pub mod error;
pub mod hash;
pub mod json;
pub mod messages;
pub mod monetary;
pub mod qualify;
pub mod reference;
pub mod shared_orderbook;
pub mod storage;
pub mod wasm;

pub use error::PortRefusal;
pub use json::{FieldSchema, FieldValue, RecordSchema, ValueType};
pub use messages::{
    execute_submessage, variant_tag, CallRequest, ContractEvent, EntryPoint, FailureMapping,
    MessageVariant, RuntimeOutcome,
};
pub use monetary::{
    translate_all, ProgramAccountTransferPlan, Transfer402Plan, TranslatedValueFlow, ValueFlow,
};
pub use qualify::{
    build_plan, deploy_and_verify, execute_donate, execute_donations, execute_remaining,
    import_state, published_source, settle, source_archive, validated_module, AbsentReceipts,
    DeployedContract, Invocation, PortBuildRunner, Publication,
};
pub use reference::{DonationPort, DonationTerms};
pub use storage::{
    composite_map_key, item_key, map_key, map_prefix, sender_indexed_import, MigrationCell,
    StateBinding, StateHolder,
};

/// Identifies the `CosmWasm` porting kit and the ABI version it targets.
#[must_use]
pub const fn programs_porting_cosmwasm() -> &'static str {
    "programs/porting/cosmwasm targeting layerx_v2 ABI version 2"
}
