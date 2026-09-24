//! Blockscout rows and their conversion into the EVM JSON-RPC shapes the
//! live Paxeer path feeds [`crate::paxeer::decode_block`].
//!
//! The row structs mirror the Blockscout Postgres columns one to one
//! (`bytea` columns as bytes, `numeric` columns as their decimal text), so
//! the conversion is exercised from plain rows without a database. A block
//! becomes an `eth_getBlockByNumber(.., true)` object and each of its
//! transactions an `eth_getTransactionReceipt` object carrying its logs.
//! Token transfers, internal transactions and tokens produce no rows of
//! their own on the live path, so they are cross-checked against the
//! blocks, transactions and logs they reference instead.

use std::collections::{BTreeMap, BTreeSet};

use serde::Deserialize;
use serde_json::{json, Value};

use crate::codec::{hex0x, to_quantity};
use crate::paxeer::erc20_transfer_topic;
use crate::IndexError;

mod bytes_hex {
    use serde::{Deserialize, Deserializer};

    pub fn deserialize<'de, D: Deserializer<'de>>(deserializer: D) -> Result<Vec<u8>, D::Error> {
        let text = String::deserialize(deserializer)?;
        crate::codec::unhex(&text).map_err(serde::de::Error::custom)
    }
}

mod option_bytes_hex {
    use serde::{Deserialize, Deserializer};

    pub fn deserialize<'de, D: Deserializer<'de>>(
        deserializer: D,
    ) -> Result<Option<Vec<u8>>, D::Error> {
        Option::<String>::deserialize(deserializer)?
            .map(|text| crate::codec::unhex(&text).map_err(serde::de::Error::custom))
            .transpose()
    }
}

/// One `blocks` row (consensus blocks only).
#[derive(Clone, Debug, Deserialize, Eq, PartialEq)]
pub struct BlockRow {
    pub number: u64,
    #[serde(with = "bytes_hex")]
    pub hash: Vec<u8>,
    #[serde(with = "bytes_hex")]
    pub parent_hash: Vec<u8>,
    /// Seconds since the Unix epoch.
    pub timestamp: u64,
    #[serde(with = "bytes_hex")]
    pub miner_hash: Vec<u8>,
    /// `numeric`, as decimal text.
    pub gas_used: String,
}

/// One `transactions` row.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq)]
pub struct TransactionRow {
    #[serde(with = "bytes_hex")]
    pub hash: Vec<u8>,
    pub block_number: u64,
    pub index: u64,
    #[serde(with = "bytes_hex")]
    pub from_address_hash: Vec<u8>,
    #[serde(default, with = "option_bytes_hex")]
    pub to_address_hash: Option<Vec<u8>>,
    /// `numeric` wei, as decimal text.
    pub value: String,
    pub gas_used: Option<String>,
    /// `1` success, `0` failure, `NULL` when Blockscout has no receipt.
    pub status: Option<i64>,
    #[serde(default, with = "option_bytes_hex")]
    pub input: Option<Vec<u8>>,
    #[serde(default, with = "option_bytes_hex")]
    pub created_contract_address_hash: Option<Vec<u8>>,
}

/// One `logs` row. `index` is the block-wide log index.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq)]
pub struct LogRow {
    #[serde(with = "bytes_hex")]
    pub transaction_hash: Vec<u8>,
    pub index: u64,
    #[serde(with = "bytes_hex")]
    pub address_hash: Vec<u8>,
    #[serde(default, with = "option_bytes_hex")]
    pub first_topic: Option<Vec<u8>>,
    #[serde(default, with = "option_bytes_hex")]
    pub second_topic: Option<Vec<u8>>,
    #[serde(default, with = "option_bytes_hex")]
    pub third_topic: Option<Vec<u8>>,
    #[serde(default, with = "option_bytes_hex")]
    pub fourth_topic: Option<Vec<u8>>,
    #[serde(with = "bytes_hex")]
    pub data: Vec<u8>,
    pub block_number: u64,
}

/// One `token_transfers` row.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq)]
pub struct TokenTransferRow {
    #[serde(with = "bytes_hex")]
    pub transaction_hash: Vec<u8>,
    pub log_index: u64,
    #[serde(default, with = "option_bytes_hex")]
    pub from_address_hash: Option<Vec<u8>>,
    #[serde(default, with = "option_bytes_hex")]
    pub to_address_hash: Option<Vec<u8>>,
    /// `numeric`, as decimal text; `NULL` for token ids without amounts.
    pub amount: Option<String>,
    #[serde(with = "bytes_hex")]
    pub token_contract_address_hash: Vec<u8>,
    pub token_type: Option<String>,
    pub block_number: u64,
}

