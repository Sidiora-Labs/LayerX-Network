//! Guest-side bindings for the version-one `LayerX` programs ABI.
//!
//! A program compiles to WASM and imports the seven `layerx_v1` host functions
//! the runtime freezes. This crate binds each of them with types that make the
//! runtime's laws unrepresentable rather than merely discouraged:
//!
//! - money crosses the boundary only as [`Amount`], an exact unsigned 128-bit
//!   integer with no floating-point constructor, conversion or operator, and
//!   with every arithmetic operation checked;
//! - identifiers, storage keys, event topics, call inputs and capability sets
//!   check their bounds at construction, so a value the host would refuse
//!   cannot be built in the first place;
//! - authority is explicit: a capability set is ordered by the runtime's own
//!   authority key, refuses duplicates, and can only be narrowed on the way
//!   into another program.
//!
//! The crate is `no_std` and never allocates. Every host call that returns
//! bytes writes into a buffer the program declared itself, so a program's
//! memory ceiling is visible in its own source and stays inside the resource
//! budget the runtime meters.
//!
//! The bindings that actually cross into the host exist only for `wasm32`,
//! which is the single compiled target for programs. Building this crate for
//! any other target compiles the pure vocabulary - amounts, identifiers,
//! bounds, capability encoding and receipt decoding - and nothing that would
//! pretend to reach a runtime that is not there.

#![no_std]

#[cfg(test)]
extern crate std;

pub mod abi;
pub mod amount;
pub mod buffer;
pub mod call;
pub mod capability;
pub mod error;
pub mod event;
pub mod ids;
pub mod receipt;
pub mod storage;
pub mod transfer;

#[cfg(target_arch = "wasm32")]
pub mod entry;

#[cfg(target_arch = "wasm32")]
mod host;

mod macros;

pub use abi::{
    HostFunction, ABI_MANIFEST, ABI_MODULE, ABI_VERSION, CALL_ENTRY_EXPORT, CALL_RESERVE_EXPORT,
    CANDIDATE_ABI_MANIFEST, CANDIDATE_ABI_MODULE, CANDIDATE_HOST_FUNCTIONS,
    CANDIDATE_REFUSAL_SENTINEL, ENTRYPOINT, HOST_FUNCTIONS, MAX_CALL_INPUT_BYTES,
    MAX_CALL_RESPONSE_BYTES, MAX_CAPABILITIES, MAX_CAPABILITY_ENCODING_BYTES, MAX_EVENT_DATA_BYTES,
    MAX_EVENT_TOPIC_BYTES, MAX_REFUSAL_REASON_BYTES, MAX_STORAGE_KEY_BYTES,
    MAX_STORAGE_VALUE_BYTES, MAX_PROGRAM_ACCOUNT_SEED_BYTES, MEMORY_EXPORT, RECEIPT_ENCODING_BYTES,
};
pub use amount::{Amount, ProtocolInteger};
pub use buffer::Bytes;
pub use call::{CallInput, CallResponse, CallResult, GrantedCapabilities};
pub use capability::{Capability, CapabilitySet};
#[cfg(target_arch = "wasm32")]
pub use entry::EntryResponse;
pub use error::{
    Field, HostRefusal, ProgramError, ProgramRefusal, Reason, RefusalClass, RefusalReason,
    ValueError, REFUSAL_CLASS_MANIFEST, STATUS_BOUNDS, STATUS_BUFFER_TOO_SMALL,
    STATUS_CAPABILITY_BYTES, STATUS_CAPABILITY_LIMIT, STATUS_DATA_TOO_LARGE, STATUS_DENIED,
    STATUS_DUPLICATE_CAPABILITY, STATUS_EMPTY_KEY, STATUS_EMPTY_TOPIC, STATUS_EVIDENCE,
    STATUS_INPUT_TOO_LARGE, STATUS_INVALID, STATUS_KEY_TOO_LARGE, STATUS_METER,
    STATUS_NULL_ARGUMENT, STATUS_OVERFLOW, STATUS_RECEIPT_ENCODING, STATUS_RESERVED_IDENTIFIER,
    STATUS_TOPIC_TOO_LARGE, STATUS_UNDERFLOW, STATUS_VALUE_TOO_LARGE, STATUS_ZERO_AMOUNT,
};
pub use event::{EventData, EventTopic};
pub use ids::{AccountId, AssetId, ProgramId, ReceiptDigest};
pub use receipt::Receipt;
pub use storage::{StorageKey, StorageValue};
pub use transfer::{Payment, ProgramAccountPayment, ProgramAccountSeed, ProgramDeposit};

/// Returns the repository path of the escrow reference program shipped with
/// this SDK.
#[must_use]
pub const fn programs_escrow_reference() -> &'static str {
    "programs/sdk/rust/examples/escrow"
}

/// Returns the repository path of the pooled-vault reference program shipped
/// with this SDK.
#[must_use]
pub const fn programs_vault_reference() -> &'static str {
    "programs/sdk/rust/examples/vault"
}
