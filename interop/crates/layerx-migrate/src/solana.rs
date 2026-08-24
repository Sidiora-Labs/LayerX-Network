use std::collections::{BTreeMap, BTreeSet};
use std::time::{SystemTime, UNIX_EPOCH};

use base64::engine::general_purpose::STANDARD as BASE64;
use base64::Engine as _;
use ed25519_dalek::{Signature, VerifyingKey};
use layerx_interop_gateway::trace::TraceId;
use serde::Deserialize;
use serde_json::{json, Value};
use sha2::{Digest as _, Sha256};

use crate::journal::{ChainCheckpoint, Journal, JournalConfig};
use crate::rpc::{RpcCluster, RpcQuorumConfig};
use crate::source_codec::{base58_decode, base58_encode as base58, hex, Reader, Writer};
use crate::{
    ExternalAddress, ExternalHistoryKind, ExternalHistoryRecord, ExternalProvenance,
    MigrationError, SourceChain, SourceEvidence, SourceTransaction, SourceVerifier,
    VerifiedAssetFinality, VerifiedHistoryPage, VerifiedOwnership,
};

const OWNERSHIP_DOMAIN: &[u8] = b"LXM/SOL/OWNERSHIP/1\0";
const ASSET_DOMAIN: &[u8] = b"LXM/SOL/ASSET/1\0";
const HISTORY_DOMAIN: &[u8] = b"LXM/SOL/HISTORY/1\0";
const SIGNING_DOMAIN: &[u8] = b"LayerX Solana migration ownership v1\n";
const MAX_INSTRUCTIONS: usize = 1024;
const MAX_TRANSACTIONS: usize = 100_000;
const MAX_ACCOUNTS: usize = 4096;
const MAX_INSTRUCTION_BYTES: usize = 4096;

/// Integer encoding used by the deployed custody instruction.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq)]
pub enum SolanaAmountEndian {
    Big,
    Little,
}

/// Exact account positions and byte ranges for one deployed custody
/// instruction. The verifier never guesses an instruction layout.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq)]
pub struct SolanaInstructionSchema {
    pub discriminator: Vec<u8>,
    pub source_account: u16,
    pub custody_account: u16,
    pub source_asset_account: u16,
    pub source_asset_offset: u16,
    pub source_amount_offset: u16,
    pub custody_reference_offset: u16,
    pub layerx_asset_offset: u16,
    pub layerx_amount_offset: u16,
    pub destination_offset: u16,
    pub amount_endian: SolanaAmountEndian,
}

/// Solana verifier policy, including exact cluster, deployed program, account
/// ownership, finality, transport, and durable checkpoint identities.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq)]
pub struct SolanaConfig {
    pub genesis_hash: [u8; 32],
    pub custody_program: [u8; 32],
    pub custody_program_owner: [u8; 32],
    pub custody_program_data_account: [u8; 32],
    pub custody_program_data_reference_offset: u16,
    pub custody_program_data_hash: [u8; 32],
    pub custody_account: [u8; 32],
    pub custody_account_owner: [u8; 32],
    pub custody_token_authority: [u8; 32],
    pub source_asset_account_owner: [u8; 32],
    pub native_asset: [u8; 32],
    pub custody: SolanaInstructionSchema,
    pub minimum_rooted_slots: u64,
    pub maximum_ancestry: u64,
    pub maximum_history_slots: u64,
    pub maximum_ownership_ttl_seconds: u64,
    pub rpc: RpcQuorumConfig,
    pub journal: JournalConfig,
}

/// Wallet-signed Solana ownership claim.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct SolanaOwnershipClaim {
    pub genesis_hash: [u8; 32],
    pub address: [u8; 32],
    pub layerx_identity: [u8; 32],
    pub issued_at: u64,
    pub expires_at: u64,
    pub nonce: [u8; 32],
    pub signature: [u8; 64],
}

impl SolanaOwnershipClaim {
    /// Returns the exact domain-separated bytes signed by the source account.
    #[must_use]
    pub fn signing_message(&self) -> Vec<u8> {
        let mut writer = Writer::new(SIGNING_DOMAIN);
        writer.fixed(&self.genesis_hash);
        writer.fixed(&self.address);
        writer.fixed(&self.layerx_identity);
        writer.u64(self.issued_at);
        writer.u64(self.expires_at);
        writer.fixed(&self.nonce);
        writer.finish()
    }

    /// Encodes the exact bounded ownership evidence.
    ///
    /// # Errors
    ///
    /// Refuses reserved values and invalid validity intervals.
    pub fn evidence(&self) -> Result<SourceEvidence, MigrationError> {
        validate_ownership(self)?;
        let mut writer = Writer::new(OWNERSHIP_DOMAIN);
        writer.fixed(&self.genesis_hash);
        writer.fixed(&self.address);
        writer.fixed(&self.layerx_identity);
        writer.u64(self.issued_at);
        writer.u64(self.expires_at);
        writer.fixed(&self.nonce);
        writer.fixed(&self.signature);
        SourceEvidence::new(writer.finish())
    }
}

/// Expected source transaction and custody-instruction binding.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct SolanaAssetClaim {
    pub genesis_hash: [u8; 32],
    pub signature: [u8; 64],
    pub source: [u8; 32],
    pub source_asset_account: [u8; 32],
    pub source_asset: [u8; 32],
    pub source_amount: u128,
    pub custody_reference: [u8; 32],
    pub layerx_asset: [u8; 32],
    pub layerx_amount: u128,
    pub destination: [u8; 32],
}

impl SolanaAssetClaim {
    /// Encodes one exact expected custody instruction.
    ///
    /// # Errors
    ///
    /// Refuses zero identities, signatures, assets, references, or amounts.
    pub fn evidence(self) -> Result<SourceEvidence, MigrationError> {
        validate_asset(&self)?;
        let mut writer = Writer::new(ASSET_DOMAIN);
        writer.fixed(&self.genesis_hash);
        writer.fixed(&self.signature);
        writer.fixed(&self.source);
        writer.fixed(&self.source_asset_account);
        writer.fixed(&self.source_asset);
        writer.u128(self.source_amount);
        writer.fixed(&self.custody_reference);
        writer.fixed(&self.layerx_asset);
        writer.u128(self.layerx_amount);
        writer.fixed(&self.destination);
        SourceEvidence::new(writer.finish())
    }
}

/// Ascending finalized slot range imported as external provenance.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct SolanaHistoryClaim {
    pub genesis_hash: [u8; 32],
    pub address: [u8; 32],
    pub from_slot: u64,
    pub to_slot: u64,
    pub previous_cursor: Option<[u8; 32]>,
}

