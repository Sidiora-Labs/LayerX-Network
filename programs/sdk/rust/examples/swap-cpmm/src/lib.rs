#![no_std]

use layerx_program_sdk::{
    call,
    crypto::U256,
    lxt20::Request,
    payments::{PaymentGrant, PreparedProgramAccount, ProgramPaymentCapabilities},
    storage::{
        shared::{self, SharedStorageKey},
        StorageValue,
    },
    transfer as settlement, AccountId, Amount, AssetId, Bytes, CallInput, CallResult, Capability,
    Context, Field, GrantedCapabilities, Principal, ProgramError, ProgramId, Reason,
};

layerx_program_sdk::trap_on_panic!();

const ADD_LIQUIDITY: u8 = 1;
const REMOVE_LIQUIDITY: u8 = 2;
const SWAP_EXACT_IN: u8 = 3;
const QUOTE: u8 = 4;
const RESERVES: u8 = 5;

const TOKEN_A: Token = Token {
    program: [0x5a; 32],
    asset: [0x6a; 32],
};
const TOKEN_B: Token = Token {
    program: [0x5b; 32],
    asset: [0x6b; 32],
};
const SHARE_ASSET: [u8; 32] = [0x5c; 32];
const SHARE_CEILING: u128 = 100_000;
const TREASURY_SEED: &[u8] = b"lx.ref.swap.treasury";
const BASIS_POINTS: u128 = 10_000;
const FEE_BASIS_POINTS: u128 = 30;
const NET_BASIS_POINTS: u128 = BASIS_POINTS - FEE_BASIS_POINTS;

const RESERVE_A_KEY: &[u8] = b"lx.ref.swap.reserve.a";
const RESERVE_B_KEY: &[u8] = b"lx.ref.swap.reserve.b";
const SUPPLY_KEY: &[u8] = b"lx.ref.swap.shares";
const SHARE_PREFIX: &[u8] = b"lx.ref.swap.share:";

const A_TO_B: u8 = 0;
const B_TO_A: u8 = 1;

#[derive(Clone, Copy)]
struct Token {
    program: [u8; 32],
    asset: [u8; 32],
}

fn denied() -> ProgramError {
    ProgramError::value(Field::CallInput, Reason::Malformed)
}

fn array<const N: usize>(input: &[u8], offset: usize) -> Result<[u8; N], ProgramError> {
    let end = offset.checked_add(N).ok_or_else(denied)?;
    input
        .get(offset..end)
        .ok_or_else(denied)?
        .try_into()
        .map_err(|_| denied())
}

fn calldata(input: &[u8], ordinal: u8, length: usize) -> Result<&[u8], ProgramError> {
    if input.get(..3) != Some(&[b'L', b'X', b'S'])
        || input.get(3) != Some(&ordinal)
        || input.get(4..6) != Some(&[1, 0x20])
    {
        return Err(denied());
    }
    let declared = usize::try_from(u32::from_be_bytes(array(input, 6)?)).map_err(|_| denied())?;
    let payload = input.get(10..).ok_or_else(denied)?;
    if declared != length || payload.len() != length {
        return Err(denied());
    }
    Ok(payload)
}

