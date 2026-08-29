//! Receipt-verified Programs operations for direct-node and hosted transports.

use layerx_proof::program::{
    verify_authorized_program_execution, AuthorizedProgramExecutionExpectation,
    VerifiedProgramExecution,
};
use layerx_proof::receipt::AuthorizedBatch;
use layerx_types::intent::ProgramCall;
use layerx_types::payload::{ModuleId, ModuleRegistry};
use layerx_wire::activity::decode_signed;
use layerx_wire::hash::activity_id;

pub const MAX_SIGNED_ACTIVITY_BYTES: usize = 1_048_576;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ProgramCallRequest {
    call: ProgramCall,
    signed_activity: Vec<u8>,
}

impl ProgramCallRequest {
    /// Binds a typed call to the exact signed Programs activity transported by
    /// the client.
    ///
    /// # Errors
    ///
    /// Refuses non-canonical activity bytes, the wrong module or ordinal, and
    /// any payload which differs from the typed call.
    pub fn new(
        registry: &ModuleRegistry,
        call: ProgramCall,
        signed_activity: &[u8],
    ) -> Result<Self, ProgramOperationError> {
        if signed_activity.is_empty() || signed_activity.len() > MAX_SIGNED_ACTIVITY_BYTES {
            return Err(ProgramOperationError::Bounds);
        }
        let activity =
            decode_signed(signed_activity, registry).map_err(|_| ProgramOperationError::Decode)?;
        if activity.activity_type().module() != ModuleId::Programs
            || activity.activity_type().ordinal() != 3
            || activity.payload() != call.canonical_payload()
        {
            return Err(ProgramOperationError::IdentityMismatch);
        }
        Ok(Self {
            call,
            signed_activity: signed_activity.to_vec(),
        })
    }

    #[must_use]
    pub const fn call(&self) -> &ProgramCall {
        &self.call
    }

    #[must_use]
    pub fn signed_activity(&self) -> &[u8] {
        &self.signed_activity
    }

    /// Returns the activity identifier derived from the retained signed bytes.
    ///
    /// # Errors
    ///
    /// Refuses only if the retained canonical activity cannot be hashed.
    pub fn activity_id(
        &self,
        registry: &ModuleRegistry,
    ) -> Result<[u8; 32], ProgramOperationError> {
        let activity = decode_signed(&self.signed_activity, registry)
            .map_err(|_| ProgramOperationError::Decode)?;
        activity_id(&activity).map_err(|_| ProgramOperationError::Decode)
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ProgramOperationError {
    Bounds,
    Transport,
    Decode,
    Verification,
    IdentityMismatch,
    UnknownOutcome,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ProgramExecutionEvidence {
    pub receipt: Vec<u8>,
    pub terminal_payload: Vec<u8>,
    pub call_graph: Vec<u8>,
    pub authority: AuthorizedBatch,
    pub activity_id: [u8; 32],
    pub program_id: [u8; 32],
    pub guest_abi_version: u16,
}

pub enum ProgramSubmission {
    Refused(VerifiedProgramExecution),
    Unknown {
        activity_id: [u8; 32],
        retained_signed_activity: Vec<u8>,
    },
    Executed(VerifiedProgramExecution),
}

pub trait ProgramTransport {
    type Discovery;
    type Interface;

    fn discover(&self, program: [u8; 32]) -> Result<Self::Discovery, ProgramOperationError>;
    fn interface(&self, program: [u8; 32]) -> Result<Self::Interface, ProgramOperationError>;
    fn simulate(
        &self,
        request: &ProgramCallRequest,
    ) -> Result<ProgramExecutionEvidence, ProgramOperationError>;
    fn submit(
        &self,
        request: &ProgramCallRequest,
        idempotency_key: [u8; 32],
    ) -> Result<ProgramSubmission, ProgramOperationError>;
    fn receipt(
        &self,
        idempotency_key: [u8; 32],
        expected_activity: [u8; 32],
    ) -> Result<ProgramSubmission, ProgramOperationError>;
    fn activity(
        &self,
        activity_id: [u8; 32],
    ) -> Result<ProgramSubmission, ProgramOperationError>;
}

/// Derives a typed Programs outcome only from a canonical signed receipt and
/// its committed terminal payload and call graph.
///
/// # Errors
///
/// Refuses any signature, state-root, activity, program, ABI, terminal, graph,
/// occupancy, or transfer-authority mismatch.
pub fn verify_program_evidence(
    evidence: &ProgramExecutionEvidence,
) -> Result<VerifiedProgramExecution, ProgramOperationError> {
    if evidence.receipt.is_empty()
        || evidence.receipt.len() > MAX_SIGNED_ACTIVITY_BYTES
        || evidence.terminal_payload.len() > MAX_SIGNED_ACTIVITY_BYTES
        || evidence.call_graph.len() > MAX_SIGNED_ACTIVITY_BYTES
    {
        return Err(ProgramOperationError::Bounds);
    }
    verify_authorized_program_execution(
        &evidence.receipt,
        &evidence.terminal_payload,
        &evidence.call_graph,
        AuthorizedProgramExecutionExpectation {
            authority: evidence.authority,
            activity_id: evidence.activity_id,
            program_id: evidence.program_id,
            guest_abi_version: evidence.guest_abi_version,
        },
    )
    .map_err(|_| ProgramOperationError::Verification)
}

#[must_use]
pub const fn platform_sdk_programs() -> &'static str {
    "receipt-and-terminal-verified-program-operations-v1"
}
