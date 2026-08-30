use layerx_programs_runtime::test_support::{
    code_section, func_body, function_section, import_section, module, type_section, unsigned_leb,
    TYPE_I32,
};
use layerx_programs_runtime::{
    derive_program_account, AbiError, ActivityBudgetBinding, AuthorizationContext,
    AuthorizedExecutionRequest, BudgetMeterRefusal, BudgetResourceKind,
    BudgetedAuthorizedExecutionRequest, BudgetedV1ActivityOutcome, BudgetedV1FailureCause,
    Capability, CapabilitySet, CompositionContext, CompositionRefusal, CompositionRules,
    DeclaredBudget, ExecutionError, Executor, FeeSchedule, MeterRefusal, PrincipalId,
    ProgramCatalog, ProgramId, ReceiptOracle, ReceiptView, ResourceBudget, ResourceKind, Storage,
    StorageNamespace, WasmEngine, ABI_MODULE, CALL_ENTRY_EXPORT,
};

const ASSET: [u8; 32] = [0xa5; 32];
const RECIPIENT: [u8; 32] = [0x5a; 32];
const ATTACK_INVENTORY: &str = include_str!("../../../tests/gauntlet/attack-inventory.tsv");
type ProgramModule = (ProgramId, Vec<u8>);
type ProgramTopology = (Vec<u8>, Vec<ProgramModule>, CapabilitySet);
type EdgeTopology = (ProgramId, Vec<u8>, Vec<ProgramModule>, CapabilitySet);

#[derive(Debug)]
struct NoReceipts;

impl ReceiptOracle for NoReceipts {
    fn verified_receipt(
        &self,
        _: [u8; 32],
    ) -> Result<ReceiptView, layerx_programs_runtime::AbiError> {
        Err(layerx_programs_runtime::AbiError::ReceiptMismatch)
    }
}

fn id(byte: u8) -> ProgramId {
    ProgramId::new([byte; 32]).unwrap_or_else(|error| panic!("program: {error}"))
}

fn principal(byte: u8) -> PrincipalId {
    PrincipalId::new([byte; 32]).unwrap_or_else(|error| panic!("principal: {error}"))
}

fn section(id: u8, payload: &[u8]) -> Vec<u8> {
    let mut encoded = vec![id];
    encoded.extend(unsigned_leb(payload.len() as u64));
    encoded.extend_from_slice(payload);
    encoded
}

fn data_section(entries: &[(u32, &[u8])]) -> Vec<u8> {
    let mut payload = unsigned_leb(entries.len() as u64);
    for (offset, bytes) in entries {
        payload.push(0);
        payload.push(0x41);
        payload.extend(signed_leb_i32(
            i32::try_from(*offset).unwrap_or_else(|error| panic!("data offset: {error}")),
        ));
        payload.push(0x0b);
        payload.extend(unsigned_leb(bytes.len() as u64));
        payload.extend_from_slice(bytes);
    }
    section(11, &payload)
}

fn exports(entries: &[(&str, u8, u8)]) -> Vec<u8> {
    let mut payload = unsigned_leb(entries.len() as u64);
    for (name, kind, index) in entries {
        payload.extend(unsigned_leb(name.len() as u64));
        payload.extend_from_slice(name.as_bytes());
        payload.extend_from_slice(&[*kind, *index]);
    }
    section(7, &payload)
}

fn signed_leb_i32(value: i32) -> Vec<u8> {
    let mut value = i64::from(value);
    let mut bytes = Vec::new();
    loop {
        let byte =
            u8::try_from(value & 0x7f).unwrap_or_else(|error| panic!("signed LEB byte: {error}"));
        value >>= 7;
        let done = (value == 0 && byte & 0x40 == 0) || (value == -1 && byte & 0x40 != 0);
        bytes.push(if done { byte } else { byte | 0x80 });
        if done {
            return bytes;
        }
    }
}

fn push_i32(instructions: &mut Vec<u8>, value: i32) {
    instructions.push(0x41);
    instructions.extend(signed_leb_i32(value));
}

fn common_capabilities(calls: impl IntoIterator<Item = ProgramId>) -> CapabilitySet {
    let mut capabilities = vec![
        Capability::StorageWrite,
        Capability::EmitEvent,
        Capability::Transfer402 {
            asset: ASSET,
            to: RECIPIENT,
            maximum_amount: 100,
        },
    ];
    capabilities.extend(
        calls
            .into_iter()
            .map(|program| Capability::Call { program }),
    );
    CapabilitySet::new(capabilities).unwrap_or_else(|error| panic!("capabilities: {error}"))
}

