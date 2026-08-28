//! Persistent program storage whose address space is structurally scoped to
//! either one program/principal pair or one program-shared plane. Guest-facing
//! APIs never accept an arbitrary namespace, so neither adjacent programs nor
//! adjacent principals can be reached by choosing a key.

use core::fmt::{self, Display};
use std::cell::RefCell;
use std::collections::{BTreeMap, BTreeSet};

mod namespace;
#[path = "scan.rs"]
mod ordered_scan;
pub mod reclaim;

pub use namespace::StorageNamespace;
pub(crate) use ordered_scan::scan_cells;
pub use ordered_scan::{ScanEntry, ScanLimits, StorageScan, MAX_STORAGE_SCAN_CURSOR_BYTES};
pub use reclaim::NamespaceDrop;

/// Maximum key length admitted by the version-one storage ABI.
pub const MAX_STORAGE_KEY_BYTES: usize = 256;
/// Maximum value length admitted by the version-one storage ABI.
pub const MAX_STORAGE_VALUE_BYTES: usize = 1_048_576;

/// Stable identifier of a deployed program.
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub struct ProgramId([u8; 32]);

impl ProgramId {
    /// Constructs a nonzero program identifier.
    ///
    /// # Errors
    ///
    /// Refuses the all-zero identifier reserved for absence.
    pub fn new(bytes: [u8; 32]) -> Result<Self, StorageError> {
        if bytes == [0; 32] {
            return Err(StorageError::InvalidProgram);
        }
        Ok(Self(bytes))
    }

    /// Returns the canonical identifier bytes.
    #[must_use]
    pub const fn bytes(self) -> [u8; 32] {
        self.0
    }
}

/// Stable identifier of the principal whose authority invoked a program.
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub struct PrincipalId([u8; 32]);

impl PrincipalId {
    /// Constructs a nonzero principal identifier.
    ///
    /// # Errors
    ///
    /// Refuses the all-zero identifier reserved for absence.
    pub fn new(bytes: [u8; 32]) -> Result<Self, StorageError> {
        if bytes == [0; 32] {
            return Err(StorageError::InvalidPrincipal);
        }
        Ok(Self(bytes))
    }

    /// Returns the canonical identifier bytes.
    #[must_use]
    pub const fn bytes(self) -> [u8; 32] {
        self.0
    }
}

#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub(crate) struct StorageAddress {
    namespace: StorageNamespace,
    key: Vec<u8>,
}

/// Typed namespaced-storage refusal.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum StorageError {
    InvalidProgram,
    InvalidPrincipal,
    EmptyKey,
    KeyTooLarge,
    ValueTooLarge,
    PrefixTooLarge,
    InvalidScanCursor,
    InvalidScanLimits,
    ScanCeilingExceeded,
    FrozenNamespace,
    SizeOverflow,
}

impl Display for StorageError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidProgram => formatter.write_str("program identifier is reserved"),
            Self::InvalidPrincipal => formatter.write_str("principal identifier is reserved"),
            Self::EmptyKey => formatter.write_str("storage key is empty"),
            Self::KeyTooLarge => formatter.write_str("storage key exceeds the ABI bound"),
            Self::ValueTooLarge => formatter.write_str("storage value exceeds the ABI bound"),
            Self::PrefixTooLarge => {
                formatter.write_str("storage scan prefix exceeds the ABI bound")
            }
            Self::InvalidScanCursor => {
                formatter.write_str("storage scan cursor is invalid or belongs to another scan")
            }
            Self::InvalidScanLimits => formatter.write_str("storage scan limits are invalid"),
            Self::ScanCeilingExceeded => formatter
                .write_str("storage scan entry exceeds the declared complete page byte ceiling"),
            Self::FrozenNamespace => formatter.write_str("storage namespace is frozen"),
            Self::SizeOverflow => formatter.write_str("storage accounting overflowed"),
        }
    }
}

impl std::error::Error for StorageError {}

/// Durable storage shared by program executions. Every map key includes a
/// closed namespace value carrying its owning program and declared scope.
#[derive(Clone, Debug, Default)]
pub struct Storage {
    cells: BTreeMap<StorageAddress, Vec<u8>>,
    frozen_namespaces: BTreeSet<StorageNamespace>,
    accessed_namespaces: RefCell<BTreeSet<StorageNamespace>>,
}

