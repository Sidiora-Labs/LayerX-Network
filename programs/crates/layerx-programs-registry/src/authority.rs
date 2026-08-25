//! Receipt-verified registry reads over the canonical deployment journal.
//!
//! A registry projection is never returned on its own authority. Every read
//! re-derives the deployment digest from the canonical journal record, checks
//! that the record commits to exactly the projected identifier, version, code
//! hash and ABI version, checks that the recorded module still hashes to the
//! registered code hash, and refuses reads whose observed head is stale.

use core::fmt::{self, Display};

use layerx_programs_runtime::{
    DeploymentReceipt, ExecutionRecord, MeteredUsage, ProgramId, ProgramVersion, UpgradePolicy,
    WasmValue,
};

use crate::hash::sha256;
use crate::{ReadFreshness, RegistryError, RegistryReadAuthority, RegistryVersion};

const RECORD_DOMAIN: &[u8] = b"LayerX/programs/registry/deployment/v1\0";
const MAX_MODULE_BYTES: usize = 32 * 1024 * 1024;
const MAX_OUTPUTS: usize = 256;

/// The protocol head last observed by the node boundary that feeds the
/// journal. Reads carry it so every answer states exactly how fresh it is.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ObservedHead {
    pub sequence: u64,
    pub observed_at: u64,
}

/// Opaque callable deployment material admitted from the canonical journal.
/// Ordinary callers can inspect this evidence but cannot construct it.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct VerifiedDeploymentEvidence {
    program: ProgramId,
    version: u32,
    code_hash: [u8; 32],
    abi_version: u16,
    receipt_digest: [u8; 32],
    freshness: ReadFreshness,
    module: Vec<u8>,
    migration: Option<ExecutionRecord>,
}

impl VerifiedDeploymentEvidence {
    #[must_use]
    pub const fn program(&self) -> ProgramId {
        self.program
    }

    #[must_use]
    pub const fn version(&self) -> u32 {
        self.version
    }

    #[must_use]
    pub const fn code_hash(&self) -> [u8; 32] {
        self.code_hash
    }

    #[must_use]
    pub const fn abi_version(&self) -> u16 {
        self.abi_version
    }

    #[must_use]
    pub const fn receipt_digest(&self) -> [u8; 32] {
        self.receipt_digest
    }

    #[must_use]
    pub const fn freshness(&self) -> ReadFreshness {
        self.freshness
    }

    #[must_use]
    pub fn module(&self) -> &[u8] {
        &self.module
    }

    #[must_use]
    pub const fn migration(&self) -> Option<&ExecutionRecord> {
        self.migration.as_ref()
    }
}

/// One canonical deployment or upgrade record. The record is the durable,
/// digest-addressed evidence a registry read is verified against.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DeploymentRecord {
    pub program: ProgramId,
    pub version: u32,
    pub abi_version: u16,
    pub upgrade_policy: UpgradePolicy,
    pub old_code_hash: Option<[u8; 32]>,
    pub new_code_hash: [u8; 32],
    pub sequence: u64,
    pub observed_at: u64,
    pub module: Vec<u8>,
    pub migration: Option<ExecutionRecord>,
}

impl DeploymentRecord {
    /// Records one accepted lifecycle outcome for the durable journal.
    ///
    /// # Errors
    ///
    /// Refuses receipts whose code hash, version or module do not agree.
    pub fn from_deployment(
        receipt: &DeploymentReceipt,
        version: &ProgramVersion,
        upgrade_policy: UpgradePolicy,
        sequence: u64,
        observed_at: u64,
    ) -> Result<Self, RegistryError> {
        if receipt.new_code_hash() != version.code_hash {
            return Err(RegistryError::DeploymentMismatch);
        }
        let record = Self {
            program: receipt.program(),
            version: receipt.version(),
            abi_version: version.abi_version,
            upgrade_policy,
            old_code_hash: receipt.old_code_hash(),
            new_code_hash: receipt.new_code_hash(),
            sequence,
            observed_at,
            module: version.wasm.clone(),
            migration: receipt.migration().cloned(),
        };
        record.validate()?;
        Ok(record)
    }

    /// Checks the record's internal bindings before it is trusted anywhere.
    ///
    /// # Errors
    ///
    /// Refuses zero versions, absent code, reserved authorities, oversized
    /// modules, modules that do not hash to the recorded code hash, and
    /// inconsistent version history.
    pub fn validate(&self) -> Result<(), RegistryError> {
        if self.version == 0
            || self.new_code_hash == [0; 32]
            || self.sequence == 0
            || self.observed_at == 0
            || self.module.is_empty()
            || self.module.len() > MAX_MODULE_BYTES
            || matches!(self.upgrade_policy, UpgradePolicy::Authority([0; 32]))
        {
            return Err(RegistryError::CorruptRecord);
        }
        if (self.version == 1) != self.old_code_hash.is_none() {
            return Err(RegistryError::VersionHistoryMismatch);
        }
        if sha256(&self.module) != self.new_code_hash {
            return Err(RegistryError::DeploymentMismatch);
        }
        Ok(())
    }

