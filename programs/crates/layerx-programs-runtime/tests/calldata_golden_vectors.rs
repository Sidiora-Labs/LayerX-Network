//! Golden vector tests for calldata encoding.
//!
//! This test suite validates the frozen LayerX canonical calldata encoding
//! against comprehensive golden vectors covering every type, boundary case,
//! rejection scenario, and the empty call.

use layerx_programs_runtime::abi::codec::{Calldata, CodecError};
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::Path;

#[derive(Debug, Deserialize, Serialize)]
struct TestVector {
    description: String,
    hex: String,
    expected: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    error: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    note: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    hex_generator: Option<String>,
}

fn hex_decode(hex: &str) -> Vec<u8> {
    let cleaned = hex.replace(' ', "");
    (0..cleaned.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(&cleaned[i..i + 2], 16).expect("invalid hex"))
        .collect()
}

fn load_vectors(path: &Path) -> Vec<TestVector> {
    let content = fs::read_to_string(path)
        .unwrap_or_else(|error| panic!("failed to read {}: {}", path.display(), error));
    serde_json::from_str(&content)
        .unwrap_or_else(|error| panic!("failed to parse {}: {}", path.display(), error))
}

fn run_vector(vector: &TestVector) -> bool {
    if vector.hex_generator.is_some() {
        return false;
    }

    let bytes = hex_decode(&vector.hex);
    let result = Calldata::from_bytes(&bytes);

    match vector.expected.as_str() {
        "pass" => {
            if let Err(error) = result {
                panic!(
                    "Vector '{}' expected to pass but failed: {:?}\nBytes: {:02x?}",
                    vector.description, error, bytes
                );
            }
        }
        "reject" => {
            if result.is_ok() {
                panic!(
                    "Vector '{}' expected to reject but passed\nBytes: {:02x?}",
                    vector.description, bytes
                );
            }
            if let Some(expected_error) = &vector.error {
                let actual_error = format!("{:?}", result.unwrap_err());
                if !actual_error.contains(expected_error) {
                    panic!(
                        "Vector '{}' rejected with wrong error\nExpected: {}\nActual: {}\nBytes: {:02x?}",
                        vector.description, expected_error, actual_error, bytes
                    );
                }
            }
        }
        other => panic!("Invalid expected value: {}", other),
    }

    true
}

fn test_vector_directory(dir: &str, expected_executed: usize, expected_skipped: usize) {
    let base = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../tests/vectors/calldata")
        .join(dir);
    if !base.exists() {
        panic!("Vector directory does not exist: {}", base.display());
    }

    let mut executed = 0;
    let mut skipped = 0;
    for entry in fs::read_dir(&base).expect("read directory") {
        let entry = entry.expect("directory entry");
        let path = entry.path();
        if path.extension().and_then(|s| s.to_str()) == Some("json") {
            let vectors = load_vectors(&path);
            for vector in &vectors {
                if run_vector(vector) {
                    executed += 1;
                } else {
                    skipped += 1;
                }
            }
        }
    }

    assert!(executed > 0, "No vectors executed in {}", base.display());
    assert_eq!(
        executed,
        expected_executed,
        "Executed vector count mismatch in {} ({} skipped)",
        base.display(),
        skipped
    );
    assert_eq!(
        skipped,
        expected_skipped,
        "Skipped vector count mismatch in {} ({} executed)",
        base.display(),
        executed
    );
}

#[test]
fn valid_primitives() {
    test_vector_directory("valid", 43, 0);
}

#[test]
fn invalid_malformed() {
    test_vector_directory("invalid", 22, 0);
}

#[test]
fn boundaries_depth_and_size() {
    test_vector_directory("boundaries", 10, 1);
}

#[test]
fn evm_head_only_layout() {
    test_vector_directory("evm", 10, 0);
}

#[test]
fn empty_calldata_golden() {
    let empty = Calldata::from_bytes(&[]).expect("empty");
    assert!(empty.payload().is_empty());
    assert_eq!(empty.as_bytes(), vec![0x01]);
}

#[test]
fn layerx_convention_frozen() {
    let mut calldata = Calldata::new();
    calldata.encode_u32(42).expect("encode");
    let bytes = calldata.as_bytes();
    assert_eq!(bytes, vec![0x01, 0x12, 0x00, 0x00, 0x00, 0x2a]);
    let decoded = Calldata::from_bytes(&bytes).expect("decode");
    assert_eq!(decoded.payload(), &[0x12, 0x00, 0x00, 0x00, 0x2a]);
}

#[test]
fn evm_convention_frozen() {
    let evm = Calldata::with_convention(
        layerx_programs_runtime::abi::codec::EncodingConvention::EvmHeadOnly,
    );
    let bytes = evm.as_bytes();
    assert_eq!(bytes, vec![0x02]);
}

#[test]
fn non_canonical_encoding_is_rejected() {
    let invalid_option = vec![0x01, 0x40, 0x03];
    assert_eq!(
        Calldata::from_bytes(&invalid_option).unwrap_err(),
        CodecError::InvalidOption
    );
}

