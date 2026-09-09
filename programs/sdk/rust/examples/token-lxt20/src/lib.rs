#![no_std]

use layerx_program_sdk::{
    call,
    lxt20::{
        Request, REFERENCE_ASSET, REFERENCE_CEILING, REFERENCE_ISSUER, REFERENCE_METADATA,
        REFERENCE_SUPPLY,
    },
    payments::PreparedProgramAccount,
    storage::{
        shared::{self, SharedStorageKey},
        StorageValue,
    },
    transfer as settlement, AccountId, Amount, AssetId, Bytes, CallResult, Context, Field,
    Principal, ProgramError, Reason,
};

layerx_program_sdk::trap_on_panic!();

fn denied() -> ProgramError {
    ProgramError::value(Field::CallInput, Reason::Malformed)
}

fn direct() -> Result<Principal, ProgramError> {
    if Context::immediate_caller()?.is_some() {
        return Err(denied());
    }
    Context::invoking_principal()
}

fn account(owner: &[u8; 32]) -> Result<PreparedProgramAccount<'_>, ProgramError> {
    PreparedProgramAccount::new(
        Context::executing_program()?,
        owner,
        AssetId::new(REFERENCE_ASSET)?,
    )
}

fn key<const N: usize>(prefix: &[u8], tail: &[u8]) -> Result<Bytes<N>, ProgramError> {
    let mut out = Bytes::empty();
    out.extend(prefix)?;
    out.extend(tail)?;
    Ok(out)
}

fn read_amount(key: &[u8]) -> Result<Amount, ProgramError> {
    let mut value = [0; 16];
    match shared::read(SharedStorageKey::new(key)?, &mut value)? {
        None => Ok(Amount::ZERO),
        Some(16) => Ok(Amount::from_be_bytes(value)),
        Some(_) => Err(denied()),
    }
}

fn write_amount(key: &[u8], value: Amount) -> Result<(), ProgramError> {
    shared::write(
        SharedStorageKey::new(key)?,
        StorageValue::new(&value.to_be_bytes())?,
    )
}

fn balance_key(id: AccountId) -> Result<Bytes<40>, ProgramError> {
    key(b"balance:", &id.bytes())
}
fn known_key(id: AccountId) -> Result<Bytes<38>, ProgramError> {
    key(b"known:", &id.bytes())
}

fn register(owner: &Principal) -> Result<(), ProgramError> {
    let id = account(&owner.bytes())?.account();
    write_amount(known_key(id)?.as_slice(), Amount::from_u128(1))
}

fn allowance_key(owner: Principal, spender: Principal) -> Result<Bytes<74>, ProgramError> {
    let mut out = key(b"allowance:", &owner.bytes())?;
    out.extend(&spender.bytes())?;
    Ok(out)
}

fn response(bytes: &[u8]) -> Result<CallResult, ProgramError> {
    let mut encoded = Bytes::<70>::empty();
    encoded.extend(&[1, 0x20])?;
    encoded.extend(
        &u32::try_from(bytes.len())
            .map_err(|_| denied())?
            .to_be_bytes(),
    )?;
    encoded.extend(bytes)?;
    call::publish_response(CallResult::OK, encoded.as_slice())?;
    Ok(CallResult::OK)
}

fn initialize_impl(input: &[u8]) -> Result<CallResult, ProgramError> {
    let issuer = direct()?;
    if issuer.bytes() != REFERENCE_ISSUER
        || input != [b'L', b'X', 20, 0, 1, 0x20, 0, 0, 0, 0]
        || !read_amount(b"supply")?.is_zero()
    {
        return Err(denied());
    }
    let issuer_seed = issuer.bytes();
    let prepared = account(&issuer_seed)?;
    settlement::fund_program_account(prepared.deposit(Amount::from_u128(REFERENCE_SUPPLY))?)?;
    register(&issuer)?;
    write_amount(
        balance_key(prepared.account())?.as_slice(),
        Amount::from_u128(REFERENCE_SUPPLY),
    )?;
    write_amount(b"supply", Amount::from_u128(REFERENCE_SUPPLY))?;
    response(&[])
}

