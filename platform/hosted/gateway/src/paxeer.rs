//! The Paxeer half of the single network endpoint: byte-faithful EVM JSON-RPC
//! relay and the unified `px_*` reads that join a Paxeer account, asset or
//! head with its `LayerX` counterpart.
//!
//! The relay target is the Paxeer boundary's JSON-RPC service, which fronts
//! the chain's `paxd` RPC. Reads here are the same public tier that answers
//! `lx_*` reads: no key, one public read budget, and every answer is either
//! the node's own or an explicit refusal.

use super::{public_reads, Config};
use layerx_platform_gateway::evm;
use layerx_platform_gateway::http::OutboundRequest;
use serde_json::{json, Map, Value};

/// Assets joined per `px_getBalances` and `px_listAssets` answer.
const MAX_JOINED_ASSETS: usize = 16;

pub(super) fn configured_endpoint() -> Result<Option<super::Endpoint>, String> {
    std::env::var("LAYERX_GATEWAY_PAXEER_RPC_URL")
        .ok()
        .map(|url| super::Endpoint::parse(&url))
        .transpose()
}

pub(super) fn unconfigured(id: &Value) -> Value {
    let mut refusal = super::rpc::error(id, -32001, "Paxeer endpoint not configured");
    refusal["error"]["data"] = json!({"code": "paxeer_rpc_not_configured"});
    refusal
}

pub(super) fn unavailable(id: &Value, code: &str) -> Value {
    let mut refusal = super::rpc::error(id, -32001, "Paxeer read unavailable");
    refusal["error"]["data"] = json!({"code": code});
    refusal
}

/// Sends one JSON-RPC request to the Paxeer node and returns its answer
/// document unchanged.
pub(super) fn node(config: &Config, request: &Value) -> Result<Value, &'static str> {
    let Some(endpoint) = &config.paxeer else {
        return Err("paxeer_rpc_not_configured");
    };
    if !public_reads::consume_read() {
        return Err("public_read_rate_limit");
    }
    let body = serde_json::to_vec(request).map_err(|_| "invalid_paxeer_request")?;
    let upstream = config
        .client
        .request_unauthenticated(
            endpoint,
            &OutboundRequest {
                method: "POST",
                path: "/",
                idempotency: None,
                content_type: "application/json",
                body: &body,
            },
        )
        .map_err(|_| "paxeer_unreachable")?;
    if upstream.status != 200 || upstream.content_type != "application/json" {
        return Err("paxeer_node_unavailable");
    }
    let document: Value =
        serde_json::from_slice(&upstream.body).map_err(|_| "invalid_paxeer_response")?;
    if document.get("jsonrpc") != Some(&json!("2.0"))
        || document.get("id") != request.get("id")
        || document.get("result").is_some() == document.get("error").is_some()
    {
        return Err("invalid_paxeer_response");
    }
    Ok(document)
}

/// Relays one EVM request under the caller's own identifier.
pub(super) fn relay(config: &Config, id: &Value, method: &str, params: Option<&Value>) -> Value {
    if config.paxeer.is_none() {
        return unconfigured(id);
    }
    if !evm::relayable_method(method) {
        if evm::SUBSCRIPTION_METHODS.contains(&method) {
            return super::rpc::error(id, -32004, "Subscriptions are not relayed");
        }
        return super::rpc::error(id, -32601, "Method not relayed");
    }
    let mut request = Map::new();
    request.insert("jsonrpc".into(), json!("2.0"));
    request.insert("id".into(), id.clone());
    request.insert("method".into(), json!(method));
    match params {
        None => {}
        Some(params @ (Value::Array(_) | Value::Object(_))) => {
            request.insert("params".into(), params.clone());
        }
        Some(_) => return super::rpc::error(id, -32602, "Invalid params"),
    }
    match node(config, &Value::Object(request)) {
        Ok(answer) => answer,
        Err(code) => unavailable(id, code),
    }
}

/// One `eth_call` against a native precompile, returning its raw answer bytes.
fn call(config: &Config, to: &[u8; 20], data: &[u8]) -> Result<Vec<u8>, &'static str> {
    let request = json!({
        "jsonrpc": "2.0",
        "id": "px-call",
        "method": "eth_call",
        "params": [{"to": evm::address_hex(to), "data": format!("0x{}", super::hex(data))}, "latest"]
    });
    let answer = node(config, &request)?;
    let Some(result) = answer.get("result").and_then(Value::as_str) else {
        return Err("paxeer_call_refused");
    };
    super::decode_hex(result.strip_prefix("0x").unwrap_or(result), 128 * 1024)
        .map_err(|_| "invalid_paxeer_response")
}

