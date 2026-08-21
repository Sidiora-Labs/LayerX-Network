//! Raw `layerx_v1` imports and their pointer marshalling.
//!
//! The seven declarations below are the whole host surface a version-one
//! program may import. The deterministic-subset validator refuses any module
//! importing anything else, so this module is the only place guest code
//! crosses into the runtime.

use crate::error::{Field, ProgramError, Reason};

mod raw {
    #[link(wasm_import_module = "layerx_v1")]
    unsafe extern "C" {
        pub(super) fn storage_read(
            key_pointer: i32,
            key_length: i32,
            output_pointer: i32,
            output_capacity: i32,
        ) -> i32;

        pub(super) fn storage_write(
            key_pointer: i32,
            key_length: i32,
            value_pointer: i32,
            value_length: i32,
        ) -> i32;

        pub(super) fn storage_delete(key_pointer: i32, key_length: i32) -> i32;

        pub(super) fn event_emit(
            topic_pointer: i32,
            topic_length: i32,
            data_pointer: i32,
            data_length: i32,
        ) -> i32;

        pub(super) fn program_call(
            program_pointer: i32,
            program_length: i32,
            input_pointer: i32,
            input_length: i32,
            capabilities_pointer: i32,
            capabilities_length: i32,
        ) -> i32;

        pub(super) fn transfer_402(
            amount_high: i64,
            amount_low: i64,
            asset_pointer: i32,
            asset_length: i32,
            recipient_pointer: i32,
            recipient_length: i32,
        ) -> i32;

        pub(super) fn receipt_read(
            digest_pointer: i32,
            digest_length: i32,
            output_pointer: i32,
            output_capacity: i32,
        ) -> i32;
    }
}

mod candidate_raw {
    #[link(wasm_import_module = "layerx_v2_candidate")]
    unsafe extern "C" {
        pub(super) fn response_write(code: i32, pointer: i32, length: i32) -> i32;
        pub(super) fn program_call_response(
            program_pointer: i32,
            program_length: i32,
            input_pointer: i32,
            input_length: i32,
            capabilities_pointer: i32,
            capabilities_length: i32,
            output_pointer: i32,
            output_capacity: i32,
        ) -> i64;
        pub(super) fn refusal_write(class: i32, reason_pointer: i32, reason_length: i32) -> i32;
    }
}

fn pointer(bytes: &[u8]) -> Result<i32, ProgramError> {
    i32::try_from(bytes.as_ptr() as usize)
        .map_err(|_| ProgramError::value(Field::Buffer, Reason::TooLarge))
}

fn pointer_mut(bytes: &mut [u8]) -> Result<i32, ProgramError> {
    i32::try_from(bytes.as_mut_ptr() as usize)
        .map_err(|_| ProgramError::value(Field::Buffer, Reason::TooLarge))
}

fn length(bytes: &[u8]) -> Result<i32, ProgramError> {
    i32::try_from(bytes.len()).map_err(|_| ProgramError::value(Field::Buffer, Reason::TooLarge))
}

pub(crate) fn storage_read(key: &[u8], output: &mut [u8]) -> Result<i32, ProgramError> {
    let key_pointer = pointer(key)?;
    let key_length = length(key)?;
    let output_capacity = length(output)?;
    let output_pointer = pointer_mut(output)?;
    let status =
        unsafe { raw::storage_read(key_pointer, key_length, output_pointer, output_capacity) };
    ProgramError::from_status(status)
}

pub(crate) fn storage_write(key: &[u8], value: &[u8]) -> Result<i32, ProgramError> {
    let key_pointer = pointer(key)?;
    let key_length = length(key)?;
    let value_pointer = pointer(value)?;
    let value_length = length(value)?;
    let status =
        unsafe { raw::storage_write(key_pointer, key_length, value_pointer, value_length) };
    ProgramError::from_status(status)
}

