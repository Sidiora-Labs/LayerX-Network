use std::path::Path;
use std::process::Command;

#[test]
fn real_disposable_custody_over_verified_tls() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"))
        .ancestors()
        .nth(3)
        .unwrap_or_else(|| panic!("repository root"));
    let status = Command::new("python3")
        .arg(root.join("human/crates/layerx-paxeer-client/tests/disposable_custody.py"))
        .status()
        .unwrap_or_else(|error| panic!("disposable custody test: {error}"));
    assert!(status.success(), "disposable custody test failed: {status}");
}
