use std::collections::HashMap;
use std::ffi::{c_char, c_int, c_uchar, c_uint, c_ulonglong, c_void, CStr};
use std::fmt::Write as _;
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::ptr;
use std::slice;

const DEFAULT_LISTEN: &str = "127.0.0.1:9402";
const DEFAULT_NETWORK_ID: u32 = 402;
const DEFAULT_TIME_MS: u64 = 1_700_000_000_000;
const MAX_REQUEST_BYTES: usize = 64 * 1024 * 1024;

#[repr(C)]
struct CoreReceipt {
    activity_id: [u8; 32],
    batch_id: [u8; 32],
    state_root: [u8; 32],
    global_sequence: c_ulonglong,
    result_code: c_int,
    bytes: *const c_uchar,
    length: usize,
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

unsafe extern "C" {
    fn platform_emulator_create(network_id: c_uint, timestamp_ms: c_ulonglong) -> *mut c_void;
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
    fn platform_emulator_inspect(emulator: *const c_void, state: *mut CoreState) -> c_int;
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
    receipts: HashMap<String, String>,
    trace: u64,
}

impl Drop for Emulator {
    fn drop(&mut self) {
        unsafe { platform_emulator_destroy(self.core) };
    }
}

struct Config {
    listen: String,
    network_id: u32,
    timestamp_ms: u64,
    prefunds: Vec<Prefund>,
}

struct Prefund {
    did: String,
    public_key: [u8; 32],
    amount_hi: u64,
    amount_lo: u64,
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

fn json_string(body: &[u8], field: &str) -> Option<String> {
    let source = std::str::from_utf8(body).ok()?;
    let marker = format!("\"{field}\"");
    let after = source.get(source.find(&marker)? + marker.len()..)?;
    let after_colon = after.get(after.find(':')? + 1..)?.trim_start();
    let quoted = after_colon.strip_prefix('"')?;
    let end = quoted.find('"')?;
    Some(quoted[..end].to_string())
}

fn json_u64(body: &[u8], field: &str) -> Option<u64> {
    let source = std::str::from_utf8(body).ok()?;
    let marker = format!("\"{field}\"");
    let after = source.get(source.find(&marker)? + marker.len()..)?;
    let value = after.get(after.find(':')? + 1..)?.trim_start();
    let end = value
        .find(|character: char| !character.is_ascii_digit())
        .unwrap_or(value.len());
    value.get(..end)?.parse().ok()
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
            value if value.is_control() => escaped.push_str("\\u001f"),
            value => escaped.push(value),
        }
    }
    escaped
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
        if bytes.len() > MAX_REQUEST_BYTES {
            return Err("request exceeds emulator limit".into());
        }
        if let Some(position) = bytes.windows(4).position(|window| window == b"\r\n\r\n") {
            break position + 4;
        }
    };
    let headers = std::str::from_utf8(&bytes[..header_end]).map_err(|_| "headers are not UTF-8")?;
    let mut lines = headers.split("\r\n");
    let mut request_line = lines
        .next()
        .ok_or("missing request line")?
        .split_whitespace();
    let method = request_line.next().ok_or("missing method")?.to_string();
    let path = request_line
        .next()
        .ok_or("missing path")?
        .split('?')
        .next()
        .unwrap_or("/")
        .to_string();
    let mut content_length = 0_usize;
    let mut content_type = String::new();
    for line in lines {
        if let Some((name, value)) = line.split_once(':') {
            if name.eq_ignore_ascii_case("content-length") {
                content_length = value.trim().parse().map_err(|_| "invalid content length")?;
            } else if name.eq_ignore_ascii_case("content-type") {
                content_type = value.trim().to_ascii_lowercase();
            }
        }
    }
    if content_length > MAX_REQUEST_BYTES {
        return Err("request body exceeds emulator limit".into());
    }
    while bytes.len() - header_end < content_length {
        let read = stream.read(&mut chunk).map_err(|error| error.to_string())?;
        if read == 0 {
            return Err("connection closed before request body".into());
        }
        bytes.extend_from_slice(&chunk[..read]);
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
    let activity = if request.content_type.starts_with("application/octet-stream") {
        Ok(request.body.clone())
    } else {
        json_string(&request.body, "activity")
            .ok_or_else(|| "missing activity hex".to_string())
            .and_then(|value| hex_decode(&value))
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
        global_sequence: 0,
        result_code: 0,
        bytes: ptr::null(),
        length: 0,
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
    let encoded = unsafe { slice::from_raw_parts(receipt.bytes, receipt.length) };
    let receipt_hex = hex_encode(encoded);
    let activity_id = hex_encode(&receipt.activity_id);
    emulator
        .receipts
        .insert(activity_id.clone(), receipt_hex.clone());
    success(trace, &format!("{{\"activity_id\":\"{activity_id}\",\"batch_id\":\"{}\",\"global_sequence\":{},\"result_code\":{},\"state_root\":\"{}\",\"receipt\":\"{receipt_hex}\"}}", hex_encode(&receipt.batch_id), receipt.global_sequence, receipt.result_code, hex_encode(&receipt.state_root)))
}

fn decode_activity(request: &Request) -> Result<Vec<u8>, String> {
    if request.content_type.starts_with("application/octet-stream") {
        Ok(request.body.clone())
    } else {
        json_string(&request.body, "activity")
            .ok_or_else(|| "missing activity hex".to_string())
            .and_then(|value| hex_decode(&value))
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
        global_sequence: 0,
        result_code: 0,
        bytes: ptr::null(),
        length: 0,
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
    let encoded = unsafe { slice::from_raw_parts(receipt.bytes, receipt.length) };
    let receipt_hex = hex_encode(encoded);
    let activity_id = hex_encode(&receipt.activity_id);
    emulator
        .receipts
        .insert(activity_id.clone(), receipt_hex.clone());
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
    success(trace, &format!("{{\"activity_kind\":\"program-call\",\"transition\":\"real\",\"activity_id\":\"{activity_id}\",\"batch_id\":\"{}\",\"global_sequence\":{},\"result_code\":{},\"state_root\":\"{}\",\"receipt\":\"{receipt_hex}\",\"outcome\":{outcome}}}", hex_encode(&receipt.batch_id), receipt.global_sequence, receipt.result_code, hex_encode(&receipt.state_root)))
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

fn prefund(emulator: &mut Emulator, body: &[u8], trace: u64) -> Response {
    let Some(did) = json_string(body, "did") else {
        return refusal(trace, 400, "invalid_argument", "missing did");
    };
    let Some(key_hex) = json_string(body, "public_key") else {
        return refusal(trace, 400, "invalid_argument", "missing public_key");
    };
    let Ok(key) = hex_decode(&key_hex) else {
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
    let amount_hi = json_u64(body, "amount_hi").unwrap_or(0);
    let Some(amount_lo) = json_u64(body, "amount_lo") else {
        return refusal(trace, 400, "invalid_argument", "missing integer amount_lo");
    };
    let code = unsafe {
        platform_emulator_prefund(
            emulator.core,
            did.as_ptr(),
            did.len(),
            key.as_ptr(),
            amount_hi,
            amount_lo,
        )
    };
    if code != 0 {
        return core_response(trace, code);
    }
    success(trace, "{\"prefunded\":true}")
}

fn update_time(emulator: &mut Emulator, body: &[u8], trace: u64, advance: bool) -> Response {
    let field = if advance { "delta_ms" } else { "timestamp_ms" };
    let Some(value) = json_u64(body, field) else {
        return refusal(
            trace,
            400,
            "invalid_argument",
            &format!("missing integer {field}"),
        );
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

fn inject_fault(emulator: &mut Emulator, body: &[u8], trace: u64) -> Response {
    let kind = match json_string(body, "kind").as_deref() {
        Some("reject") => 1,
        Some("drop_receipt") => 2,
        Some("corrupt_receipt") => 3,
        _ => return refusal(trace, 400, "invalid_argument", "unknown fault kind"),
    };
    let count = json_u64(body, "count").unwrap_or(1);
    let code = unsafe { platform_emulator_inject_failure(emulator.core, kind, count) };
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
    success(trace, "{\"imported\":true}")
}

fn route(emulator: &mut Emulator, request: &Request) -> Response {
    emulator.trace += 1;
    let trace = emulator.trace;
    match (request.method.as_str(), request.path.as_str()) {
        ("GET", "/healthz") => success(trace, "{\"status\":\"ready\",\"core\":\"layerx\"}"),
        ("POST", "/v1/activities") => submit(emulator, request, trace),
        ("POST", "/v1/programs/call") => program_call(emulator, request, trace),
        ("GET", "/v1/state") => inspect(emulator, trace),
        ("POST", "/__emulator/accounts/prefund") => prefund(emulator, &request.body, trace),
        ("POST", "/__emulator/time/set") => update_time(emulator, &request.body, trace, false),
        ("POST", "/__emulator/time/advance") => update_time(emulator, &request.body, trace, true),
        ("POST", "/__emulator/faults") => inject_fault(emulator, &request.body, trace),
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
            | "/v1/programs/call"
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
        return Err("usage: layerx emulator up [--listen ADDRESS] [--network-id ID] [--time-ms MS] [--prefund DID,PUBLIC_KEY,AMOUNT]".into());
    }
    let mut config = Config {
        listen: DEFAULT_LISTEN.into(),
        network_id: DEFAULT_NETWORK_ID,
        timestamp_ms: DEFAULT_TIME_MS,
        prefunds: Vec::new(),
    };
    while let Some(argument) = arguments.next() {
        let value = arguments
            .next()
            .ok_or_else(|| format!("missing value for {argument}"))?;
        match argument.as_str() {
            "--listen" => config.listen = value,
            "--network-id" => {
                config.network_id = value.parse().map_err(|_| "network id must be an integer")?;
            }
            "--time-ms" => {
                config.timestamp_ms = value.parse().map_err(|_| "time must be an integer")?;
            }
            "--prefund" => config.prefunds.push(parse_prefund(&value)?),
            _ => return Err(format!("unknown argument: {argument}")),
        }
    }
    Ok(config)
}

/// Starts the local gateway adapter around the production `LayerX` transition.
fn platform_emulator(config: Config) -> Result<(), String> {
    let core = unsafe { platform_emulator_create(config.network_id, config.timestamp_ms) };
    if core.is_null() {
        return Err("could not initialize the LayerX core".into());
    }
    let mut emulator = Emulator {
        core,
        receipts: HashMap::new(),
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
    let listener = TcpListener::bind(&config.listen)
        .map_err(|error| format!("cannot listen on {}: {error}", config.listen))?;
    eprintln!(
        "LayerX emulator ready on http://{} (network {}, deterministic time {})",
        config.listen, config.network_id, config.timestamp_ms
    );
    for connection in listener.incoming() {
        match connection {
            Ok(mut stream) => {
                let response = match parse_request(&mut stream) {
                    Ok(request) => route(&mut emulator, &request),
                    Err(error) => {
                        emulator.trace += 1;
                        refusal(emulator.trace, 400, "invalid_request", &error)
                    }
                };
                let _ = write_response(&mut stream, &response);
            }
            Err(error) => eprintln!("emulator connection error: {error}"),
        }
    }
    Ok(())
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
}
