//! Canonical byte entry protocol shared by activity and nested call boundaries.

use core::fmt::{self, Display};

use crate::abi::MAX_CALL_INPUT_BYTES;
use crate::calls::{CALL_INPUT_FUEL_PER_BYTE, CALL_RESERVE_EXPORT};
use crate::execute::{ExecutionFault, ProgramInstance, WasmValue};
use crate::meter::MeterRefusal;

/// Typed refusal produced while entering a program with calldata.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum EntrypointRefusal {
    InputTooLarge { bytes: usize, limit: usize },
    MissingAllocator,
    MissingMemory,
    MissingEntry,
    AllocationRefused { code: i32 },
    GuestRefused { code: i32 },
    Fault(ExecutionFault),
    Resource(MeterRefusal),
}

impl Display for EntrypointRefusal {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InputTooLarge { bytes, limit } => {
                write!(f, "calldata size {bytes} exceeds limit {limit}")
            }
            Self::MissingAllocator => f.write_str("program exports no input reservation"),
            Self::MissingMemory => f.write_str("program exports no linear memory"),
            Self::MissingEntry => f.write_str("program exports no requested entry point"),
            Self::AllocationRefused { code } => {
                write!(f, "program refused input reservation with {code}")
            }
            Self::GuestRefused { code } => write!(f, "program entry point refused with {code}"),
            Self::Fault(fault) => Display::fmt(fault, f),
            Self::Resource(refusal) => Display::fmt(refusal, f),
        }
    }
}

impl std::error::Error for EntrypointRefusal {}

/// Validates calldata before any guest state can be constructed.
///
/// # Errors
///
/// Returns [`EntrypointRefusal::InputTooLarge`] past the frozen ABI bound.
pub fn preflight(calldata: &[u8]) -> Result<(), EntrypointRefusal> {
    if calldata.len() > MAX_CALL_INPUT_BYTES {
        return Err(EntrypointRefusal::InputTooLarge {
            bytes: calldata.len(),
            limit: MAX_CALL_INPUT_BYTES,
        });
    }
    Ok(())
}

pub(crate) fn invoke(
    instance: &mut ProgramInstance,
    entrypoint: &str,
    calldata: &[u8],
) -> Result<i32, EntrypointRefusal> {
    preflight(calldata)?;
    let length = i32::try_from(calldata.len()).map_err(|_| EntrypointRefusal::InputTooLarge {
        bytes: calldata.len(),
        limit: MAX_CALL_INPUT_BYTES,
    })?;
    let pointer = if calldata.is_empty() {
        0
    } else {
        let fuel = u64::try_from(calldata.len())
            .ok()
            .and_then(|bytes| bytes.checked_mul(CALL_INPUT_FUEL_PER_BYTE))
            .ok_or(EntrypointRefusal::InputTooLarge {
                bytes: calldata.len(),
                limit: MAX_CALL_INPUT_BYTES,
            })?;
        instance.consume_copy_fuel(fuel)?;
        let outputs = instance
            .call(CALL_RESERVE_EXPORT, &[WasmValue::I32(length)])
            .map_err(|fault| classify(instance, fault, EntrypointRefusal::MissingAllocator))?;
        let pointer = match outputs.as_slice() {
            [WasmValue::I32(pointer)] => *pointer,
            _ => return Err(EntrypointRefusal::MissingAllocator),
        };
        let offset = usize::try_from(pointer)
            .map_err(|_| EntrypointRefusal::AllocationRefused { code: pointer })?;
        let memory = instance
            .linear_memory()
            .ok_or(EntrypointRefusal::MissingMemory)?;
        instance
            .write_linear_memory(memory, offset, calldata)
            .map_err(EntrypointRefusal::Fault)?;
        pointer
    };
    let outputs = instance
        .call(
            entrypoint,
            &[WasmValue::I32(pointer), WasmValue::I32(length)],
        )
        .map_err(|fault| classify(instance, fault, EntrypointRefusal::MissingEntry))?;
    let code = match outputs.as_slice() {
        [WasmValue::I32(code)] => *code,
        _ => return Err(EntrypointRefusal::MissingEntry),
    };
    if code < 0 {
        if let Some(refusal) = instance.meter().exhaustion() {
            return Err(EntrypointRefusal::Resource(refusal));
        }
        return Err(EntrypointRefusal::GuestRefused { code });
    }
    Ok(code)
}

fn classify(
    instance: &ProgramInstance,
    fault: ExecutionFault,
    missing: EntrypointRefusal,
) -> EntrypointRefusal {
    if let Some(refusal) = instance.meter().exhaustion() {
        return EntrypointRefusal::Resource(refusal);
    }
    match fault {
        ExecutionFault::UnknownExport { .. } | ExecutionFault::NotAFunction { .. } => missing,
        other => EntrypointRefusal::Fault(other),
    }
}
