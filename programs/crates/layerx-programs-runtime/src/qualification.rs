//! Determinism, replay and bounded fuzz qualification entry points.

use core::fmt::{self, Display};

use crate::{
    ExecutionError, Executor, FeeSchedule, ResourceBudget, ValidationLimits, ValidationRefusal,
    WasmEngine, WasmValue,
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
    pub wasm: &'a [u8],
    pub export: &'a str,
    pub args: &'a [WasmValue],
}

/// Typed replay refusal. Unknown versions are preserved but never executed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ReplayRefusal {
    UnknownRuntimeVersion { version: u16 },
    UnknownAbiVersion { version: u16 },
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
}

impl ExecutorRevision {
    const fn v1() -> Self {
        Self {
            runtime_version: crate::RUNTIME_VERSION,
            abi_version: crate::ABI_VERSION,
            budget: ResourceBudget::declared(),
            prices: FeeSchedule::declared(),
        }
    }

    #[cfg(test)]
    const fn test_v2() -> Self {
        Self {
            runtime_version: crate::RUNTIME_VERSION + 1,
            abi_version: crate::ABI_VERSION,
            budget: ResourceBudget::declared(),
            prices: FeeSchedule::new(2, 1, 2, 4, 1),
        }
    }

    fn replay(self, record: &RecordedExecution<'_>) -> Result<Vec<u8>, ReplayRefusal> {
        let engine =
            WasmEngine::declared().map_err(|error| ReplayRefusal::Engine(error.to_string()))?;
        let module = engine
            .validate(record.wasm)
            .map_err(ReplayRefusal::Validation)?;
        let mut result = Executor::new(self.budget, self.prices)
            .execute(&module, record.export, record.args)
            .map_err(ReplayRefusal::Execution)?;
        result.runtime_version = self.runtime_version;
        result.abi_version = self.abi_version;
        Ok(result.canonical_evidence())
    }
}

const DECLARED_REVISIONS: [ExecutorRevision; 1] = [ExecutorRevision::v1()];

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

/// Executes the same immutable input using two separately constructed engines
/// and executors, rejecting any byte-level evidence divergence.
///
/// # Errors
///
/// Returns both evidence byte strings if the independent executions diverge.
pub fn programs_differential_gate(
    wasm: &[u8],
    export: &str,
    args: &[WasmValue],
) -> Result<Vec<u8>, DifferentialMismatch> {
    let run = || -> Result<Vec<u8>, Vec<u8>> {
        let engine = WasmEngine::declared().map_err(|error| error.to_string().into_bytes())?;
        let module = engine
            .validate(wasm)
            .map_err(|error| error.to_string().into_bytes())?;
        Executor::declared()
            .execute(&module, export, args)
            .map(|record| record.canonical_evidence())
            .map_err(|error| error.to_string().into_bytes())
    };
    let first = run().unwrap_or_else(|evidence| evidence);
    let second = run().unwrap_or_else(|evidence| evidence);
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
            abi_version: ABI_VERSION,
            wasm: &wasm,
            export: "add",
            args: &[WasmValue::I32(20), WasmValue::I32(22)],
        };
        let before = replay_with_revisions(&v1_record, &[ExecutorRevision::v1()]);
        let upgraded = [ExecutorRevision::v1(), ExecutorRevision::test_v2()];
        let after = replay_with_revisions(&v1_record, &upgraded);
        assert_eq!(before, after);

        let v2_record = RecordedExecution {
            runtime_version: RUNTIME_VERSION + 1,
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
        let revisions = [ExecutorRevision::v1(), ExecutorRevision::test_v2()];
        let unknown_runtime = RecordedExecution {
            runtime_version: RUNTIME_VERSION + 2,
            abi_version: ABI_VERSION,
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
        assert_ne!(ExecutorRevision::v1(), ExecutorRevision::test_v2());
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
