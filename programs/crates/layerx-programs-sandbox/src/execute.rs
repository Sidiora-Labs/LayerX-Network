//! Lease-scoped execution through the ordinary Programs runtime.

use core::fmt::{self, Display};

use layerx_programs_runtime::{
    AbiError, AuthorizationContext, AuthorizedExecutionRecord, AuthorizedExecutionRequest,
    Capability, CapabilitySet, CompositionContext, ExecutionError, ExecutionFault, Executor,
    FeeSchedule, MeterRefusal, ReceiptOracle, ResourceBudget, ResourceKind, Storage,
    StorageNamespace, ValidatedModule,
};

use crate::{BoundKind, Lease, LeaseRefusal, LeaseState, LeaseUsage};

/// Authority derived entirely from immutable lease state.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct LeaseCapabilities {
    principal: layerx_programs_runtime::PrincipalId,
    namespace: StorageNamespace,
    grants: CapabilitySet,
}

impl LeaseCapabilities {
    /// Derives the only authority available to a sandbox image. The root host
    /// program may access its lease-principal namespace; no shared storage,
    /// transfer, balance, receipt, event, or callee authority is admitted.
    pub fn derive(lease: &Lease) -> Result<Self, SandboxRefusal> {
        let principal = lease.namespace().execution_principal()
            .map_err(SandboxRefusal::Lease)?;
        let namespace = lease.namespace().storage_namespace()
            .map_err(SandboxRefusal::Lease)?;
        let grants = CapabilitySet::new([Capability::StorageRead, Capability::StorageWrite])
            .map_err(SandboxRefusal::Capability)?;
        Ok(Self { principal, namespace, grants })
    }

    #[must_use]
    pub const fn principal(&self) -> layerx_programs_runtime::PrincipalId { self.principal }

    #[must_use]
    pub const fn namespace(&self) -> StorageNamespace { self.namespace }

    #[must_use]
    pub const fn grants(&self) -> &CapabilitySet { &self.grants }

    fn authorization(&self) -> AuthorizationContext {
        AuthorizationContext::new(self.principal, self.grants.clone())
    }
}

/// Borrowed inputs for one ordinary Programs call made on behalf of a lease.
pub struct SandboxExecutionRequest<'a> {
    pub module: &'a ValidatedModule,
    pub receipts: &'a dyn ReceiptOracle,
    pub entrypoint: &'a str,
    pub calldata: &'a [u8],
    pub composition: CompositionContext,
    pub response_capacity: usize,
    pub observed_batch: u64,
}

/// Exact execution result and cumulative lease accounting to commit together.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SandboxExecutionRecord {
    pub execution: AuthorizedExecutionRecord,
    pub activity_usage: LeaseUsage,
    pub cumulative_usage: LeaseUsage,
    pub activity_fee_units: u128,
    pub cumulative_escrow_consumed: u128,
}

/// Typed sandbox refusal. Ceiling exhaustion is structurally distinct from a
/// guest/runtime program failure.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SandboxRefusal {
    Lease(LeaseRefusal),
    Capability(AbiError),
    LeaseNotActive { state: LeaseState },
    LeaseExpired { expiry: u64, observed: u64 },
    CeilingExhausted { bound: BoundKind, limit: u128, attempted: u128 },
    GrowthCeilingExhausted { memory_limit: u64, table_limit: u64 },
    Program(ExecutionError),
    AccountingOverflow { bound: BoundKind },
}

impl Display for SandboxRefusal {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "{self:?}")
    }
}

impl std::error::Error for SandboxRefusal {}

