#[allow(dead_code)]
#[path = "../src/main.rs"]
mod release;

use std::fmt::Write as _;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use release::{
    artifact_manifest, parse_artifact_manifest, plan, release_pipeline, release_pipeline_verify,
    render_artifact_manifest, sha256_digest, ArtifactManifest, ArtifactSource, PublicationJob,
    ReleasePipeline,
};

const VERSION: &str = "0.1.0";
const ROLLBACK: &str = "0.0.9";
const REVISION: &str = "0123456789abcdef0123456789abcdef01234567";
const OTHER_REVISION: &str = "fedcba9876543210fedcba9876543210fedcba98";
const SOURCE_DIGEST: &str = "9f86d081884c7d659a2feaa0c55ad015a3bf4f1b2b0b822cd15d6c15b0f00a08";
const ARTIFACT_COUNT: usize = 28;

fn read(relative: &str) -> String {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join(relative);
    fs::read_to_string(&path).unwrap_or_else(|error| panic!("read {}: {error}", path.display()))
}

fn registries_path() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("registries.kvx")
}

fn workflow_path() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../.github/workflows/platform.yml")
}

fn committed_workflow() -> String {
    read("../../.github/workflows/platform.yml")
}

fn committed_pipeline() -> ReleasePipeline {
    release_pipeline(&read("registries.kvx"), &committed_workflow())
        .unwrap_or_else(|error| panic!("committed release pipeline refused: {error}"))
}

fn write(path: &Path, bytes: &[u8]) {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)
            .unwrap_or_else(|error| panic!("create {}: {error}", parent.display()));
    }
    fs::write(path, bytes).unwrap_or_else(|error| panic!("write {}: {error}", path.display()));
}

struct Record {
    name: String,
    file: String,
    digest_of: &'static str,
    signature: String,
    sbom: String,
    attestation: String,
    location: String,
}

fn crate_record(package: &str) -> Record {
    let file = format!("{package}-{VERSION}.crate");
    let sbom = match package {
        "layerx-mirror" | "layerx-programs-runtime" => format!("{package}.sbom.spdx.json"),
        _ => "agent.sbom.spdx.json".to_owned(),
    };
    Record {
        name: package.to_owned(),
        signature: format!("{file}.sigstore.json"),
        sbom,
        attestation: "provenance.sigstore.json".to_owned(),
        location: format!("https://crates.io/api/v1/crates/{package}/{VERSION}/download"),
        file,
        digest_of: "built-bytes",
    }
}

fn npm_record(package: &str) -> Record {
    let bare = package.trim_start_matches("@sidiora/");
    let file = format!("sidiora-{bare}-{VERSION}.tgz");
    Record {
        name: package.to_owned(),
        signature: format!("{file}.sigstore.json"),
        sbom: format!("_sidiora-{bare}.sbom.spdx.json"),
        attestation: format!(
            "npm-provenance:https://registry.npmjs.org/-/npm/v1/attestations/%40sidiora%2F{bare}@{VERSION}"
        ),
        location: format!("https://registry.npmjs.org/{package}/-/{bare}-{VERSION}.tgz"),
        file,
        digest_of: "built-bytes",
    }
}

fn pypi_record(package: &str) -> Record {
    let wheel = format!("{}-{VERSION}-py3-none-any.whl", package.replace('-', "_"));
    Record {
        name: package.to_owned(),
        file: format!("dist/{wheel}"),
        digest_of: "built-bytes",
        signature: format!("signatures/{wheel}.sigstore.json"),
        sbom: format!("{package}.sbom.spdx.json"),
        attestation: format!(
            "pypi-integrity:https://pypi.org/integrity/{package}/{VERSION}/{wheel}/provenance"
        ),
        location: format!("simple+https://pypi.org/simple/{package}/"),
    }
}

fn go_record(package: &str) -> Record {
    Record {
        name: package.to_owned(),
        file: format!("layerx-go-v{VERSION}.zip"),
        digest_of: "registry-bytes",
        signature: "tag-signature.txt".to_owned(),
        sbom: "layerx-go.sbom.spdx.json".to_owned(),
        attestation: format!("go-checksum-database:sum.golang.org {package} v{VERSION} h1:fixture="),
        location: format!(
            "https://proxy.golang.org/github.com/!sidiora-!labs/!layer-x-!protocol/platform/sdk/go/@v/v{VERSION}.zip"
        ),
    }
}

