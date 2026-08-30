//! Canonical commitments to complete deterministic execution state.

use core::fmt::{self, Display};
use std::sync::Arc;

use sha2::{Digest, Sha256};

use crate::meter::MeteredUsage;

/// Domain separating execution-state commitments from every other protocol hash.
pub const STEP_COMMITMENT_DOMAIN: &[u8] = b"layerx-programs-step-commitment-v1\0";
/// Version of the frozen canonical execution-state encoding.
pub const STEP_COMMITMENT_VERSION: u16 = 1;
/// Arbitration-capable execution-state encoding. Version one remains frozen
/// receipt evidence but is not a complete single-step pre-state.
pub const ARBITRATION_STEP_COMMITMENT_VERSION: u16 = 2;
/// Domain separating complete arbitration commitments from legacy v1 state.
pub const ARBITRATION_STEP_COMMITMENT_DOMAIN: &[u8] =
    b"layerx-programs-arbitration-step-commitment-v2\0";
/// Fuel charged for the fixed work of producing one commitment.
pub const STEP_COMMITMENT_BASE_FUEL: u64 = 32;
/// Fuel charged for each byte in the committed canonical state.
pub const STEP_COMMITMENT_FUEL_PER_BYTE: u64 = 1;
/// Protocol bound on a single encoded state proof.
pub const MAX_STEP_STATE_BYTES: usize = 32 * 1_024 * 1_024;
/// Protocol bound on commitments carried by one execution trace.
pub const MAX_TRACE_COMMITMENTS: usize = 65_536;
/// Maximum validated source instruction encoding retained in a step witness.
pub const MAX_STEP_INSTRUCTION_BYTES: usize = 65_536;
/// Aggregate canonical state bytes retained by one trace.
pub const MAX_TRACE_STATE_BYTES: u64 = 64 * 1_024 * 1_024;
/// Maximum canonical engine-owned extension in one arbitration state.
pub const MAX_ARBITRATION_ENGINE_STATE_BYTES: usize = 32 * 1_024 * 1_024;
/// Maximum canonical host-owned extension in one arbitration state.
pub const MAX_ARBITRATION_HOST_STATE_BYTES: usize = 32 * 1_024 * 1_024;
/// Maximum complete v2 state evidence retained or encoded for one boundary.
pub const MAX_ARBITRATION_STATE_BYTES: usize = 64 * 1_024 * 1_024;

/// Immutable execution identity bound into every state commitment.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ExecutionTraceIdentity {
    pub module_code_hash: [u8; 32],
    pub input_digest: [u8; 32],
    pub execution_parameters_digest: [u8; 32],
}

/// Version-addressed identity for a state that is eligible for arbitration.
/// Immutable host inputs are authenticated here instead of copied into every
/// mutable boundary snapshot.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ArbitrationExecutionIdentity {
    pub module_code_hash: [u8; 32],
    pub input_digest: [u8; 32],
    pub runtime_version: u16,
    pub abi_version: u16,
    pub fee_schedule_version: u32,
    pub metering_schedule_version: u32,
    pub trace_policy: TracePolicy,
    pub host_base_state_root: [u8; 32],
    pub receipt_oracle_root: [u8; 32],
    pub balance_oracle_root: [u8; 32],
}

/// Complete version-two arbitration pre-state. `legacy` preserves the frozen
/// v1 fields while the canonical engine and host extensions cover every
/// transition-determining component that v1 omitted.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ArbitrationExecutionState {
    pub identity: ArbitrationExecutionIdentity,
    pub legacy: Arc<ExecutionState>,
    pub engine_state: Vec<u8>,
    pub host_state_root: [u8; 32],
    pub host_state_bytes: u64,
}

/// Digest and exact accounting metadata for an arbitration-capable boundary.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ArbitrationStepCommitment {
    pub version: u16,
    pub step_index: u64,
    pub digest: [u8; 32],
    pub encoded_state_bytes: u32,
    pub commitment_fuel: u64,
}

/// Integer value admitted by the deterministic Programs execution subset.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExecutionValue {
    I32(i32),
    I64(i64),
}

/// One active interpreter frame at a step boundary.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExecutionFrame {
    pub function_index: u32,
    pub return_program_counter: Option<u64>,
    pub locals: Vec<ExecutionValue>,
}

/// One structured interpreter control label at a step boundary.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ExecutionControlFrame {
    pub kind: u8,
    pub operand_stack_height: u32,
    pub unreachable: bool,
}

/// One mutable global, ordered by its module global index.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ExecutionGlobal {
    pub global_index: u32,
    pub mutable: bool,
    pub value: ExecutionValue,
}

/// A write or deletion staged in the activity's atomic storage overlay.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StorageOverlayEntry {
    Write { key: Vec<u8>, value: Vec<u8> },
    Delete { key: Vec<u8> },
}

impl StorageOverlayEntry {
    fn key(&self) -> &[u8] {
        match self {
            Self::Write { key, .. } | Self::Delete { key } => key,
        }
    }
}

/// Complete deterministic runtime state at a single instruction boundary.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExecutionState {
    pub module_code_hash: [u8; 32],
    pub input_digest: [u8; 32],
    pub execution_parameters_digest: [u8; 32],
    pub step_index: u64,
    pub program_counter: u64,
    pub value_stack: Vec<ExecutionValue>,
    pub call_frames: Vec<ExecutionFrame>,
    pub control_stack: Vec<ExecutionControlFrame>,
    pub linear_memory: Vec<u8>,
    pub globals: Vec<ExecutionGlobal>,
    pub storage_overlay: Vec<StorageOverlayEntry>,
    pub fuel_remaining: u64,
    pub metered_usage: MeteredUsage,
}

