//! This module is used to instrument a Wasm module with the gas metering code.
//!
//! The primary public interface is the [`inject`] function which transforms a given
//! module into one that charges gas for code to be executed. See function documentation for usage
//! and details.

mod backend;

pub use backend::{host_function, mutable_global, Backend, GasMeter};

#[cfg(test)]
mod validation;

use alloc::{vec, vec::Vec};
use core::{cmp::min, mem, num::NonZeroU32};
use parity_wasm::{
	builder,
	elements::{self, IndexMap, Instruction, ValueType},
};

/// An interface that describes instruction costs.
pub trait Rules {
	/// Returns the cost for the passed `instruction`.
	///
	/// Returning `None` makes the gas instrumention end with an error. This is meant
	/// as a way to have a partial rule set where any instruction that is not specifed
	/// is considered as forbidden.
	fn instruction_cost(&self, instruction: &Instruction) -> Option<u32>;

	/// Returns the cost for an instruction at its deterministic defined-function position.
	/// Implementations that attach validation-derived branch costs override this method.
	fn instruction_cost_at(
		&self,
		_function_index: u32,
		_instruction_index: u32,
		instruction: &Instruction,
	) -> Option<u32> {
		self.instruction_cost(instruction)
	}

	/// Returns the costs for growing the memory using the `memory.grow` instruction.
	///
	/// Please note that these costs are in addition to the costs specified by `instruction_cost`
	/// for the `memory.grow` instruction. Those are meant as dynamic costs which take the
	/// amount of pages that the memory is grown by into consideration. This is not possible
	/// using `instruction_cost` because those costs depend on the stack and must be injected as
	/// code into the function calling `memory.grow`. Therefore returning anything but
	/// [`MemoryGrowCost::Free`] introduces some overhead to the `memory.grow` instruction.
	fn memory_grow_cost(&self) -> MemoryGrowCost;

	/// A surcharge cost to calling a function that is added per local of that function.
	fn call_per_local_cost(&self) -> u32;

	/// Returns the total call-entry cost for the function's parameters and locals.
	fn call_locals_cost(&self, locals: u32) -> Option<u32> {
		self.call_per_local_cost().checked_mul(locals)
	}

	/// Number of successfully processed memory bytes charged per unit of fuel.
	fn memory_bytes_per_fuel(&self) -> Option<NonZeroU32> {
		None
	}

	/// Number of successfully processed table elements charged per unit of fuel.
	fn table_elements_per_fuel(&self) -> Option<NonZeroU32> {
		None
	}

	/// Cost seeded at each translated function body and reachable control-flow arm.
	fn block_entry_cost(&self) -> u32 {
		0
	}
}

/// Wasmi 0.31.2's default instruction cost coefficients.
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub struct WasmiFuelCosts {
	pub base: u32,
	pub entity: u32,
	pub load: u32,
	pub store: u32,
	pub call: u32,
	pub branch_kept_per_fuel: u32,
	pub func_locals_per_fuel: u32,
	pub memory_bytes_per_fuel: u32,
	pub table_elements_per_fuel: u32,
}

impl Default for WasmiFuelCosts {
	fn default() -> Self {
		Self {
			base: 1,
			entity: 1,
			load: 1,
			store: 1,
			call: 1,
			branch_kept_per_fuel: 8,
			func_locals_per_fuel: 8,
			memory_bytes_per_fuel: 64,
			table_elements_per_fuel: 8,
		}
	}
}

/// A validation-derived total cost override for a branch or return at a stable position.
#[derive(Debug, Copy, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct BranchCost {
	pub function_index: u32,
	pub instruction_index: u32,
	pub cost: u32,
}

/// Rules reproducing Wasmi 0.31.2's default cost categories.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WasmiParityRules {
	costs: WasmiFuelCosts,
	branch_costs: Vec<BranchCost>,
}

impl Default for WasmiParityRules {
	fn default() -> Self {
		Self { costs: WasmiFuelCosts::default(), branch_costs: Vec::new() }
	}
}

impl WasmiParityRules {
	pub fn new(costs: WasmiFuelCosts, mut branch_costs: Vec<BranchCost>) -> Self {
		branch_costs.sort_unstable();
		Self { costs, branch_costs }
	}

	pub fn costs(&self) -> WasmiFuelCosts {
		self.costs
	}

	fn branch_cost(&self, function_index: u32, instruction_index: u32) -> Option<u32> {
		self.branch_costs
			.binary_search_by_key(&(function_index, instruction_index), |entry| {
				(entry.function_index, entry.instruction_index)
			})
			.ok()
			.and_then(|index| self.branch_costs.get(index))
			.map(|entry| entry.cost)
	}
}

impl Rules for WasmiParityRules {
	fn instruction_cost(&self, instruction: &Instruction) -> Option<u32> {
		use parity_wasm::elements::Instruction::*;
		let cost = match instruction {
			Nop | Block(_) | Loop(_) | Else | End => 0,
			I32ReinterpretF32 | I64ReinterpretF64 | F32ReinterpretI32 | F64ReinterpretI64 => 0,
			Call(_) | CallIndirect(_, _) => self.costs.call,
			GetGlobal(_) | SetGlobal(_) | CurrentMemory(_) | GrowMemory(_) => self.costs.entity,
			I32Load(_, _) | I64Load(_, _) | F32Load(_, _) | F64Load(_, _) |
			I32Load8S(_, _) | I32Load8U(_, _) | I32Load16S(_, _) | I32Load16U(_, _) |
			I64Load8S(_, _) | I64Load8U(_, _) | I64Load16S(_, _) | I64Load16U(_, _) |
			I64Load32S(_, _) | I64Load32U(_, _) => self.costs.load,
			I32Store(_, _) | I64Store(_, _) | F32Store(_, _) | F64Store(_, _) |
			I32Store8(_, _) | I32Store16(_, _) | I64Store8(_, _) | I64Store16(_, _) |
			I64Store32(_, _) => self.costs.store,
			#[cfg(feature = "bulk")]
			Bulk(_) => self.costs.entity,
			#[cfg(feature = "reference_types")]
			TableGet(_) | TableSet(_) => self.costs.entity,
			_ => self.costs.base,
		};
		Some(cost)
	}

	fn instruction_cost_at(
		&self,
		function_index: u32,
		instruction_index: u32,
		instruction: &Instruction,
	) -> Option<u32> {
		self.branch_cost(function_index, instruction_index)
			.or_else(|| self.instruction_cost(instruction))
	}

	fn memory_grow_cost(&self) -> MemoryGrowCost {
		NonZeroU32::new(65_536 / self.costs.memory_bytes_per_fuel)
			.map_or(MemoryGrowCost::Free, MemoryGrowCost::Linear)
	}

	fn call_per_local_cost(&self) -> u32 {
		0
	}

	fn call_locals_cost(&self, locals: u32) -> Option<u32> {
		if self.costs.func_locals_per_fuel == 0 {
			Some(0)
		} else {
			Some(locals / self.costs.func_locals_per_fuel)
		}
	}

	fn memory_bytes_per_fuel(&self) -> Option<NonZeroU32> {
		NonZeroU32::new(self.costs.memory_bytes_per_fuel)
	}

	fn table_elements_per_fuel(&self) -> Option<NonZeroU32> {
		NonZeroU32::new(self.costs.table_elements_per_fuel)
	}

	fn block_entry_cost(&self) -> u32 {
		self.costs.base
	}
}

/// Dynamic costs for memory growth.
#[derive(Debug, PartialEq, Eq, Copy, Clone)]
pub enum MemoryGrowCost {
	/// Skip per page charge.
	///
	/// # Note
	///
	/// This makes sense when the amount of pages that a module is allowed to use is limited
	/// to a rather small number by static validation. In that case it is viable to
	/// benchmark the costs of `memory.grow` as the worst case (growing to to the maximum
	/// number of pages).
	Free,
	/// Charge the specified amount for each page that the memory is grown by.
	Linear(NonZeroU32),
}

