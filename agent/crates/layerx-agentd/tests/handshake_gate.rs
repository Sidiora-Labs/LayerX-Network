use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::os::unix::fs::{symlink, PermissionsExt};
use std::os::unix::net::UnixListener;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::thread;
use std::time::Duration;

use layerx_agentd::boot::{handshake_gate, Gate, GateError, Status, WriteGateError};
use layerx_agentd::config::StartupConfig;
use layerx_agentd::store::TenantId;
use layerx_client::lni::framing::{read_frame, write_frame};
use layerx_client::lni::handshake::{encode_node_info, HandshakeError, NodeInfo, NodeRole};
use layerx_client::lni::schema::{decode_envelope, encode_envelope, Capability, Envelope, Version};
use layerx_client::lni::transport::{ConnectionGate, Limits, Uds};
use layerx_programs::hex;
use layerx_types::verify::VerificationLevel;

static NEXT_SOCKET: AtomicU64 = AtomicU64::new(1);

fn socket_path(label: &str) -> PathBuf {
    let sequence = NEXT_SOCKET.fetch_add(1, Ordering::Relaxed);
    std::env::temp_dir().join(format!(
        "layerx-handshake-gate-{label}-{}-{sequence}.sock",
        std::process::id()
    ))
}

fn tenant() -> TenantId {
    TenantId::new("tenant-a").unwrap_or_else(|error| panic!("tenant: {error}"))
}

fn config() -> StartupConfig {
    let authority_source = socket_path("authority").with_extension("csv");
    let key = [8; 32];
    fs::write(
        &authority_source,
        format!(
            "layerx-sequencer-authority-v1\n{},{},0,1,100,active\n",
            hex::encode(&key),
            hex::encode(&key)
        ),
    )
    .unwrap_or_else(|error| panic!("authority source: {error}"));
    fs::set_permissions(&authority_source, fs::Permissions::from_mode(0o600))
        .unwrap_or_else(|error| panic!("authority source permissions: {error}"));
    config_with_authority(authority_source)
}

fn config_with_authority(authority_source: PathBuf) -> StartupConfig {
    let tenant = tenant();
    StartupConfig {
        network_id: 42,
        node_endpoint: PathBuf::from("/run/layerx/layerxd.sock"),
        expected_protocol_version: layerx_wire::limits::PROTOCOL_VERSION,
        tenants: BTreeSet::from([tenant.clone()]),
        policy_sources: BTreeMap::from([(
            tenant.clone(),
            PathBuf::from("/etc/layerx/policy-a.kvx"),
        )]),
        signer_configurations: BTreeMap::from([(
            tenant.clone(),
            PathBuf::from("/etc/layerx/signer-a.kvx"),
        )]),
        verification_defaults: BTreeMap::from([(tenant, VerificationLevel::STATE_PROVEN)]),
        sequencer_authority_source: authority_source,
    }
}

fn protected_authority(path: &Path) {
    let key = [8; 32];
    fs::write(
        path,
        format!(
            "layerx-sequencer-authority-v1\n{},{},0,0,100,active\n",
            hex::encode(&key),
            hex::encode(&key)
        ),
    )
    .unwrap_or_else(|error| panic!("authority source: {error}"));
    fs::set_permissions(path, fs::Permissions::from_mode(0o600))
        .unwrap_or_else(|error| panic!("authority source permissions: {error}"));
}

fn node(minor: u16, capabilities: &[&str]) -> NodeInfo {
    NodeInfo {
        interface_version: Version { major: 1, minor },
        protocol_version: layerx_wire::limits::PROTOCOL_VERSION,
        network_id: 42,
        role: NodeRole::Sequencer,
        chain_head_sequence: 900 + u64::from(minor),
        latest_sealed_batch: 44,
        latest_finalised_checkpoint: [7; 32],
        authorised_sequencer_key: [8; 32],
        advertised_capabilities: capabilities
            .iter()
            .map(|capability| (*capability).to_owned())
            .collect(),
    }
}

fn limits() -> Limits {
    Limits {
        maximum_frame_bytes: 1_048_576,
        maximum_connections: 1,
        maximum_streams: 1,
        maximum_queued_bytes: 1_048_576,
        deadline: Duration::from_secs(2),
    }
}

