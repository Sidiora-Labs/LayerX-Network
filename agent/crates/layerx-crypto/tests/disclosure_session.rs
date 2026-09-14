use std::fmt::Debug;

use ed25519_dalek::SigningKey;
use layerx_crypto::authority_grant::NativeFeeBudget;
use layerx_crypto::disclosure::{
    bind, DisclosedSessionGrant, DisclosedSessionReplacement, DisclosureError,
};
use layerx_crypto::session::{issue_session_key, SessionKeyRequest, SessionPurpose};
use layerx_crypto::signer::{SignError, SigningRequest};
use layerx_crypto::SignatureMessage;
use layerx_types::activity::{Authority, EnvelopeBuilder, TimestampBound};
use layerx_types::amount::Amount;
use layerx_types::ids::{Did, IdempotencyKey};
use layerx_types::payload::{ActivityType, ModuleId, ModuleRegistration, ModuleRegistry, Payload};
use layerx_wire::activity::encode_unsigned_envelope;
use layerx_wire::encode::Encoder;
use layerx_wire::hash;
use sha2::{Digest as _, Sha256};

fn checked<T, E: Debug>(result: Result<T, E>) -> T {
    result.unwrap_or_else(|error| panic!("canonical fixture rejected: {error:?}"))
}

fn owner() -> (Did, [u8; 32]) {
    let key = SigningKey::from_bytes(&[0x11; 32])
        .verifying_key()
        .to_bytes();
    let mut did = String::from("did:layerx:");
    for byte in key {
        use std::fmt::Write as _;
        checked(write!(&mut did, "{byte:02x}"));
    }
    (checked(Did::new(did.as_bytes())), key)
}

fn registry() -> ModuleRegistry {
    let kind = checked(ActivityType::new(ModuleId::Governance, 5));
    checked(ModuleRegistry::new(&[checked(ModuleRegistration::new(
        ModuleId::Governance,
        &[kind],
    ))]))
}

fn request(version: u8) -> SessionKeyRequest {
    let (did, _) = owner();
    SessionKeyRequest {
        grantor: checked(hash::did_id_for_protocol(&did, 3)),
        session_public_key: SigningKey::from_bytes(&[0x33; 32])
            .verifying_key()
            .to_bytes(),
        not_before: 1000,
        expires_at: Some(3_601_000),
        permitted_activity_types: if version == 3 {
            Vec::new()
        } else {
            vec![checked(ActivityType::new(ModuleId::Asset, 5))]
        },
        revocation_sequence: Some(7),
        fee_budget: (version == 2).then_some(NativeFeeBudget {
            asset: [0x44; 32],
            maximum_per_activity: 10,
            maximum_total: 100,
            period_length: 60_000,
            maximum_per_period: 30,
            period_start: 1000,
        }),
        purpose: if version == 3 {
            SessionPurpose::Authentication
        } else {
            SessionPurpose::Activity
        },
    }
}

fn session(version: u8) -> DisclosedSessionGrant {
    DisclosedSessionGrant {
        grant: checked(issue_session_key(&request(version))),
        expiry_sequence: 9000,
        action_key: [0x55; 32],
        replacement: None,
    }
}

fn payload(value: &DisclosedSessionGrant) -> Vec<u8> {
    let mut encoder = Encoder::new(1024);
    checked(encoder.u16(0x7105));
    checked(encoder.u8(if value.replacement.is_some() { 2 } else { 1 }));
    checked(encoder.u8(if value.replacement.is_some() { 5 } else { 3 }));
    checked(encoder.bytes(&value.grant.registration_payload, 1024));
    checked(encoder.u64(value.expiry_sequence));
    checked(encoder.bytes(&value.action_key, 32));
    if let Some(replacement) = value.replacement {
        checked(encoder.bytes(&replacement.predecessor_grant_id, 32));
        checked(encoder.bytes(&replacement.expected_charge_state, 32));
    }
    encoder.finish()
}

fn unsigned(payload: &[u8], did: &Did, key: &[u8]) -> Vec<u8> {
    let kind = checked(ActivityType::new(ModuleId::Governance, 5));
    let mut hasher = Sha256::new();
    hasher.update(hash::Domain::PayloadHash.tag());
    hasher.update(payload);
    let mut builder = EnvelopeBuilder::new();
    checked(builder.protocol_version(3));
    checked(builder.network_id(77));
    checked(builder.activity_type(kind));
    checked(builder.actor_did(did.clone()));
    checked(builder.authority(checked(Authority::owner(key))));
    checked(builder.account_sequence(10));
    checked(builder.timestamp_bound(checked(TimestampBound::new(1000, 2000))));
    checked(builder.idempotency_key(IdempotencyKey::new([9; 32])));
    checked(builder.fee_limit(Amount::from_u128(4)));
    checked(builder.payload_hash(hasher.finalize().into()));
    checked(builder.payload(checked(Payload::new(&registry(), kind, payload))));
    checked(encode_unsigned_envelope(&checked(builder.build())))
}

