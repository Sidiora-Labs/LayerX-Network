use std::collections::BTreeMap;
use std::fs;
use std::io::{Read as _, Write as _};
use std::net::{SocketAddr, TcpListener, TcpStream};
use std::path::Path;
use std::process::{Command, Stdio};
use std::sync::{
    mpsc::{sync_channel, TrySendError},
    Arc, Mutex,
};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use layerx_mcp::server::{DeploymentMode, ToolDefinition};
use serde::{Deserialize, Serialize};
use serde_json::{json, Map, Value};
use sha2::{Digest as _, Sha256};
use subtle::ConstantTimeEq as _;
use zeroize::{Zeroize as _, Zeroizing};

use crate::config::Configuration;
use crate::encoding::hex_encode;
use crate::install::a2a::agent_card;
use crate::toolset;

const MAX_HEADER_BYTES: usize = 32 * 1024;
const MAX_REQUEST_BYTES: usize = 256 * 1024;
const MAX_TRACKED_TASKS: usize = 128;
const MAX_CONTEXT_ID_BYTES: usize = 256;
const WORKER_COUNT: usize = 8;
const ADMISSION_CAPACITY: usize = 32;
const CARD_ROUTES: [&str; 2] = ["/.well-known/agent-card.json", "/.well-known/agent.json"];
const STATE_FILE: &str = "service.json";

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct ServiceState {
    pid: u32,
    process_start_ticks: u64,
    launch_digest: String,
}

struct Request {
    method: String,
    path: String,
    authorization: Option<Zeroizing<String>>,
    body: Vec<u8>,
}

impl Drop for Request {
    fn drop(&mut self) {
        self.body.zeroize();
    }
}

struct Response {
    status: u16,
    body: Vec<u8>,
}

struct Instruction {
    skill: String,
    arguments: Value,
}

pub fn endpoint(listen: &str) -> Result<SocketAddr, String> {
    let address = listen
        .parse::<SocketAddr>()
        .map_err(|_| format!("{listen} is not a host:port endpoint"))?;
    if !address.ip().is_loopback() {
        return Err("the agent-to-agent endpoint must bind a loopback address".into());
    }
    if address.port() == 0 {
        return Err("the agent-to-agent endpoint must declare a fixed port".into());
    }
    Ok(address)
}

pub fn serve(
    configuration: &Configuration,
    gateway_credential: &str,
    key: &str,
    source: Option<&str>,
    asset: Option<&str>,
    listen: &str,
    authorization_file: &Path,
    mode: DeploymentMode,
) -> Result<(), String> {
    let address = endpoint(listen)?;
    let tools = toolset::surface(mode)?;
    let runtime = Arc::new(toolset::Runtime::new(
        configuration,
        gateway_credential,
        key,
        source,
        asset,
        mode,
    )?);
    let card = Arc::new(agent_card(
        &configuration.current_environment,
        &address.to_string(),
        mode,
        &tools,
    ));
    let tools = Arc::new(tools);
    let authorization = Arc::new(crate::install::a2a::read_authorization(
        authorization_file,
    )?);
    let listener =
        TcpListener::bind(address).map_err(|error| format!("could not bind {listen}: {error}"))?;
    let tasks = Arc::new(Mutex::new(BTreeMap::<String, Value>::new()));
    let (sender, receiver) = sync_channel::<TcpStream>(ADMISSION_CAPACITY);
    let receiver = Arc::new(Mutex::new(receiver));
    for index in 0..WORKER_COUNT {
        let runtime = Arc::clone(&runtime);
        let tools = Arc::clone(&tools);
        let card = Arc::clone(&card);
        let tasks = Arc::clone(&tasks);
        let authorization = Arc::clone(&authorization);
        let receiver = Arc::clone(&receiver);
        std::thread::Builder::new()
            .name(format!("layerx-a2a-{index}"))
            .spawn(move || loop {
                let stream = match receiver.lock() {
                    Ok(receiver) => receiver.recv(),
                    Err(_) => return,
                };
                let Ok(mut stream) = stream else {
                    return;
                };
                if stream
                    .set_read_timeout(Some(Duration::from_secs(10)))
                    .and_then(|()| stream.set_write_timeout(Some(Duration::from_secs(10))))
                    .is_err()
                {
                    continue;
                }
                let response = match read_request(&mut stream) {
                    Ok(request) => route(
                        &runtime,
                        &tools,
                        &card,
                        &tasks,
                        &authorization,
                        &request,
                    ),
                    Err(error) => encode(400, &json!({"error": error})),
                };
                let _ = write_response(&mut stream, &response);
            })
            .map_err(|error| format!("could not start A2A worker {index}: {error}"))?;
    }
    for accepted in listener.incoming() {
        let Ok(stream) = accepted else {
            continue;
        };
        match sender.try_send(stream) {
            Ok(()) => {}
            Err(TrySendError::Full(mut stream)) => {
                let _ = stream.set_write_timeout(Some(Duration::from_secs(2)));
                let _ = write_response(
                    &mut stream,
                    &encode(503, &json!({"error": "agent-to-agent service is at capacity"})),
                );
            }
            Err(TrySendError::Disconnected(_)) => {
                return Err("all agent-to-agent workers stopped".into())
            }
        }
    }
    Ok(())
}

