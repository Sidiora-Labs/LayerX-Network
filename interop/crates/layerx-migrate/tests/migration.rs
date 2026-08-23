use ed25519_dalek::{Signer as _, SigningKey};
use layerx_interop_gateway::adapter::{
    AdapterId, ConformanceSuite, PinnedSpec, SpecVersion,
};
use layerx_interop_gateway::principal::PrincipalId;
use layerx_interop_gateway::trace::TraceId;
use layerx_interop_gateway::{interop_gateway_core, GatewayCore};
use layerx_migrate::{
    migration_adapter_descriptor, BindingReceiptPolicy, ExternalAddress, ExternalHistoryKind,
    ExternalHistoryRecord, ExternalHistorySink, ExternalProvenance, MigrationAdapter,
    MigrationError, MigrationIntent, MigrationPlane, MigrationPlaneResult, MigrationState,
    SourceChain, SourceEvidence, SourceTransaction, SourceVerifier, VerifiedAssetFinality,
    VerifiedHistoryPage, VerifiedOwnership,
};
use layerx_proof::receipt::{AuthorizedBatch, VerifiedReceipt};
use layerx_types::payload::ModuleId;
use sha2::{Digest as _, Sha256};

const ETHEREUM_MAINNET_CHAIN_ID: u64 = 1;
const ETHEREUM_SEPOLIA_CHAIN_ID: u64 = 11_155_111;
const SOLANA_MAINNET_GENESIS: [u8; 32] = [
    0x5e, 0x92, 0x22, 0xf9, 0x9e, 0x93, 0xed, 0x8e, 0xf0, 0x98, 0x7f, 0xf0, 0x60, 0x9d, 0x1c,
    0x77, 0xf9, 0x0d, 0x5e, 0xa9, 0xcc, 0x62, 0xb9, 0xc3, 0x2a, 0x11, 0x76, 0x3a, 0x7e, 0x8d,
    0x37, 0x1e,
];
const SOLANA_TESTNET_GENESIS: [u8; 32] = [
    0x4c, 0x0d, 0x8c, 0xf9, 0x5e, 0x19, 0xe4, 0x4c, 0x8a, 0x24, 0xe4, 0xe7, 0x6b, 0xf1, 0x5e,
    0x8a, 0xb4, 0x32, 0x9d, 0x3d, 0x97, 0xa0, 0x8f, 0x9c, 0x8e, 0x39, 0xb6, 0x8d, 0x8f, 0x9a,
    0x1c, 0xf5,
];

const PERIOD_START: u64 = 1_700_000_000;
const WINDOW_START: u64 = 200;
const TEST_AGENT_ACCOUNT: [u8; 32] = [0xa2; 32];
const TEST_ASSET: [u8; 32] = [0xc2; 32];

fn principal(name: &str) -> PrincipalId {
    PrincipalId::new(name).unwrap_or_else(|error| panic!("principal {name}: {error}"))
}

fn trace() -> TraceId {
    TraceId::mint([0xaa; 16])
}

struct TestPlane {
    binding_sequence: u64,
    custody_sequence: u64,
    sequencer: SigningKey,
}

impl TestPlane {
    fn new() -> Self {
        Self {
            binding_sequence: WINDOW_START,
            custody_sequence: WINDOW_START + 1000,
            sequencer: SigningKey::from_bytes(&[0xbb; 32]),
        }
    }

    fn mint_binding_receipt(
        &mut self,
        ownership: &VerifiedOwnership,
        key: [u8; 32],
    ) -> (Vec<u8>, AuthorizedBatch) {
        self.binding_sequence += 1;
        let activity_id = Self::activity_id(self.binding_sequence, key);
        let batch_id = Self::batch_id(&activity_id);
        let previous = Self::state_root(&activity_id, b"before");
        let resulting = Self::state_root(&activity_id, b"after");
        let canonical = self.encode_binding_receipt(
            activity_id,
            self.binding_sequence,
            previous,
            resulting,
            batch_id,
            ownership,
        );
        let authority = AuthorizedBatch::new(
            batch_id,
            TEST_ASSET,
            previous,
            resulting,
            self.sequencer.verifying_key().to_bytes(),
        );
        (canonical, authority)
    }

    fn mint_custody_receipt(
        &mut self,
        finality: &VerifiedAssetFinality,
        key: [u8; 32],
    ) -> (Vec<u8>, AuthorizedBatch) {
        self.custody_sequence += 1;
        let activity_id = Self::activity_id(self.custody_sequence, key);
        let batch_id = Self::batch_id(&activity_id);
        let previous = Self::state_root(&activity_id, b"before");
        let resulting = Self::state_root(&activity_id, b"after");
        let canonical = self.encode_custody_receipt(
            activity_id,
            self.custody_sequence,
            previous,
            resulting,
            batch_id,
            finality,
        );
        let authority = AuthorizedBatch::new(
            batch_id,
            finality.layerx_asset,
            previous,
            resulting,
            self.sequencer.verifying_key().to_bytes(),
        );
        (canonical, authority)
    }

    fn activity_id(sequence: u64, key: [u8; 32]) -> [u8; 32] {
        Sha256::digest(
            [
                b"layerx-migration-activity/v1".as_slice(),
                &sequence.to_be_bytes(),
                &key,
            ]
            .concat(),
        )
        .into()
    }

    fn batch_id(activity_id: &[u8; 32]) -> [u8; 32] {
        Sha256::digest([b"batch".as_slice(), activity_id].concat()).into()
    }

