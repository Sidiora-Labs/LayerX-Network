//! The Paxeer half: walks EVM blocks with their receipts and logs, plus the
//! CometBFT `tx_search` page for the same heights, and decodes them into
//! transfers and events.
//!
//! EVM: native value transfers, ERC-20 `Transfer` (a registered pointer
//! contract's transfer is labelled `pointer_transfer`), and every event a
//! loaded precompile ABI declares; custody deposits, releases and emergency
//! exits also become transfer legs. Cosmos: bank `transfer`, tokenfactory
//! `create_denom`/`mint`/`burn`/`change_admin`, and every `layerx_*` anchor
//! string event. Cosmos transactions that carry EVM execution are skipped
//! there because the EVM walk already indexed them.

use std::collections::BTreeMap;
use std::sync::atomic::{AtomicBool, Ordering};

use serde_json::{json, Map, Value};

use crate::abi::{address_text, bare_bytes32, keccak, AbiRegistry, DecodedEvent};
use crate::codec::{
    base64_decode, be_decimal, hex, hex0x, is_zero, quantity_u64, quantity_word, to_quantity,
    unhex, unhex_fixed,
};
use crate::follow::{walk_back, FollowPolicy, StepOutcome};
use crate::store::{AssetRow, EventRow, Store, TransferRow, Unit};
use crate::transport::Endpoint;
use crate::IndexError;

/// The chain label of every Paxeer row.
pub const CHAIN: &str = "paxeer";

/// The asset label of the EVM chain's native coin.
pub const NATIVE_ASSET: &str = "evm:native";

const TX_SEARCH_PAGE: u64 = 100;

/// The JSON-RPC error `data` CometBFT answers `tx_search` with when the
/// node runs with its transaction index off.
pub const TX_INDEX_DISABLED: &str = "transaction searching is disabled due to no kvEventSink";

/// How CometBFT renders event attribute keys and values.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum AttributeEncoding {
    /// `[]byte` attributes, JSON base64 (this chain's ABCI types).
    Base64,
    /// Plain strings (CometBFT 0.37+ style).
    Plain,
}

/// The `Transfer(address,address,uint256)` topic.
#[must_use]
pub fn erc20_transfer_topic() -> [u8; 32] {
    keccak(b"Transfer(address,address,uint256)")
}

fn field<'a>(value: &'a Value, name: &str) -> Result<&'a Value, IndexError> {
    value
        .get(name)
        .ok_or_else(|| IndexError::Decode(format!("field {name} is missing")))
}

fn field_str<'a>(value: &'a Value, name: &str) -> Result<&'a str, IndexError> {
    field(value, name)?
        .as_str()
        .ok_or_else(|| IndexError::Decode(format!("field {name} is not text")))
}

fn hash_text(value: &Value, name: &str) -> Result<String, IndexError> {
    Ok(hex0x(&unhex_fixed::<32>(field_str(value, name)?)?))
}

fn address_of(value: &Value, name: &str) -> Result<Option<String>, IndexError> {
    match value.get(name) {
        None | Some(Value::Null) => Ok(None),
        Some(Value::String(text)) => Ok(Some(address_text(unhex_fixed::<20>(text)?))),
        Some(_) => Err(IndexError::Decode(format!("{name} is not an address"))),
    }
}

/// Everything a block decode needs besides the block itself.
pub struct BlockInputs<'a> {
    pub registry: &'a AbiRegistry,
    /// Transaction hash to its receipt.
    pub receipts: &'a BTreeMap<String, Value>,
    /// The `tx_search` results for this block's height.
    pub cosmos_txs: &'a [Value],
    pub encoding: AttributeEncoding,
    /// Answers whether an emitter address is a registered pointer contract.
    pub is_pointer: &'a dyn Fn(&str) -> Result<bool, IndexError>,
}

