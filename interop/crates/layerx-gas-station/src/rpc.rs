use std::collections::BTreeMap;
use std::io::{Read as _, Write as _};
use std::net::{TcpStream, ToSocketAddrs as _};
use std::time::Duration;

use serde::de::DeserializeOwned;
use serde_json::{json, Value};

use crate::config::StationConfig;
use crate::quote::{keccak, Word};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RpcFault {
    Configuration,
    Unavailable,
    RateLimited,
    Divergence,
    Malformed,
    Rejected { code: i64 },
}
impl std::fmt::Display for RpcFault {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Configuration => f.write_str("rpc configuration refused"),
            Self::Unavailable => f.write_str("rpc unavailable"),
            Self::RateLimited => f.write_str("rpc rate limited"),
            Self::Divergence => f.write_str("rpc responses diverge"),
            Self::Malformed => f.write_str("rpc response malformed"),
            Self::Rejected { code } => write!(f, "rpc rejected {code}"),
        }
    }
}
impl std::error::Error for RpcFault {}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SendOutcome {
    Accepted,
    Unknown,
}

pub trait JsonRpc {
    /// # Errors
    /// Returns a sanitized transport or response fault.
    fn call(&self, method: &str, params: Value) -> Result<Value, RpcFault>;
    /// # Errors
    /// Refuses mismatched bytes or a deterministic rejection. Unknown outcomes retain the bytes.
    fn send_raw_transaction(&self, raw: &[u8], hash: &Word) -> Result<SendOutcome, RpcFault>;
}

/// # Errors
/// Refuses a response that cannot be decoded as the requested type.
pub fn read<T: DeserializeOwned>(
    rpc: &impl JsonRpc,
    method: &str,
    params: Value,
) -> Result<T, RpcFault> {
    serde_json::from_value(rpc.call(method, params)?).map_err(|_| RpcFault::Malformed)
}

#[must_use]
pub fn hex(bytes: &[u8]) -> String {
    const DIGITS: &[u8] = b"0123456789abcdef";
    let mut result = String::from("0x");
    for byte in bytes {
        result.push(char::from(DIGITS[usize::from(byte >> 4)]));
        result.push(char::from(DIGITS[usize::from(byte & 15)]));
    }
    result
}

/// # Errors
/// Refuses noncanonical or oversized hexadecimal data.
pub fn bytes(text: &str) -> Result<Vec<u8>, RpcFault> {
    let text = text.strip_prefix("0x").ok_or(RpcFault::Malformed)?;
    if !text.len().is_multiple_of(2) || text.len() > 2_097_152 || !text.is_ascii() {
        return Err(RpcFault::Malformed);
    }
    (0..text.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(&text[i..i + 2], 16).map_err(|_| RpcFault::Malformed))
        .collect()
}

/// # Errors
/// Refuses overflowing or noncanonical JSON-RPC quantities.
pub fn quantity(text: &str) -> Result<u128, RpcFault> {
    let raw = text.strip_prefix("0x").ok_or(RpcFault::Malformed)?;
    if raw.is_empty()
        || raw.len() > 32
        || (raw.len() > 1 && raw.starts_with('0'))
        || !raw.bytes().all(|b| b.is_ascii_hexdigit())
    {
        return Err(RpcFault::Malformed);
    }
    u128::from_str_radix(raw, 16).map_err(|_| RpcFault::Malformed)
}

pub trait Exchange {
    /// # Errors
    /// Returns a sanitized fault; response error messages are never retained.
    fn request(&self, endpoint: &str, method: &str, params: &Value) -> Result<Value, RpcFault>;
}

pub struct ConfiguredRpc<E> {
    endpoints: Vec<String>,
    exchange: E,
}
impl<E: Exchange> ConfiguredRpc<E> {
    /// # Errors
    /// Refuses invalid configuration, duplicate endpoints or more than eight endpoints.
    pub fn new(config: &StationConfig, exchange: E) -> Result<Self, RpcFault> {
        config.validate().map_err(|_| RpcFault::Configuration)?;
        let mut unique = config.endpoints.clone();
        unique.sort();
        unique.dedup();
        if unique.len() != config.endpoints.len() || unique.len() > 8 {
            return Err(RpcFault::Configuration);
        }
        Ok(Self {
            endpoints: config.endpoints.clone(),
            exchange,
        })
    }
}
impl<E: Exchange> JsonRpc for ConfiguredRpc<E> {
    fn call(&self, method: &str, params: Value) -> Result<Value, RpcFault> {
        let mut votes: BTreeMap<String, (usize, Value)> = BTreeMap::new();
        let mut faults = Vec::new();
        for endpoint in &self.endpoints {
            match self.exchange.request(endpoint, method, &params) {
                Ok(value) => votes.entry(value.to_string()).or_insert((0, value)).0 += 1,
                Err(fault) => faults.push(fault),
            }
        }
        let majority = self.endpoints.len() / 2 + 1;
        if let Some((_, value)) = votes.values().find(|(count, _)| *count >= majority) {
            return Ok(value.clone());
        }
        if votes.len() > 1 {
            return Err(RpcFault::Divergence);
        }
        for fault in &faults {
            if faults.iter().filter(|other| *other == fault).count() >= majority {
                return Err(*fault);
            }
        }
        Err(RpcFault::Unavailable)
    }

