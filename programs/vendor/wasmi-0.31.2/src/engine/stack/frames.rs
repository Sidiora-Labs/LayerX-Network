//! Data structures to represent the Wasm call stack during execution.

use super::{err_stack_overflow, DEFAULT_MAX_RECURSION_DEPTH};
use crate::{core::TrapCode, engine::{code_map::InstructionPtr, CompiledFunc}, Instance};
use alloc::vec::Vec;

/// A function frame of a function on the call stack.
#[derive(Debug, Clone)]
pub struct FuncFrame {
    /// The pointer to the currently executed instruction.
    ip: InstructionPtr,
    /// The instance in which the function has been defined.
    ///
    /// # Note
    ///
    /// The instance is used to inspect and manipulate with data that is
    /// non-local to the function such as linear memories, global variables
    /// and tables.
    instance: Instance,
    function: CompiledFunc,
    value_base: usize,
    operand_types: Vec<crate::execution_trace::ExecutionValueType>,
}

impl FuncFrame {
    /// Creates a new [`FuncFrame`].
    pub fn new(ip: InstructionPtr, instance: &Instance, function: CompiledFunc, value_base: usize) -> Self {
        Self {
            ip,
            instance: *instance,
            function,
            value_base,
            operand_types: Vec::new(),
        }
    }

    /// Returns the current instruction pointer.
    pub fn ip(&self) -> InstructionPtr {
        self.ip
    }

    /// Returns the instance of the [`FuncFrame`].
    pub fn instance(&self) -> &Instance {
        &self.instance
    }
    pub fn function(&self) -> CompiledFunc { self.function }
    pub fn value_base(&self) -> usize { self.value_base }
    pub(crate) fn operand_types(&self) -> &[crate::execution_trace::ExecutionValueType] { &self.operand_types }
    pub(crate) fn set_operand_types(&mut self, operand_types: Vec<crate::execution_trace::ExecutionValueType>) {
        self.operand_types = operand_types;
    }
}

/// The live function call stack storing the live function activation frames.
#[derive(Debug)]
pub struct CallStack {
    /// The call stack featuring the function frames in order.
    frames: Vec<FuncFrame>,
    /// The maximum allowed depth of the `frames` stack.
    recursion_limit: usize,
}

impl Default for CallStack {
    fn default() -> Self {
        Self::new(DEFAULT_MAX_RECURSION_DEPTH)
    }
}

impl CallStack {
    /// Creates a new [`CallStack`] using the given recursion limit.
    pub fn new(recursion_limit: usize) -> Self {
        Self {
            frames: Vec::new(),
            recursion_limit,
        }
    }

    /// Initializes the [`CallStack`] given the Wasm function.
    pub fn init(&mut self, ip: InstructionPtr, instance: &Instance, function: CompiledFunc, value_base: usize) {
        self.reset();
        self.frames.push(FuncFrame::new(ip, instance, function, value_base));
    }

    /// Pushes a Wasm caller function onto the [`CallStack`].
    #[inline]
    pub fn push(&mut self, caller: FuncFrame) -> Result<(), TrapCode> {
        if self.len() == self.recursion_limit {
            return Err(err_stack_overflow());
        }
        self.frames.push(caller);
        Ok(())
    }

    /// Pops the last [`FuncFrame`] from the [`CallStack`] if any.
    #[inline]
    pub fn pop(&mut self) -> Option<FuncFrame> {
        self.frames.pop()
    }

    /// Peeks the last [`FuncFrame`] from the [`CallStack`] if any.
    #[inline]
    pub fn peek(&self) -> Option<&FuncFrame> {
        self.frames.last()
    }
    pub(crate) fn peek_mut(&mut self) -> Option<&mut FuncFrame> { self.frames.last_mut() }
    pub(crate) fn frames(&self) -> &[FuncFrame] { &self.frames }

    /// Returns the amount of function frames on the [`CallStack`].
    #[inline]
    fn len(&self) -> usize {
        self.frames.len()
    }

    /// Clears the [`CallStack`] entirely.
    ///
    /// # Note
    ///
    /// This is required since sometimes execution can halt in the middle of
    /// function execution which leaves the [`CallStack`] in an unspecified
    /// state. Therefore the [`CallStack`] is required to be reset before
    /// function execution happens.
    pub fn reset(&mut self) {
        self.frames.clear();
    }
}
