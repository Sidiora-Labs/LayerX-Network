//! Canonical program-activity access declarations and enforcement.

use core::fmt::{self, Display};
use std::collections::BTreeSet;

use crate::accounts::{derive_program_account, ProgramAccountError};
use crate::crypto::{hash_bytes, HashAlgorithm};
use crate::storage::{
    PrincipalId, ProgramId, StorageNamespace, MAX_STORAGE_KEY_BYTES,
};

/// Domain separating an access set from every other canonical artifact.
pub const ACCESS_SET_DOMAIN: &[u8] = b"LayerX/programs/access-set/v1\0";
/// Domain separating presence or absence of a declaration from its access set.
pub const ACCESS_DECLARATION_DOMAIN: &[u8] =
    b"LayerX/programs/access-declaration/v1\0";

/// Maximum storage scopes in one explicit access set.
pub const MAX_ACCESS_STORAGE_ENTRIES: usize = 1_024;
/// Maximum account/asset pairs in one explicit access set.
pub const MAX_ACCESS_ACCOUNT_ENTRIES: usize = 512;
/// Maximum callees named by one explicit access set.
pub const MAX_ACCESS_CALLEE_ENTRIES: usize = 512;

/// Maximum canonical byte length of a present or absent access declaration.
pub const MAX_ACCESS_DECLARATION_BYTES: usize = 1_048_576;
/// Maximum access-set bytes transportable inside that declaration envelope.
pub const MAX_ACCESS_SET_BYTES: usize = MAX_ACCESS_DECLARATION_BYTES
    - ACCESS_DECLARATION_DOMAIN.len() - 1 - 4;

/// Charge units added for each account/asset declaration, separately from its
/// canonical encoded bytes.
pub const ACCOUNT_ACCESS_CHARGE_UNITS: u64 = 64;

const PRINCIPAL_NAMESPACE_TAG: u8 = 0;
const SHARED_NAMESPACE_TAG: u8 = 1;
const READ_TAG: u8 = 0;
const WRITE_TAG: u8 = 1;
const EXACT_KEY_TAG: u8 = 0;
const PREFIX_TAG: u8 = 1;
const RANGE_TAG: u8 = 2;
const ABSENT_DECLARATION_TAG: u8 = 0;
const EXPLICIT_DECLARATION_TAG: u8 = 1;

/// Whether an activity observes or mutates a declared resource.
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub enum AccessMode {
    Read,
    Write,
}

impl AccessMode {
    const fn tag(self) -> u8 {
        match self {
            Self::Read => READ_TAG,
            Self::Write => WRITE_TAG,
        }
    }

    fn from_tag(tag: u8) -> Result<Self, AccessRefusal> {
        match tag {
            READ_TAG => Ok(Self::Read),
            WRITE_TAG => Ok(Self::Write),
            _ => Err(AccessRefusal::MalformedCanonicalBytes),
        }
    }
}

impl Display for AccessMode {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Read => formatter.write_str("read"),
            Self::Write => formatter.write_str("write"),
        }
    }
}

/// A bounded part of a namespace's canonical byte-key order.
///
/// Ranges are start-inclusive and end-exclusive. An empty prefix denotes the
/// entire namespace and is the only whole-namespace spelling.
#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub enum KeyAccess {
    Exact(Vec<u8>),
    Prefix(Vec<u8>),
    Range { start: Vec<u8>, end: Vec<u8> },
}

impl KeyAccess {
    /// Declares one exact nonempty storage key.
    ///
    /// # Errors
    ///
    /// Refuses an empty key or one exceeding [`MAX_STORAGE_KEY_BYTES`].
    pub fn exact(key: impl AsRef<[u8]>) -> Result<Self, AccessRefusal> {
        let key = key.as_ref();
        validate_exact_key(key)?;
        Ok(Self::Exact(key.to_vec()))
    }

    /// Declares every key carrying a bounded prefix. The empty prefix declares
    /// the whole namespace.
    ///
    /// # Errors
    ///
    /// Refuses a prefix exceeding [`MAX_STORAGE_KEY_BYTES`].
    pub fn prefix(prefix: impl AsRef<[u8]>) -> Result<Self, AccessRefusal> {
        let prefix = prefix.as_ref();
        validate_key_bound(prefix)?;
        Ok(Self::Prefix(prefix.to_vec()))
    }

    /// Declares a nonempty, half-open interval in canonical byte-key order.
    ///
    /// # Errors
    ///
    /// Refuses empty or oversized bounds and intervals whose start is not
    /// strictly before their end.
    pub fn range(
        start: impl AsRef<[u8]>,
        end: impl AsRef<[u8]>,
    ) -> Result<Self, AccessRefusal> {
        let start = start.as_ref();
        let end = end.as_ref();
        validate_exact_key(start)?;
        validate_exact_key(end)?;
        if start >= end {
            return Err(AccessRefusal::InvalidKeyRange);
        }
        Ok(Self::Range {
            start: start.to_vec(),
            end: end.to_vec(),
        })
    }

    /// Returns the exact key for an exact declaration.
    #[must_use]
    pub fn exact_key(&self) -> Option<&[u8]> {
        match self {
            Self::Exact(key) => Some(key),
            Self::Prefix(_) | Self::Range { .. } => None,
        }
    }

    /// Returns the key prefix for a prefix declaration.
    #[must_use]
    pub fn key_prefix(&self) -> Option<&[u8]> {
        match self {
            Self::Prefix(prefix) => Some(prefix),
            Self::Exact(_) | Self::Range { .. } => None,
        }
    }

    /// Returns the half-open bounds for a range declaration.
    #[must_use]
    pub fn range_bounds(&self) -> Option<(&[u8], &[u8])> {
        match self {
            Self::Range { start, end } => Some((start, end)),
            Self::Exact(_) | Self::Prefix(_) => None,
        }
    }

    fn validate(&self) -> Result<(), AccessRefusal> {
        match self {
            Self::Exact(key) => validate_exact_key(key),
            Self::Prefix(prefix) => validate_key_bound(prefix),
            Self::Range { start, end } => {
                validate_exact_key(start)?;
                validate_exact_key(end)?;
                if start >= end {
                    return Err(AccessRefusal::InvalidKeyRange);
                }
                Ok(())
            }
        }
    }

    fn contains(&self, requested: &Self) -> bool {
        match (self, requested) {
            (Self::Exact(declared), Self::Exact(key)) => declared == key,
            (Self::Prefix(declared), Self::Exact(key)) => key.starts_with(declared),
            (Self::Prefix(declared), Self::Prefix(prefix)) => prefix.starts_with(declared),
            (
                Self::Prefix(declared),
                Self::Range {
                    start: requested_start,
                    end: requested_end,
                },
            ) => range_inside_prefix(declared, requested_start, requested_end),
            (
                Self::Range { start, end },
                Self::Exact(key),
            ) => start <= key && key < end,
            (
                Self::Range { start, end },
                Self::Range {
                    start: requested_start,
                    end: requested_end,
                },
            ) => start <= requested_start && requested_end <= end,
            (Self::Range { start, end }, Self::Prefix(prefix)) => {
                prefix_successor(prefix)
                    .is_some_and(|upper| start.as_slice() <= prefix && upper.as_slice() <= end)
            }
            (Self::Exact(_), Self::Prefix(_) | Self::Range { .. }) => false,
        }
    }

    fn overlaps(&self, other: &Self) -> bool {
        match (self, other) {
            (Self::Exact(left), Self::Exact(right)) => left == right,
            (Self::Exact(key), Self::Prefix(prefix))
            | (Self::Prefix(prefix), Self::Exact(key)) => key.starts_with(prefix),
            (Self::Exact(key), Self::Range { start, end })
            | (Self::Range { start, end }, Self::Exact(key)) => start <= key && key < end,
            (Self::Prefix(left), Self::Prefix(right)) => {
                left.starts_with(right) || right.starts_with(left)
            }
            (Self::Prefix(prefix), Self::Range { start, end })
            | (Self::Range { start, end }, Self::Prefix(prefix)) => {
                prefix_range_overlaps(prefix, start, end)
            }
            (
                Self::Range {
                    start: left_start,
                    end: left_end,
                },
                Self::Range {
                    start: right_start,
                    end: right_end,
                },
            ) => left_start < right_end && right_start < left_end,
        }
    }

    fn charge_units(&self) -> Result<u64, AccessRefusal> {
        match self {
            Self::Exact(_) => Ok(1),
            Self::Prefix(prefix) => {
                let unspecified = MAX_STORAGE_KEY_BYTES
                    .checked_sub(prefix.len())
                    .and_then(|value| value.checked_add(1))
                    .ok_or(AccessRefusal::ChargeOverflow)?;
                u64::try_from(unspecified).map_err(|_| AccessRefusal::ChargeOverflow)
            }
            Self::Range { .. } => u64::try_from(
                MAX_STORAGE_KEY_BYTES
                    .checked_mul(2)
                    .and_then(|value| value.checked_add(1))
                    .ok_or(AccessRefusal::ChargeOverflow)?,
            )
            .map_err(|_| AccessRefusal::ChargeOverflow),
        }
    }
}

