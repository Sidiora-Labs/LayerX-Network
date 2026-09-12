use layerx_crypto::authority_grant::AuthorityGrant;
use layerx_intents::{compile, inspect_intent, DisclosureCheck, Intent, IntentKind, IntentKindTag};
use layerx_types::payload::{ActivityType, ModuleId, ModuleRegistration, ModuleRegistry};

#[test]
fn native_capability_and_budget_grants_compile_and_disclose_exactly() {
    let activity = ActivityType::new(ModuleId::Governance, 8)
        .unwrap_or_else(|error| panic!("activity: {error:?}"));
    let module = ModuleRegistration::new(ModuleId::Governance, &[activity])
        .unwrap_or_else(|error| panic!("module: {error:?}"));
    let registry =
        ModuleRegistry::new(&[module]).unwrap_or_else(|error| panic!("registry: {error:?}"));
    for body in [
        include_bytes!(
            "../../../../agent/crates/layerx-crypto/tests/fixtures/authority-grant-capability.bin"
        )
        .as_slice(),
        include_bytes!(
            "../../../../agent/crates/layerx-crypto/tests/fixtures/authority-grant-budget.bin"
        )
        .as_slice(),
    ] {
        let grant = AuthorityGrant::decode(body).unwrap_or_else(|error| panic!("grant: {error:?}"));
        let intent = Intent::v1(IntentKind::AuthorityGrant(grant));
        let compiled =
            compile(&intent, &registry).unwrap_or_else(|error| panic!("compile: {error:?}"));
        assert_eq!(compiled.activity_type(), activity);
        assert_eq!(
            compiled.payload().as_bytes(),
            grant
                .payload()
                .unwrap_or_else(|error| panic!("payload: {error:?}"))
        );
        assert!(DisclosureCheck::verify(&intent, &compiled).is_ok());
        let mut changed = grant;
        changed.scope.maximum_total += 1;
        assert!(DisclosureCheck::verify(
            &Intent::v1(IntentKind::AuthorityGrant(changed)),
            &compiled
        )
        .is_err());
        changed.scope.purpose = [0; 32];
        assert!(compile(&Intent::v1(IntentKind::AuthorityGrant(changed)), &registry).is_err());
    }
    let header = inspect_intent(&[0, 1, 0, 15]).unwrap_or_else(|error| panic!("header: {error:?}"));
    assert_eq!(header.kind, IntentKindTag::AuthorityGrant);
}
