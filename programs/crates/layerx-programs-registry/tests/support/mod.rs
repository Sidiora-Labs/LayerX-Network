use ed25519_dalek::{Signer as _, SigningKey};
use layerx_programs::{
    interface_state_key, interface_state_value, programs_root_commitment, state_leaf_commitment,
    state_node_commitment, DeploymentProof, InterfaceEntryPoint, InterfaceStateWitness,
    ProgramInterface, ProgramLifecycleProof, ProgramStateProof, ProtocolDeploymentVerifier,
    StateLeafWitness, StateProof, ValueSchema, ValueType,
};
use layerx_programs_runtime::{hash_bytes, HashAlgorithm, ProgramId, UpgradePolicy};
use layerx_proof::merkle::build_proof;
use layerx_types::payload::{ActivityType, ModuleId, ModuleRegistration, ModuleRegistry};
use layerx_wire::activity::decode_signed;
use layerx_wire::encode::Encoder;
use layerx_wire::hash::{activity_id, batch_header_digest, execution_batch_id, receipt_digest};

pub const NOW: u64 = 1_700_000_150;
pub const PROGRAM_BYTES: [u8; 32] = [0x31; 32];
pub const AUTHORITY: [u8; 32] = [0x51; 32];
pub const WASM_V1: &[u8] = &[
    0, 97, 115, 109, 1, 0, 0, 0, 1, 12, 2, 96, 2, 127, 127, 1, 127, 96, 1, 127, 1, 127, 3, 3, 2, 0,
    1, 5, 3, 1, 0, 1, 7, 34, 3, 4, b'c', b'a', b'l', b'l', 0, 0, 14, b'l', b'a', b'y', b'e', b'r',
    b'x', b'_', b'r', b'e', b's', b'e', b'r', b'v', b'e', 0, 1, 6, b'm', b'e', b'm', b'o', b'r',
    b'y', 2, 0, 10, 11, 2, 4, 0, 65, 0, 11, 4, 0, 65, 0, 11,
];
pub const WASM_V2: &[u8] = &[
    0, 97, 115, 109, 1, 0, 0, 0, 1, 12, 2, 96, 2, 127, 127, 1, 127, 96, 1, 127, 1, 127, 3, 3, 2, 0,
    1, 5, 3, 1, 0, 1, 7, 34, 3, 4, b'c', b'a', b'l', b'l', 0, 0, 14, b'l', b'a', b'y', b'e', b'r',
    b'x', b'_', b'r', b'e', b's', b'e', b'r', b'v', b'e', 0, 1, 6, b'm', b'e', b'm', b'o', b'r',
    b'y', 2, 0, 10, 11, 2, 4, 0, 65, 1, 11, 4, 0, 65, 0, 11,
];
const TRUST_HISTORY_DOMAIN: &[u8] = b"LayerX/sequencer-trust-history/v1\0";

pub struct ProtocolFixture {
    pub proof: DeploymentProof,
    pub interface_witness: InterfaceStateWitness,
    pub sequencer_id: [u8; 32],
    pub sequencer_public_key: [u8; 32],
    pub batch_number: u64,
}

#[derive(Clone, Copy)]
pub struct TrustAnchorFixture {
    pub protocol_version: u16,
    pub network_id: u32,
    pub epoch: u64,
    pub sequencer_id: [u8; 32],
    pub sequencer_public_key: [u8; 32],
    pub first_batch: u64,
    pub last_batch: u64,
    pub revoked_from_batch: Option<u64>,
}

pub fn verifier_for_fixture(
    fixture: &ProtocolFixture,
    first_batch: u64,
    last_batch: u64,
    revoked_from_batch: Option<u64>,
    staleness_ms: u64,
) -> ProtocolDeploymentVerifier {
    verifier_from_history(
        &[TrustAnchorFixture {
            protocol_version: 2,
            network_id: 42,
            epoch: 2,
            sequencer_id: fixture.sequencer_id,
            sequencer_public_key: fixture.sequencer_public_key,
            first_batch,
            last_batch,
            revoked_from_batch,
        }],
        0,
        staleness_ms,
    )
}

