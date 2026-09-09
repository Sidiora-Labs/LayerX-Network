//! Byte-exact canonical `LayerX` wire encoding.

pub mod account;
pub mod activity;
pub mod decode;
pub mod encode;
pub mod hash;
pub mod limits;
pub mod maintenance;
pub mod receipt;
pub mod sign;

use layerx_types::result::{KnownResult, ResultCode};

/// Typed canonical-codec failure with its exact protocol result and byte offset.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct WireError {
    /// Exact protocol result code produced by the same rejection class in core.
    pub result: ResultCode,
    /// Reader or writer byte offset at which the failure occurred.
    pub offset: usize,
}

impl WireError {
    pub(crate) fn known(result: KnownResult, offset: usize) -> Self {
        Self {
            result: result.into(),
            offset,
        }
    }
}

/// Requires byte-string keys to be strictly lexicographically increasing.
///
/// # Errors
///
/// Returns the protocol unsorted-sequence result for duplicate or decreasing
/// keys.
pub fn check_ordered_keys(keys: &[&[u8]]) -> Result<(), WireError> {
    for pair in keys.windows(2) {
        if pair[0] >= pair[1] {
            return Err(WireError::known(KnownResult::UnsortedSequence, 0));
        }
    }
    Ok(())
}
