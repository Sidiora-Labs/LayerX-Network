//! Versioned canonical Programs batch-maintenance records.

use layerx_types::result::KnownResult;

use crate::decode::Decoder;
use crate::hash::{domain, sha256, CanonicalBytes, Domain};
use crate::WireError;

const DOMAIN: &[u8] = b"LXP/programs/occupancy-receipt/v2\0";
const MAX_PAYERS: usize = 256;
const MAX_EVIDENCE: usize = 65536;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct OccupancyPayer {
    pub principal: [u8; 32],
    pub due: u128,
    pub paid: u128,
    pub arrears: u128,
    pub frozen: bool,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct OccupancyMaintenance<'a> {
    pub batch_number: u64,
    pub global_sequence: u64,
    pub parameter_version: u32,
    pub schedule_version: u32,
    pub schedule_prices: [u64; 7],
    pub occupancy_asset_id: [u8; 32],
    pub byte_batches: u128,
    pub fee_units: u128,
    pub paid_fee_units: u128,
    pub arrears_fee_units: u128,
    pub payers: Vec<OccupancyPayer>,
    pub schedule_commitment: [u8; 32],
    pub settlement_evidence: &'a [u8],
    pub settlement_evidence_digest: [u8; 32],
    pub ledger_root: [u8; 32],
    pub transfer_set_root: [u8; 32],
    pub previous_state_root: [u8; 32],
    pub resulting_state_root: [u8; 32],
}

fn invalid() -> WireError {
    WireError::known(KnownResult::NonCanonical, 0)
}

fn array(reader: &mut Decoder<'_>) -> Result<[u8; 32], WireError> {
    reader.fixed(32)?.try_into().map_err(|_| invalid())
}

/// Decodes the native occupancy-receipt/v2 record and checks its commitments.
///
/// # Errors
/// Rejects unsupported formats, invalid payer accounting, bounds, trailing
/// bytes, and mismatched schedule or evidence digests. Settlement semantics
/// must additionally be verified by the proof consumer.
pub fn decode_occupancy_maintenance(bytes: &[u8]) -> Result<OccupancyMaintenance<'_>, WireError> {
    let mut reader = Decoder::new(bytes, 0);
    if reader.fixed(DOMAIN.len())? != DOMAIN {
        return Err(invalid());
    }
    let batch_number = reader.u64()?;
    let global_sequence = reader.u64()?;
    let parameter_version = reader.u32()?;
    let schedule_version = reader.u32()?;
    let mut schedule_prices = [0; 7];
    for price in &mut schedule_prices {
        *price = reader.u64()?;
    }
    let occupancy_asset_id = array(&mut reader)?;
    let byte_batches = reader.u128()?;
    let fee_units = reader.u128()?;
    let paid_fee_units = reader.u128()?;
    let arrears_fee_units = reader.u128()?;
    let count = usize::from(reader.u16()?);
    if count > MAX_PAYERS {
        return Err(invalid());
    }
    let mut payers: Vec<OccupancyPayer> = Vec::with_capacity(count);
    let mut paid_total = 0_u128;
    let mut arrears_total = 0_u128;
    for _ in 0..count {
        let principal = array(&mut reader)?;
        let due = reader.u128()?;
        let paid = reader.u128()?;
        let arrears = reader.u128()?;
        let frozen = match reader.u8()? {
            0 => false,
            1 => true,
            _ => return Err(invalid()),
        };
        if principal == [0; 32]
            || payers.last().is_some_and(|prior| prior.principal >= principal)
            || paid.checked_add(arrears) != Some(due)
            || frozen != (arrears != 0)
        {
            return Err(invalid());
        }
        paid_total = paid_total.checked_add(paid).ok_or_else(invalid)?;
        arrears_total = arrears_total.checked_add(arrears).ok_or_else(invalid)?;
        payers.push(OccupancyPayer { principal, due, paid, arrears, frozen });
    }
    let schedule_commitment = array(&mut reader)?;
    let settlement_evidence = reader.bytes(MAX_EVIDENCE)?;
    let settlement_evidence_digest = array(&mut reader)?;
    let ledger_root = array(&mut reader)?;
    let transfer_set_root = array(&mut reader)?;
    let previous_state_root = array(&mut reader)?;
    let resulting_state_root = array(&mut reader)?;
    reader.finish()?;
    if batch_number == 0 || parameter_version == 0 || schedule_version == 0
        || occupancy_asset_id == [0; 32] || ledger_root == [0; 32]
        || resulting_state_root == [0; 32] || settlement_evidence.is_empty()
        || paid_total != paid_fee_units || arrears_total != arrears_fee_units
        || (paid_total == 0) != (transfer_set_root == [0; 32])
    {
        return Err(invalid());
    }
    let mut schedule = Vec::with_capacity(92);
    schedule.extend_from_slice(&schedule_version.to_be_bytes());
    for price in schedule_prices {
        schedule.extend_from_slice(&price.to_be_bytes());
    }
    schedule.extend_from_slice(&occupancy_asset_id);
    if domain(Domain::ContextHash, &CanonicalBytes::from_wire(schedule))? != schedule_commitment
        || sha256(settlement_evidence)? != settlement_evidence_digest
    {
        return Err(invalid());
    }
    Ok(OccupancyMaintenance { batch_number, global_sequence, parameter_version,
        schedule_version, schedule_prices, occupancy_asset_id, byte_batches,
        fee_units, paid_fee_units, arrears_fee_units, payers, schedule_commitment,
        settlement_evidence, settlement_evidence_digest, ledger_root,
        transfer_set_root, previous_state_root, resulting_state_root })
}
