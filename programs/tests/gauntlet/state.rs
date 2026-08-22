use super::*;
use layerx_programs_runtime::{
    ActivityBudgetBinding, BudgetMeterRefusal, BudgetResourceKind,
    BudgetedAuthorizedExecutionRequest, CandidateActivityOutcome, DeclaredBudget,
};

fn candidate_exports(reserve: u8, call: u8) -> (Vec<u8>, Vec<u8>) {
    let memory = section(5, &[1, 1, 1, 1]);
    let exports = exports(&[
        ("layerx_reserve", 0, reserve),
        (CALL_ENTRY_EXPORT, 0, call),
        ("memory", 2, 0),
    ]);
    (memory, exports)
}

fn scoped_read_guest(selector: i32, key: &[u8]) -> Vec<u8> {
    let mut entry = Vec::new();
    for value in [
        selector,
        0,
        i32::try_from(key.len()).unwrap_or(i32::MAX),
        256,
        64,
    ] {
        push_i32(&mut entry, value);
    }
    entry.extend([OP_CALL, 0, OP_END]);
    let (memory, exports) = candidate_exports(1, 2);
    module(&[
        type_section(&[
            (&[TYPE_I32; 5], &[TYPE_I32]),
            (&[TYPE_I32], &[TYPE_I32]),
            (&[TYPE_I32, TYPE_I32], &[TYPE_I32]),
        ]),
        import_section(&[(CANDIDATE_ABI_MODULE, "storage_read_scoped", 0)]),
        function_section(&[1, 2]),
        memory,
        exports,
        code_section(&[
            func_body(&[], &[OP_I32_CONST, 0, OP_END]),
            func_body(&[], &entry),
        ]),
        data_section(&[(0, key)]),
    ])
}

fn scoped_read_response_guest(selector: i32, key: &[u8], response_length: i32) -> Vec<u8> {
    let mut entry = Vec::new();
    for value in [
        selector,
        0,
        i32::try_from(key.len()).unwrap_or(i32::MAX),
        256,
        64,
    ] {
        push_i32(&mut entry, value);
    }
    entry.extend([OP_CALL, 0, 0x1a]);
    for value in [0, 256, response_length] {
        push_i32(&mut entry, value);
    }
    entry.extend([OP_CALL, 1, 0x1a, OP_I32_CONST, 0, OP_END]);
    let (memory, exports) = candidate_exports(2, 3);
    module(&[
        type_section(&[
            (&[TYPE_I32; 5], &[TYPE_I32]),
            (&[TYPE_I32; 3], &[TYPE_I32]),
            (&[TYPE_I32], &[TYPE_I32]),
            (&[TYPE_I32, TYPE_I32], &[TYPE_I32]),
        ]),
        import_section(&[
            (CANDIDATE_ABI_MODULE, "storage_read_scoped", 0),
            (CANDIDATE_ABI_MODULE, "response_write", 1),
        ]),
        function_section(&[2, 3]),
        memory,
        exports,
        code_section(&[
            func_body(&[], &[OP_I32_CONST, 0, OP_END]),
            func_body(&[], &entry),
        ]),
        data_section(&[(0, key)]),
    ])
}

fn scoped_write_guest(selector: i32, key: &[u8], value: &[u8]) -> Vec<u8> {
    let mut entry = Vec::new();
    for argument in [
        selector,
        0,
        i32::try_from(key.len()).unwrap_or(i32::MAX),
        256,
        i32::try_from(value.len()).unwrap_or(i32::MAX),
    ] {
        push_i32(&mut entry, argument);
    }
    entry.extend([OP_CALL, 0, OP_END]);
    let (memory, exports) = candidate_exports(1, 2);
    module(&[
        type_section(&[
            (&[TYPE_I32; 5], &[TYPE_I32]),
            (&[TYPE_I32], &[TYPE_I32]),
            (&[TYPE_I32, TYPE_I32], &[TYPE_I32]),
        ]),
        import_section(&[(CANDIDATE_ABI_MODULE, "storage_write_scoped", 0)]),
        function_section(&[1, 2]),
        memory,
        exports,
        code_section(&[
            func_body(&[], &[OP_I32_CONST, 0, OP_END]),
            func_body(&[], &entry),
        ]),
        data_section(&[(0, key), (256, value)]),
    ])
}

