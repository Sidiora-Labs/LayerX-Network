//! Bounded, semantics-neutral retention of validated and compiled modules.

use core::fmt::{self, Display};
use std::collections::BTreeMap;
use std::sync::{Arc, Mutex};

#[cfg(feature = "host-ffi")]
use std::sync::OnceLock;

use sha2::{Digest, Sha256};

use crate::lifecycle::CodeHash;
use crate::{
    AbiRevision, EngineRefusal, ValidatedModule, ValidationLimits, ValidationRefusal, WasmEngine,
};

/// Default number of compiled modules retained by one runtime cache.
pub const DEFAULT_MAX_CACHED_MODULES: usize = 64;
/// Default deterministic byte weight retained by one runtime cache.
pub const DEFAULT_MAX_CACHED_MODULE_BYTES: u64 = 64 * 1_048_576;
/// Fixed charge for one engine module and its shared linker references.
pub const COMPILED_MODULE_BASE_WEIGHT_BYTES: u64 = 4_096;
/// Per-function charge for engine-owned compiled function metadata.
pub const COMPILED_FUNCTION_WEIGHT_BYTES: u64 = 512;

/// Receipt-recorded identity of one compiled module artifact.
///
/// The field order is also the canonical eviction order. Every field comes
/// from authenticated protocol state; cache access order is deliberately not
/// represented.
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub struct ModuleCacheKey {
    runtime_version: u16,
    abi_version: u16,
    code_hash: CodeHash,
    metering_schedule_version: u32,
    meter_artifact_digest: [u8; 32],
    metering_schedule_bytes: [u8; 76],
}

impl ModuleCacheKey {
    #[must_use]
    pub(crate) const fn new(code_hash: CodeHash, runtime_version: u16, abi_version: u16) -> Self {
        Self {
            runtime_version,
            abi_version,
            code_hash,
            metering_schedule_version: crate::meter::inject::GENESIS_METERING_SCHEDULE_VERSION,
            meter_artifact_digest: [0; 32],
            metering_schedule_bytes: [0; 76],
        }
    }

    #[must_use]
    const fn with_meter_artifact(mut self, version: u32, digest: [u8; 32]) -> Self {
        self.metering_schedule_version = version;
        self.meter_artifact_digest = digest;
        self
    }

    /// Constructs the complete cache identity from authenticated source bytes.
    #[cfg(test)]
    pub(crate) fn for_legacy_v1_wasm(
        code_hash: CodeHash,
        runtime_version: u16,
        abi_version: u16,
        wasm: &[u8],
    ) -> Result<Self, crate::meter::inject::InjectionRefusal> {
        Self::for_wasm_with_schedule(
            code_hash, runtime_version, abi_version, wasm, crate::FuelSchedule::WASMI_0_31_2,
        )
    }

    pub fn for_wasm_with_schedule(
        code_hash: CodeHash,
        runtime_version: u16,
        abi_version: u16,
        wasm: &[u8],
        schedule: crate::FuelSchedule,
    ) -> Result<Self, crate::meter::inject::InjectionRefusal> {
        let injection = crate::meter::inject::MeterInjection::instrument(
            wasm, schedule,
        )?;
        let mut key = Self::new(code_hash, runtime_version, abi_version).with_meter_artifact(
            injection.schedule().version(),
            injection.digest(),
        );
        key.metering_schedule_bytes = schedule.canonical_bytes();
        Ok(key)
    }

    #[must_use]
    pub const fn code_hash(self) -> CodeHash {
        self.code_hash
    }

    #[must_use]
    pub const fn runtime_version(self) -> u16 {
        self.runtime_version
    }

    #[must_use]
    pub const fn abi_version(self) -> u16 {
        self.abi_version
    }

    #[must_use]
    pub const fn metering_schedule_version(self) -> u32 { self.metering_schedule_version }

    #[must_use]
    pub const fn meter_artifact_digest(self) -> [u8; 32] { self.meter_artifact_digest }

    fn expected_revision(self) -> Result<AbiRevision, CompiledModuleRefusal> {
        if self.runtime_version != crate::RUNTIME_VERSION {
            return Err(CompiledModuleRefusal::UnsupportedRuntimeVersion {
                requested: self.runtime_version,
                supported: crate::RUNTIME_VERSION,
            });
        }
        match self.abi_version {
            crate::ABI_V1_VERSION => Ok(AbiRevision::V1),
            crate::ABI_V2_VERSION => Ok(AbiRevision::V2),
            requested => Err(CompiledModuleRefusal::UnsupportedAbiVersion { requested }),
        }
    }