    fn state_root(activity_id: &[u8; 32], label: &[u8]) -> [u8; 32] {
        Sha256::digest([label, activity_id].concat()).into()
    }

    fn encode_binding_receipt(
        &self,
        activity_id: [u8; 32],
        sequence: u64,
        previous: [u8; 32],
        resulting: [u8; 32],
        batch_id: [u8; 32],
        ownership: &VerifiedOwnership,
    ) -> Vec<u8> {
        let mut bytes = Vec::new();
        push_u16(&mut bytes, 1);
        push_u16(&mut bytes, 0x5201);
        push_u16(&mut bytes, 1);
        push_bytes(&mut bytes, &activity_id);
        push_u64(&mut bytes, sequence);
        push_bytes(&mut bytes, &previous);
        push_bytes(&mut bytes, &resulting);
        push_bytes(&mut bytes, &[0x81; 32]);
        bytes.extend_from_slice(&0_i32.to_be_bytes());
        bytes.extend_from_slice(&0_u32.to_be_bytes());
        bytes.extend_from_slice(&1_u128.to_be_bytes());
        push_bytes(&mut bytes, &batch_id);
        push_u16(&mut bytes, ModuleId::Governance as u16);
        bytes.extend_from_slice(&1_u32.to_be_bytes());
        bytes.extend_from_slice(&1_u32.to_be_bytes());
        bytes.push(1);
        push_bytes(&mut bytes, &TEST_ASSET);
        bytes.extend_from_slice(&1_u128.to_be_bytes());
        push_bytes(&mut bytes, &[0x42; 32]);
        bytes.extend_from_slice(&2_u128.to_be_bytes());
        bytes.extend_from_slice(&1_u128.to_be_bytes());
        push_u64(&mut bytes, sequence);
        push_bytes(&mut bytes, &ownership.layerx_identity);
        bytes.extend_from_slice(&0_u128.to_be_bytes());
        bytes.extend_from_slice(&1_u128.to_be_bytes());
        push_bytes(&mut bytes, &[0x91; 32]);
        push_bytes(&mut bytes, &[0x92; 32]);
        push_bytes(&mut bytes, &[0x93; 32]);
        push_u64(&mut bytes, PERIOD_START + sequence);
        bytes.push(0);
        let signature = self.sign(&bytes);
        *bytes.last_mut().unwrap_or_else(|| panic!("signature flag missing")) = 1;
        push_bytes(&mut bytes, &signature);
        bytes
    }

    fn encode_custody_receipt(
        &self,
        activity_id: [u8; 32],
        sequence: u64,
        previous: [u8; 32],
        resulting: [u8; 32],
        batch_id: [u8; 32],
        finality: &VerifiedAssetFinality,
    ) -> Vec<u8> {
        let debit_before = 5_000_u128;
        let credit_before = 10_000_u128;
        let mut bytes = Vec::new();
        push_u16(&mut bytes, 1);
        push_u16(&mut bytes, 0x5201);
        push_u16(&mut bytes, 1);
        push_bytes(&mut bytes, &activity_id);
        push_u64(&mut bytes, sequence);
        push_bytes(&mut bytes, &previous);
        push_bytes(&mut bytes, &resulting);
        push_bytes(&mut bytes, &[0x81; 32]);
        bytes.extend_from_slice(&0_i32.to_be_bytes());
        bytes.extend_from_slice(&0_u32.to_be_bytes());
        bytes.extend_from_slice(&1_u128.to_be_bytes());
        push_bytes(&mut bytes, &batch_id);
        push_u16(&mut bytes, ModuleId::Asset as u16);
        bytes.extend_from_slice(&1_u32.to_be_bytes());
        bytes.extend_from_slice(&0_u32.to_be_bytes());
        bytes.push(1);
        push_bytes(&mut bytes, &finality.layerx_asset);
        bytes.extend_from_slice(&finality.layerx_amount.to_be_bytes());
        push_bytes(&mut bytes, &[0x42; 32]);
        bytes.extend_from_slice(&debit_before.to_be_bytes());
        bytes.extend_from_slice(&(debit_before - finality.layerx_amount).to_be_bytes());
        push_u64(&mut bytes, sequence);
        push_bytes(&mut bytes, &finality.destination);
        bytes.extend_from_slice(&credit_before.to_be_bytes());
        bytes.extend_from_slice(&(credit_before + finality.layerx_amount).to_be_bytes());
        push_bytes(&mut bytes, &[0x91; 32]);
        push_bytes(&mut bytes, &[0x92; 32]);
        push_bytes(&mut bytes, &[0x93; 32]);
        push_u64(&mut bytes, PERIOD_START + sequence);
        bytes.push(0);
        let signature = self.sign(&bytes);
        *bytes.last_mut().unwrap_or_else(|| panic!("signature flag missing")) = 1;
        push_bytes(&mut bytes, &signature);
        bytes
    }

    fn sign(&self, message: &[u8]) -> [u8; 64] {
        let mut digest = Sha256::new();
        digest.update(b"LXP/v1/receipt\0");
        digest.update(message);
        self.sequencer.sign(&<[u8; 32]>::from(digest.finalize())).to_bytes()
    }
}

