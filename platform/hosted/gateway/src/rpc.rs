use super::{
    json_response, media_type_is, parse_hex32, public_reads, response, Config, IncomingRequest,
    OutgoingResponse,
};
use serde_json::{json, Value};

fn error(id: &Value, code: i32, message: &str) -> Value {
    json!({"jsonrpc":"2.0", "id":id, "error":{"code":code,"message":message}})
}

fn selector(method: &str, params: Option<&Value>) -> Result<String, i32> {
    let empty = Vec::new();
    let args = match params {
        None => &empty,
        Some(Value::Array(args)) => args,
        _ => return Err(-32602),
    };
    if method == "lx_getProof" {
        if let [Value::String(kind), Value::String(activity), Value::String(account)] =
            args.as_slice()
        {
            if kind != "account"
                || [activity, account]
                    .iter()
                    .any(|id| parse_hex32(id).is_err() || **id == "00".repeat(32))
            {
                return Err(-32602);
            }
            return Ok(format!("/v1/proofs/account/{activity}/{account}"));
        }
        let [Value::String(kind), Value::String(id)] = args.as_slice() else {
            return Err(-32602);
        };
        if !matches!(kind.as_str(), "activity" | "receipt")
            || parse_hex32(id).is_err()
            || id == &"00".repeat(32)
        {
            return Err(-32602);
        }
        return Ok(format!("/v1/proofs/{kind}/{id}"));
    }
    if method == "lx_getNodeInfo" {
        return if args.is_empty() {
            Ok("/v1/node-info".into())
        } else {
            Err(-32602)
        };
    }
    let prefix = match method {
        "lx_getAccount" | "lx_getBalance" => "/v1/accounts/",
        "lx_getBalances" | "lx_getSequence" => "/v1/dids/",
        "lx_getReceipt" | "lx_getActivityStatus" => "/v1/receipts/",
        "lx_getBatchHeader" => "/v1/batches/",
        "lx_getCheckpoint" => "/v1/checkpoints/",
        _ => return Err(-32601),
    };
    let [Value::String(id)] = args.as_slice() else {
        return Err(-32602);
    };
    if method == "lx_getBatchHeader" {
        if id
            .parse::<u64>()
            .ok()
            .filter(|n| *n > 0 && n.to_string() == *id)
            .is_none()
        {
            return Err(-32602);
        }
    } else if matches!(method, "lx_getBalances" | "lx_getSequence") {
        if layerx_types::ids::Did::new(id.as_bytes()).is_err()
            || id
                .bytes()
                .any(|b| !b.is_ascii_alphanumeric() && !b"-._:".contains(&b))
        {
            return Err(-32602);
        }
    } else if parse_hex32(id).is_err() || id == &"00".repeat(32) {
        return Err(-32602);
    }
    let suffix = match method {
        "lx_getBalance" => "/balance",
        "lx_getBalances" => "/accounts",
        "lx_getSequence" => "/sequence",
        _ => "",
    };
    Ok(format!("{prefix}{id}{suffix}"))
}

fn invalid_request(value: &Value) -> Option<Value> {
    let id = value.get("id").unwrap_or(&Value::Null);
    if !value.is_object()
        || value.get("jsonrpc") != Some(&json!("2.0"))
        || !value.get("method").is_some_and(Value::is_string)
        || !(id.is_null() || id.is_string() || id.is_number())
    {
        Some(error(&Value::Null, -32600, "Invalid Request"))
    } else {
        None
    }
}

fn dispatch(config: &Config, value: &Value) -> Option<Value> {
    if let Some(refusal) = invalid_request(value) {
        return Some(refusal);
    }
    let id = value.get("id").cloned().unwrap_or(Value::Null);
    let method = value["method"].as_str()?;
    let result = match selector(method, value.get("params")) {
        Ok(path) => {
            let upstream = public_reads::read(config, &path);
            match serde_json::from_slice::<Value>(&upstream.body) {
                Ok(body) if upstream.status == 200 && body.get("result").is_some() => {
                    json!({"jsonrpc":"2.0","id":id,"result":body["result"]})
                }
                Ok(body) => {
                    let code = if upstream.status == 429 {
                        -32005
                    } else {
                        -32001
                    };
                    let mut refusal = error(&id, code, "Read unavailable");
                    refusal["error"]["data"] = body;
                    refusal
                }
                Err(_) => error(&id, -32603, "Invalid upstream response"),
            }
        }
        Err(code) => error(
            &id,
            code,
            if code == -32601 {
                "Method not found"
            } else {
                "Invalid params"
            },
        ),
    };
    value.get("id").map(|_| result)
}

