//! Deterministic engine-neutral instrumentation of guest WebAssembly.

use core::fmt::{self, Display};

use sha2::{Digest, Sha256};
use wasm_instrument::{
    gas_metering::{host_function, inject, BranchCost, WasmiFuelCosts, WasmiParityRules},
    parity_wasm::elements,
};
use wasmparser_nostd::{
    BlockType, FrameKind, FuncValidator, FuncValidatorAllocations, FunctionBody, Operator, Parser,
    ValidPayload, Validator, ValidatorResources, WasmFuncType, WasmModuleResources,
};

pub const PRIVATE_METER_MODULE: &str = "layerx_private_metering/v1";
pub const PRIVATE_CHARGE_FUNCTION: &str = "charge_i64";
pub const PRIVATE_CHECK_FUNCTION: &str = "check_i64";
pub const PRIVATE_CHARGE_SIGNATURE: &[u8] = b"(i64)->()";
pub const PRIVATE_CHECK_SIGNATURE: &[u8] = b"(i64)->()";
pub const METER_INJECTION_DOMAIN: &[u8] = b"layerx-meter-injection-v1\0";
pub const GENESIS_METERING_SCHEDULE_VERSION: u32 = 1;

/// Frozen prices matching Wasmi 0.31.2's default `FuelCosts` exactly.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FuelSchedule {
    version: u32,
    base: u64,
    entity: u64,
    load: u64,
    store: u64,
    call: u64,
    branch_kept_per_fuel: u64,
    func_locals_per_fuel: u64,
    memory_bytes_per_fuel: u64,
    table_elements_per_fuel: u64,
}

impl FuelSchedule {
    pub const WASMI_0_31_2: Self = Self {
        version: GENESIS_METERING_SCHEDULE_VERSION,
        base: 1,
        entity: 1,
        load: 1,
        store: 1,
        call: 1,
        branch_kept_per_fuel: 8,
        func_locals_per_fuel: 8,
        memory_bytes_per_fuel: 64,
        table_elements_per_fuel: 8,
    };

    #[must_use]
    pub const fn version(self) -> u32 { self.version }

    #[must_use]
    pub fn canonical_bytes(self) -> [u8; 76] {
        let mut bytes = [0_u8; 76];
        bytes[0..4].copy_from_slice(&self.version.to_be_bytes());
        for (index, value) in [self.base, self.entity, self.load, self.store, self.call,
            self.branch_kept_per_fuel, self.func_locals_per_fuel,
            self.memory_bytes_per_fuel, self.table_elements_per_fuel]
            .into_iter().enumerate()
        {
            let start = 4 + index * 8;
            bytes[start..start + 8].copy_from_slice(&value.to_be_bytes());
        }
        bytes
    }

    /// Decodes the exact governed protocol-state record; no node-local default is consulted.
    pub fn from_protocol_bytes(bytes: &[u8]) -> Result<Self, InjectionRefusal> {
        if bytes.len() != 76 {
            return Err(InjectionRefusal::InvalidScheduleEncoding { byte_length: bytes.len() });
        }
        let version = u32::from_be_bytes(copy_array::<4>(&bytes[0..4])?);
        if version != GENESIS_METERING_SCHEDULE_VERSION {
            return Err(InjectionRefusal::UnknownSchedule { version });
        }
        let mut coefficients = [0_u64; 9];
        for (index, coefficient) in coefficients.iter_mut().enumerate() {
            let start = 4 + index * 8;
            *coefficient = u64::from_be_bytes(copy_array::<8>(&bytes[start..start + 8])?);
            if *coefficient == 0 {
                return Err(InjectionRefusal::ZeroScheduleCoefficient { index: index as u8 });
            }
        }
        let schedule = Self {
            version,
            base: coefficients[0],
            entity: coefficients[1],
            load: coefficients[2],
            store: coefficients[3],
            call: coefficients[4],
            branch_kept_per_fuel: coefficients[5],
            func_locals_per_fuel: coefficients[6],
            memory_bytes_per_fuel: coefficients[7],
            table_elements_per_fuel: coefficients[8],
        };
        let expected = Self::WASMI_0_31_2;
        let expected_coefficients = [
            expected.base,
            expected.entity,
            expected.load,
            expected.store,
            expected.call,
            expected.branch_kept_per_fuel,
            expected.func_locals_per_fuel,
            expected.memory_bytes_per_fuel,
            expected.table_elements_per_fuel,
        ];
        for (index, (actual, expected)) in coefficients
            .into_iter()
            .zip(expected_coefficients)
            .enumerate()
        {
            if actual != expected {
                return Err(InjectionRefusal::ScheduleCoefficientMismatch {
                    index: index as u8,
                    expected,
                    actual,
                });
            }
        }
        Ok(schedule)
    }
}

