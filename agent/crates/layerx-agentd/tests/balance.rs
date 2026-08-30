use std::fs;
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::thread;
use std::time::Duration;

use ed25519_dalek::{Signer as _, SigningKey};
use layerx_agentd::read::balance;
use layerx_client::evidence::RootSelector;
use layerx_client::head::Head;
use layerx_client::lni::framing::{read_frame, write_frame};
use layerx_client::lni::schema::{decode_envelope, encode_envelope, Envelope, Version};
use layerx_client::lni::transport::{ConnectionGate, Limits, Uds};
use layerx_client::read::{ReadContext, ReadError, Requested};
use layerx_proof::inclusion::SequencerAuthorization;
use layerx_proof::merkle::{build_proof, Proof};
use layerx_types::verify::VerificationLevel;
use layerx_wire::encode::Encoder;
use layerx_wire::hash::{batch_header_digest, receipt_digest};
use sha2::{Digest as _, Sha256};

static NEXT_SOCKET: AtomicU64 = AtomicU64::new(1);

struct SocketPath(PathBuf);

impl SocketPath {
    fn new(label: &str) -> Self {
        let sequence = NEXT_SOCKET.fetch_add(1, Ordering::Relaxed);
        Self(std::env::temp_dir().join(format!(
            "layerx-agentd-balance-{label}-{}-{sequence}.sock",
            std::process::id()
        )))
    }
}

impl Drop for SocketPath {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.0);
    }
}

fn limits() -> Limits {
    Limits {
        maximum_frame_bytes: 1024 * 1024,
        maximum_connections: 1,
        maximum_streams: 4,
        maximum_queued_bytes: 2 * 1024 * 1024,
        deadline: Duration::from_secs(2),
    }
}

fn header_bytes(state_root: [u8; 32], receipt_root: [u8; 32], sequencer_id: [u8; 32]) -> Vec<u8> {
    let mut encoder = Encoder::new(354);
    assert_eq!(encoder.structure_header(0x1701), Ok(()));
    assert_eq!(encoder.u8(15), Ok(()));
    for field in 1..=15 {
        assert_eq!(encoder.tag(field, 15), Ok(()));
        match field {
            1 => assert_eq!(encoder.u16(1), Ok(())),
            2 => assert_eq!(encoder.u32(77), Ok(())),
            3 => assert_eq!(encoder.u64(2), Ok(())),
            4 => assert_eq!(encoder.u64(7), Ok(())),
            5 => assert_eq!(encoder.u64(10), Ok(())),
            6 => assert_eq!(encoder.u64(12), Ok(())),
            7 => assert_eq!(encoder.bytes(&[1; 32], 32), Ok(())),
            8 => assert_eq!(encoder.bytes(&state_root, 32), Ok(())),
            9 => assert_eq!(encoder.bytes(&[0x51; 32], 32), Ok(())),
            10 => assert_eq!(encoder.bytes(&receipt_root, 32), Ok(())),
            11 => assert_eq!(encoder.bytes(&[3; 32], 32), Ok(())),
            12 => assert_eq!(encoder.bytes(&[4; 32], 32), Ok(())),
            13 => assert_eq!(encoder.bytes(&[5; 32], 32), Ok(())),
            14 => assert_eq!(encoder.u64(1_000), Ok(())),
            15 => assert_eq!(encoder.bytes(&sequencer_id, 32), Ok(())),
            _ => panic!("unreachable header field"),
        }
    }
    encoder.finish()
}

fn encode_path(bytes: &mut Vec<u8>, proof: &Proof) {
    bytes.extend_from_slice(&proof.leaf_index().to_be_bytes());
    bytes.extend_from_slice(&proof.leaf_count().to_be_bytes());
    bytes.push(
        u8::try_from(proof.siblings().len())
            .unwrap_or_else(|error| panic!("proof length: {error}")),
    );
    for sibling in proof.siblings() {
        bytes.extend_from_slice(sibling);
    }
}

fn state_leaf(key: &[u8], value: &[u8]) -> [u8; 32] {
    let mut bytes = Vec::new();
    bytes.extend_from_slice(b"LXP/v1/state-leaf\0");
    bytes.extend_from_slice(
        &u32::try_from(key.len())
            .unwrap_or_else(|_| panic!("state key length"))
            .to_be_bytes(),
    );
    bytes.extend_from_slice(
        &u32::try_from(value.len())
            .unwrap_or_else(|_| panic!("state value length"))
            .to_be_bytes(),
    );
    bytes.extend_from_slice(key);
    bytes.extend_from_slice(value);
    Sha256::digest(bytes).into()
}