pub fn verifier_from_history(
    anchors: &[TrustAnchorFixture],
    current_anchor: usize,
    staleness_ms: u64,
) -> ProtocolDeploymentVerifier {
    try_verifier_from_history(anchors, current_anchor, staleness_ms)
        .unwrap_or_else(|error| panic!("protocol verifier: {error}"))
}

pub fn try_verifier_from_history(
    anchors: &[TrustAnchorFixture],
    current_anchor: usize,
    staleness_ms: u64,
) -> Result<ProtocolDeploymentVerifier, layerx_programs::ProtocolEvidenceError> {
    use std::fs;
    use std::sync::atomic::{AtomicU64, Ordering};

    static NEXT_HISTORY: AtomicU64 = AtomicU64::new(0);
    let mut entries = anchors
        .iter()
        .copied()
        .map(|anchor| {
            let mut bytes = Vec::with_capacity(103);
            bytes.extend_from_slice(&anchor.protocol_version.to_be_bytes());
            bytes.extend_from_slice(&anchor.network_id.to_be_bytes());
            bytes.extend_from_slice(&anchor.epoch.to_be_bytes());
            bytes.extend_from_slice(&anchor.sequencer_id);
            bytes.extend_from_slice(&anchor.sequencer_public_key);
            bytes.extend_from_slice(&anchor.first_batch.to_be_bytes());
            bytes.extend_from_slice(&anchor.last_batch.to_be_bytes());
            bytes.push(u8::from(anchor.revoked_from_batch.is_some()));
            bytes.extend_from_slice(&anchor.revoked_from_batch.unwrap_or(0).to_be_bytes());
            (bytes, anchor)
        })
        .collect::<Vec<_>>();
    entries.sort_by(|left, right| left.0.cmp(&right.0));
    let selected = entries
        .iter()
        .position(|(_, anchor)| {
            let requested = anchors[current_anchor];
            anchor.protocol_version == requested.protocol_version
                && anchor.network_id == requested.network_id
                && anchor.epoch == requested.epoch
                && anchor.sequencer_id == requested.sequencer_id
                && anchor.sequencer_public_key == requested.sequencer_public_key
                && anchor.first_batch == requested.first_batch
                && anchor.last_batch == requested.last_batch
                && anchor.revoked_from_batch == requested.revoked_from_batch
        })
        .unwrap_or_else(|| panic!("current trust anchor is absent"));
    let mut bytes = Vec::new();
    bytes.extend_from_slice(TRUST_HISTORY_DOMAIN);
    bytes.extend_from_slice(
        &u16::try_from(entries.len())
            .unwrap_or_else(|_| panic!("trust history length"))
            .to_be_bytes(),
    );
    bytes.extend_from_slice(
        &u16::try_from(selected)
            .unwrap_or_else(|_| panic!("current trust anchor index"))
            .to_be_bytes(),
    );
    for (entry, _) in entries {
        bytes.extend_from_slice(&entry);
    }
    let path = std::env::temp_dir().join(format!(
        "layerx-programs-trust-{}-{}",
        std::process::id(),
        NEXT_HISTORY.fetch_add(1, Ordering::Relaxed),
    ));
    fs::write(&path, bytes).unwrap_or_else(|error| panic!("write trust history: {error}"));
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt as _;
        fs::set_permissions(&path, fs::Permissions::from_mode(0o600))
            .unwrap_or_else(|error| panic!("protect trust history: {error}"));
    }
    let verifier = ProtocolDeploymentVerifier::from_protected_history(&path, staleness_ms);
    fs::remove_file(&path).unwrap_or_else(|error| panic!("remove trust history: {error}"));
    verifier
}

pub fn program() -> ProgramId {
    ProgramId::new(PROGRAM_BYTES).unwrap_or_else(|error| panic!("program: {error}"))
}

pub fn code_hash(wasm: &[u8]) -> [u8; 32] {
    hash_bytes(HashAlgorithm::Sha256, wasm)
        .unwrap_or_else(|error| panic!("program code hash: {error}"))
}

pub fn deploy_fixture(
    wasm: &[u8],
    policy: UpgradePolicy,
    batch_number: u64,
    timestamp: u64,
) -> ProtocolFixture {
    deploy_fixture_in_epoch(wasm, policy, batch_number, timestamp, 2, [7; 32])
}

