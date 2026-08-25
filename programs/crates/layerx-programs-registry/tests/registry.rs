use std::collections::BTreeMap;

use layerx_programs::{
    BuildEnvironment, DeploymentJournal, DeploymentRecord, ExecutableAdmissionError,
    JournalReadAuthority, ObservedHead, ProgramLifecycle, PublishedSource, ReadFreshness,
    Registry, RegistryError,
    RegistryReadAuthority, ReproducibleBuild, SourceStatus, VerifiedProgramCatalog,
};
use layerx_programs_runtime::{
    hash_bytes, CompositionRules, Deploy, HashAlgorithm, Lifecycle, ProgramId, ProgramResolver,
    ProgramVersion, UpgradePolicy, ABI_VERSION,
};

const PROGRAM: [u8; 32] = [0x31; 32];
const WASM_V1: &[u8] = &[0, 97, 115, 109, 1, 0, 0, 0];
const WASM_V2: &[u8] = &[0, 97, 115, 109, 1, 0, 0, 0, 0, 3, 2, b'v', b'2'];
const FOREIGN_WASM: &[u8] = &[
    0, 97, 115, 109, 1, 0, 0, 0, 0, 8, 7, b'f', b'o', b'r', b'e', b'i', b'g', b'n',
];

struct WrongAuthority;

impl RegistryReadAuthority for WrongAuthority {
    fn verify_registry_read(
        &self,
        _program: ProgramId,
        _latest: &layerx_programs::RegistryVersion,
    ) -> Result<([u8; 32], ReadFreshness), RegistryError> {
        Ok((
            [0x99; 32],
            ReadFreshness {
                observed_sequence: 1,
                observed_at: 1,
            },
        ))
    }
}

#[derive(Clone)]
struct CanonicalJournal {
    records: BTreeMap<[u8; 32], Vec<u8>>,
    head: ObservedHead,
}

impl CanonicalJournal {
    fn new(records: &[DeploymentRecord], head: ObservedHead) -> Self {
        Self {
            records: records
                .iter()
                .map(|record| (record.digest(), record.canonical_encoding()))
                .collect(),
            head,
        }
    }
}

impl DeploymentJournal for CanonicalJournal {
    fn canonical_record(&self, receipt_digest: [u8; 32]) -> Result<Vec<u8>, RegistryError> {
        self.records
            .get(&receipt_digest)
            .cloned()
            .ok_or(RegistryError::JournalUnavailable)
    }

    fn observed_head(&self) -> Result<ObservedHead, RegistryError> {
        Ok(self.head)
    }
}

#[test]
fn registry_resolves_latest_and_historical_code_only_from_canonical_journal_evidence() {
    let program = program();
    let policy = UpgradePolicy::Authority([0x51; 32]);
    let first = record(program, 1, None, WASM_V1, policy, 70);
    let second = record(
        program,
        2,
        Some(first.new_code_hash),
        WASM_V2,
        policy,
        71,
    );
    let mut registry = Registry::new();
    registry
        .replay_journal(&[first.clone(), second.clone()])
        .unwrap_or_else(|error| panic!("journal replay: {error}"));

    let journal = CanonicalJournal::new(
        &[first.clone(), second.clone()],
        ObservedHead {
            sequence: 77,
            observed_at: 1_700_000_100,
        },
    );
    let authority = JournalReadAuthority::new(journal, 1_700_000_150, 100)
        .unwrap_or_else(|error| panic!("journal authority: {error}"));
    let historical = registry
        .resolve_deployment(program, 1, &authority)
        .unwrap_or_else(|error| panic!("historical deployment: {error}"));
    let latest = registry
        .resolve_deployment(program, 2, &authority)
        .unwrap_or_else(|error| panic!("latest deployment: {error}"));

    assert_eq!(historical.program(), program);
    assert_eq!(historical.version(), 1);
    assert_eq!(historical.code_hash(), first.new_code_hash);
    assert_eq!(historical.receipt_digest(), first.digest());
    assert_eq!(historical.module(), WASM_V1);
    assert_eq!(historical.freshness().observed_sequence, 77);
    assert_eq!(historical.lifecycle(), ProgramLifecycle::Active);
    assert_eq!(latest.version(), 2);
    assert_eq!(latest.code_hash(), second.new_code_hash);
    assert_eq!(latest.receipt_digest(), second.digest());
    assert_eq!(latest.module(), WASM_V2);

    let mut executable = VerifiedProgramCatalog::declared()
        .unwrap_or_else(|error| panic!("verified catalog: {error}"));
    executable
        .admit(historical)
        .unwrap_or_else(|error| panic!("historical admission: {error}"));
    assert_eq!(executable.version(program), Some(1));
    executable
        .admit(latest)
        .unwrap_or_else(|error| panic!("latest admission: {error}"));
    assert_eq!(executable.version(program), Some(2));
    assert_eq!(executable.code_hash(program), Some(second.new_code_hash));
    assert_eq!(executable.abi_version(program), Some(ABI_VERSION));
    assert_eq!(executable.receipt_digest(program), Some(second.digest()));
    assert!(ProgramResolver::program_module(&executable, program).is_some());
    let composition = executable.into_composition_context(CompositionRules::declared());
    assert!(composition.resolver().program_module(program).is_some());

    let read = registry
        .read(program, &authority)
        .unwrap_or_else(|error| panic!("registry read: {error}"));
    assert_eq!(read.entry.program, program);
    assert_eq!(read.entry.upgrade_policy, policy);
    assert_eq!(read.entry.lifecycle, ProgramLifecycle::Active);
    assert_eq!(read.entry.versions.len(), 2);
    assert_eq!(read.entry.versions[0].code_hash, code_hash(WASM_V1));
    assert_eq!(read.entry.versions[1].code_hash, second.new_code_hash);
}

