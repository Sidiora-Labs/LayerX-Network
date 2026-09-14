use layerx_types::result::KnownResult;

use crate::decode::Decoder;
use crate::hash::sha256;
use crate::maintenance::{decode_occupancy_maintenance, OccupancyMaintenance};
use crate::receipt::BatchHeader;
use crate::WireError;

pub const DOMAIN: &[u8] = b"LXP/batch-maintenance/v1\0";
pub const EFFECTS_DOMAIN: &[u8] = b"LXP/batch-maintenance-effects/v1\0";
pub const MAX_BYTES: usize = 524_288;
const MAX_OCCUPANCY_BYTES: usize =
    b"LXP/programs/occupancy-receipt/v2\0".len() + 374 + 256 * 81 + 65_536;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct BatchMaintenance<'a> {
    pub protocol_version: u16,
    pub epoch: u64,
    pub timestamp_ms: u64,
    pub occupancy: OccupancyMaintenance<'a>,
    pub effects: &'a [u8],
    pub frame_count: u16,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum MaintenanceReceipt<'a> {
    Occupancy(OccupancyMaintenance<'a>),
    Batch(BatchMaintenance<'a>),
}

impl MaintenanceReceipt<'_> {
    #[must_use]
    pub const fn occupancy(&self) -> &OccupancyMaintenance<'_> {
        match self {
            Self::Occupancy(record) => record,
            Self::Batch(record) => &record.occupancy,
        }
    }

    /// Binds the decoded record to the independently authenticated batch header.
    ///
    /// # Errors
    /// Refuses inconsistent batch, sequence, root, protocol, epoch or timestamp.
    pub fn verify_header(&self, header: &BatchHeader) -> Result<(), WireError> {
        let occupancy = self.occupancy();
        if occupancy.batch_number != header.batch_number()
            || occupancy.global_sequence != header.last_sequence()
            || occupancy.resulting_state_root != header.resulting_state_root()
        {
            return Err(invalid());
        }
        if let Self::Batch(record) = self {
            if record.protocol_version != header.protocol_version()
                || record.epoch != header.epoch()
                || record.timestamp_ms != header.timestamp_ms()
            {
                return Err(invalid());
            }
        }
        Ok(())
    }
}

fn invalid() -> WireError {
    WireError::known(KnownResult::NonCanonical, 0)
}

fn validate_state_body(body: &[u8]) -> Result<(), WireError> {
    let mut reader = Decoder::new(body, 0);
    let key_length = usize::from(reader.u16()?);
    if !(1..=64).contains(&key_length) {
        return Err(invalid());
    }
    reader.fixed(key_length)?;
    let deleted = reader.u8()?;
    let digest = reader.fixed(32)?;
    reader.finish()?;
    if deleted > 1 || (deleted == 1 && digest != sha256(&[])?) {
        return Err(invalid());
    }
    Ok(())
}

fn validate_effect(reader: &mut Decoder<'_>, ordinal: u16) -> Result<(), WireError> {
    if reader.u16()? != ordinal {
        return Err(invalid());
    }
    let event_type = reader.u16()?;
    let kind = reader.u8()?;
    let monetary = reader.u8()?;
    let transfer_root = reader.fixed(32)?;
    let body_length = usize::from(reader.u16()?);
    if !(1..=3).contains(&kind)
        || monetary > 1
        || (monetary != 0 && kind != 2)
        || ((kind == 2) == (transfer_root == [0; 32]))
        || body_length > 256
    {
        return Err(invalid());
    }
    let body = reader.fixed(body_length)?;
    if kind == 1 {
        if event_type != 0 {
            return Err(invalid());
        }
        validate_state_body(body)?;
    }
    Ok(())
}

/// Validates the canonical, bounded module effects committed by maintenance.
///
/// # Errors
/// Refuses unsupported modules, noncanonical ordering, effects or state bodies.
pub fn validate_effects(bytes: &[u8]) -> Result<u16, WireError> {
    crate::limits::enforce(bytes.len(), MAX_BYTES, 0)?;
    let mut reader = Decoder::new(bytes, 0);
    if reader.fixed(EFFECTS_DOMAIN.len())? != EFFECTS_DOMAIN {
        return Err(invalid());
    }
    let frame_count = reader.u16()?;
    if frame_count > 512 {
        return Err(invalid());
    }
    let mut previous_module = 0;
    for _ in 0..frame_count {
        let module = reader.u16()?;
        if !matches!(module, 2 | 3 | 5) || module < previous_module || reader.u32()? != 1 {
            return Err(invalid());
        }
        previous_module = module;
        let effect_count = reader.u16()?;
        if !(1..=512).contains(&effect_count) {
            return Err(invalid());
        }
        for ordinal in 0..effect_count {
            validate_effect(&mut reader, ordinal)?;
        }
    }
    reader.finish()?;
    Ok(frame_count)
}

/// Decodes a batch-maintenance/v1 envelope without accepting legacy records.
///
/// # Errors
/// Refuses unsupported identities, malformed nested records or trailing bytes.
pub fn decode_batch_maintenance(bytes: &[u8]) -> Result<BatchMaintenance<'_>, WireError> {
    crate::limits::enforce(bytes.len(), MAX_BYTES, 0)?;
    let mut reader = Decoder::new(bytes, 0);
    if reader.fixed(DOMAIN.len())? != DOMAIN {
        return Err(invalid());
    }
    let protocol_version = reader.u16()?;
    let epoch = reader.u64()?;
    let batch_number = reader.u64()?;
    let timestamp_ms = reader.u64()?;
    let global_sequence = reader.u64()?;
    let parameter_version = reader.u32()?;
    if protocol_version != 3 || epoch == 0 || timestamp_ms == 0 || global_sequence == u64::MAX {
        return Err(invalid());
    }
    let occupancy = decode_occupancy_maintenance(reader.bytes(MAX_OCCUPANCY_BYTES)?)?;
    if occupancy.batch_number != batch_number
        || occupancy.global_sequence != global_sequence
        || occupancy.parameter_version != parameter_version
    {
        return Err(invalid());
    }
    let effects = reader.bytes(MAX_BYTES)?;
    let frame_count = validate_effects(effects)?;
    reader.finish()?;
    Ok(BatchMaintenance {
        protocol_version,
        epoch,
        timestamp_ms,
        occupancy,
        effects,
        frame_count,
    })
}

/// Selects a maintenance format by its exact versioned domain.
///
/// # Errors
/// Refuses malformed records through the selected format's strict decoder.
pub fn decode_maintenance(bytes: &[u8]) -> Result<MaintenanceReceipt<'_>, WireError> {
    if bytes.starts_with(DOMAIN) {
        decode_batch_maintenance(bytes).map(MaintenanceReceipt::Batch)
    } else {
        decode_occupancy_maintenance(bytes).map(MaintenanceReceipt::Occupancy)
    }
}
