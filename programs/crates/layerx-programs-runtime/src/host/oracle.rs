//! Sight-only read of the committed perps oracle observation.

use wasmi::{Caller, Linker};

use crate::abi::ORACLE_OBSERVATION_BYTES;
use crate::execute::ExecutionFault;

use super::memory::{nonnegative, read_fixed, write_guest};
use super::{error_status, linker_fault, RuntimeState, STATUS_BOUNDS};

const ORACLE_RESULT_BYTES_I32: i32 = 64;
const ORACLE_READ_METER_BYTES: u64 = 64;

pub(super) fn register_v3(linker: &mut Linker<RuntimeState>) -> Result<(), ExecutionFault> {
    linker
        .func_wrap(
            crate::abi::manifest::ABI_V3_MODULE,
            "oracle_read",
            |mut caller: Caller<'_, RuntimeState>,
             market_pointer: i32,
             market_length: i32,
             output_pointer: i32,
             output_capacity: i32|
             -> i32 {
                let market = match read_fixed::<32>(&caller, market_pointer, market_length) {
                    Ok(value) => value,
                    Err(status) => return status,
                };
                let capacity = match nonnegative(output_capacity) {
                    Ok(value) => value,
                    Err(status) => return status,
                };
                if capacity < ORACLE_OBSERVATION_BYTES {
                    return STATUS_BOUNDS;
                }
                let record = match caller.data_mut().with_abi(|abi, meter| {
                    meter.charge_storage_read(ORACLE_READ_METER_BYTES)?;
                    let observation = abi.oracle_read(market)?;
                    Ok(observation.canonical_bytes())
                }) {
                    Ok(value) => value,
                    Err(error) => return error_status(&error),
                };
                if let Err(status) = write_guest(&mut caller, output_pointer, &record) {
                    return status;
                }
                ORACLE_RESULT_BYTES_I32
            },
        )
        .map_err(|error| linker_fault(&error))?;
    Ok(())
}