#[test]
fn callable_resolution_preserves_typed_mismatch_and_stale_refusals() {
    let program = program();
    let policy = UpgradePolicy::Immutable;
    let record = record(program, 1, None, WASM_V1, policy, 70);
    let foreign = record(program, 1, None, FOREIGN_WASM, policy, 70);
    let mut registry = Registry::new();
    registry
        .replay_journal(&[record.clone()])
        .unwrap_or_else(|error| panic!("journal replay: {error}"));

    let mut mismatched = CanonicalJournal::new(
        &[record.clone()],
        ObservedHead {
            sequence: 77,
            observed_at: 1_700_000_100,
        },
    );
    mismatched
        .records
        .insert(record.digest(), foreign.canonical_encoding());
    let mismatch_authority = JournalReadAuthority::new(mismatched, 1_700_000_150, 100)
        .unwrap_or_else(|error| panic!("mismatch authority: {error}"));
    assert_eq!(
        registry.resolve_deployment(program, 1, &mismatch_authority),
        Err(RegistryError::UnverifiedRead)
    );

    let stale = CanonicalJournal::new(
        &[record],
        ObservedHead {
            sequence: 77,
            observed_at: 1_700_000_100,
        },
    );
    let stale_authority = JournalReadAuthority::new(stale, 1_700_001_000, 100)
        .unwrap_or_else(|error| panic!("stale authority: {error}"));
    assert_eq!(
        registry.resolve_deployment(program, 1, &stale_authority),
        Err(RegistryError::StaleRead)
    );
}