impl Default for FuelSchedule {
    fn default() -> Self { Self::WASMI_0_31_2 }
}

/// Executable artifact whose Wasm bodies contain unavoidable private charge calls.
///
/// Because the charge sites and prices live in these validated bytes instead of
/// an engine's private instruction stream, a later execution tier can consume
/// the same artifact without redefining what the program costs.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MeterInjection {
    schedule: FuelSchedule,
    original_code_hash: [u8; 32],
    original_function_count: u32,
    instrumented_wasm: Vec<u8>,
    digest: [u8; 32],
}

impl MeterInjection {
    /// Rewrites every defined function using the protocol-owned parity schedule.
    pub fn instrument(wasm: &[u8], schedule: FuelSchedule) -> Result<Self, InjectionRefusal> {
        if schedule != FuelSchedule::WASMI_0_31_2 {
            return Err(InjectionRefusal::UnknownSchedule { version: schedule.version() });
        }
        let module: elements::Module = elements::deserialize_buffer(wasm).map_err(|error| {
            InjectionRefusal::MalformedModule { reason: error.to_string() }
        })?;
        refuse_private_import_collision(&module)?;
        let function_count = module.functions_space();
        let original_function_count = u32::try_from(function_count)
            .map_err(|_| InjectionRefusal::FunctionCountOutOfRange { function_count })?;
        let branch_costs = analyze_branch_costs(wasm, schedule)?;
        let rules = WasmiParityRules::new(schedule.wasmi_costs()?, branch_costs);
        let backend = host_function::Injector::new(PRIVATE_METER_MODULE, PRIVATE_CHARGE_FUNCTION)
            .with_dynamic_check(PRIVATE_CHECK_FUNCTION);
        let instrumented = inject(module, backend, &rules)
            .map_err(|_| InjectionRefusal::UnsupportedInstruction)?;
        verify_private_charge_surface(&instrumented)?;
        let instrumented_wasm = elements::serialize(instrumented).map_err(|error| {
            InjectionRefusal::Serialization { reason: error.to_string() }
        })?;
        let original_code_hash: [u8; 32] = Sha256::digest(wasm).into();
        let mut hasher = Sha256::new();
        hasher.update(METER_INJECTION_DOMAIN);
        hasher.update(schedule.canonical_bytes());
        hasher.update(original_code_hash);
        hasher.update(PRIVATE_METER_MODULE.as_bytes());
        hasher.update(PRIVATE_CHARGE_FUNCTION.as_bytes());
        hasher.update(PRIVATE_CHARGE_SIGNATURE);
        hasher.update(PRIVATE_CHECK_FUNCTION.as_bytes());
        hasher.update(PRIVATE_CHECK_SIGNATURE);
        hasher.update(&instrumented_wasm);
        let digest = hasher.finalize().into();
        Ok(Self {
            schedule,
            original_code_hash,
            original_function_count,
            instrumented_wasm,
            digest,
        })
    }

    #[must_use]
    pub const fn schedule(&self) -> FuelSchedule { self.schedule }
    /// Hash of the exact source module from which these executable bytes were derived.
    #[must_use]
    pub const fn original_code_hash(&self) -> [u8; 32] { self.original_code_hash }
    #[must_use]
    pub const fn original_function_count(&self) -> u32 { self.original_function_count }
    #[must_use]
    pub fn instrumented_wasm(&self) -> &[u8] { &self.instrumented_wasm }
    #[must_use]
    pub const fn digest(&self) -> [u8; 32] { self.digest }
    #[must_use]
    pub fn into_instrumented_wasm(self) -> Vec<u8> { self.instrumented_wasm }
}

