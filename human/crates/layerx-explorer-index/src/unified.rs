//! One unified account across the Paxeer X Network's two execution domains.
//!
//! A visitor may spell the same account as an EVM address, a `did:layerx`
//! identifier or a LayerX account identifier. This module resolves all three
//! spellings to one canonical key through the network gateway, joins balances
//! across the custody asset map, and reads a bounded window of Paxeer-side
//! custody and binding activity through `eth_getLogs`.
//!
//! Nothing here carries a protocol proof: the gateway answers are typed
//! `Evidence::GatewayReported` so no caller can mistake them for the
//! receipt-verified rows the index builds from availability data.

use std::fmt;
use std::io::{Read, Write};
use std::net::{TcpStream, ToSocketAddrs as _};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

use layerx_programs::hex;
use serde_json::Value;
use sha3::{Digest as _, Keccak256};

use crate::{AccountActivityRecord, Freshness, Page};

/// The custody precompile that emits deposit, claim and exit events.
pub const CUSTODY_PRECOMPILE: [u8; 20] = [
    0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
    0x00, 0x00, 0x10, 0x13,
];

/// The address precompile that emits LayerX bind and unbind events.
pub const ADDR_PRECOMPILE: [u8; 20] = [
    0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
    0x00, 0x00, 0x10, 0x04,
];

const DID_PREFIX: &str = "did:layerx:";
const ANSWER_LIMIT: u64 = 8 * 1024 * 1024;
const IO_TIMEOUT: Duration = Duration::from_secs(10);
const MAXIMUM_ACTIVITY_LIMIT: usize = 100;
const MAXIMUM_JOINED_BALANCES: usize = 1_024;
const MAXIMUM_LOGS_PER_CHUNK: usize = 10_000;
const DENOM_LIMIT: usize = 128;
const PAX_ADDRESS_LIMIT: usize = 128;
const STATUS_NAME_LIMIT: usize = 64;
const NETWORK_ID_LIMIT: usize = 64;

static NEXT_REQUEST: AtomicU64 = AtomicU64::new(1);

/// How a fact reached the explorer. The index never upgrades a gateway answer
/// to a verified level: the two sources stay distinguishable at every hop.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Evidence {
    /// Reported by the network gateway without an index-checkable proof.
    GatewayReported,
}

impl Evidence {
    #[must_use]
    pub const fn label(self) -> &'static str {
        match self {
            Self::GatewayReported => "gateway-reported",
        }
    }
}

/// Refusal for a spelling that is not one of the three public account forms.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct IdentifierError;

impl fmt::Display for IdentifierError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(
            "account identifier is not an EVM address, a did:layerx identifier or a LayerX account",
        )
    }
}

impl std::error::Error for IdentifierError {}

/// One of the three public spellings of the same unified account.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum AccountIdentifier {
    /// A twenty-byte Paxeer EVM address.
    Evm([u8; 20]),
    /// A LayerX decentralised identifier public key.
    Did([u8; 32]),
    /// A LayerX account identifier.
    Account([u8; 32]),
}

impl AccountIdentifier {
    /// Parses and normalises any of the three public spellings.
    ///
    /// # Errors
    /// Refuses every other spelling, including partial or over-long hexadecimal.
    pub fn parse(text: &str) -> Result<Self, IdentifierError> {
        let trimmed = text.trim();
        let lowered = trimmed.to_ascii_lowercase();
        if let Some(body) = lowered.strip_prefix("0x") {
            return decode_evm(body).map(Self::Evm);
        }
        if let Some(body) = lowered.strip_prefix(DID_PREFIX) {
            return hex::decode_digest(body)
                .map(Self::Did)
                .map_err(|_| IdentifierError);
        }
        hex::decode_digest(&lowered)
            .map(Self::Account)
            .map_err(|_| IdentifierError)
    }

    /// Renders the exact normalised spelling this identifier is addressed by.
    #[must_use]
    pub fn canonical_text(self) -> String {
        match self {
            Self::Evm(address) => format!("0x{}", hex::encode(&address)),
            Self::Did(key) => format!("{DID_PREFIX}{}", hex::encode(&key)),
            Self::Account(account) => hex::encode(&account),
        }
    }

    /// The EVM address this identifier names directly, if it names one.
    #[must_use]
    pub const fn evm_address(self) -> Option<[u8; 20]> {
        match self {
            Self::Evm(address) => Some(address),
            Self::Did(_) | Self::Account(_) => None,
        }
    }
}

impl fmt::Display for AccountIdentifier {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.canonical_text())
    }
}

fn decode_evm(body: &str) -> Result<[u8; 20], IdentifierError> {
    let bytes = hex::decode(body).map_err(|_| IdentifierError)?;
    <[u8; 20]>::try_from(bytes.as_slice()).map_err(|_| IdentifierError)
}

/// Both identities of one account as the gateway reports them.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ResolvedIdentities {
    pub evm_address: Option<[u8; 20]>,
    pub pax_address: Option<String>,
    pub layerx_did: Option<[u8; 32]>,
    pub layerx_account: Option<[u8; 32]>,
    pub bound: bool,
    pub evidence: Evidence,
}

impl ResolvedIdentities {
    /// The one identifier this account's single page lives at: the LayerX
    /// account when the network knows one, otherwise the spelling asked for.
    #[must_use]
    pub fn canonical(&self, requested: AccountIdentifier) -> AccountIdentifier {
        if let Some(account) = self.layerx_account {
            return AccountIdentifier::Account(account);
        }
        if let Some(address) = self.evm_address {
            if requested.evm_address().is_some() {
                return AccountIdentifier::Evm(address);
            }
        }
        requested
    }
}

/// One asset held by the same account in either domain, joined through the
/// custody asset map.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct JoinedBalance {
    pub asset_id: [u8; 32],
    pub denom: String,
    pub custody: Option<u128>,
    pub paxeer: Option<u128>,
    pub layerx: Option<u128>,
}

/// The bounded joined balance table for one account.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct JoinedBalances {
    pub items: Vec<JoinedBalance>,
    pub joined_limit: u64,
    pub evidence: Evidence,
}

/// The three settlement rungs the network shows, each with the coordinate the
/// network itself reports for it.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SettlementLadder {
    pub network_id: String,
    pub chain_id: u64,
    /// Instant: the Paxeer block the gateway currently reports as latest.
    pub instant_block: u64,
    /// Final: the newest batch the anchor reports as finalised.
    pub finalized_batch: u64,
    pub anchor_status: u64,
    pub anchor_status_name: String,
    pub evidence: Evidence,
}

/// One Paxeer-side event this account took part in.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PaxeerEvent {
    CustodyDeposit,
    ClaimQueued,
    ClaimFinalised,
    CustodyRelease,
    EmergencyExit,
    LayerXBound,
    LayerXUnbound,
}

impl PaxeerEvent {
    /// Every event the bounded reader admits, in declaration order.
    pub const ALL: [Self; 7] = [
        Self::CustodyDeposit,
        Self::ClaimQueued,
        Self::ClaimFinalised,
        Self::CustodyRelease,
        Self::EmergencyExit,
        Self::LayerXBound,
        Self::LayerXUnbound,
    ];

