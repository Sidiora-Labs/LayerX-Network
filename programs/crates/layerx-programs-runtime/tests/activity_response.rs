#![allow(
    clippy::cast_possible_truncation,
    clippy::cast_possible_wrap,
    clippy::cast_sign_loss,
    clippy::similar_names
)]

use layerx_programs_runtime::abi::response::{CANDIDATE_ABI_MODULE, MAX_CALL_RESPONSE_BYTES};
use layerx_programs_runtime::test_support::{
    code_section, export_section, func_body, function_section, import_section, module,
    type_section, unsigned_leb, TYPE_I32,
};
use layerx_programs_runtime::{
    AbiError, AuthorizationContext, AuthorizedExecutionRequest, Capability, CapabilitySet,
    CompositionContext, CompositionRules, ExecutionError, Executor, FeeSchedule, PrincipalId,
    ProgramCatalog, ProgramId, ReceiptOracle, ReceiptView, ResourceBudget, ResponseRefusal,
    Storage, ValidationRefusal, WasmEngine, CALL_ENTRY_EXPORT,
};

struct NoReceipts;
impl ReceiptOracle for NoReceipts {
    fn verified_receipt(&self, _: [u8; 32]) -> Result<ReceiptView, AbiError> {
        Err(AbiError::ReceiptMismatch)
    }
}

fn data_section(segments: &[(i32, &[u8])]) -> Vec<u8> {
    let mut payload = unsigned_leb(segments.len() as u64);
    for (offset, bytes) in segments {
        payload.extend_from_slice(&[0, 0x41]);
        payload.extend(signed_leb(*offset));
        payload.push(0x0b);
        payload.extend(unsigned_leb(bytes.len() as u64));
        payload.extend_from_slice(bytes);
    }
    section(11, &payload)
}

fn echo_responder() -> Vec<u8> {
    let types = type_section(&[
        (&[TYPE_I32, TYPE_I32, TYPE_I32], &[TYPE_I32]),
        (&[TYPE_I32], &[TYPE_I32]),
        (&[TYPE_I32, TYPE_I32], &[TYPE_I32]),
    ]);
    let imports = import_section(&[(CANDIDATE_ABI_MODULE, "response_write", 0)]);
    let functions = function_section(&[1, 2]);
    let memory = section(5, &[1, 1, 1, 1]);
    let mut export_payload = unsigned_leb(3);
    for (name, kind, index) in [
        ("layerx_reserve", 0u8, 1u8),
        ("layerx_call", 0, 2),
        ("memory", 2, 0),
    ] {
        export_payload.extend(unsigned_leb(name.len() as u64));
        export_payload.extend_from_slice(name.as_bytes());
        export_payload.extend_from_slice(&[kind, index]);
    }
    let entry = func_body(
        &[],
        &[0x41, 7, 0x20, 0, 0x20, 1, 0x10, 0, 0x1a, 0x41, 7, 0x0b],
    );
    module(&[
        types,
        imports,
        functions,
        memory,
        section(7, &export_payload),
        code_section(&[func_body(&[], &[0x41, 0, 0x0b]), entry]),
    ])
}

fn repeated_forwarder(callee: ProgramId, first: &[u8], second: &[u8]) -> Vec<u8> {
    let types = type_section(&[
        (&[TYPE_I32, TYPE_I32, TYPE_I32], &[TYPE_I32]),
        (
            &[TYPE_I32; 8],
            &[layerx_programs_runtime::test_support::TYPE_I64],
        ),
        (&[TYPE_I32], &[TYPE_I32]),
        (&[TYPE_I32, TYPE_I32], &[TYPE_I32]),
    ]);
    let imports = import_section(&[
        (CANDIDATE_ABI_MODULE, "response_write", 0),
        (CANDIDATE_ABI_MODULE, "program_call_response", 1),
    ]);
    let functions = function_section(&[2, 3]);
    let memory = section(5, &[1, 1, 17, 17]);
    let mut export_payload = unsigned_leb(3);
    for (name, kind, index) in [
        ("layerx_reserve", 0u8, 2u8),
        ("layerx_call", 0, 3),
        ("memory", 2, 0),
    ] {
        export_payload.extend(unsigned_leb(name.len() as u64));
        export_payload.extend_from_slice(name.as_bytes());
        export_payload.extend_from_slice(&[kind, index]);
    }
    let mut body = Vec::new();
    let input_offsets = [64i32, 64 + first.len() as i32];
    let output_offsets = [2048i32, 2048 + first.len() as i32];
    for ((input, output), length) in input_offsets
        .into_iter()
        .zip(output_offsets)
        .zip([first.len(), second.len()])
    {
        for value in [0i32, 32, input, length as i32, 32, 2, output, length as i32] {
            body.push(0x41);
            body.extend(signed_leb(value));
        }
        body.extend_from_slice(&[0x10, 1, 0x1a]);
    }
    for value in [7i32, 2048, (first.len() + second.len()) as i32] {
        body.push(0x41);
        body.extend(signed_leb(value));
    }
    body.extend_from_slice(&[0x10, 0, 0x1a, 0x41, 7, 0x0b]);
    let mut inputs = first.to_vec();
    inputs.extend_from_slice(second);
    module(&[
        types,
        imports,
        functions,
        memory,
        section(7, &export_payload),
        code_section(&[func_body(&[], &[0x41, 0, 0x0b]), func_body(&[], &body)]),
        data_section(&[(0, &callee.bytes()), (32, &[0, 0]), (64, &inputs)]),
    ])
}

fn invalid_destination_forwarder(callee: ProgramId, pointer: i32, capacity: i32) -> Vec<u8> {
    response_forwarder(callee, pointer, capacity, &[0, 0])
}

