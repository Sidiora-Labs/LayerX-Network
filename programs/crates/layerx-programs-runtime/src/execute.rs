//! Typed execution surface over instantiated deterministic programs.

use core::fmt::{self, Display};
use std::collections::BTreeSet;

use wasmi::core::{Pages, TrapCode};
use wasmi::{
    ExecutionControlKind as WasmiControlKind, ExecutionSnapshot as WasmiExecutionSnapshot,
    ExecutionTraceValue as WasmiExecutionValue, ExecutionValueType as WasmiExecutionValueType,
    Extern, Instance, Memory, Store, Value,
};

use crate::abi::context::ExecutionContext;
use crate::abi::response::{CallResponse, ResponseRefusal};
use crate::abi::{Abi, AbiEffects, AbiError, AuthorizationContext, ReceiptOracle};
use crate::budget::{
    maximum_fee_units, validate_bounds, ActivityBudgetBinding, AdmittedBudget,
    BudgetAdmissionRefusal, DeclaredBudget, PayerCoverage,
};
use crate::calls::{CallGraph, Composition, CompositionContext, CompositionRefusal};
use crate::entrypoint::{self, EntrypointRefusal};
use crate::fault::{ProgramFailure, RefusalClass, RefusalReason, CANDIDATE_REFUSAL_SENTINEL};
use crate::host::RuntimeState;
use crate::meter::{
    BudgetMeterRefusal, BudgetResourceKind, FeeSchedule, Meter, MeterRefusal, MeteredUsage,
    ResourceBudget, ResourceKind,
};
use crate::storage::{PrincipalId, ProgramId, Storage};
use crate::transfer::{
    AtomicTransferSet, KernelTransferPrimitive, TransferCapability, TransferLawError,
    VerifiedProgramSettlement,
};
use crate::validate::{AbiRevision, ValidatedModule};

/// Runtime version recorded for versioned replay of every execution.
pub const RUNTIME_VERSION: u16 = 1;
pub use crate::ABI_VERSION;

/// An integer-only value crossing the program boundary.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WasmValue {
    /// A 32-bit integer value.
    I32(i32),
    /// A 64-bit integer value.
    I64(i64),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RuntimeGlobal {
    pub name: String,
    pub value: WasmValue,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RuntimeContinuation {
    pub linear_memory: Vec<u8>,
    pub globals: Vec<RuntimeGlobal>,
    pub entrypoint: String,
    pub arguments: Vec<WasmValue>,
}

impl From<WasmValue> for Value {
    fn from(value: WasmValue) -> Self {
        match value {
            WasmValue::I32(inner) => Self::I32(inner),
            WasmValue::I64(inner) => Self::I64(inner),
        }
    }
}

/// A typed fault produced while instantiating or executing a program.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ExecutionFault {
    /// The named export does not exist.
    UnknownExport {
        /// The export name that was not found.
        name: String,
    },
    /// The named export exists but is not a function.
    NotAFunction {
        /// The export name that is not a function.
        name: String,
    },
    /// Guest code executed the `unreachable` instruction.
    UnreachableExecuted,
    /// Guest code accessed linear memory out of bounds.
    MemoryOutOfBounds,
    /// Guest code accessed a table out of bounds.
    TableOutOfBounds,
    /// Guest code called an uninitialised table element indirectly.
    IndirectCallToNull,
    /// Guest code divided an integer by zero.
    IntegerDivisionByZero,
    /// Guest integer arithmetic overflowed.
    IntegerOverflow,
    /// Guest code attempted an invalid integer conversion.
    BadConversionToInteger,
    /// Execution exceeded the declared value stack height or call depth.
    StackExhausted,
    /// An indirect call used a mismatching signature.
    BadSignature,
    /// Execution exhausted its metered fuel budget.
    OutOfFuel,
    /// A growth operation was refused by a resource limit.
    GrowthLimited,
    /// A deterministic meter refused the execution before an effect escaped.
    Resource {
        /// Exact resource refusal.
        refusal: MeterRefusal,
    },
    /// A program value crossed the boundary outside the integer subset.
    NonIntegerValue,
    /// The engine reported a fault outside the typed trap set.
    EngineFault {
        /// The engine's description of the fault.
        reason: String,
    },
}

impl Display for ExecutionFault {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::UnknownExport { name } => write!(f, "unknown export {name}"),
            Self::NotAFunction { name } => write!(f, "export {name} is not a function"),
            Self::UnreachableExecuted => write!(f, "unreachable instruction executed"),
            Self::MemoryOutOfBounds => write!(f, "memory access out of bounds"),
            Self::TableOutOfBounds => write!(f, "table access out of bounds"),
            Self::IndirectCallToNull => write!(f, "indirect call to null table element"),
            Self::IntegerDivisionByZero => write!(f, "integer division by zero"),
            Self::IntegerOverflow => write!(f, "integer overflow"),
            Self::BadConversionToInteger => write!(f, "invalid conversion to integer"),
            Self::StackExhausted => write!(f, "declared stack or call depth limit exhausted"),
            Self::BadSignature => write!(f, "indirect call signature mismatch"),
            Self::OutOfFuel => write!(f, "metered fuel budget exhausted"),
            Self::GrowthLimited => write!(f, "growth operation refused by resource limit"),
            Self::Resource { refusal } => write!(f, "resource refusal: {refusal}"),
            Self::NonIntegerValue => write!(f, "non-integer value crossed the boundary"),
            Self::EngineFault { reason } => write!(f, "engine fault: {reason}"),
        }
    }
}

impl std::error::Error for ExecutionFault {}

pub(crate) fn fault_from_error(error: &wasmi::Error) -> ExecutionFault {
    if let wasmi::Error::Trap(trap) = error {
        if let Some(code) = trap.trap_code() {
            return fault_from_trap_code(code);
        }
    }
    ExecutionFault::EngineFault {
        reason: error.to_string(),
    }
}

const fn fault_from_trap_code(code: TrapCode) -> ExecutionFault {
    match code {
        TrapCode::UnreachableCodeReached => ExecutionFault::UnreachableExecuted,
        TrapCode::MemoryOutOfBounds => ExecutionFault::MemoryOutOfBounds,
        TrapCode::TableOutOfBounds => ExecutionFault::TableOutOfBounds,
        TrapCode::IndirectCallToNull => ExecutionFault::IndirectCallToNull,
        TrapCode::IntegerDivisionByZero => ExecutionFault::IntegerDivisionByZero,
        TrapCode::IntegerOverflow => ExecutionFault::IntegerOverflow,
        TrapCode::BadConversionToInteger => ExecutionFault::BadConversionToInteger,
        TrapCode::StackOverflow => ExecutionFault::StackExhausted,
        TrapCode::BadSignature => ExecutionFault::BadSignature,
        TrapCode::OutOfFuel => ExecutionFault::OutOfFuel,
        TrapCode::GrowthOperationLimited => ExecutionFault::GrowthLimited,
    }
}

/// An instantiated program isolated inside its own store.
#[derive(Debug)]
pub struct ProgramInstance {
    store: Store<RuntimeState>,
    instance: Instance,
    resumable_globals: Option<Vec<String>>,
    validated_code_hash: [u8; 32],
}

fn commitment_fault(error: crate::CommitmentError) -> ExecutionFault {
    ExecutionFault::EngineFault {
        reason: format!("deterministic execution commitment refused: {error}"),
    }
}

fn trace_identity(
    module: &ValidatedModule,
    entrypoint: &str,
    inputs: &[u8],
    runtime_version: u16,
    abi_version: u16,
    fee_schedule_version: u32,
    policy: crate::TracePolicy,
) -> Result<crate::ExecutionTraceIdentity, ExecutionFault> {
    let mut input_preimage = b"LXP/program-trace-input/v1\0".to_vec();
    let entrypoint_length = u32::try_from(entrypoint.len()).map_err(|_| ExecutionFault::EngineFault {
        reason: "trace entry point length is unrepresentable".to_string(),
    })?;
    input_preimage.extend_from_slice(&entrypoint_length.to_be_bytes());
    input_preimage.extend_from_slice(entrypoint.as_bytes());
    let input_length = u64::try_from(inputs.len()).map_err(|_| ExecutionFault::EngineFault {
        reason: "trace input length is unrepresentable".to_string(),
    })?;
    input_preimage.extend_from_slice(&input_length.to_be_bytes());
    input_preimage.extend_from_slice(inputs);
    let input_digest = crate::hash_bytes(crate::HashAlgorithm::Sha256, &input_preimage)
        .map_err(|error| ExecutionFault::EngineFault { reason: error.to_string() })?;
    let mut parameters = b"LXP/program-trace-parameters/v1\0".to_vec();
    parameters.extend_from_slice(&runtime_version.to_be_bytes());
    parameters.extend_from_slice(&abi_version.to_be_bytes());
    parameters.extend_from_slice(&fee_schedule_version.to_be_bytes());
    parameters.extend_from_slice(&module.metering_schedule_version().to_be_bytes());
    parameters.extend_from_slice(&policy.canonical_bytes());
    let execution_parameters_digest = crate::hash_bytes(crate::HashAlgorithm::Sha256, &parameters)
        .map_err(|error| ExecutionFault::EngineFault { reason: error.to_string() })?;
    Ok(crate::ExecutionTraceIdentity {
        module_code_hash: module.code_hash(),
        input_digest,
        execution_parameters_digest,
    })
}

fn canonical_trace_bytes(trace: &crate::ExecutionTrace) -> Vec<u8> {
    let commitments = trace.commitments();
    let mut bytes = Vec::with_capacity(32_usize.saturating_add(commitments.len().saturating_mul(52)));
    bytes.extend_from_slice(&crate::STEP_COMMITMENT_VERSION.to_be_bytes());
    bytes.extend_from_slice(&trace.policy().canonical_bytes());
    bytes.extend_from_slice(&(commitments.len() as u32).to_be_bytes());
    for commitment in commitments {
        bytes.extend_from_slice(&commitment.step_index.to_be_bytes());
        bytes.extend_from_slice(&commitment.digest);
        bytes.extend_from_slice(&commitment.encoded_state_bytes.to_be_bytes());
        bytes.extend_from_slice(&commitment.commitment_fuel.to_be_bytes());
    }
    bytes.extend_from_slice(&trace.total_commitment_fuel().to_be_bytes());
    bytes.extend_from_slice(&trace.total_state_bytes().to_be_bytes());
    bytes
}

fn canonical_wasm_arguments(args: &[WasmValue]) -> Vec<u8> {
    let mut encoded = Vec::with_capacity(8_usize.saturating_add(args.len().saturating_mul(9)));
    encoded.extend_from_slice(&(args.len() as u64).to_be_bytes());
    for argument in args {
        match argument {
            WasmValue::I32(value) => {
                encoded.push(0);
                encoded.extend_from_slice(&value.to_be_bytes());
            }
            WasmValue::I64(value) => {
                encoded.push(1);
                encoded.extend_from_slice(&value.to_be_bytes());
            }
        }
    }
    encoded
}

fn execution_value(value: WasmiExecutionValue) -> crate::ExecutionValue {
    match value.value_type {
        WasmiExecutionValueType::I32 => crate::ExecutionValue::I32(value.bits as u32 as i32),
        WasmiExecutionValueType::I64 => crate::ExecutionValue::I64(value.bits as i64),
    }
}

fn execution_state_from_snapshot(
    snapshot: &WasmiExecutionSnapshot,
    identity: crate::ExecutionTraceIdentity,
) -> Result<crate::ExecutionState, ExecutionFault> {
    let usage = &snapshot.supplement.authoritative_usage;
    let mut storage_overlay = Vec::with_capacity(snapshot.supplement.storage_overlay.len());
    let mut previous_key: Option<&[u8]> = None;
    for (key, value) in &snapshot.supplement.storage_overlay {
        if key.is_empty() || previous_key.is_some_and(|previous| previous >= key.as_slice()) {
            return Err(ExecutionFault::EngineFault {
                reason: "deterministic execution storage overlay is not canonically ordered".to_string(),
            });
        }
        previous_key = Some(key);
        storage_overlay.push(match value {
            Some(value) => crate::StorageOverlayEntry::Write {
                key: key.clone(),
                value: value.clone(),
            },
            None => crate::StorageOverlayEntry::Delete { key: key.clone() },
        });
    }
    Ok(crate::ExecutionState {
        module_code_hash: identity.module_code_hash,
        input_digest: identity.input_digest,
        execution_parameters_digest: identity.execution_parameters_digest,
        step_index: snapshot.step_index,
        program_counter: snapshot.program_counter,
        value_stack: snapshot.value_stack.iter().copied().map(execution_value).collect(),
        call_frames: snapshot.call_frames.iter().map(|frame| crate::ExecutionFrame {
            function_index: frame.function_index,
            return_program_counter: frame.return_program_counter,
            locals: frame.locals.iter().copied().map(execution_value).collect(),
        }).collect(),
        control_stack: snapshot.control_stack.iter().map(|frame| crate::ExecutionControlFrame {
            kind: match frame.kind {
                WasmiControlKind::Block => 0,
                WasmiControlKind::If => 1,
                WasmiControlKind::Else => 2,
                WasmiControlKind::Loop => 3,
            },
            operand_stack_height: frame.operand_stack_height,
            unreachable: frame.unreachable,
        }).collect(),
        linear_memory: snapshot.linear_memory.clone(),
        globals: snapshot.globals.iter().map(|global| crate::ExecutionGlobal {
            global_index: global.global_index,
            mutable: global.mutable,
            value: execution_value(global.value),
        }).collect(),
        storage_overlay,
        fuel_remaining: snapshot.supplement.authoritative_fuel,
        metered_usage: MeteredUsage {
            cpu_fuel: usage.cpu_fuel,
            memory_bytes: usage.memory_bytes,
            storage_read_bytes: usage.storage_read_bytes,
            storage_write_bytes: usage.storage_write_bytes,
            output_values: usage.output_values,
            output_bytes: usage.output_bytes,
            occupancy_byte_batches: usage.occupancy_byte_batches,
            occupancy_fee_units: usage.occupancy_fee_units,
            fee_units: usage.fee_units,
        },
    })
}

impl ProgramInstance {
    pub(crate) const fn new(store: Store<RuntimeState>, instance: Instance) -> Self {
        Self { store, instance, resumable_globals: None, validated_code_hash: [0; 32] }
    }

    pub(crate) fn declare_resumable_globals(&mut self, globals: Option<Vec<String>>) {
        self.resumable_globals = globals;
    }

    pub(crate) fn bind_validated_code_hash(&mut self, code_hash: [u8; 32]) {
        self.validated_code_hash = code_hash;
    }

    #[must_use] pub const fn validated_code_hash(&self) -> [u8; 32] { self.validated_code_hash }

    pub fn storage_snapshot(&self) -> Option<Storage> {
        self.store.data().authorization_abi().map(Abi::storage_snapshot)
    }

    pub fn commit_snapshot_storage(
        &mut self, storage: Storage, write_bytes: u64,
    ) -> Result<(), ExecutionFault> {
        if self.store.data().authorization_abi().is_none() {
            return Err(ExecutionFault::EngineFault {
            reason: "sandbox runtime has no lease storage transaction".to_string(),
            });
        }
        let mut meter = self.store.data().meter().clone();
        meter.charge_storage_write(write_bytes)
            .map_err(|refusal| ExecutionFault::Resource { refusal })?;
        let state = self.store.data_mut();
        state.abi_mut().ok_or_else(|| ExecutionFault::EngineFault {
            reason: "sandbox runtime lost its lease storage transaction".to_string(),
        })?.adopt_storage(storage);
        state.set_meter(meter);
        Ok(())
    }

    pub(crate) fn enable_execution_trace(
        &mut self,
        policy: crate::TracePolicy,
    ) -> Result<(), ExecutionFault> {
        let maximum_snapshots = usize::try_from(policy.maximum_commitments())
            .ok()
            .ok_or_else(|| ExecutionFault::EngineFault {
                reason: "execution trace snapshot bound overflowed".to_string(),
            })?;
        self.store.enable_execution_observer_with_limits(
            policy.interval(),
            maximum_snapshots,
            crate::MAX_TRACE_STATE_BYTES,
            crate::MAX_TRACE_STATE_BYTES,
        );
        self.store.set_execution_supplement(RuntimeState::execution_supplement);
        Ok(())
    }

