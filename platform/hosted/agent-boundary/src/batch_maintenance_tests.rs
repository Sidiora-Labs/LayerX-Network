use super::*;
use layerx_types::payload::{ActivityType, ModuleId, ModuleRegistration, ModuleRegistry};
use layerx_types::program_call::NativeProgramCall;
use layerx_wire::activity::decode_signed;
use layerx_wire::hash::{activity_id, payload_hash};

fn must<T, E: std::fmt::Debug>(result: Result<T, E>) -> T {
    result.unwrap_or_else(|failure| panic!("{failure:?}"))
}

fn fixture(name: &str) -> Vec<u8> {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
    must(std::fs::read(
        root.join("../../../tests/fixtures/programs/module-maintained-call")
            .join(name),
    ))
}

fn captured() -> serde_json::Value {
    serde_json::json!({
        "version": 1,
        "sequencer_public_key": hex(&fixture("sequencer.public")),
        "terminal_payload": hex(&fixture("terminal")),
        "call_graph": hex(&fixture("call-graph")),
        "evidence": {
            "header_hex": hex(&fixture("header")),
            "header_signature": hex(&fixture("header.signature")),
            "receipt_proof_hex": hex(&fixture("receipt.proof")),
            "batch_identity": {
                "kind": "batch_maintenance_v1",
                "receipt_hex": hex(&fixture("maintenance.receipt")),
                "receipt_proof_hex": hex(&fixture("maintenance.proof")),
                "activity_receipts_hex": [hex(&fixture("receipt"))]
            }
        }
    })
}

fn verify_capture(document: serde_json::Value) -> Result<(), String> {
    let stored: StoredExecution = serde_json::from_value(document).map_err(error)?;
    let activity_type = must(ActivityType::new(ModuleId::Programs, 3));
    let registration = must(ModuleRegistration::new(
        ModuleId::Programs,
        &[activity_type],
    ));
    let registry = must(ModuleRegistry::new(&[registration]));
    let signed = fixture("activity.lxa");
    let activity = must(decode_signed(&signed, &registry));
    let call = must(NativeProgramCall::decode(activity.payload()));
    verify(
        &stored,
        &fixture("receipt"),
        must(activity_id(&activity)),
        call.callee().bytes(),
        must(payload_hash(&activity)),
        call.guest_abi,
        activity.network_id(),
    )
}

#[test]
fn actual_module_maintained_call_binds_the_original_signed_execution() {
    must(verify_capture(captured()));
    let receipt = must(decode(&fixture("receipt")));
    let protocol = receipt
        .protocol()
        .unwrap_or_else(|| panic!("protocol receipt"));
    assert_eq!(protocol.protocol_version(), 3);
    assert_eq!(protocol.module_id(), 9);
    assert_eq!(protocol.operation(), 3);
    assert_eq!(protocol.result_code(), 0);
    assert_eq!(protocol.global_sequence(), 15);
}

#[test]
fn module_maintenance_requires_exact_kind_chain_and_authenticated_artifacts() {
    for case in 0..12 {
        let mut document = captured();
        match case {
            0 => {
                document["evidence"]["batch_identity"]["kind"] = "occupancy_maintenance_v2".into();
            }
            1 => {
                document["evidence"]["batch_identity"]["activity_receipts_hex"] =
                    serde_json::json!([]);
            }
            2 => {
                document["evidence"]["batch_identity"]
                    .as_object_mut()
                    .unwrap_or_else(|| panic!("identity"))
                    .remove("activity_receipts_hex");
            }
            3 => {
                document["evidence"]["batch_identity"] = serde_json::json!({"kind":"historical"});
            }
            4 => {
                document["evidence"]["batch_identity"]["receipt_proof_hex"] =
                    document["evidence"]["receipt_proof_hex"].clone();
            }
            5 => {
                document["sequencer_public_key"] = "00".repeat(32).into();
            }
            6..=10 => {
                let path: &[&str] = match case {
                    6 => &["terminal_payload"],
                    7 => &["call_graph"],
                    8 => &["evidence", "header_signature"],
                    9 => &["evidence", "batch_identity", "receipt_hex"],
                    _ => &["evidence", "header_hex"],
                };
                let mut value = &mut document;
                for key in path {
                    value = &mut value[*key];
                }
                let text = value.as_str().unwrap_or_else(|| panic!("bytes"));
                let mut bytes = must(canonical_hex(text, MAX_ACTIVITY_BYTES));
                let last = bytes.len() - 1;
                bytes[last] ^= 1;
                *value = hex(&bytes).into();
            }
            11 => {
                document["evidence"]["batch_identity"]["activity_receipts_hex"] =
                    serde_json::json!([hex(&fixture("receipt")), hex(&fixture("receipt"))]);
            }
            _ => unreachable!(),
        }
        assert!(verify_capture(document).is_err(), "case {case}");
    }
}