impl MigrationPlane for TestPlane {
    fn execute(
        &mut self,
        intent: &MigrationIntent,
        idempotency_key: [u8; 32],
        _trace: &TraceId,
    ) -> Result<MigrationPlaneResult, MigrationError> {
        match intent {
            MigrationIntent::BindAccount(ownership) => {
                let (canonical, authority) = self.mint_binding_receipt(ownership, idempotency_key);
                Ok(MigrationPlaneResult::Executed {
                    canonical_receipt: canonical,
                    authorised_batch: authority,
                })
            }
            MigrationIntent::CreditCustody(finality) => {
                let (canonical, authority) = self.mint_custody_receipt(finality, idempotency_key);
                Ok(MigrationPlaneResult::Executed {
                    canonical_receipt: canonical,
                    authorised_batch: authority,
                })
            }
        }
    }
}

fn push_u16(bytes: &mut Vec<u8>, value: u16) {
    bytes.extend_from_slice(&value.to_be_bytes());
}

fn push_u64(bytes: &mut Vec<u8>, value: u64) {
    bytes.extend_from_slice(&value.to_be_bytes());
}

fn push_bytes(output: &mut Vec<u8>, value: &[u8]) {
    let length = u32::try_from(value.len()).unwrap_or_else(|_| panic!("field overflow"));
    output.extend_from_slice(&length.to_be_bytes());
    output.extend_from_slice(value);
}

struct TestBindingPolicy;

impl BindingReceiptPolicy for TestBindingPolicy {
    fn verify_binding(
        &self,
        ownership: &VerifiedOwnership,
        receipt: &VerifiedReceipt,
    ) -> Result<(), MigrationError> {
        let Some(protocol) = receipt.receipt().protocol() else {
            return Err(MigrationError::ReceiptMismatch);
        };
        if protocol.to() != ownership.layerx_identity || ownership.layerx_identity == [0; 32] {
            return Err(MigrationError::ReceiptMismatch);
        }
        Ok(())
    }
}

struct TestHistorySink {
    stored: Vec<ExternalHistoryRecord>,
}

impl TestHistorySink {
    fn new() -> Self {
        Self {
            stored: Vec::new(),
        }
    }
}

impl ExternalHistorySink for TestHistorySink {
    fn store_external(
        &mut self,
        _principal: &PrincipalId,
        page: &VerifiedHistoryPage,
        _trace: &TraceId,
    ) -> Result<(), MigrationError> {
        for record in &page.records {
            if !self.stored.iter().any(|existing| {
                existing.chain == record.chain
                    && existing.transaction == record.transaction
                    && existing.address == record.address
            }) {
                self.stored.push(*record);
            }
        }
        Ok(())
    }
}

struct EthereumTestVerifier {
    chain_id: u64,
}

impl EthereumTestVerifier {
    fn new(chain_id: u64) -> Self {
        Self { chain_id }
    }

    fn test_ownership_evidence(
        chain_id: u64,
        address: [u8; 20],
        layerx_identity: [u8; 32],
    ) -> SourceEvidence {
        let chain = SourceChain::Ethereum { chain_id };
        let canonical = [
            b"ethereum-signature-v1".as_slice(),
            &chain_id.to_be_bytes(),
            &address,
            &layerx_identity,
        ]
        .concat();
        SourceEvidence::new(canonical).unwrap_or_else(|error| panic!("evidence: {error}"))
    }

    fn test_finality_evidence(
        chain_id: u64,
        transaction: SourceTransaction,
        source: [u8; 20],
        source_asset: [u8; 32],
        source_amount: u128,
        custody_reference: [u8; 32],
        layerx_asset: [u8; 32],
        layerx_amount: u128,
        destination: [u8; 32],
        finality_height: u64,
    ) -> SourceEvidence {
        let canonical = [
            b"ethereum-finality-v1".as_slice(),
            &chain_id.to_be_bytes(),
            &transaction.bytes(),
            &source,
            &source_asset,
            &source_amount.to_be_bytes(),
            &custody_reference,
            &layerx_asset,
            &layerx_amount.to_be_bytes(),
            &destination,
            &finality_height.to_be_bytes(),
        ]
        .concat();
        SourceEvidence::new(canonical).unwrap_or_else(|error| panic!("evidence: {error}"))
    }

    fn test_history_evidence(
        chain_id: u64,
        records: Vec<ExternalHistoryRecord>,
        next_cursor: Option<[u8; 32]>,
    ) -> SourceEvidence {
        let mut canonical = Vec::new();
        canonical.extend_from_slice(b"ethereum-history-v1");
        canonical.extend_from_slice(&chain_id.to_be_bytes());
        canonical.extend_from_slice(&(records.len() as u64).to_be_bytes());
        if let Some(cursor) = next_cursor {
            canonical.push(1);
            canonical.extend_from_slice(&cursor);
        } else {
            canonical.push(0);
        }
        for record in &records {
            if let ExternalAddress::Ethereum(address) = record.address {
                canonical.extend_from_slice(&record.transaction.bytes());
                canonical.extend_from_slice(&address);
                canonical.push(match record.kind {
                    ExternalHistoryKind::Incoming => 1,
                    ExternalHistoryKind::Outgoing => 2,
                    ExternalHistoryKind::Contract => 3,
                });
                canonical.extend_from_slice(&record.timestamp.to_be_bytes());
                canonical.extend_from_slice(&record.source_asset);
                canonical.extend_from_slice(&record.source_amount.to_be_bytes());
            }
        }
        SourceEvidence::new(canonical).unwrap_or_else(|error| panic!("evidence: {error}"))
    }
}

