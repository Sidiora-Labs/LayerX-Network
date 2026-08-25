//! Atomic deployment and upgrade lifecycle over the deterministic runtime.

use core::fmt::{self, Display};
use std::collections::BTreeMap;

use sha2::{Digest, Sha256};

use crate::{
    ExecutionError, ExecutionRecord, Executor, ProgramId, ValidationRefusal, WasmEngine,
    ABI_VERSION,
};

/// Code digest authenticated by the programs activity envelope.
pub type CodeHash = [u8; 32];

/// Upgrade authority declared once at deployment. Absence means immutable.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum UpgradePolicy {
    #[default]
    Immutable,
    Authority([u8; 32]),
}

/// Ordinary permissionless deployment activity.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Deploy {
    pub program: ProgramId,
    pub code_hash: CodeHash,
    pub wasm: Vec<u8>,
    pub abi_version: u16,
    pub upgrade_policy: UpgradePolicy,
}

/// Ordinary authority-scoped upgrade activity.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Upgrade {
    pub program: ProgramId,
    pub authority: [u8; 32],
    pub code_hash: CodeHash,
    pub wasm: Vec<u8>,
    pub abi_version: u16,
    pub migration: Option<Migration>,
}

/// Declared migration hook executed before an upgrade becomes durable.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Migration {
    pub export: String,
}

/// One immutable code version in protocol state.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ProgramVersion {
    pub code_hash: CodeHash,
    pub wasm: Vec<u8>,
    pub abi_version: u16,
}

/// Receipt evidence making a successfully deployed version callable.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DeploymentReceipt {
    pub program: ProgramId,
    pub version: u32,
    pub old_code_hash: Option<CodeHash>,
    pub new_code_hash: CodeHash,
    pub migration: Option<ExecutionRecord>,
}

/// Rejected artifact retained outside executable program state for diagnosis.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DiagnosticArtifact {
    pub program: ProgramId,
    pub code_hash: CodeHash,
    pub wasm: Vec<u8>,
    pub refusal: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LifecycleRefusal {
    UnknownProgram,
    AlreadyDeployed,
    Immutable,
    InvalidAuthority,
    CodeHashMismatch {
        declared: CodeHash,
        computed: CodeHash,
    },
    IncompatibleAbi { requested: u16, supported: u16 },
    Validation(ValidationRefusal),
    Migration(ExecutionError),
    UnverifiedReceipt,
    VersionOverflow,
}

impl Display for LifecycleRefusal {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::UnknownProgram => write!(formatter, "unknown program"),
            Self::AlreadyDeployed => write!(formatter, "program already deployed"),
            Self::Immutable => write!(formatter, "program is immutable"),
            Self::InvalidAuthority => write!(formatter, "upgrade authority does not match"),
            Self::CodeHashMismatch { .. } => {
                formatter.write_str("declared program code hash does not match exact WASM bytes")
            }
            Self::IncompatibleAbi {
                requested,
                supported,
            } => write!(
                formatter,
                "ABI version {requested} is incompatible with supported version {supported}"
            ),
            Self::Validation(refusal) => write!(formatter, "validation refusal: {refusal}"),
            Self::Migration(error) => write!(formatter, "migration refusal: {error}"),
            Self::UnverifiedReceipt => write!(formatter, "deployment receipt is not verified"),
            Self::VersionOverflow => write!(formatter, "program version overflow"),
        }
    }
}

impl std::error::Error for LifecycleRefusal {}

#[derive(Debug)]
struct ProgramRecord {
    policy: UpgradePolicy,
    versions: Vec<ProgramVersion>,
}

/// Programs-module lifecycle state. Program changes are committed only after
/// validation and migration complete successfully.
#[derive(Debug)]
pub struct Lifecycle {
    engine: WasmEngine,
    executor: Executor,
    programs: BTreeMap<ProgramId, ProgramRecord>,
    diagnostics: Vec<DiagnosticArtifact>,
}

impl Lifecycle {
    #[must_use]
    pub fn new(engine: WasmEngine, executor: Executor) -> Self {
        Self {
            engine,
            executor,
            programs: BTreeMap::new(),
            diagnostics: Vec::new(),
        }
    }

    /// Constructs lifecycle state with the declared deterministic runtime.
    ///
    /// # Errors
    ///
    /// Returns an engine refusal when the declared stack limits are invalid.
    pub fn declared() -> Result<Self, crate::EngineRefusal> {
        Ok(Self::new(WasmEngine::declared()?, Executor::declared()))
    }

    /// Validates and atomically installs an ordinary deployment activity.
    ///
    /// # Errors
    ///
    /// Returns a typed lifecycle refusal without installing executable code.
    pub fn deploy(&mut self, activity: Deploy) -> Result<DeploymentReceipt, LifecycleRefusal> {
        if self.programs.contains_key(&activity.program) {
            return Err(LifecycleRefusal::AlreadyDeployed);
        }
        if matches!(activity.upgrade_policy, UpgradePolicy::Authority([0; 32])) {
            return Err(LifecycleRefusal::InvalidAuthority);
        }
        Self::check_abi(activity.abi_version)?;
        if let Err(refusal) = self.engine.validate(&activity.wasm) {
            self.retain(&activity, &refusal.to_string());
            return Err(LifecycleRefusal::Validation(refusal));
        }
        if let Err(refusal) = verify_code_hash(activity.code_hash, &activity.wasm) {
            self.retain(&activity, &refusal.to_string());
            return Err(refusal);
        }
        let version = ProgramVersion {
            code_hash: activity.code_hash,
            wasm: activity.wasm,
            abi_version: activity.abi_version,
        };
        self.programs.insert(
            activity.program,
            ProgramRecord {
                policy: activity.upgrade_policy,
                versions: vec![version],
            },
        );
        Ok(DeploymentReceipt {
            program: activity.program,
            version: 1,
            old_code_hash: None,
            new_code_hash: activity.code_hash,
            migration: None,
        })
    }

