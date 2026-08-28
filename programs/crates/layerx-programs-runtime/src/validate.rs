//! Module validation against the deterministic subset and the declared limits.

use core::fmt::{self, Display};
use std::collections::{BTreeMap, BTreeSet};
use std::sync::Arc;

use wasmparser_nostd::{
    BlockType, FuncType, FunctionBody, Import, Operator, Parser, Payload, Type, TypeRef, ValType,
};

use crate::abi::{Abi, AbiValueType};
use crate::calls::Composition;
use crate::engine::WasmEngine;
use crate::entrypoint::EntrypointRefusal;
use crate::execute::{fault_from_error, ExecutionFault, ProgramInstance};
use crate::host::{self, RuntimeState};
use crate::meter::Meter;
use crate::meter::inject::{FuelSchedule, InjectionRefusal, MeterInjection};

/// Explicitly selected ABI surface used to validate and instantiate a module.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum AbiRevision {
    V1,
    V2,
}

/// A typed refusal produced while validating a module.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ValidationRefusal {
    /// The deployment names an ABI revision this runtime cannot replay.
    UnsupportedAbiVersion { abi_version: u16 },
    /// The module exceeds the declared byte-size limit.
    ModuleTooLarge {
        /// The byte size of the refused module.
        byte_size: u64,
        /// The declared byte-size limit.
        limit: u64,
    },
    /// The module declares more functions than the declared limit allows.
    TooManyFunctions {
        /// The number of functions the module declares.
        function_count: u32,
        /// The declared function-count limit.
        limit: u32,
    },
    /// The module imports a host item outside the declared deterministic ABI.
    ForbiddenImport {
        /// The module field of the refused import.
        import_module: String,
        /// The name field of the refused import.
        import_name: String,
    },
    /// A module imports the same ABI function more than once.
    DuplicateImport { import_module: String, import_name: String },
    /// A declared ABI name was imported as a non-function item.
    WrongImportKind { import_name: String },
    /// A declared ABI function was imported with the wrong type.
    WrongImportSignature { import_name: String },
    /// The module declares a floating-point type.
    ForbiddenFloatType,
    /// The module contains a floating-point instruction.
    ForbiddenFloatInstruction,
    /// The module declares a vector type.
    ForbiddenVectorType,
    /// The module bytes are not a well-formed WASM module.
    MalformedModule {
        /// The parser's reason for refusing the bytes.
        reason: String,
    },
    /// The stripped engine refused the module during validation.
    RejectedByEngine {
        /// The engine's reason for refusing the module.
        reason: String,
    },
    MeterInjection { reason: String },
}

impl Display for ValidationRefusal {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::UnsupportedAbiVersion { abi_version } => {
                write!(f, "unsupported ABI version {abi_version}")
            }
            Self::ModuleTooLarge { byte_size, limit } => {
                write!(
                    f,
                    "module of {byte_size} bytes exceeds declared limit {limit}"
                )
            }
            Self::TooManyFunctions {
                function_count,
                limit,
            } => {
                write!(
                    f,
                    "module declares {function_count} functions exceeding declared limit {limit}"
                )
            }
            Self::ForbiddenImport {
                import_module,
                import_name,
            } => {
                write!(f, "forbidden import {import_module}::{import_name}")
            }
            Self::DuplicateImport { import_module, import_name } => {
                write!(f, "duplicate import {import_module}::{import_name}")
            }
            Self::WrongImportKind { import_name } => {
                write!(f, "ABI import {import_name} is not a function")
            }
            Self::WrongImportSignature { import_name } => {
                write!(f, "ABI import {import_name} has the wrong signature")
            }
            Self::ForbiddenFloatType => write!(f, "floating-point types are forbidden"),
            Self::ForbiddenFloatInstruction => {
                write!(f, "floating-point instructions are forbidden")
            }
            Self::ForbiddenVectorType => write!(f, "vector types are forbidden"),
            Self::MalformedModule { reason } => write!(f, "malformed module: {reason}"),
            Self::RejectedByEngine { reason } => write!(f, "rejected by engine: {reason}"),
            Self::MeterInjection { reason } => write!(f, "meter injection refused: {reason}"),
        }
    }
}

impl std::error::Error for ValidationRefusal {}

