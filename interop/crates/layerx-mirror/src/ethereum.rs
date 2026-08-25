//! Production Ethereum immutable chunk/manifest archive client.

use std::path::PathBuf;

use serde_json::{json, Value};
use sha2::{Digest as _, Sha256};
use sha3::Keccak256;

use crate::rpc::{BroadcastResult, RpcCluster, RpcError, RpcQuorumConfig};
use crate::signer::{ChainSignature, RemoteChainSigner, SignerError};
use crate::store::{
    FinalityPosition, MirrorChain, PublicationJournal, PublicationPhase, PublicationRecord,
    PublicationStage, StoreError, TransactionIdentity,
};
use crate::{archive_commitment, Archive, ArchiveCommitment, MirrorCursor};

const ETHEREUM_TX_TYPE: u8 = 2;
const MAX_CALL_DATA_BYTES: usize = 128 * 1024;
const MIN_CHUNK_BYTES: usize = 1024;
const MAX_CHUNK_BYTES: usize = 24 * 1024;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct EthereumProductionConfig {
    pub rpc: RpcQuorumConfig,
    pub chain_id: u64,
    pub genesis_hash: [u8; 32],
    pub archive_contract: [u8; 20],
    pub archive_code_hash: [u8; 32],
    pub first_batch_number: u64,
    pub required_confirmations: u64,
    pub maximum_reorg_depth: u64,
    pub chunk_bytes: usize,
    pub transaction_gas_limit: u64,
    pub maximum_fee_per_gas: u128,
    pub maximum_priority_fee_per_gas: u128,
    pub journal_directory: PathBuf,
}

/// Immutable read-only target used by SDK and explorer mirror verification.
/// The publisher and contract identity are operator configuration and are
/// never accepted from a verification request or archive payload.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct EthereumMirrorReadConfig {
    pub rpc: RpcQuorumConfig,
    pub chain_id: u64,
    pub genesis_hash: [u8; 32],
    pub archive_contract: [u8; 20],
    pub archive_code_hash: [u8; 32],
    pub publisher: [u8; 20],
}

/// Canonical finalized chain coordinate at which an archive was read.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct EthereumMirrorRead {
    pub archive: Vec<u8>,
    pub block_number: u64,
    pub block_hash: [u8; 32],
    pub reference_head_number: u64,
    pub reference_head_hash: [u8; 32],
}

/// Production read-only mirror client. It shares the exact ABI encoders and
/// decoders used by the publisher rather than maintaining a second ABI.
pub struct EthereumMirrorReader {
    config: EthereumMirrorReadConfig,
    rpc: RpcCluster,
}

impl EthereumMirrorReader {
    pub fn open(config: EthereumMirrorReadConfig) -> Result<Self, EthereumError> {
        if config.chain_id == 0
            || config.genesis_hash == [0; 32]
            || config.archive_contract == [0; 20]
            || config.archive_code_hash == [0; 32]
            || config.publisher == [0; 20]
        {
            return Err(EthereumError::Configuration);
        }
        let rpc = RpcCluster::new(&config.rpc)?;
        let reader = Self { config, rpc };
        reader.verify_static_identity()?;
        Ok(reader)
    }

    /// Retrieves one exact commitment from one finalized chain view. All
    /// contract calls are pinned to the returned block; latest-state results
    /// are never combined with finalized archive bytes.
    pub fn retrieve(
        &self,
        commitment: ArchiveCommitment,
    ) -> Result<Option<EthereumMirrorRead>, EthereumError> {
        self.verify_static_identity()?;
        let head = self
            .rpc
            .call("eth_getBlockByNumber", json!(["finalized", false]))?;
        if head.is_null() {
            return Err(EthereumError::Retrieval);
        }
        let block_number = parse_quantity(string(&head, "number")?)?;
        let block_hash = fixed_hex::<32>(string(&head, "hash")?)?;
        let block = quantity(block_number);
        self.verify_target_at(&block)?;

        let metadata = self.eth_call_at(
            &abi_call(
                "manifest(bytes32)",
                &[AbiValue::Fixed(*commitment.as_bytes())],
            ),
            &block,
        )?;
        if metadata.is_empty() || metadata.iter().all(|byte| *byte == 0) {
            return Ok(None);
        }
        let (length, chunks, digest, finalized) = decode_manifest(&metadata)?;
        if !finalized || length == 0 || length > 64 * 1024 * 1024 || chunks == 0 || chunks > 65_536
        {
            return Err(EthereumError::Retrieval);
        }
        let mut archive = Vec::with_capacity(length);
        for index in 0..chunks {
            let encoded = self.eth_call_at(
                &abi_call(
                    "chunk(bytes32,uint32)",
                    &[
                        AbiValue::Fixed(*commitment.as_bytes()),
                        AbiValue::Uint(u128::from(index)),
                    ],
                ),
                &block,
            )?;
            let bytes = decode_dynamic_bytes(&encoded, MAX_CHUNK_BYTES)?;
            archive.extend_from_slice(&bytes);
            if archive.len() > length {
                return Err(EthereumError::Retrieval);
            }
        }
        if archive.len() != length
            || <[u8; 32]>::from(Sha256::digest(&archive)) != digest
            || archive_commitment(&archive) != commitment
        {
            return Err(EthereumError::Retrieval);
        }
        Ok(Some(EthereumMirrorRead {
            archive,
            block_number,
            block_hash,
            reference_head_number: block_number,
            reference_head_hash: block_hash,
        }))
    }

