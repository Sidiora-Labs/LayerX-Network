mod native_call;

use std::collections::{HashMap, VecDeque};
use std::ffi::{c_char, c_int, c_uchar, c_uint, c_ulonglong, c_void, CStr};
use std::fmt::Write as _;
use std::io::{Read, Write};
use std::net::{SocketAddr, TcpListener, TcpStream};
use std::ptr;
use std::slice;
use std::sync::{
    mpsc::{sync_channel, SyncSender, TrySendError},
    Arc, Mutex,
};
use std::thread;
use std::time::Duration;

use ed25519_dalek::{Signer as _, SigningKey};
use layerx_proof::program::{
    verify_program_execution, ProgramExecutionExpectation, VerifiedProgramExecution,
};
use layerx_proof::receipt::{verify as verify_receipt, AuthorizedBatch};
use layerx_types::activity::{Authority, EnvelopeBuilder, Signature, TimestampBound};
use layerx_types::amount::Amount;
use layerx_types::ids::{Did, IdempotencyKey};
use layerx_types::intent::{
    CallBudget, Calldata, CapabilityRequest, ProgramCall, ProgramCallFailure, ProgramCallOutcome,
    ProgramId, ProgramLegacyValue, RequestedCapabilities,
};
use layerx_types::payload::{ActivityType, ModuleId, ModuleRegistration, ModuleRegistry, Payload};
use layerx_wire::activity::{decode_signed, encode_signed_envelope, encode_unsigned_envelope};
use layerx_wire::encode::Encoder;
use layerx_wire::hash::{activity_id as derive_activity_id, Domain};
use serde::de::DeserializeOwned;
use serde::Deserialize;
use sha2::{Digest as _, Sha256};

const DEFAULT_PORT: u16 = 9402;
const DEFAULT_NETWORK_ID: u32 = 402;
const DEFAULT_TIME_MS: u64 = 1_700_000_000_000;
const MAX_HEADER_BYTES: usize = 32 * 1024;
const MAX_REQUEST_BYTES: usize = 64 * 1024 * 1024;
const MAX_JSON_BYTES: usize = 8 * 1024 * 1024;
const IO_TIMEOUT: Duration = Duration::from_secs(10);
const PARSER_WORKERS: usize = 8;
const ADMISSION_CAPACITY: usize = 32;
const TRANSITION_CAPACITY: usize = 32;
const MAX_RECEIPTS: usize = 4096;
const MAX_RECEIPT_BYTES: usize = 1024 * 1024;
const MAX_CORE_SNAPSHOT_BYTES: usize = 24 * 1024 * 1024;
const MAX_PROGRAM_OPERATIONS: usize = 4096;
const MAX_MOVE_QUOTES: usize = 4096;
const MAX_MOVE_OPERATIONS: usize = 4096;
const MAX_EMULATOR_ACCOUNTS: usize = 512;
const MAX_PROGRAM_RESPONSE_BYTES: usize = MAX_JSON_BYTES;
const MAX_RETAINED_SIGNED_ACTIVITY_BYTES: usize = 1024 * 1024;
const RECOVERY_SNAPSHOT_MAGIC: &[u8; 8] = b"LXEMR001";
const RECOVERY_SNAPSHOT_VERSION: u32 = 4;
const RECOVERY_SNAPSHOT_DIGEST_DOMAIN: &[u8] = b"layerx-emulator-recovery-snapshot-v1\0";
const MOVE_QUOTE_DOMAIN: &[u8] = b"layerx-emulator-move-quote-v1\0";
const MOVE_IDEMPOTENCY_DOMAIN: &[u8] = b"layerx-emulator-move-idempotency-v1\0";
const MOVE_JOURNEY_DOMAIN: &[u8] = b"layerx-emulator-move-journey-v1\0";
const MOVE_STAGE_DOMAIN: &[u8] = b"layerx-emulator-move-stage-v1\0";
const MOVE_QUOTE_WINDOW_MS: u64 = 300_000;
const ASSET_SEND_OPERATION: u16 = 5;
const ASSET_SEND_TAG: u16 = 0x5301;
const ASSET_SEND_FIELDS: u16 = 10;
const NATIVE_ASSET: [u8; 32] = [
    1, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
];

#[repr(C)]
struct CoreReceipt {
    activity_id: [u8; 32],
    batch_id: [u8; 32],
    state_root: [u8; 32],
    previous_state_root: [u8; 32],
    asset: [u8; 32],
    sequencer_public_key: [u8; 32],
    global_sequence: c_ulonglong,
    result_code: c_int,
    metered_cost_hi: c_ulonglong,
    metered_cost_lo: c_ulonglong,
    bytes: *const c_uchar,
    length: usize,
    terminal_payload: *const c_uchar,
    terminal_payload_length: usize,
    call_graph: *const c_uchar,
    call_graph_length: usize,
    isolated_owner: *mut c_void,
}

#[repr(C)]
struct CoreState {
    canonical_state_root: [u8; 32],
    receipt_state_root: [u8; 32],
    next_sequence: c_ulonglong,
    batch_number: c_ulonglong,
    timestamp_ms: c_ulonglong,
    cell_count: usize,
    account_count: usize,
}

#[repr(C)]
struct CoreProgram {
    program_id: [u8; 32],
    code_hash: [u8; 32],
    deployment_receipt_digest: [u8; 32],
    version: c_uint,
    abi_version: u16,
    lifecycle: u8,
    interface_bytes: *const c_uchar,
    interface_length: usize,
    has_interface: u8,
    state_root: [u8; 32],
    observed_sequence: c_ulonglong,
}

unsafe extern "C" {
    fn platform_emulator_create(
        network_id: c_uint,
        timestamp_ms: c_ulonglong,
        sequencer_seed: *const c_uchar,
    ) -> *mut c_void;
    fn platform_emulator_destroy(emulator: *mut c_void);
    fn platform_emulator_error_name(result: c_int) -> *const c_char;
    fn platform_emulator_set_time(emulator: *mut c_void, timestamp_ms: c_ulonglong) -> c_int;
    fn platform_emulator_advance_time(emulator: *mut c_void, delta_ms: c_ulonglong) -> c_int;
    fn platform_emulator_inject_failure(
        emulator: *mut c_void,
        kind: c_uint,
        count: c_ulonglong,
    ) -> c_int;
    fn platform_emulator_prefund(
        emulator: *mut c_void,
        did: *const c_uchar,
        did_length: usize,
        public_key: *const c_uchar,
        amount_hi: c_ulonglong,
        amount_lo: c_ulonglong,
    ) -> c_int;
    fn platform_emulator_execute(
        emulator: *mut c_void,
        activity: *const c_uchar,
        length: usize,
        receipt: *mut CoreReceipt,
    ) -> c_int;
    fn platform_emulator_simulate(
        emulator: *mut c_void,
        activity: *const c_uchar,
        length: usize,
        receipt: *mut CoreReceipt,
    ) -> c_int;
    fn platform_emulator_receipt_release(receipt: *mut CoreReceipt);
    fn platform_emulator_inspect(emulator: *const c_void, state: *mut CoreState) -> c_int;
    fn platform_emulator_program_read(
        emulator: *mut c_void,
        program_id: *const c_uchar,
        program: *mut CoreProgram,
    ) -> c_int;
    fn platform_emulator_cell(
        emulator: *const c_void,
        index: usize,
        key: *mut c_uchar,
        value_hi: *mut c_ulonglong,
        value_lo: *mut c_ulonglong,
    ) -> c_int;
    fn platform_emulator_account(
        emulator: *const c_void,
        index: usize,
        id: *mut c_uchar,
        name: *mut *const c_uchar,
        name_length: *mut usize,
        balance_hi: *mut c_ulonglong,
        balance_lo: *mut c_ulonglong,
        next_sequence: *mut c_ulonglong,
    ) -> c_int;
    fn platform_emulator_identity_sequence(
        emulator: *const c_void,
        did: *const c_uchar,
        did_length: usize,
        next_sequence: *mut c_ulonglong,
    ) -> c_int;
    fn platform_emulator_snapshot_export(
        emulator: *mut c_void,
        bytes: *mut *const c_uchar,
        length: *mut usize,
    ) -> c_int;
    fn platform_emulator_snapshot_import(
        emulator: *mut c_void,
        bytes: *const c_uchar,
        length: usize,
    ) -> c_int;
}