fn scoped_delete_guest(selector: i32, key: &[u8]) -> Vec<u8> {
    let mut entry = Vec::new();
    for argument in [selector, 0, i32::try_from(key.len()).unwrap_or(i32::MAX)] {
        push_i32(&mut entry, argument);
    }
    entry.extend([OP_CALL, 0, OP_END]);
    let (memory, exports) = candidate_exports(1, 2);
    module(&[
        type_section(&[
            (&[TYPE_I32; 3], &[TYPE_I32]),
            (&[TYPE_I32], &[TYPE_I32]),
            (&[TYPE_I32, TYPE_I32], &[TYPE_I32]),
        ]),
        import_section(&[(CANDIDATE_ABI_MODULE, "storage_delete_scoped", 0)]),
        function_section(&[1, 2]),
        memory,
        exports,
        code_section(&[
            func_body(&[], &[OP_I32_CONST, 0, OP_END]),
            func_body(&[], &entry),
        ]),
        data_section(&[(0, key)]),
    ])
}

fn scoped_drop_guest(selector: i32) -> Vec<u8> {
    let mut entry = Vec::new();
    push_i32(&mut entry, selector);
    entry.extend([OP_CALL, 0, OP_END]);
    let (memory, exports) = candidate_exports(1, 2);
    module(&[
        type_section(&[
            (&[TYPE_I32], &[TYPE_I32]),
            (&[TYPE_I32], &[TYPE_I32]),
            (&[TYPE_I32, TYPE_I32], &[TYPE_I32]),
        ]),
        import_section(&[(CANDIDATE_ABI_MODULE, "storage_drop_scoped", 0)]),
        function_section(&[1, 2]),
        memory,
        exports,
        code_section(&[
            func_body(&[], &[OP_I32_CONST, 0, OP_END]),
            func_body(&[], &entry),
        ]),
    ])
}

fn scoped_scan_status_guest(selector: i32, cursor: &[u8]) -> Vec<u8> {
    let mut entry = Vec::new();
    for argument in [
        selector,
        0,
        0,
        32,
        i32::try_from(cursor.len()).unwrap_or(i32::MAX),
        1,
        128,
        256,
        128,
    ] {
        push_i32(&mut entry, argument);
    }
    entry.extend([OP_CALL, 0, OP_END]);
    let (memory, exports) = candidate_exports(1, 2);
    module(&[
        type_section(&[
            (&[TYPE_I32; 9], &[TYPE_I32]),
            (&[TYPE_I32], &[TYPE_I32]),
            (&[TYPE_I32, TYPE_I32], &[TYPE_I32]),
        ]),
        import_section(&[(CANDIDATE_ABI_MODULE, "storage_scan_scoped", 0)]),
        function_section(&[1, 2]),
        memory,
        exports,
        code_section(&[
            func_body(&[], &[OP_I32_CONST, 0, OP_END]),
            func_body(&[], &entry),
        ]),
        data_section(&[(32, cursor)]),
    ])
}

fn call_with_capability_bytes(callee: ProgramId, encoded: &[u8]) -> Vec<u8> {
    let mut entry = Vec::new();
    for argument in [
        0,
        32,
        64,
        0,
        96,
        i32::try_from(encoded.len()).unwrap_or(i32::MAX),
        192,
        0,
    ] {
        push_i32(&mut entry, argument);
    }
    entry.extend([OP_CALL, 0, 0xa7, OP_END]);
    let (memory, exports) = candidate_exports(1, 2);
    module(&[
        type_section(&[
            (&[TYPE_I32; 8], &[TYPE_I64]),
            (&[TYPE_I32], &[TYPE_I32]),
            (&[TYPE_I32, TYPE_I32], &[TYPE_I32]),
        ]),
        import_section(&[(CANDIDATE_ABI_MODULE, "program_call_response", 0)]),
        function_section(&[1, 2]),
        memory,
        exports,
        code_section(&[
            func_body(&[], &[OP_I32_CONST, 0, OP_END]),
            func_body(&[], &entry),
        ]),
        data_section(&[(0, &callee.bytes()), (96, encoded)]),
    ])
}

fn repeated_scan_guest(repetitions: usize) -> Vec<u8> {
    let mut entry = Vec::new();
    for _ in 0..repetitions {
        for argument in [2, 0, 0, 32, 0, 1, 16, 128, 128] {
            push_i32(&mut entry, argument);
        }
        entry.extend([OP_CALL, 0, 0x1a]);
    }
    entry.extend([OP_I32_CONST, 0, OP_END]);
    let (memory, exports) = candidate_exports(1, 2);
    module(&[
        type_section(&[
            (&[TYPE_I32; 9], &[TYPE_I32]),
            (&[TYPE_I32], &[TYPE_I32]),
            (&[TYPE_I32, TYPE_I32], &[TYPE_I32]),
        ]),
        import_section(&[(CANDIDATE_ABI_MODULE, "storage_scan_scoped", 0)]),
        function_section(&[1, 2]),
        memory,
        exports,
        code_section(&[
            func_body(&[], &[OP_I32_CONST, 0, OP_END]),
            func_body(&[], &entry),
        ]),
    ])
}