#[test]
fn nesting_depth_limit_is_enforced() {
    let mut deep = vec![0x01];
    for _ in 0..17 {
        deep.extend_from_slice(&[0x30, 0x00, 0x00, 0x00, 0x01]);
    }
    deep.extend_from_slice(&[0x10, 0xff]);
    assert_eq!(
        Calldata::from_bytes(&deep).unwrap_err(),
        CodecError::NestingTooDeep
    );
}

#[test]
fn input_size_limit_is_enforced() {
    let oversized = vec![0u8; layerx_programs_runtime::abi::codec::MAX_CALLDATA_BYTES + 1];
    assert_eq!(
        Calldata::from_bytes(&oversized).unwrap_err(),
        CodecError::InputTooLarge
    );
}

#[test]
fn truncated_integer_is_rejected() {
    let truncated_u32 = vec![0x01, 0x12, 0x00, 0x00];
    assert_eq!(
        Calldata::from_bytes(&truncated_u32).unwrap_err(),
        CodecError::Truncated
    );
}

#[test]
fn invalid_type_tag_is_rejected() {
    let invalid = vec![0x01, 0xff];
    assert_eq!(
        Calldata::from_bytes(&invalid).unwrap_err(),
        CodecError::InvalidType
    );
}

#[test]
fn invalid_convention_tag_is_rejected() {
    let invalid = vec![0xff, 0x10, 0x42];
    assert_eq!(
        Calldata::from_bytes(&invalid).unwrap_err(),
        CodecError::InvalidConvention
    );
}

#[test]
fn evm_misalignment_is_rejected() {
    let misaligned = vec![0x02, 0x42];
    assert_eq!(
        Calldata::from_bytes(&misaligned).unwrap_err(),
        CodecError::NonCanonical
    );
}

#[test]
fn canonical_u8_roundtrip() {
    let mut calldata = Calldata::new();
    calldata.encode_u8(255).expect("encode");
    let bytes = calldata.as_bytes();
    let decoded = Calldata::from_bytes(&bytes).expect("decode");
    assert_eq!(decoded.payload(), &[0x10, 0xff]);
}

#[test]
fn canonical_bytes_roundtrip() {
    let mut calldata = Calldata::new();
    calldata.encode_bytes(b"test").expect("encode");
    let bytes = calldata.as_bytes();
    let decoded = Calldata::from_bytes(&bytes).expect("decode");
    assert_eq!(
        decoded.payload(),
        &[0x20, 0x00, 0x00, 0x00, 0x04, b't', b'e', b's', b't']
    );
}

#[test]
fn canonical_option_none_roundtrip() {
    let mut calldata = Calldata::new();
    calldata.encode_option_none().expect("encode");
    let bytes = calldata.as_bytes();
    let decoded = Calldata::from_bytes(&bytes).expect("decode");
    assert_eq!(decoded.payload(), &[0x40, 0x00]);
}

#[test]
fn canonical_option_some_roundtrip() {
    let mut calldata = Calldata::new();
    calldata.begin_option_some().expect("begin");
    calldata.encode_u8(42).expect("encode");
    let bytes = calldata.as_bytes();
    let decoded = Calldata::from_bytes(&bytes).expect("decode");
    assert_eq!(decoded.payload(), &[0x40, 0x01, 0x10, 0x2a]);
}

#[test]
fn canonical_union_roundtrip() {
    let mut calldata = Calldata::new();
    calldata.begin_union(7).expect("begin");
    calldata.encode_u32(999).expect("encode");
    let bytes = calldata.as_bytes();
    let decoded = Calldata::from_bytes(&bytes).expect("decode");
    assert_eq!(
        decoded.payload(),
        &[0x50, 0x00, 0x00, 0x00, 0x07, 0x12, 0x00, 0x00, 0x03, 0xe7]
    );
}

#[test]
fn canonical_fixed_array_roundtrip() {
    let mut calldata = Calldata::new();
    calldata.begin_fixed_array(2).expect("begin");
    calldata.encode_u8(1).expect("first");
    calldata.encode_u8(2).expect("second");
    let bytes = calldata.as_bytes();
    let decoded = Calldata::from_bytes(&bytes).expect("decode");
    assert_eq!(
        decoded.payload(),
        &[0x30, 0x00, 0x00, 0x00, 0x02, 0x10, 0x01, 0x10, 0x02]
    );
}

#[test]
fn canonical_variable_array_roundtrip() {
    let mut calldata = Calldata::new();
    calldata.begin_variable_array(1).expect("begin");
    calldata.encode_bytes(b"x").expect("element");
    let bytes = calldata.as_bytes();
    let decoded = Calldata::from_bytes(&bytes).expect("decode");
    assert_eq!(
        decoded.payload(),
        &[0x31, 0x00, 0x00, 0x00, 0x01, 0x20, 0x00, 0x00, 0x00, 0x01, b'x']
    );
}
