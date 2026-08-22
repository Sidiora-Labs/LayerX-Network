#![allow(clippy::too_many_lines)]

use layerx_programs_runtime::abi::response::CANDIDATE_ABI_MODULE;
use layerx_programs_runtime::test_support::{
    code_section, func_body, function_section, import_section, module, type_section, unsigned_leb,
    TYPE_I32,
};
use layerx_programs_runtime::{
    AbiError, AuthorizationContext, AuthorizedExecutionRequest, CandidateActivityOutcome,
    CandidateAuthorizedExecutionRecord, Capability, CapabilitySet, CompositionContext,
    ExecutionError, Executor, FeeSchedule, MeterRefusal, NamespaceDrop, PrincipalId, ProgramId,
    ReceiptOracle, ReceiptView, ResourceBudget, ResourceKind, Storage, StorageNamespace,
    WasmEngine, WasmValue, CALL_ENTRY_EXPORT,
};

/// Task 29.4's boundary fixture, not a protocol namespace-size policy.
const SIXTY_FOUR_CELL_BOUNDARY_FIXTURE: u8 = 64;

struct NoReceipts;

impl ReceiptOracle for NoReceipts {
    fn verified_receipt(&self, _: [u8; 32]) -> Result<ReceiptView, AbiError> {
        Err(AbiError::ReceiptMismatch)
    }
}

fn program(byte: u8) -> ProgramId {
    ProgramId::new([byte; 32]).unwrap_or_else(|error| panic!("program: {error}"))
}

fn principal(byte: u8) -> PrincipalId {
    PrincipalId::new([byte; 32]).unwrap_or_else(|error| panic!("principal: {error}"))
}

fn capabilities(grants: impl IntoIterator<Item = Capability>) -> CapabilitySet {
    CapabilitySet::new(grants).unwrap_or_else(|error| panic!("capabilities: {error}"))
}

fn signed_leb(mut value: i32) -> Vec<u8> {
    let mut bytes = Vec::new();
    loop {
        let byte = (value as u8) & 0x7f;
        value >>= 7;
        let done = (value == 0 && byte & 0x40 == 0) || (value == -1 && byte & 0x40 != 0);
        bytes.push(if done { byte } else { byte | 0x80 });
        if done {
            return bytes;
        }
    }
}

fn section(id: u8, payload: &[u8]) -> Vec<u8> {
    let mut bytes = vec![id];
    bytes.extend(unsigned_leb(payload.len() as u64));
    bytes.extend_from_slice(payload);
    bytes
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

fn memory_and_exports() -> (Vec<u8>, Vec<u8>) {
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
    (memory, section(7, &exports))
}

fn push_i32(body: &mut Vec<u8>, value: i32) {
    body.push(0x41);
    body.extend(signed_leb(value));
}

enum GuestOperation<'a> {
    Drop(i32),
    Write {
        selector: i32,
        key: &'a [u8],
        value: &'a [u8],
    },
    Trap,
}