fn canonical(value: &DisclosedSessionGrant) -> Vec<u8> {
    let (did, key) = owner();
    unsigned(&payload(value), &did, &key)
}

#[test]
fn supported_session_versions_disclose_exact_canonical_registration() {
    for version in [1, 2, 3] {
        let value = session(version);
        let bytes = canonical(&value);
        let disclosure = checked(bind(&bytes, &registry()));
        assert_eq!(disclosure.session_grant, Some(value.clone()));
        assert!(disclosure.counterparties.is_empty());
        assert!(disclosure.amounts.is_empty());
        assert!(disclosure.authority_grant.is_none());
        assert!(disclosure.payment.is_none());
        assert_eq!(
            disclosure.asset,
            value.grant.fee_budget.map_or([0; 32], |fee| fee.asset)
        );
        assert_eq!(disclosure.fee_limit, 4);
        assert_eq!(disclosure.expiry.payload_expires_at, value.grant.expires_at);
        assert_eq!(disclosure.reencode(), Ok(bytes.clone()));
        let message = checked(SignatureMessage::new(
            hash::Domain::SignaturePreimage,
            3,
            77,
            &bytes,
        ));
        assert!(SigningRequest::new(message, &disclosure).is_ok());
        assert!(disclosure.audit_digest().is_ok());
    }
}

#[test]
fn every_session_disclosure_field_is_checked_before_signing() {
    let value = session(2);
    let bytes = canonical(&value);
    let disclosure = checked(bind(&bytes, &registry()));
    let mutations: &[fn(&mut DisclosedSessionGrant)] = &[
        |s| s.grant.grantor[0] ^= 1,
        |s| s.grant.not_before += 1,
        |s| s.grant.expires_at += 1,
        |s| s.grant.session_public_key[0] ^= 1,
        |s| s.grant.grant_id[0] ^= 1,
        |s| s.grant.registration_payload[0] ^= 1,
        |s| s.grant.authority = checked(Authority::owner(&[0x66; 32])),
        |s| s.grant.permitted_activity_types.clear(),
        |s| s.grant.revocation_sequence += 1,
        |s| s.grant.purpose = SessionPurpose::Authentication,
        |s| s.grant.fee_budget = None,
        |s| s.expiry_sequence += 1,
        |s| s.action_key[0] ^= 1,
    ];
    for mutate in mutations {
        let mut changed = disclosure.clone();
        let Some(session) = changed.session_grant.as_mut() else {
            panic!("missing session")
        };
        mutate(session);
        assert_ne!(changed.session_grant, disclosure.session_grant);
        assert_eq!(
            changed.reencode(),
            Err(DisclosureError::FieldMismatch("session_grant"))
        );
        assert!(changed.audit_digest().is_err());
        let message = checked(SignatureMessage::new(
            hash::Domain::SignaturePreimage,
            3,
            77,
            &bytes,
        ));
        assert_eq!(
            SigningRequest::new(message, &changed).err(),
            Some(SignError::DisclosureMismatch("session_grant"))
        );
    }
    let mut missing = disclosure.clone();
    missing.session_grant = None;
    assert_eq!(
        missing.reencode(),
        Err(DisclosureError::FieldMismatch("session_grant"))
    );
}

#[test]
fn every_fee_budget_field_changes_the_validated_signing_disclosure() {
    let value = session(2);
    let bytes = canonical(&value);
    let disclosure = checked(bind(&bytes, &registry()));
    let mutations: &[fn(&mut NativeFeeBudget)] = &[
        |fee| fee.asset[0] ^= 1,
        |fee| fee.maximum_per_activity += 1,
        |fee| fee.maximum_total += 1,
        |fee| fee.period_length += 1,
        |fee| fee.maximum_per_period += 1,
        |fee| fee.period_start += 1,
    ];
    for mutate in mutations {
        let mut changed = disclosure.clone();
        let Some(session) = changed.session_grant.as_mut() else {
            panic!("missing session")
        };
        let Some(fee) = session.grant.fee_budget.as_mut() else {
            panic!("missing fee budget")
        };
        mutate(fee);
        assert_eq!(
            changed.reencode(),
            Err(DisclosureError::FieldMismatch("session_grant"))
        );
        assert!(changed.audit_digest().is_err());
    }
}

