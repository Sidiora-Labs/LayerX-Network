#![forbid(unsafe_code)]

mod freshness;
pub mod mirror;
pub mod programs;
mod query;
pub mod verify;

use std::collections::{BTreeMap, BTreeSet};

use layerx_agentd::read::LayerxdProgramBalanceReader;
use layerx_client::availability::AvailabilityResult;
use layerx_client::head::Head;
use layerx_programs_protocol_adapter::ProtocolAdapterError;
use layerx_proof::availability::RootCommitments;
use layerx_proof::checkpoint::{verify_certificate, Certificate, CheckpointError, GuarantorKey};
use layerx_proof::receipt::{verify_outcome, AuthorizedBatch, ReceiptCheck};
use layerx_types::verify::VerificationLevel;
use sha2::{Digest as _, Sha256};

pub use freshness::{Freshness, Indexed};
use programs::{ExplorerProgram, ExplorerProgramReadError};
pub use query::{Page, PublicExplorer, QueryError, QueryFailure, VerificationFailure};

/// Stable identity of the rebuildable public projection.
pub const CRATE_IDENTITY: &str = "layerx-explorer-index";

const RECORD_DOMAIN: &[u8] = b"layerx-explorer-record/v1\0";

/// Content identity for one exact protocol-public record.
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub struct RecordId([u8; 32]);

impl RecordId {
    /// Reconstructs a public record identifier from its exact link bytes.
    #[must_use]
    pub const fn from_bytes(bytes: [u8; 32]) -> Self {
        Self(bytes)
    }

    #[must_use]
    pub const fn bytes(self) -> [u8; 32] {
        self.0
    }
}

/// A protocol-public receipt or event, never profile or principal-local state.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PublicRecord {
    pub id: RecordId,
    pub batch_number: u64,
    pub ordinal: u32,
    pub canonical_bytes: Vec<u8>,
    pub verification_level: VerificationLevel,
}

/// One verified guarantor signature retained with its checkpoint.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct CheckpointSignature {
    pub guarantor_id: [u8; 32],
    pub signature: [u8; 64],
}

/// Public checkpoint material whose threshold certificate passed locally.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CheckpointRecord {
    pub checkpoint_id: [u8; 32],
    pub batch_number: u64,
    pub first_sequence: u64,
    pub last_sequence: u64,
    pub header_bytes: Vec<u8>,
    pub validity_proof: Vec<u8>,
    pub data_availability_root: [u8; 32],
    pub record_roots: RootCommitments,
    pub signatures: Vec<CheckpointSignature>,
    pub achieved_signatures: usize,
    pub required_signatures: usize,
    pub settlement_reference: Option<Vec<u8>>,
    pub verification_level: VerificationLevel,
}

/// Public batch projection derived only from a complete availability result.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct BatchRecord {
    pub batch_number: u64,
    pub data_availability_root: [u8; 32],
    pub record_roots: RootCommitments,
    pub total_availability_bytes: usize,
    pub activity_ids: Vec<RecordId>,
    pub receipt_ids: Vec<RecordId>,
    pub event_ids: Vec<RecordId>,
    pub oracle_input_ids: Vec<RecordId>,
    pub checkpoint_id: Option<[u8; 32]>,
    pub verification_level: VerificationLevel,
}

/// One account-affecting fact decoded only after its canonical receipt and
/// independent sequencer authority pass `layerx-proof`.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AccountActivityRecord {
    pub receipt_id: RecordId,
    pub receipt_digest: [u8; 32],
    pub batch_number: u64,
    pub global_sequence: u64,
    pub activity_id: [u8; 32],
    pub operation: u8,
    pub result_code: i32,
    pub asset: [u8; 32],
    pub amount: u128,
    pub from: [u8; 32],
    pub to: [u8; 32],
    pub verification_level: VerificationLevel,
}

