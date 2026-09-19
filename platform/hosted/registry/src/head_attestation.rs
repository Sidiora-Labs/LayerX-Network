//! Sequencer-signed program discovery proof for the hosted registry document.
//!
//! The registry asks the sequencer node, over its LNI socket or through the
//! node boundary's registry-plane relay of the same LNI messages, to attest the
//! program's current head under the sequencer key the independently verified
//! batch header already proved. The proof fields are attached to the registry
//! document only when every attested fact equals the fact the registry verified
//! for itself and the signature verifies over the recomputed
//! `LayerX/program-discovery-proof/v1` digest. Otherwise the document is
//! published without them; nothing is ever fabricated.

use std::path::Path;
use std::time::Instant;

use layerx_client::lni::handshake::{perform, HandshakeConfig};
use layerx_client::lni::head_attestation::{
    attest_program_head, decode_program_head_attestation, program_discovery_proof_digest,
    ProgramDiscoveryHead, ProgramHeadAttestContext, ProgramHeadAttestation,
    PROGRAM_HEAD_ATTEST_PAYLOAD_BYTES, PROGRAM_HEAD_ATTEST_PROOF_BYTES,
};
use layerx_client::lni::schema::{Capability, Version};
use layerx_client::lni::transport::{ConnectionGate, Limits, Uds};
use layerx_programs::hex;
use serde_json::Value;

use crate::node_state::{HeadAuthority, NodeProgramStateSource};

const FRAME_BYTES: usize = 1_212_416;
const CORRELATION_ID: u64 = 1;

/// The head and latest-version facts the registry verified independently.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ExpectedDiscoveryHead {
    pub head: ProgramDiscoveryHead,
    pub head_receipt_digest: [u8; 32],
}

/// The discovery proof fields the registry document publishes.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct DiscoveryProofFields {
    pub receipt_digest: [u8; 32],
    pub public_key: [u8; 32],
    pub signature: [u8; 64],
}

/// Requests the sequencer's attestation of one program at its current head.
///
/// # Errors
/// Refuses an unreachable socket, a node that does not advertise
/// `program_head_attest`, a node authorised under a different sequencer key
/// than the independently verified one, and every attestation refusal.
pub fn request_head_attestation(
    socket: &Path,
    program_id: [u8; 32],
    staleness_ms: u64,
    authority: &HeadAuthority,
    deadline: Instant,
) -> Result<ProgramHeadAttestation, String> {
    let remaining = deadline
        .checked_duration_since(Instant::now())
        .filter(|value| !value.is_zero())
        .ok_or_else(|| "head attestation unavailable at request deadline".to_owned())?;
    let mut transport = Uds::connect(
        socket,
        &ConnectionGate::new(1),
        Limits {
            maximum_frame_bytes: FRAME_BYTES,
            maximum_connections: 1,
            maximum_streams: 1,
            maximum_queued_bytes: FRAME_BYTES,
            deadline: remaining,
        },
    )
    .map_err(|error| format!("head attestation LNI unavailable: {error:?}"))?;
    let config = HandshakeConfig {
        built_interface_version: Version::V1_7,
        expected_protocol_version: authority.protocol_version,
        expected_network_id: authority.network_id,
    };
    let handshake = perform(&mut transport, &config, None)
        .map_err(|error| format!("head attestation handshake refused: {error:?}"))?;
    if !handshake
        .capabilities()
        .contains(Capability::ProgramHeadAttest)
    {
        return Err("node does not advertise program_head_attest".to_owned());
    }
    if handshake.node().authorised_sequencer_key != authority.sequencer_public_key {
        return Err(
            "node is authorised under a different sequencer key than the verified head".to_owned(),
        );
    }
    attest_program_head(
        &mut transport,
        ProgramHeadAttestContext {
            interface_version: handshake.node().interface_version,
            sequencer_public_key: authority.sequencer_public_key,
            correlation_id: CORRELATION_ID,
            program_id,
            staleness_ms,
        },
    )
    .map_err(|error| format!("head attestation refused: {error:?}"))
}

