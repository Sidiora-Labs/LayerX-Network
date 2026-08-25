//! Publishing, rebuilding, deploying and running the ported contract against
//! the real programs plane.
//!
//! Nothing here stands in for the protocol. The deterministic engine, the
//! deployment lifecycle, the registry, the reproducible-build pipeline, the
//! metered executor and the monetary law are the production types; the kernel
//! transfer primitive and the receipt oracle stay caller-supplied boundaries,
//! because a port that settled money against a substitute kernel would prove
//! nothing at all.

use layerx_programs::{
    BuildAttempt, BuildEnvironment, BuildPlan, BuildRefusal, BuildRunner, DeploymentRecord,
    PublishedSource, Registry, SourceArchive, SourceFile, SourceStatus, SourceVerifier,
};
use layerx_programs_runtime::{
    AbiEffects, AbiError, AuthorizationContext, AuthorizedExecutionRecord,
    AuthorizedExecutionRequest, CompositionContext, Deploy, DeploymentReceipt, Executor,
    KernelTransferPrimitive, Lifecycle, PreparedAuthorizedActivity, PrincipalId, ProgramId,
    ProgramVersion, ReceiptOracle, ReceiptView, Storage, StorageNamespace, TransferCapability,
    UpgradePolicy, ValidatedModule, VerifiedStorageAssignment, WasmEngine, ABI_VERSION,
    CALL_ENTRY_EXPORT,
};

use crate::error::PortRefusal;
use crate::hash::sha256;
use crate::layout::MigrationCell;
use crate::reference::{
    query_capabilities, PublicLockPort, ARTIFACT_PATH, BUILD_COMMAND, DEPENDENCY_LOCK,
    DEPENDENCY_LOCK_PATH, DESCRIPTOR_PATH, HAS_VALID_KEY_EXPORT, HAS_VALID_KEY_METHOD,
    PURCHASE_METHOD, REMAINING_PERIODS_EXPORT, REMAINING_PERIODS_METHOD, SOLIDITY_SOURCE,
    SOURCE_PATH, TOOLCHAIN_MANIFEST, TOOLCHAIN_PATH,
};
use crate::semantics::MethodAbi;
use crate::value::Word;

/// Independent hermetic rebuilds a published port is verified with. Two is the
/// pipeline's floor: one build cannot demonstrate reproducibility.
pub const REBUILD_ATTEMPTS: u32 = 2;

/// The pinned build epoch. The emitter reads no clock, so the epoch is a fixed
/// constant rather than the wall time of whoever ran the build.
pub const SOURCE_EPOCH: u64 = 1;

const BUILDER_DOMAIN: &[u8] = b"LayerX/porting/evm/builder/v1\0";

/// A receipt oracle holding no verified receipts.
///
/// The ported contract declares no receipt-read authority, so the ABI never
/// consults it. It refuses every digest rather than inventing receipt facts,
/// which is the only honest answer an empty oracle can give.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct AbsentReceipts;

impl ReceiptOracle for AbsentReceipts {
    fn verified_receipt(&self, _receipt_digest: [u8; 32]) -> Result<ReceiptView, AbiError> {
        Err(AbiError::ReceiptMismatch)
    }
}

/// The hermetic builder for `EVM` ports.
///
/// The published archive carries the port descriptor, and this kit's emitter
/// is its compiler: the runner parses the descriptor out of the archive and
/// emits the module from it. The build depends on nothing outside the archive,
/// so every attempt produces identical bytes.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct PortBuildRunner;

impl BuildRunner for PortBuildRunner {
    fn run(&self, attempt: &BuildAttempt<'_>) -> Result<Vec<u8>, BuildRefusal> {
        let descriptor_path = attempt
            .plan
            .environment
            .command
            .last()
            .ok_or(BuildRefusal::InvalidPlan)?;
        let descriptor = attempt.archive.file(descriptor_path).ok_or_else(|| {
            BuildRefusal::MissingPinnedFile {
                path: descriptor_path.clone(),
            }
        })?;
        let text =
            core::str::from_utf8(&descriptor.content).map_err(|_| BuildRefusal::BuilderFailed {
                reason: "port descriptor is not valid text".to_owned(),
            })?;
        let provenance =
            attempt
                .archive
                .file(SOURCE_PATH)
                .ok_or_else(|| BuildRefusal::MissingPinnedFile {
                    path: SOURCE_PATH.to_owned(),
                })?;
        if provenance.content != SOLIDITY_SOURCE.as_bytes() {
            return Err(BuildRefusal::BuilderFailed {
                reason: "published Solidity is not the contract this kit ports".to_owned(),
            });
        }
        let port = PublicLockPort::parse(text).map_err(|refusal| BuildRefusal::BuilderFailed {
            reason: refusal.to_string(),
        })?;
        port.code()
            .map_err(|refusal| BuildRefusal::ArtifactRejected {
                reason: refusal.to_string(),
            })
    }
}

/// Where and when a port is published, and which program identifier it takes.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Publication {
    /// The identifier the deployed program takes.
    pub program: ProgramId,
    /// The protocol sequence at which the deployment is journalled.
    pub sequence: u64,
    /// The protocol time at which the deployment is journalled.
    pub observed_at: u64,
}

