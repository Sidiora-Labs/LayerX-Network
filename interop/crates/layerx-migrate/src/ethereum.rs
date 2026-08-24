use std::collections::BTreeSet;
use std::time::{SystemTime, UNIX_EPOCH};

use k256::ecdsa::{RecoveryId, Signature, VerifyingKey};
use layerx_interop_gateway::trace::TraceId;
use serde::Deserialize;
use serde_json::{json, Value};
use sha2::{Digest as _, Sha256};
use sha3::Keccak256;

use crate::journal::{ChainCheckpoint, Journal, JournalConfig};
use crate::rpc::{RpcCluster, RpcQuorumConfig};
use crate::source_codec::{
    decode_fixed_hex, decode_hex, decode_quantity, ethereum_hex, hex, quantity, Reader, Writer,
};
use crate::{
    ExternalAddress, ExternalHistoryKind, ExternalHistoryRecord, ExternalProvenance,
    MigrationError, SourceChain, SourceEvidence, SourceTransaction, SourceVerifier,
    VerifiedAssetFinality, VerifiedHistoryPage, VerifiedOwnership,
};

const OWNERSHIP_DOMAIN: &[u8] = b"LXM/ETH/OWNERSHIP/1\0";
const ASSET_DOMAIN: &[u8] = b"LXM/ETH/ASSET/1\0";
const HISTORY_DOMAIN: &[u8] = b"LXM/ETH/HISTORY/1\0";
const SIGNING_DOMAIN: &[u8] = b"LayerX Ethereum migration ownership v1\n";
const MAX_LOGS: usize = 256;
const ERC20_TRANSFER_TOPIC: [u8; 32] = [
    0xdd, 0xf2, 0x52, 0xad, 0x1b, 0xe2, 0xc8, 0x9b, 0x69, 0xc2, 0xb0, 0x68, 0xfc, 0x37, 0x8d, 0xaa,
    0x95, 0x2b, 0xa7, 0xf1, 0x63, 0xc4, 0xa1, 0x16, 0x28, 0xf5, 0x5a, 0x4d, 0xf5, 0x23, 0xb3, 0xef,
];

/// Location of one ABI word in the configured custody event.
#[derive(Clone, Copy, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd)]
pub enum EthereumWord {
    Topic(u8),
    Data(u16),
}

/// Operator-owned ABI map for the deployed custody contract event. LayerX
/// does not guess a custody ABI: every bound field and selector is explicit.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq)]
pub struct EthereumCustodySchema {
    pub function_selector: [u8; 4],
    pub event_topic: [u8; 32],
    pub source: EthereumWord,
    pub source_asset: EthereumWord,
    pub source_amount: EthereumWord,
    pub custody_reference: EthereumWord,
    pub layerx_asset: EthereumWord,
    pub layerx_amount: EthereumWord,
    pub destination: EthereumWord,
}

/// Canonical head used for credit eligibility.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq)]
pub enum EthereumFinalityTag {
    Safe,
    Finalized,
}

/// Runtime identity policy for the custody deployment. Proxy deployments must
/// additionally pin the implementation storage slot, address, and bytecode.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq)]
pub enum EthereumContractIdentity {
    Immutable,
    Proxy {
        implementation_slot: [u8; 32],
        implementation_address: [u8; 20],
        implementation_code_hash: [u8; 32],
    },
}

impl EthereumFinalityTag {
    fn rpc(self) -> &'static str {
        match self {
            Self::Safe => "safe",
            Self::Finalized => "finalized",
        }
    }
}

/// Ethereum source verifier policy. Contract identity includes both its exact
/// address and the Keccak-256 digest of the bytecode observed at the finality
/// tag, preventing a configuration from silently crossing deployments.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq)]
pub struct EthereumConfig {
    pub chain_id: u64,
    pub genesis_hash: [u8; 32],
    pub custody_contract: [u8; 20],
    pub custody_code_hash: [u8; 32],
    pub custody_identity: EthereumContractIdentity,
    pub native_asset: [u8; 32],
    pub custody: EthereumCustodySchema,
    pub finality_tag: EthereumFinalityTag,
    pub minimum_confirmations: u64,
    pub maximum_ancestry: u64,
    pub maximum_history_blocks: u64,
    pub maximum_ownership_ttl_seconds: u64,
    pub rpc: RpcQuorumConfig,
    pub journal: JournalConfig,
}

/// Wallet-signed ownership request. The signature is an EIP-191 personal-sign
/// signature over [`EthereumOwnershipClaim::signing_message`].
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct EthereumOwnershipClaim {
    pub chain_id: u64,
    pub address: [u8; 20],
    pub layerx_identity: [u8; 32],
    pub issued_at: u64,
    pub expires_at: u64,
    pub nonce: [u8; 32],
    pub signature: [u8; 65],
}

impl EthereumOwnershipClaim {
    /// Exact bytes a wallet signs through EIP-191 personal sign.
    #[must_use]
    pub fn signing_message(&self) -> Vec<u8> {
        let mut writer = Writer::new(SIGNING_DOMAIN);
        writer.u64(self.chain_id);
        writer.fixed(&self.address);
        writer.fixed(&self.layerx_identity);
        writer.u64(self.issued_at);
        writer.u64(self.expires_at);
        writer.fixed(&self.nonce);
        writer.finish()
    }

