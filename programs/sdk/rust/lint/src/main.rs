//! Build-time determinism gate for `LayerX` program projects.
//!
//! The lint is a build step, not a report: it exits non-zero and names the
//! violated rule, so a program that reaches for a clock, for randomness, for
//! floating point or for a host interface outside its explicitly recorded
//! frozen ABI surface fails its build before deployment ever sees it.

use std::env;
use std::fs;
use std::path::Path;
use std::process::ExitCode;

use layerx_program_lint::{
    abi_surface_violations, lint_artifact_for_abi, lint_project_for_abi,
    DeterminismViolation,
};

const USAGE: &str = "usage: layerx-program-lint <project-directory> [artifact.wasm]\n       layerx-program-lint --abi-version <1|2> <project-directory> [artifact.wasm]\n       layerx-program-lint --abi-version <1|2> --artifact <artifact.wasm>\n       layerx-program-lint --abi";
const USAGE_STATUS: u8 = 2;

fn main() -> ExitCode {
    let arguments: Vec<String> = env::args().skip(1).collect();
    let requested: Vec<&str> = arguments.iter().map(String::as_str).collect();
    let violations = match requested.as_slice() {
        ["--abi"] => abi_surface_violations(),
        ["--abi-version", version, "--artifact", artifact] => {
            let Some(version) = parse_version(version) else {
                return usage_refusal("ABI version must be exactly 1 or 2");
            };
            artifact_violations(Path::new(artifact), version)
        }
        ["--artifact", _] => {
            return usage_refusal("artifact lint requires --abi-version <1|2>");
        }
        ["--abi-version", version, project] => {
            project_violations(Path::new(project), None, Some(version))
        }
        ["--abi-version", version, project, artifact] => {
            project_violations(Path::new(project), Some(Path::new(artifact)), Some(version))
        }
        [project] => project_violations(Path::new(project), None, None),
        [project, artifact] => {
            project_violations(Path::new(project), Some(Path::new(artifact)), None)
        }
        _ => {
            eprintln!("{USAGE}");
            return ExitCode::from(USAGE_STATUS);
        }
    };
    if violations.is_empty() {
        return ExitCode::SUCCESS;
    }
    for violation in &violations {
        eprintln!("determinism violation [{}]: {violation}", violation.name());
    }
    ExitCode::FAILURE
}

fn artifact_violations(artifact: &Path, abi_version: u16) -> Vec<DeterminismViolation> {
    match fs::read(artifact) {
        Ok(wasm) => lint_artifact_for_abi(&wasm, abi_version),
        Err(error) => vec![DeterminismViolation::UnreadablePath {
            path: artifact.to_path_buf(),
            reason: error.to_string(),
        }],
    }
}

fn project_violations(
    project: &Path,
    artifact: Option<&Path>,
    requested_version: Option<&str>,
) -> Vec<DeterminismViolation> {
    let recorded = match recorded_abi_version(project) {
        Ok(version) => version,
        Err(reason) => return vec![metadata_refusal(project, reason)],
    };
    if let Some(requested) = requested_version {
        let Some(requested) = parse_version(requested) else {
            return vec![metadata_refusal(project, "ABI version must be exactly 1 or 2")];
        };
        if requested != recorded {
            return vec![metadata_refusal(
                project,
                "requested ABI version differs from layerx-program.json",
            )];
        }
    }
    lint_project_for_abi(project, artifact, recorded)
}

fn recorded_abi_version(project: &Path) -> Result<u16, &'static str> {
    let manifest = fs::read_to_string(project.join("layerx-program.json"))
        .map_err(|_| "missing or unreadable layerx-program.json")?;
    let mut recorded = None;
    for (offset, _) in manifest.match_indices("\"abi_version\"") {
        let value = manifest[offset + "\"abi_version\"".len()..]
            .trim_start()
            .strip_prefix(':')
            .ok_or("malformed abi_version field")?
            .trim_start();
        let length = value.bytes().take_while(|byte| byte.is_ascii_digit()).count();
        let (value, remainder) = value.split_at(length);
        if value.is_empty()
            || !matches!(remainder.trim_start().bytes().next(), Some(b',') | Some(b'}'))
        {
            return Err("malformed abi_version field");
        }
        let version = value
            .parse::<u16>()
            .ok()
            .and_then(|version| parse_version_number(version))
            .ok_or("ABI version must be exactly 1 or 2")?;
        if recorded.replace(version).is_some() {
            return Err("duplicate abi_version fields");
        }
    }
    recorded.ok_or("missing abi_version field")
}

fn parse_version(value: &str) -> Option<u16> {
    value.parse::<u16>().ok().and_then(parse_version_number)
}

const fn parse_version_number(value: u16) -> Option<u16> {
    match value {
        layerx_programs_runtime::ABI_V1_VERSION
        | layerx_programs_runtime::ABI_V2_VERSION => Some(value),
        _ => None,
    }
}

fn metadata_refusal(project: &Path, reason: &'static str) -> DeterminismViolation {
    DeterminismViolation::UnreadablePath {
        path: project.join("layerx-program.json"),
        reason: reason.to_string(),
    }
}

fn usage_refusal(reason: &str) -> ExitCode {
    eprintln!("{reason}");
    eprintln!("{USAGE}");
    ExitCode::from(USAGE_STATUS)
}

#[cfg(test)]
mod tests {
    use super::{parse_version, recorded_abi_version};
    use std::fs;

    #[test]
    fn project_metadata_requires_one_supported_abi_version() {
        let root = std::env::temp_dir().join(format!(
            "layerx-lint-abi-{}",
            std::process::id()
        ));
        fs::create_dir_all(&root).unwrap_or_else(|error| panic!("fixture directory: {error}"));
        fs::write(root.join("layerx-program.json"), "{\n  \"abi_version\": 2\n}\n")
            .unwrap_or_else(|error| panic!("fixture manifest: {error}"));
        assert_eq!(recorded_abi_version(&root), Ok(2));
        fs::write(root.join("layerx-program.json"), "{\"abi_version\":1}\n")
            .unwrap_or_else(|error| panic!("one-line fixture manifest: {error}"));
        assert_eq!(recorded_abi_version(&root), Ok(1));
        fs::write(
            root.join("layerx-program.json"),
            "{\"abi_version\":1,\"abi_version\":2}\n",
        )
        .unwrap_or_else(|error| panic!("duplicate fixture manifest: {error}"));
        assert!(recorded_abi_version(&root).is_err());
        fs::write(root.join("layerx-program.json"), "{}\n")
            .unwrap_or_else(|error| panic!("fixture manifest: {error}"));
        assert!(recorded_abi_version(&root).is_err());
        fs::remove_dir_all(&root).unwrap_or_else(|error| panic!("fixture cleanup: {error}"));
        assert_eq!(parse_version("1"), Some(1));
        assert_eq!(parse_version("2"), Some(2));
        assert_eq!(parse_version("3"), None);
    }
}