/// A deployed, journalled and source-verified port.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DeployedLock {
    /// The deployed program identifier.
    pub program: ProgramId,
    /// The receipt making the version callable.
    pub receipt: DeploymentReceipt,
    /// The canonical journal record naming the deployment.
    pub record: DeploymentRecord,
    /// The `SHA-256` digest of the deployed module.
    pub code_hash: [u8; 32],
    /// The status a hermetic rebuild of the published source produced.
    pub source: SourceStatus,
}

/// One authorised invocation of a deployed port.
pub struct Invocation<'plane> {
    /// The validated module the executor instantiates.
    pub module: &'plane ValidatedModule,
    /// The deployed program identifier.
    pub program: ProgramId,
    /// The principal whose authority the activity carries.
    pub principal: PrincipalId,
    /// The core-owned boundary supplying verified receipt facts.
    pub receipts: &'plane dyn ReceiptOracle,
}

/// Assembles the canonical published archive: the Solidity the port
/// reproduces, the descriptor the build compiles, and the pinned toolchain and
/// dependency lock the build plan commits to.
///
/// # Errors
///
/// Refuses an archive the canonical ordering rules reject.
pub fn source_archive(port: &PublicLockPort) -> Result<SourceArchive, PortRefusal> {
    Ok(SourceArchive::new(vec![
        SourceFile {
            path: SOURCE_PATH.to_owned(),
            executable: false,
            content: SOLIDITY_SOURCE.as_bytes().to_vec(),
        },
        SourceFile {
            path: DESCRIPTOR_PATH.to_owned(),
            executable: false,
            content: port.encode().into_bytes(),
        },
        SourceFile {
            path: TOOLCHAIN_PATH.to_owned(),
            executable: false,
            content: TOOLCHAIN_MANIFEST.as_bytes().to_vec(),
        },
        SourceFile {
            path: DEPENDENCY_LOCK_PATH.to_owned(),
            executable: false,
            content: DEPENDENCY_LOCK.as_bytes().to_vec(),
        },
    ])?)
}

/// Returns the published source document for a port at a caller-supplied
/// location. The kit never invents where source is published.
///
/// # Errors
///
/// Refuses an archive the canonical ordering rules reject.
pub fn published_source(port: &PublicLockPort, uri: &str) -> Result<PublishedSource, PortRefusal> {
    Ok(PublishedSource {
        uri: uri.to_owned(),
        canonical_archive: source_archive(port)?.encode(),
    })
}

/// Returns the declared build recipe: the pinned builder identity, the pinned
/// toolchain and lock digests, the exact command and the artifact it produces.
#[must_use]
pub fn build_plan() -> BuildPlan {
    let toolchain_digest = sha256(TOOLCHAIN_MANIFEST.as_bytes());
    let mut builder_identity = Vec::with_capacity(BUILDER_DOMAIN.len() + 32);
    builder_identity.extend_from_slice(BUILDER_DOMAIN);
    builder_identity.extend_from_slice(&toolchain_digest);
    BuildPlan {
        environment: BuildEnvironment {
            builder_image_digest: sha256(&builder_identity),
            toolchain_digest,
            dependency_lock_digest: sha256(DEPENDENCY_LOCK.as_bytes()),
            source_date_epoch: SOURCE_EPOCH,
            command: BUILD_COMMAND
                .split_whitespace()
                .map(str::to_owned)
                .collect(),
        },
        artifact_path: ARTIFACT_PATH.to_owned(),
        toolchain_manifest: TOOLCHAIN_PATH.to_owned(),
        dependency_lock: DEPENDENCY_LOCK_PATH.to_owned(),
    }
}

/// Validates the emitted module against the deterministic subset and the
/// declared runtime limits.
///
/// # Errors
///
/// Refuses an engine the declared stack limits reject and a module outside the
/// deterministic subset.
pub fn validated_module(port: &PublicLockPort) -> Result<ValidatedModule, PortRefusal> {
    let engine = WasmEngine::declared()?;
    Ok(engine.validate(&port.code()?)?)
}

/// Deploys a port, journals the deployment, rebuilds the published source in
/// independent hermetic attempts and records the resulting source status.
///
/// # Errors
///
/// Refuses an artifact the deterministic subset rejects, a deployment the
/// lifecycle rejects, a journal record that does not bind its module, a
/// rebuild that is not reproducible and any rebuild whose artifact digest is
/// not the registered code hash.
pub fn deploy_and_verify(
    port: &PublicLockPort,
    publication: Publication,
    source: &PublishedSource,
    lifecycle: &mut Lifecycle,
    registry: &mut Registry,
) -> Result<DeployedLock, PortRefusal> {
    let wasm = port.code()?;
    let code_hash = sha256(&wasm);
    let receipt = lifecycle.deploy(Deploy {
        program: publication.program,
        code_hash,
        wasm: wasm.clone(),
        abi_version: ABI_VERSION,
        upgrade_policy: UpgradePolicy::Immutable,
    })?;
    let version = ProgramVersion {
        code_hash,
        wasm,
        abi_version: ABI_VERSION,
    };
    let record = DeploymentRecord::from_deployment(
        &receipt,
        &version,
        UpgradePolicy::Immutable,
        publication.sequence,
        publication.observed_at,
    )?;
    registry.replay_journal(core::slice::from_ref(&record))?;
    let verifier = SourceVerifier::new(PortBuildRunner, REBUILD_ATTEMPTS)?;
    let build = verifier.reproduce(source, &build_plan())?;
    let status = registry.verify_source(publication.program, receipt.version(), &build)?;
    if !matches!(status, SourceStatus::Verified { .. }) {
        return Err(PortRefusal::UnverifiedSource);
    }
    Ok(DeployedLock {
        program: publication.program,
        receipt,
        record,
        code_hash,
        source: status,
    })
}