    pub(crate) fn take_execution_trace(
        &mut self,
        policy: crate::TracePolicy,
        identity: crate::ExecutionTraceIdentity,
    ) -> Result<crate::ExecutionTrace, ExecutionFault> {
        if let Some(error) = self.store.execution_observer_error() {
            return Err(ExecutionFault::EngineFault {
                reason: format!("deterministic execution observer refused: {error:?}"),
            });
        }
        let transitions = self.store.take_execution_transitions();
        let mut retained_snapshot_bytes = 0_u64;
        let mut maximum_encoding_bytes = 0_u64;
        let mut previous_post: Option<&std::sync::Arc<wasmi::ExecutionSnapshot>> = None;
        for transition in &transitions {
            if previous_post.map_or(true, |post| !std::sync::Arc::ptr_eq(post, &transition.pre)) {
                let state_bytes = transition.pre.supplement.canonical_state_bytes;
                retained_snapshot_bytes = retained_snapshot_bytes.checked_add(state_bytes)
                    .and_then(|bytes| bytes.checked_add(u64::try_from(transition.pre.canonical_instruction.len()).ok()?))
                    .ok_or_else(|| ExecutionFault::EngineFault { reason: "execution trace peak-byte accounting overflowed".to_string() })?;
                maximum_encoding_bytes = maximum_encoding_bytes.max(state_bytes);
            }
            let state_bytes = transition.post.supplement.canonical_state_bytes;
            retained_snapshot_bytes = retained_snapshot_bytes.checked_add(state_bytes)
                .and_then(|bytes| bytes.checked_add(u64::try_from(transition.post.canonical_instruction.len()).ok()?))
                .ok_or_else(|| ExecutionFault::EngineFault { reason: "execution trace peak-byte accounting overflowed".to_string() })?;
            maximum_encoding_bytes = maximum_encoding_bytes.max(state_bytes);
            previous_post = Some(&transition.post);
        }
        let peak_bytes = retained_snapshot_bytes.checked_mul(2)
            .and_then(|bytes| bytes.checked_add(maximum_encoding_bytes))
            .ok_or_else(|| ExecutionFault::EngineFault { reason: "execution trace peak-byte accounting overflowed".to_string() })?;
        if peak_bytes > crate::MAX_TRACE_STATE_BYTES {
            return Err(ExecutionFault::EngineFault {
                reason: format!("execution trace peak retained bytes {peak_bytes} exceed {}", crate::MAX_TRACE_STATE_BYTES),
            });
        }
        let mut trace = crate::ExecutionTrace::new(policy);
        let mut last_recorded_step = None;
        let mut last_state: Option<std::sync::Arc<crate::ExecutionState>> = None;
        let mut last_snapshot: Option<std::sync::Arc<wasmi::ExecutionSnapshot>> = None;
        for transition in transitions {
            let pre_state = match (&last_snapshot, &last_state) {
                (Some(snapshot), Some(state)) if std::sync::Arc::ptr_eq(snapshot, &transition.pre) => std::sync::Arc::clone(state),
                _ => std::sync::Arc::new(execution_state_from_snapshot(&transition.pre, identity)?),
            };
            let post_state = std::sync::Arc::new(execution_state_from_snapshot(&transition.post, identity)?);
            let pre_commitment = crate::StepCommitment::from_state(pre_state.as_ref())
                .map_err(commitment_fault)?;
            let post_commitment = crate::StepCommitment::from_state(post_state.as_ref())
                .map_err(commitment_fault)?;
            for (snapshot, commitment) in [
                (transition.pre.as_ref(), pre_commitment),
                (transition.post.as_ref(), post_commitment),
            ] {
                if u64::from(commitment.encoded_state_bytes) != snapshot.supplement.canonical_state_bytes
                    || commitment.commitment_fuel != snapshot.supplement.commitment_fuel
                {
                    return Err(ExecutionFault::EngineFault {
                        reason: "preauthorized execution commitment accounting diverged from canonical state".to_string(),
                    });
                }
                if last_recorded_step != Some(commitment.step_index) {
                    trace.record_commitment(commitment).map_err(commitment_fault)?;
                    last_recorded_step = Some(commitment.step_index);
                }
            }
            last_state = Some(std::sync::Arc::clone(&post_state));
            last_snapshot = Some(std::sync::Arc::clone(&transition.post));
            trace.record_step(crate::ExecutionStep {
                instruction: transition.pre.canonical_instruction.clone(),
                instruction_fuel: transition.pre.instruction_fuel,
                memory_expansion_bytes: transition.memory_expansion_bytes,
                pre_state,
                post_state,
                pre_commitment,
                post_commitment,
            }).map_err(commitment_fault)?;
        }
        Ok(trace)
    }

    pub(crate) fn execution_observer_fault(&self) -> Option<ExecutionFault> {
        self.store.execution_observer_error().map(|error| {
            self.store.data().meter().exhaustion().map_or_else(
                || ExecutionFault::EngineFault {
                    reason: format!("deterministic execution observer refused: {error:?}"),
                },
                |refusal| ExecutionFault::Resource { refusal },
            )
        })
    }

    /// Reconciles trailing legacy Wasmi guest-instruction fuel into the meter.
    /// Host work is mirrored into both counters at its execution boundary and
    /// advances the committed engine baseline, so it is not charged twice.
    pub(crate) fn commit_reference_fuel(&mut self) -> Result<u64, ExecutionFault> {
        let consumed = self
            .store
            .fuel_consumed()
            .ok_or_else(|| ExecutionFault::EngineFault {
                reason: "legacy reference engine fuel is disabled".to_string(),
            })?;
        let committed = self.store.data().legacy_reference_engine_committed();
        let guest = consumed.checked_sub(committed).ok_or_else(|| {
            ExecutionFault::EngineFault {
                reason: "legacy reference host fuel exceeded engine fuel".to_string(),
            }
        })?;
        self.store
            .data_mut()
            .meter_mut()
            .charge_cpu(guest)
            .map_err(|refusal| ExecutionFault::Resource { refusal })?;
        self.store.data_mut().set_legacy_reference_engine_committed(consumed);
        Ok(consumed)
    }

    /// Calls an exported function with integer arguments.
    ///
    /// # Errors
    ///
    /// Returns a typed [`ExecutionFault`] when the export is missing or not a
    /// function, when execution traps, or when a non-integer value would cross
    /// the boundary.
    pub fn call(
        &mut self,
        export: &str,
        args: &[WasmValue],
    ) -> Result<Vec<WasmValue>, ExecutionFault> {
        let Some(external) = self.instance.get_export(&self.store, export) else {
            return Err(ExecutionFault::UnknownExport {
                name: export.to_string(),
            });
        };
        let Some(func) = external.into_func() else {
            return Err(ExecutionFault::NotAFunction {
                name: export.to_string(),
            });
        };
        let result_count = func.ty(&self.store).results().len();
        self.store
            .data_mut()
            .meter_mut()
            .charge_output(result_count)
            .map_err(|refusal| ExecutionFault::Resource { refusal })?;
        let inputs: Vec<Value> = args.iter().copied().map(Value::from).collect();
        let mut outputs = vec![Value::I64(0); result_count];
        let result = func.call(&mut self.store, &inputs, &mut outputs);
        if let Err(error) = result {
            let fault = fault_from_error(&error);
            if fault == ExecutionFault::OutOfFuel {
                self.store.data_mut().meter_mut().mark_cpu_exhausted();
            }
            return Err(fault);
        }
        outputs
            .into_iter()
            .map(|value| match value {
                Value::I32(inner) => Ok(WasmValue::I32(inner)),
                Value::I64(inner) => Ok(WasmValue::I64(inner)),
                Value::F32(_) | Value::F64(_) | Value::FuncRef(_) | Value::ExternRef(_) => {
                    Err(ExecutionFault::NonIntegerValue)
                }
            })
            .collect()
    }

    pub fn capture_continuation(
        &mut self, entrypoint: &str, arguments: &[WasmValue],
    ) -> Result<RuntimeContinuation, ExecutionFault> {
        if self.instance.get_export(&self.store, entrypoint)
            .and_then(Extern::into_func).is_none() {
            return Err(ExecutionFault::UnknownExport { name: entrypoint.to_string() });
        }
        let memory = self.linear_memory().ok_or_else(|| ExecutionFault::UnknownExport {
            name: "memory".to_string(),
        })?;
        let declared = self.resumable_globals.as_ref().ok_or_else(|| ExecutionFault::EngineFault {
            reason: "sandbox continuation requires every mutable global to be exported exactly once".to_string(),
        })?.clone();
        let capture_bytes = continuation_copy_bytes(memory.data(&self.store).len(), &declared,
            arguments).ok_or_else(|| ExecutionFault::EngineFault {
                reason: "sandbox continuation byte accounting overflowed".to_string(),
            })?;
        self.store.data_mut().meter_mut().charge_storage_read(capture_bytes)
            .map_err(|refusal| ExecutionFault::Resource { refusal })?;
        let linear_memory = memory.data(&self.store).to_vec();
        let mut globals = Vec::new();
        for name in &declared {
            let global = self.instance.get_export(&self.store, name)
                .and_then(Extern::into_global)
                .ok_or_else(|| ExecutionFault::UnknownExport { name: name.clone() })?;
            let value = match global.get(&self.store) {
                Value::I32(value) => WasmValue::I32(value),
                Value::I64(value) => WasmValue::I64(value),
                Value::F32(_) | Value::F64(_) | Value::FuncRef(_) | Value::ExternRef(_) => {
                    return Err(ExecutionFault::NonIntegerValue);
                }
            };
            globals.push(RuntimeGlobal { name: name.clone(), value });
        }
        globals.sort_by(|left, right| left.name.cmp(&right.name));
        Ok(RuntimeContinuation { linear_memory, globals, entrypoint: entrypoint.to_string(),
            arguments: arguments.to_vec() })
    }

    pub fn restore_continuation(
        &mut self, continuation: &RuntimeContinuation,
    ) -> Result<Vec<WasmValue>, ExecutionFault> {
        let declared = self.resumable_globals.as_ref().ok_or_else(|| ExecutionFault::EngineFault {
            reason: "sandbox continuation requires every mutable global to be exported exactly once".to_string(),
        })?;
        if continuation.globals.iter().map(|global| &global.name).ne(declared.iter()) {
            return Err(ExecutionFault::EngineFault {
                reason: "sandbox continuation mutable-global set is incomplete or non-canonical".to_string(),
            });
        }
        let memory = self.linear_memory().ok_or_else(|| ExecutionFault::UnknownExport {
            name: "memory".to_string(),
        })?;
        let names: Vec<String> = continuation.globals.iter().map(|global| global.name.clone()).collect();
        let restore_bytes = continuation_copy_bytes(continuation.linear_memory.len(), &names,
            &continuation.arguments).ok_or_else(|| ExecutionFault::EngineFault {
                reason: "sandbox continuation byte accounting overflowed".to_string(),
            })?;
        self.store.data_mut().meter_mut().charge_storage_write(restore_bytes)
            .map_err(|refusal| ExecutionFault::Resource { refusal })?;
        let current = memory.data(&self.store).len();
        if continuation.linear_memory.len() < current
            || continuation.linear_memory.len() % 65_536 != 0 {
            return Err(ExecutionFault::MemoryOutOfBounds);
        }
        let additional = (continuation.linear_memory.len() - current) / 65_536;
        if additional != 0 {
            let pages = Pages::new(u32::try_from(additional)
                .map_err(|_| ExecutionFault::MemoryOutOfBounds)?)
                .ok_or(ExecutionFault::MemoryOutOfBounds)?;
            memory.grow(&mut self.store, pages)
                .map_err(|_| ExecutionFault::MemoryOutOfBounds)?;
        }
        memory.write(&mut self.store, 0, &continuation.linear_memory)
            .map_err(|_| ExecutionFault::MemoryOutOfBounds)?;
        for restored in &continuation.globals {
            let global = self.instance.get_export(&self.store, &restored.name)
                .and_then(Extern::into_global)
                .ok_or_else(|| ExecutionFault::UnknownExport { name: restored.name.clone() })?;
            global.set(&mut self.store, Value::from(restored.value))
                .map_err(|error| ExecutionFault::EngineFault { reason: error.to_string() })?;
        }
        self.call(&continuation.entrypoint, &continuation.arguments)
    }

    /// Borrows the exact meter state for this isolated execution.
    #[must_use]
    pub fn meter(&self) -> &Meter {
        self.store.data().meter()
    }

    pub(crate) fn state(&self) -> &RuntimeState {
        self.store.data()
    }

    pub(crate) fn linear_memory(&self) -> Option<Memory> {
        self.instance
            .get_export(&self.store, "memory")
            .and_then(Extern::into_memory)
    }

    pub(crate) fn write_linear_memory(
        &mut self,
        memory: Memory,
        offset: usize,
        bytes: &[u8],
    ) -> Result<(), ExecutionFault> {
        memory
            .write(&mut self.store, offset, bytes)
            .map_err(|_| ExecutionFault::MemoryOutOfBounds)
    }

    pub(crate) fn consume_copy_fuel(&mut self, fuel: u64) -> Result<(), EntrypointRefusal> {
        if self.store.data().uses_legacy_reference_fuel() {
            let consumed = self.store.fuel_consumed().ok_or_else(|| EntrypointRefusal::Fault(
                ExecutionFault::EngineFault { reason: "legacy reference engine fuel is disabled".to_string() }
            ))?;
            let committed = self.store.data().legacy_reference_engine_committed();
            let guest = consumed.checked_sub(committed).ok_or_else(|| EntrypointRefusal::Fault(
                ExecutionFault::EngineFault { reason: "legacy reference host fuel exceeded engine fuel".to_string() }
            ))?;
            self.store.data_mut().meter_mut().charge_cpu(guest).map_err(EntrypointRefusal::Resource)?;
            self.store.data_mut().set_legacy_reference_engine_committed(consumed);
        }
        if self.store.data().uses_legacy_reference_fuel() && self.store.consume_fuel(fuel).is_err() {
            self.store.data_mut().meter_mut().mark_cpu_exhausted();
            return Err(EntrypointRefusal::Resource(
                self.store.data().meter().exhaustion().unwrap_or(MeterRefusal::BudgetExceeded {
                    resource: ResourceKind::Cpu,
                    limit: self.store.data().meter().cpu_budget(),
                    attempted: self.store.data().meter().cpu_budget().saturating_add(1),
                }),
            ));
        }
        self.store.data_mut().meter_mut().charge_cpu(fuel)
            .map_err(EntrypointRefusal::Resource)?;
        if self.store.data().uses_legacy_reference_fuel() {
            let consumed = self.store.fuel_consumed().unwrap_or_else(|| unreachable!());
            self.store.data_mut().set_legacy_reference_engine_committed(consumed);
        }
        Ok(())
    }

    pub(crate) fn into_state(self) -> RuntimeState {
        self.store.into_data()
    }
}

fn continuation_copy_bytes(
    memory_bytes: usize, globals: &[String], arguments: &[WasmValue],
) -> Option<u64> {
    let globals = globals.iter().try_fold(0usize, |total, name|
        total.checked_add(name.len())?.checked_add(9))?;
    let arguments = arguments.iter().try_fold(0usize, |total, value|
        total.checked_add(match value { WasmValue::I32(_) => 5, WasmValue::I64(_) => 9 }))?;
    u64::try_from(memory_bytes.checked_add(globals)?.checked_add(arguments)?).ok()
}

/// Receipt-carriable deterministic execution result.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExecutionRecord {
    /// Runtime version under which the program executed.
    pub runtime_version: u16,
    /// ABI version under which the program executed.
    pub abi_version: u16,
    /// Engine-neutral instruction schedule selected by the validated artifact.
    pub metering_schedule_version: u32,
    /// Integer-only guest outputs.
    pub outputs: Vec<WasmValue>,
    /// Exact deterministic resource use and fee units.
    pub usage: MeteredUsage,
    /// Receipt-bound deterministic trace when the executor declared a policy.
    pub trace: Option<crate::ExecutionTrace>,
}

/// Ordinary execution together with its receipt-bindable deterministic step evidence.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TracedExecutionRecord {
    pub execution: ExecutionRecord,
    pub trace: crate::ExecutionTrace,
}

