#[allow(dead_code)]
#[path = "../src/main.rs"]
mod release;

use std::fs;
use std::path::PathBuf;

use release::workflow;
use release::{plan, registries_declarations, release_pipeline};

const RELEASE_CONDITION: &str =
    "github.event_name == 'workflow_dispatch' || startsWith(github.ref, 'refs/tags/sdk-v')";
const TAG_CONDITION: &str = "startsWith(github.ref, 'refs/tags/sdk-v')";

fn read(relative: &str) -> String {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join(relative);
    fs::read_to_string(&path).unwrap_or_else(|error| panic!("read {}: {error}", path.display()))
}

fn committed_manifest() -> String {
    read("registries.kvx")
}

fn committed_workflow() -> String {
    read("../../.github/workflows/platform.yml")
}

fn committed_pipeline() -> release::ReleasePipeline {
    release_pipeline(&committed_manifest(), &committed_workflow())
        .unwrap_or_else(|error| panic!("committed release pipeline refused: {error}"))
}

#[test]
fn committed_manifest_declares_all_seven_registries() {
    let declarations = registries_declarations(&committed_manifest())
        .unwrap_or_else(|error| panic!("manifest refused: {error}"));
    let names = declarations
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
fn every_registry_is_active_with_real_sorted_package_identities() {
    let declarations = registries_declarations(&committed_manifest())
        .unwrap_or_else(|error| panic!("manifest refused: {error}"));
    for registry in &declarations.registries {
        assert!(
            !registry.packages.is_empty(),
            "{} declares no packages",
            registry.name
        );
        let mut sorted = registry.packages.clone();
        sorted.sort();
        sorted.dedup();
        assert_eq!(
            sorted, registry.packages,
            "{} packages unsorted",
            registry.name
        );
        assert!(
            registry
                .declarations
                .contains(&("status".to_owned(), "active".to_owned())),
            "{} is not active",
            registry.name
        );
    }
    let package = |name: &str| {
        declarations
            .registries
            .iter()
            .find(|registry| registry.name == name)
            .map_or_else(
                || panic!("{name} missing"),
                |registry| registry.packages.clone(),
            )
    };
    assert!(package("npm").contains(&"@sidiora/layerx-sdk".to_owned()));
    assert!(package("crates-io").contains(&"layerx-sdk".to_owned()));
    assert!(package("pypi").contains(&"layerx-sdk".to_owned()));
    assert_eq!(
        package("go-modules"),
        vec!["github.com/Sidiora-Labs/LayerX-Protocol/platform/sdk/go"]
    );
    assert!(package("maven-central").contains(&"com.sidiora.layerx:layerx-sdk".to_owned()));
    assert_eq!(package("swiftpm"), vec!["LayerXSDK"]);
    assert_eq!(package("nuget"), vec!["LayerX.Sdk"]);
}

#[test]
fn committed_workflow_publishes_every_declared_registry_and_nothing_else() {
    let pipeline = committed_pipeline();
    let jobs = pipeline
        .publications
        .iter()
        .map(|publication| (publication.registry.as_str(), publication.job.as_str()))
        .collect::<Vec<_>>();
    assert_eq!(
        jobs,
        vec![
            ("crates-io", "publish-crates-io"),
            ("npm", "publish-npm"),
            ("pypi", "publish-pypi"),
            ("go-modules", "publish-go-modules"),
            ("maven-central", "publish-maven-central"),
            ("swiftpm", "publish-swiftpm"),
            ("nuget", "publish-nuget"),
        ]
    );
    for (publication, registry) in pipeline
        .publications
        .iter()
        .zip(&pipeline.declarations.registries)
    {
        assert_eq!(publication.packages, registry.packages);
    }
}

#[test]
fn committed_workflow_requires_every_release_gate() {
    let pipeline = committed_pipeline();
    let gates = pipeline
        .gates
        .iter()
        .map(|gate| (gate.job.as_str(), gate.command.as_str()))
        .collect::<Vec<_>>();
    assert_eq!(
        gates,
        vec![
            ("programs-acceptance", "make programs-test"),
            ("agent-sanitizers", "make agent-test-sanitize"),
            ("agent-fuzz-corpus", "make agent-fuzz-long"),
            ("replay-matrix", "make test-replay-golden"),
        ]
    );
    let replay = pipeline
        .gates
        .iter()
        .find(|gate| gate.job == "replay-matrix")
        .unwrap_or_else(|| panic!("replay gate missing"));
    assert_eq!(replay.machines, vec!["aarch64", "x86_64"]);
}

#[test]
fn plan_is_deterministic_and_machine_readable() {
    let pipeline = committed_pipeline();
    let first = plan(&pipeline).unwrap_or_else(|error| panic!("plan: {error}"));
    let second = plan(&pipeline).unwrap_or_else(|error| panic!("plan: {error}"));
    assert_eq!(first, second);
    assert_eq!(first.lines().count(), 3 + 7 + 4 + 1);
    assert!(first.starts_with("tag_format=sdk-v{version}\n"));
    assert!(first
        .lines()
        .nth(2)
        .is_some_and(|line| line.starts_with("reference_applications=")));
    for line in first.lines().skip(3).take(7) {
        assert!(
            line.starts_with("registry="),
            "unexpected plan line: {line}"
        );
        assert!(line.contains(" signing="), "plan line lost signing: {line}");
        assert!(
            line.contains(" verification=byte-compare-against-tagged-source"),
            "plan line lost verification: {line}"
        );
        assert!(
            line.contains(" packages=") && line.contains(" publication_job=publish-"),
            "plan line lost its publication binding: {line}"
        );
    }
    for line in first.lines().skip(10).take(4) {
        assert!(
            line.starts_with("gate=") && line.contains(" command=make "),
            "unexpected gate line: {line}"
        );
    }
    assert!(first
        .contains("gate=replay-matrix command=make test-replay-golden machines=aarch64,x86_64"));
    assert!(first
        .lines()
        .last()
        .is_some_and(|line| line.starts_with("verification_job=release-verification ")));
}

#[test]
fn release_plan_carries_every_cloneable_reference_application() {
    let declarations = registries_declarations(&committed_manifest())
        .unwrap_or_else(|error| panic!("manifest refused: {error}"));
    assert_eq!(
        declarations.reference_applications,
        vec![
            "@sidiora/layerx-example-buyer-agent",
            "@sidiora/layerx-example-marketplace",
            "@sidiora/layerx-example-merchant-shop",
            "@sidiora/layerx-example-paid-api",
        ]
    );
}

#[test]
fn maven_release_preserves_its_typed_coordinates() {
    let pipeline = committed_pipeline();
    let maven = pipeline
        .declarations
        .registries
        .iter()
        .find(|registry| registry.name == "maven-central")
        .unwrap_or_else(|| panic!("Maven Central registry missing"));
    assert!(maven.declarations.contains(&(
        "coordinate".to_owned(),
        "com.sidiora.layerx:layerx-sdk".to_owned()
    )));
    assert!(maven
        .declarations
        .contains(&("languages".to_owned(), "java,kotlin".to_owned())));
    assert!(maven.declarations.contains(&(
        "module_name".to_owned(),
        "com.sidiora.layerx.sdk".to_owned()
    )));
    let rendered = plan(&pipeline).unwrap_or_else(|error| panic!("plan: {error}"));
    assert!(rendered.contains(
        "coordinate=com.sidiora.layerx:layerx-sdk languages=java,kotlin module_name=com.sidiora.layerx.sdk"
    ));
}

fn expect_manifest_refusal(source: &str, needle: &str) {
    let error = registries_declarations(source)
        .err()
        .unwrap_or_else(|| panic!("manifest accepted but expected refusal about {needle}"));
    assert!(
        error.contains(needle),
        "expected refusal mentioning {needle}, got: {error}"
    );
}

fn expect_workflow_refusal(workflow_source: &str, needle: &str) {
    let error = release_pipeline(&committed_manifest(), workflow_source)
        .err()
        .unwrap_or_else(|| panic!("workflow accepted but expected refusal about {needle}"));
    assert!(
        error.contains(needle),
        "expected refusal mentioning {needle}, got: {error}"
    );
}

fn replaced(source: &str, from: &str, to: &str) -> String {
    assert!(source.contains(from), "fixture lost the text {from:?}");
    source.replacen(from, to, 1)
}

#[test]
fn dropping_a_registry_from_the_list_is_refused() {
    let source = committed_manifest().replace(", \"nuget\"]", "]");
    expect_manifest_refusal(&source, "seven mandated registries");
}

#[test]
fn missing_registry_section_is_refused() {
    let manifest = committed_manifest();
    let start = manifest
        .find("[registry.nuget]")
        .unwrap_or_else(|| panic!("nuget section missing from fixture"));
    expect_manifest_refusal(&manifest[..start], "registry nuget is not declared");
}

#[test]
fn missing_signing_declaration_is_refused() {
    let source = committed_manifest().replace("signing = \"nuget-repository-signature\"\n", "");
    expect_manifest_refusal(&source, "missing declaration registry.nuget.signing");
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
        manifest[start..].replace("status = \"active\"", "status = \"imagined\"")
    );
    expect_manifest_refusal(&source, "unknown status imagined");
}

