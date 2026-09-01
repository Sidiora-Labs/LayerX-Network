use layerx_programs_runtime::test_support::{
    code_section, func_body, function_section, import_section, module, type_section, unsigned_leb,
    TYPE_I32,
};
use layerx_programs_runtime::{
    AbiError, AccessSet, ActivityBudgetBinding, AuthorizationContext, AuthorizedExecutionRequest,
    BudgetAdmissionRefusal, BudgetDimension, BudgetMeterRefusal, BudgetResourceKind,
    BudgetedAuthorizedExecutionRequest, BudgetedV1ActivityOutcome, BudgetedV1FailureCause,
    CandidateActivityOutcome, CandidateReceiptOutcome, Capability, CapabilitySet,
    CompositionContext, CompositionRefusal, DeclaredBudget, EntrypointRefusal, ExecutionError,
    Executor, FeeSchedule, FuelSchedule, MeterInjection, MeterRefusal, PrincipalId, ProgramCatalog,
    ProgramId, ReceiptOracle, ReceiptView, RefusalClass, ResourceBudget, ResourceKind, Storage,
    StorageNamespace, WasmEngine, CALL_ENTRY_EXPORT, MIN_ACTIVITY_CPU_FUEL,
};
use wasm_instrument::parity_wasm::elements::{self, External, Instruction};

struct NoReceipts;

impl ReceiptOracle for NoReceipts {
    fn verified_receipt(&self, _: [u8; 32]) -> Result<ReceiptView, AbiError> {
        Err(AbiError::ReceiptMismatch)
    }
}

fn principal(byte: u8) -> PrincipalId {
    match PrincipalId::new([byte; 32]) {
        Ok(principal) => principal,
        Err(error) => panic!("principal: {error}"),
    }
}

fn declared(
    cpu_fuel: u64,
    memory_bytes: u64,
    storage_read_bytes: u64,
    storage_write_bytes: u64,
    output_values: u32,
    output_bytes: u64,
    table_elements: u32,
) -> DeclaredBudget {
    match DeclaredBudget::new(
        cpu_fuel,
        memory_bytes,
        storage_read_bytes,
        storage_write_bytes,
        output_values,
        output_bytes,
        table_elements,
    ) {
        Ok(declared) => declared,
        Err(error) => panic!("declared budget: {error}"),
    }
}

fn section(id: u8, payload: &[u8]) -> Vec<u8> {
    let mut bytes = vec![id];
    bytes.extend(unsigned_leb(payload.len() as u64));
    bytes.extend_from_slice(payload);
    bytes
}

fn data_section(entries: &[(u32, &[u8])]) -> Vec<u8> {
    let mut payload = unsigned_leb(entries.len() as u64);
    for (offset, bytes) in entries {
        payload.push(0);
        payload.push(0x41);
        payload.extend(unsigned_leb(u64::from(*offset)));
        payload.push(0x0b);
        payload.extend(unsigned_leb(bytes.len() as u64));
        payload.extend_from_slice(bytes);
    }
    section(11, &payload)
}

fn poison_start_candidate() -> Vec<u8> {
    module(&[
        type_section(&[(&[], &[])]),
        function_section(&[0]),
        section(8, &[0]),
        code_section(&[func_body(&[], &[0x00, 0x0b])]),
    ])
}

fn poison_start_missing_allocator_candidate() -> Vec<u8> {
    let types = type_section(&[(&[], &[]), (&[TYPE_I32, TYPE_I32], &[TYPE_I32])]);
    let functions = function_section(&[0, 1]);
    let memory = section(5, &[1, 1, 1, 1]);
    let mut exports = unsigned_leb(2);
    for (name, kind, index) in [("layerx_call", 0_u8, 1_u8), ("memory", 2, 0)] {
        exports.extend(unsigned_leb(name.len() as u64));
        exports.extend_from_slice(name.as_bytes());
        exports.extend_from_slice(&[kind, index]);
    }
    module(&[
        types,
        functions,
        memory,
        section(7, &exports),
        section(8, &[0]),
        code_section(&[
            func_body(&[], &[0x00, 0x0b]),
            func_body(&[], &[0x41, 0, 0x0b]),
        ]),
    ])
}

fn negative_allocator_candidate() -> Vec<u8> {
    let types = type_section(&[
        (&[TYPE_I32], &[TYPE_I32]),
        (&[TYPE_I32, TYPE_I32], &[TYPE_I32]),
    ]);
    let functions = function_section(&[0, 1]);
    let memory = section(5, &[1, 1, 1, 1]);
    let mut exports = unsigned_leb(3);
    for (name, kind, index) in [
        ("layerx_reserve", 0_u8, 0_u8),
        ("layerx_call", 0, 1),
        ("memory", 2, 0),
    ] {
        exports.extend(unsigned_leb(name.len() as u64));
        exports.extend_from_slice(name.as_bytes());
        exports.extend_from_slice(&[kind, index]);
    }
    module(&[
        types,
        functions,
        memory,
        section(7, &exports),
        code_section(&[
            func_body(&[], &[0x41, 0x7f, 0x0b]),
            func_body(&[], &[0x41, 0, 0x0b]),
        ]),
    ])
}

fn looping_candidate() -> Vec<u8> {
    candidate_with_entry(&[0x03, 0x40, 0x0c, 0, 0x0b, 0x41, 0, 0x0b])
}

fn candidate_with_entry(entry: &[u8]) -> Vec<u8> {
    let types = type_section(&[
        (&[TYPE_I32], &[TYPE_I32]),
        (&[TYPE_I32, TYPE_I32], &[TYPE_I32]),
    ]);
    let functions = function_section(&[0, 1]);
    let memory = section(5, &[1, 1, 1, 1]);
    let mut exports = unsigned_leb(3);
    for (name, kind, index) in [
        ("layerx_reserve", 0_u8, 0_u8),
        ("layerx_call", 0, 1),
        ("memory", 2, 0),
    ] {
        exports.extend(unsigned_leb(name.len() as u64));
        exports.extend_from_slice(name.as_bytes());
        exports.extend_from_slice(&[kind, index]);
    }
    module(&[
        types,
        functions,
        memory,
        section(7, &exports),
        code_section(&[func_body(&[], &[0x41, 0, 0x0b]), func_body(&[], entry)]),
    ])
}

fn frozen_defined_function_charge_sites(wasm: &[u8], defined_index: usize) -> Vec<u64> {
    let injection = MeterInjection::instrument(wasm, FuelSchedule::WASMI_0_31_2)
        .unwrap_or_else(|error| panic!("meter injection: {error}"));
    let module = elements::deserialize_buffer::<elements::Module>(injection.instrumented_wasm())
        .unwrap_or_else(|error| panic!("instrumented module decode: {error}"));
    let mut function_index = 0_u32;
    let charge_function = module
        .import_section()
        .unwrap_or_else(|| panic!("instrumented import section absent"))
        .entries()
        .iter()
        .find_map(|entry| match entry.external() {
            External::Function(_) => {
                let current = function_index;
                function_index += 1;
                (entry.module() == "layerx_private_metering/v1"
                    && entry.field() == "charge_i64")
                    .then_some(current)
            }
            _ => None,
        })
        .unwrap_or_else(|| panic!("private charge import absent"));
    let body = &module
        .code_section()
        .unwrap_or_else(|| panic!("instrumented code section absent"))
        .bodies()[defined_index];
    let instructions = body.code().elements();
    instructions
        .windows(2)
        .filter_map(|window| match window {
            [Instruction::I64Const(charge), Instruction::Call(target)]
                if *target == charge_function => {
                u64::try_from(*charge).ok()
            }
            _ => None,
        })
        .collect()
}

fn access_charge(callees: impl IntoIterator<Item = ProgramId>) -> u64 {
    AccessSet::new_with_callees([], [], callees)
        .and_then(|set| set.charge())
        .unwrap_or_else(|error| panic!("access charge: {error}"))
        .total_units()
}

fn storage_write_call_access_charge(
    root: ProgramId,
    payer: PrincipalId,
    child: ProgramId,
) -> u64 {
    let mut builder = AccessSet::builder();
    builder
        .write_namespace(StorageNamespace::principal(root, payer))
        .and_then(|builder| {
            builder.write_namespace(StorageNamespace::principal(child, payer))
        })
        .and_then(|builder| builder.call(child))
        .unwrap_or_else(|error| panic!("storage/call access set: {error}"));
    builder
        .build()
        .and_then(|set| set.charge())
        .unwrap_or_else(|error| panic!("storage/call access charge: {error}"))
        .total_units()
}

fn repeated_charge_exhaustion(limit: u64, prefix: u64, repeated: u64) -> (u64, u64) {
    assert!(repeated > 0);
    assert!(prefix <= limit);
    let admitted_repetitions = (limit - prefix) / repeated;
    let usage = prefix + admitted_repetitions * repeated;
    (usage, usage + repeated)
}

fn v1_forwarder(callee: ProgramId) -> Vec<u8> {
    v1_forwarder_configured(callee, &[0, 0], false)
}

fn v1_forwarder_configured(
    callee: ProgramId,
    encoded_capabilities: &[u8],
    with_table: bool,
) -> Vec<u8> {
    let types = type_section(&[
        (&[TYPE_I32; 6], &[TYPE_I32]),
        (&[TYPE_I32], &[TYPE_I32]),
        (&[TYPE_I32, TYPE_I32], &[TYPE_I32]),
    ]);
    let imports = import_section(&[("layerx_v1", "program_call", 0)]);
    let functions = function_section(&[1, 2]);
    let memory = section(5, &[1, 1, 1, 1]);
    let mut exports = unsigned_leb(3);
    for (name, kind, index) in [
        ("layerx_reserve", 0_u8, 1_u8),
        ("layerx_call", 0, 2),
        ("memory", 2, 0),
    ] {
        exports.extend(unsigned_leb(name.len() as u64));
        exports.extend_from_slice(name.as_bytes());
        exports.extend_from_slice(&[kind, index]);
    }
    let entry = [
        0x41,
        0,
        0x41,
        32,
        0x41,
        32,
        0x41,
        0,
        0x41,
        32,
        0x41,
        u8::try_from(encoded_capabilities.len()).unwrap_or(u8::MAX),
        0x10,
        0,
        0x0b,
    ];
    let mut sections = vec![types, imports, functions];
    if with_table {
        sections.push(section(4, &[1, 0x70, 0, 1]));
    }
    sections.extend([
        memory,
        section(7, &exports),
        code_section(&[func_body(&[], &[0x41, 0, 0x0b]), func_body(&[], &entry)]),
        data_section(&[(0, &callee.bytes()[..]), (32, encoded_capabilities)]),
    ]);
    module(&sections)
}

fn start_forwarder(callee: ProgramId) -> Vec<u8> {
    start_forwarder_configured(callee, &[0, 0])
}

fn start_forwarder_configured(callee: ProgramId, encoded_capabilities: &[u8]) -> Vec<u8> {
    let types = type_section(&[
        (&[TYPE_I32; 6], &[TYPE_I32]),
        (&[], &[]),
        (&[TYPE_I32], &[TYPE_I32]),
        (&[TYPE_I32, TYPE_I32], &[TYPE_I32]),
    ]);
    let imports = import_section(&[("layerx_v1", "program_call", 0)]);
    let functions = function_section(&[1, 2, 3]);
    let memory = section(5, &[1, 1, 1, 1]);
    let mut exports = unsigned_leb(3);
    for (name, kind, index) in [
        ("layerx_reserve", 0_u8, 2_u8),
        ("layerx_call", 0, 3),
        ("memory", 2, 0),
    ] {
        exports.extend(unsigned_leb(name.len() as u64));
        exports.extend_from_slice(name.as_bytes());
        exports.extend_from_slice(&[kind, index]);
    }
    let start = [
        0x41,
        0,
        0x41,
        32,
        0x41,
        32,
        0x41,
        0,
        0x41,
        32,
        0x41,
        u8::try_from(encoded_capabilities.len()).unwrap_or(u8::MAX),
        0x10,
        0,
        0x1a,
        0x0b,
    ];
    module(&[
        types,
        imports,
        functions,
        memory,
        section(7, &exports),
        section(8, &[1]),
        code_section(&[
            func_body(&[], &start),
            func_body(&[], &[0x41, 0, 0x0b]),
            func_body(&[], &[0x41, 0, 0x0b]),
        ]),
        data_section(&[(0, &callee.bytes()[..]), (32, encoded_capabilities)]),
    ])
}

fn response_forwarder(callee: ProgramId, capacity: u8) -> Vec<u8> {
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
        ("layerx_v2", "response_write", 0),
        ("layerx_v2", "program_call_response", 1),
    ]);
    let functions = function_section(&[2, 3]);
    let memory = section(5, &[1, 1, 1, 1]);
    let mut exports = unsigned_leb(3);
    for (name, kind, index) in [
        ("layerx_reserve", 0_u8, 2_u8),
        ("layerx_call", 0, 3),
        ("memory", 2, 0),
    ] {
        exports.extend(unsigned_leb(name.len() as u64));
        exports.extend_from_slice(name.as_bytes());
        exports.extend_from_slice(&[kind, index]);
    }
    let mut entry = Vec::new();
    for value in [0_u8, 32, 32, 0, 32, 2, 48, capacity] {
        entry.extend_from_slice(&[0x41, value]);
    }
    entry.extend_from_slice(&[
        0x10, 1, 0x1a, 0x41, 0, 0x41, 48, 0x41, capacity, 0x10, 0, 0x1a, 0x41, 0, 0x0b,
    ]);
    module(&[
        types,
        imports,
        functions,
        memory,
        section(7, &exports),
        code_section(&[func_body(&[], &[0x41, 0, 0x0b]), func_body(&[], &entry)]),
        data_section(&[(0, &callee.bytes()[..]), (32, &[0, 0])]),
    ])
}

fn v1_fanout(callee: ProgramId, encoded_capabilities: &[u8]) -> Vec<u8> {
    v1_fanout_configured(callee, encoded_capabilities, false)
}

fn v1_fanout_configured(
    callee: ProgramId,
    encoded_capabilities: &[u8],
    with_table: bool,
) -> Vec<u8> {
    let types = type_section(&[
        (&[TYPE_I32; 6], &[TYPE_I32]),
        (&[TYPE_I32], &[TYPE_I32]),
        (&[TYPE_I32, TYPE_I32], &[TYPE_I32]),
    ]);
    let imports = import_section(&[("layerx_v1", "program_call", 0)]);
    let functions = function_section(&[1, 2]);
    let memory = section(5, &[1, 1, 1, 1]);
    let mut exports = unsigned_leb(3);
    for (name, kind, index) in [
        ("layerx_reserve", 0_u8, 1_u8),
        ("layerx_call", 0, 2),
        ("memory", 2, 0),
    ] {
        exports.extend(unsigned_leb(name.len() as u64));
        exports.extend_from_slice(name.as_bytes());
        exports.extend_from_slice(&[kind, index]);
    }
    let call = [
        0x41,
        0,
        0x41,
        32,
        0x41,
        32,
        0x41,
        0,
        0x41,
        32,
        0x41,
        u8::try_from(encoded_capabilities.len()).unwrap_or(u8::MAX),
        0x10,
        0,
    ];
    let mut entry = call.to_vec();
    entry.push(0x1a);
    entry.extend_from_slice(&call);
    entry.push(0x0b);
    let mut sections = vec![types, imports, functions];
    if with_table {
        sections.push(section(4, &[1, 0x70, 0, 1]));
    }
    sections.extend([
        memory,
        section(7, &exports),
        code_section(&[func_body(&[], &[0x41, 0, 0x0b]), func_body(&[], &entry)]),
        data_section(&[(0, &callee.bytes()[..]), (32, encoded_capabilities)]),
    ]);
    module(&sections)
}

