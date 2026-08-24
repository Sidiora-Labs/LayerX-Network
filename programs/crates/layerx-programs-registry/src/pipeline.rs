//! Reproducible-build pipeline mapping published source to the on-chain code
//! hash. Nothing here trusts a submitted artifact: the published archive is
//! rebuilt in a declared, pinned toolchain environment and only the digest of
//! the rebuilt module is compared with registered protocol state.

use core::fmt::{self, Display};
use std::collections::BTreeMap;

use layerx_programs_runtime::WasmEngine;

use crate::archive::{validate_path, ArchiveError, SourceArchive};
use crate::hash::sha256;
use crate::hex;
use crate::{BuildEnvironment, PublishedSource, RegistryError, ReproducibleBuild};

const PLAN_VERSION: &str = "1";
const PLAN_KEYS: [&str; 9] = [
    "artifact_path",
    "builder_image_digest",
    "command",
    "dependency_lock",
    "dependency_lock_digest",
    "source_date_epoch",
    "toolchain_digest",
    "toolchain_manifest",
    "version",
];
const MAX_PLAN_BYTES: usize = 8 * 1024;
const MIN_ATTEMPTS: u32 = 2;
const MAX_ATTEMPTS: u32 = 8;
const MAX_ARTIFACT_BYTES: usize = 32 * 1024 * 1024;

/// Operator-declared build recipe published beside the program source. The
/// recipe is protocol evidence, not caller input: it names the pinned builder
/// image, the pinned toolchain and lock files inside the archive, the exact
/// build command and the artifact the build is expected to produce.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct BuildPlan {
    pub environment: BuildEnvironment,
    pub artifact_path: String,
    pub toolchain_manifest: String,
    pub dependency_lock: String,
}

impl BuildPlan {
    /// Parses the declared build recipe.
    ///
    /// # Errors
    ///
    /// Refuses oversized documents, malformed lines, unknown keys, repeated
    /// keys, missing keys and unpinned environments.
    pub fn parse(text: &str) -> Result<Self, BuildRefusal> {
        if text.len() > MAX_PLAN_BYTES {
            return Err(BuildRefusal::InvalidPlan);
        }
        let mut fields = BTreeMap::new();
        for line in text.lines() {
            let trimmed = line.trim();
            if trimmed.is_empty() || trimmed.starts_with('#') {
                continue;
            }
            let (key, value) = trimmed.split_once('=').ok_or(BuildRefusal::InvalidPlan)?;
            if fields.insert(key.trim(), value.trim()).is_some() {
                return Err(BuildRefusal::InvalidPlan);
            }
        }
        for key in fields.keys() {
            if !PLAN_KEYS.contains(key) {
                return Err(BuildRefusal::InvalidPlan);
            }
        }
        if field(&fields, "version")? != PLAN_VERSION {
            return Err(BuildRefusal::InvalidPlan);
        }
        let plan = Self {
            environment: BuildEnvironment {
                builder_image_digest: digest(&fields, "builder_image_digest")?,
                toolchain_digest: digest(&fields, "toolchain_digest")?,
                dependency_lock_digest: digest(&fields, "dependency_lock_digest")?,
                source_date_epoch: field(&fields, "source_date_epoch")?
                    .parse()
                    .map_err(|_| BuildRefusal::InvalidPlan)?,
                command: field(&fields, "command")?
                    .split_whitespace()
                    .map(str::to_owned)
                    .collect(),
            },
            artifact_path: field(&fields, "artifact_path")?.to_owned(),
            toolchain_manifest: field(&fields, "toolchain_manifest")?.to_owned(),
            dependency_lock: field(&fields, "dependency_lock")?.to_owned(),
        };
        plan.validate()?;
        Ok(plan)
    }

    /// Encodes the declared build recipe in its canonical published form.
    #[must_use]
    pub fn encode(&self) -> String {
        format!(
            "version = {PLAN_VERSION}\nbuilder_image_digest = {}\ntoolchain_digest = {}\ndependency_lock_digest = {}\nsource_date_epoch = {}\nartifact_path = {}\ntoolchain_manifest = {}\ndependency_lock = {}\ncommand = {}\n",
            hex::encode(&self.environment.builder_image_digest),
            hex::encode(&self.environment.toolchain_digest),
            hex::encode(&self.environment.dependency_lock_digest),
            self.environment.source_date_epoch,
            self.artifact_path,
            self.toolchain_manifest,
            self.dependency_lock,
            self.environment.command.join(" "),
        )
    }

