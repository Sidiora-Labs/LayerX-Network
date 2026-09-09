use std::path::Path;
use std::process::Command;

#[test]
fn generated_catalog_loads_through_the_service_reader() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../..");
    let directory =
        tempfile::tempdir().unwrap_or_else(|error| panic!("temporary directory: {error:?}"));
    let script = "import sys; sys.path.insert(0, sys.argv[1]); import test_provision; test_provision.generated_catalog(sys.argv[2], sys.argv[3])";
    let output = Command::new("python3")
        .args(["-c", script])
        .arg(root.join("platform/hosted/human"))
        .arg(directory.path())
        .arg(env!("CARGO_BIN_EXE_layerx-human-identity-provider"))
        .output()
        .unwrap_or_else(|error| panic!("catalog generator: {error:?}"));
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let bytes = std::fs::read(directory.path().join("purpose-catalog.json"))
        .unwrap_or_else(|error| panic!("generated catalog: {error:?}"));
    let catalog = layerx_human_service::agents::PurposePresetCatalog::from_json(&bytes)
        .unwrap_or_else(|error| panic!("catalog load: {error:?}"));
    assert_eq!(catalog.version(), "layerx-beta-v1");
}
