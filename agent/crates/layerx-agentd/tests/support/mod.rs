use std::collections::{BTreeMap, BTreeSet};
use std::future::Future;
use std::os::unix::fs::PermissionsExt;
use std::os::unix::net::UnixListener;
use std::path::PathBuf;
use std::pin::pin;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::task::{Context, Poll, Wake, Waker};
use std::thread;
use std::time::Duration;

use layerx_agentd::boot::{handshake_gate, Gate, GateError};
use layerx_agentd::config::StartupConfig;
use layerx_agentd::protocol_evidence::{
    EvidenceAuthority, RawReceiptEvidence, RawStateEvidence,
};
use layerx_agentd::prepare::{
    prepare_activity, CorePreparationBoundary, CorePreparationState, CoreStateError,
    PreparationDefaults, PrepareRequest,
};
use layerx_agentd::sign::{attach_external_signature, verify_before_submit, VerifiedSubmission};
use layerx_agentd::store::TenantId;
use layerx_crypto::local::LocalSigner;
use layerx_crypto::signer::{sign_disclosed, Signer};
use layerx_types::activity::{Authority, TimestampBound};
use layerx_types::amount::Amount;
use layerx_types::ids::{Did, IdempotencyKey};
use layerx_types::payload::{ActivityType, ModuleId, ModuleRegistration, ModuleRegistry};
use layerx_wire::encode::Encoder;
use layerx_client::lni::framing::{read_frame, write_frame};
use layerx_client::lni::handshake::{encode_node_info, NodeInfo, NodeRole};
use layerx_client::lni::schema::{
    decode_envelope, encode_envelope, Envelope, Version,
};
use layerx_client::lni::transport::{ConnectionGate, Limits, Uds};
use layerx_programs::hex;
use layerx_proof::merkle::build_proof;
use layerx_types::verify::VerificationLevel;
use layerx_wire::hash::{batch_header_digest, execution_batch_id, receipt_digest};
use ed25519_dalek::{Signer as _, SigningKey};

static NEXT_DIRECTORY: AtomicU64 = AtomicU64::new(1);

struct NoopWake;

impl Wake for NoopWake {
    fn wake(self: Arc<Self>) {}
}

fn ready<F: Future>(future: F) -> F::Output {
    let mut future = pin!(future);
    let waker = Waker::from(Arc::new(NoopWake));
    let mut context = Context::from_waker(&waker);
    match future.as_mut().poll(&mut context) {
        Poll::Ready(value) => value,
        Poll::Pending => panic!("local signing unexpectedly blocked"),
    }
}

struct RecordedCore(CorePreparationState);

impl CorePreparationBoundary for RecordedCore {
    fn preparation_state(&mut self, _actor: &Did) -> Result<CorePreparationState, CoreStateError> {
        Ok(self.0.clone())
    }
}

fn activity_type() -> ActivityType {
    ActivityType::new(ModuleId::Asset, 5).unwrap_or_else(|error| panic!("activity: {error:?}"))
}

fn registry() -> ModuleRegistry {
    ModuleRegistry::new(
        &[ModuleRegistration::new(ModuleId::Asset, &[activity_type()])
            .unwrap_or_else(|error| panic!("registration: {error:?}"))],
    )
    .unwrap_or_else(|error| panic!("registry: {error:?}"))
}

