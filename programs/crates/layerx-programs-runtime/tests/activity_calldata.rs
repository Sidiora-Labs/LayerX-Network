use layerx_programs_runtime::abi::MAX_CALL_INPUT_BYTES;
use layerx_programs_runtime::test_support::{
    code_section, func_body, function_section, module, type_section, unsigned_leb, TYPE_I32,
};
use layerx_programs_runtime::{
    AbiError, AuthorizationContext, AuthorizedExecutionRequest, Capability, CapabilitySet,
    CompositionContext, CompositionRules, EntrypointRefusal, ExecutionError, Executor, FeeSchedule,
    PrincipalId, ProgramCatalog, ProgramId, ReceiptOracle, ReceiptView, ResourceBudget, Storage,
    StorageNamespace, WasmEngine, WasmValue, CALL_ENTRY_EXPORT,
};

struct NoReceipts;
impl ReceiptOracle for NoReceipts {
    fn verified_receipt(&self, _: [u8; 32]) -> Result<ReceiptView, AbiError> {
        Err(AbiError::ReceiptMismatch)
    }
}

fn section(id: u8, payload: &[u8]) -> Vec<u8> {
    let mut bytes = vec![id];
    bytes.extend(unsigned_leb(payload.len() as u64));
    bytes.extend_from_slice(payload);
    bytes
}

fn sink_module() -> Vec<u8> {
    let types = type_section(&[
        (&[TYPE_I32, TYPE_I32, TYPE_I32, TYPE_I32], &[TYPE_I32]),
        (&[TYPE_I32], &[TYPE_I32]),
        (&[TYPE_I32, TYPE_I32], &[TYPE_I32]),
    ]);
    let import = section(
        2,
        &[
            1, 9, b'l', b'a', b'y', b'e', b'r', b'x', b'_', b'v', b'1', 13, b's', b't', b'o', b'r',
            b'a', b'g', b'e', b'_', b'w', b'r', b'i', b't', b'e', 0, 0,
        ],
    );
    let functions = function_section(&[1, 2]);
    let memory = section(5, &[1, 1, 17, 17]);
    let exports = section(
        7,
        &[
            3, 14, b'l', b'a', b'y', b'e', b'r', b'x', b'_', b'r', b'e', b's', b'e', b'r', b'v',
            b'e', 0, 1, 11, b'l', b'a', b'y', b'e', b'r', b'x', b'_', b'c', b'a', b'l', b'l', 0, 2,
            6, b'm', b'e', b'm', b'o', b'r', b'y', 2, 0,
        ],
    );
    let reserve = func_body(&[], &[0x41, 0x80, 0x08, 0x0b]);
    let entry = func_body(&[], &[0x41, 0, 0x41, 3, 0x20, 0, 0x20, 1, 0x10, 0, 0x0b]);
    let data = section(11, &[1, 0, 0x41, 0, 0x0b, 3, b'k', b'e', b'y']);
    module(&[
        types,
        import,
        functions,
        memory,
        exports,
        code_section(&[reserve, entry]),
        data,
    ])
}

fn forwarder_module(callee: ProgramId) -> Vec<u8> {
    let types = type_section(&[
        (&[TYPE_I32; 6], &[TYPE_I32]),
        (&[TYPE_I32], &[TYPE_I32]),
        (&[TYPE_I32, TYPE_I32], &[TYPE_I32]),
    ]);
    let import = section(
        2,
        &[
            1, 9, b'l', b'a', b'y', b'e', b'r', b'x', b'_', b'v', b'1', 12, b'p', b'r', b'o', b'g',
            b'r', b'a', b'm', b'_', b'c', b'a', b'l', b'l', 0, 0,
        ],
    );
    let functions = function_section(&[1, 2]);
    let memory = section(5, &[1, 1, 17, 17]);
    let exports = section(
        7,
        &[
            3, 14, b'l', b'a', b'y', b'e', b'r', b'x', b'_', b'r', b'e', b's', b'e', b'r', b'v',
            b'e', 0, 1, 11, b'l', b'a', b'y', b'e', b'r', b'x', b'_', b'c', b'a', b'l', b'l', 0, 2,
            6, b'm', b'e', b'm', b'o', b'r', b'y', 2, 0,
        ],
    );
    let reserve = func_body(&[], &[0x41, 0x80, 0x08, 0x0b]);
    let entry = func_body(
        &[],
        &[
            0x41, 0, 0x41, 32, 0x20, 0, 0x20, 1, 0x41, 32, 0x41, 3, 0x10, 0, 0x0b,
        ],
    );
    let mut data_payload = vec![1, 0, 0x41, 0, 0x0b, 35];
    data_payload.extend_from_slice(&callee.bytes());
    data_payload.extend_from_slice(&[0, 1, 2]);
    module(&[
        types,
        import,
        functions,
        memory,
        exports,
        code_section(&[reserve, entry]),
        section(11, &data_payload),
    ])
}

