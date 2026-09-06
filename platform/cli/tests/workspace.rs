use std::path::{Path, PathBuf};
use std::process::{Command, Output};

use serde_json::Value;

fn run(arguments: &[&str]) -> Output {
    let config = std::env::temp_dir().join(format!(
        "layerx-workspace-test-{}-config.json",
        std::process::id()
    ));
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let result = Command::new(env!("CARGO_BIN_EXE_layerx"))
        .args(arguments)
        .env("LAYERX_CONFIG", config)
        .env("LAYERX_REPO_ROOT", root)
        .output();
    match result {
        Ok(output) => output,
        Err(error) => panic!("workspace CLI should start: {error}"),
    }
}

fn json_output(output: &Output) -> Value {
    match serde_json::from_slice(&output.stdout) {
        Ok(value) => value,
        Err(error) => panic!(
            "workspace CLI should emit JSON: {error}; stdout={}",
            String::from_utf8_lossy(&output.stdout)
        ),
    }
}

fn data_array<'a>(value: &'a Value, field: &str) -> &'a Vec<Value> {
    match value
        .get("data")
        .and_then(|data| data.get(field))
        .and_then(Value::as_array)
    {
        Some(array) => array,
        None => panic!("response data should contain array {field}"),
    }
}

#[test]
fn module_inventory_covers_every_top_level_surface() {
    let output = run(&["--json", "workspace", "modules"]);
    assert!(output.status.success());
    let value = json_output(&output);
    let Some(modules) = value.get("data").and_then(Value::as_array) else { panic!("module response should contain an array"); };
    let ids = modules
        .iter()
        .filter_map(|module| module.get("id").and_then(Value::as_str))
        .collect::<Vec<_>>();
    assert_eq!(
        ids,
        [
            "core",
            "contracts",
            "agent",
            "human",
            "platform",
            "programs",
            "interop",
            "specgen"
        ]
    );
}

#[test]
fn doctor_is_read_only_and_machine_readable() {
    let output = run(&[
        "--json",
        "workspace",
        "doctor",
        "--module",
        "human,programs",
    ]);
    assert!(output.status.success());
    let value = json_output(&output);
    assert_eq!(
        value.get("kind").and_then(Value::as_str),
        Some("workspace.doctor")
    );
    assert!(!data_array(&value, "tools").is_empty());
}

#[test]
fn all_dry_run_plans_without_executing_dependencies_or_builds() {
    let output = run(&["--json", "workspace", "all", "--all", "--dry-run"]);
    assert!(output.status.success());
    let value = json_output(&output);
    let steps = data_array(&value, "steps");
    assert!(steps.len() > 20);
    assert!(steps
        .iter()
        .all(|step| { step.get("status").and_then(Value::as_str) == Some("planned") }));
}

#[test]
fn executing_without_an_explicit_selection_is_rejected() {
    let output = run(&["--json", "workspace", "build", "--dry-run"]);
    assert!(!output.status.success());
    let error: Result<Value, _> = serde_json::from_slice(&output.stderr);
    match error {
        Ok(value) => assert_eq!(
            value
                .get("error")
                .and_then(|error| error.get("code"))
                .and_then(Value::as_str),
            Some("command_failed")
        ),
        Err(parse_error) => panic!("error should be JSON: {parse_error}"),
    }
}

#[test]
fn repo_root_override_must_point_at_layerx() {
    let invalid_root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("src");
    let config = std::env::temp_dir().join(format!(
        "layerx-workspace-invalid-root-{}.json",
        std::process::id()
    ));
    let output = Command::new(env!("CARGO_BIN_EXE_layerx"))
        .args(["--json", "workspace", "build", "--all", "--dry-run"])
        .env("LAYERX_CONFIG", config)
        .env("LAYERX_REPO_ROOT", invalid_root)
        .output();
    match output {
        Ok(output) => assert!(!output.status.success()),
        Err(error) => panic!("workspace CLI should start: {error}"),
    }
}