fn response_forwarder(
    callee: ProgramId,
    pointer: i32,
    capacity: i32,
    encoded_capabilities: &[u8],
) -> Vec<u8> {
    let types = type_section(&[
        (
            &[TYPE_I32; 8],
            &[layerx_programs_runtime::test_support::TYPE_I64],
        ),
        (&[TYPE_I32], &[TYPE_I32]),
        (&[TYPE_I32, TYPE_I32], &[TYPE_I32]),
    ]);
    let imports = import_section(&[(CANDIDATE_ABI_MODULE, "program_call_response", 0)]);
    let functions = function_section(&[1, 2]);
    let memory = section(5, &[1, 1, 1, 1]);
    let mut export_payload = unsigned_leb(3);
    for (name, kind, index) in [
        ("layerx_reserve", 0u8, 1u8),
        ("layerx_call", 0, 2),
        ("memory", 2, 0),
    ] {
        export_payload.extend(unsigned_leb(name.len() as u64));
        export_payload.extend_from_slice(name.as_bytes());
        export_payload.extend_from_slice(&[kind, index]);
    }
    let mut body = Vec::new();
    for value in [
        0i32,
        32,
        32,
        0,
        32,
        encoded_capabilities.len() as i32,
        pointer,
        capacity,
    ] {
        body.push(0x41);
        body.extend(signed_leb(value));
    }
    body.extend_from_slice(&[0x10, 0, 0x1a, 0x41, 0, 0x0b]);
    module(&[
        types,
        imports,
        functions,
        memory,
        section(7, &export_payload),
        code_section(&[func_body(&[], &[0x41, 0, 0x0b]), func_body(&[], &body)]),
        data_section(&[(0, &callee.bytes()), (32, encoded_capabilities)]),
    ])
}

fn v1_forwarder(callee: ProgramId) -> Vec<u8> {
    legacy_forwarder(callee, &[0, 0])
}

fn legacy_forwarder(callee: ProgramId, encoded_capabilities: &[u8]) -> Vec<u8> {
    let types = type_section(&[
        (&[TYPE_I32; 6], &[TYPE_I32]),
        (&[TYPE_I32], &[TYPE_I32]),
        (&[TYPE_I32, TYPE_I32], &[TYPE_I32]),
    ]);
    let imports = import_section(&[("layerx_v1", "program_call", 0)]);
    let functions = function_section(&[1, 2]);
    let memory = section(5, &[1, 1, 1, 1]);
    let mut export_payload = unsigned_leb(3);
    for (name, kind, index) in [
        ("layerx_reserve", 0u8, 1u8),
        ("layerx_call", 0, 2),
        ("memory", 2, 0),
    ] {
        export_payload.extend(unsigned_leb(name.len() as u64));
        export_payload.extend_from_slice(name.as_bytes());
        export_payload.extend_from_slice(&[kind, index]);
    }
    let mut body = Vec::new();
    for value in [0i32, 32, 32, 0, 32, encoded_capabilities.len() as i32] {
        body.push(0x41);
        body.extend(signed_leb(value));
    }
    body.extend_from_slice(&[0x10, 0, 0x0b]);
    module(&[
        types,
        imports,
        functions,
        memory,
        section(7, &export_payload),
        code_section(&[func_body(&[], &[0x41, 0, 0x0b]), func_body(&[], &body)]),
        data_section(&[(0, &callee.bytes()), (32, encoded_capabilities)]),
    ])
}

fn trapping_start_candidate() -> Vec<u8> {
    let types = type_section(&[(&[], &[])]);
    module(&[
        types,
        function_section(&[0]),
        section(8, &[0]),
        code_section(&[func_body(&[], &[0x00, 0x0b])]),
    ])
}

fn start_publishing_responder(
    start_pointer: i32,
    start_length: i32,
    entry_publishes: bool,
    start_traps: bool,
) -> Vec<u8> {
    let types = type_section(&[
        (&[TYPE_I32, TYPE_I32, TYPE_I32], &[TYPE_I32]),
        (&[], &[]),
        (&[TYPE_I32], &[TYPE_I32]),
        (&[TYPE_I32, TYPE_I32], &[TYPE_I32]),
    ]);
    let imports = import_section(&[(CANDIDATE_ABI_MODULE, "response_write", 0)]);
    let functions = function_section(&[1, 2, 3]);
    let memory = section(5, &[1, 1, 1, 1]);
    let mut export_payload = unsigned_leb(3);
    for (name, kind, index) in [
        ("layerx_reserve", 0u8, 2u8),
        ("layerx_call", 0, 3),
        ("memory", 2, 0),
    ] {
        export_payload.extend(unsigned_leb(name.len() as u64));
        export_payload.extend_from_slice(name.as_bytes());
        export_payload.extend_from_slice(&[kind, index]);
    }
    let mut start = vec![0x41, 0, 0x41];
    start.extend(signed_leb(start_pointer));
    start.push(0x41);
    start.extend(signed_leb(start_length));
    start.extend_from_slice(&[0x10, 0, 0x1a]);
    if start_traps {
        start.push(0x00);
    }
    start.push(0x0b);
    let entry = if entry_publishes {
        func_body(
            &[],
            &[0x41, 0, 0x41, 0, 0x41, 1, 0x10, 0, 0x1a, 0x41, 0, 0x0b],
        )
    } else {
        func_body(&[], &[0x41, 0, 0x0b])
    };
    module(&[
        types,
        imports,
        functions,
        memory,
        section(7, &export_payload),
        section(8, &[1]),
        code_section(&[
            func_body(&[], &start),
            func_body(&[], &[0x41, 0, 0x0b]),
            entry,
        ]),
        data_section(&[(0, &[0xa5])]),
    ])
}

fn section(id: u8, payload: &[u8]) -> Vec<u8> {
    let mut encoded = vec![id];
    encoded.extend(unsigned_leb(payload.len() as u64));
    encoded.extend_from_slice(payload);
    encoded
}

fn signed_leb(mut value: i32) -> Vec<u8> {
    let mut encoded = Vec::new();
    loop {
        let byte = (value as u8) & 0x7f;
        value >>= 7;
        let done = (value == 0 && byte & 0x40 == 0) || (value == -1 && byte & 0x40 != 0);
        encoded.push(if done { byte } else { byte | 0x80 });
        if done {
            return encoded;
        }
    }
}

fn responder(bytes: &[u8], published_code: i32, returned_code: i32, publications: u8) -> Vec<u8> {
    responder_body(bytes, published_code, returned_code, publications, false)
}

fn trapping_responder(bytes: &[u8], code: i32) -> Vec<u8> {
    responder_body(bytes, code, code, 1, true)
}

