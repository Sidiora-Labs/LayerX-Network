//! Canonical protocol byte vectors minted by the sole human-plane payload
//! authority so no other human component encodes a protocol payload itself.

use layerx_wire::encode::Encoder;
use layerx_wire::hash;
use layerx_wire::limits::PROTOCOL_VERSION;
use layerx_wire::WireError;

/// Builds the committed canonical receipt vector, optionally carrying its
/// sequencer signature.
///
/// # Errors
///
/// Returns the wire error raised by the canonical encoder when a field exceeds
/// its declared bound.
pub fn receipt(signature: Option<[u8; 64]>) -> Result<Vec<u8>, WireError> {
    let mut encoder = Encoder::new(4096);
    encoder.structure_header_version(0x5201, PROTOCOL_VERSION)?;
    encoder.u16(PROTOCOL_VERSION)?;
    encoder.bytes(&[1; 32], 32)?;
    encoder.u64(9)?;
    encoder.bytes(&[2; 32], 32)?;
    encoder.bytes(&[3; 32], 32)?;
    encoder.bytes(&[8; 32], 32)?;
    encoder.i32(0)?;
    encoder.sequence_length(0, 512)?;
    encoder.u128(1)?;
    encoder.bytes(&[4; 32], 32)?;
    encoder.u16(1)?;
    encoder.u32(1)?;
    encoder.u32(1)?;
    encoder.u8(1)?;
    encoder.bytes(&[5; 32], 32)?;
    encoder.u128(25)?;
    encoder.bytes(&[6; 32], 32)?;
    encoder.u128(100)?;
    encoder.u128(75)?;
    encoder.u64(1)?;
    encoder.bytes(&[7; 32], 32)?;
    encoder.u128(10)?;
    encoder.u128(35)?;
    encoder.bytes(&[9; 32], 32)?;
    encoder.bytes(&[10; 32], 32)?;
    encoder.bytes(&[11; 32], 32)?;
    encoder.u64(1_000)?;
    encoder.u8(u8::from(signature.is_some()))?;
    if let Some(value) = signature {
        encoder.bytes(&value, 64)?;
    }
    Ok(encoder.finish())
}

/// Builds the committed canonical batch header vector over the named roots.
///
/// # Errors
///
/// Returns the wire error raised by the canonical encoder when a field exceeds
/// its declared bound.
pub fn batch_header(
    state_root: [u8; 32],
    activity_root: [u8; 32],
    sequencer_id: [u8; 32],
) -> Result<Vec<u8>, WireError> {
    let mut encoder = Encoder::new(354);
    encoder.structure_header_version(0x1701, PROTOCOL_VERSION)?;
    encoder.u8(15)?;
    for field in 1..=15_u8 {
        encoder.tag(field, 15)?;
        match field {
            1 => encoder.u16(PROTOCOL_VERSION)?,
            2 => encoder.u32(42)?,
            3 => encoder.u64(7)?,
            4 => encoder.u64(8)?,
            5 => encoder.u64(11)?,
            6 => encoder.u64(19)?,
            7 => encoder.bytes(&[7; 32], 32)?,
            8 => encoder.bytes(&state_root, 32)?,
            9 => encoder.bytes(&activity_root, 32)?,
            10 => encoder.bytes(&[10; 32], 32)?,
            11 => encoder.bytes(&[11; 32], 32)?,
            12 => encoder.bytes(&[12; 32], 32)?,
            13 => encoder.bytes(&[13; 32], 32)?,
            14 => encoder.u64(1_000)?,
            _ => encoder.bytes(&sequencer_id, 32)?,
        }
    }
    Ok(encoder.finish())
}

/// Returns the domain-separated signing digest of an unsigned canonical receipt.
///
/// # Errors
///
/// Returns the wire error raised when the receipt bytes are not canonical.
pub fn receipt_signing_digest(unsigned_receipt: &[u8]) -> Result<[u8; 32], WireError> {
    hash::receipt_digest(unsigned_receipt)
}

/// Returns the domain-separated signing digest of a canonical batch header.
///
/// # Errors
///
/// Returns the wire error raised when the header bytes are not canonical.
pub fn batch_header_signing_digest(header: &[u8]) -> Result<[u8; 32], WireError> {
    hash::batch_header_digest(header)
}
