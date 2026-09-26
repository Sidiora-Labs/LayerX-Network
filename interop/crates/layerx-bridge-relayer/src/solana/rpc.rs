//! Solana JSON-RPC seam, beside the Ethereum one. Every request goes through
//! the crate's existing [`JsonRpc`] transport (the HTTPS strict-majority
//! quorum in production, recorded exchanges in tests) and every answer is
//! parsed into a typed value here, so the observer never reads raw JSON and
//! every failure is one of the [`RpcFault`]s the Ethereum seam returns.

use serde::Deserialize;
use serde_json::{json, Map, Value};

use super::{base58_data, base58_encode, base58_fixed, base64_decode, base64_encode};
use crate::rpc::{JsonRpc, RpcFault, SendOutcome};

/// The most signatures one `getSignaturesForAddress` page may return.
pub const MAX_SIGNATURE_PAGE: usize = 1000;

/// The largest wire transaction Solana accepts.
pub const MAX_TRANSACTION_BYTES: usize = 1232;

/// The commitment the relayer reads Solana at. `processed` is never accepted:
/// a processed slot can still be skipped.
#[derive(Clone, Copy, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd)]
#[serde(rename_all = "snake_case")]
pub enum Commitment {
    Confirmed,
    Finalized,
}

impl Commitment {
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Confirmed => "confirmed",
            Self::Finalized => "finalized",
        }
    }
}

/// How far a cluster has confirmed a signature.
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub enum Confirmation {
    Processed,
    Confirmed,
    Finalized,
}

impl Confirmation {
    fn parse(value: &Value) -> Result<Self, RpcFault> {
        match value.as_str() {
            Some("processed") => Ok(Self::Processed),
            Some("confirmed") => Ok(Self::Confirmed),
            Some("finalized") => Ok(Self::Finalized),
            _ => Err(RpcFault::Malformed),
        }
    }

    /// Whether this confirmation satisfies `commitment`.
    #[must_use]
    pub const fn reaches(self, commitment: Commitment) -> bool {
        match commitment {
            Commitment::Confirmed => matches!(self, Self::Confirmed | Self::Finalized),
            Commitment::Finalized => matches!(self, Self::Finalized),
        }
    }
}

/// One entry of `getSignaturesForAddress`.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct SignatureInfo {
    pub signature: [u8; 64],
    pub slot: u64,
    /// The transaction executed and failed; it moved nothing.
    pub failed: bool,
}

/// One executed instruction with its program and accounts resolved to keys.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Instruction {
    pub program: [u8; 32],
    pub accounts: Vec<[u8; 32]>,
    pub data: Vec<u8>,
}

/// A transaction as `getTransaction` returns it in `json` encoding.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SolanaTransaction {
    pub slot: u64,
    pub signatures: Vec<[u8; 64]>,
    /// The static keys followed by the writable and read-only keys loaded
    /// from address lookup tables, the order instruction indices refer to.
    pub account_keys: Vec<[u8; 32]>,
    pub recent_blockhash: [u8; 32],
    /// Every instruction in execution order: each top-level instruction
    /// followed by the instructions it invoked.
    pub instructions: Vec<Instruction>,
    pub log_messages: Vec<String>,
    pub failed: bool,
}

/// An account as `getAccountInfo` returns it.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AccountData {
    pub owner: [u8; 32],
    pub lamports: u64,
    pub executable: bool,
    pub data: Vec<u8>,
}

/// A blockhash to build a transaction against and the last block height at
/// which a transaction built against it can still land.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Blockhash {
    pub blockhash: [u8; 32],
    pub last_valid_block_height: u64,
}

/// One entry of `getSignatureStatuses`.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct SignatureStatus {
    pub slot: u64,
    pub failed: bool,
    pub confirmation: Confirmation,
}

/// The Solana cluster, spoken to through one [`JsonRpc`] transport.
pub struct SolanaRpc {
    transport: Box<dyn JsonRpc>,
}

