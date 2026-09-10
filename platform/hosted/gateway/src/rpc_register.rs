use super::Config;
use crate::rpc::error;
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::sync::Mutex;
use zeroize::Zeroizing;

const TENANT: &str = "beta";
const TOKEN_VARIABLE: &str = "LAYERX_GATEWAY_IDENTITY_PROVISIONING_TOKEN_FILE";
const BINDING_DOMAIN: &[u8] = b"layerx-register-binding-v1";
const SUBJECT_DOMAIN: &[u8] = b"layerx-register-subject-v1";
const REGISTRATIONS_PER_MINUTE: u32 = 30;

static REGISTER_WINDOW: Mutex<(u64, u32)> = Mutex::new((0, 0));

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
        &super::hex(&tagged(SUBJECT_DOMAIN, tenant, signer_public_key))[..32]
    )
}

fn lowercase_hex(value: &str, bytes: usize) -> bool {
    value.len() == bytes * 2
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

fn register_params(params: Option<&Value>) -> Result<([u8; 32], [u8; 64]), i32> {
    let Some(Value::Array(args)) = params else {
        return Err(-32602);
    };
    let [Value::String(signer_public_key), Value::String(signature)] = args.as_slice() else {
        return Err(-32602);
    };
    if !lowercase_hex(signer_public_key, 32) || !lowercase_hex(signature, 64) {
        return Err(-32602);
    }
    let signer_public_key = super::parse_hex32(signer_public_key).map_err(|_| -32602)?;
    let signature: [u8; 64] = super::decode_hex(signature, 64)
        .ok()
        .and_then(|bytes| <[u8; 64]>::try_from(bytes).ok())
        .ok_or(-32602)?;
    Ok((signer_public_key, signature))
}

fn proven(signer_public_key: &[u8; 32], signature: &[u8; 64]) -> bool {
    layerx_crypto::ed25519::verify_digest(
        signer_public_key,
        signature,
        &binding(TENANT, signer_public_key),
    )
    .is_ok()
}

fn consume(window: &mut (u64, u32), minute: u64) -> bool {
    if window.0 != minute {
        *window = (minute, 0);
    }
    if window.1 >= REGISTRATIONS_PER_MINUTE {
        return false;
    }
    window.1 += 1;
    true
}

fn consume_registration() -> bool {
    let Ok(second) = super::now() else {
        return false;
    };
    let Ok(mut window) = REGISTER_WINDOW.lock() else {
        return false;
    };
    consume(&mut window, second / 60)
}

fn refusal(id: &Value, code: i32, message: &str, reason: &str) -> Value {
    let mut refused = error(id, code, message);
    refused["error"]["data"] = json!({ "code": reason });
    refused
}

fn principal_result(
    id: &Value,
    sub: &str,
    signer: &str,
    status: u16,
    content_type: &str,
    body: &[u8],
) -> Value {
    if content_type != "application/json" {
        return error(id, -32603, "Invalid upstream response");
    }
    let Ok(document) = serde_json::from_slice::<Value>(body) else {
        return error(id, -32603, "Invalid upstream response");
    };
    if status != 200 {
        let code = match status {
            401 | 403 => -32002,
            429 => -32005,
            _ => -32001,
        };
        let mut refused = error(id, code, "Registration unavailable");
        refused["error"]["data"] = document;
        return refused;
    }
    if document["tenant"] != json!(TENANT)
        || document["sub"] != json!(sub)
        || document["allowed_signer_public_keys"] != json!([signer])
    {
        return error(id, -32603, "Registration principal mismatch");
    }
    json!({"jsonrpc":"2.0", "id":id, "result":document})
}

fn principal_request(sub: &str, signer: &str) -> String {
    json!({
        "tenant": TENANT,
        "sub": sub,
        "allowed_signer_public_keys": [signer],
    })
    .to_string()
}

fn register(config: &Config, id: &Value, params: Option<&Value>) -> Value {
    let (signer_public_key, signature) = match register_params(params) {
        Ok(parsed) => parsed,
        Err(code) => return error(id, code, "Invalid params"),
    };
    if !proven(&signer_public_key, &signature) {
        return refusal(
            id,
            -32002,
            "Registration refused",
            "registration_proof_invalid",
        );
    }
    let Some(token) = &config.registration_token else {
        return refusal(
            id,
            -32001,
            "Registration unavailable",
            "registration_not_configured",
        );
    };
    if !consume_registration() {
        return refusal(
            id,
            -32005,
            "Registration unavailable",
            "registration_rate_limit",
        );
    }
    let sub = subject(TENANT, &signer_public_key);
    let signer = super::hex(&signer_public_key);
    let request = principal_request(&sub, &signer);
    match super::upstream_json(
        config,
        &config.identity,
        token.as_str(),
        "POST",
        "/v1/principals",
        None,
        request.as_bytes(),
    ) {
        Ok(upstream) => principal_result(
            id,
            &sub,
            &signer,
            upstream.status,
            &upstream.content_type,
            &upstream.body,
        ),
        Err(_) => refusal(
            id,
            -32001,
            "Registration unavailable",
            "identity_unavailable",
        ),
    }
}

pub(super) fn dispatch(
    config: &Config,
    method: &str,
    id: &Value,
    params: Option<&Value>,
) -> Option<Value> {
    (method == "lx_register").then(|| register(config, id, params))
}

pub(super) fn configured_token() -> Result<Option<Zeroizing<String>>, String> {
    if std::env::var_os(TOKEN_VARIABLE).is_none() {
        return Ok(None);
    }
    super::read_secret(TOKEN_VARIABLE).map(Some)
}

#[cfg(test)]
mod tests {
    use super::*;
    use ed25519_dalek::{Signer as _, SigningKey};

    fn signing_key(seed: u8) -> SigningKey {
        SigningKey::from_bytes(&[seed; 32])
    }

    #[test]
    fn the_registration_binding_and_subject_are_pinned_domain_separated_digests() {
        let key = [0xab_u8; 32];
        assert_eq!(
            crate::hex(&binding(TENANT, &key)),
            "425fa6c5988efc16bce8a7931f1efbc039b75d10f0ffa617c3c2d3de2366e128"
        );
        assert_eq!(
            subject(TENANT, &key),
            "beta.7399f031b011aa1198718d62c3f79984"
        );
        assert_ne!(binding(TENANT, &key), binding("gamma", &key));
        assert_ne!(subject(TENANT, &key), subject("gamma", &key));
        assert_ne!(binding(TENANT, &key).as_slice(), &key[..]);
        assert_ne!(
            binding(TENANT, &key),
            tagged(SUBJECT_DOMAIN, TENANT, &key),
            "the signed binding and the subject digest must not collide"
        );
        assert_ne!(subject(TENANT, &key), subject(TENANT, &[0xac_u8; 32]));
        assert!(!subject(TENANT, &key).contains(':'));
        assert_eq!(subject(TENANT, &key).len(), 37);
    }

    #[test]
    fn registration_params_require_two_lowercase_hexadecimal_arguments() {
        let key = "ab".repeat(32);
        let signature = "cd".repeat(64);
        assert_eq!(
            register_params(Some(&json!([key, signature]))),
            Ok(([0xab_u8; 32], [0xcd_u8; 64]))
        );
        for args in [
            json!([]),
            json!([key]),
            json!([key, signature, key]),
            json!([key, key]),
            json!([signature, signature]),
            json!(["AB".repeat(32), signature]),
            json!([key, "CD".repeat(64)]),
            json!(["zz".repeat(32), signature]),
            json!([format!("{key}ab"), signature]),
            json!([1, signature]),
            json!({"signer_public_key": key}),
        ] {
            assert_eq!(register_params(Some(&args)), Err(-32602), "{args}");
        }
        assert_eq!(register_params(None), Err(-32602));
    }

    #[test]
    fn only_a_signature_over_the_tenant_binding_by_the_named_key_is_proof() {
        let key = signing_key(7);
        let public_key = key.verifying_key().to_bytes();
        let signature = key.sign(&binding(TENANT, &public_key)).to_bytes();
        assert!(proven(&public_key, &signature));

        let mut tampered = signature;
        tampered[0] ^= 0x01;
        assert!(!proven(&public_key, &tampered));

        let other_tenant = key.sign(&binding("gamma", &public_key)).to_bytes();
        assert!(!proven(&public_key, &other_tenant));

        let raw = key.sign(&public_key).to_bytes();
        assert!(!proven(&public_key, &raw));

        let impostor = signing_key(9);
        let impostor_public_key = impostor.verifying_key().to_bytes();
        let borrowed = impostor.sign(&binding(TENANT, &public_key)).to_bytes();
        assert!(!proven(&public_key, &borrowed));
        assert!(!proven(&impostor_public_key, &signature));
        assert!(!proven(&[0_u8; 32], &signature));
    }

    #[test]
    fn the_registration_budget_is_bounded_and_resets_with_the_minute() {
        let mut window = (0_u64, 0_u32);
        for _ in 0..REGISTRATIONS_PER_MINUTE {
            assert!(consume(&mut window, 4));
        }
        assert!(!consume(&mut window, 4));
        assert_eq!(window, (4, REGISTRATIONS_PER_MINUTE));
        assert!(consume(&mut window, 5));
        assert_eq!(window, (5, 1));
    }

    #[test]
    fn the_registered_principal_is_only_returned_when_identity_confirms_it() {
        let sub = subject(TENANT, &[0xab_u8; 32]);
        let signer = "ab".repeat(32);
        let confirmed = json!({
            "tenant": TENANT,
            "sub": sub.as_str(),
            "allowed_signer_public_keys": [signer.as_str()],
            "account": Value::Null,
            "audiences": [],
        });
        let accepted = principal_result(
            &json!(3),
            &sub,
            &signer,
            200,
            "application/json",
            confirmed.to_string().as_bytes(),
        );
        assert_eq!(accepted["id"], 3);
        assert_eq!(accepted["result"], confirmed);
        assert!(accepted.get("error").is_none());

        for divergent in [
            json!({"tenant":"gamma","sub":sub.as_str(),"allowed_signer_public_keys":[signer.as_str()]}),
            json!({"tenant":TENANT,"sub":"beta.other","allowed_signer_public_keys":[signer.as_str()]}),
            json!({"tenant":TENANT,"sub":sub.as_str(),"allowed_signer_public_keys":["cd".repeat(32)]}),
            json!({"tenant":TENANT,"sub":sub.as_str(),"allowed_signer_public_keys":[signer.as_str(),"cd".repeat(32)]}),
            json!({"tenant":TENANT,"sub":sub.as_str()}),
        ] {
            let refused = principal_result(
                &json!(3),
                &sub,
                &signer,
                200,
                "application/json",
                divergent.to_string().as_bytes(),
            );
            assert_eq!(refused["error"]["code"], -32603, "{divergent}");
            assert!(refused.get("result").is_none());
        }
    }

    #[test]
    fn identity_refusals_are_preserved_with_their_own_json_rpc_codes() {
        let sub = subject(TENANT, &[0xab_u8; 32]);
        let signer = "ab".repeat(32);
        for (status, code) in [
            (400_u16, -32001_i32),
            (403, -32002),
            (401, -32002),
            (429, -32005),
            (404, -32001),
            (503, -32001),
        ] {
            let upstream = json!({"error":{"code":"invalid_argument","retry":"never"}});
            let refused = principal_result(
                &json!("r1"),
                &sub,
                &signer,
                status,
                "application/json",
                upstream.to_string().as_bytes(),
            );
            assert_eq!(refused["error"]["code"], code, "{status}");
            assert_eq!(refused["error"]["data"], upstream);
            assert!(refused.get("result").is_none());
        }
        for (content_type, body) in [("application/json", "not json"), ("text/html", "{}")] {
            assert_eq!(
                principal_result(
                    &json!("r1"),
                    &sub,
                    &signer,
                    200,
                    content_type,
                    body.as_bytes()
                )["error"]["code"],
                -32603
            );
        }
    }

    #[test]
    fn registration_refusals_carry_a_machine_readable_reason() {
        for (code, reason) in [
            (-32002, "registration_proof_invalid"),
            (-32001, "registration_not_configured"),
            (-32005, "registration_rate_limit"),
            (-32001, "identity_unavailable"),
        ] {
            let refused = refusal(&json!(1), code, "Registration unavailable", reason);
            assert_eq!(refused["error"]["code"], code);
            assert_eq!(refused["error"]["data"]["code"], reason);
            assert!(refused.get("result").is_none());
        }
    }

    #[test]
    fn the_provisioning_request_matches_the_identity_principal_contract() {
        let key = [0xab_u8; 32];
        let sub = subject(TENANT, &key);
        let signer = crate::hex(&key);
        assert_eq!(
            principal_request(&sub, &signer),
            format!(
                "{{\"allowed_signer_public_keys\":[\"{signer}\"],\"sub\":\"{sub}\",\"tenant\":\"beta\"}}"
            )
        );
        let request: Value = serde_json::from_str(&principal_request(&sub, &signer))
            .unwrap_or_else(|error| panic!("{error}"));
        assert_eq!(request.as_object().map(serde_json::Map::len), Some(3));
        for field in [TENANT, sub.as_str()] {
            assert!(!field.is_empty() && field.len() <= 128, "{field}");
            assert!(
                field.bytes().all(|byte| byte.is_ascii_lowercase()
                    || byte.is_ascii_digit()
                    || matches!(byte, b'-' | b'_' | b'.' | b':')),
                "{field}"
            );
        }
        assert!(!TENANT.contains(':'));
        assert!(!sub.contains(':'));
        assert!(lowercase_hex(&signer, 32));
    }
}
