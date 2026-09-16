//! Snapshot-pinned, noncommitting program reads over LNI v1.6.

use layerx_types::payload::{ModuleId, ModuleRegistry};
use layerx_types::result::{KnownResult, ResultCode};
use layerx_wire::activity::decode_signed;
use layerx_wire::hash::activity_id;

use super::refusal::decode_core_refusal;
use super::schema::{decode_envelope, encode_envelope, Envelope, SchemaError, Version};
use super::simulate::{
    decode_simulation_evidence, decode_simulation_payload, verify_simulation, SimulateError,
    SimulatedExecution, SimulationEvidence,
};
use super::transport::{FrameTransport, TransportError};

/// Tag carrying a snapshot constraint and one exact signed ProgramCall.
pub const PROGRAM_READ_REQUEST_TAG: u16 = 38;
/// Tag carrying the existing signed simulation result shape.
pub const PROGRAM_READ_RESPONSE_TAG: u16 = 39;
const ERROR_RESPONSE_TAG: u16 = 25;
const PROGRAM_READ_REQUEST_VERSION: u16 = 1;

/// Request identity and immutable snapshot constraints.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ProgramReadContext {
    pub interface_version: Version,
    pub sequencer_public_key: [u8; 32],
    pub correlation_id: u64,
    pub minimum_sequence: u64,
    pub expected_state_root: Option<[u8; 32]>,
}

/// Canonical state snapshot against which the program read executed.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ProgramReadSnapshot {
    pub minimum_sequence: u64,
    pub observed_sequence: u64,
    pub state_root: [u8; 32],
}

/// Verified noncommitting execution and its exact observed snapshot.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ProgramReadResult {
    pub execution: SimulatedExecution,
    pub evidence: SimulationEvidence,
    pub snapshot: ProgramReadSnapshot,
}

/// Fail-closed program-read boundary error.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ProgramReadError {
    Transport(TransportError),
    Envelope(SchemaError),
    CoreRefusal { class: u8, result: ResultCode },
    UnavailableCapability,
    Disconnected,
    InvalidCorrelation,
    InterfaceVersion(Version),
    CanonicalActivity,
    MalformedRequest,
    MalformedResponse,
    SnapshotStale,
    SnapshotMismatch,
    Integrity(SimulateError),
}

impl From<TransportError> for ProgramReadError {
    fn from(value: TransportError) -> Self {
        Self::Transport(value)
    }
}

impl From<SchemaError> for ProgramReadError {
    fn from(value: SchemaError) -> Self {
        Self::Envelope(value)
    }
}

/// Executes one exact signed ProgramCall against a server-captured immutable
/// snapshot and verifies the existing sequencer-signed simulation evidence.
///
/// This operation sends once and receives once. It never polls, retries, or
/// submits the activity for execution.
///
/// # Errors
///
/// Refuses unsupported interface versions, malformed or non-ProgramCall
/// activities, typed core refusals, stale/mismatched snapshots, and every
/// existing simulation-integrity failure.
pub fn read_program(
    transport: &mut dyn FrameTransport,
    registry: &ModuleRegistry,
    signed_activity: &[u8],
    context: ProgramReadContext,
) -> Result<ProgramReadResult, ProgramReadError> {
    if context.correlation_id == 0 {
        return Err(ProgramReadError::InvalidCorrelation);
    }
    if context.interface_version.major != Version::V1_6.major
        || context.interface_version.minor < Version::V1_6.minor
    {
        return Err(ProgramReadError::InterfaceVersion(
            context.interface_version,
        ));
    }
    if context.expected_state_root == Some([0; 32]) {
        return Err(ProgramReadError::MalformedRequest);
    }
    let activity = decode_signed(signed_activity, registry)
        .map_err(|_| ProgramReadError::CanonicalActivity)?;
    if activity.activity_type().module() != ModuleId::Programs
        || activity.activity_type().ordinal() != 3
    {
        return Err(ProgramReadError::CanonicalActivity);
    }
    let expected_activity_id =
        activity_id(&activity).map_err(|_| ProgramReadError::CanonicalActivity)?;
    let activity_length =
        u32::try_from(signed_activity.len()).map_err(|_| ProgramReadError::MalformedRequest)?;
    let mut payload = Vec::with_capacity(2 + 8 + 1 + 32 + 4 + signed_activity.len());
    payload.extend_from_slice(&PROGRAM_READ_REQUEST_VERSION.to_be_bytes());
    payload.extend_from_slice(&context.minimum_sequence.to_be_bytes());
    match context.expected_state_root {
        Some(root) => {
            payload.push(1);
            payload.extend_from_slice(&root);
        }
        None => {
            payload.push(0);
            payload.extend_from_slice(&[0; 32]);
        }
    }
    payload.extend_from_slice(&activity_length.to_be_bytes());
    payload.extend_from_slice(signed_activity);

    let request = encode_envelope(Envelope {
        version: context.interface_version,
        message_tag: PROGRAM_READ_REQUEST_TAG,
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
        return Err(ProgramReadError::MalformedResponse);
    }
    if response.message_tag == ERROR_RESPONSE_TAG {
        if !response.proof_material.is_empty() {
            return Err(ProgramReadError::MalformedResponse);
        }
        let refusal = decode_core_refusal(response.canonical_payload)
            .ok_or(ProgramReadError::MalformedResponse)?;
        return match refusal.result.known() {
            Some(KnownResult::ProjectionStale) => Err(ProgramReadError::SnapshotStale),
            Some(KnownResult::ContextMismatch) => Err(ProgramReadError::SnapshotMismatch),
            _ => Err(ProgramReadError::CoreRefusal {
                class: refusal.class,
                result: refusal.result,
            }),
        };
    }
    if response.message_tag != PROGRAM_READ_RESPONSE_TAG {
        return Err(ProgramReadError::MalformedResponse);
    }
    let execution = decode_simulation_payload(response.canonical_payload)
        .map_err(ProgramReadError::Integrity)?;
    let evidence =
        decode_simulation_evidence(response.proof_material).map_err(ProgramReadError::Integrity)?;
    let simulation = verify_simulation(
        execution,
        evidence,
        expected_activity_id,
        context.sequencer_public_key,
    )
    .map_err(ProgramReadError::Integrity)?;
    if simulation.evidence.observed_sequence < context.minimum_sequence {
        return Err(ProgramReadError::SnapshotStale);
    }
    if context
        .expected_state_root
        .is_some_and(|root| root != simulation.evidence.previous_state_root)
    {
        return Err(ProgramReadError::SnapshotMismatch);
    }
    let snapshot = ProgramReadSnapshot {
        minimum_sequence: context.minimum_sequence,
        observed_sequence: simulation.evidence.observed_sequence,
        state_root: simulation.evidence.previous_state_root,
    };
    Ok(ProgramReadResult {
        execution: simulation.execution,
        evidence: simulation.evidence,
        snapshot,
    })
}