impl SourceVerifier for EthereumTestVerifier {
    fn verify_ownership(
        &self,
        evidence: &SourceEvidence,
        _trace: &TraceId,
    ) -> Result<VerifiedOwnership, MigrationError> {
        let canonical = evidence.canonical();
        if !canonical.starts_with(b"ethereum-signature-v1") {
            return Err(MigrationError::InvalidEvidence);
        }
        let chain_id = u64::from_be_bytes(
            canonical[21..29]
                .try_into()
                .map_err(|_| MigrationError::InvalidEvidence)?,
        );
        if chain_id != self.chain_id {
            return Err(MigrationError::InvalidNetwork);
        }
        let address: [u8; 20] = canonical[29..49]
            .try_into()
            .map_err(|_| MigrationError::InvalidAddress)?;
        let layerx_identity: [u8; 32] = canonical[49..81]
            .try_into()
            .map_err(|_| MigrationError::InvalidEvidence)?;
        Ok(VerifiedOwnership {
            chain: SourceChain::Ethereum { chain_id },
            address: ExternalAddress::Ethereum(address),
            layerx_identity,
            evidence_digest: evidence.digest(),
        })
    }

    fn verify_asset_finality(
        &self,
        evidence: &SourceEvidence,
        _trace: &TraceId,
    ) -> Result<VerifiedAssetFinality, MigrationError> {
        let canonical = evidence.canonical();
        if !canonical.starts_with(b"ethereum-finality-v1") {
            return Err(MigrationError::InvalidEvidence);
        }
        let mut offset = 20;
        let chain_id = u64::from_be_bytes(
            canonical[offset..offset + 8]
                .try_into()
                .map_err(|_| MigrationError::InvalidEvidence)?,
        );
        if chain_id != self.chain_id {
            return Err(MigrationError::InvalidNetwork);
        }
        offset += 8;
        let transaction = SourceTransaction::new(
            canonical[offset..offset + 32]
                .try_into()
                .map_err(|_| MigrationError::InvalidTransaction)?,
        )?;
        offset += 32;
        let source: [u8; 20] = canonical[offset..offset + 20]
            .try_into()
            .map_err(|_| MigrationError::InvalidAddress)?;
        offset += 20;
        let source_asset: [u8; 32] = canonical[offset..offset + 32]
            .try_into()
            .map_err(|_| MigrationError::InvalidEvidence)?;
        offset += 32;
        let source_amount = u128::from_be_bytes(
            canonical[offset..offset + 16]
                .try_into()
                .map_err(|_| MigrationError::InvalidEvidence)?,
        );
        offset += 16;
        let custody_reference: [u8; 32] = canonical[offset..offset + 32]
            .try_into()
            .map_err(|_| MigrationError::InvalidEvidence)?;
        offset += 32;
        let layerx_asset: [u8; 32] = canonical[offset..offset + 32]
            .try_into()
            .map_err(|_| MigrationError::InvalidEvidence)?;
        offset += 32;
        let layerx_amount = u128::from_be_bytes(
            canonical[offset..offset + 16]
                .try_into()
                .map_err(|_| MigrationError::InvalidEvidence)?,
        );
        offset += 16;
        let destination: [u8; 32] = canonical[offset..offset + 32]
            .try_into()
            .map_err(|_| MigrationError::InvalidEvidence)?;
        offset += 32;
        let finality_height = u64::from_be_bytes(
            canonical[offset..offset + 8]
                .try_into()
                .map_err(|_| MigrationError::InvalidEvidence)?,
        );
        Ok(VerifiedAssetFinality {
            chain: SourceChain::Ethereum { chain_id },
            transaction,
            source: ExternalAddress::Ethereum(source),
            source_asset,
            source_amount,
            custody_reference,
            layerx_asset,
            layerx_amount,
            destination,
            finality_height,
            evidence_digest: evidence.digest(),
        })
    }

    fn verify_history(
        &self,
        evidence: &SourceEvidence,
        _trace: &TraceId,
    ) -> Result<VerifiedHistoryPage, MigrationError> {
        let canonical = evidence.canonical();
        if !canonical.starts_with(b"ethereum-history-v1") {
            return Err(MigrationError::InvalidEvidence);
        }
        let mut offset = 19;
        let chain_id = u64::from_be_bytes(
            canonical[offset..offset + 8]
                .try_into()
                .map_err(|_| MigrationError::InvalidEvidence)?,
        );
        if chain_id != self.chain_id {
            return Err(MigrationError::InvalidNetwork);
        }
        offset += 8;
        let record_count = u64::from_be_bytes(
            canonical[offset..offset + 8]
                .try_into()
                .map_err(|_| MigrationError::InvalidEvidence)?,
        ) as usize;
        offset += 8;
        let next_cursor = if canonical[offset] == 1 {
            offset += 1;
            let cursor: [u8; 32] = canonical[offset..offset + 32]
                .try_into()
                .map_err(|_| MigrationError::InvalidEvidence)?;
            offset += 32;
            Some(cursor)
        } else {
            offset += 1;
            None
        };
        let mut records = Vec::new();
        for _ in 0..record_count {
            let transaction = SourceTransaction::new(
                canonical[offset..offset + 32]
                    .try_into()
                    .map_err(|_| MigrationError::InvalidTransaction)?,
            )?;
            offset += 32;
            let address: [u8; 20] = canonical[offset..offset + 20]
                .try_into()
                .map_err(|_| MigrationError::InvalidAddress)?;
            offset += 20;
            let kind = match canonical[offset] {
                1 => ExternalHistoryKind::Incoming,
                2 => ExternalHistoryKind::Outgoing,
                3 => ExternalHistoryKind::Contract,
                _ => return Err(MigrationError::InvalidHistory),
            };
            offset += 1;
            let timestamp = u64::from_be_bytes(
                canonical[offset..offset + 8]
                    .try_into()
                    .map_err(|_| MigrationError::InvalidEvidence)?,
            );
            offset += 8;
            let source_asset: [u8; 32] = canonical[offset..offset + 32]
                .try_into()
                .map_err(|_| MigrationError::InvalidEvidence)?;
            offset += 32;
            let source_amount = u128::from_be_bytes(
                canonical[offset..offset + 16]
                    .try_into()
                    .map_err(|_| MigrationError::InvalidEvidence)?,
            );
            offset += 16;
            records.push(ExternalHistoryRecord {
                chain: SourceChain::Ethereum { chain_id },
                transaction,
                address: ExternalAddress::Ethereum(address),
                kind,
                timestamp,
                source_asset,
                source_amount,
                provenance: ExternalProvenance::Ethereum,
            });
        }
        Ok(VerifiedHistoryPage {
            records,
            next_cursor,
            evidence_digest: evidence.digest(),
        })
    }
}

