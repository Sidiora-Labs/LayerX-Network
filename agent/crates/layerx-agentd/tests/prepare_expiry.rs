use layerx_agentd::budget::{
    reserve, BudgetLimiter, LimitConfig, LimitId, LimitScope, ReservationRequest,
};
use layerx_agentd::prepare::{
    expire, prepare_activity, retention_sweep, CorePreparationBoundary, CorePreparationState,
    CoreStateError, LifecycleError, LifecycleState, PayloadRedaction, PreparationDefaults,
    PreparationLifecycle, PrepareRequest,
};
use layerx_types::activity::{Authority, TimestampBound};
use layerx_types::amount::Amount;
use layerx_types::ids::{Did, IdempotencyKey};
use layerx_types::payload::{ActivityType, ModuleId, ModuleRegistration, ModuleRegistry};
use layerx_wire::encode::Encoder;

struct RecordedCore(CorePreparationState);

impl CorePreparationBoundary for RecordedCore {
    fn preparation_state(&mut self, _actor: &Did) -> Result<CorePreparationState, CoreStateError> {
        Ok(self.0.clone())
    }
}

fn activity_type() -> ActivityType {
    ActivityType::new(ModuleId::Asset, 5).unwrap_or_else(|error| panic!("activity: {error:?}"))
}

fn registry() -> ModuleRegistry {
    ModuleRegistry::new(
        &[ModuleRegistration::new(ModuleId::Asset, &[activity_type()])
            .unwrap_or_else(|error| panic!("registration: {error:?}"))],
    )
    .unwrap_or_else(|error| panic!("registry: {error:?}"))
}

fn send_payload() -> Vec<u8> {
    let mut encoder = Encoder::new(512);
    encoder
        .u16(0x5301)
        .unwrap_or_else(|error| panic!("tag: {error:?}"));
    encoder
        .u16(10)
        .unwrap_or_else(|error| panic!("fields: {error:?}"));
    encoder
        .fixed(&[0x11; 32])
        .unwrap_or_else(|error| panic!("from: {error:?}"));
    encoder
        .fixed(&[0x22; 32])
        .unwrap_or_else(|error| panic!("to: {error:?}"));
    encoder
        .fixed(&[0x33; 32])
        .unwrap_or_else(|error| panic!("asset: {error:?}"));
    encoder
        .u128(25)
        .unwrap_or_else(|error| panic!("amount: {error:?}"));
    encoder
        .u64(5)
        .unwrap_or_else(|error| panic!("sequence: {error:?}"));
    encoder
        .fixed(&[4; 32])
        .unwrap_or_else(|error| panic!("idempotency: {error:?}"));
    encoder
        .u64(1_010)
        .unwrap_or_else(|error| panic!("expiry: {error:?}"));
    encoder
        .fixed(&[0x55; 32])
        .unwrap_or_else(|error| panic!("context: {error:?}"));
    encoder
        .u8(0)
        .unwrap_or_else(|error| panic!("conditions: {error:?}"));
    encoder
        .u8(1)
        .unwrap_or_else(|error| panic!("authority kind: {error:?}"));
    encoder
        .fixed(&[0x11; 32])
        .unwrap_or_else(|error| panic!("controller: {error:?}"));
    encoder
        .fixed(&[0x66; 32])
        .unwrap_or_else(|error| panic!("payload key: {error:?}"));
    encoder
        .fixed(&[0x77; 64])
        .unwrap_or_else(|error| panic!("payload signature: {error:?}"));
    encoder
        .fixed(&[0x55; 32])
        .unwrap_or_else(|error| panic!("signed context: {error:?}"));
    encoder
        .u32(17)
        .unwrap_or_else(|error| panic!("network: {error:?}"));
    encoder
        .u16(layerx_wire::limits::PROTOCOL_VERSION)
        .unwrap_or_else(|error| panic!("version: {error:?}"));
    encoder.finish()
}

fn prepared() -> layerx_agentd::prepare::Prepared {
    let mut core = RecordedCore(CorePreparationState {
        network_id: 17,
        account_sequence: 5,
        protocol_timestamp: 1_000,
        observed_head_sequence: 88,
        module_registry: registry(),
    });
    prepare_activity(
        &mut core,
        PreparationDefaults {
            timestamp_span: 30,
            fee_limit: Amount::from_u128(12),
            maximum_payload_bytes: 1_024,
        },
        PrepareRequest {
            actor: Did::new(b"did:layerx:expiry").unwrap_or_else(|error| panic!("DID: {error:?}")),
            authority: Authority::owner(b"external-authority")
                .unwrap_or_else(|error| panic!("authority: {error:?}")),
            activity_type: activity_type(),
            expected_account_sequence: Some(5),
            timestamp_bound: Some(
                TimestampBound::new(995, 1_010)
                    .unwrap_or_else(|error| panic!("timestamp: {error:?}")),
            ),
            fee_limit: Some(Amount::from_u128(7)),
            idempotency_key: IdempotencyKey::new([4; 32]),
            payload: send_payload(),
            declared_payload_limit: 1_024,
        },
    )
    .unwrap_or_else(|error| panic!("prepare: {error:?}"))
}