    /// Encodes the canonical, digest-addressed record bytes.
    #[must_use]
    pub fn canonical_encoding(&self) -> Vec<u8> {
        let mut bytes = Vec::with_capacity(self.module.len().saturating_add(256));
        bytes.extend_from_slice(RECORD_DOMAIN);
        bytes.extend_from_slice(&self.program.bytes());
        bytes.extend_from_slice(&self.version.to_be_bytes());
        bytes.extend_from_slice(&self.abi_version.to_be_bytes());
        match self.upgrade_policy {
            UpgradePolicy::Immutable => bytes.push(0),
            UpgradePolicy::Authority(authority) => {
                bytes.push(1);
                bytes.extend_from_slice(&authority);
            }
        }
        match self.old_code_hash {
            None => bytes.push(0),
            Some(code_hash) => {
                bytes.push(1);
                bytes.extend_from_slice(&code_hash);
            }
        }
        bytes.extend_from_slice(&self.new_code_hash);
        bytes.extend_from_slice(&self.sequence.to_be_bytes());
        bytes.extend_from_slice(&self.observed_at.to_be_bytes());
        bytes.extend_from_slice(
            &u32::try_from(self.module.len())
                .unwrap_or(u32::MAX)
                .to_be_bytes(),
        );
        bytes.extend_from_slice(&self.module);
        match &self.migration {
            None => bytes.push(0),
            Some(execution) => {
                bytes.push(1);
                encode_migration(&mut bytes, execution);
            }
        }
        bytes
    }

    /// Decodes canonical record bytes.
    ///
    /// # Errors
    ///
    /// Refuses a foreign domain, truncated framing, unknown tags, reserved
    /// identifiers and trailing bytes.
    pub fn decode(bytes: &[u8]) -> Result<Self, RegistryError> {
        if bytes.get(..RECORD_DOMAIN.len()) != Some(RECORD_DOMAIN) {
            return Err(RegistryError::CorruptRecord);
        }
        let mut cursor = RECORD_DOMAIN.len();
        let program = ProgramId::new(take_array::<32>(bytes, &mut cursor)?)
            .map_err(|_| RegistryError::CorruptRecord)?;
        let version = u32::from_be_bytes(take_array::<4>(bytes, &mut cursor)?);
        let abi_version = u16::from_be_bytes(take_array::<2>(bytes, &mut cursor)?);
        let upgrade_policy = match take_array::<1>(bytes, &mut cursor)? {
            [0] => UpgradePolicy::Immutable,
            [1] => UpgradePolicy::Authority(take_array::<32>(bytes, &mut cursor)?),
            _ => return Err(RegistryError::CorruptRecord),
        };
        let old_code_hash = match take_array::<1>(bytes, &mut cursor)? {
            [0] => None,
            [1] => Some(take_array::<32>(bytes, &mut cursor)?),
            _ => return Err(RegistryError::CorruptRecord),
        };
        let new_code_hash = take_array::<32>(bytes, &mut cursor)?;
        let sequence = u64::from_be_bytes(take_array::<8>(bytes, &mut cursor)?);
        let observed_at = u64::from_be_bytes(take_array::<8>(bytes, &mut cursor)?);
        let module_bytes =
            usize::try_from(u32::from_be_bytes(take_array::<4>(bytes, &mut cursor)?))
                .map_err(|_| RegistryError::CorruptRecord)?;
        if module_bytes > MAX_MODULE_BYTES {
            return Err(RegistryError::CorruptRecord);
        }
        let module = take_slice(bytes, &mut cursor, module_bytes)?.to_vec();
        let migration = match take_array::<1>(bytes, &mut cursor)? {
            [0] => None,
            [1] => Some(decode_migration(bytes, &mut cursor)?),
            _ => return Err(RegistryError::CorruptRecord),
        };
        if cursor != bytes.len() {
            return Err(RegistryError::CorruptRecord);
        }
        Ok(Self {
            program,
            version,
            abi_version,
            upgrade_policy,
            old_code_hash,
            new_code_hash,
            sequence,
            observed_at,
            module,
            migration,
        })
    }