fn repeated_drop_rewrite_guest(selector: i32) -> Vec<u8> {
    let mut entry = Vec::new();
    push_i32(&mut entry, selector);
    entry.extend([OP_CALL, 0, 0x1a]);
    for argument in [selector, 0, 1, 1, 1] {
        push_i32(&mut entry, argument);
    }
    entry.extend([OP_CALL, 1, 0x1a]);
    push_i32(&mut entry, selector);
    entry.extend([OP_CALL, 0, 0x1a, OP_I32_CONST, 0, OP_END]);
    let (memory, exports) = candidate_exports(2, 3);
    module(&[
        type_section(&[
            (&[TYPE_I32], &[TYPE_I32]),
            (&[TYPE_I32; 5], &[TYPE_I32]),
            (&[TYPE_I32], &[TYPE_I32]),
            (&[TYPE_I32, TYPE_I32], &[TYPE_I32]),
        ]),
        import_section(&[
            (CANDIDATE_ABI_MODULE, "storage_drop_scoped", 0),
            (CANDIDATE_ABI_MODULE, "storage_write_scoped", 1),
        ]),
        function_section(&[2, 3]),
        memory,
        exports,
        code_section(&[
            func_body(&[], &[OP_I32_CONST, 0, OP_END]),
            func_body(&[], &entry),
        ]),
        data_section(&[(0, b"k"), (1, b"v")]),
    ])
}

fn execute_candidate(
    executor: &Executor,
    wasm: &[u8],
    storage: &mut Storage,
    owner: ProgramId,
    actor: PrincipalId,
    capabilities: CapabilitySet,
    composition: CompositionContext,
) -> Result<layerx_programs_runtime::CandidateAuthorizedExecutionRecord, ExecutionError> {
    let engine = WasmEngine::declared().unwrap_or_else(|error| panic!("engine: {error}"));
    let module = engine
        .validate_candidate_v2(wasm)
        .unwrap_or_else(|error| panic!("candidate: {error}"));
    executor.execute_authorized_candidate(
        storage,
        AuthorizedExecutionRequest {
            module: &module,
            program: owner,
            authorization: AuthorizationContext::new(actor, capabilities),
            receipts: &NoReceipts,
            entrypoint: CALL_ENTRY_EXPORT,
            calldata: &[],
            composition,
            response_capacity: 0,
        },
    )
}

fn execute_candidate_with_response(
    wasm: &[u8],
    storage: &mut Storage,
    owner: ProgramId,
    actor: PrincipalId,
    capabilities: CapabilitySet,
    response_capacity: usize,
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
                calldata: &[],
                composition: CompositionContext::isolated(),
                response_capacity,
            },
        )
        .unwrap_or_else(|error| panic!("response candidate: {error}"))
}

fn execute_budgeted_candidate(
    wasm: &[u8],
    storage: &mut Storage,
    owner: ProgramId,
    actor: PrincipalId,
    capabilities: CapabilitySet,
    budget: DeclaredBudget,
    binding_byte: u8,
) -> layerx_programs_runtime::CandidateAuthorizedExecutionRecord {
    let executor = Executor::declared();
    let binding = ActivityBudgetBinding::new([binding_byte; 32])
        .unwrap_or_else(|error| panic!("binding: {error}"));
    let admitted = executor
        .admit_activity_budget_for_qualification(budget, actor, binding, u128::MAX)
        .unwrap_or_else(|error| panic!("admission: {error}"));
    let engine = WasmEngine::declared().unwrap_or_else(|error| panic!("engine: {error}"));
    let module = engine
        .validate_candidate_v2(wasm)
        .unwrap_or_else(|error| panic!("candidate: {error}"));
    executor
        .execute_authorized_candidate_budgeted_for_qualification(
            storage,
            BudgetedAuthorizedExecutionRequest::new(
                AuthorizedExecutionRequest {
                    module: &module,
                    program: owner,
                    authorization: AuthorizationContext::new(actor, capabilities),
                    receipts: &NoReceipts,
                    entrypoint: CALL_ENTRY_EXPORT,
                    calldata: &[],
                    composition: CompositionContext::isolated(),
                    response_capacity: 0,
                },
                admitted,
                actor,
                binding,
            ),
        )
        .unwrap_or_else(|error| panic!("budgeted candidate: {error}"))
}

fn declared(read: u64, write: u64) -> DeclaredBudget {
    DeclaredBudget::new(1_000_000, 65_536, read, write, 1, 0, 1)
        .unwrap_or_else(|error| panic!("declared budget: {error}"))
}

fn shared_cursor(owner: ProgramId, after: &[u8]) -> Vec<u8> {
    let mut cursor = vec![1, 33];
    cursor.extend_from_slice(&owner.bytes());
    cursor.push(1);
    cursor.extend_from_slice(&0u16.to_be_bytes());
    cursor.extend_from_slice(&1u32.to_be_bytes());
    cursor.extend_from_slice(&128u32.to_be_bytes());
    cursor.extend_from_slice(&(after.len() as u16).to_be_bytes());
    cursor.extend_from_slice(after);
    cursor
}