/// Deterministic query image used by rebuild and replica convergence checks.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct IndexSnapshot {
    pub freshness: Freshness,
    pub checkpoints: Vec<CheckpointRecord>,
    pub batches: Vec<BatchRecord>,
    pub receipts: Vec<PublicRecord>,
    pub events: Vec<PublicRecord>,
    pub account_activities: Vec<AccountActivityRecord>,
    pub receipt_authority_batches: Vec<u64>,
    pub programs: Vec<ExplorerProgram>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum IngestOutcome {
    Inserted,
    AlreadyPresent,
}

/// Fail-closed refusal for boundary regression, mismatched evidence or replay.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum IndexError {
    HeadRegression,
    BatchAheadOfHead {
        batch: u64,
        head: u64,
    },
    CheckpointHeadMismatch {
        expected: [u8; 32],
        actual: [u8; 32],
    },
    Checkpoint(CheckpointError),
    ConflictingCheckpoint {
        batch: u64,
    },
    ConflictingBatch {
        batch: u64,
    },
    AvailabilityCheckpointMismatch {
        batch: u64,
    },
    ReplayedPublicRecord {
        id: RecordId,
    },
    ReplayedProtocolReceipt {
        identifier: [u8; 32],
    },
    ReceiptVerification {
        id: RecordId,
        check: ReceiptCheck,
    },
    RecordCountOverflow,
    ProgramRead(ExplorerProgramReadError),
    ProgramProtocolUnavailable(ProtocolAdapterError),
    ProgramHeadRegression {
        program: [u8; 32],
    },
    ConflictingProgram {
        program: [u8; 32],
    },
}

/// Running explorer ingestion bridge. It refreshes the exact program record
/// from the same layerxd evidence service used by agent reads before every
/// index update; no caller-supplied balance list can enter the projection.
pub struct ProtocolProgramIngestor {
    reader: LayerxdProgramBalanceReader,
    staleness_limit: u64,
}

impl ProtocolProgramIngestor {
    #[must_use]
    pub const fn new(reader: LayerxdProgramBalanceReader) -> Self {
        let staleness_limit = reader.staleness_limit();
        Self {
            reader,
            staleness_limit,
        }
    }

    pub fn ingest(
        &mut self,
        index: &mut Indexer,
        mut registry_read: layerx_programs::VerifiedRegistryRead,
        now: u64,
    ) -> Result<IngestOutcome, IndexError> {
        let state = self
            .reader
            .read_protocol_state(registry_read.entry.program, now)
            .map_err(IndexError::ProgramProtocolUnavailable)?;
        registry_read.entry.lifecycle = state.balances().lifecycle();
        registry_read.entry.value_accounts = state.balances().bindings().to_vec();
        registry_read.entry.exit_routes = state.routes().to_vec();
        registry_read.entry.lifecycle_history = state.history().to_vec();
        registry_read.receipt_digest = state.balances().receipt_digest();
        registry_read.freshness = state.balances().freshness();
        index.ingest_program(registry_read, &state, now, self.staleness_limit)
    }
}

/// Rebuildable, non-authoritative projection over proof-gated boundary data.
pub struct Indexer {
    observed_head: Head,
    checkpoints: BTreeMap<[u8; 32], CheckpointRecord>,
    checkpoints_by_batch: BTreeMap<u64, [u8; 32]>,
    batches: BTreeMap<u64, BatchRecord>,
    receipts: BTreeMap<RecordId, PublicRecord>,
    receipts_by_protocol_id: BTreeMap<[u8; 32], RecordId>,
    events: BTreeMap<RecordId, PublicRecord>,
    account_activities: BTreeMap<RecordId, AccountActivityRecord>,
    receipt_authority_batches: BTreeSet<u64>,
    programs: BTreeMap<[u8; 32], ExplorerProgram>,
}

impl Indexer {
    /// Starts an empty projection relative to the accepted node-boundary head.
    #[must_use]
    pub const fn new(observed_head: Head) -> Self {
        Self {
            observed_head,
            checkpoints: BTreeMap::new(),
            checkpoints_by_batch: BTreeMap::new(),
            batches: BTreeMap::new(),
            receipts: BTreeMap::new(),
            receipts_by_protocol_id: BTreeMap::new(),
            events: BTreeMap::new(),
            account_activities: BTreeMap::new(),
            receipt_authority_batches: BTreeSet::new(),
            programs: BTreeMap::new(),
        }
    }

