//! The append-only hash-chained audit log of the interoperability gateway,
//! mirroring the human plane's audit enforcement. Every consequential gateway
//! action - adapter registration, translation begin, receipt-verified
//! completion and refusal - is appended with its trace identifier, each entry
//! linking to its predecessor, so truncation, reordering or alteration is
//! refused rather than silently served.

use std::fmt::{Display, Formatter};

use layerx_proof::merkle::leaf_hash;

use crate::codec::{self, CodecError, Decoder};
use crate::trace::TraceId;

const ENTRY_MAGIC: &[u8; 4] = b"LXGA";
const ENTRY_VERSION: u8 = 1;
const GENESIS_DOMAIN: &[u8; 4] = b"LXGG";
const LINK_DOMAIN: &[u8; 4] = b"LXGL";

/// The consequential gateway actions the audit log records.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum AuditEventKind {
    AdapterRegistered,
    TranslationBegun,
    TranslationCompleted,
    TranslationRefused,
}

impl AuditEventKind {
    /// Returns the event's emission label.
    #[must_use]
    pub const fn label(self) -> &'static str {
        match self {
            Self::AdapterRegistered => "adapter-registered",
            Self::TranslationBegun => "translation-begun",
            Self::TranslationCompleted => "translation-completed",
            Self::TranslationRefused => "translation-refused",
        }
    }

    const fn code(self) -> u8 {
        match self {
            Self::AdapterRegistered => 1,
            Self::TranslationBegun => 2,
            Self::TranslationCompleted => 3,
            Self::TranslationRefused => 4,
        }
    }

    fn from_code(value: u8) -> Result<Self, AuditError> {
        match value {
            1 => Ok(Self::AdapterRegistered),
            2 => Ok(Self::TranslationBegun),
            3 => Ok(Self::TranslationCompleted),
            4 => Ok(Self::TranslationRefused),
            _ => Err(AuditError::Corrupt("unknown audit event kind")),
        }
    }
}

/// One verified entry of an audit chain. The subject digest binds the entry
/// to its evidence: the pinned specification digest for registrations, the
/// request digest for begun or refused translations, and the verified receipt
/// digest for completions.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ChainEntry {
    sequence: u64,
    recorded_at: u64,
    trace: TraceId,
    kind: AuditEventKind,
    adapter: String,
    subject: [u8; 32],
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

    /// Returns the trace the recorded action travelled under.
    #[must_use]
    pub const fn trace(&self) -> &TraceId {
        &self.trace
    }

    /// Returns the recorded event kind.
    #[must_use]
    pub const fn kind(&self) -> AuditEventKind {
        self.kind
    }

    /// Returns the adapter the recorded action concerned.
    #[must_use]
    pub fn adapter(&self) -> &str {
        &self.adapter
    }

    /// Returns the evidence digest the entry binds.
    #[must_use]
    pub const fn subject(&self) -> [u8; 32] {
        self.subject
    }

    /// Returns the predecessor link.
    #[must_use]
    pub const fn prev_link(&self) -> [u8; 32] {
        self.prev_link
    }

    /// Returns this entry's chain link.
    #[must_use]
    pub const fn link(&self) -> [u8; 32] {
        self.link
    }

    /// Returns the entry's canonical bytes for export.
    #[must_use]
    pub fn bytes(&self) -> &[u8] {
        &self.bytes
    }
}

/// The chain head - entry count and tip link - recordable out of band so tail
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
}

fn genesis_link(name: &str) -> Result<[u8; 32], AuditError> {
    let mut bytes = GENESIS_DOMAIN.to_vec();
    bytes.extend_from_slice(name.as_bytes());
    leaf_hash(&bytes).map_err(|_| AuditError::Unhashable)
}

fn entry_link(entry_bytes: &[u8]) -> Result<[u8; 32], AuditError> {
    let mut bytes = LINK_DOMAIN.to_vec();
    bytes.extend_from_slice(entry_bytes);
    leaf_hash(&bytes).map_err(|_| AuditError::Unhashable)
}

fn encode_entry(
    sequence: u64,
    recorded_at: u64,
    trace: &TraceId,
    kind: AuditEventKind,
    adapter: &str,
    subject: [u8; 32],
    prev_link: [u8; 32],
) -> Result<Vec<u8>, AuditError> {
    let mut bytes = Vec::new();
    bytes.extend_from_slice(ENTRY_MAGIC);
    bytes.push(ENTRY_VERSION);
    bytes.extend_from_slice(&sequence.to_be_bytes());
    bytes.extend_from_slice(&recorded_at.to_be_bytes());
    codec::push_bytes(&mut bytes, trace.as_str().as_bytes())?;
    bytes.push(kind.code());
    codec::push_bytes(&mut bytes, adapter.as_bytes())?;
    bytes.extend_from_slice(&subject);
    bytes.extend_from_slice(&prev_link);
    Ok(bytes)
}