fn route(
    runtime: &toolset::Runtime,
    tools: &[ToolDefinition],
    card: &Value,
    tasks: &Mutex<BTreeMap<String, Value>>,
    authorization: &str,
    request: &Request,
) -> Response {
    match request.method.as_str() {
        "GET" if CARD_ROUTES.contains(&request.path.as_str()) => encode(200, card),
        "POST" if !authorized(request, authorization) => {
            encode(401, &json!({"error": "valid bearer authorization is required"}))
        }
        "POST" if request.path == "/" => {
            encode(200, &dispatch(runtime, tools, tasks, &request.body))
        }
        "GET" | "POST" => encode(404, &json!({"error": "unknown agent-to-agent route"})),
        _ => encode(405, &json!({"error": "unsupported method"})),
    }
}

fn authorized(request: &Request, expected: &str) -> bool {
    let Some(presented) = request
        .authorization
        .as_deref()
        .and_then(|value| value.strip_prefix("Bearer "))
    else {
        return false;
    };
    presented.len() == expected.len()
        && presented.as_bytes().ct_eq(expected.as_bytes()).unwrap_u8() == 1
}

fn dispatch(
    runtime: &toolset::Runtime,
    tools: &[ToolDefinition],
    tasks: &Mutex<BTreeMap<String, Value>>,
    body: &[u8],
) -> Value {
    let Ok(message) = serde_json::from_slice::<Value>(body) else {
        return failure(Value::Null, -32700, "the message is not valid JSON");
    };
    let identifier = message.get("id").cloned().unwrap_or(Value::Null);
    let Some(method) = message.get("method").and_then(Value::as_str) else {
        return failure(identifier, -32600, "the message did not name a method");
    };
    let parameters = message.get("params").cloned().unwrap_or(Value::Null);
    match method {
        "message/send" => send(runtime, tools, tasks, identifier, &parameters),
        "tasks/get" => fetch(tasks, identifier, &parameters),
        "tasks/cancel" => cancel(tasks, identifier, &parameters),
        "message/stream" | "tasks/resubscribe" | "tasks/pushNotificationConfig/set" => failure(
            identifier,
            -32004,
            "the operation is not supported by this deployment",
        ),
        _ => failure(identifier, -32601, "the method is not implemented"),
    }
}

