//! Authenticated LNI acquisition of signed batch headers and availability data.

use std::path::PathBuf;

use layerx_client::availability::{
    fetch, AvailabilityResult, AvailabilitySelector, FetchContext, FetchError, FetchOutcome,
    Provider, ProviderSet, RetrievalLimits,
};
use layerx_client::lni::handshake::{perform, HandshakeConfig, HandshakeError};
use layerx_client::lni::schema::{
    decode_envelope, encode_envelope, Capability, Envelope, SchemaError,
};
use layerx_client::lni::transport::{ConnectionGate, FrameTransport, Limits, TransportError, Uds};
use layerx_crypto::ed25519;
use layerx_proof::availability::RootCommitments;
use layerx_wire::hash::batch_header_digest;
use layerx_wire::receipt::{decode_batch_header, encode_batch_header};

use crate::{BatchAuthorization, NodeBatch, NodeHead};

const BATCH_HEADER_REQUEST_TAG: u16 = 12;
const BATCH_HEADER_RESPONSE_TAG: u16 = 13;
const BATCH_SELECTOR_VERSION: u16 = 1;
const BATCH_PROOF_VERSION: u16 = 1;
const BATCH_PROOF_BYTES: usize = 2 + 32 + 32 + 8 + 8 + 64;

/// Immutable startup policy for the sole core evidence boundary.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct NodeSourceConfig {
    pub socket: PathBuf,
    pub handshake: HandshakeConfig,
    pub transport_limits: Limits,
    pub retrieval_limits: RetrievalLimits,
}

/// A batch archive input established from one authenticated LNI connection.
pub struct AcquiredBatch {
    pub batch: NodeBatch,
    pub availability: AvailabilityResult,
    pub head: NodeHead,
    /// The handshake carries a checkpoint identifier but not its batch
    /// coordinate. A non-zero value remains visible so runtime readiness never
    /// represents checkpoint mirroring as current without certificate proof.
    pub uncoordinated_checkpoint_id: Option<[u8; 32]>,
}

/// Exact failure while acquiring core evidence. Partial bytes are never
/// returned as an archive input.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum NodeSourceError {
    Transport(TransportError),
    Handshake(HandshakeError),
    Schema(SchemaError),
    Capability(Capability),
    UnexpectedResponse,
    MalformedBatchProof,
    BatchHeader,
    BatchSelector,
    SequencerIdentity,
    HeaderSignature,
    Availability(FetchError),
    AvailabilityPartial,
}

impl From<TransportError> for NodeSourceError {
    fn from(value: TransportError) -> Self {
        Self::Transport(value)
    }
}

impl From<SchemaError> for NodeSourceError {
    fn from(value: SchemaError) -> Self {
        Self::Schema(value)
    }
}

/// Reconnect-per-acquisition LNI client. It never accepts an unsigned batch
/// value and never synthesises a proof level from node metadata.
pub struct LniArchiveSource {
    config: NodeSourceConfig,
    gate: ConnectionGate,
    correlation_id: u64,
}

impl LniArchiveSource {
    #[must_use]
    pub fn new(config: NodeSourceConfig) -> Self {
        Self {
            gate: ConnectionGate::new(config.transport_limits.maximum_connections),
            config,
            correlation_id: 1,
        }
    }

