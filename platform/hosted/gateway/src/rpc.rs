use super::{
    json_response, media_type_is, parse_hex32, public_reads, response, Config, IncomingRequest,
    OutgoingResponse,
};
use serde_json::{json, Value};

const LIST_ASSETS_DEFAULT_PAGE: usize = 64;
const LIST_ASSETS_MAX_PAGE: usize = 256;
const PROGRAM_EVENTS_MAX_PAGE: u64 = 256;
const PROGRAM_EVENT_MAX_TOPIC_BYTES: usize = 64;
const PROGRAM_EVENT_MAX_DATA_BYTES: usize = 65_536;

#[derive(Debug, Eq, PartialEq)]
struct ProgramEventsQuery {
    topic: String,
    from_sequence: u64,
    limit: u64,
}

impl ProgramEventsQuery {
    fn path(&self) -> String {
        format!(
            "/v1/programs/events/{}/{}/{}",
            self.topic, self.from_sequence, self.limit
        )
    }
}

fn lower_hex(value: &str, maximum: usize) -> Option<Vec<u8>> {
    if !value
        .bytes()
        .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
    {
        return None;
    }
    super::decode_hex(value, maximum).ok()
}

fn program_events_params(params: Option<&Value>) -> Result<ProgramEventsQuery, i32> {
    let Some(Value::Array(args)) = params else {
        return Err(-32602);
    };
    let [Value::Object(query)] = args.as_slice() else {
        return Err(-32602);
    };
    if query.len() != 3 {
        return Err(-32602);
    }
    let topic = query
        .get("topic")
        .and_then(Value::as_str)
        .filter(|topic| {
            lower_hex(topic, PROGRAM_EVENT_MAX_TOPIC_BYTES).is_some_and(|bytes| !bytes.is_empty())
        })
        .ok_or(-32602)?;
    let from_sequence = query
        .get("from_sequence")
        .and_then(Value::as_u64)
        .ok_or(-32602)?;
    let limit = query
        .get("limit")
        .and_then(Value::as_u64)
        .filter(|limit| (1..=PROGRAM_EVENTS_MAX_PAGE).contains(limit))
        .ok_or(-32602)?;
    Ok(ProgramEventsQuery {
        topic: topic.to_owned(),
        from_sequence,
        limit,
    })
}

fn program_events_page(query: &ProgramEventsQuery, result: &Value) -> Option<Value> {
    let next = result
        .get("next_sequence")
        .and_then(Value::as_u64)
        .filter(|next| *next >= query.from_sequence)?;
    let events = result
        .get("events")
        .and_then(Value::as_array)
        .filter(|events| u64::try_from(events.len()).is_ok_and(|count| count <= query.limit))?;
    let mut previous = None;
    let mut page = Vec::with_capacity(events.len());
    for event in events {
        let sequence = event
            .get("sequence")
            .and_then(Value::as_u64)
            .filter(|sequence| (query.from_sequence..next).contains(sequence))
            .filter(|sequence| previous.is_none_or(|previous| previous < *sequence))?;
        let program_id = event
            .get("program_id")
            .and_then(Value::as_str)
            .filter(|id| lower_hex(id, 32).is_some_and(|bytes| bytes.len() == 32))
            .filter(|id| *id != "00".repeat(32))?;
        let topic = event
            .get("topic")
            .and_then(Value::as_str)
            .filter(|topic| *topic == query.topic)?;
        let data = event
            .get("data")
            .and_then(Value::as_str)
            .filter(|data| lower_hex(data, PROGRAM_EVENT_MAX_DATA_BYTES).is_some())?;
        previous = Some(sequence);
        page.push(json!({
            "sequence": sequence, "program_id": program_id, "topic": topic, "data": data
        }));
    }
    Some(json!({"events": page, "next_sequence": next}))
}

fn program_events_answer(
    id: &Value,
    query: &ProgramEventsQuery,
    upstream: &OutgoingResponse,
) -> Value {
    let answer = read_response(id, upstream);
    let Some(result) = answer.get("result") else {
        return answer;
    };
    program_events_page(query, result).map_or_else(
        || error(id, -32603, "Invalid upstream response"),
        |page| json!({"jsonrpc":"2.0", "id":id, "result":page}),
    )
}

fn program_events(config: &Config, id: &Value, params: Option<&Value>) -> Value {
    match program_events_params(params) {
        Ok(query) => program_events_answer(id, &query, &public_reads::read(config, &query.path())),
        Err(code) => error(id, code, "Invalid params"),
    }
}