/// Buys or extends a key under exactly the authority the purchase needs.
///
/// The monetary effect leaves as a typed request. This function never touches
/// a balance, and there is no path from here to one.
///
/// # Errors
///
/// Refuses a purchase outside the declared bounds, an unauthorised grant and
/// every guest, resource or ABI refusal the executor returns.
pub fn execute_purchase(
    port: &PublicLockPort,
    invocation: &Invocation<'_>,
    storage: &mut Storage,
    periods: u64,
) -> Result<AuthorizedExecutionRecord, PortRefusal> {
    let capabilities = port.purchase_capabilities(periods)?;
    let calldata = MethodAbi::new(PURCHASE_METHOD)?.calldata(&[Word::from_u64(periods)])?;
    Ok(Executor::declared().execute_authorized(
        storage,
        AuthorizedExecutionRequest {
            module: invocation.module,
            program: invocation.program,
            authorization: AuthorizationContext::new(invocation.principal, capabilities),
            receipts: invocation.receipts,
            entrypoint: CALL_ENTRY_EXPORT,
            calldata: &calldata,
            composition: CompositionContext::isolated(),
            response_capacity: 0,
        },
    )?)
}

/// Answers whether the invoking principal holds a live key.
///
/// # Errors
///
/// Refuses an unauthorised grant and every refusal the executor returns.
pub fn execute_has_valid_key(
    invocation: &Invocation<'_>,
    storage: &mut Storage,
) -> Result<AuthorizedExecutionRecord, PortRefusal> {
    query(invocation, storage, HAS_VALID_KEY_EXPORT)
}

/// Answers how many periods the invoking principal holds.
///
/// # Errors
///
/// Refuses an unauthorised grant and every refusal the executor returns.
pub fn execute_remaining_periods(
    invocation: &Invocation<'_>,
    storage: &mut Storage,
) -> Result<AuthorizedExecutionRecord, PortRefusal> {
    query(invocation, storage, REMAINING_PERIODS_EXPORT)
}

/// Settles a successful execution's effects through the kernel's own 402LXP
/// primitive and returns only its verified receipt.
///
/// # Errors
///
/// Refuses an unverified invocation authority, every monetary-law violation
/// and every kernel or receipt refusal.
pub fn settle(
    activity: PreparedAuthorizedActivity,
    storage: &mut Storage,
    kernel: &mut impl KernelTransferPrimitive,
) -> Result<VerifiedStorageAssignment, PortRefusal> {
    activity
        .strict_settle(storage, kernel)
        .map_err(|failure| PortRefusal::from(failure.error()))
}

/// Imports an exported `EVM` state dump into namespaced storage, writing each
/// cell into the namespace of the principal that owns it, and returns the
/// number of changed cells.
///
/// # Errors
///
/// Refuses the reserved zero principal and every key or value the storage
/// bounds reject.
pub fn import_state(
    storage: &mut Storage,
    program: ProgramId,
    cells: &[(MigrationCell, [u8; 32])],
) -> Result<usize, PortRefusal> {
    let mut changed = 0_usize;
    for (cell, value) in cells {
        let principal = PrincipalId::new(cell.principal)?;
        let mut transaction = storage.transaction(StorageNamespace::principal(program, principal));
        transaction.write(&cell.layerx_key, value)?;
        changed = changed.saturating_add(transaction.commit());
    }
    Ok(changed)
}

fn query(
    invocation: &Invocation<'_>,
    storage: &mut Storage,
    export: &str,
) -> Result<AuthorizedExecutionRecord, PortRefusal> {
    let method = match export {
        HAS_VALID_KEY_EXPORT => HAS_VALID_KEY_METHOD,
        REMAINING_PERIODS_EXPORT => REMAINING_PERIODS_METHOD,
        _ => return Err(PortRefusal::OutOfRange),
    };
    let calldata = MethodAbi::new(method)?.calldata(&[])?;
    Ok(Executor::declared().execute_authorized(
        storage,
        AuthorizedExecutionRequest {
            module: invocation.module,
            program: invocation.program,
            authorization: AuthorizationContext::new(invocation.principal, query_capabilities()?),
            receipts: invocation.receipts,
            entrypoint: CALL_ENTRY_EXPORT,
            calldata: &calldata,
            composition: CompositionContext::isolated(),
            response_capacity: 0,
        },
    )?)
}
