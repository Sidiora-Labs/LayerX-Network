//! Committed perps oracle observations exposed to programs.
#[cfg(target_arch = "wasm32")]
use crate::ProgramError;

/// Fixed little-endian bytes of one committed observation.
pub const OBSERVATION_BYTES: usize = 64;

/// The observation the perps engine committed under `oracle_root`.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Observation {
    pub price: u128,
    pub observed_at: u64,
    pub sequence: u64,
    pub source_set_digest: [u8; 32],
}

impl Observation {
    /// Decodes the fixed record the host writes into guest memory.
    #[must_use]
    pub fn from_record(bytes: &[u8; OBSERVATION_BYTES]) -> Self {
        let mut price = [0u8; 16];
        let mut observed_at = [0u8; 8];
        let mut sequence = [0u8; 8];
        let mut source_set_digest = [0u8; 32];
        price.copy_from_slice(&bytes[..16]);
        observed_at.copy_from_slice(&bytes[16..24]);
        sequence.copy_from_slice(&bytes[24..32]);
        source_set_digest.copy_from_slice(&bytes[32..]);
        Self {
            price: u128::from_le_bytes(price),
            observed_at: u64::from_le_bytes(observed_at),
            sequence: u64::from_le_bytes(sequence),
            source_set_digest,
        }
    }
}

/// Reads the latest committed observation for one perps market.
///
/// # Errors
///
/// Returns the typed refusal the host produced for an unknown or halted market.
#[cfg(target_arch = "wasm32")]
pub fn read(market_id: &[u8; 32]) -> Result<Observation, ProgramError> {
    let mut encoded = [0u8; OBSERVATION_BYTES];
    crate::host::oracle_read(market_id, &mut encoded)?;
    Ok(Observation::from_record(&encoded))
}
