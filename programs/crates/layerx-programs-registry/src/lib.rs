#![forbid(unsafe_code)]

mod account_state;
mod archive;
mod authority;
mod deprecate;
mod hash;
mod interface;
pub mod hex;
mod pipeline;
mod protocol_evidence;
mod resolver;

pub use account_state::{
    account_tree_commitment, program_account_registration_commitment, programs_root_commitment,
    state_leaf_commitment, state_node_commitment, universal_root_commitment, AccountStateError,
    AccountStateHead, AccountStateJournal, CanonicalAccountLeaf, JournalAccountStateAuthority,
    ProgramValueAccountBinding, ProvenAccountLeaf, ProvenProgramBinding, StateProof, ValueAccount,
    VerifiedAccountSnapshot, verify_state_membership, MAX_PROGRAM_VALUE_ACCOUNTS,
};
pub use archive::{ArchiveError, SourceArchive, SourceFile};
pub use authority::{
    DeploymentJournal, DeploymentRecord, JournalReadAuthority, ObservedHead,
};
pub use deprecate::{
    AuthorizedExit, Deprecation, DeprecationRefusal, DeprecationRequest, ExitRoute,
    LegacyDeprecationRequest, WindDownExitActivity, WindDownView,
};
pub use interface::{
    interface_state_key, interface_state_value, verify_interface_read, InterfaceCapability,
    InterfaceDigest, InterfaceEntryPoint, InterfaceRefusal, InterfaceStateWitness,
    ProgramInterface, SchemaVariant, TypedFailure, ValueSchema, ValueType,
    VerifiedInterfaceRead,
};
pub use pipeline::{BuildAttempt, BuildPlan, BuildRefusal, BuildRunner, SourceVerifier};
pub use protocol_evidence::{
    DeploymentProof, ProgramLifecycleProof, ProgramStateProof, ProtocolDeploymentVerifier,
    ProtocolEvidenceError, StateLeafWitness, VerifiedDeploymentEvidence, VerifiedProgramHead,
    VerifiedProtocolHead,
};
pub use resolver::{ExecutableAdmissionError, VerifiedProgramCatalog};

use core::fmt::{self, Display};
use std::collections::BTreeMap;

use layerx_programs_runtime::{DeploymentReceipt, ProgramVersion};
pub use layerx_programs_runtime::{ProgramId, UpgradePolicy};

use hash::sha256;

const TEXT_LIMIT: usize = 512;
const COMMAND_LIMIT: usize = 32;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct BuildEnvironment {
    pub builder_image_digest: [u8; 32],
    pub toolchain_digest: [u8; 32],
    pub dependency_lock_digest: [u8; 32],
    pub source_date_epoch: u64,
    pub command: Vec<String>,
}