/// Decodes one full EVM block (with transaction objects) into a unit.
///
/// # Errors
/// Refuses a malformed block, a missing receipt, or a receipt for another
/// block.
pub fn decode_block(block: &Value, inputs: &BlockInputs<'_>) -> Result<Unit, IndexError> {
    let height = quantity_u64(field_str(block, "number")?)?;
    let hash = hash_text(block, "hash")?;
    let parent = hash_text(block, "parentHash")?;
    let mut unit = Unit {
        chain: CHAIN.to_owned(),
        position: height,
        hash: hash.clone(),
        parent,
        link: hash.clone(),
        boundary: height,
        ..Unit::default()
    };
    let transactions = field(block, "transactions")?
        .as_array()
        .ok_or_else(|| IndexError::Decode("block transactions are not an array".to_owned()))?;
    for transaction in transactions {
        if !transaction.is_object() {
            return Err(IndexError::Decode(
                "block was fetched without transaction objects".to_owned(),
            ));
        }
        let tx_hash = hash_text(transaction, "hash")?;
        let receipt = inputs
            .receipts
            .get(&tx_hash)
            .ok_or_else(|| IndexError::Source(format!("receipt of {tx_hash} is missing")))?;
        if hash_text(receipt, "blockHash")? != hash {
            return Err(IndexError::Integrity(format!(
                "receipt of {tx_hash} belongs to another block"
            )));
        }
        if field_str(receipt, "status")? != "0x1" {
            continue;
        }
        let value = quantity_word(field_str(transaction, "value")?)?;
        let from = address_of(transaction, "from")?
            .ok_or_else(|| IndexError::Decode("transaction has no sender".to_owned()))?;
        if let (false, Some(to)) = (is_zero(&value), address_of(transaction, "to")?) {
            let amount = be_decimal(&value);
            let decoded = json!({ "block_number": height.to_string(), "value_wei": amount });
            unit.transfers.push(TransferRow {
                height_or_seq: height,
                kind: "native_transfer".to_owned(),
                direction: "out",
                account: from.clone(),
                counterparty: Some(to.clone()),
                asset: NATIVE_ASSET.to_owned(),
                amount: amount.clone(),
                tx_id: tx_hash.clone(),
                ordinal: 0,
                decoded: decoded.clone(),
            });
            unit.transfers.push(TransferRow {
                height_or_seq: height,
                kind: "native_transfer".to_owned(),
                direction: "in",
                account: to,
                counterparty: Some(from.clone()),
                asset: NATIVE_ASSET.to_owned(),
                amount,
                tx_id: tx_hash.clone(),
                ordinal: 0,
                decoded,
            });
            unit.assets.push(AssetRow {
                asset: NATIVE_ASSET.to_owned(),
                chain: CHAIN.to_owned(),
                kind: "native".to_owned(),
                address: None,
                denom: None,
                metadata: json!({}),
            });
        }
        let logs = field(receipt, "logs")?
            .as_array()
            .ok_or_else(|| IndexError::Decode("receipt logs are not an array".to_owned()))?;
        for log in logs {
            if log.get("removed").and_then(Value::as_bool) == Some(true) {
                continue;
            }
            decode_log(&mut unit, log, height, &tx_hash, inputs)?;
        }
    }
    for transaction in inputs.cosmos_txs {
        decode_cosmos_tx(&mut unit, transaction, inputs.encoding)?;
    }
    Ok(unit)
}

