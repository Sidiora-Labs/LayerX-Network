use layerx_proof::program::{
    verify_authorized_program_execution, AuthorizedProgramExecutionExpectation,
    ProgramExecutionCheck,
};
use layerx_proof::receipt::{verify_program_outcome, AuthorizedBatch};
use layerx_wire::receipt::{decode, decode_applied_terminal};
use sha2::{Digest as _, Sha256};
use std::path::PathBuf;

fn bytes(document: &str, field: &str) -> Vec<u8> {
    let marker = format!("\"{field}\": \"");
    let parts: Vec<_> = document.split(&marker).collect();
    assert_eq!(parts.len(), 2, "unique field {field}");
    let hex = parts[1]
        .split('"')
        .next()
        .unwrap_or_else(|| panic!("field {field}"));
    assert_eq!(hex.len() % 2, 0);
    hex.as_bytes()
        .chunks_exact(2)
        .map(|pair| {
            let text = core::str::from_utf8(pair).unwrap_or_else(|error| panic!("{error}"));
            u8::from_str_radix(text, 16).unwrap_or_else(|error| panic!("{error}"))
        })
        .collect()
}

fn array(document: &str, field: &str) -> [u8; 32] {
    bytes(document, field)
        .try_into()
        .unwrap_or_else(|_| panic!("array {field}"))
}

fn load(name: &str) -> String {
    let directory = std::env::var_os("LAYERX_TERMINAL_FIXTURE_DIR").map_or_else(
        || {
            PathBuf::from(env!("CARGO_MANIFEST_DIR"))
                .join("../../../platform/sdk/conformance/fixtures")
        },
        PathBuf::from,
    );
    std::fs::read_to_string(directory.join(name))
        .unwrap_or_else(|error| panic!("fixture {name}: {error}"))
}

#[test]
fn real_executed_v4_and_signed_mutated_leg_refusal() {
    for (name, abi, mutated) in [
        ("receipt-programs-executed-v4.json", 2, false),
        ("receipt-programs-principal-v4.json", 1, false),
        ("receipt-programs-mutated-leg-v4.json", 2, true),
    ] {
        let document = load(name);
        let canonical = bytes(&document, "canonical_receipt_hex");
        let receipt = decode(&canonical).unwrap_or_else(|error| panic!("{error:?}"));
        let protocol = receipt
            .protocol()
            .unwrap_or_else(|| panic!("protocol receipt"));
        let outcome = protocol
            .program_outcome()
            .unwrap_or_else(|| panic!("program outcome"));
        assert_eq!(outcome.encoding_version(), 4);
        let authority = AuthorizedBatch::new(
            array(&document, "batch_id_hex"),
            array(&document, "asset_hex"),
            array(&document, "previous_state_root_hex"),
            array(&document, "resulting_state_root_hex"),
            array(&document, "sequencer_public_key_hex"),
        );
        assert!(
            verify_program_outcome(&canonical, &authority).is_ok(),
            "real signature for {name}"
        );
        assert_eq!(
            layerx_proof::receipt::verify_historical_program_outcome_v1(&canonical, &authority)
                .err()
                .map(|error| error.check),
            Some(layerx_proof::receipt::ReceiptCheck::ProtocolVersion)
        );
        let terminal = bytes(&document, "terminal_payload_hex");
        let (_, legs) =
            decode_applied_terminal(&terminal).unwrap_or_else(|error| panic!("{error:?}"));
        assert_eq!(legs.len(), 115);
        assert_eq!(
            <[u8; 32]>::from(Sha256::digest(legs)),
            outcome.applied_legs_digest()
        );
        let expected = AuthorizedProgramExecutionExpectation {
            authority,
            activity_id: protocol.activity_id(),
            program_id: array(&document, "program_id_hex"),
            guest_abi_version: abi,
        };
        let graph = bytes(&document, "call_graph_hex");
        let result = verify_authorized_program_execution(&canonical, &terminal, &graph, expected);
        if mutated {
            assert_eq!(
                result.err().map(|error| error.check),
                Some(ProgramExecutionCheck::TransferAuthority)
            );
        } else {
            assert!(result.is_ok(), "{name}: {:?}", result.err());
            for offset in [0, 1, 33, 65, 97, 112, 113, 114] {
                let mut changed = legs.to_vec();
                changed[offset] ^= 1;
                assert!(
                    layerx_programs_runtime::transfer::verify_applied_kernel_legs(
                        &changed,
                        outcome.transfer_root()
                    )
                    .is_err()
                );
            }
        }
    }
}

#[test]
fn stored_historical_protocol_v1_receipt_still_verifies_byte_exactly() {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../../platform/sdk/conformance/fixtures/receipt-programs-positive-v1.json");
    let document = std::fs::read_to_string(path).unwrap_or_else(|error| panic!("{error}"));
    let canonical = bytes(&document, "canonical_receipt_hex");
    let authority = AuthorizedBatch::new(
        array(&document, "batch_id_hex"),
        array(&document, "asset_hex"),
        array(&document, "previous_state_root_hex"),
        array(&document, "resulting_state_root_hex"),
        array(&document, "sequencer_public_key_hex"),
    );
    assert!(
        layerx_proof::receipt::verify_historical_program_outcome_v1(&canonical, &authority).is_ok()
    );
    assert_eq!(
        verify_program_outcome(&canonical, &authority)
            .err()
            .map(|error| error.check),
        Some(layerx_proof::receipt::ReceiptCheck::ProtocolVersion)
    );
    for index in 0..4 {
        let mut binding = [
            array(&document, "batch_id_hex"),
            array(&document, "previous_state_root_hex"),
            array(&document, "resulting_state_root_hex"),
            array(&document, "sequencer_public_key_hex"),
        ];
        binding[index][0] ^= 1;
        let changed = AuthorizedBatch::new(
            binding[0],
            array(&document, "asset_hex"),
            binding[1],
            binding[2],
            binding[3],
        );
        assert!(
            layerx_proof::receipt::verify_historical_program_outcome_v1(&canonical, &changed)
                .is_err()
        );
    }
    let decoded = decode(&canonical).unwrap_or_else(|error| panic!("{error:?}"));
    assert_eq!(layerx_wire::receipt::encode(&decoded), Ok(canonical));
    let protocol = decoded.protocol().unwrap_or_else(|| panic!("protocol"));
    assert_eq!(protocol.protocol_version(), 1);
    assert_eq!(
        protocol
            .program_outcome()
            .unwrap_or_else(|| panic!("outcome"))
            .applied_legs_digest(),
        [0; 32]
    );
}

#[test]
fn terminal_v4_envelope_refuses_bounds_truncation_and_trailing_bytes() {
    let document = load("receipt-programs-executed-v4.json");
    let terminal = bytes(&document, "terminal_payload_hex");
    for length in 0..terminal.len() {
        assert!(decode_applied_terminal(&terminal[..length]).is_err());
    }
    let mut changed = terminal.clone();
    changed.push(0);
    assert!(decode_applied_terminal(&changed).is_err());
    changed = terminal.clone();
    let length_offset = b"LXP/programs/terminal-applied-legs/v1\0".len();
    changed[length_offset..length_offset + 4].copy_from_slice(&u32::MAX.to_be_bytes());
    assert!(decode_applied_terminal(&changed).is_err());
    changed = terminal;
    changed[0] ^= 1;
    assert!(decode_applied_terminal(&changed).is_err());
}
