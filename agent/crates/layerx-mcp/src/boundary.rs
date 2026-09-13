//! Execution boundary reached only after the daemon has authorized an invocation.

use std::io::{Read as _, Write as _};
use std::net::TcpStream;
use std::time::Duration;

use serde_json::{json, Map, Value};
use zeroize::Zeroizing;

use crate::server::ToolDefinition;

const MAX_RESPONSE_BYTES: usize = 262_144;
const MINIMUM_BEARER_BYTES: usize = 32;

/// Typed refusal of one authorized invocation. Nothing here is presented as a completed effect.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum BoundaryRefusal {
    NotServed(&'static str),
    Unauthorized,
    Unavailable(String),
    Malformed(String),
}

impl BoundaryRefusal {
    /// Renders the refusal without echoing credentials or argument values.
    #[must_use]
    pub fn detail(&self) -> String {
        match self {
            Self::NotServed(tool) => {
                format!("tool {tool} has no operation on the bound daemon surface")
            }
            Self::Unauthorized => "the daemon refused the bound agent credential".to_owned(),
            Self::Unavailable(reason) => format!("the daemon is unavailable: {reason}"),
            Self::Malformed(reason) => format!("the daemon response is unusable: {reason}"),
        }
    }

    /// Reports whether the refusal leaves the externally visible effect unknown.
    #[must_use]
    pub const fn unknown(&self) -> bool {
        matches!(self, Self::Unavailable(_))
    }
}

/// The only surface an authorized invocation may reach.
pub trait ToolBoundary {
    /// Executes one already authorized tool invocation.
    ///
    /// # Errors
    ///
    /// Returns the boundary's typed refusal; it never fabricates a result.
    fn execute(
        &mut self,
        tool: ToolDefinition,
        arguments: &Value,
    ) -> Result<Value, BoundaryRefusal>;

    /// Reads the core sequence the daemon currently observes.
    ///
    /// # Errors
    ///
    /// Returns a typed refusal when the daemon cannot produce verified freshness.
    fn observed_sequence(&mut self) -> Result<u64, BoundaryRefusal>;
}

/// Authenticated loopback client for the agent daemon's verified program read surface.
pub struct AgentSurface {
    endpoint: String,
    bearer: Zeroizing<String>,
    probe_program: String,
    deadline: Duration,
}

impl AgentSurface {
    /// Binds the loopback endpoint the agent daemon serves its verified reads on.
    ///
    /// # Errors
    ///
    /// Refuses a non-loopback endpoint, a short bearer, a non-canonical probe program, and a
    /// zero deadline.
    pub fn new(
        endpoint: &str,
        bearer: String,
        probe_program: &str,
        deadline: Duration,
    ) -> Result<Self, BoundaryRefusal> {
        if !endpoint.starts_with("127.0.0.1:") {
            return Err(BoundaryRefusal::Malformed(
                "the agent daemon endpoint is not loopback".to_owned(),
            ));
        }
        if bearer.len() < MINIMUM_BEARER_BYTES {
            return Err(BoundaryRefusal::Malformed(
                "the agent daemon bearer is shorter than the daemon accepts".to_owned(),
            ));
        }
        if !is_digest(probe_program) {
            return Err(BoundaryRefusal::Malformed(
                "the agent daemon probe program is not a canonical program id".to_owned(),
            ));
        }
        if deadline.is_zero() {
            return Err(BoundaryRefusal::Malformed(
                "the agent daemon deadline is zero".to_owned(),
            ));
        }
        Ok(Self {
            endpoint: endpoint.to_owned(),
            bearer: Zeroizing::new(bearer),
            probe_program: probe_program.to_ascii_lowercase(),
            deadline,
        })
    }

    /// Reads one verified program balance page from the daemon.
    ///
    /// # Errors
    ///
    /// Returns a typed refusal for an unreachable daemon, a refused credential, and any
    /// response the daemon does not present as verified program state.
    pub fn program_balances(&self, program: &str) -> Result<Value, BoundaryRefusal> {
        if !is_digest(program) {
            return Err(BoundaryRefusal::Malformed(
                "the requested program is not a canonical program id".to_owned(),
            ));
        }
        let path = format!("/v1/programs/{}/balances", program.to_ascii_lowercase());
        let (status, body) = self.get(&path)?;
        match status {
            200 => serde_json::from_str::<Value>(&body)
                .map_err(|error| BoundaryRefusal::Malformed(error.to_string())),
            401 => Err(BoundaryRefusal::Unauthorized),
            503 => Err(BoundaryRefusal::Unavailable(
                "program state is unavailable".to_owned(),
            )),
            other => Err(BoundaryRefusal::Malformed(format!(
                "the daemon answered with status {other}"
            ))),
        }
    }