    /// Encodes the bounded request consumed by the production verifier.
    ///
    /// # Errors
    ///
    /// Refuses structurally invalid ownership requests.
    pub fn evidence(&self) -> Result<SourceEvidence, MigrationError> {
        if self.chain_id == 0
            || self.address == [0; 20]
            || self.layerx_identity == [0; 32]
            || self.nonce == [0; 32]
            || self.issued_at >= self.expires_at
        {
            return Err(MigrationError::InvalidEvidence);
        }
        let mut writer = Writer::new(OWNERSHIP_DOMAIN);
        writer.u64(self.chain_id);
        writer.fixed(&self.address);
        writer.fixed(&self.layerx_identity);
        writer.u64(self.issued_at);
        writer.u64(self.expires_at);
        writer.fixed(&self.nonce);
        writer.fixed(&self.signature);
        SourceEvidence::new(writer.finish())
    }
}

/// Expected binding for one custody transaction. Every field is compared to
/// the configured on-chain event; none becomes verified because it was claimed.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct EthereumAssetClaim {
    pub chain_id: u64,
    pub transaction_hash: [u8; 32],
    pub source: [u8; 20],
    pub source_asset: [u8; 32],
    pub source_amount: u128,
    pub custody_reference: [u8; 32],
    pub layerx_asset: [u8; 32],
    pub layerx_amount: u128,
    pub destination: [u8; 32],
}

impl EthereumAssetClaim {
    /// Encodes a bounded expected custody-event binding.
    ///
    /// # Errors
    ///
    /// Refuses zero or malformed claim fields.
    pub fn evidence(self) -> Result<SourceEvidence, MigrationError> {
        if self.chain_id == 0
            || self.transaction_hash == [0; 32]
            || self.source == [0; 20]
            || self.source_asset == [0; 32]
            || self.source_amount == 0
            || self.custody_reference == [0; 32]
            || self.layerx_asset == [0; 32]
            || self.layerx_amount == 0
            || self.destination == [0; 32]
        {
            return Err(MigrationError::InvalidEvidence);
        }
        let mut writer = Writer::new(ASSET_DOMAIN);
        writer.u64(self.chain_id);
        writer.fixed(&self.transaction_hash);
        writer.fixed(&self.source);
        writer.fixed(&self.source_asset);
        writer.u128(self.source_amount);
        writer.fixed(&self.custody_reference);
        writer.fixed(&self.layerx_asset);
        writer.u128(self.layerx_amount);
        writer.fixed(&self.destination);
        SourceEvidence::new(writer.finish())
    }
}

/// Ascending finalized block range imported as Ethereum provenance.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct EthereumHistoryClaim {
    pub chain_id: u64,
    pub address: [u8; 20],
    pub from_block: u64,
    pub to_block: u64,
    pub previous_cursor: Option<[u8; 32]>,
}

impl EthereumHistoryClaim {
    /// Encodes one explicit history range.
    ///
    /// # Errors
    ///
    /// Refuses zero or descending ranges.
    pub fn evidence(self) -> Result<SourceEvidence, MigrationError> {
        if self.chain_id == 0
            || self.address == [0; 20]
            || self.from_block == 0
            || self.to_block < self.from_block
            || self.previous_cursor == Some([0; 32])
        {
            return Err(MigrationError::InvalidHistory);
        }
        let mut writer = Writer::new(HISTORY_DOMAIN);
        writer.u64(self.chain_id);
        writer.fixed(&self.address);
        writer.u64(self.from_block);
        writer.u64(self.to_block);
        if let Some(cursor) = self.previous_cursor {
            writer.u8(1);
            writer.fixed(&cursor);
        } else {
            writer.u8(0);
        }
        SourceEvidence::new(writer.finish())
    }
}

/// Production Ethereum verifier using authenticated HTTPS quorum reads and an
/// integrity-protected monotonic checkpoint journal.
pub struct EthereumVerifier {
    config: EthereumConfig,
    rpc: RpcCluster,
    journal: Journal,
}

impl crate::sealed::SourceVerifier for EthereumVerifier {}

impl EthereumVerifier {
    /// Builds a verifier and reconciles its authenticated journal head with
    /// the configured non-rollbackable quorum authority.
    ///
    /// # Errors
    ///
    /// Refuses incomplete ABI, network, quorum, finality, or journal policy.
    pub fn new(config: EthereumConfig) -> Result<Self, MigrationError> {
        validate_config(&config)?;
        let rpc = RpcCluster::new(&config.rpc)?;
        let journal = Journal::new(&config.journal)?;
        Ok(Self {
            config,
            rpc,
            journal,
        })
    }

    fn verify_network(&self) -> Result<(), MigrationError> {
        let chain = self.rpc.call("eth_chainId", json!([]))?;
        let chain = chain
            .as_str()
            .ok_or(MigrationError::RpcResponseMismatch)
            .and_then(decode_quantity)?;
        if chain != self.config.chain_id {
            return Err(MigrationError::InvalidNetwork);
        }
        let genesis = self.block_by_number(0)?;
        if genesis.hash != self.config.genesis_hash {
            return Err(MigrationError::InvalidNetwork);
        }
        Ok(())
    }

    fn verify_contract(&self, block_number: u64) -> Result<(), MigrationError> {
        let block = quantity(block_number);
        let code = self.rpc.call(
            "eth_getCode",
            json!([ethereum_hex(&self.config.custody_contract), block]),
        )?;
        let code = code
            .as_str()
            .ok_or(MigrationError::RpcResponseMismatch)
            .and_then(decode_hex)?;
        if code.is_empty()
            || <[u8; 32]>::from(Keccak256::digest(&code)) != self.config.custody_code_hash
        {
            return Err(MigrationError::CustodyEventMismatch);
        }
        if let EthereumContractIdentity::Proxy {
            implementation_slot,
            implementation_address,
            implementation_code_hash,
        } = self.config.custody_identity
        {
            let storage = self.rpc.call(
                "eth_getStorageAt",
                json!([
                    ethereum_hex(&self.config.custody_contract),
                    ethereum_hex(&implementation_slot),
                    quantity(block_number)
                ]),
            )?;
            let storage = decode_fixed_hex::<32>(
                storage
                    .as_str()
                    .ok_or(MigrationError::RpcResponseMismatch)?,
            )?;
            if storage[..12] != [0; 12] || storage[12..] != implementation_address {
                return Err(MigrationError::CustodyEventMismatch);
            }
            let implementation = self.rpc.call(
                "eth_getCode",
                json!([
                    ethereum_hex(&implementation_address),
                    quantity(block_number)
                ]),
            )?;
            let implementation = decode_hex(
                implementation
                    .as_str()
                    .ok_or(MigrationError::RpcResponseMismatch)?,
            )?;
            if implementation.is_empty()
                || <[u8; 32]>::from(Keccak256::digest(&implementation)) != implementation_code_hash
            {
                return Err(MigrationError::CustodyEventMismatch);
            }
        }
        Ok(())
    }