fn seed(storage: &mut Storage, namespace: StorageNamespace, key: &[u8], value: &[u8]) {
    let mut transaction = storage.transaction(namespace);
    transaction
        .write(key, value)
        .unwrap_or_else(|error| panic!("seed: {error}"));
    assert_eq!(transaction.commit(), 1);
}

pub(super) fn foreign_shared_selectors_are_structurally_closed() {
    let (attacker, actor) = ids(41, 51);
    let (victim, _) = ids(42, 51);
    let victim_namespace = StorageNamespace::shared(victim);
    let attacker_namespace = StorageNamespace::shared(attacker);
    let mut storage = Storage::new();
    seed(&mut storage, victim_namespace, b"target", b"canary");
    let principal_canary = StorageNamespace::principal(attacker, actor);
    seed(
        &mut storage,
        principal_canary,
        b"target",
        b"principal-canary",
    );
    let grants = CapabilitySet::new([
        Capability::SharedStorageRead,
        Capability::SharedStorageWrite,
    ])
    .unwrap_or_else(|error| panic!("grants: {error}"));
    for selector in [-1, 0, 3, i32::MAX] {
        for wasm in [
            scoped_read_guest(selector, b"target"),
            scoped_write_guest(selector, b"target", b"hostile"),
            scoped_delete_guest(selector, b"target"),
            scoped_scan_status_guest(selector, b""),
            scoped_drop_guest(selector),
        ] {
            let before = storage.clone();
            let record = execute_candidate(
                &Executor::declared(),
                &wasm,
                &mut storage,
                attacker,
                actor,
                grants.clone(),
                CompositionContext::isolated(),
            )
            .unwrap_or_else(|error| panic!("selector: {error}"));
            assert_eq!(record.execution().outputs(), [WasmValue::I32(-2)]);
            assert_eq!(record.execution().usage().storage_read_bytes, 0);
            assert_eq!(record.execution().usage().storage_write_bytes, 0);
            let effects = record
                .effects()
                .unwrap_or_else(|| panic!("selector effects"));
            assert!(
                effects.calls.is_empty()
                    && effects.events.is_empty()
                    && effects.transfers.is_empty()
                    && effects.namespace_drops.is_empty()
            );
            assert_eq!(storage, before);
        }
    }
    for (selector, capability) in [
        (1, Capability::SharedStorageRead),
        (1, Capability::SharedStorageWrite),
        (2, Capability::StorageRead),
        (2, Capability::StorageWrite),
    ] {
        let operations = if matches!(
            capability,
            Capability::StorageRead | Capability::SharedStorageRead
        ) {
            vec![
                scoped_read_guest(selector, b"target"),
                scoped_scan_status_guest(selector, b""),
            ]
        } else {
            vec![
                scoped_write_guest(selector, b"target", b"hostile"),
                scoped_delete_guest(selector, b"target"),
                scoped_drop_guest(selector),
            ]
        };
        for wasm in operations {
            let before = storage.clone();
            let record = execute_candidate(
                &Executor::declared(),
                &wasm,
                &mut storage,
                attacker,
                actor,
                CapabilitySet::new([capability])
                    .unwrap_or_else(|error| panic!("cross grant: {error}")),
                CompositionContext::isolated(),
            )
            .unwrap_or_else(|error| panic!("cross selector: {error}"));
            assert_eq!(record.execution().outputs(), [WasmValue::I32(-1)]);
            assert_eq!(record.execution().usage().storage_read_bytes, 0);
            assert_eq!(record.execution().usage().storage_write_bytes, 0);
            assert_eq!(storage, before);
        }
    }
    execute_candidate(
        &Executor::declared(),
        &scoped_write_guest(2, b"target", b"attacker"),
        &mut storage,
        attacker,
        actor,
        grants,
        CompositionContext::isolated(),
    )
    .unwrap_or_else(|error| panic!("own shared write: {error}"));
    assert_eq!(
        storage.transaction(victim_namespace).read(b"target"),
        Ok(Some(b"canary".to_vec()))
    );
    assert_eq!(
        storage.transaction(attacker_namespace).read(b"target"),
        Ok(Some(b"attacker".to_vec()))
    );
    for wasm in [
        scoped_scan_status_guest(2, b""),
        scoped_delete_guest(2, b"target"),
    ] {
        execute_candidate(
            &Executor::declared(),
            &wasm,
            &mut storage,
            attacker,
            actor,
            CapabilitySet::new([
                Capability::SharedStorageRead,
                Capability::SharedStorageWrite,
            ])
            .unwrap_or_else(|error| panic!("destructive grant: {error}")),
            CompositionContext::isolated(),
        )
        .unwrap_or_else(|error| panic!("destructive operation: {error}"));
    }
    seed(&mut storage, attacker_namespace, b"drop", b"only-attacker");
    execute_candidate(
        &Executor::declared(),
        &scoped_drop_guest(2),
        &mut storage,
        attacker,
        actor,
        CapabilitySet::new([Capability::SharedStorageWrite])
            .unwrap_or_else(|error| panic!("drop grant: {error}")),
        CompositionContext::isolated(),
    )
    .unwrap_or_else(|error| panic!("drop operation: {error}"));
    assert_eq!(storage.namespace_cell_count(attacker_namespace), 0);
    assert_eq!(
        storage.transaction(victim_namespace).read(b"target"),
        Ok(Some(b"canary".to_vec()))
    );
    assert_eq!(
        storage.transaction(principal_canary).read(b"target"),
        Ok(Some(b"principal-canary".to_vec()))
    );
}

