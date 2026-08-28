//! Concrete `wasmi` bindings for the version-one capability ABI.

mod balance;
mod calls;
mod context;
mod crypto;
mod events;
pub(crate) mod memory;
mod scan;
mod signature;
mod storage;
mod transfer;

use std::collections::BTreeMap;

use wasmi::{Caller, Engine, InstancePre, Linker, Module, Store};

use crate::abi::context::{ContextField, ContextRefusal, ExecutionContext};
use crate::abi::response::{CallResponse, ResponseRefusal, ResponseRegion};
use crate::abi::{Abi, AbiError, ReceiptView, ABI_MODULE};
use crate::calls::{CallGraph, Composition, CompositionRefusal};
use crate::crypto::bigint;
use crate::execute::ExecutionFault;
use crate::fault::{ProgramFailure, RefusalClass, RefusalReason};
use crate::meter::Meter;
use crate::meter::inject::{PRIVATE_CHARGE_FUNCTION, PRIVATE_CHECK_FUNCTION, PRIVATE_METER_MODULE};

use self::memory::{nonnegative, read_fixed, write_guest};

pub(super) const STATUS_DENIED: i32 = -1;
pub(super) const STATUS_INVALID: i32 = -2;
pub(super) const STATUS_BOUNDS: i32 = -3;
pub(super) const STATUS_METER: i32 = -4;
pub(super) const STATUS_EVIDENCE: i32 = -5;
pub(super) const STATUS_ABSENT: i32 = -7;
pub(super) const COMPOSITION_REFUSED: &str = "program composition refused the call graph";

fn storage_overlay_entry_bytes(key: &[u8], value: Option<&[u8]>) -> Option<u64> {
    let key_bytes = u64::try_from(key.len()).ok()?;
    match value {
        Some(value) => 9_u64.checked_add(key_bytes)?
            .checked_add(u64::try_from(value.len()).ok()?),
        None => 5_u64.checked_add(key_bytes),
    }
}

fn wasmi_usage(meter: crate::MeteredUsage) -> wasmi::ExecutionMeteredUsage {
    wasmi::ExecutionMeteredUsage {
        cpu_fuel: meter.cpu_fuel,
        memory_bytes: meter.memory_bytes,
        storage_read_bytes: meter.storage_read_bytes,
        storage_write_bytes: meter.storage_write_bytes,
        output_values: meter.output_values,
        output_bytes: meter.output_bytes,
        occupancy_byte_batches: meter.occupancy_byte_batches,
        occupancy_fee_units: meter.occupancy_fee_units,
        fee_units: meter.fee_units,
    }
}

/// Per-execution host state owned by a `Store`; the shared linker holds none of it.
#[derive(Debug)]
pub(crate) struct RuntimeState {
    meter: Meter,
    abi: Option<Abi>,
    composition: Option<Composition>,
    refusal: Option<CompositionRefusal>,
    outcome: Option<CandidateOutcomeRegion>,
    failure_subtree_fuel: Option<u64>,
    failure_graph: Option<CallGraph>,
    protocol_context: Option<ExecutionContext>,
    metering_schedule: crate::FuelSchedule,
    legacy_reference_fuel: bool,
    legacy_reference_engine_committed: u64,
    trace_storage_baseline: crate::storage::Storage,
    v2_host_identity: Option<crate::abi::HostStateIdentity>,
}

#[derive(Debug)]
enum CandidateOutcomeRegion {
    Response(ResponseRegion),
    Failure(ProgramFailure),
}

/// The versioned host surface sealed after its one construction for an engine.
#[derive(Debug)]
pub(crate) struct HostLinker {
    linker: Linker<RuntimeState>,
    construction_count: usize,
    registered_function_count: usize,
}

impl HostLinker {
    pub(crate) fn instantiate(
        &self,
        store: &mut Store<RuntimeState>,
        module: &Module,
    ) -> Result<InstancePre, wasmi::Error> {
        self.linker.instantiate(store, module)
    }

    pub(crate) const fn construction_count(&self) -> usize {
        self.construction_count
    }

    pub(crate) const fn registered_function_count(&self) -> usize {
        self.registered_function_count
    }
}

fn write_execution_fault(
    fault: &ExecutionFault,
    write: &mut dyn FnMut(&[u8]) -> Result<(), AbiError>,
) -> Result<(), AbiError> {
    match fault {
        ExecutionFault::UnknownExport { name } => { write(&[0])?; write(&u32::try_from(name.len()).map_err(|_| AbiError::InvalidEncoding)?.to_be_bytes())?; write(name.as_bytes())?; }
        ExecutionFault::NotAFunction { name } => { write(&[1])?; write(&u32::try_from(name.len()).map_err(|_| AbiError::InvalidEncoding)?.to_be_bytes())?; write(name.as_bytes())?; }
        ExecutionFault::UnreachableExecuted => write(&[2])?,
        ExecutionFault::MemoryOutOfBounds => write(&[3])?,
        ExecutionFault::TableOutOfBounds => write(&[4])?,
        ExecutionFault::IndirectCallToNull => write(&[5])?,
        ExecutionFault::IntegerDivisionByZero => write(&[6])?,
        ExecutionFault::IntegerOverflow => write(&[7])?,
        ExecutionFault::BadConversionToInteger => write(&[8])?,
        ExecutionFault::StackExhausted => write(&[9])?,
        ExecutionFault::BadSignature => write(&[10])?,
        ExecutionFault::OutOfFuel => write(&[11])?,
        ExecutionFault::GrowthLimited => write(&[12])?,
        ExecutionFault::Resource { refusal } => { write(&[13])?; write(&crate::abi::abi_error_bytes(&AbiError::Meter(*refusal)))?; }
        ExecutionFault::NonIntegerValue => write(&[14])?,
        ExecutionFault::EngineFault { .. } => return Err(AbiError::InvalidEncoding),
    }
    Ok(())
}

