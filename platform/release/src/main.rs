pub mod fetch;
pub mod workflow;

use std::env;
use std::fmt::Write as _;
use std::fs;
use std::path::{Component, Path, PathBuf};

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use workflow::Node;

const MANIFEST_PATH: &str = "platform/release/registries.kvx";
const WORKFLOW_PATH: &str = ".github/workflows/platform.yml";
const RELEASE_JOB: &str = "release-pipeline";
const VERIFICATION_JOB: &str = "release-verification";
const PROMOTION_JOB: &str = "release-promotion";
const ARTIFACT_MANIFEST_SCHEMA: &str = "layerx/artifact-manifest/1";
const ARTIFACT_MANIFEST_FILE: &str = "artifact-manifest.json";
const ARTIFACT_MANIFEST_WORKFLOW_ARTIFACT: &str = "release-artifact-manifest";
const MANIFEST_COMMAND: &str = "-- manifest";
const VERIFY_COMMAND: &str = "-- verify";
const FETCH_FLAG: &str = "--fetch";
const PIPELINE_BUNDLE: &str = "release-pipeline";
const VERSION_RECORD: &str = "version.txt";
const SOURCE_DIGEST_RECORD: &str = "source-digest.txt";
const PUBLICATION_RECORD: &str = "publication.txt";
const ARTIFACTS_RECORD: &str = "artifacts.kvx";
const ARTIFACT_KEYS: [&str; 9] = [
    "name",
    "version",
    "file",
    "digest",
    "digest_of",
    "signature",
    "sbom",
    "attestation",
    "location",
];
const DIGEST_PREFIX: &str = "sha256:";
const DIGEST_OF: [&str; 3] = ["built-bytes", "registry-bytes", "source-archive"];
const VERIFIABLE_DIGEST_OF: [&str; 2] = ["built-bytes", "registry-bytes"];
const REFERENCE_SCHEMES: [&str; 4] = [
    "npm-provenance",
    "pypi-integrity",
    "go-checksum-database",
    "git-tag",
];
const LOCATION_SCHEMES: [&str; 3] = ["https://", "simple+https://", "git+https://"];
const HEX: &[u8; 16] = b"0123456789abcdef";
const RELEASE_CONDITION: &str =
    "github.event_name == 'workflow_dispatch' || startsWith(github.ref, 'refs/tags/sdk-v')";
const TAG_CONDITION: &str = "startsWith(github.ref, 'refs/tags/sdk-v')";
const GATE_RUNNER: &str = "tools/ci/release-gate.sh";
const REGISTRY_ENV: &str = "LAYERX_RELEASE_REGISTRY";
const DISTRIBUTION_ENV: &str = "LAYERX_RELEASE_DISTRIBUTION";
const PACKAGES_ENV: &str = "LAYERX_RELEASE_PACKAGES";
const DIGEST_RECOGNISER: &str = "sha256sum";
const SBOM_RECOGNISERS: [&str; 2] = ["syft scan", "npm sbom"];
const ATTESTATION_ACTION: &str = "actions/attest-build-provenance";

const REGISTRIES: [&str; 7] = [
    "crates-io",
    "npm",
    "pypi",
    "go-modules",
    "maven-central",
    "swiftpm",
    "nuget",
];

const REGISTRY_KEYS: [&str; 7] = [
    "ecosystem",
    "artifact",
    "distribution",
    "signing",
    "provenance",
    "verification",
    "status",
];

const MAVEN_CENTRAL_KEYS: [&str; 3] = ["coordinate", "languages", "module_name"];
const MAVEN_CENTRAL_COORDINATE: &str = "com.sidiora.layerx:layerx-sdk";
const MAVEN_CENTRAL_LANGUAGES: [&str; 2] = ["java", "kotlin"];
const MAVEN_CENTRAL_MODULE_NAME: &str = "com.sidiora.layerx.sdk";

const REFERENCE_APPLICATIONS: [&str; 4] = [
    "@sidiora/layerx-example-buyer-agent",
    "@sidiora/layerx-example-marketplace",
    "@sidiora/layerx-example-merchant-shop",
    "@sidiora/layerx-example-paid-api",
];

const STATUSES: [&str; 2] = ["skeleton", "active"];

struct ReleaseGate {
    job: &'static str,
    command: &'static str,
}

const RELEASE_GATES: [ReleaseGate; 4] = [
    ReleaseGate {
        job: "programs-acceptance",
        command: "make programs-test",
    },
    ReleaseGate {
        job: "agent-sanitizers",
        command: "make agent-test-sanitize",
    },
    ReleaseGate {
        job: "agent-fuzz-corpus",
        command: "make agent-fuzz-long",
    },
    ReleaseGate {
        job: "replay-matrix",
        command: "make test-replay-golden",
    },
];
const REPLAY_GATE: &str = "replay-matrix";
const REPLAY_MACHINES: [&str; 2] = ["aarch64", "x86_64"];

struct Publication {
    registry: &'static str,
    publish: &'static [&'static str],
    install: &'static [&'static str],
    explicit: &'static [&'static str],
}

