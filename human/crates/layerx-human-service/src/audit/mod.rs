//! The append-only hash-chained audit log of the human plane. Every
//! consequential action - authentication, signing decisions, approvals,
//! journey transitions, security changes and notification dispatches - is
//! appended as exportable evidence of record on top of the principal store's
//! audit namespace, each entry linking to its predecessor and binding the
//! content of every row it references, so truncation, reordering or
//! alteration is refused rather than silently served.

mod event;
mod export;
mod wire;

pub use event::{
    ApprovalOutcome, AuditEvent, AuthMethod, Decision, IdentityEvent, JourneyKind, JourneyState,
    NotificationChannel, NotificationClass, SecurityChangeKind, SigningOperation, StepUpEvidence,
};
pub use export::{verify_export, ExportReport};

use std::fmt::{Display, Formatter};

use layerx_proof::merkle::leaf_hash;

use crate::redaction::RedactionError;
use crate::store::{
    AuditDisposition, EvidenceRef, PrincipalId, PrincipalScope, RowKey, StoreError, Table,
};
use crate::trace::{TraceError, TraceId};

use wire::{push_bytes, push_length, Reader};

const ENTRY_MAGIC: &[u8; 4] = b"LXAE";
const ENTRY_VERSION: u8 = 1;
const HEAD_MAGIC: &[u8; 4] = b"LXAH";
const GENESIS_DOMAIN: &[u8; 4] = b"LXAG";
const CHAIN_PREFIX: &str = "chain-";
const SEQUENCE_DIGITS: usize = 16;

const fn table_code(table: Table) -> u8 {
    match table {
        Table::Journeys => 1,
        Table::Notifications => 2,
        Table::Support => 6,
        Table::Telemetry => 4,
        Table::Cache => 5,
        Table::Stream => 7,
    }
}

fn table_from_code(value: u8) -> Result<Table, AuditError> {
    match value {
        1 => Ok(Table::Journeys),
        2 => Ok(Table::Notifications),
        6 => Ok(Table::Support),
        4 => Ok(Table::Telemetry),
        5 => Ok(Table::Cache),
        7 => Ok(Table::Stream),
        _ => Err(AuditError::Corrupt("unknown evidence table code")),
    }
}

fn chain_key(sequence: u64) -> Result<RowKey, AuditError> {
    Ok(RowKey::new(format!("{CHAIN_PREFIX}{sequence:016x}"))?)
}

fn parse_sequence(digits: &str) -> Option<u64> {
    if digits.len() != SEQUENCE_DIGITS
        || !digits
            .bytes()
            .all(|byte| byte.is_ascii_digit() || matches!(byte, b'a'..=b'f'))
    {
        return None;
    }
    u64::from_str_radix(digits, 16).ok()
}

fn genesis_link(principal: &PrincipalId) -> Result<[u8; 32], AuditError> {
    let mut bytes = GENESIS_DOMAIN.to_vec();
    bytes.extend_from_slice(principal.as_str().as_bytes());
    leaf_hash(&bytes).map_err(|_| AuditError::Unhashable)
}

fn evidence_digest(written_at: u64, bytes: &[u8]) -> Result<[u8; 32], AuditError> {
    let mut buffer = Vec::with_capacity(8_usize.saturating_add(bytes.len()));
    buffer.extend_from_slice(&written_at.to_be_bytes());
    buffer.extend_from_slice(bytes);
    leaf_hash(&buffer).map_err(|_| AuditError::Unhashable)
}

/// One evidence reference with the content digest the chain entry binds, so
/// altering a referenced row after the fact is detectable.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct BoundEvidence {
    table: Table,
    key: RowKey,
    digest: [u8; 32],
}

impl BoundEvidence {
    /// Returns the referenced table.
    #[must_use]
    pub const fn table(&self) -> Table {
        self.table
    }

    /// Returns the referenced key.
    #[must_use]
    pub const fn key(&self) -> &RowKey {
        &self.key
    }

    /// Returns the bound content digest.
    #[must_use]
    pub const fn digest(&self) -> [u8; 32] {
        self.digest
    }
}