#[cfg(feature = "bulk")]
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
enum DynamicHelper {
	MemoryGrow(u32, NonZeroU32),
	MemoryCopy(u32, u32, NonZeroU32),
	MemoryFill(u32, NonZeroU32),
	MemoryInit(u32, u32, NonZeroU32),
	TableCopy(u32, u32, NonZeroU32),
	TableInit(u32, u32, NonZeroU32),
	#[cfg(feature = "reference_types")]
	TableGrow(u32, ValueType, NonZeroU32),
	#[cfg(feature = "reference_types")]
	TableFill(u32, ValueType, NonZeroU32),
}

impl MemoryGrowCost {
	/// True iff memory growths code needs to be injected.
	fn enabled(&self) -> bool {
		match self {
			Self::Free => false,
			Self::Linear(_) => true,
		}
	}
}

/// A type that implements [`Rules`] so that every instruction costs the same.
///
/// This is a simplification that is mostly useful for development and testing.
///
/// # Note
///
/// In a production environment it usually makes no sense to assign every instruction
/// the same cost. A proper implemention of [`Rules`] should be provided that is probably
/// created by benchmarking.
pub struct ConstantCostRules {
	instruction_cost: u32,
	memory_grow_cost: u32,
	call_per_local_cost: u32,
}

impl ConstantCostRules {
	/// Create a new [`ConstantCostRules`].
	///
	/// Uses `instruction_cost` for every instruction and `memory_grow_cost` to dynamically
	/// meter the memory growth instruction.
	pub fn new(instruction_cost: u32, memory_grow_cost: u32, call_per_local_cost: u32) -> Self {
		Self { instruction_cost, memory_grow_cost, call_per_local_cost }
	}
}

impl Default for ConstantCostRules {
	/// Uses instruction cost of `1` and disables memory growth instrumentation.
	fn default() -> Self {
		Self { instruction_cost: 1, memory_grow_cost: 0, call_per_local_cost: 1 }
	}
}

impl Rules for ConstantCostRules {
	fn instruction_cost(&self, _: &Instruction) -> Option<u32> {
		Some(self.instruction_cost)
	}

	fn memory_grow_cost(&self) -> MemoryGrowCost {
		NonZeroU32::new(self.memory_grow_cost).map_or(MemoryGrowCost::Free, MemoryGrowCost::Linear)
	}

	fn call_per_local_cost(&self) -> u32 {
		self.call_per_local_cost
	}
}

