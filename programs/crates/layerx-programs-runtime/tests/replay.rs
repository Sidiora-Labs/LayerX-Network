use layerx_programs_runtime::test_support::{
    add_module, code_section, func_body, function_section, module, padding_section, type_section,
    OP_END,
};
use layerx_programs_runtime::{
    hash_bytes, programs_differential_gate, replay_recorded_execution, Deploy, Executor,
    HashAlgorithm, Lifecycle, LifecycleRefusal, ProgramId, RecordedExecution, ReplayRefusal,
    UpgradePolicy, ValidationLimits, ValidationRefusal, WasmEngine, WasmValue, ABI_VERSION,
    RUNTIME_VERSION,
};

#[test]
fn independent_engines_produce_identical_evidence() {
    let wasm = add_module();
    let evidence =
        programs_differential_gate(&wasm, "add", &[WasmValue::I32(20), WasmValue::I32(22)]);
    assert!(evidence.is_ok(), "independent runtime builds diverged");
}

#[test]
fn recorded_v1_replays_identically_after_a_simulated_upgrade() {
    let wasm = add_module();
    let record = RecordedExecution {
        runtime_version: RUNTIME_VERSION,
        abi_version: ABI_VERSION,
        wasm: &wasm,
        export: "add",
        args: &[WasmValue::I32(20), WasmValue::I32(22)],
    };
    let before = replay_recorded_execution(&record);
    let after = replay_recorded_execution(&record);
    assert_eq!(before, after);
}

#[test]
fn unknown_runtime_and_abi_artifacts_are_preserved_without_execution() {
    let wasm = add_module();
    let runtime = RecordedExecution {
        runtime_version: RUNTIME_VERSION + 1,
        abi_version: ABI_VERSION,
        wasm: &wasm,
        export: "add",
        args: &[],
    };
    assert_eq!(
        replay_recorded_execution(&runtime),
        Err(ReplayRefusal::UnknownRuntimeVersion {
            version: RUNTIME_VERSION + 1,
        })
    );
    let abi = RecordedExecution {
        runtime_version: RUNTIME_VERSION,
        abi_version: ABI_VERSION + 1,
        ..runtime
    };
    assert_eq!(
        replay_recorded_execution(&abi),
        Err(ReplayRefusal::UnknownAbiVersion {
            version: ABI_VERSION + 1
        })
    );
}

/// Committed differential vectors. Each is executed twice through independently
/// constructed engines and executors; the two evidence bundles - state roots,
/// receipts and events alike, all folded into the canonical evidence - MUST be
/// byte-identical. Both accepted and rejected inputs are included so the gate
/// proves agreement on refusals as well as successes.
fn differential_vectors() -> Vec<(Vec<u8>, &'static str, Vec<WasmValue>)> {
    vec![
        (
            add_module(),
            "add",
            vec![WasmValue::I32(20), WasmValue::I32(22)],
        ),
        (add_module(), "add", vec![WasmValue::I32(0), WasmValue::I32(0)]),
        (
            add_module(),
            "add",
            vec![WasmValue::I32(-7), WasmValue::I32(7)],
        ),
        (
            add_module(),
            "add",
            vec![WasmValue::I32(i32::MAX), WasmValue::I32(1)],
        ),
        (
            add_module(),
            "add",
            vec![WasmValue::I32(i32::MIN), WasmValue::I32(-1)],
        ),
        // A missing export is a typed refusal; both builds must refuse alike.
        (add_module(), "absent", vec![]),
        // Malformed bytes are refused during validation; both builds must agree.
        (vec![0x00, 0x61, 0x73, 0x6d, 0x01], "add", vec![]),
    ]
}

#[test]
fn differential_gate_agrees_on_every_committed_vector() {
    for (index, (wasm, export, args)) in differential_vectors().into_iter().enumerate() {
        match programs_differential_gate(&wasm, export, &args) {
            Ok(evidence) => assert!(
                !evidence.is_empty(),
                "vector {index} produced empty evidence"
            ),
            Err(mismatch) => panic!("vector {index} diverged across builds: {mismatch:?}"),
        }
    }
}