#[test]
fn malformed_wrappers_or_grants_cannot_reach_signing() {
    let (did, key) = owner();
    for version in [1, 2, 3] {
        let value = session(version);
        let bytes = payload(&value);
        for length in 0..bytes.len() {
            assert!(
                bind(&unsigned(&bytes[..length], &did, &key), &registry()).is_err(),
                "accepted prefix {length}"
            );
        }
        for offset in [0, 1, 2, 3] {
            let mut malformed = bytes.clone();
            malformed[offset] ^= 0x80;
            assert!(bind(&unsigned(&malformed, &did, &key), &registry()).is_err());
        }
        let mut trailing = bytes.clone();
        trailing.push(0);
        assert!(bind(&unsigned(&trailing, &did, &key), &registry()).is_err());
        for mutate in [
            |s: &mut DisclosedSessionGrant| s.expiry_sequence = 0,
            |s: &mut DisclosedSessionGrant| s.action_key = [0; 32],
            |s: &mut DisclosedSessionGrant| s.grant.registration_payload.push(0),
        ] {
            let mut malformed = value.clone();
            mutate(&mut malformed);
            assert!(bind(&canonical(&malformed), &registry()).is_err());
        }
        assert!(bind(
            &unsigned(&bytes, &did, &value.grant.session_public_key),
            &registry()
        )
        .is_err());
        assert!(bind(
            &unsigned(&bytes, &did, &value.grant.registration_payload),
            &registry()
        )
        .is_err());
        let other = checked(Did::new(b"did:layerx:other"));
        assert!(bind(&unsigned(&bytes, &other, &key), &registry()).is_err());
    }
}

#[test]
fn replacement_binds_predecessor_and_charged_state_without_replacing_fee_limits() {
    let mut value = session(2);
    value.replacement = Some(DisclosedSessionReplacement {
        predecessor_grant_id: [0x77; 32],
        expected_charge_state: [0x88; 32],
    });
    let bytes = canonical(&value);
    let disclosure = checked(bind(&bytes, &registry()));
    assert_eq!(disclosure.session_grant, Some(value.clone()));
    for mutate in [
        |r: &mut DisclosedSessionReplacement| r.predecessor_grant_id[0] ^= 1,
        |r: &mut DisclosedSessionReplacement| r.expected_charge_state[0] ^= 1,
    ] {
        let mut changed = value.clone();
        let Some(replacement) = changed.replacement.as_mut() else {
            panic!("missing replacement")
        };
        mutate(replacement);
        let changed_bytes = canonical(&changed);
        let changed_disclosure = checked(bind(&changed_bytes, &registry()));
        assert_ne!(
            checked(disclosure.audit_digest()),
            checked(changed_disclosure.audit_digest())
        );
        let message = checked(SignatureMessage::new(
            hash::Domain::SignaturePreimage,
            3,
            77,
            &changed_bytes,
        ));
        assert!(SigningRequest::new(message, &disclosure).is_err());
        let mut tampered = disclosure.clone();
        tampered.session_grant = Some(changed);
        assert_eq!(
            tampered.reencode(),
            Err(DisclosureError::FieldMismatch("session_grant"))
        );
    }
    for predecessor in [[0; 32], value.grant.grant_id] {
        let mut invalid = value.clone();
        invalid.replacement = Some(DisclosedSessionReplacement {
            predecessor_grant_id: predecessor,
            expected_charge_state: [0x88; 32],
        });
        assert!(bind(&canonical(&invalid), &registry()).is_err());
    }
    let mut zero = value.clone();
    zero.replacement = Some(DisclosedSessionReplacement {
        predecessor_grant_id: [0x77; 32],
        expected_charge_state: [0; 32],
    });
    assert!(bind(&canonical(&zero), &registry()).is_err());
    for version in [1, 3] {
        let mut invalid = session(version);
        invalid.replacement = value.replacement;
        assert!(bind(&canonical(&invalid), &registry()).is_err());
    }
    let registration = checked(bind(&canonical(&session(2)), &registry()));
    assert_ne!(
        checked(registration.audit_digest()),
        checked(disclosure.audit_digest())
    );
}
