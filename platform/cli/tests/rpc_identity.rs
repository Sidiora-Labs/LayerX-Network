use ed25519_dalek::{Signer as _, SigningKey};
use layerx_platform_cli::rpc::{decode_response, request, RpcClient};
use serde_json::{json, Value};
use sha2::{Digest as _, Sha256};
use zeroize::Zeroizing;

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}

fn registration() -> Result<(String, String), String> {
    let signer = SigningKey::from_bytes(&[0x53; 32]);
    let public_key = signer.verifying_key().to_bytes();
    let mut digest = Sha256::new();
    for part in [
        b"layerx-register-binding-v1".as_slice(),
        b"beta".as_slice(),
        public_key.as_slice(),
    ] {
        digest.update((part.len() as u64).to_be_bytes());
        digest.update(part);
    }
    let digest: [u8; 32] = digest.finalize().into();
    let signature = signer.sign(&digest).to_bytes();
    layerx_crypto::ed25519::verify_digest(&public_key, &signature, &digest)
        .map_err(|error| format!("registration signature: {error:?}"))?;
    Ok((hex(&public_key), hex(&signature)))
}

#[test]
fn registration_request_preserves_the_real_proof_and_exact_contract() -> Result<(), String> {
    let (key, signature) = registration()?;
    assert_eq!(
        request("lx_register", &json!([key, signature]))?,
        json!({"jsonrpc":"2.0","id":1,"method":"lx_register","params":[key,signature]})
    );
    for args in [
        json!([]),
        json!([key]),
        json!([key, signature, "beta"]),
        json!({"signer_public_key":key,"registration_signature":signature}),
        json!([key.to_uppercase(), signature]),
        json!([key, signature.to_uppercase()]),
        json!([format!("0x{key}"), signature]),
        json!([key, format!("0x{signature}")]),
        json!(["ab".repeat(31), signature]),
        json!([key, "ab".repeat(63)]),
        json!(["gg".repeat(32), signature]),
        json!([key, "gg".repeat(64)]),
        json!([1, signature]),
    ] {
        assert!(request("lx_register", &args).is_err());
    }
    Ok(())
}

#[test]
fn faucet_parameters_match_the_gateway_did_and_signer_bounds() -> Result<(), String> {
    let (key, _) = registration()?;
    let maximum = format!("did:layerx:{}", "a".repeat(244));
    for did in ["did:layerx:alice-1._:wallet".to_owned(), maximum.clone()] {
        assert_eq!(
            request("lx_requestFunds", &json!([did, key]))?,
            json!({"jsonrpc":"2.0","id":1,"method":"lx_requestFunds","params":[did,key]})
        );
    }
    for did in [
        String::new(),
        "layerx:alice".to_owned(),
        "did:alice".to_owned(),
        "did::alice".to_owned(),
        "did:layerx:".to_owned(),
        "did:layerx:alice/bob".to_owned(),
        "did:layerx:alice?query".to_owned(),
        "did:layerx:alice#fragment".to_owned(),
        "did:layerx:álîce".to_owned(),
        format!("{maximum}a"),
        format!("did:layerx:{}", "a".repeat(502)),
    ] {
        assert!(request("lx_requestFunds", &json!([did, key])).is_err());
    }
    for args in [
        json!([]),
        json!(["did:layerx:alice"]),
        json!(["did:layerx:alice", key, "amount"]),
        json!({"did":"did:layerx:alice","signer_public_key":key}),
        json!(["did:layerx:alice", "00".repeat(32)]),
        json!(["did:layerx:alice", key.to_uppercase()]),
        json!(["did:layerx:alice", format!("0x{key}")]),
        json!(["did:layerx:alice", "ab".repeat(31)]),
        json!(["did:layerx:alice", "gg".repeat(32)]),
        json!(["did:layerx:alice", 1]),
    ] {
        assert!(request("lx_requestFunds", &args).is_err());
    }
    Ok(())
}

#[test]
fn identity_sessions_and_gateway_credentials_are_distinct() -> Result<(), String> {
    let url = "http://127.0.0.1:1/rpc";
    let token = || Zeroizing::new("identity-session-transport-test".to_owned());
    let session = RpcClient::new_session(url, token())?;
    assert!(RpcClient::new_session(url, Zeroizing::new(String::new())).is_err());
    assert!(RpcClient::new_session("https://example.com", token()).is_err());
    assert!(RpcClient::new_session("http://example.com/rpc", token()).is_err());
    assert!(session
        .subscribe(&json!(["receipts"]), std::time::Duration::from_secs(1))
        .err()
        .ok_or("identity session authorized a gateway subscription")?
        .starts_with("gateway_credential_required:"));
    assert!(session.call("lx_subscribe", &json!(["receipts"])).is_err());
    assert!(session.call("lx_unsubscribe", &json!(["1"])).is_err());
    let credential = Zeroizing::new(format!("key:lxp_live_{}", "a".repeat(64)));
    for client in [
        RpcClient::new(url, None)?,
        RpcClient::new(url, Some(credential))?,
    ] {
        assert!(client
            .call(
                "lx_requestFunds",
                &json!(["did:layerx:alice", registration()?.0])
            )
            .err()
            .ok_or("faucet accepted without an identity session")?
            .starts_with("identity_session_required:"));
    }
    Ok(())
}

#[test]
fn identity_rpc_responses_remain_bound_and_preserve_all_refusal_fields() -> Result<(), String> {
    for method in ["lx_register", "lx_requestFunds"] {
        let result = json!({"state":"upstream-record"});
        assert_eq!(
            decode_response(method, &json!({"jsonrpc":"2.0","id":1,"result":result}))?,
            result
        );
        for (code, reason) in [
            (-32602, "invalid_params"),
            (-32002, "registration_proof_invalid"),
            (-32002, "session_required"),
            (-32002, "signer_not_bound"),
            (-32005, "faucet_rate_limit"),
            (-32001, "faucet_claim_pending"),
            (-32001, "registration_not_configured"),
            (-32603, "evidence_incomplete"),
        ] {
            let error = json!({"code":code,"message":"refused","data":{"code":reason}});
            let refused = decode_response(method, &json!({"jsonrpc":"2.0","id":1,"error":error}))
                .err()
                .ok_or("RPC refusal was accepted")?;
            assert_eq!(
                serde_json::from_str::<Value>(&refused).map_err(|e| e.to_string())?,
                error
            );
        }
        for response in [
            json!({"jsonrpc":"2.0","id":2,"result":{}}),
            json!({"jsonrpc":"2.0","id":1,"result":{},"error":{}}),
            json!({"jsonrpc":"2.0","id":1,"result":null}),
            json!({"jsonrpc":"2.0","id":1,"result":true}),
            json!({"jsonrpc":"2.0","id":1,"error":{"code":"-32002","message":"refused"}}),
            json!({"jsonrpc":"2.0","id":1,"error":{"code":-32002}}),
            json!({"jsonrpc":"2.0","id":1}),
            json!({"id":1,"result":{}}),
        ] {
            assert!(decode_response(method, &response).is_err());
        }
    }
    Ok(())
}
