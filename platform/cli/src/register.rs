use clap::Args;
use ed25519_dalek::{Signer as _, SigningKey};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};

use crate::config::Configuration;
use crate::credential;
use crate::encoding::hex_encode;
use crate::output::CommandOutput;

pub const TENANT: &str = "beta";

const BINDING_DOMAIN: &[u8] = b"layerx-register-binding-v1";
const SUBJECT_DOMAIN: &[u8] = b"layerx-register-subject-v1";

#[derive(Args)]
pub struct RegisterArgs {
    /// Local key whose Ed25519 public key authorises the new principal.
    #[arg(long)]
    key: Option<String>,
}

fn tagged(domain: &[u8], tenant: &str, signer_public_key: &[u8; 32]) -> [u8; 32] {
    let mut hash = Sha256::new();
    for part in [domain, tenant.as_bytes(), signer_public_key.as_slice()] {
        hash.update((part.len() as u64).to_be_bytes());
        hash.update(part);
    }
    hash.finalize().into()
}

fn binding(tenant: &str, signer_public_key: &[u8; 32]) -> [u8; 32] {
    tagged(BINDING_DOMAIN, tenant, signer_public_key)
}

fn subject(tenant: &str, signer_public_key: &[u8; 32]) -> String {
    format!(
        "{tenant}.{}",
        &hex_encode(&tagged(SUBJECT_DOMAIN, tenant, signer_public_key))[..32]
    )
}

fn request(signer_public_key: &[u8; 32], signature: &[u8; 64]) -> Value {
    json!({
        "jsonrpc": "2.0",
        "id": "register",
        "method": "lx_register",
        "params": [hex_encode(signer_public_key), hex_encode(signature)],
    })
}

fn decode(response: &Value, sub: &str, signer: &str) -> Result<Value, String> {
    if response.get("jsonrpc").and_then(Value::as_str) != Some("2.0") {
        return Err("registration response is not a JSON-RPC 2.0 envelope".into());
    }
    if let Some(failure) = response.get("error") {
        let code = failure
            .get("code")
            .and_then(Value::as_i64)
            .unwrap_or_default();
        let message = failure
            .get("message")
            .and_then(Value::as_str)
            .unwrap_or("registration refused");
        let reason = failure
            .pointer("/data/code")
            .and_then(Value::as_str)
            .unwrap_or_default();
        return Err(if reason.is_empty() {
            format!("registration refused ({code}): {message}")
        } else {
            format!("registration refused ({code}): {message} [{reason}]")
        });
    }
    let principal = response
        .get("result")
        .ok_or_else(|| "registration response carries neither a result nor an error".to_owned())?;
    if principal.get("tenant").and_then(Value::as_str) != Some(TENANT) {
        return Err("the registered principal names another tenant".into());
    }
    if principal.get("sub").and_then(Value::as_str) != Some(sub) {
        return Err("the registered principal names another subject".into());
    }
    if principal.get("allowed_signer_public_keys") != Some(&json!([signer])) {
        return Err("the registered principal authorises another signer key".into());
    }
    Ok(principal.clone())
}

