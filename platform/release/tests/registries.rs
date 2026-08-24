#[allow(dead_code)]
#[path = "../src/main.rs"]
mod release;

use std::fs;
use std::path::PathBuf;

use release::{plan, release_pipeline};

fn committed_manifest() -> String {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("registries.kvx");
    fs::read_to_string(&path)
        .unwrap_or_else(|error| panic!("read {}: {error}", path.display()))
}

#[test]
fn committed_manifest_declares_all_seven_registries() {
    let pipeline = release_pipeline(&committed_manifest())
        .unwrap_or_else(|error| panic!("manifest refused: {error}"));
    let names = pipeline
        .registries
        .iter()
        .map(|registry| registry.name.as_str())
        .collect::<Vec<_>>();
    assert_eq!(
        names,
        vec![
            "crates-io",
            "npm",
            "pypi",
            "go-modules",
            "maven-central",
            "swiftpm",
            "nuget"
        ]
    );
}

#[test]
fn plan_is_deterministic_and_machine_readable() {
    let pipeline = release_pipeline(&committed_manifest())
        .unwrap_or_else(|error| panic!("manifest refused: {error}"));
    let first = plan(&pipeline).unwrap_or_else(|error| panic!("plan: {error}"));
    let second = plan(&pipeline).unwrap_or_else(|error| panic!("plan: {error}"));
    assert_eq!(first, second);
    assert_eq!(first.lines().count(), 10);
    assert!(first.starts_with("tag_format=sdk-v{version}\n"));
    assert!(first.lines().nth(2).is_some_and(|line| line.starts_with("reference_applications=")));
    for line in first.lines().skip(3) {
        assert!(line.starts_with("registry="), "unexpected plan line: {line}");
        assert!(line.contains(" signing="), "plan line lost signing: {line}");
        assert!(
            line.contains(" verification=byte-compare-against-tagged-source"),
            "plan line lost verification: {line}"
        );
    }
}

#[test]
fn release_plan_carries_every_cloneable_reference_application() {
    let pipeline = release_pipeline(&committed_manifest())
        .unwrap_or_else(|error| panic!("manifest refused: {error}"));
    assert_eq!(
        pipeline.reference_applications,
        vec![
            "@sidiora/layerx-example-buyer-agent",
            "@sidiora/layerx-example-marketplace",
            "@sidiora/layerx-example-merchant-shop",
            "@sidiora/layerx-example-paid-api",
        ]
    );
}

#[test]
fn npm_release_declares_every_middleware_package() {
    let pipeline = release_pipeline(&committed_manifest())
        .unwrap_or_else(|error| panic!("manifest refused: {error}"));
    let npm = pipeline
        .registries
        .iter()
        .find(|registry| registry.name == "npm")
        .unwrap_or_else(|| panic!("npm registry missing"));
    assert_eq!(
        npm.packages,
        vec![
            "@sidiora/layerx-agent-middleware",
            "@sidiora/layerx-buyer-middleware",
            "@sidiora/layerx-merchant-middleware",
            "@sidiora/layerx-seller-middleware",
        ]
    );
}

fn expect_refusal(source: &str, needle: &str) {
    let error = release_pipeline(source)
        .err()
        .unwrap_or_else(|| panic!("manifest accepted but expected refusal about {needle}"));
    assert!(
        error.contains(needle),
        "expected refusal mentioning {needle}, got: {error}"
    );
}

#[test]
fn dropping_a_registry_from_the_list_is_refused() {
    let source = committed_manifest().replace(", \"nuget\"]", "]");
    expect_refusal(&source, "seven mandated registries");
}

#[test]
fn missing_registry_section_is_refused() {
    let manifest = committed_manifest();
    let start = manifest
        .find("[registry.nuget]")
        .unwrap_or_else(|| panic!("nuget section missing from fixture"));
    expect_refusal(&manifest[..start], "registry nuget is not declared");
}

#[test]
fn missing_signing_declaration_is_refused() {
    let source = committed_manifest().replace("signing = \"nuget-repository-signature\"\n", "");
    expect_refusal(&source, "missing declaration registry.nuget.signing");
}

#[test]
fn unknown_status_is_refused() {
    let manifest = committed_manifest();
    let start = manifest
        .find("[registry.nuget]")
        .unwrap_or_else(|| panic!("nuget section missing from fixture"));
    let source = format!(
        "{}{}",
        &manifest[..start],
        manifest[start..].replace("status = \"skeleton\"", "status = \"imagined\"")
    );
    expect_refusal(&source, "unknown status imagined");
}

#[test]
fn unknown_registry_section_is_refused() {
    let source = format!(
        "{}\n[registry.homebrew]\necosystem = \"ruby\"\n",
        committed_manifest()
    );
    expect_refusal(&source, "unknown registry homebrew");
}

#[test]
fn unknown_registry_declaration_is_refused() {
    let source = committed_manifest().replace(
        "[registry.npm]\necosystem = \"typescript\"",
        "[registry.npm]\nmirror = \"true\"\necosystem = \"typescript\"",
    );
    expect_refusal(&source, "unknown declaration registry.npm.mirror");
}

#[test]
fn empty_declaration_is_refused() {
    let source = committed_manifest().replace(
        "artifact = \"nupkg\"",
        "artifact = \"\"",
    );
    expect_refusal(&source, "empty declaration registry.nuget.artifact");
}

#[test]
fn tag_format_without_version_placeholder_is_refused() {
    let source = committed_manifest().replace("sdk-v{version}", "sdk-latest");
    expect_refusal(&source, "tag_format");
}
