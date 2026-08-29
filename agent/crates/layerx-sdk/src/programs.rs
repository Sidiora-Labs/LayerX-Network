//! Receipt-verified Programs operations for direct-node and hosted transports.

use layerx_proof::program::{
    verify_authorized_program_execution, AuthorizedProgramExecutionExpectation,
    VerifiedProgramExecution,
};
use layerx_proof::receipt::AuthorizedBatch;
pub use layerx_agent_api::error::{ErrorClass as AgentErrorClass, Retriability};
use layerx_types::intent::ProgramCall;
use layerx_types::payload::{ModuleId, ModuleRegistry};
use layerx_types::result::ResultCode;
use layerx_wire::activity::decode_signed;
use layerx_wire::hash::activity_id;

pub const MAX_SIGNED_ACTIVITY_BYTES: usize = 1_048_576;

#[path = "program_http.rs"]
mod http;

pub use http::{HttpProgramTransport, LayerXKeyCredential};

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ProgramCallRequest {
    call: ProgramCall,
    signed_activity: Vec<u8>,
    activity_id: [u8; 32],
    idempotency_key: [u8; 32],
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
        let bound_activity_id =
            activity_id(&activity).map_err(|_| ProgramOperationError::Decode)?;
        Ok(Self {
            call,
            signed_activity: signed_activity.to_vec(),
            activity_id: bound_activity_id,
            idempotency_key: activity.idempotency_key(),
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
        _registry: &ModuleRegistry,
    ) -> Result<[u8; 32], ProgramOperationError> {
        Ok(self.activity_id)
    }

    #[must_use]
    pub const fn bound_activity_id(&self) -> [u8; 32] {
        self.activity_id
    }

    #[must_use]
    pub const fn bound_idempotency_key(&self) -> [u8; 32] {
        self.idempotency_key
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ProgramServiceError {
    pub class: AgentErrorClass,
    pub retriability: Retriability,
    pub request_id: String,
    pub reason: String,
    pub protocol_result_code: Option<ResultCode>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ProgramOperationError {
    Bounds,
    InvalidEndpoint,
    Authentication,
    Transport,
    Decode,
    Verification,
    IdentityMismatch,
    UnknownOutcome,
    Service(ProgramServiceError),
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ProgramLifecycle {
    Active,
    Deprecated,
    Tombstoned,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ProgramSource {
    Unpublished,
    Verified {
        source_digest: [u8; 32],
        environment_digest: [u8; 32],
    },
    Mismatch {
        expected_code_hash: [u8; 32],
        reproduced_artifact_digest: [u8; 32],
    },
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct VerifiedProgramDiscovery {
    program_id: [u8; 32],
    lifecycle: ProgramLifecycle,
    version: u32,
    code_hash: [u8; 32],
    abi_version: u16,
    receipt_digest: [u8; 32],
    state_root: [u8; 32],
    observed_sequence: u64,
    observed_at: u64,
    valid_through: u64,
}

impl VerifiedProgramDiscovery {
    #[must_use] pub const fn program_id(&self) -> [u8; 32] { self.program_id }
    #[must_use] pub const fn lifecycle(&self) -> ProgramLifecycle { self.lifecycle }
    #[must_use] pub const fn version(&self) -> u32 { self.version }
    #[must_use] pub const fn code_hash(&self) -> [u8; 32] { self.code_hash }
    #[must_use] pub const fn abi_version(&self) -> u16 { self.abi_version }
    #[must_use] pub const fn receipt_digest(&self) -> [u8; 32] { self.receipt_digest }
    #[must_use] pub const fn state_root(&self) -> [u8; 32] { self.state_root }
    #[must_use] pub const fn observed_sequence(&self) -> u64 { self.observed_sequence }
    #[must_use] pub const fn observed_at(&self) -> u64 { self.observed_at }
    #[must_use] pub const fn valid_through(&self) -> u64 { self.valid_through }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct VerifiedProgramInterface {
    program_id: [u8; 32],
    version: u32,
    code_hash: [u8; 32],
    abi_version: u16,
    interface: Vec<u8>,
    interface_digest: [u8; 32],
    receipt_digest: [u8; 32],
    state_root: [u8; 32],
    observed_sequence: u64,
    observed_at: u64,
    valid_through: u64,
    source: ProgramSource,
}

impl VerifiedProgramInterface {
    #[must_use] pub const fn program_id(&self) -> [u8; 32] { self.program_id }
    #[must_use] pub const fn version(&self) -> u32 { self.version }
    #[must_use] pub const fn code_hash(&self) -> [u8; 32] { self.code_hash }
    #[must_use] pub const fn abi_version(&self) -> u16 { self.abi_version }
    #[must_use] pub fn interface(&self) -> &[u8] { &self.interface }
    #[must_use] pub const fn interface_digest(&self) -> [u8; 32] { self.interface_digest }
    #[must_use] pub const fn receipt_digest(&self) -> [u8; 32] { self.receipt_digest }
    #[must_use] pub const fn state_root(&self) -> [u8; 32] { self.state_root }
    #[must_use] pub const fn observed_sequence(&self) -> u64 { self.observed_sequence }
    #[must_use] pub const fn observed_at(&self) -> u64 { self.observed_at }
    #[must_use] pub const fn valid_through(&self) -> u64 { self.valid_through }
    #[must_use] pub const fn source(&self) -> ProgramSource { self.source }
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
        idempotency_key: [u8; 32],
        retained_signed_activity: Option<Vec<u8>>,
    },
    Executed(VerifiedProgramExecution),
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ProgramSimulationEvidence {
    pub boundary_id: [u8; 32],
    pub activity_id: [u8; 32],
    pub previous_state_root: [u8; 32],
    pub hypothetical_state_root: [u8; 32],
    pub observed_sequence: u64,
    pub observed_at: u64,
    pub public_key: [u8; 32],
    pub signature: [u8; 64],
}

pub struct VerifiedProgramSimulation {
    execution: VerifiedProgramExecution,
    evidence: ProgramSimulationEvidence,
}

impl VerifiedProgramSimulation {
    #[must_use]
    pub const fn execution(&self) -> &VerifiedProgramExecution {
        &self.execution
    }

    #[must_use]
    pub const fn evidence(&self) -> ProgramSimulationEvidence {
        self.evidence
    }
}

pub trait ProgramTransport {
    fn discover(
        &self,
        program: [u8; 32],
    ) -> Result<VerifiedProgramDiscovery, ProgramOperationError>;
    fn interface(
        &self,
        program: [u8; 32],
    ) -> Result<VerifiedProgramInterface, ProgramOperationError>;
    fn simulate(
        &self,
        request: &ProgramCallRequest,
    ) -> Result<VerifiedProgramSimulation, ProgramOperationError>;
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

pub struct ProgramOperations<T> {
    transport: T,
}

impl<T: ProgramTransport> ProgramOperations<T> {
    #[must_use]
    pub const fn new(transport: T) -> Self {
        Self { transport }
    }

    pub fn discover(
        &self,
        program: [u8; 32],
    ) -> Result<VerifiedProgramDiscovery, ProgramOperationError> {
        self.transport.discover(program)
    }

    pub fn interface(
        &self,
        program: [u8; 32],
    ) -> Result<VerifiedProgramInterface, ProgramOperationError> {
        self.transport.interface(program)
    }

    pub fn simulate(
        &self,
        request: &ProgramCallRequest,
    ) -> Result<VerifiedProgramSimulation, ProgramOperationError> {
        self.transport.simulate(request)
    }

    pub fn submit(
        &self,
        request: &ProgramCallRequest,
        idempotency_key: [u8; 32],
    ) -> Result<ProgramSubmission, ProgramOperationError> {
        self.transport.submit(request, idempotency_key)
    }

    pub fn receipt(
        &self,
        idempotency_key: [u8; 32],
        expected_activity: [u8; 32],
    ) -> Result<ProgramSubmission, ProgramOperationError> {
        self.transport.receipt(idempotency_key, expected_activity)
    }

    pub fn activity(
        &self,
        activity_id: [u8; 32],
    ) -> Result<ProgramSubmission, ProgramOperationError> {
        self.transport.activity(activity_id)
    }

    #[must_use]
    pub const fn transport(&self) -> &T {
        &self.transport
    }
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
