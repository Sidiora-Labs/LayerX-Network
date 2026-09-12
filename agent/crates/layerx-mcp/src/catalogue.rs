//! Descriptions, argument schemas and strict argument validation for the daemon-bound catalogue.

use layerx_types::limits::MAX_DID_BYTES;
use serde_json::{json, Map, Value};

use crate::server::{catalogue, DeploymentMode, ToolDefinition, ToolKind};

const MAX_ARGUMENT_BYTES: usize = 65_536;
const MAX_TEXT_BYTES: usize = 256;
const MAX_PAYLOAD_HEX_BYTES: usize = 8_192;
const MAX_PAGE_ITEMS: u64 = 256;
const MAX_WAIT_MS: u64 = 600_000;

/// Every argument accepted by one catalogue tool.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct Field {
    name: &'static str,
    shape: Shape,
    required: bool,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum Shape {
    Hex32,
    Hex64,
    Bytes,
    Reference,
    Unsigned,
    Bounded(u64),
    Symbol,
    Decimals,
    Did,
}

impl Shape {
    const fn pattern(self) -> Option<&'static str> {
        match self {
            Self::Hex32 => Some("^[0-9a-fA-F]{64}$"),
            Self::Hex64 => Some("^[0-9a-fA-F]{128}$"),
            Self::Bytes => Some("^([0-9a-fA-F]{2})+$"),
            Self::Reference | Self::Symbol => None,
            Self::Unsigned | Self::Bounded(_) => Some("^[0-9]+$"),
            Self::Decimals => Some("^([0-9]|1[0-8])$"),
            Self::Did => Some("^did:[0-9A-Za-z._:-]+$"),
        }
    }

    const fn maximum_length(self) -> usize {
        match self {
            Self::Hex32 => 64,
            Self::Hex64 => 128,
            Self::Bytes => MAX_PAYLOAD_HEX_BYTES,
            Self::Reference => MAX_TEXT_BYTES,
            Self::Unsigned | Self::Bounded(_) => 39,
            Self::Symbol => 32,
            Self::Decimals => 2,
            Self::Did => MAX_DID_BYTES,
        }
    }
}