    /// # Errors
    /// Returns the authenticated daemon refusal or malformed response without fabricating evidence.
    pub fn native_read(&self, path: &str) -> Result<Value, BoundaryRefusal> {
        if !path.starts_with("/v1/reads/")
            || !path
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || b"/?=&_-".contains(&byte))
        {
            return Err(BoundaryRefusal::Malformed(
                "invalid native read path".to_owned(),
            ));
        }
        let (status, body) = self.get(path)?;
        match status {
            200 => serde_json::from_str(&body)
                .map_err(|_| BoundaryRefusal::Malformed("invalid native read result".to_owned())),
            401 => Err(BoundaryRefusal::Unauthorized),
            400 | 413 => Err(BoundaryRefusal::Malformed(
                "the daemon refused the read selector or result bound".to_owned(),
            )),
            _ => Err(BoundaryRefusal::Unavailable(
                "verified native evidence is unavailable".to_owned(),
            )),
        }
    }

    fn get(&self, path: &str) -> Result<(u16, String), BoundaryRefusal> {
        let mut stream = TcpStream::connect(&self.endpoint)
            .map_err(|error| BoundaryRefusal::Unavailable(error.kind().to_string()))?;
        stream
            .set_read_timeout(Some(self.deadline))
            .and_then(|()| stream.set_write_timeout(Some(self.deadline)))
            .map_err(|error| BoundaryRefusal::Unavailable(error.kind().to_string()))?;
        let request = format!(
            "GET {path} HTTP/1.1\r\nHost: {}\r\nAuthorization: Bearer {}\r\nAccept: application/json\r\nConnection: close\r\n\r\n",
            self.endpoint,
            self.bearer.as_str()
        );
        stream
            .write_all(request.as_bytes())
            .and_then(|()| stream.flush())
            .map_err(|error| BoundaryRefusal::Unavailable(error.kind().to_string()))?;
        let mut bytes = Vec::with_capacity(4_096);
        let mut chunk = [0_u8; 4_096];
        loop {
            let read = stream
                .read(&mut chunk)
                .map_err(|error| BoundaryRefusal::Unavailable(error.kind().to_string()))?;
            if read == 0 {
                break;
            }
            if bytes.len().saturating_add(read) > MAX_RESPONSE_BYTES {
                return Err(BoundaryRefusal::Malformed(
                    "the daemon response exceeded its transport bound".to_owned(),
                ));
            }
            bytes.extend_from_slice(&chunk[..read]);
        }
        decode_response(&bytes)
    }
}

fn decode_response(bytes: &[u8]) -> Result<(u16, String), BoundaryRefusal> {
    let split = bytes
        .windows(4)
        .position(|window| window == b"\r\n\r\n")
        .ok_or_else(|| {
            BoundaryRefusal::Malformed("the daemon response has no header boundary".to_owned())
        })?;
    let head = std::str::from_utf8(&bytes[..split]).map_err(|_| {
        BoundaryRefusal::Malformed("the daemon response headers are not UTF-8".to_owned())
    })?;
    let status = head
        .lines()
        .next()
        .and_then(|line| line.split_ascii_whitespace().nth(1))
        .and_then(|code| code.parse::<u16>().ok())
        .ok_or_else(|| {
            BoundaryRefusal::Malformed("the daemon response has no status".to_owned())
        })?;
    let body = std::str::from_utf8(&bytes[split.saturating_add(4)..]).map_err(|_| {
        BoundaryRefusal::Malformed("the daemon response body is not UTF-8".to_owned())
    })?;
    Ok((status, body.to_owned()))
}

fn is_digest(value: &str) -> bool {
    value.len() == 64 && value.bytes().all(|byte| byte.is_ascii_hexdigit())
}

/// Routes the catalogue reads the agent daemon serves and refuses every other tool by name.
pub struct ProgramReads {
    surface: AgentSurface,
}

impl ProgramReads {
    #[must_use]
    pub const fn new(surface: AgentSurface) -> Self {
        Self { surface }
    }

