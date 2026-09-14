use crate::{EndpointConfig, EndpointFailure, EndpointFault, Json};
const LIMIT: usize = 1_048_576;

pub(crate) fn invalid() -> EndpointFault {
    EndpointFault::InconsistentObservation
}
pub(crate) fn failure(endpoint: &EndpointConfig, fault: EndpointFault) -> EndpointFailure {
    EndpointFailure {
        url: endpoint.url.clone(),
        fault,
    }
}
pub(crate) fn required<'a>(value: &'a Json, name: &str) -> Result<&'a Json, EndpointFault> {
    value.member(name).ok_or_else(invalid)
}
pub(crate) fn hex(bytes: &[u8]) -> String {
    use std::fmt::Write as _;
    let mut out = String::from("0x");
    for byte in bytes {
        let _ = write!(out, "{byte:02x}");
    }
    out
}
pub(crate) fn bytes(value: &Json) -> Result<Vec<u8>, EndpointFault> {
    let text = value
        .as_text()
        .and_then(|s| s.strip_prefix("0x"))
        .ok_or_else(invalid)?;
    if text.len() % 2 != 0 || text.len() > LIMIT * 2 {
        return Err(invalid());
    }
    text.as_bytes()
        .chunks_exact(2)
        .map(|pair| {
            let text = std::str::from_utf8(pair).map_err(|_| invalid())?;
            u8::from_str_radix(text, 16).map_err(|_| invalid())
        })
        .collect()
}
pub(crate) fn fixed<const N: usize>(value: &Json) -> Result<[u8; N], EndpointFault> {
    bytes(value)?.try_into().map_err(|_| invalid())
}
pub(crate) fn quantity(value: &Json) -> Result<u64, EndpointFault> {
    let text = value
        .as_text()
        .and_then(|s| s.strip_prefix("0x"))
        .ok_or_else(invalid)?;
    if text.is_empty() || (text.len() > 1 && text.starts_with('0')) {
        return Err(invalid());
    }
    u64::from_str_radix(text, 16).map_err(|_| invalid())
}