fn decode_log(
    unit: &mut Unit,
    log: &Value,
    height: u64,
    tx_hash: &str,
    inputs: &BlockInputs<'_>,
) -> Result<(), IndexError> {
    let emitter = unhex_fixed::<20>(field_str(log, "address")?)?;
    let emitter_text = address_text(emitter);
    let log_index = quantity_u64(field_str(log, "logIndex")?)?;
    let topics = field(log, "topics")?
        .as_array()
        .ok_or_else(|| IndexError::Decode("log topics are not an array".to_owned()))?
        .iter()
        .map(|topic| {
            topic
                .as_str()
                .ok_or_else(|| IndexError::Decode("topic is not text".to_owned()))
                .and_then(unhex_fixed::<32>)
        })
        .collect::<Result<Vec<_>, _>>()?;
    let data = unhex(field_str(log, "data")?)?;
    let ordinal = log_index + 1;
    if topics.len() == 3 && topics[0] == erc20_transfer_topic() && data.len() == 32 {
        if !is_zero(&topics[1][..12]) || !is_zero(&topics[2][..12]) {
            return Ok(());
        }
        let from = address_text(unhex_fixed::<20>(&hex(&topics[1][12..]))?);
        let to = address_text(unhex_fixed::<20>(&hex(&topics[2][12..]))?);
        let amount = be_decimal(&data);
        let pointer = (inputs.is_pointer)(&emitter_text)?;
        let kind = if pointer {
            "pointer_transfer"
        } else {
            "erc20_transfer"
        };
        let asset = format!("evm:{emitter_text}");
        let decoded = json!({
            "block_number": height.to_string(),
            "log_index": log_index.to_string(),
            "contract": emitter_text,
            "from": from,
            "to": to,
            "value": amount,
        });
        let zero = address_text([0; 20]);
        if from != zero {
            unit.transfers.push(TransferRow {
                height_or_seq: height,
                kind: kind.to_owned(),
                direction: "out",
                account: from.clone(),
                counterparty: (to != zero).then(|| to.clone()),
                asset: asset.clone(),
                amount: amount.clone(),
                tx_id: tx_hash.to_owned(),
                ordinal,
                decoded: decoded.clone(),
            });
        }
        if to != zero {
            unit.transfers.push(TransferRow {
                height_or_seq: height,
                kind: kind.to_owned(),
                direction: "in",
                account: to,
                counterparty: (from != zero).then_some(from),
                asset: asset.clone(),
                amount,
                tx_id: tx_hash.to_owned(),
                ordinal,
                decoded,
            });
        }
        unit.assets.push(AssetRow {
            asset,
            chain: CHAIN.to_owned(),
            kind: if pointer { "pointer" } else { "erc20" }.to_owned(),
            address: Some(emitter_text),
            denom: None,
            metadata: json!({}),
        });
        return Ok(());
    }
    let (event, decoded) = match inputs.registry.decode_log(emitter, &topics, &data) {
        Ok(Some((event, decoded))) => (event, decoded),
        Ok(None) => return Ok(()),
        Err(error) => {
            unit.events.push(EventRow {
                height_or_seq: height,
                source: "evm-log".to_owned(),
                name: "undecodable".to_owned(),
                contract: None,
                account: None,
                tx_id: tx_hash.to_owned(),
                ordinal,
                decoded: json!({
                    "address": emitter_text,
                    "error": error.to_string(),
                    "topics": topics.iter().map(|topic| hex0x(topic)).collect::<Vec<_>>(),
                    "data": hex0x(&data),
                }),
            });
            return Ok(());
        }
    };
    let account = decoded
        .first_address(&event.params)
        .or_else(|| decoded.arg("account").and_then(bare_bytes32))
        .or_else(|| decoded.arg("beneficiary").and_then(bare_bytes32));
    custody_legs(unit, &decoded, height, tx_hash, ordinal, &emitter_text);
    unit.events.push(EventRow {
        height_or_seq: height,
        source: "evm-log".to_owned(),
        name: decoded.name.clone(),
        contract: Some(decoded.contract.clone()),
        account,
        tx_id: tx_hash.to_owned(),
        ordinal,
        decoded: json!({
            "block_number": height.to_string(),
            "log_index": log_index.to_string(),
            "address": emitter_text,
            "contract": decoded.contract,
            "event": decoded.name,
            "signature": decoded.signature,
            "args": Value::Object(decoded.args.clone()),
        }),
    });
    Ok(())
}