fn staging_program(marker: u8, calls: &[(ProgramId, CapabilitySet)]) -> Vec<u8> {
    let mut instructions = Vec::new();
    for value in [0, 1, 1, 1] {
        push_i32(&mut instructions, value);
    }
    instructions.extend_from_slice(&[0x10, 0, 0x1a]);
    for value in [2, 1, 3, 1] {
        push_i32(&mut instructions, value);
    }
    instructions.extend_from_slice(&[0x10, 1, 0x1a, 0x42, 0, 0x42, 1]);
    for value in [8, 32, 40, 32] {
        push_i32(&mut instructions, value);
    }
    instructions.extend_from_slice(&[0x10, 3, 0x1a]);

    let mut owned_data = vec![
        (0, b"k".to_vec()),
        (1, vec![marker]),
        (2, b"t".to_vec()),
        (3, vec![marker]),
        (8, ASSET.to_vec()),
        (40, RECIPIENT.to_vec()),
    ];
    let mut offset = 80u32;
    let encodings: Vec<_> = calls
        .iter()
        .map(|(_, capabilities)| capabilities.canonical_encoding())
        .collect();
    for ((callee, _), encoded) in calls.iter().zip(&encodings) {
        let program_offset = offset;
        offset = offset.saturating_add(32);
        let capabilities_offset = offset;
        offset = offset.saturating_add(
            u32::try_from(encoded.len()).unwrap_or_else(|error| panic!("encoding length: {error}")),
        );
        owned_data.push((program_offset, callee.bytes().to_vec()));
        owned_data.push((capabilities_offset, encoded.clone()));
        for value in [
            i32::try_from(program_offset).unwrap_or(i32::MAX),
            32,
            0,
            0,
            i32::try_from(capabilities_offset).unwrap_or(i32::MAX),
            i32::try_from(encoded.len()).unwrap_or(i32::MAX),
        ] {
            push_i32(&mut instructions, value);
        }
        instructions.extend_from_slice(&[0x10, 2, 0x1a]);
    }
    push_i32(&mut instructions, 0);
    instructions.push(0x0b);

    let data: Vec<_> = owned_data
        .iter()
        .map(|(offset, bytes)| (*offset, bytes.as_slice()))
        .collect();
    module(&[
        type_section(&[
            (&[TYPE_I32; 4], &[TYPE_I32]),
            (&[TYPE_I32; 6], &[TYPE_I32]),
            (
                &[0x7e, 0x7e, TYPE_I32, TYPE_I32, TYPE_I32, TYPE_I32],
                &[TYPE_I32],
            ),
            (&[TYPE_I32], &[TYPE_I32]),
            (&[TYPE_I32, TYPE_I32], &[TYPE_I32]),
        ]),
        import_section(&[
            (ABI_MODULE, "storage_write", 0),
            (ABI_MODULE, "event_emit", 0),
            (ABI_MODULE, "program_call", 1),
            (ABI_MODULE, "transfer_402", 2),
        ]),
        function_section(&[3, 4]),
        section(5, &[1, 1, 1, 1]),
        exports(&[
            ("layerx_reserve", 0, 4),
            (CALL_ENTRY_EXPORT, 0, 5),
            ("memory", 2, 0),
        ]),
        code_section(&[
            func_body(&[], &[0x41, 0, 0x0b]),
            func_body(&[], &instructions),
        ]),
        data_section(&data),
    ])
}

fn forwarding_program(callee: ProgramId, requested: &CapabilitySet) -> Vec<u8> {
    let encoded = requested.canonical_encoding();
    let mut entry = Vec::new();
    for value in [
        0,
        32,
        32,
        0,
        32,
        i32::try_from(encoded.len()).unwrap_or(i32::MAX),
    ] {
        push_i32(&mut entry, value);
    }
    entry.extend([0x10, 0, 0x0b]);
    module(&[
        type_section(&[
            (&[TYPE_I32; 6], &[TYPE_I32]),
            (&[TYPE_I32], &[TYPE_I32]),
            (&[TYPE_I32, TYPE_I32], &[TYPE_I32]),
        ]),
        import_section(&[(ABI_MODULE, "program_call", 0)]),
        function_section(&[1, 2]),
        section(5, &[1, 1, 1, 1]),
        exports(&[
            ("layerx_reserve", 0, 1),
            (CALL_ENTRY_EXPORT, 0, 2),
            ("memory", 2, 0),
        ]),
        code_section(&[func_body(&[], &[0x41, 0, 0x0b]), func_body(&[], &entry)]),
        data_section(&[(0, &callee.bytes()), (32, &encoded)]),
    ])
}

fn event_chain_program(event_count: usize, child: Option<(ProgramId, &CapabilitySet)>) -> Vec<u8> {
    let mut entry = Vec::new();
    for _ in 0..event_count {
        for value in [0, 1, 1, 0] {
            push_i32(&mut entry, value);
        }
        entry.extend([0x10, 0, 0x1a]);
    }
    let mut data = vec![(0_u32, vec![1_u8])];
    if let Some((callee, requested)) = child {
        let encoded = requested.canonical_encoding();
        data.push((32, callee.bytes().to_vec()));
        data.push((64, encoded.clone()));
        for value in [
            32,
            32,
            0,
            0,
            64,
            i32::try_from(encoded.len()).unwrap_or(i32::MAX),
        ] {
            push_i32(&mut entry, value);
        }
        entry.extend([0x10, 1]);
    } else {
        push_i32(&mut entry, 0);
    }
    entry.push(0x0b);
    let data = data
        .iter()
        .map(|(offset, bytes)| (*offset, bytes.as_slice()))
        .collect::<Vec<_>>();
    module(&[
        type_section(&[
            (&[TYPE_I32; 4], &[TYPE_I32]),
            (&[TYPE_I32; 6], &[TYPE_I32]),
            (&[TYPE_I32], &[TYPE_I32]),
            (&[TYPE_I32, TYPE_I32], &[TYPE_I32]),
        ]),
        import_section(&[
            (ABI_MODULE, "event_emit", 0),
            (ABI_MODULE, "program_call", 1),
        ]),
        function_section(&[2, 3]),
        section(5, &[1, 1, 1, 1]),
        exports(&[
            ("layerx_reserve", 0, 2),
            (CALL_ENTRY_EXPORT, 0, 3),
            ("memory", 2, 0),
        ]),
        code_section(&[
            func_body(&[], &[0x41, 0, 0x0b]),
            func_body(&[], &entry),
        ]),
        data_section(&data),
    ])
}

fn trapping_start_program() -> Vec<u8> {
    module(&[
        type_section(&[
            (&[TYPE_I32], &[TYPE_I32]),
            (&[TYPE_I32, TYPE_I32], &[TYPE_I32]),
            (&[], &[]),
        ]),
        function_section(&[0, 1, 2]),
        section(5, &[1, 1, 1, 1]),
        exports(&[
            ("layerx_reserve", 0, 0),
            (CALL_ENTRY_EXPORT, 0, 1),
            ("memory", 2, 0),
        ]),
        section(8, &[2]),
        code_section(&[
            func_body(&[], &[0x41, 0, 0x0b]),
            func_body(&[], &[0x41, 0, 0x0b]),
            func_body(&[], &[0x00, 0x0b]),
        ]),
    ])
}