pub fn legacy_deploy_fixture(
    wasm: &[u8],
    policy: UpgradePolicy,
    batch_number: u64,
    timestamp: u64,
) -> ProtocolFixture {
    let mut payload = deploy_payload(wasm, policy);
    let interface_length = usize::try_from(u32::from_be_bytes(
        payload[104..108]
            .try_into()
            .unwrap_or_else(|_| panic!("interface length")),
    ))
    .unwrap_or_else(|_| panic!("interface length usize"));
    payload.drain(104..108 + interface_length);
    fixture(
        payload,
        1,
        wasm,
        policy,
        1,
        batch_number,
        timestamp,
        false,
        false,
    )
}

pub fn programs_call_fixture(
    payload: Vec<u8>,
    batch_number: u64,
    timestamp: u64,
) -> ProtocolFixture {
    fixture(
        payload,
        3,
        WASM_V1,
        UpgradePolicy::Authority(AUTHORITY),
        1,
        batch_number,
        timestamp,
        false,
        false,
    )
}

pub fn deploy_fixture_in_epoch(
    wasm: &[u8],
    policy: UpgradePolicy,
    batch_number: u64,
    timestamp: u64,
    epoch: u64,
    signing_key: [u8; 32],
) -> ProtocolFixture {
    let payload = deploy_payload(wasm, policy);
    fixture_in_epoch(
        payload,
        1,
        wasm,
        policy,
        1,
        batch_number,
        timestamp,
        false,
        false,
        epoch,
        signing_key,
    )
}

pub fn wrong_batch_id_fixture(batch_number: u64, timestamp: u64) -> ProtocolFixture {
    let policy = UpgradePolicy::Authority(AUTHORITY);
    fixture(
        deploy_payload(WASM_V1, policy),
        1,
        WASM_V1,
        policy,
        1,
        batch_number,
        timestamp,
        false,
        true,
    )
}

pub fn wrong_abi_fixture(batch_number: u64, timestamp: u64) -> ProtocolFixture {
    let policy = UpgradePolicy::Authority(AUTHORITY);
    let mut payload = deploy_payload(WASM_V1, policy);
    payload[32..34].copy_from_slice(&3_u16.to_be_bytes());
    payload[168..170].copy_from_slice(&3_u16.to_be_bytes());
    fixture(
        payload,
        1,
        WASM_V1,
        policy,
        1,
        batch_number,
        timestamp,
        false,
        false,
    )
}

pub fn upgrade_fixture(
    old_wasm: &[u8],
    new_wasm: &[u8],
    batch_number: u64,
    timestamp: u64,
) -> ProtocolFixture {
    let payload = upgrade_payload(old_wasm, new_wasm);
    fixture(
        payload,
        2,
        new_wasm,
        UpgradePolicy::Authority(AUTHORITY),
        2,
        batch_number,
        timestamp,
        false,
        false,
    )
}

pub fn deprecated_state(fixture: &ProtocolFixture, timestamp: u64) -> ProgramStateProof {
    state_fixture(
        &fixture.proof.activity,
        fixture.sequencer_id,
        fixture.batch_number + 1,
        timestamp,
        WASM_V1,
        UpgradePolicy::Authority(AUTHORITY),
        1,
        true,
        false,
    )
    .0
}

fn fixture(
    payload: Vec<u8>,
    ordinal: u16,
    wasm: &[u8],
    policy: UpgradePolicy,
    version: u32,
    batch_number: u64,
    timestamp: u64,
    deprecated: bool,
    wrong_batch_id: bool,
) -> ProtocolFixture {
    fixture_in_epoch(
        payload,
        ordinal,
        wasm,
        policy,
        version,
        batch_number,
        timestamp,
        deprecated,
        wrong_batch_id,
        2,
        [7; 32],
    )
}