    /// Advances freshness from a later accepted handshake without inventing
    /// data for the newly observed range.
    ///
    /// # Errors
    ///
    /// Refuses chain-sequence or sealed-batch regression.
    pub fn refresh_head(&mut self, head: Head) -> Result<(), IndexError> {
        if head.chain_sequence < self.observed_head.chain_sequence
            || head.sealed_batch < self.observed_head.sealed_batch
        {
            return Err(IndexError::HeadRegression);
        }
        self.observed_head = head;
        Ok(())
    }

    /// Verifies and indexes one checkpoint certificate from the node boundary.
    ///
    /// # Errors
    ///
    /// Refuses invalid certificates, head-inconsistent latest checkpoints,
    /// conflicting batch certificates, and mismatched availability evidence.
    pub fn ingest_checkpoint(
        &mut self,
        certificate: &Certificate,
        bonded_set: &[GuarantorKey],
        registered_checkpoint_id: [u8; 32],
        registered_settlement_reference: Option<&[u8]>,
    ) -> Result<IngestOutcome, IndexError> {
        let report = verify_certificate(
            certificate,
            bonded_set,
            &registered_checkpoint_id,
            registered_settlement_reference,
        )
        .map_err(IndexError::Checkpoint)?;
        let batch_number = report.batch_number();
        self.require_not_ahead(batch_number)?;
        if batch_number == self.observed_head.sealed_batch
            && registered_checkpoint_id != self.observed_head.finalised_checkpoint
        {
            return Err(IndexError::CheckpointHeadMismatch {
                expected: self.observed_head.finalised_checkpoint,
                actual: registered_checkpoint_id,
            });
        }
        let record = CheckpointRecord {
            checkpoint_id: registered_checkpoint_id,
            batch_number,
            first_sequence: report.first_sequence(),
            last_sequence: report.last_sequence(),
            header_bytes: certificate.checkpoint().header_bytes().to_vec(),
            validity_proof: certificate.checkpoint().validity_proof().to_vec(),
            data_availability_root: report.data_availability_root(),
            record_roots: report.record_roots(),
            signatures: certificate
                .attestations()
                .iter()
                .map(|attestation| CheckpointSignature {
                    guarantor_id: attestation.guarantor_id(),
                    signature: attestation.signature(),
                })
                .collect(),
            achieved_signatures: report.achieved,
            required_signatures: report.required,
            settlement_reference: certificate.settlement_reference().map(<[u8]>::to_vec),
            verification_level: report.level(),
        };
        if let Some(existing) = self.checkpoints.get(&registered_checkpoint_id) {
            return if existing == &record {
                Ok(IngestOutcome::AlreadyPresent)
            } else {
                Err(IndexError::ConflictingCheckpoint {
                    batch: batch_number,
                })
            };
        }
        if self.checkpoints_by_batch.contains_key(&batch_number) {
            return Err(IndexError::ConflictingCheckpoint {
                batch: batch_number,
            });
        }
        if let Some(batch) = self.batches.get(&batch_number) {
            require_matching_evidence(batch, &record)?;
        }
        self.checkpoints_by_batch
            .insert(batch_number, registered_checkpoint_id);
        self.checkpoints
            .insert(registered_checkpoint_id, record.clone());
        self.upgrade_batch(&record);
        Ok(IngestOutcome::Inserted)
    }

