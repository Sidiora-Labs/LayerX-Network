//! Principal-scoped durable storage for the human control plane.

mod codec;
mod migrate;
mod retention;
mod tenancy;

pub use migrate::MigrationError;
pub use retention::{ExpiryReport, RetentionPeriod, RetentionPolicy};
pub use tenancy::{AgentTenantId, TenancyDigest, TenancyError, TenancyMap};

use std::collections::{BTreeMap, BTreeSet};
use std::fmt::{Display, Formatter};
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use codec::{CodecError, Decoder};

pub(crate) const STORE_MAGIC: &[u8; 4] = b"LXHP";
pub(crate) const PRINCIPALS_DIR: &str = "principals";
pub(crate) const STORE_FILE: &str = "store.bin";
const TEMP_FILE: &str = "store.bin.tmp";
const IDENTIFIER_LIMIT: usize = 128;

fn valid_identifier(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= IDENTIFIER_LIMIT
        && value.bytes().all(|byte| {
            byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'-' || byte == b'_'
        })
}

/// A validated human principal identifier, the only handle that reaches
/// stored rows and the sole source of one on-disk subtree name.
#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct PrincipalId(String);

impl PrincipalId {
    /// Creates a bounded identifier limited to `a-z`, `0-9`, `-` and `_` so no
    /// identifier can traverse or alias another principal's subtree.
    ///
    /// # Errors
    ///
    /// Rejects empty, oversize and out-of-charset identifiers.
    pub fn new(value: impl Into<String>) -> Result<Self, StoreError> {
        let value = value.into();
        if valid_identifier(&value) {
            Ok(Self(value))
        } else {
            Err(StoreError::InvalidPrincipal)
        }
    }

    /// Returns the identifier.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// A validated row key sharing the principal identifier charset, so no key
/// can escape its principal's namespace.
#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct RowKey(String);

impl RowKey {
    /// Creates a bounded key limited to `a-z`, `0-9`, `-` and `_`.
    ///
    /// # Errors
    ///
    /// Rejects empty, oversize and out-of-charset keys.
    pub fn new(value: impl Into<String>) -> Result<Self, StoreError> {
        let value = value.into();
        if valid_identifier(&value) {
            Ok(Self(value))
        } else {
            Err(StoreError::InvalidKey)
        }
    }

    /// Returns the key.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// Every non-audit namespace the store persists per principal.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub enum Table {
    Journeys,
    Notifications,
    Support,
    Telemetry,
    Cache,
    Stream,
}

impl Table {
    const fn code(self) -> u8 {
        match self {
            Self::Journeys => 1,
            Self::Notifications => 2,
            Self::Support => 6,
            Self::Telemetry => 4,
            Self::Cache => 5,
            Self::Stream => 7,
        }
    }

    fn from_code(value: u8) -> Result<Self, StoreError> {
        match value {
            1 => Ok(Self::Journeys),
            2 => Ok(Self::Notifications),
            6 => Ok(Self::Support),
            4 => Ok(Self::Telemetry),
            5 => Ok(Self::Cache),
            7 => Ok(Self::Stream),
            _ => Err(StoreError::Corrupt("unknown table code")),
        }
    }
}

/// A same-scope reference from an exportable audit entry to one stored row.
#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub struct EvidenceRef {
    table: Table,
    key: RowKey,
}

impl EvidenceRef {
    /// References one row of the scope the audit entry is appended to.
    #[must_use]
    pub const fn new(table: Table, key: RowKey) -> Self {
        Self { table, key }
    }

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
}

/// Whether an audit entry is exportable evidence of record or internal
/// bookkeeping.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum AuditDisposition {
    /// Expires under the audit retention period and pins no evidence.
    Internal,
    /// Never expires and pins every referenced row against deletion.
    Exportable {
        /// Rows this entry references as its evidence.
        evidence: Vec<EvidenceRef>,
    },
}

/// One stored row with its injected write timestamp.
#[derive(Clone, Copy, Debug)]
pub struct StoredRow<'a> {
    written_at: u64,
    bytes: &'a [u8],
}

