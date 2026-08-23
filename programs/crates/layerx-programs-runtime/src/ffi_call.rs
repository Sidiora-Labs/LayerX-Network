//! Scalar-only C ingress for one real Programs CALL activity.

use crate::storage::StorageError;
use crate::validate::AbiRevision;
use crate::{
    AbiError, ActivityBudgetBinding, AtomicTransferSet, AuthorizationContext,
    AuthorizedExecutionRecord, BudgetMeterRefusal, BudgetResourceKind,
    BudgetedAuthorizedExecutionRequest, BudgetedV1FailureCause, CapabilitySet, CompositionContext,
    CompositionRefusal, CompositionRules, DeclaredBudget, EntrypointRefusal, ExecutionFault,
    Executor, KernelTransferEvidence, KernelTransferPrimitive, MeterRefusal, MeteredUsage,
    PreparedAuthorizedActivityOutcome, PrincipalId, ProgramCatalog, ProgramEvent, ProgramId,
    ReceiptOracle, ReceiptView, ResourceKind, ResponseRefusal, SettlementFailure, Storage,
    StorageNamespace, TransferLawError, WasmEngine,
};

const OK: i32 = 0;
const NON_CANONICAL: i32 = -3;
const LENGTH_LIMIT: i32 = -5;
const MODULE_DISABLED: i32 = -103;
const FATAL_INVARIANT: i32 = -1001;
const ABI_VERSION: u16 = 1;
const WASM: u16 = 1;
const ENTRYPOINT: u16 = 3;
const CALLDATA: u16 = 4;
const CAPABILITIES: u16 = 5;
const PRINCIPAL: u16 = 0;
const SHARED: u16 = 1;
const KEY: u16 = 0;
const VALUE: u16 = 1;
const SUCCESS: u8 = 1;
const FAILURE: u8 = 2;
const RESOURCE: u8 = 3;
const GAS_EXHAUSTED: i32 = -601;
const PROGRAM_REFUSED: i32 = -736;