/// Typed refusal produced before any daemon invocation is created.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ArgumentError {
    NotAnObject,
    TooLarge,
    Unknown(String),
    Missing(&'static str),
    NotText(&'static str),
    Malformed(&'static str),
    OutOfRange(&'static str),
}

impl ArgumentError {
    /// Renders the refusal in the daemon's own vocabulary without echoing argument values.
    #[must_use]
    pub fn detail(&self) -> String {
        match self {
            Self::NotAnObject => "the call arguments are not a JSON object".to_owned(),
            Self::TooLarge => {
                format!("the call arguments exceed {MAX_ARGUMENT_BYTES} encoded bytes")
            }
            Self::Unknown(field) => format!("argument {field} is not accepted by this tool"),
            Self::Missing(field) => format!("argument {field} is required"),
            Self::NotText(field) => format!("argument {field} must be a string"),
            Self::Malformed(field) => format!("argument {field} is not in its declared shape"),
            Self::OutOfRange(field) => format!("argument {field} is outside its declared bound"),
        }
    }
}

const fn required(name: &'static str, shape: Shape) -> Field {
    Field {
        name,
        shape,
        required: true,
    }
}
const fn optional(name: &'static str, shape: Shape) -> Field {
    Field {
        name,
        shape,
        required: false,
    }
}
const BALANCE_GET: [Field; 3] = [
    required("program", Shape::Hex32),
    optional("account", Shape::Hex32),
    optional("asset", Shape::Hex32),
];
const HISTORY_LIST: [Field; 3] = [
    required("account", Shape::Hex32),
    required("limit", Shape::Bounded(MAX_PAGE_ITEMS)),
    optional("cursor", Shape::Hex32),
];
const RECEIPT_GET: [Field; 1] = [required("activity_id", Shape::Hex32)];
const CHECKPOINT_GET: [Field; 1] = [required("sequence", Shape::Unsigned)];
const PROOF_GET: [Field; 1] = [required("activity_id", Shape::Hex32)];
const AVAILABILITY_GET: [Field; 1] = [required("batch", Shape::Hex32)];
const WALLET_ACCOUNTS: [Field; 1] = [required("program", Shape::Hex32)];
const WALLET_BALANCE: [Field; 3] = [
    required("program", Shape::Hex32),
    required("account", Shape::Hex32),
    required("asset", Shape::Hex32),
];
const ACTIVITY_PREPARE: [Field; 7] = [
    required("activity_type", Shape::Unsigned),
    required("payload", Shape::Bytes),
    required("account_sequence", Shape::Unsigned),
    required("not_before_ms", Shape::Unsigned),
    required("expires_at_ms", Shape::Unsigned),
    required("fee_limit", Shape::Unsigned),
    required("idempotency_key", Shape::Hex32),
];
const ACTIVITY_DISCLOSE: [Field; 1] = [required("canonical_bytes", Shape::Bytes)];
const ACTIVITY_SIGN: [Field; 1] = [required("preparation_ref", Shape::Reference)];
const ACTIVITY_SUBMIT: [Field; 3] = [
    required("preparation_ref", Shape::Reference),
    required("signature", Shape::Hex64),
    required("signer_public_key", Shape::Hex32),
];
const ACTIVITY_TRACK: [Field; 1] = [required("submission_ref", Shape::Reference)];
const ACTIVITY_WAIT: [Field; 2] = [
    required("submission_ref", Shape::Reference),
    required("timeout_ms", Shape::Bounded(MAX_WAIT_MS)),
];
const WALLET_SEND: [Field; 4] = [
    required("destination", Shape::Hex32),
    required("asset", Shape::Hex32),
    required("amount", Shape::Unsigned),
    required("idempotency_key", Shape::Hex32),
];
const TOKEN_CREATE: [Field; 4] = [
    required("symbol", Shape::Symbol),
    required("decimals", Shape::Decimals),
    required("supply", Shape::Unsigned),
    required("idempotency_key", Shape::Hex32),
];
const TOKEN_MINT: [Field; 4] = [
    required("asset", Shape::Hex32),
    required("destination", Shape::Hex32),
    required("amount", Shape::Unsigned),
    required("idempotency_key", Shape::Hex32),
];
const TOKEN_TRANSFER: [Field; 4] = [
    required("asset", Shape::Hex32),
    required("destination", Shape::Hex32),
    required("amount", Shape::Unsigned),
    required("idempotency_key", Shape::Hex32),
];
const GRANT_ISSUE: [Field; 5] = [
    required("beneficiary", Shape::Hex32),
    required("asset", Shape::Hex32),
    required("amount", Shape::Unsigned),
    required("expires_at_ms", Shape::Unsigned),
    required("idempotency_key", Shape::Hex32),
];
const GRANT_DRAW: [Field; 3] = [
    required("grant_id", Shape::Hex32),
    required("amount", Shape::Unsigned),
    required("idempotency_key", Shape::Hex32),
];
const FAUCET_REQUEST: [Field; 2] = [
    required("did", Shape::Did),
    required("public_key", Shape::Hex32),
];
const NONE: [Field; 0] = [];

fn fields(name: &str) -> &'static [Field] {
    match name.as_bytes() {
        b"balance.get" => &BALANCE_GET,
        b"history.list" => &HISTORY_LIST,
        b"receipt.get" => &RECEIPT_GET,
        b"checkpoint.get" => &CHECKPOINT_GET,
        b"proof.get" => &PROOF_GET,
        b"availability.get" => &AVAILABILITY_GET,
        b"wallet.accounts" => &WALLET_ACCOUNTS,
        b"wallet.balance" => &WALLET_BALANCE,
        b"activity.prepare" => &ACTIVITY_PREPARE,
        b"activity.disclose" => &ACTIVITY_DISCLOSE,
        b"activity.sign" => &ACTIVITY_SIGN,
        b"activity.submit" => &ACTIVITY_SUBMIT,
        b"activity.track" => &ACTIVITY_TRACK,
        b"activity.wait" => &ACTIVITY_WAIT,
        b"wallet.send" => &WALLET_SEND,
        b"token.create" => &TOKEN_CREATE,
        b"token.mint" => &TOKEN_MINT,
        b"token.transfer" => &TOKEN_TRANSFER,
        b"grant.issue" => &GRANT_ISSUE,
        b"grant.draw" => &GRANT_DRAW,
        b"faucet.request" => &FAUCET_REQUEST,
        _ => &NONE,
    }
}

/// Returns the operator-facing description of one catalogue tool.
#[must_use]
pub fn description(name: &str) -> Option<&'static str> {
    Some(match name.as_bytes() {
        b"balance.get" => {
            "Read verified program-scoped balances from the daemon with their verification level and freshness."
        }
        b"history.list" => {
            "Page verified account history from the daemon under an explicit item bound and stable cursor."
        }
        b"receipt.get" => "Read the canonical receipt the daemon holds for one activity.",
        b"checkpoint.get" => "Read one finalised checkpoint certificate held by the daemon.",
        b"proof.get" => "Read the proof bundle the daemon holds for one activity.",
        b"availability.get" => {
            "Read verified availability chunks and attributed availability failures for one batch."
        }
        b"wallet.accounts" => "List the verified accounts the daemon observes for one program.",
        b"wallet.balance" => "Read one verified account balance for one asset through the daemon.",
        b"activity.prepare" => {
            "Prepare canonical activity bytes and their bound disclosure inside the daemon."
        }
        b"activity.disclose" => "Decode the bound disclosure of canonical activity bytes.",
        b"activity.sign" => {
            "Sign a daemon preparation at the daemon's external signing boundary; no key material is read."
        }
        b"activity.submit" => {
            "Submit an externally signed preparation through the ordinary daemon path."
        }
        b"activity.track" => "Resolve the daemon's current state for one submission.",
        b"activity.wait" => {
            "Wait, under an explicit bound, for the daemon to resolve one submission."
        }
        b"wallet.send" => "Send one asset amount through the ordinary daemon submission path.",
        b"token.create" => "Create one asset through the ordinary daemon submission path.",
        b"token.mint" => "Mint one asset amount through the ordinary daemon submission path.",
        b"token.transfer" => "Transfer one asset amount through the ordinary daemon submission path.",
        b"grant.issue" => "Issue one spending grant through the ordinary daemon submission path.",
        b"grant.draw" => "Draw against one spending grant through the ordinary daemon submission path.",
        b"faucet.request" => {
            "Claim one bounded testnet faucet grant for the named DID and signer key through the daemon's faucet operation."
        }
        _ => return None,
    })
}

/// Returns the JSON Schema an agent runtime must satisfy before the daemon is asked to route.
#[must_use]
pub fn input_schema(name: &str) -> Option<Value> {
    let declared = fields(name);
    if declared.is_empty() && description(name).is_none() {
        return None;
    }
    let mut properties = Map::new();
    let mut required = Vec::new();
    for field in declared {
        let mut property = Map::new();
        property.insert("type".to_owned(), json!("string"));
        property.insert(
            "maxLength".to_owned(),
            json!(u64::try_from(field.shape.maximum_length()).unwrap_or(u64::MAX)),
        );
        if let Some(pattern) = field.shape.pattern() {
            property.insert("pattern".to_owned(), json!(pattern));
        }
        if let Shape::Bounded(bound) = field.shape {
            property.insert("description".to_owned(), json!(format!("1 to {bound}")));
        }
        properties.insert(field.name.to_owned(), Value::Object(property));
        if field.required {
            required.push(json!(field.name));
        }
    }
    Some(json!({
        "type": "object",
        "properties": Value::Object(properties),
        "required": Value::Array(required),
        "additionalProperties": false,
    }))
}

/// Validates call arguments against the declared schema of one catalogue tool.
///
/// # Errors
///
/// Refuses a non-object, an oversized argument object, an unknown or missing field, and any
/// value outside the field's declared shape or bound.
pub fn validate(name: &str, arguments: &Value) -> Result<(), ArgumentError> {
    let empty = Map::new();
    let object = match arguments {
        Value::Object(object) => object,
        Value::Null => &empty,
        _ => return Err(ArgumentError::NotAnObject),
    };
    match serde_json::to_string(arguments) {
        Ok(text) if text.len() <= MAX_ARGUMENT_BYTES => (),
        Ok(_) | Err(_) => return Err(ArgumentError::TooLarge),
    }
    let declared = fields(name);
    for key in object.keys() {
        if !declared.iter().any(|field| field.name == key.as_str()) {
            return Err(ArgumentError::Unknown(key.clone()));
        }
    }
    for field in declared {
        let Some(value) = object.get(field.name) else {
            if field.required {
                return Err(ArgumentError::Missing(field.name));
            }
            continue;
        };
        let text = value.as_str().ok_or(ArgumentError::NotText(field.name))?;
        check(field, text)?;
    }
    Ok(())
}

fn check(field: &Field, text: &str) -> Result<(), ArgumentError> {
    if text.is_empty() || text.len() > field.shape.maximum_length() {
        return Err(ArgumentError::OutOfRange(field.name));
    }
    match field.shape {
        Shape::Hex32 | Shape::Hex64 | Shape::Bytes => {
            if !text.bytes().all(|byte| byte.is_ascii_hexdigit()) || !text.len().is_multiple_of(2) {
                return Err(ArgumentError::Malformed(field.name));
            }
        }
        Shape::Reference | Shape::Symbol => {
            if !text
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-' || byte == b'_')
            {
                return Err(ArgumentError::Malformed(field.name));
            }
        }
        Shape::Unsigned | Shape::Decimals => {
            if !text.bytes().all(|byte| byte.is_ascii_digit()) {
                return Err(ArgumentError::Malformed(field.name));
            }
        }
        Shape::Bounded(bound) => {
            let parsed = text
                .parse::<u64>()
                .map_err(|_| ArgumentError::Malformed(field.name))?;
            if parsed == 0 || parsed > bound {
                return Err(ArgumentError::OutOfRange(field.name));
            }
        }
        Shape::Did => {
            let well_formed = text
                .strip_prefix("did:")
                .and_then(|rest| rest.split_once(':'))
                .is_some_and(|(method, identifier)| !method.is_empty() && !identifier.is_empty());
            if !well_formed
                || !text.bytes().all(|byte| {
                    byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b':' | b'.')
                })
            {
                return Err(ArgumentError::Malformed(field.name));
            }
        }
    }
    if field.shape == Shape::Decimals && text.parse::<u8>().is_ok_and(|value| value > 18) {
        return Err(ArgumentError::OutOfRange(field.name));
    }
    Ok(())
}

/// Returns the catalogue filtered to the tools one deployment mode can reach.
#[must_use]
pub fn surface(mode: DeploymentMode) -> Vec<ToolDefinition> {
    catalogue()
        .iter()
        .filter(|tool| mode == DeploymentMode::Full || tool.kind == ToolKind::Read)
        .copied()
        .collect()
}

/// Renders the MCP `tools/list` entry for one catalogue tool, or `None` outside the catalogue.
#[must_use]
pub fn listing(tool: ToolDefinition) -> Option<Value> {
    let read_only = tool.kind == ToolKind::Read;
    Some(json!({
        "name": tool.name,
        "description": description(tool.name)?,
        "inputSchema": input_schema(tool.name)?,
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
    }))
}