fn fixture_in_epoch(
    payload: Vec<u8>,
    ordinal: u16,
    wasm: &[u8],
    policy: UpgradePolicy,
    version: u32,
    batch_number: u64,
    timestamp: u64,
    deprecated: bool,
    wrong_batch_id: bool,
    epoch: u64,
    signing_key: [u8; 32],
) -> ProtocolFixture {
    let activity = encode_activity(&payload, ordinal);
    let key = SigningKey::from_bytes(&signing_key);
    let sequencer_id = key.verifying_key().to_bytes();
    let (state, header_signature, interface_witness) = state_fixture_with_key(
        &activity,
        &key,
        batch_number,
        timestamp,
        wasm,
        policy,
        version,
        deprecated,
        wrong_batch_id,
        epoch,
    );
    let (activity_proof, _) = build_proof(&[activity.as_slice()], 0)
        .unwrap_or_else(|error| panic!("activity proof: {error:?}"));
    let mut state = state;
    state.header_signature = header_signature;
    ProtocolFixture {
        proof: DeploymentProof {
            activity,
            activity_proof,
            state,
        },
        interface_witness,
        sequencer_id,
        sequencer_public_key: key.verifying_key().to_bytes(),
        batch_number,
    }
}

fn state_fixture(
    activity: &[u8],
    sequencer_id: [u8; 32],
    batch_number: u64,
    timestamp: u64,
    wasm: &[u8],
    policy: UpgradePolicy,
    version: u32,
    deprecated: bool,
    wrong_batch_id: bool,
) -> (ProgramStateProof, [u8; 64]) {
    let key = SigningKey::from_bytes(&[7; 32]);
    assert_eq!(key.verifying_key().to_bytes(), sequencer_id);
    let (state, signature, _) = state_fixture_with_key(
        activity,
        &key,
        batch_number,
        timestamp,
        wasm,
        policy,
        version,
        deprecated,
        wrong_batch_id,
        2,
    );
    (state, signature)
}

fn state_fixture_with_key(
    activity: &[u8],
    key: &SigningKey,
    batch_number: u64,
    timestamp: u64,
    wasm: &[u8],
    policy: UpgradePolicy,
    version: u32,
    deprecated: bool,
    wrong_batch_id: bool,
    epoch: u64,
) -> (ProgramStateProof, [u8; 64], InterfaceStateWitness) {
    let program_key = program_key();
    let program_value = program_record(wasm, policy, version);
    let interface = fixture_interface(wasm);
    let interface_key = interface_state_key(program());
    let interface_value = interface_state_value(program(), version, &interface)
        .unwrap_or_else(|error| panic!("interface state value: {error}"));
    let mut leaves = vec![
        (program_key.clone(), program_value.clone()),
        (interface_key.clone(), interface_value.clone()),
    ];
    if deprecated {
        leaves.push((status_key(), status_record()));
    }
    leaves.sort_by(|left, right| left.0.cmp(&right.0));
    let leaf_hashes = leaves
        .iter()
        .map(|(key, value)| state_leaf_commitment(key, value))
        .collect::<Vec<_>>();
    let program_index = leaves
        .iter()
        .position(|(key, _)| key == &program_key)
        .unwrap_or_else(|| panic!("program leaf absent"));
    let (program_proof, programs_root) = build_state_proof(&leaf_hashes, program_index);
    let program_witness = StateLeafWitness {
        key: program_key,
        value: program_value,
        proof: program_proof,
    };
    let interface_index = leaves
        .iter()
        .position(|(key, _)| key == &interface_key)
        .unwrap_or_else(|| panic!("interface leaf absent"));
    let (interface_proof, interface_root) = build_state_proof(&leaf_hashes, interface_index);
    assert_eq!(interface_root, programs_root);
    let interface_witness = InterfaceStateWitness {
        key: interface_key,
        value: interface_value,
        proof: interface_proof,
    };
    let lifecycle = if deprecated {
        let status_index = leaves
            .iter()
            .position(|(key, _)| key == &status_key())
            .unwrap_or_else(|| panic!("status leaf absent"));
        let (status_proof, root) = build_state_proof(&leaf_hashes, status_index);
        assert_eq!(root, programs_root);
        ProgramLifecycleProof::Status(StateLeafWitness {
            key: leaves[status_index].0.clone(),
            value: leaves[status_index].1.clone(),
            proof: status_proof,
        })
    } else {
        ProgramLifecycleProof::Active {
            lower: Some(program_witness.clone()),
            upper: None,
        }
    };
    let state_root = programs_root_commitment(programs_root);
    let programs_root_proof = StateProof {
        leaf_index: 0,
        leaf_count: 1,
        siblings: Vec::new(),
    };
    let activity_value = canonical_activity_id(activity);
    let (activity_proof, activity_root) =
        build_proof(&[activity], 0).unwrap_or_else(|error| panic!("activity root: {error:?}"));
    assert_eq!(activity_proof.leaf_count(), 1);
    let mut batch_id = execution_batch_id([0x11; 32], activity_value, batch_number, batch_number)
        .unwrap_or_else(|error| panic!("batch id: {error:?}"));
    if wrong_batch_id {
        batch_id[0] ^= 1;
    }
    let unsigned_receipt = encode_program_receipt(
        activity_value,
        batch_id,
        batch_number,
        timestamp,
        state_root,
        activity_root,
        None,
    );
    let receipt_hash = receipt_digest(&unsigned_receipt)
        .unwrap_or_else(|error| panic!("receipt digest: {error:?}"));
    let receipt = encode_program_receipt(
        activity_value,
        batch_id,
        batch_number,
        timestamp,
        state_root,
        activity_root,
        Some(key.sign(&receipt_hash).to_bytes()),
    );
    let (receipt_proof, receipt_root) = build_proof(&[receipt.as_slice()], 0)
        .unwrap_or_else(|error| panic!("receipt root: {error:?}"));
    let header = encode_header(
        batch_number,
        timestamp,
        state_root,
        activity_root,
        receipt_root,
        key.verifying_key().to_bytes(),
        epoch,
    );
    let header_hash =
        batch_header_digest(&header).unwrap_or_else(|error| panic!("header digest: {error:?}"));
    let header_signature = key.sign(&header_hash).to_bytes();
    (
        ProgramStateProof {
            receipt,
            receipt_proof,
            header,
            header_signature,
            programs_root,
            programs_root_proof,
            program_record: program_witness,
            lifecycle,
        },
        header_signature,
        interface_witness,
    )
}

