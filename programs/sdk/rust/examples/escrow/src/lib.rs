#![no_std]

use layerx_program_sdk::{
    event, receipt, storage::shared, transfer, trap_on_panic, AccountId, Amount, AssetId,
    CallResult, EventData, EventTopic, Field, ProgramAccountPayment, ProgramAccountSeed,
    ProgramDeposit, ProgramError, Reason, ReceiptDigest, StorageValue,
};

trap_on_panic!();

const VERSION: u8 = 1;
const OPEN: u8 = 1;
const RELEASE: u8 = 2;
const REFUND: u8 = 3;
const FUNDED: u8 = 1;
const RELEASED: u8 = 2;
const REFUNDED: u8 = 3;
const KEY_PREFIX: &[u8] = b"lx.ref.escrow/";
const TOPIC: &[u8] = b"lx.ref.escrow.custody";
const RECORD_CAPACITY: usize = 339;

#[derive(Clone, Copy)]
struct Record<'a> {
    status: u8,
    seed: ProgramAccountSeed<'a>,
    account: AccountId,
    asset: AssetId,
    beneficiary: AccountId,
    refund: AccountId,
    amount: Amount,
    release_receipt: ReceiptDigest,
    refund_receipt: ReceiptDigest,
}

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

fn state_key(account: AccountId) -> Result<([u8; 48], usize), ProgramError> {
    let mut key = [0; 48];
    let length = KEY_PREFIX.len().checked_add(32).ok_or_else(malformed)?;
    key[..KEY_PREFIX.len()].copy_from_slice(KEY_PREFIX);
    key[KEY_PREFIX.len()..length].copy_from_slice(&account.bytes());
    Ok((key, length))
}

fn append(output: &mut [u8], offset: &mut usize, value: &[u8]) -> Result<(), ProgramError> {
    let end = offset.checked_add(value.len()).ok_or_else(malformed)?;
    output
        .get_mut(*offset..end)
        .ok_or_else(malformed)?
        .copy_from_slice(value);
    *offset = end;
    Ok(())
}

fn encode(record: Record<'_>, output: &mut [u8]) -> Result<usize, ProgramError> {
    let mut offset = 0;
    append(output, &mut offset, &[record.status])?;
    let seed_length = u16::try_from(record.seed.bytes().len())
        .map_err(|_| malformed())?
        .to_be_bytes();
    append(output, &mut offset, &seed_length)?;
    append(output, &mut offset, record.seed.bytes())?;
    append(output, &mut offset, &record.account.bytes())?;
    append(output, &mut offset, &record.asset.bytes())?;
    append(output, &mut offset, &record.beneficiary.bytes())?;
    append(output, &mut offset, &record.refund.bytes())?;
    append(output, &mut offset, &record.amount.to_be_bytes())?;
    append(output, &mut offset, &record.release_receipt.bytes())?;
    append(output, &mut offset, &record.refund_receipt.bytes())?;
    Ok(offset)
}

fn decode(encoded: &[u8]) -> Result<Record<'_>, ProgramError> {
    let mut cursor = Cursor::new(encoded);
    let status = cursor.take(1)?[0];
    if !matches!(status, FUNDED | RELEASED | REFUNDED) {
        return Err(malformed());
    }
    let seed_length = usize::from(u16::from_be_bytes(cursor.array()?));
    let seed = ProgramAccountSeed::new(cursor.take(seed_length)?)?;
    let account = AccountId::new(cursor.array()?)?;
    let asset = AssetId::new(cursor.array()?)?;
    let beneficiary = AccountId::new(cursor.array()?)?;
    let refund = AccountId::new(cursor.array()?)?;
    let amount = Amount::from_be_bytes(cursor.array()?);
    if amount.is_zero() {
        return Err(malformed());
    }
    let release_receipt = ReceiptDigest::new(cursor.array()?)?;
    let refund_receipt = ReceiptDigest::new(cursor.array()?)?;
    cursor.finish()?;
    if release_receipt == refund_receipt {
        return Err(malformed());
    }
    Ok(Record {
        status,
        seed,
        account,
        asset,
        beneficiary,
        refund,
        amount,
        release_receipt,
        refund_receipt,
    })
}

fn store(record: Record<'_>) -> Result<(), ProgramError> {
    let (key, length) = state_key(record.account)?;
    let mut value = [0; RECORD_CAPACITY];
    let written = encode(record, &mut value)?;
    shared::write(
        shared::SharedStorageKey::new(&key[..length])?,
        StorageValue::new(&value[..written])?,
    )
}

