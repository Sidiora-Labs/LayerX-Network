use layerx_programs_runtime::test_support::{
    code_section, export_section, func_body, function_section, module, raw_section, type_section,
    unsigned_leb, OP_CALL, OP_DROP, OP_END, OP_I32_ADD, OP_I32_CONST, OP_LOCAL_GET, TYPE_I32,
};
use layerx_programs_runtime::{
    ExecutionError, ExecutionFault, Executor, FeeSchedule, ResourceBudget, TracePolicy,
    ValidatedModule, WasmEngine, WasmValue,
};

const STATE_RICH_TRACED_CPU_FUEL: u64 = 10_654_360;
const TRACE_TEST_CPU_HEADROOM: u64 = STATE_RICH_TRACED_CPU_FUEL * 2;

fn state_rich_module() -> Vec<u8> {
    let memory_section = raw_section(5, &[1, 0, 1]);
    let global_section = raw_section(
        6,
        &[2, TYPE_I32, 1, OP_I32_CONST, 0, OP_END, TYPE_I32, 0, OP_I32_CONST, 9, OP_END],
    );
    module(&[
        type_section(&[(&[TYPE_I32], &[TYPE_I32]), (&[TYPE_I32], &[TYPE_I32])]),
        function_section(&[0, 1]),
        memory_section,
        global_section,
        export_section(&[("run", 1)]),
        code_section(&[
            func_body(
                &[(1, TYPE_I32)],
                &[
                    OP_LOCAL_GET, 0, OP_I32_CONST, 2, OP_I32_ADD, 0x22, 1, OP_LOCAL_GET, 1,
                    OP_I32_ADD, OP_END,
                ],
            ),
            func_body(
                &[],
                &[
                    OP_I32_CONST, 1, 0x40, 0, OP_DROP, OP_I32_CONST, 0, OP_LOCAL_GET, 0, 0x36,
                    2, 0, OP_LOCAL_GET, 0, 0x24, 0, OP_LOCAL_GET, 0, OP_CALL, 0, OP_END,
                ],
            ),
        ]),
    ])
}

fn trapping_module() -> Vec<u8> {
    module(&[
        type_section(&[(&[], &[])]),
        function_section(&[0]),
        export_section(&[("run", 0)]),
        code_section(&[func_body(&[], &[0x00, OP_END])]),
    ])
}

fn validated(wasm: &[u8]) -> ValidatedModule {
    let engine = WasmEngine::declared()
        .unwrap_or_else(|error| panic!("engine construction refused: {error}"));
    engine.validate(wasm)
        .unwrap_or_else(|error| panic!("module validation refused: {error}"))
}

fn trace_executor(policy: TracePolicy) -> Executor {
    let declared = ResourceBudget::declared();
    Executor::new(
        ResourceBudget::new_complete(
            TRACE_TEST_CPU_HEADROOM,
            declared.memory_bytes(),
            declared.storage_read_bytes(),
            declared.storage_write_bytes(),
            declared.output_values(),
            declared.output_bytes(),
            declared.table_elements(),
        ),
        FeeSchedule::declared(),
    )
    .with_trace_policy(policy)
}

fn traced_state_rich_call() -> layerx_programs_runtime::TracedExecutionRecord {
    let policy = TracePolicy::new(3, 256)
        .unwrap_or_else(|error| panic!("trace policy refused: {error}"));
    let record = trace_executor(policy)
        .execute_traced(&validated(&state_rich_module()), "run", &[WasmValue::I32(7)])
        .unwrap_or_else(|error| panic!("traced execution refused: {error}"));
    assert_eq!(record.execution.usage.cpu_fuel, STATE_RICH_TRACED_CPU_FUEL);
    record
}