fn send_payload(id: u8) -> Vec<u8> {
    let mut encoder = Encoder::new(512);
    encoder
        .u16(0x5301)
        .unwrap_or_else(|error| panic!("tag: {error:?}"));
    encoder
        .u16(10)
        .unwrap_or_else(|error| panic!("fields: {error:?}"));
    for fixed in [[0x11; 32], [0x22; 32], [0x33; 32]] {
        encoder
            .fixed(&fixed)
            .unwrap_or_else(|error| panic!("fixed: {error:?}"));
    }
    encoder
        .u128(25)
        .unwrap_or_else(|error| panic!("amount: {error:?}"));
    encoder
        .u64(5)
        .unwrap_or_else(|error| panic!("sequence: {error:?}"));
    encoder
        .fixed(&[id; 32])
        .unwrap_or_else(|error| panic!("idempotency: {error:?}"));
    encoder
        .u64(1_010)
        .unwrap_or_else(|error| panic!("expiry: {error:?}"));
    encoder
        .fixed(&[0x55; 32])
        .unwrap_or_else(|error| panic!("context: {error:?}"));
    encoder
        .u8(0)
        .unwrap_or_else(|error| panic!("conditions: {error:?}"));
    encoder
        .u8(1)
        .unwrap_or_else(|error| panic!("authority: {error:?}"));
    encoder
        .fixed(&[0x11; 32])
        .unwrap_or_else(|error| panic!("controller: {error:?}"));
    encoder
        .fixed(&[0x66; 32])
        .unwrap_or_else(|error| panic!("payload key: {error:?}"));
    encoder
        .fixed(&[0x77; 64])
        .unwrap_or_else(|error| panic!("payload signature: {error:?}"));
    encoder
        .fixed(&[0x55; 32])
        .unwrap_or_else(|error| panic!("signed context: {error:?}"));
    encoder
        .u32(17)
        .unwrap_or_else(|error| panic!("network: {error:?}"));
    encoder
        .u16(1)
        .unwrap_or_else(|error| panic!("version: {error:?}"));
    encoder.finish()
}

pub fn verified_submission(id: u8) -> VerifiedSubmission {
    let mut core = RecordedCore(CorePreparationState {
        network_id: 17,
        account_sequence: 5,
        protocol_timestamp: 1_000,
        observed_head_sequence: 88,
        module_registry: registry(),
    });
    let prepared = prepare_activity(
        &mut core,
        PreparationDefaults {
            timestamp_span: 30,
            fee_limit: Amount::from_u128(12),
            maximum_payload_bytes: 1_024,
        },
        PrepareRequest {
            actor: Did::new(b"did:layerx:recovery")
                .unwrap_or_else(|error| panic!("DID: {error:?}")),
            authority: Authority::owner(b"external-authority")
                .unwrap_or_else(|error| panic!("authority: {error:?}")),
            activity_type: activity_type(),
            expected_account_sequence: Some(5),
            timestamp_bound: Some(
                TimestampBound::new(995, 1_010)
                    .unwrap_or_else(|error| panic!("timestamp: {error:?}")),
            ),
            fee_limit: Some(Amount::from_u128(7)),
            idempotency_key: IdempotencyKey::new([id; 32]),
            payload: send_payload(id),
            declared_payload_limit: 1_024,
        },
    )
    .unwrap_or_else(|error| panic!("prepare: {error:?}"));
    let signer = LocalSigner::new([0xa5; 32]);
    let signature = ready(sign_disclosed(
        &signer,
        &prepared.canonical_bytes,
        &prepared.disclosure,
        &registry(),
    ))
    .unwrap_or_else(|error| panic!("sign: {error:?}"));
    let signed_bytes = attach_external_signature(&prepared, *signature.as_bytes())
        .unwrap_or_else(|error| panic!("attach: {error:?}"));
    verify_before_submit(&signed_bytes, &prepared, &signer.public_key(), &registry())
        .unwrap_or_else(|error| panic!("verify: {error:?}"))
}

pub fn tenant() -> TenantId {
    TenantId::new("tenant-a").unwrap_or_else(|error| panic!("tenant: {error}"))
}

pub fn directory(label: &str) -> std::path::PathBuf {
    let sequence = NEXT_DIRECTORY.fetch_add(1, Ordering::Relaxed);
    std::env::temp_dir().join(format!(
        "layerx-recovery-{label}-{}-{sequence}",
        std::process::id()
    ))
}

