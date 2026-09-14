use std::path::Path;
use std::process::Command;

pub fn run(scenario:&str) {
    let root=Path::new(env!("CARGO_MANIFEST_DIR")).join("../../..");
    let output=Command::new("sh").arg(root.join("agent/tools/run-native-budget-tests.sh"))
        .arg(scenario).current_dir(&root).output()
        .unwrap_or_else(|error|panic!("actual native Budget fixture launch: {error}"));
    assert!(output.status.success(),"actual native Budget {scenario} failed: {}\n{}",
        String::from_utf8_lossy(&output.stdout),String::from_utf8_lossy(&output.stderr));
}
