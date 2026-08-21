use std::env;
use std::fs;

use layerx_program_sdk::{ProgramRefusal, RefusalClass as SdkRefusalClass, RefusalReason};
use layerx_programs_runtime::{
    AbiError, AuthorizationContext, AuthorizedExecutionRequest, Capability, CapabilitySet,
    CompositionContext, CompositionRules, Executor, PrincipalId, ProgramCatalog, ProgramId,
    ReceiptOracle, ReceiptView, Storage, WasmEngine, CALL_ENTRY_EXPORT,
};

struct NoReceipts;

impl ReceiptOracle for NoReceipts {
    fn verified_receipt(&self, _: [u8; 32]) -> Result<ReceiptView, AbiError> {
        Err(AbiError::ReceiptMismatch)
    }
}

fn run(
    wasm: &[u8],
    input: &[u8],
    nested: bool,
) -> layerx_programs_runtime::CandidateAuthorizedExecutionRecord {
    let engine = WasmEngine::declared().unwrap_or_else(|error| panic!("engine: {error}"));
    let root = engine
        .validate_candidate_v2(wasm)
        .unwrap_or_else(|error| panic!("root validation: {error}"));
    let child_id = ProgramId::new([7; 32]).unwrap_or_else(|error| panic!("child id: {error}"));
    let (capabilities, composition) = if nested {
        let child = engine
            .validate_candidate_v2(wasm)
            .unwrap_or_else(|error| panic!("child validation: {error}"));
        let capabilities = CapabilitySet::new([Capability::Call { program: child_id }])
            .unwrap_or_else(|error| panic!("capability: {error}"));
        let mut catalog = ProgramCatalog::new();
        catalog.insert(child_id, child);
        (
            capabilities,
            CompositionContext::catalog(catalog, CompositionRules::declared()),
        )
    } else {
        (CapabilitySet::empty(), CompositionContext::isolated())
    };
    Executor::declared()
        .execute_authorized_candidate(
            &mut Storage::new(),
            AuthorizedExecutionRequest {
                module: &root,
                program: ProgramId::new([6; 32]).unwrap_or_else(|error| panic!("root id: {error}")),
                authorization: AuthorizationContext::new(
                    PrincipalId::new([8; 32]).unwrap_or_else(|error| panic!("principal: {error}")),
                    capabilities,
                ),
                receipts: &NoReceipts,
                entrypoint: CALL_ENTRY_EXPORT,
                calldata: input,
                composition,
                response_capacity: input.len(),
            },
        )
        .unwrap_or_else(|error| panic!("candidate execution: {error}"))
}

