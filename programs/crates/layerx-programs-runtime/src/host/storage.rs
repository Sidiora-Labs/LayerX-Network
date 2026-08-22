//! Namespaced-storage host-function registration.

use wasmi::{Caller, Linker};

use crate::abi::response::CANDIDATE_ABI_MODULE;
use crate::abi::StorageSelector;
use crate::execute::ExecutionFault;

use super::memory::{nonnegative, read_guest, write_guest};
use super::{error_status, linker_fault, RuntimeState, ABI_MODULE, STATUS_BOUNDS};

fn selector(raw: i32) -> Result<StorageSelector, i32> {
    StorageSelector::try_from(raw).map_err(error_status)
}

pub(super) fn register(linker: &mut Linker<RuntimeState>) -> Result<(), ExecutionFault> {
    linker
        .func_wrap(
            ABI_MODULE,
            "storage_read",
            |mut caller: Caller<'_, RuntimeState>,
             key_pointer: i32,
             key_length: i32,
             output_pointer: i32,
             output_capacity: i32|
             -> i32 {
                let key = match read_guest(&caller, key_pointer, key_length, 256) {
                    Ok(key) => key,
                    Err(status) => return status,
                };
                let value = match caller
                    .data_mut()
                    .with_abi(|abi, meter| abi.storage_read(meter, &key))
                {
                    Ok(value) => value,
                    Err(error) => return error_status(error),
                };
                let Some(value) = value else {
                    return 0;
                };
                let capacity = match nonnegative(output_capacity) {
                    Ok(capacity) => capacity,
                    Err(status) => return status,
                };
                if value.len() > capacity {
                    return STATUS_BOUNDS;
                }
                if let Err(status) = write_guest(&mut caller, output_pointer, &value) {
                    return status;
                }
                i32::try_from(value.len())
                    .ok()
                    .and_then(|length| length.checked_add(1))
                    .unwrap_or(STATUS_BOUNDS)
            },
        )
        .map_err(|error| linker_fault(&error))?;
    linker
        .func_wrap(
            ABI_MODULE,
            "storage_write",
            |mut caller: Caller<'_, RuntimeState>,
             key_pointer: i32,
             key_length: i32,
             value_pointer: i32,
             value_length: i32|
             -> i32 {
                let key = match read_guest(&caller, key_pointer, key_length, 256) {
                    Ok(key) => key,
                    Err(status) => return status,
                };
                let value = match read_guest(&caller, value_pointer, value_length, 1_048_576) {
                    Ok(value) => value,
                    Err(status) => return status,
                };
                match caller
                    .data_mut()
                    .with_abi(|abi, meter| abi.storage_write(meter, &key, &value))
                {
                    Ok(()) => 0,
                    Err(error) => error_status(error),
                }
            },
        )
        .map_err(|error| linker_fault(&error))?;
    linker
        .func_wrap(
            ABI_MODULE,
            "storage_delete",
            |mut caller: Caller<'_, RuntimeState>, key_pointer: i32, key_length: i32| -> i32 {
                let key = match read_guest(&caller, key_pointer, key_length, 256) {
                    Ok(key) => key,
                    Err(status) => return status,
                };
                match caller
                    .data_mut()
                    .with_abi(|abi, meter| abi.storage_delete(meter, &key))
                {
                    Ok(()) => 0,
                    Err(error) => error_status(error),
                }
            },
        )
        .map_err(|error| linker_fault(&error))?;
    Ok(())
}

pub(super) fn register_candidate(linker: &mut Linker<RuntimeState>) -> Result<(), ExecutionFault> {
    linker
        .func_wrap(
            CANDIDATE_ABI_MODULE,
            "storage_read_scoped",
            |mut caller: Caller<'_, RuntimeState>,
             raw_selector: i32,
             key_pointer: i32,
             key_length: i32,
             output_pointer: i32,
             output_capacity: i32|
             -> i32 {
                let selected = match selector(raw_selector) {
                    Ok(selected) => selected,
                    Err(status) => return status,
                };
                let key = match read_guest(&caller, key_pointer, key_length, 256) {
                    Ok(key) => key,
                    Err(status) => return status,
                };
                let value = match caller
                    .data_mut()
                    .with_abi(|abi, meter| abi.storage_read_selected(meter, selected, &key))
                {
                    Ok(value) => value,
                    Err(error) => return error_status(error),
                };
                let Some(value) = value else {
                    return 0;
                };
                let capacity = match nonnegative(output_capacity) {
                    Ok(capacity) => capacity,
                    Err(status) => return status,
                };
                if value.len() > capacity {
                    return STATUS_BOUNDS;
                }
                if let Err(status) = write_guest(&mut caller, output_pointer, &value) {
                    return status;
                }
                i32::try_from(value.len())
                    .ok()
                    .and_then(|length| length.checked_add(1))
                    .unwrap_or(STATUS_BOUNDS)
            },
        )
        .map_err(|error| linker_fault(&error))?;
    linker
        .func_wrap(
            CANDIDATE_ABI_MODULE,
            "storage_write_scoped",
            |mut caller: Caller<'_, RuntimeState>,
             raw_selector: i32,
             key_pointer: i32,
             key_length: i32,
             value_pointer: i32,
             value_length: i32|
             -> i32 {
                let selected = match selector(raw_selector) {
                    Ok(selected) => selected,
                    Err(status) => return status,
                };
                let key = match read_guest(&caller, key_pointer, key_length, 256) {
                    Ok(key) => key,
                    Err(status) => return status,
                };
                let value = match read_guest(&caller, value_pointer, value_length, 1_048_576) {
                    Ok(value) => value,
                    Err(status) => return status,
                };
                match caller.data_mut().with_abi(|abi, meter| {
                    abi.storage_write_selected(meter, selected, &key, &value)
                }) {
                    Ok(()) => 0,
                    Err(error) => error_status(error),
                }
            },
        )
        .map_err(|error| linker_fault(&error))?;
    linker
        .func_wrap(
            CANDIDATE_ABI_MODULE,
            "storage_delete_scoped",
            |mut caller: Caller<'_, RuntimeState>,
             raw_selector: i32,
             key_pointer: i32,
             key_length: i32|
             -> i32 {
                let selected = match selector(raw_selector) {
                    Ok(selected) => selected,
                    Err(status) => return status,
                };
                let key = match read_guest(&caller, key_pointer, key_length, 256) {
                    Ok(key) => key,
                    Err(status) => return status,
                };
                match caller
                    .data_mut()
                    .with_abi(|abi, meter| abi.storage_delete_selected(meter, selected, &key))
                {
                    Ok(()) => 0,
                    Err(error) => error_status(error),
                }
            },
        )
        .map_err(|error| linker_fault(&error))?;
    linker
        .func_wrap(
            CANDIDATE_ABI_MODULE,
            "storage_drop_scoped",
            |mut caller: Caller<'_, RuntimeState>, raw_selector: i32| -> i32 {
                let selected = match selector(raw_selector) {
                    Ok(selected) => selected,
                    Err(status) => return status,
                };
                match caller
                    .data_mut()
                    .with_abi(|abi, meter| abi.storage_drop_selected(meter, selected))
                {
                    Ok(_) => 0,
                    Err(error) => error_status(error),
                }
            },
        )
        .map_err(|error| linker_fault(&error))?;
    Ok(())
}