const PUBLICATIONS: [Publication; 7] = [
    Publication {
        registry: "crates-io",
        publish: &["cargo publish"],
        install: &["cargo fetch"],
        explicit: &["-p", "--package"],
    },
    Publication {
        registry: "npm",
        publish: &["npm publish"],
        install: &["npm install"],
        explicit: &["--workspace", "-w"],
    },
    Publication {
        registry: "pypi",
        publish: &["pypa/gh-action-pypi-publish"],
        install: &["pip install"],
        explicit: &[],
    },
    Publication {
        registry: "go-modules",
        publish: &["git push", "refs/tags/platform/sdk/go/v"],
        install: &["go get"],
        explicit: &[],
    },
    Publication {
        registry: "maven-central",
        publish: &["mvn deploy", "central.sonatype.com/api/v1/publisher/upload"],
        install: &["dependency:get"],
        explicit: &[],
    },
    Publication {
        registry: "swiftpm",
        publish: &["git push", "LAYERX_RELEASE_SWIFTPM_REMOTE"],
        install: &["swift package resolve"],
        explicit: &[],
    },
    Publication {
        registry: "nuget",
        publish: &["dotnet nuget push"],
        install: &["dotnet add package"],
        explicit: &[],
    },
];

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Registry {
    pub name: String,
    pub declarations: Vec<(String, String)>,
    pub packages: Vec<String>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RegistriesDeclarations {
    pub tag_format: String,
    pub source_digest: String,
    pub reference_applications: Vec<String>,
    pub registries: Vec<Registry>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct GateJob {
    pub job: String,
    pub command: String,
    pub machines: Vec<String>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PublicationJob {
    pub registry: String,
    pub job: String,
    pub packages: Vec<String>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct VerificationJob {
    pub job: String,
    pub manifest: String,
    pub workflow_artifact: String,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ReleasePipeline {
    pub declarations: RegistriesDeclarations,
    pub gates: Vec<GateJob>,
    pub publications: Vec<PublicationJob>,
    pub verification: VerificationJob,
}

/// One published artifact bound to the source revision it was built from.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct ArtifactManifestEntry {
    pub name: String,
    pub version: String,
    pub registry: String,
    pub distribution: String,
    pub target: String,
    pub artifact: String,
    pub retained: String,
    pub digest: String,
    pub digest_of: String,
    pub signature: String,
    pub sbom: String,
    pub attestation: String,
    pub location: String,
    pub source_revision: String,
    pub rollback_version: Option<String>,
    pub published: bool,
    pub install_check: bool,
}

/// The source-bound artifact manifest of one release.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct ArtifactManifest {
    pub schema: String,
    pub version: String,
    pub tag: String,
    pub source_revision: String,
    pub source_digest: String,
    pub rollback_version: Option<String>,
    pub artifacts: Vec<ArtifactManifestEntry>,
}

/// Where `release_pipeline_verify` obtains the published bytes.
#[derive(Clone, Copy, Debug)]
pub enum ArtifactSource<'a> {
    /// A directory of already downloaded artifacts laid out as
    /// `<dir>/<registry>/<artifact>`.
    Directory(&'a Path),
    /// Every artifact is fetched from its registry into
    /// `<into>/<registry>/<artifact>` before it is hashed.
    Registries { into: &'a Path },
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct VerifiedArtifact {
    pub name: String,
    pub version: String,
    pub registry: String,
    pub digest: String,
}

/// Parses and validates the release registry manifest.
///
/// # Errors
///
/// Fails unless every mandated registry is declared exactly once with every
/// required declaration, a known status and its published package identities.
pub fn registries_declarations(source: &str) -> Result<RegistriesDeclarations, String> {
    let document = layerx_platform_kvx::parse(source)?;
    let declared = layerx_platform_kvx::string_list(document.required("release", "registries")?)?;
    if declared != REGISTRIES {
        return Err(format!(
            "release.registries must list exactly the seven mandated registries {REGISTRIES:?}, got {declared:?}"
        ));
    }
    for (key, _) in document.section_entries("release") {
        if !matches!(
            key,
            "registries" | "tag_format" | "source_digest" | "reference_applications"
        ) {
            return Err(format!("unknown declaration release.{key}"));
        }
    }
    let tag_format = layerx_platform_kvx::unquote(document.required("release", "tag_format")?)?;
    if !tag_format.contains("{version}") {
        return Err("release.tag_format must carry the {version} placeholder".to_owned());
    }
    let source_digest =
        layerx_platform_kvx::unquote(document.required("release", "source_digest")?)?;
    if source_digest.is_empty() {
        return Err("release.source_digest must not be empty".to_owned());
    }
    let reference_applications =
        layerx_platform_kvx::string_list(document.required("release", "reference_applications")?)?;
    if reference_applications != REFERENCE_APPLICATIONS {
        return Err(format!(
            "release.reference_applications must list exactly the four cloneable applications {REFERENCE_APPLICATIONS:?}, got {reference_applications:?}"
        ));
    }
    for section in document.sections() {
        if section == "release" {
            continue;
        }
        let Some(name) = section.strip_prefix("registry.") else {
            return Err(format!("unknown section {section}"));
        };
        if !REGISTRIES.contains(&name) {
            return Err(format!("unknown registry {name}"));
        }
    }
    let registries = REGISTRIES
        .iter()
        .map(|name| registry_declarations(&document, name))
        .collect::<Result<Vec<_>, _>>()?;
    Ok(RegistriesDeclarations {
        tag_format,
        source_digest,
        reference_applications,
        registries,
    })
}

fn registry_declarations(
    document: &layerx_platform_kvx::Document,
    name: &str,
) -> Result<Registry, String> {
    let section = format!("registry.{name}");
    let entries = document.section_entries(&section);
    if entries.is_empty() {
        return Err(format!("registry {name} is not declared"));
    }
    let mut declarations = Vec::new();
    let mut packages = Vec::new();
    for (key, value) in &entries {
        if *key == "packages" {
            packages = layerx_platform_kvx::string_list(value)?;
            if packages.is_empty()
                || packages.iter().any(String::is_empty)
                || packages.windows(2).any(|pair| pair[0] >= pair[1])
            {
                return Err(format!(
                    "{section}.packages must be a non-empty, sorted, duplicate-free string list"
                ));
            }
            continue;
        }
        if *key == "languages" && name == "maven-central" {
            let languages = layerx_platform_kvx::string_list(value)?;
            if languages != MAVEN_CENTRAL_LANGUAGES {
                return Err(format!(
                    "registry.maven-central.languages must list exactly {MAVEN_CENTRAL_LANGUAGES:?}, got {languages:?}"
                ));
            }
            declarations.push(((*key).to_owned(), languages.join(",")));
            continue;
        }
        if !REGISTRY_KEYS.contains(key) && !registry_specific_keys(name).contains(key) {
            return Err(format!("unknown declaration {section}.{key}"));
        }
        let value = layerx_platform_kvx::unquote(value)?;
        if value.is_empty() {
            return Err(format!("empty declaration {section}.{key}"));
        }
        if name == "maven-central"
            && ((*key == "coordinate" && value != MAVEN_CENTRAL_COORDINATE)
                || (*key == "module_name" && value != MAVEN_CENTRAL_MODULE_NAME))
        {
            return Err(format!("registry.maven-central.{key} is not canonical"));
        }
        declarations.push(((*key).to_owned(), value));
    }
    for key in REGISTRY_KEYS
        .iter()
        .chain(registry_specific_keys(name))
        .copied()
    {
        if !declarations.iter().any(|(declared, _)| declared == key) {
            return Err(format!("missing declaration {section}.{key}"));
        }
    }
    if packages.is_empty() {
        return Err(format!("missing declaration {section}.packages"));
    }
    let status = layerx_platform_kvx::unquote(document.required(&section, "status")?)?;
    if !STATUSES.contains(&status.as_str()) {
        return Err(format!(
            "registry {name} has unknown status {status}; expected one of {STATUSES:?}"
        ));
    }
    if name == "maven-central"
        && !packages
            .iter()
            .any(|package| package == MAVEN_CENTRAL_COORDINATE)
    {
        return Err(format!(
            "registry.maven-central.packages must carry the canonical coordinate {MAVEN_CENTRAL_COORDINATE}"
        ));
    }
    Ok(Registry {
        name: name.to_owned(),
        declarations,
        packages,
    })
}

fn registry_specific_keys(name: &str) -> &'static [&'static str] {
    if name == "maven-central" {
        &MAVEN_CENTRAL_KEYS
    } else {
        &[]
    }
}

fn declaration<'a>(registry: &'a Registry, key: &str) -> Result<&'a str, String> {
    registry
        .declarations
        .iter()
        .find(|(declared, _)| declared == key)
        .map(|(_, value)| value.as_str())
        .ok_or_else(|| format!("registry {} lost {key}", registry.name))
}

fn signing_recogniser(signing: &str) -> Result<&'static str, String> {
    match signing {
        "sigstore-keyless" => Ok("cosign sign-blob"),
        "git-tag-signature" => Ok("git tag -s"),
        "pgp-detached-signature" => Ok("--detach-sign"),
        "nuget-repository-signature" => Ok("dotnet nuget verify"),
        other => Err(format!("unknown signing scheme {other}")),
    }
}

fn provenance_recogniser(provenance: &str) -> Result<&'static str, String> {
    match provenance {
        "github-actions-attestation" => Ok(ATTESTATION_ACTION),
        "npm-provenance-attestation" => Ok("--provenance"),
        "pypi-trusted-publisher-attestation" => Ok("attestations: true"),
        "go-checksum-database" => Ok("GOSUMDB=sum.golang.org"),
        other => Err(format!("unknown provenance scheme {other}")),
    }
}

