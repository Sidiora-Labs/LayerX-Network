use std::collections::BTreeMap;

mod support;

use layerx_programs::{
    BuildEnvironment, DeploymentJournal, DeploymentRecord, ExecutableAdmissionError,
    JournalReadAuthority, ObservedHead, ProgramLifecycle, ProtocolEvidenceError, PublishedSource,
    ReadFreshness, Registry, RegistryError, RegistryReadAuthority, ReproducibleBuild, SourceStatus,
    VerifiedProgramCatalog,
};
use layerx_programs_runtime::{
    hash_bytes, ActivityBudgetBinding, CompositionRules, Deploy, HashAlgorithm, Lifecycle,
    ProgramId, ProgramVersion, UpgradePolicy, ABI_V1_VERSION, ABI_V2_VERSION, ABI_VERSION,
};
use layerx_proof::merkle::verify_path;
use layerx_wire::receipt::decode_batch_header;

use support::{
    code_hash as fixture_code_hash, deploy_fixture, deploy_fixture_in_epoch, deprecated_state,
    legacy_deploy_fixture, program as fixture_program, try_verifier_from_history, upgrade_fixture,
    verifier_for_fixture, verifier_from_history, wrong_abi_fixture, wrong_batch_id_fixture,
    TrustAnchorFixture, AUTHORITY, NOW, WASM_V1 as PROTOCOL_WASM_V1, WASM_V2 as PROTOCOL_WASM_V2,
};

const PROGRAM: [u8; 32] = [0x31; 32];
const WASM_V1: &[u8] = &[0, 97, 115, 109, 1, 0, 0, 0];
const WASM_V2: &[u8] = &[0, 97, 115, 109, 1, 0, 0, 0, 0, 3, 2, b'v', b'2'];
const FOREIGN_WASM: &[u8] = &[
    0, 97, 115, 109, 1, 0, 0, 0, 0, 8, 7, b'f', b'o', b'r', b'e', b'i', b'g', b'n',
];
const INVALID_WASM: &[u8] = &[0, 97, 115, 109, 1, 0, 0, 0, 1, 1, 0xff];

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
fn registry_resolves_historical_and_latest_code_only_from_protocol_evidence() {
    let program = fixture_program();
    let policy = UpgradePolicy::Authority(AUTHORITY);
    let deploy = deploy_fixture(PROTOCOL_WASM_V1, policy, 70, 1_700_000_070);
    let upgrade = upgrade_fixture(PROTOCOL_WASM_V1, PROTOCOL_WASM_V2, 71, 1_700_000_071);
    let verifier = verifier_for_fixture(&deploy, 70, 100, None, 1_000);
    let first = verifier
        .verify_deployment(&deploy.proof, NOW)
        .unwrap_or_else(|error| panic!("deploy evidence: {error}"));
    let second = verifier
        .verify_deployment(&upgrade.proof, NOW)
        .unwrap_or_else(|error| panic!("upgrade evidence: {error}"));
    let mut registry = Registry::new();
    registry
        .record_verified_deployment(&first)
        .unwrap_or_else(|error| panic!("verified deploy: {error}"));
    registry
        .record_verified_deployment(&second)
        .unwrap_or_else(|error| panic!("verified upgrade: {error}"));
    let historical = registry
        .resolve_deployment(first.clone())
        .unwrap_or_else(|error| panic!("historical deployment: {error}"));
    let latest = registry
        .resolve_deployment(second.clone())
        .unwrap_or_else(|error| panic!("latest deployment: {error}"));

    assert_eq!(historical.program(), program);
    assert_eq!(historical.version(), 1);
    assert_eq!(historical.code_hash(), fixture_code_hash(PROTOCOL_WASM_V1));
    assert_eq!(historical.receipt_digest(), first.receipt_digest());
    assert_eq!(historical.module(), PROTOCOL_WASM_V1);
    assert_eq!(historical.freshness().observed_sequence, 70);
    assert_eq!(historical.lifecycle(), ProgramLifecycle::Active);
    assert_eq!(latest.version(), 2);
    assert_eq!(latest.code_hash(), fixture_code_hash(PROTOCOL_WASM_V2));
    assert_eq!(latest.receipt_digest(), second.receipt_digest());
    assert_eq!(latest.module(), PROTOCOL_WASM_V2);
    let alternate = verifier
        .verify_deployment(
            &deploy_fixture(PROTOCOL_WASM_V1, policy, 73, 1_700_000_073).proof,
            NOW,
        )
        .unwrap_or_else(|error| panic!("alternate receipt evidence: {error}"));
    assert_eq!(
        registry.resolve_deployment(alternate),
        Err(RegistryError::UnverifiedRead)
    );

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
    assert_eq!(
        executable.code_hash(program),
        Some(fixture_code_hash(PROTOCOL_WASM_V2))
    );
    assert_eq!(executable.abi_version(program), Some(ABI_V1_VERSION));
    assert_eq!(
        executable.receipt_digest(program),
        Some(second.receipt_digest())
    );
    let current = verifier
        .verify_current_program(&upgrade.proof.state, program, NOW)
        .unwrap_or_else(|error| panic!("current program head: {error}"));
    let binding = ActivityBudgetBinding::new([0xa1; 32])
        .unwrap_or_else(|error| panic!("activity binding: {error}"));
    let _composition = executable
        .authorize_activity(vec![current], binding, NOW, CompositionRules::declared())
        .unwrap_or_else(|error| panic!("activity-scoped catalog: {error}"));
}