/// One exact deterministic interpreter transition retained for arbitration.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExecutionStep {
    pub instruction: Vec<u8>,
    pub instruction_fuel: u64,
    pub memory_expansion_bytes: u64,
    pub pre_state: Arc<ExecutionState>,
    pub post_state: Arc<ExecutionState>,
    pub pre_commitment: StepCommitment,
    pub post_commitment: StepCommitment,
}

/// One exact source transition with complete v2 arbitration state.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ArbitrationExecutionStep {
    pub instruction: Vec<u8>,
    pub instruction_fuel: u64,
    pub memory_expansion_bytes: u64,
    pub pre_state: Arc<ArbitrationExecutionState>,
    pub post_state: Arc<ArbitrationExecutionState>,
    pub pre_commitment: ArbitrationStepCommitment,
    pub post_commitment: ArbitrationStepCommitment,
}

/// Digest and exact accounting metadata for one committed step boundary.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct StepCommitment {
    pub step_index: u64,
    pub digest: [u8; 32],
    pub encoded_state_bytes: u32,
    pub commitment_fuel: u64,
}

/// Receipt-recorded trace policy. An interval of one records every step.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TracePolicy {
    interval: u64,
    maximum_commitments: u32,
}

impl TracePolicy {
    pub fn new(interval: u64, maximum_commitments: u32) -> Result<Self, CommitmentError> {
        if interval == 0 {
            return Err(CommitmentError::ZeroInterval);
        }
        if maximum_commitments == 0
            || usize::try_from(maximum_commitments).map_or(true, |count| count > MAX_TRACE_COMMITMENTS)
        {
            return Err(CommitmentError::CommitmentLimit {
                limit: MAX_TRACE_COMMITMENTS,
            });
        }
        Ok(Self { interval, maximum_commitments })
    }

    #[must_use]
    pub const fn interval(self) -> u64 { self.interval }

    #[must_use]
    pub const fn maximum_commitments(self) -> u32 { self.maximum_commitments }

    #[must_use]
    pub fn canonical_bytes(self) -> [u8; 12] {
        let mut bytes = [0_u8; 12];
        bytes[..8].copy_from_slice(&self.interval.to_be_bytes());
        bytes[8..].copy_from_slice(&self.maximum_commitments.to_be_bytes());
        bytes
    }
}

/// Canonically ordered commitment chain carried by an ordinary execution receipt.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExecutionTrace {
    policy: TracePolicy,
    commitments: Vec<StepCommitment>,
    steps: Vec<ExecutionStep>,
    total_commitment_fuel: u64,
    total_state_bytes: u64,
    arbitration_commitments: Vec<ArbitrationStepCommitment>,
    arbitration_steps: Vec<ArbitrationExecutionStep>,
    total_arbitration_commitment_fuel: u64,
    total_arbitration_state_bytes: u64,
}

impl ExecutionTrace {
    #[must_use]
    pub const fn new(policy: TracePolicy) -> Self {
        Self {
            policy,
            commitments: Vec::new(),
            steps: Vec::new(),
            total_commitment_fuel: 0,
            total_state_bytes: 0,
            arbitration_commitments: Vec::new(),
            arbitration_steps: Vec::new(),
            total_arbitration_commitment_fuel: 0,
            total_arbitration_state_bytes: 0,
        }
    }

    pub(crate) fn with_exact_capacity(policy: TracePolicy, steps: usize, states: usize) -> Self {
        Self {
            policy,
            commitments: Vec::with_capacity(states),
            steps: Vec::with_capacity(steps),
            total_commitment_fuel: 0,
            total_state_bytes: 0,
            arbitration_commitments: Vec::with_capacity(states),
            arbitration_steps: Vec::with_capacity(steps),
            total_arbitration_commitment_fuel: 0,
            total_arbitration_state_bytes: 0,
        }
    }

    #[must_use]
    pub const fn policy(&self) -> TracePolicy { self.policy }

    #[must_use]
    pub fn commitments(&self) -> &[StepCommitment] { &self.commitments }

    #[must_use]
    pub fn steps(&self) -> &[ExecutionStep] { &self.steps }

    #[must_use]
    pub const fn total_commitment_fuel(&self) -> u64 { self.total_commitment_fuel }

    #[must_use]
    pub const fn total_state_bytes(&self) -> u64 { self.total_state_bytes }

    #[must_use]
    pub const fn total_arbitration_commitment_fuel(&self) -> u64 {
        self.total_arbitration_commitment_fuel
    }

    #[must_use]
    pub const fn total_arbitration_state_bytes(&self) -> u64 {
        self.total_arbitration_state_bytes
    }

    #[must_use]
    pub fn arbitration_commitments(&self) -> &[ArbitrationStepCommitment] {
        &self.arbitration_commitments
    }

    #[must_use]
    pub fn arbitration_steps(&self) -> &[ArbitrationExecutionStep] {
        &self.arbitration_steps
    }

    #[must_use]
    pub const fn is_arbitration_eligible(&self) -> bool {
        !self.arbitration_steps.is_empty()
    }

    pub fn record(&mut self, state: &ExecutionState) -> Result<Option<StepCommitment>, CommitmentError> {
        if state.step_index % self.policy.interval != 0 {
            return Ok(None);
        }
        let commitment = StepCommitment::from_state(state)?;
        self.record_commitment(commitment)?;
        Ok(Some(commitment))
    }