    fn finality(&self, target: &Block) -> Result<Block, MigrationError> {
        let head = self.block_by_tag(self.config.finality_tag.rpc())?;
        if target.number > head.number
            || head.number.saturating_sub(target.number).saturating_add(1)
                < self.config.minimum_confirmations
        {
            return Err(MigrationError::SourcePending);
        }
        let previous = self.verify_previous_checkpoint(&head)?;
        let required_height = previous.as_ref().map_or(target.number, |checkpoint| {
            checkpoint.height.min(target.number)
        });
        let distance = head.number.saturating_sub(required_height);
        if distance > self.config.maximum_ancestry {
            return Err(MigrationError::FinalityWindowExceeded);
        }
        let mut cursor = head.clone();
        let mut target_seen = false;
        let mut previous_seen = previous.is_none();
        for _ in 0..=distance {
            if cursor.number == target.number {
                if cursor.hash != target.hash {
                    return Err(MigrationError::SourceDisplaced);
                }
                target_seen = true;
            }
            if previous.as_ref().is_some_and(|checkpoint| {
                checkpoint.height == cursor.number && checkpoint.hash == cursor.hash
            }) {
                previous_seen = true;
            }
            if cursor.number == required_height {
                break;
            }
            let parent = self.block_by_hash(cursor.parent_hash)?;
            if parent.number.saturating_add(1) != cursor.number || parent.hash != cursor.parent_hash
            {
                return Err(MigrationError::SourceDisplaced);
            }
            cursor = parent;
        }
        if !target_seen || !previous_seen {
            return Err(MigrationError::SourceDisplaced);
        }
        if previous
            .as_ref()
            .is_none_or(|checkpoint| checkpoint.height != head.number)
        {
            self.journal.record_chain(
                &network_key(self.config.chain_id),
                ChainCheckpoint {
                    height: head.number,
                    hash: head.hash,
                    parent_hash: head.parent_hash,
                    previous_height: previous.as_ref().map_or(0, |value| value.height),
                    previous_hash: previous.as_ref().map_or([0; 32], |value| value.hash),
                },
            )?;
        }
        Ok(head)
    }

    fn verify_previous_checkpoint(
        &self,
        head: &Block,
    ) -> Result<Option<ChainCheckpoint>, MigrationError> {
        let key = network_key(self.config.chain_id);
        let Some(previous) = self.journal.checkpoint(&key)? else {
            return Ok(None);
        };
        if previous.height > head.number {
            return Err(MigrationError::CheckpointConflict);
        }
        let observed = self.block_by_number(previous.height)?;
        if observed.hash != previous.hash {
            return Err(MigrationError::CheckpointConflict);
        }
        if previous.height == head.number && previous.hash != head.hash {
            return Err(MigrationError::CheckpointConflict);
        }
        Ok(Some(previous))
    }

    fn block_by_tag(&self, tag: &str) -> Result<Block, MigrationError> {
        parse_block(self.rpc.call("eth_getBlockByNumber", json!([tag, false]))?)
    }

    fn block_by_number(&self, number: u64) -> Result<Block, MigrationError> {
        parse_block(
            self.rpc
                .call("eth_getBlockByNumber", json!([quantity(number), false]))?,
        )
    }

    fn block_by_hash(&self, hash: [u8; 32]) -> Result<Block, MigrationError> {
        parse_block(
            self.rpc
                .call("eth_getBlockByHash", json!([ethereum_hex(&hash), false]))?,
        )
    }

    fn receipt(&self, transaction: [u8; 32]) -> Result<Value, MigrationError> {
        let value = self.rpc.call(
            "eth_getTransactionReceipt",
            json!([ethereum_hex(&transaction)]),
        )?;
        if value.is_null() {
            return Err(MigrationError::SourcePending);
        }
        Ok(value)
    }

    fn transaction(&self, transaction: [u8; 32]) -> Result<Value, MigrationError> {
        let value = self.rpc.call(
            "eth_getTransactionByHash",
            json!([ethereum_hex(&transaction)]),
        )?;
        if value.is_null() {
            return Err(MigrationError::SourcePending);
        }
        Ok(value)
    }

