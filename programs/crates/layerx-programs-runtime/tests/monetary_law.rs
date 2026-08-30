use layerx_programs_runtime::abi::response::CANDIDATE_ABI_MODULE;
use layerx_programs_runtime::test_support::{
    code_section, func_body, function_section, import_section, module, type_section, unsigned_leb,
    OP_CALL, OP_END, OP_I32_CONST, TYPE_I32, TYPE_I64,
};
use layerx_programs_runtime::{
    AbiError, AuthorizationContext, CallFrameId, Capability, CapabilitySet, PrincipalId, ProgramId,
    Storage, TransferSource,
};
use layerx_programs_runtime::{
    ActivityBudgetBinding, BudgetedAuthorizedExecutionRequest, DeclaredBudget,
};
use layerx_programs_runtime::{
    AuthorizedExecutionRequest, BudgetMeterRefusal, BudgetResourceKind, CompositionContext,
    Executor, PreparedAuthorizedActivityOutcome, ReceiptOracle, ReceiptView, WasmEngine,
    ABI_MODULE, CALL_ENTRY_EXPORT,
};

#[derive(Debug)]
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

fn exports(entries: &[(&str, u8, u8)]) -> Vec<u8> {
    let mut payload = unsigned_leb(entries.len() as u64);
    for (name, kind, index) in entries {
        payload.extend(unsigned_leb(name.len() as u64));
        payload.extend_from_slice(name.as_bytes());
        payload.extend_from_slice(&[*kind, *index]);
    }
    section(7, &payload)
}

fn data_section(entries: &[(u32, &[u8])]) -> Vec<u8> {
    let mut payload = unsigned_leb(entries.len() as u64);
    for (offset, bytes) in entries {
        payload.extend([0, OP_I32_CONST]);
        payload.extend(unsigned_leb(u64::from(*offset)));
        payload.push(OP_END);
        payload.extend(unsigned_leb(bytes.len() as u64));
        payload.extend_from_slice(bytes);
    }
    section(11, &payload)
}

fn transfer_module() -> Vec<u8> {
    let asset = [3; 32];
    let recipient = [4; 32];
    module(&[
        type_section(&[
            (
                &[TYPE_I64, TYPE_I64, TYPE_I32, TYPE_I32, TYPE_I32, TYPE_I32],
                &[TYPE_I32],
            ),
            (&[TYPE_I32, TYPE_I32], &[TYPE_I32]),
        ]),
        import_section(&[(ABI_MODULE, "transfer_402", 0)]),
        function_section(&[1]),
        section(5, &[1, 1, 1, 1]),
        exports(&[("run", 0, 1), ("memory", 2, 0)]),
        code_section(&[func_body(
            &[],
            &[
                0x42,
                0,
                0x42,
                1,
                OP_I32_CONST,
                0,
                OP_I32_CONST,
                32,
                OP_I32_CONST,
                32,
                OP_I32_CONST,
                32,
                OP_CALL,
                0,
                OP_END,
            ],
        )]),
        data_section(&[(0, &asset), (32, &recipient)]),
    ])
}

