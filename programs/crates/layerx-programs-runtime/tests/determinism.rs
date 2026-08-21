use layerx_programs_runtime::test_support::{
    add_module, code_section, export_section, func_body, function_section, import_section, module,
    type_section, unsigned_leb, OP_CALL, OP_DROP, OP_END, OP_I32_CONST, TYPE_I32,
};
use layerx_programs_runtime::{
    AbiError, AuthorizationContext, AuthorizedExecutionRequest, Capability, CapabilitySet,
    CompositionContext, ExecutionError, ExecutionFault, Executor, FeeSchedule, Meter, MeterRefusal,
    PrincipalId, ProgramId, ReceiptOracle, ReceiptView, ResourceBudget, ResourceKind, Storage,
    StorageNamespace, ValidationLimits, WasmEngine, WasmValue, ABI_VERSION, RUNTIME_VERSION,
};

const EXECUTION_V1_GOLDEN: &str = include_str!("../vectors/execution-v1.hex");

fn validated_add() -> layerx_programs_runtime::ValidatedModule {
    let engine = match WasmEngine::new(ValidationLimits::declared()) {
        Ok(engine) => engine,
        Err(refusal) => panic!("engine construction refused: {refusal}"),
    };
    match engine.validate(&add_module()) {
        Ok(module) => module,
        Err(refusal) => panic!("module validation refused: {refusal}"),
    }
}

fn execute_add(left: i32, right: i32) -> layerx_programs_runtime::ExecutionRecord {
    match Executor::declared().execute(
        &validated_add(),
        "add",
        &[WasmValue::I32(left), WasmValue::I32(right)],
    ) {
        Ok(record) => record,
        Err(error) => panic!("execution failed: {error}"),
    }
}

fn one_page_memory_module() -> Vec<u8> {
    let memory_section = vec![5, 3, 1, 0, 1];
    module(&[
        type_section(&[(&[], &[])]),
        function_section(&[0]),
        memory_section,
        export_section(&[("run", 0)]),
        code_section(&[func_body(&[], &[OP_END])]),
    ])
}

fn section(id: u8, payload: &[u8]) -> Vec<u8> {
    let mut encoded = vec![id];
    encoded.extend(unsigned_leb(payload.len() as u64));
    encoded.extend_from_slice(payload);
    encoded
}

fn storage_writer_module(tail: &[u8]) -> Vec<u8> {
    let memory_section = vec![5, 3, 1, 0, 1];
    let export_payload = [vec![
        2, 3, b'r', b'u', b'n', 0, 1, 6, b'm', b'e', b'm', b'o', b'r', b'y', 2, 0,
    ]]
    .concat();
    let mut instructions = vec![
        OP_I32_CONST,
        0,
        OP_I32_CONST,
        3,
        OP_I32_CONST,
        3,
        OP_I32_CONST,
        3,
        OP_CALL,
        0,
        OP_DROP,
    ];
    instructions.extend_from_slice(tail);
    instructions.extend([OP_I32_CONST, 0]);
    instructions.push(OP_END);
    let data_payload = [vec![1, 0, OP_I32_CONST, 0, OP_END, 6], b"keynew".to_vec()].concat();
    module(&[
        type_section(&[
            (&[TYPE_I32, TYPE_I32, TYPE_I32, TYPE_I32], &[TYPE_I32]),
            (&[TYPE_I32, TYPE_I32], &[TYPE_I32]),
        ]),
        import_section(&[("layerx_v1", "storage_write", 0)]),
        function_section(&[1]),
        memory_section,
        section(7, &export_payload),
        code_section(&[func_body(&[], &instructions)]),
        section(11, &data_payload),
    ])
}

#[derive(Debug)]
struct EmptyReceipts;

impl ReceiptOracle for EmptyReceipts {
    fn verified_receipt(&self, _receipt_digest: [u8; 32]) -> Result<ReceiptView, AbiError> {
        Err(AbiError::ReceiptMismatch)
    }
}