struct SolanaTestVerifier {
    genesis_hash: [u8; 32],
}

impl SolanaTestVerifier {
    fn new(genesis_hash: [u8; 32]) -> Self {
        Self { genesis_hash }
    }

    fn test_ownership_evidence(
        genesis_hash: [u8; 32],
        address: [u8; 32],
        layerx_identity: [u8; 32],
    ) -> SourceEvidence {
        let canonical = [
            b"solana-signature-v1".as_slice(),
            &genesis_hash,
            &address,
            &layerx_identity,
        ]
        .concat();
        SourceEvidence::new(canonical).unwrap_or_else(|error| panic!("evidence: {error}"))
    }

    fn test_finality_evidence(
        genesis_hash: [u8; 32],
        transaction: SourceTransaction,
        source: [u8; 32],
        source_asset: [u8; 32],
        source_amount: u128,
        custody_reference: [u8; 32],
        layerx_asset: [u8; 32],
        layerx_amount: u128,
        destination: [u8; 32],
        finality_height: u64,
    ) -> SourceEvidence {
        let canonical = [
            b"solana-finality-v1".as_slice(),
            &genesis_hash,
            &transaction.bytes(),
            &source,
            &source_asset,
            &source_amount.to_be_bytes(),
            &custody_reference,
            &layerx_asset,
            &layerx_amount.to_be_bytes(),
            &destination,
            &finality_height.to_be_bytes(),
        ]
        .concat();
        SourceEvidence::new(canonical).unwrap_or_else(|error| panic!("evidence: {error}"))
    }

    fn test_history_evidence(
        genesis_hash: [u8; 32],
        records: Vec<ExternalHistoryRecord>,
        next_cursor: Option<[u8; 32]>,
    ) -> SourceEvidence {
        let mut canonical = Vec::new();
        canonical.extend_from_slice(b"solana-history-v1");
        canonical.extend_from_slice(&genesis_hash);
        canonical.extend_from_slice(&(records.len() as u64).to_be_bytes());
        if let Some(cursor) = next_cursor {
            canonical.push(1);
            canonical.extend_from_slice(&cursor);
        } else {
            canonical.push(0);
        }
        for record in &records {
            if let ExternalAddress::Solana(address) = record.address {
                canonical.extend_from_slice(&record.transaction.bytes());
                canonical.extend_from_slice(&address);
                canonical.push(match record.kind {
                    ExternalHistoryKind::Incoming => 1,
                    ExternalHistoryKind::Outgoing => 2,
                    ExternalHistoryKind::Contract => 3,
                });
                canonical.extend_from_slice(&record.timestamp.to_be_bytes());
                canonical.extend_from_slice(&record.source_asset);
                canonical.extend_from_slice(&record.source_amount.to_be_bytes());
            }
        }
        SourceEvidence::new(canonical).unwrap_or_else(|error| panic!("evidence: {error}"))
    }
}

impl SourceVerifier for SolanaTestVerifier {
    fn verify_ownership(
        &self,
        evidence: &SourceEvidence,
        _trace: &TraceId,
    ) -> Result<VerifiedOwnership, MigrationError> {
        let canonical = evidence.canonical();
        if !canonical.starts_with(b"solana-signature-v1") {
            return Err(MigrationError::InvalidEvidence);
        }
        let genesis_hash: [u8; 32] = canonical[19..51]
            .try_into()
            .map_err(|_| MigrationError::InvalidEvidence)?;
        if genesis_hash != self.genesis_hash {
            return Err(MigrationError::InvalidNetwork);
        }
        let address: [u8; 32] = canonical[51..83]
            .try_into()
            .map_err(|_| MigrationError::InvalidAddress)?;
        let layerx_identity: [u8; 32] = canonical[83..115]
            .try_into()
            .map_err(|_| MigrationError::InvalidEvidence)?;
        Ok(VerifiedOwnership {
            chain: SourceChain::Solana { genesis_hash },
            address: ExternalAddress::Solana(address),
            layerx_identity,
            evidence_digest: evidence.digest(),
        })
    }

