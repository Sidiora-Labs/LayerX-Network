//! Sight-only read of a web answer committed for the calling program.

use wasmi::{Caller, Linker};

use crate::abi::WEB_ANSWER_HEADER_BYTES;
use crate::execute::ExecutionFault;

use super::memory::{nonnegative, read_fixed, write_guest};
use super::{error_status, linker_fault, RuntimeState, STATUS_ABSENT, STATUS_BOUNDS};

const WEB_READ_HEADER_METER_BYTES: u64 = 40;

pub(super) fn register_v4(linker: &mut Linker<RuntimeState>) -> Result<(), ExecutionFault> {
    linker
        .func_wrap(
            crate::abi::manifest::ABI_V4_MODULE,
            "web_read",
            |mut caller: Caller<'_, RuntimeState>,
             request_pointer: i32,
             request_length: i32,
             output_pointer: i32,
             output_capacity: i32|
             -> i32 {
                let request_id = match read_fixed::<8>(&caller, request_pointer, request_length) {
                    Ok(value) => u64::from_le_bytes(value),
                    Err(status) => return status,
                };
                let capacity = match nonnegative(output_capacity) {
                    Ok(value) => value,
                    Err(status) => return status,
                };
                if capacity < WEB_ANSWER_HEADER_BYTES {
                    return STATUS_BOUNDS;
                }
                let record = match caller.data_mut().with_abi(|abi, meter| {
                    meter.charge_storage_read(WEB_READ_HEADER_METER_BYTES)?;
                    let Some(answer) = abi.web_read(request_id)? else {
                        return Ok(None);
                    };
                    let record = answer.canonical_bytes()?;
                    meter.charge_storage_read(
                        u64::try_from(answer.response.len())
                            .map_err(|_| crate::abi::AbiError::InvalidEncoding)?,
                    )?;
                    Ok(Some(record))
                }) {
                    Ok(Some(value)) => value,
                    Ok(None) => return STATUS_ABSENT,
                    Err(error) => return error_status(&error),
                };
                if record.len() > capacity {
                    return STATUS_BOUNDS;
                }
                let Ok(written) = i32::try_from(record.len()) else {
                    return STATUS_BOUNDS;
                };
                if let Err(status) = write_guest(&mut caller, output_pointer, &record) {
                    return status;
                }
                written
            },
        )
        .map_err(|error| linker_fault(&error))?;
    Ok(())
}