#[test]
fn forged_batch_journal_and_stale_evidence_never_create_deployment_authority() {
    let fixture = deploy_fixture(
        PROTOCOL_WASM_V1,
        UpgradePolicy::Immutable,
        70,
        1_700_000_070,
    );
    let verifier = verifier_for_fixture(&fixture, 70, 100, None, 100);
    let mut forged = fixture.proof.clone();
    forged.state.header_signature[0] ^= 1;
    assert!(matches!(
        verifier.verify_deployment(&forged, NOW),
        Err(ProtocolEvidenceError::ReceiptInclusion | ProtocolEvidenceError::ActivityInclusion)
    ));
    assert_eq!(
        verifier.verify_deployment(&fixture.proof, NOW + 100),
        Err(ProtocolEvidenceError::Stale)
    );
    let wrong_batch = wrong_batch_id_fixture(70, 1_700_000_070);
    assert_eq!(
        verifier.verify_deployment(&wrong_batch.proof, NOW),
        Err(ProtocolEvidenceError::BatchIdentifier)
    );
    let wrong_abi = wrong_abi_fixture(70, 1_700_000_070);
    assert_eq!(
        verifier.verify_deployment(&wrong_abi.proof, NOW),
        Err(ProtocolEvidenceError::CanonicalActivity)
    );
    let impossible_module =
        deploy_fixture(b"not-wasm", UpgradePolicy::Immutable, 70, 1_700_000_070);
    assert_eq!(
        verifier.verify_deployment(&impossible_module.proof, NOW),
        Err(ProtocolEvidenceError::CanonicalActivity)
    );
    let foreign_network = verifier_from_history(
        &[TrustAnchorFixture {
            protocol_version: 2,
            network_id: 43,
            epoch: 2,
            sequencer_id: fixture.sequencer_id,
            sequencer_public_key: fixture.sequencer_public_key,
            first_batch: 70,
            last_batch: 100,
            revoked_from_batch: None,
        }],
        0,
        100,
    );
    assert_eq!(
        foreign_network.verify_deployment(&fixture.proof, NOW),
        Err(ProtocolEvidenceError::TrustAnchorUnavailable)
    );
    for (protocol, epoch) in [(1, 2), (2, 3)] {
        let foreign_domain = verifier_from_history(
            &[TrustAnchorFixture {
                protocol_version: protocol,
                network_id: 42,
                epoch,
                sequencer_id: fixture.sequencer_id,
                sequencer_public_key: fixture.sequencer_public_key,
                first_batch: 70,
                last_batch: 100,
                revoked_from_batch: None,
            }],
            0,
            100,
        );
        assert_eq!(
            foreign_domain.verify_deployment(&fixture.proof, NOW),
            Err(ProtocolEvidenceError::TrustAnchorUnavailable)
        );
    }
    let revoked = verifier_for_fixture(&fixture, 70, 100, Some(71), 100);
    assert_eq!(
        revoked.verify_deployment(&fixture.proof, NOW),
        Ok(verifier
            .verify_deployment(&fixture.proof, NOW)
            .unwrap_or_else(|error| panic!("pre-revocation evidence: {error}")))
    );
    let revoked_fixture = deploy_fixture(
        PROTOCOL_WASM_V1,
        UpgradePolicy::Immutable,
        71,
        1_700_000_071,
    );
    assert_eq!(
        revoked.verify_deployment(&revoked_fixture.proof, NOW),
        Err(ProtocolEvidenceError::SequencerRevoked)
    );
}

