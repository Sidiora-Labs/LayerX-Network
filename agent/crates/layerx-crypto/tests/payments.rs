use layerx_crypto::disclosure::{bind, DisclosureError};
use layerx_crypto::payments::Payment;
use layerx_types::payload::{ActivityType, ModuleId, ModuleRegistration, ModuleRegistry};
use layerx_wire::{encode::Encoder, hash::Domain};
use sha2::{Digest as _, Sha256};

const ACTOR: &[u8] = b"did:layerx:alice";
const VECTORS: &[(ModuleId, u16, &str)] = &[
    (
        ModuleId::Asset,
        1,
        include_str!("fixtures/payments/1-1.hex"),
    ),
    (
        ModuleId::Asset,
        4,
        include_str!("fixtures/payments/1-4.hex"),
    ),
    (
        ModuleId::Asset,
        6,
        include_str!("fixtures/payments/1-6.hex"),
    ),
    (
        ModuleId::Asset,
        7,
        include_str!("fixtures/payments/1-7.hex"),
    ),
    (
        ModuleId::Asset,
        8,
        include_str!("fixtures/payments/1-8.hex"),
    ),
    (
        ModuleId::Asset,
        10,
        include_str!("fixtures/payments/1-10.hex"),
    ),
    (
        ModuleId::Asset,
        11,
        include_str!("fixtures/payments/1-11.hex"),
    ),
    (
        ModuleId::Programs,
        5,
        include_str!("fixtures/payments/9-5.hex"),
    ),
    (
        ModuleId::Programs,
        6,
        include_str!("fixtures/payments/9-6.hex"),
    ),
];

fn hex(s: &str) -> Vec<u8> {
    s.trim()
        .as_bytes()
        .chunks_exact(2)
        .map(|pair| {
            let Ok(s) = std::str::from_utf8(pair) else {
                panic!("fixture UTF-8")
            };
            let Ok(b) = u8::from_str_radix(s, 16) else {
                panic!("fixture hex")
            };
            b
        })
        .collect()
}

fn canonical(
    module: ModuleId,
    ordinal: u16,
    payload: &[u8],
) -> Result<(Vec<u8>, ModuleRegistry), DisclosureError> {
    let kind = ActivityType::new(module, ordinal).map_err(|_| DisclosureError::MalformedPayload)?;
    let registration =
        ModuleRegistration::new(module, &[kind]).map_err(|_| DisclosureError::MalformedPayload)?;
    let registry =
        ModuleRegistry::new(&[registration]).map_err(|_| DisclosureError::MalformedPayload)?;
    let mut h = Sha256::new();
    h.update(Domain::PayloadHash.tag());
    h.update(payload);
    let digest: [u8; 32] = h.finalize().into();
    let mut e = Encoder::new(65536);
    let version = layerx_wire::limits::PROTOCOL_VERSION;
    e.structure_header_version(0x1001, version)?;
    e.u8(11)?;
    e.tag(1, 12)?;
    e.u16(version)?;
    e.tag(2, 12)?;
    e.u32(17)?;
    e.tag(3, 12)?;
    e.u32(kind.value())?;
    e.tag(4, 12)?;
    e.bytes(ACTOR, 255)?;
    e.tag(5, 12)?;
    e.bytes(&[9; 32], 524_288)?;
    e.tag(6, 12)?;
    e.u64(7)?;
    e.tag(7, 12)?;
    e.u64(10)?;
    e.u64(100)?;
    e.tag(8, 12)?;
    e.bytes(&[0x71; 32], 32)?;
    e.tag(9, 12)?;
    e.u128(20)?;
    e.tag(10, 12)?;
    e.bytes(&digest, 32)?;
    e.tag(11, 12)?;
    e.bytes(payload, 524_288)?;
    Ok((e.finish(), registry))
}