/// One read or write commitment over a host-owned storage namespace.
#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub struct StorageAccess {
    namespace: StorageNamespace,
    mode: AccessMode,
    keys: KeyAccess,
}

impl StorageAccess {
    /// Constructs one validated namespace/key access.
    ///
    /// # Errors
    ///
    /// Refuses an invalid exact key, prefix, or range.
    pub fn new(
        namespace: StorageNamespace,
        mode: AccessMode,
        keys: KeyAccess,
    ) -> Result<Self, AccessRefusal> {
        keys.validate()?;
        Ok(Self {
            namespace,
            mode,
            keys,
        })
    }

    #[must_use]
    pub const fn namespace(&self) -> StorageNamespace {
        self.namespace
    }

    #[must_use]
    pub const fn mode(&self) -> AccessMode {
        self.mode
    }

    #[must_use]
    pub const fn keys(&self) -> &KeyAccess {
        &self.keys
    }
}

/// One account/asset pair observed or changed by a program activity.
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub struct AccountAccess {
    account: [u8; 32],
    asset: [u8; 32],
    mode: AccessMode,
}

impl AccountAccess {
    /// Constructs a nonzero account/asset access.
    ///
    /// # Errors
    ///
    /// Refuses the reserved zero account or asset identifier.
    pub fn new(
        account: [u8; 32],
        asset: [u8; 32],
        mode: AccessMode,
    ) -> Result<Self, AccessRefusal> {
        if account == [0; 32] {
            return Err(AccessRefusal::ReservedAccount);
        }
        if asset == [0; 32] {
            return Err(AccessRefusal::ReservedAsset);
        }
        Ok(Self {
            account,
            asset,
            mode,
        })
    }

    #[must_use]
    pub const fn account(&self) -> [u8; 32] {
        self.account
    }

    #[must_use]
    pub const fn asset(&self) -> [u8; 32] {
        self.asset
    }

    #[must_use]
    pub const fn mode(&self) -> AccessMode {
        self.mode
    }
}

/// Closed, bounded and canonically ordered resources committed by an activity.
///
/// Construction rejects exact duplicate scopes. Conservative overlapping
/// scopes remain distinct, charged entries and create scheduler conflicts. The
/// set grants no authority because capabilities are still checked independently.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct AccessSet {
    storage: BTreeSet<StorageAccess>,
    accounts: BTreeSet<AccountAccess>,
    callees: BTreeSet<ProgramId>,
}

impl AccessSet {
    /// Validates and canonically orders storage and account accesses.
    ///
    /// # Errors
    ///
    /// Refuses invalid entries, exact duplicates, or entry counts above the
    /// declared bounds.
    pub fn new(
        storage: impl IntoIterator<Item = StorageAccess>,
        accounts: impl IntoIterator<Item = AccountAccess>,
    ) -> Result<Self, AccessRefusal> {
        let mut storage_entries = Vec::new();
        for access in storage {
            if storage_entries.len() == MAX_ACCESS_STORAGE_ENTRIES {
                return Err(AccessRefusal::TooManyStorageEntries {
                    limit: MAX_ACCESS_STORAGE_ENTRIES,
                });
            }
            access.keys.validate()?;
            storage_entries.push(access);
        }
        storage_entries.sort();
        for window in storage_entries.windows(2) {
            if window[0] == window[1] {
                return Err(AccessRefusal::DuplicateStorageAccess);
            }
        }
        let mut account_entries = Vec::new();
        for access in accounts {
            if account_entries.len() == MAX_ACCESS_ACCOUNT_ENTRIES {
                return Err(AccessRefusal::TooManyAccountEntries {
                    limit: MAX_ACCESS_ACCOUNT_ENTRIES,
                });
            }
            AccountAccess::new(access.account, access.asset, access.mode)?;
            account_entries.push(access);
        }
        account_entries.sort();
        for window in account_entries.windows(2) {
            if window[0] == window[1] {
                return Err(AccessRefusal::DuplicateAccountAccess);
            }
        }

        Ok(Self {
            storage: storage_entries.into_iter().collect(),
            accounts: account_entries.into_iter().collect(),
            callees: BTreeSet::new(),
        })
    }

    /// Constructs a set that also commits every reachable program-call edge.
    ///
    /// # Errors
    ///
    /// Refuses invalid, duplicate, or over-bound resources and callees.
    pub fn new_with_callees(
        storage: impl IntoIterator<Item = StorageAccess>,
        accounts: impl IntoIterator<Item = AccountAccess>,
        callees: impl IntoIterator<Item = ProgramId>,
    ) -> Result<Self, AccessRefusal> {
        let mut set = Self::new(storage, accounts)?;
        for callee in callees {
            if set.callees.len() == MAX_ACCESS_CALLEE_ENTRIES {
                return Err(AccessRefusal::TooManyCalleeEntries { limit: MAX_ACCESS_CALLEE_ENTRIES });
            }
            if !set.callees.insert(callee) {
                return Err(AccessRefusal::DuplicateCalleeAccess);
            }
        }
        Ok(set)
    }

    /// Returns an explicit set that permits no program-state access.
    #[must_use]
    pub const fn empty() -> Self {
        Self {
            storage: BTreeSet::new(),
            accounts: BTreeSet::new(),
            callees: BTreeSet::new(),
        }
    }

    /// Starts an SDK-oriented builder for accesses provable before execution.
    #[must_use]
    pub const fn builder() -> AccessSetBuilder {
        AccessSetBuilder::new()
    }

    #[must_use]
    pub fn storage_accesses(&self) -> impl ExactSizeIterator<Item = &StorageAccess> {
        self.storage.iter()
    }

    #[must_use]
    pub fn account_accesses(&self) -> impl ExactSizeIterator<Item = &AccountAccess> {
        self.accounts.iter()
    }

    #[must_use]
    pub fn storage_len(&self) -> usize {
        self.storage.len()
    }

    #[must_use]
    pub fn account_len(&self) -> usize {
        self.accounts.len()
    }