fn send(
    runtime: &toolset::Runtime,
    tools: &[ToolDefinition],
    tasks: &Mutex<BTreeMap<String, Value>>,
    identifier: Value,
    parameters: &Value,
) -> Value {
    let Some(message) = parameters.get("message") else {
        return failure(identifier, -32602, "the request carried no message");
    };
    let instruction = match read_instruction(message) {
        Ok(instruction) => instruction,
        Err(error) => return failure(identifier, -32602, &error),
    };
    let Some(tool) = tools
        .iter()
        .find(|tool| tool.name == instruction.skill)
        .copied()
    else {
        return failure(
            identifier,
            -32601,
            &format!(
                "skill {} is not served by this deployment",
                instruction.skill
            ),
        );
    };
    let ids = match (new_identifier(), new_identifier()) {
        (Ok(task), Ok(artifact)) => (task, artifact),
        _ => {
            return failure(
                identifier,
                -32603,
                "operating-system randomness failed while opening a task",
            )
        }
    };
    let context = message
        .get("contextId")
        .and_then(Value::as_str)
        .map_or_else(|| ids.0.clone(), str::to_owned);
    if context.is_empty() || context.len() > MAX_CONTEXT_ID_BYTES {
        return failure(
            identifier,
            -32602,
            "the context identifier is outside the transport limit",
        );
    }
    let timestamp = match timestamp() {
        Ok(value) => value,
        Err(error) => return failure(identifier, -32603, &error),
    };
    let outcome = toolset::invoke(runtime, tool, &instruction.arguments);
    let task = build_task(&ids, &context, message, tool.name, &timestamp, outcome);
    let mut tasks = match tasks.lock() {
        Ok(tasks) => tasks,
        Err(_) => return failure(identifier, -32603, "the task ledger is unavailable"),
    };
    if tasks.len() >= MAX_TRACKED_TASKS {
        if let Some(evicted) = tasks.keys().next().cloned() {
            tasks.remove(&evicted);
        }
    }
    tasks.insert(ids.0.clone(), task.clone());
    success(identifier, task)
}

fn build_task(
    ids: &(String, String),
    context: &str,
    message: &Value,
    tool: &str,
    timestamp: &str,
    outcome: Result<Value, String>,
) -> Value {
    match outcome {
        Ok(value) => {
            let state = match gateway_state(&value) {
                Some("refused") => "rejected",
                Some("unknown") => "unknown",
                Some("acknowledged") | Some("pending") => "submitted",
                _ => "completed",
            };
            json!({
                "id": ids.0,
                "contextId": context,
                "kind": "task",
                "status": {"state": state, "timestamp": timestamp},
                "artifacts": [{
                    "artifactId": ids.1,
                    "name": tool,
                    "parts": [{"kind": "data", "data": {"result": value}}],
                }],
                "history": [message],
            })
        }
        Err(error) => json!({
            "id": ids.0,
            "contextId": context,
            "kind": "task",
            "status": {
                "state": "rejected",
                "timestamp": timestamp,
                "message": {
                    "kind": "message",
                    "role": "agent",
                    "messageId": ids.1,
                    "taskId": ids.0,
                    "contextId": context,
                    "parts": [{"kind": "text", "text": error}],
                },
            },
            "history": [message],
        }),
    }
}

fn gateway_state(value: &Value) -> Option<&str> {
    value
        .pointer("/gateway/state")
        .or_else(|| value.pointer("/gateway/result/state"))
        .and_then(Value::as_str)
}

fn cancel(
    tasks: &Mutex<BTreeMap<String, Value>>,
    identifier: Value,
    parameters: &Value,
) -> Value {
    let Some(name) = parameters.get("id").and_then(Value::as_str) else {
        return failure(identifier, -32602, "the request did not name a task");
    };
    let tasks = match tasks.lock() {
        Ok(tasks) => tasks,
        Err(_) => return failure(identifier, -32603, "the task ledger is unavailable"),
    };
    let Some(task) = tasks.get(name) else {
        return failure(identifier, -32001, "the task was not found");
    };
    match task.pointer("/status/state").and_then(Value::as_str) {
        Some("submitted" | "unknown") => failure(
            identifier,
            -32004,
            "the submitted protocol activity cannot be canceled by the transport",
        ),
        _ => failure(
            identifier,
            -32002,
            "the task already reached a terminal state",
        ),
    }
}