/// Transforms a given module into one that tracks the gas charged during its execution.
///
/// The output module uses the `gas` function to track the gas spent. The function could be either
/// an imported or a local one modifying a mutable global. The argument is the amount of gas
/// required to continue execution. The execution engine is meant to keep track of the total amount
/// of gas used and trap or otherwise halt execution of the runtime if the gas usage exceeds some
/// allowed limit.
///
/// The body of each function of the original module is divided into metered blocks, and the calls
/// to charge gas are inserted at the beginning of every such block of code. A metered block is
/// defined so that, unless there is a trap, either all of the instructions are executed or none
/// are. These are similar to basic blocks in a control flow graph, except that in some cases
/// multiple basic blocks can be merged into a single metered block. This is the case if any path
/// through the control flow graph containing one basic block also contains another.
///
/// Charging gas at the beginning of each metered block ensures that 1) all instructions
/// executed are already paid for, 2) instructions that will not be executed are not charged for
/// unless execution traps, and 3) the number of calls to `gas` is minimized. The corollary is
/// that modules instrumented with this metering code may charge gas for instructions not
/// executed in the event of a trap.
///
/// Additionally, each `memory.grow` instruction found in the module is instrumented to first
/// make a call to charge gas for the additional pages requested. This cannot be done as part of
/// the block level gas charges as the gas cost is not static and depends on the stack argument
/// to `memory.grow`.
///
/// The above transformations are performed for every function body defined in the module. This
/// function also rewrites all function indices references by code, table elements, etc., since
/// the addition of an imported functions changes the indices of module-defined functions. If
/// the module has a `NameSection`, added by calling `parse_names`, the indices will also be
/// updated.
///
/// Syncronizing the amount of gas charged with the execution engine can be done in two ways. The
/// first way is by calling the imported `gas` host function, see [`host_function`] for details. The
/// second way is by using a local `gas` function together with a mutable global, see
/// [`mutable_global`] for details.
///
/// This routine runs in time linear in the size of the input module.
///
/// The function fails if the module contains any operation forbidden by gas rule set, returning
/// the original module as an `Err`.
pub fn inject<R: Rules, B: Backend>(
	module: elements::Module,
	backend: B,
	rules: &R,
) -> Result<elements::Module, elements::Module> {
	let module = match module.parse_names() {
		Ok(module) => module,
		Err((_, module)) => module,
	};
	let mut custom_anchors = Vec::new();
	let mut standard_count = 0usize;
	let mut retained = Vec::new();
	for section in module.into_sections() {
		let strip = match &section {
			elements::Section::Custom(custom) =>
				custom.name() == "linking" || custom.name().starts_with("reloc.") || custom.name() == "name",
			elements::Section::Reloc(_) => true,
			_ => false,
		};
		if strip { continue }
		if matches!(section, elements::Section::Custom(_) | elements::Section::Name(_)) {
			custom_anchors.push(standard_count);
			retained.push(section);
		} else {
			standard_count += 1;
			retained.push(section);
		}
	}
	let module = elements::Module::new(retained);
	let defined_param_counts: Vec<u32> = module
		.function_section()
		.into_iter()
		.flat_map(|section| section.entries())
		.map(|function| {
			module
				.type_section()
				.and_then(|section| section.types().get(function.type_ref() as usize))
				.map(|ty| match ty {
					elements::Type::Function(signature) => signature.params().len() as u32,
				})
				.unwrap_or(0)
		})
		.collect();
	#[cfg(feature = "reference_types")]
	let table_types: Vec<ValueType> = module
		.import_section()
		.into_iter()
		.flat_map(|section| section.entries())
		.filter_map(|entry| match entry.external() {
			elements::External::Table(table) => Some(table.elem_type()),
			_ => None,
		})
		.chain(module.table_section().into_iter().flat_map(|section| section.entries()).map(|table| table.elem_type()))
		.map(|ty| match ty {
			elements::TableElementType::AnyFunc => ValueType::FuncRef,
			elements::TableElementType::ExternRef => ValueType::ExternRef,
		})
		.collect();
	// Prepare module and return the gas function
	let gas_meter = backend.gas_meter(&module, rules);

	let import_count = module.import_count(elements::ImportCountType::Function) as u32;
	let functions_space = module.functions_space() as u32;
	let gas_global_idx = module.globals_space() as u32;

	let mut mbuilder = builder::from_module(module);

	// Calculate the indexes and gas function cost,
	// for external gas function the cost is counted on the host side
	let (gas_func_idx, total_func, gas_fn_cost, dynamic_check_idx, function_index_shift) = match gas_meter {
		GasMeter::External { module: gas_module, function, check } => {
			// Inject the import of the gas function
			let import_sig = mbuilder
				.push_signature(builder::signature().with_param(ValueType::I64).build_sig());
			mbuilder.push_import(
				builder::import()
					.module(gas_module)
					.field(function)
					.external()
					.func(import_sig)
					.build(),
			);
			let check_sig = mbuilder
				.push_signature(builder::signature().with_param(ValueType::I64).build_sig());
			mbuilder.push_import(
				builder::import()
					.module(gas_module)
					.field(check)
					.external()
					.func(check_sig)
					.build(),
			);

			(import_count, functions_space + 2, 0, Some(import_count + 1), 2)
		},
		GasMeter::Internal { global, ref func_instructions, cost } => {
			// Inject the gas counting global
			mbuilder.push_global(
				builder::global()
					.with_type(ValueType::I64)
					.mutable()
					.init_expr(Instruction::I64Const(0))
					.build(),
			);
			// Inject the export entry for the gas counting global
			let ebuilder = builder::ExportBuilder::new();
			let global_export = ebuilder
				.field(global)
				.with_internal(elements::Internal::Global(gas_global_idx))
				.build();
			mbuilder.push_export(global_export);

			let func_idx = functions_space;

			// Build local gas function
			let gas_func_sig =
				builder::SignatureBuilder::new().with_param(ValueType::I64).build_sig();

			let function = builder::FunctionBuilder::new()
				.with_signature(gas_func_sig)
				.body()
				.with_instructions(func_instructions.clone())
				.build()
				.build();

			// Inject local gas function
			mbuilder.push_function(function);

			(func_idx, func_idx + 1, cost, None, 0)
		},
	};

	// We need the built the module for making injections to its blocks
	let mut module = mbuilder.build();

	#[cfg(feature = "bulk")]
	let mut dynamic_helpers = Vec::<DynamicHelper>::new();
	let mut error = false;

	// Iterate over module sections and perform needed transformations.
	// Indexes are needed to be fixed up in `GasMeter::External` case, as it adds an imported
	// function, which goes to the beginning of the module's functions space.
	for section in module.sections_mut() {
		match section {
			elements::Section::Code(code_section) => {
				let injection_targets = match gas_meter {
					GasMeter::External { .. } => code_section.bodies_mut().as_mut_slice(),
					// Don't inject counters to the local gas function, which is the last one as
					// it's just added. Cost for its execution is added statically before each
					// invocation (see `inject_counter()`).
					GasMeter::Internal { .. } => {
						let len = code_section.bodies().len();
						&mut code_section.bodies_mut()[..len - 1]
					},
				};

				for (defined_function_index, func_body) in injection_targets.iter_mut().enumerate() {
					// Increment calling addresses if needed
					if let GasMeter::External { .. } = gas_meter {
						for instruction in func_body.code_mut().elements_mut().iter_mut() {
							match instruction {
								Instruction::Call(call_index) => if *call_index >= gas_func_idx {
									*call_index += function_index_shift
								},
								#[cfg(feature = "reference_types")]
								Instruction::RefFunc(function_index) => if *function_index >= gas_func_idx {
									*function_index += function_index_shift
								},
								_ => {},
							}
						}
					}
					let locals_count: u32 =
						func_body.locals().iter().map(|val_type| val_type.count()).sum();
					let locals_count = locals_count.saturating_add(
						defined_param_counts.get(defined_function_index).copied().unwrap_or(0),
					);
					if inject_counter(
						func_body.code_mut(),
						gas_fn_cost,
						locals_count,
						defined_function_index as u32,
						rules,
						gas_func_idx,
					)
					.is_err()
					{
						error = true;
						break
					}
					#[cfg(feature = "bulk")]
					inject_dynamic_counters(
						func_body.code_mut(),
						total_func,
						dynamic_check_idx,
						#[cfg(feature = "reference_types")]
						&table_types,
						rules,
						&mut dynamic_helpers,
					);
				}
			},
			elements::Section::Export(export_section) =>
			if let GasMeter::External { .. } = gas_meter {
					for export in export_section.entries_mut() {
						if let elements::Internal::Function(func_index) = export.internal_mut() {
							if *func_index >= gas_func_idx {
								*func_index += function_index_shift
							}
						}
					}
				},
				elements::Section::Element(elements_section) => {
				// Note that we do not need to check the element type referenced because in the
				// WebAssembly 1.0 spec, the only allowed element type is funcref.
				if let GasMeter::External { .. } = gas_meter {
					for segment in elements_section.entries_mut() {
						// update all indirect call addresses initial values
						for func_index in segment.members_mut() {
							if *func_index >= gas_func_idx {
								*func_index += function_index_shift
							}
						}
					}
				}
			},
			elements::Section::Start(start_idx) =>
				if let GasMeter::External { .. } = gas_meter {
					if *start_idx >= gas_func_idx {
					*start_idx += function_index_shift
					}
				},
				elements::Section::Global(global_section) => {
					if let GasMeter::External { .. } = gas_meter {
						for global in global_section.entries_mut() {
							for instruction in global.init_expr_mut().code_mut() {
								#[cfg(feature = "reference_types")]
								if let Instruction::RefFunc(function_index) = instruction {
									if *function_index >= gas_func_idx { *function_index += function_index_shift; }
								}
							}
						}
					}
				},
				elements::Section::Name(s) =>
				if let GasMeter::External { .. } = gas_meter {
					for functions in s.functions_mut() {
						*functions.names_mut() =
							IndexMap::from_iter(functions.names().iter().map(|(mut idx, name)| {
								if idx >= gas_func_idx {
									idx += function_index_shift;
								}

								(idx, name.clone())
							}));
					}
					for locals in s.locals_mut() {
						let updated = IndexMap::from_iter(
							locals.local_names().iter().map(|(mut idx, names)| {
								if idx >= gas_func_idx { idx += function_index_shift; }
								(idx, names.clone())
							}),
						);
						*locals.local_names_mut() = updated;
					}
				},
			_ => {},
		}
	}

	if error {
		return Err(module)
	}

	#[cfg(feature = "bulk")]
	let module = add_dynamic_helpers(module, gas_func_idx, dynamic_check_idx, &dynamic_helpers);
	let mut standards = Vec::new();
	let mut customs = Vec::new();
	for section in module.into_sections() {
		if matches!(section, elements::Section::Custom(_) | elements::Section::Name(_)) {
			customs.push(section);
		} else {
			standards.push(section);
		}
	}
	let mut restored = Vec::with_capacity(standards.len() + customs.len());
	let mut standards = standards.drain(..);
	let mut customs = customs.drain(..);
	let mut anchor_index = 0usize;
	for position in 0..=standard_count {
		while custom_anchors.get(anchor_index).map_or(false, |anchor| *anchor == position) {
			if let Some(section) = customs.next() { restored.push(section); }
			anchor_index += 1;
		}
		if let Some(section) = standards.next() { restored.push(section); }
	}
	restored.extend(standards);
	restored.extend(customs);
	Ok(elements::Module::new(restored))
}

/// A control flow block is opened with the `block`, `loop`, and `if` instructions and is closed
/// with `end`. Each block implicitly defines a new label. The control blocks form a stack during
/// program execution.
///
/// An example of block:
///
/// ```ignore
/// loop
///   i32.const 1
///   get_local 0
///   i32.sub
///   tee_local 0
///   br_if 0
/// end
/// ```
///
/// The start of the block is `i32.const 1`.
#[derive(Debug)]
struct ControlBlock {
	/// The lowest control stack index corresponding to a forward jump targeted by a br, br_if, or
	/// br_table instruction within this control block. The index must refer to a control block
	/// that is not a loop, meaning it is a forward jump. Given the way Wasm control flow is
	/// structured, the lowest index on the stack represents the furthest forward branch target.
	///
	/// This value will always be at most the index of the block itself, even if there is no
	/// explicit br instruction targeting this control block. This does not affect how the value is
	/// used in the metering algorithm.
	lowest_forward_br_target: usize,

	/// The active metering block that new instructions contribute a gas cost towards.
	active_metered_block: MeteredBlock,

	/// Whether the control block is a loop. Loops have the distinguishing feature that branches to
	/// them jump to the beginning of the block, not the end as with the other control blocks.
	is_loop: bool,
}

/// A block of code that metering instructions will be inserted at the beginning of. Metered blocks
/// are constructed with the property that, in the absence of any traps, either all instructions in
/// the block are executed or none are.
#[derive(Debug)]
struct MeteredBlock {
	/// Index of the first instruction (aka `Opcode`) in the block.
	start_pos: usize,
	/// Sum of costs of all instructions until end of the block.
	cost: u64,
}