impl FuelSchedule {
    fn wasmi_costs(self) -> Result<WasmiFuelCosts, InjectionRefusal> {
        Ok(WasmiFuelCosts {
            base: narrow_cost(self.base)?,
            entity: narrow_cost(self.entity)?,
            load: narrow_cost(self.load)?,
            store: narrow_cost(self.store)?,
            call: narrow_cost(self.call)?,
            branch_kept_per_fuel: narrow_cost(self.branch_kept_per_fuel)?,
            func_locals_per_fuel: narrow_cost(self.func_locals_per_fuel)?,
            memory_bytes_per_fuel: narrow_cost(self.memory_bytes_per_fuel)?,
            table_elements_per_fuel: narrow_cost(self.table_elements_per_fuel)?,
        })
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum InjectionRefusal {
    MalformedModule { reason: String },
    PrivateImportCollision,
    UnknownSchedule { version: u32 },
    FunctionCountOutOfRange { function_count: usize },
    UnsupportedInstruction,
    InvalidPrivateChargeSurface,
    ScheduleCoefficientOutOfRange,
    BranchAnalysis { reason: String },
    InvalidScheduleEncoding { byte_length: usize },
    ZeroScheduleCoefficient { index: u8 },
    ScheduleCoefficientMismatch { index: u8, expected: u64, actual: u64 },
    Serialization { reason: String },
}

impl Display for InjectionRefusal {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::MalformedModule { reason } => write!(f, "malformed module: {reason}"),
            Self::PrivateImportCollision => write!(f, "module imports the private meter namespace"),
            Self::UnknownSchedule { version } => write!(f, "unknown metering schedule {version}"),
            Self::FunctionCountOutOfRange { function_count } => write!(f, "function count {function_count} exceeds u32"),
            Self::UnsupportedInstruction => write!(f, "instruction is unsupported by the metering schedule"),
            Self::InvalidPrivateChargeSurface => write!(f, "instrumented private charge surface is not the frozen v1 signature"),
            Self::ScheduleCoefficientOutOfRange => write!(f, "metering schedule coefficient exceeds u32"),
            Self::BranchAnalysis { reason } => write!(f, "branch metering analysis failed: {reason}"),
            Self::InvalidScheduleEncoding { byte_length } => write!(f, "metering schedule record is {byte_length} bytes, expected 76"),
            Self::ZeroScheduleCoefficient { index } => write!(f, "metering schedule coefficient {index} is zero"),
            Self::ScheduleCoefficientMismatch { index, expected, actual } => write!(
                f,
                "metering schedule coefficient {index} is {actual}, expected frozen value {expected}",
            ),
            Self::Serialization { reason } => write!(f, "instrumented serialization failed: {reason}"),
        }
    }
}

fn copy_array<const N: usize>(bytes: &[u8]) -> Result<[u8; N], InjectionRefusal> {
    <[u8; N]>::try_from(bytes)
        .map_err(|_| InjectionRefusal::InvalidScheduleEncoding { byte_length: bytes.len() })
}

fn narrow_cost(value: u64) -> Result<u32, InjectionRefusal> {
    u32::try_from(value).map_err(|_| InjectionRefusal::ScheduleCoefficientOutOfRange)
}

fn analyze_branch_costs(
    wasm: &[u8], schedule: FuelSchedule,
) -> Result<Vec<BranchCost>, InjectionRefusal> {
    let mut validator = Validator::new();
    let mut allocations = FuncValidatorAllocations::default();
    let mut defined_function_index = 0_u32;
    let mut costs = Vec::new();
    for payload in Parser::new(0).parse_all(wasm) {
        let payload = payload.map_err(branch_error)?;
        if let ValidPayload::Func(to_validate, body) = validator.payload(&payload).map_err(branch_error)? {
            let mut function_validator = to_validate.into_validator(allocations);
            analyze_function_branches(
                &mut function_validator,
                &body,
                defined_function_index,
                schedule,
                &mut costs,
            )?;
            allocations = function_validator.into_allocations();
            defined_function_index = defined_function_index.checked_add(1)
                .ok_or(InjectionRefusal::FunctionCountOutOfRange { function_count: usize::MAX })?;
        }
    }
    Ok(costs)
}

