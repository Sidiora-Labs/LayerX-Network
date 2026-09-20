use crate::encoding::{bytes, failure, fixed, hex, invalid, quantity, required};
use crate::{raw_call, EndpointConfig, EndpointFailure, EndpointFault, Json};
use layerx_types::intent::EvmAddress;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct BlockAnchor {
    pub number: u64,
    pub hash: [u8; 32],
    pub timestamp: u64,
}
impl BlockAnchor {
    pub(crate) fn decode(value: &Json) -> Result<Self, EndpointFault> {
        let anchor = Self {
            number: quantity(required(value, "number")?)?,
            hash: fixed(required(value, "hash")?)?,
            timestamp: quantity(required(value, "timestamp")?)?,
        };
        if anchor.hash == [0; 32] || anchor.timestamp == 0 {
            return Err(invalid());
        }
        Ok(anchor)
    }
}

pub struct Publication {
    pub input: Vec<u8>,
    pub topics: Vec<[u8; 32]>,
    pub data: Vec<u8>,
    pub sender: [u8; 20],
    pub transaction_hash: [u8; 32],
    pub registration: BlockAnchor,
    pub confirmed_head: BlockAnchor,
}

fn publication_log(
    endpoint: &EndpointConfig,
    contract: EvmAddress,
    topic: [u8; 32],
    position: usize,
    checkpoint: [u8; 32],
) -> Result<Json, EndpointFault> {
    let mut filter = vec![Json::Null; position.checked_add(1).ok_or_else(invalid)?];
    filter[0] = Json::Text(hex(&topic));
    filter[position] = Json::Text(hex(&checkpoint));
    let rpc = |method, params: &[Json]| raw_call(endpoint, method, params).map_err(|e| e.fault);
    let first = rpc(
        "eth_getBlockByNumber",
        &[Json::Text("earliest".into()), Json::Bool(false)],
    )?;
    let mut begin = quantity(required(&first, "number")?)?;
    let end = quantity(&rpc("eth_blockNumber", &[])?)?;
    if begin > end || fixed::<32>(required(&first, "hash")?)? == [0; 32] {
        return Err(invalid());
    }
    let mut logs = Vec::new();
    loop {
        let last = begin.saturating_add(255).min(end);
        let batch = rpc(
            "eth_getLogs",
            &[Json::Object(vec![
                ("address".into(), Json::Text(hex(&contract.bytes()))),
                ("fromBlock".into(), Json::Text(format!("0x{begin:x}"))),
                ("toBlock".into(), Json::Text(format!("0x{last:x}"))),
                ("topics".into(), Json::Array(filter.clone())),
            ])],
        )?;
        let Json::Array(batch) = batch else {
            return Err(invalid());
        };
        if batch.len() > 1 || logs.len() + batch.len() > 1 {
            return Err(invalid());
        }
        for log in batch {
            if !(begin..=last).contains(&quantity(required(&log, "blockNumber")?)?) {
                return Err(invalid());
            }
            logs.push(log);
        }
        if last == end {
            break;
        }
        begin = last.checked_add(1).ok_or_else(invalid)?;
    }
    if logs.len() != 1 {
        return Err(invalid());
    }
    Ok(logs.remove(0))
}

/// # Errors
/// Refuses unauthenticated publication, displaced transaction or log facts, and insufficient confirmations.
pub fn publication(
    endpoint: &EndpointConfig,
    contract: EvmAddress,
    topic: [u8; 32],
    checkpoint: [u8; 32],
    confirmations: u64,
) -> Result<Publication, EndpointFailure> {
    publication_at(endpoint, contract, topic, 1, checkpoint, confirmations)
}

