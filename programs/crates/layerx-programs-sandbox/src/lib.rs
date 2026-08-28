//! Protocol-state models for bounded, ephemeral program sandboxes.

#![deny(unsafe_code)]

pub mod lease;
pub mod escrow;

pub use escrow::{fund, settle, Escrow, EscrowOutcome, EscrowRefusal};

pub use lease::{
    usage_observation_digest, BoundKind, EphemeralNamespace, Lease, LeaseActivity, LeaseBook, LeaseId, LeaseLimits,
    LeaseRefusal, LeaseState, LeaseStateWitness, LeaseTransition, LeaseTransitionReceipt, LeaseUsage, TransitionEvidence,
    TransitionOutcome, UsageOutcome, MAX_CONCURRENT_LEASES_PER_PRINCIPAL, MAX_LEASE_CPU_FUEL,
    MAX_LEASE_ESCROW, MAX_LEASE_LIFETIME_BATCHES, MAX_LEASE_MEMORY_BYTES,
    MAX_LEASE_NAMESPACE_BYTES, MAX_LEASE_OUTPUT_BYTES, MAX_LEASE_OUTPUT_VALUES,
    MAX_LEASE_STORAGE_READ_BYTES, MAX_LEASE_STORAGE_WRITE_BYTES, MAX_LEASE_TABLE_ELEMENTS,
};