impl<'a> StoredRow<'a> {
    /// Returns the caller-injected write timestamp.
    #[must_use]
    pub const fn written_at(&self) -> u64 {
        self.written_at
    }

    /// Returns the stored bytes.
    #[must_use]
    pub const fn bytes(&self) -> &'a [u8] {
        self.bytes
    }
}

/// One audit entry with its disposition.
#[derive(Clone, Copy, Debug)]
pub struct AuditEntry<'a> {
    written_at: u64,
    bytes: &'a [u8],
    disposition: &'a AuditDisposition,
}

impl<'a> AuditEntry<'a> {
    /// Returns the caller-injected write timestamp.
    #[must_use]
    pub const fn written_at(&self) -> u64 {
        self.written_at
    }

    /// Returns the stored bytes.
    #[must_use]
    pub const fn bytes(&self) -> &'a [u8] {
        self.bytes
    }

    /// Returns the entry's disposition.
    #[must_use]
    pub const fn disposition(&self) -> &'a AuditDisposition {
        self.disposition
    }

    /// Returns whether the entry is exportable evidence of record.
    #[must_use]
    pub const fn is_exportable(&self) -> bool {
        matches!(self.disposition, AuditDisposition::Exportable { .. })
    }
}

/// Errors from the principal store.
#[derive(Debug)]
pub enum StoreError {
    Io(io::Error),
    Migration(MigrationError),
    Tenancy(TenancyError),
    InvalidPrincipal,
    InvalidKey,
    InvalidTenant,
    Corrupt(&'static str),
    SizeOverflow,
    DuplicateAuditEntry,
    MissingEvidence,
    EvidencePinned,
}

impl Display for StoreError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Io(error) => write!(formatter, "store I/O failure: {error}"),
            Self::Migration(error) => write!(formatter, "store migration failure: {error}"),
            Self::Tenancy(error) => write!(formatter, "tenancy configuration failure: {error}"),
            Self::InvalidPrincipal => formatter.write_str("invalid principal identifier"),
            Self::InvalidKey => formatter.write_str("invalid row key"),
            Self::InvalidTenant => formatter.write_str("invalid agent tenant identifier"),
            Self::Corrupt(reason) => write!(formatter, "corrupt store: {reason}"),
            Self::SizeOverflow => formatter.write_str("store value exceeds encoding bounds"),
            Self::DuplicateAuditEntry => formatter.write_str("audit entries are append-only"),
            Self::MissingEvidence => formatter
                .write_str("audit evidence reference does not resolve in this principal scope"),
            Self::EvidencePinned => {
                formatter.write_str("row is pinned as evidence by an exportable audit entry")
            }
        }
    }
}

impl std::error::Error for StoreError {}

impl From<io::Error> for StoreError {
    fn from(value: io::Error) -> Self {
        Self::Io(value)
    }
}

impl From<MigrationError> for StoreError {
    fn from(value: MigrationError) -> Self {
        Self::Migration(value)
    }
}

impl From<TenancyError> for StoreError {
    fn from(value: TenancyError) -> Self {
        Self::Tenancy(value)
    }
}

impl From<CodecError> for StoreError {
    fn from(value: CodecError) -> Self {
        match value {
            CodecError::Truncated => Self::Corrupt("truncated store bytes"),
            CodecError::Overflow => Self::SizeOverflow,
        }
    }
}

#[derive(Clone, Debug)]
struct Row {
    written_at: u64,
    bytes: Vec<u8>,
}

#[derive(Clone, Debug)]
struct AuditRow {
    written_at: u64,
    bytes: Vec<u8>,
    disposition: AuditDisposition,
}

#[derive(Clone, Debug, Default)]
struct PrincipalState {
    tables: BTreeMap<Table, BTreeMap<RowKey, Row>>,
    audit: BTreeMap<RowKey, AuditRow>,
}

impl PrincipalState {
    fn contains(&self, table: Table, key: &RowKey) -> bool {
        self.tables
            .get(&table)
            .is_some_and(|rows| rows.contains_key(key))
    }