    #[must_use]
    pub fn callees(&self) -> impl ExactSizeIterator<Item = &ProgramId> { self.callees.iter() }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.storage.is_empty() && self.accounts.is_empty() && self.callees.is_empty()
    }

    /// Canonically encodes the set as sorted namespace accesses followed by
    /// sorted account accesses. All integers are fixed-width big-endian.
    ///
    /// # Errors
    ///
    /// Refuses an entry count, key bound, or total encoding length outside the
    /// frozen limits.
    pub fn canonical_bytes(&self) -> Result<Vec<u8>, AccessRefusal> {
        let storage_count = u16::try_from(self.storage.len())
            .map_err(|_| AccessRefusal::TooManyStorageEntries {
                limit: MAX_ACCESS_STORAGE_ENTRIES,
            })?;
        let account_count = u16::try_from(self.accounts.len())
            .map_err(|_| AccessRefusal::TooManyAccountEntries {
                limit: MAX_ACCESS_ACCOUNT_ENTRIES,
            })?;
        let mut encoded = Vec::new();
        encoded.extend_from_slice(ACCESS_SET_DOMAIN);
        encoded.extend_from_slice(&storage_count.to_be_bytes());
        for access in &self.storage {
            encode_namespace(&mut encoded, access.namespace);
            encoded.push(access.mode.tag());
            encode_key_access(&mut encoded, &access.keys)?;
        }
        encoded.extend_from_slice(&account_count.to_be_bytes());
        for access in &self.accounts {
            encoded.extend_from_slice(&access.account);
            encoded.extend_from_slice(&access.asset);
            encoded.push(access.mode.tag());
        }
        let callee_count = u16::try_from(self.callees.len())
            .map_err(|_| AccessRefusal::TooManyCalleeEntries { limit: MAX_ACCESS_CALLEE_ENTRIES })?;
        encoded.extend_from_slice(&callee_count.to_be_bytes());
        for callee in &self.callees { encoded.extend_from_slice(&callee.bytes()); }
        if encoded.len() > MAX_ACCESS_SET_BYTES {
            return Err(AccessRefusal::EncodingTooLarge {
                length: encoded.len(),
                limit: MAX_ACCESS_SET_BYTES,
            });
        }
        Ok(encoded)
    }

    /// Strictly decodes a canonical set, refusing unknown tags, bad bounds,
    /// duplicate scopes, noncanonical order and trailing bytes.
    ///
    /// # Errors
    ///
    /// Refuses malformed, noncanonical, duplicate, unknown, or over-bound
    /// input.
    pub fn canonical_decode(encoded: &[u8]) -> Result<Self, AccessRefusal> {
        if encoded.len() > MAX_ACCESS_SET_BYTES {
            return Err(AccessRefusal::EncodingTooLarge {
                length: encoded.len(),
                limit: MAX_ACCESS_SET_BYTES,
            });
        }
        let mut cursor = AccessCursor::new(encoded);
        if cursor.take(ACCESS_SET_DOMAIN.len())? != ACCESS_SET_DOMAIN {
            return Err(AccessRefusal::MalformedCanonicalBytes);
        }
        let storage_count = usize::from(cursor.take_u16()?);
        if storage_count > MAX_ACCESS_STORAGE_ENTRIES {
            return Err(AccessRefusal::TooManyStorageEntries {
                limit: MAX_ACCESS_STORAGE_ENTRIES,
            });
        }
        let mut storage = Vec::with_capacity(storage_count);
        for _ in 0..storage_count {
            let namespace = decode_namespace(&mut cursor)?;
            let mode = AccessMode::from_tag(cursor.take_u8()?)?;
            let keys = decode_key_access(&mut cursor)?;
            storage.push(StorageAccess::new(namespace, mode, keys)?);
        }
        let account_count = usize::from(cursor.take_u16()?);
        if account_count > MAX_ACCESS_ACCOUNT_ENTRIES {
            return Err(AccessRefusal::TooManyAccountEntries {
                limit: MAX_ACCESS_ACCOUNT_ENTRIES,
            });
        }
        let mut accounts = Vec::with_capacity(account_count);
        for _ in 0..account_count {
            accounts.push(AccountAccess::new(
                cursor.take_array()?,
                cursor.take_array()?,
                AccessMode::from_tag(cursor.take_u8()?)?,
            )?);
        }
        let callee_count = usize::from(cursor.take_u16()?);
        if callee_count > MAX_ACCESS_CALLEE_ENTRIES {
            return Err(AccessRefusal::TooManyCalleeEntries { limit: MAX_ACCESS_CALLEE_ENTRIES });
        }
        let mut callees = Vec::with_capacity(callee_count);
        for _ in 0..callee_count {
            callees.push(ProgramId::new(cursor.take_array()?)
                .map_err(|_| AccessRefusal::MalformedCanonicalBytes)?);
        }
        if !cursor.is_empty() {
            return Err(AccessRefusal::MalformedCanonicalBytes);
        }
        let decoded = Self::new_with_callees(storage, accounts, callees)?;
        if decoded.canonical_bytes()?.as_slice() != encoded {
            return Err(AccessRefusal::MalformedCanonicalBytes);
        }
        Ok(decoded)
    }

    /// Hashes the exact canonical set bytes for evidence and activity binding.
    ///
    /// # Errors
    ///
    /// Refuses a set whose canonical encoding exceeds the frozen hash-input
    /// bound.
    pub fn commitment(&self) -> Result<[u8; 32], AccessRefusal> {
        let encoded = self.canonical_bytes()?;
        hash_bytes(HashAlgorithm::Sha256, &encoded).map_err(|_| {
            AccessRefusal::EncodingTooLarge {
                length: encoded.len(),
                limit: MAX_ACCESS_SET_BYTES,
            }
        })
    }

    /// Returns the deterministic declaration charge. Encoded bytes, each
    /// account pair and the breadth class of each key scope are charged.
    ///
    /// # Errors
    ///
    /// Refuses an invalid encoding or arithmetic overflow while accumulating
    /// charge units.
    pub fn charge(&self) -> Result<AccessCharge, AccessRefusal> {
        let encoding_bytes = u64::try_from(self.canonical_bytes()?.len())
            .map_err(|_| AccessRefusal::ChargeOverflow)?;
        let storage_units = self.storage.iter().try_fold(0u64, |total, access| {
            total
                .checked_add(access.keys.charge_units()?)
                .ok_or(AccessRefusal::ChargeOverflow)
        })?;
        let account_count = u64::try_from(self.accounts.len())
            .map_err(|_| AccessRefusal::ChargeOverflow)?;
        let account_units = account_count
            .checked_mul(ACCOUNT_ACCESS_CHARGE_UNITS)
            .ok_or(AccessRefusal::ChargeOverflow)?;
        let total_units = encoding_bytes
            .checked_add(storage_units)
            .and_then(|total| total.checked_add(account_units))
            .ok_or(AccessRefusal::ChargeOverflow)?;
        Ok(AccessCharge {
            encoding_bytes,
            storage_units,
            account_units,
            total_units,
        })
    }

    /// Deterministic serializability conflict relation used by the scheduler.
    /// Reads commute; overlapping resources conflict when either side writes.
    #[must_use]
    pub fn conflicts_with(&self, other: &Self) -> bool {
        for left in &self.storage {
            for right in &other.storage {
                if left.namespace == right.namespace
                    && (left.mode == AccessMode::Write || right.mode == AccessMode::Write)
                    && left.keys.overlaps(&right.keys)
                {
                    return true;
                }
            }
        }
        for left in &self.accounts {
            for right in &other.accounts {
                if left.account == right.account
                    && left.asset == right.asset
                    && (left.mode == AccessMode::Write || right.mode == AccessMode::Write)
                {
                    return true;
                }
            }
        }
        false
    }

    fn permits_storage(
        &self,
        namespace: StorageNamespace,
        mode: AccessMode,
        requested: &KeyAccess,
    ) -> bool {
        self.storage.iter().any(|declared| {
            declared.namespace == namespace
                && declared.mode == mode
                && declared.keys.contains(requested)
        })
    }

    fn permits_account(&self, access: AccountAccess) -> bool {
        self.accounts.contains(&access)
    }

    fn permits_call(&self, callee: ProgramId) -> bool { self.callees.contains(&callee) }
}

/// Presence-sensitive activity commitment. Absence means the complete set
/// reachable through the independently enforced capabilities, never an empty
/// set and never an authority widening.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum AccessDeclaration {
    Absent,
    Explicit(AccessSet),
}

impl AccessDeclaration {
    /// Declares the whole capability-reachable set.
    #[must_use]
    pub const fn absent() -> Self {
        Self::Absent
    }

    /// Commits to exactly one validated explicit set.
    #[must_use]
    pub const fn explicit(accesses: AccessSet) -> Self {
        Self::Explicit(accesses)
    }

    /// SDK construction path for access that is a pure function of canonical
    /// calldata. The callback receives no runtime state, so it cannot quietly
    /// claim that a prior-state-dependent access was proved ahead of time.
    pub fn derive_from_calldata(
        calldata: &[u8],
        derive: fn(&[u8], &mut AccessSetBuilder) -> Result<(), AccessRefusal>,
    ) -> Result<Self, AccessRefusal> {
        let mut builder = AccessSetBuilder::new();
        derive(calldata, &mut builder)?;
        builder.build().map(Self::Explicit)
    }

    /// SDK construction path for accesses an interface cannot derive. The
    /// caller must provide a complete builder explicitly; absence remains a
    /// separate whole-reachable-set declaration.
    pub fn explicit_builder(builder: AccessSetBuilder) -> Result<Self, AccessRefusal> {
        builder.build().map(Self::Explicit)
    }

    #[must_use]
    pub const fn explicit_set(&self) -> Option<&AccessSet> {
        match self {
            Self::Absent => None,
            Self::Explicit(accesses) => Some(accesses),
        }
    }

    #[must_use]
    pub const fn is_absent(&self) -> bool {
        matches!(self, Self::Absent)
    }

    /// Resolves absence to the whole set derived from actual capabilities.
    /// Explicit over-declarations remain intact so they are charged and create
    /// conservative scheduler conflicts even when no capability can use them.
    #[must_use]
    pub const fn effective_set<'a>(&'a self, reachable: &'a AccessSet) -> &'a AccessSet {
        match self {
            Self::Absent => reachable,
            Self::Explicit(accesses) => accesses,
        }
    }

    /// Canonical presence encoding that must be included in call-activity bytes
    /// before their digest is computed.
    ///
    /// # Errors
    ///
    /// Refuses an over-bound access set or declaration encoding.
    pub fn canonical_bytes(&self) -> Result<Vec<u8>, AccessRefusal> {
        let mut encoded = Vec::new();
        encoded.extend_from_slice(ACCESS_DECLARATION_DOMAIN);
        match self {
            Self::Absent => encoded.push(ABSENT_DECLARATION_TAG),
            Self::Explicit(accesses) => {
                encoded.push(EXPLICIT_DECLARATION_TAG);
                let access_bytes = accesses.canonical_bytes()?;
                let length = u32::try_from(access_bytes.len()).map_err(|_| {
                    AccessRefusal::EncodingTooLarge {
                        length: access_bytes.len(),
                        limit: MAX_ACCESS_SET_BYTES,
                    }
                })?;
                encoded.extend_from_slice(&length.to_be_bytes());
                encoded.extend_from_slice(&access_bytes);
            }
        }
        if encoded.len() > MAX_ACCESS_DECLARATION_BYTES {
            return Err(AccessRefusal::EncodingTooLarge {
                length: encoded.len(),
                limit: MAX_ACCESS_DECLARATION_BYTES,
            });
        }
        Ok(encoded)
    }