/// The chain head - entry count and tip link - recorded out of band so tail
/// truncation of an otherwise-consistent chain is detectable.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ChainHead {
    length: u64,
    link: [u8; 32],
}

impl ChainHead {
    /// Returns the number of chained entries.
    #[must_use]
    pub const fn length(&self) -> u64 {
        self.length
    }

    /// Returns the tip link.
    #[must_use]
    pub const fn link(&self) -> [u8; 32] {
        self.link
    }

    /// Encodes the head for out-of-band recording.
    #[must_use]
    pub fn encode(&self) -> Vec<u8> {
        let mut bytes = Vec::with_capacity(44);
        bytes.extend_from_slice(HEAD_MAGIC);
        bytes.extend_from_slice(&self.length.to_be_bytes());
        bytes.extend_from_slice(&self.link);
        bytes
    }

    /// Reconstructs a head recorded out of band.
    ///
    /// # Errors
    ///
    /// Refuses bytes that are not exactly one encoded head.
    pub fn decode(bytes: &[u8]) -> Result<Self, AuditError> {
        let mut reader = Reader::new(bytes);
        if reader.take(4)? != HEAD_MAGIC {
            return Err(AuditError::Corrupt("invalid chain head header"));
        }
        let length = reader.u64()?;
        let link = reader.array()?;
        if !reader.is_empty() {
            return Err(AuditError::Corrupt("trailing bytes"));
        }
        Ok(Self { length, link })
    }
}

/// One verified entry of the audit chain.
#[derive(Clone, Debug)]
pub struct ChainEntry {
    sequence: u64,
    recorded_at: u64,
    trace: TraceId,
    event: AuditEvent,
    evidence: Vec<BoundEvidence>,
    prev_link: [u8; 32],
    link: [u8; 32],
    bytes: Vec<u8>,
}

impl ChainEntry {
    /// Returns the entry's position in the chain.
    #[must_use]
    pub const fn sequence(&self) -> u64 {
        self.sequence
    }

    /// Returns the caller-injected append timestamp.
    #[must_use]
    pub const fn recorded_at(&self) -> u64 {
        self.recorded_at
    }

    /// Returns the trace the audited action ran under.
    #[must_use]
    pub const fn trace(&self) -> &TraceId {
        &self.trace
    }

    /// Returns the audited event.
    #[must_use]
    pub const fn event(&self) -> &AuditEvent {
        &self.event
    }

    /// Returns the evidence the entry binds.
    #[must_use]
    pub fn evidence(&self) -> &[BoundEvidence] {
        &self.evidence
    }

    /// Returns the predecessor link the entry commits to.
    #[must_use]
    pub const fn prev_link(&self) -> [u8; 32] {
        self.prev_link
    }

    /// Returns the entry's own link.
    #[must_use]
    pub const fn link(&self) -> [u8; 32] {
        self.link
    }

    /// Returns the entry's canonical bytes, the exact preimage of its link.
    #[must_use]
    pub fn bytes(&self) -> &[u8] {
        &self.bytes
    }
}

struct EntryBody {
    sequence: u64,
    recorded_at: u64,
    prev_link: [u8; 32],
    trace: TraceId,
    event: AuditEvent,
    evidence: Vec<BoundEvidence>,
}

fn encode_entry(
    sequence: u64,
    recorded_at: u64,
    prev_link: &[u8; 32],
    trace: &TraceId,
    event: &AuditEvent,
    evidence: &[BoundEvidence],
) -> Result<Vec<u8>, AuditError> {
    let mut output = Vec::new();
    output.extend_from_slice(ENTRY_MAGIC);
    output.push(ENTRY_VERSION);
    output.extend_from_slice(&sequence.to_be_bytes());
    output.extend_from_slice(&recorded_at.to_be_bytes());
    output.extend_from_slice(prev_link);
    push_bytes(&mut output, trace.as_str().as_bytes())?;
    event.encode(&mut output)?;
    push_length(&mut output, evidence.len())?;
    for binding in evidence {
        output.push(table_code(binding.table));
        push_bytes(&mut output, binding.key.as_str().as_bytes())?;
        output.extend_from_slice(&binding.digest);
    }
    Ok(output)
}

