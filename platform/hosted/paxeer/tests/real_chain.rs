use layerx_paxeer_client::{raw_call, EndpointConfig, EndpointTransport};
use std::process::Command;
use std::time::Duration;

#[test]
fn real_paxd_boundary() {
    if let Ok(url) = std::env::var("LAYERX_PAXEER_TEST_CLIENT_URL") {
        let ca = std::env::var("LAYERX_PAXEER_TEST_CLIENT_CA")
            .unwrap_or_else(|error| panic!("test CA: {error}"));
        let endpoint = EndpointConfig {
            url,
            expected_chain_id: 125,
            request_timeout: Duration::from_secs(5),
            transport: EndpointTransport::PinnedTls {
                trust_anchor_der: std::fs::read(ca)
                    .unwrap_or_else(|error| panic!("read test CA: {error}")),
            },
        };
        let result = raw_call(&endpoint, "eth_chainId", &[])
            .unwrap_or_else(|error| panic!("real Paxeer client: {error:?}"));
        assert_eq!(result.as_text(), Some("0x7d"));
        return;
    }
    let status = Command::new("python3")
        .arg(concat!(env!("CARGO_MANIFEST_DIR"), "/tests/real_chain.py"))
        .arg(env!("CARGO_BIN_EXE_layerx-paxeer-boundary"))
        .arg(std::env::current_exe().unwrap_or_else(|error| panic!("test executable: {error}")))
        .status()
        .unwrap_or_else(|error| panic!("launch real Paxeer qualification: {error}"));
    assert!(
        status.success(),
        "real Paxeer qualification failed: {status}"
    );
}

#[test]
fn settlement_domains() {
    let status = Command::new("python3")
        .arg(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/tests/settlement_domains.py"
        ))
        .status()
        .unwrap_or_else(|error| panic!("launch domain qualification: {error}"));
    assert!(status.success(), "settlement domain qualification failed");
}