fn start_forwarding_program(calls: &[(ProgramId, CapabilitySet)]) -> Vec<u8> {
    let mut start = Vec::new();
    let mut owned_data = Vec::new();
    let mut offset = 0u32;
    for (callee, capabilities) in calls {
        let encoded = capabilities.canonical_encoding();
        let program_offset = offset;
        offset = offset.saturating_add(32);
        let capabilities_offset = offset;
        offset = offset.saturating_add(
            u32::try_from(encoded.len()).unwrap_or_else(|error| panic!("encoding: {error}")),
        );
        owned_data.push((program_offset, callee.bytes().to_vec()));
        owned_data.push((capabilities_offset, encoded.clone()));
        for value in [
            i32::try_from(program_offset).unwrap_or(i32::MAX),
            32,
            0,
            0,
            i32::try_from(capabilities_offset).unwrap_or(i32::MAX),
            i32::try_from(encoded.len()).unwrap_or(i32::MAX),
        ] {
            push_i32(&mut start, value);
        }
        start.extend([0x10, 0, 0x1a]);
    }
    start.push(0x0b);
    let data = owned_data
        .iter()
        .map(|(offset, bytes)| (*offset, bytes.as_slice()))
        .collect::<Vec<_>>();
    module(&[
        type_section(&[
            (&[TYPE_I32; 6], &[TYPE_I32]),
            (&[TYPE_I32], &[TYPE_I32]),
            (&[TYPE_I32, TYPE_I32], &[TYPE_I32]),
            (&[], &[]),
        ]),
        import_section(&[(ABI_MODULE, "program_call", 0)]),
        function_section(&[1, 2, 3]),
        section(5, &[1, 1, 1, 1]),
        exports(&[
            ("layerx_reserve", 0, 1),
            (CALL_ENTRY_EXPORT, 0, 2),
            ("memory", 2, 0),
        ]),
        section(8, &[3]),
        code_section(&[
            func_body(&[], &[0x41, 0, 0x0b]),
            func_body(&[], &[0x41, 0, 0x0b]),
            func_body(&[], &start),
        ]),
        data_section(&data),
    ])
}

fn execute_budgeted(
    root_program: ProgramId,
    root_wasm: &[u8],
    children: &[(ProgramId, Vec<u8>)],
    grants: CapabilitySet,
    rules: CompositionRules,
) -> (
    Result<BudgetedV1ActivityOutcome, layerx_programs_runtime::ExecutionError>,
    Storage,
) {
    execute_budgeted_with_storage(
        root_program,
        root_wasm,
        children,
        grants,
        rules,
        Storage::new(),
    )
}

fn execute_budgeted_with_storage(
    root_program: ProgramId,
    root_wasm: &[u8],
    children: &[(ProgramId, Vec<u8>)],
    grants: CapabilitySet,
    rules: CompositionRules,
    mut storage: Storage,
) -> (
    Result<BudgetedV1ActivityOutcome, layerx_programs_runtime::ExecutionError>,
    Storage,
) {
    let executor = Executor::declared();
    let payer = principal(9);
    let binding =
        ActivityBudgetBinding::new([10; 32]).unwrap_or_else(|error| panic!("binding: {error}"));
    let declared = DeclaredBudget::new(
        1_000_000,
        1_048_576,
        1_048_576,
        1_048_576,
        64,
        ResourceBudget::declared().output_bytes(),
        64,
    )
    .unwrap_or_else(|error| panic!("budget: {error}"));
    let admitted = executor
        .admit_activity_budget_for_qualification(declared, payer, binding, u128::MAX)
        .unwrap_or_else(|error| panic!("admission: {error}"));
    let engine = WasmEngine::declared().unwrap_or_else(|error| panic!("engine: {error}"));
    let root = engine
        .validate(root_wasm)
        .unwrap_or_else(|error| panic!("root validation: {error}"));
    let mut catalog = ProgramCatalog::new();
    for (program, wasm) in children {
        catalog.insert(
            *program,
            engine
                .validate(wasm)
                .unwrap_or_else(|error| panic!("child validation: {error}")),
        );
    }
    let request = BudgetedAuthorizedExecutionRequest::new(
        AuthorizedExecutionRequest {
            module: &root,
            program: root_program,
            authorization: AuthorizationContext::new(payer, grants),
            receipts: &NoReceipts,
            entrypoint: CALL_ENTRY_EXPORT,
            calldata: &[],
            composition: CompositionContext::catalog(catalog, rules),
            response_capacity: 0,
        },
        admitted,
        payer,
        binding,
    );
    let outcome = executor.execute_authorized_budgeted_for_qualification(&mut storage, request);
    (outcome, storage)
}

fn execute_unbudgeted_with_output_limit(
    root_program: ProgramId,
    root_wasm: &[u8],
    children: &[(ProgramId, Vec<u8>)],
    grants: CapabilitySet,
    rules: CompositionRules,
    output_values: u32,
    output_bytes: u64,
    mut storage: Storage,
) -> (
    Result<
        layerx_programs_runtime::AuthorizedExecutionRecord,
        layerx_programs_runtime::ExecutionError,
    >,
    Storage,
) {
    let engine = WasmEngine::declared().unwrap_or_else(|error| panic!("engine: {error}"));
    let root = engine
        .validate(root_wasm)
        .unwrap_or_else(|error| panic!("root validation: {error}"));
    let mut catalog = ProgramCatalog::new();
    for (program, wasm) in children {
        catalog.insert(
            *program,
            engine
                .validate(wasm)
                .unwrap_or_else(|error| panic!("child validation: {error}")),
        );
    }
    let executor = Executor::new(
        ResourceBudget::new_complete(
            1_000_000,
            16 * 1_024 * 1_024,
            1_048_576,
            1_048_576,
            output_values,
            output_bytes,
            4_096,
        ),
        FeeSchedule::declared(),
    );
    let outcome = executor.execute_authorized(
        &mut storage,
        AuthorizedExecutionRequest {
            module: &root,
            program: root_program,
            authorization: AuthorizationContext::new(principal(9), grants),
            receipts: &NoReceipts,
            entrypoint: CALL_ENTRY_EXPORT,
            calldata: &[],
            composition: CompositionContext::catalog(catalog, rules),
            response_capacity: 0,
        },
    );
    (outcome, storage)
}

