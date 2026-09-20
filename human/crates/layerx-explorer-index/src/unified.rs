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

use layerx_network_gateway as gateway;
use layerx_programs::hex;
use serde_json::Value;
use sha3::{Digest as _, Keccak256};

use crate::{AccountActivityRecord, Freshness, Page};

pub use layerx_network_gateway::{
    rpc_request, AccountIdentifier, Evidence, GatewayEndpoint, IdentifierError, ResolvedIdentities,
};

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
const MAXIMUM_ACTIVITY_LIMIT: usize = 100;
const MAXIMUM_LOGS_PER_CHUNK: usize = 10_000;

/// One asset held by the same account in either domain, joined through the
/// custody asset map.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct JoinedBalance {
    pub asset_id: [u8; 32],
    /// The joined denomination, absent when neither domain names one for this
    /// asset. The gateway reports a null denomination for an asset that is not
    /// custodied and carries no denomination of its own.
    pub denom: Option<String>,
    /// The amount the custody precompile reports as custodied for this asset.
    pub custody: Option<u128>,
    /// The Paxeer bank balance under the joined denomination.
    pub paxeer: Option<u128>,
    /// The spendable amount the LayerX account document reports.
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
    /// Final: the newest batch the anchor reports as finalised, absent when the
    /// anchor reports no finalised batch yet. Never a substituted zero.
    pub finalized_batch: Option<u64>,
    /// The anchor's own status code for that batch, absent with the batch.
    pub anchor_status: Option<u64>,
    /// The anchor's own name for that status, absent with the batch.
    pub anchor_status_name: Option<String>,
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
            Self::CustodyDeposit => {
                "CustodyDeposit(bytes32,bytes32,address,bytes32,uint256,uint64)"
            }
            Self::ClaimQueued => {
                "ClaimQueued(bytes32,bytes32,bytes32,bytes32,address,uint256,uint64)"
            }
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
        self.account.is_some_and(|value| accounts.contains(&value))
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

impl From<gateway::GatewayError> for GatewayError {
    fn from(error: gateway::GatewayError) -> Self {
        match error {
            gateway::GatewayError::InvalidEndpoint => Self::InvalidEndpoint,
            gateway::GatewayError::Transport(detail) => Self::Transport(detail),
            gateway::GatewayError::Refused { code, message } => Self::Refused { code, message },
            gateway::GatewayError::Unbound => Self::Unbound,
            gateway::GatewayError::MalformedAnswer => Self::MalformedAnswer,
        }
    }
}

/// Extracts the body of one HTTP/1.1 answer, failing closed on a refusal.
///
/// # Errors
/// Refuses an unframed answer, a non-200 status and a truncated body.
pub fn http_body(answer: &[u8]) -> Result<Vec<u8>, GatewayError> {
    gateway::http_body(answer).map_err(GatewayError::from)
}

/// Interprets one JSON-RPC answer for the request `id`, failing closed.
///
/// # Errors
/// Refuses a malformed envelope, an answer for another request, and returns
/// the gateway's own refusal as [`GatewayError::Refused`].
pub fn rpc_result(id: u64, answer: &[u8]) -> Result<Value, GatewayError> {
    gateway::rpc_result(id, answer).map_err(GatewayError::from)
}

fn address(text: &str) -> Result<[u8; 20], GatewayError> {
    gateway::address(text).map_err(GatewayError::from)
}

fn digest(text: &str) -> Result<[u8; 32], GatewayError> {
    gateway::digest(text).map_err(GatewayError::from)
}

/// Decodes a gateway quantity: an unsigned decimal string, a `0x` quantity or
/// a JSON integer. Nothing else is admitted.
///
/// # Errors
/// Refuses every other spelling and any value beyond 128 bits.
pub fn quantity(value: &Value) -> Result<u128, GatewayError> {
    gateway::quantity(value).map_err(GatewayError::from)
}