fn fetch(
    tasks: &Mutex<BTreeMap<String, Value>>,
    identifier: Value,
    parameters: &Value,
) -> Value {
    let Some(name) = parameters.get("id").and_then(Value::as_str) else {
        return failure(identifier, -32602, "the request did not name a task");
    };
    let tasks = match tasks.lock() {
        Ok(tasks) => tasks,
        Err(_) => return failure(identifier, -32603, "the task ledger is unavailable"),
    };
    match tasks.get(name) {
        Some(task) => success(identifier, task.clone()),
        None => failure(identifier, -32001, "the task was not found"),
    }
}

fn read_instruction(message: &Value) -> Result<Instruction, String> {
    let parts = message
        .get("parts")
        .and_then(Value::as_array)
        .ok_or_else(|| "the message carried no parts".to_string())?;
    for part in parts {
        let payload = match part.get("kind").and_then(Value::as_str) {
            Some("data") => part.get("data").cloned(),
            Some("text") => part
                .get("text")
                .and_then(Value::as_str)
                .and_then(|text| serde_json::from_str::<Value>(text).ok()),
            _ => None,
        };
        let Some(payload) = payload else {
            continue;
        };
        let Some(skill) = payload.get("skill").and_then(Value::as_str) else {
            continue;
        };
        return Ok(Instruction {
            skill: skill.to_owned(),
            arguments: payload.get("arguments").cloned().unwrap_or(Value::Null),
        });
    }
    Err("the message did not name a skill and its arguments".into())
}

fn read_request(stream: &mut TcpStream) -> Result<Request, String> {
    let mut bytes = Vec::with_capacity(4096);
    let mut chunk = [0_u8; 4096];
    let header_end = loop {
        let read = stream
            .read(&mut chunk)
            .map_err(|error| format!("could not read the request: {error}"))?;
        if read == 0 {
            return Err("the connection closed before the request headers".into());
        }
        bytes.extend_from_slice(&chunk[..read]);
        if let Some(position) = bytes.windows(4).position(|window| window == b"\r\n\r\n") {
            let end = position + 4;
            if end > MAX_HEADER_BYTES {
                return Err("the request headers exceed the transport limit".into());
            }
            break end;
        }
        if bytes.len() > MAX_HEADER_BYTES {
            return Err("the request headers exceed the transport limit".into());
        }
    };
    let headers = std::str::from_utf8(&bytes[..header_end])
        .map_err(|_| "the request headers are not UTF-8".to_string())?;
    let mut lines = headers.split("\r\n");
    let start = lines
        .next()
        .ok_or_else(|| "the request has no start line".to_string())?;
    let mut start_fields = start.split(' ');
    let method = start_fields.next().unwrap_or_default();
    let path = start_fields.next().unwrap_or_default();
    if method.is_empty()
        || !method.bytes().all(|byte| byte.is_ascii_uppercase())
        || path.is_empty()
        || !path.starts_with('/')
        || path.starts_with("//")
        || path.contains(['?', '#', '\\', '\0'])
        || path.split('/').any(|segment| matches!(segment, "." | ".."))
        || start_fields.next() != Some("HTTP/1.1")
        || start_fields.next().is_some()
    {
        return Err("the request line is not canonical HTTP/1.1".into());
    }
    let mut length = 0_usize;
    let mut has_length = false;
    let mut has_host = false;
    let mut content_type = None;
    let mut authorization = None;
    for line in lines.filter(|line| !line.is_empty()) {
        let (name, value) = line
            .split_once(':')
            .ok_or_else(|| "the request contains a malformed header".to_string())?;
        if name.is_empty()
            || !name
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-')
        {
            return Err("the request contains a malformed header name".into());
        }
        let value = value.trim_matches([' ', '\t']);
        if value.bytes().any(|byte| byte.is_ascii_control() && byte != b'\t') {
            return Err("the request contains a malformed header value".into());
        }
        if name.eq_ignore_ascii_case("content-length") {
            if has_length || value.is_empty() || !value.bytes().all(|byte| byte.is_ascii_digit()) {
                return Err("the request contains an ambiguous content length".into());
            }
            length = value
                .parse()
                .map_err(|_| "the content length is not a number".to_string())?;
            has_length = true;
        } else if name.eq_ignore_ascii_case("host") {
            if has_host || value.is_empty() {
                return Err("the request contains an ambiguous host".into());
            }
            has_host = true;
        } else if name.eq_ignore_ascii_case("content-type") {
            if content_type.is_some() {
                return Err("the request contains an ambiguous content type".into());
            }
            content_type = Some(value);
        } else if name.eq_ignore_ascii_case("authorization") {
            if authorization.is_some() || value.len() > 256 {
                return Err("the request contains ambiguous authorization".into());
            }
            authorization = Some(Zeroizing::new(value.to_owned()));
        } else if name.eq_ignore_ascii_case("transfer-encoding") {
            return Err("transfer encoding is not supported".into());
        }
    }
    if !has_host {
        return Err("the request has no host header".into());
    }
    if method == "POST"
        && (!has_length || content_type != Some("application/json"))
    {
        return Err("POST requires one application/json body with an explicit length".into());
    }
    if method != "POST" && length != 0 {
        return Err("a non-POST request may not carry a body".into());
    }
    if length > MAX_REQUEST_BYTES {
        return Err("the request body exceeds the transport limit".into());
    }
    let method = method.to_owned();
    let path = path.to_owned();
    while bytes.len() - header_end < length {
        let read = stream
            .read(&mut chunk)
            .map_err(|error| format!("could not read the request body: {error}"))?;
        if read == 0 {
            return Err("the connection closed before the request body".into());
        }
        bytes.extend_from_slice(&chunk[..read]);
        if bytes.len() > MAX_REQUEST_BYTES.saturating_add(header_end) {
            return Err("the request exceeds the transport limit".into());
        }
    }
    if bytes.len() != header_end + length {
        return Err("the request carries bytes beyond its declared body".into());
    }
    let body = bytes[header_end..header_end + length].to_vec();
    Ok(Request {
        method,
        path,
        authorization,
        body,
    })
}

