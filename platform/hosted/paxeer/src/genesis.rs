use super::{
    refusal, transport, NodeEndpoint, NodeFailure, Response, NODE_IO_TIMEOUT,
};
use serde_json::{value::RawValue, Value};
use std::collections::BTreeMap;
use std::fmt::Write as _;
use std::time::Instant;

const MAX_GENESIS_BYTES: usize = 32 * 1024 * 1024;
const MAX_COMET_RESPONSE_BYTES: usize = MAX_GENESIS_BYTES + 64 * 1024;
const MAX_GENESIS_CHUNKS: usize = 32;
const CHUNKED_ERROR: &str = "genesis response is large, please use the genesis_chunked API instead";

pub(super) fn endpoint(value: &str) -> Result<NodeEndpoint, String> {
    let endpoint = NodeEndpoint::parse(value)
        .map_err(|error| error.replace("LAYERX_PAXEER_NODE_URL", "LAYERX_PAXEER_COMET_URL"))?;
    if endpoint.path != "/" || value.chars().any(char::is_whitespace) {
        return Err("LAYERX_PAXEER_COMET_URL must be a loopback HTTP origin".to_owned());
    }
    Ok(endpoint)
}

fn request(node: &NodeEndpoint, path: &str, deadline: Instant) -> Result<Vec<u8>, NodeFailure> {
    transport::request(node, path, None, MAX_COMET_RESPONSE_BYTES, deadline)
}

enum CometReply {
    Success(Value),
    Error(Value),
}

fn document(bytes: &[u8]) -> Result<CometReply, NodeFailure> {
    let value: Value = serde_json::from_slice(bytes).map_err(|_| NodeFailure::Invalid)?;
    if !value.is_object() {
        return Err(NodeFailure::Invalid);
    }
    if value.get("jsonrpc").is_some() {
        if value.get("jsonrpc").and_then(Value::as_str) != Some("2.0")
            || !(value.get("result").is_some() ^ value.get("error").is_some())
        {
            return Err(NodeFailure::Invalid);
        }
        return if let Some(result) = value.get("result") {
            Ok(CometReply::Success(result.clone()))
        } else {
            value
                .get("error")
                .cloned()
                .map(CometReply::Error)
                .ok_or(NodeFailure::Invalid)
        };
    }
    if value.get("result").is_some() || value.get("error").is_some() {
        return Err(NodeFailure::Invalid);
    }
    if value.get("code").is_some() {
        Ok(CometReply::Error(value))
    } else {
        Ok(CometReply::Success(value))
    }
}

fn genesis_field(bytes: &[u8]) -> Result<Vec<u8>, NodeFailure> {
    let envelope: BTreeMap<&str, &RawValue> =
        serde_json::from_slice(bytes).map_err(|_| NodeFailure::Invalid)?;
    let result = if envelope.contains_key("jsonrpc") {
        serde_json::from_str::<BTreeMap<&str, &RawValue>>(
            envelope.get("result").ok_or(NodeFailure::Invalid)?.get(),
        )
        .map_err(|_| NodeFailure::Invalid)?
    } else {
        envelope
    };
    Ok(result
        .get("genesis")
        .ok_or(NodeFailure::Invalid)?
        .get()
        .as_bytes()
        .to_vec())
}

fn chunk_number(value: &Value, field: &str) -> Result<usize, NodeFailure> {
    let text = value
        .get(field)
        .and_then(Value::as_str)
        .ok_or(NodeFailure::Invalid)?;
    if text.is_empty() || !text.bytes().all(|byte| byte.is_ascii_digit()) {
        return Err(NodeFailure::Invalid);
    }
    text.parse().map_err(|_| NodeFailure::Invalid)
}

fn base64_digit(byte: u8) -> Result<u8, NodeFailure> {
    match byte {
        b'A'..=b'Z' => Ok(byte - b'A'),
        b'a'..=b'z' => Ok(byte - b'a' + 26),
        b'0'..=b'9' => Ok(byte - b'0' + 52),
        b'+' => Ok(62),
        b'/' => Ok(63),
        _ => Err(NodeFailure::Invalid),
    }
}

