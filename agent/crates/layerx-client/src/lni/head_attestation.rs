//! Sequencer-signed program head attestation over LNI v1.7.

use layerx_crypto::ed25519;
use layerx_types::result::{KnownResult, ResultCode};
use sha2::{Digest as _, Sha256};

use super::refusal::decode_core_refusal;
use super::schema::{decode_envelope, encode_envelope, Envelope, SchemaError, Version};
use super::transport::{FrameTransport, TransportError};

/// Tag naming one registered program and a staleness bound.
pub const PROGRAM_HEAD_ATTEST_REQUEST_TAG: u16 = 40;
/// Tag carrying the signed current head and program record.
pub const PROGRAM_HEAD_ATTEST_RESPONSE_TAG: u16 = 41;
const ERROR_RESPONSE_TAG: u16 = 25;
const PROGRAM_HEAD_ATTEST_VERSION: u16 = 1;
/// Exact request payload length.
pub const PROGRAM_HEAD_ATTEST_REQUEST_BYTES: usize = 2 + 32 + 8;
/// Exact response payload length.
pub const PROGRAM_HEAD_ATTEST_PAYLOAD_BYTES: usize = 2 + 32 + 4 + 32 + 2 + 8 + 8 + 8 + 32 + 32;
/// Exact response proof-material length.
pub const PROGRAM_HEAD_ATTEST_PROOF_BYTES: usize = 32 + 64;
/// Domain of the program discovery proof digest. Identical to the CLI, the
/// emulator and the gateway.
pub const PROGRAM_DISCOVERY_PROOF_DOMAIN: &[u8] = b"LayerX/program-discovery-proof/v1\0";

/// Request identity and the freshness bound the node folds into the proof.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ProgramHeadAttestContext {
    pub interface_version: Version,
    pub sequencer_public_key: [u8; 32],
    pub correlation_id: u64,
    pub program_id: [u8; 32],
    pub staleness_ms: u64,
}

/// The exact facts bound by the program discovery proof digest.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ProgramDiscoveryHead {
    pub program_id: [u8; 32],
    pub version: u32,
    pub code_hash: [u8; 32],
    pub abi_version: u16,
    pub observed_sequence: u64,
    pub observed_at: u64,
    pub valid_through: u64,
    pub state_root: [u8; 32],
}

/// Verified sequencer attestation of one program at the node's current head.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ProgramHeadAttestation {
    pub head: ProgramDiscoveryHead,
    pub head_receipt_digest: [u8; 32],
    pub digest: [u8; 32],
    pub public_key: [u8; 32],
    pub signature: [u8; 64],
}

/// Fail-closed head attestation boundary error.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ProgramHeadAttestError {
    Transport(TransportError),
    Envelope(SchemaError),
    CoreRefusal { class: u8, result: ResultCode },
    UnavailableCapability,
    InvalidCorrelation,
    InterfaceVersion(Version),
    MalformedRequest,
    MalformedResponse,
    ProgramMismatch,
    FreshnessBinding,
    HeadStale,
    UnknownProgram,
    SequencerKeyMismatch,
    Signature,
}

impl From<TransportError> for ProgramHeadAttestError {
    fn from(value: TransportError) -> Self {
        Self::Transport(value)
    }
}

impl From<SchemaError> for ProgramHeadAttestError {
    fn from(value: SchemaError) -> Self {
        Self::Envelope(value)
    }
}

/// Computes the `LayerX/program-discovery-proof/v1` digest the CLI verifies.
#[must_use]
pub fn program_discovery_proof_digest(head: &ProgramDiscoveryHead) -> [u8; 32] {
    let mut proof = Vec::with_capacity(PROGRAM_DISCOVERY_PROOF_DOMAIN.len() + 127);
    proof.extend_from_slice(PROGRAM_DISCOVERY_PROOF_DOMAIN);
    proof.extend_from_slice(&head.program_id);
    proof.push(1);
    proof.extend_from_slice(&head.version.to_be_bytes());
    proof.extend_from_slice(&head.code_hash);
    proof.extend_from_slice(&head.abi_version.to_be_bytes());
    proof.extend_from_slice(&head.observed_sequence.to_be_bytes());
    proof.extend_from_slice(&head.observed_at.to_be_bytes());
    proof.extend_from_slice(&head.valid_through.to_be_bytes());
    proof.extend_from_slice(&head.state_root);
    Sha256::digest(&proof).into()
}