    /// Returns the digest that names this record in the journal and in the
    /// registry's version history.
    #[must_use]
    pub fn digest(&self) -> [u8; 32] {
        sha256(&self.canonical_encoding())
    }

    /// Rebuilds the immutable program version this record captured.
    #[must_use]
    pub fn program_version(&self) -> ProgramVersion {
        ProgramVersion {
            code_hash: self.new_code_hash,
            wasm: self.module.clone(),
            abi_version: self.abi_version,
        }
    }
}

/// Durable canonical deployment journal maintained by the node boundary.
pub trait DeploymentJournal {
    /// Returns the canonical record bytes named by a deployment digest.
    ///
    /// # Errors
    ///
    /// Refuses absent and unreadable journal entries.
    fn canonical_record(&self, receipt_digest: [u8; 32]) -> Result<Vec<u8>, RegistryError>;

    /// Returns the protocol head last observed by the node boundary.
    ///
    /// # Errors
    ///
    /// Refuses an absent or unreadable head observation.
    fn observed_head(&self) -> Result<ObservedHead, RegistryError>;
}

impl<J> DeploymentJournal for &J
where
    J: DeploymentJournal + ?Sized,
{
    fn canonical_record(&self, receipt_digest: [u8; 32]) -> Result<Vec<u8>, RegistryError> {
        (**self).canonical_record(receipt_digest)
    }

    fn observed_head(&self) -> Result<ObservedHead, RegistryError> {
        (**self).observed_head()
    }
}

/// Production registry-read authority. It answers only from journal evidence
/// it has re-derived itself, and it refuses to answer from an observation
/// older than the declared freshness bound.
#[derive(Clone, Copy, Debug)]
pub struct JournalReadAuthority<J> {
    journal: J,
    now: u64,
    staleness_limit: u64,
}

impl<J: DeploymentJournal> JournalReadAuthority<J> {
    /// Binds the journal to one wall-clock observation and freshness bound.
    ///
    /// # Errors
    ///
    /// Refuses an absent clock reading or an unbounded freshness rule.
    pub fn new(journal: J, now: u64, staleness_limit: u64) -> Result<Self, RegistryError> {
        if now == 0 || staleness_limit == 0 {
            return Err(RegistryError::UnverifiedRead);
        }
        Ok(Self {
            journal,
            now,
            staleness_limit,
        })
    }

    /// Borrows the journal this authority verifies against.
    #[must_use]
    pub const fn journal(&self) -> &J {
        &self.journal
    }

    pub(crate) fn verify_deployment(
        &self,
        program: ProgramId,
        expected: &RegistryVersion,
        expected_policy: UpgradePolicy,
        expected_old_code_hash: Option<[u8; 32]>,
    ) -> Result<VerifiedDeploymentEvidence, RegistryError> {
        let bytes = self
            .journal
            .canonical_record(expected.deployment_receipt_digest)?;
        let record = DeploymentRecord::decode(&bytes)?;
        record.validate()?;
        let receipt_digest = record.digest();
        if receipt_digest != expected.deployment_receipt_digest
            || record.program != program
            || record.version != expected.number
            || record.abi_version != expected.abi_version
            || record.new_code_hash != expected.code_hash
            || record.old_code_hash != expected_old_code_hash
            || record.upgrade_policy != expected_policy
        {
            return Err(RegistryError::UnverifiedRead);
        }
        let head = self.journal.observed_head()?;
        if head.sequence == 0
            || head.observed_at == 0
            || head.sequence < record.sequence
            || head.observed_at < record.observed_at
        {
            return Err(RegistryError::UnverifiedRead);
        }
        if self.now < head.observed_at
            || self.now.saturating_sub(head.observed_at) > self.staleness_limit
        {
            return Err(RegistryError::StaleRead);
        }
        Ok(VerifiedDeploymentEvidence {
            program,
            version: record.version,
            code_hash: record.new_code_hash,
            abi_version: record.abi_version,
            receipt_digest,
            freshness: ReadFreshness {
                observed_sequence: head.sequence,
                observed_at: head.observed_at,
            },
            module: record.module,
            migration: record.migration,
        })
    }
}

