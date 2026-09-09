#![no_std]

use layerx_program_sdk::{
    payments::PreparedProgramAccount, transfer, AccountId, Amount, AssetId, CallResult, Context,
    Field, ProgramError, Reason,
};

layerx_program_sdk::trap_on_panic!();

fn malformed() -> ProgramError {
    ProgramError::value(Field::CallInput, Reason::Malformed)
}

fn array<const N: usize>(input: &[u8], offset: usize) -> Result<[u8; N], ProgramError> {
    input
        .get(offset..offset + N)
        .ok_or_else(malformed)?
        .try_into()
        .map_err(|_| malformed())
}

fn invoke(input: &[u8]) -> Result<CallResult, ProgramError> {
    if input.len() != 130 || input[..2] != [0, 1] || Context::immediate_caller()?.is_some() {
        return Err(malformed());
    }
    let asset = AssetId::new(array(input, 2)?)?;
    let merchant = AccountId::new(array(input, 34)?)?;
    let collector = AccountId::new(array(input, 66)?)?;
    let gross = Amount::from_be_bytes(array(input, 98)?);
    let fee = Amount::from_be_bytes(array(input, 114)?);
    let net = gross.checked_sub(fee)?;
    let prepared =
        PreparedProgramAccount::new(Context::executing_program()?, b"payments-merchant", asset)?;
    let deposit = prepared.deposit(gross)?;
    let proceeds = prepared.payment(merchant, net)?;
    let commission = prepared.payment(collector, fee)?;
    transfer::fund_program_account(deposit)?;
    transfer::pay_from_program_account(proceeds)?;
    transfer::pay_from_program_account(commission)?;
    Ok(CallResult::OK)
}

fn legacy(_: i64) -> Result<i64, ProgramError> {
    Err(malformed())
}
layerx_program_sdk::program!(legacy);
layerx_program_sdk::entrypoint!(invoke);