/// Counter is used to manage state during the gas metering algorithm implemented by
/// `inject_counter`.
struct Counter {
	/// A stack of control blocks. This stack grows when new control blocks are opened with
	/// `block`, `loop`, and `if` and shrinks when control blocks are closed with `end`. The first
	/// block on the stack corresponds to the function body, not to any labelled block. Therefore
	/// the actual Wasm label index associated with each control block is 1 less than its position
	/// in this stack.
	stack: Vec<ControlBlock>,

	/// A list of metered blocks that have been finalized, meaning they will no longer change.
	finalized_blocks: Vec<MeteredBlock>,
}

impl Counter {
	fn new() -> Counter {
		Counter { stack: Vec::new(), finalized_blocks: Vec::new() }
	}

	/// Open a new control block. The cursor is the position of the first instruction in the block.
	fn begin_control_block(&mut self, cursor: usize, is_loop: bool) {
		let index = self.stack.len();
		self.stack.push(ControlBlock {
			lowest_forward_br_target: index,
			active_metered_block: MeteredBlock { start_pos: cursor, cost: 0 },
			is_loop,
		})
	}

	/// Close the last control block. The cursor is the position of the final (pseudo-)instruction
	/// in the block.
	fn finalize_control_block(&mut self, cursor: usize) -> Result<(), ()> {
		// This either finalizes the active metered block or merges its cost into the active
		// metered block in the previous control block on the stack.
		self.finalize_metered_block(cursor)?;

		// Pop the control block stack.
		let closing_control_block = self.stack.pop().ok_or(())?;
		let closing_control_index = self.stack.len();

		if self.stack.is_empty() {
			return Ok(())
		}

		// Update the lowest_forward_br_target for the control block now on top of the stack.
		{
			let control_block = self.stack.last_mut().ok_or(())?;
			control_block.lowest_forward_br_target = min(
				control_block.lowest_forward_br_target,
				closing_control_block.lowest_forward_br_target,
			);
		}

		// If there may have been a branch to a lower index, then also finalize the active metered
		// block for the previous control block. Otherwise, finalize it and begin a new one.
		let may_br_out = closing_control_block.lowest_forward_br_target < closing_control_index;
		if may_br_out {
			self.finalize_metered_block(cursor)?;
		}

		Ok(())
	}

	/// Finalize the current active metered block.
	///
	/// Finalized blocks have final cost which will not change later.
	fn finalize_metered_block(&mut self, cursor: usize) -> Result<(), ()> {
		let closing_metered_block = {
			let control_block = self.stack.last_mut().ok_or(())?;
			mem::replace(
				&mut control_block.active_metered_block,
				MeteredBlock { start_pos: cursor + 1, cost: 0 },
			)
		};

		// If the block was opened with a `block`, then its start position will be set to that of
		// the active metered block in the control block one higher on the stack. This is because
		// any instructions between a `block` and the first branch are part of the same basic block
		// as the preceding instruction. In this case, instead of finalizing the block, merge its
		// cost into the other active metered block to avoid injecting unnecessary instructions.
		let last_index = self.stack.len() - 1;
		if last_index > 0 {
			let prev_control_block = self
				.stack
				.get_mut(last_index - 1)
				.expect("last_index is greater than 0; last_index is stack size - 1; qed");
			let prev_metered_block = &mut prev_control_block.active_metered_block;
			if closing_metered_block.start_pos == prev_metered_block.start_pos {
				let cost = prev_metered_block.cost.checked_add(closing_metered_block.cost).ok_or(())?;
				if cost > u64::from(u32::MAX) { return Err(()) }
				prev_metered_block.cost = cost;
				return Ok(())
			}
		}

		if closing_metered_block.cost > 0 {
			self.finalized_blocks.push(closing_metered_block);
		}
		Ok(())
	}

	/// Handle a branch instruction in the program. The cursor is the index of the branch
	/// instruction in the program. The indices are the stack positions of the target control
	/// blocks. Recall that the index is 0 for a `return` and relatively indexed from the top of
	/// the stack by the label of `br`, `br_if`, and `br_table` instructions.
	fn branch(&mut self, cursor: usize, indices: &[usize]) -> Result<(), ()> {
		self.finalize_metered_block(cursor)?;
		self.record_branch_targets(indices)
	}

	fn record_branch_targets(&mut self, indices: &[usize]) -> Result<(), ()> {

		// Update the lowest_forward_br_target of the current control block.
		for &index in indices {
			let target_is_loop = {
				let target_block = self.stack.get(index).ok_or(())?;
				target_block.is_loop
			};
			if target_is_loop {
				continue
			}

			let control_block = self.stack.last_mut().ok_or(())?;
			control_block.lowest_forward_br_target =
				min(control_block.lowest_forward_br_target, index);
		}

		Ok(())
	}

	/// Returns the stack index of the active control block. Returns None if stack is empty.
	fn active_control_block_index(&self) -> Option<usize> {
		self.stack.len().checked_sub(1)
	}

	/// Get a reference to the currently active metered block.
	fn active_metered_block(&mut self) -> Result<&mut MeteredBlock, ()> {
		let top_block = self.stack.last_mut().ok_or(())?;
		Ok(&mut top_block.active_metered_block)
	}

	/// Increment the cost of the current block by the specified value.
	fn increment(&mut self, val: u32) -> Result<(), ()> {
		let top_block = self.active_metered_block()?;
		let cost = top_block.cost.checked_add(val.into()).ok_or(())?;
		if cost > u64::from(u32::MAX) { return Err(()) }
		top_block.cost = cost;
		Ok(())
	}
}

#[cfg(feature = "bulk")]
fn inject_dynamic_counters<R: Rules>(
	instructions: &mut elements::Instructions,
	first_helper: u32,
	dynamic_check: Option<u32>,
	#[cfg(feature = "reference_types")] table_types: &[ValueType],
	rules: &R,
	helpers: &mut Vec<DynamicHelper>,
) {
	if dynamic_check.is_none() { return }
	use parity_wasm::elements::{BulkInstruction, Instruction};
	for instruction in instructions.elements_mut() {
		let helper = match instruction {
			Instruction::GrowMemory(memory) => match rules.memory_grow_cost() {
				MemoryGrowCost::Linear(cost) => Some(DynamicHelper::MemoryGrow(*memory, cost)),
				MemoryGrowCost::Free => None,
			},
			Instruction::Bulk(BulkInstruction::MemoryCopy(dst, src)) => rules
				.memory_bytes_per_fuel()
				.map(|divisor| DynamicHelper::MemoryCopy(*dst, *src, divisor)),
			Instruction::Bulk(BulkInstruction::MemoryFill(memory)) => rules
				.memory_bytes_per_fuel()
				.map(|divisor| DynamicHelper::MemoryFill(*memory, divisor)),
			Instruction::Bulk(BulkInstruction::MemoryInit(segment, memory)) => rules
				.memory_bytes_per_fuel()
				.map(|divisor| DynamicHelper::MemoryInit(*segment, *memory, divisor)),
			Instruction::Bulk(BulkInstruction::TableCopy(dst, src)) => rules
				.table_elements_per_fuel()
				.map(|divisor| DynamicHelper::TableCopy(*dst, *src, divisor)),
			Instruction::Bulk(BulkInstruction::TableInit(segment, table)) => rules
				.table_elements_per_fuel()
				.map(|divisor| DynamicHelper::TableInit(*segment, *table, divisor)),
			#[cfg(feature = "reference_types")]
			Instruction::Bulk(BulkInstruction::TableGrow(table)) => rules
				.table_elements_per_fuel()
				.and_then(|divisor| table_types.get(*table as usize).copied().map(|ty| DynamicHelper::TableGrow(*table, ty, divisor))),
			#[cfg(feature = "reference_types")]
			Instruction::Bulk(BulkInstruction::TableFill(table)) => rules
				.table_elements_per_fuel()
				.and_then(|divisor| table_types.get(*table as usize).copied().map(|ty| DynamicHelper::TableFill(*table, ty, divisor))),
			_ => None,
		};
		if let Some(helper) = helper {
			let index = helpers.iter().position(|candidate| *candidate == helper).unwrap_or_else(|| {
				helpers.push(helper);
				helpers.len() - 1
			});
			*instruction = Instruction::Call(first_helper + index as u32);
		}
	}
}