fn put_usize(out: &mut Vec<u8>, value: usize) -> Result<(), i32> {
    out.extend_from_slice(
        &u64::try_from(value)
            .map_err(|_| LENGTH_LIMIT)?
            .to_be_bytes(),
    );
    Ok(())
}
fn put_text(out: &mut Vec<u8>, value: &str) -> Result<(), i32> {
    out.extend_from_slice(
        &u32::try_from(value.len())
            .map_err(|_| LENGTH_LIMIT)?
            .to_be_bytes(),
    );
    out.extend_from_slice(value.as_bytes());
    Ok(())
}
const fn revision_tag(value: AbiRevision) -> u8 {
    match value {
        AbiRevision::V1 => 1,
        AbiRevision::CandidateV2 => 2,
    }
}
const fn meter_kind(value: ResourceKind) -> u8 {
    match value {
        ResourceKind::Cpu => 1,
        ResourceKind::Memory => 2,
        ResourceKind::StorageRead => 3,
        ResourceKind::StorageWrite => 4,
        ResourceKind::StorageOccupancy => 5,
        ResourceKind::Output => 6,
        ResourceKind::OutputBytes => 7,
    }
}
fn meter_payload(value: MeterRefusal) -> Vec<u8> {
    let mut out = Vec::new();
    match value {
        MeterRefusal::BudgetExceeded {
            resource,
            limit,
            attempted,
        } => {
            out.extend([1, meter_kind(resource)]);
            out.extend_from_slice(&limit.to_be_bytes());
            out.extend_from_slice(&attempted.to_be_bytes());
        }
        MeterRefusal::CounterOverflow { resource } => out.extend([2, meter_kind(resource)]),
        MeterRefusal::FeeOverflow => out.push(3),
    }
    out
}
const fn storage_tag(value: StorageError) -> u8 {
    match value {
        StorageError::InvalidProgram => 1,
        StorageError::InvalidPrincipal => 2,
        StorageError::EmptyKey => 3,
        StorageError::KeyTooLarge => 4,
        StorageError::ValueTooLarge => 5,
        StorageError::PrefixTooLarge => 6,
        StorageError::InvalidScanCursor => 7,
        StorageError::InvalidScanLimits => 8,
        StorageError::ScanCeilingExceeded => 9,
        StorageError::SizeOverflow => 10,
    }
}
fn abi_payload(value: &AbiError) -> Vec<u8> {
    match value {
        AbiError::WrongVersion => vec![1],
        AbiError::InvalidCapability => vec![2],
        AbiError::DuplicateCapability => vec![3],
        AbiError::CapabilityDenied => vec![4],
        AbiError::CapabilityEscalation => vec![5],
        AbiError::EventBounds => vec![6],
        AbiError::CallBounds => vec![7],
        AbiError::AmountBounds => vec![8],
        AbiError::ReceiptMismatch => vec![9],
        AbiError::InvalidEncoding => vec![10],
        AbiError::Storage(error) => vec![11, storage_tag(*error)],
        AbiError::Meter(error) => {
            let mut out = vec![12];
            out.extend(meter_payload(*error));
            out
        }
    }
}
fn fault_payload(value: &ExecutionFault) -> Result<Vec<u8>, i32> {
    let mut out = Vec::new();
    match value {
        ExecutionFault::UnknownExport { name } => {
            out.push(1);
            put_text(&mut out, name)?
        }
        ExecutionFault::NotAFunction { name } => {
            out.push(2);
            put_text(&mut out, name)?
        }
        ExecutionFault::UnreachableExecuted => out.push(3),
        ExecutionFault::MemoryOutOfBounds => out.push(4),
        ExecutionFault::TableOutOfBounds => out.push(5),
        ExecutionFault::IndirectCallToNull => out.push(6),
        ExecutionFault::IntegerDivisionByZero => out.push(7),
        ExecutionFault::IntegerOverflow => out.push(8),
        ExecutionFault::BadConversionToInteger => out.push(9),
        ExecutionFault::StackExhausted => out.push(10),
        ExecutionFault::BadSignature => out.push(11),
        ExecutionFault::OutOfFuel => out.push(12),
        ExecutionFault::GrowthLimited => out.push(13),
        ExecutionFault::Resource { refusal } => {
            out.push(14);
            out.extend(meter_payload(*refusal))
        }
        ExecutionFault::NonIntegerValue => out.push(15),
        ExecutionFault::EngineFault { reason } => {
            out.push(16);
            put_text(&mut out, reason)?
        }
    }
    Ok(out)
}
fn response_payload(value: &ResponseRefusal) -> Result<Vec<u8>, i32> {
    let mut out = Vec::new();
    match value {
        ResponseRefusal::TooLarge { bytes, limit } => {
            out.push(1);
            put_usize(&mut out, *bytes)?;
            put_usize(&mut out, *limit)?
        }
        ResponseRefusal::CapacityExceeded { bytes, capacity } => {
            out.push(2);
            put_usize(&mut out, *bytes)?;
            put_usize(&mut out, *capacity)?
        }
        ResponseRefusal::DuplicatePublication => out.push(3),
        ResponseRefusal::InvalidPublication => out.push(4),
        ResponseRefusal::CodeMismatch {
            published,
            returned,
        } => {
            out.push(5);
            out.extend_from_slice(&published.to_be_bytes());
            out.extend_from_slice(&returned.to_be_bytes())
        }
        ResponseRefusal::Meter(error) => {
            out.push(6);
            out.extend(meter_payload(*error))
        }
    }
    Ok(out)
}
fn entry_payload(value: &EntrypointRefusal) -> Result<Vec<u8>, i32> {
    let mut out = Vec::new();
    match value {
        EntrypointRefusal::InputTooLarge { bytes, limit } => {
            out.push(1);
            put_usize(&mut out, *bytes)?;
            put_usize(&mut out, *limit)?
        }
        EntrypointRefusal::MissingAllocator => out.push(2),
        EntrypointRefusal::MissingMemory => out.push(3),
        EntrypointRefusal::MissingEntry => out.push(4),
        EntrypointRefusal::AllocationRefused { code } => {
            out.push(5);
            out.extend_from_slice(&code.to_be_bytes())
        }
        EntrypointRefusal::GuestRefused { code } => {
            out.push(6);
            out.extend_from_slice(&code.to_be_bytes())
        }
        EntrypointRefusal::Fault(fault) => {
            out.push(7);
            out.extend(fault_payload(fault)?)
        }
        EntrypointRefusal::Resource(error) => {
            out.push(8);
            out.extend(meter_payload(*error))
        }
    }
    Ok(out)
}
fn composition_payload(value: &CompositionRefusal) -> Result<Vec<u8>, i32> {
    let mut out = Vec::new();
    match value {
        CompositionRefusal::NotComposable => out.push(1),
        CompositionRefusal::WrongVersion { expected, actual } => {
            out.extend([2, revision_tag(*expected), revision_tag(*actual)])
        }
        CompositionRefusal::UnknownProgram { program } => {
            out.push(3);
            out.extend_from_slice(&program.bytes())
        }
        CompositionRefusal::Reentrancy { program } => {
            out.push(4);
            out.extend_from_slice(&program.bytes())
        }
        CompositionRefusal::DepthExceeded { limit, attempted } => {
            out.push(5);
            out.extend_from_slice(&limit.to_be_bytes());
            out.extend_from_slice(&attempted.to_be_bytes())
        }
        CompositionRefusal::EdgesExceeded { limit, attempted } => {
            out.push(6);
            out.extend_from_slice(&limit.to_be_bytes());
            out.extend_from_slice(&attempted.to_be_bytes())
        }
        CompositionRefusal::FanoutExceeded { limit, attempted } => {
            out.push(7);
            out.extend_from_slice(&limit.to_be_bytes());
            out.extend_from_slice(&attempted.to_be_bytes())
        }
        CompositionRefusal::VisitsExceeded {
            program,
            limit,
            attempted,
        } => {
            out.push(8);
            out.extend_from_slice(&program.bytes());
            out.extend_from_slice(&limit.to_be_bytes());
            out.extend_from_slice(&attempted.to_be_bytes())
        }
        CompositionRefusal::MissingEntry => out.push(9),
        CompositionRefusal::MissingAllocator => out.push(10),
        CompositionRefusal::MissingMemory => out.push(11),
        CompositionRefusal::AllocationRefused { code } => {
            out.push(12);
            out.extend_from_slice(&code.to_be_bytes())
        }
        CompositionRefusal::InputTooLarge { bytes, limit } => {
            out.push(13);
            put_usize(&mut out, *bytes)?;
            put_usize(&mut out, *limit)?
        }
        CompositionRefusal::GuestRefused { program, code } => {
            out.push(14);
            out.extend_from_slice(&program.bytes());
            out.extend_from_slice(&code.to_be_bytes())
        }
        CompositionRefusal::Program(failure) => {
            out.push(15);
            out.extend(failure.canonical_encode())
        }
        CompositionRefusal::Authority(error) => {
            out.push(16);
            out.extend(abi_payload(error))
        }
        CompositionRefusal::Fault(fault) => {
            out.push(17);
            out.extend(fault_payload(fault)?)
        }
        CompositionRefusal::Resource(error) => {
            out.push(18);
            out.extend(meter_payload(*error))
        }
        CompositionRefusal::Response(error) => {
            out.push(19);
            out.extend(response_payload(error)?)
        }
    }
    Ok(out)
}

