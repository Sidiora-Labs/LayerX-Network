//! Canonical Programs execution verification shared by every Rust surface.

use layerx_programs_runtime::terminal::{
    decode_terminal_payload, CandidateTerminalOutcome, DecodedTerminal, ExecutionTerminal,
    TerminalDetail,
};
use layerx_programs_runtime::{BudgetMeterRefusal, OccupancySettlement, ProgramFailure};
use layerx_types::intent::{
    ProgramCallOutcome, ProgramCallResponse, ProgramLegacyCallResponse, ProgramLegacyValue,
};
use layerx_wire::limits::PROTOCOL_VERSION;
use layerx_wire::receipt::ProgramOutcome;
use sha2::{Digest as _, Sha256};

use crate::receipt::{
    verify_program_outcome, verify_program_outcome_at_root, AuthorizedBatch, VerifiedReceipt,
};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ProgramExecutionCheck {
    Receipt,
    Activity,
    GuestAbi,
    TerminalPayload,
    Terminal,
    CallGraph,
    Occupancy,
    TransferAuthority,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ProgramExecutionVerificationFailure {
    pub check: ProgramExecutionCheck,
}

impl ProgramExecutionVerificationFailure {
    const fn at(check: ProgramExecutionCheck) -> Self {
        Self { check }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ProgramExecutionExpectation {
    pub sequencer_public_key: [u8; 32],
    pub previous_state_root: [u8; 32],
    pub activity_id: [u8; 32],
    pub program_id: [u8; 32],
    pub guest_abi_version: u16,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct AuthorizedProgramExecutionExpectation {
    pub authority: AuthorizedBatch,
    pub activity_id: [u8; 32],
    pub program_id: [u8; 32],
    pub guest_abi_version: u16,
}

pub struct VerifiedProgramExecution {
    receipt: VerifiedReceipt,
    result_code: i32,
    fee_units: u128,
    cpu_fuel: u64,
    memory_bytes: u64,
    storage_read_bytes: u64,
    storage_write_bytes: u64,
    output_values: u32,
    output_bytes: u64,
    terminal_payload_root: [u8; 32],
    outcome: ProgramCallOutcome,
    authenticated_failure: Option<ProgramFailure>,
    authenticated_resource: Option<BudgetMeterRefusal>,
    terminal: DecodedTerminal,
    call_graph: Vec<u8>,
}

impl VerifiedProgramExecution {
    #[must_use]
    pub const fn receipt(&self) -> &VerifiedReceipt {
        &self.receipt
    }

    #[must_use]
    pub const fn result_code(&self) -> i32 {
        self.result_code
    }

    #[must_use]
    pub const fn fee_units(&self) -> u128 {
        self.fee_units
    }

    #[must_use]
    pub const fn cpu_fuel(&self) -> u64 {
        self.cpu_fuel
    }

    #[must_use]
    pub const fn memory_bytes(&self) -> u64 {
        self.memory_bytes
    }

    #[must_use]
    pub const fn storage_read_bytes(&self) -> u64 {
        self.storage_read_bytes
    }

    #[must_use]
    pub const fn storage_write_bytes(&self) -> u64 {
        self.storage_write_bytes
    }

    #[must_use]
    pub const fn output_values(&self) -> u32 {
        self.output_values
    }

    #[must_use]
    pub const fn output_bytes(&self) -> u64 {
        self.output_bytes
    }

    #[must_use]
    pub const fn terminal_payload_root(&self) -> [u8; 32] {
        self.terminal_payload_root
    }

    #[must_use]
    pub const fn outcome(&self) -> &ProgramCallOutcome {
        &self.outcome
    }

    #[must_use]
    pub const fn authenticated_failure(&self) -> Option<&ProgramFailure> {
        self.authenticated_failure.as_ref()
    }

    #[must_use]
    pub const fn authenticated_resource(&self) -> Option<&BudgetMeterRefusal> {
        self.authenticated_resource.as_ref()
    }

    #[must_use]
    pub const fn terminal(&self) -> &DecodedTerminal {
        &self.terminal
    }

    #[must_use]
    pub fn call_graph(&self) -> &[u8] {
        &self.call_graph
    }
}

/// Verifies the sequencer receipt, signed activity identity, terminal payload,
/// call graph, occupancy evidence, and transfer authority as one atomic value.
///
/// # Errors
///
/// Returns the first failed proof boundary without returning a partial outcome.
pub fn verify_program_execution(
    receipt: &[u8],
    terminal_payload: &[u8],
    call_graph: &[u8],
    expected: ProgramExecutionExpectation,
) -> Result<VerifiedProgramExecution, ProgramExecutionVerificationFailure> {
    let verified = verify_program_outcome_at_root(
        receipt,
        expected.sequencer_public_key,
        expected.previous_state_root,
    )
    .map_err(|_| ProgramExecutionVerificationFailure::at(ProgramExecutionCheck::Receipt))?;
    verify_program_execution_receipt(
        verified,
        terminal_payload,
        call_graph,
        expected.activity_id,
        expected.program_id,
        expected.guest_abi_version,
    )
}

/// Verifies a committed Programs execution against independently supplied
/// batch, asset, state-root, and sequencer authority.
///
/// # Errors
///
/// Returns the first failed receipt or terminal proof boundary.
pub fn verify_authorized_program_execution(
    receipt: &[u8],
    terminal_payload: &[u8],
    call_graph: &[u8],
    expected: AuthorizedProgramExecutionExpectation,
) -> Result<VerifiedProgramExecution, ProgramExecutionVerificationFailure> {
    let verified = verify_program_outcome(receipt, &expected.authority)
        .map_err(|_| ProgramExecutionVerificationFailure::at(ProgramExecutionCheck::Receipt))?;
    verify_program_execution_receipt(
        verified,
        terminal_payload,
        call_graph,
        expected.activity_id,
        expected.program_id,
        expected.guest_abi_version,
    )
}

fn verify_program_execution_receipt(
    verified: VerifiedReceipt,
    terminal_payload: &[u8],
    call_graph: &[u8],
    expected_activity_id: [u8; 32],
    expected_program_id: [u8; 32],
    expected_guest_abi_version: u16,
) -> Result<VerifiedProgramExecution, ProgramExecutionVerificationFailure> {
    let protocol = verified
        .receipt()
        .protocol()
        .ok_or_else(|| ProgramExecutionVerificationFailure::at(ProgramExecutionCheck::Receipt))?;
    if protocol.activity_id() != expected_activity_id {
        return Err(ProgramExecutionVerificationFailure::at(
            ProgramExecutionCheck::Activity,
        ));
    }
    let outcome = protocol
        .program_outcome()
        .ok_or_else(|| ProgramExecutionVerificationFailure::at(ProgramExecutionCheck::Receipt))?;
    if outcome.abi_version() != expected_guest_abi_version {
        return Err(ProgramExecutionVerificationFailure::at(
            ProgramExecutionCheck::GuestAbi,
        ));
    }
    let terminal_digest: [u8; 32] = Sha256::digest(terminal_payload).into();
    if terminal_digest != outcome.terminal_payload_root() {
        return Err(ProgramExecutionVerificationFailure::at(
            ProgramExecutionCheck::TerminalPayload,
        ));
    }
    let terminal = decode_terminal_payload(
        outcome.terminal_kind(),
        outcome.abi_version(),
        terminal_payload,
    )
    .map_err(|_| ProgramExecutionVerificationFailure::at(ProgramExecutionCheck::Terminal))?;
    verify_terminal_commitments(&terminal, call_graph, protocol.protocol_version(), outcome)?;
    let (typed_outcome, authenticated_failure, authenticated_resource) =
        verified_terminal_outcome(&terminal, expected_program_id, outcome)?;
    Ok(VerifiedProgramExecution {
        result_code: outcome.result_code(),
        fee_units: outcome.fee_units(),
        cpu_fuel: outcome.cpu_fuel(),
        memory_bytes: outcome.memory_bytes(),
        storage_read_bytes: outcome.storage_read_bytes(),
        storage_write_bytes: outcome.storage_write_bytes(),
        output_values: outcome.output_values(),
        output_bytes: outcome.output_bytes(),
        terminal_payload_root: outcome.terminal_payload_root(),
        outcome: typed_outcome,
        authenticated_failure,
        authenticated_resource,
        terminal,
        call_graph: call_graph.to_vec(),
        receipt: verified,
    })
}

fn verified_terminal_outcome(
    terminal: &DecodedTerminal,
    expected_program: [u8; 32],
    outcome: &ProgramOutcome,
) -> Result<
    (
        ProgramCallOutcome,
        Option<ProgramFailure>,
        Option<BudgetMeterRefusal>,
    ),
    ProgramExecutionVerificationFailure,
> {
    match &terminal.detail {
        TerminalDetail::Execution(ExecutionTerminal::CandidateV4 {
            program,
            abi_version,
            runtime_version,
            fee_schedule_version,
            metering_schedule_version,
            usage,
            outcome: CandidateTerminalOutcome::Success { code, response },
            ..
        }) => {
            if !candidate_matches(
                *program,
                *abi_version,
                *runtime_version,
                *fee_schedule_version,
                *metering_schedule_version,
                *usage,
                expected_program,
                outcome,
            ) {
                return terminal_failure();
            }
            let response = ProgramCallResponse::new(*code, response).map_err(|_| {
                ProgramExecutionVerificationFailure::at(ProgramExecutionCheck::Terminal)
            })?;
            Ok((ProgramCallOutcome::Completed(response), None, None))
        }
        TerminalDetail::Execution(ExecutionTerminal::CandidateV4 {
            program,
            abi_version,
            runtime_version,
            fee_schedule_version,
            metering_schedule_version,
            usage,
            outcome: CandidateTerminalOutcome::Failure(failure),
            ..
        }) => {
            if !candidate_matches(
                *program,
                *abi_version,
                *runtime_version,
                *fee_schedule_version,
                *metering_schedule_version,
                *usage,
                expected_program,
                outcome,
            ) {
                return terminal_failure();
            }
            Ok((
                ProgramCallOutcome::Refused(
                    layerx_types::intent::ProgramCallFailure::GuestRefused {
                        code: outcome.result_code(),
                    },
                ),
                Some(failure.clone()),
                None,
            ))
        }
        TerminalDetail::Execution(ExecutionTerminal::CandidateV4 {
            program,
            abi_version,
            runtime_version,
            fee_schedule_version,
            metering_schedule_version,
            usage,
            outcome: CandidateTerminalOutcome::Resource(resource),
            ..
        }) => {
            if !candidate_matches(
                *program,
                *abi_version,
                *runtime_version,
                *fee_schedule_version,
                *metering_schedule_version,
                *usage,
                expected_program,
                outcome,
            ) {
                return terminal_failure();
            }
            Ok((
                ProgramCallOutcome::Refused(layerx_types::intent::ProgramCallFailure::Resource),
                None,
                Some(*resource),
            ))
        }
        TerminalDetail::Execution(ExecutionTerminal::Legacy {
            runtime_version,
            abi_version,
            metering_schedule_version,
            usage,
            values,
            ..
        }) => {
            if *abi_version != outcome.abi_version()
                || *runtime_version != outcome.runtime_version()
                || *metering_schedule_version != outcome.metering_schedule_version()
                || !usage_matches(*usage, outcome)
            {
                return terminal_failure();
            }
            let values = values
                .iter()
                .map(|value| match value {
                    layerx_programs_runtime::terminal::ExecutionValue::I32(value) => {
                        ProgramLegacyValue::I32(*value)
                    }
                    layerx_programs_runtime::terminal::ExecutionValue::I64(value) => {
                        ProgramLegacyValue::I64(*value)
                    }
                })
                .collect();
            let response =
                ProgramLegacyCallResponse::new(outcome.result_code(), values).map_err(|_| {
                    ProgramExecutionVerificationFailure::at(ProgramExecutionCheck::Terminal)
                })?;
            Ok((ProgramCallOutcome::LegacyCompleted(response), None, None))
        }
        TerminalDetail::Failure(layerx_programs_runtime::terminal::FailureTerminal::Program(
            failure,
        )) => Ok((
            ProgramCallOutcome::Refused(layerx_types::intent::ProgramCallFailure::GuestRefused {
                code: outcome.result_code(),
            }),
            Some(failure.clone()),
            None,
        )),
        TerminalDetail::Failure(_) => Ok((
            ProgramCallOutcome::Refused(layerx_types::intent::ProgramCallFailure::GuestRefused {
                code: outcome.result_code(),
            }),
            None,
            None,
        )),
        TerminalDetail::Resource(resource) => Ok((
            ProgramCallOutcome::Refused(layerx_types::intent::ProgramCallFailure::Resource),
            None,
            Some(*resource),
        )),
    }
}

fn terminal_failure<T>() -> Result<T, ProgramExecutionVerificationFailure> {
    Err(ProgramExecutionVerificationFailure::at(
        ProgramExecutionCheck::Terminal,
    ))
}

fn verify_terminal_commitments(
    terminal: &DecodedTerminal,
    available_graph: &[u8],
    protocol_version: u16,
    outcome: &ProgramOutcome,
) -> Result<(), ProgramExecutionVerificationFailure> {
    if available_graph.is_empty()
        || <[u8; 32]>::from(Sha256::digest(available_graph)) != outcome.call_graph_root()
    {
        return Err(ProgramExecutionVerificationFailure::at(
            ProgramExecutionCheck::CallGraph,
        ));
    }
    if let TerminalDetail::Execution(ExecutionTerminal::CandidateV4 { graph, .. }) =
        &terminal.detail
    {
        if graph != available_graph {
            return Err(ProgramExecutionVerificationFailure::at(
                ProgramExecutionCheck::CallGraph,
            ));
        }
    }
    let candidate = matches!(
        &terminal.detail,
        TerminalDetail::Execution(ExecutionTerminal::CandidateV4 { .. })
    );
    let successful_execution = outcome.terminal_kind() == 1
        && matches!(
            &terminal.detail,
            TerminalDetail::Execution(
                ExecutionTerminal::Legacy { .. }
                    | ExecutionTerminal::CandidateV4 {
                        outcome: CandidateTerminalOutcome::Success { .. },
                        ..
                    }
            )
        );
    if protocol_version != PROTOCOL_VERSION {
        return Err(ProgramExecutionVerificationFailure::at(
            ProgramExecutionCheck::Receipt,
        ));
    }
    let occupancy_required = protocol_version == 2 && successful_execution;
    let mut occupancy_seen = false;
    let mut occupancy_present = false;
    let mut authority_seen = false;
    for attachment in &terminal.attachments {
        match attachment {
            layerx_programs_runtime::terminal::TerminalAttachment::Occupancy(bytes) => {
                if occupancy_seen || !occupancy_required {
                    return Err(ProgramExecutionVerificationFailure::at(
                        ProgramExecutionCheck::Occupancy,
                    ));
                }
                occupancy_seen = true;
                if bytes.is_empty() {
                    if outcome.occupancy_evidence_digest() != [0; 32]
                        || outcome.occupancy_transfer_root() != [0; 32]
                        || outcome.occupancy_byte_batches() != 0
                        || outcome.occupancy_fee_units() != 0
                    {
                        return Err(ProgramExecutionVerificationFailure::at(
                            ProgramExecutionCheck::Occupancy,
                        ));
                    }
                    continue;
                }
                occupancy_present = true;
                if <[u8; 32]>::from(Sha256::digest(bytes)) != outcome.occupancy_evidence_digest() {
                    return Err(ProgramExecutionVerificationFailure::at(
                        ProgramExecutionCheck::Occupancy,
                    ));
                }
                let settlement = OccupancySettlement::canonical_decode(bytes).map_err(|_| {
                    ProgramExecutionVerificationFailure::at(ProgramExecutionCheck::Occupancy)
                })?;
                if settlement.usage().byte_batches != outcome.occupancy_byte_batches()
                    || settlement.usage().fee_units != outcome.occupancy_fee_units()
                    || settlement
                        .transfer_root(outcome.occupancy_asset_id())
                        .map_err(|_| {
                            ProgramExecutionVerificationFailure::at(
                                ProgramExecutionCheck::Occupancy,
                            )
                        })?
                        != outcome.occupancy_transfer_root()
                {
                    return Err(ProgramExecutionVerificationFailure::at(
                        ProgramExecutionCheck::Occupancy,
                    ));
                }
            }
            layerx_programs_runtime::terminal::TerminalAttachment::TransferAuthority {
                authorization,
                transfer_root,
            } => {
                if !candidate
                    || authority_seen
                    || *transfer_root != outcome.transfer_root()
                    || layerx_programs_runtime::transfer::verify_authorization_root(
                        authorization,
                        *transfer_root,
                    )
                    .is_err()
                {
                    return Err(ProgramExecutionVerificationFailure::at(
                        ProgramExecutionCheck::TransferAuthority,
                    ));
                }
                authority_seen = true;
            }
        }
    }
    if (occupancy_required && !occupancy_seen)
        || occupancy_present != (outcome.occupancy_evidence_digest() != [0; 32])
    {
        return Err(ProgramExecutionVerificationFailure::at(
            ProgramExecutionCheck::Occupancy,
        ));
    }
    if candidate && authority_seen != (outcome.transfer_root() != [0; 32]) {
        return Err(ProgramExecutionVerificationFailure::at(
            ProgramExecutionCheck::TransferAuthority,
        ));
    }
    Ok(())
}

fn usage_matches(usage: layerx_programs_runtime::MeteredUsage, outcome: &ProgramOutcome) -> bool {
    usage.cpu_fuel == outcome.cpu_fuel()
        && usage.memory_bytes == outcome.memory_bytes()
        && usage.storage_read_bytes == outcome.storage_read_bytes()
        && usage.storage_write_bytes == outcome.storage_write_bytes()
        && usage.output_values == outcome.output_values()
        && usage.output_bytes == outcome.output_bytes()
        && usage.fee_units == outcome.fee_units()
}

fn candidate_matches(
    program: [u8; 32],
    abi: u16,
    runtime: u16,
    fee: u32,
    metering: u32,
    usage: layerx_programs_runtime::MeteredUsage,
    expected_program: [u8; 32],
    outcome: &ProgramOutcome,
) -> bool {
    program == expected_program
        && abi == outcome.abi_version()
        && runtime == outcome.runtime_version()
        && fee == outcome.fee_schedule_version()
        && metering == outcome.metering_schedule_version()
        && usage_matches(usage, outcome)
}