fn candidate_program_transfer_module(
    seed: &[u8],
    source: [u8; 32],
    asset: [u8; 32],
    recipient: [u8; 32],
) -> Vec<u8> {
    const SOURCE_OFFSET: u32 = 128;
    const ASSET_OFFSET: u32 = 160;
    const RECIPIENT_OFFSET: u32 = 192;
    let mut entries = unsigned_leb(3);
    for (name, kind, index) in [
        ("layerx_reserve", 0_u8, 1_u8),
        (CALL_ENTRY_EXPORT, 0, 2),
        ("memory", 2, 0),
    ] {
        entries.extend(unsigned_leb(name.len() as u64));
        entries.extend_from_slice(name.as_bytes());
        entries.extend_from_slice(&[kind, index]);
    }
    let mut entry = vec![0x42, 0, 0x42, 5, OP_I32_CONST, 0, OP_I32_CONST];
    entry.extend(unsigned_leb(seed.len() as u64));
    for value in [SOURCE_OFFSET, 32, ASSET_OFFSET, 32, RECIPIENT_OFFSET, 32] {
        entry.push(OP_I32_CONST);
        entry.extend(unsigned_leb(u64::from(value)));
    }
    entry.extend([OP_CALL, 0, OP_END]);
    module(&[
        type_section(&[
            (
                &[
                    TYPE_I64, TYPE_I64, TYPE_I32, TYPE_I32, TYPE_I32, TYPE_I32, TYPE_I32, TYPE_I32,
                    TYPE_I32, TYPE_I32,
                ],
                &[TYPE_I32],
            ),
            (&[TYPE_I32], &[TYPE_I32]),
            (&[TYPE_I32, TYPE_I32], &[TYPE_I32]),
        ]),
        import_section(&[(CANDIDATE_ABI_MODULE, "transfer_program_402", 0)]),
        function_section(&[1, 2]),
        section(5, &[1, 1, 1, 1]),
        section(7, &entries),
        code_section(&[
            func_body(&[], &[OP_I32_CONST, 0, OP_END]),
            func_body(&[], &entry),
        ]),
        data_section(&[
            (0, seed),
            (SOURCE_OFFSET, &source),
            (ASSET_OFFSET, &asset),
            (RECIPIENT_OFFSET, &recipient),
        ]),
    ])
}

fn no_effect_module() -> Vec<u8> {
    module(&[
        type_section(&[(&[TYPE_I32, TYPE_I32], &[TYPE_I32])]),
        function_section(&[0]),
        section(5, &[1, 1, 1, 1]),
        exports(&[("run", 0, 0), ("memory", 2, 0)]),
        code_section(&[func_body(&[], &[OP_I32_CONST, 0, OP_END])]),
    ])
}

fn candidate_with_entry(entry: &[u8]) -> Vec<u8> {
    let functions = function_section(&[0, 1]);
    let memory = section(5, &[1, 1, 1, 1]);
    let mut entries = unsigned_leb(3);
    for (name, kind, index) in [
        ("layerx_reserve", 0_u8, 0_u8),
        (CALL_ENTRY_EXPORT, 0, 1),
        ("memory", 2, 0),
    ] {
        entries.extend(unsigned_leb(name.len() as u64));
        entries.extend_from_slice(name.as_bytes());
        entries.extend_from_slice(&[kind, index]);
    }
    module(&[
        type_section(&[
            (&[TYPE_I32], &[TYPE_I32]),
            (&[TYPE_I32, TYPE_I32], &[TYPE_I32]),
        ]),
        functions,
        memory,
        section(7, &entries),
        code_section(&[
            func_body(&[], &[OP_I32_CONST, 0, OP_END]),
            func_body(&[], entry),
        ]),
    ])
}

fn looping_module() -> Vec<u8> {
    candidate_with_entry(&[0x03, 0x40, 0x0c, 0, OP_END, OP_I32_CONST, 0, OP_END])
}

fn admitted_request<'a>(
    executor: &Executor,
    request: AuthorizedExecutionRequest<'a>,
    payer: PrincipalId,
    declared: DeclaredBudget,
    binding: ActivityBudgetBinding,
) -> BudgetedAuthorizedExecutionRequest<'a> {
    let admitted = executor
        .admit_activity_budget_for_qualification(declared, payer, binding, u128::MAX)
        .unwrap_or_else(|error| panic!("admitted: {error}"));
    BudgetedAuthorizedExecutionRequest::new(request, admitted, payer, binding)
}

fn generous_budget() -> DeclaredBudget {
    DeclaredBudget::new(1_000_000, 1_048_576, 1_048_576, 1_048_576, 64, 0, 64)
        .unwrap_or_else(|error| panic!("declared: {error}"))
}

