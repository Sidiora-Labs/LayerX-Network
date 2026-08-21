//! Program-call host-function registration.

use wasmi::core::{Trap, TrapCode};
use wasmi::{Caller, Linker};

use crate::abi::CapabilitySet;
use crate::calls::{self as runtime_calls};
use crate::execute::ExecutionFault;
use crate::storage::ProgramId;

use super::memory::{read_fixed, read_guest, validate_output};
use super::{
    error_status, linker_fault, RuntimeState, ABI_MODULE, COMPOSITION_REFUSED,
    FUEL_METERING_DISABLED, STATUS_BOUNDS, STATUS_DENIED, STATUS_INVALID,
};

pub(super) fn register(linker: &mut Linker<RuntimeState>) -> Result<(), ExecutionFault> {
    linker
        .func_wrap(
            ABI_MODULE,
            "program_call",
            |mut caller: Caller<'_, RuntimeState>,
             program_pointer: i32,
             program_length: i32,
             input_pointer: i32,
             input_length: i32,
             capabilities_pointer: i32,
             capabilities_length: i32|
             -> Result<i32, Trap> {
                let program = match read_fixed::<32>(&caller, program_pointer, program_length) {
                    Ok(program) => program,
                    Err(status) => return Ok(status),
                };
                let Ok(program) = ProgramId::new(program) else {
                    return Ok(STATUS_INVALID);
                };
                let input = match read_guest(&caller, input_pointer, input_length, 1_048_576) {
                    Ok(input) => input,
                    Err(status) => return Ok(status),
                };
                let encoded =
                    match read_guest(&caller, capabilities_pointer, capabilities_length, 16_384) {
                        Ok(encoded) => encoded,
                        Err(status) => return Ok(status),
                    };
                let capabilities = match CapabilitySet::decode_canonical(&encoded) {
                    Ok(capabilities) => capabilities,
                    Err(error) => return Ok(error_status(error)),
                };
                if caller.data().authorization_abi().is_none()
                    || caller.data().composition().is_none()
                {
                    return Ok(STATUS_DENIED);
                }
                if caller
                    .consume_fuel(crate::calls::CALL_ADMISSION_FUEL)
                    .is_err()
                {
                    caller.data_mut().meter_mut().mark_cpu_exhausted();
                    return Err(Trap::from(TrapCode::OutOfFuel));
                }
                let Some(consumed) = caller.fuel_consumed() else {
                    return Err(Trap::new(FUEL_METERING_DISABLED));
                };
                let outcome = runtime_calls::execute_nested_call(
                    caller.data_mut(),
                    consumed,
                    program,
                    &input,
                    capabilities,
                );
                match outcome {
                    Ok(outcome) => {
                        if caller.consume_fuel(outcome.subtree_fuel).is_err() {
                            caller.data_mut().meter_mut().mark_cpu_exhausted();
                            return Err(Trap::from(TrapCode::OutOfFuel));
                        }
                        Ok(outcome.code)
                    }
                    Err(refusal) => {
                        if let Some(fuel) = caller.data_mut().take_failure_subtree_fuel() {
                            if caller.consume_fuel(fuel).is_err() {
                                caller.data_mut().meter_mut().mark_cpu_exhausted();
                                return Err(Trap::from(TrapCode::OutOfFuel));
                            }
                        }
                        caller.data_mut().record_refusal(refusal);
                        Err(Trap::new(COMPOSITION_REFUSED))
                    }
                }
            },
        )
        .map_err(|error| linker_fault(&error))?;
    Ok(())
}