fn decode_entry(bytes: &[u8]) -> Result<EntryBody, AuditError> {
    let mut reader = Reader::new(bytes);
    if reader.take(4)? != ENTRY_MAGIC {
        return Err(AuditError::Corrupt("invalid audit entry header"));
    }
    if reader.byte()? != ENTRY_VERSION {
        return Err(AuditError::Corrupt("unknown audit entry version"));
    }
    let sequence = reader.u64()?;
    let recorded_at = reader.u64()?;
    let prev_link = reader.array()?;
    let trace_text = std::str::from_utf8(reader.bytes()?)
        .map_err(|_| AuditError::Corrupt("trace is not UTF-8"))?;
    let trace = TraceId::parse(trace_text)?;
    let event = AuditEvent::decode(&mut reader)?;
    let count = reader.length()?;
    let mut evidence = Vec::new();
    for _ in 0..count {
        let table = table_from_code(reader.byte()?)?;
        let key_text = std::str::from_utf8(reader.bytes()?)
            .map_err(|_| AuditError::Corrupt("evidence key is not UTF-8"))?;
        let key = RowKey::new(key_text).map_err(|_| AuditError::Corrupt("invalid evidence key"))?;
        let digest = reader.array()?;
        evidence.push(BoundEvidence { table, key, digest });
    }
    if !reader.is_empty() {
        return Err(AuditError::Corrupt("trailing bytes"));
    }
    Ok(EntryBody {
        sequence,
        recorded_at,
        prev_link,
        trace,
        event,
        evidence,
    })
}

fn verify_bindings(
    scope: &PrincipalScope<'_>,
    sequence: u64,
    bindings: &[BoundEvidence],
    references: &[EvidenceRef],
) -> Result<(), AuditError> {
    if bindings.len() != references.len() {
        return Err(AuditError::EvidenceMismatch { sequence });
    }
    for (binding, reference) in bindings.iter().zip(references) {
        if reference.table() != binding.table || reference.key() != &binding.key {
            return Err(AuditError::EvidenceMismatch { sequence });
        }
        let row = scope
            .get(binding.table, &binding.key)
            .ok_or(AuditError::Corrupt("dangling evidence reference"))?;
        if evidence_digest(row.written_at(), row.bytes())? != binding.digest {
            return Err(AuditError::EvidenceDigestMismatch { sequence });
        }
    }
    Ok(())
}

fn load(scope: &PrincipalScope<'_>) -> Result<(Vec<ChainEntry>, ChainHead), AuditError> {
    let mut link = genesis_link(scope.principal())?;
    let mut entries = Vec::new();
    let mut expected: u64 = 0;
    for key in scope.audit_keys() {
        let Some(digits) = key.as_str().strip_prefix(CHAIN_PREFIX) else {
            continue;
        };
        let sequence = parse_sequence(digits).ok_or(AuditError::ForeignChainKey)?;
        if sequence != expected {
            return Err(AuditError::MissingEntry { sequence: expected });
        }
        let entry = scope
            .audit(&key)
            .ok_or(AuditError::Corrupt("audit entry unreadable"))?;
        let AuditDisposition::Exportable { evidence } = entry.disposition() else {
            return Err(AuditError::DispositionMismatch { sequence });
        };
        let body = decode_entry(entry.bytes())?;
        if body.sequence != sequence {
            return Err(AuditError::SequenceMismatch { sequence });
        }
        if body.recorded_at != entry.written_at() {
            return Err(AuditError::TimestampMismatch { sequence });
        }
        if body.prev_link != link {
            return Err(AuditError::LinkMismatch { sequence });
        }
        verify_bindings(scope, sequence, &body.evidence, evidence)?;
        link = leaf_hash(entry.bytes()).map_err(|_| AuditError::Unhashable)?;
        entries.push(ChainEntry {
            sequence,
            recorded_at: body.recorded_at,
            trace: body.trace,
            event: body.event,
            evidence: body.evidence,
            prev_link: body.prev_link,
            link,
            bytes: entry.bytes().to_vec(),
        });
        expected = expected.checked_add(1).ok_or(AuditError::SizeOverflow)?;
    }
    Ok((
        entries,
        ChainHead {
            length: expected,
            link,
        },
    ))
}