#[cfg(feature = "bulk")]
fn add_dynamic_helpers(
	module: elements::Module,
	gas_func: u32,
	dynamic_check: Option<u32>,
	helpers: &[DynamicHelper],
) -> elements::Module {
	use parity_wasm::elements::{BlockType, BulkInstruction, Instruction::*, Local};
	let dynamic_check = match dynamic_check { Some(index) => index, None => return module };
	let mut builder = builder::from_module(module);
	for helper in helpers {
		let (params, mut body, length_local, divisor) = match *helper {
			DynamicHelper::MemoryGrow(memory, cost_per_page) => {
				let body = vec![
					GetLocal(0), I32Const(65_536), I32GtU,
					If(BlockType::Value(ValueType::I32)),
						I32Const(-1),
					Else,
						GetLocal(0), I64ExtendUI32, I64Const(i64::from(cost_per_page.get())), I64Mul,
						Call(dynamic_check),
						GetLocal(0), GrowMemory(memory), TeeLocal(1),
						I32Const(-1), I32Ne, If(BlockType::NoResult),
							GetLocal(0), I64ExtendUI32, I64Const(i64::from(cost_per_page.get())), I64Mul,
							Call(gas_func),
						End,
						GetLocal(1),
					End, End,
				];
				let signature = builder::SignatureBuilder::new()
					.with_param(ValueType::I32).with_result(ValueType::I32).build_sig();
				builder.push_function(builder::FunctionBuilder::new()
					.with_signature(signature).body()
					.with_locals(vec![Local::new(1, ValueType::I32)])
					.with_instructions(elements::Instructions::new(body)).build().build());
				continue
			},
			DynamicHelper::MemoryCopy(dst, src, divisor) => (
				vec![ValueType::I32, ValueType::I32, ValueType::I32],
				vec![GetLocal(0), GetLocal(1), GetLocal(2), Bulk(BulkInstruction::MemoryCopy(dst, src))], 2, divisor,
			),
			DynamicHelper::MemoryFill(memory, divisor) => (
				vec![ValueType::I32, ValueType::I32, ValueType::I32],
				vec![GetLocal(0), GetLocal(1), GetLocal(2), Bulk(BulkInstruction::MemoryFill(memory))], 2, divisor,
			),
			DynamicHelper::MemoryInit(segment, memory, divisor) => (
				vec![ValueType::I32, ValueType::I32, ValueType::I32],
				vec![GetLocal(0), GetLocal(1), GetLocal(2), Bulk(BulkInstruction::MemoryInit(segment, memory))], 2, divisor,
			),
			DynamicHelper::TableCopy(dst, src, divisor) => (
				vec![ValueType::I32, ValueType::I32, ValueType::I32],
				vec![GetLocal(0), GetLocal(1), GetLocal(2), Bulk(BulkInstruction::TableCopy(dst, src))], 2, divisor,
			),
			DynamicHelper::TableInit(segment, table, divisor) => (
				vec![ValueType::I32, ValueType::I32, ValueType::I32],
				vec![GetLocal(0), GetLocal(1), GetLocal(2), Bulk(BulkInstruction::TableInit(segment, table))], 2, divisor,
			),
			#[cfg(feature = "reference_types")]
			DynamicHelper::TableFill(table, element_type, divisor) => (
				vec![ValueType::I32, element_type, ValueType::I32],
				vec![GetLocal(0), GetLocal(1), GetLocal(2), Bulk(BulkInstruction::TableFill(table))], 2, divisor,
			),
			#[cfg(feature = "reference_types")]
			DynamicHelper::TableGrow(table, element_type, divisor) => {
				let mut body = vec![
					GetLocal(1), I64ExtendUI32, I64Const(i64::from(divisor.get())), I64DivU,
					Call(dynamic_check),
					GetLocal(0), GetLocal(1), Bulk(BulkInstruction::TableGrow(table)), TeeLocal(2),
					I32Const(-1), I32Ne, If(BlockType::NoResult),
					GetLocal(1), I64ExtendUI32, I64Const(i64::from(divisor.get())), I64DivU,
					Call(gas_func), End, GetLocal(2), End,
				];
				let signature = builder::SignatureBuilder::new()
					.with_params(vec![element_type, ValueType::I32])
					.with_result(ValueType::I32)
					.build_sig();
				builder.push_function(builder::FunctionBuilder::new()
					.with_signature(signature).body()
					.with_locals(vec![Local::new(1, ValueType::I32)])
					.with_instructions(elements::Instructions::new(body)).build().build());
				continue
			},
		};
		let mut wrapped = vec![
			GetLocal(length_local), I64ExtendUI32, I64Const(i64::from(divisor.get())), I64DivU,
			Call(dynamic_check),
		];
		wrapped.append(&mut body);
		wrapped.extend([
			GetLocal(length_local), I64ExtendUI32, I64Const(i64::from(divisor.get())), I64DivU,
			Call(gas_func), End,
		]);
		let signature = builder::SignatureBuilder::new().with_params(params);
		builder.push_function(builder::FunctionBuilder::new()
			.with_signature(signature.build_sig()).body()
			.with_instructions(elements::Instructions::new(wrapped)).build().build());
	}
	builder.build()
}