impl PartialEq for Storage {
    fn eq(&self, other: &Self) -> bool {
        self.cells == other.cells &&
            self.frozen_namespaces == other.frozen_namespaces
    }
}

impl Eq for Storage {}

impl Storage {
    fn commitment_key_len(address: &StorageAddress) -> Option<u64> {
        let mut namespace = [0_u8; 65];
        let namespace_len = address.namespace.write_canonical(&mut namespace);
        u64::try_from(2_usize.checked_add(namespace_len)?.checked_add(address.key.len())?).ok()
    }
    fn commitment_key(address: &StorageAddress) -> Vec<u8> {
        let namespace = address.namespace.canonical_bytes();
        let namespace_length = u16::try_from(namespace.len())
            .unwrap_or_else(|_| unreachable!("closed storage namespace length is bounded"));
        let mut key = Vec::with_capacity(
            2_usize.saturating_add(namespace.len()).saturating_add(address.key.len()),
        );
        key.extend_from_slice(&namespace_length.to_be_bytes());
        key.extend_from_slice(&namespace);
        key.extend_from_slice(&address.key);
        key
    }
    /// Creates an empty storage plane.
    #[must_use]
    pub const fn new() -> Self {
        Self {
            cells: BTreeMap::new(),
            frozen_namespaces: BTreeSet::new(),
            accessed_namespaces: RefCell::new(BTreeSet::new()),
        }
    }

    pub(crate) fn enforce_frozen_namespaces(
        &mut self,
        namespaces: impl IntoIterator<Item = StorageNamespace>,
    ) {
        self.frozen_namespaces = namespaces.into_iter().collect();
    }

    fn ensure_accessible(&self, namespace: StorageNamespace) -> Result<(), StorageError> {
        if self.frozen_namespaces.contains(&namespace) {
            Err(StorageError::FrozenNamespace)
        } else {
            self.accessed_namespaces.borrow_mut().insert(namespace);
            Ok(())
        }
    }

    pub(crate) fn clear_access_log(&self) {
        self.accessed_namespaces.borrow_mut().clear();
    }

    pub(crate) fn was_accessed(&self, namespace: StorageNamespace) -> bool {
        self.accessed_namespaces.borrow().contains(&namespace)
    }

    pub(crate) fn commitment_entries(&self) -> Vec<(Vec<u8>, Vec<u8>)> {
        self.cells.iter().map(|(address, value)| {
            (Self::commitment_key(address), value.clone())
        }).collect()
    }

    pub(crate) fn for_each_commitment_entry(
        &self,
        mut visit: impl FnMut(Vec<u8>, &[u8]),
    ) {
        for (address, value) in &self.cells {
            visit(Self::commitment_key(address), value);
        }
    }

    pub(crate) fn for_each_commitment_delta(
        &self,
        baseline: &Self,
        mut visit: impl FnMut(Vec<u8>, Option<&[u8]>),
    ) {
        let mut current = self.cells.iter().peekable();
        let mut baseline = baseline.cells.iter().peekable();
        loop {
            match (current.peek(), baseline.peek()) {
                (Some((address, value)), Some((baseline_address, baseline_value))) => match address.cmp(baseline_address) {
                    core::cmp::Ordering::Less => { visit(Self::commitment_key(address), Some(value)); current.next(); }
                    core::cmp::Ordering::Greater => { visit(Self::commitment_key(baseline_address), None); baseline.next(); }
                    core::cmp::Ordering::Equal => {
                        if value.as_slice() != baseline_value.as_slice() { visit(Self::commitment_key(address), Some(value)); }
                        current.next(); baseline.next();
                    }
                },
                (Some((address, value)), None) => { visit(Self::commitment_key(address), Some(value)); current.next(); }
                (None, Some((address, _))) => { visit(Self::commitment_key(address), None); baseline.next(); }
                (None, None) => break,
            }
        }
    }