fn typed_failure_detail(cause: &BudgetedV1FailureCause) -> Result<Vec<u8>, i32> {
    let mut encoded = b"LXP/programs/failure-detail/v1\0".to_vec();
    let (tag, payload) = match cause {
        BudgetedV1FailureCause::Program(failure) => (1_u8, failure.canonical_encode()),
        BudgetedV1FailureCause::Composition(refusal) => (2, composition_payload(refusal)?),
        BudgetedV1FailureCause::Entrypoint(refusal) => (3, entry_payload(refusal)?),
        BudgetedV1FailureCause::Abi(refusal) => (4, abi_payload(refusal)),
    };
    encoded.push(tag);
    encoded.extend_from_slice(
        &u32::try_from(payload.len())
            .map_err(|_| LENGTH_LIMIT)?
            .to_be_bytes(),
    );
    encoded.extend_from_slice(&payload);
    Ok(encoded)
}

const fn resource_tag(resource: BudgetResourceKind) -> u8 {
    match resource {
        BudgetResourceKind::Cpu => 1,
        BudgetResourceKind::Memory => 2,
        BudgetResourceKind::StorageRead => 3,
        BudgetResourceKind::StorageWrite => 4,
        BudgetResourceKind::Output => 5,
        BudgetResourceKind::OutputBytes => 6,
        BudgetResourceKind::Table => 7,
    }
}

fn typed_resource_detail(refusal: BudgetMeterRefusal) -> Vec<u8> {
    let mut encoded = b"LXP/programs/resource-detail/v1\0".to_vec();
    match refusal {
        BudgetMeterRefusal::BudgetExceeded {
            resource,
            limit,
            attempted,
        } => {
            encoded.push(1);
            encoded.push(resource_tag(resource));
            encoded.extend_from_slice(&limit.to_be_bytes());
            encoded.extend_from_slice(&attempted.to_be_bytes());
        }
        BudgetMeterRefusal::CounterOverflow { resource } => {
            encoded.push(2);
            encoded.push(resource_tag(resource));
        }
    }
    encoded
}

fn transfer_error_tag(error: TransferLawError) -> u8 {
    match error {
        TransferLawError::UnverifiedAuthority => 1,
        TransferLawError::InvalidTransfer => 2,
        TransferLawError::InvalidTransferSet => 3,
        TransferLawError::AmountOverflow => 4,
        TransferLawError::InvariantViolation => 5,
        TransferLawError::CapabilityEscalation => 6,
        TransferLawError::KernelRefused => 7,
        TransferLawError::ReceiptInvalid => 8,
        TransferLawError::ReceiptMismatch => 9,
        TransferLawError::StaleStorage => 10,
    }
}

fn settlement_detail(error: TransferLawError) -> Vec<u8> {
    let mut encoded = b"LXP/programs/settlement-failure/v1\0".to_vec();
    encoded.push(transfer_error_tag(error));
    encoded
}

fn callback_detail(stage: u8, status: i32) -> Vec<u8> {
    let mut encoded = b"LXP/programs/callback-failure/v1\0".to_vec();
    encoded.push(stage);
    encoded.extend_from_slice(&status.to_be_bytes());
    encoded
}

