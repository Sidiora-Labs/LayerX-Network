use std::io;
use std::path::{Path, PathBuf};

use serde_json::{json, Value};
use sha3::{Digest as _, Keccak256};

use crate::payment::{GatewayRpc, RpcAnswer};

/// The xweb precompile every request is made through and every fulfil is
/// posted to.
pub const XWEB_PRECOMPILE: [u8; 20] = [
    0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0x10, 0x19,
];

/// The event the precompile emits for every request.
pub const REQUESTED_EVENT: &str = "XWebRequested(uint64,address,uint8,bytes,uint64,uint256,uint64)";

/// The most blocks one `eth_getLogs` query spans.
pub const MAX_BLOCK_RANGE: u64 = 1_000;

/// The largest request payload the watcher accepts from a log.
pub const MAX_PAYLOAD_BYTES: usize = 65_536;

const CURSOR_FILE: &str = "cursor";

/// keccak256 of `bytes`.
#[must_use]
pub fn keccak(bytes: &[u8]) -> [u8; 32] {
    Keccak256::digest(bytes).into()
}

/// The first topic of every `XWebRequested` log.
#[must_use]
pub fn requested_topic() -> [u8; 32] {
    keccak(REQUESTED_EVENT.as_bytes())
}

/// `0x` followed by lower-case hexadecimal.
#[must_use]
pub fn hex0x(bytes: &[u8]) -> String {
    format!("0x{}", crate::payment::hex(bytes))
}

/// Decodes `0x`-prefixed hexadecimal of either case.
#[must_use]
pub fn unhex0x(text: &str) -> Option<Vec<u8>> {
    let digits = text.strip_prefix("0x")?;
    crate::payment::unhex(&digits.to_ascii_lowercase())
}

/// A JSON-RPC quantity: `0x` and hexadecimal digits with no leading zero.
#[must_use]
pub fn quantity(value: u128) -> String {
    format!("0x{value:x}")
}

/// Decodes a JSON-RPC quantity, refusing leading zeros and overflow.
#[must_use]
pub fn parse_quantity(text: &str) -> Option<u128> {
    let digits = text.strip_prefix("0x")?;
    if digits.is_empty()
        || digits.len() > 32
        || (digits.len() > 1 && digits.starts_with('0'))
        || !digits.bytes().all(|byte| byte.is_ascii_hexdigit())
    {
        return None;
    }
    u128::from_str_radix(digits, 16).ok()
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum EvmError {
    /// The endpoint is not an http or https URL with a host.
    Endpoint,
    /// No well-formed answer arrived.
    Unavailable,
    /// The node answered with a JSON-RPC error.
    Rejected { code: i64 },
    /// The answer does not have the shape the method defines.
    Malformed,
    /// A log is not an `XWebRequested` log of the precompile.
    ForeignLog,
    /// The watcher's cursor could not be read or written.
    Cursor,
}

impl std::fmt::Display for EvmError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Endpoint => f.write_str("evm endpoint refused"),
            Self::Unavailable => f.write_str("evm endpoint unavailable"),
            Self::Rejected { code } => write!(f, "evm endpoint rejected the call with {code}"),
            Self::Malformed => f.write_str("evm answer malformed"),
            Self::ForeignLog => f.write_str("log is not an xweb request"),
            Self::Cursor => f.write_str("watch cursor unreadable or unwritable"),
        }
    }
}

impl std::error::Error for EvmError {}

/// A JSON-RPC client for the configured EVM endpoint, over the same
/// std-library transport the gateway client uses.
#[derive(Clone, Debug)]
pub struct EvmRpc {
    rpc: GatewayRpc,
}

impl EvmRpc {
    /// # Errors
    /// Refuses an endpoint that is not an http or https URL with a host.
    pub fn new(endpoint: &str) -> Result<Self, EvmError> {
        GatewayRpc::new(endpoint)
            .map(|rpc| Self { rpc })
            .map_err(|_| EvmError::Endpoint)
    }

    /// Calls one method and returns its result.
    ///
    /// # Errors
    /// Returns an unavailable endpoint and a JSON-RPC error by its code.
    pub fn call(&self, method: &str, params: &Value) -> Result<Value, EvmError> {
        match self.rpc.call(method, params) {
            Some(RpcAnswer::Result(value)) => Ok(value),
            Some(RpcAnswer::Error { code, .. }) => Err(EvmError::Rejected { code }),
            None => Err(EvmError::Unavailable),
        }
    }