    fn send_raw_transaction(&self, raw: &[u8], hash: &Word) -> Result<SendOutcome, RpcFault> {
        if raw.first() != Some(&4) || keccak(raw) != *hash {
            return Err(RpcFault::Malformed);
        }
        match self.call("eth_sendRawTransaction", json!([hex(raw)])) {
            Ok(Value::String(value)) if value == hex(hash) => Ok(SendOutcome::Accepted),
            Err(fault @ (RpcFault::Rejected { .. } | RpcFault::Configuration)) => Err(fault),
            _ => Ok(SendOutcome::Unknown),
        }
    }
}

pub struct HttpsExchange;
impl Exchange for HttpsExchange {
    fn request(&self, endpoint: &str, method: &str, params: &Value) -> Result<Value, RpcFault> {
        let tail = endpoint
            .strip_prefix("https://")
            .ok_or(RpcFault::Configuration)?;
        let (authority, path) = tail
            .split_once('/')
            .map_or((tail, "/".to_owned()), |(a, p)| (a, format!("/{p}")));
        if authority.is_empty()
            || endpoint.chars().any(char::is_whitespace)
            || endpoint.contains(['@', '#', '?', '\r', '\n'])
        {
            return Err(RpcFault::Configuration);
        }
        let (host, port) = authority
            .rsplit_once(':')
            .map_or(Ok((authority, 443)), |(h, p)| {
                p.parse::<u16>()
                    .map(|port| (h, port))
                    .map_err(|_| RpcFault::Configuration)
            })?;
        let timeout = Duration::from_secs(10);
        let socket = (host, port)
            .to_socket_addrs()
            .map_err(|_| RpcFault::Unavailable)?
            .next()
            .ok_or(RpcFault::Unavailable)?;
        let stream =
            TcpStream::connect_timeout(&socket, timeout).map_err(|_| RpcFault::Unavailable)?;
        stream
            .set_read_timeout(Some(timeout))
            .map_err(|_| RpcFault::Unavailable)?;
        stream
            .set_write_timeout(Some(timeout))
            .map_err(|_| RpcFault::Unavailable)?;
        let connector = native_tls::TlsConnector::new().map_err(|_| RpcFault::Configuration)?;
        let mut stream = connector
            .connect(host, stream)
            .map_err(|_| RpcFault::Unavailable)?;
        let body = json!({"jsonrpc":"2.0","id":1,"method":method,"params":params}).to_string();
        if body.len() > 1_048_576 {
            return Err(RpcFault::Configuration);
        }
        write!(stream, "POST {path} HTTP/1.1\r\nHost: {authority}\r\nContent-Type: application/json\r\nConnection: close\r\nContent-Length: {}\r\n\r\n{body}", body.len()).map_err(|_| RpcFault::Unavailable)?;
        stream.flush().map_err(|_| RpcFault::Unavailable)?;
        let mut response = Vec::new();
        stream
            .take(1_048_577)
            .read_to_end(&mut response)
            .map_err(|_| RpcFault::Unavailable)?;
        decode_http(&response)
    }
}