fn seeded_storage(programs: impl IntoIterator<Item = ProgramId>) -> Storage {
    let mut storage = Storage::new();
    for program in programs {
        let mut transaction =
            storage.transaction(StorageNamespace::principal(program, principal(9)));
        transaction
            .write(b"canary", &[program.bytes()[0]])
            .unwrap_or_else(|error| panic!("seed {program:?}: {error}"));
        assert_eq!(transaction.commit(), 1);
    }
    storage
}

fn chain_programs(ids: &[ProgramId]) -> ProgramTopology {
    assert!(ids.len() >= 2);
    let root_requested = common_capabilities(ids[2..].iter().copied());
    let root = staging_program(0, &[(ids[1], root_requested)]);
    let mut children = Vec::new();
    for index in 1..ids.len() {
        let calls = if let Some(callee) = ids.get(index + 1) {
            vec![(
                *callee,
                common_capabilities(ids[index + 2..].iter().copied()),
            )]
        } else {
            Vec::new()
        };
        children.push((
            ids[index],
            staging_program(u8::try_from(index).unwrap_or(u8::MAX), &calls),
        ));
    }
    let grants = common_capabilities(ids[1..].iter().copied());
    (root, children, grants)
}

fn edge_programs(include_sixty_fifth: bool) -> EdgeTopology {
    let root = id(80);
    let branches: Vec<_> = (81..=88).map(id).collect();
    let leaves: Vec<_> = (90..=97).map(id).collect();
    let mut children = Vec::new();
    for (index, branch) in branches.iter().copied().enumerate() {
        let count = match index {
            0..=5 => 8,
            6 => 7,
            7 if include_sixty_fifth => 2,
            7 => 1,
            _ => unreachable!("eight branches"),
        };
        let targets: Vec<_> = (0..count)
            .map(|offset| leaves[(index + offset) % leaves.len()])
            .collect();
        let calls: Vec<_> = targets
            .iter()
            .copied()
            .map(|leaf| (leaf, common_capabilities([])))
            .collect();
        children.push((
            branch,
            if index == 7 {
                start_forwarding_program(&calls)
            } else {
                staging_program(u8::try_from(index + 1).unwrap_or(u8::MAX), &calls)
            },
        ));
    }
    for (index, leaf) in leaves.iter().copied().enumerate() {
        children.push((
            leaf,
            staging_program(u8::try_from(index + 20).unwrap_or(u8::MAX), &[]),
        ));
    }
    let root_calls: Vec<_> = branches
        .iter()
        .copied()
        .enumerate()
        .map(|(index, branch)| {
            let count = match index {
                0..=5 => 8,
                6 => 7,
                7 if include_sixty_fifth => 2,
                7 => 1,
                _ => unreachable!("eight branches"),
            };
            let delegated = (0..count).map(|offset| leaves[(index + offset) % leaves.len()]);
            (branch, common_capabilities(delegated))
        })
        .collect();
    let grants = common_capabilities(branches.iter().copied().chain(leaves.iter().copied()));
    (root, staging_program(0, &root_calls), children, grants)
}

fn inventory_rows(suite: &str) -> Vec<(&'static str, &'static str)> {
    let mut lines = ATTACK_INVENTORY.lines();
    assert_eq!(
        lines.next(),
        Some("id\tsuite\thostile_action\tboundary\texpected\ttest\tatomicity\tfuture_owner")
    );
    let mut identifiers = std::collections::BTreeSet::new();
    let mut selected = Vec::new();
    for (offset, line) in lines.enumerate() {
        assert!(
            !line.trim().is_empty(),
            "blank inventory row {}",
            offset + 2
        );
        let fields = line.split('\t').collect::<Vec<_>>();
        assert_eq!(fields.len(), 8, "malformed inventory row {}", offset + 2);
        assert!(
            fields.iter().all(|field| !field.trim().is_empty()),
            "empty inventory field on row {}",
            offset + 2
        );
        assert!(identifiers.insert(fields[0]), "duplicate id {}", fields[0]);
        assert!(
            matches!(fields[1], "isolation" | "composition"),
            "unknown inventory suite {} on row {}",
            fields[1],
            offset + 2
        );
        if fields[1] == suite {
            selected.push((fields[0], fields[5]));
        }
    }
    selected
}

#[test]
fn authority_denial_does_not_create_a_phantom_edge_or_start_the_child() {
    let root = id(1);
    let child = id(2);
    let requested = CapabilitySet::new([Capability::StorageWrite])
        .unwrap_or_else(|error| panic!("requested: {error}"));
    let grants = CapabilitySet::new([Capability::Call { program: child }])
        .unwrap_or_else(|error| panic!("grants: {error}"));
    let before = seeded_storage([root, child]);
    let (outcome, storage) = execute_budgeted_with_storage(
        root,
        &forwarding_program(child, &requested),
        &[(child, trapping_start_program())],
        grants,
        CompositionRules::declared(),
        before.clone(),
    );
    let BudgetedV1ActivityOutcome::Failure(failure) =
        outcome.unwrap_or_else(|error| panic!("authority outcome: {error}"))
    else {
        panic!("authority denial escaped the failure outcome");
    };
    assert_eq!(
        failure.cause(),
        &BudgetedV1FailureCause::Composition(CompositionRefusal::Authority(
            AbiError::CapabilityDenied,
        ))
    );
    assert!(failure.call_graph().edges().is_empty());
    assert_eq!(failure.call_graph().visits(child), 0);
    assert_eq!(storage, before);
}