    /// Validates, migrates and atomically installs an upgrade activity.
    ///
    /// # Errors
    ///
    /// Returns a typed refusal without changing the current program version.
    pub fn upgrade(&mut self, activity: Upgrade) -> Result<DeploymentReceipt, LifecycleRefusal> {
        Self::check_abi(activity.abi_version)?;
        if activity.authority == [0; 32] {
            return Err(LifecycleRefusal::InvalidAuthority);
        }
        let record = self
            .programs
            .get(&activity.program)
            .ok_or(LifecycleRefusal::UnknownProgram)?;
        match record.policy {
            UpgradePolicy::Immutable => return Err(LifecycleRefusal::Immutable),
            UpgradePolicy::Authority(expected) if expected != activity.authority => {
                return Err(LifecycleRefusal::InvalidAuthority);
            }
            UpgradePolicy::Authority(_) => {}
        }
        let validated = match self.engine.validate(&activity.wasm) {
            Ok(module) => module,
            Err(refusal) => {
                self.retain_upgrade(&activity, &refusal.to_string());
                return Err(LifecycleRefusal::Validation(refusal));
            }
        };
        if let Err(refusal) = verify_code_hash(activity.code_hash, &activity.wasm) {
            self.retain_upgrade(&activity, &refusal.to_string());
            return Err(refusal);
        }
        let migration = match &activity.migration {
            Some(migration) => match self.executor.execute(&validated, &migration.export, &[]) {
                Ok(record) => Some(record),
                Err(error) => {
                    self.retain_upgrade(&activity, &error.to_string());
                    return Err(LifecycleRefusal::Migration(error));
                }
            },
            None => None,
        };
        let record = self
            .programs
            .get_mut(&activity.program)
            .ok_or(LifecycleRefusal::UnknownProgram)?;
        let old_code_hash = record
            .versions
            .last()
            .map(|version| version.code_hash)
            .ok_or(LifecycleRefusal::UnknownProgram)?;
        let version = u32::try_from(record.versions.len())
            .ok()
            .and_then(|value| value.checked_add(1))
            .ok_or(LifecycleRefusal::VersionOverflow)?;
        record.versions.push(ProgramVersion {
            code_hash: activity.code_hash,
            wasm: activity.wasm,
            abi_version: activity.abi_version,
        });
        Ok(DeploymentReceipt {
            program: activity.program,
            version,
            old_code_hash: Some(old_code_hash),
            new_code_hash: activity.code_hash,
            migration,
        })
    }

    /// Resolves only a version named by verified deployment evidence.
    ///
    /// # Errors
    ///
    /// Refuses unverified, unknown or mismatching receipt evidence.
    pub fn callable(
        &self,
        receipt: &DeploymentReceipt,
        receipt_verified: bool,
    ) -> Result<&ProgramVersion, LifecycleRefusal> {
        if !receipt_verified {
            return Err(LifecycleRefusal::UnverifiedReceipt);
        }
        let record = self
            .programs
            .get(&receipt.program)
            .ok_or(LifecycleRefusal::UnknownProgram)?;
        let version_index = receipt
            .version
            .checked_sub(1)
            .and_then(|value| usize::try_from(value).ok())
            .ok_or(LifecycleRefusal::UnknownProgram)?;
        let version = record
            .versions
            .get(version_index)
            .ok_or(LifecycleRefusal::UnknownProgram)?;
        if version.code_hash != receipt.new_code_hash {
            return Err(LifecycleRefusal::UnverifiedReceipt);
        }
        Ok(version)
    }

    #[must_use]
    pub fn diagnostics(&self) -> &[DiagnosticArtifact] {
        &self.diagnostics
    }

    fn check_abi(requested: u16) -> Result<(), LifecycleRefusal> {
        if requested == ABI_VERSION {
            Ok(())
        } else {
            Err(LifecycleRefusal::IncompatibleAbi {
                requested,
                supported: ABI_VERSION,
            })
        }
    }

    fn retain(&mut self, activity: &Deploy, refusal: &str) {
        self.diagnostics.push(DiagnosticArtifact {
            program: activity.program,
            code_hash: activity.code_hash,
            wasm: activity.wasm.clone(),
            refusal: refusal.to_string(),
        });
    }

    fn retain_upgrade(&mut self, activity: &Upgrade, refusal: &str) {
        self.diagnostics.push(DiagnosticArtifact {
            program: activity.program,
            code_hash: activity.code_hash,
            wasm: activity.wasm.clone(),
            refusal: refusal.to_string(),
        });
    }
}

fn verify_code_hash(declared: CodeHash, wasm: &[u8]) -> Result<(), LifecycleRefusal> {
    let computed: CodeHash = Sha256::digest(wasm).into();
    if declared != computed {
        return Err(LifecycleRefusal::CodeHashMismatch { declared, computed });
    }
    Ok(())
}
