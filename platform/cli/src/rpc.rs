use serde_json::{json, Value};

use crate::http::Client;

pub struct RpcClient {
    client: Client,
    url: String,
    credential: Option<zeroize::Zeroizing<String>>,
}

impl RpcClient {
    /// # Errors
    /// Requires the gateway's explicit /rpc endpoint and the HTTP client's TLS rules.
    pub fn new(url: &str, credential: Option<zeroize::Zeroizing<String>>) -> Result<Self, String> {
        let base = url.strip_suffix("/rpc").ok_or("RPC URL must end in /rpc")?;
        let client = match credential.clone() {
            Some(value) => Client::new_gateway(base, value)?,
            None => Client::new(base, None)?,
        };
        Ok(Self {
            client,
            url: url.to_owned(),
            credential,
        })
    }

    /// # Errors
    /// Refuses unsupported methods, invalid parameters, transport failures and RPC errors.
    pub fn call(&self, method: &str, params: &Value) -> Result<Value, String> {
        if method == "lx_subscribe" {
            return Err(
                "rpc_transport_required: lx_subscribe requires authenticated WebSocket transport"
                    .into(),
            );
        }
        let request = request(method, params)?;
        decode_response(method, &self.client.post("/rpc", &request, None)?)
    }

    /// # Errors
    /// Requires a gateway credential and a valid live subscription; notifications are unverified.
    pub fn subscribe(&self, params: &Value, timeout: std::time::Duration) -> Result<Value, String> {
        let credential = self
            .credential
            .as_ref()
            .ok_or("gateway_credential_required: subscriptions require --gateway-credential")?;
        crate::rpc_subscription::next(&self.url, credential, params, timeout)
    }
}

/// # Errors
/// Validates positional parameters against the published `OpenRPC` contract.
pub fn request(method: &str, params: &Value) -> Result<Value, String> {
    let args = params
        .as_array()
        .ok_or("RPC parameters must be positional")?;
    match method {
        "lx_getNodeInfo" | "lx_listAssets" if args.is_empty() => {}
        "lx_getAsset"
        | "lx_getAccount"
        | "lx_getBalance"
        | "lx_getReceipt"
        | "lx_getActivityStatus"
        | "lx_getCheckpoint" => {
            let [Value::String(id)] = args.as_slice() else {
                return Err(format!("{method} requires one hexadecimal identifier"));
            };
            id32(id)?;
        }
        "lx_getSequence" => sequence_params(args)?,
        "lx_getBatchHeader" => {
            let [Value::String(number)] = args.as_slice() else {
                return Err("lx_getBatchHeader requires one decimal string".into());
            };
            if number
                .parse::<u64>()
                .ok()
                .filter(|n| *n > 0 && n.to_string() == *number)
                .is_none()
            {
                return Err("batch number must be a positive canonical u64 string".into());
            }
        }
        "lx_getBalances" => {
            let [Value::String(did)] = args.as_slice() else {
                return Err("lx_getBalances requires one DID".into());
            };
            layerx_types::ids::Did::new(did.as_bytes())
                .map_err(|e| format!("invalid DID: {e:?}"))?;
            crate::http::validate_resource_id(did, "DID")?;
        }
        "lx_getProof" => match args.as_slice() {
            [Value::String(kind), Value::String(activity)]
                if matches!(kind.as_str(), "activity" | "receipt") =>
            {
                id32(activity)?;
            }
            [Value::String(kind), Value::String(activity), Value::String(account)]
                if kind == "account" =>
            {
                id32(activity)?;
                id32(account)?;
            }
            _ => return Err(
                "lx_getProof requires kind, activity_id, and account_id only for account proofs"
                    .into(),
            ),
        },
        "lx_estimateFee" => {
            let [Value::String(canonical)] = args.as_slice() else {
                return Err("lx_estimateFee requires canonical_hex".into());
            };
            canonical_hex(canonical)?;
        }
        "lx_subscribe" => match args.as_slice() {
            [Value::String(topic)] if matches!(topic.as_str(), "receipts" | "checkpoints") => {}
            [Value::String(topic), Value::String(account)] if topic == "account" => id32(account)?,
            _ => {
                return Err(
                    "lx_subscribe requires receipts, checkpoints, or account with account_id"
                        .into(),
                )
            }
        },
        "lx_sendActivity" => {
            let [Value::String(canonical), Value::String(commitment)] = args.as_slice() else {
                return Err("lx_sendActivity requires canonical_hex and commitment".into());
            };
            if canonical.is_empty()
                || canonical.len() > 1_048_576
                || canonical.len() % 2 != 0
                || !canonical.bytes().all(|b| b.is_ascii_hexdigit())
                || !matches!(commitment.as_str(), "executed" | "batched" | "finalised")
            {
                return Err("invalid canonical activity or commitment".into());
            }
        }
        "lx_getNodeInfo" => return Err("lx_getNodeInfo takes no parameters".into()),
        _ => {
            return Err(format!(
                "rpc_method_unavailable: {method} is absent from the published OpenRPC contract"
            ))
        }
    }
    Ok(json!({"jsonrpc":"2.0","id":1,"method":method,"params":params}))
}

