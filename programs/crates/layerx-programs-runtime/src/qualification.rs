//! Determinism, replay and bounded fuzz qualification entry points.

use core::fmt::{self, Display};

use crate::{
    ExecutionError, ExecutionFault, ExecutionRecord, Executor, FeeSchedule, Meter, ResourceBudget,
    ValidationLimits, ValidationRefusal, WasmEngine, WasmValue,
};

/// Maximum input accepted by the in-process fuzz targets.
pub const MAX_FUZZ_INPUT_BYTES: usize = 1_048_576;

/// One independently selected fuzz surface.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FuzzTarget {
    /// Parser and deterministic-subset validation.
    Validation,
    /// Validated-module instantiation.
    Instantiation,
    /// Metered invocation of an exported function.
    Execution,
}

/// A replayable execution pinned to its consensus runtime and ABI versions.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RecordedExecution<'a> {
    pub runtime_version: u16,
    pub abi_version: u16,
    pub fee_schedule_version: u32,
    pub metering_schedule_version: u32,
    pub wasm: &'a [u8],
    pub export: &'a str,
    pub args: &'a [WasmValue],
}

/// Typed replay refusal. Unknown versions are preserved but never executed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ReplayRefusal {
    UnknownRuntimeVersion { version: u16 },
    UnknownAbiVersion { version: u16 },
    UnknownFeeScheduleVersion { version: u32 },
    UnknownMeteringScheduleVersion { version: u32 },
    MeteringPlanMismatch { recorded: u32, artifact: u32 },
    Engine(String),
    Validation(ValidationRefusal),
    Execution(ExecutionError),
}

impl Display for ReplayRefusal {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::UnknownRuntimeVersion { version } => {
                write!(f, "unknown runtime version {version}")
            }
            Self::UnknownAbiVersion { version } => write!(f, "unknown ABI version {version}"),
            Self::UnknownFeeScheduleVersion { version } => {
                write!(f, "unknown fee schedule version {version}")
            }
            Self::UnknownMeteringScheduleVersion { version } => {
                write!(f, "unknown metering schedule version {version}")
            }
            Self::MeteringPlanMismatch { recorded, artifact } => write!(
                f, "recorded metering schedule {recorded} differs from artifact {artifact}"
            ),
            Self::Engine(reason) => write!(f, "engine refusal: {reason}"),
            Self::Validation(reason) => write!(f, "validation refusal: {reason}"),
            Self::Execution(reason) => write!(f, "execution refusal: {reason}"),
        }
    }
}

impl std::error::Error for ReplayRefusal {}

/// Evidence mismatch between two independently constructed executions.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DifferentialMismatch {
    pub first: Vec<u8>,
    pub second: Vec<u8>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct ExecutorRevision {
    runtime_version: u16,
    abi_version: u16,
    budget: ResourceBudget,
    prices: FeeSchedule,
    metering_schedule: crate::FuelSchedule,
}

impl ExecutorRevision {
    const fn v1() -> Self {
        Self {
            runtime_version: crate::RUNTIME_VERSION,
            abi_version: crate::ABI_V1_VERSION,
            budget: ResourceBudget::declared(),
            prices: FeeSchedule::declared(),
            metering_schedule: crate::FuelSchedule::WASMI_0_31_2,
        }
    }

    const fn v2() -> Self {
        Self {
            runtime_version: crate::RUNTIME_VERSION,
            abi_version: crate::ABI_V2_VERSION,
            budget: ResourceBudget::declared(),
            prices: FeeSchedule::declared(),
            metering_schedule: crate::FuelSchedule::WASMI_0_31_2,
        }
    }

    fn replay(self, record: &RecordedExecution<'_>) -> Result<Vec<u8>, ReplayRefusal> {
        let engine =
            WasmEngine::declared().map_err(|error| ReplayRefusal::Engine(error.to_string()))?;
        if self.metering_schedule.version() != record.metering_schedule_version {
            return Err(ReplayRefusal::UnknownMeteringScheduleVersion {
                version: record.metering_schedule_version,
            });
        }
        let module = engine
            .validate_versioned_metered(record.abi_version, record.wasm, self.metering_schedule)
            .map_err(ReplayRefusal::Validation)?;
        if module.metering_schedule_version() != record.metering_schedule_version {
            return Err(ReplayRefusal::MeteringPlanMismatch {
                recorded: record.metering_schedule_version,
                artifact: module.metering_schedule_version(),
            });
        }
        let mut result = Executor::new_versioned(
            self.budget,
            self.prices,
            self.runtime_version,
            self.abi_version,
        )
            .execute(&module, record.export, record.args)
            .map_err(ReplayRefusal::Execution)?;
        result.runtime_version = self.runtime_version;
        result.abi_version = self.abi_version;
        Ok(result.canonical_evidence())
    }
}