/// Validates the manifest against the release workflow in both directions.
///
/// # Errors
///
/// Fails when a declared registry has no publication job, a job publishes an
/// undeclared registry or package, a publication job lacks its digest,
/// signature, SBOM, provenance or install check, or a release gate is
/// missing, conditional or absent from the release job's dependencies.
pub fn release_pipeline(manifest: &str, workflow_source: &str) -> Result<ReleasePipeline, String> {
    let declarations = registries_declarations(manifest)?;
    let workflow =
        workflow::parse(workflow_source).map_err(|error| format!("release workflow: {error}"))?;
    tag_trigger(&workflow, &declarations.tag_format)?;
    let jobs = workflow
        .get("jobs")
        .filter(|jobs| !jobs.entries().is_empty())
        .ok_or_else(|| "release workflow declares no jobs".to_owned())?;
    release_pipeline_workflow_job(jobs)?;
    let gates = gate_jobs(jobs)?;
    let publications = declarations
        .registries
        .iter()
        .map(|registry| publication_job(jobs, registry))
        .collect::<Result<Vec<_>, _>>()?;
    undeclared_publications(jobs, &publications)?;
    let verification = verification_job(jobs, &publications)?;
    promotion_job(jobs, &publications)?;
    Ok(ReleasePipeline {
        declarations,
        gates,
        publications,
        verification,
    })
}

fn tag_trigger(workflow: &Node, tag_format: &str) -> Result<(), String> {
    let pattern = tag_format.replace("{version}", "*");
    let tags = workflow
        .path(&["on", "push", "tags"])
        .map(Node::strings)
        .unwrap_or_default();
    if tags.iter().any(|tag| *tag == pattern) {
        Ok(())
    } else {
        Err(format!(
            "release workflow on.push.tags must carry the release tag pattern {pattern}, got {tags:?}"
        ))
    }
}

fn job<'a>(jobs: &'a Node, id: &str) -> Result<&'a Node, String> {
    jobs.get(id)
        .filter(|job| !job.entries().is_empty())
        .ok_or_else(|| format!("release workflow has no job {id}"))
}

fn needs(job: &Node) -> Vec<&str> {
    job.get("needs").map(Node::strings).unwrap_or_default()
}

fn condition(job: &Node) -> Option<&str> {
    job.get("if").and_then(Node::as_str).map(str::trim)
}

fn env_value<'a>(job: &'a Node, key: &str) -> Option<&'a str> {
    job.path(&["env", key])
        .and_then(Node::as_str)
        .map(str::trim)
}

fn permission<'a>(job: &'a Node, key: &str) -> Option<&'a str> {
    job.path(&["permissions", key])
        .and_then(Node::as_str)
        .map(str::trim)
}

fn release_pipeline_workflow_job(jobs: &Node) -> Result<(), String> {
    let release = job(jobs, RELEASE_JOB)?;
    if condition(release) != Some(RELEASE_CONDITION) {
        return Err(format!(
            "job {RELEASE_JOB} must run under the release condition `{RELEASE_CONDITION}`"
        ));
    }
    let dependencies = needs(release);
    for gate in &RELEASE_GATES {
        if !dependencies.contains(&gate.job) {
            return Err(format!(
                "job {RELEASE_JOB} must need the release gate {} before publication",
                gate.job
            ));
        }
    }
    Ok(())
}

fn gate_jobs(jobs: &Node) -> Result<Vec<GateJob>, String> {
    let mut gates = Vec::new();
    for gate in &RELEASE_GATES {
        let node = job(jobs, gate.job)?;
        if let Some(condition) = condition(node) {
            if condition != RELEASE_CONDITION {
                return Err(format!(
                    "release gate {} is conditional on `{condition}`; release gates may only run under the release condition `{RELEASE_CONDITION}`",
                    gate.job
                ));
            }
        }
        let text = node.text();
        if !text.contains(GATE_RUNNER) {
            return Err(format!(
                "release gate {} must run through {GATE_RUNNER} so its gate record is emitted",
                gate.job
            ));
        }
        if !text.contains(gate.command) {
            return Err(format!(
                "release gate {} must run `{}`",
                gate.job, gate.command
            ));
        }
        let mut machines = Vec::new();
        if gate.job == REPLAY_GATE {
            machines = node
                .path(&["strategy", "matrix", "include"])
                .map(Node::items)
                .unwrap_or_default()
                .iter()
                .filter_map(|entry| entry.get("machine").and_then(Node::as_str))
                .map(str::to_owned)
                .collect::<Vec<_>>();
            machines.sort();
            if machines != REPLAY_MACHINES {
                return Err(format!(
                    "release gate {REPLAY_GATE} must replay on every supported architecture {REPLAY_MACHINES:?}, got {machines:?}"
                ));
            }
        }
        gates.push(GateJob {
            job: gate.job.to_owned(),
            command: gate.command.to_owned(),
            machines,
        });
    }
    Ok(gates)
}

fn publication_rules(registry: &str) -> Result<&'static Publication, String> {
    PUBLICATIONS
        .iter()
        .find(|rules| rules.registry == registry)
        .ok_or_else(|| format!("registry {registry} has no publication rules"))
}

fn explicit_packages(text: &str, flags: &[&str]) -> Vec<String> {
    let mut packages = Vec::new();
    for line in text.lines() {
        let tokens = line.split_whitespace().collect::<Vec<_>>();
        for (index, token) in tokens.iter().enumerate() {
            if flags.contains(token) {
                if let Some(name) = tokens.get(index + 1) {
                    let name = name.trim_matches(['"', '\'']);
                    if !name.is_empty() && !name.starts_with('$') && !name.starts_with('-') {
                        packages.push(name.to_owned());
                    }
                }
            }
        }
    }
    packages
}

fn publication_job(jobs: &Node, registry: &Registry) -> Result<PublicationJob, String> {
    let rules = publication_rules(&registry.name)?;
    let carriers = jobs
        .entries()
        .iter()
        .filter(|(_, node)| env_value(node, REGISTRY_ENV) == Some(registry.name.as_str()))
        .map(|(id, node)| (id.as_str(), node))
        .collect::<Vec<_>>();
    let (id, node) = match carriers.as_slice() {
        [] => {
            return Err(format!(
                "registry {} is declared but no publication job carries {REGISTRY_ENV}={}",
                registry.name, registry.name
            ))
        }
        [single] => *single,
        _ => {
            return Err(format!(
                "registry {} is published by more than one job: {:?}",
                registry.name,
                carriers.iter().map(|(id, _)| *id).collect::<Vec<_>>()
            ))
        }
    };
    let expected = format!("publish-{}", registry.name);
    if id != expected {
        return Err(format!(
            "publication job for {} must be named {expected}, found {id}",
            registry.name
        ));
    }
    if declaration(registry, "status")? != "active" {
        return Err(format!(
            "registry {} is published by job {id} but its manifest status is not active",
            registry.name
        ));
    }
    if condition(node) != Some(RELEASE_CONDITION) {
        return Err(format!(
            "job {id} must run under the release condition `{RELEASE_CONDITION}`"
        ));
    }
    if !needs(node).contains(&RELEASE_JOB) {
        return Err(format!("job {id} must need {RELEASE_JOB}"));
    }
    let distribution = declaration(registry, "distribution")?;
    if env_value(node, DISTRIBUTION_ENV) != Some(distribution) {
        return Err(format!(
            "job {id} must target {DISTRIBUTION_ENV}={distribution} as the manifest declares"
        ));
    }
    let mut packages = env_value(node, PACKAGES_ENV)
        .unwrap_or_default()
        .split_whitespace()
        .map(str::to_owned)
        .collect::<Vec<_>>();
    packages.sort();
    packages.dedup();
    if packages != registry.packages {
        return Err(format!(
            "job {id} publishes {PACKAGES_ENV}={packages:?} but the manifest declares {:?}",
            registry.packages
        ));
    }
    for step in node.get("steps").map(Node::items).unwrap_or_default() {
        if let Some(condition) = condition(step) {
            if condition != TAG_CONDITION {
                return Err(format!(
                    "job {id} has a step conditional on `{condition}`; only `{TAG_CONDITION}` may gate publication steps"
                ));
            }
        }
    }
    publication_evidence(id, node, registry, rules)?;
    Ok(PublicationJob {
        registry: registry.name.clone(),
        job: id.to_owned(),
        packages,
    })
}