#[test]
fn historical_deployments_replay_across_an_explicit_sequencer_rotation() {
    let historical = deploy_fixture_in_epoch(
        PROTOCOL_WASM_V1,
        UpgradePolicy::Immutable,
        70,
        1_700_000_070,
        2,
        [7; 32],
    );
    let current = deploy_fixture_in_epoch(
        PROTOCOL_WASM_V1,
        UpgradePolicy::Immutable,
        101,
        1_700_000_101,
        3,
        [8; 32],
    );
    let verifier = verifier_from_history(
        &[
            TrustAnchorFixture {
                protocol_version: 2,
                network_id: 42,
                epoch: 2,
                sequencer_id: historical.sequencer_id,
                sequencer_public_key: historical.sequencer_public_key,
                first_batch: 70,
                last_batch: 100,
                revoked_from_batch: Some(101),
            },
            TrustAnchorFixture {
                protocol_version: 2,
                network_id: 42,
                epoch: 3,
                sequencer_id: current.sequencer_id,
                sequencer_public_key: current.sequencer_public_key,
                first_batch: 101,
                last_batch: 200,
                revoked_from_batch: None,
            },
        ],
        1,
        50,
    );

    assert!(verifier
        .verify_historical_deployment(&historical.proof)
        .is_ok());
    assert_eq!(
        verifier.verify_deployment(&historical.proof, NOW),
        Err(ProtocolEvidenceError::HistoricalTrustAnchor)
    );
    assert!(verifier.verify_deployment(&current.proof, NOW).is_ok());

    let current_only = verifier_from_history(
        &[TrustAnchorFixture {
            protocol_version: 2,
            network_id: 42,
            epoch: 3,
            sequencer_id: current.sequencer_id,
            sequencer_public_key: current.sequencer_public_key,
            first_batch: 101,
            last_batch: 200,
            revoked_from_batch: None,
        }],
        0,
        50,
    );
    assert_eq!(
        current_only.verify_historical_deployment(&historical.proof),
        Err(ProtocolEvidenceError::TrustAnchorUnavailable)
    );

    assert!(matches!(
        try_verifier_from_history(
            &[
                TrustAnchorFixture {
                    protocol_version: 2,
                    network_id: 42,
                    epoch: 2,
                    sequencer_id: historical.sequencer_id,
                    sequencer_public_key: historical.sequencer_public_key,
                    first_batch: 70,
                    last_batch: 100,
                    revoked_from_batch: None,
                },
                TrustAnchorFixture {
                    protocol_version: 2,
                    network_id: 42,
                    epoch: 2,
                    sequencer_id: historical.sequencer_id,
                    sequencer_public_key: historical.sequencer_public_key,
                    first_batch: 80,
                    last_batch: 110,
                    revoked_from_batch: None,
                },
            ],
            1,
            1_000,
        ),
        Err(ProtocolEvidenceError::TrustAnchorAmbiguous)
    ));
}