fn counted(value: &Value) -> Result<u64, GatewayError> {
    gateway::counted(value).map_err(GatewayError::from)
}

/// Decodes the `px_resolveAccount` result.
///
/// # Errors
/// Refuses a document that is not the declared shape.
pub fn decode_identities(result: &Value) -> Result<ResolvedIdentities, GatewayError> {
    gateway::decode_identities(result).map_err(GatewayError::from)
}

/// Decodes the `px_getBalances` result into the explorer's joined table: the
/// custodied amount of the custody record, the Paxeer bank amount and the
/// LayerX account's own balance, each absent when that half reports none.
///
/// # Errors
/// Refuses a document that is not the declared shape or exceeds the joined
/// balance ceiling.
pub fn decode_balances(result: &Value) -> Result<JoinedBalances, GatewayError> {
    let joined = gateway::decode_account_balances(result)?;
    let mut items = Vec::with_capacity(joined.balances.len());
    for row in &joined.balances {
        items.push(JoinedBalance {
            asset_id: row.asset_id,
            denom: row.denom.clone(),
            custody: row.custody.as_ref().map(|custody| custody.custodied),
            paxeer: row.paxeer.as_ref().map(|paxeer| paxeer.amount),
            layerx: row.layerx_amount()?,
        });
    }
    Ok(JoinedBalances {
        items,
        joined_limit: joined.joined_limit,
        evidence: Evidence::GatewayReported,
    })
}

