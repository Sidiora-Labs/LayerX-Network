use std::path::{Path, PathBuf};

use layerx_types::payload::{ActivityType, ModuleId, ModuleRegistration, ModuleRegistry};
use layerx_types::result::KnownResult;
use layerx_types::vectors::Corpus;
use layerx_wire::activity::{
    decode_signed, decode_unsigned, encode_signed, encode_unsigned, signing_bytes,
};
use layerx_wire::encode::Encoder;
use layerx_wire::receipt::{
    decode, decode_batch_header, decode_checkpoint, decode_merkle_proof, encode,
    encode_batch_header, encode_checkpoint, encode_merkle_proof,
};
use sha2::{Digest as _, Sha256};

fn repository_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../..")
}

fn activity_type(module: ModuleId, ordinal: u16) -> ActivityType {
    let Ok(value) = ActivityType::new(module, ordinal) else {
        panic!("valid activity type rejected");
    };
    value
}

fn registry() -> ModuleRegistry {
    let module_maximums = [
        (ModuleId::Asset, 8),
        (ModuleId::Escrow, 7),
        (ModuleId::Budget, 7),
        (ModuleId::Stream, 7),
        (ModuleId::Service, 13),
        (ModuleId::Perps, 11),
        (ModuleId::Governance, 1),
        (ModuleId::Bridge, 1),
        (ModuleId::Programs, 7),
    ];
    let registrations: Vec<_> = module_maximums
        .into_iter()
        .map(|(module, maximum)| {
            let activity_types: Vec<_> = (1..=maximum)
                .map(|ordinal| activity_type(module, ordinal))
                .collect();
            let Ok(registration) = ModuleRegistration::new(module, &activity_types) else {
                panic!("valid module registration rejected");
            };
            registration
        })
        .collect();
    let Ok(registry) = ModuleRegistry::new(&registrations) else {
        panic!("valid registry rejected");
    };
    registry
}

fn batch_bytes() -> Vec<u8> {
    batch_bytes_for_version(1)
}

fn batch_bytes_for_version(protocol_version: u16) -> Vec<u8> {
    let mut encoder = Encoder::new(354);
    assert!(encoder
        .structure_header_version(0x1701, protocol_version)
        .is_ok());
    assert!(encoder.u8(15).is_ok());
    for (field, scalar) in [
        (1, u64::from(protocol_version)),
        (2, 77),
        (3, 2),
        (4, 3),
        (5, 4),
        (6, 5),
    ] {
        assert!(encoder.tag(field, 15).is_ok());
        if field == 1 {
            assert!(encoder
                .u16(u16::try_from(scalar).unwrap_or_default())
                .is_ok());
        } else if field == 2 {
            assert!(encoder
                .u32(u32::try_from(scalar).unwrap_or_default())
                .is_ok());
        } else {
            assert!(encoder.u64(scalar).is_ok());
        }
    }
    for field in 7..=13 {
        assert!(encoder.tag(field, 15).is_ok());
        assert!(encoder.bytes(&[field; 32], 32).is_ok());
    }
    assert!(encoder.tag(14, 15).is_ok());
    assert!(encoder.u64(6).is_ok());
    assert!(encoder.tag(15, 15).is_ok());
    assert!(encoder.bytes(&[15; 32], 32).is_ok());
    encoder.finish()
}

fn checkpoint_vector_root(first: u8) -> [u8; 32] {
    let mut root = [0; 32];
    root[0] = first;
    root
}

fn protocol_v2_checkpoint_vector_header() -> Vec<u8> {
    let mut encoder = Encoder::new(354);
    assert!(encoder.structure_header_version(0x1701, 2).is_ok());
    assert!(encoder.u8(15).is_ok());
    for (field, scalar) in [(1, 2_u64), (2, 42), (3, 1), (4, 1), (5, 1), (6, 1_000_000)] {
        assert!(encoder.tag(field, 15).is_ok());
        if field == 1 {
            assert!(encoder.u16(2).is_ok());
        } else if field == 2 {
            assert!(encoder.u32(42).is_ok());
        } else {
            assert!(encoder.u64(scalar).is_ok());
        }
    }
    for (field, first) in (7_u8..=13).zip([0x11_u8, 0x22, 0x33, 0x44, 0x55, 0x66, 0x77]) {
        assert!(encoder.tag(field, 15).is_ok());
        assert!(encoder.bytes(&checkpoint_vector_root(first), 32).is_ok());
    }
    assert!(encoder.tag(14, 15).is_ok());
    assert!(encoder.u64(1000).is_ok());
    assert!(encoder.tag(15, 15).is_ok());
    assert!(encoder.bytes(&checkpoint_vector_root(0x88), 32).is_ok());
    encoder.finish()
}