fn storage_writer_candidate() -> Vec<u8> {
    let types = type_section(&[
        (&[TYPE_I32, TYPE_I32, TYPE_I32, TYPE_I32], &[TYPE_I32]),
        (&[TYPE_I32], &[TYPE_I32]),
        (&[TYPE_I32, TYPE_I32], &[TYPE_I32]),
    ]);
    let imports = import_section(&[("layerx_v1", "storage_write", 0)]);
    let functions = function_section(&[1, 2]);
    let memory = section(5, &[1, 1, 1, 1]);
    let mut exports = unsigned_leb(3);
    for (name, kind, index) in [
        ("layerx_reserve", 0_u8, 1_u8),
        ("layerx_call", 0, 2),
        ("memory", 2, 0),
    ] {
        exports.extend(unsigned_leb(name.len() as u64));
        exports.extend_from_slice(name.as_bytes());
        exports.extend_from_slice(&[kind, index]);
    }
    module(&[
        types,
        imports,
        functions,
        memory,
        section(7, &exports),
        code_section(&[
            func_body(&[], &[0x41, 0, 0x0b]),
            func_body(&[], &[0x41, 0, 0x41, 3, 0x41, 3, 0x41, 4, 0x10, 0, 0x0b]),
        ]),
        data_section(&[(0, b"keydata")]),
    ])
}

fn storage_writer_ignores_status_candidate() -> Vec<u8> {
    storage_writer_ignores_status_with_return(0)
}

fn storage_writer_ignores_status_with_return(returned: u8) -> Vec<u8> {
    let types = type_section(&[
        (&[TYPE_I32, TYPE_I32, TYPE_I32, TYPE_I32], &[TYPE_I32]),
        (&[TYPE_I32], &[TYPE_I32]),
        (&[TYPE_I32, TYPE_I32], &[TYPE_I32]),
    ]);
    let imports = import_section(&[("layerx_v1", "storage_write", 0)]);
    let functions = function_section(&[1, 2]);
    let memory = section(5, &[1, 1, 1, 1]);
    let mut exports = unsigned_leb(3);
    for (name, kind, index) in [
        ("layerx_reserve", 0_u8, 1_u8),
        ("layerx_call", 0, 2),
        ("memory", 2, 0),
    ] {
        exports.extend(unsigned_leb(name.len() as u64));
        exports.extend_from_slice(name.as_bytes());
        exports.extend_from_slice(&[kind, index]);
    }
    module(&[
        types,
        imports,
        functions,
        memory,
        section(7, &exports),
        code_section(&[
            func_body(&[], &[0x41, 0, 0x0b]),
            func_body(
                &[],
                &[
                    0x41, 0, 0x41, 3, 0x41, 3, 0x41, 4, 0x10, 0, 0x1a, 0x41, returned, 0x0b,
                ],
            ),
        ]),
        data_section(&[(0, b"keydata")]),
    ])
}

fn storage_refusal_then_call_candidate(callee: ProgramId) -> Vec<u8> {
    let types = type_section(&[
        (&[TYPE_I32, TYPE_I32, TYPE_I32, TYPE_I32], &[TYPE_I32]),
        (&[TYPE_I32; 6], &[TYPE_I32]),
        (&[TYPE_I32], &[TYPE_I32]),
        (&[TYPE_I32, TYPE_I32], &[TYPE_I32]),
    ]);
    let imports = import_section(&[
        ("layerx_v1", "storage_write", 0),
        ("layerx_v1", "program_call", 1),
    ]);
    let functions = function_section(&[2, 3]);
    let memory = section(5, &[1, 1, 1, 1]);
    let mut exports = unsigned_leb(3);
    for (name, kind, index) in [
        ("layerx_reserve", 0_u8, 2_u8),
        ("layerx_call", 0, 3),
        ("memory", 2, 0),
    ] {
        exports.extend(unsigned_leb(name.len() as u64));
        exports.extend_from_slice(name.as_bytes());
        exports.extend_from_slice(&[kind, index]);
    }
    module(&[
        types,
        imports,
        functions,
        memory,
        section(7, &exports),
        code_section(&[
            func_body(&[], &[0x41, 0, 0x0b]),
            func_body(
                &[],
                &[
                    0x41, 0, 0x41, 3, 0x41, 3, 0x41, 4, 0x10, 0, 0x1a, 0x41, 32, 0x41, 32, 0x41, 0,
                    0x41, 0, 0x41, 8, 0x41, 2, 0x10, 1, 0x0b,
                ],
            ),
        ]),
        data_section(&[(0, b"keydata"), (8, &[0, 0]), (32, &callee.bytes()[..])]),
    ])
}

fn resource_then_negative_allocator_candidate() -> Vec<u8> {
    let types = type_section(&[
        (&[TYPE_I32, TYPE_I32, TYPE_I32, TYPE_I32], &[TYPE_I32]),
        (&[TYPE_I32], &[TYPE_I32]),
        (&[TYPE_I32, TYPE_I32], &[TYPE_I32]),
    ]);
    let imports = import_section(&[("layerx_v1", "storage_write", 0)]);
    let functions = function_section(&[1, 2]);
    let memory = section(5, &[1, 1, 1, 1]);
    let mut exports = unsigned_leb(3);
    for (name, kind, index) in [
        ("layerx_reserve", 0_u8, 1_u8),
        ("layerx_call", 0, 2),
        ("memory", 2, 0),
    ] {
        exports.extend(unsigned_leb(name.len() as u64));
        exports.extend_from_slice(name.as_bytes());
        exports.extend_from_slice(&[kind, index]);
    }
    module(&[
        types,
        imports,
        functions,
        memory,
        section(7, &exports),
        code_section(&[
            func_body(
                &[],
                &[
                    0x41, 0, 0x41, 3, 0x41, 3, 0x41, 4, 0x10, 0, 0x1a, 0x41, 0x7f, 0x0b,
                ],
            ),
            func_body(&[], &[0x41, 0, 0x0b]),
        ]),
        data_section(&[(0, b"keydata")]),
    ])
}

fn storage_reader_candidate() -> Vec<u8> {
    let types = type_section(&[
        (&[TYPE_I32, TYPE_I32, TYPE_I32, TYPE_I32], &[TYPE_I32]),
        (&[TYPE_I32], &[TYPE_I32]),
        (&[TYPE_I32, TYPE_I32], &[TYPE_I32]),
    ]);
    let imports = import_section(&[("layerx_v1", "storage_read", 0)]);
    let functions = function_section(&[1, 2]);
    let memory = section(5, &[1, 1, 1, 1]);
    let mut exports = unsigned_leb(3);
    for (name, kind, index) in [
        ("layerx_reserve", 0_u8, 1_u8),
        ("layerx_call", 0, 2),
        ("memory", 2, 0),
    ] {
        exports.extend(unsigned_leb(name.len() as u64));
        exports.extend_from_slice(name.as_bytes());
        exports.extend_from_slice(&[kind, index]);
    }
    module(&[
        types,
        imports,
        functions,
        memory,
        section(7, &exports),
        code_section(&[
            func_body(&[], &[0x41, 0, 0x0b]),
            func_body(&[], &[0x41, 0, 0x41, 3, 0x41, 16, 0x41, 4, 0x10, 0, 0x0b]),
        ]),
        data_section(&[(0, b"key")]),
    ])
}

fn storage_start_candidate(loop_forever: bool) -> Vec<u8> {
    let types = type_section(&[
        (&[TYPE_I32, TYPE_I32, TYPE_I32, TYPE_I32], &[TYPE_I32]),
        (&[], &[]),
        (&[TYPE_I32], &[TYPE_I32]),
        (&[TYPE_I32, TYPE_I32], &[TYPE_I32]),
    ]);
    let imports = import_section(&[("layerx_v1", "storage_write", 0)]);
    let functions = function_section(&[1, 2, 3]);
    let memory = section(5, &[1, 1, 1, 1]);
    let mut exports = unsigned_leb(3);
    for (name, kind, index) in [
        ("layerx_reserve", 0_u8, 2_u8),
        ("layerx_call", 0, 3),
        ("memory", 2, 0),
    ] {
        exports.extend(unsigned_leb(name.len() as u64));
        exports.extend_from_slice(name.as_bytes());
        exports.extend_from_slice(&[kind, index]);
    }
    let mut start = vec![0x41, 0, 0x41, 1, 0x41, 1, 0x41, 1, 0x10, 0, 0x1a];
    if loop_forever {
        start.extend_from_slice(&[0x03, 0x40, 0x0c, 0, 0x0b, 0x0b]);
    } else {
        start.extend_from_slice(&[0x00, 0x0b]);
    }
    module(&[
        types,
        imports,
        functions,
        memory,
        section(7, &exports),
        section(8, &[1]),
        code_section(&[
            func_body(&[], &start),
            func_body(&[], &[0x41, 0, 0x0b]),
            func_body(&[], &[0x41, 0, 0x0b]),
        ]),
        data_section(&[(0, b"kv")]),
    ])
}

fn response_candidate(bytes: &[u8], exhausts_cpu_after_publish: bool) -> Vec<u8> {
    let types = type_section(&[
        (&[TYPE_I32, TYPE_I32, TYPE_I32], &[TYPE_I32]),
        (&[TYPE_I32], &[TYPE_I32]),
        (&[TYPE_I32, TYPE_I32], &[TYPE_I32]),
    ]);
    let imports = import_section(&[("layerx_v2", "response_write", 0)]);
    let functions = function_section(&[1, 2]);
    let memory = section(5, &[1, 1, 1, 1]);
    let mut exports = unsigned_leb(3);
    for (name, kind, index) in [
        ("layerx_reserve", 0_u8, 1_u8),
        ("layerx_call", 0, 2),
        ("memory", 2, 0),
    ] {
        exports.extend(unsigned_leb(name.len() as u64));
        exports.extend_from_slice(name.as_bytes());
        exports.extend_from_slice(&[kind, index]);
    }
    let mut entry = vec![
        0x41,
        0,
        0x41,
        0,
        0x41,
        u8::try_from(bytes.len()).unwrap_or(u8::MAX),
        0x10,
        0,
        0x1a,
    ];
    if exhausts_cpu_after_publish {
        entry.extend_from_slice(&[0x03, 0x40, 0x0c, 0, 0x0b]);
    }
    entry.extend_from_slice(&[0x41, 0, 0x0b]);
    module(&[
        types,
        imports,
        functions,
        memory,
        section(7, &exports),
        code_section(&[func_body(&[], &[0x41, 0, 0x0b]), func_body(&[], &entry)]),
        data_section(&[(0, bytes)]),
    ])
}

fn refusal_then_loop_candidate() -> Vec<u8> {
    let types = type_section(&[
        (&[TYPE_I32, TYPE_I32, TYPE_I32], &[TYPE_I32]),
        (&[TYPE_I32], &[TYPE_I32]),
        (&[TYPE_I32, TYPE_I32], &[TYPE_I32]),
    ]);
    let imports = import_section(&[("layerx_v2", "refusal_write", 0)]);
    let functions = function_section(&[1, 2]);
    let memory = section(5, &[1, 1, 1, 1]);
    let mut exports = unsigned_leb(3);
    for (name, kind, index) in [
        ("layerx_reserve", 0_u8, 1_u8),
        ("layerx_call", 0, 2),
        ("memory", 2, 0),
    ] {
        exports.extend(unsigned_leb(name.len() as u64));
        exports.extend_from_slice(name.as_bytes());
        exports.extend_from_slice(&[kind, index]);
    }
    module(&[
        types,
        imports,
        functions,
        memory,
        section(7, &exports),
        code_section(&[
            func_body(&[], &[0x41, 0, 0x0b]),
            func_body(
                &[],
                &[
                    0x41, 1, 0x41, 0, 0x41, 1, 0x10, 0, 0x1a, 0x03, 0x40, 0x0c, 0, 0x0b, 0x41,
                    0x40, 0x0b,
                ],
            ),
        ]),
        data_section(&[(0, b"x")]),
    ])
}

fn table_candidate() -> Vec<u8> {
    let types = type_section(&[
        (&[TYPE_I32], &[TYPE_I32]),
        (&[TYPE_I32, TYPE_I32], &[TYPE_I32]),
    ]);
    let functions = function_section(&[0, 1]);
    let table = section(4, &[1, 0x70, 0, 1]);
    let memory = section(5, &[1, 1, 1, 1]);
    let mut exports = unsigned_leb(3);
    for (name, kind, index) in [
        ("layerx_reserve", 0_u8, 0_u8),
        ("layerx_call", 0, 1),
        ("memory", 2, 0),
    ] {
        exports.extend(unsigned_leb(name.len() as u64));
        exports.extend_from_slice(name.as_bytes());
        exports.extend_from_slice(&[kind, index]);
    }
    module(&[
        types,
        functions,
        table,
        memory,
        section(7, &exports),
        code_section(&[
            func_body(&[], &[0x41, 0, 0x0b]),
            func_body(&[], &[0x41, 0, 0x0b]),
        ]),
    ])
}

fn run_isolated_candidate(
    wasm: &[u8],
    budget: DeclaredBudget,
    capabilities: CapabilitySet,
    mut storage: Storage,
    response_capacity: usize,
    calldata: &[u8],
    binding_byte: u8,
) -> (
    Result<layerx_programs_runtime::CandidateAuthorizedExecutionRecord, ExecutionError>,
    Storage,
) {
    let executor = Executor::declared();
    let payer = principal(200);
    let binding = ActivityBudgetBinding::new([binding_byte; 32])
        .unwrap_or_else(|error| panic!("binding: {error}"));
    let token = executor
        .admit_activity_budget_for_qualification(budget, payer, binding, u128::MAX)
        .unwrap_or_else(|error| panic!("admission: {error}"));
    let engine = WasmEngine::declared().unwrap_or_else(|error| panic!("engine: {error}"));
    let module = engine
        .validate_candidate_v2(wasm)
        .unwrap_or_else(|error| panic!("validation: {error}"));
    let request = BudgetedAuthorizedExecutionRequest::new(
        AuthorizedExecutionRequest {
            module: &module,
            program: ProgramId::new([201; 32]).unwrap_or_else(|error| panic!("program: {error}")),
            authorization: AuthorizationContext::new(payer, capabilities),
            receipts: &NoReceipts,
            entrypoint: CALL_ENTRY_EXPORT,
            calldata,
            composition: CompositionContext::isolated(),
            response_capacity,
        },
        token,
        payer,
        binding,
    );
    let result =
        executor.execute_authorized_candidate_budgeted_for_qualification(&mut storage, request);
    (result, storage)
}