fn publication_evidence(
    id: &str,
    node: &Node,
    registry: &Registry,
    rules: &Publication,
) -> Result<(), String> {
    let text = node.text();
    let missing = |what: &str, needle: &str| {
        format!("job {id} lacks its {what}: expected `{needle}` in the job")
    };
    for needle in rules.publish {
        if !text.contains(needle) {
            return Err(missing("publication command", needle));
        }
    }
    for needle in rules.install {
        if !text.contains(needle) {
            return Err(missing("install check from the registry", needle));
        }
    }
    if !text.contains(DIGEST_RECOGNISER) {
        return Err(missing("immutable digest", DIGEST_RECOGNISER));
    }
    if !SBOM_RECOGNISERS.iter().any(|needle| text.contains(needle)) {
        return Err(format!(
            "job {id} lacks its SBOM: expected one of {SBOM_RECOGNISERS:?} in the job"
        ));
    }
    let signing = signing_recogniser(declaration(registry, "signing")?)?;
    if !text.contains(signing) {
        return Err(missing("signature", signing));
    }
    let provenance = provenance_recogniser(declaration(registry, "provenance")?)?;
    if !text.contains(provenance) {
        return Err(missing("provenance attestation", provenance));
    }
    if permission(node, "id-token") != Some("write") {
        return Err(format!(
            "job {id} must grant permissions.id-token: write for keyless signing and attestation"
        ));
    }
    if text.contains(ATTESTATION_ACTION) && permission(node, "attestations") != Some("write") {
        return Err(format!(
            "job {id} must grant permissions.attestations: write to store its provenance attestation"
        ));
    }
    let explicit = explicit_packages(&text, rules.explicit);
    for package in explicit {
        if !registry.packages.contains(&package) {
            return Err(format!(
                "job {id} publishes {package}, which the manifest does not declare for {}",
                registry.name
            ));
        }
    }
    Ok(())
}

fn undeclared_publications(jobs: &Node, publications: &[PublicationJob]) -> Result<(), String> {
    for (id, node) in jobs.entries() {
        let declared = publications
            .iter()
            .find(|publication| publication.job == *id);
        if let Some(registry) = env_value(node, REGISTRY_ENV) {
            if declared.is_none() {
                return Err(format!(
                    "job {id} publishes {REGISTRY_ENV}={registry}, which the manifest does not declare"
                ));
            }
        } else if id.starts_with("publish-") {
            return Err(format!(
                "job {id} looks like a publication job but carries no {REGISTRY_ENV}"
            ));
        }
        let text = node.text();
        for rules in &PUBLICATIONS {
            if rules.publish.iter().all(|needle| text.contains(needle))
                && declared.is_none_or(|publication| publication.registry != rules.registry)
            {
                return Err(format!(
                    "job {id} publishes to {} outside the declared publication job",
                    rules.registry
                ));
            }
        }
    }
    Ok(())
}

fn verification_job(
    jobs: &Node,
    publications: &[PublicationJob],
) -> Result<VerificationJob, String> {
    let node = job(jobs, VERIFICATION_JOB)?;
    if condition(node) != Some(RELEASE_CONDITION) {
        return Err(format!(
            "job {VERIFICATION_JOB} must run under the release condition `{RELEASE_CONDITION}`"
        ));
    }
    let dependencies = needs(node);
    if !dependencies.contains(&RELEASE_JOB) {
        return Err(format!("job {VERIFICATION_JOB} must need {RELEASE_JOB}"));
    }
    for publication in publications {
        if !dependencies.contains(&publication.job.as_str()) {
            return Err(format!(
                "job {VERIFICATION_JOB} must need {} so every published artifact is verified before promotion",
                publication.job
            ));
        }
    }
    for step in node.get("steps").map(Node::items).unwrap_or_default() {
        if let Some(condition) = condition(step) {
            if condition != TAG_CONDITION {
                return Err(format!(
                    "job {VERIFICATION_JOB} has a step conditional on `{condition}`; only `{TAG_CONDITION}` may gate verification steps"
                ));
            }
        }
    }
    let text = node.text();
    let missing = |what: &str, needle: &str| {
        format!("job {VERIFICATION_JOB} lacks its {what}: expected `{needle}` in the job")
    };
    if !text.contains(MANIFEST_COMMAND) {
        return Err(missing("artifact manifest emission", MANIFEST_COMMAND));
    }
    if !text.contains(VERIFY_COMMAND) {
        return Err(missing("published-bytes verification", VERIFY_COMMAND));
    }
    if !text.contains(FETCH_FLAG) {
        return Err(missing("fetch from every registry", FETCH_FLAG));
    }
    if !text.contains(ARTIFACT_MANIFEST_FILE) {
        return Err(missing("artifact manifest file", ARTIFACT_MANIFEST_FILE));
    }
    let retention = format!("name: {ARTIFACT_MANIFEST_WORKFLOW_ARTIFACT}");
    if !text.contains(&retention) {
        return Err(missing("retained manifest artifact", &retention));
    }
    Ok(VerificationJob {
        job: VERIFICATION_JOB.to_owned(),
        manifest: ARTIFACT_MANIFEST_FILE.to_owned(),
        workflow_artifact: ARTIFACT_MANIFEST_WORKFLOW_ARTIFACT.to_owned(),
    })
}

fn promotion_job(jobs: &Node, publications: &[PublicationJob]) -> Result<(), String> {
    let promotion = job(jobs, PROMOTION_JOB)?;
    if condition(promotion).is_some_and(|condition| condition != RELEASE_CONDITION) {
        return Err(format!(
            "job {PROMOTION_JOB} may only be conditional on `{RELEASE_CONDITION}`"
        ));
    }
    let dependencies = needs(promotion);
    if !dependencies.contains(&RELEASE_JOB) {
        return Err(format!("job {PROMOTION_JOB} must need {RELEASE_JOB}"));
    }
    for publication in publications {
        if !dependencies.contains(&publication.job.as_str()) {
            return Err(format!(
                "job {PROMOTION_JOB} must need {} so no partial promotion is presented as a release",
                publication.job
            ));
        }
    }
    if !dependencies.contains(&VERIFICATION_JOB) {
        return Err(format!(
            "job {PROMOTION_JOB} must need {VERIFICATION_JOB} so no unverified artifact is promoted"
        ));
    }
    if !promotion.text().contains(ARTIFACT_MANIFEST_FILE) {
        return Err(format!(
            "job {PROMOTION_JOB} must promote the retained {ARTIFACT_MANIFEST_FILE}"
        ));
    }
    Ok(())
}