const DECLARED_REVISIONS: [ExecutorRevision; 2] = [ExecutorRevision::v1(), ExecutorRevision::v2()];

fn replay_with_revisions(
    record: &RecordedExecution<'_>,
    revisions: &[ExecutorRevision],
) -> Result<Vec<u8>, ReplayRefusal> {
    if !revisions
        .iter()
        .any(|revision| revision.runtime_version == record.runtime_version)
    {
        return Err(ReplayRefusal::UnknownRuntimeVersion {
            version: record.runtime_version,
        });
    }
    let Some(revision) = revisions.iter().copied().find(|revision| {
        revision.runtime_version == record.runtime_version
            && revision.abi_version == record.abi_version
    }) else {
        return Err(ReplayRefusal::UnknownAbiVersion {
            version: record.abi_version,
        });
    };
    if revision.prices.version() != record.fee_schedule_version {
        return Err(ReplayRefusal::UnknownFeeScheduleVersion {
            version: record.fee_schedule_version,
        });
    }
    revision.replay(record)
}

/// Observation tag bytes prefixing a canonical fuzz observation. The tag makes
/// two structurally different outcomes (a refusal and a success that happen to
/// share a suffix) impossible to confuse when the fuzz runner compares runs.
const OBSERVE_INPUT_TOO_LARGE: u8 = 0x00;
const OBSERVE_ENGINE_REFUSED: u8 = 0x01;
const OBSERVE_VALIDATED: u8 = 0x10;
const OBSERVE_VALIDATION_REFUSED: u8 = 0x11;
const OBSERVE_INSTANTIATED: u8 = 0x20;
const OBSERVE_INSTANTIATION_FAULT: u8 = 0x21;
const OBSERVE_EXECUTED: u8 = 0x30;
const OBSERVE_EXECUTION_REFUSED: u8 = 0x31;

/// Runs one bounded fuzz input through a named real runtime surface, returning a
/// canonical, comparable observation of the outcome.
///
/// The observation is byte-stable for a given input and build: two calls with
/// the same input MUST return identical bytes. The fuzz runner exploits this to
/// treat any divergence as non-determinism - and therefore a build-breaking
/// defect - alongside the panic, hang and unbounded-allocation faults the
/// runner already observes at the process boundary. Invalid input is an
/// expected typed refusal encoded into the observation, never a fault.
#[must_use]
pub fn programs_fuzz_observation(target: FuzzTarget, input: &[u8]) -> Vec<u8> {
    let mut observation = Vec::new();
    if input.len() > MAX_FUZZ_INPUT_BYTES {
        observation.push(OBSERVE_INPUT_TOO_LARGE);
        return observation;
    }
    let engine = match WasmEngine::new(ValidationLimits::declared()) {
        Ok(engine) => engine,
        Err(refusal) => {
            observation.push(OBSERVE_ENGINE_REFUSED);
            observation.extend_from_slice(refusal.to_string().as_bytes());
            return observation;
        }
    };
    let validated = engine.validate(input);
    if target == FuzzTarget::Validation {
        match validated {
            Ok(module) => {
                observation.push(OBSERVE_VALIDATED);
                observation.extend_from_slice(&module.byte_size().to_le_bytes());
                observation.extend_from_slice(&module.function_count().to_le_bytes());
            }
            Err(refusal) => {
                observation.push(OBSERVE_VALIDATION_REFUSED);
                observation.extend_from_slice(refusal.to_string().as_bytes());
            }
        }
        return observation;
    }
    let module = match validated {
        Ok(module) => module,
        Err(refusal) => {
            observation.push(OBSERVE_VALIDATION_REFUSED);
            observation.extend_from_slice(refusal.to_string().as_bytes());
            return observation;
        }
    };
    if target == FuzzTarget::Instantiation {
        match module.instantiate_for_qualification() {
            Ok(()) => observation.push(OBSERVE_INSTANTIATED),
            Err(fault) => {
                observation.push(OBSERVE_INSTANTIATION_FAULT);
                observation.extend_from_slice(fault.to_string().as_bytes());
            }
        }
        return observation;
    }
    match Executor::declared().execute(&module, "add", &[WasmValue::I32(19), WasmValue::I32(23)]) {
        Ok(record) => {
            observation.push(OBSERVE_EXECUTED);
            observation.extend_from_slice(&record.canonical_evidence());
        }
        Err(error) => {
            observation.push(OBSERVE_EXECUTION_REFUSED);
            observation.extend_from_slice(error.to_string().as_bytes());
        }
    }
    observation
}