fn limiter() -> BudgetLimiter {
    BudgetLimiter::new(vec![LimitConfig {
        id: LimitId([1; 16]),
        name: "tenant-limit".to_owned(),
        scope: LimitScope::Tenant([1; 32]),
        ceiling: 1_000,
        consumed: 0,
    }])
    .unwrap_or_else(|error| panic!("limiter: {error:?}"))
}

fn reserve_one(limiter: &BudgetLimiter, id: [u8; 32]) {
    reserve(
        limiter,
        &ReservationRequest {
            id,
            amount: 100,
            expiry_sequence: 1_010,
            current_sequence: 1_000,
            applicable_limits: vec![LimitId([1; 16])],
        },
    )
    .unwrap_or_else(|error| panic!("reserve: {error:?}"));
}

#[test]
fn expiry_during_and_after_signing_releases_every_reservation() {
    let prepared = prepared();
    let limiter = limiter();
    reserve_one(&limiter, [1; 32]);
    reserve_one(&limiter, [2; 32]);
    let lifecycle = PreparationLifecycle::default();
    lifecycle
        .register([11; 32], &prepared, vec![[1; 32]])
        .unwrap_or_else(|error| panic!("register: {error:?}"));
    lifecycle
        .transition([11; 32], LifecycleState::Signing, 1_001)
        .unwrap_or_else(|error| panic!("signing: {error:?}"));
    lifecycle
        .register([12; 32], &prepared, vec![[2; 32]])
        .unwrap_or_else(|error| panic!("register: {error:?}"));
    lifecycle
        .transition([12; 32], LifecycleState::Signing, 1_001)
        .unwrap_or_else(|error| panic!("signing: {error:?}"));
    lifecycle
        .retain_signed_bytes([12; 32], vec![9; 64], [0x44; 32])
        .unwrap_or_else(|error| panic!("signed: {error:?}"));

    let report =
        expire(&lifecycle, &limiter, 1_011).unwrap_or_else(|error| panic!("expire: {error:?}"));
    assert_eq!(report.expired_preparations, vec![[11; 32], [12; 32]]);
    assert_eq!(report.released_reservations, vec![[1; 32], [2; 32]]);
    assert_eq!(limiter.held_reservations(), Ok(0));
    assert_eq!(lifecycle.state([11; 32]), Ok(LifecycleState::Expired));
    assert_eq!(
        lifecycle.admit_submission([12; 32], 1_011),
        Err(LifecycleError::PreparationExpired)
    );
}

#[test]
fn retention_discards_terminal_bytes_but_preserves_unknown_and_redacts_payload() {
    let prepared = prepared();
    let lifecycle = PreparationLifecycle::default();
    for id in [21_u8, 22] {
        lifecycle
            .register([id; 32], &prepared, Vec::new())
            .unwrap_or_else(|error| panic!("register: {error:?}"));
        lifecycle
            .transition([id; 32], LifecycleState::Signing, 90)
            .unwrap_or_else(|error| panic!("signing: {error:?}"));
        lifecycle
            .retain_signed_bytes([id; 32], vec![id; 64], [id; 32])
            .unwrap_or_else(|error| panic!("signed: {error:?}"));
        lifecycle
            .transition([id; 32], LifecycleState::Submitted, 91)
            .unwrap_or_else(|error| panic!("submitted: {error:?}"));
    }
    lifecycle
        .transition([21; 32], LifecycleState::Unknown, 92)
        .unwrap_or_else(|error| panic!("unknown: {error:?}"));
    lifecycle
        .transition([22; 32], LifecycleState::Failed, 100)
        .unwrap_or_else(|error| panic!("failed: {error:?}"));

    let report =
        retention_sweep(&lifecycle, 110, 5).unwrap_or_else(|error| panic!("sweep: {error:?}"));
    assert_eq!(report.discarded_terminal_signed_bytes, 1);
    assert_eq!(report.preserved_unresolved_signed_bytes, 1);
    assert_eq!(lifecycle.has_signed_bytes([21; 32]), Ok(true));
    assert_eq!(lifecycle.has_signed_bytes([22; 32]), Ok(false));
    let log = lifecycle
        .redacted_log([21; 32], PayloadRedaction::DigestOnly)
        .unwrap_or_else(|error| panic!("log: {error:?}"));
    assert!(log.contains("activity_id="));
    assert!(log.contains("payload=[redacted]"));
    assert!(log.contains("payload_hash="));
    assert!(!log.contains("did:layerx"));
}