fn run_isolated_v1(
    wasm: &[u8],
    budget: DeclaredBudget,
    capabilities: CapabilitySet,
    calldata: &[u8],
    binding_byte: u8,
) -> (Result<BudgetedV1ActivityOutcome, ExecutionError>, Storage) {
    let executor = Executor::declared();
    let payer = principal(206);
    let binding = ActivityBudgetBinding::new([binding_byte; 32])
        .unwrap_or_else(|error| panic!("binding: {error}"));
    let token = executor
        .admit_activity_budget_for_qualification(budget, payer, binding, u128::MAX)
        .unwrap_or_else(|error| panic!("admission: {error}"));
    let engine = WasmEngine::declared().unwrap_or_else(|error| panic!("engine: {error}"));
    let module = engine
        .validate(wasm)
        .unwrap_or_else(|error| panic!("validation: {error}"));
    let request = BudgetedAuthorizedExecutionRequest::new(
        AuthorizedExecutionRequest {
            module: &module,
            program: ProgramId::new([207; 32]).unwrap_or_else(|error| panic!("program: {error}")),
            authorization: AuthorizationContext::new(payer, capabilities),
            receipts: &NoReceipts,
            entrypoint: CALL_ENTRY_EXPORT,
            calldata,
            composition: CompositionContext::isolated(),
            response_capacity: 0,
        },
        token,
        payer,
        binding,
    );
    let mut storage = Storage::new();
    let result = executor.execute_authorized_budgeted_for_qualification(&mut storage, request);
    (result, storage)
}

fn run_v1_graph(
    root_wasm: &[u8],
    children: &[(ProgramId, Vec<u8>)],
    budget: DeclaredBudget,
    capabilities: CapabilitySet,
    binding_byte: u8,
) -> (Result<BudgetedV1ActivityOutcome, ExecutionError>, Storage) {
    let executor = Executor::declared();
    let payer = principal(202);
    let binding = ActivityBudgetBinding::new([binding_byte; 32])
        .unwrap_or_else(|error| panic!("binding: {error}"));
    let token = executor
        .admit_activity_budget_for_qualification(budget, payer, binding, u128::MAX)
        .unwrap_or_else(|error| panic!("admission: {error}"));
    let engine = WasmEngine::declared().unwrap_or_else(|error| panic!("engine: {error}"));
    let root = engine
        .validate(root_wasm)
        .unwrap_or_else(|error| panic!("root validation: {error}"));
    let mut catalog = ProgramCatalog::new();
    for (program, wasm) in children {
        let module = engine
            .validate(wasm)
            .unwrap_or_else(|error| panic!("child validation: {error}"));
        catalog.insert(*program, module);
    }
    let request = BudgetedAuthorizedExecutionRequest::new(
        AuthorizedExecutionRequest {
            module: &root,
            program: ProgramId::new([203; 32]).unwrap_or_else(|error| panic!("program: {error}")),
            authorization: AuthorizationContext::new(payer, capabilities),
            receipts: &NoReceipts,
            entrypoint: CALL_ENTRY_EXPORT,
            calldata: &[],
            composition: CompositionContext::catalog(
                catalog,
                layerx_programs_runtime::CompositionRules::declared(),
            ),
            response_capacity: 0,
        },
        token,
        payer,
        binding,
    );
    let mut storage = Storage::new();
    let result = executor.execute_authorized_budgeted_for_qualification(&mut storage, request);
    (result, storage)
}

fn run_candidate_graph(
    root_wasm: &[u8],
    children: &[(ProgramId, Vec<u8>)],
    budget: DeclaredBudget,
    capabilities: CapabilitySet,
    binding_byte: u8,
) -> (
    Result<layerx_programs_runtime::CandidateAuthorizedExecutionRecord, ExecutionError>,
    Storage,
) {
    let executor = Executor::declared();
    let payer = principal(204);
    let binding = ActivityBudgetBinding::new([binding_byte; 32])
        .unwrap_or_else(|error| panic!("binding: {error}"));
    let token = executor
        .admit_activity_budget_for_qualification(budget, payer, binding, u128::MAX)
        .unwrap_or_else(|error| panic!("admission: {error}"));
    let engine = WasmEngine::declared().unwrap_or_else(|error| panic!("engine: {error}"));
    let root = engine
        .validate_candidate_v2(root_wasm)
        .unwrap_or_else(|error| panic!("root validation: {error}"));
    let mut catalog = ProgramCatalog::new();
    for (program, wasm) in children {
        let module = engine
            .validate_candidate_v2(wasm)
            .unwrap_or_else(|error| panic!("child validation: {error}"));
        catalog.insert(*program, module);
    }
    let request = BudgetedAuthorizedExecutionRequest::new(
        AuthorizedExecutionRequest {
            module: &root,
            program: ProgramId::new([205; 32]).unwrap_or_else(|error| panic!("program: {error}")),
            authorization: AuthorizationContext::new(payer, capabilities),
            receipts: &NoReceipts,
            entrypoint: CALL_ENTRY_EXPORT,
            calldata: &[],
            composition: CompositionContext::catalog(
                catalog,
                layerx_programs_runtime::CompositionRules::declared(),
            ),
            response_capacity: 64,
        },
        token,
        payer,
        binding,
    );
    let mut storage = Storage::new();
    let result =
        executor.execute_authorized_candidate_budgeted_for_qualification(&mut storage, request);
    (result, storage)
}

#[test]
fn declared_budget_enforces_exact_v1_bounds_in_stable_dimension_order() {
    let minimum = declared(MIN_ACTIVITY_CPU_FUEL, 65_536, 0, 0, 1, 0, 0);
    assert_eq!(minimum.cpu_fuel(), MIN_ACTIVITY_CPU_FUEL);
    assert_eq!(minimum.memory_bytes(), 65_536);
    assert_eq!(minimum.storage_read_bytes(), 0);
    assert_eq!(minimum.storage_write_bytes(), 0);
    assert_eq!(minimum.output_values(), 1);
    assert_eq!(minimum.output_bytes(), 0);
    assert_eq!(minimum.table_elements(), 0);

    assert_eq!(
        DeclaredBudget::new(MIN_ACTIVITY_CPU_FUEL - 1, 65_535, 0, 0, 0, 0, 0),
        Err(BudgetAdmissionRefusal::BelowMinimum {
            dimension: BudgetDimension::CpuFuel,
            minimum: MIN_ACTIVITY_CPU_FUEL,
            declared: MIN_ACTIVITY_CPU_FUEL - 1,
        })
    );
    assert_eq!(
        DeclaredBudget::new(MIN_ACTIVITY_CPU_FUEL, 65_535, 0, 0, 1, 0, 0),
        Err(BudgetAdmissionRefusal::BelowMinimum {
            dimension: BudgetDimension::MemoryBytes,
            minimum: 65_536,
            declared: 65_535,
        })
    );
    assert_eq!(
        DeclaredBudget::new(MIN_ACTIVITY_CPU_FUEL, 65_536, 0, 0, 0, 0, 0),
        Err(BudgetAdmissionRefusal::BelowMinimum {
            dimension: BudgetDimension::OutputValues,
            minimum: 1,
            declared: 0,
        })
    );

    let maximum = ResourceBudget::declared();
    let maximum_declared = declared(
        maximum.cpu_fuel(),
        maximum.memory_bytes(),
        maximum.storage_read_bytes(),
        maximum.storage_write_bytes(),
        maximum.output_values(),
        maximum.output_bytes(),
        maximum.table_elements(),
    );
    assert_eq!(maximum_declared.resource_budget(), maximum);

    let over = [
        (
            BudgetDimension::CpuFuel,
            DeclaredBudget::new(maximum.cpu_fuel() + 1, 65_536, 0, 0, 1, 0, 0),
            maximum.cpu_fuel(),
        ),
        (
            BudgetDimension::MemoryBytes,
            DeclaredBudget::new(
                MIN_ACTIVITY_CPU_FUEL,
                maximum.memory_bytes() + 1,
                0,
                0,
                1,
                0,
                0,
            ),
            maximum.memory_bytes(),
        ),
        (
            BudgetDimension::StorageReadBytes,
            DeclaredBudget::new(
                MIN_ACTIVITY_CPU_FUEL,
                65_536,
                maximum.storage_read_bytes() + 1,
                0,
                1,
                0,
                0,
            ),
            maximum.storage_read_bytes(),
        ),
        (
            BudgetDimension::StorageWriteBytes,
            DeclaredBudget::new(
                MIN_ACTIVITY_CPU_FUEL,
                65_536,
                0,
                maximum.storage_write_bytes() + 1,
                1,
                0,
                0,
            ),
            maximum.storage_write_bytes(),
        ),
        (
            BudgetDimension::OutputValues,
            DeclaredBudget::new(
                MIN_ACTIVITY_CPU_FUEL,
                65_536,
                0,
                0,
                maximum.output_values() + 1,
                0,
                0,
            ),
            u64::from(maximum.output_values()),
        ),
        (
            BudgetDimension::OutputBytes,
            DeclaredBudget::new(
                MIN_ACTIVITY_CPU_FUEL,
                65_536,
                0,
                0,
                1,
                maximum.output_bytes() + 1,
                0,
            ),
            maximum.output_bytes(),
        ),
        (
            BudgetDimension::TableElements,
            DeclaredBudget::new(
                MIN_ACTIVITY_CPU_FUEL,
                65_536,
                0,
                0,
                1,
                0,
                maximum.table_elements() + 1,
            ),
            u64::from(maximum.table_elements()),
        ),
    ];
    for (dimension, result, limit) in over {
        assert_eq!(
            result,
            Err(BudgetAdmissionRefusal::AboveMaximum {
                dimension,
                maximum: limit,
                declared: limit + 1,
            })
        );
    }
}

#[test]
fn protocol_minimum_executes_the_smallest_valid_empty_call() {
    let minimum = DeclaredBudget::minimum();
    let wasm = candidate_with_entry(&[0x41, 0, 0x0b]);
    let charge_sites = frozen_defined_function_charge_sites(&wasm, 1);
    assert_eq!(charge_sites, [3]);
    let exact_cpu = access_charge([]) + charge_sites[0];
    assert_eq!(exact_cpu, MIN_ACTIVITY_CPU_FUEL);
    assert_eq!(minimum.cpu_fuel(), exact_cpu);
    assert_eq!(minimum.memory_bytes(), 65_536);
    assert_eq!(minimum.output_values(), 1);

    let (result, storage) = run_isolated_candidate(
        &wasm,
        minimum,
        CapabilitySet::empty(),
        Storage::new(),
        0,
        &[],
        230,
    );
    let record = result.unwrap_or_else(|error| panic!("minimum measurement: {error}"));
    assert_eq!(record.execution().usage().cpu_fuel, exact_cpu);
    assert_eq!(record.execution().usage().memory_bytes, 65_536);
    assert_eq!(record.execution().usage().output_values, 1);
    assert_eq!(storage, Storage::new());

    let executor = Executor::declared();
    let payer = principal(231);
    let binding =
        ActivityBudgetBinding::new([232; 32]).unwrap_or_else(|error| panic!("binding: {error}"));
    let token = executor
        .admit_activity_budget_for_qualification(minimum, payer, binding, u128::MAX)
        .unwrap_or_else(|error| panic!("admission: {error}"));
    let engine = WasmEngine::declared().unwrap_or_else(|error| panic!("engine: {error}"));
    let module = engine
        .validate(&wasm)
        .unwrap_or_else(|error| panic!("validation: {error}"));
    let request = BudgetedAuthorizedExecutionRequest::new(
        AuthorizedExecutionRequest {
            module: &module,
            program: ProgramId::new([233; 32]).unwrap_or_else(|error| panic!("program: {error}")),
            authorization: AuthorizationContext::new(payer, CapabilitySet::empty()),
            receipts: &NoReceipts,
            entrypoint: CALL_ENTRY_EXPORT,
            calldata: &[],
            composition: CompositionContext::isolated(),
            response_capacity: 0,
        },
        token,
        payer,
        binding,
    );
    let v1 = executor
        .execute_authorized_budgeted_for_qualification(&mut Storage::new(), request)
        .unwrap_or_else(|error| panic!("v1 minimum execution: {error}"));
    let BudgetedV1ActivityOutcome::Success(v1) = v1 else {
        panic!("minimum v1 execution did not succeed");
    };
    assert_eq!(v1.execution.usage.cpu_fuel, exact_cpu);
    assert_eq!(v1.execution.usage.memory_bytes, 65_536);
    assert_eq!(v1.execution.usage.output_values, 1);
}

#[test]
fn legacy_candidate_calldata_copy_exhaustion_keeps_frozen_cpu_diagnostics() {
    let calldata = b"copy exhaustion";
    let attempted = access_charge([])
        + u64::try_from(calldata.len()).unwrap_or_else(|_| unreachable!())
            * layerx_programs_runtime::CALL_INPUT_FUEL_PER_BYTE;
    let limit = attempted - 1;
    let executor = Executor::new(
        ResourceBudget::new_complete(limit, 65_536, 0, 0, 2, 0, 0),
        FeeSchedule::declared(),
    );
    let engine = WasmEngine::declared().unwrap_or_else(|error| panic!("engine: {error}"));
    let module = engine
        .validate_candidate_v2(&candidate_with_entry(&[0x41, 0, 0x0b]))
        .unwrap_or_else(|error| panic!("validation: {error}"));
    let payer = principal(183);
    let result = executor.execute_authorized_candidate(
        &mut Storage::new(),
        AuthorizedExecutionRequest {
            module: &module,
            program: ProgramId::new([183; 32]).unwrap_or_else(|error| panic!("program: {error}")),
            authorization: AuthorizationContext::new(payer, CapabilitySet::empty()),
            receipts: &NoReceipts,
            entrypoint: CALL_ENTRY_EXPORT,
            calldata,
            composition: CompositionContext::isolated(),
            response_capacity: 0,
        },
    );
    assert_eq!(
        result,
        Err(ExecutionError::Resource(MeterRefusal::BudgetExceeded {
            resource: ResourceKind::Cpu,
            limit,
            attempted,
        }))
    );
}

#[test]
fn declared_budget_codec_is_fixed_width_strict_and_revalidates() {
    let budget = declared(MIN_ACTIVITY_CPU_FUEL + 16, 65_536, 23, 29, 3, 31, 5);
    let encoded = budget.canonical_bytes();
    assert_eq!(encoded.len(), 79);
    assert!(encoded.starts_with(b"LXP/program-declared-budget/v1\0"));
    assert_eq!(DeclaredBudget::canonical_decode(&encoded), Ok(budget));

    for end in 0..encoded.len() {
        assert_eq!(
            DeclaredBudget::canonical_decode(&encoded[..end]),
            Err(BudgetAdmissionRefusal::MalformedCanonicalBytes)
        );
    }
    let mut trailing = encoded.clone();
    trailing.push(0);
    assert_eq!(
        DeclaredBudget::canonical_decode(&trailing),
        Err(BudgetAdmissionRefusal::MalformedCanonicalBytes)
    );

    let mut below_minimum = encoded;
    let cpu_offset = b"LXP/program-declared-budget/v1\0".len();
    below_minimum[cpu_offset..cpu_offset + 8].copy_from_slice(&0_u64.to_be_bytes());
    assert_eq!(
        DeclaredBudget::canonical_decode(&below_minimum),
        Err(BudgetAdmissionRefusal::BelowMinimum {
            dimension: BudgetDimension::CpuFuel,
            minimum: MIN_ACTIVITY_CPU_FUEL,
            declared: 0,
        })
    );
}