/// Runs one bounded fuzz input through a named real runtime surface.
///
/// Invalid input is an expected typed refusal. A panic, hang, or allocation
/// beyond the declared module bound is therefore visible to the fuzz runner.
pub fn programs_fuzz_targets(target: FuzzTarget, input: &[u8]) {
    let _ = programs_fuzz_observation(target, input);
}

fn differential_observation(
    module: &crate::ValidatedModule,
    export: &str,
    args: &[WasmValue],
) -> Vec<u8> {
    let mut instance = match module.instantiate_metered_retained_for_qualification(Meter::declared()) {
        Ok(instance) => instance,
        Err(failure) => {
            let (fault, state) = *failure;
            return refusal_observation(
                &fault,
                state.meter(),
            );
        }
    };
    let outputs = match instance.call(export, args) {
        Ok(outputs) => outputs,
        Err(fault) => {
            return refusal_observation(
                &fault,
                instance.meter(),
            );
        }
    };
    let abi_version = match module.abi_revision() {
        crate::validate::AbiRevision::V1 => crate::ABI_V1_VERSION,
        crate::validate::AbiRevision::V2 => crate::ABI_V2_VERSION,
    };
    success_observation(abi_version, module.metering_schedule_version(), outputs, instance.meter())
}

fn legacy_reference_observation(
    abi_version: u16,
    wasm: &[u8],
    export: &str,
    args: &[WasmValue],
) -> Vec<u8> {
    let limits = ValidationLimits::declared();
    let initial_height = 1_024usize.min(limits.max_value_stack_height() as usize);
    let stack_limits = match wasmi::StackLimits::new(
        initial_height,
        limits.max_value_stack_height() as usize,
        limits.max_call_depth() as usize,
    ) {
        Ok(limits) => limits,
        Err(error) => return validation_observation(&error.to_string()),
    };
    let mut config = wasmi::Config::default();
    config
        .set_stack_limits(stack_limits)
        .wasm_mutable_global(true)
        .wasm_sign_extension(true)
        .wasm_multi_value(true)
        .wasm_bulk_memory(true)
        .wasm_saturating_float_to_int(false)
        .wasm_reference_types(false)
        .wasm_tail_call(false)
        .wasm_extended_const(false)
        .consume_fuel(true)
        .floats(false);
    let engine = wasmi::Engine::new(&config);
    let revision = match abi_version {
        crate::ABI_V1_VERSION => crate::validate::AbiRevision::V1,
        crate::ABI_V2_VERSION => crate::validate::AbiRevision::V2,
        _ => return validation_observation(&format!("unsupported ABI version {abi_version}")),
    };
    let module = match crate::validate::validate_original_for_qualification(
        &engine,
        limits,
        wasm,
        revision,
    ) {
        Ok(module) => module,
        Err(error) => return validation_observation(&error.to_string()),
    };
    let linker = match crate::host::linker(&engine) {
        Ok(linker) => linker,
        Err(fault) => return engine_observation(&fault.to_string()),
    };
    let mut state = crate::host::RuntimeState::isolated_legacy_reference(Meter::declared());
    state.bind_metering_schedule(crate::FuelSchedule::WASMI_0_31_2);
    let initial_fuel = state.meter().cpu_remaining();
    let mut store = wasmi::Store::new(&engine, state);
    store.limiter(|state| state.meter_mut() as &mut dyn wasmi::ResourceLimiter);
    if let Err(error) = store.add_fuel(initial_fuel) {
        return engine_observation(&error.to_string());
    }
    let pre = match linker.instantiate(&mut store, &module) {
        Ok(pre) => pre,
        Err(error) => {
            let fault = crate::execute::fault_from_error(&error);
            let commit = commit_reference_store(&mut store);
            if fault == ExecutionFault::OutOfFuel {
                store.data_mut().meter_mut().mark_cpu_exhausted();
            }
            if let Err(commit_fault) = commit {
                return refusal_observation(&commit_fault, store.data().meter());
            }
            return refusal_observation(
                &fault,
                store.data().meter(),
            );
        }
    };
    let instance = match pre.start(&mut store) {
        Ok(instance) => instance,
        Err(error) => {
            let fault = crate::execute::fault_from_error(&error);
            let commit = commit_reference_store(&mut store);
            if fault == ExecutionFault::OutOfFuel {
                store.data_mut().meter_mut().mark_cpu_exhausted();
            }
            if let Err(commit_fault) = commit {
                return refusal_observation(&commit_fault, store.data().meter());
            }
            return refusal_observation(
                &fault,
                store.data().meter(),
            );
        }
    };
    let mut instance = crate::ProgramInstance::new(store, instance);
    let outcome = instance.call(export, args);
    let fuel_commit = instance.commit_reference_fuel();
    match (outcome, fuel_commit) {
        (Ok(outputs), Ok(_)) => success_observation(
            abi_version,
            crate::meter::inject::GENESIS_METERING_SCHEDULE_VERSION,
            outputs,
            instance.meter(),
        ),
        (Err(fault), Ok(_)) | (Ok(_), Err(fault)) => refusal_observation(
            &fault,
            instance.meter(),
        ),
        (Err(outcome_fault), Err(commit_fault)) => {
            let fault = if outcome_fault == ExecutionFault::OutOfFuel {
                outcome_fault
            } else {
                commit_fault
            };
            refusal_observation(
                &fault,
                instance.meter(),
            )
        }
    }
}