/// A module accepted by deterministic-subset validation under declared limits.
#[derive(Debug)]
pub struct ValidatedModule {
    module: wasmi::Module,
    linker: Arc<host::HostLinker>,
    byte_size: u64,
    function_count: u32,
    revision: AbiRevision,
    meter_injection: MeterInjection,
    interface_entry_capability_masks: BTreeMap<String, u16>,
    resumable_globals: Option<Vec<String>>,
    code_hash: [u8; 32],
}

impl ValidatedModule {
    /// Domain-independent SHA-256 identity of the exact validated source module.
    #[must_use]
    pub const fn code_hash(&self) -> [u8; 32] {
        self.code_hash
    }
    /// Reports whether the module exports an ABI-callable program entry point.
    ///
    /// Interface publication uses this after ordinary deterministic module
    /// validation, so a description cannot name a function that deployment
    /// cannot call with the frozen `(i32, i32) -> i32` convention.
    #[must_use]
    pub fn exports_callable_entrypoint(&self, entrypoint: &str) -> bool {
        use wasmi::core::ValueType;

        self.module
            .get_export(entrypoint)
            .and_then(|export| export.func().cloned())
            .is_some_and(|function| {
                function.params() == [ValueType::I32, ValueType::I32]
                    && function.results() == [ValueType::I32]
            })
    }

    /// Reports whether a published interface entry can accept its mandatory
    /// discriminator-prefixed calldata through the complete production call
    /// preflight, including allocator and memory exports.
    #[must_use]
    pub fn supports_interface_entrypoint(&self, entrypoint: &str) -> bool {
        self.preflight_entrypoint(entrypoint, false).is_ok()
    }

    #[must_use]
    pub fn required_interface_capability_mask(&self, entrypoint: &str) -> Option<u16> {
        self.interface_entry_capability_masks.get(entrypoint).copied()
    }

    #[must_use]
    pub fn interface_capability_mask_matches(&self, entrypoint: &str, declared: u16) -> bool {
        exact_interface_capability_mask(
            self.required_interface_capability_mask(entrypoint),
            declared,
        )
    }

    /// Instantiates the validated module for qualification without invoking it.
    ///
    /// # Errors
    ///
    /// Returns the typed engine fault if deterministic instantiation fails.
    pub fn instantiate_for_qualification(&self) -> Result<(), crate::ExecutionFault> {
        let meter = crate::Meter::new(
            crate::ResourceBudget::declared(),
            crate::FeeSchedule::declared(),
        );
        self.instantiate_metered(meter)
            .map(|_| ())
            .map_err(|(fault, _)| fault)
    }
    /// Returns the byte size of the validated module.
    #[must_use]
    pub const fn byte_size(&self) -> u64 {
        self.byte_size
    }

    /// Returns the number of functions the validated module declares.
    #[must_use]
    pub const fn function_count(&self) -> u32 {
        self.function_count
    }

    #[must_use]
    pub const fn abi_revision(&self) -> AbiRevision {
        self.revision
    }

    #[must_use]
    pub const fn meter_injection(&self) -> &MeterInjection { &self.meter_injection }

    #[must_use]
    pub const fn code_hash(&self) -> [u8; 32] { self.meter_injection.original_code_hash() }

    #[must_use]
    pub const fn metering_schedule_version(&self) -> u32 {
        self.meter_injection.schedule().version()
    }

    /// Returns how many times the owning engine built its versioned linker.
    #[must_use]
    pub fn host_linker_construction_count(&self) -> usize {
        self.linker.construction_count()
    }

    /// Returns the number of frozen host functions in this module's shared linker.
    #[must_use]
    pub fn host_function_registration_count(&self) -> usize {
        self.linker.registered_function_count()
    }

    pub(crate) fn preflight_entrypoint(
        &self,
        entrypoint: &str,
        calldata_is_empty: bool,
    ) -> Result<(), EntrypointRefusal> {
        if !self.exports_callable_entrypoint(entrypoint) {
            return Err(EntrypointRefusal::MissingEntry);
        }
        if calldata_is_empty {
            return Ok(());
        }
        self.module
            .get_export(crate::calls::CALL_RESERVE_EXPORT)
            .and_then(|export| export.func().cloned())
            .filter(|function| {
                function.params() == [ValueType::I32] && function.results() == [ValueType::I32]
            })
            .ok_or(EntrypointRefusal::MissingAllocator)?;
        if !matches!(
            self.module.get_export("memory"),
            Some(wasmi::ExternType::Memory(_))
        ) {
            return Err(EntrypointRefusal::MissingMemory);
        }
        Ok(())
    }