#[derive(Clone, Copy)]
pub struct TestAuthorityRecord {
    pub signing_seed: [u8; 32],
    pub epoch: u64,
    pub first_batch: u64,
    pub last_batch: u64,
    pub revoked_at_batch: Option<u64>,
}

pub struct TestAuthorityPolicy<'a> {
    pub protocol_version: u16,
    pub network_id: u32,
    pub records: &'a [TestAuthorityRecord],
    pub handshake_signing_seed: [u8; 32],
    pub handshake_batch: u64,
}

pub fn evidence_authority(policy: TestAuthorityPolicy<'_>) -> EvidenceAuthority {
    try_evidence_authority(policy)
        .unwrap_or_else(|error| panic!("evidence authority: {error:?}"))
}

pub fn evidence_authority_for_sequencer(
    policy: TestAuthorityPolicy<'_>,
    sequencer_id: [u8; 32],
) -> EvidenceAuthority {
    try_evidence_authority_with_sequencer(policy, Some(sequencer_id))
        .unwrap_or_else(|error| panic!("evidence authority: {error:?}"))
}

pub fn try_evidence_authority(
    policy: TestAuthorityPolicy<'_>,
) -> Result<EvidenceAuthority, GateError> {
    try_evidence_authority_with_sequencer(policy, None)
}