fn field<'a>(value: &'a Value, name: &str) -> Result<&'a Value, RpcFault> {
    value.get(name).ok_or(RpcFault::Malformed)
}

fn number(value: &Value, name: &str) -> Result<u64, RpcFault> {
    field(value, name)?.as_u64().ok_or(RpcFault::Malformed)
}

fn text<'a>(value: &'a Value, name: &str) -> Result<&'a str, RpcFault> {
    field(value, name)?.as_str().ok_or(RpcFault::Malformed)
}

fn list<'a>(value: &'a Value, name: &str) -> Result<&'a Vec<Value>, RpcFault> {
    field(value, name)?.as_array().ok_or(RpcFault::Malformed)
}

fn key_of(value: &Value) -> Result<[u8; 32], RpcFault> {
    value
        .as_str()
        .and_then(|text| base58_fixed::<32>(text).ok())
        .ok_or(RpcFault::Malformed)
}

fn signature_of(value: &Value) -> Result<[u8; 64], RpcFault> {
    value
        .as_str()
        .and_then(|text| base58_fixed::<64>(text).ok())
        .ok_or(RpcFault::Malformed)
}

/// `err` is `null` for a transaction that succeeded and an object or string
/// naming the failure otherwise; its absence is malformed.
fn failed(value: &Value) -> Result<bool, RpcFault> {
    Ok(!field(value, "err")?.is_null())
}

/// The `value` of a `{context, value}` response.
fn contextual(value: &Value) -> Result<&Value, RpcFault> {
    field(value, "context")?;
    field(value, "value")
}

fn instruction(value: &Value, keys: &[[u8; 32]]) -> Result<Instruction, RpcFault> {
    let resolve = |index: &Value| -> Result<[u8; 32], RpcFault> {
        index
            .as_u64()
            .and_then(|index| usize::try_from(index).ok())
            .and_then(|index| keys.get(index).copied())
            .ok_or(RpcFault::Malformed)
    };
    Ok(Instruction {
        program: resolve(field(value, "programIdIndex")?)?,
        accounts: list(value, "accounts")?
            .iter()
            .map(resolve)
            .collect::<Result<_, _>>()?,
        data: base58_data(text(value, "data")?).map_err(|_| RpcFault::Malformed)?,
    })
}

fn parse_transaction(value: &Value) -> Result<SolanaTransaction, RpcFault> {
    let meta = field(value, "meta")?;
    let transaction = field(value, "transaction")?;
    let message = field(transaction, "message")?;
    let mut account_keys: Vec<[u8; 32]> = list(message, "accountKeys")?
        .iter()
        .map(key_of)
        .collect::<Result<_, _>>()?;
    if let Some(loaded) = meta
        .get("loadedAddresses")
        .filter(|loaded| !loaded.is_null())
    {
        for group in ["writable", "readonly"] {
            for key in list(loaded, group)? {
                account_keys.push(key_of(key)?);
            }
        }
    }
    let top_level = list(message, "instructions")?;
    let mut inner: Vec<Vec<Instruction>> = vec![Vec::new(); top_level.len()];
    if let Some(groups) = meta
        .get("innerInstructions")
        .filter(|groups| !groups.is_null())
    {
        for group in groups.as_array().ok_or(RpcFault::Malformed)? {
            let index =
                usize::try_from(number(group, "index")?).map_err(|_| RpcFault::Malformed)?;
            let slot = inner.get_mut(index).ok_or(RpcFault::Malformed)?;
            if !slot.is_empty() {
                return Err(RpcFault::Malformed);
            }
            for value in list(group, "instructions")? {
                slot.push(instruction(value, &account_keys)?);
            }
        }
    }
    let mut instructions = Vec::new();
    for (value, invoked) in top_level.iter().zip(inner) {
        instructions.push(instruction(value, &account_keys)?);
        instructions.extend(invoked);
    }
    let log_messages = list(meta, "logMessages")?
        .iter()
        .map(|line| line.as_str().map(str::to_owned).ok_or(RpcFault::Malformed))
        .collect::<Result<_, _>>()?;
    Ok(SolanaTransaction {
        slot: number(value, "slot")?,
        signatures: list(transaction, "signatures")?
            .iter()
            .map(signature_of)
            .collect::<Result<_, _>>()?,
        account_keys,
        recent_blockhash: key_of(field(message, "recentBlockhash")?)?,
        instructions,
        log_messages,
        failed: failed(meta)?,
    })
}

