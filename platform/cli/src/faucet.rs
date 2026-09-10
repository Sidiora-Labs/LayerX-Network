use clap::Args;
use serde_json::{json, Value};

use crate::config::Configuration;
use crate::output::CommandOutput;

#[derive(Args)]
pub struct FaucetArgs {
    /// Local key whose DID and Ed25519 public key receive the testnet grant.
    #[arg(long)]
    key: Option<String>,
}

pub fn request(did: &str, public_key: &str) -> Value {
    json!({
        "jsonrpc": "2.0",
        "id": "faucet",
        "method": "lx_requestFunds",
        "params": [did, public_key],
    })
}

pub fn decode(response: &Value) -> Result<Value, String> {
    if response.get("jsonrpc").and_then(Value::as_str) != Some("2.0") {
        return Err("faucet response is not a JSON-RPC 2.0 envelope".into());
    }
    if let Some(failure) = response.get("error") {
        let code = failure
            .get("code")
            .and_then(Value::as_i64)
            .unwrap_or_default();
        let message = failure
            .get("message")
            .and_then(Value::as_str)
            .unwrap_or("faucet claim refused");
        let reason = failure
            .pointer("/data/code")
            .and_then(Value::as_str)
            .unwrap_or_default();
        return Err(if reason.is_empty() {
            format!("faucet claim refused ({code}): {message}")
        } else {
            format!("faucet claim refused ({code}): {message} [{reason}]")
        });
    }
    let claim = response
        .get("result")
        .ok_or_else(|| "faucet response carries neither a result nor an error".to_owned())?;
    if claim.get("funded") != Some(&Value::Bool(true)) {
        return Err("the faucet did not confirm this claim as funded".into());
    }
    if claim
        .get("funding_id")
        .and_then(Value::as_str)
        .is_none_or(|id| id.is_empty() || !id.bytes().all(|byte| byte.is_ascii_hexdigit()))
    {
        return Err("the faucet claim carries no funding identifier".into());
    }
    if claim
        .get("transaction_id")
        .and_then(Value::as_str)
        .is_none_or(str::is_empty)
    {
        return Err("the faucet claim carries no funding transaction".into());
    }
    if claim
        .get("amount")
        .and_then(Value::as_str)
        .and_then(|amount| {
            amount
                .parse::<u128>()
                .ok()
                .filter(|value| value.to_string() == amount)
        })
        .is_none_or(|amount| amount == 0)
    {
        return Err("the faucet claim carries no funded amount".into());
    }
    if claim
        .get("network")
        .and_then(Value::as_str)
        .is_none_or(str::is_empty)
    {
        return Err("the faucet claim names no network".into());
    }
    Ok(claim.clone())
}

pub fn run(arguments: &FaucetArgs) -> Result<CommandOutput, String> {
    let configuration = Configuration::load()?;
    let (environment, client) = crate::active_client(&configuration)?;
    let name = arguments
        .key
        .clone()
        .or_else(|| configuration.default_key.clone())
        .ok_or_else(|| {
            "a faucet claim requires a local key; create one with layerx key create <name>"
                .to_owned()
        })?;
    let metadata = configuration
        .keys
        .get(&name)
        .ok_or_else(|| format!("key {name} does not exist"))?;
    let public_key = metadata.public_key.to_ascii_lowercase();
    let response = client.post("/rpc", &request(&metadata.did, &public_key), None)?;
    let claim = decode(&response)?;
    Ok(CommandOutput::new(
        "faucet.claimed",
        format!("Funded {} on {environment}", metadata.did),
        json!({
            "environment": environment,
            "key": name,
            "did": metadata.did,
            "public_key": public_key,
            "claim": claim,
        }),
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn funded() -> Value {
        json!({
            "jsonrpc": "2.0",
            "id": "faucet",
            "result": {
                "funded": true,
                "funding_id": "ef".repeat(32),
                "transaction_id": "cd".repeat(32),
                "amount": "1000000",
                "network": "layerx-testnet",
            },
        })
    }

    #[test]
    fn the_request_is_the_gateway_faucet_method_with_two_positional_arguments() {
        let key = "ab".repeat(32);
        let envelope = request("did:layerx:alice", &key);
        assert_eq!(envelope["jsonrpc"], "2.0");
        assert_eq!(envelope["method"], "lx_requestFunds");
        assert_eq!(envelope["params"][0], "did:layerx:alice");
        assert_eq!(envelope["params"][1], key);
        assert_eq!(envelope["params"].as_array().map(Vec::len), Some(2));
    }

    #[test]
    fn only_a_confirmed_funded_claim_is_reported() {
        let claim = decode(&funded()).unwrap_or_else(|error| panic!("{error}"));
        assert_eq!(claim["funded"], true);
        assert_eq!(claim["amount"], "1000000");
        assert_eq!(claim["network"], "layerx-testnet");

        for (response, fragment) in [
            (
                json!({"jsonrpc":"2.0","id":"faucet","result":{"funded":false,"funding_id":"ef","transaction_id":"cd","amount":"1","network":"n"}}),
                "did not confirm this claim as funded",
            ),
            (
                json!({"jsonrpc":"2.0","id":"faucet","result":{"funded":true,"transaction_id":"cd","amount":"1","network":"n"}}),
                "no funding identifier",
            ),
            (
                json!({"jsonrpc":"2.0","id":"faucet","result":{"funded":true,"funding_id":"zz","transaction_id":"cd","amount":"1","network":"n"}}),
                "no funding identifier",
            ),
            (
                json!({"jsonrpc":"2.0","id":"faucet","result":{"funded":true,"funding_id":"ef","amount":"1","network":"n"}}),
                "no funding transaction",
            ),
            (
                json!({"jsonrpc":"2.0","id":"faucet","result":{"funded":true,"funding_id":"ef","transaction_id":"cd","amount":"0","network":"n"}}),
                "no funded amount",
            ),
            (
                json!({"jsonrpc":"2.0","id":"faucet","result":{"funded":true,"funding_id":"ef","transaction_id":"cd","amount":1,"network":"n"}}),
                "no funded amount",
            ),
            (
                json!({"jsonrpc":"2.0","id":"faucet","result":{"funded":true,"funding_id":"ef","transaction_id":"cd","amount":"1"}}),
                "names no network",
            ),
            (
                json!({"jsonrpc":"2.0","id":"faucet"}),
                "neither a result nor an error",
            ),
            (json!({"id":"faucet","result":{}}), "JSON-RPC 2.0 envelope"),
        ] {
            let failure = decode(&response)
                .err()
                .unwrap_or_else(|| panic!("accepted {response}"));
            assert!(failure.contains(fragment), "{failure}");
        }
    }

    #[test]
    fn gateway_refusals_are_reported_with_their_code_and_reason() {
        let refusal = json!({
            "jsonrpc": "2.0",
            "id": "faucet",
            "error": {
                "code": -32005,
                "message": "Faucet claim unavailable",
                "data": {"code": "faucet_rate_limit"},
            },
        });
        let failure = decode(&refusal)
            .err()
            .unwrap_or_else(|| panic!("accepted a refusal"));
        assert!(failure.contains("-32005"), "{failure}");
        assert!(failure.contains("Faucet claim unavailable"), "{failure}");
        assert!(failure.contains("faucet_rate_limit"), "{failure}");

        let bare = json!({
            "jsonrpc": "2.0",
            "id": "faucet",
            "error": {"code": -32602, "message": "Invalid params"},
        });
        assert_eq!(
            decode(&bare).err(),
            Some("faucet claim refused (-32602): Invalid params".to_owned())
        );
    }
}