    /// Calls a method whose result is one quantity.
    ///
    /// # Errors
    /// Returns the call's error and a result that is not a quantity.
    pub fn quantity(&self, method: &str, params: &Value) -> Result<u128, EvmError> {
        self.call(method, params)?
            .as_str()
            .and_then(parse_quantity)
            .ok_or(EvmError::Malformed)
    }

    /// The latest block number.
    ///
    /// # Errors
    /// Returns the call's error and a number beyond 64 bits.
    pub fn block_number(&self) -> Result<u64, EvmError> {
        u64::try_from(self.quantity("eth_blockNumber", &json!([]))?)
            .map_err(|_| EvmError::Malformed)
    }

    /// `eth_call` of `data` on `to` at the latest block.
    ///
    /// # Errors
    /// Returns the call's error and a result that is not hexadecimal bytes.
    pub fn eth_call(&self, to: [u8; 20], data: &[u8]) -> Result<Vec<u8>, EvmError> {
        let result = self.call(
            "eth_call",
            &json!([{ "to": hex0x(&to), "data": hex0x(data) }, "latest"]),
        )?;
        result.as_str().and_then(unhex0x).ok_or(EvmError::Malformed)
    }
}

/// One `XWebRequested` log, decoded.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct WebRequest {
    pub request_id: u64,
    pub requester: [u8; 20],
    pub kind: u8,
    pub payload: Vec<u8>,
    pub callback_gas: u64,
    pub paid: [u8; 32],
    pub timeout_height: u64,
    pub block_number: u64,
}

fn word_u64(word: &[u8]) -> Option<u64> {
    let word: &[u8; 32] = word.try_into().ok()?;
    if word[..24].iter().any(|byte| *byte != 0) {
        return None;
    }
    let mut low = [0; 8];
    low.copy_from_slice(&word[24..]);
    Some(u64::from_be_bytes(low))
}

fn topic(value: &Value) -> Option<[u8; 32]> {
    unhex0x(value.as_str()?)?.try_into().ok()
}

/// Decodes one `XWebRequested` log of the precompile. The data is the ABI
/// encoding of `(uint8 kind, bytes payload, uint64 callbackGas, uint256
/// paid, uint64 timeoutHeight)` with nothing before, between or after.
///
/// # Errors
/// Refuses a log of another address or event, a removed log and any field
/// that does not decode exactly.
pub fn decode_requested(log: &Value) -> Result<WebRequest, EvmError> {
    let malformed = || EvmError::Malformed;
    let address = log
        .get("address")
        .and_then(Value::as_str)
        .and_then(unhex0x)
        .ok_or_else(malformed)?;
    let topics = log
        .get("topics")
        .and_then(Value::as_array)
        .ok_or_else(malformed)?;
    if address != XWEB_PRECOMPILE
        || topics.first().and_then(topic) != Some(requested_topic())
        || log.get("removed").and_then(Value::as_bool) == Some(true)
    {
        return Err(EvmError::ForeignLog);
    }
    let [_, id_topic, requester_topic] = topics.as_slice() else {
        return Err(malformed());
    };
    let request_id = topic(id_topic)
        .and_then(|word| word_u64(&word))
        .ok_or_else(malformed)?;
    let requester_word = topic(requester_topic).ok_or_else(malformed)?;
    if requester_word[..12].iter().any(|byte| *byte != 0) {
        return Err(malformed());
    }
    let mut requester = [0; 20];
    requester.copy_from_slice(&requester_word[12..]);
    let block_number = log
        .get("blockNumber")
        .and_then(Value::as_str)
        .and_then(parse_quantity)
        .and_then(|number| u64::try_from(number).ok())
        .ok_or_else(malformed)?;
    let data = log
        .get("data")
        .and_then(Value::as_str)
        .and_then(unhex0x)
        .ok_or_else(malformed)?;
    if data.len() < 6 * 32 || !data.len().is_multiple_of(32) {
        return Err(malformed());
    }
    let word = |index: usize| &data[index * 32..(index + 1) * 32];
    let kind = word_u64(word(0))
        .and_then(|kind| u8::try_from(kind).ok())
        .ok_or_else(malformed)?;
    if word_u64(word(1)) != Some(5 * 32) {
        return Err(malformed());
    }
    let callback_gas = word_u64(word(2)).ok_or_else(malformed)?;
    let paid: [u8; 32] = word(3).try_into().map_err(|_| malformed())?;
    let timeout_height = word_u64(word(4)).ok_or_else(malformed)?;
    let length = word_u64(word(5))
        .and_then(|length| usize::try_from(length).ok())
        .filter(|length| *length <= MAX_PAYLOAD_BYTES)
        .ok_or_else(malformed)?;
    let padded = length.div_ceil(32) * 32;
    if data.len() != 6 * 32 + padded {
        return Err(malformed());
    }
    let tail = &data[6 * 32..];
    if tail[length..].iter().any(|byte| *byte != 0) {
        return Err(malformed());
    }
    Ok(WebRequest {
        request_id,
        requester,
        kind,
        payload: tail[..length].to_vec(),
        callback_gas,
        paid,
        timeout_height,
        block_number,
    })
}