fn analyze_function_branches(
    validator: &mut FuncValidator<ValidatorResources>,
    body: &FunctionBody<'_>,
    function_index: u32,
    schedule: FuelSchedule,
    costs: &mut Vec<BranchCost>,
) -> Result<(), InjectionRefusal> {
    let mut locals_reader = body.get_binary_reader();
    validator.read_locals(&mut locals_reader).map_err(branch_error)?;
    let mut operators = body.get_operators_reader().map_err(branch_error)?;
    let mut instruction_index = 0_u32;
    while !operators.eof() {
        let offset = operators.original_position();
        let operator = operators.read().map_err(branch_error)?;
        if let Some(cost) = positioned_cost(validator, &operator, schedule)? {
            costs.push(BranchCost { function_index, instruction_index, cost });
        }
        validator.op(offset, &operator).map_err(branch_error)?;
        instruction_index = instruction_index.checked_add(1)
            .ok_or(InjectionRefusal::FunctionCountOutOfRange { function_count: usize::MAX })?;
    }
    validator.finish(operators.original_position()).map_err(branch_error)
}

fn positioned_cost<R: WasmModuleResources>(
    validator: &FuncValidator<R>, operator: &Operator<'_>, schedule: FuelSchedule,
) -> Result<Option<u32>, InjectionRefusal> {
    if matches!(operator, Operator::Else) {
        let cost = if validator.get_control_frame(0).is_some_and(|frame| frame.unreachable) {
            0
        } else {
            narrow_cost(schedule.base)?
        };
        return Ok(Some(cost));
    }
    if validator.get_control_frame(0).is_some_and(|frame| frame.unreachable) {
        return Ok(None);
    }
    let total = match operator {
        Operator::Br { relative_depth } => {
            schedule.base.saturating_add(branch_drop_keep(validator, *relative_depth, 0, 0, schedule)?)
        }
        Operator::BrIf { relative_depth }
            if *relative_depth == validator.control_stack_height().saturating_sub(1) => 0,
        Operator::BrIf { relative_depth } => {
            schedule.base.saturating_add(branch_drop_keep(validator, *relative_depth, 0, 1, schedule)?)
        }
        Operator::BrTable { targets } => {
            let mut maximum = branch_drop_keep(validator, targets.default(), 0, 1, schedule)?;
            for target in targets.targets() {
                maximum = maximum.max(branch_drop_keep(
                    validator,
                    target.map_err(branch_error)?,
                    0,
                    1,
                    schedule,
                )?);
            }
            schedule.base.saturating_add(maximum)
        }
        Operator::Return => {
            let depth = validator.control_stack_height().saturating_sub(1);
            schedule.base.saturating_add(branch_drop_keep(
                validator,
                depth,
                validator.len_locals(),
                0,
                schedule,
            )?)
        }
        Operator::End if validator.control_stack_height() == 1 => {
            schedule.base.saturating_add(branch_drop_keep(
                validator,
                0,
                validator.len_locals(),
                0,
                schedule,
            )?)
        }
        _ => return Ok(None),
    };
    Ok(Some(u32::try_from(total).map_err(|_| InjectionRefusal::ScheduleCoefficientOutOfRange)?))
}

fn branch_drop_keep<R: WasmModuleResources>(
    validator: &FuncValidator<R>, depth: u32, locals: u32, operand_pop: u32,
    schedule: FuelSchedule,
) -> Result<u64, InjectionRefusal> {
    let frame = validator.get_control_frame(depth as usize).ok_or_else(|| InjectionRefusal::BranchAnalysis {
        reason: format!("missing control frame at depth {depth}"),
    })?;
    let keep = match frame.kind {
        FrameKind::Loop => block_arity(validator.resources(), frame.block_type, true),
        _ => block_arity(validator.resources(), frame.block_type, false),
    };
    let height = validator.operand_stack_height();
    let available = height.checked_sub(operand_pop)
        .and_then(|value| value.checked_sub(frame.height as u32))
        .ok_or_else(|| InjectionRefusal::BranchAnalysis {
        reason: format!("operand height {height} below frame height {}", frame.height),
    })?;
    let exit_locals = if depth == validator.control_stack_height().saturating_sub(1) {
        validator.len_locals()
    } else {
        locals
    };
    let drop = available.checked_sub(keep).and_then(|value| value.checked_add(exit_locals))
        .ok_or_else(|| InjectionRefusal::BranchAnalysis { reason: "invalid drop/keep shape".to_string() })?;
    if drop == 0 || schedule.branch_kept_per_fuel == 0 {
        Ok(0)
    } else {
        Ok(u64::from(keep) / schedule.branch_kept_per_fuel)
    }
}