fn protocol_receipt_bytes() -> Vec<u8> {
    let mut encoder = Encoder::new(4096);
    assert!(encoder.structure_header(0x5201).is_ok());
    assert!(encoder.u16(1).is_ok());
    assert!(encoder.bytes(&[1; 32], 32).is_ok());
    assert!(encoder.u64(9).is_ok());
    for byte in 2..=4 {
        assert!(encoder.bytes(&[byte; 32], 32).is_ok());
    }
    assert!(encoder.i32(0).is_ok());
    assert!(encoder.sequence_length(0, 512).is_ok());
    assert!(encoder.u128(7).is_ok());
    assert!(encoder.bytes(&[5; 32], 32).is_ok());
    assert!(encoder.u16(1).is_ok());
    assert!(encoder.u32(1).is_ok());
    assert!(encoder.u32(1).is_ok());
    assert!(encoder.u8(5).is_ok());
    assert!(encoder.bytes(&[6; 32], 32).is_ok());
    assert!(encoder.u128(11).is_ok());
    assert!(encoder.bytes(&[7; 32], 32).is_ok());
    assert!(encoder.u128(20).is_ok());
    assert!(encoder.u128(9).is_ok());
    assert!(encoder.u64(2).is_ok());
    assert!(encoder.bytes(&[8; 32], 32).is_ok());
    assert!(encoder.u128(3).is_ok());
    assert!(encoder.u128(14).is_ok());
    for byte in 9..=11 {
        assert!(encoder.bytes(&[byte; 32], 32).is_ok());
    }
    assert!(encoder.u64(12).is_ok());
    assert!(encoder.u8(1).is_ok());
    assert!(encoder.bytes(&[13; 64], 64).is_ok());
    encoder.finish()
}

fn proof_bytes() -> Vec<u8> {
    let mut encoder = Encoder::new(64);
    assert!(encoder.structure_header(0x4d50).is_ok());
    assert!(encoder.u32(0).is_ok());
    assert!(encoder.u32(2).is_ok());
    assert!(encoder.u8(1).is_ok());
    assert!(encoder.bytes(&[3; 32], 1024).is_ok());
    encoder.finish()
}

#[test]
fn every_published_activity_and_receipt_round_trips_exactly() {
    let Ok(corpus) = Corpus::load(&repository_root()) else {
        panic!("published corpora failed to load");
    };
    let registry = registry();
    for (index, bytes) in corpus.replay.canonical_activities.iter().enumerate() {
        let Ok(activity) = decode_signed(bytes, &registry) else {
            panic!("activity vector {index} failed decode");
        };
        let Ok(reencoded) = encode_signed(&activity) else {
            panic!("activity vector {index} failed re-encode");
        };
        assert_eq!(reencoded, *bytes, "activity vector {index}");
        let Ok(unsigned) = encode_unsigned(&activity) else {
            panic!("activity vector {index} failed unsigned encode");
        };
        let Ok(decoded_unsigned) = decode_unsigned(&unsigned, &registry) else {
            panic!("activity vector {index} failed unsigned decode");
        };
        assert_eq!(encode_unsigned(&decoded_unsigned), Ok(unsigned.clone()));
        assert_eq!(
            signing_bytes(&activity).map(|value| value.as_bytes().to_vec()),
            Ok(unsigned)
        );
    }
    for (index, bytes) in corpus.replay.expected_receipts.iter().enumerate() {
        let Ok(receipt) = decode(bytes) else {
            panic!("receipt vector {index} failed decode");
        };
        assert_eq!(
            encode(&receipt),
            Ok(bytes.clone()),
            "receipt vector {index}"
        );
    }
}

#[test]
fn all_nine_module_payload_tags_are_preserved() {
    let Ok(corpus) = Corpus::load(&repository_root()) else {
        panic!("published corpora failed to load");
    };
    let registry = registry();
    let template = &corpus.replay.canonical_activities[0];
    for module in 1_u32..=9 {
        let mut bytes = template.clone();
        bytes[14..18].copy_from_slice(&((module << 16) | 1).to_be_bytes());
        let Ok(activity) = decode_signed(&bytes, &registry) else {
            panic!("declared module payload failed decode");
        };
        assert_eq!(activity.activity_type().value(), (module << 16) | 1);
        assert_eq!(encode_signed(&activity), Ok(bytes));
    }
}