fn success_observation(
    abi_version: u16,
    metering_schedule_version: u32,
    outputs: Vec<WasmValue>,
    meter: &Meter,
) -> Vec<u8> {
    match meter.finish() {
        Ok(usage) => {
            let record = ExecutionRecord {
                runtime_version: crate::RUNTIME_VERSION,
                abi_version,
                metering_schedule_version,
                outputs,
                usage,
            };
            let mut observation = vec![OBSERVE_EXECUTED];
            observation.extend_from_slice(&record.canonical_evidence());
            append_metered_usage(&mut observation, &usage);
            observation
        }
        Err(refusal) => refusal_observation(&ExecutionFault::Resource { refusal }, meter),
    }
}

fn refusal_observation(fault: &ExecutionFault, meter: &Meter) -> Vec<u8> {
    let mut observation = vec![OBSERVE_EXECUTION_REFUSED];
    let fault = fault.to_string();
    observation.extend_from_slice(&(fault.len() as u64).to_be_bytes());
    observation.extend_from_slice(fault.as_bytes());
    observation.extend_from_slice(&meter.cpu_total().to_be_bytes());
    if let Some(exhaustion) = meter.exhaustion() {
        observation.push(1);
        let exhaustion = exhaustion.to_string();
        observation.extend_from_slice(&(exhaustion.len() as u64).to_be_bytes());
        observation.extend_from_slice(exhaustion.as_bytes());
    } else {
        observation.push(0);
    }
    let raw = meter.qualification_snapshot();
    observation.extend_from_slice(&raw.cpu_fuel.to_be_bytes());
    observation.extend_from_slice(&raw.memory_bytes.to_be_bytes());
    observation.extend_from_slice(&raw.storage_read_bytes.to_be_bytes());
    observation.extend_from_slice(&raw.storage_write_bytes.to_be_bytes());
    observation.extend_from_slice(&raw.output_values.to_be_bytes());
    observation.extend_from_slice(&raw.output_bytes.to_be_bytes());
    match meter.finish_resource_failure() {
        Ok(usage) => {
            observation.push(1);
            append_metered_usage(&mut observation, &usage);
        }
        Err(refusal) => {
            observation.push(0);
            let refusal = refusal.to_string();
            observation.extend_from_slice(&(refusal.len() as u64).to_be_bytes());
            observation.extend_from_slice(refusal.as_bytes());
        }
    }
    observation
}