    /// Returns the length-delimited field an activity encoder appends before
    /// hashing. This prevents declaration bytes from being spliced into an
    /// adjacent variable field ambiguously.
    ///
    /// # Errors
    ///
    /// Refuses an over-bound declaration encoding.
    pub fn canonical_activity_field(&self) -> Result<Vec<u8>, AccessRefusal> {
        let declaration = self.canonical_bytes()?;
        let length = u32::try_from(declaration.len()).map_err(|_| {
            AccessRefusal::EncodingTooLarge {
                length: declaration.len(),
                limit: MAX_ACCESS_DECLARATION_BYTES,
            }
        })?;
        let mut field = Vec::with_capacity(4usize.saturating_add(declaration.len()));
        field.extend_from_slice(&length.to_be_bytes());
        field.extend_from_slice(&declaration);
        Ok(field)
    }

    /// Strictly decodes presence and an optional canonical access set.
    ///
    /// # Errors
    ///
    /// Refuses malformed, noncanonical, unknown, duplicate, or over-bound
    /// input.
    pub fn canonical_decode(encoded: &[u8]) -> Result<Self, AccessRefusal> {
        if encoded.len() > MAX_ACCESS_DECLARATION_BYTES {
            return Err(AccessRefusal::EncodingTooLarge {
                length: encoded.len(),
                limit: MAX_ACCESS_DECLARATION_BYTES,
            });
        }
        let mut cursor = AccessCursor::new(encoded);
        if cursor.take(ACCESS_DECLARATION_DOMAIN.len())? != ACCESS_DECLARATION_DOMAIN {
            return Err(AccessRefusal::MalformedCanonicalBytes);
        }
        let declaration = match cursor.take_u8()? {
            ABSENT_DECLARATION_TAG => Self::Absent,
            EXPLICIT_DECLARATION_TAG => {
                let length = usize::try_from(cursor.take_u32()?)
                    .map_err(|_| AccessRefusal::MalformedCanonicalBytes)?;
                if length > MAX_ACCESS_SET_BYTES {
                    return Err(AccessRefusal::EncodingTooLarge {
                        length,
                        limit: MAX_ACCESS_SET_BYTES,
                    });
                }
                Self::Explicit(AccessSet::canonical_decode(cursor.take(length)?)?)
            }
            _ => return Err(AccessRefusal::MalformedCanonicalBytes),
        };
        if !cursor.is_empty() || declaration.canonical_bytes()?.as_slice() != encoded {
            return Err(AccessRefusal::MalformedCanonicalBytes);
        }
        Ok(declaration)
    }

    /// Hashes the exact presence-sensitive declaration bytes.
    ///
    /// # Errors
    ///
    /// Refuses a declaration whose encoding exceeds the frozen hash-input
    /// bound.
    pub fn commitment(&self) -> Result<[u8; 32], AccessRefusal> {
        let encoded = self.canonical_bytes()?;
        hash_bytes(HashAlgorithm::Sha256, &encoded).map_err(|_| {
            AccessRefusal::EncodingTooLarge {
                length: encoded.len(),
                limit: MAX_ACCESS_DECLARATION_BYTES,
            }
        })
    }

    /// Charges an absent declaration as the complete reachable set and an
    /// explicit declaration exactly as written, including conservative excess.
    ///
    /// # Errors
    ///
    /// Refuses an invalid effective encoding or charge arithmetic overflow.
    pub fn charge(&self, reachable: &AccessSet) -> Result<AccessCharge, AccessRefusal> {
        self.effective_set(reachable).charge()
    }

    /// Enforces one exact storage operation. An explicit miss is a typed
    /// refusal and the declaration is never widened from observed execution.
    ///
    /// # Errors
    ///
    /// Refuses an invalid key or an access outside an explicit declaration.
    pub fn enforce_storage_key(
        &self,
        namespace: StorageNamespace,
        mode: AccessMode,
        key: &[u8],
    ) -> Result<(), AccessRefusal> {
        self.enforce_storage(namespace, mode, KeyAccess::exact(key)?)
    }

    /// Enforces a complete prefix scan or whole-namespace operation.
    ///
    /// # Errors
    ///
    /// Refuses an oversized prefix or an access outside an explicit
    /// declaration.
    pub fn enforce_storage_prefix(
        &self,
        namespace: StorageNamespace,
        mode: AccessMode,
        prefix: &[u8],
    ) -> Result<(), AccessRefusal> {
        self.enforce_storage(namespace, mode, KeyAccess::prefix(prefix)?)
    }

    /// Enforces a complete half-open storage range.
    ///
    /// # Errors
    ///
    /// Refuses invalid bounds or an access outside an explicit declaration.
    pub fn enforce_storage_range(
        &self,
        namespace: StorageNamespace,
        mode: AccessMode,
        start: &[u8],
        end: &[u8],
    ) -> Result<(), AccessRefusal> {
        self.enforce_storage(namespace, mode, KeyAccess::range(start, end)?)
    }

    /// Enforces an account/asset observation or mutation.
    ///
    /// # Errors
    ///
    /// Refuses reserved identifiers or an access outside an explicit
    /// declaration.
    pub fn enforce_account(
        &self,
        account: [u8; 32],
        asset: [u8; 32],
        mode: AccessMode,
    ) -> Result<(), AccessRefusal> {
        let requested = AccountAccess::new(account, asset, mode)?;
        match self {
            Self::Absent => Ok(()),
            Self::Explicit(accesses) if accesses.permits_account(requested) => Ok(()),
            Self::Explicit(_) => Err(AccessRefusal::UndeclaredAccount {
                account,
                asset,
                mode,
            }),
        }
    }

    /// Enforces one program-call edge before child instantiation.
    ///
    /// # Errors
    ///
    /// Refuses a callee absent from an explicit declaration.
    pub fn enforce_call(&self, callee: ProgramId) -> Result<(), AccessRefusal> {
        match self {
            Self::Absent => Ok(()),
            Self::Explicit(accesses) if accesses.permits_call(callee) => Ok(()),
            Self::Explicit(_) => Err(AccessRefusal::UndeclaredCall { callee }),
        }
    }

    fn enforce_storage(
        &self,
        namespace: StorageNamespace,
        mode: AccessMode,
        requested: KeyAccess,
    ) -> Result<(), AccessRefusal> {
        match self {
            Self::Absent => Ok(()),
            Self::Explicit(accesses)
                if accesses.permits_storage(namespace, mode, &requested) =>
            {
                Ok(())
            }
            Self::Explicit(_) => Err(AccessRefusal::UndeclaredStorage {
                namespace,
                mode,
                requested,
            }),
        }
    }

    /// Conservative conflict check requiring no external reachability facts.
    /// Any absent declaration conflicts because its whole reachable set is not
    /// represented here; schedulers with derived sets should use
    /// [`Self::conflicts_with_resolved`].
    #[must_use]
    pub fn conflicts_with(&self, other: &Self) -> bool {
        match (self, other) {
            (Self::Explicit(left), Self::Explicit(right)) => left.conflicts_with(right),
            (Self::Absent, _) | (_, Self::Absent) => true,
        }
    }

    /// Exact deterministic conflict check after resolving absent declarations
    /// to each activity's complete capability-reachable set.
    #[must_use]
    pub fn conflicts_with_resolved(
        &self,
        self_reachable: &AccessSet,
        other: &Self,
        other_reachable: &AccessSet,
    ) -> bool {
        self.effective_set(self_reachable)
            .conflicts_with(other.effective_set(other_reachable))
    }
}

impl Default for AccessDeclaration {
    fn default() -> Self {
        Self::Absent
    }
}

/// Deterministic access-list charge components for the governed fee layer.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct AccessCharge {
    encoding_bytes: u64,
    storage_units: u64,
    account_units: u64,
    total_units: u64,
}

impl AccessCharge {
    #[must_use]
    pub const fn encoding_bytes(self) -> u64 {
        self.encoding_bytes
    }

    #[must_use]
    pub const fn storage_units(self) -> u64 {
        self.storage_units
    }

    #[must_use]
    pub const fn account_units(self) -> u64 {
        self.account_units
    }

    #[must_use]
    pub const fn total_units(self) -> u64 {
        self.total_units
    }

