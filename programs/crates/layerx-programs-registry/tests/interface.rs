mod support;

use layerx_programs::{
    verify_interface_read, InterfaceCapability, InterfaceEntryPoint, InterfaceRefusal,
    ProgramInterface, TypedFailure, ValueSchema, ValueType,
};
use layerx_programs_runtime::test_support::{
    code_section, func_body, function_section, import_section, module, raw_section, type_section,
    unsigned_leb, OP_CALL, OP_DROP, OP_END, OP_I32_CONST, TYPE_I32,
};
use layerx_programs_runtime::ABI_MODULE;
use layerx_programs_runtime::{UpgradePolicy, ABI_V1_VERSION};

use support::{
    deploy_fixture, legacy_deploy_fixture, upgrade_fixture, verifier_for_fixture, AUTHORITY, NOW,
    WASM_V1, WASM_V2,
};

fn entry(max_payload: u32) -> InterfaceEntryPoint {
    InterfaceEntryPoint {
        name: "call".to_owned(),
        discriminator: [0x10, 0x20, 0x30, 0x40],
        calldata: ValueSchema::layerx(ValueType::Bytes {
            max_len: max_payload,
        }),
        response: ValueSchema::layerx(ValueType::Bytes { max_len: 64 }),
        capabilities: vec![
            InterfaceCapability::StorageRead,
            InterfaceCapability::StorageWrite,
        ],
        event_topics: vec![[0x44; 32]],
        failures: vec![TypedFailure {
            code: 7,
            name: "denied".to_owned(),
            detail: ValueSchema::layerx(ValueType::Bytes { max_len: 64 }),
        }],
    }
}

fn interface(entry: InterfaceEntryPoint) -> ProgramInterface {
    let module = callable_module(&entry.capabilities);
    ProgramInterface::bind(&module, ABI_V1_VERSION, vec![entry])
        .unwrap_or_else(|error| panic!("bind interface fixture: {error}"))
}

fn callable_module(capabilities: &[InterfaceCapability]) -> Vec<u8> {
    let mut imports = Vec::new();
    for capability in capabilities {
        let name = match capability {
            InterfaceCapability::StorageRead => "storage_read",
            InterfaceCapability::StorageWrite => "storage_write",
            InterfaceCapability::EmitEvent => "event_emit",
            _ => panic!("interface fixture does not implement this host capability"),
        };
        imports.push((ABI_MODULE, name, 0));
    }
    let mut exports = unsigned_leb(3);
    let reserve_index =
        u32::try_from(imports.len()).unwrap_or_else(|_| panic!("interface fixture import count"));
    let call_index = reserve_index + 1;
    for (name, kind, index) in [
        ("layerx_reserve", 0_u8, reserve_index),
        ("call", 0_u8, call_index),
        ("memory", 2_u8, 0_u32),
    ] {
        exports.extend(unsigned_leb(name.len() as u64));
        exports.extend_from_slice(name.as_bytes());
        exports.push(kind);
        exports.extend(unsigned_leb(u64::from(index)));
    }
    let mut call = Vec::new();
    for import in 0..imports.len() {
        for _ in 0..4 {
            call.extend([OP_I32_CONST, 0]);
        }
        call.push(OP_CALL);
        call.extend(unsigned_leb(import as u64));
        call.push(OP_DROP);
    }
    call.extend([OP_I32_CONST, 0, OP_END]);
    module(&[
        type_section(&[
            (&[TYPE_I32; 4], &[TYPE_I32]),
            (&[TYPE_I32], &[TYPE_I32]),
            (&[TYPE_I32, TYPE_I32], &[TYPE_I32]),
        ]),
        import_section(&imports),
        function_section(&[1, 2]),
        raw_section(5, &[1, 1, 1, 1]),
        raw_section(7, &exports),
        code_section(&[
            func_body(&[], &[OP_I32_CONST, 0, OP_END]),
            func_body(&[], &call),
        ]),
    ])
}

