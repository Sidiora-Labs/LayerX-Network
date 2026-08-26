//! Candidate execution-context host-function registration.

use wasmi::core::{Trap, TrapCode};
use wasmi::{Caller, Linker};

use crate::abi::context::{ContextField, ContextRefusal, CONTEXT_FUEL_PER_BYTE};
use crate::abi::response::CANDIDATE_ABI_MODULE;
use crate::execute::ExecutionFault;

use super::memory::validate_output;
use super::{
    linker_fault, RuntimeState, FUEL_METERING_DISABLED, STATUS_BOUNDS, STATUS_DENIED,
    STATUS_INVALID,
};

pub(super) fn register_candidate(linker: &mut Linker<RuntimeState>) -> Result<(), ExecutionFault> {
    linker
        .func_wrap(
            CANDIDATE_ABI_MODULE,
            "context_read",
            |mut caller: Caller<'_, RuntimeState>,
             raw_field: i32,
             output_pointer: i32,
             output_capacity: i32|
             -> Result<i32, Trap> {
                let field = match ContextField::try_from(raw_field) {
                    Ok(field) => field,
                    Err(ContextRefusal::UnknownField) => return Ok(STATUS_INVALID),
                    Err(_) => return Ok(STATUS_DENIED),
                };
                let output = match validate_output(&caller, output_pointer, output_capacity) {
                    Ok(output) => output,
                    Err(status) => return Ok(status),
                };
                let Some(consumed) = caller.fuel_consumed() else {
                    return Err(Trap::new(FUEL_METERING_DISABLED));
                };
                let initial = match caller.data().context_field(field, consumed) {
                    Ok(value) => value,
                    Err(ContextRefusal::UnknownField) => return Ok(STATUS_INVALID),
                    Err(ContextRefusal::Unauthenticated | ContextRefusal::FrameMismatch) => {
                        return Ok(STATUS_DENIED)
                    }
                };
                if initial.len() > output.capacity() {
                    return Ok(STATUS_BOUNDS);
                }
                let fuel = u64::try_from(initial.len())
                    .ok()
                    .and_then(|bytes| bytes.checked_mul(CONTEXT_FUEL_PER_BYTE))
                    .ok_or_else(|| Trap::from(TrapCode::OutOfFuel))?;
                if caller.consume_fuel(fuel).is_err() {
                    caller.data_mut().meter_mut().mark_cpu_exhausted();
                    return Err(Trap::from(TrapCode::OutOfFuel));
                }
                let Some(consumed) = caller.fuel_consumed() else {
                    return Err(Trap::new(FUEL_METERING_DISABLED));
                };
                let value = match caller.data().context_field(field, consumed) {
                    Ok(value) => value,
                    Err(_) => return Ok(STATUS_DENIED),
                };
                if let Err(status) = output.write(&mut caller, &value) {
                    return Ok(status);
                }
                Ok(i32::try_from(value.len()).unwrap_or(STATUS_BOUNDS))
            },
        )
        .map_err(|error| linker_fault(&error))?;
    Ok(())
}