    /// Instantiates the validated module in an isolated store.
    ///
    /// # Errors
    ///
    /// Returns a typed [`ExecutionFault`] when instantiation fails or the
    /// module's start function traps.
    pub fn instantiate(&self) -> Result<ProgramInstance, ExecutionFault> {
        self.instantiate_metered(Meter::declared())
            .map_err(|(fault, _)| fault)
    }

    pub fn instantiate_sandbox(
        &self, meter: Meter, abi: Abi,
    ) -> Result<ProgramInstance, ExecutionFault> {
        self.instantiate_state(RuntimeState::sandbox(meter, abi)).map_err(|(fault, _)| fault)
    }

    pub(crate) fn instantiate_metered(
        &self,
        meter: Meter,
    ) -> Result<ProgramInstance, (ExecutionFault, Option<crate::meter::MeterRefusal>)> {
        self.instantiate_state(RuntimeState::isolated(meter))
    }

    pub(crate) fn instantiate_metered_retained_for_qualification(
        &self,
        meter: Meter,
    ) -> Result<ProgramInstance, Box<(ExecutionFault, RuntimeState)>> {
        self.instantiate_state_retained(RuntimeState::isolated(meter))
    }

    pub(crate) fn instantiate_composed(
        &self,
        meter: Meter,
        abi: Abi,
        composition: Composition,
    ) -> Result<ProgramInstance, (ExecutionFault, Option<crate::meter::MeterRefusal>)> {
        self.instantiate_state(RuntimeState::composed(meter, abi, composition))
    }

    pub(crate) fn instantiate_composed_retained(
        &self,
        meter: Meter,
        abi: Abi,
        composition: Composition,
    ) -> Result<ProgramInstance, Box<(ExecutionFault, RuntimeState)>> {
        self.instantiate_state_retained(RuntimeState::composed(meter, abi, composition))
    }

    pub(crate) fn instantiate_composed_response_retained(
        &self,
        meter: Meter,
        abi: Abi,
        composition: Composition,
        capacity: usize,
    ) -> Result<
        Result<ProgramInstance, Box<(ExecutionFault, RuntimeState)>>,
        crate::abi::response::ResponseRefusal,
    > {
        self.instantiate_composed_response_context_retained(
            meter,
            abi,
            composition,
            capacity,
            None,
        )
    }

    pub(crate) fn instantiate_composed_response_context_retained(
        &self,
        meter: Meter,
        abi: Abi,
        composition: Composition,
        capacity: usize,
        context: Option<crate::abi::context::ExecutionContext>,
    ) -> Result<
        Result<ProgramInstance, Box<(ExecutionFault, RuntimeState)>>,
        crate::abi::response::ResponseRefusal,
    > {
        let mut state = RuntimeState::composed_with_response(meter, abi, composition, capacity)?;
        if let Some(context) = context {
            state.authenticate_protocol_context(context);
        }
        Ok(self.instantiate_state_retained(state))
    }

    fn instantiate_state_retained(
        &self,
        mut state: RuntimeState,
    ) -> Result<ProgramInstance, Box<(ExecutionFault, RuntimeState)>> {
        state.bind_metering_schedule(self.meter_injection.schedule());
        let mut store = wasmi::Store::new(self.module.engine(), state);
        store.limiter(|state| state.meter_mut() as &mut dyn wasmi::ResourceLimiter);
        let pre = match self.linker.instantiate(&mut store, &self.module) {
            Ok(pre) => pre,
            Err(error) => {
                let fault = fault_from_error(&error);
                return Err(Box::new(retained_failure(store, fault)));
            }
        };
        let instance = match pre.start(&mut store) {
            Ok(instance) => instance,
            Err(error) => {
                let fault = fault_from_error(&error);
                return Err(Box::new(retained_failure(store, fault)));
            }
        };
        let mut instance = ProgramInstance::new(store, instance);
        instance.declare_resumable_globals(self.resumable_globals.clone());
        instance.bind_validated_code_hash(self.code_hash());
        Ok(instance)
    }