/// Follows `XWebRequested` logs of the precompile to the configured
/// confirmation depth. The next block to read is kept in a cursor file under
/// the state directory, so a restart resumes where the last poll stopped.
pub struct RequestWatcher {
    rpc: EvmRpc,
    confirmations: u64,
    cursor_path: PathBuf,
    next_block: Option<u64>,
    last_head: u64,
}

impl RequestWatcher {
    /// Opens the watcher. A cursor file already under `state_dir` wins over
    /// `start`; with neither, the first poll starts at the confirmed head.
    ///
    /// # Errors
    /// Returns the error creating the directory and a cursor file that does
    /// not hold one block number.
    pub fn open(
        rpc: EvmRpc,
        confirmations: u32,
        state_dir: &Path,
        start: Option<u64>,
    ) -> io::Result<Self> {
        std::fs::create_dir_all(state_dir)?;
        let cursor_path = state_dir.join(CURSOR_FILE);
        let stored = match std::fs::read_to_string(&cursor_path) {
            Ok(text) => Some(text.trim().parse::<u64>().map_err(|_| {
                io::Error::new(io::ErrorKind::InvalidData, "watch cursor malformed")
            })?),
            Err(error) if error.kind() == io::ErrorKind::NotFound => None,
            Err(error) => return Err(error),
        };
        Ok(Self {
            rpc,
            confirmations: u64::from(confirmations),
            cursor_path,
            next_block: stored.or(start),
            last_head: 0,
        })
    }

    /// The next block the watcher reads, once it knows it.
    #[must_use]
    pub const fn next_block(&self) -> Option<u64> {
        self.next_block
    }

    /// The chain head the last poll saw.
    #[must_use]
    pub const fn last_head(&self) -> u64 {
        self.last_head
    }

    fn store_cursor(&self, next: u64) -> Result<(), EvmError> {
        let temporary = self.cursor_path.with_extension("tmp");
        std::fs::write(&temporary, next.to_string())
            .and_then(|()| std::fs::File::open(&temporary)?.sync_all())
            .and_then(|()| std::fs::rename(&temporary, &self.cursor_path))
            .map_err(|_| EvmError::Cursor)
    }

    /// Reads the confirmed blocks after the cursor, at most
    /// [`MAX_BLOCK_RANGE`] of them, and returns their requests in log order.
    /// The cursor moves only after every log in the range decoded.
    ///
    /// # Errors
    /// Returns the endpoint's error, a malformed or foreign log and a cursor
    /// that could not be written.
    pub fn poll(&mut self) -> Result<Vec<WebRequest>, EvmError> {
        let head = self.rpc.block_number()?;
        self.last_head = head;
        let Some(safe) = head.checked_sub(self.confirmations) else {
            return Ok(Vec::new());
        };
        let from = *self.next_block.get_or_insert(safe);
        if from > safe {
            return Ok(Vec::new());
        }
        let to = safe.min(from.saturating_add(MAX_BLOCK_RANGE - 1));
        let logs = self.rpc.call(
            "eth_getLogs",
            &json!([{
                "address": hex0x(&XWEB_PRECOMPILE),
                "fromBlock": quantity(u128::from(from)),
                "toBlock": quantity(u128::from(to)),
                "topics": [hex0x(&requested_topic())],
            }]),
        )?;
        let requests = logs
            .as_array()
            .ok_or(EvmError::Malformed)?
            .iter()
            .map(decode_requested)
            .collect::<Result<Vec<_>, _>>()?;
        if requests
            .iter()
            .any(|request| request.block_number < from || request.block_number > to)
        {
            return Err(EvmError::Malformed);
        }
        let next = to + 1;
        self.store_cursor(next)?;
        self.next_block = Some(next);
        Ok(requests)
    }
}