#[test]
fn activity_budget_admission_binds_exact_coverage_schedule_and_custom_maximum() {
    let prices = FeeSchedule::new(1, 1, 2, 4, 1).with_output_byte_price(1);
    let maximum = ResourceBudget::new_complete(100, 100_000, 100, 100, 5, 100, 5);
    let executor = Executor::new(maximum, prices);
    let budget = declared(MIN_ACTIVITY_CPU_FUEL, 65_536, 3, 4, 2, 5, 3);
    let payer = principal(7);
    let binding = match ActivityBudgetBinding::new([8; 32]) {
        Ok(binding) => binding,
        Err(error) => panic!("binding: {error}"),
    };
    let admitted =
        match executor.admit_activity_budget_for_qualification(budget, payer, binding, 65_604) {
            Ok(admitted) => admitted,
            Err(error) => panic!("admission: {error}"),
        };
    assert_eq!(admitted.resource_budget(), budget.resource_budget());
    assert_eq!(admitted.payer(), payer);
    assert_eq!(admitted.maximum_fee_units(), 65_604);

    assert_eq!(
        executor.admit_activity_budget_for_qualification(budget, payer, binding, 65_603),
        Err(BudgetAdmissionRefusal::InsufficientCoverage {
            required: 65_604,
            available: 65_603,
        })
    );
    assert_eq!(
        executor.admit_activity_budget_for_qualification(
            declared(101, 65_536, 3, 4, 2, 5, 3),
            payer,
            binding,
            u128::MAX,
        ),
        Err(BudgetAdmissionRefusal::AboveMaximum {
            dimension: BudgetDimension::CpuFuel,
            maximum: 100,
            declared: 101,
        })
    );
}

#[test]
fn invalid_over_and_unfunded_admission_precede_any_poison_guest_work() {
    let engine = WasmEngine::declared().unwrap_or_else(|error| panic!("engine: {error}"));
    let _poison = engine
        .validate_candidate_v2(&poison_start_candidate())
        .unwrap_or_else(|error| panic!("poison validation: {error}"));

    let mut malformed = DeclaredBudget::minimum().canonical_bytes();
    malformed[0] ^= 0xff;
    assert_eq!(
        DeclaredBudget::canonical_decode(&malformed),
        Err(BudgetAdmissionRefusal::MalformedCanonicalBytes)
    );

    let mut over = DeclaredBudget::minimum().canonical_bytes();
    let cpu_offset = b"LXP/program-declared-budget/v1\0".len();
    over[cpu_offset..cpu_offset + 8]
        .copy_from_slice(&(ResourceBudget::declared().cpu_fuel() + 1).to_be_bytes());
    assert_eq!(
        DeclaredBudget::canonical_decode(&over),
        Err(BudgetAdmissionRefusal::AboveMaximum {
            dimension: BudgetDimension::CpuFuel,
            maximum: ResourceBudget::declared().cpu_fuel(),
            declared: ResourceBudget::declared().cpu_fuel() + 1,
        })
    );

    let executor = Executor::declared();
    let payer = principal(178);
    let binding =
        ActivityBudgetBinding::new([178; 32]).unwrap_or_else(|error| panic!("binding: {error}"));
    let admitted = executor
        .admit_activity_budget_for_qualification(
            DeclaredBudget::minimum(),
            payer,
            binding,
            u128::MAX,
        )
        .unwrap_or_else(|error| panic!("ceiling measurement: {error}"));
    let required = admitted.maximum_fee_units();
    assert_eq!(
        executor.admit_activity_budget_for_qualification(
            DeclaredBudget::minimum(),
            payer,
            binding,
            required - 1,
        ),
        Err(BudgetAdmissionRefusal::InsufficientCoverage {
            required,
            available: required - 1,
        })
    );
}

#[test]
fn protocol_budget_law_makes_public_ceiling_fee_overflow_unreachable() {
    let maximum_price = u64::MAX;
    let executor = Executor::new(
        ResourceBudget::declared(),
        FeeSchedule::new(
            maximum_price,
            maximum_price,
            maximum_price,
            maximum_price,
            maximum_price,
        )
        .with_output_byte_price(maximum_price),
    );
    let payer = principal(179);
    let binding =
        ActivityBudgetBinding::new([179; 32]).unwrap_or_else(|error| panic!("binding: {error}"));
    let admitted = executor
        .admit_activity_budget_for_qualification(
            DeclaredBudget::protocol_maximum(),
            payer,
            binding,
            u128::MAX,
        )
        .unwrap_or_else(|error| panic!("protocol maximum overflowed: {error}"));
    assert!(admitted.maximum_fee_units() < u128::MAX);
}

#[test]
fn budget_token_mismatch_refuses_before_a_poison_start() {
    let admitting = Executor::declared();
    let payer = principal(9);
    let binding = match ActivityBudgetBinding::new([10; 32]) {
        Ok(binding) => binding,
        Err(error) => panic!("binding: {error}"),
    };
    let engine = WasmEngine::declared().unwrap_or_else(|error| panic!("engine: {error}"));
    let module = engine
        .validate_candidate_v2(&poison_start_candidate())
        .unwrap_or_else(|error| panic!("validation: {error}"));
    let other_binding = ActivityBudgetBinding::new([11; 32])
        .unwrap_or_else(|error| panic!("other binding: {error}"));
    let narrower = ResourceBudget::new_complete(10_000, 65_536, 1_024, 1_024, 4, 1_024, 4);
    for (case, executing, request_payer, request_binding, expected) in [
        (
            "payer",
            admitting,
            principal(8),
            binding,
            BudgetAdmissionRefusal::PayerMismatch,
        ),
        (
            "binding",
            admitting,
            payer,
            other_binding,
            BudgetAdmissionRefusal::ActivityBindingMismatch,
        ),
        (
            "schedule",
            Executor::new(ResourceBudget::declared(), FeeSchedule::new(2, 1, 2, 4, 1)),
            payer,
            binding,
            BudgetAdmissionRefusal::ScheduleMismatch,
        ),
        (
            "maximum",
            Executor::new(narrower, FeeSchedule::declared()),
            payer,
            binding,
            BudgetAdmissionRefusal::MaximumPolicyMismatch,
        ),
    ] {
        let token = admitting
            .admit_activity_budget_for_qualification(
                DeclaredBudget::protocol_maximum(),
                payer,
                binding,
                u128::MAX,
            )
            .unwrap_or_else(|error| panic!("{case} admission: {error}"));
        let request = BudgetedAuthorizedExecutionRequest::new(
            AuthorizedExecutionRequest {
                module: &module,
                program: ProgramId::new([12; 32])
                    .unwrap_or_else(|error| panic!("program: {error}")),
                authorization: AuthorizationContext::new(payer, CapabilitySet::empty()),
                receipts: &NoReceipts,
                entrypoint: CALL_ENTRY_EXPORT,
                calldata: &[],
                composition: CompositionContext::isolated(),
                response_capacity: 0,
            },
            token,
            request_payer,
            request_binding,
        );
        assert_eq!(
            executing.execute_authorized_candidate_budgeted_for_qualification(
                &mut Storage::new(),
                request,
            ),
            Err(ExecutionError::Budget(expected)),
            "{case} mismatch reached poison start"
        );
    }
}

#[test]
fn budgeted_preflight_beats_poison_start_and_allocator_refusal_retains_usage() {
    let budget = declared(10_000, 65_536, 0, 0, 2, 0, 0);
    let (candidate_missing_entry, storage) = run_isolated_candidate(
        &poison_start_candidate(),
        budget,
        CapabilitySet::empty(),
        Storage::new(),
        0,
        &[],
        155,
    );
    assert_eq!(
        candidate_missing_entry,
        Err(ExecutionError::Entrypoint(EntrypointRefusal::MissingEntry))
    );
    assert_eq!(storage, Storage::new());
    let (v1_missing_entry, storage) = run_isolated_v1(
        &poison_start_candidate(),
        budget,
        CapabilitySet::empty(),
        &[],
        156,
    );
    assert_eq!(
        v1_missing_entry,
        Err(ExecutionError::Entrypoint(EntrypointRefusal::MissingEntry))
    );
    assert_eq!(storage, Storage::new());

    let missing_allocator = poison_start_missing_allocator_candidate();
    let (candidate_missing_allocator, storage) = run_isolated_candidate(
        &missing_allocator,
        budget,
        CapabilitySet::empty(),
        Storage::new(),
        0,
        b"x",
        157,
    );
    assert_eq!(
        candidate_missing_allocator,
        Err(ExecutionError::Entrypoint(
            EntrypointRefusal::MissingAllocator
        ))
    );
    assert_eq!(storage, Storage::new());
    let (v1_missing_allocator, storage) = run_isolated_v1(
        &missing_allocator,
        budget,
        CapabilitySet::empty(),
        b"x",
        158,
    );
    assert_eq!(
        v1_missing_allocator,
        Err(ExecutionError::Entrypoint(
            EntrypointRefusal::MissingAllocator
        ))
    );
    assert_eq!(storage, Storage::new());

    let allocator = negative_allocator_candidate();
    let (candidate, storage) = run_isolated_candidate(
        &allocator,
        budget,
        CapabilitySet::empty(),
        Storage::new(),
        0,
        b"x",
        159,
    );
    let candidate = candidate.unwrap_or_else(|error| panic!("candidate allocator: {error}"));
    let candidate_failure = candidate
        .failure()
        .unwrap_or_else(|| panic!("candidate allocator refusal was not receipt ready"));
    assert_eq!(candidate_failure.program(), candidate.root_program());
    assert_eq!(candidate_failure.class(), RefusalClass::Legacy);
    assert!(candidate.execution().usage().cpu_fuel > 0);
    assert_eq!(candidate.execution().usage().memory_bytes, 65_536);
    assert_eq!(storage, Storage::new());

    let (v1, storage) = run_isolated_v1(&allocator, budget, CapabilitySet::empty(), b"x", 160);
    let BudgetedV1ActivityOutcome::Failure(v1) =
        v1.unwrap_or_else(|error| panic!("v1 allocator: {error}"))
    else {
        panic!("v1 allocator refusal was not receipt ready");
    };
    assert_eq!(
        v1.cause(),
        &BudgetedV1FailureCause::Entrypoint(EntrypointRefusal::AllocationRefused { code: -1 })
    );
    assert!(v1.usage().cpu_fuel > 0);
    assert_eq!(v1.usage().memory_bytes, 65_536);
    assert_eq!(storage, Storage::new());
}

#[test]
fn nested_budgeted_preflight_rejects_missing_child_entry_before_graph_entry_or_start() {
    let child = ProgramId::new([161; 32]).unwrap_or_else(|error| panic!("child: {error}"));
    let root_wasm = v1_forwarder(child);
    let capabilities = CapabilitySet::new([Capability::Call { program: child }])
        .unwrap_or_else(|error| panic!("capabilities: {error}"));
    let (outcome, storage) = run_v1_graph(
        &root_wasm,
        &[(child, poison_start_candidate())],
        declared(100_000, 131_072, 0, 0, 2, 0, 0),
        capabilities,
        161,
    );
    let BudgetedV1ActivityOutcome::Failure(failure) =
        outcome.unwrap_or_else(|error| panic!("nested preflight: {error}"))
    else {
        panic!("nested missing entry was not receipt ready");
    };
    assert_eq!(
        failure.cause(),
        &BudgetedV1FailureCause::Composition(CompositionRefusal::MissingEntry)
    );
    assert!(failure.usage().cpu_fuel > 0);
    assert!(failure.call_graph().edges().is_empty());
    assert_eq!(storage, Storage::new());
}

#[test]
fn ignored_host_resource_and_post_start_unknown_program_are_receipt_ready() {
    let storage_capability = CapabilitySet::new([Capability::StorageWrite])
        .unwrap_or_else(|error| panic!("storage capability: {error}"));
    let (outcome, storage) = run_isolated_v1(
        &storage_writer_ignores_status_candidate(),
        declared(10_000, 65_536, 0, 0, 1, 0, 0),
        storage_capability,
        &[],
        162,
    );
    let BudgetedV1ActivityOutcome::Resource(resource) =
        outcome.unwrap_or_else(|error| panic!("ignored resource: {error}"))
    else {
        panic!("ignored host resource status escaped as success");
    };
    assert_eq!(
        resource.refusal(),
        BudgetMeterRefusal::BudgetExceeded {
            resource: BudgetResourceKind::StorageWrite,
            limit: 0,
            attempted: 7,
        }
    );
    assert_eq!(resource.usage().storage_write_bytes, 0);
    assert_eq!(storage, Storage::new());

    let missing = ProgramId::new([163; 32]).unwrap_or_else(|error| panic!("missing: {error}"));
    let capabilities = CapabilitySet::new([Capability::Call { program: missing }])
        .unwrap_or_else(|error| panic!("capabilities: {error}"));
    let (outcome, storage) = run_v1_graph(
        &v1_forwarder(missing),
        &[],
        declared(100_000, 65_536, 0, 0, 2, 0, 0),
        capabilities,
        163,
    );
    let BudgetedV1ActivityOutcome::Failure(failure) =
        outcome.unwrap_or_else(|error| panic!("unknown program: {error}"))
    else {
        panic!("post-start unknown program was not receipt ready");
    };
    assert_eq!(
        failure.cause(),
        &BudgetedV1FailureCause::Composition(CompositionRefusal::UnknownProgram {
            program: missing,
        })
    );
    assert!(failure.usage().cpu_fuel > 0);
    assert!(failure.call_graph().edges().is_empty());
    assert_eq!(storage, Storage::new());
}

#[test]
fn candidate_resource_precedes_negative_entry_and_allocator_results() {
    let storage_capability = CapabilitySet::new([Capability::StorageWrite])
        .unwrap_or_else(|error| panic!("storage capability: {error}"));
    for (index, returned) in [0x7f_u8, 0x40].into_iter().enumerate() {
        let (record, storage) = run_isolated_candidate(
            &storage_writer_ignores_status_with_return(returned),
            declared(10_000, 65_536, 0, 0, 1, 0, 0),
            storage_capability.clone(),
            Storage::new(),
            0,
            &[],
            180 + u8::try_from(index).unwrap_or(u8::MAX),
        );
        let record = record.unwrap_or_else(|error| panic!("negative entry {index}: {error}"));
        assert_eq!(
            record.resource_refusal(),
            Some(&BudgetMeterRefusal::BudgetExceeded {
                resource: BudgetResourceKind::StorageWrite,
                limit: 0,
                attempted: 7,
            })
        );
        assert!(record.failure().is_none());
        assert_eq!(record.execution().usage().storage_write_bytes, 0);
        assert_eq!(storage, Storage::new());
    }

    let (record, storage) = run_isolated_candidate(
        &resource_then_negative_allocator_candidate(),
        declared(10_000, 65_536, 0, 0, 2, 0, 0),
        storage_capability,
        Storage::new(),
        0,
        b"x",
        182,
    );
    let record = record.unwrap_or_else(|error| panic!("negative allocator: {error}"));
    assert_eq!(
        record.resource_refusal(),
        Some(&BudgetMeterRefusal::BudgetExceeded {
            resource: BudgetResourceKind::StorageWrite,
            limit: 0,
            attempted: 7,
        })
    );
    assert!(record.failure().is_none());
    assert_eq!(record.execution().usage().storage_write_bytes, 0);
    assert_eq!(storage, Storage::new());
}

