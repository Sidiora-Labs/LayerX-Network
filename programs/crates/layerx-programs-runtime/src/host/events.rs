//! Event host-function registration.

use wasmi::{Caller, Linker};

use crate::execute::ExecutionFault;

use super::memory::read_guest;
use super::{error_status, linker_fault, RuntimeState, ABI_MODULE};

pub(super) fn register(linker: &mut Linker<RuntimeState>) -> Result<(), ExecutionFault> {
    linker
        .func_wrap(
            ABI_MODULE,
            "event_emit",
            |mut caller: Caller<'_, RuntimeState>,
             topic_pointer: i32,
             topic_length: i32,
             data_pointer: i32,
             data_length: i32|
             -> i32 {
                let topic = match read_guest(&caller, topic_pointer, topic_length, 64) {
                    Ok(topic) => topic,
                    Err(status) => return status,
                };
                let data = match read_guest(&caller, data_pointer, data_length, 65_536) {
                    Ok(data) => data,
                    Err(status) => return status,
                };
                match caller
                    .data_mut()
                    .with_abi(|abi, _| abi.emit_event(&topic, &data))
                {
                    Ok(()) => 0,
                    Err(error) => error_status(error),
                }
            },
        )
        .map_err(|error| linker_fault(&error))?;
    Ok(())
}
