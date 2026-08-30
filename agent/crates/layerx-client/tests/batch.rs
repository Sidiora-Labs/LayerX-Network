use std::collections::VecDeque;

use ed25519_dalek::{Signer as _, SigningKey};
use layerx_client::batch::{lookup, BatchHeaderError};
use layerx_client::lni::schema::{decode_envelope, encode_envelope, Envelope, Version};
use layerx_client::lni::transport::{FrameTransport, TransportError};
use layerx_wire::encode::Encoder;
use layerx_wire::hash::batch_header_digest;

struct Scripted {
    sent: Vec<Vec<u8>>,
    responses: VecDeque<Vec<u8>>,
}

impl FrameTransport for Scripted {
    fn send(&mut self, frame: &[u8]) -> Result<(), TransportError> {
        self.sent.push(frame.to_vec());
        Ok(())
    }

    fn receive(&mut self) -> Result<Vec<u8>, TransportError> {
        self.responses
            .pop_front()
            .ok_or(TransportError::PeerShutdown)
    }
}

fn header(sequencer: [u8; 32]) -> Vec<u8> {
    let mut e = Encoder::new(354);
    assert!(e.structure_header(0x1701).is_ok());
    assert!(e.u8(15).is_ok());
    assert!(e.tag(1, 15).is_ok());
    assert!(e.u16(1).is_ok());
    assert!(e.tag(2, 15).is_ok());
    assert!(e.u32(77).is_ok());
    for (tag, value) in [(3, 2), (4, 7), (5, 10), (6, 12)] {
        assert!(e.tag(tag, 15).is_ok());
        assert!(e.u64(value).is_ok());
    }
    for (tag, value) in [
        (7, [1; 32]),
        (8, [2; 32]),
        (9, [3; 32]),
        (10, [4; 32]),
        (11, [5; 32]),
        (12, [6; 32]),
        (13, [7; 32]),
    ] {
        assert!(e.tag(tag, 15).is_ok());
        assert!(e.bytes(&value, 32).is_ok());
    }
    assert!(e.tag(14, 15).is_ok());
    assert!(e.u64(1_000).is_ok());
    assert!(e.tag(15, 15).is_ok());
    assert!(e.bytes(&sequencer, 32).is_ok());
    e.finish()
}

fn response(key: &SigningKey, corrupt_signature: bool) -> Vec<u8> {
    let public = key.verifying_key().to_bytes();
    let header = header(public);
    let digest = batch_header_digest(&header).expect("header digest");
    let mut signature = key.sign(&digest).to_bytes();
    if corrupt_signature {
        signature[0] ^= 1;
    }
    let mut proof = Vec::with_capacity(146);
    proof.extend_from_slice(&1_u16.to_be_bytes());
    proof.extend_from_slice(&public);
    proof.extend_from_slice(&public);
    proof.extend_from_slice(&7_u64.to_be_bytes());
    proof.extend_from_slice(&9_u64.to_be_bytes());
    proof.extend_from_slice(&signature);
    encode_envelope(Envelope {
        version: Version::V1_1,
        message_tag: 13,
        correlation_id: 44,
        canonical_payload: &header,
        proof_material: &proof,
    })
    .expect("response")
}

#[test]
fn verifies_signed_canonical_batch_header_and_selector() {
    let key = SigningKey::from_bytes(&[0x41; 32]);
    let public = key.verifying_key().to_bytes();
    let mut transport = Scripted {
        sent: Vec::new(),
        responses: VecDeque::from([response(&key, false)]),
    };
    let result = lookup(&mut transport, Version::V1_1, 7, 44, public).expect("verified header");
    assert_eq!(result.header.batch_number(), 7);
    let request = decode_envelope(&transport.sent[0]).expect("request");
    assert_eq!(request.message_tag, 12);
    assert_eq!(
        request.canonical_payload,
        [&1_u16.to_be_bytes()[..], &7_u64.to_be_bytes()[..]].concat()
    );
}

#[test]
fn refuses_invalid_signature_and_absence() {
    let key = SigningKey::from_bytes(&[0x42; 32]);
    let public = key.verifying_key().to_bytes();
    let mut invalid = Scripted {
        sent: Vec::new(),
        responses: VecDeque::from([response(&key, true)]),
    };
    assert_eq!(
        lookup(&mut invalid, Version::V1_1, 7, 44, public),
        Err(BatchHeaderError::Signature)
    );

    let absent = encode_envelope(Envelope {
        version: Version::V1_1,
        message_tag: 13,
        correlation_id: 44,
        canonical_payload: &[],
        proof_material: &[],
    })
    .expect("absence");
    let mut missing = Scripted {
        sent: Vec::new(),
        responses: VecDeque::from([absent]),
    };
    assert_eq!(
        lookup(&mut missing, Version::V1_1, 7, 44, public),
        Err(BatchHeaderError::Missing)
    );
}
