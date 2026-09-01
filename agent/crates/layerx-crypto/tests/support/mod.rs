#![allow(dead_code)]

use ed25519_dalek::{Signer as _, SigningKey};
use layerx_types::payload::{ActivityType, ModuleId, ModuleRegistration, ModuleRegistry};
use layerx_wire::encode::Encoder;
use layerx_wire::hash::Domain;
use sha2::{Digest as _, Sha256};

const NETWORK_ID: u32 = 17;
const PROTOCOL_VERSION: u16 = layerx_wire::limits::PROTOCOL_VERSION;
const SEQUENCE: u64 = 7;
const NOT_BEFORE: u64 = 10;
const EXPIRES_AT: u64 = 100;
const FEE_LIMIT: u128 = 20;
const IDEMPOTENCY_KEY: [u8; 32] = [0x71; 32];

pub fn registry() -> ModuleRegistry {
    let Ok(send) = ActivityType::new(ModuleId::Asset, 5) else {
        panic!("asset send activity type rejected");
    };
    let Ok(asset) = ModuleRegistration::new(ModuleId::Asset, &[send]) else {
        panic!("asset registration rejected");
    };
    let Ok(registry) = ModuleRegistry::new(&[asset]) else {
        panic!("module registry rejected");
    };
    registry
}

pub fn bridge_registry() -> ModuleRegistry {
    let Ok(credit) = ActivityType::new(ModuleId::Bridge, 1) else {
        panic!("bridge credit activity type rejected");
    };
    let Ok(bridge) = ModuleRegistration::new(ModuleId::Bridge, &[credit]) else {
        panic!("bridge registration rejected");
    };
    let Ok(registry) = ModuleRegistry::new(&[bridge]) else {
        panic!("module registry rejected");
    };
    registry
}

pub fn bridge_withdraw_registry() -> ModuleRegistry {
    let Ok(withdraw) = ActivityType::new(ModuleId::Bridge, 2) else {
        panic!("bridge withdrawal activity type rejected");
    };
    let Ok(bridge) = ModuleRegistration::new(ModuleId::Bridge, &[withdraw]) else {
        panic!("bridge registration rejected");
    };
    let Ok(registry) = ModuleRegistry::new(&[bridge]) else {
        panic!("module registry rejected");
    };
    registry
}

fn encode_send_common(encoder: &mut Encoder, amount: u128) {
    assert!(encoder.fixed(&[0x11; 32]).is_ok());
    assert!(encoder.fixed(&[0x22; 32]).is_ok());
    assert!(encoder.fixed(&[0x33; 32]).is_ok());
    assert!(encoder.u128(amount).is_ok());
    assert!(encoder.u64(SEQUENCE).is_ok());
    assert!(encoder.fixed(&IDEMPOTENCY_KEY).is_ok());
    assert!(encoder.u64(EXPIRES_AT).is_ok());
    assert!(encoder.fixed(&[0x44; 32]).is_ok());
    assert!(encoder.u8(2).is_ok());
    assert!(encoder.u8(1).is_ok());
    assert!(encoder.u64(NOT_BEFORE).is_ok());
    assert!(encoder.u8(2).is_ok());
    assert!(encoder.u64(EXPIRES_AT).is_ok());
}

fn send_payload(amount: u128) -> (Vec<u8>, [u8; 32]) {
    let signing_key = SigningKey::from_bytes(&[0x53; 32]);
    let public_key = signing_key.verifying_key().to_bytes();

    let mut authorization = Encoder::new(512);
    assert!(authorization.u16(0x5301).is_ok());
    encode_send_common(&mut authorization, amount);
    assert!(authorization.u8(1).is_ok());
    assert!(authorization.fixed(&[0x11; 32]).is_ok());
    assert!(authorization.fixed(&[0x44; 32]).is_ok());
    assert!(authorization.u32(NETWORK_ID).is_ok());
    assert!(authorization.u16(PROTOCOL_VERSION).is_ok());
    let authorization = authorization.finish();
    let mut hasher = Sha256::new();
    hasher.update(Domain::SignaturePreimage.tag());
    hasher.update(&authorization);
    let digest: [u8; 32] = hasher.finalize().into();
    let signature = signing_key.sign(&digest).to_bytes();

    let mut payload = Encoder::new(512);
    assert!(payload.u16(0x5301).is_ok());
    assert!(payload.u16(10).is_ok());
    encode_send_common(&mut payload, amount);
    assert!(payload.u8(1).is_ok());
    assert!(payload.fixed(&[0x11; 32]).is_ok());
    assert!(payload.fixed(&public_key).is_ok());
    assert!(payload.fixed(&signature).is_ok());
    assert!(payload.fixed(&[0x44; 32]).is_ok());
    assert!(payload.u32(NETWORK_ID).is_ok());
    assert!(payload.u16(PROTOCOL_VERSION).is_ok());
    (payload.finish(), public_key)
}

