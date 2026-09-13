use ed25519_dalek::{Signer as _, SigningKey};
use layerx_programs_runtime::terminal::{
    decode_terminal_payload, FailureTerminal, TerminalDetail, EMPTY_CALL_GRAPH, PRE_RUNTIME_FAILURE,
};
use layerx_proof::program::{
    verify_program_execution, ProgramExecutionCheck, ProgramExecutionExpectation,
};
use layerx_types::intent::{ProgramCallFailure, ProgramCallOutcome};
use layerx_types::payload::{ActivityType, ModuleId, ModuleRegistration, ModuleRegistry};
use layerx_wire::activity::decode_signed;
use layerx_wire::hash::{activity_id, payload_hash, receipt_digest};
use layerx_wire::receipt::{decode, encode_unsigned};
use sha2::{Digest as _, Sha256};
use std::path::PathBuf;

fn fixture(name: &str) -> Vec<u8> {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../../tests/fixtures/programs/pre-runtime-refusal")
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
    assert_eq!(activity.network_id(), 7332);
    let native = layerx_types::program_call::NativeProgramCall::decode(activity.payload())
        .unwrap_or_else(|error| panic!("{error:?}"));
    assert_eq!(native.resources.0[0], 100_000_000);
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
fn actual_native_admission_refusal_authenticates_as_refused() {
    let terminal = fixture("terminal.hex");
    let graph = fixture("call-graph.hex");
    assert_eq!(terminal.len(), 165);
    assert_eq!(graph, EMPTY_CALL_GRAPH);
    let verified = verify_program_execution(&fixture("receipt.hex"), &terminal, &graph, expected())
        .unwrap_or_else(|error| panic!("{error:?}"));
    assert_eq!(verified.result_code(), -3);
    assert!(matches!(
        verified.outcome(),
        ProgramCallOutcome::Refused(ProgramCallFailure::GuestRefused { code: -3 })
    ));
    let TerminalDetail::Failure(FailureTerminal::PreRuntime(failure)) = &verified.terminal().detail
    else {
        panic!("pre-runtime refusal");
    };
    assert_eq!(failure.payload_hash, expected().payload_hash);
    assert_eq!(failure.module_version, 4);
    assert_eq!(failure.parameter_version, 1);
    assert_eq!(
        failure.applied_legs_digest,
        <[u8; 32]>::from(Sha256::digest([]))
    );
    assert!(verified.terminal().attachments.is_empty());
    let mut wrong_payload = expected();
    wrong_payload.payload_hash[0] ^= 1;
    assert_eq!(
        verify_program_execution(&fixture("receipt.hex"), &terminal, &graph, wrong_payload)
            .err()
            .map(|error| error.check),
        Some(ProgramExecutionCheck::Terminal)
    );
    let mut wrong_activity = expected();
    wrong_activity.activity_id[0] ^= 1;
    assert_eq!(
        verify_program_execution(&fixture("receipt.hex"), &terminal, &graph, wrong_activity)
            .err()
            .map(|error| error.check),
        Some(ProgramExecutionCheck::Activity)
    );
}

#[test]
fn native_refusal_codec_rejects_trailing_bytes_and_incomplete_fields() {
    let terminal = fixture("terminal.hex");
    for length in 0..terminal.len() {
        if length == terminal.len() - 33 {
            let decoded = decode_terminal_payload(2, 2, &terminal[..length])
                .unwrap_or_else(|error| panic!("historical v3 layout: {error:?}"));
            assert!(
                matches!(decoded.detail, TerminalDetail::Failure(FailureTerminal::PreRuntime(ref failure)) if failure.encoding_version == 3)
            );
        } else {
            assert!(
                decode_terminal_payload(2, 2, &terminal[..length]).is_err(),
                "truncated at {length}"
            );
        }
    }
    let mut trailing = terminal.clone();
    trailing.push(0);
    assert!(decode_terminal_payload(2, 2, &trailing).is_err());
    for kind in [0, 1, 3, 4] {
        assert!(decode_terminal_payload(kind, 2, &terminal).is_err());
    }
}

fn resign_terminal(terminal: &[u8], key: &SigningKey) -> Vec<u8> {
    let mut receipt = fixture("receipt.hex");
    let old: [u8; 32] = Sha256::digest(fixture("terminal.hex")).into();
    let new: [u8; 32] = Sha256::digest(terminal).into();
    let offsets = receipt
        .windows(32)
        .enumerate()
        .filter_map(|(i, value)| (value == old).then_some(i))
        .collect::<Vec<_>>();
    assert_eq!(offsets.len(), 1);
    receipt[offsets[0]..offsets[0] + 32].copy_from_slice(&new);
    let decoded = decode(&receipt).unwrap_or_else(|error| panic!("{error:?}"));
    let unsigned = encode_unsigned(&decoded).unwrap_or_else(|error| panic!("{error:?}"));
    let digest = receipt_digest(&unsigned).unwrap_or_else(|error| panic!("{error:?}"));
    let offset = receipt.len() - 64;
    receipt[offset..].copy_from_slice(&key.sign(&digest).to_bytes());
    assert!(layerx_proof::receipt::verify_sequencer_signature(
        &receipt,
        key.verifying_key().to_bytes()
    )
    .is_ok());
    receipt
}

#[test]
fn resigned_codec_mutations_cannot_rebind_native_refusal() {
    let key = SigningKey::from_bytes(&[3; 32]);
    let mut expected = expected();
    expected.sequencer_public_key = key.verifying_key().to_bytes();
    let graph = fixture("call-graph.hex");
    let terminal = fixture("terminal.hex");
    let receipt = resign_terminal(&terminal, &key);
    assert!(verify_program_execution(&receipt, &terminal, &graph, expected).is_ok());
    let base = PRE_RUNTIME_FAILURE.len();
    for offset in [
        0,
        base,
        base + 32,
        base + 67,
        base + 71,
        base + 75,
        base + 76,
        base + 77,
    ] {
        let mut changed = terminal.clone();
        changed[offset] ^= 1;
        let receipt = resign_terminal(&changed, &key);
        assert!(
            verify_program_execution(&receipt, &changed, &graph, expected).is_err(),
            "re-signed changed field {offset}"
        );
    }
    let historical = &terminal[..terminal.len() - 33];
    let receipt = resign_terminal(historical, &key);
    assert_eq!(
        verify_program_execution(&receipt, historical, &graph, expected)
            .err()
            .map(|error| error.check),
        Some(ProgramExecutionCheck::Terminal)
    );
}

#[test]
fn resigned_redundant_applied_wrapper_is_not_a_native_refusal() -> Result<(), layerx_wire::WireError>
{
    let terminal = fixture("terminal.hex");
    let mut encoder = layerx_wire::encode::Encoder::new(1024);
    encoder.fixed(b"LXP/programs/terminal-applied-legs/v1\0")?;
    encoder.bytes(&terminal, 1024)?;
    encoder.bytes(&[], 1024)?;
    let wrapped = encoder.finish();
    assert_eq!(
        layerx_wire::receipt::decode_applied_terminal(&wrapped)?,
        (terminal.as_slice(), &[][..])
    );
    let key = SigningKey::from_bytes(&[3; 32]);
    let receipt = resign_terminal(&wrapped, &key);
    let mut expected = expected();
    expected.sequencer_public_key = key.verifying_key().to_bytes();
    assert_eq!(
        verify_program_execution(&receipt, &wrapped, &fixture("call-graph.hex"), expected)
            .err()
            .map(|error| error.check),
        Some(ProgramExecutionCheck::Terminal)
    );
    Ok(())
}