    fn verify_asset(&self, claim: EthereumAssetClaim) -> Result<(Event, Block), MigrationError> {
        self.verify_network()?;
        let receipt = self.receipt(claim.transaction_hash)?;
        let transaction = self.transaction(claim.transaction_hash)?;
        if string(&receipt, "transactionHash")? != ethereum_hex(&claim.transaction_hash)
            || string(&transaction, "hash")? != ethereum_hex(&claim.transaction_hash)
            || decode_quantity(string(&receipt, "status")?)? != 1
        {
            return Err(MigrationError::SourceReverted);
        }
        let block_hash = decode_fixed_hex(string(&receipt, "blockHash")?)?;
        let block_number = decode_quantity(string(&receipt, "blockNumber")?)?;
        self.verify_contract(block_number)?;
        if string(&transaction, "blockHash")? != ethereum_hex(&block_hash)
            || decode_quantity(string(&transaction, "blockNumber")?)? != block_number
            || decode_fixed_hex::<20>(string(&transaction, "from")?)? != claim.source
            || decode_fixed_hex::<20>(string(&transaction, "to")?)? != self.config.custody_contract
        {
            return Err(MigrationError::RpcResponseMismatch);
        }
        let input = decode_hex(string(&transaction, "input")?)?;
        if input.get(..4) != Some(&self.config.custody.function_selector) {
            return Err(MigrationError::CustodyEventMismatch);
        }
        let logs = receipt
            .get("logs")
            .and_then(Value::as_array)
            .ok_or(MigrationError::RpcResponseMismatch)?;
        if logs.len() > MAX_LOGS {
            return Err(MigrationError::RpcResponseMismatch);
        }
        let mut matching = Vec::new();
        for log in logs {
            if let Some(event) = self.parse_event(log)? {
                matching.push(event);
            }
        }
        if matching.len() != 1 {
            return Err(MigrationError::CustodyEventMismatch);
        }
        let event = matching[0];
        if event.source != claim.source
            || event.source_asset != claim.source_asset
            || event.source_amount != claim.source_amount
            || event.custody_reference != claim.custody_reference
            || event.layerx_asset != claim.layerx_asset
            || event.layerx_amount != claim.layerx_amount
            || event.destination != claim.destination
            || event.transaction != claim.transaction_hash
            || event.block_hash != block_hash
            || event.block_number != block_number
        {
            return Err(MigrationError::CustodyEventMismatch);
        }
        let block = self.block_by_hash(block_hash)?;
        if block.number != block_number {
            return Err(MigrationError::SourceDisplaced);
        }
        let head = self.finality(&block)?;
        Ok((event, head))
    }

    fn parse_event(&self, value: &Value) -> Result<Option<Event>, MigrationError> {
        if decode_fixed_hex::<20>(string(value, "address")?)? != self.config.custody_contract {
            return Ok(None);
        }
        if value.get("removed").and_then(Value::as_bool) == Some(true) {
            return Err(MigrationError::SourceDisplaced);
        }
        let topics = value
            .get("topics")
            .and_then(Value::as_array)
            .ok_or(MigrationError::RpcResponseMismatch)?
            .iter()
            .map(|topic| {
                topic
                    .as_str()
                    .ok_or(MigrationError::RpcResponseMismatch)
                    .and_then(decode_fixed_hex)
            })
            .collect::<Result<Vec<[u8; 32]>, _>>()?;
        if topics.first() != Some(&self.config.custody.event_topic) {
            return Ok(None);
        }
        let data = decode_hex(string(value, "data")?)?;
        if data.len() % 32 != 0 || data.len() > 32 * 64 {
            return Err(MigrationError::CustodyEventMismatch);
        }
        let source_word = event_word(&topics, &data, self.config.custody.source)?;
        if source_word[..12] != [0; 12] {
            return Err(MigrationError::CustodyEventMismatch);
        }
        let mut source = [0_u8; 20];
        source.copy_from_slice(&source_word[12..]);
        Ok(Some(Event {
            transaction: decode_fixed_hex(string(value, "transactionHash")?)?,
            block_hash: decode_fixed_hex(string(value, "blockHash")?)?,
            block_number: decode_quantity(string(value, "blockNumber")?)?,
            source,
            source_asset: event_word(&topics, &data, self.config.custody.source_asset)?,
            source_amount: word_u128(event_word(
                &topics,
                &data,
                self.config.custody.source_amount,
            )?)?,
            custody_reference: event_word(&topics, &data, self.config.custody.custody_reference)?,
            layerx_asset: event_word(&topics, &data, self.config.custody.layerx_asset)?,
            layerx_amount: word_u128(event_word(
                &topics,
                &data,
                self.config.custody.layerx_amount,
            )?)?,
            destination: event_word(&topics, &data, self.config.custody.destination)?,
        }))
    }