fn determine_metered_blocks<R: Rules>(
	instructions: &elements::Instructions,
	rules: &R,
	locals_count: u32,
	function_index: u32,
) -> Result<Vec<MeteredBlock>, ()> {
	use parity_wasm::elements::Instruction::*;

	let mut counter = Counter::new();
	#[derive(Copy, Clone)]
	struct Reachability {
		entry: bool,
		current: bool,
		end_branch: bool,
		is_loop: bool,
	}
	let mut reachability = vec![Reachability {
		entry: true,
		current: true,
		end_branch: false,
		is_loop: false,
	}];

	// Begin an implicit function (i.e. `func...end`) block.
	counter.begin_control_block(0, false);
	// Wasmi opens every reachable function body with one base unit.
	counter.increment(rules.block_entry_cost())?;
	// Add locals initialization cost to the function block.
	let locals_init_cost = rules.call_locals_cost(locals_count).ok_or(())?;
	counter.increment(locals_init_cost)?;

	for cursor in 0..instructions.elements().len() {
		let instruction = &instructions.elements()[cursor];
		let instruction_cost = rules
			.instruction_cost_at(function_index, cursor as u32, instruction)
			.ok_or(())?;
		match instruction {
			Block(_) => {
				let reachable = reachability.last().ok_or(())?.current;
				if reachable { counter.increment(instruction_cost)?; }

				// Begin new block. The cost of the following opcodes until `end` or `else` will
				// be included into this block. The start position is set to that of the previous
				// active metered block to signal that they should be merged in order to reduce
				// unnecessary metering instructions.
				let top_block_start_pos = counter.active_metered_block()?.start_pos;
				counter.begin_control_block(top_block_start_pos, false);
				reachability.push(Reachability { entry: reachable, current: reachable, end_branch: false, is_loop: false });
			},
			If(_) => {
				let reachable = reachability.last().ok_or(())?.current;
				if reachable { counter.increment(instruction_cost)?; }
				counter.begin_control_block(cursor + 1, false);
				if reachable { counter.increment(rules.block_entry_cost())?; }
				reachability.push(Reachability { entry: reachable, current: reachable, end_branch: reachable, is_loop: false });
			},
			Loop(_) => {
				let reachable = reachability.last().ok_or(())?.current;
				if reachable { counter.increment(instruction_cost)?; }
				counter.begin_control_block(cursor + 1, true);
				if reachable { counter.increment(rules.block_entry_cost())?; }
				reachability.push(Reachability { entry: reachable, current: reachable, end_branch: false, is_loop: true });
			},
			End => {
				let closing = reachability.pop().ok_or(())?;
				if closing.current { counter.increment(instruction_cost)?; }
				counter.finalize_control_block(cursor)?;
				if let Some(parent) = reachability.last_mut() {
					parent.current = closing.current || closing.end_branch;
				}
			},
			Else => {
				let frame = reachability.last_mut().ok_or(())?;
				let then_reachable = frame.current;
				frame.end_branch |= then_reachable;
				frame.current = frame.entry;
				counter.finalize_metered_block(cursor)?;
				if then_reachable { counter.increment(instruction_cost)?; }
				if frame.entry { counter.increment(rules.block_entry_cost())?; }
			},
			Br(label) => {
				let reachable = reachability.last().ok_or(())?.current;
				if reachable { counter.increment(instruction_cost)?; }

				// Label is a relative index into the control stack.
				let active_index = counter.active_control_block_index().ok_or(())?;
				let target_index = active_index.checked_sub(*label as usize).ok_or(())?;
				if reachable && !reachability.get(target_index).ok_or(())?.is_loop {
					reachability.get_mut(target_index).ok_or(())?.end_branch = true;
				}
				counter.branch(cursor, &[target_index])?;
				if reachable { reachability.last_mut().ok_or(())?.current = false; }
			},
			BrIf(label) => {
				let reachable = reachability.last().ok_or(())?.current;
				if reachable { counter.increment(instruction_cost)?; }
				let active_index = counter.active_control_block_index().ok_or(())?;
				let target_index = active_index.checked_sub(*label as usize).ok_or(())?;
				if reachable && !reachability.get(target_index).ok_or(())?.is_loop {
					reachability.get_mut(target_index).ok_or(())?.end_branch = true;
				}
			},
			BrTable(br_table_data) => {
				let reachable = reachability.last().ok_or(())?.current;
				if reachable { counter.increment(instruction_cost)?; }

				let active_index = counter.active_control_block_index().ok_or(())?;
				let target_indices = [br_table_data.default]
					.iter()
					.chain(br_table_data.table.iter())
					.map(|label| active_index.checked_sub(*label as usize))
					.collect::<Option<Vec<_>>>()
					.ok_or(())?;
				if reachable {
					for target in &target_indices {
						if !reachability.get(*target).ok_or(())?.is_loop {
							reachability.get_mut(*target).ok_or(())?.end_branch = true;
						}
					}
					reachability.last_mut().ok_or(())?.current = false;
				}
				counter.branch(cursor, &target_indices)?;
			},
			Return => {
				let reachable = reachability.last().ok_or(())?.current;
				if reachable { counter.increment(instruction_cost)?; }
				counter.branch(cursor, &[0])?;
				if reachable { reachability.last_mut().ok_or(())?.current = false; }
			},
			_ => {
				// An ordinal non control flow instruction increments the cost of the current block.
				let reachable = reachability.last().ok_or(())?.current;
				if reachable { counter.increment(instruction_cost)?; }
				if reachable && matches!(instruction, Unreachable) {
					reachability.last_mut().ok_or(())?.current = false;
				}
			},
		}
	}

	counter.finalized_blocks.sort_unstable_by_key(|block| block.start_pos);
	Ok(counter.finalized_blocks)
}

fn inject_counter<R: Rules>(
	instructions: &mut elements::Instructions,
	gas_function_cost: u64,
	locals_count: u32,
	function_index: u32,
	rules: &R,
	gas_func: u32,
) -> Result<(), ()> {
	let blocks = determine_metered_blocks(instructions, rules, locals_count, function_index)?;
	insert_metering_calls(instructions, gas_function_cost, blocks, gas_func)
}

// Then insert metering calls into a sequence of instructions given the block locations and costs.
fn insert_metering_calls(
	instructions: &mut elements::Instructions,
	gas_function_cost: u64,
	blocks: Vec<MeteredBlock>,
	gas_func: u32,
) -> Result<(), ()> {
	use parity_wasm::elements::Instruction::*;

	// To do this in linear time, construct a new vector of instructions, copying over old
	// instructions one by one and injecting new ones as required.
	let new_instrs_len = instructions.elements().len() + 2 * blocks.len();
	let original_instrs =
		mem::replace(instructions.elements_mut(), Vec::with_capacity(new_instrs_len));
	let new_instrs = instructions.elements_mut();

	let mut block_iter = blocks.into_iter().peekable();
	for (original_pos, instr) in original_instrs.into_iter().enumerate() {
		// If there the next block starts at this position, inject metering instructions.
		let used_block = if let Some(block) = block_iter.peek() {
			if block.start_pos == original_pos {
				let cost = block.cost.checked_add(gas_function_cost).ok_or(())?;
				let cost = i64::try_from(cost).map_err(|_| ())?;
				new_instrs.push(I64Const(cost));
				new_instrs.push(Call(gas_func));
				true
			} else {
				false
			}
		} else {
			false
		};

		if used_block {
			block_iter.next();
		}

		// Copy over the original instruction.
		new_instrs.push(instr);
	}

	if block_iter.next().is_some() {
		return Err(())
	}

	Ok(())
}

#[cfg(test)]
mod tests {
	use super::*;
	use parity_wasm::{builder, elements, elements::Instruction::*, serialize};
	use pretty_assertions::assert_eq;

	fn get_function_body(
		module: &elements::Module,
		index: usize,
	) -> Option<&[elements::Instruction]> {
		module
			.code_section()
			.and_then(|code_section| code_section.bodies().get(index))
			.map(|func_body| func_body.code().elements())
	}

	#[test]
	fn layerx_wasmi_loop_header_and_backedge_sequence_is_frozen() {
		let module = builder::module()
			.function()
			.signature()
			.build()
			.body()
			.with_instructions(elements::Instructions::new(vec![
				Loop(elements::BlockType::NoResult),
				I32Const(1),
				BrIf(0),
				I32Const(2),
				Drop,
				End,
				End,
			]))
			.build()
			.build()
			.build();
		let rules = WasmiParityRules::new(
			WasmiFuelCosts::default(),
			vec![
				BranchCost { function_index: 0, instruction_index: 2, cost: 1 },
				BranchCost { function_index: 0, instruction_index: 6, cost: 1 },
			],
		);
		let instrumented = inject(
			module,
			host_function::Injector::new("layerx_private_metering", "charge"),
			&rules,
		)
		.unwrap();
		assert_eq!(
			get_function_body(&instrumented, 0).unwrap(),
			&[
				I64Const(2), Call(0),
				Loop(elements::BlockType::NoResult),
				I64Const(5), Call(0), I32Const(1), BrIf(0), I32Const(2), Drop, End, End,
			],
		);
	}

	#[test]
	fn layerx_root_target_br_if_prepays_outer_continuation() {
		let module = builder::module()
			.function()
			.signature()
			.build()
			.body()
			.with_instructions(elements::Instructions::new(vec![
				Block(elements::BlockType::NoResult),
				I32Const(1),
				BrIf(1),
				End,
				I32Const(2),
				Drop,
				End,
			]))
			.build()
			.build()
			.build();
		let rules = WasmiParityRules::new(
			WasmiFuelCosts::default(),
			vec![
				BranchCost { function_index: 0, instruction_index: 2, cost: 0 },
				BranchCost { function_index: 0, instruction_index: 6, cost: 0 },
			],
		);
		let instrumented = inject(
			module,
			host_function::Injector::new("layerx_private_metering", "charge"),
			&rules,
		)
		.unwrap();
		assert_eq!(
			get_function_body(&instrumented, 0).unwrap(),
			&[
				I64Const(4), Call(0), Block(elements::BlockType::NoResult),
				I32Const(1), BrIf(1), End, I32Const(2), Drop, End,
			],
		);
	}