impl SolanaRpc {
    #[must_use]
    pub fn new(transport: Box<dyn JsonRpc>) -> Self {
        Self { transport }
    }

    fn call(&self, method: &str, params: Value) -> Result<Value, RpcFault> {
        self.transport.call(method, params)
    }

    /// The highest slot the cluster has reached at `commitment`.
    ///
    /// # Errors
    ///
    /// Returns the transport's fault, or `Malformed` for a non-integer slot.
    pub fn get_slot(&self, commitment: Commitment) -> Result<u64, RpcFault> {
        self.call("getSlot", json!([{"commitment": commitment.as_str()}]))?
            .as_u64()
            .ok_or(RpcFault::Malformed)
    }

    /// One page of the signatures that touched `address`, newest first,
    /// starting below `before` when it is given.
    ///
    /// # Errors
    ///
    /// Refuses a page size outside `1..=1000` as `Configuration`, and returns
    /// `Malformed` for an oversized page or an unreadable entry.
    pub fn get_signatures_for_address(
        &self,
        address: &[u8; 32],
        before: Option<&[u8; 64]>,
        limit: usize,
        commitment: Commitment,
    ) -> Result<Vec<SignatureInfo>, RpcFault> {
        if limit == 0 || limit > MAX_SIGNATURE_PAGE {
            return Err(RpcFault::Configuration);
        }
        let mut options = Map::new();
        options.insert("commitment".to_owned(), json!(commitment.as_str()));
        options.insert("limit".to_owned(), json!(limit));
        if let Some(before) = before {
            options.insert("before".to_owned(), json!(base58_encode(before)));
        }
        let page = self.call(
            "getSignaturesForAddress",
            json!([base58_encode(address), Value::Object(options)]),
        )?;
        let entries = page.as_array().ok_or(RpcFault::Malformed)?;
        if entries.len() > limit {
            return Err(RpcFault::Malformed);
        }
        entries
            .iter()
            .map(|entry| {
                Ok(SignatureInfo {
                    signature: signature_of(field(entry, "signature")?)?,
                    slot: number(entry, "slot")?,
                    failed: failed(entry)?,
                })
            })
            .collect()
    }

    /// The transaction `signature` names, or `None` when the cluster does not
    /// know it at `commitment`.
    ///
    /// # Errors
    ///
    /// Returns the transport's fault, or `Malformed` for a transaction that
    /// cannot be read in full.
    pub fn get_transaction(
        &self,
        signature: &[u8; 64],
        commitment: Commitment,
    ) -> Result<Option<SolanaTransaction>, RpcFault> {
        let value = self.call(
            "getTransaction",
            json!([
                base58_encode(signature),
                {
                    "commitment": commitment.as_str(),
                    "encoding": "json",
                    "maxSupportedTransactionVersion": 0
                }
            ]),
        )?;
        if value.is_null() {
            return Ok(None);
        }
        parse_transaction(&value).map(Some)
    }

