use std::collections::{BTreeMap, BTreeSet};
use std::fmt::{Display, Formatter, Write as _};

use layerx_proof::checkpoint::SettlementDomain;
use layerx_proof::export::{
    verify as verify_offline, ExportVerificationError, OfflineExport, ReceiptFact,
    VerificationReport,
};
use layerx_proof::merkle::encode_proof;
use layerx_proof::receipt::AuthorizedBatch;
use layerx_wire::hash::receipt_digest as protocol_receipt_digest;
use layerx_wire::receipt::{decode as decode_receipt, encode_unsigned as encode_unsigned_receipt};
use sha2::{Digest as _, Sha256};

use crate::audit::{verify_export as verify_audit, AuditChain, AuditError};
use crate::notify::ActivityEntryId;
use crate::store::{PrincipalId, PrincipalScope};

use super::{
    ActivityEntry, ActivityKind, AppliedFilters, Feed, FeedCursor, FeedError, PageRequest,
};

const PAGE_SIZE: usize = 100;
const MAXIMUM_EXPORT_BYTES: usize = 64 * 1024 * 1024;
const PRINCIPAL_DOMAIN: &[u8] = b"layerx-human-export-principal/v1";
const CSV_HEADER: &str =
    "entry_id,kind,status,occurred_at,projected_at,verification,receipt_references\r\n";
const BUNDLE_MAGIC: &[u8; 8] = b"LXHEXP02";

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
    settlement_domain: SettlementDomain,
    entries: Vec<EvidenceEntry>,
    protocol_evidence: Vec<OfflineExport>,
    audit_export: Option<Vec<u8>>,
    bounded_bytes: usize,
}

impl EvidenceBundle {
    /// Encodes the receipt evidence bundle in a bounded canonical binary form.
    pub fn encode(&self) -> Result<Vec<u8>, ExportError> {
        let mut out = Vec::new();
        out.extend_from_slice(BUNDLE_MAGIC);
        out.extend_from_slice(&self.principal_binding);
        out.extend_from_slice(&self.settlement_domain.paxeer_chain_id().to_be_bytes());
        out.extend_from_slice(&self.settlement_domain.settlement_contract());
        push_u32(&mut out, self.entries.len())?;
        for entry in &self.entries {
            push_bytes(&mut out, entry.entry_id.as_str().as_bytes())?;
            push_u32(&mut out, entry.receipt_references.len())?;
            for reference in &entry.receipt_references {
                push_bytes(&mut out, reference.as_bytes())?;
            }
        }
        push_u32(&mut out, self.protocol_evidence.len())?;
        for export in &self.protocol_evidence {
            if !export.inclusions.is_empty()
                || !export.checkpoints.is_empty()
                || !export.derived_aggregates.is_empty()
            {
                return Err(ExportError::UnboundProtocolEvidence);
            }
            push_u32(&mut out, export.receipts.len())?;
            for receipt in &export.receipts {
                push_bytes(&mut out, receipt.statement.as_bytes())?;
                push_bytes(&mut out, &receipt.canonical_receipt_bytes)?;
                out.extend_from_slice(&receipt.authorised_batch.batch_id());
                out.extend_from_slice(&receipt.authorised_batch.asset());
                out.extend_from_slice(&receipt.authorised_batch.previous_state_root());
                out.extend_from_slice(&receipt.authorised_batch.resulting_state_root());
                out.extend_from_slice(&receipt.authorised_batch.sequencer_public_key());
                out.extend_from_slice(&receipt.expected_receipt_digest);
            }
        }
        match &self.audit_export {
            Some(value) => {
                out.push(1);
                push_bytes(&mut out, value)?
            }
            None => out.push(0),
        }
        require_bound(out.len(), MAXIMUM_EXPORT_BYTES)?;
        Ok(out)
    }