fn responder_body(
    bytes: &[u8],
    published_code: i32,
    returned_code: i32,
    publications: u8,
    trap: bool,
) -> Vec<u8> {
    let types = type_section(&[
        (&[TYPE_I32, TYPE_I32, TYPE_I32], &[TYPE_I32]),
        (&[TYPE_I32], &[TYPE_I32]),
        (&[TYPE_I32, TYPE_I32], &[TYPE_I32]),
    ]);
    let imports = import_section(&[(CANDIDATE_ABI_MODULE, "response_write", 0)]);
    let functions = function_section(&[1, 2]);
    let memory = section(5, &[1, 1, 17, 17]);
    let mut export_payload = unsigned_leb(3);
    for (name, kind, index) in [
        ("layerx_reserve", 0u8, 1u8),
        ("layerx_call", 0, 2),
        ("memory", 2, 0),
    ] {
        export_payload.extend(unsigned_leb(name.len() as u64));
        export_payload.extend_from_slice(name.as_bytes());
        export_payload.extend_from_slice(&[kind, index]);
    }
    let exports = section(7, &export_payload);
    let mut call = Vec::new();
    for _ in 0..publications {
        call.push(0x41);
        call.extend(signed_leb(published_code));
        call.push(0x41);
        call.extend(signed_leb(1024));
        call.push(0x41);
        call.extend(signed_leb(bytes.len() as i32));
        call.extend_from_slice(&[0x10, 0, 0x1a]);
    }
    if trap {
        call.extend_from_slice(&[0x00, 0x0b]);
    } else {
        call.push(0x41);
        call.extend(signed_leb(returned_code));
        call.push(0x0b);
    }
    let reserve = func_body(&[], &[0x41, 0, 0x0b]);
    let mut data_payload = vec![1, 0, 0x41];
    data_payload.extend(signed_leb(1024));
    data_payload.push(0x0b);
    data_payload.extend(unsigned_leb(bytes.len() as u64));
    data_payload.extend_from_slice(bytes);
    let mut sections = vec![
        types,
        imports,
        functions,
        memory,
        exports,
        code_section(&[reserve, func_body(&[], &call)]),
    ];
    if bytes.iter().any(|byte| *byte != 0) {
        sections.push(section(11, &data_payload));
    }
    module(&sections)
}

fn execute(
    wasm: &[u8],
    capacity: usize,
    output_budget: u64,
    storage: &mut Storage,
) -> Result<layerx_programs_runtime::CandidateAuthorizedExecutionRecord, ExecutionError> {
    execute_with_capabilities(
        wasm,
        capacity,
        output_budget,
        storage,
        CapabilitySet::empty(),
    )
}

fn execute_with_capabilities(
    wasm: &[u8],
    capacity: usize,
    output_budget: u64,
    storage: &mut Storage,
    capabilities: CapabilitySet,
) -> Result<layerx_programs_runtime::CandidateAuthorizedExecutionRecord, ExecutionError> {
    let engine = WasmEngine::declared().unwrap_or_else(|error| panic!("engine: {error}"));
    let module = engine
        .validate_candidate_v2(wasm)
        .unwrap_or_else(|error| panic!("candidate validation: {error}"));
    Executor::new(
        ResourceBudget::declared().with_output_bytes(output_budget),
        FeeSchedule::declared(),
    )
    .execute_authorized_candidate(
        storage,
        AuthorizedExecutionRequest {
            module: &module,
            program: ProgramId::new([1; 32]).unwrap_or_else(|error| panic!("program: {error}")),
            authorization: AuthorizationContext::new(
                PrincipalId::new([2; 32]).unwrap_or_else(|error| panic!("principal: {error}")),
                capabilities,
            ),
            receipts: &NoReceipts,
            entrypoint: CALL_ENTRY_EXPORT,
            calldata: &[],
            composition: CompositionContext::isolated(),
            response_capacity: capacity,
        },
    )
}

fn execute_with_prices(
    wasm: &[u8],
    prices: FeeSchedule,
) -> layerx_programs_runtime::CandidateAuthorizedExecutionRecord {
    let engine = WasmEngine::declared().unwrap_or_else(|error| panic!("engine: {error}"));
    let module = engine
        .validate_candidate_v2(wasm)
        .unwrap_or_else(|error| panic!("candidate validation: {error}"));
    Executor::new(ResourceBudget::declared(), prices)
        .execute_authorized_candidate(
            &mut Storage::new(),
            AuthorizedExecutionRequest {
                module: &module,
                program: ProgramId::new([1; 32]).unwrap_or_else(|error| panic!("program: {error}")),
                authorization: AuthorizationContext::new(
                    PrincipalId::new([2; 32]).unwrap_or_else(|error| panic!("principal: {error}")),
                    CapabilitySet::empty(),
                ),
                receipts: &NoReceipts,
                entrypoint: CALL_ENTRY_EXPORT,
                calldata: &[],
                composition: CompositionContext::isolated(),
                response_capacity: 4,
            },
        )
        .unwrap_or_else(|error| panic!("candidate execution: {error}"))
}

fn writing_overflow_responder() -> Vec<u8> {
    writing_malformed_responder(1024, (MAX_CALL_RESPONSE_BYTES + 1) as i32)
}

fn writing_malformed_responder(pointer: i32, length: i32) -> Vec<u8> {
    let types = type_section(&[
        (&[TYPE_I32; 4], &[TYPE_I32]),
        (&[TYPE_I32, TYPE_I32, TYPE_I32], &[TYPE_I32]),
        (&[TYPE_I32], &[TYPE_I32]),
        (&[TYPE_I32, TYPE_I32], &[TYPE_I32]),
    ]);
    let imports = import_section(&[
        ("layerx_v1", "storage_write", 0),
        (CANDIDATE_ABI_MODULE, "response_write", 1),
    ]);
    let functions = function_section(&[2, 3]);
    let memory = section(5, &[1, 1, 17, 17]);
    let mut export_payload = unsigned_leb(3);
    for (name, kind, index) in [
        ("layerx_reserve", 0u8, 2u8),
        ("layerx_call", 0, 3),
        ("memory", 2, 0),
    ] {
        export_payload.extend(unsigned_leb(name.len() as u64));
        export_payload.extend_from_slice(name.as_bytes());
        export_payload.extend_from_slice(&[kind, index]);
    }
    let mut body = vec![
        0x41, 0, 0x41, 1, 0x41, 1, 0x41, 1, 0x10, 0, 0x1a, 0x41, 0, 0x41,
    ];
    body.extend(signed_leb(pointer));
    body.push(0x41);
    body.extend(signed_leb(length));
    body.extend_from_slice(&[0x10, 1, 0x1a, 0x41, 0, 0x0b]);
    module(&[
        types,
        imports,
        functions,
        memory,
        section(7, &export_payload),
        code_section(&[func_body(&[], &[0x41, 0, 0x0b]), func_body(&[], &body)]),
        data_section(&[(0, b"kv")]),
    ])
}

