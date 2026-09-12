use std::fmt::Debug;

use layerx_crypto::disclosure::{bind, Disclosure};
use layerx_intents::{compile, DisclosureCheck, Intent, IntentKind, NativeCustodyCredit};
use layerx_types::account::AccountId;
use layerx_types::activity::{Authority, EnvelopeBuilder, TimestampBound};
use layerx_types::amount::Amount;
use layerx_types::ids::{Did, IdempotencyKey};
use layerx_types::payload::{ActivityType, ModuleId, ModuleRegistration, ModuleRegistry};
use layerx_wire::activity::encode_unsigned_envelope;
use sha2::{Digest as _, Sha256};

const CREDIT: &[u8; 427] =
    include_bytes!("../../../../tests/fixtures/custody/paxeer-state-v2/custody.credit");

fn checked<T, E: Debug>(value: Result<T, E>) -> T {
    value.unwrap_or_else(|error| panic!("{error:?}"))
}

fn recipient() -> AccountId {
    let key: String = CREDIT[139..171]
        .iter()
        .flat_map(|byte| {
            let digits = b"0123456789abcdef";
            [
                char::from(digits[usize::from(byte >> 4)]),
                char::from(digits[usize::from(byte & 15)]),
            ]
        })
        .collect();
    checked(AccountId::parse(&format!("agent:did:layerx:{key}:main")))
}

fn credit(payload: [u8; 427]) -> NativeCustodyCredit {
    checked(NativeCustodyCredit::new(
        payload,
        checked(AccountId::parse("system:paxeer-reserve")),
        recipient(),
    ))
}

fn registry() -> ModuleRegistry {
    let activity = checked(ActivityType::new(ModuleId::Bridge, 1));
    checked(ModuleRegistry::new(&[checked(ModuleRegistration::new(
        ModuleId::Bridge,
        &[activity],
    ))]))
}

fn disclosure(value: &NativeCustodyCredit) -> Disclosure {
    let registry = registry();
    let compiled = checked(compile(
        &Intent::v1(IntentKind::NativeCustodyCredit(value.clone())),
        &registry,
    ));
    let name = recipient();
    let did = checked(Did::new(
        name.canonical()
            .strip_prefix("agent:")
            .and_then(|text| text.strip_suffix(":main"))
            .unwrap_or_else(|| panic!("account"))
            .as_bytes(),
    ));
    let mut envelope = EnvelopeBuilder::new();
    checked(envelope.protocol_version(3));
    checked(envelope.network_id(u32::from_be_bytes(checked(CREDIT[37..41].try_into()))));
    checked(envelope.activity_type(compiled.activity_type()));
    checked(envelope.actor_did(did));
    checked(envelope.authority(checked(Authority::owner(&CREDIT[139..171]))));
    checked(envelope.account_sequence(1));
    checked(envelope.timestamp_bound(checked(TimestampBound::new(1, 100))));
    checked(envelope.idempotency_key(IdempotencyKey::new(value.nullifier())));
    checked(envelope.fee_limit(Amount::from_u128(0)));
    checked(envelope.payload_hash(compiled.payload_hash()));
    checked(envelope.payload(compiled.payload().clone()));
    let bytes = checked(encode_unsigned_envelope(&checked(envelope.build())));
    checked(bind(&bytes, &registry))
}

#[test]
fn native_credit_preserves_real_attestation_and_all_disclosed_bytes() {
    let value = credit(*CREDIT);
    let intent = Intent::v1(IntentKind::NativeCustodyCredit(value.clone()));
    let compiled = checked(compile(&intent, &registry()));
    assert_eq!(
        compiled.activity_type(),
        checked(ActivityType::new(ModuleId::Bridge, 1))
    );
    assert_eq!(compiled.payload().as_bytes(), CREDIT);
    let check = checked(DisclosureCheck::verify(&intent, &compiled));
    assert_eq!(check.canonical_payload(), CREDIT);
    let agent = disclosure(&value);
    checked(DisclosureCheck::verify_agent(&intent, &agent));
    let mut digest = Sha256::new();
    digest.update(b"LX:DEPOSIT:NULLIFIER:v1");
    digest.update(&CREDIT[43..75]);
    let expected: [u8; 32] = digest.finalize().into();
    assert_eq!(value.nullifier(), expected);
    for index in 0..CREDIT.len() {
        let mut changed = *CREDIT;
        changed[index] ^= 1;
        if let Ok(changed) =
            NativeCustodyCredit::new(changed, value.reserve().clone(), value.recipient().clone())
        {
            let changed = Intent::v1(IntentKind::NativeCustodyCredit(changed));
            assert!(
                DisclosureCheck::verify(&changed, &compiled).is_err(),
                "byte {index}"
            );
            assert!(
                DisclosureCheck::verify_agent(&changed, &agent).is_err(),
                "byte {index}"
            );
        }
    }
    assert!(compile(
        &Intent::v2(IntentKind::NativeCustodyCredit(value)),
        &registry()
    )
    .is_err());
}

#[test]
fn native_credit_refuses_wrong_accounts_and_semantic_substitution() {
    let reserve = checked(AccountId::parse("system:paxeer-reserve"));
    let wrong = checked(AccountId::parse("agent:did:layerx:another:main"));
    assert!(NativeCustodyCredit::new(*CREDIT, reserve, wrong).is_err());
    let fees = checked(AccountId::parse("system:fees"));
    assert!(NativeCustodyCredit::new(*CREDIT, fees, recipient()).is_err());
    let value = credit(*CREDIT);
    let intent = Intent::v1(IntentKind::NativeCustodyCredit(value.clone()));
    let agent = disclosure(&value);
    let mut changed = agent.clone();
    changed.amounts[0].value += 1;
    assert!(DisclosureCheck::verify_agent(&intent, &changed).is_err());
    let mut changed = agent.clone();
    changed.asset[0] ^= 1;
    assert!(DisclosureCheck::verify_agent(&intent, &changed).is_err());
    let mut changed = agent;
    changed.idempotency_key[0] ^= 1;
    assert!(DisclosureCheck::verify_agent(&intent, &changed).is_err());
}