fn quantity(config: &Config, method: &str, params: Value) -> Result<String, &'static str> {
    let request = json!({"jsonrpc":"2.0","id":"px-read","method":method,"params":params});
    let answer = node(config, &request)?;
    answer
        .get("result")
        .and_then(Value::as_str)
        .filter(|value| value.starts_with("0x") && value.len() <= 66)
        .map(str::to_owned)
        .ok_or("invalid_paxeer_response")
}

fn hex32(bytes: &[u8; 32]) -> String {
    super::hex(bytes)
}

fn is_zero(bytes: &[u8]) -> bool {
    bytes.iter().all(|byte| *byte == 0)
}

/// One account as both domains know it.
struct Resolution {
    evm: Option<[u8; 20]>,
    pax_address: Option<String>,
    did_public_key: Option<[u8; 32]>,
    layerx_account: Option<[u8; 32]>,
}

impl Resolution {
    fn document(&self) -> Value {
        json!({
            "evm_address": self.evm.as_ref().map(evm::address_hex),
            "pax_address": self.pax_address,
            "layerx_did": self.did_public_key.map(|key| format!("did:layerx:{}", hex32(&key))),
            "layerx_account": self.layerx_account.as_ref().map(hex32),
            "bound": self.did_public_key.is_some()
        })
    }

    fn did(&self) -> Option<String> {
        self.did_public_key
            .map(|key| format!("did:layerx:{}", hex32(&key)))
    }
}

pub(super) fn no_params(params: Option<&Value>) -> bool {
    match params {
        None | Some(Value::Null) => true,
        Some(Value::Array(args)) => args.is_empty(),
        Some(_) => false,
    }
}

fn account_selector(params: Option<&Value>) -> Result<Selector, i32> {
    let Some(Value::Array(args)) = params else {
        return Err(-32602);
    };
    let [Value::String(account)] = args.as_slice() else {
        return Err(-32602);
    };
    if let Some(address) = evm::parse_address(account) {
        if account.starts_with("0x") {
            return Ok(Selector::Evm(address));
        }
    }
    let key = account
        .strip_prefix("did:layerx:")
        .unwrap_or(account.as_str());
    let key = super::parse_hex32(key).map_err(|_| -32602)?;
    if is_zero(&key) {
        return Err(-32602);
    }
    Ok(Selector::LayerX(key))
}

enum Selector {
    Evm([u8; 20]),
    LayerX([u8; 32]),
}

fn resolve(config: &Config, selector: &Selector) -> Result<Resolution, &'static str> {
    let address = match selector {
        Selector::Evm(address) => *address,
        Selector::LayerX(key) => {
            let answer = call(
                config,
                &evm::ADDR_PRECOMPILE,
                &evm::calldata_word(evm::SELECTOR_GET_EVM_ADDR_BY_LAYERX, key),
            )?;
            let bound = evm::decode_address(&answer).map_err(|_| "invalid_paxeer_response")?;
            if is_zero(&bound) {
                return Ok(Resolution {
                    evm: None,
                    pax_address: None,
                    did_public_key: Some(*key),
                    layerx_account: None,
                });
            }
            bound
        }
    };
    let answer = call(
        config,
        &evm::ADDR_PRECOMPILE,
        &evm::calldata_address(evm::SELECTOR_GET_UNIFIED_ACCOUNT, &address),
    )?;
    let unified = evm::decode_unified_account(&answer).map_err(|_| "invalid_paxeer_response")?;
    let bound = !is_zero(&unified.did_public_key);
    Ok(Resolution {
        evm: Some(address),
        pax_address: Some(unified.pax_address).filter(|value| !value.is_empty()),
        did_public_key: bound.then_some(unified.did_public_key),
        layerx_account: (bound && !is_zero(&unified.layerx_main_account_id))
            .then_some(unified.layerx_main_account_id),
    })
}

fn resolved(config: &Config, id: &Value, params: Option<&Value>) -> Result<Resolution, Value> {
    if config.paxeer.is_none() {
        return Err(unconfigured(id));
    }
    let selector = account_selector(params).map_err(|code| {
        super::rpc::error(
            id,
            code,
            if code == -32602 {
                "Invalid params"
            } else {
                "Read unavailable"
            },
        )
    })?;
    resolve(config, &selector).map_err(|code| unavailable(id, code))
}