fn custody_legs(
    unit: &mut Unit,
    decoded: &DecodedEvent,
    height: u64,
    tx_hash: &str,
    ordinal: u64,
    emitter: &str,
) {
    let text = |name: &str| decoded.arg(name).and_then(Value::as_str).map(str::to_owned);
    let bytes32 = |name: &str| decoded.arg(name).and_then(bare_bytes32);
    let legs: Vec<(&str, &'static str, String, Option<String>)> = match decoded.name.as_str() {
        "CustodyDeposit" => {
            let (Some(payer), Some(beneficiary)) = (text("payer"), bytes32("beneficiary")) else {
                return;
            };
            vec![
                (
                    "custody_deposit",
                    "out",
                    payer.clone(),
                    Some(beneficiary.clone()),
                ),
                ("custody_deposit", "in", beneficiary, Some(payer)),
            ]
        }
        "CustodyRelease" => {
            let Some(recipient) = text("recipient") else {
                return;
            };
            vec![("custody_release", "in", recipient, Some(emitter.to_owned()))]
        }
        "EmergencyExitExecuted" => {
            let Some(recipient) = text("recipient") else {
                return;
            };
            vec![("emergency_exit", "in", recipient, bytes32("account"))]
        }
        _ => return,
    };
    if decoded.contract != "layerxcustody" {
        return;
    }
    let (Some(asset), Some(amount)) = (bytes32("assetId"), text("amount")) else {
        return;
    };
    unit.assets.push(AssetRow {
        asset: asset.clone(),
        chain: crate::layerx::CHAIN.to_owned(),
        kind: "layerx".to_owned(),
        address: None,
        denom: None,
        metadata: json!({}),
    });
    for (kind, direction, account, counterparty) in legs {
        unit.transfers.push(TransferRow {
            height_or_seq: height,
            kind: kind.to_owned(),
            direction,
            account,
            counterparty,
            asset: asset.clone(),
            amount: amount.clone(),
            tx_id: tx_hash.to_owned(),
            ordinal,
            decoded: json!({
                "block_number": height.to_string(),
                "event": decoded.name,
                "args": Value::Object(decoded.args.clone()),
            }),
        });
    }
}

fn attribute_text(value: &Value, encoding: AttributeEncoding) -> Result<String, IndexError> {
    let raw = match value {
        Value::Null => return Ok(String::new()),
        Value::String(text) => text,
        _ => return Err(IndexError::Decode("attribute is not text".to_owned())),
    };
    match encoding {
        AttributeEncoding::Plain => Ok(raw.clone()),
        AttributeEncoding::Base64 => String::from_utf8(base64_decode(raw)?)
            .map_err(|_| IndexError::Decode("attribute is not UTF-8".to_owned())),
    }
}

/// Splits a Cosmos coin list (`10upax,5factory/x/y`) into amount and denom.
///
/// # Errors
/// Refuses an entry without digits or a malformed denom.
pub fn parse_coins(text: &str) -> Result<Vec<(String, String)>, IndexError> {
    if text.is_empty() {
        return Ok(Vec::new());
    }
    text.split(',')
        .map(|coin| {
            let split = coin
                .find(|character: char| !character.is_ascii_digit())
                .ok_or_else(|| IndexError::Decode(format!("coin {coin} has no denom")))?;
            let (amount, denom) = coin.split_at(split);
            let valid_denom = denom.len() >= 2
                && denom.len() <= 128
                && denom.starts_with(|character: char| character.is_ascii_alphabetic())
                && denom.bytes().all(|byte| {
                    byte.is_ascii_alphanumeric() || matches!(byte, b'/' | b':' | b'.' | b'_' | b'-')
                });
            if amount.is_empty() || (amount.len() > 1 && amount.starts_with('0')) || !valid_denom {
                return Err(IndexError::Decode(format!("coin {coin} is malformed")));
            }
            Ok((amount.to_owned(), denom.to_owned()))
        })
        .collect()
}

fn denom_asset(denom: &str) -> AssetRow {
    AssetRow {
        asset: format!("denom:{denom}"),
        chain: CHAIN.to_owned(),
        kind: if denom.starts_with("factory/") {
            "tokenfactory"
        } else {
            "bank"
        }
        .to_owned(),
        address: None,
        denom: Some(denom.to_owned()),
        metadata: json!({}),
    }
}