	#[test]
	fn layerx_memory_grow_lazy_helper_sequence_is_frozen() {
		let module = add_dynamic_helpers(
			elements::Module::new(Vec::new()),
			0,
			Some(1),
			&[DynamicHelper::MemoryGrow(3, NonZeroU32::new(1_024).unwrap())],
		);
		assert_eq!(
			get_function_body(&module, 0).unwrap(),
			&[
				GetLocal(0), I32Const(65_536), I32GtU,
				If(elements::BlockType::Value(ValueType::I32)), I32Const(-1), Else,
				GetLocal(0), I64ExtendUI32, I64Const(1_024), I64Mul, Call(1),
				GetLocal(0), GrowMemory(3), TeeLocal(1), I32Const(-1), I32Ne,
				If(elements::BlockType::NoResult), GetLocal(0), I64ExtendUI32,
				I64Const(1_024), I64Mul, Call(0), End, GetLocal(1), End, End,
			],
		);
	}

	#[test]
	fn layerx_bulk_memory_lazy_helper_sequence_is_frozen() {
		use parity_wasm::elements::BulkInstruction;
		let module = add_dynamic_helpers(
			elements::Module::new(Vec::new()),
			0,
			Some(1),
			&[DynamicHelper::MemoryCopy(2, 5, NonZeroU32::new(64).unwrap())],
		);
		assert_eq!(
			get_function_body(&module, 0).unwrap(),
			&[
				GetLocal(2), I64ExtendUI32, I64Const(64), I64DivU, Call(1),
				GetLocal(0), GetLocal(1), GetLocal(2), Bulk(BulkInstruction::MemoryCopy(2, 5)),
				GetLocal(2), I64ExtendUI32, I64Const(64), I64DivU, Call(0), End,
			],
		);
	}

	#[test]
	fn simple_grow_host_fn() {
		let module = parse_wat(
			r#"(module
			(func (result i32)
			  global.get 0
			  memory.grow)
			(global i32 (i32.const 42))
			(memory 0 1)
			)"#,
		);
		let backend = host_function::Injector::new("env", "gas");
		let injected_module =
			super::inject(module, backend, &ConstantCostRules::new(1, 10_000, 1)).unwrap();

		assert_eq!(
			get_function_body(&injected_module, 0).unwrap(),
			&vec![I64Const(2), Call(0), GetGlobal(0), Call(2), End][..]
		);
		assert_eq!(
			get_function_body(&injected_module, 1).unwrap(),
			&vec![
				GetLocal(0),
				GetLocal(0),
				I64ExtendUI32,
				I64Const(10000),
				I64Mul,
				Call(0),
				GrowMemory(0),
				End,
			][..]
		);