impl TracedExecutionRecord {
    /// Canonical receipt evidence binding the ordinary result and declared trace policy.
    pub fn canonical_evidence(&self) -> Result<Vec<u8>, crate::CommitmentError> {
        let execution = self.execution.canonical_evidence();
        let trace = self.trace.canonical_bytes()?;
        let mut evidence = Vec::with_capacity(
            32_usize.saturating_add(execution.len()).saturating_add(trace.len()),
        );
        evidence.extend_from_slice(b"LXP/program-traced-execution/v1\0");
        let execution_length = u32::try_from(execution.len())
            .map_err(|_| crate::CommitmentError::LengthOutOfRange { bytes: execution.len() })?;
        evidence.extend_from_slice(&execution_length.to_be_bytes());
        evidence.extend_from_slice(&execution);
        let trace_length = u32::try_from(trace.len())
            .map_err(|_| crate::CommitmentError::LengthOutOfRange { bytes: trace.len() })?;
        evidence.extend_from_slice(&trace_length.to_be_bytes());
        evidence.extend_from_slice(&trace);
        Ok(evidence)
    }
}

/// Successful authorized execution plus effects awaiting the kernel's atomic
/// application boundary. The effects and the call graph belong to the whole
/// composition: every program the activity entered contributed to them, and a
/// refusal anywhere in the graph returns none of them.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AuthorizedExecutionRecord {
    pub execution: ExecutionRecord,
    pub effects: AbiEffects,
    pub call_graph: CallGraph,
}

/// A successful WASM execution held before its only monetary settlement path.
/// The contained storage is private until strict kernel settlement succeeds.
#[derive(Debug, PartialEq, Eq)]
pub struct PreparedAuthorizedActivity {
    record: AuthorizedExecutionRecord,
    prior_storage: Storage,
    held_storage: Storage,
    transfer: Option<TransferCapability>,
    transfer_set: Option<AtomicTransferSet>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ProtocolStateCas {
    key: Vec<u8>,
    expected: Option<Vec<u8>>,
    replacement: Vec<u8>,
}

impl ProtocolStateCas {
    #[must_use]
    pub fn new(key: Vec<u8>, expected: Option<Vec<u8>>, replacement: Vec<u8>) -> Self {
        Self { key, expected, replacement }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ProtocolStateCasRefusal { Empty, Duplicate, Limit, Stale, Storage(crate::storage::StorageError) }

impl PreparedAuthorizedActivity {
    /// Returns execution-only receipt diagnostics. Staged effects and held
    /// storage remain inaccessible until affine settlement succeeds.
    #[must_use]
    pub const fn execution(&self) -> &ExecutionRecord {
        &self.record.execution
    }

    #[must_use]
    pub(crate) const fn transfer_set(&self) -> Option<&AtomicTransferSet> {
        self.transfer_set.as_ref()
    }

    /// Reports sealed monetary work without exposing an executable transfer
    /// set outside this crate's affine settlement boundary.
    #[must_use]
    pub const fn has_monetary_effects(&self) -> bool {
        self.transfer_set.is_some()
    }

    /// Returns non-executable sealed-set diagnostics for activity evidence.
    /// The kernel input itself remains inaccessible until `strict_settle`.
    #[must_use]
    pub fn monetary_summary(&self) -> Option<PreparedMonetarySummary> {
        self.transfer_set
            .as_ref()
            .map(|set| PreparedMonetarySummary {
                program: set.program(),
                principal: set.principal(),
                invocation_authority: set.invocation_authority(),
                total_amount: set.total_amount(),
                legs: set
                    .legs()
                    .iter()
                    .map(|leg| PreparedTransferLegSummary {
                        program: leg.program,
                        principal: leg.principal,
                        frame: leg.frame,
                        source: leg.source.clone(),
                        asset: leg.asset,
                        to: leg.to,
                        amount: leg.amount,
                    })
                    .collect(),
            })
    }

    /// Returns the deterministic root of the exact sealed transfer set before
    /// kernel application. The root carries no authority to execute the set.
    #[must_use]
    pub fn expected_transfer_set_root(&self) -> Option<[u8; 32]> {
        self.transfer_set.as_ref().map(AtomicTransferSet::kernel_root)
    }

    /// Measures one namespace in the held post-execution snapshot without
    /// releasing or mutating that snapshot.
    pub fn held_namespace_persistent_bytes(
        &self,
        namespace: crate::StorageNamespace,
    ) -> Result<u64, crate::storage::StorageError> {
        self.held_storage.namespace_persistent_bytes(namespace)
    }

    /// Stages protocol-owned canonical state in the same held storage snapshot
    /// as guest effects, after comparing every expected value to prior state.
    pub fn stage_protocol_state_cas(
        &mut self,
        namespace: crate::StorageNamespace,
        changes: Vec<ProtocolStateCas>,
    ) -> Result<(), ProtocolStateCasRefusal> {
        if changes.is_empty() { return Err(ProtocolStateCasRefusal::Empty); }
        if changes.len() > 16 { return Err(ProtocolStateCasRefusal::Limit); }
        let prior = self.prior_storage.namespace_entries(namespace);
        let mut keys = BTreeSet::new();
        for change in &changes {
            if change.key.is_empty() || !keys.insert(change.key.clone()) {
                return Err(ProtocolStateCasRefusal::Duplicate);
            }
            let actual = prior.iter().find_map(|(key, value)|
                (key == &change.key).then_some(value));
            if actual.map(Vec::as_slice) != change.expected.as_deref() {
                return Err(ProtocolStateCasRefusal::Stale);
            }
        }
        let mut transaction = self.held_storage.transaction(namespace);
        for change in changes {
            transaction.write(&change.key, &change.replacement)
                .map_err(ProtocolStateCasRefusal::Storage)?;
        }
        let _ = transaction.commit();
        Ok(())
    }

    /// Consumes the held activity, performs its single kernel settlement, and
    /// assigns its storage exactly once on success. A refusal carries only
    /// execution diagnostics and graph evidence, never staged effects.
    pub fn strict_settle(
        self,
        storage: &mut Storage,
        kernel: &mut impl KernelTransferPrimitive,
    ) -> Result<VerifiedStorageAssignment, SettlementFailure> {
        if *storage != self.prior_storage {
            return Err(SettlementFailure::new(
                self.record.execution,
                self.record.call_graph,
                TransferLawError::StaleStorage,
            ));
        }
        let settlement = match (self.transfer, self.transfer_set) {
            (Some(transfer), Some(transfer_set)) => Some(
                transfer
                    .settle_authorized_set(&transfer_set, kernel)
                    .map_err(|error| {
                        SettlementFailure::new(
                            self.record.execution.clone(),
                            self.record.call_graph.clone(),
                            error,
                        )
                    })?,
            ),
            (None, None) => None,
            _ => {
                return Err(SettlementFailure::new(
                    self.record.execution,
                    self.record.call_graph,
                    TransferLawError::InvariantViolation,
                ))
            }
        };
        *storage = self.held_storage;
        Ok(VerifiedStorageAssignment {
            record: self.record,
            settlement,
        })
    }
}

/// Non-executable evidence describing the monetary work sealed in an affine
/// prepared activity.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PreparedMonetarySummary {
    program: ProgramId,
    principal: PrincipalId,
    invocation_authority: [u8; 32],
    total_amount: u128,
    legs: Vec<PreparedTransferLegSummary>,
}

impl PreparedMonetarySummary {
    #[must_use]
    pub const fn program(&self) -> ProgramId {
        self.program
    }
    #[must_use]
    pub const fn principal(&self) -> PrincipalId {
        self.principal
    }
    #[must_use]
    pub const fn invocation_authority(&self) -> [u8; 32] {
        self.invocation_authority
    }
    #[must_use]
    pub const fn total_amount(&self) -> u128 {
        self.total_amount
    }
    #[must_use]
    pub fn legs(&self) -> &[PreparedTransferLegSummary] {
        &self.legs
    }
}

/// One non-executable transfer-leg fact retained for receipt diagnostics.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PreparedTransferLegSummary {
    program: ProgramId,
    principal: PrincipalId,
    frame: crate::abi::CallFrameId,
    source: crate::TransferSource,
    asset: [u8; 32],
    to: [u8; 32],
    amount: u128,
}

impl PreparedTransferLegSummary {
    #[must_use]
    pub const fn program(&self) -> ProgramId {
        self.program
    }
    #[must_use]
    pub const fn principal(&self) -> PrincipalId {
        self.principal
    }
    #[must_use]
    pub const fn frame(&self) -> crate::abi::CallFrameId {
        self.frame
    }
    #[must_use]
    pub const fn source(&self) -> &crate::TransferSource {
        &self.source
    }
    #[must_use]
    pub const fn asset(&self) -> [u8; 32] {
        self.asset
    }
    #[must_use]
    pub const fn to(&self) -> [u8; 32] {
        self.to
    }
    #[must_use]
    pub const fn amount(&self) -> u128 {
        self.amount
    }
}

/// Receipt-ready failure diagnostics for an affine settlement attempt.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SettlementFailure {
    execution: ExecutionRecord,
    call_graph: CallGraph,
    error: TransferLawError,
}

impl SettlementFailure {
    const fn new(
        execution: ExecutionRecord,
        call_graph: CallGraph,
        error: TransferLawError,
    ) -> Self {
        Self {
            execution,
            call_graph,
            error,
        }
    }
    #[must_use]
    pub const fn execution(&self) -> &ExecutionRecord {
        &self.execution
    }
    #[must_use]
    pub const fn call_graph(&self) -> &CallGraph {
        &self.call_graph
    }
    #[must_use]
    pub const fn error(&self) -> TransferLawError {
        self.error
    }
}

/// A prepared activity whose exact set and receipt commitment were verified by
/// the real kernel. Only this token can publish the held storage snapshot.
#[derive(Debug, PartialEq, Eq)]
pub struct VerifiedStorageAssignment {
    record: AuthorizedExecutionRecord,
    settlement: Option<VerifiedProgramSettlement>,
}

impl VerifiedStorageAssignment {
    #[must_use]
    pub const fn settlement(&self) -> Option<&VerifiedProgramSettlement> {
        self.settlement.as_ref()
    }

    #[must_use]
    pub const fn record(&self) -> &AuthorizedExecutionRecord {
        &self.record
    }
}

/// Additive receipt-ready result of an ABI-v1 activity using an admitted budget.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum BudgetedV1ActivityOutcome {
    /// Frozen v1 success record, byte-for-byte unchanged.
    Success(AuthorizedExecutionRecord),
    /// Guest refusal or deterministic runtime fault with no committed effects.
    Failure(BudgetedV1FailureRecord),
    /// Typed resource exhaustion with no committed effects.
    Resource(BudgetedResourceFailureRecord),
}

/// Receipt-ready outcome of preparing an activity under an admitted budget.
/// Only the success variant holds an affine activity that can be settled.
#[derive(Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum PreparedAuthorizedActivityOutcome {
    Success(PreparedAuthorizedActivity),
    Failure(BudgetedV1FailureRecord),
    Resource(BudgetedResourceFailureRecord),
}

/// Receipt-ready ABI-v1 program failure produced only by the budgeted bridge.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BudgetedV1FailureRecord {
    root_program: ProgramId,
    activity_binding: ActivityBudgetBinding,
    cause: BudgetedV1FailureCause,
    usage: MeteredUsage,
    call_graph: CallGraph,
}

/// Closed typed cause retained by the additive budgeted ABI-v1 bridge.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum BudgetedV1FailureCause {
    /// Negative guest result or deterministic guest runtime fault.
    Program(ProgramFailure),
    /// Typed call-graph or capability refusal observed after metering began.
    Composition(CompositionRefusal),
    /// Typed entry protocol refusal observed after metering began.
    Entrypoint(EntrypointRefusal),
    /// Typed ABI refusal observed after metering began.
    Abi(AbiError),
}

impl BudgetedV1FailureRecord {
    #[must_use]
    pub const fn root_program(&self) -> ProgramId {
        self.root_program
    }
    #[must_use] pub const fn activity_binding(&self) -> ActivityBudgetBinding { self.activity_binding }

    #[must_use]
    pub const fn cause(&self) -> &BudgetedV1FailureCause {
        &self.cause
    }

    #[must_use]
    pub const fn program_failure(&self) -> Option<&ProgramFailure> {
        match &self.cause {
            BudgetedV1FailureCause::Program(failure) => Some(failure),
            _ => None,
        }
    }

    #[must_use]
    pub const fn usage(&self) -> MeteredUsage {
        self.usage
    }

    #[must_use]
    pub const fn call_graph(&self) -> &CallGraph {
        &self.call_graph
    }
}

/// Receipt-ready resource failure shared by new budgeted activity routes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BudgetedResourceFailureRecord {
    root_program: ProgramId,
    activity_binding: ActivityBudgetBinding,
    refusal: BudgetMeterRefusal,
    usage: MeteredUsage,
    call_graph: CallGraph,
}

impl BudgetedResourceFailureRecord {
    #[must_use]
    pub const fn root_program(&self) -> ProgramId {
        self.root_program
    }
    #[must_use] pub const fn activity_binding(&self) -> ActivityBudgetBinding { self.activity_binding }

    #[must_use]
    pub const fn refusal(&self) -> BudgetMeterRefusal {
        self.refusal
    }

    #[must_use]
    pub const fn usage(&self) -> MeteredUsage {
        self.usage
    }

    #[must_use]
    pub const fn call_graph(&self) -> &CallGraph {
        &self.call_graph
    }
}

/// Qualification-only result produced under the explicitly selected candidate ABI.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CandidateAuthorizedExecutionRecord {
    root_program: ProgramId,
    abi_revision: AbiRevision,
    execution: CandidateExecutionRecord,
    outcome: CandidateActivityOutcome,
    call_graph: CallGraph,
}

/// Mutually exclusive candidate activity result carried into receipt projection.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CandidateActivityOutcome {
    Success {
        response: CallResponse,
        effects: AbiEffects,
    },
    Failure(ProgramFailure),
    /// Typed resource exhaustion with no committed program effects.
    Resource(BudgetMeterRefusal),
}

/// Public, canonical candidate activity receipt projection.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CandidateActivityReceipt {
    root_program: ProgramId,
    abi_revision: u16,
    runtime_version: u16,
    fee_schedule_version: u32,
    metering_schedule_version: u32,
    usage: MeteredUsage,
    graph_evidence: Vec<u8>,
    trace_evidence: Option<Vec<u8>>,
    outcome: CandidateReceiptOutcome,
}

/// Receipt outcome with no representable success/failure overlap.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CandidateReceiptOutcome {
    Success(CallResponse),
    Failure(ProgramFailure),
    /// Typed resource exhaustion with actual failed usage in the receipt header.
    Resource(BudgetMeterRefusal),
}

/// Execution facts that cannot be confused with frozen v1 receipt evidence.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CandidateExecutionRecord {
    runtime_version: u16,
    fee_schedule_version: u32,
    metering_schedule_version: u32,
    outputs: Vec<WasmValue>,
    usage: MeteredUsage,
    trace: Option<crate::ExecutionTrace>,
}

/// Frozen ABI-v2 execution and receipt vocabulary. The candidate spellings
/// remain aliases until task 31.7 migrates SDK and porting-kit ownership.
pub type V2AuthorizedExecutionRecord = CandidateAuthorizedExecutionRecord;
pub type V2ActivityOutcome = CandidateActivityOutcome;
pub type V2ActivityReceipt = CandidateActivityReceipt;
pub type V2ExecutionRecord = CandidateExecutionRecord;
pub type V2ReceiptOutcome = CandidateReceiptOutcome;

impl CandidateAuthorizedExecutionRecord {
    #[must_use]
    pub const fn root_program(&self) -> ProgramId {
        self.root_program
    }

    #[must_use]
    pub const fn abi_revision(&self) -> AbiRevision {
        self.abi_revision
    }

    #[must_use]
    pub const fn execution(&self) -> &CandidateExecutionRecord {
        &self.execution
    }

    #[must_use]
    pub const fn outcome(&self) -> &CandidateActivityOutcome {
        &self.outcome
    }

    #[must_use]
    pub const fn call_graph(&self) -> &CallGraph {
        &self.call_graph
    }