fn hex(bytes: &[u8]) -> String {
    let mut text = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        text.push(char::from(HEX[usize::from(byte >> 4)]));
        text.push(char::from(HEX[usize::from(byte & 0x0f)]));
    }
    text
}

#[must_use]
pub fn sha256_digest(bytes: &[u8]) -> String {
    format!("{DIGEST_PREFIX}{}", hex(&Sha256::digest(bytes)))
}

fn file_digest(path: &Path) -> Result<String, String> {
    let bytes = fs::read(path).map_err(|error| format!("read {}: {error}", path.display()))?;
    Ok(sha256_digest(&bytes))
}

fn is_hex(value: &str, length: usize) -> bool {
    value.len() == length
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

fn checked_digest(value: &str, what: &str) -> Result<String, String> {
    match value.strip_prefix(DIGEST_PREFIX) {
        Some(digest) if is_hex(digest, 64) => Ok(value.to_owned()),
        _ => Err(format!(
            "{what} must be {DIGEST_PREFIX}<64 lowercase hex digits>, got {value}"
        )),
    }
}

fn record_values(source: &str, record: &str) -> Result<Vec<(String, String)>, String> {
    let mut values: Vec<(String, String)> = Vec::new();
    for (index, line) in source.lines().enumerate() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        let (key, value) = line
            .split_once('=')
            .ok_or_else(|| format!("{record} line {}: expected key=value", index + 1))?;
        let key = key.trim();
        if values.iter().any(|(existing, _)| existing == key) {
            return Err(format!("{record} line {}: duplicate key {key}", index + 1));
        }
        values.push((key.to_owned(), value.trim().to_owned()));
    }
    Ok(values)
}

fn record_value<'a>(
    values: &'a [(String, String)],
    key: &str,
    record: &str,
) -> Result<&'a str, String> {
    values
        .iter()
        .find(|(existing, _)| existing == key)
        .map(|(_, value)| value.as_str())
        .filter(|value| !value.is_empty())
        .ok_or_else(|| format!("{record} lacks {key}"))
}

struct PublicationRecord {
    registry: String,
    version: String,
    distribution: String,
    target: String,
    packages: Vec<String>,
    revision: String,
    published: bool,
    install_check: bool,
}

fn publication_record(source: &str, record: &str) -> Result<PublicationRecord, String> {
    let values = record_values(source, record)?;
    let mut packages = record_value(&values, "packages", record)?
        .split_whitespace()
        .map(str::to_owned)
        .collect::<Vec<_>>();
    packages.sort();
    packages.dedup();
    let flag = |key: &str, truth: &str| {
        values
            .iter()
            .any(|(existing, value)| existing == key && value == truth)
    };
    Ok(PublicationRecord {
        registry: record_value(&values, "registry", record)?.to_owned(),
        version: record_value(&values, "version", record)?.to_owned(),
        distribution: record_value(&values, "distribution", record)?.to_owned(),
        target: record_value(&values, "target", record)?.to_owned(),
        packages,
        revision: record_value(&values, "revision", record)?.to_owned(),
        published: flag("published", "true"),
        install_check: flag("install_check", "pass"),
    })
}

fn artifact_records(source: &str, record: &str) -> Result<Vec<Vec<(String, String)>>, String> {
    let document =
        layerx_platform_kvx::parse(source).map_err(|error| format!("{record}: {error}"))?;
    let mut records = Vec::new();
    for section in document.sections() {
        if !section.starts_with("artifact.") {
            return Err(format!("{record}: unknown section {section}"));
        }
        let mut values = Vec::new();
        for (key, value) in document.section_entries(section) {
            if !ARTIFACT_KEYS.contains(&key) {
                return Err(format!("{record}: unknown declaration {section}.{key}"));
            }
            let value = layerx_platform_kvx::unquote(value)
                .map_err(|error| format!("{record}: {section}.{key}: {error}"))?;
            values.push((key.to_owned(), value));
        }
        for key in ARTIFACT_KEYS {
            if !values.iter().any(|(existing, _)| existing == key) {
                return Err(format!("{record}: missing declaration {section}.{key}"));
            }
        }
        records.push(values);
    }
    if records.is_empty() {
        return Err(format!("{record} lists no artifacts"));
    }
    Ok(records)
}

fn relative_file(bundle: &Path, value: &str, what: &str) -> Result<PathBuf, String> {
    let relative = Path::new(value);
    if value.is_empty()
        || !relative
            .components()
            .all(|component| matches!(component, Component::Normal(_)))
    {
        return Err(format!(
            "{what} must be a relative path inside the retained bundle, got {value}"
        ));
    }
    let path = bundle.join(relative);
    if !path.is_file() {
        return Err(format!(
            "{what} names {value}, which is not a retained file"
        ));
    }
    Ok(path)
}

fn external_reference(value: &str) -> bool {
    value
        .split_once(':')
        .is_some_and(|(scheme, locator)| REFERENCE_SCHEMES.contains(&scheme) && !locator.is_empty())
}

fn checked_reference(
    bundle: &Path,
    value: &str,
    required: bool,
    what: &str,
) -> Result<String, String> {
    if value.is_empty() {
        return Err(format!("{what} is empty"));
    }
    if external_reference(value) {
        return Ok(value.to_owned());
    }
    if let Some((scheme, _)) = value.split_once(':') {
        if !scheme.contains('/') && !scheme.contains('.') {
            return Err(format!(
                "{what} uses the unknown reference scheme {scheme}; expected a retained file or one of {REFERENCE_SCHEMES:?}"
            ));
        }
    }
    if required {
        relative_file(bundle, value, what)?;
    }
    Ok(value.to_owned())
}

fn checked_location(value: &str, published: bool, what: &str) -> Result<String, String> {
    if LOCATION_SCHEMES
        .iter()
        .any(|scheme| value.starts_with(scheme) && value.len() > scheme.len())
    {
        return Ok(value.to_owned());
    }
    if !published && value.is_empty() {
        return Ok(String::new());
    }
    Err(format!(
        "{what} must be a registry location starting with one of {LOCATION_SCHEMES:?}, got {value:?}"
    ))
}

fn checked_version(value: &str, what: &str) -> Result<String, String> {
    if value.is_empty()
        || value.contains(char::is_whitespace)
        || !value.starts_with(|character: char| character.is_ascii_digit())
    {
        return Err(format!("{what} must be a release version, got {value:?}"));
    }
    Ok(value.to_owned())
}