#[test]
fn verified_deployment_binding_and_current_head_use_the_same_receipt_state() {
    let fixture = deploy_fixture(
        WASM_V1,
        UpgradePolicy::Authority(AUTHORITY),
        70,
        1_700_000_070,
    );
    let verifier = verifier_for_fixture(&fixture, 70, 100, None, 1_000);
    let deployment = verifier
        .verify_deployment(&fixture.proof, NOW)
        .unwrap_or_else(|error| panic!("verified deployment fixture: {error}"));

    let published = support::fixture_interface(WASM_V1);
    let bound = ProgramInterface::bind_deployment(&deployment, published.entries().to_vec())
        .unwrap_or_else(|error| panic!("bind receipted deployment: {error}"));
    assert_eq!(bound.code_hash(), deployment.code_hash());
    let head = verifier
        .verify_current_program(&fixture.proof.state, support::program(), NOW)
        .unwrap_or_else(|error| panic!("verified current head: {error}"));
    let read = verify_interface_read(&head, &fixture.interface_witness)
        .unwrap_or_else(|error| panic!("verified interface read: {error}"));
    assert_eq!(read.interface, bound);
    assert_eq!(read.receipt_digest, head.receipt_digest());
    assert_eq!(read.state_root, head.state_root());
}

#[test]
fn legacy_evidence_cannot_manufacture_a_published_interface() {
    let fixture = legacy_deploy_fixture(
        WASM_V1,
        UpgradePolicy::Authority(AUTHORITY),
        72,
        1_700_000_072,
    );
    let verifier = verifier_for_fixture(&fixture, 72, 100, None, 1_000);
    let deployment = verifier
        .verify_deployment(&fixture.proof, NOW)
        .unwrap_or_else(|error| panic!("legacy deployment evidence: {error}"));
    assert!(!deployment.interface_present());
    assert_eq!(
        ProgramInterface::bind_deployment(
            &deployment,
            support::fixture_interface(WASM_V1).entries().to_vec()
        ),
        Err(InterfaceRefusal::InterfaceAbsent),
    );
}

#[test]
fn schema_capability_event_and_failure_changes_have_directional_compatibility() {
    let prior = interface(entry(64));

    let wider_schema = interface(entry(128));
    assert!(wider_schema.is_widening_of(&prior));

    let narrower_schema = interface(entry(32));
    assert_eq!(
        narrower_schema.authorize_upgrade(&prior, false),
        Err(InterfaceRefusal::NarrowingUpgrade),
    );

    let mut fewer_capabilities = entry(64);
    fewer_capabilities.capabilities.pop();
    assert!(interface(fewer_capabilities).is_widening_of(&prior));

    let mut added_capability = entry(64);
    added_capability
        .capabilities
        .push(InterfaceCapability::EmitEvent);
    assert_eq!(
        interface(added_capability).authorize_upgrade(&prior, false),
        Err(InterfaceRefusal::NarrowingUpgrade),
    );

    let mut added_event = entry(64);
    added_event.event_topics.push([0x55; 32]);
    assert!(interface(added_event).is_widening_of(&prior));

    let mut removed_event = entry(64);
    removed_event.event_topics.clear();
    assert_eq!(
        interface(removed_event).authorize_upgrade(&prior, false),
        Err(InterfaceRefusal::NarrowingUpgrade),
    );

    let mut added_failure = entry(64);
    added_failure.failures.push(TypedFailure {
        code: 8,
        name: "busy".to_owned(),
        detail: ValueSchema::layerx(ValueType::U8),
    });
    assert!(interface(added_failure).is_widening_of(&prior));

    let mut removed_failure = entry(64);
    removed_failure.failures.clear();
    assert_eq!(
        interface(removed_failure).authorize_upgrade(&prior, false),
        Err(InterfaceRefusal::NarrowingUpgrade),
    );
}

#[test]
fn explicit_breaking_upgrade_is_distinct_from_compatible_upgrade() {
    let prior = interface(entry(64));
    let narrower = interface(entry(32));
    assert_eq!(narrower.authorize_upgrade(&prior, true), Ok(()));

    let fixture = upgrade_fixture(WASM_V1, WASM_V2, 71, 1_700_000_071);
    let verifier = verifier_for_fixture(&fixture, 71, 100, None, 1_000);
    let deployment = verifier
        .verify_deployment(&fixture.proof, NOW)
        .unwrap_or_else(|error| panic!("verified upgrade fixture: {error}"));
    let prior_published = support::fixture_interface(WASM_V1);
    let published = support::fixture_interface(WASM_V2);
    let upgraded = ProgramInterface::bind_verified_upgrade(
        &deployment,
        published.entries().to_vec(),
        &prior_published,
        false,
    )
    .unwrap_or_else(|error| panic!("bind verified upgrade: {error}"));
    assert_eq!(upgraded.code_hash(), deployment.code_hash());
}