#[test]
fn first_activity_meter_refusal_survives_a_later_cpu_exhaustion() {
    let capabilities = CapabilitySet::new([Capability::StorageWrite])
        .unwrap_or_else(|error| panic!("capabilities: {error}"));
    let budget = declared(10_000, 65_536, 0, 0, 1, 0, 0);
    let expected = BudgetMeterRefusal::BudgetExceeded {
        resource: BudgetResourceKind::StorageWrite,
        limit: 0,
        attempted: 2,
    };

    let (v1, storage) = run_isolated_v1(
        &storage_start_candidate(true),
        budget,
        capabilities.clone(),
        &[],
        184,
    );
    let BudgetedV1ActivityOutcome::Resource(v1) =
        v1.unwrap_or_else(|error| panic!("v1 sticky resource: {error}"))
    else {
        panic!("v1 sticky resource escaped");
    };
    assert_eq!(v1.refusal(), expected);
    assert_eq!(v1.usage().storage_write_bytes, 0);
    assert_eq!(storage, Storage::new());

    let (candidate, storage) = run_isolated_candidate(
        &storage_start_candidate(true),
        budget,
        capabilities,
        Storage::new(),
        0,
        &[],
        185,
    );
    let candidate = candidate.unwrap_or_else(|error| panic!("candidate sticky resource: {error}"));
    assert_eq!(candidate.resource_refusal(), Some(&expected));
    assert_eq!(candidate.execution().usage().storage_write_bytes, 0);
    assert_eq!(storage, Storage::new());
}

#[test]
fn first_non_table_refusal_survives_later_table_growth() {
    let child = ProgramId::new([186; 32]).unwrap_or_else(|error| panic!("child: {error}"));
    let capabilities = CapabilitySet::new([
        Capability::StorageWrite,
        Capability::Call { program: child },
    ])
    .unwrap_or_else(|error| panic!("capabilities: {error}"));
    let budget = declared(10_000, 131_072, 0, 0, 2, 0, 0);
    let expected = BudgetMeterRefusal::BudgetExceeded {
        resource: BudgetResourceKind::StorageWrite,
        limit: 0,
        attempted: 7,
    };
    let wasm = storage_refusal_then_call_candidate(child);
    let child_wasm = table_candidate();

    let (v1, storage) = run_v1_graph(
        &wasm,
        &[(child, child_wasm.clone())],
        budget,
        capabilities.clone(),
        186,
    );
    let BudgetedV1ActivityOutcome::Resource(v1) =
        v1.unwrap_or_else(|error| panic!("v1 sticky table resource: {error}"))
    else {
        panic!("v1 sticky table resource escaped");
    };
    assert_eq!(v1.refusal(), expected);
    assert_eq!(v1.usage().storage_write_bytes, 0);
    assert_eq!(v1.call_graph().edges().len(), 1);
    assert_eq!(storage, Storage::new());

    let (candidate, storage) =
        run_candidate_graph(&wasm, &[(child, child_wasm)], budget, capabilities, 187);
    let candidate =
        candidate.unwrap_or_else(|error| panic!("candidate sticky table resource: {error}"));
    assert_eq!(candidate.resource_refusal(), Some(&expected));
    assert_eq!(candidate.execution().usage().storage_write_bytes, 0);
    assert_eq!(candidate.call_graph().edges().len(), 1);
    assert_eq!(storage, Storage::new());
}

#[test]
fn budgeted_candidate_cpu_exhaustion_is_a_receipt_resource_outcome() {
    let executor = Executor::declared();
    let payer = principal(12);
    let binding =
        ActivityBudgetBinding::new([13; 32]).unwrap_or_else(|error| panic!("binding: {error}"));
    let declared = declared(100, 65_536, 0, 0, 2, 0, 0);
    let token = executor
        .admit_activity_budget_for_qualification(declared, payer, binding, u128::MAX)
        .unwrap_or_else(|error| panic!("admission: {error}"));
    let engine = WasmEngine::declared().unwrap_or_else(|error| panic!("engine: {error}"));
    let module = engine
        .validate_candidate_v2(&looping_candidate())
        .unwrap_or_else(|error| panic!("validation: {error}"));
    let request = BudgetedAuthorizedExecutionRequest::new(
        AuthorizedExecutionRequest {
            module: &module,
            program: ProgramId::new([14; 32]).unwrap_or_else(|error| panic!("program: {error}")),
            authorization: AuthorizationContext::new(payer, CapabilitySet::empty()),
            receipts: &NoReceipts,
            entrypoint: CALL_ENTRY_EXPORT,
            calldata: &[],
            composition: CompositionContext::isolated(),
            response_capacity: 0,
        },
        token,
        payer,
        binding,
    );
    let record = executor
        .execute_authorized_candidate_budgeted_for_qualification(&mut Storage::new(), request)
        .unwrap_or_else(|error| panic!("budgeted execution: {error}"));
    let charge_sites = frozen_defined_function_charge_sites(&looping_candidate(), 1);
    assert_eq!(charge_sites, [1, 2]);
    let (usage, attempted) =
        repeated_charge_exhaustion(100, access_charge([]) + charge_sites[0], charge_sites[1]);
    let refusal = BudgetMeterRefusal::BudgetExceeded {
        resource: BudgetResourceKind::Cpu,
        limit: 100,
        attempted,
    };
    assert_eq!(
        record.outcome(),
        &CandidateActivityOutcome::Resource(refusal)
    );
    assert_eq!(record.resource_refusal(), Some(&refusal));
    assert!(record.response().is_none());
    assert!(record.failure().is_none());
    assert!(record.effects().is_none());
    assert_eq!(record.execution().usage().cpu_fuel, usage);
    let projection = record.receipt_projection();
    assert_eq!(
        projection.outcome(),
        &CandidateReceiptOutcome::Resource(refusal)
    );
    let encoded = projection.canonical_encode();
    assert_eq!(
        layerx_programs_runtime::CandidateActivityReceipt::canonical_decode(&encoded),
        Ok(projection.clone())
    );
    let outcome_offset = encoded.len() - 19;
    for offset in [outcome_offset, outcome_offset + 1, outcome_offset + 2] {
        let mut malformed = encoded.clone();
        malformed[offset] = 0xff;
        assert!(
            layerx_programs_runtime::CandidateActivityReceipt::canonical_decode(&malformed)
                .is_err()
        );
    }
    let mut non_exceeding = encoded.clone();
    let limit = non_exceeding[outcome_offset + 3..outcome_offset + 11].to_vec();
    non_exceeding[outcome_offset + 11..outcome_offset + 19].copy_from_slice(&limit);
    assert!(
        layerx_programs_runtime::CandidateActivityReceipt::canonical_decode(&non_exceeding)
            .is_err()
    );
    let mut usage_past_limit = encoded.clone();
    let usage_offset = b"LXP/program-activity-receipt/v2\0".len() + 32 + 2 + 2 + 4;
    usage_past_limit[usage_offset..usage_offset + 8].copy_from_slice(&101_u64.to_be_bytes());
    assert!(
        layerx_programs_runtime::CandidateActivityReceipt::canonical_decode(&usage_past_limit)
            .is_err()
    );
    let mut counter_overflow = encoded[..encoded.len() - 16].to_vec();
    counter_overflow[outcome_offset + 1] = 1;
    let decoded_counter =
        layerx_programs_runtime::CandidateActivityReceipt::canonical_decode(&counter_overflow)
            .unwrap_or_else(|error| panic!("counter resource decode: {error}"));
    assert_eq!(
        decoded_counter.outcome(),
        &CandidateReceiptOutcome::Resource(BudgetMeterRefusal::CounterOverflow {
            resource: BudgetResourceKind::Cpu,
        })
    );
    let mut trailing = encoded;
    trailing.push(0);
    assert!(
        layerx_programs_runtime::CandidateActivityReceipt::canonical_decode(&trailing).is_err()
    );
}

#[test]
fn budgeted_v1_cpu_exhaustion_retains_actual_usage_and_failed_graph() {
    let executor = Executor::declared();
    let payer = principal(15);
    let binding =
        ActivityBudgetBinding::new([16; 32]).unwrap_or_else(|error| panic!("binding: {error}"));
    let token = executor
        .admit_activity_budget_for_qualification(
            declared(100, 65_536, 0, 0, 2, 0, 0),
            payer,
            binding,
            u128::MAX,
        )
        .unwrap_or_else(|error| panic!("admission: {error}"));
    let engine = WasmEngine::declared().unwrap_or_else(|error| panic!("engine: {error}"));
    let module = engine
        .validate(&looping_candidate())
        .unwrap_or_else(|error| panic!("validation: {error}"));
    let program = ProgramId::new([17; 32]).unwrap_or_else(|error| panic!("program: {error}"));
    let request = BudgetedAuthorizedExecutionRequest::new(
        AuthorizedExecutionRequest {
            module: &module,
            program,
            authorization: AuthorizationContext::new(payer, CapabilitySet::empty()),
            receipts: &NoReceipts,
            entrypoint: CALL_ENTRY_EXPORT,
            calldata: &[],
            composition: CompositionContext::isolated(),
            response_capacity: 0,
        },
        token,
        payer,
        binding,
    );
    let before = Storage::new();
    let mut storage = before.clone();
    let outcome = executor
        .execute_authorized_budgeted_for_qualification(&mut storage, request)
        .unwrap_or_else(|error| panic!("budgeted v1 execution: {error}"));
    let BudgetedV1ActivityOutcome::Resource(failure) = outcome else {
        panic!("expected resource outcome");
    };
    assert_eq!(failure.root_program(), program);
    let charge_sites = frozen_defined_function_charge_sites(&looping_candidate(), 1);
    assert_eq!(charge_sites, [1, 2]);
    let (usage, attempted) =
        repeated_charge_exhaustion(100, access_charge([]) + charge_sites[0], charge_sites[1]);
    assert_eq!(
        failure.refusal(),
        BudgetMeterRefusal::BudgetExceeded {
            resource: BudgetResourceKind::Cpu,
            limit: 100,
            attempted,
        }
    );
    assert_eq!(failure.usage().cpu_fuel, usage);
    assert!(failure.call_graph().edges().is_empty());
    assert_eq!(storage, before);
}

#[test]
fn budgeted_v1_root_refusal_and_fault_are_receipt_ready_without_state_escape() {
    let executor = Executor::declared();
    let payer = principal(18);
    let program = ProgramId::new([19; 32]).unwrap_or_else(|error| panic!("program: {error}"));
    for (case, wasm, class) in [
        (
            "refusal",
            candidate_with_entry(&[0x41, 0x7f, 0x0b]),
            RefusalClass::Legacy,
        ),
        (
            "fault",
            candidate_with_entry(&[0x00, 0x0b]),
            RefusalClass::RuntimeFault,
        ),
    ] {
        let binding = ActivityBudgetBinding::new(if case == "refusal" {
            [20; 32]
        } else {
            [21; 32]
        })
        .unwrap_or_else(|error| panic!("binding: {error}"));
        let token = executor
            .admit_activity_budget_for_qualification(
                declared(10_000, 65_536, 0, 0, 2, 0, 0),
                payer,
                binding,
                u128::MAX,
            )
            .unwrap_or_else(|error| panic!("admission: {error}"));
        let engine = WasmEngine::declared().unwrap_or_else(|error| panic!("engine: {error}"));
        let module = engine
            .validate(&wasm)
            .unwrap_or_else(|error| panic!("validation: {error}"));
        let request = BudgetedAuthorizedExecutionRequest::new(
            AuthorizedExecutionRequest {
                module: &module,
                program,
                authorization: AuthorizationContext::new(payer, CapabilitySet::empty()),
                receipts: &NoReceipts,
                entrypoint: CALL_ENTRY_EXPORT,
                calldata: &[],
                composition: CompositionContext::isolated(),
                response_capacity: 0,
            },
            token,
            payer,
            binding,
        );
        let before = Storage::new();
        let mut storage = before.clone();
        let outcome = executor
            .execute_authorized_budgeted_for_qualification(&mut storage, request)
            .unwrap_or_else(|error| panic!("{case}: {error}"));
        let BudgetedV1ActivityOutcome::Failure(failure) = outcome else {
            panic!("{case}: expected failure outcome");
        };
        assert_eq!(failure.root_program(), program);
        let program_failure = failure
            .program_failure()
            .unwrap_or_else(|| panic!("{case}: missing program failure"));
        assert_eq!(program_failure.program(), program);
        assert_eq!(program_failure.class(), class);
        assert!(program_failure.reason().bytes().is_empty());
        assert!(failure.usage().cpu_fuel > 0);
        assert!(failure.call_graph().edges().is_empty());
        assert_eq!(storage, before);
    }
}

#[test]
fn budgeted_v1_nested_refusal_and_fault_keep_the_actual_leaf() {
    let executor = Executor::declared();
    let payer = principal(22);
    let root_program = ProgramId::new([23; 32]).unwrap_or_else(|error| panic!("root: {error}"));
    let child_program = ProgramId::new([24; 32]).unwrap_or_else(|error| panic!("child: {error}"));
    for (index, child_wasm, class) in [
        (
            0_u8,
            candidate_with_entry(&[0x41, 0x7f, 0x0b]),
            RefusalClass::Legacy,
        ),
        (
            1,
            candidate_with_entry(&[0x00, 0x0b]),
            RefusalClass::RuntimeFault,
        ),
    ] {
        let binding = ActivityBudgetBinding::new([25 + index; 32])
            .unwrap_or_else(|error| panic!("binding: {error}"));
        let token = executor
            .admit_activity_budget_for_qualification(
                declared(100_000, 131_072, 0, 0, 4, 0, 0),
                payer,
                binding,
                u128::MAX,
            )
            .unwrap_or_else(|error| panic!("admission: {error}"));
        let engine = WasmEngine::declared().unwrap_or_else(|error| panic!("engine: {error}"));
        let root = engine
            .validate(&v1_forwarder(child_program))
            .unwrap_or_else(|error| panic!("root validation: {error}"));
        let child = engine
            .validate(&child_wasm)
            .unwrap_or_else(|error| panic!("child validation: {error}"));
        let mut catalog = ProgramCatalog::new();
        catalog.insert(child_program, child);
        let capabilities = CapabilitySet::new([Capability::Call {
            program: child_program,
        }])
        .unwrap_or_else(|error| panic!("capability: {error}"));
        let request = BudgetedAuthorizedExecutionRequest::new(
            AuthorizedExecutionRequest {
                module: &root,
                program: root_program,
                authorization: AuthorizationContext::new(payer, capabilities),
                receipts: &NoReceipts,
                entrypoint: CALL_ENTRY_EXPORT,
                calldata: &[],
                composition: CompositionContext::catalog(
                    catalog,
                    layerx_programs_runtime::CompositionRules::declared(),
                ),
                response_capacity: 0,
            },
            token,
            payer,
            binding,
        );
        let before = Storage::new();
        let mut storage = before.clone();
        let outcome = executor
            .execute_authorized_budgeted_for_qualification(&mut storage, request)
            .unwrap_or_else(|error| panic!("nested: {error}"));
        let BudgetedV1ActivityOutcome::Failure(failure) = outcome else {
            panic!("expected nested failure");
        };
        assert_eq!(failure.root_program(), root_program);
        let program_failure = failure
            .program_failure()
            .unwrap_or_else(|| panic!("missing nested program failure"));
        assert_eq!(program_failure.program(), child_program);
        assert_eq!(program_failure.class(), class);
        assert!(program_failure.reason().bytes().is_empty());
        assert_eq!(failure.call_graph().edges().len(), 1);
        assert_eq!(failure.call_graph().edges()[0].callee(), child_program);
        assert!(failure.usage().cpu_fuel > 0);
        assert_eq!(storage, before);
    }
}