    /// Applies a governed integer price without introducing a compiled fee.
    ///
    /// # Errors
    ///
    /// Refuses multiplication overflow.
    pub fn fee_units(self, price_per_unit: u64) -> Result<u128, AccessRefusal> {
        u128::from(self.total_units)
            .checked_mul(u128::from(price_per_unit))
            .ok_or(AccessRefusal::ChargeOverflow)
    }
}

/// Builder used by SDKs and activity compilers when access is provable from
/// the call shape, calldata or a published interface.
#[derive(Clone, Debug, Default)]
pub struct AccessSetBuilder {
    storage: Vec<StorageAccess>,
    accounts: Vec<AccountAccess>,
    callees: Vec<ProgramId>,
}

impl AccessSetBuilder {
    #[must_use]
    pub const fn new() -> Self {
        Self {
            storage: Vec::new(),
            accounts: Vec::new(),
            callees: Vec::new(),
        }
    }

    /// Adds an exact namespace read.
    ///
    /// # Errors
    ///
    /// Refuses an invalid key or a builder above the storage-entry bound.
    pub fn read_key(
        &mut self,
        namespace: StorageNamespace,
        key: impl AsRef<[u8]>,
    ) -> Result<&mut Self, AccessRefusal> {
        self.push_storage(StorageAccess::new(
            namespace,
            AccessMode::Read,
            KeyAccess::exact(key)?,
        )?)
    }

    /// Adds an exact namespace write.
    ///
    /// # Errors
    ///
    /// Refuses an invalid key or a builder above the storage-entry bound.
    pub fn write_key(
        &mut self,
        namespace: StorageNamespace,
        key: impl AsRef<[u8]>,
    ) -> Result<&mut Self, AccessRefusal> {
        self.push_storage(StorageAccess::new(
            namespace,
            AccessMode::Write,
            KeyAccess::exact(key)?,
        )?)
    }

    /// Adds a bounded namespace-prefix read.
    ///
    /// # Errors
    ///
    /// Refuses an oversized prefix or a builder above the storage-entry bound.
    pub fn read_prefix(
        &mut self,
        namespace: StorageNamespace,
        prefix: impl AsRef<[u8]>,
    ) -> Result<&mut Self, AccessRefusal> {
        self.push_storage(StorageAccess::new(
            namespace,
            AccessMode::Read,
            KeyAccess::prefix(prefix)?,
        )?)
    }

    /// Adds a bounded namespace-prefix write.
    ///
    /// # Errors
    ///
    /// Refuses an oversized prefix or a builder above the storage-entry bound.
    pub fn write_prefix(
        &mut self,
        namespace: StorageNamespace,
        prefix: impl AsRef<[u8]>,
    ) -> Result<&mut Self, AccessRefusal> {
        self.push_storage(StorageAccess::new(
            namespace,
            AccessMode::Write,
            KeyAccess::prefix(prefix)?,
        )?)
    }

    /// Adds a half-open namespace-range read.
    ///
    /// # Errors
    ///
    /// Refuses invalid bounds or a builder above the storage-entry bound.
    pub fn read_range(
        &mut self,
        namespace: StorageNamespace,
        start: impl AsRef<[u8]>,
        end: impl AsRef<[u8]>,
    ) -> Result<&mut Self, AccessRefusal> {
        self.push_storage(StorageAccess::new(
            namespace,
            AccessMode::Read,
            KeyAccess::range(start, end)?,
        )?)
    }

    /// Adds a half-open namespace-range write.
    ///
    /// # Errors
    ///
    /// Refuses invalid bounds or a builder above the storage-entry bound.
    pub fn write_range(
        &mut self,
        namespace: StorageNamespace,
        start: impl AsRef<[u8]>,
        end: impl AsRef<[u8]>,
    ) -> Result<&mut Self, AccessRefusal> {
        self.push_storage(StorageAccess::new(
            namespace,
            AccessMode::Write,
            KeyAccess::range(start, end)?,
        )?)
    }

    /// Adds a whole-namespace read.
    ///
    /// # Errors
    ///
    /// Refuses a builder above the storage-entry bound.
    pub fn read_namespace(
        &mut self,
        namespace: StorageNamespace,
    ) -> Result<&mut Self, AccessRefusal> {
        self.read_prefix(namespace, [])
    }

    /// Adds a whole-namespace write.
    ///
    /// # Errors
    ///
    /// Refuses a builder above the storage-entry bound.
    pub fn write_namespace(
        &mut self,
        namespace: StorageNamespace,
    ) -> Result<&mut Self, AccessRefusal> {
        self.write_prefix(namespace, [])
    }

    /// Adds an exact principal-scoped read derived from public identifiers.
    ///
    /// # Errors
    ///
    /// Refuses an invalid key or a builder above the storage-entry bound.
    pub fn read_principal_key(
        &mut self,
        program: ProgramId,
        principal: PrincipalId,
        key: impl AsRef<[u8]>,
    ) -> Result<&mut Self, AccessRefusal> {
        self.read_key(StorageNamespace::principal(program, principal), key)
    }

    /// Adds an exact principal-scoped write derived from public identifiers.
    ///
    /// # Errors
    ///
    /// Refuses an invalid key or a builder above the storage-entry bound.
    pub fn write_principal_key(
        &mut self,
        program: ProgramId,
        principal: PrincipalId,
        key: impl AsRef<[u8]>,
    ) -> Result<&mut Self, AccessRefusal> {
        self.write_key(StorageNamespace::principal(program, principal), key)
    }

    /// Adds an exact shared-state read derived from a program identifier.
    ///
    /// # Errors
    ///
    /// Refuses an invalid key or a builder above the storage-entry bound.
    pub fn read_shared_key(
        &mut self,
        program: ProgramId,
        key: impl AsRef<[u8]>,
    ) -> Result<&mut Self, AccessRefusal> {
        self.read_key(StorageNamespace::shared(program), key)
    }

    /// Adds an exact shared-state write derived from a program identifier.
    ///
    /// # Errors
    ///
    /// Refuses an invalid key or a builder above the storage-entry bound.
    pub fn write_shared_key(
        &mut self,
        program: ProgramId,
        key: impl AsRef<[u8]>,
    ) -> Result<&mut Self, AccessRefusal> {
        self.write_key(StorageNamespace::shared(program), key)
    }

    /// Declares receipt-verified visibility of one exact account/asset pair.
    ///
    /// # Errors
    ///
    /// Refuses reserved identifiers or a builder above the account-entry bound.
    pub fn visible_account(
        &mut self,
        account: [u8; 32],
        asset: [u8; 32],
    ) -> Result<&mut Self, AccessRefusal> {
        self.push_account(AccountAccess::new(account, asset, AccessMode::Read)?)
    }

    /// Declares mutation of one exact account/asset pair by the kernel transfer
    /// boundary. It does not grant spending authority.
    ///
    /// # Errors
    ///
    /// Refuses reserved identifiers or a builder above the account-entry bound.
    pub fn write_account(
        &mut self,
        account: [u8; 32],
        asset: [u8; 32],
    ) -> Result<&mut Self, AccessRefusal> {
        self.push_account(AccountAccess::new(account, asset, AccessMode::Write)?)
    }

    /// Declares visibility of a principal's account for one asset.
    ///
    /// # Errors
    ///
    /// Refuses a reserved asset or a builder above the account-entry bound.
    pub fn visible_principal_account(
        &mut self,
        principal: PrincipalId,
        asset: [u8; 32],
    ) -> Result<&mut Self, AccessRefusal> {
        self.visible_account(principal.bytes(), asset)
    }

    /// Derives and declares a visible program-owned account from public inputs.
    ///
    /// # Errors
    ///
    /// Refuses an invalid derivation, reserved asset, or builder above the
    /// account-entry bound.
    pub fn visible_program_account(
        &mut self,
        program: ProgramId,
        seed: &[u8],
        asset: [u8; 32],
    ) -> Result<&mut Self, AccessRefusal> {
        let account = derive_access_account(program, seed)?;
        self.visible_account(account, asset)
    }

    /// Derives and declares a mutated program-owned account from public inputs.
    ///
    /// # Errors
    ///
    /// Refuses an invalid derivation, reserved asset, or builder above the
    /// account-entry bound.
    pub fn write_program_account(
        &mut self,
        program: ProgramId,
        seed: &[u8],
        asset: [u8; 32],
    ) -> Result<&mut Self, AccessRefusal> {
        let account = derive_access_account(program, seed)?;
        self.write_account(account, asset)
    }

    /// Declares one reachable program-call edge.
    ///
    /// # Errors
    ///
    /// Refuses a builder above the callee-entry bound.
    pub fn call(&mut self, callee: ProgramId) -> Result<&mut Self, AccessRefusal> {
        if self.callees.len() == MAX_ACCESS_CALLEE_ENTRIES {
            return Err(AccessRefusal::TooManyCalleeEntries { limit: MAX_ACCESS_CALLEE_ENTRIES });
        }
        self.callees.push(callee);
        Ok(self)
    }

