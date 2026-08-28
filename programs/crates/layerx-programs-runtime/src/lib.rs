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
pub mod access;
#[deny(unsafe_code)]
pub mod budget;
#[deny(unsafe_code)]
pub mod cache;
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
#[cfg(feature = "host-ffi")]
#[allow(unsafe_code)]
mod ffi;
#[cfg(feature = "host-ffi")]
#[allow(unsafe_code)]
mod ffi_call;
#[cfg(feature = "host-ffi")]
#[allow(unsafe_code)]
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
pub mod schedule;
#[deny(unsafe_code)]
pub mod storage;
#[deny(unsafe_code)]
pub mod test_support;
#[deny(unsafe_code)]
pub mod transfer;
#[deny(unsafe_code)]
pub mod validate;

/// Current ABI revision recorded in new execution evidence.
pub const ABI_VERSION: u16 = 2;
/// Canonical manifest bytes for the current ABI revision.
pub const ABI_MANIFEST: &str = "layerx_v1\0storage_read(i32,i32,i32,i32)->i32\0storage_write(i32,i32,i32,i32)->i32\0storage_delete(i32,i32)->i32\0event_emit(i32,i32,i32,i32)->i32\0program_call(i32,i32,i32,i32,i32,i32)->i32\0transfer_402(i64,i64,i32,i32,i32,i32)->i32\0receipt_read(i32,i32,i32,i32)->i32\0layerx_v2\0response_write(i32,i32,i32)->i32\0program_call_response(i32,i32,i32,i32,i32,i32,i32,i32)->i64\0refusal_write(i32,i32,i32)->i32\0storage_read_scoped(i32,i32,i32,i32,i32)->i32\0storage_write_scoped(i32,i32,i32,i32,i32)->i32\0storage_delete_scoped(i32,i32,i32)->i32\0storage_drop_scoped(i32)->i32\0storage_scan_scoped(i32,i32,i32,i32,i32,i32,i32,i32,i32)->i32\0transfer_program_402(i64,i64,i32,i32,i32,i32,i32,i32,i32,i32)->i32\0fund_program_402(i64,i64,i32,i32,i32,i32,i32,i32)->i32\0context_read(i32,i32,i32)->i32\0balance_read(i32,i32,i32,i32,i32,i32)->i32\0hash(i32,i32,i32,i32)->i32\0signature_verify(i32,i32,i32,i32,i32,i32,i32)->i32\0signature_recover(i32,i32,i32,i32,i32,i32,i32)->i32\0bigint_mul_256(i32,i32,i32,i32,i32,i32)->i32\0bigint_div_256(i32,i32,i32,i32,i32,i32)->i32\0bigint_rem_256(i32,i32,i32,i32,i32,i32)->i32\0bigint_modexp_256(i32,i32,i32,i32,i32,i32,i32,i32)->i32\0";

/// Returns the ABI revision recorded in new execution evidence.
#[must_use]
pub const fn current_abi_version() -> u16 {
    ABI_VERSION
}

/// Returns the canonical manifest bytes for the current ABI revision.
#[must_use]
pub const fn current_abi_manifest() -> &'static str {
    ABI_MANIFEST
}