#[test]
fn production_depth_boundary_and_one_past_are_atomic() {
    let principal = principal(9);
    let exact_ids: Vec<_> = (20..=28).map(id).collect();
    let (root, children, grants) = chain_programs(&exact_ids);
    let (outcome, mut storage) = execute_budgeted(
        exact_ids[0],
        &root,
        &children,
        grants,
        CompositionRules::declared(),
    );
    let BudgetedV1ActivityOutcome::Success(success) =
        outcome.unwrap_or_else(|error| panic!("depth-eight control: {error}"))
    else {
        panic!("depth-eight control did not succeed");
    };
    assert_eq!(success.call_graph.edges().len(), 8);
    assert_eq!(success.effects.events.len(), 9);
    assert_eq!(success.effects.transfers.len(), 9);
    for (index, program) in exact_ids.iter().copied().enumerate() {
        assert_eq!(
            storage
                .transaction(StorageNamespace::principal(program, principal))
                .read(b"k"),
            Ok(Some(vec![u8::try_from(index).unwrap_or(u8::MAX)]))
        );
    }

    let refused_ids: Vec<_> = (30..=39).map(id).collect();
    let (root, children, grants) = chain_programs(&refused_ids);
    let before = seeded_storage(refused_ids.iter().copied());
    let (outcome, storage) = execute_budgeted_with_storage(
        refused_ids[0],
        &root,
        &children,
        grants,
        CompositionRules::declared(),
        before.clone(),
    );
    let BudgetedV1ActivityOutcome::Failure(failure) =
        outcome.unwrap_or_else(|error| panic!("depth-nine refusal: {error}"))
    else {
        panic!("depth-nine call escaped failure");
    };
    assert_eq!(
        failure.cause(),
        &BudgetedV1FailureCause::Composition(CompositionRefusal::DepthExceeded {
            limit: 8,
            attempted: 9,
        })
    );
    assert_eq!(failure.call_graph().edges().len(), 8);
    for (index, edge) in failure.call_graph().edges().iter().enumerate() {
        assert_eq!(edge.caller(), refused_ids[index]);
        assert_eq!(edge.callee(), refused_ids[index + 1]);
        assert_eq!(edge.depth(), u32::try_from(index + 1).unwrap_or(u32::MAX));
    }
    assert_eq!(failure.call_graph().visits(refused_ids[9]), 0);
    assert_eq!(storage, before);
}

#[test]
fn delegated_capability_escalation_matrix_never_enters_the_child() {
    let root = id(3);
    let child = id(4);
    let unrelated = id(5);
    let transfer = Capability::Transfer402 {
        asset: ASSET,
        to: RECIPIENT,
        maximum_amount: 1,
    };
    let wider_transfer = Capability::Transfer402 {
        asset: ASSET,
        to: RECIPIENT,
        maximum_amount: 2,
    };
    let cases = [
        (
            "storage",
            CapabilitySet::new([Capability::StorageWrite])
                .unwrap_or_else(|error| panic!("storage request: {error}")),
            CapabilitySet::new([Capability::Call { program: child }])
                .unwrap_or_else(|error| panic!("storage parent: {error}")),
            AbiError::CapabilityDenied,
        ),
        (
            "event",
            CapabilitySet::new([Capability::EmitEvent])
                .unwrap_or_else(|error| panic!("event request: {error}")),
            CapabilitySet::new([Capability::Call { program: child }])
                .unwrap_or_else(|error| panic!("event parent: {error}")),
            AbiError::CapabilityDenied,
        ),
        (
            "transfer",
            CapabilitySet::new([wider_transfer])
                .unwrap_or_else(|error| panic!("transfer request: {error}")),
            CapabilitySet::new([Capability::Call { program: child }, transfer])
                .unwrap_or_else(|error| panic!("transfer parent: {error}")),
            AbiError::CapabilityEscalation,
        ),
        (
            "unrelated call",
            CapabilitySet::new([Capability::Call { program: unrelated }])
                .unwrap_or_else(|error| panic!("call request: {error}")),
            CapabilitySet::new([Capability::Call { program: child }])
                .unwrap_or_else(|error| panic!("call parent: {error}")),
            AbiError::CapabilityDenied,
        ),
    ];
    for (name, requested, grants, expected) in cases {
        let root_wasm = forwarding_program(child, &requested);
        let before = seeded_storage([root, child]);
        let (outcome, storage) = execute_budgeted_with_storage(
            root,
            &root_wasm,
            &[(child, trapping_start_program())],
            grants,
            CompositionRules::declared(),
            before.clone(),
        );
        let BudgetedV1ActivityOutcome::Failure(failure) =
            outcome.unwrap_or_else(|error| panic!("{name} delegation: {error}"))
        else {
            panic!("{name} delegation escaped failure");
        };
        assert_eq!(
            failure.cause(),
            &BudgetedV1FailureCause::Composition(CompositionRefusal::Authority(expected)),
            "{name}"
        );
        assert!(failure.call_graph().edges().is_empty(), "{name}");
        assert_eq!(failure.call_graph().visits(child), 0, "{name}");
        assert_eq!(storage, before, "{name}");
    }
}