#[test]
fn retained_start_faults_keep_usage_leaf_identity_and_atomic_rollback() {
    let root_wasm = storage_start_candidate(false);
    let storage_capability = CapabilitySet::new([Capability::StorageWrite])
        .unwrap_or_else(|error| panic!("storage capability: {error}"));
    let budget = declared(50_000, 65_536, 0, 2, 1, 0, 0);

    let (v1, storage) = run_v1_graph(&root_wasm, &[], budget, storage_capability.clone(), 151);
    let BudgetedV1ActivityOutcome::Failure(v1) =
        v1.unwrap_or_else(|error| panic!("v1 start fault: {error}"))
    else {
        panic!("v1 start fault was not receipt ready");
    };
    let v1_failure = v1
        .program_failure()
        .unwrap_or_else(|| panic!("v1 start fault had no program identity"));
    assert_eq!(v1_failure.program(), v1.root_program());
    assert_eq!(v1_failure.class(), RefusalClass::RuntimeFault);
    assert!(v1.usage().cpu_fuel > 0);
    assert_eq!(v1.usage().storage_write_bytes, 2);
    assert!(v1.call_graph().edges().is_empty());
    assert_eq!(storage, Storage::new());

    let (candidate, storage) = run_isolated_candidate(
        &root_wasm,
        budget,
        storage_capability.clone(),
        Storage::new(),
        0,
        &[],
        152,
    );
    let candidate = candidate.unwrap_or_else(|error| panic!("candidate start fault: {error}"));
    let candidate_failure = candidate
        .failure()
        .unwrap_or_else(|| panic!("candidate start fault had no failure"));
    assert_eq!(candidate_failure.program(), candidate.root_program());
    assert_eq!(candidate_failure.class(), RefusalClass::RuntimeFault);
    assert!(candidate.execution().usage().cpu_fuel > 0);
    assert_eq!(candidate.execution().usage().storage_write_bytes, 2);
    assert_eq!(storage, Storage::new());

    let child = ProgramId::new([153; 32]).unwrap_or_else(|error| panic!("child: {error}"));
    let requested = CapabilitySet::new([Capability::StorageWrite])
        .unwrap_or_else(|error| panic!("requested: {error}"));
    let root_wasm = v1_forwarder_configured(child, &requested.canonical_encoding(), false);
    let capabilities = CapabilitySet::new([
        Capability::Call { program: child },
        Capability::StorageWrite,
    ])
    .unwrap_or_else(|error| panic!("capabilities: {error}"));
    let (nested, storage) = run_v1_graph(
        &root_wasm,
        &[(child, storage_start_candidate(false))],
        declared(100_000, 131_072, 0, 2, 2, 0, 0),
        capabilities.clone(),
        153,
    );
    let BudgetedV1ActivityOutcome::Failure(nested) =
        nested.unwrap_or_else(|error| panic!("nested start fault: {error}"))
    else {
        panic!("nested start fault was not receipt ready");
    };
    let nested_failure = nested
        .program_failure()
        .unwrap_or_else(|| panic!("nested start fault had no failure"));
    assert_eq!(nested_failure.program(), child);
    assert_eq!(nested_failure.class(), RefusalClass::RuntimeFault);
    assert!(nested.usage().cpu_fuel > 0);
    assert_eq!(nested.usage().storage_write_bytes, 2);
    assert_eq!(nested.call_graph().edges().len(), 1);
    assert_eq!(storage, Storage::new());

    let (exhausted, storage) = run_v1_graph(
        &root_wasm,
        &[(child, storage_start_candidate(true))],
        declared(5_000, 131_072, 0, 2, 2, 0, 0),
        capabilities,
        154,
    );
    let BudgetedV1ActivityOutcome::Resource(exhausted) =
        exhausted.unwrap_or_else(|error| panic!("nested start exhaustion: {error}"))
    else {
        panic!("nested start exhaustion was not a resource outcome");
    };
    let root_program =
        ProgramId::new([203; 32]).unwrap_or_else(|error| panic!("root program: {error}"));
    let root_charge_sites = frozen_defined_function_charge_sites(&root_wasm, 1);
    let child_wasm = storage_start_candidate(true);
    let child_charge_sites = frozen_defined_function_charge_sites(&child_wasm, 0);
    assert_eq!(root_charge_sites, [9]);
    assert_eq!(child_charge_sites, [7, 2]);
    let prefix = storage_write_call_access_charge(root_program, principal(202), child)
        + root_charge_sites[0]
        + layerx_programs_runtime::call_admission_fuel(0)
        + child_charge_sites[0];
    let (usage, attempted) =
        repeated_charge_exhaustion(5_000, prefix, child_charge_sites[1]);
    assert_eq!(
        exhausted.refusal(),
        BudgetMeterRefusal::BudgetExceeded {
            resource: BudgetResourceKind::Cpu,
            limit: 5_000,
            attempted,
        }
    );
    assert_eq!(exhausted.usage().cpu_fuel, usage);
    assert_eq!(exhausted.usage().storage_write_bytes, 2);
    assert_eq!(exhausted.call_graph().edges().len(), 1);
    assert_eq!(storage, Storage::new());
}

#[test]
fn start_time_call_at_exact_remaining_fuel_is_a_resource_outcome() {
    let child = ProgramId::new([186; 32]).unwrap_or_else(|error| panic!("child: {error}"));
    let root_wasm = start_forwarder(child);
    let child_wasm = candidate_with_entry(&[0x41, 0, 0x0b]);
    let capabilities = CapabilitySet::new([Capability::Call { program: child }])
        .unwrap_or_else(|error| panic!("capabilities: {error}"));
    let start_charge_sites = frozen_defined_function_charge_sites(&root_wasm, 0);
    assert_eq!(start_charge_sites, [10]);
    let cpu_fuel = access_charge([child])
        + start_charge_sites[0]
        + layerx_programs_runtime::call_admission_fuel(0);
    let attempted = cpu_fuel + 1;
    let (outcome, storage) = run_v1_graph(
        &root_wasm,
        &[(child, child_wasm)],
        declared(cpu_fuel, 131_072, 0, 0, 2, 0, 0),
        capabilities,
        186,
    );
    let BudgetedV1ActivityOutcome::Resource(resource) =
        outcome.unwrap_or_else(|error| panic!("exact-remaining start call: {error}"))
    else {
        panic!("exact-remaining start call was not a resource outcome");
    };
    assert_eq!(storage, Storage::new());
    assert_eq!(
        resource.refusal(),
        BudgetMeterRefusal::BudgetExceeded {
            resource: BudgetResourceKind::Cpu,
            limit: cpu_fuel,
            attempted,
        }
    );
    assert_eq!(resource.call_graph().edges()[0].callee(), child);
    assert!(resource.usage().cpu_fuel <= cpu_fuel);
}

#[test]
fn nested_start_time_call_keeps_the_full_failed_graph_on_exact_fuel() {
    let middle = ProgramId::new([187; 32]).unwrap_or_else(|error| panic!("middle: {error}"));
    let leaf = ProgramId::new([188; 32]).unwrap_or_else(|error| panic!("leaf: {error}"));
    let requested = CapabilitySet::new([Capability::Call { program: leaf }])
        .unwrap_or_else(|error| panic!("requested: {error}"));
    let root_wasm = start_forwarder_configured(middle, &requested.canonical_encoding());
    let middle_wasm = start_forwarder(leaf);
    let leaf_wasm = candidate_with_entry(&[0x41, 0, 0x0b]);
    let capabilities = CapabilitySet::new([
        Capability::Call { program: middle },
        Capability::Call { program: leaf },
    ])
    .unwrap_or_else(|error| panic!("capabilities: {error}"));
    let children = [(middle, middle_wasm), (leaf, leaf_wasm)];
    let root_start_sites = frozen_defined_function_charge_sites(&root_wasm, 0);
    let middle_start_sites = frozen_defined_function_charge_sites(&children[0].1, 0);
    assert_eq!(root_start_sites, [10]);
    assert_eq!(middle_start_sites, [10]);
    let exact_fuel = access_charge([middle, leaf])
        + root_start_sites[0]
        + layerx_programs_runtime::call_admission_fuel(0)
        + middle_start_sites[0]
        + layerx_programs_runtime::call_admission_fuel(0);
    let attempted = exact_fuel + 1;
    let (outcome, storage) = run_v1_graph(
        &root_wasm,
        &children,
        declared(exact_fuel, 196_608, 0, 0, 3, 0, 0),
        capabilities,
        190,
    );
    let BudgetedV1ActivityOutcome::Resource(resource) =
        outcome.unwrap_or_else(|error| panic!("nested exact-fuel start call: {error}"))
    else {
        panic!("nested exact-fuel start call was not a resource outcome");
    };
    assert_eq!(storage, Storage::new());
    assert_eq!(
        resource.refusal(),
        BudgetMeterRefusal::BudgetExceeded {
            resource: BudgetResourceKind::Cpu,
            limit: exact_fuel,
            attempted,
        }
    );
    assert_eq!(resource.call_graph().edges()[0].callee(), middle);
    assert_eq!(resource.call_graph().edges()[1].callee(), leaf);
}

#[test]
fn one_declared_cpu_ceiling_is_shared_by_the_real_v1_call_graph() {
    let payer = principal(27);
    let root_program = ProgramId::new([28; 32]).unwrap_or_else(|error| panic!("root: {error}"));
    let child_program = ProgramId::new([29; 32]).unwrap_or_else(|error| panic!("child: {error}"));
    let root_wasm = v1_forwarder(child_program);
    let child_wasm = candidate_with_entry(&[0x41, 0, 0x0b]);
    let run = |cpu_fuel: u64, binding_byte: u8| {
        let executor = Executor::declared();
        let binding = ActivityBudgetBinding::new([binding_byte; 32])
            .unwrap_or_else(|error| panic!("binding: {error}"));
        let token = executor
            .admit_activity_budget_for_qualification(
                declared(cpu_fuel, 131_072, 0, 0, 4, 0, 0),
                payer,
                binding,
                u128::MAX,
            )
            .unwrap_or_else(|error| panic!("admission: {error}"));
        let engine = WasmEngine::declared().unwrap_or_else(|error| panic!("engine: {error}"));
        let root = engine
            .validate(&root_wasm)
            .unwrap_or_else(|error| panic!("root validation: {error}"));
        let child = engine
            .validate(&child_wasm)
            .unwrap_or_else(|error| panic!("child validation: {error}"));
        let mut catalog = ProgramCatalog::new();
        catalog.insert(child_program, child);
        let capabilities = CapabilitySet::new([Capability::Call {
            program: child_program,
        }])
        .unwrap_or_else(|error| panic!("capability: {error}"));
        let request = BudgetedAuthorizedExecutionRequest::new(
            AuthorizedExecutionRequest {
                module: &root,
                program: root_program,
                authorization: AuthorizationContext::new(payer, capabilities),
                receipts: &NoReceipts,
                entrypoint: CALL_ENTRY_EXPORT,
                calldata: &[],
                composition: CompositionContext::catalog(
                    catalog,
                    layerx_programs_runtime::CompositionRules::declared(),
                ),
                response_capacity: 0,
            },
            token,
            payer,
            binding,
        );
        let mut storage = Storage::new();
        executor
            .execute_authorized_budgeted_for_qualification(&mut storage, request)
            .unwrap_or_else(|error| panic!("execution: {error}"))
    };
    let high = run(100_000, 30);
    let BudgetedV1ActivityOutcome::Success(high_record) = high else {
        panic!("high ceiling did not succeed");
    };
    let exact_cpu = high_record.execution.usage.cpu_fuel;
    let exact = run(exact_cpu, 31);
    let BudgetedV1ActivityOutcome::Success(exact_record) = exact else {
        panic!("exact aggregate ceiling did not succeed");
    };
    assert_eq!(exact_record.execution.usage, high_record.execution.usage);
    assert_eq!(
        exact_record.execution.canonical_evidence(),
        high_record.execution.canonical_evidence()
    );
    assert_eq!(exact_record.call_graph.edges().len(), 1);

    let short = run(exact_cpu - 1, 32);
    let BudgetedV1ActivityOutcome::Resource(failure) = short else {
        panic!("one-short aggregate ceiling did not produce a resource outcome");
    };
    assert_eq!(
        failure.refusal(),
        BudgetMeterRefusal::BudgetExceeded {
            resource: BudgetResourceKind::Cpu,
            limit: exact_cpu - 1,
            attempted: exact_cpu,
        }
    );
    assert_eq!(failure.call_graph().edges().len(), 1);
    assert!(failure.usage().cpu_fuel < exact_cpu);
}

#[test]
fn sufficient_declared_headroom_never_changes_candidate_usage_or_evidence() {
    let payer = principal(33);
    let program = ProgramId::new([34; 32]).unwrap_or_else(|error| panic!("program: {error}"));
    let wasm = candidate_with_entry(&[0x41, 0, 0x0b]);
    for length in 0_u8..64 {
        let calldata = vec![length; usize::from(length)];
        let run = |budget: DeclaredBudget, binding_byte: u8| {
            let executor = Executor::declared();
            let binding = ActivityBudgetBinding::new([binding_byte; 32])
                .unwrap_or_else(|error| panic!("binding: {error}"));
            let token = executor
                .admit_activity_budget_for_qualification(budget, payer, binding, u128::MAX)
                .unwrap_or_else(|error| panic!("admission: {error}"));
            let engine = WasmEngine::declared().unwrap_or_else(|error| panic!("engine: {error}"));
            let module = engine
                .validate_candidate_v2(&wasm)
                .unwrap_or_else(|error| panic!("validation: {error}"));
            let request = BudgetedAuthorizedExecutionRequest::new(
                AuthorizedExecutionRequest {
                    module: &module,
                    program,
                    authorization: AuthorizationContext::new(payer, CapabilitySet::empty()),
                    receipts: &NoReceipts,
                    entrypoint: CALL_ENTRY_EXPORT,
                    calldata: &calldata,
                    composition: CompositionContext::isolated(),
                    response_capacity: 0,
                },
                token,
                payer,
                binding,
            );
            executor
                .execute_authorized_candidate_budgeted_for_qualification(
                    &mut Storage::new(),
                    request,
                )
                .unwrap_or_else(|error| panic!("candidate: {error}"))
        };
        let low = run(
            declared(10_000, 65_536, 0, 0, if length == 0 { 1 } else { 2 }, 0, 0),
            length.saturating_add(1),
        );
        let high = run(
            DeclaredBudget::protocol_maximum(),
            length.saturating_add(65),
        );
        assert_eq!(low.execution().usage(), high.execution().usage());
        assert_eq!(low.outcome(), high.outcome());
        assert_eq!(low.call_graph(), high.call_graph());
        assert_eq!(low.canonical_evidence(), high.canonical_evidence());
        assert_eq!(low.receipt_projection(), high.receipt_projection());
    }
}