    /// Decodes the canonical bounded receipt bundle without trusting lengths.
    pub fn decode(bytes: &[u8]) -> Result<Self, ExportError> {
        require_bound(bytes.len(), MAXIMUM_EXPORT_BYTES)?;
        let mut reader = BundleReader { bytes, offset: 0 };
        if reader.take(8)? != BUNDLE_MAGIC {
            return Err(ExportError::UnboundProtocolEvidence);
        }
        let principal_binding = reader.array()?;
        let settlement_domain =
            SettlementDomain::new(u64::from_be_bytes(reader.array()?), reader.array()?);
        let entry_count = reader.count()?;
        let mut entries = Vec::with_capacity(entry_count);
        for _ in 0..entry_count {
            let id = String::from_utf8(reader.bytes()?.to_vec())
                .map_err(|_| ExportError::UnboundProtocolEvidence)?;
            let entry_id = ActivityEntryId::new(id).map_err(FeedError::from)?;
            let count = reader.count()?;
            let mut refs = Vec::with_capacity(count);
            for _ in 0..count {
                refs.push(
                    String::from_utf8(reader.bytes()?.to_vec())
                        .map_err(|_| ExportError::UnboundProtocolEvidence)?,
                )
            }
            entries.push(EvidenceEntry {
                entry_id,
                receipt_references: refs,
            });
        }
        let export_count = reader.count()?;
        let mut protocol_evidence = Vec::with_capacity(export_count);
        for _ in 0..export_count {
            let count = reader.count()?;
            let mut receipts = Vec::with_capacity(count);
            for _ in 0..count {
                let statement = String::from_utf8(reader.bytes()?.to_vec())
                    .map_err(|_| ExportError::UnboundProtocolEvidence)?;
                let canonical_receipt_bytes = reader.bytes()?.to_vec();
                let authorised_batch = AuthorizedBatch::new(
                    reader.array()?,
                    reader.array()?,
                    reader.array()?,
                    reader.array()?,
                    reader.array()?,
                );
                let expected_receipt_digest = reader.array()?;
                receipts.push(layerx_proof::export::ReceiptFact {
                    statement,
                    canonical_receipt_bytes,
                    authorised_batch,
                    expected_receipt_digest,
                });
            }
            protocol_evidence.push(OfflineExport {
                receipts,
                inclusions: Vec::new(),
                checkpoints: Vec::new(),
                derived_aggregates: Vec::new(),
            });
        }
        let audit_export = match reader.byte()? {
            0 => None,
            1 => Some(reader.bytes()?.to_vec()),
            _ => return Err(ExportError::UnboundProtocolEvidence),
        };
        if reader.offset != bytes.len() {
            return Err(ExportError::UnboundProtocolEvidence);
        }
        Ok(Self {
            principal_binding,
            settlement_domain,
            entries,
            protocol_evidence,
            audit_export,
            bounded_bytes: bytes.len(),
        })
    }
    #[must_use]
    pub const fn principal_binding(&self) -> [u8; 32] {
        self.principal_binding
    }

