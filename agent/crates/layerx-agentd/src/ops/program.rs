//! First-class program discovery, interface, simulation, and call operations.

use layerx_client::submit::{Submission, SubmitError};
use layerx_proof::receipt::verify_program_outcome_at_root;
use layerx_programs::{ProgramId, ProgramInterface, ProgramLifecycle, VerifiedInterfaceRead, VerifiedProgramHead, VerifiedProtocolHead};
use layerx_programs_runtime::terminal::{decode_terminal_payload, CandidateTerminalOutcome, DecodedTerminal, ExecutionTerminal, TerminalDetail};
use layerx_programs_runtime::{BudgetMeterRefusal, OccupancySettlement, ProgramFailure};
use layerx_types::intent::{ProgramCall, ProgramCallOutcome, ProgramCallResponse, ProgramLegacyCallResponse, ProgramLegacyValue};
use layerx_types::payload::{ModuleId, ModuleRegistry};
use layerx_wire::activity::decode_signed;
use layerx_wire::hash::activity_id;
use sha2::{Digest as _, Sha256};
use layerx_crypto::ed25519;

const SIMULATION_EVIDENCE_DOMAIN: &[u8] = b"LayerX/agent/program-simulation-evidence/v1\0";

use crate::read::LayerxdProgramBalanceReader;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ProgramOperationError {
    InvalidRequest,
    UnknownProgram,
    InactiveProgram,
    Stale,
    UnverifiedReceipt,
    Submit(SubmitError),
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ProgramDiscovery {
    pub program: ProgramId,
    pub lifecycle: ProgramLifecycle,
    pub observed_sequence: u64,
    pub observed_at: u64,
    pub valid_through: u64,
    pub receipt_digest: [u8; 32],
    pub state_root: [u8; 32],
    pub version: u32,
    pub abi_version: u16,
    pub code_hash: [u8; 32],
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ProgramInterfaceRead {
    pub discovery: ProgramDiscovery,
    pub version: u32,
    pub interface: ProgramInterface,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ProgramExecution {
    committed: bool,
    result_code: i32,
    metered_cost: u128,
    fee_units: u128,
    terminal_payload_root: [u8; 32],
    cpu_fuel: u64,
    memory_bytes: u64,
    storage_read_bytes: u64,
    storage_write_bytes: u64,
    output_values: u32,
    output_bytes: u64,
    outcome: Option<ProgramCallOutcome>,
    authenticated_failure: Option<ProgramFailure>,
    authenticated_resource: Option<BudgetMeterRefusal>,
    terminal: DecodedTerminal,
    call_graph: Vec<u8>,
    receipt: Vec<u8>,
}

impl ProgramExecution {
    #[must_use] pub const fn committed(&self) -> bool { self.committed }
    #[must_use] pub const fn result_code(&self) -> i32 { self.result_code }
    #[must_use] pub const fn metered_cost(&self) -> u128 { self.metered_cost }
    #[must_use] pub const fn fee_units(&self) -> u128 { self.fee_units }
    #[must_use] pub const fn terminal_payload_root(&self) -> [u8; 32] { self.terminal_payload_root }
    #[must_use] pub const fn cpu_fuel(&self) -> u64 { self.cpu_fuel }
    #[must_use] pub const fn memory_bytes(&self) -> u64 { self.memory_bytes }
    #[must_use] pub const fn storage_read_bytes(&self) -> u64 { self.storage_read_bytes }
    #[must_use] pub const fn storage_write_bytes(&self) -> u64 { self.storage_write_bytes }
    #[must_use] pub const fn output_values(&self) -> u32 { self.output_values }
    #[must_use] pub const fn output_bytes(&self) -> u64 { self.output_bytes }
    #[must_use] pub const fn outcome(&self) -> Option<&ProgramCallOutcome> { self.outcome.as_ref() }
    #[must_use] pub const fn authenticated_failure(&self) -> Option<&ProgramFailure> { self.authenticated_failure.as_ref() }
    #[must_use] pub const fn authenticated_resource(&self) -> Option<&BudgetMeterRefusal> { self.authenticated_resource.as_ref() }
    #[must_use] pub const fn terminal(&self) -> &DecodedTerminal { &self.terminal }
    #[must_use] pub fn call_graph(&self) -> &[u8] { &self.call_graph }
    #[must_use] pub fn receipt(&self) -> &[u8] { &self.receipt }
}

pub struct RawProgramSimulation {
    pub receipt: Vec<u8>,
    pub terminal_payload: Vec<u8>,
    pub call_graph: Vec<u8>,
    pub evidence: ProgramSimulationEvidence,
    pub evidence_signature: [u8; 64],
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ProgramSimulationEvidence {
    pub boundary_id: [u8; 32],
    pub activity_id: [u8; 32],
    pub previous_state_root: [u8; 32],
    pub hypothetical_state_root: [u8; 32],
    pub observed_sequence: u64,
    pub observed_at: u64,
    pub committed: bool,
}

impl ProgramSimulationEvidence {
    #[must_use]
    pub fn signing_digest(&self) -> [u8; 32] {
        let mut bytes = Vec::with_capacity(SIMULATION_EVIDENCE_DOMAIN.len() + 137);
        bytes.extend_from_slice(SIMULATION_EVIDENCE_DOMAIN);
        bytes.extend_from_slice(&self.boundary_id);
        bytes.extend_from_slice(&self.activity_id);
        bytes.extend_from_slice(&self.previous_state_root);
        bytes.extend_from_slice(&self.hypothetical_state_root);
        bytes.extend_from_slice(&self.observed_sequence.to_be_bytes());
        bytes.extend_from_slice(&self.observed_at.to_be_bytes());
        bytes.push(u8::from(self.committed));
        Sha256::digest(bytes).into()
    }

    fn matches_context(
        &self,
        boundary_id: [u8; 32],
        activity_id: [u8; 32],
        previous_state_root: [u8; 32],
        hypothetical_state_root: [u8; 32],
        observed_sequence: u64,
        observed_at: u64,
    ) -> bool {
        !self.committed && self.boundary_id == boundary_id
            && self.activity_id == activity_id
            && self.previous_state_root == previous_state_root
            && self.hypothetical_state_root == hypothetical_state_root
            && self.observed_sequence == observed_sequence
            && self.observed_at == observed_at
    }
}

pub trait ProgramSimulationTransport {
    fn simulate_exact(
        &mut self,
        signed_activity: &[u8],
    ) -> Result<RawProgramSimulation, ProgramOperationError>;
}

pub struct EmulatorProgramSimulationTransport {
    agent: ureq::Agent,
    endpoint: String,
}

impl EmulatorProgramSimulationTransport {
    #[must_use]
    pub fn connect(endpoint: &str) -> Self {
        let config = ureq::Agent::config_builder()
            .http_status_as_error(false)
            .build();
        Self { agent: config.into(), endpoint: endpoint.trim_end_matches('/').to_owned() }
    }
}

impl ProgramSimulationTransport for EmulatorProgramSimulationTransport {
    fn simulate_exact(&mut self, signed_activity: &[u8]) -> Result<RawProgramSimulation, ProgramOperationError> {
        let url = format!("{}/v1/programs/simulate", self.endpoint);
        let mut response = self.agent.post(&url)
            .send_json(serde_json::json!({"activity": encode_hex(signed_activity)}))
            .map_err(|_| ProgramOperationError::InvalidRequest)?;
        if !response.status().is_success() { return Err(ProgramOperationError::InvalidRequest); }
        let text = response.body_mut().read_to_string().map_err(|_| ProgramOperationError::InvalidRequest)?;
        let document: serde_json::Value = serde_json::from_str(&text).map_err(|_| ProgramOperationError::InvalidRequest)?;
        let result = document.get("result").unwrap_or(&document);
        let evidence = result.get("simulation_evidence").ok_or(ProgramOperationError::UnverifiedReceipt)?;
        Ok(RawProgramSimulation {
            receipt: decode_hex_json(result, "receipt")?,
            terminal_payload: decode_hex_json(result, "terminal_payload")?,
            call_graph: decode_hex_json(result, "call_graph")?,
            evidence: ProgramSimulationEvidence {
                boundary_id: decode_fixed_json(evidence, "boundary_id")?,
                activity_id: decode_fixed_json(evidence, "activity_id")?,
                previous_state_root: decode_fixed_json(evidence, "previous_state_root")?,
                hypothetical_state_root: decode_fixed_json(evidence, "hypothetical_state_root")?,
                observed_sequence: evidence.get("observed_sequence").and_then(serde_json::Value::as_u64).ok_or(ProgramOperationError::UnverifiedReceipt)?,
                observed_at: evidence.get("observed_at").and_then(serde_json::Value::as_u64).ok_or(ProgramOperationError::UnverifiedReceipt)?,
                committed: evidence.get("committed").and_then(serde_json::Value::as_bool).ok_or(ProgramOperationError::UnverifiedReceipt)?,
            },
            evidence_signature: decode_fixed_json(evidence, "signature")?,
        })
    }
}

fn encode_hex(bytes: &[u8]) -> String { bytes.iter().map(|byte| format!("{byte:02x}")).collect() }
fn decode_hex_json(value: &serde_json::Value, field: &str) -> Result<Vec<u8>, ProgramOperationError> {
    let text = value.get(field).and_then(serde_json::Value::as_str).ok_or(ProgramOperationError::UnverifiedReceipt)?;
    if text.len() % 2 != 0 { return Err(ProgramOperationError::UnverifiedReceipt); }
    (0..text.len()).step_by(2).map(|offset| u8::from_str_radix(&text[offset..offset + 2], 16).map_err(|_| ProgramOperationError::UnverifiedReceipt)).collect()
}
fn decode_fixed_json<const N: usize>(value: &serde_json::Value, field: &str) -> Result<[u8; N], ProgramOperationError> {
    decode_hex_json(value, field)?.try_into().map_err(|_| ProgramOperationError::UnverifiedReceipt)
}

pub struct ReceiptVerifiedProgramSimulator<T> {
    transport: T,
    registry: ModuleRegistry,
    expected_abi_version: u16,
    expected_program: ProgramId,
    expected_version: u32,
    expected_code_hash: [u8; 32],
    simulation_public_key: [u8; 32],
    boundary_id: [u8; 32],
    observed_sequence: u64,
    observed_at: u64,
    trusted_previous_state_root: [u8; 32],
}

impl<T: ProgramSimulationTransport> ReceiptVerifiedProgramSimulator<T> {
    #[must_use]
    pub fn new(
        transport: T,
        registry: ModuleRegistry,
        protocol_head: &VerifiedProtocolHead,
        program_head: &VerifiedProgramHead,
    ) -> Self {
        let simulation_public_key = protocol_head.sequencer_public_key();
        let mut boundary = b"LayerX/emulator/simulation-boundary/v1\0".to_vec();
        boundary.extend_from_slice(&simulation_public_key);
        Self { transport, registry, expected_abi_version: program_head.abi_version(),
            expected_program: program_head.program(), expected_version: program_head.version(),
            expected_code_hash: program_head.code_hash(),
            simulation_public_key, boundary_id: Sha256::digest(boundary).into(),
            observed_sequence: program_head.freshness().observed_sequence,
            observed_at: program_head.freshness().observed_at,
            trusted_previous_state_root: protocol_head.state_root() }
    }
}

trait ProgramSimulationBoundary {
    fn simulate_signed(&mut self, signed_activity: &[u8])
        -> Result<ProgramExecution, ProgramOperationError>;
}

impl<T: ProgramSimulationTransport> ProgramSimulationBoundary
    for ReceiptVerifiedProgramSimulator<T>
{
    fn simulate_signed(&mut self, signed_activity: &[u8])
        -> Result<ProgramExecution, ProgramOperationError>
    {
        let raw = self.transport.simulate_exact(signed_activity)?;
        if raw.receipt.is_empty() {
            return Err(ProgramOperationError::UnverifiedReceipt);
        }
        let verified = verify_program_outcome_at_root(&raw.receipt,
            self.simulation_public_key, self.trusted_previous_state_root)
            .map_err(|_| ProgramOperationError::UnverifiedReceipt)?;
        let activity = decode_signed(signed_activity, &self.registry)
            .map_err(|_| ProgramOperationError::InvalidRequest)?;
        let expected = activity_id(&activity)
            .map_err(|_| ProgramOperationError::InvalidRequest)?;
        let protocol = verified.receipt().protocol()
            .ok_or(ProgramOperationError::UnverifiedReceipt)?;
        if !raw.evidence.matches_context(self.boundary_id, expected,
                self.trusted_previous_state_root,
                protocol.resulting_state_root(), self.observed_sequence,
                self.observed_at)
            || ed25519::verify_digest(&self.simulation_public_key,
                &raw.evidence_signature, &raw.evidence.signing_digest()).is_err()
        { return Err(ProgramOperationError::UnverifiedReceipt); }
        if self.observed_sequence.checked_add(1) != Some(protocol.global_sequence()) {
            return Err(ProgramOperationError::UnverifiedReceipt);
        }
        let outcome = protocol.program_outcome()
            .ok_or(ProgramOperationError::UnverifiedReceipt)?;
        if outcome.abi_version() != self.expected_abi_version
            || protocol.module_version() != u32::from(self.expected_abi_version)
        { return Err(ProgramOperationError::UnverifiedReceipt); }
        let payload_digest: [u8; 32] = Sha256::digest(&raw.terminal_payload).into();
        if payload_digest != outcome.terminal_payload_root() {
            return Err(ProgramOperationError::UnverifiedReceipt);
        }
        let terminal = decode_terminal_payload(outcome.terminal_kind(), outcome.abi_version(), &raw.terminal_payload)
            .map_err(|_| ProgramOperationError::UnverifiedReceipt)?;
        verify_terminal_commitments(&terminal, &raw.call_graph, protocol.protocol_version(), outcome)?;
        let (typed_outcome, authenticated_failure, authenticated_resource) = match &terminal.detail {
            TerminalDetail::Execution(ExecutionTerminal::CandidateV4 { program, abi_version,
                runtime_version, fee_schedule_version, metering_schedule_version,
                usage, outcome: CandidateTerminalOutcome::Success { code, response }, .. }) => {
                if *program != self.expected_program.bytes() || *abi_version != outcome.abi_version()
                    || *runtime_version != outcome.runtime_version()
                    || *fee_schedule_version != outcome.fee_schedule_version()
                    || *metering_schedule_version != outcome.metering_schedule_version()
                    || !usage_matches(*usage, outcome)
                { return Err(ProgramOperationError::UnverifiedReceipt); }
                (Some(
                ProgramCallOutcome::Completed(ProgramCallResponse::new(*code, response)
                    .map_err(|_| ProgramOperationError::UnverifiedReceipt)?)), None, None)
            }
            TerminalDetail::Execution(ExecutionTerminal::CandidateV4 { program, abi_version, runtime_version, fee_schedule_version, metering_schedule_version, usage, outcome: CandidateTerminalOutcome::Failure(failure), .. }) => {
                if !candidate_matches(*program,*abi_version,*runtime_version,*fee_schedule_version,*metering_schedule_version,*usage,self.expected_program,outcome) { return Err(ProgramOperationError::UnverifiedReceipt); }
                (Some(ProgramCallOutcome::Refused(layerx_types::intent::ProgramCallFailure::GuestRefused { code: outcome.result_code() })), Some(failure.clone()), None)
            }
            TerminalDetail::Execution(ExecutionTerminal::CandidateV4 { program, abi_version, runtime_version, fee_schedule_version, metering_schedule_version, usage, outcome: CandidateTerminalOutcome::Resource(resource), .. }) => {
                if !candidate_matches(*program,*abi_version,*runtime_version,*fee_schedule_version,*metering_schedule_version,*usage,self.expected_program,outcome) { return Err(ProgramOperationError::UnverifiedReceipt); }
                (Some(ProgramCallOutcome::Refused(layerx_types::intent::ProgramCallFailure::Resource)), None, Some(*resource))
            }
            TerminalDetail::Execution(ExecutionTerminal::Legacy { runtime_version, abi_version, metering_schedule_version, usage, values, .. }) => {
                if *abi_version != outcome.abi_version() || *runtime_version != outcome.runtime_version()
                    || *metering_schedule_version != outcome.metering_schedule_version()
                    || usage.cpu_fuel != outcome.cpu_fuel() || usage.memory_bytes != outcome.memory_bytes()
                    || usage.storage_read_bytes != outcome.storage_read_bytes()
                    || usage.storage_write_bytes != outcome.storage_write_bytes()
                    || usage.output_values != outcome.output_values() || usage.fee_units != outcome.fee_units()
                { return Err(ProgramOperationError::UnverifiedReceipt); }
                let values=values.iter().map(|value|match value{
                    layerx_programs_runtime::terminal::ExecutionValue::I32(value)=>ProgramLegacyValue::I32(*value),
                    layerx_programs_runtime::terminal::ExecutionValue::I64(value)=>ProgramLegacyValue::I64(*value),
                }).collect();
                (Some(ProgramCallOutcome::LegacyCompleted(ProgramLegacyCallResponse::new(outcome.result_code(),values)
                    .map_err(|_|ProgramOperationError::UnverifiedReceipt)?)), None, None)
            }
            TerminalDetail::Failure(layerx_programs_runtime::terminal::FailureTerminal::Program(failure)) =>
                (Some(ProgramCallOutcome::Refused(layerx_types::intent::ProgramCallFailure::GuestRefused { code: outcome.result_code() })), Some(failure.clone()), None),
            TerminalDetail::Failure(_) => (Some(ProgramCallOutcome::Refused(layerx_types::intent::ProgramCallFailure::GuestRefused { code: outcome.result_code() })), None, None),
            TerminalDetail::Resource(resource) => (Some(ProgramCallOutcome::Refused(layerx_types::intent::ProgramCallFailure::Resource)), None, Some(*resource)),
        };
        if protocol.activity_id() != expected {
            return Err(ProgramOperationError::UnverifiedReceipt);
        }
        Ok(ProgramExecution {
            committed: false,
            result_code: outcome.result_code(),
            metered_cost: outcome.fee_units(),
            fee_units: outcome.fee_units(),
            terminal_payload_root: outcome.terminal_payload_root(),
            cpu_fuel: outcome.cpu_fuel(),
            memory_bytes: outcome.memory_bytes(),
            storage_read_bytes: outcome.storage_read_bytes(),
            storage_write_bytes: outcome.storage_write_bytes(),
            output_values: outcome.output_values(),
            output_bytes: outcome.output_bytes(),
            outcome: typed_outcome,
            authenticated_failure,
            authenticated_resource,
            terminal,
            call_graph: raw.call_graph.clone(),
            receipt: verified.canonical_bytes().to_vec(),
        })
    }
}

fn verify_terminal_commitments(terminal:&DecodedTerminal,available_graph:&[u8],protocol_version:u16,outcome:&layerx_wire::receipt::ProgramOutcome)->Result<(),ProgramOperationError>{
    if available_graph.is_empty()||<[u8;32]>::from(Sha256::digest(available_graph))!=outcome.call_graph_root(){return Err(ProgramOperationError::UnverifiedReceipt)}
    if let TerminalDetail::Execution(ExecutionTerminal::CandidateV4{graph,..})=&terminal.detail {if graph!=available_graph{return Err(ProgramOperationError::UnverifiedReceipt)}}
    let candidate=matches!(&terminal.detail,TerminalDetail::Execution(ExecutionTerminal::CandidateV4{..}));
    let successful_execution=outcome.terminal_kind()==1&&matches!(&terminal.detail,TerminalDetail::Execution(ExecutionTerminal::Legacy{..}|ExecutionTerminal::CandidateV4{outcome:CandidateTerminalOutcome::Success{..},..}));
    let occupancy_required=protocol_version==2&&successful_execution;
    if !matches!(protocol_version,1|2){return Err(ProgramOperationError::UnverifiedReceipt)}
    let mut occupancy_seen=false;let mut occupancy_present=false;let mut authority_seen=false;
    for attachment in &terminal.attachments { match attachment {
        layerx_programs_runtime::terminal::TerminalAttachment::Occupancy(bytes)=>{
            if occupancy_seen||!occupancy_required{return Err(ProgramOperationError::UnverifiedReceipt)} occupancy_seen=true;
            if bytes.is_empty(){
                if outcome.occupancy_evidence_digest()!=[0;32]||outcome.occupancy_transfer_root()!=[0;32]
                    ||outcome.occupancy_byte_batches()!=0||outcome.occupancy_fee_units()!=0
                {return Err(ProgramOperationError::UnverifiedReceipt)}
                continue
            }
            occupancy_present=true;
            if <[u8;32]>::from(Sha256::digest(bytes))!=outcome.occupancy_evidence_digest(){return Err(ProgramOperationError::UnverifiedReceipt)}
            let settlement=OccupancySettlement::canonical_decode(bytes).map_err(|_|ProgramOperationError::UnverifiedReceipt)?;
            if settlement.usage().byte_batches!=outcome.occupancy_byte_batches()||settlement.usage().fee_units!=outcome.occupancy_fee_units()
                || settlement.transfer_root(outcome.occupancy_asset_id()).map_err(|_|ProgramOperationError::UnverifiedReceipt)?!=outcome.occupancy_transfer_root(){return Err(ProgramOperationError::UnverifiedReceipt)}
        }
        layerx_programs_runtime::terminal::TerminalAttachment::TransferAuthority{authorization,transfer_root}=>{
            if !candidate||authority_seen||*transfer_root!=outcome.transfer_root()||layerx_programs_runtime::transfer::verify_authorization_root(authorization,*transfer_root).is_err(){return Err(ProgramOperationError::UnverifiedReceipt)}authority_seen=true;
        }
    }}
    if occupancy_required&&!occupancy_seen||occupancy_present!=(outcome.occupancy_evidence_digest()!=[0;32])
        ||candidate&&authority_seen!=(outcome.transfer_root()!=[0;32]){return Err(ProgramOperationError::UnverifiedReceipt)}
    Ok(())
}

fn usage_matches(usage: layerx_programs_runtime::MeteredUsage, outcome: &layerx_wire::receipt::ProgramOutcome) -> bool {
    usage.cpu_fuel == outcome.cpu_fuel() && usage.memory_bytes == outcome.memory_bytes()
        && usage.storage_read_bytes == outcome.storage_read_bytes()
        && usage.storage_write_bytes == outcome.storage_write_bytes()
        && usage.output_values == outcome.output_values() && usage.output_bytes == outcome.output_bytes()
        && usage.fee_units == outcome.fee_units()
}
fn candidate_matches(program:[u8;32],abi:u16,runtime:u16,fee:u32,metering:u32,
    usage:layerx_programs_runtime::MeteredUsage,expected:ProgramId,
    outcome:&layerx_wire::receipt::ProgramOutcome)->bool {
    program==expected.bytes() && abi==outcome.abi_version() && runtime==outcome.runtime_version()
        && fee==outcome.fee_schedule_version() && metering==outcome.metering_schedule_version()
        && usage_matches(usage,outcome)
}

#[cfg(test)]
mod simulation_rejection_vectors {
    use super::ProgramSimulationEvidence;

    fn evidence() -> ProgramSimulationEvidence {
        ProgramSimulationEvidence {
            boundary_id: [1; 32], activity_id: [2; 32],
            previous_state_root: [3; 32], hypothetical_state_root: [4; 32],
            observed_sequence: 5, observed_at: 6, committed: false,
        }
    }

    #[test]
    fn committed_lie_is_refused_before_signature_authority() {
        let mut value = evidence(); value.committed = true;
        assert!(!value.matches_context([1; 32], [2; 32], [3; 32], [4; 32], 5, 6));
    }

    #[test]
    fn stale_discovery_root_is_refused() {
        assert!(!evidence().matches_context([1; 32], [2; 32], [9; 32], [4; 32], 5, 6));
    }

    #[test]
    fn stale_sequence_and_freshness_are_refused() {
        assert!(!evidence().matches_context([1; 32], [2; 32], [3; 32], [4; 32], 7, 8));
    }
}

pub struct ProgramOperations {
    reader: LayerxdProgramBalanceReader,
}

impl ProgramOperations {
    #[must_use]
    pub const fn new(reader: LayerxdProgramBalanceReader) -> Self {
        Self { reader }
    }

    pub fn discover(
        &mut self,
        program: ProgramId,
        now: u64,
        head: &VerifiedProgramHead,
    ) -> Result<ProgramDiscovery, ProgramOperationError> {
        let state = self
            .reader
            .read_protocol_state(program, now)
            .map_err(|_| ProgramOperationError::UnknownProgram)?;
        let balances = state.balances();
        let freshness = balances.freshness();
        let valid_through = freshness
            .observed_at
            .checked_add(self.reader.staleness_limit())
            .ok_or(ProgramOperationError::Stale)?;
        if now > valid_through {
            return Err(ProgramOperationError::Stale);
        }
        if balances.lifecycle() != ProgramLifecycle::Active {
            return Err(ProgramOperationError::InactiveProgram);
        }
        if head.program() != program || head.lifecycle() != ProgramLifecycle::Active
            || head.receipt_digest() != balances.receipt_digest()
            || head.state_root() != balances.state_root()
            || head.freshness() != freshness
            || now > head.valid_until_ms()
        { return Err(ProgramOperationError::UnverifiedReceipt); }
        Ok(ProgramDiscovery {
            program,
            lifecycle: balances.lifecycle(),
            observed_sequence: freshness.observed_sequence,
            observed_at: freshness.observed_at,
            valid_through,
            receipt_digest: balances.receipt_digest(),
            state_root: balances.state_root(),
            version: head.version(),
            abi_version: head.abi_version(),
            code_hash: head.code_hash(),
        })
    }

    pub fn interface(
        &mut self,
        program: ProgramId,
        now: u64,
        verified: VerifiedInterfaceRead,
        head: &VerifiedProgramHead,
    ) -> Result<ProgramInterfaceRead, ProgramOperationError> {
        let discovery = self.discover(program, now, head)?;
        if verified.program != program
            || verified.receipt_digest != discovery.receipt_digest
            || verified.state_root != discovery.state_root
            || verified.freshness.observed_sequence != discovery.observed_sequence
            || verified.freshness.observed_at != discovery.observed_at
        {
            return Err(ProgramOperationError::UnverifiedReceipt);
        }
        Ok(ProgramInterfaceRead {
            discovery,
            version: verified.version,
            interface: verified.interface,
        })
    }

    pub fn simulate(
        &mut self,
        boundary: &mut ReceiptVerifiedProgramSimulator<impl ProgramSimulationTransport>,
        call: &ProgramCall,
        signed_activity: &[u8],
        now: u64,
        head: &VerifiedProgramHead,
    ) -> Result<ProgramExecution, ProgramOperationError> {
        let program = ProgramId::new(call.callee().bytes())
            .map_err(|_| ProgramOperationError::InvalidRequest)?;
        let discovery = self.discover(program, now, head)?;
        if boundary.trusted_previous_state_root != discovery.state_root
            || boundary.expected_abi_version != discovery.abi_version
            || boundary.expected_program != discovery.program
            || boundary.expected_version != discovery.version
            || boundary.expected_code_hash != discovery.code_hash
            || boundary.observed_sequence != discovery.observed_sequence
            || boundary.observed_at != discovery.observed_at
        { return Err(ProgramOperationError::UnverifiedReceipt); }
        validate_call_activity(boundary.registry(), call, signed_activity)?;
        let execution = boundary.simulate_signed(signed_activity)?;
        if execution.committed() || execution.receipt().is_empty() {
            return Err(ProgramOperationError::UnverifiedReceipt);
        }
        Ok(execution)
    }

    pub fn submit(
        &mut self,
        client: &mut layerx_client::Client,
        registry: &ModuleRegistry,
        call: &ProgramCall,
        signer_public_key: [u8; 32],
        correlation_id: u64,
        attempt: u32,
        signed_activity: &[u8],
        now: u64,
        head: &VerifiedProgramHead,
    ) -> Result<Submission, ProgramOperationError> {
        let program = ProgramId::new(call.callee().bytes())
            .map_err(|_| ProgramOperationError::InvalidRequest)?;
        self.discover(program, now, head)?;
        validate_call_activity(registry, call, signed_activity)?;
        client
            .submit_signed(
                registry,
                signer_public_key,
                correlation_id,
                attempt,
                signed_activity,
            )
            .map_err(ProgramOperationError::Submit)
    }
}

impl<T> ReceiptVerifiedProgramSimulator<T> {
    const fn registry(&self) -> &ModuleRegistry { &self.registry }
}

fn validate_call_activity(
    registry: &ModuleRegistry,
    call: &ProgramCall,
    signed_activity: &[u8],
) -> Result<(), ProgramOperationError> {
    let activity = decode_signed(signed_activity, registry)
        .map_err(|_| ProgramOperationError::InvalidRequest)?;
    let kind = activity.activity_type();
    if kind.module() != ModuleId::Programs || kind.ordinal() != 3
        || activity.payload() != call.canonical_payload()
    {
        return Err(ProgramOperationError::InvalidRequest);
    }
    Ok(())
}
