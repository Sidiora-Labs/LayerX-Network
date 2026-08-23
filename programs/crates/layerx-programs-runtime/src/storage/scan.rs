//! Canonical bounded storage iteration and portable continuation cursors.
//!
//! A cursor carries the complete scan identity rather than a host iterator or
//! allocator handle. It therefore resumes identically across activities and
//! platforms while refusing namespace, prefix, and limit-contract changes.

use std::collections::BTreeMap;

use super::{StorageAddress, StorageError, StorageNamespace, MAX_STORAGE_KEY_BYTES};

/// The maximum number of entries one storage-scan host call may return.
pub const MAX_STORAGE_SCAN_ENTRIES: u32 = 64;

const CURSOR_VERSION: u8 = 1;
const MAX_CURSOR_NAMESPACE_BYTES: usize = 65;
/// Maximum encoded bytes accepted for one portable scan cursor.
pub const MAX_STORAGE_SCAN_CURSOR_BYTES: usize = 1
    + 1
    + MAX_CURSOR_NAMESPACE_BYTES
    + 2
    + MAX_STORAGE_KEY_BYTES
    + 4
    + 4
    + 2
    + MAX_STORAGE_KEY_BYTES;
const MIN_STORAGE_SCAN_PAGE_BYTES: u32 = 5;
/// The maximum complete canonical page bytes one storage-scan call may return.
pub const MAX_STORAGE_SCAN_BYTES: u32 = 67_126_228;

/// Declared limits for one deterministic scan call.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ScanLimits {
    max_entries: u32,
    max_bytes: u32,
}

impl ScanLimits {
    /// Constructs limits bounded by the protocol-declared per-call ceilings.
    ///
    /// # Errors
    ///
    /// Refuses zero or widening limits so a scan contract is always explicit
    /// and cannot exceed the runtime's fixed ceiling.
    pub const fn new(max_entries: u32, max_bytes: u32) -> Result<Self, StorageError> {
        if max_entries == 0
            || max_entries > MAX_STORAGE_SCAN_ENTRIES
            || max_bytes < MIN_STORAGE_SCAN_PAGE_BYTES
            || max_bytes > MAX_STORAGE_SCAN_BYTES
        {
            return Err(StorageError::InvalidScanLimits);
        }
        Ok(Self {
            max_entries,
            max_bytes,
        })
    }

    /// Returns the fixed production page ceilings.
    #[must_use]
    pub const fn declared() -> Self {
        Self {
            max_entries: MAX_STORAGE_SCAN_ENTRIES,
            max_bytes: MAX_STORAGE_SCAN_BYTES,
        }
    }

    /// Returns the caller-declared entry ceiling.
    #[must_use]
    pub const fn max_entries(self) -> u32 {
        self.max_entries
    }

    /// Returns the caller-declared complete canonical page byte ceiling.
    #[must_use]
    pub const fn max_bytes(self) -> u32 {
        self.max_bytes
    }
}

/// One key/value pair returned by a storage scan.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ScanEntry {
    pub key: Vec<u8>,
    pub value: Vec<u8>,
}

/// Ordered page returned by a storage scan.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct StorageScan {
    entries: Vec<ScanEntry>,
    cursor: Option<Vec<u8>>,
    metered_bytes: u64,
}

impl StorageScan {
    /// Returns entries in canonical namespace/key order.
    #[must_use]
    pub fn entries(&self) -> &[ScanEntry] {
        &self.entries
    }

    /// Returns the resumable cursor, if more matching entries remain.
    #[must_use]
    pub fn cursor(&self) -> Option<&[u8]> {
        self.cursor.as_deref()
    }

    /// Returns exact canonical page bytes copied to the guest and charged to
    /// the storage-read resource class.
    #[must_use]
    pub const fn metered_bytes(&self) -> u64 {
        self.metered_bytes
    }