pub(super) fn forged_shared_capability_bytes_never_enter_the_child() {
    let (root, actor) = ids(43, 52);
    let (child, _) = ids(44, 52);
    let child_wasm = scoped_write_guest(2, b"target", b"escaped");
    let engine = WasmEngine::declared().unwrap_or_else(|error| panic!("engine: {error}"));
    let capabilities = CapabilitySet::new([
        Capability::Call { program: child },
        Capability::SharedStorageWrite,
    ])
    .unwrap_or_else(|error| panic!("grants: {error}"));
    for forged in [
        vec![0xff],
        vec![0, 1],
        vec![0, 2, 7],
        vec![0, 2, 7, 7],
        vec![0, 1, 7, 7],
        vec![0, 2, 8, 8],
        vec![0, 1, 8, 8],
        vec![0, 2, 7, 8, 8],
        vec![0, 1, 255],
        vec![0, 2, 255],
    ] {
        let mut catalog = ProgramCatalog::new();
        catalog.insert(
            child,
            engine
                .validate_candidate_v2(&child_wasm)
                .unwrap_or_else(|error| panic!("child: {error}")),
        );
        let mut storage = Storage::new();
        let before = storage.clone();
        let result = execute_candidate(
            &Executor::declared(),
            &call_with_capability_bytes(child, &forged),
            &mut storage,
            root,
            actor,
            capabilities.clone(),
            CompositionContext::catalog(catalog, CompositionRules::declared()),
        );
        let record = result.unwrap_or_else(|error| panic!("forged status: {error}"));
        assert_eq!(record.execution().outputs(), [WasmValue::I32(-2)]);
        assert!(record.call_graph().edges().is_empty());
        let effects = record
            .effects()
            .unwrap_or_else(|| panic!("success effects"));
        assert!(effects.calls.is_empty());
        assert!(effects.events.is_empty());
        assert!(effects.transfers.is_empty());
        assert!(effects.namespace_drops.is_empty());
        assert_eq!(storage, before);
        assert_eq!(
            storage.namespace_cell_count(StorageNamespace::shared(child)),
            0
        );
    }
}

pub(super) fn crafted_keys_cannot_name_a_foreign_namespace() {
    let (attacker, actor) = ids(45, 53);
    let (victim, _) = ids(46, 53);
    let mut key = StorageNamespace::shared(victim).canonical_bytes();
    key.extend_from_slice(b"/target");
    let mut storage = Storage::new();
    seed(
        &mut storage,
        StorageNamespace::shared(victim),
        &key,
        b"canary",
    );
    execute_candidate(
        &Executor::declared(),
        &scoped_write_guest(2, &key, b"crafted"),
        &mut storage,
        attacker,
        actor,
        CapabilitySet::new([Capability::SharedStorageWrite])
            .unwrap_or_else(|error| panic!("grant: {error}")),
        CompositionContext::isolated(),
    )
    .unwrap_or_else(|error| panic!("crafted write: {error}"));
    assert_eq!(
        storage
            .transaction(StorageNamespace::shared(victim))
            .read(&key),
        Ok(Some(b"canary".to_vec()))
    );
    assert_eq!(
        storage
            .transaction(StorageNamespace::shared(attacker))
            .read(&key),
        Ok(Some(b"crafted".to_vec()))
    );
    let read = execute_candidate_with_response(
        &scoped_read_response_guest(2, &key, 7),
        &mut storage,
        attacker,
        actor,
        CapabilitySet::new([Capability::SharedStorageRead])
            .unwrap_or_else(|error| panic!("crafted read grant: {error}")),
        7,
    );
    assert_eq!(
        read.response()
            .unwrap_or_else(|| panic!("crafted read response"))
            .bytes,
        b"crafted".to_vec()
    );
    assert_eq!(
        storage
            .transaction(StorageNamespace::shared(victim))
            .read(&key),
        Ok(Some(b"canary".to_vec()))
    );
}

