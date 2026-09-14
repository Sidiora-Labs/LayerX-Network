use ed25519_dalek::{Signer as _, SigningKey};
use layerx_crypto::rotation::{
    OwnerRotation, OwnerRotationCommit, OwnerRotationConsent, OwnerRotationState,
};
use layerx_crypto::SignatureMessage;
use layerx_types::activity::{Authority, EnvelopeBuilder, Signature, TimestampBound};
use layerx_types::amount::Amount;
use layerx_types::ids::{Did, IdempotencyKey};
use layerx_types::payload::{ActivityType, ModuleId, ModuleRegistration, ModuleRegistry, Payload};
use layerx_wire::{
    activity,
    hash::{self, Domain},
};

fn checked<T, E: std::fmt::Debug>(value: Result<T, E>) -> T {
    value.unwrap_or_else(|error| panic!("owner rotation rejected: {error:?}"))
}

fn registry() -> ModuleRegistry {
    checked(ModuleRegistry::new(&[checked(ModuleRegistration::new(
        ModuleId::Governance,
        &[checked(ActivityType::new(ModuleId::Governance, 2))],
    ))]))
}

fn consent() -> OwnerRotationConsent {
    OwnerRotationConsent {
        owner: checked(Did::new(b"did:layerx:owner")),
        current_public_key: SigningKey::from_bytes(&[1; 32]).verifying_key().to_bytes(),
        pending_public_key: SigningKey::from_bytes(&[2; 32]).verifying_key().to_bytes(),
        announcement: [3; 32],
        action_key: [4; 32],
        expires_at: 2000,
    }
}

fn signed(rotation: &OwnerRotation, key: &SigningKey, network: u32) -> Vec<u8> {
    let kind = checked(ActivityType::new(ModuleId::Governance, 2));
    let payload = checked(Payload::new(
        &registry(),
        kind,
        &checked(rotation.payload()),
    ));
    let mut builder = EnvelopeBuilder::new();
    checked(builder.protocol_version(3));
    checked(builder.network_id(network));
    checked(builder.activity_type(kind));
    checked(builder.actor_did(rotation.owner().clone()));
    checked(builder.authority(checked(Authority::owner(&key.verifying_key().to_bytes()))));
    checked(builder.account_sequence(0));
    checked(builder.timestamp_bound(checked(TimestampBound::new(1000, 2000))));
    checked(builder.idempotency_key(IdempotencyKey::new([4; 32])));
    checked(builder.fee_limit(Amount::ZERO));
    checked(builder.payload_hash(checked(hash::payload_hash_for(&payload))));
    checked(builder.payload(payload));
    let envelope = checked(builder.build());
    let unsigned = checked(activity::encode_unsigned_envelope(&envelope));
    let message = checked(SignatureMessage::new(
        Domain::SignaturePreimage,
        3,
        network,
        &unsigned,
    ));
    let signature = key.sign(&message.digest()).to_bytes();
    checked(activity::encode_signed_envelope(
        &envelope.attach_signature(checked(Signature::new(&signature))),
    ))
}

#[test]
fn complete_original_two_key_rotation_binding() {
    let expected = consent();
    let bytes = signed(
        &OwnerRotation::Consent(expected.clone()),
        &SigningKey::from_bytes(&[2; 32]),
        77,
    );
    let commit = checked(OwnerRotationCommit::from_signed_consent(&bytes));
    assert_eq!(commit.consent, expected);
    assert_eq!(commit.signed_consent(), bytes);
    assert_eq!(
        OwnerRotationCommit::decode(&checked(commit.payload())),
        Ok(commit.clone())
    );
    let outer = signed(
        &OwnerRotation::Commit(commit.clone()),
        &SigningKey::from_bytes(&[1; 32]),
        77,
    );
    let outer = checked(activity::decode_signed(&outer, &registry()));
    assert_eq!(
        OwnerRotation::from_activity(&outer),
        Ok(OwnerRotation::Commit(commit.clone()))
    );
    assert!(OwnerRotation::decode(
        &checked(commit.payload()),
        checked(Did::new(b"did:layerx:other")),
        expected.current_public_key
    )
    .is_err());
    assert!(OwnerRotation::decode(
        &checked(commit.payload()),
        expected.owner.clone(),
        expected.pending_public_key
    )
    .is_err());
    for (key, network) in [
        (SigningKey::from_bytes(&[2; 32]), 77),
        (SigningKey::from_bytes(&[1; 32]), 78),
    ] {
        let outer = signed(&OwnerRotation::Commit(commit.clone()), &key, network);
        assert!(
            OwnerRotation::from_activity(&checked(activity::decode_signed(&outer, &registry())))
                .is_err()
        );
    }
    for index in 0..bytes.len() {
        let mut changed = bytes.clone();
        changed[index] ^= 1;
        assert!(
            OwnerRotationCommit::from_signed_consent(&changed).is_err(),
            "signed byte {index}"
        );
    }
    for length in 0..bytes.len() {
        assert!(OwnerRotationCommit::from_signed_consent(&bytes[..length]).is_err());
    }
    let mut extra = bytes;
    extra.push(0);
    assert!(OwnerRotationCommit::from_signed_consent(&extra).is_err());
    let mut changed = commit.clone();
    changed.consent.announcement[0] ^= 1;
    assert!(changed.payload().is_err());
    changed = commit;
    changed.not_before += 1;
    assert!(changed.payload().is_err());
}

