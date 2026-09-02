#[allow(dead_code)]
#[path = "../../../../programs/crates/layerx-programs-registry/tests/support/mod.rs"]
mod support;

use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use layerx_platform_registry::{
    DeploymentEnvelope, FileDeploymentJournal, JournalLoad, UnitDefect, UnitPart, WriteStep,
};
use layerx_programs::{
    hex, DeploymentJournal as _, ObservedHead, ProtocolDeploymentVerifier, Registry, RegistryError,
    VerifiedDeploymentEvidence,
};
use layerx_programs_runtime::UpgradePolicy;

use support::{
    deploy_fixture, program, upgrade_fixture, verifier_for_fixture, AUTHORITY, NOW, WASM_V1,
    WASM_V2,
};

const UNEQUAL_SETS: &str = "every deployment projection must have one protocol admission proof";

struct Evidence {
    verifier: ProtocolDeploymentVerifier,
    deploy: VerifiedDeploymentEvidence,
    upgrade: VerifiedDeploymentEvidence,
}

impl Evidence {
    fn all(&self) -> Vec<&VerifiedDeploymentEvidence> {
        vec![&self.deploy, &self.upgrade]
    }
}

fn evidence() -> Evidence {
    let policy = UpgradePolicy::Authority(AUTHORITY);
    let deploy = deploy_fixture(WASM_V1, policy, 70, 1_700_000_070);
    let upgrade = upgrade_fixture(WASM_V1, WASM_V2, 71, 1_700_000_071);
    let verifier = verifier_for_fixture(&deploy, 70, 100, None, 1_000);
    let deploy = verifier
        .verify_deployment(&deploy.proof, NOW)
        .unwrap_or_else(|error| panic!("deploy evidence: {error}"));
    let upgrade = verifier
        .verify_deployment(&upgrade.proof, NOW)
        .unwrap_or_else(|error| panic!("upgrade evidence: {error}"));
    Evidence {
        verifier,
        deploy,
        upgrade,
    }
}

fn journal_root(name: &str) -> PathBuf {
    static NEXT: AtomicU64 = AtomicU64::new(0);
    let root = Path::new(env!("CARGO_TARGET_TMPDIR")).join(format!(
        "deployment-journal-{name}-{}-{}",
        std::process::id(),
        NEXT.fetch_add(1, Ordering::Relaxed)
    ));
    let _ = fs::remove_dir_all(&root);
    root
}

fn open(root: &Path) -> FileDeploymentJournal {
    FileDeploymentJournal::open(root.to_path_buf())
        .unwrap_or_else(|error| panic!("open journal: {error}"))
}

fn append(journal: &FileDeploymentJournal, evidence: &VerifiedDeploymentEvidence) -> [u8; 32] {
    journal
        .append(evidence)
        .unwrap_or_else(|error| panic!("append: {error}"))
}

fn load(journal: &FileDeploymentJournal) -> JournalLoad {
    journal
        .load()
        .unwrap_or_else(|error| panic!("load journal: {error}"))
}

fn digests(loaded: &JournalLoad) -> BTreeSet<[u8; 32]> {
    loaded
        .units
        .iter()
        .map(DeploymentEnvelope::receipt_digest)
        .collect()
}

fn quarantine(loaded: &JournalLoad) -> BTreeMap<[u8; 32], (Vec<PathBuf>, UnitDefect)> {
    loaded
        .quarantined
        .iter()
        .map(|unit| {
            (
                unit.receipt_digest,
                (unit.paths.clone(), unit.defect.clone()),
            )
        })
        .collect()
}

fn unit_path(root: &Path, digest: [u8; 32], suffix: &str) -> PathBuf {
    root.join(format!("{}.{suffix}", hex::encode(&digest)))
}

fn write(path: &Path, bytes: &[u8]) {
    fs::write(path, bytes).unwrap_or_else(|error| panic!("write {}: {error}", path.display()));
}

fn read(path: &Path) -> Vec<u8> {
    fs::read(path).unwrap_or_else(|error| panic!("read {}: {error}", path.display()))
}

fn record_frame_end(envelope: &DeploymentEnvelope) -> usize {
    let bytes = envelope.canonical_encoding();
    bytes.len() - 32 - 4 - envelope.proof().canonical_encoding().len()
}