fn encode_activity(payload: &[u8], ordinal: u16) -> Vec<u8> {
    let mut encoder = Encoder::new(1_048_576);
    assert_eq!(encoder.structure_header(0x1001), Ok(()));
    assert_eq!(encoder.u8(12), Ok(()));
    assert_eq!(encoder.tag(1, 12), Ok(()));
    assert_eq!(encoder.u16(2), Ok(()));
    assert_eq!(encoder.tag(2, 12), Ok(()));
    assert_eq!(encoder.u32(42), Ok(()));
    assert_eq!(encoder.tag(3, 12), Ok(()));
    assert_eq!(encoder.u32((9_u32 << 16) | u32::from(ordinal)), Ok(()));
    assert_eq!(encoder.tag(4, 12), Ok(()));
    assert_eq!(encoder.bytes(b"did:layerx:test", 255), Ok(()));
    assert_eq!(encoder.tag(5, 12), Ok(()));
    assert_eq!(encoder.bytes(&[0x44; 32], 524_288), Ok(()));
    assert_eq!(encoder.tag(6, 12), Ok(()));
    assert_eq!(encoder.u64(9), Ok(()));
    assert_eq!(encoder.tag(7, 12), Ok(()));
    assert_eq!(encoder.u64(1_700_000_000), Ok(()));
    assert_eq!(encoder.u64(1_800_000_000), Ok(()));
    assert_eq!(encoder.tag(8, 12), Ok(()));
    assert_eq!(encoder.bytes(&[0x45; 32], 32), Ok(()));
    assert_eq!(encoder.tag(9, 12), Ok(()));
    assert_eq!(encoder.u128(100), Ok(()));
    assert_eq!(encoder.tag(10, 12), Ok(()));
    assert_eq!(encoder.bytes(&payload_hash(payload), 32), Ok(()));
    assert_eq!(encoder.tag(11, 12), Ok(()));
    assert_eq!(encoder.bytes(payload, 524_288), Ok(()));
    assert_eq!(encoder.tag(12, 12), Ok(()));
    assert_eq!(encoder.bytes(&[0x46; 64], 128), Ok(()));
    encoder.finish()
}