fn assert_authorized_write_rolls_back(wasm: &[u8], executor: Executor, expected: ExecutionError) {
    let engine = match WasmEngine::declared() {
        Ok(engine) => engine,
        Err(refusal) => panic!("declared engine refused: {refusal}"),
    };
    let module = match engine.validate(wasm) {
        Ok(module) => module,
        Err(refusal) => panic!("storage writer refused: {refusal}"),
    };
    let program = match ProgramId::new([7; 32]) {
        Ok(program) => program,
        Err(refusal) => panic!("program id refused: {refusal}"),
    };
    let principal = match PrincipalId::new([9; 32]) {
        Ok(principal) => principal,
        Err(refusal) => panic!("principal id refused: {refusal}"),
    };
    let namespace = StorageNamespace::new(program, principal);
    let mut storage = Storage::new();
    {
        let mut transaction = storage.transaction(namespace);
        if let Err(refusal) = transaction.write(b"key", b"old") {
            panic!("preseed write refused: {refusal}");
        }
        let _changed = transaction.commit();
    }
    let before = storage.clone();
    let capabilities = match CapabilitySet::new([Capability::StorageWrite]) {
        Ok(capabilities) => capabilities,
        Err(refusal) => panic!("capabilities refused: {refusal}"),
    };
    let result = executor.execute_authorized(
        &mut storage,
        AuthorizedExecutionRequest {
            module: &module,
            program,
            authorization: AuthorizationContext::new(principal, capabilities),
            receipts: &EmptyReceipts,
            entrypoint: "run",
            calldata: &[],
            composition: CompositionContext::isolated(),
            response_capacity: 0,
        },
    );
    assert_eq!(result, Err(expected));
    assert_eq!(storage, before);
    let transaction = storage.transaction(namespace);
    assert_eq!(transaction.read(b"key"), Ok(Some(b"old".to_vec())));
}

fn decode_hex(encoded: &str) -> Vec<u8> {
    let bytes = encoded.trim().as_bytes();
    assert_eq!(bytes.len() % 2, 0, "golden vector has an odd hex length");
    bytes
        .chunks_exact(2)
        .map(|pair| (hex_nibble(pair[0]) << 4) | hex_nibble(pair[1]))
        .collect()
}

fn hex_nibble(byte: u8) -> u8 {
    match byte {
        b'0'..=b'9' => byte - b'0',
        b'a'..=b'f' => byte - b'a' + 10,
        b'A'..=b'F' => byte - b'A' + 10,
        _ => panic!("golden vector contains non-hex byte {byte}"),
    }
}

#[test]
fn execution_evidence_matches_the_architecture_independent_golden() {
    let record = execute_add(19, 23);
    assert_eq!(record.runtime_version, RUNTIME_VERSION);
    assert_eq!(record.abi_version, ABI_VERSION);
    assert_eq!(record.outputs, vec![WasmValue::I32(42)]);
    assert_eq!(record.canonical_evidence(), decode_hex(EXECUTION_V1_GOLDEN));
}

#[test]
fn equal_executions_consume_equal_budgets() {
    let mut state = 0x4c_61_79_65_72_58_19_02_u64;
    for case in 0..2_048 {
        state = state
            .wrapping_mul(6_364_136_223_846_793_005)
            .wrapping_add(1_442_695_040_888_963_407);
        let bytes = state.to_le_bytes();
        let left = i32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]);
        state = state
            .wrapping_mul(6_364_136_223_846_793_005)
            .wrapping_add(1_442_695_040_888_963_407);
        let bytes = state.to_le_bytes();
        let right = i32::from_le_bytes([bytes[4], bytes[5], bytes[6], bytes[7]]);
        let first = execute_add(left, right);
        let second = execute_add(left, right);
        assert_eq!(first, second, "metering diverged in case {case}");
        assert_eq!(first.canonical_evidence(), second.canonical_evidence());
    }
}

