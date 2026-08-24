//! Production Solana immutable PDA chunk/manifest archive client.

use std::collections::BTreeSet;
use std::path::PathBuf;

use base64::engine::general_purpose::STANDARD as BASE64;
use base64::Engine as _;
use ed25519_dalek::VerifyingKey;
use serde_json::{json, Value};
use sha2::{Digest, Sha256};

use crate::rpc::{BroadcastResult, RpcCluster, RpcError, RpcQuorumConfig};
use crate::signer::{ChainSignature, RemoteChainSigner, SignerError};
use crate::store::{
    FinalityPosition, MirrorChain, PublicationJournal, PublicationPhase, PublicationRecord,
    PublicationStage, StoreError, TransactionIdentity,
};
use crate::{archive_commitment, Archive, ArchiveCommitment, MirrorCursor};

const SYSTEM_PROGRAM: [u8; 32] = [0; 32];
const PDA_MARKER: &[u8] = b"ProgramDerivedAddress";
const MANIFEST_MAGIC: &[u8; 8] = b"LXMMAN02";
const CHUNK_MAGIC: &[u8; 8] = b"LXMCHK02";
const INSTRUCTION_MAGIC: &[u8; 4] = b"LXMA";
const INSTRUCTION_VERSION: u16 = 2;
const MAX_TRANSACTION_BYTES: usize = 1232;
const MIN_CHUNK_BYTES: usize = 128;
const MAX_CHUNK_BYTES: usize = 720;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SolanaProductionConfig {
    pub rpc: RpcQuorumConfig,
    pub genesis_hash: [u8; 32],
    pub archive_program: [u8; 32],
    pub upgradeable_loader: [u8; 32],
    pub program_data_account: [u8; 32],
    pub program_code_hash: [u8; 32],
    pub first_batch_number: u64,
    pub required_rooted_slots: u64,
    pub maximum_ancestry: u64,
    pub chunk_bytes: usize,
    pub journal_directory: PathBuf,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct SolanaProgress {
    pub commitment: ArchiveCommitment,
    pub stage: PublicationStage,
    pub phase: PublicationPhase,
    pub signature: Option<[u8; 64]>,
    pub position: FinalityPosition,
    pub cursor: MirrorCursor,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum SolanaError {
    Configuration,
    Rpc(RpcError),
    Signer(SignerError),
    Store(StoreError),
    ClusterIdentity,
    ProgramIdentity,
    ProgramMutable,
    Pda,
    Blockhash,
    Transaction,
    SignatureHistory,
    Finality,
    Reorg,
    Retrieval,
    WorkerTerminated,
}

impl From<RpcError> for SolanaError {
    fn from(value: RpcError) -> Self {
        Self::Rpc(value)
    }
}

impl From<SignerError> for SolanaError {
    fn from(value: SignerError) -> Self {
        Self::Signer(value)
    }
}

impl From<StoreError> for SolanaError {
    fn from(value: StoreError) -> Self {
        Self::Store(value)
    }
}

/// Durable Solana publisher using exact signed transaction bytes and rooted
/// signature history. The chain key remains an opaque remote signer handle.
pub struct SolanaArchiveClient {
    config: SolanaProductionConfig,
    rpc: RpcCluster,
    signer: RemoteChainSigner,
    payer: [u8; 32],
    journal: PublicationJournal,
}

impl SolanaArchiveClient {
    pub fn open(
        config: SolanaProductionConfig,
        signer: RemoteChainSigner,
    ) -> Result<Self, SolanaError> {
        validate_config(&config)?;
        let payer = signer
            .public_key()
            .try_into()
            .map_err(|_| SolanaError::Configuration)?;
        let rpc = RpcCluster::new(&config.rpc)?;
        let journal = PublicationJournal::open(
            &config.journal_directory,
            MirrorChain::Solana,
            config.first_batch_number,
        )?;
        let client = Self {
            config,
            rpc,
            signer,
            payer,
            journal,
        };
        client.verify_target()?;
        Ok(client)
    }

    pub fn advance(&mut self, archive: &Archive) -> Result<SolanaProgress, SolanaError> {
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
            .ok_or(SolanaError::Transaction)?;
        Ok(self.progress(&final_record))
    }

    pub fn retrieve(&self, commitment: ArchiveCommitment) -> Result<Option<Vec<u8>>, SolanaError> {
        self.verify_target()?;
        let (manifest_address, _) =
            manifest_pda(self.config.archive_program, self.payer, commitment)?;
        let Some(manifest_bytes) = self.account_data(manifest_address)? else {
            return Ok(None);
        };
        let manifest = decode_manifest(&manifest_bytes, commitment, self.payer)?;
        if !manifest.finalized || manifest.length == 0 || manifest.length > 64 * 1024 * 1024 {
            return Err(SolanaError::Retrieval);
        }
        let mut archive = Vec::with_capacity(manifest.length);
        for index in 0..manifest.chunk_count {
            let (address, _) =
                chunk_pda(self.config.archive_program, self.payer, commitment, index)?;
            let bytes = self.account_data(address)?.ok_or(SolanaError::Retrieval)?;
            let chunk = decode_chunk(&bytes, commitment, index, self.config.chunk_bytes)?;
            archive.extend_from_slice(&chunk);
            if archive.len() > manifest.length {
                return Err(SolanaError::Retrieval);
            }
        }
        if archive.len() != manifest.length
            || <[u8; 32]>::from(Sha256::digest(&archive)) != manifest.digest
            || archive_commitment(&archive) != commitment
        {
            return Err(SolanaError::Retrieval);
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
        stage: StageInstruction,
    ) -> Result<SolanaProgress, SolanaError> {
        let digest = Sha256::digest(&stage.data).into();
        let base = PublicationRecord {
            chain: MirrorChain::Solana,
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
        let signed = match self.sign_transaction(&stage) {
            Ok(value) => value,
            Err(error @ SolanaError::Signer(SignerError::Refused)) => {
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
        persisted.transaction = TransactionIdentity::Solana(signed.signature);
        self.journal.append(persisted.clone())?;
        let signature_text = base58(&signed.signature);
        let outcome = self.rpc.broadcast(
            "sendTransaction",
            json!([BASE64.encode(&signed.raw), {
                "encoding": "base64",
                "skipPreflight": false,
                "preflightCommitment": "finalized",
                "maxRetries": 0
            }]),
            &signature_text,
        );
        match outcome {
            Ok(BroadcastResult::Accepted) => persisted.phase = PublicationPhase::Pending,
            Ok(BroadcastResult::Unknown) => persisted.phase = PublicationPhase::BroadcastUnknown,
            Err(RpcError::Rejected { .. }) => persisted.phase = PublicationPhase::PermanentRefusal,
            Err(error) => {
                persisted.phase = PublicationPhase::BroadcastUnknown;
                self.journal.append(persisted.clone())?;
                return Err(SolanaError::Rpc(error));
            }
        }
        self.journal.append(persisted.clone())?;
        Ok(self.progress(&persisted))
    }

    fn observe_stage(
        &mut self,
        archive: &Archive,
        stage: StageInstruction,
        mut record: PublicationRecord,
    ) -> Result<SolanaProgress, SolanaError> {
        let TransactionIdentity::Solana(mut signature) = record.transaction else {
            return Err(SolanaError::Transaction);
        };
        if matches!(
            record.phase,
            PublicationPhase::Reorged | PublicationPhase::BroadcastExpired
        ) {
            let replacement = self.sign_transaction(&stage)?;
            record.phase = PublicationPhase::Signed;
            record.signed_payload = replacement.raw;
            record.transaction = TransactionIdentity::Solana(replacement.signature);
            record.position = FinalityPosition::None;
            self.journal.append(record.clone())?;
            signature = replacement.signature;
        }
        if record.phase == PublicationPhase::Signed {
            let outcome = self.rpc.broadcast(
                "sendTransaction",
                json!([BASE64.encode(&record.signed_payload), {
                    "encoding": "base64",
                    "skipPreflight": false,
                    "preflightCommitment": "finalized",
                    "maxRetries": 0
                }]),
                &base58(&signature),
            );
            record.phase = match outcome {
                Ok(BroadcastResult::Accepted) => PublicationPhase::Pending,
                Ok(BroadcastResult::Unknown) => PublicationPhase::BroadcastUnknown,
                Err(RpcError::Rejected { .. }) => PublicationPhase::PermanentRefusal,
                Err(error) => {
                    record.phase = PublicationPhase::BroadcastUnknown;
                    self.journal.append(record)?;
                    return Err(SolanaError::Rpc(error));
                }
            };
            self.journal.append(record.clone())?;
        }
        let was_retrieved = record.phase == PublicationPhase::RetrievedVerified;
        if was_retrieved && self.reorg_monitoring_complete(record.position)? {
            return Ok(self.progress(&record));
        }
        let statuses = self.rpc.call(
            "getSignatureStatuses",
            json!([[base58(&signature)], { "searchTransactionHistory": true }]),
        )?;
        let status = statuses
            .get("value")
            .and_then(Value::as_array)
            .and_then(|values| values.first())
            .ok_or(SolanaError::SignatureHistory)?;
        if status.is_null() {
            record.phase = if was_retrieved {
                PublicationPhase::Reorged
            } else if self.blockhash_is_valid(&record.signed_payload)? {
                PublicationPhase::BroadcastUnknown
            } else {
                PublicationPhase::BroadcastExpired
            };
            self.journal.append(record.clone())?;
            return Ok(self.progress(&record));
        }
        if !status.get("err").is_some_and(Value::is_null) {
            record.phase = if was_retrieved {
                PublicationPhase::Reorged
            } else {
                PublicationPhase::PermanentRefusal
            };
            self.journal.append(record.clone())?;
            return Ok(self.progress(&record));
        }
        let confirmation = status
            .get("confirmationStatus")
            .and_then(Value::as_str)
            .ok_or(SolanaError::SignatureHistory)?;
        if confirmation != "finalized" {
            record.phase = if was_retrieved {
                PublicationPhase::Reorged
            } else {
                PublicationPhase::Pending
            };
            self.journal.append(record.clone())?;
            return Ok(self.progress(&record));
        }
        let transaction = self.rpc.call(
            "getTransaction",
            json!([base58(&signature), {
                "commitment": "finalized",
                "encoding": "base64",
                "maxSupportedTransactionVersion": 0
            }]),
        )?;
        let encoded = transaction
            .get("transaction")
            .and_then(Value::as_array)
            .and_then(|value| value.first())
            .and_then(Value::as_str)
            .ok_or(SolanaError::Transaction)?;
        let observed = BASE64
            .decode(encoded)
            .map_err(|_| SolanaError::Transaction)?;
        if observed != record.signed_payload
            || !transaction
                .get("meta")
                .and_then(|meta| meta.get("err"))
                .is_some_and(Value::is_null)
        {
            return Err(SolanaError::Transaction);
        }
        let slot = transaction
            .get("slot")
            .and_then(Value::as_u64)
            .ok_or(SolanaError::Finality)?;
        if self.program_deployment_slot()? > slot {
            return Err(SolanaError::ProgramIdentity);
        }
        let block = self.rpc.call(
            "getBlock",
            json!([slot, {
                "commitment": "finalized",
                "transactionDetails": "none",
                "rewards": false,
                "maxSupportedTransactionVersion": 0
            }]),
        )?;
        let blockhash = fixed_base58::<32>(string(&block, "blockhash")?)?;
        if let Err(error) = self.verify_rooted_ancestry(slot, blockhash) {
            if was_retrieved {
                record.phase = PublicationPhase::Reorged;
                self.journal.append(record.clone())?;
                return Ok(self.progress(&record));
            }
            return Err(error);
        }
        if let FinalityPosition::Solana {
            slot: former_slot,
            blockhash: former_hash,
        } = record.position
        {
            if was_retrieved && (former_slot != slot || former_hash != blockhash) {
                record.phase = PublicationPhase::Reorged;
                self.journal.append(record.clone())?;
                return Ok(self.progress(&record));
            }
        }
        record.position = FinalityPosition::Solana { slot, blockhash };
        if was_retrieved {
            return Ok(self.progress(&record));
        }
        record.phase = PublicationPhase::Finalized;
        self.journal.append(record.clone())?;
        if stage.stage == PublicationStage::Finalize {
            let retrieved = self
                .retrieve(archive.commitment())?
                .ok_or(SolanaError::Retrieval)?;
            if retrieved != archive.bytes() {
                return Err(SolanaError::Retrieval);
            }
            record.phase = PublicationPhase::RetrievedVerified;
            self.journal.append(record.clone())?;
        }
        Ok(self.progress(&record))
    }

    fn sign_transaction(&self, stage: &StageInstruction) -> Result<SignedTransaction, SolanaError> {
        let latest = self
            .rpc
            .call("getLatestBlockhash", json!([{ "commitment": "finalized" }]))?;
        let blockhash = latest
            .get("value")
            .and_then(|value| value.get("blockhash"))
            .and_then(Value::as_str)
            .ok_or(SolanaError::Blockhash)?;
        let blockhash = fixed_base58::<32>(blockhash)?;
        let message = compile_message(self.payer, self.config.archive_program, stage, blockhash)?;
        let ChainSignature::Ed25519(signature) = self
            .signer
            .sign_message(b"LayerX/mirror/solana-transaction/v1", &message)?
        else {
            return Err(SolanaError::Signer(SignerError::InvalidSignature));
        };
        let mut raw = Vec::with_capacity(1 + 64 + message.len());
        shortvec(&mut raw, 1)?;
        raw.extend_from_slice(&signature);
        raw.extend_from_slice(&message);
        if raw.len() > MAX_TRANSACTION_BYTES {
            return Err(SolanaError::Transaction);
        }
        Ok(SignedTransaction { raw, signature })
    }

    fn verify_target(&self) -> Result<(), SolanaError> {
        let genesis = self.rpc.call("getGenesisHash", json!([]))?;
        if fixed_base58::<32>(genesis.as_str().ok_or(SolanaError::ClusterIdentity)?)?
            != self.config.genesis_hash
        {
            return Err(SolanaError::ClusterIdentity);
        }
        let program = self
            .account(self.config.archive_program)?
            .ok_or(SolanaError::ProgramIdentity)?;
        if !program.executable
            || program.owner != self.config.upgradeable_loader
            || decode_program_data_pointer(&program.data)? != self.config.program_data_account
        {
            return Err(SolanaError::ProgramIdentity);
        }
        self.program_deployment_slot()?;
        Ok(())
    }

    fn program_deployment_slot(&self) -> Result<u64, SolanaError> {
        let program_data = self
            .account(self.config.program_data_account)?
            .ok_or(SolanaError::ProgramIdentity)?;
        if program_data.owner != self.config.upgradeable_loader || program_data.executable {
            return Err(SolanaError::ProgramIdentity);
        }
        let (deployment_slot, code) = immutable_program_code(&program_data.data)?;
        if <[u8; 32]>::from(Sha256::digest(code)) != self.config.program_code_hash {
            return Err(SolanaError::ProgramIdentity);
        }
        Ok(deployment_slot)
    }

    fn reorg_monitoring_complete(&self, position: FinalityPosition) -> Result<bool, SolanaError> {
        let FinalityPosition::Solana { slot, .. } = position else {
            return Ok(false);
        };
        let root = self
            .rpc
            .call("getSlot", json!([{ "commitment": "finalized" }]))?
            .as_u64()
            .ok_or(SolanaError::Finality)?;
        Ok(root.saturating_sub(slot) > self.config.maximum_ancestry)
    }

    fn blockhash_is_valid(&self, signed_transaction: &[u8]) -> Result<bool, SolanaError> {
        let blockhash = transaction_blockhash(signed_transaction)?;
        let result = self.rpc.call(
            "isBlockhashValid",
            json!([base58(&blockhash), { "commitment": "finalized" }]),
        )?;
        result
            .get("value")
            .and_then(Value::as_bool)
            .ok_or(SolanaError::Blockhash)
    }

    fn verify_rooted_ancestry(
        &self,
        target_slot: u64,
        target_hash: [u8; 32],
    ) -> Result<(), SolanaError> {
        let root = self
            .rpc
            .call("getSlot", json!([{ "commitment": "finalized" }]))?
            .as_u64()
            .ok_or(SolanaError::Finality)?;
        if root < target_slot
            || root.saturating_sub(target_slot) < self.config.required_rooted_slots
            || root.saturating_sub(target_slot) > self.config.maximum_ancestry
        {
            return Err(SolanaError::Finality);
        }
        let mut slot = root;
        let mut child_hash = None;
        loop {
            let block = self.rpc.call(
                "getBlock",
                json!([slot, {
                    "commitment": "finalized",
                    "transactionDetails": "none",
                    "rewards": false,
                    "maxSupportedTransactionVersion": 0
                }]),
            )?;
            let hash = fixed_base58::<32>(string(&block, "blockhash")?)?;
            if let Some(expected) = child_hash {
                if hash != expected {
                    return Err(SolanaError::Reorg);
                }
            }
            if slot == target_slot {
                return if hash == target_hash {
                    Ok(())
                } else {
                    Err(SolanaError::Reorg)
                };
            }
            let parent = block
                .get("parentSlot")
                .and_then(Value::as_u64)
                .ok_or(SolanaError::Finality)?;
            if parent >= slot || parent < target_slot {
                return Err(SolanaError::Finality);
            }
            child_hash = Some(fixed_base58::<32>(string(&block, "previousBlockhash")?)?);
            slot = parent;
        }
    }

    fn account_data(&self, address: [u8; 32]) -> Result<Option<Vec<u8>>, SolanaError> {
        self.account(address).and_then(|value| {
            value
                .map(|account| {
                    if account.owner != self.config.archive_program || account.executable {
                        Err(SolanaError::Retrieval)
                    } else {
                        Ok(account.data)
                    }
                })
                .transpose()
        })
    }

    fn account(&self, address: [u8; 32]) -> Result<Option<Account>, SolanaError> {
        let value = self.rpc.call(
            "getAccountInfo",
            json!([base58(&address), {
                "commitment": "finalized",
                "encoding": "base64"
            }]),
        )?;
        let value = value.get("value").ok_or(SolanaError::Retrieval)?;
        if value.is_null() {
            return Ok(None);
        }
        let owner = fixed_base58::<32>(string(value, "owner")?)?;
        let executable = value
            .get("executable")
            .and_then(Value::as_bool)
            .ok_or(SolanaError::Retrieval)?;
        let data = value
            .get("data")
            .and_then(Value::as_array)
            .and_then(|parts| parts.first())
            .and_then(Value::as_str)
            .ok_or(SolanaError::Retrieval)?;
        let data = BASE64.decode(data).map_err(|_| SolanaError::Retrieval)?;
        Ok(Some(Account {
            owner,
            executable,
            data,
        }))
    }

    fn progress(&self, record: &PublicationRecord) -> SolanaProgress {
        let signature = match record.transaction {
            TransactionIdentity::Solana(value) => Some(value),
            _ => None,
        };
        SolanaProgress {
            commitment: record.commitment,
            stage: record.stage,
            phase: record.phase,
            signature,
            position: record.position,
            cursor: self.journal.cursor(),
        }
    }
}

struct Account {
    owner: [u8; 32],
    executable: bool,
    data: Vec<u8>,
}

struct StageInstruction {
    stage: PublicationStage,
    accounts: Vec<AccountMeta>,
    data: Vec<u8>,
}

#[derive(Clone, Copy)]
struct AccountMeta {
    address: [u8; 32],
    signer: bool,
    writable: bool,
}

struct SignedTransaction {
    raw: Vec<u8>,
    signature: [u8; 64],
}

struct Manifest {
    length: usize,
    chunk_count: u32,
    digest: [u8; 32],
    finalized: bool,
}

fn stages(archive: &Archive, chunk_bytes: usize) -> Result<Vec<StageInstruction>, SolanaError> {
    let commitment = archive.commitment();
    let manifest = [0; 32];
    let chunk_count = u32::try_from(archive.bytes().len().div_ceil(chunk_bytes))
        .map_err(|_| SolanaError::Transaction)?;
    let checkpoint = archive
        .data()
        .checkpoint
        .as_ref()
        .map_or([0; 32], |value| value.coordinate.checkpoint_id);
    let mut begin = Vec::new();
    begin.extend_from_slice(INSTRUCTION_MAGIC);
    begin.extend_from_slice(&INSTRUCTION_VERSION.to_be_bytes());
    begin.push(1);
    begin.extend_from_slice(commitment.as_bytes());
    begin.extend_from_slice(&archive.data().network_id.to_be_bytes());
    begin.extend_from_slice(&archive.data().batch_number.to_be_bytes());
    begin.extend_from_slice(&checkpoint);
    begin.extend_from_slice(
        &u64::try_from(archive.bytes().len())
            .map_err(|_| SolanaError::Transaction)?
            .to_be_bytes(),
    );
    begin.extend_from_slice(&chunk_count.to_be_bytes());
    begin.extend_from_slice(&<[u8; 32]>::from(Sha256::digest(archive.bytes())));
    begin.extend_from_slice(&chunk_chain_root(archive.bytes(), chunk_bytes)?);
    let mut output = vec![StageInstruction {
        stage: PublicationStage::Manifest,
        accounts: vec![
            AccountMeta {
                address: [0; 32],
                signer: true,
                writable: true,
            },
            AccountMeta {
                address: manifest,
                signer: false,
                writable: true,
            },
            AccountMeta {
                address: SYSTEM_PROGRAM,
                signer: false,
                writable: false,
            },
        ],
        data: begin,
    }];
    for (index, bytes) in archive.bytes().chunks(chunk_bytes).enumerate() {
        let index = u32::try_from(index).map_err(|_| SolanaError::Transaction)?;
        let chunk = [0; 32];
        let mut data = Vec::new();
        data.extend_from_slice(INSTRUCTION_MAGIC);
        data.extend_from_slice(&INSTRUCTION_VERSION.to_be_bytes());
        data.push(2);
        data.extend_from_slice(commitment.as_bytes());
        data.extend_from_slice(&index.to_be_bytes());
        data.extend_from_slice(
            &u16::try_from(bytes.len())
                .map_err(|_| SolanaError::Transaction)?
                .to_be_bytes(),
        );
        data.extend_from_slice(bytes);
        data.extend_from_slice(&<[u8; 32]>::from(Sha256::digest(bytes)));
        output.push(StageInstruction {
            stage: PublicationStage::Chunk(index),
            accounts: vec![
                AccountMeta {
                    address: [0; 32],
                    signer: true,
                    writable: true,
                },
                AccountMeta {
                    address: manifest,
                    signer: false,
                    writable: true,
                },
                AccountMeta {
                    address: chunk,
                    signer: false,
                    writable: true,
                },
                AccountMeta {
                    address: SYSTEM_PROGRAM,
                    signer: false,
                    writable: false,
                },
            ],
            data,
        });
    }
    let mut finalize = Vec::new();
    finalize.extend_from_slice(INSTRUCTION_MAGIC);
    finalize.extend_from_slice(&INSTRUCTION_VERSION.to_be_bytes());
    finalize.push(3);
    finalize.extend_from_slice(commitment.as_bytes());
    output.push(StageInstruction {
        stage: PublicationStage::Finalize,
        accounts: vec![
            AccountMeta {
                address: [0; 32],
                signer: true,
                writable: true,
            },
            AccountMeta {
                address: manifest,
                signer: false,
                writable: true,
            },
        ],
        data: finalize,
    });
    Ok(output)
}

fn compile_message(
    payer: [u8; 32],
    program: [u8; 32],
    stage: &StageInstruction,
    recent_blockhash: [u8; 32],
) -> Result<Vec<u8>, SolanaError> {
    let commitment: [u8; 32] = stage
        .data
        .get(7..39)
        .ok_or(SolanaError::Transaction)?
        .try_into()
        .map_err(|_| SolanaError::Transaction)?;
    let commitment = ArchiveCommitment::from_bytes(commitment);
    let mut metas = stage.accounts.clone();
    let manifest = manifest_pda(program, payer, commitment)?.0;
    if let Some(value) = metas.first_mut() {
        value.address = payer;
    }
    if let Some(value) = metas.get_mut(1) {
        value.address = manifest;
    }
    if let PublicationStage::Chunk(index) = stage.stage {
        let chunk = chunk_pda(program, payer, commitment, index)?.0;
        if let Some(value) = metas.get_mut(2) {
            value.address = chunk;
        }
    }
    let mut keys = metas.iter().map(|meta| meta.address).collect::<Vec<_>>();
    keys.push(program);
    if keys.iter().copied().collect::<BTreeSet<_>>().len() != keys.len() {
        return Err(SolanaError::Transaction);
    }
    let readonly_signed = u8::try_from(
        metas
            .iter()
            .filter(|meta| meta.signer && !meta.writable)
            .count(),
    )
    .map_err(|_| SolanaError::Transaction)?;
    let readonly_unsigned = u8::try_from(
        metas
            .iter()
            .filter(|meta| !meta.signer && !meta.writable)
            .count()
            .saturating_add(1),
    )
    .map_err(|_| SolanaError::Transaction)?;
    let mut message = vec![1, readonly_signed, readonly_unsigned];
    shortvec(&mut message, keys.len())?;
    for key in &keys {
        message.extend_from_slice(key);
    }
    message.extend_from_slice(&recent_blockhash);
    shortvec(&mut message, 1)?;
    message.push(u8::try_from(keys.len() - 1).map_err(|_| SolanaError::Transaction)?);
    shortvec(&mut message, metas.len())?;
    for index in 0..metas.len() {
        message.push(u8::try_from(index).map_err(|_| SolanaError::Transaction)?);
    }
    shortvec(&mut message, stage.data.len())?;
    message.extend_from_slice(&stage.data);
    Ok(message)
}

fn manifest_pda(
    program: [u8; 32],
    publisher: [u8; 32],
    commitment: ArchiveCommitment,
) -> Result<([u8; 32], u8), SolanaError> {
    find_pda(&[b"manifest", &publisher, commitment.as_bytes()], program)
}

fn chunk_pda(
    program: [u8; 32],
    publisher: [u8; 32],
    commitment: ArchiveCommitment,
    index: u32,
) -> Result<([u8; 32], u8), SolanaError> {
    find_pda(
        &[
            b"chunk",
            &publisher,
            commitment.as_bytes(),
            &index.to_be_bytes(),
        ],
        program,
    )
}

fn find_pda(seeds: &[&[u8]], program: [u8; 32]) -> Result<([u8; 32], u8), SolanaError> {
    if seeds.len() > 15 || seeds.iter().any(|seed| seed.len() > 32) {
        return Err(SolanaError::Pda);
    }
    for bump in (0_u8..=u8::MAX).rev() {
        let mut hasher = Sha256::new();
        for seed in seeds {
            hasher.update(seed);
        }
        hasher.update([bump]);
        hasher.update(program);
        hasher.update(PDA_MARKER);
        let address: [u8; 32] = hasher.finalize().into();
        if VerifyingKey::from_bytes(&address).is_err() {
            return Ok((address, bump));
        }
    }
    Err(SolanaError::Pda)
}

fn decode_program_data_pointer(bytes: &[u8]) -> Result<[u8; 32], SolanaError> {
    if bytes.len() != 36 || bytes[..4] != 2_u32.to_le_bytes() {
        return Err(SolanaError::ProgramIdentity);
    }
    bytes[4..36]
        .try_into()
        .map_err(|_| SolanaError::ProgramIdentity)
}

fn immutable_program_code(bytes: &[u8]) -> Result<(u64, &[u8]), SolanaError> {
    if bytes.len() <= 45 || bytes[..4] != 3_u32.to_le_bytes() {
        return Err(SolanaError::ProgramIdentity);
    }
    if bytes[12] != 0 {
        return Err(SolanaError::ProgramMutable);
    }
    let deployment_slot = u64::from_le_bytes(
        bytes[4..12]
            .try_into()
            .map_err(|_| SolanaError::ProgramIdentity)?,
    );
    if deployment_slot == 0 {
        return Err(SolanaError::ProgramIdentity);
    }
    Ok((deployment_slot, &bytes[45..]))
}

fn transaction_blockhash(bytes: &[u8]) -> Result<[u8; 32], SolanaError> {
    if bytes.len() < 1 + 64 + 3 + 1 + 32 + 32 || bytes.first() != Some(&1) {
        return Err(SolanaError::Transaction);
    }
    let message = &bytes[65..];
    let (key_count, length_bytes) =
        decode_shortvec(message.get(3..).ok_or(SolanaError::Transaction)?)?;
    let blockhash_offset = 3_usize
        .checked_add(length_bytes)
        .and_then(|value| value.checked_add(key_count.checked_mul(32)?))
        .ok_or(SolanaError::Transaction)?;
    message
        .get(blockhash_offset..blockhash_offset.saturating_add(32))
        .ok_or(SolanaError::Transaction)?
        .try_into()
        .map_err(|_| SolanaError::Transaction)
}

fn decode_shortvec(bytes: &[u8]) -> Result<(usize, usize), SolanaError> {
    let mut value = 0_usize;
    let mut shift = 0_u32;
    for (index, byte) in bytes.iter().copied().take(3).enumerate() {
        value = value
            .checked_add(
                usize::from(byte & 0x7f)
                    .checked_shl(shift)
                    .ok_or(SolanaError::Transaction)?,
            )
            .ok_or(SolanaError::Transaction)?;
        if byte & 0x80 == 0 {
            return Ok((value, index + 1));
        }
        shift = shift.checked_add(7).ok_or(SolanaError::Transaction)?;
    }
    Err(SolanaError::Transaction)
}

fn decode_manifest(
    bytes: &[u8],
    commitment: ArchiveCommitment,
    publisher: [u8; 32],
) -> Result<Manifest, SolanaError> {
    if bytes.len() != 8 + 32 + 32 + 4 + 8 + 32 + 8 + 4 + 32 + 32 + 32 + 8 + 4 + 1
        || &bytes[..8] != MANIFEST_MAGIC
        || bytes[8..40] != *commitment.as_bytes()
        || bytes[40..72] != publisher
    {
        return Err(SolanaError::Retrieval);
    }
    let length = u64::from_be_bytes(
        bytes[116..124]
            .try_into()
            .map_err(|_| SolanaError::Retrieval)?,
    ) as usize;
    let chunk_count = u32::from_be_bytes(
        bytes[124..128]
            .try_into()
            .map_err(|_| SolanaError::Retrieval)?,
    );
    let digest = bytes[128..160]
        .try_into()
        .map_err(|_| SolanaError::Retrieval)?;
    let expected_chain: [u8; 32] = bytes[160..192]
        .try_into()
        .map_err(|_| SolanaError::Retrieval)?;
    let observed_chain: [u8; 32] = bytes[192..224]
        .try_into()
        .map_err(|_| SolanaError::Retrieval)?;
    let received = u64::from_be_bytes(
        bytes[224..232]
            .try_into()
            .map_err(|_| SolanaError::Retrieval)?,
    ) as usize;
    let next_chunk = u32::from_be_bytes(
        bytes[232..236]
            .try_into()
            .map_err(|_| SolanaError::Retrieval)?,
    );
    let finalized = match bytes[236] {
        0 => false,
        1 => true,
        _ => return Err(SolanaError::Retrieval),
    };
    if received != length || next_chunk != chunk_count || expected_chain != observed_chain {
        return Err(SolanaError::Retrieval);
    }
    Ok(Manifest {
        length,
        chunk_count,
        digest,
        finalized,
    })
}

fn chunk_chain_root(bytes: &[u8], chunk_bytes: usize) -> Result<[u8; 32], SolanaError> {
    let mut chain = [0_u8; 32];
    for (index, chunk) in bytes.chunks(chunk_bytes).enumerate() {
        let index = u32::try_from(index).map_err(|_| SolanaError::Transaction)?;
        let digest: [u8; 32] = Sha256::digest(chunk).into();
        let length = u32::try_from(chunk.len()).map_err(|_| SolanaError::Transaction)?;
        let mut hasher = Sha256::new();
        hasher.update(chain);
        hasher.update(index.to_be_bytes());
        hasher.update(digest);
        hasher.update(length.to_be_bytes());
        chain = hasher.finalize().into();
    }
    if chain == [0; 32] {
        Err(SolanaError::Transaction)
    } else {
        Ok(chain)
    }
}

fn decode_chunk(
    bytes: &[u8],
    commitment: ArchiveCommitment,
    index: u32,
    maximum: usize,
) -> Result<Vec<u8>, SolanaError> {
    if bytes.len() < 8 + 32 + 4 + 32 + 4
        || &bytes[..8] != CHUNK_MAGIC
        || bytes[8..40] != *commitment.as_bytes()
        || bytes[40..44] != index.to_be_bytes()
    {
        return Err(SolanaError::Retrieval);
    }
    let digest: [u8; 32] = bytes[44..76]
        .try_into()
        .map_err(|_| SolanaError::Retrieval)?;
    let length = u32::from_be_bytes(
        bytes[76..80]
            .try_into()
            .map_err(|_| SolanaError::Retrieval)?,
    ) as usize;
    let data = bytes.get(80..).ok_or(SolanaError::Retrieval)?;
    if length == 0
        || length > maximum
        || data.len() != length
        || <[u8; 32]>::from(Sha256::digest(data)) != digest
    {
        return Err(SolanaError::Retrieval);
    }
    Ok(data.to_vec())
}

pub(crate) fn validate_config(config: &SolanaProductionConfig) -> Result<(), SolanaError> {
    if config.genesis_hash == [0; 32]
        || config.archive_program == [0; 32]
        || config.upgradeable_loader == [0; 32]
        || config.program_data_account == [0; 32]
        || config.program_code_hash == [0; 32]
        || config.first_batch_number == 0
        || config.required_rooted_slots == 0
        || config.maximum_ancestry < config.required_rooted_slots
        || config.maximum_ancestry > 4096
        || !(MIN_CHUNK_BYTES..=MAX_CHUNK_BYTES).contains(&config.chunk_bytes)
        || config.journal_directory.as_os_str().is_empty()
    {
        Err(SolanaError::Configuration)
    } else {
        Ok(())
    }
}

fn shortvec(output: &mut Vec<u8>, mut value: usize) -> Result<(), SolanaError> {
    loop {
        let low = u8::try_from(value & 0x7f).map_err(|_| SolanaError::Transaction)?;
        value >>= 7;
        output.push(if value == 0 { low } else { low | 0x80 });
        if value == 0 {
            return Ok(());
        }
        if output.len() > MAX_TRANSACTION_BYTES {
            return Err(SolanaError::Transaction);
        }
    }
}

fn string<'a>(value: &'a Value, field: &str) -> Result<&'a str, SolanaError> {
    value
        .get(field)
        .and_then(Value::as_str)
        .ok_or(SolanaError::Retrieval)
}

fn fixed_base58<const N: usize>(value: &str) -> Result<[u8; N], SolanaError> {
    base58_decode(value, N)?
        .try_into()
        .map_err(|_| SolanaError::Retrieval)
}

fn base58_decode(value: &str, maximum: usize) -> Result<Vec<u8>, SolanaError> {
    if value.is_empty() || value.len() > maximum.saturating_mul(2) {
        return Err(SolanaError::Retrieval);
    }
    let mut bytes = vec![0_u8];
    for character in value.bytes() {
        let digit = base58_digit(character).ok_or(SolanaError::Retrieval)?;
        let mut carry = u32::from(digit);
        for byte in bytes.iter_mut().rev() {
            let next = u32::from(*byte).saturating_mul(58).saturating_add(carry);
            *byte = u8::try_from(next & 0xff).map_err(|_| SolanaError::Retrieval)?;
            carry = next >> 8;
        }
        while carry > 0 {
            bytes.insert(
                0,
                u8::try_from(carry & 0xff).map_err(|_| SolanaError::Retrieval)?,
            );
            carry >>= 8;
        }
        if bytes.len() > maximum {
            return Err(SolanaError::Retrieval);
        }
    }
    let leading = value.bytes().take_while(|byte| *byte == b'1').count();
    let first_nonzero = bytes
        .iter()
        .position(|byte| *byte != 0)
        .unwrap_or(bytes.len());
    let mut decoded = vec![0_u8; leading];
    decoded.extend_from_slice(&bytes[first_nonzero..]);
    if decoded.len() > maximum {
        return Err(SolanaError::Retrieval);
    }
    Ok(decoded)
}

fn base58(value: &[u8]) -> String {
    const ALPHABET: &[u8; 58] = b"123456789ABCDEFGHJKLMNPQRSTUVWXYZabcdefghijkmnopqrstuvwxyz";
    if value.is_empty() {
        return String::new();
    }
    let leading = value.iter().take_while(|byte| **byte == 0).count();
    let mut digits = vec![0_u8];
    for byte in value {
        let mut carry = u32::from(*byte);
        for digit in digits.iter_mut().rev() {
            let next = u32::from(*digit).saturating_mul(256).saturating_add(carry);
            *digit = u8::try_from(next % 58).unwrap_or(0);
            carry = next / 58;
        }
        while carry > 0 {
            digits.insert(0, u8::try_from(carry % 58).unwrap_or(0));
            carry /= 58;
        }
    }
    let first_nonzero = digits
        .iter()
        .position(|digit| *digit != 0)
        .unwrap_or(digits.len());
    let mut output = String::with_capacity(leading.saturating_add(digits.len()));
    output.extend(std::iter::repeat_n('1', leading));
    for digit in &digits[first_nonzero..] {
        output.push(char::from(ALPHABET[usize::from(*digit)]));
    }
    output
}

fn base58_digit(value: u8) -> Option<u8> {
    b"123456789ABCDEFGHJKLMNPQRSTUVWXYZabcdefghijkmnopqrstuvwxyz"
        .iter()
        .position(|candidate| *candidate == value)
        .and_then(|index| u8::try_from(index).ok())
}