fn calldata_module(trapping_start: bool) -> Vec<u8> {
    let types = type_section(&[
        (&[TYPE_I32], &[TYPE_I32]),
        (&[TYPE_I32, TYPE_I32], &[TYPE_I32]),
        (&[], &[]),
    ]);
    let functions = function_section(if trapping_start { &[0, 1, 2] } else { &[0, 1] });
    let memory = section(5, &[1, 1, 16, 16]);
    let exports = section(
        7,
        &[
            3, 14, b'l', b'a', b'y', b'e', b'r', b'x', b'_', b'r', b'e', b's', b'e', b'r', b'v',
            b'e', 0, 0, 11, b'l', b'a', b'y', b'e', b'r', b'x', b'_', b'c', b'a', b'l', b'l', 0, 1,
            6, b'm', b'e', b'm', b'o', b'r', b'y', 2, 0,
        ],
    );
    let reserve = func_body(&[], &[0x41, 0, 0x0b]);
    let entry = func_body(&[], &[0x20, 0, 0x2d, 0, 0, 0x0b]);
    let mut sections = vec![types, functions, memory, exports];
    if trapping_start {
        sections.push(section(8, &[2]));
        sections.push(code_section(&[
            reserve,
            entry,
            func_body(&[], &[0x00, 0x0b]),
        ]));
    } else {
        sections.push(code_section(&[reserve, entry]));
    }
    module(&sections)
}

fn execute(
    bytes: &[u8],
    calldata: &[u8],
    storage: &mut Storage,
) -> Result<layerx_programs_runtime::AuthorizedExecutionRecord, ExecutionError> {
    execute_with(bytes, calldata, storage, CapabilitySet::empty())
}

fn execute_with(
    bytes: &[u8],
    calldata: &[u8],
    storage: &mut Storage,
    capabilities: CapabilitySet,
) -> Result<layerx_programs_runtime::AuthorizedExecutionRecord, ExecutionError> {
    execute_program(
        bytes,
        calldata,
        storage,
        ProgramId::new([1; 32]).unwrap_or_else(|error| panic!("program: {error}")),
        capabilities,
        CompositionContext::isolated(),
    )
}

fn execute_program(
    bytes: &[u8],
    calldata: &[u8],
    storage: &mut Storage,
    program: ProgramId,
    capabilities: CapabilitySet,
    composition: CompositionContext,
) -> Result<layerx_programs_runtime::AuthorizedExecutionRecord, ExecutionError> {
    let engine = WasmEngine::declared().unwrap_or_else(|error| panic!("engine: {error}"));
    let module = engine
        .validate(bytes)
        .unwrap_or_else(|error| panic!("validation: {error}"));
    Executor::new(
        ResourceBudget::new(
            2_000_000,
            16 * 1_024 * 1_024,
            2_000_000,
            2_000_000,
            64,
            4_096,
        ),
        FeeSchedule::declared(),
    )
    .execute_authorized(
        storage,
        AuthorizedExecutionRequest {
            module: &module,
            program,
            authorization: AuthorizationContext::new(
                PrincipalId::new([2; 32]).unwrap_or_else(|error| panic!("principal: {error}")),
                capabilities,
            ),
            receipts: &NoReceipts,
            entrypoint: CALL_ENTRY_EXPORT,
            calldata,
            composition,
            response_capacity: 0,
        },
    )
}

#[test]
fn activity_entry_accepts_empty_and_exact_maximum_calldata() {
    let wasm = calldata_module(false);
    let mut storage = Storage::new();
    let empty = execute(&wasm, &[], &mut storage).unwrap_or_else(|error| panic!("empty: {error}"));
    assert_eq!(empty.execution.outputs, vec![WasmValue::I32(0)]);
    let mut maximum = vec![0u8; MAX_CALL_INPUT_BYTES];
    maximum[0] = 7;
    let exact =
        execute(&wasm, &maximum, &mut storage).unwrap_or_else(|error| panic!("max: {error}"));
    assert_eq!(exact.execution.outputs, vec![WasmValue::I32(7)]);

    let mut sink_storage = Storage::new();
    let capabilities = CapabilitySet::new([Capability::StorageWrite])
        .unwrap_or_else(|error| panic!("capability: {error}"));
    execute_with(&sink_module(), &maximum, &mut sink_storage, capabilities)
        .unwrap_or_else(|error| panic!("sink: {error}"));
    let namespace = StorageNamespace::principal(
        ProgramId::new([1; 32]).unwrap_or_else(|error| panic!("program: {error}")),
        PrincipalId::new([2; 32]).unwrap_or_else(|error| panic!("principal: {error}")),
    );
    assert_eq!(
        sink_storage.transaction(namespace).read(b"key"),
        Ok(Some(maximum))
    );
}