    pub(crate) fn commitment_delta_metrics(&self, baseline: &Self) -> Option<(usize, u64)> {
        let mut current = self.cells.iter().peekable();
        let mut baseline = baseline.cells.iter().peekable();
        let mut entries = 0_usize;
        let mut bytes = 4_u64;
        loop {
            let (key_bytes, value_bytes, advance_current, advance_baseline) = match (current.peek(), baseline.peek()) {
                (Some((address, value)), Some((baseline_address, baseline_value))) => match address.cmp(baseline_address) {
                    core::cmp::Ordering::Less => (Self::commitment_key_len(address)?, Some(u64::try_from(value.len()).ok()?), true, false),
                    core::cmp::Ordering::Greater => (Self::commitment_key_len(baseline_address)?, None, false, true),
                    core::cmp::Ordering::Equal if value.as_slice() != baseline_value.as_slice() => (Self::commitment_key_len(address)?, Some(u64::try_from(value.len()).ok()?), true, true),
                    core::cmp::Ordering::Equal => { current.next(); baseline.next(); continue },
                },
                (Some((address, value)), None) => (Self::commitment_key_len(address)?, Some(u64::try_from(value.len()).ok()?), true, false),
                (None, Some((baseline_address, _))) => (Self::commitment_key_len(baseline_address)?, None, false, true),
                (None, None) => break,
            };
            entries = entries.checked_add(1)?;
            bytes = bytes.checked_add(match value_bytes {
                Some(value_bytes) => 9_u64.checked_add(key_bytes)?.checked_add(value_bytes)?,
                None => 5_u64.checked_add(key_bytes)?,
            })?;
            if advance_current { current.next(); }
            if advance_baseline { baseline.next(); }
        }
        Some((entries, bytes))
    }

