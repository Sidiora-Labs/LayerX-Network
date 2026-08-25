use std::collections::{BTreeMap, BTreeSet};
use std::os::unix::net::UnixListener;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::thread;
use std::time::Duration;

use ed25519_dalek::{Signer as _, SigningKey};
use layerx_agentd::boot::{handshake_gate, Gate};
use layerx_agentd::config::StartupConfig;
use layerx_agentd::protocol_evidence::{
    EvidenceAuthority, RawReceiptEvidence, RawStateEvidence,
};
use layerx_agentd::store::TenantId;
use layerx_client::lni::framing::{read_frame, write_frame};
use layerx_client::lni::handshake::{encode_node_info, NodeInfo, NodeRole};
use layerx_client::lni::schema::{
    decode_envelope, encode_envelope, Envelope, Version,
};
use layerx_client::lni::transport::{ConnectionGate, Limits, Uds};
use layerx_human_service::store::{
    AgentTenantId, PrincipalId, PrincipalStore, RetentionPeriod, RetentionPolicy, RowKey,
    TenancyDigest, TenancyMap,
};
use layerx_proof::merkle::build_proof;
use layerx_proof::receipt::AuthorizedBatch;
use layerx_types::verify::VerificationLevel;
use layerx_wire::hash::execution_batch_id as wire_execution_batch_id;
use sha2::{Digest as _, Sha256};

static NEXT_DIRECTORY: AtomicU64 = AtomicU64::new(1);

pub fn directory(label: &str) -> PathBuf {
    let sequence = NEXT_DIRECTORY.fetch_add(1, Ordering::Relaxed);
    std::env::temp_dir().join(format!(
        "layerx-human-store-{label}-{}-{sequence}",
        std::process::id()
    ))
}

pub fn principal(name: &str) -> PrincipalId {
    PrincipalId::new(name).unwrap_or_else(|error| panic!("principal: {error}"))
}

pub fn row_key(name: &str) -> RowKey {
    RowKey::new(name).unwrap_or_else(|error| panic!("row key: {error}"))
}

pub fn tenancy(pairs: &[(&str, &str)]) -> TenancyMap {
    let entries = pairs.iter().map(|(name, tenant)| {
        (
            principal(name),
            AgentTenantId::new(*tenant).unwrap_or_else(|error| panic!("tenant: {error}")),
        )
    });
    TenancyMap::new(entries).unwrap_or_else(|error| panic!("tenancy map: {error}"))
}

pub fn retention_uniform(units: u64) -> RetentionPolicy {
    RetentionPolicy {
        journeys: RetentionPeriod::new(units),
        notifications: RetentionPeriod::new(units),
        audit: RetentionPeriod::new(units),
        telemetry: RetentionPeriod::new(units),
        cache: RetentionPeriod::new(units),
    }
}

pub fn install_and_open(
    root: &Path,
    map: &TenancyMap,
    retention: RetentionPolicy,
) -> (PrincipalStore, TenancyDigest) {
    let digest = map
        .install(root)
        .unwrap_or_else(|error| panic!("install tenancy: {error}"));
    let store = PrincipalStore::open(root, retention, digest)
        .unwrap_or_else(|error| panic!("open store: {error}"));
    (store, digest)
}

