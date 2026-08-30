use std::collections::BTreeSet;

use layerx_client::lni::schema::{
    encode_envelope, lni_golden_vectors, lni_schema_v1, Envelope, Version, LNI_V1_SOURCE,
};

const NODE_BOUNDARY: &str =
    include_str!("../../../../spec/layerx-agent-interface/docs/node-boundary.md");

fn hex(value: &str) -> Vec<u8> {
    value
        .as_bytes()
        .chunks_exact(2)
        .map(|digits| {
            let text = std::str::from_utf8(digits)
                .unwrap_or_else(|error| panic!("invalid UTF-8 in golden: {error}"));
            u8::from_str_radix(text, 16)
                .unwrap_or_else(|error| panic!("invalid golden hex: {error}"))
        })
        .collect()
}

#[test]
fn lni_schema_and_document_cover_every_declared_message() {
    let schema = lni_schema_v1();
    assert_eq!(schema.version, Version::V1_1);
    assert_eq!(schema.messages.len(), lni_golden_vectors().len());
    let mut tags = BTreeSet::new();
    for message in schema.messages {
        assert!(tags.insert(message.tag));
        assert!(LNI_V1_SOURCE.contains(&format!("name = \"{}\"", message.name)));
        assert!(NODE_BOUNDARY.contains(&format!("`{}`", message.name)));
        assert!(schema.capabilities.contains(&message.capability));
    }
}

#[test]
fn lni_golden_vectors_are_literal_and_canonical_for_every_message() {
    let schema = lni_schema_v1();
    for (message, golden) in schema.messages.iter().zip(lni_golden_vectors()) {
        assert_eq!(message.name, golden.message);
        assert!(LNI_V1_SOURCE.contains(golden.encoded_hex));
        assert_eq!(
            encode_envelope(Envelope {
                version: golden.version(),
                message_tag: message.tag,
                correlation_id: 0,
                canonical_payload: golden.payload,
                proof_material: golden.proof_material,
            }),
            Ok(hex(golden.encoded_hex))
        );
    }
}

#[test]
fn version_and_capability_rules_are_checked_against_the_schema_source() {
    assert!(Version::V1_0.is_compatible_with(Version {
        major: 1,
        minor: 99
    }));
    assert!(!Version::V1_0.is_compatible_with(Version { major: 2, minor: 0 }));
    assert!(LNI_V1_SOURCE.contains("minor releases may add only"));
    assert!(LNI_V1_SOURCE.contains("opaque canonical LayerX bytes"));
    assert!(LNI_V1_SOURCE.contains("availability_fetch"));
    assert!(LNI_V1_SOURCE.contains("historical_proofs"));
    assert!(LNI_V1_SOURCE.contains("preparation_state"));
}