    /// The exact ABI signature the topic is derived from.
    #[must_use]
    pub const fn signature(self) -> &'static str {
        match self {
            Self::CustodyDeposit => "CustodyDeposit(bytes32,bytes32,address,bytes32,uint256,uint64)",
            Self::ClaimQueued => "ClaimQueued(bytes32,bytes32,bytes32,bytes32,address,uint256,uint64)",
            Self::ClaimFinalised => "ClaimFinalised(bytes32,bytes32)",
            Self::CustodyRelease => "CustodyRelease(bytes32,bytes32,address,uint256,address)",
            Self::EmergencyExit => {
                "EmergencyExitExecuted(bytes32,bytes32,bytes32,bytes32,bytes32,address,uint256)"
            }
            Self::LayerXBound => "LayerXBound(address,bytes32,uint64)",
            Self::LayerXUnbound => "LayerXUnbound(address,bytes32,uint64)",
        }
    }

    /// The stable public name of this event.
    #[must_use]
    pub const fn label(self) -> &'static str {
        match self {
            Self::CustodyDeposit => "custody-deposit",
            Self::ClaimQueued => "claim-queued",
            Self::ClaimFinalised => "claim-finalised",
            Self::CustodyRelease => "custody-release",
            Self::EmergencyExit => "emergency-exit",
            Self::LayerXBound => "bound",
            Self::LayerXUnbound => "unbound",
        }
    }

    /// The precompile that emits this event.
    #[must_use]
    pub const fn emitter(self) -> [u8; 20] {
        match self {
            Self::LayerXBound | Self::LayerXUnbound => ADDR_PRECOMPILE,
            Self::CustodyDeposit
            | Self::ClaimQueued
            | Self::ClaimFinalised
            | Self::CustodyRelease
            | Self::EmergencyExit => CUSTODY_PRECOMPILE,
        }
    }

    /// The keccak-256 topic of this event's ABI signature.
    #[must_use]
    pub fn topic(self) -> [u8; 32] {
        Keccak256::digest(self.signature().as_bytes()).into()
    }

    /// Recovers the event a log's first topic names.
    #[must_use]
    pub fn from_topic(topic: [u8; 32]) -> Option<Self> {
        Self::ALL.into_iter().find(|event| event.topic() == topic)
    }
}

/// One decoded Paxeer-side log line.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PaxeerActivityRecord {
    pub event: PaxeerEvent,
    pub block_number: u64,
    pub log_index: u64,
    pub transaction_hash: [u8; 32],
    pub asset_id: Option<[u8; 32]>,
    pub amount: Option<u128>,
    /// The EVM address the event names: payer, recipient or bound address.
    pub address: Option<[u8; 20]>,
    /// The LayerX account or identifier key the event names.
    pub account: Option<[u8; 32]>,
    pub evidence: Evidence,
}

impl PaxeerActivityRecord {
    /// Whether this log belongs on the page for these identities.
    #[must_use]
    pub fn concerns(&self, address: Option<[u8; 20]>, accounts: &[[u8; 32]]) -> bool {
        if address.is_some() && self.address == address {
            return true;
        }
        self.account
            .is_some_and(|value| accounts.contains(&value))
    }
}

/// One bounded newest-first page of Paxeer-side activity and the exact block
/// range it was read from.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PaxeerActivityPage {
    pub items: Vec<PaxeerActivityRecord>,
    pub from_block: u64,
    pub to_block: u64,
    /// The exclusive upper block of the next older page, when the bounded
    /// window has not been exhausted.
    pub next_before_block: Option<u64>,
    pub evidence: Evidence,
}

/// The bounded recent-window policy for the Paxeer-side reader. There is no
/// full-chain EVM index behind this: every read states the range it covers.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ActivityWindow {
    /// How many blocks back from the head the reader may look.
    pub span_blocks: u64,
    /// How many blocks one `eth_getLogs` request may cover.
    pub chunk_blocks: u64,
    /// How many requests one page may issue.
    pub max_chunks: u32,
    /// How many matched rows one page may return.
    pub limit: usize,
}

impl ActivityWindow {
    /// Refuses a window that is empty, unbounded or over the page ceiling.
    ///
    /// # Errors
    /// Returns [`GatewayError::InvalidWindow`] for a non-canonical bound.
    pub const fn validate(self) -> Result<Self, GatewayError> {
        if self.span_blocks == 0
            || self.chunk_blocks == 0
            || self.chunk_blocks > self.span_blocks
            || self.max_chunks == 0
            || self.limit == 0
            || self.limit > MAXIMUM_ACTIVITY_LIMIT
        {
            return Err(GatewayError::InvalidWindow);
        }
        Ok(self)
    }

    /// The newest-first block ranges one page reads, oldest bound included.
    #[must_use]
    pub fn chunks(self, head: u64, before: Option<u64>) -> Vec<(u64, u64)> {
        let mut ranges = Vec::new();
        let floor = head.saturating_sub(self.span_blocks.saturating_sub(1));
        let Some(mut to) = before.map_or(Some(head), |value| value.checked_sub(1)) else {
            return ranges;
        };
        to = to.min(head);
        while ranges.len() < self.max_chunks as usize && to >= floor {
            let from = to
                .saturating_sub(self.chunk_blocks.saturating_sub(1))
                .max(floor);
            ranges.push((from, to));
            if from == floor {
                break;
            }
            to = from.saturating_sub(1);
        }
        ranges
    }

    /// The exclusive upper block of the next older page, or `None` when the
    /// bounded window is exhausted.
    #[must_use]
    pub fn next_before(self, head: u64, lowest_read: u64) -> Option<u64> {
        let floor = head.saturating_sub(self.span_blocks.saturating_sub(1));
        (lowest_read > floor).then_some(lowest_read)
    }
}

/// Every way a gateway read can fail to produce a usable answer.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum GatewayError {
    /// The endpoint is not `http://<host>[:<port>]` or `https://<host>[:<port>]`.
    InvalidEndpoint,
    /// The declared activity window is empty or over its ceiling.
    InvalidWindow,
    /// The endpoint could not be reached or answered outside HTTP.
    Transport(String),
    /// The gateway answered a JSON-RPC error.
    Refused { code: i64, message: String },
    /// The answer is not a JSON-RPC answer for the request that was sent.
    Unbound,
    /// The answer is not the documented gateway document.
    MalformedAnswer,
}

impl fmt::Display for GatewayError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidEndpoint => formatter.write_str("gateway endpoint is not http(s)://host"),
            Self::InvalidWindow => formatter.write_str("activity window is outside its bounds"),
            Self::Transport(error) => write!(formatter, "gateway transport failed: {error}"),
            Self::Refused { code, message } => {
                write!(formatter, "gateway refused the read: {code} {message}")
            }
            Self::Unbound => formatter.write_str("gateway answer is bound to another request"),
            Self::MalformedAnswer => formatter.write_str("gateway answer is malformed"),
        }
    }
}

impl std::error::Error for GatewayError {}

/// The network gateway's JSON-RPC endpoint: one endpoint for `px_*` joins and
/// the unchanged `eth_*` reads.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct GatewayEndpoint {
    secure: bool,
    host: String,
    port: u16,
    path: String,
}