fn append_metered_usage(observation: &mut Vec<u8>, usage: &crate::MeteredUsage) {
    observation.extend_from_slice(&usage.cpu_fuel.to_be_bytes());
    observation.extend_from_slice(&usage.memory_bytes.to_be_bytes());
    observation.extend_from_slice(&usage.storage_read_bytes.to_be_bytes());
    observation.extend_from_slice(&usage.storage_write_bytes.to_be_bytes());
    observation.extend_from_slice(&usage.output_values.to_be_bytes());
    observation.extend_from_slice(&usage.output_bytes.to_be_bytes());
    observation.extend_from_slice(&usage.occupancy_byte_batches.to_be_bytes());
    observation.extend_from_slice(&usage.occupancy_fee_units.to_be_bytes());
    observation.extend_from_slice(&usage.fee_units.to_be_bytes());
}

fn engine_observation(reason: &str) -> Vec<u8> {
    let mut observation = vec![OBSERVE_ENGINE_REFUSED];
    observation.extend_from_slice(reason.as_bytes());
    observation
}

fn validation_observation(reason: &str) -> Vec<u8> {
    let mut observation = vec![OBSERVE_VALIDATION_REFUSED];
    observation.extend_from_slice(reason.as_bytes());
    observation
}

fn commit_reference_store(store: &mut wasmi::Store<crate::host::RuntimeState>) -> Result<(), ExecutionFault> {
    let consumed = store.fuel_consumed().ok_or_else(|| ExecutionFault::EngineFault {
        reason: "legacy reference engine fuel is disabled".to_string(),
    })?;
    let committed = store.data().legacy_reference_engine_committed();
    let guest = consumed.checked_sub(committed).ok_or_else(|| ExecutionFault::EngineFault {
        reason: "legacy reference host fuel exceeded engine fuel".to_string(),
    })?;
    store.data_mut().meter_mut().charge_cpu(guest).map_err(|refusal| ExecutionFault::Resource { refusal })?;
    store.data_mut().set_legacy_reference_engine_committed(consumed);
    Ok(())
}

/// Executes the same immutable input with the historical Wasmi 0.31.2
/// internal-fuel interpreter over the original bytes and with the production
/// injected/private-hook path, rejecting output, refusal, or usage divergence.
///
/// # Errors
///
/// Returns both evidence byte strings if the independent executions diverge.
pub fn programs_differential_gate(
    wasm: &[u8],
    export: &str,
    args: &[WasmValue],
) -> Result<Vec<u8>, DifferentialMismatch> {
    programs_differential_gate_versioned(crate::ABI_V1_VERSION, wasm, export, args)
}

pub fn programs_differential_gate_versioned(
    abi_version: u16,
    wasm: &[u8],
    export: &str,
    args: &[WasmValue],
) -> Result<Vec<u8>, DifferentialMismatch> {
    let first = legacy_reference_observation(abi_version, wasm, export, args);
    let engine = match WasmEngine::declared() {
        Ok(engine) => engine,
        Err(error) => {
            let second = engine_observation(&error.to_string());
            return Err(DifferentialMismatch { first, second });
        }
    };
    let module = match engine.validate_versioned(abi_version, wasm) {
        Ok(module) => module,
        Err(error) => {
            let second = validation_observation(&error.to_string());
            return if first == second { Ok(second) } else { Err(DifferentialMismatch { first, second }) };
        }
    };
    let second = differential_observation(&module, export, args);
    if first == second {
        Ok(first)
    } else {
        Err(DifferentialMismatch { first, second })
    }
}