#[test]
fn skeleton_registry_cannot_be_published() {
    let manifest = committed_manifest();
    let start = manifest
        .find("[registry.nuget]")
        .unwrap_or_else(|| panic!("nuget section missing from fixture"));
    let source = format!(
        "{}{}",
        &manifest[..start],
        manifest[start..].replace("status = \"active\"", "status = \"skeleton\"")
    );
    let error = release_pipeline(&source, &committed_workflow())
        .err()
        .unwrap_or_else(|| panic!("skeleton registry accepted for publication"));
    assert!(error.contains("manifest status is not active"), "{error}");
}

#[test]
fn empty_package_list_is_refused() {
    let source = committed_manifest().replace("packages = [\"LayerX.Sdk\"]", "packages = []");
    expect_manifest_refusal(&source, "registry.nuget.packages");
}

#[test]
fn unsorted_package_list_is_refused() {
    let source = committed_manifest().replace(
        "packages = [\"layerx-fastapi\",\"layerx-sdk\"]",
        "packages = [\"layerx-sdk\",\"layerx-fastapi\"]",
    );
    expect_manifest_refusal(&source, "registry.pypi.packages");
}

#[test]
fn unknown_registry_section_is_refused() {
    let source = format!(
        "{}\n[registry.homebrew]\necosystem = \"ruby\"\n",
        committed_manifest()
    );
    expect_manifest_refusal(&source, "unknown registry homebrew");
}

