#![no_std]

use layerx_program_sdk::{
    event, transfer, trap_on_panic, web, AccountId, Amount, AssetId, CallResult, EventData,
    EventTopic, Field, Payment, ProgramError, Reason, RECORD_BYTES,
};

trap_on_panic!();

const VERSION: u8 = 1;
const REQUEST: u8 = 1;
const READ: u8 = 2;
const FETCH: u8 = 1;
const SEARCH: u8 = 2;
const TOPIC: &[u8] = b"PAXEERX_WEB_REQUEST_V1";
const RECORD_HEADER_BYTES: usize = 13;
const MAX_PAYLOAD_BYTES: usize = 2_048;

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

    fn rest(&mut self) -> &'a [u8] {
        let value = self.bytes.get(self.offset..).unwrap_or_default();
        self.offset = self.bytes.len();
        value
    }
}

fn malformed() -> ProgramError {
    ProgramError::value(Field::CallInput, Reason::Malformed)
}

/// Pays the request fee into the web fee account with `transfer_402` and
/// emits the request record the kernel admits only beside that payment.
fn request(mut cursor: Cursor<'_>) -> Result<CallResult, ProgramError> {
    let request_id = cursor.array::<8>()?;
    let kind = cursor.take(1)?[0];
    let asset = AssetId::new(cursor.array::<32>()?)?;
    let fee_account = AccountId::new(cursor.array::<32>()?)?;
    let amount = Amount::from_be_bytes(cursor.array::<16>()?);
    let payload = cursor.rest();
    if (kind != FETCH && kind != SEARCH) || payload.is_empty() {
        return Err(malformed());
    }
    if payload.len() > MAX_PAYLOAD_BYTES {
        return Err(ProgramError::value(Field::CallInput, Reason::TooLarge));
    }
    let payload_length = u32::try_from(payload.len()).map_err(|_| malformed())?;
    let mut record = [0u8; RECORD_HEADER_BYTES + MAX_PAYLOAD_BYTES];
    let length = RECORD_HEADER_BYTES + payload.len();
    record[..8].copy_from_slice(&request_id);
    record[8] = kind;
    record[9..RECORD_HEADER_BYTES].copy_from_slice(&payload_length.to_be_bytes());
    record[RECORD_HEADER_BYTES..length].copy_from_slice(payload);
    transfer::pay(Payment::new(asset, fee_account, amount)?)?;
    event::emit(
        EventTopic::new(TOPIC)?,
        EventData::new(record.get(..length).ok_or_else(malformed)?)?,
    )?;
    Ok(CallResult::OK)
}

/// Reads the committed answer through `web_read` and refuses unless it is
/// present and equal to the expected digest, full length and response.
fn read(mut cursor: Cursor<'_>) -> Result<CallResult, ProgramError> {
    let request_id = u64::from_be_bytes(cursor.array::<8>()?);
    let digest = cursor.array::<32>()?;
    let full_length = u32::from_be_bytes(cursor.array::<4>()?);
    let response = cursor.rest();
    let mut buffer = [0u8; RECORD_BYTES];
    let answer = web::read(request_id, &mut buffer)?
        .ok_or(ProgramError::value(Field::Buffer, Reason::Empty))?;
    if answer.content_digest != digest
        || answer.full_length != full_length
        || answer.response != response
    {
        return Err(ProgramError::value(Field::Buffer, Reason::Malformed));
    }
    Ok(CallResult::OK)
}

fn invoke(input: &[u8]) -> Result<CallResult, ProgramError> {
    let mut cursor = Cursor::new(input);
    if cursor.take(1)?[0] != VERSION {
        return Err(malformed());
    }
    match cursor.take(1)?[0] {
        REQUEST => request(cursor),
        READ => read(cursor),
        _ => Err(malformed()),
    }
}

fn legacy(_: i64) -> Result<i64, ProgramError> {
    Err(malformed())
}
layerx_program_sdk::program!(legacy);
layerx_program_sdk::entrypoint!(invoke);
