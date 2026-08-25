use std::any::TypeId;

use layerx_types::activity::{
    ActivityBuildError, Authority, Envelope, EnvelopeBuilder, Signature, TimestampBound,
    UnsignedEnvelope, ENVELOPE_FIELDS,
};
use layerx_types::amount::Amount;
use layerx_types::ids::{Did, IdempotencyKey};
use layerx_types::payload::{
    ActivityType, ModuleId, ModuleRegistration, ModuleRegistry, Payload, PayloadError,
};

fn activity_type(module: ModuleId, ordinal: u16) -> ActivityType {
    let Ok(value) = ActivityType::new(module, ordinal) else {
        panic!("valid module activity rejected");
    };
    value
}

fn registry() -> ModuleRegistry {
    let modules = [
        ModuleId::Asset,
        ModuleId::Escrow,
        ModuleId::Budget,
        ModuleId::Stream,
        ModuleId::Service,
        ModuleId::Perps,
        ModuleId::Governance,
        ModuleId::Bridge,
        ModuleId::Programs,
    ];
    let registrations: Vec<_> = modules
        .into_iter()
        .map(|module| {
            let declared = [activity_type(module, 1)];
            let Ok(registration) = ModuleRegistration::new(module, &declared) else {
                panic!("valid registration rejected");
            };
            registration
        })
        .collect();
    let Ok(registry) = ModuleRegistry::new(&registrations) else {
        panic!("valid registry rejected");
    };
    registry
}

fn complete_builder(payload: Payload) -> EnvelopeBuilder {
    let mut builder = EnvelopeBuilder::new();
    let Ok(actor) = Did::new(b"did:key:alice") else {
        panic!("valid DID rejected");
    };
    let Ok(authority) = Authority::owner(&[7; 32]) else {
        panic!("bounded authority rejected");
    };
    let Ok(timestamp) = TimestampBound::new(10, 20) else {
        panic!("valid timestamp rejected");
    };
    assert!(builder.protocol_version(1).is_ok());
    assert!(builder.network_id(42).is_ok());
    assert!(builder.activity_type(payload.activity_type()).is_ok());
    assert!(builder.actor_did(actor).is_ok());
    assert!(builder.authority(authority).is_ok());
    assert!(builder.account_sequence(3).is_ok());
    assert!(builder.timestamp_bound(timestamp).is_ok());
    assert!(builder
        .idempotency_key(IdempotencyKey::new([8; 32]))
        .is_ok());
    assert!(builder.fee_limit(Amount::from_u128(99)).is_ok());
    assert!(builder.payload_hash([9; 32]).is_ok());
    assert!(builder.payload(payload).is_ok());
    builder
}

#[test]
fn signed_envelope_has_exact_protocol_fields() {
    assert_eq!(
        ENVELOPE_FIELDS,
        [
            "protocol_version",
            "network_id",
            "activity_type",
            "actor_did",
            "authority",
            "account_sequence",
            "timestamp_bound",
            "idempotency_key",
            "fee_limit",
            "payload_hash",
            "payload",
            "signature",
        ]
    );
    let declared = activity_type(ModuleId::Asset, 1);
    let Ok(payload) = Payload::new(&registry(), declared, &[1, 2, 3]) else {
        panic!("declared payload rejected");
    };
    let Ok(unsigned) = complete_builder(payload).build() else {
        panic!("complete envelope rejected");
    };
    let Ok(signature) = Signature::new(&[4; 64]) else {
        panic!("bounded signature rejected");
    };
    let envelope = unsigned.attach_signature(signature);
    assert_eq!(envelope.protocol_version(), 1);
    assert_eq!(envelope.payload().activity_type(), declared);
    assert_eq!(envelope.signature().as_bytes(), &[4; 64]);
}

#[test]
fn missing_and_repeated_fields_are_rejected() {
    let declared = activity_type(ModuleId::Asset, 1);
    let Ok(payload) = Payload::new(&registry(), declared, &[]) else {
        panic!("declared payload rejected");
    };
    let mut complete = complete_builder(payload.clone());
    assert_eq!(
        complete.protocol_version(2),
        Err(ActivityBuildError::RepeatedField("protocol_version"))
    );

    let mut missing = EnvelopeBuilder::new();
    assert!(missing.payload(payload).is_ok());
    assert_eq!(
        missing.build(),
        Err(ActivityBuildError::MissingField("activity_type"))
    );
}

#[test]
fn every_module_payload_requires_an_exact_registration() {
    let registry = registry();
    for module in [
        ModuleId::Asset,
        ModuleId::Escrow,
        ModuleId::Budget,
        ModuleId::Stream,
        ModuleId::Service,
        ModuleId::Perps,
        ModuleId::Governance,
        ModuleId::Bridge,
        ModuleId::Programs,
    ] {
        let declared = activity_type(module, 1);
        let Ok(payload) = Payload::new(&registry, declared, &[module as u8]) else {
            panic!("registered module payload rejected");
        };
        assert_eq!(payload.activity_type(), declared);
    }
    let undeclared = activity_type(ModuleId::Bridge, 2);
    assert_eq!(
        Payload::new(&registry, undeclared, &[]),
        Err(PayloadError::UndeclaredActivity(undeclared.value()))
    );
}

#[test]
fn unsigned_and_signed_envelopes_are_distinct_types() {
    assert_ne!(TypeId::of::<UnsignedEnvelope>(), TypeId::of::<Envelope>());
    assert_eq!(
        ActivityType::from_u32(0x000a_0001),
        Err(PayloadError::UnknownModule(10))
    );
    assert_eq!(
        TimestampBound::new(20, 10),
        Err(ActivityBuildError::InvalidTimestampBound)
    );
}
