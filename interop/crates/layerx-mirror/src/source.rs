//! Pinned production mirror reads and non-composable source selection.

use std::collections::BTreeSet;

use crate::ethereum::{EthereumError, EthereumMirrorReadConfig, EthereumMirrorReader};
use crate::rpc::RpcError;
use crate::solana::{SolanaError, SolanaMirrorReadConfig, SolanaMirrorReader};
use crate::{archive_commitment, ArchiveCommitment, ArchiveData, CheckpointCoordinate};

pub const MAX_MIRROR_SOURCES: usize = 8;
pub const MAX_SOURCE_ID_BYTES: usize = 64;

#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub struct MirrorSourceId(String);

impl MirrorSourceId {
    pub fn new(value: impl Into<String>) -> Result<Self, MirrorSourceError> {
        let value = value.into();
        if value.is_empty()
            || value.len() > MAX_SOURCE_ID_BYTES
            || value.bytes().any(|byte| byte.is_ascii_control())
        {
            return Err(MirrorSourceError::Configuration);
        }
        Ok(Self(value))
    }

    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum MirrorTargetIdentity {
    Ethereum {
        chain_id: u64,
        genesis_hash: [u8; 32],
        contract: [u8; 20],
        code_hash: [u8; 32],
        publisher: [u8; 20],
    },
    Solana {
        genesis_hash: [u8; 32],
        program: [u8; 32],
        program_data: [u8; 32],
        code_hash: [u8; 32],
        publisher: [u8; 32],
    },
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum MirrorCanonicalPosition {
    Ethereum {
        block_number: u64,
        block_hash: [u8; 32],
        reference_head_number: u64,
        reference_head_hash: [u8; 32],
    },
    Solana {
        rooted_slot: u64,
        rooted_blockhash: [u8; 32],
    },
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum MirrorProvenance {
    Canonical,
    Reorged,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum MirrorLag {
    Known(u64),
    Unknown,
}

/// Per-source freshness. A source read does not claim to be the latest archive
/// unless the source ABI independently proves such a head.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct MirrorSourceFreshness {
    pub latest_batch: Option<u64>,
    pub latest_checkpoint: Option<CheckpointCoordinate>,
    pub batch_lag: MirrorLag,
    pub checkpoint_lag: MirrorLag,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct MirrorObservation {
    pub source: MirrorSourceId,
    pub target: MirrorTargetIdentity,
    pub commitment: ArchiveCommitment,
    pub batch_number: u64,
    pub checkpoint: Option<CheckpointCoordinate>,
    pub position: MirrorCanonicalPosition,
    pub provenance: MirrorProvenance,
    pub freshness: MirrorSourceFreshness,
    pub failover_count: usize,
    pub agreeing_sources: usize,
}

/// One source's complete evidence. Fields are private so callers cannot pair
/// an archive from one source with chain observations from another.
pub struct ObservedArchive {
    archive: Vec<u8>,
    observation: MirrorObservation,
}

impl ObservedArchive {
    #[must_use]
    pub fn archive(&self) -> &[u8] {
        &self.archive
    }

    #[must_use]
    pub fn observation(&self) -> &MirrorObservation {
        &self.observation
    }

    pub(crate) fn into_parts(self) -> (Vec<u8>, MirrorObservation) {
        (self.archive, self.observation)
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct MirrorLocator {
    pub source_index: usize,
    pub commitment: ArchiveCommitment,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum MirrorReadPolicy {
    Exact(MirrorLocator),
    OrderedPreference(Vec<MirrorLocator>),
    Agreement {
        candidates: Vec<MirrorLocator>,
        minimum: usize,
    },
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum MirrorSourceError {
    Configuration,
    Unavailable,
    RateLimited { retry_after_seconds: u64 },
    RpcDivergent,
    Missing,
    Archive,
    TargetMismatch,
    Divergent,
    InsufficientAgreement,
}

impl From<EthereumError> for MirrorSourceError {
    fn from(value: EthereumError) -> Self {
        match value {
            EthereumError::Configuration => Self::Configuration,
            EthereumError::Rpc(RpcError::RateLimited {
                retry_after_seconds,
            }) => Self::RateLimited {
                retry_after_seconds,
            },
            EthereumError::Rpc(RpcError::Divergence | RpcError::ResponseMismatch) => {
                Self::RpcDivergent
            }
            EthereumError::ChainIdentity | EthereumError::ContractIdentity => Self::TargetMismatch,
            EthereumError::Retrieval => Self::Archive,
            _ => Self::Unavailable,
        }
    }
}

impl From<SolanaError> for MirrorSourceError {
    fn from(value: SolanaError) -> Self {
        match value {
            SolanaError::Configuration => Self::Configuration,
            SolanaError::Rpc(RpcError::RateLimited {
                retry_after_seconds,
            }) => Self::RateLimited {
                retry_after_seconds,
            },
            SolanaError::Rpc(RpcError::Divergence | RpcError::ResponseMismatch) => {
                Self::RpcDivergent
            }
            SolanaError::ClusterIdentity
            | SolanaError::ProgramIdentity
            | SolanaError::ProgramMutable
            | SolanaError::Pda => Self::TargetMismatch,
            SolanaError::Retrieval => Self::Archive,
            _ => Self::Unavailable,
        }
    }
}

enum SourceReader {
    Ethereum(EthereumMirrorReadConfig),
    Solana(SolanaMirrorReadConfig),
}

pub struct MirrorSource {
    id: MirrorSourceId,
    target: MirrorTargetIdentity,
    reader: SourceReader,
}

impl MirrorSource {
    pub fn ethereum(
        id: MirrorSourceId,
        config: EthereumMirrorReadConfig,
    ) -> Result<Self, MirrorSourceError> {
        if config.chain_id == 0
            || config.genesis_hash == [0; 32]
            || config.archive_contract == [0; 20]
            || config.archive_code_hash == [0; 32]
            || config.publisher == [0; 20]
        {
            return Err(MirrorSourceError::Configuration);
        }
        let target = MirrorTargetIdentity::Ethereum {
            chain_id: config.chain_id,
            genesis_hash: config.genesis_hash,
            contract: config.archive_contract,
            code_hash: config.archive_code_hash,
            publisher: config.publisher,
        };
        Ok(Self {
            id,
            target,
            reader: SourceReader::Ethereum(config),
        })
    }

    pub fn solana(
        id: MirrorSourceId,
        config: SolanaMirrorReadConfig,
    ) -> Result<Self, MirrorSourceError> {
        if config.genesis_hash == [0; 32]
            || config.archive_program == [0; 32]
            || config.upgradeable_loader == [0; 32]
            || config.program_data_account == [0; 32]
            || config.program_code_hash == [0; 32]
            || config.publisher == [0; 32]
        {
            return Err(MirrorSourceError::Configuration);
        }
        let target = MirrorTargetIdentity::Solana {
            genesis_hash: config.genesis_hash,
            program: config.archive_program,
            program_data: config.program_data_account,
            code_hash: config.program_code_hash,
            publisher: config.publisher,
        };
        Ok(Self {
            id,
            target,
            reader: SourceReader::Solana(config),
        })
    }

    fn read(
        &self,
        layerx_network_id: u32,
        batch_number: u64,
        commitment: ArchiveCommitment,
    ) -> Result<Option<ObservedArchive>, MirrorSourceError> {
        let (archive, position) = match &self.reader {
            SourceReader::Ethereum(config) => {
                let reader = EthereumMirrorReader::open(config.clone())?;
                let Some(value) = reader.retrieve(commitment)? else {
                    return Ok(None);
                };
                let position = MirrorCanonicalPosition::Ethereum {
                    block_number: value.block_number,
                    block_hash: value.block_hash,
                    reference_head_number: value.reference_head_number,
                    reference_head_hash: value.reference_head_hash,
                };
                (value.archive, position)
            }
            SourceReader::Solana(config) => {
                let reader = SolanaMirrorReader::open(config.clone())?;
                let Some(value) = reader.retrieve(commitment)? else {
                    return Ok(None);
                };
                let position = MirrorCanonicalPosition::Solana {
                    rooted_slot: value.rooted_slot,
                    rooted_blockhash: value.rooted_blockhash,
                };
                (value.archive, position)
            }
        };
        if archive_commitment(&archive) != commitment {
            return Err(MirrorSourceError::Archive);
        }
        let decoded = ArchiveData::decode(&archive).map_err(|_| MirrorSourceError::Archive)?;
        if decoded.network_id != layerx_network_id || decoded.batch_number != batch_number {
            return Err(MirrorSourceError::TargetMismatch);
        }
        let checkpoint = decoded.checkpoint.as_ref().map(|value| value.coordinate);
        Ok(Some(ObservedArchive {
            archive,
            observation: MirrorObservation {
                source: self.id.clone(),
                target: self.target.clone(),
                commitment,
                batch_number,
                checkpoint,
                position,
                provenance: MirrorProvenance::Canonical,
                freshness: MirrorSourceFreshness {
                    latest_batch: None,
                    latest_checkpoint: None,
                    batch_lag: MirrorLag::Unknown,
                    checkpoint_lag: MirrorLag::Unknown,
                },
                failover_count: 0,
                agreeing_sources: 1,
            },
        }))
    }

    fn recheck(&self, position: MirrorCanonicalPosition) -> Result<bool, MirrorSourceError> {
        match (&self.reader, position) {
            (
                SourceReader::Ethereum(config),
                MirrorCanonicalPosition::Ethereum {
                    block_number,
                    block_hash,
                    ..
                },
            ) => Ok(EthereumMirrorReader::open(config.clone())?
                .is_coordinate_canonical(block_number, block_hash)?),
            (
                SourceReader::Solana(config),
                MirrorCanonicalPosition::Solana {
                    rooted_slot,
                    rooted_blockhash,
                },
            ) => Ok(SolanaMirrorReader::open(config.clone())?
                .is_coordinate_canonical(rooted_slot, rooted_blockhash)?),
            _ => Err(MirrorSourceError::TargetMismatch),
        }
    }
}

/// Operator-created set of pinned sources. Public verification inputs select
/// only indices in this set; they cannot inject endpoints or trust material.
pub struct MirrorSources {
    layerx_network_id: u32,
    sources: Vec<MirrorSource>,
}

impl MirrorSources {
    pub fn new(
        layerx_network_id: u32,
        sources: Vec<MirrorSource>,
    ) -> Result<Self, MirrorSourceError> {
        if layerx_network_id == 0 || sources.is_empty() || sources.len() > MAX_MIRROR_SOURCES {
            return Err(MirrorSourceError::Configuration);
        }
        let identities = sources
            .iter()
            .map(|source| source.id.clone())
            .collect::<BTreeSet<_>>();
        if identities.len() != sources.len() {
            return Err(MirrorSourceError::Configuration);
        }
        Ok(Self {
            layerx_network_id,
            sources,
        })
    }

    /// Reads all policy candidates so a conflict cannot be hidden by an
    /// earlier preferred response. The selected archive always remains one
    /// source's indivisible evidence bundle.
    pub fn read(
        &self,
        batch_number: u64,
        policy: &MirrorReadPolicy,
    ) -> Result<ObservedArchive, MirrorSourceError> {
        if batch_number == 0 {
            return Err(MirrorSourceError::Configuration);
        }
        let (locators, minimum, exact) = match policy {
            MirrorReadPolicy::Exact(locator) => (std::slice::from_ref(locator), 1, true),
            MirrorReadPolicy::OrderedPreference(locators) => (locators.as_slice(), 1, false),
            MirrorReadPolicy::Agreement {
                candidates,
                minimum,
            } => (candidates.as_slice(), *minimum, false),
        };
        if locators.is_empty()
            || locators.len() > self.sources.len()
            || minimum == 0
            || minimum > locators.len()
        {
            return Err(MirrorSourceError::Configuration);
        }
        if locators
            .iter()
            .map(|locator| locator.commitment)
            .collect::<BTreeSet<_>>()
            .len()
            != 1
        {
            return Err(MirrorSourceError::Divergent);
        }
        let mut seen = BTreeSet::new();
        let mut selected = None;
        let mut success_count = 0_usize;
        let mut observed_commitment = None;
        let mut first_error = None;
        for (preference, locator) in locators.iter().enumerate() {
            if locator.source_index >= self.sources.len() || !seen.insert(locator.source_index) {
                return Err(MirrorSourceError::Configuration);
            }
            match self.sources[locator.source_index].read(
                self.layerx_network_id,
                batch_number,
                locator.commitment,
            ) {
                Ok(Some(mut archive)) => {
                    if observed_commitment
                        .is_some_and(|value| value != archive.observation.commitment)
                    {
                        return Err(MirrorSourceError::Divergent);
                    }
                    observed_commitment = Some(archive.observation.commitment);
                    archive.observation.failover_count = preference;
                    success_count = success_count.saturating_add(1);
                    if selected.is_none() {
                        selected = Some(archive);
                    }
                }
                Ok(None) => {
                    first_error.get_or_insert(MirrorSourceError::Missing);
                }
                Err(error) => {
                    first_error.get_or_insert(error);
                }
            }
        }
        if exact && success_count == 0 {
            return Err(first_error.unwrap_or(MirrorSourceError::Unavailable));
        }
        if success_count < minimum {
            return Err(if success_count == 0 {
                first_error.unwrap_or(MirrorSourceError::Unavailable)
            } else {
                MirrorSourceError::InsufficientAgreement
            });
        }
        let mut selected = selected.ok_or(MirrorSourceError::Unavailable)?;
        selected.observation.agreeing_sources = success_count;
        Ok(selected)
    }

    /// Rechecks publication provenance while retaining the already verified
    /// archive bytes and signed LayerX evidence.
    pub fn recheck(
        &self,
        mut archive: ObservedArchive,
    ) -> Result<ObservedArchive, MirrorSourceError> {
        let source = self
            .sources
            .iter()
            .find(|candidate| candidate.id == archive.observation.source)
            .ok_or(MirrorSourceError::TargetMismatch)?;
        archive.observation.provenance = if source.recheck(archive.observation.position)? {
            MirrorProvenance::Canonical
        } else {
            MirrorProvenance::Reorged
        };
        Ok(archive)
    }
}