fn write_response(stream: &mut TcpStream, response: &Response) -> std::io::Result<()> {
    let reason = match response.status {
        200 => "OK",
        400 => "Bad Request",
        401 => "Unauthorized",
        404 => "Not Found",
        405 => "Method Not Allowed",
        503 => "Service Unavailable",
        _ => "Error",
    };
    write!(
        stream,
        "HTTP/1.1 {} {reason}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nCache-Control: no-store\r\nConnection: close\r\n\r\n",
        response.status,
        response.body.len()
    )?;
    stream.write_all(&response.body)
}

fn encode(status: u16, value: &Value) -> Response {
    let body = serde_json::to_vec(value)
        .unwrap_or_else(|_| b"{\"error\":\"result encoding failed\"}".to_vec());
    Response { status, body }
}

fn success(identifier: Value, result: Value) -> Value {
    let mut envelope = Map::new();
    envelope.insert("jsonrpc".to_owned(), Value::String("2.0".to_owned()));
    envelope.insert("id".to_owned(), identifier);
    envelope.insert("result".to_owned(), result);
    Value::Object(envelope)
}

fn failure(identifier: Value, code: i32, detail: &str) -> Value {
    let mut error = Map::new();
    error.insert("code".to_owned(), Value::from(code));
    error.insert("message".to_owned(), Value::String(detail.to_owned()));
    let mut envelope = Map::new();
    envelope.insert("jsonrpc".to_owned(), Value::String("2.0".to_owned()));
    envelope.insert("id".to_owned(), identifier);
    envelope.insert("error".to_owned(), Value::Object(error));
    Value::Object(envelope)
}

fn new_identifier() -> Result<String, String> {
    let mut bytes = [0_u8; 16];
    getrandom::fill(&mut bytes)
        .map_err(|error| format!("operating-system randomness failed: {error}"))?;
    Ok(hex_encode(&bytes))
}

