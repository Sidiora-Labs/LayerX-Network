use std::env;
use std::fmt::Write as _;
use std::fs;
use std::path::PathBuf;

const MANIFEST_PATH: &str = "platform/release/registries.kvx";

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

const NPM_MIDDLEWARE_PACKAGES: [&str; 4] = [
    "@sidiora/layerx-agent-middleware",
    "@sidiora/layerx-buyer-middleware",
    "@sidiora/layerx-merchant-middleware",
    "@sidiora/layerx-seller-middleware",
];

const STATUSES: [&str; 2] = ["skeleton", "active"];

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Registry {
    pub name: String,
    pub declarations: Vec<(String, String)>,
    pub packages: Vec<String>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ReleasePipeline {
    pub tag_format: String,
    pub source_digest: String,
    pub registries: Vec<Registry>,
}

/// Parses and validates the release registry manifest.
///
/// # Errors
///
/// Fails unless every mandated registry is declared exactly once with every
/// required declaration and a known status.
pub fn release_pipeline(source: &str) -> Result<ReleasePipeline, String> {
    let document = layerx_platform_kvx::parse(source)?;
    let declared =
        layerx_platform_kvx::string_list(document.required("release", "registries")?)?;
    if declared != REGISTRIES {
        return Err(format!(
            "release.registries must list exactly the seven mandated registries {REGISTRIES:?}, got {declared:?}"
        ));
    }
    for (key, _) in document.section_entries("release") {
        if !matches!(key, "registries" | "tag_format" | "source_digest") {
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
    let mut registries = Vec::new();
    for name in REGISTRIES {
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
            if !REGISTRY_KEYS.contains(key) {
                return Err(format!("unknown declaration {section}.{key}"));
            }
            let value = layerx_platform_kvx::unquote(value)?;
            if value.is_empty() {
                return Err(format!("empty declaration {section}.{key}"));
            }
            declarations.push(((*key).to_owned(), value));
        }
        for key in REGISTRY_KEYS {
            if !declarations.iter().any(|(declared, _)| declared == key) {
                return Err(format!("missing declaration {section}.{key}"));
            }
        }
        let status = document.required(&section, "status")?;
        let status = layerx_platform_kvx::unquote(status)?;
        if !STATUSES.contains(&status.as_str()) {
            return Err(format!(
                "registry {name} has unknown status {status}; expected one of {STATUSES:?}"
            ));
        }
        if name == "npm" && packages != NPM_MIDDLEWARE_PACKAGES {
            return Err(format!(
                "registry.npm.packages must list the four published middleware packages {NPM_MIDDLEWARE_PACKAGES:?}, got {packages:?}"
            ));
        }
        if name != "npm" && !packages.is_empty() {
            return Err(format!("registry {name} does not accept a packages declaration"));
        }
        registries.push(Registry {
            name: name.to_owned(),
            declarations,
            packages,
        });
    }
    Ok(ReleasePipeline {
        tag_format,
        source_digest,
        registries,
    })
}

/// Renders the machine-readable release plan.
///
/// # Errors
///
/// Fails only when formatting the plan fails.
pub fn plan(pipeline: &ReleasePipeline) -> Result<String, String> {
    let mut text = String::new();
    let fail = |error| format!("render plan: {error}");
    writeln!(text, "tag_format={}", pipeline.tag_format).map_err(fail)?;
    writeln!(text, "source_digest={}", pipeline.source_digest).map_err(fail)?;
    for registry in &pipeline.registries {
        write!(text, "registry={}", registry.name).map_err(fail)?;
        for key in REGISTRY_KEYS {
            let value = registry
                .declarations
                .iter()
                .find(|(declared, _)| declared == key)
                .map(|(_, value)| value.as_str())
                .ok_or_else(|| format!("registry {} lost {key}", registry.name))?;
            write!(text, " {key}={value}").map_err(fail)?;
        }
        if !registry.packages.is_empty() {
            write!(text, " packages={}", registry.packages.join(",")).map_err(fail)?;
        }
        writeln!(text).map_err(fail)?;
    }
    Ok(text)
}

fn run() -> Result<(), String> {
    let arguments = env::args().skip(1).collect::<Vec<_>>();
    let mode = arguments.first().map_or("--check", String::as_str);
    let manifest_path = arguments
        .get(1)
        .map_or_else(|| PathBuf::from(MANIFEST_PATH), PathBuf::from);
    let source = fs::read_to_string(&manifest_path)
        .map_err(|error| format!("read {}: {error}", manifest_path.display()))?;
    let pipeline = release_pipeline(&source)?;
    match mode {
        "--check" => Ok(()),
        "--plan" => {
            print!("{}", plan(&pipeline)?);
            Ok(())
        }
        _ => Err("usage: layerx-platform-release [--check|--plan] [manifest-path]".to_owned()),
    }
}

fn main() {
    if let Err(error) = run() {
        eprintln!("platform-release: {error}");
        std::process::exit(1);
    }
}
