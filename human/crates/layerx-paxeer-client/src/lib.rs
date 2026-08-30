#![forbid(unsafe_code)]

mod client;
mod deposit;
mod exit;
mod finality;
mod json;
mod rpc;
mod status;
pub mod wire;
mod withdraw;

pub use client::{
    BlockRef, ClientConfigError, EndpointError, ExecutionOutcome, LogRecord, PaxeerClient,
    TransactionHash, TransactionHashError, TransactionInclusion, TransactionView,
};
pub use deposit::{
    account_address, deposit_leaf_bytes, deposit_root_registration_message, AgentCreditContext,
    CreditFault, CreditPath, CustodyDeposit, CustodyFault, DepositFailure, DepositProof,
    DepositProofConfig, DepositProofConfigError, DepositProofVerifier, DepositRootRegistration,
    ProofFault, PublishedDepositProof,
};
pub use exit::{
    balance_leaf, emergency_withdrawal_id, exit_nullifier, merkle_node, EmergencyExit, ExitClaim,
    ExitConfig, ExitConfigError, ExitEligibility, ExitError, ExitEvidence, ExitProgress,
    ExitRefusal, GuarantorAttestation,
};
pub use finality::{
    ChainSignal, ConfirmationProgress, EndpointSignal, FinalityReport, FinalityStage,
    FinalityTracker, TrackerConfig, TrackerConfigError,
};
pub use json::{parse as parse_json, Json, JsonError, JsonErrorReason};
pub use rpc::{raw_call, EndpointConfig, EndpointFailure, EndpointFault, EndpointTransport};
pub use status::{
    BoundaryHealth, BoundaryStatus, ChainStatus, ContractStatus, DelayExpectation, EndpointStatus,
};
pub use withdraw::{
    CancellationEvidence, CancelledFundsDisposition, ChallengeHold, ChallengeKind, CheckpointProof,
    ClaimProgress, ClaimRefusal, CommittedWithdrawalDebit, DebitExpectation, DebitFault,
    PaxeerFundsDisposition, PayoutEvidence, ProtocolDebitDisposition, SubmittedWithdrawalClaim,
    WithdrawalAttestation, WithdrawalBoundary, WithdrawalClaim, WithdrawalConfig,
    WithdrawalConfigError, WithdrawalError,
};

/// Stable identity of the Paxeer custody-boundary client.
pub const CRATE_IDENTITY: &str = "layerx-paxeer-client";
