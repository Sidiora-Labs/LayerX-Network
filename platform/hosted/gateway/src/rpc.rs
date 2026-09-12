use super::{
    json_response, media_type_is, parse_hex32, public_reads, response, Config, IncomingRequest,
    OutgoingResponse,
};
use serde_json::{json, Value};

pub(super) fn error(id: &Value, code: i32, message: &str) -> Value {
    json!({"jsonrpc":"2.0", "id":id, "error":{"code":code,"message":message}})
}

fn selector(method: &str, params: Option<&Value>) -> Result<String, i32> {
    let empty = Vec::new();
    let args = match params {
        None => &empty,
        Some(Value::Array(args)) => args,
        _ => return Err(-32602),
    };
    if method == "lx_getSequence" {
        if let [Value::String(did), Value::String(kind)] = args.as_slice() {
            if kind != "identity"
                || layerx_types::ids::Did::new(did.as_bytes()).is_err()
                || did
                    .bytes()
                    .any(|b| !b.is_ascii_alphanumeric() && !b"-._:".contains(&b))
            {
                return Err(-32602);
            }
            return Ok(format!("/v1/dids/{did}/sequence"));
        }
    }
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
    if matches!(method, "lx_getNodeInfo" | "lx_listAssets") {
        return if args.is_empty() {
            Ok(if method == "lx_getNodeInfo" {
                "/v1/node-info"
            } else {
                "/v1/assets"
            }
            .into())
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
        "lx_getAsset" => "/v1/assets/",
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

pub(super) fn invalid_request(value: &Value) -> Option<Value> {
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

pub(super) fn dispatch(config: &Config, request: &IncomingRequest, value: &Value) -> Option<Value> {
    if let Some(refusal) = invalid_request(value) {
        return Some(refusal);
    }
    let id = value.get("id").cloned().unwrap_or(Value::Null);
    let method = value["method"].as_str()?;
    if let Some(result) = crate::rpc_register::dispatch(config, method, &id, value.get("params")) {
        return value.get("id").map(|_| result);
    }
    if let Some(result) =
        crate::rpc_faucet::dispatch(config, request, method, &id, value.get("params"))
    {
        return value.get("id").map(|_| result);
    }
    if method == "lx_sendActivity" {
        let result = send(config, request, &id, value.get("params"));
        return value.get("id").map(|_| result);
    }
    if matches!(method, "lx_subscribe" | "lx_unsubscribe") {
        return value
            .get("id")
            .map(|_| error(&id, -32004, "WebSocket required"));
    }
    if method == "lx_estimateFee" {
        let result = match fee_params(value.get("params")) {
            Ok(canonical) => {
                let body = json!({"canonical_hex": super::hex(&canonical)}).to_string();
                read_response(
                    &id,
                    &public_reads::request(config, "POST", "/v1/fees/estimate", body.as_bytes()),
                )
            }
            Err(code) => error(&id, code, "Invalid params"),
        };
        return value.get("id").map(|_| result);
    }
    let result = match selector(method, value.get("params")) {
        Ok(path) => read_response(&id, &public_reads::read(config, &path)),
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

fn read_response(id: &Value, upstream: &OutgoingResponse) -> Value {
    match serde_json::from_slice::<Value>(&upstream.body) {
        Ok(body) if upstream.status == 200 && body.get("result").is_some() => {
            json!({"jsonrpc":"2.0","id":id,"result":body["result"]})
        }
        Ok(body) => {
            let (code, message) = match upstream.status {
                400 | 415 => (-32602, "Invalid params"),
                401 | 403 => (-32002, "Insufficient scope"),
                429 => (-32005, "Read unavailable"),
                _ => (-32001, "Read unavailable"),
            };
            let mut refusal = error(id, code, message);
            refusal["error"]["data"] = body;
            refusal
        }
        Err(_) => error(id, -32603, "Invalid upstream response"),
    }
}

fn fee_params(params: Option<&Value>) -> Result<Vec<u8>, i32> {
    let Some(Value::Array(args)) = params else {
        return Err(-32602);
    };
    let [Value::String(canonical)] = args.as_slice() else {
        return Err(-32602);
    };
    let canonical = super::decode_hex(canonical, 512 * 1024).map_err(|_| -32602)?;
    if canonical.is_empty() {
        return Err(-32602);
    }
    Ok(canonical)
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum Commitment {
    Executed,
    Batched,
    Finalised,
}

impl Commitment {
    fn name(self) -> &'static str {
        match self {
            Self::Executed => "executed",
            Self::Batched => "batched",
            Self::Finalised => "finalised",
        }
    }
}

fn pending_commitment(id: &Value, commitment: Commitment, evidence: &Value) -> Value {
    let mut refusal = error(id, -32001, "Requested commitment unavailable");
    refusal["error"]["data"] = json!({
        "state": "pending", "requested_commitment": commitment.name(), "evidence": evidence
    });
    refusal
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
    if answer.status == 200 && body.get("result").is_some() {
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

fn send_path(activity_type: layerx_types::payload::ActivityType) -> &'static str {
    match (activity_type.module(), activity_type.ordinal()) {
        (super::ModuleId::Programs, 1) => "/v1/programs/deploy",
        (super::ModuleId::Programs, 2) => "/v1/programs/upgrade",
        (super::ModuleId::Programs, 3) => "/v1/programs/call",
        (super::ModuleId::Programs, 7) => "/v1/programs/wind-down",
        _ => "/v1/activities",
    }
}

fn send(config: &Config, request: &IncomingRequest, id: &Value, params: Option<&Value>) -> Value {
    let total_started = std::time::Instant::now();
    let params_started = std::time::Instant::now();
    let (canonical, commitment) = match send_params(params) {
        Ok(value) => value,
        Err(code) => return error(id, code, "Invalid params"),
    };
    layerx_platform_gateway::pay_timing("gateway.rpc.params", params_started);
    let auth_started = std::time::Instant::now();
    let record = match super::authenticate_key(config, request) {
        Ok(record) => record,
        Err(answer) => return upstream_result(id, &answer).unwrap_or_else(|value| value),
    };
    layerx_platform_gateway::pay_timing("gateway.rpc.authenticate", auth_started);
    if !super::permits(&record, &super::ProductionRoute::Activity) {
        return error(id, -32002, "Insufficient scope");
    }
    let verify_started = std::time::Instant::now();
    let Ok(signer_public_key) = super::parse_hex32(&record.signer_public_key) else {
        return error(id, -32603, "Gateway persistence unavailable");
    };
    let verified = match layerx_platform_gateway::verify_submission(
        &canonical,
        &config.modules,
        config.protocol_version,
        config.protocol_network_id,
        &signer_public_key,
    ) {
        Ok(verified) => verified,
        Err(layerx_platform_gateway::GatewayError::Forbidden) => {
            return error(id, -32002, "Activity authorization refused");
        }
        Err(_) => return error(id, -32602, "Invalid canonical activity"),
    };
    layerx_platform_gateway::pay_timing("gateway.rpc.verify_submission", verify_started);
    let path = send_path(verified.activity_type());
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
        super::hex(&verified.idempotency_key()),
    );
    let forwarded = IncomingRequest {
        method: "POST".into(),
        path: path.into(),
        headers,
        body: canonical,
    };
    let proxy_started = std::time::Instant::now();
    let answer = super::activity(
        config,
        &forwarded,
        &record,
        &super::trace(request),
        path == "/v1/programs/call",
        true,
        Some(verified),
    );
    layerx_platform_gateway::pay_timing("gateway.rpc.activity", proxy_started);
    let mut result = match upstream_result(id, &answer) {
        Ok(result) => result,
        Err(mut error) => {
            if answer.status == 202 {
                let upstream = error["error"]["data"].take();
                error["error"]["data"] = json!({
                    "requested_commitment": commitment.name(), "state": "pending",
                    "upstream": upstream
                });
            }
            return error;
        }
    };
    if result
        .get("receipt")
        .and_then(Value::as_str)
        .is_none_or(str::is_empty)
    {
        return error(id, -32603, "Missing verified receipt");
    }
    if commitment == Commitment::Executed {
        result["commitment"] = json!("executed");
    } else if !complete_commitment(config, &mut result, commitment) {
        return pending_commitment(id, commitment, &result);
    }
    let response = json!({"jsonrpc":"2.0", "id":id, "result":result});
    layerx_platform_gateway::pay_timing("gateway.rpc.total", total_started);
    response
}

pub(super) fn read_result(config: &Config, path: &str) -> Option<Value> {
    let answer = public_reads::read(config, path);
    if answer.status != 200 {
        return None;
    }
    let document: Value = serde_json::from_slice(&answer.body).ok()?;
    document.get("result").cloned()
}

fn complete_commitment(config: &Config, result: &mut Value, commitment: Commitment) -> bool {
    let Some(activity) = result
        .get("activity_id")
        .and_then(Value::as_str)
        .map(str::to_owned)
    else {
        return false;
    };
    let Some(proof) =
        read_result(config, &format!("/v1/proofs/receipt/{activity}")).filter(|proof| {
            proof["activity_id"] == activity && proof["canonical_value"] == result["receipt"]
        })
    else {
        return false;
    };
    if commitment == Commitment::Batched {
        result["commitment"] = json!("batched");
        result["batch_evidence"] = proof;
        return true;
    }
    if let Some(node) = read_result(config, "/v1/node-info") {
        if let Some(checkpoint) = node
            .get("latest_finalised_checkpoint")
            .and_then(Value::as_str)
            .filter(|id| parse_hex32(id).is_ok() && *id != "00".repeat(32))
        {
            if let Some(evidence) = read_result(config, &format!("/v1/checkpoints/{checkpoint}")) {
                if evidence
                    .get("canonical_header")
                    .and_then(Value::as_str)
                    .is_some()
                    && evidence["canonical_header"] == proof["signed_header"]["canonical_header"]
                {
                    result["commitment"] = json!("finalised");
                    result["batch_evidence"] = proof;
                    result["checkpoint_evidence"] = evidence;
                    return true;
                }
            }
        }
    }
    false
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
    fn pending_is_an_error_without_commitment_downgrade() {
        let answer = json_response(
            202,
            &json!({"result":{"activity_id":"ab","state":"pending"}}),
        );
        let refusal = upstream_result(&json!(1), &answer)
            .err()
            .unwrap_or_else(|| panic!("pending accepted"));
        assert_eq!(refusal["error"]["code"], -32001);
        assert!(refusal.get("result").is_none());
        for commitment in [
            Commitment::Executed,
            Commitment::Batched,
            Commitment::Finalised,
        ] {
            let refusal = pending_commitment(&json!(1), commitment, &json!({"receipt":"ab"}));
            assert_eq!(
                refusal["error"]["data"]["requested_commitment"],
                commitment.name()
            );
            assert_eq!(refusal["error"]["data"]["state"], "pending");
            assert!(refusal.get("result").is_none());
            assert!(refusal["error"]["data"].get("commitment").is_none());
        }
    }

    #[test]
    fn identity_sequence_selector_and_canonical_bound_are_exact() {
        assert_eq!(
            selector(
                "lx_getSequence",
                Some(&json!(["did:layerx:alice", "identity"]))
            ),
            Ok("/v1/dids/did:layerx:alice/sequence".into())
        );
        for args in [
            json!(["../", "identity"]),
            json!(["did:layerx:alice", "account"]),
        ] {
            assert_eq!(selector("lx_getSequence", Some(&args)), Err(-32602));
        }
        assert_eq!(
            fee_params(Some(&json!(["ab".repeat(512 * 1024)])))
                .unwrap_or_else(|error| panic!("{error:?}"))
                .len(),
            512 * 1024
        );
        assert_eq!(
            send_params(Some(&json!(["ab".repeat(512 * 1024), "executed"])))
                .unwrap_or_else(|error| panic!("{error:?}"))
                .0
                .len(),
            512 * 1024
        );
        assert_eq!(
            send_params(Some(&json!(["ab".repeat(512 * 1024 + 1), "executed"]))),
            Err(-32602)
        );
    }

    #[test]
    fn remaining_read_selectors_and_unavailability_are_explicit() {
        let id = "ab".repeat(32);
        assert_eq!(selector("lx_listAssets", None), Ok("/v1/assets".into()));
        assert_eq!(
            selector("lx_listAssets", Some(&json!([]))),
            Ok("/v1/assets".into())
        );
        assert_eq!(selector("lx_listAssets", Some(&json!([1]))), Err(-32602));
        assert_eq!(
            selector("lx_getAsset", Some(&json!([id]))),
            Ok(format!("/v1/assets/{id}"))
        );
        for args in [
            json!([]),
            json!([id, id]),
            json!(["../"]),
            json!(["00".repeat(32)]),
            json!({}),
        ] {
            assert_eq!(selector("lx_getAsset", Some(&args)), Err(-32602));
        }
        assert_eq!(fee_params(Some(&json!(["abcd"]))), Ok(vec![0xab, 0xcd]));
        for args in [
            json!([]),
            json!([""]),
            json!(["x1"]),
            json!(["123"]),
            json!(["abcd", 1]),
            json!({}),
        ] {
            assert_eq!(fee_params(Some(&args)), Err(-32602));
        }
        assert_eq!(
            fee_params(Some(&json!(["ab".repeat(512 * 1024 + 1)]))),
            Err(-32602)
        );
        for (status, code, message) in [
            (400, -32602, "Invalid params"),
            (415, -32602, "Invalid params"),
            (401, -32002, "Insufficient scope"),
            (403, -32002, "Insufficient scope"),
            (404, -32001, "Read unavailable"),
            (503, -32001, "Read unavailable"),
            (429, -32005, "Read unavailable"),
        ] {
            let answer =
                read_response(&json!(7), &response(status, "capability_unavailable", None));
            assert_eq!(answer["id"], 7);
            assert_eq!(answer["error"]["code"], code);
            assert_eq!(answer["error"]["message"], message);
            assert!(answer.get("result").is_none());
        }
        assert_eq!(
            read_response(
                &json!(7),
                &OutgoingResponse {
                    status: 200,
                    body: b"invalid".to_vec(),
                    retry_after: None
                }
            )["error"]["code"],
            -32603
        );
    }

    #[test]
    fn schema_lists_every_public_method() {
        let schema: Value = serde_json::from_slice(include_bytes!("../openrpc.json"))
            .unwrap_or_else(|e| panic!("{e}"));
        let methods = schema["methods"]
            .as_array()
            .unwrap_or_else(|| panic!("methods missing"));
        let published = [
            "lx_register",
            "lx_requestFunds",
            "lx_getAccount",
            "lx_getBalance",
            "lx_getBalances",
            "lx_getSequence",
            "lx_estimateFee",
            "lx_sendActivity",
            "lx_getReceipt",
            "lx_getActivityStatus",
            "lx_getBatchHeader",
            "lx_getCheckpoint",
            "lx_getProof",
            "lx_listAssets",
            "lx_getAsset",
            "lx_getNodeInfo",
            "lx_subscribe",
            "lx_unsubscribe",
        ];
        for name in published {
            assert_eq!(
                methods
                    .iter()
                    .filter(|method| method["name"] == name)
                    .count(),
                1,
                "{name}"
            );
        }
        assert_eq!(methods.len(), published.len());
    }

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
