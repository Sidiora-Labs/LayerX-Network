use layerx_programs_runtime::test_support::{
    add_module, code_section, export_section, func_body, function_section, import_section, module,
    padding_section, type_section, OP_DROP, OP_END, OP_F32_CONST, OP_I32_CONST, TYPE_F32, TYPE_F64,
    TYPE_I32, TYPE_I64, TYPE_V128,
};
use layerx_programs_runtime::{
    DeclaredLimit, LimitsRefusal, ValidationLimits, ValidationRefusal, WasmEngine,
};

fn engine_with(limits: ValidationLimits) -> WasmEngine {
    match WasmEngine::new(limits) {
        Ok(engine) => engine,
        Err(refusal) => panic!("engine construction refused: {refusal}"),
    }
}

fn declared_engine() -> WasmEngine {
    engine_with(ValidationLimits::declared())
}

#[test]
fn frozen_v2_validation_retains_compatibility_spelling() {
    let engine = declared_engine();
    let wasm = add_module();
    assert!(engine.validate_v2(&wasm).is_ok());
    assert!(engine.validate_candidate_v2(&wasm).is_ok());
    let invalid = b"not a wasm module";
    assert_eq!(
        engine.validate_v2(invalid).err(),
        engine.validate_candidate_v2(invalid).err()
    );
    assert_eq!(
        layerx_programs_runtime::programs_wasm_engine(),
        "wasmi 0.31.2 vendored at programs/vendor/wasmi-0.31.2"
    );
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

fn refusal_of(engine: &WasmEngine, wasm: &[u8]) -> ValidationRefusal {
    match engine.validate(wasm) {
        Err(refusal) => refusal,
        Ok(_) => panic!("module unexpectedly accepted"),
    }
}

#[test]
fn valid_integer_module_is_accepted() {
    let engine = declared_engine();
    let validated = match engine.validate(&add_module()) {
        Ok(validated) => validated,
        Err(refusal) => panic!("integer module refused: {refusal}"),
    };
    assert_eq!(validated.function_count(), 1);
    assert_eq!(validated.byte_size(), add_module().len() as u64);
}

#[test]
fn zero_limits_are_refused_with_the_offending_limit_named() {
    assert_eq!(
        ValidationLimits::new(0, 1, 1, 1),
        Err(LimitsRefusal::ZeroLimit {
            limit: DeclaredLimit::ModuleBytes
        })
    );
    assert_eq!(
        ValidationLimits::new(1, 0, 1, 1),
        Err(LimitsRefusal::ZeroLimit {
            limit: DeclaredLimit::Functions
        })
    );
    assert_eq!(
        ValidationLimits::new(1, 1, 0, 1),
        Err(LimitsRefusal::ZeroLimit {
            limit: DeclaredLimit::ValueStackHeight
        })
    );
    assert_eq!(
        ValidationLimits::new(1, 1, 1, 0),
        Err(LimitsRefusal::ZeroLimit {
            limit: DeclaredLimit::CallDepth
        })
    );
}

#[test]
fn oversized_module_is_refused_under_a_configured_limit() {
    let engine = engine_with(limits(64, 16, 1_024, 16));
    let wasm = module(&[padding_section(128)]);
    assert_eq!(
        refusal_of(&engine, &wasm),
        ValidationRefusal::ModuleTooLarge {
            byte_size: wasm.len() as u64,
            limit: 64,
        }
    );
}

#[test]
fn oversized_module_is_refused_under_the_declared_limit() {
    let engine = declared_engine();
    let wasm = module(&[padding_section(1_048_576)]);
    assert_eq!(
        refusal_of(&engine, &wasm),
        ValidationRefusal::ModuleTooLarge {
            byte_size: wasm.len() as u64,
            limit: 1_048_576,
        }
    );
}

#[test]
fn function_count_beyond_the_declared_limit_is_refused() {
    let engine = engine_with(limits(65_536, 4, 1_024, 16));
    let bodies: Vec<Vec<u8>> = (0..5).map(|_| func_body(&[], &[OP_END])).collect();
    let wasm = module(&[
        type_section(&[(&[], &[])]),
        function_section(&[0, 0, 0, 0, 0]),
        code_section(&bodies),
    ]);
    assert_eq!(
        refusal_of(&engine, &wasm),
        ValidationRefusal::TooManyFunctions {
            function_count: 5,
            limit: 4,
        }
    );
}

#[test]
fn clock_import_is_refused_by_name() {
    let engine = declared_engine();
    let wasm = module(&[
        type_section(&[(&[TYPE_I32, TYPE_I64, TYPE_I32], &[TYPE_I32])]),
        import_section(&[("wasi_snapshot_preview1", "clock_time_get", 0)]),
    ]);
    assert_eq!(
        refusal_of(&engine, &wasm),
        ValidationRefusal::ForbiddenImport {
            import_module: "wasi_snapshot_preview1".to_string(),
            import_name: "clock_time_get".to_string(),
        }
    );
}

#[test]
fn randomness_import_is_refused_by_name() {
    let engine = declared_engine();
    let wasm = module(&[
        type_section(&[(&[TYPE_I32, TYPE_I32], &[TYPE_I32])]),
        import_section(&[("wasi_snapshot_preview1", "random_get", 0)]),
    ]);
    assert_eq!(
        refusal_of(&engine, &wasm),
        ValidationRefusal::ForbiddenImport {
            import_module: "wasi_snapshot_preview1".to_string(),
            import_name: "random_get".to_string(),
        }
    );
}

#[test]
fn host_environment_import_is_refused_by_name() {
    let engine = declared_engine();
    let wasm = module(&[
        type_section(&[(&[TYPE_I32], &[TYPE_I32])]),
        import_section(&[("env", "fd_write", 0)]),
    ]);
    assert_eq!(
        refusal_of(&engine, &wasm),
        ValidationRefusal::ForbiddenImport {
            import_module: "env".to_string(),
            import_name: "fd_write".to_string(),
        }
    );
}

#[test]
fn float_signature_is_refused() {
    let engine = declared_engine();
    let wasm = module(&[type_section(&[(&[TYPE_F32], &[])])]);
    assert_eq!(
        refusal_of(&engine, &wasm),
        ValidationRefusal::ForbiddenFloatType
    );
}

#[test]
fn float_local_is_refused() {
    let engine = declared_engine();
    let wasm = module(&[
        type_section(&[(&[], &[])]),
        function_section(&[0]),
        code_section(&[func_body(&[(1, TYPE_F64)], &[OP_END])]),
    ]);
    assert_eq!(
        refusal_of(&engine, &wasm),
        ValidationRefusal::ForbiddenFloatType
    );
}

#[test]
fn float_instruction_is_refused() {
    let engine = declared_engine();
    let wasm = module(&[
        type_section(&[(&[], &[TYPE_I32])]),
        function_section(&[0]),
        export_section(&[("float", 0)]),
        code_section(&[func_body(
            &[],
            &[OP_F32_CONST, 0, 0, 0, 0, OP_DROP, OP_I32_CONST, 0, OP_END],
        )]),
    ]);
    assert_eq!(
        refusal_of(&engine, &wasm),
        ValidationRefusal::ForbiddenFloatInstruction
    );
}

#[test]
fn vector_type_is_refused() {
    let engine = declared_engine();
    let wasm = module(&[type_section(&[(&[TYPE_V128], &[])])]);
    assert_eq!(
        refusal_of(&engine, &wasm),
        ValidationRefusal::ForbiddenVectorType
    );
}

#[test]
fn malformed_bytes_are_refused() {
    let engine = declared_engine();
    let refusal = engine.validate(&[0x00, 0x61, 0x73]);
    assert!(matches!(
        refusal,
        Err(ValidationRefusal::MalformedModule { .. })
    ));
}