    /// Indexes a complete data-availability result returned by `layerx-client`.
    /// Its private construction path guarantees chunk and record-root proof
    /// verification before this method can observe any bytes.
    ///
    /// # Errors
    ///
    /// Refuses data ahead of the accepted head, divergent batches, checkpoint
    /// root mismatches and cross-batch record replay without partial insertion.
    pub fn ingest_availability(
        &mut self,
        result: &AvailabilityResult,
    ) -> Result<IngestOutcome, IndexError> {
        let batch_number = result.batch_number();
        self.require_not_ahead(batch_number)?;
        let checkpoint = self
            .checkpoints_by_batch
            .get(&batch_number)
            .and_then(|identifier| self.checkpoints.get(identifier));
        if let Some(checkpoint) = checkpoint {
            if checkpoint.data_availability_root != result.data_availability_root()
                || checkpoint.record_roots != result.record_roots()
            {
                return Err(IndexError::AvailabilityCheckpointMismatch {
                    batch: batch_number,
                });
            }
        }
        let level = checkpoint.map_or(VerificationLevel::UNVERIFIED, |record| {
            record.verification_level
        });
        let records = result.records();
        let activity_ids = record_ids(b"activity", &records.activities);
        let receipt_ids = record_ids(b"receipt", &records.receipts);
        let event_ids = record_ids(b"event", &records.events);
        let oracle_input_ids = record_ids(b"oracle", &records.oracle_inputs);
        let batch = BatchRecord {
            batch_number,
            data_availability_root: result.data_availability_root(),
            record_roots: result.record_roots(),
            total_availability_bytes: result.report.total_bytes,
            activity_ids,
            receipt_ids: receipt_ids.clone(),
            event_ids: event_ids.clone(),
            oracle_input_ids,
            checkpoint_id: checkpoint.map(|record| record.checkpoint_id),
            verification_level: level,
        };
        if let Some(existing) = self.batches.get(&batch_number) {
            return if existing == &batch {
                Ok(IngestOutcome::AlreadyPresent)
            } else {
                Err(IndexError::ConflictingBatch {
                    batch: batch_number,
                })
            };
        }
        let staged_receipts = stage_records(b"receipt", batch_number, &records.receipts, level)?;
        let staged_events = stage_records(b"event", batch_number, &records.events, level)?;
        require_no_replay(&self.receipts, &staged_receipts)?;
        require_no_replay(&self.events, &staged_events)?;
        self.receipts.extend(
            staged_receipts
                .into_iter()
                .map(|record| (record.id, record)),
        );
        self.events
            .extend(staged_events.into_iter().map(|record| (record.id, record)));
        self.batches.insert(batch_number, batch);
        Ok(IngestOutcome::Inserted)
    }

    /// Verifies every canonical receipt in one indexed batch against
    /// independently supplied core batch authority, then materialises the
    /// protocol-public account activity view atomically.
    ///
    /// # Errors
    ///
    /// Refuses an unknown batch or the first receipt check that fails. No
    /// account activity row or completeness marker is inserted on failure.
    pub fn ingest_receipt_authority(
        &mut self,
        batch_number: u64,
        authorised: &AuthorizedBatch,
    ) -> Result<IngestOutcome, IndexError> {
        let batch = self
            .batches
            .get(&batch_number)
            .ok_or(IndexError::ConflictingBatch {
                batch: batch_number,
            })?;
        let mut staged = Vec::with_capacity(batch.receipt_ids.len());
        for identifier in &batch.receipt_ids {
            let record = self
                .receipts
                .get(identifier)
                .ok_or(IndexError::ReplayedPublicRecord { id: *identifier })?;
            let verified =
                verify_outcome(&record.canonical_bytes, authorised).map_err(|failure| {
                    IndexError::ReceiptVerification {
                        id: *identifier,
                        check: failure.check,
                    }
                })?;
            let receipt = verified
                .receipt()
                .protocol()
                .ok_or(IndexError::ReceiptVerification {
                    id: *identifier,
                    check: ReceiptCheck::ReceiptShape,
                })?;
            let receipt_digest =
                verified
                    .evidence()
                    .receipt_digest()
                    .ok_or(IndexError::ReceiptVerification {
                        id: *identifier,
                        check: ReceiptCheck::ReceiptShape,
                    })?;
            let verification_level =
                if record.verification_level >= VerificationLevel::CHECKPOINT_FINALISED {
                    record.verification_level
                } else {
                    verified.level()
                };
            staged.push(AccountActivityRecord {
                receipt_id: *identifier,
                receipt_digest,
                batch_number,
                global_sequence: receipt.global_sequence(),
                activity_id: receipt.activity_id(),
                operation: receipt.operation(),
                result_code: receipt.result_code(),
                asset: receipt.asset(),
                amount: receipt.amount(),
                from: receipt.from(),
                to: receipt.to(),
                verification_level,
            });
        }
        if self.receipt_authority_batches.contains(&batch_number) {
            let mut existing = self
                .account_activities
                .values()
                .filter(|record| record.batch_number == batch_number)
                .cloned()
                .collect::<Vec<_>>();
            existing.sort_by_key(|record| record.receipt_id);
            staged.sort_by_key(|record| record.receipt_id);
            return if existing == staged {
                Ok(IngestOutcome::AlreadyPresent)
            } else {
                Err(IndexError::ConflictingBatch {
                    batch: batch_number,
                })
            };
        }
        let mut protocol_ids = BTreeMap::new();
        for record in &staged {
            for lookup in [record.activity_id, record.receipt_digest] {
                let existing = self
                    .receipts_by_protocol_id
                    .get(&lookup)
                    .copied()
                    .or_else(|| protocol_ids.get(&lookup).copied());
                if existing.is_some_and(|identifier| identifier != record.receipt_id) {
                    return Err(IndexError::ReplayedProtocolReceipt { identifier: lookup });
                }
                protocol_ids.insert(lookup, record.receipt_id);
            }
        }
        self.receipts_by_protocol_id.extend(protocol_ids);
        self.account_activities
            .extend(staged.into_iter().map(|record| (record.receipt_id, record)));
        self.receipt_authority_batches.insert(batch_number);
        Ok(IngestOutcome::Inserted)
    }

