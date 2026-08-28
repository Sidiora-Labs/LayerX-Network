//! Construction of the vendored WASM engine stripped to the deterministic subset.

use core::fmt::{self, Display};
use std::sync::Arc;

use wasmi::{Config, Engine, StackLimits};

use crate::host::{self, HostLinker};
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
    /// The immutable versioned host surface could not be registered.
    HostLinkerConstruction {
        /// The engine's reason for refusing the host definition.
        reason: String,
    },
}

impl Display for EngineRefusal {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::StackConfiguration { reason } => {
                write!(f, "stack configuration refused: {reason}")
            }
            Self::HostLinkerConstruction { reason } => {
                write!(f, "versioned host linker construction refused: {reason}")
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
    linker: Arc<HostLinker>,
}

impl WasmEngine {
    /// Constructs the deterministic engine under the given declared limits.
    ///
    /// # Errors
    ///
    /// Returns [`EngineRefusal::StackConfiguration`] when the engine rejects
    /// the declared stack limits, or [`EngineRefusal::HostLinkerConstruction`]
    /// when an ABI host surface cannot be sealed.
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
            // Consensus CPU accounting is injected into the module and routed
            // through RuntimeState::Meter; engine fuel is not authoritative.
            .consume_fuel(false)
            .floats(false);
        let engine = Engine::new(&config);
        let linker = construct_host_linker(&engine)?;
        Ok(Self {
            engine,
            limits,
            linker,
        })
    }

    /// Constructs the deterministic engine under the declared default limits.
    ///
    /// # Errors
    ///
    /// Returns an [`EngineRefusal`] when the stack limits or an ABI host
    /// surface cannot be constructed.
    pub fn declared() -> Result<Self, EngineRefusal> {
        Self::new(ValidationLimits::declared())
    }

    /// Validates a legacy-v1/qualification module under historical metering schedule one.
    /// Consensus admission must use [`Self::validate_versioned_metered`] with
    /// the exact protocol-state schedule selected for the activity.
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

    /// Validates the frozen version-two ABI for qualification under historical
    /// metering schedule one. Consensus admission supplies its schedule explicitly.
    pub fn validate_v2(&self, wasm: &[u8]) -> Result<ValidatedModule, ValidationRefusal> {
        validate::validate_module(self, wasm, AbiRevision::V2)
    }

    /// Replays legacy schedule-one validation from the recorded ABI version.
    /// New protocol execution must use [`Self::validate_versioned_metered`].
    pub fn validate_versioned(
        &self,
        abi_version: u16,
        wasm: &[u8],
    ) -> Result<ValidatedModule, ValidationRefusal> {
        self.validate_versioned_metered(abi_version, wasm, crate::FuelSchedule::WASMI_0_31_2)
    }

    /// Validates and instruments under the exact protocol-resolved schedule.
    pub fn validate_versioned_metered(
        &self,
        abi_version: u16,
        wasm: &[u8],
        schedule: crate::FuelSchedule,
    ) -> Result<ValidatedModule, ValidationRefusal> {
        match abi_version {
            crate::abi::manifest::ABI_V1_VERSION => {
                validate::validate_module_metered(self, wasm, AbiRevision::V1, schedule)
            }
            crate::abi::manifest::ABI_V2_VERSION => {
                validate::validate_module_metered(self, wasm, AbiRevision::V2, schedule)
            }
            _ => Err(ValidationRefusal::UnsupportedAbiVersion { abi_version }),
        }
    }

    /// Returns the declared validation limits of this engine.
    #[must_use]
    pub const fn limits(&self) -> ValidationLimits {
        self.limits
    }

    /// Returns how many times this engine built its versioned host linker.
    #[must_use]
    pub fn host_linker_construction_count(&self) -> usize {
        self.linker.construction_count()
    }

    /// Returns the frozen number of host functions registered in the linker.
    #[must_use]
    pub fn host_function_registration_count(&self) -> usize {
        self.linker.registered_function_count()
    }

    pub(crate) const fn inner(&self) -> &Engine {
        &self.engine
    }

    pub(crate) fn host_linker(&self) -> Arc<HostLinker> {
        Arc::clone(&self.linker)
    }
}

fn construct_host_linker(engine: &Engine) -> Result<Arc<HostLinker>, EngineRefusal> {
    host::linker(engine)
        .map(Arc::new)
        .map_err(|error| EngineRefusal::HostLinkerConstruction {
            reason: error.to_string(),
        })
}