impl GatewayEndpoint {
    /// Parses `http://host[:port][/path]` or `https://host[:port][/path]`.
    ///
    /// # Errors
    /// Refuses another scheme, an empty host, a zero port and a host outside
    /// the ASCII host grammar.
    pub fn parse(endpoint: &str) -> Result<Self, GatewayError> {
        let trimmed = endpoint.trim();
        let (secure, rest) = if let Some(rest) = trimmed.strip_prefix("https://") {
            (true, rest)
        } else if let Some(rest) = trimmed.strip_prefix("http://") {
            (false, rest)
        } else {
            return Err(GatewayError::InvalidEndpoint);
        };
        let (authority, path) = rest.split_once('/').map_or((rest, "/".to_owned()), |(authority, path)| {
            (authority, format!("/{path}"))
        });
        let (host, port) = match authority.rsplit_once(':') {
            Some((host, port)) => (
                host,
                port.parse::<u16>()
                    .map_err(|_| GatewayError::InvalidEndpoint)?,
            ),
            None => (authority, if secure { 443 } else { 80 }),
        };
        if port == 0
            || host.is_empty()
            || !host
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || byte == b'.' || byte == b'-')
        {
            return Err(GatewayError::InvalidEndpoint);
        }
        Ok(Self {
            secure,
            host: host.to_ascii_lowercase(),
            port,
            path,
        })
    }

    /// Posts one JSON-RPC request and returns its bounded answer body.
    ///
    /// # Errors
    /// Reports connection, TLS and HTTP framing failures.
    pub fn post(&self, body: &str) -> Result<Vec<u8>, GatewayError> {
        let head = format!(
            "POST {} HTTP/1.1\r\nHost: {}:{}\r\nContent-Type: application/json\r\nAccept: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
            self.path,
            self.host,
            self.port,
            body.len()
        );
        let mut last = "gateway endpoint has no address".to_owned();
        let mut connected = None;
        for address in (self.host.as_str(), self.port)
            .to_socket_addrs()
            .map_err(|error| GatewayError::Transport(format!("resolution failed: {error}")))?
        {
            match TcpStream::connect_timeout(&address, IO_TIMEOUT) {
                Ok(stream) => {
                    connected = Some(stream);
                    break;
                }
                Err(error) => last = format!("connection failed: {error}"),
            }
        }
        let stream = connected.ok_or(GatewayError::Transport(last))?;
        stream
            .set_read_timeout(Some(IO_TIMEOUT))
            .and_then(|()| stream.set_write_timeout(Some(IO_TIMEOUT)))
            .map_err(|error| GatewayError::Transport(format!("timeout setup failed: {error}")))?;
        let answer = if self.secure {
            let connector = native_tls::TlsConnector::builder()
                .build()
                .map_err(|error| GatewayError::Transport(format!("TLS setup failed: {error}")))?;
            let mut stream = connector
                .connect(&self.host, stream)
                .map_err(|error| GatewayError::Transport(format!("TLS handshake failed: {error}")))?;
            exchange(&mut stream, &head, body.as_bytes())?
        } else {
            let mut stream = stream;
            exchange(&mut stream, &head, body.as_bytes())?
        };
        http_body(&answer)
    }
}

fn exchange<S: Read + Write>(
    stream: &mut S,
    head: &str,
    body: &[u8],
) -> Result<Vec<u8>, GatewayError> {
    stream
        .write_all(head.as_bytes())
        .and_then(|()| stream.write_all(body))
        .and_then(|()| stream.flush())
        .map_err(|error| GatewayError::Transport(format!("request failed: {error}")))?;
    let mut answer = Vec::new();
    Read::take(&mut *stream, ANSWER_LIMIT + 1)
        .read_to_end(&mut answer)
        .map_err(|error| GatewayError::Transport(format!("answer failed: {error}")))?;
    if u64::try_from(answer.len()).map_or(true, |length| length > ANSWER_LIMIT) {
        return Err(GatewayError::Transport(
            "answer exceeds its size limit".to_owned(),
        ));
    }
    Ok(answer)
}

/// Extracts the body of one HTTP/1.1 answer, failing closed on a refusal.
///
/// # Errors
/// Refuses an unframed answer, a non-200 status and a truncated body.
pub fn http_body(answer: &[u8]) -> Result<Vec<u8>, GatewayError> {
    let end = answer
        .windows(4)
        .position(|value| value == b"\r\n\r\n")
        .ok_or_else(|| GatewayError::Transport("answer has no headers".to_owned()))?;
    let head = std::str::from_utf8(answer.get(..end).unwrap_or_default())
        .map_err(|_| GatewayError::Transport("answer headers are not UTF-8".to_owned()))?;
    let body = answer.get(end + 4..).unwrap_or_default();
    let status = head
        .lines()
        .next()
        .and_then(|line| line.split_ascii_whitespace().nth(1))
        .and_then(|code| code.parse::<u16>().ok())
        .ok_or_else(|| GatewayError::Transport("answer has no status".to_owned()))?;
    if status != 200 {
        return Err(GatewayError::Transport(format!(
            "gateway answered HTTP {status}"
        )));
    }
    Ok(body.to_vec())
}

/// Renders one positional-parameter JSON-RPC request.
#[must_use]
pub fn rpc_request(id: u64, method: &str, params: &[Value]) -> String {
    serde_json::json!({
        "jsonrpc": "2.0",
        "id": id,
        "method": method,
        "params": params,
    })
    .to_string()
}

/// Interprets one JSON-RPC answer for the request `id`, failing closed.
///
/// # Errors
/// Refuses a malformed envelope, an answer for another request, and returns
/// the gateway's own refusal as [`GatewayError::Refused`].
pub fn rpc_result(id: u64, answer: &[u8]) -> Result<Value, GatewayError> {
    let document: Value =
        serde_json::from_slice(answer).map_err(|_| GatewayError::MalformedAnswer)?;
    if document["jsonrpc"] != Value::String("2.0".to_owned()) {
        return Err(GatewayError::MalformedAnswer);
    }
    if document["id"].as_u64() != Some(id) {
        return Err(GatewayError::Unbound);
    }
    if let Some(error) = document.get("error").filter(|value| !value.is_null()) {
        return Err(GatewayError::Refused {
            code: error["code"].as_i64().unwrap_or(0),
            message: error["message"].as_str().unwrap_or("unspecified").to_owned(),
        });
    }
    document
        .get("result")
        .filter(|value| !value.is_null())
        .cloned()
        .ok_or(GatewayError::MalformedAnswer)
}

fn optional_text(value: &Value, limit: usize) -> Result<Option<String>, GatewayError> {
    match value {
        Value::Null => Ok(None),
        Value::String(text) if !text.is_empty() && text.len() <= limit => Ok(Some(text.clone())),
        _ => Err(GatewayError::MalformedAnswer),
    }
}

fn required_text(value: &Value, limit: usize) -> Result<String, GatewayError> {
    optional_text(value, limit)?.ok_or(GatewayError::MalformedAnswer)
}

fn optional_address(value: &Value) -> Result<Option<[u8; 20]>, GatewayError> {
    match value {
        Value::Null => Ok(None),
        Value::String(text) if text.is_empty() => Ok(None),
        Value::String(text) => address(text).map(Some),
        _ => Err(GatewayError::MalformedAnswer),
    }
}

fn address(text: &str) -> Result<[u8; 20], GatewayError> {
    let lowered = text.trim().to_ascii_lowercase();
    let body = lowered
        .strip_prefix("0x")
        .ok_or(GatewayError::MalformedAnswer)?;
    decode_evm(body).map_err(|_| GatewayError::MalformedAnswer)
}

