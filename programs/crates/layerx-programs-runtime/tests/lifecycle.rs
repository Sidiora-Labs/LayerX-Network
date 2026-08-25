use layerx_programs_runtime::test_support::{
    add_module, code_section, export_section, func_body, function_section, module, type_section,
    OP_END, OP_I32_CONST, TYPE_I32,
};
use layerx_programs_runtime::{
    hash_bytes, Deploy, HashAlgorithm, Lifecycle, LifecycleRefusal, Migration, ProgramId, Upgrade,
    UpgradePolicy, ValidatedModule, WasmEngine, WasmValue, ABI_VERSION,
};

fn program(byte: u8) -> ProgramId {
    ProgramId::new([byte; 32]).unwrap_or_else(|error| panic!("program id refused: {error}"))
}

fn code_hash(wasm: &[u8]) -> [u8; 32] {
    hash_bytes(HashAlgorithm::Sha256, wasm)
        .unwrap_or_else(|error| panic!("program code hash refused: {error}"))
}

fn call_deployed(wasm: &[u8], export: &str, args: &[WasmValue]) -> Vec<WasmValue> {
    let engine =
        WasmEngine::declared().unwrap_or_else(|error| panic!("engine construction refused: {error}"));
    let validated: ValidatedModule = engine
        .validate(wasm)
        .unwrap_or_else(|error| panic!("deployed module refused: {error}"));
    let mut instance = validated
        .instantiate()
        .unwrap_or_else(|fault| panic!("deployed module instantiation faulted: {fault}"));
    instance
        .call(export, args)
        .unwrap_or_else(|fault| panic!("deployed export faulted: {fault}"))
}

fn migration_module() -> Vec<u8> {
    module(&[
        type_section(&[(&[], &[TYPE_I32])]),
        function_section(&[0]),
        export_section(&[("migrate", 0)]),
        code_section(&[func_body(&[], &[OP_I32_CONST, 0, OP_END])]),
    ])
}

#[test]
fn deploy_becomes_callable_only_with_verified_receipt() {
    let mut lifecycle = Lifecycle::declared()
        .unwrap_or_else(|error| panic!("lifecycle construction refused: {error}"));
    let wasm = add_module();
    let expected_code_hash = code_hash(&wasm);
    let receipt = lifecycle
        .deploy(Deploy {
            program: program(1),
            code_hash: expected_code_hash,
            wasm,
            abi_version: ABI_VERSION,
            upgrade_policy: UpgradePolicy::default(),
        })
        .unwrap_or_else(|error| panic!("deployment refused: {error}"));
    assert_eq!(
        lifecycle.callable(&receipt, false),
        Err(LifecycleRefusal::UnverifiedReceipt)
    );
    let callable = lifecycle
        .callable(&receipt, true)
        .unwrap_or_else(|error| panic!("verified deployment not callable: {error}"));
    assert_eq!(callable.code_hash, expected_code_hash);
}

#[test]
fn immutable_is_default_and_upgrade_records_hash_history() {
    let mut immutable = Lifecycle::declared()
        .unwrap_or_else(|error| panic!("lifecycle construction refused: {error}"));
    let immutable_wasm = add_module();
    immutable
        .deploy(Deploy {
            program: program(3),
            code_hash: code_hash(&immutable_wasm),
            wasm: immutable_wasm,
            abi_version: ABI_VERSION,
            upgrade_policy: UpgradePolicy::default(),
        })
        .unwrap_or_else(|error| panic!("deployment refused: {error}"));
    let immutable_upgrade_wasm = migration_module();
    let request = Upgrade {
        program: program(3),
        authority: [9; 32],
        code_hash: code_hash(&immutable_upgrade_wasm),
        wasm: immutable_upgrade_wasm,
        abi_version: ABI_VERSION,
        migration: Some(Migration {
            export: "migrate".to_string(),
        }),
    };
    assert_eq!(immutable.upgrade(request), Err(LifecycleRefusal::Immutable));

    let mut upgradeable = Lifecycle::declared()
        .unwrap_or_else(|error| panic!("lifecycle construction refused: {error}"));
    let upgradeable_wasm = add_module();
    let deployed_hash = code_hash(&upgradeable_wasm);
    upgradeable
        .deploy(Deploy {
            program: program(6),
            code_hash: deployed_hash,
            wasm: upgradeable_wasm,
            abi_version: ABI_VERSION,
            upgrade_policy: UpgradePolicy::Authority([9; 32]),
        })
        .unwrap_or_else(|error| panic!("deployment refused: {error}"));
    let upgrade_wasm = migration_module();
    let receipt = upgradeable
        .upgrade(Upgrade {
            program: program(6),
            authority: [9; 32],
            code_hash: code_hash(&upgrade_wasm),
            wasm: upgrade_wasm,
            abi_version: ABI_VERSION,
            migration: Some(Migration {
                export: "migrate".to_string(),
            }),
        })
        .unwrap_or_else(|error| panic!("upgrade refused: {error}"));
    assert_eq!(receipt.version, 2);
    assert_eq!(receipt.old_code_hash, Some(deployed_hash));
    assert!(receipt.migration.is_some());
}

