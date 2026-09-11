//! Model context protocol line transport served directly from a daemon-bound tool surface.

use std::io::{BufRead, Read as _, Write};

use serde_json::{json, Map, Value};

use crate::boundary::{BoundaryRefusal, ToolBoundary};
use crate::catalogue;
use crate::readonly::ReadOnly;
use crate::server::{
    CapabilityDeclaration, DeploymentMode, InvocationOutcome, Server, ServerError, ToolDefinition,
};

const PROTOCOL_VERSION: &str = "2025-06-18";
const MAX_MESSAGE_BYTES: usize = 1_048_576;
const INSTRUCTIONS: &str = "Every result carries the exact protocol material the bound LayerX daemon released after its policy, capability, budget, rate and audit gates. Treat a tool refusal as a refusal, never as a completed effect, and confirm an effect only against verified receipt material.";

/// The daemon-bound server this transport serves, in either deployment mode.
pub enum Bound {
    Full(Box<Server>),
    ReadOnly(Box<ReadOnly>),
}

impl Bound {
    #[must_use]
    pub fn tools(&self) -> &[ToolDefinition] {
        match self {
            Self::Full(server) => server.tools(),
            Self::ReadOnly(server) => server.tools(),
        }
    }

    #[must_use]
    pub const fn mode(&self) -> DeploymentMode {
        match self {
            Self::Full(_) => DeploymentMode::Full,
            Self::ReadOnly(_) => DeploymentMode::ReadOnly,
        }
    }

    #[must_use]
    pub fn capability_declaration(&self) -> CapabilityDeclaration {
        match self {
            Self::Full(server) => server.capability_declaration(),
            Self::ReadOnly(server) => server.capability_declaration(),
        }
    }

    #[must_use]
    pub fn tool(&self, name: &str) -> Option<ToolDefinition> {
        self.tools().iter().find(|tool| tool.name == name).copied()
    }

    #[must_use]
    pub const fn audit_entries(&self) -> u64 {
        match self {
            Self::Full(server) => server.audit_entries(),
            Self::ReadOnly(server) => server.audit_entries(),
        }
    }
}

/// One daemon-bound protocol session. It owns no key material and holds no gateway credential.
pub struct Session<B: ToolBoundary> {
    bound: Bound,
    boundary: B,
}

impl<B: ToolBoundary> Session<B> {
    #[must_use]
    pub const fn new(bound: Bound, boundary: B) -> Self {
        Self { bound, boundary }
    }

    #[must_use]
    pub const fn bound(&self) -> &Bound {
        &self.bound
    }

