pub mod workflow;

use std::env;
use std::fmt::Write as _;
use std::fs;
use std::path::{Path, PathBuf};

use workflow::Node;

const MANIFEST_PATH: &str = "platform/release/registries.kvx";
const WORKFLOW_PATH: &str = ".github/workflows/platform.yml";
const RELEASE_JOB: &str = "release-pipeline";
const PROMOTION_JOB: &str = "release-promotion";
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
pub struct ReleasePipeline {
    pub declarations: RegistriesDeclarations,
    pub gates: Vec<GateJob>,
    pub publications: Vec<PublicationJob>,
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
    promotion_job(jobs, &publications)?;
    Ok(ReleasePipeline {
        declarations,
        gates,
        publications,
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
    Ok(())
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
    Ok(text)
}

fn read(path: &Path) -> Result<String, String> {
    fs::read_to_string(path).map_err(|error| format!("read {}: {error}", path.display()))
}

fn run() -> Result<(), String> {
    let arguments = env::args().skip(1).collect::<Vec<_>>();
    let mode = arguments.first().map_or("--check", String::as_str);
    let manifest_path = arguments
        .get(1)
        .map_or_else(|| PathBuf::from(MANIFEST_PATH), PathBuf::from);
    let workflow_path = arguments
        .get(2)
        .map_or_else(|| PathBuf::from(WORKFLOW_PATH), PathBuf::from);
    if arguments.len() > 3 || !matches!(mode, "--check" | "--plan") {
        return Err(
            "usage: layerx-platform-release [--check|--plan] [manifest-path] [workflow-path]"
                .to_owned(),
        );
    }
    let pipeline = release_pipeline(&read(&manifest_path)?, &read(&workflow_path)?)?;
    if mode == "--plan" {
        print!("{}", plan(&pipeline)?);
    }
    Ok(())
}

fn main() {
    if let Err(error) = run() {
        eprintln!("platform-release: {error}");
        std::process::exit(1);
    }
}