    /// The account at `address`, or `None` when it does not exist.
    ///
    /// # Errors
    ///
    /// Returns the transport's fault, or `Malformed` for data that is not
    /// base64.
    pub fn get_account_info(
        &self,
        address: &[u8; 32],
        commitment: Commitment,
    ) -> Result<Option<AccountData>, RpcFault> {
        let response = self.call(
            "getAccountInfo",
            json!([
                base58_encode(address),
                {"commitment": commitment.as_str(), "encoding": "base64"}
            ]),
        )?;
        let value = contextual(&response)?;
        if value.is_null() {
            return Ok(None);
        }
        let data = list(value, "data")?;
        let [encoded, Value::String(encoding)] = data.as_slice() else {
            return Err(RpcFault::Malformed);
        };
        if encoding != "base64" {
            return Err(RpcFault::Malformed);
        }
        Ok(Some(AccountData {
            owner: key_of(field(value, "owner")?)?,
            lamports: number(value, "lamports")?,
            executable: field(value, "executable")?
                .as_bool()
                .ok_or(RpcFault::Malformed)?,
            data: encoded
                .as_str()
                .and_then(|text| base64_decode(text).ok())
                .ok_or(RpcFault::Malformed)?,
        }))
    }

    /// The latest blockhash at `commitment`.
    ///
    /// # Errors
    ///
    /// Returns the transport's fault, or `Malformed` for an unreadable answer.
    pub fn get_latest_blockhash(&self, commitment: Commitment) -> Result<Blockhash, RpcFault> {
        let response = self.call(
            "getLatestBlockhash",
            json!([{"commitment": commitment.as_str()}]),
        )?;
        let value = contextual(&response)?;
        Ok(Blockhash {
            blockhash: key_of(field(value, "blockhash")?)?,
            last_valid_block_height: number(value, "lastValidBlockHeight")?,
        })
    }

    /// Broadcasts already signed wire bytes whose first signature is
    /// `signature`. `Unknown` is never permission to sign a replacement: the
    /// same bytes are rebroadcast.
    ///
    /// # Errors
    ///
    /// Refuses an empty or oversized transaction as `Configuration`, returns
    /// `Rejected` when the cluster deterministically refuses it and
    /// `Malformed` when it acknowledges a different signature.
    pub fn send_transaction(
        &self,
        raw: &[u8],
        signature: &[u8; 64],
    ) -> Result<SendOutcome, RpcFault> {
        if raw.is_empty() || raw.len() > MAX_TRANSACTION_BYTES {
            return Err(RpcFault::Configuration);
        }
        match self.call(
            "sendTransaction",
            json!([base64_encode(raw), {"encoding": "base64"}]),
        ) {
            Ok(value) if value.as_str() == Some(base58_encode(signature).as_str()) => {
                Ok(SendOutcome::Accepted)
            }
            Ok(_) => Err(RpcFault::Malformed),
            Err(error @ (RpcFault::Rejected { .. } | RpcFault::Configuration)) => Err(error),
            Err(_) => Ok(SendOutcome::Unknown),
        }
    }

    /// The status of each signature, `None` where the cluster has no record.
    ///
    /// # Errors
    ///
    /// Refuses an empty or oversized request as `Configuration`, and returns
    /// `Malformed` when the answer does not carry one entry per signature.
    pub fn get_signature_statuses(
        &self,
        signatures: &[[u8; 64]],
    ) -> Result<Vec<Option<SignatureStatus>>, RpcFault> {
        if signatures.is_empty() || signatures.len() > 256 {
            return Err(RpcFault::Configuration);
        }
        let encoded: Vec<String> = signatures
            .iter()
            .map(|signature| base58_encode(signature))
            .collect();
        let response = self.call(
            "getSignatureStatuses",
            json!([encoded, {"searchTransactionHistory": true}]),
        )?;
        let values = contextual(&response)?
            .as_array()
            .ok_or(RpcFault::Malformed)?;
        if values.len() != signatures.len() {
            return Err(RpcFault::Malformed);
        }
        values
            .iter()
            .map(|value| {
                if value.is_null() {
                    return Ok(None);
                }
                Ok(Some(SignatureStatus {
                    slot: number(value, "slot")?,
                    failed: failed(value)?,
                    confirmation: Confirmation::parse(field(value, "confirmationStatus")?)?,
                }))
            })
            .collect()
    }
}
