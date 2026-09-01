use layerx_agentd::prepare::{
    prepare_activity, CorePreparationBoundary, CorePreparationState, CoreStateError,
    PreparationDefaults, PrepareError, PrepareRequest,
};
use layerx_types::activity::{Authority, TimestampBound};
use layerx_types::amount::Amount;
use layerx_types::ids::{Did, IdempotencyKey};
use layerx_types::payload::{ActivityType, ModuleId, ModuleRegistration, ModuleRegistry};
use layerx_wire::activity::decode_unsigned;
use layerx_wire::encode::Encoder;
use layerx_wire::sign::preimage;

struct RecordedCoreBoundary {
    result: Result<CorePreparationState, CoreStateError>,
}

impl CorePreparationBoundary for RecordedCoreBoundary {
    fn preparation_state(&mut self, _actor: &Did) -> Result<CorePreparationState, CoreStateError> {
        self.result.clone()
    }
}

fn activity_type() -> ActivityType {
    ActivityType::new(ModuleId::Asset, 5).unwrap_or_else(|error| panic!("activity type: {error:?}"))
}

fn registry() -> ModuleRegistry {
    ModuleRegistry::new(
        &[ModuleRegistration::new(ModuleId::Asset, &[activity_type()])
            .unwrap_or_else(|error| panic!("registration: {error:?}"))],
    )
    .unwrap_or_else(|error| panic!("registry: {error:?}"))
}

fn state() -> CorePreparationState {
    CorePreparationState {
        network_id: 17,
        account_sequence: 5,
        protocol_timestamp: 1_000,
        observed_head_sequence: 88,
        module_registry: registry(),
    }
}

fn defaults() -> PreparationDefaults {
    PreparationDefaults {
        timestamp_span: 30,
        fee_limit: Amount::from_u128(12),
        maximum_payload_bytes: 1_024,
    }
}

fn request() -> PrepareRequest {
    PrepareRequest {
        actor: Did::new(b"did:layerx:prepare-agent")
            .unwrap_or_else(|error| panic!("DID: {error:?}")),
        authority: Authority::session_key(b"session-authority")
            .unwrap_or_else(|error| panic!("authority: {error:?}")),
        activity_type: activity_type(),
        expected_account_sequence: Some(5),
        timestamp_bound: Some(
            TimestampBound::new(995, 1_010).unwrap_or_else(|error| panic!("timestamp: {error:?}")),
        ),
        fee_limit: Some(Amount::from_u128(7)),
        idempotency_key: IdempotencyKey::new([4; 32]),
        payload: send_payload(),
        declared_payload_limit: 1_024,
    }
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
        .unwrap_or_else(|error| panic!("public key: {error:?}"));
    encoder
        .fixed(&[0x77; 64])
        .unwrap_or_else(|error| panic!("signature: {error:?}"));
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

#[test]
fn canonical_bytes_and_preimage_come_only_from_layerx_wire() {
    let mut boundary = RecordedCoreBoundary {
        result: Ok(state()),
    };
    let prepared = prepare_activity(&mut boundary, defaults(), request())
        .unwrap_or_else(|error| panic!("prepare: {error:?}"));
    let decoded = decode_unsigned(&prepared.canonical_bytes, &registry())
        .unwrap_or_else(|error| panic!("decode: {error:?}"));
    assert_eq!(decoded.network_id(), 17);
    assert_eq!(decoded.actor_did(), b"did:layerx:prepare-agent");
    assert_eq!(decoded.authority(), b"session-authority");
    assert_eq!(decoded.account_sequence(), 5);
    assert_eq!(decoded.timestamp_bound().not_before, 995);
    assert_eq!(decoded.timestamp_bound().not_after, 1_010);
    assert_eq!(decoded.idempotency_key(), [4; 32]);
    assert_eq!(decoded.fee_limit(), 7);
    assert_eq!(decoded.payload(), send_payload());
    assert_eq!(
        prepared.signing_preimage,
        *preimage(&decoded)
            .unwrap_or_else(|error| panic!("preimage: {error:?}"))
            .as_bytes()
    );
}

#[test]
fn stale_sequence_and_unavailable_core_are_refused_without_guessing() {
    let mut stale = request();
    stale.expected_account_sequence = Some(4);
    let mut boundary = RecordedCoreBoundary {
        result: Ok(state()),
    };
    assert_eq!(
        prepare_activity(&mut boundary, defaults(), stale),
        Err(PrepareError::StaleSequence {
            expected: 4,
            core: 5,
        })
    );
    let mut unavailable = RecordedCoreBoundary {
        result: Err(CoreStateError::Unavailable),
    };
    assert!(matches!(
        prepare_activity(&mut unavailable, defaults(), request()),
        Err(PrepareError::Core(CoreStateError::Unavailable))
    ));
}

#[test]
fn explicit_bounds_are_never_widened_and_payload_limits_are_exact() {
    let mut widened = request();
    widened.timestamp_bound = Some(
        TimestampBound::new(980, 1_020).unwrap_or_else(|error| panic!("timestamp: {error:?}")),
    );
    let mut boundary = RecordedCoreBoundary {
        result: Ok(state()),
    };
    assert_eq!(
        prepare_activity(&mut boundary, defaults(), widened),
        Err(PrepareError::TimestampBoundWidened)
    );

    let mut oversized = request();
    let actual = oversized.payload.len();
    oversized.declared_payload_limit = actual.saturating_sub(1);
    let mut boundary = RecordedCoreBoundary {
        result: Ok(state()),
    };
    assert_eq!(
        prepare_activity(&mut boundary, defaults(), oversized),
        Err(PrepareError::PayloadLimitExceeded {
            actual,
            maximum: actual - 1,
        })
    );
}