pub use abi::context::{ContextField, ContextRefusal, ExecutionContext};
pub use abi::response::{CallResponse, ResponseRefusal, MAX_CALL_RESPONSE_BYTES};
pub use accounts::{
    derive_program_account, program_account_preimage, ProgramAccount, ProgramAccountError,
    MAX_PROGRAM_ACCOUNT_SEED_BYTES, PROGRAM_ACCOUNT_BYTES, PROGRAM_ACCOUNT_DOMAIN,
};
pub use access::{
    AccessCharge, AccessDeclaration, AccessMode, AccessRefusal, AccessSet, AccessSetBuilder,
    AccountAccess, KeyAccess, StorageAccess, ACCESS_DECLARATION_DOMAIN, ACCESS_SET_DOMAIN,
    MAX_ACCESS_ACCOUNT_ENTRIES, MAX_ACCESS_DECLARATION_BYTES, MAX_ACCESS_SET_BYTES,
    MAX_ACCESS_STORAGE_ENTRIES, MAX_ACCESS_CALLEE_ENTRIES,
};
pub use budget::{
    ActivityBudgetBinding, AdmittedBudget, BudgetAdmissionRefusal, BudgetDimension, DeclaredBudget,
    DECLARED_BUDGET_DOMAIN,
};
pub use cache::{
    CompiledModule, CompiledModuleRefusal, ModuleCache, ModuleCacheKey, ModuleCacheLimits,
    ModuleCacheLimitsRefusal, RuntimeArtifactOwner, RuntimeArtifactOwnerRefusal,
    COMPILED_FUNCTION_WEIGHT_BYTES, COMPILED_MODULE_BASE_WEIGHT_BYTES,
    DEFAULT_MAX_CACHED_MODULE_BYTES, DEFAULT_MAX_CACHED_MODULES,
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
    ProgramInstance, SettlementFailure, VerifiedStorageAssignment, WasmValue, RUNTIME_VERSION,
    V2ActivityOutcome, V2ActivityReceipt, V2AuthorizedExecutionRecord,
    V2ExecutionRecord, V2ReceiptOutcome,
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
    BudgetMeterRefusal, BudgetResourceKind, DemandPriceAdjustment, DemandPricePolicy,
    FeeGovernance, FeeSchedule, FeeScheduleError, FeeScheduleHistory, Meter, MeterRefusal, MeteredUsage,
    ResourceBudget, ResourceKind,
};
pub use meter::inject::{FuelSchedule, InjectionRefusal, MeterInjection};
pub use occupancy::{
    OccupancyCharge, OccupancyDisposition, OccupancyError, OccupancyLedger,
    OccupancyResponsibility, OccupancySettlement, OccupancyUsage, PreparedOccupancySettlement,
    MAX_OCCUPANCY_EVIDENCE_BYTES, MAX_OCCUPANCY_LEDGER_BYTES, MAX_OCCUPANCY_POSITIONS,
};
pub use qualification::{
    programs_differential_gate, programs_differential_gate_versioned, programs_fuzz_observation, programs_fuzz_targets,
    replay_recorded_execution, replay_recorded_execution_with_fee_history, DifferentialMismatch,
    FuzzTarget, RecordedExecution, ReplayRefusal,
};
pub use schedule::{
    ConflictGraph, ParallelScheduler, ScheduleAccess, ScheduleError, SchedulePlan,
    SchedulingStrategy, DEFAULT_MAXIMUM_SCHEDULER_WORKERS,
};
pub use storage::{
    NamespaceDrop, PrincipalId, ProgramId, ScanEntry, ScanLimits, Storage, StorageError,
    StorageNamespace, StorageScan,
};
pub use transfer::{
    AtomicTransferSet, KernelTransferEvidence, KernelTransferPrimitive, ProgramAuthority,
    ProgramFundingBinding, TransferCapability, TransferLawError, TransferSource,
    VerifiedProgramSettlement,
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
    Abi, AbiCommit, AbiEffects, AbiError, AuthorizationContext, BalanceView, CallFrameId,
    Capability, CapabilitySet, HostFunction, ProgramCall, ProgramEvent, ReceiptOracle,
    ReceiptView, StorageSelector, TransferRequest, UnavailableReceiptOracle, ABI_MODULE,
    HOST_FUNCTIONS,
};
pub use abi::manifest::{
    manifest as abi_manifest, ABI_V1_MANIFEST, ABI_V1_MODULE, ABI_V1_VERSION,
    ABI_V2_HOST_FUNCTIONS, ABI_V2_MANIFEST, ABI_V2_MODULE, ABI_V2_VERSION,
};
pub use crypto::bigint::{WideIntegerOp, WideIntegerRefusal, WideIntegerRefusalReason};
pub use crypto::{
    recover_secp256k1, verify_ed25519, verify_secp256k1, SignatureAlgorithm, SignatureRefusal,
    ED25519_PUBLIC_KEY_BYTES, ED25519_SIGNATURE_BYTES, MAX_MESSAGE_DIGEST_BYTES,
    SECP256K1_COMPRESSED_PUBLIC_KEY_BYTES, SECP256K1_SIGNATURE_BYTES,
    SECP256K1_UNCOMPRESSED_PUBLIC_KEY_BYTES,
};