    fn history(
        &self,
        claim: EthereumHistoryClaim,
        evidence_digest: [u8; 32],
    ) -> Result<VerifiedHistoryPage, MigrationError> {
        self.verify_network()?;
        if claim.chain_id != self.config.chain_id
            || claim
                .to_block
                .saturating_sub(claim.from_block)
                .saturating_add(1)
                > self.config.maximum_history_blocks
        {
            return Err(MigrationError::InvalidHistory);
        }
        let stream = format!("ethereum:{}:{}", self.config.chain_id, hex(&claim.address));
        self.journal.validate_history(
            &stream,
            claim.previous_cursor,
            claim.from_block,
            claim.to_block,
            evidence_digest,
        )?;
        let head = self.block_by_tag(self.config.finality_tag.rpc())?;
        if claim.to_block > head.number
            || head.number.saturating_sub(claim.to_block).saturating_add(1)
                < self.config.minimum_confirmations
        {
            return Err(MigrationError::SourcePending);
        }
        let parent_anchor = self
            .journal
            .history_parent_anchor(&stream, claim.previous_cursor)?;
        let logs = self.rpc.call(
            "eth_getLogs",
            json!([{
                "address": ethereum_hex(&self.config.custody_contract),
                "fromBlock": quantity(claim.from_block),
                "toBlock": quantity(claim.to_block),
                "topics": [ethereum_hex(&self.config.custody.event_topic)]
            }]),
        )?;
        let logs = logs.as_array().ok_or(MigrationError::RpcResponseMismatch)?;
        if logs.len() > MAX_LOGS {
            return Err(MigrationError::InvalidHistory);
        }
        let address_topic = address_word(claim.address);
        let outgoing_transfers = self.rpc.call(
            "eth_getLogs",
            json!([{
                "fromBlock": quantity(claim.from_block),
                "toBlock": quantity(claim.to_block),
                "topics": [ethereum_hex(&ERC20_TRANSFER_TOPIC), ethereum_hex(&address_topic)]
            }]),
        )?;
        let incoming_transfers = self.rpc.call(
            "eth_getLogs",
            json!([{
                "fromBlock": quantity(claim.from_block),
                "toBlock": quantity(claim.to_block),
                "topics": [ethereum_hex(&ERC20_TRANSFER_TOPIC), null, ethereum_hex(&address_topic)]
            }]),
        )?;
        let outgoing_transfers = outgoing_transfers
            .as_array()
            .ok_or(MigrationError::RpcResponseMismatch)?;
        let incoming_transfers = incoming_transfers
            .as_array()
            .ok_or(MigrationError::RpcResponseMismatch)?;
        if outgoing_transfers.len() > MAX_LOGS || incoming_transfers.len() > MAX_LOGS {
            return Err(MigrationError::InvalidHistory);
        }
        let mut records = Vec::new();
        let mut previous_hash = parent_anchor;
        let mut range_anchor = None;
        for number in claim.from_block..=claim.to_block {
            let full = self
                .rpc
                .call("eth_getBlockByNumber", json!([quantity(number), true]))?;
            let header = parse_block(full.clone())?;
            if header.number != number
                || previous_hash.is_some_and(|hash| header.parent_hash != hash)
            {
                return Err(MigrationError::SourceDisplaced);
            }
            previous_hash = Some(header.hash);
            range_anchor = Some(header.clone());
            let transactions = full
                .get("transactions")
                .and_then(Value::as_array)
                .ok_or(MigrationError::RpcResponseMismatch)?;
            if transactions.len() > 100_000 {
                return Err(MigrationError::InvalidHistory);
            }
            for transaction in transactions {
                let from = decode_fixed_hex::<20>(string(transaction, "from")?)?;
                let to = transaction
                    .get("to")
                    .and_then(Value::as_str)
                    .map(decode_fixed_hex)
                    .transpose()?;
                if from != claim.address && to != Some(claim.address) {
                    continue;
                }
                let transaction_hash = decode_fixed_hex(string(transaction, "hash")?)?;
                let receipt = self.receipt(transaction_hash)?;
                if string(&receipt, "transactionHash")? != ethereum_hex(&transaction_hash)
                    || decode_fixed_hex::<32>(string(&receipt, "blockHash")?)? != header.hash
                    || decode_quantity(string(&receipt, "blockNumber")?)? != number
                {
                    return Err(MigrationError::RpcResponseMismatch);
                }
                let status = decode_quantity(string(&receipt, "status")?)?;
                if status == 0 {
                    continue;
                }
                if status != 1 {
                    return Err(MigrationError::RpcResponseMismatch);
                }
                let value = decode_u128_quantity(string(transaction, "value")?)?;
                let input = decode_hex(string(transaction, "input")?)?;
                if value == 0 && input.is_empty() {
                    continue;
                }
                records.push(ExternalHistoryRecord {
                    chain: SourceChain::Ethereum {
                        chain_id: self.config.chain_id,
                    },
                    transaction: SourceTransaction::ethereum(transaction_hash)?,
                    address: ExternalAddress::Ethereum(claim.address),
                    kind: if !input.is_empty() {
                        ExternalHistoryKind::Contract
                    } else if from == claim.address {
                        ExternalHistoryKind::Outgoing
                    } else {
                        ExternalHistoryKind::Incoming
                    },
                    timestamp: header.timestamp,
                    source_asset: self.config.native_asset,
                    source_amount: value,
                    provenance: ExternalProvenance::Ethereum,
                });
                if records.len() > 256 {
                    return Err(MigrationError::InvalidHistory);
                }
            }
        }
        let range_anchor = range_anchor.ok_or(MigrationError::InvalidHistory)?;
        for value in logs {
            let Some(event) = self.parse_event(value)? else {
                continue;
            };
            if event.source != claim.address
                || !(claim.from_block..=claim.to_block).contains(&event.block_number)
            {
                continue;
            }
            self.verify_contract(event.block_number)?;
            let block = self.block_by_number(event.block_number)?;
            if block.hash != event.block_hash {
                return Err(MigrationError::SourceDisplaced);
            }
            records.push(ExternalHistoryRecord {
                chain: SourceChain::Ethereum {
                    chain_id: self.config.chain_id,
                },
                transaction: SourceTransaction::ethereum(event.transaction)?,
                address: ExternalAddress::Ethereum(event.source),
                kind: ExternalHistoryKind::Outgoing,
                timestamp: block.timestamp,
                source_asset: event.source_asset,
                source_amount: event.source_amount,
                provenance: ExternalProvenance::Ethereum,
            });
            if records.len() > 256 {
                return Err(MigrationError::InvalidHistory);
            }
        }
        for value in outgoing_transfers.iter().chain(incoming_transfers) {
            let Some(transfer) = parse_transfer(value, claim.address)? else {
                continue;
            };
            if !(claim.from_block..=claim.to_block).contains(&transfer.block_number) {
                return Err(MigrationError::RpcResponseMismatch);
            }
            let block = self.block_by_number(transfer.block_number)?;
            if block.hash != transfer.block_hash {
                return Err(MigrationError::SourceDisplaced);
            }
            records.push(ExternalHistoryRecord {
                chain: SourceChain::Ethereum {
                    chain_id: self.config.chain_id,
                },
                transaction: SourceTransaction::ethereum(transfer.transaction)?,
                address: ExternalAddress::Ethereum(claim.address),
                kind: transfer.kind,
                timestamp: block.timestamp,
                source_asset: transfer.asset,
                source_amount: transfer.amount,
                provenance: ExternalProvenance::Ethereum,
            });
            if records.len() > 256 {
                return Err(MigrationError::InvalidHistory);
            }
        }
        records.sort_by_key(|record| {
            (
                record.timestamp,
                record.transaction,
                record.source_asset,
                record.kind,
            )
        });
        records.dedup_by_key(|record| (record.transaction, record.source_asset, record.kind));
        let next = claim.to_block.saturating_add(1);
        let mut cursor_context = Vec::new();
        cursor_context.extend_from_slice(&self.config.chain_id.to_be_bytes());
        cursor_context.extend_from_slice(&claim.address);
        cursor_context.extend_from_slice(&next.to_be_bytes());
        cursor_context.extend_from_slice(&range_anchor.hash);
        let cursor = self.journal.cursor(&cursor_context);
        self.journal.prepare_history(
            &stream,
            claim.previous_cursor,
            claim.from_block,
            claim.to_block,
            range_anchor.hash,
            evidence_digest,
            cursor,
        )?;
        Ok(VerifiedHistoryPage {
            records,
            next_cursor: Some(cursor),
            evidence_digest,
        })
    }
}