fn write_composition_refusal(
    refusal: &CompositionRefusal,
    write: &mut dyn FnMut(&[u8]) -> Result<(), AbiError>,
) -> Result<(), AbiError> {
    let revision = |revision: AbiRevision| match revision { AbiRevision::V1 => 1_u8, AbiRevision::V2 => 2_u8 };
    match refusal {
        CompositionRefusal::NotComposable => write(&[0])?,
        CompositionRefusal::ActivityEvidenceRequired => write(&[1])?,
        CompositionRefusal::ActivityEvidenceMismatch => write(&[2])?,
        CompositionRefusal::ActivityEvidenceReused => write(&[3])?,
        CompositionRefusal::WrongVersion { expected, actual } => write(&[4, revision(*expected), revision(*actual)])?,
        CompositionRefusal::MeteringPlanMismatch { expected, actual } => { write(&[5])?; write(expected)?; write(actual)?; }
        CompositionRefusal::UnknownProgram { program } => { write(&[6])?; write(&program.bytes())?; }
        CompositionRefusal::Reentrancy { program } => { write(&[7])?; write(&program.bytes())?; }
        CompositionRefusal::DepthExceeded { limit, attempted } => { write(&[8])?; write(&limit.to_be_bytes())?; write(&attempted.to_be_bytes())?; }
        CompositionRefusal::EdgesExceeded { limit, attempted } => { write(&[9])?; write(&limit.to_be_bytes())?; write(&attempted.to_be_bytes())?; }
        CompositionRefusal::FanoutExceeded { limit, attempted } => { write(&[10])?; write(&limit.to_be_bytes())?; write(&attempted.to_be_bytes())?; }
        CompositionRefusal::VisitsExceeded { program, limit, attempted } => { write(&[11])?; write(&program.bytes())?; write(&limit.to_be_bytes())?; write(&attempted.to_be_bytes())?; }
        CompositionRefusal::MissingEntry => write(&[12])?,
        CompositionRefusal::MissingAllocator => write(&[13])?,
        CompositionRefusal::MissingMemory => write(&[14])?,
        CompositionRefusal::AllocationRefused { code } => { write(&[15])?; write(&code.to_be_bytes())?; }
        CompositionRefusal::InputTooLarge { bytes, limit } => { write(&[16])?; write(&u64::try_from(*bytes).map_err(|_| AbiError::InvalidEncoding)?.to_be_bytes())?; write(&u64::try_from(*limit).map_err(|_| AbiError::InvalidEncoding)?.to_be_bytes())?; }
        CompositionRefusal::GuestRefused { program, code } => { write(&[17])?; write(&program.bytes())?; write(&code.to_be_bytes())?; }
        CompositionRefusal::Program(failure) => { write(&[18])?; let failure = failure.canonical_encode(); write(&u32::try_from(failure.len()).map_err(|_| AbiError::InvalidEncoding)?.to_be_bytes())?; write(&failure)?; }
        CompositionRefusal::Authority(error) => { write(&[19])?; write(&crate::abi::abi_error_bytes(error))?; }
        CompositionRefusal::Fault(fault) => { write(&[20])?; write_execution_fault(fault, write)?; }
        CompositionRefusal::Resource(refusal) => { write(&[21])?; write(&crate::abi::abi_error_bytes(&AbiError::Meter(*refusal)))?; }
        CompositionRefusal::Response(refusal) => {
            write(&[22])?;
            let mut failed = false;
            refusal.canonical_write(|bytes| { if write(bytes).is_err() { failed = true; } });
            if failed { return Err(AbiError::InvalidEncoding) }
        }
    }
    Ok(())
}

impl RuntimeState {
    fn v2_identity(abi: &Abi, baseline: &crate::storage::Storage) -> Option<crate::abi::HostStateIdentity> {
        abi.v2_host_state_identity(baseline).ok()
    }

    fn trace_storage_entries(abi: &Abi) -> crate::storage::Storage {
        abi.storage_snapshot()
    }

