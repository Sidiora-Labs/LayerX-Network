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
    #[link(wasm_import_module = "layerx_v2")]
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
        pub(super) fn storage_read_scoped(
            selector: i32,
            key_pointer: i32,
            key_length: i32,
            output_pointer: i32,
            output_capacity: i32,
        ) -> i32;
        pub(super) fn storage_write_scoped(
            selector: i32,
            key_pointer: i32,
            key_length: i32,
            value_pointer: i32,
            value_length: i32,
        ) -> i32;
        pub(super) fn storage_delete_scoped(
            selector: i32,
            key_pointer: i32,
            key_length: i32,
        ) -> i32;
        pub(super) fn storage_drop_scoped(selector: i32) -> i32;
        pub(super) fn storage_scan_scoped(selector: i32, prefix_pointer: i32, prefix_length: i32, cursor_pointer: i32, cursor_length: i32, max_entries: i32, max_bytes: i32, output_pointer: i32, output_capacity: i32) -> i32;
        pub(super) fn transfer_program_402(
            amount_high: i64,
            amount_low: i64,
            seed_pointer: i32,
            seed_length: i32,
            source_pointer: i32,
            source_length: i32,
            asset_pointer: i32,
            asset_length: i32,
            recipient_pointer: i32,
            recipient_length: i32,
        ) -> i32;
        pub(super) fn fund_program_402(
            amount_high: i64,
            amount_low: i64,
            seed_pointer: i32,
            seed_length: i32,
            destination_pointer: i32,
            destination_length: i32,
            asset_pointer: i32,
            asset_length: i32,
        ) -> i32;
        pub(super) fn context_read(field: i32, output_pointer: i32, output_capacity: i32) -> i32;
        pub(super) fn balance_read(account_pointer: i32, account_length: i32, asset_pointer: i32, asset_length: i32, output_pointer: i32, output_capacity: i32) -> i32;
        pub(super) fn hash(algorithm: i32, input_pointer: i32, input_length: i32, output_pointer: i32) -> i32;
        pub(super) fn signature_verify(algorithm: i32, message_pointer: i32, message_length: i32, public_key_pointer: i32, public_key_length: i32, signature_pointer: i32, signature_length: i32) -> i32;
        pub(super) fn signature_recover(message_pointer: i32, message_length: i32, signature_pointer: i32, signature_length: i32, recovery_id: i32, output_pointer: i32, output_capacity: i32) -> i32;
        pub(super) fn bigint_mul_256(left_pointer: i32, left_length: i32, right_pointer: i32, right_length: i32, output_pointer: i32, output_capacity: i32) -> i32;
        pub(super) fn bigint_div_256(left_pointer: i32, left_length: i32, right_pointer: i32, right_length: i32, output_pointer: i32, output_capacity: i32) -> i32;
        pub(super) fn bigint_rem_256(left_pointer: i32, left_length: i32, right_pointer: i32, right_length: i32, output_pointer: i32, output_capacity: i32) -> i32;
        pub(super) fn bigint_modexp_256(base_pointer: i32, base_length: i32, exponent_pointer: i32, exponent_length: i32, modulus_pointer: i32, modulus_length: i32, output_pointer: i32, output_capacity: i32) -> i32;
    }
}

fn exact(status: i32, expected: i32) -> Result<(), ProgramError> {
    let actual = ProgramError::from_status(status)?;
    if actual != expected {
        return Err(ProgramError::value(Field::Buffer, Reason::Malformed));
    }
    Ok(())
}

pub(crate) fn context_read(field: i32, output: &mut [u8]) -> Result<i32, ProgramError> {
    let status = unsafe { candidate_raw::context_read(field, pointer_mut(output)?, length(output)?) };
    ProgramError::from_status(status)
}

pub(crate) fn balance_read(account: &[u8; 32], asset: &[u8; 32], output: &mut [u8; 16]) -> Result<i32, ProgramError> {
    let status = unsafe { candidate_raw::balance_read(pointer(account)?, 32, pointer(asset)?, 32, pointer_mut(output)?, 16) };
    exact(status, 16)?; Ok(16)
}

