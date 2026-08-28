//! Host functions for deterministic cryptographic primitives.

use wasmi::{Caller, Linker};

use crate::crypto::{hash_bytes, HashAlgorithm, HashRefusal};
use crate::execute::ExecutionFault;

use super::memory::{nonnegative, read_slice, write_guest};
use super::{linker_fault, RuntimeState, STATUS_BOUNDS, STATUS_INVALID, STATUS_METER};

pub(super) fn register(linker: &mut Linker<RuntimeState>) -> Result<(), ExecutionFault> {
    linker
        .func_wrap(
            crate::abi::ABI_V2_MODULE,
            "hash",
            |mut caller: Caller<'_, RuntimeState>,
             algorithm_id: i32,
             input_pointer: i32,
             input_length: i32,
             output_pointer: i32|
             -> i32 {
                let algorithm_id = match nonnegative(algorithm_id) {
                    Ok(id) => id as u32,
                    Err(status) => return status,
                };
                let algorithm = match HashAlgorithm::from_identifier(algorithm_id) {
                    Ok(algorithm) => algorithm,
                    Err(_) => return STATUS_INVALID,
                };
                let input_length = match nonnegative(input_length) {
                    Ok(length) => length,
                    Err(status) => return status,
                };
                let input = match read_slice(&caller, input_pointer, input_length) {
                    Ok(input) => input,
                    Err(status) => return status,
                };
                let fuel_cost = match u64::try_from(input.len())
                    .ok()
                    .and_then(|len| len.checked_mul(algorithm.fuel_per_byte()))
                {
                    Some(fuel) => fuel,
                    None => return STATUS_BOUNDS,
                };
                if let Err(refusal) = super::charge_host_cpu(&mut caller, fuel_cost) {
                    caller.data_mut().record_refusal(
                        crate::calls::CompositionRefusal::Resource(refusal),
                    );
                    return STATUS_METER;
                }
                let digest = match hash_bytes(algorithm, &input) {
                    Ok(digest) => digest,
                    Err(HashRefusal::InputTooLong { .. }) => return STATUS_BOUNDS,
                    Err(HashRefusal::UnknownAlgorithm { .. }) => return STATUS_INVALID,
                };
                if let Err(status) = write_guest(&mut caller, output_pointer, &digest) {
                    return status;
                }
                0
            },
        )
        .map_err(|error| linker_fault(&error))?;
    Ok(())
}