    pub(crate) fn encode_for_guest(&self) -> Result<Vec<u8>, StorageError> {
        let entry_count =
            u16::try_from(self.entries.len()).map_err(|_| StorageError::SizeOverflow)?;
        let mut encoded = Vec::new();
        encoded.extend_from_slice(&entry_count.to_be_bytes());
        for entry in &self.entries {
            let key_len = u16::try_from(entry.key.len()).map_err(|_| StorageError::SizeOverflow)?;
            let value_len =
                u32::try_from(entry.value.len()).map_err(|_| StorageError::SizeOverflow)?;
            encoded.extend_from_slice(&key_len.to_be_bytes());
            encoded.extend_from_slice(&entry.key);
            encoded.extend_from_slice(&value_len.to_be_bytes());
            encoded.extend_from_slice(&entry.value);
        }
        let cursor = self.cursor.as_deref().unwrap_or_default();
        encoded.push(u8::from(!cursor.is_empty()));
        let cursor_len = u16::try_from(cursor.len()).map_err(|_| StorageError::SizeOverflow)?;
        encoded.extend_from_slice(&cursor_len.to_be_bytes());
        encoded.extend_from_slice(cursor);
        Ok(encoded)
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct ScanCursor {
    namespace: Vec<u8>,
    prefix: Vec<u8>,
    limits: ScanLimits,
    after: Vec<u8>,
}

impl ScanCursor {
    fn issued(
        namespace: StorageNamespace,
        prefix: &[u8],
        limits: ScanLimits,
        after: &[u8],
    ) -> Self {
        Self {
            namespace: namespace.canonical_bytes(),
            prefix: prefix.to_vec(),
            limits,
            after: after.to_vec(),
        }
    }

    fn encode(&self) -> Result<Vec<u8>, StorageError> {
        let namespace_len =
            u8::try_from(self.namespace.len()).map_err(|_| StorageError::SizeOverflow)?;
        let prefix_len =
            u16::try_from(self.prefix.len()).map_err(|_| StorageError::SizeOverflow)?;
        let after_len = u16::try_from(self.after.len()).map_err(|_| StorageError::SizeOverflow)?;
        let mut encoded = Vec::with_capacity(MAX_STORAGE_SCAN_CURSOR_BYTES);
        encoded.push(CURSOR_VERSION);
        encoded.push(namespace_len);
        encoded.extend_from_slice(&self.namespace);
        encoded.extend_from_slice(&prefix_len.to_be_bytes());
        encoded.extend_from_slice(&self.prefix);
        encoded.extend_from_slice(&self.limits.max_entries().to_be_bytes());
        encoded.extend_from_slice(&self.limits.max_bytes().to_be_bytes());
        encoded.extend_from_slice(&after_len.to_be_bytes());
        encoded.extend_from_slice(&self.after);
        Ok(encoded)
    }

    fn decode_for(
        encoded: &[u8],
        namespace: StorageNamespace,
        prefix: &[u8],
        limits: ScanLimits,
    ) -> Result<Self, StorageError> {
        if encoded.len() > MAX_STORAGE_SCAN_CURSOR_BYTES {
            return Err(StorageError::InvalidScanCursor);
        }
        let mut reader = CursorReader::new(encoded);
        if reader.take_u8()? != CURSOR_VERSION {
            return Err(StorageError::InvalidScanCursor);
        }
        let namespace_len = usize::from(reader.take_u8()?);
        let cursor_namespace = reader.take(namespace_len)?;
        let prefix_len = usize::from(reader.take_u16()?);
        let cursor_prefix = reader.take(prefix_len)?;
        let cursor_limits = ScanLimits::new(reader.take_u32()?, reader.take_u32()?)
            .map_err(|_| StorageError::InvalidScanCursor)?;
        let after_len = usize::from(reader.take_u16()?);
        let after = reader.take(after_len)?;
        if !reader.finished()
            || after.is_empty()
            || after.len() > MAX_STORAGE_KEY_BYTES
            || !after.starts_with(prefix)
            || cursor_namespace != namespace.canonical_bytes()
            || cursor_prefix != prefix
            || cursor_limits != limits
        {
            return Err(StorageError::InvalidScanCursor);
        }
        Ok(Self {
            namespace: cursor_namespace.to_vec(),
            prefix: cursor_prefix.to_vec(),
            limits: cursor_limits,
            after: after.to_vec(),
        })
    }
}

struct CursorReader<'a> {
    bytes: &'a [u8],
    position: usize,
}

impl<'a> CursorReader<'a> {
    const fn new(bytes: &'a [u8]) -> Self {
        Self { bytes, position: 0 }
    }

