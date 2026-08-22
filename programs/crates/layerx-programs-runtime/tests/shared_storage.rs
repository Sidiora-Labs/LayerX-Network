use layerx_programs_runtime::abi::response::CANDIDATE_ABI_MODULE;
use layerx_programs_runtime::test_support::{
    code_section, func_body, function_section, import_section, module, type_section, unsigned_leb,
    TYPE_I32, TYPE_I64,
};
use layerx_programs_runtime::{
    Abi, AbiError, AuthorizationContext, AuthorizedExecutionRequest, Capability, CapabilitySet,
    CompositionContext, CompositionRefusal, CompositionRules, ExecutionError, Executor,
    FeeSchedule, Meter, PrincipalId, ProgramCatalog, ProgramId, ReceiptOracle, ReceiptView,
    ResourceBudget, Storage, StorageNamespace, StorageSelector, WasmEngine, WasmValue, ABI_VERSION,
    CALL_ENTRY_EXPORT,
};

#[derive(Debug)]
struct NoReceipts;

impl ReceiptOracle for NoReceipts {
    fn verified_receipt(&self, _receipt_digest: [u8; 32]) -> Result<ReceiptView, AbiError> {
        Err(AbiError::ReceiptMismatch)
    }
}

fn program(byte: u8) -> ProgramId {
    ProgramId::new([byte; 32]).unwrap_or_else(|error| panic!("program: {error}"))
}

fn principal(byte: u8) -> PrincipalId {
    PrincipalId::new([byte; 32]).unwrap_or_else(|error| panic!("principal: {error}"))
}

fn meter() -> Meter {
    Meter::new(ResourceBudget::declared(), FeeSchedule::declared())
}

fn shared_grants() -> CapabilitySet {
    CapabilitySet::new([
        Capability::SharedStorageRead,
        Capability::SharedStorageWrite,
    ])
    .unwrap_or_else(|error| panic!("capabilities: {error}"))
}

fn section(id: u8, payload: &[u8]) -> Vec<u8> {
    let mut bytes = vec![id];
    bytes.extend(unsigned_leb(payload.len() as u64));
    bytes.extend_from_slice(payload);
    bytes
}

fn memory_and_exports(reserve_index: u8, call_index: u8) -> (Vec<u8>, Vec<u8>) {
    let memory = section(5, &[1, 1, 1, 1]);
    let mut exports = unsigned_leb(3);
    for (name, kind, index) in [
        ("layerx_reserve", 0u8, reserve_index),
        ("layerx_call", 0, call_index),
        ("memory", 2, 0),
    ] {
        exports.extend(unsigned_leb(name.len() as u64));
        exports.extend_from_slice(name.as_bytes());
        exports.extend_from_slice(&[kind, index]);
    }
    (memory, section(7, &exports))
}

fn data(offset: u8, bytes: &[u8]) -> Vec<u8> {
    let mut payload = unsigned_leb(1);
    payload.extend_from_slice(&[0, 0x41, offset, 0x0b]);
    payload.extend(unsigned_leb(bytes.len() as u64));
    payload.extend_from_slice(bytes);
    section(11, &payload)
}

fn shared_increment_guest() -> Vec<u8> {
    let types = type_section(&[
        (&[TYPE_I32; 5], &[TYPE_I32]),
        (&[TYPE_I32], &[TYPE_I32]),
        (&[TYPE_I32, TYPE_I32], &[TYPE_I32]),
    ]);
    let imports = import_section(&[
        (CANDIDATE_ABI_MODULE, "storage_read_scoped", 0),
        (CANDIDATE_ABI_MODULE, "storage_write_scoped", 0),
    ]);
    let (memory, exports) = memory_and_exports(2, 3);
    let entry = [
        0x41, 2, 0x41, 8, 0x41, 5, 0x41, 16, 0x41, 1, 0x10, 0, 0x1a, 0x41, 16, 0x41, 16, 0x2d, 0,
        0, 0x20, 0, 0x2d, 0, 0, 0x6a, 0x3a, 0, 0, 0x41, 2, 0x41, 8, 0x41, 5, 0x41, 16, 0x41, 1,
        0x10, 1, 0x1a, 0x41, 0, 0x0b,
    ];
    module(&[
        types,
        imports,
        function_section(&[1, 2]),
        memory,
        exports,
        code_section(&[func_body(&[], &[0x41, 0, 0x0b]), func_body(&[], &entry)]),
        data(8, b"total"),
    ])
}