/// Encodes the exact request payload.
#[must_use]
pub fn encode_program_head_attest_request(
    program_id: &[u8; 32],
    staleness_ms: u64,
) -> [u8; PROGRAM_HEAD_ATTEST_REQUEST_BYTES] {
    let mut payload = [0; PROGRAM_HEAD_ATTEST_REQUEST_BYTES];
    payload[..2].copy_from_slice(&PROGRAM_HEAD_ATTEST_VERSION.to_be_bytes());
    payload[2..34].copy_from_slice(program_id);
    payload[34..].copy_from_slice(&staleness_ms.to_be_bytes());
    payload
}

/// Encodes the exact response payload and proof material of one attestation.
#[must_use]
pub fn encode_program_head_attestation(
    attestation: &ProgramHeadAttestation,
) -> (
    [u8; PROGRAM_HEAD_ATTEST_PAYLOAD_BYTES],
    [u8; PROGRAM_HEAD_ATTEST_PROOF_BYTES],
) {
    let head = &attestation.head;
    let mut payload = [0; PROGRAM_HEAD_ATTEST_PAYLOAD_BYTES];
    payload[..2].copy_from_slice(&PROGRAM_HEAD_ATTEST_VERSION.to_be_bytes());
    payload[2..34].copy_from_slice(&head.program_id);
    payload[34..38].copy_from_slice(&head.version.to_be_bytes());
    payload[38..70].copy_from_slice(&head.code_hash);
    payload[70..72].copy_from_slice(&head.abi_version.to_be_bytes());
    payload[72..80].copy_from_slice(&head.observed_sequence.to_be_bytes());
    payload[80..88].copy_from_slice(&head.observed_at.to_be_bytes());
    payload[88..96].copy_from_slice(&head.valid_through.to_be_bytes());
    payload[96..128].copy_from_slice(&head.state_root);
    payload[128..].copy_from_slice(&attestation.head_receipt_digest);
    let mut proof = [0; PROGRAM_HEAD_ATTEST_PROOF_BYTES];
    proof[..32].copy_from_slice(&attestation.public_key);
    proof[32..].copy_from_slice(&attestation.signature);
    (payload, proof)
}

fn fixed<const N: usize>(bytes: &[u8]) -> Result<[u8; N], ProgramHeadAttestError> {
    bytes
        .try_into()
        .map_err(|_| ProgramHeadAttestError::MalformedResponse)
}

fn be_u64(bytes: &[u8]) -> Result<u64, ProgramHeadAttestError> {
    fixed::<8>(bytes).map(u64::from_be_bytes)
}

