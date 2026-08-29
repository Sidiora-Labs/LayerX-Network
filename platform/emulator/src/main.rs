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
use layerx_proof::program::{
    verify_program_execution, ProgramExecutionExpectation, VerifiedProgramExecution,
};
use layerx_types::intent::{
    Amount, CallBudget, Calldata, CapabilityRequest, ProgramCall, ProgramCallFailure,
    ProgramCallOutcome, ProgramId, ProgramLegacyValue, RequestedCapabilities,
};
use layerx_types::payload::{ActivityType, ModuleId, ModuleRegistration, ModuleRegistry};
use layerx_wire::activity::decode_signed;
use layerx_wire::hash::activity_id as derive_activity_id;

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
    program_operations: HashMap<String, ProgramOperation>,
    program_activity_operations: HashMap<String, String>,
    trace: u64,
}

struct ProgramOperation {
    activity_id: String,
    response: String,
}

struct DecodedProgramActivity {
    signed: Vec<u8>,
    call: ProgramCall,
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
    matches!(
        (method, path),
        ("POST", "/v1/programs/call") | ("POST", "/v1/programs/simulate")
    ) || (method == "GET"
        && (path.starts_with("/v1/programs/registry/")
            || path.starts_with("/v1/programs/receipts/by-idempotency/")
            || path.starts_with("/v1/programs/activities/")))
}

