//! Typed terminal refusals returned by the production node boundary.

use layerx_types::result::ResultCode;

/// Stable error-envelope payload emitted after a request reached the node.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct CoreRefusal {
    pub class: u8,
    pub result: ResultCode,
}

/// Decodes the fixed class plus signed protocol-result payload.
#[must_use]
pub fn decode_core_refusal(payload: &[u8]) -> Option<CoreRefusal> {
    let raw = i32::from_be_bytes(payload.get(1..5)?.try_into().ok()?);
    (payload.len() == 5).then_some(CoreRefusal {
        class: payload[0],
        result: ResultCode::from_raw(raw),
    })
}
