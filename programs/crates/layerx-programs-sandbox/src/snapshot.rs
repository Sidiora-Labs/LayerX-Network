//! Canonical renter-authorized sandbox snapshots and prepared restoration.

use core::fmt::{self, Display};

use layerx_programs_runtime::{hash_bytes, Abi, AuthorizationContext, HashAlgorithm, MeterRefusal,
    ExecutionFault, PrincipalId, ProgramInstance, RuntimeContinuation, RuntimeGlobal, Storage,
    StorageError, UnavailableReceiptOracle, ValidatedModule, WasmValue, ABI_V1_VERSION, ABI_V2_VERSION};

use crate::{EphemeralNamespace, Lease, LeaseActivity, LeaseId, LeaseRefusal, LeaseState,
    LeaseTransition, TransitionEvidence};

const SNAPSHOT_DOMAIN: &[u8] = b"LayerX/programs/sandbox/snapshot/v1\0";
const SNAPSHOT_KEY_DOMAIN: &[u8] = b"/snapshot/v1/";
const SNAPSHOT_CHUNK_BYTES: usize = 65_536;
pub const MAX_SANDBOX_LINEAR_MEMORY_BYTES: usize = 1 << 30;
pub const MAX_SANDBOX_GLOBALS: usize = 65_536;
pub const MAX_SANDBOX_OPERAND_STACK: usize = 65_536;
pub const MAX_SANDBOX_NAMESPACE_CELLS: usize = 1_048_576;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ContinuationPoint {
    pub entrypoint: String,
    pub arguments: Vec<WasmValue>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct NamespaceCell {
    key: Vec<u8>,
    value: Vec<u8>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SandboxState {
    source_lease: LeaseId,
    source_namespace: EphemeralNamespace,
    linear_memory: Vec<u8>,
    globals: Vec<RuntimeGlobal>,
    continuation: ContinuationPoint,
    namespace_cells: Vec<NamespaceCell>,
}

impl SandboxState {
    fn new(
        lease: &Lease,
        linear_memory: Vec<u8>,
        globals: Vec<RuntimeGlobal>,
        continuation: ContinuationPoint,
        namespace_cells: Vec<NamespaceCell>,
    ) -> Result<Self, SnapshotRefusal> {
        let state = Self { source_lease: lease.id(), source_namespace: lease.namespace(),
            linear_memory, globals, continuation, namespace_cells };
        state.validate()?;
        Ok(state)
    }

    fn from_runtime(
        lease: &Lease, runtime: RuntimeContinuation,
    ) -> Result<Self, SnapshotRefusal> {
        Self::new(lease, runtime.linear_memory, runtime.globals,
            ContinuationPoint { entrypoint: runtime.entrypoint, arguments: runtime.arguments },
            Vec::new())
    }

    pub fn from_canonical(lease: &Lease, bytes: &[u8]) -> Result<Self, SnapshotRefusal> {
        let mut cursor = SnapshotCursor::new(bytes);
        if cursor.take(SNAPSHOT_DOMAIN.len())? != SNAPSHOT_DOMAIN
            || cursor.array::<32>()? != lease.id().bytes()
            || cursor.array::<32>()? != lease.host_program().bytes()
            || cursor.array::<32>()? != lease.namespace().bytes() {
            return Err(SnapshotRefusal::StateLeaseMismatch);
        }
        let linear_memory = cursor.bytes()?;
        let globals_count = cursor.length()?;
        let mut globals = Vec::with_capacity(globals_count);
        for _ in 0..globals_count {
            let name = String::from_utf8(cursor.bytes()?)
                .map_err(|_| SnapshotRefusal::NonCanonicalContinuation)?;
            globals.push(RuntimeGlobal { name, value: cursor.value()? });
        }
        let entrypoint = String::from_utf8(cursor.bytes()?)
            .map_err(|_| SnapshotRefusal::NonCanonicalContinuation)?;
        let argument_count = cursor.length()?;
        let mut arguments = Vec::with_capacity(argument_count);
        for _ in 0..argument_count { arguments.push(cursor.value()?); }
        let cell_count = cursor.length()?;
        let mut namespace_cells = Vec::with_capacity(cell_count);
        for _ in 0..cell_count {
            namespace_cells.push(NamespaceCell { key: cursor.bytes()?, value: cursor.bytes()? });
        }
        if !cursor.is_empty() { return Err(SnapshotRefusal::NonCanonicalContinuation); }
        let state = Self::new(lease, linear_memory, globals,
            ContinuationPoint { entrypoint, arguments }, namespace_cells)?;
        if state.canonical_bytes()? != bytes { return Err(SnapshotRefusal::NonCanonicalContinuation); }
        Ok(state)
    }

    #[must_use] pub const fn source_lease(&self) -> LeaseId { self.source_lease }
    #[must_use] pub const fn source_namespace(&self) -> EphemeralNamespace { self.source_namespace }
    #[must_use] pub fn linear_memory(&self) -> &[u8] { &self.linear_memory }
    #[must_use] pub fn globals(&self) -> &[RuntimeGlobal] { &self.globals }
    #[must_use] pub const fn continuation(&self) -> &ContinuationPoint { &self.continuation }
    #[must_use] pub fn namespace_cells(&self) -> &[NamespaceCell] { &self.namespace_cells }

    fn validate(&self) -> Result<(), SnapshotRefusal> {
        if self.linear_memory.len() > MAX_SANDBOX_LINEAR_MEMORY_BYTES
            || self.globals.len() > MAX_SANDBOX_GLOBALS
            || self.continuation.arguments.len() > MAX_SANDBOX_OPERAND_STACK
            || self.namespace_cells.len() > MAX_SANDBOX_NAMESPACE_CELLS {
            return Err(SnapshotRefusal::StateBoundExceeded);
        }
        let mut prior: Option<&[u8]> = None;
        for cell in &self.namespace_cells {
            if cell.key.is_empty() || prior.is_some_and(|key| key >= cell.key.as_slice()) {
                return Err(SnapshotRefusal::NonCanonicalNamespace);
            }
            prior = Some(&cell.key);
        }
        if self.continuation.entrypoint.is_empty()
            || self.globals.windows(2).any(|pair| pair[0].name >= pair[1].name)
            || self.globals.iter().any(|global| global.name.is_empty()) {
            return Err(SnapshotRefusal::NonCanonicalContinuation);
        }
        Ok(())
    }

    fn validate_against(&self, lease: &Lease) -> Result<(), SnapshotRefusal> {
        self.validate()?;
        let memory_bytes = u64::try_from(self.linear_memory.len())
            .map_err(|_| SnapshotRefusal::StateBoundExceeded)?;
        let namespace_bytes = self.namespace_cells.iter().try_fold(0u64, |total, cell| {
            let key = u64::try_from(cell.key.len()).map_err(|_| SnapshotRefusal::StateBoundExceeded)?;
            let value = u64::try_from(cell.value.len()).map_err(|_| SnapshotRefusal::StateBoundExceeded)?;
            total.checked_add(key).and_then(|sum| sum.checked_add(value))
                .ok_or(SnapshotRefusal::StateBoundExceeded)
        })?;
        if memory_bytes > lease.limits().memory_bytes
            || namespace_bytes > lease.limits().namespace_bytes {
            return Err(SnapshotRefusal::TargetBoundExceeded);
        }
        Ok(())
    }

    pub fn canonical_bytes(&self) -> Result<Vec<u8>, SnapshotRefusal> {
        self.validate()?;
        let mut output = Vec::new();
        output.extend_from_slice(SNAPSHOT_DOMAIN);
        output.extend_from_slice(&self.source_lease.bytes());
        output.extend_from_slice(&self.source_namespace.host().bytes());
        output.extend_from_slice(&self.source_namespace.bytes());
        put_bytes(&mut output, &self.linear_memory)?;
        put_len(&mut output, self.globals.len())?;
        for global in &self.globals {
            put_bytes(&mut output, global.name.as_bytes())?;
            encode_value(global.value, &mut output);
        }
        put_bytes(&mut output, self.continuation.entrypoint.as_bytes())?;
        put_len(&mut output, self.continuation.arguments.len())?;
        for value in &self.continuation.arguments { encode_value(*value, &mut output); }
        put_len(&mut output, self.namespace_cells.len())?;
        for cell in &self.namespace_cells {
            put_bytes(&mut output, &cell.key)?;
            put_bytes(&mut output, &cell.value)?;
        }
        Ok(output)
    }

    pub fn digest(&self) -> Result<[u8; 32], SnapshotRefusal> {
        hash_bytes(HashAlgorithm::Sha256, &self.canonical_bytes()?)
            .map_err(|_| SnapshotRefusal::HashRefusal)
    }

    fn rebind(mut self, target: &Lease) -> Self {
        self.source_lease = target.id();
        self.source_namespace = target.namespace();
        self
    }


    #[must_use]
    pub fn runtime_continuation(&self) -> RuntimeContinuation {
        RuntimeContinuation { linear_memory: self.linear_memory.clone(), globals: self.globals.clone(),
            entrypoint: self.continuation.entrypoint.clone(),
            arguments: self.continuation.arguments.clone() }
    }
}

struct SnapshotCursor<'a> { bytes: &'a [u8], offset: usize }
impl<'a> SnapshotCursor<'a> {
    const fn new(bytes: &'a [u8]) -> Self { Self { bytes, offset: 0 } }
    fn take(&mut self, length: usize) -> Result<&'a [u8], SnapshotRefusal> {
        let end = self.offset.checked_add(length).ok_or(SnapshotRefusal::StateBoundExceeded)?;
        let value = self.bytes.get(self.offset..end).ok_or(SnapshotRefusal::NonCanonicalContinuation)?;
        self.offset = end; Ok(value)
    }
    fn array<const N: usize>(&mut self) -> Result<[u8; N], SnapshotRefusal> {
        self.take(N)?.try_into().map_err(|_| SnapshotRefusal::NonCanonicalContinuation)
    }
    fn length(&mut self) -> Result<usize, SnapshotRefusal> {
        usize::try_from(u64::from_be_bytes(self.array()?)).map_err(|_| SnapshotRefusal::StateBoundExceeded)
    }
    fn bytes(&mut self) -> Result<Vec<u8>, SnapshotRefusal> {
        let length = self.length()?; Ok(self.take(length)?.to_vec())
    }
    fn value(&mut self) -> Result<WasmValue, SnapshotRefusal> {
        match self.array::<1>()?[0] {
            0 => Ok(WasmValue::I32(i32::from_be_bytes(self.array()?))),
            1 => Ok(WasmValue::I64(i64::from_be_bytes(self.array()?))),
            _ => Err(SnapshotRefusal::NonCanonicalContinuation),
        }
    }
    fn is_empty(&self) -> bool { self.offset == self.bytes.len() }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Snapshot {
    digest: [u8; 32],
    owner: PrincipalId,
    source_lease: LeaseId,
    source_namespace: EphemeralNamespace,
    host_program: [u8; 32],
    image_code_hash: [u8; 32],
    byte_length: u64,
    storage_bytes: u64,
    state: SandboxState,
}

#[derive(Debug)]
pub struct CapturedSnapshot {
    state: SandboxState,
    validated_code_hash: [u8; 32],
}

impl CapturedSnapshot {
    #[must_use] pub const fn state(&self) -> &SandboxState { &self.state }
    pub fn digest(&self) -> Result<[u8; 32], SnapshotRefusal> { self.state.digest() }
}

impl Snapshot {
    pub fn capture(
        lease: &Lease, instance: &mut ProgramInstance, entrypoint: &str, arguments: &[WasmValue],
    ) -> Result<CapturedSnapshot, SnapshotRefusal> {
        if lease.state() != LeaseState::Active { return Err(SnapshotRefusal::LeaseNotActive); }
        if instance.validated_code_hash() != lease.image_code_hash() {
            return Err(SnapshotRefusal::IncompatibleTarget);
        }
        let runtime = instance.capture_continuation(entrypoint, arguments)
            .map_err(SnapshotRefusal::Runtime)?;
        let mut state = SandboxState::from_runtime(lease, runtime)?;
        let storage = instance.storage_snapshot().ok_or(SnapshotRefusal::MissingRuntimeStorage)?;
        state.namespace_cells = live_namespace_cells(&storage, lease.namespace())?;
        state.validate_against(lease)?;
        Ok(CapturedSnapshot { state, validated_code_hash: instance.validated_code_hash() })
    }

    pub fn commit(
        lease: &mut Lease, instance: &mut ProgramInstance, capture: CapturedSnapshot,
        transition: LeaseTransition, evidence: TransitionEvidence,
    ) -> Result<Self, SnapshotRefusal> {
        if lease.state() != LeaseState::Active { return Err(SnapshotRefusal::LeaseNotActive); }
        if instance.validated_code_hash() != lease.image_code_hash()
            || capture.validated_code_hash != instance.validated_code_hash() {
            return Err(SnapshotRefusal::IncompatibleTarget);
        }
        let state = capture.state;
        if state.source_lease != lease.id() || state.source_namespace != lease.namespace() {
            return Err(SnapshotRefusal::StateLeaseMismatch);
        }
        state.validate_against(lease)?;
        let captured = state;
        captured.validate_against(lease)?;
        let bytes = captured.canonical_bytes()?;
        let byte_length = u64::try_from(bytes.len()).map_err(|_| SnapshotRefusal::StateBoundExceeded)?;
        let digest = hash_bytes(HashAlgorithm::Sha256, &bytes)
            .map_err(|_| SnapshotRefusal::HashRefusal)?;
        if transition.activity != LeaseActivity::Snapshot || transition.from != LeaseState::Active
            || transition.to != LeaseState::Active || transition.usage_observation_digest != digest {
            return Err(SnapshotRefusal::InvalidSnapshotEvidence);
        }
        let entries = snapshot_entries(digest, &bytes)?;
        let chunk_count = u32::try_from(entries.len().saturating_sub(1))
            .map_err(|_| SnapshotRefusal::StateBoundExceeded)?;
        let mut candidate = lease.clone();
        candidate.snapshot_transition(transition, evidence).map_err(SnapshotRefusal::Lease)?;
        candidate.bind_snapshot(digest, lease.tenant(), byte_length, chunk_count)
            .map_err(SnapshotRefusal::Lease)?;
        let mut candidate_storage = instance.storage_snapshot().ok_or(SnapshotRefusal::MissingRuntimeStorage)?;
        candidate_storage.replace_protocol_prefix(lease.namespace().snapshot_storage_namespace(),
            &snapshot_prefix(digest), &entries)
            .map_err(SnapshotRefusal::Storage)?;
        verify_persisted_snapshot(&candidate_storage, lease.namespace(), digest, &bytes)?;
        let occupied = candidate_storage.namespace_persistent_bytes(lease.namespace().storage_namespace()
            .map_err(SnapshotRefusal::Lease)?).map_err(SnapshotRefusal::Storage)?
            .checked_add(candidate_storage.protocol_prefix_bytes(lease.namespace().snapshot_storage_namespace(), b"snapshot")
                .map_err(SnapshotRefusal::Storage)?).ok_or(SnapshotRefusal::StateBoundExceeded)?;
        if occupied > lease.limits().namespace_bytes { return Err(SnapshotRefusal::TargetBoundExceeded); }
        let storage_bytes = entries_metered_bytes(&entries)?;
        instance.commit_snapshot_storage(candidate_storage, storage_bytes)
            .map_err(SnapshotRefusal::Runtime)?;
        *lease = candidate;
        Ok(Self { digest, owner: lease.tenant(), source_lease: lease.id(),
            source_namespace: lease.namespace(), host_program: lease.host_program().bytes(),
            image_code_hash: lease.image_code_hash(), byte_length, storage_bytes, state: captured })
    }

    #[must_use] pub const fn digest(&self) -> [u8; 32] { self.digest }
    #[must_use] pub const fn owner(&self) -> PrincipalId { self.owner }
    #[must_use] pub const fn source_lease(&self) -> LeaseId { self.source_lease }
    #[must_use] pub const fn byte_length(&self) -> u64 { self.byte_length }
    #[must_use] pub const fn storage_bytes(&self) -> u64 { self.storage_bytes }
    #[must_use] pub const fn state(&self) -> &SandboxState { &self.state }
}

#[derive(Debug)]
pub struct RestoredSandbox {
    target_lease: LeaseId,
    snapshot_digest: [u8; 32],
    state: SandboxState,
    instance: ProgramInstance,
    outputs: Vec<WasmValue>,
}

impl RestoredSandbox {
    #[must_use] pub const fn target_lease(&self) -> LeaseId { self.target_lease }
    #[must_use] pub const fn snapshot_digest(&self) -> [u8; 32] { self.snapshot_digest }
    #[must_use] pub const fn state(&self) -> &SandboxState { &self.state }
    #[must_use] pub fn instance(&self) -> &ProgramInstance { &self.instance }
    #[must_use] pub fn outputs(&self) -> &[WasmValue] { &self.outputs }
}

pub fn restore(
    source: &Lease, target: &mut Lease, storage: &mut Storage, snapshot_digest: [u8; 32],
    supplied: SandboxState, authorization: AuthorizationContext, meter: &mut layerx_programs_runtime::Meter,
    module: &ValidatedModule, activation: LeaseTransition, evidence: TransitionEvidence,
) -> Result<RestoredSandbox, SnapshotRefusal> {
    let record = source.snapshot_records().iter().find(|record| record.digest() == snapshot_digest)
        .ok_or(SnapshotRefusal::UnknownSnapshot)?;
    if target.state() != LeaseState::Funded { return Err(SnapshotRefusal::TargetNotFunded); }
    if authorization.principal() != record.owner() || authorization.principal() != target.tenant() {
        return Err(SnapshotRefusal::NotSnapshotOwner);
    }
    if target.host_program() != record.host_program()
        || target.image_code_hash() != record.image_code_hash()
        || module.code_hash() != record.image_code_hash() {
        return Err(SnapshotRefusal::IncompatibleTarget);
    }
    if supplied.source_lease != record.source_lease()
        || supplied.source_namespace != record.namespace() {
        return Err(SnapshotRefusal::StateLeaseMismatch);
    }
    supplied.validate_against(target)?;
    let bytes = supplied.canonical_bytes()?;
    let byte_length = u64::try_from(bytes.len()).map_err(|_| SnapshotRefusal::StateBoundExceeded)?;
    if byte_length != record.byte_length()
        || hash_bytes(HashAlgorithm::Sha256, &bytes).map_err(|_| SnapshotRefusal::HashRefusal)? != record.digest() {
        return Err(SnapshotRefusal::DigestMismatch);
    }
    if activation.activity != LeaseActivity::Activate
        || activation.from != LeaseState::Funded || activation.to != LeaseState::Active {
        return Err(SnapshotRefusal::InvalidActivation);
    }
    let mut candidate = target.clone();
    candidate.bind_restore(record.digest()).map_err(SnapshotRefusal::Lease)?;
    candidate.transition(activation, evidence).map_err(SnapshotRefusal::Lease)?;
    if source.state() != LeaseState::Destroyed {
        verify_persisted_snapshot(storage, source.namespace(), record.digest(), &bytes)?;
    }
    let entries = rebound_live_entries(&supplied.namespace_cells)?;
    let mut candidate_storage = storage.clone();
    candidate_storage.replace_protocol_namespace(target.namespace().storage_namespace()
        .map_err(SnapshotRefusal::Lease)?, &entries)
        .map_err(SnapshotRefusal::Storage)?;
    let occupied = candidate_storage.namespace_persistent_bytes(target.namespace().storage_namespace()
        .map_err(SnapshotRefusal::Lease)?)
        .map_err(SnapshotRefusal::Storage)?;
    if occupied > target.limits().namespace_bytes { return Err(SnapshotRefusal::TargetBoundExceeded); }
    let mut candidate_meter = meter.clone();
    candidate_meter.charge_storage_write(entries_metered_bytes(&entries)?)
        .map_err(SnapshotRefusal::Meter)?;
    let state = supplied.rebind(target);
    let abi_version = match module.abi_revision() {
        layerx_programs_runtime::AbiRevision::V1 => ABI_V1_VERSION,
        layerx_programs_runtime::AbiRevision::V2 => ABI_V2_VERSION,
    };
    let abi = Abi::new(abi_version, target.host_program(), authorization, candidate_storage,
        &UnavailableReceiptOracle).map_err(SnapshotRefusal::Abi)?;
    let mut instance = module.instantiate_sandbox(candidate_meter, abi)
        .map_err(SnapshotRefusal::Runtime)?;
    let outputs = instance.restore_continuation(&state.runtime_continuation())
        .map_err(SnapshotRefusal::Runtime)?;
    let committed_storage = instance.storage_snapshot()
        .ok_or(SnapshotRefusal::MissingRuntimeStorage)?;
    let committed_meter = instance.meter().clone();
    let restored = RestoredSandbox { target_lease: target.id(), snapshot_digest: record.digest(),
        state, instance, outputs };
    *target = candidate;
    *storage = committed_storage;
    *meter = committed_meter;
    Ok(restored)
}

fn snapshot_prefix(digest: [u8; 32]) -> Vec<u8> {
    let mut key = b"snapshot".to_vec();
    key.extend_from_slice(SNAPSHOT_KEY_DOMAIN);
    key.extend_from_slice(&digest);
    key
}

fn live_namespace_cells(
    storage: &Storage, namespace: EphemeralNamespace,
) -> Result<Vec<NamespaceCell>, SnapshotRefusal> {
    storage.protocol_namespace_entries(namespace.storage_namespace()
        .map_err(SnapshotRefusal::Lease)?)
        .map_err(SnapshotRefusal::Storage)?.into_iter()
        .try_fold(Vec::new(), |mut cells, (key, value)| {
            let cell = NamespaceCell { key, value };
            if cell.key.is_empty() { return Err(SnapshotRefusal::NonCanonicalNamespace); }
            cells.push(cell); Ok(cells)
        })
}

fn rebound_live_entries(cells: &[NamespaceCell]) -> Result<Vec<(Vec<u8>, Vec<u8>)>, SnapshotRefusal> {
    let mut entries = Vec::with_capacity(cells.len());
    for cell in cells {
        if cell.key.is_empty() {
            return Err(SnapshotRefusal::NonCanonicalNamespace);
        }
        entries.push((cell.key.clone(), cell.value.clone()));
    }
    Ok(entries)
}

fn snapshot_entries(
    digest: [u8; 32], bytes: &[u8],
) -> Result<Vec<(Vec<u8>, Vec<u8>)>, SnapshotRefusal> {
    let prefix = snapshot_prefix(digest);
    let count = bytes.len().div_ceil(SNAPSHOT_CHUNK_BYTES);
    let mut manifest_key = prefix.clone();
    manifest_key.push(0);
    let mut manifest = Vec::new();
    manifest.extend_from_slice(&digest);
    manifest.extend_from_slice(&u64::try_from(bytes.len()).map_err(|_| SnapshotRefusal::StateBoundExceeded)?.to_be_bytes());
    manifest.extend_from_slice(&u32::try_from(count).map_err(|_| SnapshotRefusal::StateBoundExceeded)?.to_be_bytes());
    let mut entries = vec![(manifest_key, manifest)];
    for (index, chunk) in bytes.chunks(SNAPSHOT_CHUNK_BYTES).enumerate() {
        let mut key = prefix.clone();
        key.push(1);
        key.extend_from_slice(&u32::try_from(index).map_err(|_| SnapshotRefusal::StateBoundExceeded)?.to_be_bytes());
        entries.push((key, chunk.to_vec()));
    }
    Ok(entries)
}

fn entries_metered_bytes(entries: &[(Vec<u8>, Vec<u8>)]) -> Result<u64, SnapshotRefusal> {
    entries.iter().try_fold(0u64, |total, (key, value)| {
        let bytes = layerx_programs_runtime::storage::metered_bytes(key, Some(value))
            .map_err(SnapshotRefusal::Storage)?;
        total.checked_add(bytes).ok_or(SnapshotRefusal::StateBoundExceeded)
    })
}

fn verify_persisted_snapshot(
    storage: &Storage, namespace: EphemeralNamespace, digest: [u8; 32], expected: &[u8],
) -> Result<(), SnapshotRefusal> {
    let prefix = snapshot_prefix(digest);
    let entries = storage.protocol_prefix_entries(namespace.snapshot_storage_namespace(), &prefix)
        .map_err(SnapshotRefusal::Storage)?;
    let Some((_, manifest)) = entries.first() else { return Err(SnapshotRefusal::DigestMismatch); };
    let expected_length = u64::try_from(expected.len()).map_err(|_| SnapshotRefusal::StateBoundExceeded)?;
    let stored_chunks = usize::try_from(u32::from_be_bytes(manifest.get(40..44)
        .ok_or(SnapshotRefusal::DigestMismatch)?.try_into()
        .map_err(|_| SnapshotRefusal::DigestMismatch)?))
        .map_err(|_| SnapshotRefusal::StateBoundExceeded)?;
    if manifest.len() != 44 || manifest[..32] != digest
        || u64::from_be_bytes(manifest[32..40].try_into()
            .map_err(|_| SnapshotRefusal::DigestMismatch)?) != expected_length
        || stored_chunks != entries.len().saturating_sub(1) {
        return Err(SnapshotRefusal::DigestMismatch);
    }
    let restored: Vec<u8> = entries.iter().skip(1)
        .flat_map(|(_, value)| value.iter().copied()).collect();
    if restored != expected || hash_bytes(HashAlgorithm::Sha256, &restored)
        .map_err(|_| SnapshotRefusal::HashRefusal)? != digest {
        return Err(SnapshotRefusal::DigestMismatch);
    }
    Ok(())
}

fn put_len(output: &mut Vec<u8>, length: usize) -> Result<(), SnapshotRefusal> {
    output.extend_from_slice(&u64::try_from(length).map_err(|_| SnapshotRefusal::StateBoundExceeded)?.to_be_bytes());
    Ok(())
}

fn encode_value(value: WasmValue, output: &mut Vec<u8>) {
    match value {
        WasmValue::I32(value) => {
            output.push(0);
            output.extend_from_slice(&value.to_be_bytes());
        }
        WasmValue::I64(value) => {
            output.push(1);
            output.extend_from_slice(&value.to_be_bytes());
        }
    }
}

fn put_bytes(output: &mut Vec<u8>, bytes: &[u8]) -> Result<(), SnapshotRefusal> {
    put_len(output, bytes.len())?;
    output.extend_from_slice(bytes);
    Ok(())
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum SnapshotRefusal {
    LeaseNotActive,
    UnknownSnapshot,
    TargetNotFunded,
    NotSnapshotOwner,
    StateLeaseMismatch,
    IncompatibleTarget,
    NonCanonicalNamespace,
    NonCanonicalContinuation,
    StateBoundExceeded,
    TargetBoundExceeded,
    DigestMismatch,
    InvalidActivation,
    InvalidSnapshotEvidence,
    MissingRuntimeStorage,
    HashRefusal,
    Meter(MeterRefusal),
    Lease(LeaseRefusal),
    Storage(StorageError),
    Runtime(ExecutionFault),
    Abi(layerx_programs_runtime::AbiError),
}

impl Display for SnapshotRefusal {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result { write!(formatter, "{self:?}") }
}

impl std::error::Error for SnapshotRefusal {}