fn agent_error_class(status: u16, code: &str) -> &'static str {
    if code.starts_with("LXP_ERR_") {
        "CoreRejection"
    } else if code.contains("idempotency") {
        "IdempotencyConflict"
    } else if code.contains("quota") {
        "RateLimit"
    } else if code.contains("authorization")
        || code.contains("scope")
        || code.contains("not_active")
        || code.contains("refused")
    {
        "PolicyRefusal"
    } else if code.contains("verification")
        || code.contains("unverified")
        || code.contains("invalid_output")
        || code.contains("selector_mismatch")
        || code.contains("binding_invalid")
    {
        "VerificationFailure"
    } else if status == 404 || code.contains("absent") || code.contains("unknown_program") {
        "UnavailableCapability"
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
                    let Some(output_values) = output_values
                        .and_then(|number| u32::try_from(number).ok())
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
                        serde_json::json!({
                            "request_id": request_id.as_str(),
                            "value": value,
                            "verification_status": {
                                "state": "Achieved",
                                "level": "SequencerSigned",
                            },
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
            "retriability": if status >= 500 { "Retriable" } else { "Terminal" },
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
        return Err("program receipt selector does not match the route and verification level".to_owned());
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
        return Err("program activity selector does not match the route and verification level".to_owned());
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
        } else if name.eq_ignore_ascii_case("idempotency-key") {
            if idempotency_key.is_some()
                || if target == "/v1/programs/call" {
                    value.is_empty() || value.len() > 128
                } else {
                    value.len() != 64 || !value.bytes().all(|byte| byte.is_ascii_hexdigit())
                }
            {
                return Err("invalid idempotency key".into());
            }
            idempotency_key = Some(if target == "/v1/programs/call" {
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
    let state = if result_code == 0 { "completed" } else { "refused" };
    success(trace, &format!("{{\"state\":\"{state}\",\"activity_id\":\"{activity_id}\",\"batch_id\":\"{}\",\"global_sequence\":{global_sequence},\"result_code\":{result_code},\"state_root\":\"{}\",\"receipt\":\"{receipt_hex}\"}}", hex_encode(&batch_id), hex_encode(&state_root)))
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

fn programs_registry() -> Result<(ActivityType, ModuleRegistry), String> {
    let call_type = ActivityType::new(ModuleId::Programs, 3)
        .map_err(|_| "Programs CALL activity type is unavailable".to_owned())?;
    let registration = ModuleRegistration::new(ModuleId::Programs, &[call_type])
        .map_err(|_| "Programs module registration is unavailable".to_owned())?;
    let registry = ModuleRegistry::new(&[registration])
        .map_err(|_| "Programs module registry is unavailable".to_owned())?;
    Ok((call_type, registry))
}

fn decode_program_activity(request: &Request) -> Result<DecodedProgramActivity, String> {
    let media_type = request.content_type.split(';').next().unwrap_or_default().trim();
    let (signed, expected_call) = if media_type == "application/octet-stream" {
        if request.body.len() > MAX_RECEIPT_BYTES {
            return Err("signed program activity exceeds its bound".to_owned());
        }
        (request.body.clone(), None)
    } else if media_type == "application/json" {
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
        if body.signed_activity.len() % 2 != 0
            || body.signed_activity.len() / 2 > MAX_RECEIPT_BYTES
        {
            return Err("signed program activity exceeds its bound".to_owned());
        }
        (
            hex_decode(&body.signed_activity)?,
            Some(ProgramCall::new(program, calldata, budget, capabilities)),
        )
    } else {
        return Err("program call content type is not supported".to_owned());
    };
    if signed.is_empty() {
        return Err("program call activity must not be empty".to_owned());
    }
    let (call_type, registry) = programs_registry()?;
    let activity = decode_signed(&signed, &registry)
        .map_err(|_| "signed program activity is invalid".to_owned())?;
    if activity.activity_type() != call_type {
        return Err("signed activity is not a Programs CALL".to_owned());
    }
    let call = ProgramCall::from_canonical_payload(activity.payload())
        .map_err(|_| "signed program payload is not canonical".to_owned())?;
    if expected_call.as_ref().is_some_and(|expected| expected != &call) {
        return Err("signed program activity does not match the typed call".to_owned());
    }
    let activity_id = derive_activity_id(&activity)
        .map_err(|_| "signed program activity identity is invalid".to_owned())?;
    Ok(DecodedProgramActivity {
        signed,
        call,
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
        let receipt_bytes = unsafe { slice::from_raw_parts(receipt.bytes, receipt.length) }.to_vec();
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
        state_root: [0; 32],
        next_sequence: 0,
        batch_number: 0,
        timestamp_ms: 0,
        cell_count: 0,
        account_count: 0,
    };
    let code = unsafe { platform_emulator_inspect(emulator.core, &raw mut state) };
    if code == 0 { Ok(state) } else { Err(code) }
}

fn program_head(emulator: &mut Emulator, program_id: [u8; 32]) -> Result<CoreProgram, i32> {
    let mut program = CoreProgram {
        program_id: [0; 32],
        code_hash: [0; 32],
        version: 0,
        abi_version: 0,
        interface_bytes: ptr::null(),
        interface_length: 0,
        has_interface: 0,
        state_root: [0; 32],
        observed_sequence: 0,
    };
    let code = unsafe {
        platform_emulator_program_read(emulator.core, program_id.as_ptr(), &raw mut program)
    };
    if code == 0 { Ok(program) } else { Err(code) }
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

fn program_call(emulator: &mut Emulator, request: &Request, trace: u64) -> Response {
    let decoded = match decode_program_activity(request) {
        Ok(activity) => activity,
        Err(error) => return refusal(trace, 400, "invalid_argument", &error),
    };
    let protocol_idempotency = hex_encode(&decoded.idempotency_key);
    if request.idempotency_key.as_deref() != Some(protocol_idempotency.as_str()) {
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
            return success(trace, &existing.response);
        }
        return refusal(
            trace,
            409,
            "idempotency_conflict",
            "the idempotency key is already bound to a different activity",
        );
    }
    let program_id = decoded.call.callee().bytes();
    let head = match program_head(emulator, program_id) {
        Ok(head) => head,
        Err(code) => return core_response(trace, code),
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
        call_graph: ptr::null(), call_graph_length: 0,
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
            previous_state_root: before.state_root,
            activity_id: decoded.activity_id,
            program_id,
            guest_abi_version: head.abi_version,
        },
    ) {
        Ok(verified) => verified,
        Err(_) => {
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
        },
    );
    success(trace, &response)
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
    emulator.program_operations.clear();
    emulator.program_activity_operations.clear();
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
        return refusal(trace, 404, "program_receipt_not_found", "program receipt route is invalid");
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
            success(trace, &operation.response)
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
        return refusal(trace, 404, "program_activity_not_found", "program activity route is invalid");
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
            success(trace, &operation.response)
        }
        Some(_) => refusal(
            trace,
            503,
            "program_activity_binding_invalid",
            "the stored activity binding is inconsistent",
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
        ("POST", "/v1/activities") => submit(emulator, request, trace),
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
            "/v1/activities"
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
        _ => return refusal(trace, 400, "invalid_argument", "program id must be 32-byte hex"),
    };
    let mut program = CoreProgram {
        program_id: [0; 32], code_hash: [0; 32], deployment_receipt_digest: [0; 32],
        version: 0, abi_version: 0, lifecycle: 0,
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
    let lifecycle = match program.lifecycle {
        1 => "active",
        2 => "deprecated",
        3 => "tombstoned",
        _ => return refusal(trace, 503, "core_invalid_output", "program lifecycle is invalid"),
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
        return refusal(trace, 404, "interface_absent", "program has no published interface");
    }
    if program.has_interface != 1 || program.interface_bytes.is_null() || program.interface_length == 0 || program.interface_length > 952 {
        return refusal(trace, 503, "core_invalid_output", "program interface state is invalid");
    }
    let interface = unsafe { slice::from_raw_parts(program.interface_bytes, program.interface_length) };
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
        Err(error) => return refusal(trace, 400, "invalid_argument", &error),
    };
    let program_id = decoded.call.callee().bytes();
    let head = match program_head(emulator, program_id) {
        Ok(head) => head,
        Err(code) => return core_response(trace, code),
    };
    let live = match inspect_state(emulator) {
        Ok(state) => state,
        Err(code) => return core_response(trace, code),
    };
    let mut receipt = CoreReceipt {
        activity_id: [0; 32], batch_id: [0; 32], state_root: [0; 32],
        previous_state_root: [0; 32], asset: [0; 32], sequencer_public_key: [0; 32],
        global_sequence: 0, result_code: 0, metered_cost_hi: 0,
        metered_cost_lo: 0, bytes: ptr::null(), length: 0,
        terminal_payload: ptr::null(), terminal_payload_length: 0,
        call_graph: ptr::null(), call_graph_length: 0,
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
        Ok(verified) => verified,
        Err(_) => {
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
        return refusal(trace, 503, "core_invalid_output", "verified simulation has no protocol state root");
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
        program_operations: HashMap::new(),
        program_activity_operations: HashMap::new(),
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
    use super::{
        agent_response, decode_activity, decode_program_activity, hex_decode,
        program_activity_selector, program_receipt_selector, program_selector, programs_route,
        refusal, success, Request, MAX_RECEIPT_BYTES,
    };

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
        for state in ["refused", "unknown", "executed"] {
            let output = agent_response(
                7,
                success(
                    7,
                    &format!(
                        "{{\"state\":\"{state}\",\"global_sequence\":{},\"usage\":{{\"output_values\":\"7\"}}}}",
                        u64::MAX
                    ),
                ),
            );
            let document: serde_json::Value = serde_json::from_slice(&output.body).unwrap();
            assert_eq!(
                document,
                serde_json::json!({
                    "request_id":"emu-0000000000000007",
                    "value":{
                        "state":state,
                        "global_sequence":u64::MAX.to_string(),
                        "usage":{"output_values":7}
                    },
                    "verification_status":{"state":"Achieved","level":"SequencerSigned"},
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