fn optional_digest(value: &Value) -> Result<Option<[u8; 32]>, GatewayError> {
    match value {
        Value::Null => Ok(None),
        Value::String(text) if text.is_empty() => Ok(None),
        Value::String(text) => digest(text).map(Some),
        _ => Err(GatewayError::MalformedAnswer),
    }
}

fn digest(text: &str) -> Result<[u8; 32], GatewayError> {
    let lowered = text.trim().to_ascii_lowercase();
    let body = lowered
        .strip_prefix(DID_PREFIX)
        .or_else(|| lowered.strip_prefix("0x"))
        .unwrap_or(&lowered);
    hex::decode_digest(body).map_err(|_| GatewayError::MalformedAnswer)
}

fn boolean(value: &Value) -> Result<bool, GatewayError> {
    value.as_bool().ok_or(GatewayError::MalformedAnswer)
}

/// Decodes a gateway quantity: an unsigned decimal string, a `0x` quantity or
/// a JSON integer. Nothing else is admitted.
///
/// # Errors
/// Refuses every other spelling and any value beyond 128 bits.
pub fn quantity(value: &Value) -> Result<u128, GatewayError> {
    match value {
        Value::Number(number) => number.as_u64().map(u128::from).ok_or(GatewayError::MalformedAnswer),
        Value::String(text) => {
            let trimmed = text.trim();
            if let Some(body) = trimmed
                .strip_prefix("0x")
                .or_else(|| trimmed.strip_prefix("0X"))
            {
                if body.is_empty() || body.len() > 32 || !body.bytes().all(|b| b.is_ascii_hexdigit())
                {
                    return Err(GatewayError::MalformedAnswer);
                }
                return u128::from_str_radix(body, 16).map_err(|_| GatewayError::MalformedAnswer);
            }
            trimmed
                .parse::<u128>()
                .map_err(|_| GatewayError::MalformedAnswer)
        }
        _ => Err(GatewayError::MalformedAnswer),
    }
}

fn optional_quantity(value: &Value) -> Result<Option<u128>, GatewayError> {
    if value.is_null() {
        return Ok(None);
    }
    quantity(value).map(Some)
}

fn counted(value: &Value) -> Result<u64, GatewayError> {
    u64::try_from(quantity(value)?).map_err(|_| GatewayError::MalformedAnswer)
}

/// Decodes the `px_resolveAccount` result.
///
/// # Errors
/// Refuses a document that is not the declared shape.
pub fn decode_identities(result: &Value) -> Result<ResolvedIdentities, GatewayError> {
    if !result.is_object() {
        return Err(GatewayError::MalformedAnswer);
    }
    Ok(ResolvedIdentities {
        evm_address: optional_address(&result["evm_address"])?,
        pax_address: optional_text(&result["pax_address"], PAX_ADDRESS_LIMIT)?,
        layerx_did: optional_digest(&result["layerx_did"])?,
        layerx_account: optional_digest(&result["layerx_account"])?,
        bound: boolean(&result["bound"])?,
        evidence: Evidence::GatewayReported,
    })
}

/// Decodes the `px_getBalances` result.
///
/// # Errors
/// Refuses a document that is not the declared shape or exceeds the joined
/// balance ceiling.
pub fn decode_balances(result: &Value) -> Result<JoinedBalances, GatewayError> {
    let rows = result["balances"]
        .as_array()
        .ok_or(GatewayError::MalformedAnswer)?;
    if rows.len() > MAXIMUM_JOINED_BALANCES {
        return Err(GatewayError::MalformedAnswer);
    }
    let mut items = Vec::with_capacity(rows.len());
    for row in rows {
        items.push(JoinedBalance {
            asset_id: digest(
                row["asset_id"]
                    .as_str()
                    .ok_or(GatewayError::MalformedAnswer)?,
            )?,
            denom: required_text(&row["denom"], DENOM_LIMIT)?,
            custody: optional_quantity(&row["custody"])?,
            paxeer: optional_quantity(&row["paxeer"])?,
            layerx: optional_quantity(&row["layerx"])?,
        });
    }
    Ok(JoinedBalances {
        items,
        joined_limit: counted(&result["joined_limit"])?,
        evidence: Evidence::GatewayReported,
    })
}

/// Decodes the `px_getNetwork` result into the settlement ladder.
///
/// # Errors
/// Refuses a document that is not the declared shape.
pub fn decode_settlement(result: &Value) -> Result<SettlementLadder, GatewayError> {
    let paxeer = &result["paxeer"];
    let anchor = &result["anchor"];
    Ok(SettlementLadder {
        network_id: required_text(&result["network_id"], NETWORK_ID_LIMIT)?,
        chain_id: counted(&paxeer["chain_id"])?,
        instant_block: counted(&paxeer["latest_block"])?,
        finalized_batch: counted(&anchor["latest_finalized_batch"])?,
        anchor_status: counted(&anchor["status"])?,
        anchor_status_name: required_text(&anchor["status_name"], STATUS_NAME_LIMIT)?,
        evidence: Evidence::GatewayReported,
    })
}

fn topic(log: &Value, position: usize) -> Result<[u8; 32], GatewayError> {
    let topics = log["topics"]
        .as_array()
        .ok_or(GatewayError::MalformedAnswer)?;
    let value = topics.get(position).ok_or(GatewayError::MalformedAnswer)?;
    digest(value.as_str().ok_or(GatewayError::MalformedAnswer)?)
}

fn topic_address(log: &Value, position: usize) -> Result<[u8; 20], GatewayError> {
    let word = topic(log, position)?;
    let (padding, tail) = word.split_at(12);
    if padding.iter().any(|byte| *byte != 0) {
        return Err(GatewayError::MalformedAnswer);
    }
    <[u8; 20]>::try_from(tail).map_err(|_| GatewayError::MalformedAnswer)
}

fn data_words(log: &Value) -> Result<Vec<[u8; 32]>, GatewayError> {
    let text = log["data"].as_str().ok_or(GatewayError::MalformedAnswer)?;
    let lowered = text.trim().to_ascii_lowercase();
    let body = lowered
        .strip_prefix("0x")
        .ok_or(GatewayError::MalformedAnswer)?;
    let bytes = hex::decode(body).map_err(|_| GatewayError::MalformedAnswer)?;
    if bytes.len() % 32 != 0 {
        return Err(GatewayError::MalformedAnswer);
    }
    bytes
        .chunks_exact(32)
        .map(|chunk| <[u8; 32]>::try_from(chunk).map_err(|_| GatewayError::MalformedAnswer))
        .collect()
}

fn word_address(word: [u8; 32]) -> Result<[u8; 20], GatewayError> {
    let (padding, tail) = word.split_at(12);
    if padding.iter().any(|byte| *byte != 0) {
        return Err(GatewayError::MalformedAnswer);
    }
    <[u8; 20]>::try_from(tail).map_err(|_| GatewayError::MalformedAnswer)
}

fn word_amount(word: [u8; 32]) -> Result<u128, GatewayError> {
    let (high, low) = word.split_at(16);
    if high.iter().any(|byte| *byte != 0) {
        return Err(GatewayError::MalformedAnswer);
    }
    <[u8; 16]>::try_from(low)
        .map(u128::from_be_bytes)
        .map_err(|_| GatewayError::MalformedAnswer)
}

fn word(words: &[[u8; 32]], position: usize) -> Result<[u8; 32], GatewayError> {
    words
        .get(position)
        .copied()
        .ok_or(GatewayError::MalformedAnswer)
}