#[test]
fn real_wasm_budgeted_preparation_seals_transfer_or_zero_transfer_without_kernel() {
    let program = ProgramId::new([1; 32]).unwrap_or_else(|error| panic!("program: {error}"));
    let payer = PrincipalId::new([2; 32]).unwrap_or_else(|error| panic!("payer: {error}"));
    let executor = Executor::declared();
    for (wasm, grants, has_transfer) in [
        (
            transfer_module(),
            CapabilitySet::new([Capability::Transfer402 {
                asset: [3; 32],
                to: [4; 32],
                maximum_amount: 1,
            }])
            .unwrap_or_else(|error| panic!("grants: {error}")),
            true,
        ),
        (no_effect_module(), CapabilitySet::empty(), false),
    ] {
        let module = WasmEngine::declared()
            .unwrap_or_else(|error| panic!("engine: {error}"))
            .validate(&wasm)
            .unwrap_or_else(|error| panic!("module: {error}"));
        let storage = Storage::default();
        let outcome = executor
            .prepare_authorized_activity_budgeted(
                &storage,
                admitted_request(
                    &executor,
                    AuthorizedExecutionRequest {
                        module: &module,
                        program,
                        authorization: AuthorizationContext::new(payer, grants),
                        receipts: &NoReceipts,
                        entrypoint: "run",
                        calldata: &[],
                        composition: CompositionContext::isolated(),
                        response_capacity: 0,
                    },
                    payer,
                    generous_budget(),
                    ActivityBudgetBinding::new([9; 32])
                        .unwrap_or_else(|error| panic!("binding: {error}")),
                ),
            )
            .unwrap_or_else(|error| panic!("prepared: {error}"));
        let PreparedAuthorizedActivityOutcome::Success(prepared) = outcome else {
            panic!("real wasm preparation must succeed")
        };
        assert!(prepared.execution().usage.cpu_fuel > 0);
        assert_eq!(prepared.execution().usage.memory_bytes, 65_536);
        assert!(prepared.execution().usage.fee_units > 0);
        assert_eq!(prepared.has_monetary_effects(), has_transfer);
        let summary = prepared.monetary_summary();
        if has_transfer {
            let summary = summary.unwrap_or_else(|| panic!("missing monetary summary"));
            assert_eq!(summary.program(), program);
            assert_eq!(summary.principal(), payer);
            assert_eq!(summary.invocation_authority(), [9; 32]);
            assert_eq!(summary.total_amount(), 1);
            assert_eq!(summary.legs().len(), 1);
            let leg = &summary.legs()[0];
            assert_eq!(leg.program(), program);
            assert_eq!(leg.principal(), payer);
            assert_eq!(leg.frame(), CallFrameId::root());
            assert_eq!(leg.asset(), [3; 32]);
            assert_eq!(leg.to(), [4; 32]);
            assert_eq!(leg.amount(), 1);
        } else {
            assert_eq!(summary, None);
        }
    }
}

#[test]
fn candidate_program_transfer_host_issues_exact_owner_frame_authority() {
    let program = ProgramId::new([31; 32]).unwrap_or_else(|error| panic!("program: {error}"));
    let payer = PrincipalId::new([32; 32]).unwrap_or_else(|error| panic!("payer: {error}"));
    let seed = b"merchant/settlement";
    let source = layerx_programs_runtime::derive_program_account(program, seed)
        .unwrap_or_else(|error| panic!("source: {error}"))
        .bytes();
    let asset = [33; 32];
    let recipient = [34; 32];
    let grants = CapabilitySet::new([Capability::ProgramSpend {
        owner_program: program,
        seed: seed.to_vec(),
        source_account: source,
        asset,
        to: recipient,
        maximum_amount: 5,
    }])
    .unwrap_or_else(|error| panic!("grants: {error}"));
    let module = WasmEngine::declared()
        .unwrap_or_else(|error| panic!("engine: {error}"))
        .validate_candidate_v2(&candidate_program_transfer_module(
            seed, source, asset, recipient,
        ))
        .unwrap_or_else(|error| panic!("module: {error}"));
    let record = Executor::declared()
        .execute_authorized_candidate(
            &mut Storage::default(),
            AuthorizedExecutionRequest {
                module: &module,
                program,
                authorization: AuthorizationContext::new(payer, grants),
                receipts: &NoReceipts,
                entrypoint: CALL_ENTRY_EXPORT,
                calldata: &[],
                composition: CompositionContext::isolated(),
                response_capacity: 0,
            },
        )
        .unwrap_or_else(|error| panic!("candidate execution: {error}"));
    let effects = record
        .effects()
        .unwrap_or_else(|| panic!("candidate effects missing"));
    assert_eq!(effects.transfers.len(), 1);
    let transfer = &effects.transfers[0];
    assert_eq!(transfer.program, program);
    assert_eq!(transfer.principal, payer);
    assert_eq!(transfer.frame, CallFrameId::root());
    assert_eq!(transfer.asset, asset);
    assert_eq!(transfer.to, recipient);
    assert_eq!(transfer.amount, 5);
    let TransferSource::Program(authority) = transfer.source() else {
        panic!("candidate transfer must carry program authority")
    };
    assert_eq!(authority.owner_program(), program);
    assert_eq!(authority.seed(), seed);
    assert_eq!(authority.source_account(), source);
    assert_eq!(authority.staging_frame(), CallFrameId::root());
    assert_eq!(authority.asset(), asset);
    assert_eq!(authority.to(), recipient);
    assert_eq!(authority.amount(), 5);
}

