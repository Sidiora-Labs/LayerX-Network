use layerx_wire::hash::{
    execution_batch_id, program_execution_batch_id, receipt_execution_batch_id,
    receipt_execution_batch_id_maintenance,
};
use layerx_wire::maintenance::decode_occupancy_maintenance;
#[path = "support/maintenance.rs"]
mod maintenance;
use layerx_wire::receipt::{decode, decode_batch_header};
use std::fmt::Debug;

fn must<T, E: Debug>(value: Result<T, E>) -> T {
    value.unwrap_or_else(|error| panic!("{error:?}"))
}

fn fixture_field(document: &str, field: &str) -> Vec<u8> {
    let marker = format!("\"{field}\": \"");
    let value = document
        .split(&marker)
        .nth(1)
        .unwrap_or_else(|| panic!("field"))
        .split('"')
        .next()
        .unwrap_or_else(|| panic!("value"));
    value
        .as_bytes()
        .chunks_exact(2)
        .map(|pair| {
            let pair = must(std::str::from_utf8(pair));
            must(u8::from_str_radix(pair, 16))
        })
        .collect()
}

#[test]
fn historical_and_maintained_identity_select_distinct_committed_ranges() {
    let document = include_str!(
        "../../../../platform/hosted/authority/tests/fixtures/real-program-deploy-receipt.json"
    );
    let receipt = must(decode(&fixture_field(document, "receipt_hex")));
    let protocol = receipt.protocol().unwrap_or_else(|| panic!("protocol"));
    let mut header_bytes = fixture_field(document, "header_hex");
    let legacy = must(decode_batch_header(&header_bytes));
    assert_eq!(
        receipt_execution_batch_id(protocol, &legacy),
        execution_batch_id(
            legacy.previous_state_root(),
            protocol.activity_id(),
            protocol.global_sequence(),
            legacy.batch_number()
        )
    );
    header_bytes[41..49].copy_from_slice(&(legacy.last_sequence() + 1).to_be_bytes());
    let header = must(decode_batch_header(&header_bytes));
    let maintenance = maintenance::maintenance_bytes(&header);
    let mut record = must(decode_occupancy_maintenance(&maintenance));
    let expected = program_execution_batch_id(
        header.previous_state_root(),
        header.activity_merkle_root(),
        header.first_sequence(),
        header.last_sequence() - 1,
        header.batch_number(),
    );
    assert_eq!(
        receipt_execution_batch_id_maintenance(protocol, &header, &record, 1),
        expected
    );
    assert_ne!(receipt_execution_batch_id(protocol, &header), expected);
    for count in [0, 2, u32::MAX] {
        assert!(receipt_execution_batch_id_maintenance(protocol, &header, &record, count).is_err());
    }
    record.global_sequence += 1;
    assert!(receipt_execution_batch_id_maintenance(protocol, &header, &record, 1).is_err());
    record.global_sequence -= 1;
    header_bytes[41..49].copy_from_slice(&(header.last_sequence() + 1).to_be_bytes());
    let changed = must(decode_batch_header(&header_bytes));
    assert!(receipt_execution_batch_id_maintenance(protocol, &changed, &record, 1).is_err());
}

#[test]
fn call_selection_keeps_activity_root_equality_and_excludes_maintenance_sequence() {
    let call_document = include_str!(
        "../../../../platform/sdk/conformance/fixtures/receipt-programs-positive-v3.json"
    );
    let receipt = must(decode(&fixture_field(
        call_document,
        "canonical_receipt_hex",
    )));
    let protocol = receipt.protocol().unwrap_or_else(|| panic!("protocol"));
    let document = include_str!(
        "../../../../platform/hosted/authority/tests/fixtures/real-program-deploy-receipt.json"
    );
    let mut bytes = fixture_field(document, "header_hex");
    let original = must(decode_batch_header(&bytes));
    let offset = bytes
        .windows(32)
        .position(|value| value == original.activity_merkle_root())
        .unwrap_or_else(|| panic!("root"));
    bytes[offset..offset + 32].copy_from_slice(&protocol.activity_root());
    bytes[32..40].copy_from_slice(&protocol.global_sequence().to_be_bytes());
    bytes[41..49].copy_from_slice(&(protocol.global_sequence() + 1).to_be_bytes());
    let header = must(decode_batch_header(&bytes));
    let maintenance = maintenance::maintenance_bytes(&header);
    let record = must(decode_occupancy_maintenance(&maintenance));
    assert_eq!(
        receipt_execution_batch_id(protocol, &header),
        program_execution_batch_id(
            header.previous_state_root(),
            header.activity_merkle_root(),
            header.first_sequence(),
            header.last_sequence(),
            header.batch_number()
        )
    );
    assert_eq!(
        receipt_execution_batch_id_maintenance(protocol, &header, &record, 1),
        program_execution_batch_id(
            header.previous_state_root(),
            header.activity_merkle_root(),
            header.first_sequence(),
            header.last_sequence() - 1,
            header.batch_number()
        )
    );
    bytes[offset] ^= 1;
    let changed = must(decode_batch_header(&bytes));
    assert!(receipt_execution_batch_id(protocol, &changed).is_err());
    assert!(receipt_execution_batch_id_maintenance(protocol, &changed, &record, 1).is_err());
}