fn block_arity<R: WasmModuleResources>(resources: &R, block: BlockType, params: bool) -> u32 {
    match block {
        BlockType::Empty => 0,
        BlockType::Type(_) => u32::from(!params),
        BlockType::FuncType(index) => resources.func_type_at(index).map_or(0, |ty| {
            if params { ty.len_inputs() as u32 } else { ty.len_outputs() as u32 }
        }),
    }
}

fn branch_error(error: wasmparser_nostd::BinaryReaderError) -> InjectionRefusal {
    InjectionRefusal::BranchAnalysis { reason: error.to_string() }
}

fn verify_private_charge_surface(module: &elements::Module) -> Result<(), InjectionRefusal> {
    use elements::{External, Type, ValueType};
    let imports = module.import_section().ok_or(InjectionRefusal::InvalidPrivateChargeSurface)?;
    let private = imports.entries().iter()
        .filter(|entry| entry.module() == PRIVATE_METER_MODULE)
        .collect::<Vec<_>>();
    if private.len() != 2 {
        return Err(InjectionRefusal::InvalidPrivateChargeSurface);
    }
    let types = module.type_section().ok_or(InjectionRefusal::InvalidPrivateChargeSurface)?;
    for expected_name in [PRIVATE_CHARGE_FUNCTION, PRIVATE_CHECK_FUNCTION] {
        let mut named = private.iter().filter(|entry| entry.field() == expected_name);
        let entry = named.next().ok_or(InjectionRefusal::InvalidPrivateChargeSurface)?;
        if named.next().is_some() {
            return Err(InjectionRefusal::InvalidPrivateChargeSurface);
        }
        let External::Function(type_index) = entry.external() else {
            return Err(InjectionRefusal::InvalidPrivateChargeSurface);
        };
        let Some(Type::Function(function_type)) = types.types().get(*type_index as usize) else {
            return Err(InjectionRefusal::InvalidPrivateChargeSurface);
        };
        if function_type.params() != [ValueType::I64] || !function_type.results().is_empty() {
            return Err(InjectionRefusal::InvalidPrivateChargeSurface);
        }
    }
    Ok(())
}

fn refuse_private_import_collision(module: &elements::Module) -> Result<(), InjectionRefusal> {
    let collides = module.import_section().is_some_and(|section| {
        section.entries().iter().any(|entry| entry.module() == PRIVATE_METER_MODULE)
    });
    if collides { Err(InjectionRefusal::PrivateImportCollision) } else { Ok(()) }
}

#[cfg(test)]
mod golden_vectors {
    use super::{analyze_branch_costs, BranchCost, FuelSchedule, MeterInjection};

    const EMPTY_FUNCTION_SOURCE: &[u8] = &[
        0x00, 0x61, 0x73, 0x6d, 0x01, 0x00, 0x00, 0x00,
        0x01, 0x04, 0x01, 0x60, 0x00, 0x00,
        0x03, 0x02, 0x01, 0x00,
        0x0a, 0x04, 0x01, 0x02, 0x00, 0x0b,
    ];