fn respond(bytes: &[u8]) -> Result<CallResult, ProgramError> {
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

fn widen(value: Amount) -> U256 {
    let mut bytes = [0; 32];
    bytes[16..].copy_from_slice(&value.to_be_bytes());
    U256::from_be_bytes(bytes)
}

fn product(left: Amount, right: Amount) -> Result<(u128, u128), ProgramError> {
    let wide = widen(left).widening_mul(widen(right))?.to_be_bytes();
    if wide[..32] != [0; 32] {
        return Err(ProgramError::value(Field::Amount, Reason::Overflow));
    }
    Ok((
        u128::from_be_bytes(array(&wide, 32)?),
        u128::from_be_bytes(array(&wide, 48)?),
    ))
}

fn wide_div(high: u128, low: u128, divisor: u128) -> Option<(u128, u128)> {
    if divisor == 0 || high >= divisor {
        return None;
    }
    let mut remainder = high;
    let mut quotient = 0_u128;
    let mut index = 128_u32;
    while index > 0 {
        index -= 1;
        let carry = remainder >> 127;
        remainder = (remainder << 1) | ((low >> index) & 1);
        if carry == 1 || remainder >= divisor {
            remainder = remainder.wrapping_sub(divisor);
            quotient |= 1_u128 << index;
        }
    }
    Some((quotient, remainder))
}

fn divide(left: Amount, right: Amount, divisor: Amount) -> Result<(Amount, bool), ProgramError> {
    let (high, low) = product(left, right)?;
    let (quotient, remainder) = wide_div(high, low, divisor.value())
        .ok_or_else(|| ProgramError::value(Field::Amount, Reason::Overflow))?;
    Ok((Amount::from_u128(quotient), remainder != 0))
}

fn mul_div(left: Amount, right: Amount, divisor: Amount) -> Result<Amount, ProgramError> {
    Ok(divide(left, right, divisor)?.0)
}

fn mul_div_ceil(left: Amount, right: Amount, divisor: Amount) -> Result<Amount, ProgramError> {
    let (quotient, remainder) = divide(left, right, divisor)?;
    if remainder {
        quotient.checked_add(Amount::from_u128(1))
    } else {
        Ok(quotient)
    }
}

fn output_amount(
    amount_in: Amount,
    reserve_in: Amount,
    reserve_out: Amount,
) -> Result<Amount, ProgramError> {
    if amount_in.is_zero() || reserve_in.is_zero() || reserve_out.is_zero() {
        return Err(denied());
    }
    let net = mul_div(
        amount_in,
        Amount::from_u128(NET_BASIS_POINTS),
        Amount::from_u128(BASIS_POINTS),
    )?;
    if net.is_zero() {
        return Err(denied());
    }
    let out = mul_div(reserve_out, net, reserve_in.checked_add(net)?)?;
    if out.is_zero() || out >= reserve_out {
        return Err(denied());
    }
    Ok(out)
}

fn share_account(seed: &[u8]) -> Result<AccountId, ProgramError> {
    let pool = Context::executing_program()?;
    Ok(PreparedProgramAccount::new(pool, seed, AssetId::new(SHARE_ASSET)?)?.account())
}

fn share_key(account: AccountId) -> Result<Bytes<50>, ProgramError> {
    let mut out = Bytes::empty();
    out.extend(SHARE_PREFIX)?;
    out.extend(&account.bytes())?;
    Ok(out)
}

fn token_account(token: Token, seed: &[u8]) -> Result<AccountId, ProgramError> {
    Ok(PreparedProgramAccount::new(
        ProgramId::new(token.program)?,
        seed,
        AssetId::new(token.asset)?,
    )?
    .account())
}

fn call_token(
    token: Token,
    request: Request,
    source_seed: &[u8],
    to: AccountId,
    amount: Amount,
) -> Result<(), ProgramError> {
    let program = ProgramId::new(token.program)?;
    let source = PreparedProgramAccount::new(program, source_seed, AssetId::new(token.asset)?)?;
    let mut grants = ProgramPaymentCapabilities::<3>::empty();
    grants.insert(PaymentGrant::Basic(Capability::SharedStorageRead))?;
    grants.insert(PaymentGrant::Basic(Capability::SharedStorageWrite))?;
    grants.insert(PaymentGrant::ProgramSpend {
        account: source,
        to,
        maximum: amount,
    })?;
    let mut scratch = [0; 256];
    let written = grants.encode_into(&mut scratch)?;
    let input = request.encode()?;
    let code = call::invoke(
        program,
        CallInput::new(input.as_slice())?,
        GrantedCapabilities::new(scratch.get(..written).ok_or_else(denied)?)?,
    )?;
    if code == 0 {
        Ok(())
    } else {
        Err(denied())
    }
}

fn pull(token: Token, owner: Principal, amount: Amount) -> Result<(), ProgramError> {
    let pool = Context::executing_program()?.bytes();
    let to = token_account(token, &pool)?;
    let seed = owner.bytes();
    call_token(
        token,
        Request::TransferFrom { owner, to, amount },
        &seed,
        to,
        amount,
    )
}

fn push(token: Token, owner: Principal, amount: Amount) -> Result<(), ProgramError> {
    let pool = Context::executing_program()?.bytes();
    let seed = owner.bytes();
    let to = token_account(token, &seed)?;
    call_token(token, Request::Transfer { to, amount }, &pool, to, amount)
}

fn move_shares(seed: &[u8], to: AccountId, amount: Amount) -> Result<(), ProgramError> {
    let pool = Context::executing_program()?;
    let source = PreparedProgramAccount::new(pool, seed, AssetId::new(SHARE_ASSET)?)?;
    settlement::pay_from_program_account(source.payment(to, amount)?)
}

fn credit_shares(account: AccountId, amount: Amount) -> Result<(), ProgramError> {
    let key = share_key(account)?;
    let balance = read_amount(key.as_slice())?.checked_add(amount)?;
    write_amount(key.as_slice(), balance)
}

fn debit_shares(account: AccountId, amount: Amount) -> Result<(), ProgramError> {
    let key = share_key(account)?;
    let balance = read_amount(key.as_slice())?.checked_sub(amount)?;
    write_amount(key.as_slice(), balance)
}

fn add_liquidity_impl(input: &[u8]) -> Result<CallResult, ProgramError> {
    let payload = calldata(input, ADD_LIQUIDITY, 80)?;
    let recipient = AccountId::new(array(payload, 0)?)?;
    let shares = Amount::from_be_bytes(array(payload, 32)?);
    let maximum_a = Amount::from_be_bytes(array(payload, 48)?);
    let maximum_b = Amount::from_be_bytes(array(payload, 64)?);
    let provider = Context::invoking_principal()?;
    let seed = provider.bytes();
    if shares.is_zero() || recipient != share_account(&seed)? {
        return Err(denied());
    }
    let supply = read_amount(SUPPLY_KEY)?;
    let reserve_a = read_amount(RESERVE_A_KEY)?;
    let reserve_b = read_amount(RESERVE_B_KEY)?;
    let (amount_a, amount_b) = if supply.is_zero() {
        if !reserve_a.is_zero() || !reserve_b.is_zero() {
            return Err(denied());
        }
        (shares, maximum_b)
    } else {
        (
            mul_div_ceil(shares, reserve_a, supply)?,
            mul_div_ceil(shares, reserve_b, supply)?,
        )
    };
    let next_supply = supply.checked_add(shares)?;
    if amount_a.is_zero()
        || amount_b.is_zero()
        || amount_a > maximum_a
        || amount_b > maximum_b
        || next_supply > Amount::from_u128(SHARE_CEILING)
    {
        return Err(denied());
    }
    pull(TOKEN_A, provider, amount_a)?;
    pull(TOKEN_B, provider, amount_b)?;
    move_shares(TREASURY_SEED, recipient, shares)?;
    write_amount(RESERVE_A_KEY, reserve_a.checked_add(amount_a)?)?;
    write_amount(RESERVE_B_KEY, reserve_b.checked_add(amount_b)?)?;
    write_amount(SUPPLY_KEY, next_supply)?;
    credit_shares(recipient, shares)?;
    respond(&[])
}

fn remove_liquidity_impl(input: &[u8]) -> Result<CallResult, ProgramError> {
    let payload = calldata(input, REMOVE_LIQUIDITY, 80)?;
    let treasury = AccountId::new(array(payload, 0)?)?;
    let shares = Amount::from_be_bytes(array(payload, 32)?);
    let minimum_a = Amount::from_be_bytes(array(payload, 48)?);
    let minimum_b = Amount::from_be_bytes(array(payload, 64)?);
    let provider = Context::invoking_principal()?;
    let seed = provider.bytes();
    if shares.is_zero() || treasury != share_account(TREASURY_SEED)? {
        return Err(denied());
    }
    let supply = read_amount(SUPPLY_KEY)?;
    let reserve_a = read_amount(RESERVE_A_KEY)?;
    let reserve_b = read_amount(RESERVE_B_KEY)?;
    let amount_a = mul_div(shares, reserve_a, supply)?;
    let amount_b = mul_div(shares, reserve_b, supply)?;
    if amount_a.is_zero() || amount_b.is_zero() || amount_a < minimum_a || amount_b < minimum_b {
        return Err(denied());
    }
    let holder = share_account(&seed)?;
    debit_shares(holder, shares)?;
    move_shares(&seed, treasury, shares)?;
    push(TOKEN_A, provider, amount_a)?;
    push(TOKEN_B, provider, amount_b)?;
    write_amount(RESERVE_A_KEY, reserve_a.checked_sub(amount_a)?)?;
    write_amount(RESERVE_B_KEY, reserve_b.checked_sub(amount_b)?)?;
    write_amount(SUPPLY_KEY, supply.checked_sub(shares)?)?;
    respond(&[])
}

fn swap_exact_in_impl(input: &[u8]) -> Result<CallResult, ProgramError> {
    let payload = calldata(input, SWAP_EXACT_IN, 33)?;
    let direction = *payload.first().ok_or_else(denied)?;
    let amount_in = Amount::from_be_bytes(array(payload, 1)?);
    let minimum_out = Amount::from_be_bytes(array(payload, 17)?);
    let trader = Context::invoking_principal()?;
    let reserve_a = read_amount(RESERVE_A_KEY)?;
    let reserve_b = read_amount(RESERVE_B_KEY)?;
    let (token_in, token_out, reserve_in, reserve_out) = match direction {
        A_TO_B => (TOKEN_A, TOKEN_B, reserve_a, reserve_b),
        B_TO_A => (TOKEN_B, TOKEN_A, reserve_b, reserve_a),
        _ => return Err(denied()),
    };
    let amount_out = output_amount(amount_in, reserve_in, reserve_out)?;
    if amount_out < minimum_out {
        return Err(denied());
    }
    let next_in = reserve_in.checked_add(amount_in)?;
    let next_out = reserve_out.checked_sub(amount_out)?;
    pull(token_in, trader, amount_in)?;
    push(token_out, trader, amount_out)?;
    if direction == A_TO_B {
        write_amount(RESERVE_A_KEY, next_in)?;
        write_amount(RESERVE_B_KEY, next_out)?;
    } else {
        write_amount(RESERVE_B_KEY, next_in)?;
        write_amount(RESERVE_A_KEY, next_out)?;
    }
    respond(&amount_out.to_be_bytes())
}

fn quote_impl(input: &[u8]) -> Result<CallResult, ProgramError> {
    let payload = calldata(input, QUOTE, 17)?;
    let direction = *payload.first().ok_or_else(denied)?;
    let amount_in = Amount::from_be_bytes(array(payload, 1)?);
    let reserve_a = read_amount(RESERVE_A_KEY)?;
    let reserve_b = read_amount(RESERVE_B_KEY)?;
    let (reserve_in, reserve_out) = match direction {
        A_TO_B => (reserve_a, reserve_b),
        B_TO_A => (reserve_b, reserve_a),
        _ => return Err(denied()),
    };
    respond(&output_amount(amount_in, reserve_in, reserve_out)?.to_be_bytes())
}

fn reserves_impl(input: &[u8]) -> Result<CallResult, ProgramError> {
    calldata(input, RESERVES, 0)?;
    let mut out = Bytes::<48>::empty();
    out.extend(&read_amount(RESERVE_A_KEY)?.to_be_bytes())?;
    out.extend(&read_amount(RESERVE_B_KEY)?.to_be_bytes())?;
    out.extend(&read_amount(SUPPLY_KEY)?.to_be_bytes())?;
    respond(out.as_slice())
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

export!(add_liquidity, add_liquidity_impl);
export!(remove_liquidity, remove_liquidity_impl);
export!(swap_exact_in, swap_exact_in_impl);
export!(quote, quote_impl);
export!(reserves, reserves_impl);

fn refuse(_: &[u8]) -> Result<CallResult, ProgramError> {
    Err(denied())
}
fn legacy(_: i64) -> Result<i64, ProgramError> {
    Err(denied())
}
layerx_program_sdk::entrypoint!(refuse);
layerx_program_sdk::program!(legacy);