#[test]
fn unknown_registry_declaration_is_refused() {
    let source = committed_manifest().replace(
        "[registry.npm]\necosystem = \"typescript\"",
        "[registry.npm]\nmirror = \"true\"\necosystem = \"typescript\"",
    );
    expect_manifest_refusal(&source, "unknown declaration registry.npm.mirror");
}

#[test]
fn maven_declarations_are_refused_for_other_registries() {
    let source = committed_manifest().replace(
        "[registry.nuget]\necosystem = \"dotnet\"",
        "[registry.nuget]\ncoordinate = \"invalid:coordinate\"\necosystem = \"dotnet\"",
    );
    expect_manifest_refusal(&source, "unknown declaration registry.nuget.coordinate");
}

#[test]
fn incomplete_maven_coordinates_are_refused() {
    let source =
        committed_manifest().replace("coordinate = \"com.sidiora.layerx:layerx-sdk\"\n", "");
    expect_manifest_refusal(
        &source,
        "missing declaration registry.maven-central.coordinate",
    );
}

#[test]
fn non_canonical_maven_coordinates_are_refused() {
    let coordinate = committed_manifest().replace(
        "coordinate = \"com.sidiora.layerx:layerx-sdk\"",
        "coordinate = \"com.sidiora.layerx:other-sdk\"",
    );
    expect_manifest_refusal(
        &coordinate,
        "registry.maven-central.coordinate is not canonical",
    );

    let languages = committed_manifest().replace(
        "languages = [\"java\",\"kotlin\"]",
        "languages = [\"kotlin\",\"java\"]",
    );
    expect_manifest_refusal(
        &languages,
        "registry.maven-central.languages must list exactly",
    );

    let module = committed_manifest().replace(
        "module_name = \"com.sidiora.layerx.sdk\"",
        "module_name = \"com.sidiora.layerx.other\"",
    );
    expect_manifest_refusal(
        &module,
        "registry.maven-central.module_name is not canonical",
    );
}

