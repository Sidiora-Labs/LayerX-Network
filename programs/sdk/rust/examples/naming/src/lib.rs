#![no_std]

use layerx_program_sdk::{
    call,
    naming::{
        decode_label, encode_label, occupancy_expiry, occupancy_price, Name, Record, Request,
        DID_KEY_PREFIX, LABEL_BYTES, NAME_KEY_PREFIX, RECORD_BYTES, REFERENCE_ASSET,
        REFERENCE_OCCUPANCY_SEED,
    },
    payments::PreparedProgramAccount,
    storage::{
        shared::{self, SharedStorageKey},
        StorageValue,
    },
    transfer as settlement, AssetId, Bytes, CallResult, Context, Field, Principal, ProgramError,
    Reason,
};

layerx_program_sdk::trap_on_panic!();

const NAME_KEY_BYTES: usize = 82;
const DID_KEY_BYTES: usize = 50;
const RESPONSE_BYTES: usize = 70;

fn denied() -> ProgramError {
    ProgramError::value(Field::CallInput, Reason::Malformed)
}

fn direct() -> Result<Principal, ProgramError> {
    if Context::immediate_caller()?.is_some() {
        return Err(denied());
    }
    Context::invoking_principal()
}

fn name_key(name: Name<'_>) -> Result<Bytes<NAME_KEY_BYTES>, ProgramError> {
    let mut out = Bytes::empty();
    out.extend(NAME_KEY_PREFIX)?;
    out.extend(name.bytes())?;
    Ok(out)
}

fn did_key(did: Principal) -> Result<Bytes<DID_KEY_BYTES>, ProgramError> {
    let mut out = Bytes::empty();
    out.extend(DID_KEY_PREFIX)?;
    out.extend(&did.bytes())?;
    Ok(out)
}

fn occupancy() -> Result<PreparedProgramAccount<'static>, ProgramError> {
    PreparedProgramAccount::new(
        Context::executing_program()?,
        REFERENCE_OCCUPANCY_SEED,
        AssetId::new(REFERENCE_ASSET)?,
    )
}

fn read_record(name: Name<'_>) -> Result<Option<Record>, ProgramError> {
    let key = name_key(name)?;
    let mut value = [0; RECORD_BYTES];
    match shared::read(SharedStorageKey::new(key.as_slice())?, &mut value)? {
        None => Ok(None),
        Some(RECORD_BYTES) => Record::decode(&value).map(Some),
        Some(_) => Err(denied()),
    }
}

fn read_reverse(
    did: Principal,
    value: &mut [u8; LABEL_BYTES],
) -> Result<Option<usize>, ProgramError> {
    let key = did_key(did)?;
    shared::read(SharedStorageKey::new(key.as_slice())?, value)
}

fn write_record(name: Name<'_>, record: Record) -> Result<(), ProgramError> {
    let key = name_key(name)?;
    shared::write(
        SharedStorageKey::new(key.as_slice())?,
        StorageValue::new(record.encode()?.as_slice())?,
    )
}

fn write_reverse(did: Principal, name: Name<'_>) -> Result<(), ProgramError> {
    let key = did_key(did)?;
    shared::write(
        SharedStorageKey::new(key.as_slice())?,
        StorageValue::new(encode_label(name)?.as_slice())?,
    )
}

fn clear_reverse(did: Principal) -> Result<(), ProgramError> {
    let key = did_key(did)?;
    shared::delete(SharedStorageKey::new(key.as_slice())?)
}

fn response(bytes: &[u8]) -> Result<CallResult, ProgramError> {
    let mut encoded = Bytes::<RESPONSE_BYTES>::empty();
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

fn register_impl(input: &[u8]) -> Result<CallResult, ProgramError> {
    let caller = direct()?;
    let Request::Register { name, did, periods } = Request::decode(input)? else {
        return Err(denied());
    };
    if did != caller {
        return Err(denied());
    }
    let height = Context::batch_height()?;
    let released = match read_record(name)? {
        None => None,
        Some(previous) if previous.expiry > height => return Err(denied()),
        Some(previous) => Some(previous.did),
    };
    settlement::fund_program_account(occupancy()?.deposit(occupancy_price(periods)?)?)?;
    if let Some(previous) = released {
        if previous != did {
            clear_reverse(previous)?;
        }
    }
    write_record(
        name,
        Record {
            did,
            expiry: occupancy_expiry(height, periods)?,
        },
    )?;
    write_reverse(did, name)?;
    response(&[])
}

fn transfer_impl(input: &[u8]) -> Result<CallResult, ProgramError> {
    let caller = direct()?;
    let Request::Transfer { name, did } = Request::decode(input)? else {
        return Err(denied());
    };
    if did == caller {
        return Err(denied());
    }
    let height = Context::batch_height()?;
    let record = read_record(name)?.ok_or_else(denied)?;
    if record.did != caller || record.expiry <= height {
        return Err(denied());
    }
    clear_reverse(caller)?;
    write_record(
        name,
        Record {
            did,
            expiry: record.expiry,
        },
    )?;
    write_reverse(did, name)?;
    response(&[])
}

fn renew_impl(input: &[u8]) -> Result<CallResult, ProgramError> {
    let caller = direct()?;
    let Request::Renew { name, periods } = Request::decode(input)? else {
        return Err(denied());
    };
    let height = Context::batch_height()?;
    let record = read_record(name)?.ok_or_else(denied)?;
    if record.did != caller || record.expiry <= height {
        return Err(denied());
    }
    settlement::fund_program_account(occupancy()?.deposit(occupancy_price(periods)?)?)?;
    write_record(
        name,
        Record {
            did: record.did,
            expiry: occupancy_expiry(record.expiry, periods)?,
        },
    )?;
    response(&[])
}

fn resolve_impl(input: &[u8]) -> Result<CallResult, ProgramError> {
    direct()?;
    let Request::Resolve { name } = Request::decode(input)? else {
        return Err(denied());
    };
    let height = Context::batch_height()?;
    let record = read_record(name)?.ok_or_else(denied)?;
    if record.expiry <= height {
        return Err(denied());
    }
    response(record.encode()?.as_slice())
}

fn reverse_resolve_impl(input: &[u8]) -> Result<CallResult, ProgramError> {
    direct()?;
    let Request::ReverseResolve { did } = Request::decode(input)? else {
        return Err(denied());
    };
    let height = Context::batch_height()?;
    let mut value = [0; LABEL_BYTES];
    let written = read_reverse(did, &mut value)?.ok_or_else(denied)?;
    let encoded = value.get(..written).ok_or_else(denied)?;
    let name = decode_label(encoded)?;
    let record = read_record(name)?.ok_or_else(denied)?;
    if record.did != did || record.expiry <= height {
        return Err(denied());
    }
    response(encoded)
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

export!(register, register_impl);
export!(transfer, transfer_impl);
export!(renew, renew_impl);
export!(resolve, resolve_impl);
export!(reverse_resolve, reverse_resolve_impl);

fn refuse(_: &[u8]) -> Result<CallResult, ProgramError> {
    Err(denied())
}
fn legacy(_: i64) -> Result<i64, ProgramError> {
    Err(denied())
}
layerx_program_sdk::entrypoint!(refuse);
layerx_program_sdk::program!(legacy);