    fn pinned(&self, table: Table, key: &RowKey) -> bool {
        self.audit.values().any(|row| match &row.disposition {
            AuditDisposition::Exportable { evidence } => evidence
                .iter()
                .any(|reference| reference.table == table && &reference.key == key),
            AuditDisposition::Internal => false,
        })
    }

    fn pins(&self) -> BTreeSet<(Table, RowKey)> {
        let mut pins = BTreeSet::new();
        for row in self.audit.values() {
            if let AuditDisposition::Exportable { evidence } = &row.disposition {
                for reference in evidence {
                    pins.insert((reference.table, reference.key.clone()));
                }
            }
        }
        pins
    }

    fn expire(&mut self, now: u64, policy: RetentionPolicy) -> ExpiryReport {
        let mut report = ExpiryReport::default();
        self.audit.retain(|_, row| match &row.disposition {
            AuditDisposition::Internal => {
                if retention::elapsed(now, row.written_at, policy.audit) {
                    report.expired_audit += 1;
                    false
                } else {
                    true
                }
            }
            AuditDisposition::Exportable { .. } => {
                if retention::elapsed(now, row.written_at, policy.audit) {
                    report.exportable_audit_retained += 1;
                }
                true
            }
        });
        let pins = self.pins();
        for (table, rows) in &mut self.tables {
            let period = policy.period(*table);
            rows.retain(|key, row| {
                if !retention::elapsed(now, row.written_at, period) {
                    return true;
                }
                if pins.contains(&(*table, key.clone())) {
                    report.pinned_evidence_retained += 1;
                    true
                } else {
                    report.expired_rows += 1;
                    false
                }
            });
        }
        report
    }
}

/// Durable principal-scoped storage. Every row, cache entry, journey,
/// notification and audit record lives under exactly one authenticated
/// principal, and rows are only reachable through a [`PrincipalScope`].
#[derive(Debug)]
pub struct PrincipalStore {
    root: PathBuf,
    version: u32,
    retention: RetentionPolicy,
    tenancy: TenancyMap,
}

impl PrincipalStore {
    /// Revalidates the durable store root and installed tenancy authority.
    /// Readiness uses this to detect replacement, corruption, or loss after startup.
    pub fn probe(&self) -> Result<(), StoreError> {
        verify_principals_tree(&self.root)?;
        let installed = tenancy::load_installed(&self.root)?;
        if installed.digest()? != self.tenancy.digest()? {
            return Err(StoreError::Tenancy(TenancyError::DigestMismatch));
        }
        Ok(())
    }

    /// Opens the store at `root`, applying forward-only migrations, refusing
    /// versions this binary does not know, and authenticating the installed
    /// tenancy configuration against the digest recorded out of band.
    ///
    /// # Errors
    ///
    /// Returns typed migration refusals, tenancy authentication failures, and
    /// storage corruption or I/O failures.
    pub fn open(
        root: impl AsRef<Path>,
        retention: RetentionPolicy,
        tenancy_digest: TenancyDigest,
    ) -> Result<Self, StoreError> {
        let root = root.as_ref().to_path_buf();
        fs::create_dir_all(&root)?;
        let version = migrate::prepare(&root)?;
        verify_principals_tree(&root)?;
        let tenancy = tenancy::load_installed(&root)?;
        if tenancy.digest()? != tenancy_digest {
            return Err(StoreError::Tenancy(TenancyError::DigestMismatch));
        }
        Ok(Self {
            root,
            version,
            retention,
            tenancy,
        })
    }