pub fn evidence_verifier(receipt_signer: &SigningKey) -> EvidenceAuthority {
    let receipt_key = receipt_signer.verifying_key().to_bytes();
    let authority_path = directory("evidence-authority").with_extension("csv");
    let authority_source = format!(
        "layerx-sequencer-authority-v1\n{},{},2,7,7,active\n",
        encode_hex(&receipt_key),
        encode_hex(&receipt_key),
    );
    std::fs::write(&authority_path, authority_source)
        .unwrap_or_else(|error| panic!("authority source: {error}"));
    let tenant = TenantId::new("human-evidence")
        .unwrap_or_else(|error| panic!("tenant: {error}"));
    let config = StartupConfig {
        network_id: 42,
        node_endpoint: PathBuf::from("/run/layerx/layerxd.sock"),
        expected_protocol_version: 1,
        tenants: BTreeSet::from([tenant.clone()]),
        policy_sources: BTreeMap::from([(
            tenant.clone(),
            PathBuf::from("/etc/layerx/human-policy.kvx"),
        )]),
        signer_configurations: BTreeMap::from([(
            tenant.clone(),
            PathBuf::from("/etc/layerx/human-signer.kvx"),
        )]),
        verification_defaults: BTreeMap::from([(
            tenant,
            VerificationLevel::STATE_PROVEN,
        )]),
        sequencer_authority_source: authority_path,
    };
    let mut gate = Gate::new(&config)
        .unwrap_or_else(|error| panic!("authority gate: {error:?}"));
    let socket_path = directory("evidence-handshake").with_extension("sock");
    let listener = UnixListener::bind(&socket_path)
        .unwrap_or_else(|error| panic!("bind evidence handshake: {error}"));
    let node = NodeInfo {
        interface_version: Version::V1_0,
        protocol_version: 1,
        network_id: 42,
        role: NodeRole::Sequencer,
        chain_head_sequence: 50,
        latest_sealed_batch: 7,
        latest_finalised_checkpoint: [0x91; 32],
        authorised_sequencer_key: receipt_key,
        advertised_capabilities: vec!["submit".to_owned()],
    };
    let server = thread::spawn(move || {
        let (mut stream, _) = listener
            .accept()
            .unwrap_or_else(|error| panic!("accept evidence handshake: {error}"));
        let request = read_frame(&mut stream, 1_048_576)
            .unwrap_or_else(|error| panic!("read evidence handshake: {error:?}"));
        let request = decode_envelope(&request)
            .unwrap_or_else(|error| panic!("decode evidence handshake: {error:?}"));
        assert_eq!(request.message_tag, 1);
        assert_eq!(request.correlation_id, 0);
        assert!(request.canonical_payload.is_empty());
        assert!(request.proof_material.is_empty());
        let payload = encode_node_info(&node)
            .unwrap_or_else(|error| panic!("encode evidence node information: {error:?}"));
        let response = encode_envelope(Envelope {
            version: node.interface_version,
            message_tag: 2,
            correlation_id: 0,
            canonical_payload: &payload,
            proof_material: &[],
        })
        .unwrap_or_else(|error| panic!("encode evidence handshake response: {error:?}"));
        write_frame(&mut stream, &response, 1_048_576)
            .unwrap_or_else(|error| panic!("write evidence handshake: {error:?}"));
    });
    let mut transport = Uds::connect(
        &socket_path,
        &ConnectionGate::new(1),
        Limits {
            maximum_frame_bytes: 1_048_576,
            maximum_connections: 1,
            maximum_streams: 1,
            maximum_queued_bytes: 1_048_576,
            deadline: Duration::from_secs(2),
        },
    )
    .unwrap_or_else(|error| panic!("connect evidence handshake: {error:?}"));
    handshake_gate(&mut gate, &mut transport)
        .unwrap_or_else(|error| panic!("accepted handshake: {error:?}"));
    server
        .join()
        .unwrap_or_else(|_| panic!("evidence handshake server panicked"));
    let _ = std::fs::remove_file(socket_path);
    gate.evidence_authority()
        .unwrap_or_else(|error| panic!("evidence authority: {error:?}"))
        .clone()
}

fn encode_hex(bytes: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut encoded = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        encoded.push(char::from(HEX[usize::from(byte >> 4)]));
        encoded.push(char::from(HEX[usize::from(byte & 0x0f)]));
    }
    encoded
}

pub fn execution_batch_id(
    previous_state_root: [u8; 32],
    activity_id: [u8; 32],
    global_sequence: u64,
) -> [u8; 32] {
    wire_execution_batch_id(previous_state_root, activity_id, global_sequence, 7)
        .unwrap_or_else(|error| panic!("execution batch id: {error:?}"))
}