    fn verify_asset_finality(
        &self,
        evidence: &SourceEvidence,
        _trace: &TraceId,
    ) -> Result<VerifiedAssetFinality, MigrationError> {
        let canonical = evidence.canonical();
        if !canonical.starts_with(b"solana-finality-v1") {
            return Err(MigrationError::InvalidEvidence);
        }
        let mut offset = 18;
        let genesis_hash: [u8; 32] = canonical[offset..offset + 32]
            .try_into()
            .map_err(|_| MigrationError::InvalidEvidence)?;
        if genesis_hash != self.genesis_hash {
            return Err(MigrationError::InvalidNetwork);
        }
        offset += 32;
        let transaction = SourceTransaction::new(
            canonical[offset..offset + 32]
                .try_into()
                .map_err(|_| MigrationError::InvalidTransaction)?,
        )?;
        offset += 32;
        let source: [u8; 32] = canonical[offset..offset + 32]
            .try_into()
            .map_err(|_| MigrationError::InvalidAddress)?;
        offset += 32;
        let source_asset: [u8; 32] = canonical[offset..offset + 32]
            .try_into()
            .map_err(|_| MigrationError::InvalidEvidence)?;
        offset += 32;
        let source_amount = u128::from_be_bytes(
            canonical[offset..offset + 16]
                .try_into()
                .map_err(|_| MigrationError::InvalidEvidence)?,
        );
        offset += 16;
        let custody_reference: [u8; 32] = canonical[offset..offset + 32]
            .try_into()
            .map_err(|_| MigrationError::InvalidEvidence)?;
        offset += 32;
        let layerx_asset: [u8; 32] = canonical[offset..offset + 32]
            .try_into()
            .map_err(|_| MigrationError::InvalidEvidence)?;
        offset += 32;
        let layerx_amount = u128::from_be_bytes(
            canonical[offset..offset + 16]
                .try_into()
                .map_err(|_| MigrationError::InvalidEvidence)?,
        );
        offset += 16;
        let destination: [u8; 32] = canonical[offset..offset + 32]
            .try_into()
            .map_err(|_| MigrationError::InvalidEvidence)?;
        offset += 32;
        let finality_height = u64::from_be_bytes(
            canonical[offset..offset + 8]
                .try_into()
                .map_err(|_| MigrationError::InvalidEvidence)?,
        );
        Ok(VerifiedAssetFinality {
            chain: SourceChain::Solana { genesis_hash },
            transaction,
            source: ExternalAddress::Solana(source),
            source_asset,
            source_amount,
            custody_reference,
            layerx_asset,
            layerx_amount,
            destination,
            finality_height,
            evidence_digest: evidence.digest(),
        })
    }

    fn verify_history(
        &self,
        evidence: &SourceEvidence,
        _trace: &TraceId,
    ) -> Result<VerifiedHistoryPage, MigrationError> {
        let canonical = evidence.canonical();
        if !canonical.starts_with(b"solana-history-v1") {
            return Err(MigrationError::InvalidEvidence);
        }
        let mut offset = 17;
        let genesis_hash: [u8; 32] = canonical[offset..offset + 32]
            .try_into()
            .map_err(|_| MigrationError::InvalidEvidence)?;
        if genesis_hash != self.genesis_hash {
            return Err(MigrationError::InvalidNetwork);
        }
        offset += 32;
        let record_count = u64::from_be_bytes(
            canonical[offset..offset + 8]
                .try_into()
                .map_err(|_| MigrationError::InvalidEvidence)?,
        ) as usize;
        offset += 8;
        let next_cursor = if canonical[offset] == 1 {
            offset += 1;
            let cursor: [u8; 32] = canonical[offset..offset + 32]
                .try_into()
                .map_err(|_| MigrationError::InvalidEvidence)?;
            offset += 32;
            Some(cursor)
        } else {
            offset += 1;
            None
        };
        let mut records = Vec::new();
        for _ in 0..record_count {
            let transaction = SourceTransaction::new(
                canonical[offset..offset + 32]
                    .try_into()
                    .map_err(|_| MigrationError::InvalidTransaction)?,
            )?;
            offset += 32;
            let address: [u8; 32] = canonical[offset..offset + 32]
                .try_into()
                .map_err(|_| MigrationError::InvalidAddress)?;
            offset += 32;
            let kind = match canonical[offset] {
                1 => ExternalHistoryKind::Incoming,
                2 => ExternalHistoryKind::Outgoing,
                3 => ExternalHistoryKind::Contract,
                _ => return Err(MigrationError::InvalidHistory),
            };
            offset += 1;
            let timestamp = u64::from_be_bytes(
                canonical[offset..offset + 8]
                    .try_into()
                    .map_err(|_| MigrationError::InvalidEvidence)?,
            );
            offset += 8;
            let source_asset: [u8; 32] = canonical[offset..offset + 32]
                .try_into()
                .map_err(|_| MigrationError::InvalidEvidence)?;
            offset += 32;
            let source_amount = u128::from_be_bytes(
                canonical[offset..offset + 16]
                    .try_into()
                    .map_err(|_| MigrationError::InvalidEvidence)?,
            );
            offset += 16;
            records.push(ExternalHistoryRecord {
                chain: SourceChain::Solana { genesis_hash },
                transaction,
                address: ExternalAddress::Solana(address),
                kind,
                timestamp,
                source_asset,
                source_amount,
                provenance: ExternalProvenance::Solana,
            });
        }
        Ok(VerifiedHistoryPage {
            records,
            next_cursor,
            evidence_digest: evidence.digest(),
        })
    }
}