    fn validate(&self) -> Result<(), BuildRefusal> {
        self.environment.validate()?;
        validate_path(&self.artifact_path)?;
        validate_path(&self.toolchain_manifest)?;
        validate_path(&self.dependency_lock)?;
        Ok(())
    }
}

/// One hermetic build attempt handed to the sandbox that executes the pinned
/// command outside the protocol plane.
pub struct BuildAttempt<'inputs> {
    pub archive: &'inputs SourceArchive,
    pub plan: &'inputs BuildPlan,
    pub attempt: u32,
}

/// Sandbox boundary that materialises the published archive, executes the
/// declared pinned command and returns the produced artifact bytes.
pub trait BuildRunner {
    /// Runs one hermetic attempt and returns the produced artifact.
    ///
    /// # Errors
    ///
    /// Returns a typed refusal when the sandbox is unavailable, the pinned
    /// builder image does not match, the command fails or the declared
    /// artifact is absent.
    fn run(&self, attempt: &BuildAttempt<'_>) -> Result<Vec<u8>, BuildRefusal>;
}

/// The reproducible-build pipeline. Every verification rebuilds the published
/// source at least twice in independent sandboxes, refuses a build whose own
/// output is not stable, validates the rebuilt module against the
/// deterministic runtime policy and only then records build evidence.
#[derive(Clone, Copy, Debug)]
pub struct SourceVerifier<R> {
    runner: R,
    attempts: u32,
}

impl<R: BuildRunner> SourceVerifier<R> {
    /// Creates the pipeline with the declared number of independent rebuilds.
    ///
    /// # Errors
    ///
    /// Refuses fewer than two or more than eight attempts, because a single
    /// build cannot demonstrate reproducibility.
    pub fn new(runner: R, attempts: u32) -> Result<Self, BuildRefusal> {
        if !(MIN_ATTEMPTS..=MAX_ATTEMPTS).contains(&attempts) {
            return Err(BuildRefusal::InvalidAttempts);
        }
        Ok(Self { runner, attempts })
    }

    /// Borrows the configured sandbox runner.
    #[must_use]
    pub const fn runner(&self) -> &R {
        &self.runner
    }

    /// Rebuilds published source into recorded build evidence.
    ///
    /// # Errors
    ///
    /// Refuses invalid source locations, non-canonical archives, unpinned or
    /// mismatched toolchain material, failing sandboxes, unstable rebuilds and
    /// artifacts the deterministic runtime policy rejects.
    pub fn reproduce(
        &self,
        source: &PublishedSource,
        plan: &BuildPlan,
    ) -> Result<ReproducibleBuild, BuildRefusal> {
        crate::validate_uri(&source.uri)?;
        plan.validate()?;
        let archive = SourceArchive::decode(&source.canonical_archive)?;
        if pinned_digest(&archive, &plan.toolchain_manifest)? != plan.environment.toolchain_digest {
            return Err(BuildRefusal::ToolchainMismatch);
        }
        if pinned_digest(&archive, &plan.dependency_lock)?
            != plan.environment.dependency_lock_digest
        {
            return Err(BuildRefusal::DependencyLockMismatch);
        }
        let mut reproduced: Option<Vec<u8>> = None;
        let mut first = [0_u8; 32];
        for attempt in 0..self.attempts {
            let produced = self.runner.run(&BuildAttempt {
                archive: &archive,
                plan,
                attempt,
            })?;
            if produced.is_empty() || produced.len() > MAX_ARTIFACT_BYTES {
                return Err(BuildRefusal::InvalidArtifact);
            }
            let digest = sha256(&produced);
            if attempt == 0 {
                first = digest;
                reproduced = Some(produced);
            } else if digest != first {
                return Err(BuildRefusal::NondeterministicBuild {
                    first,
                    repeated: digest,
                });
            }
        }
        let artifact = reproduced.ok_or(BuildRefusal::InvalidArtifact)?;
        validate_artifact(&artifact)?;
        ReproducibleBuild::from_output(source, plan.environment.clone(), &artifact)
            .map_err(BuildRefusal::Registry)
    }
}

/// Typed refusal produced by the reproducible-build pipeline. Every refusal
/// names the exact stage that failed so a mismatch is never reported as a
/// verified source.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum BuildRefusal {
    Registry(RegistryError),
    Archive(ArchiveError),
    InvalidPlan,
    InvalidAttempts,
    MissingPinnedFile { path: String },
    ToolchainMismatch,
    DependencyLockMismatch,
    BuilderImageMismatch,
    SandboxUnavailable { reason: String },
    BuilderFailed { reason: String },
    MissingArtifact { path: String },
    InvalidArtifact,
    NondeterministicBuild { first: [u8; 32], repeated: [u8; 32] },
    ArtifactRejected { reason: String },
    Engine { reason: String },
}

