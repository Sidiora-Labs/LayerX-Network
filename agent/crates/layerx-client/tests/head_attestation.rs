use layerx_client::lni::head_attestation::{
    attest_program_head, decode_program_head_attestation, encode_program_head_attestation,
    program_discovery_proof_digest, ProgramDiscoveryHead, ProgramHeadAttestContext,
    ProgramHeadAttestError, ProgramHeadAttestation, PROGRAM_HEAD_ATTEST_PAYLOAD_BYTES,
    PROGRAM_HEAD_ATTEST_PROOF_BYTES, PROGRAM_HEAD_ATTEST_REQUEST_TAG,
    PROGRAM_HEAD_ATTEST_RESPONSE_TAG,
};
use layerx_client::lni::schema::{decode_envelope, encode_envelope, Envelope, Version};
use layerx_client::lni::transport::{FrameTransport, TransportError};
use layerx_types::result::KnownResult;

const VECTOR: &str = include_str!("../../../../tests/vectors/native-head-attestation.json");

fn field(name: &str) -> &'static str {
    let marker = format!("\"{name}\":\"");
    let (_, rest) = VECTOR
        .split_once(&marker)
        .unwrap_or_else(|| panic!("missing vector field {name}"));
    rest.split('"')
        .next()
        .unwrap_or_else(|| panic!("unterminated vector field {name}"))
}

fn bytes(name: &str) -> Vec<u8> {
    let text = field(name)
        .strip_prefix("0x")
        .unwrap_or_else(|| panic!("vector field {name} is not hex"));
    text.as_bytes()
        .chunks_exact(2)
        .map(|pair| {
            let digits = std::str::from_utf8(pair)
                .unwrap_or_else(|error| panic!("vector field is not UTF-8: {error}"));
            u8::from_str_radix(digits, 16)
                .unwrap_or_else(|error| panic!("vector field is not hex: {error}"))
        })
        .collect()
}

fn fixed<const N: usize>(name: &str) -> [u8; N] {
    bytes(name)
        .try_into()
        .unwrap_or_else(|_| panic!("vector field {name} is not {N} bytes"))
}

fn number(name: &str) -> u64 {
    field(name)
        .parse()
        .unwrap_or_else(|error| panic!("vector field {name} is not a number: {error}"))
}

fn vector_head() -> ProgramDiscoveryHead {
    ProgramDiscoveryHead {
        program_id: fixed("program_id"),
        version: u32::try_from(number("version")).unwrap_or_else(|_| panic!("version")),
        code_hash: fixed("code_hash"),
        abi_version: u16::try_from(number("abi_version")).unwrap_or_else(|_| panic!("abi")),
        observed_sequence: number("observed_sequence"),
        observed_at: number("observed_at"),
        valid_through: number("valid_through"),
        state_root: fixed("state_root"),
    }
}

fn vector_attestation() -> ProgramHeadAttestation {
    ProgramHeadAttestation {
        head: vector_head(),
        head_receipt_digest: fixed("head_receipt_digest"),
        digest: fixed("digest"),
        public_key: fixed("sequencer_public_key"),
        signature: fixed("signature"),
    }
}

fn staleness() -> u64 {
    number("valid_through") - number("observed_at")
}

struct Scripted {
    response: Option<Vec<u8>>,
    sent: Vec<Vec<u8>>,
    receives: u8,
}

impl Scripted {
    fn new(response: Vec<u8>) -> Self {
        Self {
            response: Some(response),
            sent: Vec::new(),
            receives: 0,
        }
    }
}

impl FrameTransport for Scripted {
    fn send(&mut self, canonical_envelope: &[u8]) -> Result<(), TransportError> {
        self.sent.push(canonical_envelope.to_vec());
        Ok(())
    }

    fn receive(&mut self) -> Result<Vec<u8>, TransportError> {
        self.receives = self.receives.saturating_add(1);
        self.response.take().ok_or(TransportError::PeerShutdown)
    }
}

fn response(payload: &[u8], proof: &[u8], correlation_id: u64) -> Vec<u8> {
    encode_envelope(Envelope {
        version: Version::V1_7,
        message_tag: PROGRAM_HEAD_ATTEST_RESPONSE_TAG,
        correlation_id,
        canonical_payload: payload,
        proof_material: proof,
    })
    .unwrap_or_else(|error| panic!("attestation response failed: {error:?}"))
}

fn context(correlation_id: u64) -> ProgramHeadAttestContext {
    ProgramHeadAttestContext {
        interface_version: Version::V1_7,
        sequencer_public_key: fixed("sequencer_public_key"),
        correlation_id,
        program_id: fixed("program_id"),
        staleness_ms: staleness(),
    }
}

#[test]
fn native_vector_digest_layout_matches_the_discovery_proof() {
    let head = vector_head();
    assert_eq!(program_discovery_proof_digest(&head), fixed::<32>("digest"));
    let (payload, proof) = encode_program_head_attestation(&vector_attestation());
    assert_eq!(payload.to_vec(), bytes("payload"));
    assert_eq!(proof.to_vec(), bytes("proof"));
    assert_eq!(payload.len(), PROGRAM_HEAD_ATTEST_PAYLOAD_BYTES);
    assert_eq!(proof.len(), PROGRAM_HEAD_ATTEST_PROOF_BYTES);
}

