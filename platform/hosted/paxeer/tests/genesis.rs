use std::process::Command;

#[test]
fn genesis_through_real_tls_boundary() {
    let status = Command::new("python3")
        .arg(concat!(env!("CARGO_MANIFEST_DIR"), "/tests/genesis.py"))
        .arg(env!("CARGO_BIN_EXE_layerx-paxeer-boundary"))
        .status()
        .unwrap_or_else(|error| panic!("launch genesis qualification: {error}"));
    assert!(status.success(), "genesis qualification failed: {status}");
}
