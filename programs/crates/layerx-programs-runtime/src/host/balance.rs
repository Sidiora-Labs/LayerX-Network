//! Receipt-verified, sight-only balance host function.

use wasmi::{Caller, Linker};

use crate::execute::ExecutionFault;

use super::memory::{nonnegative, read_fixed, write_guest};
use super::{error_status, linker_fault, RuntimeState, STATUS_BOUNDS};

const BALANCE_RESULT_BYTES: usize = 16;
const BALANCE_RESULT_BYTES_I32: i32 = 16;
const BALANCE_READ_METER_BYTES: u64 = 16;

pub(super) fn register_candidate(
    linker: &mut Linker<RuntimeState>,
) -> Result<(), ExecutionFault> {
    linker
        .func_wrap(
            crate::abi::response::CANDIDATE_ABI_MODULE,
            "balance_read",
            |mut caller: Caller<'_, RuntimeState>,
             account_pointer: i32,
             account_length: i32,
             asset_pointer: i32,
             asset_length: i32,
             output_pointer: i32,
             output_capacity: i32|
             -> i32 {
                let account = match read_fixed::<32>(&caller, account_pointer, account_length) {
                    Ok(value) => value,
                    Err(status) => return status,
                };
                let asset = match read_fixed::<32>(&caller, asset_pointer, asset_length) {
                    Ok(value) => value,
                    Err(status) => return status,
                };
                let capacity = match nonnegative(output_capacity) {
                    Ok(value) => value,
                    Err(status) => return status,
                };
                if capacity < BALANCE_RESULT_BYTES {
                    return STATUS_BOUNDS;
                }
                let balance = match caller.data_mut().with_abi(|abi, meter| {
                    meter.charge_storage_read(BALANCE_READ_METER_BYTES)?;
                    let view = abi.balance_read(account, asset)?;
                    Ok(view.balance)
                }) {
                    Ok(value) => value,
                    Err(error) => return error_status(error),
                };
                if let Err(status) = write_guest(&mut caller, output_pointer, &balance.to_be_bytes())
                {
                    return status;
                }
                BALANCE_RESULT_BYTES_I32
            },
        )
        .map_err(|error| linker_fault(&error))?;
    Ok(())
}