impl SourceVerifier for EthereumVerifier {
    fn verify_ownership(
        &self,
        evidence: &SourceEvidence,
        _trace: &TraceId,
    ) -> Result<VerifiedOwnership, MigrationError> {
        let claim = parse_ownership(evidence)?;
        self.verify_network()?;
        if claim.chain_id != self.config.chain_id
            || claim.expires_at.saturating_sub(claim.issued_at)
                > self.config.maximum_ownership_ttl_seconds
        {
            return Err(MigrationError::InvalidNetwork);
        }
        let current = now()?;
        if current < claim.issued_at || current > claim.expires_at {
            return Err(MigrationError::InvalidEvidence);
        }
        let digest = personal_sign_digest(&claim.signing_message());
        let signature = Signature::from_slice(&claim.signature[..64])
            .map_err(|_| MigrationError::OwnershipSignatureMismatch)?;
        if signature.normalize_s().is_some() {
            return Err(MigrationError::OwnershipSignatureMismatch);
        }
        let recovery_byte = match claim.signature[64] {
            27 | 28 => claim.signature[64] - 27,
            value @ 0..=3 => value,
            _ => return Err(MigrationError::OwnershipSignatureMismatch),
        };
        let recovery = RecoveryId::from_byte(recovery_byte)
            .ok_or(MigrationError::OwnershipSignatureMismatch)?;
        let key = VerifyingKey::recover_from_prehash(&digest, &signature, recovery)
            .map_err(|_| MigrationError::OwnershipSignatureMismatch)?;
        let point = key.to_encoded_point(false);
        let public = point
            .as_bytes()
            .get(1..)
            .ok_or(MigrationError::OwnershipSignatureMismatch)?;
        let recovered = Keccak256::digest(public);
        if recovered[12..] != claim.address {
            return Err(MigrationError::OwnershipSignatureMismatch);
        }
        let ownership_key = format!(
            "ethereum:{}:{}:{}",
            self.config.chain_id,
            hex(&claim.address),
            hex(&claim.nonce)
        );
        self.journal
            .record_ownership(&ownership_key, evidence.digest())?;
        Ok(VerifiedOwnership {
            chain: SourceChain::Ethereum {
                chain_id: self.config.chain_id,
            },
            address: ExternalAddress::Ethereum(claim.address),
            layerx_identity: claim.layerx_identity,
            evidence_digest: evidence.digest(),
        })
    }

    fn verify_asset_finality(
        &self,
        evidence: &SourceEvidence,
        _trace: &TraceId,
    ) -> Result<VerifiedAssetFinality, MigrationError> {
        let claim = parse_asset(evidence)?;
        if claim.chain_id != self.config.chain_id {
            return Err(MigrationError::InvalidNetwork);
        }
        let (event, head) = self.verify_asset(claim)?;
        self.journal.record_claim(
            &format!(
                "ethereum:{}:{}",
                self.config.chain_id,
                hex(&event.transaction)
            ),
            event.block_number,
            event.block_hash,
            evidence.digest(),
        )?;
        let mut reference_claim = Sha256::new();
        reference_claim.update(b"LayerX/migration/ethereum/custody-reference/v1\0");
        reference_claim.update(event.transaction);
        reference_claim.update(evidence.digest());
        self.journal.record_custody_reference(
            &format!(
                "ethereum:{}:{}:{}",
                self.config.chain_id,
                hex(&self.config.custody_contract),
                hex(&event.custody_reference)
            ),
            reference_claim.finalize().into(),
        )?;
        Ok(VerifiedAssetFinality {
            chain: SourceChain::Ethereum {
                chain_id: self.config.chain_id,
            },
            transaction: SourceTransaction::ethereum(event.transaction)?,
            source: ExternalAddress::Ethereum(event.source),
            source_asset: event.source_asset,
            source_amount: event.source_amount,
            custody_reference: event.custody_reference,
            layerx_asset: event.layerx_asset,
            layerx_amount: event.layerx_amount,
            destination: event.destination,
            finality_height: head.number,
            evidence_digest: evidence.digest(),
        })
    }

    fn verify_history(
        &self,
        evidence: &SourceEvidence,
        _trace: &TraceId,
    ) -> Result<VerifiedHistoryPage, MigrationError> {
        self.history(parse_history(evidence)?, evidence.digest())
    }