#[test]
fn empty_declaration_is_refused() {
    let source = committed_manifest().replace("artifact = \"nupkg\"", "artifact = \"\"");
    expect_manifest_refusal(&source, "empty declaration registry.nuget.artifact");
}

#[test]
fn tag_format_without_version_placeholder_is_refused() {
    let source = committed_manifest().replace("sdk-v{version}", "sdk-latest");
    expect_manifest_refusal(&source, "tag_format");
}

#[test]
fn declared_registry_without_a_publication_job_is_refused() {
    let workflow = committed_workflow();
    let start = workflow
        .find("\n  publish-nuget:\n")
        .unwrap_or_else(|| panic!("publish-nuget job missing from the workflow"));
    let end = workflow
        .find("\n  release-promotion:\n")
        .unwrap_or_else(|| panic!("release-promotion job missing from the workflow"));
    assert!(start < end, "publish-nuget must precede release-promotion");
    let source = format!("{}{}", &workflow[..start], &workflow[end..]);
    expect_workflow_refusal(
        &source,
        "registry nuget is declared but no publication job carries LAYERX_RELEASE_REGISTRY=nuget",
    );
}

#[test]
fn publication_job_for_an_undeclared_registry_is_refused() {
    let source = format!(
        "{}\n  publish-homebrew:\n    if: {RELEASE_CONDITION}\n    needs: [release-pipeline]\n    runs-on: ubuntu-24.04\n    env:\n      LAYERX_RELEASE_REGISTRY: homebrew\n    steps:\n      - run: brew tap\n",
        committed_workflow()
    );
    expect_workflow_refusal(
        &source,
        "job publish-homebrew publishes LAYERX_RELEASE_REGISTRY=homebrew, which the manifest does not declare",
    );
}

#[test]
fn publication_outside_the_declared_job_is_refused() {
    let source = format!(
        "{}\n  side-channel:\n    runs-on: ubuntu-24.04\n    steps:\n      - run: npm publish --workspace @sidiora/layerx-sdk\n",
        committed_workflow()
    );
    expect_workflow_refusal(
        &source,
        "job side-channel publishes to npm outside the declared publication job",
    );
}

#[test]
fn publishing_fewer_packages_than_declared_is_refused() {
    let source = replaced(
        &committed_workflow(),
        "@sidiora/layerx-next @sidiora/layerx-sdk",
        "@sidiora/layerx-next",
    );
    expect_workflow_refusal(
        &source,
        "job publish-npm publishes LAYERX_RELEASE_PACKAGES=",
    );
}

#[test]
fn explicit_undeclared_package_is_refused() {
    let source = replaced(
        &committed_workflow(),
        "cargo publish --locked --manifest-path interop/Cargo.toml -p layerx-mirror",
        "cargo publish --locked --manifest-path interop/Cargo.toml -p layerx-ucp",
    );
    expect_workflow_refusal(
        &source,
        "job publish-crates-io publishes layerx-ucp, which the manifest does not declare",
    );
}

#[test]
fn missing_signature_is_refused() {
    let source = committed_workflow().replace("cosign sign-blob", "cosign sign-nothing");
    expect_workflow_refusal(&source, "job publish-crates-io lacks its signature");
}

#[test]
fn missing_install_check_is_refused() {
    let source = committed_workflow().replace("dotnet add package", "dotnet add reference");
    expect_workflow_refusal(
        &source,
        "job publish-nuget lacks its install check from the registry",
    );
}

