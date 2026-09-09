use std::io::Write;
use std::path::Path;
use std::process::{Command, Stdio};

use layerx_proof::inclusion::{verify_receipt, SequencerAuthorization};
use layerx_proof::merkle::{encode_proof, Proof};

fn hex(value: &str) -> Vec<u8> {
    value
        .as_bytes()
        .chunks_exact(2)
        .map(|pair| {
            let text = std::str::from_utf8(pair).unwrap_or_else(|e| panic!("hex: {e}"));
            u8::from_str_radix(text, 16).unwrap_or_else(|e| panic!("hex: {e}"))
        })
        .collect()
}

fn encoded(bytes: &[u8]) -> String {
    bytes
        .iter()
        .flat_map(|b| {
            let digits = b"0123456789abcdef";
            [
                char::from(digits[usize::from(b >> 4)]),
                char::from(digits[usize::from(b & 15)]),
            ]
        })
        .collect()
}

fn run(input: &str) -> std::process::Output {
    let mut child = Command::new(env!("CARGO_BIN_EXE_layerx-human-identity-provider"))
        .arg("validate-account-head")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap_or_else(|e| panic!("provider: {e}"));
    child
        .stdin
        .take()
        .unwrap_or_else(|| panic!("stdin"))
        .write_all(input.as_bytes())
        .unwrap_or_else(|e| panic!("write: {e}"));
    child
        .wait_with_output()
        .unwrap_or_else(|e| panic!("output: {e}"))
}

#[test]
fn real_retained_second_batch_cannot_initialize_fresh_limits() {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../../platform/hosted/authority/tests/fixtures/real-program-deploy-receipt.json");
    let source = std::fs::read(path).unwrap_or_else(|e| panic!("real fixture: {e}"));
    let value: serde_json::Value =
        serde_json::from_slice(&source).unwrap_or_else(|e| panic!("fixture JSON: {e}"));
    let text = |name| {
        value[name]
            .as_str()
            .unwrap_or_else(|| panic!("fixture field {name}"))
    };
    let siblings = value["proof_siblings"]
        .as_array()
        .unwrap_or_else(|| panic!("siblings"))
        .iter()
        .map(|v| {
            hex(v.as_str().unwrap_or_else(|| panic!("sibling")))
                .try_into()
                .unwrap_or_else(|_| panic!("sibling length"))
        })
        .collect();
    let number = |name| {
        u32::try_from(
            value[name]
                .as_u64()
                .unwrap_or_else(|| panic!("number {name}")),
        )
        .unwrap_or_else(|e| panic!("number {e}"))
    };
    let proof = Proof::new(number("proof_index"), number("proof_count"), siblings)
        .unwrap_or_else(|e| panic!("real proof: {e:?}"));
    let authorization = SequencerAuthorization::from_config(
        text("sequencer_id_hex"),
        text("sequencer_public_key_hex"),
        "1",
        "2",
    )
    .unwrap_or_else(|e| panic!("authorization: {e}"));
    let signature: [u8; 64] = hex(text("header_signature_hex"))
        .try_into()
        .unwrap_or_else(|_| panic!("signature"));
    let evidence = verify_receipt(
        &hex(text("receipt_hex")),
        &proof,
        &hex(text("header_hex")),
        &signature,
        &authorization,
    )
    .unwrap_or_else(|e| panic!("real receipt inclusion: {e:?}"));
    let header = evidence.header().header();
    assert_eq!(header.batch_number(), 2);
    let receipt = layerx_wire::receipt::decode(&hex(text("receipt_hex")))
        .unwrap_or_else(|e| panic!("receipt: {e:?}"));
    let digest = layerx_wire::hash::receipt_digest(
        &layerx_wire::receipt::encode_unsigned(&receipt)
            .unwrap_or_else(|e| panic!("unsigned: {e:?}")),
    )
    .unwrap_or_else(|e| panic!("digest: {e:?}"));
    let input = serde_json::json!({"network_id": header.network_id(),
        "sequencer_id": text("sequencer_id_hex"), "public_key": text("sequencer_public_key_hex"),
        "head": {"current": true, "receipt_hex": text("receipt_hex"), "receipt_digest": encoded(&digest),
            "state_root": encoded(&header.resulting_state_root()), "observed_sequence": header.last_sequence(),
            "observed_at": header.timestamp_ms(), "batch_evidence": {"header_hex": text("header_hex"),
                "header_signature": text("header_signature_hex"), "receipt_proof_hex": encoded(&encode_proof(&proof))}}});
    let output = run(&input.to_string());
    assert!(!output.status.success());
    assert!(output.stdout.is_empty());
}

#[test]
fn missing_and_malformed_heads_refuse_without_a_consumed_value() {
    for input in ["{}", "[]", "{\"head\":null}"] {
        let output = run(input);
        assert!(!output.status.success());
        assert!(output.stdout.is_empty());
    }
}