/// # Errors
/// Rejects mismatched IDs, malformed envelopes, RPC errors, and non-object results.
pub fn decode_response(method: &str, response: &Value) -> Result<Value, String> {
    if response.get("jsonrpc") != Some(&json!("2.0"))
        || response.get("id") != Some(&json!(1))
        || response.get("result").is_some() == response.get("error").is_some()
    {
        return Err(format!("invalid JSON-RPC response for {method}"));
    }
    if let Some(error) = response.get("error") {
        let code = error
            .get("code")
            .and_then(Value::as_i64)
            .ok_or("malformed RPC error code")?;
        let message = error
            .get("message")
            .and_then(Value::as_str)
            .ok_or("malformed RPC error message")?;
        return Err(json!({"code":code,"message":message,"data":error.get("data")}).to_string());
    }
    if method == "lx_subscribe" {
        return response
            .get("result")
            .filter(|value| {
                value
                    .as_str()
                    .is_some_and(|id| !id.is_empty() && id.len() <= 128)
            })
            .cloned()
            .ok_or_else(|| "invalid subscription identifier".to_owned());
    }
    response
        .get("result")
        .filter(|value| value.is_object())
        .cloned()
        .ok_or_else(|| format!("{method} returned a non-object result"))
}

fn sequence_params(args: &[Value]) -> Result<(), String> {
    match args {
        [Value::String(account)] => id32(account)?,
        [Value::String(did), Value::String(selector)] if selector == "identity" => {
            layerx_types::ids::Did::new(did.as_bytes())
                .map_err(|e| format!("invalid DID: {e:?}"))?;
            crate::http::validate_resource_id(did, "DID")?;
        }
        _ => return Err("lx_getSequence requires account_id or DID and identity selector".into()),
    }
    Ok(())
}

fn canonical_hex(value: &str) -> Result<(), String> {
    if value.is_empty()
        || value.len() > 1_048_576
        || !value.len().is_multiple_of(2)
        || !value.bytes().all(|byte| byte.is_ascii_hexdigit())
    {
        return Err("invalid canonical activity hex".into());
    }
    Ok(())
}

