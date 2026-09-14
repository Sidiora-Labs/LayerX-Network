type TestResult<T = ()> = Result<T, String>;

fn test_error<E: std::fmt::Debug>(error: E) -> String {
    format!("{error:?}")
}

use layerx_intents::{
    compile, DisclosureCheck, Intent, IntentKind, NativeBudgetCreate, RecoveryRegistration,
};
use layerx_types::account::AccountId;
use layerx_types::ids::{AssetId, Did};
use layerx_types::intent::{ApprovalThreshold, RecoveryRoot};
use layerx_types::payload::{ActivityType, ModuleId, ModuleRegistration, ModuleRegistry};

fn registry() -> TestResult<ModuleRegistry> {
    Ok(ModuleRegistry::new(&[
        ModuleRegistration::new(
            ModuleId::Governance,
            &[ActivityType::new(ModuleId::Governance, 3).map_err(test_error)?],
        )
        .map_err(test_error)?,
        ModuleRegistration::new(
            ModuleId::Asset,
            &[ActivityType::new(ModuleId::Asset, 4).map_err(test_error)?],
        )
        .map_err(test_error)?,
        ModuleRegistration::new(
            ModuleId::Budget,
            &[ActivityType::new(ModuleId::Budget, 1).map_err(test_error)?],
        )
        .map_err(test_error)?,
    ])
    .map_err(test_error)?)
}

fn budget() -> TestResult<NativeBudgetCreate> {
    Ok(NativeBudgetCreate {
        budget_id: [0x31; 32],
        budget_account: AccountId::parse(&format!(
            "agent:did:key:managed:budget:{}",
            "31".repeat(32)
        ))
        .map_err(test_error)?,
        asset: [0x42; 32],
        purpose: [0x53; 32],
        per_period_limit: 400,
        carry_cap: 0,
        initial_amount: 600,
        period_length_ms: 1000,
        period_start_ms: 2000,
        expiry_ms: 5000,
        revocation_sequence: 0,
        rollover: 1,
        source_account: AccountId::parse(&format!(
            "agent:did:key:managed:asset:{}",
            "42".repeat(32)
        ))
        .map_err(test_error)?,
        source_sequence: 7,
    })
}

#[test]
fn native_budget_binds_exact_account_sequence_and_every_field() -> TestResult {
    let original = budget().map_err(test_error)?;
    let bytes = original.payload().map_err(test_error)?;
    assert_eq!(bytes.len(), 251);
    assert_eq!(&bytes[..2], &[0, 2]);
    assert_eq!(&bytes[243..], &7_u64.to_be_bytes());
    let intent = Intent::v3(IntentKind::NativeBudgetCreate(original.clone()));
    let compiled = compile(&intent, &registry().map_err(test_error)?).map_err(test_error)?;
    assert_eq!(compiled.payload().as_bytes(), bytes);
    DisclosureCheck::verify(&intent, &compiled).map_err(test_error)?;
    original.verify_payload(&bytes).map_err(test_error)?;
    for offset in 0..bytes.len() {
        let mut changed = bytes.clone();
        changed[offset] ^= 1;
        assert!(
            original.verify_payload(&changed).is_err(),
            "changed byte {offset}"
        );
    }
    let legacy = Intent::v1(IntentKind::NativeBudgetCreate(original.clone()));
    assert!(compile(&legacy, &registry().map_err(test_error)?).is_err());
    for invalid in [0, 3, 255] {
        let mut changed = original.clone();
        changed.rollover = invalid;
        assert!(changed.payload().is_err());
    }
    let mut changed = original.clone();
    changed.carry_cap = 1;
    assert!(changed.payload().is_err());
    changed = original.clone();
    changed.source_sequence = u64::MAX;
    assert!(changed.payload().is_err());
    changed = original.clone();
    changed.source_account = AccountId::parse("agent:did:key:other:main").map_err(test_error)?;
    assert!(changed.payload().is_err());
    changed = original.clone();
    changed.asset[0] ^= 1;
    assert!(changed.payload().is_err());
    changed = original;
    changed.budget_id[0] ^= 1;
    assert!(changed.payload().is_err());
    Ok(())
}

#[test]
fn native_recovery_and_asset_open_preserve_distinct_legacy_encoding() -> TestResult {
    let did = Did::new(b"did:key:managed").map_err(test_error)?;
    let recovery = RecoveryRegistration::new(
        did.clone(),
        RecoveryRoot::new([0x65; 32]),
        ApprovalThreshold::new(2).map_err(test_error)?,
    )
    .map_err(test_error)?;
    let native = Intent::v3(IntentKind::RecoveryRegistration(recovery.clone()));
    let compiled = compile(&native, &registry().map_err(test_error)?).map_err(test_error)?;
    assert_eq!(compiled.payload().as_bytes().len(), 70);
    assert_eq!(&compiled.payload().as_bytes()[..4], &[0x71, 3, 0, 3]);
    assert_eq!(
        &compiled.payload().as_bytes()[4..36],
        &layerx_intents::canonical::did_id_for_protocol(&did, 3).map_err(test_error)?
    );
    DisclosureCheck::verify(&native, &compiled).map_err(test_error)?;
    let legacy = Intent::v1(IntentKind::RecoveryRegistration(recovery));
    let old = compile(&legacy, &registry().map_err(test_error)?).map_err(test_error)?;
    assert_ne!(old.payload().as_bytes(), compiled.payload().as_bytes());
    DisclosureCheck::verify(&legacy, &old).map_err(test_error)?;
    let opening = Intent::v3(IntentKind::NativeAssetAccountOpen(AssetId::new([0x42; 32])));
    let compiled = compile(&opening, &registry().map_err(test_error)?).map_err(test_error)?;
    assert_eq!(
        compiled.payload().as_bytes(),
        [&[0, 1][..], &[0x42; 32][..]].concat()
    );
    DisclosureCheck::verify(&opening, &compiled).map_err(test_error)?;
    assert!(compile(
        &Intent::v1(IntentKind::NativeAssetAccountOpen(AssetId::new([0x42; 32]))),
        &registry().map_err(test_error)?
    )
    .is_err());
    Ok(())
}
