//! The paid-counter quickstart.
//!
//! It configures a fee, settles a verified 402LXP receipt against that fee,
//! advances a counter, pays the operator and emits the evidence a consumer
//! renders. Every value that crosses the guest boundary is an integer, and
//! every effect is an explicit capability the invoking activity granted.
//!
//! This is the program the programs five-minute benchmark deploys, and the
//! reference the other authoring languages reproduce byte for byte: the same
//! exports, the same storage keys, the same event layouts and the same status
//! numbers.

#![no_std]

use layerx_program_sdk::{
    call, event, receipt, storage, transfer, trap_on_panic, AccountId, Amount, AssetId, CallInput,
    Capability, CapabilitySet, EventData, EventTopic, Field, Payment, ProgramError, ProgramId,
    Reason, ReceiptDigest, StorageKey, StorageValue, STATUS_INVALID,
};

trap_on_panic!();

const ERR_NOT_CONFIGURED: i32 = -64;
const ERR_STATE: i32 = -65;
const ERR_ASSET_MISMATCH: i32 = -66;
const ERR_UNDERPAID: i32 = -67;
const ERR_RECEIPT_FAILED: i32 = -68;
const ERR_COUNTER_OVERFLOW: i32 = -69;

const ENTRY_SELECTOR_COUNT: i64 = 0;
const ENTRY_SELECTOR_RESET: i64 = 1;

const IDENTIFIER_BYTES: usize = 32;
const AMOUNT_BYTES: usize = 16;
const COUNTER_BYTES: usize = 8;
const CONFIGURED_EVENT_BYTES: usize = 80;
const SETTLED_EVENT_BYTES: usize = 56;
const NOTED_EVENT_BYTES: usize = 16;
const FORWARD_CAPABILITY_BYTES: usize = 4;
const FORWARD_INPUT_BYTES: usize = 8;

const KEY_FEE: &[u8] = b"layerx.quickstart.fee";
const KEY_ASSET: &[u8] = b"layerx.quickstart.asset";
const KEY_PAYEE: &[u8] = b"layerx.quickstart.payee";
const KEY_COUNT: &[u8] = b"layerx.quickstart.count";
const TOPIC_CONFIGURED: &[u8] = b"layerx.quickstart.configured";
const TOPIC_SETTLED: &[u8] = b"layerx.quickstart.settled";
const TOPIC_NOTED: &[u8] = b"layerx.quickstart.noted";

fn identifier(word0: i64, word1: i64, word2: i64, word3: i64) -> [u8; IDENTIFIER_BYTES] {
    let mut bytes = [0u8; IDENTIFIER_BYTES];
    bytes[0..8].copy_from_slice(&word0.to_be_bytes());
    bytes[8..16].copy_from_slice(&word1.to_be_bytes());
    bytes[16..24].copy_from_slice(&word2.to_be_bytes());
    bytes[24..32].copy_from_slice(&word3.to_be_bytes());
    bytes
}

fn store(key: &[u8], value: &[u8]) -> Result<(), i32> {
    let key = StorageKey::new(key).map_err(ProgramError::code)?;
    let value = StorageValue::new(value).map_err(ProgramError::code)?;
    storage::write(key, value).map_err(ProgramError::code)
}

fn emit(topic: &[u8], data: &[u8]) -> Result<i32, i32> {
    let topic = EventTopic::new(topic).map_err(ProgramError::code)?;
    let data = EventData::new(data).map_err(ProgramError::code)?;
    event::emit(topic, data).map_err(ProgramError::code)?;
    Ok(0)
}

fn load_exact<const N: usize>(key: &[u8]) -> Result<[u8; N], i32> {
    let key = StorageKey::new(key).map_err(ProgramError::code)?;
    let mut value = [0u8; N];
    let Some(length) = storage::read(key, &mut value).map_err(ProgramError::code)? else {
        return Err(ERR_NOT_CONFIGURED);
    };
    if length != N {
        return Err(ERR_STATE);
    }
    Ok(value)
}

fn load_counter() -> Result<u64, i32> {
    let key = StorageKey::new(KEY_COUNT).map_err(ProgramError::code)?;
    let mut encoded = [0u8; COUNTER_BYTES];
    let Some(length) = storage::read(key, &mut encoded).map_err(ProgramError::code)? else {
        return Ok(0);
    };
    if length != COUNTER_BYTES {
        return Err(ERR_STATE);
    }
    Ok(u64::from_be_bytes(encoded))
}