pub fn canonical_send(amount: u128) -> Vec<u8> {
    let (payload, authority) = send_payload(amount);
    let mut hasher = Sha256::new();
    hasher.update(Domain::PayloadHash.tag());
    hasher.update(&payload);
    let payload_hash: [u8; 32] = hasher.finalize().into();

    let mut activity = Encoder::new(4096);
    assert!(activity
        .structure_header_version(0x1001, PROTOCOL_VERSION)
        .is_ok());
    assert!(activity.u8(11).is_ok());
    assert!(activity.tag(1, 12).is_ok());
    assert!(activity.u16(PROTOCOL_VERSION).is_ok());
    assert!(activity.tag(2, 12).is_ok());
    assert!(activity.u32(NETWORK_ID).is_ok());
    assert!(activity.tag(3, 12).is_ok());
    assert!(activity.u32(0x0001_0005).is_ok());
    assert!(activity.tag(4, 12).is_ok());
    assert!(activity.bytes(b"did:layerx:alice", 255).is_ok());
    assert!(activity.tag(5, 12).is_ok());
    assert!(activity.bytes(&authority, 524_288).is_ok());
    assert!(activity.tag(6, 12).is_ok());
    assert!(activity.u64(SEQUENCE).is_ok());
    assert!(activity.tag(7, 12).is_ok());
    assert!(activity.u64(NOT_BEFORE).is_ok());
    assert!(activity.u64(EXPIRES_AT).is_ok());
    assert!(activity.tag(8, 12).is_ok());
    assert!(activity.bytes(&IDEMPOTENCY_KEY, 32).is_ok());
    assert!(activity.tag(9, 12).is_ok());
    assert!(activity.u128(FEE_LIMIT).is_ok());
    assert!(activity.tag(10, 12).is_ok());
    assert!(activity.bytes(&payload_hash, 32).is_ok());
    assert!(activity.tag(11, 12).is_ok());
    assert!(activity.bytes(&payload, 524_288).is_ok());
    activity.finish()
}

