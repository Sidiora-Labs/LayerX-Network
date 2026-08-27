//! Codec-wide hard bounds.

use layerx_types::result::KnownResult;

use crate::WireError;

/// Maximum canonical activity or receipt allocation.
pub const MAX_MESSAGE_BYTES: usize = 1_048_576;
/// Protocol version emitted and accepted by this crate revision.
pub const PROTOCOL_VERSION: u16 = 1;
/// Highest protocol version accepted in a version-carrying envelope.
pub const MAX_PROTOCOL_VERSION: u16 = 2;

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