fn invalid_selector_guest(raw_selector: i8) -> Vec<u8> {
    let types = type_section(&[
        (&[TYPE_I32; 5], &[TYPE_I32]),
        (&[TYPE_I32], &[TYPE_I32]),
        (&[TYPE_I32, TYPE_I32], &[TYPE_I32]),
    ]);
    let imports = import_section(&[(CANDIDATE_ABI_MODULE, "storage_read_scoped", 0)]);
    let (memory, exports) = memory_and_exports(1, 2);
    let encoded_selector = if raw_selector == -1 {
        0x7f
    } else {
        raw_selector as u8
    };
    let entry = [
        0x41,
        encoded_selector,
        0x41,
        0x7f,
        0x41,
        5,
        0x41,
        16,
        0x41,
        1,
        0x10,
        0,
        0x0b,
    ];
    module(&[
        types,
        imports,
        function_section(&[1, 2]),
        memory,
        exports,
        code_section(&[func_body(&[], &[0x41, 0, 0x0b]), func_body(&[], &entry)]),
    ])
}

fn shared_read_then_delete_guest() -> Vec<u8> {
    let types = type_section(&[
        (&[TYPE_I32; 5], &[TYPE_I32]),
        (&[TYPE_I32; 3], &[TYPE_I32]),
        (&[TYPE_I32], &[TYPE_I32]),
        (&[TYPE_I32, TYPE_I32], &[TYPE_I32]),
    ]);
    let imports = import_section(&[
        (CANDIDATE_ABI_MODULE, "storage_read_scoped", 0),
        (CANDIDATE_ABI_MODULE, "storage_delete_scoped", 1),
    ]);
    let (memory, exports) = memory_and_exports(2, 3);
    let entry = [
        0x41, 2, 0x41, 8, 0x41, 5, 0x41, 16, 0x41, 8, 0x10, 0, 0x1a, 0x41, 2, 0x41, 8, 0x41, 5,
        0x10, 1, 0x0b,
    ];
    module(&[
        types,
        imports,
        function_section(&[2, 3]),
        memory,
        exports,
        code_section(&[func_body(&[], &[0x41, 0, 0x0b]), func_body(&[], &entry)]),
        data(8, b"total"),
    ])
}

fn shared_read_then_write_guest() -> Vec<u8> {
    let types = type_section(&[
        (&[TYPE_I32; 5], &[TYPE_I32]),
        (&[TYPE_I32], &[TYPE_I32]),
        (&[TYPE_I32, TYPE_I32], &[TYPE_I32]),
    ]);
    let imports = import_section(&[
        (CANDIDATE_ABI_MODULE, "storage_read_scoped", 0),
        (CANDIDATE_ABI_MODULE, "storage_write_scoped", 0),
    ]);
    let (memory, exports) = memory_and_exports(2, 3);
    let entry = [
        0x41, 2, 0x41, 8, 0x41, 5, 0x41, 16, 0x41, 8, 0x10, 0, 0x1a, 0x41, 2, 0x41, 8, 0x41, 5,
        0x41, 24, 0x41, 1, 0x10, 1, 0x0b,
    ];
    module(&[
        types,
        imports,
        function_section(&[1, 2]),
        memory,
        exports,
        code_section(&[func_body(&[], &[0x41, 0, 0x0b]), func_body(&[], &entry)]),
        data(8, b"total"),
    ])
}

fn candidate_forwarder(callee: ProgramId, requested: &CapabilitySet) -> Vec<u8> {
    let encoded = requested.canonical_encoding();
    let types = type_section(&[
        (&[TYPE_I32; 8], &[TYPE_I64]),
        (&[TYPE_I32], &[TYPE_I32]),
        (&[TYPE_I32, TYPE_I32], &[TYPE_I32]),
    ]);
    let imports = import_section(&[(CANDIDATE_ABI_MODULE, "program_call_response", 0)]);
    let (memory, exports) = memory_and_exports(1, 2);
    let mut entry = Vec::new();
    for value in [
        0,
        32,
        32,
        0,
        32,
        i32::try_from(encoded.len()).unwrap_or(i32::MAX),
        96,
        0,
    ] {
        entry.extend([0x41, u8::try_from(value).unwrap_or(u8::MAX)]);
    }
    entry.extend([0x10, 0, 0x1a, 0x41, 0, 0x0b]);
    module(&[
        types,
        imports,
        function_section(&[1, 2]),
        memory,
        exports,
        code_section(&[func_body(&[], &[0x41, 0, 0x0b]), func_body(&[], &entry)]),
        data(0, &callee.bytes()),
        data(32, &encoded),
    ])
}