pub(crate) fn hash(algorithm: i32, input: &[u8], output: &mut [u8; 32]) -> Result<i32, ProgramError> {
    let status = unsafe { candidate_raw::hash(algorithm, pointer(input)?, length(input)?, pointer_mut(output)?) };
    exact(status, 0)?; Ok(0)
}

pub(crate) fn signature_verify(algorithm: i32, message: &[u8], public_key: &[u8], signature: &[u8]) -> Result<i32, ProgramError> {
    let status = unsafe { candidate_raw::signature_verify(algorithm, pointer(message)?, length(message)?, pointer(public_key)?, length(public_key)?, pointer(signature)?, length(signature)?) };
    exact(status, 0)?; Ok(0)
}

pub(crate) fn signature_recover(message: &[u8; 32], signature: &[u8; 64], recovery_id: i32, output: &mut [u8; 65]) -> Result<i32, ProgramError> {
    let status = unsafe { candidate_raw::signature_recover(pointer(message)?, 32, pointer(signature)?, 64, recovery_id, pointer_mut(output)?, 65) };
    exact(status, 65)?; Ok(65)
}

pub(crate) fn bigint_binary(operation: unsafe extern "C" fn(i32,i32,i32,i32,i32,i32)->i32, left: &[u8;32], right: &[u8;32], output: &mut [u8;32]) -> Result<i32, ProgramError> {
    let status = unsafe { operation(pointer(left)?, 32, pointer(right)?, 32, pointer_mut(output)?, 32) };
    exact(status, 32)?; Ok(32)
}

pub(crate) fn bigint_mul(left: &[u8;32], right: &[u8;32], output: &mut [u8;64]) -> Result<i32, ProgramError> { let status=unsafe{candidate_raw::bigint_mul_256(pointer(left)?,32,pointer(right)?,32,pointer_mut(output)?,64)};exact(status,64)?;Ok(64) }
pub(crate) fn bigint_div(left: &[u8;32], right: &[u8;32], output: &mut [u8;32]) -> Result<i32, ProgramError> { bigint_binary(candidate_raw::bigint_div_256, left, right, output) }
pub(crate) fn bigint_rem(left: &[u8;32], right: &[u8;32], output: &mut [u8;32]) -> Result<i32, ProgramError> { bigint_binary(candidate_raw::bigint_rem_256, left, right, output) }
pub(crate) fn bigint_modexp(base: &[u8;32], exponent: &[u8;32], modulus: &[u8;32], output: &mut [u8;32]) -> Result<i32, ProgramError> {
    let status = unsafe { candidate_raw::bigint_modexp_256(pointer(base)?,32,pointer(exponent)?,32,pointer(modulus)?,32,pointer_mut(output)?,32) };
    exact(status, 32)?; Ok(32)
}

const STORAGE_SELECTOR_PRINCIPAL: i32 = 1;

pub(crate) fn storage_drop(selector:i32) -> Result<(), ProgramError> {
    exact(unsafe { candidate_raw::storage_drop_scoped(selector) }, 0)
}

pub(crate) fn storage_scan(selector:i32, prefix: &[u8], cursor: &[u8], max_entries: u32, max_bytes: u32, output: &mut [u8]) -> Result<usize, ProgramError> {
    let status=unsafe{candidate_raw::storage_scan_scoped(selector,pointer(prefix)?,length(prefix)?,pointer(cursor)?,length(cursor)?,i32::try_from(max_entries).map_err(|_|ProgramError::value(Field::Buffer,Reason::TooLarge))?,i32::try_from(max_bytes).map_err(|_|ProgramError::value(Field::Buffer,Reason::TooLarge))?,pointer_mut(output)?,length(output)?)};
    let length=usize::try_from(ProgramError::from_status(status)?).map_err(|_|ProgramError::value(Field::Buffer,Reason::Malformed))?;
    if length>output.len(){return Err(ProgramError::value(Field::Buffer,Reason::Malformed));} Ok(length)
}