#[derive(Debug, Eq, PartialEq)]
struct Projection {
    digests: Vec<[u8; 32]>,
    records: Vec<Vec<u8>>,
    latest: u32,
    resolved: Vec<[u8; 32]>,
}

fn project(mut evidence: Vec<VerifiedDeploymentEvidence>, records: Vec<Vec<u8>>) -> Projection {
    let mut ordered = evidence
        .drain(..)
        .zip(records)
        .collect::<Vec<(VerifiedDeploymentEvidence, Vec<u8>)>>();
    ordered.sort_by_key(|(evidence, _)| (evidence.record().sequence, evidence.version()));
    let mut registry = Registry::new();
    for (evidence, _) in &ordered {
        registry
            .record_verified_deployment(evidence)
            .unwrap_or_else(|error| panic!("projection: {error}"));
    }
    let latest = registry
        .latest_version(program())
        .unwrap_or_else(|error| panic!("latest version: {error}"));
    let digests = ordered
        .iter()
        .map(|(evidence, _)| evidence.receipt_digest())
        .collect();
    let records = ordered.iter().map(|(_, record)| record.clone()).collect();
    let resolved = ordered
        .into_iter()
        .map(|(evidence, _)| {
            registry
                .resolve_deployment(evidence)
                .map(|resolved| resolved.receipt_digest())
                .unwrap_or_else(|error| panic!("resolve deployment: {error}"))
        })
        .collect();
    Projection {
        digests,
        records,
        latest,
        resolved,
    }
}

fn expected_projection(evidence: &Evidence) -> Projection {
    let records = evidence
        .all()
        .iter()
        .map(|evidence| evidence.record().canonical_encoding())
        .collect();
    project(evidence.all().into_iter().cloned().collect(), records)
}

fn replay(journal: &FileDeploymentJournal, verifier: &ProtocolDeploymentVerifier) -> Projection {
    let mut evidence = Vec::new();
    let mut records = Vec::new();
    for proof in journal
        .proofs()
        .unwrap_or_else(|error| panic!("replay proofs: {error}"))
    {
        let verified = verifier
            .verify_historical_deployment(&proof)
            .unwrap_or_else(|error| panic!("replay verification: {error}"));
        journal
            .audit_projection(&verified)
            .unwrap_or_else(|error| panic!("replay audit: {error}"));
        records.push(
            journal
                .canonical_record(verified.receipt_digest())
                .unwrap_or_else(|error| panic!("replay record: {error}")),
        );
        evidence.push(verified);
    }
    project(evidence, records)
}

fn interrupted_defect(step: WriteStep) -> Option<UnitDefect> {
    match step {
        WriteStep::CreateTemporary | WriteStep::SyncDirectory => None,
        WriteStep::WriteRecord => Some(UnitDefect::Interrupted(UnitPart::Record)),
        WriteStep::WriteProof => Some(UnitDefect::Interrupted(UnitPart::Proof)),
        WriteStep::WriteSeal => Some(UnitDefect::Interrupted(UnitPart::Seal)),
        WriteStep::SyncTemporary | WriteStep::Commit => {
            Some(UnitDefect::Interrupted(UnitPart::Commit))
        }
    }
}