    pub(crate) fn record_commitment(&mut self, commitment: StepCommitment) -> Result<(), CommitmentError> {
        if let Some(previous) = self.commitments.last() {
            if commitment.step_index <= previous.step_index {
                return Err(CommitmentError::NonMonotonicStep {
                    previous: previous.step_index,
                    attempted: commitment.step_index,
                });
            }
        }
        if self.commitments.len() >= self.policy.maximum_commitments as usize {
            return Err(CommitmentError::CommitmentLimit { limit: self.policy.maximum_commitments as usize });
        }
        self.total_commitment_fuel = self.total_commitment_fuel.checked_add(commitment.commitment_fuel)
            .ok_or(CommitmentError::CostOverflow)?;
        self.total_state_bytes = self.total_state_bytes.checked_add(u64::from(commitment.encoded_state_bytes))
            .ok_or(CommitmentError::CostOverflow)?;
        if self.total_state_bytes > MAX_TRACE_STATE_BYTES {
            return Err(CommitmentError::TraceByteLimit {
                attempted: self.total_state_bytes,
                limit: MAX_TRACE_STATE_BYTES,
            });
        }
        self.commitments.push(commitment);
        Ok(())
    }

    pub(crate) fn record_step(&mut self, step: ExecutionStep) -> Result<(), CommitmentError> {
        let expected_post = step.pre_state.step_index.checked_add(1)
            .ok_or(CommitmentError::InvalidStepTransition)?;
        if step.instruction.is_empty() || step.instruction.len() > MAX_STEP_INSTRUCTION_BYTES {
            return Err(CommitmentError::InvalidInstructionEncoding);
        }
        if step.post_state.step_index != expected_post
            || step.pre_commitment.step_index != step.pre_state.step_index
            || step.post_commitment.step_index != step.post_state.step_index
        {
            return Err(CommitmentError::InvalidStepTransition);
        }
        if self.steps.len() >= MAX_TRACE_COMMITMENTS {
            return Err(CommitmentError::CommitmentLimit { limit: MAX_TRACE_COMMITMENTS });
        }
        let retained_bytes = u64::try_from(step.instruction.len())
            .map_err(|_| CommitmentError::CostOverflow)?;
        let total_state_bytes = self.total_state_bytes.checked_add(retained_bytes)
            .ok_or(CommitmentError::CostOverflow)?;
        if total_state_bytes > MAX_TRACE_STATE_BYTES {
            return Err(CommitmentError::TraceByteLimit {
                attempted: total_state_bytes,
                limit: MAX_TRACE_STATE_BYTES,
            });
        }
        self.total_state_bytes = total_state_bytes;
        self.steps.push(step);
        Ok(())
    }

    pub(crate) fn record_arbitration_step(
        &mut self,
        step: ArbitrationExecutionStep,
    ) -> Result<(), CommitmentError> {
        let expected_post = step.pre_state.legacy.step_index.checked_add(1)
            .ok_or(CommitmentError::InvalidStepTransition)?;
        if step.instruction.is_empty() || step.instruction.len() > MAX_STEP_INSTRUCTION_BYTES {
            return Err(CommitmentError::InvalidInstructionEncoding);
        }
        if step.post_state.legacy.step_index != expected_post
            || step.pre_commitment.step_index != step.pre_state.legacy.step_index
            || step.post_commitment.step_index != step.post_state.legacy.step_index
            || !step.pre_commitment.arbitration_eligible()
            || !step.post_commitment.arbitration_eligible()
        {
            return Err(CommitmentError::InvalidStepTransition);
        }
        if self.arbitration_steps.len() >= MAX_TRACE_COMMITMENTS {
            return Err(CommitmentError::CommitmentLimit { limit: MAX_TRACE_COMMITMENTS });
        }
        let mut additions = [None, None];
        let mut addition_count = 0_usize;
        let mut next_fuel = self.total_arbitration_commitment_fuel;
        let mut next_bytes = self.total_arbitration_state_bytes;
        for commitment in [step.pre_commitment, step.post_commitment] {
            if self.arbitration_commitments.last().map_or(
                false,
                |previous| previous.step_index == commitment.step_index,
            ) || additions[..addition_count].iter().flatten().any(
                |previous: &ArbitrationStepCommitment| previous.step_index == commitment.step_index,
            ) {
                continue;
            }
            let previous_index = additions[..addition_count].iter().flatten().last()
                .map(|previous| previous.step_index)
                .or_else(|| self.arbitration_commitments.last().map(|previous| previous.step_index));
            if previous_index.is_some_and(|previous| previous >= commitment.step_index) {
                return Err(CommitmentError::InvalidStepTransition);
            }
            next_fuel = next_fuel
                .checked_add(commitment.commitment_fuel)
                .ok_or(CommitmentError::CostOverflow)?;
            next_bytes = next_bytes
                .checked_add(u64::from(commitment.encoded_state_bytes))
                .ok_or(CommitmentError::CostOverflow)?;
            if next_bytes > MAX_TRACE_STATE_BYTES {
                return Err(CommitmentError::TraceByteLimit {
                    attempted: next_bytes,
                    limit: MAX_TRACE_STATE_BYTES,
                });
            }
            additions[addition_count] = Some(commitment);
            addition_count += 1;
        }
        next_bytes = next_bytes
            .checked_add(
                u64::try_from(step.instruction.len())
                    .map_err(|_| CommitmentError::CostOverflow)?,
            )
            .ok_or(CommitmentError::CostOverflow)?;
        if next_bytes > MAX_TRACE_STATE_BYTES {
            return Err(CommitmentError::TraceByteLimit {
                attempted: next_bytes,
                limit: MAX_TRACE_STATE_BYTES,
            });
        }
        self.total_arbitration_commitment_fuel = next_fuel;
        self.total_arbitration_state_bytes = next_bytes;
        self.arbitration_commitments.extend(additions.into_iter().flatten());
        self.arbitration_steps.push(step);
        Ok(())
    }

