use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use layerx_types::payload::{ActivityType, ModuleId, ModuleRegistration, ModuleRegistry};
use layerx_types::vectors::Corpus;
use layerx_wire::activity::decode_signed;
use layerx_wire::hash::{activity_id, canonical_activity, domain, payload_hash, Domain};
use layerx_wire::sign::preimage;

fn repository_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../..")
}

fn registry() -> ModuleRegistry {
    let module_maximums = [
        (ModuleId::Asset, 8),
        (ModuleId::Escrow, 7),
        (ModuleId::Budget, 7),
        (ModuleId::Stream, 7),
        (ModuleId::Service, 13),
        (ModuleId::Perps, 11),
    ];
    let registrations: Vec<_> = module_maximums
        .into_iter()
        .map(|(module, maximum)| {
            let values: Vec<_> = (1..=maximum)
                .map(|ordinal| {
                    let Ok(value) = ActivityType::new(module, ordinal) else {
                        panic!("valid activity type rejected");
                    };
                    value
                })
                .collect();
            let Ok(registration) = ModuleRegistration::new(module, &values) else {
                panic!("valid registration rejected");
            };
            registration
        })
        .collect();
    let Ok(registry) = ModuleRegistry::new(&registrations) else {
        panic!("valid registry rejected");
    };
    registry
}

#[test]
fn activity_and_payload_hashes_match_every_replay_vector() {
    let Ok(corpus) = Corpus::load(&repository_root()) else {
        panic!("published corpus failed to load");
    };
    let registry = registry();
    for (index, (activity_bytes, receipt_bytes)) in corpus
        .replay
        .canonical_activities
        .iter()
        .zip(&corpus.replay.expected_receipts)
        .enumerate()
    {
        let Ok(activity) = decode_signed(activity_bytes, &registry) else {
            panic!("activity {index} failed decode");
        };
        let expected_activity_id: [u8; 32] = receipt_bytes[10..42].try_into().unwrap_or([0; 32]);
        assert_eq!(
            activity_id(&activity),
            Ok(expected_activity_id),
            "activity {index}"
        );
        assert_eq!(
            payload_hash(&activity),
            Ok(activity.payload_hash()),
            "payload {index}"
        );
    }
}

#[test]
fn all_domains_separate_identical_canonical_bytes() {
    let Ok(corpus) = Corpus::load(&repository_root()) else {
        panic!("published corpus failed to load");
    };
    let Ok(activity) = decode_signed(&corpus.replay.canonical_activities[0], &registry()) else {
        panic!("activity failed decode");
    };
    let Ok(canonical) = canonical_activity(&activity) else {
        panic!("canonical activity failed encode");
    };
    let domains = [
        Domain::ActivityId,
        Domain::PayloadHash,
        Domain::SignaturePreimage,
        Domain::AuthorityHash,
        Domain::ContextHash,
        Domain::MerkleLeaf,
        Domain::MerkleInternal,
        Domain::BatchHeader,
        Domain::Receipt,
        Domain::CheckpointCertificate,
        Domain::AccountId,
        Domain::DidId,
        Domain::EvmPayoutBinding,
        Domain::StateLeaf,
        Domain::StateNode,
        Domain::StateRootChain,
        Domain::Snapshot,
        Domain::DaChunk,
        Domain::DaChallenge,
        Domain::GuarantorAttestation,
    ];
    let digests: Vec<_> = domains
        .into_iter()
        .map(|purpose| domain(purpose, &canonical))
        .collect();
    assert!(digests.iter().all(Result::is_ok));
    for (index, left) in digests.iter().enumerate() {
        for right in &digests[index + 1..] {
            assert_ne!(left, right);
        }
    }
}

#[test]
fn signing_preimage_binds_network_and_protocol_version() {
    let Ok(corpus) = Corpus::load(&repository_root()) else {
        panic!("published corpus failed to load");
    };
    let registry = registry();
    let original_bytes = &corpus.replay.canonical_activities[0];
    let Ok(original) = decode_signed(original_bytes, &registry) else {
        panic!("activity failed decode");
    };
    let mut other_network_bytes = original_bytes.clone();
    other_network_bytes[9..13].copy_from_slice(&78_u32.to_be_bytes());
    let Ok(other_network) = decode_signed(&other_network_bytes, &registry) else {
        panic!("other-network activity failed decode");
    };
    assert_ne!(preimage(&original), preimage(&other_network));

    let mut other_version = original_bytes.clone();
    other_version[1] = 2;
    assert!(decode_signed(&other_version, &registry).is_err());
}

#[test]
fn raw_text_and_debug_values_cannot_enter_consensus_hashing() {
    let dependency_dir = std::env::current_exe()
        .ok()
        .and_then(|path| path.parent().map(Path::to_path_buf));
    let Some(dependency_dir) = dependency_dir else {
        panic!("dependency directory unavailable");
    };
    let rlib = fs::read_dir(&dependency_dir).ok().and_then(|entries| {
        entries
            .filter_map(Result::ok)
            .map(|entry| entry.path())
            .filter(|path| {
                path.file_name().is_some_and(|name| {
                    name.to_string_lossy().starts_with("liblayerx_wire-")
                        && path
                            .extension()
                            .is_some_and(|extension| extension == "rlib")
                })
            })
            .max_by_key(|path| {
                fs::metadata(path)
                    .and_then(|metadata| metadata.modified())
                    .ok()
            })
    });
    let Some(rlib) = rlib else {
        panic!("layerx-wire rlib unavailable");
    };
    let source = std::env::temp_dir().join(format!("layerx-hash-gate-{}.rs", std::process::id()));
    let binary = source.with_extension("bin");
    assert!(fs::write(
        &source,
        "extern crate layerx_wire;\nuse layerx_wire::hash::{domain, Domain};\nfn main() { let _ = domain(Domain::ActivityId, b\"debug text\"); }\n",
    )
    .is_ok());
    let output = Command::new("rustc")
        .arg("--edition=2021")
        .arg(&source)
        .arg("--extern")
        .arg(format!("layerx_wire={}", rlib.display()))
        .arg("-L")
        .arg(format!("dependency={}", dependency_dir.display()))
        .arg("-o")
        .arg(&binary)
        .output();
    let _ = fs::remove_file(source);
    let _ = fs::remove_file(binary);
    let Ok(output) = output else {
        panic!("rustc unavailable for hash input gate");
    };
    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("expected `&CanonicalBytes`"));
}
