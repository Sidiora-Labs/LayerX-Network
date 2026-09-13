use layerx_proof::program::{
    verify_program_execution, ProgramExecutionCheck, ProgramExecutionExpectation,
};
use layerx_types::intent::ProgramCallOutcome;
use layerx_types::payload::{ActivityType, ModuleId, ModuleRegistration, ModuleRegistry};
use layerx_wire::activity::decode_signed;
use layerx_wire::hash::{activity_id, payload_hash};
use layerx_wire::receipt::decode;
use std::path::PathBuf;

fn fixture(name: &str) -> Vec<u8> {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../../tests/fixtures/programs/emulator-response-code")
        .join(name);
    let text = std::fs::read_to_string(path).unwrap_or_else(|error| panic!("{error}"));
    let text = text.trim();
    assert_eq!(text.len() % 2, 0);
    text.as_bytes()
        .chunks_exact(2)
        .map(|pair| {
            u8::from_str_radix(
                core::str::from_utf8(pair).unwrap_or_else(|error| panic!("{error}")),
                16,
            )
            .unwrap_or_else(|error| panic!("{error}"))
        })
        .collect()
}

fn expected() -> ProgramExecutionExpectation {
    let call = ActivityType::new(ModuleId::Programs, 3).unwrap_or_else(|error| panic!("{error:?}"));
    let module = ModuleRegistration::new(ModuleId::Programs, &[call])
        .unwrap_or_else(|error| panic!("{error:?}"));
    let registry = ModuleRegistry::new(&[module]).unwrap_or_else(|error| panic!("{error:?}"));
    let activity = decode_signed(&fixture("signed-activity.hex"), &registry)
        .unwrap_or_else(|error| panic!("{error:?}"));
    let native = layerx_types::program_call::NativeProgramCall::decode(activity.payload())
        .unwrap_or_else(|error| panic!("{error:?}"));
    let receipt = decode(&fixture("receipt.hex")).unwrap_or_else(|error| panic!("{error:?}"));
    ProgramExecutionExpectation {
        sequencer_public_key: fixture("sequencer-public.hex")
            .try_into()
            .unwrap_or_else(|_| panic!("public key")),
        previous_state_root: receipt
            .protocol()
            .unwrap_or_else(|| panic!("protocol"))
            .previous_state_root(),
        activity_id: activity_id(&activity).unwrap_or_else(|error| panic!("{error:?}")),
        payload_hash: payload_hash(&activity).unwrap_or_else(|error| panic!("{error:?}")),
        program_id: native.callee().bytes(),
        guest_abi_version: native.guest_abi,
    }
}

#[test]
fn actual_guest_response_code_is_authenticated_by_the_signed_terminal_root() {
    let receipt = fixture("receipt.hex");
    let terminal = fixture("terminal.hex");
    let graph = fixture("call-graph.hex");
    let verified = verify_program_execution(&receipt, &terminal, &graph, expected())
        .unwrap_or_else(|error| panic!("{error:?}"));
    assert_eq!(verified.result_code(), 0);
    let ProgramCallOutcome::Completed(response) = verified.outcome() else {
        panic!("successful guest response");
    };
    assert_eq!(response.code(), 7);
    for offset in 0..terminal.len() {
        let mut changed = terminal.clone();
        changed[offset] ^= 1;
        assert_eq!(
            verify_program_execution(&receipt, &changed, &graph, expected())
                .err()
                .map(|error| error.check),
            Some(ProgramExecutionCheck::TerminalPayload),
            "changed signed terminal byte {offset}"
        );
    }
}