#[test]
fn differential_gate_evidence_is_reproducible_per_vector() {
    for (index, (wasm, export, args)) in differential_vectors().into_iter().enumerate() {
        let first = programs_differential_gate(&wasm, export, &args);
        let second = programs_differential_gate(&wasm, export, &args);
        assert_eq!(first, second, "vector {index} was not reproducible");
    }
}

fn engine_with(limits: ValidationLimits) -> WasmEngine {
    match WasmEngine::new(limits) {
        Ok(engine) => engine,
        Err(refusal) => panic!("engine construction refused: {refusal}"),
    }
}

fn limits(
    max_module_bytes: u64,
    max_functions: u32,
    max_value_stack_height: u32,
    max_call_depth: u32,
) -> ValidationLimits {
    match ValidationLimits::new(
        max_module_bytes,
        max_functions,
        max_value_stack_height,
        max_call_depth,
    ) {
        Ok(limits) => limits,
        Err(refusal) => panic!("limit construction refused: {refusal}"),
    }
}

fn program_id(byte: u8) -> ProgramId {
    match ProgramId::new([byte; 32]) {
        Ok(program) => program,
        Err(refusal) => panic!("program id refused: {refusal}"),
    }
}

fn deploy_activity(byte: u8, wasm: Vec<u8>) -> Deploy {
    let code_hash = hash_bytes(HashAlgorithm::Sha256, &wasm)
        .unwrap_or_else(|error| panic!("program code hash refused: {error}"));
    Deploy {
        program: program_id(byte),
        code_hash,
        wasm,
        abi_version: ABI_VERSION,
        upgrade_policy: UpgradePolicy::Immutable,
    }
}

#[test]
fn oversized_module_is_refused_at_deploy_time_with_a_typed_result() {
    let mut lifecycle = Lifecycle::new(engine_with(limits(64, 16, 1_024, 16)), Executor::declared());
    let wasm = module(&[padding_section(128)]);
    let refusal = lifecycle.deploy(deploy_activity(1, wasm.clone()));
    assert_eq!(
        refusal,
        Err(LifecycleRefusal::Validation(
            ValidationRefusal::ModuleTooLarge {
                byte_size: wasm.len() as u64,
                limit: 64,
            }
        ))
    );
    assert_eq!(
        lifecycle.diagnostics().len(),
        1,
        "the refused module must be preserved for diagnosis"
    );
}

#[test]
fn function_count_over_limit_is_refused_at_deploy_time_with_a_typed_result() {
    let mut lifecycle =
        Lifecycle::new(engine_with(limits(65_536, 4, 1_024, 16)), Executor::declared());
    let bodies: Vec<Vec<u8>> = (0..5).map(|_| func_body(&[], &[OP_END])).collect();
    let wasm = module(&[
        type_section(&[(&[], &[])]),
        function_section(&[0, 0, 0, 0, 0]),
        code_section(&bodies),
    ]);
    let refusal = lifecycle.deploy(deploy_activity(2, wasm));
    assert_eq!(
        refusal,
        Err(LifecycleRefusal::Validation(
            ValidationRefusal::TooManyFunctions {
                function_count: 5,
                limit: 4,
            }
        ))
    );
    assert_eq!(lifecycle.diagnostics().len(), 1);
}

#[test]
fn declared_validation_limits_expose_every_named_bound() {
    let declared = ValidationLimits::declared();
    assert!(declared.max_module_bytes() > 0);
    assert!(declared.max_functions() > 0);
    assert!(declared.max_value_stack_height() > 0);
    assert!(declared.max_call_depth() > 0);
}

#[test]
fn a_valid_module_deploys_under_declared_limits() {
    let mut lifecycle = match Lifecycle::declared() {
        Ok(lifecycle) => lifecycle,
        Err(refusal) => panic!("declared lifecycle refused: {refusal}"),
    };
    let receipt = match lifecycle.deploy(deploy_activity(3, add_module())) {
        Ok(receipt) => receipt,
        Err(refusal) => panic!("valid module refused at deploy: {refusal}"),
    };
    assert_eq!(receipt.version(), 1);
    assert!(lifecycle.diagnostics().is_empty());
}
