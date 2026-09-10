mod required;
use required::Required;

use std::{env, fs, path::PathBuf};

fn main() {
    let manifest = PathBuf::from(env::var_os("CARGO_MANIFEST_DIR").required("manifest"));
    let core = manifest
        .join("../../../core")
        .canonicalize()
        .required("core source");
    let repository = core
        .join("../../..")
        .canonicalize()
        .required("repository source");
    let source = core.join("tests/boundary.rs");
    println!("cargo:rerun-if-changed={}", source.display());
    println!("cargo:rerun-if-env-changed=LAYERX_TEST_CORE_BIN");
    let binary = PathBuf::from(
        env::var_os("LAYERX_TEST_CORE_BIN").required("parent-built core binary required"),
    );
    assert!(
        binary.is_absolute() && binary.is_file(),
        "prebuilt core binary required"
    );
    let original = fs::read_to_string(source).required("real core fixture");
    let mut adapted = original
        .lines()
        .map(|line| {
            if let Some(doc) = line.strip_prefix("//!") {
                format!("//{doc}\n")
            } else {
                format!("{line}\n")
            }
        })
        .collect::<String>();
    for (needle, replacement) in [
        (
            "env!(\"CARGO_MANIFEST_DIR\")",
            format!("{:?}", core.to_str().required("core path")),
        ),
        (
            "env!(\"CARGO_BIN_EXE_layerx-core-boundary\")",
            format!("{:?}", binary.to_str().required("binary path")),
        ),
        (
            "\"../../../../tests/support/lxgb_metadata.rs\"",
            format!(
                "{:?}",
                repository
                    .join("tests/support/lxgb_metadata.rs")
                    .to_str()
                    .required("metadata path")
            ),
        ),
    ] {
        assert_eq!(
            adapted.matches(needle).count(),
            1,
            "fixture path contract changed"
        );
        adapted = adapted.replace(needle, &replacement);
    }
    fs::write(
        PathBuf::from(env::var_os("OUT_DIR").required("out dir")).join("core_fixture.rs"),
        adapted,
    )
    .required("fixture adapter");
}