#[test]
fn mismatched_receipt_invalid_module_stale_head_and_deprecation_are_fail_closed() {
    let fixture = legacy_deploy_fixture(
        INVALID_WASM,
        UpgradePolicy::Authority(AUTHORITY),
        70,
        1_700_000_070,
    );
    let other = deploy_fixture(
        PROTOCOL_WASM_V1,
        UpgradePolicy::Authority(AUTHORITY),
        71,
        1_700_000_071,
    );
    let verifier = verifier_for_fixture(&fixture, 70, 100, None, 1_000);
    let mut swapped_receipt = fixture.proof.clone();
    swapped_receipt.state.receipt = other.proof.state.receipt.clone();
    let signed_header = decode_batch_header(&swapped_receipt.state.header)
        .unwrap_or_else(|error| panic!("decode signed header: {error:?}"));
    assert!(verify_path(
        &swapped_receipt.state.receipt,
        &swapped_receipt.state.receipt_proof,
        &signed_header.receipt_merkle_root(),
    )
    .is_err());
    let swapped_head =
        verifier.verify_current_program(&swapped_receipt.state, fixture_program(), NOW);
    assert!(
        matches!(swapped_head, Err(ProtocolEvidenceError::ReceiptInclusion)),
        "unexpected swapped-receipt head result: {swapped_head:?}"
    );
    let swapped_result = verifier.verify_deployment(&swapped_receipt, NOW);
    assert!(
        matches!(swapped_result, Err(ProtocolEvidenceError::ReceiptInclusion)),
        "unexpected swapped-receipt result: {swapped_result:?}"
    );
    let malformed_evidence = verifier
        .verify_deployment(&fixture.proof, NOW)
        .unwrap_or_else(|error| panic!("malformed module protocol evidence: {error}"));
    let mut executable = VerifiedProgramCatalog::declared()
        .unwrap_or_else(|error| panic!("verified catalog: {error}"));
    assert!(matches!(
        executable.admit(malformed_evidence),
        Err(ExecutableAdmissionError::Validation(_))
    ));
    assert!(executable.is_empty());

    let valid = deploy_fixture(
        PROTOCOL_WASM_V1,
        UpgradePolicy::Authority(AUTHORITY),
        72,
        1_700_000_072,
    );
    let evidence = verifier
        .verify_deployment(&valid.proof, NOW)
        .unwrap_or_else(|error| panic!("valid deployment: {error}"));
    let active = verifier
        .verify_current_program(&valid.proof.state, fixture_program(), NOW)
        .unwrap_or_else(|error| panic!("active head: {error}"));
    let binding = ActivityBudgetBinding::new([0xa2; 32])
        .unwrap_or_else(|error| panic!("activity binding: {error}"));
    let mut stale_catalog =
        VerifiedProgramCatalog::declared().unwrap_or_else(|error| panic!("stale catalog: {error}"));
    stale_catalog
        .admit(evidence.clone())
        .unwrap_or_else(|error| panic!("catalog admission: {error}"));
    assert!(matches!(
        stale_catalog.authorize_activity(
            vec![active.clone()],
            binding,
            active.valid_until_ms() + 1,
            CompositionRules::declared(),
        ),
        Err(ExecutableAdmissionError::EvidenceExpired { .. })
    ));

    let deprecated = verifier
        .verify_current_program(
            &deprecated_state(&valid, 1_700_000_073),
            fixture_program(),
            NOW,
        )
        .unwrap_or_else(|error| panic!("deprecated head: {error}"));
    let mut deprecated_catalog = VerifiedProgramCatalog::declared()
        .unwrap_or_else(|error| panic!("deprecated catalog: {error}"));
    deprecated_catalog
        .admit(evidence)
        .unwrap_or_else(|error| panic!("catalog admission: {error}"));
    assert!(matches!(
        deprecated_catalog.authorize_activity(
            vec![deprecated],
            binding,
            NOW,
            CompositionRules::declared(),
        ),
        Err(ExecutableAdmissionError::InactiveLifecycle {
            lifecycle: ProgramLifecycle::Deprecated,
        })
    ));
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
    let mut lifecycle =
        Lifecycle::declared().unwrap_or_else(|error| panic!("lifecycle construction: {error}"));
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
fn registry_replay_refuses_downgrade_without_rewriting_historical_version() {
    let program = program();
    let first = record_with_abi(
        program,
        1,
        None,
        WASM_V1,
        UpgradePolicy::Authority([0x51; 32]),
        80,
        ABI_V2_VERSION,
    );
    let downgrade = record_with_abi(
        program,
        2,
        Some(first.new_code_hash),
        WASM_V2,
        UpgradePolicy::Authority([0x51; 32]),
        81,
        ABI_V1_VERSION,
    );
    let mut registry = Registry::new();
    assert_eq!(
        registry.replay_journal(&[first.clone(), downgrade]),
        Err(RegistryError::AbiVersion(
            layerx_programs_runtime::AbiVersionRefusal::Downgrade {
                current: ABI_V2_VERSION,
                requested: ABI_V1_VERSION,
            },
        )),
    );
    assert_eq!(registry.latest_version(program), Ok(1));
    assert_eq!(
        registry
            .entry_for_wind_down(program)
            .map(|entry| entry.versions[0].abi_version),
        Ok(ABI_V2_VERSION),
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
