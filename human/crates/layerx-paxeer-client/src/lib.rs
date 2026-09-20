#![forbid(unsafe_code)]

mod client;
pub mod custody;
mod deposit;
mod exit;
mod finality;
mod json;
mod native_custody;
mod rpc;
mod status;
pub mod wire;
mod withdraw;

pub use client::{
    BlockRef, ClientConfigError, EndpointError, ExecutionOutcome, LogRecord, PaxeerClient,
    TransactionHash, TransactionHashError, TransactionInclusion, TransactionView,
};
pub use custody::{
    CustodyAbiError, CustodyAsset, CustodyClaim, ForcedExitMaterial, WithdrawalMaterial,
    CUSTODY_PRECOMPILE, WEI_PER_BASE_UNIT,
};
pub use deposit::{
    account_address, account_address_for_protocol, deposit_leaf_bytes,
    deposit_root_registration_message, AccountAddressError, AdmittedCustody, AgentCreditContext,
    CreditFault, CreditPath, CustodyDeposit, CustodyFault, DepositFailure, DepositProof,
    DepositProofConfig, DepositProofConfigError, DepositProofVerifier, DepositRootRegistration,
    ProofFault, PublishedDepositProof,
};
pub use exit::{
    verify_exit_balance, EmergencyExit, ExitClaim, ExitConfig, ExitConfigError, ExitEligibility,
    ExitError, ExitEvidence, ExitProgress, ExitRefusal,
};
pub use finality::{
    ChainSignal, ConfirmationProgress, EndpointSignal, FinalityReport, FinalityStage,
    FinalityTracker, TrackerConfig, TrackerConfigError,
};
pub use json::{parse as parse_json, Json, JsonError, JsonErrorReason};
pub use native_custody::{
    AttestedNativeCustodyCredit, NativeCustodyError, NativeCustodyEvidence,
    NativeCustodyExpectation,
};
pub use rpc::{raw_call, EndpointConfig, EndpointFailure, EndpointFault, EndpointTransport};
pub use status::{
    BoundaryHealth, BoundaryStatus, ChainStatus, ContractStatus, DelayExpectation, EndpointStatus,
};
pub use withdraw::{
    CancellationEvidence, CancelledFundsDisposition, ClaimProgress, ClaimRefusal,
    CommittedWithdrawalDebit, DebitExpectation, DebitFault, PaxeerFundsDisposition, PayoutEvidence,
    ProtocolDebitDisposition, SubmittedWithdrawalClaim, WithdrawalBoundary, WithdrawalClaim,
    WithdrawalConfig, WithdrawalConfigError, WithdrawalError, ANCHOR_PRECOMPILE,
};

/// Stable identity of the Paxeer custody-boundary client.
pub const CRATE_IDENTITY: &str = "layerx-paxeer-client";

pub mod state_proof;