    /// Receipt encoding binds both declared policy and the ordered chain.
    pub fn canonical_bytes(&self) -> Result<Vec<u8>, CommitmentError> {
        let mut bytes = Vec::with_capacity(20 + self.commitments.len() * 52);
        bytes.extend_from_slice(&STEP_COMMITMENT_VERSION.to_be_bytes());
        bytes.extend_from_slice(&self.policy.canonical_bytes());
        put_len(&mut bytes, self.commitments.len())?;
        for commitment in &self.commitments {
            bytes.extend_from_slice(&commitment.step_index.to_be_bytes());
            bytes.extend_from_slice(&commitment.digest);
            bytes.extend_from_slice(&commitment.encoded_state_bytes.to_be_bytes());
            bytes.extend_from_slice(&commitment.commitment_fuel.to_be_bytes());
        }
        bytes.extend_from_slice(&self.total_commitment_fuel.to_be_bytes());
        bytes.extend_from_slice(&self.total_state_bytes.to_be_bytes());
        Ok(bytes)
    }


    /// Canonical arbitration evidence. Legacy commitments remain embedded for
    /// receipt compatibility, but eligibility is established only by the
    /// complete v2 chain appended here.
    pub fn canonical_arbitration_bytes(&self) -> Result<Vec<u8>, CommitmentError> {
        if self.arbitration_commitments.is_empty() || self.arbitration_steps.is_empty() {
            return Err(CommitmentError::LegacyCommitmentNotArbitrable);
        }
        let legacy = self.canonical_bytes()?;
        let per_commitment = 2_usize + 8 + 32 + 4 + 8;
        let commitment_bytes = self.arbitration_commitments.len()
            .checked_mul(per_commitment)
            .ok_or(CommitmentError::CostOverflow)?;
        let capacity = 2_usize.checked_add(4).and_then(|bytes| bytes.checked_add(legacy.len()))
            .and_then(|bytes| bytes.checked_add(4))
            .and_then(|bytes| bytes.checked_add(commitment_bytes))
            .and_then(|bytes| bytes.checked_add(16))
            .ok_or(CommitmentError::CostOverflow)?;
        if capacity > MAX_ARBITRATION_STATE_BYTES {
            return Err(CommitmentError::ArbitrationStateTooLarge {
                bytes: capacity,
                limit: MAX_ARBITRATION_STATE_BYTES,
            });
        }
        let mut bytes = Vec::with_capacity(capacity);
        bytes.extend_from_slice(&ARBITRATION_STEP_COMMITMENT_VERSION.to_be_bytes());
        put_bytes(&mut bytes, &legacy)?;
        put_len(&mut bytes, self.arbitration_commitments.len())?;
        for commitment in &self.arbitration_commitments {
            bytes.extend_from_slice(&commitment.version.to_be_bytes());
            bytes.extend_from_slice(&commitment.step_index.to_be_bytes());
            bytes.extend_from_slice(&commitment.digest);
            bytes.extend_from_slice(&commitment.encoded_state_bytes.to_be_bytes());
            bytes.extend_from_slice(&commitment.commitment_fuel.to_be_bytes());
        }
        bytes.extend_from_slice(&self.total_arbitration_commitment_fuel.to_be_bytes());
        bytes.extend_from_slice(&self.total_arbitration_state_bytes.to_be_bytes());
        Ok(bytes)
    }
}

impl StepCommitment {
    pub fn from_state(state: &ExecutionState) -> Result<Self, CommitmentError> {
        let encoded = state.canonical_bytes()?;
        let encoded_state_bytes = u32::try_from(encoded.len())
            .map_err(|_| CommitmentError::StateTooLarge { bytes: encoded.len(), limit: MAX_STEP_STATE_BYTES })?;
        let commitment_fuel = step_commitment_fuel(u64::from(encoded_state_bytes))?;
        let mut hasher = Sha256::new();
        hasher.update(STEP_COMMITMENT_DOMAIN);
        hasher.update(&encoded);
        Ok(Self {
            step_index: state.step_index,
            digest: hasher.finalize().into(),
            encoded_state_bytes,
            commitment_fuel,
        })
    }

    /// Version-one commitments are retained for receipt compatibility only.
    /// They omit transition-determining state and cannot be used by the
    /// single-step arbiter.
    #[must_use]
    pub const fn arbitration_eligible(self) -> bool { false }
}