pub(crate) fn storage_drop_principal()->Result<(),ProgramError>{storage_drop(STORAGE_SELECTOR_PRINCIPAL)}
pub(crate) fn storage_drop_shared()->Result<(),ProgramError>{storage_drop(STORAGE_SELECTOR_SHARED)}
pub(crate) fn storage_scan_principal(prefix:&[u8],cursor:&[u8],max_entries:u32,max_bytes:u32,output:&mut[u8])->Result<usize,ProgramError>{storage_scan(STORAGE_SELECTOR_PRINCIPAL,prefix,cursor,max_entries,max_bytes,output)}
pub(crate) fn storage_scan_shared(prefix:&[u8],cursor:&[u8],max_entries:u32,max_bytes:u32,output:&mut[u8])->Result<usize,ProgramError>{storage_scan(STORAGE_SELECTOR_SHARED,prefix,cursor,max_entries,max_bytes,output)}

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
    exact(status, 0)?; Ok(0)
}

pub(crate) fn storage_delete(key: &[u8]) -> Result<i32, ProgramError> {
    let key_pointer = pointer(key)?;
    let key_length = length(key)?;
    let status = unsafe { raw::storage_delete(key_pointer, key_length) };
    exact(status, 0)?; Ok(0)
}

pub(crate) fn event_emit(topic: &[u8], data: &[u8]) -> Result<i32, ProgramError> {
    let topic_pointer = pointer(topic)?;
    let topic_length = length(topic)?;
    let data_pointer = pointer(data)?;
    let data_length = length(data)?;
    let status = unsafe { raw::event_emit(topic_pointer, topic_length, data_pointer, data_length) };
    exact(status, 0)?; Ok(0)
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
    exact(status, 0)?; Ok(0)
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
    exact(status, 0)?; Ok(0)
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

pub(crate) fn transfer_program_402(
    amount_high: i64,
    amount_low: i64,
    seed: &[u8],
    source: &[u8],
    asset: &[u8],
    recipient: &[u8],
) -> Result<i32, ProgramError> {
    let status = unsafe {
        candidate_raw::transfer_program_402(
            amount_high,
            amount_low,
            pointer(seed)?,
            length(seed)?,
            pointer(source)?,
            length(source)?,
            pointer(asset)?,
            length(asset)?,
            pointer(recipient)?,
            length(recipient)?,
        )
    };
    exact(status, 0)?; Ok(0)
}

pub(crate) fn fund_program_402(
    amount_high: i64,
    amount_low: i64,
    seed: &[u8],
    destination: &[u8],
    asset: &[u8],
) -> Result<i32, ProgramError> {
    let status = unsafe {
        candidate_raw::fund_program_402(
            amount_high,
            amount_low,
            pointer(seed)?,
            length(seed)?,
            pointer(destination)?,
            length(destination)?,
            pointer(asset)?,
            length(asset)?,
        )
    };
    exact(status, 0)?; Ok(0)
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

/// Frozen selector identifying the shared storage namespace.
const STORAGE_SELECTOR_SHARED: i32 = 2;

pub(crate) fn storage_read_shared(key: &[u8], output: &mut [u8]) -> Result<i32, ProgramError> {
    let key_pointer = pointer(key)?;
    let key_length = length(key)?;
    let output_capacity = length(output)?;
    let output_pointer = pointer_mut(output)?;
    let status = unsafe {
        candidate_raw::storage_read_scoped(
            STORAGE_SELECTOR_SHARED,
            key_pointer,
            key_length,
            output_pointer,
            output_capacity,
        )
    };
    ProgramError::from_status(status)
}

pub(crate) fn storage_write_shared(key: &[u8], value: &[u8]) -> Result<i32, ProgramError> {
    let key_pointer = pointer(key)?;
    let key_length = length(key)?;
    let value_pointer = pointer(value)?;
    let value_length = length(value)?;
    let status = unsafe {
        candidate_raw::storage_write_scoped(
            STORAGE_SELECTOR_SHARED,
            key_pointer,
            key_length,
            value_pointer,
            value_length,
        )
    };
    exact(status, 0)?; Ok(0)
}

pub(crate) fn storage_delete_shared(key: &[u8]) -> Result<i32, ProgramError> {
    let key_pointer = pointer(key)?;
    let key_length = length(key)?;
    let status = unsafe {
        candidate_raw::storage_delete_scoped(STORAGE_SELECTOR_SHARED, key_pointer, key_length)
    };
    exact(status, 0)?; Ok(0)
}
