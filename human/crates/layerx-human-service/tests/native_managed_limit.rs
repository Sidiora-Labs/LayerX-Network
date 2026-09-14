use std::path::PathBuf;
use std::process::Command;

#[test]
fn actual_native_managed_limit_preserves_session_finality_and_pending_holds() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../..");
    let current = std::env::current_exe()
        .unwrap_or_else(|error| panic!("real managed test executable: {error}"));
    let profile = current
        .parent()
        .and_then(std::path::Path::parent)
        .unwrap_or_else(|| panic!("real test profile missing"));
    let target = profile
        .parent()
        .unwrap_or_else(|| panic!("real target missing"));
    let example = profile.join("examples/native_managed_limit");
    assert!(
        example.is_file(),
        "same-profile native managed example is required"
    );
    let status = Command::new("sh")
        .arg(root.join("human/tools/run-native-managed-limit.sh"))
        .env("CARGO_TARGET_DIR", target)
        .env("LAYERX_TEST_NATIVE_MANAGED_CLIENT", example)
        .current_dir(&root)
        .status()
        .unwrap_or_else(|error| panic!("real managed limit fixture launch: {error}"));
    assert!(
        status.success(),
        "real managed limit fixture failed: {status}"
    );
}
