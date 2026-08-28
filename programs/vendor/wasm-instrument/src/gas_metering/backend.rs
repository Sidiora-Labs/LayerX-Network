//! Provides backends for the gas metering instrumentation
use parity_wasm::elements;

/// Implementation details of the specific method of the gas metering.
#[derive(Clone)]
pub enum GasMeter {
	/// Gas metering with an external function.
	External {
		/// Name of the module to import the gas function from.
		module: &'static str,
		/// Name of the external gas function to be imported.
		function: &'static str,
		/// Private pre-operation fuel sufficiency hook.
		check: &'static str,
	},
	/// Gas metering with a local function and a mutable global.
	Internal {
		/// Name of the mutable global to be exported.
		global: &'static str,
		/// Body of the local gas counting function to be injected.
		func_instructions: elements::Instructions,
		/// Cost of the gas function execution.
		cost: u64,
	},
}

use super::Rules;
/// Under the hood part of the gas metering mechanics.
pub trait Backend {
	/// Provides the gas metering implementation details.
	fn gas_meter<R: Rules>(self, module: &elements::Module, rules: &R) -> GasMeter;
}

/// Gas metering with an external host function.
pub mod host_function {
	use super::{Backend, GasMeter, Rules};
	use parity_wasm::elements::Module;
	/// Injects invocations of the gas charging host function into each metering block.
	pub struct Injector {
		/// The name of the module to import the gas function from.
		module: &'static str,
		/// The name of the gas function to import.
		name: &'static str,
		check: &'static str,
	}

	impl Injector {
		pub fn new(module: &'static str, name: &'static str) -> Self {
			Self { module, name, check: "check" }
		}

		/// Overrides the runtime-private dynamic metering hook names.
		pub fn with_dynamic_check(mut self, check: &'static str) -> Self {
			self.check = check;
			self
		}
	}

	impl Backend for Injector {
		fn gas_meter<R: Rules>(self, _module: &Module, _rules: &R) -> GasMeter {
			GasMeter::External {
				module: self.module,
				function: self.name,
				check: self.check,
			}
		}
	}
}

/// Gas metering with a mutable global.
///
/// # Note
///
/// Not for all execution engines this method gives performance wins compared to using an [external
/// host function](host_function). See benchmarks and size overhead tests for examples of how to
/// make measurements needed to decide which gas metering method is better for your particular case.
///
/// # Warning
///
/// It is not recommended to apply [stack limiter](crate::inject_stack_limiter) instrumentation to a
/// module instrumented with this type of gas metering. This could lead to a massive module size
/// bloat. This is a known issue to be fixed in upcoming versions.
pub mod mutable_global {
	use super::{Backend, GasMeter, Rules};
	use alloc::vec;
	use parity_wasm::elements::{self, Instruction, Module};
	/// Injects a mutable global variable and a local function to the module to track
	/// current gas left.
	///
	/// The function is called in every metering block. In case of falling out of gas, the global is
	/// set to the sentinel value `U64::MAX` and `unreachable` instruction is called. The execution
	/// engine should take care of getting the current global value and setting it back in order to
	/// sync the gas left value during an execution.
	pub struct Injector {
		/// The export name of the gas tracking global.
		pub global_name: &'static str,
	}

	impl Injector {
		pub fn new(global_name: &'static str) -> Self {
			Self { global_name }
		}
	}

	impl Backend for Injector {
		fn gas_meter<R: Rules>(self, module: &Module, rules: &R) -> GasMeter {
			let gas_global_idx = module.globals_space() as u32;

			let func_instructions = vec![
				Instruction::GetGlobal(gas_global_idx),
				Instruction::GetLocal(0),
				Instruction::I64GeU,
				Instruction::If(elements::BlockType::NoResult),
				Instruction::GetGlobal(gas_global_idx),
				Instruction::GetLocal(0),
				Instruction::I64Sub,
				Instruction::SetGlobal(gas_global_idx),
				Instruction::Else,
				// sentinel val u64::MAX
				Instruction::I64Const(-1i64),           // non-charged instruction
				Instruction::SetGlobal(gas_global_idx), // non-charged instruction
				Instruction::Unreachable,               // non-charged instruction
				Instruction::End,
				Instruction::End,
			];

			// calculate gas used for the gas charging func execution itself
			let mut gas_fn_cost = func_instructions.iter().fold(0, |cost, instruction| {
				cost + (rules.instruction_cost(instruction).unwrap_or(0) as u64)
			});
			// don't charge for the instructions used to fail when out of gas
			let fail_cost = vec![
				Instruction::I64Const(-1i64),           // non-charged instruction
				Instruction::SetGlobal(gas_global_idx), // non-charged instruction
				Instruction::Unreachable,               // non-charged instruction
			]
			.iter()
			.fold(0, |cost, instruction| {
				cost + (rules.instruction_cost(instruction).unwrap_or(0) as u64)
			});

			gas_fn_cost -= fail_cost;

			GasMeter::Internal {
				global: self.global_name,
				func_instructions: elements::Instructions::new(func_instructions),
				cost: gas_fn_cost,
			}
		}
	}
}