fn timestamp() -> Result<String, String> {
    let seconds = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|_| "the system clock is before the Unix epoch".to_owned())?
        .as_secs();
    let days = seconds / 86_400;
    let remainder = seconds % 86_400;
    let (year, month, day) = civil_date(days);
    let hour = remainder / 3_600;
    let minute = (remainder % 3_600) / 60;
    let second = remainder % 60;
    Ok(format!(
        "{year:04}-{month:02}-{day:02}T{hour:02}:{minute:02}:{second:02}Z"
    ))
}

pub fn start_installed(
    command: &str,
    arguments: &[String],
    variables: &BTreeMap<String, String>,
) -> Result<Value, String> {
    if !cfg!(target_os = "linux") {
        return Err("managed A2A lifecycle is currently supported on Linux runtimes".into());
    }
    if variables
        .keys()
        .any(|name| !matches!(name.as_str(), "LAYERX_CONFIG" | "LAYERX_GATEWAY_KEY_ID"))
    {
        return Err("installed A2A runtime environment contains an unsupported variable".into());
    }
    let state_path = state_path()?;
    let digest = launch_digest(command, arguments, variables);
    if let Some(state) = read_state(&state_path)? {
        if process_live(&state) && state.launch_digest == digest {
            return Ok(json!({
                "state": "running",
                "pid": state.pid,
                "changed": false,
                "state_file": state_path.display().to_string(),
            }));
        }
        if process_live(&state) {
            return Err(
                "an A2A process is already running with different installed arguments; stop it before reinstalling"
                    .into(),
            );
        }
        fs::remove_file(&state_path)
            .map_err(|error| format!("could not clear stale {}: {error}", state_path.display()))?;
    }
    let mut process = Command::new(command);
    process
        .args(arguments)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .env_remove("LAYERX_TOKEN")
        .env_remove("LAYERX_API_TOKEN")
        .env_remove("LAYERX_AUTH_TOKEN");
    for (name, value) in variables {
        process.env(name, value);
    }
    let mut child = process
        .spawn()
        .map_err(|error| format!("could not start installed A2A runtime: {error}"))?;
    let process_start_ticks = match process_start_ticks(child.id()) {
        Ok(value) => value,
        Err(error) => {
            let _ = child.kill();
            return Err(error);
        }
    };
    let state = ServiceState {
        pid: child.id(),
        process_start_ticks,
        launch_digest: digest,
    };
    if let Err(error) = write_state(&state_path, &state) {
        let _ = child.kill();
        return Err(error);
    }
    Ok(json!({
        "state": "running",
        "pid": state.pid,
        "changed": true,
        "state_file": state_path.display().to_string(),
    }))
}

pub fn start_from_manifest() -> Result<Value, String> {
    let manifest_path = crate::install::layerx_directory()?.join("a2a/runtime.json");
    let source = fs::read_to_string(&manifest_path)
        .map_err(|error| format!("could not read {}: {error}", manifest_path.display()))?;
    let document: Value = serde_json::from_str(&source)
        .map_err(|error| format!("could not parse {}: {error}", manifest_path.display()))?;
    let entry = document
        .pointer("/agents/layerx")
        .ok_or_else(|| "installed A2A runtime manifest has no LayerX entry".to_owned())?;
    let command = entry
        .get("command")
        .and_then(Value::as_str)
        .ok_or_else(|| "installed A2A runtime command is missing".to_owned())?;
    let arguments = entry
        .get("args")
        .and_then(Value::as_array)
        .ok_or_else(|| "installed A2A runtime arguments are missing".to_owned())?
        .iter()
        .map(|value| {
            value
                .as_str()
                .map(str::to_owned)
                .ok_or_else(|| "installed A2A runtime argument is not text".to_owned())
        })
        .collect::<Result<Vec<_>, _>>()?;
    let variables = entry
        .get("env")
        .and_then(Value::as_object)
        .ok_or_else(|| "installed A2A runtime environment is missing".to_owned())?
        .iter()
        .map(|(name, value)| {
            value
                .as_str()
                .map(|value| (name.clone(), value.to_owned()))
                .ok_or_else(|| "installed A2A runtime environment value is not text".to_owned())
        })
        .collect::<Result<BTreeMap<_, _>, _>>()?;
    start_installed(command, &arguments, &variables)
}

