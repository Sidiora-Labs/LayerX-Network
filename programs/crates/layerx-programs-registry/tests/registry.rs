use std::collections::BTreeMap;

use layerx_programs::{
    BuildEnvironment, DeploymentJournal, DeploymentRecord, JournalReadAuthority, ObservedHead,
    ProgramLifecycle, PublishedSource, ReadFreshness, Registry, RegistryError,
    RegistryReadAuthority, ReproducibleBuild, SourceStatus,
};
use layerx_programs_runtime::{hash_bytes, HashAlgorithm, ProgramId, UpgradePolicy};

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
    assert_eq!(latest.version(), 2);
    assert_eq!(latest.code_hash(), second.new_code_hash);
    assert_eq!(latest.receipt_digest(), second.digest());
    assert_eq!(latest.module(), WASM_V2);

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
        Err(RegistryError::CorruptRecord)
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
    let new_code_hash = code_hash(wasm);
    DeploymentRecord {
        program,
        version,
        abi_version: 1,
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