#[test]
fn native_vector_attestation_verifies_and_request_is_exact() {
    let correlation_id = 91;
    let mut transport = Scripted::new(response(&bytes("payload"), &bytes("proof"), correlation_id));
    let attestation = attest_program_head(&mut transport, context(correlation_id))
        .unwrap_or_else(|error| panic!("attestation failed: {error:?}"));
    assert_eq!(attestation, vector_attestation());
    assert_eq!(transport.sent.len(), 1);
    assert_eq!(transport.receives, 1);
    let request = decode_envelope(&transport.sent[0])
        .unwrap_or_else(|error| panic!("request envelope failed: {error:?}"));
    assert_eq!(request.message_tag, PROGRAM_HEAD_ATTEST_REQUEST_TAG);
    assert_eq!(request.version, Version::V1_7);
    assert_eq!(request.correlation_id, correlation_id);
    assert!(request.proof_material.is_empty());
    assert_eq!(request.canonical_payload.len(), 42);
    assert_eq!(&request.canonical_payload[..2], &1_u16.to_be_bytes());
    assert_eq!(
        &request.canonical_payload[2..34],
        &fixed::<32>("program_id")
    );
    assert_eq!(&request.canonical_payload[34..], &staleness().to_be_bytes());
}

#[test]
fn tampered_head_key_or_binding_is_refused() {
    let payload = bytes("payload");
    let proof = bytes("proof");
    let program_id = fixed::<32>("program_id");
    let key = fixed::<32>("sequencer_public_key");
    for (offset, expected) in [
        (34, ProgramHeadAttestError::Signature),
        (38, ProgramHeadAttestError::Signature),
        (70, ProgramHeadAttestError::Signature),
        (72, ProgramHeadAttestError::Signature),
        (80, ProgramHeadAttestError::FreshnessBinding),
        (88, ProgramHeadAttestError::FreshnessBinding),
        (96, ProgramHeadAttestError::Signature),
        (2, ProgramHeadAttestError::ProgramMismatch),
    ] {
        let mut changed = payload.clone();
        changed[offset] ^= 1;
        assert_eq!(
            decode_program_head_attestation(&changed, &proof, &program_id, staleness(), &key),
            Err(expected),
            "offset {offset}"
        );
    }
    let mut receipt_changed = payload.clone();
    receipt_changed[128] ^= 1;
    let attestation =
        decode_program_head_attestation(&receipt_changed, &proof, &program_id, staleness(), &key)
            .unwrap_or_else(|error| panic!("receipt digest is outside the proof: {error:?}"));
    assert_ne!(
        attestation.head_receipt_digest,
        fixed::<32>("head_receipt_digest")
    );
    let mut signature_changed = proof.clone();
    signature_changed[40] ^= 1;
    assert_eq!(
        decode_program_head_attestation(
            &payload,
            &signature_changed,
            &program_id,
            staleness(),
            &key
        ),
        Err(ProgramHeadAttestError::Signature)
    );
    let mut key_changed = proof.clone();
    key_changed[0] ^= 1;
    assert_eq!(
        decode_program_head_attestation(&payload, &key_changed, &program_id, staleness(), &key),
        Err(ProgramHeadAttestError::SequencerKeyMismatch)
    );
    let mut foreign = key;
    foreign[0] ^= 1;
    assert_eq!(
        decode_program_head_attestation(&payload, &proof, &program_id, staleness(), &foreign),
        Err(ProgramHeadAttestError::SequencerKeyMismatch)
    );
    assert_eq!(
        decode_program_head_attestation(&payload, &proof, &program_id, staleness() + 1, &key),
        Err(ProgramHeadAttestError::FreshnessBinding)
    );
    assert_eq!(
        decode_program_head_attestation(&payload[..159], &proof, &program_id, staleness(), &key),
        Err(ProgramHeadAttestError::MalformedResponse)
    );
    let mut transport = Scripted::new(response(&payload, &proof, 5));
    assert_eq!(
        attest_program_head(
            &mut transport,
            ProgramHeadAttestContext {
                interface_version: Version::V1_6,
                ..context(5)
            }
        ),
        Err(ProgramHeadAttestError::InterfaceVersion(Version::V1_6))
    );
    assert!(transport.sent.is_empty());
}

#[test]
fn typed_refusals_are_preserved_without_retrying() {
    for (raw, expected) in [
        (
            KnownResult::ProjectionStale.raw(),
            ProgramHeadAttestError::HeadStale,
        ),
        (
            KnownResult::UnknownField.raw(),
            ProgramHeadAttestError::UnknownProgram,
        ),
    ] {
        let correlation_id = u64::from(raw.unsigned_abs());
        let mut refusal = vec![4];
        refusal.extend_from_slice(&raw.to_be_bytes());
        let response = encode_envelope(Envelope {
            version: Version::V1_7,
            message_tag: 25,
            correlation_id,
            canonical_payload: &refusal,
            proof_material: &[],
        })
        .unwrap_or_else(|error| panic!("refusal envelope failed: {error:?}"));
        let mut transport = Scripted::new(response);
        assert_eq!(
            attest_program_head(&mut transport, context(correlation_id)),
            Err(expected)
        );
        assert_eq!(transport.sent.len(), 1);
        assert_eq!(transport.receives, 1);
    }
    let raw = KnownResult::ModuleDisabled.raw();
    let mut refusal = vec![3];
    refusal.extend_from_slice(&raw.to_be_bytes());
    let response = encode_envelope(Envelope {
        version: Version::V1_7,
        message_tag: 25,
        correlation_id: 8,
        canonical_payload: &refusal,
        proof_material: &[],
    })
    .unwrap_or_else(|error| panic!("refusal envelope failed: {error:?}"));
    let mut transport = Scripted::new(response);
    assert_eq!(
        attest_program_head(&mut transport, context(8)),
        Err(ProgramHeadAttestError::CoreRefusal {
            class: 3,
            result: layerx_types::result::ResultCode::from_raw(raw),
        })
    );
}