    #[must_use]
    pub fn receipt_projection(&self) -> CandidateActivityReceipt {
        CandidateActivityReceipt {
            root_program: self.root_program,
            abi_revision: match self.abi_revision {
                AbiRevision::V1 => crate::ABI_V1_VERSION,
                AbiRevision::V2 => crate::ABI_V2_VERSION,
            },
            runtime_version: self.execution.runtime_version,
            fee_schedule_version: self.execution.fee_schedule_version,
            metering_schedule_version: self.execution.metering_schedule_version,
            usage: self.execution.usage,
            graph_evidence: self.call_graph.canonical_evidence(),
            trace_evidence: self.execution.trace.as_ref().map(canonical_trace_bytes),
            outcome: match &self.outcome {
                CandidateActivityOutcome::Success { response, .. } => {
                    CandidateReceiptOutcome::Success(response.clone())
                }
                CandidateActivityOutcome::Failure(failure) => {
                    CandidateReceiptOutcome::Failure(failure.clone())
                }
                CandidateActivityOutcome::Resource(refusal) => {
                    CandidateReceiptOutcome::Resource(*refusal)
                }
            },
        }
    }
    #[must_use]
    pub const fn response(&self) -> Option<&CallResponse> {
        match &self.outcome {
            CandidateActivityOutcome::Success { response, .. } => Some(response),
            CandidateActivityOutcome::Failure(_) | CandidateActivityOutcome::Resource(_) => None,
        }
    }

    #[must_use]
    pub const fn failure(&self) -> Option<&ProgramFailure> {
        match &self.outcome {
            CandidateActivityOutcome::Failure(failure) => Some(failure),
            CandidateActivityOutcome::Success { .. } | CandidateActivityOutcome::Resource(_) => {
                None
            }
        }
    }

    #[must_use]
    pub const fn resource_refusal(&self) -> Option<&BudgetMeterRefusal> {
        match &self.outcome {
            CandidateActivityOutcome::Resource(refusal) => Some(refusal),
            CandidateActivityOutcome::Success { .. } | CandidateActivityOutcome::Failure(_) => None,
        }
    }

    #[must_use]
    pub const fn effects(&self) -> Option<&AbiEffects> {
        match &self.outcome {
            CandidateActivityOutcome::Success { effects, .. } => Some(effects),
            CandidateActivityOutcome::Failure(_) | CandidateActivityOutcome::Resource(_) => None,
        }
    }

    #[must_use]
    pub fn canonical_evidence(&self) -> Vec<u8> {
        let mut evidence = b"LXP/program-execution/v4\0".to_vec();
        evidence.extend_from_slice(&self.execution.runtime_version.to_be_bytes());
        evidence.extend_from_slice(&self.execution.fee_schedule_version.to_be_bytes());
        evidence.extend_from_slice(&self.execution.metering_schedule_version.to_be_bytes());
        evidence.extend_from_slice(&(self.execution.outputs.len() as u64).to_be_bytes());
        for output in &self.execution.outputs {
            match output {
                WasmValue::I32(value) => {
                    evidence.push(1);
                    evidence.extend_from_slice(&value.to_be_bytes());
                }
                WasmValue::I64(value) => {
                    evidence.push(2);
                    evidence.extend_from_slice(&value.to_be_bytes());
                }
            }
        }
        evidence.extend_from_slice(&self.execution.usage.cpu_fuel.to_be_bytes());
        evidence.extend_from_slice(&self.execution.usage.memory_bytes.to_be_bytes());
        evidence.extend_from_slice(&self.execution.usage.storage_read_bytes.to_be_bytes());
        evidence.extend_from_slice(&self.execution.usage.storage_write_bytes.to_be_bytes());
        evidence.extend_from_slice(&self.execution.usage.output_values.to_be_bytes());
        evidence.extend_from_slice(&self.execution.usage.output_bytes.to_be_bytes());
        evidence.extend_from_slice(&self.execution.usage.fee_units.to_be_bytes());
        match &self.execution.trace {
            Some(trace) => {
                evidence.push(1);
                let trace = canonical_trace_bytes(trace);
                evidence.extend_from_slice(&(trace.len() as u64).to_be_bytes());
                evidence.extend_from_slice(&trace);
            }
            None => evidence.push(0),
        }
        evidence.extend_from_slice(&self.root_program.bytes());
        let abi_revision = match self.abi_revision {
            AbiRevision::V1 => crate::abi::manifest::ABI_V1_VERSION,
            AbiRevision::V2 => 2,
        };
        evidence.extend_from_slice(&abi_revision.to_be_bytes());
        match &self.outcome {
            CandidateActivityOutcome::Failure(failure) => {
                evidence.push(1);
                let failure = failure.canonical_encode();
                evidence.extend_from_slice(&(failure.len() as u64).to_be_bytes());
                evidence.extend_from_slice(&failure);
            }
            CandidateActivityOutcome::Success { response, .. } => {
                evidence.push(0);
                evidence.extend_from_slice(&response.code.to_be_bytes());
                evidence.extend_from_slice(&(response.bytes.len() as u64).to_be_bytes());
                evidence.extend_from_slice(&response.bytes);
            }
            CandidateActivityOutcome::Resource(refusal) => {
                evidence.push(2);
                encode_meter_refusal(&mut evidence, refusal);
            }
        }
        let graph = self.call_graph.canonical_evidence();
        evidence.extend_from_slice(&(graph.len() as u64).to_be_bytes());
        evidence.extend_from_slice(&graph);
        evidence
    }

    pub(crate) fn write_canonical_evidence(
        &self, evidence: &mut Vec<u8>, graph: &mut Vec<u8>,
    ) {
        evidence.clear();
        evidence.extend_from_slice(b"LXP/program-execution/v4\0");
        evidence.extend_from_slice(&self.execution.runtime_version.to_be_bytes());
        evidence.extend_from_slice(&self.execution.fee_schedule_version.to_be_bytes());
        evidence.extend_from_slice(&self.execution.metering_schedule_version.to_be_bytes());
        evidence.extend_from_slice(&(self.execution.outputs.len() as u64).to_be_bytes());
        for output in &self.execution.outputs {
            match output {
                WasmValue::I32(value) => { evidence.push(1); evidence.extend_from_slice(&value.to_be_bytes()); }
                WasmValue::I64(value) => { evidence.push(2); evidence.extend_from_slice(&value.to_be_bytes()); }
            }
        }
        evidence.extend_from_slice(&self.execution.usage.cpu_fuel.to_be_bytes());
        evidence.extend_from_slice(&self.execution.usage.memory_bytes.to_be_bytes());
        evidence.extend_from_slice(&self.execution.usage.storage_read_bytes.to_be_bytes());
        evidence.extend_from_slice(&self.execution.usage.storage_write_bytes.to_be_bytes());
        evidence.extend_from_slice(&self.execution.usage.output_values.to_be_bytes());
        evidence.extend_from_slice(&self.execution.usage.output_bytes.to_be_bytes());
        evidence.extend_from_slice(&self.execution.usage.fee_units.to_be_bytes());
        match &self.execution.trace {
            Some(trace) => {
                evidence.push(1);
                let length_offset = evidence.len(); evidence.extend_from_slice(&[0; 8]);
                let start = evidence.len();
                evidence.extend_from_slice(&crate::STEP_COMMITMENT_VERSION.to_be_bytes());
                evidence.extend_from_slice(&trace.policy().canonical_bytes());
                evidence.extend_from_slice(&(trace.commitments().len() as u32).to_be_bytes());
                for commitment in trace.commitments() {
                    evidence.extend_from_slice(&commitment.step_index.to_be_bytes());
                    evidence.extend_from_slice(&commitment.digest);
                    evidence.extend_from_slice(&commitment.encoded_state_bytes.to_be_bytes());
                    evidence.extend_from_slice(&commitment.commitment_fuel.to_be_bytes());
                }
                evidence.extend_from_slice(&trace.total_commitment_fuel().to_be_bytes());
                evidence.extend_from_slice(&trace.total_state_bytes().to_be_bytes());
                let length = (evidence.len() - start) as u64;
                evidence[length_offset..length_offset + 8].copy_from_slice(&length.to_be_bytes());
            }
            None => evidence.push(0),
        }
        evidence.extend_from_slice(&self.root_program.bytes());
        evidence.extend_from_slice(&match self.abi_revision {
            AbiRevision::V1 => crate::abi::manifest::ABI_V1_VERSION, AbiRevision::V2 => 2,
        }.to_be_bytes());
        match &self.outcome {
            CandidateActivityOutcome::Failure(failure) => {
                evidence.push(1); let length_offset = evidence.len(); evidence.extend_from_slice(&[0; 8]);
                let start = evidence.len(); failure.append_canonical(evidence);
                let length = (evidence.len() - start) as u64;
                evidence[length_offset..length_offset + 8].copy_from_slice(&length.to_be_bytes());
            }
            CandidateActivityOutcome::Success { response, .. } => {
                evidence.push(0); evidence.extend_from_slice(&response.code.to_be_bytes());
                evidence.extend_from_slice(&(response.bytes.len() as u64).to_be_bytes());
                evidence.extend_from_slice(&response.bytes);
            }
            CandidateActivityOutcome::Resource(refusal) => {
                evidence.push(2); encode_meter_refusal(evidence, refusal);
            }
        }
        self.call_graph.write_canonical_evidence(graph);
        evidence.extend_from_slice(&(graph.len() as u64).to_be_bytes());
        evidence.extend_from_slice(graph);
    }
}

impl CandidateExecutionRecord {
    #[must_use]
    pub const fn runtime_version(&self) -> u16 {
        self.runtime_version
    }

    #[must_use]
    pub const fn fee_schedule_version(&self) -> u32 {
        self.fee_schedule_version
    }

    #[must_use]
    pub const fn metering_schedule_version(&self) -> u32 {
        self.metering_schedule_version
    }

    #[must_use]
    pub fn outputs(&self) -> &[WasmValue] {
        &self.outputs
    }

    #[must_use]
    pub const fn usage(&self) -> MeteredUsage {
        self.usage
    }

    #[must_use]
    pub const fn trace(&self) -> Option<&crate::ExecutionTrace> {
        self.trace.as_ref()
    }
}

impl CandidateActivityReceipt {
    const DOMAIN: &'static [u8] = b"LXP/program-activity-receipt/v4\0";
    const LEGACY_V3_DOMAIN: &'static [u8] = b"LXP/program-activity-receipt/v3\0";
    const LEGACY_V2_DOMAIN: &'static [u8] = b"LXP/program-activity-receipt/v2\0";
    const MAX_GRAPH_EVIDENCE_BYTES: usize = b"LayerX/programs/call-graph/v1\0".len()
        + 32
        + 16
        + 8
        + (crate::calls::DEFAULT_MAX_CALL_GRAPH_EDGES as usize * 68);
    const MAX_TRACE_EVIDENCE_BYTES: usize = 34
        + (crate::MAX_TRACE_COMMITMENTS * 52);

    #[must_use]
    pub const fn root_program(&self) -> ProgramId {
        self.root_program
    }

    #[must_use]
    pub const fn abi_revision(&self) -> u16 {
        self.abi_revision
    }

    #[must_use]
    pub const fn runtime_version(&self) -> u16 {
        self.runtime_version
    }

    #[must_use]
    pub const fn fee_schedule_version(&self) -> u32 {
        self.fee_schedule_version
    }


    #[must_use]
    pub const fn metering_schedule_version(&self) -> u32 {
        self.metering_schedule_version
    }

    #[must_use]
    pub const fn usage(&self) -> MeteredUsage {
        self.usage
    }

    #[must_use]
    pub fn graph_evidence(&self) -> &[u8] {
        &self.graph_evidence
    }


    #[must_use]
    pub fn trace_evidence(&self) -> Option<&[u8]> {
        self.trace_evidence.as_deref()
    }

    #[must_use]
    pub const fn outcome(&self) -> &CandidateReceiptOutcome {
        &self.outcome
    }

    #[must_use]
    pub fn canonical_encode(&self) -> Vec<u8> {
        let mut encoded = Self::DOMAIN.to_vec();
        encoded.extend_from_slice(&self.root_program.bytes());
        encoded.extend_from_slice(&self.abi_revision.to_be_bytes());
        encoded.extend_from_slice(&self.runtime_version.to_be_bytes());
        encoded.extend_from_slice(&self.fee_schedule_version.to_be_bytes());
        encoded.extend_from_slice(&self.metering_schedule_version.to_be_bytes());
        for value in [
            self.usage.cpu_fuel,
            self.usage.memory_bytes,
            self.usage.storage_read_bytes,
            self.usage.storage_write_bytes,
            self.usage.output_bytes,
        ] {
            encoded.extend_from_slice(&value.to_be_bytes());
        }
        encoded.extend_from_slice(&self.usage.output_values.to_be_bytes());
        encoded.extend_from_slice(&self.usage.fee_units.to_be_bytes());
        encoded.extend_from_slice(
            &u32::try_from(self.graph_evidence.len())
                .unwrap_or(u32::MAX)
                .to_be_bytes(),
        );
        encoded.extend_from_slice(&self.graph_evidence);
        match &self.trace_evidence {
            Some(trace) => {
                encoded.push(1);
                encoded.extend_from_slice(&(trace.len() as u32).to_be_bytes());
                encoded.extend_from_slice(trace);
            }
            None => encoded.push(0),
        }
        match &self.outcome {
            CandidateReceiptOutcome::Success(response) => {
                encoded.push(0);
                encoded.extend_from_slice(&response.code.to_be_bytes());
                encoded.extend_from_slice(
                    &u32::try_from(response.bytes.len())
                        .unwrap_or(u32::MAX)
                        .to_be_bytes(),
                );
                encoded.extend_from_slice(&response.bytes);
            }
            CandidateReceiptOutcome::Failure(failure) => {
                encoded.push(1);
                let failure = failure.canonical_encode();
                encoded.extend_from_slice(
                    &u32::try_from(failure.len())
                        .unwrap_or(u32::MAX)
                        .to_be_bytes(),
                );
                encoded.extend_from_slice(&failure);
            }
            CandidateReceiptOutcome::Resource(refusal) => {
                encoded.push(2);
                encode_meter_refusal(&mut encoded, refusal);
            }
        }
        encoded
    }

    /// Strictly decodes a candidate receipt projection.
    ///
    /// # Errors
    ///
    /// Refuses the wrong domain/revision, invalid fields, truncation, and trailing bytes.
    pub fn canonical_decode(encoded: &[u8]) -> Result<Self, crate::fault::FailureEncodingError> {
        use crate::fault::FailureEncodingError as Error;
        let mut cursor = ReceiptCursor::new(encoded);
        let domain = cursor.take(Self::DOMAIN.len())?;
        let legacy_v2 = domain == Self::LEGACY_V2_DOMAIN;
        let legacy_v3 = domain == Self::LEGACY_V3_DOMAIN;
        if domain != Self::DOMAIN && !legacy_v2 && !legacy_v3 {
            return Err(Error::Malformed);
        }
        let root_program =
            ProgramId::new(cursor.array::<32>()?).map_err(|_| Error::InvalidProgram)?;
        let abi_revision = u16::from_be_bytes(cursor.array()?);
        if abi_revision != 2 {
            return Err(Error::Malformed);
        }
        let runtime_version = u16::from_be_bytes(cursor.array()?);
        let fee_schedule_version = u32::from_be_bytes(cursor.array()?);
        let metering_schedule_version = if legacy_v2 {
            crate::meter::inject::GENESIS_METERING_SCHEDULE_VERSION
        } else {
            u32::from_be_bytes(cursor.array()?)
        };
        if runtime_version == 0 || fee_schedule_version == 0 || metering_schedule_version == 0 {
            return Err(Error::Malformed);
        }
        let usage = MeteredUsage {
            cpu_fuel: u64::from_be_bytes(cursor.array()?),
            memory_bytes: u64::from_be_bytes(cursor.array()?),
            storage_read_bytes: u64::from_be_bytes(cursor.array()?),
            storage_write_bytes: u64::from_be_bytes(cursor.array()?),
            output_bytes: u64::from_be_bytes(cursor.array()?),
            output_values: u32::from_be_bytes(cursor.array()?),
            occupancy_byte_batches: 0,
            occupancy_fee_units: 0,
            fee_units: u128::from_be_bytes(cursor.array()?),
        };
        let graph_length = u32::from_be_bytes(cursor.array()?) as usize;
        if graph_length > Self::MAX_GRAPH_EVIDENCE_BYTES {
            return Err(Error::Malformed);
        }
        let graph_evidence = cursor.take(graph_length)?.to_vec();
        let trace_evidence = if legacy_v2 || legacy_v3 {
            None
        } else {
            match cursor.take(1)?[0] {
                0 => None,
                1 => {
                    let length = u32::from_be_bytes(cursor.array()?) as usize;
                    Some(cursor.take(length)?.to_vec())
                }
                _ => return Err(Error::Malformed),
            }
        };
        let tag = cursor.take(1)?[0];
        let outcome = match tag {
            0 => {
                let code = i32::from_be_bytes(cursor.array()?);
                if code < 0 {
                    return Err(Error::Malformed);
                }
                let length = u32::from_be_bytes(cursor.array()?) as usize;
                if length > Self::MAX_TRACE_EVIDENCE_BYTES {
                    return Err(Error::Malformed);
                }
                if length > crate::MAX_CALL_RESPONSE_BYTES {
                    return Err(Error::Malformed);
                }
                CandidateReceiptOutcome::Success(CallResponse {
                    code,
                    bytes: cursor.take(length)?.to_vec(),
                })
            }
            1 => {
                let length = u32::from_be_bytes(cursor.array()?) as usize;
                CandidateReceiptOutcome::Failure(ProgramFailure::canonical_decode(
                    cursor.take(length)?,
                )?)
            }
            2 => CandidateReceiptOutcome::Resource(decode_meter_refusal(&mut cursor, usage)?),
            _ => return Err(Error::Malformed),
        };
        if !cursor.is_empty() {
            return Err(Error::Malformed);
        }
        Ok(Self {
            root_program,
            abi_revision,
            runtime_version,
            fee_schedule_version,
            metering_schedule_version,
            usage,
            graph_evidence,
            trace_evidence,
            outcome,
        })
    }
}