/// Decodes one `eth_getLogs` entry emitted by the custody or address
/// precompile. Logs of any other shape are refused, never guessed at.
///
/// # Errors
/// Refuses a log whose topic is unknown, whose emitter does not match the
/// event, or whose data does not decode against the declared ABI.
pub fn decode_log(log: &Value) -> Result<PaxeerActivityRecord, GatewayError> {
    let emitter = address(log["address"].as_str().ok_or(GatewayError::MalformedAnswer)?)?;
    let event = PaxeerEvent::from_topic(topic(log, 0)?).ok_or(GatewayError::MalformedAnswer)?;
    if event.emitter() != emitter {
        return Err(GatewayError::MalformedAnswer);
    }
    let words = data_words(log)?;
    let (asset_id, amount, evm, account) = match event {
        PaxeerEvent::CustodyDeposit => (
            Some(topic(log, 2)?),
            Some(word_amount(word(&words, 1)?)?),
            Some(topic_address(log, 3)?),
            Some(word(&words, 0)?),
        ),
        PaxeerEvent::ClaimQueued => (
            Some(word(&words, 0)?),
            Some(word_amount(word(&words, 2)?)?),
            Some(word_address(word(&words, 1)?)?),
            None,
        ),
        PaxeerEvent::ClaimFinalised => (None, None, None, None),
        PaxeerEvent::CustodyRelease => (
            Some(topic(log, 2)?),
            Some(word_amount(word(&words, 0)?)?),
            Some(topic_address(log, 3)?),
            None,
        ),
        PaxeerEvent::EmergencyExit => (
            Some(word(&words, 1)?),
            Some(word_amount(word(&words, 3)?)?),
            Some(word_address(word(&words, 2)?)?),
            Some(word(&words, 0)?),
        ),
        PaxeerEvent::LayerXBound | PaxeerEvent::LayerXUnbound => (
            None,
            None,
            Some(topic_address(log, 1)?),
            Some(topic(log, 2)?),
        ),
    };
    Ok(PaxeerActivityRecord {
        event,
        block_number: counted(&log["blockNumber"])?,
        log_index: counted(&log["logIndex"])?,
        transaction_hash: digest(
            log["transactionHash"]
                .as_str()
                .ok_or(GatewayError::MalformedAnswer)?,
        )?,
        asset_id,
        amount,
        address: evm,
        account,
        evidence: Evidence::GatewayReported,
    })
}

/// Renders the `eth_getLogs` filter for one bounded block range over both
/// precompiles and every event the reader decodes.
#[must_use]
pub fn logs_filter(from_block: u64, to_block: u64) -> Value {
    serde_json::json!({
        "fromBlock": format!("0x{from_block:x}"),
        "toBlock": format!("0x{to_block:x}"),
        "address": [
            format!("0x{}", hex::encode(&CUSTODY_PRECOMPILE)),
            format!("0x{}", hex::encode(&ADDR_PRECOMPILE)),
        ],
        "topics": [PaxeerEvent::ALL
            .iter()
            .map(|event| format!("0x{}", hex::encode(&event.topic())))
            .collect::<Vec<_>>()],
    })
}

/// Decodes one `eth_getLogs` answer into the rows that concern this account,
/// newest first.
///
/// # Errors
/// Refuses an over-long answer or any log that does not decode.
pub fn decode_logs(
    result: &Value,
    address: Option<[u8; 20]>,
    accounts: &[[u8; 32]],
) -> Result<Vec<PaxeerActivityRecord>, GatewayError> {
    let logs = result.as_array().ok_or(GatewayError::MalformedAnswer)?;
    if logs.len() > MAXIMUM_LOGS_PER_CHUNK {
        return Err(GatewayError::MalformedAnswer);
    }
    let mut records = Vec::new();
    for log in logs {
        let record = decode_log(log)?;
        if record.concerns(address, accounts) {
            records.push(record);
        }
    }
    records.sort_by(|left, right| {
        right
            .block_number
            .cmp(&left.block_number)
            .then_with(|| right.log_index.cmp(&left.log_index))
    });
    Ok(records)
}

/// Everything one gateway join reports for an account before the index's own
/// receipt-verified rows are attached.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct UnifiedAccountJoin {
    pub requested: AccountIdentifier,
    pub canonical: AccountIdentifier,
    pub identities: ResolvedIdentities,
    pub balances: JoinedBalances,
    pub settlement: SettlementLadder,
    pub paxeer_activity: PaxeerActivityPage,
}

/// The one unified account view: both identities, the joined balance table,
/// both sides' recent activity and the settlement ladder.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct UnifiedAccountView {
    pub join: UnifiedAccountJoin,
    pub layerx_activity: Page<AccountActivityRecord>,
}

/// Reads one account's unified view through the network gateway.
pub struct UnifiedAccountReader<'a> {
    endpoint: &'a GatewayEndpoint,
    window: ActivityWindow,
}

impl<'a> UnifiedAccountReader<'a> {
    /// # Errors
    /// Refuses a window outside its declared bounds.
    pub fn new(
        endpoint: &'a GatewayEndpoint,
        window: ActivityWindow,
    ) -> Result<Self, GatewayError> {
        Ok(Self {
            endpoint,
            window: window.validate()?,
        })
    }

    fn call(&self, method: &str, params: &[Value]) -> Result<Value, GatewayError> {
        let id = NEXT_REQUEST.fetch_add(1, Ordering::Relaxed);
        let answer = self.endpoint.post(&rpc_request(id, method, params))?;
        rpc_result(id, &answer)
    }

    /// Resolves any of the three spellings, joins balances and the settlement
    /// ladder, and reads one bounded page of Paxeer-side activity.
    ///
    /// # Errors
    /// Reports the first transport, refusal or decoding failure.
    pub fn join(
        &self,
        requested: AccountIdentifier,
        before_block: Option<u64>,
    ) -> Result<UnifiedAccountJoin, GatewayError> {
        let key = Value::String(requested.canonical_text());
        let identities = decode_identities(&self.call("px_resolveAccount", &[key.clone()])?)?;
        let canonical = identities.canonical(requested);
        let balances = decode_balances(&self.call("px_getBalances", &[key])?)?;
        let settlement = decode_settlement(&self.call("px_getNetwork", &[])?)?;
        let paxeer_activity =
            self.paxeer_activity(&identities, settlement.instant_block, before_block)?;
        Ok(UnifiedAccountJoin {
            requested,
            canonical,
            identities,
            balances,
            settlement,
            paxeer_activity,
        })
    }

    /// Reads one bounded newest-first page of custody and binding activity for
    /// the account's EVM address.
    ///
    /// # Errors
    /// Reports the first transport, refusal or decoding failure.
    pub fn paxeer_activity(
        &self,
        identities: &ResolvedIdentities,
        head: u64,
        before_block: Option<u64>,
    ) -> Result<PaxeerActivityPage, GatewayError> {
        let accounts = [identities.layerx_account, identities.layerx_did]
            .into_iter()
            .flatten()
            .collect::<Vec<_>>();
        let chunks = self.window.chunks(head, before_block);
        let Some((_, newest)) = chunks.first().copied() else {
            return Ok(PaxeerActivityPage {
                items: Vec::new(),
                from_block: head,
                to_block: head,
                next_before_block: None,
                evidence: Evidence::GatewayReported,
            });
        };
        let mut items = Vec::new();
        let mut lowest = newest;
        for (from, to) in chunks {
            lowest = from;
            let result = self.call("eth_getLogs", &[logs_filter(from, to)])?;
            items.extend(decode_logs(&result, identities.evm_address, &accounts)?);
            if items.len() >= self.window.limit {
                break;
            }
        }
        items.sort_by(|left, right| {
            right
                .block_number
                .cmp(&left.block_number)
                .then_with(|| right.log_index.cmp(&left.log_index))
        });
        items.truncate(self.window.limit);
        Ok(PaxeerActivityPage {
            items,
            from_block: lowest,
            to_block: newest,
            next_before_block: self.window.next_before(head, lowest),
            evidence: Evidence::GatewayReported,
        })
    }
}

