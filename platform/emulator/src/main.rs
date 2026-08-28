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

use serde::de::DeserializeOwned;
use serde::Deserialize;
use sha2::{Digest as _, Sha256};
use ed25519_dalek::{Signer as _, SigningKey};

const DEFAULT_PORT: u16 = 9402;
const DEFAULT_NETWORK_ID: u32 = 402;
const DEFAULT_TIME_MS: u64 = 1_700_000_000_000;
const MAX_HEADER_BYTES: usize = 32 * 1024;
const MAX_REQUEST_BYTES: usize = 64 * 1024 * 1024;
const MAX_JSON_BYTES: usize = 1024 * 1024;
const IO_TIMEOUT: Duration = Duration::from_secs(10);
const PARSER_WORKERS: usize = 8;
const ADMISSION_CAPACITY: usize = 32;
const TRANSITION_CAPACITY: usize = 32;
const MAX_RECEIPTS: usize = 4096;
const MAX_RECEIPT_BYTES: usize = 64 * 1024;

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
    state_root: [u8; 32],
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
    version: c_uint,
    abi_version: u16,
    interface_bytes: *const c_uchar,
    interface_length: usize,
    has_interface: u8,
    state_root: [u8; 32],
    observed_sequence: c_ulonglong,
}

unsafe extern "C" {
    fn platform_emulator_create(network_id: c_uint, timestamp_ms: c_ulonglong, sequencer_seed: *const c_uchar) -> *mut c_void;
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
    fn platform_emulator_program_count(emulator: *const c_void) -> usize;
    fn platform_emulator_program_at(emulator: *const c_void, index: usize, program_id: *mut c_uchar) -> c_int;
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
    receipts: HashMap<String, String>,
    receipt_order: VecDeque<String>,
    trace: u64,
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
struct PrefundBody {
    did: String,
    public_key: String,
    #[serde(default)]
    amount_hi: u64,
    amount_lo: u64,
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
        || target.split('/').any(|segment| matches!(segment, "." | ".."))
    {
        return Err("invalid request line".into());
    }
    let path = target.to_string();
    let mut content_length = 0_usize;
    let mut has_content_length = false;
    let mut content_type = String::new();
    let mut has_content_type = false;
    let mut has_host = false;
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
        if value.bytes().any(|byte| byte.is_ascii_control() && byte != b'\t') {
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
    if !matches!(method, "POST" | "PUT") && content_length != 0 {
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
        body: bytes[header_end..header_end + content_length].to_vec(),
    })
}

fn write_response(stream: &mut TcpStream, response: &Response) -> std::io::Result<()> {
    let reason = match response.status {
        200 => "OK",
        400 => "Bad Request",
        404 => "Not Found",
        405 => "Method Not Allowed",
        413 => "Content Too Large",
        503 => "Service Unavailable",
        _ => "Error",
    };
    write!(stream, "HTTP/1.1 {} {}\r\nContent-Type: {}\r\nContent-Length: {}\r\nCache-Control: no-store\r\nConnection: close\r\n\r\n", response.status, reason, response.content_type, response.body.len())?;
    stream.write_all(&response.body)
}

fn submit(emulator: &mut Emulator, request: &Request, trace: u64) -> Response {
    let media_type = request.content_type.split(';').next().unwrap_or_default().trim();
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
        call_graph: ptr::null(), call_graph_length: 0,
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
    if receipt.bytes.is_null()
        || receipt.length == 0
        || receipt.length > MAX_RECEIPT_BYTES
    {
        return refusal(
            trace,
            503,
            "core_invalid_output",
            "the LayerX core returned an invalid receipt buffer",
        );
    }
    let encoded = unsafe { slice::from_raw_parts(receipt.bytes, receipt.length) };
    if receipt.terminal_payload_length > MAX_RECEIPT_BYTES
        || (receipt.terminal_payload_length != 0 && receipt.terminal_payload.is_null())
    { return refusal(trace, 503, "core_invalid_output", "terminal payload availability is invalid"); }
    let terminal_payload = if receipt.terminal_payload_length == 0 { &[] } else { unsafe { slice::from_raw_parts(receipt.terminal_payload, receipt.terminal_payload_length) } };
    if receipt.call_graph_length > MAX_RECEIPT_BYTES || (receipt.call_graph_length != 0 && receipt.call_graph.is_null()) { return refusal(trace,503,"core_invalid_output","call graph availability is invalid"); }
    let _call_graph = if receipt.call_graph_length == 0 { &[] } else { unsafe { slice::from_raw_parts(receipt.call_graph,receipt.call_graph_length) } };
    let receipt_hex = hex_encode(encoded);
    let activity_id = hex_encode(&receipt.activity_id);
    remember_receipt(emulator, activity_id.clone(), receipt_hex.clone());
    success(trace, &format!("{{\"activity_id\":\"{activity_id}\",\"batch_id\":\"{}\",\"global_sequence\":{},\"result_code\":{},\"state_root\":\"{}\",\"receipt\":\"{receipt_hex}\"}}", hex_encode(&receipt.batch_id), receipt.global_sequence, receipt.result_code, hex_encode(&receipt.state_root)))
}

fn decode_activity(request: &Request) -> Result<Vec<u8>, String> {
    let media_type = request.content_type.split(';').next().unwrap_or_default().trim();
    if media_type == "application/octet-stream" {
        Ok(request.body.clone())
    } else if media_type == "application/json" {
        decode_json::<ActivityBody>(request).and_then(|value| hex_decode(&value.activity))
    } else {
        Err("program-call content type is not supported".into())
    }
}

/// Runs one Programs CALL activity through the same real transition function as
/// every other activity. The emulator holds no mock call path: a call submitted
/// here and the identical canonical activity submitted to the network execute
/// the exact same transition, so a local call and a network call differ only in
/// the state they run against. The receipt is stored and the typed outcome is
/// derived from the receipt's own result code, never invented beside it.
fn program_call(emulator: &mut Emulator, request: &Request, trace: u64) -> Response {
    let activity = match decode_activity(request) {
        Ok(activity) if !activity.is_empty() => activity,
        Ok(_) => {
            return refusal(
                trace,
                400,
                "invalid_argument",
                "program call activity must not be empty",
            )
        }
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
        call_graph: ptr::null(), call_graph_length: 0,
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
    if receipt.bytes.is_null()
        || receipt.length == 0
        || receipt.length > MAX_RECEIPT_BYTES
    {
        return refusal(
            trace,
            503,
            "core_invalid_output",
            "the LayerX core returned an invalid receipt buffer",
        );
    }
    let encoded = unsafe { slice::from_raw_parts(receipt.bytes, receipt.length) };
    if receipt.terminal_payload_length > MAX_RECEIPT_BYTES
        || (receipt.terminal_payload_length != 0 && receipt.terminal_payload.is_null())
    { return refusal(trace, 503, "core_invalid_output", "terminal payload availability is invalid"); }
    let terminal_payload = if receipt.terminal_payload_length == 0 { &[] } else { unsafe { slice::from_raw_parts(receipt.terminal_payload, receipt.terminal_payload_length) } };
    let receipt_hex = hex_encode(encoded);
    if receipt.call_graph_length > MAX_RECEIPT_BYTES || (receipt.call_graph_length != 0 && receipt.call_graph.is_null()) { return refusal(trace,503,"core_invalid_output","call graph availability is invalid"); }
    let call_graph = if receipt.call_graph_length == 0 { &[] } else { unsafe { slice::from_raw_parts(receipt.call_graph,receipt.call_graph_length) } };
    let receipt_digest: [u8; 32] = Sha256::digest(encoded).into();
    let metered_cost = (u128::from(receipt.metered_cost_hi) << 64)
        | u128::from(receipt.metered_cost_lo);
    let activity_id = hex_encode(&receipt.activity_id);
    remember_receipt(emulator, activity_id.clone(), receipt_hex.clone());
    let outcome = if receipt.result_code >= 0 {
        format!(
            "{{\"status\":\"completed\",\"code\":{}}}",
            receipt.result_code
        )
    } else {
        format!(
            "{{\"status\":\"refused\",\"failure\":{{\"result_code\":{}}}}}",
            receipt.result_code
        )
    };
    success(trace, &format!("{{\"activity_kind\":\"program-call\",\"transition\":\"real\",\"committed\":true,\"activity_id\":\"{activity_id}\",\"batch_id\":\"{}\",\"global_sequence\":{},\"result_code\":{},\"metered_cost\":\"{metered_cost}\",\"previous_state_root\":\"{}\",\"state_root\":\"{}\",\"asset\":\"{}\",\"sequencer_public_key\":\"{}\",\"receipt\":\"{receipt_hex}\",\"receipt_digest\":\"{}\",\"terminal_payload\":\"{}\",\"call_graph\":\"{}\",\"outcome\":{outcome}}}", hex_encode(&receipt.batch_id), receipt.global_sequence, receipt.result_code, hex_encode(&receipt.previous_state_root), hex_encode(&receipt.state_root), hex_encode(&receipt.asset), hex_encode(&receipt.sequencer_public_key), hex_encode(&receipt_digest), hex_encode(terminal_payload),hex_encode(call_graph)))
}

fn inspect(emulator: &Emulator, trace: u64) -> Response {
    let mut state = CoreState {
        state_root: [0; 32],
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
        let code = unsafe {
            platform_emulator_account(
                emulator.core,
                index,
                id.as_mut_ptr(),
                &raw mut name,
                &raw mut name_length,
                &raw mut hi,
                &raw mut lo,
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
            "{{\"id\":\"{}\",\"name\":\"{}\",\"balance_hi\":{hi},\"balance_lo\":{lo}}}",
            hex_encode(&id),
            escape_json(&account_name)
        );
    }
    success(trace, &format!("{{\"network_mode\":\"emulator\",\"batch_cadence\":\"instant\",\"state_root\":\"{}\",\"next_sequence\":{},\"batch_number\":{},\"timestamp_ms\":{},\"cells\":[{cells}],\"accounts\":[{accounts}]}}", hex_encode(&state.state_root), state.next_sequence, state.batch_number, state.timestamp_ms))
}

fn health(emulator: &Emulator, trace: u64) -> Response {
    let mut state = CoreState {
        state_root: [0; 32],
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
    let code = unsafe {
        platform_emulator_prefund(
            emulator.core,
            body.did.as_ptr(),
            body.did.len(),
            key.as_ptr(),
            body.amount_hi,
            body.amount_lo,
        )
    };
    if code != 0 {
        return core_response(trace, code);
    }
    success(trace, "{\"prefunded\":true}")
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
        return refusal(trace, 400, "invalid_argument", "fault count is outside its bound");
    }
    let code = unsafe { platform_emulator_inject_failure(emulator.core, kind, body.count) };
    if code == 0 {
        success(trace, "{\"configured\":true}")
    } else {
        core_response(trace, code)
    }
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
    if bytes.is_null() || length == 0 || length > MAX_REQUEST_BYTES {
        return refusal(
            trace,
            503,
            "core_invalid_output",
            "the LayerX core returned an invalid snapshot buffer",
        );
    }
    Response {
        status: 200,
        content_type: "application/vnd.layerx.emulator-snapshot",
        body: unsafe { slice::from_raw_parts(bytes, length) }.to_vec(),
    }
}

fn import_snapshot(emulator: &mut Emulator, body: &[u8], trace: u64) -> Response {
    let code =
        unsafe { platform_emulator_snapshot_import(emulator.core, body.as_ptr(), body.len()) };
    if code != 0 {
        return core_response(trace, code);
    }
    emulator.receipts.clear();
    emulator.receipt_order.clear();
    success(trace, "{\"imported\":true}")
}

fn advance_trace(trace: &mut u64) -> Option<u64> {
    let next = trace.checked_add(1)?;
    *trace = next;
    Some(next)
}

fn route(emulator: &mut Emulator, request: &Request) -> Response {
    let Some(trace) = advance_trace(&mut emulator.trace) else {
        return refusal(
            u64::MAX,
            503,
            "trace_exhausted",
            "the emulator trace space is exhausted",
        );
    };
    match (request.method.as_str(), request.path.as_str()) {
        ("GET", "/healthz") => health(emulator, trace),
        ("POST", "/v1/activities") => submit(emulator, request, trace),
        ("POST", "/v1/programs/call") => program_call(emulator, request, trace),
        ("POST", "/v1/programs/simulate") => program_simulate(emulator, request, trace),
        ("GET", "/v1/state") => inspect(emulator, trace),
        ("GET", "/v1/programs/registry") => program_registry_list(emulator, trace),
        ("GET", path) if path.starts_with("/v1/programs/registry/") => {
            program_registry_read(emulator, path, trace)
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
            "/v1/activities"
            | "/v1/programs/registry"
            | "/v1/programs/call"
            | "/v1/programs/simulate"
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
    }
}

fn program_registry_list(emulator: &Emulator, trace: u64) -> Response {
    let count = unsafe { platform_emulator_program_count(emulator.core) };
    let mut identifiers = Vec::with_capacity(count);
    for index in 0..count {
        let mut id = [0_u8; 32];
        if unsafe { platform_emulator_program_at(emulator.core, index, id.as_mut_ptr()) } != 0 {
            return refusal(trace, 503, "core_invalid_output", "program registry enumeration failed");
        }
        identifiers.push(format!("\"{}\"", hex_encode(&id)));
    }
    success(trace, &format!("{{\"program_ids\":[{}]}}", identifiers.join(",")))
}

fn program_registry_read(emulator: &mut Emulator, path: &str, trace: u64) -> Response {
    let tail = &path[22..];
    let (program_text, interface_only) = match tail.strip_suffix("/interface") {
        Some(value) => (value, true),
        None => (tail, false),
    };
    let bytes = match hex_decode(program_text) {
        Ok(bytes) if bytes.len() == 32 => bytes,
        _ => return refusal(trace, 400, "invalid_argument", "program id must be 32-byte hex"),
    };
    let mut program = CoreProgram {
        program_id: [0; 32], code_hash: [0; 32], version: 0, abi_version: 0,
        interface_bytes: ptr::null(), interface_length: 0, has_interface: 0,
        state_root: [0; 32],
        observed_sequence: 0,
    };
    let code = unsafe { platform_emulator_program_read(emulator.core, bytes.as_ptr(), &raw mut program) };
    if code != 0 { return core_response(trace, code); }
    let mut live = CoreState { state_root: [0; 32], next_sequence: 0,
        batch_number: 0, timestamp_ms: 0, cell_count: 0, account_count: 0 };
    let inspect_code = unsafe { platform_emulator_inspect(emulator.core, &raw mut live) };
    if inspect_code != 0 { return core_response(trace, inspect_code); }
    let valid_through = match live.timestamp_ms.checked_add(300_000) {
        Some(value) => value, None => return refusal(trace, 503, "core_invalid_output", "freshness overflow"),
    };
    let mut discovery = b"LayerX/program-discovery-proof/v1\0".to_vec();
    discovery.extend_from_slice(&program.program_id); discovery.push(1);
    discovery.extend_from_slice(&program.version.to_be_bytes()); discovery.extend_from_slice(&program.code_hash);
    discovery.extend_from_slice(&program.abi_version.to_be_bytes()); discovery.extend_from_slice(&program.observed_sequence.to_be_bytes());
    discovery.extend_from_slice(&live.timestamp_ms.to_be_bytes()); discovery.extend_from_slice(&valid_through.to_be_bytes()); discovery.extend_from_slice(&program.state_root);
    let receipt_digest: [u8; 32] = Sha256::digest(&discovery).into();
    let signing = &emulator.signing_key;
    let proof_signature = signing.sign(&receipt_digest).to_bytes();
    let proof = format!("\"receipt_digest\":\"{}\",\"observed_at\":{},\"valid_through\":{},\"discovery_public_key\":\"{}\",\"discovery_signature\":\"{}\"", hex_encode(&receipt_digest), live.timestamp_ms, valid_through, hex_encode(&signing.verifying_key().to_bytes()), hex_encode(&proof_signature));
    if program.has_interface == 0 {
        if interface_only {
            return refusal(trace, 404, "interface_absent", "program has no published interface");
        }
        let common = format!("\"program_id\":\"{}\",\"version\":{},\"code_hash\":\"{}\",\"abi_version\":{},\"interface_status\":\"absent\",\"observed_sequence\":{},\"state_root\":\"{}\",{proof},\"freshness\":{{\"mode\":\"signed-emulator-head\"}}", hex_encode(&program.program_id), program.version, hex_encode(&program.code_hash), program.abi_version, program.observed_sequence, hex_encode(&program.state_root));
        return success(trace, &format!("{{{common},\"lifecycle\":\"active\"}}"));
    }
    if program.has_interface != 1 || program.interface_bytes.is_null() || program.interface_length == 0 || program.interface_length > 952 {
        return refusal(trace, 503, "core_invalid_output", "program interface state is invalid");
    }
    let interface = unsafe { slice::from_raw_parts(program.interface_bytes, program.interface_length) };
    let interface_digest: [u8; 32] = Sha256::digest(interface).into();
    let common = format!("\"program_id\":\"{}\",\"version\":{},\"code_hash\":\"{}\",\"abi_version\":{},\"interface\":\"{}\",\"interface_digest\":\"{}\",\"observed_sequence\":{},\"state_root\":\"{}\",{proof},\"freshness\":{{\"mode\":\"signed-emulator-head\"}}", hex_encode(&program.program_id), program.version, hex_encode(&program.code_hash), program.abi_version, hex_encode(interface), hex_encode(&interface_digest), program.observed_sequence, hex_encode(&program.state_root));
    if interface_only {
        success(trace, &format!("{{{common}}}"))
    } else {
        success(trace, &format!("{{{common},\"lifecycle\":\"active\"}}"))
    }
}

fn program_simulate(emulator: &mut Emulator, request: &Request, trace: u64) -> Response {
    let activity = match decode_activity(request) {
        Ok(activity) if !activity.is_empty() => activity,
        Ok(_) => return refusal(trace, 400, "invalid_argument", "program simulation activity must not be empty"),
        Err(error) => return refusal(trace, 400, "invalid_argument", &error),
    };
    let mut live = CoreState { state_root: [0; 32], next_sequence: 0,
        batch_number: 0, timestamp_ms: 0, cell_count: 0, account_count: 0 };
    let inspect_code = unsafe { platform_emulator_inspect(emulator.core, &raw mut live) };
    if inspect_code != 0 { return core_response(trace, inspect_code); }
    let mut receipt = CoreReceipt {
        activity_id: [0; 32], batch_id: [0; 32], state_root: [0; 32],
        previous_state_root: [0; 32], asset: [0; 32], sequencer_public_key: [0; 32],
        global_sequence: 0, result_code: 0, metered_cost_hi: 0,
        metered_cost_lo: 0, bytes: ptr::null(), length: 0,
        terminal_payload: ptr::null(), terminal_payload_length: 0,
        call_graph: ptr::null(), call_graph_length: 0,
        isolated_owner: ptr::null_mut(),
    };
    let code = unsafe { platform_emulator_simulate(emulator.core, activity.as_ptr(), activity.len(), &raw mut receipt) };
    if code != 0 { return core_response(trace, code); }
    if receipt.bytes.is_null() || receipt.length == 0 || receipt.length > MAX_RECEIPT_BYTES {
        unsafe { platform_emulator_receipt_release(&raw mut receipt); }
        return refusal(trace, 503, "core_invalid_output", "the LayerX core returned an invalid simulation receipt buffer");
    }
    let encoded = unsafe { slice::from_raw_parts(receipt.bytes, receipt.length) };
    if receipt.terminal_payload_length > MAX_RECEIPT_BYTES
        || (receipt.terminal_payload_length != 0 && receipt.terminal_payload.is_null())
    { unsafe { platform_emulator_receipt_release(&raw mut receipt); } return refusal(trace, 503, "core_invalid_output", "terminal payload availability is invalid"); }
    let terminal_payload = if receipt.terminal_payload_length == 0 { &[] } else { unsafe { slice::from_raw_parts(receipt.terminal_payload, receipt.terminal_payload_length) } };
    if receipt.call_graph_length > MAX_RECEIPT_BYTES || (receipt.call_graph_length != 0 && receipt.call_graph.is_null()) { unsafe { platform_emulator_receipt_release(&raw mut receipt); } return refusal(trace,503,"core_invalid_output","call graph availability is invalid"); }
    let call_graph = if receipt.call_graph_length == 0 { &[] } else { unsafe { slice::from_raw_parts(receipt.call_graph,receipt.call_graph_length) } };
    let receipt_digest: [u8; 32] = Sha256::digest(encoded).into();
    let metered_cost = (u128::from(receipt.metered_cost_hi) << 64)
        | u128::from(receipt.metered_cost_lo);
    let outcome = if receipt.result_code >= 0 {
        format!("{{\"status\":\"completed\",\"code\":{}}}", receipt.result_code)
    } else {
        format!("{{\"status\":\"refused\",\"failure\":{{\"result_code\":{}}}}}", receipt.result_code)
    };
    let signing = &emulator.signing_key;
    let mut boundary_material = b"LayerX/emulator/simulation-boundary/v1\0".to_vec();
    boundary_material.extend_from_slice(&signing.verifying_key().to_bytes());
    let boundary_id: [u8; 32] = Sha256::digest(&boundary_material).into();
    let observed_sequence = receipt.global_sequence.saturating_sub(1);
    let mut evidence = b"LayerX/agent/program-simulation-evidence/v1\0".to_vec();
    evidence.extend_from_slice(&boundary_id); evidence.extend_from_slice(&receipt.activity_id);
    evidence.extend_from_slice(&receipt.previous_state_root); evidence.extend_from_slice(&receipt.state_root);
    evidence.extend_from_slice(&observed_sequence.to_be_bytes()); evidence.extend_from_slice(&live.timestamp_ms.to_be_bytes()); evidence.push(0);
    let evidence_digest: [u8; 32] = Sha256::digest(&evidence).into();
    let evidence_signature = signing.sign(&evidence_digest).to_bytes();
    let response = success(trace, &format!("{{\"activity_kind\":\"program-call\",\"transition\":\"isolated-candidate\",\"committed\":false,\"activity_id\":\"{}\",\"batch_id\":\"{}\",\"global_sequence\":{},\"result_code\":{},\"metered_cost\":\"{metered_cost}\",\"previous_state_root\":\"{}\",\"state_root\":\"{}\",\"asset\":\"{}\",\"sequencer_public_key\":\"{}\",\"receipt\":\"{}\",\"receipt_digest\":\"{}\",\"terminal_payload\":\"{}\",\"call_graph\":\"{}\",\"simulation_evidence\":{{\"boundary_id\":\"{}\",\"activity_id\":\"{}\",\"previous_state_root\":\"{}\",\"hypothetical_state_root\":\"{}\",\"observed_sequence\":{},\"observed_at\":{},\"committed\":false,\"public_key\":\"{}\",\"signature\":\"{}\"}},\"outcome\":{outcome}}}", hex_encode(&receipt.activity_id), hex_encode(&receipt.batch_id), receipt.global_sequence, receipt.result_code, hex_encode(&receipt.previous_state_root), hex_encode(&receipt.state_root), hex_encode(&receipt.asset), hex_encode(&receipt.sequencer_public_key), hex_encode(encoded), hex_encode(&receipt_digest), hex_encode(terminal_payload),hex_encode(call_graph), hex_encode(&boundary_id), hex_encode(&receipt.activity_id), hex_encode(&receipt.previous_state_root), hex_encode(&receipt.state_root), observed_sequence, live.timestamp_ms, hex_encode(&signing.verifying_key().to_bytes()), hex_encode(&evidence_signature)));
    unsafe { platform_emulator_receipt_release(&raw mut receipt); }
    response
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
                let bytes = std::fs::read(&value).map_err(|error| format!("could not read sequencer seed file {value}: {error}"))?;
                let seed = if bytes.len() == 32 { bytes } else {
                    let text = std::str::from_utf8(&bytes).map_err(|_| "sequencer seed file must contain 32 raw bytes or 64 hexadecimal characters")?;
                    hex_decode(text.trim())?
                };
                config.sequencer_seed = Some(seed.try_into().map_err(|_| "sequencer seed must be exactly 32 bytes")?);
            }
            "--prefund" => config.prefunds.push(parse_prefund(&value)?),
            _ => return Err(format!("unknown argument: {argument}")),
        }
    }
    Ok(config)
}

/// Starts the local gateway adapter around the production `LayerX` transition.
fn platform_emulator(config: Config) -> Result<(), String> {
    let seed = config.sequencer_seed.ok_or_else(|| "--sequencer-seed-file is required; the emulator has no compiled-in signing authority".to_owned())?;
    if seed == [0; 32] { return Err("sequencer seed must not be zero".to_owned()); }
    let core = unsafe { platform_emulator_create(config.network_id, config.timestamp_ms, seed.as_ptr()) };
    if core.is_null() {
        return Err("could not initialize the LayerX core".into());
    }
    let mut emulator = Emulator {
        core,
        signing_key: SigningKey::from_bytes(&seed),
        receipts: HashMap::new(),
        receipt_order: VecDeque::new(),
        trace: 0,
    };
    for prefund in config.prefunds {
        let status = unsafe {
            platform_emulator_prefund(
                emulator.core,
                prefund.did.as_ptr(),
                prefund.did.len(),
                prefund.public_key.as_ptr(),
                prefund.amount_hi,
                prefund.amount_lo,
            )
        };
        if status != 0 {
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
    use super::{advance_trace, parse_config};

    #[test]
    fn control_surface_refuses_non_loopback_listeners() {
        assert!(parse_config(["up".to_owned(), "--listen".to_owned(), "0.0.0.0:9402".to_owned()]).is_err());
        assert!(parse_config(["up".to_owned(), "--listen".to_owned(), "192.0.2.1:9402".to_owned()]).is_err());
    }

    #[test]
    fn control_surface_accepts_ipv4_and_ipv6_loopback() {
        assert!(parse_config(["up".to_owned(), "--listen".to_owned(), "127.0.0.1:0".to_owned()]).is_ok());
        assert!(parse_config(["up".to_owned(), "--listen".to_owned(), "[::1]:0".to_owned()]).is_ok());
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
    use super::{decode_activity, hex_decode, Request};

    /// A representative canonical program-call activity. The value only has to
    /// be the exact bytes both ingress forms carry unchanged; the emulator hands
    /// these same bytes to the real transition on every path.
    const CANONICAL_ACTIVITY_HEX: &str = "4c61796572582f70726f6772616d732f63616c6c2f763100111111111111111111111111111111111111111111111111111111111111111100000000000003e8000000000000000000000000000000fa0002010300000002aabb";

    fn json_request(hex: &str) -> Request {
        Request {
            method: "POST".to_string(),
            path: "/v1/programs/call".to_string(),
            content_type: "application/json".to_string(),
            body: format!("{{\"activity\":\"{hex}\"}}").into_bytes(),
        }
    }

    fn octet_request(bytes: &[u8]) -> Request {
        Request {
            method: "POST".to_string(),
            path: "/v1/programs/call".to_string(),
            content_type: "application/octet-stream".to_string(),
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
            body: b"{\"activity\":\"00\",\"activity\":\"11\"}".to_vec(),
        };
        let unknown = Request {
            method: "POST".to_string(),
            path: "/v1/programs/call".to_string(),
            content_type: "application/json".to_string(),
            body: b"{\"activity\":\"00\",\"trusted\":true}".to_vec(),
        };
        assert!(decode_activity(&duplicate).is_err());
        assert!(decode_activity(&unknown).is_err());
    }
}
