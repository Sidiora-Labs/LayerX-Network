use std::fs;
use std::path::{Path, PathBuf};
#[path = "../../../tests/support/rustc.rs"]
mod rustc;

use layerx_types::verify::{Projection, VerificationLevel};

fn dependency_rlib() -> (PathBuf, PathBuf) {
    let dependency_dir = std::env::current_exe()
        .ok()
        .and_then(|path| path.parent().map(Path::to_path_buf));
    let Some(dependency_dir) = dependency_dir else {
        panic!("test dependency directory unavailable");
    };
    let rlib = fs::read_dir(&dependency_dir).ok().and_then(|entries| {
        entries
            .filter_map(Result::ok)
            .map(|entry| entry.path())
            .filter(|path| {
                path.file_name().is_some_and(|name| {
                    name.to_string_lossy().starts_with("liblayerx_types-")
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
        panic!("layerx-types rlib unavailable");
    };
    (dependency_dir, rlib)
}

fn compile_fails(name: &str, source_text: &str, expected: &str) {
    let (dependency_dir, rlib) = dependency_rlib();
    let source = std::env::temp_dir().join(format!(
        "layerx-verification-{name}-{}.rs",
        std::process::id()
    ));
    let binary = source.with_extension("bin");
    assert!(fs::write(&source, source_text).is_ok());
    let output = rustc::command(&dependency_dir)
        .arg("--edition=2021")
        .arg(&source)
        .arg("--extern")
        .arg(format!("layerx_types={}", rlib.display()))
        .arg("-L")
        .arg(format!("dependency={}", dependency_dir.display()))
        .arg("-o")
        .arg(&binary)
        .output();
    let _ = fs::remove_file(&source);
    let _ = fs::remove_file(&binary);
    let Ok(output) = output else {
        panic!("rustc unavailable for type-boundary test");
    };
    assert!(!output.status.success(), "negative type case compiled");
    assert!(String::from_utf8_lossy(&output.stderr).contains(expected));
}

#[test]
fn verification_levels_form_the_declared_order() {
    let levels = [
        VerificationLevel::UNVERIFIED,
        VerificationLevel::SEQUENCER_SIGNED,
        VerificationLevel::BATCH_INCLUDED,
        VerificationLevel::STATE_PROVEN,
        VerificationLevel::CHECKPOINT_FINALISED,
        VerificationLevel::SETTLEMENT_ANCHORED,
    ];
    for pair in levels.windows(2) {
        assert_eq!(pair[0].compare(pair[1]), std::cmp::Ordering::Less);
        assert!(pair[0] < pair[1]);
    }
    assert_eq!(VerificationLevel::UNVERIFIED.wire_rank(), 0);
    assert_eq!(VerificationLevel::SETTLEMENT_ANCHORED.wire_rank(), 5);
}

#[test]
fn projection_is_structurally_distinct_from_verified_value() {
    let projection = Projection::new(41_u64, "local fee estimate");
    assert_eq!(*projection.value(), 41);
    assert_eq!(projection.rationale(), "local fee estimate");
    compile_fails(
        "projection",
        "extern crate layerx_types;\nuse layerx_types::verify::{Projection, Verified};\nfn accepts(_: Verified<u64>) {}\nfn main() { accepts(Projection::new(1_u64, \"estimate\")); }\n",
        "mismatched types",
    );
}

#[test]
fn achieved_level_cannot_be_constructed_or_raised_without_evidence() {
    compile_fails(
        "forge",
        "extern crate layerx_types;\nuse layerx_types::verify::Verified;\nfn main() { let _ = Verified::<u64>::new(); }\n",
        "no function or associated item named `new`",
    );
    compile_fails(
        "raise",
        "extern crate layerx_types;\nuse layerx_types::verify::Verified;\nfn raise(value: &mut Verified<u64>) { value.raise_level(); }\nfn main() {}\n",
        "no method named `raise_level`",
    );
}