/// One `internal_transactions` row. This Blockscout schema keys internal
/// transactions by `(block_number, transaction_index, index)` and carries
/// no transaction hash.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq)]
pub struct InternalTransactionRow {
    pub block_number: u64,
    pub transaction_index: u64,
    pub index: u64,
    #[serde(default, with = "option_bytes_hex")]
    pub from_address_hash: Option<Vec<u8>>,
    #[serde(default, with = "option_bytes_hex")]
    pub to_address_hash: Option<Vec<u8>>,
    pub value: Option<String>,
    pub call_type: Option<String>,
    #[serde(rename = "type")]
    pub kind: String,
}

/// One `tokens` row.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq)]
pub struct TokenRow {
    #[serde(with = "bytes_hex")]
    pub contract_address_hash: Vec<u8>,
    pub symbol: Option<String>,
    pub name: Option<String>,
    pub decimals: Option<String>,
    #[serde(rename = "type")]
    pub kind: String,
}

/// Every Blockscout row of one height range.
#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq)]
pub struct BlockscoutRange {
    #[serde(default)]
    pub blocks: Vec<BlockRow>,
    #[serde(default)]
    pub transactions: Vec<TransactionRow>,
    #[serde(default)]
    pub logs: Vec<LogRow>,
    #[serde(default)]
    pub token_transfers: Vec<TokenTransferRow>,
    #[serde(default)]
    pub internal_transactions: Vec<InternalTransactionRow>,
    #[serde(default)]
    pub tokens: Vec<TokenRow>,
}

/// One block in the live path's JSON-RPC shapes.
#[derive(Clone, Debug, PartialEq)]
pub struct RpcBlock {
    /// The `eth_getBlockByNumber(.., true)` object.
    pub block: Value,
    /// Transaction hash to its `eth_getTransactionReceipt` object.
    pub receipts: BTreeMap<String, Value>,
    /// The transaction hashes in block order.
    pub transaction_hashes: Vec<String>,
}

fn fixed<'a>(bytes: &'a [u8], width: usize, what: &str) -> Result<&'a [u8], IndexError> {
    if bytes.len() == width {
        Ok(bytes)
    } else {
        Err(IndexError::Decode(format!(
            "paxscan {what} is {} bytes, expected {width}",
            bytes.len()
        )))
    }
}

fn hash32(bytes: &[u8], what: &str) -> Result<String, IndexError> {
    Ok(hex0x(fixed(bytes, 32, what)?))
}

fn address20(bytes: &[u8], what: &str) -> Result<String, IndexError> {
    Ok(hex0x(fixed(bytes, 20, what)?))
}

fn optional_address(bytes: Option<&Vec<u8>>, what: &str) -> Result<Value, IndexError> {
    bytes.map_or(Ok(Value::Null), |bytes| {
        address20(bytes, what).map(Value::String)
    })
}

/// Parses an unsigned decimal of at most 256 bits into a big-endian word.
///
/// # Errors
/// Refuses empty text, non-digits and values wider than 256 bits.
pub fn decimal_word(text: &str) -> Result<[u8; 32], IndexError> {
    if text.is_empty() || !text.bytes().all(|byte| byte.is_ascii_digit()) {
        return Err(IndexError::Decode(format!(
            "paxscan numeric {text} is not an unsigned integer"
        )));
    }
    let mut word = [0_u8; 32];
    for digit in text.bytes() {
        let mut carry = u32::from(digit - b'0');
        for byte in word.iter_mut().rev() {
            let next = u32::from(*byte) * 10 + carry;
            *byte = u8::try_from(next & 0xff).unwrap_or(0);
            carry = next >> 8;
        }
        if carry != 0 {
            return Err(IndexError::Decode(format!(
                "paxscan numeric {text} exceeds 256 bits"
            )));
        }
    }
    Ok(word)
}

/// Renders an unsigned decimal as a canonical JSON-RPC quantity.
///
/// # Errors
/// As [`decimal_word`].
pub fn decimal_quantity(text: &str) -> Result<String, IndexError> {
    let word = decimal_word(text)?;
    let digits = crate::codec::hex(&word);
    let trimmed = digits.trim_start_matches('0');
    Ok(format!(
        "0x{}",
        if trimmed.is_empty() { "0" } else { trimmed }
    ))
}

fn topics(log: &LogRow) -> Result<Vec<Vec<u8>>, IndexError> {
    let mut topics = Vec::new();
    let mut ended = false;
    for topic in [
        &log.first_topic,
        &log.second_topic,
        &log.third_topic,
        &log.fourth_topic,
    ] {
        match topic {
            Some(topic) if !ended => {
                fixed(topic, 32, "log topic")?;
                topics.push(topic.clone());
            }
            Some(_) => {
                return Err(IndexError::Decode(format!(
                    "paxscan log {} at {} has a topic after an empty one",
                    log.index, log.block_number
                )))
            }
            None => ended = true,
        }
    }
    Ok(topics)
}

