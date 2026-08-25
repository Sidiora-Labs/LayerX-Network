use core::fmt::{self, Display};
use std::collections::BTreeMap;
use std::rc::Rc;

use layerx_programs_runtime::{
    AbiRevision, CompositionContext, CompositionRules, EngineRefusal, ProgramId,
    ProgramResolver, ValidatedModule, ValidationRefusal, WasmEngine, ABI_VERSION,
};

use crate::{ProgramLifecycle, ReadFreshness, VerifiedDeploymentEvidence};

const CANDIDATE_ABI_VERSION: u16 = 2;

/// Typed refusal returned before any deployment enters the executable
/// resolver.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ExecutableAdmissionError {
    InactiveLifecycle { lifecycle: ProgramLifecycle },
    NonIncreasingVersion { current: u32, requested: u32 },
    FreshnessRegression {
        current: ReadFreshness,
        requested: ReadFreshness,
    },
    UnsupportedAbi { declared: u16 },
    RevisionMismatch {
        declared: u16,
        validated: AbiRevision,
    },
    Validation(ValidationRefusal),
}

impl Display for ExecutableAdmissionError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InactiveLifecycle { lifecycle } => {
                write!(formatter, "program lifecycle {lifecycle:?} is not executable")
            }
            Self::NonIncreasingVersion { current, requested } => write!(
                formatter,
                "deployment version {requested} does not advance admitted version {current}"
            ),
            Self::FreshnessRegression { .. } => {
                formatter.write_str("deployment evidence freshness regressed")
            }
            Self::UnsupportedAbi { declared } => {
                write!(formatter, "deployment ABI {declared} is not executable")
            }
            Self::RevisionMismatch {
                declared,
                validated,
            } => write!(
                formatter,
                "deployment ABI {declared} validated as unexpected revision {validated:?}"
            ),
            Self::Validation(refusal) => write!(formatter, "module validation refusal: {refusal}"),
        }
    }
}

impl std::error::Error for ExecutableAdmissionError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Validation(refusal) => Some(refusal),
            Self::InactiveLifecycle { .. }
            | Self::NonIncreasingVersion { .. }
            | Self::FreshnessRegression { .. }
            | Self::UnsupportedAbi { .. }
            | Self::RevisionMismatch { .. } => None,
        }
    }
}

#[derive(Debug)]
struct VerifiedProgram {
    version: u32,
    code_hash: [u8; 32],
    abi_version: u16,
    receipt_digest: [u8; 32],
    freshness: ReadFreshness,
    module: ValidatedModule,
}

/// Evidence-backed executable catalog. Its only admission path consumes an
/// opaque deployment proof and validates the proof-bound bytes under the ABI
/// recorded by that exact deployment.
#[derive(Debug)]
pub struct VerifiedProgramCatalog {
    engine: WasmEngine,
    programs: BTreeMap<ProgramId, VerifiedProgram>,
}

impl VerifiedProgramCatalog {
    /// Creates an empty catalog over the protocol-declared deterministic
    /// engine and validation limits.
    ///
    /// # Errors
    ///
    /// Refuses when the declared engine limits cannot be constructed.
    pub fn declared() -> Result<Self, EngineRefusal> {
        Ok(Self {
            engine: WasmEngine::declared()?,
            programs: BTreeMap::new(),
        })
    }

    /// Validates and admits one receipt-verified deployment.
    ///
    /// # Errors
    ///
    /// Refuses unsupported ABI declarations, modules rejected by the exact
    /// ABI validator, non-increasing versions and freshness regression.
    pub fn admit(
        &mut self,
        evidence: VerifiedDeploymentEvidence,
    ) -> Result<(), ExecutableAdmissionError> {
        if evidence.lifecycle() != ProgramLifecycle::Active {
            return Err(ExecutableAdmissionError::InactiveLifecycle {
                lifecycle: evidence.lifecycle(),
            });
        }
        if let Some(current) = self.programs.get(&evidence.program()) {
            if evidence.version() <= current.version {
                return Err(ExecutableAdmissionError::NonIncreasingVersion {
                    current: current.version,
                    requested: evidence.version(),
                });
            }
            if evidence.freshness().observed_sequence < current.freshness.observed_sequence
                || evidence.freshness().observed_at < current.freshness.observed_at
            {
                return Err(ExecutableAdmissionError::FreshnessRegression {
                    current: current.freshness,
                    requested: evidence.freshness(),
                });
            }
        }
        let expected_revision = match evidence.abi_version() {
            ABI_VERSION => AbiRevision::V1,
            CANDIDATE_ABI_VERSION => AbiRevision::CandidateV2,
            declared => return Err(ExecutableAdmissionError::UnsupportedAbi { declared }),
        };
        let module = match expected_revision {
            AbiRevision::V1 => self.engine.validate(evidence.module()),
            AbiRevision::CandidateV2 => self.engine.validate_candidate_v2(evidence.module()),
        }
        .map_err(ExecutableAdmissionError::Validation)?;
        if module.abi_revision() != expected_revision {
            return Err(ExecutableAdmissionError::RevisionMismatch {
                declared: evidence.abi_version(),
                validated: module.abi_revision(),
            });
        }
        self.programs.insert(
            evidence.program(),
            VerifiedProgram {
                version: evidence.version(),
                code_hash: evidence.code_hash(),
                abi_version: evidence.abi_version(),
                receipt_digest: evidence.receipt_digest(),
                freshness: evidence.freshness(),
                module,
            },
        );
        Ok(())
    }

    /// Returns the number of evidence-backed executable programs.
    #[must_use]
    pub fn len(&self) -> usize {
        self.programs.len()
    }

    /// Returns whether the catalog contains no executable deployment.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.programs.is_empty()
    }

    /// Returns whether a program has an evidence-backed executable version.
    #[must_use]
    pub fn contains(&self, program: ProgramId) -> bool {
        self.programs.contains_key(&program)
    }

    /// Returns the admitted deployment version for a program.
    #[must_use]
    pub fn version(&self, program: ProgramId) -> Option<u32> {
        self.programs.get(&program).map(|entry| entry.version)
    }

    /// Returns the exact admitted code hash for a program.
    #[must_use]
    pub fn code_hash(&self, program: ProgramId) -> Option<[u8; 32]> {
        self.programs.get(&program).map(|entry| entry.code_hash)
    }

    /// Returns the exact ABI under which a program was validated.
    #[must_use]
    pub fn abi_version(&self, program: ProgramId) -> Option<u16> {
        self.programs.get(&program).map(|entry| entry.abi_version)
    }

    /// Returns the canonical receipt digest that admitted a program.
    #[must_use]
    pub fn receipt_digest(&self, program: ProgramId) -> Option<[u8; 32]> {
        self.programs
            .get(&program)
            .map(|entry| entry.receipt_digest)
    }

    /// Returns the observed protocol freshness attached to an admission.
    #[must_use]
    pub fn freshness(&self, program: ProgramId) -> Option<ReadFreshness> {
        self.programs.get(&program).map(|entry| entry.freshness)
    }

    /// Consumes this verified resolver into the runtime composition surface.
    #[must_use]
    pub fn into_composition_context(self, rules: CompositionRules) -> CompositionContext {
        CompositionContext::new(Rc::new(self), rules)
    }
}

impl ProgramResolver for VerifiedProgramCatalog {
    fn program_module(&self, program: ProgramId) -> Option<&ValidatedModule> {
        self.programs.get(&program).map(|entry| &entry.module)
    }
}