fn encode_meter_refusal(encoded: &mut Vec<u8>, refusal: &BudgetMeterRefusal) {
    match refusal {
        BudgetMeterRefusal::BudgetExceeded {
            resource,
            limit,
            attempted,
        } => {
            encoded.push(0);
            encoded.push(resource_tag(*resource));
            encoded.extend_from_slice(&limit.to_be_bytes());
            encoded.extend_from_slice(&attempted.to_be_bytes());
        }
        BudgetMeterRefusal::CounterOverflow { resource } => {
            encoded.push(1);
            encoded.push(resource_tag(*resource));
        }
    }
}

const fn resource_tag(resource: BudgetResourceKind) -> u8 {
    match resource {
        BudgetResourceKind::Cpu => 0,
        BudgetResourceKind::Memory => 1,
        BudgetResourceKind::StorageRead => 2,
        BudgetResourceKind::StorageWrite => 3,
        BudgetResourceKind::Output => 4,
        BudgetResourceKind::OutputBytes => 5,
        BudgetResourceKind::Table => 6,
    }
}

fn decode_meter_refusal(
    cursor: &mut ReceiptCursor<'_>,
    usage: MeteredUsage,
) -> Result<BudgetMeterRefusal, crate::fault::FailureEncodingError> {
    use crate::fault::FailureEncodingError as Error;
    let refusal_tag = cursor.take(1)?[0];
    let resource = match cursor.take(1)?[0] {
        0 => BudgetResourceKind::Cpu,
        1 => BudgetResourceKind::Memory,
        2 => BudgetResourceKind::StorageRead,
        3 => BudgetResourceKind::StorageWrite,
        4 => BudgetResourceKind::Output,
        5 => BudgetResourceKind::OutputBytes,
        6 => BudgetResourceKind::Table,
        _ => return Err(Error::Malformed),
    };
    match refusal_tag {
        0 => {
            let limit = u64::from_be_bytes(cursor.array()?);
            let attempted = u64::from_be_bytes(cursor.array()?);
            if attempted <= limit
                || resource_usage(resource, usage).is_some_and(|consumed| consumed > limit)
            {
                return Err(Error::Malformed);
            }
            Ok(BudgetMeterRefusal::BudgetExceeded {
                resource,
                limit,
                attempted,
            })
        }
        1 => Ok(BudgetMeterRefusal::CounterOverflow { resource }),
        _ => Err(Error::Malformed),
    }
}

const fn resource_usage(resource: BudgetResourceKind, usage: MeteredUsage) -> Option<u64> {
    match resource {
        BudgetResourceKind::Cpu => Some(usage.cpu_fuel),
        BudgetResourceKind::Memory => Some(usage.memory_bytes),
        BudgetResourceKind::StorageRead => Some(usage.storage_read_bytes),
        BudgetResourceKind::StorageWrite => Some(usage.storage_write_bytes),
        BudgetResourceKind::Output => Some(usage.output_values as u64),
        BudgetResourceKind::OutputBytes => Some(usage.output_bytes),
        BudgetResourceKind::Table => None,
    }
}

struct ReceiptCursor<'a> {
    remaining: &'a [u8],
}

impl<'a> ReceiptCursor<'a> {
    const fn new(remaining: &'a [u8]) -> Self {
        Self { remaining }
    }

    fn take(&mut self, length: usize) -> Result<&'a [u8], crate::fault::FailureEncodingError> {
        let (value, remaining) = self
            .remaining
            .split_at_checked(length)
            .ok_or(crate::fault::FailureEncodingError::Malformed)?;
        self.remaining = remaining;
        Ok(value)
    }

    fn array<const N: usize>(&mut self) -> Result<[u8; N], crate::fault::FailureEncodingError> {
        self.take(N)?
            .try_into()
            .map_err(|_| crate::fault::FailureEncodingError::Malformed)
    }

    const fn is_empty(&self) -> bool {
        self.remaining.is_empty()
    }
}

/// Complete immutable input to one authorized guest execution.
pub struct AuthorizedExecutionRequest<'a> {
    pub module: &'a ValidatedModule,
    pub program: ProgramId,
    pub authorization: AuthorizationContext,
    pub receipts: &'a dyn ReceiptOracle,
    pub entrypoint: &'a str,
    pub calldata: &'a [u8],
    pub composition: CompositionContext,
    pub response_capacity: usize,
}

/// Versioned activity request paired with a consumed admission token.
pub struct BudgetedAuthorizedExecutionRequest<'a> {
    request: AuthorizedExecutionRequest<'a>,
    admitted_budget: AdmittedBudget,
    payer: PrincipalId,
    activity_binding: ActivityBudgetBinding,
    execution_context: Option<ExecutionContext>,
    access_declaration: crate::AccessDeclaration,
}

impl<'a> BudgetedAuthorizedExecutionRequest<'a> {
    /// Binds independently authenticated activity identity to an admitted token.
    #[must_use]
    pub const fn new(
        request: AuthorizedExecutionRequest<'a>,
        admitted_budget: AdmittedBudget,
        payer: PrincipalId,
        activity_binding: ActivityBudgetBinding,
    ) -> Self {
        Self {
            request,
            admitted_budget,
            payer,
            activity_binding,
            execution_context: None,
            access_declaration: crate::AccessDeclaration::absent(),
        }
    }

    /// Attaches the declaration already committed by the canonical activity
    /// binding. Explicit declarations are enforced in every call frame.
    #[must_use]
    pub(crate) fn with_access_declaration(mut self, declaration: crate::AccessDeclaration) -> Self {
        self.access_declaration = declaration;
        self
    }

    /// Attaches an explicit declaration only after reproducing the kernel's
    /// canonical activity-id hash and proving the named activity byte range is
    /// exactly that declaration's canonical encoding.
    pub fn with_bound_access_declaration(
        mut self,
        declaration: crate::AccessDeclaration,
        canonical_activity: &[u8],
        declaration_offset: usize,
    ) -> Result<Self, BudgetAdmissionRefusal> {
        if canonical_activity.len() > 1_048_576 {
            return Err(BudgetAdmissionRefusal::MalformedCanonicalBytes);
        }
        let declaration_bytes = declaration
            .canonical_bytes()
            .map_err(|_| BudgetAdmissionRefusal::MalformedCanonicalBytes)?;
        let end = declaration_offset
            .checked_add(declaration_bytes.len())
            .ok_or(BudgetAdmissionRefusal::MalformedCanonicalBytes)?;
        if canonical_activity.get(declaration_offset..end) != Some(declaration_bytes.as_slice()) {
            return Err(BudgetAdmissionRefusal::ActivityBindingMismatch);
        }
        let mut preimage = b"LXP/v1/activity-id\0".to_vec();
        preimage.extend_from_slice(canonical_activity);
        let digest = crate::hash_bytes(crate::HashAlgorithm::Sha256, &preimage)
            .map_err(|_| BudgetAdmissionRefusal::MalformedCanonicalBytes)?;
        if digest != self.activity_binding.bytes() {
            return Err(BudgetAdmissionRefusal::ActivityBindingMismatch);
        }
        self.access_declaration = declaration;
        Ok(self)
    }

    pub(crate) fn with_authenticated_execution_context(
        mut self,
        execution_context: ExecutionContext,
    ) -> Self {
        self.execution_context = Some(execution_context);
        self
    }

}

impl ExecutionRecord {
    /// Encodes the execution outcome into architecture-independent evidence bytes.
    ///
    /// Every integer uses network byte order and every value carries an explicit
    /// width tag, so the same execution can be compared byte-for-byte across
    /// operating systems, CPU architectures and optimisation profiles.
    #[must_use]
    pub fn canonical_evidence(&self) -> Vec<u8> {
        let mut evidence = Vec::with_capacity(64 + self.outputs.len().saturating_mul(9));
        evidence.extend_from_slice(if self.trace.is_some() {
            b"LXP/program-execution/v3\0"
        } else {
            b"LXP/program-execution/v2\0"
        });
        evidence.extend_from_slice(&self.runtime_version.to_be_bytes());
        evidence.extend_from_slice(&self.abi_version.to_be_bytes());
        evidence.extend_from_slice(&self.metering_schedule_version.to_be_bytes());
        let native_output_count = self.outputs.len().to_be_bytes();
        let mut output_count = [0u8; 16];
        let count_offset = output_count.len() - native_output_count.len();
        output_count[count_offset..].copy_from_slice(&native_output_count);
        evidence.extend_from_slice(&output_count);
        for output in &self.outputs {
            match output {
                WasmValue::I32(value) => {
                    evidence.push(1);
                    evidence.extend_from_slice(&value.to_be_bytes());
                }
                WasmValue::I64(value) => {
                    evidence.push(2);
                    evidence.extend_from_slice(&value.to_be_bytes());
                }
            }
        }
        evidence.extend_from_slice(&self.usage.cpu_fuel.to_be_bytes());
        evidence.extend_from_slice(&self.usage.memory_bytes.to_be_bytes());
        evidence.extend_from_slice(&self.usage.storage_read_bytes.to_be_bytes());
        evidence.extend_from_slice(&self.usage.storage_write_bytes.to_be_bytes());
        evidence.extend_from_slice(&self.usage.output_values.to_be_bytes());
        evidence.extend_from_slice(&self.usage.fee_units.to_be_bytes());
        match &self.trace {
            Some(trace) => {
                evidence.push(1);
                let trace = canonical_trace_bytes(trace);
                evidence.extend_from_slice(&(trace.len() as u64).to_be_bytes());
                evidence.extend_from_slice(&trace);
            }
            None => {}
        }
        evidence
    }

    pub(crate) fn write_canonical_evidence(&self, evidence: &mut Vec<u8>) {
        evidence.clear();
        evidence.extend_from_slice(if self.trace.is_some() {
            b"LXP/program-execution/v3\0"
        } else {
            b"LXP/program-execution/v2\0"
        });
        evidence.extend_from_slice(&self.runtime_version.to_be_bytes());
        evidence.extend_from_slice(&self.abi_version.to_be_bytes());
        evidence.extend_from_slice(&self.metering_schedule_version.to_be_bytes());
        let native_output_count = self.outputs.len().to_be_bytes();
        let mut output_count = [0u8; 16];
        let count_offset = output_count.len() - native_output_count.len();
        output_count[count_offset..].copy_from_slice(&native_output_count);
        evidence.extend_from_slice(&output_count);
        for output in &self.outputs {
            match output {
                WasmValue::I32(value) => { evidence.push(1); evidence.extend_from_slice(&value.to_be_bytes()); }
                WasmValue::I64(value) => { evidence.push(2); evidence.extend_from_slice(&value.to_be_bytes()); }
            }
        }
        evidence.extend_from_slice(&self.usage.cpu_fuel.to_be_bytes());
        evidence.extend_from_slice(&self.usage.memory_bytes.to_be_bytes());
        evidence.extend_from_slice(&self.usage.storage_read_bytes.to_be_bytes());
        evidence.extend_from_slice(&self.usage.storage_write_bytes.to_be_bytes());
        evidence.extend_from_slice(&self.usage.output_values.to_be_bytes());
        evidence.extend_from_slice(&self.usage.fee_units.to_be_bytes());
        if let Some(trace) = &self.trace {
            evidence.push(1);
            let length_offset = evidence.len(); evidence.extend_from_slice(&[0; 8]);
            let start = evidence.len();
            evidence.extend_from_slice(&crate::STEP_COMMITMENT_VERSION.to_be_bytes());
            evidence.extend_from_slice(&trace.policy().canonical_bytes());
            evidence.extend_from_slice(&(trace.commitments().len() as u32).to_be_bytes());
            for commitment in trace.commitments() {
                evidence.extend_from_slice(&commitment.step_index.to_be_bytes());
                evidence.extend_from_slice(&commitment.digest);
            }
            let length = (evidence.len() - start) as u64;
            evidence[length_offset..length_offset + 8].copy_from_slice(&length.to_be_bytes());
        }
    }
}

/// Failure of an isolated execution; no instance or guest mutation is returned.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ExecutionError {
    /// Guest or engine execution fault.
    Fault(ExecutionFault),
    /// Typed resource-budget refusal.
    Resource(MeterRefusal),
    /// Capability ABI refusal before or during execution.
    Abi(AbiError),
    /// Canonical calldata entry protocol refusal.
    Entrypoint(EntrypointRefusal),
    /// Program-to-program composition refusal; no leg of the call graph was
    /// committed.
    Composition(CompositionRefusal),
    /// Candidate successful-response transport refusal.
    Response(ResponseRefusal),
    /// Caller-declared budget was structurally refused before execution.
    Budget(BudgetAdmissionRefusal),
    /// Monetary-law refusal while sealing a successful guest activity.
    Transfer(TransferLawError),
    /// Protocol-owned execution versions were absent or disagreed with the executor.
    Context(crate::abi::context::ContextRefusal),
}

impl Display for ExecutionError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Fault(fault) => write!(formatter, "execution fault: {fault}"),
            Self::Resource(refusal) => write!(formatter, "resource refusal: {refusal}"),
            Self::Abi(error) => write!(formatter, "ABI refusal: {error}"),
            Self::Entrypoint(refusal) => write!(formatter, "entrypoint refusal: {refusal}"),
            Self::Composition(refusal) => write!(formatter, "composition refusal: {refusal}"),
            Self::Response(refusal) => write!(formatter, "response refusal: {refusal}"),
            Self::Budget(refusal) => write!(formatter, "budget admission refusal: {refusal}"),
            Self::Transfer(refusal) => write!(formatter, "transfer-law refusal: {refusal}"),
            Self::Context(refusal) => write!(formatter, "execution-context refusal: {refusal:?}"),
        }
    }
}

impl std::error::Error for ExecutionError {}

/// Stateless executor creating a fresh isolated instance for every call.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Executor {
    budget: ResourceBudget,
    prices: FeeSchedule,
    runtime_version: u16,
    abi_version: u16,
    trace_policy: Option<crate::TracePolicy>,
}

impl Executor {
    /// Constructs an executor with explicit integer-only budgets and prices.
    #[must_use]
    pub const fn new(budget: ResourceBudget, prices: FeeSchedule) -> Self {
        Self {
            budget,
            prices,
            runtime_version: RUNTIME_VERSION,
            abi_version: crate::abi::manifest::ABI_V1_VERSION,
            trace_policy: None,
        }
    }

    pub(crate) const fn new_versioned(
        budget: ResourceBudget,
        prices: FeeSchedule,
        runtime_version: u16,
        abi_version: u16,
    ) -> Self {
        Self { budget, prices, runtime_version, abi_version, trace_policy: None }
    }

    pub(crate) const fn for_abi(self, abi_version: u16) -> Self {
        Self { abi_version, ..self }
    }