impl SolanaHistoryClaim {
    /// Encodes one bounded history range.
    ///
    /// # Errors
    ///
    /// Refuses reserved identities, descending ranges, and zero cursors.
    pub fn evidence(self) -> Result<SourceEvidence, MigrationError> {
        validate_history(&self)?;
        let mut writer = Writer::new(HISTORY_DOMAIN);
        writer.fixed(&self.genesis_hash);
        writer.fixed(&self.address);
        writer.u64(self.from_slot);
        writer.u64(self.to_slot);
        if let Some(cursor) = self.previous_cursor {
            writer.u8(1);
            writer.fixed(&cursor);
        } else {
            writer.u8(0);
        }
        SourceEvidence::new(writer.finish())
    }
}

/// Production Solana verifier using finalized quorum reads and an externally
/// rollback-anchored local journal.
pub struct SolanaVerifier {
    config: SolanaConfig,
    rpc: RpcCluster,
    journal: Journal,
}

impl crate::sealed::SourceVerifier for SolanaVerifier {}

impl SolanaVerifier {
    /// Builds a verifier and reconciles its authenticated journal head with
    /// the configured non-rollbackable quorum authority.
    ///
    /// # Errors
    ///
    /// Refuses incomplete network, program, instruction, quorum, or journal
    /// policy, including overlapping instruction fields.
    pub fn new(config: SolanaConfig) -> Result<Self, MigrationError> {
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
        let value = self.rpc.call("getGenesisHash", json!([]))?;
        let observed =
            decode_base58_fixed::<32>(value.as_str().ok_or(MigrationError::RpcResponseMismatch)?)?;
        if observed != self.config.genesis_hash {
            return Err(MigrationError::InvalidNetwork);
        }
        Ok(())
    }

    fn verify_program_boundary(
        &self,
        source_asset: [u8; 32],
        minimum_slot: u64,
    ) -> Result<(), MigrationError> {
        let program = self.account(self.config.custody_program, minimum_slot)?;
        if !program.executable
            || program.owner != self.config.custody_program_owner
            || program_data_reference(
                &program.data,
                self.config.custody_program_data_reference_offset,
            )? != self.config.custody_program_data_account
        {
            return Err(MigrationError::CustodyProgramMismatch);
        }
        let program_data = self.account(self.config.custody_program_data_account, minimum_slot)?;
        let code = immutable_program_code(&program_data.data, minimum_slot)?;
        if program_data.executable
            || program_data.owner != self.config.custody_program_owner
            || <[u8; 32]>::from(Sha256::digest(code)) != self.config.custody_program_data_hash
        {
            return Err(MigrationError::CustodyProgramMismatch);
        }
        let custody = self.account(self.config.custody_account, minimum_slot)?;
        if custody.executable || custody.owner != self.config.custody_account_owner {
            return Err(MigrationError::CustodyProgramMismatch);
        }
        let asset = self.account(source_asset, minimum_slot)?;
        if asset.executable || asset.owner != self.config.source_asset_account_owner {
            return Err(MigrationError::CustodyProgramMismatch);
        }
        Ok(())
    }

    fn account(&self, address: [u8; 32], minimum_slot: u64) -> Result<Account, MigrationError> {
        let value = self.rpc.call(
            "getAccountInfo",
            json!([
                base58(&address),
                {
                    "commitment": "finalized",
                    "encoding": "base64",
                    "minContextSlot": minimum_slot
                }
            ]),
        )?;
        let context_slot = value
            .get("context")
            .and_then(|context| context.get("slot"))
            .and_then(Value::as_u64)
            .ok_or(MigrationError::RpcResponseMismatch)?;
        if context_slot < minimum_slot {
            return Err(MigrationError::RpcResponseMismatch);
        }
        let account = value
            .get("value")
            .filter(|value| !value.is_null())
            .ok_or(MigrationError::CustodyProgramMismatch)?;
        let owner = decode_base58_fixed::<32>(string(account, "owner")?)?;
        let executable = account
            .get("executable")
            .and_then(Value::as_bool)
            .ok_or(MigrationError::RpcResponseMismatch)?;
        let data = account
            .get("data")
            .and_then(Value::as_array)
            .filter(|value| value.len() == 2)
            .ok_or(MigrationError::RpcResponseMismatch)?;
        if data.get(1).and_then(Value::as_str) != Some("base64") {
            return Err(MigrationError::RpcResponseMismatch);
        }
        let data = BASE64
            .decode(
                data.first()
                    .and_then(Value::as_str)
                    .ok_or(MigrationError::RpcResponseMismatch)?,
            )
            .map_err(|_| MigrationError::RpcResponseMismatch)?;
        if data.len() > 10 * 1024 * 1024 {
            return Err(MigrationError::RpcResponseMismatch);
        }
        Ok(Account {
            owner,
            executable,
            data,
        })
    }

    fn transaction(&self, signature: [u8; 64]) -> Result<Value, MigrationError> {
        let value = self.rpc.call(
            "getTransaction",
            json!([
                base58(&signature),
                {
                    "commitment": "finalized",
                    "encoding": "jsonParsed",
                    "maxSupportedTransactionVersion": 0
                }
            ]),
        )?;
        if value.is_null() {
            return Err(MigrationError::SourcePending);
        }
        Ok(value)
    }

    fn block(&self, slot: u64) -> Result<Option<Block>, MigrationError> {
        let value = self.rpc.call(
            "getBlock",
            json!([
                slot,
                {
                    "commitment": "finalized",
                    "encoding": "jsonParsed",
                    "transactionDetails": "full",
                    "rewards": false,
                    "maxSupportedTransactionVersion": 0
                }
            ]),
        )?;
        if value.is_null() {
            return Ok(None);
        }
        parse_block(slot, value).map(Some)
    }

    fn block_at_or_before(&self, slot: u64) -> Result<Block, MigrationError> {
        let mut cursor = slot;
        for _ in 0..=self.config.maximum_ancestry {
            if let Some(block) = self.block(cursor)? {
                return Ok(block);
            }
            cursor = cursor
                .checked_sub(1)
                .ok_or(MigrationError::SourceDisplaced)?;
        }
        Err(MigrationError::FinalityWindowExceeded)
    }

    fn verify_previous_checkpoint(
        &self,
        head: &Block,
    ) -> Result<Option<ChainCheckpoint>, MigrationError> {
        let key = network_key(&self.config.genesis_hash);
        let Some(previous) = self.journal.checkpoint(&key)? else {
            return Ok(None);
        };
        if previous.height > head.slot {
            return Err(MigrationError::CheckpointConflict);
        }
        let observed = self
            .block(previous.height)?
            .ok_or(MigrationError::CheckpointConflict)?;
        if observed.hash != previous.hash
            || (previous.height == head.slot && previous.hash != head.hash)
        {
            return Err(MigrationError::CheckpointConflict);
        }
        Ok(Some(previous))
    }