/// Builds the source-bound artifact manifest from the retained release bundle.
///
/// The bundle holds `release-pipeline/{version.txt,source-digest.txt}` and,
/// for every declared registry, `release-<registry>/publication.txt` and
/// `release-<registry>/artifacts.kvx` next to the retained artifacts, as the
/// release workflow uploads them.
///
/// # Errors
///
/// Fails when a registry's publication record disagrees with the manifest or
/// the pipeline version, an artifact is not a declared package, a declared
/// package has no artifact, a retained artifact's bytes do not hash to its
/// recorded digest, or a published artifact lacks its signature, SBOM,
/// attestation or registry location.
pub fn artifact_manifest(
    pipeline: &ReleasePipeline,
    release_dir: &Path,
    rollback_version: Option<&str>,
) -> Result<ArtifactManifest, String> {
    let (version, source_digest) = release_header(release_dir)?;
    let rollback_version = match rollback_version {
        None => None,
        Some(rollback) => {
            let rollback = checked_version(rollback, "rollback version")?;
            if rollback == version {
                return Err(format!(
                    "rollback version {rollback} is the version being released"
                ));
            }
            Some(rollback)
        }
    };
    let tag = pipeline
        .declarations
        .tag_format
        .replace("{version}", &version);
    let mut source_revision: Option<String> = None;
    let mut artifacts = Vec::new();
    for publication in &pipeline.publications {
        let registry = pipeline
            .declarations
            .registries
            .iter()
            .find(|registry| registry.name == publication.registry)
            .ok_or_else(|| format!("registry {} lost its declarations", publication.registry))?;
        let distribution = declaration(registry, "distribution")?;
        let bundle_name = format!("release-{}", publication.registry);
        let bundle = release_dir.join(&bundle_name);
        let publication_name = format!("{bundle_name}/{PUBLICATION_RECORD}");
        let record = checked_publication(
            publication,
            distribution,
            &bundle,
            &publication_name,
            &version,
        )?;
        match &source_revision {
            None => source_revision = Some(record.revision.clone()),
            Some(revision) if *revision == record.revision => {}
            Some(revision) => {
                return Err(format!(
                    "{publication_name} records revision {}, but {revision} published the other registries",
                    record.revision
                ))
            }
        }
        let scope = ArtifactScope {
            publication,
            distribution,
            bundle: &bundle,
            bundle_name: &bundle_name,
            record: &record,
            version: &version,
            rollback_version: rollback_version.as_deref(),
        };
        artifacts.extend(registry_artifacts(&scope)?);
    }
    let source_revision =
        source_revision.ok_or_else(|| "the release published no registries".to_owned())?;
    Ok(ArtifactManifest {
        schema: ARTIFACT_MANIFEST_SCHEMA.to_owned(),
        version,
        tag,
        source_revision,
        source_digest,
        rollback_version,
        artifacts,
    })
}

fn release_header(release_dir: &Path) -> Result<(String, String), String> {
    let pipeline_record = format!("{PIPELINE_BUNDLE}/{VERSION_RECORD}");
    let pipeline_dir = release_dir.join(PIPELINE_BUNDLE);
    let version_values =
        record_values(&read(&pipeline_dir.join(VERSION_RECORD))?, &pipeline_record)?;
    let version = checked_version(
        record_value(&version_values, "version", &pipeline_record)?,
        &format!("{pipeline_record} version"),
    )?;
    let digest_record = format!("{PIPELINE_BUNDLE}/{SOURCE_DIGEST_RECORD}");
    let source_digest = read(&pipeline_dir.join(SOURCE_DIGEST_RECORD))?
        .split_whitespace()
        .next()
        .filter(|digest| is_hex(digest, 64))
        .map(|digest| format!("{DIGEST_PREFIX}{digest}"))
        .ok_or_else(|| {
            format!("{digest_record} must start with the sha256 of the source archive")
        })?;
    Ok((version, source_digest))
}

fn checked_publication(
    publication: &PublicationJob,
    distribution: &str,
    bundle: &Path,
    publication_name: &str,
    version: &str,
) -> Result<PublicationRecord, String> {
    let record = publication_record(&read(&bundle.join(PUBLICATION_RECORD))?, publication_name)?;
    if record.registry != publication.registry {
        return Err(format!(
            "{publication_name} records registry {}, expected {}",
            record.registry, publication.registry
        ));
    }
    if record.version != version {
        return Err(format!(
            "{publication_name} records version {}, but the pipeline released {version}",
            record.version
        ));
    }
    if record.distribution != distribution {
        return Err(format!(
            "{publication_name} records distribution {}, but the manifest declares {distribution}",
            record.distribution
        ));
    }
    if record.packages != publication.packages {
        return Err(format!(
            "{publication_name} records packages {:?}, but job {} publishes {:?}",
            record.packages, publication.job, publication.packages
        ));
    }
    if !is_hex(&record.revision, 40) {
        return Err(format!(
            "{publication_name} records revision {:?}, expected a 40-hex commit",
            record.revision
        ));
    }
    Ok(record)
}

struct ArtifactScope<'a> {
    publication: &'a PublicationJob,
    distribution: &'a str,
    bundle: &'a Path,
    bundle_name: &'a str,
    record: &'a PublicationRecord,
    version: &'a str,
    rollback_version: Option<&'a str>,
}

fn registry_artifacts(scope: &ArtifactScope<'_>) -> Result<Vec<ArtifactManifestEntry>, String> {
    let artifacts_name = format!("{}/{ARTIFACTS_RECORD}", scope.bundle_name);
    let mut entries: Vec<ArtifactManifestEntry> = Vec::new();
    for values in artifact_records(
        &read(&scope.bundle.join(ARTIFACTS_RECORD))?,
        &artifacts_name,
    )? {
        let entry = artifact_entry(scope, &values, &artifacts_name)?;
        if entries
            .iter()
            .any(|existing| existing.artifact == entry.artifact)
        {
            return Err(format!(
                "{artifacts_name} lists the artifact file {} twice",
                entry.artifact
            ));
        }
        entries.push(entry);
    }
    for package in &scope.publication.packages {
        if !entries.iter().any(|entry| entry.name == *package) {
            return Err(format!(
                "{artifacts_name} lists no artifact for the declared package {package}"
            ));
        }
    }
    entries.sort_by(|left, right| {
        left.name
            .cmp(&right.name)
            .then_with(|| left.artifact.cmp(&right.artifact))
    });
    Ok(entries)
}

fn artifact_entry(
    scope: &ArtifactScope<'_>,
    values: &[(String, String)],
    artifacts_name: &str,
) -> Result<ArtifactManifestEntry, String> {
    let registry = &scope.publication.registry;
    let version = scope.version;
    let value = |key: &str| record_value(values, key, artifacts_name);
    let name = value("name")?;
    if !scope
        .publication
        .packages
        .iter()
        .any(|package| package == name)
    {
        return Err(format!(
            "{artifacts_name} lists {name}, which the manifest does not declare for {registry}"
        ));
    }
    let what = |field: &str| format!("{artifacts_name} {name} {field}");
    let entry_version = value("version")?;
    if entry_version != version {
        return Err(format!(
            "{} is {entry_version}, but the pipeline released {version}",
            what("version")
        ));
    }
    let retained = value("file")?;
    let path = relative_file(scope.bundle, retained, &what("file"))?;
    let artifact = path
        .file_name()
        .and_then(|name| name.to_str())
        .map(str::to_owned)
        .ok_or_else(|| format!("{} has no file name", what("file")))?;
    let digest = checked_digest(value("digest")?, &what("digest"))?;
    let actual = file_digest(&path)?;
    if actual != digest {
        return Err(format!(
            "retained artifact {name}@{version} from {registry} ({retained}) hashes to {actual}, but its record lists {digest}"
        ));
    }
    let digest_of = value("digest_of")?;
    if !DIGEST_OF.contains(&digest_of) {
        return Err(format!(
            "{} is {digest_of}; expected one of {DIGEST_OF:?}",
            what("digest_of")
        ));
    }
    let published = scope.record.published;
    Ok(ArtifactManifestEntry {
        name: name.to_owned(),
        version: version.to_owned(),
        registry: registry.clone(),
        distribution: scope.distribution.to_owned(),
        target: scope.record.target.clone(),
        artifact,
        retained: format!("{}/{retained}", scope.bundle_name),
        digest,
        digest_of: digest_of.to_owned(),
        signature: checked_reference(
            scope.bundle,
            value("signature")?,
            published,
            &what("signature"),
        )?,
        sbom: checked_reference(scope.bundle, value("sbom")?, true, &what("sbom"))?,
        attestation: checked_reference(
            scope.bundle,
            value("attestation")?,
            published,
            &what("attestation"),
        )?,
        location: checked_location(value("location")?, published, &what("location"))?,
        source_revision: scope.record.revision.clone(),
        rollback_version: scope.rollback_version.map(str::to_owned),
        published,
        install_check: scope.record.install_check,
    })
}