pub(super) fn narrowing_never_widens_shared_authority_across_a_call() {
    let (root, actor) = ids(47, 54);
    let (child, _) = ids(48, 54);
    let requested = CapabilitySet::new([Capability::SharedStorageWrite])
        .unwrap_or_else(|error| panic!("requested: {error}"))
        .canonical_encoding();
    let engine = WasmEngine::declared().unwrap_or_else(|error| panic!("engine: {error}"));
    let mut catalog = ProgramCatalog::new();
    catalog.insert(
        child,
        engine
            .validate_candidate_v2(&scoped_write_guest(2, b"target", b"escaped"))
            .unwrap_or_else(|error| panic!("child: {error}")),
    );
    let mut storage = Storage::new();
    let before = storage.clone();
    assert_eq!(
        execute_candidate(
            &Executor::declared(),
            &call_with_capability_bytes(child, &requested),
            &mut storage,
            root,
            actor,
            CapabilitySet::new([
                Capability::Call { program: child },
                Capability::SharedStorageRead,
            ])
            .unwrap_or_else(|error| panic!("parent: {error}")),
            CompositionContext::catalog(catalog, CompositionRules::declared()),
        ),
        Err(ExecutionError::Composition(CompositionRefusal::Authority(
            AbiError::CapabilityDenied,
        )))
    );
    assert_eq!(storage, before);
    assert_eq!(
        storage.namespace_cell_count(StorageNamespace::shared(child)),
        0
    );

    let requested = CapabilitySet::new([Capability::SharedStorageWrite])
        .unwrap_or_else(|error| panic!("requested: {error}"))
        .canonical_encoding();
    let mut catalog = ProgramCatalog::new();
    catalog.insert(
        child,
        engine
            .validate_candidate_v2(&scoped_write_guest(2, b"target", b"delegated"))
            .unwrap_or_else(|error| panic!("child: {error}")),
    );
    let mut delegated_storage = Storage::new();
    seed(
        &mut delegated_storage,
        StorageNamespace::shared(root),
        b"target",
        b"root-canary",
    );
    let delegated = execute_candidate(
        &Executor::declared(),
        &call_with_capability_bytes(child, &requested),
        &mut delegated_storage,
        root,
        actor,
        CapabilitySet::new([
            Capability::Call { program: child },
            Capability::SharedStorageWrite,
        ])
        .unwrap_or_else(|error| panic!("parent: {error}")),
        CompositionContext::catalog(catalog, CompositionRules::declared()),
    )
    .unwrap_or_else(|error| panic!("delegated call: {error}"));
    assert_eq!(delegated.call_graph().edges().len(), 1);
    assert_eq!(
        delegated_storage
            .transaction(StorageNamespace::shared(root))
            .read(b"target"),
        Ok(Some(b"root-canary".to_vec()))
    );
    assert_eq!(
        delegated_storage
            .transaction(StorageNamespace::shared(child))
            .read(b"target"),
        Ok(Some(b"delegated".to_vec()))
    );

    for (parent_grant, requested_grant) in [
        (Capability::StorageWrite, Capability::SharedStorageWrite),
        (Capability::SharedStorageWrite, Capability::StorageWrite),
    ] {
        let requested = CapabilitySet::new([requested_grant])
            .unwrap_or_else(|error| panic!("cross request: {error}"))
            .canonical_encoding();
        let child_wasm = if requested_grant == Capability::StorageWrite {
            scoped_write_guest(1, b"target", b"crossed")
        } else {
            scoped_write_guest(2, b"target", b"crossed")
        };
        let mut catalog = ProgramCatalog::new();
        catalog.insert(
            child,
            engine
                .validate_candidate_v2(&child_wasm)
                .unwrap_or_else(|error| panic!("cross child: {error}")),
        );
        let before = delegated_storage.clone();
        assert_eq!(
            execute_candidate(
                &Executor::declared(),
                &call_with_capability_bytes(child, &requested),
                &mut delegated_storage,
                root,
                actor,
                CapabilitySet::new([Capability::Call { program: child }, parent_grant])
                    .unwrap_or_else(|error| panic!("cross parent: {error}")),
                CompositionContext::catalog(catalog, CompositionRules::declared()),
            ),
            Err(ExecutionError::Composition(CompositionRefusal::Authority(
                AbiError::CapabilityDenied,
            )))
        );
        assert_eq!(delegated_storage, before);
    }
    for (parent_grant, requested_grant, selector) in [
        (Capability::StorageRead, Capability::SharedStorageRead, 2),
        (Capability::SharedStorageRead, Capability::StorageRead, 1),
    ] {
        let requested = CapabilitySet::new([requested_grant])
            .unwrap_or_else(|error| panic!("cross read request: {error}"))
            .canonical_encoding();
        let mut catalog = ProgramCatalog::new();
        catalog.insert(
            child,
            engine
                .validate_candidate_v2(&scoped_read_guest(selector, b"target"))
                .unwrap_or_else(|error| panic!("cross read child: {error}")),
        );
        let before = delegated_storage.clone();
        assert_eq!(
            execute_candidate(
                &Executor::declared(),
                &call_with_capability_bytes(child, &requested),
                &mut delegated_storage,
                root,
                actor,
                CapabilitySet::new([Capability::Call { program: child }, parent_grant])
                    .unwrap_or_else(|error| panic!("cross read parent: {error}")),
                CompositionContext::catalog(catalog, CompositionRules::declared()),
            ),
            Err(ExecutionError::Composition(CompositionRefusal::Authority(
                AbiError::CapabilityDenied,
            )))
        );
        assert_eq!(delegated_storage, before);
    }
}