unsafe extern "C" {
    fn layerx_programs_call_activity_byte(token: u64, section: u16, offset: u32) -> i32;
    fn layerx_programs_call_catalog_count(token: u64) -> i32;
    fn layerx_programs_call_catalog_identity_byte(
        token: u64,
        index: u32,
        section: u16,
        offset: u32,
    ) -> i32;
    fn layerx_programs_call_catalog_wasm_length(token: u64, index: u32) -> i32;
    fn layerx_programs_call_catalog_wasm_byte(token: u64, index: u32, offset: u32) -> i32;
    fn layerx_programs_call_receipt_view_begin(
        token: u64,
        d0: u64,
        d1: u64,
        d2: u64,
        d3: u64,
    ) -> i32;
    fn layerx_programs_call_receipt_view_byte(token: u64, section: u16, offset: u32) -> i32;
    fn layerx_programs_call_catalog_storage_cell_count(
        token: u64,
        index: u32,
        selector: u16,
    ) -> i32;
    fn layerx_programs_call_catalog_storage_cell_length(
        token: u64,
        index: u32,
        selector: u16,
        cell: u32,
        section: u16,
    ) -> i32;
    fn layerx_programs_call_catalog_storage_cell_byte(
        token: u64,
        index: u32,
        selector: u16,
        cell: u32,
        section: u16,
        offset: u32,
    ) -> i32;
    fn layerx_programs_call_catalog_storage_final_begin(
        token: u64,
        index: u32,
        selector: u16,
        count: u32,
    ) -> i32;
    fn layerx_programs_call_catalog_storage_final_cell(
        token: u64,
        index: u32,
        selector: u16,
        cell: u32,
        key_length: u16,
        value_length: u32,
    ) -> i32;
    fn layerx_programs_call_catalog_storage_final_byte(
        token: u64,
        index: u32,
        selector: u16,
        cell: u32,
        section: u16,
        offset: u32,
        byte: u8,
    ) -> i32;
    fn layerx_programs_call_catalog_storage_final_apply(
        token: u64,
        index: u32,
        selector: u16,
    ) -> i32;
    fn layerx_programs_call_storage_final_authorize(token: u64) -> i32;
    fn layerx_programs_call_terminal_begin(
        token: u64,
        kind: u8,
        result: i32,
        runtime: u16,
        abi: u16,
        schedule: u32,
        cpu: u64,
        memory: u64,
        storage_read: u64,
        storage_write: u64,
        output_values: u32,
        output_bytes: u64,
        fee_hi: u64,
        fee_lo: u64,
        root0: u64,
        root1: u64,
        root2: u64,
        root3: u64,
        graph_length: u32,
        detail_length: u32,
        events_length: u32,
    ) -> i32;
    fn layerx_programs_call_terminal_byte(token: u64, section: u16, offset: u32, byte: u8) -> i32;
    fn layerx_programs_call_terminal_publish(token: u64) -> i32;
    fn layerx_programs_call_event_begin(
        token: u64,
        index: u32,
        p0: u64,
        p1: u64,
        p2: u64,
        p3: u64,
        r0: u64,
        r1: u64,
        r2: u64,
        r3: u64,
        frame_path: u64,
        depth: u8,
        topic_length: u16,
        data_length: u32,
    ) -> i32;
    fn layerx_programs_call_event_byte(token: u64, section: u16, offset: u32, byte: u8) -> i32;
    fn layerx_programs_call_event_emit(token: u64) -> i32;
    fn layerx_programs_call_transfer_begin(token: u64, legs: u16) -> i32;
    fn layerx_programs_call_transfer_leg(
        token: u64,
        index: u16,
        f0: u64,
        f1: u64,
        f2: u64,
        f3: u64,
        t0: u64,
        t1: u64,
        t2: u64,
        t3: u64,
        a0: u64,
        a1: u64,
        a2: u64,
        a3: u64,
        amount_hi: u64,
        amount_lo: u64,
    ) -> i32;
    fn layerx_programs_call_transfer_apply(token: u64) -> i32;
    fn layerx_programs_call_transfer_root_byte(token: u64, offset: u32) -> i32;
}

fn bytes(words: [u64; 4]) -> [u8; 32] {
    let mut result = [0; 32];
    for (index, word) in words.into_iter().enumerate() {
        result[index * 8..index * 8 + 8].copy_from_slice(&word.to_be_bytes());
    }
    result
}

fn words(bytes: [u8; 32]) -> [u64; 4] {
    core::array::from_fn(|index| {
        let mut word = [0; 8];
        word.copy_from_slice(&bytes[index * 8..index * 8 + 8]);
        u64::from_be_bytes(word)
    })
}

fn scalar_bytes(length: usize, callback: impl Fn(u32) -> i32) -> Result<Vec<u8>, i32> {
    let length = u32::try_from(length).map_err(|_| LENGTH_LIMIT)?;
    let mut result = Vec::with_capacity(usize::try_from(length).map_err(|_| LENGTH_LIMIT)?);
    for offset in 0..length {
        let value = callback(offset);
        result
            .push(u8::try_from(value).map_err(|_| if value < 0 { value } else { NON_CANONICAL })?);
    }
    Ok(result)
}

fn c_ok(status: i32) -> Result<(), i32> {
    if status == OK {
        Ok(())
    } else {
        Err(status)
    }
}
fn c_count(value: i32) -> Result<u32, i32> {
    u32::try_from(value).map_err(|_| value)
}

fn catalog_identity(token: u64, index: u32, section: u16) -> Result<[u8; 32], i32> {
    let bytes = scalar_bytes(32, |offset| unsafe {
        layerx_programs_call_catalog_identity_byte(token, index, section, offset)
    })?;
    bytes.try_into().map_err(|_| NON_CANONICAL)
}

fn import_catalog_storage(
    token: u64,
    index: u32,
    program: ProgramId,
    payer: PrincipalId,
    storage: &mut Storage,
) -> Result<(), i32> {
    for selector in [PRINCIPAL, SHARED] {
        let namespace = if selector == PRINCIPAL {
            StorageNamespace::principal(program, payer)
        } else {
            StorageNamespace::shared(program)
        };
        let count = c_count(unsafe {
            layerx_programs_call_catalog_storage_cell_count(token, index, selector)
        })?;
        for cell in 0..count {
            let key_length = c_count(unsafe {
                layerx_programs_call_catalog_storage_cell_length(token, index, selector, cell, KEY)
            })?;
            let value_length = c_count(unsafe {
                layerx_programs_call_catalog_storage_cell_length(
                    token, index, selector, cell, VALUE,
                )
            })?;
            let key = scalar_bytes(
                usize::try_from(key_length).map_err(|_| LENGTH_LIMIT)?,
                |offset| unsafe {
                    layerx_programs_call_catalog_storage_cell_byte(
                        token, index, selector, cell, KEY, offset,
                    )
                },
            )?;
            let value = scalar_bytes(
                usize::try_from(value_length).map_err(|_| LENGTH_LIMIT)?,
                |offset| unsafe {
                    layerx_programs_call_catalog_storage_cell_byte(
                        token, index, selector, cell, VALUE, offset,
                    )
                },
            )?;
            storage
                .write(namespace, &key, &value)
                .map_err(|_| NON_CANONICAL)?;
        }
    }
    Ok(())
}