#[test]
fn fixtures_roundtrip_and_bind_all_fields() -> Result<(), Box<dyn std::error::Error>> {
    for &(module, ordinal, fixture) in VECTORS {
        let payload = hex(fixture);
        let payment = Payment::decode(module, ordinal, &payload, ACTOR)?;
        assert_eq!(payment.encode(ACTOR)?, payload);
        let (canonical, registry) = canonical(module, ordinal, &payload)?;
        let mut disclosure = bind(&canonical, &registry)?;
        assert_eq!(disclosure.payment, Some(payment));
        assert_eq!(disclosure.reencode()?, canonical);
        let _ = disclosure.audit_digest()?;
        disclosure.payment = None;
        assert!(disclosure.reencode().is_err());
        for length in 0..payload.len() {
            assert!(Payment::decode(module, ordinal, &payload[..length], ACTOR).is_err());
        }
        let mut extra = payload;
        extra.push(0);
        assert!(Payment::decode(module, ordinal, &extra, ACTOR).is_err());
    }
    Ok(())
}

#[test]
fn invalid_shapes_are_refused() -> Result<(), Box<dyn std::error::Error>> {
    assert!(Payment::decode(ModuleId::Asset, 9, &[0; 80], ACTOR).is_err());
    for &(module, ordinal, fixture) in VECTORS {
        let payload = hex(fixture);
        let mut p = Payment::decode(module, ordinal, &payload, ACTOR)?;
        match &mut p {
            Payment::Register(r) => r.decimals = 39,
            Payment::Receive { amount, .. }
            | Payment::Mint { amount, .. }
            | Payment::Burn { amount, .. } => *amount = 0,
            Payment::IssueGrant(g) => g.per_draw_maximum = 0,
            Payment::ProgramTransfer { legs, .. } => legs.clear(),
            Payment::ProgramAccount { seed, .. } => *seed = vec![0; 129],
            Payment::OpenAccount { .. } | Payment::RevokeGrant { .. } => continue,
        }
        assert!(p.encode(ACTOR).is_err());
    }
    let p = hex(VECTORS[0].2);
    assert!(Payment::decode(ModuleId::Asset, 1, &p, b"did:layerx:bob").is_err());
    Ok(())
}

#[test]
fn every_accepted_field_mutation_is_refused_at_the_signing_boundary(
) -> Result<(), Box<dyn std::error::Error>> {
    use layerx_crypto::{signer::SigningRequest, SignatureMessage};
    for &(module, ordinal, fixture) in VECTORS {
        let payload = hex(fixture);
        let (canonical, registry) = canonical(module, ordinal, &payload)?;
        let disclosure = bind(&canonical, &registry)?;
        let message = SignatureMessage::new(
            Domain::SignaturePreimage,
            layerx_wire::limits::PROTOCOL_VERSION,
            17,
            &canonical,
        )
        .map_err(|e| format!("{e:?}"))?;
        assert!(SigningRequest::new(message, &disclosure).is_ok());
        for offset in 0..payload.len() {
            let mut changed = payload.clone();
            changed[offset] ^= 1;
            if let Ok(payment) = Payment::decode(module, ordinal, &changed, ACTOR) {
                let mut altered = disclosure.clone();
                altered.payment = Some(payment);
                assert!(
                    SigningRequest::new(message, &altered).is_err(),
                    "undisclosed field {module:?}/{ordinal} at {offset}"
                );
            }
        }
    }
    Ok(())
}

#[test]
fn receive_discloses_source_sequence_independently() -> Result<(), Box<dyn std::error::Error>> {
    let mut payment = Payment::decode(ModuleId::Asset, 6, &hex(VECTORS[2].2), ACTOR)?;
    if let Payment::Receive { sequence, .. } = &mut payment {
        *sequence = 23;
    }
    let payload = payment.encode(ACTOR)?;
    assert_eq!(
        payload,
        hex(include_str!("fixtures/payments/1-6-source-sequence-23.hex"))
    );
    let (bytes, registry) = canonical(ModuleId::Asset, 6, &payload)?;
    let disclosure = bind(&bytes, &registry)?;
    assert_eq!(disclosure.envelope_sequence(), 7);
    assert_eq!(disclosure.payload_sequence()?, Some(23));
    assert_eq!(disclosure.reencode()?, bytes);
    Ok(())
}

#[test]
fn payer_grant_signature_and_identifier_are_bound() {
    let payload = hex(VECTORS[3].2);
    assert_eq!(payload.len(), 346);
    for offset in 0..payload.len() {
        let mut changed = payload.clone();
        changed[offset] ^= 1;
        assert!(Payment::decode(ModuleId::Asset, 7, &changed, ACTOR).is_err());
    }
}
