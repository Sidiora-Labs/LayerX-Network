//! Principal-scoped gateway state, enforcing the same isolation rules as the
//! human plane's principal store: every translation record, telemetry row and
//! audit entry lives under exactly one validated principal, and rows are only
//! reachable through a [`PrincipalScope`]. No unscoped query exists.

use std::collections::BTreeMap;
use std::fmt::{Display, Formatter};

use crate::audit::AuditChain;

const IDENTIFIER_LIMIT: usize = 128;

fn valid_identifier(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= IDENTIFIER_LIMIT
        && value.bytes().all(|byte| {
            byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'-' || byte == b'_'
        })
}

/// A validated principal identifier, the only handle that reaches stored
/// gateway rows.
#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct PrincipalId(String);

impl PrincipalId {
    /// Creates a bounded identifier limited to `a-z`, `0-9`, `-` and `_` so no
    /// identifier can alias another principal's namespace.
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

/// Every namespace the gateway persists per principal.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub enum Table {
    Translations,
    Telemetry,
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

#[derive(Clone, Debug)]
struct Row {
    written_at: u64,
    bytes: Vec<u8>,
}

#[derive(Debug)]
struct PrincipalState {
    tables: BTreeMap<Table, BTreeMap<RowKey, Row>>,
    audit: AuditChain,
}

/// Principal-scoped gateway storage. Every row and audit entry lives under
/// exactly one validated principal, and rows are only reachable through a
/// [`PrincipalScope`].
#[derive(Debug, Default)]
pub struct GatewayStore {
    principals: BTreeMap<PrincipalId, PrincipalState>,
}

impl GatewayStore {
    /// Creates an empty store.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Presents a principal and receives the only mutable access path to its
    /// rows and audit chain.
    #[must_use]
    pub fn principal(&mut self, id: &PrincipalId) -> PrincipalScope<'_> {
        let state = self
            .principals
            .entry(id.clone())
            .or_insert_with(|| PrincipalState {
                tables: BTreeMap::new(),
                audit: AuditChain::new(id.as_str()),
            });
        PrincipalScope {
            principal: id.clone(),
            state,
        }
    }

    /// Reads one row of one principal without creating state for absent
    /// principals. The principal identifier is still the only access path.
    #[must_use]
    pub fn read(&self, id: &PrincipalId, table: Table, key: &RowKey) -> Option<StoredRow<'_>> {
        let row = self.principals.get(id)?.tables.get(&table)?.get(key)?;
        Some(StoredRow {
            written_at: row.written_at,
            bytes: &row.bytes,
        })
    }

    /// Returns one principal's audit chain, keyed by that principal only.
    #[must_use]
    pub fn principal_audit(&self, id: &PrincipalId) -> Option<&AuditChain> {
        self.principals.get(id).map(|state| &state.audit)
    }
}

/// The only access path to stored rows: a handle bound to one validated
/// principal. No unscoped query exists.
#[derive(Debug)]
pub struct PrincipalScope<'a> {
    principal: PrincipalId,
    state: &'a mut PrincipalState,
}

impl PrincipalScope<'_> {
    /// Returns the principal this scope is bound to.
    #[must_use]
    pub const fn principal(&self) -> &PrincipalId {
        &self.principal
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
    pub fn put(&mut self, table: Table, key: RowKey, written_at: u64, bytes: Vec<u8>) {
        self.state
            .tables
            .entry(table)
            .or_default()
            .insert(key, Row { written_at, bytes });
    }

    /// Returns this principal's append-only audit chain.
    #[must_use]
    pub fn audit(&mut self) -> &mut AuditChain {
        &mut self.state.audit
    }

    /// Returns a read-only view of this principal's audit chain.
    #[must_use]
    pub fn audit_view(&self) -> &AuditChain {
        &self.state.audit
    }
}

/// Errors from the principal store.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum StoreError {
    InvalidPrincipal,
    InvalidKey,
}

impl Display for StoreError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::InvalidPrincipal => formatter.write_str("invalid principal identifier"),
            Self::InvalidKey => formatter.write_str("invalid row key"),
        }
    }
}

impl std::error::Error for StoreError {}

#[cfg(test)]
mod tests {
    use super::{GatewayStore, PrincipalId, RowKey, StoreError, Table};

    fn principal(name: &str) -> PrincipalId {
        PrincipalId::new(name).unwrap_or_else(|error| panic!("principal {name}: {error}"))
    }

    fn key(name: &str) -> RowKey {
        RowKey::new(name).unwrap_or_else(|error| panic!("key {name}: {error}"))
    }

    #[test]
    fn identifiers_that_could_alias_or_traverse_are_refused() {
        assert_eq!(PrincipalId::new(""), Err(StoreError::InvalidPrincipal));
        assert_eq!(
            PrincipalId::new("../other"),
            Err(StoreError::InvalidPrincipal)
        );
        assert_eq!(PrincipalId::new("Alice"), Err(StoreError::InvalidPrincipal));
        assert_eq!(
            PrincipalId::new("a".repeat(129)),
            Err(StoreError::InvalidPrincipal)
        );
        assert_eq!(RowKey::new("k\0ey"), Err(StoreError::InvalidKey));
    }

    #[test]
    fn rows_written_under_one_principal_are_unreachable_from_another() {
        let mut store = GatewayStore::new();
        let alice = principal("alice");
        let mallory = principal("mallory");
        let row = key("tr-1");
        store
            .principal(&alice)
            .put(Table::Translations, row.clone(), 7, vec![1, 2, 3]);
        assert!(store
            .principal(&mallory)
            .get(Table::Translations, &row)
            .is_none());
        assert!(store.read(&mallory, Table::Translations, &row).is_none());
        assert!(store
            .principal(&mallory)
            .keys(Table::Translations)
            .is_empty());
        let stored = store
            .read(&alice, Table::Translations, &row)
            .unwrap_or_else(|| panic!("owner lost its own row"));
        assert_eq!(stored.bytes(), &[1, 2, 3]);
        assert_eq!(stored.written_at(), 7);
    }
}