    /// Produces the validated, canonically ordered commitment.
    ///
    /// # Errors
    ///
    /// Refuses invalid entries, exact duplicates, or an over-bound set.
    pub fn build(self) -> Result<AccessSet, AccessRefusal> {
        AccessSet::new_with_callees(self.storage, self.accounts, self.callees)
    }

    fn push_storage(&mut self, access: StorageAccess) -> Result<&mut Self, AccessRefusal> {
        if self.storage.len() == MAX_ACCESS_STORAGE_ENTRIES {
            return Err(AccessRefusal::TooManyStorageEntries {
                limit: MAX_ACCESS_STORAGE_ENTRIES,
            });
        }
        self.storage.push(access);
        Ok(self)
    }

    fn push_account(&mut self, access: AccountAccess) -> Result<&mut Self, AccessRefusal> {
        if self.accounts.len() == MAX_ACCESS_ACCOUNT_ENTRIES {
            return Err(AccessRefusal::TooManyAccountEntries {
                limit: MAX_ACCESS_ACCOUNT_ENTRIES,
            });
        }
        self.accounts.push(access);
        Ok(self)
    }
}

/// Closed construction, decoding, charging and execution-refusal taxonomy.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum AccessRefusal {
    EmptyKey,
    KeyTooLarge { length: usize, limit: usize },
    InvalidKeyRange,
    ReservedAccount,
    ReservedAsset,
    ProgramAccountSeedTooLarge { length: usize, limit: usize },
    ProgramAccountDerivation,
    TooManyStorageEntries { limit: usize },
    TooManyAccountEntries { limit: usize },
    TooManyCalleeEntries { limit: usize },
    DuplicateStorageAccess,
    DuplicateAccountAccess,
    DuplicateCalleeAccess,
    EncodingTooLarge { length: usize, limit: usize },
    MalformedCanonicalBytes,
    ChargeOverflow,
    UndeclaredStorage {
        namespace: StorageNamespace,
        mode: AccessMode,
        requested: KeyAccess,
    },
    UndeclaredAccount {
        account: [u8; 32],
        asset: [u8; 32],
        mode: AccessMode,
    },
    UndeclaredCall { callee: ProgramId },
}

impl Display for AccessRefusal {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::EmptyKey => formatter.write_str("storage key is empty"),
            Self::KeyTooLarge { length, limit } => {
                write!(formatter, "storage key length {length} exceeds limit {limit}")
            }
            Self::InvalidKeyRange => formatter.write_str("storage key range is empty or reversed"),
            Self::ReservedAccount => formatter.write_str("account identifier is reserved"),
            Self::ReservedAsset => formatter.write_str("asset identifier is reserved"),
            Self::ProgramAccountSeedTooLarge { length, limit } => write!(
                formatter,
                "program account seed length {length} exceeds limit {limit}"
            ),
            Self::ProgramAccountDerivation => {
                formatter.write_str("program account cannot be derived")
            }
            Self::TooManyStorageEntries { limit } => {
                write!(formatter, "access set exceeds {limit} storage entries")
            }
            Self::TooManyAccountEntries { limit } => {
                write!(formatter, "access set exceeds {limit} account entries")
            }
            Self::TooManyCalleeEntries { limit } => write!(formatter, "access set exceeds {limit} callee entries"),
            Self::DuplicateStorageAccess => {
                formatter.write_str("storage access is declared twice")
            }
            Self::DuplicateAccountAccess => {
                formatter.write_str("account access is declared twice")
            }
            Self::DuplicateCalleeAccess => formatter.write_str("callee access is declared twice"),
            Self::EncodingTooLarge { length, limit } => write!(
                formatter,
                "access encoding length {length} exceeds limit {limit}"
            ),
            Self::MalformedCanonicalBytes => {
                formatter.write_str("access declaration bytes are not canonical")
            }
            Self::ChargeOverflow => formatter.write_str("access declaration charge overflowed"),
            Self::UndeclaredStorage {
                namespace, mode, ..
            } => write!(
                formatter,
                "{mode} access falls outside the declaration for namespace {:?}",
                namespace
            ),
            Self::UndeclaredAccount { mode, .. } => {
                write!(formatter, "account {mode} falls outside the declaration")
            }
            Self::UndeclaredCall { callee } => write!(formatter, "call to program {:?} falls outside the declaration", callee),
        }
    }
}

impl std::error::Error for AccessRefusal {}

fn validate_exact_key(key: &[u8]) -> Result<(), AccessRefusal> {
    if key.is_empty() {
        return Err(AccessRefusal::EmptyKey);
    }
    validate_key_bound(key)
}

fn validate_key_bound(key: &[u8]) -> Result<(), AccessRefusal> {
    if key.len() > MAX_STORAGE_KEY_BYTES {
        return Err(AccessRefusal::KeyTooLarge {
            length: key.len(),
            limit: MAX_STORAGE_KEY_BYTES,
        });
    }
    Ok(())
}

fn prefix_successor(prefix: &[u8]) -> Option<Vec<u8>> {
    let index = prefix.iter().rposition(|byte| *byte != u8::MAX)?;
    let mut successor = prefix[..=index].to_vec();
    successor[index] = successor[index].saturating_add(1);
    Some(successor)
}

fn range_inside_prefix(prefix: &[u8], start: &[u8], end: &[u8]) -> bool {
    if prefix.is_empty() {
        return true;
    }
    prefix <= start
        && prefix_successor(prefix).is_none_or(|upper| end <= upper.as_slice())
}

fn prefix_range_overlaps(prefix: &[u8], start: &[u8], end: &[u8]) -> bool {
    prefix < end
        && prefix_successor(prefix).is_none_or(|upper| start < upper.as_slice())
}

fn encode_namespace(encoded: &mut Vec<u8>, namespace: StorageNamespace) {
    encoded.extend_from_slice(&namespace.program().bytes());
    match namespace {
        StorageNamespace::PrincipalScoped { principal, .. } => {
            encoded.push(PRINCIPAL_NAMESPACE_TAG);
            encoded.extend_from_slice(&principal.bytes());
        }
        StorageNamespace::ProgramShared { .. } => encoded.push(SHARED_NAMESPACE_TAG),
        StorageNamespace::ProtocolPrivate { scope, .. } => {
            encoded.push(2);
            encoded.extend_from_slice(&scope);
        }
    }
}

fn encode_key_access(encoded: &mut Vec<u8>, keys: &KeyAccess) -> Result<(), AccessRefusal> {
    match keys {
        KeyAccess::Exact(key) => {
            encoded.push(EXACT_KEY_TAG);
            encode_key(encoded, key)?;
        }
        KeyAccess::Prefix(prefix) => {
            encoded.push(PREFIX_TAG);
            encode_key(encoded, prefix)?;
        }
        KeyAccess::Range { start, end } => {
            encoded.push(RANGE_TAG);
            encode_key(encoded, start)?;
            encode_key(encoded, end)?;
        }
    }
    Ok(())
}

fn encode_key(encoded: &mut Vec<u8>, key: &[u8]) -> Result<(), AccessRefusal> {
    let length = u16::try_from(key.len()).map_err(|_| AccessRefusal::KeyTooLarge {
        length: key.len(),
        limit: MAX_STORAGE_KEY_BYTES,
    })?;
    encoded.extend_from_slice(&length.to_be_bytes());
    encoded.extend_from_slice(key);
    Ok(())
}

fn decode_namespace(cursor: &mut AccessCursor<'_>) -> Result<StorageNamespace, AccessRefusal> {
    let program = ProgramId::new(cursor.take_array()?)
        .map_err(|_| AccessRefusal::MalformedCanonicalBytes)?;
    match cursor.take_u8()? {
        PRINCIPAL_NAMESPACE_TAG => {
            let principal = PrincipalId::new(cursor.take_array()?)
                .map_err(|_| AccessRefusal::MalformedCanonicalBytes)?;
            Ok(StorageNamespace::principal(program, principal))
        }
        SHARED_NAMESPACE_TAG => Ok(StorageNamespace::shared(program)),
        2 => Ok(StorageNamespace::protocol_private(program, cursor.take_array()?)),
        _ => Err(AccessRefusal::MalformedCanonicalBytes),
    }
}

fn decode_key_access(cursor: &mut AccessCursor<'_>) -> Result<KeyAccess, AccessRefusal> {
    match cursor.take_u8()? {
        EXACT_KEY_TAG => KeyAccess::exact(cursor.take_key()?),
        PREFIX_TAG => KeyAccess::prefix(cursor.take_key()?),
        RANGE_TAG => {
            let start = cursor.take_key()?.to_vec();
            let end = cursor.take_key()?.to_vec();
            KeyAccess::range(start, end)
        }
        _ => Err(AccessRefusal::MalformedCanonicalBytes),
    }
}

