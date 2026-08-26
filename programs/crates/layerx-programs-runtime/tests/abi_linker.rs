use layerx_programs_runtime::test_support::{import_section, module, type_section, TYPE_I32, TYPE_I64};
use layerx_programs_runtime::{
    HostFunction, ValidationRefusal, WasmEngine, ABI_MODULE, ABI_V2_HOST_FUNCTIONS,
    ABI_V2_MODULE, HOST_FUNCTIONS,
};

fn value_types(signature: &str) -> (Vec<u8>, Vec<u8>) {
    let (parameters, result) = signature
        .split_once(")->")
        .unwrap_or_else(|| panic!("malformed canonical signature {signature}"));
    let parameters = parameters
        .strip_prefix('(')
        .unwrap_or_else(|| panic!("malformed canonical parameters {signature}"));
    let parameters = if parameters.is_empty() {
        Vec::new()
    } else {
        parameters
            .split(',')
            .map(|value| match value {
                "i32" => TYPE_I32,
                "i64" => TYPE_I64,
                _ => panic!("non-integer canonical parameter {value}"),
            })
            .collect()
    };
    let results = match result {
        "i32" => vec![TYPE_I32],
        "i64" => vec![TYPE_I64],
        _ => panic!("non-integer canonical result {result}"),
    };
    (parameters, results)
}

fn importing(module_name: &str, function: HostFunction) -> Vec<u8> {
    let (parameters, results) = value_types(function.signature);
    module(&[
        type_section(&[(&parameters, &results)]),
        import_section(&[(module_name, function.name, 0)]),
    ])
}

#[test]
fn every_frozen_import_instantiates_against_its_revision_linker() {
    let engine = WasmEngine::declared().unwrap_or_else(|error| panic!("engine: {error}"));
    for function in HOST_FUNCTIONS {
        let wasm = importing(ABI_MODULE, function);
        engine
            .validate(&wasm)
            .unwrap_or_else(|error| panic!("v1 {} validation: {error}", function.name))
            .instantiate_for_qualification()
            .unwrap_or_else(|error| panic!("v1 {} linking: {error}", function.name));
        engine
            .validate_v2(&wasm)
            .unwrap_or_else(|error| panic!("v2 inherited {} validation: {error}", function.name))
            .instantiate_for_qualification()
            .unwrap_or_else(|error| panic!("v2 inherited {} linking: {error}", function.name));
    }
    for function in ABI_V2_HOST_FUNCTIONS {
        let wasm = importing(ABI_V2_MODULE, function);
        assert!(matches!(
            engine.validate(&wasm),
            Err(ValidationRefusal::ForbiddenImport { .. })
        ));
        engine
            .validate_v2(&wasm)
            .unwrap_or_else(|error| panic!("v2 {} validation: {error}", function.name))
            .instantiate_for_qualification()
            .unwrap_or_else(|error| panic!("v2 {} linking: {error}", function.name));
    }
}

#[test]
fn wrong_signatures_duplicates_and_extra_imports_are_rejected() {
    let engine = WasmEngine::declared().unwrap_or_else(|error| panic!("engine: {error}"));
    let function = ABI_V2_HOST_FUNCTIONS[0];
    let (parameters, results) = value_types(function.signature);
    let wrong_result = if results == [TYPE_I32] { TYPE_I64 } else { TYPE_I32 };
    let wrong = module(&[
        type_section(&[(&parameters, &[wrong_result])]),
        import_section(&[(ABI_V2_MODULE, function.name, 0)]),
    ]);
    assert!(matches!(
        engine.validate_v2(&wrong),
        Err(ValidationRefusal::WrongImportSignature { .. })
    ));

    let duplicate = module(&[
        type_section(&[(&parameters, &results)]),
        import_section(&[
            (ABI_V2_MODULE, function.name, 0),
            (ABI_V2_MODULE, function.name, 0),
        ]),
    ]);
    assert!(matches!(
        engine.validate_v2(&duplicate),
        Err(ValidationRefusal::DuplicateImport { .. })
    ));

    let extra = module(&[
        type_section(&[(&[], &[TYPE_I32])]),
        import_section(&[(ABI_V2_MODULE, "undeclared", 0)]),
    ]);
    assert!(matches!(
        engine.validate_v2(&extra),
        Err(ValidationRefusal::ForbiddenImport { .. })
    ));
}