/// Decodes one `tx_search` result entry into the unit.
///
/// # Errors
/// Refuses a malformed entry or attribute encoding.
pub fn decode_cosmos_tx(
    unit: &mut Unit,
    transaction: &Value,
    encoding: AttributeEncoding,
) -> Result<(), IndexError> {
    let height: u64 = field_str(transaction, "height")?
        .parse()
        .map_err(|_| IndexError::Decode("tx height is not decimal".to_owned()))?;
    if height != unit.position {
        return Err(IndexError::Integrity(format!(
            "tx_search entry at {height} delivered with block {}",
            unit.position
        )));
    }
    let tx_hash = hex0x(&unhex_fixed::<32>(field_str(transaction, "hash")?)?);
    let result = field(transaction, "tx_result")?;
    if result.get("code").and_then(Value::as_u64).unwrap_or(0) != 0 {
        return Ok(());
    }
    if result
        .get("evm_tx_info")
        .is_some_and(|info| !info.is_null())
    {
        return Ok(());
    }
    let events = result
        .get("events")
        .and_then(Value::as_array)
        .map_or(&[][..], Vec::as_slice);
    for (event_index, event) in events.iter().enumerate() {
        let kind = field_str(event, "type")?;
        let mut attributes: Vec<(String, String)> = Vec::new();
        for attribute in event
            .get("attributes")
            .and_then(Value::as_array)
            .map_or(&[][..], Vec::as_slice)
        {
            attributes.push((
                attribute_text(attribute.get("key").unwrap_or(&Value::Null), encoding)?,
                attribute_text(attribute.get("value").unwrap_or(&Value::Null), encoding)?,
            ));
        }
        let ordinal = u64::try_from(event_index).unwrap_or(u64::MAX >> 17) << 16;
        let lookup = |name: &str| {
            attributes
                .iter()
                .find(|(key, _)| key == name)
                .map(|(_, value)| value.clone())
        };
        let attribute_map: Map<String, Value> = attributes
            .iter()
            .map(|(key, value)| (key.clone(), Value::String(value.clone())))
            .collect();
        let event_row = |name: &str, account: Option<String>| EventRow {
            height_or_seq: height,
            source: "cosmos-event".to_owned(),
            name: name.to_owned(),
            contract: None,
            account,
            tx_id: tx_hash.clone(),
            ordinal: u64::try_from(event_index).unwrap_or_default(),
            decoded: json!({
                "block_number": height.to_string(),
                "type": name,
                "attributes": Value::Object(attribute_map.clone()),
            }),
        };
        match kind {
            "transfer" => {
                let mut sender = None;
                let mut recipient = None;
                let mut group = 0_u64;
                for (key, value) in &attributes {
                    match key.as_str() {
                        "sender" => sender = Some(value.clone()),
                        "recipient" => recipient = Some(value.clone()),
                        "amount" => {
                            let (Some(from), Some(to)) = (sender.take(), recipient.take()) else {
                                continue;
                            };
                            for (coin_index, (amount, denom)) in
                                parse_coins(value)?.into_iter().enumerate()
                            {
                                let leg = ordinal
                                    | (group << 8)
                                    | u64::try_from(coin_index).unwrap_or(0xff).min(0xff);
                                let decoded = json!({
                                    "block_number": height.to_string(),
                                    "type": "transfer",
                                    "sender": from,
                                    "recipient": to,
                                    "amount": amount,
                                    "denom": denom,
                                });
                                unit.assets.push(denom_asset(&denom));
                                for (direction, account, counterparty) in
                                    [("out", &from, &to), ("in", &to, &from)]
                                {
                                    unit.transfers.push(TransferRow {
                                        height_or_seq: height,
                                        kind: "bank_transfer".to_owned(),
                                        direction,
                                        account: account.clone(),
                                        counterparty: Some(counterparty.clone()),
                                        asset: format!("denom:{denom}"),
                                        amount: amount.clone(),
                                        tx_id: tx_hash.clone(),
                                        ordinal: leg,
                                        decoded: decoded.clone(),
                                    });
                                }
                            }
                            group += 1;
                        }
                        _ => {}
                    }
                }
            }
            "create_denom" => {
                if let Some(denom) = lookup("new_token_denom") {
                    let mut asset = denom_asset(&denom);
                    asset.metadata = json!({ "creator": lookup("creator") });
                    unit.assets.push(asset);
                }
                unit.events.push(event_row(kind, lookup("creator")));
            }
            "mint" | "burn" => {
                let (holder, direction, transfer_kind) = if kind == "mint" {
                    (lookup("mint_to_address"), "in", "tokenfactory_mint")
                } else {
                    (lookup("burn_from_address"), "out", "tokenfactory_burn")
                };
                let Some(holder) = holder else {
                    continue;
                };
                for (coin_index, (amount, denom)) in
                    parse_coins(&lookup("amount").unwrap_or_default())?
                        .into_iter()
                        .enumerate()
                {
                    unit.assets.push(denom_asset(&denom));
                    unit.transfers.push(TransferRow {
                        height_or_seq: height,
                        kind: transfer_kind.to_owned(),
                        direction,
                        account: holder.clone(),
                        counterparty: None,
                        asset: format!("denom:{denom}"),
                        amount: amount.clone(),
                        tx_id: tx_hash.clone(),
                        ordinal: ordinal | u64::try_from(coin_index).unwrap_or(0xff).min(0xff),
                        decoded: json!({
                            "block_number": height.to_string(),
                            "type": kind,
                            "amount": amount,
                            "denom": denom,
                        }),
                    });
                }
                unit.events.push(event_row(kind, Some(holder)));
            }
            "change_admin" => unit.events.push(event_row(kind, lookup("new_admin"))),
            _ if kind.starts_with("layerx_") => {
                let account = lookup("operator")
                    .or_else(|| lookup("signer"))
                    .or_else(|| lookup("reporter"))
                    .or_else(|| lookup("guarantor_id"));
                unit.events.push(event_row(kind, account));
            }
            _ => {}
        }
    }
    Ok(())
}

