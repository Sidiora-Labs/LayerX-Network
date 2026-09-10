use super::{Config, Endpoint, IncomingRequest};
use crate::rpc::error;
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::sync::Mutex;
use zeroize::Zeroizing;

const URL_VARIABLE: &str = "LAYERX_GATEWAY_FAUCET_URL";
const TOKEN_VARIABLE: &str = "LAYERX_GATEWAY_FAUCET_SERVICE_TOKEN_FILE";
const CLAIM_PATH: &str = "/v1/faucet/service-claims";
const IDEMPOTENCY_DOMAIN: &[u8] = b"layerx-faucet-claim-v1";
const CLAIMS_PER_MINUTE: u32 = 30;

static FAUCET_WINDOW: Mutex<(u64, u32)> = Mutex::new((0, 0));

pub(super) struct Faucet {
    endpoint: Endpoint,
    token: Zeroizing<String>,
}

fn idempotency(principal_digest: &str, did: &str, public_key: &str) -> String {
    let mut hash = Sha256::new();
    for part in [
        IDEMPOTENCY_DOMAIN,
        principal_digest.as_bytes(),
        did.as_bytes(),
        public_key.as_bytes(),
    ] {
        hash.update((part.len() as u64).to_be_bytes());
        hash.update(part);
    }
    super::hex(&hash.finalize())
}

fn path_safe(value: &str, maximum: usize) -> bool {
    !value.is_empty()
        && value.len() <= maximum
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || b"-._:".contains(&byte))
}