/// The verified head state of one principal's audit chain. Opening fully
/// verifies the stored chain; every read re-verifies it; appends extend it
/// as exportable evidence of record that the store never expires.
#[derive(Debug)]
pub struct AuditChain {
    next_sequence: u64,
    head_link: [u8; 32],
}

impl AuditChain {
    /// Opens the principal's chain, verifying every entry, link, timestamp,
    /// disposition and evidence binding, and refusing a truncated, reordered
    /// or altered chain.
    ///
    /// # Errors
    ///
    /// Returns the typed refusal naming the first violated entry.
    pub fn open(scope: &PrincipalScope<'_>) -> Result<Self, AuditError> {
        let (_, head) = load(scope)?;
        Ok(Self {
            next_sequence: head.length,
            head_link: head.link,
        })
    }

    /// Opens and verifies the principal's chain against the durable head an
    /// operator recorded outside the principal store. Startup uses this path
    /// after the first anchor exists, making removal of a valid tail entry a
    /// typed refusal instead of a silently shorter history.
    ///
    /// # Errors
    ///
    /// Returns the first chain violation, or [`AuditError::HeadMismatch`] when
    /// the in-store chain no longer reaches the expected durable head.
    pub fn open_anchored(
        scope: &PrincipalScope<'_>,
        expected: ChainHead,
    ) -> Result<Self, AuditError> {
        let chain = Self::open(scope)?;
        chain.verify_head(expected)?;
        Ok(chain)
    }

    /// Returns the chain head to record out of band as the anchor against
    /// tail truncation.
    #[must_use]
    pub const fn head(&self) -> ChainHead {
        ChainHead {
            length: self.next_sequence,
            link: self.head_link,
        }
    }

    /// Compares the verified chain against a head recorded out of band,
    /// catching tail truncation that in-store verification cannot see.
    ///
    /// # Errors
    ///
    /// Returns [`AuditError::HeadMismatch`] when the heads differ.
    pub fn verify_head(&self, expected: ChainHead) -> Result<(), AuditError> {
        if self.head() == expected {
            Ok(())
        } else {
            Err(AuditError::HeadMismatch {
                expected,
                found: self.head(),
            })
        }
    }

    /// Appends one audited event, binding the current content of every
    /// referenced row and linking the entry to the chain tip. The entry is
    /// stored exportable, so it never expires and pins its evidence.
    ///
    /// # Errors
    ///
    /// Returns evidence resolution failures, hashing failures and store
    /// persistence failures with nothing appended.
    pub fn append(
        &mut self,
        scope: &mut PrincipalScope<'_>,
        now: u64,
        trace: &TraceId,
        event: &AuditEvent,
        evidence: &[EvidenceRef],
    ) -> Result<ChainHead, AuditError> {
        let mut bound = Vec::with_capacity(evidence.len());
        for reference in evidence {
            let row = scope
                .get(reference.table(), reference.key())
                .ok_or(AuditError::Store(StoreError::MissingEvidence))?;
            let digest = evidence_digest(row.written_at(), row.bytes())?;
            bound.push(BoundEvidence {
                table: reference.table(),
                key: reference.key().clone(),
                digest,
            });
        }
        let bytes = encode_entry(
            self.next_sequence,
            now,
            &self.head_link,
            trace,
            event,
            &bound,
        )?;
        let link = leaf_hash(&bytes).map_err(|_| AuditError::Unhashable)?;
        let key = chain_key(self.next_sequence)?;
        scope.append_audit(
            key,
            now,
            bytes,
            AuditDisposition::Exportable {
                evidence: evidence.to_vec(),
            },
        )?;
        self.next_sequence = self
            .next_sequence
            .checked_add(1)
            .ok_or(AuditError::SizeOverflow)?;
        self.head_link = link;
        Ok(self.head())
    }