    fn instantiate_state(
        &self,
        mut state: RuntimeState,
    ) -> Result<ProgramInstance, (ExecutionFault, Option<crate::meter::MeterRefusal>)> {
        state.bind_metering_schedule(self.meter_injection.schedule());
        let mut store = wasmi::Store::new(self.module.engine(), state);
        store.limiter(|state| state.meter_mut() as &mut dyn wasmi::ResourceLimiter);
        let pre = self
            .linker
            .instantiate(&mut store, &self.module)
            .map_err(|error| (fault_from_error(&error), store.data().meter().exhaustion()))?;
        let instance = pre
            .start(&mut store)
            .map_err(|error| (fault_from_error(&error), store.data().meter().exhaustion()))?;
        let mut instance = ProgramInstance::new(store, instance);
        instance.declare_resumable_globals(self.resumable_globals.clone());
        instance.bind_validated_code_hash(self.code_hash());
        Ok(instance)
    }
}

fn retained_failure(
    mut store: wasmi::Store<RuntimeState>,
    fault: ExecutionFault,
) -> (ExecutionFault, RuntimeState) {
    if fault == ExecutionFault::OutOfFuel {
        store.data_mut().meter_mut().mark_cpu_exhausted();
    }
    (fault, store.into_data())
}

pub(crate) fn validate_module(
    engine: &WasmEngine,
    wasm: &[u8],
    revision: AbiRevision,
) -> Result<ValidatedModule, ValidationRefusal> {
    validate_module_metered(engine, wasm, revision, FuelSchedule::WASMI_0_31_2)
}

pub(crate) fn validate_module_metered(
    engine: &WasmEngine,
    wasm: &[u8],
    revision: AbiRevision,
    schedule: FuelSchedule,
) -> Result<ValidatedModule, ValidationRefusal> {
    let limits = engine.limits();
    let original = validate_original_module(engine.inner(), limits, wasm, revision)?;
    let meter_injection = MeterInjection::instrument(wasm, schedule)
        .map_err(|refusal: InjectionRefusal| ValidationRefusal::MeterInjection {
            reason: refusal.to_string(),
        })?;
    let module = wasmi::Module::new(engine.inner(), meter_injection.instrumented_wasm())
        .map_err(|error| ValidationRefusal::RejectedByEngine { reason: error.to_string() })?;
    let linker = engine.host_linker();
    Ok(ValidatedModule {
        module,
        linker,
        byte_size: original.byte_size,
        function_count: original.function_count,
        revision,
        meter_injection,
        interface_entry_capability_masks: original.interface_entry_capability_masks,
        resumable_globals: original.resumable_globals,
        code_hash: crate::hash_bytes(crate::HashAlgorithm::Sha256, wasm)
            .map_err(|error| ValidationRefusal::MalformedModule { reason: error.to_string() })?,
    })
}

struct OriginalValidation {
    module: wasmi::Module,
    byte_size: u64,
    function_count: u32,
    interface_entry_capability_masks: BTreeMap<String, u16>,
    resumable_globals: Option<Vec<String>>,
}

