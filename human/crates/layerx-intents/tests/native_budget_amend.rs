use layerx_intents::{compile, DisclosureCheck, Intent, IntentKind, NativeBudgetAmend};
use layerx_types::payload::{ActivityType, ModuleId, ModuleRegistration, ModuleRegistry};

type Result<T = ()> = std::result::Result<T, String>;
fn checked<T, E: std::fmt::Debug>(result: std::result::Result<T, E>) -> Result<T> {
    result.map_err(|error| format!("{error:?}"))
}
fn registry() -> Result<ModuleRegistry> {
    checked(ModuleRegistry::new(&[checked(ModuleRegistration::new(
        ModuleId::Budget,
        &[checked(ActivityType::new(ModuleId::Budget, 3))?],
    ))?]))
}
fn amendment() -> NativeBudgetAmend {
    NativeBudgetAmend {
        budget_id: [9; 32],
        per_period_limit: 80,
        carry_cap: 0,
        expiry_ms: 2_000,
        rollover: 1,
    }
}

#[test]
fn native_amendment_compiles_exact_nonmonetary_payload_and_binds_every_byte() -> Result {
    let original = amendment();
    let bytes = checked(original.payload())?;
    assert_eq!(bytes.len(), 75);
    assert_eq!(&bytes[..2], &[0, 1]);
    assert_eq!(&bytes[2..34], &[9; 32]);
    assert_eq!(&bytes[34..50], &80_u128.to_be_bytes());
    assert_eq!(&bytes[50..66], &0_u128.to_be_bytes());
    assert_eq!(&bytes[66..74], &2_000_u64.to_be_bytes());
    assert_eq!(bytes[74], 1);
    assert_eq!(
        checked(NativeBudgetAmend::decode_payload(&bytes))?,
        original
    );
    let intent = Intent::v3(IntentKind::NativeBudgetAmend(original.clone()));
    let compiled = checked(compile(&intent, &registry()?))?;
    assert_eq!(
        compiled.activity_type(),
        checked(ActivityType::new(ModuleId::Budget, 3))?
    );
    assert_eq!(compiled.payload().as_bytes(), bytes);
    checked(DisclosureCheck::verify(&intent, &compiled))?;
    for offset in 0..bytes.len() {
        let mut changed = bytes.clone();
        changed[offset] ^= 1;
        assert!(original.verify_payload(&changed).is_err(), "byte {offset}");
    }
    for length in 0..75 {
        assert!(NativeBudgetAmend::decode_payload(&bytes[..length]).is_err());
    }
    let mut trailing = bytes.clone();
    trailing.push(0);
    assert!(NativeBudgetAmend::decode_payload(&trailing).is_err());
    for legacy in [
        Intent::v1(IntentKind::NativeBudgetAmend(original.clone())),
        Intent::v2(IntentKind::NativeBudgetAmend(original.clone())),
    ] {
        assert!(compile(&legacy, &registry()?).is_err());
    }
    Ok(())
}

#[test]
fn native_amendment_refuses_invalid_limits_rollover_and_disclosure_changes() -> Result {
    let original = amendment();
    let mutations: &[fn(&mut NativeBudgetAmend)] = &[
        |v| v.budget_id = [0; 32],
        |v| v.per_period_limit = 0,
        |v| v.expiry_ms = 0,
        |v| v.rollover = 0,
        |v| v.rollover = 3,
        |v| v.carry_cap = 1,
    ];
    for mutation in mutations {
        let mut changed = original.clone();
        mutation(&mut changed);
        assert!(changed.payload().is_err());
    }
    let compiled = checked(compile(
        &Intent::v3(IntentKind::NativeBudgetAmend(original.clone())),
        &registry()?,
    ))?;
    let mutations: &[fn(&mut NativeBudgetAmend)] = &[
        |v| v.budget_id[0] ^= 1,
        |v| v.per_period_limit += 1,
        |v| v.expiry_ms += 1,
        |v| {
            v.rollover = 2;
            v.carry_cap = 1;
        },
    ];
    for mutate in mutations {
        let mut changed = original.clone();
        mutate(&mut changed);
        assert!(DisclosureCheck::verify(
            &Intent::v3(IntentKind::NativeBudgetAmend(changed)),
            &compiled
        )
        .is_err());
    }
    Ok(())
}