#[test]
fn production_fanout_boundary_and_one_past_are_atomic() {
    let root = id(50);
    let children: Vec<_> = (51..=67).map(id).collect();
    let requested = common_capabilities([]);
    let child_modules: Vec<_> = children
        .iter()
        .enumerate()
        .map(|(index, child)| {
            (
                *child,
                staging_program(u8::try_from(index + 1).unwrap_or(u8::MAX), &[]),
            )
        })
        .collect();

    let exact_calls: Vec<_> = children[..16]
        .iter()
        .copied()
        .map(|child| (child, requested.clone()))
        .collect();
    let (outcome, storage) = execute_budgeted(
        root,
        &staging_program(0, &exact_calls),
        &child_modules[..16],
        common_capabilities(children[..16].iter().copied()),
        CompositionRules::declared(),
    );
    let BudgetedV1ActivityOutcome::Success(success) =
        outcome.unwrap_or_else(|error| panic!("fanout-sixteen control: {error}"))
    else {
        panic!("fanout-sixteen control did not succeed");
    };
    assert_eq!(success.call_graph.edges().len(), 16);
    assert_eq!(success.effects.events.len(), 17);
    assert_eq!(success.effects.transfers.len(), 17);
    assert_ne!(storage, Storage::new());

    let over_calls: Vec<_> = children
        .iter()
        .copied()
        .map(|child| (child, requested.clone()))
        .collect();
    let before = seeded_storage(std::iter::once(root).chain(children.iter().copied()));
    let (outcome, storage) = execute_budgeted_with_storage(
        root,
        &staging_program(0, &over_calls),
        &child_modules,
        common_capabilities(children.iter().copied()),
        CompositionRules::declared(),
        before.clone(),
    );
    let BudgetedV1ActivityOutcome::Failure(failure) =
        outcome.unwrap_or_else(|error| panic!("fanout-seventeen refusal: {error}"))
    else {
        panic!("fanout-seventeen call escaped failure");
    };
    assert_eq!(
        failure.cause(),
        &BudgetedV1FailureCause::Composition(CompositionRefusal::FanoutExceeded {
            limit: 16,
            attempted: 17,
        })
    );
    assert_eq!(failure.call_graph().edges().len(), 16);
    assert_eq!(failure.call_graph().visits(children[16]), 0);
    assert_eq!(storage, before);
}

#[test]
fn production_visit_boundary_and_one_past_are_atomic() {
    let root = id(70);
    let child = id(71);
    let requested = common_capabilities([]);
    let child_module = staging_program(1, &[]);
    let calls = |count: usize| vec![(child, requested.clone()); count];
    let grants = common_capabilities([child]);

    let (outcome, storage) = execute_budgeted(
        root,
        &staging_program(0, &calls(8)),
        &[(child, child_module.clone())],
        grants.clone(),
        CompositionRules::declared(),
    );
    let BudgetedV1ActivityOutcome::Success(success) =
        outcome.unwrap_or_else(|error| panic!("visit-eight control: {error}"))
    else {
        panic!("visit-eight control did not succeed");
    };
    assert_eq!(success.call_graph.visits(child), 8);
    assert_eq!(success.call_graph.edges().len(), 8);
    assert_eq!(success.effects.events.len(), 9);
    assert_eq!(success.effects.transfers.len(), 9);
    assert_ne!(storage, Storage::new());

    let before = seeded_storage([root, child]);
    let (outcome, storage) = execute_budgeted_with_storage(
        root,
        &staging_program(0, &calls(9)),
        &[(child, child_module)],
        grants,
        CompositionRules::declared(),
        before.clone(),
    );
    let BudgetedV1ActivityOutcome::Failure(failure) =
        outcome.unwrap_or_else(|error| panic!("visit-nine refusal: {error}"))
    else {
        panic!("visit-nine call escaped failure");
    };
    assert_eq!(
        failure.cause(),
        &BudgetedV1FailureCause::Composition(CompositionRefusal::VisitsExceeded {
            program: child,
            limit: 8,
            attempted: 9,
        })
    );
    assert_eq!(failure.call_graph().edges().len(), 8);
    assert_eq!(failure.call_graph().visits(child), 8);
    assert_eq!(storage, before);
}

#[test]
fn direct_and_indirect_reentrancy_are_typed_and_atomic() {
    let direct = id(72);
    let direct_requested = common_capabilities([]);
    let direct_wasm = staging_program(direct.bytes()[0], &[(direct, direct_requested)]);
    let before = seeded_storage([direct]);
    let (outcome, storage) = execute_budgeted_with_storage(
        direct,
        &direct_wasm,
        &[(direct, direct_wasm.clone())],
        common_capabilities([direct]),
        CompositionRules::declared(),
        before.clone(),
    );
    let BudgetedV1ActivityOutcome::Failure(failure) =
        outcome.unwrap_or_else(|error| panic!("direct reentrancy: {error}"))
    else {
        panic!("direct reentrancy escaped failure");
    };
    assert_eq!(
        failure.cause(),
        &BudgetedV1FailureCause::Composition(CompositionRefusal::Reentrancy { program: direct })
    );
    assert!(failure.call_graph().edges().is_empty());
    assert_eq!(failure.call_graph().visits(direct), 1);
    assert_eq!(storage, before);

    let root = id(73);
    let child = id(74);
    let child_wasm = staging_program(2, &[(root, common_capabilities([]))]);
    let root_wasm = staging_program(1, &[(child, common_capabilities([root]))]);
    let before = seeded_storage([root, child]);
    let (outcome, storage) = execute_budgeted_with_storage(
        root,
        &root_wasm,
        &[(child, child_wasm), (root, root_wasm.clone())],
        common_capabilities([child, root]),
        CompositionRules::declared(),
        before.clone(),
    );
    let BudgetedV1ActivityOutcome::Failure(failure) =
        outcome.unwrap_or_else(|error| panic!("indirect reentrancy: {error}"))
    else {
        panic!("indirect reentrancy escaped failure");
    };
    assert_eq!(
        failure.cause(),
        &BudgetedV1FailureCause::Composition(CompositionRefusal::Reentrancy { program: root })
    );
    assert_eq!(failure.call_graph().edges().len(), 1);
    assert_eq!(failure.call_graph().edges()[0].caller(), root);
    assert_eq!(failure.call_graph().edges()[0].callee(), child);
    assert_eq!(failure.call_graph().visits(root), 1);
    assert_eq!(storage, before);
}