fn state_node(left: [u8; 32], right: [u8; 32]) -> [u8; 32] {
    let mut bytes = Vec::with_capacity(82);
    bytes.extend_from_slice(b"LXP/v1/state-node\0");
    bytes.extend_from_slice(&left);
    bytes.extend_from_slice(&right);
    Sha256::digest(bytes).into()
}

fn account_value(account: [u8; 32], asset: [u8; 32]) -> Vec<u8> {
    let name = format!("module:programs:value:{}", "11".repeat(32));
    let mut value = Vec::new();
    value.extend_from_slice(
        &u16::try_from(name.len())
            .unwrap_or_else(|_| panic!("account name length"))
            .to_be_bytes(),
    );
    value.extend_from_slice(name.as_bytes());
    value.push(13);
    value.extend_from_slice(&0x12_34_56_u128.to_be_bytes());
    value.extend_from_slice(&asset);
    value.push(1);
    value.extend_from_slice(&7_u64.to_be_bytes());
    value.extend_from_slice(&3_u64.to_be_bytes());
    value.extend_from_slice(&[0, 1]);
    value.extend_from_slice(&[0; 32]);
    value.push(0);
    assert_eq!(account, [0x11; 32]);
    value
}

fn receipt_bytes(
    activity_id: [u8; 32],
    resulting_state_root: [u8; 32],
    signature: Option<[u8; 64]>,
) -> Vec<u8> {
    let mut encoder = Encoder::new(4_096);
    assert_eq!(encoder.structure_header(0x5201), Ok(()));
    assert_eq!(encoder.u16(1), Ok(()));
    assert_eq!(encoder.bytes(&activity_id, 32), Ok(()));
    assert_eq!(encoder.u64(12), Ok(()));
    assert_eq!(encoder.bytes(&[0x21; 32], 32), Ok(()));
    assert_eq!(encoder.bytes(&resulting_state_root, 32), Ok(()));
    assert_eq!(encoder.bytes(&[0x22; 32], 32), Ok(()));
    assert_eq!(encoder.i32(0), Ok(()));
    assert_eq!(encoder.sequence_length(0, 512), Ok(()));
    assert_eq!(encoder.u128(1), Ok(()));
    assert_eq!(encoder.bytes(&[0x23; 32], 32), Ok(()));
    assert_eq!(encoder.u16(1), Ok(()));
    assert_eq!(encoder.u32(1), Ok(()));
    assert_eq!(encoder.u32(1), Ok(()));
    assert_eq!(encoder.u8(1), Ok(()));
    assert_eq!(encoder.bytes(&[0x24; 32], 32), Ok(()));
    assert_eq!(encoder.u128(25), Ok(()));
    assert_eq!(encoder.bytes(&[0x25; 32], 32), Ok(()));
    assert_eq!(encoder.u128(100), Ok(()));
    assert_eq!(encoder.u128(75), Ok(()));
    assert_eq!(encoder.u64(1), Ok(()));
    assert_eq!(encoder.bytes(&[0x26; 32], 32), Ok(()));
    assert_eq!(encoder.u128(10), Ok(()));
    assert_eq!(encoder.u128(35), Ok(()));
    assert_eq!(encoder.bytes(&[0x27; 32], 32), Ok(()));
    assert_eq!(encoder.bytes(&[0x28; 32], 32), Ok(()));
    assert_eq!(encoder.bytes(&[0x29; 32], 32), Ok(()));
    assert_eq!(encoder.u64(1_000), Ok(()));
    assert_eq!(encoder.u8(u8::from(signature.is_some())), Ok(()));
    if let Some(signature) = signature {
        assert_eq!(encoder.bytes(&signature, 64), Ok(()));
    }
    encoder.finish()
}

