use layerx_programs_runtime::test_support::{
    add_module, code_section, export_section, func_body, function_section, module, type_section,
    OP_END, OP_I32_CONST, TYPE_I32,
};
use layerx_programs_runtime::{
    hash_bytes, Deploy, HashAlgorithm, Lifecycle, LifecycleRefusal, Migration, ProgramId, Upgrade,
    UpgradePolicy, ABI_VERSION,
};

fn program(byte: u8) -> ProgramId {
    ProgramId::new([byte; 32]).unwrap_or_else(|error| panic!("program id refused: {error}"))
}

fn code_hash(wasm: &[u8]) -> [u8; 32] {
    hash_bytes(HashAlgorithm::Sha256, wasm)
        .unwrap_or_else(|error| panic!("program code hash refused: {error}"))
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
fn deploy_returns_an_opaque_receipt_without_a_runtime_trust_bypass() {
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
    assert_eq!(receipt.program(), program(1));
    assert_eq!(receipt.version(), 1);
    assert_eq!(receipt.new_code_hash(), expected_code_hash);
    assert_eq!(receipt.old_code_hash(), None);
    assert!(receipt.migration().is_none());
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
    assert_eq!(receipt.version(), 2);
    assert_eq!(receipt.old_code_hash(), Some(deployed_hash));
    assert!(receipt.migration().is_some());
}

#[test]
fn deploy_and_upgrade_record_exact_hashes_and_migration_outcome() {
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
    assert_eq!(deploy_receipt.version(), 1);
    assert_eq!(deploy_receipt.old_code_hash(), None);
    assert!(deploy_receipt.migration().is_none());
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
    assert_eq!(upgrade_receipt.version(), 2);
    assert_eq!(upgrade_receipt.old_code_hash(), Some(deploy_hash));
    assert_eq!(upgrade_receipt.new_code_hash(), upgrade_hash);
    assert!(upgrade_receipt.migration().is_some());
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
    assert_eq!(receipt.version(), 1);
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
    assert_eq!(receipt.version(), 2);
    assert_eq!(receipt.old_code_hash(), Some(deployed_hash));
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
        Err(LifecycleRefusal::AbiVersion(
            layerx_programs_runtime::AbiVersionRefusal::Unsupported {
                requested: ABI_VERSION + 1,
            },
        ))
    );
}

#[test]
fn abi_upgrades_are_monotonic_and_historical_versions_remain_admitted() {
    use layerx_programs_runtime::{
        admit_abi_upgrade, admit_abi_version, AbiVersionRefusal, ABI_V1_VERSION,
        ABI_V2_VERSION,
    };

    assert_eq!(admit_abi_version(ABI_V1_VERSION), Ok(()));
    assert_eq!(admit_abi_version(ABI_V2_VERSION), Ok(()));
    assert_eq!(admit_abi_upgrade(ABI_V1_VERSION, ABI_V2_VERSION), Ok(()));
    assert_eq!(admit_abi_upgrade(ABI_V2_VERSION, ABI_V2_VERSION), Ok(()));
    assert_eq!(
        admit_abi_upgrade(ABI_V2_VERSION, ABI_V1_VERSION),
        Err(AbiVersionRefusal::Downgrade {
            current: ABI_V2_VERSION,
            requested: ABI_V1_VERSION,
        })
    );
}