    fn commit_history(
        &self,
        evidence: &SourceEvidence,
        page: &VerifiedHistoryPage,
        _trace: &TraceId,
    ) -> Result<(), MigrationError> {
        let claim = parse_history(evidence)?;
        if page.evidence_digest != evidence.digest() {
            return Err(MigrationError::EvidenceMismatch);
        }
        let stream = format!("ethereum:{}:{}", self.config.chain_id, hex(&claim.address));
        let cursor = page.next_cursor.ok_or(MigrationError::CheckpointConflict)?;
        self.journal
            .commit_history(&stream, evidence.digest(), cursor)
    }
}

#[derive(Clone, Debug)]
struct Block {
    number: u64,
    hash: [u8; 32],
    parent_hash: [u8; 32],
    timestamp: u64,
}

#[derive(Clone, Copy)]
struct Event {
    transaction: [u8; 32],
    block_hash: [u8; 32],
    block_number: u64,
    source: [u8; 20],
    source_asset: [u8; 32],
    source_amount: u128,
    custody_reference: [u8; 32],
    layerx_asset: [u8; 32],
    layerx_amount: u128,
    destination: [u8; 32],
}

struct Transfer {
    transaction: [u8; 32],
    block_hash: [u8; 32],
    block_number: u64,
    asset: [u8; 32],
    kind: ExternalHistoryKind,
    amount: u128,
}

fn validate_config(config: &EthereumConfig) -> Result<(), MigrationError> {
    if config.chain_id == 0
        || config.genesis_hash == [0; 32]
        || config.custody_contract == [0; 20]
        || config.custody_code_hash == [0; 32]
        || config.native_asset == [0; 32]
        || config.custody.event_topic == [0; 32]
        || config.minimum_confirmations == 0
        || config.maximum_ancestry < config.minimum_confirmations
        || !(1..=4096).contains(&config.maximum_ancestry)
        || !(1..=256).contains(&config.maximum_history_blocks)
        || !(60..=3600).contains(&config.maximum_ownership_ttl_seconds)
    {
        return Err(MigrationError::Configuration);
    }
    if let EthereumContractIdentity::Proxy {
        implementation_slot,
        implementation_address,
        implementation_code_hash,
    } = config.custody_identity
    {
        if implementation_slot == [0; 32]
            || implementation_address == [0; 20]
            || implementation_code_hash == [0; 32]
        {
            return Err(MigrationError::Configuration);
        }
    }
    let words = [
        config.custody.source,
        config.custody.source_asset,
        config.custody.source_amount,
        config.custody.custody_reference,
        config.custody.layerx_asset,
        config.custody.layerx_amount,
        config.custody.destination,
    ];
    let unique: BTreeSet<EthereumWord> = words.into_iter().collect();
    if unique.len() != words.len()
        || words.iter().any(|word| match word {
            EthereumWord::Topic(index) => !(1..=3).contains(index),
            EthereumWord::Data(index) => *index >= 64,
        })
    {
        return Err(MigrationError::Configuration);
    }
    Ok(())
}

fn parse_ownership(evidence: &SourceEvidence) -> Result<EthereumOwnershipClaim, MigrationError> {
    let mut reader = Reader::new(evidence.canonical(), OWNERSHIP_DOMAIN)?;
    let claim = EthereumOwnershipClaim {
        chain_id: reader.u64()?,
        address: reader.array()?,
        layerx_identity: reader.array()?,
        issued_at: reader.u64()?,
        expires_at: reader.u64()?,
        nonce: reader.array()?,
        signature: reader.array()?,
    };
    reader.finish()?;
    if claim.chain_id == 0
        || claim.address == [0; 20]
        || claim.layerx_identity == [0; 32]
        || claim.nonce == [0; 32]
        || claim.issued_at >= claim.expires_at
    {
        return Err(MigrationError::InvalidEvidence);
    }
    Ok(claim)
}

fn parse_asset(evidence: &SourceEvidence) -> Result<EthereumAssetClaim, MigrationError> {
    let mut reader = Reader::new(evidence.canonical(), ASSET_DOMAIN)?;
    let claim = EthereumAssetClaim {
        chain_id: reader.u64()?,
        transaction_hash: reader.array()?,
        source: reader.array()?,
        source_asset: reader.array()?,
        source_amount: reader.u128()?,
        custody_reference: reader.array()?,
        layerx_asset: reader.array()?,
        layerx_amount: reader.u128()?,
        destination: reader.array()?,
    };
    reader.finish()?;
    if claim.chain_id == 0
        || claim.transaction_hash == [0; 32]
        || claim.source == [0; 20]
        || claim.source_asset == [0; 32]
        || claim.source_amount == 0
        || claim.custody_reference == [0; 32]
        || claim.layerx_asset == [0; 32]
        || claim.layerx_amount == 0
        || claim.destination == [0; 32]
    {
        return Err(MigrationError::InvalidEvidence);
    }
    Ok(claim)
}

fn parse_history(evidence: &SourceEvidence) -> Result<EthereumHistoryClaim, MigrationError> {
    let mut reader = Reader::new(evidence.canonical(), HISTORY_DOMAIN)?;
    let claim = EthereumHistoryClaim {
        chain_id: reader.u64()?,
        address: reader.array()?,
        from_block: reader.u64()?,
        to_block: reader.u64()?,
        previous_cursor: match reader.u8()? {
            0 => None,
            1 => Some(reader.array()?),
            _ => return Err(MigrationError::InvalidHistory),
        },
    };
    reader.finish()?;
    if claim.chain_id == 0
        || claim.address == [0; 20]
        || claim.from_block == 0
        || claim.to_block < claim.from_block
        || claim.previous_cursor == Some([0; 32])
    {
        return Err(MigrationError::InvalidHistory);
    }
    Ok(claim)
}