fn registered_migration_gateway() -> GatewayCore {
    let mut core = interop_gateway_core();
    let version =
        SpecVersion::parse("1.0.0").unwrap_or_else(|error| panic!("version: {error}"));
    let spec_id = AdapterId::new("ethereum-solana-migration")
        .unwrap_or_else(|error| panic!("spec id: {error}"));
    let spec = PinnedSpec::new(spec_id, version, [0xea; 32])
        .unwrap_or_else(|error| panic!("spec: {error}"));
    let conformance_id = AdapterId::new("migration-conformance")
        .unwrap_or_else(|error| panic!("conformance id: {error}"));
    let conformance = ConformanceSuite::new(conformance_id, 128, [0xeb; 32])
        .unwrap_or_else(|error| panic!("conformance: {error}"));
    let descriptor = migration_adapter_descriptor(spec, conformance)
        .unwrap_or_else(|error| panic!("descriptor: {error}"));
    core.register_adapter(descriptor, &trace(), 1)
        .unwrap_or_else(|error| panic!("register: {error}"));
    core
}

#[test]
fn ethereum_account_mapping_binds_external_address_to_layerx_identity() {
    let mut gateway = registered_migration_gateway();
    let mut plane = TestPlane::new();
    let verifier = EthereumTestVerifier::new(ETHEREUM_SEPOLIA_CHAIN_ID);
    let binding_policy = TestBindingPolicy;
    let alice = principal("alice");
    let eth_address = [0xe1; 20];
    let layerx_identity = [0xf1; 32];
    let evidence =
        EthereumTestVerifier::test_ownership_evidence(ETHEREUM_SEPOLIA_CHAIN_ID, eth_address, layerx_identity);
    let state = MigrationAdapter::map_account(
        &mut gateway,
        &alice,
        &evidence,
        &verifier,
        &mut plane,
        &binding_policy,
        &trace(),
        10,
    )
    .unwrap_or_else(|error| panic!("map account: {error}"));
    let MigrationState::AccountMapped { .. } = state else {
        panic!("expected AccountMapped, got {state:?}");
    };
    let replay = MigrationAdapter::map_account(
        &mut gateway,
        &alice,
        &evidence,
        &verifier,
        &mut plane,
        &binding_policy,
        &trace(),
        11,
    )
    .unwrap_or_else(|error| panic!("replay: {error}"));
    assert_eq!(state, replay);
}

#[test]
fn ethereum_asset_migration_credits_only_against_verified_finality() {
    let mut gateway = registered_migration_gateway();
    let mut plane = TestPlane::new();
    let verifier = EthereumTestVerifier::new(ETHEREUM_MAINNET_CHAIN_ID);
    let alice = principal("alice");
    let eth_source = [0xe2; 20];
    let transaction = SourceTransaction::new([0xd1; 32])
        .unwrap_or_else(|error| panic!("transaction: {error}"));
    let source_asset = [0xa1; 32];
    let source_amount = 1_000_000_u128;
    let custody_reference = [0xb1; 32];
    let layerx_asset = TEST_ASSET;
    let layerx_amount = 1_000_u128;
    let destination = [0xc1; 32];
    let finality_height = 20_000_000;
    let evidence = EthereumTestVerifier::test_finality_evidence(
        ETHEREUM_MAINNET_CHAIN_ID,
        transaction,
        eth_source,
        source_asset,
        source_amount,
        custody_reference,
        layerx_asset,
        layerx_amount,
        destination,
        finality_height,
    );
    let state = MigrationAdapter::migrate_asset(
        &mut gateway,
        &alice,
        &evidence,
        &verifier,
        &mut plane,
        &trace(),
        20,
    )
    .unwrap_or_else(|error| panic!("migrate asset: {error}"));
    let MigrationState::AssetCredited { .. } = state else {
        panic!("expected AssetCredited, got {state:?}");
    };
}

#[test]
fn ethereum_history_import_labels_records_as_external_provenance() {
    let mut gateway = registered_migration_gateway();
    let verifier = EthereumTestVerifier::new(ETHEREUM_SEPOLIA_CHAIN_ID);
    let mut sink = TestHistorySink::new();
    let alice = principal("alice");
    let eth_address = [0xe3; 20];
    let tx1 = SourceTransaction::new([0xd2; 32])
        .unwrap_or_else(|error| panic!("transaction: {error}"));
    let tx2 = SourceTransaction::new([0xd3; 32])
        .unwrap_or_else(|error| panic!("transaction: {error}"));
    let records = vec![
        ExternalHistoryRecord {
            chain: SourceChain::Ethereum {
                chain_id: ETHEREUM_SEPOLIA_CHAIN_ID,
            },
            transaction: tx1,
            address: ExternalAddress::Ethereum(eth_address),
            kind: ExternalHistoryKind::Incoming,
            timestamp: 1_700_000_010,
            source_asset: [0xa2; 32],
            source_amount: 500_000_u128,
            provenance: ExternalProvenance::Ethereum,
        },
        ExternalHistoryRecord {
            chain: SourceChain::Ethereum {
                chain_id: ETHEREUM_SEPOLIA_CHAIN_ID,
            },
            transaction: tx2,
            address: ExternalAddress::Ethereum(eth_address),
            kind: ExternalHistoryKind::Outgoing,
            timestamp: 1_700_000_020,
            source_asset: [0xa2; 32],
            source_amount: 250_000_u128,
            provenance: ExternalProvenance::Ethereum,
        },
    ];
    let evidence =
        EthereumTestVerifier::test_history_evidence(ETHEREUM_SEPOLIA_CHAIN_ID, records.clone(), None);
    let state = MigrationAdapter::import_history(
        &mut gateway,
        &alice,
        &evidence,
        &verifier,
        &mut sink,
        &trace(),
        30,
    )
    .unwrap_or_else(|error| panic!("import history: {error}"));
    let MigrationState::HistoryImported { record_count } = state else {
        panic!("expected HistoryImported, got {state:?}");
    };
    assert_eq!(record_count, 2);
    assert_eq!(sink.stored.len(), 2);
    for stored in &sink.stored {
        assert_eq!(stored.provenance, ExternalProvenance::Ethereum);
    }
}