    fn v2_host_state(
        &self,
        hash: bool,
    ) -> Result<crate::abi::HostStateCommitment, AbiError> {
        use sha2::{Digest, Sha256};
        let abi_state = self.abi.as_ref().ok_or(AbiError::WrongVersion)?;
        let abi = if hash {
            abi_state.v2_host_state_commitment()?
        } else {
            abi_state.v2_host_state_measurement()?
        };
        let write_state = |write: &mut dyn FnMut(&[u8]) -> Result<(), AbiError>| -> Result<(), AbiError> {
            write(b"LayerX/programs/v2/runtime-host-state\0")?;
            write(&abi.root)?;
            write(&abi.canonical_bytes.to_be_bytes())?;
            let usage = self.meter.execution_trace_usage()?;
            write(&usage.cpu_fuel.to_be_bytes())?;
            write(&usage.memory_bytes.to_be_bytes())?;
            write(&usage.storage_read_bytes.to_be_bytes())?;
            write(&usage.storage_write_bytes.to_be_bytes())?;
            write(&usage.output_values.to_be_bytes())?;
            write(&usage.output_bytes.to_be_bytes())?;
            write(&usage.occupancy_byte_batches.to_be_bytes())?;
            write(&usage.occupancy_fee_units.to_be_bytes())?;
            write(&usage.fee_units.to_be_bytes())?;
            write(&self.meter.cpu_remaining().to_be_bytes())?;
            match self.failure_subtree_fuel { None => write(&[0])?, Some(value) => { write(&[1])?; write(&value.to_be_bytes())?; } }
            write(&self.legacy_reference_engine_committed.to_be_bytes())?;
            write(&self.metering_schedule.canonical_bytes())?;
            write(&[u8::from(self.legacy_reference_fuel)])?;
            match self.protocol_context { None => write(&[0])?, Some(context) => { write(&[1])?; write(&context.canonical_bytes())?; } }
            for graph in [self.composition.as_ref().map(Composition::graph), self.failure_graph.as_ref()] {
                match graph {
                    None => write(&[0])?,
                    Some(graph) => { let graph = graph.canonical_evidence(); write(&[1])?; write(&u64::try_from(graph.len()).map_err(|_| AbiError::InvalidEncoding)?.to_be_bytes())?; write(&graph)?; }
                }
            }
            match &self.refusal {
                None => write(&[0])?,
                Some(refusal) => { write(&[1])?; write_composition_refusal(refusal, write)?; }
            }
            match &self.outcome {
                None => write(&[0])?,
                Some(CandidateOutcomeRegion::Response(response)) => {
                    write(&[1])?;
                    write(&response.canonical_state_len().map_err(|_| AbiError::InvalidEncoding)?.to_be_bytes())?;
                    let mut failed = false;
                    response.canonical_state_write(|bytes| { if write(bytes).is_err() { failed = true; } });
                    if failed { return Err(AbiError::InvalidEncoding) }
                }
                Some(CandidateOutcomeRegion::Failure(failure)) => {
                    let failure = failure.canonical_encode();
                    write(&[2])?; write(&u64::try_from(failure.len()).map_err(|_| AbiError::InvalidEncoding)?.to_be_bytes())?; write(&failure)?;
                }
            }
            Ok(())
        };
        let mut runtime_bytes = 0_u64;
        write_state(&mut |bytes| {
            runtime_bytes = runtime_bytes.checked_add(u64::try_from(bytes.len()).map_err(|_| AbiError::InvalidEncoding)?)
                .ok_or(AbiError::InvalidEncoding)?;
            if runtime_bytes > crate::MAX_ARBITRATION_HOST_STATE_BYTES as u64 { return Err(AbiError::InvalidEncoding) }
            Ok(())
        })?;
        if !hash {
            return Ok(crate::abi::HostStateCommitment {
                root: [0; 32],
                canonical_bytes: abi.canonical_bytes.checked_add(runtime_bytes).ok_or(AbiError::InvalidEncoding)?,
            });
        }
        let mut written = 0_u64;
        let mut hasher = Sha256::new();
        write_state(&mut |bytes| {
            written = written.checked_add(u64::try_from(bytes.len()).map_err(|_| AbiError::InvalidEncoding)?)
                .ok_or(AbiError::InvalidEncoding)?;
            if written > runtime_bytes { return Err(AbiError::InvalidEncoding) }
            hasher.update(bytes);
            Ok(())
        })?;
        if written != runtime_bytes { return Err(AbiError::InvalidEncoding) }
        Ok(crate::abi::HostStateCommitment {
            root: hasher.finalize().into(),
            canonical_bytes: abi.canonical_bytes.checked_add(runtime_bytes).ok_or(AbiError::InvalidEncoding)?,
        })
    }

    pub(crate) fn v2_host_state_commitment(
        &self,
    ) -> Result<crate::abi::HostStateCommitment, AbiError> {
        self.v2_host_state(true)
    }

    pub(crate) fn v2_host_state_measurement(
        &self,
    ) -> Result<crate::abi::HostStateCommitment, AbiError> {
        self.v2_host_state(false)
    }

    pub(crate) fn v2_host_state_identity(
        &self,
    ) -> Result<crate::abi::HostStateIdentity, AbiError> {
        self.abi.as_ref().ok_or(AbiError::WrongVersion)?
            .version();
        self.v2_host_identity.ok_or(AbiError::WrongVersion)
    }