fn validate_original_module(
    engine: &wasmi::Engine,
    limits: crate::ValidationLimits,
    wasm: &[u8],
    revision: AbiRevision,
) -> Result<OriginalValidation, ValidationRefusal> {
    let byte_size = wasm.len() as u64;
    if byte_size > limits.max_module_bytes() {
        return Err(ValidationRefusal::ModuleTooLarge {
            byte_size,
            limit: limits.max_module_bytes(),
        });
    }
    let mut function_count: u32 = 0;
    let mut function_types = Vec::new();
    let mut imports = BTreeSet::new();
    let mut imported_function_masks = Vec::new();
    let mut all_imported_capability_mask = 0_u16;
    let mut exported_functions = BTreeMap::new();
    let mut exported_globals = BTreeMap::<u32, Vec<String>>::new();
    let mut mutable_globals = Vec::new();
    let mut function_calls: Vec<(Vec<u32>, bool)> = Vec::new();
    for payload in Parser::new(0).parse_all(wasm) {
        let payload = payload.map_err(|error| ValidationRefusal::MalformedModule {
            reason: error.to_string(),
        })?;
        match payload {
            Payload::TypeSection(reader) => {
                for entry in reader {
                    let entry = entry.map_err(|error| ValidationRefusal::MalformedModule {
                        reason: error.to_string(),
                    })?;
                    let Type::Func(func_type) = entry;
                    for value_type in func_type.params().iter().chain(func_type.results()) {
                        refuse_value_type(*value_type)?;
                    }
                    function_types.push(func_type);
                }
            }
            Payload::ImportSection(reader) => {
                for entry in reader {
                    let entry = entry.map_err(|error| ValidationRefusal::MalformedModule {
                        reason: error.to_string(),
                    })?;
                    refuse_import(&entry, &function_types, revision)?;
                    if matches!(entry.ty, TypeRef::Func(_)) {
                        let mask = interface_capability_for_import(entry.name);
                        imported_function_masks.push(mask);
                        all_imported_capability_mask |= mask;
                    }
                    if !imports.insert((entry.module.to_string(), entry.name.to_string())) {
                        return Err(ValidationRefusal::DuplicateImport {
                            import_module: entry.module.to_string(),
                            import_name: entry.name.to_string(),
                        });
                    }
                }
            }
            Payload::FunctionSection(reader) => {
                function_count = reader.count();
                if function_count > limits.max_functions() {
                    return Err(ValidationRefusal::TooManyFunctions {
                        function_count,
                        limit: limits.max_functions(),
                    });
                }
            }
            Payload::ExportSection(reader) => {
                for entry in reader {
                    let entry = entry.map_err(|error| ValidationRefusal::MalformedModule {
                        reason: error.to_string(),
                    })?;
                    if entry.kind == wasmparser_nostd::ExternalKind::Func {
                        exported_functions.insert(entry.name.to_string(), entry.index);
                    } else if entry.kind == wasmparser_nostd::ExternalKind::Global {
                        exported_globals.entry(entry.index).or_default().push(entry.name.to_string());
                    }
                }
            }
            Payload::GlobalSection(reader) => {
                for entry in reader {
                    let entry = entry.map_err(|error| ValidationRefusal::MalformedModule {
                        reason: error.to_string(),
                    })?;
                    refuse_value_type(entry.ty.content_type)?;
                    mutable_globals.push(entry.ty.mutable);
                }
            }
            Payload::CodeSectionEntry(body) => {
                refuse_float_code(&body)?;
                function_calls.push(interface_calls(&body)?);
            }
            _ => {}
        }
    }
    // Only after the original guest has passed the public ABI and deterministic
    // subset checks may the runtime-private import be introduced.
    let module = wasmi::Module::new(engine, wasm).map_err(|error| {
        ValidationRefusal::RejectedByEngine {
            reason: error.to_string(),
        }
    })?;
    let imported_function_count = u32::try_from(imported_function_masks.len()).map_err(|_| {
        ValidationRefusal::MalformedModule { reason: "imported function count exceeds u32".into() }
    })?;
    let interface_entry_capability_masks = exported_functions
        .into_iter()
        .map(|(name, function_index)| {
            let mask = reachable_interface_capabilities(
                function_index,
                imported_function_count,
                &imported_function_masks,
                &function_calls,
                all_imported_capability_mask,
            );
            (name, mask)
        })
        .collect();
    let mut resumable_globals = Vec::new();
    let mut complete = true;
    for (index, mutable) in mutable_globals.into_iter().enumerate() {
        if !mutable { continue; }
        let names = exported_globals.get(&(index as u32));
        if let Some(names) = names.filter(|names| names.len() == 1) {
            resumable_globals.push(names[0].clone());
        } else {
            complete = false;
        }
    }
    resumable_globals.sort();
    Ok(OriginalValidation {
        module,
        byte_size,
        function_count,
        interface_entry_capability_masks,
        resumable_globals: complete.then_some(resumable_globals),
    })
}

fn interface_calls(body: &FunctionBody<'_>) -> Result<(Vec<u32>, bool), ValidationRefusal> {
    let mut direct = Vec::new();
    let mut ambiguous_indirect = false;
    let reader = body.get_operators_reader().map_err(|error| ValidationRefusal::MalformedModule {
        reason: error.to_string(),
    })?;
    for operator in reader {
        match operator.map_err(|error| ValidationRefusal::MalformedModule {
            reason: error.to_string(),
        })? {
            Operator::Call { function_index } | Operator::ReturnCall { function_index } => {
                direct.push(function_index);
            }
            Operator::CallIndirect { .. } | Operator::ReturnCallIndirect { .. } => {
                ambiguous_indirect = true;
            }
            _ => {}
        }
    }
    Ok((direct, ambiguous_indirect))
}