fn export_catalog_storage(
    token: u64,
    index: u32,
    program: ProgramId,
    payer: PrincipalId,
    storage: &Storage,
) -> Result<(), i32> {
    for selector in [PRINCIPAL, SHARED] {
        let namespace = if selector == PRINCIPAL {
            StorageNamespace::principal(program, payer)
        } else {
            StorageNamespace::shared(program)
        };
        let cells = storage.namespace_entries(namespace);
        c_ok(unsafe {
            layerx_programs_call_catalog_storage_final_begin(
                token,
                index,
                selector,
                u32::try_from(cells.len()).map_err(|_| LENGTH_LIMIT)?,
            )
        })?;
        for (number, (key, value)) in cells.into_iter().enumerate() {
            let cell = u32::try_from(number).map_err(|_| LENGTH_LIMIT)?;
            c_ok(unsafe {
                layerx_programs_call_catalog_storage_final_cell(
                    token,
                    index,
                    selector,
                    cell,
                    u16::try_from(key.len()).map_err(|_| LENGTH_LIMIT)?,
                    u32::try_from(value.len()).map_err(|_| LENGTH_LIMIT)?,
                )
            })?;
            for (offset, byte) in key.into_iter().enumerate() {
                c_ok(unsafe {
                    layerx_programs_call_catalog_storage_final_byte(
                        token,
                        index,
                        selector,
                        cell,
                        KEY,
                        u32::try_from(offset).map_err(|_| LENGTH_LIMIT)?,
                        byte,
                    )
                })?;
            }
            for (offset, byte) in value.into_iter().enumerate() {
                c_ok(unsafe {
                    layerx_programs_call_catalog_storage_final_byte(
                        token,
                        index,
                        selector,
                        cell,
                        VALUE,
                        u32::try_from(offset).map_err(|_| LENGTH_LIMIT)?,
                        byte,
                    )
                })?;
            }
        }
        c_ok(unsafe { layerx_programs_call_catalog_storage_final_apply(token, index, selector) })?;
    }
    Ok(())
}

#[derive(Debug)]
struct CReceiptOracle {
    token: u64,
}
impl ReceiptOracle for CReceiptOracle {
    fn verified_receipt(&self, digest: [u8; 32]) -> Result<ReceiptView, AbiError> {
        let digest_words = words(digest);
        c_ok(unsafe {
            layerx_programs_call_receipt_view_begin(
                self.token,
                digest_words[0],
                digest_words[1],
                digest_words[2],
                digest_words[3],
            )
        })
        .map_err(|_| AbiError::ReceiptMismatch)?;
        let returned_digest: [u8; 32] = scalar_bytes(32, |offset| unsafe {
            layerx_programs_call_receipt_view_byte(self.token, 0, offset)
        })
        .and_then(|bytes| bytes.try_into().map_err(|_| NON_CANONICAL))
        .map_err(|_| AbiError::ReceiptMismatch)?;
        if returned_digest != digest {
            return Err(AbiError::ReceiptMismatch);
        }
        let result: [u8; 4] = scalar_bytes(4, |offset| unsafe {
            layerx_programs_call_receipt_view_byte(self.token, 1, offset)
        })
        .and_then(|bytes| bytes.try_into().map_err(|_| NON_CANONICAL))
        .map_err(|_| AbiError::ReceiptMismatch)?;
        let asset = scalar_bytes(32, |offset| unsafe {
            layerx_programs_call_receipt_view_byte(self.token, 2, offset)
        })
        .and_then(|bytes| bytes.try_into().map_err(|_| NON_CANONICAL))
        .map_err(|_| AbiError::ReceiptMismatch)?;
        let amount = scalar_bytes(16, |offset| unsafe {
            layerx_programs_call_receipt_view_byte(self.token, 3, offset)
        })
        .and_then(|bytes| <[u8; 16]>::try_from(bytes).map_err(|_| NON_CANONICAL))
        .map_err(|_| AbiError::ReceiptMismatch)?;
        let state_root = scalar_bytes(32, |offset| unsafe {
            layerx_programs_call_receipt_view_byte(self.token, 4, offset)
        })
        .and_then(|bytes| bytes.try_into().map_err(|_| NON_CANONICAL))
        .map_err(|_| AbiError::ReceiptMismatch)?;
        Ok(ReceiptView {
            receipt_digest: returned_digest,
            result_code: i32::from_be_bytes(result),
            asset,
            amount: u128::from_be_bytes(amount),
            state_root,
        })
    }
}