#[test]
fn receipt_batch_proof_and_checkpoint_structures_round_trip() {
    let receipt_bytes = protocol_receipt_bytes();
    let Ok(receipt) = decode(&receipt_bytes) else {
        panic!("protocol receipt failed decode");
    };
    assert_eq!(encode(&receipt), Ok(receipt_bytes));

    let batch_bytes = batch_bytes();
    assert_eq!(batch_bytes.len(), 354);
    let Ok(batch) = decode_batch_header(&batch_bytes) else {
        panic!("batch failed decode");
    };
    assert_eq!(encode_batch_header(&batch), Ok(batch_bytes.clone()));

    let proof_bytes = proof_bytes();
    let Ok(proof) = decode_merkle_proof(&proof_bytes) else {
        panic!("proof failed decode");
    };
    assert_eq!(encode_merkle_proof(&proof), Ok(proof_bytes));

    let mut checkpoint_encoder = Encoder::new(2048);
    assert!(checkpoint_encoder.fixed(&batch_bytes).is_ok());
    assert!(checkpoint_encoder.bytes(&[1, 2, 3], 1_048_576).is_ok());
    assert!(checkpoint_encoder.sequence_length(1, 32).is_ok());
    assert!(checkpoint_encoder.bytes(&[4; 32], 32).is_ok());
    assert!(checkpoint_encoder.bytes(&[5; 64], 64).is_ok());
    assert!(checkpoint_encoder.u32(1).is_ok());
    assert!(checkpoint_encoder.bytes(&[6, 7], 1024).is_ok());
    let checkpoint_bytes = checkpoint_encoder.finish();
    let Ok(checkpoint) = decode_checkpoint(&checkpoint_bytes) else {
        panic!("checkpoint failed decode");
    };
    assert_eq!(encode_checkpoint(&checkpoint), Ok(checkpoint_bytes));
}

#[test]
fn occupancy_batch_header_round_trips_with_its_envelope_version() {
    let batch_bytes = batch_bytes_for_version(2);
    let Ok(batch) = decode_batch_header(&batch_bytes) else {
        panic!("occupancy batch failed decode");
    };
    assert_eq!(batch.protocol_version(), 2);
    assert_eq!(encode_batch_header(&batch), Ok(batch_bytes));
}

#[test]
fn protocol_v2_checkpoint_header_matches_the_c_and_solidity_vector() {
    let header = protocol_v2_checkpoint_vector_header();
    assert_eq!(header.len(), 354);
    assert_eq!(&header[..2], &[0, 2]);
    let decoded = decode_batch_header(&header).expect("v2 checkpoint header");
    assert_eq!(decoded.protocol_version(), 2);
    assert_eq!(encode_batch_header(&decoded), Ok(header.clone()));

    let mut hasher = Sha256::new();
    hasher.update(b"LXP/v2/checkpoint-certificate\x00");
    hasher.update(&header);
    hasher.update(0_u32.to_be_bytes());
    assert_eq!(
        <[u8; 32]>::from(hasher.finalize()),
        [
            0xf5, 0xd3, 0x5d, 0xfd, 0x94, 0x88, 0x12, 0xaa, 0xc7, 0x2e, 0x8b, 0xc5, 0xbd, 0x87,
            0xc5, 0x7b, 0xe8, 0x37, 0x7c, 0x89, 0x1e, 0x9b, 0x9f, 0xec, 0x6d, 0x84, 0x35, 0x0c,
            0xdd, 0x8f, 0xf7, 0x43,
        ]
    );
}

#[test]
fn batch_header_refuses_an_envelope_field_version_mismatch() {
    let mut batch_bytes = batch_bytes_for_version(2);
    batch_bytes[6..8].copy_from_slice(&1_u16.to_be_bytes());
    assert_eq!(
        decode_batch_header(&batch_bytes).map(|_| ()),
        Err(layerx_wire::WireError {
            result: KnownResult::VersionUnsupported.into(),
            offset: 0,
        })
    );
}

#[test]
fn unknown_version_activity_and_field_are_deterministic_rejections() {
    let Ok(corpus) = Corpus::load(&repository_root()) else {
        panic!("published corpora failed to load");
    };
    let registry = registry();
    let template = &corpus.replay.canonical_activities[0];

    let mut version = template.clone();
    version[1] = 3;
    assert_eq!(
        decode_signed(&version, &registry).map(|_| ()),
        Err(layerx_wire::WireError {
            result: KnownResult::VersionUnsupported.into(),
            offset: 0,
        })
    );

    let mut activity_type = template.clone();
    activity_type[14..18].copy_from_slice(&0x000a_0001_u32.to_be_bytes());
    assert_eq!(
        decode_signed(&activity_type, &registry)
            .map_err(|error| error.result)
            .map(|_| ()),
        Err(KnownResult::UnknownActivity.into())
    );

    let mut field = template.clone();
    field[5] = 13;
    assert_eq!(
        decode_signed(&field, &registry)
            .map_err(|error| error.result)
            .map(|_| ()),
        Err(KnownResult::UnknownField.into())
    );
}

#[test]
fn state_commitment_header_round_trips_without_changing_default_version() {
    assert_eq!(layerx_wire::limits::PROTOCOL_VERSION, 2);
    let bytes = batch_bytes_for_version(layerx_wire::limits::STATE_COMMITMENT_PROTOCOL_VERSION);
    let header = decode_batch_header(&bytes).expect("explicit state commitment header");
    assert_eq!(header.protocol_version(), 3);
    assert_eq!(encode_batch_header(&header), Ok(bytes));
    let mut future = batch_bytes_for_version(3);
    future[..2].copy_from_slice(&4_u16.to_be_bytes());
    assert!(decode_batch_header(&future).is_err());
}