#[test]
fn interruption_at_every_write_step_recovers_on_restart_without_repair() {
    let evidence = evidence();
    let expected = expected_projection(&evidence);
    for step in WriteStep::ALL {
        let root = journal_root("interrupt");
        let first = append(&open(&root), &evidence.deploy);
        let interrupted = open(&root).interrupt_before(step);
        let error = match interrupted.append(&evidence.upgrade) {
            Ok(digest) => panic!("{step} did not interrupt {}", hex::encode(&digest)),
            Err(error) => error,
        };
        assert!(error.contains(&step.to_string()), "{step}: {error}");
        drop(interrupted);

        let restarted = open(&root);
        let loaded = load(&restarted);
        let committed = step == WriteStep::SyncDirectory;
        let second = evidence.upgrade.receipt_digest();
        let mut expected_units = BTreeSet::from([first]);
        if committed {
            expected_units.insert(second);
        }
        assert_eq!(digests(&loaded), expected_units, "{step}");
        assert!(
            loaded.leftovers.is_empty(),
            "{step}: {:?}",
            loaded.leftovers
        );
        assert_eq!(
            unit_path(&root, second, "envelope").exists(),
            committed,
            "{step}"
        );
        match interrupted_defect(step) {
            None => assert!(loaded.quarantined.is_empty(), "{step}: {loaded:?}"),
            Some(defect) => {
                let quarantined = quarantine(&loaded);
                assert_eq!(quarantined.len(), 1, "{step}: {loaded:?}");
                let (paths, reported) = quarantined.get(&second).unwrap_or_else(|| {
                    panic!("{step}: unit {} not reported", hex::encode(&second))
                });
                assert_eq!(reported, &defect, "{step}");
                assert_eq!(
                    paths,
                    &vec![unit_path(&root, second, "envelope.tmp")],
                    "{step}"
                );
                assert_eq!(
                    reported.to_string(),
                    format!(
                        "publication was interrupted before its {}",
                        match defect {
                            UnitDefect::Interrupted(part) => part,
                            _ => unreachable!(),
                        }
                    ),
                    "{step}"
                );
            }
        }
        let proofs = restarted
            .proofs()
            .unwrap_or_else(|error| panic!("{step}: proofs after interruption: {error}"));
        assert_eq!(proofs.len(), expected_units.len(), "{step}");
        for unit in &loaded.units {
            let source = evidence
                .all()
                .into_iter()
                .find(|candidate| candidate.receipt_digest() == unit.receipt_digest())
                .unwrap_or_else(|| panic!("{step}: unknown unit"));
            assert_eq!(unit.record(), source.record(), "{step}");
            assert_eq!(unit.proof(), source.proof(), "{step}");
        }

        assert_eq!(append(&restarted, &evidence.upgrade), second, "{step}");
        let recovered = load(&restarted);
        assert_eq!(
            digests(&recovered),
            BTreeSet::from([first, second]),
            "{step}"
        );
        assert!(recovered.quarantined.is_empty(), "{step}: {recovered:?}");
        assert!(recovered.leftovers.is_empty(), "{step}");
        assert!(!unit_path(&root, second, "envelope.tmp").exists(), "{step}");
        assert_eq!(replay(&restarted, &evidence.verifier), expected, "{step}");
        fs::remove_dir_all(&root).unwrap_or_else(|error| panic!("cleanup: {error}"));
    }
}

#[test]
fn replaying_the_journal_reproduces_the_projection_of_its_evidence() {
    let evidence = evidence();
    let expected = expected_projection(&evidence);
    let reference_root = journal_root("reference");
    let reference = open(&reference_root);
    append(&reference, &evidence.upgrade);
    append(&reference, &evidence.deploy);
    assert_eq!(replay(&reference, &evidence.verifier), expected);

    let recovered_root = journal_root("recovered");
    append(&open(&recovered_root), &evidence.deploy);
    assert!(open(&recovered_root)
        .interrupt_before(WriteStep::Commit)
        .append(&evidence.upgrade)
        .is_err());
    let recovered = open(&recovered_root);
    assert_eq!(
        recovered
            .proofs()
            .unwrap_or_else(|error| panic!("proofs: {error}"))
            .len(),
        1
    );
    append(&recovered, &evidence.upgrade);
    assert_eq!(replay(&recovered, &evidence.verifier), expected);
    assert_eq!(load(&recovered).units, load(&open(&reference_root)).units);
    for evidence in evidence.all() {
        assert_eq!(
            recovered.canonical_record(evidence.receipt_digest()),
            reference.canonical_record(evidence.receipt_digest())
        );
    }
    for root in [reference_root, recovered_root] {
        fs::remove_dir_all(root).unwrap_or_else(|error| panic!("cleanup: {error}"));
    }
}