    pub(crate) fn execution_supplement(
        &mut self,
        charge: &mut wasmi::ObservationCharge,
        remaining_bytes: u64,
        remaining_work: u64,
    ) -> Result<wasmi::ExecutionSupplement, wasmi::ExecutionObserverError> {
        if !charge.collect {
            let meter = self.meter.execution_trace_usage()
                .map_err(|_| wasmi::ExecutionObserverError::SupplementRejected)?;
            return Ok(wasmi::ExecutionSupplement {
                storage_overlay: Vec::new(),
                authoritative_fuel: self.meter.cpu_remaining(),
                authoritative_usage: wasmi_usage(meter),
                canonical_state_bytes: 0,
                commitment_fuel: 0,
                arbitration_host_state_root: [0; 32],
                arbitration_host_state_bytes: 0,
                arbitration_base_state_root: [0; 32],
                arbitration_receipt_oracle_root: [0; 32],
                arbitration_balance_oracle_root: [0; 32],
                arbitration_engine_canonical_bytes: 0,
                arbitration_instance_retained_bytes: 0,
                arbitration_canonical_state_bytes: 0,
                arbitration_commitment_fuel: 0,
            });
        }
        let (overlay_entries, overlay_bytes) = if let Some(abi) = self.abi.as_ref() {
            abi.storage_commitment_delta_metrics(&self.trace_storage_baseline)
                .ok_or(wasmi::ExecutionObserverError::SupplementRejected)?
        } else {
            crate::storage::Storage::new().commitment_delta_metrics(&self.trace_storage_baseline)
                .ok_or(wasmi::ExecutionObserverError::SupplementRejected)?
        };
        charge.storage_overlay_bytes = overlay_bytes;
        let (host_state_bytes, host_identity, isolated_host_state) = if self.abi.is_some() {
            let state = self.v2_host_state_measurement().map_err(|_| wasmi::ExecutionObserverError::SupplementRejected)?;
            let identity = self.v2_host_state_identity().map_err(|_| wasmi::ExecutionObserverError::SupplementRejected)?;
            charge.host_state_bytes = state.canonical_bytes.checked_mul(3)
                .ok_or(wasmi::ExecutionObserverError::SupplementRejected)?;
            (state.canonical_bytes, Some(identity), None)
        } else {
            use sha2::{Digest, Sha256};
            let state = crate::abi::HostStateCommitment {
                root: Sha256::digest(b"LayerX/programs/v2/isolated-host-state\0").into(),
                canonical_bytes: b"LayerX/programs/v2/isolated-host-state\0".len() as u64,
            };
            let identity = self.v2_host_identity
                .ok_or(wasmi::ExecutionObserverError::SupplementRejected)?;
            charge.host_state_bytes = state.canonical_bytes.checked_mul(3)
                .ok_or(wasmi::ExecutionObserverError::SupplementRejected)?;
            (state.canonical_bytes, Some(identity), Some(state))
        };
        let retained_instruction_bytes = charge.retained_instruction_bytes;
        let arbitration_instance_retained_bytes = charge.instance_state_bytes;
        let engine_bytes = charge.total_bytes()
            .and_then(|bytes| bytes.checked_sub(retained_instruction_bytes))
            .and_then(|bytes| bytes.checked_sub(charge.host_state_bytes))
            .and_then(|bytes| bytes.checked_sub(arbitration_instance_retained_bytes))
            .ok_or(wasmi::ExecutionObserverError::SupplementRejected)?;
        let snapshot_bytes = 214_u64.checked_add(engine_bytes)
            .ok_or(wasmi::ExecutionObserverError::SupplementRejected)?;
        let retained_bytes = snapshot_bytes.checked_add(retained_instruction_bytes)
            .and_then(|bytes| bytes.checked_add(arbitration_instance_retained_bytes))
            .and_then(|bytes| bytes.checked_add(charge.host_state_bytes))
            .and_then(|bytes| bytes.checked_add(charge.arbitration_engine_canonical_bytes))
            .ok_or(wasmi::ExecutionObserverError::SupplementRejected)?;
        if retained_bytes > remaining_bytes || retained_bytes > remaining_work {
            return Err(wasmi::ExecutionObserverError::SnapshotLimitExceeded);
        }
        let snapshot_fuel = crate::step_commitment_fuel(snapshot_bytes)
            .map_err(|_| wasmi::ExecutionObserverError::SupplementRejected)?;
        let arbitration_engine_bytes = charge.arbitration_engine_canonical_bytes;
        let arbitration_fuel = crate::arbitration_step_commitment_fuel_with_host(
            snapshot_bytes,
            arbitration_engine_bytes,
            host_state_bytes,
        )
            .map_err(|_| wasmi::ExecutionObserverError::SupplementRejected)?;
        let total_fuel = snapshot_fuel.checked_add(arbitration_fuel)
            .ok_or(wasmi::ExecutionObserverError::SupplementRejected)?;
        self.meter.charge_cpu(total_fuel)
            .map_err(|_| wasmi::ExecutionObserverError::SupplementRejected)?;
        let host_state = match isolated_host_state {
            Some(state) => state,
            None => {
                let state = self.v2_host_state_commitment()
                    .map_err(|_| wasmi::ExecutionObserverError::SupplementRejected)?;
                if state.canonical_bytes != host_state_bytes {
                    return Err(wasmi::ExecutionObserverError::SupplementRejected)
                }
                state
            }
        };
        charge.value_bytes = snapshot_bytes.checked_add(retained_instruction_bytes)
            .ok_or(wasmi::ExecutionObserverError::SupplementRejected)?;
        charge.frame_bytes = 0;
        charge.local_bytes = 0;
        charge.global_bytes = 0;
        charge.memory_bytes = 0;
        charge.storage_overlay_bytes = 0;
        charge.instruction_bytes = 0;
        charge.retained_instruction_bytes = 0;
        let meter = self.meter.execution_trace_usage()
            .map_err(|_| wasmi::ExecutionObserverError::SupplementRejected)?;
        let mut storage_overlay = Vec::with_capacity(overlay_entries);
        if let Some(abi) = self.abi.as_ref() {
            abi.for_each_storage_commitment_delta(&self.trace_storage_baseline, |key, value| {
                storage_overlay.push((key, value.map(<[u8]>::to_vec)));
            });
        } else {
            crate::storage::Storage::new().for_each_commitment_delta(
                &self.trace_storage_baseline,
                |key, value| storage_overlay.push((key, value.map(<[u8]>::to_vec))),
            );
        }
        storage_overlay.sort_by(|left, right| left.0.cmp(&right.0));
        Ok(wasmi::ExecutionSupplement {
            storage_overlay,
            authoritative_fuel: self.meter.cpu_remaining(),
            authoritative_usage: wasmi_usage(meter),
            canonical_state_bytes: snapshot_bytes,
            commitment_fuel: snapshot_fuel,
            arbitration_host_state_root: host_state.root,
            arbitration_host_state_bytes: host_state.canonical_bytes,
            arbitration_base_state_root: host_identity.map_or([0; 32], |identity| identity.base_state_root),
            arbitration_receipt_oracle_root: host_identity.map_or([0; 32], |identity| identity.receipt_oracle_root),
            arbitration_balance_oracle_root: host_identity.map_or([0; 32], |identity| identity.balance_oracle_root),
            arbitration_engine_canonical_bytes: arbitration_engine_bytes,
            arbitration_instance_retained_bytes,
            arbitration_canonical_state_bytes: crate::arbitration_step_state_bytes(snapshot_bytes, arbitration_engine_bytes).map_err(|_| wasmi::ExecutionObserverError::SupplementRejected)?,
            arbitration_commitment_fuel: arbitration_fuel,
        })
    }

