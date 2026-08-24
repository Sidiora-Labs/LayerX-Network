#![no_std]

use layerx_program_sdk::{
    event, receipt, storage, transfer, trap_on_panic, AccountId, Amount, AssetId,
    CallResult, EventData, EventTopic, Field, Payment, ProgramError, Reason, ReceiptDigest,
    StorageValue,
};
use layerx_program_sdk::storage::shared::SharedStorageKey;

trap_on_panic!();

const ACTION_LIST: u8 = 1;
const ACTION_BUY: u8 = 2;
const IDENTIFIER_BYTES: usize = 32;
const AMOUNT_BYTES: usize = 16;
const LIST_INPUT_BYTES: usize = 1 + IDENTIFIER_BYTES * 3 + AMOUNT_BYTES;
const BUY_INPUT_BYTES: usize = 1 + IDENTIFIER_BYTES * 2;
const LISTING_BYTES: usize = IDENTIFIER_BYTES * 2 + AMOUNT_BYTES;
const KEY_BYTES: usize = 7 + IDENTIFIER_BYTES;
const TOPIC_LISTED: &[u8] = b"layerx.marketplace.listed";
const TOPIC_BOUGHT: &[u8] = b"layerx.marketplace.bought";

fn malformed() -> ProgramError {
    ProgramError::value(Field::CallInput, Reason::Malformed)
}

fn fixed<const N: usize>(bytes: &[u8], offset: usize) -> Result<[u8; N], ProgramError> {
    let end = offset.checked_add(N).ok_or_else(malformed)?;
    let value = bytes.get(offset..end).ok_or_else(malformed)?;
    let mut output = [0u8; N];
    output.copy_from_slice(value);
    Ok(output)
}

fn listing_key(identifier: [u8; IDENTIFIER_BYTES]) -> Result<[u8; KEY_BYTES], ProgramError> {
    let mut key = [0u8; KEY_BYTES];
    key[..7].copy_from_slice(b"listing");
    key[7..].copy_from_slice(&identifier);
    SharedStorageKey::new(&key)?;
    Ok(key)
}

fn receipt_key(digest: [u8; IDENTIFIER_BYTES]) -> Result<[u8; KEY_BYTES], ProgramError> {
    let mut key = [0u8; KEY_BYTES];
    key[..7].copy_from_slice(b"receipt");
    key[7..].copy_from_slice(&digest);
    SharedStorageKey::new(&key)?;
    Ok(key)
}

fn emit(topic: &[u8], data: &[u8]) -> Result<(), ProgramError> {
    event::emit(EventTopic::new(topic)?, EventData::new(data)?)
}

fn list(input: &[u8]) -> Result<CallResult, ProgramError> {
    if input.len() != LIST_INPUT_BYTES {
        return Err(malformed());
    }
    let listing_id = fixed::<IDENTIFIER_BYTES>(input, 1)?;
    if listing_id.iter().all(|byte| *byte == 0) {
        return Err(malformed());
    }
    let asset = fixed::<IDENTIFIER_BYTES>(input, 33)?;
    let seller = fixed::<IDENTIFIER_BYTES>(input, 65)?;
    let price = fixed::<AMOUNT_BYTES>(input, 97)?;
    AssetId::new(asset)?;
    AccountId::new(seller)?;
    let price = Amount::from_be_bytes(price);
    if price.is_zero() {
        return Err(ProgramError::value(Field::Amount, Reason::Zero));
    }
    let key_bytes = listing_key(listing_id)?;
    let key = SharedStorageKey::new(&key_bytes)?;
    let mut existing = [0u8; LISTING_BYTES];
    if storage::shared::read(key, &mut existing)?.is_some() {
        return Err(malformed());
    }
    let mut listing = [0u8; LISTING_BYTES];
    listing[..32].copy_from_slice(&asset);
    listing[32..64].copy_from_slice(&seller);
    listing[64..].copy_from_slice(&price.to_be_bytes());
    storage::shared::write(key, StorageValue::new(&listing)?)?;
    emit(TOPIC_LISTED, input.get(1..).ok_or_else(malformed)?)?;
    CallResult::new(1)
}

fn buy(input: &[u8]) -> Result<CallResult, ProgramError> {
    if input.len() != BUY_INPUT_BYTES {
        return Err(malformed());
    }
    let listing_id = fixed::<IDENTIFIER_BYTES>(input, 1)?;
    if listing_id.iter().all(|byte| *byte == 0) {
        return Err(malformed());
    }
    let digest_bytes = fixed::<IDENTIFIER_BYTES>(input, 33)?;
    let digest = ReceiptDigest::new(digest_bytes)?;
    let key_bytes = listing_key(listing_id)?;
    let key = SharedStorageKey::new(&key_bytes)?;
    let receipt_key_bytes = receipt_key(digest_bytes)?;
    let consumed_key = SharedStorageKey::new(&receipt_key_bytes)?;
    let mut consumed = [0u8; IDENTIFIER_BYTES];
    if storage::shared::read(consumed_key, &mut consumed)?.is_some() {
        return Err(malformed());
    }
    let mut listing = [0u8; LISTING_BYTES];
    let Some(length) = storage::shared::read(key, &mut listing)? else {
        return Err(malformed());
    };
    if length != LISTING_BYTES {
        return Err(ProgramError::value(Field::StorageValue, Reason::Malformed));
    }
    let asset_bytes = fixed::<IDENTIFIER_BYTES>(&listing, 0)?;
    let seller_bytes = fixed::<IDENTIFIER_BYTES>(&listing, 32)?;
    let price = Amount::from_be_bytes(fixed::<AMOUNT_BYTES>(&listing, 64)?);
    let evidence = receipt::read(digest)?;
    if evidence.result_code != 0 || evidence.asset != asset_bytes || evidence.amount < price {
        return Err(ProgramError::value(Field::Receipt, Reason::Malformed));
    }
    transfer::pay(Payment::new(
        AssetId::new(asset_bytes)?,
        AccountId::new(seller_bytes)?,
        price,
    )?)?;
    storage::shared::write(consumed_key, StorageValue::new(&listing_id)?)?;
    storage::shared::delete(key)?;
    emit(TOPIC_BOUGHT, input.get(1..).ok_or_else(malformed)?)?;
    CallResult::new(2)
}

fn marketplace(input: &[u8]) -> Result<CallResult, ProgramError> {
    match input.first().copied() {
        Some(ACTION_LIST) => list(input),
        Some(ACTION_BUY) => buy(input),
        _ => Err(malformed()),
    }
}

layerx_program_sdk::entrypoint!(marketplace);