    /// Presents a principal and receives the only access path to its rows.
    /// The scope carries the principal's agent-layer tenancy so agent-layer
    /// isolation composes with principal scoping.
    ///
    /// # Errors
    ///
    /// Refuses principals without a tenancy mapping and corrupt or
    /// newer-versioned principal files.
    pub fn principal(&mut self, id: &PrincipalId) -> Result<PrincipalScope<'_>, StoreError> {
        let tenant = self
            .tenancy
            .tenant_for(id)
            .map_err(|_| StoreError::Tenancy(TenancyError::Unmapped))?
            .clone();
        let directory = self.root.join(PRINCIPALS_DIR).join(id.as_str());
        let path = directory.join(STORE_FILE);
        let state = if path.exists() {
            decode_state(&fs::read(&path)?, self.version)?
        } else {
            PrincipalState::default()
        };
        Ok(PrincipalScope {
            store: self,
            principal: id.clone(),
            tenant,
            directory,
            state,
        })
    }

    /// Returns the store version this binary declared and enforces.
    #[must_use]
    pub const fn version(&self) -> u32 {
        self.version
    }

    /// Returns the declared per-store retention policy.
    #[must_use]
    pub const fn retention(&self) -> RetentionPolicy {
        self.retention
    }

    /// Returns the authenticated tenancy configuration.
    #[must_use]
    pub const fn tenancy(&self) -> &TenancyMap {
        &self.tenancy
    }
}

fn verify_principals_tree(root: &Path) -> Result<(), StoreError> {
    let principals = root.join(PRINCIPALS_DIR);
    if !principals.is_dir() {
        return Ok(());
    }
    for entry in fs::read_dir(principals)? {
        let entry = entry?;
        let name = entry.file_name();
        let Some(name) = name.to_str() else {
            return Err(StoreError::Corrupt("foreign principals entry"));
        };
        if PrincipalId::new(name).is_err() || !entry.path().is_dir() {
            return Err(StoreError::Corrupt("foreign principals entry"));
        }
    }
    Ok(())
}

/// The only access path to stored rows: a handle bound to one authenticated
/// principal and its agent-layer tenancy. No unscoped query exists.
#[derive(Debug)]
pub struct PrincipalScope<'a> {
    store: &'a PrincipalStore,
    principal: PrincipalId,
    tenant: AgentTenantId,
    directory: PathBuf,
    state: PrincipalState,
}