    fn verify_wasm(self, wasm: &[u8]) -> Result<AbiRevision, CompiledModuleRefusal> {
        let expected_revision = self.expected_revision()?;
        let computed: CodeHash = Sha256::digest(wasm).into();
        if computed != self.code_hash {
            return Err(CompiledModuleRefusal::CodeHashMismatch {
                declared: self.code_hash,
                computed,
            });
        }
        Ok(expected_revision)
    }
}

/// Deterministic cache capacity. Artifact accounting combines validated module
/// bytes with fixed module and function charges; it never observes allocator
/// behavior that can differ by host or build profile.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ModuleCacheLimits {
    max_entries: usize,
    max_accounted_bytes: u64,
}

impl ModuleCacheLimits {
    /// Constructs nonzero entry and byte ceilings.
    ///
    /// # Errors
    ///
    /// Returns [`ModuleCacheLimitsRefusal::ZeroEntries`] or
    /// [`ModuleCacheLimitsRefusal::ZeroBytes`] for a zero ceiling.
    pub const fn new(
        max_entries: usize,
        max_accounted_bytes: u64,
    ) -> Result<Self, ModuleCacheLimitsRefusal> {
        if max_entries == 0 {
            return Err(ModuleCacheLimitsRefusal::ZeroEntries);
        }
        if max_accounted_bytes == 0 {
            return Err(ModuleCacheLimitsRefusal::ZeroBytes);
        }
        Ok(Self {
            max_entries,
            max_accounted_bytes,
        })
    }

    #[must_use]
    pub const fn declared() -> Self {
        Self {
            max_entries: DEFAULT_MAX_CACHED_MODULES,
            max_accounted_bytes: DEFAULT_MAX_CACHED_MODULE_BYTES,
        }
    }

    #[must_use]
    pub const fn max_entries(self) -> usize {
        self.max_entries
    }

    #[must_use]
    pub const fn max_accounted_bytes(self) -> u64 {
        self.max_accounted_bytes
    }
}

impl Default for ModuleCacheLimits {
    fn default() -> Self {
        Self::declared()
    }
}

/// Refusal to construct cache limits that cannot retain any artifact.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ModuleCacheLimitsRefusal {
    ZeroEntries,
    ZeroBytes,
}

impl Display for ModuleCacheLimitsRefusal {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::ZeroEntries => formatter.write_str("module cache entry limit must not be zero"),
            Self::ZeroBytes => formatter.write_str("module cache byte limit must not be zero"),
        }
    }
}

impl std::error::Error for ModuleCacheLimitsRefusal {}

/// Refusal produced before an artifact can enter executable cache state.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum CompiledModuleRefusal {
    UnsupportedRuntimeVersion { requested: u16, supported: u16 },
    UnsupportedAbiVersion { requested: u16 },
    CodeHashMismatch {
        declared: CodeHash,
        computed: CodeHash,
    },
    AbiArtifactMismatch {
        requested: u16,
        compiled: AbiRevision,
    },
    MeterArtifactMismatch,
    Validation(ValidationRefusal),
}

impl Display for CompiledModuleRefusal {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::UnsupportedRuntimeVersion {
                requested,
                supported,
            } => write!(
                formatter,
                "runtime version {requested} is not supported by runtime {supported}"
            ),
            Self::UnsupportedAbiVersion { requested } => {
                write!(formatter, "ABI version {requested} is not supported")
            }
            Self::CodeHashMismatch { .. } => {
                formatter.write_str("compiled module bytes do not match the recorded code hash")
            }
            Self::AbiArtifactMismatch {
                requested,
                compiled,
            } => write!(
                formatter,
                "ABI version {requested} compiled as unexpected revision {compiled:?}"
            ),
            Self::MeterArtifactMismatch => formatter.write_str(
                "compiled metering artifact does not match the recorded schedule and digest",
            ),
            Self::Validation(refusal) => write!(formatter, "module validation refusal: {refusal}"),
        }
    }
}

impl std::error::Error for CompiledModuleRefusal {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Validation(refusal) => Some(refusal),
            Self::UnsupportedRuntimeVersion { .. }
            | Self::UnsupportedAbiVersion { .. }
            | Self::CodeHashMismatch { .. }
            | Self::AbiArtifactMismatch { .. }
            | Self::MeterArtifactMismatch => None,
        }
    }
}

impl From<ValidationRefusal> for CompiledModuleRefusal {
    fn from(refusal: ValidationRefusal) -> Self {
        Self::Validation(refusal)
    }
}

