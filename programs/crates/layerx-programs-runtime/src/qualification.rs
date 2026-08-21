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

/// Runs one bounded fuzz input through a named real runtime surface.
///
/// Invalid input is an expected typed refusal. A panic, hang, or allocation
/// beyond the declared module bound is therefore visible to the fuzz runner.
pub fn programs_fuzz_targets(target: FuzzTarget, input: &[u8]) {
    if input.len() > MAX_FUZZ_INPUT_BYTES {
        return;
    }
    let Ok(engine) = WasmEngine::new(ValidationLimits::declared()) else {
        return;
    };
    let validated = engine.validate(input);
    if target == FuzzTarget::Validation {
        return;
    }
    let Ok(module) = validated else {
        return;
    };
    if target == FuzzTarget::Instantiation {
        let _ = module.instantiate_for_qualification();
        return;
    }
    let _ = Executor::declared().execute(&module, "add", &[WasmValue::I32(19), WasmValue::I32(23)]);
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
}