/// Groups `tx_search` result entries by height.
///
/// # Errors
/// Refuses an entry without a decimal height.
pub fn group_by_height(txs: &[Value]) -> Result<BTreeMap<u64, Vec<Value>>, IndexError> {
    let mut grouped: BTreeMap<u64, Vec<Value>> = BTreeMap::new();
    for transaction in txs {
        let height: u64 = field_str(transaction, "height")?
            .parse()
            .map_err(|_| IndexError::Decode("tx height is not decimal".to_owned()))?;
        grouped.entry(height).or_default().push(transaction.clone());
    }
    Ok(grouped)
}

/// Follows one Paxeer node's EVM JSON-RPC and, when configured, its
/// CometBFT RPC.
pub struct PaxeerIngester {
    evm: Endpoint,
    comet: Option<Endpoint>,
    registry: AbiRegistry,
    policy: FollowPolicy,
    start_block: u64,
    expected_chain_id: Option<u64>,
    encoding: AttributeEncoding,
    tx_index_off_reported: AtomicBool,
}

impl PaxeerIngester {
    #[must_use]
    pub const fn new(
        evm: Endpoint,
        comet: Option<Endpoint>,
        registry: AbiRegistry,
        policy: FollowPolicy,
        start_block: u64,
        expected_chain_id: Option<u64>,
        encoding: AttributeEncoding,
    ) -> Self {
        Self {
            evm,
            comet,
            registry,
            policy,
            start_block,
            expected_chain_id,
            encoding,
            tx_index_off_reported: AtomicBool::new(false),
        }
    }

    /// The first block this ingester indexes on an empty store.
    #[must_use]
    pub const fn start_block(&self) -> u64 {
        self.start_block
    }

    /// The follow policy (finality depth and step size).
    #[must_use]
    pub const fn policy(&self) -> FollowPolicy {
        self.policy
    }

    /// Confirms the EVM endpoint serves the configured chain.
    ///
    /// # Errors
    /// Returns source failures and [`IndexError::Integrity`] on a mismatch.
    pub fn check_chain_id(&self) -> Result<(), IndexError> {
        if let Some(expected) = self.expected_chain_id {
            let chain_id = self.evm.rpc("eth_chainId", &json!([]))?;
            let actual = quantity_u64(chain_id.as_str().unwrap_or_default())?;
            if actual != expected {
                return Err(IndexError::Integrity(format!(
                    "EVM endpoint serves chain {actual}, expected {expected}"
                )));
            }
        }
        Ok(())
    }

