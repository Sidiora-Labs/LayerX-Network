use std::io::{self, BufRead as _, Read as _, Write as _};

use layerx_mcp::server::{DeploymentMode, ToolDefinition, ToolKind};
use serde_json::{json, Map, Value};

use crate::config::Configuration;
use crate::toolset;

const PROTOCOL_VERSION: &str = "2025-06-18";
const MAX_MESSAGE_BYTES: usize = 1_048_576;
const INSTRUCTIONS: &str = "Every result carries the exact protocol material returned by the bound LayerX environment. Treat a tool refusal as a refusal, never as a completed payment, and confirm a payment only against a verified receipt.";

pub fn serve(
    configuration: &Configuration,
    gateway_credential: &str,
    key: &str,
    source: Option<&str>,
    asset: Option<&str>,
    mode: DeploymentMode,
) -> Result<(), String> {
    let tools = toolset::surface(mode)?;
    let runtime =
        toolset::Runtime::new(configuration, gateway_credential, key, source, asset, mode)?;
    let stdin = io::stdin();
    let mut reader = stdin.lock();
    let stdout = io::stdout();
    let mut writer = stdout.lock();
    let limit = u64::try_from(MAX_MESSAGE_BYTES).unwrap_or(u64::MAX);
    let mut line = String::new();
    loop {
        line.clear();
        let read = reader
            .by_ref()
            .take(limit)
            .read_line(&mut line)
            .map_err(|error| format!("could not read a protocol message: {error}"))?;
        if read == 0 {
            return Ok(());
        }
        if read >= MAX_MESSAGE_BYTES && !line.ends_with('\n') {
            return Err("a protocol message exceeded the transport limit".into());
        }
        let message = line.trim();
        if message.is_empty() {
            continue;
        }
        let Some(response) = handle(&runtime, &tools, mode, message) else {
            continue;
        };
        let encoded = serde_json::to_string(&response)
            .map_err(|error| format!("could not encode a protocol message: {error}"))?;
        writeln!(writer, "{encoded}")
            .and_then(|()| writer.flush())
            .map_err(|error| format!("could not write a protocol message: {error}"))?;
    }
}

fn handle(
    runtime: &toolset::Runtime,
    tools: &[ToolDefinition],
    mode: DeploymentMode,
    message: &str,
) -> Option<Value> {
    let Ok(request) = serde_json::from_str::<Value>(message) else {
        return Some(failure(
            Value::Null,
            -32700,
            "the message is not valid JSON",
        ));
    };
    let identifier = request.get("id").cloned().unwrap_or(Value::Null);
    let Some(method) = request.get("method").and_then(Value::as_str) else {
        return Some(failure(
            identifier,
            -32600,
            "the message did not name a method",
        ));
    };
    if identifier.is_null() {
        return None;
    }
    let parameters = request.get("params").cloned().unwrap_or(Value::Null);
    Some(match method {
        "initialize" => success(identifier, initialize(mode)),
        "ping" => success(identifier, json!({})),
        "tools/list" => success(identifier, json!({"tools": listing(tools)})),
        "tools/call" => call(runtime, tools, identifier, &parameters),
        _ => failure(
            identifier,
            -32601,
            &format!("method {method} is not implemented"),
        ),
    })
}

fn initialize(mode: DeploymentMode) -> Value {
    json!({
        "protocolVersion": PROTOCOL_VERSION,
        "capabilities": {"tools": {"listChanged": false}},
        "serverInfo": {
            "name": "layerx",
            "title": "LayerX",
            "version": env!("CARGO_PKG_VERSION"),
        },
        "instructions": INSTRUCTIONS,
        "_meta": {"layerx/deployment_mode": toolset::mode_name(mode)},
    })
}

fn listing(tools: &[ToolDefinition]) -> Vec<Value> {
    tools
        .iter()
        .map(|tool| {
            let read_only = tool.kind == ToolKind::Read;
            json!({
                "name": tool.name,
                "description": toolset::description(tool.name),
                "inputSchema": toolset::schema(tool.name),
                "annotations": {
                    "readOnlyHint": read_only,
                    "destructiveHint": false,
                    "idempotentHint": read_only,
                    "openWorldHint": true,
                },
                "_meta": {
                    "layerx/scope": tool.required_scope,
                    "layerx/mutation": tool.mutation,
                    "layerx/evidence": tool.evidence,
                },
            })
        })
        .collect()
}

fn call(
    runtime: &toolset::Runtime,
    tools: &[ToolDefinition],
    identifier: Value,
    parameters: &Value,
) -> Value {
    let Some(name) = parameters.get("name").and_then(Value::as_str) else {
        return failure(identifier, -32602, "the call did not name a tool");
    };
    let Some(tool) = tools.iter().find(|tool| tool.name == name).copied() else {
        return failure(
            identifier,
            -32602,
            &format!("tool {name} is not served by this deployment"),
        );
    };
    let arguments = parameters.get("arguments").cloned().unwrap_or(Value::Null);
    match toolset::invoke(runtime, tool, &arguments) {
        Ok(value) => {
            let refused = gateway_state(&value) == Some("refused");
            success(identifier, content(&json!({"result": value}), refused))
        }
        Err(error) => success(
            identifier,
            content(&json!({"refusal": error, "tool": tool.name}), true),
        ),
    }
}

fn gateway_state(value: &Value) -> Option<&str> {
    value
        .pointer("/gateway/state")
        .or_else(|| value.pointer("/gateway/result/state"))
        .and_then(Value::as_str)
}

fn content(value: &Value, refused: bool) -> Value {
    let rendered = serde_json::to_string_pretty(value)
        .unwrap_or_else(|_| "{\"refusal\":\"result encoding failed\"}".to_owned());
    json!({
        "content": [{"type": "text", "text": rendered}],
        "structuredContent": value,
        "isError": refused,
    })
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
