//! Noncommitting program simulation served by the sequencer boundary.

use layerx_crypto::ed25519;
use layerx_proof::receipt::{verify_sequencer_signature, VerificationFailure};
use layerx_types::payload::ModuleRegistry;
use layerx_types::result::ResultCode;
use layerx_wire::activity::decode_signed;
use layerx_wire::hash::activity_id;
use sha2::{Digest as _, Sha256};

use super::refusal::decode_core_refusal;
use super::schema::{decode_envelope, encode_envelope, Envelope, SchemaError, Version};
use super::transport::{FrameTransport, TransportError};

/// Tag of the request carrying one canonical signed program-call activity.
pub const SIMULATE_REQUEST_TAG: u16 = 30;
/// Tag of the response carrying the execution and its signed evidence.
pub const SIMULATE_RESPONSE_TAG: u16 = 31;
const ERROR_RESPONSE_TAG: u16 = 25;
const SIMULATION_PAYLOAD_VERSION: u16 = 1;
const SIMULATION_EVIDENCE_VERSION: u16 = 1;
const SIMULATION_EVIDENCE_BYTES: usize = 2 + 32 * 4 + 8 + 8 + 32 + 64;
const MAX_ARTIFACT_BYTES: usize = 1 << 20;

/// Domain prefix binding the simulation boundary identity to the sequencer
/// key. Identical to the emulator and gateway simulation boundary.
pub const SIMULATION_BOUNDARY_DOMAIN: &[u8] = b"LayerX/emulator/simulation-boundary/v1\0";
/// Domain prefix of the sequencer-signed simulation evidence digest.
pub const SIMULATION_EVIDENCE_DOMAIN: &[u8] = b"LayerX/agent/program-simulation-evidence/v1\0";

/// Execution of one program call against the current head that was never
/// committed, journaled, or queued.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SimulatedExecution {
    pub activity_id: [u8; 32],
    pub receipt: Vec<u8>,
    pub terminal_payload: Vec<u8>,
    pub call_graph: Vec<u8>,
}

/// Sequencer-signed statement of what the simulation observed and produced.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct SimulationEvidence {
    pub boundary_id: [u8; 32],
    pub activity_id: [u8; 32],
    pub previous_state_root: [u8; 32],
    pub hypothetical_state_root: [u8; 32],
    pub observed_sequence: u64,
    pub observed_at: u64,
    pub public_key: [u8; 32],
    pub signature: [u8; 64],
}

/// Verified simulation returned by the boundary.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Simulation {
    pub execution: SimulatedExecution,
    pub evidence: SimulationEvidence,
}

/// Request identity and verification expectations from the accepted LNI
/// handshake.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct SimulateContext {
    pub interface_version: Version,
    pub sequencer_public_key: [u8; 32],
    pub correlation_id: u64,
}

/// Fail-closed simulation boundary error.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SimulateError {
    Transport(TransportError),
    Envelope(SchemaError),
    CoreRefusal { class: u8, result: ResultCode },
    UnavailableCapability,
    Disconnected,
    InvalidCorrelation,
    InterfaceVersion(Version),
    MalformedRequest,
    MalformedResponse,
    ActivityMismatch,
    Receipt(VerificationFailure),
    ArtifactMismatch,
    SequencerKeyMismatch,
    EvidenceBinding,
    EvidenceSignature,
}

impl From<TransportError> for SimulateError {
    fn from(value: TransportError) -> Self {
        Self::Transport(value)
    }
}

impl From<SchemaError> for SimulateError {
    fn from(value: SchemaError) -> Self {
        Self::Envelope(value)
    }
}

/// Derives the simulation boundary identity bound to one sequencer key.
#[must_use]
pub fn simulation_boundary_id(sequencer_public_key: &[u8; 32]) -> [u8; 32] {
    let mut hasher = Sha256::new();
    hasher.update(SIMULATION_BOUNDARY_DOMAIN);
    hasher.update(sequencer_public_key);
    hasher.finalize().into()
}

/// Computes the exact digest the sequencer signs for one evidence record.
#[must_use]
pub fn simulation_evidence_digest(evidence: &SimulationEvidence) -> [u8; 32] {
    let mut hasher = Sha256::new();
    hasher.update(SIMULATION_EVIDENCE_DOMAIN);
    hasher.update(evidence.boundary_id);
    hasher.update(evidence.activity_id);
    hasher.update(evidence.previous_state_root);
    hasher.update(evidence.hypothetical_state_root);
    hasher.update(evidence.observed_sequence.to_be_bytes());
    hasher.update(evidence.observed_at.to_be_bytes());
    hasher.update([0_u8]);
    hasher.finalize().into()
}