#[test]
fn cumulative_storage_accounting_refuses_counter_overflow() {
    let budget = ResourceBudget::new(1, 1, u64::MAX, u64::MAX, 1, 1);
    for (resource, charge) in [
        (
            ResourceKind::StorageRead,
            Meter::charge_storage_read as fn(&mut Meter, u64) -> Result<(), MeterRefusal>,
        ),
        (ResourceKind::StorageWrite, Meter::charge_storage_write),
    ] {
        let mut meter = Meter::new(budget, FeeSchedule::declared());
        assert_eq!(charge(&mut meter, u64::MAX), Ok(()));
        let refusal = MeterRefusal::CounterOverflow { resource };
        assert_eq!(charge(&mut meter, 1), Err(refusal));
        assert_eq!(meter.finish(), Err(refusal));
    }
}

#[test]
fn cpu_exhaustion_is_a_typed_resource_result() {
    let executor = Executor::new(
        ResourceBudget::new(1, 65_536, 1_024, 1_024, 1, 1),
        FeeSchedule::declared(),
    );
    let result = executor.execute(
        &validated_add(),
        "add",
        &[WasmValue::I32(19), WasmValue::I32(23)],
    );
    assert!(matches!(
        result,
        Err(ExecutionError::Resource(MeterRefusal::BudgetExceeded {
            resource: ResourceKind::Cpu,
            limit: 1,
            ..
        }))
    ));
}

#[test]
fn initial_memory_exhaustion_is_a_typed_resource_result() {
    let engine = match WasmEngine::new(ValidationLimits::declared()) {
        Ok(engine) => engine,
        Err(refusal) => panic!("engine construction refused: {refusal}"),
    };
    let module = match engine.validate(&one_page_memory_module()) {
        Ok(module) => module,
        Err(refusal) => panic!("module validation refused: {refusal}"),
    };
    let executor = Executor::new(
        ResourceBudget::new(1_000, 65_535, 1_024, 1_024, 1, 1),
        FeeSchedule::declared(),
    );
    let result = executor.execute(&module, "run", &[]);
    assert_eq!(
        result,
        Err(ExecutionError::Resource(MeterRefusal::BudgetExceeded {
            resource: ResourceKind::Memory,
            limit: 65_535,
            attempted: 65_536,
        }))
    );
}

#[test]
fn storage_accounting_is_integer_exact_and_repeatable() {
    let budget = ResourceBudget::new(1_000, 65_536, 10, 10, 1, 1);
    let prices = FeeSchedule::new(1, 2, 3, 5, 7);
    let mut first = Meter::new(budget, prices);
    let mut second = Meter::new(budget, prices);
    for meter in [&mut first, &mut second] {
        assert_eq!(meter.charge_storage_read(4), Ok(()));
        assert_eq!(meter.charge_storage_write(6), Ok(()));
    }
    let first_usage = first.finish();
    let second_usage = second.finish();
    assert_eq!(first_usage, second_usage);
    let usage = match first_usage {
        Ok(usage) => usage,
        Err(refusal) => panic!("meter finalization refused: {refusal}"),
    };
    assert_eq!(usage.storage_read_bytes, 4);
    assert_eq!(usage.storage_write_bytes, 6);
    assert_eq!(usage.fee_units, 42);
}

#[test]
fn authorized_storage_write_rolls_back_after_guest_trap() {
    assert_authorized_write_rolls_back(
        &storage_writer_module(&[0x00]),
        Executor::declared(),
        ExecutionError::Fault(ExecutionFault::UnreachableExecuted),
    );
}

#[test]
fn authorized_storage_write_rolls_back_after_real_fuel_exhaustion() {
    let loop_forever = [0x03, 0x40, 0x0c, 0x00, OP_END];
    let budget = ResourceBudget::new(100, 65_536, 1_024, 1_024, 1, 1);
    assert_authorized_write_rolls_back(
        &storage_writer_module(&loop_forever),
        Executor::new(budget, FeeSchedule::declared()),
        ExecutionError::Resource(MeterRefusal::BudgetExceeded {
            resource: ResourceKind::Cpu,
            limit: 100,
            attempted: 101,
        }),
    );
}