impl ArbitrationStepCommitment {
    pub fn from_state(state: &ArbitrationExecutionState) -> Result<Self, CommitmentError> {
        let encoded = state.canonical_bytes()?;
        let encoded_state_bytes = u32::try_from(encoded.len()).map_err(|_| {
            CommitmentError::ArbitrationStateTooLarge {
                bytes: encoded.len(),
                limit: MAX_ARBITRATION_STATE_BYTES,
            }
        })?;
        let legacy_state_bytes = u64::try_from(state.legacy.canonical_bytes()?.len())
            .map_err(|_| CommitmentError::CostOverflow)?;
        let engine_state_bytes = u64::try_from(state.engine_state.len())
            .map_err(|_| CommitmentError::CostOverflow)?;
        let commitment_fuel = arbitration_step_commitment_fuel_with_host(
            legacy_state_bytes,
            engine_state_bytes,
            state.host_state_bytes,
        )?;
        if state.host_state_bytes == 0
            && commitment_fuel != step_commitment_fuel(u64::from(encoded_state_bytes))?
        {
            return Err(CommitmentError::CanonicalLengthMismatch {
                measured: usize::try_from(arbitration_step_state_bytes(
                    legacy_state_bytes,
                    engine_state_bytes,
                )?).unwrap_or(usize::MAX),
                encoded: encoded.len(),
            });
        }
        let mut hasher = Sha256::new();
        hasher.update(ARBITRATION_STEP_COMMITMENT_DOMAIN);
        hasher.update(&encoded);
        Ok(Self {
            version: ARBITRATION_STEP_COMMITMENT_VERSION,
            step_index: state.legacy.step_index,
            digest: hasher.finalize().into(),
            encoded_state_bytes,
            commitment_fuel,
        })
    }

    #[must_use]
    pub const fn arbitration_eligible(self) -> bool {
        self.version == ARBITRATION_STEP_COMMITMENT_VERSION
    }
}

pub fn step_commitment_fuel(encoded_state_bytes: u64) -> Result<u64, CommitmentError> {
    STEP_COMMITMENT_BASE_FUEL.checked_add(
        encoded_state_bytes.checked_mul(STEP_COMMITMENT_FUEL_PER_BYTE)
            .ok_or(CommitmentError::CostOverflow)?,
    ).ok_or(CommitmentError::CostOverflow)
}

/// Exact v2 state length shared by observer authorization, commitment hashing
/// and receipt accounting. The fixed portion contains the v2 identity, three
/// length/root fields and the host-state length.
pub fn arbitration_step_state_bytes(
    legacy_state_bytes: u64,
    engine_state_bytes: u64,
) -> Result<u64, CommitmentError> {
    218_u64.checked_add(4).and_then(|bytes| bytes.checked_add(legacy_state_bytes))
        .and_then(|bytes| bytes.checked_add(4))
        .and_then(|bytes| bytes.checked_add(engine_state_bytes))
        .and_then(|bytes| bytes.checked_add(40))
        .ok_or(CommitmentError::CostOverflow)
}

pub fn arbitration_step_commitment_fuel(
    legacy_state_bytes: u64,
    engine_state_bytes: u64,
) -> Result<u64, CommitmentError> {
    arbitration_step_commitment_fuel_with_host(legacy_state_bytes, engine_state_bytes, 0)
}

pub fn arbitration_step_commitment_fuel_with_host(
    legacy_state_bytes: u64,
    engine_state_bytes: u64,
    host_state_bytes: u64,
) -> Result<u64, CommitmentError> {
    let host_work = host_state_bytes.checked_mul(3).ok_or(CommitmentError::CostOverflow)?;
    let work = arbitration_step_state_bytes(
        legacy_state_bytes,
        engine_state_bytes,
    )?.checked_add(host_work).ok_or(CommitmentError::CostOverflow)?;
    step_commitment_fuel(work)
}

impl ExecutionState {
    /// Produces the frozen, endian-independent state encoding.
    pub fn canonical_bytes(&self) -> Result<Vec<u8>, CommitmentError> {
        validate_canonical_globals(&self.globals)?;
        validate_canonical_overlay(&self.storage_overlay)?;
        let mut bytes = Vec::new();
        bytes.extend_from_slice(&STEP_COMMITMENT_VERSION.to_be_bytes());
        bytes.extend_from_slice(&self.module_code_hash);
        bytes.extend_from_slice(&self.input_digest);
        bytes.extend_from_slice(&self.execution_parameters_digest);
        bytes.extend_from_slice(&self.step_index.to_be_bytes());
        bytes.extend_from_slice(&self.program_counter.to_be_bytes());
        put_values(&mut bytes, &self.value_stack)?;
        put_len(&mut bytes, self.call_frames.len())?;
        for frame in &self.call_frames {
            bytes.extend_from_slice(&frame.function_index.to_be_bytes());
            match frame.return_program_counter {
                Some(pc) => { bytes.push(1); bytes.extend_from_slice(&pc.to_be_bytes()); }
                None => bytes.push(0),
            }
            put_values(&mut bytes, &frame.locals)?;
        }
        put_len(&mut bytes, self.control_stack.len())?;
        for frame in &self.control_stack {
            bytes.push(frame.kind);
            bytes.extend_from_slice(&frame.operand_stack_height.to_be_bytes());
            bytes.push(u8::from(frame.unreachable));
        }
        put_bytes(&mut bytes, &self.linear_memory)?;
        put_len(&mut bytes, self.globals.len())?;
        for global in &self.globals {
            bytes.extend_from_slice(&global.global_index.to_be_bytes());
            bytes.push(u8::from(global.mutable));
            put_value(&mut bytes, global.value);
        }
        put_len(&mut bytes, self.storage_overlay.len())?;
        for entry in &self.storage_overlay {
            match entry {
                StorageOverlayEntry::Write { key, value } => {
                    bytes.push(0);
                    put_bytes(&mut bytes, key)?;
                    put_bytes(&mut bytes, value)?;
                }
                StorageOverlayEntry::Delete { key } => {
                    bytes.push(1);
                    put_bytes(&mut bytes, key)?;
                }
            }
        }
        bytes.extend_from_slice(&self.fuel_remaining.to_be_bytes());
        put_usage(&mut bytes, self.metered_usage);
        if bytes.len() > MAX_STEP_STATE_BYTES {
            return Err(CommitmentError::StateTooLarge { bytes: bytes.len(), limit: MAX_STEP_STATE_BYTES });
        }
        Ok(bytes)
    }
}

