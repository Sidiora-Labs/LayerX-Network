//! Tenant-scoped audit slices with independently verifiable protocol evidence.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt::{Display, Formatter};
use std::path::Path;

use layerx_proof::export::{
    verify as verify_protocol_evidence, ExportVerificationError, OfflineExport,
};
use layerx_proof::checkpoint::SettlementDomain;
use layerx_types::ids::Did;
use sha2::{Digest, Sha256};

use crate::store::TenantId;
use crate::tenant::{Config, RedactionPolicy};

use super::log::{entry_hash, read_frames, verify_chain, AuditError, VerifiedFrame};
use super::records::{decode_entry, Entry, RecordError};
use super::PayloadEvidence;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Query {
    pub tenant: TenantId,
    pub agent: Option<Did>,
    pub from_observed_at_ms: Option<u64>,
    pub through_observed_at_ms: Option<u64>,
}

impl Query {
    fn includes(&self, entry: &Entry) -> bool {
        self.agent
            .as_ref()
            .is_none_or(|agent| agent == &entry.agent)
            && self
                .from_observed_at_ms
                .is_none_or(|start| entry.observed_at_ms >= start)
            && self
                .through_observed_at_ms
                .is_none_or(|end| entry.observed_at_ms <= end)
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ExportedEntry {
    pub sequence: u64,
    pub entry: Entry,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ChainLink {
    pub sequence: u64,
    pub previous_hash: [u8; 32],
    pub payload_digest: [u8; 32],
    pub entry_hash: [u8; 32],
    pub selected: bool,
    pub canonical_entry_bytes: Option<Vec<u8>>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ChainMaterial {
    pub tenant_hash: [u8; 32],
    pub anchor_entries: u64,
    pub anchor_tail_hash: [u8; 32],
    pub links: Vec<ChainLink>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ReferencedEvidence {
    pub receipt_id: [u8; 32],
    pub protocol_facts: OfflineExport,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AuditExport {
    pub query: Query,
    pub entries: Vec<ExportedEntry>,
    pub chain: ChainMaterial,
    pub referenced_evidence: Vec<ReferencedEvidence>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct EvidenceStore {
    tenant: TenantId,
    by_receipt: BTreeMap<[u8; 32], OfflineExport>,
}

impl EvidenceStore {
    #[must_use]
    pub fn new(tenant: TenantId) -> Self {
        Self {
            tenant,
            by_receipt: BTreeMap::new(),
        }
    }

    pub fn insert(&mut self, receipt_id: [u8; 32], evidence: OfflineExport) {
        self.by_receipt.insert(receipt_id, evidence);
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ExportError {
    Audit(String),
    Record(String),
    WrongTenant,
    InvalidTimeRange,
    EmptySlice,
    EvidenceUnavailable {
        receipt_id: [u8; 32],
    },
    EvidenceMismatch {
        receipt_id: [u8; 32],
    },
    EvidenceInvalid {
        receipt_id: [u8; 32],
        error: ExportVerificationError,
    },
}

impl Display for ExportError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Audit(error) => write!(formatter, "audit chain unavailable: {error}"),
            Self::Record(error) => write!(formatter, "audit record unavailable: {error}"),
            Self::WrongTenant => formatter.write_str("audit export tenant does not own the data"),
            Self::InvalidTimeRange => formatter.write_str("audit export time range is reversed"),
            Self::EmptySlice => formatter.write_str("audit export slice contains no records"),
            Self::EvidenceUnavailable { receipt_id } => write!(
                formatter,
                "referenced receipt evidence {} is unavailable",
                hex(receipt_id)
            ),
            Self::EvidenceMismatch { receipt_id } => write!(
                formatter,
                "evidence does not contain referenced receipt {}",
                hex(receipt_id)
            ),
            Self::EvidenceInvalid { receipt_id, error } => write!(
                formatter,
                "referenced evidence {} failed verification: {error:?}",
                hex(receipt_id)
            ),
        }
    }
}

impl std::error::Error for ExportError {}

impl From<AuditError> for ExportError {
    fn from(value: AuditError) -> Self {
        Self::Audit(value.to_string())
    }
}

impl From<RecordError> for ExportError {
    fn from(value: RecordError) -> Self {
        Self::Record(value.to_string())
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ChainError {
    Empty,
    Sequence,
    Link,
    PayloadDigest,
    EntryHash,
    Anchor,
    Selection,
    UnselectedPayload,
}

impl Display for ChainError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        let reason = match self {
            Self::Empty => "chain material is empty",
            Self::Sequence => "chain sequence is not contiguous",
            Self::Link => "chain link does not name its predecessor",
            Self::PayloadDigest => "canonical entry bytes do not match their digest",
            Self::EntryHash => "canonical entry bytes do not match the audit hash",
            Self::Anchor => "chain material does not reach the durable tail anchor",
            Self::Selection => "chain material does not identify an exported record",
            Self::UnselectedPayload => "chain material exposes an unselected record",
        };
        formatter.write_str(reason)
    }
}

impl std::error::Error for ChainError {}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ReviewError {
    Chain(ChainError),
    Query,
    EntryBinding,
    EvidenceSet,
    Evidence {
        receipt_id: [u8; 32],
        error: ExportVerificationError,
    },
}

impl Display for ReviewError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Chain(error) => write!(formatter, "audit chain verification failed: {error}"),
            Self::Query => formatter.write_str("exported audit record is outside the stated query"),
            Self::EntryBinding => {
                formatter.write_str("exported audit record is not bound to its chain link")
            }
            Self::EvidenceSet => {
                formatter.write_str("referenced receipt and evidence sets do not match")
            }
            Self::Evidence { receipt_id, error } => write!(
                formatter,
                "protocol evidence {} failed verification: {error:?}",
                hex(receipt_id)
            ),
        }
    }
}

impl std::error::Error for ReviewError {}

impl From<ChainError> for ReviewError {
    fn from(value: ChainError) -> Self {
        Self::Chain(value)
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ReviewReport {
    pub exported_entries: usize,
    pub verified_receipts: usize,
    pub verified_inclusions: usize,
    pub verified_checkpoints: usize,
    pub failed_records: usize,
}

/// Exports one tenant-owned audit slice and every protocol evidence bundle it references.
///
/// # Errors
///
/// Returns an error when the tenant or range is invalid, the chain cannot be verified, the
/// slice is empty, or referenced evidence is missing, mismatched, or invalid.
pub fn export(
    log_path: impl AsRef<Path>,
    config: &Config,
    query: Query,
    evidence_store: &EvidenceStore,
    expected_settlement_domain: SettlementDomain,
) -> Result<AuditExport, ExportError> {
    if query.tenant != config.tenant || evidence_store.tenant != config.tenant {
        return Err(ExportError::WrongTenant);
    }
    if matches!(
        (query.from_observed_at_ms, query.through_observed_at_ms),
        (Some(start), Some(end)) if start > end
    ) {
        return Err(ExportError::InvalidTimeRange);
    }

    let verification = verify_chain(log_path.as_ref())?;
    let expected_tenant_hash: [u8; 32] = Sha256::digest(config.tenant.as_str().as_bytes()).into();
    if verification.tenant_hash != expected_tenant_hash {
        return Err(ExportError::WrongTenant);
    }
    let frames = read_frames(log_path)?;
    let decoded = decode_owned_entries(&frames, config)?;

    let selected_sequences: BTreeSet<_> = decoded
        .iter()
        .enumerate()
        .filter(|(_, entry)| query.includes(entry))
        .map(|(sequence, _)| sequence as u64)
        .collect();
    let first_sequence = selected_sequences
        .first()
        .copied()
        .ok_or(ExportError::EmptySlice)?;
    let current_sequence = verification.entries.saturating_sub(1);
    let mut entries = Vec::with_capacity(selected_sequences.len());
    for sequence in &selected_sequences {
        let index = usize::try_from(*sequence).map_err(|_| ExportError::EmptySlice)?;
        let mut entry = decoded.get(index).cloned().ok_or(ExportError::EmptySlice)?;
        apply_retention(config, *sequence, current_sequence, &mut entry);
        entries.push(ExportedEntry {
            sequence: *sequence,
            entry,
        });
    }

    let referenced_evidence = collect_referenced_evidence(
        &entries,
        evidence_store,
        expected_settlement_domain,
    )?;

    let first_index = usize::try_from(first_sequence).map_err(|_| ExportError::EmptySlice)?;
    let links = chain_links(
        &frames,
        &decoded,
        &entries,
        &selected_sequences,
        first_index,
    );

    Ok(AuditExport {
        query,
        entries,
        chain: ChainMaterial {
            tenant_hash: verification.tenant_hash,
            anchor_entries: verification.entries,
            anchor_tail_hash: verification.tail_hash,
            links,
        },
        referenced_evidence,
    })
}

fn decode_owned_entries(
    frames: &[VerifiedFrame],
    config: &Config,
) -> Result<Vec<Entry>, ExportError> {
    let mut decoded = Vec::with_capacity(frames.len());
    for frame in frames {
        let entry = decode_entry(&frame.payload)?;
        if entry.tenant != config.tenant {
            return Err(ExportError::WrongTenant);
        }
        decoded.push(entry);
    }
    Ok(decoded)
}

fn collect_referenced_evidence(
    entries: &[ExportedEntry],
    evidence_store: &EvidenceStore,
    expected_settlement_domain: SettlementDomain,
) -> Result<Vec<ReferencedEvidence>, ExportError> {
    let receipt_ids: BTreeSet<_> = entries
        .iter()
        .filter_map(|exported| exported.entry.receipt_id)
        .collect();
    let mut referenced_evidence = Vec::with_capacity(receipt_ids.len());
    for receipt_id in receipt_ids {
        let protocol_facts = evidence_store
            .by_receipt
            .get(&receipt_id)
            .cloned()
            .ok_or(ExportError::EvidenceUnavailable { receipt_id })?;
        if !protocol_facts
            .receipts
            .iter()
            .any(|fact| fact.expected_receipt_digest == receipt_id)
        {
            return Err(ExportError::EvidenceMismatch { receipt_id });
        }
        verify_protocol_evidence(&protocol_facts, expected_settlement_domain)
            .map_err(|error| ExportError::EvidenceInvalid { receipt_id, error })?;
        referenced_evidence.push(ReferencedEvidence {
            receipt_id,
            protocol_facts,
        });
    }
    Ok(referenced_evidence)
}

fn chain_links(
    frames: &[VerifiedFrame],
    decoded: &[Entry],
    entries: &[ExportedEntry],
    selected_sequences: &BTreeSet<u64>,
    first_index: usize,
) -> Vec<ChainLink> {
    frames
        .iter()
        .enumerate()
        .skip(first_index)
        .map(|(index, frame)| {
            let sequence = index as u64;
            let selected = selected_sequences.contains(&sequence);
            let canonical_entry_bytes = if selected
                && entries
                    .iter()
                    .find(|entry| entry.sequence == sequence)
                    .is_some_and(|entry| entry.entry == decoded[index])
            {
                Some(frame.payload.clone())
            } else {
                None
            };
            ChainLink {
                sequence,
                previous_hash: frame.previous_hash,
                payload_digest: Sha256::digest(&frame.payload).into(),
                entry_hash: frame.entry_hash,
                selected,
                canonical_entry_bytes,
            }
        })
        .collect()
}

/// Verifies the exported hash-chain suffix against its durable tail anchor.
///
/// # Errors
///
/// Returns the first structural, payload, selection, or anchor mismatch.
pub fn verify_chain_material(material: &ChainMaterial) -> Result<(), ChainError> {
    let first = material.links.first().ok_or(ChainError::Empty)?;
    if !first.selected {
        return Err(ChainError::Selection);
    }
    for (index, link) in material.links.iter().enumerate() {
        if index > 0 {
            let previous = &material.links[index - 1];
            if link.sequence != previous.sequence.saturating_add(1) {
                return Err(ChainError::Sequence);
            }
            if link.previous_hash != previous.entry_hash {
                return Err(ChainError::Link);
            }
        }
        match &link.canonical_entry_bytes {
            Some(payload) => {
                if !link.selected {
                    return Err(ChainError::UnselectedPayload);
                }
                if <[u8; 32]>::from(Sha256::digest(payload)) != link.payload_digest {
                    return Err(ChainError::PayloadDigest);
                }
                let payload_length =
                    u32::try_from(payload.len()).map_err(|_| ChainError::EntryHash)?;
                if entry_hash(
                    material.tenant_hash,
                    link.sequence,
                    link.previous_hash,
                    payload_length,
                    payload,
                ) != link.entry_hash
                {
                    return Err(ChainError::EntryHash);
                }
            }
            None if !link.selected => {}
            None => {}
        }
    }
    let last = material.links.last().ok_or(ChainError::Empty)?;
    if last.sequence.saturating_add(1) != material.anchor_entries
        || last.entry_hash != material.anchor_tail_hash
    {
        return Err(ChainError::Anchor);
    }
    Ok(())
}

/// Independently reviews audit-chain integrity and every referenced protocol fact.
///
/// # Errors
///
/// Returns an error when the export has been altered, falls outside its query, or contains
/// protocol evidence that `layerx-proof` cannot verify.
pub fn review(
    exported: &AuditExport,
    expected_settlement_domain: SettlementDomain,
) -> Result<ReviewReport, ReviewError> {
    verify_chain_material(&exported.chain)?;
    let selected: Vec<_> = exported
        .chain
        .links
        .iter()
        .filter(|link| link.selected)
        .collect();
    if selected.len() != exported.entries.len() {
        return Err(ReviewError::EntryBinding);
    }
    for (entry, link) in exported.entries.iter().zip(selected) {
        if entry.sequence != link.sequence
            || entry.entry.tenant != exported.query.tenant
            || !exported.query.includes(&entry.entry)
        {
            return Err(ReviewError::Query);
        }
        match &link.canonical_entry_bytes {
            Some(payload) if decode_entry(payload).ok().as_ref() == Some(&entry.entry) => {}
            None if entry.entry.submitted_bytes == Some(PayloadEvidence::Redacted) => {}
            _ => return Err(ReviewError::EntryBinding),
        }
    }

    let expected_receipts: BTreeSet<_> = exported
        .entries
        .iter()
        .filter_map(|entry| entry.entry.receipt_id)
        .collect();
    let supplied_receipts: BTreeSet<_> = exported
        .referenced_evidence
        .iter()
        .map(|evidence| evidence.receipt_id)
        .collect();
    if expected_receipts != supplied_receipts
        || supplied_receipts.len() != exported.referenced_evidence.len()
    {
        return Err(ReviewError::EvidenceSet);
    }

    let mut report = ReviewReport {
        exported_entries: exported.entries.len(),
        verified_receipts: 0,
        verified_inclusions: 0,
        verified_checkpoints: 0,
        failed_records: exported
            .entries
            .iter()
            .filter(|entry| entry.entry.decision == super::Decision::Failed)
            .count(),
    };
    for evidence in &exported.referenced_evidence {
        if !evidence
            .protocol_facts
            .receipts
            .iter()
            .any(|fact| fact.expected_receipt_digest == evidence.receipt_id)
        {
            return Err(ReviewError::EvidenceSet);
        }
        let verified = verify_protocol_evidence(
            &evidence.protocol_facts,
            expected_settlement_domain,
        ).map_err(|error| {
            ReviewError::Evidence {
                receipt_id: evidence.receipt_id,
                error,
            }
        })?;
        report.verified_receipts = report
            .verified_receipts
            .saturating_add(verified.verified_receipts);
        report.verified_inclusions = report
            .verified_inclusions
            .saturating_add(verified.verified_inclusions);
        report.verified_checkpoints = report
            .verified_checkpoints
            .saturating_add(verified.verified_checkpoints);
    }
    Ok(report)
}

fn apply_retention(config: &Config, sequence: u64, current_sequence: u64, entry: &mut Entry) {
    let retained = current_sequence <= sequence.saturating_add(config.retention.audit);
    if entry.submitted_bytes.is_some()
        && (config.redaction == RedactionPolicy::ReceiptOnly || !retained)
    {
        entry.submitted_bytes = Some(PayloadEvidence::Redacted);
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