#[test]
fn solana_account_mapping_binds_external_address_to_layerx_identity() {
    let mut gateway = registered_migration_gateway();
    let mut plane = TestPlane::new();
    let verifier = SolanaTestVerifier::new(SOLANA_TESTNET_GENESIS);
    let binding_policy = TestBindingPolicy;
    let bob = principal("bob");
    let sol_address = [0xe4; 32];
    let layerx_identity = [0xf2; 32];
    let evidence =
        SolanaTestVerifier::test_ownership_evidence(SOLANA_TESTNET_GENESIS, sol_address, layerx_identity);
    let state = MigrationAdapter::map_account(
        &mut gateway,
        &bob,
        &evidence,
        &verifier,
        &mut plane,
        &binding_policy,
        &trace(),
        40,
    )
    .unwrap_or_else(|error| panic!("map account: {error}"));
    let MigrationState::AccountMapped { .. } = state else {
        panic!("expected AccountMapped, got {state:?}");
    };
}

#[test]
fn solana_asset_migration_credits_only_against_verified_finality() {
    let mut gateway = registered_migration_gateway();
    let mut plane = TestPlane::new();
    let verifier = SolanaTestVerifier::new(SOLANA_MAINNET_GENESIS);
    let carol = principal("carol");
    let sol_source = [0xe5; 32];
    let transaction = SourceTransaction::new([0xd4; 32])
        .unwrap_or_else(|error| panic!("transaction: {error}"));
    let source_asset = [0xa3; 32];
    let source_amount = 2_000_000_u128;
    let custody_reference = [0xb2; 32];
    let layerx_asset = TEST_ASSET;
    let layerx_amount = 2_000_u128;
    let destination = [0xc2; 32];
    let finality_height = 150_000_000;
    let evidence = SolanaTestVerifier::test_finality_evidence(
        SOLANA_MAINNET_GENESIS,
        transaction,
        sol_source,
        source_asset,
        source_amount,
        custody_reference,
        layerx_asset,
        layerx_amount,
        destination,
        finality_height,
    );
    let state = MigrationAdapter::migrate_asset(
        &mut gateway,
        &carol,
        &evidence,
        &verifier,
        &mut plane,
        &trace(),
        50,
    )
    .unwrap_or_else(|error| panic!("migrate asset: {error}"));
    let MigrationState::AssetCredited { .. } = state else {
        panic!("expected AssetCredited, got {state:?}");
    };
}

#[test]
fn solana_history_import_labels_records_as_external_provenance() {
    let mut gateway = registered_migration_gateway();
    let verifier = SolanaTestVerifier::new(SOLANA_MAINNET_GENESIS);
    let mut sink = TestHistorySink::new();
    let dave = principal("dave");
    let sol_address = [0xe6; 32];
    let tx1 = SourceTransaction::new([0xd5; 32])
        .unwrap_or_else(|error| panic!("transaction: {error}"));
    let records = vec![ExternalHistoryRecord {
        chain: SourceChain::Solana {
            genesis_hash: SOLANA_MAINNET_GENESIS,
        },
        transaction: tx1,
        address: ExternalAddress::Solana(sol_address),
        kind: ExternalHistoryKind::Contract,
        timestamp: 1_700_000_030,
        source_asset: [0xa4; 32],
        source_amount: 750_000_u128,
        provenance: ExternalProvenance::Solana,
    }];
    let evidence =
        SolanaTestVerifier::test_history_evidence(SOLANA_MAINNET_GENESIS, records.clone(), None);
    let state = MigrationAdapter::import_history(
        &mut gateway,
        &dave,
        &evidence,
        &verifier,
        &mut sink,
        &trace(),
        60,
    )
    .unwrap_or_else(|error| panic!("import history: {error}"));
    let MigrationState::HistoryImported { record_count } = state else {
        panic!("expected HistoryImported, got {state:?}");
    };
    assert_eq!(record_count, 1);
    assert_eq!(sink.stored.len(), 1);
    assert_eq!(sink.stored[0].provenance, ExternalProvenance::Solana);
}

#[test]
fn migration_refuses_claims_lacking_verifiable_finality_evidence() {
    let mut gateway = registered_migration_gateway();
    let verifier = EthereumTestVerifier::new(ETHEREUM_MAINNET_CHAIN_ID);
    let eve = principal("eve");
    let invalid_evidence =
        SourceEvidence::new(b"invalid-ethereum-finality-evidence".to_vec())
            .unwrap_or_else(|error| panic!("evidence: {error}"));
    let mut plane = TestPlane::new();
    let result = MigrationAdapter::migrate_asset(
        &mut gateway,
        &eve,
        &invalid_evidence,
        &verifier,
        &mut plane,
        &trace(),
        70,
    );
    assert!(
        result.is_err(),
        "migration must refuse invalid finality evidence"
    );
}

#[test]
fn ethereum_and_solana_codify_anchors_are_stable() {
    assert_eq!(
        layerx_migrate::interop_migrate_ethereum(),
        "verified-finality-ethereum-migration"
    );
    assert_eq!(
        layerx_migrate::interop_migrate_solana(),
        "verified-finality-solana-migration"
    );
}