    #[must_use]
    pub const fn settlement_domain(&self) -> SettlementDomain {
        self.settlement_domain
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

    /// Content digest naming this bundle as evidence.
    ///
    /// # Errors
    ///
    /// Returns encoding and size-bound failures.
    pub fn digest(&self) -> Result<[u8; 32], ExportError> {
        Ok(Sha256::digest(self.encode()?).into())
    }

    /// Builds the offline receipt fact for one service-held receipt: the
    /// reference is the digest of the exact canonical bytes and the expected
    /// protocol digest is recomputed from those bytes, so the fact claims
    /// nothing the bytes do not carry.
    ///
    /// # Errors
    ///
    /// Returns `VerifyError::Unverified(Tampered)` when the bytes are not a
    /// canonical receipt.
    pub fn receipt_fact(
        entry_id: &ActivityEntryId,
        canonical_receipt: Vec<u8>,
        authorised_batch: AuthorizedBatch,
    ) -> Result<ReceiptFact, VerifyError> {
        let reference = receipt_reference(&canonical_receipt);
        let expected_receipt_digest = decode_receipt(&canonical_receipt)
            .ok()
            .and_then(|receipt| encode_unsigned_receipt(&receipt).ok())
            .and_then(|unsigned| protocol_receipt_digest(&unsigned).ok())
            .ok_or(VerifyError::Unverified(UnverifiedReason::Tampered))?;
        Ok(ReceiptFact {
            statement: format!("activity {} receipt {reference}", entry_id.as_str()),
            canonical_receipt_bytes: canonical_receipt,
            authorised_batch,
            expected_receipt_digest,
        })
    }

    /// Binds one service-held receipt to its activity entry so a single
    /// receipt row is verified through the same bundle verifier as an
    /// exported bundle.
    ///
    /// # Errors
    ///
    /// Returns `VerifyError::Unverified(Tampered)` for bytes that are not a
    /// canonical receipt and `VerifyError::Unavailable` on size overflows.
    pub fn receipt(
        entry_id: ActivityEntryId,
        canonical_receipt: Vec<u8>,
        authorised_batch: AuthorizedBatch,
        principal: &PrincipalId,
        settlement_domain: SettlementDomain,
    ) -> Result<Self, VerifyError> {
        let reference = receipt_reference(&canonical_receipt);
        let fact = Self::receipt_fact(&entry_id, canonical_receipt, authorised_batch)?;
        let entries = vec![EvidenceEntry {
            entry_id,
            receipt_references: vec![reference],
        }];
        let protocol_evidence = vec![OfflineExport {
            receipts: vec![fact],
            inclusions: Vec::new(),
            checkpoints: Vec::new(),
            derived_aggregates: Vec::new(),
        }];
        let bounded_bytes =
            evidence_size(&entries, &protocol_evidence, None).map_err(VerifyError::Unavailable)?;
        let bundle = Self {
            principal_binding: principal_binding(principal.as_str()),
            settlement_domain,
            entries,
            protocol_evidence,
            audit_export: None,
            bounded_bytes,
        };
        bundle.encode().map_err(VerifyError::Unavailable)?;
        Ok(bundle)
    }

    /// Loads the receipt authority the service holds for every activity entry
    /// this bundle binds.
    ///
    /// # Errors
    ///
    /// Returns feed read failures.
    pub fn receipt_authority(
        &self,
        feed: Feed,
        scope: &PrincipalScope<'_>,
    ) -> Result<ReceiptAuthority, FeedError> {
        let mut authority = ReceiptAuthority::default();
        for entry in &self.entries {
            if let Some(entry) = feed.entry(scope, &entry.entry_id)? {
                authority.extend_from_entry(&entry);
            }
        }
        Ok(authority)
    }

    /// Re-verifies the bundle against the evidence identity the caller
    /// expects: the digest the evidence identifier names, the requesting
    /// principal, the service settlement domain and the receipt authority the
    /// service loaded from the agent layer. The checks run in that order and
    /// the embedded protocol and audit evidence is re-verified independently
    /// last, so each refusal names the first binding that disagrees.
    ///
    /// # Errors
    ///
    /// Returns `VerifyError::Unverified` with the typed reason on a digest,
    /// principal, settlement-domain or authority mismatch and on altered or
    /// inconsistent protocol or audit evidence; returns
    /// `VerifyError::Unavailable` when a bound receipt has no loaded
    /// authority or the bundle cannot be re-encoded.
    pub fn verify(
        &self,
        expected_digest: [u8; 32],
        principal: &PrincipalId,
        expected_settlement_domain: SettlementDomain,
        receipt_authority: &ReceiptAuthority,
    ) -> Result<BundleReport, VerifyError> {
        if self.digest().map_err(VerifyError::Unavailable)? != expected_digest {
            return Err(VerifyError::Unverified(UnverifiedReason::DigestMismatch));
        }
        self.verify_bindings(principal, expected_settlement_domain, receipt_authority)
    }

    fn verify_bindings(
        &self,
        principal: &PrincipalId,
        expected_settlement_domain: SettlementDomain,
        receipt_authority: &ReceiptAuthority,
    ) -> Result<BundleReport, VerifyError> {
        if principal_binding(principal.as_str()) != self.principal_binding {
            return Err(VerifyError::Unverified(UnverifiedReason::PrincipalMismatch));
        }
        if self.settlement_domain != expected_settlement_domain {
            return Err(VerifyError::Unverified(
                UnverifiedReason::SettlementDomainMismatch,
            ));
        }
        let expected = referenced_receipts(&self.entries);
        for receipt in self
            .protocol_evidence
            .iter()
            .flat_map(|export| export.receipts.iter())
        {
            let reference = receipt_reference(&receipt.canonical_receipt_bytes);
            if !expected.contains(&reference) {
                return Err(VerifyError::Unverified(UnverifiedReason::Tampered));
            }
            let Some(loaded) = receipt_authority.get(&reference) else {
                return Err(VerifyError::Unavailable(
                    ExportError::AuthorityUnavailable { reference },
                ));
            };
            if *loaded != receipt.authorised_batch {
                return Err(VerifyError::Unverified(UnverifiedReason::AuthorityMismatch));
            }
        }
        let protocol = verify_protocol_set(
            &self.protocol_evidence,
            &expected,
            expected_settlement_domain,
        )
        .map_err(VerifyError::from)?;
        let audit = self
            .audit_export
            .as_deref()
            .map(verify_audit)
            .transpose()
            .map_err(|error| VerifyError::from(ExportError::Audit(error)))?;
        if let Some(report) = &audit {
            if principal_binding(report.principal().as_str()) != self.principal_binding {
                return Err(VerifyError::Unverified(UnverifiedReason::PrincipalMismatch));
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

    /// Verifies one service-held receipt row through the bundle verifier,
    /// binding it to its activity entry, the requesting principal, the
    /// service settlement domain and the authority the service loaded for it.
    ///
    /// # Errors
    ///
    /// Returns the same typed result as [`Self::verify`].
    pub fn verify_receipt(
        entry_id: ActivityEntryId,
        canonical_receipt: &[u8],
        authorised_batch: &AuthorizedBatch,
        expected_receipt_digest: [u8; 32],
        principal: &PrincipalId,
        settlement_domain: SettlementDomain,
    ) -> Result<BundleReport, VerifyError> {
        let reference = receipt_reference(canonical_receipt);
        if reference != hex(expected_receipt_digest) {
            return Err(VerifyError::Unverified(UnverifiedReason::DigestMismatch));
        }
        let bundle = Self::receipt(
            entry_id,
            canonical_receipt.to_vec(),
            *authorised_batch,
            principal,
            settlement_domain,
        )?;
        let digest = bundle.digest().map_err(VerifyError::Unavailable)?;
        let mut receipt_authority = ReceiptAuthority::default();
        receipt_authority.insert(reference, *authorised_batch);
        bundle.verify(digest, principal, settlement_domain, &receipt_authority)
    }
}

fn push_u32(out: &mut Vec<u8>, value: usize) -> Result<(), ExportError> {
    out.extend_from_slice(
        &u32::try_from(value)
            .map_err(|_| ExportError::SizeOverflow)?
            .to_be_bytes(),
    );
    Ok(())
}
fn push_bytes(out: &mut Vec<u8>, value: &[u8]) -> Result<(), ExportError> {
    if value.is_empty() {
        return Err(ExportError::UnboundProtocolEvidence);
    }
    push_u32(out, value.len())?;
    out.extend_from_slice(value);
    require_bound(out.len(), MAXIMUM_EXPORT_BYTES)
}
struct BundleReader<'a> {
    bytes: &'a [u8],
    offset: usize,
}
impl<'a> BundleReader<'a> {
    fn take(&mut self, n: usize) -> Result<&'a [u8], ExportError> {
        let end = self
            .offset
            .checked_add(n)
            .ok_or(ExportError::SizeOverflow)?;
        let value = self
            .bytes
            .get(self.offset..end)
            .ok_or(ExportError::UnboundProtocolEvidence)?;
        self.offset = end;
        Ok(value)
    }
    fn byte(&mut self) -> Result<u8, ExportError> {
        Ok(self.take(1)?[0])
    }
    fn array<const N: usize>(&mut self) -> Result<[u8; N], ExportError> {
        self.take(N)?
            .try_into()
            .map_err(|_| ExportError::UnboundProtocolEvidence)
    }
    fn count(&mut self) -> Result<usize, ExportError> {
        let value = u32::from_be_bytes(self.array()?);
        usize::try_from(value).map_err(|_| ExportError::SizeOverflow)
    }
    fn bytes(&mut self) -> Result<&'a [u8], ExportError> {
        let n = self.count()?;
        if n == 0 {
            return Err(ExportError::UnboundProtocolEvidence);
        }
        self.take(n)
    }
}

/// Counts reported by one successful verifier run. It is neither cloneable
/// nor copyable, so a verified result cannot be reused for other evidence.
#[derive(Debug, Eq, PartialEq)]
pub struct BundleReport {
    entries: usize,
    verified_receipts: usize,
    verified_inclusions: usize,
    verified_checkpoints: usize,
    audit_entries: usize,
}

impl BundleReport {
    #[must_use]
    pub const fn entries(&self) -> usize {
        self.entries
    }