/// A hash- and version-bound artifact produced only by the real runtime
/// validator and compiler.
#[derive(Debug)]
pub struct CompiledModule {
    key: ModuleCacheKey,
    artifact: ValidatedModule,
    validation_limits: ValidationLimits,
    accounted_bytes: u64,
}

impl CompiledModule {
    /// Hashes the exact WASM bytes, validates the recorded versions, and
    /// compiles through [`WasmEngine`] before constructing an artifact.
    ///
    /// # Errors
    ///
    /// Refuses a runtime or ABI the current engine cannot execute, a code-hash
    /// mismatch, or any deterministic module-validation failure.
    pub fn compile(
        engine: &WasmEngine,
        key: ModuleCacheKey,
        wasm: &[u8],
    ) -> Result<Self, CompiledModuleRefusal> {
        let expected_revision = key.verify_wasm(wasm)?;
        Self::compile_verified(engine, key, wasm, expected_revision)
    }

    fn compile_verified(
        engine: &WasmEngine,
        key: ModuleCacheKey,
        wasm: &[u8],
        expected_revision: AbiRevision,
    ) -> Result<Self, CompiledModuleRefusal> {
        let schedule = crate::FuelSchedule::from_protocol_bytes(&key.metering_schedule_bytes)
            .map_err(|refusal| CompiledModuleRefusal::Validation(
                ValidationRefusal::MeterInjection { reason: refusal.to_string() },
            ))?;
        let artifact = engine.validate_versioned_metered(key.abi_version, wasm, schedule)?;
        if key.metering_schedule_version != artifact.metering_schedule_version()
            || key.meter_artifact_digest != artifact.meter_injection().digest()
        {
            return Err(CompiledModuleRefusal::MeterArtifactMismatch);
        }
        if artifact.abi_revision() != expected_revision {
            return Err(CompiledModuleRefusal::AbiArtifactMismatch {
                requested: key.abi_version,
                compiled: artifact.abi_revision(),
            });
        }
        let accounted_bytes = deterministic_artifact_weight(&artifact);
        Ok(Self {
            key,
            artifact,
            validation_limits: engine.limits(),
            accounted_bytes,
        })
    }

    #[must_use]
    pub const fn key(&self) -> ModuleCacheKey {
        self.key
    }

    #[must_use]
    pub const fn code_hash(&self) -> CodeHash {
        self.key.code_hash
    }

    #[must_use]
    pub const fn runtime_version(&self) -> u16 {
        self.key.runtime_version
    }

    #[must_use]
    pub const fn abi_version(&self) -> u16 {
        self.key.abi_version
    }

    #[must_use]
    pub const fn accounted_bytes(&self) -> u64 {
        self.accounted_bytes
    }

    /// Returns the validated, engine-compiled artifact used by every execution
    /// path; cache disposition never enters its meter or evidence state.
    #[must_use]
    pub const fn validated(&self) -> &ValidatedModule {
        &self.artifact
    }
}

fn deterministic_artifact_weight(artifact: &ValidatedModule) -> u64 {
    let function_bytes = u64::from(artifact.function_count()) * COMPILED_FUNCTION_WEIGHT_BYTES;
    match artifact
        .byte_size()
        .checked_add(COMPILED_MODULE_BASE_WEIGHT_BYTES)
        .and_then(|weight| weight.checked_add(function_bytes))
    {
        Some(weight) => weight,
        // An unrepresentable deterministic charge is explicitly uncacheable.
        None => u64::MAX,
    }
}

/// Bounded compiled-module accelerator with no consensus authority.
///
/// Misses compile through the same [`CompiledModule::compile`] path used when
/// caching is disabled. Hits return only that artifact, with no cache marker
/// available to guest execution, metering, receipts, or evidence. When a bound
/// is crossed, admission greedily retains artifacts in receipt-derived key
/// order, making the retained set independent of wall-clock timing and lookup
/// recency. The entry ceiling and each artifact's validated module/function
/// limits provide an additional strict bound on resident compilation inputs.
#[derive(Debug)]
pub struct ModuleCache {
    limits: Option<ModuleCacheLimits>,
    entries: BTreeMap<ModuleCacheKey, Arc<CompiledModule>>,
    accounted_bytes: u64,
}

impl ModuleCache {
    #[must_use]
    pub fn new(limits: ModuleCacheLimits) -> Self {
        Self {
            limits: Some(limits),
            entries: BTreeMap::new(),
            accounted_bytes: 0,
        }
    }

    #[must_use]
    pub fn declared() -> Self {
        Self::new(ModuleCacheLimits::declared())
    }