#[test]
fn real_wasm_budgeted_preparation_retains_failure_and_resource_diagnostics() {
    let executor = Executor::declared();
    let payer = PrincipalId::new([2; 32]).unwrap_or_else(|error| panic!("payer: {error}"));
    let program = ProgramId::new([1; 32]).unwrap_or_else(|error| panic!("program: {error}"));
    let engine = WasmEngine::declared().unwrap_or_else(|error| panic!("engine: {error}"));

    let failed_module = engine
        .validate(&candidate_with_entry(&[OP_I32_CONST, 0x7f, OP_END]))
        .unwrap_or_else(|error| panic!("failed module: {error}"));
    let failure = executor
        .prepare_authorized_activity_budgeted(
            &Storage::default(),
            admitted_request(
                &executor,
                AuthorizedExecutionRequest {
                    module: &failed_module,
                    program,
                    authorization: AuthorizationContext::new(payer, CapabilitySet::empty()),
                    receipts: &NoReceipts,
                    entrypoint: CALL_ENTRY_EXPORT,
                    calldata: &[],
                    composition: CompositionContext::isolated(),
                    response_capacity: 0,
                },
                payer,
                generous_budget(),
                ActivityBudgetBinding::new([10; 32])
                    .unwrap_or_else(|error| panic!("binding: {error}")),
            ),
        )
        .unwrap_or_else(|error| panic!("failure preparation: {error}"));
    let PreparedAuthorizedActivityOutcome::Failure(failure) = failure else {
        panic!("expected receipt-ready failure")
    };
    assert!(failure.usage().cpu_fuel > 0);
    assert!(failure.call_graph().edges().is_empty());

    let looping = engine
        .validate(&looping_module())
        .unwrap_or_else(|error| panic!("looping module: {error}"));
    let resource = executor
        .prepare_authorized_activity_budgeted(
            &Storage::default(),
            admitted_request(
                &executor,
                AuthorizedExecutionRequest {
                    module: &looping,
                    program,
                    authorization: AuthorizationContext::new(payer, CapabilitySet::empty()),
                    receipts: &NoReceipts,
                    entrypoint: CALL_ENTRY_EXPORT,
                    calldata: &[],
                    composition: CompositionContext::isolated(),
                    response_capacity: 0,
                },
                payer,
                DeclaredBudget::new(100, 65_536, 0, 0, 2, 0, 0)
                    .unwrap_or_else(|error| panic!("cpu budget: {error}")),
                ActivityBudgetBinding::new([11; 32])
                    .unwrap_or_else(|error| panic!("binding: {error}")),
            ),
        )
        .unwrap_or_else(|error| panic!("resource preparation: {error}"));
    let PreparedAuthorizedActivityOutcome::Resource(resource) = resource else {
        panic!("expected receipt-ready resource refusal")
    };
    assert_eq!(
        resource.refusal(),
        BudgetMeterRefusal::BudgetExceeded {
            resource: BudgetResourceKind::Cpu,
            limit: 100,
            attempted: 101,
        }
    );
    assert_eq!(resource.usage().cpu_fuel, 99);
    assert!(resource.call_graph().edges().is_empty());
}