impl ArbitrationExecutionState {
    /// Produces the separately versioned, endian-independent arbitration
    /// encoding after checking every component and the aggregate before the
    /// output allocation.
    pub fn canonical_bytes(&self) -> Result<Vec<u8>, CommitmentError> {
        if self.legacy.module_code_hash != self.identity.module_code_hash
            || self.legacy.input_digest != self.identity.input_digest
        {
            return Err(CommitmentError::ArbitrationIdentityMismatch);
        }
        if self.engine_state.len() > MAX_ARBITRATION_ENGINE_STATE_BYTES {
            return Err(CommitmentError::ArbitrationComponentTooLarge {
                component: "engine",
                bytes: self.engine_state.len(),
                limit: MAX_ARBITRATION_ENGINE_STATE_BYTES,
            });
        }
        let host_state_bytes = usize::try_from(self.host_state_bytes)
            .map_err(|_| CommitmentError::ArbitrationComponentTooLarge {
                component: "host",
                bytes: usize::MAX,
                limit: MAX_ARBITRATION_HOST_STATE_BYTES,
            })?;
        if host_state_bytes > MAX_ARBITRATION_HOST_STATE_BYTES {
            return Err(CommitmentError::ArbitrationComponentTooLarge {
                component: "host",
                bytes: host_state_bytes,
                limit: MAX_ARBITRATION_HOST_STATE_BYTES,
            });
        }
        let legacy = self.legacy.canonical_bytes()?;
        let identity_bytes = 2_usize
            .checked_add(2).and_then(|bytes| bytes.checked_add(2))
            .and_then(|bytes| bytes.checked_add(4)).and_then(|bytes| bytes.checked_add(4))
            .and_then(|bytes| bytes.checked_add(12)).and_then(|bytes| bytes.checked_add(32 * 6))
            .ok_or(CommitmentError::CostOverflow)?;
        let total = identity_bytes
            .checked_add(4).and_then(|bytes| bytes.checked_add(legacy.len()))
            .and_then(|bytes| bytes.checked_add(4)).and_then(|bytes| bytes.checked_add(self.engine_state.len()))
            .and_then(|bytes| bytes.checked_add(32 + 8))
            .ok_or(CommitmentError::CostOverflow)?;
        let shared_total = arbitration_step_state_bytes(
            u64::try_from(legacy.len()).map_err(|_| CommitmentError::CostOverflow)?,
            u64::try_from(self.engine_state.len()).map_err(|_| CommitmentError::CostOverflow)?,
        )?;
        if u64::try_from(total).map_err(|_| CommitmentError::CostOverflow)? != shared_total {
            return Err(CommitmentError::CanonicalLengthMismatch {
                measured: usize::try_from(shared_total).unwrap_or(usize::MAX),
                encoded: total,
            });
        }
        if total > MAX_ARBITRATION_STATE_BYTES {
            return Err(CommitmentError::ArbitrationStateTooLarge {
                bytes: total,
                limit: MAX_ARBITRATION_STATE_BYTES,
            });
        }
        let mut bytes = Vec::with_capacity(total);
        bytes.extend_from_slice(&ARBITRATION_STEP_COMMITMENT_VERSION.to_be_bytes());
        bytes.extend_from_slice(&self.identity.runtime_version.to_be_bytes());
        bytes.extend_from_slice(&self.identity.abi_version.to_be_bytes());
        bytes.extend_from_slice(&self.identity.fee_schedule_version.to_be_bytes());
        bytes.extend_from_slice(&self.identity.metering_schedule_version.to_be_bytes());
        bytes.extend_from_slice(&self.identity.trace_policy.canonical_bytes());
        bytes.extend_from_slice(&self.identity.module_code_hash);
        bytes.extend_from_slice(&self.identity.input_digest);
        bytes.extend_from_slice(&self.legacy.execution_parameters_digest);
        bytes.extend_from_slice(&self.identity.host_base_state_root);
        bytes.extend_from_slice(&self.identity.receipt_oracle_root);
        bytes.extend_from_slice(&self.identity.balance_oracle_root);
        put_bytes(&mut bytes, &legacy)?;
        put_bytes(&mut bytes, &self.engine_state)?;
        bytes.extend_from_slice(&self.host_state_root);
        bytes.extend_from_slice(&self.host_state_bytes.to_be_bytes());
        if bytes.len() != total {
            return Err(CommitmentError::CanonicalLengthMismatch {
                measured: total,
                encoded: bytes.len(),
            });
        }
        Ok(bytes)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CommitmentError {
    ZeroInterval,
    LengthOutOfRange { bytes: usize },
    StateTooLarge { bytes: usize, limit: usize },
    GlobalsNotCanonical,
    StorageOverlayNotCanonical,
    NonMonotonicStep { previous: u64, attempted: u64 },
    CommitmentLimit { limit: usize },
    CostOverflow,
    InvalidStepTransition,
    InvalidInstructionEncoding,
    TraceByteLimit { attempted: u64, limit: u64 },
    ArbitrationIdentityMismatch,
    ArbitrationComponentTooLarge { component: &'static str, bytes: usize, limit: usize },
    ArbitrationStateTooLarge { bytes: usize, limit: usize },
    CanonicalLengthMismatch { measured: usize, encoded: usize },
    LegacyCommitmentNotArbitrable,
}

impl Display for CommitmentError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::ZeroInterval => formatter.write_str("execution trace interval is zero"),
            Self::LengthOutOfRange { bytes } => write!(formatter, "canonical field length {bytes} exceeds u32"),
            Self::StateTooLarge { bytes, limit } => write!(formatter, "encoded execution state {bytes} exceeds {limit}"),
            Self::GlobalsNotCanonical => formatter.write_str("execution globals are not strictly ordered"),
            Self::StorageOverlayNotCanonical => formatter.write_str("storage overlay keys are not strictly ordered"),
            Self::NonMonotonicStep { previous, attempted } => write!(formatter, "step {attempted} does not follow committed step {previous}"),
            Self::CommitmentLimit { limit } => write!(formatter, "execution trace exceeds commitment limit {limit}"),
            Self::CostOverflow => formatter.write_str("execution commitment cost overflowed"),
            Self::InvalidStepTransition => formatter.write_str("execution step evidence is not a consecutive committed transition"),
            Self::InvalidInstructionEncoding => formatter.write_str("execution step instruction encoding is empty or exceeds its bound"),
            Self::TraceByteLimit { attempted, limit } => write!(formatter, "execution trace state bytes {attempted} exceed {limit}"),
            Self::ArbitrationIdentityMismatch => formatter.write_str("arbitration state identity does not match its frozen v1 state"),
            Self::ArbitrationComponentTooLarge { component, bytes, limit } => write!(formatter, "arbitration {component} state {bytes} exceeds {limit}"),
            Self::ArbitrationStateTooLarge { bytes, limit } => write!(formatter, "encoded arbitration state {bytes} exceeds {limit}"),
            Self::CanonicalLengthMismatch { measured, encoded } => write!(formatter, "canonical state measured {measured} bytes but encoded {encoded}"),
            Self::LegacyCommitmentNotArbitrable => formatter.write_str("version-one execution commitment is not eligible for arbitration"),
        }
    }
}