#[test]
fn authority_source_rejects_leaf_and_ancestor_symlinks() {
    let leaf_target = socket_path("authority-leaf-target").with_extension("csv");
    let leaf_link = socket_path("authority-leaf-link").with_extension("csv");
    protected_authority(&leaf_target);
    symlink(&leaf_target, &leaf_link).unwrap_or_else(|error| panic!("leaf symlink: {error}"));
    assert!(matches!(
        Gate::new(&config_with_authority(leaf_link.clone())),
        Err(GateError::Evidence(
            layerx_agentd::protocol_evidence::VerifierPolicyError::AuthoritySourceUnprotected
        ))
    ));

    let root = socket_path("authority-ancestor-root").with_extension("directory");
    let real = root.join("real");
    let linked = root.join("linked");
    fs::create_dir(&root).unwrap_or_else(|error| panic!("authority root: {error}"));
    fs::create_dir(&real).unwrap_or_else(|error| panic!("authority real directory: {error}"));
    symlink(&real, &linked).unwrap_or_else(|error| panic!("ancestor symlink: {error}"));
    let real_source = real.join("authorities.csv");
    protected_authority(&real_source);
    assert!(matches!(
        Gate::new(&config_with_authority(linked.join("authorities.csv"))),
        Err(GateError::Evidence(
            layerx_agentd::protocol_evidence::VerifierPolicyError::AuthoritySourceUnprotected
        ))
    ));

    let _ = fs::remove_file(leaf_link);
    let _ = fs::remove_file(leaf_target);
    let _ = fs::remove_file(linked);
    let _ = fs::remove_file(real_source);
    let _ = fs::remove_dir(real);
    let _ = fs::remove_dir(root);
}

#[test]
fn authority_source_rejects_permissive_and_non_regular_inputs() {
    let permissive = socket_path("authority-permissive").with_extension("csv");
    protected_authority(&permissive);
    fs::set_permissions(&permissive, fs::Permissions::from_mode(0o640))
        .unwrap_or_else(|error| panic!("permissive authority permissions: {error}"));
    assert!(matches!(
        Gate::new(&config_with_authority(permissive.clone())),
        Err(GateError::Evidence(
            layerx_agentd::protocol_evidence::VerifierPolicyError::AuthoritySourceUnprotected
        ))
    ));

    let directory = socket_path("authority-directory").with_extension("directory");
    fs::create_dir(&directory).unwrap_or_else(|error| panic!("authority directory: {error}"));
    assert!(matches!(
        Gate::new(&config_with_authority(directory.clone())),
        Err(GateError::Evidence(
            layerx_agentd::protocol_evidence::VerifierPolicyError::AuthoritySourceUnprotected
        ))
    ));

    let _ = fs::remove_file(permissive);
    let _ = fs::remove_dir(directory);
}

fn serve_handshake(path: &PathBuf, info: NodeInfo) -> thread::JoinHandle<()> {
    let listener =
        UnixListener::bind(path).unwrap_or_else(|error| panic!("bind handshake socket: {error}"));
    thread::spawn(move || {
        let (mut stream, _) = listener
            .accept()
            .unwrap_or_else(|error| panic!("accept handshake: {error}"));
        let request = read_frame(&mut stream, 1_048_576)
            .unwrap_or_else(|error| panic!("read handshake request: {error:?}"));
        let request = decode_envelope(&request)
            .unwrap_or_else(|error| panic!("decode handshake request: {error:?}"));
        assert_eq!(request.message_tag, 1);
        assert_eq!(request.correlation_id, 0);
        assert!(request.canonical_payload.is_empty());
        assert!(request.proof_material.is_empty());
        let payload = encode_node_info(&info)
            .unwrap_or_else(|error| panic!("encode node information: {error:?}"));
        let response = encode_envelope(Envelope {
            version: info.interface_version,
            message_tag: 2,
            correlation_id: 0,
            canonical_payload: &payload,
            proof_material: &[],
        })
        .unwrap_or_else(|error| panic!("encode handshake response: {error:?}"));
        write_frame(&mut stream, &response, 1_048_576)
            .unwrap_or_else(|error| panic!("write handshake response: {error:?}"));
    })
}

fn connect(path: &Path) -> Uds {
    Uds::connect(path, &ConnectionGate::new(1), limits())
        .unwrap_or_else(|error| panic!("connect handshake socket: {error:?}"))
}