#[test]
fn traced_execution_captures_complete_integer_runtime_state() {
    let record = traced_state_rich_call();
    assert_eq!(record.execution.outputs, vec![WasmValue::I32(18)]);
    assert_eq!(record.trace.policy().interval(), 3);
    assert!(!record.trace.steps().is_empty());
    assert!(!record.trace.commitments().is_empty());
    let mut expected_commitments = Vec::new();
    for step in record.trace.steps() {
        for commitment in [step.pre_commitment, step.post_commitment] {
            if expected_commitments.last().map(|prior: &layerx_programs_runtime::StepCommitment| prior.step_index)
                != Some(commitment.step_index)
            {
                expected_commitments.push(commitment);
            }
        }
    }
    assert_eq!(record.trace.commitments(), expected_commitments);
    let mut expected_arbitration_commitments = Vec::new();
    for step in record.trace.arbitration_steps() {
        for commitment in [step.pre_commitment, step.post_commitment] {
            if expected_arbitration_commitments
                .last()
                .map(|prior: &layerx_programs_runtime::ArbitrationStepCommitment| prior.step_index)
                != Some(commitment.step_index)
            {
                expected_arbitration_commitments.push(commitment);
            }
        }
    }
    assert_eq!(
        record.trace.arbitration_commitments(),
        expected_arbitration_commitments
    );
    assert!(record.trace.steps().iter().any(|step| {
        step.pre_state.globals.iter().any(|global| {
            global.mutable
                && matches!(global.value, layerx_programs_runtime::ExecutionValue::I32(7))
        })
    }));
    assert!(record.trace.steps().iter().all(|step| {
        step.pre_state.globals.iter().any(|global| {
            !global.mutable
                && matches!(global.value, layerx_programs_runtime::ExecutionValue::I32(9))
        })
    }));
    assert!(record.trace.steps().iter().any(|step| {
        step.post_state.linear_memory.len() > 65_536 || step.memory_expansion_bytes >= 65_536
    }));
    assert!(record.trace.steps().iter().any(|step| {
        step.pre_state.call_frames.len() > 1
            && step.pre_state.call_frames.iter().any(|frame| !frame.locals.is_empty())
    }));
    assert!(record.trace.steps().iter().any(|step| {
        step.pre_state.call_frames.len() > 1
            && step.pre_state.value_stack.len()
                > step.pre_state.call_frames.iter().map(|frame| frame.locals.len()).sum::<usize>()
    }));
}

#[test]
fn traced_execution_evidence_is_canonical_and_repeatable() {
    let first = traced_state_rich_call();
    let second = traced_state_rich_call();
    let first_evidence = first.canonical_evidence()
        .unwrap_or_else(|error| panic!("first evidence refused: {error}"));
    let second_evidence = second.canonical_evidence()
        .unwrap_or_else(|error| panic!("second evidence refused: {error}"));
    assert_eq!(first.trace, second.trace);
    assert_eq!(first_evidence, second_evidence);
}

#[test]
fn receipt_commitment_total_equals_the_metered_trace_delta() {
    let wasm = state_rich_module();
    let module = validated(&wasm);
    let plain = Executor::declared().execute(&module, "run", &[WasmValue::I32(7)])
        .unwrap_or_else(|error| panic!("plain execution refused: {error}"));
    let traced = traced_state_rich_call();
    let charged = traced.execution.usage.cpu_fuel.checked_sub(plain.usage.cpu_fuel)
        .unwrap_or_else(|| panic!("traced execution consumed less fuel than plain execution"));
    let total_trace_fuel = traced.trace.total_commitment_fuel()
        .checked_add(traced.trace.total_arbitration_commitment_fuel())
        .unwrap_or_else(|| panic!("trace fuel total overflowed"));
    assert_eq!(charged, total_trace_fuel);
    assert_eq!(
        traced.trace.total_commitment_fuel(),
        traced.trace.commitments().iter().map(|commitment| commitment.commitment_fuel).sum(),
    );
    assert_eq!(
        traced.trace.total_arbitration_commitment_fuel(),
        traced.trace.arbitration_commitments().iter().map(|commitment| commitment.commitment_fuel).sum(),
    );
}