/// Decodes one attestation the node boundary relayed from the sequencer and
/// verifies it under the independently verified sequencer key. The boundary
/// carries the node's LNI response bytes and holds no signing key, so a
/// document it altered or invented fails this verification.
///
/// # Errors
/// Refuses a malformed relay document, a different program or freshness
/// bound, a key other than the verified sequencer key and a signature that
/// does not verify over the recomputed discovery proof digest.
pub fn decode_relayed_head_attestation(
    document: &Value,
    program_id: [u8; 32],
    staleness_ms: u64,
    authority: &HeadAuthority,
) -> Result<ProgramHeadAttestation, String> {
    let part = |name: &str, length: usize| {
        let text = document[name]
            .as_str()
            .filter(|text| text.len() == length * 2)
            .ok_or_else(|| format!("relayed head attestation omitted {name}"))?;
        hex::decode(text).map_err(|error| format!("relayed head attestation {name}: {error}"))
    };
    let payload = part("payload_hex", PROGRAM_HEAD_ATTEST_PAYLOAD_BYTES)?;
    let proof = part("proof_hex", PROGRAM_HEAD_ATTEST_PROOF_BYTES)?;
    decode_program_head_attestation(
        &payload,
        &proof,
        &program_id,
        staleness_ms,
        &authority.sequencer_public_key,
    )
    .map_err(|error| format!("relayed head attestation refused: {error:?}"))
}

/// Requests the sequencer's attestation of the verified head over the local
/// LNI socket when one is configured and through the node boundary's
/// registry-plane relay otherwise, then derives the publishable proof fields.
///
/// # Errors
/// Refuses an unavailable node or boundary, every attestation refusal and
/// every attested fact that differs from what the registry verified.
pub fn verified_discovery_proof(
    socket: Option<&Path>,
    node_state: &NodeProgramStateSource,
    expected: &ExpectedDiscoveryHead,
    staleness_ms: u64,
    authority: &HeadAuthority,
) -> Result<DiscoveryProofFields, String> {
    let attestation = match socket {
        Some(socket) => request_head_attestation(
            socket,
            expected.head.program_id,
            staleness_ms,
            authority,
            node_state
                .request_deadline()
                .ok_or_else(|| "the registry request deadline is unavailable".to_owned())?,
        )?,
        None => node_state.relayed_head_attestation(
            expected.head.program_id,
            staleness_ms,
            authority,
        )?,
    };
    discovery_proof_fields(&attestation, expected, authority)
}

/// Derives the publishable proof fields from one attestation, refusing any
/// attested fact that differs from what the registry verified for itself.
///
/// # Errors
/// Refuses a different head, program record, head receipt or key, a digest
/// other than the discovery proof digest of the verified facts, and a
/// signature that does not verify under the verified sequencer key.
pub fn discovery_proof_fields(
    attestation: &ProgramHeadAttestation,
    expected: &ExpectedDiscoveryHead,
    authority: &HeadAuthority,
) -> Result<DiscoveryProofFields, String> {
    if attestation.head != expected.head {
        return Err("attested program head differs from the verified head".to_owned());
    }
    if attestation.head_receipt_digest != expected.head_receipt_digest {
        return Err("attested head receipt differs from the verified head receipt".to_owned());
    }
    if attestation.public_key != authority.sequencer_public_key {
        return Err("attestation key differs from the verified sequencer key".to_owned());
    }
    let digest = program_discovery_proof_digest(&expected.head);
    if attestation.digest != digest {
        return Err("attestation digest differs from the discovery proof digest".to_owned());
    }
    layerx_crypto::ed25519::verify_digest(
        &authority.sequencer_public_key,
        &attestation.signature,
        &digest,
    )
    .map_err(|error| format!("attestation signature refused: {error:?}"))?;
    Ok(DiscoveryProofFields {
        receipt_digest: digest,
        public_key: authority.sequencer_public_key,
        signature: attestation.signature,
    })
}