fn candidate_shared_reader() -> Vec<u8> {
    let types = type_section(&[
        (&[TYPE_I32; 5], &[TYPE_I32]),
        (&[TYPE_I32], &[TYPE_I32]),
        (&[TYPE_I32, TYPE_I32], &[TYPE_I32]),
    ]);
    let imports = import_section(&[(CANDIDATE_ABI_MODULE, "storage_read_scoped", 0)]);
    let (memory, exports) = memory_and_exports(1, 2);
    let entry = [0x41, 2, 0x41, 8, 0x41, 5, 0x41, 16, 0x41, 8, 0x10, 0, 0x0b];
    module(&[
        types,
        imports,
        function_section(&[1, 2]),
        memory,
        exports,
        code_section(&[func_body(&[], &[0x41, 0, 0x0b]), func_body(&[], &entry)]),
        data(8, b"total"),
    ])
}

fn execute_guest(
    wasm: &[u8],
    owner: ProgramId,
    actor: PrincipalId,
    capabilities: CapabilitySet,
    storage: &mut Storage,
    calldata: &[u8],
) -> layerx_programs_runtime::CandidateAuthorizedExecutionRecord {
    let engine = WasmEngine::declared().unwrap_or_else(|error| panic!("engine: {error}"));
    let module = engine
        .validate_candidate_v2(wasm)
        .unwrap_or_else(|error| panic!("candidate: {error}"));
    Executor::declared()
        .execute_authorized_candidate(
            storage,
            AuthorizedExecutionRequest {
                module: &module,
                program: owner,
                authorization: AuthorizationContext::new(actor, capabilities),
                receipts: &NoReceipts,
                entrypoint: CALL_ENTRY_EXPORT,
                calldata,
                composition: CompositionContext::isolated(),
                response_capacity: 0,
            },
        )
        .unwrap_or_else(|error| panic!("execution: {error}"))
}

#[test]
fn shared_capabilities_encode_append_only_and_narrow_downward() {
    let parent = shared_grants();
    assert_eq!(parent.canonical_encoding(), [0, 2, 7, 8]);
    assert_eq!(
        parent.narrow([Capability::SharedStorageRead]),
        CapabilitySet::new([Capability::SharedStorageRead])
    );
    assert_eq!(
        CapabilitySet::new([Capability::StorageRead])
            .unwrap_or_else(|error| panic!("principal grant: {error}"))
            .narrow([Capability::SharedStorageRead]),
        Err(AbiError::CapabilityDenied)
    );
    assert_eq!(
        CapabilitySet::new([Capability::SharedStorageRead])
            .unwrap_or_else(|error| panic!("shared read: {error}"))
            .narrow([Capability::SharedStorageWrite]),
        Err(AbiError::CapabilityDenied)
    );
}

#[test]
fn two_principals_update_one_shared_total_with_equal_metering() {
    let owner = program(9);
    let first = principal(1);
    let second = principal(2);
    let key = b"total";
    let mut first_meter = meter();
    let mut first_abi = Abi::new(
        ABI_VERSION,
        owner,
        AuthorizationContext::new(first, shared_grants()),
        Storage::new(),
        &NoReceipts,
    )
    .unwrap_or_else(|error| panic!("first abi: {error}"));
    first_abi
        .storage_write_selected(
            &mut first_meter,
            StorageSelector::Shared,
            key,
            &10u64.to_be_bytes(),
        )
        .unwrap_or_else(|error| panic!("first write: {error}"));
    let storage = first_abi.commit().storage;
    let mut second_meter = meter();
    let mut second_abi = Abi::new(
        ABI_VERSION,
        owner,
        AuthorizationContext::new(second, shared_grants()),
        storage,
        &NoReceipts,
    )
    .unwrap_or_else(|error| panic!("second abi: {error}"));
    let prior = second_abi
        .storage_read_selected(&mut second_meter, StorageSelector::Shared, key)
        .unwrap_or_else(|error| panic!("second read: {error}"))
        .unwrap_or_else(|| panic!("shared total absent"));
    let prior = u64::from_be_bytes(
        prior
            .try_into()
            .unwrap_or_else(|_| panic!("shared total encoding")),
    );
    second_abi
        .storage_write_selected(
            &mut second_meter,
            StorageSelector::Shared,
            key,
            &(prior + 7).to_be_bytes(),
        )
        .unwrap_or_else(|error| panic!("second write: {error}"));
    assert_eq!(
        second_abi.storage_read_selected(&mut second_meter, StorageSelector::Principal, key,),
        Err(AbiError::CapabilityDenied)
    );
    let mut storage = second_abi.commit().storage;
    assert_eq!(
        storage
            .transaction(StorageNamespace::shared(owner))
            .read(key),
        Ok(Some(17u64.to_be_bytes().to_vec()))
    );
    assert_eq!(
        storage
            .transaction(StorageNamespace::principal(owner, first))
            .read(key),
        Ok(None)
    );
    assert_eq!(
        first_meter.finish().map(|usage| usage.storage_write_bytes),
        Ok(13)
    );
    assert_eq!(
        second_meter
            .finish()
            .map(|usage| (usage.storage_read_bytes, usage.storage_write_bytes)),
        Ok((13, 13))
    );
}

