//! Exact reclamation of one host-fixed storage namespace.

use std::collections::BTreeMap;

use super::{metered_bytes, StorageAddress, StorageError, StorageNamespace};

/// Exact provisional released-occupancy facts produced by dropping one namespace.
///
/// The facts are recorded with the committed activity so task 29.5's occupancy
/// ledger can net the pre- and post-activity state without reconstructing a
/// policy from wall-clock time or from post-commit storage.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct NamespaceDrop {
    namespace: StorageNamespace,
    reclaimed_cells: u64,
    reclaimed_key_value_bytes: u64,
    metered_work: u64,
}

impl NamespaceDrop {
    #[must_use]
    pub const fn namespace(self) -> StorageNamespace {
        self.namespace
    }

    #[must_use]
    pub const fn reclaimed_cells(self) -> u64 {
        self.reclaimed_cells
    }

    #[must_use]
    pub const fn reclaimed_key_value_bytes(self) -> u64 {
        self.reclaimed_key_value_bytes
    }

    /// Returns the exact storage-write work for this drop: each reclaimed cell
    /// plus every reclaimed key and value byte.
    #[must_use]
    pub const fn metered_work(self) -> u64 {
        self.metered_work
    }
}

/// Computes exact namespace reclamation before a meter or state mutation.
pub(crate) fn preview(
    cells: &BTreeMap<StorageAddress, Vec<u8>>,
    namespace: StorageNamespace,
) -> Result<NamespaceDrop, StorageError> {
    let (reclaimed_cells, reclaimed_key_value_bytes) = cells.iter().try_fold(
        (0u64, 0u64),
        |(cell_count, byte_count), (address, value)| {
            if address.namespace != namespace {
                return Ok((cell_count, byte_count));
            }
            let reclaimed_cells = cell_count
                .checked_add(1)
                .ok_or(StorageError::SizeOverflow)?;
            let reclaimed_bytes = metered_bytes(&address.key, Some(value))?;
            let reclaimed_key_value_bytes = byte_count
                .checked_add(reclaimed_bytes)
                .ok_or(StorageError::SizeOverflow)?;
            Ok((reclaimed_cells, reclaimed_key_value_bytes))
        },
    )?;
    let metered_work = reclaimed_cells
        .checked_add(reclaimed_key_value_bytes)
        .ok_or(StorageError::SizeOverflow)?;
    Ok(NamespaceDrop {
        namespace,
        reclaimed_cells,
        reclaimed_key_value_bytes,
        metered_work,
    })
}

/// Removes every cell of exactly the namespace described by `drop`.
///
/// The caller must use a preview from the same storage snapshot. The storage
/// transaction owns that snapshot, so no concurrent mutation can make the
/// recorded provisional fact diverge before this deterministic removal.
pub(crate) fn apply(cells: &mut BTreeMap<StorageAddress, Vec<u8>>, drop: NamespaceDrop) {
    cells.retain(|address, _| address.namespace != drop.namespace);
}
