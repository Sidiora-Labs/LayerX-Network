use std::env;
use std::fs;
use std::path::{Path, PathBuf};

const MANIFEST_PATH: &str = "platform/workspace.kvx";

const REQUIRED_DIRECTORIES: [&str; 9] = [
    "sdk-generators",
    "middleware",
    "cli",
    "emulator",
    "gateway",
    "docs",
    "examples",
    "release",
    "tools",
];

const ECOSYSTEM_KEYS: [&str; 5] = ["manifest", "lockfile", "audit", "lint", "policy"];

/// Validates the platform workspace manifest against the repository tree.
///
/// # Errors
///
/// Fails when a declared directory or ecosystem policy file is missing, when a
/// mandated directory is undeclared, or when the manifest carries unknown
/// declarations.
pub fn workspace_gate(repo_root: &Path, source: &str) -> Result<(), String> {
    let document = layerx_platform_kvx::parse(source)?;
    let name = layerx_platform_kvx::unquote(document.required("workspace", "name")?)?;
    if name.is_empty() {
        return Err("workspace.name must not be empty".to_owned());
    }
    let root = layerx_platform_kvx::unquote(document.required("workspace", "root")?)?;
    if !repo_root.join(&root).is_dir() {
        return Err(format!("workspace root {root} is not a directory"));
    }
    for (key, _) in document.section_entries("workspace") {
        if !matches!(key, "name" | "root") {
            return Err(format!("unknown declaration workspace.{key}"));
        }
    }
    let mut directories = Vec::new();
    let mut ecosystems = Vec::new();
    for section in document.sections() {
        if section == "workspace" {
            continue;
        }
        if let Some(directory) = section.strip_prefix("dir.") {
            directories.push(directory.to_owned());
            for (key, value) in document.section_entries(section) {
                if key != "path" {
                    return Err(format!("unknown declaration {section}.{key}"));
                }
                let path = layerx_platform_kvx::unquote(value)?;
                if !path.starts_with(&format!("{root}/")) && path != root {
                    return Err(format!(
                        "{section}.path {path} escapes the workspace root {root}"
                    ));
                }
                if !repo_root.join(&path).is_dir() {
                    return Err(format!(
                        "declared workspace directory missing: {path} ({section})"
                    ));
                }
            }
            document.required(section, "path")?;
        } else if let Some(ecosystem) = section.strip_prefix("ecosystem.") {
            ecosystems.push(ecosystem.to_owned());
            for (key, value) in document.section_entries(section) {
                if !ECOSYSTEM_KEYS.contains(&key) {
                    return Err(format!("unknown declaration {section}.{key}"));
                }
                let path = layerx_platform_kvx::unquote(value)?;
                if !repo_root.join(&path).is_file() {
                    return Err(format!(
                        "declared {ecosystem} policy file missing: {path} ({section}.{key})"
                    ));
                }
            }
            for key in ECOSYSTEM_KEYS {
                document.required(section, key)?;
            }
        } else {
            return Err(format!("unknown section {section}"));
        }
    }
    for required in REQUIRED_DIRECTORIES {
        if !directories.iter().any(|declared| declared == required) {
            return Err(format!(
                "mandated workspace directory undeclared: {required}"
            ));
        }
    }
    if !ecosystems.iter().any(|declared| declared == "rust") {
        return Err("the rust ecosystem must be declared".to_owned());
    }
    Ok(())
}

fn run() -> Result<(), String> {
    let arguments = env::args().skip(1).collect::<Vec<_>>();
    let manifest_path = arguments
        .first()
        .map_or_else(|| PathBuf::from(MANIFEST_PATH), PathBuf::from);
    let repo_root = arguments
        .get(1)
        .map_or_else(|| PathBuf::from("."), PathBuf::from);
    let source = fs::read_to_string(&manifest_path)
        .map_err(|error| format!("read {}: {error}", manifest_path.display()))?;
    workspace_gate(&repo_root, &source)
}

fn main() {
    if let Err(error) = run() {
        eprintln!("platform-workspace-gate: {error}");
        std::process::exit(1);
    }
}