#[test]
#[allow(clippy::too_many_lines)]
fn sufficient_seven_dimension_headroom_preserves_nested_success_refusal_and_fault() {
    let child = ProgramId::new([164; 32]).unwrap_or_else(|error| panic!("child: {error}"));
    let root_wasm = v1_forwarder(child);
    let capabilities = CapabilitySet::new([Capability::Call { program: child }])
        .unwrap_or_else(|error| panic!("capabilities: {error}"));
    let low = declared(100_000, 131_072, 0, 0, 2, 0, 0);
    let high = DeclaredBudget::protocol_maximum();
    let success_child = candidate_with_entry(&[0x41, 0, 0x0b]);

    let (v1_low, storage_low) = run_v1_graph(
        &root_wasm,
        &[(child, success_child.clone())],
        low,
        capabilities.clone(),
        164,
    );
    let (v1_high, storage_high) = run_v1_graph(
        &root_wasm,
        &[(child, success_child.clone())],
        high,
        capabilities.clone(),
        165,
    );
    assert_eq!(v1_low, v1_high);
    assert_eq!(storage_low, storage_high);
    let BudgetedV1ActivityOutcome::Success(v1) =
        v1_low.unwrap_or_else(|error| panic!("v1 nested success: {error}"))
    else {
        panic!("v1 nested success was not successful");
    };
    assert_eq!(v1.call_graph.edges().len(), 1);

    let (candidate_low, storage_low) = run_candidate_graph(
        &root_wasm,
        &[(child, success_child)],
        low,
        capabilities.clone(),
        166,
    );
    let (candidate_high, storage_high) = run_candidate_graph(
        &root_wasm,
        &[(child, candidate_with_entry(&[0x41, 0, 0x0b]))],
        high,
        capabilities.clone(),
        167,
    );
    assert_eq!(candidate_low, candidate_high);
    assert_eq!(storage_low, storage_high);

    for (index, child_wasm, expected_class) in [
        (
            0_u8,
            candidate_with_entry(&[0x41, 0x7f, 0x0b]),
            RefusalClass::Legacy,
        ),
        (
            1,
            candidate_with_entry(&[0x00, 0x0b]),
            RefusalClass::RuntimeFault,
        ),
    ] {
        let (nested_low, storage_low) = run_candidate_graph(
            &root_wasm,
            &[(child, child_wasm.clone())],
            low,
            capabilities.clone(),
            168 + index * 4,
        );
        let (nested_high, storage_high) = run_candidate_graph(
            &root_wasm,
            &[(child, child_wasm.clone())],
            high,
            capabilities.clone(),
            169 + index * 4,
        );
        assert_eq!(nested_low, nested_high);
        assert_eq!(storage_low, storage_high);
        let nested = nested_low.unwrap_or_else(|error| panic!("nested failure: {error}"));
        let failure = nested
            .failure()
            .unwrap_or_else(|| panic!("nested failure was not receipt ready"));
        assert_eq!(failure.program(), child);
        assert_eq!(failure.class(), expected_class);
        assert_eq!(nested.call_graph().edges().len(), 1);

        let (root_low, storage_low) = run_isolated_candidate(
            &child_wasm,
            declared(100_000, 65_536, 0, 0, 1, 0, 0),
            CapabilitySet::empty(),
            Storage::new(),
            0,
            &[],
            170 + index * 4,
        );
        let (root_high, storage_high) = run_isolated_candidate(
            &child_wasm,
            high,
            CapabilitySet::empty(),
            Storage::new(),
            0,
            &[],
            171 + index * 4,
        );
        assert_eq!(root_low, root_high);
        assert_eq!(storage_low, storage_high);
        let root = root_low.unwrap_or_else(|error| panic!("root failure: {error}"));
        let failure = root
            .failure()
            .unwrap_or_else(|| panic!("root failure was not receipt ready"));
        assert_eq!(failure.program(), root.root_program());
        assert_eq!(failure.class(), expected_class);
        assert!(root.call_graph().edges().is_empty());
    }
}

#[test]
#[allow(clippy::too_many_lines)]
fn sibling_calls_share_storage_and_output_ceilings_and_rollback_atomically() {
    let payer = principal(130);
    let root_program = ProgramId::new([131; 32]).unwrap_or_else(|error| panic!("root: {error}"));
    let child_program = ProgramId::new([132; 32]).unwrap_or_else(|error| panic!("child: {error}"));
    let child_grants = CapabilitySet::new([Capability::StorageWrite])
        .unwrap_or_else(|error| panic!("child grants: {error}"));
    let root_wasm = v1_fanout(child_program, &child_grants.canonical_encoding());
    let child_wasm = storage_writer_candidate();
    let run = |storage_limit: u64, output_limit: u32, binding_byte: u8| {
        let executor = Executor::declared();
        let binding = ActivityBudgetBinding::new([binding_byte; 32])
            .unwrap_or_else(|error| panic!("binding: {error}"));
        let token = executor
            .admit_activity_budget_for_qualification(
                declared(200_000, 131_072, 0, storage_limit, output_limit, 0, 0),
                payer,
                binding,
                u128::MAX,
            )
            .unwrap_or_else(|error| panic!("admission: {error}"));
        let engine = WasmEngine::declared().unwrap_or_else(|error| panic!("engine: {error}"));
        let root = engine
            .validate(&root_wasm)
            .unwrap_or_else(|error| panic!("root validation: {error}"));
        let child = engine
            .validate(&child_wasm)
            .unwrap_or_else(|error| panic!("child validation: {error}"));
        let mut catalog = ProgramCatalog::new();
        catalog.insert(child_program, child);
        let capabilities = CapabilitySet::new([
            Capability::Call {
                program: child_program,
            },
            Capability::StorageWrite,
        ])
        .unwrap_or_else(|error| panic!("root grants: {error}"));
        let request = BudgetedAuthorizedExecutionRequest::new(
            AuthorizedExecutionRequest {
                module: &root,
                program: root_program,
                authorization: AuthorizationContext::new(payer, capabilities),
                receipts: &NoReceipts,
                entrypoint: CALL_ENTRY_EXPORT,
                calldata: &[],
                composition: CompositionContext::catalog(
                    catalog,
                    layerx_programs_runtime::CompositionRules::declared(),
                ),
                response_capacity: 0,
            },
            token,
            payer,
            binding,
        );
        let mut storage = Storage::new();
        let result = executor
            .execute_authorized_budgeted_for_qualification(&mut storage, request)
            .unwrap_or_else(|error| panic!("execution: {error}"));
        (result, storage)
    };

    let (exact, mut committed) = run(14, 3, 133);
    let BudgetedV1ActivityOutcome::Success(record) = exact else {
        panic!("exact sibling ceilings did not succeed");
    };
    assert_eq!(record.execution.usage.storage_write_bytes, 14);
    assert_eq!(record.execution.usage.output_values, 3);
    assert_eq!(record.execution.usage.memory_bytes, 131_072);
    assert_eq!(record.call_graph.edges().len(), 2);
    let namespace = StorageNamespace::principal(child_program, payer);
    assert_eq!(
        committed.transaction(namespace).read(b"key"),
        Ok(Some(b"data".to_vec()))
    );

    let (storage_short, mut rolled_back) = run(13, 3, 134);
    let BudgetedV1ActivityOutcome::Resource(storage_failure) = storage_short else {
        panic!("one-short storage ceiling did not refuse");
    };
    assert_eq!(
        storage_failure.refusal(),
        BudgetMeterRefusal::BudgetExceeded {
            resource: BudgetResourceKind::StorageWrite,
            limit: 13,
            attempted: 14,
        }
    );
    assert_eq!(storage_failure.usage().storage_write_bytes, 7);
    assert_eq!(storage_failure.call_graph().edges().len(), 2);
    assert_eq!(rolled_back.transaction(namespace).read(b"key"), Ok(None));

    let (output_short, mut rolled_back) = run(14, 2, 135);
    let BudgetedV1ActivityOutcome::Resource(output_failure) = output_short else {
        panic!("one-short output ceiling did not refuse");
    };
    assert_eq!(
        output_failure.refusal(),
        BudgetMeterRefusal::BudgetExceeded {
            resource: BudgetResourceKind::Output,
            limit: 2,
            attempted: 3,
        }
    );
    assert_eq!(output_failure.usage().output_values, 1);
    assert_eq!(output_failure.usage().storage_write_bytes, 7);
    assert_eq!(output_failure.call_graph().edges().len(), 2);
    assert_eq!(rolled_back.transaction(namespace).read(b"key"), Ok(None));
}

#[test]
#[allow(clippy::too_many_lines)]
fn nested_memory_and_table_limits_are_graph_wide_for_v1_and_candidate() {
    let child = ProgramId::new([140; 32]).unwrap_or_else(|error| panic!("child: {error}"));
    let root_wasm = v1_forwarder_configured(child, &[0, 0], true);
    let child_wasm = table_candidate();
    let capabilities = CapabilitySet::new([Capability::Call { program: child }])
        .unwrap_or_else(|error| panic!("capabilities: {error}"));

    let exact = declared(200_000, 131_072, 0, 0, 2, 0, 2);
    let (v1, storage) = run_v1_graph(
        &root_wasm,
        &[(child, child_wasm.clone())],
        exact,
        capabilities.clone(),
        141,
    );
    let BudgetedV1ActivityOutcome::Success(v1) =
        v1.unwrap_or_else(|error| panic!("v1 exact: {error}"))
    else {
        panic!("v1 exact aggregate resources did not succeed");
    };
    assert_eq!(v1.execution.usage.memory_bytes, 131_072);
    assert_eq!(v1.call_graph.edges().len(), 1);
    assert_eq!(storage, Storage::new());

    let (candidate, storage) = run_candidate_graph(
        &root_wasm,
        &[(child, child_wasm.clone())],
        exact,
        capabilities.clone(),
        142,
    );
    let candidate = candidate.unwrap_or_else(|error| panic!("candidate exact: {error}"));
    assert!(matches!(
        candidate.outcome(),
        CandidateActivityOutcome::Success { .. }
    ));
    assert_eq!(candidate.execution().usage().memory_bytes, 131_072);
    assert_eq!(candidate.call_graph().edges().len(), 1);
    assert_eq!(storage, Storage::new());

    let memory_short = declared(200_000, 131_071, 0, 0, 2, 0, 2);
    let (v1, storage) = run_v1_graph(
        &root_wasm,
        &[(child, child_wasm.clone())],
        memory_short,
        capabilities.clone(),
        143,
    );
    let BudgetedV1ActivityOutcome::Resource(v1) =
        v1.unwrap_or_else(|error| panic!("v1 memory short: {error}"))
    else {
        panic!("v1 memory-short execution did not refuse");
    };
    assert_eq!(
        v1.refusal(),
        BudgetMeterRefusal::BudgetExceeded {
            resource: BudgetResourceKind::Memory,
            limit: 131_071,
            attempted: 131_072,
        }
    );
    assert_eq!(v1.usage().memory_bytes, 65_536);
    assert_eq!(v1.call_graph().edges().len(), 1);
    assert_eq!(storage, Storage::new());

    let (candidate, storage) = run_candidate_graph(
        &root_wasm,
        &[(child, child_wasm.clone())],
        memory_short,
        capabilities.clone(),
        144,
    );
    let candidate = candidate.unwrap_or_else(|error| panic!("candidate memory short: {error}"));
    assert_eq!(
        candidate.resource_refusal(),
        Some(&BudgetMeterRefusal::BudgetExceeded {
            resource: BudgetResourceKind::Memory,
            limit: 131_071,
            attempted: 131_072,
        })
    );
    assert_eq!(candidate.execution().usage().memory_bytes, 65_536);
    assert_eq!(candidate.call_graph().edges().len(), 1);
    assert_eq!(storage, Storage::new());

    let table_short = declared(200_000, 131_072, 0, 0, 2, 0, 1);
    let (v1, storage) = run_v1_graph(
        &root_wasm,
        &[(child, child_wasm.clone())],
        table_short,
        capabilities.clone(),
        145,
    );
    let BudgetedV1ActivityOutcome::Resource(v1) =
        v1.unwrap_or_else(|error| panic!("v1 table short: {error}"))
    else {
        panic!("v1 table-short execution did not refuse");
    };
    assert_eq!(
        v1.refusal(),
        BudgetMeterRefusal::BudgetExceeded {
            resource: BudgetResourceKind::Table,
            limit: 1,
            attempted: 2,
        }
    );
    assert_eq!(v1.call_graph().edges().len(), 1);
    assert_eq!(storage, Storage::new());

    let (candidate, storage) = run_candidate_graph(
        &root_wasm,
        &[(child, child_wasm)],
        table_short,
        capabilities,
        146,
    );
    let candidate = candidate.unwrap_or_else(|error| panic!("candidate table short: {error}"));
    assert_eq!(
        candidate.resource_refusal(),
        Some(&BudgetMeterRefusal::BudgetExceeded {
            resource: BudgetResourceKind::Table,
            limit: 1,
            attempted: 2,
        })
    );
    assert_eq!(candidate.call_graph().edges().len(), 1);
    assert_eq!(storage, Storage::new());
}

#[test]
fn depth_three_memory_peak_is_exact_and_one_short_refuses_at_the_leaf() {
    let middle = ProgramId::new([147; 32]).unwrap_or_else(|error| panic!("middle: {error}"));
    let leaf = ProgramId::new([148; 32]).unwrap_or_else(|error| panic!("leaf: {error}"));
    let requested = CapabilitySet::new([Capability::Call { program: leaf }])
        .unwrap_or_else(|error| panic!("requested: {error}"));
    let root_wasm = v1_forwarder_configured(middle, &requested.canonical_encoding(), false);
    let middle_wasm = v1_forwarder(leaf);
    let leaf_wasm = candidate_with_entry(&[0x41, 0, 0x0b]);
    let capabilities = CapabilitySet::new([
        Capability::Call { program: middle },
        Capability::Call { program: leaf },
    ])
    .unwrap_or_else(|error| panic!("capabilities: {error}"));
    let children = [(middle, middle_wasm), (leaf, leaf_wasm)];

    let (exact, storage) = run_v1_graph(
        &root_wasm,
        &children,
        declared(300_000, 196_608, 0, 0, 3, 0, 0),
        capabilities.clone(),
        147,
    );
    let BudgetedV1ActivityOutcome::Success(exact) =
        exact.unwrap_or_else(|error| panic!("exact: {error}"))
    else {
        panic!("depth-three exact memory did not succeed");
    };
    assert_eq!(exact.execution.usage.memory_bytes, 196_608);
    assert_eq!(exact.call_graph.edges().len(), 2);
    assert_eq!(storage, Storage::new());

    let (short, storage) = run_v1_graph(
        &root_wasm,
        &children,
        declared(300_000, 196_607, 0, 0, 3, 0, 0),
        capabilities,
        148,
    );
    let BudgetedV1ActivityOutcome::Resource(short) =
        short.unwrap_or_else(|error| panic!("short: {error}"))
    else {
        panic!("depth-three one-short memory did not refuse");
    };
    assert_eq!(
        short.refusal(),
        BudgetMeterRefusal::BudgetExceeded {
            resource: BudgetResourceKind::Memory,
            limit: 196_607,
            attempted: 196_608,
        }
    );
    assert_eq!(short.usage().memory_bytes, 131_072);
    assert_eq!(short.call_graph().edges().len(), 2);
    assert_eq!(storage, Storage::new());
}