fn qualify_sdk_failure_vocabulary() {
    let empty = RefusalReason::new(&[]).unwrap_or_else(|error| panic!("empty reason: {error}"));
    assert!(empty.bytes().is_empty());
    let maximum = vec![0xa5; layerx_program_sdk::MAX_REFUSAL_REASON_BYTES];
    assert_eq!(
        RefusalReason::new(&maximum)
            .unwrap_or_else(|error| panic!("maximum reason: {error}"))
            .bytes(),
        maximum
    );
    assert!(
        RefusalReason::new(&vec![0; layerx_program_sdk::MAX_REFUSAL_REASON_BYTES + 1]).is_err()
    );
    let empty_encoded = [0, 0, 0, 1, 0, 0, 0, 0];
    assert!(ProgramRefusal::decode(&empty_encoded)
        .unwrap_or_else(|error| panic!("empty refusal decode: {error}"))
        .reason()
        .bytes()
        .is_empty());
    let mut maximum_encoded = SdkRefusalClass::Unauthorized.code().to_be_bytes().to_vec();
    maximum_encoded.extend_from_slice(
        &u32::try_from(layerx_program_sdk::MAX_REFUSAL_REASON_BYTES)
            .unwrap_or_else(|error| panic!("maximum length: {error}"))
            .to_be_bytes(),
    );
    maximum_encoded.extend_from_slice(&maximum);
    assert_eq!(
        ProgramRefusal::decode(&maximum_encoded)
            .unwrap_or_else(|error| panic!("maximum refusal decode: {error}"))
            .reason()
            .bytes(),
        maximum
    );
    let one_past_length = layerx_program_sdk::MAX_REFUSAL_REASON_BYTES + 1;
    let mut one_past_encoded = SdkRefusalClass::Unauthorized.code().to_be_bytes().to_vec();
    one_past_encoded.extend_from_slice(
        &u32::try_from(one_past_length)
            .unwrap_or_else(|error| panic!("one-past length: {error}"))
            .to_be_bytes(),
    );
    one_past_encoded.extend(vec![0; one_past_length]);
    assert!(ProgramRefusal::decode(&one_past_encoded).is_err());
    let binary = [0, 0, 0, 2, 0, 0, 0, 3, 0, 0xff, 0x80];
    let decoded = ProgramRefusal::decode(&binary)
        .unwrap_or_else(|error| panic!("binary refusal decode: {error}"));
    assert_eq!(decoded.class(), SdkRefusalClass::InvalidInput);
    assert_eq!(decoded.reason().bytes(), [0, 0xff, 0x80]);
    assert!(ProgramRefusal::decode(&[0, 0, 0, 99, 0, 0, 0, 0]).is_err());
    assert!(ProgramRefusal::decode(&[0, 0, 0, 2, 0, 0, 0, 1]).is_err());
    assert!(ProgramRefusal::decode(&[0, 0, 0, 2, 0, 0, 0, 0, 7]).is_err());
}

fn main() {
    qualify_sdk_failure_vocabulary();
    let artifact = env::args()
        .nth(1)
        .unwrap_or_else(|| panic!("candidate response fixture artifact path is required"));
    let wasm = fs::read(&artifact).unwrap_or_else(|error| panic!("read {artifact}: {error}"));
    let child_id = ProgramId::new([7; 32]).unwrap_or_else(|error| panic!("child id: {error}"));
    let input = [0x11, 0, 0xff, 0x22];
    let record = run(&wasm, &input, true);

    assert_eq!(record.response().expect("success response").code, 0);
    assert_eq!(record.response().expect("success response").bytes, input);
    assert_eq!(record.call_graph().edges().len(), 1);
    assert_eq!(record.call_graph().edges()[0].callee(), child_id);
    assert_eq!(record.execution().usage().output_bytes, 7);

    let direct = run(&wasm, &[0xf0], false);
    let direct_failure = direct.failure().unwrap_or_else(|| panic!("direct failure"));
    assert_eq!(
        direct_failure.class(),
        layerx_programs_runtime::RefusalClass::InvalidInput
    );
    assert_eq!(direct_failure.reason().bytes(), [0, 0xff, 0x80]);

    let nested = run(&wasm, &[0xf1], true);
    let nested_failure = nested.failure().unwrap_or_else(|| panic!("nested failure"));
    assert_eq!(nested_failure.program(), child_id);
    assert_eq!(
        nested_failure.class(),
        layerx_programs_runtime::RefusalClass::InvalidInput
    );
    assert_eq!(nested_failure.reason().bytes(), [0, 0xff, 0x80]);

    let empty = run(&wasm, &[0xf2], false);
    assert!(empty
        .failure()
        .unwrap_or_else(|| panic!("empty failure"))
        .reason()
        .bytes()
        .is_empty());

    let maximum = run(&wasm, &[0xf3], false);
    let maximum_failure = maximum
        .failure()
        .unwrap_or_else(|| panic!("maximum failure"));
    assert_eq!(
        maximum_failure.class(),
        layerx_programs_runtime::RefusalClass::Unauthorized
    );
    assert_eq!(
        maximum_failure.reason().bytes(),
        [0xa5; layerx_program_sdk::MAX_REFUSAL_REASON_BYTES]
    );
    assert_eq!(
        maximum.execution().usage().output_bytes,
        layerx_program_sdk::MAX_REFUSAL_REASON_BYTES as u64
    );
}