    // One empty function costs one function-entry base and one implicit-return base.
    // Both private imports are frozen even though this module has no dynamic operation.
    const EMPTY_FUNCTION_INSTRUMENTED: &[u8] = &[
        0x00, 0x61, 0x73, 0x6d, 0x01, 0x00, 0x00, 0x00,
        0x01, 0x0c, 0x03, 0x60, 0x00, 0x00, 0x60, 0x01, 0x7e, 0x00,
        0x60, 0x01, 0x7e, 0x00,
        0x02, 0x50, 0x02,
        0x1a, 0x6c, 0x61, 0x79, 0x65, 0x72, 0x78, 0x5f, 0x70, 0x72, 0x69,
        0x76, 0x61, 0x74, 0x65, 0x5f, 0x6d, 0x65, 0x74, 0x65, 0x72, 0x69,
        0x6e, 0x67, 0x2f, 0x76, 0x31,
        0x0a, 0x63, 0x68, 0x61, 0x72, 0x67, 0x65, 0x5f, 0x69, 0x36, 0x34,
        0x00, 0x01,
        0x1a, 0x6c, 0x61, 0x79, 0x65, 0x72, 0x78, 0x5f, 0x70, 0x72, 0x69,
        0x76, 0x61, 0x74, 0x65, 0x5f, 0x6d, 0x65, 0x74, 0x65, 0x72, 0x69,
        0x6e, 0x67, 0x2f, 0x76, 0x31,
        0x09, 0x63, 0x68, 0x65, 0x63, 0x6b, 0x5f, 0x69, 0x36, 0x34,
        0x00, 0x02,
        0x03, 0x02, 0x01, 0x00,
        0x0a, 0x08, 0x01, 0x06, 0x00, 0x42, 0x02, 0x10, 0x00, 0x0b,
    ];

    // Eight kept results plus the br_table selector. Popping the selector leaves drop=0,
    // therefore Wasmi charges base only rather than one kept-value unit.
    const BR_TABLE_SELECTOR_SOURCE: &[u8] = &[
        0x00, 0x61, 0x73, 0x6d, 0x01, 0x00, 0x00, 0x00,
        0x01, 0x0c, 0x01, 0x60, 0x00, 0x08,
        0x7f, 0x7f, 0x7f, 0x7f, 0x7f, 0x7f, 0x7f, 0x7f,
        0x03, 0x02, 0x01, 0x00,
        0x0a, 0x19, 0x01, 0x17, 0x00,
        0x41, 0x00, 0x41, 0x00, 0x41, 0x00, 0x41, 0x00,
        0x41, 0x00, 0x41, 0x00, 0x41, 0x00, 0x41, 0x00,
        0x41, 0x00, 0x0e, 0x00, 0x00, 0x0b,
    ];

    #[test]
    fn empty_function_has_frozen_executable_bytes() {
        let injection = MeterInjection::instrument(
            EMPTY_FUNCTION_SOURCE,
            FuelSchedule::WASMI_0_31_2,
        )
        .unwrap_or_else(|error| panic!("golden instrumentation refused: {error}"));
        assert_eq!(injection.instrumented_wasm(), EMPTY_FUNCTION_INSTRUMENTED);
    }

    #[test]
    fn protocol_schedule_has_frozen_big_endian_record() {
        let expected = [
            0, 0, 0, 1,
            0, 0, 0, 0, 0, 0, 0, 1, 0, 0, 0, 0, 0, 0, 0, 1,
            0, 0, 0, 0, 0, 0, 0, 1, 0, 0, 0, 0, 0, 0, 0, 1,
            0, 0, 0, 0, 0, 0, 0, 1, 0, 0, 0, 0, 0, 0, 0, 8,
            0, 0, 0, 0, 0, 0, 0, 8, 0, 0, 0, 0, 0, 0, 0, 64,
            0, 0, 0, 0, 0, 0, 0, 8,
        ];
        assert_eq!(FuelSchedule::WASMI_0_31_2.canonical_bytes(), expected);
        assert_eq!(FuelSchedule::from_protocol_bytes(&expected), Ok(FuelSchedule::WASMI_0_31_2));
    }

    #[test]
    fn version_one_refuses_a_nonzero_coefficient_change() {
        let mut altered = FuelSchedule::WASMI_0_31_2.canonical_bytes();
        altered[11] = 2;
        assert_eq!(
            FuelSchedule::from_protocol_bytes(&altered),
            Err(super::InjectionRefusal::ScheduleCoefficientMismatch {
                index: 0,
                expected: 1,
                actual: 2,
            }),
        );
    }

    #[test]
    fn br_table_selector_is_not_counted_as_a_dropped_kept_value() {
        let costs = analyze_branch_costs(BR_TABLE_SELECTOR_SOURCE, FuelSchedule::WASMI_0_31_2)
            .unwrap_or_else(|error| panic!("branch golden refused: {error}"));
        assert_eq!(costs, vec![BranchCost { function_index: 0, instruction_index: 9, cost: 1 }]);
    }
}
