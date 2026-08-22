//! Candidate-V2 bounded, resumable storage-scan host function.

use wasmi::{Caller, Linker};

use crate::abi::response::CANDIDATE_ABI_MODULE;
use crate::abi::StorageSelector;
use crate::execute::ExecutionFault;
use crate::storage::{ScanLimits, MAX_STORAGE_KEY_BYTES, MAX_STORAGE_SCAN_CURSOR_BYTES};

use super::memory::{nonnegative, read_guest, validate_output};
use super::{error_status, linker_fault, RuntimeState, STATUS_BOUNDS};

fn selector(raw: i32) -> Result<StorageSelector, i32> {
    StorageSelector::try_from(raw).map_err(error_status)
}

/// Registers `storage_scan_scoped` without changing the frozen V1 ABI.
pub(super) fn register_candidate(linker: &mut Linker<RuntimeState>) -> Result<(), ExecutionFault> {
    linker
        .func_wrap(
            CANDIDATE_ABI_MODULE,
            "storage_scan_scoped",
            |mut caller: Caller<'_, RuntimeState>,
             raw_selector: i32,
             prefix_pointer: i32,
             prefix_length: i32,
             cursor_pointer: i32,
             cursor_length: i32,
             max_entries: i32,
             max_bytes: i32,
             output_pointer: i32,
             output_capacity: i32|
             -> i32 {
                let selected = match selector(raw_selector) {
                    Ok(selected) => selected,
                    Err(status) => return status,
                };
                let output = match validate_output(&caller, output_pointer, output_capacity) {
                    Ok(output) => output,
                    Err(status) => return status,
                };
                let prefix = match read_guest(
                    &caller,
                    prefix_pointer,
                    prefix_length,
                    MAX_STORAGE_KEY_BYTES,
                ) {
                    Ok(prefix) => prefix,
                    Err(status) => return status,
                };
                let cursor = match read_guest(
                    &caller,
                    cursor_pointer,
                    cursor_length,
                    MAX_STORAGE_SCAN_CURSOR_BYTES,
                ) {
                    Ok(cursor) => cursor,
                    Err(status) => return status,
                };
                let max_entries = match nonnegative(max_entries)
                    .and_then(|value| u32::try_from(value).map_err(|_| STATUS_BOUNDS))
                {
                    Ok(value) => value,
                    Err(status) => return status,
                };
                let max_bytes = match nonnegative(max_bytes)
                    .and_then(|value| u32::try_from(value).map_err(|_| STATUS_BOUNDS))
                {
                    Ok(value) => value,
                    Err(status) => return status,
                };
                let limits = match ScanLimits::new(max_entries, max_bytes) {
                    Ok(limits) => limits,
                    Err(error) => return error_status(error.into()),
                };
                let page = match caller
                    .data_mut()
                    .with_abi(|abi, _| abi.storage_scan_preview(selected, &prefix, &cursor, limits))
                {
                    Ok(page) => page,
                    Err(error) => return error_status(error),
                };
                let encoded = match page.encode_for_guest() {
                    Ok(encoded) => encoded,
                    Err(error) => return error_status(error.into()),
                };
                if encoded.len() > output.capacity() {
                    return STATUS_BOUNDS;
                }
                if let Err(error) = caller
                    .data_mut()
                    .with_abi(|abi, meter| abi.charge_storage_scan(meter, selected, &page))
                {
                    return error_status(error);
                }
                if let Err(status) = output.write(&mut caller, &encoded) {
                    return status;
                }
                i32::try_from(encoded.len()).unwrap_or(STATUS_BOUNDS)
            },
        )
        .map_err(|error| linker_fault(&error))
}
