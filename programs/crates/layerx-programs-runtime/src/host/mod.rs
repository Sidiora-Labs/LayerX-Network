//! Concrete `wasmi` bindings for the version-one capability ABI.

mod calls;
mod crypto;
mod events;
pub(crate) mod memory;
mod scan;
mod signature;
mod storage;
mod transfer;

use wasmi::{Caller, Engine, Linker};

use crate::abi::response::{CallResponse, ResponseRefusal, ResponseRegion};
use crate::abi::{Abi, AbiError, ReceiptView, ABI_MODULE};
use crate::calls::{CallGraph, Composition, CompositionRefusal};
use crate::crypto::bigint;
use crate::execute::ExecutionFault;
use crate::fault::{ProgramFailure, RefusalClass, RefusalReason};
use crate::meter::Meter;
use crate::validate::AbiRevision;

use self::memory::{nonnegative, read_fixed, write_guest};

pub(super) const STATUS_DENIED: i32 = -1;
pub(super) const STATUS_INVALID: i32 = -2;
pub(super) const STATUS_BOUNDS: i32 = -3;
pub(super) const STATUS_METER: i32 = -4;
pub(super) const STATUS_EVIDENCE: i32 = -5;
pub(super) const STATUS_VERIFY_FAILED: i32 = -6;
pub(super) const FUEL_METERING_DISABLED: &str = "programs runtime fuel metering is disabled";
pub(super) const COMPOSITION_REFUSED: &str = "program composition refused the call graph";

#[derive(Debug)]
pub(crate) struct RuntimeState {
    meter: Meter,
    abi: Option<Abi>,
    composition: Option<Composition>,
    refusal: Option<CompositionRefusal>,
    outcome: Option<CandidateOutcomeRegion>,
    failure_subtree_fuel: Option<u64>,
    failure_graph: Option<CallGraph>,
}

#[derive(Debug)]
enum CandidateOutcomeRegion {
    Response(ResponseRegion),
    Failure(ProgramFailure),
}

impl RuntimeState {
    pub(crate) const fn isolated(meter: Meter) -> Self {
        Self {
            meter,
            abi: None,
            composition: None,
            refusal: None,
            outcome: None,
            failure_subtree_fuel: None,
            failure_graph: None,
        }
    }

    pub(crate) fn composed(meter: Meter, abi: Abi, composition: Composition) -> Self {
        Self {
            meter,
            abi: Some(abi),
            composition: Some(composition),
            refusal: None,
            outcome: None,
            failure_subtree_fuel: None,
            failure_graph: None,
        }
    }

    pub(crate) fn composed_with_response(
        meter: Meter,
        abi: Abi,
        composition: Composition,
        capacity: usize,
    ) -> Result<Self, ResponseRefusal> {
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
        })
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

    pub(crate) fn set_meter(&mut self, meter: Meter) {
        self.meter = meter;
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

#[allow(clippy::too_many_lines)]
pub(crate) fn linker(
    engine: &Engine,
    revision: AbiRevision,
) -> Result<Linker<RuntimeState>, ExecutionFault> {
    let mut linker = Linker::new(engine);
    storage::register(&mut linker)?;
    events::register(&mut linker)?;
    calls::register(&mut linker)?;
    crypto::register(&mut linker)?;
    signature::register(&mut linker)?;
    bigint::register(&mut linker)?;
    if revision == AbiRevision::CandidateV2 {
        calls::register_candidate(&mut linker)?;
        scan::register_candidate(&mut linker)?;
        storage::register_candidate(&mut linker)?;
        transfer::register_candidate(&mut linker)?;
    }
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
    Ok(linker)
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
        AbiError::CapabilityDenied | AbiError::CapabilityEscalation => STATUS_DENIED,
        AbiError::Meter(_) => STATUS_METER,
        AbiError::ReceiptMismatch => STATUS_EVIDENCE,
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
