#[path = "../../../platform/hosted/gateway/tests/local/required.rs"]
mod required;

use required::Required;
use std::{
    env, fs,
    path::{Path, PathBuf},
};

fn adapt_core(repository: &Path, core: &Path, binary: &Path) -> String {
    let core_source = core.join("tests/boundary.rs");
    let original = fs::read_to_string(&core_source).required("real core fixture");
    let mut core_fixture = original
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
            core_fixture.matches(needle).count(),
            1,
            "fixture path contract changed"
        );
        core_fixture = core_fixture.replace(needle, &replacement);
    }
    core_fixture
}

fn adapt_lifecycle(repository: &Path, local: &Path, pay2_source: &Path) -> String {
    let mut lifecycle =
        fs::read_to_string(local.join("lifecycle.rs")).required("gateway lifecycle");
    for (needle, replacement) in [
        (
            "mod funding;",
            format!(
                "#[path = \"{}\"]\nmod funding;",
                local.join("funding.rs").display()
            ),
        ),
        (
            "mod required;",
            format!(
                "#[path = \"{}\"]\nmod required;",
                local.join("required.rs").display()
            ),
        ),
        (
            "(\"Idempotency-Key\", idempotency),",
            "(\"Idempotency-Key\", idempotency),\n            (\"Connection\", \"close\"),"
                .to_owned(),
        ),
    ] {
        assert_eq!(
            lifecycle.matches(needle).count(),
            1,
            "gateway fixture module contract changed"
        );
        lifecycle = lifecycle.replace(needle, &replacement);
    }
    let fixture_prefix = "../../../../../agent/crates/layerx-crypto/tests/fixtures/payments/";
    assert_eq!(
        lifecycle.matches(fixture_prefix).count(),
        9,
        "gateway fixture path contract changed"
    );
    lifecycle = lifecycle.replace(
        fixture_prefix,
        &format!(
            "{}/agent/crates/layerx-crypto/tests/fixtures/payments/",
            repository.display()
        ),
    );
    lifecycle.push('\n');
    lifecycle.push_str(&fs::read_to_string(pay2_source).required("PAY2 qualification source"));
    lifecycle
}

fn main() {
    let manifest = PathBuf::from(env::var_os("CARGO_MANIFEST_DIR").required("manifest"));
    let repository = manifest
        .join("../../..")
        .canonicalize()
        .required("repository");
    let local = repository.join("platform/hosted/gateway/tests/local");
    let core = repository
        .join("platform/hosted/core")
        .canonicalize()
        .required("core source");
    let core_source = core.join("tests/boundary.rs");
    let lifecycle_source = local.join("lifecycle.rs");
    let pay2_source = manifest.join("pay2e_test.rs");
    println!("cargo:rerun-if-changed={}", core_source.display());
    println!("cargo:rerun-if-changed={}", lifecycle_source.display());
    println!("cargo:rerun-if-changed={}", pay2_source.display());
    println!("cargo:rerun-if-env-changed=LAYERX_TEST_CORE_BIN");
    let binary = PathBuf::from(
        env::var_os("LAYERX_TEST_CORE_BIN").required("parent-built core binary required"),
    );
    assert!(
        binary.is_absolute() && binary.is_file(),
        "prebuilt core binary required"
    );
    let out = PathBuf::from(env::var_os("OUT_DIR").required("out dir"));
    fs::write(
        out.join("core_fixture.rs"),
        adapt_core(&repository, &core, &binary),
    )
    .required("fixture adapter");
    let lifecycle = adapt_lifecycle(&repository, &local, &pay2_source);
    fs::write(out.join("lifecycle.rs"), lifecycle).required("gateway lifecycle adapter");
}
