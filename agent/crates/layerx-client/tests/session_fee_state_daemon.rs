use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::Duration;

use layerx_client::client::{ClientConfig, ReconnectPolicy};
use layerx_client::lni::handshake::HandshakeConfig;
use layerx_client::lni::schema::{Capability, Version};
use layerx_client::lni::transport::Limits;
use layerx_client::read::ReadError;
use layerx_client::Client;

fn checked<T, E: std::fmt::Debug>(value: Result<T, E>) -> T {
    value.unwrap_or_else(|error| panic!("real session-state boundary: {error:?}"))
}

fn bytes(value: &str) -> Vec<u8> {
    assert!(value.is_ascii() && value.len().is_multiple_of(2));
    value
        .as_bytes()
        .chunks_exact(2)
        .map(|pair| checked(u8::from_str_radix(checked(std::str::from_utf8(pair)), 16)))
        .collect()
}

fn probe(socket: &Path) {
    let id: [u8; 32] =
        checked(bytes(&checked(std::env::var("LAYERX_TEST_SESSION_FEE_GRANT"))).try_into());
    let expected = bytes(&checked(std::env::var("LAYERX_TEST_SESSION_FEE_EXPECTED")));
    assert!(expected.len() >= 188);
    let mut client = checked(Client::connect(ClientConfig {
        endpoint: socket.to_path_buf(),
        handshake: HandshakeConfig {
            built_interface_version: Version::V1_5,
            expected_protocol_version: 3,
            expected_network_id: 77,
        },
        limits: Limits {
            maximum_frame_bytes: 1_212_416,
            maximum_connections: 1,
            maximum_streams: 1,
            maximum_queued_bytes: 1_212_416,
            deadline: Duration::from_secs(10),
        },
        reconnect: ReconnectPolicy {
            maximum_attempts: 1,
            base_delay: Duration::ZERO,
            maximum_delay: Duration::ZERO,
            jitter_percent: 0,
        },
    }));
    assert!(client
        .handshake()
        .capabilities()
        .contains(Capability::SessionFeeState));
    let state = checked(client.session_fee_state(100, id));
    assert_eq!(
        state.observed_sequence,
        u64::from_be_bytes(checked(expected[2..10].try_into()))
    );
    assert_eq!(state.state_root.as_slice(), &expected[10..42]);
    assert_eq!(state.value.as_slice(), &expected[42..]);
    assert_eq!(
        client.session_fee_state(101, [0; 32]),
        Err(ReadError::SelectorMismatch)
    );
    println!(
        "actual Rust Client session fee state matches committed native grant and fee counters"
    );
}

#[test]
fn real_daemon_session_fee_state() {
    if let Some(socket) = std::env::var_os("LAYERX_TEST_SESSION_FEE_SOCKET") {
        probe(Path::new(&socket));
        return;
    }
    let repository = checked(
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../..")
            .canonicalize(),
    );
    let build = std::env::var_os("LAYERX_TEST_NATIVE_BUILD_DIR")
        .map_or_else(|| repository.join("build"), PathBuf::from);
    for artifact in [
        "bin/layerxd",
        "bin/layerx-genesis-build",
        "tests/lxp_test_metered_allowance",
        "tests/bridge/sign-credit",
    ] {
        assert!(
            build.join(artifact).is_file(),
            "missing actual native prerequisite: {artifact}"
        );
    }
    let evidence =
        std::env::temp_dir().join(format!("lxp-session-fee-evidence-{}", std::process::id()));
    checked(fs::create_dir(&evidence));
    let output = checked(
        Command::new(std::env::var_os("LAYERX_TEST_PYTHON").unwrap_or_else(|| "python3".into()))
            .arg(repository.join("tests/daemon/withdraw-custody.py"))
            .arg(&build)
            .arg("--metered-allowance")
            .env(
                "LAYERX_TEST_SESSION_FEE_CLIENT",
                checked(std::env::current_exe()),
            )
            .env("LAYERX_TEST_ADMISSION_LOG_DIR", &evidence)
            .env("CARGO_BUILD_JOBS", "4")
            .env("MAKEFLAGS", "-j4")
            .current_dir(&repository)
            .output(),
    );
    checked(fs::write(evidence.join("client.stdout"), &output.stdout));
    checked(fs::write(evidence.join("client.stderr"), &output.stderr));
    assert!(
        output.status.success(),
        "actual native session gate failed; evidence {}",
        evidence.display()
    );
    let stdout = checked(std::str::from_utf8(&output.stdout));
    assert!(stdout.matches("actual Rust Client session fee state matches committed native grant and fee counters").count() >= 6,
        "native success, charge, revocation, replacement and restart probes missing; evidence {}", evidence.display());
    println!(
        "actual native and Rust session fee qualification evidence: {}",
        evidence.display()
    );
}
