use layerx_programs_runtime::test_support::{
    add_module, code_section, export_section, func_body, function_section, module, type_section,
    OP_CALL, OP_END, OP_I32_DIV_S, OP_LOCAL_GET, TYPE_I32,
};
use layerx_programs_runtime::{
    ExecutionFault, ValidatedModule, ValidationLimits, WasmEngine, WasmValue,
};

fn validated(engine: &WasmEngine, wasm: &[u8]) -> ValidatedModule {
    match engine.validate(wasm) {
        Ok(validated) => validated,
        Err(refusal) => panic!("module refused: {refusal}"),
    }
}

fn declared_engine() -> WasmEngine {
    match WasmEngine::new(ValidationLimits::declared()) {
        Ok(engine) => engine,
        Err(refusal) => panic!("engine construction refused: {refusal}"),
    }
}

fn div_module() -> Vec<u8> {
    module(&[
        type_section(&[(&[TYPE_I32, TYPE_I32], &[TYPE_I32])]),
        function_section(&[0]),
        export_section(&[("div", 0)]),
        code_section(&[func_body(
            &[],
            &[OP_LOCAL_GET, 0, OP_LOCAL_GET, 1, OP_I32_DIV_S, OP_END],
        )]),
    ])
}

fn recursion_module() -> Vec<u8> {
    module(&[
        type_section(&[(&[], &[])]),
        function_section(&[0]),
        export_section(&[("run", 0)]),
        code_section(&[func_body(&[], &[OP_CALL, 0, OP_END])]),
    ])
}

#[test]
fn validated_integer_module_executes() {
    let engine = declared_engine();
    let mut instance = match validated(&engine, &add_module()).instantiate() {
        Ok(instance) => instance,
        Err(fault) => panic!("instantiation faulted: {fault}"),
    };
    let result = instance.call("add", &[WasmValue::I32(19), WasmValue::I32(23)]);
    assert_eq!(result, Ok(vec![WasmValue::I32(42)]));
}

#[test]
fn execution_is_repeatable_within_an_instance() {
    let engine = declared_engine();
    let mut instance = match validated(&engine, &add_module()).instantiate() {
        Ok(instance) => instance,
        Err(fault) => panic!("instantiation faulted: {fault}"),
    };
    let first = instance.call("add", &[WasmValue::I32(-7), WasmValue::I32(3)]);
    let second = instance.call("add", &[WasmValue::I32(-7), WasmValue::I32(3)]);
    assert_eq!(first, Ok(vec![WasmValue::I32(-4)]));
    assert_eq!(first, second);
}

#[test]
fn integer_division_by_zero_faults_typed() {
    let engine = declared_engine();
    let mut instance = match validated(&engine, &div_module()).instantiate() {
        Ok(instance) => instance,
        Err(fault) => panic!("instantiation faulted: {fault}"),
    };
    let result = instance.call("div", &[WasmValue::I32(7), WasmValue::I32(0)]);
    assert_eq!(result, Err(ExecutionFault::IntegerDivisionByZero));
    let usage = instance
        .meter()
        .finish()
        .unwrap_or_else(|error| panic!("trap usage: {error}"));
    assert_eq!(usage.output_values, 0);
}

#[test]
fn integer_overflow_faults_typed() {
    let engine = declared_engine();
    let mut instance = match validated(&engine, &div_module()).instantiate() {
        Ok(instance) => instance,
        Err(fault) => panic!("instantiation faulted: {fault}"),
    };
    let result = instance.call("div", &[WasmValue::I32(i32::MIN), WasmValue::I32(-1)]);
    assert_eq!(result, Err(ExecutionFault::IntegerOverflow));
}

#[test]
fn declared_call_depth_limit_faults_typed() {
    let limits = match ValidationLimits::new(1_048_576, 4_096, 65_536, 16) {
        Ok(limits) => limits,
        Err(refusal) => panic!("limit construction refused: {refusal}"),
    };
    let engine = match WasmEngine::new(limits) {
        Ok(engine) => engine,
        Err(refusal) => panic!("engine construction refused: {refusal}"),
    };
    let mut instance = match validated(&engine, &recursion_module()).instantiate() {
        Ok(instance) => instance,
        Err(fault) => panic!("instantiation faulted: {fault}"),
    };
    let result = instance.call("run", &[]);
    assert_eq!(result, Err(ExecutionFault::StackExhausted));
    let usage = instance
        .meter()
        .finish()
        .unwrap_or_else(|error| panic!("trap usage: {error}"));
    assert_eq!(usage.output_values, 0);
}

#[test]
fn unknown_export_faults_typed() {
    let engine = declared_engine();
    let mut instance = match validated(&engine, &add_module()).instantiate() {
        Ok(instance) => instance,
        Err(fault) => panic!("instantiation faulted: {fault}"),
    };
    let result = instance.call("absent", &[]);
    assert_eq!(
        result,
        Err(ExecutionFault::UnknownExport {
            name: "absent".to_string(),
        })
    );
}