/// Reads the one publication whose topic at `position` is `checkpoint`. The
/// layerxAnchor checkpoint events index the batch number first and the
/// checkpoint identifier second.
///
/// # Errors
/// Refuses what `publication` refuses, and a position outside the indexed topics.
pub fn publication_at(
    endpoint: &EndpointConfig,
    contract: EvmAddress,
    topic: [u8; 32],
    position: usize,
    checkpoint: [u8; 32],
    confirmations: u64,
) -> Result<Publication, EndpointFailure> {
    let run = || -> Result<Publication, EndpointFault> {
        if confirmations == 0 || !(1..=3).contains(&position) {
            return Err(invalid());
        }
        let rpc = |method, params: &[Json]| raw_call(endpoint, method, params).map_err(|e| e.fault);
        let log = publication_log(endpoint, contract, topic, position, checkpoint)?;
        let log = &log;
        if required(log, "removed")? != &Json::Bool(false)
            || fixed::<20>(required(log, "address")?)? != contract.bytes()
        {
            return Err(invalid());
        }
        let Json::Array(topics) = required(log, "topics")? else {
            return Err(invalid());
        };
        let topics = topics
            .iter()
            .map(fixed::<32>)
            .collect::<Result<Vec<_>, _>>()?;
        if topics.first() != Some(&topic) || topics.get(position) != Some(&checkpoint) {
            return Err(invalid());
        }
        let hash = fixed::<32>(required(log, "transactionHash")?)?;
        let block_hash = fixed::<32>(required(log, "blockHash")?)?;
        let block_number = quantity(required(log, "blockNumber")?)?;
        let tx_index = quantity(required(log, "transactionIndex")?)?;
        let log_index = quantity(required(log, "logIndex")?)?;
        let params = [Json::Text(hex(&hash))];
        let tx = rpc("eth_getTransactionByHash", &params)?;
        let receipt = rpc("eth_getTransactionReceipt", &params)?;
        if fixed::<32>(required(&tx, "hash")?)? != hash
            || fixed::<32>(required(&receipt, "transactionHash")?)? != hash
            || quantity(required(&receipt, "status")?)? != 1
        {
            return Err(invalid());
        }
        for value in [&tx, &receipt] {
            if fixed::<20>(required(value, "to")?)? != contract.bytes()
                || fixed::<32>(required(value, "blockHash")?)? != block_hash
                || quantity(required(value, "blockNumber")?)? != block_number
                || quantity(required(value, "transactionIndex")?)? != tx_index
            {
                return Err(invalid());
            }
        }
        let Json::Array(receipt_logs) = required(&receipt, "logs")? else {
            return Err(invalid());
        };
        let matching = receipt_logs
            .iter()
            .filter(|item| {
                item.member("logIndex").and_then(|v| quantity(v).ok()) == Some(log_index)
            })
            .collect::<Vec<_>>();
        if matching.len() != 1 {
            return Err(invalid());
        }
        for field in [
            "address",
            "topics",
            "data",
            "blockHash",
            "blockNumber",
            "transactionHash",
            "transactionIndex",
            "logIndex",
            "removed",
        ] {
            if required(matching[0], field)? != required(log, field)? {
                return Err(invalid());
            }
        }
        let (registration, confirmed_head) = confirm_block(endpoint, log, confirmations)?;
        Ok(Publication {
            input: bytes(required(&tx, "input")?)?,
            topics,
            data: bytes(required(log, "data")?)?,
            sender: fixed(required(&tx, "from")?)?,
            transaction_hash: hash,
            registration,
            confirmed_head,
        })
    };
    run().map_err(|fault| failure(endpoint, fault))
}

fn confirm_block(
    endpoint: &EndpointConfig,
    log: &Json,
    confirmations: u64,
) -> Result<(BlockAnchor, BlockAnchor), EndpointFault> {
    let rpc = |method, params: &[Json]| raw_call(endpoint, method, params).map_err(|e| e.fault);
    let hash = fixed::<32>(required(log, "transactionHash")?)?;
    let block_hash = fixed::<32>(required(log, "blockHash")?)?;
    let block_number = quantity(required(log, "blockNumber")?)?;
    let tx_index = quantity(required(log, "transactionIndex")?)?;
    let head = rpc(
        "eth_getBlockByNumber",
        &[Json::Text("latest".into()), Json::Bool(false)],
    )?;
    let head_number = quantity(required(&head, "number")?)?;
    if head_number
        .checked_sub(block_number)
        .and_then(|n| n.checked_add(1))
        .is_none_or(|n| n < confirmations)
    {
        return Err(invalid());
    }
    let block = rpc(
        "eth_getBlockByNumber",
        &[Json::Text(format!("0x{block_number:x}")), Json::Bool(false)],
    )?;
    if quantity(required(&block, "number")?)? != block_number
        || fixed::<32>(required(&block, "hash")?)? != block_hash
    {
        return Err(invalid());
    }
    let Json::Array(transactions) = required(&block, "transactions")? else {
        return Err(invalid());
    };
    if fixed::<32>(
        transactions
            .get(usize::try_from(tx_index).map_err(|_| invalid())?)
            .ok_or_else(invalid)?,
    )? != hash
    {
        return Err(invalid());
    }
    Ok((BlockAnchor::decode(&block)?, BlockAnchor::decode(&head)?))
}