/// Attaches the proof fields to one registry document.
///
/// # Errors
/// Refuses a document that is not a JSON object.
pub fn attach_discovery_proof(
    document: &mut Value,
    fields: &DiscoveryProofFields,
) -> Result<(), String> {
    let object = document
        .as_object_mut()
        .ok_or_else(|| "registry document is not an object".to_owned())?;
    object.insert(
        "receipt_digest".to_owned(),
        Value::String(hex::encode(&fields.receipt_digest)),
    );
    object.insert(
        "discovery_public_key".to_owned(),
        Value::String(hex::encode(&fields.public_key)),
    );
    object.insert(
        "discovery_signature".to_owned(),
        Value::String(hex::encode(&fields.signature)),
    );
    Ok(())
}

#[cfg(test)]
mod tests {
    use ed25519_dalek::{Signer as _, SigningKey};
    use layerx_client::lni::head_attestation::{
        decode_program_head_attestation, encode_program_head_attestation,
        program_discovery_proof_digest, ProgramDiscoveryHead, ProgramHeadAttestError,
        ProgramHeadAttestation,
    };
    use layerx_programs::hex;
    use serde_json::json;

    use super::{
        attach_discovery_proof, discovery_proof_fields, DiscoveryProofFields, ExpectedDiscoveryHead,
    };
    use crate::node_state::HeadAuthority;

    const STALENESS_MS: u64 = 60_000;

    fn verified_head() -> ExpectedDiscoveryHead {
        ExpectedDiscoveryHead {
            head: ProgramDiscoveryHead {
                program_id: [0x11; 32],
                version: 3,
                code_hash: [0x22; 32],
                abi_version: 2,
                observed_sequence: 4_242,
                observed_at: 1_758_200_000_000,
                valid_through: 1_758_200_000_000 + STALENESS_MS,
                state_root: [0x33; 32],
            },
            head_receipt_digest: [0x44; 32],
        }
    }

    fn signed(expected: &ExpectedDiscoveryHead, key: &SigningKey) -> ProgramHeadAttestation {
        let digest = program_discovery_proof_digest(&expected.head);
        ProgramHeadAttestation {
            head: expected.head,
            head_receipt_digest: expected.head_receipt_digest,
            digest,
            public_key: key.verifying_key().to_bytes(),
            signature: key.sign(&digest).to_bytes(),
        }
    }

    fn fixture() -> (ExpectedDiscoveryHead, HeadAuthority, ProgramHeadAttestation) {
        let key = SigningKey::from_bytes(&[0x31; 32]);
        let expected = verified_head();
        let authority = HeadAuthority {
            sequencer_public_key: key.verifying_key().to_bytes(),
            protocol_version: 3,
            network_id: 7_332,
        };
        (expected, authority, signed(&expected, &key))
    }

    fn decoded(
        attestation: &ProgramHeadAttestation,
        expected: &ExpectedDiscoveryHead,
        authority: &HeadAuthority,
    ) -> Result<ProgramHeadAttestation, ProgramHeadAttestError> {
        let (payload, proof) = encode_program_head_attestation(attestation);
        decode_program_head_attestation(
            &payload,
            &proof,
            &expected.head.program_id,
            STALENESS_MS,
            &authority.sequencer_public_key,
        )
    }

    #[test]
    fn signed_fixture_is_published_with_the_proof_fields() {
        let (expected, authority, attestation) = fixture();
        let received = decoded(&attestation, &expected, &authority)
            .unwrap_or_else(|error| panic!("client decode: {error:?}"));
        assert_eq!(received, attestation);
        let fields = discovery_proof_fields(&received, &expected, &authority)
            .unwrap_or_else(|error| panic!("proof fields: {error}"));
        assert_eq!(
            fields,
            DiscoveryProofFields {
                receipt_digest: program_discovery_proof_digest(&expected.head),
                public_key: authority.sequencer_public_key,
                signature: attestation.signature,
            }
        );
        let mut document = json!({
            "program_id": hex::encode(&expected.head.program_id),
            "state_root": hex::encode(&expected.head.state_root),
            "latest_version": expected.head.version,
        });
        attach_discovery_proof(&mut document, &fields)
            .unwrap_or_else(|error| panic!("attach: {error}"));
        assert_eq!(
            document["receipt_digest"],
            json!(hex::encode(&fields.receipt_digest))
        );
        assert_eq!(
            document["discovery_public_key"],
            json!(hex::encode(&authority.sequencer_public_key))
        );
        assert_eq!(
            document["discovery_signature"],
            json!(hex::encode(&attestation.signature))
        );
        assert_eq!(
            document["discovery_signature"].as_str().map(str::len),
            Some(128)
        );
        assert_eq!(document["latest_version"], json!(3));
        assert!(attach_discovery_proof(&mut json!([]), &fields).is_err());
    }