#[test]
fn envelope_encoding_round_trips_and_names_the_missing_part() {
    let evidence = evidence();
    let envelope = DeploymentEnvelope::from_evidence(&evidence.deploy);
    let bytes = envelope.canonical_encoding();
    assert_eq!(DeploymentEnvelope::decode(&bytes), Ok(envelope.clone()));
    assert_eq!(envelope.receipt_digest(), evidence.deploy.receipt_digest());
    assert_eq!(envelope.record(), evidence.deploy.record());
    assert_eq!(envelope.proof(), evidence.deploy.proof());
    let record_end = record_frame_end(&envelope);

    assert_eq!(
        DeploymentEnvelope::decode(&[]),
        Err(UnitDefect::Missing(UnitPart::Record))
    );
    assert_eq!(
        DeploymentEnvelope::decode(&bytes[..record_end - 1]),
        Err(UnitDefect::Missing(UnitPart::Record))
    );
    assert_eq!(
        DeploymentEnvelope::decode(&bytes[..record_end]),
        Err(UnitDefect::Missing(UnitPart::Proof))
    );
    assert_eq!(
        DeploymentEnvelope::decode(&bytes[..bytes.len() - 33]),
        Err(UnitDefect::Missing(UnitPart::Proof))
    );
    assert_eq!(
        DeploymentEnvelope::decode(&bytes[..bytes.len() - 32]),
        Err(UnitDefect::Missing(UnitPart::Seal))
    );
    assert_eq!(
        DeploymentEnvelope::decode(&bytes[..bytes.len() - 1]),
        Err(UnitDefect::Missing(UnitPart::Seal))
    );

    let mut trailing = bytes.clone();
    trailing.push(0);
    assert!(matches!(
        DeploymentEnvelope::decode(&trailing),
        Err(UnitDefect::Corrupt {
            part: UnitPart::Seal,
            ..
        })
    ));
    let mut unsealed = bytes.clone();
    let last = unsealed.len() - 1;
    unsealed[last] ^= 0x01;
    assert!(matches!(
        DeploymentEnvelope::decode(&unsealed),
        Err(UnitDefect::Corrupt {
            part: UnitPart::Seal,
            ..
        })
    ));
    let mut foreign = bytes.clone();
    foreign[0] ^= 0x01;
    assert!(matches!(
        DeploymentEnvelope::decode(&foreign),
        Err(UnitDefect::Corrupt {
            part: UnitPart::Record,
            ..
        })
    ));

    assert_eq!(
        UnitDefect::Missing(UnitPart::Proof).to_string(),
        "the committed unit has no admission proof"
    );
    assert_eq!(
        UnitDefect::Interrupted(UnitPart::Commit).to_string(),
        "publication was interrupted before its commit"
    );
    assert_eq!(WriteStep::ALL.len(), 7);
    assert_eq!(
        WriteStep::ALL
            .iter()
            .map(ToString::to_string)
            .collect::<BTreeSet<_>>()
            .len(),
        7
    );
}

