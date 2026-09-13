use std::fmt::Debug;

use ed25519_dalek::{Signer as _, SigningKey};
use layerx_crypto::SignatureMessage;
use layerx_intents::canonical;
use layerx_types::activity::{Authority, EnvelopeBuilder, Signature, TimestampBound};
use layerx_types::amount::Amount;
use layerx_types::ids::{Did, IdempotencyKey};
use layerx_types::payload::{ModuleRegistration, ModuleRegistry, Payload};

pub struct SignedOwner {
    pub bytes: Vec<u8>,
    pub id: [u8; 32],
    pub public_key: [u8; 32],
    pub actor: Vec<u8>,
}

fn checked<T, E: Debug>(value: Result<T, E>) -> T {
    value.unwrap_or_else(|error| panic!("canonical owner encoding: {error:?}"))
}

pub fn sign(payload: &Payload, action_key: [u8; 32], version: u16, network: u32) -> SignedOwner {
    let signer = SigningKey::from_bytes(&[0x68; 32]);
    let public_key = signer.verifying_key().to_bytes();
    let actor = b"did:layerx:owner-receipt-fixture".to_vec();
    let mut builder = EnvelopeBuilder::new();
    checked(builder.protocol_version(version));
    checked(builder.network_id(network));
    checked(builder.activity_type(payload.activity_type()));
    checked(builder.actor_did(checked(Did::new(&actor))));
    checked(builder.authority(checked(Authority::owner(&public_key))));
    checked(builder.account_sequence(1));
    checked(builder.timestamp_bound(checked(TimestampBound::new(900, 2000))));
    checked(builder.idempotency_key(IdempotencyKey::new(action_key)));
    checked(builder.fee_limit(Amount::from_u128(10)));
    checked(builder.payload_hash(checked(canonical::payload_hash_for(payload))));
    checked(builder.payload(payload.clone()));
    let unsigned = checked(builder.build());
    let canonical = checked(canonical::unsigned_envelope_bytes(&unsigned));
    let digest = checked(SignatureMessage::new(
        canonical::Domain::SignaturePreimage,
        version,
        network,
        &canonical,
    ))
    .digest();
    let signature = signer.sign(&digest).to_bytes();
    let bytes = checked(canonical::signed_envelope_bytes(
        &unsigned.attach_signature(checked(Signature::new(&signature))),
    ));
    let registry = checked(ModuleRegistry::new(&[checked(ModuleRegistration::new(
        payload.activity_type().module(),
        &[payload.activity_type()],
    ))]));
    let decoded = checked(canonical::decode_signed_activity(&bytes, &registry));
    SignedOwner {
        id: checked(canonical::activity_id(&decoded)),
        bytes,
        public_key,
        actor,
    }
}