    /// Serves protocol messages until the reader ends.
    ///
    /// # Errors
    ///
    /// Returns a transport failure or an oversized protocol message; a refused tool is a
    /// protocol result, never a transport error.
    pub fn serve<R: BufRead, W: Write>(
        &mut self,
        reader: &mut R,
        writer: &mut W,
    ) -> Result<(), String> {
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
            let Some(response) = self.handle(message) else {
                continue;
            };
            let encoded = serde_json::to_string(&response)
                .map_err(|error| format!("could not encode a protocol message: {error}"))?;
            writeln!(writer, "{encoded}")
                .and_then(|()| writer.flush())
                .map_err(|error| format!("could not write a protocol message: {error}"))?;
        }
    }

    /// Answers one protocol message, or nothing for a notification.
    #[must_use]
    pub fn handle(&mut self, message: &str) -> Option<Value> {
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
            "initialize" => success(identifier, self.initialize()),
            "ping" => success(identifier, json!({})),
            "tools/list" => success(identifier, json!({"tools": self.listing()})),
            "tools/call" => self.call(identifier, &parameters),
            _ => failure(
                identifier,
                -32601,
                &format!("method {method} is not implemented"),
            ),
        })
    }

    fn initialize(&self) -> Value {
        let declaration = self.bound.capability_declaration();
        json!({
            "protocolVersion": PROTOCOL_VERSION,
            "capabilities": {"tools": {"listChanged": false}},
            "serverInfo": {
                "name": "layerx",
                "title": "LayerX",
                "version": env!("CARGO_PKG_VERSION"),
            },
            "instructions": INSTRUCTIONS,
            "_meta": {
                "layerx/deployment_mode": mode_name(self.bound.mode()),
                "layerx/binding": "agent-daemon",
                "layerx/read_tools": declaration.read_tools,
                "layerx/write_tools": declaration.write_tools,
                "layerx/mutations_reachable": declaration.mutations_reachable,
            },
        })
    }

    fn listing(&self) -> Vec<Value> {
        self.bound
            .tools()
            .iter()
            .filter_map(|tool| catalogue::listing(*tool))
            .collect()
    }

    fn call(&mut self, identifier: Value, parameters: &Value) -> Value {
        let Some(name) = parameters.get("name").and_then(Value::as_str) else {
            return failure(identifier, -32602, "the call did not name a tool");
        };
        let Some(tool) = self.bound.tool(name) else {
            return failure(
                identifier,
                -32602,
                &format!("tool {name} is not served by this deployment"),
            );
        };
        let arguments = parameters.get("arguments").cloned().unwrap_or(Value::Null);
        if let Err(error) = catalogue::validate(tool.name, &arguments) {
            return success(
                identifier,
                content(
                    &json!({"refusal": error.detail(), "tool": tool.name, "stage": "arguments"}),
                    true,
                ),
            );
        }
        match self.invoke(tool, &arguments) {
            Ok(value) => success(
                identifier,
                content(&json!({"tool": tool.name, "result": value}), false),
            ),
            Err(refusal) => success(identifier, content(&refusal, true)),
        }
    }

    fn invoke(&mut self, tool: ToolDefinition, arguments: &Value) -> Result<Value, Value> {
        let core_sequence = self.boundary.observed_sequence().map_err(
            |refusal| json!({"refusal": refusal.detail(), "tool": tool.name, "stage": "freshness"}),
        )?;
        let encoded = serde_json::to_vec(arguments).map_err(
            |error| json!({"refusal": error.to_string(), "tool": tool.name, "stage": "arguments"}),
        )?;
        let Self { bound, boundary } = self;
        let executor = |_: &crate::server::DaemonInvocation| {
            let outcome = boundary.execute(tool, arguments);
            let state = match &outcome {
                Ok(_) => InvocationOutcome::Completed,
                Err(refusal) if refusal.unknown() => InvocationOutcome::Unknown,
                Err(_) => InvocationOutcome::Refused,
            };
            (outcome, state)
        };
        let routed = match bound {
            Bound::Full(server) => match tool.kind {
                crate::server::ToolKind::Read => {
                    server.execute_read(core_sequence, tool.name, encoded, executor)
                }
                crate::server::ToolKind::Write => {
                    server.execute_committed(core_sequence, tool.name, encoded, executor)
                }
            },
            Bound::ReadOnly(server) => {
                server.execute_authorized(core_sequence, tool.name, encoded, executor)
            }
        };
        match routed {
            Ok(Ok(value)) => Ok(value),
            Ok(Err(refusal)) => Err(refusal_value(tool, &refusal)),
            Err(error) => Err(json!({
                "refusal": authority_detail(&error),
                "tool": tool.name,
                "stage": "authority",
            })),
        }
    }
}

fn refusal_value(tool: ToolDefinition, refusal: &BoundaryRefusal) -> Value {
    json!({
        "refusal": refusal.detail(),
        "tool": tool.name,
        "stage": "daemon",
        "state": if refusal.unknown() { "unknown" } else { "refused" },
    })
}

fn authority_detail(error: &ServerError) -> String {
    match error {
        ServerError::MissingSession => "the bound daemon session is absent",
        ServerError::MissingCapability => "the bound capability is absent",
        ServerError::ClosedSession => "the bound daemon session is closed",
        ServerError::RevokedSession => "the bound daemon session is revoked",
        ServerError::TenantMismatch => "the bound session and capability are cross-tenant",
        ServerError::CapabilityMismatch => "the bound capability does not authorize this session",
        ServerError::ExpiredAuthority => "the bound authority has expired",
        ServerError::NoScope => "the bound session carries no scope for this catalogue",
        ServerError::InvalidInvocation => "the invocation is outside the daemon's bounds",
        ServerError::ToolAbsent => "the daemon does not authorize this tool for this session",
        ServerError::WrongServer => "the invocation belongs to another server binding",
        ServerError::AuthorizationUnavailable => "the daemon authorization state is unavailable",
        ServerError::Arithmetic => "the daemon invocation counter is exhausted",
        ServerError::Capability(_) => "the bound capability record is unusable",
        ServerError::Audit(_) => "the daemon audit log could not record this invocation",
    }
    .to_owned()
}

/// Names one deployment mode exactly as the installed manifest declares it.
#[must_use]
pub const fn mode_name(mode: DeploymentMode) -> &'static str {
    match mode {
        DeploymentMode::Full => "full",
        DeploymentMode::ReadOnly => "read-only",
    }
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