    #[must_use]
    pub fn freshness(&self) -> Freshness {
        Freshness {
            observed_chain_sequence: self.observed_head.chain_sequence,
            observed_sealed_batch: self.observed_head.sealed_batch,
            observed_finalised_checkpoint: self.observed_head.finalised_checkpoint,
            indexed_batch: self.batches.keys().next_back().copied(),
            indexed_checkpoint: self
                .checkpoints_by_batch
                .iter()
                .next_back()
                .map(|(_, identifier)| *identifier),
        }
    }

    #[must_use]
    pub fn checkpoint(&self, identifier: [u8; 32]) -> Indexed<Option<CheckpointRecord>> {
        Indexed {
            value: self.checkpoints.get(&identifier).cloned(),
            freshness: self.freshness(),
        }
    }

    #[must_use]
    pub fn batch(&self, batch_number: u64) -> Indexed<Option<BatchRecord>> {
        Indexed {
            value: self.batches.get(&batch_number).cloned(),
            freshness: self.freshness(),
        }
    }

    #[must_use]
    pub fn receipt(&self, identifier: RecordId) -> Indexed<Option<PublicRecord>> {
        Indexed {
            value: self.receipts.get(&identifier).cloned(),
            freshness: self.freshness(),
        }
    }

    #[must_use]
    pub fn event(&self, identifier: RecordId) -> Indexed<Option<PublicRecord>> {
        Indexed {
            value: self.events.get(&identifier).cloned(),
            freshness: self.freshness(),
        }
    }

    /// Projects one production adapter read into the public program index.
    /// Existing rows are refreshed only by a later current-head proof for the
    /// exact same program identity.
    pub fn ingest_program(
        &mut self,
        registry_read: layerx_programs::VerifiedRegistryRead,
        state: &layerx_programs_protocol_adapter::ProtocolProgramStateRead,
        now: u64,
        staleness_limit: u64,
    ) -> Result<IngestOutcome, IndexError> {
        let program =
            ExplorerProgram::from_protocol_state(registry_read, state, now, staleness_limit)
                .map_err(IndexError::ProgramRead)?;
        let identifier = program.identifier;
        if let Some(existing) = self.programs.get(&identifier) {
            if program.balance_observed_sequence < existing.balance_observed_sequence {
                return Err(IndexError::ProgramHeadRegression {
                    program: identifier,
                });
            }
            if program.balance_observed_sequence == existing.balance_observed_sequence {
                return if existing == &program {
                    Ok(IngestOutcome::AlreadyPresent)
                } else {
                    Err(IndexError::ConflictingProgram {
                        program: identifier,
                    })
                };
            }
        }
        self.programs.insert(identifier, program);
        Ok(IngestOutcome::Inserted)
    }