#[allow(clippy::too_many_arguments)]
fn configure_program(
    fee_high: i64,
    fee_low: i64,
    asset_word0: i64,
    asset_word1: i64,
    asset_word2: i64,
    asset_word3: i64,
    payee_word0: i64,
    payee_word1: i64,
    payee_word2: i64,
    payee_word3: i64,
) -> Result<i32, i32> {
    let fee = Amount::from_words(fee_high, fee_low);
    if fee.is_zero() {
        return Err(ProgramError::value(Field::Amount, Reason::Zero).code());
    }
    let asset = AssetId::new(identifier(
        asset_word0,
        asset_word1,
        asset_word2,
        asset_word3,
    ))
    .map_err(ProgramError::code)?;
    let payee = AccountId::new(identifier(
        payee_word0,
        payee_word1,
        payee_word2,
        payee_word3,
    ))
    .map_err(ProgramError::code)?;
    let mut configured = [0u8; CONFIGURED_EVENT_BYTES];
    configured[0..16].copy_from_slice(&fee.to_be_bytes());
    configured[16..48].copy_from_slice(&asset.bytes());
    configured[48..80].copy_from_slice(&payee.bytes());
    store(KEY_FEE, &configured[0..16])?;
    store(KEY_ASSET, &configured[16..48])?;
    store(KEY_PAYEE, &configured[48..80])?;
    store(KEY_COUNT, &[0u8; COUNTER_BYTES])?;
    emit(TOPIC_CONFIGURED, &configured)
}

fn settle_receipt(
    digest_word0: i64,
    digest_word1: i64,
    digest_word2: i64,
    digest_word3: i64,
) -> Result<i64, i32> {
    let fee = Amount::from_be_bytes(load_exact::<AMOUNT_BYTES>(KEY_FEE)?);
    let asset = load_exact::<IDENTIFIER_BYTES>(KEY_ASSET)?;
    let payee = load_exact::<IDENTIFIER_BYTES>(KEY_PAYEE)?;
    let digest = ReceiptDigest::new(identifier(
        digest_word0,
        digest_word1,
        digest_word2,
        digest_word3,
    ))
    .map_err(ProgramError::code)?;
    let settlement = receipt::read(digest).map_err(ProgramError::code)?;
    if settlement.result_code != 0 {
        return Err(ERR_RECEIPT_FAILED);
    }
    if settlement.asset != asset {
        return Err(ERR_ASSET_MISMATCH);
    }
    if settlement.amount < fee {
        return Err(ERR_UNDERPAID);
    }
    let counter = load_counter()?.checked_add(1).ok_or(ERR_COUNTER_OVERFLOW)?;
    let mut settled = [0u8; SETTLED_EVENT_BYTES];
    settled[0..8].copy_from_slice(&counter.to_be_bytes());
    settled[8..24].copy_from_slice(&settlement.amount.to_be_bytes());
    settled[24..56].copy_from_slice(&digest.bytes());
    store(KEY_COUNT, &settled[0..8])?;
    let asset = AssetId::new(asset).map_err(ProgramError::code)?;
    let payee = AccountId::new(payee).map_err(ProgramError::code)?;
    let payment = Payment::new(asset, payee, fee).map_err(ProgramError::code)?;
    transfer::pay(payment).map_err(ProgramError::code)?;
    emit(TOPIC_SETTLED, &settled)?;
    i64::try_from(counter).map_err(|_| ERR_COUNTER_OVERFLOW)
}

fn forward_note(
    callee_word0: i64,
    callee_word1: i64,
    callee_word2: i64,
    callee_word3: i64,
    note: i64,
) -> Result<i32, i32> {
    let callee = ProgramId::new(identifier(
        callee_word0,
        callee_word1,
        callee_word2,
        callee_word3,
    ))
    .map_err(ProgramError::code)?;
    let mut narrowed = CapabilitySet::<2>::empty();
    narrowed
        .insert(Capability::StorageRead)
        .map_err(ProgramError::code)?;
    narrowed
        .insert(Capability::EmitEvent)
        .map_err(ProgramError::code)?;
    let mut encoded = [0u8; FORWARD_CAPABILITY_BYTES];
    let note = note.to_be_bytes();
    let input = CallInput::new(&note).map_err(ProgramError::code)?;
    call::invoke_with(callee, input, &narrowed, &mut encoded).map_err(ProgramError::code)
}

