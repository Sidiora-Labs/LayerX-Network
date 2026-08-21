//! 402LXP transfer host-function registration.

use wasmi::{Caller, Linker};

use crate::execute::ExecutionFault;

use super::memory::read_fixed;
use super::{error_status, linker_fault, RuntimeState, ABI_MODULE};

pub(super) fn register(linker: &mut Linker<RuntimeState>) -> Result<(), ExecutionFault> {
    linker
        .func_wrap(
            ABI_MODULE,
            "transfer_402",
            |mut caller: Caller<'_, RuntimeState>,
             amount_high: i64,
             amount_low: i64,
             asset_pointer: i32,
             asset_length: i32,
             recipient_pointer: i32,
             recipient_length: i32|
             -> i32 {
                let asset = match read_fixed::<32>(&caller, asset_pointer, asset_length) {
                    Ok(asset) => asset,
                    Err(status) => return status,
                };
                let recipient = match read_fixed::<32>(&caller, recipient_pointer, recipient_length)
                {
                    Ok(recipient) => recipient,
                    Err(status) => return status,
                };
                let high = u64::from_be_bytes(amount_high.to_be_bytes());
                let low = u64::from_be_bytes(amount_low.to_be_bytes());
                let amount = u128::from(high) << 64 | u128::from(low);
                match caller
                    .data_mut()
                    .with_abi(|abi, _| abi.request_transfer(asset, recipient, amount))
                {
                    Ok(()) => 0,
                    Err(error) => error_status(error),
                }
            },
        )
        .map_err(|error| linker_fault(&error))?;
    Ok(())
}
