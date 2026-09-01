use layerx_agentd::prepare::{
    prepare_activity, verify_disclosure_binding, CorePreparationBoundary, CorePreparationState,
    CoreStateError, DisclosureBindingError, PreparationDefaults, PrepareRequest,
};
use layerx_crypto::disclosure::{AmountRole, CounterpartyRole};
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
            actor: Did::new(b"did:layerx:prepare-agent")
                .unwrap_or_else(|error| panic!("DID: {error:?}")),
            authority: Authority::session_key(b"session-authority")
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

#[test]
fn preparation_discloses_semantics_decoded_from_the_bytes() {
    let prepared = prepared();
    assert_eq!(prepared.disclosure.activity_type, activity_type());
    assert_eq!(prepared.disclosure.actor, b"did:layerx:prepare-agent");
    assert_eq!(prepared.disclosure.authority, b"session-authority");
    assert_eq!(prepared.disclosure.counterparties.len(), 2);
    assert_eq!(
        prepared.disclosure.counterparties[0].role,
        CounterpartyRole::Payer
    );
    assert_eq!(
        prepared.disclosure.counterparties[1].role,
        CounterpartyRole::Recipient
    );
    assert_eq!(prepared.disclosure.amounts[0].role, AmountRole::Transfer);
    assert_eq!(prepared.disclosure.amounts[0].value, 25);
    assert_eq!(prepared.disclosure.asset, [0x33; 32]);
    assert_eq!(prepared.disclosure.fee_limit, 7);
    assert_eq!(prepared.disclosure.expiry.not_after, 1_010);
    assert_eq!(prepared.disclosure.idempotency_key, [4; 32]);
    assert_eq!(
        prepared.disclosure.reencode(),
        Ok(prepared.canonical_bytes.clone())
    );
    assert_eq!(prepared.audit.disclosure_digest, prepared.disclosure_digest);
    assert_eq!(verify_disclosure_binding(&prepared), Ok(()));
}

#[test]
fn mutation_after_disclosure_generation_is_refused() {
    let mut prepared = prepared();
    let last = prepared.canonical_bytes.len().saturating_sub(1);
    prepared.canonical_bytes[last] ^= 1;
    assert_eq!(
        verify_disclosure_binding(&prepared),
        Err(DisclosureBindingError::CanonicalMismatch)
    );
}