fn append_base64(encoded: &str, output: &mut Vec<u8>) -> Result<(), NodeFailure> {
    if encoded.is_empty() || !encoded.len().is_multiple_of(4) {
        return Err(NodeFailure::Invalid);
    }
    for (index, group) in encoded.as_bytes().chunks_exact(4).enumerate() {
        let a = base64_digit(group[0])?;
        let b = base64_digit(group[1])?;
        let padding = usize::from(group[3] == b'=') + usize::from(group[2] == b'=');
        if output.len().saturating_add(3 - padding) > MAX_GENESIS_BYTES
            || (padding != 0 && (index + 1) * 4 != encoded.len())
        {
            return Err(NodeFailure::Invalid);
        }
        output.push((a << 2) | (b >> 4));
        if group[2] == b'=' {
            if group[3] != b'=' || b & 15 != 0 {
                return Err(NodeFailure::Invalid);
            }
        } else {
            let c = base64_digit(group[2])?;
            output.push((b << 4) | (c >> 2));
            if group[3] == b'=' {
                if c & 3 != 0 {
                    return Err(NodeFailure::Invalid);
                }
            } else {
                output.push((c << 6) | base64_digit(group[3])?);
            }
        }
    }
    Ok(())
}

fn chunked_genesis(node: &NodeEndpoint, deadline: Instant) -> Result<Vec<u8>, NodeFailure> {
    let mut output = Vec::new();
    let mut total = None;
    for index in 0..MAX_GENESIS_CHUNKS {
        let bytes = request(node, &format!("/genesis_chunked?chunk={index}"), deadline)?;
        let CometReply::Success(result) = document(&bytes)? else {
            return Err(NodeFailure::Invalid);
        };
        let count = chunk_number(&result, "total")?;
        if count == 0
            || count > MAX_GENESIS_CHUNKS
            || total.is_some_and(|total| total != count)
            || chunk_number(&result, "chunk")? != index
        {
            return Err(NodeFailure::Invalid);
        }
        total = Some(count);
        append_base64(
            result
                .get("data")
                .and_then(Value::as_str)
                .ok_or(NodeFailure::Invalid)?,
            &mut output,
        )?;
        if index + 1 == count {
            return Ok(output);
        }
    }
    Err(NodeFailure::Invalid)
}

fn fetch(node: &NodeEndpoint) -> Result<Vec<u8>, NodeFailure> {
    let deadline = Instant::now() + NODE_IO_TIMEOUT;
    let bytes = request(node, "/genesis", deadline)?;
    let genesis = if let CometReply::Error(reply) = document(&bytes)? {
        if reply.get("code").and_then(Value::as_i64) != Some(-32603)
            || reply.get("message").and_then(Value::as_str) != Some("Internal error")
            || reply.get("data").and_then(Value::as_str) != Some(CHUNKED_ERROR)
        {
            return Err(NodeFailure::Invalid);
        }
        chunked_genesis(node, deadline)?
    } else {
        genesis_field(&bytes)?
    };
    if genesis.len() > MAX_GENESIS_BYTES || Instant::now() >= deadline {
        return Err(NodeFailure::Invalid);
    }
    let value: Value = serde_json::from_slice(&genesis).map_err(|_| NodeFailure::Invalid)?;
    if value
        .get("chain_id")
        .and_then(Value::as_str)
        .is_none_or(str::is_empty)
    {
        return Err(NodeFailure::Invalid);
    }
    Ok(genesis)
}

pub(super) fn response(node: &NodeEndpoint) -> Response {
    match fetch(node).and_then(|body| {
        let suite = rustls::crypto::ring::cipher_suite::TLS13_AES_128_GCM_SHA256
            .tls13()
            .ok_or(NodeFailure::Invalid)?;
        let hash = suite.common.hash_provider.hash(&body);
        let mut digest = String::with_capacity(64);
        for byte in hash.as_ref() {
            write!(digest, "{byte:02x}").map_err(|_| NodeFailure::Invalid)?;
        }
        Ok(Response {
            status: 200,
            body,
            retry_after: None,
            genesis_sha256: Some(digest),
        })
    }) {
        Ok(response) => response,
        Err(NodeFailure::Unreachable) => refusal(503, "comet_unavailable", Some(5)),
        Err(NodeFailure::Invalid) => refusal(502, "comet_response_invalid", Some(5)),
    }
}