#[allow(clippy::too_many_lines)]
pub(super) fn register_candidate(linker: &mut Linker<RuntimeState>) -> Result<(), ExecutionFault> {
    linker
        .func_wrap(
            crate::abi::response::CANDIDATE_ABI_MODULE,
            "response_write",
            |mut caller: Caller<'_, RuntimeState>, code: i32, pointer: i32, length: i32| -> i32 {
                if let Ok(bytes) = usize::try_from(length) {
                    if bytes > crate::abi::response::MAX_CALL_RESPONSE_BYTES {
                        caller.data_mut().refuse_response(
                            crate::abi::response::ResponseRefusal::TooLarge {
                                bytes,
                                limit: crate::abi::response::MAX_CALL_RESPONSE_BYTES,
                            },
                        );
                        return STATUS_BOUNDS;
                    }
                }
                let bytes = match read_guest(
                    &caller,
                    pointer,
                    length,
                    crate::abi::response::MAX_CALL_RESPONSE_BYTES,
                ) {
                    Ok(bytes) => bytes,
                    Err(status) => {
                        caller.data_mut().refuse_response(
                            crate::abi::response::ResponseRefusal::InvalidPublication,
                        );
                        return status;
                    }
                };
                caller
                    .data_mut()
                    .publish_response_status(crate::abi::response::CallResponse { code, bytes })
            },
        )
        .map_err(|error| linker_fault(&error))?;
    linker
        .func_wrap(
            crate::abi::response::CANDIDATE_ABI_MODULE,
            "program_call_response",
            |mut caller: Caller<'_, RuntimeState>,
             program_pointer: i32,
             program_length: i32,
             input_pointer: i32,
             input_length: i32,
             capabilities_pointer: i32,
             capabilities_length: i32,
             output_pointer: i32,
             output_capacity: i32|
             -> Result<i64, Trap> {
                let output = match validate_output(&caller, output_pointer, output_capacity) {
                    Ok(output) => output,
                    Err(status) => return Ok(i64::from(status)),
                };
                if output.capacity() > crate::abi::response::MAX_CALL_RESPONSE_BYTES {
                    caller
                        .data_mut()
                        .record_refusal(crate::calls::CompositionRefusal::Response(
                            crate::abi::response::ResponseRefusal::TooLarge {
                                bytes: output.capacity(),
                                limit: crate::abi::response::MAX_CALL_RESPONSE_BYTES,
                            },
                        ));
                    return Err(Trap::new(COMPOSITION_REFUSED));
                }
                let program = match read_fixed::<32>(&caller, program_pointer, program_length) {
                    Ok(program) => program,
                    Err(status) => return Ok(i64::from(status)),
                };
                let Ok(program) = ProgramId::new(program) else {
                    return Ok(i64::from(STATUS_INVALID));
                };
                let input = match read_guest(&caller, input_pointer, input_length, 1_048_576) {
                    Ok(input) => input,
                    Err(status) => return Ok(i64::from(status)),
                };
                let encoded =
                    match read_guest(&caller, capabilities_pointer, capabilities_length, 16_384) {
                        Ok(encoded) => encoded,
                        Err(status) => return Ok(i64::from(status)),
                    };
                let capabilities = match CapabilitySet::decode_canonical(&encoded) {
                    Ok(capabilities) => capabilities,
                    Err(error) => return Ok(i64::from(error_status(error))),
                };
                if caller.data().authorization_abi().is_none()
                    || caller.data().composition().is_none()
                {
                    return Ok(i64::from(STATUS_DENIED));
                }
                if caller
                    .consume_fuel(crate::calls::CALL_ADMISSION_FUEL)
                    .is_err()
                {
                    caller.data_mut().meter_mut().mark_cpu_exhausted();
                    return Err(Trap::from(TrapCode::OutOfFuel));
                }
                let Some(consumed) = caller.fuel_consumed() else {
                    return Err(Trap::new(FUEL_METERING_DISABLED));
                };
                let outcome = runtime_calls::execute_nested_call_response(
                    caller.data_mut(),
                    consumed,
                    program,
                    &input,
                    capabilities,
                    output.capacity(),
                );
                match outcome {
                    Ok(pending) => {
                        if let Err(status) = output.write(&mut caller, &pending.response.bytes) {
                            return Ok(i64::from(status));
                        }
                        let outcome =
                            match runtime_calls::adopt_nested_call(caller.data_mut(), pending) {
                                Ok(outcome) => outcome,
                                Err(refusal) => {
                                    caller.data_mut().record_refusal(refusal);
                                    return Err(Trap::new(COMPOSITION_REFUSED));
                                }
                            };
                        if caller.consume_fuel(outcome.subtree_fuel).is_err() {
                            caller.data_mut().meter_mut().mark_cpu_exhausted();
                            return Err(Trap::from(TrapCode::OutOfFuel));
                        }
                        let code = u64::try_from(outcome.code).unwrap_or(0);
                        let length =
                            u64::try_from(outcome.response.bytes.len()).unwrap_or(u64::MAX);
                        Ok(((code << 32) | length).cast_signed())
                    }
                    Err(refusal) => {
                        if let Some(fuel) = caller.data_mut().take_failure_subtree_fuel() {
                            if caller.consume_fuel(fuel).is_err() {
                                caller.data_mut().meter_mut().mark_cpu_exhausted();
                                return Err(Trap::from(TrapCode::OutOfFuel));
                            }
                        }
                        caller.data_mut().record_refusal(refusal);
                        Err(Trap::new(COMPOSITION_REFUSED))
                    }
                }
            },
        )
        .map_err(|error| linker_fault(&error))?;
    linker
        .func_wrap(
            crate::abi::response::CANDIDATE_ABI_MODULE,
            "refusal_write",
            |mut caller: Caller<'_, RuntimeState>, class: i32, pointer: i32, length: i32| -> i32 {
                let class = u32::try_from(class)
                    .ok()
                    .and_then(|class| crate::fault::RefusalClass::decode(class).ok())
                    .filter(|class| class.is_guest_publishable());
                let Some(class) = class else {
                    caller
                        .data_mut()
                        .record_refusal(crate::calls::CompositionRefusal::Response(
                            crate::abi::response::ResponseRefusal::InvalidPublication,
                        ));
                    return STATUS_INVALID;
                };
                if let Ok(bytes) = usize::try_from(length) {
                    if bytes > crate::fault::MAX_REFUSAL_REASON_BYTES {
                        caller.data_mut().record_refusal(
                            crate::calls::CompositionRefusal::Response(
                                crate::abi::response::ResponseRefusal::TooLarge {
                                    bytes,
                                    limit: crate::fault::MAX_REFUSAL_REASON_BYTES,
                                },
                            ),
                        );
                        return STATUS_BOUNDS;
                    }
                }
                let bytes = match read_guest(
                    &caller,
                    pointer,
                    length,
                    crate::fault::MAX_REFUSAL_REASON_BYTES,
                ) {
                    Ok(bytes) => bytes,
                    Err(status) => {
                        caller.data_mut().record_refusal(
                            crate::calls::CompositionRefusal::Response(
                                crate::abi::response::ResponseRefusal::InvalidPublication,
                            ),
                        );
                        return status;
                    }
                };
                let Ok(reason) = crate::fault::RefusalReason::new(&bytes) else {
                    return STATUS_BOUNDS;
                };
                caller.data_mut().publish_failure_status(class, reason)
            },
        )
        .map_err(|error| linker_fault(&error))?;
    Ok(())
}
