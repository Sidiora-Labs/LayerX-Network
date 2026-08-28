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

/// A function reference identified only by its canonical position in the
/// deterministically reached instance graph.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ExecutionFunctionRef {
    pub instance_index: u32,
    pub function_index: u32,
}

/// One function-reference table in module-index order.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExecutionTable {
    pub table_index: u32,
    pub minimum: u32,
    pub maximum: Option<u32>,
    pub elements: Vec<Option<ExecutionFunctionRef>>,
}

/// One linear memory owned by a reached instance.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExecutionMemory {
    pub memory_index: u32,
    pub initial_pages: u32,
    pub maximum_pages: Option<u32>,
    pub bytes: Vec<u8>,
}

/// One instantiated passive data segment. `dropped` distinguishes a dropped
/// segment from a live, zero-length segment.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExecutionDataSegment {
    pub segment_index: u32,
    pub dropped: bool,
    pub bytes: Vec<u8>,
}

/// One instantiated passive element segment in module-index order.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExecutionElementSegment {
    pub segment_index: u32,
    pub dropped: bool,
    pub elements: Vec<Option<ExecutionFunctionRef>>,
}

/// Mutable state owned by an instance reached from the executing root through
/// Wasm function references. Instances are assigned indices in breadth-first,
/// module-function order.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExecutionInstanceState {
    pub instance_index: u32,
    pub memories: Vec<ExecutionMemory>,
    pub globals: Vec<ExecutionGlobal>,
    pub tables: Vec<ExecutionTable>,
    pub data_segments: Vec<ExecutionDataSegment>,
    pub element_segments: Vec<ExecutionElementSegment>,
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
    pub instance_state_bytes: u64,
    pub arbitration_engine_canonical_bytes: u64,
    pub host_state_bytes: u64,
    pub storage_overlay_bytes: u64,
    pub instruction_bytes: u64,
    pub retained_instruction_bytes: u64,
}

impl ObservationCharge {
    pub fn total_bytes(self) -> Option<u64> {
        self.value_bytes.checked_add(self.frame_bytes)?
            .checked_add(self.local_bytes)?.checked_add(self.global_bytes)?
            .checked_add(self.memory_bytes)?.checked_add(self.instance_state_bytes)?
            .checked_add(self.host_state_bytes)?
            .checked_add(self.storage_overlay_bytes)?
            .checked_add(self.instruction_bytes)?.checked_add(self.retained_instruction_bytes)
    }

    pub fn total_work(self) -> Option<u64> {
        self.total_bytes()?.checked_add(self.arbitration_engine_canonical_bytes)
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
    pub arbitration_host_state_root: [u8; 32],
    pub arbitration_host_state_bytes: u64,
    pub arbitration_base_state_root: [u8; 32],
    pub arbitration_receipt_oracle_root: [u8; 32],
    pub arbitration_balance_oracle_root: [u8; 32],
    pub arbitration_engine_canonical_bytes: u64,
    pub arbitration_instance_retained_bytes: u64,
    pub arbitration_canonical_state_bytes: u64,
    pub arbitration_commitment_fuel: u64,
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
    pub arbitration_instances: Vec<ExecutionInstanceState>,
    pub control_stack: Vec<ExecutionControlFrame>,
    pub canonical_instruction: Vec<u8>,
    pub instruction_fuel: u64,
    pub memory_expansion_bytes: u64,
    pub supplement: ExecutionSupplement,
}

impl ExecutionSnapshot {
    /// Exact owned `Vec` backing bytes retained by this captured snapshot.
    pub fn retained_vec_bytes(&self) -> Option<u64> {
        fn vec_bytes<T>(value: &Vec<T>) -> Option<u64> {
            u64::try_from(value.capacity().checked_mul(core::mem::size_of::<T>())?).ok()
        }
        let mut total = vec_bytes(&self.value_stack)?.checked_add(vec_bytes(&self.call_frames)?)?
            .checked_add(vec_bytes(&self.linear_memory)?)?.checked_add(vec_bytes(&self.globals)?)?
            .checked_add(vec_bytes(&self.arbitration_instances)?)?.checked_add(vec_bytes(&self.control_stack)?)?
            .checked_add(vec_bytes(&self.canonical_instruction)?)?.checked_add(vec_bytes(&self.supplement.storage_overlay)?)?;
        for frame in &self.call_frames { total = total.checked_add(vec_bytes(&frame.locals)?)?; }
        for (key, value) in &self.supplement.storage_overlay {
            total = total.checked_add(vec_bytes(key)?)?;
            if let Some(value) = value { total = total.checked_add(vec_bytes(value)?)?; }
        }
        for instance in &self.arbitration_instances {
            total = total.checked_add(vec_bytes(&instance.memories)?)?.checked_add(vec_bytes(&instance.globals)?)?
                .checked_add(vec_bytes(&instance.tables)?)?.checked_add(vec_bytes(&instance.data_segments)?)?
                .checked_add(vec_bytes(&instance.element_segments)?)?;
            for memory in &instance.memories { total = total.checked_add(vec_bytes(&memory.bytes)?)?; }
            for table in &instance.tables { total = total.checked_add(vec_bytes(&table.elements)?)?; }
            for data in &instance.data_segments { total = total.checked_add(vec_bytes(&data.bytes)?)?; }
            for element in &instance.element_segments { total = total.checked_add(vec_bytes(&element.elements)?)?; }
        }
        Some(total)
    }
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