    fn take(&mut self, length: usize) -> Result<&'a [u8], StorageError> {
        let end = self
            .position
            .checked_add(length)
            .ok_or(StorageError::InvalidScanCursor)?;
        let value = self
            .bytes
            .get(self.position..end)
            .ok_or(StorageError::InvalidScanCursor)?;
        self.position = end;
        Ok(value)
    }

    fn take_u8(&mut self) -> Result<u8, StorageError> {
        self.take(1).and_then(|bytes| {
            bytes
                .first()
                .copied()
                .ok_or(StorageError::InvalidScanCursor)
        })
    }

    fn take_u16(&mut self) -> Result<u16, StorageError> {
        let bytes = self.take(2)?;
        Ok(u16::from_be_bytes([bytes[0], bytes[1]]))
    }

    fn take_u32(&mut self) -> Result<u32, StorageError> {
        let bytes = self.take(4)?;
        Ok(u32::from_be_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]))
    }

    const fn finished(&self) -> bool {
        self.position == self.bytes.len()
    }
}

/// Scans a namespace-owned B-tree map in canonical key order.
///
/// The `StorageAddress` ordering is program-major, scope-tagged, then key;
/// filtering one fixed namespace leaves the protocol's canonical key order.
pub(crate) fn scan_cells(
    cells: &BTreeMap<StorageAddress, Vec<u8>>,
    namespace: StorageNamespace,
    prefix: &[u8],
    cursor: &[u8],
    limits: ScanLimits,
) -> Result<StorageScan, StorageError> {
    if prefix.len() > MAX_STORAGE_KEY_BYTES {
        return Err(StorageError::PrefixTooLarge);
    }
    let after = if cursor.is_empty() {
        None
    } else {
        Some(ScanCursor::decode_for(cursor, namespace, prefix, limits)?.after)
    };
    let mut matching = cells
        .iter()
        .filter(|(address, _)| {
            address.namespace == namespace
                && address.key.starts_with(prefix)
                && !after
                    .as_ref()
                    .is_some_and(|after| address.key.as_slice() <= after.as_slice())
        })
        .peekable();
    let mut entries = Vec::new();

    while let Some((address, value)) = matching.next() {
        let has_more = matching.peek().is_some();
        if entries.len()
            == usize::try_from(limits.max_entries()).map_err(|_| StorageError::SizeOverflow)?
        {
            return page_with_cursor(namespace, prefix, limits, entries);
        }
        entries.push(ScanEntry {
            key: address.key.clone(),
            value: value.clone(),
        });
        let cursor = if has_more {
            Some(ScanCursor::issued(namespace, prefix, limits, &address.key).encode()?)
        } else {
            None
        };
        let page = page(entries.clone(), cursor)?;
        if page.metered_bytes > u64::from(limits.max_bytes()) {
            let _ = entries.pop();
            if entries.is_empty() {
                return Err(StorageError::ScanCeilingExceeded);
            }
            return page_with_cursor(namespace, prefix, limits, entries);
        }
        if has_more
            && entries.len()
                == usize::try_from(limits.max_entries()).map_err(|_| StorageError::SizeOverflow)?
        {
            return Ok(page);
        }
        if !has_more {
            return Ok(page);
        }
    }
    page(Vec::new(), None)
}

fn page_with_cursor(
    namespace: StorageNamespace,
    prefix: &[u8],
    limits: ScanLimits,
    entries: Vec<ScanEntry>,
) -> Result<StorageScan, StorageError> {
    let cursor = {
        let after = entries
            .last()
            .map(|entry| entry.key.as_slice())
            .ok_or(StorageError::ScanCeilingExceeded)?;
        ScanCursor::issued(namespace, prefix, limits, after).encode()?
    };
    let page = page(entries, Some(cursor))?;
    if page.metered_bytes > u64::from(limits.max_bytes()) {
        return Err(StorageError::ScanCeilingExceeded);
    }
    Ok(page)
}