fn amount_text(value: Option<u128>) -> Value {
    value.map_or(Value::Null, |amount| Value::String(amount.to_string()))
}

fn digest_text(value: Option<[u8; 32]>) -> Value {
    value.map_or(Value::Null, |bytes| Value::String(hex::encode(&bytes)))
}

fn address_text(value: Option<[u8; 20]>) -> Value {
    value.map_or(Value::Null, |bytes| {
        Value::String(format!("0x{}", hex::encode(&bytes)))
    })
}

/// Renders the exact unified-account document the web explorer decodes.
#[must_use]
pub fn unified_account_json(join: &UnifiedAccountJoin, freshness: Freshness) -> String {
    serde_json::json!({
        "requested": join.requested.canonical_text(),
        "canonical": join.canonical.canonical_text(),
        "evidence": Evidence::GatewayReported.label(),
        "identities": {
            "evm_address": address_text(join.identities.evm_address),
            "pax_address": join.identities.pax_address.clone().map_or(Value::Null, Value::String),
            "layerx_did": join.identities.layerx_did.map_or(Value::Null, |key| {
                Value::String(format!("{DID_PREFIX}{}", hex::encode(&key)))
            }),
            "layerx_account": digest_text(join.identities.layerx_account),
            "bound": join.identities.bound,
        },
        "balances": {
            "joined_limit": join.balances.joined_limit.to_string(),
            "items": join
                .balances
                .items
                .iter()
                .map(|balance| serde_json::json!({
                    "asset_id": hex::encode(&balance.asset_id),
                    "denom": balance.denom,
                    "custody": amount_text(balance.custody),
                    "paxeer": amount_text(balance.paxeer),
                    "layerx": amount_text(balance.layerx),
                }))
                .collect::<Vec<_>>(),
        },
        "settlement": {
            "network_id": join.settlement.network_id,
            "chain_id": join.settlement.chain_id.to_string(),
            "instant_block": join.settlement.instant_block.to_string(),
            "sealed_batch": freshness.observed_sealed_batch.to_string(),
            "finalized_batch": join.settlement.finalized_batch.to_string(),
            "anchor_status": join.settlement.anchor_status.to_string(),
            "anchor_status_name": join.settlement.anchor_status_name,
        },
        "paxeer_activity": {
            "from_block": join.paxeer_activity.from_block.to_string(),
            "to_block": join.paxeer_activity.to_block.to_string(),
            "next_before_block": join
                .paxeer_activity
                .next_before_block
                .map_or(Value::Null, |block| Value::String(block.to_string())),
            "items": join
                .paxeer_activity
                .items
                .iter()
                .map(|record| serde_json::json!({
                    "event": record.event.label(),
                    "block_number": record.block_number.to_string(),
                    "log_index": record.log_index.to_string(),
                    "transaction_hash": format!("0x{}", hex::encode(&record.transaction_hash)),
                    "asset_id": digest_text(record.asset_id),
                    "amount": amount_text(record.amount),
                    "address": address_text(record.address),
                    "account": digest_text(record.account),
                }))
                .collect::<Vec<_>>(),
        },
    })
    .to_string()
}

