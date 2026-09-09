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
        "lx_getAccount" | "lx_getBalance" | "lx_getSequence" => "/v1/accounts/",
        "lx_getBalances" => "/v1/dids/",
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
    } else if method == "lx_getBalances" {
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
        "lx_getBalance" | "lx_getSequence" => "/balance",
        "lx_getBalances" => "/accounts",
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

fn dispatch(config: &Config, request: &IncomingRequest, value: &Value) -> Option<Value> {
    if let Some(refusal) = invalid_request(value) {
        return Some(refusal);
    }
    let id = value.get("id").cloned().unwrap_or(Value::Null);
    let method = value["method"].as_str()?;
    if method == "lx_sendActivity" {
        let result = send(config, request, &id, value.get("params"));
        return value.get("id").map(|_| result);
    }
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

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum Commitment {
    Executed,
    Batched,
    Finalised,
}

fn send_params(params: Option<&Value>) -> Result<(Vec<u8>, Commitment), i32> {
    let Some(Value::Array(args)) = params else {
        return Err(-32602);
    };
    let [Value::String(canonical), Value::String(commitment)] = args.as_slice() else {
        return Err(-32602);
    };
    let commitment = match commitment.as_str() {
        "executed" => Commitment::Executed,
        "batched" => Commitment::Batched,
        "finalised" => Commitment::Finalised,
        _ => return Err(-32602),
    };
    let canonical = super::decode_hex(canonical, 512 * 1024).map_err(|_| -32602)?;
    if canonical.is_empty() {
        return Err(-32602);
    }
    Ok((canonical, commitment))
}

fn upstream_result(id: &Value, answer: &OutgoingResponse) -> Result<Value, Value> {
    let body: Value = serde_json::from_slice(&answer.body)
        .map_err(|_| error(id, -32603, "Invalid upstream response"))?;
    if matches!(answer.status, 200 | 202) && body.get("result").is_some() {
        return Ok(body["result"].clone());
    }
    let code = match answer.status {
        400 | 415 => -32602,
        401 | 403 => -32002,
        429 => -32005,
        _ => -32001,
    };
    let mut refused = error(id, code, "Submission unavailable");
    refused["error"]["data"] = body;
    Err(refused)
}

fn send(config: &Config, request: &IncomingRequest, id: &Value, params: Option<&Value>) -> Value {
    let (canonical, commitment) = match send_params(params) {
        Ok(value) => value,
        Err(code) => return error(id, code, "Invalid params"),
    };
    let record = match super::authenticate_key(config, request) {
        Ok(record) => record,
        Err(answer) => return upstream_result(id, &answer).unwrap_or_else(|value| value),
    };
    if !super::permits(&record, &super::ProductionRoute::Activity) {
        return error(id, -32002, "Insufficient scope");
    }
    let Ok(activity) = super::decode_signed(&canonical, &config.modules) else {
        return error(id, -32602, "Invalid canonical activity");
    };
    let path = match (
        activity.activity_type().module(),
        activity.activity_type().ordinal(),
    ) {
        (super::ModuleId::Programs, 1) => "/v1/programs/deploy",
        (super::ModuleId::Programs, 2) => "/v1/programs/upgrade",
        (super::ModuleId::Programs, 3) => "/v1/programs/call",
        (super::ModuleId::Programs, 7) => "/v1/programs/wind-down",
        _ => "/v1/activities",
    };
    let Ok(route) = super::production_route("POST", path) else {
        return error(id, -32603, "Invalid submission route");
    };
    if !super::permits(&record, &route) {
        return error(id, -32002, "Insufficient scope");
    }
    let mut headers = request.headers.clone();
    headers.insert("content-type".into(), "application/octet-stream".into());
    headers.insert(
        "idempotency-key".into(),
        super::hex(&activity.idempotency_key()),
    );
    let forwarded = IncomingRequest {
        method: "POST".into(),
        path: path.into(),
        headers,
        body: canonical,
    };
    let answer = super::activity(
        config,
        &forwarded,
        &record,
        &super::trace(request),
        path == "/v1/programs/call",
        true,
    );
    let mut result = match upstream_result(id, &answer) {
        Ok(result) => result,
        Err(error) => return error,
    };
    if answer.status == 202 {
        result["state"] = json!("pending");
        return json!({"jsonrpc":"2.0", "id":id, "result":result});
    }
    if result
        .get("receipt")
        .and_then(Value::as_str)
        .is_none_or(str::is_empty)
    {
        return error(id, -32603, "Missing verified receipt");
    }
    if commitment == Commitment::Executed {
        result["commitment"] = json!("executed");
    } else {
        complete_commitment(config, &mut result, commitment);
    }
    json!({"jsonrpc":"2.0", "id":id, "result":result})
}

fn read_result(config: &Config, path: &str) -> Option<Value> {
    let answer = public_reads::read(config, path);
    if answer.status != 200 {
        return None;
    }
    let document: Value = serde_json::from_slice(&answer.body).ok()?;
    document.get("result").cloned()
}

fn complete_commitment(config: &Config, result: &mut Value, commitment: Commitment) {
    let Some(activity) = result
        .get("activity_id")
        .and_then(Value::as_str)
        .map(str::to_owned)
    else {
        result["state"] = json!("pending");
        return;
    };
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
    loop {
        if let Some(proof) =
            read_result(config, &format!("/v1/proofs/receipt/{activity}")).filter(|proof| {
                proof["activity_id"] == activity && proof["canonical_value"] == result["receipt"]
            })
        {
            if commitment == Commitment::Batched {
                result["commitment"] = json!("batched");
                result["batch_evidence"] = proof;
                return;
            }
            if let Some(node) = read_result(config, "/v1/node-info") {
                if let Some(checkpoint) = node
                    .get("latest_finalised_checkpoint")
                    .and_then(Value::as_str)
                    .filter(|id| parse_hex32(id).is_ok() && *id != "00".repeat(32))
                {
                    if let Some(evidence) =
                        read_result(config, &format!("/v1/checkpoints/{checkpoint}"))
                    {
                        if evidence
                            .get("canonical_header")
                            .and_then(Value::as_str)
                            .is_some()
                            && evidence["canonical_header"]
                                == proof["signed_header"]["canonical_header"]
                        {
                            result["commitment"] = json!("finalised");
                            result["batch_evidence"] = proof;
                            result["checkpoint_evidence"] = evidence;
                            return;
                        }
                    }
                }
            }
        }
        if std::time::Instant::now() >= deadline {
            result["state"] = json!("pending");
            result["commitment"] = json!("executed");
            return;
        }
        std::thread::sleep(std::time::Duration::from_millis(50));
    }
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
                .filter_map(|entry| dispatch(config, request, entry))
                .collect();
            if results.is_empty() {
                None
            } else {
                Some(Value::Array(results))
            }
        }
    } else {
        dispatch(config, request, &value)
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
    fn send_params_require_canonical_hex_and_an_explicit_commitment() {
        for (name, commitment) in [
            ("executed", Commitment::Executed),
            ("batched", Commitment::Batched),
            ("finalised", Commitment::Finalised),
        ] {
            assert_eq!(
                send_params(Some(&json!(["abcd", name]))),
                Ok((vec![0xab, 0xcd], commitment))
            );
        }
        for params in [
            json!([]),
            json!(["abcd"]),
            json!(["", "executed"]),
            json!(["0xz1", "executed"]),
            json!(["abc", "executed"]),
            json!(["abcd", "ack"]),
            json!(["abcd", "executed", 3]),
            json!({}),
        ] {
            assert_eq!(send_params(Some(&params)), Err(-32602));
        }
    }

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
            ("lx_getSequence", format!("/v1/accounts/{id}/balance")),
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
            selector("lx_getSequence", Some(&json!([id]))),
            Ok(format!("/v1/accounts/{id}/balance"))
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