#[test]
fn empty_skips_trapping_allocator_and_copy_meter_is_exact() {
    let mut trapping = calldata_module(false);
    let marker = trapping
        .windows(3)
        .position(|window| window == [0x41, 0, 0x0b])
        .unwrap_or_else(|| panic!("reserve body marker missing"));
    trapping[marker] = 0x00;
    let mut storage = Storage::new();
    let empty =
        execute(&trapping, &[], &mut storage).unwrap_or_else(|error| panic!("empty: {error}"));
    assert_eq!(empty.execution.outputs, vec![WasmValue::I32(0)]);

    let wasm = calldata_module(false);
    let one = execute(&wasm, &[7], &mut storage).unwrap_or_else(|error| panic!("one: {error}"));
    let two = execute(&wasm, &[7, 9], &mut storage).unwrap_or_else(|error| panic!("two: {error}"));
    assert_eq!(
        two.execution.usage.cpu_fuel - one.execution.usage.cpu_fuel,
        1
    );
}

#[test]
fn root_and_nested_boundaries_deliver_identical_bytes_without_double_charge() {
    let engine = WasmEngine::declared().unwrap_or_else(|error| panic!("engine: {error}"));
    let child = ProgramId::new([3; 32]).unwrap_or_else(|error| panic!("child: {error}"));
    let sink = sink_module();
    let validated_sink = engine
        .validate(&sink)
        .unwrap_or_else(|error| panic!("sink validation: {error}"));
    let mut catalog = ProgramCatalog::new();
    assert!(catalog.insert(child, validated_sink).is_none());
    let payload = b"nontrivial canonical calldata";

    let direct_caps = CapabilitySet::new([Capability::StorageWrite])
        .unwrap_or_else(|error| panic!("direct caps: {error}"));
    let mut direct_storage = Storage::new();
    let direct = execute_program(
        &sink,
        payload,
        &mut direct_storage,
        child,
        direct_caps,
        CompositionContext::isolated(),
    )
    .unwrap_or_else(|error| panic!("direct: {error}"));

    let root = ProgramId::new([4; 32]).unwrap_or_else(|error| panic!("root: {error}"));
    let nested_caps = CapabilitySet::new([
        Capability::Call { program: child },
        Capability::StorageWrite,
    ])
    .unwrap_or_else(|error| panic!("nested caps: {error}"));
    let mut nested_storage = Storage::new();
    let nested = execute_program(
        &forwarder_module(child),
        payload,
        &mut nested_storage,
        root,
        nested_caps,
        CompositionContext::catalog(catalog, CompositionRules::declared()),
    )
    .unwrap_or_else(|error| panic!("nested: {error}"));

    let principal = PrincipalId::new([2; 32]).unwrap_or_else(|error| panic!("principal: {error}"));
    let namespace = StorageNamespace::principal(child, principal);
    assert_eq!(
        direct_storage.transaction(namespace).read(b"key"),
        Ok(Some(payload.to_vec()))
    );
    assert_eq!(
        nested_storage.transaction(namespace).read(b"key"),
        Ok(Some(payload.to_vec()))
    );
    assert!(direct.call_graph.edges().is_empty());
    assert_eq!(nested.call_graph.edges().len(), 1);
    assert_eq!(nested.call_graph.edges()[0].caller(), root);
    assert_eq!(nested.call_graph.edges()[0].callee(), child);

    let mut direct_plus_storage = Storage::new();
    let direct_plus = execute_program(
        &sink,
        b"nontrivial canonical calldata!",
        &mut direct_plus_storage,
        child,
        CapabilitySet::new([Capability::StorageWrite])
            .unwrap_or_else(|error| panic!("caps: {error}")),
        CompositionContext::isolated(),
    )
    .unwrap_or_else(|error| panic!("direct plus: {error}"));
    assert_eq!(
        direct_plus.execution.usage.cpu_fuel - direct.execution.usage.cpu_fuel,
        1
    );
    let mut plus_catalog = ProgramCatalog::new();
    let plus_sink = engine
        .validate(&sink)
        .unwrap_or_else(|error| panic!("plus sink: {error}"));
    assert!(plus_catalog.insert(child, plus_sink).is_none());
    let mut nested_plus_storage = Storage::new();
    let nested_plus = execute_program(
        &forwarder_module(child),
        b"nontrivial canonical calldata!",
        &mut nested_plus_storage,
        root,
        CapabilitySet::new([
            Capability::Call { program: child },
            Capability::StorageWrite,
        ])
        .unwrap_or_else(|error| panic!("nested plus caps: {error}")),
        CompositionContext::catalog(plus_catalog, CompositionRules::declared()),
    )
    .unwrap_or_else(|error| panic!("nested plus: {error}"));
    assert_eq!(
        nested_plus.execution.usage.cpu_fuel - nested.execution.usage.cpu_fuel,
        2
    );
}

#[test]
fn one_past_maximum_refuses_before_a_trapping_start_runs() {
    let wasm = calldata_module(true);
    let mut storage = Storage::new();
    let before = storage.clone();
    assert_eq!(
        execute(&wasm, &vec![0; MAX_CALL_INPUT_BYTES + 1], &mut storage),
        Err(ExecutionError::Entrypoint(
            EntrypointRefusal::InputTooLarge {
                bytes: MAX_CALL_INPUT_BYTES + 1,
                limit: MAX_CALL_INPUT_BYTES,
            }
        ))
    );
    assert_eq!(storage, before);
}