pub(super) fn shared_surface_cannot_reach_another_principal_cells() {
    let (owner, invoker) = ids(49, 55);
    let (_, victim) = ids(49, 56);
    let key = b"same";
    let mut storage = Storage::new();
    seed(
        &mut storage,
        StorageNamespace::principal(owner, victim),
        key,
        b"principal-secret",
    );
    seed(
        &mut storage,
        StorageNamespace::shared(owner),
        key,
        b"shared",
    );
    let record = execute_candidate(
        &Executor::declared(),
        &scoped_read_guest(2, key),
        &mut storage,
        owner,
        invoker,
        CapabilitySet::new([Capability::SharedStorageRead])
            .unwrap_or_else(|error| panic!("grant: {error}")),
        CompositionContext::isolated(),
    )
    .unwrap_or_else(|error| panic!("shared read: {error}"));
    assert_eq!(record.execution().outputs(), [WasmValue::I32(7)]);
    let published = execute_candidate_with_response(
        &scoped_read_response_guest(2, key, 6),
        &mut storage,
        owner,
        invoker,
        CapabilitySet::new([Capability::SharedStorageRead])
            .unwrap_or_else(|error| panic!("response grant: {error}")),
        6,
    );
    assert_eq!(
        published
            .response()
            .unwrap_or_else(|| panic!("published response"))
            .bytes,
        b"shared".to_vec()
    );
    let write = execute_candidate(
        &Executor::declared(),
        &scoped_write_guest(2, key, b"hostile-shared"),
        &mut storage,
        owner,
        invoker,
        CapabilitySet::new([Capability::SharedStorageWrite])
            .unwrap_or_else(|error| panic!("write grant: {error}")),
        CompositionContext::isolated(),
    )
    .unwrap_or_else(|error| panic!("shared write: {error}"));
    assert_eq!(write.execution().outputs(), [WasmValue::I32(0)]);
    assert_eq!(
        storage
            .transaction(StorageNamespace::shared(owner))
            .read(key),
        Ok(Some(b"hostile-shared".to_vec()))
    );
    assert_eq!(
        storage
            .transaction(StorageNamespace::principal(owner, victim))
            .read(key),
        Ok(Some(b"principal-secret".to_vec()))
    );
}

pub(super) fn repeated_iteration_exhaustion_has_no_partial_output() {
    let (owner, actor) = ids(50, 57);
    let mut storage = Storage::new();
    seed(
        &mut storage,
        StorageNamespace::shared(owner),
        b"a",
        b"value",
    );
    let before = storage.clone();
    let exact = execute_budgeted_candidate(
        &repeated_scan_guest(1),
        &mut storage,
        owner,
        actor,
        CapabilitySet::new([Capability::SharedStorageRead])
            .unwrap_or_else(|error| panic!("exact grant: {error}")),
        declared(17, 0),
        71,
    );
    assert!(matches!(
        exact.outcome(),
        CandidateActivityOutcome::Success { .. }
    ));
    assert_eq!(exact.execution().usage().storage_read_bytes, 17);
    assert_eq!(exact.execution().outputs(), [WasmValue::I32(0)]);
    assert!(exact
        .response()
        .is_some_and(|response| response.bytes.is_empty()));
    assert_eq!(storage, before);
    let result = execute_budgeted_candidate(
        &repeated_scan_guest(2),
        &mut storage,
        owner,
        actor,
        CapabilitySet::new([Capability::SharedStorageRead])
            .unwrap_or_else(|error| panic!("grant: {error}")),
        declared(17, 0),
        72,
    );
    assert_eq!(
        result.outcome(),
        &CandidateActivityOutcome::Resource(BudgetMeterRefusal::BudgetExceeded {
            resource: BudgetResourceKind::StorageRead,
            limit: 17,
            attempted: 34,
        })
    );
    assert_eq!(result.execution().usage().storage_read_bytes, 17);
    assert!(result.response().is_none());
    assert!(result.effects().is_none());
    assert_eq!(storage, before);

    let (foreign, _) = ids(52, 57);
    let genuine_foreign = shared_cursor(foreign, b"a");
    let mut tampered = shared_cursor(owner, b"a");
    tampered[2] ^= 1;
    for cursor in [genuine_foreign, tampered] {
        let cursor_record = execute_candidate(
            &Executor::declared(),
            &scoped_scan_status_guest(2, &cursor),
            &mut storage,
            owner,
            actor,
            CapabilitySet::new([Capability::SharedStorageRead])
                .unwrap_or_else(|error| panic!("cursor grant: {error}")),
            CompositionContext::isolated(),
        )
        .unwrap_or_else(|error| panic!("cursor refusal: {error}"));
        assert_eq!(cursor_record.execution().outputs(), [WasmValue::I32(-2)]);
        assert_eq!(cursor_record.execution().usage().storage_read_bytes, 0);
        assert_eq!(cursor_record.execution().usage().storage_write_bytes, 0);
        assert!(cursor_record
            .response()
            .is_some_and(|response| response.bytes.is_empty()));
        let effects = cursor_record
            .effects()
            .unwrap_or_else(|| panic!("cursor effects"));
        assert!(
            effects.calls.is_empty()
                && effects.events.is_empty()
                && effects.transfers.is_empty()
                && effects.namespace_drops.is_empty()
        );
        assert_eq!(storage, before);
    }
}