impl<J: DeploymentJournal> RegistryReadAuthority for JournalReadAuthority<J> {
    fn verify_registry_read(
        &self,
        program: ProgramId,
        latest: &RegistryVersion,
    ) -> Result<([u8; 32], ReadFreshness), RegistryError> {
        let bytes = self
            .journal
            .canonical_record(latest.deployment_receipt_digest)?;
        let record = DeploymentRecord::decode(&bytes)?;
        record.validate()?;
        let digest = record.digest();
        if digest != latest.deployment_receipt_digest
            || record.program != program
            || record.version != latest.number
            || record.abi_version != latest.abi_version
            || record.new_code_hash != latest.code_hash
        {
            return Err(RegistryError::UnverifiedRead);
        }
        let head = self.journal.observed_head()?;
        if head.sequence == 0 || head.observed_at == 0 || head.sequence < record.sequence {
            return Err(RegistryError::UnverifiedRead);
        }
        if self.now < head.observed_at
            || self.now.saturating_sub(head.observed_at) > self.staleness_limit
        {
            return Err(RegistryError::StaleRead);
        }
        Ok((
            digest,
            ReadFreshness {
                observed_sequence: head.sequence,
                observed_at: head.observed_at,
            },
        ))
    }
}

impl Display for ObservedHead {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            formatter,
            "sequence {} observed at {}",
            self.sequence, self.observed_at
        )
    }
}

fn encode_migration(bytes: &mut Vec<u8>, execution: &ExecutionRecord) {
    bytes.extend_from_slice(&execution.runtime_version.to_be_bytes());
    bytes.extend_from_slice(&execution.abi_version.to_be_bytes());
    bytes.extend_from_slice(
        &u16::try_from(execution.outputs.len())
            .unwrap_or(u16::MAX)
            .to_be_bytes(),
    );
    for output in &execution.outputs {
        match output {
            WasmValue::I32(value) => {
                bytes.push(1);
                bytes.extend_from_slice(&value.to_be_bytes());
            }
            WasmValue::I64(value) => {
                bytes.push(2);
                bytes.extend_from_slice(&value.to_be_bytes());
            }
        }
    }
    bytes.extend_from_slice(&execution.usage.cpu_fuel.to_be_bytes());
    bytes.extend_from_slice(&execution.usage.memory_bytes.to_be_bytes());
    bytes.extend_from_slice(&execution.usage.storage_read_bytes.to_be_bytes());
    bytes.extend_from_slice(&execution.usage.storage_write_bytes.to_be_bytes());
    bytes.extend_from_slice(&execution.usage.output_values.to_be_bytes());
    bytes.extend_from_slice(&execution.usage.fee_units.to_be_bytes());
}

fn decode_migration(bytes: &[u8], cursor: &mut usize) -> Result<ExecutionRecord, RegistryError> {
    let runtime_version = u16::from_be_bytes(take_array::<2>(bytes, cursor)?);
    let abi_version = u16::from_be_bytes(take_array::<2>(bytes, cursor)?);
    let count = usize::from(u16::from_be_bytes(take_array::<2>(bytes, cursor)?));
    if count > MAX_OUTPUTS {
        return Err(RegistryError::CorruptRecord);
    }
    let mut outputs = Vec::with_capacity(count);
    for _ in 0..count {
        let output = match take_array::<1>(bytes, cursor)? {
            [1] => WasmValue::I32(i32::from_be_bytes(take_array::<4>(bytes, cursor)?)),
            [2] => WasmValue::I64(i64::from_be_bytes(take_array::<8>(bytes, cursor)?)),
            _ => return Err(RegistryError::CorruptRecord),
        };
        outputs.push(output);
    }
    Ok(ExecutionRecord {
        runtime_version,
        abi_version,
        outputs,
        usage: MeteredUsage {
            occupancy_byte_batches: 0,
            occupancy_fee_units: 0,
            cpu_fuel: u64::from_be_bytes(take_array::<8>(bytes, cursor)?),
            memory_bytes: u64::from_be_bytes(take_array::<8>(bytes, cursor)?),
            storage_read_bytes: u64::from_be_bytes(take_array::<8>(bytes, cursor)?),
            storage_write_bytes: u64::from_be_bytes(take_array::<8>(bytes, cursor)?),
            output_values: u32::from_be_bytes(take_array::<4>(bytes, cursor)?),
            output_bytes: 0,
            fee_units: u128::from_be_bytes(take_array::<16>(bytes, cursor)?),
        },
    })
}

fn take_array<const N: usize>(bytes: &[u8], cursor: &mut usize) -> Result<[u8; N], RegistryError> {
    take_slice(bytes, cursor, N)?
        .try_into()
        .map_err(|_| RegistryError::CorruptRecord)
}

fn take_slice<'bytes>(
    bytes: &'bytes [u8],
    cursor: &mut usize,
    length: usize,
) -> Result<&'bytes [u8], RegistryError> {
    let end = cursor
        .checked_add(length)
        .ok_or(RegistryError::CorruptRecord)?;
    let slice = bytes
        .get(*cursor..end)
        .ok_or(RegistryError::CorruptRecord)?;
    *cursor = end;
    Ok(slice)
}