impl std::error::Error for CommitmentError {}

fn validate_canonical_globals(globals: &[ExecutionGlobal]) -> Result<(), CommitmentError> {
    if globals.windows(2).any(|pair| pair[0].global_index >= pair[1].global_index) {
        return Err(CommitmentError::GlobalsNotCanonical);
    }
    Ok(())
}

fn validate_canonical_overlay(entries: &[StorageOverlayEntry]) -> Result<(), CommitmentError> {
    if entries.windows(2).any(|pair| pair[0].key() >= pair[1].key()) {
        return Err(CommitmentError::StorageOverlayNotCanonical);
    }
    Ok(())
}

fn put_len(output: &mut Vec<u8>, len: usize) -> Result<(), CommitmentError> {
    let len = u32::try_from(len).map_err(|_| CommitmentError::LengthOutOfRange { bytes: len })?;
    output.extend_from_slice(&len.to_be_bytes());
    Ok(())
}

fn put_bytes(output: &mut Vec<u8>, bytes: &[u8]) -> Result<(), CommitmentError> {
    put_len(output, bytes.len())?;
    output.extend_from_slice(bytes);
    Ok(())
}

fn put_values(output: &mut Vec<u8>, values: &[ExecutionValue]) -> Result<(), CommitmentError> {
    put_len(output, values.len())?;
    for value in values { put_value(output, *value); }
    Ok(())
}

fn put_value(output: &mut Vec<u8>, value: ExecutionValue) {
    match value {
        ExecutionValue::I32(value) => { output.push(0); output.extend_from_slice(&value.to_be_bytes()); }
        ExecutionValue::I64(value) => { output.push(1); output.extend_from_slice(&value.to_be_bytes()); }
    }
}

fn put_usage(output: &mut Vec<u8>, usage: MeteredUsage) {
    output.extend_from_slice(&usage.cpu_fuel.to_be_bytes());
    output.extend_from_slice(&usage.memory_bytes.to_be_bytes());
    output.extend_from_slice(&usage.storage_read_bytes.to_be_bytes());
    output.extend_from_slice(&usage.storage_write_bytes.to_be_bytes());
    output.extend_from_slice(&usage.output_values.to_be_bytes());
    output.extend_from_slice(&usage.output_bytes.to_be_bytes());
    output.extend_from_slice(&usage.occupancy_byte_batches.to_be_bytes());
    output.extend_from_slice(&usage.occupancy_fee_units.to_be_bytes());
    output.extend_from_slice(&usage.fee_units.to_be_bytes());
}

#[cfg(test)]
mod tests {
    use super::*;

    fn zero_usage() -> MeteredUsage {
        MeteredUsage {
            cpu_fuel: 9,
            memory_bytes: 65_536,
            storage_read_bytes: 2,
            storage_write_bytes: 3,
            output_values: 1,
            output_bytes: 4,
            occupancy_byte_batches: 5,
            occupancy_fee_units: 6,
            fee_units: 7,
        }
    }

    fn golden_state() -> ExecutionState {
        ExecutionState {
            module_code_hash: [0x11; 32],
            input_digest: [0x22; 32],
            execution_parameters_digest: [0x33; 32],
            step_index: 8,
            program_counter: 13,
            value_stack: vec![ExecutionValue::I32(-1), ExecutionValue::I64(0x0102_0304_0506_0708)],
            call_frames: vec![ExecutionFrame {
                function_index: 2,
                return_program_counter: Some(21),
                locals: vec![ExecutionValue::I64(-2)],
            }],
            control_stack: vec![ExecutionControlFrame {
                kind: 1,
                operand_stack_height: 2,
                unreachable: false,
            }],
            linear_memory: vec![0x00, 0x7f, 0x80, 0xff],
            globals: vec![ExecutionGlobal { global_index: 0, mutable: true, value: ExecutionValue::I32(42) }],
            storage_overlay: vec![
                StorageOverlayEntry::Write { key: b"a".to_vec(), value: b"one".to_vec() },
                StorageOverlayEntry::Delete { key: b"b".to_vec() },
            ],
            fuel_remaining: 999,
            metered_usage: zero_usage(),
        }
    }