pub(super) fn error(id: &Value, code: i32, message: &str) -> Value {
    json!({"jsonrpc":"2.0", "id":id, "error":{"code":code,"message":message}})
}

fn list_assets_params(params: Option<&Value>) -> Result<(Option<String>, usize), i32> {
    let empty = Vec::new();
    let args = match params {
        None => &empty,
        Some(Value::Array(args)) => args,
        _ => return Err(-32602),
    };
    let (cursor, limit) = match args.as_slice() {
        [] => (None, None),
        [cursor] => (Some(cursor), None),
        [cursor, limit] => (Some(cursor), Some(limit)),
        _ => return Err(-32602),
    };
    let cursor = match cursor {
        None | Some(Value::Null) => None,
        Some(Value::String(cursor)) => {
            if parse_hex32(cursor).is_err() || cursor == &"00".repeat(32) {
                return Err(-32602);
            }
            Some(cursor.to_ascii_lowercase())
        }
        Some(_) => return Err(-32602),
    };
    let limit = match limit {
        None => LIST_ASSETS_DEFAULT_PAGE,
        Some(Value::Number(limit)) => {
            let Some(limit) = limit
                .as_u64()
                .filter(|count| *count >= 1 && *count <= LIST_ASSETS_MAX_PAGE as u64)
            else {
                return Err(-32602);
            };
            limit as usize
        }
        Some(_) => return Err(-32602),
    };
    Ok((cursor, limit))
}

fn paginate_assets(result: &mut Value, cursor: Option<&str>, limit: usize) -> Result<(), i32> {
    let (page, truncated) = {
        let Some(assets) = result.get("assets").and_then(Value::as_array) else {
            return Err(-32603);
        };
        let mut ordered: Vec<(&str, &Value)> = Vec::with_capacity(assets.len());
        for asset in assets {
            let Some(id) = asset.get("asset_id").and_then(Value::as_str) else {
                return Err(-32603);
            };
            ordered.push((id, asset));
        }
        ordered.sort_by(|left, right| left.0.cmp(right.0));
        let start = match cursor {
            Some(cursor) => ordered.partition_point(|(id, _)| *id <= cursor),
            None => 0,
        };
        let remaining = &ordered[start..];
        (
            remaining
                .iter()
                .take(limit)
                .map(|(_, asset)| (*asset).clone())
                .collect::<Vec<Value>>(),
            remaining.len() > limit,
        )
    };
    let next_cursor = if truncated {
        page.last()
            .and_then(|asset| asset.get("asset_id"))
            .cloned()
            .unwrap_or(Value::Null)
    } else {
        Value::Null
    };
    result["assets"] = Value::Array(page);
    result["next_cursor"] = next_cursor;
    Ok(())
}