pub fn raw_receipt_evidence(
    canonical_receipt: Vec<u8>,
    authorised_batch: AuthorizedBatch,
    global_sequence: u64,
    signer: &SigningKey,
) -> RawReceiptEvidence {
    assert_eq!(
        authorised_batch.sequencer_public_key(),
        signer.verifying_key().to_bytes()
    );
    let leaves = [canonical_receipt.as_slice()];
    let (proof, receipt_root) = build_proof(&leaves, 0)
        .unwrap_or_else(|error| panic!("receipt proof: {error:?}"));
    let sequencer_id = signer.verifying_key().to_bytes();
    let header = canonical_header(
        authorised_batch.previous_state_root(),
        authorised_batch.resulting_state_root(),
        [0x32; 32],
        receipt_root,
        global_sequence,
        sequencer_id,
    );
    let digest = batch_header_digest(&header);
    RawReceiptEvidence::new(
        canonical_receipt,
        proof,
        header,
        signer.sign(&digest).to_bytes(),
    )
}

pub fn raw_state_leaf(canonical_state: Vec<u8>, observed_head: u64) -> RawStateEvidence {
    let leaves = [canonical_state.as_slice()];
    let (proof, state_root) = build_proof(&leaves, 0)
        .unwrap_or_else(|error| panic!("state proof: {error:?}"));
    let signer = SigningKey::from_bytes(&[0x84; 32]);
    let sequencer_id = signer.verifying_key().to_bytes();
    let header = canonical_header(
        [0x31; 32],
        state_root,
        [0x32; 32],
        [0x33; 32],
        observed_head,
        sequencer_id,
    );
    let digest = batch_header_digest(&header);
    RawStateEvidence::new(
        canonical_state,
        proof,
        state_root,
        header,
        signer.sign(&digest).to_bytes(),
    )
}

fn canonical_header(
    previous_state_root: [u8; 32],
    resulting_state_root: [u8; 32],
    activity_root: [u8; 32],
    receipt_root: [u8; 32],
    last_sequence: u64,
    sequencer_id: [u8; 32],
) -> Vec<u8> {
    let mut encoded = Vec::with_capacity(354);
    encoded.extend_from_slice(&1_u16.to_be_bytes());
    encoded.extend_from_slice(&0x1701_u16.to_be_bytes());
    encoded.push(15);
    let fields: [(u8, Vec<u8>); 15] = [
        (1, 1_u16.to_be_bytes().to_vec()),
        (2, 42_u32.to_be_bytes().to_vec()),
        (3, 2_u64.to_be_bytes().to_vec()),
        (4, 7_u64.to_be_bytes().to_vec()),
        (5, 1_u64.to_be_bytes().to_vec()),
        (6, last_sequence.to_be_bytes().to_vec()),
        (7, previous_state_root.to_vec()),
        (8, resulting_state_root.to_vec()),
        (9, activity_root.to_vec()),
        (10, receipt_root.to_vec()),
        (11, [0x34; 32].to_vec()),
        (12, [0x35; 32].to_vec()),
        (13, [0x36; 32].to_vec()),
        (14, 1_000_u64.to_be_bytes().to_vec()),
        (15, sequencer_id.to_vec()),
    ];
    for (field, value) in fields {
        encoded.push(field);
        match field {
            1..=6 | 14 => encoded.extend_from_slice(&value),
            _ => {
                encoded.extend_from_slice(&32_u32.to_be_bytes());
                encoded.extend_from_slice(&value);
            }
        }
    }
    encoded
}

fn batch_header_digest(header: &[u8]) -> [u8; 32] {
    let mut digest = Sha256::new();
    digest.update(b"LXP/v1/batch-header\0");
    digest.update(header);
    digest.finalize().into()
}

#[allow(dead_code)]
pub fn version_file_bytes(version: u32) -> Vec<u8> {
    let mut bytes = Vec::with_capacity(8);
    bytes.extend_from_slice(b"LXHV");
    bytes.extend_from_slice(&version.to_be_bytes());
    bytes
}