fn reset_counter() -> Result<i32, i32> {
    let key = StorageKey::new(KEY_COUNT).map_err(ProgramError::code)?;
    storage::delete(key).map_err(ProgramError::code)?;
    Ok(0)
}

fn note_call(input: &[u8]) -> Result<layerx_program_sdk::CallResult, ProgramError> {
    if input.len() != FORWARD_INPUT_BYTES {
        return Err(ProgramError::value(Field::CallInput, Reason::Malformed));
    }
    let counter = load_counter().map_err(typed_status)?;
    let mut noted = [0u8; NOTED_EVENT_BYTES];
    noted[0..8].copy_from_slice(&counter.to_be_bytes());
    noted[8..16].copy_from_slice(input);
    let status = emit(TOPIC_NOTED, &noted).map_err(typed_status)?;
    layerx_program_sdk::CallResult::new(u32::try_from(status).unwrap_or(u32::MAX))
}

fn typed_status(status: i32) -> ProgramError {
    match ProgramError::from_status(status) {
        Err(error) => error,
        Ok(_) => ProgramError::value(Field::CallInput, Reason::Malformed),
    }
}

/// Records the fee, the asset it is denominated in and the account the
/// settled fee is paid to, and zeroes the counter.
#[allow(clippy::too_many_arguments)]
#[allow(unsafe_code)]
#[no_mangle]
pub extern "C" fn configure(
    fee_high: i64,
    fee_low: i64,
    asset_word0: i64,
    asset_word1: i64,
    asset_word2: i64,
    asset_word3: i64,
    payee_word0: i64,
    payee_word1: i64,
    payee_word2: i64,
    payee_word3: i64,
) -> i32 {
    match configure_program(
        fee_high,
        fee_low,
        asset_word0,
        asset_word1,
        asset_word2,
        asset_word3,
        payee_word0,
        payee_word1,
        payee_word2,
        payee_word3,
    ) {
        Ok(status) => status,
        Err(status) => status,
    }
}

/// Returns the number of settlements the program has recorded.
#[allow(unsafe_code)]
#[no_mangle]
pub extern "C" fn count() -> i64 {
    match load_counter() {
        Ok(counter) => i64::try_from(counter).unwrap_or(i64::from(ERR_COUNTER_OVERFLOW)),
        Err(status) => i64::from(status),
    }
}

/// Settles one verified receipt against the configured fee, advances the
/// counter, pays the configured account and files the evidence.
#[allow(unsafe_code)]
#[no_mangle]
pub extern "C" fn settle(
    digest_word0: i64,
    digest_word1: i64,
    digest_word2: i64,
    digest_word3: i64,
) -> i64 {
    match settle_receipt(digest_word0, digest_word1, digest_word2, digest_word3) {
        Ok(counter) => counter,
        Err(status) => i64::from(status),
    }
}

/// Calls another program with a note, handing on only the storage-read and
/// emit-event authority this program already holds.
#[allow(unsafe_code)]
#[no_mangle]
pub extern "C" fn forward(
    callee_word0: i64,
    callee_word1: i64,
    callee_word2: i64,
    callee_word3: i64,
    note: i64,
) -> i32 {
    match forward_note(callee_word0, callee_word1, callee_word2, callee_word3, note) {
        Ok(status) => status,
        Err(status) => status,
    }
}

/// Clears the counter without disturbing the fee configuration.
#[allow(unsafe_code)]
#[no_mangle]
pub extern "C" fn reset() -> i32 {
    match reset_counter() {
        Ok(status) => status,
        Err(status) => status,
    }
}

/// Canonical entrypoint: selector zero reads the counter, selector one
/// clears it.
#[allow(unsafe_code)]
#[no_mangle]
pub extern "C" fn layerx_main(selector: i64) -> i64 {
    match selector {
        ENTRY_SELECTOR_COUNT => count(),
        ENTRY_SELECTOR_RESET => i64::from(reset()),
        _ => i64::from(STATUS_INVALID),
    }
}

layerx_program_sdk::entrypoint!(note_call);
