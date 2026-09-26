use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use layerx_program_lint::{lint_artifact_for_abi, lint_project_for_abi, DeterminismViolation};
use layerx_programs_runtime::test_support::{
    code_section, func_body, function_section, import_section, module, raw_section, type_section,
    unsigned_leb, OP_CALL, OP_END, OP_LOCAL_GET, TYPE_I32,
};
use layerx_programs_runtime::{
    ABI_V1_MODULE, ABI_V1_VERSION, ABI_V2_VERSION, ABI_V3_MODULE, ABI_V3_VERSION, ABI_V4_MODULE,
    ABI_V4_VERSION,
};

const SECTION_MEMORY: u8 = 5;
const SECTION_EXPORT: u8 = 7;
const KIND_FUNC: u8 = 0x00;
const KIND_MEMORY: u8 = 0x02;
const WEB_READER_ARTIFACT: &str = "wasm32-unknown-unknown/release/layerx_reference_web_reader.wasm";

fn exports(entries: &[(&str, u8, u32)]) -> Vec<u8> {
    let mut payload = unsigned_leb(entries.len() as u64);
    for (name, kind, index) in entries {
        payload.extend(unsigned_leb(name.len() as u64));
        payload.extend_from_slice(name.as_bytes());
        payload.push(*kind);
        payload.extend(unsigned_leb(u64::from(*index)));
    }
    raw_section(SECTION_EXPORT, &payload)
}

/// A complete program module whose `layerx_call` forwards to one imported
/// four-argument host function, with the reservation and memory exports the
/// lint requires.
fn program_importing(import_module: &str, import_name: &str) -> Vec<u8> {
    let call = [
        OP_LOCAL_GET,
        0,
        OP_LOCAL_GET,
        1,
        OP_LOCAL_GET,
        0,
        OP_LOCAL_GET,
        1,
        OP_CALL,
        0,
        OP_END,
    ];
    let reserve = [OP_LOCAL_GET, 0, OP_END];
    module(&[
        type_section(&[
            (&[TYPE_I32; 4], &[TYPE_I32]),
            (&[TYPE_I32; 2], &[TYPE_I32]),
            (&[TYPE_I32], &[TYPE_I32]),
        ]),
        import_section(&[(import_module, import_name, 0)]),
        function_section(&[1, 2]),
        raw_section(SECTION_MEMORY, &[1, 1, 1, 1]),
        exports(&[
            ("layerx_call", KIND_FUNC, 1),
            ("layerx_reserve", KIND_FUNC, 2),
            ("memory", KIND_MEMORY, 0),
        ]),
        code_section(&[func_body(&[], &call), func_body(&[], &reserve)]),
    ])
}

fn undeclared(import_module: &str, import_name: &str) -> DeterminismViolation {
    DeterminismViolation::UndeclaredHostImport {
        import_module: import_module.to_string(),
        import_name: import_name.to_string(),
    }
}

fn web_reader_project() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../examples/web-reader")
}

/// Builds the web-reader reference program exactly as its build script does
/// and returns the release artifact it produced.
fn web_reader_artifact(project: &Path) -> PathBuf {
    let target_dir = project.join("target");
    let status = Command::new(env!("CARGO"))
        .current_dir(project)
        .args([
            "build",
            "--locked",
            "--release",
            "--target",
            "wasm32-unknown-unknown",
            "--target-dir",
        ])
        .arg(&target_dir)
        .status()
        .unwrap_or_else(|error| panic!("web-reader build did not start: {error}"));
    assert!(status.success(), "web-reader build failed: {status}");
    target_dir.join(WEB_READER_ARTIFACT)
}

#[test]
fn web_reader_artifact_passes_at_v4_and_is_refused_below_it() {
    let project = web_reader_project();
    let artifact = web_reader_artifact(&project);
    let wasm = fs::read(&artifact)
        .unwrap_or_else(|error| panic!("web-reader artifact {}: {error}", artifact.display()));

    assert_eq!(lint_artifact_for_abi(&wasm, ABI_V4_VERSION), Vec::new());
    assert_eq!(
        lint_project_for_abi(&project, Some(&artifact), ABI_V4_VERSION),
        Vec::new()
    );
    for version in [ABI_V1_VERSION, ABI_V2_VERSION, ABI_V3_VERSION] {
        let violations = lint_artifact_for_abi(&wasm, version);
        assert!(
            violations.contains(&undeclared(ABI_V4_MODULE, "web_read")),
            "v{version} admitted web_read: {violations:?}"
        );
        assert!(lint_project_for_abi(&project, Some(&artifact), version)
            .contains(&undeclared(ABI_V4_MODULE, "web_read")));
    }
}

#[test]
fn v3_admits_oracle_read_and_refuses_the_v4_namespace() {
    let oracle = program_importing(ABI_V3_MODULE, "oracle_read");
    assert_eq!(lint_artifact_for_abi(&oracle, ABI_V3_VERSION), Vec::new());
    assert_eq!(lint_artifact_for_abi(&oracle, ABI_V4_VERSION), Vec::new());
    assert!(lint_artifact_for_abi(&oracle, ABI_V2_VERSION)
        .contains(&undeclared(ABI_V3_MODULE, "oracle_read")));

    let web = program_importing(ABI_V4_MODULE, "web_read");
    assert_eq!(lint_artifact_for_abi(&web, ABI_V4_VERSION), Vec::new());
    assert!(lint_artifact_for_abi(&web, ABI_V3_VERSION)
        .contains(&undeclared(ABI_V4_MODULE, "web_read")));

    let misplaced = program_importing(ABI_V3_MODULE, "web_read");
    assert!(lint_artifact_for_abi(&misplaced, ABI_V3_VERSION)
        .contains(&undeclared(ABI_V3_MODULE, "web_read")));
}

#[test]
fn v4_artifact_importing_outside_the_host_set_is_refused() {
    for (import_module, import_name) in [
        (ABI_V4_MODULE, "web_fetch"),
        (ABI_V4_MODULE, "oracle_read"),
        (ABI_V1_MODULE, "web_read"),
        ("layerx_v5", "web_read"),
    ] {
        let violations = lint_artifact_for_abi(
            &program_importing(import_module, import_name),
            ABI_V4_VERSION,
        );
        let refusal = undeclared(import_module, import_name);
        assert!(
            violations.contains(&refusal),
            "{import_module}::{import_name} was admitted: {violations:?}"
        );
        assert_eq!(refusal.name(), "undeclared-host-import");
    }

    let clock = lint_artifact_for_abi(&program_importing(ABI_V4_MODULE, "now"), ABI_V4_VERSION);
    assert!(clock.contains(&DeterminismViolation::ClockImport {
        import_module: ABI_V4_MODULE.to_string(),
        import_name: "now".to_string(),
    }));
}

#[test]
fn versions_above_v4_keep_the_unsupported_refusal() {
    let web = program_importing(ABI_V4_MODULE, "web_read");
    for version in [ABI_V4_VERSION + 1, u16::MAX, 0] {
        assert_eq!(
            lint_artifact_for_abi(&web, version),
            vec![DeterminismViolation::RejectedByEngine {
                reason: format!("unsupported LayerX ABI version {version}"),
            }]
        );
    }
}