#[test]
fn invalid_module_abi_and_receipt_evidence_never_enter_the_verified_resolver() {
    let program = program();
    let expected = record(program, 1, None, WASM_V1, UpgradePolicy::Immutable, 70);
    let wrong_receipt = record(
        program,
        1,
        None,
        FOREIGN_WASM,
        UpgradePolicy::Immutable,
        70,
    );
    let mut receipt_registry = Registry::new();
    receipt_registry
        .replay_journal(&[expected.clone()])
        .unwrap_or_else(|error| panic!("receipt projection: {error}"));
    let mut receipt_journal = CanonicalJournal::new(
        &[expected.clone()],
        ObservedHead {
            sequence: 77,
            observed_at: 1_700_000_100,
        },
    );
    receipt_journal
        .records
        .insert(expected.digest(), wrong_receipt.canonical_encoding());
    let receipt_authority = JournalReadAuthority::new(receipt_journal, 1_700_000_150, 100)
        .unwrap_or_else(|error| panic!("receipt authority: {error}"));
    assert_eq!(
        receipt_registry.resolve_deployment(program, 1, &receipt_authority),
        Err(RegistryError::UnverifiedRead)
    );

    let wrong_abi = record_with_abi(
        program,
        1,
        None,
        WASM_V1,
        UpgradePolicy::Immutable,
        70,
        2,
    );
    let mut abi_journal = CanonicalJournal::new(
        &[expected.clone()],
        ObservedHead {
            sequence: 77,
            observed_at: 1_700_000_100,
        },
    );
    abi_journal
        .records
        .insert(expected.digest(), wrong_abi.canonical_encoding());
    let abi_authority = JournalReadAuthority::new(abi_journal, 1_700_000_150, 100)
        .unwrap_or_else(|error| panic!("ABI authority: {error}"));
    assert_eq!(
        receipt_registry.resolve_deployment(program, 1, &abi_authority),
        Err(RegistryError::UnverifiedRead)
    );

    let malformed = record(
        program,
        1,
        None,
        b"not-wasm",
        UpgradePolicy::Immutable,
        70,
    );
    let mut malformed_registry = Registry::new();
    malformed_registry
        .replay_journal(&[malformed.clone()])
        .unwrap_or_else(|error| panic!("malformed projection: {error}"));
    let malformed_authority = JournalReadAuthority::new(
        CanonicalJournal::new(
            &[malformed],
            ObservedHead {
                sequence: 77,
                observed_at: 1_700_000_100,
            },
        ),
        1_700_000_150,
        100,
    )
    .unwrap_or_else(|error| panic!("malformed authority: {error}"));
    let malformed_evidence = malformed_registry
        .resolve_deployment(program, 1, &malformed_authority)
        .unwrap_or_else(|error| panic!("malformed evidence resolution: {error}"));

    let unsupported = record_with_abi(
        program,
        1,
        None,
        WASM_V1,
        UpgradePolicy::Immutable,
        70,
        3,
    );
    let mut unsupported_registry = Registry::new();
    unsupported_registry
        .replay_journal(&[unsupported.clone()])
        .unwrap_or_else(|error| panic!("unsupported ABI projection: {error}"));
    let unsupported_authority = JournalReadAuthority::new(
        CanonicalJournal::new(
            &[unsupported],
            ObservedHead {
                sequence: 77,
                observed_at: 1_700_000_100,
            },
        ),
        1_700_000_150,
        100,
    )
    .unwrap_or_else(|error| panic!("unsupported ABI authority: {error}"));
    let unsupported_evidence = unsupported_registry
        .resolve_deployment(program, 1, &unsupported_authority)
        .unwrap_or_else(|error| panic!("unsupported ABI evidence: {error}"));

    let mut executable = VerifiedProgramCatalog::declared()
        .unwrap_or_else(|error| panic!("verified catalog: {error}"));
    assert!(matches!(
        executable.admit(malformed_evidence),
        Err(ExecutableAdmissionError::Validation(_))
    ));
    assert!(executable.is_empty());
    assert_eq!(
        executable.admit(unsupported_evidence),
        Err(ExecutableAdmissionError::UnsupportedAbi { declared: 3 })
    );
    assert!(executable.is_empty());
}

#[test]
fn direct_deployment_insertion_refuses_the_reserved_upgrade_authority() {
    let program = program();
    let code_hash = code_hash(WASM_V1);
    let version = ProgramVersion {
        code_hash,
        wasm: WASM_V1.to_vec(),
        abi_version: ABI_VERSION,
    };
    let mut lifecycle = Lifecycle::declared()
        .unwrap_or_else(|error| panic!("lifecycle construction: {error}"));
    let receipt = lifecycle
        .deploy(Deploy {
            program,
            code_hash,
            wasm: WASM_V1.to_vec(),
            abi_version: ABI_VERSION,
            upgrade_policy: UpgradePolicy::Immutable,
        })
        .unwrap_or_else(|error| panic!("deployment: {error}"));
    let canonical = DeploymentRecord::from_deployment(
        &receipt,
        &version,
        UpgradePolicy::Immutable,
        70,
        1_700_000_070,
    )
    .unwrap_or_else(|error| panic!("canonical deployment: {error}"));
    let mut registry = Registry::new();
    assert_eq!(
        registry.record_deployment(
            &receipt,
            &version,
            UpgradePolicy::Authority([0; 32]),
            canonical.digest(),
        ),
        Err(RegistryError::InvalidUpgradeAuthority)
    );
    assert_eq!(
        registry.latest_version(program),
        Err(RegistryError::UnknownProgram)
    );
}

