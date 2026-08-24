//! Proof-gated daemon read surfaces.

#[path = "availability.rs"]
mod available;
#[path = "balance.rs"]
mod balances;
#[path = "checkpoint.rs"]
mod checkpoint_impl;
#[path = "history.rs"]
mod historical;
#[path = "program_balances.rs"]
mod program_balances_impl;
#[path = "program_node.rs"]
mod program_node;

pub use available::{
    availability, AvailabilityAudit, AvailabilityFailure, AvailabilityRead, AvailabilityRequest,
    ReplayFraming,
};
pub use balances::{balance, BalanceRead, Freshness};
pub use checkpoint_impl::{
    proof_bundle, CheckpointReadError, GuarantorSignature, HeaderCommitments, ProofBundleKind,
    ProofBundleRequest, ServedCheckpoint, ServedProofBundle,
};
pub use program_balances_impl::{
    program_balances_from_protocol, ProgramBalanceFreshness, ProgramBalanceRead,
    ProgramValueBalance, ProtocolProgramBalanceReader,
};
pub use program_node::{LayerxdProgramBalanceReader, ProgramBalanceReadRoute};

/// Verifies and serves one checkpoint certificate.
///
/// # Errors
///
/// Returns `AvailabilityUnavailable` when availability was not obtained, the certificate failure
/// when the bonded set does not meet threshold, and `Header` when the batch header does not
/// decode.
pub fn checkpoint(
    certificate: &layerx_proof::checkpoint::Certificate,
    bonded_set: &[layerx_proof::checkpoint::GuarantorKey],
    registered_checkpoint_id: [u8; 32],
    registered_settlement_reference: Option<&[u8]>,
    availability_obtained: bool,
) -> Result<ServedCheckpoint, CheckpointReadError> {
    checkpoint_impl::serve_checkpoint(
        certificate,
        bonded_set,
        registered_checkpoint_id,
        registered_settlement_reference,
        availability_obtained,
    )
}
pub use historical::{history, Cursor, HistoryLimits, HistoryPage, HistoryReadError};