/// Decodes the `px_getNetwork` result into the settlement ladder. An anchor
/// that reports no finalised batch yet leaves the final rung absent rather
/// than failing the whole read or standing in a zero for it.
///
/// # Errors
/// Refuses a document that is not the declared shape.
pub fn decode_settlement(result: &Value) -> Result<SettlementLadder, GatewayError> {
    let head = gateway::decode_network(result)?;
    Ok(SettlementLadder {
        network_id: head.network_id,
        chain_id: head.chain_id,
        instant_block: head.latest_block,
        finalized_batch: head.anchor.latest_finalized_batch,
        anchor_status: head.anchor.status,
        anchor_status_name: head.anchor.status_name,
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
    let emitter = address(
        log["address"]
            .as_str()
            .ok_or(GatewayError::MalformedAnswer)?,
    )?;
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
        self.endpoint
            .call(method, params)
            .map_err(GatewayError::from)
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

fn count_text(value: Option<u64>) -> Value {
    value.map_or(Value::Null, |count| Value::String(count.to_string()))
}

fn optional_text(value: Option<&str>) -> Value {
    value.map_or(Value::Null, |text| Value::String(text.to_owned()))
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
                    "denom": optional_text(balance.denom.as_deref()),
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
            "finalized_batch": count_text(join.settlement.finalized_batch),
            "anchor_status": count_text(join.settlement.anchor_status),
            "anchor_status_name": optional_text(join.settlement.anchor_status_name.as_deref()),
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
        logs_filter, quantity, rpc_request, rpc_result, AccountIdentifier, ActivityWindow,
        Evidence, GatewayEndpoint, GatewayError, JoinedBalance, PaxeerEvent, ResolvedIdentities,
    };
    use layerx_network_gateway::GatewayError as NetworkGatewayError;
    use serde_json::Value;

    const ADDRESS: &str = "0x00112233445566778899aabbccddeeff00112233";
    const ACCOUNT: &str = "1111111111111111111111111111111111111111111111111111111111111111";
    const DID: &str = "2222222222222222222222222222222222222222222222222222222222222222";
    const ASSET: &str = "3333333333333333333333333333333333333333333333333333333333333333";
    const POINTER: &str = "0x00000000000000000000000000000000000f0013";

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

    /// The `account` half of every `px_getBalances` answer: the gateway sends
    /// the whole resolution document, never a bare identifier.
    fn resolution() -> Value {
        serde_json::json!({
            "evm_address": ADDRESS,
            "pax_address": "pax1qqqqq",
            "layerx_did": format!("did:layerx:{DID}"),
            "layerx_account": ACCOUNT,
            "bound": true,
        })
    }

    /// The custody precompile's record exactly as `px_getBalances` nests it.
    fn custody_asset(asset_id: &str, denom: &str, custodied: &str) -> Value {
        serde_json::json!({
            "asset_id": asset_id,
            "denom": denom,
            "pointer": POINTER,
            "enabled": true,
            "paused": false,
            "minimum_deposit": "1",
            "custody_cap": "1000000",
            "custodied": custodied,
            "released": "25",
            "pending": "0",
        })
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
            "account": resolution(),
            "balances": [{
                "asset_id": ACCOUNT,
                "denom": "upaxd",
                "custody": custody_asset(ACCOUNT, "upaxd", "1000"),
                "paxeer": { "denom": "upaxd", "amount": "16" },
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
                denom: Some("upaxd".to_owned()),
                custody: Some(1_000),
                paxeer: Some(16),
                layerx: None,
            }]
        );
        assert_eq!(
            decode_balances(&serde_json::json!({ "balances": [{ "asset_id": ACCOUNT }] })),
            Err(GatewayError::MalformedAnswer)
        );
        assert_eq!(quantity(&Value::String("0x10".to_owned())), Ok(16));
        assert_eq!(
            quantity(&Value::String("0x".to_owned())),
            Err(GatewayError::MalformedAnswer)
        );
        assert_eq!(
            quantity(&Value::String("-1".to_owned())),
            Err(GatewayError::MalformedAnswer)
        );
        assert_eq!(
            quantity(&Value::Bool(true)),
            Err(GatewayError::MalformedAnswer)
        );
    }

    #[test]
    fn the_gateway_balance_answer_decodes_every_nested_half() {
        let balances = decode_balances(&serde_json::json!({
            "account": resolution(),
            "balances": [
                {
                    "asset_id": ACCOUNT,
                    "denom": "upaxd",
                    "custody": custody_asset(ACCOUNT, "upaxd", "4200"),
                    "paxeer": { "denom": "upaxd", "amount": "1275" },
                    "layerx": {
                        "account_id": ACCOUNT,
                        "name": "treasury",
                        "asset_id": ACCOUNT,
                        "balance": "930",
                        "verification": "settlement_anchored",
                    },
                },
                {
                    "asset_id": ASSET,
                    "denom": Value::Null,
                    "custody": Value::Null,
                    "paxeer": Value::Null,
                    "layerx": {
                        "account_id": ASSET,
                        "name": "grant",
                        "asset_id": ASSET,
                        "balance": "7",
                        "verification": "settlement_anchored",
                    },
                },
            ],
            "joined_limit": 16,
        }))
        .expect("the gateway's own balance answer decodes");
        assert_eq!(balances.joined_limit, 16);
        assert_eq!(balances.evidence, Evidence::GatewayReported);
        assert_eq!(
            balances.items,
            vec![
                JoinedBalance {
                    asset_id: [0x11; 32],
                    denom: Some("upaxd".to_owned()),
                    custody: Some(4_200),
                    paxeer: Some(1_275),
                    layerx: Some(930),
                },
                JoinedBalance {
                    asset_id: [0x33; 32],
                    denom: None,
                    custody: None,
                    paxeer: None,
                    layerx: Some(7),
                },
            ]
        );
    }

    #[test]
    fn the_settlement_ladder_carries_the_anchor_position() {
        let ladder = decode_settlement(&serde_json::json!({
            "network_id": "paxeer-x",
            "paxeer": { "chain_id": "0x22b8", "latest_block": "0x2a" },
            "layerx": { "node_info": {} },
            "anchor": {
                "latest_finalized_batch": 19,
                "status": 2,
                "status_name": "final",
                "status_ladder": { "0": "unknown", "1": "submitted", "2": "final" },
            },
        }))
        .expect("declared network document decodes");
        assert_eq!(ladder.chain_id, 8_888);
        assert_eq!(ladder.instant_block, 42);
        assert_eq!(ladder.finalized_batch, Some(19));
        assert_eq!(ladder.anchor_status, Some(2));
        assert_eq!(ladder.anchor_status_name.as_deref(), Some("final"));
        assert_eq!(
            decode_settlement(&serde_json::json!({ "network_id": "paxeer-x" })),
            Err(GatewayError::MalformedAnswer)
        );
    }

    #[test]
    fn an_anchor_without_a_finalised_batch_leaves_that_rung_absent() {
        let unanchored = decode_settlement(&serde_json::json!({
            "network_id": "paxeer-x",
            "paxeer": { "chain_id": "0x22b8", "latest_block": "0x2a" },
            "layerx": { "node_info": {} },
            "anchor": Value::Null,
        }))
        .expect("a network document with no anchor rung decodes");
        assert_eq!(unanchored.instant_block, 42);
        assert_eq!(unanchored.finalized_batch, None);
        assert_eq!(unanchored.anchor_status, None);
        assert_eq!(unanchored.anchor_status_name, None);

        let unfinalised = decode_settlement(&serde_json::json!({
            "network_id": "paxeer-x",
            "paxeer": { "chain_id": "0x22b8", "latest_block": "0x2a" },
            "layerx": { "node_info": {} },
            "anchor": {
                "latest_finalized_batch": Value::Null,
                "status": Value::Null,
                "status_name": Value::Null,
                "status_ladder": { "0": "unknown", "1": "submitted", "2": "final" },
            },
        }))
        .expect("an anchor with no finalised batch decodes");
        assert_eq!(unfinalised.finalized_batch, None);
        assert_eq!(unfinalised.anchor_status, None);
        assert_eq!(unfinalised.anchor_status_name, None);
        assert_eq!(unfinalised.network_id, "paxeer-x");
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
            filter["topics"][0].as_array().map(|topics| topics.len()),
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
            record
                .address
                .map(|bytes| format!("0x{}", hex_bytes(&bytes))),
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
        assert_eq!(
            window.chunks(10_000, None),
            vec![(9_601, 10_000), (9_201, 9_600)]
        );
        assert_eq!(window.next_before(10_000, 9_201), Some(9_201));
        assert_eq!(window.chunks(10_000, Some(9_201)), vec![(9_001, 9_200)]);
        assert_eq!(window.next_before(10_000, 9_001), None);
        assert_eq!(window.chunks(300, None), vec![(0, 300)]);
        assert_eq!(window.next_before(300, 0), None);
        assert_eq!(window.chunks(10_000, Some(0)), Vec::new());
        for refused in [
            ActivityWindow {
                span_blocks: 0,
                ..window
            },
            ActivityWindow {
                chunk_blocks: 0,
                ..window
            },
            ActivityWindow {
                chunk_blocks: 2_000,
                ..window
            },
            ActivityWindow {
                max_chunks: 0,
                ..window
            },
            ActivityWindow { limit: 0, ..window },
            ActivityWindow {
                limit: 101,
                ..window
            },
        ] {
            assert_eq!(refused.validate(), Err(GatewayError::InvalidWindow));
        }
    }

    #[test]
    fn endpoints_and_http_answers_fail_closed() {
        assert_eq!(
            GatewayEndpoint::parse("https://gateway.example:8545/rpc")
                .map(|endpoint| format!("{endpoint:?}")),
            Ok(concat!(
                "GatewayEndpoint { secure: true, host: \"gateway.example\", ",
                "port: 8545, path: \"/rpc\" }"
            )
            .to_owned())
        );
        assert_eq!(
            GatewayEndpoint::parse("http://127.0.0.1:26657")
                .map(|endpoint| format!("{endpoint:?}")),
            Ok(concat!(
                "GatewayEndpoint { secure: false, host: \"127.0.0.1\", ",
                "port: 26657, path: \"/\" }"
            )
            .to_owned())
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
                Err(NetworkGatewayError::InvalidEndpoint),
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
                "account": resolution(),
                "balances": [{
                    "asset_id": ACCOUNT,
                    "denom": "upaxd",
                    "custody": custody_asset(ACCOUNT, "upaxd", "7"),
                    "paxeer": { "denom": "upaxd", "amount": "5" },
                    "layerx": {
                        "account_id": ACCOUNT,
                        "name": "treasury",
                        "asset_id": ACCOUNT,
                        "balance": "2",
                        "verification": "settlement_anchored",
                    },
                }],
                "joined_limit": 64,
            }))
            .expect("balances decode"),
            settlement: SettlementLadder {
                network_id: "paxeer-x".to_owned(),
                chain_id: 8_888,
                instant_block: 100,
                finalized_batch: Some(19),
                anchor_status: Some(2),
                anchor_status_name: Some("finalised".to_owned()),
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
        assert_eq!(
            document["identities"]["layerx_did"],
            format!("did:layerx:{DID}")
        );
        assert_eq!(document["balances"]["items"][0]["denom"], "upaxd");
        assert_eq!(document["balances"]["items"][0]["layerx"], "2");
        assert_eq!(document["settlement"]["sealed_batch"], "7");
        assert_eq!(document["settlement"]["finalized_batch"], "19");
        assert_eq!(document["settlement"]["instant_block"], "100");
        assert_eq!(
            document["paxeer_activity"]["items"][0]["event"],
            "custody-deposit"
        );
        assert_eq!(document["paxeer_activity"]["items"][0]["amount"], "255");
        assert_eq!(document["paxeer_activity"]["next_before_block"], "1");
    }

    #[test]
    fn an_unread_rung_or_denomination_renders_as_null_never_as_zero() {
        use super::{
            decode_settlement, unified_account_json, PaxeerActivityPage, UnifiedAccountJoin,
        };
        let identities = identities(true);
        let requested = AccountIdentifier::parse(ADDRESS).expect("address parses");
        let join = UnifiedAccountJoin {
            requested,
            canonical: identities.canonical(requested),
            identities,
            balances: decode_balances(&serde_json::json!({
                "account": resolution(),
                "balances": [{
                    "asset_id": ASSET,
                    "denom": Value::Null,
                    "custody": Value::Null,
                    "paxeer": Value::Null,
                    "layerx": Value::Null,
                }],
                "joined_limit": 16,
            }))
            .expect("a row with no joined half decodes"),
            settlement: decode_settlement(&serde_json::json!({
                "network_id": "paxeer-x",
                "paxeer": { "chain_id": "0x22b8", "latest_block": "0x2a" },
                "layerx": { "node_info": {} },
                "anchor": Value::Null,
            }))
            .expect("a network document with no anchor rung decodes"),
            paxeer_activity: PaxeerActivityPage {
                items: Vec::new(),
                from_block: 42,
                to_block: 42,
                next_before_block: None,
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
        assert_eq!(document["balances"]["items"][0]["denom"], Value::Null);
        assert_eq!(document["balances"]["items"][0]["custody"], Value::Null);
        assert_eq!(document["balances"]["items"][0]["paxeer"], Value::Null);
        assert_eq!(document["balances"]["items"][0]["layerx"], Value::Null);
        assert_eq!(document["settlement"]["instant_block"], "42");
        assert_eq!(document["settlement"]["sealed_batch"], "7");
        assert_eq!(document["settlement"]["finalized_batch"], Value::Null);
        assert_eq!(document["settlement"]["anchor_status"], Value::Null);
        assert_eq!(document["settlement"]["anchor_status_name"], Value::Null);
    }
}
