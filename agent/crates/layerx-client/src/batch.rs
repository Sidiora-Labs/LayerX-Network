//! Canonical signed batch-header retrieval.

use layerx_crypto::ed25519;
use layerx_wire::hash::batch_header_digest;
use layerx_wire::receipt::{decode_batch_header, encode_batch_header, BatchHeader};

use crate::lni::schema::{decode_envelope, encode_envelope, Envelope, SchemaError, Version};
use crate::lni::transport::{FrameTransport, TransportError};

const REQUEST_TAG: u16 = 12;
const RESPONSE_TAG: u16 = 13;
const PROOF_BYTES: usize = 146;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SignedBatchHeader {
    pub header: BatchHeader,
    canonical_bytes: Vec<u8>,
    pub sequencer_id: [u8; 32],
    pub sequencer_public_key: [u8; 32],
    pub first_batch_number: u64,
    pub last_batch_number: u64,
    pub signature: [u8; 64],
}

impl SignedBatchHeader {
    #[must_use]
    pub fn canonical_bytes(&self) -> &[u8] {
        &self.canonical_bytes
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum BatchHeaderError {
    Transport(TransportError),
    Envelope(SchemaError),
    UnexpectedResponse,
    Malformed,
    Missing,
    SelectorMismatch,
    AuthorityMismatch,
    Signature,
    UnavailableCapability,
    Disconnected,
}

impl From<TransportError> for BatchHeaderError {
    fn from(value: TransportError) -> Self {
        Self::Transport(value)
    }
}

impl From<SchemaError> for BatchHeaderError {
    fn from(value: SchemaError) -> Self {
        Self::Envelope(value)
    }
}

pub fn lookup(
    transport: &mut dyn FrameTransport,
    version: Version,
    batch_number: u64,
    correlation_id: u64,
    handshake_key: [u8; 32],
) -> Result<SignedBatchHeader, BatchHeaderError> {
    if batch_number == 0 || correlation_id == 0 {
        return Err(BatchHeaderError::Malformed);
    }
    let mut selector = [0_u8; 10];
    selector[..2].copy_from_slice(&1_u16.to_be_bytes());
    selector[2..].copy_from_slice(&batch_number.to_be_bytes());
    transport.send(&encode_envelope(Envelope {
        version,
        message_tag: REQUEST_TAG,
        correlation_id,
        canonical_payload: &selector,
        proof_material: &[],
    })?)?;
    let response_bytes = transport.receive()?;
    let response = decode_envelope(&response_bytes)?;
    if response.version.major != version.major
        || response.message_tag != RESPONSE_TAG
        || response.correlation_id != correlation_id
    {
        return Err(BatchHeaderError::UnexpectedResponse);
    }
    if response.canonical_payload.is_empty() && response.proof_material.is_empty() {
        return Err(BatchHeaderError::Missing);
    }
    if response.proof_material.len() != PROOF_BYTES
        || u16::from_be_bytes(
            response.proof_material[..2]
                .try_into()
                .map_err(|_| BatchHeaderError::Malformed)?,
        ) != 1
    {
        return Err(BatchHeaderError::Malformed);
    }
    let proof = response.proof_material;
    let sequencer_id = proof[2..34]
        .try_into()
        .map_err(|_| BatchHeaderError::Malformed)?;
    let public_key = proof[34..66]
        .try_into()
        .map_err(|_| BatchHeaderError::Malformed)?;
    let first = u64::from_be_bytes(
        proof[66..74]
            .try_into()
            .map_err(|_| BatchHeaderError::Malformed)?,
    );
    let last = u64::from_be_bytes(
        proof[74..82]
            .try_into()
            .map_err(|_| BatchHeaderError::Malformed)?,
    );
    let signature = proof[82..146]
        .try_into()
        .map_err(|_| BatchHeaderError::Malformed)?;
    if public_key != handshake_key
        || first == 0
        || last < first
        || batch_number < first
        || batch_number > last
    {
        return Err(BatchHeaderError::AuthorityMismatch);
    }
    let header =
        decode_batch_header(response.canonical_payload).map_err(|_| BatchHeaderError::Malformed)?;
    let reproduced = encode_batch_header(&header).map_err(|_| BatchHeaderError::Malformed)?;
    if reproduced != response.canonical_payload {
        return Err(BatchHeaderError::Malformed);
    }
    if header.batch_number() != batch_number {
        return Err(BatchHeaderError::SelectorMismatch);
    }
    if header.sequencer_id() != sequencer_id {
        return Err(BatchHeaderError::AuthorityMismatch);
    }
    let digest = batch_header_digest(&reproduced).map_err(|_| BatchHeaderError::Malformed)?;
    ed25519::verify_digest(&public_key, &signature, &digest)
        .map_err(|_| BatchHeaderError::Signature)?;
    Ok(SignedBatchHeader {
        header,
        canonical_bytes: reproduced,
        sequencer_id,
        sequencer_public_key: public_key,
        first_batch_number: first,
        last_batch_number: last,
        signature,
    })
}