fn move_units(owner: Principal, to: AccountId, amount: Amount) -> Result<(), ProgramError> {
    if amount.is_zero()
        || amount > Amount::from_u128(REFERENCE_CEILING)
        || read_amount(known_key(to)?.as_slice())? != Amount::from_u128(1)
    {
        return Err(denied());
    }
    let owner_seed = owner.bytes();
    let prepared = account(&owner_seed)?;
    let from_key = balance_key(prepared.account())?;
    let from_before = read_amount(from_key.as_slice())?;
    let from_after = from_before.checked_sub(amount)?;
    let to_key = balance_key(to)?;
    let to_after = read_amount(to_key.as_slice())?.checked_add(amount)?;
    settlement::pay_from_program_account(prepared.payment(to, amount)?)?;
    if prepared.account() != to {
        write_amount(from_key.as_slice(), from_after)?;
        write_amount(to_key.as_slice(), to_after)?;
    }
    Ok(())
}

fn transfer_impl(input: &[u8]) -> Result<CallResult, ProgramError> {
    let caller = direct()?;
    let Request::Transfer { to, amount } = Request::decode(input)? else {
        return Err(denied());
    };
    move_units(caller, to, amount)?;
    response(&[])
}

fn approve_impl(input: &[u8]) -> Result<CallResult, ProgramError> {
    let caller = direct()?;
    let Request::Approve { spender, amount } = Request::decode(input)? else {
        return Err(denied());
    };
    register(&caller)?;
    write_amount(allowance_key(caller, spender)?.as_slice(), amount)?;
    response(&[])
}

fn transfer_from_impl(input: &[u8]) -> Result<CallResult, ProgramError> {
    let caller = direct()?;
    let Request::TransferFrom { owner, to, amount } = Request::decode(input)? else {
        return Err(denied());
    };
    let key = allowance_key(owner, caller)?;
    let remaining = read_amount(key.as_slice())?.checked_sub(amount)?;
    write_amount(key.as_slice(), remaining)?;
    move_units(owner, to, amount)?;
    response(&[])
}

fn balance_of_impl(input: &[u8]) -> Result<CallResult, ProgramError> {
    direct()?;
    let Request::BalanceOf { owner } = Request::decode(input)? else {
        return Err(denied());
    };
    response(
        &read_amount(balance_key(account(&owner.bytes())?.account())?.as_slice())?.to_be_bytes(),
    )
}

fn allowance_impl(input: &[u8]) -> Result<CallResult, ProgramError> {
    direct()?;
    let Request::Allowance { owner, spender } = Request::decode(input)? else {
        return Err(denied());
    };
    response(&read_amount(allowance_key(owner, spender)?.as_slice())?.to_be_bytes())
}

fn total_supply_impl(input: &[u8]) -> Result<CallResult, ProgramError> {
    direct()?;
    if Request::decode(input)? != Request::TotalSupply {
        return Err(denied());
    }
    response(&read_amount(b"supply")?.to_be_bytes())
}

fn metadata_impl(input: &[u8]) -> Result<CallResult, ProgramError> {
    direct()?;
    if Request::decode(input)? != Request::Metadata {
        return Err(denied());
    }
    response(REFERENCE_METADATA)
}

macro_rules! export {
    ($name:ident, $handler:ident) => {
        #[allow(unsafe_code)]
        #[no_mangle]
        pub extern "C" fn $name(pointer: i32, length: i32) -> i32 {
            match layerx_program_sdk::entry::with_call_input(pointer, length, $handler) {
                Ok(Ok(result)) => result.code(),
                Ok(Err(error)) | Err(error) => error.code(),
            }
        }
    };
}

export!(initialize, initialize_impl);
export!(transfer, transfer_impl);
export!(approve, approve_impl);
export!(transfer_from, transfer_from_impl);
export!(balance_of, balance_of_impl);
export!(allowance, allowance_impl);
export!(total_supply, total_supply_impl);
export!(metadata, metadata_impl);

fn refuse(_: &[u8]) -> Result<CallResult, ProgramError> {
    Err(denied())
}
fn legacy(_: i64) -> Result<i64, ProgramError> {
    Err(denied())
}
layerx_program_sdk::entrypoint!(refuse);
layerx_program_sdk::program!(legacy);
