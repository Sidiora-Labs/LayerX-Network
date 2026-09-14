use super::*;

#[test]
fn genuine_sponsored_activity_binds_owner_target_network_and_every_original_byte() {
    let bytes = include_bytes!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../../tests/fixtures/authority/provider-subject/onboarding.activity"
    ));
    let kind = ActivityType::new(ModuleId::Governance, 1).unwrap_or_else(|_| panic!("Governance1"));
    let registry = ModuleRegistry::new(&[ModuleRegistration::new(ModuleId::Governance, &[kind])
        .unwrap_or_else(|_| panic!("registration"))])
    .unwrap_or_else(|_| panic!("registry"));
    let activity =
        signed(bytes, &registry, 77).unwrap_or_else(|_| panic!("original KMS signature"));
    let owner = std::str::from_utf8(activity.actor_did()).unwrap_or_else(|_| panic!("owner"));
    let registration = layerx_crypto::onboarding::SponsoredRegistration::decode(activity.payload())
        .unwrap_or_else(|_| panic!("signed consent"));
    let target = std::str::from_utf8(registration.consent.target.as_bytes())
        .unwrap_or_else(|_| panic!("target"));
    let (bound, consent) = registration_binding(bytes, &registry, 77, owner, target)
        .unwrap_or_else(|_| panic!("original bindings"));
    assert_eq!(
        activity_id(&bound).unwrap_or_else(|_| panic!("activity")),
        activity_id(&activity).unwrap_or_else(|_| panic!("activity"))
    );
    assert_eq!(consent, registration);
    assert!(registration_binding(bytes, &registry, 78, owner, target).is_err());
    assert!(
        registration_binding(bytes, &registry, 77, "did:layerx:another-owner", target).is_err()
    );
    assert!(registration_binding(bytes, &registry, 77, owner, "did:layerx:another-child").is_err());
    for index in 0..bytes.len() {
        let mut changed = bytes.to_vec();
        changed[index] ^= 1;
        assert!(
            registration_binding(&changed, &registry, 77, owner, target).is_err(),
            "signed byte {index}"
        );
    }
    for end in 0..bytes.len() {
        assert!(
            registration_binding(&bytes[..end], &registry, 77, owner, target).is_err(),
            "truncation {end}"
        );
    }
}

#[test]
fn scoped_queries_require_every_coordinate_and_reject_unknown_or_unbound_artifacts() {
    let base = BTreeMap::from([
        ("tenant".to_owned(), "service".to_owned()),
        ("principal".to_owned(), "transport".to_owned()),
        ("subject_principal".to_owned(), "person".to_owned()),
        ("owner_did".to_owned(), "did:layerx:person".to_owned()),
        (
            "owner_account".to_owned(),
            "agent:did:layerx:person:main".to_owned(),
        ),
        ("asset_id".to_owned(), "01".repeat(32)),
    ]);
    assert!(validate_parameters("subject-context", &base).is_ok());
    for key in base.keys() {
        let mut missing = base.clone();
        missing.remove(key);
        assert!(validate_parameters("subject-context", &missing).is_err());
    }
    let mut extra = base.clone();
    extra.insert("storage_tenant".to_owned(), "another-person".to_owned());
    assert!(validate_parameters("subject-context", &extra).is_err());
    let mut extra = base.clone();
    extra.insert("registration".to_owned(), "00".to_owned());
    assert!(validate_parameters("subject-context", &extra).is_err());
    let mut receipt = base.clone();
    receipt.insert("activity_id".to_owned(), "02".repeat(32));
    assert!(validate_parameters("authorized-batch", &receipt).is_err());
    receipt.insert("signed_activity".to_owned(), "00".to_owned());
    assert!(validate_parameters("authorized-batch", &receipt).is_ok());
    assert!(validate_parameters("unrecognized", &base).is_err());
}