    fn balances(&self, arguments: &Value) -> Result<(Value, Vec<Value>), BoundaryRefusal> {
        let program = text(arguments, "program")?;
        let read = self.surface.program_balances(program)?;
        let freshness = read
            .get("freshness")
            .cloned()
            .ok_or_else(|| BoundaryRefusal::Malformed("no freshness was returned".to_owned()))?;
        let accounts = read
            .get("accounts")
            .and_then(Value::as_array)
            .cloned()
            .ok_or_else(|| BoundaryRefusal::Malformed("no accounts were returned".to_owned()))?;
        let mut selected = Vec::new();
        for entry in accounts {
            if matches(&entry, arguments, "account") && matches(&entry, arguments, "asset") {
                selected.push(entry);
            }
        }
        let mut envelope = Map::new();
        envelope.insert("program".to_owned(), json!(program));
        envelope.insert(
            "lifecycle".to_owned(),
            read.get("lifecycle").cloned().unwrap_or(Value::Null),
        );
        envelope.insert("freshness".to_owned(), freshness);
        Ok((Value::Object(envelope), selected))
    }
}

fn matches(entry: &Value, arguments: &Value, field: &str) -> bool {
    let Some(expected) = arguments.get(field).and_then(Value::as_str) else {
        return true;
    };
    entry
        .get(field)
        .and_then(Value::as_str)
        .is_some_and(|actual| actual.eq_ignore_ascii_case(expected))
}

fn text<'a>(arguments: &'a Value, field: &str) -> Result<&'a str, BoundaryRefusal> {
    arguments
        .get(field)
        .and_then(Value::as_str)
        .ok_or_else(|| BoundaryRefusal::Malformed(format!("argument {field} is absent")))
}

impl ToolBoundary for ProgramReads {
    fn execute(
        &mut self,
        tool: ToolDefinition,
        arguments: &Value,
    ) -> Result<Value, BoundaryRefusal> {
        match tool.name {
            "balance.get" => {
                let (mut envelope, selected) = self.balances(arguments)?;
                if let Some(fields) = envelope.as_object_mut() {
                    fields.insert("balances".to_owned(), Value::Array(selected));
                    fields.insert("evidence".to_owned(), json!(tool.evidence));
                }
                Ok(envelope)
            }
            "wallet.balance" => {
                let (mut envelope, selected) = self.balances(arguments)?;
                let balance = selected.into_iter().next();
                if let Some(fields) = envelope.as_object_mut() {
                    fields.insert("account".to_owned(), json!(text(arguments, "account")?));
                    fields.insert("asset".to_owned(), json!(text(arguments, "asset")?));
                    fields.insert("observed".to_owned(), json!(balance.is_some()));
                    fields.insert("balance".to_owned(), balance.unwrap_or(Value::Null));
                    fields.insert("evidence".to_owned(), json!(tool.evidence));
                }
                Ok(envelope)
            }
            "wallet.accounts" => {
                let (mut envelope, selected) = self.balances(arguments)?;
                let mut accounts = Vec::new();
                for entry in &selected {
                    if let Some(account) = entry.get("account").and_then(Value::as_str) {
                        let value = json!(account);
                        if !accounts.contains(&value) {
                            accounts.push(value);
                        }
                    }
                }
                if let Some(fields) = envelope.as_object_mut() {
                    fields.insert("accounts".to_owned(), Value::Array(accounts));
                    fields.insert("evidence".to_owned(), json!(tool.evidence));
                }
                Ok(envelope)
            }
            "receipt.get" | "proof.get" => {
                let kind = if tool.name == "receipt.get" {
                    "receipt"
                } else {
                    "proof"
                };
                self.surface.native_read(&format!(
                    "/v1/reads/{kind}/{}",
                    text(arguments, "activity_id")?
                ))
            }
            "checkpoint.get" => self.surface.native_read(&format!(
                "/v1/reads/checkpoint/{}",
                text(arguments, "sequence")?
            )),
            "availability.get" => self.surface.native_read(&format!(
                "/v1/reads/availability/{}",
                text(arguments, "batch")?
            )),
            "history.list" => {
                let mut path = format!(
                    "/v1/reads/history/{}?limit={}",
                    text(arguments, "account")?,
                    text(arguments, "limit")?
                );
                if let Some(cursor) = arguments.get("cursor").and_then(Value::as_str) {
                    path.push_str("&cursor=");
                    path.push_str(cursor);
                }
                self.surface.native_read(&path)
            }
            other => Err(BoundaryRefusal::NotServed(static_name(other))),
        }
    }

    fn observed_sequence(&mut self) -> Result<u64, BoundaryRefusal> {
        let read = self
            .surface
            .program_balances(&self.surface.probe_program.clone())?;
        read.pointer("/freshness/observed_sequence")
            .and_then(Value::as_u64)
            .ok_or_else(|| {
                BoundaryRefusal::Malformed(
                    "the daemon returned no observed core sequence".to_owned(),
                )
            })
    }
}

fn static_name(name: &str) -> &'static str {
    crate::server::catalogue()
        .iter()
        .find(|tool| tool.name == name)
        .map_or("unknown", |tool| tool.name)
}