fn maven_records(package: &str) -> Vec<Record> {
    let (group, name) = package
        .split_once(':')
        .unwrap_or_else(|| panic!("maven coordinate {package} has no group"));
    let directory = format!("{}/{name}/{VERSION}", group.replace('.', "/"));
    ["jar", "pom"]
        .into_iter()
        .map(|extension| {
            let file = format!("staging/{directory}/{name}-{VERSION}.{extension}");
            Record {
                name: package.to_owned(),
                signature: format!("{file}.asc"),
                sbom: format!("{name}.sbom.spdx.json"),
                attestation: "provenance.sigstore.json".to_owned(),
                location: format!(
                    "https://repo1.maven.org/maven2/{directory}/{name}-{VERSION}.{extension}"
                ),
                file,
                digest_of: "built-bytes",
            }
        })
        .collect()
}

fn swift_record(package: &str) -> Record {
    Record {
        name: package.to_owned(),
        file: format!("LayerXSDK-{VERSION}.tar"),
        digest_of: "built-bytes",
        signature: "tag-signature.txt".to_owned(),
        sbom: "LayerXSDK.sbom.spdx.json".to_owned(),
        attestation: "provenance.sigstore.json".to_owned(),
        location: format!("git+https://github.com/Sidiora-Labs/LayerXSDK#{VERSION}"),
    }
}

fn nuget_record(package: &str) -> Record {
    Record {
        name: package.to_owned(),
        file: format!("LayerX.Sdk.{VERSION}.registry.nupkg"),
        digest_of: "registry-bytes",
        signature: "repository-signature.txt".to_owned(),
        sbom: "LayerX.Sdk.sbom.spdx.json".to_owned(),
        attestation: "provenance.sigstore.json".to_owned(),
        location: format!(
            "https://api.nuget.org/v3-flatcontainer/layerx.sdk/{VERSION}/layerx.sdk.{VERSION}.nupkg"
        ),
    }
}

fn records(registry: &str, packages: &[String]) -> Vec<Record> {
    packages
        .iter()
        .flat_map(|package| match registry {
            "crates-io" => vec![crate_record(package)],
            "npm" => vec![npm_record(package)],
            "pypi" => vec![pypi_record(package)],
            "go-modules" => vec![go_record(package)],
            "maven-central" => maven_records(package),
            "swiftpm" => vec![swift_record(package)],
            "nuget" => vec![nuget_record(package)],
            other => panic!("no fixture records for registry {other}"),
        })
        .collect()
}

struct Fixture {
    release_dir: PathBuf,
    downloads: PathBuf,
}

fn write_publication(pipeline: &ReleasePipeline, publication: &PublicationJob, bundle: &Path) {
    let registry = pipeline
        .declarations
        .registries
        .iter()
        .find(|registry| registry.name == publication.registry)
        .unwrap_or_else(|| panic!("registry {} undeclared", publication.registry));
    let distribution = registry
        .declarations
        .iter()
        .find(|(key, _)| key == "distribution")
        .map_or_else(
            || panic!("registry {} has no distribution", registry.name),
            |(_, value)| value.clone(),
        );
    write(
        &bundle.join("publication.txt"),
        format!(
            "registry={}\nversion={VERSION}\ndistribution={distribution}\ntarget={distribution}\npackages={}\nrevision={REVISION}\npublished=true\ninstall_check=pass\n",
            publication.registry,
            publication.packages.join(" ")
        )
        .as_bytes(),
    );
}

