//! Codec-wide hard bounds.

use layerx_types::result::KnownResult;

use crate::WireError;

/// Maximum canonical activity or receipt allocation.
pub const MAX_MESSAGE_BYTES: usize = 1_048_576;
/// Legacy protocol version retained for explicit decode compatibility.
pub const LEGACY_PROTOCOL_VERSION: u16 = 1;
/// Protocol version emitted by every current production encoder.
pub const PROTOCOL_VERSION: u16 = 2;
/// Highest protocol version accepted in a version-carrying envelope.
pub const MAX_PROTOCOL_VERSION: u16 = 2;
/// Version used by structures whose canonical layout is not version-carrying.
pub const STRUCTURE_VERSION: u16 = LEGACY_PROTOCOL_VERSION;

/// Enforces a declared element/count bound before allocation.
///
/// # Errors
///
/// Returns the protocol length-limit result at `offset` when `actual` exceeds
/// `maximum`.
pub fn enforce(actual: usize, maximum: usize, offset: usize) -> Result<(), WireError> {
    if actual > maximum {
        Err(WireError::known(KnownResult::LengthLimit, offset))
    } else {
        Ok(())
    }
}