fn try_evidence_authority_with_sequencer(
    policy: TestAuthorityPolicy<'_>,
    sequencer_id: Option<[u8; 32]>,
) -> Result<EvidenceAuthority, GateError> {
    let authority_path = directory("evidence-authority").with_extension("csv");
    let mut authority_source = "layerx-sequencer-authority-v1\n".to_owned();
    for record in policy.records {
        let public_key = SigningKey::from_bytes(&record.signing_seed)
            .verifying_key()
            .to_bytes();
        authority_source.push_str(&hex::encode(&sequencer_id.unwrap_or(public_key)));
        authority_source.push(',');
        authority_source.push_str(&hex::encode(&public_key));
        authority_source.push(',');
        authority_source.push_str(&record.epoch.to_string());
        authority_source.push(',');
        authority_source.push_str(&record.first_batch.to_string());
        authority_source.push(',');
        authority_source.push_str(&record.last_batch.to_string());
        authority_source.push(',');
        authority_source.push_str(
            &record
                .revoked_at_batch
                .map_or_else(|| "active".to_owned(), |batch| batch.to_string()),
        );
        authority_source.push('\n');
    }
    std::fs::write(&authority_path, authority_source)
        .unwrap_or_else(|error| panic!("authority source: {error}"));
    std::fs::set_permissions(&authority_path, std::fs::Permissions::from_mode(0o600))
        .unwrap_or_else(|error| panic!("authority source permissions: {error}"));
    let tenant = tenant();
    let config = StartupConfig {
        network_id: policy.network_id,
        node_endpoint: PathBuf::from("/run/layerx/layerxd.sock"),
        expected_protocol_version: policy.protocol_version,
        tenants: BTreeSet::from([tenant.clone()]),
        policy_sources: BTreeMap::from([(
            tenant.clone(),
            PathBuf::from("/etc/layerx/policy-a.kvx"),
        )]),
        signer_configurations: BTreeMap::from([(
            tenant.clone(),
            PathBuf::from("/etc/layerx/signer-a.kvx"),
        )]),
        verification_defaults: BTreeMap::from([(
            tenant,
            VerificationLevel::STATE_PROVEN,
        )]),
        sequencer_authority_source: authority_path,
    };
    let mut gate = Gate::new(&config)?;
    let handshake_key = SigningKey::from_bytes(&policy.handshake_signing_seed)
        .verifying_key()
        .to_bytes();
    let socket_path = directory("evidence-handshake").with_extension("sock");
    let listener = UnixListener::bind(&socket_path)
        .unwrap_or_else(|error| panic!("bind evidence handshake: {error}"));
    let node = NodeInfo {
        interface_version: Version::V1_0,
        protocol_version: policy.protocol_version,
        network_id: policy.network_id,
        role: NodeRole::Sequencer,
        chain_head_sequence: 50,
        latest_sealed_batch: policy.handshake_batch,
        latest_finalised_checkpoint: [0x91; 32],
        authorised_sequencer_key: handshake_key,
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
    let handshake = handshake_gate(&mut gate, &mut transport).map(|_| ());
    server
        .join()
        .unwrap_or_else(|_| panic!("evidence handshake server panicked"));
    let _ = std::fs::remove_file(socket_path);
    handshake?;
    Ok(gate
        .evidence_authority()
        .unwrap_or_else(|error| panic!("write-ready evidence authority: {error:?}"))
        .clone())
}

pub fn evidence_verifier() -> EvidenceAuthority {
    evidence_authority(TestAuthorityPolicy {
        protocol_version: 1,
        network_id: 42,
        records: &[TestAuthorityRecord {
            signing_seed: [0x3a; 32],
            epoch: 2,
            first_batch: 7,
            last_batch: 7,
            revoked_at_batch: None,
        }],
        handshake_signing_seed: [0x3a; 32],
        handshake_batch: 7,
    })
}

pub fn raw_receipt(activity_id: [u8; 32], result_code: i32, amount: u128) -> RawReceiptEvidence {
    raw_receipt_at(activity_id, result_code, amount, 9)
}

pub fn raw_receipt_at(
    activity_id: [u8; 32],
    result_code: i32,
    amount: u128,
    global_sequence: u64,
) -> RawReceiptEvidence {
    raw_receipt_for_execution_batch(
        activity_id,
        result_code,
        amount,
        global_sequence,
        7,
    )
}

pub fn raw_receipt_for_execution_batch(
    activity_id: [u8; 32],
    result_code: i32,
    amount: u128,
    global_sequence: u64,
    execution_batch_number: u64,
) -> RawReceiptEvidence {
    let key = SigningKey::from_bytes(&[0x3a; 32]);
    let previous_state_root = [0x21; 32];
    let resulting_state_root = [0x22; 32];
    let batch_id = execution_batch_id(
        previous_state_root,
        activity_id,
        global_sequence,
        execution_batch_number,
    )
    .unwrap_or_else(|error| panic!("execution batch id: {error:?}"));
    let asset = [0x24; 32];
    let encode = |signature: Option<[u8; 64]>| {
        let mut encoder = Encoder::new(4096);
        assert_eq!(encoder.structure_header(0x5201), Ok(()));
        assert_eq!(encoder.u16(1), Ok(()));
        assert_eq!(encoder.bytes(&activity_id, 32), Ok(()));
        assert_eq!(encoder.u64(global_sequence), Ok(()));
        assert_eq!(encoder.bytes(&previous_state_root, 32), Ok(()));
        assert_eq!(encoder.bytes(&resulting_state_root, 32), Ok(()));
        assert_eq!(encoder.bytes(&[0x25; 32], 32), Ok(()));
        assert_eq!(encoder.i32(result_code), Ok(()));
        assert_eq!(encoder.sequence_length(0, 512), Ok(()));
        assert_eq!(encoder.u128(1), Ok(()));
        assert_eq!(encoder.bytes(&batch_id, 32), Ok(()));
        assert_eq!(encoder.u16(1), Ok(()));
        assert_eq!(encoder.u32(1), Ok(()));
        assert_eq!(encoder.u32(1), Ok(()));
        assert_eq!(encoder.u8(1), Ok(()));
        assert_eq!(encoder.bytes(&asset, 32), Ok(()));
        assert_eq!(encoder.u128(amount), Ok(()));
        assert_eq!(encoder.bytes(&[0x26; 32], 32), Ok(()));
        assert_eq!(encoder.u128(1_000 + amount), Ok(()));
        assert_eq!(encoder.u128(if result_code == 0 { 1_000 } else { 1_000 + amount }), Ok(()));
        assert_eq!(encoder.u64(1), Ok(()));
        assert_eq!(encoder.bytes(&[0x27; 32], 32), Ok(()));
        assert_eq!(encoder.u128(10), Ok(()));
        assert_eq!(encoder.u128(if result_code == 0 { 10 + amount } else { 10 }), Ok(()));
        for value in [[0x28; 32], [0x29; 32], [0x2a; 32]] {
            assert_eq!(encoder.bytes(&value, 32), Ok(()));
        }
        assert_eq!(encoder.u64(1_000), Ok(()));
        assert_eq!(encoder.u8(u8::from(signature.is_some())), Ok(()));
        if let Some(signature) = signature {
            assert_eq!(encoder.bytes(&signature, 64), Ok(()));
        }
        encoder.finish()
    };
    let unsigned = encode(None);
    let digest = receipt_digest(&unsigned)
        .unwrap_or_else(|error| panic!("receipt digest: {error:?}"));
    let canonical_receipt = encode(Some(key.sign(&digest).to_bytes()));
    let leaves = [canonical_receipt.as_slice()];
    let (proof, receipt_root) = build_proof(&leaves, 0)
        .unwrap_or_else(|error| panic!("receipt proof: {error:?}"));
    let sequencer_id = key.verifying_key().to_bytes();
    let mut encoder = Encoder::new(354);
    assert_eq!(encoder.structure_header(0x1701), Ok(()));
    assert_eq!(encoder.u8(15), Ok(()));
    let fields: [(u8, Vec<u8>); 15] = [
        (1, 1_u16.to_be_bytes().to_vec()),
        (2, 42_u32.to_be_bytes().to_vec()),
        (3, 2_u64.to_be_bytes().to_vec()),
        (4, 7_u64.to_be_bytes().to_vec()),
        (5, global_sequence.to_be_bytes().to_vec()),
        (6, global_sequence.to_be_bytes().to_vec()),
        (7, previous_state_root.to_vec()),
        (8, resulting_state_root.to_vec()),
        (9, [0x25; 32].to_vec()),
        (10, receipt_root.to_vec()),
        (11, [0x34; 32].to_vec()),
        (12, [0x35; 32].to_vec()),
        (13, [0x36; 32].to_vec()),
        (14, 1_000_u64.to_be_bytes().to_vec()),
        (15, sequencer_id.to_vec()),
    ];
    for (field, value) in fields {
        assert_eq!(encoder.tag(field, 15), Ok(()));
        match field {
            1 => assert_eq!(
                encoder.u16(u16::from_be_bytes([value[0], value[1]])),
                Ok(())
            ),
            2 => assert_eq!(
                encoder.u32(u32::from_be_bytes(
                    value
                        .as_slice()
                        .try_into()
                        .unwrap_or_else(|_| panic!("u32 field"))
                )),
                Ok(())
            ),
            3..=6 | 14 => assert_eq!(
                encoder.u64(u64::from_be_bytes(
                    value
                        .as_slice()
                        .try_into()
                        .unwrap_or_else(|_| panic!("u64 field"))
                )),
                Ok(())
            ),
            _ => assert_eq!(encoder.bytes(&value, 32), Ok(())),
        }
    }
    let header = encoder.finish();
    let header_digest = batch_header_digest(&header)
        .unwrap_or_else(|error| panic!("header digest: {error:?}"));
    RawReceiptEvidence::new(
        canonical_receipt,
        proof,
        header,
        key.sign(&header_digest).to_bytes(),
    )
}

pub fn corrupt_raw_receipt(
    raw: &RawReceiptEvidence,
    canonical_receipt: Vec<u8>,
) -> RawReceiptEvidence {
    RawReceiptEvidence::new(
        canonical_receipt,
        raw.proof().clone(),
        raw.canonical_header().to_vec(),
        raw.header_signature(),
    )
}

pub fn raw_state_leaf(
    canonical_state: Vec<u8>,
    observed_head: u64,
) -> RawStateEvidence {
    raw_state_leaf_with(
        canonical_state,
        observed_head,
        StateHeaderIdentity {
            signing_seed: [0x3a; 32],
            protocol_version: 1,
            network_id: 42,
            epoch: 2,
            batch_number: 7,
        },
    )
}

#[derive(Clone, Copy)]
pub struct StateHeaderIdentity {
    pub signing_seed: [u8; 32],
    pub protocol_version: u16,
    pub network_id: u32,
    pub epoch: u64,
    pub batch_number: u64,
}

pub fn raw_state_leaf_with(
    canonical_state: Vec<u8>,
    observed_head: u64,
    identity: StateHeaderIdentity,
) -> RawStateEvidence {
    let sequencer_id = SigningKey::from_bytes(&identity.signing_seed)
        .verifying_key()
        .to_bytes();
    raw_state_leaf_with_sequencer_id(canonical_state, observed_head, identity, sequencer_id)
}

pub fn raw_state_leaf_with_sequencer_id(
    canonical_state: Vec<u8>,
    observed_head: u64,
    identity: StateHeaderIdentity,
    sequencer_id: [u8; 32],
) -> RawStateEvidence {
    let leaves = [canonical_state.as_slice()];
    let (proof, root) = build_proof(&leaves, 0)
        .unwrap_or_else(|error| panic!("state proof: {error:?}"));
    let key = SigningKey::from_bytes(&identity.signing_seed);
    let mut encoder = Encoder::new(354);
    assert_eq!(encoder.structure_header(0x1701), Ok(()));
    assert_eq!(encoder.u8(15), Ok(()));
    let fields: [(u8, Vec<u8>); 15] = [
        (1, identity.protocol_version.to_be_bytes().to_vec()),
        (2, identity.network_id.to_be_bytes().to_vec()),
        (3, identity.epoch.to_be_bytes().to_vec()),
        (4, identity.batch_number.to_be_bytes().to_vec()),
        (5, 1_u64.to_be_bytes().to_vec()),
        (6, observed_head.to_be_bytes().to_vec()),
        (7, [0x31; 32].to_vec()),
        (8, root.to_vec()),
        (9, [0x32; 32].to_vec()),
        (10, [0x33; 32].to_vec()),
        (11, [0x34; 32].to_vec()),
        (12, [0x35; 32].to_vec()),
        (13, [0x36; 32].to_vec()),
        (14, 1_000_u64.to_be_bytes().to_vec()),
        (15, sequencer_id.to_vec()),
    ];
    for (field, value) in fields {
        assert_eq!(encoder.tag(field, 15), Ok(()));
        match field {
            1 => assert_eq!(encoder.u16(u16::from_be_bytes([value[0], value[1]])), Ok(())),
            2 => assert_eq!(encoder.u32(u32::from_be_bytes(value.as_slice().try_into().unwrap_or_else(|_| panic!("u32 field")))), Ok(())),
            3..=6 | 14 => assert_eq!(encoder.u64(u64::from_be_bytes(value.as_slice().try_into().unwrap_or_else(|_| panic!("u64 field")))), Ok(())),
            _ => assert_eq!(encoder.bytes(&value, 32), Ok(())),
        }
    }
    let header = encoder.finish();
    let digest = batch_header_digest(&header)
        .unwrap_or_else(|error| panic!("header digest: {error:?}"));
    RawStateEvidence::new(
        canonical_state,
        proof,
        root,
        header,
        key.sign(&digest).to_bytes(),
    )
}

pub fn corrupt_raw_state(
    raw: &RawStateEvidence,
    canonical_state: Vec<u8>,
) -> RawStateEvidence {
    RawStateEvidence::new(
        canonical_state,
        raw.proof().clone(),
        raw.resulting_state_root(),
        raw.canonical_header().to_vec(),
        raw.header_signature(),
    )
}