    #[must_use]
    pub const fn verified_receipts(&self) -> usize {
        self.verified_receipts
    }

    #[must_use]
    pub const fn verified_inclusions(&self) -> usize {
        self.verified_inclusions
    }

    #[must_use]
    pub const fn verified_checkpoints(&self) -> usize {
        self.verified_checkpoints
    }

    #[must_use]
    pub const fn audit_entries(&self) -> usize {
        self.audit_entries
    }
}

/// Receipt authorities the service loaded from the agent layer, keyed by the
/// receipt reference each one authorises.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct ReceiptAuthority {
    batches: BTreeMap<String, AuthorizedBatch>,
}

impl ReceiptAuthority {
    /// Collects the stored authority of every receipt on the given entries.
    #[must_use]
    pub fn from_entries(entries: &[ActivityEntry]) -> Self {
        let mut authority = Self::default();
        for entry in entries {
            authority.extend_from_entry(entry);
        }
        authority
    }

    /// Adds the stored authority of every receipt on one entry.
    pub fn extend_from_entry(&mut self, entry: &ActivityEntry) {
        for receipt in entry.receipts() {
            if let Some(batch) = receipt.authority() {
                self.batches.insert(receipt.reference().to_owned(), *batch);
            }
        }
    }

    /// Records the authority the service loaded for one receipt reference.
    pub fn insert(&mut self, reference: String, batch: AuthorizedBatch) {
        self.batches.insert(reference, batch);
    }