fn namespace_guest(operations: &[GuestOperation<'_>]) -> Vec<u8> {
    let types = type_section(&[
        (&[TYPE_I32], &[TYPE_I32]),
        (&[TYPE_I32; 5], &[TYPE_I32]),
        (&[TYPE_I32], &[TYPE_I32]),
        (&[TYPE_I32, TYPE_I32], &[TYPE_I32]),
    ]);
    let imports = import_section(&[
        (CANDIDATE_ABI_MODULE, "storage_drop_scoped", 0),
        (CANDIDATE_ABI_MODULE, "storage_write_scoped", 1),
    ]);
    let (memory, exports) = memory_and_exports();
    let mut body = Vec::new();
    let mut data = Vec::new();
    let mut next_offset = 0i32;
    for operation in operations {
        match operation {
            GuestOperation::Drop(selector) => {
                push_i32(&mut body, *selector);
                body.extend_from_slice(&[0x10, 0, 0x1a]);
            }
            GuestOperation::Write {
                selector,
                key,
                value,
            } => {
                let key_offset = next_offset;
                data.push((key_offset, *key));
                next_offset += i32::try_from(key.len()).unwrap_or(i32::MAX) + 16;
                let value_offset = next_offset;
                data.push((value_offset, *value));
                next_offset += i32::try_from(value.len()).unwrap_or(i32::MAX) + 16;
                for value in [
                    *selector,
                    key_offset,
                    i32::try_from(key.len()).unwrap_or(i32::MAX),
                    value_offset,
                    i32::try_from(value.len()).unwrap_or(i32::MAX),
                ] {
                    push_i32(&mut body, value);
                }
                body.extend_from_slice(&[0x10, 1, 0x1a]);
            }
            GuestOperation::Trap => body.push(0x00),
        }
    }
    push_i32(&mut body, 0);
    body.push(0x0b);
    module(&[
        types,
        imports,
        function_section(&[2, 3]),
        memory,
        exports,
        code_section(&[func_body(&[], &[0x41, 0, 0x0b]), func_body(&[], &body)]),
        data_section(&data),
    ])
}

fn drop_status_guest(selector: i32) -> Vec<u8> {
    let types = type_section(&[
        (&[TYPE_I32], &[TYPE_I32]),
        (&[TYPE_I32], &[TYPE_I32]),
        (&[TYPE_I32, TYPE_I32], &[TYPE_I32]),
    ]);
    let imports = import_section(&[(CANDIDATE_ABI_MODULE, "storage_drop_scoped", 0)]);
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
    push_i32(&mut entry, selector);
    entry.extend_from_slice(&[0x10, 0, 0x0b]);
    module(&[
        types,
        imports,
        function_section(&[1, 2]),
        memory,
        section(7, &exports),
        code_section(&[func_body(&[], &[0x41, 0, 0x0b]), func_body(&[], &entry)]),
    ])
}

fn drop_then_status_guest(first_selector: i32, refused_selector: i32) -> Vec<u8> {
    let types = type_section(&[
        (&[TYPE_I32], &[TYPE_I32]),
        (&[TYPE_I32], &[TYPE_I32]),
        (&[TYPE_I32, TYPE_I32], &[TYPE_I32]),
    ]);
    let imports = import_section(&[(CANDIDATE_ABI_MODULE, "storage_drop_scoped", 0)]);
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
    push_i32(&mut entry, first_selector);
    entry.extend_from_slice(&[0x10, 0, 0x1a]);
    push_i32(&mut entry, refused_selector);
    entry.extend_from_slice(&[0x10, 0, 0x0b]);
    module(&[
        types,
        imports,
        function_section(&[1, 2]),
        memory,
        section(7, &exports),
        code_section(&[func_body(&[], &[0x41, 0, 0x0b]), func_body(&[], &entry)]),
    ])
}

fn execute(
    executor: &Executor,
    wasm: &[u8],
    owner: ProgramId,
    actor: PrincipalId,
    grants: CapabilitySet,
    storage: &mut Storage,
) -> Result<CandidateAuthorizedExecutionRecord, ExecutionError> {
    let engine = WasmEngine::declared().unwrap_or_else(|error| panic!("engine: {error}"));
    let module = engine
        .validate_candidate_v2(wasm)
        .unwrap_or_else(|error| panic!("candidate: {error}"));
    executor.execute_authorized_candidate(
        storage,
        AuthorizedExecutionRequest {
            module: &module,
            program: owner,
            authorization: AuthorizationContext::new(actor, grants),
            receipts: &NoReceipts,
            entrypoint: CALL_ENTRY_EXPORT,
            calldata: &[],
            composition: CompositionContext::isolated(),
            response_capacity: 0,
        },
    )
}

fn seed(storage: &mut Storage, namespace: StorageNamespace, key: &[u8], value: &[u8]) {
    let mut transaction = storage.transaction(namespace);
    transaction
        .write(key, value)
        .unwrap_or_else(|error| panic!("seed write: {error}"));
    assert_eq!(transaction.commit(), 1);
}

fn drops(record: &CandidateAuthorizedExecutionRecord) -> &[NamespaceDrop] {
    match record.outcome() {
        CandidateActivityOutcome::Success { effects, .. } => effects.namespace_drops.as_slice(),
        outcome => panic!("expected success, got {outcome:?}"),
    }
}

fn assert_failure_has_no_committed_effects(record: &CandidateAuthorizedExecutionRecord) {
    assert!(matches!(
        record.outcome(),
        CandidateActivityOutcome::Failure(_)
    ));
    assert_eq!(record.effects(), None);
}

#[test]
fn candidate_drop_of_empty_namespace_records_zero_provisional_reclamation_fact() {
    let owner = program(1);
    let actor = principal(1);
    let mut storage = Storage::new();
    let record = execute(
        &Executor::declared(),
        &namespace_guest(&[GuestOperation::Drop(1)]),
        owner,
        actor,
        capabilities([Capability::StorageWrite]),
        &mut storage,
    )
    .unwrap_or_else(|error| panic!("empty drop: {error}"));
    assert_eq!(record.execution().outputs(), [WasmValue::I32(0)]);
    assert_eq!(record.execution().usage().storage_write_bytes, 0);
    assert_eq!(drops(&record).len(), 1);
    assert_eq!(
        drops(&record)[0].namespace(),
        StorageNamespace::principal(owner, actor)
    );
    assert_eq!(drops(&record)[0].reclaimed_cells(), 0);
    assert_eq!(drops(&record)[0].reclaimed_key_value_bytes(), 0);
    assert_eq!(drops(&record)[0].metered_work(), 0);
}

#[test]
fn candidate_drop_reclaims_every_cell_in_the_sixty_four_cell_boundary_fixture() {
    let owner = program(2);
    let actor = principal(2);
    let namespace = StorageNamespace::principal(owner, actor);
    let mut storage = Storage::new();
    let mut expected_bytes = 0u64;
    for cell in 0..SIXTY_FOUR_CELL_BOUNDARY_FIXTURE {
        let key = [b'k', cell];
        let value = [b'v', cell, cell.wrapping_add(1)];
        expected_bytes += u64::try_from(key.len() + value.len()).unwrap_or(u64::MAX);
        seed(&mut storage, namespace, &key, &value);
    }
    let record = execute(
        &Executor::declared(),
        &namespace_guest(&[GuestOperation::Drop(1)]),
        owner,
        actor,
        capabilities([Capability::StorageWrite]),
        &mut storage,
    )
    .unwrap_or_else(|error| panic!("ceiling drop: {error}"));
    assert_eq!(storage.namespace_cell_count(namespace), 0);
    assert_eq!(
        drops(&record)[0].reclaimed_cells(),
        u64::from(SIXTY_FOUR_CELL_BOUNDARY_FIXTURE)
    );
    assert_eq!(
        drops(&record)[0].reclaimed_key_value_bytes(),
        expected_bytes
    );
    assert_eq!(
        record.execution().usage().storage_write_bytes,
        expected_bytes + u64::from(SIXTY_FOUR_CELL_BOUNDARY_FIXTURE)
    );
}

#[test]
fn candidate_drop_then_write_and_write_then_drop_have_deterministic_ordering() {
    let owner = program(3);
    let actor = principal(3);
    let namespace = StorageNamespace::principal(owner, actor);
    let grants = capabilities([Capability::StorageWrite]);

    let mut drop_then_write = Storage::new();
    seed(&mut drop_then_write, namespace, b"old", b"value");
    let record = execute(
        &Executor::declared(),
        &namespace_guest(&[
            GuestOperation::Drop(1),
            GuestOperation::Write {
                selector: 1,
                key: b"fresh",
                value: b"value",
            },
        ]),
        owner,
        actor,
        grants.clone(),
        &mut drop_then_write,
    )
    .unwrap_or_else(|error| panic!("drop then write: {error}"));
    assert_eq!(drops(&record)[0].reclaimed_key_value_bytes(), 8);
    assert_eq!(
        drop_then_write.transaction(namespace).read(b"fresh"),
        Ok(Some(b"value".to_vec()))
    );
    assert_eq!(
        drop_then_write.transaction(namespace).read(b"old"),
        Ok(None)
    );

    let mut write_then_drop = Storage::new();
    seed(&mut write_then_drop, namespace, b"old", b"value");
    let record = execute(
        &Executor::declared(),
        &namespace_guest(&[
            GuestOperation::Write {
                selector: 1,
                key: b"fresh",
                value: b"value",
            },
            GuestOperation::Drop(1),
        ]),
        owner,
        actor,
        grants,
        &mut write_then_drop,
    )
    .unwrap_or_else(|error| panic!("write then drop: {error}"));
    assert_eq!(storage_cell_count(&write_then_drop, namespace), 0);
    assert_eq!(drops(&record)[0].reclaimed_cells(), 2);
    assert_eq!(drops(&record)[0].reclaimed_key_value_bytes(), 18);
}

#[test]
fn candidate_later_fault_discards_namespace_drop_and_provisional_reclamation_fact() {
    let owner = program(4);
    let actor = principal(4);
    let namespace = StorageNamespace::principal(owner, actor);
    let mut storage = Storage::new();
    seed(&mut storage, namespace, b"keep", b"this");
    let before = storage.clone();
    let record = execute(
        &Executor::declared(),
        &namespace_guest(&[GuestOperation::Drop(1), GuestOperation::Trap]),
        owner,
        actor,
        capabilities([Capability::StorageWrite]),
        &mut storage,
    )
    .unwrap_or_else(|error| panic!("rollback candidate: {error}"));
    assert_failure_has_no_committed_effects(&record);
    assert_eq!(storage, before);
}

#[test]
fn candidate_later_typed_host_refusal_discards_namespace_drop_and_effects() {
    let owner = program(9);
    let actor = principal(9);
    let namespace = StorageNamespace::principal(owner, actor);
    let mut storage = Storage::new();
    seed(&mut storage, namespace, b"keep", b"this");
    let before = storage.clone();
    let record = execute(
        &Executor::declared(),
        &drop_then_status_guest(1, 0),
        owner,
        actor,
        capabilities([Capability::StorageWrite]),
        &mut storage,
    )
    .unwrap_or_else(|error| panic!("typed host rollback: {error}"));
    assert_eq!(record.execution().outputs(), [WasmValue::I32(-2)]);
    assert_failure_has_no_committed_effects(&record);
    assert_eq!(storage, before);
}

#[test]
fn candidate_denied_and_invalid_selectors_do_not_reclaim_or_meter() {
    let owner = program(5);
    let actor = principal(5);
    let namespace = StorageNamespace::principal(owner, actor);
    for (selector, grants, expected) in [
        (-1, capabilities([Capability::StorageWrite]), -2),
        (0, capabilities([Capability::StorageWrite]), -2),
        (3, capabilities([Capability::StorageWrite]), -2),
        (i32::MAX, capabilities([Capability::StorageWrite]), -2),
        (1, capabilities([Capability::SharedStorageWrite]), -1),
        (2, capabilities([Capability::StorageWrite]), -1),
    ] {
        let mut storage = Storage::new();
        seed(&mut storage, namespace, b"keep", b"this");
        let before = storage.clone();
        let record = execute(
            &Executor::declared(),
            &drop_status_guest(selector),
            owner,
            actor,
            grants,
            &mut storage,
        )
        .unwrap_or_else(|error| panic!("refused drop: {error}"));
        assert_eq!(record.execution().outputs(), [WasmValue::I32(expected)]);
        assert_eq!(record.execution().usage().storage_write_bytes, 0);
        assert_failure_has_no_committed_effects(&record);
        assert_eq!(storage, before);
    }
}

#[test]
fn candidate_drop_meter_is_exact_and_one_past_refuses_before_mutation() {
    let owner = program(6);
    let actor = principal(6);
    let namespace = StorageNamespace::principal(owner, actor);
    let wasm = namespace_guest(&[GuestOperation::Drop(1)]);
    let exact_work = 5;
    let exact_executor = Executor::new(
        ResourceBudget::new(
            1_000_000,
            16 * 1_024 * 1_024,
            1_048_576,
            exact_work,
            64,
            4_096,
        ),
        FeeSchedule::declared(),
    );
    let mut exact_storage = Storage::new();
    seed(&mut exact_storage, namespace, b"a", b"one");
    let record = execute(
        &exact_executor,
        &wasm,
        owner,
        actor,
        capabilities([Capability::StorageWrite]),
        &mut exact_storage,
    )
    .unwrap_or_else(|error| panic!("exact meter: {error}"));
    assert_eq!(record.execution().usage().storage_write_bytes, exact_work);
    assert_eq!(storage_cell_count(&exact_storage, namespace), 0);

    let one_past_executor = Executor::new(
        ResourceBudget::new(
            1_000_000,
            16 * 1_024 * 1_024,
            1_048_576,
            exact_work - 1,
            64,
            4_096,
        ),
        FeeSchedule::declared(),
    );
    let mut rejected_storage = Storage::new();
    seed(&mut rejected_storage, namespace, b"a", b"one");
    let before = rejected_storage.clone();
    assert_eq!(
        execute(
            &one_past_executor,
            &wasm,
            owner,
            actor,
            capabilities([Capability::StorageWrite]),
            &mut rejected_storage,
        ),
        Err(ExecutionError::Resource(MeterRefusal::BudgetExceeded {
            resource: ResourceKind::StorageWrite,
            limit: exact_work - 1,
            attempted: exact_work,
        }))
    );
    assert_eq!(rejected_storage, before);
}

#[test]
fn candidate_drop_meter_distinguishes_cells_and_key_value_bytes() {
    let owner = program(10);
    let actor = principal(10);
    let namespace = StorageNamespace::principal(owner, actor);

    let mut one_small_cell = Storage::new();
    seed(&mut one_small_cell, namespace, b"a", b"1");
    let small = execute(
        &Executor::declared(),
        &namespace_guest(&[GuestOperation::Drop(1)]),
        owner,
        actor,
        capabilities([Capability::StorageWrite]),
        &mut one_small_cell,
    )
    .unwrap_or_else(|error| panic!("small-cell drop: {error}"));

    let mut one_large_cell = Storage::new();
    seed(&mut one_large_cell, namespace, b"abc", b"def");
    let large = execute(
        &Executor::declared(),
        &namespace_guest(&[GuestOperation::Drop(1)]),
        owner,
        actor,
        capabilities([Capability::StorageWrite]),
        &mut one_large_cell,
    )
    .unwrap_or_else(|error| panic!("large-cell drop: {error}"));
    assert_eq!(drops(&small)[0].reclaimed_cells(), 1);
    assert_eq!(drops(&large)[0].reclaimed_cells(), 1);
    assert_eq!(drops(&small)[0].reclaimed_key_value_bytes(), 2);
    assert_eq!(drops(&large)[0].reclaimed_key_value_bytes(), 6);
    assert_eq!(small.execution().usage().storage_write_bytes, 3);
    assert_eq!(large.execution().usage().storage_write_bytes, 7);

    let mut one_cell_same_bytes = Storage::new();
    seed(&mut one_cell_same_bytes, namespace, b"a", b"xyz");
    let one_cell = execute(
        &Executor::declared(),
        &namespace_guest(&[GuestOperation::Drop(1)]),
        owner,
        actor,
        capabilities([Capability::StorageWrite]),
        &mut one_cell_same_bytes,
    )
    .unwrap_or_else(|error| panic!("one-cell same-bytes drop: {error}"));

    let mut two_cells_same_bytes = Storage::new();
    seed(&mut two_cells_same_bytes, namespace, b"a", b"1");
    seed(&mut two_cells_same_bytes, namespace, b"b", b"2");
    let two_cells = execute(
        &Executor::declared(),
        &namespace_guest(&[GuestOperation::Drop(1)]),
        owner,
        actor,
        capabilities([Capability::StorageWrite]),
        &mut two_cells_same_bytes,
    )
    .unwrap_or_else(|error| panic!("two-cell same-bytes drop: {error}"));
    assert_eq!(drops(&one_cell)[0].reclaimed_key_value_bytes(), 4);
    assert_eq!(drops(&two_cells)[0].reclaimed_key_value_bytes(), 4);
    assert_eq!(drops(&one_cell)[0].reclaimed_cells(), 1);
    assert_eq!(drops(&two_cells)[0].reclaimed_cells(), 2);
    assert_eq!(one_cell.execution().usage().storage_write_bytes, 5);
    assert_eq!(two_cells.execution().usage().storage_write_bytes, 6);
}

#[test]
fn candidate_drop_preserves_every_adjacent_program_principal_and_scope() {
    let owner = program(7);
    let other_program = program(8);
    let actor = principal(7);
    let other_actor = principal(8);
    let owned_principal = StorageNamespace::principal(owner, actor);
    let namespaces = [
        (owned_principal, b"owned-principal".as_slice()),
        (StorageNamespace::shared(owner), b"owned-shared".as_slice()),
        (
            StorageNamespace::principal(owner, other_actor),
            b"other-principal".as_slice(),
        ),
        (
            StorageNamespace::principal(other_program, actor),
            b"other-program-principal".as_slice(),
        ),
        (
            StorageNamespace::shared(other_program),
            b"other-program-shared".as_slice(),
        ),
    ];
    let mut storage = Storage::new();
    for (namespace, value) in namespaces {
        seed(&mut storage, namespace, b"same", value);
    }
    let record = execute(
        &Executor::declared(),
        &namespace_guest(&[GuestOperation::Drop(1)]),
        owner,
        actor,
        capabilities([Capability::StorageWrite]),
        &mut storage,
    )
    .unwrap_or_else(|error| panic!("principal isolation: {error}"));
    assert_eq!(drops(&record)[0].namespace(), owned_principal);
    assert_eq!(storage_cell_count(&storage, owned_principal), 0);
    for (namespace, expected) in namespaces.into_iter().skip(1) {
        assert_eq!(
            storage.transaction(namespace).read(b"same"),
            Ok(Some(expected.to_vec()))
        );
    }

    let record = execute(
        &Executor::declared(),
        &namespace_guest(&[GuestOperation::Drop(2)]),
        owner,
        actor,
        capabilities([Capability::SharedStorageWrite]),
        &mut storage,
    )
    .unwrap_or_else(|error| panic!("shared isolation: {error}"));
    assert_eq!(
        drops(&record)[0].namespace(),
        StorageNamespace::shared(owner)
    );
    assert_eq!(
        storage_cell_count(&storage, StorageNamespace::shared(owner)),
        0
    );
    for (namespace, expected) in namespaces.into_iter().skip(2) {
        assert_eq!(
            storage.transaction(namespace).read(b"same"),
            Ok(Some(expected.to_vec()))
        );
    }
}

fn storage_cell_count(storage: &Storage, namespace: StorageNamespace) -> usize {
    storage.namespace_cell_count(namespace)
}