    pub(crate) fn isolated(meter: Meter) -> Self {
        use sha2::{Digest, Sha256};
        Self {
            meter,
            abi: None,
            composition: None,
            refusal: None,
            outcome: None,
            failure_subtree_fuel: None,
            failure_graph: None,
            protocol_context: None,
            metering_schedule: crate::FuelSchedule::WASMI_0_31_2,
            legacy_reference_fuel: false,
            legacy_reference_engine_committed: 0,
            trace_storage_baseline: crate::storage::Storage::new(),
            v2_host_identity: Some(crate::abi::HostStateIdentity {
                base_state_root: Sha256::digest(b"LayerX/programs/v2/isolated-base-state\0").into(),
                receipt_oracle_root: Sha256::digest(b"LayerX/programs/v2/isolated-receipt-oracle\0").into(),
                balance_oracle_root: Sha256::digest(b"LayerX/programs/v2/isolated-balance-oracle\0").into(),
            }),
        }
    }

    pub(crate) fn composed(meter: Meter, abi: Abi, composition: Composition) -> Self {
        let trace_storage_baseline = Self::trace_storage_entries(&abi);
        let v2_host_identity = Self::v2_identity(&abi, &trace_storage_baseline);
        Self {
            meter,
            abi: Some(abi),
            composition: Some(composition),
            refusal: None,
            outcome: None,
            failure_subtree_fuel: None,
            failure_graph: None,
            protocol_context: None,
            metering_schedule: crate::FuelSchedule::WASMI_0_31_2,
            legacy_reference_fuel: false,
            legacy_reference_engine_committed: 0,
            trace_storage_baseline,
            v2_host_identity,
        }
    }

    pub(crate) fn sandbox(meter: Meter, abi: Abi) -> Self {
        let mut state = Self::isolated(meter);
        state.trace_storage_baseline = Self::trace_storage_entries(&abi);
        state.v2_host_identity = Self::v2_identity(&abi, &state.trace_storage_baseline);
        state.abi = Some(abi);
        state
    }

    pub(crate) fn composed_with_response(
        meter: Meter,
        abi: Abi,
        composition: Composition,
        capacity: usize,
    ) -> Result<Self, ResponseRefusal> {
        let trace_storage_baseline = Self::trace_storage_entries(&abi);
        let v2_host_identity = Self::v2_identity(&abi, &trace_storage_baseline);
        Ok(Self {
            meter,
            abi: Some(abi),
            composition: Some(composition),
            refusal: None,
            outcome: Some(CandidateOutcomeRegion::Response(ResponseRegion::new(
                capacity,
            )?)),
            failure_subtree_fuel: None,
            failure_graph: None,
            protocol_context: None,
            metering_schedule: crate::FuelSchedule::WASMI_0_31_2,
            legacy_reference_fuel: false,
            legacy_reference_engine_committed: 0,
            trace_storage_baseline,
            v2_host_identity,
        })
    }

    pub(crate) const fn isolated_legacy_reference(meter: Meter) -> Self {
        let mut state = Self::isolated(meter);
        state.legacy_reference_fuel = true;
        state
    }