/// Decodes and verifies one attestation response against the requested
/// program, the requested staleness bound and the sequencer key the caller
/// already trusts.
///
/// # Errors
///
/// Refuses malformed layouts, a different program, a `valid_through` that is
/// not `observed_at` plus the requested bound, a public key other than the
/// trusted sequencer key, and a signature that does not verify over the
/// recomputed discovery proof digest.
pub fn decode_program_head_attestation(
    payload: &[u8],
    proof_material: &[u8],
    program_id: &[u8; 32],
    staleness_ms: u64,
    sequencer_public_key: &[u8; 32],
) -> Result<ProgramHeadAttestation, ProgramHeadAttestError> {
    if payload.len() != PROGRAM_HEAD_ATTEST_PAYLOAD_BYTES
        || proof_material.len() != PROGRAM_HEAD_ATTEST_PROOF_BYTES
        || fixed::<2>(&payload[..2]).map(u16::from_be_bytes)? != PROGRAM_HEAD_ATTEST_VERSION
    {
        return Err(ProgramHeadAttestError::MalformedResponse);
    }
    let head = ProgramDiscoveryHead {
        program_id: fixed(&payload[2..34])?,
        version: fixed::<4>(&payload[34..38]).map(u32::from_be_bytes)?,
        code_hash: fixed(&payload[38..70])?,
        abi_version: fixed::<2>(&payload[70..72]).map(u16::from_be_bytes)?,
        observed_sequence: be_u64(&payload[72..80])?,
        observed_at: be_u64(&payload[80..88])?,
        valid_through: be_u64(&payload[88..96])?,
        state_root: fixed(&payload[96..128])?,
    };
    let head_receipt_digest: [u8; 32] = fixed(&payload[128..])?;
    let public_key: [u8; 32] = fixed(&proof_material[..32])?;
    let signature: [u8; 64] = fixed(&proof_material[32..])?;
    if head.program_id != *program_id {
        return Err(ProgramHeadAttestError::ProgramMismatch);
    }
    if head.version == 0
        || head.observed_sequence == 0
        || head.observed_at == 0
        || head.state_root == [0; 32]
        || head_receipt_digest == [0; 32]
    {
        return Err(ProgramHeadAttestError::MalformedResponse);
    }
    if head.observed_at.checked_add(staleness_ms) != Some(head.valid_through) {
        return Err(ProgramHeadAttestError::FreshnessBinding);
    }
    if public_key != *sequencer_public_key {
        return Err(ProgramHeadAttestError::SequencerKeyMismatch);
    }
    let digest = program_discovery_proof_digest(&head);
    ed25519::verify_digest(&public_key, &signature, &digest)
        .map_err(|_| ProgramHeadAttestError::Signature)?;
    Ok(ProgramHeadAttestation {
        head,
        head_receipt_digest,
        digest,
        public_key,
        signature,
    })
}

/// Requests the sequencer's signed attestation of one program at its current
/// head and verifies it under the trusted sequencer key.
///
/// This operation sends once and receives once. It never polls or retries.
///
/// # Errors
///
/// Refuses unsupported interface versions, malformed requests, typed core
/// refusals, and every verification failure of the returned attestation.
pub fn attest_program_head(
    transport: &mut dyn FrameTransport,
    context: ProgramHeadAttestContext,
) -> Result<ProgramHeadAttestation, ProgramHeadAttestError> {
    if context.correlation_id == 0 {
        return Err(ProgramHeadAttestError::InvalidCorrelation);
    }
    if context.interface_version.major != Version::V1_7.major
        || context.interface_version.minor < Version::V1_7.minor
    {
        return Err(ProgramHeadAttestError::InterfaceVersion(
            context.interface_version,
        ));
    }
    if context.program_id == [0; 32] || context.staleness_ms == 0 {
        return Err(ProgramHeadAttestError::MalformedRequest);
    }
    let payload = encode_program_head_attest_request(&context.program_id, context.staleness_ms);
    let request = encode_envelope(Envelope {
        version: context.interface_version,
        message_tag: PROGRAM_HEAD_ATTEST_REQUEST_TAG,
        correlation_id: context.correlation_id,
        canonical_payload: &payload,
        proof_material: &[],
    })?;
    transport.send(&request)?;
    let response_bytes = transport.receive()?;
    let response = decode_envelope(&response_bytes)?;
    if response.version != context.interface_version
        || response.correlation_id != context.correlation_id
    {
        return Err(ProgramHeadAttestError::MalformedResponse);
    }
    if response.message_tag == ERROR_RESPONSE_TAG {
        if !response.proof_material.is_empty() {
            return Err(ProgramHeadAttestError::MalformedResponse);
        }
        let refusal = decode_core_refusal(response.canonical_payload)
            .ok_or(ProgramHeadAttestError::MalformedResponse)?;
        return match refusal.result.known() {
            Some(KnownResult::ProjectionStale) => Err(ProgramHeadAttestError::HeadStale),
            Some(KnownResult::UnknownField) => Err(ProgramHeadAttestError::UnknownProgram),
            _ => Err(ProgramHeadAttestError::CoreRefusal {
                class: refusal.class,
                result: refusal.result,
            }),
        };
    }
    if response.message_tag != PROGRAM_HEAD_ATTEST_RESPONSE_TAG {
        return Err(ProgramHeadAttestError::MalformedResponse);
    }
    decode_program_head_attestation(
        response.canonical_payload,
        response.proof_material,
        &context.program_id,
        context.staleness_ms,
        &context.sequencer_public_key,
    )
}
