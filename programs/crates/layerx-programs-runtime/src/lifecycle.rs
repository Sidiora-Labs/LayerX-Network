//! Atomic deployment and upgrade lifecycle over the deterministic runtime.

use core::fmt::{self, Display};
use std::collections::BTreeMap;

use sha2::{Digest, Sha256};

use crate::{
    admit_abi_upgrade, admit_abi_version, AbiVersionRefusal, ExecutionError, ExecutionRecord,
    Executor, ProgramId, ValidationRefusal, WasmEngine,
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

/// Lifecycle outcome used to append an accepted deployment to the canonical
/// journal. Call authorization is established separately from verified
/// journal evidence.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DeploymentReceipt {
    program: ProgramId,
    version: u32,
    old_code_hash: Option<CodeHash>,
    new_code_hash: CodeHash,
    migration: Option<ExecutionRecord>,
}

impl DeploymentReceipt {
    #[must_use]
    pub const fn program(&self) -> ProgramId {
        self.program
    }

    #[must_use]
    pub const fn version(&self) -> u32 {
        self.version
    }

    #[must_use]
    pub const fn old_code_hash(&self) -> Option<CodeHash> {
        self.old_code_hash
    }

    #[must_use]
    pub const fn new_code_hash(&self) -> CodeHash {
        self.new_code_hash
    }

    #[must_use]
    pub const fn migration(&self) -> Option<&ExecutionRecord> {
        self.migration.as_ref()
    }
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
    AbiVersion(AbiVersionRefusal),
    Validation(ValidationRefusal),
    Migration(ExecutionError),
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
            Self::AbiVersion(refusal) => Display::fmt(refusal, formatter),
            Self::Validation(refusal) => write!(formatter, "validation refusal: {refusal}"),
            Self::Migration(error) => write!(formatter, "migration refusal: {error}"),
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

/// Qualification and historical schedule-one lifecycle model. The production
/// C lifecycle resolves protocol metering state before crossing the Rust bridge.
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
        if matches!(
            activity.upgrade_policy,
            UpgradePolicy::Authority(authority) if authority == [0; 32]
        ) {
            return Err(LifecycleRefusal::InvalidAuthority);
        }
        Self::check_abi(activity.abi_version)?;
        if let Err(refusal) = self.engine.validate_versioned(activity.abi_version, &activity.wasm) {
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
        let current_abi = record.versions.last().ok_or(LifecycleRefusal::UnknownProgram)?.abi_version;
        admit_abi_upgrade(current_abi, activity.abi_version)
            .map_err(LifecycleRefusal::AbiVersion)?;
        match record.policy {
            UpgradePolicy::Immutable => return Err(LifecycleRefusal::Immutable),
            UpgradePolicy::Authority(expected) if expected != activity.authority => {
                return Err(LifecycleRefusal::InvalidAuthority);
            }
            UpgradePolicy::Authority(_) => {}
        }
        let validated = match self.engine.validate_versioned(activity.abi_version, &activity.wasm) {
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
        let migration_executor = self.executor.for_abi(activity.abi_version);
        let migration = match &activity.migration {
            Some(migration) => match migration_executor.execute(&validated, &migration.export, &[]) {
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

    #[must_use]
    pub fn diagnostics(&self) -> &[DiagnosticArtifact] {
        &self.diagnostics
    }

    fn check_abi(requested: u16) -> Result<(), LifecycleRefusal> {
        admit_abi_version(requested).map_err(LifecycleRefusal::AbiVersion)
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
