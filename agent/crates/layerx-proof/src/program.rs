//! Canonical Programs execution verification shared by every Rust surface.

use layerx_programs_runtime::terminal::{
    decode_terminal_payload, CandidateTerminalOutcome, DecodedTerminal, ExecutionTerminal,
    FailureTerminal, PreRuntimeFailure, TerminalDetail, EMPTY_CALL_GRAPH, PRE_RUNTIME_FAILURE,
};
use layerx_programs_runtime::{BudgetMeterRefusal, OccupancySettlement, ProgramFailure};
pub use layerx_programs_runtime::{OccupancyPaymentAccount, MAX_OCCUPANCY_PAYERS};
use layerx_types::intent::{
    ProgramCallOutcome, ProgramCallResponse, ProgramLegacyCallResponse, ProgramLegacyValue,
};
use layerx_wire::limits::{protocol_version_uses_occupancy, STATE_COMMITMENT_PROTOCOL_VERSION};
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
    pub payload_hash: [u8; 32],
    pub program_id: [u8; 32],
    pub guest_abi_version: u16,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct AuthorizedProgramExecutionExpectation {
    pub authority: AuthorizedBatch,
    pub activity_id: [u8; 32],
    pub payload_hash: [u8; 32],
    pub program_id: [u8; 32],
    pub guest_abi_version: u16,
}

/// A DID offered as an occupancy payer of a state-commitment receipt.
///
/// The DID and the optional account identifier are untrusted input. The
/// verifier only uses an account it derives from the DID, for a payer whose
/// signed settlement identifier is the DID's identifier.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct OccupancyPayer<'a> {
    pub did: &'a [u8],
    pub account: Option<[u8; 32]>,
}

/// Proves one offered payer against the receipt's occupancy asset: the named
/// account when one is offered, otherwise both accounts the DID derives.
///
/// # Errors
///
/// Refuses a DID that derives no payer and every account identifier that is
/// not derivable from the DID and the asset.
pub fn prove_occupancy_payer(
    payer: &OccupancyPayer<'_>,
    asset: [u8; 32],
) -> Result<Vec<OccupancyPaymentAccount>, ProgramExecutionVerificationFailure> {
    let failure = |_| ProgramExecutionVerificationFailure::at(ProgramExecutionCheck::Occupancy);
    match payer.account {
        Some(account) => Ok(vec![OccupancyPaymentAccount::prove(
            payer.did, asset, account,
        )
        .map_err(failure)?]),
        None => Ok(vec![
            OccupancyPaymentAccount::main(payer.did, asset).map_err(failure)?,
            OccupancyPaymentAccount::asset(payer.did, asset).map_err(failure)?,
        ]),
    }
}