    /// Constructs a cold-only resolver. It performs the same validation and
    /// compilation but retains no artifact and exposes no different execution
    /// type to the caller.
    #[must_use]
    pub fn disabled() -> Self {
        Self {
            limits: None,
            entries: BTreeMap::new(),
            accounted_bytes: 0,
        }
    }

    #[must_use]
    pub const fn limits(&self) -> Option<ModuleCacheLimits> {
        self.limits
    }

    #[must_use]
    pub const fn is_enabled(&self) -> bool {
        self.limits.is_some()
    }

    #[must_use]
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    #[must_use]
    pub const fn accounted_bytes(&self) -> u64 {
        self.accounted_bytes
    }

    #[must_use]
    pub fn contains(&self, key: ModuleCacheKey) -> bool {
        self.entries.contains_key(&key)
    }

    /// Resolves an exact recorded artifact, compiling on disabled, miss, and
    /// post-eviction paths. Cache admission cannot turn valid execution into a
    /// refusal: an artifact larger than local cache capacity is returned cold
    /// and simply not retained.
    ///
    /// # Errors
    ///
    /// Returns only hash, version, or real validation/compiler refusals.
    pub fn get_or_compile(
        &mut self,
        engine: &WasmEngine,
        key: ModuleCacheKey,
        wasm: &[u8],
    ) -> Result<Arc<CompiledModule>, CompiledModuleRefusal> {
        let expected_revision = key.verify_wasm(wasm)?;
        if let Some(cached) = self.entries.get(&key) {
            if cached.validation_limits == engine.limits() {
                return Ok(Arc::clone(cached));
            }
        }
        let compiled = Arc::new(CompiledModule::compile_verified(
            engine,
            key,
            wasm,
            expected_revision,
        )?);
        self.admit(Arc::clone(&compiled));
        Ok(compiled)
    }

    /// Removes every artifact compiled under a retired runtime version.
    pub fn invalidate_runtime(&mut self, retired_runtime_version: u16) -> usize {
        self.invalidate_where(|key| key.runtime_version == retired_runtime_version)
    }

    /// Removes every artifact compiled under a retired ABI version.
    pub fn invalidate_abi(&mut self, retired_abi_version: u16) -> usize {
        self.invalidate_where(|key| key.abi_version == retired_abi_version)
    }

    /// Removes every artifact for the code hash replaced by a protocol upgrade,
    /// across all runtime and ABI revisions.
    pub fn invalidate_upgrade(&mut self, replaced_code_hash: CodeHash) -> usize {
        self.invalidate_where(|key| key.code_hash == replaced_code_hash)
    }

    pub fn clear(&mut self) {
        self.entries.clear();
        self.accounted_bytes = 0;
    }

    fn admit(&mut self, compiled: Arc<CompiledModule>) {
        let Some(limits) = self.limits else {
            return;
        };
        self.entries.insert(compiled.key, compiled);

        let mut retained = BTreeMap::new();
        let mut retained_bytes = 0_u64;
        for (key, artifact) in core::mem::take(&mut self.entries) {
            if retained.len() == limits.max_entries {
                break;
            }
            let weight = artifact.accounted_bytes;
            if weight == u64::MAX {
                continue;
            }
            let Some(next_bytes) = retained_bytes.checked_add(weight) else {
                continue;
            };
            if next_bytes > limits.max_accounted_bytes {
                continue;
            }
            retained.insert(key, artifact);
            retained_bytes = next_bytes;
        }
        self.entries = retained;
        self.accounted_bytes = retained_bytes;
    }

    fn invalidate_where(
        &mut self,
        mut invalidated: impl FnMut(&ModuleCacheKey) -> bool,
    ) -> usize {
        let before = self.entries.len();
        let mut removed_bytes = 0;
        self.entries.retain(|key, compiled| {
            if invalidated(key) {
                removed_bytes += compiled.accounted_bytes;
                false
            } else {
                true
            }
        });
        self.accounted_bytes -= removed_bytes;
        before - self.entries.len()
    }
}

impl Default for ModuleCache {
    fn default() -> Self {
        Self::declared()
    }
}

/// Failure to use the process-long runtime artifact owner.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum RuntimeArtifactOwnerRefusal {
    /// The immutable engine could not be initialized.
    Initialization(EngineRefusal),
    /// A panic occurred while the process-local cache lock was held.
    SynchronizationPoisoned,
    /// Exact hash/version validation or engine compilation was refused.
    Compilation(CompiledModuleRefusal),
}