#[test]
fn candidate_guest_increments_one_shared_total_for_two_principals() {
    let owner = program(19);
    let first = principal(1);
    let second = principal(2);
    let wasm = shared_increment_guest();
    let mut storage = Storage::new();
    let first_record = execute_guest(&wasm, owner, first, shared_grants(), &mut storage, &[10]);
    let second_record = execute_guest(&wasm, owner, second, shared_grants(), &mut storage, &[7]);
    assert_eq!(first_record.execution().outputs(), [WasmValue::I32(0)]);
    assert_eq!(second_record.execution().outputs(), [WasmValue::I32(0)]);
    assert_eq!(first_record.execution().usage().storage_read_bytes, 5);
    assert_eq!(first_record.execution().usage().storage_write_bytes, 6);
    assert_eq!(second_record.execution().usage().storage_read_bytes, 6);
    assert_eq!(second_record.execution().usage().storage_write_bytes, 6);
    assert_eq!(
        storage
            .transaction(StorageNamespace::shared(owner))
            .read(b"total"),
        Ok(Some(vec![17]))
    );
    for actor in [first, second] {
        assert_eq!(
            storage
                .transaction(StorageNamespace::principal(owner, actor))
                .read(b"total"),
            Ok(None)
        );
    }
}

#[test]
fn candidate_guest_shared_read_only_cannot_mutate_the_total() {
    let owner = program(20);
    let actor = principal(3);
    let mut storage = Storage::new();
    let mut seed = storage.transaction(StorageNamespace::shared(owner));
    seed.write(b"total", &[10])
        .unwrap_or_else(|error| panic!("seed: {error}"));
    assert_eq!(seed.commit(), 1);
    let read_only = CapabilitySet::new([Capability::SharedStorageRead])
        .unwrap_or_else(|error| panic!("read only: {error}"));
    let record = execute_guest(
        &shared_increment_guest(),
        owner,
        actor,
        read_only,
        &mut storage,
        &[7],
    );
    assert_eq!(record.execution().outputs(), [WasmValue::I32(0)]);
    assert_eq!(record.execution().usage().storage_read_bytes, 6);
    assert_eq!(record.execution().usage().storage_write_bytes, 0);
    assert_eq!(
        storage
            .transaction(StorageNamespace::shared(owner))
            .read(b"total"),
        Ok(Some(vec![10]))
    );
}

#[test]
fn candidate_guest_shared_read_succeeds_and_delete_is_denied_without_mutation() {
    let owner = program(23);
    let actor = principal(6);
    let mut storage = Storage::new();
    let mut seed = storage.transaction(StorageNamespace::shared(owner));
    seed.write(b"total", &17u64.to_be_bytes())
        .unwrap_or_else(|error| panic!("seed: {error}"));
    assert_eq!(seed.commit(), 1);
    let before = storage.clone();
    for wasm in [
        shared_read_then_write_guest(),
        shared_read_then_delete_guest(),
    ] {
        let record = execute_guest(
            &wasm,
            owner,
            actor,
            CapabilitySet::new([Capability::SharedStorageRead])
                .unwrap_or_else(|error| panic!("read grant: {error}")),
            &mut storage,
            &[],
        );
        assert_eq!(record.execution().outputs(), [WasmValue::I32(-1)]);
        assert_eq!(record.execution().usage().storage_read_bytes, 13);
        assert_eq!(record.execution().usage().storage_write_bytes, 0);
        assert_eq!(storage, before);
    }
}