#[test]
fn deploy_call_upgrade_and_migration_run_end_to_end() {
    let mut lifecycle = Lifecycle::declared()
        .unwrap_or_else(|error| panic!("lifecycle construction refused: {error}"));

    let deploy_wasm = add_module();
    let deploy_hash = code_hash(&deploy_wasm);
    let deploy_receipt = lifecycle
        .deploy(Deploy {
            program: program(20),
            code_hash: deploy_hash,
            wasm: deploy_wasm,
            abi_version: ABI_VERSION,
            upgrade_policy: UpgradePolicy::Authority([22; 32]),
        })
        .unwrap_or_else(|error| panic!("deployment refused: {error}"));
    assert_eq!(deploy_receipt.version, 1);
    assert_eq!(deploy_receipt.old_code_hash, None);
    assert!(deploy_receipt.migration.is_none());

    assert_eq!(
        lifecycle.callable(&deploy_receipt, false),
        Err(LifecycleRefusal::UnverifiedReceipt)
    );
    let deployed = lifecycle
        .callable(&deploy_receipt, true)
        .unwrap_or_else(|error| panic!("verified deployment not callable: {error}"));
    assert_eq!(deployed.code_hash, deploy_hash);
    assert_eq!(
        call_deployed(
            &deployed.wasm,
            "add",
            &[WasmValue::I32(19), WasmValue::I32(23)],
        ),
        vec![WasmValue::I32(42)]
    );

    let upgrade_wasm = migration_module();
    let upgrade_hash = code_hash(&upgrade_wasm);
    let upgrade_receipt = lifecycle
        .upgrade(Upgrade {
            program: program(20),
            authority: [22; 32],
            code_hash: upgrade_hash,
            wasm: upgrade_wasm,
            abi_version: ABI_VERSION,
            migration: Some(Migration {
                export: "migrate".to_string(),
            }),
        })
        .unwrap_or_else(|error| panic!("upgrade refused: {error}"));
    assert_eq!(upgrade_receipt.version, 2);
    assert_eq!(upgrade_receipt.old_code_hash, Some(deploy_hash));
    assert_eq!(upgrade_receipt.new_code_hash, upgrade_hash);
    assert!(upgrade_receipt.migration.is_some());

    let upgraded = lifecycle
        .callable(&upgrade_receipt, true)
        .unwrap_or_else(|error| panic!("verified upgrade not callable: {error}"));
    assert_eq!(upgraded.code_hash, upgrade_hash);
    assert_eq!(
        call_deployed(&upgraded.wasm, "migrate", &[]),
        vec![WasmValue::I32(0)]
    );

    let original = lifecycle
        .callable(&deploy_receipt, true)
        .unwrap_or_else(|error| panic!("original version not callable: {error}"));
    assert_eq!(original.code_hash, deploy_hash);
}

#[test]
fn deploy_refuses_a_mismatched_code_hash_without_installing_the_program() {
    let mut lifecycle = Lifecycle::declared()
        .unwrap_or_else(|error| panic!("lifecycle construction refused: {error}"));
    let wasm = add_module();
    let computed = code_hash(&wasm);
    let mut declared = computed;
    declared[0] ^= 1;

    assert_eq!(
        lifecycle.deploy(Deploy {
            program: program(24),
            code_hash: declared,
            wasm: wasm.clone(),
            abi_version: ABI_VERSION,
            upgrade_policy: UpgradePolicy::Immutable,
        }),
        Err(LifecycleRefusal::CodeHashMismatch { declared, computed })
    );

    let receipt = lifecycle
        .deploy(Deploy {
            program: program(24),
            code_hash: computed,
            wasm,
            abi_version: ABI_VERSION,
            upgrade_policy: UpgradePolicy::Immutable,
        })
        .unwrap_or_else(|error| panic!("matching deployment refused: {error}"));
    assert_eq!(receipt.version, 1);
}

