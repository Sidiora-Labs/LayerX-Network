mod support;

use layerx_programs::{
    verify_interface_read, InterfaceCapability, InterfaceEntryPoint, InterfaceRefusal,
    ProgramInterface, TypedFailure, ValueSchema, ValueType,
};
use layerx_programs_runtime::{UpgradePolicy, ABI_V1_VERSION};

use support::{
    deploy_fixture, legacy_deploy_fixture, upgrade_fixture, verifier_for_fixture, AUTHORITY, NOW, WASM_V1, WASM_V2,
};

const CALLABLE_MODULE: &[u8] = &[
    0,97,115,109,1,0,0,0,1,12,2,96,2,127,127,1,127,96,1,127,1,127,
    3,3,2,0,1,5,3,1,0,1,7,34,3,4,b'c',b'a',b'l',b'l',0,0,
    14,b'l',b'a',b'y',b'e',b'r',b'x',b'_',b'r',b'e',b's',b'e',b'r',b'v',b'e',0,1,
    6,b'm',b'e',b'm',b'o',b'r',b'y',2,0,10,11,2,4,0,65,0,11,4,0,65,0,11,
];

fn entry(max_payload: u32) -> InterfaceEntryPoint {
    InterfaceEntryPoint {
        name: "call".to_owned(),
        discriminator: [0x10, 0x20, 0x30, 0x40],
        calldata: ValueSchema::layerx(ValueType::Bytes { max_len: max_payload }),
        response: ValueSchema::layerx(ValueType::Bytes { max_len: 64 }),
        capabilities: vec![InterfaceCapability::StorageRead, InterfaceCapability::StorageWrite],
        event_topics: vec![[0x44; 32]],
        failures: vec![TypedFailure {
            code: 7,
            name: "denied".to_owned(),
            detail: ValueSchema::layerx(ValueType::Bytes { max_len: 64 }),
        }],
    }
}

fn interface(entry: InterfaceEntryPoint) -> ProgramInterface {
    ProgramInterface::bind(CALLABLE_MODULE, ABI_V1_VERSION, vec![entry])
        .unwrap_or_else(|error| panic!("bind interface fixture: {error}"))
}

#[test]
fn verified_deployment_binding_and_current_head_use_the_same_receipt_state() {
    let fixture = deploy_fixture(WASM_V1, UpgradePolicy::Authority(AUTHORITY), 70, 1_700_000_070);
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
    let fixture = legacy_deploy_fixture(WASM_V1, UpgradePolicy::Authority(AUTHORITY), 72, 1_700_000_072);
    let verifier = verifier_for_fixture(&fixture, 72, 100, None, 1_000);
    let deployment = verifier.verify_deployment(&fixture.proof, NOW)
        .unwrap_or_else(|error| panic!("legacy deployment evidence: {error}"));
    assert!(!deployment.interface_present());
    assert_eq!(
        ProgramInterface::bind_deployment(&deployment, support::fixture_interface(WASM_V1).entries().to_vec()),
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
    added_capability.capabilities.push(InterfaceCapability::EmitEvent);
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
    let upgraded = ProgramInterface::bind_verified_upgrade(
        &deployment,
        vec![entry(64)],
        &prior,
        false,
    )
    .unwrap_or_else(|error| panic!("bind verified upgrade: {error}"));
    assert_eq!(upgraded.code_hash(), deployment.code_hash());
}
