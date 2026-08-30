//! Durable deployment records and local registry-projection integrity reads.
//!
//! Cryptographic deployment authority lives in `protocol_evidence`; this
//! module only rechecks records that production persisted after that boundary.

use core::fmt::{self, Display};

use layerx_programs_runtime::{
    DeploymentReceipt, ExecutionRecord, ExecutionTrace, MeteredUsage, ProgramId, ProgramVersion,
    UpgradePolicy, WasmValue,
};

use crate::hash::sha256;
use crate::{ReadFreshness, RegistryError, RegistryReadAuthority, RegistryVersion};

const RECORD_DOMAIN: &[u8] = b"LayerX/programs/registry/deployment/v1\0";
const MIGRATION_EVIDENCE_DOMAIN: &[u8] = b"LayerX/programs/registry/migration-execution/v2\0";
const MAX_MODULE_BYTES: usize = 32 * 1024 * 1024;
const MAX_OUTPUTS: usize = 256;

/// Authenticated, canonical migration execution evidence retained by the
/// durable registry record. The arbitration trace remains its exact canonical
/// evidence bytes because that receipt format intentionally omits runtime-only
/// execution witnesses and therefore cannot be losslessly reconstructed as an
/// [`ExecutionTrace`].
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct MigrationExecutionEvidence {
    runtime_version: u16,
    abi_version: u16,
    metering_schedule_version: u32,
    outputs: Vec<WasmValue>,
    usage: MeteredUsage,
    trace_evidence: Option<Vec<u8>>,
    canonical_bytes: Vec<u8>,
    canonical_length: u32,
    commitment: [u8; 32],
}

impl MigrationExecutionEvidence {
    /// Freezes a runtime migration result into complete durable evidence.
    ///
    /// # Errors
    ///
    /// Refuses incomplete versions, inconsistent output accounting, oversized
    /// outputs, and traces that are not canonical arbitration evidence.
    pub fn from_execution(execution: &ExecutionRecord) -> Result<Self, RegistryError> {
        let trace_evidence = execution
            .trace
            .as_ref()
            .map(ExecutionTrace::canonical_arbitration_bytes)
            .transpose()
            .map_err(|_| RegistryError::CorruptRecord)?;
        Self::from_parts(
            execution.runtime_version,
            execution.abi_version,
            execution.metering_schedule_version,
            execution.outputs.clone(),
            execution.usage,
            trace_evidence,
        )
    }

    fn from_parts(
        runtime_version: u16,
        abi_version: u16,
        metering_schedule_version: u32,
        outputs: Vec<WasmValue>,
        usage: MeteredUsage,
        trace_evidence: Option<Vec<u8>>,
    ) -> Result<Self, RegistryError> {
        if runtime_version == 0
            || abi_version == 0
            || metering_schedule_version == 0
            || outputs.len() > MAX_OUTPUTS
            || usize::try_from(usage.output_values).ok() != Some(outputs.len())
        {
            return Err(RegistryError::CorruptRecord);
        }
        if let Some(trace) = &trace_evidence {
            ExecutionTrace::verify_canonical_arbitration_bytes(trace)
                .map_err(|_| RegistryError::CorruptRecord)?;
        }
        let canonical_bytes = encode_migration_evidence(
            runtime_version,
            abi_version,
            metering_schedule_version,
            &outputs,
            usage,
            trace_evidence.as_deref(),
        )?;
        let canonical_length =
            u32::try_from(canonical_bytes.len()).map_err(|_| RegistryError::CorruptRecord)?;
        let commitment = sha256(&canonical_bytes);
        Ok(Self {
            runtime_version,
            abi_version,
            metering_schedule_version,
            outputs,
            usage,
            trace_evidence,
            canonical_bytes,
            canonical_length,
            commitment,
        })
    }

    #[must_use]
    pub const fn runtime_version(&self) -> u16 {
        self.runtime_version
    }
    #[must_use]
    pub const fn abi_version(&self) -> u16 {
        self.abi_version
    }
    #[must_use]
    pub const fn metering_schedule_version(&self) -> u32 {
        self.metering_schedule_version
    }
    #[must_use]
    pub fn outputs(&self) -> &[WasmValue] {
        &self.outputs
    }
    #[must_use]
    pub const fn usage(&self) -> MeteredUsage {
        self.usage
    }
    #[must_use]
    pub fn trace_evidence(&self) -> Option<&[u8]> {
        self.trace_evidence.as_deref()
    }
    #[must_use]
    pub fn canonical_bytes(&self) -> &[u8] {
        &self.canonical_bytes
    }
    #[must_use]
    pub const fn commitment(&self) -> [u8; 32] {
        self.commitment
    }
}

