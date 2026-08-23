//! Namespaced storage bindings.
//!
//! Keys address the calling program's own namespace, which the runtime fixes
//! before guest entry. The namespace may be principal-scoped `(program,
//! principal)` or program-shared `(program)`, and the type system makes it
//! impossible to address the wrong scope by accident.

use crate::abi::{MAX_STORAGE_KEY_BYTES, MAX_STORAGE_VALUE_BYTES};
use crate::error::{Field, ProgramError, Reason};

#[cfg(target_arch = "wasm32")]
use crate::buffer::Bytes;
#[cfg(target_arch = "wasm32")]
use crate::host;

/// A key inside this program's own namespace.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct StorageKey<'a>(&'a [u8]);

impl<'a> StorageKey<'a> {
    /// Constructs a key inside the version-one storage bound.
    ///
    /// # Errors
    ///
    /// Refuses an empty key and a key past the declared bound.
    pub const fn new(bytes: &'a [u8]) -> Result<Self, ProgramError> {
        if bytes.is_empty() {
            return Err(ProgramError::value(Field::StorageKey, Reason::Empty));
        }
        if bytes.len() > MAX_STORAGE_KEY_BYTES {
            return Err(ProgramError::value(Field::StorageKey, Reason::TooLarge));
        }
        Ok(Self(bytes))
    }

    /// Borrows the canonical key bytes.
    #[must_use]
    pub const fn bytes(self) -> &'a [u8] {
        self.0
    }
}

/// A value stored inside this program's own namespace.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct StorageValue<'a>(&'a [u8]);

impl<'a> StorageValue<'a> {
    /// Constructs a value inside the version-one storage bound.
    ///
    /// # Errors
    ///
    /// Refuses a value past the declared bound.
    pub const fn new(bytes: &'a [u8]) -> Result<Self, ProgramError> {
        if bytes.len() > MAX_STORAGE_VALUE_BYTES {
            return Err(ProgramError::value(Field::StorageValue, Reason::TooLarge));
        }
        Ok(Self(bytes))
    }

    /// Borrows the canonical value bytes.
    #[must_use]
    pub const fn bytes(self) -> &'a [u8] {
        self.0
    }
}

/// Reads one value into a caller-owned buffer, returning its length.
///
/// Returns `None` when the key holds no value.
///
/// # Errors
///
/// Refuses missing read authority, a buffer shorter than the stored value,
/// and every meter refusal.
#[cfg(target_arch = "wasm32")]
pub fn read(key: StorageKey<'_>, output: &mut [u8]) -> Result<Option<usize>, ProgramError> {
    let status = host::storage_read(key.bytes(), output)?;
    if status == 0 {
        return Ok(None);
    }
    let reported = usize::try_from(status)
        .map_err(|_| ProgramError::value(Field::StorageValue, Reason::Malformed))?;
    reported
        .checked_sub(1)
        .map(Some)
        .ok_or_else(|| ProgramError::value(Field::StorageValue, Reason::Malformed))
}

/// Reads one value into a fixed-capacity buffer, reporting whether the key
/// held a value at all.
///
/// # Errors
///
/// Refuses missing read authority, a buffer shorter than the stored value,
/// and every meter refusal.
#[cfg(target_arch = "wasm32")]
pub fn read_into<const N: usize>(
    key: StorageKey<'_>,
    output: &mut Bytes<N>,
) -> Result<bool, ProgramError> {
    output.clear();
    let Some(length) = read(key, output.as_mut_slice())? else {
        return Ok(false);
    };
    output.set_length(length)?;
    Ok(true)
}

/// Stages one value in this program's namespace.
///
/// # Errors
///
/// Refuses missing write authority, invalid bounds, and every meter refusal.
#[cfg(target_arch = "wasm32")]
pub fn write(key: StorageKey<'_>, value: StorageValue<'_>) -> Result<(), ProgramError> {
    host::storage_write(key.bytes(), value.bytes())?;
    Ok(())
}