    /// Begins an isolated write transaction. Dropping it without commit leaves
    /// durable storage byte-identical.
    #[must_use]
    pub fn transaction(&mut self, namespace: StorageNamespace) -> StorageTransaction<'_> {
        StorageTransaction {
            owner: self,
            namespace,
            writes: BTreeMap::new(),
        }
    }

    /// Returns the number of cells visible in exactly one namespace.
    #[must_use]
    pub fn namespace_cell_count(&self, namespace: StorageNamespace) -> usize {
        self.cells
            .keys()
            .filter(|address| address.namespace == namespace)
            .count()
    }

    /// Returns one fixed namespace in canonical key order for the protocol
    /// persistence bridge. The returned copies cannot mutate the held state.
    pub(crate) fn namespace_entries(&self, namespace: StorageNamespace) -> Vec<(Vec<u8>, Vec<u8>)> {
        self.cells
            .iter()
            .filter(|(address, _)| address.namespace == namespace)
            .map(|(address, value)| (address.key.clone(), value.clone()))
            .collect()
    }

    /// Returns exact persistent key-plus-value bytes in one namespace.
    /// Adjacent program or principal namespaces never contribute.
    ///
    /// # Errors
    ///
    /// Refuses accounting that cannot fit the runtime's `u64` counters.
    pub fn namespace_persistent_bytes(
        &self,
        namespace: StorageNamespace,
    ) -> Result<u64, StorageError> {
        self.cells
            .iter()
            .filter(|(address, _)| address.namespace == namespace)
            .try_fold(0u64, |total, (address, value)| {
                let cell_bytes = metered_bytes(&address.key, Some(value))?;
                total
                    .checked_add(cell_bytes)
                    .ok_or(StorageError::SizeOverflow)
            })
    }

    /// Returns every nonempty namespace and its exact persistent bytes in
    /// canonical namespace order. Protocol state transitions use this to prove
    /// that no occupied namespace escaped responsibility accounting.
    pub fn namespace_sizes(&self) -> Result<Vec<(StorageNamespace, u64)>, StorageError> {
        let mut sizes = BTreeMap::<StorageNamespace, u64>::new();
        for (address, value) in &self.cells {
            let bytes = metered_bytes(&address.key, Some(value))?;
            let size = sizes.entry(address.namespace).or_default();
            *size = size.checked_add(bytes).ok_or(StorageError::SizeOverflow)?;
        }
        Ok(sizes.into_iter().collect())
    }

    /// Returns canonical copies of every cell in one protocol-owned namespace.
    pub fn protocol_namespace_entries(
        &self, namespace: StorageNamespace,
    ) -> Result<Vec<(Vec<u8>, Vec<u8>)>, StorageError> {
        self.ensure_accessible(namespace)?;
        Ok(self.namespace_entries(namespace))
    }

    /// Atomically replaces one protocol-owned namespace with an exact canonical cell set.
    pub fn replace_protocol_namespace(
        &mut self, namespace: StorageNamespace, entries: &[(Vec<u8>, Vec<u8>)],
    ) -> Result<(), StorageError> {
        if entries.windows(2).any(|pair| pair[0].0 >= pair[1].0) {
            return Err(StorageError::PrefixTooLarge);
        }
        let existing = self.protocol_namespace_entries(namespace)?;
        let mut transaction = self.transaction(namespace);
        for (key, _) in existing { transaction.delete(&key)?; }
        for (key, value) in entries { transaction.write(key, value)?; }
        transaction.commit();
        Ok(())
    }

    /// Returns canonical copies of cells beneath one protocol-owned key prefix.
    pub fn protocol_prefix_entries(
        &self, namespace: StorageNamespace, prefix: &[u8],
    ) -> Result<Vec<(Vec<u8>, Vec<u8>)>, StorageError> {
        self.ensure_accessible(namespace)?;
        validate_key(prefix)?;
        Ok(self.cells.iter()
            .filter(|(address, _)| address.namespace == namespace && address.key.starts_with(prefix))
            .map(|(address, value)| (address.key.clone(), value.clone())).collect())
    }

    /// Returns exact key-plus-value occupancy beneath one protocol-owned prefix.
    pub fn protocol_prefix_bytes(
        &self, namespace: StorageNamespace, prefix: &[u8],
    ) -> Result<u64, StorageError> {
        self.protocol_prefix_entries(namespace, prefix)?.iter().try_fold(0u64,
            |total, (key, value)| total.checked_add(metered_bytes(key, Some(value))?)
                .ok_or(StorageError::SizeOverflow))
    }

    /// Atomically replaces one protocol-owned prefix with an exact canonical cell set.
    pub fn replace_protocol_prefix(
        &mut self, namespace: StorageNamespace, prefix: &[u8],
        entries: &[(Vec<u8>, Vec<u8>)],
    ) -> Result<(), StorageError> {
        validate_key(prefix)?;
        if entries.windows(2).any(|pair| pair[0].0 >= pair[1].0)
            || entries.iter().any(|(key, _)| !key.starts_with(prefix)) {
            return Err(StorageError::PrefixTooLarge);
        }
        let existing = self.protocol_prefix_entries(namespace, prefix)?;
        let mut transaction = self.transaction(namespace);
        for (key, _) in existing { transaction.delete(&key)?; }
        for (key, value) in entries { transaction.write(key, value)?; }
        transaction.commit();
        Ok(())
    }

    /// Computes exact facts for dropping one namespace without mutating this
    /// storage snapshot. The caller charges this preview before committing the
    /// corresponding reclamation.
    pub(crate) fn namespace_drop_preview(
        &self,
        namespace: StorageNamespace,
    ) -> Result<NamespaceDrop, StorageError> {
        self.ensure_accessible(namespace)?;
        reclaim::preview(&self.cells, namespace)
    }

    /// Removes every cell of a preflighted namespace from this storage
    /// snapshot. Only the ABI can obtain a namespace drop fact from a
    /// guest-selected scope and matching write authority.
    pub(crate) fn reclaim_namespace(&mut self, drop: NamespaceDrop) {
        reclaim::apply(&mut self.cells, drop);
    }

    pub(crate) fn read(
        &self,
        namespace: StorageNamespace,
        key: &[u8],
    ) -> Result<Option<Vec<u8>>, StorageError> {
        self.ensure_accessible(namespace)?;
        validate_key(key)?;
        Ok(self
            .cells
            .get(&StorageAddress {
                namespace,
                key: key.to_vec(),
            })
            .cloned())
    }

    pub(crate) fn write(
        &mut self,
        namespace: StorageNamespace,
        key: &[u8],
        value: &[u8],
    ) -> Result<(), StorageError> {
        self.ensure_accessible(namespace)?;
        validate_key(key)?;
        if value.len() > MAX_STORAGE_VALUE_BYTES {
            return Err(StorageError::ValueTooLarge);
        }
        self.cells.insert(
            StorageAddress {
                namespace,
                key: key.to_vec(),
            },
            value.to_vec(),
        );
        Ok(())
    }

    pub(crate) fn delete(
        &mut self,
        namespace: StorageNamespace,
        key: &[u8],
    ) -> Result<(), StorageError> {
        self.ensure_accessible(namespace)?;
        validate_key(key)?;
        self.cells.remove(&StorageAddress {
            namespace,
            key: key.to_vec(),
        });
        Ok(())
    }

    /// Scans one fixed namespace in canonical key order. The cursor is an
    /// externally portable, self-describing continuation token; it is checked
    /// against this exact namespace, prefix, and declared page contract before
    /// any entries are returned.
    ///
    /// # Errors
    ///
    /// Refuses malformed, foreign, or non-canonical cursors, invalid limits,
    /// and an entry that cannot fit the caller-declared complete canonical
    /// page byte ceiling.
    pub(crate) fn scan(
        &self,
        namespace: StorageNamespace,
        prefix: &[u8],
        cursor: &[u8],
        limits: ScanLimits,
    ) -> Result<StorageScan, StorageError> {
        self.ensure_accessible(namespace)?;
        scan_cells(&self.cells, namespace, prefix, cursor, limits)
    }
}