impl TryFrom<&ExecutionRecord> for MigrationExecutionEvidence {
    type Error = RegistryError;

    fn try_from(execution: &ExecutionRecord) -> Result<Self, Self::Error> {
        Self::from_execution(execution)
    }
}

/// The protocol head last observed by the node boundary that feeds the
/// journal. Reads carry it so every answer states exactly how fresh it is.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ObservedHead {
    pub sequence: u64,
    pub observed_at: u64,
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
    pub migration: Option<MigrationExecutionEvidence>,
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
            migration: receipt
                .migration()
                .map(MigrationExecutionEvidence::from_execution)
                .transpose()?,
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
        if matches!(
            self.upgrade_policy,
            UpgradePolicy::Authority(authority) if authority == [0; 32]
        ) {
            return Err(RegistryError::InvalidUpgradeAuthority);
        }
        if self.version == 0
            || !matches!(
                self.abi_version,
                layerx_programs_runtime::ABI_V1_VERSION | layerx_programs_runtime::ABI_V2_VERSION
            )
            || self.new_code_hash == [0; 32]
            || self.sequence == 0
            || self.observed_at == 0
            || self.module.is_empty()
            || self.module.len() > MAX_MODULE_BYTES
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
            Some(evidence) => {
                bytes.push(2);
                bytes.extend_from_slice(&evidence.canonical_length.to_be_bytes());
                bytes.extend_from_slice(&evidence.canonical_bytes);
                bytes.extend_from_slice(&evidence.commitment);
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
            [1] => return Err(RegistryError::CorruptRecord),
            [2] => Some(decode_migration_evidence(bytes, &mut cursor)?),
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

/// Durable deployment-record store used for projection integrity. This trait
/// carries no cryptographic authority and cannot produce executable evidence.
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

/// Local registry-projection integrity reader. Production ingestion writes
/// these records only after protocol evidence verification; this reader does
/// not turn a caller-implemented journal into receipt authority.
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
        if record.program != program
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
            latest.deployment_receipt_digest,
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

fn encode_migration_evidence(
    runtime_version: u16,
    abi_version: u16,
    metering_schedule_version: u32,
    outputs: &[WasmValue],
    usage: MeteredUsage,
    trace_evidence: Option<&[u8]>,
) -> Result<Vec<u8>, RegistryError> {
    let output_count = u16::try_from(outputs.len()).map_err(|_| RegistryError::CorruptRecord)?;
    let mut bytes = Vec::new();
    bytes.extend_from_slice(MIGRATION_EVIDENCE_DOMAIN);
    bytes.extend_from_slice(&runtime_version.to_be_bytes());
    bytes.extend_from_slice(&abi_version.to_be_bytes());
    bytes.extend_from_slice(&metering_schedule_version.to_be_bytes());
    bytes.extend_from_slice(&output_count.to_be_bytes());
    for output in outputs {
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
    bytes.extend_from_slice(&usage.cpu_fuel.to_be_bytes());
    bytes.extend_from_slice(&usage.memory_bytes.to_be_bytes());
    bytes.extend_from_slice(&usage.storage_read_bytes.to_be_bytes());
    bytes.extend_from_slice(&usage.storage_write_bytes.to_be_bytes());
    bytes.extend_from_slice(&usage.output_values.to_be_bytes());
    bytes.extend_from_slice(&usage.output_bytes.to_be_bytes());
    bytes.extend_from_slice(&usage.occupancy_byte_batches.to_be_bytes());
    bytes.extend_from_slice(&usage.occupancy_fee_units.to_be_bytes());
    bytes.extend_from_slice(&usage.fee_units.to_be_bytes());
    match trace_evidence {
        None => bytes.push(0),
        Some(trace) => {
            ExecutionTrace::verify_canonical_arbitration_bytes(trace)
                .map_err(|_| RegistryError::CorruptRecord)?;
            let trace_len = u32::try_from(trace.len()).map_err(|_| RegistryError::CorruptRecord)?;
            bytes.push(1);
            bytes.extend_from_slice(&trace_len.to_be_bytes());
            bytes.extend_from_slice(trace);
        }
    }
    Ok(bytes)
}

fn decode_migration_evidence(
    bytes: &[u8],
    cursor: &mut usize,
) -> Result<MigrationExecutionEvidence, RegistryError> {
    let evidence_len = usize::try_from(u32::from_be_bytes(take_array::<4>(bytes, cursor)?))
        .map_err(|_| RegistryError::CorruptRecord)?;
    let evidence_bytes = take_slice(bytes, cursor, evidence_len)?;
    let expected_commitment = take_array::<32>(bytes, cursor)?;
    if sha256(evidence_bytes) != expected_commitment
        || evidence_bytes.get(..MIGRATION_EVIDENCE_DOMAIN.len()) != Some(MIGRATION_EVIDENCE_DOMAIN)
    {
        return Err(RegistryError::CorruptRecord);
    }
    let mut evidence_cursor = MIGRATION_EVIDENCE_DOMAIN.len();
    let runtime_version =
        u16::from_be_bytes(take_array::<2>(evidence_bytes, &mut evidence_cursor)?);
    let abi_version = u16::from_be_bytes(take_array::<2>(evidence_bytes, &mut evidence_cursor)?);
    let metering_schedule_version =
        u32::from_be_bytes(take_array::<4>(evidence_bytes, &mut evidence_cursor)?);
    let count = usize::from(u16::from_be_bytes(take_array::<2>(
        evidence_bytes,
        &mut evidence_cursor,
    )?));
    if count > MAX_OUTPUTS {
        return Err(RegistryError::CorruptRecord);
    }
    let mut outputs = Vec::with_capacity(count);
    for _ in 0..count {
        let output = match take_array::<1>(evidence_bytes, &mut evidence_cursor)? {
            [1] => WasmValue::I32(i32::from_be_bytes(take_array::<4>(
                evidence_bytes,
                &mut evidence_cursor,
            )?)),
            [2] => WasmValue::I64(i64::from_be_bytes(take_array::<8>(
                evidence_bytes,
                &mut evidence_cursor,
            )?)),
            _ => return Err(RegistryError::CorruptRecord),
        };
        outputs.push(output);
    }
    let usage = MeteredUsage {
        cpu_fuel: u64::from_be_bytes(take_array::<8>(evidence_bytes, &mut evidence_cursor)?),
        memory_bytes: u64::from_be_bytes(take_array::<8>(evidence_bytes, &mut evidence_cursor)?),
        storage_read_bytes: u64::from_be_bytes(take_array::<8>(
            evidence_bytes,
            &mut evidence_cursor,
        )?),
        storage_write_bytes: u64::from_be_bytes(take_array::<8>(
            evidence_bytes,
            &mut evidence_cursor,
        )?),
        output_values: u32::from_be_bytes(take_array::<4>(evidence_bytes, &mut evidence_cursor)?),
        output_bytes: u64::from_be_bytes(take_array::<8>(evidence_bytes, &mut evidence_cursor)?),
        occupancy_byte_batches: u128::from_be_bytes(take_array::<16>(
            evidence_bytes,
            &mut evidence_cursor,
        )?),
        occupancy_fee_units: u128::from_be_bytes(take_array::<16>(
            evidence_bytes,
            &mut evidence_cursor,
        )?),
        fee_units: u128::from_be_bytes(take_array::<16>(evidence_bytes, &mut evidence_cursor)?),
    };
    let trace_evidence = match take_array::<1>(evidence_bytes, &mut evidence_cursor)? {
        [0] => None,
        [1] => {
            let len = usize::try_from(u32::from_be_bytes(take_array::<4>(
                evidence_bytes,
                &mut evidence_cursor,
            )?))
            .map_err(|_| RegistryError::CorruptRecord)?;
            Some(take_slice(evidence_bytes, &mut evidence_cursor, len)?.to_vec())
        }
        _ => return Err(RegistryError::CorruptRecord),
    };
    if evidence_cursor != evidence_bytes.len() {
        return Err(RegistryError::CorruptRecord);
    }
    let evidence = MigrationExecutionEvidence::from_parts(
        runtime_version,
        abi_version,
        metering_schedule_version,
        outputs,
        usage,
        trace_evidence,
    )?;
    if evidence.canonical_bytes != evidence_bytes || evidence.commitment != expected_commitment {
        return Err(RegistryError::CorruptRecord);
    }
    Ok(evidence)
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

#[cfg(test)]
mod tests {
    use super::*;
    use layerx_programs_runtime::test_support::{
        code_section, export_section, func_body, function_section, module, raw_section,
        type_section, OP_CALL, OP_DROP, OP_END, OP_I32_ADD, OP_I32_CONST, OP_LOCAL_GET, TYPE_I32,
    };
    use layerx_programs_runtime::{
        Executor, FeeSchedule, ResourceBudget, TracePolicy, WasmEngine, MAX_TRACE_STATE_BYTES,
        STEP_COMMITMENT_BASE_FUEL, STEP_COMMITMENT_FUEL_PER_BYTE,
    };

    fn traced_execution() -> ExecutionRecord {
        let memory_section = raw_section(5, &[1, 0, 1]);
        let global_section = raw_section(
            6,
            &[
                2,
                TYPE_I32,
                1,
                OP_I32_CONST,
                0,
                OP_END,
                TYPE_I32,
                0,
                OP_I32_CONST,
                9,
                OP_END,
            ],
        );
        let wasm = module(&[
            type_section(&[(&[TYPE_I32], &[TYPE_I32]), (&[TYPE_I32], &[TYPE_I32])]),
            function_section(&[0, 1]),
            memory_section,
            global_section,
            export_section(&[("run", 1)]),
            code_section(&[
                func_body(
                    &[(1, TYPE_I32)],
                    &[
                        OP_LOCAL_GET,
                        0,
                        OP_I32_CONST,
                        2,
                        OP_I32_ADD,
                        0x22,
                        1,
                        OP_LOCAL_GET,
                        1,
                        OP_I32_ADD,
                        OP_END,
                    ],
                ),
                func_body(
                    &[],
                    &[
                        OP_I32_CONST,
                        1,
                        0x40,
                        0,
                        OP_DROP,
                        OP_I32_CONST,
                        0,
                        OP_LOCAL_GET,
                        0,
                        0x36,
                        2,
                        0,
                        OP_LOCAL_GET,
                        0,
                        0x24,
                        0,
                        OP_LOCAL_GET,
                        0,
                        OP_CALL,
                        0,
                        OP_END,
                    ],
                ),
            ]),
        ]);
        let engine = WasmEngine::declared().expect("declared engine");
        let validated = engine.validate(&wasm).expect("valid traced module");
        let trace_policy = TracePolicy::new(3, 256).expect("valid trace policy");
        let declared = ResourceBudget::declared();
        let trace_state_fuel = MAX_TRACE_STATE_BYTES
            .checked_mul(STEP_COMMITMENT_FUEL_PER_BYTE)
            .and_then(|fuel| {
                fuel.checked_add(
                    u64::from(trace_policy.maximum_commitments()) * STEP_COMMITMENT_BASE_FUEL,
                )
            })
            .and_then(|fuel| fuel.checked_mul(2))
            .expect("protocol trace bounds fit fuel accounting");
        let budget = ResourceBudget::new_complete(
            declared
                .cpu_fuel()
                .checked_add(trace_state_fuel)
                .expect("declared trace budget fits"),
            declared.memory_bytes(),
            declared.storage_read_bytes(),
            declared.storage_write_bytes(),
            declared.output_values(),
            declared.output_bytes(),
            declared.table_elements(),
        );
        Executor::new(budget, FeeSchedule::declared())
            .with_trace_policy(trace_policy)
            .execute_traced(&validated, "run", &[WasmValue::I32(7)])
            .expect("real traced execution")
            .execution
    }

    fn migration_evidence() -> MigrationExecutionEvidence {
        MigrationExecutionEvidence::from_parts(
            7,
            layerx_programs_runtime::ABI_V2_VERSION,
            11,
            vec![WasmValue::I32(-4), WasmValue::I64(9)],
            MeteredUsage {
                cpu_fuel: 101,
                memory_bytes: 102,
                storage_read_bytes: 103,
                storage_write_bytes: 104,
                output_values: 2,
                output_bytes: 105,
                occupancy_byte_batches: 106,
                occupancy_fee_units: 107,
                fee_units: 108,
            },
            None,
        )
        .expect("valid migration execution evidence")
    }

    fn deployment_record() -> DeploymentRecord {
        let module = vec![1, 2, 3];
        DeploymentRecord {
            program: ProgramId::new([1; 32]).expect("nonzero program"),
            version: 2,
            abi_version: layerx_programs_runtime::ABI_V2_VERSION,
            upgrade_policy: UpgradePolicy::Authority([2; 32]),
            old_code_hash: Some([3; 32]),
            new_code_hash: sha256(&module),
            sequence: 9,
            observed_at: 10,
            module,
            migration: Some(migration_evidence()),
        }
    }

    #[test]
    fn version_two_migration_evidence_round_trips_every_metered_field() {
        let record = deployment_record();
        let encoded = record.canonical_encoding();
        let decoded = DeploymentRecord::decode(&encoded).expect("record decodes");

        assert_eq!(decoded, record);
        let evidence = decoded.migration.expect("migration evidence retained");
        assert_eq!(evidence.runtime_version(), 7);
        assert_eq!(evidence.metering_schedule_version(), 11);
        assert_eq!(evidence.outputs(), [WasmValue::I32(-4), WasmValue::I64(9)]);
        assert_eq!(evidence.usage().output_bytes, 105);
        assert_eq!(evidence.usage().occupancy_byte_batches, 106);
        assert_eq!(evidence.usage().occupancy_fee_units, 107);
        assert_eq!(evidence.commitment(), sha256(evidence.canonical_bytes()));
    }

    #[test]
    fn real_traced_execution_is_retained_exactly_and_trace_mutation_is_rejected() {
        let execution = traced_execution();
        let expected_trace = execution
            .trace
            .as_ref()
            .expect("execution contains trace")
            .canonical_arbitration_bytes()
            .expect("trace evidence encodes");
        let evidence = MigrationExecutionEvidence::from_execution(&execution)
            .expect("traced execution is admitted");
        assert_eq!(evidence.trace_evidence(), Some(expected_trace.as_slice()));

        let mut record = deployment_record();
        record.migration = Some(evidence);
        let encoded = record.canonical_encoding();
        let decoded = DeploymentRecord::decode(&encoded).expect("traced record round trips");
        assert_eq!(
            decoded
                .migration
                .as_ref()
                .and_then(MigrationExecutionEvidence::trace_evidence),
            Some(expected_trace.as_slice()),
        );

        let trace_offset = encoded
            .windows(expected_trace.len())
            .position(|window| window == expected_trace)
            .expect("trace occurs in canonical record");
        let mut mutated = encoded;
        mutated[trace_offset] ^= 1;
        assert_eq!(
            DeploymentRecord::decode(&mutated),
            Err(RegistryError::CorruptRecord),
        );
    }

    #[test]
    fn migration_evidence_mutation_and_truncation_fail_closed() {
        let record = deployment_record();
        let encoded = record.canonical_encoding();
        let evidence_len = record
            .migration
            .as_ref()
            .expect("migration evidence")
            .canonical_bytes()
            .len();
        let migration_tag = encoded.len() - 32 - evidence_len - 4 - 1;

        let mut mutated = encoded.clone();
        mutated[migration_tag + 5] ^= 1;
        assert_eq!(
            DeploymentRecord::decode(&mutated),
            Err(RegistryError::CorruptRecord)
        );

        assert_eq!(
            DeploymentRecord::decode(&encoded[..encoded.len() - 1]),
            Err(RegistryError::CorruptRecord),
        );
    }

    #[test]
    fn legacy_migration_record_without_complete_evidence_is_rejected() {
        let record = deployment_record();
        let mut encoded = record.canonical_encoding();
        let evidence_len = record
            .migration
            .as_ref()
            .expect("migration evidence")
            .canonical_bytes()
            .len();
        let migration_tag = encoded.len() - 32 - evidence_len - 4 - 1;
        encoded[migration_tag] = 1;

        assert_eq!(
            DeploymentRecord::decode(&encoded),
            Err(RegistryError::CorruptRecord)
        );
    }
}