    #[must_use]
    pub fn get(&self, reference: &str) -> Option<&AuthorizedBatch> {
        self.batches.get(reference)
    }
}

/// Typed reason the verifier refused to label evidence receipt-verified.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum UnverifiedReason {
    Tampered,
    DigestMismatch,
    PrincipalMismatch,
    SettlementDomainMismatch,
    AuthorityMismatch,
}

impl UnverifiedReason {
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Tampered => "tampered",
            Self::DigestMismatch => "digest-mismatch",
            Self::PrincipalMismatch => "principal-mismatch",
            Self::SettlementDomainMismatch => "settlement-domain-mismatch",
            Self::AuthorityMismatch => "authority-mismatch",
        }
    }
}

/// Typed outcome of a verifier run that did not verify.
#[derive(Debug)]
pub enum VerifyError {
    Unverified(UnverifiedReason),
    Unavailable(ExportError),
}

impl Display for VerifyError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Unverified(reason) => {
                write!(formatter, "evidence is unverified: {}", reason.as_str())
            }
            Self::Unavailable(error) => write!(formatter, "evidence verifier unavailable: {error}"),
        }
    }
}

impl std::error::Error for VerifyError {}

impl From<ExportError> for VerifyError {
    fn from(value: ExportError) -> Self {
        match value {
            ExportError::PrincipalMismatch => Self::Unverified(UnverifiedReason::PrincipalMismatch),
            ExportError::Protocol(_)
            | ExportError::Audit(_)
            | ExportError::EvidenceUnavailable { .. }
            | ExportError::UnexpectedEvidence { .. }
            | ExportError::DuplicateEvidence
            | ExportError::UnboundProtocolEvidence => Self::Unverified(UnverifiedReason::Tampered),
            ExportError::Unverified(reason) => Self::Unverified(reason),
            ExportError::Feed(_)
            | ExportError::InvalidSizeBound
            | ExportError::SizeBoundExceeded { .. }
            | ExportError::SizeOverflow
            | ExportError::DuplicateEntry
            | ExportError::EntryNotFound
            | ExportError::AuthorityUnavailable { .. } => Self::Unavailable(value),
        }
    }
}

/// Verification status of one evidence row as the service may label it.
///
/// The receipt-verified verdict exists only as the return path of
/// [`EvidenceBundle::verify`]: the verdict field is private and the sole
/// constructor, [`verification_status`], consumes that verifier's typed
/// result, whose success value cannot be built outside this module.
///
/// ```
/// use layerx_human_service::activity::{verification_status, UnverifiedReason, VerifyError};
///
/// let status = verification_status(Err(VerifyError::Unverified(UnverifiedReason::Tampered)));
/// assert_eq!(status.label(), "unverified");
/// assert_eq!(status.unverified_reason(), Some(UnverifiedReason::Tampered));
/// assert!(!status.is_receipt_verified());
/// ```
///
/// Outside code cannot name the verdict:
///
/// ```compile_fail
/// use layerx_human_service::activity::VerificationStatus;
///
/// let forged = VerificationStatus { verdict: () };
/// ```
///
/// Outside code cannot forge the verifier's success value either:
///
/// ```compile_fail
/// use layerx_human_service::activity::{verification_status, BundleReport};
///
/// let forged = verification_status(Ok(BundleReport {
///     entries: 0,
///     verified_receipts: 0,
///     verified_inclusions: 0,
///     verified_checkpoints: 0,
///     audit_entries: 0,
/// }));
/// ```
#[derive(Debug, Eq, PartialEq)]
pub struct VerificationStatus {
    verdict: Verdict,
}

#[derive(Debug, Eq, PartialEq)]
enum Verdict {
    ReceiptVerified(BundleReport),
    Unverified(UnverifiedReason),
    Unavailable,
}