#[test]
fn trace_identity_distinguishes_code_and_inputs() {
    let first = traced_state_rich_call();
    let policy = TracePolicy::new(3, 256)
        .unwrap_or_else(|error| panic!("trace policy refused: {error}"));
    let different_input = trace_executor(policy)
        .execute_traced(&validated(&state_rich_module()), "run", &[WasmValue::I32(8)])
        .unwrap_or_else(|error| panic!("traced execution refused: {error}"));
    let mut distinct_code = state_rich_module();
    let custom_name = b"distinct-module-identity";
    let mut custom_payload = unsigned_leb(custom_name.len() as u64);
    custom_payload.extend_from_slice(custom_name);
    distinct_code.extend(raw_section(0, &custom_payload));
    let different_code = trace_executor(policy)
        .execute_traced(&validated(&distinct_code), "run", &[WasmValue::I32(7)])
        .unwrap_or_else(|error| panic!("traced execution refused: {error}"));
    assert_ne!(first.trace.commitments()[0].digest, different_input.trace.commitments()[0].digest);
    assert_ne!(first.trace.commitments()[0].digest, different_code.trace.commitments()[0].digest);
}

#[test]
fn ordinary_observer_trace_is_the_complete_canonical_record() {
    let record = traced_state_rich_call();
    let evidence = record.canonical_evidence()
        .unwrap_or_else(|error| panic!("ordinary trace evidence refused: {error}"));
    let execution = record.execution.canonical_evidence();
    let trace = record.trace.canonical_arbitration_bytes()
        .unwrap_or_else(|error| panic!("ordinary trace encoding refused: {error}"));
    let mut complete = b"LXP/program-traced-execution/v2\0".to_vec();
    complete.extend_from_slice(&u32::try_from(execution.len())
        .unwrap_or_else(|_| panic!("execution evidence exceeds u32")).to_be_bytes());
    complete.extend_from_slice(&execution);
    complete.extend_from_slice(&u32::try_from(trace.len())
        .unwrap_or_else(|_| panic!("trace evidence exceeds u32")).to_be_bytes());
    complete.extend_from_slice(&trace);
    assert_eq!(evidence, complete);
}

#[test]
fn ordinary_observer_emits_complete_v2_arbitration_state() {
    let record = traced_state_rich_call();
    assert!(record.trace.is_arbitration_eligible());
    assert_eq!(
        record.trace.arbitration_steps().len(),
        record.trace.steps().len(),
    );
    for step in record.trace.arbitration_steps() {
        assert!(step.pre_commitment.arbitration_eligible());
        assert!(step.post_commitment.arbitration_eligible());
        assert!(!step.pre_state.engine_state.is_empty());
        assert!(!step.post_state.engine_state.is_empty());
        assert_ne!(step.pre_state.identity.module_code_hash, [0; 32]);
        assert_ne!(step.pre_state.identity.input_digest, [0; 32]);
        assert_eq!(
            step.pre_commitment,
            layerx_programs_runtime::ArbitrationStepCommitment::from_state(
                step.pre_state.as_ref(),
            ).unwrap_or_else(|error| panic!("pre-state v2 commitment refused: {error}")),
        );
        assert_eq!(
            step.post_commitment,
            layerx_programs_runtime::ArbitrationStepCommitment::from_state(
                step.post_state.as_ref(),
            ).unwrap_or_else(|error| panic!("post-state v2 commitment refused: {error}")),
        );
    }
}

#[test]
fn trapped_execution_refuses_partial_trace_evidence() {
    let policy = TracePolicy::new(1, 64)
        .unwrap_or_else(|error| panic!("trace policy refused: {error}"));
    let result = trace_executor(policy)
        .execute_traced(&validated(&trapping_module()), "run", &[]);
    match result {
        Err(ExecutionError::Fault(ExecutionFault::EngineFault { reason })) => {
            assert!(reason.contains("execution observer refused"));
        }
        other => panic!("trapped trace did not fail closed: {other:?}"),
    }
}

#[test]
fn receipt_commitment_bound_refuses_an_incomplete_chain() {
    let policy = TracePolicy::new(1, 1)
        .unwrap_or_else(|error| panic!("trace policy refused: {error}"));
    let result = trace_executor(policy)
        .execute_traced(&validated(&state_rich_module()), "run", &[WasmValue::I32(7)]);
    match result {
        Err(ExecutionError::Fault(ExecutionFault::EngineFault { reason })) => {
            assert_eq!(
                reason,
                "deterministic execution commitment refused: execution trace exceeds commitment limit 1"
            );
        }
        other => panic!("bounded trace returned partial evidence: {other:?}"),
    }
}