impl PrincipalScope<'_> {
    /// Returns the principal this scope is bound to.
    #[must_use]
    pub const fn principal(&self) -> &PrincipalId {
        &self.principal
    }

    /// Returns the agent-layer tenant mapped to this principal.
    #[must_use]
    pub const fn tenant(&self) -> &AgentTenantId {
        &self.tenant
    }

    /// Reads one row of this principal.
    #[must_use]
    pub fn get(&self, table: Table, key: &RowKey) -> Option<StoredRow<'_>> {
        let row = self.state.tables.get(&table)?.get(key)?;
        Some(StoredRow {
            written_at: row.written_at,
            bytes: &row.bytes,
        })
    }

    /// Lists this principal's keys in one table in deterministic order.
    #[must_use]
    pub fn keys(&self, table: Table) -> Vec<RowKey> {
        self.state
            .tables
            .get(&table)
            .map_or_else(Vec::new, |rows| rows.keys().cloned().collect())
    }

    /// Writes one row with a caller-injected timestamp.
    ///
    /// # Errors
    ///
    /// Refuses to overwrite a row pinned as evidence by an exportable audit
    /// entry, and returns persistence failures with the write rolled back.
    pub fn put(
        &mut self,
        table: Table,
        key: RowKey,
        written_at: u64,
        bytes: Vec<u8>,
    ) -> Result<(), StoreError> {
        if self.state.pinned(table, &key) {
            return Err(StoreError::EvidencePinned);
        }
        let previous = self
            .state
            .tables
            .entry(table)
            .or_default()
            .insert(key.clone(), Row { written_at, bytes });
        if let Err(error) = self.persist() {
            match previous {
                Some(row) => {
                    self.state.tables.entry(table).or_default().insert(key, row);
                }
                None => {
                    if let Some(rows) = self.state.tables.get_mut(&table) {
                        rows.remove(&key);
                    }
                }
            }
            return Err(error);
        }
        Ok(())
    }

    /// Removes one row, reporting whether it existed.
    ///
    /// # Errors
    ///
    /// Refuses to remove a row pinned as evidence by an exportable audit
    /// entry, and returns persistence failures with the removal rolled back.
    pub fn remove(&mut self, table: Table, key: &RowKey) -> Result<bool, StoreError> {
        if self.state.pinned(table, key) {
            return Err(StoreError::EvidencePinned);
        }
        let previous = self
            .state
            .tables
            .get_mut(&table)
            .and_then(|rows| rows.remove(key));
        let Some(previous) = previous else {
            return Ok(false);
        };
        if let Err(error) = self.persist() {
            self.state
                .tables
                .entry(table)
                .or_default()
                .insert(key.clone(), previous);
            return Err(error);
        }
        Ok(true)
    }

    /// Appends one audit entry; the audit log is append-only. Exportable
    /// entries must reference rows that exist in this scope.
    ///
    /// # Errors
    ///
    /// Refuses duplicate keys and dangling evidence references, and returns
    /// persistence failures with the append rolled back.
    pub fn append_audit(
        &mut self,
        key: RowKey,
        written_at: u64,
        bytes: Vec<u8>,
        disposition: AuditDisposition,
    ) -> Result<(), StoreError> {
        if self.state.audit.contains_key(&key) {
            return Err(StoreError::DuplicateAuditEntry);
        }
        if let AuditDisposition::Exportable { evidence } = &disposition {
            for reference in evidence {
                if !self.state.contains(reference.table, &reference.key) {
                    return Err(StoreError::MissingEvidence);
                }
            }
        }
        let rollback = key.clone();
        self.state.audit.insert(
            key,
            AuditRow {
                written_at,
                bytes,
                disposition,
            },
        );
        if let Err(error) = self.persist() {
            self.state.audit.remove(&rollback);
            return Err(error);
        }
        Ok(())
    }

    /// Reads one audit entry of this principal.
    #[must_use]
    pub fn audit(&self, key: &RowKey) -> Option<AuditEntry<'_>> {
        let row = self.state.audit.get(key)?;
        Some(AuditEntry {
            written_at: row.written_at,
            bytes: &row.bytes,
            disposition: &row.disposition,
        })
    }

    /// Lists this principal's audit keys in deterministic order.
    #[must_use]
    pub fn audit_keys(&self) -> Vec<RowKey> {
        self.state.audit.keys().cloned().collect()
    }

    /// Sweeps this principal under the declared retention policy at the
    /// injected time. Exportable audit entries and every row they reference
    /// as evidence are never deleted.
    ///
    /// # Errors
    ///
    /// Returns persistence failures with the sweep rolled back.
    pub fn expire(&mut self, now: u64) -> Result<ExpiryReport, StoreError> {
        let before = self.state.clone();
        let report = self.state.expire(now, self.store.retention);
        if let Err(error) = self.persist() {
            self.state = before;
            return Err(error);
        }
        Ok(report)
    }

    fn persist(&self) -> Result<(), StoreError> {
        let bytes = encode_state(&self.state, self.store.version)?;
        codec::write_atomic(&self.directory, STORE_FILE, TEMP_FILE, &bytes)?;
        Ok(())
    }
}

fn encode_state(state: &PrincipalState, version: u32) -> Result<Vec<u8>, StoreError> {
    let mut output = Vec::new();
    output.extend_from_slice(STORE_MAGIC);
    output.extend_from_slice(&version.to_be_bytes());
    let row_total: usize = state.tables.values().map(BTreeMap::len).sum();
    codec::push_length(&mut output, row_total)?;
    for (table, rows) in &state.tables {
        for (key, row) in rows {
            output.push(table.code());
            codec::push_bytes(&mut output, key.as_str().as_bytes())?;
            output.extend_from_slice(&row.written_at.to_be_bytes());
            codec::push_bytes(&mut output, &row.bytes)?;
        }
    }
    codec::push_length(&mut output, state.audit.len())?;
    for (key, row) in &state.audit {
        encode_audit_row(&mut output, key, row)?;
    }
    Ok(output)
}

