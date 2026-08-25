use std::collections::BTreeSet;
use std::fmt::{Display, Formatter, Write as _};

use layerx_proof::export::{
    verify as verify_offline, ExportVerificationError, OfflineExport, VerificationReport,
};
use layerx_proof::checkpoint::SettlementDomain;
use layerx_proof::merkle::encode_proof;
use sha2::{Digest as _, Sha256};

use crate::audit::{verify_export as verify_audit, AuditChain, AuditError};
use crate::notify::ActivityEntryId;
use crate::store::PrincipalScope;

use super::{
    ActivityEntry, ActivityKind, AppliedFilters, Feed, FeedCursor, FeedError, PageRequest,
};

const PAGE_SIZE: usize = 100;
const MAXIMUM_EXPORT_BYTES: usize = 64 * 1024 * 1024;
const PRINCIPAL_DOMAIN: &[u8] = b"layerx-human-export-principal/v1";
const CSV_HEADER: &str =
    "entry_id,kind,status,occurred_at,projected_at,verification,receipt_references\r\n";

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct StatementExport {
    content: Vec<u8>,
    entries: usize,
}

impl StatementExport {
    #[must_use]
    pub fn content(&self) -> &[u8] {
        &self.content
    }

    #[must_use]
    pub const fn content_type() -> &'static str {
        "text/csv; charset=utf-8"
    }

    #[must_use]
    pub const fn entries(&self) -> usize {
        self.entries
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct EvidenceEntry {
    entry_id: ActivityEntryId,
    receipt_references: Vec<String>,
}

impl EvidenceEntry {
    #[must_use]
    pub const fn entry_id(&self) -> &ActivityEntryId {
        &self.entry_id
    }

    #[must_use]
    pub fn receipt_references(&self) -> &[String] {
        &self.receipt_references
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct EvidenceBundle {
    principal_binding: [u8; 32],
    entries: Vec<EvidenceEntry>,
    protocol_evidence: Vec<OfflineExport>,
    audit_export: Option<Vec<u8>>,
    bounded_bytes: usize,
}

impl EvidenceBundle {
    #[must_use]
    pub const fn principal_binding(&self) -> [u8; 32] {
        self.principal_binding
    }

    #[must_use]
    pub fn entries(&self) -> &[EvidenceEntry] {
        &self.entries
    }

    #[must_use]
    pub fn protocol_evidence(&self) -> &[OfflineExport] {
        &self.protocol_evidence
    }

    #[must_use]
    pub fn audit_export(&self) -> Option<&[u8]> {
        self.audit_export.as_deref()
    }

    #[must_use]
    pub const fn bounded_bytes(&self) -> usize {
        self.bounded_bytes
    }

    /// Re-verifies the bundle without access to a human-service store.
    ///
    /// # Errors
    ///
    /// Refuses altered protocol evidence, changed activity-to-receipt bindings,
    /// or an invalid audit export.
    pub fn verify(
        &self,
        expected_settlement_domain: SettlementDomain,
    ) -> Result<BundleReport, ExportError> {
        let expected = referenced_receipts(&self.entries);
        let protocol = verify_protocol_set(
            &self.protocol_evidence,
            &expected,
            expected_settlement_domain,
        )?;
        let audit = self.audit_export.as_deref().map(verify_audit).transpose()?;
        if let Some(report) = &audit {
            if principal_binding(report.principal().as_str()) != self.principal_binding {
                return Err(ExportError::PrincipalMismatch);
            }
        }
        Ok(BundleReport {
            entries: self.entries.len(),
            verified_receipts: protocol.iter().map(|report| report.verified_receipts).sum(),
            verified_inclusions: protocol
                .iter()
                .map(|report| report.verified_inclusions)
                .sum(),
            verified_checkpoints: protocol
                .iter()
                .map(|report| report.verified_checkpoints)
                .sum(),
            audit_entries: audit.map_or(0, |report| report.entries()),
        })
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct BundleReport {
    entries: usize,
    verified_receipts: usize,
    verified_inclusions: usize,
    verified_checkpoints: usize,
    audit_entries: usize,
}

impl BundleReport {
    #[must_use]
    pub const fn entries(self) -> usize {
        self.entries
    }

    #[must_use]
    pub const fn verified_receipts(self) -> usize {
        self.verified_receipts
    }

    #[must_use]
    pub const fn verified_inclusions(self) -> usize {
        self.verified_inclusions
    }

    #[must_use]
    pub const fn verified_checkpoints(self) -> usize {
        self.verified_checkpoints
    }

    #[must_use]
    pub const fn audit_entries(self) -> usize {
        self.audit_entries
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct EvidenceExport {
    feed: Feed,
    maximum_bytes: usize,
}

impl EvidenceExport {
    /// Creates an exporter with a finite output bound.
    ///
    /// # Errors
    ///
    /// Refuses zero and unreasonably large bounds.
    pub const fn new(feed: Feed, maximum_bytes: usize) -> Result<Self, ExportError> {
        if maximum_bytes == 0 || maximum_bytes > MAXIMUM_EXPORT_BYTES {
            Err(ExportError::InvalidSizeBound)
        } else {
            Ok(Self {
                feed,
                maximum_bytes,
            })
        }
    }

    /// Exports one CSV statement from the same applied filters as the feed.
    ///
    /// # Errors
    ///
    /// Returns feed, encoding and size-bound failures.
    pub fn statement(
        self,
        scope: &PrincipalScope<'_>,
        filters: &AppliedFilters,
        now: u64,
        observed_agent_head: u64,
    ) -> Result<StatementExport, ExportError> {
        let entries = self.select(scope, filters, &[], now, observed_agent_head)?;
        let mut content = Vec::with_capacity(CSV_HEADER.len());
        append_bounded(&mut content, CSV_HEADER.as_bytes(), self.maximum_bytes)?;
        for entry in &entries {
            let row = statement_row(entry);
            append_bounded(&mut content, row.as_bytes(), self.maximum_bytes)?;
        }
        Ok(StatementExport {
            content,
            entries: entries.len(),
        })
    }

    /// Exports selected activity and its exact independently verified protocol
    /// evidence.
    ///
    /// # Errors
    ///
    /// Refuses missing entries, missing or extra receipt evidence, invalid
    /// proofs, cross-scope selections and outputs over the configured bound.
    pub fn evidence(
        self,
        scope: &PrincipalScope<'_>,
        filters: &AppliedFilters,
        entry_ids: &[ActivityEntryId],
        protocol_evidence: Vec<OfflineExport>,
        expected_settlement_domain: SettlementDomain,
        now: u64,
        observed_agent_head: u64,
    ) -> Result<EvidenceBundle, ExportError> {
        let selected = self.select(scope, filters, entry_ids, now, observed_agent_head)?;
        let entries = selected
            .iter()
            .map(|entry| EvidenceEntry {
                entry_id: entry.entry_id().clone(),
                receipt_references: entry
                    .receipts()
                    .iter()
                    .map(|receipt| receipt.reference().to_owned())
                    .collect(),
            })
            .collect::<Vec<_>>();
        let expected = referenced_receipts(&entries);
        verify_protocol_set(
            &protocol_evidence,
            &expected,
            expected_settlement_domain,
        )?;
        let bounded_bytes = evidence_size(&entries, &protocol_evidence, None)?;
        require_bound(bounded_bytes, self.maximum_bytes)?;
        let bundle = EvidenceBundle {
            principal_binding: principal_binding(scope.principal().as_str()),
            entries,
            protocol_evidence,
            audit_export: None,
            bounded_bytes,
        };
        bundle.verify(expected_settlement_domain)?;
        Ok(bundle)
    }

    /// Exports the principal's audit chain through the same independently
    /// verifiable bundle surface.
    ///
    /// # Errors
    ///
    /// Returns audit-chain verification, encoding and size-bound failures.
    pub fn audit(
        self,
        scope: &PrincipalScope<'_>,
        audit: &AuditChain,
        expected_settlement_domain: SettlementDomain,
    ) -> Result<EvidenceBundle, ExportError> {
        let audit_export = audit.export(scope)?;
        verify_audit(&audit_export)?;
        let bounded_bytes = evidence_size(&[], &[], Some(&audit_export))?;
        require_bound(bounded_bytes, self.maximum_bytes)?;
        let bundle = EvidenceBundle {
            principal_binding: principal_binding(scope.principal().as_str()),
            entries: Vec::new(),
            protocol_evidence: Vec::new(),
            audit_export: Some(audit_export),
            bounded_bytes,
        };
        bundle.verify(expected_settlement_domain)?;
        Ok(bundle)
    }

    fn select(
        self,
        scope: &PrincipalScope<'_>,
        filters: &AppliedFilters,
        entry_ids: &[ActivityEntryId],
        now: u64,
        observed_agent_head: u64,
    ) -> Result<Vec<ActivityEntry>, ExportError> {
        let requested = entry_ids
            .iter()
            .map(|id| id.as_str().to_owned())
            .collect::<BTreeSet<_>>();
        if requested.len() != entry_ids.len() {
            return Err(ExportError::DuplicateEntry);
        }
        let mut cursor: Option<FeedCursor> = None;
        let mut selected = Vec::new();
        loop {
            let mut request = PageRequest::new(PAGE_SIZE, filters.clone());
            if let Some(next) = cursor {
                request = request.after(next);
            }
            let page = self.feed.page(scope, request, now, observed_agent_head)?;
            selected.extend(
                page.entries()
                    .iter()
                    .filter(|entry| {
                        requested.is_empty() || requested.contains(entry.entry_id().as_str())
                    })
                    .cloned(),
            );
            let Some(next) = page.next().cloned() else {
                break;
            };
            cursor = Some(next);
        }
        if !requested.is_empty()
            && selected
                .iter()
                .map(|entry| entry.entry_id().as_str())
                .collect::<BTreeSet<_>>()
                .len()
                != requested.len()
        {
            return Err(ExportError::EntryNotFound);
        }
        Ok(selected)
    }
}

fn statement_row(entry: &ActivityEntry) -> String {
    let verification = entry
        .receipts()
        .iter()
        .map(|receipt| format!("{:?}", receipt.level()))
        .collect::<Vec<_>>()
        .join(";");
    let receipts = entry
        .receipts()
        .iter()
        .map(super::ReceiptEvidence::reference)
        .collect::<Vec<_>>()
        .join(";");
    [
        entry.entry_id().as_str().to_owned(),
        activity_kind(entry.kind()).to_owned(),
        entry.status().label().to_owned(),
        entry.occurred_at().to_string(),
        entry.projected_at().to_string(),
        verification,
        receipts,
    ]
    .iter()
    .map(|field| csv_field(field))
    .collect::<Vec<_>>()
    .join(",")
        + "\r\n"
}

const fn activity_kind(kind: ActivityKind) -> &'static str {
    match kind {
        ActivityKind::Deposit => "deposit",
        ActivityKind::Withdrawal => "withdrawal",
        ActivityKind::Movement => "movement",
        ActivityKind::AgentAction => "agent-action",
        ActivityKind::Approval => "approval",
        ActivityKind::Security => "security-event",
    }
}

fn csv_field(value: &str) -> String {
    let mut safe = value.to_owned();
    if safe
        .as_bytes()
        .first()
        .is_some_and(|byte| matches!(byte, b'=' | b'+' | b'-' | b'@' | b'\t' | b'\r'))
    {
        safe.insert(0, '\'');
    }
    if safe.contains([',', '"', '\r', '\n']) {
        format!("\"{}\"", safe.replace('"', "\"\""))
    } else {
        safe
    }
}

fn referenced_receipts(entries: &[EvidenceEntry]) -> BTreeSet<String> {
    entries
        .iter()
        .flat_map(|entry| entry.receipt_references.iter().cloned())
        .collect()
}

fn verify_protocol_set(
    protocol: &[OfflineExport],
    expected: &BTreeSet<String>,
    expected_settlement_domain: SettlementDomain,
) -> Result<Vec<VerificationReport>, ExportError> {
    let mut provided = BTreeSet::new();
    let mut reports = Vec::with_capacity(protocol.len());
    for artifact in protocol {
        if artifact.receipts.is_empty() {
            return Err(ExportError::UnboundProtocolEvidence);
        }
        let report = verify_offline(artifact, expected_settlement_domain)
            .map_err(ExportError::Protocol)?;
        for receipt in &artifact.receipts {
            let reference = hex(Sha256::digest(&receipt.canonical_receipt_bytes));
            if !provided.insert(reference.clone()) {
                return Err(ExportError::DuplicateEvidence);
            }
            if !expected.contains(&reference) {
                return Err(ExportError::UnexpectedEvidence { reference });
            }
        }
        reports.push(report);
    }
    if let Some(reference) = expected.difference(&provided).next() {
        return Err(ExportError::EvidenceUnavailable {
            reference: reference.clone(),
        });
    }
    Ok(reports)
}

fn evidence_size(
    entries: &[EvidenceEntry],
    protocol: &[OfflineExport],
    audit: Option<&[u8]>,
) -> Result<usize, ExportError> {
    let mut size = 64_usize;
    for entry in entries {
        size = checked_add(size, entry.entry_id.as_str().len())?;
        for reference in &entry.receipt_references {
            size = checked_add(size, reference.len())?;
        }
    }
    for artifact in protocol {
        for receipt in &artifact.receipts {
            size = checked_add(size, receipt.statement.len())?;
            size = checked_add(size, receipt.canonical_receipt_bytes.len())?;
            size = checked_add(size, 192)?;
        }
        for inclusion in &artifact.inclusions {
            size = checked_add(size, inclusion.statement.len())?;
            size = checked_add(size, inclusion.canonical_leaf_bytes.len())?;
            size = checked_add(size, encode_proof(&inclusion.proof).len())?;
            size = checked_add(size, inclusion.canonical_header_bytes.len())?;
            size = checked_add(size, 256)?;
        }
        for checkpoint in &artifact.checkpoints {
            size = checked_add(size, checkpoint.statement.len())?;
            size = checked_add(
                size,
                checkpoint.certificate.checkpoint().header_bytes().len(),
            )?;
            size = checked_add(
                size,
                checkpoint.certificate.checkpoint().validity_proof().len(),
            )?;
            size = checked_add(
                size,
                checkpoint
                    .certificate
                    .attestations()
                    .len()
                    .saturating_mul(256),
            )?;
            size = checked_add(size, checkpoint.bonded_set.len().saturating_mul(96))?;
            size = checked_add(
                size,
                checkpoint
                    .registered_settlement_reference
                    .as_ref()
                    .map_or(0, Vec::len),
            )?;
            size = checked_add(size, 128)?;
        }
        for aggregate in &artifact.derived_aggregates {
            size = checked_add(size, aggregate.label.len())?;
            size = checked_add(size, aggregate.rendered_value.len())?;
            size = checked_add(
                size,
                aggregate
                    .contributing_receipt_digests
                    .len()
                    .saturating_mul(32),
            )?;
        }
    }
    checked_add(size, audit.map_or(0, <[u8]>::len))
}

fn append_bounded(output: &mut Vec<u8>, bytes: &[u8], maximum: usize) -> Result<(), ExportError> {
    let next = checked_add(output.len(), bytes.len())?;
    require_bound(next, maximum)?;
    output.extend_from_slice(bytes);
    Ok(())
}

const fn require_bound(actual: usize, maximum: usize) -> Result<(), ExportError> {
    if actual > maximum {
        Err(ExportError::SizeBoundExceeded { maximum })
    } else {
        Ok(())
    }
}

const fn checked_add(left: usize, right: usize) -> Result<usize, ExportError> {
    match left.checked_add(right) {
        Some(value) => Ok(value),
        None => Err(ExportError::SizeOverflow),
    }
}

fn principal_binding(principal: &str) -> [u8; 32] {
    let mut digest = Sha256::new();
    digest.update(PRINCIPAL_DOMAIN);
    digest.update(principal.as_bytes());
    digest.finalize().into()
}

fn hex(bytes: impl AsRef<[u8]>) -> String {
    let bytes = bytes.as_ref();
    let mut output = String::with_capacity(bytes.len().saturating_mul(2));
    for byte in bytes {
        let _ = write!(output, "{byte:02x}");
    }
    output
}

#[derive(Debug)]
pub enum ExportError {
    Feed(FeedError),
    Audit(AuditError),
    Protocol(ExportVerificationError),
    InvalidSizeBound,
    SizeBoundExceeded { maximum: usize },
    SizeOverflow,
    DuplicateEntry,
    EntryNotFound,
    EvidenceUnavailable { reference: String },
    UnexpectedEvidence { reference: String },
    DuplicateEvidence,
    UnboundProtocolEvidence,
    PrincipalMismatch,
}

impl Display for ExportError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Feed(error) => write!(formatter, "activity export feed failure: {error}"),
            Self::Audit(error) => write!(formatter, "activity export audit failure: {error}"),
            Self::Protocol(error) => {
                write!(
                    formatter,
                    "activity export protocol evidence failed: {error:?}"
                )
            }
            Self::InvalidSizeBound => formatter.write_str("activity export size bound is invalid"),
            Self::SizeBoundExceeded { maximum } => {
                write!(
                    formatter,
                    "activity export exceeds its {maximum}-byte bound"
                )
            }
            Self::SizeOverflow => formatter.write_str("activity export size overflowed"),
            Self::DuplicateEntry => formatter.write_str("activity export repeats an entry"),
            Self::EntryNotFound => {
                formatter.write_str("activity export entry is outside the principal or filter")
            }
            Self::EvidenceUnavailable { reference } => {
                write!(formatter, "activity evidence {reference} is unavailable")
            }
            Self::UnexpectedEvidence { reference } => {
                write!(formatter, "activity evidence {reference} was not requested")
            }
            Self::DuplicateEvidence => formatter.write_str("activity evidence is duplicated"),
            Self::UnboundProtocolEvidence => {
                formatter.write_str("protocol evidence has no selected receipt binding")
            }
            Self::PrincipalMismatch => {
                formatter.write_str("audit export principal binding does not match")
            }
        }
    }
}

impl std::error::Error for ExportError {}

impl From<FeedError> for ExportError {
    fn from(value: FeedError) -> Self {
        Self::Feed(value)
    }
}

impl From<AuditError> for ExportError {
    fn from(value: AuditError) -> Self {
        Self::Audit(value)
    }
}