/// Executes one sandbox activity using the production runtime meter and an
/// isolated working storage snapshot. Storage is assigned only on success and
/// only after namespace and escrow ceilings are checked.
pub(crate) fn execute_scoped(
    storage: &mut Storage,
    lease: &Lease,
    prices: FeeSchedule,
    request: SandboxExecutionRequest<'_>,
) -> Result<SandboxExecutionRecord, SandboxRefusal> {
    if lease.state() != LeaseState::Active {
        return Err(SandboxRefusal::LeaseNotActive { state: lease.state() });
    }
    if request.observed_batch >= lease.expiry() {
        return Err(SandboxRefusal::LeaseExpired {
            expiry: lease.expiry(), observed: request.observed_batch,
        });
    }
    let capabilities = LeaseCapabilities::derive(lease)?;
    let limits = lease.limits();
    let prior = lease.usage();
    let budget = ResourceBudget::new_complete(
        remaining(BoundKind::CpuFuel, limits.cpu_fuel, prior.cpu_fuel)?,
        limits.memory_bytes,
        remaining(BoundKind::StorageReadBytes, limits.storage_read_bytes, prior.storage_read_bytes)?,
        remaining(BoundKind::StorageWriteBytes, limits.storage_write_bytes, prior.storage_write_bytes)?,
        u32::try_from(remaining(BoundKind::OutputValues, limits.output_values, prior.output_values)?)
            .map_err(|_| SandboxRefusal::AccountingOverflow { bound: BoundKind::OutputValues })?,
        remaining(BoundKind::OutputBytes, limits.output_bytes, prior.output_bytes)?,
        u32::try_from(limits.table_elements)
            .map_err(|_| SandboxRefusal::AccountingOverflow { bound: BoundKind::TableElements })?,
    );
    let mut held = storage.clone();
    let runtime_request = AuthorizedExecutionRequest {
        module: request.module,
        program: lease.host_program(),
        authorization: capabilities.authorization(),
        receipts: request.receipts,
        entrypoint: request.entrypoint,
        calldata: request.calldata,
        composition: request.composition,
        response_capacity: request.response_capacity,
    };
    let execution = Executor::new(budget, prices)
        .execute_authorized(&mut held, runtime_request)
        .map_err(|error| classify_execution(error, budget))?;
    let metered = execution.execution.usage;
    let namespace_bytes = held.namespace_persistent_bytes(capabilities.namespace())
        .map_err(|_| SandboxRefusal::AccountingOverflow { bound: BoundKind::NamespaceBytes })?;
    if namespace_bytes > limits.namespace_bytes {
        return Err(SandboxRefusal::CeilingExhausted {
            bound: BoundKind::NamespaceBytes,
            limit: u128::from(limits.namespace_bytes),
            attempted: u128::from(namespace_bytes),
        });
    }
    let activity_usage = LeaseUsage {
        cpu_fuel: metered.cpu_fuel,
        memory_bytes: metered.memory_bytes,
        storage_read_bytes: metered.storage_read_bytes,
        storage_write_bytes: metered.storage_write_bytes,
        output_values: u64::from(metered.output_values),
        output_bytes: metered.output_bytes,
        table_elements: 0,
        namespace_bytes,
    };
    let cumulative_usage = LeaseUsage {
        cpu_fuel: add(BoundKind::CpuFuel, prior.cpu_fuel, activity_usage.cpu_fuel)?,
        memory_bytes: prior.memory_bytes.max(activity_usage.memory_bytes),
        storage_read_bytes: add(BoundKind::StorageReadBytes, prior.storage_read_bytes, activity_usage.storage_read_bytes)?,
        storage_write_bytes: add(BoundKind::StorageWriteBytes, prior.storage_write_bytes, activity_usage.storage_write_bytes)?,
        output_values: add(BoundKind::OutputValues, prior.output_values, activity_usage.output_values)?,
        output_bytes: add(BoundKind::OutputBytes, prior.output_bytes, activity_usage.output_bytes)?,
        table_elements: prior.table_elements,
        namespace_bytes,
    };
    let cumulative_escrow_consumed = lease.escrow_consumed().checked_add(metered.fee_units)
        .ok_or(SandboxRefusal::AccountingOverflow { bound: BoundKind::Escrow })?;
    if cumulative_escrow_consumed > lease.escrow_amount() {
        return Err(SandboxRefusal::CeilingExhausted {
            bound: BoundKind::Escrow,
            limit: lease.escrow_amount(),
            attempted: cumulative_escrow_consumed,
        });
    }
    *storage = held;
    Ok(SandboxExecutionRecord {
        execution,
        activity_usage,
        cumulative_usage,
        activity_fee_units: metered.fee_units,
        cumulative_escrow_consumed,
    })
}