impl Display for RuntimeArtifactOwnerRefusal {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Initialization(refusal) => {
                write!(formatter, "runtime artifact owner initialization refused: {refusal}")
            }
            Self::SynchronizationPoisoned => {
                formatter.write_str("runtime artifact cache synchronization is poisoned")
            }
            Self::Compilation(refusal) => write!(formatter, "compiled module refused: {refusal}"),
        }
    }
}

impl std::error::Error for RuntimeArtifactOwnerRefusal {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Initialization(refusal) => Some(refusal),
            Self::Compilation(refusal) => Some(refusal),
            Self::SynchronizationPoisoned => None,
        }
    }
}

/// Process-long owner of the immutable engine and synchronized local cache.
#[derive(Debug)]
pub struct RuntimeArtifactOwner {
    engine: WasmEngine,
    cache: Mutex<ModuleCache>,
}

impl RuntimeArtifactOwner {
    /// Constructs one owner using the runtime's declared engine and cache bounds.
    ///
    /// # Errors
    ///
    /// Returns the engine's typed initialization refusal.
    pub fn declared() -> Result<Self, EngineRefusal> {
        Ok(Self {
            engine: WasmEngine::declared()?,
            cache: Mutex::new(ModuleCache::declared()),
        })
    }

    /// Resolves one exact hash and version, retaining successful compilations
    /// behind an activity-owned [`Arc`].
    ///
    /// # Errors
    ///
    /// Returns a typed synchronization or compilation refusal.
    pub fn get_or_compile(
        &self,
        key: ModuleCacheKey,
        wasm: &[u8],
    ) -> Result<Arc<CompiledModule>, RuntimeArtifactOwnerRefusal> {
        self.cache
            .lock()
            .map_err(|_| RuntimeArtifactOwnerRefusal::SynchronizationPoisoned)?
            .get_or_compile(&self.engine, key, wasm)
            .map_err(RuntimeArtifactOwnerRefusal::Compilation)
    }

    /// Invalidates artifacts for code replaced by a successful upgrade.
    ///
    /// # Errors
    ///
    /// Returns a typed refusal if cache synchronization was poisoned.
    pub fn invalidate_upgrade(
        &self,
        replaced_code_hash: CodeHash,
    ) -> Result<usize, RuntimeArtifactOwnerRefusal> {
        self.cache
            .lock()
            .map_err(|_| RuntimeArtifactOwnerRefusal::SynchronizationPoisoned)
            .map(|mut cache| cache.invalidate_upgrade(replaced_code_hash))
    }

    /// Invalidates artifacts compiled for a retired runtime version.
    ///
    /// # Errors
    ///
    /// Returns a typed refusal if cache synchronization was poisoned.
    pub fn invalidate_runtime(
        &self,
        retired_runtime_version: u16,
    ) -> Result<usize, RuntimeArtifactOwnerRefusal> {
        self.cache
            .lock()
            .map_err(|_| RuntimeArtifactOwnerRefusal::SynchronizationPoisoned)
            .map(|mut cache| cache.invalidate_runtime(retired_runtime_version))
    }

    /// Invalidates artifacts compiled for a retired ABI version.
    ///
    /// # Errors
    ///
    /// Returns a typed refusal if cache synchronization was poisoned.
    pub fn invalidate_abi(
        &self,
        retired_abi_version: u16,
    ) -> Result<usize, RuntimeArtifactOwnerRefusal> {
        self.cache
            .lock()
            .map_err(|_| RuntimeArtifactOwnerRefusal::SynchronizationPoisoned)
            .map(|mut cache| cache.invalidate_abi(retired_abi_version))
    }
}

#[cfg(feature = "host-ffi")]
static RUNTIME_ARTIFACTS: OnceLock<Result<RuntimeArtifactOwner, EngineRefusal>> = OnceLock::new();

#[cfg(feature = "host-ffi")]
pub(crate) fn runtime_artifacts(
) -> Result<&'static RuntimeArtifactOwner, RuntimeArtifactOwnerRefusal> {
    match RUNTIME_ARTIFACTS.get_or_init(RuntimeArtifactOwner::declared) {
        Ok(owner) => Ok(owner),
        Err(refusal) => Err(RuntimeArtifactOwnerRefusal::Initialization(refusal.clone())),
    }
}