/// Stages the deletion of one key in this program's namespace.
///
/// # Errors
///
/// Refuses missing write authority, invalid keys, and every meter refusal.
#[cfg(target_arch = "wasm32")]
pub fn delete(key: StorageKey<'_>) -> Result<(), ProgramError> {
    host::storage_delete(key.bytes())?;
    Ok(())
}

/// Program-shared storage operations.
///
/// Every binding addresses the shared namespace `(program)` only, which is
/// readable and writable by every principal invoking this program. The
/// namespace is visible in the type so a program cannot accidentally address
/// principal-scoped state when it needs shared state, or shared state when it
/// needs principal-scoped state.
pub mod shared {
    use super::*;

    /// A key inside this program's shared namespace.
    #[derive(Clone, Copy, Debug, Eq, PartialEq)]
    pub struct SharedStorageKey<'a>(&'a [u8]);

    impl<'a> SharedStorageKey<'a> {
        /// Constructs a key inside the shared namespace bound.
        ///
        /// # Errors
        ///
        /// Refuses an empty key and a key past the declared bound.
        pub const fn new(bytes: &'a [u8]) -> Result<Self, ProgramError> {
            if bytes.is_empty() {
                return Err(ProgramError::value(Field::StorageKey, Reason::Empty));
            }
            if bytes.len() > MAX_STORAGE_KEY_BYTES {
                return Err(ProgramError::value(Field::StorageKey, Reason::TooLarge));
            }
            Ok(Self(bytes))
        }

        /// Borrows the canonical key bytes.
        #[must_use]
        pub const fn bytes(self) -> &'a [u8] {
            self.0
        }
    }

    /// Reads one value from shared storage into a caller-owned buffer.
    ///
    /// Returns `None` when the key holds no value.
    ///
    /// # Errors
    ///
    /// Refuses missing `SharedStorageRead` authority, a buffer shorter than
    /// the stored value, and every meter refusal.
    #[cfg(target_arch = "wasm32")]
    pub fn read(
        key: SharedStorageKey<'_>,
        output: &mut [u8],
    ) -> Result<Option<usize>, ProgramError> {
        let status = host::storage_read_shared(key.bytes(), output)?;
        if status == 0 {
            return Ok(None);
        }
        let reported = usize::try_from(status)
            .map_err(|_| ProgramError::value(Field::StorageValue, Reason::Malformed))?;
        reported
            .checked_sub(1)
            .map(Some)
            .ok_or_else(|| ProgramError::value(Field::StorageValue, Reason::Malformed))
    }

    /// Reads one value from shared storage into a fixed-capacity buffer.
    ///
    /// # Errors
    ///
    /// Refuses missing `SharedStorageRead` authority, a buffer shorter than
    /// the stored value, and every meter refusal.
    #[cfg(target_arch = "wasm32")]
    pub fn read_into<const N: usize>(
        key: SharedStorageKey<'_>,
        output: &mut Bytes<N>,
    ) -> Result<bool, ProgramError> {
        output.clear();
        let Some(length) = read(key, output.as_mut_slice())? else {
            return Ok(false);
        };
        output.set_length(length)?;
        Ok(true)
    }

    /// Stages one value in this program's shared namespace.
    ///
    /// # Errors
    ///
    /// Refuses missing `SharedStorageWrite` authority, invalid bounds, and
    /// every meter refusal.
    #[cfg(target_arch = "wasm32")]
    pub fn write(
        key: SharedStorageKey<'_>,
        value: StorageValue<'_>,
    ) -> Result<(), ProgramError> {
        host::storage_write_shared(key.bytes(), value.bytes())?;
        Ok(())
    }

    /// Stages the deletion of one key in this program's shared namespace.
    ///
    /// # Errors
    ///
    /// Refuses missing `SharedStorageWrite` authority, invalid keys, and
    /// every meter refusal.
    #[cfg(target_arch = "wasm32")]
    pub fn delete(key: SharedStorageKey<'_>) -> Result<(), ProgramError> {
        host::storage_delete_shared(key.bytes())?;
        Ok(())
    }
}