impl Display for BuildRefusal {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Registry(error) => write!(formatter, "registry refusal: {error}"),
            Self::Archive(error) => write!(formatter, "source refusal: {error}"),
            Self::InvalidPlan => formatter.write_str("declared build plan is invalid"),
            Self::InvalidAttempts => {
                formatter.write_str("a reproducible build needs at least two independent attempts")
            }
            Self::MissingPinnedFile { path } => {
                write!(formatter, "published source does not contain {path}")
            }
            Self::ToolchainMismatch => {
                formatter.write_str("published toolchain pin does not match the declared digest")
            }
            Self::DependencyLockMismatch => {
                formatter.write_str("published dependency lock does not match the declared digest")
            }
            Self::BuilderImageMismatch => {
                formatter.write_str("declared builder image is not the pinned builder image")
            }
            Self::SandboxUnavailable { reason } => {
                write!(formatter, "hermetic build sandbox unavailable: {reason}")
            }
            Self::BuilderFailed { reason } => write!(formatter, "pinned build failed: {reason}"),
            Self::MissingArtifact { path } => {
                write!(formatter, "pinned build produced no artifact at {path}")
            }
            Self::InvalidArtifact => {
                formatter.write_str("pinned build produced an empty or oversized artifact")
            }
            Self::NondeterministicBuild { first, repeated } => write!(
                formatter,
                "rebuild is not reproducible: {} then {}",
                hex::encode(first),
                hex::encode(repeated)
            ),
            Self::ArtifactRejected { reason } => {
                write!(
                    formatter,
                    "rebuilt module violates the runtime policy: {reason}"
                )
            }
            Self::Engine { reason } => {
                write!(formatter, "deterministic engine unavailable: {reason}")
            }
        }
    }
}

impl std::error::Error for BuildRefusal {}

impl From<RegistryError> for BuildRefusal {
    fn from(value: RegistryError) -> Self {
        Self::Registry(value)
    }
}

impl From<ArchiveError> for BuildRefusal {
    fn from(value: ArchiveError) -> Self {
        Self::Archive(value)
    }
}

fn pinned_digest(archive: &SourceArchive, path: &str) -> Result<[u8; 32], BuildRefusal> {
    let file = archive
        .file(path)
        .ok_or_else(|| BuildRefusal::MissingPinnedFile {
            path: path.to_owned(),
        })?;
    Ok(sha256(&file.content))
}

fn validate_artifact(wasm: &[u8]) -> Result<(), BuildRefusal> {
    let engine = WasmEngine::declared().map_err(|refusal| BuildRefusal::Engine {
        reason: refusal.to_string(),
    })?;
    engine
        .validate(wasm)
        .map_err(|refusal| BuildRefusal::ArtifactRejected {
            reason: refusal.to_string(),
        })?;
    Ok(())
}

fn field<'text>(
    fields: &BTreeMap<&'text str, &'text str>,
    key: &str,
) -> Result<&'text str, BuildRefusal> {
    fields.get(key).copied().ok_or(BuildRefusal::InvalidPlan)
}

fn digest<'text>(
    fields: &BTreeMap<&'text str, &'text str>,
    key: &str,
) -> Result<[u8; 32], BuildRefusal> {
    hex::decode_digest(field(fields, key)?).map_err(BuildRefusal::Registry)
}
