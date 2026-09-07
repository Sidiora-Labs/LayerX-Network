//! Lease-scoped execution through the ordinary Programs runtime.

use core::fmt::{self, Display};

use layerx_programs_runtime::{
    AbiError, AuthorizedExecutionRecord, Capability, CapabilitySet, CompositionContext,
    ExecutionError, ReceiptOracle, StorageNamespace, ValidatedModule,
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
    /// # Errors
    ///
    /// Returns a refusal when the lease namespace, execution principal or capability set is invalid.
    pub fn derive(lease: &Lease) -> Result<Self, SandboxRefusal> {
        let principal = lease
            .namespace()
            .execution_principal()
            .map_err(SandboxRefusal::Lease)?;
        let namespace = lease
            .namespace()
            .storage_namespace()
            .map_err(SandboxRefusal::Lease)?;
        let grants = CapabilitySet::new([Capability::StorageRead, Capability::StorageWrite])
            .map_err(SandboxRefusal::Capability)?;
        Ok(Self {
            principal,
            namespace,
            grants,
        })
    }

    #[must_use]
    pub const fn principal(&self) -> layerx_programs_runtime::PrincipalId {
        self.principal
    }

    #[must_use]
    pub const fn namespace(&self) -> StorageNamespace {
        self.namespace
    }

    #[must_use]
    pub const fn grants(&self) -> &CapabilitySet {
        &self.grants
    }

    #[cfg(test)]
    fn authorization(&self) -> layerx_programs_runtime::AuthorizationContext {
        layerx_programs_runtime::AuthorizationContext::new(self.principal, self.grants.clone())
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
    LeaseNotActive {
        state: LeaseState,
    },
    LeaseExpired {
        expiry: u64,
        observed: u64,
    },
    CeilingExhausted {
        bound: BoundKind,
        limit: u128,
        attempted: u128,
    },
    GrowthCeilingExhausted {
        memory_limit: u64,
        table_limit: u64,
    },
    Program(ExecutionError),
    AccountingOverflow {
        bound: BoundKind,
    },
}

impl Display for SandboxRefusal {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "{self:?}")
    }
}

impl std::error::Error for SandboxRefusal {}

#[cfg(test)]
mod tests {
    use super::*;
    use layerx_programs_runtime::test_support::{
        code_section, func_body, function_section, import_section, module, raw_section,
        type_section, unsigned_leb, OP_CALL, OP_END, OP_I32_CONST, TYPE_I32,
    };
    use layerx_programs_runtime::{
        AuthorizedExecutionRequest, Executor, PrincipalId, ReceiptView, Storage, WasmEngine,
        ABI_MODULE, CALL_ENTRY_EXPORT,
    };

    struct NoReceipts;

    impl ReceiptOracle for NoReceipts {
        fn verified_receipt(&self, _digest: [u8; 32]) -> Result<ReceiptView, AbiError> {
            Err(AbiError::ReceiptMismatch)
        }
    }

    fn lease(id: u8) -> Lease {
        Lease::request(
            crate::LeaseId::new([id; 32]).unwrap_or_else(|error| panic!("lease id: {error:?}")),
            PrincipalId::new([9; 32]).unwrap_or_else(|error| panic!("tenant: {error:?}")),
            layerx_programs_runtime::ProgramId::new([7; 32])
                .unwrap_or_else(|error| panic!("program: {error:?}")),
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
        )
        .unwrap_or_else(|error| panic!("lease: {error:?}"))
    }

    #[test]
    fn capabilities_are_derived_and_contain_no_escape_authority() {
        let lease = lease(1);
        let capabilities = LeaseCapabilities::derive(&lease)
            .unwrap_or_else(|error| panic!("capabilities: {error:?}"));
        assert_eq!(
            capabilities.principal(),
            lease
                .namespace()
                .execution_principal()
                .unwrap_or_else(|error| panic!("principal: {error:?}"))
        );
        assert_eq!(
            capabilities.namespace(),
            lease
                .namespace()
                .storage_namespace()
                .unwrap_or_else(|error| panic!("namespace: {error:?}"))
        );
        assert_eq!(capabilities.grants().canonical_encoding(), vec![0, 2, 1, 2]);
    }

    #[test]
    fn adjacent_leases_cannot_observe_the_same_runtime_namespace() {
        let left =
            LeaseCapabilities::derive(&lease(1)).unwrap_or_else(|error| panic!("left: {error:?}"));
        let right =
            LeaseCapabilities::derive(&lease(2)).unwrap_or_else(|error| panic!("right: {error:?}"));
        assert_ne!(left.principal(), right.principal());
        assert_ne!(left.namespace(), right.namespace());
    }

    #[test]
    fn hostile_authority_families_are_absent_by_construction() {
        let capabilities = LeaseCapabilities::derive(&lease(3))
            .unwrap_or_else(|error| panic!("capabilities: {error:?}"));
        let encoded = capabilities.grants().canonical_encoding();
        for hostile_tag in [3u8, 4, 5, 6, 7, 8, 9, 10] {
            assert!(!encoded[2..].contains(&hostile_tag));
        }
    }

    fn exports() -> Vec<u8> {
        let entries = [
            ("layerx_reserve", 0u8, 1u8),
            (CALL_ENTRY_EXPORT, 0, 2),
            ("memory", 2, 0),
        ];
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
        let arguments = if function == "program_call" {
            vec![0, 32, 0, 0, 32, 2]
        } else {
            vec![0; arity]
        };
        for argument in arguments {
            entry.extend([OP_I32_CONST, argument]);
        }
        let mut data = vec![1, 0, OP_I32_CONST, 0, OP_END, 32];
        data.extend([8; 32]);
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
            raw_section(11, &data),
        ])
    }

    #[test]
    fn hostile_images_cannot_emit_or_call_an_unleased_program() {
        let lease = lease(4);
        let capabilities = LeaseCapabilities::derive(&lease)
            .unwrap_or_else(|error| panic!("capabilities: {error:?}"));
        let engine = WasmEngine::declared().unwrap_or_else(|error| panic!("engine: {error:?}"));
        for (function, arity) in [("event_emit", 4usize), ("program_call", 6usize)] {
            let module = engine
                .validate(&hostile_host_call_image(function, arity))
                .unwrap_or_else(|error| panic!("image: {error:?}"));
            let mut catalog = layerx_programs_runtime::ProgramCatalog::new();
            assert!(catalog
                .insert(
                    layerx_programs_runtime::ProgramId::new([8; 32])
                        .unwrap_or_else(|error| panic!("callee: {error:?}")),
                    engine
                        .validate(&hostile_host_call_image(function, arity))
                        .unwrap_or_else(|error| panic!("callee image: {error:?}"))
                )
                .is_none());
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
                    composition: CompositionContext::catalog(
                        catalog,
                        layerx_programs_runtime::CompositionRules::declared(),
                    ),
                    response_capacity: 0,
                },
            );
            let expected = if function == "event_emit" {
                ExecutionError::Entrypoint(
                    layerx_programs_runtime::EntrypointRefusal::GuestRefused { code: -1 },
                )
            } else {
                ExecutionError::Composition(layerx_programs_runtime::CompositionRefusal::Authority(
                    AbiError::CapabilityDenied,
                ))
            };
            assert_eq!(result, Err(expected));
            assert_eq!(storage, before);
        }
    }
}