fn canonical_activity_id(bytes: &[u8]) -> [u8; 32] {
    let deploy = ActivityType::new(ModuleId::Programs, 1)
        .unwrap_or_else(|error| panic!("deploy type: {error:?}"));
    let upgrade = ActivityType::new(ModuleId::Programs, 2)
        .unwrap_or_else(|error| panic!("upgrade type: {error:?}"));
    let call = ActivityType::new(ModuleId::Programs, 3)
        .unwrap_or_else(|error| panic!("call type: {error:?}"));
    let registration = ModuleRegistration::new(ModuleId::Programs, &[deploy, upgrade, call])
        .unwrap_or_else(|error| panic!("program registration: {error:?}"));
    let registry = ModuleRegistry::new(&[registration])
        .unwrap_or_else(|error| panic!("module registry: {error:?}"));
    let activity = decode_signed(bytes, &registry)
        .unwrap_or_else(|error| panic!("canonical activity: {error:?}"));
    activity_id(&activity).unwrap_or_else(|error| panic!("activity id: {error:?}"))
}

fn encode_program_receipt(
    activity_id: [u8; 32],
    batch_id: [u8; 32],
    batch_number: u64,
    timestamp: u64,
    resulting_state_root: [u8; 32],
    activity_root: [u8; 32],
    signature: Option<[u8; 64]>,
) -> Vec<u8> {
    let mut encoder = Encoder::new(4_096);
    assert_eq!(encoder.structure_header_version(0x5201, 2), Ok(()));
    assert_eq!(encoder.u16(2), Ok(()));
    assert_eq!(encoder.bytes(&activity_id, 32), Ok(()));
    assert_eq!(encoder.u64(batch_number), Ok(()));
    assert_eq!(encoder.bytes(&[0x11; 32], 32), Ok(()));
    assert_eq!(encoder.bytes(&resulting_state_root, 32), Ok(()));
    assert_eq!(encoder.bytes(&activity_root, 32), Ok(()));
    assert_eq!(encoder.i32(0), Ok(()));
    assert_eq!(encoder.sequence_length(0, 512), Ok(()));
    assert_eq!(encoder.u128(0), Ok(()));
    assert_eq!(encoder.bytes(&batch_id, 32), Ok(()));
    assert_eq!(encoder.u16(9), Ok(()));
    assert_eq!(encoder.u32(2), Ok(()));
    assert_eq!(encoder.u32(0), Ok(()));
    assert_eq!(encoder.u8(0), Ok(()));
    assert_eq!(encoder.bytes(&[0; 32], 32), Ok(()));
    assert_eq!(encoder.u128(0), Ok(()));
    assert_eq!(encoder.bytes(&[0; 32], 32), Ok(()));
    assert_eq!(encoder.u128(0), Ok(()));
    assert_eq!(encoder.u128(0), Ok(()));
    assert_eq!(encoder.u64(0), Ok(()));
    assert_eq!(encoder.bytes(&[0; 32], 32), Ok(()));
    assert_eq!(encoder.u128(0), Ok(()));
    assert_eq!(encoder.u128(0), Ok(()));
    assert_eq!(encoder.bytes(&[0; 32], 32), Ok(()));
    assert_eq!(encoder.bytes(&[0x13; 32], 32), Ok(()));
    assert_eq!(encoder.bytes(&[0x14; 32], 32), Ok(()));
    assert_eq!(encoder.u64(timestamp), Ok(()));
    assert_eq!(encoder.u8(u8::from(signature.is_some())), Ok(()));
    if let Some(signature) = signature {
        assert_eq!(encoder.bytes(&signature, 64), Ok(()));
    }
    encoder.finish()
}