#[test]
fn production_edge_boundary_and_one_past_are_independently_typed() {
    let (root, root_wasm, children, grants) = edge_programs(false);
    let (outcome, storage) = execute_unbudgeted_with_output_limit(
        root,
        &root_wasm,
        &children,
        grants.clone(),
        CompositionRules::declared(),
        65,
        1_048_576,
        Storage::new(),
    );
    let success = outcome.unwrap_or_else(|error| panic!("edge-sixty-four control: {error}"));
    assert_eq!(success.call_graph.edges().len(), 64);
    assert_eq!(success.effects.calls.len(), 64);
    assert_eq!(success.effects.events.len(), 64);
    assert_eq!(success.effects.transfers.len(), 64);
    assert_ne!(storage, Storage::new());

    let before =
        seeded_storage(std::iter::once(root).chain(children.iter().map(|(program, _)| *program)));
    let (legacy_refusal, storage) = execute_unbudgeted_with_output_limit(
        root,
        &root_wasm,
        &children,
        grants.clone(),
        CompositionRules::declared(),
        64,
        1_048_576,
        before.clone(),
    );
    assert_eq!(
        legacy_refusal,
        Err(ExecutionError::Resource(MeterRefusal::BudgetExceeded {
            resource: ResourceKind::Output,
            limit: 64,
            attempted: 65,
        }))
    );
    assert_eq!(storage, before);

    let before =
        seeded_storage(std::iter::once(root).chain(children.iter().map(|(program, _)| *program)));
    let (outcome, storage) = execute_budgeted_with_storage(
        root,
        &root_wasm,
        &children,
        grants,
        CompositionRules::declared(),
        before.clone(),
    );
    let BudgetedV1ActivityOutcome::Resource(resource) =
        outcome.unwrap_or_else(|error| panic!("production output cross-limit: {error}"))
    else {
        panic!("production output cross-limit did not refuse as a resource");
    };
    assert_eq!(
        resource.refusal(),
        BudgetMeterRefusal::BudgetExceeded {
            resource: BudgetResourceKind::Output,
            limit: 64,
            attempted: 65,
        }
    );
    assert_eq!(resource.call_graph().edges().len(), 64);
    assert_eq!(storage, before);

    let (root, root_wasm, children, grants) = edge_programs(true);
    let before =
        seeded_storage(std::iter::once(root).chain(children.iter().map(|(program, _)| *program)));
    let (outcome, storage) = execute_budgeted_with_storage(
        root,
        &root_wasm,
        &children,
        grants,
        CompositionRules::declared(),
        before.clone(),
    );
    let BudgetedV1ActivityOutcome::Failure(failure) =
        outcome.unwrap_or_else(|error| panic!("edge-sixty-five refusal: {error}"))
    else {
        panic!("edge-sixty-five call escaped failure");
    };
    assert_eq!(
        failure.cause(),
        &BudgetedV1FailureCause::Composition(CompositionRefusal::EdgesExceeded {
            limit: 64,
            attempted: 65,
        })
    );
    assert_eq!(failure.call_graph().edges().len(), 64);
    assert!(!failure
        .call_graph()
        .edges()
        .iter()
        .any(|edge| edge.caller() == id(88) && edge.callee() == id(90)));
    assert_eq!(failure.call_graph().visits(id(90)), 7);
    assert!(failure.usage().output_values <= 64);
    assert_eq!(storage, before);
}

#[test]
fn nested_guest_event_aggregate_accepts_sixty_four_and_rolls_back_sixty_five() {
    let root = id(120);
    let child = id(121);
    let delegated = CapabilitySet::new([Capability::EmitEvent])
        .unwrap_or_else(|error| panic!("delegated event capability: {error}"));
    let grants = CapabilitySet::new([
        Capability::EmitEvent,
        Capability::Call { program: child },
    ])
    .unwrap_or_else(|error| panic!("root event capabilities: {error}"));
    let root_wasm = event_chain_program(32, Some((child, &delegated)));

    let (outcome, _) = execute_unbudgeted_with_output_limit(
        root,
        &root_wasm,
        &[(child, event_chain_program(32, None))],
        grants.clone(),
        CompositionRules::declared(),
        64,
        64,
        Storage::new(),
    );
    let success = outcome.unwrap_or_else(|error| panic!("sixty-four events: {error}"));
    assert_eq!(success.effects.events.len(), 64);
    assert_eq!(success.execution.usage.output_bytes, 64);

    let before = seeded_storage([root, child]);
    let (outcome, storage) = execute_unbudgeted_with_output_limit(
        root,
        &root_wasm,
        &[(child, event_chain_program(33, None))],
        grants,
        CompositionRules::declared(),
        64,
        64,
        before.clone(),
    );
    assert_eq!(
        outcome,
        Err(ExecutionError::Composition(CompositionRefusal::Authority(
            AbiError::EventBounds,
        )))
    );
    assert_eq!(storage, before);
}

