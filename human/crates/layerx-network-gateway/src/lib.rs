//! One client for the Paxeer X Network gateway's `px_*` reads.
//!
//! The network gateway serves a single JSON-RPC endpoint that joins the two
//! execution domains: `px_resolveAccount`, `px_getAccount`, `px_getBalances`,
//! `px_listAssets` and `px_getNetwork`, alongside the byte-faithful `eth_*`
//! relay. Every service that reads those joins shares this client rather than
//! restating the transport, the JSON-RPC envelope or the answer grammar.
//!
//! Nothing here carries a protocol proof. Every value this client returns is
//! what the gateway reported, tagged [`Evidence::GatewayReported`], so no
//! caller can mistake a gateway answer for receipt-verified state. An
//! unavailable or malformed answer is a typed [`GatewayError`]: this client
//! never substitutes a zero, an empty table or a default for a read it could
//! not complete.

use std::fmt;
use std::io::{Read, Write};
use std::net::{TcpStream, ToSocketAddrs as _};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

use serde_json::Value;

const DID_PREFIX: &str = "did:layerx:";
const ANSWER_LIMIT: u64 = 8 * 1024 * 1024;
const IO_TIMEOUT: Duration = Duration::from_secs(10);
const MAXIMUM_JOINED_ASSETS: usize = 1_024;

/// Longest accepted `pax_address` text.
pub const PAX_ADDRESS_LIMIT: usize = 128;
/// Longest accepted asset denomination text.
pub const DENOM_LIMIT: usize = 128;
/// Longest accepted anchor status name.
pub const STATUS_NAME_LIMIT: usize = 64;
/// Longest accepted network identifier text.
pub const NETWORK_ID_LIMIT: usize = 64;

static NEXT_REQUEST: AtomicU64 = AtomicU64::new(1);

/// How a fact reached a caller. A gateway answer is never upgraded to a
/// verified level: the two sources stay distinguishable at every hop.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Evidence {
    /// Reported by the network gateway without a caller-checkable proof.
    GatewayReported,
}

impl Evidence {
    /// Returns the stable lowercase label used in APIs and logs.
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
            return decode_digest(body).map(Self::Did);
        }
        decode_digest(&lowered).map(Self::Account)
    }

    /// Renders the exact normalised spelling this identifier is addressed by.
    #[must_use]
    pub fn canonical_text(self) -> String {
        match self {
            Self::Evm(address) => format!("0x{}", encode_hex(&address)),
            Self::Did(key) => format!("{DID_PREFIX}{}", encode_hex(&key)),
            Self::Account(account) => encode_hex(&account),
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

const DIGITS: &[u8; 16] = b"0123456789abcdef";

/// Encodes bytes as lowercase hexadecimal.
#[must_use]
pub fn encode_hex(bytes: &[u8]) -> String {
    let mut encoded = String::with_capacity(bytes.len().saturating_mul(2));
    for byte in bytes {
        encoded.push(char::from(DIGITS[usize::from(byte >> 4)]));
        encoded.push(char::from(DIGITS[usize::from(byte & 0x0f)]));
    }
    encoded
}

fn nibble(byte: u8) -> Option<u8> {
    match byte {
        b'0'..=b'9' => Some(byte - b'0'),
        b'a'..=b'f' => Some(byte - b'a' + 10),
        b'A'..=b'F' => Some(byte - b'A' + 10),
        _ => None,
    }
}

/// Decodes an even-length hexadecimal string.
///
/// # Errors
/// Refuses odd lengths and non-hexadecimal characters.
pub fn decode_hex(text: &str) -> Result<Vec<u8>, IdentifierError> {
    if text.len() % 2 != 0 {
        return Err(IdentifierError);
    }
    let mut bytes = Vec::with_capacity(text.len() / 2);
    for pair in text.as_bytes().chunks_exact(2) {
        let high = nibble(pair[0]).ok_or(IdentifierError)?;
        let low = nibble(pair[1]).ok_or(IdentifierError)?;
        bytes.push((high << 4) | low);
    }
    Ok(bytes)
}

/// Decodes exactly thirty-two hexadecimal-encoded bytes.
///
/// # Errors
/// Refuses wrong lengths and non-hexadecimal characters.
pub fn decode_digest(text: &str) -> Result<[u8; 32], IdentifierError> {
    if text.len() != 64 {
        return Err(IdentifierError);
    }
    let bytes = decode_hex(text)?;
    <[u8; 32]>::try_from(bytes.as_slice()).map_err(|_| IdentifierError)
}

fn decode_evm(body: &str) -> Result<[u8; 20], IdentifierError> {
    let bytes = decode_hex(body)?;
    <[u8; 20]>::try_from(bytes.as_slice()).map_err(|_| IdentifierError)
}

/// Both identities of one account as the gateway reports them.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ResolvedIdentities {
    /// The bound Paxeer EVM address, when the network knows one.
    pub evm_address: Option<[u8; 20]>,
    /// The bech32 Paxeer address, when the network reports one.
    pub pax_address: Option<String>,
    /// The bound LayerX decentralised identifier public key.
    pub layerx_did: Option<[u8; 32]>,
    /// The bound LayerX account identifier.
    pub layerx_account: Option<[u8; 32]>,
    /// Whether the two halves are bound to each other.
    pub bound: bool,
    /// How this fact reached the caller.
    pub evidence: Evidence,
}

impl ResolvedIdentities {
    /// The one identifier this account is addressed by: the LayerX account when
    /// the network knows one, otherwise the spelling that was asked for.
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

/// The Paxeer half of one account as `px_getAccount` reports it.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PaxeerAccount {
    /// The EVM address the balance and nonce were read at.
    pub address: [u8; 20],
    /// The native balance at the latest block.
    pub balance: u128,
    /// The transaction count at the latest block.
    pub nonce: u64,
}

/// One account joined across both domains by `px_getAccount`.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AccountJoin {
    /// The resolved identities of the account.
    pub account: ResolvedIdentities,
    /// The Paxeer half, absent when the account is not bound to an address.
    pub paxeer: Option<PaxeerAccount>,
    /// The LayerX public core account document, absent when there is none.
    pub layerx: Option<Value>,
    /// How this fact reached the caller.
    pub evidence: Evidence,
}