fn decode_http(raw: &[u8]) -> Result<Value, RpcFault> {
    if raw.len() > 1_048_576 {
        return Err(RpcFault::Malformed);
    }
    let split = raw
        .windows(4)
        .position(|w| w == b"\r\n\r\n")
        .ok_or(RpcFault::Malformed)?;
    let head = std::str::from_utf8(&raw[..split]).map_err(|_| RpcFault::Malformed)?;
    let mut lines = head.split("\r\n");
    let status = lines.next().ok_or(RpcFault::Malformed)?;
    match status.split_whitespace().nth(1) {
        Some("200") => (),
        Some("429") => return Err(RpcFault::RateLimited),
        _ => return Err(RpcFault::Unavailable),
    }
    let mut length = None;
    for line in lines {
        let (name, value) = line.split_once(':').ok_or(RpcFault::Malformed)?;
        if name.eq_ignore_ascii_case("transfer-encoding") {
            return Err(RpcFault::Malformed);
        }
        if name.eq_ignore_ascii_case("content-length") {
            if length.is_some() {
                return Err(RpcFault::Malformed);
            }
            length = Some(
                value
                    .trim()
                    .parse::<usize>()
                    .map_err(|_| RpcFault::Malformed)?,
            );
        }
    }
    let body = &raw[split + 4..];
    if length.is_some_and(|length| length != body.len()) {
        return Err(RpcFault::Malformed);
    }
    decode_response(&serde_json::from_slice::<Value>(body).map_err(|_| RpcFault::Malformed)?)
}

fn decode_response(value: &Value) -> Result<Value, RpcFault> {
    if value["jsonrpc"] != "2.0"
        || value["id"] != 1
        || value.get("result").is_some() == value.get("error").is_some()
    {
        return Err(RpcFault::Malformed);
    }
    if let Some(error) = value.get("error") {
        return Err(RpcFault::Rejected {
            code: error["code"].as_i64().ok_or(RpcFault::Malformed)?,
        });
    }
    value.get("result").cloned().ok_or(RpcFault::Malformed)
}

#[cfg(test)]
mod tests {
    use super::*;
    struct Replay {
        replies: Vec<Value>,
        cursor: std::cell::Cell<usize>,
    }
    impl Exchange for Replay {
        fn request(&self, _: &str, _: &str, _: &Value) -> Result<Value, RpcFault> {
            let index = self.cursor.get();
            self.cursor.set(index + 1);
            decode_response(&self.replies[index])
        }
    }
    #[test]
    fn configured_quorum_refuses_disagreement_and_malformed_responses(
    ) -> Result<(), Box<dyn std::error::Error>> {
        let values: Value = serde_json::from_str(include_str!("../tests/fixtures/rpc.json"))?;
        let mut config = crate::config::tests::config();
        config.endpoints.push(format!("{}/", config.endpoints[0]));
        for (names, expected) in [
            (["ok", "other"], RpcFault::Divergence),
            (["malformed", "malformed"], RpcFault::Malformed),
            (
                ["rejected", "rejected"],
                RpcFault::Rejected { code: -32000 },
            ),
        ] {
            let rpc = ConfiguredRpc::new(
                &config,
                Replay {
                    replies: names.map(|n| values[n].clone()).to_vec(),
                    cursor: std::cell::Cell::new(0),
                },
            )?;
            assert_eq!(rpc.call("eth_chainId", json!([])), Err(expected));
        }
        config.endpoints.push(config.endpoints[0].clone());
        assert!(matches!(
            ConfiguredRpc::new(&config, HttpsExchange),
            Err(RpcFault::Configuration)
        ));
        Ok(())
    }
    #[test]
    fn refusals_and_untrusted_errors_are_sanitized() {
        let faults = [
            RpcFault::Configuration,
            RpcFault::Unavailable,
            RpcFault::RateLimited,
            RpcFault::Divergence,
            RpcFault::Malformed,
            RpcFault::Rejected { code: -32000 },
        ];
        for fault in faults {
            assert!(fault.to_string().starts_with("rpc "));
        }
        assert_eq!(
            decode_response(
                &json!({"jsonrpc":"2.0","id":1,"error":{"code":-1,"message":"sensitive"}})
            ),
            Err(RpcFault::Rejected { code: -1 })
        );
        for value in [
            json!({}),
            json!({"jsonrpc":"2.0","id":2,"result":1}),
            json!({"jsonrpc":"2.0","id":1,"result":1,"error":{}}),
        ] {
            assert_eq!(decode_response(&value), Err(RpcFault::Malformed));
        }
        assert_eq!(
            decode_http(b"HTTP/1.1 429 Limited\r\n\r\n"),
            Err(RpcFault::RateLimited)
        );
        assert_eq!(decode_http(b"bad"), Err(RpcFault::Malformed));
        for invalid in ["", "0x", "0x00", "0x-1", "0xg"] {
            assert!(quantity(invalid).is_err());
        }
        assert_eq!(quantity("0x0"), Ok(0));
        assert_eq!(bytes("0x0"), Err(RpcFault::Malformed));
        assert_eq!(bytes(&hex(&[0, 255])), Ok(vec![0, 255]));
    }
}