fn reachable_interface_capabilities(
    root: u32,
    imported_count: u32,
    imported_masks: &[u16],
    defined_calls: &[(Vec<u32>, bool)],
    all_imported_mask: u16,
) -> u16 {
    let mut required = 0_u16;
    let mut pending = vec![root];
    let mut visited = BTreeSet::new();
    while let Some(function_index) = pending.pop() {
        if !visited.insert(function_index) {
            continue;
        }
        if function_index < imported_count {
            required |= imported_masks[function_index as usize];
            continue;
        }
        let Some((calls, ambiguous_indirect)) = defined_calls
            .get((function_index - imported_count) as usize)
        else {
            // A malformed index is rejected by the engine below; fail closed here too.
            required |= all_imported_mask;
            continue;
        };
        if *ambiguous_indirect {
            required |= all_imported_mask;
        }
        pending.extend(calls.iter().copied());
    }
    required
}

fn exact_interface_capability_mask(required: Option<u16>, declared: u16) -> bool {
    required == Some(declared)
}

fn interface_capability_for_import(name: &str) -> u16 {
    match name {
        "storage_read" => 1 << 0,
        "storage_write" | "storage_delete" => 1 << 1,
        "storage_read_scoped" | "storage_scan_scoped" => (1 << 0) | (1 << 2),
        "storage_write_scoped" | "storage_delete_scoped" | "storage_drop_scoped" => (1 << 1) | (1 << 3),
        "event_emit" => 1 << 4,
        "program_call" | "program_call_response" => 1 << 5,
        "transfer_402" | "fund_program_402" => 1 << 6,
        "transfer_program_402" => 1 << 7,
        "receipt_read" => 1 << 8,
        "balance_read" => 1 << 9,
        _ => 0,
    }
}

pub(crate) fn validate_original_for_qualification(
    engine: &wasmi::Engine,
    limits: crate::ValidationLimits,
    wasm: &[u8],
    revision: AbiRevision,
) -> Result<wasmi::Module, ValidationRefusal> {
    validate_original_module(engine, limits, wasm, revision).map(|validated| validated.module)
}

fn refuse_import(
    import: &Import<'_>,
    types: &[FuncType],
    revision: AbiRevision,
) -> Result<(), ValidationRefusal> {
    let version = match revision {
        AbiRevision::V1 => crate::abi::manifest::ABI_V1_VERSION,
        AbiRevision::V2 => crate::abi::manifest::ABI_V2_VERSION,
    };
    let declaration = crate::abi::manifest::permitted_import(
        version,
        import.module,
        import.name,
    )
    .ok_or_else(|| ValidationRefusal::ForbiddenImport {
            import_module: import.module.to_string(),
            import_name: import.name.to_string(),
        })?;
    let TypeRef::Func(type_index) = import.ty else {
        return Err(ValidationRefusal::WrongImportKind {
            import_name: import.name.to_string(),
        });
    };
    let function_type =
        types
            .get(type_index as usize)
            .ok_or_else(|| ValidationRefusal::MalformedModule {
                reason: format!("import {} references absent type {type_index}", import.name),
            })?;
    if values_match(function_type.params(), declaration.params)
        && values_match(function_type.results(), declaration.results)
    {
        Ok(())
    } else {
        Err(ValidationRefusal::WrongImportSignature {
            import_name: import.name.to_string(),
        })
    }
}

fn values_match(actual: &[ValType], expected: &[AbiValueType]) -> bool {
    actual.len() == expected.len()
        && actual.iter().zip(expected).all(|(actual, expected)| {
            matches!(
                (actual, expected),
                (ValType::I32, AbiValueType::I32) | (ValType::I64, AbiValueType::I64)
            )
        })
}

fn refuse_value_type(value_type: ValType) -> Result<(), ValidationRefusal> {
    match value_type {
        ValType::F32 | ValType::F64 => Err(ValidationRefusal::ForbiddenFloatType),
        ValType::V128 => Err(ValidationRefusal::ForbiddenVectorType),
        ValType::I32 | ValType::I64 | ValType::FuncRef | ValType::ExternRef => Ok(()),
    }
}

fn refuse_block_type(block_type: BlockType) -> Result<(), ValidationRefusal> {
    match block_type {
        BlockType::Type(value_type) => refuse_value_type(value_type),
        BlockType::Empty | BlockType::FuncType(_) => Ok(()),
    }
}