    /// Fetches one exact signed header and its complete verified availability
    /// records through the same authenticated node session.
    ///
    /// # Errors
    ///
    /// Refuses missing capabilities, selector substitution, key/range drift,
    /// invalid signatures, partial availability and all bounded transport
    /// failures.
    pub fn acquire(&mut self, batch_number: u64) -> Result<AcquiredBatch, NodeSourceError> {
        if batch_number == 0 {
            return Err(NodeSourceError::BatchSelector);
        }
        let mut transport = Uds::connect(
            &self.config.socket,
            &self.gate,
            self.config.transport_limits,
        )?;
        let handshake = perform(&mut transport, &self.config.handshake, None)
            .map_err(NodeSourceError::Handshake)?;
        for capability in [Capability::BatchHeader, Capability::AvailabilityFetch] {
            if !handshake.capabilities().contains(capability) {
                return Err(NodeSourceError::Capability(capability));
            }
        }
        let correlation_id = self.next_correlation()?;
        let mut selector = Vec::with_capacity(10);
        selector.extend_from_slice(&BATCH_SELECTOR_VERSION.to_be_bytes());
        selector.extend_from_slice(&batch_number.to_be_bytes());
        let request = encode_envelope(Envelope {
            version: handshake.node().interface_version,
            message_tag: BATCH_HEADER_REQUEST_TAG,
            correlation_id,
            canonical_payload: &selector,
            proof_material: &[],
        })?;
        transport.send(&request)?;
        let response_bytes = transport.receive()?;
        let response = decode_envelope(&response_bytes)?;
        if response.version.major != handshake.node().interface_version.major
            || response.message_tag != BATCH_HEADER_RESPONSE_TAG
            || response.correlation_id != correlation_id
        {
            return Err(NodeSourceError::UnexpectedResponse);
        }
        let authorization = decode_batch_proof(response.proof_material)?;
        let header = decode_batch_header(response.canonical_payload)
            .map_err(|_| NodeSourceError::BatchHeader)?;
        let reproduced = encode_batch_header(&header).map_err(|_| NodeSourceError::BatchHeader)?;
        if reproduced != response.canonical_payload
            || header.protocol_version() != handshake.node().protocol_version
            || header.network_id() != handshake.node().network_id
            || header.batch_number() != batch_number
        {
            return Err(NodeSourceError::BatchSelector);
        }
        if authorization.sequencer_public_key != handshake.node().authorised_sequencer_key
            || authorization.sequencer_id != header.sequencer_id()
            || batch_number < authorization.first_batch_number
            || batch_number > authorization.last_batch_number
        {
            return Err(NodeSourceError::SequencerIdentity);
        }
        let digest = batch_header_digest(&reproduced).map_err(|_| NodeSourceError::BatchHeader)?;
        ed25519::verify_digest(
            &authorization.sequencer_public_key,
            &authorization.header_signature,
            &digest,
        )
        .map_err(|_| NodeSourceError::HeaderSignature)?;

        let roots = RootCommitments {
            activity: header.activity_merkle_root(),
            receipt: header.receipt_merkle_root(),
            event: header.event_merkle_root(),
            oracle: header.oracle_root(),
        };
        let availability_correlation = self.next_correlation()?;
        let mut providers = ProviderSet::new(vec![Provider {
            name: "authenticated-node".to_owned(),
            transport: &mut transport,
        }]);
        let outcome = fetch(
            &mut providers,
            AvailabilitySelector::Batch(batch_number),
            FetchContext {
                interface_version: handshake.node().interface_version,
                correlation_id: availability_correlation,
                expected_batch_number: batch_number,
                data_availability_root: header.data_availability_root(),
                record_roots: roots,
                limits: self.config.retrieval_limits,
            },
            |_| {},
        )
        .map_err(NodeSourceError::Availability)?;
        let FetchOutcome::Complete(availability) = outcome else {
            return Err(NodeSourceError::AvailabilityPartial);
        };
        let checkpoint_id = (handshake.node().latest_finalised_checkpoint != [0; 32])
            .then_some(handshake.node().latest_finalised_checkpoint);
        Ok(AcquiredBatch {
            batch: NodeBatch::authenticated(reproduced, authorization),
            availability: *availability,
            head: NodeHead {
                latest_sealed_batch: handshake.node().latest_sealed_batch,
                latest_finalised_checkpoint: None,
            },
            uncoordinated_checkpoint_id: checkpoint_id,
        })
    }

    fn next_correlation(&mut self) -> Result<u64, NodeSourceError> {
        let current = self.correlation_id;
        self.correlation_id = self
            .correlation_id
            .checked_add(1)
            .ok_or(NodeSourceError::BatchSelector)?;
        Ok(current)
    }
}

fn decode_batch_proof(bytes: &[u8]) -> Result<BatchAuthorization, NodeSourceError> {
    if bytes.len() != BATCH_PROOF_BYTES {
        return Err(NodeSourceError::MalformedBatchProof);
    }
    let version = u16::from_be_bytes(
        bytes[0..2]
            .try_into()
            .map_err(|_| NodeSourceError::MalformedBatchProof)?,
    );
    if version != BATCH_PROOF_VERSION {
        return Err(NodeSourceError::MalformedBatchProof);
    }
    let sequencer_id = bytes[2..34]
        .try_into()
        .map_err(|_| NodeSourceError::MalformedBatchProof)?;
    let sequencer_public_key = bytes[34..66]
        .try_into()
        .map_err(|_| NodeSourceError::MalformedBatchProof)?;
    let first_batch_number = u64::from_be_bytes(
        bytes[66..74]
            .try_into()
            .map_err(|_| NodeSourceError::MalformedBatchProof)?,
    );
    let last_batch_number = u64::from_be_bytes(
        bytes[74..82]
            .try_into()
            .map_err(|_| NodeSourceError::MalformedBatchProof)?,
    );
    let header_signature = bytes[82..146]
        .try_into()
        .map_err(|_| NodeSourceError::MalformedBatchProof)?;
    if first_batch_number == 0 || first_batch_number > last_batch_number {
        return Err(NodeSourceError::MalformedBatchProof);
    }
    Ok(BatchAuthorization {
        sequencer_id,
        sequencer_public_key,
        first_batch_number,
        last_batch_number,
        header_signature,
    })
}