fn missing_memory_responder() -> Vec<u8> {
    let types = type_section(&[
        (&[TYPE_I32, TYPE_I32, TYPE_I32], &[TYPE_I32]),
        (&[TYPE_I32], &[TYPE_I32]),
        (&[TYPE_I32, TYPE_I32], &[TYPE_I32]),
    ]);
    module(&[
        types,
        import_section(&[(CANDIDATE_ABI_MODULE, "response_write", 0)]),
        function_section(&[1, 2]),
        export_section(&[("layerx_reserve", 1), ("layerx_call", 2)]),
        code_section(&[
            func_body(&[], &[0x41, 0, 0x0b]),
            func_body(
                &[],
                &[0x41, 0, 0x41, 0, 0x41, 1, 0x10, 0, 0x1a, 0x41, 0, 0x0b],
            ),
        ]),
    ])
}

fn writing_effect_response_responder() -> Vec<u8> {
    let types = type_section(&[
        (&[TYPE_I32; 4], &[TYPE_I32]),
        (&[TYPE_I32, TYPE_I32, TYPE_I32], &[TYPE_I32]),
        (&[TYPE_I32], &[TYPE_I32]),
        (&[TYPE_I32, TYPE_I32], &[TYPE_I32]),
    ]);
    let imports = import_section(&[
        ("layerx_v1", "storage_write", 0),
        ("layerx_v1", "event_emit", 0),
        (CANDIDATE_ABI_MODULE, "response_write", 1),
    ]);
    let functions = function_section(&[2, 3]);
    let memory = section(5, &[1, 1, 1, 1]);
    let mut export_payload = unsigned_leb(3);
    for (name, kind, index) in [
        ("layerx_reserve", 0u8, 3u8),
        ("layerx_call", 0, 4),
        ("memory", 2, 0),
    ] {
        export_payload.extend(unsigned_leb(name.len() as u64));
        export_payload.extend_from_slice(name.as_bytes());
        export_payload.extend_from_slice(&[kind, index]);
    }
    let body = [
        0x41, 0, 0x41, 1, 0x41, 1, 0x41, 1, 0x10, 0, 0x1a, 0x41, 0, 0x41, 1, 0x41, 1, 0x41, 1,
        0x10, 1, 0x1a, 0x41, 0, 0x41, 0, 0x41, 1, 0x10, 2, 0x1a, 0x41, 0, 0x0b,
    ];
    module(&[
        types,
        imports,
        functions,
        memory,
        section(7, &export_payload),
        code_section(&[func_body(&[], &[0x41, 0, 0x0b]), func_body(&[], &body)]),
        data_section(&[(0, b"kv")]),
    ])
}

#[test]
fn v1_refuses_candidate_import_and_candidate_returns_binary_response() {
    let bytes = [0, 0xff, 7, 0x80, 3];
    let wasm = responder(&bytes, 9, 9, 1);
    let engine = WasmEngine::declared().unwrap_or_else(|error| panic!("engine: {error}"));
    assert!(matches!(
        engine.validate(&wasm),
        Err(ValidationRefusal::ForbiddenImport { .. })
    ));
    let candidate = engine
        .validate_candidate_v2(&wasm)
        .unwrap_or_else(|error| panic!("candidate: {error}"));
    assert_eq!(
        Executor::declared().execute(&candidate, CALL_ENTRY_EXPORT, &[]),
        Err(ExecutionError::Abi(AbiError::WrongVersion))
    );
    let record = execute(&wasm, bytes.len(), 100, &mut Storage::new())
        .unwrap_or_else(|error| panic!("candidate execution: {error}"));
    assert_eq!(
        record
            .response()
            .unwrap_or_else(|| panic!("success response"))
            .code,
        9
    );
    assert_eq!(
        record
            .response()
            .unwrap_or_else(|| panic!("success response"))
            .bytes,
        bytes
    );
    assert_eq!(record.execution().usage().output_bytes, bytes.len() as u64);
    assert!(record
        .canonical_evidence()
        .starts_with(b"LXP/program-execution/v2-candidate\0"));
    let projection = record.receipt_projection();
    let encoded = projection.canonical_encode();
    let decoded = layerx_programs_runtime::CandidateActivityReceipt::canonical_decode(&encoded)
        .unwrap_or_else(|error| panic!("success receipt decode: {error}"));
    assert_eq!(decoded, projection);
    assert_eq!(decoded.canonical_encode(), encoded);
    let domain = b"LXP/candidate-activity-receipt/v2\0".len();
    let graph_length_offset = domain + 32 + 2 + 2 + 5 * 8 + 4 + 16;
    let graph_length = projection.graph_evidence().len();
    let outcome_offset = graph_length_offset + 4 + graph_length;
    let mut negative_success = encoded.clone();
    negative_success[outcome_offset + 1..outcome_offset + 5]
        .copy_from_slice(&(-1i32).to_be_bytes());
    assert!(
        layerx_programs_runtime::CandidateActivityReceipt::canonical_decode(&negative_success)
            .is_err()
    );
    let maximum_graph = b"LayerX/programs/call-graph/v1\0".len()
        + 32
        + 16
        + 8
        + (layerx_programs_runtime::DEFAULT_MAX_CALL_GRAPH_EDGES as usize * 68);
    let mut oversized_graph = encoded[..graph_length_offset].to_vec();
    oversized_graph.extend_from_slice(&((maximum_graph + 1) as u32).to_be_bytes());
    oversized_graph.extend(vec![0; maximum_graph + 1]);
    oversized_graph.extend_from_slice(&encoded[outcome_offset..]);
    assert!(
        layerx_programs_runtime::CandidateActivityReceipt::canonical_decode(&oversized_graph)
            .is_err()
    );
    let mut oversized_response = encoded[..=outcome_offset].to_vec();
    oversized_response.extend_from_slice(&0i32.to_be_bytes());
    oversized_response.extend_from_slice(&((MAX_CALL_RESPONSE_BYTES + 1) as u32).to_be_bytes());
    oversized_response.extend(vec![0; MAX_CALL_RESPONSE_BYTES + 1]);
    assert!(
        layerx_programs_runtime::CandidateActivityReceipt::canonical_decode(&oversized_response)
            .is_err()
    );
    for end in 0..encoded.len() {
        assert!(
            layerx_programs_runtime::CandidateActivityReceipt::canonical_decode(&encoded[..end])
                .is_err()
        );
    }
}