impl Fixture {
    fn build(name: &str, pipeline: &ReleasePipeline) -> Self {
        let root = PathBuf::from(env!("CARGO_TARGET_TMPDIR"))
            .join("artifact-manifest")
            .join(name);
        if root.exists() {
            fs::remove_dir_all(&root)
                .unwrap_or_else(|error| panic!("remove {}: {error}", root.display()));
        }
        let release_dir = root.join("release");
        let downloads = root.join("downloads");
        write(
            &release_dir.join("release-pipeline/version.txt"),
            format!("version={VERSION}\n").as_bytes(),
        );
        write(
            &release_dir.join("release-pipeline/source-digest.txt"),
            format!("{SOURCE_DIGEST}  -\n").as_bytes(),
        );
        write(
            &release_dir.join("release-pipeline/release-plan.txt"),
            plan(pipeline)
                .unwrap_or_else(|error| panic!("plan: {error}"))
                .as_bytes(),
        );
        for publication in &pipeline.publications {
            let bundle = release_dir.join(format!("release-{}", publication.registry));
            write_publication(pipeline, publication, &bundle);
            let mut artifacts = String::new();
            for (index, record) in records(&publication.registry, &publication.packages)
                .iter()
                .enumerate()
            {
                let bytes = format!(
                    "{} {} {} published bytes\n",
                    publication.registry, record.name, record.file
                );
                let path = bundle.join(&record.file);
                write(&path, bytes.as_bytes());
                let basename = Path::new(&record.file)
                    .file_name()
                    .and_then(|name| name.to_str())
                    .unwrap_or_else(|| panic!("record file {} has no name", record.file));
                write(
                    &downloads.join(&publication.registry).join(basename),
                    bytes.as_bytes(),
                );
                write(
                    &bundle.join(&record.signature),
                    format!("signature of {}\n", record.file).as_bytes(),
                );
                write(
                    &bundle.join(&record.sbom),
                    b"{\"spdxVersion\": \"SPDX-2.3\"}\n",
                );
                if !record.attestation.contains(':') {
                    write(
                        &bundle.join(&record.attestation),
                        b"{\"provenance\": true}\n",
                    );
                }
                write!(
                    artifacts,
                    "[artifact.{}]\nname = \"{}\"\nversion = \"{VERSION}\"\nfile = \"{}\"\ndigest = \"{}\"\ndigest_of = \"{}\"\nsignature = \"{}\"\nsbom = \"{}\"\nattestation = \"{}\"\nlocation = \"{}\"\n\n",
                    index + 1,
                    record.name,
                    record.file,
                    sha256_digest(bytes.as_bytes()),
                    record.digest_of,
                    record.signature,
                    record.sbom,
                    record.attestation,
                    record.location
                )
                .unwrap_or_else(|error| panic!("write artifact record: {error}"));
            }
            write(&bundle.join("artifacts.kvx"), artifacts.as_bytes());
        }
        Self {
            release_dir,
            downloads,
        }
    }

    fn manifest(&self, pipeline: &ReleasePipeline) -> ArtifactManifest {
        artifact_manifest(pipeline, &self.release_dir, Some(ROLLBACK))
            .unwrap_or_else(|error| panic!("artifact manifest refused: {error}"))
    }
}

#[test]
fn manifest_lists_every_declared_artifact_bound_to_its_source() {
    let pipeline = committed_pipeline();
    let fixture = Fixture::build("complete", &pipeline);
    let manifest = fixture.manifest(&pipeline);
    assert_eq!(manifest.schema, "layerx/artifact-manifest/1");
    assert_eq!(manifest.version, VERSION);
    assert_eq!(manifest.tag, format!("sdk-v{VERSION}"));
    assert_eq!(manifest.source_revision, REVISION);
    assert_eq!(manifest.source_digest, format!("sha256:{SOURCE_DIGEST}"));
    assert_eq!(manifest.rollback_version.as_deref(), Some(ROLLBACK));
    assert_eq!(manifest.artifacts.len(), ARTIFACT_COUNT);
    for publication in &pipeline.publications {
        for package in &publication.packages {
            assert!(
                manifest
                    .artifacts
                    .iter()
                    .any(|entry| entry.registry == publication.registry && entry.name == *package),
                "manifest lost {package} from {}",
                publication.registry
            );
        }
    }
    for entry in &manifest.artifacts {
        assert_eq!(entry.version, VERSION);
        assert_eq!(entry.source_revision, REVISION);
        assert_eq!(entry.rollback_version.as_deref(), Some(ROLLBACK));
        assert!(entry.digest.starts_with("sha256:") && entry.digest.len() == 71);
        assert!(!entry.signature.is_empty() && !entry.sbom.is_empty());
        assert!(!entry.attestation.is_empty());
        assert!(
            entry.location.starts_with("https://")
                || entry.location.starts_with("simple+https://")
                || entry.location.starts_with("git+https://"),
            "{}@{} has no registry location: {}",
            entry.name,
            entry.version,
            entry.location
        );
        assert!(entry.published && entry.install_check);
        assert!(!entry.artifact.contains('/'));
        assert!(entry
            .retained
            .starts_with(&format!("release-{}/", entry.registry)));
    }
    let names = manifest
        .artifacts
        .iter()
        .map(|entry| (entry.registry.clone(), entry.artifact.clone()))
        .collect::<Vec<_>>();
    let mut unique = names.clone();
    unique.sort();
    unique.dedup();
    assert_eq!(
        unique.len(),
        names.len(),
        "artifact files are not unique per registry"
    );
}

