//! Event host-function registration.

use wasmi::core::Trap;
use wasmi::{Caller, Linker};

use crate::calls::CompositionRefusal;
use crate::execute::ExecutionFault;

use super::memory::{read_guest, validate_guest_read};
use super::{error_status, linker_fault, RuntimeState, ABI_MODULE, COMPOSITION_REFUSED};

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
             -> Result<i32, Trap> {
                let topic_bytes = match validate_guest_read(
                    &caller,
                    topic_pointer,
                    topic_length,
                    crate::abi::MAX_EVENT_TOPIC_BYTES,
                ) {
                    Ok(length) => length,
                    Err(status) => return Ok(status),
                };
                let data_bytes = match validate_guest_read(
                    &caller,
                    data_pointer,
                    data_length,
                    crate::abi::MAX_EVENT_DATA_BYTES,
                ) {
                    Ok(length) => length,
                    Err(status) => return Ok(status),
                };
                let activity_event_limit_reached =
                    caller.data().authorization_abi().is_some_and(|abi| {
                        abi.emitted_event_count() >= crate::abi::MAX_EVENTS_PER_ACTIVITY
                    });
                if let Err(error) = caller
                    .data_mut()
                    .with_abi(|abi, meter| abi.emit_event(meter, topic_bytes, data_bytes))
                {
                    if error == crate::abi::AbiError::EventBounds && activity_event_limit_reached {
                        caller
                            .data_mut()
                            .record_refusal(CompositionRefusal::Authority(error));
                        return Err(Trap::new(COMPOSITION_REFUSED));
                    }
                    return Ok(error_status(error));
                }
                let topic = match read_guest(&caller, topic_pointer, topic_length, topic_bytes) {
                    Ok(topic) => topic,
                    Err(status) => return Ok(status),
                };
                let data = match read_guest(&caller, data_pointer, data_length, data_bytes) {
                    Ok(data) => data,
                    Err(status) => return Ok(status),
                };
                match caller
                    .data_mut()
                    .with_abi(|abi, _| abi.stage_reserved_event(topic, data))
                {
                    Ok(()) => Ok(0),
                    Err(error) => Ok(error_status(error)),
                }
            },
        )
        .map_err(|error| linker_fault(&error))?;
    Ok(())
}