#[test]
fn candidate_evidence_binds_fee_units_in_addition_to_resource_counters() {
    let wasm = responder(&[1, 2, 3, 4], 7, 7, 1);
    let first = execute_with_prices(&wasm, FeeSchedule::declared().with_output_byte_price(1));
    let second = execute_with_prices(&wasm, FeeSchedule::declared().with_output_byte_price(9));

    assert_eq!(
        first.execution().usage().output_bytes,
        second.execution().usage().output_bytes
    );
    assert_eq!(first.response(), second.response());
    assert_ne!(
        first.execution().usage().fee_units,
        second.execution().usage().fee_units
    );
    assert_ne!(first.canonical_evidence(), second.canonical_evidence());
}

#[test]
fn candidate_start_response_refusals_are_configured_and_sticky_before_entry() {
    assert_eq!(
        execute(
            &start_publishing_responder(-1, 1, false, false),
            1,
            8,
            &mut Storage::new(),
        ),
        Err(ExecutionError::Response(
            ResponseRefusal::InvalidPublication
        ))
    );
    assert_eq!(
        execute(
            &start_publishing_responder(0, 1, true, false),
            1,
            8,
            &mut Storage::new(),
        ),
        Err(ExecutionError::Response(
            ResponseRefusal::DuplicatePublication
        ))
    );
    let trapped = execute(
        &start_publishing_responder(0, 1, false, true),
        1,
        8,
        &mut Storage::new(),
    )
    .unwrap_or_else(|error| panic!("start fault outcome: {error}"));
    let failure = trapped
        .failure()
        .unwrap_or_else(|| panic!("start runtime fault"));
    assert_eq!(
        failure.class(),
        layerx_programs_runtime::RefusalClass::RuntimeFault
    );
    assert!(failure.reason().bytes().is_empty());
    assert!(trapped.execution().usage().cpu_fuel > 0);
    let repeated = execute(
        &start_publishing_responder(0, 1, false, true),
        1,
        8,
        &mut Storage::new(),
    )
    .unwrap_or_else(|error| panic!("repeated start fault: {error}"));
    assert_eq!(
        trapped.execution().usage().cpu_fuel,
        repeated.execution().usage().cpu_fuel
    );
}

#[test]
fn nested_candidate_start_trap_preserves_leaf_runtime_fault() {
    let engine = WasmEngine::declared().unwrap_or_else(|error| panic!("engine: {error}"));
    let child_id = ProgramId::new([73; 32]).unwrap_or_else(|error| panic!("child: {error}"));
    let root_id = ProgramId::new([72; 32]).unwrap_or_else(|error| panic!("root: {error}"));
    let child = engine
        .validate_candidate_v2(&start_publishing_responder(0, 0, false, true))
        .unwrap_or_else(|error| panic!("child validation: {error}"));
    let root = engine
        .validate_candidate_v2(&response_forwarder(child_id, 1024, 0, &[0, 0]))
        .unwrap_or_else(|error| panic!("root validation: {error}"));
    let mut catalog = ProgramCatalog::new();
    catalog.insert(child_id, child);
    let capabilities = CapabilitySet::new([Capability::Call { program: child_id }])
        .unwrap_or_else(|error| panic!("capability: {error}"));
    let record = Executor::declared()
        .execute_authorized_candidate(
            &mut Storage::new(),
            AuthorizedExecutionRequest {
                module: &root,
                program: root_id,
                authorization: AuthorizationContext::new(
                    PrincipalId::new([74; 32]).unwrap_or_else(|error| panic!("principal: {error}")),
                    capabilities,
                ),
                receipts: &NoReceipts,
                entrypoint: CALL_ENTRY_EXPORT,
                calldata: &[],
                composition: CompositionContext::catalog(catalog, CompositionRules::declared()),
                response_capacity: 0,
            },
        )
        .unwrap_or_else(|error| panic!("nested start fault outcome: {error}"));
    let failure = record
        .failure()
        .unwrap_or_else(|| panic!("nested runtime fault"));
    assert_eq!(failure.program(), child_id);
    assert_eq!(
        failure.class(),
        layerx_programs_runtime::RefusalClass::RuntimeFault
    );
    assert!(failure.reason().bytes().is_empty());
    assert!(record.execution().usage().cpu_fuel > 0);
}