    fn finality(&self, target: &Block) -> Result<Block, MigrationError> {
        let head_slot = self
            .rpc
            .call("getSlot", json!([{ "commitment": "finalized" }]))?
            .as_u64()
            .ok_or(MigrationError::RpcResponseMismatch)?;
        if target.slot > head_slot
            || head_slot.saturating_sub(target.slot) < self.config.minimum_rooted_slots
        {
            return Err(MigrationError::SourcePending);
        }
        let head = self.block_at_or_before(head_slot)?;
        let previous = self.verify_previous_checkpoint(&head)?;
        let required_slot = previous
            .as_ref()
            .map_or(target.slot, |checkpoint| checkpoint.height.min(target.slot));
        if head.slot.saturating_sub(required_slot) > self.config.maximum_ancestry {
            return Err(MigrationError::FinalityWindowExceeded);
        }
        let mut cursor = head.clone();
        let mut target_seen = false;
        let mut previous_seen = previous.is_none();
        for _ in 0..=self.config.maximum_ancestry {
            if cursor.slot == target.slot {
                if cursor.hash != target.hash {
                    return Err(MigrationError::SourceDisplaced);
                }
                target_seen = true;
            }
            if previous.as_ref().is_some_and(|checkpoint| {
                checkpoint.height == cursor.slot && checkpoint.hash == cursor.hash
            }) {
                previous_seen = true;
            }
            if cursor.slot <= required_slot {
                break;
            }
            let parent = self
                .block(cursor.parent_slot)?
                .ok_or(MigrationError::SourceDisplaced)?;
            if parent.hash != cursor.previous_hash || parent.slot != cursor.parent_slot {
                return Err(MigrationError::SourceDisplaced);
            }
            cursor = parent;
        }
        if !target_seen || !previous_seen {
            return Err(MigrationError::SourceDisplaced);
        }
        if previous
            .as_ref()
            .is_none_or(|checkpoint| checkpoint.height != head.slot)
        {
            self.journal.record_chain(
                &network_key(&self.config.genesis_hash),
                ChainCheckpoint {
                    height: head.slot,
                    hash: head.hash,
                    parent_hash: head.previous_hash,
                    previous_height: previous.as_ref().map_or(0, |value| value.height),
                    previous_hash: previous.as_ref().map_or([0; 32], |value| value.hash),
                },
            )?;
        }
        Ok(head)
    }

    fn verify_status(&self, signature: [u8; 64], slot: u64) -> Result<(), MigrationError> {
        let value = self.rpc.call(
            "getSignatureStatuses",
            json!([[base58(&signature)], { "searchTransactionHistory": true }]),
        )?;
        let status = value
            .get("value")
            .and_then(Value::as_array)
            .filter(|values| values.len() == 1)
            .and_then(|values| values.first())
            .filter(|value| !value.is_null())
            .ok_or(MigrationError::SourcePending)?;
        if status.get("err").is_some_and(|value| !value.is_null()) {
            return Err(MigrationError::SourceReverted);
        }
        if status.get("slot").and_then(Value::as_u64) != Some(slot)
            || status.get("confirmationStatus").and_then(Value::as_str) != Some("finalized")
        {
            return Err(MigrationError::SourcePending);
        }
        Ok(())
    }

    fn verify_asset(
        &self,
        claim: SolanaAssetClaim,
    ) -> Result<(Instruction, Block, Block), MigrationError> {
        self.verify_network()?;
        let transaction = self.transaction(claim.signature)?;
        let slot = transaction
            .get("slot")
            .and_then(Value::as_u64)
            .ok_or(MigrationError::RpcResponseMismatch)?;
        let body = &transaction;
        verify_success(body)?;
        let signatures = transaction_signatures(body)?;
        if signatures.first().copied() != Some(claim.signature)
            || signatures
                .iter()
                .filter(|value| **value == claim.signature)
                .count()
                != 1
        {
            return Err(MigrationError::RpcResponseMismatch);
        }
        let keys = account_keys(body)?;
        if !keys
            .iter()
            .any(|key| key.address == claim.source && key.signer)
        {
            return Err(MigrationError::CustodyProgramMismatch);
        }
        let mut matched = Vec::new();
        for value in instructions(body)? {
            if let Some(instruction) = self.parse_instruction(value, &keys, true)? {
                matched.push(instruction);
            }
        }
        if matched.len() != 1 {
            return Err(MigrationError::CustodyProgramMismatch);
        }
        let instruction = matched[0];
        if instruction.source != claim.source
            || instruction.source_asset_account != claim.source_asset_account
            || instruction.source_asset != claim.source_asset
            || instruction.source_amount != claim.source_amount
            || instruction.custody_reference != claim.custody_reference
            || instruction.layerx_asset != claim.layerx_asset
            || instruction.layerx_amount != claim.layerx_amount
            || instruction.destination != claim.destination
        {
            return Err(MigrationError::CustodyProgramMismatch);
        }
        self.verify_program_boundary(claim.source_asset_account, slot)?;
        verify_token_custody(
            body,
            &keys,
            &instruction,
            self.config.custody_account,
            self.config.custody_token_authority,
        )?;
        self.verify_status(claim.signature, slot)?;
        let block = self.block(slot)?.ok_or(MigrationError::SourcePending)?;
        let occurrence = block
            .transactions
            .iter()
            .filter(|value| {
                transaction_signatures(value)
                    .is_ok_and(|values| values.first().copied() == Some(claim.signature))
            })
            .count();
        if occurrence != 1 {
            return Err(MigrationError::SourceDisplaced);
        }
        let head = self.finality(&block)?;
        Ok((instruction, block, head))
    }