fn encode_audit_row(output: &mut Vec<u8>, key: &RowKey, row: &AuditRow) -> Result<(), StoreError> {
    codec::push_bytes(output, key.as_str().as_bytes())?;
    output.extend_from_slice(&row.written_at.to_be_bytes());
    codec::push_bytes(output, &row.bytes)?;
    match &row.disposition {
        AuditDisposition::Internal => {
            output.push(0);
            codec::push_length(output, 0)?;
        }
        AuditDisposition::Exportable { evidence } => {
            output.push(1);
            codec::push_length(output, evidence.len())?;
            for reference in evidence {
                output.push(reference.table.code());
                codec::push_bytes(output, reference.key.as_str().as_bytes())?;
            }
        }
    }
    Ok(())
}

fn decode_state(bytes: &[u8], current: u32) -> Result<PrincipalState, StoreError> {
    let mut decoder = Decoder::new(bytes);
    if decoder.take(4)? != STORE_MAGIC {
        return Err(StoreError::Corrupt("invalid principal header"));
    }
    let version = decoder.u32()?;
    if version > current {
        return Err(StoreError::Migration(MigrationError::NewerSchema {
            found: version,
            supported: current,
        }));
    }
    if version < current {
        return Err(StoreError::Corrupt("principal file below store version"));
    }
    let mut state = PrincipalState::default();
    decode_rows(&mut decoder, &mut state)?;
    decode_audit(&mut decoder, &mut state)?;
    if !decoder.is_empty() {
        return Err(StoreError::Corrupt("trailing bytes"));
    }
    Ok(state)
}

fn decode_rows(decoder: &mut Decoder<'_>, state: &mut PrincipalState) -> Result<(), StoreError> {
    let count = decoder.length()?;
    for _ in 0..count {
        let table = Table::from_code(decoder.byte()?)?;
        let key = decode_key(decoder)?;
        let written_at = decoder.u64()?;
        let bytes = decoder.bytes()?.to_vec();
        if state
            .tables
            .entry(table)
            .or_default()
            .insert(key, Row { written_at, bytes })
            .is_some()
        {
            return Err(StoreError::Corrupt("duplicate row"));
        }
    }
    Ok(())
}

fn decode_audit(decoder: &mut Decoder<'_>, state: &mut PrincipalState) -> Result<(), StoreError> {
    let count = decoder.length()?;
    for _ in 0..count {
        let key = decode_key(decoder)?;
        let written_at = decoder.u64()?;
        let bytes = decoder.bytes()?.to_vec();
        let disposition = decode_disposition(decoder, state)?;
        if state
            .audit
            .insert(
                key,
                AuditRow {
                    written_at,
                    bytes,
                    disposition,
                },
            )
            .is_some()
        {
            return Err(StoreError::Corrupt("duplicate audit entry"));
        }
    }
    Ok(())
}

fn decode_disposition(
    decoder: &mut Decoder<'_>,
    state: &PrincipalState,
) -> Result<AuditDisposition, StoreError> {
    let flag = decoder.byte()?;
    let references = decoder.length()?;
    match flag {
        0 => {
            if references != 0 {
                return Err(StoreError::Corrupt("internal audit entry carries evidence"));
            }
            Ok(AuditDisposition::Internal)
        }
        1 => {
            let mut evidence = Vec::new();
            for _ in 0..references {
                let table = Table::from_code(decoder.byte()?)?;
                let key = decode_key(decoder)?;
                if !state.contains(table, &key) {
                    return Err(StoreError::Corrupt("dangling evidence reference"));
                }
                evidence.push(EvidenceRef { table, key });
            }
            Ok(AuditDisposition::Exportable { evidence })
        }
        _ => Err(StoreError::Corrupt("unknown audit disposition")),
    }
}

fn decode_key(decoder: &mut Decoder<'_>) -> Result<RowKey, StoreError> {
    let text = std::str::from_utf8(decoder.bytes()?)
        .map_err(|_| StoreError::Corrupt("stored key is not UTF-8"))?;
    RowKey::new(text).map_err(|_| StoreError::Corrupt("invalid stored key"))
}