fn encode_header(
    batch_number: u64,
    timestamp: u64,
    resulting_state_root: [u8; 32],
    activity_root: [u8; 32],
    receipt_root: [u8; 32],
    sequencer_id: [u8; 32],
    epoch: u64,
) -> Vec<u8> {
    let mut encoder = Encoder::new(354);
    assert_eq!(encoder.structure_header_version(0x1701, 2), Ok(()));
    assert_eq!(encoder.u8(15), Ok(()));
    assert_eq!(encoder.tag(1, 15), Ok(()));
    assert_eq!(encoder.u16(2), Ok(()));
    assert_eq!(encoder.tag(2, 15), Ok(()));
    assert_eq!(encoder.u32(42), Ok(()));
    assert_eq!(encoder.tag(3, 15), Ok(()));
    assert_eq!(encoder.u64(epoch), Ok(()));
    assert_eq!(encoder.tag(4, 15), Ok(()));
    assert_eq!(encoder.u64(batch_number), Ok(()));
    assert_eq!(encoder.tag(5, 15), Ok(()));
    assert_eq!(encoder.u64(batch_number), Ok(()));
    assert_eq!(encoder.tag(6, 15), Ok(()));
    assert_eq!(encoder.u64(batch_number), Ok(()));
    assert_eq!(encoder.tag(7, 15), Ok(()));
    assert_eq!(encoder.bytes(&[0x11; 32], 32), Ok(()));
    assert_eq!(encoder.tag(8, 15), Ok(()));
    assert_eq!(encoder.bytes(&resulting_state_root, 32), Ok(()));
    assert_eq!(encoder.tag(9, 15), Ok(()));
    assert_eq!(encoder.bytes(&activity_root, 32), Ok(()));
    assert_eq!(encoder.tag(10, 15), Ok(()));
    assert_eq!(encoder.bytes(&receipt_root, 32), Ok(()));
    assert_eq!(encoder.tag(11, 15), Ok(()));
    assert_eq!(encoder.bytes(&[0x15; 32], 32), Ok(()));
    assert_eq!(encoder.tag(12, 15), Ok(()));
    assert_eq!(encoder.bytes(&[0x16; 32], 32), Ok(()));
    assert_eq!(encoder.tag(13, 15), Ok(()));
    assert_eq!(encoder.bytes(&[0x17; 32], 32), Ok(()));
    assert_eq!(encoder.tag(14, 15), Ok(()));
    assert_eq!(encoder.u64(timestamp), Ok(()));
    assert_eq!(encoder.tag(15, 15), Ok(()));
    assert_eq!(encoder.bytes(&sequencer_id, 32), Ok(()));
    encoder.finish()
}

fn deploy_payload(wasm: &[u8], policy: UpgradePolicy) -> Vec<u8> {
    let interface_encoding = fixture_interface_encoding(wasm);
    let mut payload = Vec::new();
    payload.extend_from_slice(&PROGRAM_BYTES);
    payload.extend_from_slice(&1_u16.to_be_bytes());
    match policy {
        UpgradePolicy::Immutable => {
            payload.extend([0, 0]);
            payload.extend_from_slice(&[0; 32]);
        }
        UpgradePolicy::Authority(authority) => {
            payload.extend([1, 0]);
            payload.extend_from_slice(&authority);
        }
    }
    payload.extend_from_slice(&code_hash(wasm));
    payload.extend_from_slice(
        &u32::try_from(wasm.len())
            .unwrap_or_else(|_| panic!("WASM length"))
            .to_be_bytes(),
    );
    payload.extend_from_slice(
        &u32::try_from(interface_encoding.len())
            .unwrap_or_else(|_| panic!("interface length"))
            .to_be_bytes(),
    );
    payload.extend_from_slice(&interface_encoding);
    payload.extend_from_slice(wasm);
    payload
}

fn upgrade_payload(old_wasm: &[u8], new_wasm: &[u8]) -> Vec<u8> {
    let interface_encoding = fixture_interface_encoding(new_wasm);
    let mut payload = Vec::new();
    payload.extend_from_slice(&PROGRAM_BYTES);
    payload.extend_from_slice(&1_u16.to_be_bytes());
    payload.extend([0, 0]);
    payload.extend_from_slice(&code_hash(old_wasm));
    payload.extend_from_slice(&code_hash(new_wasm));
    payload.extend_from_slice(&0_u16.to_be_bytes());
    payload.extend_from_slice(
        &u32::try_from(new_wasm.len())
            .unwrap_or_else(|_| panic!("WASM length"))
            .to_be_bytes(),
    );
    payload.extend_from_slice(
        &u32::try_from(interface_encoding.len())
            .unwrap_or_else(|_| panic!("interface length"))
            .to_be_bytes(),
    );
    payload.extend_from_slice(&interface_encoding);
    payload.extend_from_slice(new_wasm);
    payload
}