#[test]
fn upgrade_refuses_a_mismatched_code_hash_without_advancing_history() {
    let mut lifecycle = Lifecycle::declared()
        .unwrap_or_else(|error| panic!("lifecycle construction refused: {error}"));
    let deployed_wasm = add_module();
    let deployed_hash = code_hash(&deployed_wasm);
    lifecycle
        .deploy(Deploy {
            program: program(25),
            code_hash: deployed_hash,
            wasm: deployed_wasm,
            abi_version: ABI_VERSION,
            upgrade_policy: UpgradePolicy::Authority([0x25; 32]),
        })
        .unwrap_or_else(|error| panic!("deployment refused: {error}"));
    let upgrade_wasm = migration_module();
    let computed = code_hash(&upgrade_wasm);
    let mut declared = computed;
    declared[0] ^= 1;

    assert_eq!(
        lifecycle.upgrade(Upgrade {
            program: program(25),
            authority: [0x25; 32],
            code_hash: declared,
            wasm: upgrade_wasm.clone(),
            abi_version: ABI_VERSION,
            migration: None,
        }),
        Err(LifecycleRefusal::CodeHashMismatch { declared, computed })
    );

    let receipt = lifecycle
        .upgrade(Upgrade {
            program: program(25),
            authority: [0x25; 32],
            code_hash: computed,
            wasm: upgrade_wasm,
            abi_version: ABI_VERSION,
            migration: None,
        })
        .unwrap_or_else(|error| panic!("matching upgrade refused: {error}"));
    assert_eq!(receipt.version, 2);
    assert_eq!(receipt.old_code_hash, Some(deployed_hash));
}

#[test]
fn zero_upgrade_authorities_are_never_admitted() {
    let mut lifecycle = Lifecycle::declared()
        .unwrap_or_else(|error| panic!("lifecycle construction refused: {error}"));
    let wasm = add_module();
    let digest = code_hash(&wasm);
    assert_eq!(
        lifecycle.deploy(Deploy {
            program: program(26),
            code_hash: digest,
            wasm: wasm.clone(),
            abi_version: ABI_VERSION,
            upgrade_policy: UpgradePolicy::Authority([0; 32]),
        }),
        Err(LifecycleRefusal::InvalidAuthority)
    );
    lifecycle
        .deploy(Deploy {
            program: program(26),
            code_hash: digest,
            wasm,
            abi_version: ABI_VERSION,
            upgrade_policy: UpgradePolicy::Authority([0x26; 32]),
        })
        .unwrap_or_else(|error| panic!("nonzero authority deployment refused: {error}"));
    let upgrade_wasm = migration_module();
    assert_eq!(
        lifecycle.upgrade(Upgrade {
            program: program(26),
            authority: [0; 32],
            code_hash: code_hash(&upgrade_wasm),
            wasm: upgrade_wasm,
            abi_version: ABI_VERSION,
            migration: None,
        }),
        Err(LifecycleRefusal::InvalidAuthority)
    );
}

#[test]
fn failed_validation_is_preserved_but_never_executable() {
    let mut lifecycle = Lifecycle::declared()
        .unwrap_or_else(|error| panic!("lifecycle construction refused: {error}"));
    let result = lifecycle.deploy(Deploy {
        program: program(10),
        code_hash: [11; 32],
        wasm: vec![1, 2, 3],
        abi_version: ABI_VERSION,
        upgrade_policy: UpgradePolicy::default(),
    });
    assert!(matches!(result, Err(LifecycleRefusal::Validation(_))));
    assert_eq!(lifecycle.diagnostics().len(), 1);
}

#[test]
fn unknown_program_and_incompatible_abi_are_typed() {
    let mut lifecycle = Lifecycle::declared()
        .unwrap_or_else(|error| panic!("lifecycle construction refused: {error}"));
    let unknown = lifecycle.upgrade(Upgrade {
        program: program(12),
        authority: [13; 32],
        code_hash: [14; 32],
        wasm: migration_module(),
        abi_version: ABI_VERSION,
        migration: None,
    });
    assert_eq!(unknown, Err(LifecycleRefusal::UnknownProgram));
    let incompatible = lifecycle.deploy(Deploy {
        program: program(15),
        code_hash: [16; 32],
        wasm: add_module(),
        abi_version: ABI_VERSION + 1,
        upgrade_policy: UpgradePolicy::default(),
    });
    assert_eq!(
        incompatible,
        Err(LifecycleRefusal::IncompatibleAbi {
            requested: ABI_VERSION + 1,
            supported: ABI_VERSION,
        })
    );
}