/// Encodes the response payload exactly as the node serialises it.
///
/// # Errors
///
/// Refuses artifacts that do not fit the u32 wire length.
pub fn encode_simulation_payload(execution: &SimulatedExecution) -> Result<Vec<u8>, SimulateError> {
    let mut payload = Vec::with_capacity(
        2 + 32
            + 12
            + execution.receipt.len()
            + execution.terminal_payload.len()
            + execution.call_graph.len(),
    );
    payload.extend_from_slice(&SIMULATION_PAYLOAD_VERSION.to_be_bytes());
    payload.extend_from_slice(&execution.activity_id);
    for bytes in [
        &execution.receipt,
        &execution.terminal_payload,
        &execution.call_graph,
    ] {
        let length = u32::try_from(bytes.len()).map_err(|_| SimulateError::MalformedRequest)?;
        payload.extend_from_slice(&length.to_be_bytes());
        payload.extend_from_slice(bytes);
    }
    Ok(payload)
}

/// Encodes the evidence proof material exactly as the node serialises it.
#[must_use]
pub fn encode_simulation_evidence(evidence: &SimulationEvidence) -> Vec<u8> {
    let mut bytes = Vec::with_capacity(SIMULATION_EVIDENCE_BYTES);
    bytes.extend_from_slice(&SIMULATION_EVIDENCE_VERSION.to_be_bytes());
    bytes.extend_from_slice(&evidence.boundary_id);
    bytes.extend_from_slice(&evidence.activity_id);
    bytes.extend_from_slice(&evidence.previous_state_root);
    bytes.extend_from_slice(&evidence.hypothetical_state_root);
    bytes.extend_from_slice(&evidence.observed_sequence.to_be_bytes());
    bytes.extend_from_slice(&evidence.observed_at.to_be_bytes());
    bytes.extend_from_slice(&evidence.public_key);
    bytes.extend_from_slice(&evidence.signature);
    bytes
}

/// Decodes the strict response payload without verifying it.
///
/// # Errors
///
/// Refuses any truncated, oversized, mis-versioned, or trailing bytes.
pub fn decode_simulation_payload(payload: &[u8]) -> Result<SimulatedExecution, SimulateError> {
    let mut cursor = Cursor::new(payload);
    if cursor.u16()? != SIMULATION_PAYLOAD_VERSION {
        return Err(SimulateError::MalformedResponse);
    }
    let activity_id = cursor.array()?;
    let receipt = cursor.bounded()?.to_vec();
    let terminal_payload = cursor.bounded()?.to_vec();
    let call_graph = cursor.bounded()?.to_vec();
    if receipt.is_empty() || !cursor.finished() {
        return Err(SimulateError::MalformedResponse);
    }
    Ok(SimulatedExecution {
        activity_id,
        receipt,
        terminal_payload,
        call_graph,
    })
}

/// Decodes the strict evidence proof material without verifying it.
///
/// # Errors
///
/// Refuses any truncated, mis-versioned, or trailing bytes.
pub fn decode_simulation_evidence(bytes: &[u8]) -> Result<SimulationEvidence, SimulateError> {
    if bytes.len() != SIMULATION_EVIDENCE_BYTES {
        return Err(SimulateError::MalformedResponse);
    }
    let mut cursor = Cursor::new(bytes);
    if cursor.u16()? != SIMULATION_EVIDENCE_VERSION {
        return Err(SimulateError::MalformedResponse);
    }
    let evidence = SimulationEvidence {
        boundary_id: cursor.array()?,
        activity_id: cursor.array()?,
        previous_state_root: cursor.array()?,
        hypothetical_state_root: cursor.array()?,
        observed_sequence: cursor.u64()?,
        observed_at: cursor.u64()?,
        public_key: cursor.array()?,
        signature: cursor.array()?,
    };
    if !cursor.finished() {
        return Err(SimulateError::MalformedResponse);
    }
    Ok(evidence)
}

/// Verifies one decoded simulation against the activity it was requested for
/// and the handshake-authorised sequencer key.
///
/// # Errors
///
/// Refuses an unsigned or foreign receipt, artifacts that do not hash to the
/// receipt outcome roots, evidence bound to a different activity, key, or
/// state transition, and any invalid evidence signature.
pub fn verify_simulation(
    execution: SimulatedExecution,
    evidence: SimulationEvidence,
    expected_activity_id: [u8; 32],
    sequencer_public_key: [u8; 32],
) -> Result<Simulation, SimulateError> {
    if execution.activity_id != expected_activity_id {
        return Err(SimulateError::ActivityMismatch);
    }
    let receipt = verify_sequencer_signature(&execution.receipt, sequencer_public_key)
        .map_err(SimulateError::Receipt)?;
    let protocol = receipt.protocol().ok_or(SimulateError::MalformedResponse)?;
    if protocol.activity_id() != expected_activity_id {
        return Err(SimulateError::ActivityMismatch);
    }
    match protocol.program_outcome() {
        Some(outcome) => {
            if !execution.terminal_payload.is_empty()
                && digest(&execution.terminal_payload) != outcome.terminal_payload_root()
            {
                return Err(SimulateError::ArtifactMismatch);
            }
            if !execution.call_graph.is_empty()
                && digest(&execution.call_graph) != outcome.call_graph_root()
            {
                return Err(SimulateError::ArtifactMismatch);
            }
        }
        None => {
            if !execution.terminal_payload.is_empty() || !execution.call_graph.is_empty() {
                return Err(SimulateError::ArtifactMismatch);
            }
        }
    }
    if evidence.public_key != sequencer_public_key {
        return Err(SimulateError::SequencerKeyMismatch);
    }
    if evidence.boundary_id != simulation_boundary_id(&sequencer_public_key)
        || evidence.activity_id != expected_activity_id
        || evidence.previous_state_root != protocol.previous_state_root()
        || evidence.hypothetical_state_root != protocol.resulting_state_root()
    {
        return Err(SimulateError::EvidenceBinding);
    }
    let evidence_digest = simulation_evidence_digest(&evidence);
    ed25519::verify_digest(&evidence.public_key, &evidence.signature, &evidence_digest)
        .map_err(|_| SimulateError::EvidenceSignature)?;
    Ok(Simulation {
        execution,
        evidence,
    })
}