    /// Declares the receipt-recorded deterministic execution trace policy.
    #[must_use]
    pub const fn with_trace_policy(self, trace_policy: crate::TracePolicy) -> Self {
        Self { trace_policy: Some(trace_policy), ..self }
    }

    #[must_use]
    pub const fn trace_policy(&self) -> Option<crate::TracePolicy> {
        self.trace_policy
    }

    fn selected_revision(&self) -> Result<AbiRevision, ExecutionError> {
        match self.abi_version {
            crate::ABI_V1_VERSION => Ok(AbiRevision::V1),
            crate::ABI_V2_VERSION => Ok(AbiRevision::V2),
            _ => Err(ExecutionError::Abi(AbiError::WrongVersion)),
        }
    }

    /// Constructs the declared production executor.
    #[must_use]
    pub const fn declared() -> Self {
        Self::new(ResourceBudget::declared(), FeeSchedule::declared())
    }

    /// Admits one activity declaration before program lookup or guest execution.
    ///
    /// # Errors
    ///
    /// Returns a typed structural or payer-coverage refusal without creating a meter.
    pub(crate) fn admit_activity_budget(
        &self,
        declared: DeclaredBudget,
        coverage: PayerCoverage,
    ) -> Result<AdmittedBudget, BudgetAdmissionRefusal> {
        let resources = declared.resource_budget();
        validate_bounds(resources, self.effective_activity_maximum())?;
        let maximum_fee_units = maximum_fee_units(resources, self.prices)?;
        let (payer, activity_binding, available_fee_units) = coverage.into_parts();
        if available_fee_units < maximum_fee_units {
            return Err(BudgetAdmissionRefusal::InsufficientCoverage {
                required: maximum_fee_units,
                available: available_fee_units,
            });
        }
        Ok(AdmittedBudget::new(
            resources,
            payer,
            activity_binding,
            maximum_fee_units,
            self.prices,
            self.effective_activity_maximum(),
        ))
    }

    /// Qualification-only admission seam used until the protocol call activity
    /// constructs authenticated coverage in task 28.7.
    ///
    /// This does not read or reserve a balance. Production transition code must
    /// call the crate-internal authenticated coverage path.
    ///
    /// # Errors
    ///
    /// Returns the same typed admission refusal as the protocol transition path.
    pub fn admit_activity_budget_for_qualification(
        &self,
        declared: DeclaredBudget,
        payer: PrincipalId,
        activity_binding: ActivityBudgetBinding,
        available_fee_units: u128,
    ) -> Result<AdmittedBudget, BudgetAdmissionRefusal> {
        self.admit_activity_budget(
            declared,
            PayerCoverage::new(payer, activity_binding, available_fee_units),
        )
    }

    const fn effective_activity_maximum(&self) -> ResourceBudget {
        let protocol = ResourceBudget::declared();
        ResourceBudget::new_complete(
            if self.budget.cpu_fuel() < protocol.cpu_fuel() {
                self.budget.cpu_fuel()
            } else {
                protocol.cpu_fuel()
            },
            if self.budget.memory_bytes() < protocol.memory_bytes() {
                self.budget.memory_bytes()
            } else {
                protocol.memory_bytes()
            },
            if self.budget.storage_read_bytes() < protocol.storage_read_bytes() {
                self.budget.storage_read_bytes()
            } else {
                protocol.storage_read_bytes()
            },
            if self.budget.storage_write_bytes() < protocol.storage_write_bytes() {
                self.budget.storage_write_bytes()
            } else {
                protocol.storage_write_bytes()
            },
            if self.budget.output_values() < protocol.output_values() {
                self.budget.output_values()
            } else {
                protocol.output_values()
            },
            if self.budget.output_bytes() < protocol.output_bytes() {
                self.budget.output_bytes()
            } else {
                protocol.output_bytes()
            },
            if self.budget.table_elements() < protocol.table_elements() {
                self.budget.table_elements()
            } else {
                protocol.table_elements()
            },
        )
    }

    fn validate_budget_token(
        &self,
        admitted: &AdmittedBudget,
        payer: PrincipalId,
        activity_binding: ActivityBudgetBinding,
    ) -> Result<(), ExecutionError> {
        let refusal = if admitted.payer() != payer {
            Some(BudgetAdmissionRefusal::PayerMismatch)
        } else if admitted.activity_binding() != activity_binding {
            Some(BudgetAdmissionRefusal::ActivityBindingMismatch)
        } else if admitted.schedule() != self.prices {
            Some(BudgetAdmissionRefusal::ScheduleMismatch)
        } else if admitted.maximum_policy() != self.effective_activity_maximum() {
            Some(BudgetAdmissionRefusal::MaximumPolicyMismatch)
        } else {
            None
        };
        refusal.map_or(Ok(()), |refusal| Err(ExecutionError::Budget(refusal)))
    }

    /// Executes a validated module under a fresh store and exact resource budget.
    ///
    /// A failed call returns no instance, output or guest mutation, providing the
    /// rollback boundary consumed by the programs module transition.
    ///
    /// # Errors
    ///
    /// Returns a typed guest fault or resource refusal.
    pub fn execute(
        &self,
        module: &ValidatedModule,
        export: &str,
        args: &[WasmValue],
    ) -> Result<ExecutionRecord, ExecutionError> {
        let selected = match module.abi_revision() {
            AbiRevision::V1 => crate::ABI_V1_VERSION,
            AbiRevision::V2 => crate::ABI_V2_VERSION,
        };
        if selected != self.abi_version {
            return Err(ExecutionError::Abi(AbiError::WrongVersion));
        }
        let meter = Meter::new(self.budget, self.prices);
        let mut instance = module
            .instantiate_metered(meter)
            .map_err(|(fault, exhausted)| self.classify_fault(fault, exhausted))?;
        let identity = self.trace_policy.map(|policy| {
            trace_identity(
                module,
                export,
                &canonical_wasm_arguments(args),
                self.runtime_version,
                self.abi_version,
                self.prices.version(),
                policy,
            )
        }).transpose().map_err(ExecutionError::Fault)?;
        if let Some(policy) = self.trace_policy {
            instance.enable_execution_trace(policy).map_err(ExecutionError::Fault)?;
        }
        let outputs = match instance.call(export, args) {
            Ok(outputs) => outputs,
            Err(fault) => {
                if let Some(observer) = instance.execution_observer_fault() {
                    return Err(ExecutionError::Fault(observer));
                }
                return Err(self.classify_fault(fault, instance.meter().exhaustion()));
            }
        };
        let usage = instance
            .meter()
            .finish()
            .map_err(ExecutionError::Resource)?;
        let trace = match (self.trace_policy, identity) {
            (Some(policy), Some(identity)) => Some(
                instance.take_execution_trace(policy, identity).map_err(ExecutionError::Fault)?,
            ),
            (None, None) => None,
            _ => return Err(ExecutionError::Fault(ExecutionFault::EngineFault {
                reason: "execution trace identity and policy diverged".to_string(),
            })),
        };
        Ok(ExecutionRecord {
            runtime_version: self.runtime_version,
            abi_version: self.abi_version,
            metering_schedule_version: module.metering_schedule_version(),
            outputs,
            usage,
            trace,
        })
    }

    /// Executes an ordinary validated call while retaining exact per-step evidence.
    ///
    /// The configured policy is part of the returned trace. Observation or state
    /// conversion failures refuse the call instead of returning partial evidence.
    pub fn execute_traced(
        &self,
        module: &ValidatedModule,
        export: &str,
        args: &[WasmValue],
    ) -> Result<TracedExecutionRecord, ExecutionError> {
        self.trace_policy.ok_or_else(|| ExecutionError::Fault(
            ExecutionFault::EngineFault {
                reason: "deterministic execution trace policy is not configured".to_string(),
            },
        ))?;
        let execution = self.execute(module, export, args)?;
        let trace = execution.trace.clone().ok_or_else(|| ExecutionError::Fault(
            ExecutionFault::EngineFault { reason: "configured trace was not recorded".to_string() },
        ))?;
        Ok(TracedExecutionRecord {
            execution,
            trace,
        })
    }

    /// Executes a program with an explicit authorization context and atomic
    /// namespaced storage. Durable storage changes only after guest success and
    /// successful resource finalization.
    ///
    /// # Errors
    ///
    /// Returns typed ABI, guest, or resource refusals without committing
    /// storage or exposing partial effects.
    pub fn execute_authorized(
        &self,
        storage: &mut Storage,
        request: AuthorizedExecutionRequest<'_>,
    ) -> Result<AuthorizedExecutionRecord, ExecutionError> {
        if request.module.abi_revision() != AbiRevision::V1 {
            return Err(ExecutionError::Abi(AbiError::WrongVersion));
        }
        entrypoint::preflight(request.calldata).map_err(ExecutionError::Entrypoint)?;
        let meter = Meter::new(self.budget, self.prices);
        let principal = request.authorization.principal();
        let abi = Abi::new(
            self.abi_version,
            request.program,
            request.authorization,
            storage.clone(),
            request.receipts,
        )
        .map_err(ExecutionError::Abi)?;
        let composition = Composition::new(
            request
                .composition
                .claim_resolver(None)
                .map_err(ExecutionError::Composition)?,
            CallGraph::root(request.composition.rules(), request.program, principal),
            AbiRevision::V1,
        );
        let mut instance = request
            .module
            .instantiate_composed(meter, abi, composition)
            .map_err(|(fault, exhausted)| self.classify_fault(fault, exhausted))?;
        let identity = self.trace_policy.map(|policy| trace_identity(
            request.module,
            request.entrypoint,
            request.calldata,
            self.runtime_version,
            self.abi_version,
            self.prices.version(),
            policy,
        )).transpose().map_err(ExecutionError::Fault)?;
        if let Some(policy) = self.trace_policy {
            instance.enable_execution_trace(policy).map_err(ExecutionError::Fault)?;
        }
        let code = match entrypoint::invoke(&mut instance, request.entrypoint, request.calldata) {
            Ok(code) => code,
            Err(EntrypointRefusal::Fault(fault)) => {
                if let Some(observer) = instance.execution_observer_fault() {
                    return Err(ExecutionError::Fault(observer));
                }
                if let Some(refusal) = instance.state().refusal() {
                    return Err(ExecutionError::Composition(refusal.clone()));
                }
                return Err(self.classify_fault(fault, instance.meter().exhaustion()));
            }
            Err(EntrypointRefusal::Resource(MeterRefusal::BudgetExceeded {
                resource: ResourceKind::Cpu,
                ..
            })) => return Err(self.classify_fault(ExecutionFault::OutOfFuel, None)),
            Err(EntrypointRefusal::Resource(refusal)) => {
                return Err(ExecutionError::Resource(refusal));
            }
            Err(refusal) => return Err(ExecutionError::Entrypoint(refusal)),
        };
        if let Some(observer) = instance.execution_observer_fault() {
            return Err(ExecutionError::Fault(observer));
        }
        let usage = instance
            .meter()
            .finish()
            .map_err(ExecutionError::Resource)?;
        let trace = match (self.trace_policy, identity) {
            (Some(policy), Some(identity)) => Some(
                instance.take_execution_trace(policy, identity).map_err(ExecutionError::Fault)?,
            ),
            (None, None) => None,
            _ => return Err(ExecutionError::Fault(ExecutionFault::EngineFault {
                reason: "execution trace identity and policy diverged".to_string(),
            })),
        };
        let (_, abi, composition) = instance.into_state().into_parts();
        let committed = abi
            .ok_or(ExecutionError::Abi(AbiError::CapabilityDenied))?
            .commit();
        let call_graph = composition
            .ok_or(ExecutionError::Composition(
                CompositionRefusal::NotComposable,
            ))?
            .into_graph();
        *storage = committed.storage;
        Ok(AuthorizedExecutionRecord {
            execution: ExecutionRecord {
                runtime_version: self.runtime_version,
                abi_version: self.abi_version,
                metering_schedule_version: request.module.metering_schedule_version(),
                outputs: vec![WasmValue::I32(code)],
                usage,
                trace,
            },
            effects: committed.effects,
            call_graph,
        })
    }

    fn seal_authorized_activity(
        prior_storage: Storage,
        held_storage: Storage,
        record: AuthorizedExecutionRecord,
        transfer: TransferCapability,
    ) -> Result<PreparedAuthorizedActivity, ExecutionError> {
        let (transfer, transfer_set) = if record.effects.transfers.is_empty() {
            (None, None)
        } else {
            let transfer_set = transfer
                .authorize_for_graph(&record.effects, &record.call_graph)
                .map_err(ExecutionError::Transfer)?;
            (Some(transfer), Some(transfer_set))
        };
        Ok(PreparedAuthorizedActivity {
            record,
            prior_storage,
            held_storage,
            transfer,
            transfer_set,
        })
    }

    /// Affine settlement preparation for the production transition. The
    /// admitted token is consumed and its authenticated activity binding is
    /// the sole source of invocation authority; callers cannot mint a raw
    /// digest authority.
    pub fn prepare_authorized_activity_budgeted(
        &self,
        storage: &Storage,
        budgeted: BudgetedAuthorizedExecutionRequest<'_>,
    ) -> Result<PreparedAuthorizedActivityOutcome, ExecutionError> {
        let activity_binding = budgeted.activity_binding;
        let transfer = TransferCapability::from_root_authorization(
            budgeted.request.program,
            &budgeted.request.authorization,
            activity_binding.bytes(),
        )
        .map_err(ExecutionError::Transfer)?;
        let mut held_storage = storage.clone();
        match self.execute_authorized_budgeted(&mut held_storage, budgeted)? {
            BudgetedV1ActivityOutcome::Success(record) => {
                Self::seal_authorized_activity(storage.clone(), held_storage, record, transfer)
                    .map(PreparedAuthorizedActivityOutcome::Success)
            }
            BudgetedV1ActivityOutcome::Failure(failure) => {
                Ok(PreparedAuthorizedActivityOutcome::Failure(failure))
            }
            BudgetedV1ActivityOutcome::Resource(resource) => {
                Ok(PreparedAuthorizedActivityOutcome::Resource(resource))
            }
        }
    }

    /// Qualification-only execution of recorded ABI-v1 code under one consumed token.
    ///
    /// Frozen unbudgeted v1 execution and evidence are not changed by this additive
    /// receipt-ready bridge.
    ///
    /// # Errors
    ///
    /// Returns structural admission, ABI, entrypoint, or composition errors that
    /// occur before a receipt-ready terminal guest outcome exists.
    #[allow(clippy::too_many_lines)]
    pub fn execute_authorized_budgeted_for_qualification(
        &self,
        storage: &mut Storage,
        budgeted: BudgetedAuthorizedExecutionRequest<'_>,
    ) -> Result<BudgetedV1ActivityOutcome, ExecutionError> {
        self.execute_authorized_budgeted(storage, budgeted)
    }