pub fn canonical_bridge_withdraw(amount: u128) -> Vec<u8> {
    let mut payload = Encoder::new(512);
    assert!(payload.u16(0x4802).is_ok());
    assert!(payload.u16(7).is_ok());
    assert!(payload.fixed(&[0x41; 32]).is_ok());
    assert!(payload.fixed(&[0x11; 32]).is_ok());
    assert!(payload.fixed(&[0x22; 32]).is_ok());
    assert!(payload.fixed(&[0x44; 20]).is_ok());
    assert!(payload.fixed(&[0x33; 32]).is_ok());
    assert!(payload.u128(amount).is_ok());
    assert!(payload.fixed(&IDEMPOTENCY_KEY).is_ok());
    let payload = payload.finish();
    let mut hasher = Sha256::new();
    hasher.update(Domain::PayloadHash.tag());
    hasher.update(&payload);
    let payload_hash: [u8; 32] = hasher.finalize().into();

    let mut activity = Encoder::new(4096);
    assert!(activity
        .structure_header_version(0x1001, PROTOCOL_VERSION)
        .is_ok());
    assert!(activity.u8(11).is_ok());
    assert!(activity.tag(1, 12).is_ok());
    assert!(activity.u16(PROTOCOL_VERSION).is_ok());
    assert!(activity.tag(2, 12).is_ok());
    assert!(activity.u32(NETWORK_ID).is_ok());
    assert!(activity.tag(3, 12).is_ok());
    assert!(activity.u32(0x0008_0002).is_ok());
    assert!(activity.tag(4, 12).is_ok());
    assert!(activity.bytes(b"did:layerx:withdraw", 255).is_ok());
    assert!(activity.tag(5, 12).is_ok());
    assert!(activity.bytes(&[0x55; 32], 524_288).is_ok());
    assert!(activity.tag(6, 12).is_ok());
    assert!(activity.u64(SEQUENCE).is_ok());
    assert!(activity.tag(7, 12).is_ok());
    assert!(activity.u64(NOT_BEFORE).is_ok());
    assert!(activity.u64(EXPIRES_AT).is_ok());
    assert!(activity.tag(8, 12).is_ok());
    assert!(activity.bytes(&IDEMPOTENCY_KEY, 32).is_ok());
    assert!(activity.tag(9, 12).is_ok());
    assert!(activity.u128(FEE_LIMIT).is_ok());
    assert!(activity.tag(10, 12).is_ok());
    assert!(activity.bytes(&payload_hash, 32).is_ok());
    assert!(activity.tag(11, 12).is_ok());
    assert!(activity.bytes(&payload, 524_288).is_ok());
    activity.finish()
}

pub fn canonical_bridge_credit(amount: u128) -> Vec<u8> {
    let mut payload = Encoder::new(512);
    assert!(payload.u16(0x4801).is_ok());
    assert!(payload.u16(7).is_ok());
    assert!(payload.fixed(&[0x41; 32]).is_ok());
    assert!(payload.fixed(&[0x42; 32]).is_ok());
    assert!(payload.fixed(&[0x11; 32]).is_ok());
    assert!(payload.fixed(&[0x22; 32]).is_ok());
    assert!(payload.fixed(&[0x33; 32]).is_ok());
    assert!(payload.u128(amount).is_ok());
    assert!(payload.fixed(&IDEMPOTENCY_KEY).is_ok());
    let payload = payload.finish();
    let mut hasher = Sha256::new();
    hasher.update(Domain::PayloadHash.tag());
    hasher.update(&payload);
    let payload_hash: [u8; 32] = hasher.finalize().into();

    let mut activity = Encoder::new(4096);
    assert!(activity
        .structure_header_version(0x1001, PROTOCOL_VERSION)
        .is_ok());
    assert!(activity.u8(11).is_ok());
    assert!(activity.tag(1, 12).is_ok());
    assert!(activity.u16(PROTOCOL_VERSION).is_ok());
    assert!(activity.tag(2, 12).is_ok());
    assert!(activity.u32(NETWORK_ID).is_ok());
    assert!(activity.tag(3, 12).is_ok());
    assert!(activity.u32(0x0008_0001).is_ok());
    assert!(activity.tag(4, 12).is_ok());
    assert!(activity.bytes(b"did:layerx:deposit", 255).is_ok());
    assert!(activity.tag(5, 12).is_ok());
    assert!(activity.bytes(&[0x55; 32], 524_288).is_ok());
    assert!(activity.tag(6, 12).is_ok());
    assert!(activity.u64(SEQUENCE).is_ok());
    assert!(activity.tag(7, 12).is_ok());
    assert!(activity.u64(NOT_BEFORE).is_ok());
    assert!(activity.u64(EXPIRES_AT).is_ok());
    assert!(activity.tag(8, 12).is_ok());
    assert!(activity.bytes(&IDEMPOTENCY_KEY, 32).is_ok());
    assert!(activity.tag(9, 12).is_ok());
    assert!(activity.u128(FEE_LIMIT).is_ok());
    assert!(activity.tag(10, 12).is_ok());
    assert!(activity.bytes(&payload_hash, 32).is_ok());
    assert!(activity.tag(11, 12).is_ok());
    assert!(activity.bytes(&payload, 524_288).is_ok());
    activity.finish()
}
