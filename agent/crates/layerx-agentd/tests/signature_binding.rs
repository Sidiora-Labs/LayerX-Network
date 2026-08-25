use std::future::Future;
use std::pin::pin;
use std::sync::Arc;
use std::task::{Context, Poll, Wake, Waker};

use layerx_agentd::prepare::{
    prepare_activity, CorePreparationBoundary, CorePreparationState, CoreStateError,
    PreparationDefaults, PrepareRequest, Prepared,
};
use layerx_agentd::sign::{attach_external_signature, verify_before_submit, SigningError};
use layerx_crypto::local::LocalSigner;
use layerx_crypto::signer::{sign_disclosed, Signer};
use layerx_types::activity::{Authority, TimestampBound};
use layerx_types::amount::Amount;
use layerx_types::ids::{Did, IdempotencyKey};
use layerx_types::payload::{ActivityType, ModuleId, ModuleRegistration, ModuleRegistry};
use layerx_wire::encode::Encoder;

struct NoopWake;

impl Wake for NoopWake {
    fn wake(self: Arc<Self>) {}
}

fn ready<F: Future>(future: F) -> F::Output {
    let mut future = pin!(future);
    let waker = Waker::from(Arc::new(NoopWake));
    let mut context = Context::from_waker(&waker);
    match future.as_mut().poll(&mut context) {
        Poll::Ready(value) => value,
        Poll::Pending => panic!("local signing unexpectedly blocked"),
    }
}

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

fn send_payload(amount: u128, recipient: [u8; 32]) -> Vec<u8> {
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
        .fixed(&recipient)
        .unwrap_or_else(|error| panic!("to: {error:?}"));
    encoder
        .fixed(&[0x33; 32])
        .unwrap_or_else(|error| panic!("asset: {error:?}"));
    encoder
        .u128(amount)
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
        .u16(1)
        .unwrap_or_else(|error| panic!("version: {error:?}"));
    encoder.finish()
}

fn prepared(amount: u128, recipient: [u8; 32], fee: u128) -> Prepared {
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
            actor: Did::new(b"did:layerx:signature-binding")
                .unwrap_or_else(|error| panic!("DID: {error:?}")),
            authority: Authority::owner(b"external-authority")
                .unwrap_or_else(|error| panic!("authority: {error:?}")),
            activity_type: activity_type(),
            expected_account_sequence: Some(5),
            timestamp_bound: Some(
                TimestampBound::new(995, 1_010)
                    .unwrap_or_else(|error| panic!("timestamp: {error:?}")),
            ),
            fee_limit: Some(Amount::from_u128(fee)),
            idempotency_key: IdempotencyKey::new([4; 32]),
            payload: send_payload(amount, recipient),
            declared_payload_limit: 1_024,
        },
    )
    .unwrap_or_else(|error| panic!("prepare: {error:?}"))
}

#[test]
fn verified_wrapper_preserves_the_exact_signed_input_and_activity_id() {
    let prepared = prepared(25, [0x22; 32], 7);
    let signer = LocalSigner::new([0xa5; 32]);
    let signature = ready(sign_disclosed(
        &signer,
        &prepared.canonical_bytes,
        &prepared.disclosure,
        &registry(),
    ))
    .unwrap_or_else(|error| panic!("sign: {error:?}"));
    let signed_bytes = attach_external_signature(&prepared, *signature.as_bytes())
        .unwrap_or_else(|error| panic!("attach: {error:?}"));
    let verified =
        verify_before_submit(&signed_bytes, &prepared, &signer.public_key(), &registry())
            .unwrap_or_else(|error| panic!("verify: {error:?}"));
    assert_eq!(verified.exact_bytes(), signed_bytes);
    assert_eq!(verified.signed_byte_length(), signed_bytes.len());
    assert_ne!(verified.activity_id(), [0; 32]);
}

#[test]
fn amount_recipient_and_fee_alterations_cannot_ride_an_old_signature() {
    let original = prepared(25, [0x22; 32], 7);
    let signer = LocalSigner::new([0xa5; 32]);
    let signature = ready(sign_disclosed(
        &signer,
        &original.canonical_bytes,
        &original.disclosure,
        &registry(),
    ))
    .unwrap_or_else(|error| panic!("sign: {error:?}"));

    for altered in [
        prepared(26, [0x22; 32], 7),
        prepared(25, [0x23; 32], 7),
        prepared(25, [0x22; 32], 8),
    ] {
        let signed_bytes = attach_external_signature(&altered, *signature.as_bytes())
            .unwrap_or_else(|error| panic!("attach: {error:?}"));
        assert_eq!(
            verify_before_submit(&signed_bytes, &altered, &signer.public_key(), &registry()),
            Err(SigningError::SignatureInvalid)
        );
    }
}