    fn parse_instruction(
        &self,
        value: &Value,
        keys: &[AccountKey],
        strict: bool,
    ) -> Result<Option<Instruction>, MigrationError> {
        let program = instruction_program(value, keys)?;
        if program != self.config.custody_program {
            return Ok(None);
        }
        let accounts = instruction_accounts(value, keys)?;
        let data = base58_decode(
            value
                .get("data")
                .and_then(Value::as_str)
                .ok_or(MigrationError::CustodyProgramMismatch)?,
            MAX_INSTRUCTION_BYTES,
        )?;
        if data.len() > MAX_INSTRUCTION_BYTES
            || !data.starts_with(&self.config.custody.discriminator)
        {
            return if strict {
                Err(MigrationError::CustodyProgramMismatch)
            } else {
                Ok(None)
            };
        }
        let source = account_at(&accounts, self.config.custody.source_account)?;
        let custody = account_at(&accounts, self.config.custody.custody_account)?;
        let source_asset_account = account_at(&accounts, self.config.custody.source_asset_account)?;
        if custody != self.config.custody_account
            || !account_privilege(keys, source)?.signer
            || !account_privilege(keys, source)?.writable
            || !account_privilege(keys, custody)?.writable
            || !account_privilege(keys, source_asset_account)?.writable
        {
            return Err(MigrationError::CustodyProgramMismatch);
        }
        Ok(Some(Instruction {
            source,
            source_asset_account,
            source_asset: read_array(&data, self.config.custody.source_asset_offset)?,
            source_amount: read_amount(
                &data,
                self.config.custody.source_amount_offset,
                self.config.custody.amount_endian,
            )?,
            custody_reference: read_array(&data, self.config.custody.custody_reference_offset)?,
            layerx_asset: read_array(&data, self.config.custody.layerx_asset_offset)?,
            layerx_amount: read_amount(
                &data,
                self.config.custody.layerx_amount_offset,
                self.config.custody.amount_endian,
            )?,
            destination: read_array(&data, self.config.custody.destination_offset)?,
        }))
    }

