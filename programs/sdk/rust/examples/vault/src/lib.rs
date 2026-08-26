#![no_std]

use layerx_program_sdk::{
    event, storage, storage::shared, transfer, trap_on_panic, AccountId, Amount, AssetId,
    CallResult, EventData, EventTopic, Field, ProgramAccountPayment, ProgramAccountSeed,
    ProgramDeposit, ProgramError, Reason, StorageKey, StorageValue,
};

trap_on_panic!();

const VERSION: u8 = 1;
const DEPOSIT: u8 = 1;
const WITHDRAW: u8 = 2;
const POSITION_PREFIX: &[u8] = b"lx.ref.vault.position/";
const TOTAL_PREFIX: &[u8] = b"lx.ref.vault.total/";
const TOPIC: &[u8] = b"lx.ref.vault.custody";

struct Cursor<'a> {
    bytes: &'a [u8],
    offset: usize,
}

impl<'a> Cursor<'a> {
    const fn new(bytes: &'a [u8]) -> Self {
        Self { bytes, offset: 0 }
    }

    fn take(&mut self, length: usize) -> Result<&'a [u8], ProgramError> {
        let end = self.offset.checked_add(length).ok_or_else(malformed)?;
        let value = self.bytes.get(self.offset..end).ok_or_else(malformed)?;
        self.offset = end;
        Ok(value)
    }
    fn array<const N: usize>(&mut self) -> Result<[u8; N], ProgramError> {
        self.take(N)?.try_into().map_err(|_| malformed())
    }
    fn finish(self) -> Result<(), ProgramError> {
        if self.offset == self.bytes.len() {
            Ok(())
        } else {
            Err(malformed())
        }
    }
}

fn malformed() -> ProgramError {
    ProgramError::value(Field::CallInput, Reason::Malformed)
}

fn key(
    prefix: &[u8],
    account: AccountId,
    asset: AssetId,
) -> Result<([u8; 96], usize), ProgramError> {
    let mut key = [0; 96];
    let account_end = prefix.len().checked_add(32).ok_or_else(malformed)?;
    let end = account_end.checked_add(32).ok_or_else(malformed)?;
    key[..prefix.len()].copy_from_slice(prefix);
    key[prefix.len()..account_end].copy_from_slice(&account.bytes());
    key[account_end..end].copy_from_slice(&asset.bytes());
    Ok((key, end))
}

fn read_position(account: AccountId, asset: AssetId) -> Result<Amount, ProgramError> {
    let (key, length) = key(POSITION_PREFIX, account, asset)?;
    let mut encoded = [0; 16];
    match storage::read(StorageKey::new(&key[..length])?, &mut encoded)? {
        None => Ok(Amount::ZERO),
        Some(16) => Ok(Amount::from_be_bytes(encoded)),
        Some(_) => Err(malformed()),
    }
}

fn read_total(account: AccountId, asset: AssetId) -> Result<Amount, ProgramError> {
    let (key, length) = key(TOTAL_PREFIX, account, asset)?;
    let mut encoded = [0; 16];
    match shared::read(shared::SharedStorageKey::new(&key[..length])?, &mut encoded)? {
        None => Ok(Amount::ZERO),
        Some(16) => Ok(Amount::from_be_bytes(encoded)),
        Some(_) => Err(malformed()),
    }
}

fn write_position(
    account: AccountId,
    asset: AssetId,
    amount: Amount,
) -> Result<(), ProgramError> {
    let (key, length) = key(POSITION_PREFIX, account, asset)?;
    storage::write(
        StorageKey::new(&key[..length])?,
        StorageValue::new(&amount.to_be_bytes())?,
    )
}

fn write_total(
    account: AccountId,
    asset: AssetId,
    amount: Amount,
) -> Result<(), ProgramError> {
    let (key, length) = key(TOTAL_PREFIX, account, asset)?;
    shared::write(
        shared::SharedStorageKey::new(&key[..length])?,
        StorageValue::new(&amount.to_be_bytes())?,
    )
}

fn emit(
    operation: u8,
    account: AccountId,
    asset: AssetId,
    destination: AccountId,
    amount: Amount,
    position: Amount,
    total: Amount,
) -> Result<(), ProgramError> {
    let mut data = [0; 145];
    data[0] = operation;
    data[1..33].copy_from_slice(&account.bytes());
    data[33..65].copy_from_slice(&asset.bytes());
    data[65..97].copy_from_slice(&destination.bytes());
    data[97..113].copy_from_slice(&amount.to_be_bytes());
    data[113..129].copy_from_slice(&position.to_be_bytes());
    data[129..145].copy_from_slice(&total.to_be_bytes());
    event::emit(EventTopic::new(TOPIC)?, EventData::new(&data)?)
}

fn invoke(input: &[u8]) -> Result<CallResult, ProgramError> {
    let mut cursor = Cursor::new(input);
    if cursor.take(1)?[0] != VERSION {
        return Err(malformed());
    }
    let operation = cursor.take(1)?[0];
    let seed_length = usize::from(u16::from_be_bytes(cursor.array()?));
    let seed = ProgramAccountSeed::new(cursor.take(seed_length)?)?;
    let account = AccountId::new(cursor.array()?)?;
    let asset = AssetId::new(cursor.array()?)?;
    let destination = AccountId::new(cursor.array()?)?;
    let amount = Amount::from_be_bytes(cursor.array()?);
    cursor.finish()?;
    if amount.is_zero() {
        return Err(ProgramError::value(Field::Amount, Reason::Zero));
    }
    let position = read_position(account, asset)?;
    let total = read_total(account, asset)?;
    match operation {
        DEPOSIT => {
            if destination != account {
                return Err(malformed());
            }
            let next_position = position.checked_add(amount)?;
            let next_total = total.checked_add(amount)?;
            let deposit = ProgramDeposit::new(seed, account, asset, amount)?;
            transfer::fund_program_account(deposit)?;
            write_position(account, asset, next_position)?;
            write_total(account, asset, next_total)?;
            emit(
                DEPOSIT,
                account,
                asset,
                account,
                amount,
                next_position,
                next_total,
            )?;
        }
        WITHDRAW => {
            let next_position = position.checked_sub(amount)?;
            let next_total = total.checked_sub(amount)?;
            let payment = ProgramAccountPayment::new(seed, account, asset, destination, amount)?;
            transfer::pay_from_program_account(payment)?;
            write_position(account, asset, next_position)?;
            write_total(account, asset, next_total)?;
            emit(
                WITHDRAW,
                account,
                asset,
                destination,
                amount,
                next_position,
                next_total,
            )?;
        }
        _ => return Err(malformed()),
    }
    Ok(CallResult::OK)
}

fn legacy(_: i64) -> Result<i64, ProgramError> {
    Err(malformed())
}
layerx_program_sdk::program!(legacy);
layerx_program_sdk::entrypoint!(invoke);
