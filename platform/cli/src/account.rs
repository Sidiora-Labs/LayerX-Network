use serde_json::{json, Value};

use crate::config::KeyMetadata;
use crate::http::Client;

pub fn create(
    client: &Client,
    environment: &str,
    key: Option<&KeyMetadata>,
    initial_amount: &str,
    email: Option<&str>,
    display_name: Option<&str>,
    idempotency_key: Option<&str>,
) -> Result<Value, String> {
    let amount = initial_amount
        .parse::<u128>()
        .map_err(|_| "initial amount must be an unsigned protocol integer".to_string())?;
    if environment == "emulator" {
        let key =
            key.ok_or_else(|| "emulator account creation requires a local key".to_string())?;
        let amount_hi = u64::try_from(amount >> 64)
            .map_err(|_| "initial amount high word does not fit u64".to_string())?;
        let amount_lo = u64::try_from(amount & u128::from(u64::MAX))
            .map_err(|_| "initial amount low word does not fit u64".to_string())?;
        let response = client.post(
            "/__emulator/accounts/prefund",
            &json!({
                "did": key.did,
                "public_key": key.public_key,
                "amount_hi": amount_hi,
                "amount_lo": amount_lo,
            }),
            None,
        )?;
        return Ok(json!({
            "account": format!("agent:{}:main", key.did),
            "environment": environment,
            "registration": response,
            "funding": "emulator-prefund",
        }));
    }
    if amount != 0 {
        return Err(
            "hosted account creation cannot mint an initial balance; use the beta faucet or a deposit"
                .into(),
        );
    }
    let email = email.ok_or_else(|| "hosted account creation requires --email".to_string())?;
    let display_name = display_name
        .ok_or_else(|| "hosted account creation requires --display-name".to_string())?;
    let idempotency = idempotency_key.ok_or_else(|| {
        "hosted account creation requires --idempotency-key so retries cannot duplicate state"
            .to_string()
    })?;
    crate::http::validate_idempotency_key(idempotency)?;
    let response = client.post(
        "/v1/accounts",
        &json!({
            "email": email,
            "display_name": display_name,
        }),
        Some(idempotency),
    )?;
    Ok(response)
}

pub fn get(client: &Client, environment: &str, did: Option<&str>) -> Result<Value, String> {
    if environment == "emulator" {
        let did = did.ok_or_else(|| "emulator account lookup requires --did".to_string())?;
        let state = client.get("/v1/state")?;
        let expected = format!("agent:{did}:main");
        let account = state
            .pointer("/result/accounts")
            .and_then(Value::as_array)
            .and_then(|accounts| {
                accounts
                    .iter()
                    .find(|account| account.get("name").and_then(Value::as_str) == Some(&expected))
            })
            .cloned()
            .ok_or_else(|| format!("account {expected} is not present in the emulator"))?;
        return Ok(json!({"account": account, "environment": environment}));
    }
    client.get("/v1/profile")
}