/// The Paxeer bank balance of one asset for one address.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PaxeerAssetBalance {
    /// The bank denomination the balance was read under.
    pub denom: String,
    /// The exact balance.
    pub amount: u128,
}

/// The custody precompile's record for one asset.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CustodyAsset {
    /// The LayerX asset identifier.
    pub asset_id: [u8; 32],
    /// The bank denomination custody mints against.
    pub denom: String,
    /// Whether deposits are accepted.
    pub enabled: bool,
    /// Whether the asset is paused.
    pub paused: bool,
    /// The smallest accepted deposit.
    pub minimum_deposit: u128,
    /// The ceiling on total custodied value.
    pub custody_cap: u128,
    /// The amount currently custodied.
    pub custodied: u128,
    /// The amount already released.
    pub released: u128,
    /// The amount queued for release.
    pub pending: u128,
}

/// One asset held by the same account in either domain, joined through the
/// custody asset map, exactly as `px_getBalances` reports it.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AssetBalance {
    /// The LayerX asset identifier.
    pub asset_id: [u8; 32],
    /// The joined denomination, when either domain reports one.
    pub denom: Option<String>,
    /// The custody precompile's record, absent when the asset is not custodied.
    pub custody: Option<CustodyAsset>,
    /// The Paxeer bank balance, absent when no address or denom is known.
    pub paxeer: Option<PaxeerAssetBalance>,
    /// The LayerX account document for this asset, absent when there is none.
    pub layerx: Option<Value>,
}

impl AssetBalance {
    /// The LayerX-side spendable amount this row reports, when it reports one.
    ///
    /// # Errors
    /// Refuses a LayerX document whose balance is not a gateway quantity.
    pub fn layerx_amount(&self) -> Result<Option<u128>, GatewayError> {
        let Some(document) = self.layerx.as_ref() else {
            return Ok(None);
        };
        let value = document
            .get("balance")
            .or_else(|| document.get("amount"))
            .ok_or(GatewayError::MalformedAnswer)?;
        quantity(value).map(Some)
    }
}