#[test]
fn invalid_guest_selectors_refuse_before_memory_or_storage_access() {
    let owner = program(21);
    let actor = principal(4);
    for selector in [-1, 0, 3] {
        let mut storage = Storage::new();
        let before = storage.clone();
        let record = execute_guest(
            &invalid_selector_guest(selector),
            owner,
            actor,
            shared_grants(),
            &mut storage,
            &[],
        );
        assert_eq!(record.execution().outputs(), [WasmValue::I32(-2)]);
        assert_eq!(record.execution().usage().storage_read_bytes, 0);
        assert_eq!(record.execution().usage().storage_write_bytes, 0);
        assert_eq!(storage, before);
    }
}

#[test]
fn selector_values_are_frozen_and_invalid_values_are_typed() {
    assert_eq!(StorageSelector::try_from(1), Ok(StorageSelector::Principal));
    assert_eq!(StorageSelector::try_from(2), Ok(StorageSelector::Shared));
    for invalid in [-1, 0, 3, i32::MAX] {
        assert_eq!(
            StorageSelector::try_from(invalid),
            Err(AbiError::InvalidEncoding)
        );
    }
}

#[test]
fn principal_and_shared_grants_do_not_cross_and_denials_are_unmetered() {
    let owner = program(12);
    let actor = principal(13);
    let mut principal_meter = meter();
    let mut principal_only = Abi::new(
        ABI_VERSION,
        owner,
        AuthorizationContext::new(
            actor,
            CapabilitySet::new([Capability::StorageRead, Capability::StorageWrite])
                .unwrap_or_else(|error| panic!("principal grants: {error}")),
        ),
        Storage::new(),
        &NoReceipts,
    )
    .unwrap_or_else(|error| panic!("principal abi: {error}"));
    assert_eq!(
        principal_only.storage_write_selected(
            &mut principal_meter,
            StorageSelector::Shared,
            b"total",
            b"1",
        ),
        Err(AbiError::CapabilityDenied)
    );
    assert_eq!(
        principal_meter
            .finish()
            .map(|usage| usage.storage_write_bytes),
        Ok(0)
    );

    let mut shared_meter = meter();
    let mut shared_read_only = Abi::new(
        ABI_VERSION,
        owner,
        AuthorizationContext::new(
            actor,
            CapabilitySet::new([Capability::SharedStorageRead])
                .unwrap_or_else(|error| panic!("shared grant: {error}")),
        ),
        Storage::new(),
        &NoReceipts,
    )
    .unwrap_or_else(|error| panic!("shared abi: {error}"));
    assert_eq!(
        shared_read_only.storage_write_selected(
            &mut shared_meter,
            StorageSelector::Shared,
            b"total",
            b"1",
        ),
        Err(AbiError::CapabilityDenied)
    );
    assert_eq!(
        shared_meter.finish().map(|usage| usage.storage_write_bytes),
        Ok(0)
    );
}

#[test]
fn principal_and_shared_access_charge_identical_bytes() {
    let owner = program(22);
    let actor = principal(5);
    let grants = CapabilitySet::new([
        Capability::StorageRead,
        Capability::StorageWrite,
        Capability::SharedStorageRead,
        Capability::SharedStorageWrite,
    ])
    .unwrap_or_else(|error| panic!("grants: {error}"));
    let mut abi = Abi::new(
        ABI_VERSION,
        owner,
        AuthorizationContext::new(actor, grants),
        Storage::new(),
        &NoReceipts,
    )
    .unwrap_or_else(|error| panic!("abi: {error}"));
    let mut principal_write = meter();
    let mut shared_write = meter();
    abi.storage_write_selected(
        &mut principal_write,
        StorageSelector::Principal,
        b"same",
        b"value",
    )
    .unwrap_or_else(|error| panic!("principal write: {error}"));
    abi.storage_write_selected(
        &mut shared_write,
        StorageSelector::Shared,
        b"same",
        b"value",
    )
    .unwrap_or_else(|error| panic!("shared write: {error}"));
    assert_eq!(
        principal_write
            .finish()
            .map(|usage| usage.storage_write_bytes),
        shared_write.finish().map(|usage| usage.storage_write_bytes)
    );
    let mut principal_read = meter();
    let mut shared_read = meter();
    assert_eq!(
        abi.storage_read_selected(&mut principal_read, StorageSelector::Principal, b"same",),
        abi.storage_read_selected(&mut shared_read, StorageSelector::Shared, b"same")
    );
    assert_eq!(
        principal_read
            .finish()
            .map(|usage| usage.storage_read_bytes),
        shared_read.finish().map(|usage| usage.storage_read_bytes)
    );
    let mut principal_delete = meter();
    let mut shared_delete = meter();
    abi.storage_delete_selected(&mut principal_delete, StorageSelector::Principal, b"same")
        .unwrap_or_else(|error| panic!("principal delete: {error}"));
    abi.storage_delete_selected(&mut shared_delete, StorageSelector::Shared, b"same")
        .unwrap_or_else(|error| panic!("shared delete: {error}"));
    assert_eq!(
        principal_delete
            .finish()
            .map(|usage| usage.storage_write_bytes),
        shared_delete
            .finish()
            .map(|usage| usage.storage_write_bytes)
    );
}

