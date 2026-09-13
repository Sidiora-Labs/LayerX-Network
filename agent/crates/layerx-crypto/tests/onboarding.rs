use std::fmt::Debug;

use ed25519_dalek::{Signer as _, SigningKey};
use layerx_crypto::onboarding::{OnboardingConsent, SponsoredRegistration};
use layerx_crypto::SignatureMessage;
use layerx_types::activity::{Authority, EnvelopeBuilder, Signature, TimestampBound};
use layerx_types::amount::Amount;
use layerx_types::ids::{Did, IdempotencyKey};
use layerx_types::payload::{ActivityType, ModuleId, ModuleRegistration, ModuleRegistry, Payload};
use layerx_wire::activity;
use layerx_wire::hash::{self, Domain};
use sha2::{Digest as _, Sha256};

fn checked<T, E: Debug>(value: Result<T, E>) -> T {
    value.unwrap_or_else(|error| panic!("native consent fixture rejected: {error:?}"))
}

fn registry() -> ModuleRegistry {
    let kind = checked(ActivityType::new(ModuleId::Governance, 1));
    checked(ModuleRegistry::new(&[checked(ModuleRegistration::new(ModuleId::Governance, &[kind]))]))
}

fn consent() -> OnboardingConsent {
    OnboardingConsent {
        sponsor: checked(hash::did_id_for_protocol(&checked(Did::new(b"did:layerx:sponsor")), 3)),
        target: checked(Did::new(b"did:layerx:target")),
        target_public_key: SigningKey::from_bytes(&[0x62; 32]).verifying_key().to_bytes(),
        native_asset: [0x73; 32],
        action_key: [0x84; 32],
        expires_at: 2000,
    }
}

fn signed(payload: &[u8], actor: &Did, key: &SigningKey, network: u32) -> Vec<u8> {
    let kind = checked(ActivityType::new(ModuleId::Governance, 1));
    let mut hash = Sha256::new();
    hash.update(Domain::PayloadHash.tag());
    hash.update(payload);
    let mut builder = EnvelopeBuilder::new();
    checked(builder.protocol_version(3));
    checked(builder.network_id(network));
    checked(builder.activity_type(kind));
    checked(builder.actor_did(actor.clone()));
    checked(builder.authority(checked(Authority::owner(&key.verifying_key().to_bytes()))));
    checked(builder.account_sequence(0));
    checked(builder.timestamp_bound(checked(TimestampBound::new(1000, 2000))));
    checked(builder.idempotency_key(IdempotencyKey::new(consent().action_key)));
    checked(builder.fee_limit(Amount::ZERO));
    checked(builder.payload_hash(hash.finalize().into()));
    checked(builder.payload(checked(Payload::new(&registry(), kind, payload))));
    let envelope = checked(builder.build());
    let unsigned = checked(activity::encode_unsigned_envelope(&envelope));
    let message = checked(SignatureMessage::new(Domain::SignaturePreimage, 3, network, &unsigned));
    let signature = key.sign(&message.digest()).to_bytes();
    checked(activity::encode_signed_envelope(&envelope.attach_signature(checked(Signature::new(&signature)))))
}

fn original() -> (OnboardingConsent, Vec<u8>, SponsoredRegistration) {
    let consent = consent();
    let signed = signed(&checked(consent.payload()), &consent.target, &SigningKey::from_bytes(&[0x62; 32]), 77);
    let registration = checked(SponsoredRegistration::from_signed_consent(&signed));
    (consent, signed, registration)
}

#[test]
fn original_target_consent_binds_the_sponsor_registration() {
    let (consent, bytes, registration) = original();
    assert_eq!(registration.consent, consent);
    assert_eq!(registration.signed_consent(), bytes);
    assert_eq!(SponsoredRegistration::decode(&checked(registration.payload())), Ok(registration.clone()));
    let outer = signed(&checked(registration.payload()), &checked(Did::new(b"did:layerx:sponsor")), &SigningKey::from_bytes(&[0x51; 32]), 77);
    let outer = checked(activity::decode_signed(&outer, &registry()));
    assert_eq!(registration.validate_outer(&outer), Ok(()));
}

#[test]
fn every_original_signed_consent_byte_is_authenticated() {
    let (_, original, _) = original();
    for index in 0..original.len() {
        let mut changed = original.clone();
        changed[index] ^= 1;
        assert!(SponsoredRegistration::from_signed_consent(&changed).is_err(), "accepted changed byte {index}");
    }
}

#[test]
fn correctly_signed_wrong_consent_relations_are_refused() {
    let (consent, _, registration) = original();
    let key = SigningKey::from_bytes(&[0x62; 32]);
    let payload = checked(consent.payload());
    for offset in [0, 1, 2, 3, 68, 99, 100, 131, 132, 139] {
        let mut changed = payload.clone();
        changed[offset] ^= 1;
        let bytes = signed(&changed, &consent.target, &key, 77);
        assert!(SponsoredRegistration::from_signed_consent(&bytes).is_err(), "accepted changed relation {offset}");
    }
    for (actor, network) in [(b"did:layerx:other".as_slice(), 77), (b"did:layerx:sponsor".as_slice(), 78)] {
        let bytes = signed(&checked(registration.payload()), &checked(Did::new(actor)), &SigningKey::from_bytes(&[0x51; 32]), network);
        assert!(registration.validate_outer(&checked(activity::decode_signed(&bytes, &registry()))).is_err());
    }
    let mut payload = checked(registration.payload());
    payload.push(0);
    assert!(SponsoredRegistration::decode(&payload).is_err());
}

#[test]
fn changed_public_registration_projection_is_not_encodable() {
    let (_, _, original) = original();
    let mutations: &[fn(&mut SponsoredRegistration)] = &[
        |r| r.network_id += 1,
        |r| r.not_before += 1,
        |r| r.consent.sponsor[0] ^= 1,
        |r| r.consent.target = checked(Did::new(b"did:layerx:other")),
        |r| r.consent.target_public_key[0] ^= 1,
        |r| r.consent.native_asset[0] ^= 1,
        |r| r.consent.action_key[0] ^= 1,
        |r| r.consent.expires_at += 1,
    ];
    for mutate in mutations {
        let mut changed = original.clone();
        mutate(&mut changed);
        assert!(changed.payload().is_err());
    }
    for version in [0, 1, 3, 255] {
        let mut payload = checked(original.payload());
        payload[2] = version;
        assert!(SponsoredRegistration::decode(&payload).is_err());
    }
    let mut consent = consent();
    consent.target_public_key = [0; 32];
    assert!(consent.payload().is_err());
}