impl BuildEnvironment {
    pub(crate) fn validate(&self) -> Result<(), RegistryError> {
        if self.builder_image_digest == [0; 32]
            || self.toolchain_digest == [0; 32]
            || self.dependency_lock_digest == [0; 32]
            || self.source_date_epoch == 0
            || self.command.is_empty()
            || self.command.len() > COMMAND_LIMIT
            || self.command.iter().any(|part| {
                part.is_empty()
                    || part.len() > TEXT_LIMIT
                    || part.bytes().any(|byte| byte.is_ascii_control())
            })
        {
            return Err(RegistryError::InvalidBuildEnvironment);
        }
        Ok(())
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PublishedSource {
    pub uri: String,
    pub canonical_archive: Vec<u8>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ReproducibleBuild {
    pub source_uri: String,
    pub source_digest: [u8; 32],
    pub environment: BuildEnvironment,
    pub environment_digest: [u8; 32],
    pub artifact_digest: [u8; 32],
}

impl ReproducibleBuild {
    /// Records the exact source, hermetic build environment and resulting
    /// artifact digest from a completed reproducible build.
    ///
    /// # Errors
    ///
    /// Refuses invalid source locations, empty inputs and unpinned build
    /// environments.
    pub fn from_output(
        source: &PublishedSource,
        environment: BuildEnvironment,
        wasm: &[u8],
    ) -> Result<Self, RegistryError> {
        validate_uri(&source.uri)?;
        if source.canonical_archive.is_empty() || wasm.is_empty() {
            return Err(RegistryError::EmptyArtifact);
        }
        environment.validate()?;
        let environment_digest = environment_hash(&environment);
        Ok(Self {
            source_uri: source.uri.clone(),
            source_digest: sha256(&source.canonical_archive),
            environment,
            environment_digest,
            artifact_digest: sha256(wasm),
        })
    }

    /// Reconstructs recorded build evidence from a durable verification
    /// record so a verified-source status survives a restart without
    /// rebuilding, while the artifact digest is still compared with protocol
    /// state before any status is published.
    ///
    /// # Errors
    ///
    /// Refuses invalid source locations, absent digests and unpinned build
    /// environments.
    pub fn from_record(
        source_uri: String,
        source_digest: [u8; 32],
        environment: BuildEnvironment,
        artifact_digest: [u8; 32],
    ) -> Result<Self, RegistryError> {
        validate_uri(&source_uri)?;
        if source_digest == [0; 32] || artifact_digest == [0; 32] {
            return Err(RegistryError::EmptyArtifact);
        }
        environment.validate()?;
        let environment_digest = environment_hash(&environment);
        Ok(Self {
            source_uri,
            source_digest,
            environment,
            environment_digest,
            artifact_digest,
        })
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SourceStatus {
    Unpublished,
    Verified {
        source_digest: [u8; 32],
        environment_digest: [u8; 32],
    },
    Mismatch {
        expected: [u8; 32],
        reproduced: [u8; 32],
    },
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ProgramLifecycle {
    Active,
    Deprecated,
    Tombstoned,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum WindDownStateAccess {
    ReadOnly,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct WindDownPolicy {
    pub exit_program: [u8; 32],
    pub deadline: u64,
    pub state_access: WindDownStateAccess,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct LifecycleTransition {
    pub program: ProgramId,
    pub expected: ProgramLifecycle,
    pub target: ProgramLifecycle,
    pub authority: [u8; 32],
    pub effective_sequence: u64,
    pub wind_down: WindDownPolicy,
    pub live_value_accounts: u32,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct LifecycleReceipt {
    pub program: ProgramId,
    pub prior: ProgramLifecycle,
    pub current: ProgramLifecycle,
    pub authority: [u8; 32],
    pub effective_sequence: u64,
    pub wind_down: WindDownPolicy,
    pub live_value_accounts: u32,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RegistryVersion {
    pub number: u32,
    pub code_hash: [u8; 32],
    pub abi_version: u16,
    pub deployment_receipt_digest: [u8; 32],
    pub source: SourceStatus,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RegistryEntry {
    pub program: ProgramId,
    pub upgrade_policy: UpgradePolicy,
    pub lifecycle: ProgramLifecycle,
    pub versions: Vec<RegistryVersion>,
    pub lifecycle_history: Vec<LifecycleReceipt>,
    pub value_accounts: Vec<ProgramValueAccountBinding>,
    pub exit_routes: Vec<ExitRoute>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ReadFreshness {
    pub observed_sequence: u64,
    pub observed_at: u64,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct VerifiedRegistryRead {
    pub entry: RegistryEntry,
    pub receipt_digest: [u8; 32],
    pub freshness: ReadFreshness,
}

/// Current program-held balances after both the durable Programs primary
/// index and every live `MODULE_VALUE` account have been proven into one
/// canonical receipt state root.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct VerifiedProgramBalanceRead {
    program: ProgramId,
    lifecycle: ProgramLifecycle,
    bindings: Vec<ProgramValueAccountBinding>,
    value_accounts: Vec<ValueAccount>,
    receipt_digest: [u8; 32],
    state_root: [u8; 32],
    freshness: ReadFreshness,
}

impl VerifiedProgramBalanceRead {
    #[must_use]
    pub const fn program(&self) -> ProgramId {
        self.program
    }

    #[must_use]
    pub const fn lifecycle(&self) -> ProgramLifecycle {
        self.lifecycle
    }

    #[must_use]
    pub fn value_accounts(&self) -> &[ValueAccount] {
        &self.value_accounts
    }

    #[must_use]
    pub fn bindings(&self) -> &[ProgramValueAccountBinding] {
        &self.bindings
    }

    #[must_use]
    pub const fn receipt_digest(&self) -> [u8; 32] {
        self.receipt_digest
    }

    #[must_use]
    pub const fn state_root(&self) -> [u8; 32] {
        self.state_root
    }

    #[must_use]
    pub const fn freshness(&self) -> ReadFreshness {
        self.freshness
    }
}

pub trait RegistryReadAuthority {
    /// Verifies the protocol receipt backing the latest registry projection.
    ///
    /// # Errors
    ///
    /// Refuses missing, stale, malformed or mismatched receipt evidence.
    fn verify_registry_read(
        &self,
        program: ProgramId,
        latest: &RegistryVersion,
    ) -> Result<([u8; 32], ReadFreshness), RegistryError>;
}

#[derive(Clone, Debug, Default)]
pub struct Registry {
    entries: BTreeMap<ProgramId, RegistryEntry>,
}

impl Registry {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Returns every durably deployed program in canonical identifier order.
    /// The protocol-state ingestor uses this to obtain a complete initial
    /// ABI-two projection before it exposes any balance read.
    #[must_use]
    pub fn program_ids(&self) -> Vec<ProgramId> {
        self.entries.keys().copied().collect()
    }

    /// Appends one lifecycle-produced deployment or upgrade to contiguous
    /// registry history.
    ///
    /// # Errors
    ///
    /// Refuses mismatched receipts, policies, code hashes or version history.
    pub fn record_deployment(
        &mut self,
        receipt: &DeploymentReceipt,
        version: &ProgramVersion,
        policy: UpgradePolicy,
        receipt_digest: [u8; 32],
    ) -> Result<(), RegistryError> {
        self.record_deployment_version(
            receipt.program(),
            receipt.version(),
            receipt.old_code_hash(),
            receipt.new_code_hash(),
            version,
            policy,
            receipt_digest,
        )
    }

    /// Appends one deployment only after the canonical activity, successful
    /// receipt, signed batch, resulting Programs root and registry leaf were
    /// verified together.
    pub fn record_verified_deployment(
        &mut self,
        evidence: &VerifiedDeploymentEvidence,
    ) -> Result<(), RegistryError> {
        let record = evidence.record();
        self.record_deployment_version(
            record.program,
            record.version,
            record.old_code_hash,
            record.new_code_hash,
            &record.program_version(),
            record.upgrade_policy,
            evidence.receipt_digest(),
        )
    }

    fn record_deployment_version(
        &mut self,
        program: ProgramId,
        number: u32,
        old_code_hash: Option<[u8; 32]>,
        new_code_hash: [u8; 32],
        version: &ProgramVersion,
        policy: UpgradePolicy,
        receipt_digest: [u8; 32],
    ) -> Result<(), RegistryError> {
        if matches!(
            policy,
            UpgradePolicy::Authority(authority) if authority == [0; 32]
        ) {
            return Err(RegistryError::InvalidUpgradeAuthority);
        }
        if receipt_digest == [0; 32] || new_code_hash != version.code_hash || number == 0 {
            return Err(RegistryError::DeploymentMismatch);
        }
        let entry = self
            .entries
            .entry(program)
            .or_insert_with(|| RegistryEntry {
                program,
                upgrade_policy: policy,
                lifecycle: ProgramLifecycle::Active,
                versions: Vec::new(),
                lifecycle_history: Vec::new(),
                value_accounts: Vec::new(),
                exit_routes: Vec::new(),
            });
        if entry.upgrade_policy != policy
            || usize::try_from(number).ok() != Some(entry.versions.len() + 1)
            || old_code_hash != entry.versions.last().map(|prior| prior.code_hash)
        {
            return Err(RegistryError::VersionHistoryMismatch);
        }
        if let Some(prior) = entry.versions.last() {
            layerx_programs_runtime::admit_abi_upgrade(prior.abi_version, version.abi_version)
                .map_err(RegistryError::AbiVersion)?;
        } else {
            layerx_programs_runtime::admit_abi_version(version.abi_version)
                .map_err(RegistryError::AbiVersion)?;
        }
        entry.versions.push(RegistryVersion {
            number,
            code_hash: version.code_hash,
            abi_version: version.abi_version,
            deployment_receipt_digest: receipt_digest,
            source: SourceStatus::Unpublished,
        });
        Ok(())
    }

    /// Rebuilds the registry projection from the durable canonical deployment
    /// journal, in protocol order, verifying each record before it is applied.
    ///
    /// # Errors
    ///
    /// Refuses corrupt records, code that does not hash to its recorded code
    /// hash, and non-contiguous version history.
    pub fn replay_journal(&mut self, records: &[DeploymentRecord]) -> Result<(), RegistryError> {
        let mut ordered: Vec<&DeploymentRecord> = records.iter().collect();
        ordered.sort_by_key(|record| (record.program.bytes(), record.version));
        for record in ordered {
            record.validate()?;
            self.record_deployment_version(
                record.program,
                record.version,
                record.old_code_hash,
                record.new_code_hash,
                &record.program_version(),
                record.upgrade_policy,
                record.digest(),
            )?;
        }
        Ok(())
    }

    /// Returns the highest recorded version number of a registered program.
    ///
    /// # Errors
    ///
    /// Refuses unknown programs.
    pub fn latest_version(&self, program: ProgramId) -> Result<u32, RegistryError> {
        self.entries
            .get(&program)
            .and_then(|entry| entry.versions.last())
            .map(|version| version.number)
            .ok_or(RegistryError::UnknownProgram)
    }

    /// Compares a hermetic rebuild with the registered on-chain code hash and
    /// stores either verified or visibly mismatched status.
    ///
    /// # Errors
    ///
    /// Refuses unknown programs and versions.
    pub fn verify_source(
        &mut self,
        program: ProgramId,
        version: u32,
        build: &ReproducibleBuild,
    ) -> Result<SourceStatus, RegistryError> {
        let target = self.version_mut(program, version)?;
        let status = if build.artifact_digest == target.code_hash {
            SourceStatus::Verified {
                source_digest: build.source_digest,
                environment_digest: build.environment_digest,
            }
        } else {
            SourceStatus::Mismatch {
                expected: target.code_hash,
                reproduced: build.artifact_digest,
            }
        };
        target.source = status;
        Ok(status)
    }

    /// Returns authority, lifecycle and complete version history only after
    /// the latest registry projection's receipt is independently verified.
    ///
    /// # Errors
    ///
    /// Refuses unknown programs or absent, stale and mismatched evidence.
    pub fn read(
        &self,
        program: ProgramId,
        authority: &impl RegistryReadAuthority,
    ) -> Result<VerifiedRegistryRead, RegistryError> {
        let entry = self
            .entries
            .get(&program)
            .ok_or(RegistryError::UnknownProgram)?;
        let latest = entry.versions.last().ok_or(RegistryError::UnknownProgram)?;
        let (receipt_digest, freshness) = authority.verify_registry_read(program, latest)?;
        if receipt_digest == [0; 32]
            || receipt_digest != latest.deployment_receipt_digest
            || freshness.observed_sequence == 0
            || freshness.observed_at == 0
        {
            return Err(RegistryError::UnverifiedRead);
        }
        Ok(VerifiedRegistryRead {
            entry: entry.clone(),
            receipt_digest,
            freshness,
        })
    }

    /// Resolves one historical or latest program version only from opaque
    /// protocol evidence whose exact claims match the registry projection.
    ///
    /// # Errors
    ///
    /// Refuses unknown versions, corrupt or mismatched records, unavailable
    /// journal state and stale head observations.
    pub fn resolve_deployment(
        &self,
        evidence: VerifiedDeploymentEvidence,
    ) -> Result<VerifiedDeploymentEvidence, RegistryError> {
        let program = evidence.program();
        let version = evidence.version();
        let entry = self
            .entries
            .get(&program)
            .ok_or(RegistryError::UnknownProgram)?;
        let index = version
            .checked_sub(1)
            .and_then(|value| usize::try_from(value).ok())
            .ok_or(RegistryError::UnknownVersion)?;
        let expected = entry
            .versions
            .get(index)
            .filter(|candidate| candidate.number == version)
            .ok_or(RegistryError::UnknownVersion)?;
        if expected.code_hash != evidence.code_hash()
            || expected.abi_version != evidence.abi_version()
            || expected.deployment_receipt_digest != evidence.receipt_digest()
            || entry.upgrade_policy != evidence.record().upgrade_policy
            || entry.lifecycle != ProgramLifecycle::Active
        {
            return Err(RegistryError::UnverifiedRead);
        }
        Ok(evidence)
    }

    /// Records one receipt-verified ABI-two binding in the program registry's
    /// durable primary account enumeration. This records identity and asset
    /// only; balances are always resolved from canonical account-tree proofs.
    ///
    /// # Errors
    ///
    /// Refuses unknown programs, non-canonical registration event records and
    /// any attempt to rebind a seed or account identifier.
    pub fn record_value_account(
        &mut self,
        binding: ProgramValueAccountBinding,
    ) -> Result<(), AccountStateError> {
        binding.validate()?;
        let entry = self
            .entries
            .get(&binding.program)
            .ok_or(AccountStateError::UnknownProgram)?;
        if entry.versions.last().map(|version| version.abi_version) != Some(2) {
            return Err(AccountStateError::LegacyProtocol);
        }
        if entry.lifecycle != ProgramLifecycle::Active {
            return entry
                .value_accounts
                .iter()
                .any(|existing| existing == &binding)
                .then_some(())
                .ok_or(AccountStateError::InactiveProgram);
        }
        let entry = self
            .entries
            .get_mut(&binding.program)
            .ok_or(AccountStateError::UnknownProgram)?;
        if let Some(existing) = entry.value_accounts.iter().find(|existing| {
            existing.seed == binding.seed || existing.account_id == binding.account_id
        }) {
            return if existing == &binding {
                Ok(())
            } else {
                Err(AccountStateError::BindingConflict)
            };
        }
        entry.value_accounts.push(binding);
        entry
            .value_accounts
            .sort_by_key(|binding| binding.account_id);
        Ok(())
    }

    /// Returns the append-only derived-account enumeration for a program.
    ///
    /// # Errors
    ///
    /// Refuses unknown programs.
    pub fn value_account_bindings(
        &self,
        program: ProgramId,
    ) -> Result<&[ProgramValueAccountBinding], AccountStateError> {
        self.entries
            .get(&program)
            .map(|entry| entry.value_accounts.as_slice())
            .ok_or(AccountStateError::UnknownProgram)
    }

    /// Replays the exact protocol-owned binding, route and lifecycle indexes
    /// obtained by the production C adapter. The update is atomic and accepts
    /// inactive programs only when their previously registered bindings are
    /// byte-for-byte unchanged.
    ///
    /// # Errors
    ///
    /// Refuses an ABI mismatch, a non-canonical binding/route, a gap or fork
    /// in lifecycle history, or any conflict with an already replayed index.
    pub fn replay_protocol_state(
        &mut self,
        program: ProgramId,
        bindings: &[ProgramValueAccountBinding],
        routes: &[ExitRoute],
        lifecycle: ProgramLifecycle,
        history: &[LifecycleReceipt],
    ) -> Result<(), RegistryError> {
        let mut candidate = self.clone();
        let entry = candidate
            .entries
            .get(&program)
            .ok_or(RegistryError::UnknownProgram)?;
        if entry.versions.last().map(|version| version.abi_version) != Some(2) {
            return Err(RegistryError::ProtocolStateMismatch);
        }
        for binding in bindings {
            if binding.program != program || binding.validate().is_err() {
                return Err(RegistryError::ProtocolStateMismatch);
            }
        }
        let mut ordered_bindings = bindings.to_vec();
        ordered_bindings.sort_by_key(|binding| binding.account_id);
        if ordered_bindings
            .windows(2)
            .any(|pair| pair[0].account_id == pair[1].account_id || pair[0].seed == pair[1].seed)
        {
            return Err(RegistryError::ProtocolStateMismatch);
        }
        let mut ordered_routes = routes.to_vec();
        ordered_routes.sort_by_key(|route| route.account_id);
        if ordered_routes
            .windows(2)
            .any(|pair| pair[0].account_id == pair[1].account_id)
            || ordered_routes.iter().any(|route| {
                route.destination == [0; 32]
                    || !ordered_bindings.iter().any(|binding| {
                        binding.account_id == route.account_id
                            && binding.asset_id == route.asset_id
                            && binding.seed == route.seed
                    })
            })
        {
            return Err(RegistryError::ProtocolStateMismatch);
        }
        let mut prior = ProgramLifecycle::Active;
        let mut prior_sequence = 0_u64;
        let mut policy = None;
        for receipt in history {
            if receipt.program != program
                || receipt.prior != prior
                || receipt.current == ProgramLifecycle::Active
                || receipt.effective_sequence <= prior_sequence
                || receipt.authority == [0; 32]
                || receipt.wind_down.exit_program != program.bytes()
                || receipt.wind_down.deadline == 0
                || policy.is_some_and(|value| value != receipt.wind_down)
            {
                return Err(RegistryError::ProtocolStateMismatch);
            }
            let edge = matches!(
                (receipt.prior, receipt.current),
                (ProgramLifecycle::Active, ProgramLifecycle::Deprecated)
                    | (ProgramLifecycle::Deprecated, ProgramLifecycle::Tombstoned)
            );
            if !edge {
                return Err(RegistryError::ProtocolStateMismatch);
            }
            prior = receipt.current;
            prior_sequence = receipt.effective_sequence;
            policy = Some(receipt.wind_down);
        }
        if prior != lifecycle
            || (lifecycle == ProgramLifecycle::Active
                && (!history.is_empty() || !ordered_routes.is_empty()))
            || (lifecycle != ProgramLifecycle::Active
                && (history.is_empty() || ordered_routes.len() != ordered_bindings.len()))
        {
            return Err(RegistryError::ProtocolStateMismatch);
        }
        let entry = candidate
            .entries
            .get_mut(&program)
            .ok_or(RegistryError::UnknownProgram)?;
        let retains_bindings = entry
            .value_accounts
            .iter()
            .all(|existing| ordered_bindings.iter().any(|value| value == existing));
        let retains_routes = entry
            .exit_routes
            .iter()
            .all(|existing| ordered_routes.iter().any(|value| value == existing));
        let retains_history = history.starts_with(&entry.lifecycle_history);
        let lifecycle_does_not_regress = match (entry.lifecycle, lifecycle) {
            (ProgramLifecycle::Active, _) => true,
            (
                ProgramLifecycle::Deprecated,
                ProgramLifecycle::Deprecated | ProgramLifecycle::Tombstoned,
            )
            | (ProgramLifecycle::Tombstoned, ProgramLifecycle::Tombstoned) => true,
            _ => false,
        };
        let inactive_indexes_immutable = entry.lifecycle == ProgramLifecycle::Active
            || (entry.value_accounts == ordered_bindings && entry.exit_routes == ordered_routes);
        if !retains_bindings
            || !retains_routes
            || !retains_history
            || !lifecycle_does_not_regress
            || !inactive_indexes_immutable
        {
            return Err(RegistryError::ProtocolStateMismatch);
        }
        entry.value_accounts = ordered_bindings;
        entry.exit_routes = ordered_routes;
        entry.lifecycle_history = history.to_vec();
        entry.lifecycle = lifecycle;
        *self = candidate;
        Ok(())
    }

    /// Resolves the complete ABI-two primary enumeration into current,
    /// receipt-bound balances suitable for agent, explorer and CLI surfaces.
    ///
    /// # Errors
    ///
    /// Refuses ABI one, historical or stale evidence, an incomplete primary
    /// enumeration, and any account, asset or root mismatch.
    pub fn read_value_accounts(
        &self,
        program: ProgramId,
        snapshot: &VerifiedAccountSnapshot,
        authority: &JournalAccountStateAuthority<impl AccountStateJournal>,
    ) -> Result<VerifiedProgramBalanceRead, AccountStateError> {
        let entry = self
            .entries
            .get(&program)
            .ok_or(AccountStateError::UnknownProgram)?;
        if entry.versions.last().map(|version| version.abi_version) != Some(2) {
            return Err(AccountStateError::LegacyProtocol);
        }
        let value_accounts = snapshot.resolve_program(program, &entry.value_accounts, authority)?;
        Ok(VerifiedProgramBalanceRead {
            program,
            lifecycle: entry.lifecycle,
            bindings: entry.value_accounts.clone(),
            value_accounts,
            receipt_digest: snapshot.receipt_digest,
            state_root: snapshot.state_root,
            freshness: snapshot.freshness,
        })
    }

    /// Applies one authority-bearing lifecycle record while retaining the
    /// complete append-only transition history for the wind-down subsystem.
    ///
    /// # Errors
    ///
    /// Refuses unknown programs, stale expected state, missing authority or
    /// malformed transition evidence.
    pub fn transition_lifecycle(
        &mut self,
        request: LifecycleTransition,
    ) -> Result<LifecycleReceipt, RegistryError> {
        if request.authority == [0; 32]
            || request.effective_sequence == 0
            || request.wind_down.exit_program == [0; 32]
            || request.wind_down.deadline == 0
            || request.target == ProgramLifecycle::Active
        {
            return Err(RegistryError::InvalidLifecycleTransition);
        }
        let entry = self
            .entries
            .get_mut(&request.program)
            .ok_or(RegistryError::UnknownProgram)?;
        if !matches!(
            entry.upgrade_policy,
            UpgradePolicy::Authority(expected) if expected == request.authority
        ) {
            return Err(RegistryError::InvalidLifecycleTransition);
        }
        if entry.lifecycle != request.expected {
            return Err(RegistryError::LifecycleConflict);
        }
        let receipt = LifecycleReceipt {
            program: request.program,
            prior: request.expected,
            current: request.target,
            authority: request.authority,
            effective_sequence: request.effective_sequence,
            wind_down: request.wind_down,
            live_value_accounts: request.live_value_accounts,
        };
        entry.lifecycle = request.target;
        entry.lifecycle_history.push(receipt);
        Ok(receipt)
    }

    /// Returns immutable registry state for the wind-down rules engine.
    ///
    /// # Errors
    ///
    /// Refuses an unknown program.
    pub fn entry_for_wind_down(&self, program: ProgramId) -> Result<&RegistryEntry, RegistryError> {
        self.entries
            .get(&program)
            .ok_or(RegistryError::UnknownProgram)
    }

    fn version_mut(
        &mut self,
        program: ProgramId,
        version: u32,
    ) -> Result<&mut RegistryVersion, RegistryError> {
        let index = version
            .checked_sub(1)
            .and_then(|value| usize::try_from(value).ok())
            .ok_or(RegistryError::UnknownVersion)?;
        self.entries
            .get_mut(&program)
            .ok_or(RegistryError::UnknownProgram)?
            .versions
            .get_mut(index)
            .ok_or(RegistryError::UnknownVersion)
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RegistryError {
    InvalidSourceUri,
    InvalidBuildEnvironment,
    EmptyArtifact,
    UnknownProgram,
    UnknownVersion,
    DeploymentMismatch,
    InvalidUpgradeAuthority,
    VersionHistoryMismatch,
    AbiVersion(layerx_programs_runtime::AbiVersionRefusal),
    UnverifiedRead,
    StaleRead,
    CorruptRecord,
    JournalUnavailable,
    InvalidDigestEncoding,
    InvalidLifecycleTransition,
    LifecycleConflict,
    ProtocolStateMismatch,
}

impl Display for RegistryError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        let message = match self {
            Self::InvalidSourceUri => "published source URI is invalid",
            Self::InvalidBuildEnvironment => "reproducible build environment is invalid",
            Self::EmptyArtifact => "source archive or build artifact is empty",
            Self::UnknownProgram => "program is not registered",
            Self::UnknownVersion => "program version is not registered",
            Self::DeploymentMismatch => "deployment receipt and program version do not match",
            Self::InvalidUpgradeAuthority => "upgrade authority uses the reserved zero identifier",
            Self::VersionHistoryMismatch => "program version history is not contiguous",
            Self::AbiVersion(_) => "program ABI version transition was refused",
            Self::UnverifiedRead => "registry read lacks matching receipt evidence",
            Self::StaleRead => "registry read is older than the declared freshness bound",
            Self::CorruptRecord => "canonical deployment record is corrupt",
            Self::JournalUnavailable => "canonical deployment journal is unavailable",
            Self::InvalidDigestEncoding => "digest is not thirty-two hexadecimal-encoded bytes",
            Self::InvalidLifecycleTransition => "program lifecycle transition is invalid",
            Self::LifecycleConflict => "program lifecycle state changed before transition",
            Self::ProtocolStateMismatch => {
                "protocol program-state replay conflicts with the canonical registry"
            }
        };
        formatter.write_str(message)
    }
}

impl std::error::Error for RegistryError {}

pub(crate) fn validate_uri(uri: &str) -> Result<(), RegistryError> {
    if uri.len() > TEXT_LIMIT
        || !(uri.starts_with("https://") || uri.starts_with("ipfs://"))
        || uri
            .bytes()
            .any(|byte| byte.is_ascii_control() || byte.is_ascii_whitespace())
    {
        Err(RegistryError::InvalidSourceUri)
    } else {
        Ok(())
    }
}

fn environment_hash(environment: &BuildEnvironment) -> [u8; 32] {
    let mut bytes = Vec::new();
    bytes.extend_from_slice(b"LayerX/program-build-environment/v1\0");
    bytes.extend_from_slice(&environment.builder_image_digest);
    bytes.extend_from_slice(&environment.toolchain_digest);
    bytes.extend_from_slice(&environment.dependency_lock_digest);
    bytes.extend_from_slice(&environment.source_date_epoch.to_be_bytes());
    for part in &environment.command {
        bytes.extend_from_slice(&(part.len() as u64).to_be_bytes());
        bytes.extend_from_slice(part.as_bytes());
    }
    sha256(&bytes)
}

#[must_use]
pub const fn programs_source_verification() -> &'static str {
    "sha256-source-artifact-reproducible-build-v1"
}