#[test]
fn empty_exact_maximum_and_capacity_refusal_are_not_truncated() {
    assert_eq!(
        execute(
            &trapping_start_candidate(),
            MAX_CALL_RESPONSE_BYTES + 1,
            u64::MAX,
            &mut Storage::new(),
        ),
        Err(ExecutionError::Response(ResponseRefusal::TooLarge {
            bytes: MAX_CALL_RESPONSE_BYTES + 1,
            limit: MAX_CALL_RESPONSE_BYTES,
        }))
    );
    let empty = execute(&responder(&[], 4, 4, 0), 0, 0, &mut Storage::new())
        .unwrap_or_else(|error| panic!("empty: {error}"));
    assert_eq!(
        empty
            .response()
            .unwrap_or_else(|| panic!("empty response"))
            .bytes,
        Vec::<u8>::new()
    );
    assert_eq!(
        empty
            .response()
            .unwrap_or_else(|| panic!("empty response"))
            .code,
        4
    );

    let maximum = vec![0; MAX_CALL_RESPONSE_BYTES];
    let exact = execute(
        &responder(&maximum, 1, 1, 1),
        maximum.len(),
        maximum.len() as u64,
        &mut Storage::new(),
    )
    .unwrap_or_else(|error| panic!("maximum: {error}"));
    assert_eq!(
        exact
            .response()
            .unwrap_or_else(|| panic!("exact response"))
            .bytes,
        maximum
    );

    let over = vec![0; MAX_CALL_RESPONSE_BYTES + 1];
    assert_eq!(
        execute(
            &responder(&over, 1, 1, 1),
            MAX_CALL_RESPONSE_BYTES,
            u64::MAX,
            &mut Storage::new()
        ),
        Err(ExecutionError::Response(ResponseRefusal::TooLarge {
            bytes: MAX_CALL_RESPONSE_BYTES + 1,
            limit: MAX_CALL_RESPONSE_BYTES,
        }))
    );

    let payload = [1, 2, 3, 4];
    assert_eq!(
        execute(
            &responder(&payload, 2, 2, 1),
            payload.len() - 1,
            100,
            &mut Storage::new()
        ),
        Err(ExecutionError::Response(
            ResponseRefusal::CapacityExceeded {
                bytes: 4,
                capacity: 3
            }
        ))
    );
}

#[test]
fn ignored_duplicate_code_mismatch_and_meter_refusals_stay_sticky() {
    let payload = [8, 9];
    assert_eq!(
        execute(&responder(&payload, 3, 3, 2), 2, 100, &mut Storage::new()),
        Err(ExecutionError::Response(
            ResponseRefusal::DuplicatePublication
        ))
    );
    assert_eq!(
        execute(&responder(&payload, 3, 4, 1), 2, 100, &mut Storage::new()),
        Err(ExecutionError::Response(ResponseRefusal::CodeMismatch {
            published: 3,
            returned: 4
        }))
    );
    assert!(matches!(
        execute(&responder(&payload, 3, 3, 1), 2, 1, &mut Storage::new()),
        Err(ExecutionError::Resource(_))
    ));
    assert!(matches!(
        execute(&responder(&payload, 3, 3, 2), 2, 1, &mut Storage::new()),
        Err(ExecutionError::Resource(
            layerx_programs_runtime::MeterRefusal::BudgetExceeded {
                resource: layerx_programs_runtime::ResourceKind::OutputBytes,
                limit: 1,
                attempted: 2,
            }
        ))
    ));
    let trapped_after_publication = execute(
        &trapping_responder(&payload, 3),
        2,
        100,
        &mut Storage::new(),
    )
    .unwrap_or_else(|error| panic!("runtime fault must be receipt-carriable: {error}"));
    let failure = trapped_after_publication
        .failure()
        .unwrap_or_else(|| panic!("runtime-fault outcome"));
    assert_eq!(
        failure.class(),
        layerx_programs_runtime::RefusalClass::RuntimeFault
    );
    assert!(failure.reason().bytes().is_empty());
}

#[test]
fn repeated_same_callee_fanout_keeps_edge_responses_distinct_and_charges_each_boundary() {
    let engine = WasmEngine::declared().unwrap_or_else(|error| panic!("engine: {error}"));
    let callee = ProgramId::new([9; 32]).unwrap_or_else(|error| panic!("callee: {error}"));
    let first = [0x11, 0, 0xff, 0x22];
    let second = [0x33, 0x80, 0, 0x44];
    let child = engine
        .validate_candidate_v2(&echo_responder())
        .unwrap_or_else(|error| panic!("child: {error}"));
    let root_bytes = repeated_forwarder(callee, &first, &second);
    let root = engine
        .validate_candidate_v2(&root_bytes)
        .unwrap_or_else(|error| panic!("root: {error}"));
    let mut catalog = ProgramCatalog::new();
    catalog.insert(callee, child);
    let caller = ProgramId::new([8; 32]).unwrap_or_else(|error| panic!("caller: {error}"));
    let capabilities = CapabilitySet::new([Capability::Call { program: callee }])
        .unwrap_or_else(|error| panic!("capability: {error}"));
    let record = Executor::new(ResourceBudget::declared(), FeeSchedule::declared())
        .execute_authorized_candidate(
            &mut Storage::new(),
            AuthorizedExecutionRequest {
                module: &root,
                program: caller,
                authorization: AuthorizationContext::new(
                    PrincipalId::new([2; 32]).unwrap_or_else(|error| panic!("principal: {error}")),
                    capabilities,
                ),
                receipts: &NoReceipts,
                entrypoint: CALL_ENTRY_EXPORT,
                calldata: &[],
                composition: CompositionContext::catalog(
                    catalog,
                    layerx_programs_runtime::CompositionRules::declared(),
                ),
                response_capacity: first.len() + second.len(),
            },
        )
        .unwrap_or_else(|error| panic!("nested response: {error}"));
    let mut expected = first.to_vec();
    expected.extend_from_slice(&second);
    assert_eq!(
        record
            .response()
            .unwrap_or_else(|| panic!("nested response"))
            .bytes,
        expected
    );
    assert_eq!(record.call_graph().edges().len(), 2);
    assert!(record
        .call_graph()
        .edges()
        .iter()
        .all(|edge| edge.callee() == callee));
    assert_eq!(
        record.execution().usage().output_bytes,
        (expected.len() * 2) as u64
    );
}