pub(super) fn route(config: &Config, request: &IncomingRequest) -> OutgoingResponse {
    if request.path == "/rpc/schema" {
        return if request.method == "GET" {
            OutgoingResponse {
                status: 200,
                body: include_bytes!("../openrpc.json").to_vec(),
                retry_after: None,
            }
        } else {
            response(405, "method_not_allowed", None)
        };
    }
    if request.method != "POST" {
        return response(405, "method_not_allowed", None);
    }
    if !media_type_is(request, "application/json") {
        return response(415, "json_content_type_required", None);
    }
    let Ok(value) = serde_json::from_slice::<Value>(&request.body) else {
        return json_response(200, &error(&Value::Null, -32700, "Parse error"));
    };
    let result = if let Value::Array(batch) = &value {
        if batch.is_empty() || batch.len() > 32 {
            Some(error(&Value::Null, -32600, "Invalid Request"))
        } else {
            let results: Vec<_> = batch
                .iter()
                .filter_map(|entry| dispatch(config, entry))
                .collect();
            if results.is_empty() {
                None
            } else {
                Some(Value::Array(results))
            }
        }
    } else {
        dispatch(config, &value)
    };
    result.map_or_else(
        || OutgoingResponse {
            status: 204,
            body: Vec::new(),
            retry_after: None,
        },
        |value| json_response(200, &value),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn invalid_envelopes_have_json_rpc_errors_and_null_ids() {
        for value in [
            json!(null),
            json!(4),
            json!([]),
            json!({}),
            json!({"jsonrpc":"1.0","method":"lx_getNodeInfo","id":7}),
            json!({"jsonrpc":"2.0","method":7,"id":7}),
            json!({"jsonrpc":"2.0","method":"lx_getNodeInfo","id":true}),
        ] {
            assert_eq!(
                invalid_request(&value),
                Some(
                    json!({"jsonrpc":"2.0","id":null,"error":{"code":-32600,"message":"Invalid Request"}})
                )
            );
        }
        for value in [
            json!({"jsonrpc":"2.0","method":"lx_getNodeInfo"}),
            json!({"jsonrpc":"2.0","method":"lx_getNodeInfo","id":null}),
            json!({"jsonrpc":"2.0","method":"lx_getNodeInfo","id":"abc"}),
            json!({"jsonrpc":"2.0","method":"lx_getNodeInfo","id":1}),
        ] {
            assert_eq!(invalid_request(&value), None);
        }
    }

    #[test]
    fn selectors_bind_each_public_read() {
        let id = "ab".repeat(32);
        for (method, path) in [
            ("lx_getAccount", format!("/v1/accounts/{id}")),
            ("lx_getBalance", format!("/v1/accounts/{id}/balance")),
            ("lx_getReceipt", format!("/v1/receipts/{id}")),
            ("lx_getActivityStatus", format!("/v1/receipts/{id}")),
            ("lx_getCheckpoint", format!("/v1/checkpoints/{id}")),
        ] {
            assert_eq!(selector(method, Some(&json!([id]))), Ok(path));
            for invalid in [
                json!([]),
                json!([id, id]),
                json!(["../state"]),
                json!(["00".repeat(32)]),
                json!({"id":id}),
            ] {
                assert_eq!(selector(method, Some(&invalid)), Err(-32602));
            }
        }
        assert_eq!(selector("lx_getNodeInfo", None), Ok("/v1/node-info".into()));
        assert_eq!(
            selector("lx_getBatchHeader", Some(&json!(["1"]))),
            Ok("/v1/batches/1".into())
        );
        for invalid in ["0", "01", "-1", "18446744073709551616"] {
            assert_eq!(
                selector("lx_getBatchHeader", Some(&json!([invalid]))),
                Err(-32602)
            );
        }
        assert_eq!(
            selector("lx_getBalances", Some(&json!(["did:layerx:alice"]))),
            Ok("/v1/dids/did:layerx:alice/accounts".into())
        );
        assert_eq!(
            selector("lx_getSequence", Some(&json!(["did:layerx:alice"]))),
            Ok("/v1/dids/did:layerx:alice/sequence".into())
        );
        for kind in ["activity", "receipt"] {
            assert_eq!(
                selector("lx_getProof", Some(&json!([kind, id]))),
                Ok(format!("/v1/proofs/{kind}/{id}"))
            );
        }
        for args in [
            json!([]),
            json!(["receipt"]),
            json!(["unknown", id]),
            json!(["receipt", "00".repeat(32)]),
            json!(["receipt", "../state"]),
            json!(["receipt", id, id]),
        ] {
            assert_eq!(selector("lx_getProof", Some(&args)), Err(-32602));
        }
        assert_eq!(
            selector("lx_getProof", Some(&json!(["account", id, id]))),
            Ok(format!("/v1/proofs/account/{id}/{id}"))
        );
        for args in [
            json!(["account", id]),
            json!(["account", id, "00".repeat(32)]),
            json!(["account", "../", id]),
            json!(["account", id, id, id]),
        ] {
            assert_eq!(selector("lx_getProof", Some(&args)), Err(-32602));
        }
        assert_eq!(selector("unknown", None), Err(-32601));
    }
}