fn list_assets(config: &Config, id: &Value, params: Option<&Value>) -> Value {
    let (cursor, limit) = match list_assets_params(params) {
        Ok(page) => page,
        Err(code) => return error(id, code, "Invalid params"),
    };
    let mut answer = read_response(id, &public_reads::read(config, "/v1/assets"));
    let Some(result) = answer.get_mut("result") else {
        return answer;
    };
    match paginate_assets(result, cursor.as_deref(), limit) {
        Ok(()) => answer,
        Err(code) => error(id, code, "Invalid upstream response"),
    }
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
    if method == "lx_listAssets" {
        list_assets_params(params)?;
        return Ok("/v1/assets".into());
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
    if let Some(result) = crate::history::dispatch(config, method, &id, value.get("params")) {
        return value.get("id").map(|_| result);
    }
    if let Some(result) = crate::paxeer::dispatch(config, method, &id, value.get("params")) {
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
    if method == "lx_listAssets" {
        let result = list_assets(config, &id, value.get("params"));
        return value.get("id").map(|_| result);
    }
    if method == "lx_getProgramEvents" {
        let result = program_events(config, &id, value.get("params"));
        return value.get("id").map(|_| result);
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

    fn fixture(bytes: &[u8]) -> Value {
        serde_json::from_slice(bytes).unwrap_or_else(|error| panic!("fixture: {error}"))
    }

    fn exchanges(document: &Value, pointer: &str) -> Vec<Value> {
        document
            .pointer(pointer)
            .and_then(Value::as_array)
            .unwrap_or_else(|| panic!("{pointer} missing"))
            .clone()
    }

    fn replay(exchange: &Value) -> (ProgramEventsQuery, Value) {
        assert_eq!(exchange["method"], "lx_getProgramEvents");
        let query = program_events_params(exchange.get("params"))
            .unwrap_or_else(|code| panic!("params refused with {code}"));
        assert_eq!(query.path(), exchange["upstream"]["path"]);
        let upstream = json_response(
            u16::try_from(
                exchange["upstream"]["status"]
                    .as_u64()
                    .unwrap_or_else(|| panic!("upstream status missing")),
            )
            .unwrap_or_else(|error| panic!("{error}")),
            &exchange["upstream"]["body"],
        );
        let answer = program_events_answer(&json!(9), &query, &upstream);
        assert_eq!(answer["jsonrpc"], "2.0");
        assert_eq!(answer["id"], 9);
        (query, answer)
    }

    #[test]
    fn program_events_replay_the_recorded_upstream_exchange() {
        let recorded = fixture(include_bytes!("../tests/fixtures/program-events.json"));
        for exchange in exchanges(&recorded, "/exchanges") {
            let (_, answer) = replay(&exchange);
            if let Some(result) = exchange.get("result") {
                assert_eq!(&answer["result"], result);
                assert!(answer.get("error").is_none());
            } else {
                assert_eq!(answer["error"], exchange["error"]);
                assert!(answer.get("result").is_none());
            }
        }
    }

    #[test]
    fn program_events_answer_in_the_shape_the_kernel_watcher_pins() {
        let watch = fixture(include_bytes!(
            "../../../../interop/crates/x-websearch/tests/fixtures/kernel/watch.json"
        ));
        let recorded = fixture(include_bytes!("../tests/fixtures/program-events.json"));
        let recorded = exchanges(&recorded, "/exchanges");
        let pinned = exchanges(&watch, "/endpoints/gateway/*");
        let mut matched = 0;
        for call in &pinned {
            assert_eq!(call["method"], "lx_getProgramEvents");
            let query = program_events_params(call.get("params"))
                .unwrap_or_else(|code| panic!("sidecar params refused with {code}"));
            let Some(result) = call.get("result") else {
                continue;
            };
            let consistent = program_events_page(&query, result);
            let events = result["events"]
                .as_array()
                .unwrap_or_else(|| panic!("events missing"));
            if events.iter().all(|event| event["topic"] == query.topic) {
                assert_eq!(consistent.as_ref(), Some(result));
            } else {
                assert_eq!(consistent, None);
            }
            for exchange in recorded
                .iter()
                .filter(|exchange| exchange["params"] == call["params"])
            {
                let (_, answer) = replay(exchange);
                if let Some(served) = answer.get("result") {
                    assert_eq!(served, result);
                    for event in served["events"]
                        .as_array()
                        .unwrap_or_else(|| panic!("events missing"))
                    {
                        let mut keys: Vec<_> = event
                            .as_object()
                            .unwrap_or_else(|| panic!("event is not an object"))
                            .keys()
                            .map(String::as_str)
                            .collect();
                        keys.sort_unstable();
                        assert_eq!(keys, ["data", "program_id", "sequence", "topic"]);
                    }
                    matched += 1;
                }
            }
        }
        assert_eq!(matched, 2);
    }

    #[test]
    fn program_events_cursor_advances_and_the_limit_is_honoured() {
        let recorded = fixture(include_bytes!("../tests/fixtures/program-events.json"));
        let paged: Vec<_> = exchanges(&recorded, "/exchanges")
            .into_iter()
            .filter(|exchange| {
                exchange["params"][0]["limit"] == 1 && exchange.get("result").is_some()
            })
            .collect();
        assert_eq!(paged.len(), 2);
        let mut cursor = 40;
        let mut sequences = Vec::new();
        for exchange in &paged {
            let (query, answer) = replay(exchange);
            assert_eq!(query.from_sequence, cursor);
            let events = answer["result"]["events"]
                .as_array()
                .unwrap_or_else(|| panic!("events missing"));
            assert!(u64::try_from(events.len()).is_ok_and(|count| count <= query.limit));
            let next = answer["result"]["next_sequence"]
                .as_u64()
                .unwrap_or_else(|| panic!("next_sequence missing"));
            assert!(next > cursor);
            sequences.extend(events.iter().filter_map(|event| event["sequence"].as_u64()));
            cursor = next;
        }
        assert_eq!(sequences, [41, 43]);
        assert_eq!(cursor, 44);
        let query = ProgramEventsQuery {
            topic: "504158454552585f5745425f524551554553545f5631".into(),
            from_sequence: 40,
            limit: 2,
        };
        let event = |sequence: u64, topic: &str| json!({"sequence": sequence, "program_id": "ab".repeat(32), "topic": topic, "data": "01"});
        let topic = query.topic.clone();
        assert!(program_events_page(
            &query,
            &json!({"events": [event(40, &topic), event(41, &topic)], "next_sequence": 42})
        )
        .is_some());
        for refused in [
            json!({"events": [event(40, &topic), event(41, &topic), event(42, &topic)],
                "next_sequence": 43}),
            json!({"events": [], "next_sequence": 39}),
            json!({"events": [event(44, &topic)], "next_sequence": 44}),
            json!({"events": [event(39, &topic)], "next_sequence": 44}),
            json!({"events": [event(42, &topic), event(41, &topic)], "next_sequence": 44}),
            json!({"events": [event(41, &topic), event(41, &topic)], "next_sequence": 44}),
            json!({"events": [event(41, "00")], "next_sequence": 44}),
            json!({"events": [{"sequence": 41, "program_id": "00".repeat(32), "topic": topic,
                "data": "01"}], "next_sequence": 44}),
            json!({"events": [{"sequence": 41, "program_id": "AB".repeat(32), "topic": topic,
                "data": "01"}], "next_sequence": 44}),
            json!({"events": [{"sequence": 41, "program_id": "ab".repeat(32), "topic": topic,
                "data": "0"}], "next_sequence": 44}),
            json!({"events": [{"sequence": 41, "program_id": "ab".repeat(32), "topic": topic,
                "data": "ab".repeat(PROGRAM_EVENT_MAX_DATA_BYTES + 1)}], "next_sequence": 44}),
            json!({"events": [{"sequence": 41, "program_id": "ab".repeat(32), "topic": topic}],
                "next_sequence": 44}),
            json!({"events": {}, "next_sequence": 44}),
            json!({"events": []}),
        ] {
            assert_eq!(program_events_page(&query, &refused), None, "{refused}");
        }
        let trimmed = program_events_page(
            &query,
            &json!({"events": [{"sequence": 41, "program_id": "ab".repeat(32), "topic": topic,
                "data": "01", "extra": true}], "next_sequence": 42}),
        )
        .unwrap_or_else(|| panic!("page refused"));
        assert!(trimmed["events"][0].get("extra").is_none());
    }

    #[test]
    fn program_events_params_are_exact() {
        let topic = "504158454552585f5745425f524551554553545f5631";
        assert_eq!(
            program_events_params(Some(
                &json!([{"topic": topic, "from_sequence": 0, "limit": 1}])
            )),
            Ok(ProgramEventsQuery {
                topic: topic.into(),
                from_sequence: 0,
                limit: 1
            })
        );
        assert_eq!(
            program_events_params(Some(
                &json!([{"topic": "ab".repeat(64), "from_sequence": u64::MAX, "limit": 256}])
            ))
            .map(|query| query.path()),
            Ok(format!(
                "/v1/programs/events/{}/{}/256",
                "ab".repeat(64),
                u64::MAX
            ))
        );
        for params in [
            json!([]),
            json!({}),
            json!([{"topic": topic, "from_sequence": 0}]),
            json!([{"topic": topic, "from_sequence": 0, "limit": 0}]),
            json!([{"topic": topic, "from_sequence": 0, "limit": 257}]),
            json!([{"topic": topic, "from_sequence": -1, "limit": 1}]),
            json!([{"topic": topic, "from_sequence": "1", "limit": 1}]),
            json!([{"topic": "", "from_sequence": 0, "limit": 1}]),
            json!([{"topic": "ABCD", "from_sequence": 0, "limit": 1}]),
            json!([{"topic": "abc", "from_sequence": 0, "limit": 1}]),
            json!([{"topic": "+a", "from_sequence": 0, "limit": 1}]),
            json!([{"topic": "../", "from_sequence": 0, "limit": 1}]),
            json!([{"topic": "ab".repeat(65), "from_sequence": 0, "limit": 1}]),
            json!([{"topic": topic, "from_sequence": 0, "limit": 1, "extra": 1}]),
            json!([{"topic": topic, "from_sequence": 0, "limit": 1}, 1]),
        ] {
            assert_eq!(
                program_events_params(Some(&params)),
                Err(-32602),
                "{params}"
            );
        }
        assert_eq!(program_events_params(None), Err(-32602));
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
            "px_resolveAccount",
            "px_getAccount",
            "px_getBalances",
            "px_listAssets",
            "px_getNetwork",
            "lx_getHistory",
            "px_getHistory",
            "px_getUnifiedHistory",
            "px_getCapabilities",
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