fn decode_entry(bytes: &[u8]) -> Result<ChainEntry, AuditError> {
    let mut reader = Decoder::new(bytes);
    if reader.take(4)? != ENTRY_MAGIC {
        return Err(AuditError::Corrupt("invalid audit entry header"));
    }
    if reader.byte()? != ENTRY_VERSION {
        return Err(AuditError::Corrupt("unknown audit entry version"));
    }
    let sequence = reader.u64()?;
    let recorded_at = reader.u64()?;
    let trace = TraceId::parse(reader.text()?)
        .map_err(|_| AuditError::Corrupt("malformed trace identifier"))?;
    let kind = AuditEventKind::from_code(reader.byte()?)?;
    let adapter = reader.text()?.to_owned();
    let subject = reader.array()?;
    let prev_link = reader.array()?;
    if !reader.is_empty() {
        return Err(AuditError::Corrupt("trailing bytes"));
    }
    let link = entry_link(bytes)?;
    Ok(ChainEntry {
        sequence,
        recorded_at,
        trace,
        kind,
        adapter,
        subject,
        prev_link,
        link,
        bytes: bytes.to_vec(),
    })
}

/// One append-only hash-chained audit log, genesis-linked to the scope that
/// owns it so chains of different scopes can never be spliced together.
#[derive(Debug)]
pub struct AuditChain {
    name: String,
    entries: Vec<ChainEntry>,
}

impl AuditChain {
    /// Creates an empty chain owned by the named scope.
    #[must_use]
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            entries: Vec::new(),
        }
    }

    /// Returns the owning scope name.
    #[must_use]
    pub fn name(&self) -> &str {
        &self.name
    }

    /// Appends one entry stamped with the trace the action travelled under.
    /// The log is append-only; no removal or rewrite operation exists.
    ///
    /// # Errors
    ///
    /// Returns encoding and hashing failures without appending.
    pub fn append(
        &mut self,
        recorded_at: u64,
        trace: &TraceId,
        kind: AuditEventKind,
        adapter: &str,
        subject: [u8; 32],
    ) -> Result<&ChainEntry, AuditError> {
        let sequence = u64::try_from(self.entries.len())
            .ok()
            .and_then(|length| length.checked_add(1))
            .ok_or(AuditError::SequenceOverflow)?;
        let prev_link = match self.entries.last() {
            Some(entry) => entry.link,
            None => genesis_link(&self.name)?,
        };
        let bytes = encode_entry(
            sequence,
            recorded_at,
            trace,
            kind,
            adapter,
            subject,
            prev_link,
        )?;
        let link = entry_link(&bytes)?;
        self.entries.push(ChainEntry {
            sequence,
            recorded_at,
            trace: trace.clone(),
            kind,
            adapter: adapter.to_owned(),
            subject,
            prev_link,
            link,
            bytes,
        });
        self.entries
            .last()
            .ok_or(AuditError::Corrupt("append lost its entry"))
    }

    /// Returns the chained entries in append order.
    #[must_use]
    pub fn entries(&self) -> &[ChainEntry] {
        &self.entries
    }

    /// Returns the current head for out-of-band recording.
    ///
    /// # Errors
    ///
    /// Returns a hashing failure for the empty chain's genesis link.
    pub fn head(&self) -> Result<ChainHead, AuditError> {
        let link = match self.entries.last() {
            Some(entry) => entry.link,
            None => genesis_link(&self.name)?,
        };
        let length = u64::try_from(self.entries.len()).map_err(|_| AuditError::SequenceOverflow)?;
        Ok(ChainHead { length, link })
    }

    /// Exports every entry's canonical bytes in append order.
    #[must_use]
    pub fn export(&self) -> Vec<Vec<u8>> {
        self.entries
            .iter()
            .map(|entry| entry.bytes.clone())
            .collect()
    }
}

