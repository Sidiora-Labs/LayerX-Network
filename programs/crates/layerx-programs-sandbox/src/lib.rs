//! Protocol-state models for bounded, ephemeral program sandboxes.

#![deny(unsafe_code)]

pub mod activity;
pub mod escrow;
pub mod execute;
pub mod expiry;
#[cfg(feature = "host-ffi")]
#[allow(unsafe_code)]
mod host_ffi;
pub mod lease;
pub mod snapshot;
pub mod usage;

#[cfg(feature = "host-ffi")]
#[inline(never)]
pub fn retain_host_ffi_exports() {
    let exports = [
        host_ffi::layerx_programs_sandbox_admit_host as *const (),
        host_ffi::layerx_programs_sandbox_final_reserve_host as *const (),
        host_ffi::layerx_programs_sandbox_cancel_host as *const (),
        host_ffi::layerx_programs_sandbox_settle_call_rust as *const (),
        host_ffi::layerx_programs_sandbox_lifecycle_validate as *const (),
    ];
    let _ = std::hint::black_box(exports);
}

pub use activity::{
    activate_lease, canonical_activate, canonical_execute, canonical_fund,
    canonical_submission_digest, execute, fund_lease, SandboxActivityPlane, SandboxActivityRefusal,
    SandboxExecuteRefusal, SandboxProtocolContext, SandboxReceiptEvidence, SandboxSubmission,
    SandboxSubmissionReceipt, PROGRAMS_SANDBOX_ACTIVITY_TYPE,
};

pub use escrow::{settle, Escrow, EscrowOutcome, EscrowRefusal};
pub use execute::{
    LeaseCapabilities, SandboxExecutionRecord, SandboxExecutionRequest, SandboxRefusal,
};
pub use expiry::{
    destroy, sweep, AuthenticatedTerminalRecord, ExpiryQueue, ExpiryRefusal, SweepEvidence,
    SweepPage, TerminalLeaseRecord, TerminalReceiptEvidence, MAX_SWEEP_LEASES_PER_BATCH,
};
pub use usage::{
    ActivityOutcome, AuthenticatedUsageReceipt, DurableUsageState, UsageLedger, UsageObservation,
    UsagePrices, UsageReceipt, UsageRefusal, MAX_USAGE_RECEIPTS, MAX_USAGE_STATE_VALUE_BYTES,
};

pub use lease::{
    usage_observation_digest, BoundKind, EphemeralNamespace, Lease, LeaseActivity, LeaseBook,
    LeaseId, LeaseLimits, LeaseRefusal, LeaseSnapshotRecord, LeaseState, LeaseStateWitness,
    LeaseTransition, LeaseTransitionReceipt, LeaseUsage, TransitionEvidence, TransitionOutcome,
    UsageOutcome, MAX_CONCURRENT_LEASES_PER_PRINCIPAL, MAX_LEASE_CPU_FUEL, MAX_LEASE_ESCROW,
    MAX_LEASE_LIFETIME_BATCHES, MAX_LEASE_MEMORY_BYTES, MAX_LEASE_NAMESPACE_BYTES,
    MAX_LEASE_OUTPUT_BYTES, MAX_LEASE_OUTPUT_VALUES, MAX_LEASE_STORAGE_READ_BYTES,
    MAX_LEASE_STORAGE_WRITE_BYTES, MAX_LEASE_TABLE_ELEMENTS,
};
pub use snapshot::{
    restore, CapturedSnapshot, ContinuationPoint, NamespaceCell, RestoredSandbox, SandboxState,
    Snapshot, SnapshotRefusal, MAX_SANDBOX_GLOBALS, MAX_SANDBOX_LINEAR_MEMORY_BYTES,
    MAX_SANDBOX_NAMESPACE_CELLS, MAX_SANDBOX_OPERAND_STACK,
};