#[test]
fn response_refusal_after_storage_write_rolls_back_atomically() {
    let mut storage = Storage::new();
    let before = storage.clone();
    let capabilities = CapabilitySet::new([Capability::StorageWrite])
        .unwrap_or_else(|error| panic!("capability: {error}"));
    assert!(matches!(
        execute_with_capabilities(
            &writing_overflow_responder(),
            MAX_CALL_RESPONSE_BYTES,
            u64::MAX,
            &mut storage,
            capabilities
        ),
        Err(ExecutionError::Response(ResponseRefusal::TooLarge { .. }))
    ));
    assert_eq!(storage, before);

    for malformed in [
        writing_malformed_responder(-1, 1),
        writing_malformed_responder(1_114_111, 2),
        writing_malformed_responder(0, -1),
    ] {
        let mut storage = Storage::new();
        let before = storage.clone();
        let capabilities = CapabilitySet::new([Capability::StorageWrite])
            .unwrap_or_else(|error| panic!("capability: {error}"));
        assert_eq!(
            execute_with_capabilities(&malformed, 8, 8, &mut storage, capabilities),
            Err(ExecutionError::Response(
                ResponseRefusal::InvalidPublication
            ))
        );
        assert_eq!(storage, before);
    }
    assert_eq!(
        execute(&missing_memory_responder(), 8, 8, &mut Storage::new()),
        Err(ExecutionError::Response(
            ResponseRefusal::InvalidPublication
        ))
    );
}

#[test]
fn invalid_nested_destination_is_refused_before_child_start_or_graph_entry() {
    let child_id = ProgramId::new([6; 32]).unwrap_or_else(|error| panic!("child: {error}"));
    for (pointer, capacity) in [(65_535, 2), (-1, 1), (0, -1)] {
        let child_engine =
            WasmEngine::declared().unwrap_or_else(|error| panic!("child engine: {error}"));
        let child = child_engine
            .validate_candidate_v2(&trapping_start_candidate())
            .unwrap_or_else(|error| panic!("child validation: {error}"));
        let mut catalog = ProgramCatalog::new();
        catalog.insert(child_id, child);
        let root_engine =
            WasmEngine::declared().unwrap_or_else(|error| panic!("root engine: {error}"));
        let root = root_engine
            .validate_candidate_v2(&invalid_destination_forwarder(child_id, pointer, capacity))
            .unwrap_or_else(|error| panic!("root: {error}"));
        let capability = CapabilitySet::new([Capability::Call { program: child_id }])
            .unwrap_or_else(|error| panic!("capability: {error}"));
        let record = Executor::declared()
            .execute_authorized_candidate(
                &mut Storage::new(),
                AuthorizedExecutionRequest {
                    module: &root,
                    program: ProgramId::new([5; 32])
                        .unwrap_or_else(|error| panic!("root id: {error}")),
                    authorization: AuthorizationContext::new(
                        PrincipalId::new([2; 32])
                            .unwrap_or_else(|error| panic!("principal: {error}")),
                        capability,
                    ),
                    receipts: &NoReceipts,
                    entrypoint: CALL_ENTRY_EXPORT,
                    calldata: &[],
                    composition: CompositionContext::catalog(
                        catalog,
                        layerx_programs_runtime::CompositionRules::declared(),
                    ),
                    response_capacity: 0,
                },
            )
            .unwrap_or_else(|error| panic!("invalid destination must not enter child: {error}"));
        assert!(record.call_graph().edges().is_empty());
        assert!(record
            .response()
            .unwrap_or_else(|| panic!("empty response"))
            .bytes
            .is_empty());
    }

    let child_engine =
        WasmEngine::declared().unwrap_or_else(|error| panic!("child engine: {error}"));
    let child = child_engine
        .validate_candidate_v2(&echo_responder())
        .unwrap_or_else(|error| panic!("empty child: {error}"));
    let mut catalog = ProgramCatalog::new();
    catalog.insert(child_id, child);
    let root_engine = WasmEngine::declared().unwrap_or_else(|error| panic!("root engine: {error}"));
    let root = root_engine
        .validate_candidate_v2(&invalid_destination_forwarder(child_id, -1, 0))
        .unwrap_or_else(|error| panic!("zero-capacity root: {error}"));
    let capability = CapabilitySet::new([Capability::Call { program: child_id }])
        .unwrap_or_else(|error| panic!("capability: {error}"));
    let record = Executor::declared()
        .execute_authorized_candidate(
            &mut Storage::new(),
            AuthorizedExecutionRequest {
                module: &root,
                program: ProgramId::new([5; 32]).unwrap_or_else(|error| panic!("root id: {error}")),
                authorization: AuthorizationContext::new(
                    PrincipalId::new([2; 32]).unwrap_or_else(|error| panic!("principal: {error}")),
                    capability,
                ),
                receipts: &NoReceipts,
                entrypoint: CALL_ENTRY_EXPORT,
                calldata: &[],
                composition: CompositionContext::catalog(
                    catalog,
                    layerx_programs_runtime::CompositionRules::declared(),
                ),
                response_capacity: 0,
            },
        )
        .unwrap_or_else(|error| panic!("zero capacity: {error}"));
    assert_eq!(record.call_graph().edges().len(), 1);
    assert!(record
        .response()
        .unwrap_or_else(|| panic!("empty response"))
        .bytes
        .is_empty());
}

#[test]
fn legacy_candidate_call_cannot_discard_a_child_response_or_adopt_child_storage() {
    let engine = WasmEngine::declared().unwrap_or_else(|error| panic!("engine: {error}"));
    let child_id = ProgramId::new([6; 32]).unwrap_or_else(|error| panic!("child: {error}"));
    let child = engine
        .validate_candidate_v2(&writing_malformed_responder(0, 1))
        .unwrap_or_else(|error| panic!("child validation: {error}"));
    let root = engine
        .validate_candidate_v2(&legacy_forwarder(child_id, &[0, 1, 2]))
        .unwrap_or_else(|error| panic!("root validation: {error}"));
    let mut catalog = ProgramCatalog::new();
    catalog.insert(child_id, child);
    let capabilities = CapabilitySet::new([
        Capability::Call { program: child_id },
        Capability::StorageWrite,
    ])
    .unwrap_or_else(|error| panic!("capabilities: {error}"));
    let mut storage = Storage::new();
    let before = storage.clone();

    let result = Executor::declared().execute_authorized_candidate(
        &mut storage,
        AuthorizedExecutionRequest {
            module: &root,
            program: ProgramId::new([5; 32]).unwrap_or_else(|error| panic!("root id: {error}")),
            authorization: AuthorizationContext::new(
                PrincipalId::new([2; 32]).unwrap_or_else(|error| panic!("principal: {error}")),
                capabilities,
            ),
            receipts: &NoReceipts,
            entrypoint: CALL_ENTRY_EXPORT,
            calldata: &[],
            composition: CompositionContext::catalog(
                catalog,
                layerx_programs_runtime::CompositionRules::declared(),
            ),
            response_capacity: 0,
        },
    );

    assert_eq!(
        result,
        Err(ExecutionError::Composition(
            layerx_programs_runtime::CompositionRefusal::Response(
                ResponseRefusal::CapacityExceeded {
                    bytes: 1,
                    capacity: 0,
                },
            ),
        ))
    );
    assert_eq!(storage, before);
}

