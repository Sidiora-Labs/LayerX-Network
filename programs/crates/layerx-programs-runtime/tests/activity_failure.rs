#![allow(
    clippy::cast_possible_truncation,
    clippy::cast_possible_wrap,
    clippy::cast_sign_loss
)]

use layerx_programs_runtime::abi::response::CANDIDATE_ABI_MODULE;
use layerx_programs_runtime::test_support::{
    code_section, func_body, function_section, import_section, module, type_section, unsigned_leb,
    TYPE_I32,
};
use layerx_programs_runtime::{
    AbiError, AuthorizationContext, AuthorizedExecutionRequest, Capability, CapabilitySet,
    CompositionContext, CompositionRules, ExecutionError, Executor, FeeSchedule, PrincipalId,
    ProgramCatalog, ProgramId, ReceiptOracle, ReceiptView, RefusalClass, ResourceBudget,
    ResponseRefusal, Storage, WasmEngine, CALL_ENTRY_EXPORT, CANDIDATE_REFUSAL_SENTINEL,
    MAX_REFUSAL_REASON_BYTES,
};

struct NoReceipts;
impl ReceiptOracle for NoReceipts {
    fn verified_receipt(&self, _: [u8; 32]) -> Result<ReceiptView, AbiError> {
        Err(AbiError::ReceiptMismatch)
    }
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

fn refusing_program(reason: &[u8], class: RefusalClass) -> Vec<u8> {
    refusal_writer_program(
        reason,
        class,
        &[(0, reason.len() as i32)],
        CANDIDATE_REFUSAL_SENTINEL,
        false,
    )
}

fn refusal_writer_program(
    reason: &[u8],
    class: RefusalClass,
    writes: &[(i32, i32)],
    returned: i32,
    trap: bool,
) -> Vec<u8> {
    raw_refusal_writer_program(reason, class.code() as i32, writes, returned, trap, true)
}

fn raw_refusal_writer_program(
    reason: &[u8],
    class_code: i32,
    writes: &[(i32, i32)],
    returned: i32,
    trap: bool,
    memory_exported: bool,
) -> Vec<u8> {
    let types = type_section(&[
        (&[TYPE_I32, TYPE_I32, TYPE_I32], &[TYPE_I32]),
        (&[TYPE_I32], &[TYPE_I32]),
        (&[TYPE_I32, TYPE_I32], &[TYPE_I32]),
    ]);
    let imports = import_section(&[(CANDIDATE_ABI_MODULE, "refusal_write", 0)]);
    let functions = function_section(&[1, 2]);
    let memory = section(5, &[1, 1, 1, 1]);
    let export_entries = if memory_exported { 3 } else { 2 };
    let mut exports = unsigned_leb(export_entries);
    let all_exports = [
        ("layerx_reserve", 0u8, 1u8),
        ("layerx_call", 0, 2),
        ("memory", 2, 0),
    ];
    for (name, kind, index) in all_exports.into_iter().take(export_entries as usize) {
        exports.extend(unsigned_leb(name.len() as u64));
        exports.extend_from_slice(name.as_bytes());
        exports.extend_from_slice(&[kind, index]);
    }
    let mut entry = Vec::new();
    for (pointer, length) in writes {
        entry.push(0x41);
        entry.extend(signed_leb(class_code));
        entry.push(0x41);
        entry.extend(signed_leb(*pointer));
        entry.push(0x41);
        entry.extend(signed_leb(*length));
        entry.extend_from_slice(&[0x10, 0, 0x1a]);
    }
    if trap {
        entry.push(0x00);
    } else {
        entry.push(0x41);
        entry.extend(signed_leb(returned));
    }
    entry.push(0x0b);
    let mut data = vec![1, 0, 0x41, 0, 0x0b];
    data.extend(unsigned_leb(reason.len() as u64));
    data.extend_from_slice(reason);
    module(&[
        types,
        imports,
        functions,
        memory,
        section(7, &exports),
        code_section(&[func_body(&[], &[0x41, 0, 0x0b]), func_body(&[], &entry)]),
        section(11, &data),
    ])
}

fn refusal_then_exhaust_program(reason: &[u8], class: RefusalClass) -> Vec<u8> {
    let types = type_section(&[
        (&[TYPE_I32, TYPE_I32, TYPE_I32], &[TYPE_I32]),
        (&[TYPE_I32], &[TYPE_I32]),
        (&[TYPE_I32, TYPE_I32], &[TYPE_I32]),
    ]);
    let imports = import_section(&[(CANDIDATE_ABI_MODULE, "refusal_write", 0)]);
    let functions = function_section(&[1, 2]);
    let memory = section(5, &[1, 1, 1, 1]);
    let mut exports = unsigned_leb(3);
    for (name, kind, index) in [
        ("layerx_reserve", 0u8, 1u8),
        ("layerx_call", 0, 2),
        ("memory", 2, 0),
    ] {
        exports.extend(unsigned_leb(name.len() as u64));
        exports.extend_from_slice(name.as_bytes());
        exports.extend_from_slice(&[kind, index]);
    }
    let mut entry = vec![0x41];
    entry.extend(signed_leb(class.code() as i32));
    entry.push(0x41);
    entry.extend(signed_leb(0));
    entry.push(0x41);
    entry.extend(signed_leb(reason.len() as i32));
    entry.extend_from_slice(&[
        0x10, 0, 0x1a, // publish and ignore status
        0x03, 0x40, // loop
        0x0c, 0,    // branch to loop forever
        0x0b, // end loop
        0x41,
    ]);
    entry.extend(signed_leb(CANDIDATE_REFUSAL_SENTINEL));
    entry.push(0x0b);
    module(&[
        types,
        imports,
        functions,
        memory,
        section(7, &exports),
        code_section(&[func_body(&[], &[0x41, 0, 0x0b]), func_body(&[], &entry)]),
        data_section(&[(0, reason)]),
    ])
}

fn start_refusing_program(
    reason: &[u8],
    class_code: i32,
    pointer: i32,
    length: i32,
    entry_return: i32,
    start_traps: bool,
    stage_storage: bool,
) -> Vec<u8> {
    let types = type_section(&[
        (&[TYPE_I32, TYPE_I32, TYPE_I32], &[TYPE_I32]),
        (&[TYPE_I32, TYPE_I32, TYPE_I32, TYPE_I32], &[TYPE_I32]),
        (&[], &[]),
        (&[TYPE_I32], &[TYPE_I32]),
        (&[TYPE_I32, TYPE_I32], &[TYPE_I32]),
    ]);
    let imports = import_section(&[
        (CANDIDATE_ABI_MODULE, "refusal_write", 0),
        ("layerx_v1", "storage_write", 1),
    ]);
    let functions = function_section(&[2, 3, 4]);
    let memory = section(5, &[1, 1, 1, 1]);
    let mut exports = unsigned_leb(3);
    for (name, kind, index) in [
        ("layerx_reserve", 0u8, 3u8),
        ("layerx_call", 0, 4),
        ("memory", 2, 0),
    ] {
        exports.extend(unsigned_leb(name.len() as u64));
        exports.extend_from_slice(name.as_bytes());
        exports.extend_from_slice(&[kind, index]);
    }
    let mut start = Vec::new();
    if stage_storage {
        for value in [8_192i32, 1, 8_193, 1] {
            start.push(0x41);
            start.extend(signed_leb(value));
        }
        start.extend_from_slice(&[0x10, 1, 0x1a]);
    }
    for value in [class_code, pointer, length] {
        start.push(0x41);
        start.extend(signed_leb(value));
    }
    start.extend_from_slice(&[0x10, 0, 0x1a]);
    if start_traps {
        start.push(0x00);
    }
    start.push(0x0b);
    let mut entry = vec![0x41];
    entry.extend(signed_leb(entry_return));
    entry.push(0x0b);
    module(&[
        types,
        imports,
        functions,
        memory,
        section(7, &exports),
        section(8, &[2]),
        code_section(&[
            func_body(&[], &start),
            func_body(&[], &[0x41, 0, 0x0b]),
            func_body(&[], &entry),
        ]),
        data_section(&[(0, reason), (8_192, &[0x6b, 0x76])]),
    ])
}

fn response_forwarder(callee: ProgramId, capacity: i32) -> Vec<u8> {
    response_forwarder_with_capabilities(callee, capacity, &[0, 0])
}

fn response_forwarder_with_capabilities(
    callee: ProgramId,
    capacity: i32,
    capabilities: &[u8],
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
    let mut exports = unsigned_leb(3);
    for (name, kind, index) in [
        ("layerx_reserve", 0u8, 1u8),
        ("layerx_call", 0, 2),
        ("memory", 2, 0),
    ] {
        exports.extend(unsigned_leb(name.len() as u64));
        exports.extend_from_slice(name.as_bytes());
        exports.extend_from_slice(&[kind, index]);
    }
    let mut entry = Vec::new();
    for value in [
        0i32,
        32,
        32,
        0,
        32,
        capabilities.len() as i32,
        1024,
        capacity,
    ] {
        entry.push(0x41);
        entry.extend(signed_leb(value));
    }
    entry.extend_from_slice(&[0x10, 0, 0x1a, 0x41, 0, 0x0b]);
    let mut data = vec![2, 0, 0x41, 0, 0x0b, 32];
    data.extend_from_slice(&callee.bytes());
    data.extend_from_slice(&[0, 0x41, 32, 0x0b]);
    data.extend(unsigned_leb(capabilities.len() as u64));
    data.extend_from_slice(capabilities);
    module(&[
        types,
        imports,
        functions,
        memory,
        section(7, &exports),
        code_section(&[func_body(&[], &[0x41, 0, 0x0b]), func_body(&[], &entry)]),
        section(11, &data),
    ])
}

fn staged_forwarder(callee: ProgramId, capabilities: &[u8]) -> Vec<u8> {
    let types = type_section(&[
        (&[TYPE_I32; 4], &[TYPE_I32]),
        (
            &[
                layerx_programs_runtime::test_support::TYPE_I64,
                layerx_programs_runtime::test_support::TYPE_I64,
                TYPE_I32,
                TYPE_I32,
                TYPE_I32,
                TYPE_I32,
            ],
            &[TYPE_I32],
        ),
        (
            &[TYPE_I32; 8],
            &[layerx_programs_runtime::test_support::TYPE_I64],
        ),
        (&[TYPE_I32], &[TYPE_I32]),
        (&[TYPE_I32, TYPE_I32], &[TYPE_I32]),
    ]);
    let imports = import_section(&[
        ("layerx_v1", "storage_write", 0),
        ("layerx_v1", "event_emit", 0),
        ("layerx_v1", "transfer_402", 1),
        (CANDIDATE_ABI_MODULE, "program_call_response", 2),
    ]);
    let functions = function_section(&[3, 4]);
    let memory = section(5, &[1, 1, 1, 1]);
    let mut exports = unsigned_leb(3);
    for (name, kind, index) in [
        ("layerx_reserve", 0u8, 4u8),
        ("layerx_call", 0, 5),
        ("memory", 2, 0),
    ] {
        exports.extend(unsigned_leb(name.len() as u64));
        exports.extend_from_slice(name.as_bytes());
        exports.extend_from_slice(&[kind, index]);
    }
    let mut body = vec![
        0x41, 0, 0x41, 1, 0x41, 1, 0x41, 1, 0x10, 0, 0x1a, 0x41, 0, 0x41, 1, 0x41, 1, 0x41, 1,
        0x10, 1, 0x1a, 0x42, 0, 0x42, 1, 0x41,
    ];
    body.extend(signed_leb(128));
    body.extend_from_slice(&[0x41, 32, 0x41]);
    body.extend(signed_leb(160));
    body.extend_from_slice(&[0x41, 32, 0x10, 2, 0x1a]);
    for value in [64i32, 32, 96, 0, 256, capabilities.len() as i32, 1024, 0] {
        body.push(0x41);
        body.extend(signed_leb(value));
    }
    body.extend_from_slice(&[0x10, 3, 0x1a, 0x41, 0, 0x0b]);
    let callee_bytes = callee.bytes();
    module(&[
        types,
        imports,
        functions,
        memory,
        section(7, &exports),
        code_section(&[func_body(&[], &[0x41, 0, 0x0b]), func_body(&[], &body)]),
        data_section(&[
            (0, &b"kv"[..]),
            (64, &callee_bytes),
            (256, capabilities),
            (128, &[9; 32]),
            (160, &[10; 32]),
        ]),
    ])
}

fn staged_refusing_program(reason: &[u8]) -> Vec<u8> {
    let types = type_section(&[
        (&[TYPE_I32; 4], &[TYPE_I32]),
        (
            &[
                layerx_programs_runtime::test_support::TYPE_I64,
                layerx_programs_runtime::test_support::TYPE_I64,
                TYPE_I32,
                TYPE_I32,
                TYPE_I32,
                TYPE_I32,
            ],
            &[TYPE_I32],
        ),
        (&[TYPE_I32, TYPE_I32, TYPE_I32], &[TYPE_I32]),
        (&[TYPE_I32], &[TYPE_I32]),
        (&[TYPE_I32, TYPE_I32], &[TYPE_I32]),
    ]);
    let imports = import_section(&[
        ("layerx_v1", "storage_write", 0),
        ("layerx_v1", "event_emit", 0),
        ("layerx_v1", "transfer_402", 1),
        (CANDIDATE_ABI_MODULE, "refusal_write", 2),
    ]);
    let functions = function_section(&[3, 4]);
    let memory = section(5, &[1, 1, 1, 1]);
    let mut exports = unsigned_leb(3);
    for (name, kind, index) in [
        ("layerx_reserve", 0u8, 4u8),
        ("layerx_call", 0, 5),
        ("memory", 2, 0),
    ] {
        exports.extend(unsigned_leb(name.len() as u64));
        exports.extend_from_slice(name.as_bytes());
        exports.extend_from_slice(&[kind, index]);
    }
    let mut body = vec![
        0x41, 0, 0x41, 1, 0x41, 1, 0x41, 1, 0x10, 0, 0x1a, 0x41, 0, 0x41, 1, 0x41, 1, 0x41, 1,
        0x10, 1, 0x1a, 0x42, 0, 0x42, 1, 0x41,
    ];
    body.extend(signed_leb(128));
    body.extend_from_slice(&[0x41, 32, 0x41]);
    body.extend(signed_leb(160));
    body.extend_from_slice(&[0x41, 32, 0x10, 2, 0x1a, 0x41, 1, 0x41, 2, 0x41]);
    body.extend(signed_leb(reason.len() as i32));
    body.extend_from_slice(&[0x10, 3, 0x1a, 0x41]);
    body.extend(signed_leb(CANDIDATE_REFUSAL_SENTINEL));
    body.push(0x0b);
    let mut data = b"kv".to_vec();
    data.extend_from_slice(reason);
    module(&[
        types,
        imports,
        functions,
        memory,
        section(7, &exports),
        code_section(&[func_body(&[], &[0x41, 0, 0x0b]), func_body(&[], &body)]),
        data_section(&[(0, &data), (128, &[9; 32]), (160, &[10; 32])]),
    ])
}

fn staged_runtime_fault_program() -> Vec<u8> {
    let types = type_section(&[
        (&[TYPE_I32; 4], &[TYPE_I32]),
        (
            &[
                layerx_programs_runtime::test_support::TYPE_I64,
                layerx_programs_runtime::test_support::TYPE_I64,
                TYPE_I32,
                TYPE_I32,
                TYPE_I32,
                TYPE_I32,
            ],
            &[TYPE_I32],
        ),
        (&[TYPE_I32], &[TYPE_I32]),
        (&[TYPE_I32, TYPE_I32], &[TYPE_I32]),
    ]);
    let imports = import_section(&[
        ("layerx_v1", "storage_write", 0),
        ("layerx_v1", "event_emit", 0),
        ("layerx_v1", "transfer_402", 1),
    ]);
    let functions = function_section(&[2, 3]);
    let memory = section(5, &[1, 1, 1, 1]);
    let mut exports = unsigned_leb(3);
    for (name, kind, index) in [
        ("layerx_reserve", 0u8, 3u8),
        ("layerx_call", 0, 4),
        ("memory", 2, 0),
    ] {
        exports.extend(unsigned_leb(name.len() as u64));
        exports.extend_from_slice(name.as_bytes());
        exports.extend_from_slice(&[kind, index]);
    }
    let mut body = vec![
        0x41, 0, 0x41, 1, 0x41, 1, 0x41, 1, 0x10, 0, 0x1a, 0x41, 0, 0x41, 1, 0x41, 1, 0x41, 1,
        0x10, 1, 0x1a, 0x42, 0, 0x42, 1, 0x41,
    ];
    body.extend(signed_leb(128));
    body.extend_from_slice(&[0x41, 32, 0x41]);
    body.extend(signed_leb(160));
    body.extend_from_slice(&[0x41, 32, 0x10, 2, 0x1a, 0x00, 0x0b]);
    module(&[
        types,
        imports,
        functions,
        memory,
        section(7, &exports),
        code_section(&[func_body(&[], &[0x41, 0, 0x0b]), func_body(&[], &body)]),
        data_section(&[(0, &b"kv"[..]), (128, &[9; 32]), (160, &[10; 32])]),
    ])
}

fn cross_publication_program(response_first: bool, returned: i32) -> Vec<u8> {
    let types = type_section(&[
        (&[TYPE_I32, TYPE_I32, TYPE_I32], &[TYPE_I32]),
        (&[TYPE_I32], &[TYPE_I32]),
        (&[TYPE_I32, TYPE_I32], &[TYPE_I32]),
    ]);
    let imports = import_section(&[
        (CANDIDATE_ABI_MODULE, "response_write", 0),
        (CANDIDATE_ABI_MODULE, "refusal_write", 0),
    ]);
    let functions = function_section(&[1, 2]);
    let memory = section(5, &[1, 1, 1, 1]);
    let mut exports = unsigned_leb(3);
    for (name, kind, index) in [
        ("layerx_reserve", 0u8, 2u8),
        ("layerx_call", 0, 3),
        ("memory", 2, 0),
    ] {
        exports.extend(unsigned_leb(name.len() as u64));
        exports.extend_from_slice(name.as_bytes());
        exports.extend_from_slice(&[kind, index]);
    }
    let response = [0x41, 0, 0x41, 0, 0x41, 1, 0x10, 0, 0x1a];
    let refusal = [0x41, 1, 0x41, 1, 0x41, 2, 0x10, 1, 0x1a];
    let mut entry = Vec::new();
    if response_first {
        entry.extend_from_slice(&response);
        entry.extend_from_slice(&refusal);
    } else {
        entry.extend_from_slice(&refusal);
        entry.extend_from_slice(&response);
    }
    entry.push(0x41);
    entry.extend(signed_leb(returned));
    entry.push(0x0b);
    module(&[
        types,
        imports,
        functions,
        memory,
        section(7, &exports),
        code_section(&[func_body(&[], &[0x41, 0, 0x0b]), func_body(&[], &entry)]),
        section(11, &[1, 0, 0x41, 0, 0x0b, 3, 0xaa, 0, 0xff]),
    ])
}

fn run_root(
    wasm: &[u8],
    budget: ResourceBudget,
    prices: FeeSchedule,
) -> Result<layerx_programs_runtime::CandidateAuthorizedExecutionRecord, ExecutionError> {
    execute_root(
        wasm,
        budget,
        prices,
        CapabilitySet::empty(),
        &mut Storage::new(),
    )
}

fn execute_root(
    wasm: &[u8],
    budget: ResourceBudget,
    prices: FeeSchedule,
    capabilities: CapabilitySet,
    storage: &mut Storage,
) -> Result<layerx_programs_runtime::CandidateAuthorizedExecutionRecord, ExecutionError> {
    let engine = WasmEngine::declared().unwrap_or_else(|error| panic!("engine: {error}"));
    let module = engine
        .validate_candidate_v2(wasm)
        .unwrap_or_else(|error| panic!("validation: {error}"));
    Executor::new(budget, prices).execute_authorized_candidate(
        storage,
        AuthorizedExecutionRequest {
            module: &module,
            program: ProgramId::new([90; 32]).unwrap_or_else(|error| panic!("program: {error}")),
            authorization: AuthorizationContext::new(
                PrincipalId::new([91; 32]).unwrap_or_else(|error| panic!("principal: {error}")),
                capabilities,
            ),
            receipts: &NoReceipts,
            entrypoint: CALL_ENTRY_EXPORT,
            calldata: &[],
            composition: CompositionContext::isolated(),
            response_capacity: 0,
        },
    )
}

#[test]
fn start_refusal_publication_is_preconfigured_sticky_metered_and_atomic() {
    let reason = [0, 0xff, 0x81];
    let capabilities = CapabilitySet::new([Capability::StorageWrite])
        .unwrap_or_else(|error| panic!("capability: {error}"));
    let mut storage = Storage::new();
    let before = storage.clone();
    let record = execute_root(
        &start_refusing_program(
            &reason,
            RefusalClass::Conflict.code() as i32,
            0,
            reason.len() as i32,
            CANDIDATE_REFUSAL_SENTINEL,
            true,
            true,
        ),
        ResourceBudget::declared().with_output_bytes(reason.len() as u64),
        FeeSchedule::declared(),
        capabilities,
        &mut storage,
    )
    .unwrap_or_else(|error| panic!("published start refusal must win: {error}"));
    let failure = record.failure().unwrap_or_else(|| panic!("failure"));
    assert_eq!(failure.program(), record.root_program());
    assert_eq!(failure.class(), RefusalClass::Conflict);
    assert_eq!(failure.reason().bytes(), reason);
    assert_eq!(record.execution().usage().output_bytes, reason.len() as u64);
    assert!(record.execution().usage().cpu_fuel > 0);
    assert!(record.execution().usage().storage_write_bytes > 0);
    assert_eq!(storage, before);
    assert!(record.effects().is_none());

    let completed = run_root(
        &start_refusing_program(
            &reason,
            RefusalClass::Conflict.code() as i32,
            0,
            reason.len() as i32,
            CANDIDATE_REFUSAL_SENTINEL,
            false,
            false,
        ),
        ResourceBudget::declared().with_output_bytes(reason.len() as u64),
        FeeSchedule::declared(),
    )
    .unwrap_or_else(|error| panic!("start refusal followed by sentinel: {error}"));
    assert_eq!(completed.failure(), record.failure());
    assert_eq!(
        completed.execution().usage().output_bytes,
        reason.len() as u64
    );

    let invalid = execute_root(
        &start_refusing_program(
            &[7],
            RefusalClass::Rejected.code() as i32,
            -1,
            1,
            CANDIDATE_REFUSAL_SENTINEL,
            false,
            true,
        ),
        ResourceBudget::declared().with_output_bytes(1),
        FeeSchedule::declared(),
        CapabilitySet::new([Capability::StorageWrite])
            .unwrap_or_else(|error| panic!("capability: {error}")),
        &mut storage,
    );
    assert_eq!(
        invalid,
        Err(ExecutionError::Composition(
            layerx_programs_runtime::CompositionRefusal::Response(
                ResponseRefusal::InvalidPublication
            )
        ))
    );
    assert_eq!(storage, before);
}

#[test]
fn nested_start_refusal_preserves_leaf_identity_usage_and_rollback() {
    let engine = WasmEngine::declared().unwrap_or_else(|error| panic!("engine: {error}"));
    let root_id = ProgramId::new([92; 32]).unwrap_or_else(|error| panic!("root: {error}"));
    let child_id = ProgramId::new([93; 32]).unwrap_or_else(|error| panic!("child: {error}"));
    let reason = [0x41, 0, 0xff, 0x42];
    let child = engine
        .validate_candidate_v2(&start_refusing_program(
            &reason,
            RefusalClass::NotFound.code() as i32,
            0,
            reason.len() as i32,
            CANDIDATE_REFUSAL_SENTINEL,
            true,
            true,
        ))
        .unwrap_or_else(|error| panic!("child validation: {error}"));
    let delegated = CapabilitySet::new([Capability::StorageWrite])
        .unwrap_or_else(|error| panic!("delegated capability: {error}"));
    let root = engine
        .validate_candidate_v2(&response_forwarder_with_capabilities(
            child_id,
            reason.len() as i32,
            &delegated.canonical_encoding(),
        ))
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
    let record = Executor::declared()
        .execute_authorized_candidate(
            &mut storage,
            AuthorizedExecutionRequest {
                module: &root,
                program: root_id,
                authorization: AuthorizationContext::new(
                    PrincipalId::new([94; 32]).unwrap_or_else(|error| panic!("principal: {error}")),
                    capabilities,
                ),
                receipts: &NoReceipts,
                entrypoint: CALL_ENTRY_EXPORT,
                calldata: &[],
                composition: CompositionContext::catalog(catalog, CompositionRules::declared()),
                response_capacity: 0,
            },
        )
        .unwrap_or_else(|error| panic!("nested start refusal: {error}"));
    let failure = record.failure().unwrap_or_else(|| panic!("failure"));
    assert_eq!(failure.program(), child_id);
    assert_eq!(failure.class(), RefusalClass::NotFound);
    assert_eq!(failure.reason().bytes(), reason);
    assert_eq!(record.execution().usage().output_bytes, reason.len() as u64);
    assert!(record.execution().usage().cpu_fuel > 0);
    assert!(record.execution().usage().storage_write_bytes > 0);
    assert_eq!(record.call_graph().edges().len(), 1);
    assert_eq!(storage, before);
    assert!(record.effects().is_none());
}

#[test]
#[allow(clippy::too_many_lines)]
fn root_binary_refusal_is_receipt_carriable_with_usage() {
    let engine = WasmEngine::declared().unwrap_or_else(|error| panic!("engine: {error}"));
    let reason = [0, 0xff, 0x80, 7];
    let module = engine
        .validate_candidate_v2(&refusing_program(&reason, RefusalClass::InvalidInput))
        .unwrap_or_else(|error| panic!("validation: {error}"));
    let program = ProgramId::new([9; 32]).unwrap_or_else(|error| panic!("program: {error}"));
    let record = Executor::declared()
        .execute_authorized_candidate(
            &mut Storage::new(),
            AuthorizedExecutionRequest {
                module: &module,
                program,
                authorization: AuthorizationContext::new(
                    PrincipalId::new([8; 32]).unwrap_or_else(|error| panic!("principal: {error}")),
                    CapabilitySet::empty(),
                ),
                receipts: &NoReceipts,
                entrypoint: CALL_ENTRY_EXPORT,
                calldata: &[],
                composition: CompositionContext::isolated(),
                response_capacity: 0,
            },
        )
        .unwrap_or_else(|error| panic!("failure must be an outcome: {error}"));

    let failure = record.failure().unwrap_or_else(|| panic!("failure"));
    assert_eq!(failure.program(), program);
    assert_eq!(failure.class(), RefusalClass::InvalidInput);
    assert_eq!(failure.reason().bytes(), reason);
    assert_eq!(record.execution().usage().output_bytes, reason.len() as u64);
    assert!(record.effects().is_none());
    let projection = record.receipt_projection();
    let encoded = projection.canonical_encode();
    assert_eq!(
        layerx_programs_runtime::CandidateActivityReceipt::canonical_decode(&encoded),
        Ok(projection.clone())
    );
    let mut trailing = encoded.clone();
    trailing.push(0);
    assert!(
        layerx_programs_runtime::CandidateActivityReceipt::canonical_decode(&trailing).is_err()
    );
    let mut wrong_revision = encoded.clone();
    let revision_offset = b"LXP/program-activity-receipt/v4\0".len() + 32;
    wrong_revision[revision_offset..revision_offset + 2].copy_from_slice(&1u16.to_be_bytes());
    assert!(
        layerx_programs_runtime::CandidateActivityReceipt::canonical_decode(&wrong_revision)
            .is_err()
    );

    let domain = b"LXP/program-activity-receipt/v4\0".len();
    let usage_offset = domain + 32 + 2 + 2 + 4 + 4;
    let graph_length_offset = usage_offset + 5 * 8 + 4 + 16;
    let trace_length = projection
        .trace_evidence()
        .map_or(1, |trace| 1 + 4 + trace.len());
    let outcome_offset = graph_length_offset + 4 + projection.graph_evidence().len() + trace_length;
    let failure_offset = outcome_offset + 1 + 4;
    let decode = |bytes: &[u8]| {
        layerx_programs_runtime::CandidateActivityReceipt::canonical_decode(bytes)
            .unwrap_or_else(|error| panic!("tampered canonical receipt: {error}"))
    };

    assert_eq!(encoded[outcome_offset], 1);
    let mut unknown_outcome = encoded.clone();
    unknown_outcome[outcome_offset] = 2;
    assert_eq!(
        layerx_programs_runtime::CandidateActivityReceipt::canonical_decode(&unknown_outcome),
        Err(layerx_programs_runtime::FailureEncodingError::Malformed)
    );

    let mut changed_root = encoded.clone();
    changed_root[domain] ^= 1;
    let changed_root_receipt = decode(&changed_root);
    assert_ne!(
        changed_root_receipt.root_program(),
        projection.root_program()
    );
    assert_eq!(changed_root_receipt.canonical_encode(), changed_root);

    let mut changed_refusing_program = encoded.clone();
    changed_refusing_program[failure_offset] ^= 1;
    let changed_failure_receipt = decode(&changed_refusing_program);
    let layerx_programs_runtime::CandidateReceiptOutcome::Failure(changed_failure) =
        changed_failure_receipt.outcome()
    else {
        panic!("failure outcome")
    };
    assert_ne!(changed_failure.program(), failure.program());
    assert_eq!(
        changed_failure_receipt.canonical_encode(),
        changed_refusing_program
    );

    let mut unknown_class = encoded.clone();
    unknown_class[failure_offset + 32..failure_offset + 36].copy_from_slice(&99u32.to_be_bytes());
    assert!(
        layerx_programs_runtime::CandidateActivityReceipt::canonical_decode(&unknown_class)
            .is_err()
    );

    let mut changed_reason = encoded.clone();
    changed_reason[failure_offset + 40] ^= 1;
    let changed_reason_receipt = decode(&changed_reason);
    assert_ne!(changed_reason_receipt, projection);
    assert_eq!(changed_reason_receipt.canonical_encode(), changed_reason);

    let mut changed_usage = encoded.clone();
    changed_usage[usage_offset + 7] ^= 1;
    let changed_usage_receipt = decode(&changed_usage);
    assert_ne!(changed_usage_receipt.usage(), projection.usage());
    assert_eq!(changed_usage_receipt.canonical_encode(), changed_usage);

    let fee_offset = usage_offset + 5 * 8 + 4;
    let mut changed_fee = encoded.clone();
    changed_fee[fee_offset + 15] ^= 1;
    let changed_fee_receipt = decode(&changed_fee);
    assert_ne!(
        changed_fee_receipt.usage().fee_units,
        projection.usage().fee_units
    );
    assert_eq!(changed_fee_receipt.canonical_encode(), changed_fee);
}

#[test]
fn depth_one_refusal_preserves_leaf_and_reason_usage() {
    let engine = WasmEngine::declared().unwrap_or_else(|error| panic!("engine: {error}"));
    let leaf_id = ProgramId::new([7; 32]).unwrap_or_else(|error| panic!("leaf: {error}"));
    let root_id = ProgramId::new([6; 32]).unwrap_or_else(|error| panic!("root: {error}"));
    let reason = [0x11, 0, 0xff, 0x22];
    let leaf = engine
        .validate_candidate_v2(&refusing_program(&reason, RefusalClass::Conflict))
        .unwrap_or_else(|error| panic!("leaf validation: {error}"));
    let root = engine
        .validate_candidate_v2(&response_forwarder(leaf_id, reason.len() as i32))
        .unwrap_or_else(|error| panic!("root validation: {error}"));
    let mut catalog = ProgramCatalog::new();
    catalog.insert(leaf_id, leaf);
    let capabilities = CapabilitySet::new([Capability::Call { program: leaf_id }])
        .unwrap_or_else(|error| panic!("capability: {error}"));
    let record = Executor::declared()
        .execute_authorized_candidate(
            &mut Storage::new(),
            AuthorizedExecutionRequest {
                module: &root,
                program: root_id,
                authorization: AuthorizationContext::new(
                    PrincipalId::new([8; 32]).unwrap_or_else(|error| panic!("principal: {error}")),
                    capabilities,
                ),
                receipts: &NoReceipts,
                entrypoint: CALL_ENTRY_EXPORT,
                calldata: &[],
                composition: CompositionContext::catalog(catalog, CompositionRules::declared()),
                response_capacity: 0,
            },
        )
        .unwrap_or_else(|error| panic!("nested failure outcome: {error}"));

    let failure = record.failure().unwrap_or_else(|| panic!("failure"));
    assert_eq!(failure.program(), leaf_id);
    assert_eq!(failure.class(), RefusalClass::Conflict);
    assert_eq!(failure.reason().bytes(), reason);
    assert_eq!(record.execution().usage().output_bytes, reason.len() as u64);
    assert!(record.effects().is_none());
}

#[test]
fn declared_maximum_depth_preserves_the_actual_leaf_failure() {
    let engine = WasmEngine::declared().unwrap_or_else(|error| panic!("engine: {error}"));
    let ids: Vec<_> = (1u8..=9)
        .map(|byte| ProgramId::new([byte; 32]).unwrap_or_else(|error| panic!("id: {error}")))
        .collect();
    let reason = [0xde, 0, 0xad, 0xff];
    let mut catalog = ProgramCatalog::new();
    let leaf = engine
        .validate_candidate_v2(&refusing_program(&reason, RefusalClass::NotFound))
        .unwrap_or_else(|error| panic!("leaf validation: {error}"));
    catalog.insert(ids[8], leaf);
    for index in (1..8).rev() {
        let delegated = CapabilitySet::new(
            ids[index + 1..]
                .iter()
                .copied()
                .map(|program| Capability::Call { program }),
        )
        .unwrap_or_else(|error| panic!("delegated capabilities: {error}"));
        let forwarder = response_forwarder_with_capabilities(
            ids[index + 1],
            reason.len() as i32,
            &delegated.canonical_encoding(),
        );
        catalog.insert(
            ids[index],
            engine
                .validate_candidate_v2(&forwarder)
                .unwrap_or_else(|error| panic!("forwarder validation: {error}")),
        );
    }
    let root_delegation = CapabilitySet::new(
        ids[1..]
            .iter()
            .copied()
            .map(|program| Capability::Call { program }),
    )
    .unwrap_or_else(|error| panic!("root delegation: {error}"));
    let root = engine
        .validate_candidate_v2(&response_forwarder_with_capabilities(
            ids[1],
            reason.len() as i32,
            &root_delegation.canonical_encoding(),
        ))
        .unwrap_or_else(|error| panic!("root validation: {error}"));
    let record = Executor::declared()
        .execute_authorized_candidate(
            &mut Storage::new(),
            AuthorizedExecutionRequest {
                module: &root,
                program: ids[0],
                authorization: AuthorizationContext::new(
                    PrincipalId::new([42; 32]).unwrap_or_else(|error| panic!("principal: {error}")),
                    root_delegation,
                ),
                receipts: &NoReceipts,
                entrypoint: CALL_ENTRY_EXPORT,
                calldata: &[],
                composition: CompositionContext::catalog(catalog, CompositionRules::declared()),
                response_capacity: 0,
            },
        )
        .unwrap_or_else(|error| panic!("maximum-depth refusal: {error}"));
    let failure = record.failure().unwrap_or_else(|| panic!("failure"));
    assert_eq!(failure.program(), ids[8]);
    assert_eq!(failure.class(), RefusalClass::NotFound);
    assert_eq!(failure.reason().bytes(), reason);
    assert_eq!(record.execution().usage().output_bytes, reason.len() as u64);
    assert!(record.effects().is_none());
    assert_eq!(record.call_graph().edges().len(), 8);
    for (index, edge) in record.call_graph().edges().iter().enumerate() {
        assert_eq!(edge.caller(), ids[index]);
        assert_eq!(edge.callee(), ids[index + 1]);
        assert_eq!(edge.depth(), (index + 1) as u32);
    }
}

#[test]
fn one_edge_past_declared_depth_is_a_typed_depth_refusal() {
    let engine = WasmEngine::declared().unwrap_or_else(|error| panic!("engine: {error}"));
    let ids: Vec<_> = (1u8..=10)
        .map(|byte| ProgramId::new([byte; 32]).unwrap_or_else(|error| panic!("id: {error}")))
        .collect();
    let mut catalog = ProgramCatalog::new();
    catalog.insert(
        ids[9],
        engine
            .validate_candidate_v2(&refusing_program(&[1], RefusalClass::Rejected))
            .unwrap_or_else(|error| panic!("leaf validation: {error}")),
    );
    for index in (1..9).rev() {
        let delegated = CapabilitySet::new(
            ids[index + 1..]
                .iter()
                .copied()
                .map(|program| Capability::Call { program }),
        )
        .unwrap_or_else(|error| panic!("delegated capabilities: {error}"));
        catalog.insert(
            ids[index],
            engine
                .validate_candidate_v2(&response_forwarder_with_capabilities(
                    ids[index + 1],
                    1,
                    &delegated.canonical_encoding(),
                ))
                .unwrap_or_else(|error| panic!("forwarder validation: {error}")),
        );
    }
    let root_delegation = CapabilitySet::new(
        ids[1..]
            .iter()
            .copied()
            .map(|program| Capability::Call { program }),
    )
    .unwrap_or_else(|error| panic!("root delegation: {error}"));
    let root = engine
        .validate_candidate_v2(&response_forwarder_with_capabilities(
            ids[1],
            1,
            &root_delegation.canonical_encoding(),
        ))
        .unwrap_or_else(|error| panic!("root validation: {error}"));
    let result = Executor::declared().execute_authorized_candidate(
        &mut Storage::new(),
        AuthorizedExecutionRequest {
            module: &root,
            program: ids[0],
            authorization: AuthorizationContext::new(
                PrincipalId::new([43; 32]).unwrap_or_else(|error| panic!("principal: {error}")),
                root_delegation,
            ),
            receipts: &NoReceipts,
            entrypoint: CALL_ENTRY_EXPORT,
            calldata: &[],
            composition: CompositionContext::catalog(catalog, CompositionRules::declared()),
            response_capacity: 0,
        },
    );
    assert!(matches!(
        result,
        Err(ExecutionError::Composition(
            layerx_programs_runtime::CompositionRefusal::DepthExceeded {
                limit: 8,
                attempted: 9,
            }
        ))
    ));
}

#[test]
fn published_refusal_mismatches_nonnegative_return_and_cross_publication_is_refused() {
    let engine = WasmEngine::declared().unwrap_or_else(|error| panic!("engine: {error}"));
    let program = ProgramId::new([31; 32]).unwrap_or_else(|error| panic!("program: {error}"));
    let run = |wasm: Vec<u8>| {
        let module = engine
            .validate_candidate_v2(&wasm)
            .unwrap_or_else(|error| panic!("validation: {error}"));
        Executor::declared().execute_authorized_candidate(
            &mut Storage::new(),
            AuthorizedExecutionRequest {
                module: &module,
                program,
                authorization: AuthorizationContext::new(
                    PrincipalId::new([32; 32]).unwrap_or_else(|error| panic!("principal: {error}")),
                    CapabilitySet::empty(),
                ),
                receipts: &NoReceipts,
                entrypoint: CALL_ENTRY_EXPORT,
                calldata: &[],
                composition: CompositionContext::isolated(),
                response_capacity: 1,
            },
        )
    };
    assert_eq!(
        run(cross_publication_program(false, 0)),
        Err(ExecutionError::Response(ResponseRefusal::CodeMismatch {
            published: CANDIDATE_REFUSAL_SENTINEL,
            returned: 0,
        }))
    );
    assert_eq!(
        run(refusal_writer_program(
            &[1, 2, 3],
            RefusalClass::Rejected,
            &[(0, 3)],
            -65,
            false,
        )),
        Err(ExecutionError::Response(ResponseRefusal::CodeMismatch {
            published: CANDIDATE_REFUSAL_SENTINEL,
            returned: -65,
        }))
    );
    assert_eq!(
        run(refusal_writer_program(
            &[],
            RefusalClass::Rejected,
            &[],
            CANDIDATE_REFUSAL_SENTINEL,
            false,
        )),
        Err(ExecutionError::Response(
            ResponseRefusal::InvalidPublication
        ))
    );
    let legacy = run(refusal_writer_program(
        &[],
        RefusalClass::Rejected,
        &[],
        -65,
        false,
    ))
    .unwrap_or_else(|error| panic!("uninstrumented legacy refusal: {error}"));
    let legacy_failure = legacy.failure().unwrap_or_else(|| panic!("legacy failure"));
    assert_eq!(legacy_failure.program(), program);
    assert_eq!(legacy_failure.class(), RefusalClass::Legacy);
    assert!(legacy_failure.reason().bytes().is_empty());
    assert!(matches!(
        run(cross_publication_program(true, 0)),
        Err(layerx_programs_runtime::ExecutionError::Composition(
            layerx_programs_runtime::CompositionRefusal::Response(
                layerx_programs_runtime::ResponseRefusal::DuplicatePublication
            )
        ))
    ));
    let failure_first = run(cross_publication_program(false, CANDIDATE_REFUSAL_SENTINEL))
        .unwrap_or_else(|error| panic!("first failure publication wins: {error}"));
    let failure = failure_first.failure().unwrap_or_else(|| panic!("failure"));
    assert_eq!(failure.class(), RefusalClass::Rejected);
    assert_eq!(failure.reason().bytes(), [0, 0xff]);
}

#[test]
#[allow(clippy::too_many_lines)]
fn refusal_host_boundary_is_exact_bounded_and_first_publication_wins() {
    let empty = run_root(
        &refusing_program(&[], RefusalClass::Rejected),
        ResourceBudget::declared().with_output_bytes(0),
        FeeSchedule::declared(),
    )
    .unwrap_or_else(|error| panic!("empty refusal: {error}"));
    assert!(empty
        .failure()
        .unwrap_or_else(|| panic!("empty failure"))
        .reason()
        .bytes()
        .is_empty());
    assert_eq!(empty.execution().usage().output_bytes, 0);

    let maximum = vec![0xa5; MAX_REFUSAL_REASON_BYTES];
    let exact = run_root(
        &refusing_program(&maximum, RefusalClass::Unauthorized),
        ResourceBudget::declared().with_output_bytes(MAX_REFUSAL_REASON_BYTES as u64),
        FeeSchedule::declared(),
    )
    .unwrap_or_else(|error| panic!("maximum refusal: {error}"));
    assert_eq!(
        exact
            .failure()
            .unwrap_or_else(|| panic!("maximum failure"))
            .reason()
            .bytes(),
        maximum
    );
    assert_eq!(
        exact.execution().usage().output_bytes,
        MAX_REFUSAL_REASON_BYTES as u64
    );

    let over = vec![0x5a; MAX_REFUSAL_REASON_BYTES + 1];
    assert!(matches!(
        run_root(
            &refusing_program(&over, RefusalClass::Rejected),
            ResourceBudget::declared().with_output_bytes(0),
            FeeSchedule::declared(),
        ),
        Err(ExecutionError::Composition(
            layerx_programs_runtime::CompositionRefusal::Response(ResponseRefusal::TooLarge {
                bytes,
                limit: MAX_REFUSAL_REASON_BYTES,
            })
        )) if bytes == MAX_REFUSAL_REASON_BYTES + 1
    ));
    for malformed in [(-1, 1), (0, -1)] {
        assert!(matches!(
            run_root(
                &refusal_writer_program(
                    &[7],
                    RefusalClass::Rejected,
                    &[malformed],
                    CANDIDATE_REFUSAL_SENTINEL,
                    false,
                ),
                ResourceBudget::declared().with_output_bytes(0),
                FeeSchedule::declared(),
            ),
            Err(ExecutionError::Composition(
                layerx_programs_runtime::CompositionRefusal::Response(
                    ResponseRefusal::InvalidPublication
                )
            ))
        ));
    }
    for invalid_class in [
        RefusalClass::RuntimeFault.code() as i32,
        RefusalClass::Legacy.code() as i32,
        999,
    ] {
        assert!(matches!(
            run_root(
                &raw_refusal_writer_program(
                    &[7],
                    invalid_class,
                    &[(0, 1)],
                    CANDIDATE_REFUSAL_SENTINEL,
                    false,
                    true,
                ),
                ResourceBudget::declared().with_output_bytes(0),
                FeeSchedule::declared(),
            ),
            Err(ExecutionError::Composition(
                layerx_programs_runtime::CompositionRefusal::Response(
                    ResponseRefusal::InvalidPublication
                )
            ))
        ));
    }
    for (pointer, memory_exported) in [(70_000, true), (0, false)] {
        assert!(matches!(
            run_root(
                &raw_refusal_writer_program(
                    &[7],
                    RefusalClass::Rejected.code() as i32,
                    &[(pointer, 1)],
                    CANDIDATE_REFUSAL_SENTINEL,
                    false,
                    memory_exported,
                ),
                ResourceBudget::declared().with_output_bytes(0),
                FeeSchedule::declared(),
            ),
            Err(ExecutionError::Composition(
                layerx_programs_runtime::CompositionRefusal::Response(
                    ResponseRefusal::InvalidPublication
                )
            ))
        ));
    }

    for (writes, trap) in [
        (vec![(0, 3), (0, 3)], false),
        (vec![(0, 3), (-1, 1)], false),
        (vec![(0, 3)], true),
    ] {
        let record = run_root(
            &refusal_writer_program(
                &[0, 0xff, 9],
                RefusalClass::Conflict,
                &writes,
                CANDIDATE_REFUSAL_SENTINEL,
                trap,
            ),
            ResourceBudget::declared().with_output_bytes(3),
            FeeSchedule::declared(),
        )
        .unwrap_or_else(|error| panic!("first failure must win: {error}"));
        let failure = record.failure().unwrap_or_else(|| panic!("failure"));
        assert_eq!(failure.class(), RefusalClass::Conflict);
        assert_eq!(failure.reason().bytes(), [0, 0xff, 9]);
        assert_eq!(record.execution().usage().output_bytes, 3);
    }
}

#[test]
fn refusal_meter_boundary_and_fee_evidence_are_exact() {
    let reason = [1, 2, 3, 4, 5];
    let wasm = refusing_program(&reason, RefusalClass::NotFound);
    let low_price = FeeSchedule::declared().with_output_byte_price(2);
    let high_price = FeeSchedule::declared().with_output_byte_price(9);
    let first = run_root(
        &wasm,
        ResourceBudget::declared().with_output_bytes(reason.len() as u64),
        low_price,
    )
    .unwrap_or_else(|error| panic!("exact budget: {error}"));
    let one_byte_shorter = run_root(
        &refusing_program(&reason[..reason.len() - 1], RefusalClass::NotFound),
        ResourceBudget::declared().with_output_bytes(reason.len() as u64 - 1),
        low_price,
    )
    .unwrap_or_else(|error| panic!("one byte shorter: {error}"));
    let second = run_root(
        &wasm,
        ResourceBudget::declared().with_output_bytes(reason.len() as u64),
        high_price,
    )
    .unwrap_or_else(|error| panic!("higher price: {error}"));
    assert_eq!(first.execution().usage().output_bytes, reason.len() as u64);
    assert_eq!(
        first.execution().usage().output_bytes - one_byte_shorter.execution().usage().output_bytes,
        1
    );
    assert_eq!(
        first.execution().usage().fee_units - one_byte_shorter.execution().usage().fee_units,
        2
    );
    assert_eq!(second.execution().usage().output_bytes, reason.len() as u64);
    assert_eq!(
        second.execution().usage().fee_units - first.execution().usage().fee_units,
        (reason.len() as u128) * 7
    );
    assert_ne!(
        first.receipt_projection().canonical_encode(),
        second.receipt_projection().canonical_encode()
    );
    let exhausted = run_root(
        &wasm,
        ResourceBudget::declared().with_output_bytes(reason.len() as u64 - 1),
        low_price,
    );
    assert!(
        matches!(
            exhausted,
            Err(ExecutionError::Resource(
                layerx_programs_runtime::MeterRefusal::BudgetExceeded {
                    resource: layerx_programs_runtime::ResourceKind::OutputBytes,
                    limit: 4,
                    attempted: 5,
                }
            ))
        ),
        "{exhausted:?}"
    );
}

#[test]
fn published_refusal_survives_same_frame_cpu_exhaustion_at_root_and_nested_leaf() {
    let reason = [0, 0xff, 0x55];
    let root_budget =
        ResourceBudget::new(500, 65_536, 1_024, 1_024, 1, 1).with_output_bytes(reason.len() as u64);
    let root = run_root(
        &refusal_then_exhaust_program(&reason, RefusalClass::Conflict),
        root_budget,
        FeeSchedule::declared(),
    )
    .unwrap_or_else(|error| panic!("published root refusal must survive exhaustion: {error}"));
    let root_failure = root.failure().unwrap_or_else(|| panic!("root failure"));
    assert_eq!(root_failure.program(), root.root_program());
    assert_eq!(root_failure.class(), RefusalClass::Conflict);
    assert_eq!(root_failure.reason().bytes(), reason);
    assert_eq!(root.execution().usage().cpu_fuel, root_budget.cpu_fuel());
    assert_eq!(root.execution().usage().output_bytes, reason.len() as u64);
    assert!(root.execution().usage().fee_units > 0);
    let root_receipt = root.receipt_projection();
    let root_bytes = root_receipt.canonical_encode();
    assert_eq!(
        layerx_programs_runtime::CandidateActivityReceipt::canonical_decode(&root_bytes),
        Ok(root_receipt)
    );

    let engine = WasmEngine::declared().unwrap_or_else(|error| panic!("engine: {error}"));
    let root_id = ProgramId::new([131; 32]).unwrap_or_else(|error| panic!("root: {error}"));
    let leaf_id = ProgramId::new([132; 32]).unwrap_or_else(|error| panic!("leaf: {error}"));
    let leaf = engine
        .validate_candidate_v2(&refusal_then_exhaust_program(
            &reason,
            RefusalClass::Conflict,
        ))
        .unwrap_or_else(|error| panic!("leaf validation: {error}"));
    let root_module = engine
        .validate_candidate_v2(&response_forwarder(leaf_id, reason.len() as i32))
        .unwrap_or_else(|error| panic!("root validation: {error}"));
    let mut catalog = ProgramCatalog::new();
    catalog.insert(leaf_id, leaf);
    let nested_budget = ResourceBudget::new(5_000, 65_536, 1_024, 1_024, 2, 1)
        .with_output_bytes(reason.len() as u64);
    let nested = Executor::new(nested_budget, FeeSchedule::declared())
        .execute_authorized_candidate(
            &mut Storage::new(),
            AuthorizedExecutionRequest {
                module: &root_module,
                program: root_id,
                authorization: AuthorizationContext::new(
                    PrincipalId::new([133; 32])
                        .unwrap_or_else(|error| panic!("principal: {error}")),
                    CapabilitySet::new([Capability::Call { program: leaf_id }])
                        .unwrap_or_else(|error| panic!("capability: {error}")),
                ),
                receipts: &NoReceipts,
                entrypoint: CALL_ENTRY_EXPORT,
                calldata: &[],
                composition: CompositionContext::catalog(catalog, CompositionRules::declared()),
                response_capacity: 0,
            },
        )
        .unwrap_or_else(|error| panic!("published leaf refusal must survive exhaustion: {error}"));
    let nested_failure = nested.failure().unwrap_or_else(|| panic!("nested failure"));
    assert_eq!(nested_failure.program(), leaf_id);
    assert_eq!(nested_failure.class(), RefusalClass::Conflict);
    assert_eq!(nested_failure.reason().bytes(), reason);
    assert_eq!(
        nested.execution().usage().cpu_fuel,
        nested_budget.cpu_fuel()
    );
    assert_eq!(nested.execution().usage().output_bytes, reason.len() as u64);
    assert_eq!(nested.call_graph().edges().len(), 1);
    assert!(nested.effects().is_none());
}

#[test]
fn failure_evidence_is_deterministic_and_binds_leaf_class_reason_and_fee() {
    let base_wasm = refusing_program(&[1, 2, 3], RefusalClass::Rejected);
    let base = run_root(
        &base_wasm,
        ResourceBudget::declared(),
        FeeSchedule::declared(),
    )
    .unwrap_or_else(|error| panic!("base: {error}"));
    let repeated = run_root(
        &base_wasm,
        ResourceBudget::declared(),
        FeeSchedule::declared(),
    )
    .unwrap_or_else(|error| panic!("repeated: {error}"));
    assert_eq!(base.canonical_evidence(), repeated.canonical_evidence());
    assert_eq!(
        base.receipt_projection().canonical_encode(),
        repeated.receipt_projection().canonical_encode()
    );

    let changed_class = run_root(
        &refusing_program(&[1, 2, 3], RefusalClass::Conflict),
        ResourceBudget::declared(),
        FeeSchedule::declared(),
    )
    .unwrap_or_else(|error| panic!("changed class: {error}"));
    let changed_reason = run_root(
        &refusing_program(&[1, 2, 4], RefusalClass::Rejected),
        ResourceBudget::declared(),
        FeeSchedule::declared(),
    )
    .unwrap_or_else(|error| panic!("changed reason: {error}"));
    let changed_fee = run_root(
        &base_wasm,
        ResourceBudget::declared(),
        FeeSchedule::declared().with_output_byte_price(2),
    )
    .unwrap_or_else(|error| panic!("changed fee: {error}"));
    for changed in [&changed_class, &changed_reason, &changed_fee] {
        assert_ne!(base.canonical_evidence(), changed.canonical_evidence());
        assert_ne!(
            base.receipt_projection().canonical_encode(),
            changed.receipt_projection().canonical_encode()
        );
    }

    let nested = |leaf_byte: u8| {
        let engine = WasmEngine::declared().unwrap_or_else(|error| panic!("engine: {error}"));
        let leaf_id =
            ProgramId::new([leaf_byte; 32]).unwrap_or_else(|error| panic!("leaf: {error}"));
        let leaf = engine
            .validate_candidate_v2(&base_wasm)
            .unwrap_or_else(|error| panic!("leaf validation: {error}"));
        let root = engine
            .validate_candidate_v2(&response_forwarder(leaf_id, 3))
            .unwrap_or_else(|error| panic!("root validation: {error}"));
        let mut catalog = ProgramCatalog::new();
        catalog.insert(leaf_id, leaf);
        Executor::declared()
            .execute_authorized_candidate(
                &mut Storage::new(),
                AuthorizedExecutionRequest {
                    module: &root,
                    program: ProgramId::new([124; 32])
                        .unwrap_or_else(|error| panic!("root: {error}")),
                    authorization: AuthorizationContext::new(
                        PrincipalId::new([125; 32])
                            .unwrap_or_else(|error| panic!("principal: {error}")),
                        CapabilitySet::new([Capability::Call { program: leaf_id }])
                            .unwrap_or_else(|error| panic!("capability: {error}")),
                    ),
                    receipts: &NoReceipts,
                    entrypoint: CALL_ENTRY_EXPORT,
                    calldata: &[],
                    composition: CompositionContext::catalog(catalog, CompositionRules::declared()),
                    response_capacity: 0,
                },
            )
            .unwrap_or_else(|error| panic!("nested failure: {error}"))
    };
    let first_leaf = nested(126);
    let second_leaf = nested(127);
    assert_ne!(
        first_leaf
            .failure()
            .unwrap_or_else(|| panic!("first leaf"))
            .program(),
        second_leaf
            .failure()
            .unwrap_or_else(|| panic!("second leaf"))
            .program()
    );
    assert_ne!(
        first_leaf.canonical_evidence(),
        second_leaf.canonical_evidence()
    );
    assert_ne!(
        first_leaf.receipt_projection().canonical_encode(),
        second_leaf.receipt_projection().canonical_encode()
    );
}

#[test]
fn program_failure_discards_storage_and_effects_across_multiple_frames() {
    let engine = WasmEngine::declared().unwrap_or_else(|error| panic!("engine: {error}"));
    let root_id = ProgramId::new([101; 32]).unwrap_or_else(|error| panic!("root: {error}"));
    let middle_id = ProgramId::new([102; 32]).unwrap_or_else(|error| panic!("middle: {error}"));
    let leaf_id = ProgramId::new([103; 32]).unwrap_or_else(|error| panic!("leaf: {error}"));
    let leaf = engine
        .validate_candidate_v2(&staged_refusing_program(&[0, 0xff, 4]))
        .unwrap_or_else(|error| panic!("leaf validation: {error}"));
    let transfer = Capability::Transfer402 {
        asset: [9; 32],
        to: [10; 32],
        maximum_amount: 1,
    };
    let middle_grants = CapabilitySet::new([
        Capability::StorageWrite,
        Capability::EmitEvent,
        transfer.clone(),
        Capability::Call { program: leaf_id },
    ])
    .unwrap_or_else(|error| panic!("middle grants: {error}"));
    let middle = engine
        .validate_candidate_v2(&staged_forwarder(
            leaf_id,
            &CapabilitySet::new([
                Capability::StorageWrite,
                Capability::EmitEvent,
                transfer.clone(),
            ])
            .unwrap_or_else(|error| panic!("leaf grants: {error}"))
            .canonical_encoding(),
        ))
        .unwrap_or_else(|error| panic!("middle validation: {error}"));
    let root_grants = CapabilitySet::new([
        Capability::StorageWrite,
        Capability::EmitEvent,
        transfer,
        Capability::Call { program: middle_id },
        Capability::Call { program: leaf_id },
    ])
    .unwrap_or_else(|error| panic!("root grants: {error}"));
    let root = engine
        .validate_candidate_v2(&staged_forwarder(
            middle_id,
            &middle_grants.canonical_encoding(),
        ))
        .unwrap_or_else(|error| panic!("root validation: {error}"));
    let mut catalog = ProgramCatalog::new();
    catalog.insert(middle_id, middle);
    catalog.insert(leaf_id, leaf);
    let mut storage = Storage::new();
    let before = storage.clone();
    let record = Executor::declared()
        .execute_authorized_candidate(
            &mut storage,
            AuthorizedExecutionRequest {
                module: &root,
                program: root_id,
                authorization: AuthorizationContext::new(
                    PrincipalId::new([104; 32])
                        .unwrap_or_else(|error| panic!("principal: {error}")),
                    root_grants,
                ),
                receipts: &NoReceipts,
                entrypoint: CALL_ENTRY_EXPORT,
                calldata: &[],
                composition: CompositionContext::catalog(catalog, CompositionRules::declared()),
                response_capacity: 0,
            },
        )
        .unwrap_or_else(|error| panic!("failure outcome: {error}"));
    let failure = record.failure().unwrap_or_else(|| panic!("failure"));
    assert_eq!(failure.program(), leaf_id);
    assert_eq!(failure.reason().bytes(), [0, 0xff, 4]);
    assert_eq!(record.call_graph().edges().len(), 2);
    assert_eq!(storage, before);
    assert!(record.effects().is_none());
}

#[test]
fn runtime_faults_are_canonical_metered_and_atomic_at_root_and_leaf() {
    let transfer = Capability::Transfer402 {
        asset: [9; 32],
        to: [10; 32],
        maximum_amount: 1,
    };
    let staged = CapabilitySet::new([
        Capability::StorageWrite,
        Capability::EmitEvent,
        transfer.clone(),
    ])
    .unwrap_or_else(|error| panic!("staged capabilities: {error}"));
    let mut storage = Storage::new();
    let before = storage.clone();
    let root_fault = execute_root(
        &staged_runtime_fault_program(),
        ResourceBudget::declared(),
        FeeSchedule::declared(),
        staged.clone(),
        &mut storage,
    )
    .unwrap_or_else(|error| panic!("root runtime fault: {error}"));
    let root_failure = root_fault.failure().unwrap_or_else(|| panic!("failure"));
    assert_eq!(root_failure.program(), root_fault.root_program());
    assert_eq!(root_failure.class(), RefusalClass::RuntimeFault);
    assert!(root_failure.reason().bytes().is_empty());
    assert!(root_fault.execution().usage().cpu_fuel > 0);
    assert!(root_fault.execution().usage().storage_write_bytes > 0);
    assert_eq!(storage, before);
    assert!(root_fault.effects().is_none());

    let engine = WasmEngine::declared().unwrap_or_else(|error| panic!("engine: {error}"));
    let root_id = ProgramId::new([121; 32]).unwrap_or_else(|error| panic!("root: {error}"));
    let leaf_id = ProgramId::new([122; 32]).unwrap_or_else(|error| panic!("leaf: {error}"));
    let leaf = engine
        .validate_candidate_v2(&staged_runtime_fault_program())
        .unwrap_or_else(|error| panic!("leaf validation: {error}"));
    let root_grants = CapabilitySet::new([
        Capability::StorageWrite,
        Capability::EmitEvent,
        transfer,
        Capability::Call { program: leaf_id },
    ])
    .unwrap_or_else(|error| panic!("root grants: {error}"));
    let root = engine
        .validate_candidate_v2(&staged_forwarder(leaf_id, &staged.canonical_encoding()))
        .unwrap_or_else(|error| panic!("root validation: {error}"));
    let mut catalog = ProgramCatalog::new();
    catalog.insert(leaf_id, leaf);
    let leaf_fault = Executor::declared()
        .execute_authorized_candidate(
            &mut storage,
            AuthorizedExecutionRequest {
                module: &root,
                program: root_id,
                authorization: AuthorizationContext::new(
                    PrincipalId::new([123; 32])
                        .unwrap_or_else(|error| panic!("principal: {error}")),
                    root_grants,
                ),
                receipts: &NoReceipts,
                entrypoint: CALL_ENTRY_EXPORT,
                calldata: &[],
                composition: CompositionContext::catalog(catalog, CompositionRules::declared()),
                response_capacity: 0,
            },
        )
        .unwrap_or_else(|error| panic!("leaf runtime fault: {error}"));
    let leaf_failure = leaf_fault.failure().unwrap_or_else(|| panic!("failure"));
    assert_eq!(leaf_failure.program(), leaf_id);
    assert_eq!(leaf_failure.class(), RefusalClass::RuntimeFault);
    assert!(leaf_failure.reason().bytes().is_empty());
    assert!(leaf_fault.execution().usage().cpu_fuel > root_fault.execution().usage().cpu_fuel);
    assert!(leaf_fault.execution().usage().storage_write_bytes > 0);
    assert_eq!(leaf_fault.call_graph().edges().len(), 1);
    assert_eq!(storage, before);
    assert!(leaf_fault.effects().is_none());
    for record in [&root_fault, &leaf_fault] {
        let receipt = record.receipt_projection();
        let encoded = receipt.canonical_encode();
        assert_eq!(
            layerx_programs_runtime::CandidateActivityReceipt::canonical_decode(&encoded),
            Ok(receipt)
        );
        assert!(!encoded
            .windows(b"unreachable".len())
            .any(|window| window == b"unreachable"));
    }
}

#[test]
fn nested_published_refusal_requires_sentinel_and_wins_over_trap() {
    let child_id = ProgramId::new([111; 32]).unwrap_or_else(|error| panic!("child: {error}"));
    let run = |child_wasm: Vec<u8>| {
        let engine = WasmEngine::declared().unwrap_or_else(|error| panic!("engine: {error}"));
        let child = engine
            .validate_candidate_v2(&child_wasm)
            .unwrap_or_else(|error| panic!("child validation: {error}"));
        let root = engine
            .validate_candidate_v2(&response_forwarder(child_id, 3))
            .unwrap_or_else(|error| panic!("root validation: {error}"));
        let mut catalog = ProgramCatalog::new();
        catalog.insert(child_id, child);
        Executor::declared().execute_authorized_candidate(
            &mut Storage::new(),
            AuthorizedExecutionRequest {
                module: &root,
                program: ProgramId::new([110; 32]).unwrap_or_else(|error| panic!("root: {error}")),
                authorization: AuthorizationContext::new(
                    PrincipalId::new([112; 32])
                        .unwrap_or_else(|error| panic!("principal: {error}")),
                    CapabilitySet::new([Capability::Call { program: child_id }])
                        .unwrap_or_else(|error| panic!("capability: {error}")),
                ),
                receipts: &NoReceipts,
                entrypoint: CALL_ENTRY_EXPORT,
                calldata: &[],
                composition: CompositionContext::catalog(catalog, CompositionRules::declared()),
                response_capacity: 0,
            },
        )
    };
    assert_eq!(
        run(refusal_writer_program(
            &[1, 2, 3],
            RefusalClass::Rejected,
            &[(0, 3)],
            0,
            false,
        )),
        Err(ExecutionError::Composition(
            layerx_programs_runtime::CompositionRefusal::Response(ResponseRefusal::CodeMismatch {
                published: CANDIDATE_REFUSAL_SENTINEL,
                returned: 0,
            })
        ))
    );
    let legacy = run(refusal_writer_program(
        &[],
        RefusalClass::Rejected,
        &[],
        -65,
        false,
    ))
    .unwrap_or_else(|error| panic!("nested legacy refusal: {error}"));
    let legacy_failure = legacy.failure().unwrap_or_else(|| panic!("legacy failure"));
    assert_eq!(legacy_failure.program(), child_id);
    assert_eq!(legacy_failure.class(), RefusalClass::Legacy);
    assert!(legacy_failure.reason().bytes().is_empty());
    assert_eq!(
        run(refusal_writer_program(
            &[1, 2, 3],
            RefusalClass::Rejected,
            &[(0, 3)],
            -65,
            false,
        )),
        Err(ExecutionError::Composition(
            layerx_programs_runtime::CompositionRefusal::Response(ResponseRefusal::CodeMismatch {
                published: CANDIDATE_REFUSAL_SENTINEL,
                returned: -65,
            })
        ))
    );
    assert_eq!(
        run(refusal_writer_program(
            &[],
            RefusalClass::Rejected,
            &[],
            CANDIDATE_REFUSAL_SENTINEL,
            false,
        )),
        Err(ExecutionError::Composition(
            layerx_programs_runtime::CompositionRefusal::Response(
                ResponseRefusal::InvalidPublication
            )
        ))
    );
    let trapped = run(refusal_writer_program(
        &[1, 2, 3],
        RefusalClass::Rejected,
        &[(0, 3)],
        CANDIDATE_REFUSAL_SENTINEL,
        true,
    ))
    .unwrap_or_else(|error| panic!("published child refusal: {error}"));
    let failure = trapped.failure().unwrap_or_else(|| panic!("failure"));
    assert_eq!(failure.program(), child_id);
    assert_eq!(failure.class(), RefusalClass::Rejected);
    assert_eq!(failure.reason().bytes(), [1, 2, 3]);
}