/// Executes one canonical signed program call against the node's current head
/// without committing it and verifies the returned execution and evidence.
///
/// # Errors
///
/// Preserves transport and typed core refusals and rejects every malformed,
/// unverifiable, or mismatched response.
pub fn simulate(
    transport: &mut dyn FrameTransport,
    registry: &ModuleRegistry,
    signed_activity: &[u8],
    context: SimulateContext,
) -> Result<Simulation, SimulateError> {
    if context.correlation_id == 0 {
        return Err(SimulateError::InvalidCorrelation);
    }
    if context.interface_version.major != Version::V1_4.major
        || context.interface_version.minor < Version::V1_4.minor
    {
        return Err(SimulateError::InterfaceVersion(context.interface_version));
    }
    let activity =
        decode_signed(signed_activity, registry).map_err(|_| SimulateError::MalformedRequest)?;
    let expected_activity_id =
        activity_id(&activity).map_err(|_| SimulateError::MalformedRequest)?;
    let request = encode_envelope(Envelope {
        version: context.interface_version,
        message_tag: SIMULATE_REQUEST_TAG,
        correlation_id: context.correlation_id,
        canonical_payload: signed_activity,
        proof_material: &[],
    })?;
    transport.send(&request)?;
    let response_bytes = transport.receive()?;
    let response = decode_envelope(&response_bytes)?;
    if response.version != context.interface_version
        || response.correlation_id != context.correlation_id
    {
        return Err(SimulateError::MalformedResponse);
    }
    if response.message_tag == ERROR_RESPONSE_TAG {
        if !response.proof_material.is_empty() {
            return Err(SimulateError::MalformedResponse);
        }
        let refusal = decode_core_refusal(response.canonical_payload)
            .ok_or(SimulateError::MalformedResponse)?;
        return Err(SimulateError::CoreRefusal {
            class: refusal.class,
            result: refusal.result,
        });
    }
    if response.message_tag != SIMULATE_RESPONSE_TAG {
        return Err(SimulateError::MalformedResponse);
    }
    let execution = decode_simulation_payload(response.canonical_payload)?;
    let evidence = decode_simulation_evidence(response.proof_material)?;
    verify_simulation(
        execution,
        evidence,
        expected_activity_id,
        context.sequencer_public_key,
    )
}

fn digest(bytes: &[u8]) -> [u8; 32] {
    Sha256::digest(bytes).into()
}

struct Cursor<'a> {
    bytes: &'a [u8],
    offset: usize,
}

impl<'a> Cursor<'a> {
    const fn new(bytes: &'a [u8]) -> Self {
        Self { bytes, offset: 0 }
    }

    fn take(&mut self, length: usize) -> Result<&'a [u8], SimulateError> {
        let end = self
            .offset
            .checked_add(length)
            .ok_or(SimulateError::MalformedResponse)?;
        let value = self
            .bytes
            .get(self.offset..end)
            .ok_or(SimulateError::MalformedResponse)?;
        self.offset = end;
        Ok(value)
    }

    fn bounded(&mut self) -> Result<&'a [u8], SimulateError> {
        let length = usize::try_from(self.u32()?).map_err(|_| SimulateError::MalformedResponse)?;
        if length > MAX_ARTIFACT_BYTES {
            return Err(SimulateError::MalformedResponse);
        }
        self.take(length)
    }

    fn u16(&mut self) -> Result<u16, SimulateError> {
        Ok(u16::from_be_bytes(self.array()?))
    }

    fn u32(&mut self) -> Result<u32, SimulateError> {
        Ok(u32::from_be_bytes(self.array()?))
    }

    fn u64(&mut self) -> Result<u64, SimulateError> {
        Ok(u64::from_be_bytes(self.array()?))
    }

    fn array<const LENGTH: usize>(&mut self) -> Result<[u8; LENGTH], SimulateError> {
        self.take(LENGTH)?
            .try_into()
            .map_err(|_| SimulateError::MalformedResponse)
    }

    const fn finished(&self) -> bool {
        self.offset == self.bytes.len()
    }
}