pub fn stop_installed() -> Result<Value, String> {
    let path = state_path()?;
    let Some(state) = read_state(&path)? else {
        return Ok(json!({"state": "stopped", "changed": false}));
    };
    if process_live(&state) {
        let status = Command::new("kill")
            .arg(state.pid.to_string())
            .status()
            .map_err(|error| format!("could not stop A2A process {}: {error}", state.pid))?;
        if !status.success() {
            return Err(format!(
                "A2A process {} refused the stop request",
                state.pid
            ));
        }
    }
    fs::remove_file(&path)
        .map_err(|error| format!("could not remove {}: {error}", path.display()))?;
    Ok(json!({"state": "stopped", "pid": state.pid, "changed": true}))
}

pub fn installed_status() -> Result<Value, String> {
    let path = state_path()?;
    let Some(state) = read_state(&path)? else {
        return Ok(json!({"state": "stopped"}));
    };
    Ok(json!({
        "state": if process_live(&state) { "running" } else { "stale" },
        "pid": state.pid,
        "state_file": path.display().to_string(),
    }))
}

fn state_path() -> Result<std::path::PathBuf, String> {
    Ok(crate::install::layerx_directory()?
        .join("a2a")
        .join(STATE_FILE))
}

fn process_live(state: &ServiceState) -> bool {
    cfg!(target_os = "linux")
        && process_start_ticks(state.pid).ok() == Some(state.process_start_ticks)
}

fn process_start_ticks(pid: u32) -> Result<u64, String> {
    let path = std::path::Path::new("/proc")
        .join(pid.to_string())
        .join("stat");
    let source = fs::read_to_string(&path)
        .map_err(|error| format!("could not inspect started A2A process {pid}: {error}"))?;
    let (_, fields) = source
        .rsplit_once(") ")
        .ok_or_else(|| format!("{} has an invalid process record", path.display()))?;
    fields
        .split_whitespace()
        .nth(19)
        .ok_or_else(|| format!("{} has no process start identity", path.display()))?
        .parse()
        .map_err(|_| format!("{} has an invalid process start identity", path.display()))
}

fn read_state(path: &std::path::Path) -> Result<Option<ServiceState>, String> {
    match fs::read_to_string(path) {
        Ok(source) => serde_json::from_str(&source)
            .map(Some)
            .map_err(|error| format!("could not parse {}: {error}", path.display())),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(format!("could not read {}: {error}", path.display())),
    }
}

fn write_state(path: &std::path::Path, state: &ServiceState) -> Result<(), String> {
    let encoded = serde_json::to_string(state)
        .map_err(|error| format!("could not encode A2A service state: {error}"))?;
    crate::install::publish(
        path,
        &serde_json::from_str(&encoded)
            .map_err(|error| format!("could not encode A2A service state: {error}"))?,
    )
    .map(|_| ())
}

fn launch_digest(
    command: &str,
    arguments: &[String],
    variables: &BTreeMap<String, String>,
) -> String {
    let mut digest = Sha256::new();
    digest.update((command.len() as u64).to_be_bytes());
    digest.update(command.as_bytes());
    for argument in arguments {
        digest.update((argument.len() as u64).to_be_bytes());
        digest.update(argument.as_bytes());
    }
    for (name, value) in variables {
        digest.update((name.len() as u64).to_be_bytes());
        digest.update(name.as_bytes());
        digest.update((value.len() as u64).to_be_bytes());
        digest.update(value.as_bytes());
    }
    hex_encode(&digest.finalize())
}