/// Verifies an exported chain against its owning scope name and the head
/// recorded out of band, refusing truncation, reordering and alteration.
///
/// # Errors
///
/// Returns the exact defect: a broken link, a sequence gap, a malformed
/// entry, or a head mismatch.
pub fn verify_export(
    name: &str,
    exported: &[Vec<u8>],
    head: &ChainHead,
) -> Result<Vec<ChainEntry>, AuditError> {
    let mut prev_link = genesis_link(name)?;
    let mut entries = Vec::with_capacity(exported.len());
    for (index, bytes) in exported.iter().enumerate() {
        let entry = decode_entry(bytes)?;
        let expected_sequence = u64::try_from(index)
            .ok()
            .and_then(|value| value.checked_add(1))
            .ok_or(AuditError::SequenceOverflow)?;
        if entry.sequence != expected_sequence {
            return Err(AuditError::Reordered);
        }
        if entry.prev_link != prev_link {
            return Err(AuditError::BrokenLink);
        }
        prev_link = entry.link;
        entries.push(entry);
    }
    let length = u64::try_from(exported.len()).map_err(|_| AuditError::SequenceOverflow)?;
    if head.length != length || head.link != prev_link {
        return Err(AuditError::Truncated);
    }
    Ok(entries)
}

/// Audit chain failures.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum AuditError {
    Corrupt(&'static str),
    Unhashable,
    SequenceOverflow,
    BrokenLink,
    Reordered,
    Truncated,
}

impl Display for AuditError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Corrupt(reason) => write!(formatter, "corrupt audit entry: {reason}"),
            Self::Unhashable => formatter.write_str("audit entry cannot be digested"),
            Self::SequenceOverflow => formatter.write_str("audit chain exceeds sequence bounds"),
            Self::BrokenLink => formatter.write_str("audit chain link does not match predecessor"),
            Self::Reordered => formatter.write_str("audit chain entries are out of sequence"),
            Self::Truncated => formatter.write_str("audit chain does not reach its recorded head"),
        }
    }
}

impl std::error::Error for AuditError {}

impl From<CodecError> for AuditError {
    fn from(value: CodecError) -> Self {
        match value {
            CodecError::Truncated => Self::Corrupt("truncated audit entry"),
            CodecError::Overflow => Self::Corrupt("audit entry exceeds encoding bounds"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{verify_export, AuditChain, AuditError, AuditEventKind};
    use crate::trace::TraceId;

    fn chain_with_entries() -> AuditChain {
        let mut chain = AuditChain::new("alice");
        let trace = TraceId::mint([1; 16]);
        for (index, kind) in [
            AuditEventKind::AdapterRegistered,
            AuditEventKind::TranslationBegun,
            AuditEventKind::TranslationCompleted,
        ]
        .into_iter()
        .enumerate()
        {
            let subject = [u8::try_from(index).unwrap_or(0).saturating_add(1); 32];
            let recorded_at = 10_u64.saturating_add(u64::try_from(index).unwrap_or(0));
            chain
                .append(recorded_at, &trace, kind, "x402", subject)
                .unwrap_or_else(|error| panic!("append {index}: {error}"));
        }
        chain
    }

    #[test]
    fn an_intact_export_verifies_and_stamps_every_entry_with_its_trace() {
        let chain = chain_with_entries();
        let head = chain.head().unwrap_or_else(|error| panic!("head: {error}"));
        let entries = verify_export("alice", &chain.export(), &head)
            .unwrap_or_else(|error| panic!("verify: {error}"));
        assert_eq!(entries.len(), 3);
        for entry in &entries {
            assert_eq!(entry.trace(), &TraceId::mint([1; 16]));
            assert_eq!(entry.adapter(), "x402");
        }
    }

    #[test]
    fn truncation_reordering_and_alteration_are_refused() {
        let chain = chain_with_entries();
        let head = chain.head().unwrap_or_else(|error| panic!("head: {error}"));
        let export = chain.export();

        let mut truncated = export.clone();
        truncated.pop();
        assert_eq!(
            verify_export("alice", &truncated, &head),
            Err(AuditError::Truncated)
        );

        let mut reordered = export.clone();
        reordered.swap(0, 1);
        assert_eq!(
            verify_export("alice", &reordered, &head),
            Err(AuditError::Reordered)
        );

        let mut altered = export.clone();
        let position = altered[1].len() - 1;
        altered[1][position] ^= 0x01;
        assert_eq!(
            verify_export("alice", &altered, &head),
            Err(AuditError::BrokenLink)
        );

        assert_eq!(
            verify_export("mallory", &export, &head),
            Err(AuditError::BrokenLink),
            "a chain must not splice into another scope"
        );
    }
}
