//! Deterministic WASM runtime foundation for `LayerX` guest programs.
//!
//! # Module map
//!
//! Caller-declared activity ceilings and their consumed admission token live in
//! [`budget`]. The [`abi`] transaction boundary lives in `abi/mod.rs`; capability grants,
//! encoding, and narrowing live in `abi/capability.rs`; candidate response and
//! refusal transport lives in `abi/response.rs`; and namespaced storage
//! operations live in `abi/storage_ops.rs`. The private `host/mod.rs` owns
//! linker orchestration and `RuntimeState`, while `host/memory.rs` owns guest
//! memory access and `host/{storage,events,calls,transfer}.rs` each register
//! exactly one host-function family. New capability families belong in their
//! own ABI and host units and reach execution state only through `RuntimeState`.

#[deny(unsafe_code)]
pub mod abi;
#[deny(unsafe_code)]
pub mod accounts;
#[deny(unsafe_code)]
pub mod budget;
#[deny(unsafe_code)]
pub mod calls;
#[deny(unsafe_code)]
pub mod crypto;
#[deny(unsafe_code)]
pub mod engine;
#[deny(unsafe_code)]
pub mod entrypoint;
#[deny(unsafe_code)]
pub mod execute;
#[deny(unsafe_code)]
pub mod fault;
#[allow(unsafe_code)]
mod ffi;
#[allow(unsafe_code)]
mod ffi_call;
mod ffi_transfer;
#[deny(unsafe_code)]
mod host;
#[deny(unsafe_code)]
pub mod lifecycle;
#[deny(unsafe_code)]
pub mod limits;
#[deny(unsafe_code)]
pub mod meter;
#[deny(unsafe_code)]
pub mod occupancy;
#[deny(unsafe_code)]
pub mod qualification;
#[deny(unsafe_code)]
pub mod storage;
#[deny(unsafe_code)]
pub mod test_support;
#[deny(unsafe_code)]
pub mod transfer;
#[deny(unsafe_code)]
pub mod validate;

pub use abi::response::{CallResponse, ResponseRefusal, MAX_CALL_RESPONSE_BYTES};
pub use accounts::{
    derive_program_account, program_account_preimage, ProgramAccount, ProgramAccountError,
    MAX_PROGRAM_ACCOUNT_SEED_BYTES, PROGRAM_ACCOUNT_BYTES, PROGRAM_ACCOUNT_DOMAIN,
};
pub use budget::{
    ActivityBudgetBinding, AdmittedBudget, BudgetAdmissionRefusal, BudgetDimension, DeclaredBudget,
    DECLARED_BUDGET_DOMAIN,
};
pub use calls::{
    call_admission_fuel, CallEdge, CallFrame, CallGraph, CompositionContext, CompositionRefusal,
    CompositionRules, ProgramCatalog, ProgramResolver, CALL_ADMISSION_FUEL, CALL_ENTRY_EXPORT,
    CALL_INPUT_FUEL_PER_BYTE, CALL_RESERVE_EXPORT, DEFAULT_MAX_CALL_FANOUT,
    DEFAULT_MAX_CALL_GRAPH_EDGES, DEFAULT_MAX_COMPOSITION_DEPTH, DEFAULT_MAX_PROGRAM_VISITS,
};
pub use crypto::{hash_bytes, HashAlgorithm};
pub use engine::{EngineRefusal, WasmEngine};
pub use entrypoint::EntrypointRefusal;
pub use execute::{
    AuthorizedExecutionRecord, AuthorizedExecutionRequest, BudgetedAuthorizedExecutionRequest,
    BudgetedResourceFailureRecord, BudgetedV1ActivityOutcome, BudgetedV1FailureCause,
    BudgetedV1FailureRecord, CandidateActivityOutcome, CandidateActivityReceipt,
    CandidateAuthorizedExecutionRecord, CandidateExecutionRecord, CandidateReceiptOutcome,
    ExecutionError, ExecutionFault, ExecutionRecord, Executor, PreparedAuthorizedActivity,
    PreparedAuthorizedActivityOutcome, PreparedMonetarySummary, PreparedTransferLegSummary,
    ProgramInstance, SettlementFailure, VerifiedStorageAssignment, WasmValue, ABI_VERSION,
    RUNTIME_VERSION,
};
pub use fault::{
    FailureEncodingError, ProgramFailure, RefusalClass, RefusalReason, CANDIDATE_REFUSAL_SENTINEL,
    MAX_REFUSAL_REASON_BYTES, REFUSAL_CLASS_MANIFEST,
};
pub use lifecycle::{
    CodeHash, Deploy, DeploymentReceipt, DiagnosticArtifact, Lifecycle, LifecycleRefusal,
    Migration, ProgramVersion, Upgrade, UpgradePolicy,
};
pub use limits::{DeclaredLimit, LimitsRefusal, ValidationLimits};
pub use meter::{
    BudgetMeterRefusal, BudgetResourceKind, FeeSchedule, Meter, MeterRefusal, MeteredUsage,
    ResourceBudget, ResourceKind,
};
pub use occupancy::{
    OccupancyCharge, OccupancyError, OccupancyLedger, OccupancyResponsibility, OccupancySettlement,
    OccupancyUsage, PreparedOccupancySettlement,
};
pub use qualification::{
    programs_differential_gate, programs_fuzz_observation, programs_fuzz_targets,
    replay_recorded_execution, DifferentialMismatch, FuzzTarget, RecordedExecution, ReplayRefusal,
};
pub use storage::{
    NamespaceDrop, PrincipalId, ProgramId, ScanEntry, ScanLimits, Storage, StorageError,
    StorageNamespace, StorageScan,
};
pub use transfer::{
    AtomicTransferSet, KernelTransferEvidence, KernelTransferPrimitive, TransferCapability,
    TransferLawError, VerifiedProgramSettlement,
};
pub use validate::{AbiRevision, ValidatedModule, ValidationRefusal};

/// Identifies the workspace manifest that governs every programs-plane crate.
#[must_use]
pub const fn programs_workspace_manifest() -> &'static str {
    "programs/Cargo.toml"
}

/// Identifies the vendored deterministic WASM engine and its pinned revision.
#[must_use]
pub const fn programs_wasm_engine() -> &'static str {
    "wasmi 0.31.2 vendored at programs/vendor/wasmi"
}
pub use abi::{
    Abi, AbiCommit, AbiEffects, AbiError, AuthorizationContext, CallFrameId, Capability,
    CapabilitySet, HostFunction, ProgramCall, ProgramEvent, ReceiptOracle, ReceiptView,
    StorageSelector, TransferRequest, ABI_MANIFEST, ABI_MODULE, HOST_FUNCTIONS,
};
pub use crypto::{
    recover_secp256k1, verify_ed25519, verify_secp256k1, SignatureAlgorithm, SignatureRefusal,
    ED25519_PUBLIC_KEY_BYTES, ED25519_SIGNATURE_BYTES, MAX_MESSAGE_DIGEST_BYTES,
    SECP256K1_COMPRESSED_PUBLIC_KEY_BYTES, SECP256K1_SIGNATURE_BYTES,
    SECP256K1_UNCOMPRESSED_PUBLIC_KEY_BYTES,
};
pub use crypto::bigint::{WideIntegerOp, WideIntegerRefusal, WideIntegerRefusalReason};
