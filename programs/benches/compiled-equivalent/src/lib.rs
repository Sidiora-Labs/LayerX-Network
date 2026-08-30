//! Compiled ABI-v2 equivalents for the interpreter cost qualification workloads.

#![cfg_attr(target_arch = "wasm32", no_std)]

#[cfg(target_arch = "wasm32")]
mod guest {
    use layerx_program_sdk::{
        call, storage, transfer, AccountId, Amount, AssetId, CallResult, Field, Payment,
        ProgramError, Reason, StorageKey, StorageValue,
    };

    const ASSET: [u8; 32] = [1; 32];
    const RECIPIENT: [u8; 32] = [2; 32];

    fn write_i64(key: &[u8], value: i64) -> Result<(), ProgramError> {
        storage::write(
            StorageKey::new(key)?,
            StorageValue::new(&value.to_be_bytes())?,
        )
    }

    fn read_i64(key: &[u8]) -> Result<i64, ProgramError> {
        let mut bytes = [0_u8; 8];
        match storage::read(StorageKey::new(key)?, &mut bytes)? {
            Some(8) => Ok(i64::from_be_bytes(bytes)),
            Some(_) => Err(ProgramError::value(Field::StorageValue, Reason::Malformed)),
            None => Ok(0),
        }
    }

    fn invoke(input: &[u8]) -> Result<CallResult, ProgramError> {
        let steps = match input {
            [0] => {
                let left = i64::from(input[0])
                    .checked_add(7)
                    .ok_or_else(|| ProgramError::value(Field::CallInput, Reason::Overflow))?;
                let right = i64::from(input[0])
                    .checked_add(5)
                    .ok_or_else(|| ProgramError::value(Field::CallInput, Reason::Overflow))?;
                write_i64(
                    b"sum",
                    left.checked_add(right)
                        .ok_or_else(|| ProgramError::value(Field::CallInput, Reason::Overflow))?,
                )?;
                5_u32
            }
            [1] => {
                let nine = i64::from(input[0])
                    .checked_add(8)
                    .ok_or_else(|| ProgramError::value(Field::CallInput, Reason::Overflow))?;
                let three = i64::from(input[0])
                    .checked_add(2)
                    .ok_or_else(|| ProgramError::value(Field::CallInput, Reason::Overflow))?;
                write_i64(
                    b"sub",
                    nine.checked_sub(three)
                        .ok_or_else(|| ProgramError::value(Field::CallInput, Reason::Overflow))?,
                )?;
                write_i64(
                    b"mul",
                    nine.checked_mul(three)
                        .ok_or_else(|| ProgramError::value(Field::CallInput, Reason::Overflow))?,
                )?;
                write_i64(
                    b"div",
                    nine.checked_div(three)
                        .ok_or_else(|| ProgramError::value(Field::CallInput, Reason::Malformed))?,
                )?;
                write_i64(b"eq", if nine == three { 1_i64 } else { 0_i64 })?;
                write_i64(b"lt", if three < nine { 1_i64 } else { 0_i64 })?;
                13_u32
            }
            [2] => {
                let mut iterations = 0_u32;
                for _ in 0..4 {
                    iterations = iterations
                        .checked_add(1)
                        .ok_or_else(|| ProgramError::value(Field::CallInput, Reason::Overflow))?;
                }
                iterations
                    .checked_add(1)
                    .ok_or_else(|| ProgramError::value(Field::CallInput, Reason::Overflow))?
            }
            [3] => {
                write_i64(b"a", 2)?;
                let mut amount = read_i64(b"a")?;
                for _ in 0..2 {
                    amount = amount
                        .checked_add(1)
                        .ok_or_else(|| ProgramError::value(Field::CallInput, Reason::Overflow))?;
                }
                for _ in 0..2 {
                    for _ in 0..2 {
                        amount = amount.checked_add(1).ok_or_else(|| {
                            ProgramError::value(Field::CallInput, Reason::Overflow)
                        })?;
                    }
                }
                storage::delete(StorageKey::new(b"a")?)?;
                transfer::pay(Payment::new(
                    AssetId::new(ASSET)?,
                    AccountId::new(RECIPIENT)?,
                    Amount::from_i64(amount)?,
                )?)?;
                17_u32
            }
            _ => return Err(ProgramError::value(Field::CallInput, Reason::Malformed)),
        };
        call::publish_response(CallResult::OK, &steps.to_be_bytes())?;
        Ok(CallResult::OK)
    }

    fn legacy(_: i64) -> Result<i64, ProgramError> {
        Err(ProgramError::value(Field::CallInput, Reason::Malformed))
    }

    layerx_program_sdk::trap_on_panic!();
    layerx_program_sdk::program!(legacy);
    layerx_program_sdk::entrypoint!(invoke);
}
