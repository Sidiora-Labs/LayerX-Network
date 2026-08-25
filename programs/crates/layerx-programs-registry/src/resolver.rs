use core::fmt::{self, Display};
use std::cell::Cell;
use std::collections::BTreeMap;
use std::rc::Rc;

use layerx_programs_runtime::{
    AbiRevision, ActivityBudgetBinding, CompositionContext, CompositionRefusal, CompositionRules,
    EngineRefusal, ProgramId, ProgramResolver, ValidatedModule, ValidationRefusal, WasmEngine,
    ABI_VERSION,
};

use crate::{
    ProgramLifecycle, ReadFreshness, VerifiedDeploymentEvidence, VerifiedProgramHead,
};

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
    MissingCurrentHead { program: ProgramId },
    DuplicateCurrentHead { program: ProgramId },
    CurrentDeploymentMismatch { program: ProgramId },
    CurrentHeadMismatch,
    EvidenceExpired { program: ProgramId },
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
            Self::MissingCurrentHead { .. } => {
                formatter.write_str("an admitted program lacks current lifecycle evidence")
            }
            Self::DuplicateCurrentHead { .. } => {
                formatter.write_str("current lifecycle evidence duplicates a program")
            }
            Self::CurrentDeploymentMismatch { .. } => formatter
                .write_str("current Programs state differs from the admitted deployment"),
            Self::CurrentHeadMismatch => formatter
                .write_str("current program proofs do not share one receipt state head"),
            Self::EvidenceExpired { .. } => {
                formatter.write_str("current program evidence has expired")
            }
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
            | Self::RevisionMismatch { .. }
            | Self::MissingCurrentHead { .. }
            | Self::DuplicateCurrentHead { .. }
            | Self::CurrentDeploymentMismatch { .. }
            | Self::CurrentHeadMismatch
            | Self::EvidenceExpired { .. } => None,
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

/// Evidence-backed module staging catalog. It is intentionally not a
/// [`ProgramResolver`]: execution first consumes current lifecycle proofs for
/// every entry and binds the resulting snapshot to one authenticated activity.
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

    /// Consumes current state proofs and produces an affine, activity-bound
    /// resolver. A later lifecycle transition or expired head is refused
    /// before a runtime composition context exists.
    pub fn authorize_activity(
        self,
        heads: Vec<VerifiedProgramHead>,
        activity: ActivityBudgetBinding,
        now_ms: u64,
        rules: CompositionRules,
    ) -> Result<CompositionContext, ExecutableAdmissionError> {
        let mut current = BTreeMap::new();
        for head in heads {
            let program = head.program();
            if current.insert(program, head).is_some() {
                return Err(ExecutableAdmissionError::DuplicateCurrentHead { program });
            }
        }
        let mut common = None;
        for (program, admitted) in &self.programs {
            let head = current
                .remove(program)
                .ok_or(ExecutableAdmissionError::MissingCurrentHead { program: *program })?;
            if now_ms == 0
                || now_ms < head.freshness().observed_at
                || now_ms > head.valid_until_ms()
            {
                return Err(ExecutableAdmissionError::EvidenceExpired { program: *program });
            }
            if head.lifecycle() != ProgramLifecycle::Active {
                return Err(ExecutableAdmissionError::InactiveLifecycle {
                    lifecycle: head.lifecycle(),
                });
            }
            if head.version() != admitted.version
                || head.code_hash() != admitted.code_hash
                || head.abi_version() != admitted.abi_version
            {
                return Err(ExecutableAdmissionError::CurrentDeploymentMismatch {
                    program: *program,
                });
            }
            let identity = (
                head.receipt_digest(),
                head.state_root(),
                head.programs_root(),
                head.freshness(),
            );
            if common.is_some_and(|expected| expected != identity) {
                return Err(ExecutableAdmissionError::CurrentHeadMismatch);
            }
            common = Some(identity);
        }
        if let Some(program) = current.keys().next().copied() {
            return Err(ExecutableAdmissionError::CurrentDeploymentMismatch {
                program,
            });
        }
        let catalog = ActivityProgramCatalog {
            programs: self.programs,
            activity,
            consumed: Cell::new(false),
        };
        Ok(CompositionContext::new(Rc::new(catalog), rules))
    }
}

/// One current deployment snapshot that can authorize exactly one matching
/// budget-admitted activity.
#[derive(Debug)]
struct ActivityProgramCatalog {
    programs: BTreeMap<ProgramId, VerifiedProgram>,
    activity: ActivityBudgetBinding,
    consumed: Cell<bool>,
}

impl ProgramResolver for ActivityProgramCatalog {
    fn authorize_activity(
        &self,
        binding: Option<ActivityBudgetBinding>,
    ) -> Result<(), CompositionRefusal> {
        let Some(binding) = binding else {
            return Err(CompositionRefusal::ActivityEvidenceRequired);
        };
        if binding != self.activity {
            return Err(CompositionRefusal::ActivityEvidenceMismatch);
        }
        if self.consumed.replace(true) {
            return Err(CompositionRefusal::ActivityEvidenceReused);
        }
        Ok(())
    }

    fn program_module(&self, program: ProgramId) -> Option<&ValidatedModule> {
        self.programs.get(&program).map(|entry| &entry.module)
    }
}