struct CKernel {
    token: u64,
}
impl KernelTransferPrimitive for CKernel {
    fn apply_and_verify_402lxp_set(
        &mut self,
        transfers: &AtomicTransferSet,
    ) -> Result<KernelTransferEvidence, TransferLawError> {
        c_ok(unsafe {
            layerx_programs_call_transfer_begin(
                self.token,
                u16::try_from(transfers.legs().len())
                    .map_err(|_| TransferLawError::InvalidTransferSet)?,
            )
        })
        .map_err(|_| TransferLawError::ReceiptMismatch)?;
        for (index, leg) in transfers.legs().iter().enumerate() {
            let from = words(leg.principal.bytes());
            let to = words(leg.to);
            let asset = words(leg.asset);
            let amount = leg.amount.to_be_bytes();
            let amount_hi = u64::from_be_bytes(
                amount[..8]
                    .try_into()
                    .map_err(|_| TransferLawError::ReceiptMismatch)?,
            );
            let amount_lo = u64::from_be_bytes(
                amount[8..]
                    .try_into()
                    .map_err(|_| TransferLawError::ReceiptMismatch)?,
            );
            c_ok(unsafe {
                layerx_programs_call_transfer_leg(
                    self.token,
                    u16::try_from(index).map_err(|_| TransferLawError::InvalidTransferSet)?,
                    from[0],
                    from[1],
                    from[2],
                    from[3],
                    to[0],
                    to[1],
                    to[2],
                    to[3],
                    asset[0],
                    asset[1],
                    asset[2],
                    asset[3],
                    amount_hi,
                    amount_lo,
                )
            })
            .map_err(|_| TransferLawError::ReceiptMismatch)?;
        }
        c_ok(unsafe { layerx_programs_call_transfer_apply(self.token) })
            .map_err(|_| TransferLawError::ReceiptMismatch)?;
        let root = scalar_bytes(32, |offset| unsafe {
            layerx_programs_call_transfer_root_byte(self.token, offset)
        })
        .map_err(|_| TransferLawError::ReceiptMismatch)?;
        Ok(KernelTransferEvidence {
            transfer_set_root: root
                .try_into()
                .map_err(|_| TransferLawError::ReceiptMismatch)?,
            leg_count: transfers.legs().len(),
            total_amount: transfers.total_amount(),
        })
    }
    fn verify_402lxp_transfer_set_root(
        &self,
        transfers: &AtomicTransferSet,
        evidence: &KernelTransferEvidence,
    ) -> Result<(), TransferLawError> {
        if evidence.transfer_set_root == [0; 32]
            || evidence.leg_count != transfers.legs().len()
            || evidence.total_amount != transfers.total_amount()
        {
            Err(TransferLawError::ReceiptMismatch)
        } else {
            Ok(())
        }
    }
}

fn emit_events(token: u64, events: &[ProgramEvent]) -> Result<(), i32> {
    for (index, event) in events.iter().enumerate() {
        let program = words(event.program.bytes());
        let principal = words(event.principal.bytes());
        let (path, depth) = event.frame.canonical_bytes();
        c_ok(unsafe {
            layerx_programs_call_event_begin(
                token,
                u32::try_from(index).map_err(|_| LENGTH_LIMIT)?,
                program[0],
                program[1],
                program[2],
                program[3],
                principal[0],
                principal[1],
                principal[2],
                principal[3],
                u64::from_be_bytes(path),
                depth,
                u16::try_from(event.topic.len()).map_err(|_| LENGTH_LIMIT)?,
                u32::try_from(event.data.len()).map_err(|_| LENGTH_LIMIT)?,
            )
        })?;
        for (offset, byte) in event.topic.iter().copied().enumerate() {
            c_ok(unsafe {
                layerx_programs_call_event_byte(
                    token,
                    KEY,
                    u32::try_from(offset).map_err(|_| LENGTH_LIMIT)?,
                    byte,
                )
            })?;
        }
        for (offset, byte) in event.data.iter().copied().enumerate() {
            c_ok(unsafe {
                layerx_programs_call_event_byte(
                    token,
                    VALUE,
                    u32::try_from(offset).map_err(|_| LENGTH_LIMIT)?,
                    byte,
                )
            })?;
        }
        c_ok(unsafe { layerx_programs_call_event_emit(token) })?;
    }
    Ok(())
}

fn terminal(
    token: u64,
    kind: u8,
    result: i32,
    runtime: u16,
    abi: u16,
    schedule: u32,
    usage: MeteredUsage,
    root: [u8; 32],
    graph: &[u8],
    detail: &[u8],
    events: &[u8],
) -> Result<i32, i32> {
    let root = words(root);
    let fee = usage.fee_units.to_be_bytes();
    let fee_hi = u64::from_be_bytes(fee[..8].try_into().map_err(|_| LENGTH_LIMIT)?);
    let fee_lo = u64::from_be_bytes(fee[8..].try_into().map_err(|_| LENGTH_LIMIT)?);
    c_ok(unsafe {
        layerx_programs_call_terminal_begin(
            token,
            kind,
            result,
            runtime,
            abi,
            schedule,
            usage.cpu_fuel,
            usage.memory_bytes,
            usage.storage_read_bytes,
            usage.storage_write_bytes,
            usage.output_values,
            usage.output_bytes,
            fee_hi,
            fee_lo,
            root[0],
            root[1],
            root[2],
            root[3],
            u32::try_from(graph.len()).map_err(|_| LENGTH_LIMIT)?,
            u32::try_from(detail.len()).map_err(|_| LENGTH_LIMIT)?,
            u32::try_from(events.len()).map_err(|_| LENGTH_LIMIT)?,
        )
    })?;
    for (section, bytes) in [(0_u16, graph), (1_u16, detail), (2_u16, events)] {
        for (offset, byte) in bytes.iter().copied().enumerate() {
            c_ok(unsafe {
                layerx_programs_call_terminal_byte(
                    token,
                    section,
                    u32::try_from(offset).map_err(|_| LENGTH_LIMIT)?,
                    byte,
                )
            })?;
        }
    }
    c_ok(unsafe { layerx_programs_call_terminal_publish(token) })?;
    Ok(result)
}

