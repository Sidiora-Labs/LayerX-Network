use std::collections::BTreeSet;

use layerx_client::lni::schema::{
    encode_envelope, lni_golden_vectors, lni_schema_v1, Capability, Envelope, Version,
    LNI_V1_SOURCE,
};

const NODE_BOUNDARY: &str = include_str!("../../../schema/lni/README.md");

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
    assert_eq!(schema.version, Version::V1_7);
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
    assert!(LNI_V1_SOURCE.contains("authenticated_durable_submit"));
    assert!(LNI_V1_SOURCE.contains("simulate"));
    assert!(LNI_V1_SOURCE.contains("program_read"));
    assert_eq!(Capability::ProgramRead.name(), "program_read");
    assert!(LNI_V1_SOURCE.contains("program_head_attest"));
    assert_eq!(Capability::ProgramHeadAttest.name(), "program_head_attest");
    assert!(Version::V1_4.is_compatible_with(Version::V1_0));
    assert_eq!(Capability::Simulate.name(), "simulate");
    assert!(Version::V1_3.is_compatible_with(Version::V1_0));
    assert_eq!(
        Capability::AuthenticatedDurableSubmit.name(),
        "authenticated_durable_submit"
    );
    assert_eq!(
        encode_envelope(Envelope {
            version: Version::V1_3,
            message_tag: 2,
            correlation_id: 0,
            canonical_payload: b"authenticated_durable_submit",
            proof_material: &[],
        }),
        Ok(hex(
            "00010003000200000000000000000000001c61757468656e746963617465645f64757261626c655f7375626d697400000000"
        ))
    );
}

#[test]
fn additive_read_tags_are_complete_and_unknown_tags_still_refuse() {
    use layerx_client::lni::schema::{decode_envelope, SchemaError};
    for tag in [36, 37, 38, 39, 40, 41] {
        let entry = lni_schema_v1()
            .messages
            .iter()
            .find(|message| message.tag == tag)
            .unwrap_or_else(|| panic!("additive read schema missing"));
        assert_eq!(
            entry.capability,
            if tag < 38 {
                Capability::SessionFeeState
            } else if tag < 40 {
                Capability::ProgramRead
            } else {
                Capability::ProgramHeadAttest
            }
        );
        let encoded = encode_envelope(Envelope {
            version: Version::V1_7,
            message_tag: tag,
            correlation_id: 7,
            canonical_payload: &[0, 1],
            proof_material: &[],
        })
        .unwrap_or_else(|error| panic!("session state encoding: {error:?}"));
        assert_eq!(
            decode_envelope(&encoded).map(|message| message.message_tag),
            Ok(tag)
        );
    }
    for tag in [0, 42, u16::MAX] {
        assert_eq!(
            encode_envelope(Envelope {
                version: Version::V1_7,
                message_tag: tag,
                correlation_id: 7,
                canonical_payload: &[],
                proof_material: &[]
            }),
            Err(SchemaError::UnknownMessage(tag))
        );
        let mut encoded = hex("00010007002400000000000000070000000000000000");
        encoded[4..6].copy_from_slice(&tag.to_be_bytes());
        assert_eq!(
            decode_envelope(&encoded),
            Err(SchemaError::UnknownMessage(tag))
        );
    }
}