pub fn fixture_interface(wasm: &[u8]) -> ProgramInterface {
    let entries = || {
        vec![InterfaceEntryPoint {
            name: "call".to_owned(),
            discriminator: [0x10, 0x20, 0x30, 0x40],
            calldata: ValueSchema::layerx(ValueType::Bytes { max_len: 64 }),
            response: ValueSchema::layerx(ValueType::Bytes { max_len: 64 }),
            capabilities: Vec::new(),
            event_topics: vec![[0x44; 32]],
            failures: Vec::new(),
        }]
    };
    ProgramInterface::bind(wasm, 1, entries()).unwrap_or_else(|_| {
        ProgramInterface::bind(WASM_V1, 1, entries())
            .unwrap_or_else(|error| panic!("fallback program interface: {error}"))
    })
}

fn fixture_interface_encoding(wasm: &[u8]) -> Vec<u8> {
    if let Ok(interface) =
        ProgramInterface::bind(wasm, 1, fixture_interface(WASM_V1).entries().to_vec())
    {
        return interface.canonical_encoding().to_vec();
    }
    let mut encoding = fixture_interface(WASM_V1).canonical_encoding().to_vec();
    encoding[28..60].copy_from_slice(&code_hash(wasm));
    encoding
}

fn program_key() -> Vec<u8> {
    let mut key = b"program\0".to_vec();
    key.extend_from_slice(&PROGRAM_BYTES);
    key
}

fn status_key() -> Vec<u8> {
    let mut key = b"wind-down\0s".to_vec();
    key.extend_from_slice(&PROGRAM_BYTES);
    key
}

fn program_record(wasm: &[u8], policy: UpgradePolicy, version: u32) -> Vec<u8> {
    let mut record = Vec::with_capacity(71);
    match policy {
        UpgradePolicy::Immutable => {
            record.push(0);
            record.extend_from_slice(&[0; 32]);
        }
        UpgradePolicy::Authority(authority) => {
            record.push(1);
            record.extend_from_slice(&authority);
        }
    }
    record.extend_from_slice(&code_hash(wasm));
    record.extend_from_slice(&1_u16.to_be_bytes());
    record.extend_from_slice(&version.to_be_bytes());
    record
}

fn status_record() -> Vec<u8> {
    let mut record = vec![0; 54];
    record[0] = 1;
    record[1] = 2;
    record[2..34].copy_from_slice(&PROGRAM_BYTES);
    record[34..42].copy_from_slice(&1_800_000_000_u64.to_be_bytes());
    record[42..50].copy_from_slice(&72_u64.to_be_bytes());
    record
}

fn build_state_proof(leaf_hashes: &[[u8; 32]], leaf_index: usize) -> (StateProof, [u8; 32]) {
    assert!(!leaf_hashes.is_empty(), "state tree must not be empty");
    assert!(
        leaf_index < leaf_hashes.len(),
        "state leaf index is out of range"
    );
    let leaf_count =
        u32::try_from(leaf_hashes.len()).unwrap_or_else(|_| panic!("state tree too large"));
    let proof_index = leaf_index;
    let mut level = leaf_hashes.to_vec();
    let mut index = leaf_index;
    let mut siblings = Vec::new();
    while level.len() > 1 {
        siblings.push(level.get(index ^ 1).copied().unwrap_or(level[index]));
        level = level
            .chunks(2)
            .map(|pair| state_node_commitment(pair[0], *pair.get(1).unwrap_or(&pair[0])))
            .collect();
        index /= 2;
    }
    (
        StateProof {
            leaf_index: u32::try_from(proof_index)
                .unwrap_or_else(|_| panic!("state leaf index too large")),
            leaf_count,
            siblings,
        },
        level[0],
    )
}

fn payload_hash(payload: &[u8]) -> [u8; 32] {
    use sha2::{Digest as _, Sha256};
    let mut hasher = Sha256::new();
    hasher.update(b"LXP/v1/payload-hash\0");
    hasher.update(payload);
    hasher.finalize().into()
}