/// Renders the artifact manifest as its canonical JSON document.
///
/// # Errors
///
/// Fails only when JSON encoding fails.
pub fn render_artifact_manifest(manifest: &ArtifactManifest) -> Result<String, String> {
    serde_json::to_string_pretty(manifest)
        .map(|text| format!("{text}\n"))
        .map_err(|error| format!("render artifact manifest: {error}"))
}

/// Parses a rendered artifact manifest.
///
/// # Errors
///
/// Fails on malformed JSON, an unknown schema, an empty artifact list or a
/// malformed digest.
pub fn parse_artifact_manifest(source: &str) -> Result<ArtifactManifest, String> {
    let manifest: ArtifactManifest =
        serde_json::from_str(source).map_err(|error| format!("artifact manifest: {error}"))?;
    if manifest.schema != ARTIFACT_MANIFEST_SCHEMA {
        return Err(format!(
            "artifact manifest schema is {}, expected {ARTIFACT_MANIFEST_SCHEMA}",
            manifest.schema
        ));
    }
    if manifest.artifacts.is_empty() {
        return Err("artifact manifest lists no artifacts".to_owned());
    }
    checked_digest(&manifest.source_digest, "artifact manifest source_digest")?;
    if !is_hex(&manifest.source_revision, 40) {
        return Err(format!(
            "artifact manifest source_revision {:?} is not a 40-hex commit",
            manifest.source_revision
        ));
    }
    for entry in &manifest.artifacts {
        checked_digest(
            &entry.digest,
            &format!("artifact manifest {}@{} digest", entry.name, entry.version),
        )?;
        if entry.artifact.is_empty() || entry.artifact.contains('/') {
            return Err(format!(
                "artifact manifest {}@{} names the artifact file {:?}",
                entry.name, entry.version, entry.artifact
            ));
        }
    }
    Ok(manifest)
}

fn artifact_failure(entry: &ArtifactManifestEntry, reason: &str) -> String {
    format!(
        "{}@{} from {} ({reason})",
        entry.name, entry.version, entry.registry
    )
}

fn published_bytes(
    entry: &ArtifactManifestEntry,
    source: ArtifactSource<'_>,
) -> Result<(PathBuf, String), String> {
    let (path, origin) = match source {
        ArtifactSource::Directory(directory) => (
            directory.join(&entry.registry).join(&entry.artifact),
            "the downloaded".to_owned(),
        ),
        ArtifactSource::Registries { into } => {
            let path = into.join(&entry.registry).join(&entry.artifact);
            fetch::fetch(entry, &path)?;
            (path, format!("the bytes {} served as", entry.location))
        }
    };
    if !path.is_file() {
        return Err(format!("{} is not present at {}", origin, path.display()));
    }
    Ok((path, origin))
}

/// Verifies every published artifact's bytes against the manifest and halts
/// the release, naming each failing artifact, when any disagree.
///
/// # Errors
///
/// Fails, naming every failing artifact, when the manifest is bound to a
/// different source revision, an artifact was not published or did not pass
/// its install check, its digest is not of registry bytes, its bytes cannot be
/// obtained, or its bytes do not hash to the manifest digest.
pub fn release_pipeline_verify(
    manifest: &ArtifactManifest,
    source: ArtifactSource<'_>,
    source_revision: Option<&str>,
) -> Result<Vec<VerifiedArtifact>, String> {
    if manifest.schema != ARTIFACT_MANIFEST_SCHEMA {
        return Err(format!(
            "release halted before promotion: artifact manifest schema is {}, expected {ARTIFACT_MANIFEST_SCHEMA}",
            manifest.schema
        ));
    }
    if let Some(revision) = source_revision {
        if revision != manifest.source_revision {
            return Err(format!(
                "release halted before promotion: the artifact manifest binds revision {}, not the release revision {revision}",
                manifest.source_revision
            ));
        }
    }
    if manifest.artifacts.is_empty() {
        return Err(
            "release halted before promotion: the artifact manifest lists no artifacts".to_owned(),
        );
    }
    let mut verified = Vec::new();
    let mut failures = Vec::new();
    for entry in &manifest.artifacts {
        if !entry.published {
            failures.push(artifact_failure(entry, "was not published"));
            continue;
        }
        if !entry.install_check {
            failures.push(artifact_failure(
                entry,
                "install check from the registry did not pass",
            ));
            continue;
        }
        if !VERIFIABLE_DIGEST_OF.contains(&entry.digest_of.as_str()) {
            failures.push(artifact_failure(
                entry,
                &format!(
                    "manifest digest is of the {}, not of the bytes the registry serves",
                    entry.digest_of
                ),
            ));
            continue;
        }
        if checked_digest(&entry.digest, "digest").is_err() {
            failures.push(artifact_failure(
                entry,
                &format!("manifest digest {} is malformed", entry.digest),
            ));
            continue;
        }
        let (path, origin) = match published_bytes(entry, source) {
            Ok(found) => found,
            Err(error) => {
                failures.push(artifact_failure(entry, &error));
                continue;
            }
        };
        let actual = match file_digest(&path) {
            Ok(actual) => actual,
            Err(error) => {
                failures.push(artifact_failure(entry, &error));
                continue;
            }
        };
        if actual != entry.digest {
            failures.push(artifact_failure(
                entry,
                &format!(
                    "{origin} {} hash to {actual}, but the manifest lists {}",
                    entry.artifact, entry.digest
                ),
            ));
            continue;
        }
        verified.push(VerifiedArtifact {
            name: entry.name.clone(),
            version: entry.version.clone(),
            registry: entry.registry.clone(),
            digest: entry.digest.clone(),
        });
    }
    if failures.is_empty() {
        Ok(verified)
    } else {
        Err(format!(
            "release halted before promotion: {} of {} artifacts failed verification: {}",
            failures.len(),
            manifest.artifacts.len(),
            failures.join("; ")
        ))
    }
}