#[test]
fn startup_is_not_ready_until_real_framed_handshake_reports_the_full_intersection() {
    let mut gate = Gate::new(&config()).unwrap_or_else(|error| panic!("startup gate: {error:?}"));
    let operation_ran = std::cell::Cell::new(false);
    assert_eq!(
        gate.guard_write(|| operation_ran.set(true)),
        Err(WriteGateError::NotReady)
    );
    assert!(!operation_ran.get());

    let path = socket_path("startup");
    let server = serve_handshake(
        &path,
        node(0, &["future_capability", "node_info", "submit"]),
    );
    let mut transport = connect(&path);
    let status = handshake_gate(&mut gate, &mut transport)
        .unwrap_or_else(|error| panic!("handshake gate: {error:?}"));
    assert_eq!(status.interface_version, Version::V1_0);
    assert_eq!(
        status.protocol_version,
        layerx_wire::limits::PROTOCOL_VERSION
    );
    assert_eq!(status.network_id, 42);
    assert_eq!(status.node_role, NodeRole::Sequencer);
    assert_eq!(status.chain_head_sequence, 900);
    assert_eq!(status.latest_sealed_batch, 44);
    assert_eq!(status.latest_finalised_checkpoint, [7; 32]);
    assert_eq!(status.authorised_sequencer_key, [8; 32]);
    assert!(status.available_capabilities.contains(&Capability::Submit));
    assert!(status
        .missing_capabilities
        .contains(&Capability::AvailabilityFetch));
    assert_eq!(status.unknown_advertised, ["future_capability"]);
    assert!(status.writes_ready);
    assert_eq!(gate.guard_write(|| 7), Ok(7));
    let report = gate
        .capability_report()
        .unwrap_or_else(|| panic!("capability report absent"));
    assert!(report.gaps().contains(&"availability_fetch"));
    server
        .join()
        .unwrap_or_else(|_| panic!("handshake server panicked"));
    let _ = fs::remove_file(path);
}

#[test]
fn reconnect_repeats_handshake_for_node_upgrade_and_disappearing_capability() {
    let mut gate = Gate::new(&config()).unwrap_or_else(|error| panic!("startup gate: {error:?}"));
    let first_path = socket_path("reconnect-first");
    let first_server = serve_handshake(&first_path, node(0, &["node_info", "submit"]));
    let mut first_transport = connect(&first_path);
    assert!(handshake_gate(&mut gate, &mut first_transport).is_ok());
    first_server
        .join()
        .unwrap_or_else(|_| panic!("first handshake server panicked"));

    gate.disconnected();
    assert_eq!(gate.guard_write(|| 1), Err(WriteGateError::NotReady));
    let second_path = socket_path("reconnect-second");
    let second_server = serve_handshake(&second_path, node(4, &["node_info"]));
    let mut second_transport = connect(&second_path);
    let status = handshake_gate(&mut gate, &mut second_transport)
        .unwrap_or_else(|error| panic!("reconnect handshake: {error:?}"));
    assert_eq!(status.generation, 2);
    assert_eq!(status.interface_version, Version { major: 1, minor: 4 });
    assert_eq!(status.chain_head_sequence, 904);
    assert!(status.missing_capabilities.contains(&Capability::Submit));
    assert!(!status.writes_ready);
    assert_eq!(
        gate.guard_write(|| 1),
        Err(WriteGateError::MissingCapability(Capability::Submit))
    );
    second_server
        .join()
        .unwrap_or_else(|_| panic!("second handshake server panicked"));
    let _ = fs::remove_file(first_path);
    let _ = fs::remove_file(second_path);
}

#[test]
fn network_protocol_and_major_upgrade_mismatches_refuse_operation() {
    let cases = [
        (
            "network",
            {
                let mut value = node(0, &["node_info", "submit"]);
                value.network_id = 99;
                value
            },
            HandshakeError::Network {
                expected: 42,
                peer: 99,
            },
        ),
        (
            "protocol",
            {
                let mut value = node(0, &["node_info", "submit"]);
                value.protocol_version = layerx_wire::limits::LEGACY_PROTOCOL_VERSION;
                value
            },
            HandshakeError::ProtocolVersion {
                expected: layerx_wire::limits::PROTOCOL_VERSION,
                peer: layerx_wire::limits::LEGACY_PROTOCOL_VERSION,
            },
        ),
        (
            "major",
            {
                let mut value = node(0, &["node_info", "submit"]);
                value.interface_version.major = 2;
                value
            },
            HandshakeError::InterfaceIncompatible {
                built: Version::V1_0,
                peer: Version { major: 2, minor: 0 },
            },
        ),
    ];
    for (label, info, expected) in cases {
        let path = socket_path(label);
        let server = serve_handshake(&path, info);
        let mut transport = connect(&path);
        let mut gate =
            Gate::new(&config()).unwrap_or_else(|error| panic!("startup gate: {error:?}"));
        assert_eq!(
            handshake_gate(&mut gate, &mut transport),
            Err(GateError::Handshake(expected))
        );
        assert_eq!(gate.status(), &Status::Refused(expected));
        assert_eq!(gate.guard_write(|| 1), Err(WriteGateError::NotReady));
        server
            .join()
            .unwrap_or_else(|_| panic!("mismatch handshake server panicked"));
        let _ = fs::remove_file(path);
    }
}