/// The indexer account keys one unified account answers to: its EVM
/// address on the Paxeer side and every `LayerX` account its DID holds (the
/// bound main account plus the public core's per-asset accounts), with the
/// resolution document naming both halves.
pub(super) fn history_accounts(
    config: &Config,
    id: &Value,
    account: &str,
) -> Result<(Value, Vec<(&'static str, String)>), Value> {
    let resolution = resolved(config, id, Some(&json!([account])))?;
    let mut accounts: Vec<(&'static str, String)> = Vec::new();
    if let Some(address) = &resolution.evm {
        accounts.push(("paxeer", evm::address_hex(address)));
    }
    if let Some(main) = &resolution.layerx_account {
        accounts.push(("layerx", hex32(main)));
    }
    if let Some(listing) = resolution
        .did()
        .and_then(|did| super::rpc::read_result(config, &format!("/v1/dids/{did}/accounts")))
    {
        for held in listing
            .get("accounts")
            .and_then(Value::as_array)
            .map(Vec::as_slice)
            .unwrap_or_default()
        {
            if let Some(key) = held
                .get("account_id")
                .and_then(Value::as_str)
                .and_then(|key| super::parse_hex32(key).ok())
                .filter(|key| !is_zero(key))
            {
                accounts.push(("layerx", hex32(&key)));
            }
        }
    }
    let mut seen = std::collections::BTreeSet::new();
    accounts.retain(|(_, key)| seen.insert(key.clone()));
    accounts.truncate(MAX_JOINED_ASSETS);
    Ok((resolution.document(), accounts))
}

fn resolve_account(config: &Config, id: &Value, params: Option<&Value>) -> Value {
    match resolved(config, id, params) {
        Ok(resolution) => json!({"jsonrpc":"2.0","id":id,"result":resolution.document()}),
        Err(refusal) => refusal,
    }
}

fn get_account(config: &Config, id: &Value, params: Option<&Value>) -> Value {
    let resolution = match resolved(config, id, params) {
        Ok(resolution) => resolution,
        Err(refusal) => return refusal,
    };
    let paxeer = match resolution.evm {
        None => Value::Null,
        Some(address) => {
            let address = evm::address_hex(&address);
            let balance =
                match quantity(config, "eth_getBalance", json!([address.clone(), "latest"])) {
                    Ok(balance) => balance,
                    Err(code) => return unavailable(id, code),
                };
            let nonce = match quantity(
                config,
                "eth_getTransactionCount",
                json!([address.clone(), "latest"]),
            ) {
                Ok(nonce) => nonce,
                Err(code) => return unavailable(id, code),
            };
            json!({"address": address, "balance": balance, "nonce": nonce})
        }
    };
    let layerx = resolution
        .layerx_account
        .and_then(|account| {
            super::rpc::read_result(config, &format!("/v1/accounts/{}", hex32(&account)))
        })
        .unwrap_or(Value::Null);
    json!({
        "jsonrpc": "2.0",
        "id": id,
        "result": {"account": resolution.document(), "paxeer": paxeer, "layerx": layerx}
    })
}

fn layerx_assets(config: &Config) -> Option<Vec<Value>> {
    let assets = super::rpc::read_result(config, "/v1/assets")?;
    let mut records: Vec<Value> = assets.get("assets")?.as_array()?.clone();
    records.sort_by(|left, right| {
        left.get("asset_id")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .cmp(
                right
                    .get("asset_id")
                    .and_then(Value::as_str)
                    .unwrap_or_default(),
            )
    });
    records.truncate(MAX_JOINED_ASSETS);
    Some(records)
}

fn custody_asset(config: &Config, asset_id: &str) -> Value {
    let Ok(id) = super::parse_hex32(asset_id) else {
        return Value::Null;
    };
    let Ok(answer) = call(
        config,
        &evm::CUSTODY_PRECOMPILE,
        &evm::calldata_word(evm::SELECTOR_GET_ASSET, &id),
    ) else {
        return Value::Null;
    };
    let Ok(asset) = evm::decode_custody_asset(&answer) else {
        return Value::Null;
    };
    if is_zero(&asset.asset_id) {
        return Value::Null;
    }
    json!({
        "asset_id": hex32(&asset.asset_id),
        "denom": asset.denom,
        "pointer": evm::address_hex(&asset.pointer),
        "enabled": asset.enabled,
        "paused": asset.paused,
        "minimum_deposit": evm::uint256_decimal(&asset.minimum_deposit),
        "custody_cap": evm::uint256_decimal(&asset.custody_cap),
        "custodied": evm::uint256_decimal(&asset.custodied),
        "released": evm::uint256_decimal(&asset.released),
        "pending": evm::uint256_decimal(&asset.pending)
    })
}

fn list_assets(config: &Config, id: &Value, params: Option<&Value>) -> Value {
    if config.paxeer.is_none() {
        return unconfigured(id);
    }
    if !no_params(params) {
        return super::rpc::error(id, -32602, "Invalid params");
    }
    let Some(records) = layerx_assets(config) else {
        return unavailable(id, "layerx_assets_unavailable");
    };
    let mut joined = Vec::with_capacity(records.len());
    for record in records {
        let asset_id = record
            .get("asset_id")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_owned();
        joined.push(json!({
            "asset_id": asset_id,
            "layerx": record,
            "paxeer": custody_asset(config, &asset_id)
        }));
    }
    json!({
        "jsonrpc": "2.0",
        "id": id,
        "result": {"assets": joined, "joined_limit": MAX_JOINED_ASSETS}
    })
}

fn get_balances(config: &Config, id: &Value, params: Option<&Value>) -> Value {
    let resolution = match resolved(config, id, params) {
        Ok(resolution) => resolution,
        Err(refusal) => return refusal,
    };
    let Some(records) = layerx_assets(config) else {
        return unavailable(id, "layerx_assets_unavailable");
    };
    let layerx_accounts = resolution
        .did()
        .and_then(|did| super::rpc::read_result(config, &format!("/v1/dids/{did}/accounts")))
        .unwrap_or(Value::Null);
    let mut balances = Vec::with_capacity(records.len());
    for record in records {
        let asset_id = record
            .get("asset_id")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_owned();
        let custody = custody_asset(config, &asset_id);
        let denom = custody
            .get("denom")
            .and_then(Value::as_str)
            .map(str::to_owned)
            .or_else(|| {
                record
                    .get("denom")
                    .and_then(Value::as_str)
                    .map(str::to_owned)
            });
        let paxeer = match (resolution.evm, denom.as_deref()) {
            (Some(address), Some(denom)) if !denom.is_empty() => {
                match call(
                    config,
                    &evm::BANK_PRECOMPILE,
                    &evm::calldata_address_string(evm::SELECTOR_BANK_BALANCE, &address, denom),
                )
                .ok()
                .and_then(|answer| evm::decode_word(&answer).ok())
                {
                    Some(word) => json!({"denom": denom, "amount": evm::uint256_decimal(&word)}),
                    None => Value::Null,
                }
            }
            _ => Value::Null,
        };
        let layerx = layerx_accounts
            .get("accounts")
            .and_then(Value::as_array)
            .and_then(|accounts| {
                accounts.iter().find(|account| {
                    account.get("asset_id").and_then(Value::as_str) == Some(&asset_id)
                })
            })
            .cloned()
            .unwrap_or(Value::Null);
        balances.push(json!({
            "asset_id": asset_id,
            "denom": denom,
            "custody": custody,
            "paxeer": paxeer,
            "layerx": layerx
        }));
    }
    json!({
        "jsonrpc": "2.0",
        "id": id,
        "result": {
            "account": resolution.document(),
            "balances": balances,
            "joined_limit": MAX_JOINED_ASSETS
        }
    })
}

fn anchor_head(config: &Config) -> Value {
    let Ok(answer) = call(
        config,
        &evm::ANCHOR_PRECOMPILE,
        &evm::calldata_empty(evm::SELECTOR_LATEST_FINALIZED),
    ) else {
        return Value::Null;
    };
    let Ok((batch, exists)) = evm::decode_latest_finalized(&answer) else {
        return Value::Null;
    };
    let status = if exists {
        call(
            config,
            &evm::ANCHOR_PRECOMPILE,
            &evm::calldata_u64(evm::SELECTOR_STATUS_OF, batch),
        )
        .ok()
        .and_then(|answer| evm::decode_u8(&answer).ok())
    } else {
        None
    };
    json!({
        "latest_finalized_batch": exists.then_some(batch),
        "status": status,
        "status_name": status.map(evm::anchor_status_name),
        "status_ladder": {"0": "unknown", "1": "submitted", "2": "final"}
    })
}

fn get_network(config: &Config, id: &Value, params: Option<&Value>) -> Value {
    if config.paxeer.is_none() {
        return unconfigured(id);
    }
    if !no_params(params) {
        return super::rpc::error(id, -32602, "Invalid params");
    }
    let chain_id = match quantity(config, "eth_chainId", json!([])) {
        Ok(chain_id) => chain_id,
        Err(code) => return unavailable(id, code),
    };
    let block = match quantity(config, "eth_blockNumber", json!([])) {
        Ok(block) => block,
        Err(code) => return unavailable(id, code),
    };
    let node_info = super::rpc::read_result(config, "/v1/node-info").unwrap_or(Value::Null);
    json!({
        "jsonrpc": "2.0",
        "id": id,
        "result": {
            "network_id": config.network_id,
            "paxeer": {"chain_id": chain_id, "latest_block": block},
            "layerx": {"node_info": node_info},
            "anchor": anchor_head(config)
        }
    })
}

/// Answers every unified and relayed EVM method, or `None` when the method
/// belongs to another namespace.
pub(super) fn dispatch(
    config: &Config,
    method: &str,
    id: &Value,
    params: Option<&Value>,
) -> Option<Value> {
    Some(match method {
        "px_resolveAccount" => resolve_account(config, id, params),
        "px_getAccount" => get_account(config, id, params),
        "px_getBalances" => get_balances(config, id, params),
        "px_listAssets" => list_assets(config, id, params),
        "px_getNetwork" => get_network(config, id, params),
        "px_getCapabilities" => super::capabilities::get(config, id, params),
        _ if evm::is_evm_namespace(method) => {
            match super::capabilities::gate(config, id, method, params) {
                Some(refusal) => refusal,
                None => relay(config, id, method, params),
            }
        }
        _ => return None,
    })
}

/// The Paxeer relay's state for the gateway's own status document.
pub(super) fn status(config: &Config) -> &'static str {
    if config.paxeer.is_none() {
        return "not_configured";
    }
    if quantity(config, "eth_chainId", json!([])).is_ok() {
        "available"
    } else {
        "unavailable"
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_parameter_lists_are_the_only_accepted_shape() {
        assert!(no_params(None));
        assert!(no_params(Some(&Value::Null)));
        assert!(no_params(Some(&json!([]))));
        for invalid in [json!([1]), json!({}), json!("x"), json!(3)] {
            assert!(!no_params(Some(&invalid)), "{invalid}");
        }
    }

    #[test]
    fn account_selectors_accept_both_domains() {
        let address = "0x102132435465768798a9bacbdcedfe0f1e2d3c4b";
        assert!(matches!(
            account_selector(Some(&json!([address]))),
            Ok(Selector::Evm(_))
        ));
        assert!(matches!(
            account_selector(Some(&json!([format!("did:layerx:{}", "61".repeat(32))]))),
            Ok(Selector::LayerX(_))
        ));
        assert!(matches!(
            account_selector(Some(&json!(["61".repeat(32)]))),
            Ok(Selector::LayerX(_))
        ));
        for invalid in [
            json!([]),
            json!([address, address]),
            json!(["0x1234"]),
            json!(["did:layerx:zz"]),
            json!(["did:layerx:".to_owned() + &"00".repeat(32)]),
            json!([1]),
            json!({}),
        ] {
            assert_eq!(account_selector(Some(&invalid)).err(), Some(-32602));
        }
        assert_eq!(account_selector(None).err(), Some(-32602));
    }

    #[test]
    fn resolution_documents_name_both_halves() {
        let bound = Resolution {
            evm: Some([0x11; 20]),
            pax_address: Some("pax1example".to_owned()),
            did_public_key: Some([0x61; 32]),
            layerx_account: Some([0x7c; 32]),
        };
        let document = bound.document();
        assert_eq!(document["bound"], json!(true));
        assert_eq!(
            document["evm_address"],
            json!(format!("0x{}", "11".repeat(20)))
        );
        assert_eq!(
            document["layerx_did"],
            json!(format!("did:layerx:{}", "61".repeat(32)))
        );
        assert_eq!(document["layerx_account"], json!("7c".repeat(32)));
        assert_eq!(bound.did(), Some(format!("did:layerx:{}", "61".repeat(32))));
        let unbound = Resolution {
            evm: Some([0x11; 20]),
            pax_address: Some("pax1example".to_owned()),
            did_public_key: None,
            layerx_account: None,
        };
        let document = unbound.document();
        assert_eq!(document["bound"], json!(false));
        assert_eq!(document["layerx_did"], Value::Null);
        assert_eq!(document["layerx_account"], Value::Null);
        assert_eq!(unbound.did(), None);
    }
}
