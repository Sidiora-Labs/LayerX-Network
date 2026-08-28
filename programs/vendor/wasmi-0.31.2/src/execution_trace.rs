//! Deterministic execution-state observations for protocol integrations.

use alloc::{sync::Arc, vec::Vec};

/// A value type retained from validated WebAssembly at an instruction boundary.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExecutionValueType {
    I32,
    I64,
}

/// One typed interpreter value represented by its canonical integer bits.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ExecutionValue {
    pub value_type: ExecutionValueType,
    pub bits: u64,
}

/// One active function frame, including its exact local values.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExecutionFrame {
    pub function_index: u32,
    pub return_program_counter: Option<u64>,
    pub locals: Vec<ExecutionValue>,
}

/// One global in module-index order. Mutability is retained as consensus state.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ExecutionGlobal {
    pub global_index: u32,
    pub mutable: bool,
    pub value: ExecutionValue,
}

/// Allocation-free upper bound presented to the host before an observation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct ObservationCharge {
    pub collect: bool,
    pub value_bytes: u64,
    pub frame_bytes: u64,
    pub local_bytes: u64,
    pub global_bytes: u64,
    pub memory_bytes: u64,
    pub storage_overlay_bytes: u64,
    pub instruction_bytes: u64,
    pub retained_instruction_bytes: u64,
}

impl ObservationCharge {
    pub fn total_bytes(self) -> Option<u64> {
        self.value_bytes.checked_add(self.frame_bytes)?
            .checked_add(self.local_bytes)?.checked_add(self.global_bytes)?
            .checked_add(self.memory_bytes)?.checked_add(self.storage_overlay_bytes)?
            .checked_add(self.instruction_bytes)?.checked_add(self.retained_instruction_bytes)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExecutionControlKind { Block, If, Else, Loop }

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ExecutionControlFrame {
    pub kind: ExecutionControlKind,
    pub operand_stack_height: u32,
    pub unreachable: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct ExecutionSupplement {
    pub storage_overlay: Vec<(Vec<u8>, Option<Vec<u8>>)>,
    pub authoritative_fuel: u64,
    pub authoritative_usage: ExecutionMeteredUsage,
    pub canonical_state_bytes: u64,
    pub commitment_fuel: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct ExecutionMeteredUsage {
    pub cpu_fuel: u64,
    pub memory_bytes: u64,
    pub storage_read_bytes: u64,
    pub storage_write_bytes: u64,
    pub output_values: u32,
    pub output_bytes: u64,
    pub occupancy_byte_batches: u128,
    pub occupancy_fee_units: u128,
    pub fee_units: u128,
}

/// Complete engine-owned state immediately before a source WebAssembly operator.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExecutionSnapshot {
    pub step_index: u64,
    pub program_counter: u64,
    pub value_stack: Vec<ExecutionValue>,
    pub call_frames: Vec<ExecutionFrame>,
    pub linear_memory: Vec<u8>,
    pub globals: Vec<ExecutionGlobal>,
    pub control_stack: Vec<ExecutionControlFrame>,
    pub canonical_instruction: Vec<u8>,
    pub instruction_fuel: u64,
    pub memory_expansion_bytes: u64,
    pub supplement: ExecutionSupplement,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExecutionTransition {
    pub pre: Arc<ExecutionSnapshot>,
    pub post: Arc<ExecutionSnapshot>,
    pub memory_expansion_bytes: u64,
}

/// Fail-closed state of deterministic execution observation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExecutionObserverError {
    SnapshotLimitExceeded,
    StepCounterOverflow,
    SupplementRejected,
    InvalidInterval,
    UnsupportedState,
}

/// Validated source metadata attached to one translated instruction boundary.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct InstructionMetadata {
    pub(crate) program_counter: u64,
    pub(crate) canonical_instruction: Vec<u8>,
    pub(crate) instruction_fuel: u64,
    pub(crate) operand_types: Vec<ExecutionValueType>,
    pub(crate) control_stack: Vec<ExecutionControlFrame>,
}

impl ExecutionValueType {
    pub(crate) fn from_validator(value_type: wasmparser::ValType) -> Option<Self> {
        match value_type {
            wasmparser::ValType::I32 => Some(Self::I32),
            wasmparser::ValType::I64 => Some(Self::I64),
            _ => None,
        }
    }
}

/// Store-owned bounded observer state. It contains no host or process-local input.
#[derive(Debug, Default)]
pub(crate) struct ExecutionObserver {
    pub(crate) interval: u64,
    pub(crate) maximum_snapshots: usize,
    pub(crate) retained_snapshots: usize,
    pub(crate) step_index: u64,
    pub(crate) transitions: Vec<ExecutionTransition>,
    pub(crate) pending: Option<Arc<ExecutionSnapshot>>,
    pub(crate) error: Option<ExecutionObserverError>,
    pub(crate) supplement: ExecutionSupplement,
    pub(crate) boundary_authorized: bool,
    pub(crate) aggregate_bytes: u64,
    pub(crate) aggregate_work: u64,
    pub(crate) maximum_bytes: u64,
    pub(crate) maximum_work: u64,
    pub(crate) sampled_current: bool,
}

impl ExecutionObserver {
    pub(crate) fn enter_boundary(
        &mut self,
    ) -> Result<bool, ExecutionObserverError> {
        if self.interval == 0 {
            self.error = Some(ExecutionObserverError::InvalidInterval);
            return Err(ExecutionObserverError::InvalidInterval)
        }
        self.sampled_current = self.step_index % self.interval == 0;
        let should_record = self.sampled_current || self.pending.is_some();
        if should_record && self.retained_snapshots >= self.maximum_snapshots {
            self.error = Some(ExecutionObserverError::SnapshotLimitExceeded);
            return Err(ExecutionObserverError::SnapshotLimitExceeded)
        }
        self.step_index = self.step_index.checked_add(1).ok_or_else(|| {
            self.error = Some(ExecutionObserverError::StepCounterOverflow);
            ExecutionObserverError::StepCounterOverflow
        })?;
        Ok(should_record)
    }
}