    fn history(
        &self,
        claim: SolanaHistoryClaim,
        evidence_digest: [u8; 32],
    ) -> Result<VerifiedHistoryPage, MigrationError> {
        self.verify_network()?;
        if claim.genesis_hash != self.config.genesis_hash
            || claim
                .to_slot
                .saturating_sub(claim.from_slot)
                .saturating_add(1)
                > self.config.maximum_history_slots
        {
            return Err(MigrationError::InvalidHistory);
        }
        let stream = format!(
            "solana:{}:{}",
            hex(&self.config.genesis_hash),
            hex(&claim.address)
        );
        self.journal.validate_history(
            &stream,
            claim.previous_cursor,
            claim.from_slot,
            claim.to_slot,
            evidence_digest,
        )?;
        let head_slot = self
            .rpc
            .call("getSlot", json!([{ "commitment": "finalized" }]))?
            .as_u64()
            .ok_or(MigrationError::RpcResponseMismatch)?;
        if claim.to_slot > head_slot
            || head_slot.saturating_sub(claim.to_slot) < self.config.minimum_rooted_slots
        {
            return Err(MigrationError::SourcePending);
        }
        let parent_anchor = self
            .journal
            .history_parent_anchor(&stream, claim.previous_cursor)?;
        let mut records = Vec::new();
        let mut previous_hash = parent_anchor;
        let mut range_anchor = None;
        for slot in claim.from_slot..=claim.to_slot {
            let Some(block) = self.block(slot)? else {
                continue;
            };
            if previous_hash.is_some_and(|hash| block.previous_hash != hash) {
                return Err(MigrationError::SourceDisplaced);
            }
            previous_hash = Some(block.hash);
            range_anchor = Some(block.hash);
            for body in &block.transactions {
                if !transaction_succeeded(body)? {
                    continue;
                }
                let keys = account_keys(body)?;
                let address_index = keys.iter().position(|key| key.address == claim.address);
                let meta = body
                    .get("meta")
                    .ok_or(MigrationError::RpcResponseMismatch)?;
                let pre_tokens = token_balances_for_owner(meta, "preTokenBalances", claim.address)?;
                let post_tokens =
                    token_balances_for_owner(meta, "postTokenBalances", claim.address)?;
                if address_index.is_none() && pre_tokens.is_empty() && post_tokens.is_empty() {
                    continue;
                }
                let signatures = transaction_signatures(body)?;
                let signature = *signatures
                    .first()
                    .ok_or(MigrationError::RpcResponseMismatch)?;
                let mut custody_record = None;
                for value in instructions(body)? {
                    if let Some(instruction) = self.parse_instruction(value, &keys, false)? {
                        if instruction.source == claim.address {
                            self.verify_program_boundary(instruction.source_asset_account, slot)?;
                            verify_token_custody(
                                body,
                                &keys,
                                &instruction,
                                self.config.custody_account,
                                self.config.custody_token_authority,
                            )?;
                            custody_record = Some(instruction);
                        }
                    }
                }
                let transaction = transaction_id(signature)?;
                let mut produced = false;
                if let Some(address_index) = address_index {
                    let before = balance_at(meta, "preBalances", address_index)?;
                    let after = balance_at(meta, "postBalances", address_index)?;
                    if after != before {
                        records.push(ExternalHistoryRecord {
                            chain: SourceChain::Solana {
                                genesis_hash: self.config.genesis_hash,
                            },
                            transaction,
                            address: ExternalAddress::Solana(claim.address),
                            kind: if after > before {
                                ExternalHistoryKind::Incoming
                            } else {
                                ExternalHistoryKind::Outgoing
                            },
                            timestamp: block.timestamp,
                            source_asset: self.config.native_asset,
                            source_amount: u128::from(after.abs_diff(before)),
                            provenance: ExternalProvenance::Solana,
                        });
                        produced = true;
                    }
                }
                let assets: BTreeSet<_> = pre_tokens
                    .keys()
                    .chain(post_tokens.keys())
                    .copied()
                    .collect();
                for asset in assets {
                    let before = pre_tokens.get(&asset).copied().unwrap_or(0);
                    let after = post_tokens.get(&asset).copied().unwrap_or(0);
                    if before == after {
                        continue;
                    }
                    records.push(ExternalHistoryRecord {
                        chain: SourceChain::Solana {
                            genesis_hash: self.config.genesis_hash,
                        },
                        transaction,
                        address: ExternalAddress::Solana(claim.address),
                        kind: if after > before {
                            ExternalHistoryKind::Incoming
                        } else {
                            ExternalHistoryKind::Outgoing
                        },
                        timestamp: block.timestamp,
                        source_asset: asset,
                        source_amount: after.abs_diff(before),
                        provenance: ExternalProvenance::Solana,
                    });
                    produced = true;
                }
                if !produced && (address_index.is_some() || custody_record.is_some()) {
                    records.push(ExternalHistoryRecord {
                        chain: SourceChain::Solana {
                            genesis_hash: self.config.genesis_hash,
                        },
                        transaction,
                        address: ExternalAddress::Solana(claim.address),
                        kind: ExternalHistoryKind::Contract,
                        timestamp: block.timestamp,
                        source_asset: custody_record
                            .map_or(self.config.native_asset, |value| value.source_asset),
                        source_amount: 0,
                        provenance: ExternalProvenance::Solana,
                    });
                }
                if records.len() > 256 {
                    return Err(MigrationError::InvalidHistory);
                }
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
        let range_anchor = range_anchor.or(parent_anchor).map_or_else(
            || {
                self.block_at_or_before(claim.to_slot)
                    .map(|block| block.hash)
            },
            Ok,
        )?;
        let next = claim.to_slot.saturating_add(1);
        let mut cursor_context = Vec::new();
        cursor_context.extend_from_slice(&self.config.genesis_hash);
        cursor_context.extend_from_slice(&claim.address);
        cursor_context.extend_from_slice(&next.to_be_bytes());
        cursor_context.extend_from_slice(&range_anchor);
        let cursor = self.journal.cursor(&cursor_context);
        self.journal.prepare_history(
            &stream,
            claim.previous_cursor,
            claim.from_slot,
            claim.to_slot,
            range_anchor,
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

impl SourceVerifier for SolanaVerifier {
    fn verify_ownership(
        &self,
        evidence: &SourceEvidence,
        _trace: &TraceId,
    ) -> Result<VerifiedOwnership, MigrationError> {
        let claim = parse_ownership(evidence)?;
        self.verify_network()?;
        if claim.genesis_hash != self.config.genesis_hash
            || claim.expires_at.saturating_sub(claim.issued_at)
                > self.config.maximum_ownership_ttl_seconds
        {
            return Err(MigrationError::InvalidNetwork);
        }
        let current = now()?;
        if current < claim.issued_at || current > claim.expires_at {
            return Err(MigrationError::InvalidEvidence);
        }
        let key = VerifyingKey::from_bytes(&claim.address)
            .map_err(|_| MigrationError::OwnershipSignatureMismatch)?;
        let signature = Signature::from_bytes(&claim.signature);
        key.verify_strict(&claim.signing_message(), &signature)
            .map_err(|_| MigrationError::OwnershipSignatureMismatch)?;
        self.journal.record_ownership(
            &format!(
                "solana:{}:{}:{}",
                hex(&self.config.genesis_hash),
                hex(&claim.address),
                hex(&claim.nonce)
            ),
            evidence.digest(),
        )?;
        Ok(VerifiedOwnership {
            chain: SourceChain::Solana {
                genesis_hash: self.config.genesis_hash,
            },
            address: ExternalAddress::Solana(claim.address),
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
        if claim.genesis_hash != self.config.genesis_hash {
            return Err(MigrationError::InvalidNetwork);
        }
        let (instruction, block, head) = self.verify_asset(claim)?;
        let transaction = transaction_id(claim.signature)?;
        self.journal.record_claim(
            &format!(
                "solana:{}:{}",
                hex(&self.config.genesis_hash),
                hex(transaction.bytes())
            ),
            block.slot,
            block.hash,
            evidence.digest(),
        )?;
        let mut reference_claim = Sha256::new();
        reference_claim.update(b"LayerX/migration/solana/custody-reference/v1\0");
        reference_claim.update(transaction.bytes());
        reference_claim.update(evidence.digest());
        self.journal.record_custody_reference(
            &format!(
                "solana:{}:{}:{}",
                hex(&self.config.genesis_hash),
                hex(&self.config.custody_program),
                hex(&instruction.custody_reference)
            ),
            reference_claim.finalize().into(),
        )?;
        Ok(VerifiedAssetFinality {
            chain: SourceChain::Solana {
                genesis_hash: self.config.genesis_hash,
            },
            transaction,
            source: ExternalAddress::Solana(instruction.source),
            source_asset: instruction.source_asset,
            source_amount: instruction.source_amount,
            custody_reference: instruction.custody_reference,
            layerx_asset: instruction.layerx_asset,
            layerx_amount: instruction.layerx_amount,
            destination: instruction.destination,
            finality_height: head.slot,
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
        let cursor = page.next_cursor.ok_or(MigrationError::CheckpointConflict)?;
        self.journal.commit_history(
            &format!(
                "solana:{}:{}",
                hex(&self.config.genesis_hash),
                hex(&claim.address)
            ),
            evidence.digest(),
            cursor,
        )
    }
}

#[derive(Clone)]
struct Block {
    slot: u64,
    hash: [u8; 32],
    previous_hash: [u8; 32],
    parent_slot: u64,
    timestamp: u64,
    transactions: Vec<Value>,
}

struct Account {
    owner: [u8; 32],
    executable: bool,
    data: Vec<u8>,
}

#[derive(Clone, Copy)]
struct AccountKey {
    address: [u8; 32],
    signer: bool,
    writable: bool,
}

#[derive(Clone, Copy)]
struct Instruction {
    source: [u8; 32],
    source_asset_account: [u8; 32],
    source_asset: [u8; 32],
    source_amount: u128,
    custody_reference: [u8; 32],
    layerx_asset: [u8; 32],
    layerx_amount: u128,
    destination: [u8; 32],
}

fn validate_config(config: &SolanaConfig) -> Result<(), MigrationError> {
    if config.genesis_hash == [0; 32]
        || config.custody_program == [0; 32]
        || config.custody_program_owner == [0; 32]
        || config.custody_program_data_account == [0; 32]
        || config.custody_program_data_reference_offset != 4
        || config.custody_program_data_hash == [0; 32]
        || config.custody_account == [0; 32]
        || config.custody_account_owner == [0; 32]
        || config.custody_token_authority == [0; 32]
        || config.source_asset_account_owner == [0; 32]
        || config.native_asset == [0; 32]
        || config.custody.discriminator.is_empty()
        || config.custody.discriminator.len() > 64
        || config.minimum_rooted_slots == 0
        || config.maximum_ancestry < config.minimum_rooted_slots
        || !(1..=4096).contains(&config.maximum_ancestry)
        || !(1..=256).contains(&config.maximum_history_slots)
        || !(60..=3600).contains(&config.maximum_ownership_ttl_seconds)
    {
        return Err(MigrationError::Configuration);
    }
    let account_positions = [
        config.custody.source_account,
        config.custody.custody_account,
        config.custody.source_asset_account,
    ];
    if account_positions.into_iter().collect::<BTreeSet<_>>().len() != account_positions.len()
        || account_positions
            .iter()
            .any(|position| usize::from(*position) >= MAX_ACCOUNTS)
    {
        return Err(MigrationError::Configuration);
    }
    let fields = [
        (config.custody.source_asset_offset, 32_usize),
        (config.custody.source_amount_offset, 16_usize),
        (config.custody.custody_reference_offset, 32),
        (config.custody.layerx_asset_offset, 32),
        (config.custody.layerx_amount_offset, 16),
        (config.custody.destination_offset, 32),
    ];
    let mut occupied = BTreeSet::new();
    for (offset, length) in fields {
        let start = usize::from(offset);
        let end = start
            .checked_add(length)
            .ok_or(MigrationError::Configuration)?;
        if start < config.custody.discriminator.len() || end > MAX_INSTRUCTION_BYTES {
            return Err(MigrationError::Configuration);
        }
        for byte in start..end {
            if !occupied.insert(byte) {
                return Err(MigrationError::Configuration);
            }
        }
    }
    Ok(())
}

fn parse_ownership(evidence: &SourceEvidence) -> Result<SolanaOwnershipClaim, MigrationError> {
    let mut reader = Reader::new(evidence.canonical(), OWNERSHIP_DOMAIN)?;
    let claim = SolanaOwnershipClaim {
        genesis_hash: reader.array()?,
        address: reader.array()?,
        layerx_identity: reader.array()?,
        issued_at: reader.u64()?,
        expires_at: reader.u64()?,
        nonce: reader.array()?,
        signature: reader.array()?,
    };
    reader.finish()?;
    validate_ownership(&claim)?;
    Ok(claim)
}

fn validate_ownership(claim: &SolanaOwnershipClaim) -> Result<(), MigrationError> {
    if claim.genesis_hash == [0; 32]
        || claim.address == [0; 32]
        || claim.layerx_identity == [0; 32]
        || claim.nonce == [0; 32]
        || claim.signature == [0; 64]
        || claim.issued_at >= claim.expires_at
    {
        return Err(MigrationError::InvalidEvidence);
    }
    Ok(())
}

fn parse_asset(evidence: &SourceEvidence) -> Result<SolanaAssetClaim, MigrationError> {
    let mut reader = Reader::new(evidence.canonical(), ASSET_DOMAIN)?;
    let claim = SolanaAssetClaim {
        genesis_hash: reader.array()?,
        signature: reader.array()?,
        source: reader.array()?,
        source_asset_account: reader.array()?,
        source_asset: reader.array()?,
        source_amount: reader.u128()?,
        custody_reference: reader.array()?,
        layerx_asset: reader.array()?,
        layerx_amount: reader.u128()?,
        destination: reader.array()?,
    };
    reader.finish()?;
    validate_asset(&claim)?;
    Ok(claim)
}

fn validate_asset(claim: &SolanaAssetClaim) -> Result<(), MigrationError> {
    if claim.genesis_hash == [0; 32]
        || claim.signature == [0; 64]
        || claim.source == [0; 32]
        || claim.source_asset_account == [0; 32]
        || claim.source_asset == [0; 32]
        || claim.source_amount == 0
        || claim.custody_reference == [0; 32]
        || claim.layerx_asset == [0; 32]
        || claim.layerx_amount == 0
        || claim.destination == [0; 32]
    {
        return Err(MigrationError::InvalidEvidence);
    }
    Ok(())
}

fn parse_history(evidence: &SourceEvidence) -> Result<SolanaHistoryClaim, MigrationError> {
    let mut reader = Reader::new(evidence.canonical(), HISTORY_DOMAIN)?;
    let claim = SolanaHistoryClaim {
        genesis_hash: reader.array()?,
        address: reader.array()?,
        from_slot: reader.u64()?,
        to_slot: reader.u64()?,
        previous_cursor: match reader.u8()? {
            0 => None,
            1 => Some(reader.array()?),
            _ => return Err(MigrationError::InvalidHistory),
        },
    };
    reader.finish()?;
    validate_history(&claim)?;
    Ok(claim)
}

fn validate_history(claim: &SolanaHistoryClaim) -> Result<(), MigrationError> {
    if claim.genesis_hash == [0; 32]
        || claim.address == [0; 32]
        || claim.from_slot == 0
        || claim.to_slot < claim.from_slot
        || claim.previous_cursor == Some([0; 32])
    {
        return Err(MigrationError::InvalidHistory);
    }
    Ok(())
}

fn parse_block(slot: u64, value: Value) -> Result<Block, MigrationError> {
    let transactions = value
        .get("transactions")
        .and_then(Value::as_array)
        .ok_or(MigrationError::RpcResponseMismatch)?;
    if transactions.len() > MAX_TRANSACTIONS {
        return Err(MigrationError::RpcResponseMismatch);
    }
    Ok(Block {
        slot,
        hash: decode_base58_fixed(string(&value, "blockhash")?)?,
        previous_hash: decode_base58_fixed(string(&value, "previousBlockhash")?)?,
        parent_slot: value
            .get("parentSlot")
            .and_then(Value::as_u64)
            .ok_or(MigrationError::RpcResponseMismatch)?,
        timestamp: value
            .get("blockTime")
            .and_then(Value::as_u64)
            .ok_or(MigrationError::RpcResponseMismatch)?,
        transactions: transactions.clone(),
    })
}

fn transaction_body(value: &Value) -> Result<&Value, MigrationError> {
    value
        .get("transaction")
        .ok_or(MigrationError::RpcResponseMismatch)
}

fn verify_success(body: &Value) -> Result<(), MigrationError> {
    if !transaction_succeeded(body)? {
        return Err(MigrationError::SourceReverted);
    }
    Ok(())
}

fn transaction_succeeded(body: &Value) -> Result<bool, MigrationError> {
    let meta = body
        .get("meta")
        .ok_or(MigrationError::RpcResponseMismatch)?;
    Ok(meta.get("err").is_some_and(Value::is_null))
}

fn transaction_signatures(body: &Value) -> Result<Vec<[u8; 64]>, MigrationError> {
    let transaction = transaction_body(body).unwrap_or(body);
    let signatures = transaction
        .get("signatures")
        .and_then(Value::as_array)
        .ok_or(MigrationError::RpcResponseMismatch)?;
    if signatures.is_empty() || signatures.len() > 256 {
        return Err(MigrationError::RpcResponseMismatch);
    }
    signatures
        .iter()
        .map(|value| {
            value
                .as_str()
                .ok_or(MigrationError::RpcResponseMismatch)
                .and_then(decode_base58_fixed)
        })
        .collect()
}

fn account_keys(body: &Value) -> Result<Vec<AccountKey>, MigrationError> {
    let transaction = transaction_body(body).unwrap_or(body);
    let message = transaction
        .get("message")
        .ok_or(MigrationError::RpcResponseMismatch)?;
    let values = message
        .get("accountKeys")
        .and_then(Value::as_array)
        .ok_or(MigrationError::RpcResponseMismatch)?;
    if values.is_empty() || values.len() > MAX_ACCOUNTS {
        return Err(MigrationError::RpcResponseMismatch);
    }
    let header = message.get("header");
    let required = header
        .and_then(|value| value.get("numRequiredSignatures"))
        .and_then(Value::as_u64)
        .and_then(|value| usize::try_from(value).ok());
    let readonly_signed = header
        .and_then(|value| value.get("numReadonlySignedAccounts"))
        .and_then(Value::as_u64)
        .and_then(|value| usize::try_from(value).ok());
    let readonly_unsigned = header
        .and_then(|value| value.get("numReadonlyUnsignedAccounts"))
        .and_then(Value::as_u64)
        .and_then(|value| usize::try_from(value).ok());
    let has_lookup_tables = message
        .get("addressTableLookups")
        .and_then(Value::as_array)
        .is_some_and(|values| !values.is_empty());
    values
        .iter()
        .enumerate()
        .map(|(index, value)| {
            if let Some(address) = value.as_str() {
                let required = required.ok_or(MigrationError::RpcResponseMismatch)?;
                let readonly_signed = readonly_signed.ok_or(MigrationError::RpcResponseMismatch)?;
                let readonly_unsigned =
                    readonly_unsigned.ok_or(MigrationError::RpcResponseMismatch)?;
                if has_lookup_tables
                    || readonly_signed > required
                    || readonly_unsigned > values.len().saturating_sub(required)
                {
                    return Err(MigrationError::RpcResponseMismatch);
                }
                Ok(AccountKey {
                    address: decode_base58_fixed(address)?,
                    signer: index < required,
                    writable: if index < required {
                        index < required.saturating_sub(readonly_signed)
                    } else {
                        index < values.len().saturating_sub(readonly_unsigned)
                    },
                })
            } else {
                Ok(AccountKey {
                    address: decode_base58_fixed(string(value, "pubkey")?)?,
                    signer: value
                        .get("signer")
                        .and_then(Value::as_bool)
                        .ok_or(MigrationError::RpcResponseMismatch)?,
                    writable: value
                        .get("writable")
                        .and_then(Value::as_bool)
                        .ok_or(MigrationError::RpcResponseMismatch)?,
                })
            }
        })
        .collect()
}

fn instructions(body: &Value) -> Result<&[Value], MigrationError> {
    let transaction = transaction_body(body).unwrap_or(body);
    let values = transaction
        .get("message")
        .and_then(|message| message.get("instructions"))
        .and_then(Value::as_array)
        .ok_or(MigrationError::RpcResponseMismatch)?;
    if values.len() > MAX_INSTRUCTIONS {
        return Err(MigrationError::RpcResponseMismatch);
    }
    Ok(values)
}

fn instruction_program(value: &Value, keys: &[AccountKey]) -> Result<[u8; 32], MigrationError> {
    if let Some(program) = value.get("programId").and_then(Value::as_str) {
        return decode_base58_fixed(program);
    }
    let index = value
        .get("programIdIndex")
        .and_then(Value::as_u64)
        .and_then(|value| usize::try_from(value).ok())
        .ok_or(MigrationError::RpcResponseMismatch)?;
    keys.get(index)
        .map(|key| key.address)
        .ok_or(MigrationError::RpcResponseMismatch)
}

fn instruction_accounts(
    value: &Value,
    keys: &[AccountKey],
) -> Result<Vec<[u8; 32]>, MigrationError> {
    let values = value
        .get("accounts")
        .and_then(Value::as_array)
        .ok_or(MigrationError::RpcResponseMismatch)?;
    if values.len() > MAX_ACCOUNTS {
        return Err(MigrationError::RpcResponseMismatch);
    }
    values
        .iter()
        .map(|value| {
            if let Some(address) = value.as_str() {
                decode_base58_fixed(address)
            } else {
                let index = value
                    .as_u64()
                    .and_then(|value| usize::try_from(value).ok())
                    .ok_or(MigrationError::RpcResponseMismatch)?;
                keys.get(index)
                    .map(|key| key.address)
                    .ok_or(MigrationError::RpcResponseMismatch)
            }
        })
        .collect()
}

fn account_at(accounts: &[[u8; 32]], position: u16) -> Result<[u8; 32], MigrationError> {
    accounts
        .get(usize::from(position))
        .copied()
        .ok_or(MigrationError::CustodyProgramMismatch)
}

fn account_privilege(keys: &[AccountKey], address: [u8; 32]) -> Result<AccountKey, MigrationError> {
    let mut matches = keys.iter().copied().filter(|key| key.address == address);
    let key = matches
        .next()
        .ok_or(MigrationError::CustodyProgramMismatch)?;
    if matches.next().is_some() {
        return Err(MigrationError::CustodyProgramMismatch);
    }
    Ok(key)
}

fn read_array(data: &[u8], offset: u16) -> Result<[u8; 32], MigrationError> {
    let start = usize::from(offset);
    data.get(start..start.saturating_add(32))
        .and_then(|value| value.try_into().ok())
        .ok_or(MigrationError::CustodyProgramMismatch)
}

fn immutable_program_code(data: &[u8], transaction_slot: u64) -> Result<&[u8], MigrationError> {
    let variant = data
        .get(..4)
        .and_then(|value| value.try_into().ok())
        .map(u32::from_le_bytes)
        .ok_or(MigrationError::CustodyProgramMismatch)?;
    let deployment_slot = data
        .get(4..12)
        .and_then(|value| value.try_into().ok())
        .map(u64::from_le_bytes)
        .ok_or(MigrationError::CustodyProgramMismatch)?;
    if variant != 3
        || deployment_slot > transaction_slot
        || data.get(12).copied() != Some(0)
        || data.len() <= 13
    {
        return Err(MigrationError::CustodyProgramMismatch);
    }
    Ok(&data[13..])
}

fn program_data_reference(data: &[u8], offset: u16) -> Result<[u8; 32], MigrationError> {
    let variant = data
        .get(..4)
        .and_then(|value| value.try_into().ok())
        .map(u32::from_le_bytes)
        .ok_or(MigrationError::CustodyProgramMismatch)?;
    if variant != 2 || offset != 4 || data.len() != 36 {
        return Err(MigrationError::CustodyProgramMismatch);
    }
    read_array(data, offset)
}

fn read_amount(
    data: &[u8],
    offset: u16,
    endian: SolanaAmountEndian,
) -> Result<u128, MigrationError> {
    let start = usize::from(offset);
    let bytes: [u8; 16] = data
        .get(start..start.saturating_add(16))
        .and_then(|value| value.try_into().ok())
        .ok_or(MigrationError::CustodyProgramMismatch)?;
    Ok(match endian {
        SolanaAmountEndian::Big => u128::from_be_bytes(bytes),
        SolanaAmountEndian::Little => u128::from_le_bytes(bytes),
    })
}

fn balance_at(meta: &Value, key: &str, index: usize) -> Result<u64, MigrationError> {
    meta.get(key)
        .and_then(Value::as_array)
        .and_then(|values| values.get(index))
        .and_then(Value::as_u64)
        .ok_or(MigrationError::RpcResponseMismatch)
}

fn verify_token_custody(
    body: &Value,
    keys: &[AccountKey],
    instruction: &Instruction,
    custody_account: [u8; 32],
    custody_authority: [u8; 32],
) -> Result<(), MigrationError> {
    let source_index = unique_account_index(keys, instruction.source_asset_account)?;
    let custody_index = unique_account_index(keys, custody_account)?;
    let meta = body
        .get("meta")
        .ok_or(MigrationError::RpcResponseMismatch)?;
    let source_before = token_balance(meta, "preTokenBalances", source_index)?;
    let source_after = token_balance(meta, "postTokenBalances", source_index)?;
    let custody_before = token_balance(meta, "preTokenBalances", custody_index)?;
    let custody_after = token_balance(meta, "postTokenBalances", custody_index)?;
    if source_before.mint != instruction.source_asset
        || source_after.mint != instruction.source_asset
        || custody_before.mint != instruction.source_asset
        || custody_after.mint != instruction.source_asset
        || source_before.owner != instruction.source
        || source_after.owner != instruction.source
        || custody_before.owner != custody_authority
        || custody_after.owner != custody_authority
        || source_before.amount.checked_sub(source_after.amount) != Some(instruction.source_amount)
        || custody_after.amount.checked_sub(custody_before.amount)
            != Some(instruction.source_amount)
    {
        return Err(MigrationError::CustodyProgramMismatch);
    }
    Ok(())
}

#[derive(Clone, Copy)]
struct TokenBalance {
    mint: [u8; 32],
    owner: [u8; 32],
    amount: u128,
}

fn token_balance(
    meta: &Value,
    field: &str,
    account_index: usize,
) -> Result<TokenBalance, MigrationError> {
    let values = meta
        .get(field)
        .and_then(Value::as_array)
        .ok_or(MigrationError::RpcResponseMismatch)?;
    let mut matches = values.iter().filter(|value| {
        value
            .get("accountIndex")
            .and_then(Value::as_u64)
            .and_then(|value| usize::try_from(value).ok())
            == Some(account_index)
    });
    let value = matches
        .next()
        .ok_or(MigrationError::CustodyProgramMismatch)?;
    if matches.next().is_some() {
        return Err(MigrationError::RpcResponseMismatch);
    }
    let amount = value
        .get("uiTokenAmount")
        .and_then(|amount| amount.get("amount"))
        .and_then(Value::as_str)
        .ok_or(MigrationError::RpcResponseMismatch)?;
    if amount.is_empty() || amount.len() > 39 || !amount.bytes().all(|byte| byte.is_ascii_digit()) {
        return Err(MigrationError::RpcResponseMismatch);
    }
    Ok(TokenBalance {
        mint: decode_base58_fixed(string(value, "mint")?)?,
        owner: decode_base58_fixed(string(value, "owner")?)?,
        amount: amount
            .parse()
            .map_err(|_| MigrationError::RpcResponseMismatch)?,
    })
}

fn token_balances_for_owner(
    meta: &Value,
    field: &str,
    owner: [u8; 32],
) -> Result<BTreeMap<[u8; 32], u128>, MigrationError> {
    let values = meta
        .get(field)
        .and_then(Value::as_array)
        .ok_or(MigrationError::RpcResponseMismatch)?;
    if values.len() > MAX_ACCOUNTS {
        return Err(MigrationError::RpcResponseMismatch);
    }
    let mut balances = BTreeMap::new();
    for value in values {
        let Some(observed_owner) = value.get("owner").and_then(Value::as_str) else {
            continue;
        };
        let observed_owner = decode_base58_fixed::<32>(observed_owner)?;
        if observed_owner != owner {
            continue;
        }
        let mint = decode_base58_fixed::<32>(string(value, "mint")?)?;
        let amount = value
            .get("uiTokenAmount")
            .and_then(|amount| amount.get("amount"))
            .and_then(Value::as_str)
            .ok_or(MigrationError::RpcResponseMismatch)?;
        if amount.is_empty()
            || amount.len() > 39
            || !amount.bytes().all(|byte| byte.is_ascii_digit())
        {
            return Err(MigrationError::RpcResponseMismatch);
        }
        let amount = amount
            .parse::<u128>()
            .map_err(|_| MigrationError::RpcResponseMismatch)?;
        let entry = balances.entry(mint).or_insert(0_u128);
        *entry = entry
            .checked_add(amount)
            .ok_or(MigrationError::RpcResponseMismatch)?;
    }
    Ok(balances)
}

fn unique_account_index(keys: &[AccountKey], address: [u8; 32]) -> Result<usize, MigrationError> {
    let mut matches = keys
        .iter()
        .enumerate()
        .filter(|(_, key)| key.address == address);
    let index = matches
        .next()
        .map(|(index, _)| index)
        .ok_or(MigrationError::CustodyProgramMismatch)?;
    if matches.next().is_some() {
        return Err(MigrationError::RpcResponseMismatch);
    }
    Ok(index)
}

fn decode_base58_fixed<const N: usize>(value: &str) -> Result<[u8; N], MigrationError> {
    base58_decode(value, N)?
        .try_into()
        .map_err(|_| MigrationError::RpcResponseMismatch)
}

fn transaction_id(signature: [u8; 64]) -> Result<SourceTransaction, MigrationError> {
    SourceTransaction::solana(signature)
}

fn string<'a>(value: &'a Value, key: &str) -> Result<&'a str, MigrationError> {
    value
        .get(key)
        .and_then(Value::as_str)
        .ok_or(MigrationError::RpcResponseMismatch)
}

fn network_key(genesis_hash: &[u8; 32]) -> String {
    format!("solana:{}", hex(genesis_hash))
}

fn now() -> Result<u64, MigrationError> {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_secs())
        .map_err(|_| MigrationError::Configuration)
}

/// Redaction-safe digest of a Solana asset claim for operator correlation.
#[must_use]
pub fn solana_claim_digest(claim: SolanaAssetClaim) -> [u8; 32] {
    let mut hash = Sha256::new();
    hash.update(ASSET_DOMAIN);
    hash.update(claim.genesis_hash);
    hash.update(claim.signature);
    hash.update(claim.destination);
    hash.finalize().into()
}