#[test]
fn manifest_round_trips_through_its_json_rendering() {
    let pipeline = committed_pipeline();
    let fixture = Fixture::build("round-trip", &pipeline);
    let manifest = fixture.manifest(&pipeline);
    let rendered =
        render_artifact_manifest(&manifest).unwrap_or_else(|error| panic!("render: {error}"));
    assert!(rendered.starts_with("{\n  \"schema\": \"layerx/artifact-manifest/1\""));
    let parsed =
        parse_artifact_manifest(&rendered).unwrap_or_else(|error| panic!("parse: {error}"));
    assert_eq!(parsed, manifest);
    let again = render_artifact_manifest(&parsed).unwrap_or_else(|error| panic!("render: {error}"));
    assert_eq!(again, rendered);
}

#[test]
fn matching_published_bytes_pass_verification() {
    let pipeline = committed_pipeline();
    let fixture = Fixture::build("matching", &pipeline);
    let manifest = fixture.manifest(&pipeline);
    let verified = release_pipeline_verify(
        &manifest,
        ArtifactSource::Directory(&fixture.downloads),
        Some(REVISION),
    )
    .unwrap_or_else(|error| panic!("verification refused matching bytes: {error}"));
    assert_eq!(verified.len(), ARTIFACT_COUNT);
    assert!(verified
        .iter()
        .any(|artifact| artifact.name == "layerx-sdk" && artifact.registry == "crates-io"));
}

#[test]
fn tampered_published_artifact_halts_the_release_naming_it() {
    let pipeline = committed_pipeline();
    let fixture = Fixture::build("tampered-download", &pipeline);
    let manifest = fixture.manifest(&pipeline);
    write(
        &fixture
            .downloads
            .join("crates-io")
            .join(format!("layerx-sdk-{VERSION}.crate")),
        b"bytes the registry replaced\n",
    );
    let error = release_pipeline_verify(
        &manifest,
        ArtifactSource::Directory(&fixture.downloads),
        Some(REVISION),
    )
    .err()
    .unwrap_or_else(|| panic!("tampered crate passed verification"));
    assert!(
        error.starts_with("release halted before promotion: 1 of 28 artifacts failed verification"),
        "{error}"
    );
    assert!(
        error.contains(&format!("layerx-sdk@{VERSION} from crates-io (")),
        "{error}"
    );
    assert!(
        error.contains("hash to sha256:") && error.contains("but the manifest lists sha256:"),
        "{error}"
    );
    assert!(
        !error.contains(&format!("layerx-client@{VERSION} from crates-io")),
        "{error}"
    );
}

#[test]
fn missing_published_artifact_halts_the_release_naming_it() {
    let pipeline = committed_pipeline();
    let fixture = Fixture::build("missing-download", &pipeline);
    let manifest = fixture.manifest(&pipeline);
    let missing = fixture
        .downloads
        .join("nuget")
        .join(format!("LayerX.Sdk.{VERSION}.registry.nupkg"));
    fs::remove_file(&missing)
        .unwrap_or_else(|error| panic!("remove {}: {error}", missing.display()));
    let error = release_pipeline_verify(
        &manifest,
        ArtifactSource::Directory(&fixture.downloads),
        Some(REVISION),
    )
    .err()
    .unwrap_or_else(|| panic!("missing nupkg passed verification"));
    assert!(
        error.contains(&format!(
            "LayerX.Sdk@{VERSION} from nuget (the downloaded is not present at"
        )),
        "{error}"
    );
}

#[test]
fn manifest_bound_to_another_revision_halts_the_release() {
    let pipeline = committed_pipeline();
    let fixture = Fixture::build("other-revision", &pipeline);
    let manifest = fixture.manifest(&pipeline);
    let error = release_pipeline_verify(
        &manifest,
        ArtifactSource::Directory(&fixture.downloads),
        Some(OTHER_REVISION),
    )
    .err()
    .unwrap_or_else(|| panic!("revision mismatch passed verification"));
    assert_eq!(
        error,
        format!("release halted before promotion: the artifact manifest binds revision {REVISION}, not the release revision {OTHER_REVISION}")
    );
}