pub(super) fn repeated_drop_rewrite_exhaustion_rolls_back_atomically() {
    let (owner, actor) = ids(51, 58);
    let mut storage = Storage::new();
    seed(&mut storage, StorageNamespace::shared(owner), b"k", b"v");
    let before = storage.clone();
    let result = execute_budgeted_candidate(
        &repeated_drop_rewrite_guest(2),
        &mut storage,
        owner,
        actor,
        CapabilitySet::new([Capability::SharedStorageWrite])
            .unwrap_or_else(|error| panic!("grant: {error}")),
        declared(0, 5),
        73,
    );
    assert_eq!(
        result.outcome(),
        &CandidateActivityOutcome::Resource(BudgetMeterRefusal::BudgetExceeded {
            resource: BudgetResourceKind::StorageWrite,
            limit: 5,
            attempted: 8,
        })
    );
    assert_eq!(result.execution().usage().storage_write_bytes, 5);
    assert!(result.response().is_none());
    assert!(result.effects().is_none());
    assert_eq!(storage, before);

    let mut exact_usages = Vec::new();
    for (selector, namespace, capability, binding) in [
        (
            1,
            StorageNamespace::principal(owner, actor),
            Capability::StorageWrite,
            74,
        ),
        (
            2,
            StorageNamespace::shared(owner),
            Capability::SharedStorageWrite,
            75,
        ),
    ] {
        let mut exact_storage = Storage::new();
        seed(&mut exact_storage, namespace, b"k", b"v");
        let exact = execute_budgeted_candidate(
            &repeated_drop_rewrite_guest(selector),
            &mut exact_storage,
            owner,
            actor,
            CapabilitySet::new([capability]).unwrap_or_else(|error| panic!("exact grant: {error}")),
            declared(0, 8),
            binding,
        );
        assert!(matches!(
            exact.outcome(),
            CandidateActivityOutcome::Success { .. }
        ));
        assert_eq!(exact.execution().usage().storage_write_bytes, 8);
        assert_eq!(exact_storage.namespace_cell_count(namespace), 0);
        let drops = &exact
            .effects()
            .unwrap_or_else(|| panic!("exact effects"))
            .namespace_drops;
        assert_eq!(drops.len(), 2);
        for drop in drops {
            assert_eq!(drop.namespace(), namespace);
            assert_eq!(drop.reclaimed_cells(), 1);
            assert_eq!(drop.reclaimed_key_value_bytes(), 2);
            assert_eq!(drop.metered_work(), 3);
        }
        exact_usages.push(exact.execution().usage().storage_write_bytes);
    }
    assert_eq!(exact_usages, [8, 8]);
}

pub(super) fn shared_state_gauntlet_suite() {
    foreign_shared_selectors_are_structurally_closed();
    forged_shared_capability_bytes_never_enter_the_child();
    crafted_keys_cannot_name_a_foreign_namespace();
    narrowing_never_widens_shared_authority_across_a_call();
    shared_surface_cannot_reach_another_principal_cells();
    repeated_iteration_exhaustion_has_no_partial_output();
    repeated_drop_rewrite_exhaustion_rolls_back_atomically();
}

#[test]
fn shared_state_attacks_are_defeated_by_construction() {
    shared_state_gauntlet_suite();
}