#[test]
#[allow(clippy::too_many_lines)]
fn small_rules_isolate_each_graph_limit_precedence() {
    let rules = |depth, edges, fanout, visits| {
        CompositionRules::new(depth, edges, fanout, visits)
            .unwrap_or_else(|error| panic!("small rules: {error}"))
    };

    let ids = [id(100), id(101), id(102)];
    let (root_wasm, children, grants) = chain_programs(&ids);
    let before = seeded_storage(ids);
    let (outcome, storage) = execute_budgeted_with_storage(
        ids[0],
        &root_wasm,
        &children,
        grants,
        rules(1, 8, 8, 8),
        before.clone(),
    );
    let BudgetedV1ActivityOutcome::Failure(failure) =
        outcome.unwrap_or_else(|error| panic!("small depth: {error}"))
    else {
        panic!("small depth escaped failure");
    };
    assert_eq!(
        failure.cause(),
        &BudgetedV1FailureCause::Composition(CompositionRefusal::DepthExceeded {
            limit: 1,
            attempted: 2,
        })
    );
    assert_eq!(failure.call_graph().edges().len(), 1);
    assert_eq!(storage, before);

    let root = id(103);
    let children_ids = [id(104), id(105)];
    let calls = children_ids
        .iter()
        .copied()
        .map(|callee| (callee, common_capabilities([])))
        .collect::<Vec<_>>();
    let root_wasm = staging_program(0, &calls);
    let children = children_ids
        .iter()
        .copied()
        .enumerate()
        .map(|(index, program)| {
            (
                program,
                staging_program(u8::try_from(index + 1).unwrap_or(u8::MAX), &[]),
            )
        })
        .collect::<Vec<_>>();
    let grants = common_capabilities(children_ids);
    let before = seeded_storage(std::iter::once(root).chain(children_ids));
    let (outcome, storage) = execute_budgeted_with_storage(
        root,
        &root_wasm,
        &children,
        grants.clone(),
        rules(8, 8, 1, 8),
        before.clone(),
    );
    let BudgetedV1ActivityOutcome::Failure(failure) =
        outcome.unwrap_or_else(|error| panic!("small fanout: {error}"))
    else {
        panic!("small fanout escaped failure");
    };
    assert_eq!(
        failure.cause(),
        &BudgetedV1FailureCause::Composition(CompositionRefusal::FanoutExceeded {
            limit: 1,
            attempted: 2,
        })
    );
    assert_eq!(failure.call_graph().edges().len(), 1);
    assert_eq!(storage, before);

    let before = seeded_storage(std::iter::once(root).chain(children_ids));
    let (outcome, storage) = execute_budgeted_with_storage(
        root,
        &root_wasm,
        &children,
        grants,
        rules(8, 1, 8, 8),
        before.clone(),
    );
    let BudgetedV1ActivityOutcome::Failure(failure) =
        outcome.unwrap_or_else(|error| panic!("small edges: {error}"))
    else {
        panic!("small edges escaped failure");
    };
    assert_eq!(
        failure.cause(),
        &BudgetedV1FailureCause::Composition(CompositionRefusal::EdgesExceeded {
            limit: 1,
            attempted: 2,
        })
    );
    assert_eq!(failure.call_graph().edges().len(), 1);
    assert_eq!(storage, before);

    let child = id(106);
    let calls = [
        (child, common_capabilities([])),
        (child, common_capabilities([])),
    ];
    let root_wasm = staging_program(0, &calls);
    let children = [(child, staging_program(1, &[]))];
    let before = seeded_storage([root, child]);
    let (outcome, storage) = execute_budgeted_with_storage(
        root,
        &root_wasm,
        &children,
        common_capabilities([child]),
        rules(8, 8, 8, 1),
        before.clone(),
    );
    let BudgetedV1ActivityOutcome::Failure(failure) =
        outcome.unwrap_or_else(|error| panic!("small visits: {error}"))
    else {
        panic!("small visits escaped failure");
    };
    assert_eq!(
        failure.cause(),
        &BudgetedV1FailureCause::Composition(CompositionRefusal::VisitsExceeded {
            program: child,
            limit: 1,
            attempted: 2,
        })
    );
    assert_eq!(failure.call_graph().edges().len(), 1);
    assert_eq!(storage, before);
}

#[test]
fn inherited_program_spend_narrows_across_depth_fanout_and_repeated_visits() {
    let owner = id(110);
    let child = id(111);
    let seed = b"escrow/composition";
    let source_account = derive_program_account(owner, seed)
        .unwrap_or_else(|error| panic!("derived account: {error}"))
        .bytes();
    let grant = |asset, to, maximum_amount| Capability::ProgramSpend {
        owner_program: owner,
        seed: seed.to_vec(),
        source_account,
        asset,
        to,
        maximum_amount,
    };
    let asset = [112; 32];
    let to = [113; 32];
    let parent = CapabilitySet::new([Capability::Call { program: child }, grant(asset, to, 80)])
        .unwrap_or_else(|error| panic!("parent: {error}"));

    let mut level = parent
        .narrow([grant(asset, to, 64)])
        .unwrap_or_else(|error| panic!("level one: {error}"));
    for amount in (56..=63).rev() {
        level = level
            .narrow([grant(asset, to, amount)])
            .unwrap_or_else(|error| panic!("depth narrowing: {error}"));
    }
    for amount in 1..=16 {
        assert!(parent.narrow([grant(asset, to, amount)]).is_ok());
    }
    for widened in [
        grant(asset, to, 81),
        grant([114; 32], to, 1),
        grant(asset, [115; 32], 1),
    ] {
        assert_eq!(
            parent.narrow([widened]),
            Err(AbiError::CapabilityEscalation)
        );
    }
}

#[test]
#[allow(clippy::missing_panics_doc)]
pub fn programs_composition_suite() {
    authority_denial_does_not_create_a_phantom_edge_or_start_the_child();
    delegated_capability_escalation_matrix_never_enters_the_child();
    production_depth_boundary_and_one_past_are_atomic();
    production_fanout_boundary_and_one_past_are_atomic();
    production_visit_boundary_and_one_past_are_atomic();
    direct_and_indirect_reentrancy_are_typed_and_atomic();
    production_edge_boundary_and_one_past_are_independently_typed();
    small_rules_isolate_each_graph_limit_precedence();

    let executed = vec![
        (
            "COMP-001",
            "authority_denial_does_not_create_a_phantom_edge_or_start_the_child",
        ),
        (
            "COMP-002",
            "delegated_capability_escalation_matrix_never_enters_the_child",
        ),
        (
            "COMP-003",
            "production_depth_boundary_and_one_past_are_atomic",
        ),
        (
            "COMP-004",
            "production_fanout_boundary_and_one_past_are_atomic",
        ),
        (
            "COMP-005",
            "production_visit_boundary_and_one_past_are_atomic",
        ),
        (
            "COMP-006",
            "direct_and_indirect_reentrancy_are_typed_and_atomic",
        ),
        (
            "COMP-007",
            "production_edge_boundary_and_one_past_are_independently_typed",
        ),
        (
            "COMP-008",
            "small_rules_isolate_each_graph_limit_precedence",
        ),
    ];
    assert_eq!(inventory_rows("composition"), executed);
}