fn refuse_float_code(body: &FunctionBody<'_>) -> Result<(), ValidationRefusal> {
    let locals = body
        .get_locals_reader()
        .map_err(|error| ValidationRefusal::MalformedModule {
            reason: error.to_string(),
        })?;
    for local in locals {
        let (_, value_type) = local.map_err(|error| ValidationRefusal::MalformedModule {
            reason: error.to_string(),
        })?;
        refuse_value_type(value_type)?;
    }
    let operators =
        body.get_operators_reader()
            .map_err(|error| ValidationRefusal::MalformedModule {
                reason: error.to_string(),
            })?;
    for operator in operators {
        let operator = operator.map_err(|error| ValidationRefusal::MalformedModule {
            reason: error.to_string(),
        })?;
        match operator {
            Operator::Block { blockty } | Operator::Loop { blockty } | Operator::If { blockty } => {
                refuse_block_type(blockty)?;
            }
            Operator::TypedSelect { ty } => refuse_value_type(ty)?,
            other => {
                if operator_uses_float(&other) {
                    return Err(ValidationRefusal::ForbiddenFloatInstruction);
                }
            }
        }
    }
    Ok(())
}

fn operator_uses_float(operator: &Operator<'_>) -> bool {
    matches!(
        operator,
        Operator::F32Load { .. }
            | Operator::F64Load { .. }
            | Operator::F32Store { .. }
            | Operator::F64Store { .. }
            | Operator::F32Const { .. }
            | Operator::F64Const { .. }
            | Operator::F32Eq
            | Operator::F32Ne
            | Operator::F32Lt
            | Operator::F32Gt
            | Operator::F32Le
            | Operator::F32Ge
            | Operator::F64Eq
            | Operator::F64Ne
            | Operator::F64Lt
            | Operator::F64Gt
            | Operator::F64Le
            | Operator::F64Ge
            | Operator::F32Abs
            | Operator::F32Neg
            | Operator::F32Ceil
            | Operator::F32Floor
            | Operator::F32Trunc
            | Operator::F32Nearest
            | Operator::F32Sqrt
            | Operator::F32Add
            | Operator::F32Sub
            | Operator::F32Mul
            | Operator::F32Div
            | Operator::F32Min
            | Operator::F32Max
            | Operator::F32Copysign
            | Operator::F64Abs
            | Operator::F64Neg
            | Operator::F64Ceil
            | Operator::F64Floor
            | Operator::F64Trunc
            | Operator::F64Nearest
            | Operator::F64Sqrt
            | Operator::F64Add
            | Operator::F64Sub
            | Operator::F64Mul
            | Operator::F64Div
            | Operator::F64Min
            | Operator::F64Max
            | Operator::F64Copysign
            | Operator::I32TruncF32S
            | Operator::I32TruncF32U
            | Operator::I32TruncF64S
            | Operator::I32TruncF64U
            | Operator::I64TruncF32S
            | Operator::I64TruncF32U
            | Operator::I64TruncF64S
            | Operator::I64TruncF64U
            | Operator::F32ConvertI32S
            | Operator::F32ConvertI32U
            | Operator::F32ConvertI64S
            | Operator::F32ConvertI64U
            | Operator::F32DemoteF64
            | Operator::F64ConvertI32S
            | Operator::F64ConvertI32U
            | Operator::F64ConvertI64S
            | Operator::F64ConvertI64U
            | Operator::F64PromoteF32
            | Operator::I32ReinterpretF32
            | Operator::I64ReinterpretF64
            | Operator::F32ReinterpretI32
            | Operator::F64ReinterpretI64
            | Operator::I32TruncSatF32S
            | Operator::I32TruncSatF32U
            | Operator::I32TruncSatF64S
            | Operator::I32TruncSatF64U
            | Operator::I64TruncSatF32S
            | Operator::I64TruncSatF32U
            | Operator::I64TruncSatF64S
            | Operator::I64TruncSatF64U
    )
}

#[cfg(test)]
mod linker_invariant_tests {
    use std::sync::Arc;

    use super::*;
    use crate::calls::{ProgramCatalog, ProgramResolver};
    use crate::test_support::add_module;
    use crate::ProgramId;