fn page(entries: Vec<ScanEntry>, cursor: Option<Vec<u8>>) -> Result<StorageScan, StorageError> {
    let mut page = StorageScan {
        entries,
        cursor,
        metered_bytes: 0,
    };
    page.metered_bytes =
        u64::try_from(page.encode_for_guest()?.len()).map_err(|_| StorageError::SizeOverflow)?;
    Ok(page)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::abi::{
        Abi, AbiError, AuthorizationContext, Capability, CapabilitySet, ReceiptOracle, ReceiptView,
        StorageSelector,
    };
    use crate::meter::{FeeSchedule, Meter, MeterRefusal, ResourceBudget, ResourceKind};
    use crate::storage::{PrincipalId, ProgramId, Storage};
    use crate::ABI_VERSION;

    #[derive(Debug)]
    struct NoReceipts;

    impl ReceiptOracle for NoReceipts {
        fn verified_receipt(&self, _receipt_digest: [u8; 32]) -> Result<ReceiptView, AbiError> {
            Err(AbiError::ReceiptMismatch)
        }
    }

    fn program(byte: u8) -> ProgramId {
        ProgramId::new([byte; 32]).unwrap_or_else(|error| panic!("program: {error}"))
    }

    fn principal(byte: u8) -> PrincipalId {
        PrincipalId::new([byte; 32]).unwrap_or_else(|error| panic!("principal: {error}"))
    }

    fn seeded(order: &[&[u8]]) -> (Storage, StorageNamespace) {
        let namespace = StorageNamespace::principal(program(1), principal(2));
        let mut storage = Storage::new();
        let mut transaction = storage.transaction(namespace);
        for key in order {
            transaction
                .write(key, key)
                .unwrap_or_else(|error| panic!("seed: {error}"));
        }
        let _ = transaction.commit();
        (storage, namespace)
    }

    #[test]
    fn empty_prefix_and_single_entry_return_canonical_entry_without_cursor() {
        let (storage, namespace) = seeded(&[b"only"]);
        let page = storage
            .scan(
                namespace,
                b"",
                b"",
                ScanLimits::new(1, 19).unwrap_or_else(|error| panic!("limits: {error}")),
            )
            .unwrap_or_else(|error| panic!("scan: {error}"));
        assert_eq!(
            page.entries(),
            &[ScanEntry {
                key: b"only".to_vec(),
                value: b"only".to_vec()
            }]
        );
        assert_eq!(page.cursor(), None);
        assert_eq!(page.metered_bytes(), 19);
    }

    #[test]
    fn ceiling_paginates_in_key_order_independent_of_insertion_order() {
        let (left, namespace) = seeded(&[b"c", b"a", b"b"]);
        let (right, _) = seeded(&[b"a", b"b", b"c"]);
        let limits = ScanLimits::new(2, 101).unwrap_or_else(|error| panic!("limits: {error}"));
        let first_left = left
            .scan(namespace, b"", b"", limits)
            .unwrap_or_else(|error| panic!("left: {error}"));
        let first_right = right
            .scan(namespace, b"", b"", limits)
            .unwrap_or_else(|error| panic!("right: {error}"));
        assert_eq!(first_left, first_right);
        assert_eq!(
            first_left
                .entries()
                .iter()
                .map(|entry| entry.key.as_slice())
                .collect::<Vec<_>>(),
            vec![b"a".as_slice(), b"b".as_slice()]
        );
        let cursor = first_left
            .cursor()
            .unwrap_or_else(|| panic!("cursor absent"));
        let second = left
            .scan(namespace, b"", cursor, limits)
            .unwrap_or_else(|error| panic!("resume: {error}"));
        assert_eq!(
            second
                .entries()
                .iter()
                .map(|entry| entry.key.as_slice())
                .collect::<Vec<_>>(),
            vec![b"c".as_slice()]
        );
        assert_eq!(second.cursor(), None);
    }

    #[test]
    fn foreign_cursor_and_nonfitting_entry_are_refused() {
        let (storage, namespace) = seeded(&[b"alpha", b"beta"]);
        let limits = ScanLimits::new(1, 105).unwrap_or_else(|error| panic!("limits: {error}"));
        let page = storage
            .scan(namespace, b"", b"", limits)
            .unwrap_or_else(|error| panic!("page: {error}"));
        let cursor = page.cursor().unwrap_or_else(|| panic!("cursor absent"));
        assert_eq!(
            storage.scan(namespace, b"a", cursor, limits),
            Err(StorageError::InvalidScanCursor)
        );
        assert_eq!(
            storage.scan(
                namespace,
                b"",
                cursor,
                ScanLimits::new(2, 105).unwrap_or_else(|error| panic!("limits: {error}"))
            ),
            Err(StorageError::InvalidScanCursor)
        );
        assert_eq!(
            storage.scan(
                StorageNamespace::principal(program(1), principal(3)),
                b"",
                cursor,
                limits
            ),
            Err(StorageError::InvalidScanCursor)
        );
        assert_eq!(
            storage.scan(namespace, b"", &cursor[..cursor.len() - 1], limits),
            Err(StorageError::InvalidScanCursor)
        );
        assert_eq!(
            storage.scan(
                namespace,
                b"",
                b"",
                ScanLimits::new(1, 104).unwrap_or_else(|error| panic!("limits: {error}"))
            ),
            Err(StorageError::ScanCeilingExceeded)
        );
    }

    #[test]
    fn entry_and_complete_page_byte_ceilings_have_independent_exact_bounds() {
        let (storage, namespace) = seeded(&[b"c", b"a", b"b"]);
        let entry_exact = ScanLimits::new(1, 93).unwrap_or_else(|error| panic!("limits: {error}"));
        let entry_page = storage
            .scan(namespace, b"", b"", entry_exact)
            .unwrap_or_else(|error| panic!("entry page: {error}"));
        assert_eq!(entry_page.entries().len(), 1);
        assert_eq!(entry_page.metered_bytes(), 93);

        let byte_exact = ScanLimits::new(64, 101).unwrap_or_else(|error| panic!("limits: {error}"));
        let byte_page = storage
            .scan(namespace, b"", b"", byte_exact)
            .unwrap_or_else(|error| panic!("byte page: {error}"));
        assert_eq!(byte_page.entries().len(), 2);
        assert_eq!(byte_page.metered_bytes(), 101);

        let byte_one_past =
            ScanLimits::new(64, 100).unwrap_or_else(|error| panic!("limits: {error}"));
        let shorter_page = storage
            .scan(namespace, b"", b"", byte_one_past)
            .unwrap_or_else(|error| panic!("one-past page: {error}"));
        assert_eq!(shorter_page.entries().len(), 1);
        assert_eq!(shorter_page.metered_bytes(), 93);
        assert!(shorter_page.metered_bytes() <= u64::from(byte_one_past.max_bytes()));

        let no_progress = ScanLimits::new(1, 92).unwrap_or_else(|error| panic!("limits: {error}"));
        assert_eq!(
            storage.scan(namespace, b"", b"", no_progress),
            Err(StorageError::ScanCeilingExceeded)
        );
    }

    #[test]
    fn scan_requires_matching_read_authority_meters_full_pages_and_resumes_across_activities() {
        let (storage, _) = seeded(&[b"beta", b"alpha"]);
        let owner = program(1);
        let actor = principal(2);
        let limits = ScanLimits::new(1, 105).unwrap_or_else(|error| panic!("limits: {error}"));
        let mut denied = Abi::new(
            ABI_VERSION,
            owner,
            AuthorizationContext::new(actor, CapabilitySet::empty()),
            storage.clone(),
            &NoReceipts,
        )
        .unwrap_or_else(|error| panic!("denied ABI: {error}"));
        assert_eq!(
            denied.storage_scan_selected(
                &mut Meter::declared(),
                StorageSelector::Principal,
                b"",
                b"",
                limits,
            ),
            Err(AbiError::CapabilityDenied)
        );

        let grants = CapabilitySet::new([Capability::StorageRead])
            .unwrap_or_else(|error| panic!("grant: {error}"));
        let mut first = Abi::new(
            ABI_VERSION,
            owner,
            AuthorizationContext::new(actor, grants.clone()),
            storage,
            &NoReceipts,
        )
        .unwrap_or_else(|error| panic!("first ABI: {error}"));
        let mut first_meter = Meter::new(
            ResourceBudget::new(1, 1, 105, 1, 1, 1),
            FeeSchedule::declared(),
        );
        let first_page = first
            .storage_scan_selected(
                &mut first_meter,
                StorageSelector::Principal,
                b"",
                b"",
                limits,
            )
            .unwrap_or_else(|error| panic!("first page: {error}"));
        assert_eq!(
            first_page.entries(),
            &[ScanEntry {
                key: b"alpha".to_vec(),
                value: b"alpha".to_vec()
            }]
        );
        assert_eq!(first_page.metered_bytes(), 105);
        assert_eq!(
            first_meter
                .finish()
                .map(|usage| (usage.storage_read_bytes, usage.output_bytes)),
            Ok((105, 0))
        );
        let cursor = first_page
            .cursor()
            .unwrap_or_else(|| panic!("cursor absent"))
            .to_vec();
        let committed = first.commit().storage;

        let mut second = Abi::new(
            ABI_VERSION,
            owner,
            AuthorizationContext::new(actor, grants),
            committed,
            &NoReceipts,
        )
        .unwrap_or_else(|error| panic!("second ABI: {error}"));
        let mut second_meter = Meter::new(
            ResourceBudget::new(1, 1, 19, 1, 1, 1),
            FeeSchedule::declared(),
        );
        let second_page = second
            .storage_scan_selected(
                &mut second_meter,
                StorageSelector::Principal,
                b"",
                &cursor,
                limits,
            )
            .unwrap_or_else(|error| panic!("second page: {error}"));
        assert_eq!(
            second_page.entries(),
            &[ScanEntry {
                key: b"beta".to_vec(),
                value: b"beta".to_vec()
            }]
        );
        assert_eq!(second_page.cursor(), None);
        assert_eq!(
            second_meter.finish().map(|usage| usage.storage_read_bytes),
            Ok(19)
        );

        let mut short_meter = Meter::new(
            ResourceBudget::new(1, 1, 104, 1, 1, 1),
            FeeSchedule::declared(),
        );
        assert_eq!(
            second.storage_scan_selected(
                &mut short_meter,
                StorageSelector::Principal,
                b"",
                b"",
                limits,
            ),
            Err(AbiError::Meter(MeterRefusal::BudgetExceeded {
                resource: ResourceKind::StorageRead,
                limit: 104,
                attempted: 105,
            }))
        );
        assert_eq!(
            short_meter.finish().map(|usage| usage.storage_read_bytes),
            Ok(0)
        );
    }

    #[test]
    fn principal_and_shared_scans_require_their_distinct_read_grants() {
        let owner = program(7);
        let actor = principal(8);
        let shared_namespace = StorageNamespace::shared(owner);
        let mut storage = Storage::new();
        let mut transaction = storage.transaction(shared_namespace);
        transaction
            .write(b"shared", b"value")
            .unwrap_or_else(|error| panic!("seed: {error}"));
        let _ = transaction.commit();
        let limits = ScanLimits::new(1, 32).unwrap_or_else(|error| panic!("limits: {error}"));

        let mut principal_only = Abi::new(
            ABI_VERSION,
            owner,
            AuthorizationContext::new(
                actor,
                CapabilitySet::new([Capability::StorageRead])
                    .unwrap_or_else(|error| panic!("grant: {error}")),
            ),
            storage.clone(),
            &NoReceipts,
        )
        .unwrap_or_else(|error| panic!("ABI: {error}"));
        assert_eq!(
            principal_only.storage_scan_selected(
                &mut Meter::declared(),
                StorageSelector::Shared,
                b"",
                b"",
                limits,
            ),
            Err(AbiError::CapabilityDenied)
        );

        let mut shared_only = Abi::new(
            ABI_VERSION,
            owner,
            AuthorizationContext::new(
                actor,
                CapabilitySet::new([Capability::SharedStorageRead])
                    .unwrap_or_else(|error| panic!("grant: {error}")),
            ),
            storage,
            &NoReceipts,
        )
        .unwrap_or_else(|error| panic!("ABI: {error}"));
        let page = shared_only
            .storage_scan_selected(
                &mut Meter::declared(),
                StorageSelector::Shared,
                b"",
                b"",
                limits,
            )
            .unwrap_or_else(|error| panic!("shared scan: {error}"));
        assert_eq!(
            page.entries(),
            &[ScanEntry {
                key: b"shared".to_vec(),
                value: b"value".to_vec()
            }]
        );
    }
}