    /// Reads the chain, re-verifying every link on the read path and
    /// refusing a store that no longer matches this verified head.
    ///
    /// # Errors
    ///
    /// Returns the typed refusal naming the first violated entry.
    pub fn entries(&self, scope: &PrincipalScope<'_>) -> Result<Vec<ChainEntry>, AuditError> {
        let (entries, head) = load(scope)?;
        if head != self.head() {
            return Err(AuditError::HeadMismatch {
                expected: self.head(),
                found: head,
            });
        }
        Ok(entries)
    }

    /// Exports the chain with every referenced evidence row as one bundle
    /// verifiable by [`verify_export`] independently of this plane.
    ///
    /// # Errors
    ///
    /// Returns verification and encoding failures.
    pub fn export(&self, scope: &PrincipalScope<'_>) -> Result<Vec<u8>, AuditError> {
        export::build(scope, self)
    }
}

/// Audit chain failures.
#[derive(Debug)]
pub enum AuditError {
    Store(StoreError),
    Trace(TraceError),
    Redaction(RedactionError),
    Unhashable,
    ForeignChainKey,
    MissingEntry {
        sequence: u64,
    },
    SequenceMismatch {
        sequence: u64,
    },
    TimestampMismatch {
        sequence: u64,
    },
    LinkMismatch {
        sequence: u64,
    },
    DispositionMismatch {
        sequence: u64,
    },
    EvidenceMismatch {
        sequence: u64,
    },
    EvidenceDigestMismatch {
        sequence: u64,
    },
    HeadMismatch {
        expected: ChainHead,
        found: ChainHead,
    },
    Corrupt(&'static str),
    SizeOverflow,
}

impl Display for AuditError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Store(error) => write!(formatter, "audit store failure: {error}"),
            Self::Trace(error) => write!(formatter, "audit trace failure: {error}"),
            Self::Redaction(error) => write!(formatter, "audit redaction failure: {error}"),
            Self::Unhashable => formatter.write_str("audit bytes cannot be hashed"),
            Self::ForeignChainKey => {
                formatter.write_str("foreign key inside the audit chain namespace")
            }
            Self::MissingEntry { sequence } => {
                write!(formatter, "audit chain entry {sequence} is missing")
            }
            Self::SequenceMismatch { sequence } => write!(
                formatter,
                "audit chain entry at position {sequence} carries another sequence"
            ),
            Self::TimestampMismatch { sequence } => write!(
                formatter,
                "audit chain entry {sequence} timestamp does not match its stored write time"
            ),
            Self::LinkMismatch { sequence } => {
                write!(formatter, "audit chain link broken at entry {sequence}")
            }
            Self::DispositionMismatch { sequence } => write!(
                formatter,
                "audit chain entry {sequence} is not exportable evidence"
            ),
            Self::EvidenceMismatch { sequence } => write!(
                formatter,
                "audit chain entry {sequence} evidence references do not match its bindings"
            ),
            Self::EvidenceDigestMismatch { sequence } => write!(
                formatter,
                "evidence content bound by audit chain entry {sequence} was altered"
            ),
            Self::HeadMismatch { expected, found } => write!(
                formatter,
                "audit chain head does not match its anchor: expected {} entries, found {}",
                expected.length, found.length
            ),
            Self::Corrupt(reason) => write!(formatter, "corrupt audit chain: {reason}"),
            Self::SizeOverflow => formatter.write_str("audit entry exceeds encoding bounds"),
        }
    }
}

impl std::error::Error for AuditError {}

impl From<StoreError> for AuditError {
    fn from(value: StoreError) -> Self {
        Self::Store(value)
    }
}

impl From<TraceError> for AuditError {
    fn from(value: TraceError) -> Self {
        Self::Trace(value)
    }
}

impl From<RedactionError> for AuditError {
    fn from(value: RedactionError) -> Self {
        Self::Redaction(value)
    }
}