#[test]
fn startup_quarantines_defective_units_and_loads_the_rest() {
    let evidence = evidence();
    let root = journal_root("quarantine");
    let journal = open(&root);
    let first = append(&journal, &evidence.deploy);
    let second = append(&journal, &evidence.upgrade);
    let envelope = DeploymentEnvelope::from_evidence(&evidence.upgrade);
    let second_path = unit_path(&root, second, "envelope");
    let whole = read(&second_path);

    write(&second_path, &whole[..record_frame_end(&envelope) + 3]);
    let loaded = load(&open(&root));
    assert_eq!(digests(&loaded), BTreeSet::from([first]));
    assert_eq!(
        quarantine(&loaded),
        BTreeMap::from([(
            second,
            (
                vec![second_path.clone()],
                UnitDefect::Missing(UnitPart::Proof)
            )
        )])
    );
    assert_eq!(open(&root).proofs(), Err(UNEQUAL_SETS.to_owned()));
    assert_eq!(
        open(&root).canonical_record(second),
        Err(RegistryError::JournalUnavailable)
    );
    assert!(open(&root)
        .audit_projection(&evidence.upgrade)
        .is_err_and(|error| error.contains("is corrupt")));

    write(&second_path, &whole[..whole.len() - 32]);
    let loaded = load(&open(&root));
    assert_eq!(digests(&loaded), BTreeSet::from([first]));
    assert_eq!(
        quarantine(&loaded).get(&second).map(|(_, defect)| defect),
        Some(&UnitDefect::Missing(UnitPart::Seal))
    );
    assert!(open(&root)
        .proofs()
        .is_err_and(|error| error.contains("ends before its seal")));

    assert_eq!(append(&open(&root), &evidence.upgrade), second);
    assert_eq!(
        digests(&load(&open(&root))),
        BTreeSet::from([first, second])
    );

    let corrupt = [0xCC; 32];
    let misfiled = [0xDD; 32];
    let unreadable = [0xEE; 32];
    write(&unit_path(&root, corrupt, "envelope"), b"not an envelope");
    write(
        &unit_path(&root, misfiled, "envelope"),
        &read(&unit_path(&root, first, "envelope")),
    );
    fs::create_dir(unit_path(&root, unreadable, "envelope"))
        .unwrap_or_else(|error| panic!("unreadable unit: {error}"));
    let stale = unit_path(&root, first, "envelope.tmp");
    write(&stale, b"abandoned attempt");
    write(&root.join("head.tmp"), b"");
    write(&root.join("notes.txt"), b"");
    fs::create_dir(root.join("program-state")).unwrap_or_else(|error| panic!("state: {error}"));

    let loaded = load(&open(&root));
    assert_eq!(digests(&loaded), BTreeSet::from([first, second]));
    assert_eq!(loaded.leftovers, vec![stale.clone()]);
    let quarantined = quarantine(&loaded);
    assert_eq!(quarantined.len(), 3);
    assert!(matches!(
        quarantined.get(&corrupt),
        Some((paths, UnitDefect::Corrupt { part: UnitPart::Record, .. }))
            if paths == &vec![unit_path(&root, corrupt, "envelope")]
    ));
    assert_eq!(
        quarantined.get(&misfiled),
        Some(&(
            vec![unit_path(&root, misfiled, "envelope")],
            UnitDefect::Misfiled { claimed: first }
        ))
    );
    assert!(matches!(
        quarantined.get(&unreadable),
        Some((paths, UnitDefect::Unreadable(_)))
            if paths == &vec![unit_path(&root, unreadable, "envelope")]
    ));
    for unit in &loaded.quarantined {
        let report = unit.to_string();
        assert!(
            report.contains(&hex::encode(&unit.receipt_digest)),
            "{report}"
        );
        assert!(report.contains(&unit.defect.to_string()), "{report}");
    }
    assert!(open(&root).proofs().is_err());

    for digest in [corrupt, misfiled] {
        open(&root)
            .discard(digest)
            .unwrap_or_else(|error| panic!("discard: {error}"));
    }
    fs::remove_dir(unit_path(&root, unreadable, "envelope"))
        .unwrap_or_else(|error| panic!("remove unreadable: {error}"));
    fs::remove_file(&stale).unwrap_or_else(|error| panic!("remove stale: {error}"));
    let loaded = load(&open(&root));
    assert!(loaded.quarantined.is_empty(), "{loaded:?}");
    assert!(loaded.leftovers.is_empty());
    assert_eq!(
        replay(&open(&root), &evidence.verifier),
        expected_projection(&evidence)
    );
    assert_eq!(
        open(&root).discard([0x11; 32]),
        Err(format!(
            "could not discard {}: no such unit",
            unit_path(&root, [0x11; 32], "envelope").display()
        ))
    );
    fs::remove_dir_all(&root).unwrap_or_else(|error| panic!("cleanup: {error}"));
}