    pub(crate) const fn uses_legacy_reference_fuel(&self) -> bool {
        self.legacy_reference_fuel
    }

    pub(crate) const fn legacy_reference_engine_committed(&self) -> u64 {
        self.legacy_reference_engine_committed
    }

    pub(crate) fn set_legacy_reference_engine_committed(&mut self, consumed: u64) {
        self.legacy_reference_engine_committed = consumed;
    }

    pub(crate) fn publish_response(
        &mut self,
        response: CallResponse,
    ) -> Result<(), ResponseRefusal> {
        let bytes = response.bytes.len();
        let region = match self.outcome.as_mut() {
            Some(CandidateOutcomeRegion::Response(region)) => region,
            Some(CandidateOutcomeRegion::Failure(_)) => {
                return Err(ResponseRefusal::DuplicatePublication)
            }
            None => return Err(ResponseRefusal::CapacityExceeded { bytes, capacity: 0 }),
        };
        region.publish(response)?;
        if let Err(refusal) = self.meter.charge_output_bytes(bytes) {
            let refusal = ResponseRefusal::Meter(refusal);
            region.refuse(refusal.clone());
            return Err(refusal);
        }
        Ok(())
    }

    pub(crate) fn publish_failure(
        &mut self,
        class: RefusalClass,
        reason: RefusalReason,
    ) -> Result<(), ResponseRefusal> {
        match self.outcome.as_ref() {
            Some(CandidateOutcomeRegion::Failure(_)) => {
                return Err(ResponseRefusal::DuplicatePublication)
            }
            Some(CandidateOutcomeRegion::Response(region)) if region.has_publication() => {
                return Err(ResponseRefusal::DuplicatePublication)
            }
            Some(CandidateOutcomeRegion::Response(_)) => {}
            None => return Err(ResponseRefusal::InvalidPublication),
        }
        let program = self
            .abi
            .as_ref()
            .map(Abi::program)
            .ok_or(ResponseRefusal::InvalidPublication)?;
        self.meter
            .charge_output_bytes(reason.bytes().len())
            .map_err(ResponseRefusal::Meter)?;
        self.outcome = Some(CandidateOutcomeRegion::Failure(
            ProgramFailure::authenticated(program, class, reason),
        ));
        Ok(())
    }

    pub(super) fn publish_failure_status(
        &mut self,
        class: RefusalClass,
        reason: RefusalReason,
    ) -> i32 {
        match self.publish_failure(class, reason) {
            Ok(()) => 0,
            Err(ResponseRefusal::Meter(refusal)) => {
                self.record_refusal(CompositionRefusal::Resource(refusal));
                STATUS_METER
            }
            Err(_) if self.failure().is_some() => STATUS_BOUNDS,
            Err(refusal) => {
                self.record_refusal(CompositionRefusal::Response(refusal));
                STATUS_BOUNDS
            }
        }
    }

    pub(crate) fn failure(&self) -> Option<&ProgramFailure> {
        match self.outcome.as_ref() {
            Some(CandidateOutcomeRegion::Failure(failure)) => Some(failure),
            _ => None,
        }
    }

    pub(crate) fn set_failure_subtree_fuel(&mut self, fuel: u64) {
        self.failure_subtree_fuel = Some(fuel);
    }

    pub(crate) fn take_failure_subtree_fuel(&mut self) -> Option<u64> {
        self.failure_subtree_fuel.take()
    }

    pub(crate) fn set_failure_graph(&mut self, graph: CallGraph) {
        self.failure_graph = Some(graph);
    }

    pub(crate) fn take_failure_graph(&mut self) -> Option<CallGraph> {
        self.failure_graph.take()
    }

    pub(crate) fn failure_graph(&self) -> Option<&CallGraph> {
        self.failure_graph.as_ref()
    }

    pub(crate) fn authenticate_protocol_context(
        &mut self,
        context: ExecutionContext,
    ) {
        self.protocol_context = Some(context);
    }

    pub(crate) fn context_field(
        &self,
        field: ContextField,
    ) -> Result<Vec<u8>, ContextRefusal> {
        let context = self
            .protocol_context
            .ok_or(ContextRefusal::Unauthenticated)?;
        let abi = self.abi.as_ref().ok_or(ContextRefusal::Unauthenticated)?;
        let graph = self
            .composition
            .as_ref()
            .ok_or(ContextRefusal::Unauthenticated)?
            .graph();
        let current = graph.current().ok_or(ContextRefusal::FrameMismatch)?;
        if current.program() != abi.program()
            || current.principal() != abi.principal()
            || current.id() != abi.frame()
        {
            return Err(ContextRefusal::FrameMismatch);
        }
        let immediate_caller = graph.immediate_caller();
        let remaining_fuel = self.meter.cpu_remaining();
        Ok(context.encode(
            field,
            current.program(),
            immediate_caller,
            current.principal(),
            remaining_fuel,
        ))
    }

    pub(crate) const fn protocol_context(&self) -> Option<ExecutionContext> {
        self.protocol_context
    }

    pub(super) fn publish_response_status(&mut self, response: CallResponse) -> i32 {
        match self.publish_response(response) {
            Ok(()) => 0,
            Err(ResponseRefusal::Meter(_)) => STATUS_METER,
            Err(_) => STATUS_BOUNDS,
        }
    }