#[test]
fn sequential_siblings_reuse_live_memory_and_table_capacity() {
    let child = ProgramId::new([149; 32]).unwrap_or_else(|error| panic!("child: {error}"));
    let root_wasm = v1_fanout_configured(child, &[0, 0], true);
    let child_wasm = table_candidate();
    let capabilities = CapabilitySet::new([Capability::Call { program: child }])
        .unwrap_or_else(|error| panic!("capabilities: {error}"));
    let exact = declared(300_000, 131_072, 0, 0, 3, 0, 2);

    let (v1, storage) = run_v1_graph(
        &root_wasm,
        &[(child, child_wasm.clone())],
        exact,
        capabilities.clone(),
        149,
    );
    let BudgetedV1ActivityOutcome::Success(v1) =
        v1.unwrap_or_else(|error| panic!("v1 siblings: {error}"))
    else {
        panic!("v1 sibling capacity was accumulated instead of reused");
    };
    assert_eq!(v1.execution.usage.memory_bytes, 131_072);
    assert_eq!(v1.execution.usage.output_values, 3);
    assert_eq!(v1.call_graph.edges().len(), 2);
    assert_eq!(storage, Storage::new());

    let (candidate, storage) =
        run_candidate_graph(&root_wasm, &[(child, child_wasm)], exact, capabilities, 150);
    let candidate = candidate.unwrap_or_else(|error| panic!("candidate siblings: {error}"));
    assert!(matches!(
        candidate.outcome(),
        CandidateActivityOutcome::Success { .. }
    ));
    assert_eq!(candidate.execution().usage().memory_bytes, 131_072);
    assert_eq!(candidate.execution().usage().output_values, 3);
    assert_eq!(candidate.call_graph().edges().len(), 2);
    assert_eq!(storage, Storage::new());
}

#[test]
fn nested_response_bytes_use_one_graph_wide_ceiling_without_double_adoption() {
    let child = ProgramId::new([176; 32]).unwrap_or_else(|error| panic!("child: {error}"));
    let payload = b"abcde";
    let root_wasm = response_forwarder(child, 5);
    let child_wasm = response_candidate(payload, false);
    let capabilities = CapabilitySet::new([Capability::Call { program: child }])
        .unwrap_or_else(|error| panic!("capabilities: {error}"));

    let (exact, storage) = run_candidate_graph(
        &root_wasm,
        &[(child, child_wasm.clone())],
        declared(200_000, 131_072, 0, 0, 2, 10, 0),
        capabilities.clone(),
        176,
    );
    let exact = exact.unwrap_or_else(|error| panic!("exact response bytes: {error}"));
    assert!(matches!(
        exact.outcome(),
        CandidateActivityOutcome::Success { .. }
    ));
    assert_eq!(exact.execution().usage().output_bytes, 10);
    assert_eq!(exact.call_graph().edges().len(), 1);
    assert_eq!(storage, Storage::new());

    let (short, storage) = run_candidate_graph(
        &root_wasm,
        &[(child, child_wasm)],
        declared(200_000, 131_072, 0, 0, 2, 9, 0),
        capabilities,
        177,
    );
    let short = short.unwrap_or_else(|error| panic!("short response bytes: {error}"));
    assert_eq!(
        short.resource_refusal(),
        Some(&BudgetMeterRefusal::BudgetExceeded {
            resource: BudgetResourceKind::OutputBytes,
            limit: 9,
            attempted: 10,
        })
    );
    assert_eq!(short.execution().usage().output_bytes, 5);
    assert_eq!(short.call_graph().edges().len(), 1);
    assert_eq!(storage, Storage::new());
}

#[test]
fn budgeted_candidate_covers_memory_table_values_and_response_bytes() {
    let cases = [
        (
            table_candidate(),
            declared(10_000, 65_536, 0, 0, 2, 0, 0),
            0,
            &[][..],
            BudgetMeterRefusal::BudgetExceeded {
                resource: BudgetResourceKind::Table,
                limit: 0,
                attempted: 1,
            },
        ),
        (
            candidate_with_entry(&[0x41, 0, 0x0b]),
            declared(10_000, 65_536, 0, 0, 1, 0, 0),
            0,
            &b"x"[..],
            BudgetMeterRefusal::BudgetExceeded {
                resource: BudgetResourceKind::Output,
                limit: 1,
                attempted: 2,
            },
        ),
        (
            response_candidate(b"abcde", false),
            declared(10_000, 65_536, 0, 0, 2, 4, 0),
            5,
            &[][..],
            BudgetMeterRefusal::BudgetExceeded {
                resource: BudgetResourceKind::OutputBytes,
                limit: 4,
                attempted: 5,
            },
        ),
    ];
    for (index, (wasm, budget, response_capacity, calldata, expected)) in
        cases.into_iter().enumerate()
    {
        let (result, storage) = run_isolated_candidate(
            &wasm,
            budget,
            CapabilitySet::empty(),
            Storage::new(),
            response_capacity,
            calldata,
            u8::try_from(210 + index).unwrap_or(u8::MAX),
        );
        let record = result.unwrap_or_else(|error| panic!("case {index}: {error}"));
        assert_eq!(record.resource_refusal(), Some(&expected));
        assert!(record.response().is_none());
        assert!(record.failure().is_none());
        assert!(record.effects().is_none());
        assert!(record.call_graph().edges().is_empty());
        assert_eq!(storage, Storage::new());
    }
}

#[test]
fn legacy_unbudgeted_table_refusal_remains_memory_taxonomy() {
    let budget = ResourceBudget::new_complete(10_000, 65_536, 0, 0, 2, 0, 0);
    let executor = Executor::new(budget, FeeSchedule::declared());
    let payer = principal(220);
    let engine = WasmEngine::declared().unwrap_or_else(|error| panic!("engine: {error}"));
    let module = engine
        .validate_candidate_v2(&table_candidate())
        .unwrap_or_else(|error| panic!("validation: {error}"));
    let result = executor.execute_authorized_candidate(
        &mut Storage::new(),
        AuthorizedExecutionRequest {
            module: &module,
            program: ProgramId::new([221; 32]).unwrap_or_else(|error| panic!("program: {error}")),
            authorization: AuthorizationContext::new(payer, CapabilitySet::empty()),
            receipts: &NoReceipts,
            entrypoint: CALL_ENTRY_EXPORT,
            calldata: &[],
            composition: CompositionContext::isolated(),
            response_capacity: 0,
        },
    );
    assert_eq!(
        result,
        Err(ExecutionError::Resource(MeterRefusal::BudgetExceeded {
            resource: ResourceKind::Memory,
            limit: 0,
            attempted: 1,
        }))
    );
}

#[test]
fn legacy_resource_taxonomy_remains_exhaustive_and_budget_conversion_is_exact() {
    fn frozen_tag(resource: ResourceKind) -> u8 {
        match resource {
            ResourceKind::Cpu => 0,
            ResourceKind::Memory => 1,
            ResourceKind::StorageRead => 2,
            ResourceKind::StorageWrite => 3,
            ResourceKind::Output => 4,
            ResourceKind::OutputBytes => 5,
            ResourceKind::StorageOccupancy => {
                panic!("occupancy is not part of the frozen activity-budget taxonomy")
            }
        }
    }

    let resources = [
        (ResourceKind::Cpu, BudgetResourceKind::Cpu),
        (ResourceKind::Memory, BudgetResourceKind::Memory),
        (ResourceKind::StorageRead, BudgetResourceKind::StorageRead),
        (ResourceKind::StorageWrite, BudgetResourceKind::StorageWrite),
        (ResourceKind::Output, BudgetResourceKind::Output),
        (ResourceKind::OutputBytes, BudgetResourceKind::OutputBytes),
    ];
    for (index, (legacy, budget)) in resources.into_iter().enumerate() {
        assert_eq!(frozen_tag(legacy), u8::try_from(index).unwrap_or(u8::MAX));
        assert_eq!(
            BudgetMeterRefusal::try_from(MeterRefusal::BudgetExceeded {
                resource: legacy,
                limit: 7,
                attempted: 8,
            }),
            Ok(BudgetMeterRefusal::BudgetExceeded {
                resource: budget,
                limit: 7,
                attempted: 8,
            })
        );
        assert_eq!(
            BudgetMeterRefusal::try_from(MeterRefusal::CounterOverflow { resource: legacy }),
            Ok(BudgetMeterRefusal::CounterOverflow { resource: budget })
        );
    }
    assert_eq!(
        BudgetMeterRefusal::try_from(MeterRefusal::FeeOverflow),
        Err(MeterRefusal::FeeOverflow)
    );
    assert_eq!(
        BudgetMeterRefusal::try_from(MeterRefusal::CounterOverflow {
            resource: ResourceKind::StorageOccupancy,
        }),
        Err(MeterRefusal::CounterOverflow {
            resource: ResourceKind::StorageOccupancy,
        })
    );
}

#[test]
fn budgeted_storage_read_is_exact_unbilled_on_rejection_and_fee_priced_on_success() {
    let payer = principal(200);
    let program = ProgramId::new([201; 32]).unwrap_or_else(|error| panic!("program: {error}"));
    let namespace = StorageNamespace::principal(program, payer);
    let mut seeded = Storage::new();
    {
        let mut transaction = seeded.transaction(namespace);
        transaction
            .write(b"key", b"data")
            .unwrap_or_else(|error| panic!("seed: {error}"));
        let _ = transaction.commit();
    }
    let capabilities = CapabilitySet::new([Capability::StorageRead])
        .unwrap_or_else(|error| panic!("capability: {error}"));
    let (exact, exact_storage) = run_isolated_candidate(
        &storage_reader_candidate(),
        declared(10_000, 65_536, 7, 0, 2, 0, 0),
        capabilities.clone(),
        seeded.clone(),
        0,
        &[],
        222,
    );
    let exact = exact.unwrap_or_else(|error| panic!("exact read: {error}"));
    assert_eq!(exact.execution().usage().storage_read_bytes, 7);
    let usage = exact.execution().usage();
    assert_eq!(
        usage.fee_units,
        u128::from(usage.cpu_fuel)
            + u128::from(usage.memory_bytes)
            + u128::from(usage.storage_read_bytes) * 2
            + u128::from(usage.storage_write_bytes) * 4
            + u128::from(usage.output_values)
            + u128::from(usage.output_bytes)
    );
    assert_eq!(exact_storage, seeded);

    let (short, short_storage) = run_isolated_candidate(
        &storage_reader_candidate(),
        declared(10_000, 65_536, 6, 0, 2, 0, 0),
        capabilities,
        seeded.clone(),
        0,
        &[],
        223,
    );
    let short = short.unwrap_or_else(|error| panic!("short read: {error}"));
    assert_eq!(
        short.resource_refusal(),
        Some(&BudgetMeterRefusal::BudgetExceeded {
            resource: BudgetResourceKind::StorageRead,
            limit: 6,
            attempted: 7,
        })
    );
    assert_eq!(short.execution().usage().storage_read_bytes, 0);
    assert_eq!(short_storage, seeded);
}

#[test]
fn budgeted_resource_exhaustion_supersedes_a_published_candidate_response() {
    let wasm = response_candidate(b"x", true);
    let charge_sites = frozen_defined_function_charge_sites(&wasm, 1);
    assert_eq!(charge_sites, [6, 2]);
    let (usage, attempted) =
        repeated_charge_exhaustion(100, access_charge([]) + charge_sites[0], charge_sites[1]);
    let (result, storage) = run_isolated_candidate(
        &wasm,
        declared(100, 65_536, 0, 0, 2, 1, 0),
        CapabilitySet::empty(),
        Storage::new(),
        1,
        &[],
        224,
    );
    let record = result.unwrap_or_else(|error| panic!("published response exhaustion: {error}"));
    assert!(matches!(
        record.resource_refusal(),
        Some(BudgetMeterRefusal::BudgetExceeded {
            resource: BudgetResourceKind::Cpu,
            limit: 100,
            attempted: actual,
        }) if *actual == attempted
    ));
    assert!(record.response().is_none());
    assert!(record.failure().is_none());
    assert_eq!(record.execution().usage().cpu_fuel, usage);
    assert_eq!(record.execution().usage().output_bytes, 1);
    assert_eq!(storage, Storage::new());
}

#[test]
fn budgeted_resource_exhaustion_supersedes_published_refusal_but_legacy_does_not() {
    let wasm = refusal_then_loop_candidate();
    let charge_sites = frozen_defined_function_charge_sites(&wasm, 1);
    assert_eq!(charge_sites, [6, 2]);
    let (usage, attempted) =
        repeated_charge_exhaustion(100, access_charge([]) + charge_sites[0], charge_sites[1]);
    let (budgeted, _) = run_isolated_candidate(
        &wasm,
        declared(100, 65_536, 0, 0, 2, 1, 0),
        CapabilitySet::empty(),
        Storage::new(),
        0,
        &[],
        225,
    );
    let budgeted = budgeted.unwrap_or_else(|error| panic!("budgeted: {error}"));
    assert!(matches!(
        budgeted.resource_refusal(),
        Some(BudgetMeterRefusal::BudgetExceeded {
            resource: BudgetResourceKind::Cpu,
            limit: 100,
            attempted: actual,
        }) if *actual == attempted
    ));
    assert_eq!(budgeted.execution().usage().cpu_fuel, usage);
    assert!(budgeted.failure().is_none());

    let legacy_budget = ResourceBudget::new_complete(100, 65_536, 0, 0, 2, 1, 0);
    let legacy_executor = Executor::new(legacy_budget, FeeSchedule::declared());
    let engine = WasmEngine::declared().unwrap_or_else(|error| panic!("engine: {error}"));
    let module = engine
        .validate_candidate_v2(&wasm)
        .unwrap_or_else(|error| panic!("validation: {error}"));
    let payer = principal(226);
    let legacy = legacy_executor
        .execute_authorized_candidate(
            &mut Storage::new(),
            AuthorizedExecutionRequest {
                module: &module,
                program: ProgramId::new([227; 32])
                    .unwrap_or_else(|error| panic!("program: {error}")),
                authorization: AuthorizationContext::new(payer, CapabilitySet::empty()),
                receipts: &NoReceipts,
                entrypoint: CALL_ENTRY_EXPORT,
                calldata: &[],
                composition: CompositionContext::isolated(),
                response_capacity: 0,
            },
        )
        .unwrap_or_else(|error| panic!("legacy: {error}"));
    let failure = legacy
        .failure()
        .unwrap_or_else(|| panic!("legacy published refusal was lost"));
    assert_eq!(failure.class(), RefusalClass::Rejected);
    assert_eq!(failure.reason().bytes(), b"x");
    assert!(legacy.resource_refusal().is_none());
}