#[test]
fn legacy_two_file_units_load_when_complete_and_quarantine_when_not() {
    let evidence = evidence();
    let root = journal_root("legacy");
    let first = evidence.deploy.receipt_digest();
    let second = evidence.upgrade.receipt_digest();
    let first_record = evidence.deploy.record().canonical_encoding();
    let first_proof = evidence.deploy.proof().canonical_encoding();
    open(&root);
    write(&unit_path(&root, first, "deployment"), &first_record);
    write(&unit_path(&root, first, "admission"), &first_proof);

    let journal = open(&root);
    let loaded = load(&journal);
    assert_eq!(
        loaded.units,
        vec![DeploymentEnvelope::from_evidence(&evidence.deploy)]
    );
    assert!(loaded.quarantined.is_empty());
    assert_eq!(journal.canonical_record(first), Ok(first_record.clone()));
    journal
        .audit_projection(&evidence.deploy)
        .unwrap_or_else(|error| panic!("legacy audit: {error}"));
    assert_eq!(journal.proofs(), Ok(vec![evidence.deploy.proof().clone()]));

    let lone_record = unit_path(&root, second, "deployment");
    write(
        &lone_record,
        &evidence.upgrade.record().canonical_encoding(),
    );
    let loaded = load(&open(&root));
    assert_eq!(digests(&loaded), BTreeSet::from([first]));
    assert_eq!(
        quarantine(&loaded),
        BTreeMap::from([(
            second,
            (
                vec![lone_record.clone()],
                UnitDefect::Missing(UnitPart::Proof)
            )
        )])
    );
    assert_eq!(open(&root).proofs(), Err(UNEQUAL_SETS.to_owned()));
    fs::remove_file(&lone_record).unwrap_or_else(|error| panic!("remove: {error}"));

    let lone_proof = unit_path(&root, second, "admission");
    write(&lone_proof, &evidence.upgrade.proof().canonical_encoding());
    let loaded = load(&open(&root));
    assert_eq!(
        quarantine(&loaded),
        BTreeMap::from([(
            second,
            (
                vec![lone_proof.clone()],
                UnitDefect::Missing(UnitPart::Record)
            )
        )])
    );
    assert_eq!(open(&root).proofs(), Err(UNEQUAL_SETS.to_owned()));
    fs::remove_file(&lone_proof).unwrap_or_else(|error| panic!("remove: {error}"));

    let interrupted = unit_path(&root, second, "admission.tmp");
    write(&interrupted, b"partial");
    let loaded = load(&open(&root));
    assert_eq!(digests(&loaded), BTreeSet::from([first]));
    assert_eq!(
        quarantine(&loaded),
        BTreeMap::from([(
            second,
            (
                vec![interrupted.clone()],
                UnitDefect::Interrupted(UnitPart::Record)
            )
        )])
    );
    assert_eq!(
        open(&root).proofs(),
        Ok(vec![evidence.deploy.proof().clone()])
    );

    let journal = open(&root);
    assert_eq!(append(&journal, &evidence.upgrade), second);
    let loaded = load(&journal);
    assert_eq!(digests(&loaded), BTreeSet::from([first, second]));
    assert!(loaded.quarantined.is_empty());
    assert_eq!(loaded.leftovers, vec![interrupted.clone()]);
    assert_eq!(
        replay(&journal, &evidence.verifier),
        expected_projection(&evidence)
    );

    assert_eq!(append(&journal, &evidence.deploy), first);
    let loaded = load(&journal);
    assert_eq!(digests(&loaded), BTreeSet::from([first, second]));
    assert_eq!(
        loaded.leftovers.iter().collect::<BTreeSet<_>>(),
        BTreeSet::from([
            &unit_path(&root, first, "deployment"),
            &unit_path(&root, first, "admission"),
            &interrupted,
        ])
    );

    journal
        .discard(second)
        .unwrap_or_else(|error| panic!("discard: {error}"));
    journal
        .discard(first)
        .unwrap_or_else(|error| panic!("discard: {error}"));
    assert_eq!(load(&journal), JournalLoad::default());
    assert!(!interrupted.exists());
    assert_eq!(
        journal.canonical_record(first),
        Err(RegistryError::JournalUnavailable)
    );
    fs::remove_dir_all(&root).unwrap_or_else(|error| panic!("cleanup: {error}"));
}

#[test]
fn observed_head_round_trips_and_refuses_absent_observations() {
    let root = journal_root("head");
    let journal = open(&root);
    assert_eq!(
        journal.observed_head(),
        Err(RegistryError::JournalUnavailable)
    );
    assert!(journal
        .refresh_head(ObservedHead {
            sequence: 0,
            observed_at: NOW,
        })
        .is_err());
    let head = ObservedHead {
        sequence: 71,
        observed_at: NOW,
    };
    journal
        .refresh_head(head)
        .unwrap_or_else(|error| panic!("refresh head: {error}"));
    assert_eq!(journal.observed_head(), Ok(head));
    assert_eq!(load(&journal), JournalLoad::default());
    fs::remove_dir_all(&root).unwrap_or_else(|error| panic!("cleanup: {error}"));
}