#[test]
fn unpublished_artifact_halts_the_release() {
    let pipeline = committed_pipeline();
    let fixture = Fixture::build("unpublished", &pipeline);
    let record = fixture.release_dir.join("release-nuget/publication.txt");
    let source = fs::read_to_string(&record)
        .unwrap_or_else(|error| panic!("read {}: {error}", record.display()));
    write(&record, source.replace("published=true\n", "").as_bytes());
    let manifest = fixture.manifest(&pipeline);
    let nupkg = manifest
        .artifacts
        .iter()
        .find(|entry| entry.registry == "nuget")
        .unwrap_or_else(|| panic!("manifest lost the nuget artifact"));
    assert!(!nupkg.published);
    let error = release_pipeline_verify(
        &manifest,
        ArtifactSource::Directory(&fixture.downloads),
        Some(REVISION),
    )
    .err()
    .unwrap_or_else(|| panic!("unpublished nupkg passed verification"));
    assert!(
        error.contains(&format!(
            "LayerX.Sdk@{VERSION} from nuget (was not published)"
        )),
        "{error}"
    );
}

#[test]
fn tampered_retained_artifact_is_refused_at_emission() {
    let pipeline = committed_pipeline();
    let fixture = Fixture::build("tampered-retained", &pipeline);
    write(
        &fixture
            .release_dir
            .join("release-npm")
            .join(format!("sidiora-layerx-sdk-{VERSION}.tgz")),
        b"bytes that were not recorded\n",
    );
    let error = artifact_manifest(&pipeline, &fixture.release_dir, Some(ROLLBACK))
        .err()
        .unwrap_or_else(|| panic!("tampered retained tarball produced a manifest"));
    assert!(error.starts_with(&format!("retained artifact @sidiora/layerx-sdk@{VERSION} from npm (sidiora-layerx-sdk-{VERSION}.tgz) hashes to sha256:")), "{error}");
}

#[test]
fn artifact_of_an_undeclared_package_is_refused_at_emission() {
    let pipeline = committed_pipeline();
    let fixture = Fixture::build("undeclared", &pipeline);
    let record = fixture.release_dir.join("release-pypi/artifacts.kvx");
    let source = fs::read_to_string(&record)
        .unwrap_or_else(|error| panic!("read {}: {error}", record.display()));
    write(
        &record,
        source
            .replace("name = \"layerx-fastapi\"", "name = \"layerx-django\"")
            .as_bytes(),
    );
    let error = artifact_manifest(&pipeline, &fixture.release_dir, Some(ROLLBACK))
        .err()
        .unwrap_or_else(|| panic!("undeclared package produced a manifest"));
    assert_eq!(error, "release-pypi/artifacts.kvx lists layerx-django, which the manifest does not declare for pypi");
}

#[test]
fn publication_from_another_revision_is_refused_at_emission() {
    let pipeline = committed_pipeline();
    let fixture = Fixture::build("split-revision", &pipeline);
    let record = fixture.release_dir.join("release-swiftpm/publication.txt");
    let source = fs::read_to_string(&record)
        .unwrap_or_else(|error| panic!("read {}: {error}", record.display()));
    write(&record, source.replace(REVISION, OTHER_REVISION).as_bytes());
    let error = artifact_manifest(&pipeline, &fixture.release_dir, Some(ROLLBACK))
        .err()
        .unwrap_or_else(|| panic!("split revision produced a manifest"));
    assert_eq!(error, format!("release-swiftpm/publication.txt records revision {OTHER_REVISION}, but {REVISION} published the other registries"));
}

#[test]
fn rollback_identity_cannot_be_the_released_version() {
    let pipeline = committed_pipeline();
    let fixture = Fixture::build("rollback", &pipeline);
    let error = artifact_manifest(&pipeline, &fixture.release_dir, Some(VERSION))
        .err()
        .unwrap_or_else(|| panic!("self-rollback produced a manifest"));
    assert_eq!(
        error,
        format!("rollback version {VERSION} is the version being released")
    );
    let first = artifact_manifest(&pipeline, &fixture.release_dir, None)
        .unwrap_or_else(|error| panic!("first release refused: {error}"));
    assert_eq!(first.rollback_version, None);
}

#[test]
fn workflow_without_the_verification_job_is_refused() {
    let source =
        committed_workflow().replace("  release-verification:\n", "  release-inspection:\n");
    let error = release_pipeline(&read("registries.kvx"), &source)
        .err()
        .unwrap_or_else(|| panic!("workflow without release-verification accepted"));
    assert_eq!(error, "release workflow has no job release-verification");
}

#[test]
fn promotion_that_does_not_need_verification_is_refused() {
    let source =
        committed_workflow().replace(", publish-nuget, release-verification]", ", publish-nuget]");
    assert_ne!(source, committed_workflow());
    let error = release_pipeline(&read("registries.kvx"), &source)
        .err()
        .unwrap_or_else(|| panic!("promotion without verification accepted"));
    assert_eq!(error, "job release-promotion must need release-verification so no unverified artifact is promoted");
}