/// An atomic transaction fixed to one namespace at construction.
#[derive(Debug)]
pub struct StorageTransaction<'a> {
    owner: &'a mut Storage,
    namespace: StorageNamespace,
    writes: BTreeMap<Vec<u8>, Option<Vec<u8>>>,
}

impl StorageTransaction<'_> {
    /// Reads only from the transaction's fixed namespace.
    ///
    /// # Errors
    ///
    /// Refuses empty and oversized keys.
    pub fn read(&self, key: &[u8]) -> Result<Option<Vec<u8>>, StorageError> {
        self.owner.ensure_accessible(self.namespace)?;
        validate_key(key)?;
        if let Some(value) = self.writes.get(key) {
            return Ok(value.clone());
        }
        Ok(self
            .owner
            .cells
            .get(&StorageAddress {
                namespace: self.namespace,
                key: key.to_vec(),
            })
            .cloned())
    }

    /// Stages a bounded value in the fixed namespace.
    ///
    /// # Errors
    ///
    /// Refuses empty or oversized keys and oversized values.
    pub fn write(&mut self, key: &[u8], value: &[u8]) -> Result<(), StorageError> {
        self.owner.ensure_accessible(self.namespace)?;
        validate_key(key)?;
        if value.len() > MAX_STORAGE_VALUE_BYTES {
            return Err(StorageError::ValueTooLarge);
        }
        self.writes.insert(key.to_vec(), Some(value.to_vec()));
        Ok(())
    }

    /// Stages deletion of a key in the fixed namespace.
    ///
    /// # Errors
    ///
    /// Refuses empty and oversized keys.
    pub fn delete(&mut self, key: &[u8]) -> Result<(), StorageError> {
        self.owner.ensure_accessible(self.namespace)?;
        validate_key(key)?;
        self.writes.insert(key.to_vec(), None);
        Ok(())
    }

    /// Atomically applies all staged writes and returns the number of changed
    /// cells. No guest-visible operation can alter the transaction namespace.
    #[must_use]
    pub fn commit(self) -> usize {
        let mut changed = 0usize;
        for (key, value) in self.writes {
            let address = StorageAddress {
                namespace: self.namespace,
                key,
            };
            match value {
                Some(value) => {
                    if self.owner.cells.get(&address) != Some(&value) {
                        self.owner.cells.insert(address, value);
                        changed = changed.saturating_add(1);
                    }
                }
                None => {
                    if self.owner.cells.remove(&address).is_some() {
                        changed = changed.saturating_add(1);
                    }
                }
            }
        }
        changed
    }
}

fn validate_key(key: &[u8]) -> Result<(), StorageError> {
    if key.is_empty() {
        return Err(StorageError::EmptyKey);
    }
    if key.len() > MAX_STORAGE_KEY_BYTES {
        return Err(StorageError::KeyTooLarge);
    }
    Ok(())
}

/// Computes exact storage metering bytes for a key and optional value.
///
/// # Errors
///
/// Refuses lengths that cannot fit the runtime's `u64` meter.
pub fn metered_bytes(key: &[u8], value: Option<&[u8]>) -> Result<u64, StorageError> {
    let bytes = key
        .len()
        .checked_add(value.map_or(0, <[u8]>::len))
        .ok_or(StorageError::SizeOverflow)?;
    u64::try_from(bytes).map_err(|_| StorageError::SizeOverflow)
}