fn load(
    account: AccountId,
    value: &mut [u8; RECORD_CAPACITY],
) -> Result<Record<'_>, ProgramError> {
    let (key, length) = state_key(account)?;
    let written = shared::read(shared::SharedStorageKey::new(&key[..length])?, value)?
        .ok_or_else(|| ProgramError::value(Field::StorageValue, Reason::Malformed))?;
    decode(&value[..written])
}

fn emit(
    record: Record<'_>,
    destination: AccountId,
    condition: ReceiptDigest,
) -> Result<(), ProgramError> {
    let mut data = [0; 145];
    data[0] = record.status;
    data[1..33].copy_from_slice(&record.account.bytes());
    data[33..65].copy_from_slice(&record.asset.bytes());
    data[65..97].copy_from_slice(&destination.bytes());
    data[97..113].copy_from_slice(&record.amount.to_be_bytes());
    data[113..145].copy_from_slice(&condition.bytes());
    event::emit(EventTopic::new(TOPIC)?, EventData::new(&data)?)
}

fn open(mut cursor: Cursor<'_>) -> Result<CallResult, ProgramError> {
    let seed_length = usize::from(u16::from_be_bytes(cursor.array()?));
    let seed = ProgramAccountSeed::new(cursor.take(seed_length)?)?;
    let account = AccountId::new(cursor.array()?)?;
    let asset = AssetId::new(cursor.array()?)?;
    let beneficiary = AccountId::new(cursor.array()?)?;
    let refund = AccountId::new(cursor.array()?)?;
    let amount = Amount::from_be_bytes(cursor.array()?);
    let release_receipt = ReceiptDigest::new(cursor.array()?)?;
    let refund_receipt = ReceiptDigest::new(cursor.array()?)?;
    cursor.finish()?;
    let (key, length) = state_key(account)?;
    let mut existing = [0; RECORD_CAPACITY];
    if shared::read(shared::SharedStorageKey::new(&key[..length])?, &mut existing)?.is_some() {
        return Err(ProgramError::value(Field::StorageValue, Reason::Duplicate));
    }
    let deposit = ProgramDeposit::new(seed, account, asset, amount)?;
    transfer::fund_program_account(deposit)?;
    let record = Record {
        status: FUNDED,
        seed,
        account,
        asset,
        beneficiary,
        refund,
        amount,
        release_receipt,
        refund_receipt,
    };
    store(record)?;
    emit(record, account, release_receipt)?;
    Ok(CallResult::OK)
}

fn settle(mut cursor: Cursor<'_>, release: bool) -> Result<CallResult, ProgramError> {
    let account = AccountId::new(cursor.array()?)?;
    let condition = ReceiptDigest::new(cursor.array()?)?;
    cursor.finish()?;
    let mut encoded = [0; RECORD_CAPACITY];
    let mut record = load(account, &mut encoded)?;
    if record.status != FUNDED {
        return Err(malformed());
    }
    let expected = if release {
        record.release_receipt
    } else {
        record.refund_receipt
    };
    let proof = receipt::read(condition)?;
    if condition != expected
        || proof.result_code != 0
        || proof.asset != record.asset.bytes()
        || proof.amount != record.amount
    {
        return Err(malformed());
    }
    let destination = if release {
        record.beneficiary
    } else {
        record.refund
    };
    let payment = ProgramAccountPayment::new(
        record.seed,
        record.account,
        record.asset,
        destination,
        record.amount,
    )?;
    transfer::pay_from_program_account(payment)?;
    record.status = if release { RELEASED } else { REFUNDED };
    store(record)?;
    emit(record, destination, condition)?;
    Ok(CallResult::OK)
}

fn invoke(input: &[u8]) -> Result<CallResult, ProgramError> {
    let mut cursor = Cursor::new(input);
    if cursor.take(1)?[0] != VERSION {
        return Err(malformed());
    }
    match cursor.take(1)?[0] {
        OPEN => open(cursor),
        RELEASE => settle(cursor, true),
        REFUND => settle(cursor, false),
        _ => Err(malformed()),
    }
}

fn legacy(_: i64) -> Result<i64, ProgramError> {
    Err(malformed())
}
layerx_program_sdk::program!(legacy);
layerx_program_sdk::entrypoint!(invoke);