    /// Rechecks the original coordinate without invalidating the archive's
    /// cryptographic evidence when its publication provenance was reorged.
    pub fn is_canonical(&self, observation: &EthereumMirrorRead) -> Result<bool, EthereumError> {
        self.is_coordinate_canonical(observation.block_number, observation.block_hash)
    }

    pub fn is_coordinate_canonical(
        &self,
        block_number: u64,
        block_hash: [u8; 32],
    ) -> Result<bool, EthereumError> {
        let block = self.rpc.call(
            "eth_getBlockByNumber",
            json!([quantity(block_number), false]),
        )?;
        Ok(!block.is_null() && fixed_hex::<32>(string(&block, "hash")?)? == block_hash)
    }

    fn verify_static_identity(&self) -> Result<(), EthereumError> {
        let chain = self.rpc.call("eth_chainId", json!([]))?;
        if parse_quantity(chain.as_str().ok_or(EthereumError::ChainIdentity)?)?
            != self.config.chain_id
        {
            return Err(EthereumError::ChainIdentity);
        }
        let genesis = self
            .rpc
            .call("eth_getBlockByNumber", json!(["0x0", false]))?;
        if fixed_hex::<32>(string(&genesis, "hash")?)? != self.config.genesis_hash {
            return Err(EthereumError::ChainIdentity);
        }
        Ok(())
    }

    fn verify_target_at(&self, block: &str) -> Result<(), EthereumError> {
        let code = self.rpc.call(
            "eth_getCode",
            json!([format!("0x{}", hex(&self.config.archive_contract)), block]),
        )?;
        let code = decode_hex(code.as_str().ok_or(EthereumError::ContractIdentity)?)?;
        if code.is_empty()
            || <[u8; 32]>::from(Keccak256::digest(&code)) != self.config.archive_code_hash
        {
            return Err(EthereumError::ContractIdentity);
        }
        let publisher = self.eth_call_at(&abi_call("publisher()", &[]), block)?;
        if publisher.len() != 32
            || publisher[..12].iter().any(|byte| *byte != 0)
            || publisher[12..] != self.config.publisher
        {
            return Err(EthereumError::ContractIdentity);
        }
        Ok(())
    }