/// Renders the machine-readable release plan.
///
/// # Errors
///
/// Fails only when formatting the plan fails.
pub fn plan(pipeline: &ReleasePipeline) -> Result<String, String> {
    let mut text = String::new();
    let fail = |error| format!("render plan: {error}");
    let declarations = &pipeline.declarations;
    writeln!(text, "tag_format={}", declarations.tag_format).map_err(fail)?;
    writeln!(text, "source_digest={}", declarations.source_digest).map_err(fail)?;
    writeln!(
        text,
        "reference_applications={}",
        declarations.reference_applications.join(",")
    )
    .map_err(fail)?;
    for registry in &declarations.registries {
        write!(text, "registry={}", registry.name).map_err(fail)?;
        for key in REGISTRY_KEYS
            .iter()
            .chain(registry_specific_keys(&registry.name))
            .copied()
        {
            write!(text, " {key}={}", declaration(registry, key)?).map_err(fail)?;
        }
        let publication = pipeline
            .publications
            .iter()
            .find(|publication| publication.registry == registry.name)
            .ok_or_else(|| format!("registry {} lost its publication job", registry.name))?;
        write!(text, " packages={}", publication.packages.join(",")).map_err(fail)?;
        writeln!(text, " publication_job={}", publication.job).map_err(fail)?;
    }
    for gate in &pipeline.gates {
        write!(text, "gate={} command={}", gate.job, gate.command).map_err(fail)?;
        if !gate.machines.is_empty() {
            write!(text, " machines={}", gate.machines.join(",")).map_err(fail)?;
        }
        writeln!(text).map_err(fail)?;
    }
    writeln!(
        text,
        "verification_job={} manifest={} workflow_artifact={}",
        pipeline.verification.job,
        pipeline.verification.manifest,
        pipeline.verification.workflow_artifact
    )
    .map_err(fail)?;
    Ok(text)
}

fn read(path: &Path) -> Result<String, String> {
    fs::read_to_string(path).map_err(|error| format!("read {}: {error}", path.display()))
}

const USAGE: &str = "usage: layerx-platform-release [--check|--plan] [manifest-path] [workflow-path]\n       layerx-platform-release manifest --release-dir <dir> --output <file> --rollback-version <version|none> [--registries <path>] [--workflow <path>]\n       layerx-platform-release verify --manifest <file> (--from <dir> | --fetch <dir>) [--source-revision <commit>]";

fn options(arguments: &[String], allowed: &[&str]) -> Result<Vec<(String, String)>, String> {
    let mut values: Vec<(String, String)> = Vec::new();
    let mut remaining = arguments.iter();
    while let Some(key) = remaining.next() {
        if !allowed.contains(&key.as_str()) {
            return Err(format!("unknown option {key}\n{USAGE}"));
        }
        let value = remaining
            .next()
            .ok_or_else(|| format!("option {key} needs a value\n{USAGE}"))?;
        if values.iter().any(|(existing, _)| existing == key) {
            return Err(format!("option {key} given twice\n{USAGE}"));
        }
        values.push((key.clone(), value.clone()));
    }
    Ok(values)
}

fn option<'a>(values: &'a [(String, String)], key: &str) -> Option<&'a str> {
    values
        .iter()
        .find(|(existing, _)| existing == key)
        .map(|(_, value)| value.as_str())
}

fn required_option<'a>(values: &'a [(String, String)], key: &str) -> Result<&'a str, String> {
    option(values, key).ok_or_else(|| format!("option {key} is required\n{USAGE}"))
}

fn run_check(arguments: &[String]) -> Result<(), String> {
    let mode = arguments.first().map_or("--check", String::as_str);
    let manifest_path = arguments
        .get(1)
        .map_or_else(|| PathBuf::from(MANIFEST_PATH), PathBuf::from);
    let workflow_path = arguments
        .get(2)
        .map_or_else(|| PathBuf::from(WORKFLOW_PATH), PathBuf::from);
    if arguments.len() > 3 || !matches!(mode, "--check" | "--plan") {
        return Err(USAGE.to_owned());
    }
    let pipeline = release_pipeline(&read(&manifest_path)?, &read(&workflow_path)?)?;
    if mode == "--plan" {
        print!("{}", plan(&pipeline)?);
    }
    Ok(())
}

fn run_manifest(arguments: &[String]) -> Result<(), String> {
    let values = options(
        arguments,
        &[
            "--release-dir",
            "--output",
            "--rollback-version",
            "--registries",
            "--workflow",
        ],
    )?;
    let release_dir = PathBuf::from(required_option(&values, "--release-dir")?);
    let output = PathBuf::from(required_option(&values, "--output")?);
    let rollback = required_option(&values, "--rollback-version")?;
    let rollback_version = (rollback != "none").then_some(rollback);
    let manifest_path =
        option(&values, "--registries").map_or_else(|| PathBuf::from(MANIFEST_PATH), PathBuf::from);
    let workflow_path =
        option(&values, "--workflow").map_or_else(|| PathBuf::from(WORKFLOW_PATH), PathBuf::from);
    let pipeline = release_pipeline(&read(&manifest_path)?, &read(&workflow_path)?)?;
    let manifest = artifact_manifest(&pipeline, &release_dir, rollback_version)?;
    let rendered = render_artifact_manifest(&manifest)?;
    if let Some(parent) = output
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
    {
        fs::create_dir_all(parent)
            .map_err(|error| format!("create {}: {error}", parent.display()))?;
    }
    fs::write(&output, rendered).map_err(|error| format!("write {}: {error}", output.display()))?;
    println!(
        "artifact manifest: {} artifacts of {} ({}) bound to revision {} written to {}",
        manifest.artifacts.len(),
        manifest.version,
        manifest.tag,
        manifest.source_revision,
        output.display()
    );
    Ok(())
}

fn run_verify(arguments: &[String]) -> Result<(), String> {
    let values = options(
        arguments,
        &["--manifest", "--from", FETCH_FLAG, "--source-revision"],
    )?;
    let manifest_path = PathBuf::from(required_option(&values, "--manifest")?);
    let manifest = parse_artifact_manifest(&read(&manifest_path)?)?;
    let from = option(&values, "--from").map(PathBuf::from);
    let fetch_into = option(&values, FETCH_FLAG).map(PathBuf::from);
    let source = match (&from, &fetch_into) {
        (Some(directory), None) => ArtifactSource::Directory(directory),
        (None, Some(into)) => ArtifactSource::Registries { into },
        _ => {
            return Err(format!(
                "verify needs exactly one of --from or {FETCH_FLAG}\n{USAGE}"
            ))
        }
    };
    let verified =
        release_pipeline_verify(&manifest, source, option(&values, "--source-revision"))?;
    for artifact in &verified {
        println!(
            "verified {}@{} from {} {}",
            artifact.name, artifact.version, artifact.registry, artifact.digest
        );
    }
    println!(
        "release verification: {} artifacts of {} match the manifest bound to revision {}",
        verified.len(),
        manifest.version,
        manifest.source_revision
    );
    Ok(())
}

fn run() -> Result<(), String> {
    let arguments = env::args().skip(1).collect::<Vec<_>>();
    match arguments.first().map(String::as_str) {
        Some("manifest") => run_manifest(&arguments[1..]),
        Some("verify") => run_verify(&arguments[1..]),
        _ => run_check(&arguments),
    }
}

fn main() {
    if let Err(error) = run() {
        eprintln!("platform-release: {error}");
        std::process::exit(1);
    }
}