struct Emulator {
    core: *mut c_void,
    signing_key: SigningKey,
    network_id: u32,
    receipts: HashMap<String, String>,
    receipt_order: VecDeque<String>,
    program_operations: HashMap<String, ProgramOperation>,
    program_activity_operations: HashMap<String, String>,
    accounts: HashMap<String, EmulatorAccount>,
    move_quotes: HashMap<String, MoveQuoteRecord>,
    move_operations: HashMap<String, MoveOperation>,
    trace: u64,
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct ProgramOperation {
    activity_id: String,
    response: String,
    retained_signed_activity: Option<String>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct EmulatorAccount {
    id: [u8; 32],
    did: String,
    public_key: [u8; 32],
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct MoveQuoteRecord {
    quote_id: String,
    source: String,
    destination: String,
    source_id: [u8; 32],
    destination_id: [u8; 32],
    amount: u128,
    currency: String,
    created_at: u64,
    expires_at: u64,
    identity_sequence: u64,
    source_sequence: u64,
    committed_idempotency: Option<String>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct MoveOperation {
    quote_id: String,
    status: u16,
    body: String,
}

struct CoreAccountView {
    id: [u8; 32],
    balance: u128,
    next_sequence: u64,
}

struct DecodedProgramActivity {
    signed: Vec<u8>,
    program_id: [u8; 32],
    protocol_version: u16,
    activity_id: [u8; 32],
    idempotency_key: [u8; 32],
}

struct OwnedCoreReceipt {
    activity_id: [u8; 32],
    receipt: Vec<u8>,
    terminal_payload: Vec<u8>,
    call_graph: Vec<u8>,
}

impl Drop for Emulator {
    fn drop(&mut self) {
        unsafe { platform_emulator_destroy(self.core) };
    }
}

struct Config {
    listen: SocketAddr,
    network_id: u32,
    timestamp_ms: u64,
    prefunds: Vec<Prefund>,
    sequencer_seed: Option<[u8; 32]>,
}

struct Prefund {
    did: String,
    public_key: [u8; 32],
    amount_hi: u64,
    amount_lo: u64,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ActivityBody {
    activity: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ProgramCallBudgetBody {
    fuel: String,
    fee_limit: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ProgramCallBody {
    program_id: String,
    calldata: String,
    budget: ProgramCallBudgetBody,
    capabilities: Vec<String>,
    signed_activity: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ProgramSelectorBody {
    program_id: String,
    requested_verification_level: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ProgramReceiptSelectorBody {
    idempotency_key: String,
    expected_activity_id: String,
    requested_verification_level: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ProgramActivitySelectorBody {
    activity_id: String,
    requested_verification_level: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct PrefundBody {
    did: String,
    public_key: String,
    #[serde(default)]
    amount_hi: u64,
    amount_lo: u64,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct MoveMoneyBody {
    currency: String,
    amount: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct MoveQuoteBody {
    source: String,
    destination: String,
    money: MoveMoneyBody,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct MoveCommitBody {
    quote_id: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct TimestampBody {
    timestamp_ms: u64,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct AdvanceBody {
    delta_ms: u64,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct FaultBody {
    kind: String,
    #[serde(default = "one")]
    count: u64,
}

const fn one() -> u64 {
    1
}

struct Request {
    method: String,
    path: String,
    content_type: String,
    idempotency_key: Option<String>,
    body: Vec<u8>,
}

struct Response {
    status: u16,
    content_type: &'static str,
    body: Vec<u8>,
}

struct ParsedConnection {
    request: Result<Request, String>,
    response: SyncSender<Response>,
}

fn hex_encode(bytes: &[u8]) -> String {
    const DIGITS: &[u8; 16] = b"0123456789abcdef";
    let mut encoded = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        encoded.push(char::from(DIGITS[usize::from(byte >> 4)]));
        encoded.push(char::from(DIGITS[usize::from(byte & 0x0f)]));
    }
    encoded
}

fn nibble(value: u8) -> Option<u8> {
    match value {
        b'0'..=b'9' => Some(value - b'0'),
        b'a'..=b'f' => Some(value - b'a' + 10),
        b'A'..=b'F' => Some(value - b'A' + 10),
        _ => None,
    }
}

fn hex_decode(encoded: &str) -> Result<Vec<u8>, String> {
    let bytes = encoded.as_bytes();
    if !bytes.len().is_multiple_of(2) {
        return Err("hex input has an odd length".into());
    }
    bytes
        .chunks_exact(2)
        .map(|pair| {
            let high = nibble(pair[0]).ok_or_else(|| "invalid hex input".to_string())?;
            let low = nibble(pair[1]).ok_or_else(|| "invalid hex input".to_string())?;
            Ok((high << 4) | low)
        })
        .collect()
}

fn hash_bytes(domain: &[u8], bytes: &[u8]) -> [u8; 32] {
    let mut digest = Sha256::new();
    digest.update(domain);
    digest.update(bytes);
    digest.finalize().into()
}

fn protocol_hash(domain: Domain, bytes: &[u8]) -> [u8; 32] {
    hash_bytes(domain.tag(), bytes)
}

fn valid_human_idempotency(value: &str) -> bool {
    (16..=128).contains(&value.len())
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_'))
}

fn account_did(account: &str) -> Option<&str> {
    account
        .strip_prefix("agent:")
        .and_then(|value| value.strip_suffix(":main"))
        .filter(|value| !value.is_empty())
}

fn timestamp_text(timestamp_ms: u64) -> Result<String, String> {
    if timestamp_ms > 253_402_300_799_999 {
        return Err("emulator timestamp is outside the RFC 3339 range".to_owned());
    }
    let seconds = timestamp_ms / 1_000;
    let days = i64::try_from(seconds / 86_400)
        .map_err(|_| "emulator timestamp day count is invalid".to_owned())?;
    let second_of_day = seconds % 86_400;
    let z = days + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let day_of_era = z - era * 146_097;
    let year_of_era =
        (day_of_era - day_of_era / 1_460 + day_of_era / 36_524 - day_of_era / 146_096) / 365;
    let mut year = year_of_era + era * 400;
    let day_of_year = day_of_era - (365 * year_of_era + year_of_era / 4 - year_of_era / 100);
    let month_prime = (5 * day_of_year + 2) / 153;
    let day = day_of_year - (153 * month_prime + 2) / 5 + 1;
    let month = month_prime + if month_prime < 10 { 3 } else { -9 };
    year += i64::from(month <= 2);
    let hour = second_of_day / 3_600;
    let minute = (second_of_day % 3_600) / 60;
    let second = second_of_day % 60;
    Ok(format!(
        "{year:04}-{month:02}-{day:02}T{hour:02}:{minute:02}:{second:02}.{:03}Z",
        timestamp_ms % 1_000
    ))
}

fn core_account(emulator: &Emulator, expected_name: &str) -> Result<Option<CoreAccountView>, i32> {
    let state = inspect_state(emulator)?;
    for index in 0..state.account_count {
        let mut id = [0_u8; 32];
        let mut name = ptr::null();
        let mut name_length = 0_usize;
        let mut balance_hi = 0_u64;
        let mut balance_lo = 0_u64;
        let mut next_sequence = 0_u64;
        let code = unsafe {
            platform_emulator_account(
                emulator.core,
                index,
                id.as_mut_ptr(),
                &raw mut name,
                &raw mut name_length,
                &raw mut balance_hi,
                &raw mut balance_lo,
                &raw mut next_sequence,
            )
        };
        if code != 0 {
            return Err(code);
        }
        if name.is_null() || name_length == 0 {
            return Err(-3);
        }
        let name = unsafe { slice::from_raw_parts(name, name_length) };
        if name == expected_name.as_bytes() {
            return Ok(Some(CoreAccountView {
                id,
                balance: (u128::from(balance_hi) << 64) | u128::from(balance_lo),
                next_sequence,
            }));
        }
    }
    Ok(None)
}

fn core_identity_sequence(emulator: &Emulator, did: &str) -> Result<u64, i32> {
    let mut next_sequence = 0_u64;
    let code = unsafe {
        platform_emulator_identity_sequence(
            emulator.core,
            did.as_ptr(),
            did.len(),
            &raw mut next_sequence,
        )
    };
    if code == 0 {
        Ok(next_sequence)
    } else {
        Err(code)
    }
}

fn asset_send_registry() -> Result<(ActivityType, ModuleRegistry), String> {
    let activity_type = ActivityType::new(ModuleId::Asset, ASSET_SEND_OPERATION)
        .map_err(|_| "Asset SEND activity type is unavailable".to_owned())?;
    let registration = ModuleRegistration::new(ModuleId::Asset, &[activity_type])
        .map_err(|_| "Asset module registration is unavailable".to_owned())?;
    let registry = ModuleRegistry::new(&[registration])
        .map_err(|_| "Asset module registry is unavailable".to_owned())?;
    Ok((activity_type, registry))
}

fn move_idempotency(value: &str) -> [u8; 32] {
    hash_bytes(MOVE_IDEMPOTENCY_DOMAIN, value.as_bytes())
}

fn move_context(
    source: &[u8; 32],
    destination: &[u8; 32],
    amount: u128,
    idempotency: &[u8; 32],
) -> [u8; 32] {
    let mut bytes = Vec::with_capacity(144);
    bytes.extend_from_slice(source);
    bytes.extend_from_slice(destination);
    bytes.extend_from_slice(&NATIVE_ASSET);
    bytes.extend_from_slice(&amount.to_be_bytes());
    bytes.extend_from_slice(idempotency);
    protocol_hash(Domain::ContextHash, &bytes)
}

fn move_send_payload(
    signing_key: &SigningKey,
    quote: &MoveQuoteRecord,
    idempotency: [u8; 32],
    network_id: u32,
) -> Result<(Vec<u8>, [u8; 32], [u8; 32]), String> {
    let public_key = signing_key.verifying_key().to_bytes();
    let context = move_context(
        &quote.source_id,
        &quote.destination_id,
        quote.amount,
        &idempotency,
    );
    let mut authorization = Encoder::new(512);
    authorization
        .u16(ASSET_SEND_TAG)
        .and_then(|()| authorization.fixed(&quote.source_id))
        .and_then(|()| authorization.fixed(&quote.destination_id))
        .and_then(|()| authorization.fixed(&NATIVE_ASSET))
        .and_then(|()| authorization.u128(quote.amount))
        .and_then(|()| authorization.u64(quote.source_sequence))
        .and_then(|()| authorization.fixed(&idempotency))
        .and_then(|()| authorization.u64(quote.expires_at))
        .and_then(|()| authorization.fixed(&context))
        .and_then(|()| authorization.u8(0))
        .and_then(|()| authorization.u8(1))
        .and_then(|()| authorization.fixed(&quote.source_id))
        .and_then(|()| authorization.fixed(&context))
        .and_then(|()| authorization.u32(network_id))
        .and_then(|()| authorization.u16(layerx_wire::limits::PROTOCOL_VERSION))
        .map_err(|_| "move authorization exceeds its canonical bound".to_owned())?;
    let authorization_hash = protocol_hash(Domain::SignaturePreimage, &authorization.finish());
    let authorization_signature = signing_key.sign(&authorization_hash).to_bytes();
    let mut payload = Encoder::new(512);
    payload
        .u16(ASSET_SEND_TAG)
        .and_then(|()| payload.u16(ASSET_SEND_FIELDS))
        .and_then(|()| payload.fixed(&quote.source_id))
        .and_then(|()| payload.fixed(&quote.destination_id))
        .and_then(|()| payload.fixed(&NATIVE_ASSET))
        .and_then(|()| payload.u128(quote.amount))
        .and_then(|()| payload.u64(quote.source_sequence))
        .and_then(|()| payload.fixed(&idempotency))
        .and_then(|()| payload.u64(quote.expires_at))
        .and_then(|()| payload.fixed(&context))
        .and_then(|()| payload.u8(0))
        .and_then(|()| payload.u8(1))
        .and_then(|()| payload.fixed(&quote.source_id))
        .and_then(|()| payload.fixed(&public_key))
        .and_then(|()| payload.fixed(&authorization_signature))
        .and_then(|()| payload.fixed(&context))
        .and_then(|()| payload.u32(network_id))
        .and_then(|()| payload.u16(layerx_wire::limits::PROTOCOL_VERSION))
        .map_err(|_| "move payload exceeds its canonical bound".to_owned())?;
    Ok((payload.finish(), context, authorization_hash))
}

fn signed_move_activity(
    emulator: &Emulator,
    quote: &MoveQuoteRecord,
    idempotency: [u8; 32],
) -> Result<(Vec<u8>, [u8; 32], [u8; 32], [u8; 32]), String> {
    let (activity_type, registry) = asset_send_registry()?;
    let (payload_bytes, context_hash, authorization_hash) = move_send_payload(
        &emulator.signing_key,
        quote,
        idempotency,
        emulator.network_id,
    )?;
    let payload = Payload::new(&registry, activity_type, &payload_bytes)
        .map_err(|_| "move payload is not canonical".to_owned())?;
    let payload_hash = protocol_hash(Domain::PayloadHash, payload.as_bytes());
    let did = account_did(&quote.source)
        .ok_or_else(|| "move source is not a canonical agent main account".to_owned())?;
    let actor = Did::new(did.as_bytes()).map_err(|_| "move source DID is invalid".to_owned())?;
    let authority = Authority::owner(&emulator.signing_key.verifying_key().to_bytes())
        .map_err(|_| "move owner authority is invalid".to_owned())?;
    let timestamps = TimestampBound::new(quote.created_at, quote.expires_at)
        .map_err(|_| "move timestamp bound is invalid".to_owned())?;
    let mut builder = EnvelopeBuilder::new();
    builder
        .protocol_version(layerx_wire::limits::PROTOCOL_VERSION)
        .and_then(|value| value.network_id(emulator.network_id))
        .and_then(|value| value.activity_type(activity_type))
        .and_then(|value| value.actor_did(actor))
        .and_then(|value| value.authority(authority))
        .and_then(|value| value.account_sequence(quote.identity_sequence))
        .and_then(|value| value.timestamp_bound(timestamps))
        .and_then(|value| value.idempotency_key(IdempotencyKey::new(idempotency)))
        .and_then(|value| value.fee_limit(Amount::from_u128(0)))
        .and_then(|value| value.payload_hash(payload_hash))
        .and_then(|value| value.payload(payload))
        .map_err(|_| "move envelope is invalid".to_owned())?;
    let unsigned = builder
        .build()
        .map_err(|_| "move envelope is incomplete".to_owned())?;
    let unsigned_bytes = encode_unsigned_envelope(&unsigned)
        .map_err(|_| "move signing bytes are invalid".to_owned())?;
    let signature = emulator
        .signing_key
        .sign(&protocol_hash(Domain::SignaturePreimage, &unsigned_bytes))
        .to_bytes();
    let signed = unsigned.attach_signature(
        Signature::new(&signature).map_err(|_| "move signature is invalid".to_owned())?,
    );
    let canonical =
        encode_signed_envelope(&signed).map_err(|_| "signed move is invalid".to_owned())?;
    let decoded = decode_signed(&canonical, &registry)
        .map_err(|_| "signed move did not decode canonically".to_owned())?;
    let activity_id =
        derive_activity_id(&decoded).map_err(|_| "signed move identity is invalid".to_owned())?;
    Ok((canonical, activity_id, context_hash, authorization_hash))
}

fn decode_json<T: DeserializeOwned>(request: &Request) -> Result<T, String> {
    let media_type = request
        .content_type
        .split(';')
        .next()
        .unwrap_or_default()
        .trim();
    if media_type != "application/json" || request.body.len() > MAX_JSON_BYTES {
        return Err("request must carry bounded application/json".into());
    }
    serde_json::from_slice(&request.body).map_err(|_| "request body is not valid JSON".into())
}

fn escape_json(value: &str) -> String {
    let mut escaped = String::with_capacity(value.len());
    for character in value.chars() {
        match character {
            '"' => escaped.push_str("\\\""),
            '\\' => escaped.push_str("\\\\"),
            '\n' => escaped.push_str("\\n"),
            '\r' => escaped.push_str("\\r"),
            '\t' => escaped.push_str("\\t"),
            value if value.is_control() => {
                let _ = write!(escaped, "\\u{:04x}", u32::from(value));
            }
            value => escaped.push(value),
        }
    }
    escaped
}

fn remember_receipt(emulator: &mut Emulator, activity_id: String, receipt: String) {
    if !emulator.receipts.contains_key(&activity_id) {
        while emulator.receipts.len() >= MAX_RECEIPTS {
            let Some(evicted) = emulator.receipt_order.pop_front() else {
                emulator.receipts.clear();
                break;
            };
            emulator.receipts.remove(&evicted);
        }
        emulator.receipt_order.push_back(activity_id.clone());
    }
    emulator.receipts.insert(activity_id, receipt);
}

fn core_error(code: i32) -> String {
    unsafe {
        let pointer = platform_emulator_error_name(code);
        if pointer.is_null() {
            return "LXP_ERR_UNKNOWN".into();
        }
        CStr::from_ptr(pointer).to_string_lossy().into_owned()
    }
}

fn success(trace: u64, result: &str) -> Response {
    Response {
        status: 200,
        content_type: "application/json",
        body: format!("{{\"ok\":true,\"result\":{result},\"trace\":\"emu-{trace:016x}\"}}")
            .into_bytes(),
    }
}

fn refusal(trace: u64, status: u16, code: &str, detail: &str) -> Response {
    Response {
        status,
        content_type: "application/json",
        body: format!("{{\"ok\":false,\"error\":{{\"code\":\"{}\",\"retry\":\"never\",\"detail\":\"{}\"}},\"trace\":\"emu-{trace:016x}\"}}", escape_json(code), escape_json(detail)).into_bytes(),
    }
}

fn core_response(trace: u64, code: i32) -> Response {
    let name = core_error(code);
    let status = if code == -904 { 503 } else { 400 };
    refusal(trace, status, &name, "the LayerX core refused the request")
}

fn programs_route(method: &str, path: &str) -> bool {
    match (method, path) {
        ("POST", "/v1/programs/call") | ("POST", "/v1/programs/simulate") => true,
        ("GET", path) if path.starts_with("/v1/programs/registry/") => {
            let tail = &path[22..];
            canonical_hex32_text(tail.strip_suffix("/interface").unwrap_or(tail))
        }
        ("GET", path) if path.starts_with("/v1/programs/receipts/by-idempotency/") => path
            .strip_prefix("/v1/programs/receipts/by-idempotency/")
            .is_some_and(canonical_hex32_text),
        ("GET", path) if path.starts_with("/v1/programs/activities/") => path
            .strip_prefix("/v1/programs/activities/")
            .is_some_and(canonical_hex32_text),
        _ => false,
    }
}

fn programs_request_path(method: &str, path: &str) -> bool {
    programs_route(method, path)
}

fn agent_error_class(status: u16, code: &str) -> &'static str {
    if code.contains("idempotency") {
        "IdempotencyConflict"
    } else if code.contains("quota") {
        "RateLimit"
    } else if code.contains("authorization")
        || code.contains("scope")
        || code.contains("api_key")
        || code.contains("identity")
        || code.contains("not_active")
        || code.contains("refused")
    {
        "PolicyRefusal"
    } else if code.contains("verification")
        || code.contains("unverified")
        || code.contains("component_invalid")
        || code.contains("invalid_output")
        || code.contains("selector_mismatch")
        || code.contains("binding_invalid")
    {
        "VerificationFailure"
    } else if status == 404 || code.contains("absent") || code.contains("unknown_program") {
        "UnavailableCapability"
    } else if code.starts_with("LXP_ERR_") {
        "CoreRejection"
    } else if code.contains("invalid") || code.contains("required") || status == 415 {
        "ProtocolIncompatibility"
    } else if status >= 500 {
        "TransportFailure"
    } else {
        "InternalFault"
    }
}

fn agent_reason(code: &str) -> String {
    let mut reason = String::with_capacity(code.len().min(128));
    for byte in code.bytes().take(128) {
        reason.push(char::from(match byte {
            b'a'..=b'z' | b'0'..=b'9' | b'_' | b'.' => byte,
            b'A'..=b'Z' => byte.to_ascii_lowercase(),
            _ => b'_',
        }));
    }
    if reason.is_empty() {
        "program_request_failed".to_owned()
    } else {
        reason
    }
}

fn program_verification_status(value: &serde_json::Value) -> serde_json::Value {
    match value.get("state").and_then(serde_json::Value::as_str) {
        Some("unknown" | "pending") => serde_json::json!({
            "state": "Unverified",
            "requested": "SequencerSigned",
            "achieved": "Unverified",
            "reason": "receipt_pending",
        }),
        _ if matches!(
            value
                .get("verification")
                .and_then(serde_json::Value::as_str),
            Some(
                "registry-receipt-and-current-head-verified"
                    | "deployment-interface-and-current-head-verified"
            )
        ) =>
        {
            serde_json::json!({
                "state": "Unverified",
                "requested": "SequencerSigned",
                "achieved": "Unverified",
                "reason": "server_side_receipt_verification_only",
            })
        }
        _ => serde_json::json!({
            "state": "Achieved",
            "level": "SequencerSigned",
        }),
    }
}

fn normalize_program_u64s(value: &mut serde_json::Value) -> bool {
    match value {
        serde_json::Value::Object(object) => {
            for (key, item) in object {
                if key == "output_values" {
                    let output_values = item.as_u64().or_else(|| {
                        item.as_str().and_then(|text| {
                            text.parse::<u64>()
                                .ok()
                                .filter(|number| text == number.to_string())
                        })
                    });
                    let Some(output_values) =
                        output_values.and_then(|number| u32::try_from(number).ok())
                    else {
                        return false;
                    };
                    *item = serde_json::json!(output_values);
                } else if matches!(
                    key.as_str(),
                    "global_sequence"
                        | "observed_sequence"
                        | "observed_at"
                        | "valid_through"
                        | "cpu_fuel"
                        | "memory_bytes"
                        | "storage_read_bytes"
                        | "storage_write_bytes"
                        | "output_bytes"
                ) {
                    if let Some(number) = item.as_u64() {
                        *item = serde_json::Value::String(number.to_string());
                    } else if item
                        .as_str()
                        .and_then(|text| text.parse::<u64>().ok().map(|number| (text, number)))
                        .is_none_or(|(text, number)| text != number.to_string())
                    {
                        return false;
                    }
                } else if !normalize_program_u64s(item) {
                    return false;
                }
            }
        }
        serde_json::Value::Array(values) => {
            for item in values {
                if !normalize_program_u64s(item) {
                    return false;
                }
            }
        }
        _ => {}
    }
    true
}

fn agent_response(trace: u64, response: Response) -> Response {
    let Response {
        status,
        content_type: _,
        body,
    } = response;
    let request_id = format!("emu-{trace:016x}");
    let document = serde_json::from_slice::<serde_json::Value>(&body).ok();
    let body = if (200..300).contains(&status) {
        document
            .as_ref()
            .and_then(|value| value.get("value").or_else(|| value.get("result")))
            .cloned()
            .map_or_else(
                || {
                    serde_json::json!({
                        "class": "InternalFault",
                        "protocol_result_code": null,
                        "retriability": "Retriable",
                        "request_id": request_id.as_str(),
                        "reason": "invalid_program_success",
                    })
                },
                |mut value| {
                    if normalize_program_u64s(&mut value) {
                        let verification_status = program_verification_status(&value);
                        serde_json::json!({
                            "request_id": request_id.as_str(),
                            "value": value,
                            "verification_status": verification_status,
                        })
                    } else {
                        serde_json::json!({
                            "class": "InternalFault",
                            "protocol_result_code": null,
                            "retriability": "Retriable",
                            "request_id": request_id.as_str(),
                            "reason": "invalid_program_u64",
                        })
                    }
                },
            )
    } else {
        let error = document.as_ref().and_then(|value| value.get("error"));
        let code = error
            .and_then(|value| value.get("code"))
            .and_then(serde_json::Value::as_str)
            .filter(|value| !value.is_empty())
            .unwrap_or("program_request_failed");
        let reason = agent_reason(code);
        let protocol_result_code = error
            .and_then(|value| value.get("protocol_result_code"))
            .filter(|value| {
                value
                    .as_i64()
                    .and_then(|number| i32::try_from(number).ok())
                    .is_some()
            })
            .cloned()
            .unwrap_or(serde_json::Value::Null);
        serde_json::json!({
            "class": agent_error_class(status, code),
            "protocol_result_code": protocol_result_code,
            "retriability": if status == 429 || status >= 500 { "Retriable" } else { "Terminal" },
            "request_id": request_id.as_str(),
            "reason": reason,
        })
    };
    Response {
        status: if (200..300).contains(&status) && body.get("class").is_some() {
            500
        } else {
            status
        },
        content_type: "application/json",
        body: body.to_string().into_bytes(),
    }
}

fn canonical_hex32_text(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

fn program_selector(request: &Request, expected_program: &str) -> Result<(), String> {
    if request.body.is_empty() || request.body.len() > 1024 {
        return Err("program selector must carry bounded canonical JSON".to_owned());
    }
    let selector = decode_json::<ProgramSelectorBody>(request)?;
    if selector.program_id != expected_program
        || !canonical_hex32_text(&selector.program_id)
        || selector.requested_verification_level != "sequencer-signed"
    {
        return Err("program selector does not match the route and verification level".to_owned());
    }
    Ok(())
}

fn program_receipt_selector(
    request: &Request,
    expected_idempotency: &str,
) -> Result<String, String> {
    if request.body.is_empty() || request.body.len() > 1024 {
        return Err("program receipt selector must carry bounded canonical JSON".to_owned());
    }
    let selector = decode_json::<ProgramReceiptSelectorBody>(request)?;
    if selector.idempotency_key != expected_idempotency
        || !canonical_hex32_text(&selector.idempotency_key)
        || !canonical_hex32_text(&selector.expected_activity_id)
        || selector.requested_verification_level != "sequencer-signed"
    {
        return Err(
            "program receipt selector does not match the route and verification level".to_owned(),
        );
    }
    Ok(selector.expected_activity_id)
}

fn program_activity_selector(request: &Request, expected_activity: &str) -> Result<(), String> {
    if request.body.is_empty() || request.body.len() > 1024 {
        return Err("program activity selector must carry bounded canonical JSON".to_owned());
    }
    let selector = decode_json::<ProgramActivitySelectorBody>(request)?;
    if selector.activity_id != expected_activity
        || !canonical_hex32_text(&selector.activity_id)
        || selector.requested_verification_level != "sequencer-signed"
    {
        return Err(
            "program activity selector does not match the route and verification level".to_owned(),
        );
    }
    Ok(())
}

fn parse_request(stream: &mut TcpStream) -> Result<Request, String> {
    let mut bytes = Vec::with_capacity(4096);
    let mut chunk = [0_u8; 4096];
    let header_end = loop {
        let read = stream.read(&mut chunk).map_err(|error| error.to_string())?;
        if read == 0 {
            return Err("connection closed before request headers".into());
        }
        bytes.extend_from_slice(&chunk[..read]);
        if let Some(position) = bytes.windows(4).position(|window| window == b"\r\n\r\n") {
            if position.saturating_add(4) > MAX_HEADER_BYTES {
                return Err("request headers exceed emulator limit".into());
            }
            break position + 4;
        }
        if bytes.len() > MAX_HEADER_BYTES {
            return Err("request headers exceed emulator limit".into());
        }
    };
    let headers = std::str::from_utf8(&bytes[..header_end]).map_err(|_| "headers are not UTF-8")?;
    let mut lines = headers.split("\r\n");
    let request_line = lines.next().ok_or("missing request line")?;
    let mut request_fields = request_line.split(' ');
    let method = request_fields.next().unwrap_or_default();
    let target = request_fields.next().unwrap_or_default();
    if method.is_empty()
        || !method.bytes().all(|byte| byte.is_ascii_uppercase())
        || request_fields.next() != Some("HTTP/1.1")
        || request_fields.next().is_some()
        || !target.starts_with('/')
        || target.starts_with("//")
        || target.contains(['?', '#', '\\', '\0'])
        || target
            .split('/')
            .any(|segment| matches!(segment, "." | ".."))
    {
        return Err("invalid request line".into());
    }
    let path = target.to_string();
    let mut content_length = 0_usize;
    let mut has_content_length = false;
    let mut content_type = String::new();
    let mut has_content_type = false;
    let mut has_host = false;
    let mut idempotency_key = None;
    for line in lines.filter(|line| !line.is_empty()) {
        let (name, value) = line
            .split_once(':')
            .ok_or_else(|| "invalid request header".to_string())?;
        if name.is_empty()
            || !name
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-')
        {
            return Err("invalid request header name".into());
        }
        let value = value.trim_matches([' ', '\t']);
        if value
            .bytes()
            .any(|byte| byte.is_ascii_control() && byte != b'\t')
        {
            return Err("invalid request header value".into());
        }
        if name.eq_ignore_ascii_case("content-length") {
            if has_content_length
                || value.is_empty()
                || !value.bytes().all(|byte| byte.is_ascii_digit())
            {
                return Err("duplicate content length".into());
            }
            content_length = value.parse().map_err(|_| "invalid content length")?;
            has_content_length = true;
        } else if name.eq_ignore_ascii_case("content-type") {
            if has_content_type {
                return Err("duplicate content type".into());
            }
            content_type = value.to_ascii_lowercase();
            has_content_type = true;
        } else if name.eq_ignore_ascii_case("host") {
            if has_host || value.is_empty() {
                return Err("invalid host header".into());
            }
            has_host = true;
        } else if name.eq_ignore_ascii_case("idempotency-key") {
            if idempotency_key.is_some()
                || if target == "/v1/programs/call" {
                    value.is_empty() || value.len() > 128
                } else if target == "/v1/moves" {
                    !valid_human_idempotency(value)
                } else {
                    value.len() != 64 || !value.bytes().all(|byte| byte.is_ascii_hexdigit())
                }
            {
                return Err("invalid idempotency key".into());
            }
            idempotency_key = Some(if matches!(target, "/v1/programs/call" | "/v1/moves") {
                value.to_owned()
            } else {
                value.to_ascii_lowercase()
            });
        } else if name.eq_ignore_ascii_case("transfer-encoding") {
            return Err("transfer encoding is not supported".into());
        }
    }
    if !has_host {
        return Err("missing host header".into());
    }
    if matches!(method, "POST" | "PUT") && !has_content_length {
        return Err("write request is missing content length".into());
    }
    let programs_get = method == "GET"
        && (target.starts_with("/v1/programs/registry/")
            || target.starts_with("/v1/programs/receipts/by-idempotency/")
            || target.starts_with("/v1/programs/activities/"));
    if !matches!(method, "POST" | "PUT") && !programs_get && content_length != 0 {
        return Err("read request may not carry a body".into());
    }
    if content_length > MAX_REQUEST_BYTES {
        return Err("request body exceeds emulator limit".into());
    }
    let method = method.to_owned();
    while bytes.len() - header_end < content_length {
        let read = stream.read(&mut chunk).map_err(|error| error.to_string())?;
        if read == 0 {
            return Err("connection closed before request body".into());
        }
        bytes.extend_from_slice(&chunk[..read]);
        if bytes.len() > header_end.saturating_add(content_length) {
            return Err("request carries bytes beyond its declared body".into());
        }
    }
    if bytes.len() != header_end + content_length {
        return Err("request carries bytes beyond its declared body".into());
    }
    Ok(Request {
        method,
        path,
        content_type,
        idempotency_key,
        body: bytes[header_end..header_end + content_length].to_vec(),
    })
}

fn write_response(stream: &mut TcpStream, response: &Response) -> std::io::Result<()> {
    let reason = match response.status {
        200 => "OK",
        202 => "Accepted",
        400 => "Bad Request",
        404 => "Not Found",
        405 => "Method Not Allowed",
        409 => "Conflict",
        413 => "Content Too Large",
        415 => "Unsupported Media Type",
        503 => "Service Unavailable",
        _ => "Error",
    };
    write!(stream, "HTTP/1.1 {} {}\r\nContent-Type: {}\r\nContent-Length: {}\r\nCache-Control: no-store\r\nConnection: close\r\n\r\n", response.status, reason, response.content_type, response.body.len())?;
    stream.write_all(&response.body)
}

fn submit(emulator: &mut Emulator, request: &Request, trace: u64) -> Response {
    let media_type = request
        .content_type
        .split(';')
        .next()
        .unwrap_or_default()
        .trim();
    let activity = if media_type == "application/octet-stream" {
        Ok(request.body.clone())
    } else if media_type == "application/json" {
        decode_json::<ActivityBody>(request).and_then(|value| hex_decode(&value.activity))
    } else {
        Err("activity content type is not supported".into())
    };
    let activity = match activity {
        Ok(activity) if !activity.is_empty() => activity,
        Ok(_) => return refusal(trace, 400, "invalid_argument", "activity must not be empty"),
        Err(error) => return refusal(trace, 400, "invalid_argument", &error),
    };
    let mut receipt = CoreReceipt {
        activity_id: [0; 32],
        batch_id: [0; 32],
        state_root: [0; 32],
        previous_state_root: [0; 32],
        asset: [0; 32],
        sequencer_public_key: [0; 32],
        global_sequence: 0,
        result_code: 0,
        metered_cost_hi: 0,
        metered_cost_lo: 0,
        bytes: ptr::null(),
        length: 0,
        terminal_payload: ptr::null(),
        terminal_payload_length: 0,
        call_graph: ptr::null(),
        call_graph_length: 0,
        isolated_owner: ptr::null_mut(),
    };
    let code = unsafe {
        platform_emulator_execute(
            emulator.core,
            activity.as_ptr(),
            activity.len(),
            &raw mut receipt,
        )
    };
    if code != 0 {
        return core_response(trace, code);
    }
    let batch_id = receipt.batch_id;
    let global_sequence = receipt.global_sequence;
    let result_code = receipt.result_code;
    let state_root = receipt.state_root;
    let material = match take_core_receipt(&mut receipt) {
        Ok(material) => material,
        Err(error) => return refusal(trace, 503, "core_invalid_output", &error),
    };
    let receipt_hex = hex_encode(&material.receipt);
    let activity_id = hex_encode(&material.activity_id);
    remember_receipt(emulator, activity_id.clone(), receipt_hex.clone());
    let state = if result_code == 0 {
        "completed"
    } else {
        "refused"
    };
    success(trace, &format!("{{\"state\":\"{state}\",\"activity_id\":\"{activity_id}\",\"batch_id\":\"{}\",\"global_sequence\":{global_sequence},\"result_code\":{result_code},\"state_root\":\"{}\",\"receipt\":\"{receipt_hex}\"}}", hex_encode(&batch_id), hex_encode(&state_root)))
}

fn decode_activity(request: &Request) -> Result<Vec<u8>, String> {
    let media_type = request
        .content_type
        .split(';')
        .next()
        .unwrap_or_default()
        .trim();
    if media_type == "application/octet-stream" {
        Ok(request.body.clone())
    } else if media_type == "application/json" {
        decode_json::<ActivityBody>(request).and_then(|value| hex_decode(&value.activity))
    } else {
        Err("program-call content type is not supported".into())
    }
}

fn programs_registry() -> Result<(ActivityType, ModuleRegistry), String> {
    let call_type = ActivityType::new(ModuleId::Programs, 3)
        .map_err(|_| "Programs CALL activity type is unavailable".to_owned())?;
    let registration = ModuleRegistration::new(ModuleId::Programs, &[call_type])
        .map_err(|_| "Programs module registration is unavailable".to_owned())?;
    let registry = ModuleRegistry::new(&[registration])
        .map_err(|_| "Programs module registry is unavailable".to_owned())?;
    Ok((call_type, registry))
}

fn legacy_program_request(request: &Request) -> Result<(Vec<u8>, ProgramCall), String> {
    let body = decode_json::<ProgramCallBody>(request)?;
    if !canonical_hex32_text(&body.program_id) {
        return Err("program id must be canonical 32-byte hexadecimal".to_owned());
    }
    let program_bytes = hex_decode(&body.program_id)?;
    let program_id: [u8; 32] = program_bytes
        .try_into()
        .map_err(|_| "program id must be 32-byte hexadecimal".to_owned())?;
    let program = ProgramId::new(program_id);
    if program.is_zero() {
        return Err("program id is reserved".to_owned());
    }
    let calldata_bytes = hex_decode(&body.calldata)?;
    let calldata = Calldata::new(&calldata_bytes)
        .map_err(|_| "program calldata exceeds its bound".to_owned())?;
    let fuel = body
        .budget
        .fuel
        .parse::<u64>()
        .map_err(|_| "program fuel budget is invalid".to_owned())?;
    if fuel.to_string() != body.budget.fuel {
        return Err("program fuel budget is not canonical".to_owned());
    }
    let fee_limit = body
        .budget
        .fee_limit
        .parse::<u128>()
        .map_err(|_| "program fee limit is invalid".to_owned())?;
    if fee_limit.to_string() != body.budget.fee_limit {
        return Err("program fee limit is not canonical".to_owned());
    }
    let budget = CallBudget::new(fuel, Amount::from_u128(fee_limit))
        .map_err(|_| "program budget is invalid".to_owned())?;
    let requested = body
        .capabilities
        .iter()
        .map(|capability| match capability.as_str() {
            "storage_read" => Ok(CapabilityRequest::StorageRead),
            "storage_write" => Ok(CapabilityRequest::StorageWrite),
            "transfer" => Ok(CapabilityRequest::Transfer),
            "emit_event" => Ok(CapabilityRequest::EmitEvent),
            "compose" => Ok(CapabilityRequest::Compose),
            _ => Err("program capability is invalid".to_owned()),
        })
        .collect::<Result<Vec<_>, _>>()?;
    let capabilities = RequestedCapabilities::new(&requested)
        .map_err(|_| "program capabilities are not canonical".to_owned())?;
    if body.signed_activity.len() % 2 != 0 || body.signed_activity.len() / 2 > MAX_RECEIPT_BYTES {
        return Err("signed program activity exceeds its bound".to_owned());
    }
    Ok((
        hex_decode(&body.signed_activity)?,
        ProgramCall::new(program, calldata, budget, capabilities),
    ))
}

fn decode_program_activity(request: &Request) -> Result<DecodedProgramActivity, String> {
    let media_type = request
        .content_type
        .split(';')
        .next()
        .unwrap_or_default()
        .trim();
    let (call_type, registry) = programs_registry()?;
    if media_type == "application/json" {
        if let Some((signed, program_id)) = native_call::parse_json(&request.body, &registry)? {
            let activity = decode_signed(&signed, &registry)
                .map_err(|_| "signed native activity is invalid".to_owned())?;
            return Ok(DecodedProgramActivity {
                activity_id: derive_activity_id(&activity)
                    .map_err(|_| "signed native activity identity is invalid".to_owned())?,
                idempotency_key: activity.idempotency_key(),
                protocol_version: activity.protocol_version(),
                program_id,
                signed,
            });
        }
    }
    let (signed, expected_call) = if media_type == "application/octet-stream" {
        if request.body.len() > MAX_RECEIPT_BYTES {
            return Err("signed program activity exceeds its bound".to_owned());
        }
        (request.body.clone(), None)
    } else if media_type == "application/json" {
        let (signed, call) = legacy_program_request(request)?;
        (signed, Some(call))
    } else {
        return Err("program call content type is not supported".to_owned());
    };
    if signed.is_empty() {
        return Err("program call activity must not be empty".to_owned());
    }
    let activity = decode_signed(&signed, &registry)
        .map_err(|_| "signed program activity is invalid".to_owned())?;
    if activity.activity_type() != call_type {
        return Err("signed activity is not a Programs CALL".to_owned());
    }
    if activity.protocol_version() == 3 {
        if expected_call.is_some() {
            return Err("native protocol requires the native request model".to_owned());
        }
        let program_id = native_call::from_activity(&activity)?;
        return Ok(DecodedProgramActivity {
            activity_id: derive_activity_id(&activity)
                .map_err(|_| "signed native activity identity is invalid".to_owned())?,
            idempotency_key: activity.idempotency_key(),
            protocol_version: activity.protocol_version(),
            program_id,
            signed,
        });
    }
    let call = ProgramCall::from_canonical_payload(activity.payload())
        .map_err(|_| "signed program payload is not canonical".to_owned())?;
    if expected_call
        .as_ref()
        .is_some_and(|expected| expected != &call)
    {
        return Err("signed program activity does not match the typed call".to_owned());
    }
    let activity_id = derive_activity_id(&activity)
        .map_err(|_| "signed program activity identity is invalid".to_owned())?;
    Ok(DecodedProgramActivity {
        signed,
        program_id: call.callee().bytes(),
        protocol_version: activity.protocol_version(),
        activity_id,
        idempotency_key: activity.idempotency_key(),
    })
}

fn take_core_receipt(receipt: &mut CoreReceipt) -> Result<OwnedCoreReceipt, String> {
    let result = if receipt.bytes.is_null()
        || receipt.length == 0
        || receipt.length > MAX_RECEIPT_BYTES
        || receipt.terminal_payload_length > MAX_RECEIPT_BYTES
        || (receipt.terminal_payload_length != 0 && receipt.terminal_payload.is_null())
        || receipt.call_graph_length > MAX_RECEIPT_BYTES
        || (receipt.call_graph_length != 0 && receipt.call_graph.is_null())
    {
        Err("the LayerX core returned invalid Programs receipt material".to_owned())
    } else {
        let receipt_bytes =
            unsafe { slice::from_raw_parts(receipt.bytes, receipt.length) }.to_vec();
        let terminal_payload = if receipt.terminal_payload_length == 0 {
            Vec::new()
        } else {
            unsafe {
                slice::from_raw_parts(receipt.terminal_payload, receipt.terminal_payload_length)
            }
            .to_vec()
        };
        let call_graph = if receipt.call_graph_length == 0 {
            Vec::new()
        } else {
            unsafe { slice::from_raw_parts(receipt.call_graph, receipt.call_graph_length) }.to_vec()
        };
        Ok(OwnedCoreReceipt {
            activity_id: receipt.activity_id,
            receipt: receipt_bytes,
            terminal_payload,
            call_graph,
        })
    };
    unsafe { platform_emulator_receipt_release(receipt) };
    result
}

fn inspect_state(emulator: &Emulator) -> Result<CoreState, i32> {
    let mut state = CoreState {
        canonical_state_root: [0; 32],
        receipt_state_root: [0; 32],
        next_sequence: 0,
        batch_number: 0,
        timestamp_ms: 0,
        cell_count: 0,
        account_count: 0,
    };
    let code = unsafe { platform_emulator_inspect(emulator.core, &raw mut state) };
    if code == 0 {
        Ok(state)
    } else {
        Err(code)
    }
}

fn prefund_core(
    emulator: &mut Emulator,
    did: &str,
    public_key: [u8; 32],
    amount_hi: u64,
    amount_lo: u64,
) -> Result<(), i32> {
    if emulator.accounts.len() >= MAX_EMULATOR_ACCOUNTS {
        return Err(-5);
    }
    let account_name = format!("agent:{did}:main");
    if emulator.accounts.contains_key(&account_name) {
        return Err(-3);
    }
    let code = unsafe {
        platform_emulator_prefund(
            emulator.core,
            did.as_ptr(),
            did.len(),
            public_key.as_ptr(),
            amount_hi,
            amount_lo,
        )
    };
    if code != 0 {
        return Err(code);
    }
    let account = core_account(emulator, &account_name)?.ok_or(-3)?;
    emulator.accounts.insert(
        account_name,
        EmulatorAccount {
            id: account.id,
            did: did.to_owned(),
            public_key,
        },
    );
    Ok(())
}

fn program_head(emulator: &mut Emulator, program_id: [u8; 32]) -> Result<CoreProgram, i32> {
    let mut program = CoreProgram {
        program_id: [0; 32],
        code_hash: [0; 32],
        deployment_receipt_digest: [0; 32],
        version: 0,
        abi_version: 0,
        lifecycle: 0,
        interface_bytes: ptr::null(),
        interface_length: 0,
        has_interface: 0,
        state_root: [0; 32],
        observed_sequence: 0,
    };
    let code = unsafe {
        platform_emulator_program_read(emulator.core, program_id.as_ptr(), &raw mut program)
    };
    if code == 0 {
        Ok(program)
    } else {
        Err(code)
    }
}

fn program_head_error(trace: u64, code: i32) -> Response {
    if code == -7 {
        refusal(trace, 404, "unknown_program", "program is not registered")
    } else {
        core_response(trace, code)
    }
}

fn active_program_head(
    emulator: &mut Emulator,
    program_id: [u8; 32],
    trace: u64,
) -> Result<CoreProgram, Response> {
    let head =
        program_head(emulator, program_id).map_err(|code| program_head_error(trace, code))?;
    if head.lifecycle != 1 {
        return Err(refusal(
            trace,
            409,
            "program_not_active",
            "program lifecycle does not permit calls",
        ));
    }
    Ok(head)
}

fn program_failure_json(failure: ProgramCallFailure) -> serde_json::Value {
    match failure {
        ProgramCallFailure::UnknownProgram => serde_json::json!({"kind":"unknown_program"}),
        ProgramCallFailure::Reentrancy => serde_json::json!({"kind":"reentrancy"}),
        ProgramCallFailure::DepthExceeded { limit, attempted } => {
            serde_json::json!({"kind":"depth_exceeded","limit":limit,"attempted":attempted})
        }
        ProgramCallFailure::FanoutExceeded { limit, attempted } => {
            serde_json::json!({"kind":"fanout_exceeded","limit":limit,"attempted":attempted})
        }
        ProgramCallFailure::GuestRefused { code } => {
            serde_json::json!({"kind":"guest_refused","code":code})
        }
        ProgramCallFailure::Authority => serde_json::json!({"kind":"authority"}),
        ProgramCallFailure::Resource => serde_json::json!({"kind":"resource"}),
        ProgramCallFailure::Response => serde_json::json!({"kind":"response"}),
        ProgramCallFailure::Fault => serde_json::json!({"kind":"fault"}),
    }
}

fn program_outcome_json(outcome: &ProgramCallOutcome) -> serde_json::Value {
    match outcome {
        ProgramCallOutcome::Completed(response) => serde_json::json!({
            "kind":"completed",
            "code":response.code(),
            "response":hex_encode(response.body()),
        }),
        ProgramCallOutcome::LegacyCompleted(response) => serde_json::json!({
            "kind":"legacy_completed",
            "code":response.code(),
            "values":response.values().iter().map(|value| match value {
                ProgramLegacyValue::I32(value) => serde_json::json!({"type":"i32","value":value}),
                ProgramLegacyValue::I64(value) => serde_json::json!({"type":"i64","value":value.to_string()}),
            }).collect::<Vec<_>>(),
        }),
        ProgramCallOutcome::Refused(failure) => serde_json::json!({
            "kind":"refused",
            "failure":program_failure_json(*failure),
        }),
    }
}

fn verified_program_document(
    verified: &VerifiedProgramExecution,
    terminal_payload: &[u8],
    call_graph: &[u8],
    program_id: [u8; 32],
    guest_abi_version: u16,
    sequencer_public_key: [u8; 32],
    state: &str,
    idempotency_key: Option<&str>,
) -> Result<serde_json::Value, String> {
    let protocol = verified
        .receipt()
        .receipt()
        .protocol()
        .ok_or_else(|| "verified Programs receipt has no protocol body".to_owned())?;
    let receipt_digest = verified
        .receipt()
        .evidence()
        .receipt_digest()
        .ok_or_else(|| "verified Programs receipt has no digest".to_owned())?;
    let state = if state == "executed" && !verified.outcome().is_completed() {
        "refused"
    } else {
        state
    };
    let mut document = serde_json::json!({
        "state":state,
        "activity_id":hex_encode(&protocol.activity_id()),
        "program_id":hex_encode(&program_id),
        "guest_abi_version":guest_abi_version,
        "module_version":protocol.module_version(),
        "batch_id":hex_encode(&protocol.batch_id()),
        "global_sequence":protocol.global_sequence().to_string(),
        "result_code":verified.result_code(),
        "state_root":hex_encode(&protocol.resulting_state_root()),
        "receipt":hex_encode(verified.receipt().canonical_bytes()),
        "receipt_digest":hex_encode(&receipt_digest),
        "terminal_payload":hex_encode(terminal_payload),
        "call_graph":hex_encode(call_graph),
        "authority":{
            "batch_id":hex_encode(&protocol.batch_id()),
            "asset":hex_encode(&protocol.asset()),
            "previous_state_root":hex_encode(&protocol.previous_state_root()),
            "resulting_state_root":hex_encode(&protocol.resulting_state_root()),
            "sequencer_public_key":hex_encode(&sequencer_public_key),
        },
        "usage":{
            "cpu_fuel":verified.cpu_fuel().to_string(),
            "memory_bytes":verified.memory_bytes().to_string(),
            "storage_read_bytes":verified.storage_read_bytes().to_string(),
            "storage_write_bytes":verified.storage_write_bytes().to_string(),
            "output_values":verified.output_values(),
            "output_bytes":verified.output_bytes().to_string(),
            "fee_units":verified.fee_units().to_string(),
        },
        "outcome":program_outcome_json(verified.outcome()),
        "verification":"receipt-terminal-and-call-graph-verified",
    });
    if let (Some(key), Some(object)) = (idempotency_key, document.as_object_mut()) {
        object.insert(
            "idempotency_key".to_owned(),
            serde_json::Value::String(key.to_owned()),
        );
    }
    Ok(document)
}

fn stored_program_operation_response(trace: u64, operation: &ProgramOperation) -> Response {
    let mut response = success(trace, &operation.response);
    if serde_json::from_str::<serde_json::Value>(&operation.response)
        .ok()
        .and_then(|value| {
            value
                .get("state")
                .and_then(serde_json::Value::as_str)
                .map(str::to_owned)
        })
        .is_some_and(|state| matches!(state.as_str(), "unknown" | "pending"))
    {
        response.status = 202;
    }
    response
}

fn program_call(emulator: &mut Emulator, request: &Request, trace: u64) -> Response {
    let request_idempotency = match request.idempotency_key.as_deref() {
        Some(value) if canonical_hex32_text(value) => value,
        _ => {
            return refusal(
                trace,
                400,
                "idempotency_key_required",
                "Idempotency-Key must be canonical 32-byte hexadecimal",
            )
        }
    };
    let media_type = request
        .content_type
        .split(';')
        .next()
        .unwrap_or_default()
        .trim();
    if request.body.is_empty()
        || !matches!(media_type, "application/json" | "application/octet-stream")
    {
        return refusal(
            trace,
            415,
            "activity_content_type_required",
            "program call requires bounded JSON or octet-stream content",
        );
    }
    let decoded = match decode_program_activity(request) {
        Ok(activity) => activity,
        Err(error) => return refusal(trace, 400, "invalid_program_call", &error),
    };
    let protocol_idempotency = hex_encode(&decoded.idempotency_key);
    if request_idempotency != protocol_idempotency {
        return refusal(
            trace,
            409,
            "protocol_idempotency_mismatch",
            "Idempotency-Key must equal the signed activity idempotency key",
        );
    }
    let activity_id = hex_encode(&decoded.activity_id);
    if let Some(existing) = emulator.program_operations.get(&protocol_idempotency) {
        if existing.activity_id == activity_id {
            return stored_program_operation_response(trace, existing);
        }
        return refusal(
            trace,
            409,
            "idempotency_conflict",
            "the idempotency key is already bound to a different activity",
        );
    }
    if emulator.program_operations.len() >= MAX_PROGRAM_OPERATIONS {
        return refusal(
            trace,
            503,
            "persistence_unavailable",
            "program recovery index reached its bounded capacity",
        );
    }
    let program_id = decoded.program_id;
    let head = match active_program_head(emulator, program_id, trace) {
        Ok(head) => head,
        Err(response) => return response,
    };
    let before = match inspect_state(emulator) {
        Ok(state) => state,
        Err(code) => return core_response(trace, code),
    };
    let mut receipt = CoreReceipt {
        activity_id: [0; 32],
        batch_id: [0; 32],
        state_root: [0; 32],
        previous_state_root: [0; 32],
        asset: [0; 32],
        sequencer_public_key: [0; 32],
        global_sequence: 0,
        result_code: 0,
        metered_cost_hi: 0,
        metered_cost_lo: 0,
        bytes: ptr::null(),
        length: 0,
        terminal_payload: ptr::null(),
        terminal_payload_length: 0,
        call_graph: ptr::null(),
        call_graph_length: 0,
        isolated_owner: ptr::null_mut(),
    };
    let code = unsafe {
        platform_emulator_execute(
            emulator.core,
            decoded.signed.as_ptr(),
            decoded.signed.len(),
            &raw mut receipt,
        )
    };
    if code == -904 {
        let retained_signed_activity = hex_encode(&decoded.signed);
        let response = serde_json::json!({
            "state":"unknown",
            "activity_id":activity_id.as_str(),
            "idempotency_key":protocol_idempotency.as_str(),
            "retained_signed_activity":retained_signed_activity.as_str(),
        })
        .to_string();
        emulator
            .program_activity_operations
            .insert(activity_id.clone(), protocol_idempotency.clone());
        let operation = ProgramOperation {
            activity_id,
            response,
            retained_signed_activity: Some(retained_signed_activity),
        };
        let result = stored_program_operation_response(trace, &operation);
        emulator
            .program_operations
            .insert(protocol_idempotency, operation);
        return result;
    }
    if code != 0 {
        return core_response(trace, code);
    }
    let material = match take_core_receipt(&mut receipt) {
        Ok(material) => material,
        Err(error) => return refusal(trace, 503, "core_invalid_output", &error),
    };
    let verified = match verify_program_execution(
        &material.receipt,
        &material.terminal_payload,
        &material.call_graph,
        ProgramExecutionExpectation {
            sequencer_public_key: emulator.signing_key.verifying_key().to_bytes(),
            previous_state_root: before.receipt_state_root,
            activity_id: decoded.activity_id,
            program_id,
            guest_abi_version: head.abi_version,
        },
    ) {
        Ok(verified)
            if verified
                .receipt()
                .receipt()
                .protocol()
                .is_some_and(|protocol| {
                    protocol.protocol_version() == decoded.protocol_version
                }) =>
        {
            verified
        }
        Ok(_) | Err(_) => {
            return refusal(
                trace,
                503,
                "program_receipt_verification_failed",
                "the core result did not verify against the submitted call and trusted state",
            )
        }
    };
    let document = match verified_program_document(
        &verified,
        &material.terminal_payload,
        &material.call_graph,
        program_id,
        head.abi_version,
        emulator.signing_key.verifying_key().to_bytes(),
        "executed",
        Some(&protocol_idempotency),
    ) {
        Ok(document) => document,
        Err(error) => return refusal(trace, 503, "core_invalid_output", &error),
    };
    let response = document.to_string();
    let receipt_hex = hex_encode(&material.receipt);
    remember_receipt(emulator, activity_id.clone(), receipt_hex);
    emulator
        .program_activity_operations
        .insert(activity_id.clone(), protocol_idempotency.clone());
    emulator.program_operations.insert(
        protocol_idempotency,
        ProgramOperation {
            activity_id,
            response: response.clone(),
            retained_signed_activity: Some(hex_encode(&decoded.signed)),
        },
    );
    success(trace, &response)
}

fn inspect(emulator: &Emulator, trace: u64) -> Response {
    let mut state = CoreState {
        canonical_state_root: [0; 32],
        receipt_state_root: [0; 32],
        next_sequence: 0,
        batch_number: 0,
        timestamp_ms: 0,
        cell_count: 0,
        account_count: 0,
    };
    let code = unsafe { platform_emulator_inspect(emulator.core, &raw mut state) };
    if code != 0 {
        return core_response(trace, code);
    }
    let mut cells = String::new();
    for index in 0..state.cell_count {
        let mut key = [0_u8; 32];
        let mut hi = 0_u64;
        let mut lo = 0_u64;
        let code = unsafe {
            platform_emulator_cell(
                emulator.core,
                index,
                key.as_mut_ptr(),
                &raw mut hi,
                &raw mut lo,
            )
        };
        if code != 0 {
            return core_response(trace, code);
        }
        if !cells.is_empty() {
            cells.push(',');
        }
        let _ = write!(
            cells,
            "{{\"key\":\"{}\",\"value_hi\":{hi},\"value_lo\":{lo}}}",
            hex_encode(&key)
        );
    }
    let mut accounts = String::new();
    for index in 0..state.account_count {
        let mut id = [0_u8; 32];
        let mut name = ptr::null();
        let mut name_length = 0_usize;
        let mut hi = 0_u64;
        let mut lo = 0_u64;
        let mut next_sequence = 0_u64;
        let code = unsafe {
            platform_emulator_account(
                emulator.core,
                index,
                id.as_mut_ptr(),
                &raw mut name,
                &raw mut name_length,
                &raw mut hi,
                &raw mut lo,
                &raw mut next_sequence,
            )
        };
        if code != 0 {
            return core_response(trace, code);
        }
        if name.is_null() || name_length == 0 {
            return refusal(
                trace,
                503,
                "core_invalid_output",
                "the LayerX core returned an invalid account name buffer",
            );
        }
        let account_name = unsafe { slice::from_raw_parts(name, name_length) };
        let account_name = String::from_utf8_lossy(account_name);
        if !accounts.is_empty() {
            accounts.push(',');
        }
        let _ = write!(
            accounts,
            "{{\"id\":\"{}\",\"name\":\"{}\",\"balance_hi\":{hi},\"balance_lo\":{lo},\"next_sequence\":{next_sequence}}}",
            hex_encode(&id),
            escape_json(&account_name)
        );
    }
    success(trace, &format!("{{\"network_mode\":\"emulator\",\"batch_cadence\":\"instant\",\"state_root\":\"{}\",\"canonical_state_root\":\"{}\",\"receipt_state_root\":\"{}\",\"next_sequence\":{},\"batch_number\":{},\"timestamp_ms\":{},\"cells\":[{cells}],\"accounts\":[{accounts}]}}", hex_encode(&state.canonical_state_root), hex_encode(&state.canonical_state_root), hex_encode(&state.receipt_state_root), state.next_sequence, state.batch_number, state.timestamp_ms))
}

fn sequencer_identity(emulator: &Emulator, trace: u64) -> Response {
    success(
        trace,
        &sequencer_identity_body(
            emulator.network_id,
            &emulator.signing_key.verifying_key().to_bytes(),
        ),
    )
}

fn sequencer_identity_body(network_id: u32, sequencer_public_key: &[u8; 32]) -> String {
    format!(
        "{{\"network_id\":{network_id},\"sequencer_public_key\":\"{}\"}}",
        hex_encode(sequencer_public_key)
    )
}

fn health(emulator: &Emulator, trace: u64) -> Response {
    let mut state = CoreState {
        canonical_state_root: [0; 32],
        receipt_state_root: [0; 32],
        next_sequence: 0,
        batch_number: 0,
        timestamp_ms: 0,
        cell_count: 0,
        account_count: 0,
    };
    let code = unsafe { platform_emulator_inspect(emulator.core, &raw mut state) };
    if code != 0 {
        return refusal(
            trace,
            503,
            "core_unavailable",
            "the LayerX core did not return a readable state",
        );
    }
    success(trace, "{\"status\":\"ready\",\"core\":\"layerx\"}")
}

fn prefund(emulator: &mut Emulator, request: &Request, trace: u64) -> Response {
    let body = match decode_json::<PrefundBody>(request) {
        Ok(body) => body,
        Err(error) => return refusal(trace, 400, "invalid_argument", &error),
    };
    if body.did.len() > 512 || !body.did.starts_with("did:") || body.did.contains('\0') {
        return refusal(trace, 400, "invalid_argument", "did is not a bounded DID");
    }
    let Ok(key) = hex_decode(&body.public_key) else {
        return refusal(trace, 400, "invalid_argument", "public_key must be hex");
    };
    if key.len() != 32 {
        return refusal(
            trace,
            400,
            "invalid_argument",
            "public_key must be 32 bytes",
        );
    }
    let public_key: [u8; 32] = match key.try_into() {
        Ok(value) => value,
        Err(_) => {
            return refusal(
                trace,
                400,
                "invalid_argument",
                "public_key must be 32 bytes",
            )
        }
    };
    match prefund_core(
        emulator,
        &body.did,
        public_key,
        body.amount_hi,
        body.amount_lo,
    ) {
        Ok(()) => success(trace, "{\"prefunded\":true}"),
        Err(code) => core_response(trace, code),
    }
}

fn move_quote_document(quote: &MoveQuoteRecord) -> Result<serde_json::Value, String> {
    Ok(serde_json::json!({
        "quote_id":quote.quote_id,
        "description_copy_key":"move.quote.send-to-account",
        "mechanism":"transfer",
        "money":{"amount":quote.amount.to_string(),"currency":quote.currency},
        "fee_estimate":{"amount":"0","currency":quote.currency},
        "fee_ceiling":{"amount":"0","currency":quote.currency},
        "arrival_estimate":timestamp_text(quote.created_at)?,
        "expires_at":timestamp_text(quote.expires_at)?,
        "irreversibility_copy_key":"move.irreversible.send-to-account",
    }))
}

fn move_quote(emulator: &mut Emulator, request: &Request, trace: u64) -> Response {
    let body = match decode_json::<MoveQuoteBody>(request) {
        Ok(body) => body,
        Err(error) => return refusal(trace, 400, "invalid_move_quote", &error),
    };
    if body.source == body.destination
        || account_did(&body.source).is_none()
        || account_did(&body.destination).is_none()
        || body.source.len() > 512
        || body.destination.len() > 512
    {
        return refusal(
            trace,
            400,
            "invalid_move_quote",
            "source and destination must name distinct bounded agent main accounts",
        );
    }
    if body.money.currency != "LXP" {
        return refusal(
            trace,
            400,
            "unsupported_move_asset",
            "the emulator canonical asset is LXP",
        );
    }
    let amount = match body.money.amount.parse::<u128>() {
        Ok(value) if value != 0 && value.to_string() == body.money.amount => value,
        _ => {
            return refusal(
                trace,
                400,
                "invalid_move_amount",
                "move amount must be a positive canonical protocol integer",
            )
        }
    };
    if emulator.move_quotes.len() >= MAX_MOVE_QUOTES {
        return refusal(
            trace,
            503,
            "persistence_unavailable",
            "move quote recovery index reached its bounded capacity",
        );
    }
    let source_authority = match emulator.accounts.get(&body.source) {
        Some(value) => value.clone(),
        None => {
            return refusal(
                trace,
                404,
                "move_source_not_found",
                "source account is not registered in this emulator",
            )
        }
    };
    if source_authority.public_key != emulator.signing_key.verifying_key().to_bytes() {
        return refusal(
            trace,
            409,
            "move_source_not_managed",
            "source account is not controlled by the emulator signing authority",
        );
    }
    let source = match core_account(emulator, &body.source) {
        Ok(Some(value)) => value,
        Ok(None) => {
            return refusal(
                trace,
                404,
                "move_source_not_found",
                "source account is absent from canonical state",
            )
        }
        Err(code) => return core_response(trace, code),
    };
    let identity_sequence = match core_identity_sequence(emulator, &source_authority.did) {
        Ok(value) => value,
        Err(code) => return core_response(trace, code),
    };
    if identity_sequence.checked_add(1).is_none() || source.next_sequence.checked_add(1).is_none() {
        return refusal(
            trace,
            503,
            "core_invalid_output",
            "move source sequence is exhausted",
        );
    }
    let destination = match core_account(emulator, &body.destination) {
        Ok(Some(value)) => value,
        Ok(None) => {
            return refusal(
                trace,
                404,
                "move_destination_not_found",
                "destination account is absent from canonical state",
            )
        }
        Err(code) => return core_response(trace, code),
    };
    if destination.balance.checked_add(amount).is_none() {
        return refusal(
            trace,
            409,
            "move_balance_unavailable",
            "destination canonical balance cannot accept the requested move",
        );
    }
    if source.id != source_authority.id || source.balance < amount {
        return refusal(
            trace,
            409,
            "move_balance_unavailable",
            "source canonical balance cannot cover the requested move",
        );
    }
    let state = match inspect_state(emulator) {
        Ok(value) => value,
        Err(code) => return core_response(trace, code),
    };
    let expires_at = match state.timestamp_ms.checked_add(MOVE_QUOTE_WINDOW_MS) {
        Some(value) => value,
        None => {
            return refusal(
                trace,
                503,
                "core_invalid_output",
                "move quote expiry overflowed",
            )
        }
    };
    if timestamp_text(state.timestamp_ms).is_err() || timestamp_text(expires_at).is_err() {
        return refusal(
            trace,
            503,
            "core_invalid_output",
            "move quote timestamp is outside the supported range",
        );
    }
    let mut quote_material = Vec::new();
    quote_material.extend_from_slice(&(body.source.len() as u16).to_be_bytes());
    quote_material.extend_from_slice(body.source.as_bytes());
    quote_material.extend_from_slice(&(body.destination.len() as u16).to_be_bytes());
    quote_material.extend_from_slice(body.destination.as_bytes());
    quote_material.extend_from_slice(&amount.to_be_bytes());
    quote_material.extend_from_slice(&state.timestamp_ms.to_be_bytes());
    quote_material.extend_from_slice(&expires_at.to_be_bytes());
    quote_material.extend_from_slice(&identity_sequence.to_be_bytes());
    quote_material.extend_from_slice(&source.next_sequence.to_be_bytes());
    quote_material.extend_from_slice(&state.canonical_state_root);
    quote_material.extend_from_slice(&state.receipt_state_root);
    let quote_id = format!(
        "qte_{}",
        hex_encode(&hash_bytes(MOVE_QUOTE_DOMAIN, &quote_material))
    );
    let quote = MoveQuoteRecord {
        quote_id: quote_id.clone(),
        source: body.source,
        destination: body.destination,
        source_id: source.id,
        destination_id: destination.id,
        amount,
        currency: body.money.currency,
        created_at: state.timestamp_ms,
        expires_at,
        identity_sequence,
        source_sequence: source.next_sequence,
        committed_idempotency: None,
    };
    let document = match move_quote_document(&quote) {
        Ok(value) => value,
        Err(error) => return refusal(trace, 503, "core_invalid_output", &error),
    };
    emulator.move_quotes.insert(quote_id, quote);
    success(trace, &document.to_string())
}

fn empty_core_receipt() -> CoreReceipt {
    CoreReceipt {
        activity_id: [0; 32],
        batch_id: [0; 32],
        state_root: [0; 32],
        previous_state_root: [0; 32],
        asset: [0; 32],
        sequencer_public_key: [0; 32],
        global_sequence: 0,
        result_code: 0,
        metered_cost_hi: 0,
        metered_cost_lo: 0,
        bytes: ptr::null(),
        length: 0,
        terminal_payload: ptr::null(),
        terminal_payload_length: 0,
        call_graph: ptr::null(),
        call_graph_length: 0,
        isolated_owner: ptr::null_mut(),
    }
}

fn stored_move_operation(operation: &MoveOperation) -> Response {
    Response {
        status: operation.status,
        content_type: "application/json",
        body: operation.body.as_bytes().to_vec(),
    }
}

fn retain_move_response(
    emulator: &mut Emulator,
    idempotency: String,
    quote_id: &str,
    response: &Response,
) {
    let body = String::from_utf8_lossy(&response.body).into_owned();
    emulator.move_operations.insert(
        idempotency.clone(),
        MoveOperation {
            quote_id: quote_id.to_owned(),
            status: response.status,
            body,
        },
    );
    if let Some(quote) = emulator.move_quotes.get_mut(quote_id) {
        quote.committed_idempotency = Some(idempotency);
    }
}

fn move_unknown(
    emulator: &mut Emulator,
    idempotency: String,
    quote_id: &str,
    trace: u64,
    code: &str,
    detail: &str,
) -> Response {
    let response = refusal(trace, 503, code, detail);
    retain_move_response(emulator, idempotency, quote_id, &response);
    response
}

fn move_journey(
    quote: &MoveQuoteRecord,
    idempotency: &str,
    activity_id: [u8; 32],
    receipt_digest: [u8; 32],
    updated_at: u64,
) -> Result<serde_json::Value, String> {
    let journey_digest = hash_bytes(MOVE_JOURNEY_DOMAIN, idempotency.as_bytes());
    let stage_digest = hash_bytes(MOVE_STAGE_DOMAIN, &journey_digest);
    let evidence_id = format!("evd_{}", hex_encode(&receipt_digest));
    let evidence = serde_json::json!({
        "evidence_id":evidence_id,
        "class":"layerx-receipt",
        "verification":"receipt-verified",
        "source_ref":format!("/v1/receipts/{}", hex_encode(&activity_id)),
    });
    Ok(serde_json::json!({
        "journey_id":format!("jrn_{}", hex_encode(&journey_digest)),
        "kind":"move",
        "state":"done",
        "state_copy_key":"status.done",
        "stages":[{
            "stage_id":format!("stg_{}", hex_encode(&stage_digest)),
            "copy_key":"move.stage.send-to-account",
            "state":"done",
            "evidence":[evidence.clone()],
        }],
        "evidence":[evidence],
        "started_at":timestamp_text(quote.created_at)?,
        "updated_at":timestamp_text(updated_at)?,
    }))
}

fn move_commit(emulator: &mut Emulator, request: &Request, trace: u64) -> Response {
    let idempotency = match request.idempotency_key.as_deref() {
        Some(value) if valid_human_idempotency(value) => value.to_owned(),
        _ => {
            return refusal(
                trace,
                400,
                "idempotency_key_required",
                "Idempotency-Key must be 16-128 safe ASCII characters",
            )
        }
    };
    let body = match decode_json::<MoveCommitBody>(request) {
        Ok(value) => value,
        Err(error) => return refusal(trace, 400, "invalid_move_commit", &error),
    };
    if let Some(operation) = emulator.move_operations.get(&idempotency) {
        if operation.quote_id == body.quote_id {
            return stored_move_operation(operation);
        }
        return refusal(
            trace,
            409,
            "idempotency_conflict",
            "the idempotency key is already bound to another move quote",
        );
    }
    if emulator.move_operations.len() >= MAX_MOVE_OPERATIONS {
        return refusal(
            trace,
            503,
            "persistence_unavailable",
            "move operation recovery index reached its bounded capacity",
        );
    }
    let quote = match emulator.move_quotes.get(&body.quote_id) {
        Some(value) => value.clone(),
        None => {
            return refusal(
                trace,
                404,
                "move_quote_not_found",
                "move quote is not present in this emulator process",
            )
        }
    };
    if quote.committed_idempotency.is_some() {
        return refusal(
            trace,
            409,
            "move_quote_already_committed",
            "move quote is already bound to another commit attempt",
        );
    }
    let state = match inspect_state(emulator) {
        Ok(value) => value,
        Err(code) => return core_response(trace, code),
    };
    if state.timestamp_ms > quote.expires_at {
        return refusal(
            trace,
            409,
            "move_quote_expired",
            "move quote expired before commitment",
        );
    }
    let source_authority = match emulator.accounts.get(&quote.source) {
        Some(value)
            if value.id == quote.source_id
                && value.public_key == emulator.signing_key.verifying_key().to_bytes() =>
        {
            value.clone()
        }
        _ => {
            return refusal(
                trace,
                409,
                "move_quote_stale",
                "source authority or account sequence changed after quotation",
            )
        }
    };
    let identity_sequence = match core_identity_sequence(emulator, &source_authority.did) {
        Ok(value) if value == quote.identity_sequence => value,
        Ok(_) => {
            return refusal(
                trace,
                409,
                "move_quote_stale",
                "source identity sequence changed after quotation",
            )
        }
        Err(code) => return core_response(trace, code),
    };
    let source = match core_account(emulator, &quote.source) {
        Ok(Some(value))
            if value.id == quote.source_id
                && value.next_sequence == quote.source_sequence
                && value.balance >= quote.amount =>
        {
            value
        }
        Ok(_) => {
            return refusal(
                trace,
                409,
                "move_balance_unavailable",
                "source canonical balance cannot cover the quoted move",
            )
        }
        Err(code) => return core_response(trace, code),
    };
    let destination = match core_account(emulator, &quote.destination) {
        Ok(Some(value)) if value.id == quote.destination_id => value,
        Ok(_) => {
            return refusal(
                trace,
                409,
                "move_quote_stale",
                "destination account changed after quotation",
            )
        }
        Err(code) => return core_response(trace, code),
    };
    let _ = (source_authority, identity_sequence);
    let protocol_idempotency = move_idempotency(&idempotency);
    let (activity, expected_activity_id, expected_context_hash, expected_authorization_hash) =
        match signed_move_activity(emulator, &quote, protocol_idempotency) {
            Ok(value) => value,
            Err(error) => return refusal(trace, 503, "move_encoding_failed", &error),
        };
    let mut receipt = empty_core_receipt();
    let code = unsafe {
        platform_emulator_execute(
            emulator.core,
            activity.as_ptr(),
            activity.len(),
            &raw mut receipt,
        )
    };
    if code != 0 && code != -904 {
        return core_response(trace, code);
    }
    let batch_id = receipt.batch_id;
    let previous_state_root = receipt.previous_state_root;
    let resulting_state_root = receipt.state_root;
    let asset = receipt.asset;
    let sequencer_public_key = receipt.sequencer_public_key;
    let result_code = receipt.result_code;
    let material = match take_core_receipt(&mut receipt) {
        Ok(value) => value,
        Err(error) => {
            return move_unknown(
                emulator,
                idempotency,
                &quote.quote_id,
                trace,
                "move_receipt_unavailable",
                &error,
            );
        }
    };
    if result_code != 0 {
        let refused = core_response(trace, result_code);
        retain_move_response(emulator, idempotency, &quote.quote_id, &refused);
        return refused;
    }
    let authorised = AuthorizedBatch::new(
        batch_id,
        asset,
        previous_state_root,
        resulting_state_root,
        sequencer_public_key,
    );
    let verified = match verify_receipt(&material.receipt, &authorised) {
        Ok(value) => value,
        Err(_) => {
            return move_unknown(
                emulator,
                idempotency,
                &quote.quote_id,
                trace,
                "move_receipt_verification_failed",
                "core committed a move but its returned receipt did not verify",
            )
        }
    };
    let expected_source_after = source.balance - quote.amount;
    let expected_destination_after = destination.balance + quote.amount;
    let protocol = match verified.receipt().protocol() {
        Some(value)
            if material.activity_id == expected_activity_id
                && value.protocol_version() == layerx_wire::limits::PROTOCOL_VERSION
                && value.activity_id() == expected_activity_id
                && value.result_code() == 0
                && value.module_id() == 1
                && value.operation() == ASSET_SEND_OPERATION as u8
                && value.asset() == NATIVE_ASSET
                && value.amount() == quote.amount
                && value.from() == quote.source_id
                && value.to() == quote.destination_id
                && value.debit_balance_before() == source.balance
                && value.debit_balance_after() == expected_source_after
                && value.credit_balance_before() == destination.balance
                && value.credit_balance_after() == expected_destination_after
                && value.debit_sequence() == quote.source_sequence
                && value.previous_state_root() == state.receipt_state_root
                && value.resulting_state_root() != value.previous_state_root()
                && value.context_hash() == expected_context_hash
                && value.authorization_hash() == expected_authorization_hash
                && value.transfer_set_root() != [0; 32]
                && value.effects().len() == 1
                && value.effects()[0].module_id() == 1
                && value.effects()[0].kind() == 2
                && value.effects()[0].monetary()
                && value.effects()[0].transfer_set_root() == value.transfer_set_root() =>
        {
            value
        }
        _ => {
            return move_unknown(
                emulator,
                idempotency,
                &quote.quote_id,
                trace,
                "move_receipt_binding_failed",
                "verified receipt does not bind the exact quoted move",
            )
        }
    };
    let post_source = match core_account(emulator, &quote.source) {
        Ok(Some(value)) => value,
        Ok(None) => {
            return move_unknown(
                emulator,
                idempotency,
                &quote.quote_id,
                trace,
                "move_receipt_binding_failed",
                "committed source account disappeared from canonical state",
            )
        }
        Err(code) => return core_response(trace, code),
    };
    let post_destination = match core_account(emulator, &quote.destination) {
        Ok(Some(value)) => value,
        Ok(None) => {
            return move_unknown(
                emulator,
                idempotency,
                &quote.quote_id,
                trace,
                "move_receipt_binding_failed",
                "committed destination account disappeared from canonical state",
            )
        }
        Err(code) => return core_response(trace, code),
    };
    let post_state = match inspect_state(emulator) {
        Ok(value) => value,
        Err(code) => return core_response(trace, code),
    };
    if post_source.balance != protocol.debit_balance_after()
        || post_source.next_sequence != quote.source_sequence + 1
        || post_destination.balance != protocol.credit_balance_after()
        || post_state.receipt_state_root != protocol.resulting_state_root()
        || post_state.canonical_state_root == state.canonical_state_root
    {
        return move_unknown(
            emulator,
            idempotency,
            &quote.quote_id,
            trace,
            "move_receipt_binding_failed",
            "receipt economic facts disagree with canonical account state",
        );
    }
    let receipt_digest = match verified.evidence().receipt_digest() {
        Some(value) => value,
        None => {
            return move_unknown(
                emulator,
                idempotency,
                &quote.quote_id,
                trace,
                "move_receipt_binding_failed",
                "verified receipt omitted its digest",
            )
        }
    };
    remember_receipt(
        emulator,
        hex_encode(&expected_activity_id),
        hex_encode(&material.receipt),
    );
    let journey = match move_journey(
        &quote,
        &idempotency,
        expected_activity_id,
        receipt_digest,
        state.timestamp_ms,
    ) {
        Ok(value) => value,
        Err(error) => {
            return move_unknown(
                emulator,
                idempotency,
                &quote.quote_id,
                trace,
                "move_journey_encoding_failed",
                &error,
            )
        }
    };
    let completed = success(trace, &journey.to_string());
    retain_move_response(emulator, idempotency, &quote.quote_id, &completed);
    if code == -904 {
        refusal(
            trace,
            503,
            "move_acknowledgement_lost",
            "move committed and was retained under the original idempotency key",
        )
    } else {
        completed
    }
}

fn update_time(emulator: &mut Emulator, request: &Request, trace: u64, advance: bool) -> Response {
    let value = if advance {
        decode_json::<AdvanceBody>(request).map(|body| body.delta_ms)
    } else {
        decode_json::<TimestampBody>(request).map(|body| body.timestamp_ms)
    };
    let value = match value {
        Ok(value) => value,
        Err(error) => return refusal(trace, 400, "invalid_argument", &error),
    };
    let code = unsafe {
        if advance {
            platform_emulator_advance_time(emulator.core, value)
        } else {
            platform_emulator_set_time(emulator.core, value)
        }
    };
    if code == 0 {
        success(trace, "{\"updated\":true}")
    } else {
        core_response(trace, code)
    }
}

fn inject_fault(emulator: &mut Emulator, request: &Request, trace: u64) -> Response {
    let body = match decode_json::<FaultBody>(request) {
        Ok(body) => body,
        Err(error) => return refusal(trace, 400, "invalid_argument", &error),
    };
    let kind = match body.kind.as_str() {
        "reject" => 1,
        "drop_receipt" => 2,
        "corrupt_receipt" => 3,
        _ => return refusal(trace, 400, "invalid_argument", "unknown fault kind"),
    };
    if body.count == 0 || body.count > 1_000_000 {
        return refusal(
            trace,
            400,
            "invalid_argument",
            "fault count is outside its bound",
        );
    }
    let code = unsafe { platform_emulator_inject_failure(emulator.core, kind, body.count) };
    if code == 0 {
        success(trace, "{\"configured\":true}")
    } else {
        core_response(trace, code)
    }
}

struct RecoverySnapshot {
    core_snapshot: Vec<u8>,
    receipts: HashMap<String, String>,
    receipt_order: VecDeque<String>,
    program_operations: HashMap<String, ProgramOperation>,
    program_activity_operations: HashMap<String, String>,
    accounts: HashMap<String, EmulatorAccount>,
    move_quotes: HashMap<String, MoveQuoteRecord>,
    move_operations: HashMap<String, MoveOperation>,
}

fn append_snapshot_bytes(encoded: &mut Vec<u8>, bytes: &[u8]) -> Result<(), String> {
    let next = encoded
        .len()
        .checked_add(bytes.len())
        .and_then(|length| length.checked_add(32))
        .ok_or_else(|| "emulator recovery snapshot length overflowed".to_owned())?;
    if next > MAX_REQUEST_BYTES {
        return Err("emulator recovery snapshot exceeds its bound".to_owned());
    }
    encoded.extend_from_slice(bytes);
    Ok(())
}

fn append_snapshot_u32(encoded: &mut Vec<u8>, value: usize) -> Result<(), String> {
    let value = u32::try_from(value)
        .map_err(|_| "emulator recovery snapshot field exceeds u32".to_owned())?;
    append_snapshot_bytes(encoded, &value.to_be_bytes())
}

fn append_snapshot_u16(encoded: &mut Vec<u8>, value: usize) -> Result<(), String> {
    let value = u16::try_from(value)
        .map_err(|_| "emulator recovery snapshot field exceeds u16".to_owned())?;
    append_snapshot_bytes(encoded, &value.to_be_bytes())
}

fn append_snapshot_text(encoded: &mut Vec<u8>, value: &str) -> Result<(), String> {
    append_snapshot_u16(encoded, value.len())?;
    append_snapshot_bytes(encoded, value.as_bytes())
}

fn canonical_program_response(response: &str, activity_id: &str, idempotency_key: &str) -> bool {
    if response.is_empty() || response.len() > MAX_PROGRAM_RESPONSE_BYTES {
        return false;
    }
    let Ok(document) = serde_json::from_str::<serde_json::Value>(response) else {
        return false;
    };
    document.is_object()
        && document.to_string() == response
        && document
            .get("activity_id")
            .and_then(serde_json::Value::as_str)
            == Some(activity_id)
        && document
            .get("idempotency_key")
            .and_then(serde_json::Value::as_str)
            == Some(idempotency_key)
}

fn encode_recovery_snapshot(
    core_snapshot: &[u8],
    receipts: &HashMap<String, String>,
    receipt_order: &VecDeque<String>,
    program_operations: &HashMap<String, ProgramOperation>,
    program_activity_operations: &HashMap<String, String>,
    accounts: &HashMap<String, EmulatorAccount>,
    move_quotes: &HashMap<String, MoveQuoteRecord>,
    move_operations: &HashMap<String, MoveOperation>,
) -> Result<Vec<u8>, String> {
    if core_snapshot.is_empty() || core_snapshot.len() > MAX_CORE_SNAPSHOT_BYTES {
        return Err("emulator core snapshot is outside its bound".to_owned());
    }
    if receipts.len() > MAX_RECEIPTS || receipt_order.len() != receipts.len() {
        return Err("emulator receipt recovery index is outside its bound".to_owned());
    }
    if program_operations.len() > MAX_PROGRAM_OPERATIONS
        || program_activity_operations.len() != program_operations.len()
    {
        return Err("emulator program recovery index is outside its bound".to_owned());
    }
    if accounts.len() > MAX_EMULATOR_ACCOUNTS
        || move_quotes.len() > MAX_MOVE_QUOTES
        || move_operations.len() > MAX_MOVE_OPERATIONS
    {
        return Err("emulator move recovery index is outside its bound".to_owned());
    }

    let mut encoded = Vec::new();
    append_snapshot_bytes(&mut encoded, RECOVERY_SNAPSHOT_MAGIC)?;
    append_snapshot_bytes(&mut encoded, &RECOVERY_SNAPSHOT_VERSION.to_be_bytes())?;
    append_snapshot_u32(&mut encoded, core_snapshot.len())?;
    append_snapshot_u32(&mut encoded, receipts.len())?;
    append_snapshot_u32(&mut encoded, program_operations.len())?;
    append_snapshot_u32(&mut encoded, accounts.len())?;
    append_snapshot_u32(&mut encoded, move_quotes.len())?;
    append_snapshot_u32(&mut encoded, move_operations.len())?;
    append_snapshot_bytes(&mut encoded, core_snapshot)?;

    let mut seen_receipts = HashMap::new();
    for activity_id in receipt_order {
        if !canonical_hex32_text(activity_id) || seen_receipts.insert(activity_id, ()).is_some() {
            return Err("emulator receipt recovery order is invalid".to_owned());
        }
        let activity = hex_decode(activity_id)
            .map_err(|_| "emulator receipt activity id is invalid".to_owned())?;
        let receipt = receipts
            .get(activity_id)
            .ok_or_else(|| "emulator receipt recovery order is incomplete".to_owned())?;
        let receipt = hex_decode(receipt)
            .map_err(|_| "emulator receipt recovery bytes are invalid".to_owned())?;
        if receipt.is_empty() || receipt.len() > MAX_RECEIPT_BYTES {
            return Err("emulator receipt recovery bytes exceed their bound".to_owned());
        }
        append_snapshot_bytes(&mut encoded, &activity)?;
        append_snapshot_u32(&mut encoded, receipt.len())?;
        append_snapshot_bytes(&mut encoded, &receipt)?;
    }

    let mut operations: Vec<_> = program_operations.iter().collect();
    operations.sort_by(|(left, _), (right, _)| left.cmp(right));
    for (idempotency_key, operation) in operations {
        if !canonical_hex32_text(idempotency_key)
            || !canonical_hex32_text(&operation.activity_id)
            || program_activity_operations.get(&operation.activity_id) != Some(idempotency_key)
            || !canonical_program_response(
                &operation.response,
                &operation.activity_id,
                idempotency_key,
            )
        {
            return Err("emulator program recovery binding is invalid".to_owned());
        }
        let idempotency = hex_decode(idempotency_key)
            .map_err(|_| "emulator program idempotency key is invalid".to_owned())?;
        let activity = hex_decode(&operation.activity_id)
            .map_err(|_| "emulator program activity id is invalid".to_owned())?;
        let retained = match operation.retained_signed_activity.as_deref() {
            Some(value) => {
                let bytes = hex_decode(value).map_err(|_| {
                    "emulator retained signed program activity is invalid".to_owned()
                })?;
                if bytes.is_empty() || bytes.len() > MAX_RETAINED_SIGNED_ACTIVITY_BYTES {
                    return Err(
                        "emulator retained signed program activity exceeds its bound".to_owned(),
                    );
                }
                bytes
            }
            None => Vec::new(),
        };
        append_snapshot_bytes(&mut encoded, &idempotency)?;
        append_snapshot_bytes(&mut encoded, &activity)?;
        append_snapshot_u32(&mut encoded, operation.response.len())?;
        append_snapshot_bytes(&mut encoded, operation.response.as_bytes())?;
        append_snapshot_u32(&mut encoded, retained.len())?;
        append_snapshot_bytes(&mut encoded, &retained)?;
    }

    let mut account_entries: Vec<_> = accounts.iter().collect();
    account_entries.sort_by(|(left, _), (right, _)| left.cmp(right));
    for (name, account) in account_entries {
        if name != &format!("agent:{}:main", account.did)
            || account.did.is_empty()
            || account.did.len() > 512
            || !account.did.starts_with("did:")
            || account.did.contains('\0')
            || account.public_key == [0; 32]
            || account.id == [0; 32]
        {
            return Err("emulator account recovery binding is invalid".to_owned());
        }
        append_snapshot_text(&mut encoded, name)?;
        append_snapshot_text(&mut encoded, &account.did)?;
        append_snapshot_bytes(&mut encoded, &account.id)?;
        append_snapshot_bytes(&mut encoded, &account.public_key)?;
    }

    let mut quote_entries: Vec<_> = move_quotes.iter().collect();
    quote_entries.sort_by(|(left, _), (right, _)| left.cmp(right));
    for (quote_id, quote) in quote_entries {
        let quote_hex = quote_id.strip_prefix("qte_");
        if quote_id != &quote.quote_id
            || quote_hex.is_none_or(|value| !canonical_hex32_text(value))
            || quote.source == quote.destination
            || quote.amount == 0
            || quote.currency != "LXP"
            || quote.created_at == 0
            || quote.expires_at <= quote.created_at
            || accounts
                .get(&quote.source)
                .is_none_or(|account| account.id != quote.source_id)
            || accounts
                .get(&quote.destination)
                .is_none_or(|account| account.id != quote.destination_id)
            || quote
                .committed_idempotency
                .as_deref()
                .is_some_and(|value| !valid_human_idempotency(value))
        {
            return Err("emulator move quote recovery binding is invalid".to_owned());
        }
        append_snapshot_text(&mut encoded, quote_id)?;
        append_snapshot_text(&mut encoded, &quote.source)?;
        append_snapshot_text(&mut encoded, &quote.destination)?;
        append_snapshot_bytes(&mut encoded, &quote.source_id)?;
        append_snapshot_bytes(&mut encoded, &quote.destination_id)?;
        append_snapshot_bytes(&mut encoded, &quote.amount.to_be_bytes())?;
        append_snapshot_text(&mut encoded, &quote.currency)?;
        append_snapshot_bytes(&mut encoded, &quote.created_at.to_be_bytes())?;
        append_snapshot_bytes(&mut encoded, &quote.expires_at.to_be_bytes())?;
        append_snapshot_bytes(&mut encoded, &quote.identity_sequence.to_be_bytes())?;
        append_snapshot_bytes(&mut encoded, &quote.source_sequence.to_be_bytes())?;
        match quote.committed_idempotency.as_deref() {
            Some(value) => {
                append_snapshot_bytes(&mut encoded, &[1])?;
                append_snapshot_text(&mut encoded, value)?;
            }
            None => append_snapshot_bytes(&mut encoded, &[0])?,
        }
    }

    let mut move_entries: Vec<_> = move_operations.iter().collect();
    move_entries.sort_by(|(left, _), (right, _)| left.cmp(right));
    for (idempotency, operation) in move_entries {
        if !valid_human_idempotency(idempotency)
            || operation.body.is_empty()
            || operation.body.len() > MAX_PROGRAM_RESPONSE_BYTES
            || !matches!(operation.status, 200 | 400 | 404 | 409 | 503)
            || serde_json::from_str::<serde_json::Value>(&operation.body).is_err()
            || move_quotes
                .get(&operation.quote_id)
                .and_then(|quote| quote.committed_idempotency.as_deref())
                != Some(idempotency)
        {
            return Err("emulator move operation recovery binding is invalid".to_owned());
        }
        append_snapshot_text(&mut encoded, idempotency)?;
        append_snapshot_text(&mut encoded, &operation.quote_id)?;
        append_snapshot_bytes(&mut encoded, &operation.status.to_be_bytes())?;
        append_snapshot_u32(&mut encoded, operation.body.len())?;
        append_snapshot_bytes(&mut encoded, operation.body.as_bytes())?;
    }

    let mut digest = Sha256::new();
    digest.update(RECOVERY_SNAPSHOT_DIGEST_DOMAIN);
    digest.update(&encoded);
    let digest: [u8; 32] = digest.finalize().into();
    append_snapshot_bytes(&mut encoded, &digest)?;
    Ok(encoded)
}

fn snapshot_take<'a>(
    snapshot: &'a [u8],
    offset: &mut usize,
    length: usize,
) -> Result<&'a [u8], String> {
    let end = offset
        .checked_add(length)
        .ok_or_else(|| "emulator recovery snapshot offset overflowed".to_owned())?;
    let value = snapshot
        .get(*offset..end)
        .ok_or_else(|| "emulator recovery snapshot is truncated".to_owned())?;
    *offset = end;
    Ok(value)
}

fn snapshot_u32(snapshot: &[u8], offset: &mut usize) -> Result<usize, String> {
    let bytes: [u8; 4] = snapshot_take(snapshot, offset, 4)?
        .try_into()
        .map_err(|_| "emulator recovery snapshot integer is invalid".to_owned())?;
    usize::try_from(u32::from_be_bytes(bytes))
        .map_err(|_| "emulator recovery snapshot integer exceeds usize".to_owned())
}

fn snapshot_u16(snapshot: &[u8], offset: &mut usize) -> Result<usize, String> {
    let bytes: [u8; 2] = snapshot_take(snapshot, offset, 2)?
        .try_into()
        .map_err(|_| "emulator recovery snapshot integer is invalid".to_owned())?;
    Ok(usize::from(u16::from_be_bytes(bytes)))
}

fn snapshot_u64(snapshot: &[u8], offset: &mut usize) -> Result<u64, String> {
    let bytes: [u8; 8] = snapshot_take(snapshot, offset, 8)?
        .try_into()
        .map_err(|_| "emulator recovery snapshot integer is invalid".to_owned())?;
    Ok(u64::from_be_bytes(bytes))
}

fn snapshot_u128(snapshot: &[u8], offset: &mut usize) -> Result<u128, String> {
    let bytes: [u8; 16] = snapshot_take(snapshot, offset, 16)?
        .try_into()
        .map_err(|_| "emulator recovery snapshot integer is invalid".to_owned())?;
    Ok(u128::from_be_bytes(bytes))
}

fn snapshot_text(snapshot: &[u8], offset: &mut usize, maximum: usize) -> Result<String, String> {
    let length = snapshot_u16(snapshot, offset)?;
    if length == 0 || length > maximum {
        return Err("emulator recovery snapshot text exceeds its bound".to_owned());
    }
    std::str::from_utf8(snapshot_take(snapshot, offset, length)?)
        .map(str::to_owned)
        .map_err(|_| "emulator recovery snapshot text is not UTF-8".to_owned())
}

fn decode_recovery_snapshot(body: &[u8]) -> Result<RecoverySnapshot, String> {
    if body.len() > MAX_REQUEST_BYTES || body.len() < 8 + 4 + 7 * 4 + 32 {
        return Err("emulator recovery snapshot is outside its bound".to_owned());
    }
    let authenticated_length = body.len() - 32;
    let (authenticated, supplied_digest) = body.split_at(authenticated_length);
    let mut digest = Sha256::new();
    digest.update(RECOVERY_SNAPSHOT_DIGEST_DOMAIN);
    digest.update(authenticated);
    let expected_digest: [u8; 32] = digest.finalize().into();
    if expected_digest.as_slice() != supplied_digest {
        return Err("emulator recovery snapshot digest is invalid".to_owned());
    }

    let mut offset = 0;
    if snapshot_take(authenticated, &mut offset, 8)? != RECOVERY_SNAPSHOT_MAGIC {
        return Err("emulator recovery snapshot magic is invalid".to_owned());
    }
    let version = snapshot_u32(authenticated, &mut offset)?;
    if version != RECOVERY_SNAPSHOT_VERSION as usize {
        return Err("emulator recovery snapshot version is unsupported".to_owned());
    }
    let core_length = snapshot_u32(authenticated, &mut offset)?;
    let receipt_count = snapshot_u32(authenticated, &mut offset)?;
    let operation_count = snapshot_u32(authenticated, &mut offset)?;
    let account_count = snapshot_u32(authenticated, &mut offset)?;
    let move_quote_count = snapshot_u32(authenticated, &mut offset)?;
    let move_operation_count = snapshot_u32(authenticated, &mut offset)?;
    if core_length == 0
        || core_length > MAX_CORE_SNAPSHOT_BYTES
        || receipt_count > MAX_RECEIPTS
        || operation_count > MAX_PROGRAM_OPERATIONS
        || account_count > MAX_EMULATOR_ACCOUNTS
        || move_quote_count > MAX_MOVE_QUOTES
        || move_operation_count > MAX_MOVE_OPERATIONS
    {
        return Err("emulator recovery snapshot counts exceed their bounds".to_owned());
    }
    let core_snapshot = snapshot_take(authenticated, &mut offset, core_length)?.to_vec();

    let mut receipts = HashMap::with_capacity(receipt_count);
    let mut receipt_order = VecDeque::with_capacity(receipt_count);
    for _ in 0..receipt_count {
        let activity_id = hex_encode(snapshot_take(authenticated, &mut offset, 32)?);
        let receipt_length = snapshot_u32(authenticated, &mut offset)?;
        if receipt_length == 0 || receipt_length > MAX_RECEIPT_BYTES {
            return Err("emulator recovery receipt exceeds its bound".to_owned());
        }
        let receipt = hex_encode(snapshot_take(authenticated, &mut offset, receipt_length)?);
        if receipts.insert(activity_id.clone(), receipt).is_some() {
            return Err("emulator recovery receipt activity is duplicated".to_owned());
        }
        receipt_order.push_back(activity_id);
    }

    let mut program_operations = HashMap::with_capacity(operation_count);
    let mut program_activity_operations = HashMap::with_capacity(operation_count);
    for _ in 0..operation_count {
        let idempotency_key = hex_encode(snapshot_take(authenticated, &mut offset, 32)?);
        let activity_id = hex_encode(snapshot_take(authenticated, &mut offset, 32)?);
        let response_length = snapshot_u32(authenticated, &mut offset)?;
        if response_length == 0 || response_length > MAX_PROGRAM_RESPONSE_BYTES {
            return Err("emulator recovery program response exceeds its bound".to_owned());
        }
        let response =
            std::str::from_utf8(snapshot_take(authenticated, &mut offset, response_length)?)
                .map_err(|_| "emulator recovery program response is not UTF-8".to_owned())?
                .to_owned();
        if !canonical_program_response(&response, &activity_id, &idempotency_key) {
            return Err("emulator recovery program response is invalid".to_owned());
        }
        let retained_length = snapshot_u32(authenticated, &mut offset)?;
        if retained_length > MAX_RETAINED_SIGNED_ACTIVITY_BYTES {
            return Err("emulator retained recovery activity exceeds its bound".to_owned());
        }
        let retained_signed_activity = if retained_length == 0 {
            None
        } else {
            Some(hex_encode(snapshot_take(
                authenticated,
                &mut offset,
                retained_length,
            )?))
        };
        if program_activity_operations
            .insert(activity_id.clone(), idempotency_key.clone())
            .is_some()
            || program_operations
                .insert(
                    idempotency_key,
                    ProgramOperation {
                        activity_id,
                        response,
                        retained_signed_activity,
                    },
                )
                .is_some()
        {
            return Err("emulator recovery program binding is duplicated".to_owned());
        }
    }

    let mut accounts = HashMap::with_capacity(account_count);
    for _ in 0..account_count {
        let name = snapshot_text(authenticated, &mut offset, 1_024)?;
        let did = snapshot_text(authenticated, &mut offset, 512)?;
        let id: [u8; 32] = snapshot_take(authenticated, &mut offset, 32)?
            .try_into()
            .map_err(|_| "emulator recovery account id is invalid".to_owned())?;
        let public_key: [u8; 32] = snapshot_take(authenticated, &mut offset, 32)?
            .try_into()
            .map_err(|_| "emulator recovery public key is invalid".to_owned())?;
        if name != format!("agent:{did}:main")
            || !did.starts_with("did:")
            || did.contains('\0')
            || id == [0; 32]
            || public_key == [0; 32]
            || accounts
                .insert(
                    name,
                    EmulatorAccount {
                        id,
                        did,
                        public_key,
                    },
                )
                .is_some()
        {
            return Err("emulator recovery account binding is invalid".to_owned());
        }
    }

    let mut move_quotes = HashMap::with_capacity(move_quote_count);
    for _ in 0..move_quote_count {
        let quote_id = snapshot_text(authenticated, &mut offset, 80)?;
        let source = snapshot_text(authenticated, &mut offset, 1_024)?;
        let destination = snapshot_text(authenticated, &mut offset, 1_024)?;
        let source_id: [u8; 32] = snapshot_take(authenticated, &mut offset, 32)?
            .try_into()
            .map_err(|_| "emulator recovery source id is invalid".to_owned())?;
        let destination_id: [u8; 32] = snapshot_take(authenticated, &mut offset, 32)?
            .try_into()
            .map_err(|_| "emulator recovery destination id is invalid".to_owned())?;
        let amount = snapshot_u128(authenticated, &mut offset)?;
        let currency = snapshot_text(authenticated, &mut offset, 16)?;
        let created_at = snapshot_u64(authenticated, &mut offset)?;
        let expires_at = snapshot_u64(authenticated, &mut offset)?;
        let identity_sequence = snapshot_u64(authenticated, &mut offset)?;
        let source_sequence = snapshot_u64(authenticated, &mut offset)?;
        let committed_idempotency = match snapshot_take(authenticated, &mut offset, 1)?[0] {
            0 => None,
            1 => Some(snapshot_text(authenticated, &mut offset, 128)?),
            _ => return Err("emulator recovery move quote state is invalid".to_owned()),
        };
        let quote = MoveQuoteRecord {
            quote_id: quote_id.clone(),
            source,
            destination,
            source_id,
            destination_id,
            amount,
            currency,
            created_at,
            expires_at,
            identity_sequence,
            source_sequence,
            committed_idempotency,
        };
        if quote_id
            .strip_prefix("qte_")
            .is_none_or(|value| !canonical_hex32_text(value))
            || quote.source == quote.destination
            || quote.amount == 0
            || quote.currency != "LXP"
            || quote.created_at == 0
            || quote.expires_at <= quote.created_at
            || accounts
                .get(&quote.source)
                .is_none_or(|account| account.id != quote.source_id)
            || accounts
                .get(&quote.destination)
                .is_none_or(|account| account.id != quote.destination_id)
            || quote
                .committed_idempotency
                .as_deref()
                .is_some_and(|value| !valid_human_idempotency(value))
            || move_quotes.insert(quote_id, quote).is_some()
        {
            return Err("emulator recovery move quote binding is invalid".to_owned());
        }
    }

    let mut move_operations = HashMap::with_capacity(move_operation_count);
    for _ in 0..move_operation_count {
        let idempotency = snapshot_text(authenticated, &mut offset, 128)?;
        let quote_id = snapshot_text(authenticated, &mut offset, 80)?;
        let status_bytes: [u8; 2] = snapshot_take(authenticated, &mut offset, 2)?
            .try_into()
            .map_err(|_| "emulator recovery move status is invalid".to_owned())?;
        let status = u16::from_be_bytes(status_bytes);
        let body_length = snapshot_u32(authenticated, &mut offset)?;
        if body_length == 0 || body_length > MAX_PROGRAM_RESPONSE_BYTES {
            return Err("emulator recovery move response exceeds its bound".to_owned());
        }
        let body = std::str::from_utf8(snapshot_take(authenticated, &mut offset, body_length)?)
            .map_err(|_| "emulator recovery move response is not UTF-8".to_owned())?
            .to_owned();
        let operation = MoveOperation {
            quote_id: quote_id.clone(),
            status,
            body,
        };
        if !valid_human_idempotency(&idempotency)
            || !matches!(status, 200 | 400 | 404 | 409 | 503)
            || serde_json::from_str::<serde_json::Value>(&operation.body).is_err()
            || move_quotes
                .get(&quote_id)
                .and_then(|quote| quote.committed_idempotency.as_deref())
                != Some(idempotency.as_str())
            || move_operations.insert(idempotency, operation).is_some()
        {
            return Err("emulator recovery move operation binding is invalid".to_owned());
        }
    }
    if move_quotes.values().any(|quote| {
        quote
            .committed_idempotency
            .as_ref()
            .is_some_and(|key| !move_operations.contains_key(key))
    }) {
        return Err("emulator recovery committed quote is missing its operation".to_owned());
    }
    if offset != authenticated.len() {
        return Err("emulator recovery snapshot has trailing bytes".to_owned());
    }
    Ok(RecoverySnapshot {
        core_snapshot,
        receipts,
        receipt_order,
        program_operations,
        program_activity_operations,
        accounts,
        move_quotes,
        move_operations,
    })
}

fn export_snapshot(emulator: &mut Emulator, trace: u64) -> Response {
    let mut bytes = ptr::null();
    let mut length = 0_usize;
    let code = unsafe {
        platform_emulator_snapshot_export(emulator.core, &raw mut bytes, &raw mut length)
    };
    if code != 0 {
        return core_response(trace, code);
    }
    if bytes.is_null() || length == 0 || length > MAX_CORE_SNAPSHOT_BYTES {
        return refusal(
            trace,
            503,
            "core_invalid_output",
            "the LayerX core returned an invalid snapshot buffer",
        );
    }
    let core_snapshot = unsafe { slice::from_raw_parts(bytes, length) };
    match encode_recovery_snapshot(
        core_snapshot,
        &emulator.receipts,
        &emulator.receipt_order,
        &emulator.program_operations,
        &emulator.program_activity_operations,
        &emulator.accounts,
        &emulator.move_quotes,
        &emulator.move_operations,
    ) {
        Ok(body) => Response {
            status: 200,
            content_type: "application/vnd.layerx.emulator-snapshot",
            body,
        },
        Err(error) => refusal(trace, 503, "snapshot_encoding_failed", &error),
    }
}

fn import_snapshot(emulator: &mut Emulator, body: &[u8], trace: u64) -> Response {
    let recovered = match decode_recovery_snapshot(body) {
        Ok(value) => value,
        Err(error) => return refusal(trace, 400, "invalid_snapshot", &error),
    };
    let mut prior_bytes = ptr::null();
    let mut prior_length = 0_usize;
    let prior_code = unsafe {
        platform_emulator_snapshot_export(
            emulator.core,
            &raw mut prior_bytes,
            &raw mut prior_length,
        )
    };
    if prior_code != 0
        || prior_bytes.is_null()
        || prior_length == 0
        || prior_length > MAX_CORE_SNAPSHOT_BYTES
    {
        return refusal(
            trace,
            503,
            "snapshot_rollback_unavailable",
            "current canonical state could not be retained before import",
        );
    }
    let prior_core = unsafe { slice::from_raw_parts(prior_bytes, prior_length) }.to_vec();
    let code = unsafe {
        platform_emulator_snapshot_import(
            emulator.core,
            recovered.core_snapshot.as_ptr(),
            recovered.core_snapshot.len(),
        )
    };
    if code != 0 {
        return core_response(trace, code);
    }
    let recovered_accounts_match = inspect_state(emulator)
        .ok()
        .is_some_and(|state| state.account_count == recovered.accounts.len())
        && recovered.accounts.iter().all(|(name, expected)| {
            core_account(emulator, name)
                .ok()
                .flatten()
                .is_some_and(|account| account.id == expected.id)
        });
    if !recovered_accounts_match {
        let rollback = unsafe {
            platform_emulator_snapshot_import(emulator.core, prior_core.as_ptr(), prior_core.len())
        };
        if rollback != 0 {
            return refusal(
                trace,
                503,
                "snapshot_rollback_failed",
                "invalid move metadata was refused but prior canonical state could not be restored",
            );
        }
        return refusal(
            trace,
            400,
            "invalid_snapshot",
            "recovered account metadata does not match canonical state",
        );
    }
    emulator.receipts = recovered.receipts;
    emulator.receipt_order = recovered.receipt_order;
    emulator.program_operations = recovered.program_operations;
    emulator.program_activity_operations = recovered.program_activity_operations;
    emulator.accounts = recovered.accounts;
    emulator.move_quotes = recovered.move_quotes;
    emulator.move_operations = recovered.move_operations;
    success(trace, "{\"imported\":true}")
}

fn advance_trace(trace: &mut u64) -> Option<u64> {
    let next = trace.checked_add(1)?;
    *trace = next;
    Some(next)
}

fn program_receipt(emulator: &Emulator, request: &Request, trace: u64) -> Response {
    let Some(idempotency) = request
        .path
        .strip_prefix("/v1/programs/receipts/by-idempotency/")
    else {
        return refusal(
            trace,
            404,
            "program_receipt_not_found",
            "program receipt route is invalid",
        );
    };
    if !canonical_hex32_text(idempotency) {
        return refusal(
            trace,
            400,
            "invalid_argument",
            "idempotency key must be canonical 32-byte hexadecimal",
        );
    }
    let expected_activity = match program_receipt_selector(request, idempotency) {
        Ok(value) => value,
        Err(error) => return refusal(trace, 400, "invalid_program_receipt_selector", &error),
    };
    match emulator.program_operations.get(idempotency) {
        Some(operation) if operation.activity_id == expected_activity => {
            stored_program_operation_response(trace, operation)
        }
        Some(_) => refusal(
            trace,
            409,
            "program_receipt_selector_mismatch",
            "the expected activity is not bound to this idempotency key",
        ),
        None => refusal(
            trace,
            404,
            "program_receipt_not_found",
            "program receipt is not present in this emulator process",
        ),
    }
}

fn program_activity(emulator: &Emulator, request: &Request, trace: u64) -> Response {
    let Some(activity_id) = request.path.strip_prefix("/v1/programs/activities/") else {
        return refusal(
            trace,
            404,
            "program_activity_not_found",
            "program activity route is invalid",
        );
    };
    if !canonical_hex32_text(activity_id) {
        return refusal(
            trace,
            400,
            "invalid_argument",
            "activity id must be canonical 32-byte hexadecimal",
        );
    }
    if let Err(error) = program_activity_selector(request, activity_id) {
        return refusal(trace, 400, "invalid_program_activity_selector", &error);
    }
    let operation = emulator
        .program_activity_operations
        .get(activity_id)
        .and_then(|idempotency| emulator.program_operations.get(idempotency));
    match operation {
        Some(operation) if operation.activity_id == activity_id => {
            stored_program_operation_response(trace, operation)
        }
        Some(_) => refusal(
            trace,
            404,
            "program_activity_not_found",
            "program activity is not present",
        ),
        None => refusal(
            trace,
            404,
            "program_activity_not_found",
            "program activity is not present in this emulator process",
        ),
    }
}

fn route(emulator: &mut Emulator, request: &Request) -> Response {
    let program_request = programs_request_path(&request.method, &request.path);
    let Some(trace) = advance_trace(&mut emulator.trace) else {
        let result = refusal(
            u64::MAX,
            503,
            "trace_exhausted",
            "the emulator trace space is exhausted",
        );
        return if program_request {
            agent_response(u64::MAX, result)
        } else {
            result
        };
    };
    let result = match (request.method.as_str(), request.path.as_str()) {
        ("GET", "/healthz") => health(emulator, trace),
        ("GET", "/v1/sequencer") => sequencer_identity(emulator, trace),
        ("POST", "/v1/activities") => submit(emulator, request, trace),
        ("POST", "/v1/moves/quote") => move_quote(emulator, request, trace),
        ("POST", "/v1/moves") => move_commit(emulator, request, trace),
        ("POST", "/v1/programs/call") => program_call(emulator, request, trace),
        ("POST", "/v1/programs/simulate") => program_simulate(emulator, request, trace),
        ("GET", path)
            if programs_route("GET", path)
                && path.starts_with("/v1/programs/receipts/by-idempotency/") =>
        {
            program_receipt(emulator, request, trace)
        }
        ("GET", path)
            if programs_route("GET", path) && path.starts_with("/v1/programs/activities/") =>
        {
            program_activity(emulator, request, trace)
        }
        ("GET", "/v1/state") => inspect(emulator, trace),
        ("GET", path)
            if programs_route("GET", path) && path.starts_with("/v1/programs/registry/") =>
        {
            program_registry_read(emulator, request, path, trace)
        }
        ("POST", "/__emulator/accounts/prefund") => prefund(emulator, request, trace),
        ("POST", "/__emulator/time/set") => update_time(emulator, request, trace, false),
        ("POST", "/__emulator/time/advance") => update_time(emulator, request, trace, true),
        ("POST", "/__emulator/faults") => inject_fault(emulator, request, trace),
        ("GET", "/__emulator/snapshot") => export_snapshot(emulator, trace),
        ("PUT", "/__emulator/snapshot") => import_snapshot(emulator, &request.body, trace),
        ("GET", path) if path.starts_with("/v1/receipts/") => {
            let id = &path[13..];
            match emulator.receipts.get(id) {
                Some(receipt) => success(
                    trace,
                    &format!(
                        "{{\"activity_id\":\"{}\",\"receipt\":\"{}\"}}",
                        escape_json(id),
                        receipt
                    ),
                ),
                None => refusal(
                    trace,
                    404,
                    "not_found",
                    "receipt is not present in this emulator process",
                ),
            }
        }
        (
            _,
            "/v1/sequencer"
            | "/v1/activities"
            | "/v1/moves/quote"
            | "/v1/moves"
            | "/v1/state"
            | "/__emulator/accounts/prefund"
            | "/__emulator/time/set"
            | "/__emulator/time/advance"
            | "/__emulator/faults"
            | "/__emulator/snapshot",
        ) => refusal(
            trace,
            405,
            "method_not_allowed",
            "method is not supported for this route",
        ),
        _ => refusal(trace, 404, "not_found", "route does not exist"),
    };
    if program_request {
        agent_response(trace, result)
    } else {
        result
    }
}

fn program_registry_read(
    emulator: &mut Emulator,
    request: &Request,
    path: &str,
    trace: u64,
) -> Response {
    let tail = &path[22..];
    let (program_text, interface_only) = match tail.strip_suffix("/interface") {
        Some(value) => (value, true),
        None => (tail, false),
    };
    if !canonical_hex32_text(program_text) {
        return refusal(
            trace,
            400,
            "invalid_argument",
            "program id must be canonical 32-byte hexadecimal",
        );
    }
    if let Err(error) = program_selector(request, program_text) {
        return refusal(trace, 400, "invalid_program_selector", &error);
    }
    let bytes = match hex_decode(program_text) {
        Ok(bytes) if bytes.len() == 32 => bytes,
        _ => {
            return refusal(
                trace,
                400,
                "invalid_argument",
                "program id must be 32-byte hex",
            )
        }
    };
    let mut program = CoreProgram {
        program_id: [0; 32],
        code_hash: [0; 32],
        deployment_receipt_digest: [0; 32],
        version: 0,
        abi_version: 0,
        lifecycle: 0,
        interface_bytes: ptr::null(),
        interface_length: 0,
        has_interface: 0,
        state_root: [0; 32],
        observed_sequence: 0,
    };
    let code =
        unsafe { platform_emulator_program_read(emulator.core, bytes.as_ptr(), &raw mut program) };
    if code != 0 {
        return program_head_error(trace, code);
    }
    let mut live = CoreState {
        canonical_state_root: [0; 32],
        receipt_state_root: [0; 32],
        next_sequence: 0,
        batch_number: 0,
        timestamp_ms: 0,
        cell_count: 0,
        account_count: 0,
    };
    let inspect_code = unsafe { platform_emulator_inspect(emulator.core, &raw mut live) };
    if inspect_code != 0 {
        return core_response(trace, inspect_code);
    }
    let valid_through = match live.timestamp_ms.checked_add(300_000) {
        Some(value) => value,
        None => return refusal(trace, 503, "core_invalid_output", "freshness overflow"),
    };
    let lifecycle = match program.lifecycle {
        1 => "active",
        2 => "deprecated",
        3 => "tombstoned",
        _ => {
            return refusal(
                trace,
                503,
                "core_invalid_output",
                "program lifecycle is invalid",
            )
        }
    };
    if !interface_only {
        let discovery = serde_json::json!({
            "program_id":hex_encode(&program.program_id),
            "lifecycle":lifecycle,
            "version":program.version,
            "code_hash":hex_encode(&program.code_hash),
            "abi_version":program.abi_version,
            "receipt_digest":hex_encode(&program.deployment_receipt_digest),
            "state_root":hex_encode(&program.state_root),
            "observed_sequence":program.observed_sequence.to_string(),
            "observed_at":live.timestamp_ms.to_string(),
            "valid_through":valid_through.to_string(),
            "verification":"registry-receipt-and-current-head-verified",
        });
        return success(trace, &discovery.to_string());
    }
    if program.has_interface == 0 {
        return refusal(
            trace,
            404,
            "program_interface_absent",
            "program has no published interface",
        );
    }
    if program.has_interface != 1
        || program.interface_bytes.is_null()
        || program.interface_length == 0
        || program.interface_length > 952
    {
        return refusal(
            trace,
            503,
            "core_invalid_output",
            "program interface state is invalid",
        );
    }
    let interface =
        unsafe { slice::from_raw_parts(program.interface_bytes, program.interface_length) };
    let interface_digest: [u8; 32] = Sha256::digest(interface).into();
    let interface = serde_json::json!({
        "program_id":hex_encode(&program.program_id),
        "version":program.version,
        "code_hash":hex_encode(&program.code_hash),
        "abi_version":program.abi_version,
        "interface":hex_encode(interface),
        "interface_digest":hex_encode(&interface_digest),
        "receipt_digest":hex_encode(&program.deployment_receipt_digest),
        "state_root":hex_encode(&program.state_root),
        "observed_sequence":program.observed_sequence.to_string(),
        "observed_at":live.timestamp_ms.to_string(),
        "valid_through":valid_through.to_string(),
        "source":{"status":"unpublished"},
        "verification":"deployment-interface-and-current-head-verified",
    });
    success(trace, &interface.to_string())
}

fn program_simulate(emulator: &mut Emulator, request: &Request, trace: u64) -> Response {
    let decoded = match decode_program_activity(request) {
        Ok(activity) => activity,
        Err(error) => return refusal(trace, 400, "invalid_program_call", &error),
    };
    let program_id = decoded.program_id;
    let head = match active_program_head(emulator, program_id, trace) {
        Ok(head) => head,
        Err(response) => return response,
    };
    let live = match inspect_state(emulator) {
        Ok(state) => state,
        Err(code) => return core_response(trace, code),
    };
    let mut receipt = CoreReceipt {
        activity_id: [0; 32],
        batch_id: [0; 32],
        state_root: [0; 32],
        previous_state_root: [0; 32],
        asset: [0; 32],
        sequencer_public_key: [0; 32],
        global_sequence: 0,
        result_code: 0,
        metered_cost_hi: 0,
        metered_cost_lo: 0,
        bytes: ptr::null(),
        length: 0,
        terminal_payload: ptr::null(),
        terminal_payload_length: 0,
        call_graph: ptr::null(),
        call_graph_length: 0,
        isolated_owner: ptr::null_mut(),
    };
    let code = unsafe {
        platform_emulator_simulate(
            emulator.core,
            decoded.signed.as_ptr(),
            decoded.signed.len(),
            &raw mut receipt,
        )
    };
    if code != 0 {
        return core_response(trace, code);
    }
    let material = match take_core_receipt(&mut receipt) {
        Ok(material) => material,
        Err(error) => return refusal(trace, 503, "core_invalid_output", &error),
    };
    let verified = match verify_program_execution(
        &material.receipt,
        &material.terminal_payload,
        &material.call_graph,
        ProgramExecutionExpectation {
            sequencer_public_key: emulator.signing_key.verifying_key().to_bytes(),
            previous_state_root: head.state_root,
            activity_id: decoded.activity_id,
            program_id,
            guest_abi_version: head.abi_version,
        },
    ) {
        Ok(verified)
            if verified
                .receipt()
                .receipt()
                .protocol()
                .is_some_and(|protocol| {
                    protocol.protocol_version() == decoded.protocol_version
                }) =>
        {
            verified
        }
        Ok(_) | Err(_) => {
            return refusal(
                trace,
                503,
                "program_simulation_unverified",
                "the simulated result did not verify against the discovered program head",
            )
        }
    };
    let execution = match verified_program_document(
        &verified,
        &material.terminal_payload,
        &material.call_graph,
        program_id,
        head.abi_version,
        emulator.signing_key.verifying_key().to_bytes(),
        "simulated",
        None,
    ) {
        Ok(document) => document,
        Err(error) => return refusal(trace, 503, "core_invalid_output", &error),
    };
    let signing = &emulator.signing_key;
    let mut boundary_material = b"LayerX/emulator/simulation-boundary/v1\0".to_vec();
    boundary_material.extend_from_slice(&signing.verifying_key().to_bytes());
    let boundary_id: [u8; 32] = Sha256::digest(&boundary_material).into();
    let mut evidence = b"LayerX/agent/program-simulation-evidence/v1\0".to_vec();
    evidence.extend_from_slice(&boundary_id);
    evidence.extend_from_slice(&decoded.activity_id);
    evidence.extend_from_slice(&head.state_root);
    let hypothetical_state_root = verified
        .receipt()
        .receipt()
        .protocol()
        .map(|protocol| protocol.resulting_state_root());
    let Some(hypothetical_state_root) = hypothetical_state_root else {
        return refusal(
            trace,
            503,
            "core_invalid_output",
            "verified simulation has no protocol state root",
        );
    };
    evidence.extend_from_slice(&hypothetical_state_root);
    evidence.extend_from_slice(&head.observed_sequence.to_be_bytes());
    evidence.extend_from_slice(&live.timestamp_ms.to_be_bytes());
    evidence.push(0);
    let evidence_digest: [u8; 32] = Sha256::digest(&evidence).into();
    let evidence_signature = signing.sign(&evidence_digest).to_bytes();
    let response = serde_json::json!({
        "committed":false,
        "execution":execution,
        "simulation_evidence":{
            "boundary_id":hex_encode(&boundary_id),
            "activity_id":hex_encode(&decoded.activity_id),
            "previous_state_root":hex_encode(&head.state_root),
            "hypothetical_state_root":hex_encode(&hypothetical_state_root),
            "observed_sequence":head.observed_sequence.to_string(),
            "observed_at":live.timestamp_ms.to_string(),
            "committed":false,
            "public_key":hex_encode(&signing.verifying_key().to_bytes()),
            "signature":hex_encode(&evidence_signature),
        },
    });
    success(trace, &response.to_string())
}

fn parse_amount(value: &str) -> Result<(u64, u64), String> {
    if let Some((hi, lo)) = value.split_once(':') {
        Ok((
            hi.parse().map_err(|_| "invalid amount high word")?,
            lo.parse().map_err(|_| "invalid amount low word")?,
        ))
    } else {
        Ok((0, value.parse().map_err(|_| "invalid amount")?))
    }
}

fn parse_prefund(value: &str) -> Result<Prefund, String> {
    let mut fields = value.split(',');
    let did = fields.next().ok_or("prefund requires did")?.to_string();
    let public = hex_decode(fields.next().ok_or("prefund requires public key")?)?;
    let (amount_hi, amount_lo) = parse_amount(fields.next().ok_or("prefund requires amount")?)?;
    if fields.next().is_some() || public.len() != 32 || did.is_empty() {
        return Err("prefund format is did,64-hex-public-key,amount-hi:amount-lo".into());
    }
    let mut public_key = [0_u8; 32];
    public_key.copy_from_slice(&public);
    Ok(Prefund {
        did,
        public_key,
        amount_hi,
        amount_lo,
    })
}

fn parse_config(arguments: impl IntoIterator<Item = String>) -> Result<Config, String> {
    let mut arguments = arguments.into_iter();
    if arguments.next().as_deref() != Some("up") {
        return Err("usage: layerx emulator up [--listen ADDRESS] [--network-id ID] [--time-ms MS] [--sequencer-seed-file PATH] [--prefund DID,PUBLIC_KEY,AMOUNT]".into());
    }
    let mut config = Config {
        listen: SocketAddr::from(([127, 0, 0, 1], DEFAULT_PORT)),
        network_id: DEFAULT_NETWORK_ID,
        timestamp_ms: DEFAULT_TIME_MS,
        prefunds: Vec::new(),
        sequencer_seed: None,
    };
    while let Some(argument) = arguments.next() {
        let value = arguments
            .next()
            .ok_or_else(|| format!("missing value for {argument}"))?;
        match argument.as_str() {
            "--listen" => {
                let listen = value
                    .parse::<SocketAddr>()
                    .map_err(|_| "listen address must be an IP socket address")?;
                if !listen.ip().is_loopback() {
                    return Err("emulator control routes may listen only on loopback".into());
                }
                config.listen = listen;
            }
            "--network-id" => {
                config.network_id = value.parse().map_err(|_| "network id must be an integer")?;
            }
            "--time-ms" => {
                config.timestamp_ms = value.parse().map_err(|_| "time must be an integer")?;
            }
            "--sequencer-seed-file" => {
                let bytes = std::fs::read(&value).map_err(|error| {
                    format!("could not read sequencer seed file {value}: {error}")
                })?;
                let seed = if bytes.len() == 32 {
                    bytes
                } else {
                    let text = std::str::from_utf8(&bytes).map_err(|_| {
                        "sequencer seed file must contain 32 raw bytes or 64 hexadecimal characters"
                    })?;
                    hex_decode(text.trim())?
                };
                config.sequencer_seed = Some(
                    seed.try_into()
                        .map_err(|_| "sequencer seed must be exactly 32 bytes")?,
                );
            }
            "--prefund" => config.prefunds.push(parse_prefund(&value)?),
            _ => return Err(format!("unknown argument: {argument}")),
        }
    }
    Ok(config)
}

/// Starts the local gateway adapter around the production `LayerX` transition.
fn platform_emulator(config: Config) -> Result<(), String> {
    layerx_programs_runtime::retain_host_ffi_exports();
    layerx_programs_sandbox::retain_host_ffi_exports();
    let seed = config.sequencer_seed.ok_or_else(|| {
        "--sequencer-seed-file is required; the emulator has no compiled-in signing authority"
            .to_owned()
    })?;
    if seed == [0; 32] {
        return Err("sequencer seed must not be zero".to_owned());
    }
    let core =
        unsafe { platform_emulator_create(config.network_id, config.timestamp_ms, seed.as_ptr()) };
    if core.is_null() {
        return Err("could not initialize the LayerX core".into());
    }
    let mut emulator = Emulator {
        core,
        signing_key: SigningKey::from_bytes(&seed),
        network_id: config.network_id,
        receipts: HashMap::new(),
        receipt_order: VecDeque::new(),
        program_operations: HashMap::new(),
        program_activity_operations: HashMap::new(),
        accounts: HashMap::new(),
        move_quotes: HashMap::new(),
        move_operations: HashMap::new(),
        trace: 0,
    };
    for prefund in config.prefunds {
        if let Err(status) = prefund_core(
            &mut emulator,
            &prefund.did,
            prefund.public_key,
            prefund.amount_hi,
            prefund.amount_lo,
        ) {
            return Err(format!(
                "prefund {} failed: {}",
                prefund.did,
                core_error(status)
            ));
        }
    }
    let listener = TcpListener::bind(config.listen)
        .map_err(|error| format!("cannot listen on {}: {error}", config.listen))?;
    eprintln!(
        "LayerX emulator ready on http://{} (network {}, deterministic time {})",
        config.listen, config.network_id, config.timestamp_ms
    );
    let (admission_sender, admission_receiver) = sync_channel::<TcpStream>(ADMISSION_CAPACITY);
    let admission_receiver = Arc::new(Mutex::new(admission_receiver));
    let (transition_sender, transition_receiver) =
        sync_channel::<ParsedConnection>(TRANSITION_CAPACITY);
    for index in 0..PARSER_WORKERS {
        let admission_receiver = Arc::clone(&admission_receiver);
        let transition_sender = transition_sender.clone();
        thread::Builder::new()
            .name(format!("layerx-emulator-http-{index}"))
            .spawn(move || loop {
                let stream = match admission_receiver.lock() {
                    Ok(receiver) => receiver.recv(),
                    Err(_) => return,
                };
                let Ok(mut stream) = stream else {
                    return;
                };
                if stream
                    .set_read_timeout(Some(IO_TIMEOUT))
                    .and_then(|()| stream.set_write_timeout(Some(IO_TIMEOUT)))
                    .is_err()
                {
                    continue;
                }
                let request = parse_request(&mut stream);
                let (response_sender, response_receiver) = sync_channel(1);
                if transition_sender
                    .send(ParsedConnection {
                        request,
                        response: response_sender,
                    })
                    .is_err()
                {
                    return;
                }
                let Ok(response) = response_receiver.recv() else {
                    return;
                };
                let _ = write_response(&mut stream, &response);
            })
            .map_err(|error| format!("could not start emulator HTTP worker {index}: {error}"))?;
    }
    drop(transition_sender);
    thread::Builder::new()
        .name("layerx-emulator-admission".to_owned())
        .spawn(move || {
            for connection in listener.incoming() {
                match connection {
                    Ok(stream) => match admission_sender.try_send(stream) {
                        Ok(()) => {}
                        Err(TrySendError::Full(mut stream)) => {
                            let _ = stream.set_write_timeout(Some(Duration::from_secs(2)));
                            let response = refusal(
                                0,
                                503,
                                "overloaded",
                                "the emulator HTTP admission queue is full",
                            );
                            let _ = write_response(&mut stream, &response);
                        }
                        Err(TrySendError::Disconnected(_)) => return,
                    },
                    Err(error) => eprintln!("emulator connection error: {error}"),
                }
            }
        })
        .map_err(|error| format!("could not start emulator admission worker: {error}"))?;
    for work in transition_receiver {
        let response = match work.request {
            Ok(request) => route(&mut emulator, &request),
            Err(error) => {
                if let Some(trace) = advance_trace(&mut emulator.trace) {
                    refusal(trace, 400, "invalid_request", &error)
                } else {
                    refusal(
                        u64::MAX,
                        503,
                        "trace_exhausted",
                        "the emulator trace space is exhausted",
                    )
                }
            }
        };
        let _ = work.response.send(response);
    }
    Err("all emulator HTTP workers stopped".into())
}

/// Runs the emulator subcommand behind the unified `layerx` CLI dispatcher.
///
/// # Errors
///
/// Returns configuration, core-initialisation, or listener failures without
/// terminating the owning CLI process.
pub fn run(arguments: impl IntoIterator<Item = String>) -> Result<(), String> {
    parse_config(arguments).and_then(platform_emulator)
}

#[cfg(test)]
mod boundary_tests {
    use super::{advance_trace, hex_encode, parse_config, sequencer_identity_body};

    #[test]
    fn sequencer_identity_advertises_the_public_key_and_never_the_seed() {
        let seed = [0x42_u8; 32];
        let public_key = ed25519_dalek::SigningKey::from_bytes(&seed)
            .verifying_key()
            .to_bytes();
        let body = sequencer_identity_body(402, &public_key);
        assert_eq!(
            body,
            format!(
                "{{\"network_id\":402,\"sequencer_public_key\":\"{}\"}}",
                hex_encode(&public_key)
            )
        );
        assert!(!body.contains(&hex_encode(&seed)));
    }

    #[test]
    fn control_surface_refuses_non_loopback_listeners() {
        assert!(parse_config([
            "up".to_owned(),
            "--listen".to_owned(),
            "0.0.0.0:9402".to_owned()
        ])
        .is_err());
        assert!(parse_config([
            "up".to_owned(),
            "--listen".to_owned(),
            "192.0.2.1:9402".to_owned()
        ])
        .is_err());
    }

    #[test]
    fn control_surface_accepts_ipv4_and_ipv6_loopback() {
        assert!(parse_config([
            "up".to_owned(),
            "--listen".to_owned(),
            "127.0.0.1:0".to_owned()
        ])
        .is_ok());
        assert!(
            parse_config(["up".to_owned(), "--listen".to_owned(), "[::1]:0".to_owned()]).is_ok()
        );
    }

    #[test]
    fn trace_exhaustion_never_wraps() {
        let mut trace = u64::MAX - 1;
        assert_eq!(advance_trace(&mut trace), Some(u64::MAX));
        assert_eq!(advance_trace(&mut trace), None);
        assert_eq!(trace, u64::MAX);
    }
}

#[cfg(test)]
mod program_call_tests {
    use super::{
        agent_error_class, agent_response, decode_activity, decode_program_activity,
        decode_recovery_snapshot, encode_recovery_snapshot, hex_decode, program_activity_selector,
        program_receipt_selector, program_selector, programs_route, refusal,
        stored_program_operation_response, success, EmulatorAccount, MoveOperation,
        MoveQuoteRecord, ProgramOperation, Request, MAX_RECEIPT_BYTES,
    };
    use std::collections::{HashMap, VecDeque};

    /// A representative canonical program-call activity. The value only has to
    /// be the exact bytes both ingress forms carry unchanged; the emulator hands
    /// these same bytes to the real transition on every path.
    const CANONICAL_ACTIVITY_HEX: &str = "4c61796572582f70726f6772616d732f63616c6c2f763100111111111111111111111111111111111111111111111111111111111111111100000000000003e8000000000000000000000000000000fa0002010300000002aabb";

    fn json_request(hex: &str) -> Request {
        Request {
            method: "POST".to_string(),
            path: "/v1/programs/call".to_string(),
            content_type: "application/json".to_string(),
            idempotency_key: None,
            body: format!("{{\"activity\":\"{hex}\"}}").into_bytes(),
        }
    }

    fn octet_request(bytes: &[u8]) -> Request {
        Request {
            method: "POST".to_string(),
            path: "/v1/programs/call".to_string(),
            content_type: "application/octet-stream".to_string(),
            idempotency_key: None,
            body: bytes.to_vec(),
        }
    }

    #[test]
    fn both_ingress_forms_feed_identical_bytes_to_the_transition() {
        let Ok(expected) = hex_decode(CANONICAL_ACTIVITY_HEX) else {
            panic!("golden activity hex did not decode");
        };
        let Ok(from_json) = decode_activity(&json_request(CANONICAL_ACTIVITY_HEX)) else {
            panic!("program-call activity hex did not decode");
        };
        let from_octets = match decode_activity(&octet_request(&expected)) {
            Ok(bytes) => bytes,
            Err(_) => panic!("octet-stream program-call activity did not decode"),
        };
        assert_eq!(from_json, expected);
        assert_eq!(from_octets, expected);
        assert_eq!(from_json, from_octets);
    }

    #[test]
    fn missing_activity_is_rejected() {
        let request = Request {
            method: "POST".to_string(),
            path: "/v1/programs/call".to_string(),
            content_type: "application/json".to_string(),
            idempotency_key: None,
            body: b"{}".to_vec(),
        };
        assert!(decode_activity(&request).is_err());
    }

    #[test]
    fn duplicate_or_unknown_activity_fields_are_rejected() {
        let duplicate = Request {
            method: "POST".to_string(),
            path: "/v1/programs/call".to_string(),
            content_type: "application/json".to_string(),
            idempotency_key: None,
            body: b"{\"activity\":\"00\",\"activity\":\"11\"}".to_vec(),
        };
        let unknown = Request {
            method: "POST".to_string(),
            path: "/v1/programs/call".to_string(),
            content_type: "application/json".to_string(),
            idempotency_key: None,
            body: b"{\"activity\":\"00\",\"trusted\":true}".to_vec(),
        };
        assert!(decode_activity(&duplicate).is_err());
        assert!(decode_activity(&unknown).is_err());
    }

    #[test]
    fn program_routes_share_the_exact_agent_envelope() {
        for state in ["refused", "unknown", "pending", "executed"] {
            let mut inner = success(
                7,
                &format!(
                    "{{\"state\":\"{state}\",\"global_sequence\":{},\"usage\":{{\"output_values\":\"7\"}}}}",
                    u64::MAX
                ),
            );
            if matches!(state, "unknown" | "pending") {
                inner.status = 202;
            }
            let output = agent_response(7, inner);
            let document: serde_json::Value = serde_json::from_slice(&output.body).unwrap();
            let verification_status = if matches!(state, "unknown" | "pending") {
                serde_json::json!({
                    "state":"Unverified",
                    "requested":"SequencerSigned",
                    "achieved":"Unverified",
                    "reason":"receipt_pending",
                })
            } else {
                serde_json::json!({"state":"Achieved","level":"SequencerSigned"})
            };
            assert_eq!(
                document,
                serde_json::json!({
                    "request_id":"emu-0000000000000007",
                    "value":{
                        "state":state,
                        "global_sequence":u64::MAX.to_string(),
                        "usage":{"output_values":7}
                    },
                    "verification_status":verification_status,
                })
            );
        }

        let output = agent_response(
            8,
            refusal(8, 409, "idempotency_conflict", "different activity"),
        );
        let document: serde_json::Value = serde_json::from_slice(&output.body).unwrap();
        assert_eq!(
            document,
            serde_json::json!({
                "class":"IdempotencyConflict",
                "protocol_result_code":null,
                "retriability":"Terminal",
                "request_id":"emu-0000000000000008",
                "reason":"idempotency_conflict",
            })
        );
    }

    #[test]
    fn discovery_is_explicitly_server_verified_only() {
        for verification in [
            "registry-receipt-and-current-head-verified",
            "deployment-interface-and-current-head-verified",
        ] {
            let output = agent_response(
                9,
                success(
                    9,
                    &serde_json::json!({
                        "program_id":"a".repeat(64),
                        "verification":verification
                    })
                    .to_string(),
                ),
            );
            let document: serde_json::Value = serde_json::from_slice(&output.body).unwrap();
            assert_eq!(
                document["verification_status"],
                serde_json::json!({
                    "state":"Unverified",
                    "requested":"SequencerSigned",
                    "achieved":"Unverified",
                    "reason":"server_side_receipt_verification_only",
                })
            );
        }
    }

    #[test]
    fn retained_unknown_operation_is_recoverable_without_a_false_signature_claim() {
        let operation = ProgramOperation {
            activity_id: "a".repeat(64),
            response: serde_json::json!({
                "state":"unknown",
                "activity_id":"a".repeat(64),
                "idempotency_key":"b".repeat(64),
                "retained_signed_activity":"00ff",
            })
            .to_string(),
            retained_signed_activity: Some("00ff".to_owned()),
        };
        let stored = stored_program_operation_response(10, &operation);
        assert_eq!(stored.status, 202);
        let output = agent_response(10, stored);
        let document: serde_json::Value = serde_json::from_slice(&output.body).unwrap();
        assert_eq!(document["value"]["retained_signed_activity"], "00ff");
        assert_eq!(
            document["verification_status"],
            serde_json::json!({
                "state":"Unverified",
                "requested":"SequencerSigned",
                "achieved":"Unverified",
                "reason":"receipt_pending",
            })
        );
    }

    #[test]
    fn program_error_classes_are_stable_for_provider_parity() {
        for (status, code, expected) in [
            (409, "idempotency_conflict", "IdempotencyConflict"),
            (429, "quota_exceeded", "RateLimit"),
            (403, "activity_authorization_refused", "PolicyRefusal"),
            (
                503,
                "program_receipt_verification_failed",
                "VerificationFailure",
            ),
            (404, "program_interface_absent", "UnavailableCapability"),
            (400, "LXP_ERR_BUDGET_EXCEEDED", "CoreRejection"),
            (400, "invalid_argument", "ProtocolIncompatibility"),
            (503, "persistence_unavailable", "TransportFailure"),
            (400, "program_request_failed", "InternalFault"),
        ] {
            assert_eq!(agent_error_class(status, code), expected);
        }
    }

    #[test]
    fn recovery_snapshot_is_deterministic_and_preserves_program_indexes() {
        let receipt_activity = "a".repeat(64);
        let program_activity = "b".repeat(64);
        let idempotency = "c".repeat(64);
        let receipts = HashMap::from([
            (receipt_activity.clone(), "deadbeef".to_owned()),
            (program_activity.clone(), "01020304".to_owned()),
        ]);
        let receipt_order = [receipt_activity, program_activity.clone()]
            .into_iter()
            .collect::<VecDeque<_>>();
        let response = serde_json::json!({
            "state":"executed",
            "activity_id":program_activity.as_str(),
            "idempotency_key":idempotency.as_str(),
        })
        .to_string();
        let program_operations = HashMap::from([(
            idempotency.clone(),
            ProgramOperation {
                activity_id: program_activity.clone(),
                response: response.clone(),
                retained_signed_activity: Some("00ff".to_owned()),
            },
        )]);
        let program_activity_operations =
            HashMap::from([(program_activity.clone(), idempotency.clone())]);
        let source_name = "agent:did:layerx:source:main".to_owned();
        let destination_name = "agent:did:layerx:destination:main".to_owned();
        let accounts = HashMap::from([
            (
                source_name.clone(),
                EmulatorAccount {
                    id: [1; 32],
                    did: "did:layerx:source".to_owned(),
                    public_key: [2; 32],
                },
            ),
            (
                destination_name.clone(),
                EmulatorAccount {
                    id: [3; 32],
                    did: "did:layerx:destination".to_owned(),
                    public_key: [4; 32],
                },
            ),
        ]);
        let move_idempotency = "move-operation-key-0001".to_owned();
        let quote_id = format!("qte_{}", "d".repeat(64));
        let move_quotes = HashMap::from([(
            quote_id.clone(),
            MoveQuoteRecord {
                quote_id: quote_id.clone(),
                source: source_name,
                destination: destination_name,
                source_id: [1; 32],
                destination_id: [3; 32],
                amount: 9,
                currency: "LXP".to_owned(),
                created_at: 1_700_000_000_000,
                expires_at: 1_700_000_300_000,
                identity_sequence: 7,
                source_sequence: 8,
                committed_idempotency: Some(move_idempotency.clone()),
            },
        )]);
        let move_operations = HashMap::from([(
            move_idempotency,
            MoveOperation {
                quote_id,
                status: 200,
                body: serde_json::json!({"ok":true,"result":{"state":"done"}}).to_string(),
            },
        )]);

        let first = encode_recovery_snapshot(
            b"bounded-core-snapshot",
            &receipts,
            &receipt_order,
            &program_operations,
            &program_activity_operations,
            &accounts,
            &move_quotes,
            &move_operations,
        )
        .unwrap();
        let second = encode_recovery_snapshot(
            b"bounded-core-snapshot",
            &receipts,
            &receipt_order,
            &program_operations,
            &program_activity_operations,
            &accounts,
            &move_quotes,
            &move_operations,
        )
        .unwrap();
        assert_eq!(first, second);

        let recovered = decode_recovery_snapshot(&first).unwrap();
        assert_eq!(recovered.core_snapshot.as_slice(), b"bounded-core-snapshot");
        assert_eq!(recovered.receipts, receipts);
        assert_eq!(recovered.receipt_order, receipt_order);
        assert_eq!(recovered.program_operations, program_operations);
        assert_eq!(
            recovered.program_activity_operations,
            program_activity_operations
        );
        assert_eq!(recovered.accounts, accounts);
        assert_eq!(recovered.move_quotes, move_quotes);
        assert_eq!(recovered.move_operations, move_operations);
    }

    #[test]
    fn recovery_snapshot_rejects_tampering_and_inconsistent_bindings() {
        let activity = "a".repeat(64);
        let idempotency = "b".repeat(64);
        let receipts = HashMap::from([(activity.clone(), "00".to_owned())]);
        let receipt_order = [activity.clone()].into_iter().collect::<VecDeque<_>>();
        let response = serde_json::json!({
            "state":"executed",
            "activity_id":activity.as_str(),
            "idempotency_key":idempotency.as_str(),
        })
        .to_string();
        let operations = HashMap::from([(
            idempotency,
            ProgramOperation {
                activity_id: activity.clone(),
                response,
                retained_signed_activity: None,
            },
        )]);
        let bindings = HashMap::from([(activity, "c".repeat(64))]);
        let accounts = HashMap::new();
        let move_quotes = HashMap::new();
        let move_operations = HashMap::new();
        assert!(encode_recovery_snapshot(
            b"bounded-core-snapshot",
            &receipts,
            &receipt_order,
            &operations,
            &bindings,
            &accounts,
            &move_quotes,
            &move_operations,
        )
        .is_err());

        let empty_operations = HashMap::new();
        let empty_bindings = HashMap::new();
        let mut encoded = encode_recovery_snapshot(
            b"bounded-core-snapshot",
            &receipts,
            &receipt_order,
            &empty_operations,
            &empty_bindings,
            &accounts,
            &move_quotes,
            &move_operations,
        )
        .unwrap();
        let last = encoded.len() - 1;
        encoded[last] ^= 1;
        assert!(decode_recovery_snapshot(&encoded).is_err());
    }

    #[test]
    fn emulator_exposes_only_the_six_program_operation_routes() {
        let id = "a".repeat(64);
        assert!(programs_route("POST", "/v1/programs/call"));
        assert!(programs_route("POST", "/v1/programs/simulate"));
        assert!(programs_route(
            "GET",
            &format!("/v1/programs/registry/{id}")
        ));
        assert!(programs_route(
            "GET",
            &format!("/v1/programs/registry/{id}/interface")
        ));
        assert!(programs_route(
            "GET",
            &format!("/v1/programs/receipts/by-idempotency/{id}")
        ));
        assert!(programs_route(
            "GET",
            &format!("/v1/programs/activities/{id}")
        ));
        assert!(!programs_route("GET", "/v1/programs/registry"));
        assert!(!programs_route(
            "GET",
            &format!("/v1/programs/registry/{id}/source")
        ));
        assert!(!programs_route(
            "GET",
            &format!("/v1/programs/activities/{}", "A".repeat(64))
        ));
    }

    #[test]
    fn program_get_selectors_bind_route_and_expected_activity() {
        let program = "a".repeat(64);
        let discovery = Request {
            method: "GET".to_owned(),
            path: format!("/v1/programs/registry/{program}"),
            content_type: "application/json".to_owned(),
            idempotency_key: None,
            body: serde_json::json!({
                "program_id":program.as_str(),
                "requested_verification_level":"sequencer-signed",
            })
            .to_string()
            .into_bytes(),
        };
        assert!(program_selector(&discovery, &program).is_ok());

        let idempotency = "b".repeat(64);
        let activity = "c".repeat(64);
        let receipt = Request {
            method: "GET".to_owned(),
            path: format!("/v1/programs/receipts/by-idempotency/{idempotency}"),
            content_type: "application/json".to_owned(),
            idempotency_key: None,
            body: serde_json::json!({
                "idempotency_key":idempotency.as_str(),
                "expected_activity_id":activity.as_str(),
                "requested_verification_level":"sequencer-signed",
            })
            .to_string()
            .into_bytes(),
        };
        assert_eq!(
            program_receipt_selector(&receipt, &idempotency).as_deref(),
            Ok(activity.as_str())
        );
        assert!(program_receipt_selector(&receipt, &"d".repeat(64)).is_err());

        let lookup = Request {
            method: "GET".to_owned(),
            path: format!("/v1/programs/activities/{activity}"),
            content_type: "application/json".to_owned(),
            idempotency_key: None,
            body: serde_json::json!({
                "activity_id":activity.as_str(),
                "requested_verification_level":"sequencer-signed",
            })
            .to_string()
            .into_bytes(),
        };
        assert!(program_activity_selector(&lookup, &activity).is_ok());
    }

    #[test]
    fn signed_program_activity_is_bounded_to_one_mebibyte() {
        let request = Request {
            method: "POST".to_owned(),
            path: "/v1/programs/call".to_owned(),
            content_type: "application/octet-stream".to_owned(),
            idempotency_key: Some("a".repeat(64)),
            body: vec![0; MAX_RECEIPT_BYTES + 1],
        };
        assert!(decode_program_activity(&request).is_err());
    }
}
