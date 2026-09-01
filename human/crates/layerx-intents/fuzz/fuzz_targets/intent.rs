#![no_main]

use layerx_intents::{compile, inspect_intent, Intent, IntentKind, LxpSend, RejectReason};
use layerx_types::account::AccountId;
use layerx_types::amount::Amount;
use layerx_types::ids::{AssetId, IdempotencyKey};
use layerx_types::intent::{
    AuthorizationSignature, ContextHash, NetworkId, ProtocolVersion, PublicKey, SendAuthorization,
    SendAuthorizationKind, Sequence, TimestampSeconds,
};
use layerx_types::payload::{ActivityType, ModuleId, ModuleRegistration, ModuleRegistry};
use libfuzzer_sys::fuzz_target;

fn registry() -> Option<ModuleRegistry> {
    let activity = ActivityType::new(ModuleId::Asset, 5).ok()?;
    let registration = ModuleRegistration::new(ModuleId::Asset, &[activity]).ok()?;
    ModuleRegistry::new(&[registration]).ok()
}

fuzz_target!(|data: &[u8]| {
    if let Err(rejected) = inspect_intent(data) {
        assert_eq!(rejected.input(), data);
        assert!(matches!(
            rejected.reason(),
            RejectReason::Malformed(_)
                | RejectReason::UnknownVersion(_)
                | RejectReason::UnknownKind(_)
        ));
    }

    let Some(registry) = registry() else {
        return;
    };
    let selector = data.first().copied().unwrap_or(0);
    let amount = u128::from(selector).saturating_add(1);
    let mut idempotency = [0_u8; 32];
    let copied = data.len().min(idempotency.len());
    idempotency[..copied].copy_from_slice(&data[..copied]);
    let Ok(from) = AccountId::parse("agent:did:layerx:fuzz:main") else {
        return;
    };
    let Ok(to) = AccountId::parse("agent:did:layerx:fuzz-recipient:main") else {
        return;
    };
    let Ok(network) = NetworkId::new(1) else {
        return;
    };
    let Ok(protocol) = ProtocolVersion::new(layerx_wire::limits::PROTOCOL_VERSION) else {
        return;
    };
    let Ok(send) = LxpSend::new(
        from,
        to,
        AssetId::new([2; 32]),
        Amount::from_u128(amount),
        Sequence::from_u64(u64::from(selector)),
        IdempotencyKey::new(idempotency),
        TimestampSeconds::from_u64(1),
        ContextHash::new([3; 32]),
        SendAuthorization::new(
            SendAuthorizationKind::Owner,
            PublicKey::new([4; 32]),
            AuthorizationSignature::new([5; 64]),
        ),
        network,
        protocol,
    ) else {
        return;
    };
    let _ = compile(&Intent::v1(IntentKind::LxpSend(send)), &registry);
});