    #[test]
    fn tampered_head_is_refused() {
        let (expected, authority, attestation) = fixture();
        let mut other_root = expected;
        other_root.head.state_root[0] ^= 1;
        assert!(discovery_proof_fields(&attestation, &other_root, &authority).is_err());
        let mut other_version = expected;
        other_version.head.version += 1;
        assert!(discovery_proof_fields(&attestation, &other_version, &authority).is_err());
        let mut other_valid_through = expected;
        other_valid_through.head.valid_through += 1;
        assert!(discovery_proof_fields(&attestation, &other_valid_through, &authority).is_err());
        let mut other_receipt = expected;
        other_receipt.head_receipt_digest[0] ^= 1;
        assert!(discovery_proof_fields(&attestation, &other_receipt, &authority).is_err());

        let mut resigned_root = attestation;
        resigned_root.head.state_root[0] ^= 1;
        assert_eq!(
            decoded(&resigned_root, &expected, &authority),
            Err(ProgramHeadAttestError::Signature)
        );
        let mut resigned_sequence = attestation;
        resigned_sequence.head.observed_sequence += 1;
        assert_eq!(
            decoded(&resigned_sequence, &expected, &authority),
            Err(ProgramHeadAttestError::Signature)
        );
        let mut unbound = attestation;
        unbound.head.valid_through += 1;
        assert_eq!(
            decoded(&unbound, &expected, &authority),
            Err(ProgramHeadAttestError::FreshnessBinding)
        );

        let mut forged_digest = attestation;
        forged_digest.digest[0] ^= 1;
        assert!(discovery_proof_fields(&forged_digest, &expected, &authority).is_err());
        let mut forged_signature = attestation;
        forged_signature.signature[0] ^= 1;
        assert!(discovery_proof_fields(&forged_signature, &expected, &authority).is_err());
        let mut moved_head = attestation;
        moved_head.head.observed_at += 1;
        moved_head.head.valid_through += 1;
        moved_head.digest = program_discovery_proof_digest(&moved_head.head);
        assert!(discovery_proof_fields(&moved_head, &expected, &authority).is_err());
        let mut moved_expected = expected;
        moved_expected.head = moved_head.head;
        assert!(discovery_proof_fields(&moved_head, &moved_expected, &authority).is_err());
    }

    #[test]
    fn foreign_sequencer_key_is_refused() {
        let (expected, authority, attestation) = fixture();
        let foreign = SigningKey::from_bytes(&[0x32; 32]);
        let foreign_authority = HeadAuthority {
            sequencer_public_key: foreign.verifying_key().to_bytes(),
            ..authority
        };
        assert!(discovery_proof_fields(&attestation, &expected, &foreign_authority).is_err());
        assert_eq!(
            decoded(&attestation, &expected, &foreign_authority),
            Err(ProgramHeadAttestError::SequencerKeyMismatch)
        );
        let foreign_attestation = signed(&expected, &foreign);
        assert!(discovery_proof_fields(&foreign_attestation, &expected, &authority).is_err());
        assert_eq!(
            decoded(&foreign_attestation, &expected, &authority),
            Err(ProgramHeadAttestError::SequencerKeyMismatch)
        );
        let mut relabelled = foreign_attestation;
        relabelled.public_key = authority.sequencer_public_key;
        assert!(discovery_proof_fields(&relabelled, &expected, &authority).is_err());
        assert_eq!(
            decoded(&relabelled, &expected, &authority),
            Err(ProgramHeadAttestError::Signature)
        );
    }
}