    /// Returns the current proof-gated program projection.
    #[must_use]
    pub fn program(&self, identifier: [u8; 32]) -> Indexed<Option<ExplorerProgram>> {
        Indexed {
            value: self.programs.get(&identifier).cloned(),
            freshness: self.freshness(),
        }
    }

    /// Returns a stable, map-order-independent image of every public query row.
    #[must_use]
    pub fn snapshot(&self) -> IndexSnapshot {
        IndexSnapshot {
            freshness: self.freshness(),
            checkpoints: self.checkpoints.values().cloned().collect(),
            batches: self.batches.values().cloned().collect(),
            receipts: self.receipts.values().cloned().collect(),
            events: self.events.values().cloned().collect(),
            account_activities: self.account_activities.values().cloned().collect(),
            receipt_authority_batches: self.receipt_authority_batches.iter().copied().collect(),
            programs: self.programs.values().cloned().collect(),
        }
    }

    fn require_not_ahead(&self, batch: u64) -> Result<(), IndexError> {
        if batch > self.observed_head.sealed_batch {
            Err(IndexError::BatchAheadOfHead {
                batch,
                head: self.observed_head.sealed_batch,
            })
        } else {
            Ok(())
        }
    }

    fn upgrade_batch(&mut self, checkpoint: &CheckpointRecord) {
        let Some(batch) = self.batches.get_mut(&checkpoint.batch_number) else {
            return;
        };
        batch.checkpoint_id = Some(checkpoint.checkpoint_id);
        batch.verification_level = checkpoint.verification_level;
        for identifier in &batch.receipt_ids {
            if let Some(record) = self.receipts.get_mut(identifier) {
                record.verification_level = checkpoint.verification_level;
            }
        }
        for identifier in &batch.event_ids {
            if let Some(record) = self.events.get_mut(identifier) {
                record.verification_level = checkpoint.verification_level;
            }
        }
        for record in self
            .account_activities
            .values_mut()
            .filter(|record| record.batch_number == checkpoint.batch_number)
        {
            record.verification_level = checkpoint.verification_level;
        }
    }
}

fn require_matching_evidence(
    batch: &BatchRecord,
    checkpoint: &CheckpointRecord,
) -> Result<(), IndexError> {
    if batch.data_availability_root == checkpoint.data_availability_root
        && batch.record_roots == checkpoint.record_roots
    {
        Ok(())
    } else {
        Err(IndexError::AvailabilityCheckpointMismatch {
            batch: batch.batch_number,
        })
    }
}

fn record_ids(kind: &[u8], records: &[Vec<u8>]) -> Vec<RecordId> {
    records.iter().map(|bytes| record_id(kind, bytes)).collect()
}

fn stage_records(
    kind: &[u8],
    batch_number: u64,
    records: &[Vec<u8>],
    verification_level: VerificationLevel,
) -> Result<Vec<PublicRecord>, IndexError> {
    records
        .iter()
        .enumerate()
        .map(|(ordinal, bytes)| {
            let ordinal = u32::try_from(ordinal).map_err(|_| IndexError::RecordCountOverflow)?;
            Ok(PublicRecord {
                id: record_id(kind, bytes),
                batch_number,
                ordinal,
                canonical_bytes: bytes.clone(),
                verification_level,
            })
        })
        .collect()
}

fn require_no_replay(
    existing: &BTreeMap<RecordId, PublicRecord>,
    staged: &[PublicRecord],
) -> Result<(), IndexError> {
    let mut in_batch = BTreeSet::new();
    for record in staged {
        if existing.contains_key(&record.id) || !in_batch.insert(record.id) {
            return Err(IndexError::ReplayedPublicRecord { id: record.id });
        }
    }
    Ok(())
}

fn record_id(kind: &[u8], bytes: &[u8]) -> RecordId {
    let mut hasher = Sha256::new();
    hasher.update(RECORD_DOMAIN);
    hasher.update(kind);
    hasher.update(bytes);
    RecordId(hasher.finalize().into())
}