/// The bounded joined balance table for one account.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AccountBalances {
    /// The resolved identities the table was joined for.
    pub account: ResolvedIdentities,
    /// One row per joined asset, in the gateway's own order.
    pub balances: Vec<AssetBalance>,
    /// The number of assets the gateway joins per answer.
    pub joined_limit: u64,
    /// How this fact reached the caller.
    pub evidence: Evidence,
}

impl AccountBalances {
    /// The row for one asset, when the joined table carries it.
    #[must_use]
    pub fn asset(&self, asset_id: [u8; 32]) -> Option<&AssetBalance> {
        self.balances.iter().find(|row| row.asset_id == asset_id)
    }
}

/// One entry of the joined asset map from `px_listAssets`.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AssetEntry {
    /// The LayerX asset identifier.
    pub asset_id: [u8; 32],
    /// The LayerX public core asset record.
    pub layerx: Option<Value>,
    /// The custody precompile's record, absent when the asset is not custodied.
    pub paxeer: Option<CustodyAsset>,
}

/// The bounded joined asset map.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AssetTable {
    /// One entry per joined asset, in the gateway's own order.
    pub assets: Vec<AssetEntry>,
    /// The number of assets the gateway joins per answer.
    pub joined_limit: u64,
    /// How this fact reached the caller.
    pub evidence: Evidence,
}

/// The anchor rung of the settlement ladder.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AnchorHead {
    /// The newest batch the anchor reports as finalised, when one exists.
    pub latest_finalized_batch: Option<u64>,
    /// The anchor's own status code for that batch.
    pub status: Option<u64>,
    /// The anchor's own name for that status.
    pub status_name: Option<String>,
}

/// One head for the whole network, as `px_getNetwork` reports it.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct NetworkHead {
    /// The network identifier text.
    pub network_id: String,
    /// The Paxeer EVM chain identifier.
    pub chain_id: u64,
    /// The Paxeer block the gateway currently reports as latest.
    pub latest_block: u64,
    /// The anchor rung.
    pub anchor: AnchorHead,
    /// How this fact reached the caller.
    pub evidence: Evidence,
}

/// Every way a gateway read can fail to produce a usable answer.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum GatewayError {
    /// The endpoint is not `http://<host>[:<port>]` or `https://<host>[:<port>]`.
    InvalidEndpoint,
    /// The endpoint could not be reached or answered outside HTTP.
    Transport(String),
    /// The gateway answered a JSON-RPC error.
    Refused {
        /// The gateway's own refusal code.
        code: i64,
        /// The gateway's own refusal message.
        message: String,
    },
    /// The answer is not a JSON-RPC answer for the request that was sent.
    Unbound,
    /// The answer is not the documented gateway document.
    MalformedAnswer,
}