/// Replays only under the recorded version, never the newest implementation.
///
/// # Errors
///
/// Returns a typed refusal for unknown versions, invalid modules, or failed
/// execution.
pub fn replay_recorded_execution(record: &RecordedExecution<'_>) -> Result<Vec<u8>, ReplayRefusal> {
    replay_with_revisions(record, &DECLARED_REVISIONS)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::add_module;
    use crate::{FeeSchedule, ResourceBudget, ABI_VERSION, RUNTIME_VERSION};

    #[test]
    fn dispatcher_keeps_v1_replay_stable_after_a_real_v2_revision_is_present() {
        let wasm = add_module();
        let v1_record = RecordedExecution {
            runtime_version: RUNTIME_VERSION,
            abi_version: crate::ABI_V1_VERSION,
            fee_schedule_version: FeeSchedule::declared().version(),
            metering_schedule_version: crate::meter::inject::GENESIS_METERING_SCHEDULE_VERSION,
            wasm: &wasm,
            export: "add",
            args: &[WasmValue::I32(20), WasmValue::I32(22)],
        };
        let before = replay_with_revisions(&v1_record, &[ExecutorRevision::v1()]);
        let upgraded = [ExecutorRevision::v1(), ExecutorRevision::v2()];
        let after = replay_with_revisions(&v1_record, &upgraded);
        assert_eq!(before, after);

        let v2_record = RecordedExecution {
            runtime_version: RUNTIME_VERSION,
            abi_version: ABI_VERSION,
            ..v1_record
        };
        let v2 = match replay_with_revisions(&v2_record, &upgraded) {
            Ok(evidence) => evidence,
            Err(refusal) => panic!("real v2 replay refused: {refusal}"),
        };
        let v1 = match after {
            Ok(evidence) => evidence,
            Err(refusal) => panic!("v1 replay refused: {refusal}"),
        };
        assert_ne!(v1, v2);
    }

    #[test]
    fn dispatcher_refuses_unknown_runtime_and_unsupported_abi() {
        let wasm = add_module();
        let revisions = [ExecutorRevision::v1(), ExecutorRevision::v2()];
        let unknown_runtime = RecordedExecution {
            runtime_version: RUNTIME_VERSION + 2,
            abi_version: ABI_VERSION,
            fee_schedule_version: FeeSchedule::declared().version(),
            metering_schedule_version: crate::meter::inject::GENESIS_METERING_SCHEDULE_VERSION,
            wasm: &wasm,
            export: "add",
            args: &[],
        };
        assert_eq!(
            replay_with_revisions(&unknown_runtime, &revisions),
            Err(ReplayRefusal::UnknownRuntimeVersion {
                version: RUNTIME_VERSION + 2,
            })
        );
        let unsupported_abi = RecordedExecution {
            runtime_version: RUNTIME_VERSION,
            abi_version: ABI_VERSION + 1,
            ..unknown_runtime
        };
        assert_eq!(
            replay_with_revisions(&unsupported_abi, &revisions),
            Err(ReplayRefusal::UnknownAbiVersion {
                version: ABI_VERSION + 1,
            })
        );
    }

    #[test]
    fn v2_is_a_distinct_executor_configuration() {
        assert_ne!(ExecutorRevision::v1(), ExecutorRevision::v2());
        assert_eq!(ExecutorRevision::v1().budget, ResourceBudget::declared());
        assert_eq!(ExecutorRevision::v1().prices, FeeSchedule::declared());
    }

    #[test]
    fn fuzz_observation_is_stable_across_every_surface_and_input() {
        let wasm = add_module();
        let malformed = [0x00, 0x61, 0x73, 0x6d, 0x01];
        let inputs: [&[u8]; 3] = [&wasm, &malformed, &[]];
        for target in [
            FuzzTarget::Validation,
            FuzzTarget::Instantiation,
            FuzzTarget::Execution,
        ] {
            for input in inputs {
                let first = programs_fuzz_observation(target, input);
                let second = programs_fuzz_observation(target, input);
                assert_eq!(first, second, "fuzz observation diverged for {target:?}");
                assert!(!first.is_empty(), "fuzz observation was empty for {target:?}");
            }
        }
    }

    #[test]
    fn fuzz_observation_records_a_typed_validation_refusal_without_faulting() {
        let observation = programs_fuzz_observation(FuzzTarget::Validation, &[0x00, 0x61, 0x73]);
        assert_eq!(observation.first().copied(), Some(super::OBSERVE_VALIDATION_REFUSED));
    }

    #[test]
    fn fuzz_observation_executes_the_valid_add_surface() {
        let observation = programs_fuzz_observation(FuzzTarget::Execution, &add_module());
        assert_eq!(observation.first().copied(), Some(super::OBSERVE_EXECUTED));
    }
}
