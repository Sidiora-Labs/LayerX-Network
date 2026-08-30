use layerx_client::lni::preparation::{
    decode_preparation_response, encode_preparation_request, PreparationStateError,
};
use layerx_types::ids::Did;
use layerx_types::payload::{ActivityType, ModuleId};

fn actor() -> Did {
    Did::new(b"did:layerx:preparation-test")
        .unwrap_or_else(|error| panic!("actor DID failed: {error:?}"))
}

fn payload(actor: &Did) -> Vec<u8> {
    let actor_length = u16::try_from(actor.as_bytes().len())
        .unwrap_or_else(|error| panic!("actor length failed: {error}"));
    let mut bytes = Vec::new();
    bytes.extend_from_slice(&1_u16.to_be_bytes());
    bytes.extend_from_slice(&actor_length.to_be_bytes());
    bytes.extend_from_slice(actor.as_bytes());
    bytes.extend_from_slice(&77_u32.to_be_bytes());
    bytes.extend_from_slice(&5_u64.to_be_bytes());
    bytes.extend_from_slice(&1_700_000_001_000_u64.to_be_bytes());
    bytes.extend_from_slice(&12_u64.to_be_bytes());
    bytes.extend_from_slice(&[9_u8; 32]);
    bytes.extend_from_slice(&3_u64.to_be_bytes());
    bytes.extend_from_slice(&2_u16.to_be_bytes());
    bytes.extend_from_slice(&1_u16.to_be_bytes());
    bytes.extend_from_slice(&2_u16.to_be_bytes());
    bytes.extend_from_slice(&0x0001_0001_u32.to_be_bytes());
    bytes.extend_from_slice(&0x0001_0002_u32.to_be_bytes());
    bytes.extend_from_slice(&9_u16.to_be_bytes());
    bytes.extend_from_slice(&1_u16.to_be_bytes());
    bytes.extend_from_slice(&0x0009_0001_u32.to_be_bytes());
    bytes
}

#[test]
fn request_codec_is_exact_and_actor_bounded() {
    let actor = actor();
    let encoded = encode_preparation_request(&actor)
        .unwrap_or_else(|error| panic!("request encoding failed: {error:?}"));
    assert_eq!(&encoded[..2], &1_u16.to_be_bytes());
    assert_eq!(
        &encoded[2..4],
        &u16::try_from(actor.as_bytes().len())
            .unwrap_or_else(|error| panic!("actor length failed: {error}"))
            .to_be_bytes()
    );
    assert_eq!(&encoded[4..], actor.as_bytes());
}

#[test]
fn response_codec_preserves_complete_atomic_snapshot() {
    let actor = actor();
    let state = decode_preparation_response(&payload(&actor), &actor, 77, 10)
        .unwrap_or_else(|error| panic!("response decoding failed: {error:?}"));
    assert_eq!(state.actor, actor);
    assert_eq!(state.network_id, 77);
    assert_eq!(state.account_sequence, 5);
    assert_eq!(state.protocol_timestamp, 1_700_000_001_000);
    assert_eq!(state.observed_head_sequence, 12);
    assert_eq!(state.observed_state_root, [9; 32]);
    assert_eq!(state.kernel_epoch, 3);
    let asset = ActivityType::new(ModuleId::Asset, 2)
        .unwrap_or_else(|error| panic!("asset activity failed: {error:?}"));
    let program = ActivityType::new(ModuleId::Programs, 1)
        .unwrap_or_else(|error| panic!("program activity failed: {error:?}"));
    assert!(state.module_registry.declares(asset));
    assert!(state.module_registry.declares(program));
}

#[test]
fn response_codec_rejects_stale_mismatched_and_noncanonical_snapshots() {
    let actor = actor();
    let mut bytes = payload(&actor);
    assert_eq!(
        decode_preparation_response(&bytes, &actor, 77, 13),
        Err(PreparationStateError::StaleSnapshot {
            minimum: 13,
            observed: 12,
        })
    );
    assert_eq!(
        decode_preparation_response(&bytes, &actor, 78, 10),
        Err(PreparationStateError::Network {
            expected: 78,
            actual: 77,
        })
    );
    let other = Did::new(b"did:layerx:someone-else")
        .unwrap_or_else(|error| panic!("other DID failed: {error:?}"));
    assert_eq!(
        decode_preparation_response(&bytes, &other, 77, 10),
        Err(PreparationStateError::ActorMismatch)
    );
    let mut trailing = bytes.clone();
    trailing.push(0);
    assert_eq!(
        decode_preparation_response(&trailing, &actor, 77, 10),
        Err(PreparationStateError::MalformedResponse)
    );
    assert_eq!(
        decode_preparation_response(&bytes[..bytes.len() - 1], &actor, 77, 10),
        Err(PreparationStateError::MalformedResponse)
    );
    let fixed_offset = 4 + actor.as_bytes().len();
    bytes[fixed_offset + 12..fixed_offset + 20].fill(0);
    assert_eq!(
        decode_preparation_response(&bytes, &actor, 77, 10),
        Err(PreparationStateError::MalformedResponse)
    );
}

#[test]
fn response_codec_rejects_unrepresentable_module_registrations() {
    let actor = actor();
    let module_offset = 74 + actor.as_bytes().len();
    let mut mismatched_activity = payload(&actor);
    mismatched_activity[module_offset + 4..module_offset + 8]
        .copy_from_slice(&0x0009_0001_u32.to_be_bytes());
    assert_eq!(
        decode_preparation_response(&mismatched_activity, &actor, 77, 10),
        Err(PreparationStateError::MalformedResponse)
    );

    let mut duplicate_module = payload(&actor);
    duplicate_module[module_offset + 12..module_offset + 14].copy_from_slice(&1_u16.to_be_bytes());
    assert_eq!(
        decode_preparation_response(&duplicate_module, &actor, 77, 10),
        Err(PreparationStateError::MalformedResponse)
    );

    let mut excessive_modules = payload(&actor);
    excessive_modules[module_offset - 2..module_offset].copy_from_slice(&10_u16.to_be_bytes());
    assert_eq!(
        decode_preparation_response(&excessive_modules, &actor, 77, 10),
        Err(PreparationStateError::MalformedResponse)
    );
}
