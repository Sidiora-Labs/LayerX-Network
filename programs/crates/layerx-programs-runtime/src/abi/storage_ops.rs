//! Namespaced storage operations exposed by the ABI transaction.

use crate::meter::Meter;
use crate::storage::metered_bytes;

use super::capability::CapabilityKey;
use super::{Abi, AbiError};

impl Abi {
    /// Reads one value from the current program/principal namespace.
    ///
    /// # Errors
    ///
    /// Refuses missing authority, invalid keys, or meter exhaustion.
    pub fn storage_read(
        &mut self,
        meter: &mut Meter,
        key: &[u8],
    ) -> Result<Option<Vec<u8>>, AbiError> {
        self.authorization
            .capabilities()
            .grant(&CapabilityKey::StorageRead)?;
        let value = self.storage.read(self.namespace, key)?;
        meter.charge_storage_read(metered_bytes(key, value.as_deref())?)?;
        Ok(value)
    }

    /// Stages one value in the current program/principal namespace.
    ///
    /// # Errors
    ///
    /// Refuses missing authority, invalid bounds, or meter exhaustion.
    pub fn storage_write(
        &mut self,
        meter: &mut Meter,
        key: &[u8],
        value: &[u8],
    ) -> Result<(), AbiError> {
        self.authorization
            .capabilities()
            .grant(&CapabilityKey::StorageWrite)?;
        let bytes = metered_bytes(key, Some(value))?;
        meter.charge_storage_write(bytes)?;
        self.storage.write(self.namespace, key, value)?;
        Ok(())
    }

    /// Stages deletion in the current program/principal namespace.
    ///
    /// # Errors
    ///
    /// Refuses missing authority, invalid keys, or meter exhaustion.
    pub fn storage_delete(&mut self, meter: &mut Meter, key: &[u8]) -> Result<(), AbiError> {
        self.authorization
            .capabilities()
            .grant(&CapabilityKey::StorageWrite)?;
        meter.charge_storage_write(metered_bytes(key, None)?)?;
        self.storage.delete(self.namespace, key)?;
        Ok(())
    }
}