    #[allow(clippy::too_many_lines)]
    pub(crate) fn execute_authorized_budgeted(
        &self,
        storage: &mut Storage,
        budgeted: BudgetedAuthorizedExecutionRequest<'_>,
    ) -> Result<BudgetedV1ActivityOutcome, ExecutionError> {
        let BudgetedAuthorizedExecutionRequest {
            request,
            admitted_budget,
            payer,
            activity_binding,
            execution_context: _,
            access_declaration,
        } = budgeted;
        self.validate_budget_token(&admitted_budget, payer, activity_binding)?;
        if request.module.abi_revision() != AbiRevision::V1 {
            return Err(ExecutionError::Abi(AbiError::WrongVersion));
        }
        entrypoint::preflight(request.calldata).map_err(ExecutionError::Entrypoint)?;
        request
            .module
            .preflight_entrypoint(request.entrypoint, request.calldata.is_empty())
            .map_err(ExecutionError::Entrypoint)?;
        let principal = request.authorization.principal();
        let reachable = request.authorization.capabilities()
            .reachable_accesses(request.program, principal)
            .map_err(|_| ExecutionError::Abi(AbiError::AccessDeclaration))?;
        let declaration_charge = access_declaration.charge(&reachable)
            .map_err(|_| ExecutionError::Abi(AbiError::AccessDeclaration))?;
        let mut abi = Abi::new(
            self.abi_version,
            request.program,
            request.authorization,
            storage.clone(),
            request.receipts,
        )
        .map_err(ExecutionError::Abi)?;
        abi.set_access_declaration(access_declaration);
        let mut meter = Meter::new_activity(admitted_budget.resource_budget(), self.prices);
        meter.charge_cpu(declaration_charge.total_units()).map_err(ExecutionError::Resource)?;
        let composition = Composition::new(
            request
                .composition
                .claim_resolver(Some(activity_binding))
                .map_err(ExecutionError::Composition)?,
            CallGraph::root(request.composition.rules(), request.program, principal),
            AbiRevision::V1,
        );
        let mut instance =
            match request
                .module
                .instantiate_composed_retained(meter, abi, composition)
            {
                Ok(instance) => instance,
                Err(error) => {
                    let (fault, state) = *error;
                    if let Some(refusal) = state.meter().budget_exhaustion() {
                        return Self::budgeted_v1_resource(request.program, activity_binding, refusal, state);
                    }
                    if let Some(refusal) = state.refusal().cloned() {
                        if let Some(resource) = composition_meter_refusal(&refusal)
                            .and_then(|resource| BudgetMeterRefusal::try_from(resource).ok())
                        {
                            return Self::budgeted_v1_resource(request.program, activity_binding, resource, state);
                        }
                        return Self::budgeted_v1_composition_failure(
                            request.program,
                            activity_binding,
                            refusal,
                            state,
                        );
                    }
                    if is_candidate_runtime_fault(&fault) {
                        return Self::budgeted_v1_program_failure(
                            request.program,
                            activity_binding,
                            request.program,
                            RefusalClass::RuntimeFault,
                            state,
                        );
                    }
                    return Err(Self::classify_fault_with_budget(
                        fault,
                        state.meter().exhaustion(),
                        admitted_budget.resource_budget(),
                    ));
                }
            };
        let identity = self.trace_policy.map(|policy| trace_identity(
            request.module,
            request.entrypoint,
            request.calldata,
            self.runtime_version,
            self.abi_version,
            self.prices.version(),
            policy,
        )).transpose().map_err(ExecutionError::Fault)?;
        if let Some(policy) = self.trace_policy {
            instance.enable_execution_trace(policy).map_err(ExecutionError::Fault)?;
        }
        let code = match entrypoint::invoke(&mut instance, request.entrypoint, request.calldata) {
            Ok(code) => code,
            Err(refusal) => {
                if let Some(observer) = instance.execution_observer_fault() {
                    return Err(ExecutionError::Fault(observer));
                }
                let exhaustion = instance.meter().budget_exhaustion();
                let carried = instance.state().refusal().cloned();
                let carried_resource = carried
                    .as_ref()
                    .and_then(composition_meter_refusal)
                    .and_then(|refusal| BudgetMeterRefusal::try_from(refusal).ok());
                let state = instance.into_state();
                if let Some(resource) = exhaustion.or(carried_resource) {
                    return Self::budgeted_v1_resource(request.program, activity_binding, resource, state);
                }
                if let Some(carried) = carried {
                    return Self::budgeted_v1_composition_failure(request.program, activity_binding, carried, state);
                }
                match refusal {
                    EntrypointRefusal::GuestRefused { .. } => {
                        return Self::budgeted_v1_program_failure(
                            request.program,
                            activity_binding,
                            request.program,
                            RefusalClass::Legacy,
                            state,
                        );
                    }
                    EntrypointRefusal::Fault(fault) if is_candidate_runtime_fault(&fault) => {
                        return Self::budgeted_v1_program_failure(
                            request.program,
                            activity_binding,
                            request.program,
                            RefusalClass::RuntimeFault,
                            state,
                        );
                    }
                    EntrypointRefusal::Fault(fault) => {
                        return Err(Self::classify_fault_with_budget(
                            fault,
                            state.meter().exhaustion(),
                            admitted_budget.resource_budget(),
                        ));
                    }
                    EntrypointRefusal::Resource(resource) => {
                        let resource = BudgetMeterRefusal::try_from(resource)
                            .map_err(ExecutionError::Resource)?;
                        return Self::budgeted_v1_resource(request.program, activity_binding, resource, state);
                    }
                    other => {
                        return Self::budgeted_v1_failure(
                            request.program,
                            activity_binding,
                            BudgetedV1FailureCause::Entrypoint(other),
                            state,
                        );
                    }
                }
            }
        };
        if let Some(observer) = instance.execution_observer_fault() {
            return Err(ExecutionError::Fault(observer));
        }
        if let Some(resource) = instance.meter().budget_exhaustion() {
            return Self::budgeted_v1_resource(request.program, activity_binding, resource, instance.into_state());
        }
        if let Some(refusal) = instance.state().refusal().cloned() {
            let state = instance.into_state();
            if let Some(resource) = composition_meter_refusal(&refusal)
                .and_then(|resource| BudgetMeterRefusal::try_from(resource).ok())
            {
                return Self::budgeted_v1_resource(request.program, activity_binding, resource, state);
            }
            return Self::budgeted_v1_composition_failure(request.program, activity_binding, refusal, state);
        }
        let usage = match instance.meter().finish() {
            Ok(usage) => usage,
            Err(resource) => return Err(ExecutionError::Resource(resource)),
        };
        let trace = match (self.trace_policy, identity) {
            (Some(policy), Some(identity)) => Some(
                instance.take_execution_trace(policy, identity).map_err(ExecutionError::Fault)?,
            ),
            (None, None) => None,
            _ => return Err(ExecutionError::Fault(ExecutionFault::EngineFault {
                reason: "execution trace identity and policy diverged".to_string(),
            })),
        };
        let (_, abi, composition) = instance.into_state().into_parts();
        let abi = abi.ok_or(ExecutionError::Abi(AbiError::CapabilityDenied))?;
        let composition = composition.ok_or(ExecutionError::Composition(
            CompositionRefusal::NotComposable,
        ))?;
        let committed = abi.commit();
        let call_graph = composition.into_graph();
        *storage = committed.storage;
        Ok(BudgetedV1ActivityOutcome::Success(
            AuthorizedExecutionRecord {
                execution: ExecutionRecord {
                    runtime_version: self.runtime_version,
                    abi_version: self.abi_version,
                    metering_schedule_version: request.module.metering_schedule_version(),
                    outputs: vec![WasmValue::I32(code)],
                    usage,
                    trace,
                },
                effects: committed.effects,
                call_graph,
            },
        ))
    }

    /// Executes an authorized activity through the explicitly selected candidate ABI.
    ///
    /// # Errors
    ///
    /// Returns typed validation, execution, composition, response, or resource refusals.
    #[allow(clippy::too_many_lines)]
    pub fn execute_authorized_candidate(
        &self,
        storage: &mut Storage,
        request: AuthorizedExecutionRequest<'_>,
    ) -> Result<CandidateAuthorizedExecutionRecord, ExecutionError> {
        let executor = self.for_abi(crate::ABI_V2_VERSION);
        executor.execute_authorized_candidate_with_budget(
            storage,
            request,
            executor.budget,
            None,
            None,
            crate::AccessDeclaration::absent(),
        )
    }

    /// Qualification-only candidate execution under one consumed admitted budget.
    ///
    /// Production transition code uses the crate-internal authenticated route;
    /// this public seam mutates only the caller-owned storage supplied here.
    ///
    /// # Errors
    ///
    /// Returns a pre-execution budget refusal when the token does not match the
    /// independently carried payer, activity binding, schedule, or maximum policy.
    pub fn execute_authorized_candidate_budgeted_for_qualification(
        &self,
        storage: &mut Storage,
        budgeted: BudgetedAuthorizedExecutionRequest<'_>,
    ) -> Result<CandidateAuthorizedExecutionRecord, ExecutionError> {
        let BudgetedAuthorizedExecutionRequest {
            request,
            admitted_budget,
            payer,
            activity_binding,
            execution_context,
            access_declaration,
        } = budgeted;
        let executor = self.for_abi(crate::ABI_V2_VERSION);
        executor.validate_budget_token(&admitted_budget, payer, activity_binding)?;
        executor.execute_authorized_candidate_with_budget(
            storage,
            request,
            admitted_budget.resource_budget(),
            Some(activity_binding),
            execution_context,
            access_declaration,
        )
    }

    pub(crate) fn execute_authorized_candidate_budgeted(
        &self,
        storage: &mut Storage,
        budgeted: BudgetedAuthorizedExecutionRequest<'_>,
    ) -> Result<CandidateAuthorizedExecutionRecord, ExecutionError> {
        let BudgetedAuthorizedExecutionRequest {
            request,
            admitted_budget,
            payer,
            activity_binding,
            execution_context,
            access_declaration,
        } = budgeted;
        self.validate_budget_token(&admitted_budget, payer, activity_binding)?;
        let execution_context = execution_context
            .ok_or(ExecutionError::Context(crate::abi::context::ContextRefusal::Unauthenticated))?;
        if !execution_context.authenticates_versions(
            self.runtime_version,
            self.abi_version,
            self.prices.version(),
        ) {
            return Err(ExecutionError::Context(
                crate::abi::context::ContextRefusal::Unauthenticated,
            ));
        }
        self.execute_authorized_candidate_with_budget(
            storage,
            request,
            admitted_budget.resource_budget(),
            Some(activity_binding),
            Some(execution_context),
            access_declaration,
        )
    }

    #[allow(clippy::too_many_lines)]
    fn execute_authorized_candidate_with_budget(
        &self,
        storage: &mut Storage,
        request: AuthorizedExecutionRequest<'_>,
        active_budget: ResourceBudget,
        activity_binding: Option<ActivityBudgetBinding>,
        execution_context: Option<ExecutionContext>,
        access_declaration: crate::AccessDeclaration,
    ) -> Result<CandidateAuthorizedExecutionRecord, ExecutionError> {
        let budgeted = activity_binding.is_some();
        if self.abi_version != crate::ABI_V2_VERSION
            || request.module.abi_revision() != AbiRevision::V2
        {
            return Err(ExecutionError::Abi(AbiError::WrongVersion));
        }
        if request.response_capacity > crate::abi::response::MAX_CALL_RESPONSE_BYTES {
            return Err(ExecutionError::Response(ResponseRefusal::TooLarge {
                bytes: request.response_capacity,
                limit: crate::abi::response::MAX_CALL_RESPONSE_BYTES,
            }));
        }
        entrypoint::preflight(request.calldata).map_err(ExecutionError::Entrypoint)?;
        if budgeted {
            request
                .module
                .preflight_entrypoint(request.entrypoint, request.calldata.is_empty())
                .map_err(ExecutionError::Entrypoint)?;
        }
        let mut meter = if budgeted {
            Meter::new_activity(active_budget, self.prices)
        } else {
            Meter::new(active_budget, self.prices)
        };
        let principal = request.authorization.principal();
        let reachable = request.authorization.capabilities()
            .reachable_accesses(request.program, principal)
            .map_err(|_| ExecutionError::Abi(AbiError::AccessDeclaration))?;
        let declaration_charge = access_declaration
            .charge(&reachable)
            .map_err(|_| ExecutionError::Abi(AbiError::AccessDeclaration))?;
        meter.charge_cpu(declaration_charge.total_units())
            .map_err(ExecutionError::Resource)?;
        let mut abi = Abi::new(
            self.abi_version,
            request.program,
            request.authorization,
            storage.clone(),
            request.receipts,
        )
        .map_err(ExecutionError::Abi)?;
        abi.set_access_declaration(access_declaration);
        let composition = Composition::new(
            request
                .composition
                .claim_resolver(activity_binding)
                .map_err(ExecutionError::Composition)?,
            CallGraph::root(request.composition.rules(), request.program, principal),
            self.selected_revision()?,
        );
        let retained = request
            .module
            .instantiate_composed_response_context_retained(
                meter,
                abi,
                composition,
                request.response_capacity,
                execution_context,
            )
            .map_err(ExecutionError::Response)?;
        let mut instance = match retained {
            Ok(instance) => instance,
            Err(error) => {
                let (fault, state) = *error;
                return self.finish_candidate_start(
                    request.program,
                    fault,
                    state,
                    active_budget,
                    budgeted,
                );
            }
        };
        let identity = self.trace_policy.map(|policy| trace_identity(
            request.module,
            request.entrypoint,
            request.calldata,
            self.runtime_version,
            self.abi_version,
            self.prices.version(),
            policy,
        )).transpose().map_err(ExecutionError::Fault)?;
        if let Some(policy) = self.trace_policy {
            instance.enable_execution_trace(policy).map_err(ExecutionError::Fault)?;
        }
        let invocation = entrypoint::invoke(&mut instance, request.entrypoint, request.calldata);
        if let Some(observer) = instance.execution_observer_fault() {
            return Err(ExecutionError::Fault(observer));
        }
        if budgeted {
            if let Some(resource) = instance
                .meter()
                .budget_exhaustion()
                .or_else(|| candidate_composition_budget_refusal(instance.state()))
            {
                let trace = match (self.trace_policy, identity) {
                    (Some(policy), Some(identity)) => Some(instance
                        .take_execution_trace(policy, identity)
                        .map_err(ExecutionError::Fault)?),
                    (None, None) => None,
                    _ => return Err(ExecutionError::Fault(ExecutionFault::EngineFault {
                        reason: "execution trace identity and policy diverged".to_string(),
                    })),
                };
                return self.candidate_resource_from_state(
                    request.program,
                    resource,
                    instance.into_state(),
                    trace,
                );
            }
        }
        let (code, failure) = match invocation {
            Ok(code) => {
                if let Some(refusal) = instance.state().refusal() {
                    return Err(ExecutionError::Composition(refusal.clone()));
                }
                if instance.state().failure().is_some() {
                    return Err(ExecutionError::Response(ResponseRefusal::CodeMismatch {
                        published: CANDIDATE_REFUSAL_SENTINEL,
                        returned: code,
                    }));
                }
                (code, None)
            }
            Err(EntrypointRefusal::GuestRefused { code }) => {
                if let Some(refusal) = instance.state().refusal() {
                    return Err(ExecutionError::Composition(refusal.clone()));
                }
                let failure = match instance.state().failure().cloned() {
                    Some(failure) if code == CANDIDATE_REFUSAL_SENTINEL => failure,
                    Some(_) => {
                        return Err(ExecutionError::Response(ResponseRefusal::CodeMismatch {
                            published: CANDIDATE_REFUSAL_SENTINEL,
                            returned: code,
                        }));
                    }
                    None if code == CANDIDATE_REFUSAL_SENTINEL => {
                        return Err(ExecutionError::Response(
                            ResponseRefusal::InvalidPublication,
                        ));
                    }
                    None => ProgramFailure::authenticated(
                        request.program,
                        RefusalClass::Legacy,
                        RefusalReason::empty(),
                    ),
                };
                (code, Some(failure))
            }
            Err(EntrypointRefusal::Fault(fault)) => {
                if let Some(refusal) = instance.state().refusal() {
                    if let CompositionRefusal::Program(failure) = refusal {
                        (
                            crate::fault::CANDIDATE_REFUSAL_SENTINEL,
                            Some(failure.clone()),
                        )
                    } else {
                        return Err(ExecutionError::Composition(refusal.clone()));
                    }
                } else if let Some(failure) = instance.state().failure().cloned() {
                    (crate::fault::CANDIDATE_REFUSAL_SENTINEL, Some(failure))
                } else if is_candidate_runtime_fault(&fault) {
                    (
                        crate::fault::CANDIDATE_REFUSAL_SENTINEL,
                        Some(ProgramFailure::authenticated(
                            request.program,
                            crate::fault::RefusalClass::RuntimeFault,
                            crate::fault::RefusalReason::empty(),
                        )),
                    )
                } else {
                    return Err(Self::classify_fault_with_budget(
                        fault,
                        instance.meter().exhaustion(),
                        active_budget,
                    ));
                }
            }
            Err(EntrypointRefusal::Resource(refusal)) => {
                if budgeted {
                    let refusal = instance
                        .meter()
                        .budget_exhaustion()
                        .or_else(|| BudgetMeterRefusal::try_from(refusal).ok())
                        .ok_or(ExecutionError::Resource(refusal))?;
                    let trace = match (self.trace_policy, identity) {
                        (Some(policy), Some(identity)) => Some(instance
                            .take_execution_trace(policy, identity)
                            .map_err(ExecutionError::Fault)?),
                        (None, None) => None,
                        _ => return Err(ExecutionError::Fault(ExecutionFault::EngineFault {
                            reason: "execution trace identity and policy diverged".to_string(),
                        })),
                    };
                    return self.candidate_resource_from_state(
                        request.program,
                        refusal,
                        instance.into_state(),
                        trace,
                    );
                }
                if let Some(CompositionRefusal::Program(failure)) = instance.state().refusal() {
                    (CANDIDATE_REFUSAL_SENTINEL, Some(failure.clone()))
                } else if let Some(failure) = instance.state().failure().cloned() {
                    (CANDIDATE_REFUSAL_SENTINEL, Some(failure))
                } else {
                    return Err(ExecutionError::Resource(refusal));
                }
            }
            Err(EntrypointRefusal::AllocationRefused { .. }) if budgeted => (
                CANDIDATE_REFUSAL_SENTINEL,
                Some(ProgramFailure::authenticated(
                    request.program,
                    RefusalClass::Legacy,
                    RefusalReason::empty(),
                )),
            ),
            Err(refusal) => return Err(ExecutionError::Entrypoint(refusal)),
        };
        if let Some(failure) = failure {
            let usage = instance
                .meter()
                .finish_published_failure()
                .map_err(ExecutionError::Resource)?;
            let trace = match (self.trace_policy, identity) {
                (Some(policy), Some(identity)) => Some(
                    instance.take_execution_trace(policy, identity).map_err(ExecutionError::Fault)?,
                ),
                (None, None) => None,
                _ => return Err(ExecutionError::Fault(ExecutionFault::EngineFault {
                    reason: "execution trace identity and policy diverged".to_string(),
                })),
            };
            let mut state = instance.into_state();
            let failure_graph = state.take_failure_graph();
            let (_, _, composition) = state.into_parts();
            let call_graph = failure_graph
                .or_else(|| composition.map(Composition::into_graph))
                .ok_or(ExecutionError::Composition(
                    CompositionRefusal::NotComposable,
                ))?;
            return Ok(CandidateAuthorizedExecutionRecord {
                root_program: request.program,
                abi_revision: request.module.abi_revision(),
                execution: CandidateExecutionRecord {
                    runtime_version: self.runtime_version,
                    fee_schedule_version: self.prices.version(),
                    metering_schedule_version: request.module.metering_schedule_version(),
                    outputs: vec![WasmValue::I32(code)],
                    usage,
                    trace,
                },
                outcome: CandidateActivityOutcome::Failure(failure),
                call_graph,
            });
        }
        let response = match instance.state().finalize_response(code) {
            Ok(response) => response,
            Err(ResponseRefusal::Meter(refusal)) if budgeted => {
                let refusal = instance
                    .meter()
                    .budget_exhaustion()
                    .or_else(|| BudgetMeterRefusal::try_from(refusal).ok())
                    .ok_or(ExecutionError::Resource(refusal))?;
                let trace = match (self.trace_policy, identity) {
                    (Some(policy), Some(identity)) => Some(instance
                        .take_execution_trace(policy, identity)
                        .map_err(ExecutionError::Fault)?),
                    (None, None) => None,
                    _ => return Err(ExecutionError::Fault(ExecutionFault::EngineFault {
                        reason: "execution trace identity and policy diverged".to_string(),
                    })),
                };
                return self.candidate_resource_from_state(
                    request.program,
                    refusal,
                    instance.into_state(),
                    trace,
                );
            }
            Err(ResponseRefusal::Meter(refusal)) => return Err(ExecutionError::Resource(refusal)),
            Err(refusal) => return Err(ExecutionError::Response(refusal)),
        };
        let usage = instance
            .meter()
            .finish()
            .map_err(ExecutionError::Resource)?;
        let trace = match (self.trace_policy, identity) {
            (Some(policy), Some(identity)) => Some(
                instance.take_execution_trace(policy, identity).map_err(ExecutionError::Fault)?,
            ),
            (None, None) => None,
            _ => return Err(ExecutionError::Fault(ExecutionFault::EngineFault {
                reason: "execution trace identity and policy diverged".to_string(),
            })),
        };
        let (_, abi, composition) = instance.into_state().into_parts();
        let committed = abi
            .ok_or(ExecutionError::Abi(AbiError::CapabilityDenied))?
            .commit();
        let call_graph = composition
            .ok_or(ExecutionError::Composition(
                CompositionRefusal::NotComposable,
            ))?
            .into_graph();
        *storage = committed.storage;
        Ok(CandidateAuthorizedExecutionRecord {
            root_program: request.program,
            abi_revision: request.module.abi_revision(),
            execution: CandidateExecutionRecord {
                runtime_version: self.runtime_version,
                fee_schedule_version: self.prices.version(),
                metering_schedule_version: request.module.metering_schedule_version(),
                outputs: vec![WasmValue::I32(code)],
                usage,
                trace,
            },
            outcome: CandidateActivityOutcome::Success {
                response,
                effects: committed.effects,
            },
            call_graph,
        })
    }