fn fixture() -> (Vec<u8>, Vec<u8>, SequencerAuthorization, [u8; 32], [u8; 32]) {
    let account = [0x11; 32];
    let asset = [0x22; 32];
    let value = account_value(account, asset);
    let mut account_key = [0_u8; 33];
    account_key[0] = 4;
    account_key[1..].copy_from_slice(&account);
    let account_leaf = state_leaf(&account_key, &value);
    let mut other_account_key = [0xff; 33];
    other_account_key[0] = 4;
    let other_account_leaf = state_leaf(&other_account_key, b"other-account");
    let account_root = state_node(account_leaf, other_account_leaf);
    let account_proof = Proof::new(0, 2, vec![other_account_leaf])
        .unwrap_or_else(|error| panic!("account proof: {error:?}"));

    let account_tree_leaf = state_leaf(b"account-tree", &account_root);
    let sequence_leaf = state_leaf(b"sequence", &13_u64.to_be_bytes());
    let universal_root = state_node(account_tree_leaf, sequence_leaf);
    let account_tree_proof = Proof::new(0, 2, vec![sequence_leaf])
        .unwrap_or_else(|error| panic!("account-tree proof: {error:?}"));

    let universal_leaf = state_leaf(&0_u16.to_be_bytes(), &universal_root);
    let module_leaf = state_leaf(&1_u16.to_be_bytes(), &[0x61; 32]);
    let resulting_state_root = state_node(universal_leaf, module_leaf);
    let universal_root_proof = Proof::new(0, 2, vec![module_leaf])
        .unwrap_or_else(|error| panic!("universal proof: {error:?}"));

    let key = SigningKey::from_bytes(&[0x41; 32]);
    let sequencer_id = key.verifying_key().to_bytes();
    let unsigned_receipt = receipt_bytes([0x71; 32], resulting_state_root, None);
    let receipt_digest = receipt_digest(&unsigned_receipt)
        .unwrap_or_else(|error| panic!("receipt digest: {error:?}"));
    let receipt = receipt_bytes(
        [0x71; 32],
        resulting_state_root,
        Some(key.sign(&receipt_digest).to_bytes()),
    );
    let (receipt_proof, receipt_root) = build_proof(&[receipt.as_slice()], 0)
        .unwrap_or_else(|error| panic!("receipt proof: {error:?}"));
    let header = header_bytes(resulting_state_root, receipt_root, sequencer_id);
    let digest =
        batch_header_digest(&header).unwrap_or_else(|error| panic!("header digest: {error:?}"));
    let signature = key.sign(&digest).to_bytes();

    let mut proof = Vec::new();
    proof.extend_from_slice(&1_u16.to_be_bytes());
    proof.push(2);
    proof.push(1);
    proof.extend_from_slice(&account);
    proof.extend_from_slice(&account_root);
    proof.extend_from_slice(&universal_root);
    proof.extend_from_slice(&resulting_state_root);
    encode_path(&mut proof, &account_proof);
    encode_path(&mut proof, &account_tree_proof);
    encode_path(&mut proof, &universal_root_proof);
    proof.extend_from_slice(
        &u32::try_from(receipt.len())
            .unwrap_or_else(|_| panic!("receipt length"))
            .to_be_bytes(),
    );
    proof.extend_from_slice(&receipt);
    encode_path(&mut proof, &receipt_proof);
    proof.extend_from_slice(&1_u16.to_be_bytes());
    proof.extend_from_slice(&sequencer_id);
    proof.extend_from_slice(&sequencer_id);
    proof.extend_from_slice(&7_u64.to_be_bytes());
    proof.extend_from_slice(&7_u64.to_be_bytes());
    proof.extend_from_slice(
        &u32::try_from(header.len())
            .unwrap_or_else(|_| panic!("header length"))
            .to_be_bytes(),
    );
    proof.extend_from_slice(&header);
    proof.extend_from_slice(&signature);
    proof.push(0);
    (
        value,
        proof,
        SequencerAuthorization::new(sequencer_id, sequencer_id, 7, 7),
        account,
        asset,
    )
}

fn context(requested: VerificationLevel, authorization: SequencerAuthorization) -> ReadContext {
    ReadContext {
        interface_version: Version::V1_2,
        correlation_id: 44,
        expected_protocol_version: 1,
        expected_network_id: 77,
        requested: Requested::new(requested),
        head: Head {
            chain_sequence: 20,
            sealed_batch: 7,
            finalised_checkpoint: [0x71; 32],
        },
        handshake_sequencer_key: authorization.public_key(),
        sequencer_authorization: authorization,
        root_selector: RootSelector::Latest,
    }
}