fn id32(value: &str) -> Result<(), String> {
    if value.len() != 64
        || !value.bytes().all(|b| b.is_ascii_hexdigit())
        || value.bytes().all(|b| b == b'0')
    {
        return Err("identifier must be 64 hexadecimal characters and nonzero".into());
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn positional_requests_match_published_contract() -> Result<(), String> {
        let published: Value = serde_json::from_str(include_str!("../tests/fixtures/openrpc.json"))
            .map_err(|error| error.to_string())?;
        let methods = published["methods"]
            .as_array()
            .ok_or("missing contract methods")?;
        let id = "ab".repeat(32);
        for method in [
            "lx_getAsset",
            "lx_getAccount",
            "lx_getBalance",
            "lx_getSequence",
            "lx_getReceipt",
            "lx_getActivityStatus",
            "lx_getCheckpoint",
        ] {
            assert!(methods.iter().any(|entry| entry["name"] == method));
            assert_eq!(request(method, &json!([id]))?["params"], json!([id]));
            assert!(request(method, &json!({"account_id":id})).is_err());
            assert!(request(method, &json!([id, id])).is_err());
        }
        let sequence = methods
            .iter()
            .find(|entry| entry["name"] == "lx_getSequence")
            .ok_or("sequence contract missing")?;
        assert_eq!(sequence["params"][1]["name"], "selector");
        assert_eq!(sequence["params"][1]["schema"]["enum"], json!(["identity"]));
        assert!(sequence["description"]
            .as_str()
            .ok_or("description missing")?
            .contains("[did, \"identity\"]"));
        request("lx_getSequence", &json!(["did:layerx:alice", "identity"]))?;
        assert!(request("lx_getSequence", &json!(["did:layerx:alice", "account"])).is_err());
        assert!(request("lx_getSequence", &json!(["../alice", "identity"])).is_err());
        request("lx_getBalances", &json!(["did:layerx:alice"]))?;
        request("lx_getNodeInfo", &json!([]))?;
        request("lx_listAssets", &json!([]))?;
        request("lx_estimateFee", &json!(["abcd"]))?;
        request("lx_subscribe", &json!(["receipts"]))?;
        request("lx_subscribe", &json!(["checkpoints"]))?;
        request("lx_subscribe", &json!(["account", id]))?;
        request("lx_getBatchHeader", &json!(["12"]))?;
        request("lx_getProof", &json!(["account", id, id]))?;
        for commitment in ["executed", "batched", "finalised"] {
            request("lx_sendActivity", &json!(["abcd", commitment]))?;
        }
        for (method, args) in [
            ("lx_getSequence", json!(["did:layerx:alice"])),
            ("lx_getBatchHeader", json!(["01"])),
            ("lx_getBalances", json!(["../alice"])),
            ("lx_getProof", json!(["account", id])),
            ("lx_sendActivity", json!(["abc", "executed"])),
            ("lx_sendActivity", json!(["abcd", "ack"])),
            ("lx_estimateFee", json!([])),
            ("lx_estimateFee", json!(["abc"])),
            ("lx_subscribe", json!(["account"])),
            ("lx_subscribe", json!(["receipts", id])),
            ("lx_subscribe", json!(["unknown"])),
            ("lx_getAsset", json!([])),
        ] {
            assert!(request(method, &args).is_err());
        }
        Ok(())
    }

    #[test]
    fn subscriptions_require_authenticated_websocket_and_string_ack() -> Result<(), String> {
        let client = RpcClient::new("http://127.0.0.1:1/rpc", None)?;
        assert!(client.call("lx_subscribe", &json!(["receipts"])).is_err());
        assert!(client
            .subscribe(&json!(["receipts"]), std::time::Duration::from_secs(1))
            .is_err());
        assert!(RpcClient::new("http://example.com/rpc", None).is_err());
        let response = json!({"jsonrpc":"2.0","id":1,"result":"1"});
        assert_eq!(decode_response("lx_subscribe", &response)?, "1");
        assert!(decode_response("lx_getReceipt", &response).is_err());
        for result in [json!(null), json!({}), json!(""), json!(1)] {
            assert!(decode_response(
                "lx_subscribe",
                &json!({"jsonrpc":"2.0","id":1,"result":result})
            )
            .is_err());
        }
        let error = json!({"code":-32005,"message":"feed unavailable","data":{"reason":"native_feed_unavailable"}});
        assert_eq!(
            serde_json::from_str::<Value>(
                &decode_response(
                    "lx_subscribe",
                    &json!({"jsonrpc":"2.0","id":1,"error":error})
                )
                .err()
                .ok_or("error lost")?
            )
            .map_err(|e| e.to_string())?,
            error
        );
        Ok(())
    }

    #[test]
    fn remote_enumeration_error_is_preserved() -> Result<(), String> {
        let fields = json!({"code":-32005,"message":"DID enumeration unavailable","data":{"reason":"native_index_unavailable"}});
        let response = json!({"jsonrpc":"2.0","id":1,"error":fields});
        let error = decode_response("lx_getBalances", &response)
            .err()
            .ok_or("RPC error was hidden")?;
        assert_eq!(
            serde_json::from_str::<Value>(&error).map_err(|e| e.to_string())?,
            fields
        );
        Ok(())
    }

    #[test]
    fn response_validation_refuses_errors_and_unbound_results() -> Result<(), String> {
        let good = json!({"jsonrpc":"2.0","id":1,"result":{"state":"pending"}});
        assert_eq!(
            decode_response("lx_sendActivity", &good)?["state"],
            "pending"
        );
        for bad in [
            json!({"jsonrpc":"2.0","id":2,"result":{}}),
            json!({"jsonrpc":"2.0","id":1,"result":null}),
            json!({"jsonrpc":"2.0","id":1,"result":{},"error":{}}),
            json!({"jsonrpc":"2.0","id":1,"error":{"code":-32601,"message":"Method not found"}}),
            json!({"id":1,"result":{}}),
        ] {
            assert!(decode_response("lx_getBalances", &bad).is_err());
        }
        Ok(())
    }
}