#[cfg(feature = "host-ffi")]
pub(crate) fn initialized_runtime_artifacts(
) -> Result<Option<&'static RuntimeArtifactOwner>, RuntimeArtifactOwnerRefusal> {
    match RUNTIME_ARTIFACTS.get() {
        Some(Ok(owner)) => Ok(Some(owner)),
        Some(Err(refusal)) => Err(RuntimeArtifactOwnerRefusal::Initialization(refusal.clone())),
        None => Ok(None),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::{add_module, padding_section};
    use crate::{ExecutionRecord, Executor, WasmValue};

    fn cache_limits(max_entries: usize, max_accounted_bytes: u64) -> ModuleCacheLimits {
        match ModuleCacheLimits::new(max_entries, max_accounted_bytes) {
            Ok(limits) => limits,
            Err(refusal) => panic!("valid cache limits refused: {refusal}"),
        }
    }

    fn key(wasm: &[u8], abi_version: u16) -> ModuleCacheKey {
        ModuleCacheKey::for_legacy_v1_wasm(
            Sha256::digest(wasm).into(), crate::RUNTIME_VERSION, abi_version, wasm,
        ).unwrap_or_else(|refusal| panic!("metering key refused: {refusal}"))
    }

    fn padded_add(padding: usize) -> Vec<u8> {
        let mut wasm = add_module();
        wasm.extend(padding_section(padding));
        wasm
    }

    fn execute(
        cache: &mut ModuleCache,
        engine: &WasmEngine,
        wasm: &[u8],
        left: i32,
        right: i32,
    ) -> ExecutionRecord {
        let compiled = compile(cache, engine, wasm);
        match Executor::declared().execute(
            compiled.validated(),
            "add",
            &[WasmValue::I32(left), WasmValue::I32(right)],
        ) {
            Ok(record) => record,
            Err(refusal) => panic!("module execution refused: {refusal}"),
        }
    }

    fn compile(
        cache: &mut ModuleCache,
        engine: &WasmEngine,
        wasm: &[u8],
    ) -> Arc<CompiledModule> {
        match cache.get_or_compile(engine, key(wasm, crate::ABI_V1_VERSION), wasm) {
            Ok(compiled) => compiled,
            Err(refusal) => panic!("module compilation refused: {refusal}"),
        }
    }

    #[test]
    fn only_exact_hash_and_versions_construct_an_artifact() {
        let engine = match WasmEngine::declared() {
            Ok(engine) => engine,
            Err(refusal) => panic!("declared engine refused: {refusal}"),
        };
        let wasm = add_module();
        let wrong_hash = ModuleCacheKey::new(
            [9; 32],
            crate::RUNTIME_VERSION,
            crate::ABI_V1_VERSION,
        );
        assert!(matches!(
            CompiledModule::compile(&engine, wrong_hash, &wasm),
            Err(CompiledModuleRefusal::CodeHashMismatch { .. })
        ));
        let cache_key = key(&wasm, crate::ABI_V1_VERSION);
        let mut cache = ModuleCache::declared();
        let _ = compile(&mut cache, &engine, &wasm);
        let altered_wasm = padded_add(1);
        assert!(matches!(
            cache.get_or_compile(&engine, cache_key, &altered_wasm),
            Err(CompiledModuleRefusal::CodeHashMismatch { .. })
        ));
        let unknown_runtime = ModuleCacheKey::new(
            key(&wasm, crate::ABI_V1_VERSION).code_hash(),
            crate::RUNTIME_VERSION + 1,
            crate::ABI_V1_VERSION,
        );
        assert_eq!(
            CompiledModule::compile(&engine, unknown_runtime, &wasm)
                .map(|compiled| compiled.key()),
            Err(CompiledModuleRefusal::UnsupportedRuntimeVersion {
                requested: crate::RUNTIME_VERSION + 1,
                supported: crate::RUNTIME_VERSION,
            })
        );
        let unknown_abi = ModuleCacheKey::new(
            key(&wasm, crate::ABI_V1_VERSION).code_hash(),
            crate::RUNTIME_VERSION,
            crate::ABI_VERSION + 1,
        );
        assert_eq!(
            CompiledModule::compile(&engine, unknown_abi, &wasm)
                .map(|compiled| compiled.key()),
            Err(CompiledModuleRefusal::UnsupportedAbiVersion {
                requested: crate::ABI_VERSION + 1,
            })
        );
    }

    #[test]
    fn disabled_miss_hit_and_eviction_have_identical_execution_observations() {
        let engine = match WasmEngine::declared() {
            Ok(engine) => engine,
            Err(refusal) => panic!("declared engine refused: {refusal}"),
        };
        let first = padded_add(3);
        let second = padded_add(5);
        let (evicted, keeper) = if key(&first, crate::ABI_V1_VERSION)
            > key(&second, crate::ABI_V1_VERSION)
        {
            (&first, &second)
        } else {
            (&second, &first)
        };
        let mut disabled = ModuleCache::disabled();
        let cold = execute(&mut disabled, &engine, evicted, 17, 25);
        assert!(disabled.is_empty());

        let mut enabled = ModuleCache::new(cache_limits(1, 1_048_576));
        let miss = execute(&mut enabled, &engine, evicted, 17, 25);
        let hit = execute(&mut enabled, &engine, evicted, 17, 25);
        let _ = execute(&mut enabled, &engine, keeper, 17, 25);
        assert!(!enabled.contains(key(evicted, crate::ABI_V1_VERSION)));
        let after_eviction = execute(&mut enabled, &engine, evicted, 17, 25);

        for observed in [&miss, &hit, &after_eviction] {
            assert_eq!(&cold.outputs, &observed.outputs);
            assert_eq!(cold.usage, observed.usage);
            assert_eq!(cold.canonical_evidence(), observed.canonical_evidence());
        }
    }

    #[test]
    fn eviction_is_canonical_and_capacity_is_never_crossed() {
        let engine = match WasmEngine::declared() {
            Ok(engine) => engine,
            Err(refusal) => panic!("declared engine refused: {refusal}"),
        };
        let modules = [padded_add(7), padded_add(11), padded_add(17)];
        let largest_weight = match modules
            .iter()
            .map(|module| {
                let compiled = match CompiledModule::compile(
                    &engine,
                    key(module, crate::ABI_V1_VERSION),
                    module,
                ) {
                    Ok(compiled) => compiled,
                    Err(refusal) => panic!("module compilation refused: {refusal}"),
                };
                compiled.accounted_bytes()
            })
            .max()
        {
            Some(weight) => weight,
            None => panic!("canonical eviction test requires at least one module"),
        };
        let byte_limit = largest_weight * 2;
        let limits = cache_limits(2, byte_limit);
        let mut forward = ModuleCache::new(limits);
        for module in &modules {
            let _ = execute(&mut forward, &engine, module, 1, 2);
        }
        let mut reverse = ModuleCache::new(limits);
        for module in modules.iter().rev() {
            let _ = execute(&mut reverse, &engine, module, 1, 2);
        }
        assert_eq!(
            forward.entries.keys().copied().collect::<Vec<_>>(),
            reverse.entries.keys().copied().collect::<Vec<_>>()
        );
        assert!(forward.len() <= limits.max_entries());
        assert!(forward.accounted_bytes() <= limits.max_accounted_bytes());
    }

    #[test]
    fn variable_weight_admission_skips_an_oversized_key_and_considers_later_keys() {
        let engine = match WasmEngine::declared() {
            Ok(engine) => engine,
            Err(refusal) => panic!("declared engine refused: {refusal}"),
        };
        let mut candidates = (0..64_usize)
            .map(|ordinal| {
                if ordinal % 2 == 0 {
                    padded_add(ordinal + 1)
                } else {
                    padded_add(8_192 + ordinal)
                }
            })
            .collect::<Vec<_>>();
        candidates.sort_by_key(|module| key(module, crate::ABI_V1_VERSION));
        let mut selection = None;
        for middle in 1..candidates.len() - 1 {
            if candidates[middle].len() < 8_192 {
                continue;
            }
            let first = (0..middle).find(|index| candidates[*index].len() < 8_192);
            let last = (middle + 1..candidates.len())
                .find(|index| candidates[*index].len() < 8_192);
            if let (Some(first), Some(last)) = (first, last) {
                selection = Some((first, middle, last));
                break;
            }
        }
        let (first, middle, last) = match selection {
            Some(selection) => selection,
            None => panic!("deterministic candidate set has no small-heavy-small key sequence"),
        };
        let selected = [&candidates[first], &candidates[middle], &candidates[last]];
        let mut cold = ModuleCache::disabled();
        let weights = selected.map(|module| compile(&mut cold, &engine, module).accounted_bytes());
        assert!(weights[1] > weights[0]);
        assert!(weights[1] > weights[2]);
        let byte_limit = match weights[0].checked_add(weights[2]) {
            Some(limit) => limit,
            None => panic!("selected artifact weights overflowed"),
        };
        let limits = cache_limits(3, byte_limit);

        let mut forward = ModuleCache::new(limits);
        for module in selected {
            let _ = compile(&mut forward, &engine, module);
        }
        let mut reverse = ModuleCache::new(limits);
        for module in selected.into_iter().rev() {
            let _ = compile(&mut reverse, &engine, module);
        }
        let expected = vec![
            key(selected[0], crate::ABI_V1_VERSION),
            key(selected[2], crate::ABI_V1_VERSION),
        ];
        assert_eq!(forward.entries.keys().copied().collect::<Vec<_>>(), expected);
        assert_eq!(
            reverse.entries.keys().copied().collect::<Vec<_>>(),
            expected
        );
        assert_eq!(forward.accounted_bytes(), byte_limit);
        assert_eq!(reverse.accounted_bytes(), byte_limit);
    }

    #[test]
    fn same_key_is_recompiled_and_replaced_for_different_engine_limits() {
        let declared_engine = match WasmEngine::declared() {
            Ok(engine) => engine,
            Err(refusal) => panic!("declared engine refused: {refusal}"),
        };
        let alternate_limits = match ValidationLimits::new(1_048_576, 4_096, 65_536, 256) {
            Ok(limits) => limits,
            Err(refusal) => panic!("alternate validation limits refused: {refusal}"),
        };
        let alternate_engine = match WasmEngine::new(alternate_limits) {
            Ok(engine) => engine,
            Err(refusal) => panic!("alternate engine refused: {refusal}"),
        };
        let wasm = add_module();
        let cache_key = key(&wasm, crate::ABI_V1_VERSION);
        let mut cache = ModuleCache::declared();
        let first = compile(&mut cache, &declared_engine, &wasm);
        let hit = compile(&mut cache, &declared_engine, &wasm);
        let replacement = compile(&mut cache, &alternate_engine, &wasm);

        assert!(Arc::ptr_eq(&first, &hit));
        assert!(!Arc::ptr_eq(&first, &replacement));
        let retained = match cache.entries.get(&cache_key) {
            Some(retained) => retained,
            None => panic!("replacement artifact was not retained"),
        };
        assert!(Arc::ptr_eq(retained, &replacement));
        assert_eq!(retained.validation_limits, alternate_limits);
        assert_eq!(cache.accounted_bytes(), replacement.accounted_bytes());
    }

    #[test]
    fn runtime_abi_and_upgrade_invalidation_are_explicit() {
        let engine = match WasmEngine::declared() {
            Ok(engine) => engine,
            Err(refusal) => panic!("declared engine refused: {refusal}"),
        };
        let wasm = add_module();
        let mut cache = ModuleCache::new(cache_limits(4, 1_048_576));
        for abi_version in [crate::ABI_V1_VERSION, crate::ABI_V2_VERSION] {
            let cache_key = key(&wasm, abi_version);
            if let Err(refusal) = cache.get_or_compile(&engine, cache_key, &wasm) {
                panic!("versioned module compilation refused: {refusal}");
            }
        }
        assert_eq!(cache.invalidate_abi(crate::ABI_V1_VERSION), 1);
        assert!(cache.contains(key(&wasm, crate::ABI_V2_VERSION)));
        assert_eq!(cache.invalidate_runtime(crate::RUNTIME_VERSION), 1);
        assert!(cache.is_empty());

        let cache_key = key(&wasm, crate::ABI_V1_VERSION);
        let held = compile(&mut cache, &engine, &wasm);
        assert_eq!(cache.invalidate_upgrade(cache_key.code_hash()), 1);
        assert!(cache.is_empty());
        assert_eq!(cache.accounted_bytes(), 0);
        let record = match Executor::declared().execute(
            held.validated(),
            "add",
            &[WasmValue::I32(19), WasmValue::I32(23)],
        ) {
            Ok(record) => record,
            Err(refusal) => panic!("held artifact execution refused: {refusal}"),
        };
        assert_eq!(record.outputs, vec![WasmValue::I32(42)]);
    }

    #[test]
    fn large_activity_mix_is_identical_with_cache_disabled_and_enabled() {
        let engine = match WasmEngine::declared() {
            Ok(engine) => engine,
            Err(refusal) => panic!("declared engine refused: {refusal}"),
        };
        let wasm = add_module();
        let mut disabled = ModuleCache::disabled();
        let mut enabled = ModuleCache::declared();
        for ordinal in 0..1_024_i32 {
            let cold = execute(&mut disabled, &engine, &wasm, ordinal, 2_048 - ordinal);
            let cached = execute(&mut enabled, &engine, &wasm, ordinal, 2_048 - ordinal);
            assert_eq!(&cold.outputs, &cached.outputs);
            assert_eq!(cold.usage, cached.usage);
            assert_eq!(cold.canonical_evidence(), cached.canonical_evidence());
        }
        assert!(disabled.is_empty());
        assert_eq!(enabled.len(), 1);
    }
}