fn request(stream: &mut UnixStream) -> u64 {
    let frame =
        read_frame(stream, 1024 * 1024).unwrap_or_else(|error| panic!("request frame: {error:?}"));
    let envelope =
        decode_envelope(&frame).unwrap_or_else(|error| panic!("request envelope: {error:?}"));
    assert_eq!(envelope.message_tag, 7);
    envelope.correlation_id
}

fn respond(stream: &mut UnixStream, correlation_id: u64, payload: &[u8], proof: &[u8]) {
    let response = encode_envelope(Envelope {
        version: Version::V1_0,
        message_tag: 8,
        correlation_id,
        canonical_payload: payload,
        proof_material: proof,
    })
    .unwrap_or_else(|error| panic!("response envelope: {error:?}"));
    write_frame(stream, &response, 1024 * 1024)
        .unwrap_or_else(|error| panic!("response frame: {error:?}"));
}

#[test]
fn proven_balance_carries_identity_bytes_level_and_all_freshness_coordinates() {
    let socket = SocketPath::new("proven");
    let listener =
        UnixListener::bind(&socket.0).unwrap_or_else(|error| panic!("listener: {error}"));
    let (leaf, proof, authorization, account, asset) = fixture();
    let server_leaf = leaf.clone();
    let server = thread::spawn(move || {
        let (mut stream, _) = listener
            .accept()
            .unwrap_or_else(|error| panic!("accept: {error}"));
        let correlation = request(&mut stream);
        respond(&mut stream, correlation, &server_leaf, &proof);
    });
    let gate = ConnectionGate::new(1);
    let mut transport = Uds::connect(&socket.0, &gate, limits())
        .unwrap_or_else(|error| panic!("connect: {error:?}"));
    let value = balance(
        &mut transport,
        account,
        asset,
        context(VerificationLevel::STATE_PROVEN, authorization),
    )
    .unwrap_or_else(|error| panic!("balance: {error:?}"));
    assert_eq!(value.account, account);
    assert_eq!(value.asset, asset);
    assert_eq!(value.amount.value(), 0x12_34_56);
    assert_eq!(value.canonical_bytes, leaf);
    assert_eq!(value.achieved, VerificationLevel::STATE_PROVEN);
    assert_eq!(value.freshness.value_global_sequence, 12);
    assert_eq!(value.freshness.value_batch_number, 7);
    assert_eq!(value.freshness.observed_head_sequence, 20);
    assert_eq!(value.freshness.latest_sealed_batch, 7);
    assert_eq!(value.freshness.latest_finalised_checkpoint, [0x71; 32]);
    assert!(server.join().is_ok(), "node thread failed");
}

#[test]
fn hostile_asserted_level_cannot_raise_evidence_and_missing_proof_is_named() {
    let socket = SocketPath::new("hostile");
    let listener =
        UnixListener::bind(&socket.0).unwrap_or_else(|error| panic!("listener: {error}"));
    let (leaf, hostile_proof, authorization, account, asset) = fixture();
    let server = thread::spawn(move || {
        for proof in [&hostile_proof[..], &[][..]] {
            let (mut stream, _) = listener
                .accept()
                .unwrap_or_else(|error| panic!("accept: {error}"));
            let correlation = request(&mut stream);
            respond(&mut stream, correlation, &leaf, proof);
        }
    });
    let gate = ConnectionGate::new(1);
    {
        let mut transport = Uds::connect(&socket.0, &gate, limits())
            .unwrap_or_else(|error| panic!("connect hostile: {error:?}"));
        assert_eq!(
            balance(
                &mut transport,
                account,
                asset,
                context(VerificationLevel::CHECKPOINT_FINALISED, authorization),
            ),
            Err(ReadError::MissingEvidence {
                requested: VerificationLevel::CHECKPOINT_FINALISED,
                achieved: VerificationLevel::STATE_PROVEN,
            })
        );
    }
    {
        let mut transport = Uds::connect(&socket.0, &gate, limits())
            .unwrap_or_else(|error| panic!("connect missing: {error:?}"));
        assert_eq!(
            balance(
                &mut transport,
                account,
                asset,
                context(VerificationLevel::STATE_PROVEN, authorization),
            ),
            Err(ReadError::MissingEvidence {
                requested: VerificationLevel::STATE_PROVEN,
                achieved: VerificationLevel::UNVERIFIED,
            })
        );
    }
    assert!(server.join().is_ok(), "node thread failed");
}