#[test]
fn native_announcement_and_cancel_are_exact_versioned_forms() {
    let value = consent();
    let announce = OwnerRotation::Announce {
        owner: value.owner.clone(),
        pending_public_key: value.pending_public_key,
        begin: 1100,
        end: 2000,
        effective_sequence: 9,
    };
    let bytes = checked(announce.payload());
    assert_eq!(bytes.len(), 92);
    assert_eq!(&bytes[..4], &[0x71, 2, 0, 4]);
    assert_eq!(&bytes[68..76], &1100_u64.to_be_bytes());
    assert_eq!(
        OwnerRotation::decode(&bytes, value.owner.clone(), value.current_public_key),
        Ok(announce)
    );
    let cancel = OwnerRotation::Cancel {
        owner: value.owner.clone(),
        announcement: value.announcement,
    };
    let bytes = checked(cancel.payload());
    assert_eq!(bytes.len(), 68);
    assert_eq!(&bytes[..4], &[0x71, 2, 3, 2]);
    assert_eq!(
        OwnerRotation::decode(&bytes, value.owner.clone(), value.current_public_key),
        Ok(cancel)
    );
    for version in [4, 5, 255] {
        let mut changed = bytes.clone();
        changed[2] = version;
        assert!(
            OwnerRotation::decode(&changed, value.owner.clone(), value.current_public_key).is_err()
        );
    }
    let mut invalid = value.clone();
    invalid.pending_public_key = value.current_public_key;
    assert!(invalid.payload().is_err());
    invalid = value.clone();
    invalid.announcement = [0; 32];
    assert!(invalid.payload().is_err());
    invalid = value.clone();
    invalid.action_key = [0; 32];
    assert!(invalid.payload().is_err());
    invalid = value.clone();
    invalid.expires_at = 0;
    assert!(invalid.payload().is_err());
    invalid = value;
    invalid.pending_public_key = [0; 32];
    assert!(invalid.payload().is_err());
}

#[test]
fn native_identity_commitment_binds_owner_and_complete_announcement() {
    let value = consent();
    let mut bytes = [0_u8; 223];
    bytes[..5].copy_from_slice(b"LXGI1");
    bytes[5..37].copy_from_slice(&checked(hash::did_id_for_protocol(&value.owner, 3)));
    bytes[37..69].copy_from_slice(&value.current_public_key);
    bytes[69..77].copy_from_slice(&1_u64.to_be_bytes());
    bytes[111..143].copy_from_slice(&value.pending_public_key);
    bytes[143..151].copy_from_slice(&1100_u64.to_be_bytes());
    bytes[151..159].copy_from_slice(&2000_u64.to_be_bytes());
    bytes[159..167].copy_from_slice(&9_u64.to_be_bytes());
    bytes[215..223].copy_from_slice(&5_u64.to_be_bytes());
    let original = checked(OwnerRotationState::decode(&bytes, &value.owner));
    assert_eq!(original.pending_public_key, Some(value.pending_public_key));
    assert_eq!(original.revocation_sequence, 1);
    assert!(OwnerRotationState::decode(&bytes, &checked(Did::new(b"did:layerx:other"))).is_err());
    for index in 0..bytes.len() {
        let mut changed = bytes;
        changed[index] ^= 1;
        if let Ok(state) = OwnerRotationState::decode(&changed, &value.owner) {
            assert_ne!(
                state.announcement, original.announcement,
                "state byte {index}"
            );
        }
    }
    for length in 0..bytes.len() {
        assert!(OwnerRotationState::decode(&bytes[..length], &value.owner).is_err());
    }
}