    /// The node's block header at `height` (transaction hashes only), or
    /// `None` when the node has no block there.
    ///
    /// # Errors
    /// Returns source failures.
    pub fn header(&self, height: u64) -> Result<Option<Value>, IndexError> {
        let block = self
            .evm
            .rpc("eth_getBlockByNumber", &json!([to_quantity(height), false]))?;
        Ok((!block.is_null()).then_some(block))
    }

    /// The node's full block at `height` with every transaction's receipt,
    /// exactly as [`PaxeerIngester::step`] fetches it.
    ///
    /// # Errors
    /// Returns source failures and a missing receipt.
    pub fn block_with_receipts(
        &self,
        height: u64,
    ) -> Result<Option<(Value, BTreeMap<String, Value>)>, IndexError> {
        let Some(block) = self.block(height)? else {
            return Ok(None);
        };
        let receipts = self.receipts(&block)?;
        Ok(Some((block, receipts)))
    }

    /// The CometBFT `tx_search` results for `from..=to`, grouped by height,
    /// exactly as [`PaxeerIngester::step`] reads them.
    ///
    /// # Errors
    /// Returns source and decode failures.
    pub fn cosmos_range(
        &self,
        from: u64,
        to: u64,
    ) -> Result<BTreeMap<u64, Vec<Value>>, IndexError> {
        self.tx_search(from, to)
    }

    /// Decodes one block with this ingester's registry and encoding.
    ///
    /// # Errors
    /// As [`decode_block`].
    pub fn decode(
        &self,
        block: &Value,
        receipts: &BTreeMap<String, Value>,
        cosmos_txs: &[Value],
        is_pointer: &dyn Fn(&str) -> Result<bool, IndexError>,
    ) -> Result<Unit, IndexError> {
        decode_block(
            block,
            &BlockInputs {
                registry: &self.registry,
                receipts,
                cosmos_txs,
                encoding: self.encoding,
                is_pointer,
            },
        )
    }

    fn block(&self, height: u64) -> Result<Option<Value>, IndexError> {
        let block = self
            .evm
            .rpc("eth_getBlockByNumber", &json!([to_quantity(height), true]))?;
        Ok((!block.is_null()).then_some(block))
    }

    fn block_hash(&self, height: u64) -> Result<Option<String>, IndexError> {
        self.block(height)?
            .map(|block| hash_text(&block, "hash"))
            .transpose()
    }

    fn receipts(&self, block: &Value) -> Result<BTreeMap<String, Value>, IndexError> {
        let mut receipts = BTreeMap::new();
        for transaction in field(block, "transactions")?
            .as_array()
            .map_or(&[][..], Vec::as_slice)
        {
            let hash = hash_text(transaction, "hash")?;
            let receipt = self.evm.rpc("eth_getTransactionReceipt", &json!([hash]))?;
            if receipt.is_null() {
                return Err(IndexError::Source(format!(
                    "receipt of {hash} is not available"
                )));
            }
            receipts.insert(hash, receipt);
        }
        Ok(receipts)
    }

    fn tx_search(&self, from: u64, to: u64) -> Result<BTreeMap<u64, Vec<Value>>, IndexError> {
        let Some(comet) = &self.comet else {
            return Ok(BTreeMap::new());
        };
        let query = format!("tx.height>={from} AND tx.height<={to}");
        let mut collected = Vec::new();
        let mut page = 1_u64;
        loop {
            let answer = comet.rpc_answer(
                "tx_search",
                &json!({
                    "query": query,
                    "prove": false,
                    "page": page.to_string(),
                    "per_page": TX_SEARCH_PAGE.to_string(),
                    "order_by": "asc",
                }),
            )?;
            let result = match answer {
                Ok(result) => result,
                Err(error)
                    if page == 1
                        && error.get("data").and_then(Value::as_str) == Some(TX_INDEX_DISABLED) =>
                {
                    if !self.tx_index_off_reported.swap(true, Ordering::Relaxed) {
                        eprintln!(
                            "layerx-indexer paxeer: the CometBFT node has its tx index off ({TX_INDEX_DISABLED}); indexing EVM only"
                        );
                    }
                    return Ok(BTreeMap::new());
                }
                Err(error) => {
                    return Err(IndexError::Source(format!("tx_search refused: {error}")))
                }
            };
            let txs = result
                .get("txs")
                .and_then(Value::as_array)
                .ok_or_else(|| IndexError::Decode("tx_search has no txs".to_owned()))?;
            let total: u64 = match result.get("total_count") {
                Some(Value::String(text)) => text
                    .parse()
                    .map_err(|_| IndexError::Decode("total_count is not decimal".to_owned()))?,
                Some(Value::Number(number)) => number
                    .as_u64()
                    .ok_or_else(|| IndexError::Decode("total_count is not a count".to_owned()))?,
                _ => {
                    return Err(IndexError::Decode(
                        "tx_search has no total_count".to_owned(),
                    ))
                }
            };
            collected.extend(txs.iter().cloned());
            if txs.is_empty() || u64::try_from(collected.len()).unwrap_or(u64::MAX) >= total {
                break;
            }
            page += 1;
        }
        group_by_height(&collected)
    }