fn lowercase_hex32(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

fn method_and_identifier(did: &str) -> Option<(&str, &str)> {
    let (method, identifier) = did.strip_prefix("did:")?.split_once(':')?;
    (!method.is_empty() && !identifier.is_empty()).then_some((method, identifier))
}

fn claim_params(params: Option<&Value>) -> Result<(String, String), i32> {
    let Some(Value::Array(args)) = params else {
        return Err(-32602);
    };
    let [Value::String(did), Value::String(public_key)] = args.as_slice() else {
        return Err(-32602);
    };
    if layerx_types::ids::Did::new(did.as_bytes()).is_err()
        || method_and_identifier(did).is_none()
        || !path_safe(did, 512)
        || !lowercase_hex32(public_key)
        || *public_key == "00".repeat(32)
    {
        return Err(-32602);
    }
    Ok((did.clone(), public_key.clone()))
}

fn consume(window: &mut (u64, u32), minute: u64) -> bool {
    if window.0 != minute {
        *window = (minute, 0);
    }
    if window.1 >= CLAIMS_PER_MINUTE {
        return false;
    }
    window.1 += 1;
    true
}

fn consume_claim() -> bool {
    let Ok(second) = super::now() else {
        return false;
    };
    let Ok(mut window) = FAUCET_WINDOW.lock() else {
        return false;
    };
    consume(&mut window, second / 60)
}

fn refusal(id: &Value, code: i32, message: &str, reason: &str) -> Value {
    let mut refused = error(id, code, message);
    refused["error"]["data"] = json!({ "code": reason });
    refused
}

fn claim_request(principal: &str, did: &str, public_key: &str) -> String {
    json!({
        "principal": principal,
        "did": did,
        "public_key": public_key,
    })
    .to_string()
}

fn claim_result(id: &Value, status: u16, content_type: &str, body: &[u8]) -> Value {
    if content_type != "application/json" {
        return error(id, -32603, "Invalid upstream response");
    }
    let Ok(document) = serde_json::from_slice::<Value>(body) else {
        return error(id, -32603, "Invalid upstream response");
    };
    if status == 202 {
        let mut pending = error(id, -32001, "Faucet claim unavailable");
        pending["error"]["data"] = json!({
            "code": "faucet_claim_pending", "state": "pending", "upstream": document
        });
        return pending;
    }
    if status != 200 {
        let code = match status {
            400 | 409 => -32602,
            401 | 403 | 404 => -32002,
            429 => -32005,
            _ => -32001,
        };
        let mut refused = error(id, code, "Faucet claim refused");
        refused["error"]["data"] = document;
        return refused;
    }
    let funded = document["funded"] == json!(true)
        && document["funding_id"].as_str().is_some_and(|id| {
            !id.is_empty() && id.len() <= 128 && id.bytes().all(|byte| byte.is_ascii_hexdigit())
        })
        && document["transaction_id"]
            .as_str()
            .is_some_and(|id| !id.is_empty() && id.len() <= 128)
        && document["amount"].as_str().is_some_and(|amount| {
            !amount.is_empty()
                && amount.len() <= 39
                && amount.bytes().all(|byte| byte.is_ascii_digit())
                && amount != "0"
        })
        && document["network"]
            .as_str()
            .is_some_and(|network| !network.is_empty() && network.len() <= 64);
    if !funded {
        return error(id, -32603, "Faucet claim evidence incomplete");
    }
    json!({"jsonrpc":"2.0", "id":id, "result":document})
}

fn bound_signer(allowed: &[String], public_key: &str) -> bool {
    allowed
        .iter()
        .any(|key| key.eq_ignore_ascii_case(public_key))
}

fn request_funds(
    config: &Config,
    request: &IncomingRequest,
    id: &Value,
    params: Option<&Value>,
) -> Value {
    let (did, public_key) = match claim_params(params) {
        Ok(parsed) => parsed,
        Err(code) => return error(id, code, "Invalid params"),
    };
    let Some(faucet) = &config.faucet else {
        return refusal(
            id,
            -32001,
            "Faucet claim unavailable",
            "faucet_not_configured",
        );
    };
    let (principal, session) = match super::session(config, request) {
        Ok(authenticated) => authenticated,
        Err(answer) if answer.status == 401 => {
            return refusal(id, -32002, "Faucet claim refused", "session_required");
        }
        Err(_) => {
            return refusal(
                id,
                -32001,
                "Faucet claim unavailable",
                "identity_unavailable",
            );
        }
    };
    if !bound_signer(&session.allowed_signer_public_keys, &public_key) {
        return refusal(id, -32002, "Faucet claim refused", "signer_not_bound");
    }
    if !consume_claim() {
        return refusal(id, -32005, "Faucet claim unavailable", "faucet_rate_limit");
    }
    let key = idempotency(principal.as_str(), &did, &public_key);
    let body = claim_request(principal.as_str(), &did, &public_key);
    match super::upstream_json(
        config,
        &faucet.endpoint,
        faucet.token.as_str(),
        "POST",
        CLAIM_PATH,
        Some(&key),
        body.as_bytes(),
    ) {
        Ok(upstream) => claim_result(id, upstream.status, &upstream.content_type, &upstream.body),
        Err(_) => refusal(id, -32001, "Faucet claim unavailable", "faucet_unavailable"),
    }
}

pub(super) fn dispatch(
    config: &Config,
    request: &IncomingRequest,
    method: &str,
    id: &Value,
    params: Option<&Value>,
) -> Option<Value> {
    (method == "lx_requestFunds").then(|| request_funds(config, request, id, params))
}

pub(super) fn configured() -> Result<Option<Faucet>, String> {
    if std::env::var_os(URL_VARIABLE).is_none() && std::env::var_os(TOKEN_VARIABLE).is_none() {
        return Ok(None);
    }
    let url = std::env::var(URL_VARIABLE)
        .map_err(|_| format!("{URL_VARIABLE} is required when {TOKEN_VARIABLE} is set"))?;
    Ok(Some(Faucet {
        endpoint: Endpoint::parse(&url)?,
        token: super::read_secret(TOKEN_VARIABLE)?,
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn funded(funding: &str) -> Value {
        json!({
            "funded": true,
            "funding_id": funding,
            "transaction_id": "cd".repeat(32),
            "amount": "1000000",
            "network": "layerx-testnet",
        })
    }

    #[test]
    fn the_claim_idempotency_key_is_a_pinned_domain_separated_digest() {
        let principal = "beta.7399f031b011aa1198718d62c3f79984";
        let did = "did:layerx:alice";
        let key = "ab".repeat(32);
        assert_eq!(
            idempotency(principal, did, &key),
            "6afc28e8efcfabecdefc70c7706e7e44365b5f5446bc1b960821164847c1252a"
        );
        assert_eq!(idempotency(principal, did, &key).len(), 64);
        assert_ne!(
            idempotency(principal, did, &key),
            idempotency("beta.other", did, &key)
        );
        assert_ne!(
            idempotency(principal, did, &key),
            idempotency(principal, "did:layerx:bob", &key)
        );
        assert_ne!(
            idempotency(principal, did, &key),
            idempotency(principal, did, &"cd".repeat(32))
        );
        assert_ne!(
            idempotency("a", "bc", &key),
            idempotency("ab", "c", &key),
            "the digest is length prefixed so field boundaries cannot be shifted"
        );
    }

    #[test]
    fn claim_params_require_one_did_and_one_lowercase_signer_key() {
        let key = "ab".repeat(32);
        assert_eq!(
            claim_params(Some(&json!(["did:layerx:alice", key]))),
            Ok(("did:layerx:alice".to_owned(), key.clone()))
        );
        for args in [
            json!([]),
            json!(["did:layerx:alice"]),
            json!(["did:layerx:alice", key, key]),
            json!(["did:layerx:alice", "AB".repeat(32)]),
            json!(["did:layerx:alice", "zz".repeat(32)]),
            json!(["did:layerx:alice", "ab".repeat(31)]),
            json!(["did:layerx:alice", "00".repeat(32)]),
            json!(["layerx:alice", key]),
            json!(["did:layerx:../bob", key]),
            json!(["did:layerx:a b", key]),
            json!(["did::alice", key]),
            json!(["did:layerx", key]),
            json!(["", key]),
            json!([1, key]),
            json!({"did": "did:layerx:alice"}),
        ] {
            assert_eq!(claim_params(Some(&args)), Err(-32602), "{args}");
        }
        assert_eq!(claim_params(None), Err(-32602));
    }

    #[test]
    fn the_claim_budget_is_bounded_and_resets_with_the_minute() {
        let mut window = (0_u64, 0_u32);
        for _ in 0..CLAIMS_PER_MINUTE {
            assert!(consume(&mut window, 11));
        }
        assert!(!consume(&mut window, 11));
        assert_eq!(window, (11, CLAIMS_PER_MINUTE));
        assert!(consume(&mut window, 12));
        assert_eq!(window, (12, 1));
    }

    #[test]
    fn only_a_key_the_session_authorises_may_be_funded() {
        let key = "ab".repeat(32);
        let other = "cd".repeat(32);
        assert!(bound_signer(std::slice::from_ref(&key), &key));
        assert!(bound_signer(&[other.clone(), key.clone()], &key));
        assert!(bound_signer(&[key.to_ascii_uppercase()], &key));
        assert!(!bound_signer(&[], &key));
        assert!(!bound_signer(&[other], &key));
    }

    #[test]
    fn only_complete_funding_evidence_becomes_a_result() {
        let funding = "ef".repeat(32);
        let accepted = claim_result(
            &json!(4),
            200,
            "application/json",
            funded(&funding).to_string().as_bytes(),
        );
        assert_eq!(accepted["id"], 4);
        assert_eq!(accepted["result"], funded(&funding));
        assert!(accepted.get("error").is_none());

        for incomplete in [
            json!({"funded": false, "funding_id": funding, "transaction_id": "cd", "amount": "1", "network": "layerx-testnet"}),
            json!({"funded": true, "transaction_id": "cd", "amount": "1", "network": "layerx-testnet"}),
            json!({"funded": true, "funding_id": funding, "amount": "1", "network": "layerx-testnet"}),
            json!({"funded": true, "funding_id": funding, "transaction_id": "cd", "network": "layerx-testnet"}),
            json!({"funded": true, "funding_id": funding, "transaction_id": "cd", "amount": "1"}),
            json!({"funded": true, "funding_id": funding, "transaction_id": "cd", "amount": "0", "network": "layerx-testnet"}),
            json!({"funded": true, "funding_id": funding, "transaction_id": "cd", "amount": 1, "network": "layerx-testnet"}),
            json!({"funded": true, "funding_id": "zz", "transaction_id": "cd", "amount": "1", "network": "layerx-testnet"}),
            json!({"funded": "true", "funding_id": funding, "transaction_id": "cd", "amount": "1", "network": "layerx-testnet"}),
        ] {
            let refused = claim_result(
                &json!(4),
                200,
                "application/json",
                incomplete.to_string().as_bytes(),
            );
            assert_eq!(refused["error"]["code"], -32603, "{incomplete}");
            assert!(refused.get("result").is_none());
        }
        for (content_type, body) in [("application/json", "not json"), ("text/html", "{}")] {
            assert_eq!(
                claim_result(&json!(4), 200, content_type, body.as_bytes())["error"]["code"],
                -32603
            );
        }
    }

    #[test]
    fn a_pending_or_refused_claim_is_never_reported_as_funded() {
        let upstream = json!({"state": "still_checking", "retry": "after"});
        let pending = claim_result(
            &json!("f1"),
            202,
            "application/json",
            upstream.to_string().as_bytes(),
        );
        assert_eq!(pending["error"]["code"], -32001);
        assert_eq!(pending["error"]["data"]["state"], "pending");
        assert_eq!(pending["error"]["data"]["code"], "faucet_claim_pending");
        assert_eq!(pending["error"]["data"]["upstream"], upstream);
        assert!(pending.get("result").is_none());

        for (status, code) in [
            (400_u16, -32602_i32),
            (409, -32602),
            (401, -32002),
            (403, -32002),
            (404, -32002),
            (429, -32005),
            (503, -32001),
            (500, -32001),
        ] {
            let upstream = json!({"error": {"code": "identity_quota", "retry": "after"}});
            let refused = claim_result(
                &json!("f1"),
                status,
                "application/json",
                upstream.to_string().as_bytes(),
            );
            assert_eq!(refused["error"]["code"], code, "{status}");
            assert_eq!(refused["error"]["data"], upstream);
            assert!(refused.get("result").is_none());
        }
    }

    #[test]
    fn the_service_claim_request_matches_the_faucet_contract() {
        let principal = "beta.7399f031b011aa1198718d62c3f79984";
        let key = "ab".repeat(32);
        assert_eq!(
            claim_request(principal, "did:layerx:alice", &key),
            format!(
                "{{\"did\":\"did:layerx:alice\",\"principal\":\"{principal}\",\"public_key\":\"{key}\"}}"
            )
        );
        let request: Value =
            serde_json::from_str(&claim_request(principal, "did:layerx:alice", &key))
                .unwrap_or_else(|error| panic!("{error}"));
        assert_eq!(request.as_object().map(serde_json::Map::len), Some(3));
        assert!(path_safe(principal, 512));
        assert!(!path_safe("", 512));
        assert!(!path_safe(&"a".repeat(513), 512));
        assert!(lowercase_hex32(&key));
    }

    #[test]
    fn faucet_refusals_carry_a_machine_readable_reason() {
        for (code, reason) in [
            (-32001, "faucet_not_configured"),
            (-32002, "session_required"),
            (-32002, "signer_not_bound"),
            (-32005, "faucet_rate_limit"),
            (-32001, "faucet_unavailable"),
            (-32001, "identity_unavailable"),
        ] {
            let refused = refusal(&json!(1), code, "Faucet claim unavailable", reason);
            assert_eq!(refused["error"]["code"], code);
            assert_eq!(refused["error"]["data"]["code"], reason);
            assert!(refused.get("result").is_none());
        }
    }
}
