use std::io::Write;
use std::process::{Command, Stdio};

fn run(value: &str) -> std::process::Output {
    let mut child = Command::new(env!("CARGO_BIN_EXE_layerx-human-identity-provider"))
        .arg("provision-account")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap_or_else(|error| panic!("provider: {error:?}"));
    child
        .stdin
        .take()
        .unwrap_or_else(|| panic!("stdin"))
        .write_all(value.as_bytes())
        .unwrap_or_else(|error| panic!("write: {error:?}"));
    child
        .wait_with_output()
        .unwrap_or_else(|error| panic!("output: {error:?}"))
}

#[test]
fn exported_did_uses_protocol_account_derivation() {
    let did = format!("did:layerx:{}", "ab".repeat(32));
    let output = run(&serde_json::json!({"did": did}).to_string());
    assert!(output.status.success());
    let result: serde_json::Value =
        serde_json::from_slice(&output.stdout).unwrap_or_else(|error| panic!("json: {error:?}"));
    let account = layerx_types::account::AccountId::parse(&format!("agent:{did}:main"))
        .unwrap_or_else(|error| panic!("account: {error:?}"));
    let expected = layerx_wire::hash::account_id_for_protocol(&account, 3)
        .unwrap_or_else(|error| panic!("protocol id: {error:?}"));
    let expected: String = expected
        .iter()
        .flat_map(|byte| {
            let digits = b"0123456789abcdef";
            [
                char::from(digits[usize::from(byte >> 4)]),
                char::from(digits[usize::from(byte & 15)]),
            ]
        })
        .collect();
    assert_eq!(result, serde_json::json!({"account": expected}));
}

#[test]
fn invalid_exported_accounts_refuse_without_output() {
    for value in [
        "{}".to_owned(),
        "[]".to_owned(),
        serde_json::json!({"did": format!("did:layerx:{}", "00".repeat(32))}).to_string(),
        serde_json::json!({"did": format!("did:layerx:{}", "AB".repeat(32))}).to_string(),
        serde_json::json!({"did": "did:key:other"}).to_string(),
        serde_json::json!({"did": "a".repeat(16384)}).to_string(),
    ] {
        let output = run(&value);
        assert!(!output.status.success());
        assert!(output.stdout.is_empty());
    }
}