    /// Runs one bounded step: confirms the durable head block is still
    /// canonical, then commits up to `max_units_per_step` new blocks.
    ///
    /// # Errors
    /// Returns source, decode, integrity and store failures, and
    /// [`IndexError::ReorgBeyondFinality`].
    pub fn step(&self, store: &Store) -> Result<StepOutcome, IndexError> {
        self.check_chain_id()?;
        let head_value = self.evm.rpc("eth_blockNumber", &json!([]))?;
        let head = quantity_u64(head_value.as_str().unwrap_or_default())?;
        let cursor = store.cursor(CHAIN)?;
        if let Some(cursor) = &cursor {
            if self.block_hash(cursor.position)?.as_deref() != Some(cursor.hash.as_str()) {
                return self.reorg(store, cursor.position);
            }
        }
        let next = cursor.map_or(self.start_block, |cursor| cursor.position + 1);
        if next > head {
            return Ok(StepOutcome::Idle);
        }
        let last = head.min(next.saturating_add(self.policy.max_units_per_step.max(1) - 1));
        let cosmos = self.tx_search(next, last)?;
        let is_pointer = |address: &str| store.is_pointer(address);
        let mut units = 0;
        for height in next..=last {
            let block = self
                .block(height)?
                .ok_or_else(|| IndexError::Source(format!("block {height} is not available")))?;
            let receipts = self.receipts(&block)?;
            let inputs = BlockInputs {
                registry: &self.registry,
                receipts: &receipts,
                cosmos_txs: cosmos.get(&height).map_or(&[][..], Vec::as_slice),
                encoding: self.encoding,
                is_pointer: &is_pointer,
            };
            let unit = decode_block(&block, &inputs)?;
            if unit.position != height {
                return Err(IndexError::Integrity(format!(
                    "node answered block {} for {height}",
                    unit.position
                )));
            }
            if let Some(previous) = height
                .checked_sub(1)
                .map(|position| store.link(CHAIN, position))
                .transpose()?
                .flatten()
            {
                if previous.link != unit.parent {
                    return self.reorg(store, previous.position);
                }
            }
            store.commit(&unit, self.policy.finality_depth)?;
            units += 1;
        }
        Ok(StepOutcome::Advanced {
            units,
            position: last,
        })
    }

    fn reorg(&self, store: &Store, from: u64) -> Result<StepOutcome, IndexError> {
        walk_back(store, CHAIN, from, self.start_block, |height| {
            self.block_hash(height)
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn coin_lists_split_exactly() {
        assert_eq!(
            parse_coins("10upax,5factory/pax1abc/sub").unwrap_or_default(),
            vec![
                ("10".to_owned(), "upax".to_owned()),
                ("5".to_owned(), "factory/pax1abc/sub".to_owned())
            ]
        );
        assert!(parse_coins("upax").is_err());
        assert!(parse_coins("01upax").is_err());
        assert!(parse_coins("10").is_err());
    }

    #[test]
    fn transfer_topic_is_the_erc20_signature() {
        assert_eq!(
            hex(&erc20_transfer_topic()),
            "ddf252ad1be2c89b69c2b068fc378daa952ba7f163c4a11628f55a4df523b3ef"
        );
    }
}
