#![no_std]

use layerx_program_sdk::{
    call,
    lxt721::{
        Request, TokenId, REFERENCE_BASE_URI, REFERENCE_ISSUER, REFERENCE_MAX_SUPPLY,
        REFERENCE_METADATA,
    },
    storage::{
        shared::{self, SharedStorageKey},
        StorageValue,
    },
    Amount, Bytes, CallResult, Context, Field, Principal, ProgramError, Reason,
};

layerx_program_sdk::trap_on_panic!();

const HEX: &[u8; 16] = b"0123456789abcdef";
const ABSENT: [u8; 32] = [0; 32];

fn denied() -> ProgramError {
    ProgramError::value(Field::CallInput, Reason::Malformed)
}

fn direct() -> Result<Principal, ProgramError> {
    if Context::immediate_caller()?.is_some() {
        return Err(denied());
    }
    Context::invoking_principal()
}

fn key<const N: usize>(prefix: &[u8], tail: &[u8]) -> Result<Bytes<N>, ProgramError> {
    let mut out = Bytes::empty();
    out.extend(prefix)?;
    out.extend(tail)?;
    Ok(out)
}

fn owner_key(token: TokenId) -> Result<Bytes<22>, ProgramError> {
    key(b"owner:", &token.to_be_bytes())
}

fn approval_key(token: TokenId) -> Result<Bytes<25>, ProgramError> {
    key(b"approved:", &token.to_be_bytes())
}

fn balance_key(owner: &[u8; 32]) -> Result<Bytes<40>, ProgramError> {
    key(b"balance:", owner)
}

fn operator_key(owner: &[u8; 32], operator: &[u8; 32]) -> Result<Bytes<73>, ProgramError> {
    let mut out = key::<73>(b"operator:", owner)?;
    out.extend(operator)?;
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

fn read_id(key: &[u8]) -> Result<[u8; 32], ProgramError> {
    let mut value = [0; 32];
    match shared::read(SharedStorageKey::new(key)?, &mut value)? {
        None => Ok(ABSENT),
        Some(32) => Ok(value),
        Some(_) => Err(denied()),
    }
}

fn write_id(key: &[u8], value: &[u8; 32]) -> Result<(), ProgramError> {
    shared::write(SharedStorageKey::new(key)?, StorageValue::new(value)?)
}

fn require_owner(token: TokenId) -> Result<[u8; 32], ProgramError> {
    let owner = read_id(owner_key(token)?.as_slice())?;
    if owner == ABSENT {
        return Err(denied());
    }
    Ok(owner)
}

fn one() -> Amount {
    Amount::from_u128(1)
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

fn mint_impl(input: &[u8]) -> Result<CallResult, ProgramError> {
    let issuer = direct()?;
    let Request::Mint { to, token } = Request::decode(input)? else {
        return Err(denied());
    };
    if issuer.bytes() != REFERENCE_ISSUER || read_id(owner_key(token)?.as_slice())? != ABSENT {
        return Err(denied());
    }
    let supply = read_amount(b"supply")?.checked_add(one())?;
    if supply > Amount::from_u128(REFERENCE_MAX_SUPPLY) {
        return Err(denied());
    }
    let owned = to.bytes();
    let balance = balance_key(&owned)?;
    let held = read_amount(balance.as_slice())?.checked_add(one())?;
    write_id(owner_key(token)?.as_slice(), &owned)?;
    write_amount(balance.as_slice(), held)?;
    write_amount(b"supply", supply)?;
    response(&[])
}

fn transfer_impl(input: &[u8]) -> Result<CallResult, ProgramError> {
    let caller = direct()?;
    let Request::Transfer { to, token } = Request::decode(input)? else {
        return Err(denied());
    };
    let owner = require_owner(token)?;
    let recipient = to.bytes();
    if recipient == owner {
        return Err(denied());
    }
    if caller.bytes() != owner
        && read_id(approval_key(token)?.as_slice())? != caller.bytes()
        && read_amount(operator_key(&owner, &caller.bytes())?.as_slice())? != one()
    {
        return Err(denied());
    }
    let from_key = balance_key(&owner)?;
    let from_after = read_amount(from_key.as_slice())?.checked_sub(one())?;
    let to_key = balance_key(&recipient)?;
    let to_after = read_amount(to_key.as_slice())?.checked_add(one())?;
    write_id(owner_key(token)?.as_slice(), &recipient)?;
    write_id(approval_key(token)?.as_slice(), &ABSENT)?;
    write_amount(from_key.as_slice(), from_after)?;
    write_amount(to_key.as_slice(), to_after)?;
    response(&[])
}

fn approve_impl(input: &[u8]) -> Result<CallResult, ProgramError> {
    let caller = direct()?;
    let Request::Approve { spender, token } = Request::decode(input)? else {
        return Err(denied());
    };
    if require_owner(token)? != caller.bytes() {
        return Err(denied());
    }
    write_id(approval_key(token)?.as_slice(), &spender.bytes())?;
    response(&[])
}

fn set_approval_for_all_impl(input: &[u8]) -> Result<CallResult, ProgramError> {
    let caller = direct()?;
    let Request::SetApprovalForAll { operator, approved } = Request::decode(input)? else {
        return Err(denied());
    };
    write_amount(
        operator_key(&caller.bytes(), &operator.bytes())?.as_slice(),
        if approved { one() } else { Amount::ZERO },
    )?;
    response(&[])
}

fn owner_of_impl(input: &[u8]) -> Result<CallResult, ProgramError> {
    direct()?;
    let Request::OwnerOf { token } = Request::decode(input)? else {
        return Err(denied());
    };
    response(&require_owner(token)?)
}

fn balance_of_impl(input: &[u8]) -> Result<CallResult, ProgramError> {
    direct()?;
    let Request::BalanceOf { owner } = Request::decode(input)? else {
        return Err(denied());
    };
    response(&read_amount(balance_key(&owner.bytes())?.as_slice())?.to_be_bytes())
}

fn token_uri_impl(input: &[u8]) -> Result<CallResult, ProgramError> {
    direct()?;
    let Request::TokenUri { token } = Request::decode(input)? else {
        return Err(denied());
    };
    require_owner(token)?;
    let mut uri = Bytes::<50>::empty();
    uri.extend(REFERENCE_BASE_URI)?;
    for byte in token.to_be_bytes() {
        uri.extend(&[HEX[usize::from(byte >> 4)], HEX[usize::from(byte & 0x0f)]])?;
    }
    response(uri.as_slice())
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

export!(mint, mint_impl);
export!(transfer, transfer_impl);
export!(approve, approve_impl);
export!(set_approval_for_all, set_approval_for_all_impl);
export!(owner_of, owner_of_impl);
export!(balance_of, balance_of_impl);
export!(token_uri, token_uri_impl);
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