    pub(crate) fn finalize_response(&self, code: i32) -> Result<CallResponse, ResponseRefusal> {
        self.outcome.as_ref().map_or_else(
            || {
                Ok(CallResponse {
                    code,
                    bytes: Vec::new(),
                })
            },
            |outcome| match outcome {
                CandidateOutcomeRegion::Response(region) => region.finish(code),
                CandidateOutcomeRegion::Failure(_) => Err(ResponseRefusal::DuplicatePublication),
            },
        )
    }

    pub(crate) fn refuse_response(&mut self, refusal: ResponseRefusal) {
        if let Some(CandidateOutcomeRegion::Response(region)) = self.outcome.as_mut() {
            region.refuse(refusal);
        }
    }

    pub(crate) const fn meter(&self) -> &Meter {
        &self.meter
    }

    pub(crate) fn meter_mut(&mut self) -> &mut Meter {
        &mut self.meter
    }

    pub(crate) fn frame_cpu_consumed(&self) -> Result<u64, crate::meter::MeterRefusal> {
        self.meter.cpu_total().checked_sub(self.meter.cpu_carried()).ok_or(
            crate::meter::MeterRefusal::CounterOverflow {
                resource: crate::meter::ResourceKind::Cpu,
            },
        )
    }

    pub(crate) fn set_meter(&mut self, meter: Meter) {
        self.meter = meter;
    }

    pub(crate) fn bind_metering_schedule(&mut self, schedule: crate::FuelSchedule) {
        self.metering_schedule = schedule;
    }

    pub(crate) const fn metering_schedule_version(&self) -> u32 {
        self.metering_schedule.version()
    }

    pub(crate) const fn metering_schedule(&self) -> crate::FuelSchedule {
        self.metering_schedule
    }

    pub(crate) fn authorization_abi(&self) -> Option<&Abi> {
        self.abi.as_ref()
    }

    pub(crate) fn abi_mut(&mut self) -> Option<&mut Abi> {
        self.abi.as_mut()
    }

    pub(crate) fn composition(&self) -> Option<&Composition> {
        self.composition.as_ref()
    }

    pub(crate) fn composition_mut(&mut self) -> Option<&mut Composition> {
        self.composition.as_mut()
    }

    pub(crate) fn record_refusal(&mut self, refusal: CompositionRefusal) {
        if self.refusal.is_none() && self.failure().is_none() {
            self.refusal = Some(refusal);
        }
    }

    pub(crate) fn refusal(&self) -> Option<&CompositionRefusal> {
        self.refusal.as_ref()
    }

    pub(crate) fn into_parts(self) -> (Meter, Option<Abi>, Option<Composition>) {
        (self.meter, self.abi, self.composition)
    }

    pub(super) fn with_abi<T>(
        &mut self,
        operation: impl FnOnce(&mut Abi, &mut Meter) -> Result<T, AbiError>,
    ) -> Result<T, AbiError> {
        let result = self
            .abi
            .as_mut()
            .ok_or(AbiError::CapabilityDenied)
            .and_then(|abi| operation(abi, &mut self.meter));
        if let Err(error) = &result {
            if self.meter.is_activity() {
                self.record_refusal(CompositionRefusal::Authority(*error));
            }
        }
        result
    }
}

pub(crate) fn charge_host_cpu(
    caller: &mut Caller<'_, RuntimeState>,
    fuel: u64,
) -> Result<(), crate::meter::MeterRefusal> {
    if caller.data().uses_legacy_reference_fuel() {
        reconcile_reference_guest_cpu(caller)?;
        if caller.consume_fuel(fuel).is_err() {
            caller.data_mut().meter_mut().mark_cpu_exhausted();
            return Err(caller.data().meter().exhaustion().unwrap_or(
                crate::meter::MeterRefusal::BudgetExceeded {
                    resource: crate::meter::ResourceKind::Cpu,
                    limit: caller.data().meter().cpu_budget(),
                    attempted: caller.data().meter().cpu_budget().saturating_add(1),
                },
            ));
        }
        caller.data_mut().meter_mut().charge_cpu(fuel)?;
        let consumed = caller.fuel_consumed().unwrap_or_else(|| unreachable!());
        caller.data_mut().set_legacy_reference_engine_committed(consumed);
        return Ok(());
    }
    caller.data_mut().meter_mut().charge_cpu(fuel)
}

pub(crate) fn reconcile_reference_guest_cpu(
    caller: &mut Caller<'_, RuntimeState>,
) -> Result<(), crate::meter::MeterRefusal> {
    if !caller.data().uses_legacy_reference_fuel() {
        return Ok(());
    }
    let consumed = caller.fuel_consumed().unwrap_or(0);
    let committed = caller.data().legacy_reference_engine_committed();
    let guest = consumed.checked_sub(committed).ok_or(
        crate::meter::MeterRefusal::CounterOverflow {
            resource: crate::meter::ResourceKind::Cpu,
        },
    )?;
    caller.data_mut().meter_mut().charge_cpu(guest)?;
    caller.data_mut().set_legacy_reference_engine_committed(consumed);
    Ok(())
}