    fn eth_call_at(&self, data: &[u8], block: &str) -> Result<Vec<u8>, EthereumError> {
        let value = self.rpc.call(
            "eth_call",
            json!([{
                "to": format!("0x{}", hex(&self.config.archive_contract)),
                "data": format!("0x{}", hex(data))
            }, block]),
        )?;
        decode_hex(value.as_str().ok_or(EthereumError::Retrieval)?)
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct EthereumProgress {
    pub commitment: ArchiveCommitment,
    pub stage: PublicationStage,
    pub phase: PublicationPhase,
    pub transaction_hash: Option<[u8; 32]>,
    pub position: FinalityPosition,
    pub cursor: MirrorCursor,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum EthereumError {
    Configuration,
    Rpc(RpcError),
    Signer(SignerError),
    Store(StoreError),
    ChainIdentity,
    ContractIdentity,
    Nonce,
    FeePolicy,
    Transaction,
    Receipt,
    Event,
    Reorg,
    Retrieval,
    WorkerTerminated,
}

impl From<RpcError> for EthereumError {
    fn from(value: RpcError) -> Self {
        Self::Rpc(value)
    }
}

impl From<SignerError> for EthereumError {
    fn from(value: SignerError) -> Self {
        Self::Signer(value)
    }
}

impl From<StoreError> for EthereumError {
    fn from(value: StoreError) -> Self {
        Self::Store(value)
    }
}

/// Durable Ethereum publisher. It owns nonce-bearing signed bytes and never
/// replaces a broadcast-unknown transaction.
pub struct EthereumArchiveClient {
    config: EthereumProductionConfig,
    rpc: RpcCluster,
    signer: RemoteChainSigner,
    signer_address: [u8; 20],
    journal: PublicationJournal,
}

impl EthereumArchiveClient {
    pub fn open(
        config: EthereumProductionConfig,
        signer: RemoteChainSigner,
    ) -> Result<Self, EthereumError> {
        validate_config(&config)?;
        let signer_address = signer.ethereum_address()?;
        let rpc = RpcCluster::new(&config.rpc)?;
        let journal = PublicationJournal::open(
            &config.journal_directory,
            MirrorChain::Ethereum,
            config.first_batch_number,
        )?;
        let client = Self {
            config,
            rpc,
            signer,
            signer_address,
            journal,
        };
        client.verify_target()?;
        Ok(client)
    }

    /// Advances at most one durable stage. Calling it after a crash first
    /// resolves the exact persisted transaction before creating another.
    pub fn advance(&mut self, archive: &Archive) -> Result<EthereumProgress, EthereumError> {
        self.verify_target()?;
        let stages = stages(archive, self.config.chunk_bytes)?;
        for stage in stages {
            let record = self
                .journal
                .record(archive.commitment(), stage.stage)
                .cloned();
            match record {
                None => return self.prepare_and_broadcast(archive, stage),
                Some(record)
                    if (record.phase == PublicationPhase::RetrievedVerified
                        && stage.stage != PublicationStage::Finalize)
                        || (record.phase == PublicationPhase::Finalized
                            && stage.stage != PublicationStage::Finalize) => {}
                Some(record) if record.phase == PublicationPhase::PermanentRefusal => {
                    return Ok(self.progress(&record));
                }
                Some(record) if record.phase == PublicationPhase::PreBroadcastFailure => {
                    return self.prepare_and_broadcast(archive, stage);
                }
                Some(record) => return self.observe_stage(archive, stage, record),
            }
        }
        let final_record = self
            .journal
            .record(archive.commitment(), PublicationStage::Finalize)
            .cloned()
            .ok_or(EthereumError::Transaction)?;
        Ok(self.progress(&final_record))
    }

    pub fn retrieve(
        &self,
        commitment: ArchiveCommitment,
    ) -> Result<Option<Vec<u8>>, EthereumError> {
        self.verify_target()?;
        let metadata_call = abi_call(
            "manifest(bytes32)",
            &[AbiValue::Fixed(*commitment.as_bytes())],
        );
        let metadata = self.eth_call(&metadata_call)?;
        if metadata.is_empty() || metadata.iter().all(|byte| *byte == 0) {
            return Ok(None);
        }
        let (length, chunks, digest, finalized) = decode_manifest(&metadata)?;
        if !finalized || length == 0 || length > 64 * 1024 * 1024 || chunks == 0 {
            return Err(EthereumError::Retrieval);
        }
        let mut archive = Vec::with_capacity(length);
        for index in 0..chunks {
            let call = abi_call(
                "chunk(bytes32,uint32)",
                &[
                    AbiValue::Fixed(*commitment.as_bytes()),
                    AbiValue::Uint(u128::from(index)),
                ],
            );
            let encoded = self.eth_call(&call)?;
            let bytes = decode_dynamic_bytes(&encoded, self.config.chunk_bytes)?;
            archive.extend_from_slice(&bytes);
            if archive.len() > length {
                return Err(EthereumError::Retrieval);
            }
        }
        if archive.len() != length
            || <[u8; 32]>::from(Sha256::digest(&archive)) != digest
            || archive_commitment(&archive) != commitment
        {
            return Err(EthereumError::Retrieval);
        }
        Ok(Some(archive))
    }

    #[must_use]
    pub fn cursor(&self) -> MirrorCursor {
        self.journal.cursor()
    }

    fn prepare_and_broadcast(
        &mut self,
        archive: &Archive,
        stage: StagePayload,
    ) -> Result<EthereumProgress, EthereumError> {
        let digest: [u8; 32] = Sha256::digest(&stage.call_data).into();
        let base = PublicationRecord {
            chain: MirrorChain::Ethereum,
            batch_number: archive.data().batch_number,
            commitment: archive.commitment(),
            checkpoint: archive
                .data()
                .checkpoint
                .as_ref()
                .map(|value| value.coordinate),
            stage: stage.stage,
            phase: PublicationPhase::Prepared,
            stage_payload_digest: digest,
            signed_payload: Vec::new(),
            transaction: TransactionIdentity::None,
            position: FinalityPosition::None,
        };
        if self
            .journal
            .record(archive.commitment(), stage.stage)
            .is_none()
        {
            self.journal.append(base.clone())?;
        }
        let signed = match self.sign_transaction(&stage.call_data) {
            Ok(signed) => signed,
            Err(error @ EthereumError::Signer(SignerError::Refused)) => {
                let mut refused = base;
                refused.phase = PublicationPhase::PermanentRefusal;
                self.journal.append(refused)?;
                return Err(error);
            }
            Err(error) => {
                let mut failed = base;
                failed.phase = PublicationPhase::PreBroadcastFailure;
                self.journal.append(failed)?;
                return Err(error);
            }
        };
        let mut persisted = base;
        persisted.phase = PublicationPhase::Signed;
        persisted.signed_payload = signed.raw.clone();
        persisted.transaction = TransactionIdentity::Ethereum(signed.hash);
        self.journal.append(persisted.clone())?;
        let result = self.rpc.broadcast(
            "eth_sendRawTransaction",
            json!([format!("0x{}", hex(&signed.raw))]),
            &format!("0x{}", hex(&signed.hash)),
        );
        match result {
            Ok(BroadcastResult::Accepted) => persisted.phase = PublicationPhase::Pending,
            Ok(BroadcastResult::Unknown) => persisted.phase = PublicationPhase::BroadcastUnknown,
            Err(RpcError::Rejected { .. }) => persisted.phase = PublicationPhase::PermanentRefusal,
            Err(error) => {
                persisted.phase = PublicationPhase::BroadcastUnknown;
                self.journal.append(persisted.clone())?;
                return Err(EthereumError::Rpc(error));
            }
        }
        self.journal.append(persisted.clone())?;
        Ok(self.progress(&persisted))
    }

    fn observe_stage(
        &mut self,
        archive: &Archive,
        stage: StagePayload,
        mut record: PublicationRecord,
    ) -> Result<EthereumProgress, EthereumError> {
        let TransactionIdentity::Ethereum(transaction_hash) = record.transaction else {
            return Err(EthereumError::Transaction);
        };
        if record.phase == PublicationPhase::Reorged {
            record.phase = PublicationPhase::Signed;
            self.journal.append(record.clone())?;
        }
        if record.phase == PublicationPhase::Signed {
            let outcome = self.rpc.broadcast(
                "eth_sendRawTransaction",
                json!([format!("0x{}", hex(&record.signed_payload))]),
                &format!("0x{}", hex(&transaction_hash)),
            );
            record.phase = match outcome {
                Ok(BroadcastResult::Accepted) => PublicationPhase::Pending,
                Ok(BroadcastResult::Unknown) => PublicationPhase::BroadcastUnknown,
                Err(RpcError::Rejected { .. }) => PublicationPhase::PermanentRefusal,
                Err(error) => {
                    record.phase = PublicationPhase::BroadcastUnknown;
                    self.journal.append(record)?;
                    return Err(EthereumError::Rpc(error));
                }
            };
            self.journal.append(record.clone())?;
        }
        let was_retrieved = record.phase == PublicationPhase::RetrievedVerified;
        if was_retrieved && self.reorg_monitoring_complete(record.position)? {
            return Ok(self.progress(&record));
        }
        let receipt = self.rpc.call(
            "eth_getTransactionReceipt",
            json!([format!("0x{}", hex(&transaction_hash))]),
        )?;
        if receipt.is_null() {
            if was_retrieved {
                record.phase = PublicationPhase::Reorged;
                self.journal.append(record.clone())?;
                return Ok(self.progress(&record));
            }
            let transaction = self.rpc.call(
                "eth_getTransactionByHash",
                json!([format!("0x{}", hex(&transaction_hash))]),
            )?;
            record.phase = if transaction.is_null() {
                PublicationPhase::BroadcastUnknown
            } else {
                verify_transaction_identity(
                    &transaction,
                    self.signer_address,
                    self.config.archive_contract,
                    &stage.call_data,
                    transaction_hash,
                )?;
                PublicationPhase::Pending
            };
            self.journal.append(record.clone())?;
            return Ok(self.progress(&record));
        }
        let receipt_identity = self.verify_receipt(
            &receipt,
            archive.commitment(),
            stage.stage,
            transaction_hash,
            &stage.call_data,
        );
        let (block_number, block_hash) = match receipt_identity {
            Ok(value) => value,
            Err(_) if was_retrieved => {
                record.phase = PublicationPhase::Reorged;
                self.journal.append(record.clone())?;
                return Ok(self.progress(&record));
            }
            Err(error) => return Err(error),
        };
        let canonical = self.rpc.call(
            "eth_getBlockByNumber",
            json!([quantity(block_number), false]),
        )?;
        let canonical_hash = fixed_hex::<32>(string(&canonical, "hash")?)?;
        if canonical_hash != block_hash {
            record.phase = PublicationPhase::Reorged;
            record.position = FinalityPosition::Ethereum {
                block_number,
                block_hash,
            };
            self.journal.append(record.clone())?;
            return Ok(self.progress(&record));
        }
        let latest = self.rpc.call("eth_blockNumber", json!([]))?;
        let latest = parse_quantity(latest.as_str().ok_or(EthereumError::Receipt)?)?;
        let confirmations = latest.saturating_sub(block_number).saturating_add(1);
        record.position = FinalityPosition::Ethereum {
            block_number,
            block_hash,
        };
        if confirmations < self.config.required_confirmations {
            if was_retrieved {
                record.phase = PublicationPhase::Reorged;
                self.journal.append(record.clone())?;
                return Ok(self.progress(&record));
            }
            record.phase = PublicationPhase::Pending;
            self.journal.append(record.clone())?;
            return Ok(self.progress(&record));
        }
        if was_retrieved {
            return Ok(self.progress(&record));
        }
        record.phase = PublicationPhase::Finalized;
        self.journal.append(record.clone())?;
        if stage.stage == PublicationStage::Finalize {
            let retrieved = self
                .retrieve(archive.commitment())?
                .ok_or(EthereumError::Retrieval)?;
            if retrieved != archive.bytes() {
                return Err(EthereumError::Retrieval);
            }
            record.phase = PublicationPhase::RetrievedVerified;
            self.journal.append(record.clone())?;
        }
        Ok(self.progress(&record))
    }

    fn sign_transaction(&self, call_data: &[u8]) -> Result<SignedTransaction, EthereumError> {
        if call_data.len() > MAX_CALL_DATA_BYTES {
            return Err(EthereumError::Transaction);
        }
        let estimated = self.rpc.call(
            "eth_estimateGas",
            json!([{
                "from": format!("0x{}", hex(&self.signer_address)),
                "to": format!("0x{}", hex(&self.config.archive_contract)),
                "data": format!("0x{}", hex(call_data)),
                "value": "0x0"
            }]),
        )?;
        let estimated = parse_quantity(estimated.as_str().ok_or(EthereumError::Transaction)?)?;
        if estimated == 0 || estimated > self.config.transaction_gas_limit {
            return Err(EthereumError::Transaction);
        }
        let nonce_value = self.rpc.call(
            "eth_getTransactionCount",
            json!([format!("0x{}", hex(&self.signer_address)), "pending"]),
        )?;
        let nonce = parse_quantity(nonce_value.as_str().ok_or(EthereumError::Nonce)?)?;
        let priority_value = self.rpc.call("eth_maxPriorityFeePerGas", json!([]))?;
        let priority =
            parse_quantity_u128(priority_value.as_str().ok_or(EthereumError::FeePolicy)?)?;
        let gas_value = self.rpc.call("eth_gasPrice", json!([]))?;
        let base = parse_quantity_u128(gas_value.as_str().ok_or(EthereumError::FeePolicy)?)?;
        let maximum_priority = priority.min(self.config.maximum_priority_fee_per_gas);
        let maximum_fee = base
            .checked_mul(2)
            .and_then(|value| value.checked_add(maximum_priority))
            .ok_or(EthereumError::FeePolicy)?;
        if maximum_priority == 0
            || maximum_fee > self.config.maximum_fee_per_gas
            || maximum_priority > maximum_fee
        {
            return Err(EthereumError::FeePolicy);
        }
        let fields = vec![
            rlp_uint(u128::from(self.config.chain_id)),
            rlp_uint(u128::from(nonce)),
            rlp_uint(maximum_priority),
            rlp_uint(maximum_fee),
            rlp_uint(u128::from(self.config.transaction_gas_limit)),
            rlp_bytes(&self.config.archive_contract),
            rlp_uint(0),
            rlp_bytes(call_data),
            rlp_list(&[]),
        ];
        let unsigned = rlp_list(&fields);
        let mut preimage = Vec::with_capacity(1 + unsigned.len());
        preimage.push(ETHEREUM_TX_TYPE);
        preimage.extend_from_slice(&unsigned);
        let digest: [u8; 32] = Keccak256::digest(&preimage).into();
        let ChainSignature::Secp256k1(signature) = self
            .signer
            .sign_digest(b"LayerX/mirror/ethereum-eip1559/v1", digest)?
        else {
            return Err(EthereumError::Signer(SignerError::InvalidSignature));
        };
        let mut signed_fields = fields;
        signed_fields.push(rlp_uint(u128::from(signature[64])));
        signed_fields.push(rlp_scalar(&signature[..32])?);
        signed_fields.push(rlp_scalar(&signature[32..64])?);
        let encoded = rlp_list(&signed_fields);
        let mut raw = Vec::with_capacity(1 + encoded.len());
        raw.push(ETHEREUM_TX_TYPE);
        raw.extend_from_slice(&encoded);
        let hash = Keccak256::digest(&raw).into();
        Ok(SignedTransaction { raw, hash })
    }

    fn verify_target(&self) -> Result<(), EthereumError> {
        let chain = self.rpc.call("eth_chainId", json!([]))?;
        if parse_quantity(chain.as_str().ok_or(EthereumError::ChainIdentity)?)?
            != self.config.chain_id
        {
            return Err(EthereumError::ChainIdentity);
        }
        let genesis = self
            .rpc
            .call("eth_getBlockByNumber", json!(["0x0", false]))?;
        if fixed_hex::<32>(string(&genesis, "hash")?)? != self.config.genesis_hash {
            return Err(EthereumError::ChainIdentity);
        }
        let code = self.rpc.call(
            "eth_getCode",
            json!([
                format!("0x{}", hex(&self.config.archive_contract)),
                "latest"
            ]),
        )?;
        let code = decode_hex(code.as_str().ok_or(EthereumError::ContractIdentity)?)?;
        if code.is_empty()
            || <[u8; 32]>::from(Keccak256::digest(&code)) != self.config.archive_code_hash
        {
            return Err(EthereumError::ContractIdentity);
        }
        let publisher = self.eth_call(&abi_call("publisher()", &[]))?;
        if publisher.len() != 32
            || publisher[..12].iter().any(|byte| *byte != 0)
            || publisher[12..] != self.signer_address
        {
            return Err(EthereumError::ContractIdentity);
        }
        Ok(())
    }

    fn verify_receipt(
        &self,
        receipt: &Value,
        commitment: ArchiveCommitment,
        stage: PublicationStage,
        transaction_hash: [u8; 32],
        call_data: &[u8],
    ) -> Result<(u64, [u8; 32]), EthereumError> {
        if parse_quantity(string(receipt, "status")?)? != 1
            || fixed_hex::<32>(string(receipt, "transactionHash")?)? != transaction_hash
            || fixed_hex::<20>(string(receipt, "to")?)? != self.config.archive_contract
        {
            return Err(EthereumError::Receipt);
        }
        let block_number = parse_quantity(string(receipt, "blockNumber")?)?;
        let block_hash = fixed_hex::<32>(string(receipt, "blockHash")?)?;
        self.verify_contract_at(&quantity(block_number))?;
        let transaction = self.rpc.call(
            "eth_getTransactionByHash",
            json!([format!("0x{}", hex(&transaction_hash))]),
        )?;
        verify_transaction_identity(
            &transaction,
            self.signer_address,
            self.config.archive_contract,
            call_data,
            transaction_hash,
        )?;
        let event_signature = match stage {
            PublicationStage::Manifest => {
                "ManifestOpened(bytes32,uint32,uint64,uint64,uint32,bytes32)"
            }
            PublicationStage::Chunk(_) => "ChunkStored(bytes32,uint32,bytes32,uint32)",
            PublicationStage::Finalize => "ArchiveFinalized(bytes32,bytes32)",
        };
        let event_topic: [u8; 32] = Keccak256::digest(event_signature.as_bytes()).into();
        let logs = receipt
            .get("logs")
            .and_then(Value::as_array)
            .ok_or(EthereumError::Event)?;
        let matched = logs.iter().any(|log| {
            let Ok(address) = string(log, "address").and_then(fixed_hex::<20>) else {
                return false;
            };
            let Some(topics) = log.get("topics").and_then(Value::as_array) else {
                return false;
            };
            let Some(first) = topics.first().and_then(Value::as_str) else {
                return false;
            };
            let Some(second) = topics.get(1).and_then(Value::as_str) else {
                return false;
            };
            address == self.config.archive_contract
                && fixed_hex::<32>(first).ok() == Some(event_topic)
                && fixed_hex::<32>(second).ok() == Some(*commitment.as_bytes())
        });
        if !matched {
            return Err(EthereumError::Event);
        }
        Ok((block_number, block_hash))
    }

    fn eth_call(&self, data: &[u8]) -> Result<Vec<u8>, EthereumError> {
        let value = self.rpc.call(
            "eth_call",
            json!([{
                "to": format!("0x{}", hex(&self.config.archive_contract)),
                "data": format!("0x{}", hex(data))
            }, "latest"]),
        )?;
        decode_hex(value.as_str().ok_or(EthereumError::Retrieval)?)
    }

    fn verify_contract_at(&self, block: &str) -> Result<(), EthereumError> {
        let code = self.rpc.call(
            "eth_getCode",
            json!([format!("0x{}", hex(&self.config.archive_contract)), block]),
        )?;
        let code = decode_hex(code.as_str().ok_or(EthereumError::ContractIdentity)?)?;
        if code.is_empty()
            || <[u8; 32]>::from(Keccak256::digest(&code)) != self.config.archive_code_hash
        {
            return Err(EthereumError::ContractIdentity);
        }
        Ok(())
    }

    fn reorg_monitoring_complete(&self, position: FinalityPosition) -> Result<bool, EthereumError> {
        let FinalityPosition::Ethereum { block_number, .. } = position else {
            return Ok(false);
        };
        let latest = self.rpc.call("eth_blockNumber", json!([]))?;
        let latest = parse_quantity(latest.as_str().ok_or(EthereumError::Receipt)?)?;
        Ok(latest.saturating_sub(block_number) > self.config.maximum_reorg_depth)
    }

    fn progress(&self, record: &PublicationRecord) -> EthereumProgress {
        let transaction_hash = match record.transaction {
            TransactionIdentity::Ethereum(value) => Some(value),
            _ => None,
        };
        EthereumProgress {
            commitment: record.commitment,
            stage: record.stage,
            phase: record.phase,
            transaction_hash,
            position: record.position,
            cursor: self.journal.cursor(),
        }
    }
}

struct StagePayload {
    stage: PublicationStage,
    call_data: Vec<u8>,
}

struct SignedTransaction {
    raw: Vec<u8>,
    hash: [u8; 32],
}

fn stages(archive: &Archive, chunk_bytes: usize) -> Result<Vec<StagePayload>, EthereumError> {
    let chunk_count = archive.bytes().len().div_ceil(chunk_bytes);
    let chunk_count_u32 = u32::try_from(chunk_count).map_err(|_| EthereumError::Transaction)?;
    let length = u64::try_from(archive.bytes().len()).map_err(|_| EthereumError::Transaction)?;
    let checkpoint = archive
        .data()
        .checkpoint
        .as_ref()
        .map_or([0; 32], |value| value.coordinate.checkpoint_id);
    let mut output = Vec::with_capacity(chunk_count.saturating_add(2));
    output.push(StagePayload {
        stage: PublicationStage::Manifest,
        call_data: abi_call(
            "begin(bytes32,uint32,uint64,bytes32,uint64,uint32,bytes32,bytes32)",
            &[
                AbiValue::Fixed(*archive.commitment().as_bytes()),
                AbiValue::Uint(u128::from(archive.data().network_id)),
                AbiValue::Uint(u128::from(archive.data().batch_number)),
                AbiValue::Fixed(checkpoint),
                AbiValue::Uint(u128::from(length)),
                AbiValue::Uint(u128::from(chunk_count_u32)),
                AbiValue::Fixed(Sha256::digest(archive.bytes()).into()),
                AbiValue::Fixed(chunk_chain_root(archive.bytes(), chunk_bytes)?),
            ],
        ),
    });
    for (index, chunk) in archive.bytes().chunks(chunk_bytes).enumerate() {
        let index = u32::try_from(index).map_err(|_| EthereumError::Transaction)?;
        output.push(StagePayload {
            stage: PublicationStage::Chunk(index),
            call_data: abi_call(
                "append(bytes32,uint32,bytes)",
                &[
                    AbiValue::Fixed(*archive.commitment().as_bytes()),
                    AbiValue::Uint(u128::from(index)),
                    AbiValue::Dynamic(chunk),
                ],
            ),
        });
    }
    output.push(StagePayload {
        stage: PublicationStage::Finalize,
        call_data: abi_call(
            "finalize(bytes32)",
            &[AbiValue::Fixed(*archive.commitment().as_bytes())],
        ),
    });
    Ok(output)
}

fn chunk_chain_root(bytes: &[u8], chunk_bytes: usize) -> Result<[u8; 32], EthereumError> {
    let mut chain = [0_u8; 32];
    for (index, chunk) in bytes.chunks(chunk_bytes).enumerate() {
        let index = u32::try_from(index).map_err(|_| EthereumError::Transaction)?;
        let digest: [u8; 32] = Sha256::digest(chunk).into();
        let length = u32::try_from(chunk.len()).map_err(|_| EthereumError::Transaction)?;
        let mut hasher = Keccak256::new();
        hasher.update(chain);
        hasher.update(index.to_be_bytes());
        hasher.update(digest);
        hasher.update(length.to_be_bytes());
        chain = hasher.finalize().into();
    }
    if chain == [0; 32] {
        Err(EthereumError::Transaction)
    } else {
        Ok(chain)
    }
}

pub(crate) fn validate_config(config: &EthereumProductionConfig) -> Result<(), EthereumError> {
    if config.chain_id == 0
        || config.genesis_hash == [0; 32]
        || config.archive_contract == [0; 20]
        || config.archive_code_hash == [0; 32]
        || config.first_batch_number == 0
        || config.required_confirmations == 0
        || config.maximum_reorg_depth < config.required_confirmations
        || config.maximum_reorg_depth > 1_000_000
        || !(MIN_CHUNK_BYTES..=MAX_CHUNK_BYTES).contains(&config.chunk_bytes)
        || !(21_000..=100_000_000).contains(&config.transaction_gas_limit)
        || config.maximum_fee_per_gas == 0
        || config.maximum_priority_fee_per_gas == 0
        || config.maximum_priority_fee_per_gas > config.maximum_fee_per_gas
        || config.journal_directory.as_os_str().is_empty()
    {
        Err(EthereumError::Configuration)
    } else {
        Ok(())
    }
}

enum AbiValue<'a> {
    Fixed([u8; 32]),
    Uint(u128),
    Dynamic(&'a [u8]),
}

fn abi_call(signature: &str, values: &[AbiValue<'_>]) -> Vec<u8> {
    let selector = Keccak256::digest(signature.as_bytes());
    let mut output = Vec::new();
    output.extend_from_slice(&selector[..4]);
    let head_bytes = values.len().saturating_mul(32);
    let mut tail = Vec::new();
    for value in values {
        match value {
            AbiValue::Fixed(value) => output.extend_from_slice(value),
            AbiValue::Uint(value) => {
                let mut word = [0_u8; 32];
                word[16..].copy_from_slice(&value.to_be_bytes());
                output.extend_from_slice(&word);
            }
            AbiValue::Dynamic(value) => {
                let offset = head_bytes.saturating_add(tail.len());
                let mut word = [0_u8; 32];
                word[24..]
                    .copy_from_slice(&u64::try_from(offset).unwrap_or(u64::MAX).to_be_bytes());
                output.extend_from_slice(&word);
                let mut length = [0_u8; 32];
                length[24..]
                    .copy_from_slice(&u64::try_from(value.len()).unwrap_or(u64::MAX).to_be_bytes());
                tail.extend_from_slice(&length);
                tail.extend_from_slice(value);
                let padding = (32 - value.len() % 32) % 32;
                tail.resize(tail.len().saturating_add(padding), 0);
            }
        }
    }
    output.extend_from_slice(&tail);
    output
}

fn decode_manifest(bytes: &[u8]) -> Result<(usize, u32, [u8; 32], bool), EthereumError> {
    if bytes.len() != 128 {
        return Err(EthereumError::Retrieval);
    }
    let length = word_u64(&bytes[0..32])?;
    let chunks = u32::try_from(word_u64(&bytes[32..64])?).map_err(|_| EthereumError::Retrieval)?;
    let digest = bytes[64..96]
        .try_into()
        .map_err(|_| EthereumError::Retrieval)?;
    let finalized = match word_u64(&bytes[96..128])? {
        0 => false,
        1 => true,
        _ => return Err(EthereumError::Retrieval),
    };
    Ok((
        usize::try_from(length).map_err(|_| EthereumError::Retrieval)?,
        chunks,
        digest,
        finalized,
    ))
}

fn decode_dynamic_bytes(bytes: &[u8], maximum: usize) -> Result<Vec<u8>, EthereumError> {
    if bytes.len() < 64 || word_u64(&bytes[..32])? != 32 {
        return Err(EthereumError::Retrieval);
    }
    let length =
        usize::try_from(word_u64(&bytes[32..64])?).map_err(|_| EthereumError::Retrieval)?;
    if length == 0
        || length > maximum
        || bytes.len() != 64_usize.saturating_add(length.div_ceil(32) * 32)
    {
        return Err(EthereumError::Retrieval);
    }
    Ok(bytes[64..64 + length].to_vec())
}

fn word_u64(bytes: &[u8]) -> Result<u64, EthereumError> {
    if bytes.len() != 32 || bytes[..24].iter().any(|byte| *byte != 0) {
        return Err(EthereumError::Retrieval);
    }
    bytes[24..]
        .try_into()
        .map(u64::from_be_bytes)
        .map_err(|_| EthereumError::Retrieval)
}

fn verify_transaction_identity(
    value: &Value,
    from: [u8; 20],
    to: [u8; 20],
    input: &[u8],
    hash: [u8; 32],
) -> Result<(), EthereumError> {
    if value.is_null()
        || fixed_hex::<32>(string(value, "hash")?)? != hash
        || fixed_hex::<20>(string(value, "from")?)? != from
        || fixed_hex::<20>(string(value, "to")?)? != to
        || decode_hex(string(value, "input")?)? != input
    {
        return Err(EthereumError::Transaction);
    }
    Ok(())
}

fn rlp_uint(value: u128) -> Vec<u8> {
    if value == 0 {
        return vec![0x80];
    }
    let bytes = value.to_be_bytes();
    let first = bytes
        .iter()
        .position(|byte| *byte != 0)
        .unwrap_or(bytes.len() - 1);
    rlp_bytes(&bytes[first..])
}

fn rlp_bytes(value: &[u8]) -> Vec<u8> {
    if value.len() == 1 && value[0] < 0x80 {
        return value.to_vec();
    }
    let mut output = length_prefix(0x80, 0xb7, value.len());
    output.extend_from_slice(value);
    output
}

fn rlp_list(values: &[Vec<u8>]) -> Vec<u8> {
    let length = values
        .iter()
        .fold(0_usize, |total, value| total.saturating_add(value.len()));
    let mut output = length_prefix(0xc0, 0xf7, length);
    for value in values {
        output.extend_from_slice(value);
    }
    output
}

fn length_prefix(short: u8, long: u8, length: usize) -> Vec<u8> {
    if length <= 55 {
        return vec![short.saturating_add(u8::try_from(length).unwrap_or(u8::MAX))];
    }
    let bytes = length.to_be_bytes();
    let first = bytes
        .iter()
        .position(|byte| *byte != 0)
        .unwrap_or(bytes.len() - 1);
    let encoded = &bytes[first..];
    let mut output = vec![long.saturating_add(u8::try_from(encoded.len()).unwrap_or(u8::MAX))];
    output.extend_from_slice(encoded);
    output
}

fn rlp_scalar(bytes: &[u8]) -> Result<Vec<u8>, EthereumError> {
    if bytes.len() != 32 || bytes.iter().all(|byte| *byte == 0) {
        return Err(EthereumError::Transaction);
    }
    let first = bytes
        .iter()
        .position(|byte| *byte != 0)
        .ok_or(EthereumError::Transaction)?;
    Ok(rlp_bytes(&bytes[first..]))
}

fn quantity(value: u64) -> String {
    format!("0x{value:x}")
}

fn parse_quantity(value: &str) -> Result<u64, EthereumError> {
    let digits = value.strip_prefix("0x").ok_or(EthereumError::Receipt)?;
    if digits.is_empty() || (digits.len() > 1 && digits.starts_with('0')) {
        return Err(EthereumError::Receipt);
    }
    u64::from_str_radix(digits, 16).map_err(|_| EthereumError::Receipt)
}

fn parse_quantity_u128(value: &str) -> Result<u128, EthereumError> {
    let digits = value.strip_prefix("0x").ok_or(EthereumError::FeePolicy)?;
    if digits.is_empty() || (digits.len() > 1 && digits.starts_with('0')) {
        return Err(EthereumError::FeePolicy);
    }
    u128::from_str_radix(digits, 16).map_err(|_| EthereumError::FeePolicy)
}

fn string<'a>(value: &'a Value, field: &str) -> Result<&'a str, EthereumError> {
    value
        .get(field)
        .and_then(Value::as_str)
        .ok_or(EthereumError::Receipt)
}

fn fixed_hex<const N: usize>(value: &str) -> Result<[u8; N], EthereumError> {
    decode_hex(value)?
        .try_into()
        .map_err(|_| EthereumError::Receipt)
}

fn decode_hex(value: &str) -> Result<Vec<u8>, EthereumError> {
    let digits = value.strip_prefix("0x").ok_or(EthereumError::Receipt)?;
    if digits.len() % 2 != 0 {
        return Err(EthereumError::Receipt);
    }
    digits
        .as_bytes()
        .chunks_exact(2)
        .map(|pair| {
            let high = hex_digit(pair[0]).ok_or(EthereumError::Receipt)?;
            let low = hex_digit(pair[1]).ok_or(EthereumError::Receipt)?;
            Ok(high << 4 | low)
        })
        .collect()
}

fn hex_digit(value: u8) -> Option<u8> {
    match value {
        b'0'..=b'9' => Some(value - b'0'),
        b'a'..=b'f' => Some(value - b'a' + 10),
        b'A'..=b'F' => Some(value - b'A' + 10),
        _ => None,
    }
}

fn hex(bytes: &[u8]) -> String {
    const DIGITS: &[u8; 16] = b"0123456789abcdef";
    let mut output = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        output.push(char::from(DIGITS[usize::from(byte >> 4)]));
        output.push(char::from(DIGITS[usize::from(byte & 0x0f)]));
    }
    output
}