fn terminal_record_failure(
    token: u64,
    schedule: u16,
    record: &AuthorizedExecutionRecord,
    detail: &[u8],
) -> Result<i32, i32> {
    terminal(
        token,
        FAILURE,
        PROGRAM_REFUSED,
        record.execution.runtime_version,
        record.execution.abi_version,
        u32::from(schedule),
        record.execution.usage,
        [0; 32],
        &record.call_graph.canonical_evidence(),
        detail,
        b"LXP/programs/events/v1\0\0\0\0\0",
    )
}

fn terminal_settlement_failure(
    token: u64,
    schedule: u16,
    failure: SettlementFailure,
) -> Result<i32, i32> {
    let detail = settlement_detail(failure.error());
    terminal(
        token,
        FAILURE,
        PROGRAM_REFUSED,
        failure.execution().runtime_version,
        failure.execution().abi_version,
        u32::from(schedule),
        failure.execution().usage,
        [0; 32],
        &failure.call_graph().canonical_evidence(),
        &detail,
        b"LXP/programs/events/v1\0\0\0\0\0",
    )
}

#[no_mangle]
#[allow(clippy::too_many_arguments, clippy::too_many_lines)]
pub extern "C" fn layerx_programs_call_begin(
    token: u64,
    p0: u64,
    p1: u64,
    p2: u64,
    p3: u64,
    r0: u64,
    r1: u64,
    r2: u64,
    r3: u64,
    h0: u64,
    h1: u64,
    h2: u64,
    h3: u64,
    b0: u64,
    b1: u64,
    b2: u64,
    b3: u64,
    signed_fee_hi: u64,
    signed_fee_lo: u64,
    available_fee_hi: u64,
    available_fee_lo: u64,
    fee_schedule_version: u16,
    parameter_version: u32,
    abi_version: u16,
    entrypoint_length: u16,
    wasm_length: u32,
    calldata_length: u32,
    capabilities_length: u16,
    response_capacity: u32,
    cpu_fuel: u64,
    memory_bytes: u64,
    storage_read_bytes: u64,
    storage_write_bytes: u64,
    output_values: u64,
    output_bytes: u64,
    table_elements: u64,
) -> i32 {
    let run = || -> Result<i32, i32> {
        if token == 0
            || parameter_version == 0
            || fee_schedule_version != 1
            || abi_version != ABI_VERSION
            || entrypoint_length == 0
            || entrypoint_length > 128
            || calldata_length > 1_048_576
            || capabilities_length > 4_096
            || response_capacity > 1_048_576
        {
            return Err(NON_CANONICAL);
        }
        let program = ProgramId::new(bytes([p0, p1, p2, p3])).map_err(|_| NON_CANONICAL)?;
        let payer = PrincipalId::new(bytes([r0, r1, r2, r3])).map_err(|_| NON_CANONICAL)?;
        let authority = bytes([h0, h1, h2, h3]);
        if authority == [0; 32] {
            return Err(NON_CANONICAL);
        }
        let binding =
            ActivityBudgetBinding::new(bytes([b0, b1, b2, b3])).map_err(|_| NON_CANONICAL)?;
        let declared = DeclaredBudget::new(
            cpu_fuel,
            memory_bytes,
            storage_read_bytes,
            storage_write_bytes,
            u32::try_from(output_values).map_err(|_| LENGTH_LIMIT)?,
            output_bytes,
            u32::try_from(table_elements).map_err(|_| LENGTH_LIMIT)?,
        )
        .map_err(|_| NON_CANONICAL)?;
        let executor = Executor::declared();
        let signed_fee = (u128::from(signed_fee_hi) << 64) | u128::from(signed_fee_lo);
        let available_fee = (u128::from(available_fee_hi) << 64) | u128::from(available_fee_lo);
        if crate::budget::maximum_fee_units(
            declared.resource_budget(),
            crate::FeeSchedule::declared(),
        )
        .map_err(|_| NON_CANONICAL)?
            > signed_fee
        {
            return Err(NON_CANONICAL);
        }
        let admitted = executor
            .admit_activity_budget(
                declared,
                crate::budget::PayerCoverage::new(payer, binding, available_fee),
            )
            .map_err(|_| NON_CANONICAL)?;
        let entrypoint = String::from_utf8(scalar_bytes(
            usize::from(entrypoint_length),
            |offset| unsafe { layerx_programs_call_activity_byte(token, ENTRYPOINT, offset) },
        )?)
        .map_err(|_| NON_CANONICAL)?;
        let calldata = scalar_bytes(
            usize::try_from(calldata_length).map_err(|_| LENGTH_LIMIT)?,
            |offset| unsafe { layerx_programs_call_activity_byte(token, CALLDATA, offset) },
        )?;
        let grants = CapabilitySet::decode_canonical(&scalar_bytes(
            usize::from(capabilities_length),
            |offset| unsafe { layerx_programs_call_activity_byte(token, CAPABILITIES, offset) },
        )?)
        .map_err(|_| NON_CANONICAL)?;
        let capabilities = CapabilitySet::new(grants).map_err(|_| NON_CANONICAL)?;
        crate::entrypoint::preflight(&calldata).map_err(|_| NON_CANONICAL)?;
        let engine = WasmEngine::declared().map_err(|_| FATAL_INVARIANT)?;
        let root_wasm = scalar_bytes(
            usize::try_from(wasm_length).map_err(|_| LENGTH_LIMIT)?,
            |offset| unsafe { layerx_programs_call_activity_byte(token, WASM, offset) },
        )?;
        let root_module = engine.validate(&root_wasm).map_err(|_| NON_CANONICAL)?;
        let count = c_count(unsafe { layerx_programs_call_catalog_count(token) })?;
        let mut catalog = ProgramCatalog::new();
        let mut entries = Vec::with_capacity(usize::try_from(count).map_err(|_| LENGTH_LIMIT)?);
        let mut storage = Storage::new();
        for index in 0..count {
            let identity = catalog_identity(token, index, 0)?;
            let hash = catalog_identity(token, index, 1)?;
            if hash == [0; 32] {
                return Err(NON_CANONICAL);
            }
            let entry_program = ProgramId::new(identity).map_err(|_| NON_CANONICAL)?;
            let length =
                c_count(unsafe { layerx_programs_call_catalog_wasm_length(token, index) })?;
            let wasm = scalar_bytes(
                usize::try_from(length).map_err(|_| LENGTH_LIMIT)?,
                |offset| unsafe { layerx_programs_call_catalog_wasm_byte(token, index, offset) },
            )?;
            if entry_program == program && wasm != root_wasm {
                return Err(NON_CANONICAL);
            }
            let module = engine.validate(&wasm).map_err(|_| NON_CANONICAL)?;
            if catalog.insert(entry_program, module).is_some() {
                return Err(NON_CANONICAL);
            }
            import_catalog_storage(token, index, entry_program, payer, &mut storage)?;
            entries.push((index, entry_program));
        }
        if !catalog.contains(program) {
            return Err(MODULE_DISABLED);
        }
        let receipts = CReceiptOracle { token };
        let request = crate::AuthorizedExecutionRequest {
            module: &root_module,
            program,
            authorization: AuthorizationContext::new(payer, capabilities),
            receipts: &receipts,
            entrypoint: &entrypoint,
            calldata: &calldata,
            composition: CompositionContext::catalog(catalog, CompositionRules::declared()),
            response_capacity: usize::try_from(response_capacity).map_err(|_| LENGTH_LIMIT)?,
        };
        match executor
            .prepare_authorized_activity_budgeted(
                &storage,
                BudgetedAuthorizedExecutionRequest::new(request, admitted, payer, binding),
            )
            .map_err(|_| NON_CANONICAL)?
        {
            PreparedAuthorizedActivityOutcome::Success(prepared) => {
                let mut kernel = CKernel { token };
                let mut final_storage = storage;
                let assignment = match prepared.strict_settle(&mut final_storage, &mut kernel) {
                    Ok(assignment) => assignment,
                    Err(failure) => {
                        return terminal_settlement_failure(token, fee_schedule_version, failure)
                    }
                };
                let record = assignment.record();
                if let Err(status) =
                    c_ok(unsafe { layerx_programs_call_storage_final_authorize(token) })
                {
                    return terminal_record_failure(
                        token,
                        fee_schedule_version,
                        record,
                        &callback_detail(1, status),
                    );
                }
                for (index, entry_program) in &entries {
                    if let Err(status) =
                        export_catalog_storage(token, *index, *entry_program, payer, &final_storage)
                    {
                        return terminal_record_failure(
                            token,
                            fee_schedule_version,
                            record,
                            &callback_detail(2, status),
                        );
                    }
                }
                let graph = record.call_graph.canonical_evidence();
                let detail = record.execution.canonical_evidence();
                let events = match record.effects.canonical_program_event_envelope() {
                    Ok(events) => events,
                    Err(_) => {
                        return terminal_record_failure(
                            token,
                            fee_schedule_version,
                            record,
                            &callback_detail(3, NON_CANONICAL),
                        )
                    }
                };
                if let Err(status) = emit_events(token, &record.effects.events) {
                    return terminal_record_failure(
                        token,
                        fee_schedule_version,
                        record,
                        &callback_detail(4, status),
                    );
                }
                terminal(
                    token,
                    SUCCESS,
                    OK,
                    record.execution.runtime_version,
                    record.execution.abi_version,
                    u32::from(fee_schedule_version),
                    record.execution.usage,
                    assignment
                        .settlement()
                        .map_or([0; 32], |settlement| settlement.transfer_set_root()),
                    &graph,
                    &detail,
                    &events,
                )
            }
            PreparedAuthorizedActivityOutcome::Failure(failure) => {
                let detail = typed_failure_detail(failure.cause())?;
                terminal(
                    token,
                    FAILURE,
                    PROGRAM_REFUSED,
                    crate::RUNTIME_VERSION,
                    crate::ABI_VERSION,
                    u32::from(fee_schedule_version),
                    failure.usage(),
                    [0; 32],
                    &failure.call_graph().canonical_evidence(),
                    &detail,
                    b"LXP/programs/events/v1\0\0\0\0\0",
                )
            }
            PreparedAuthorizedActivityOutcome::Resource(resource) => {
                let detail = typed_resource_detail(resource.refusal());
                terminal(
                    token,
                    RESOURCE,
                    GAS_EXHAUSTED,
                    crate::RUNTIME_VERSION,
                    crate::ABI_VERSION,
                    u32::from(fee_schedule_version),
                    resource.usage(),
                    [0; 32],
                    &resource.call_graph().canonical_evidence(),
                    &detail,
                    b"LXP/programs/events/v1\0\0\0\0\0",
                )
            }
        }
    };
    match run() {
        Ok(status) | Err(status) => status,
    }
}
