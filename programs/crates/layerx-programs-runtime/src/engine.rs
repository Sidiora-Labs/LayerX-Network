//! Construction of the vendored WASM engine stripped to the deterministic subset.

use core::fmt::{self, Display};

use wasmi::{Config, Engine, StackLimits};

use crate::limits::ValidationLimits;
use crate::validate::{self, AbiRevision, ValidatedModule, ValidationRefusal};

const INITIAL_VALUE_STACK_HEIGHT: u32 = 1_024;

/// A typed refusal produced while constructing a [`WasmEngine`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EngineRefusal {
    /// The declared stack limits were rejected by the engine.
    StackConfiguration {
        /// The engine's reason for rejecting the stack limits.
        reason: String,
    },
}

impl Display for EngineRefusal {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::StackConfiguration { reason } => {
                write!(f, "stack configuration refused: {reason}")
            }
        }
    }
}

impl std::error::Error for EngineRefusal {}

/// The vendored WASM engine configured for the deterministic subset only:
/// no clocks, no networking, no filesystem, no floats, no threads and no
/// randomness are reachable from guest code.
#[derive(Debug)]
pub struct WasmEngine {
    engine: Engine,
    limits: ValidationLimits,
}

impl WasmEngine {
    /// Constructs the deterministic engine under the given declared limits.
    ///
    /// # Errors
    ///
    /// Returns [`EngineRefusal::StackConfiguration`] when the engine rejects
    /// the declared stack limits.
    pub fn new(limits: ValidationLimits) -> Result<Self, EngineRefusal> {
        let initial_height = INITIAL_VALUE_STACK_HEIGHT.min(limits.max_value_stack_height());
        let stack_limits = StackLimits::new(
            initial_height as usize,
            limits.max_value_stack_height() as usize,
            limits.max_call_depth() as usize,
        )
        .map_err(|error| EngineRefusal::StackConfiguration {
            reason: error.to_string(),
        })?;
        let mut config = Config::default();
        config
            .set_stack_limits(stack_limits)
            .wasm_mutable_global(true)
            .wasm_sign_extension(true)
            .wasm_multi_value(true)
            .wasm_bulk_memory(true)
            .wasm_saturating_float_to_int(false)
            .wasm_reference_types(false)
            .wasm_tail_call(false)
            .wasm_extended_const(false)
            .consume_fuel(true)
            .floats(false);
        Ok(Self {
            engine: Engine::new(&config),
            limits,
        })
    }

    /// Constructs the deterministic engine under the declared default limits.
    ///
    /// # Errors
    ///
    /// Returns [`EngineRefusal::StackConfiguration`] when the engine rejects
    /// the declared stack limits.
    pub fn declared() -> Result<Self, EngineRefusal> {
        Self::new(ValidationLimits::declared())
    }

    /// Validates a module against the deterministic subset and declared limits.
    ///
    /// # Errors
    ///
    /// Returns a [`ValidationRefusal`] naming the violated rule when the module
    /// exceeds a declared limit, carries a forbidden import, uses floating
    /// point or vector types or instructions, or fails engine validation.
    pub fn validate(&self, wasm: &[u8]) -> Result<ValidatedModule, ValidationRefusal> {
        validate::validate_module(self, wasm, AbiRevision::V1)
    }

    /// Compatibility spelling for callers developed while v2 was a candidate.
    ///
    /// # Errors
    ///
    /// Returns the same deterministic validation refusals as [`Self::validate`].
    pub fn validate_candidate_v2(&self, wasm: &[u8]) -> Result<ValidatedModule, ValidationRefusal> {
        self.validate_v2(wasm)
    }

    /// Validates against the frozen version-two ABI.
    pub fn validate_v2(&self, wasm: &[u8]) -> Result<ValidatedModule, ValidationRefusal> {
        validate::validate_module(self, wasm, AbiRevision::V2)
    }

    /// Selects validation from the ABI version recorded with the deployment.
    pub fn validate_versioned(
        &self,
        abi_version: u16,
        wasm: &[u8],
    ) -> Result<ValidatedModule, ValidationRefusal> {
        match abi_version {
            crate::abi::manifest::ABI_V1_VERSION => self.validate(wasm),
            crate::abi::manifest::ABI_V2_VERSION => self.validate_v2(wasm),
            _ => Err(ValidationRefusal::UnsupportedAbiVersion { abi_version }),
        }
    }

    /// Returns the declared validation limits of this engine.
    #[must_use]
    pub const fn limits(&self) -> ValidationLimits {
        self.limits
    }

    pub(crate) const fn inner(&self) -> &Engine {
        &self.engine
    }
}