#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used)]

    use super::{
        decode_balances, decode_identities, decode_log, decode_logs, decode_settlement, http_body,
        logs_filter, quantity, rpc_request, rpc_result, ActivityWindow, AccountIdentifier,
        Evidence, GatewayEndpoint, GatewayError, JoinedBalance, PaxeerEvent, ResolvedIdentities,
    };
    use serde_json::Value;

    const ADDRESS: &str = "0x00112233445566778899aabbccddeeff00112233";
    const ACCOUNT: &str = "1111111111111111111111111111111111111111111111111111111111111111";
    const DID: &str = "2222222222222222222222222222222222222222222222222222222222222222";

    fn identities(bound: bool) -> ResolvedIdentities {
        decode_identities(&serde_json::json!({
            "evm_address": ADDRESS,
            "pax_address": "pax1qqqqq",
            "layerx_did": format!("did:layerx:{DID}"),
            "layerx_account": ACCOUNT,
            "bound": bound,
        }))
        .expect("declared resolve document decodes")
    }

    #[test]
    fn every_public_spelling_normalises_to_one_identifier() {
        assert_eq!(
            AccountIdentifier::parse("  0x00112233445566778899AABBCCDDEEFF00112233 "),
            Ok(AccountIdentifier::Evm([
                0x00, 0x11, 0x22, 0x33, 0x44, 0x55, 0x66, 0x77, 0x88, 0x99, 0xaa, 0xbb, 0xcc, 0xdd,
                0xee, 0xff, 0x00, 0x11, 0x22, 0x33,
            ]))
        );
        assert_eq!(
            AccountIdentifier::parse(&format!("DID:LAYERX:{}", DID.to_uppercase())),
            Ok(AccountIdentifier::Did([0x22; 32]))
        );
        assert_eq!(
            AccountIdentifier::parse(ACCOUNT),
            Ok(AccountIdentifier::Account([0x11; 32]))
        );
        assert_eq!(
            AccountIdentifier::parse(ADDRESS).map(AccountIdentifier::canonical_text),
            Ok(ADDRESS.to_owned())
        );
        assert_eq!(
            AccountIdentifier::parse(&format!("did:layerx:{DID}"))
                .map(AccountIdentifier::canonical_text),
            Ok(format!("did:layerx:{DID}"))
        );
        for refused in [
            "",
            "0x",
            "0x001122",
            "did:layerx:",
            "did:web:example",
            "11111111111111111111111111111111111111111111111111111111111111",
            "0xzz112233445566778899aabbccddeeff00112233",
        ] {
            assert!(AccountIdentifier::parse(refused).is_err(), "{refused}");
        }
    }

    #[test]
    fn a_bound_account_has_exactly_one_canonical_page() {
        let bound = identities(true);
        let requested = AccountIdentifier::parse(ADDRESS).expect("address parses");
        assert_eq!(
            bound.canonical(requested),
            AccountIdentifier::Account([0x11; 32])
        );
        assert_eq!(
            bound.canonical(AccountIdentifier::Did([0x22; 32])),
            AccountIdentifier::Account([0x11; 32])
        );
        let unbound = decode_identities(&serde_json::json!({
            "evm_address": ADDRESS,
            "pax_address": Value::Null,
            "layerx_did": Value::Null,
            "layerx_account": Value::Null,
            "bound": false,
        }))
        .expect("unbound resolve document decodes");
        assert_eq!(unbound.canonical(requested), requested);
        assert_eq!(unbound.layerx_account, None);
        assert!(!unbound.bound);
    }

    #[test]
    fn resolve_documents_outside_the_declared_shape_are_refused() {
        assert_eq!(
            decode_identities(&serde_json::json!({ "evm_address": ADDRESS })),
            Err(GatewayError::MalformedAnswer)
        );
        assert_eq!(
            decode_identities(&serde_json::json!({
                "evm_address": "0x001122",
                "bound": false,
            })),
            Err(GatewayError::MalformedAnswer)
        );
        assert_eq!(
            decode_identities(&Value::String(ADDRESS.to_owned())),
            Err(GatewayError::MalformedAnswer)
        );
    }

    #[test]
    fn requests_are_positional_and_answers_bind_to_their_request() {
        let request: Value = serde_json::from_str(&rpc_request(
            7,
            "px_resolveAccount",
            &[Value::String(ADDRESS.to_owned())],
        ))
        .expect("request is JSON");
        assert_eq!(request["jsonrpc"], "2.0");
        assert_eq!(request["id"], 7);
        assert_eq!(request["method"], "px_resolveAccount");
        assert_eq!(request["params"], serde_json::json!([ADDRESS]));

        assert_eq!(
            rpc_result(7, br#"{"jsonrpc":"2.0","id":7,"result":{"bound":true}}"#),
            Ok(serde_json::json!({ "bound": true }))
        );
        assert_eq!(
            rpc_result(7, br#"{"jsonrpc":"2.0","id":8,"result":{}}"#),
            Err(GatewayError::Unbound)
        );
        assert_eq!(
            rpc_result(
                7,
                br#"{"jsonrpc":"2.0","id":7,"error":{"code":-32601,"message":"unknown"}}"#
            ),
            Err(GatewayError::Refused {
                code: -32_601,
                message: "unknown".to_owned(),
            })
        );
        assert_eq!(
            rpc_result(7, br#"{"id":7,"result":{}}"#),
            Err(GatewayError::MalformedAnswer)
        );
        assert_eq!(
            rpc_result(7, br#"{"jsonrpc":"2.0","id":7}"#),
            Err(GatewayError::MalformedAnswer)
        );
    }

    #[test]
    fn balances_join_both_domains_through_the_custody_asset_map() {
        let balances = decode_balances(&serde_json::json!({
            "account": ACCOUNT,
            "balances": [{
                "asset_id": ACCOUNT,
                "denom": "upaxd",
                "custody": "1000",
                "paxeer": "0x10",
                "layerx": Value::Null,
            }],
            "joined_limit": "64",
        }))
        .expect("declared balance document decodes");
        assert_eq!(balances.joined_limit, 64);
        assert_eq!(balances.evidence, Evidence::GatewayReported);
        assert_eq!(
            balances.items,
            vec![JoinedBalance {
                asset_id: [0x11; 32],
                denom: "upaxd".to_owned(),
                custody: Some(1_000),
                paxeer: Some(16),
                layerx: None,
            }]
        );
        assert_eq!(
            decode_balances(&serde_json::json!({ "balances": [{ "asset_id": ACCOUNT }] })),
            Err(GatewayError::MalformedAnswer)
        );
        assert_eq!(quantity(&Value::String("0x".to_owned())), Err(GatewayError::MalformedAnswer));
        assert_eq!(quantity(&Value::String("-1".to_owned())), Err(GatewayError::MalformedAnswer));
        assert_eq!(quantity(&Value::Bool(true)), Err(GatewayError::MalformedAnswer));
    }

    #[test]
    fn the_settlement_ladder_carries_the_anchor_position() {
        let ladder = decode_settlement(&serde_json::json!({
            "network_id": "paxeer-x",
            "paxeer": { "chain_id": 8888, "latest_block": "0x2a" },
            "layerx": { "node_info": {} },
            "anchor": {
                "latest_finalized_batch": "19",
                "status": 2,
                "status_name": "finalised",
            },
        }))
        .expect("declared network document decodes");
        assert_eq!(ladder.chain_id, 8_888);
        assert_eq!(ladder.instant_block, 42);
        assert_eq!(ladder.finalized_batch, 19);
        assert_eq!(ladder.anchor_status, 2);
        assert_eq!(ladder.anchor_status_name, "finalised");
        assert_eq!(
            decode_settlement(&serde_json::json!({ "network_id": "paxeer-x" })),
            Err(GatewayError::MalformedAnswer)
        );
    }

    #[test]
    fn every_admitted_event_has_its_own_abi_topic() {
        let mut topics = PaxeerEvent::ALL.map(PaxeerEvent::topic).to_vec();
        topics.sort_unstable();
        topics.dedup();
        assert_eq!(topics.len(), PaxeerEvent::ALL.len());
        for event in PaxeerEvent::ALL {
            assert_eq!(PaxeerEvent::from_topic(event.topic()), Some(event));
        }
        assert_eq!(PaxeerEvent::from_topic([0x00; 32]), None);
        let filter = logs_filter(16, 32);
        assert_eq!(filter["fromBlock"], "0x10");
        assert_eq!(filter["toBlock"], "0x20");
        assert_eq!(
            filter["address"][0],
            "0x0000000000000000000000000000000000001013"
        );
        assert_eq!(
            filter["address"][1],
            "0x0000000000000000000000000000000000001004"
        );
        assert_eq!(
            filter["topics"][0]
                .as_array()
                .map(|topics| topics.len()),
            Some(PaxeerEvent::ALL.len())
        );
    }

    fn deposit_log() -> Value {
        serde_json::json!({
            "address": "0x0000000000000000000000000000000000001013",
            "topics": [
                format!("0x{}", hex_topic(PaxeerEvent::CustodyDeposit)),
                format!("0x{}", "33".repeat(32)),
                format!("0x{ACCOUNT}"),
                format!("0x{}{}", "0".repeat(24), &ADDRESS[2..]),
            ],
            "data": format!(
                "0x{}{}{}",
                "22".repeat(32),
                "0".repeat(62) + "ff",
                "0".repeat(63) + "1",
            ),
            "blockNumber": "0x64",
            "logIndex": "0x1",
            "transactionHash": format!("0x{}", "44".repeat(32)),
        })
    }

    fn hex_topic(event: PaxeerEvent) -> String {
        event
            .topic()
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect()
    }

    #[test]
    fn custody_deposits_decode_against_the_declared_abi() {
        let record = decode_log(&deposit_log()).expect("declared deposit log decodes");
        assert_eq!(record.event, PaxeerEvent::CustodyDeposit);
        assert_eq!(record.block_number, 100);
        assert_eq!(record.log_index, 1);
        assert_eq!(record.asset_id, Some([0x11; 32]));
        assert_eq!(record.amount, Some(255));
        assert_eq!(record.account, Some([0x22; 32]));
        assert_eq!(record.transaction_hash, [0x44; 32]);
        assert_eq!(record.evidence, Evidence::GatewayReported);
        assert_eq!(
            record.address.map(|bytes| format!("0x{}", hex_bytes(&bytes))),
            Some(ADDRESS.to_owned())
        );
        assert!(record.concerns(record.address, &[]));
        assert!(record.concerns(None, &[[0x22; 32]]));
        assert!(!record.concerns(None, &[[0x99; 32]]));
        assert!(!record.concerns(Some([0x01; 20]), &[]));
    }

    fn hex_bytes(bytes: &[u8]) -> String {
        bytes.iter().map(|byte| format!("{byte:02x}")).collect()
    }

    #[test]
    fn a_log_from_another_emitter_or_topic_is_refused() {
        let mut foreign = deposit_log();
        foreign["address"] = Value::String("0x0000000000000000000000000000000000001004".to_owned());
        assert_eq!(decode_log(&foreign), Err(GatewayError::MalformedAnswer));

        let mut unknown = deposit_log();
        unknown["topics"][0] = Value::String(format!("0x{}", "00".repeat(32)));
        assert_eq!(decode_log(&unknown), Err(GatewayError::MalformedAnswer));

        let mut ragged = deposit_log();
        ragged["data"] = Value::String("0x1234".to_owned());
        assert_eq!(decode_log(&ragged), Err(GatewayError::MalformedAnswer));
    }

    #[test]
    fn a_page_of_logs_keeps_only_this_account_newest_first() {
        let mut older = deposit_log();
        older["blockNumber"] = Value::String("0x1".to_owned());
        let mut foreign = deposit_log();
        foreign["topics"][3] = Value::String(format!(
            "0x{}{}",
            "0".repeat(24),
            "99998888777766665555444433332222abcdef00"
        ));
        foreign["data"] = Value::String(format!(
            "0x{}{}{}",
            "55".repeat(32),
            "0".repeat(62) + "ff",
            "0".repeat(63) + "1",
        ));
        let records = decode_logs(
            &serde_json::json!([older, deposit_log(), foreign]),
            Some([
                0x00, 0x11, 0x22, 0x33, 0x44, 0x55, 0x66, 0x77, 0x88, 0x99, 0xaa, 0xbb, 0xcc, 0xdd,
                0xee, 0xff, 0x00, 0x11, 0x22, 0x33,
            ]),
            &[],
        )
        .expect("declared log page decodes");
        assert_eq!(records.len(), 2);
        assert_eq!(records[0].block_number, 100);
        assert_eq!(records[1].block_number, 1);
        assert_eq!(
            decode_logs(&Value::String("logs".to_owned()), None, &[]),
            Err(GatewayError::MalformedAnswer)
        );
    }

    #[test]
    fn the_activity_window_stays_bounded_and_paginates_backwards() {
        let window = ActivityWindow {
            span_blocks: 1_000,
            chunk_blocks: 400,
            max_chunks: 2,
            limit: 25,
        };
        assert_eq!(window.validate(), Ok(window));
        assert_eq!(window.chunks(10_000, None), vec![(9_601, 10_000), (9_201, 9_600)]);
        assert_eq!(window.next_before(10_000, 9_201), Some(9_201));
        assert_eq!(window.chunks(10_000, Some(9_201)), vec![(9_001, 9_200)]);
        assert_eq!(window.next_before(10_000, 9_001), None);
        assert_eq!(window.chunks(300, None), vec![(0, 300)]);
        assert_eq!(window.next_before(300, 0), None);
        assert_eq!(window.chunks(10_000, Some(0)), Vec::new());
        for refused in [
            ActivityWindow { span_blocks: 0, ..window },
            ActivityWindow { chunk_blocks: 0, ..window },
            ActivityWindow { chunk_blocks: 2_000, ..window },
            ActivityWindow { max_chunks: 0, ..window },
            ActivityWindow { limit: 0, ..window },
            ActivityWindow { limit: 101, ..window },
        ] {
            assert_eq!(refused.validate(), Err(GatewayError::InvalidWindow));
        }
    }

    #[test]
    fn endpoints_and_http_answers_fail_closed() {
        assert_eq!(
            GatewayEndpoint::parse("https://gateway.example:8545/rpc"),
            Ok(GatewayEndpoint {
                secure: true,
                host: "gateway.example".to_owned(),
                port: 8_545,
                path: "/rpc".to_owned(),
            })
        );
        assert_eq!(
            GatewayEndpoint::parse("http://127.0.0.1:26657"),
            Ok(GatewayEndpoint {
                secure: false,
                host: "127.0.0.1".to_owned(),
                port: 26_657,
                path: "/".to_owned(),
            })
        );
        for refused in [
            "gateway.example",
            "ftp://gateway.example",
            "https://",
            "https://gateway.example:0",
            "https://gate way.example:1",
        ] {
            assert_eq!(
                GatewayEndpoint::parse(refused),
                Err(GatewayError::InvalidEndpoint),
                "{refused}"
            );
        }
        assert_eq!(
            http_body(b"HTTP/1.1 200 OK\r\nContent-Length: 2\r\n\r\n{}"),
            Ok(b"{}".to_vec())
        );
        assert!(matches!(
            http_body(b"HTTP/1.1 503 Refused\r\n\r\n{}"),
            Err(GatewayError::Transport(_))
        ));
        assert!(matches!(
            http_body(b"not an answer"),
            Err(GatewayError::Transport(_))
        ));
    }

    #[test]
    fn the_unified_document_carries_both_halves_and_its_provenance() {
        use super::{
            unified_account_json, PaxeerActivityPage, SettlementLadder, UnifiedAccountJoin,
        };
        let identities = identities(true);
        let requested = AccountIdentifier::parse(ADDRESS).expect("address parses");
        let join = UnifiedAccountJoin {
            requested,
            canonical: identities.canonical(requested),
            identities,
            balances: decode_balances(&serde_json::json!({
                "balances": [{
                    "asset_id": ACCOUNT,
                    "denom": "upaxd",
                    "custody": "7",
                    "paxeer": "5",
                    "layerx": "2",
                }],
                "joined_limit": 64,
            }))
            .expect("balances decode"),
            settlement: SettlementLadder {
                network_id: "paxeer-x".to_owned(),
                chain_id: 8_888,
                instant_block: 100,
                finalized_batch: 19,
                anchor_status: 2,
                anchor_status_name: "finalised".to_owned(),
                evidence: Evidence::GatewayReported,
            },
            paxeer_activity: PaxeerActivityPage {
                items: vec![decode_log(&deposit_log()).expect("deposit decodes")],
                from_block: 1,
                to_block: 100,
                next_before_block: Some(1),
                evidence: Evidence::GatewayReported,
            },
        };
        let document: Value = serde_json::from_str(&unified_account_json(
            &join,
            crate::Freshness {
                observed_chain_sequence: 19,
                observed_sealed_batch: 7,
                observed_finalised_checkpoint: [0xee; 32],
                indexed_batch: Some(7),
                indexed_checkpoint: Some([0xee; 32]),
            },
        ))
        .expect("document is JSON");
        assert_eq!(document["requested"], ADDRESS);
        assert_eq!(document["canonical"], ACCOUNT);
        assert_eq!(document["evidence"], "gateway-reported");
        assert_eq!(document["identities"]["bound"], true);
        assert_eq!(document["identities"]["layerx_did"], format!("did:layerx:{DID}"));
        assert_eq!(document["balances"]["items"][0]["denom"], "upaxd");
        assert_eq!(document["balances"]["items"][0]["layerx"], "2");
        assert_eq!(document["settlement"]["sealed_batch"], "7");
        assert_eq!(document["settlement"]["finalized_batch"], "19");
        assert_eq!(document["settlement"]["instant_block"], "100");
        assert_eq!(document["paxeer_activity"]["items"][0]["event"], "custody-deposit");
        assert_eq!(document["paxeer_activity"]["items"][0]["amount"], "255");
        assert_eq!(document["paxeer_activity"]["next_before_block"], "1");
    }
}