		let binary = serialize(injected_module).expect("serialization failed");
		wasmparser::validate(&binary).unwrap();
	}

	#[test]
	fn simple_grow_mut_global() {
		let module = parse_wat(
			r#"(module
			(func (result i32)
			  global.get 0
			  memory.grow)
			(global i32 (i32.const 42))
			(memory 0 1)
			)"#,
		);
		let backend = mutable_global::Injector::new("gas_left");
		let injected_module =
			super::inject(module, backend, &ConstantCostRules::new(1, 10_000, 1)).unwrap();

		assert_eq!(
			get_function_body(&injected_module, 0).unwrap(),
			&vec![I64Const(13), Call(1), GetGlobal(0), Call(2), End][..]
		);
		assert_eq!(
			get_function_body(&injected_module, 1).unwrap(),
			&vec![
				Instruction::GetGlobal(1),
				Instruction::GetLocal(0),
				Instruction::I64GeU,
				Instruction::If(elements::BlockType::NoResult),
				Instruction::GetGlobal(1),
				Instruction::GetLocal(0),
				Instruction::I64Sub,
				Instruction::SetGlobal(1),
				Instruction::Else,
				// sentinel val u64::MAX
				Instruction::I64Const(-1i64), // non-charged instruction
				Instruction::SetGlobal(1),    // non-charged instruction
				Instruction::Unreachable,     // non-charged instruction
				Instruction::End,
				Instruction::End,
			][..]
		);
		assert_eq!(
			get_function_body(&injected_module, 2).unwrap(),
			&vec![
				GetLocal(0),
				GetLocal(0),
				I64ExtendUI32,
				I64Const(10000),
				I64Mul,
				Call(1),
				GrowMemory(0),
				End,
			][..]
		);

		let binary = serialize(injected_module).expect("serialization failed");
		wasmparser::validate(&binary).unwrap();
	}

	#[test]
	fn grow_no_gas_no_track_host_fn() {
		let module = parse_wat(
			r"(module
			(func (result i32)
			  global.get 0
			  memory.grow)
			(global i32 (i32.const 42))
			(memory 0 1)
			)",
		);
		let backend = host_function::Injector::new("env", "gas");
		let injected_module =
			super::inject(module, backend, &ConstantCostRules::default()).unwrap();

		assert_eq!(
			get_function_body(&injected_module, 0).unwrap(),
			&vec![I64Const(2), Call(0), GetGlobal(0), GrowMemory(0), End][..]
		);

		assert_eq!(injected_module.functions_space(), 2);

		let binary = serialize(injected_module).expect("serialization failed");
		wasmparser::validate(&binary).unwrap();
	}

	#[test]
	fn grow_no_gas_no_track_mut_global() {
		let module = parse_wat(
			r"(module
			(func (result i32)
			  global.get 0
			  memory.grow)
			(global i32 (i32.const 42))
			(memory 0 1)
			)",
		);
		let backend = mutable_global::Injector::new("gas_left");
		let injected_module =
			super::inject(module, backend, &ConstantCostRules::default()).unwrap();

		assert_eq!(
			get_function_body(&injected_module, 0).unwrap(),
			&vec![I64Const(13), Call(1), GetGlobal(0), GrowMemory(0), End][..]
		);

		assert_eq!(injected_module.functions_space(), 2);

		let binary = serialize(injected_module).expect("serialization failed");
		wasmparser::validate(&binary).unwrap();
	}

	#[test]
	fn call_index_host_fn() {
		let module = builder::module()
			.global()
			.value_type()
			.i32()
			.build()
			.function()
			.signature()
			.param()
			.i32()
			.build()
			.body()
			.build()
			.build()
			.function()
			.signature()
			.param()
			.i32()
			.build()
			.body()
			.with_instructions(elements::Instructions::new(vec![
				Call(0),
				If(elements::BlockType::NoResult),
				Call(0),
				Call(0),
				Call(0),
				Else,
				Call(0),
				Call(0),
				End,
				Call(0),
				End,
			]))
			.build()
			.build()
			.build();

		let backend = host_function::Injector::new("env", "gas");
		let injected_module =
			super::inject(module, backend, &ConstantCostRules::default()).unwrap();

		assert_eq!(
			get_function_body(&injected_module, 1).unwrap(),
			&vec![
				I64Const(3),
				Call(0),
				Call(1),
				If(elements::BlockType::NoResult),
				I64Const(3),
				Call(0),
				Call(1),
				Call(1),
				Call(1),
				Else,
				I64Const(2),
				Call(0),
				Call(1),
				Call(1),
				End,
				Call(1),
				End
			][..]
		);
	}

	#[test]
	fn call_index_mut_global() {
		let module = builder::module()
			.global()
			.value_type()
			.i32()
			.build()
			.function()
			.signature()
			.param()
			.i32()
			.build()
			.body()
			.build()
			.build()
			.function()
			.signature()
			.param()
			.i32()
			.build()
			.body()
			.with_instructions(elements::Instructions::new(vec![
				Call(0),
				If(elements::BlockType::NoResult),
				Call(0),
				Call(0),
				Call(0),
				Else,
				Call(0),
				Call(0),
				End,
				Call(0),
				End,
			]))
			.build()
			.build()
			.build();

		let backend = mutable_global::Injector::new("gas_left");
		let injected_module =
			super::inject(module, backend, &ConstantCostRules::default()).unwrap();

		assert_eq!(
			get_function_body(&injected_module, 1).unwrap(),
			&vec![
				I64Const(14),
				Call(2),
				Call(0),
				If(elements::BlockType::NoResult),
				I64Const(14),
				Call(2),
				Call(0),
				Call(0),
				Call(0),
				Else,
				I64Const(13),
				Call(2),
				Call(0),
				Call(0),
				End,
				Call(0),
				End
			][..]
		);
	}

	fn parse_wat(source: &str) -> elements::Module {
		let module_bytes = wat::parse_str(source).unwrap();
		elements::deserialize_buffer(module_bytes.as_ref()).unwrap()
	}

	macro_rules! test_gas_counter_injection {
		(names = ($name1:ident, $name2:ident); input = $input:expr; expected = $expected:expr) => {
			#[test]
			fn $name1() {
				let input_module = parse_wat($input);
				let expected_module = parse_wat($expected);
				let injected_module = super::inject(
					input_module,
					host_function::Injector::new("env", "gas"),
					&ConstantCostRules::default(),
				)
				.expect("inject_gas_counter call failed");

				let actual_func_body = get_function_body(&injected_module, 0)
					.expect("injected module must have a function body");
				let expected_func_body = get_function_body(&expected_module, 0)
					.expect("post-module must have a function body");

				assert_eq!(actual_func_body, expected_func_body);
			}

			#[test]
			fn $name2() {
				let input_module = parse_wat($input);
				let draft_module = parse_wat($expected);
				let gas_fun_cost = match mutable_global::Injector::new("gas_left")
					.gas_meter(&input_module, &ConstantCostRules::default())
				{
					GasMeter::Internal { cost, .. } => cost as i64,
					_ => 0i64,
				};

				let injected_module = super::inject(
					input_module,
					mutable_global::Injector::new("gas_left"),
					&ConstantCostRules::default(),
				)
				.expect("inject_gas_counter call failed");

				let actual_func_body = get_function_body(&injected_module, 0)
					.expect("injected module must have a function body");
				let mut expected_func_body = get_function_body(&draft_module, 0)
					.expect("post-module must have a function body")
					.to_vec();

				// modify expected instructions set for gas_metering::mutable_global
				let mut iter = expected_func_body.iter_mut();
				while let Some(ins) = iter.next() {
					if let I64Const(cost) = ins {
						if let Some(ins_next) = iter.next() {
							if let Call(0) = ins_next {
								*cost += gas_fun_cost;
								*ins_next = Call(1);
							}
						}
					}
				}

				assert_eq!(actual_func_body, &expected_func_body);
			}
		};
	}

	test_gas_counter_injection! {
		names = (simple_host_fn, simple_mut_global);
		input = r#"
		(module
			(func (result i32)
				(get_global 0)))
		"#;
		expected = r#"
		(module
			(func (result i32)
				(call 0 (i64.const 1))
				(get_global 0)))
		"#
	}

	test_gas_counter_injection! {
		names = (nested_host_fn, nested_mut_global);
		input = r#"
		(module
			(func (result i32)
				(get_global 0)
				(block
					(get_global 0)
					(get_global 0)
					(get_global 0))
				(get_global 0)))
		"#;
		expected = r#"
		(module
			(func (result i32)
				(call 0 (i64.const 6))
				(get_global 0)
				(block
					(get_global 0)
					(get_global 0)
					(get_global 0))
				(get_global 0)))
		"#
	}

	test_gas_counter_injection! {
		names = (ifelse_host_fn, ifelse_mut_global);
		input = r#"
		(module
			(func (result i32)
				(get_global 0)
				(if
					(then
						(get_global 0)
						(get_global 0)
						(get_global 0))
					(else
						(get_global 0)
						(get_global 0)))
				(get_global 0)))
		"#;
		expected = r#"
		(module
			(func (result i32)
				(call 0 (i64.const 3))
				(get_global 0)
				(if
					(then
						(call 0 (i64.const 3))
						(get_global 0)
						(get_global 0)
						(get_global 0))
					(else
						(call 0 (i64.const 2))
						(get_global 0)
						(get_global 0)))
				(get_global 0)))
		"#
	}

	test_gas_counter_injection! {
		names = (branch_innermost_host_fn, branch_innermost_mut_global);
		input = r#"
		(module
			(func (result i32)
				(get_global 0)
				(block
					(get_global 0)
					(drop)
					(br 0)
					(get_global 0)
					(drop))
				(get_global 0)))
		"#;
		expected = r#"
		(module
			(func (result i32)
				(call 0 (i64.const 6))
				(get_global 0)
				(block
					(get_global 0)
					(drop)
					(br 0)
					(call 0 (i64.const 2))
					(get_global 0)
					(drop))
				(get_global 0)))
		"#
	}

	test_gas_counter_injection! {
		names = (branch_outer_block_host_fn, branch_outer_block_mut_global);
		input = r#"
		(module
			(func (result i32)
				(get_global 0)
				(block
					(get_global 0)
					(if
						(then
							(get_global 0)
							(get_global 0)
							(drop)
							(br_if 1)))
					(get_global 0)
					(drop))
				(get_global 0)))
		"#;
		expected = r#"
		(module
			(func (result i32)
				(call 0 (i64.const 5))
				(get_global 0)
				(block
					(get_global 0)
					(if
						(then
							(call 0 (i64.const 4))
							(get_global 0)
							(get_global 0)
							(drop)
							(br_if 1)))
					(call 0 (i64.const 2))
					(get_global 0)
					(drop))
				(get_global 0)))
		"#
	}

	test_gas_counter_injection! {
		names = (branch_outer_loop_host_fn, branch_outer_loop_mut_global);
		input = r#"
		(module
			(func (result i32)
				(get_global 0)
				(loop
					(get_global 0)
					(if
						(then
							(get_global 0)
							(br_if 0))
						(else
							(get_global 0)
							(get_global 0)
							(drop)
							(br_if 1)))
					(get_global 0)
					(drop))
				(get_global 0)))
		"#;
		expected = r#"
		(module
			(func (result i32)
				(call 0 (i64.const 3))
				(get_global 0)
				(loop
					(call 0 (i64.const 4))
					(get_global 0)
					(if
						(then
							(call 0 (i64.const 2))
							(get_global 0)
							(br_if 0))
						(else
							(call 0 (i64.const 4))
							(get_global 0)
							(get_global 0)
							(drop)
							(br_if 1)))
					(get_global 0)
					(drop))
				(get_global 0)))
		"#
	}

	test_gas_counter_injection! {
		names = (return_from_func_host_fn, return_from_func_mut_global);
		input = r#"
		(module
			(func (result i32)
				(get_global 0)
				(if
					(then
						(return)))
				(get_global 0)))
		"#;
		expected = r#"
		(module
			(func (result i32)
				(call 0 (i64.const 2))
				(get_global 0)
				(if
					(then
						(call 0 (i64.const 1))
						(return)))
				(call 0 (i64.const 1))
				(get_global 0)))
		"#
	}

	test_gas_counter_injection! {
		names = (branch_from_if_not_else_host_fn, branch_from_if_not_else_mut_global);
		input = r#"
		(module
			(func (result i32)
				(get_global 0)
				(block
					(get_global 0)
					(if
						(then (br 1))
						(else (br 0)))
					(get_global 0)
					(drop))
				(get_global 0)))
		"#;
		expected = r#"
		(module
			(func (result i32)
				(call 0 (i64.const 5))
				(get_global 0)
				(block
					(get_global 0)
					(if
						(then
							(call 0 (i64.const 1))
							(br 1))
						(else
							(call 0 (i64.const 1))
							(br 0)))
					(call 0 (i64.const 2))
					(get_global 0)
					(drop))
				(get_global 0)))
		"#
	}

	test_gas_counter_injection! {
		names = (empty_loop_host_fn, empty_loop_mut_global);
		input = r#"
		(module
			(func
				(loop
					(br 0)
				)
				unreachable
			)
		)
		"#;
		expected = r#"
		(module
			(func
				(call 0 (i64.const 2))
				(loop
					(call 0 (i64.const 1))
					(br 0)
				)
				unreachable
			)
		)
		"#
	}
}
