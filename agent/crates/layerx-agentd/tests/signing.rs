use std::future::Future;
use std::pin::pin;
use std::sync::Arc;
use std::task::{Context, Poll, Wake, Waker};

use layerx_agentd::prepare::{
    prepare_activity, CorePreparationBoundary, CorePreparationState, CoreStateError,
    PreparationDefaults, PrepareRequest,
};
use layerx_agentd::sign::{external, self_sign, ProvisionedSessionKey, SigningError, SigningMode};
use layerx_crypto::local::LocalSigner;
use layerx_crypto::session::{issue_session_key, IssuedSessionKey, SessionKeyRequest};
use layerx_crypto::signer::Signer;
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

fn send_type() -> ActivityType {
    ActivityType::new(ModuleId::Asset, 5).unwrap_or_else(|error| panic!("activity: {error:?}"))
}

fn other_type() -> ActivityType {
    ActivityType::new(ModuleId::Asset, 6).unwrap_or_else(|error| panic!("activity: {error:?}"))
}

fn registry() -> ModuleRegistry {
    ModuleRegistry::new(&[ModuleRegistration::new(ModuleId::Asset, &[send_type()])
        .unwrap_or_else(|error| panic!("registration: {error:?}"))])
    .unwrap_or_else(|error| panic!("registry: {error:?}"))
}

fn send_payload() -> Vec<u8> {
    let mut encoder = Encoder::new(512);
    for result in [encoder.u16(0x5301), encoder.u16(10)] {
        result.unwrap_or_else(|error| panic!("header: {error:?}"));
    }
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

fn issued(seed: [u8; 32], permitted: ActivityType, expires_at: u64) -> IssuedSessionKey {
    let public_key = LocalSigner::new(seed).public_key();
    issue_session_key(&SessionKeyRequest {
        grantor: [1; 32],
        session_public_key: public_key,
        not_before: 900,
        expires_at: Some(expires_at),
        permitted_activity_types: vec![permitted],
        revocation_sequence: Some(5),
    })
    .unwrap_or_else(|error| panic!("issue session key: {error:?}"))
}

fn prepared(authority: Authority) -> layerx_agentd::prepare::Prepared {
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
            authority,
            activity_type: send_type(),
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
fn external_signing_is_the_key_free_default() {
    let prepared = prepared(
        Authority::owner(b"external-primary-authority")
            .unwrap_or_else(|error| panic!("authority: {error:?}")),
    );
    let package = external(&prepared).unwrap_or_else(|error| panic!("external: {error:?}"));
    assert_eq!(package.mode, SigningMode::External);
    assert_eq!(package.canonical_bytes, prepared.canonical_bytes);
    assert_eq!(package.signing_preimage, prepared.signing_preimage);
    assert_eq!(package.disclosure_digest, prepared.disclosure_digest);
}

#[test]
fn unprovisioned_scope_exceeding_and_expired_self_signing_are_refused() {
    let seed = [0xa5; 32];
    let valid_issued = issued(seed, send_type(), 1_100);
    let valid_prepared = prepared(valid_issued.authority.clone());
    assert_eq!(
        ready(self_sign(None, &valid_prepared, &registry(), 1_000, 5)),
        Err(SigningError::NotProvisioned)
    );

    let other_issued = issued(seed, other_type(), 1_100);
    let other_prepared = prepared(other_issued.authority.clone());
    let other = ProvisionedSessionKey::new(seed, other_issued)
        .unwrap_or_else(|error| panic!("provision: {error:?}"));
    assert_eq!(
        ready(self_sign(
            Some(&other),
            &other_prepared,
            &registry(),
            1_000,
            5
        )),
        Err(SigningError::ScopeDenied)
    );

    let expired_issued = issued(seed, send_type(), 999);
    let expired_prepared = prepared(expired_issued.authority.clone());
    let expired = ProvisionedSessionKey::new(seed, expired_issued)
        .unwrap_or_else(|error| panic!("provision: {error:?}"));
    assert_eq!(
        ready(self_sign(
            Some(&expired),
            &expired_prepared,
            &registry(),
            1_000,
            5
        )),
        Err(SigningError::Expired)
    );
}

#[test]
fn session_key_self_signing_is_distinct_and_audited() {
    let seed = [0xa5; 32];
    let issued = issued(seed, send_type(), 1_100);
    let prepared = prepared(issued.authority.clone());
    let provisioned = ProvisionedSessionKey::new(seed, issued)
        .unwrap_or_else(|error| panic!("provision: {error:?}"));
    let signed = ready(self_sign(
        Some(&provisioned),
        &prepared,
        &registry(),
        1_000,
        5,
    ))
    .unwrap_or_else(|error| panic!("self sign: {error:?}"));
    assert_eq!(signed.mode, SigningMode::ProtocolSessionKey);
    assert_eq!(signed.audit.mode, SigningMode::ProtocolSessionKey);
    assert_eq!(signed.audit.activity_type, send_type());
    assert_eq!(signed.audit.disclosure_digest, prepared.disclosure_digest);
    assert_eq!(signed.canonical_bytes, prepared.canonical_bytes);
    assert_ne!(signed.signature, [0; 64]);
}