/// Converts every block of `range` into the live path's JSON-RPC shapes,
/// keyed by height, after checking that every transaction, log, token
/// transfer, internal transaction and token row is consistent with the
/// blocks it belongs to.
///
/// # Errors
/// Refuses malformed columns, a transaction without receipt status, and
/// any row that references a block, transaction or log absent from the
/// range.
#[allow(clippy::too_many_lines)]
pub fn rpc_blocks(range: &BlockscoutRange) -> Result<BTreeMap<u64, RpcBlock>, IndexError> {
    let mut hashes: BTreeMap<u64, String> = BTreeMap::new();
    for block in &range.blocks {
        let hash = hash32(&block.hash, "block hash")?;
        if hashes.insert(block.number, hash).is_some() {
            return Err(IndexError::Integrity(format!(
                "paxscan has two consensus blocks at {}",
                block.number
            )));
        }
    }
    let mut transactions: BTreeMap<u64, BTreeMap<u64, &TransactionRow>> = BTreeMap::new();
    let mut transaction_heights: BTreeMap<String, (u64, u64)> = BTreeMap::new();
    for transaction in &range.transactions {
        let hash = hash32(&transaction.hash, "transaction hash")?;
        if !hashes.contains_key(&transaction.block_number) {
            return Err(IndexError::Integrity(format!(
                "paxscan transaction {hash} names block {} outside the range",
                transaction.block_number
            )));
        }
        if transactions
            .entry(transaction.block_number)
            .or_default()
            .insert(transaction.index, transaction)
            .is_some()
        {
            return Err(IndexError::Integrity(format!(
                "paxscan has two transactions at index {} of block {}",
                transaction.index, transaction.block_number
            )));
        }
        transaction_heights.insert(hash, (transaction.block_number, transaction.index));
    }
    let mut logs: BTreeMap<String, Vec<&LogRow>> = BTreeMap::new();
    let mut log_keys: BTreeMap<(String, u64), &LogRow> = BTreeMap::new();
    let mut block_log_indexes: BTreeSet<(u64, u64)> = BTreeSet::new();
    for log in &range.logs {
        let tx_hash = hash32(&log.transaction_hash, "log transaction hash")?;
        match transaction_heights.get(&tx_hash) {
            Some((height, _)) if *height == log.block_number => {}
            _ => {
                return Err(IndexError::Integrity(format!(
                    "paxscan log {} at {} names transaction {tx_hash} outside its block",
                    log.index, log.block_number
                )))
            }
        }
        if !block_log_indexes.insert((log.block_number, log.index)) {
            return Err(IndexError::Integrity(format!(
                "paxscan has two logs at index {} of block {}",
                log.index, log.block_number
            )));
        }
        log_keys.insert((tx_hash.clone(), log.index), log);
        logs.entry(tx_hash).or_default().push(log);
    }
    let token_kinds: BTreeMap<Vec<u8>, &str> = range
        .tokens
        .iter()
        .map(|token| (token.contract_address_hash.clone(), token.kind.as_str()))
        .collect();
    for transfer in &range.token_transfers {
        let tx_hash = hash32(
            &transfer.transaction_hash,
            "token transfer transaction hash",
        )?;
        let log = log_keys
            .get(&(tx_hash.clone(), transfer.log_index))
            .ok_or_else(|| {
                IndexError::Integrity(format!(
                    "paxscan token transfer {tx_hash}/{} at {} has no log",
                    transfer.log_index, transfer.block_number
                ))
            })?;
        if log.address_hash != transfer.token_contract_address_hash
            || log.block_number != transfer.block_number
        {
            return Err(IndexError::Integrity(format!(
                "paxscan token transfer {tx_hash}/{} disagrees with its log",
                transfer.log_index
            )));
        }
        if let (Some(row_kind), Some(token_kind)) = (
            transfer.token_type.as_deref(),
            token_kinds.get(&transfer.token_contract_address_hash),
        ) {
            if row_kind != *token_kind {
                return Err(IndexError::Integrity(format!(
                    "paxscan token transfer {tx_hash}/{} is {row_kind} but its token is {token_kind}",
                    transfer.log_index
                )));
            }
        }
        let log_topics = topics(log)?;
        if transfer.token_type.as_deref() == Some("ERC-20")
            && log_topics.len() == 3
            && log_topics[0] == erc20_transfer_topic()
        {
            let amount = transfer.amount.as_deref().map(decimal_word).transpose()?;
            let from = &log_topics[1][12..];
            let to = &log_topics[2][12..];
            if amount.as_ref().map(<[u8; 32]>::as_slice) != Some(log.data.as_slice())
                || transfer.from_address_hash.as_deref() != Some(from)
                || transfer.to_address_hash.as_deref() != Some(to)
            {
                return Err(IndexError::Integrity(format!(
                    "paxscan ERC-20 transfer {tx_hash}/{} disagrees with its log",
                    transfer.log_index
                )));
            }
        }
    }
    for internal in &range.internal_transactions {
        let parent = transactions
            .get(&internal.block_number)
            .and_then(|block| block.get(&internal.transaction_index));
        if parent.is_none() {
            return Err(IndexError::Integrity(format!(
                "paxscan internal transaction {}/{}/{} has no transaction",
                internal.block_number, internal.transaction_index, internal.index
            )));
        }
        if let Some(value) = &internal.value {
            decimal_word(value)?;
        }
    }
    let mut converted = BTreeMap::new();
    for block in &range.blocks {
        let height = block.number;
        let block_hash = hash32(&block.hash, "block hash")?;
        let number = to_quantity(height);
        let mut objects = Vec::new();
        let mut receipts = BTreeMap::new();
        let mut transaction_hashes = Vec::new();
        let rows = transactions.get(&height);
        for (position, (index, transaction)) in rows.into_iter().flatten().enumerate() {
            if u64::try_from(position).ok() != Some(*index) {
                return Err(IndexError::Integrity(format!(
                    "paxscan block {height} is missing transaction index {position}"
                )));
            }
            let hash = hash32(&transaction.hash, "transaction hash")?;
            let from = address20(&transaction.from_address_hash, "transaction sender")?;
            let to = optional_address(
                transaction.to_address_hash.as_ref(),
                "transaction recipient",
            )?;
            let status = match transaction.status {
                Some(1) => "0x1",
                Some(0) => "0x0",
                Some(other) => {
                    return Err(IndexError::Decode(format!(
                        "paxscan transaction {hash} has status {other}"
                    )))
                }
                None => {
                    return Err(IndexError::Source(format!(
                        "paxscan transaction {hash} at {height} has no receipt status"
                    )))
                }
            };
            let transaction_index = to_quantity(*index);
            objects.push(json!({
                "hash": hash,
                "blockHash": block_hash,
                "blockNumber": number,
                "transactionIndex": transaction_index,
                "from": from,
                "to": to,
                "value": decimal_quantity(&transaction.value)?,
                "input": hex0x(transaction.input.as_deref().unwrap_or_default()),
            }));
            let mut receipt_logs = Vec::new();
            for log in logs.get(&hash).map_or(&[][..], Vec::as_slice) {
                receipt_logs.push(json!({
                    "address": address20(&log.address_hash, "log address")?,
                    "topics": topics(log)?.iter().map(|topic| hex0x(topic)).collect::<Vec<_>>(),
                    "data": hex0x(&log.data),
                    "blockNumber": number,
                    "blockHash": block_hash,
                    "transactionHash": hash,
                    "transactionIndex": transaction_index,
                    "logIndex": to_quantity(log.index),
                    "removed": false,
                }));
            }
            receipts.insert(
                hash.clone(),
                json!({
                    "transactionHash": hash,
                    "transactionIndex": transaction_index,
                    "blockHash": block_hash,
                    "blockNumber": number,
                    "from": from,
                    "to": to,
                    "gasUsed": transaction
                        .gas_used
                        .as_deref()
                        .map(decimal_quantity)
                        .transpose()?,
                    "contractAddress": optional_address(
                        transaction.created_contract_address_hash.as_ref(),
                        "created contract",
                    )?,
                    "status": status,
                    "logs": receipt_logs,
                }),
            );
            transaction_hashes.push(hash);
        }
        converted.insert(
            height,
            RpcBlock {
                block: json!({
                    "number": number,
                    "hash": block_hash,
                    "parentHash": hash32(&block.parent_hash, "parent hash")?,
                    "timestamp": to_quantity(block.timestamp),
                    "miner": address20(&block.miner_hash, "miner")?,
                    "gasUsed": decimal_quantity(&block.gas_used)?,
                    "transactions": objects,
                }),
                receipts,
                transaction_hashes,
            },
        );
    }
    Ok(converted)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decimals_become_canonical_quantities() {
        assert_eq!(decimal_quantity("0").unwrap_or_default(), "0x0");
        assert_eq!(
            decimal_quantity("1000000000000000000").unwrap_or_default(),
            "0xde0b6b3a7640000"
        );
        let max = "115792089237316195423570985008687907853269984665640564039457584007913129639935";
        assert_eq!(
            decimal_quantity(max).unwrap_or_default(),
            format!("0x{}", "f".repeat(64))
        );
        assert!(decimal_quantity(&format!("{max}0")).is_err());
        assert!(decimal_quantity("1.5").is_err());
        assert!(decimal_quantity("").is_err());
    }
}
