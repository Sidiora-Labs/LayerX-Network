//! Canonical `LayerX` domain types shared by the interaction layer.

pub mod account;
pub mod activity;
pub mod amount;
pub mod batch;
pub mod checkpoint;
pub mod error;
pub mod ids;
pub mod intent;
pub mod json;
pub mod limits;
pub mod payload;
pub mod policy;
pub mod receipt;
pub mod result;
pub mod settlement;
pub mod test_support;
pub mod vectors;
pub mod verify;

/// Identifies the workspace manifest used by all interaction-layer crates.
#[must_use]
pub const fn agent_workspace_manifest() -> &'static str {
    "agent/Cargo.toml"
}

/// Identifies the compiler version pinned for reproducible agent builds.
#[must_use]
pub const fn agent_toolchain_pin() -> &'static str {
    "1.91.1"
}