pub(crate) fn storage_delete(key: &[u8]) -> Result<i32, ProgramError> {
    let key_pointer = pointer(key)?;
    let key_length = length(key)?;
    let status = unsafe { raw::storage_delete(key_pointer, key_length) };
    ProgramError::from_status(status)
}

pub(crate) fn event_emit(topic: &[u8], data: &[u8]) -> Result<i32, ProgramError> {
    let topic_pointer = pointer(topic)?;
    let topic_length = length(topic)?;
    let data_pointer = pointer(data)?;
    let data_length = length(data)?;
    let status = unsafe { raw::event_emit(topic_pointer, topic_length, data_pointer, data_length) };
    ProgramError::from_status(status)
}

pub(crate) fn program_call(
    program: &[u8],
    input: &[u8],
    capabilities: &[u8],
) -> Result<i32, ProgramError> {
    let program_pointer = pointer(program)?;
    let program_length = length(program)?;
    let input_pointer = pointer(input)?;
    let input_length = length(input)?;
    let capabilities_pointer = pointer(capabilities)?;
    let capabilities_length = length(capabilities)?;
    let status = unsafe {
        raw::program_call(
            program_pointer,
            program_length,
            input_pointer,
            input_length,
            capabilities_pointer,
            capabilities_length,
        )
    };
    ProgramError::from_status(status)
}

pub(crate) fn response_write(code: i32, bytes: &[u8]) -> Result<i32, ProgramError> {
    let status = unsafe { candidate_raw::response_write(code, pointer(bytes)?, length(bytes)?) };
    ProgramError::from_status(status)
}

pub(crate) fn refusal_write(
    class: crate::RefusalClass,
    reason: crate::RefusalReason<'_>,
) -> Result<i32, ProgramError> {
    let bytes = reason.bytes();
    let reason_pointer = if bytes.is_empty() { 0 } else { pointer(bytes)? };
    let status = unsafe {
        candidate_raw::refusal_write(
            i32::try_from(class.code())
                .map_err(|_| ProgramError::value(Field::Buffer, Reason::TooLarge))?,
            reason_pointer,
            length(bytes)?,
        )
    };
    ProgramError::from_status(status)
}

pub(crate) fn program_call_response(
    program: &[u8],
    input: &[u8],
    capabilities: &[u8],
    output: &mut [u8],
) -> Result<i64, ProgramError> {
    let output_pointer = if output.is_empty() {
        0
    } else {
        pointer_mut(output)?
    };
    let packed = unsafe {
        candidate_raw::program_call_response(
            pointer(program)?,
            length(program)?,
            pointer(input)?,
            length(input)?,
            pointer(capabilities)?,
            length(capabilities)?,
            output_pointer,
            length(output)?,
        )
    };
    if packed < 0 {
        let status = i32::try_from(packed).unwrap_or(crate::error::STATUS_INVALID);
        return Err(ProgramError::Host(crate::error::HostRefusal::from_status(
            status,
        )));
    }
    Ok(packed)
}

pub(crate) fn transfer_402(
    amount_high: i64,
    amount_low: i64,
    asset: &[u8],
    recipient: &[u8],
) -> Result<i32, ProgramError> {
    let asset_pointer = pointer(asset)?;
    let asset_length = length(asset)?;
    let recipient_pointer = pointer(recipient)?;
    let recipient_length = length(recipient)?;
    let status = unsafe {
        raw::transfer_402(
            amount_high,
            amount_low,
            asset_pointer,
            asset_length,
            recipient_pointer,
            recipient_length,
        )
    };
    ProgramError::from_status(status)
}

pub(crate) fn receipt_read(digest: &[u8], output: &mut [u8]) -> Result<i32, ProgramError> {
    let digest_pointer = pointer(digest)?;
    let digest_length = length(digest)?;
    let output_capacity = length(output)?;
    let output_pointer = pointer_mut(output)?;
    let status = unsafe {
        raw::receipt_read(
            digest_pointer,
            digest_length,
            output_pointer,
            output_capacity,
        )
    };
    ProgramError::from_status(status)
}
