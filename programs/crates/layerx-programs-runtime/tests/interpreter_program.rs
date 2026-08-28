use std::path::PathBuf;

use layerx_programs_runtime::{
    AuthorizationContext, AuthorizedExecutionRequest, CandidateActivityOutcome, Capability,
    CapabilitySet, CompositionContext, Executor, FeeSchedule, PrincipalId, ProgramId,
    ResourceBudget, Storage, StorageNamespace, UnavailableReceiptOracle, WasmEngine,
    CALL_ENTRY_EXPORT,
};

const SUCCESS_VECTORS: &[u8] =
    include_bytes!("../../../crates/layerx-programs-interpreter/vectors/v1-arithmetic.hex");
const REFUSAL_VECTORS: &[u8] =
    include_bytes!("../../../crates/layerx-programs-interpreter/vectors/v1-refusals.hex");
const ASSET: [u8; 32] = [1; 32];
const RECIPIENT: [u8; 32] = [2; 32];

#[derive(Clone, Copy, Debug)]
enum RefusalStage {
    StepCeiling,
    NonCanonicalRepeat,
    NestingDepth,
    ArithmeticOverflow,
    DivisionByZero,
    InvalidTransferAmount,
}

fn vectors(source: &[u8]) -> Vec<Vec<u8>> {
    fn nibble(byte: u8) -> u8 {
        match byte {
            b'0'..=b'9' => byte - b'0',
            b'a'..=b'f' => byte - b'a' + 10,
            _ => panic!("non-hex interpreter vector"),
        }
    }
    std::str::from_utf8(source)
        .unwrap_or_else(|error| panic!("vector utf8: {error}"))
        .lines()
        .filter(|line| !line.is_empty() && !line.starts_with('#'))
        .map(|line| {
            assert_eq!(line.len() % 2, 0);
            line.as_bytes()
                .chunks_exact(2)
                .map(|pair| (nibble(pair[0]) << 4) | nibble(pair[1]))
                .collect()
        })
        .collect()
}

fn artifact() -> Vec<u8> {
    let path = std::env::var_os("LAYERX_INTERPRETER_WASM")
        .map(PathBuf::from)
        .unwrap_or_else(|| panic!("LAYERX_INTERPRETER_WASM must name the built interpreter Wasm"));
    std::fs::read(&path).unwrap_or_else(|error| panic!("read {}: {error}", path.display()))
}

fn execute(
    wasm: &[u8],
    calldata: &[u8],
    storage: &mut Storage,
) -> layerx_programs_runtime::CandidateAuthorizedExecutionRecord {
    let program = ProgramId::new([0x33; 32]).unwrap_or_else(|error| panic!("program: {error}"));
    let principal =
        PrincipalId::new([0x44; 32]).unwrap_or_else(|error| panic!("principal: {error}"));
    let capabilities = CapabilitySet::new([
        Capability::StorageRead,
        Capability::StorageWrite,
        Capability::Transfer402 {
            asset: ASSET,
            to: RECIPIENT,
            maximum_amount: 100,
        },
    ])
    .unwrap_or_else(|error| panic!("capabilities: {error}"));
    let engine = WasmEngine::declared().unwrap_or_else(|error| panic!("engine: {error}"));
    let module = engine
        .validate_v2(wasm)
        .unwrap_or_else(|error| panic!("interpreter validation: {error}"));
    Executor::new(
        ResourceBudget::new_complete(
            10_000_000,
            16 * 1_024 * 1_024,
            1_048_576,
            1_048_576,
            64,
            1_048_576,
            4_096,
        ),
        FeeSchedule::declared(),
    )
    .execute_authorized_candidate(
        storage,
        AuthorizedExecutionRequest {
            module: &module,
            program,
            authorization: AuthorizationContext::new(principal, capabilities),
            receipts: &UnavailableReceiptOracle,
            entrypoint: CALL_ENTRY_EXPORT,
            calldata,
            composition: CompositionContext::isolated(),
            response_capacity: 4,
        },
    )
    .unwrap_or_else(|error| panic!("interpreter execution: {error}"))
}

fn namespace() -> StorageNamespace {
    StorageNamespace::principal(
        ProgramId::new([0x33; 32]).unwrap_or_else(|error| panic!("program: {error}")),
        PrincipalId::new([0x44; 32]).unwrap_or_else(|error| panic!("principal: {error}")),
    )
}

#[test]
fn built_interpreter_runs_success_vectors_through_the_real_candidate_runtime() {
    let wasm = artifact();
    let scripts = vectors(SUCCESS_VECTORS);
    assert_eq!(scripts.len(), 4);
    for (index, script) in scripts.iter().enumerate() {
        let mut storage = Storage::new();
        let record = execute(&wasm, script, &mut storage);
        let CandidateActivityOutcome::Success { response, effects } = record.outcome() else {
            panic!("success vector {index} refused");
        };
        let expected_steps = [5_u32, 17, 13, 5][index];
        assert_eq!(response.bytes.as_slice(), expected_steps.to_be_bytes());
        let mut transaction = storage.transaction(namespace());
        match index {
            0 => assert_eq!(transaction.read(b"sum"), Ok(Some(12_i64.to_be_bytes().to_vec()))),
            1 => {
                assert_eq!(transaction.read(b"a"), Ok(None));
                assert_eq!(effects.transfers.len(), 1);
                assert_eq!(effects.transfers[0].asset, ASSET);
                assert_eq!(effects.transfers[0].to, RECIPIENT);
                assert_eq!(effects.transfers[0].amount, 8);
            }
            2 => {
                for (key, value) in [(b"sub".as_slice(), 6_i64), (b"mul", 27), (b"div", 3), (b"eq", 0), (b"lt", 1)] {
                    assert_eq!(transaction.read(key), Ok(Some(value.to_be_bytes().to_vec())));
                }
            }
            3 => assert!(effects.transfers.is_empty()),
            _ => unreachable!(),
        }
    }
}

#[test]
fn built_interpreter_refusals_leave_real_runtime_state_and_effects_empty() {
    let wasm = artifact();
    let expected = [
        RefusalStage::StepCeiling,
        RefusalStage::NonCanonicalRepeat,
        RefusalStage::ArithmeticOverflow,
        RefusalStage::DivisionByZero,
        RefusalStage::ArithmeticOverflow,
        RefusalStage::InvalidTransferAmount,
        RefusalStage::NestingDepth,
        RefusalStage::ArithmeticOverflow,
    ];
    for (index, (script, expected_stage)) in vectors(REFUSAL_VECTORS).iter().zip(expected).enumerate() {
        if matches!(expected_stage, RefusalStage::ArithmeticOverflow | RefusalStage::DivisionByZero) {
            assert_eq!(script[5], 3, "{expected_stage:?} vector {index} register cardinality");
        }
        let mut storage = Storage::new();
        let before = storage.clone();
        let record = execute(&wasm, script, &mut storage);
        assert!(matches!(record.outcome(), CandidateActivityOutcome::Failure(_)), "{expected_stage:?} vector {index}");
        assert_eq!(storage, before, "{expected_stage:?} vector {index}");
    }
}