#[allow(clippy::too_many_lines)]
pub(crate) fn linker(
    engine: &Engine,
) -> Result<HostLinker, ExecutionFault> {
    let mut linker = Linker::new(engine);
    linker
        .func_wrap(
            PRIVATE_METER_MODULE,
            PRIVATE_CHECK_FUNCTION,
            |caller: Caller<'_, RuntimeState>, raw_charge: i64| -> Result<(), wasmi::core::Trap> {
                let charge = u64::try_from(raw_charge)
                    .map_err(|_| wasmi::core::Trap::from(wasmi::core::TrapCode::OutOfFuel))?;
                let meter = caller.data().meter();
                match meter.cpu_total().checked_add(charge) {
                    Some(attempted) if attempted <= meter.cpu_budget() => Ok(()),
                    _ => Err(wasmi::core::Trap::from(wasmi::core::TrapCode::OutOfFuel)),
                }
            },
        )
        .map_err(|error| linker_fault(&error))?;
    linker
        .func_wrap(
            PRIVATE_METER_MODULE,
            PRIVATE_CHARGE_FUNCTION,
            |mut caller: Caller<'_, RuntimeState>, raw_charge: i64| -> Result<(), wasmi::core::Trap> {
                let charge = u64::try_from(raw_charge)
                    .map_err(|_| wasmi::core::Trap::from(wasmi::core::TrapCode::OutOfFuel))?;
                caller
                    .data_mut()
                    .meter_mut()
                    .charge_cpu(charge)
                    .map_err(|_| wasmi::core::Trap::from(wasmi::core::TrapCode::OutOfFuel))
            },
        )
        .map_err(|error| linker_fault(&error))?;
    storage::register(&mut linker)?;
    events::register(&mut linker)?;
    calls::register(&mut linker)?;
    crypto::register(&mut linker)?;
    signature::register(&mut linker)?;
    bigint::register(&mut linker)?;
    calls::register_candidate(&mut linker)?;
    context::register_candidate(&mut linker)?;
    scan::register_candidate(&mut linker)?;
    storage::register_candidate(&mut linker)?;
    transfer::register_candidate(&mut linker)?;
    balance::register_candidate(&mut linker)?;
    transfer::register(&mut linker)?;
    linker
        .func_wrap(
            ABI_MODULE,
            "receipt_read",
            |mut caller: Caller<'_, RuntimeState>,
             digest_pointer: i32,
             digest_length: i32,
             output_pointer: i32,
             output_capacity: i32|
             -> i32 {
                let digest = match read_fixed::<32>(&caller, digest_pointer, digest_length) {
                    Ok(digest) => digest,
                    Err(status) => return status,
                };
                let view = match caller
                    .data_mut()
                    .with_abi(|abi, _| abi.receipt_read(digest))
                {
                    Ok(view) => view,
                    Err(error) => return error_status(error),
                };
                let encoded = encode_receipt(&view);
                let capacity = match nonnegative(output_capacity) {
                    Ok(capacity) => capacity,
                    Err(status) => return status,
                };
                if encoded.len() > capacity {
                    return STATUS_BOUNDS;
                }
                if let Err(status) = write_guest(&mut caller, output_pointer, &encoded) {
                    return status;
                }
                i32::try_from(encoded.len()).unwrap_or(STATUS_BOUNDS)
            },
        )
        .map_err(|error| linker_fault(&error))?;
    let registered_function_count =
        crate::abi::HOST_FUNCTIONS.len() + crate::abi::manifest::ABI_V2_HOST_FUNCTIONS.len();
    Ok(HostLinker {
        linker,
        construction_count: 1,
        registered_function_count,
    })
}

fn encode_receipt(view: &ReceiptView) -> Vec<u8> {
    let mut encoded = Vec::with_capacity(116);
    encoded.extend_from_slice(&view.receipt_digest);
    encoded.extend_from_slice(&view.result_code.to_be_bytes());
    encoded.extend_from_slice(&view.asset);
    encoded.extend_from_slice(&view.amount.to_be_bytes());
    encoded.extend_from_slice(&view.state_root);
    encoded
}

pub(crate) const fn error_status(error: AbiError) -> i32 {
    match error {
        AbiError::CapabilityDenied
        | AbiError::CapabilityEscalation
        | AbiError::AccessDeclaration => STATUS_DENIED,
        AbiError::Meter(_) => STATUS_METER,
        AbiError::ReceiptMismatch | AbiError::BalanceEvidenceUnavailable => STATUS_EVIDENCE,
        AbiError::BalanceAbsent => STATUS_ABSENT,
        AbiError::Storage(
            crate::storage::StorageError::InvalidScanCursor
            | crate::storage::StorageError::InvalidScanLimits,
        ) => STATUS_INVALID,
        AbiError::EventBounds
        | AbiError::CallBounds
        | AbiError::AmountBounds
        | AbiError::Storage(_) => STATUS_BOUNDS,
        AbiError::WrongVersion
        | AbiError::InvalidCapability
        | AbiError::DuplicateCapability
        | AbiError::InvalidEncoding => STATUS_INVALID,
    }
}

pub(crate) fn linker_fault(error: &wasmi::errors::LinkerError) -> ExecutionFault {
    ExecutionFault::EngineFault {
        reason: error.to_string(),
    }
}