    #[test]
    fn interface_capabilities_are_export_specific_and_transitive() {
        use crate::test_support::{
            code_section, export_section, func_body, function_section, import_section, module,
            type_section, OP_CALL, OP_DROP, OP_END, OP_I32_CONST, OP_LOCAL_GET, TYPE_I32,
        };

        let host_call = [TYPE_I32, TYPE_I32, TYPE_I32, TYPE_I32];
        let entry = [TYPE_I32, TYPE_I32];
        let direct = |import: u8| {
            func_body(
                &[],
                &[
                    OP_I32_CONST, 0, OP_I32_CONST, 0, OP_I32_CONST, 0, OP_I32_CONST, 0,
                    OP_CALL, import, OP_DROP, OP_I32_CONST, 0, OP_END,
                ],
            )
        };
        let wasm = module(&[
            type_section(&[(&host_call, &[TYPE_I32]), (&entry, &[TYPE_I32])]),
            import_section(&[
                ("layerx_v1", "storage_read", 0),
                ("layerx_v1", "event_emit", 0),
            ]),
            function_section(&[1, 1, 1]),
            export_section(&[("read", 2), ("emit", 3), ("transitive", 4)]),
            code_section(&[
                direct(0),
                direct(1),
                func_body(
                    &[],
                    &[
                        OP_LOCAL_GET, 0, OP_LOCAL_GET, 1, OP_CALL, 2, OP_DROP, OP_I32_CONST, 0,
                        OP_END,
                    ],
                ),
            ]),
        ]);
        let engine = WasmEngine::declared()
            .unwrap_or_else(|error| panic!("declared engine refused: {error}"));
        let validated = engine
            .validate(&wasm)
            .unwrap_or_else(|error| panic!("reachability module refused: {error}"));

        assert_eq!(validated.required_interface_capability_mask("read"), Some(1 << 0));
        assert_eq!(validated.required_interface_capability_mask("emit"), Some(1 << 4));
        assert_eq!(
            validated.required_interface_capability_mask("transitive"),
            Some(1 << 0)
        );
    }

    #[test]
    fn reachable_indirect_call_requires_every_imported_capability_class() {
        let imports = [1 << 0, 1 << 5, 0];
        let calls = vec![(Vec::new(), true)];
        assert_eq!(
            reachable_interface_capabilities(3, 3, &imports, &calls, (1 << 0) | (1 << 5)),
            (1 << 0) | (1 << 5)
        );
    }

    #[test]
    fn exact_interface_capability_match_refuses_irrelevant_overclaim() {
        assert!(exact_interface_capability_mask(Some(1 << 0), 1 << 0));
        assert!(!exact_interface_capability_mask(
            Some(1 << 0),
            (1 << 0) | (1 << 4)
        ));
        assert!(!exact_interface_capability_mask(None, 1 << 0));
    }

    #[test]
    fn nested_resolution_reuses_the_engine_owned_linker() {
        let engine = WasmEngine::declared()
            .unwrap_or_else(|error| panic!("declared engine refused: {error}"));
        assert_eq!(engine.host_linker_construction_count(), 1);
        assert_eq!(
            engine.host_function_registration_count(),
            crate::abi::HOST_FUNCTIONS.len()
                + crate::abi::manifest::ABI_V2_HOST_FUNCTIONS.len()
        );

        let wasm = add_module();
        let root = engine
            .validate(&wasm)
            .unwrap_or_else(|error| panic!("root validation refused: {error}"));
        let child = engine
            .validate(&wasm)
            .unwrap_or_else(|error| panic!("child validation refused: {error}"));
        let candidate = engine
            .validate_v2(&wasm)
            .unwrap_or_else(|error| panic!("candidate validation refused: {error}"));
        let child_program = ProgramId::new([0x42; 32])
            .unwrap_or_else(|error| panic!("child program refused: {error}"));
        let mut catalog = ProgramCatalog::new();
        assert!(catalog.insert(child_program, child).is_none());
        let resolved = match catalog.program_module(child_program) {
            Some(module) => module,
            None => panic!("nested resolver lost the child module"),
        };

        assert!(Arc::ptr_eq(&root.linker, &resolved.linker));
        assert!(Arc::ptr_eq(&root.linker, &candidate.linker));
        root.instantiate_for_qualification()
            .unwrap_or_else(|error| panic!("root instantiation refused: {error}"));
        resolved
            .instantiate_for_qualification()
            .unwrap_or_else(|error| panic!("nested instantiation refused: {error}"));
        assert_eq!(root.host_linker_construction_count(), 1);
        assert_eq!(resolved.host_linker_construction_count(), 1);
    }
}