#[test]
fn verification_without_fetching_from_registries_is_refused() {
    let source = committed_workflow().replace(
        "--fetch build/release/verification",
        "--from build/release/verification",
    );
    assert_ne!(source, committed_workflow());
    let error = release_pipeline(&read("registries.kvx"), &source)
        .err()
        .unwrap_or_else(|| panic!("verification without fetch accepted"));
    assert_eq!(error, "job release-verification lacks its fetch from every registry: expected `--fetch` in the job");
}

#[test]
fn plan_binds_the_verification_job_and_the_retained_manifest() {
    let pipeline = committed_pipeline();
    let text = plan(&pipeline).unwrap_or_else(|error| panic!("plan: {error}"));
    assert!(text.lines().any(|line| line == "verification_job=release-verification manifest=artifact-manifest.json workflow_artifact=release-artifact-manifest"), "{text}");
}

fn tool() -> Command {
    Command::new(env!("CARGO_BIN_EXE_layerx-platform-release"))
}

#[test]
fn command_line_emits_the_manifest_and_verifies_downloaded_artifacts() {
    let pipeline = committed_pipeline();
    let fixture = Fixture::build("command-line", &pipeline);
    let output = fixture.release_dir.join("artifact-manifest.json");
    let emitted = tool()
        .arg("manifest")
        .arg("--release-dir")
        .arg(&fixture.release_dir)
        .arg("--output")
        .arg(&output)
        .args(["--rollback-version", "none", "--registries"])
        .arg(registries_path())
        .arg("--workflow")
        .arg(workflow_path())
        .output()
        .unwrap_or_else(|error| panic!("run manifest: {error}"));
    assert!(
        emitted.status.success(),
        "{}",
        String::from_utf8_lossy(&emitted.stderr)
    );
    let stdout = String::from_utf8_lossy(&emitted.stdout);
    assert!(stdout.contains(&format!("artifact manifest: {ARTIFACT_COUNT} artifacts of {VERSION} (sdk-v{VERSION}) bound to revision {REVISION}")), "{stdout}");
    let manifest = parse_artifact_manifest(
        &fs::read_to_string(&output)
            .unwrap_or_else(|error| panic!("read {}: {error}", output.display())),
    )
    .unwrap_or_else(|error| panic!("emitted manifest refused: {error}"));
    assert_eq!(manifest.rollback_version, None);
    let verified = tool()
        .arg("verify")
        .arg("--manifest")
        .arg(&output)
        .arg("--from")
        .arg(&fixture.downloads)
        .args(["--source-revision", REVISION])
        .output()
        .unwrap_or_else(|error| panic!("run verify: {error}"));
    assert!(
        verified.status.success(),
        "{}",
        String::from_utf8_lossy(&verified.stderr)
    );
    let stdout = String::from_utf8_lossy(&verified.stdout);
    assert!(
        stdout.contains(&format!(
            "verified LayerXSDK@{VERSION} from swiftpm sha256:"
        )),
        "{stdout}"
    );
    assert!(stdout.contains(&format!("release verification: {ARTIFACT_COUNT} artifacts of {VERSION} match the manifest bound to revision {REVISION}")), "{stdout}");
    write(
        &fixture
            .downloads
            .join("maven-central")
            .join(format!("layerx-sdk-{VERSION}.pom")),
        b"<project>tampered</project>\n",
    );
    let halted = tool()
        .arg("verify")
        .arg("--manifest")
        .arg(&output)
        .arg("--from")
        .arg(&fixture.downloads)
        .args(["--source-revision", REVISION])
        .output()
        .unwrap_or_else(|error| panic!("run verify: {error}"));
    assert!(!halted.status.success());
    let stderr = String::from_utf8_lossy(&halted.stderr);
    assert!(stderr.contains("release halted before promotion: 1 of 28 artifacts failed verification: com.sidiora.layerx:layerx-sdk@0.1.0 from maven-central (the downloaded layerx-sdk-0.1.0.pom hash to sha256:"), "{stderr}");
    let both = tool()
        .arg("verify")
        .arg("--manifest")
        .arg(&output)
        .arg("--from")
        .arg(&fixture.downloads)
        .arg("--fetch")
        .arg(&fixture.downloads)
        .output()
        .unwrap_or_else(|error| panic!("run verify: {error}"));
    assert!(!both.status.success());
    assert!(String::from_utf8_lossy(&both.stderr)
        .contains("verify needs exactly one of --from or --fetch"));
}