#[test]
fn candidate_program_call_narrows_shared_authority_before_child_entry() {
    let root = program(24);
    let child = program(25);
    let actor = principal(7);
    let engine = WasmEngine::declared().unwrap_or_else(|error| panic!("engine: {error}"));
    let child_module = engine
        .validate_candidate_v2(&candidate_shared_reader())
        .unwrap_or_else(|error| panic!("child: {error}"));
    let requested_read = CapabilitySet::new([Capability::SharedStorageRead])
        .unwrap_or_else(|error| panic!("requested read: {error}"));
    let root_module = engine
        .validate_candidate_v2(&candidate_forwarder(child, &requested_read))
        .unwrap_or_else(|error| panic!("root: {error}"));
    let mut catalog = ProgramCatalog::new();
    catalog.insert(child, child_module.clone());
    let mut storage = Storage::new();
    let mut seed = storage.transaction(StorageNamespace::shared(child));
    seed.write(b"total", &17u64.to_be_bytes())
        .unwrap_or_else(|error| panic!("seed: {error}"));
    assert_eq!(seed.commit(), 1);
    let before = storage.clone();
    let record = Executor::declared()
        .execute_authorized_candidate(
            &mut storage,
            AuthorizedExecutionRequest {
                module: &root_module,
                program: root,
                authorization: AuthorizationContext::new(
                    actor,
                    CapabilitySet::new([
                        Capability::Call { program: child },
                        Capability::SharedStorageRead,
                    ])
                    .unwrap_or_else(|error| panic!("parent grants: {error}")),
                ),
                receipts: &NoReceipts,
                entrypoint: CALL_ENTRY_EXPORT,
                calldata: &[],
                composition: CompositionContext::catalog(catalog, CompositionRules::declared()),
                response_capacity: 0,
            },
        )
        .unwrap_or_else(|error| panic!("shared propagation: {error}"));
    assert_eq!(record.call_graph().edges().len(), 1);
    assert_eq!(record.call_graph().edges()[0].callee(), child);
    assert!(record.execution().usage().storage_read_bytes >= 13);
    assert_eq!(storage, before);

    for (parent_scope, requested) in [
        (Capability::StorageRead, Capability::SharedStorageRead),
        (
            Capability::SharedStorageRead,
            Capability::SharedStorageWrite,
        ),
    ] {
        let request =
            CapabilitySet::new([requested]).unwrap_or_else(|error| panic!("request: {error}"));
        let denied_root = engine
            .validate_candidate_v2(&candidate_forwarder(child, &request))
            .unwrap_or_else(|error| panic!("denied root: {error}"));
        let mut denied_catalog = ProgramCatalog::new();
        denied_catalog.insert(child, child_module.clone());
        let mut denied_storage = before.clone();
        assert_eq!(
            Executor::declared().execute_authorized_candidate(
                &mut denied_storage,
                AuthorizedExecutionRequest {
                    module: &denied_root,
                    program: root,
                    authorization: AuthorizationContext::new(
                        actor,
                        CapabilitySet::new([Capability::Call { program: child }, parent_scope])
                            .unwrap_or_else(|error| panic!("denied grants: {error}")),
                    ),
                    receipts: &NoReceipts,
                    entrypoint: CALL_ENTRY_EXPORT,
                    calldata: &[],
                    composition: CompositionContext::catalog(
                        denied_catalog,
                        CompositionRules::declared(),
                    ),
                    response_capacity: 0,
                },
            ),
            Err(ExecutionError::Composition(CompositionRefusal::Authority(
                AbiError::CapabilityDenied,
            )))
        );
        assert_eq!(denied_storage, before);
    }
}
