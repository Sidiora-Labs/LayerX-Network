use std::path::Path;
use std::process::Command;

pub fn run(scenario: &str) {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../..");
    let current =
        std::env::current_exe().unwrap_or_else(|error| panic!("test executable: {error}"));
    let example = current
        .parent()
        .and_then(Path::parent)
        .unwrap_or_else(|| panic!("test profile directory missing"))
        .join("examples/native_budget_recovery");
    assert!(
        example.is_file(),
        "actual native Budget example from the same test profile is required: {}",
        example.display()
    );
    let output = Command::new("sh")
        .arg(root.join("agent/tools/run-native-budget-tests.sh"))
        .arg(scenario)
        .env("LAYERX_TEST_NATIVE_BUDGET_CLIENT", example)
        .current_dir(&root)
        .output()
        .unwrap_or_else(|error| panic!("actual native Budget fixture launch: {error}"));
    assert!(
        output.status.success(),
        "actual native Budget {scenario} failed: {}\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}