impl VerificationStatus {
    /// Wire-level label of this status.
    #[must_use]
    pub const fn label(&self) -> &'static str {
        match self.verdict {
            Verdict::ReceiptVerified(_) => "receipt-verified",
            Verdict::Unverified(_) => "unverified",
            Verdict::Unavailable => "unavailable",
        }
    }

    #[must_use]
    pub const fn is_receipt_verified(&self) -> bool {
        matches!(self.verdict, Verdict::ReceiptVerified(_))
    }

    #[must_use]
    pub const fn is_unavailable(&self) -> bool {
        matches!(self.verdict, Verdict::Unavailable)
    }

    #[must_use]
    pub const fn unverified_reason(&self) -> Option<UnverifiedReason> {
        match &self.verdict {
            Verdict::Unverified(reason) => Some(*reason),
            Verdict::ReceiptVerified(_) | Verdict::Unavailable => None,
        }
    }

    #[must_use]
    pub const fn report(&self) -> Option<&BundleReport> {
        match &self.verdict {
            Verdict::ReceiptVerified(report) => Some(report),
            Verdict::Unverified(_) | Verdict::Unavailable => None,
        }
    }
}

/// The only constructor of a verification status: it consumes the verifier's
/// typed result and maps success to receipt-verified, a typed refusal to
/// unverified and a verifier or authority failure to unavailable.
#[must_use]
pub fn verification_status(result: Result<BundleReport, VerifyError>) -> VerificationStatus {
    let verdict = match result {
        Ok(report) => Verdict::ReceiptVerified(report),
        Err(VerifyError::Unverified(reason)) => Verdict::Unverified(reason),
        Err(VerifyError::Unavailable(_)) => Verdict::Unavailable,
    };
    VerificationStatus { verdict }
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
    /// evidence, bound to the principal, the settlement domain and the receipt
    /// authority the service loaded, together with the verifier report that
    /// bound it.
    ///
    /// # Errors
    ///
    /// Refuses missing entries, missing or extra receipt evidence, invalid
    /// proofs, an authority the service did not load, cross-scope selections
    /// and outputs over the configured bound.
    #[allow(clippy::too_many_arguments)]
    pub fn evidence(
        self,
        scope: &PrincipalScope<'_>,
        filters: &AppliedFilters,
        entry_ids: &[ActivityEntryId],
        protocol_evidence: Vec<OfflineExport>,
        expected_settlement_domain: SettlementDomain,
        receipt_authority: &ReceiptAuthority,
        now: u64,
        observed_agent_head: u64,
    ) -> Result<(EvidenceBundle, BundleReport), ExportError> {
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
        verify_protocol_set(&protocol_evidence, &expected, expected_settlement_domain)?;
        let bounded_bytes = evidence_size(&entries, &protocol_evidence, None)?;
        require_bound(bounded_bytes, self.maximum_bytes)?;
        let bundle = EvidenceBundle {
            principal_binding: principal_binding(scope.principal().as_str()),
            settlement_domain: expected_settlement_domain,
            entries,
            protocol_evidence,
            audit_export: None,
            bounded_bytes,
        };
        let report = bundle.verify_bindings(
            scope.principal(),
            expected_settlement_domain,
            receipt_authority,
        )?;
        Ok((bundle, report))
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
            settlement_domain: expected_settlement_domain,
            entries: Vec::new(),
            protocol_evidence: Vec::new(),
            audit_export: Some(audit_export),
            bounded_bytes,
        };
        let digest = bundle.digest()?;
        bundle.verify(
            digest,
            scope.principal(),
            expected_settlement_domain,
            &ReceiptAuthority::default(),
        )?;
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

fn receipt_reference(canonical_receipt: &[u8]) -> String {
    hex(Sha256::digest(canonical_receipt))
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
        let report =
            verify_offline(artifact, expected_settlement_domain).map_err(ExportError::Protocol)?;
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
    let mut size = 96_usize;
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
    AuthorityUnavailable { reference: String },
    Unverified(UnverifiedReason),
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
            Self::AuthorityUnavailable { reference } => {
                write!(
                    formatter,
                    "activity evidence {reference} has no loaded receipt authority"
                )
            }
            Self::Unverified(reason) => {
                write!(
                    formatter,
                    "activity evidence is unverified: {}",
                    reason.as_str()
                )
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

impl From<VerifyError> for ExportError {
    fn from(value: VerifyError) -> Self {
        match value {
            VerifyError::Unverified(reason) => Self::Unverified(reason),
            VerifyError::Unavailable(error) => error,
        }
    }
}