fn proven_paying_accounts(
    settlement: &OccupancySettlement,
    payers: &[OccupancyPayer<'_>],
    asset: [u8; 32],
) -> Result<Vec<OccupancyPaymentAccount>, ProgramExecutionVerificationFailure> {
    let paying: Vec<[u8; 32]> = settlement
        .payer_dispositions()
        .map_err(|_| ProgramExecutionVerificationFailure::at(ProgramExecutionCheck::Occupancy))?
        .into_iter()
        .filter(|(_, (_, paid, _, _))| *paid != 0)
        .map(|(payer, _)| payer.bytes())
        .collect();
    let mut accounts = Vec::new();
    if paying.is_empty() || paying.len() > MAX_OCCUPANCY_PAYERS {
        return Ok(accounts);
    }
    for payer in payers {
        let Ok(main) = OccupancyPaymentAccount::main(payer.did, asset) else {
            continue;
        };
        if paying.contains(&main.payer().bytes()) {
            accounts.extend(prove_occupancy_payer(payer, asset)?);
        }
    }
    Ok(accounts)
}

pub struct VerifiedProgramExecution {
    receipt: VerifiedReceipt,
    occupancy_payment_accounts: Vec<OccupancyPaymentAccount>,
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

    /// The proven payment accounts the signed occupancy transfer root commits.
    #[must_use]
    pub fn occupancy_payment_accounts(&self) -> &[OccupancyPaymentAccount] {
        &self.occupancy_payment_accounts
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
    verify_program_execution_with_payers(receipt, terminal_payload, call_graph, expected, &[])
}

/// Verifies like [`verify_program_execution`] and proves the offered occupancy
/// payers, which a state-commitment receipt with a paid charge requires.
///
/// # Errors
///
/// Returns the first failed proof boundary without returning a partial outcome.
pub fn verify_program_execution_with_payers(
    receipt: &[u8],
    terminal_payload: &[u8],
    call_graph: &[u8],
    expected: ProgramExecutionExpectation,
    occupancy_payers: &[OccupancyPayer<'_>],
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
        expected.payload_hash,
        expected.program_id,
        expected.guest_abi_version,
        occupancy_payers,
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
    expected: &AuthorizedProgramExecutionExpectation,
) -> Result<VerifiedProgramExecution, ProgramExecutionVerificationFailure> {
    verify_authorized_program_execution_with_payers(
        receipt,
        terminal_payload,
        call_graph,
        expected,
        &[],
    )
}

/// Verifies like [`verify_authorized_program_execution`] and proves the offered
/// occupancy payers, which a state-commitment receipt with a paid charge
/// requires.
///
/// # Errors
///
/// Returns the first failed receipt or terminal proof boundary.
pub fn verify_authorized_program_execution_with_payers(
    receipt: &[u8],
    terminal_payload: &[u8],
    call_graph: &[u8],
    expected: &AuthorizedProgramExecutionExpectation,
    occupancy_payers: &[OccupancyPayer<'_>],
) -> Result<VerifiedProgramExecution, ProgramExecutionVerificationFailure> {
    let verified = verify_program_outcome(receipt, &expected.authority)
        .map_err(|_| ProgramExecutionVerificationFailure::at(ProgramExecutionCheck::Receipt))?;
    verify_program_execution_receipt(
        verified,
        terminal_payload,
        call_graph,
        expected.activity_id,
        expected.payload_hash,
        expected.program_id,
        expected.guest_abi_version,
        occupancy_payers,
    )
}

fn verify_program_execution_receipt(
    verified: VerifiedReceipt,
    terminal_payload: &[u8],
    call_graph: &[u8],
    expected_activity_id: [u8; 32],
    expected_payload_hash: [u8; 32],
    expected_program_id: [u8; 32],
    expected_guest_abi_version: u16,
    occupancy_payers: &[OccupancyPayer<'_>],
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
    let pre_runtime = terminal_payload.starts_with(PRE_RUNTIME_FAILURE);
    let terminal_detail = if outcome.encoding_version() == 4 && !pre_runtime {
        let (detail, legs) = layerx_wire::receipt::decode_applied_terminal(terminal_payload)
            .map_err(|_| {
                ProgramExecutionVerificationFailure::at(ProgramExecutionCheck::Terminal)
            })?;
        if <[u8; 32]>::from(Sha256::digest(legs)) != outcome.applied_legs_digest() {
            return Err(ProgramExecutionVerificationFailure::at(
                ProgramExecutionCheck::TransferAuthority,
            ));
        }
        layerx_programs_runtime::transfer::verify_applied_kernel_legs(
            legs,
            outcome.transfer_root(),
        )
        .map_err(|_| {
            ProgramExecutionVerificationFailure::at(ProgramExecutionCheck::TransferAuthority)
        })?;
        detail
    } else {
        terminal_payload
    };
    let terminal = decode_terminal_payload(
        outcome.terminal_kind(),
        outcome.abi_version(),
        terminal_detail,
    )
    .map_err(|_| ProgramExecutionVerificationFailure::at(ProgramExecutionCheck::Terminal))?;
    if let TerminalDetail::Failure(FailureTerminal::PreRuntime(failure)) = &terminal.detail {
        if !pre_runtime {
            return terminal_failure();
        }
        verify_pre_runtime(failure, protocol, expected_payload_hash, call_graph)?;
    }
    let occupancy_payment_accounts = verify_terminal_commitments(
        &terminal,
        call_graph,
        protocol.protocol_version(),
        outcome,
        occupancy_payers,
    )?;
    let (typed_outcome, authenticated_failure, authenticated_resource) =
        verified_terminal_outcome(&terminal, terminal_payload, expected_program_id, outcome)?;
    Ok(VerifiedProgramExecution {
        occupancy_payment_accounts,
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

fn verify_pre_runtime(
    failure: &PreRuntimeFailure,
    protocol: &layerx_wire::receipt::ProtocolReceipt,
    expected_payload_hash: [u8; 32],
    graph: &[u8],
) -> Result<(), ProgramExecutionVerificationFailure> {
    let outcome = protocol
        .program_outcome()
        .ok_or_else(|| ProgramExecutionVerificationFailure::at(ProgramExecutionCheck::Receipt))?;
    if failure.activity_id != protocol.activity_id()
        || failure.payload_hash != expected_payload_hash
        || failure.result_code != protocol.result_code()
        || failure.result_code != outcome.result_code()
        || failure.module_version != protocol.module_version()
        || failure.parameter_version != protocol.parameter_version()
        || failure.encoding_version != outcome.encoding_version()
        || protocol.module_id() != 9
        || protocol.operation() != 3
        || outcome.terminal_kind() != 2
        || outcome.runtime_version() != 1
        || !matches!(
            (protocol.protocol_version(), outcome.encoding_version()),
            (2, 3) | (3, 4)
        )
        || outcome.memory_bytes() != 0
        || outcome.storage_read_bytes() != 0
        || outcome.output_values() != 0
        || outcome.output_bytes() != 0
    {
        return terminal_failure();
    }
    let empty_digest = if outcome.encoding_version() == 4 {
        Sha256::digest([]).into()
    } else {
        [0; 32]
    };
    if failure.applied_legs_digest != empty_digest
        || outcome.applied_legs_digest() != empty_digest
        || outcome.transfer_root() != [0; 32]
    {
        return Err(ProgramExecutionVerificationFailure::at(
            ProgramExecutionCheck::TransferAuthority,
        ));
    }
    if outcome.occupancy_asset_id() != [0; 32]
        || outcome.occupancy_evidence_digest() != [0; 32]
        || outcome.occupancy_transfer_root() != [0; 32]
        || outcome.occupancy_byte_batches() != 0
        || outcome.occupancy_fee_units() != 0
    {
        return Err(ProgramExecutionVerificationFailure::at(
            ProgramExecutionCheck::Occupancy,
        ));
    }
    if graph != EMPTY_CALL_GRAPH {
        return Err(ProgramExecutionVerificationFailure::at(
            ProgramExecutionCheck::CallGraph,
        ));
    }
    Ok(())
}

type TerminalOutcome = (
    ProgramCallOutcome,
    Option<ProgramFailure>,
    Option<BudgetMeterRefusal>,
);

fn guest_refused(outcome: &ProgramOutcome) -> ProgramCallOutcome {
    ProgramCallOutcome::Refused(layerx_types::intent::ProgramCallFailure::GuestRefused {
        code: outcome.result_code(),
    })
}

fn verify_terminal_representation(
    terminal: &DecodedTerminal,
    raw: &[u8],
    outcome: &ProgramOutcome,
) -> Result<(), ProgramExecutionVerificationFailure> {
    if <[u8; 32]>::from(Sha256::digest(raw)) != outcome.terminal_payload_root() {
        return terminal_failure();
    }
    let detail = if outcome.encoding_version() == 4 && !raw.starts_with(PRE_RUNTIME_FAILURE) {
        layerx_wire::receipt::decode_applied_terminal(raw)
            .map_err(|_| ProgramExecutionVerificationFailure::at(ProgramExecutionCheck::Terminal))?
            .0
    } else {
        raw
    };
    let canonical = decode_terminal_payload(outcome.terminal_kind(), outcome.abi_version(), detail)
        .map_err(|_| ProgramExecutionVerificationFailure::at(ProgramExecutionCheck::Terminal))?;
    if &canonical != terminal {
        return terminal_failure();
    }
    Ok(())
}

fn verified_terminal_outcome(
    terminal: &DecodedTerminal,
    raw: &[u8],
    expected_program: [u8; 32],
    outcome: &ProgramOutcome,
) -> Result<TerminalOutcome, ProgramExecutionVerificationFailure> {
    verify_terminal_representation(terminal, raw, outcome)?;
    match &terminal.detail {
        TerminalDetail::Execution(ExecutionTerminal::CandidateV4 {
            program,
            abi_version,
            runtime_version,
            fee_schedule_version,
            metering_schedule_version,
            usage,
            outcome: candidate_outcome,
            ..
        }) => {
            let candidate = CandidateIdentity {
                program: *program,
                abi: *abi_version,
                runtime: *runtime_version,
                fee: *fee_schedule_version,
                metering: *metering_schedule_version,
                usage: *usage,
            };
            if !candidate_matches(&candidate, expected_program, outcome) {
                return terminal_failure();
            }
            match candidate_outcome {
                CandidateTerminalOutcome::Success { code, response } => {
                    if outcome.result_code() != 0 {
                        return terminal_failure();
                    }
                    let response = ProgramCallResponse::new(*code, response).map_err(|_| {
                        ProgramExecutionVerificationFailure::at(ProgramExecutionCheck::Terminal)
                    })?;
                    Ok((ProgramCallOutcome::Completed(response), None, None))
                }
                CandidateTerminalOutcome::Failure(failure) => {
                    Ok((guest_refused(outcome), Some(failure.clone()), None))
                }
                CandidateTerminalOutcome::Resource(resource) => Ok((
                    ProgramCallOutcome::Refused(layerx_types::intent::ProgramCallFailure::Resource),
                    None,
                    Some(*resource),
                )),
            }
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
        )) => Ok((guest_refused(outcome), Some(failure.clone()), None)),
        TerminalDetail::Failure(_) => Ok((guest_refused(outcome), None, None)),
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
    occupancy_payers: &[OccupancyPayer<'_>],
) -> Result<Vec<OccupancyPaymentAccount>, ProgramExecutionVerificationFailure> {
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
    if !protocol_version_uses_occupancy(protocol_version) {
        return Err(ProgramExecutionVerificationFailure::at(
            ProgramExecutionCheck::Receipt,
        ));
    }
    let occupancy_required =
        protocol_version_uses_occupancy(protocol_version) && successful_execution;
    let mut occupancy_seen = false;
    let mut occupancy_present = false;
    let mut occupancy_payment_accounts = Vec::new();
    let authority_required = candidate || outcome.encoding_version() == 4 && successful_execution;
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
                if let Some(accounts) =
                    verify_occupancy_attachment(bytes, protocol_version, outcome, occupancy_payers)?
                {
                    occupancy_present = true;
                    occupancy_payment_accounts = accounts;
                }
            }
            layerx_programs_runtime::terminal::TerminalAttachment::TransferAuthority {
                authorization,
                transfer_root,
            } => {
                if !authority_required || authority_seen {
                    return Err(ProgramExecutionVerificationFailure::at(
                        ProgramExecutionCheck::TransferAuthority,
                    ));
                }
                verify_transfer_authority_attachment(authorization, *transfer_root, outcome)?;
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
    if authority_required && authority_seen != (outcome.transfer_root() != [0; 32]) {
        return Err(ProgramExecutionVerificationFailure::at(
            ProgramExecutionCheck::TransferAuthority,
        ));
    }
    Ok(occupancy_payment_accounts)
}

fn verify_occupancy_attachment(
    bytes: &[u8],
    protocol_version: u16,
    outcome: &ProgramOutcome,
    occupancy_payers: &[OccupancyPayer<'_>],
) -> Result<Option<Vec<OccupancyPaymentAccount>>, ProgramExecutionVerificationFailure> {
    let occupancy_failure =
        || ProgramExecutionVerificationFailure::at(ProgramExecutionCheck::Occupancy);
    if bytes.is_empty() {
        if outcome.occupancy_evidence_digest() != [0; 32]
            || outcome.occupancy_transfer_root() != [0; 32]
            || outcome.occupancy_byte_batches() != 0
            || outcome.occupancy_fee_units() != 0
        {
            return Err(occupancy_failure());
        }
        return Ok(None);
    }
    if <[u8; 32]>::from(Sha256::digest(bytes)) != outcome.occupancy_evidence_digest() {
        return Err(occupancy_failure());
    }
    let settlement =
        OccupancySettlement::canonical_decode(bytes).map_err(|_| occupancy_failure())?;
    if settlement.usage().byte_batches != outcome.occupancy_byte_batches()
        || settlement.usage().fee_units != outcome.occupancy_fee_units()
    {
        return Err(occupancy_failure());
    }
    let accounts = if protocol_version == STATE_COMMITMENT_PROTOCOL_VERSION {
        proven_paying_accounts(&settlement, occupancy_payers, outcome.occupancy_asset_id())?
    } else {
        Vec::new()
    };
    settlement
        .verify_transfer_root(
            protocol_version,
            outcome.occupancy_asset_id(),
            &accounts,
            outcome.occupancy_transfer_root(),
        )
        .map(Some)
        .map_err(|_| occupancy_failure())
}

fn verify_transfer_authority_attachment(
    authorization: &[u8],
    transfer_root: [u8; 32],
    outcome: &ProgramOutcome,
) -> Result<(), ProgramExecutionVerificationFailure> {
    if transfer_root != outcome.transfer_root()
        || layerx_programs_runtime::transfer::verify_authorization_root(
            authorization,
            transfer_root,
        )
        .is_err()
    {
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

struct CandidateIdentity {
    program: [u8; 32],
    abi: u16,
    runtime: u16,
    fee: u32,
    metering: u32,
    usage: layerx_programs_runtime::MeteredUsage,
}

fn candidate_matches(
    candidate: &CandidateIdentity,
    expected_program: [u8; 32],
    outcome: &ProgramOutcome,
) -> bool {
    candidate.program == expected_program
        && candidate.abi == outcome.abi_version()
        && candidate.runtime == outcome.runtime_version()
        && candidate.fee == outcome.fee_schedule_version()
        && candidate.metering == outcome.metering_schedule_version()
        && usage_matches(candidate.usage, outcome)
}

#[cfg(test)]
mod terminal_binding_tests {
    use super::*;

    fn bytes(field: &str) -> Vec<u8> {
        let document = include_str!(
            "../../../../platform/sdk/conformance/fixtures/receipt-programs-executed-v4.json"
        );
        let marker = format!("\"{field}\": \"");
        let (_, rest) = document
            .split_once(&marker)
            .unwrap_or_else(|| panic!("{field}"));
        let value = rest.split('"').next().unwrap_or_else(|| panic!("{field}"));
        value
            .as_bytes()
            .chunks_exact(2)
            .map(|pair| {
                u8::from_str_radix(
                    std::str::from_utf8(pair).unwrap_or_else(|error| panic!("{error}")),
                    16,
                )
                .unwrap_or_else(|error| panic!("{error}"))
            })
            .collect()
    }

    #[test]
    fn successful_terminal_code_is_bound_to_the_signed_outcome() {
        let receipt = layerx_wire::receipt::decode(&bytes("canonical_receipt_hex"))
            .unwrap_or_else(|error| panic!("{error:?}"));
        let outcome = receipt
            .protocol()
            .and_then(layerx_wire::receipt::ProtocolReceipt::program_outcome)
            .unwrap_or_else(|| panic!("Programs outcome"));
        let raw = bytes("terminal_payload_hex");
        let (detail, _) = layerx_wire::receipt::decode_applied_terminal(&raw)
            .unwrap_or_else(|error| panic!("{error:?}"));
        let mut terminal =
            decode_terminal_payload(outcome.terminal_kind(), outcome.abi_version(), detail)
                .unwrap_or_else(|error| panic!("{error:?}"));
        let program: [u8; 32] = bytes("program_id_hex")
            .try_into()
            .unwrap_or_else(|_| panic!("program"));
        assert!(verified_terminal_outcome(&terminal, &raw, program, outcome).is_ok());
        let TerminalDetail::Execution(ExecutionTerminal::CandidateV4 {
            outcome: CandidateTerminalOutcome::Success { code, .. },
            ..
        }) = &mut terminal.detail
        else {
            panic!("success terminal");
        };
        *code = code
            .checked_add(1)
            .unwrap_or_else(|| panic!("terminal code"));
        assert_eq!(
            verified_terminal_outcome(&terminal, &raw, program, outcome)
                .err()
                .map(|error| error.check),
            Some(ProgramExecutionCheck::Terminal)
        );
    }
}

#[cfg(test)]
mod occupancy_payer_tests {
    use super::*;
    use layerx_types::account::AccountId;
    use layerx_types::ids::Did;

    const SELLER_DID: &str =
        "did:layerx:c4420d73f7b2e56599e25f99790d680f84348adf420eed89b2484b2ece345e64";

    fn native_asset() -> [u8; 32] {
        let mut asset = [0_u8; 32];
        asset[0] = 1;
        asset
    }

    fn hex(bytes: &[u8]) -> String {
        bytes.iter().map(|byte| format!("{byte:02x}")).collect()
    }

    #[test]
    fn proven_payment_accounts_are_the_wire_derivations_of_the_payer_did() {
        let asset = native_asset();
        let did = Did::new(SELLER_DID.as_bytes()).unwrap_or_else(|error| panic!("{error:?}"));
        let payer = layerx_wire::hash::did_id_for_protocol(&did, STATE_COMMITMENT_PROTOCOL_VERSION)
            .unwrap_or_else(|error| panic!("{error:?}"));
        assert_eq!(
            hex(&payer),
            "b3ae288574ffc1a59920de3c7e8b7f5bc79463a205a4df3fae8e7dc516ee4fb3"
        );
        let names = [
            format!("agent:{SELLER_DID}:main"),
            format!("agent:{SELLER_DID}:asset:{}", hex(&asset)),
        ];
        let accounts = prove_occupancy_payer(
            &OccupancyPayer {
                did: SELLER_DID.as_bytes(),
                account: None,
            },
            asset,
        )
        .unwrap_or_else(|error| panic!("{error:?}"));
        assert_eq!(accounts.len(), 2);
        for (account, name) in accounts.iter().zip(&names) {
            let parsed = AccountId::parse(name).unwrap_or_else(|error| panic!("{error:?}"));
            let expected = layerx_wire::hash::account_id_for_protocol(
                &parsed,
                STATE_COMMITMENT_PROTOCOL_VERSION,
            )
            .unwrap_or_else(|error| panic!("{error:?}"));
            assert_eq!(account.account(), expected);
            assert_eq!(account.payer().bytes(), payer);
        }
        assert_eq!(
            hex(&accounts[0].account()),
            "1673e7832a44e5fa42c263e256728b61d28245a6ae13d160dda1954b7a1f2a79"
        );
        let explicit = prove_occupancy_payer(
            &OccupancyPayer {
                did: SELLER_DID.as_bytes(),
                account: Some(accounts[0].account()),
            },
            asset,
        )
        .unwrap_or_else(|error| panic!("{error:?}"));
        assert_eq!(explicit, vec![accounts[0]]);
    }

    #[test]
    fn unrelated_account_identifiers_are_refused_before_use() {
        let asset = native_asset();
        let stranger = prove_occupancy_payer(
            &OccupancyPayer {
                did: b"did:layerx:stranger",
                account: None,
            },
            asset,
        )
        .unwrap_or_else(|error| panic!("{error:?}"));
        for account in [stranger[0].account(), [0x44; 32]] {
            assert_eq!(
                prove_occupancy_payer(
                    &OccupancyPayer {
                        did: SELLER_DID.as_bytes(),
                        account: Some(account),
                    },
                    asset,
                )
                .err()
                .map(|error| error.check),
                Some(ProgramExecutionCheck::Occupancy)
            );
        }
        assert_eq!(
            prove_occupancy_payer(
                &OccupancyPayer {
                    did: SELLER_DID.as_bytes(),
                    account: None,
                },
                [0; 32],
            )
            .err()
            .map(|error| error.check),
            Some(ProgramExecutionCheck::Occupancy)
        );
    }
}