fn remaining(bound: BoundKind, limit: u64, consumed: u64) -> Result<u64, SandboxRefusal> {
    limit.checked_sub(consumed).ok_or(SandboxRefusal::CeilingExhausted {
        bound, limit: u128::from(limit), attempted: u128::from(consumed),
    })
}

fn add(bound: BoundKind, left: u64, right: u64) -> Result<u64, SandboxRefusal> {
    left.checked_add(right).ok_or(SandboxRefusal::AccountingOverflow { bound })
}

fn classify_execution(error: ExecutionError, budget: ResourceBudget) -> SandboxRefusal {
    match error {
        ExecutionError::Resource(MeterRefusal::BudgetExceeded { resource, limit, attempted }) => {
            SandboxRefusal::CeilingExhausted {
                bound: match resource {
                    ResourceKind::Cpu => BoundKind::CpuFuel,
                    ResourceKind::Memory => BoundKind::MemoryBytes,
                    ResourceKind::StorageRead => BoundKind::StorageReadBytes,
                    ResourceKind::StorageWrite => BoundKind::StorageWriteBytes,
                    ResourceKind::StorageOccupancy => BoundKind::NamespaceBytes,
                    ResourceKind::Output => BoundKind::OutputValues,
                    ResourceKind::OutputBytes => BoundKind::OutputBytes,
                },
                limit: u128::from(limit),
                attempted: u128::from(attempted),
            }
        }
        ExecutionError::Fault(ExecutionFault::OutOfFuel) => SandboxRefusal::CeilingExhausted {
            bound: BoundKind::CpuFuel,
            limit: u128::from(budget.cpu_fuel()),
            attempted: u128::from(budget.cpu_fuel()).saturating_add(1),
        },
        ExecutionError::Fault(ExecutionFault::GrowthLimited) => {
            SandboxRefusal::GrowthCeilingExhausted {
                memory_limit: budget.memory_bytes(),
                table_limit: u64::from(budget.table_elements()),
            }
        }
        other => SandboxRefusal::Program(other),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use layerx_programs_runtime::test_support::{
        code_section, func_body, function_section, import_section, module, raw_section,
        type_section, unsigned_leb, OP_CALL, OP_END, OP_I32_CONST, TYPE_I32,
    };
    use layerx_programs_runtime::{
        PrincipalId, ReceiptView, WasmEngine, WasmValue, ABI_MODULE, CALL_ENTRY_EXPORT,
    };

    struct NoReceipts;

    impl ReceiptOracle for NoReceipts {
        fn verified_receipt(&self, _digest: [u8; 32]) -> Result<ReceiptView, AbiError> {
            Err(AbiError::ReceiptMismatch)
        }
    }

    fn lease(id: u8) -> Lease {
        Lease::request(
            crate::LeaseId::new([id; 32]).expect("lease id"),
            PrincipalId::new([9; 32]).expect("tenant"),
            layerx_programs_runtime::ProgramId::new([7; 32]).expect("program"),
            [6; 32],
            [5; 32],
            1_000_000,
            crate::LeaseLimits {
                cpu_fuel: 100_000,
                memory_bytes: 65_536,
                storage_read_bytes: 1_024,
                storage_write_bytes: 1_024,
                output_values: 4,
                output_bytes: 1_024,
                table_elements: 1,
                namespace_bytes: 1_024,
            },
            1,
            100,
        ).expect("lease")
    }

    #[test]
    fn capabilities_are_derived_and_contain_no_escape_authority() {
        let lease = lease(1);
        let capabilities = LeaseCapabilities::derive(&lease).expect("capabilities");
        assert_eq!(capabilities.principal(), lease.namespace().execution_principal().expect("principal"));
        assert_eq!(capabilities.namespace(), lease.namespace().storage_namespace().expect("namespace"));
        assert_eq!(capabilities.grants().canonical_encoding(), vec![0, 2, 1, 2]);
    }

    #[test]
    fn adjacent_leases_cannot_observe_the_same_runtime_namespace() {
        let left = LeaseCapabilities::derive(&lease(1)).expect("left");
        let right = LeaseCapabilities::derive(&lease(2)).expect("right");
        assert_ne!(left.principal(), right.principal());
        assert_ne!(left.namespace(), right.namespace());
    }

    #[test]
    fn hostile_authority_families_are_absent_by_construction() {
        let capabilities = LeaseCapabilities::derive(&lease(3)).expect("capabilities");
        let encoded = capabilities.grants().canonical_encoding();
        for hostile_tag in [3u8, 4, 5, 6, 7, 8, 9, 10] {
            assert!(!encoded[2..].contains(&hostile_tag));
        }
    }

    fn exports() -> Vec<u8> {
        let entries = [("layerx_reserve", 0u8, 1u8), (CALL_ENTRY_EXPORT, 0, 2), ("memory", 2, 0)];
        let mut payload = unsigned_leb(entries.len() as u64);
        for (name, kind, index) in entries {
            payload.extend(unsigned_leb(name.len() as u64));
            payload.extend_from_slice(name.as_bytes());
            payload.extend_from_slice(&[kind, index]);
        }
        raw_section(7, &payload)
    }

    fn hostile_host_call_image(function: &str, arity: usize) -> Vec<u8> {
        let params = vec![TYPE_I32; arity];
        let reserve_params = [TYPE_I32];
        let entry_params = [TYPE_I32, TYPE_I32];
        let result = [TYPE_I32];
        let mut entry = Vec::new();
        for _ in 0..arity { entry.extend([OP_I32_CONST, 0]); }
        entry.extend([OP_CALL, 0, OP_END]);
        module(&[
            type_section(&[
                (params.as_slice(), result.as_slice()),
                (reserve_params.as_slice(), result.as_slice()),
                (entry_params.as_slice(), result.as_slice()),
            ]),
            import_section(&[(ABI_MODULE, function, 0)]),
            function_section(&[1, 2]),
            raw_section(5, &[1, 1, 1, 1]),
            exports(),
            code_section(&[
                func_body(&[], &[OP_I32_CONST, 0, OP_END]),
                func_body(&[], &entry),
            ]),
        ])
    }

    #[test]
    fn hostile_images_cannot_emit_or_call_an_unleased_program() {
        let lease = lease(4);
        let capabilities = LeaseCapabilities::derive(&lease).expect("capabilities");
        let engine = WasmEngine::declared().expect("engine");
        for (function, arity) in [("event_emit", 4usize), ("program_call", 6usize)] {
            let module = engine.validate(&hostile_host_call_image(function, arity)).expect("image");
            let mut storage = Storage::new();
            let before = storage.clone();
            let result = Executor::declared().execute_authorized(
                &mut storage,
                AuthorizedExecutionRequest {
                    module: &module,
                    program: lease.host_program(),
                    authorization: capabilities.authorization(),
                    receipts: &NoReceipts,
                    entrypoint: CALL_ENTRY_EXPORT,
                    calldata: &[],
                    composition: CompositionContext::isolated(),
                    response_capacity: 0,
                },
            );
            match result {
                Err(ExecutionError::Abi(AbiError::CapabilityDenied)) => {}
                Ok(record) => {
                    assert!(record.effects.events.is_empty());
                    assert!(record.effects.calls.is_empty());
                    assert!(record.effects.transfers.is_empty());
                    assert!(matches!(record.execution.outputs.as_slice(), [WasmValue::I32(code)] if *code < 0));
                }
                other => panic!("unexpected hostile outcome: {other:?}"),
            }
            assert_eq!(storage, before);
        }
    }
}