fn parse_block(value: Value) -> Result<Block, MigrationError> {
    if value.is_null() {
        return Err(MigrationError::SourcePending);
    }
    Ok(Block {
        number: decode_quantity(string(&value, "number")?)?,
        hash: decode_fixed_hex(string(&value, "hash")?)?,
        parent_hash: decode_fixed_hex(string(&value, "parentHash")?)?,
        timestamp: decode_quantity(string(&value, "timestamp")?)?,
    })
}

fn event_word(
    topics: &[[u8; 32]],
    data: &[u8],
    location: EthereumWord,
) -> Result<[u8; 32], MigrationError> {
    match location {
        EthereumWord::Topic(index) => topics
            .get(usize::from(index))
            .copied()
            .ok_or(MigrationError::CustodyEventMismatch),
        EthereumWord::Data(index) => {
            let start = usize::from(index).saturating_mul(32);
            data.get(start..start.saturating_add(32))
                .and_then(|word| word.try_into().ok())
                .ok_or(MigrationError::CustodyEventMismatch)
        }
    }
}

fn parse_transfer(value: &Value, address: [u8; 20]) -> Result<Option<Transfer>, MigrationError> {
    if value.get("removed").and_then(Value::as_bool) == Some(true) {
        return Err(MigrationError::SourceDisplaced);
    }
    let topics = value
        .get("topics")
        .and_then(Value::as_array)
        .ok_or(MigrationError::RpcResponseMismatch)?;
    if topics.len() != 3 {
        return Ok(None);
    }
    let topics = topics
        .iter()
        .map(|topic| {
            topic
                .as_str()
                .ok_or(MigrationError::RpcResponseMismatch)
                .and_then(decode_fixed_hex)
        })
        .collect::<Result<Vec<[u8; 32]>, _>>()?;
    if topics[0] != ERC20_TRANSFER_TOPIC || topics[1][..12] != [0; 12] || topics[2][..12] != [0; 12]
    {
        return Ok(None);
    }
    let mut from = [0_u8; 20];
    from.copy_from_slice(&topics[1][12..]);
    let mut to = [0_u8; 20];
    to.copy_from_slice(&topics[2][12..]);
    let kind = if from == address && to != address {
        ExternalHistoryKind::Outgoing
    } else if to == address && from != address {
        ExternalHistoryKind::Incoming
    } else if from == address && to == address {
        ExternalHistoryKind::Contract
    } else {
        return Ok(None);
    };
    let contract = decode_fixed_hex::<20>(string(value, "address")?)?;
    if contract == [0; 20] {
        return Ok(None);
    }
    let mut asset = [0_u8; 32];
    asset[12..].copy_from_slice(&contract);
    let data = decode_fixed_hex::<32>(string(value, "data")?)?;
    if data[..16] != [0; 16] {
        return Ok(None);
    }
    let amount = u128::from_be_bytes(
        data[16..]
            .try_into()
            .map_err(|_| MigrationError::RpcResponseMismatch)?,
    );
    if amount == 0 {
        return Ok(None);
    }
    Ok(Some(Transfer {
        transaction: decode_fixed_hex(string(value, "transactionHash")?)?,
        block_hash: decode_fixed_hex(string(value, "blockHash")?)?,
        block_number: decode_quantity(string(value, "blockNumber")?)?,
        asset,
        kind,
        amount,
    }))
}

fn address_word(address: [u8; 20]) -> [u8; 32] {
    let mut word = [0_u8; 32];
    word[12..].copy_from_slice(&address);
    word
}

fn word_u128(word: [u8; 32]) -> Result<u128, MigrationError> {
    if word[..16] != [0; 16] {
        return Err(MigrationError::CustodyEventMismatch);
    }
    Ok(u128::from_be_bytes(
        word[16..]
            .try_into()
            .map_err(|_| MigrationError::CustodyEventMismatch)?,
    ))
}

fn decode_u128_quantity(value: &str) -> Result<u128, MigrationError> {
    let digits = value
        .strip_prefix("0x")
        .ok_or(MigrationError::RpcResponseMismatch)?;
    if digits.is_empty()
        || digits.len() > 32
        || (digits.len() > 1 && digits.starts_with('0'))
        || !digits.bytes().all(|byte| byte.is_ascii_hexdigit())
    {
        return Err(MigrationError::RpcResponseMismatch);
    }
    u128::from_str_radix(digits, 16).map_err(|_| MigrationError::RpcResponseMismatch)
}

fn string<'a>(value: &'a Value, key: &str) -> Result<&'a str, MigrationError> {
    value
        .get(key)
        .and_then(Value::as_str)
        .ok_or(MigrationError::RpcResponseMismatch)
}

fn personal_sign_digest(message: &[u8]) -> [u8; 32] {
    let prefix = format!("\x19Ethereum Signed Message:\n{}", message.len());
    let mut hasher = Keccak256::new();
    hasher.update(prefix.as_bytes());
    hasher.update(message);
    hasher.finalize().into()
}

fn network_key(chain_id: u64) -> String {
    format!("ethereum:{chain_id}")
}

fn now() -> Result<u64, MigrationError> {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_secs())
        .map_err(|_| MigrationError::Configuration)
}

/// Redaction-safe digest of an Ethereum asset claim, useful for operator audit
/// correlation without exposing the claim body.
#[must_use]
pub fn ethereum_claim_digest(claim: EthereumAssetClaim) -> [u8; 32] {
    let mut hash = Sha256::new();
    hash.update(ASSET_DOMAIN);
    hash.update(claim.chain_id.to_be_bytes());
    hash.update(claim.transaction_hash);
    hash.update(claim.destination);
    hash.finalize().into()
}