#[test]
fn reproducible_build_verification_records_success_and_visible_mismatch() {
    let program = program();
    let first = record(program, 1, None, WASM_V1, UpgradePolicy::Immutable, 70);
    let mut registry = Registry::new();
    registry
        .replay_journal(&[first.clone()])
        .unwrap_or_else(|error| panic!("deployment: {error}"));
    let verified = ReproducibleBuild::from_output(&source(), environment(), WASM_V1)
        .unwrap_or_else(|error| panic!("build evidence: {error}"));
    assert_eq!(verified.artifact_digest, code_hash(WASM_V1));
    assert!(matches!(
        registry.verify_source(program, 1, &verified),
        Ok(SourceStatus::Verified { .. })
    ));

    let altered = ReproducibleBuild::from_output(&source(), environment(), WASM_V2)
        .unwrap_or_else(|error| panic!("altered evidence: {error}"));
    let mismatch = registry
        .verify_source(program, 1, &altered)
        .unwrap_or_else(|error| panic!("mismatch status: {error}"));
    assert_eq!(
        mismatch,
        SourceStatus::Mismatch {
            expected: code_hash(WASM_V1),
            reproduced: altered.artifact_digest,
        }
    );
    let journal = CanonicalJournal::new(
        &[first],
        ObservedHead {
            sequence: 77,
            observed_at: 1_700_000_100,
        },
    );
    let authority = JournalReadAuthority::new(journal, 1_700_000_150, 100)
        .unwrap_or_else(|error| panic!("journal authority: {error}"));
    let read = registry
        .read(program, &authority)
        .unwrap_or_else(|error| panic!("registry read: {error}"));
    assert_eq!(read.entry.versions[0].source, mismatch);
}

#[test]
fn noncontiguous_or_unverified_registry_history_is_refused() {
    let program = program();
    let mut registry = Registry::new();
    let reserved_authority = record(
        program,
        1,
        None,
        WASM_V1,
        UpgradePolicy::Authority([0; 32]),
        70,
    );
    assert_eq!(
        registry.replay_journal(&[reserved_authority]),
        Err(RegistryError::InvalidUpgradeAuthority)
    );
    let second = record(
        program,
        2,
        Some(code_hash(WASM_V1)),
        WASM_V2,
        UpgradePolicy::Immutable,
        71,
    );
    assert_eq!(
        registry.replay_journal(&[second]),
        Err(RegistryError::VersionHistoryMismatch)
    );

    let first = record(program, 1, None, WASM_V1, UpgradePolicy::Immutable, 70);
    registry
        .replay_journal(&[first])
        .unwrap_or_else(|error| panic!("deployment: {error}"));
    assert_eq!(
        registry.read(program, &WrongAuthority),
        Err(RegistryError::UnverifiedRead)
    );
}

fn program() -> ProgramId {
    ProgramId::new(PROGRAM).unwrap_or_else(|error| panic!("program: {error}"))
}

fn code_hash(wasm: &[u8]) -> [u8; 32] {
    hash_bytes(HashAlgorithm::Sha256, wasm)
        .unwrap_or_else(|error| panic!("program code hash: {error}"))
}

fn record(
    program: ProgramId,
    version: u32,
    old_code_hash: Option<[u8; 32]>,
    wasm: &[u8],
    upgrade_policy: UpgradePolicy,
    sequence: u64,
) -> DeploymentRecord {
    record_with_abi(
        program,
        version,
        old_code_hash,
        wasm,
        upgrade_policy,
        sequence,
        ABI_VERSION,
    )
}

fn record_with_abi(
    program: ProgramId,
    version: u32,
    old_code_hash: Option<[u8; 32]>,
    wasm: &[u8],
    upgrade_policy: UpgradePolicy,
    sequence: u64,
    abi_version: u16,
) -> DeploymentRecord {
    let new_code_hash = code_hash(wasm);
    DeploymentRecord {
        program,
        version,
        abi_version,
        upgrade_policy,
        old_code_hash,
        new_code_hash,
        sequence,
        observed_at: 1_700_000_000 + sequence,
        module: wasm.to_vec(),
        migration: None,
    }
}

fn source() -> PublishedSource {
    PublishedSource {
        uri: "https://source.example/program.tar".to_owned(),
        canonical_archive: b"canonical-source-archive".to_vec(),
    }
}

fn environment() -> BuildEnvironment {
    BuildEnvironment {
        builder_image_digest: [0x71; 32],
        toolchain_digest: [0x72; 32],
        dependency_lock_digest: [0x73; 32],
        source_date_epoch: 1_700_000_000,
        command: vec![
            "cargo".to_owned(),
            "build".to_owned(),
            "--locked".to_owned(),
        ],
    }
}