fn derive_access_account(
    program: ProgramId,
    seed: &[u8],
) -> Result<[u8; 32], AccessRefusal> {
    derive_program_account(program, seed)
        .map(|account| account.bytes())
        .map_err(|error| match error {
            ProgramAccountError::SeedTooLarge { length, limit } => {
                AccessRefusal::ProgramAccountSeedTooLarge { length, limit }
            }
            ProgramAccountError::PreimageTooLarge => AccessRefusal::ProgramAccountDerivation,
        })
}

struct AccessCursor<'a> {
    remaining: &'a [u8],
}

impl<'a> AccessCursor<'a> {
    const fn new(remaining: &'a [u8]) -> Self {
        Self { remaining }
    }

    fn take(&mut self, length: usize) -> Result<&'a [u8], AccessRefusal> {
        let (value, remaining) = self
            .remaining
            .split_at_checked(length)
            .ok_or(AccessRefusal::MalformedCanonicalBytes)?;
        self.remaining = remaining;
        Ok(value)
    }

    fn take_u8(&mut self) -> Result<u8, AccessRefusal> {
        self.take(1)?
            .first()
            .copied()
            .ok_or(AccessRefusal::MalformedCanonicalBytes)
    }

    fn take_u16(&mut self) -> Result<u16, AccessRefusal> {
        Ok(u16::from_be_bytes(self.take_array()?))
    }

    fn take_u32(&mut self) -> Result<u32, AccessRefusal> {
        Ok(u32::from_be_bytes(self.take_array()?))
    }

    fn take_array<const N: usize>(&mut self) -> Result<[u8; N], AccessRefusal> {
        self.take(N)?
            .try_into()
            .map_err(|_| AccessRefusal::MalformedCanonicalBytes)
    }

    fn take_key(&mut self) -> Result<&'a [u8], AccessRefusal> {
        let length = usize::from(self.take_u16()?);
        if length > MAX_STORAGE_KEY_BYTES {
            return Err(AccessRefusal::KeyTooLarge {
                length,
                limit: MAX_STORAGE_KEY_BYTES,
            });
        }
        self.take(length)
    }

    const fn is_empty(&self) -> bool {
        self.remaining.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::abi::{Abi, AbiError, AuthorizationContext, Capability, CapabilitySet, ReceiptOracle, ReceiptView};
    use crate::{FeeSchedule, Meter, ResourceBudget, Storage};

    struct NoReceipts;
    impl ReceiptOracle for NoReceipts {
        fn verified_receipt(&self, _receipt_digest: [u8; 32]) -> Result<ReceiptView, AbiError> {
            Err(AbiError::ReceiptMismatch)
        }
    }

    fn program(byte: u8) -> ProgramId {
        ProgramId::new([byte; 32]).expect("program")
    }

    fn principal(byte: u8) -> PrincipalId {
        PrincipalId::new([byte; 32]).expect("principal")
    }

    fn namespace() -> StorageNamespace {
        StorageNamespace::principal(program(1), principal(2))
    }

    #[test]
    fn canonical_encoding_is_independent_of_builder_order_and_strictly_decodes() {
        let mut left = AccessSet::builder();
        left.visible_account([9; 32], [8; 32])
            .expect("account")
            .write_key(namespace(), b"z")
            .expect("z")
            .read_key(namespace(), b"a")
            .expect("a");
        let mut right = AccessSet::builder();
        right
            .read_key(namespace(), b"a")
            .expect("a")
            .write_key(namespace(), b"z")
            .expect("z")
            .visible_account([9; 32], [8; 32])
            .expect("account");
        let left = left.build().expect("left");
        let right = right.build().expect("right");
        assert_eq!(left, right);
        let encoded = left.canonical_bytes().expect("encode");
        assert_eq!(AccessSet::canonical_decode(&encoded), Ok(left));

        let mut trailing = encoded;
        trailing.push(0);
        assert_eq!(
            AccessSet::canonical_decode(&trailing),
            Err(AccessRefusal::MalformedCanonicalBytes)
        );
    }

    #[test]
    fn frozen_empty_and_absent_encodings_match_protocol_bytes() {
        let mut empty = b"LayerX/programs/access-set/v1\0".to_vec();
        empty.extend_from_slice(&[0, 0, 0, 0, 0, 0]);
        assert_eq!(AccessSet::empty().canonical_bytes().expect("empty"), empty);
        let mut absent = b"LayerX/programs/access-declaration/v1\0".to_vec();
        absent.push(0);
        assert_eq!(AccessDeclaration::absent().canonical_bytes().expect("absent"), absent);
    }

    #[test]
    fn explicit_declaration_refuses_every_access_outside_its_exact_commitment() {
        let mut builder = AccessSet::builder();
        builder
            .read_prefix(namespace(), b"orders/")
            .expect("prefix")
            .write_key(namespace(), b"total")
            .expect("write")
            .visible_account([3; 32], [4; 32])
            .expect("account");
        let declaration = AccessDeclaration::explicit(builder.build().expect("set"));
        assert!(matches!(
            declaration.enforce_call(program(9)),
            Err(AccessRefusal::UndeclaredCall { .. })
        ));

        assert!(declaration
            .enforce_storage_key(namespace(), AccessMode::Read, b"orders/7")
            .is_ok());
        assert!(matches!(
            declaration.enforce_storage_key(namespace(), AccessMode::Read, b"private/7"),
            Err(AccessRefusal::UndeclaredStorage { .. })
        ));
        assert!(matches!(
            declaration.enforce_storage_key(namespace(), AccessMode::Write, b"orders/7"),
            Err(AccessRefusal::UndeclaredStorage { .. })
        ));
        assert!(declaration
            .enforce_account([3; 32], [4; 32], AccessMode::Read)
            .is_ok());
        assert!(matches!(
            declaration.enforce_account([3; 32], [4; 32], AccessMode::Write),
            Err(AccessRefusal::UndeclaredAccount { .. })
        ));
    }

    #[test]
    fn absent_means_whole_reachable_set_for_charge_and_resolved_conflicts() {
        let mut reachable_builder = AccessSet::builder();
        reachable_builder
            .write_key(namespace(), b"state")
            .expect("reachable");
        let reachable = reachable_builder.build().expect("reachable set");
        let absent = AccessDeclaration::absent();
        assert!(absent
            .enforce_storage_key(namespace(), AccessMode::Write, b"state")
            .is_ok());
        assert_eq!(absent.charge(&reachable), reachable.charge());

        let mut reader_builder = AccessSet::builder();
        reader_builder
            .read_key(namespace(), b"state")
            .expect("reader");
        let reader = AccessDeclaration::explicit(reader_builder.build().expect("reader set"));
        assert!(absent.conflicts_with(&reader));
        assert!(absent.conflicts_with_resolved(
            &reachable,
            &reader,
            &AccessSet::empty()
        ));
    }

    #[test]
    fn conflicts_are_symmetric_and_only_write_overlap_conflicts() {
        let declaration = |mode: AccessMode, key: &[u8]| {
            AccessSet::new(
                [StorageAccess::new(namespace(), mode, KeyAccess::exact(key).expect("key"))
                    .expect("access")],
                [],
            )
            .expect("set")
        };
        let read = declaration(AccessMode::Read, b"same");
        let write = declaration(AccessMode::Write, b"same");
        let disjoint = declaration(AccessMode::Write, b"other");
        assert!(!read.conflicts_with(&read));
        assert!(read.conflicts_with(&write));
        assert!(write.conflicts_with(&read));
        assert!(!read.conflicts_with(&disjoint));
    }

    #[test]
    fn overlapping_same_mode_scopes_remain_distinct_and_charged() {
        let accesses = [
            StorageAccess::new(
                namespace(),
                AccessMode::Read,
                KeyAccess::prefix(b"orders/").expect("prefix"),
            )
            .expect("prefix access"),
            StorageAccess::new(
                namespace(),
                AccessMode::Read,
                KeyAccess::exact(b"orders/1").expect("key"),
            )
            .expect("key access"),
        ];
        let overdeclared = AccessSet::new(accesses, []).expect("overdeclared set");
        let prefix_only = AccessSet::new(
            [StorageAccess::new(
                namespace(),
                AccessMode::Read,
                KeyAccess::prefix(b"orders/").expect("prefix"),
            )
            .expect("prefix access")],
            [],
        )
        .expect("prefix set");

        assert_eq!(overdeclared.storage_len(), 2);
        assert!(
            overdeclared.charge().expect("overdeclared charge").total_units()
                > prefix_only.charge().expect("prefix charge").total_units()
        );
    }

    #[test]
    fn broad_and_extra_declarations_are_deterministically_charged() {
        let exact = AccessSet::new(
            [StorageAccess::new(
                namespace(),
                AccessMode::Read,
                KeyAccess::exact(b"a").expect("key"),
            )
            .expect("access")],
            [],
        )
        .expect("exact");
        let broad = AccessSet::new(
            [StorageAccess::new(
                namespace(),
                AccessMode::Read,
                KeyAccess::prefix([]).expect("whole namespace"),
            )
            .expect("access")],
            [],
        )
        .expect("broad");
        assert!(broad.charge().expect("broad charge").total_units()
            > exact.charge().expect("exact charge").total_units());
        assert_eq!(broad.charge(), broad.charge());
    }

    #[test]
    fn program_account_builder_uses_the_canonical_public_derivation() {
        let owner = program(7);
        let account = derive_program_account(owner, b"vault")
            .expect("derive")
            .bytes();
        let mut builder = AccessSet::builder();
        builder
            .visible_program_account(owner, b"vault", [6; 32])
            .expect("visible account");
        let set = builder.build().expect("set");
        assert_eq!(
            set.account_accesses().next().map(AccountAccess::account),
            Some(account)
        );
    }

    #[test]
    fn declaration_encoding_distinguishes_absence_from_an_explicit_empty_set() {
        let absent = AccessDeclaration::absent();
        let empty = AccessDeclaration::explicit(AccessSet::empty());
        assert_ne!(
            absent.canonical_bytes().expect("absent"),
            empty.canonical_bytes().expect("empty")
        );
        for declaration in [absent, empty] {
            let encoded = declaration.canonical_bytes().expect("encode");
            assert_eq!(
                AccessDeclaration::canonical_decode(&encoded),
                Ok(declaration)
            );
        }
    }

    #[test]
    fn call_activity_field_covers_presence_and_exact_declaration_bytes() {
        let absent = AccessDeclaration::absent();
        let mut builder = AccessSet::builder();
        builder.read_key(namespace(), b"from-calldata").expect("key");
        let explicit = AccessDeclaration::explicit(builder.build().expect("set"));
        assert_ne!(
            absent.canonical_activity_field().expect("absent field"),
            explicit.canonical_activity_field().expect("explicit field")
        );
    }

    #[test]
    fn calldata_derived_declaration_is_exact_and_prior_state_cannot_widen_it() {
        let declaration = AccessDeclaration::derive_from_calldata(
            b"orders/7",
            |calldata, builder| builder.read_key(namespace(), calldata).map(|_| ()),
        ).expect("derive");
        assert!(declaration
            .enforce_storage_key(namespace(), AccessMode::Read, b"orders/7")
            .is_ok());
        assert!(matches!(
            declaration.enforce_storage_key(namespace(), AccessMode::Read, b"orders/8"),
            Err(AccessRefusal::UndeclaredStorage { .. })
        ));
    }

    #[test]
    fn callee_behaviour_is_checked_against_the_root_activity_declaration() {
        let root = StorageNamespace::principal(program(1), principal(2));
        let callee = StorageNamespace::shared(program(9));
        let mut builder = AccessSet::builder();
        builder.read_key(root, b"route").expect("root route");
        let declaration = AccessDeclaration::explicit(builder.build().expect("set"));
        assert!(matches!(
            declaration.enforce_storage_key(callee, AccessMode::Write, b"callee-state"),
            Err(AccessRefusal::UndeclaredStorage { .. })
        ));

        let mut conservative = AccessSet::builder();
        conservative
            .read_key(root, b"route").expect("route")
            .write_namespace(callee).expect("callee namespace")
            .call(program(9)).expect("callee");
        let conservative = AccessDeclaration::explicit(conservative.build().expect("set"));
        assert!(conservative
            .enforce_storage_key(callee, AccessMode::Write, b"callee-state")
            .is_ok());
        assert!(conservative.enforce_call(program(9)).is_ok());
        assert!(conservative
            .charge(&AccessSet::empty()).expect("charge").total_units()
            > declaration.charge(&AccessSet::empty()).expect("charge").total_units());
    }

    fn authorized_abi(
        owner: ProgramId,
        actor: PrincipalId,
        capabilities: CapabilitySet,
        storage: Storage,
        declaration: AccessDeclaration,
    ) -> Abi {
        let mut abi = Abi::new(
            crate::ABI_V2_VERSION,
            owner,
            AuthorizationContext::new(actor, capabilities),
            storage,
            &NoReceipts,
        ).expect("authorized ABI");
        abi.set_access_declaration(declaration);
        abi
    }

    fn activity_meter() -> Meter {
        Meter::new_activity(ResourceBudget::declared(), FeeSchedule::declared())
    }

    #[test]
    fn actual_abi_storage_refuses_a_calldata_selected_undeclared_key() {
        let owner = program(1);
        let actor = principal(2);
        let mut declaration = AccessSet::builder();
        declaration.read_principal_key(owner, actor, b"allowed").expect("declaration");
        let declaration = AccessDeclaration::explicit(declaration.build().expect("set"));
        for (calldata, permitted) in [(b"allowed".as_slice(), true), (b"denied".as_slice(), false)] {
            let capabilities = CapabilitySet::new([Capability::StorageRead]).expect("capabilities");
            let mut abi = authorized_abi(owner, actor, capabilities, Storage::new(), declaration.clone());
            let outcome = abi.storage_read(&mut activity_meter(), calldata);
            assert_eq!(outcome.is_ok(), permitted);
            if !permitted { assert_eq!(outcome, Err(AbiError::AccessDeclaration)); }
        }
    }

    #[test]
    fn actual_abi_prior_state_cannot_select_an_undeclared_followup_key() {
        let owner = program(1);
        let actor = principal(2);
        let namespace = StorageNamespace::principal(owner, actor);
        let mut storage = Storage::new();
        let mut transaction = storage.transaction(namespace);
        transaction.write(b"route", b"secret").expect("route");
        transaction.write(b"secret", b"value").expect("secret");
        let _ = transaction.commit();
        let mut declaration = AccessSet::builder();
        declaration.read_key(namespace, b"route").expect("route declaration");
        let capabilities = CapabilitySet::new([Capability::StorageRead]).expect("capabilities");
        let mut abi = authorized_abi(
            owner, actor, capabilities, storage,
            AccessDeclaration::explicit(declaration.build().expect("set")),
        );
        let selected = abi.storage_read(&mut activity_meter(), b"route")
            .expect("declared read").expect("route value");
        assert_eq!(
            abi.storage_read(&mut activity_meter(), &selected),
            Err(AbiError::AccessDeclaration)
        );
    }

    #[test]
    fn actual_nested_abi_refuses_an_undeclared_callee_write() {
        let owner = program(1);
        let callee = program(9);
        let actor = principal(2);
        let capabilities = CapabilitySet::new([
            Capability::Call { program: callee },
            Capability::SharedStorageWrite,
        ]).expect("capabilities");
        let mut declaration = AccessSet::builder();
        declaration.call(callee).expect("call declaration");
        let declaration = AccessDeclaration::explicit(declaration.build().expect("set"));
        let mut root = authorized_abi(owner, actor, capabilities, Storage::new(), declaration.clone());
        let child_frame = crate::abi::CallFrameId::root().child(1).expect("child frame");
        let child_capabilities = root.stage_call(
            callee, b"input", vec![Capability::SharedStorageWrite], child_frame,
        ).expect("declared call");
        let mut child = Abi::nested(
            crate::ABI_V2_VERSION,
            callee,
            AuthorizationContext::nested(actor, child_capabilities, child_frame),
            root.storage_snapshot(),
            root.verified_receipts(),
            root.verified_balances(),
        ).expect("child ABI");
        child.set_access_declaration(declaration);
        assert_eq!(
            child.storage_write_selected(
                &mut activity_meter(), crate::abi::StorageSelector::Shared, b"state", b"value",
            ),
            Err(AbiError::AccessDeclaration)
        );
    }

    #[test]
    fn absent_charge_resolves_every_capability_reachable_callee_namespace() {
        let owner = program(1);
        let callee = program(9);
        let actor = principal(2);
        let capabilities = CapabilitySet::new([
            Capability::Call { program: callee },
            Capability::SharedStorageWrite,
        ]).expect("capabilities");
        let reachable = capabilities.reachable_accesses(owner, actor).expect("reachable set");
        assert!(reachable.storage_accesses().any(|access| {
            access.namespace() == StorageNamespace::shared(owner)
        }));
        assert!(reachable.storage_accesses().any(|access| {
            access.namespace() == StorageNamespace::shared(callee)
        }));
        assert_eq!(reachable.callees().copied().collect::<Vec<_>>(), vec![callee]);
        assert_eq!(
            AccessDeclaration::absent().charge(&reachable),
            reachable.charge()
        );
    }
}