    #[test]
    fn golden_state_encoding_and_digest_are_frozen() {
        let state = golden_state();
        let encoded = state.canonical_bytes().expect("golden state encodes");
        assert_eq!(hex(&encoded), "00011111111111111111111111111111111111111111111111111111111111111111222222222222222222222222222222222222222222222222222222222222222233333333333333333333333333333333333333333333333333333333333333330000000000000008000000000000000d0000000200ffffffff01010203040506070800000001000000020100000000000000150000000101fffffffffffffffe0000000101000000020000000004007f80ff000000010000000001000000002a00000002000000000161000000036f6e6501000000016200000000000003e70000000000000009000000000001000000000000000000020000000000000003000000010000000000000004000000000000000000000000000000050000000000000000000000000000000600000000000000000000000000000007");
        let commitment = StepCommitment::from_state(&state).expect("golden state commits");
        assert_eq!(
            commitment.digest,
            [
                0x92, 0x66, 0x21, 0x51, 0x5c, 0xb1, 0x0c, 0xaf,
                0xf1, 0xca, 0x8a, 0x1b, 0x1d, 0x41, 0x8b, 0xb3,
                0x5c, 0xa4, 0x63, 0x8e, 0xd9, 0x3a, 0x3c, 0x57,
                0x3c, 0x7e, 0x30, 0x16, 0xda, 0x8b, 0xed, 0x74,
            ]
        );
        assert_eq!(commitment.encoded_state_bytes as usize, encoded.len());
        assert_eq!(commitment.commitment_fuel, STEP_COMMITMENT_BASE_FUEL + encoded.len() as u64);
    }

    #[test]
    fn trace_records_only_declared_intervals_in_order() {
        let policy = TracePolicy::new(4, 3).expect("valid trace policy");
        let mut trace = ExecutionTrace::new(policy);
        let mut state = golden_state();
        state.step_index = 3;
        assert_eq!(trace.record(&state).expect("unaligned step is ignored"), None);
        state.step_index = 4;
        assert!(trace.record(&state).expect("first commitment").is_some());
        state.step_index = 8;
        assert!(trace.record(&state).expect("second commitment").is_some());
        assert_eq!(trace.commitments().len(), 2);
        assert_eq!(trace.policy().canonical_bytes(), [0, 0, 0, 0, 0, 0, 0, 4, 0, 0, 0, 3]);
    }

    #[test]
    fn complete_trace_evidence_bytes_are_platform_independent() {
        let policy = TracePolicy::new(4, 3).expect("valid trace policy");
        let mut trace = ExecutionTrace::new(policy);
        trace.record(&golden_state()).expect("golden commitment");
        assert_eq!(
            hex(&trace.canonical_bytes().expect("golden trace encodes")),
            include_str!("../vectors/step-commitment-trace-v1.hex").trim(),
        );
    }

    #[test]
    fn legacy_commitment_is_not_an_arbitration_pre_state() {
        let commitment = StepCommitment::from_state(&golden_state())
            .expect("legacy golden state commits");
        assert!(!commitment.arbitration_eligible());
        let trace = ExecutionTrace::new(TracePolicy::new(1, 2).expect("valid policy"));
        assert_eq!(
            trace.canonical_arbitration_bytes(),
            Err(CommitmentError::LegacyCommitmentNotArbitrable),
        );
    }

    #[test]
    fn arbitration_commitment_separates_engine_host_and_base_state() {
        let policy = TracePolicy::new(1, 2).expect("valid policy");
        let identity = ArbitrationExecutionIdentity {
            module_code_hash: [0x11; 32],
            input_digest: [0x22; 32],
            runtime_version: 1,
            abi_version: 2,
            fee_schedule_version: 3,
            metering_schedule_version: 4,
            trace_policy: policy,
            host_base_state_root: [0x44; 32],
            receipt_oracle_root: [0x55; 32],
            balance_oracle_root: [0x66; 32],
        };
        let state = ArbitrationExecutionState {
            identity,
            legacy: Arc::new(golden_state()),
            engine_state: vec![0x01, 0x02],
            host_state_root: [0x77; 32],
            host_state_bytes: 9,
        };
        let original = ArbitrationStepCommitment::from_state(&state)
            .expect("complete state commits");
        assert!(original.arbitration_eligible());
        for changed in [
            ArbitrationExecutionState { engine_state: vec![0x01, 0x03], ..state.clone() },
            ArbitrationExecutionState { host_state_root: [0x78; 32], ..state.clone() },
            ArbitrationExecutionState {
                identity: ArbitrationExecutionIdentity {
                    host_base_state_root: [0x45; 32],
                    ..identity
                },
                ..state.clone()
            },
        ] {
            assert_ne!(
                original.digest,
                ArbitrationStepCommitment::from_state(&changed)
                    .expect("distinct complete state commits")
                    .digest,
            );
        }
    }

    fn hex(bytes: &[u8]) -> String {
        const DIGITS: &[u8; 16] = b"0123456789abcdef";
        let mut output = String::with_capacity(bytes.len() * 2);
        for byte in bytes {
            output.push(char::from(DIGITS[usize::from(byte >> 4)]));
            output.push(char::from(DIGITS[usize::from(byte & 0x0f)]));
        }
        output
    }
}