impl fmt::Display for GatewayError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidEndpoint => formatter.write_str("gateway endpoint is not http(s)://host"),
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
        let (authority, path) = rest
            .split_once('/')
            .map_or((rest, "/".to_owned()), |(authority, path)| {
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

    /// Reads the endpoint the deployment declares in
    /// `LAYERX_NETWORK_GATEWAY_ENDPOINT`.
    ///
    /// # Errors
    /// Refuses an unset, empty or malformed endpoint. A service that reads the
    /// network through the gateway never falls back to a default address.
    pub fn from_environment() -> Result<Self, GatewayError> {
        let declared =
            std::env::var(ENDPOINT_VARIABLE).map_err(|_| GatewayError::InvalidEndpoint)?;
        Self::parse(&declared)
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
            let mut stream = connector.connect(&self.host, stream).map_err(|error| {
                GatewayError::Transport(format!("TLS handshake failed: {error}"))
            })?;
            exchange(&mut stream, &head, body.as_bytes())?
        } else {
            let mut stream = stream;
            exchange(&mut stream, &head, body.as_bytes())?
        };
        http_body(&answer)
    }

    /// Posts one `px_*` or `eth_*` call and returns its JSON-RPC result.
    ///
    /// # Errors
    /// Reports the first transport, binding or refusal failure.
    pub fn call(&self, method: &str, params: &[Value]) -> Result<Value, GatewayError> {
        let id = NEXT_REQUEST.fetch_add(1, Ordering::Relaxed);
        let answer = self.post(&rpc_request(id, method, params))?;
        rpc_result(id, &answer)
    }

    /// Reads `px_resolveAccount` for one account spelling.
    ///
    /// # Errors
    /// Reports the first transport, refusal or decoding failure.
    pub fn resolve_account(
        &self,
        account: AccountIdentifier,
    ) -> Result<ResolvedIdentities, GatewayError> {
        let key = Value::String(account.canonical_text());
        decode_identities(&self.call("px_resolveAccount", &[key])?)
    }

    /// Reads `px_getAccount` for one account spelling.
    ///
    /// # Errors
    /// Reports the first transport, refusal or decoding failure.
    pub fn get_account(&self, account: AccountIdentifier) -> Result<AccountJoin, GatewayError> {
        let key = Value::String(account.canonical_text());
        decode_account(&self.call("px_getAccount", &[key])?)
    }

    /// Reads `px_getBalances` for one account spelling.
    ///
    /// # Errors
    /// Reports the first transport, refusal or decoding failure.
    pub fn get_balances(
        &self,
        account: AccountIdentifier,
    ) -> Result<AccountBalances, GatewayError> {
        let key = Value::String(account.canonical_text());
        decode_account_balances(&self.call("px_getBalances", &[key])?)
    }

    /// Reads the joined asset map through `px_listAssets`.
    ///
    /// # Errors
    /// Reports the first transport, refusal or decoding failure.
    pub fn list_assets(&self) -> Result<AssetTable, GatewayError> {
        decode_assets(&self.call("px_listAssets", &[])?)
    }

    /// Reads the network head through `px_getNetwork`.
    ///
    /// # Errors
    /// Reports the first transport, refusal or decoding failure.
    pub fn get_network(&self) -> Result<NetworkHead, GatewayError> {
        decode_network(&self.call("px_getNetwork", &[])?)
    }
}

/// The deployment variable every service reads the gateway endpoint from.
pub const ENDPOINT_VARIABLE: &str = "LAYERX_NETWORK_GATEWAY_ENDPOINT";

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

/// Reads an optional bounded text field.
///
/// # Errors
/// Refuses a non-text value and text beyond `limit`.
pub fn optional_text(value: &Value, limit: usize) -> Result<Option<String>, GatewayError> {
    match value {
        Value::Null => Ok(None),
        Value::String(text) if !text.is_empty() && text.len() <= limit => Ok(Some(text.clone())),
        _ => Err(GatewayError::MalformedAnswer),
    }
}

/// Reads a required bounded text field.
///
/// # Errors
/// Refuses an absent, empty, non-text or over-long value.
pub fn required_text(value: &Value, limit: usize) -> Result<String, GatewayError> {
    optional_text(value, limit)?.ok_or(GatewayError::MalformedAnswer)
}

/// Reads an optional `0x`-prefixed twenty-byte address.
///
/// # Errors
/// Refuses every other spelling.
pub fn optional_address(value: &Value) -> Result<Option<[u8; 20]>, GatewayError> {
    match value {
        Value::Null => Ok(None),
        Value::String(text) if text.is_empty() => Ok(None),
        Value::String(text) => address(text).map(Some),
        _ => Err(GatewayError::MalformedAnswer),
    }
}

/// Reads a `0x`-prefixed twenty-byte address.
///
/// # Errors
/// Refuses every other spelling.
pub fn address(text: &str) -> Result<[u8; 20], GatewayError> {
    let lowered = text.trim().to_ascii_lowercase();
    let body = lowered
        .strip_prefix("0x")
        .ok_or(GatewayError::MalformedAnswer)?;
    decode_evm(body).map_err(|_| GatewayError::MalformedAnswer)
}

/// Reads an optional thirty-two byte digest in any of its public spellings.
///
/// # Errors
/// Refuses every other spelling.
pub fn optional_digest(value: &Value) -> Result<Option<[u8; 32]>, GatewayError> {
    match value {
        Value::Null => Ok(None),
        Value::String(text) if text.is_empty() => Ok(None),
        Value::String(text) => digest(text).map(Some),
        _ => Err(GatewayError::MalformedAnswer),
    }
}

