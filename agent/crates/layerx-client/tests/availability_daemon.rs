use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::Duration;

use layerx_client::batch::lookup;
use layerx_client::lni::handshake::{perform, HandshakeConfig};
use layerx_client::lni::refusal::decode_core_refusal;
use layerx_client::lni::schema::{decode_envelope, encode_envelope, Envelope, Version};
use layerx_client::lni::transport::{ConnectionGate, FrameTransport, Limits, Uds};

fn refusal(transport: &mut Uds, payload: &[u8], expected: i32, correlation: u64) {
    let encoded = encode_envelope(Envelope {
        version: Version::V1_4,
        message_tag: 18,
        correlation_id: correlation,
        canonical_payload: payload,
        proof_material: &[],
    })
    .unwrap_or_else(|error| panic!("availability request encoding: {error:?}"));
    transport
        .send(&encoded)
        .unwrap_or_else(|error| panic!("availability request send: {error:?}"));
    let bytes = transport
        .receive()
        .unwrap_or_else(|error| panic!("availability refusal receive: {error:?}"));
    let response = decode_envelope(&bytes)
        .unwrap_or_else(|error| panic!("availability refusal envelope: {error:?}"));
    assert_eq!(response.message_tag, 25);
    assert_eq!(response.correlation_id, correlation);
    assert!(response.proof_material.is_empty());
    let result = decode_core_refusal(response.canonical_payload)
        .unwrap_or_else(|| panic!("malformed core refusal"));
    assert_eq!(result.class, 3);
    assert_eq!(result.result.raw(), expected);
}

fn probe(socket: &Path, corrupt: bool) {
    let gate = ConnectionGate::new(1);
    let mut transport = Uds::connect(
        socket,
        &gate,
        Limits {
            maximum_frame_bytes: 1_212_416,
            maximum_connections: 1,
            maximum_streams: 1,
            maximum_queued_bytes: 1_212_416,
            deadline: Duration::from_secs(10),
        },
    )
    .unwrap_or_else(|error| panic!("real daemon connection: {error:?}"));
    let handshake = perform(
        &mut transport,
        &HandshakeConfig {
            built_interface_version: Version::V1_4,
            expected_protocol_version: 3,
            expected_network_id: 77,
        },
        None,
    )
    .unwrap_or_else(|error| panic!("real daemon handshake: {error:?}"));
    let node = handshake.node();
    assert_eq!(node.latest_sealed_batch, 9);
    assert_eq!(
        node.advertised_capabilities
            .iter()
            .any(|capability| capability == "availability_fetch"),
        !corrupt,
    );
    let first = lookup(
        &mut transport,
        Version::V1_4,
        1,
        1,
        node.authorised_sequencer_key,
    )
    .unwrap_or_else(|error| panic!("first signed header: {error:?}"));
    let last = lookup(
        &mut transport,
        Version::V1_4,
        9,
        2,
        node.authorised_sequencer_key,
    )
    .unwrap_or_else(|error| panic!("last signed header: {error:?}"));
    assert_ne!(first.header.data_availability_root(), [0; 32]);
    let mut batch = vec![2];
    batch.extend_from_slice(&1_u64.to_be_bytes());
    refusal(&mut transport, &batch, -804, 3);
    if !corrupt {
        let mut unknown = vec![2];
        unknown.extend_from_slice(&u64::MAX.to_be_bytes());
        refusal(&mut transport, &unknown, -106, 4);
        let mut range = vec![3];
        range.extend_from_slice(&first.header.first_sequence().to_be_bytes());
        range.extend_from_slice(&last.header.last_sequence().to_be_bytes());
        refusal(&mut transport, &range, -5, 5);
    }
}

#[test]
fn real_daemon_availability_refusals() {
    if let Some(socket) = std::env::var_os("LAYERX_TEST_AVAILABILITY_SOCKET") {
        let stage = std::env::var("LAYERX_TEST_AVAILABILITY_STAGE")
            .unwrap_or_else(|error| panic!("availability stage: {error}"));
        assert!(stage == "retained" || stage == "corrupt");
        probe(Path::new(&socket), stage == "corrupt");
        return;
    }
    let repository = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../..");
    let repository = repository
        .canonicalize()
        .unwrap_or_else(|error| panic!("repository path: {error}"));
    let binaries = std::env::var_os("LAYERX_TEST_NATIVE_BIN_DIR")
        .map_or_else(|| repository.join("build/bin"), PathBuf::from);
    assert!(binaries.join("layerxd").is_file(), "layerxd must be built");
    assert!(binaries.join("layerx-genesis-build").is_file());
    assert!(repository.join("build/tests/lxp_test_program_admission").is_file());
    let executable = std::env::current_exe()
        .unwrap_or_else(|error| panic!("Rust integration executable: {error}"));
    let status = Command::new("bash")
        .arg(repository.join("tests/daemon/program-admission.sh"))
        .arg("build")
        .arg("--availability-batches")
        .arg(executable)
        .env("LAYERX_TEST_NATIVE_BIN_DIR", binaries)
        .env("CARGO_BUILD_JOBS", "16")
        .env("MAKEFLAGS", "-j16")
        .current_dir(&repository)
        .status()
        .unwrap_or_else(|error| panic!("real daemon harness: {error}"));
    assert!(status.success(), "real daemon harness exit: {status}");
}