#[test]
fn publication_step_gated_on_anything_but_the_tag_is_refused() {
    let source = replaced(
        &committed_workflow(),
        &format!("        if: {TAG_CONDITION}\n"),
        "        if: github.event_name == 'schedule'\n",
    );
    expect_workflow_refusal(&source, "may gate publication steps");
}

#[test]
fn schedule_only_release_gate_is_refused() {
    let source = replaced(
        &committed_workflow(),
        &format!("  programs-acceptance:\n    if: {RELEASE_CONDITION}\n"),
        "  programs-acceptance:\n    if: github.event_name == 'schedule'\n",
    );
    expect_workflow_refusal(
        &source,
        "release gate programs-acceptance is conditional on `github.event_name == 'schedule'`",
    );
}

#[test]
fn release_gate_run_outside_the_gate_runner_is_refused() {
    let source = replaced(
        &committed_workflow(),
        "tools/ci/release-gate.sh --job agent-sanitizers --ordinal 2 -- make agent-test-sanitize",
        "make agent-test-sanitize",
    );
    expect_workflow_refusal(
        &source,
        "release gate agent-sanitizers must run through tools/ci/release-gate.sh",
    );
}

#[test]
fn release_without_every_gate_is_refused() {
    let source = replaced(
        &committed_workflow(),
        "needs: [programs-acceptance, agent-sanitizers, agent-fuzz-corpus, replay-matrix]",
        "needs: [programs-acceptance, agent-sanitizers, agent-fuzz-corpus]",
    );
    expect_workflow_refusal(
        &source,
        "job release-pipeline must need the release gate replay-matrix",
    );
}

#[test]
fn replay_on_a_single_architecture_is_refused() {
    let source = replaced(
        &committed_workflow(),
        "          - machine: aarch64\n            runner: ubuntu-24.04-arm\n            ordinal: 5\n",
        "",
    );
    expect_workflow_refusal(
        &source,
        "release gate replay-matrix must replay on every supported architecture",
    );
}

#[test]
fn partial_promotion_is_refused() {
    let source = replaced(
        &committed_workflow(),
        ", publish-nuget, release-verification]",
        ", release-verification]",
    );
    expect_workflow_refusal(
        &source,
        "job release-promotion must need publish-nuget so no partial promotion is presented as a release",
    );
}

#[test]
fn missing_release_tag_trigger_is_refused() {
    let source = replaced(
        &committed_workflow(),
        "tags: [\"sdk-v*\"]",
        "tags: [\"v*\"]",
    );
    expect_workflow_refusal(
        &source,
        "on.push.tags must carry the release tag pattern sdk-v*",
    );
}

#[test]
fn workflow_parser_reads_the_block_yaml_subset() {
    let document = workflow::parse(
        "name: Example\non:\n  push:\n    tags: [\"sdk-v*\"]\njobs:\n  build:\n    runs-on: ubuntu-24.04\n    steps:\n      - name: Run\n        run: |\n          echo one\n          echo two\n      - uses: actions/checkout@abc\n        with:\n          persist-credentials: false\n",
    )
    .unwrap_or_else(|error| panic!("parse: {error}"));
    assert_eq!(
        document.get("name").and_then(workflow::Node::as_str),
        Some("Example")
    );
    assert_eq!(
        document
            .path(&["on", "push", "tags"])
            .map(workflow::Node::strings),
        Some(vec!["sdk-v*"])
    );
    let steps = document
        .path(&["jobs", "build", "steps"])
        .map(workflow::Node::items)
        .unwrap_or_default();
    assert_eq!(steps.len(), 2);
    assert_eq!(
        steps[0].get("run").and_then(workflow::Node::as_str),
        Some("echo one\necho two\n")
    );
    assert_eq!(
        steps[1]
            .path(&["with", "persist-credentials"])
            .and_then(workflow::Node::as_str),
        Some("false")
    );
}

#[test]
fn workflow_parser_refuses_unsupported_yaml() {
    assert!(workflow::parse("jobs:\n\tbuild: x\n").is_err());
    assert!(workflow::parse("defaults: &shared\n  a: b\n").is_err());
    assert!(workflow::parse("jobs:\n  a: 1\n  a: 2\n").is_err());
}