/// Reads a thirty-two byte digest in any of its public spellings.
///
/// # Errors
/// Refuses every other spelling.
pub fn digest(text: &str) -> Result<[u8; 32], GatewayError> {
    let lowered = text.trim().to_ascii_lowercase();
    let body = lowered
        .strip_prefix(DID_PREFIX)
        .or_else(|| lowered.strip_prefix("0x"))
        .unwrap_or(&lowered);
    decode_digest(body).map_err(|_| GatewayError::MalformedAnswer)
}

/// Reads a boolean field.
///
/// # Errors
/// Refuses every other value.
pub fn boolean(value: &Value) -> Result<bool, GatewayError> {
    value.as_bool().ok_or(GatewayError::MalformedAnswer)
}

/// Decodes a gateway quantity: an unsigned decimal string, a `0x` quantity or
/// a JSON integer. Nothing else is admitted.
///
/// # Errors
/// Refuses every other spelling and any value beyond 128 bits.
pub fn quantity(value: &Value) -> Result<u128, GatewayError> {
    match value {
        Value::Number(number) => number
            .as_u64()
            .map(u128::from)
            .ok_or(GatewayError::MalformedAnswer),
        Value::String(text) => {
            let trimmed = text.trim();
            if let Some(body) = trimmed
                .strip_prefix("0x")
                .or_else(|| trimmed.strip_prefix("0X"))
            {
                if body.is_empty()
                    || body.len() > 32
                    || !body.bytes().all(|byte| byte.is_ascii_hexdigit())
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

/// Reads an optional gateway quantity.
///
/// # Errors
/// Refuses every spelling [`quantity`] refuses.
pub fn optional_quantity(value: &Value) -> Result<Option<u128>, GatewayError> {
    if value.is_null() {
        return Ok(None);
    }
    quantity(value).map(Some)
}

/// Reads a gateway quantity that must fit in sixty-four bits.
///
/// # Errors
/// Refuses every spelling [`quantity`] refuses and any value beyond 64 bits.
pub fn counted(value: &Value) -> Result<u64, GatewayError> {
    u64::try_from(quantity(value)?).map_err(|_| GatewayError::MalformedAnswer)
}

fn optional_counted(value: &Value) -> Result<Option<u64>, GatewayError> {
    if value.is_null() {
        return Ok(None);
    }
    counted(value).map(Some)
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

/// Decodes the `px_getAccount` result.
///
/// # Errors
/// Refuses a document that is not the declared shape.
pub fn decode_account(result: &Value) -> Result<AccountJoin, GatewayError> {
    if !result.is_object() {
        return Err(GatewayError::MalformedAnswer);
    }
    let account = decode_identities(&result["account"])?;
    let paxeer = match &result["paxeer"] {
        Value::Null => None,
        document if document.is_object() => Some(PaxeerAccount {
            address: address(
                document["address"]
                    .as_str()
                    .ok_or(GatewayError::MalformedAnswer)?,
            )?,
            balance: quantity(&document["balance"])?,
            nonce: counted(&document["nonce"])?,
        }),
        _ => return Err(GatewayError::MalformedAnswer),
    };
    Ok(AccountJoin {
        account,
        paxeer,
        layerx: object_or_null(&result["layerx"])?,
        evidence: Evidence::GatewayReported,
    })
}

fn object_or_null(value: &Value) -> Result<Option<Value>, GatewayError> {
    match value {
        Value::Null => Ok(None),
        document if document.is_object() => Ok(Some(document.clone())),
        _ => Err(GatewayError::MalformedAnswer),
    }
}

fn decode_custody_asset(value: &Value) -> Result<Option<CustodyAsset>, GatewayError> {
    let Some(document) = object_or_null(value)? else {
        return Ok(None);
    };
    Ok(Some(CustodyAsset {
        asset_id: digest(
            document["asset_id"]
                .as_str()
                .ok_or(GatewayError::MalformedAnswer)?,
        )?,
        denom: required_text(&document["denom"], DENOM_LIMIT)?,
        enabled: boolean(&document["enabled"])?,
        paused: boolean(&document["paused"])?,
        minimum_deposit: quantity(&document["minimum_deposit"])?,
        custody_cap: quantity(&document["custody_cap"])?,
        custodied: quantity(&document["custodied"])?,
        released: quantity(&document["released"])?,
        pending: quantity(&document["pending"])?,
    }))
}

/// Decodes the `px_getBalances` result.
///
/// # Errors
/// Refuses a document that is not the declared shape or exceeds the joined
/// asset ceiling.
pub fn decode_account_balances(result: &Value) -> Result<AccountBalances, GatewayError> {
    let rows = result["balances"]
        .as_array()
        .ok_or(GatewayError::MalformedAnswer)?;
    if rows.len() > MAXIMUM_JOINED_ASSETS {
        return Err(GatewayError::MalformedAnswer);
    }
    let mut balances = Vec::with_capacity(rows.len());
    for row in rows {
        let paxeer = match object_or_null(&row["paxeer"])? {
            None => None,
            Some(document) => Some(PaxeerAssetBalance {
                denom: required_text(&document["denom"], DENOM_LIMIT)?,
                amount: quantity(&document["amount"])?,
            }),
        };
        balances.push(AssetBalance {
            asset_id: digest(
                row["asset_id"]
                    .as_str()
                    .ok_or(GatewayError::MalformedAnswer)?,
            )?,
            denom: optional_text(&row["denom"], DENOM_LIMIT)?,
            custody: decode_custody_asset(&row["custody"])?,
            paxeer,
            layerx: object_or_null(&row["layerx"])?,
        });
    }
    Ok(AccountBalances {
        account: decode_identities(&result["account"])?,
        balances,
        joined_limit: counted(&result["joined_limit"])?,
        evidence: Evidence::GatewayReported,
    })
}

/// Decodes the `px_listAssets` result.
///
/// # Errors
/// Refuses a document that is not the declared shape or exceeds the joined
/// asset ceiling.
pub fn decode_assets(result: &Value) -> Result<AssetTable, GatewayError> {
    let rows = result["assets"]
        .as_array()
        .ok_or(GatewayError::MalformedAnswer)?;
    if rows.len() > MAXIMUM_JOINED_ASSETS {
        return Err(GatewayError::MalformedAnswer);
    }
    let mut assets = Vec::with_capacity(rows.len());
    for row in rows {
        assets.push(AssetEntry {
            asset_id: digest(
                row["asset_id"]
                    .as_str()
                    .ok_or(GatewayError::MalformedAnswer)?,
            )?,
            layerx: object_or_null(&row["layerx"])?,
            paxeer: decode_custody_asset(&row["paxeer"])?,
        });
    }
    Ok(AssetTable {
        assets,
        joined_limit: counted(&result["joined_limit"])?,
        evidence: Evidence::GatewayReported,
    })
}

/// Decodes the `px_getNetwork` result.
///
/// # Errors
/// Refuses a document that is not the declared shape.
pub fn decode_network(result: &Value) -> Result<NetworkHead, GatewayError> {
    let paxeer = &result["paxeer"];
    let anchor = &result["anchor"];
    Ok(NetworkHead {
        network_id: required_text(&result["network_id"], NETWORK_ID_LIMIT)?,
        chain_id: counted(&paxeer["chain_id"])?,
        latest_block: counted(&paxeer["latest_block"])?,
        anchor: AnchorHead {
            latest_finalized_batch: optional_counted(&anchor["latest_finalized_batch"])?,
            status: optional_counted(&anchor["status"])?,
            status_name: optional_text(&anchor["status_name"], STATUS_NAME_LIMIT)?,
        },
        evidence: Evidence::GatewayReported,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    use serde_json::json;

    #[test]
    fn endpoint_parsing_admits_only_http_and_https_authorities() {
        let endpoint = GatewayEndpoint::parse("https://gateway.example/rpc")
            .unwrap_or_else(|error| panic!("endpoint: {error}"));
        assert_eq!(endpoint.host, "gateway.example");
        assert_eq!(endpoint.port, 443);
        assert_eq!(endpoint.path, "/rpc");
        assert!(endpoint.secure);
        for refused in [
            "ftp://gateway.example",
            "https://",
            "https://gateway.example:0",
            "gateway.example",
        ] {
            assert_eq!(
                GatewayEndpoint::parse(refused),
                Err(GatewayError::InvalidEndpoint)
            );
        }
    }

    #[test]
    fn rpc_answers_bind_to_their_own_request_identifier() {
        let answer = br#"{"jsonrpc":"2.0","id":7,"result":{"bound":true}}"#;
        assert_eq!(rpc_result(7, answer), Ok(json!({"bound": true})));
        assert_eq!(rpc_result(8, answer), Err(GatewayError::Unbound));
        let refusal = br#"{"jsonrpc":"2.0","id":7,"error":{"code":-32001,"message":"unavailable"}}"#;
        assert_eq!(
            rpc_result(7, refusal),
            Err(GatewayError::Refused {
                code: -32001,
                message: "unavailable".to_owned()
            })
        );
    }

    #[test]
    fn balances_decode_the_declared_nested_join_shape() {
        let result = json!({
            "account": {
                "evm_address": "0x1111111111111111111111111111111111111111",
                "pax_address": "pax1example",
                "layerx_did": format!("did:layerx:{}", "22".repeat(32)),
                "layerx_account": "33".repeat(32),
                "bound": true
            },
            "balances": [{
                "asset_id": "44".repeat(32),
                "denom": "upax",
                "custody": {
                    "asset_id": "44".repeat(32),
                    "denom": "upax",
                    "pointer": "0x2222222222222222222222222222222222222222",
                    "enabled": true,
                    "paused": false,
                    "minimum_deposit": "1",
                    "custody_cap": "1000",
                    "custodied": "500",
                    "released": "10",
                    "pending": "0"
                },
                "paxeer": {"denom": "upax", "amount": "250"},
                "layerx": {"asset_id": "44".repeat(32), "balance": "125"}
            }],
            "joined_limit": 16
        });
        let balances =
            decode_account_balances(&result).unwrap_or_else(|error| panic!("balances: {error}"));
        assert_eq!(balances.joined_limit, 16);
        assert!(balances.account.bound);
        let row = balances
            .asset([0x44; 32])
            .unwrap_or_else(|| panic!("asset row"));
        assert_eq!(row.denom.as_deref(), Some("upax"));
        assert_eq!(
            row.custody.as_ref().map(|custody| custody.custodied),
            Some(500)
        );
        assert_eq!(row.paxeer.as_ref().map(|paxeer| paxeer.amount), Some(250));
        assert_eq!(row.layerx_amount(), Ok(Some(125)));
    }

    #[test]
    fn an_unavailable_half_decodes_as_absent_and_never_as_zero() {
        let result = json!({
            "account": {
                "evm_address": Value::Null,
                "pax_address": Value::Null,
                "layerx_did": Value::Null,
                "layerx_account": Value::Null,
                "bound": false
            },
            "paxeer": Value::Null,
            "layerx": Value::Null
        });
        let join = decode_account(&result).unwrap_or_else(|error| panic!("account: {error}"));
        assert!(join.paxeer.is_none());
        assert!(join.layerx.is_none());
        assert!(!join.account.bound);
        assert_eq!(join.evidence, Evidence::GatewayReported);
    }

    #[test]
    fn network_head_decodes_every_rung_the_gateway_reports() {
        let result = json!({
            "network_id": "paxeer-x",
            "paxeer": {"chain_id": "0x1f", "latest_block": 4_096},
            "anchor": {
                "latest_finalized_batch": 12,
                "status": 2,
                "status_name": "final",
                "status_ladder": {"0": "unknown", "1": "submitted", "2": "final"}
            },
            "layerx": {"node_info": {}}
        });
        let head = decode_network(&result).unwrap_or_else(|error| panic!("network: {error}"));
        assert_eq!(head.network_id, "paxeer-x");
        assert_eq!(head.chain_id, 31);
        assert_eq!(head.latest_block, 4_096);
        assert_eq!(head.anchor.latest_finalized_batch, Some(12));
        assert_eq!(head.anchor.status_name.as_deref(), Some("final"));
    }

    #[test]
    fn a_malformed_quantity_is_refused_rather_than_defaulted() {
        assert_eq!(quantity(&json!("12")), Ok(12));
        assert_eq!(quantity(&json!("0x0c")), Ok(12));
        assert_eq!(quantity(&json!(12)), Ok(12));
        for refused in [json!("-1"), json!("0x"), json!("zz"), json!(true)] {
            assert_eq!(quantity(&refused), Err(GatewayError::MalformedAnswer));
        }
    }
}