fn civil_date(days: u64) -> (u64, u64, u64) {
    let shifted = days + 719_468;
    let era = shifted / 146_097;
    let day_of_era = shifted % 146_097;
    let year_of_era =
        (day_of_era - day_of_era / 1_460 + day_of_era / 36_524 - day_of_era / 146_096) / 365;
    let civil_year = year_of_era + era * 400;
    let day_of_year = day_of_era - (365 * year_of_era + year_of_era / 4 - year_of_era / 100);
    let shifted_month = (5 * day_of_year + 2) / 153;
    let day_of_month = day_of_year - (153 * shifted_month + 2) / 5 + 1;
    let month = if shifted_month < 10 {
        shifted_month + 3
    } else {
        shifted_month - 9
    };
    let year = if month <= 2 {
        civil_year + 1
    } else {
        civil_year
    };
    (year, month, day_of_month)
}

#[cfg(test)]
mod request_boundary_tests {
    use std::io::Write as _;
    use std::net::{TcpListener, TcpStream};
    use std::thread;

    use zeroize::Zeroizing;

    use super::{authorized, read_request, Request};

    fn parse(raw: Vec<u8>) -> Result<(), String> {
        let listener = TcpListener::bind("127.0.0.1:0").map_err(|error| error.to_string())?;
        let address = listener.local_addr().map_err(|error| error.to_string())?;
        let writer = thread::spawn(move || -> Result<(), String> {
            let mut stream = TcpStream::connect(address).map_err(|error| error.to_string())?;
            stream
                .write_all(&raw)
                .map_err(|error| error.to_string())
        });
        let (mut stream, _) = listener.accept().map_err(|error| error.to_string())?;
        let parsed = read_request(&mut stream).map(|_| ());
        writer
            .join()
            .map_err(|_| "request writer panicked".to_owned())??;
        parsed
    }

    #[test]
    fn rejects_ambiguous_http_framing() {
        assert!(parse(b"POST / HTTP/1.0\r\nHost: localhost\r\nContent-Length: 0\r\nContent-Type: application/json\r\n\r\n".to_vec()).is_err());
        assert!(parse(b"POST / HTTP/1.1\r\nHost: localhost\r\nTransfer-Encoding: chunked\r\nContent-Length: 0\r\nContent-Type: application/json\r\n\r\n".to_vec()).is_err());
        assert!(parse(b"POST / HTTP/1.1\r\nHost: localhost\r\nContent-Length: 0\r\nContent-Length: 0\r\nContent-Type: application/json\r\n\r\n".to_vec()).is_err());
        assert!(parse(b"POST / HTTP/1.1\r\nHost: localhost\r\nContent-Length: 0\r\nContent-Type: application/json\r\n\r\nsurplus".to_vec()).is_err());
    }

    #[test]
    fn rejects_query_aliases_and_oversized_headers() {
        assert!(parse(b"GET /.well-known/agent-card.json?write=true HTTP/1.1\r\nHost: localhost\r\n\r\n".to_vec()).is_err());
        let mut request = b"GET / HTTP/1.1\r\nHost: localhost\r\nX-Fill: ".to_vec();
        request.extend(std::iter::repeat_n(b'a', super::MAX_HEADER_BYTES));
        request.extend_from_slice(b"\r\n\r\n");
        assert!(parse(request).is_err());
    }

    #[test]
    fn binds_mutation_authorization_to_the_exact_installed_bearer() {
        let request = Request {
            method: "POST".to_owned(),
            path: "/".to_owned(),
            authorization: Some(Zeroizing::new("Bearer lxa2a_presented".to_owned())),
            body: Vec::new(),
        };
        assert!(authorized(&request, "lxa2a_presented"));
        assert!(!authorized(&request, "lxa2a_other"));

        let absent = Request {
            method: "POST".to_owned(),
            path: "/".to_owned(),
            authorization: None,
            body: Vec::new(),
        };
        assert!(!authorized(&absent, "lxa2a_presented"));
    }
}