pub fn run(arguments: &RegisterArgs) -> Result<CommandOutput, String> {
    let configuration = Configuration::load()?;
    let (environment, client) = crate::active_client(&configuration)?;
    let name = arguments
        .key
        .clone()
        .or_else(|| configuration.default_key.clone())
        .ok_or_else(|| {
            "registration requires a local key; create one with layerx key create <name>".to_owned()
        })?;
    let metadata = configuration
        .keys
        .get(&name)
        .ok_or_else(|| format!("key {name} does not exist"))?;
    let seed = credential::key_seed(&name)?;
    let signing = SigningKey::from_bytes(&seed);
    let signer_public_key = signing.verifying_key().to_bytes();
    let signer = hex_encode(&signer_public_key);
    if signer != metadata.public_key {
        return Err(format!(
            "stored key {name} does not match the public key recorded for it"
        ));
    }
    let signature = signing
        .sign(&binding(TENANT, &signer_public_key))
        .to_bytes();
    drop(signing);
    let sub = subject(TENANT, &signer_public_key);
    let response = client.post("/rpc", &request(&signer_public_key, &signature), None)?;
    let principal = decode(&response, &sub, &signer)?;
    Ok(CommandOutput::new(
        "principal.registered",
        format!("Registered principal {sub} on {environment}"),
        json!({
            "environment": environment,
            "tenant": TENANT,
            "key": name,
            "principal": principal,
        }),
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn confirmed(sub: &str, signer: &str) -> Value {
        json!({
            "jsonrpc": "2.0",
            "id": "register",
            "result": {
                "tenant": TENANT,
                "sub": sub,
                "allowed_signer_public_keys": [signer],
                "account": Value::Null,
                "audiences": [],
            },
        })
    }

    #[test]
    fn the_binding_and_subject_match_the_gateway_vectors() {
        let key = [0xab_u8; 32];
        assert_eq!(
            hex_encode(&binding(TENANT, &key)),
            "425fa6c5988efc16bce8a7931f1efbc039b75d10f0ffa617c3c2d3de2366e128"
        );
        assert_eq!(
            subject(TENANT, &key),
            "beta.7399f031b011aa1198718d62c3f79984"
        );
        assert_ne!(binding(TENANT, &key), binding("gamma", &key));
        assert_ne!(subject(TENANT, &key), subject(TENANT, &[0xac_u8; 32]));
        assert!(!subject(TENANT, &key).contains(':'));
    }

    #[test]
    fn the_request_carries_a_proof_the_gateway_can_verify() {
        let signing = SigningKey::from_bytes(&[5_u8; 32]);
        let signer_public_key = signing.verifying_key().to_bytes();
        let signature = signing
            .sign(&binding(TENANT, &signer_public_key))
            .to_bytes();
        let envelope = request(&signer_public_key, &signature);
        assert_eq!(envelope["jsonrpc"], "2.0");
        assert_eq!(envelope["method"], "lx_register");
        assert_eq!(envelope["params"][0], hex_encode(&signer_public_key));
        assert_eq!(envelope["params"][1], hex_encode(&signature));
        assert_eq!(
            envelope["params"].as_array().map(Vec::len),
            Some(2),
            "lx_register takes exactly two positional arguments"
        );
        layerx_crypto::ed25519::verify_digest(
            &signer_public_key,
            &signature,
            &binding(TENANT, &signer_public_key),
        )
        .unwrap_or_else(|error| panic!("{error:?}"));
        assert!(layerx_crypto::ed25519::verify_digest(
            &signer_public_key,
            &signature,
            &binding("gamma", &signer_public_key),
        )
        .is_err());
    }

    #[test]
    fn only_a_principal_naming_this_key_is_accepted() {
        let key = [0xab_u8; 32];
        let sub = subject(TENANT, &key);
        let signer = hex_encode(&key);
        let principal = decode(&confirmed(&sub, &signer), &sub, &signer)
            .unwrap_or_else(|error| panic!("{error}"));
        assert_eq!(principal["sub"], sub);
        assert_eq!(principal["tenant"], TENANT);
        assert_eq!(principal["allowed_signer_public_keys"], json!([signer]));

        let other = hex_encode(&[0xac_u8; 32]);
        for (response, fragment) in [
            (confirmed("beta.deadbeef", &signer), "another subject"),
            (confirmed(&sub, &other), "another signer key"),
            (
                json!({"jsonrpc":"2.0","id":"register","result":{"tenant":"gamma","sub":sub,"allowed_signer_public_keys":[signer]}}),
                "another tenant",
            ),
            (
                json!({"jsonrpc":"2.0","id":"register"}),
                "neither a result nor an error",
            ),
            (
                json!({"id":"register","result":{}}),
                "JSON-RPC 2.0 envelope",
            ),
        ] {
            let failure = decode(&response, &sub, &signer)
                .err()
                .unwrap_or_else(|| panic!("accepted {response}"));
            assert!(failure.contains(fragment), "{failure}");
        }
    }

    #[test]
    fn gateway_refusals_are_reported_with_their_code_and_reason() {
        let key = [0xab_u8; 32];
        let sub = subject(TENANT, &key);
        let signer = hex_encode(&key);
        let refusal = json!({
            "jsonrpc": "2.0",
            "id": "register",
            "error": {
                "code": -32005,
                "message": "Registration unavailable",
                "data": {"code": "registration_rate_limit"},
            },
        });
        let failure = decode(&refusal, &sub, &signer)
            .err()
            .unwrap_or_else(|| panic!("accepted a refusal"));
        assert!(failure.contains("-32005"), "{failure}");
        assert!(failure.contains("Registration unavailable"), "{failure}");
        assert!(failure.contains("registration_rate_limit"), "{failure}");

        let bare = json!({
            "jsonrpc": "2.0",
            "id": "register",
            "error": {"code": -32602, "message": "Invalid params"},
        });
        let failure = decode(&bare, &sub, &signer)
            .err()
            .unwrap_or_else(|| panic!("accepted a refusal"));
        assert_eq!(failure, "registration refused (-32602): Invalid params");
    }
}