#[test]
fn nested_output_exhaustion_does_not_adopt_child_storage_effects_or_graph() {
    let engine = WasmEngine::declared().unwrap_or_else(|error| panic!("engine: {error}"));
    let child_id = ProgramId::new([6; 32]).unwrap_or_else(|error| panic!("child: {error}"));
    let child = engine
        .validate_candidate_v2(&writing_effect_response_responder())
        .unwrap_or_else(|error| panic!("child validation: {error}"));
    let root = engine
        .validate_candidate_v2(&response_forwarder(child_id, 1024, 1, &[0, 2, 2, 3]))
        .unwrap_or_else(|error| panic!("root validation: {error}"));
    let mut catalog = ProgramCatalog::new();
    catalog.insert(child_id, child);
    let capabilities = CapabilitySet::new([
        Capability::Call { program: child_id },
        Capability::StorageWrite,
        Capability::EmitEvent,
    ])
    .unwrap_or_else(|error| panic!("capabilities: {error}"));
    let mut storage = Storage::new();
    let before = storage.clone();

    let result = Executor::new(
        ResourceBudget::declared().with_output_bytes(0),
        FeeSchedule::declared(),
    )
    .execute_authorized_candidate(
        &mut storage,
        AuthorizedExecutionRequest {
            module: &root,
            program: ProgramId::new([5; 32]).unwrap_or_else(|error| panic!("root id: {error}")),
            authorization: AuthorizationContext::new(
                PrincipalId::new([2; 32]).unwrap_or_else(|error| panic!("principal: {error}")),
                capabilities,
            ),
            receipts: &NoReceipts,
            entrypoint: CALL_ENTRY_EXPORT,
            calldata: &[],
            composition: CompositionContext::catalog(
                catalog,
                layerx_programs_runtime::CompositionRules::declared(),
            ),
            response_capacity: 0,
        },
    );

    assert_eq!(
        result,
        Err(ExecutionError::Composition(
            layerx_programs_runtime::CompositionRefusal::Resource(
                layerx_programs_runtime::MeterRefusal::BudgetExceeded {
                    resource: layerx_programs_runtime::ResourceKind::OutputBytes,
                    limit: 0,
                    attempted: 1,
                },
            ),
        ))
    );
    assert_eq!(storage, before);
}

#[test]
fn composition_rejects_both_cross_revision_directions_before_child_start() {
    let child_id = ProgramId::new([7; 32]).unwrap_or_else(|error| panic!("child: {error}"));
    let root_id = ProgramId::new([8; 32]).unwrap_or_else(|error| panic!("root: {error}"));
    let principal = PrincipalId::new([2; 32]).unwrap_or_else(|error| panic!("principal: {error}"));
    let capability = || {
        CapabilitySet::new([Capability::Call { program: child_id }])
            .unwrap_or_else(|error| panic!("capability: {error}"))
    };

    let v1_to_candidate_engine =
        WasmEngine::declared().unwrap_or_else(|error| panic!("engine: {error}"));
    let v1_root = v1_to_candidate_engine
        .validate(&v1_forwarder(child_id))
        .unwrap_or_else(|error| panic!("v1 root: {error}"));
    let candidate_child = v1_to_candidate_engine
        .validate_candidate_v2(&trapping_start_candidate())
        .unwrap_or_else(|error| panic!("candidate child: {error}"));
    let mut catalog = ProgramCatalog::new();
    catalog.insert(child_id, candidate_child);
    assert_eq!(
        Executor::declared().execute_authorized(
            &mut Storage::new(),
            AuthorizedExecutionRequest {
                module: &v1_root,
                program: root_id,
                authorization: AuthorizationContext::new(principal, capability()),
                receipts: &NoReceipts,
                entrypoint: CALL_ENTRY_EXPORT,
                calldata: &[],
                composition: CompositionContext::catalog(
                    catalog,
                    layerx_programs_runtime::CompositionRules::declared()
                ),
                response_capacity: 0,
            }
        ),
        Err(ExecutionError::Composition(
            layerx_programs_runtime::CompositionRefusal::WrongVersion {
                expected: layerx_programs_runtime::AbiRevision::V1,
                actual: layerx_programs_runtime::AbiRevision::CandidateV2,
            }
        ))
    );

    let candidate_to_v1_engine =
        WasmEngine::declared().unwrap_or_else(|error| panic!("engine: {error}"));
    let candidate_root = candidate_to_v1_engine
        .validate_candidate_v2(&invalid_destination_forwarder(child_id, -1, 0))
        .unwrap_or_else(|error| panic!("candidate root: {error}"));
    let v1_child = candidate_to_v1_engine
        .validate(&trapping_start_candidate())
        .unwrap_or_else(|error| panic!("v1 child: {error}"));
    let mut catalog = ProgramCatalog::new();
    catalog.insert(child_id, v1_child);
    assert_eq!(
        Executor::declared().execute_authorized_candidate(
            &mut Storage::new(),
            AuthorizedExecutionRequest {
                module: &candidate_root,
                program: root_id,
                authorization: AuthorizationContext::new(principal, capability()),
                receipts: &NoReceipts,
                entrypoint: CALL_ENTRY_EXPORT,
                calldata: &[],
                composition: CompositionContext::catalog(
                    catalog,
                    layerx_programs_runtime::CompositionRules::declared()
                ),
                response_capacity: 0,
            }
        ),
        Err(ExecutionError::Composition(
            layerx_programs_runtime::CompositionRefusal::WrongVersion {
                expected: layerx_programs_runtime::AbiRevision::CandidateV2,
                actual: layerx_programs_runtime::AbiRevision::V1,
            }
        ))
    );
}
