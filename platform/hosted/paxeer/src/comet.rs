use super::{
    refusal, rpc_error, transport, NodeEndpoint, NodeFailure, Request, Response, NODE_IO_TIMEOUT,
};
use serde::Deserialize;
use serde_json::{value::RawValue, Value};
use std::time::Instant;

const MAX_COMET_REQUEST: usize = 4096;
const MAX_COMET_RESPONSE: usize = 8 * 1024 * 1024;
const MAX_VALIDATOR_PAGE: u64 = 1024;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RpcRequest<'a> {
    jsonrpc: &'a str,
    id: &'a RawValue,
    method: &'a str,
    params: &'a RawValue,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct EmptyParams {}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct HeightParams<'a> {
    height: &'a str,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ValidatorParams<'a> {
    height: &'a str,
    page: &'a str,
    per_page: &'a str,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct QueryParams<'a> {
    path: &'a str,
    data: &'a str,
    height: &'a str,
    prove: bool,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RpcReply<'a> {
    jsonrpc: &'a str,
    id: &'a RawValue,
    result: Option<&'a RawValue>,
    error: Option<&'a RawValue>,
}

#[derive(Debug, PartialEq)]
enum Method {
    Commit(Option<u64>),
    Validators { height: u64, page: u64 },
    Query(u64),
}

struct Validated {
    id: Value,
    method: Method,
    body: Vec<u8>,
}

fn decimal(text: &str) -> Option<u64> {
    if text.is_empty() || text.starts_with('0') || !text.bytes().all(|byte| byte.is_ascii_digit()) {
        return None;
    }
    let value = text.parse::<i64>().ok()?;
    u64::try_from(value).ok()
}

fn validate(body: &[u8]) -> Result<Validated, Response> {
    if body.len() > MAX_COMET_REQUEST {
        return Err(refusal(413, "body_too_large", None));
    }
    let request: RpcRequest<'_> = serde_json::from_slice(body)
        .map_err(|_| rpc_error(&Value::Null, -32600, "invalid Comet request"))?;
    let id: Value = serde_json::from_str(request.id.get())
        .map_err(|_| rpc_error(&Value::Null, -32600, "invalid Comet request"))?;
    if request.jsonrpc != "2.0"
        || !(id.as_i64().is_some() || id.as_str().is_some_and(|text| text.len() <= 256))
    {
        return Err(rpc_error(&Value::Null, -32600, "invalid Comet request"));
    }
    let invalid = || rpc_error(&id, -32602, "invalid Comet parameters");
    let method = match request.method {
        "commit" => {
            if serde_json::from_str::<EmptyParams>(request.params.get()).is_ok() {
                Method::Commit(None)
            } else {
                let params: HeightParams<'_> =
                    serde_json::from_str(request.params.get()).map_err(|_| invalid())?;
                Method::Commit(Some(decimal(params.height).ok_or_else(invalid)?))
            }
        }
        "validators" => {
            let params: ValidatorParams<'_> =
                serde_json::from_str(request.params.get()).map_err(|_| invalid())?;
            let height = decimal(params.height).ok_or_else(invalid)?;
            let page = decimal(params.page).ok_or_else(invalid)?;
            if page > MAX_VALIDATOR_PAGE || params.per_page != "100" {
                return Err(invalid());
            }
            Method::Validators { height, page }
        }
        "abci_query" => {
            let params: QueryParams<'_> =
                serde_json::from_str(request.params.get()).map_err(|_| invalid())?;
            let height = decimal(params.height)
                .filter(|height| *height < i64::MAX as u64)
                .ok_or_else(invalid)?;
            let digits = params.data.strip_prefix("0x").ok_or_else(invalid)?;
            if params.path != "/store/evm/key"
                || !params.prove
                || digits.is_empty()
                || digits.len() > 256
                || !digits.len().is_multiple_of(2)
                || !digits
                    .bytes()
                    .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
            {
                return Err(invalid());
            }
            Method::Query(height)
        }
        _ => {
            return Err(rpc_error(
                &id,
                -32601,
                "method is not relayed by the Comet boundary",
            ))
        }
    };
    let mut params: Value = serde_json::from_str(request.params.get()).map_err(|_| invalid())?;
    if matches!(method, Method::Query(_)) {
        let data = params["data"].as_str().ok_or_else(invalid)?;
        params["data"] = Value::String(data[2..].to_owned());
    }
    let body =
        serde_json::json!({"jsonrpc": "2.0", "id": id, "method": request.method, "params": params})
            .to_string()
            .into_bytes();
    Ok(Validated { id, method, body })
}

enum Failure {
    Node(NodeFailure),
    Unavailable,
    ProofUnsupported,
}

impl From<NodeFailure> for Failure {
    fn from(value: NodeFailure) -> Self {
        Self::Node(value)
    }
}

fn reply(body: &[u8], id: &Value) -> Result<Value, Failure> {
    let envelope: RpcReply<'_> = serde_json::from_slice(body).map_err(|_| NodeFailure::Invalid)?;
    let actual_id: Value =
        serde_json::from_str(envelope.id.get()).map_err(|_| NodeFailure::Invalid)?;
    if envelope.jsonrpc != "2.0"
        || &actual_id != id
        || !(envelope.result.is_some() ^ envelope.error.is_some())
    {
        return Err(NodeFailure::Invalid.into());
    }
    let result = envelope.result.ok_or(Failure::Unavailable)?;
    let result: Value = serde_json::from_str(result.get()).map_err(|_| NodeFailure::Invalid)?;
    if !result.is_object() {
        return Err(NodeFailure::Invalid.into());
    }
    Ok(result)
}

fn commit_height(result: &Value, canonical: bool) -> Result<u64, Failure> {
    let is_canonical = result
        .get("canonical")
        .and_then(Value::as_bool)
        .ok_or(NodeFailure::Invalid)?;
    if canonical && !is_canonical {
        return Err(Failure::Unavailable);
    }
    let height = result
        .pointer("/signed_header/header/height")
        .and_then(Value::as_str)
        .and_then(decimal)
        .ok_or(NodeFailure::Invalid)?;
    if result
        .pointer("/signed_header/commit/height")
        .and_then(Value::as_str)
        .and_then(decimal)
        != Some(height)
    {
        return Err(NodeFailure::Invalid.into());
    }
    Ok(height)
}

fn committed_height(
    node: &NodeEndpoint,
    id: &Value,
    height: Option<u64>,
    deadline: Instant,
) -> Result<u64, Failure> {
    let params = height.map_or_else(
        || serde_json::json!({}),
        |height| serde_json::json!({"height": height.to_string()}),
    );
    let body =
        serde_json::json!({"jsonrpc": "2.0", "id": id, "method": "commit", "params": params})
            .to_string();
    let bytes = transport::request(
        node,
        "/",
        Some(body.as_bytes()),
        MAX_COMET_RESPONSE,
        deadline,
    )?;
    let observed = commit_height(&reply(&bytes, id)?, height.is_some())?;
    if height.is_some_and(|height| observed != height) {
        return Err(NodeFailure::Invalid.into());
    }
    Ok(observed)
}

fn validate_result(method: &Method, result: &Value) -> Result<(), Failure> {
    match method {
        Method::Commit(expected) => {
            let height = commit_height(result, expected.is_some())?;
            if expected.is_some_and(|expected| expected != height) {
                return Err(NodeFailure::Invalid.into());
            }
        }
        Method::Validators { height, page } => {
            let count = result
                .get("count")
                .and_then(Value::as_str)
                .and_then(decimal)
                .ok_or(NodeFailure::Invalid)?;
            let total = result
                .get("total")
                .and_then(Value::as_str)
                .and_then(decimal)
                .ok_or(NodeFailure::Invalid)?;
            let validators = result
                .get("validators")
                .and_then(Value::as_array)
                .ok_or(NodeFailure::Invalid)?;
            let offset = (page - 1) * 100;
            if result
                .get("block_height")
                .and_then(Value::as_str)
                .and_then(decimal)
                != Some(*height)
                || total > MAX_VALIDATOR_PAGE * 100
                || offset >= total
                || count != (total - offset).min(100)
                || count != validators.len() as u64
            {
                return Err(NodeFailure::Invalid.into());
            }
        }
        Method::Query(height) => {
            let query = result.get("response").ok_or(NodeFailure::Invalid)?;
            if query
                .get("height")
                .and_then(Value::as_str)
                .and_then(decimal)
                != Some(*height)
            {
                return Err(NodeFailure::Invalid.into());
            }
            if query
                .get("code")
                .is_some_and(|code| code.as_u64() != Some(0))
            {
                return Err(Failure::Unavailable);
            }
            let ops = query
                .pointer("/proof_ops/ops")
                .and_then(Value::as_array)
                .ok_or(Failure::ProofUnsupported)?;
            if ops.len() != 2
                || ops[0].get("type").and_then(Value::as_str) != Some("ics23:iavl")
                || ops[1].get("type").and_then(Value::as_str) != Some("ics23:simple")
                || ops.iter().any(|op| {
                    op.get("data")
                        .and_then(Value::as_str)
                        .is_none_or(str::is_empty)
                        || op
                            .get("key")
                            .and_then(Value::as_str)
                            .is_none_or(str::is_empty)
                })
            {
                return Err(Failure::ProofUnsupported);
            }
        }
    }
    Ok(())
}

fn fetch(node: &NodeEndpoint, request: &Validated) -> Result<Vec<u8>, Failure> {
    let deadline = Instant::now() + NODE_IO_TIMEOUT;
    if let Method::Query(height) = request.method {
        if committed_height(node, &request.id, None, deadline)? < height + 1 {
            return Err(Failure::Unavailable);
        }
        committed_height(node, &request.id, Some(height + 1), deadline)?;
    }
    let body = transport::request(node, "/", Some(&request.body), MAX_COMET_RESPONSE, deadline)?;
    validate_result(&request.method, &reply(&body, &request.id)?)?;
    if Instant::now() >= deadline {
        return Err(NodeFailure::Invalid.into());
    }
    Ok(body)
}

pub(super) fn response(node: &NodeEndpoint, request: &Request) -> Response {
    if request.headers.get("content-type").map(String::as_str) != Some("application/json") {
        return refusal(400, "content_type_required", None);
    }
    let validated = match validate(&request.body) {
        Ok(validated) => validated,
        Err(response) => return response,
    };
    match fetch(node, &validated) {
        Ok(body) => Response {
            status: 200,
            body,
            retry_after: None,
            genesis_sha256: None,
        },
        Err(Failure::Node(NodeFailure::Unreachable)) => refusal(503, "comet_unavailable", Some(5)),
        Err(Failure::Node(NodeFailure::Invalid)) => refusal(502, "comet_response_invalid", Some(5)),
        Err(Failure::Unavailable) => refusal(503, "comet_evidence_unavailable", Some(5)),
        Err(Failure::ProofUnsupported) => refusal(502, "comet_proof_unsupported", None),
    }
}

#[cfg(test)]
mod tests {
    use super::{validate, Method};

    #[test]
    fn read_requests_preserve_ids_and_translate_hex_bytes() {
        let request = validate(br#"{"jsonrpc":"2.0","id":"proof","method":"abci_query","params":{"path":"/store/evm/key","data":"0x08ab","height":"17","prove":true}}"#)
            .unwrap_or_else(|_| panic!("canonical proof request refused"));
        assert_eq!(request.id, "proof");
        assert_eq!(request.method, Method::Query(17));
        let upstream: serde_json::Value = serde_json::from_slice(&request.body)
            .unwrap_or_else(|error| panic!("upstream encoding: {error}"));
        assert_eq!(upstream["params"]["data"], "08ab");
        assert_eq!(upstream["id"], "proof");
        assert_eq!(upstream["params"]["prove"], true);
        assert!(validate(br#"{"jsonrpc":"2.0","id":7,"method":"commit","params":{}}"#).is_ok());
        assert!(
            validate(br#"{"jsonrpc":"2.0","id":7,"method":"commit","params":{"height":"1"}}"#)
                .is_ok()
        );
        assert!(validate(br#"{"jsonrpc":"2.0","id":7,"method":"validators","params":{"height":"1","page":"1","per_page":"100"}}"#).is_ok());
    }

    #[test]
    fn ambiguous_envelopes_and_notifications_are_refused() {
        for body in [
            r#"{"jsonrpc":"2.0","id":1,"id":2,"method":"commit","params":{}}"#,
            r#"{"jsonrpc":"2.0","id":1,"method":"commit","method":"broadcast_tx_sync","params":{}}"#,
            r#"{"jsonrpc":"2.0","method":"commit","params":{}}"#,
            r#"{"jsonrpc":"2.0","id":null,"method":"commit","params":{}}"#,
            r#"{"jsonrpc":"2.0","id":1.5,"method":"commit","params":{}}"#,
            r#"{"jsonrpc":"2.0","id":1,"method":"commit","params":{},"extra":true}"#,
            r#"[{"jsonrpc":"2.0","id":1,"method":"commit","params":{}}]"#,
            r#"{"jsonrpc":"2.0","id":1,"method":"commit","params":[]}"#,
        ] {
            assert!(validate(body.as_bytes()).is_err(), "accepted {body}");
        }
    }

    #[test]
    fn mutable_and_unproved_queries_are_refused() {
        for method in [
            "broadcast_tx_sync",
            "broadcast_evidence",
            "unsafe_flush_mempool",
            "abci_info",
            "status",
            "genesis",
        ] {
            let body =
                serde_json::json!({"jsonrpc":"2.0","id":1,"method":method,"params":{}}).to_string();
            assert!(validate(body.as_bytes()).is_err(), "accepted {method}");
        }
        let body = r#"{"jsonrpc":"2.0","id":1,"method":"abci_query","params":{"path":"/store/evm/key","data":"0x08ab","height":"17","prove":true}}"#;
        for invalid in [
            body.replace("true", "false"),
            body.replace("/store/evm/key", "/store/evm/subspace"),
            body.replace("0x08ab", "0x08AB"),
            body.replace("0x08ab", "0x0"),
            body.replace("0x08ab", "0x"),
            body.replace("0x08ab", &format!("0x{}", "ab".repeat(129))),
            body.replace("\"height\":\"17\"", "\"height\":\"17\",\"height\":\"18\""),
            body.replace("\"prove\":true", "\"prove\":true,\"extra\":1"),
        ] {
            assert!(validate(invalid.as_bytes()).is_err(), "accepted {invalid}");
        }
    }

    #[test]
    fn heights_pages_and_request_bytes_are_bounded() {
        for height in [
            "0",
            "-1",
            "01",
            "+1",
            "1.0",
            "1e1",
            " 1",
            "9223372036854775808",
        ] {
            let body = serde_json::json!({"jsonrpc":"2.0","id":1,"method":"commit","params":{"height":height}}).to_string();
            assert!(validate(body.as_bytes()).is_err(), "accepted {height}");
        }
        for params in [
            r#"{"height":"1","page":"0","per_page":"100"}"#,
            r#"{"height":"1","page":"1025","per_page":"100"}"#,
            r#"{"height":"1","page":"1","per_page":"101"}"#,
            r#"{"height":"1","height":"2","page":"1","per_page":"100"}"#,
        ] {
            let body =
                format!(r#"{{"jsonrpc":"2.0","id":1,"method":"validators","params":{params}}}"#);
            assert!(validate(body.as_bytes()).is_err());
        }
        let body = br#"{"jsonrpc":"2.0","id":1,"method":"commit","params":{}}"#;
        let mut padded = body.to_vec();
        padded.resize(4096, b' ');
        assert!(validate(&padded).is_ok());
        padded.push(b' ');
        assert!(validate(&padded).is_err());
    }
}