    fn finish_candidate_start(
        &self,
        program: ProgramId,
        fault: ExecutionFault,
        state: RuntimeState,
        active_budget: ResourceBudget,
        budgeted: bool,
    ) -> Result<CandidateAuthorizedExecutionRecord, ExecutionError> {
        if budgeted {
            if let Some(resource) = state
                .meter()
                .budget_exhaustion()
                .or_else(|| candidate_composition_budget_refusal(&state))
            {
                return self.candidate_resource_from_state(program, resource, state, None);
            }
        }
        if let Some(refusal) = state.refusal() {
            if let CompositionRefusal::Program(failure) = refusal {
                return self.candidate_failure_from_state(program, failure.clone(), state);
            }
            return Err(ExecutionError::Composition(refusal.clone()));
        }
        let failure = state.failure().cloned().unwrap_or_else(|| {
            ProgramFailure::authenticated(
                program,
                RefusalClass::RuntimeFault,
                RefusalReason::empty(),
            )
        });
        if !is_candidate_runtime_fault(&fault) && state.failure().is_none() {
            return Err(Self::classify_fault_with_budget(
                fault,
                state.meter().exhaustion(),
                active_budget,
            ));
        }
        self.candidate_failure_from_state(program, failure, state)
    }

    fn candidate_failure_from_state(
        &self,
        program: ProgramId,
        failure: ProgramFailure,
        mut state: RuntimeState,
    ) -> Result<CandidateAuthorizedExecutionRecord, ExecutionError> {
        let usage = state
            .meter()
            .finish_published_failure()
            .map_err(ExecutionError::Resource)?;
        let metering_schedule_version = state.metering_schedule_version();
        let failure_graph = state.take_failure_graph();
        let (_, _, composition) = state.into_parts();
        let call_graph = failure_graph
            .or_else(|| composition.map(Composition::into_graph))
            .ok_or(ExecutionError::Composition(
                CompositionRefusal::NotComposable,
            ))?;
        Ok(CandidateAuthorizedExecutionRecord {
            root_program: program,
            abi_revision: self.selected_revision()?,
            execution: CandidateExecutionRecord {
                runtime_version: self.runtime_version,
                fee_schedule_version: self.prices.version(),
                metering_schedule_version,
                outputs: vec![WasmValue::I32(CANDIDATE_REFUSAL_SENTINEL)],
                usage,
                trace: None,
            },
            outcome: CandidateActivityOutcome::Failure(failure),
            call_graph,
        })
    }

    fn candidate_resource_from_state(
        &self,
        program: ProgramId,
        refusal: BudgetMeterRefusal,
        mut state: RuntimeState,
        trace: Option<crate::ExecutionTrace>,
    ) -> Result<CandidateAuthorizedExecutionRecord, ExecutionError> {
        let usage = state
            .meter()
            .finish_resource_failure()
            .map_err(ExecutionError::Resource)?;
        let metering_schedule_version = state.metering_schedule_version();
        let failure_graph = state.take_failure_graph();
        let (_, _, composition) = state.into_parts();
        let call_graph = failure_graph
            .or_else(|| composition.map(Composition::into_graph))
            .ok_or(ExecutionError::Composition(
                CompositionRefusal::NotComposable,
            ))?;
        Ok(CandidateAuthorizedExecutionRecord {
            root_program: program,
            abi_revision: self.selected_revision()?,
            execution: CandidateExecutionRecord {
                runtime_version: self.runtime_version,
                fee_schedule_version: self.prices.version(),
                metering_schedule_version,
                outputs: Vec::new(),
                usage,
                trace,
            },
            outcome: CandidateActivityOutcome::Resource(refusal),
            call_graph,
        })
    }

    fn budgeted_v1_resource(
        root_program: ProgramId,
        activity_binding: ActivityBudgetBinding,
        refusal: BudgetMeterRefusal,
        mut state: RuntimeState,
    ) -> Result<BudgetedV1ActivityOutcome, ExecutionError> {
        let usage = state
            .meter()
            .finish_resource_failure()
            .map_err(ExecutionError::Resource)?;
        let failure_graph = state.take_failure_graph();
        let (_, _, composition) = state.into_parts();
        let call_graph = failure_graph
            .or_else(|| composition.map(Composition::into_graph))
            .ok_or(ExecutionError::Composition(
                CompositionRefusal::NotComposable,
            ))?;
        Ok(BudgetedV1ActivityOutcome::Resource(
            BudgetedResourceFailureRecord {
                root_program,
                activity_binding,
                refusal,
                usage,
                call_graph,
            },
        ))
    }

    fn budgeted_v1_program_failure(
        root_program: ProgramId,
        activity_binding: ActivityBudgetBinding,
        refusing_program: ProgramId,
        class: RefusalClass,
        state: RuntimeState,
    ) -> Result<BudgetedV1ActivityOutcome, ExecutionError> {
        Self::budgeted_v1_failure(
            root_program,
            activity_binding,
            BudgetedV1FailureCause::Program(ProgramFailure::authenticated(
                refusing_program,
                class,
                RefusalReason::empty(),
            )),
            state,
        )
    }

    fn budgeted_v1_composition_failure(
        root_program: ProgramId,
        activity_binding: ActivityBudgetBinding,
        refusal: CompositionRefusal,
        state: RuntimeState,
    ) -> Result<BudgetedV1ActivityOutcome, ExecutionError> {
        let cause = match refusal {
            CompositionRefusal::NotComposable => {
                return Err(ExecutionError::Composition(
                    CompositionRefusal::NotComposable,
                ));
            }
            CompositionRefusal::GuestRefused { program, .. } => {
                BudgetedV1FailureCause::Program(ProgramFailure::authenticated(
                    program,
                    RefusalClass::Legacy,
                    RefusalReason::empty(),
                ))
            }
            CompositionRefusal::Program(failure) => BudgetedV1FailureCause::Program(failure),
            CompositionRefusal::Fault(fault) if is_candidate_runtime_fault(&fault) => {
                BudgetedV1FailureCause::Program(ProgramFailure::authenticated(
                    failed_program(&state, root_program),
                    RefusalClass::RuntimeFault,
                    RefusalReason::empty(),
                ))
            }
            CompositionRefusal::Fault(fault) => return Err(ExecutionError::Fault(fault)),
            other => BudgetedV1FailureCause::Composition(other),
        };
        Self::budgeted_v1_failure(root_program, activity_binding, cause, state)
    }

    fn budgeted_v1_failure(
        root_program: ProgramId,
        activity_binding: ActivityBudgetBinding,
        cause: BudgetedV1FailureCause,
        mut state: RuntimeState,
    ) -> Result<BudgetedV1ActivityOutcome, ExecutionError> {
        let usage = match state.meter().finish() {
            Ok(usage) => usage,
            Err(resource) => return Err(ExecutionError::Resource(resource)),
        };
        let failure_graph = state.take_failure_graph();
        let (_, _, composition) = state.into_parts();
        let call_graph = failure_graph
            .or_else(|| composition.map(Composition::into_graph))
            .ok_or(ExecutionError::Composition(
                CompositionRefusal::NotComposable,
            ))?;
        Ok(Self::budgeted_v1_failure_with_usage(
            root_program,
            activity_binding,
            cause,
            usage,
            call_graph,
        ))
    }

    fn budgeted_v1_failure_with_usage(
        root_program: ProgramId,
        activity_binding: ActivityBudgetBinding,
        cause: BudgetedV1FailureCause,
        usage: MeteredUsage,
        call_graph: CallGraph,
    ) -> BudgetedV1ActivityOutcome {
        BudgetedV1ActivityOutcome::Failure(BudgetedV1FailureRecord {
            root_program,
            activity_binding,
            cause,
            usage,
            call_graph,
        })
    }

    fn classify_fault(
        &self,
        fault: ExecutionFault,
        exhausted: Option<MeterRefusal>,
    ) -> ExecutionError {
        Self::classify_fault_with_budget(fault, exhausted, self.budget)
    }

    fn classify_fault_with_budget(
        fault: ExecutionFault,
        exhausted: Option<MeterRefusal>,
        active_budget: ResourceBudget,
    ) -> ExecutionError {
        if let Some(refusal) = exhausted {
            return ExecutionError::Resource(refusal);
        }
        match fault {
            ExecutionFault::Resource { refusal } => ExecutionError::Resource(refusal),
            ExecutionFault::OutOfFuel => ExecutionError::Resource(MeterRefusal::BudgetExceeded {
                resource: ResourceKind::Cpu,
                limit: active_budget.cpu_fuel(),
                attempted: active_budget.cpu_fuel().saturating_add(1),
            }),
            ExecutionFault::GrowthLimited => {
                ExecutionError::Resource(MeterRefusal::BudgetExceeded {
                    resource: ResourceKind::Memory,
                    limit: active_budget.memory_bytes(),
                    attempted: active_budget.memory_bytes().saturating_add(1),
                })
            }
            other => ExecutionError::Fault(other),
        }
    }
}

fn is_candidate_runtime_fault(fault: &ExecutionFault) -> bool {
    !matches!(
        fault,
        ExecutionFault::EngineFault { .. }
            | ExecutionFault::UnknownExport { .. }
            | ExecutionFault::NotAFunction { .. }
            | ExecutionFault::OutOfFuel
            | ExecutionFault::GrowthLimited
            | ExecutionFault::Resource { .. }
    )
}

fn failed_program(state: &RuntimeState, root: ProgramId) -> ProgramId {
    state
        .failure_graph()
        .and_then(CallGraph::current)
        .or_else(|| {
            state
                .composition()
                .and_then(|composition| composition.graph().current())
        })
        .map_or(root, |frame| frame.program())
}

const fn composition_meter_refusal(refusal: &CompositionRefusal) -> Option<MeterRefusal> {
    match refusal {
        CompositionRefusal::Resource(refusal)
        | CompositionRefusal::Authority(AbiError::Meter(refusal))
        | CompositionRefusal::Response(ResponseRefusal::Meter(refusal)) => Some(*refusal),
        _ => None,
    }
}

fn candidate_composition_budget_refusal(state: &RuntimeState) -> Option<BudgetMeterRefusal> {
    state
        .refusal()
        .and_then(composition_meter_refusal)
        .and_then(|refusal| BudgetMeterRefusal::try_from(refusal).ok())
}

impl Default for Executor {
    fn default() -> Self {
        Self::declared()
    }
}

#[cfg(test)]
mod budgeted_v1_invariant_tests {
    use super::*;

    fn isolated_activity_state() -> RuntimeState {
        RuntimeState::isolated(Meter::new_activity(
            ResourceBudget::declared(),
            FeeSchedule::declared(),
        ))
    }

    #[test]
    fn missing_composition_is_fatal_for_budgeted_v1_terminal_records() {
        let program =
            ProgramId::new([0xa5; 32]).unwrap_or_else(|error| panic!("program identity: {error}"));
        assert_eq!(
            Executor::budgeted_v1_resource(
                program,
                ActivityBudgetBinding::new([7; 32]).unwrap_or_else(|error| panic!("binding: {error}")),
                BudgetMeterRefusal::BudgetExceeded {
                    resource: BudgetResourceKind::Cpu,
                    limit: 3,
                    attempted: 4,
                },
                isolated_activity_state(),
            ),
            Err(ExecutionError::Composition(
                CompositionRefusal::NotComposable
            ))
        );
        assert_eq!(
            Executor::budgeted_v1_failure(
                program,
                ActivityBudgetBinding::new([7; 32]).unwrap_or_else(|error| panic!("binding: {error}")),
                BudgetedV1FailureCause::Abi(AbiError::CapabilityDenied),
                isolated_activity_state(),
            ),
            Err(ExecutionError::Composition(
                CompositionRefusal::NotComposable
            ))
        );
    }
}
