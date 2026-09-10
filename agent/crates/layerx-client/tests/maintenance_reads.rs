use std::collections::BTreeMap;
use std::fs;
use std::os::unix::net::UnixListener;
use std::sync::atomic::{AtomicU64, Ordering};
use std::thread;
use std::time::Duration;

use ed25519_dalek::{Signer as _, SigningKey};
use layerx_client::evidence::{
    verification_label, verify_account_evidence, AccountEvidencePolicy, EvidenceError, RootSelector,
};
use layerx_client::head::Head;
use layerx_client::lni::framing::{read_frame, write_frame};
use layerx_client::lni::schema::{decode_envelope, encode_envelope, Envelope, Version};
use layerx_client::lni::transport::{ConnectionGate, Limits, Uds};
use layerx_client::read::{account, ReadContext, ReadError, ReadValue, Requested};
use layerx_proof::inclusion::{InclusionError, SequencerAuthorization};
use layerx_proof::merkle::{build_proof, MerkleError, Proof};
use layerx_proof::state::{AccountProofError, NestedAccountProof};
use layerx_types::verify::VerificationLevel;
use layerx_wire::encode::Encoder;
use layerx_wire::hash::{batch_header_digest, receipt_digest};
use layerx_wire::limits::PROTOCOL_VERSION;
use sha2::{Digest as _, Sha256};

const PROGRAM_ACCOUNT_VECTORS: &str =
    include_str!("../../../../tests/vectors/program_account_state_v2.vec");

fn vectors(source: &str) -> BTreeMap<&str, &str> {
    source
        .lines()
        .filter_map(|line| line.split_once('='))
        .collect()
}

fn nibble(value: u8) -> Option<u8> {
    match value {
        b'0'..=b'9' => Some(value - b'0'),
        b'a'..=b'f' => Some(value - b'a' + 10),
        _ => None,
    }
}

fn hex(value: &str) -> Vec<u8> {
    assert_eq!(value.len() % 2, 0, "odd-length vector value");
    value
        .as_bytes()
        .chunks_exact(2)
        .map(|pair| {
            let high = nibble(pair[0]).unwrap_or_else(|| panic!("invalid vector hex"));
            let low = nibble(pair[1]).unwrap_or_else(|| panic!("invalid vector hex"));
            (high << 4) | low
        })
        .collect()
}

fn fixed<const LENGTH: usize>(value: &str) -> [u8; LENGTH] {
    hex(value)
        .try_into()
        .unwrap_or_else(|_| panic!("vector has wrong fixed length"))
}

fn state_leaf(key: &[u8], value: &[u8]) -> [u8; 32] {
    let mut bytes = Vec::with_capacity(18 + 8 + key.len() + value.len());
    bytes.extend_from_slice(b"LXP/v1/state-leaf\0");
    bytes.extend_from_slice(
        &u32::try_from(key.len())
            .unwrap_or_else(|_| panic!("test key length"))
            .to_be_bytes(),
    );
    bytes.extend_from_slice(
        &u32::try_from(value.len())
            .unwrap_or_else(|_| panic!("test value length"))
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

fn receipt_bytes(
    activity_id: [u8; 32],
    resulting_state_root: [u8; 32],
    signature_key: Option<&SigningKey>,
) -> Vec<u8> {
    let encode = |signature: Option<[u8; 64]>| {
        let mut encoder = Encoder::new(4096);
        assert_eq!(
            encoder.structure_header_version(0x5201, PROTOCOL_VERSION),
            Ok(())
        );
        assert_eq!(encoder.u16(PROTOCOL_VERSION), Ok(()));
        assert_eq!(encoder.bytes(&activity_id, 32), Ok(()));
        assert_eq!(encoder.u64(10), Ok(()));
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
    };
    let unsigned = encode(None);
    let Some(signature_key) = signature_key else {
        return unsigned;
    };
    let digest = receipt_digest(&unsigned)
        .unwrap_or_else(|error| panic!("receipt digest failed: {error:?}"));
    let signature = signature_key.sign(&digest).to_bytes();
    encode(Some(signature))
}

fn header_bytes(
    resulting_state_root: [u8; 32],
    receipt_root: [u8; 32],
    sequencer_id: [u8; 32],
) -> Vec<u8> {
    let mut encoder = Encoder::new(354);
    assert_eq!(
        encoder.structure_header_version(0x1701, PROTOCOL_VERSION),
        Ok(())
    );
    assert_eq!(encoder.u8(15), Ok(()));
    let fields: [(u8, Vec<u8>); 15] = [
        (1, PROTOCOL_VERSION.to_be_bytes().to_vec()),
        (2, 42_u32.to_be_bytes().to_vec()),
        (3, 2_u64.to_be_bytes().to_vec()),
        (4, 7_u64.to_be_bytes().to_vec()),
        (5, 9_u64.to_be_bytes().to_vec()),
        (6, 10_u64.to_be_bytes().to_vec()),
        (7, [0x31; 32].to_vec()),
        (8, resulting_state_root.to_vec()),
        (9, [0x32; 32].to_vec()),
        (10, receipt_root.to_vec()),
        (11, [0x33; 32].to_vec()),
        (12, [0x34; 32].to_vec()),
        (13, [0x35; 32].to_vec()),
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
                encoder.u32(u32::from_be_bytes([value[0], value[1], value[2], value[3]])),
                Ok(())
            ),
            3..=6 | 14 => assert_eq!(
                encoder.u64(u64::from_be_bytes(
                    value
                        .as_slice()
                        .try_into()
                        .unwrap_or_else(|_| panic!("invalid u64 test field")),
                )),
                Ok(())
            ),
            _ => assert_eq!(encoder.bytes(&value, 32), Ok(())),
        }
    }
    let bytes = encoder.finish();
    assert_eq!(bytes.len(), 354);
    bytes
}

fn nested_fixture(
    account_id: [u8; 32],
    account_value: &[u8],
    receipt_signature_key: Option<&SigningKey>,
) -> (NestedAccountProof, SequencerAuthorization, [u8; 32]) {
    let sequencer = SigningKey::from_bytes(&[0x51; 32]);
    let sequencer_id = sequencer.verifying_key().to_bytes();
    let mut account_key = [0_u8; 33];
    account_key[0] = 4;
    account_key[1..].copy_from_slice(&account_id);
    let account_leaf = state_leaf(&account_key, account_value);
    let mut other_account_key = [0xff_u8; 33];
    other_account_key[0] = 4;
    assert!(account_key < other_account_key);
    let other_account_leaf = state_leaf(&other_account_key, b"other-account");
    let account_root = state_node(account_leaf, other_account_leaf);
    let account_proof = Proof::new(0, 2, vec![other_account_leaf])
        .unwrap_or_else(|error| panic!("account proof: {error:?}"));

    let account_tree_leaf = state_leaf(b"account-tree", &account_root);
    let sequence_leaf = state_leaf(b"sequence", &11_u64.to_be_bytes());
    let universal_root = state_node(account_tree_leaf, sequence_leaf);
    let account_tree_proof = Proof::new(0, 2, vec![sequence_leaf])
        .unwrap_or_else(|error| panic!("account-tree proof: {error:?}"));

    let universal_leaf = state_leaf(&0_u16.to_be_bytes(), &universal_root);
    let module_leaf = state_leaf(&1_u16.to_be_bytes(), &[0x61; 32]);
    let resulting_state_root = state_node(universal_leaf, module_leaf);
    let universal_root_proof = Proof::new(0, 2, vec![module_leaf])
        .unwrap_or_else(|error| panic!("universal proof: {error:?}"));

    let receipt = receipt_bytes([0x71; 32], resulting_state_root, receipt_signature_key);
    let (receipt_proof, receipt_root) = build_proof(&[receipt.as_slice()], 0)
        .unwrap_or_else(|error| panic!("receipt proof: {error:?}"));
    let header = header_bytes(resulting_state_root, receipt_root, sequencer_id);
    let header_digest = batch_header_digest(&header)
        .unwrap_or_else(|error| panic!("header digest failed: {error:?}"));
    let header_signature = sequencer.sign(&header_digest).to_bytes();
    let authorization = SequencerAuthorization::new(sequencer_id, sequencer_id, 7, 7);
    (
        NestedAccountProof {
            account_id,
            account_root,
            universal_root,
            resulting_state_root,
            account_proof,
            account_tree_proof,
            universal_root_proof,
            receipt_bytes: receipt,
            receipt_proof,
            header_bytes: header,
            header_signature,
        },
        authorization,
        resulting_state_root,
    )
}

fn program_account_vectors() -> ([u8; 32], [u8; 32], Vec<u8>) {
    let entries = vectors(PROGRAM_ACCOUNT_VECTORS);
    let account_id = fixed::<32>(
        entries
            .get("account_id")
            .unwrap_or_else(|| panic!("missing account id vector")),
    );
    let asset_id = fixed::<32>(
        entries
            .get("asset_id")
            .unwrap_or_else(|| panic!("missing asset id vector")),
    );
    let account_value = hex(entries
        .get("account_value")
        .unwrap_or_else(|| panic!("missing account value vector")));
    (account_id, asset_id, account_value)
}

fn maintenance_leaf(resulting_root: [u8; 32]) -> Vec<u8> {
    use layerx_programs_runtime::{meter::FeeSchedule, occupancy::OccupancyLedger};

    let schedule = FeeSchedule::new_complete(layerx_programs_runtime::FeeScheduleParameters {
        version: 1,
        fee_units_per_cpu_fuel: 1,
        fee_units_per_memory_byte: 2,
        fee_units_per_storage_read_byte: 3,
        fee_units_per_storage_write_byte: 4,
        fee_units_per_output_value: 5,
        fee_units_per_output_byte: 6,
        fee_units_per_occupancy_byte_batch: 7,
    });
    let prepared = OccupancyLedger::activated_after(6)
        .prepare_unchanged_batch(7, schedule)
        .unwrap_or_else(|error| panic!("real settlement failed: {error:?}"));
    let settlement = prepared.settlement();
    let evidence = settlement.canonical_evidence();
    let usage = settlement.usage();
    let mut schedule_bytes = 1_u32.to_be_bytes().to_vec();
    for price in 1_u64..=7 {
        schedule_bytes.extend_from_slice(&price.to_be_bytes());
    }
    schedule_bytes.extend_from_slice(&[0x81; 32]);
    let mut schedule_preimage = b"LXP/v1/context-hash\0".to_vec();
    schedule_preimage.extend_from_slice(&schedule_bytes);
    let commitment = Sha256::digest(schedule_preimage);
    let mut bytes = b"LXP/programs/occupancy-receipt/v2\0".to_vec();
    bytes.extend_from_slice(&7_u64.to_be_bytes());
    bytes.extend_from_slice(&10_u64.to_be_bytes());
    bytes.extend_from_slice(&1_u32.to_be_bytes());
    bytes.extend_from_slice(&schedule_bytes);
    for amount in [
        usage.byte_batches,
        usage.fee_units,
        usage.paid_fee_units,
        usage.arrears_fee_units,
    ] {
        bytes.extend_from_slice(&amount.to_be_bytes());
    }
    assert!(settlement
        .payer_dispositions()
        .unwrap_or_else(|error| panic!("payers: {error:?}"))
        .is_empty());
    bytes.extend_from_slice(&0_u16.to_be_bytes());
    bytes.extend_from_slice(&commitment);
    bytes.extend_from_slice(
        &u32::try_from(evidence.len())
            .unwrap_or_else(|_| panic!("evidence length"))
            .to_be_bytes(),
    );
    bytes.extend_from_slice(&evidence);
    bytes.extend_from_slice(&Sha256::digest(&evidence));
    bytes.extend_from_slice(&[0x82; 32]);
    bytes.extend_from_slice(
        &settlement
            .transfer_root([0x81; 32])
            .unwrap_or_else(|error| panic!("transfer root: {error:?}")),
    );
    bytes.extend_from_slice(&[0x83; 32]);
    bytes.extend_from_slice(&resulting_root);
    bytes
}

fn sign_maintenance_proof(proof: &mut NestedAccountProof) {
    let sequencer = SigningKey::from_bytes(&[0x51; 32]);
    let activity_receipt = receipt_bytes([0x71; 32], [0x83; 32], Some(&sequencer));
    let (path, root) = build_proof(
        &[activity_receipt.as_slice(), proof.receipt_bytes.as_slice()],
        1,
    )
    .unwrap_or_else(|error| panic!("combined receipt proof: {error:?}"));
    proof.receipt_proof = path;
    proof.header_bytes = header_bytes(
        proof.resulting_state_root,
        root,
        sequencer.verifying_key().to_bytes(),
    );
    proof.header_signature = sequencer
        .sign(
            &batch_header_digest(&proof.header_bytes)
                .unwrap_or_else(|error| panic!("header hash: {error:?}")),
        )
        .to_bytes();
}

fn append_length(bytes: &mut Vec<u8>, value: &[u8]) {
    bytes.extend_from_slice(
        &u32::try_from(value.len())
            .unwrap_or_else(|_| panic!("wire length"))
            .to_be_bytes(),
    );
    bytes.extend_from_slice(value);
}

fn append_proof(bytes: &mut Vec<u8>, proof: &Proof) {
    bytes.extend_from_slice(&proof.leaf_index().to_be_bytes());
    bytes.extend_from_slice(&proof.leaf_count().to_be_bytes());
    bytes.push(u8::try_from(proof.siblings().len()).unwrap_or_else(|_| panic!("proof depth")));
    for sibling in proof.siblings() {
        bytes.extend_from_slice(sibling);
    }
}

fn evidence(version: u16, proof: &NestedAccountProof) -> Vec<u8> {
    let key = SigningKey::from_bytes(&[0x51; 32])
        .verifying_key()
        .to_bytes();
    let mut bytes = version.to_be_bytes().to_vec();
    bytes.extend_from_slice(&[2, 1]);
    for root in [
        proof.account_id,
        proof.account_root,
        proof.universal_root,
        proof.resulting_state_root,
    ] {
        bytes.extend_from_slice(&root);
    }
    for path in [
        &proof.account_proof,
        &proof.account_tree_proof,
        &proof.universal_root_proof,
    ] {
        append_proof(&mut bytes, path);
    }
    append_length(&mut bytes, &proof.receipt_bytes);
    append_proof(&mut bytes, &proof.receipt_proof);
    bytes.extend_from_slice(&1_u16.to_be_bytes());
    bytes.extend_from_slice(&key);
    bytes.extend_from_slice(&key);
    bytes.extend_from_slice(&7_u64.to_be_bytes());
    bytes.extend_from_slice(&7_u64.to_be_bytes());
    append_length(&mut bytes, &proof.header_bytes);
    bytes.extend_from_slice(&proof.header_signature);
    bytes.push(0);
    bytes
}

fn fixture(maintenance: bool) -> (NestedAccountProof, SequencerAuthorization, Vec<u8>) {
    let (account_id, _, value) = program_account_vectors();
    let key = SigningKey::from_bytes(&[0x51; 32]);
    let (mut proof, authorization, root) = nested_fixture(account_id, &value, Some(&key));
    if maintenance {
        proof.receipt_bytes = maintenance_leaf(root);
        sign_maintenance_proof(&mut proof);
    }
    (proof, authorization, value)
}

fn read_evidence(
    proof: &NestedAccountProof,
    authorization: SequencerAuthorization,
    value: &[u8],
    wire: Vec<u8>,
) -> Result<ReadValue, ReadError> {
    static NEXT_SOCKET: AtomicU64 = AtomicU64::new(0);
    let path = std::env::temp_dir().join(format!(
        "lx-maint-{}-{}.sock",
        std::process::id(),
        NEXT_SOCKET.fetch_add(1, Ordering::Relaxed)
    ));
    let listener = UnixListener::bind(&path).unwrap_or_else(|error| panic!("bind: {error}"));
    let payload = value.to_vec();
    let expected_account = proof.account_id;
    let server = thread::spawn(move || {
        let (mut stream, _) = listener
            .accept()
            .unwrap_or_else(|error| panic!("accept: {error}"));
        stream
            .set_read_timeout(Some(Duration::from_secs(5)))
            .unwrap_or_else(|error| panic!("deadline: {error}"));
        let request =
            read_frame(&mut stream, 1024 * 1024).unwrap_or_else(|error| panic!("frame: {error:?}"));
        let request =
            decode_envelope(&request).unwrap_or_else(|error| panic!("request: {error:?}"));
        assert_eq!(request.message_tag, 7);
        let response = encode_envelope(Envelope {
            version: Version::V1_2,
            message_tag: 8,
            correlation_id: request.correlation_id,
            canonical_payload: &payload,
            proof_material: &wire,
        })
        .unwrap_or_else(|error| panic!("response: {error:?}"));
        write_frame(&mut stream, &response, 1024 * 1024)
            .unwrap_or_else(|error| panic!("write: {error:?}"));
    });
    let gate = ConnectionGate::new(1);
    let limits = Limits {
        maximum_frame_bytes: 1024 * 1024,
        maximum_connections: 1,
        maximum_streams: 1,
        maximum_queued_bytes: 2 * 1024 * 1024,
        deadline: Duration::from_secs(5),
    };
    let mut transport =
        Uds::connect(&path, &gate, limits).unwrap_or_else(|error| panic!("connect: {error:?}"));
    let result = account(
        &mut transport,
        expected_account,
        ReadContext {
            interface_version: Version::V1_2,
            correlation_id: 44,
            expected_protocol_version: PROTOCOL_VERSION,
            expected_network_id: 42,
            requested: Requested::new(VerificationLevel::STATE_PROVEN),
            head: Head {
                chain_sequence: 99,
                sealed_batch: 7,
                finalised_checkpoint: [0; 32],
            },
            sequencer_authorization: authorization,
            handshake_sequencer_key: authorization.public_key(),
            root_selector: RootSelector::Latest,
        },
    );
    assert!(server.join().is_ok(), "response writer panicked");
    fs::remove_file(path).unwrap_or_else(|error| panic!("remove socket: {error}"));
    result
}

#[test]
fn exported_account_evidence_verifies_offline_and_refuses_substitution() {
    for maintenance in [false, true] {
        let (proof, authorization, value) = fixture(maintenance);
        let wire = evidence(if maintenance { 2 } else { 1 }, &proof);
        let policy = AccountEvidencePolicy {
            expected_protocol_version: PROTOCOL_VERSION,
            expected_network_id: 42,
            handshake_sequencer_key: authorization.public_key(),
            root_selector: RootSelector::Latest,
        };
        let verified = verify_account_evidence(&value, &wire, proof.account_id, None, policy)
            .unwrap_or_else(|error| panic!("offline account evidence: {error:?}"));
        assert_eq!(verified.level(), VerificationLevel::STATE_PROVEN);
        assert_eq!(verification_label(verified.level()), Some("state_proven"));
        assert_eq!(verified.observed_sequence(), 10);
        assert_eq!(verified.batch_number(), 7);
        assert_eq!(verified.state_root(), proof.resulting_state_root);
        assert_eq!(verified.account().account_id, proof.account_id);
        assert_eq!(
            verified.signed_header().public_key,
            authorization.public_key()
        );
        assert_eq!(
            verify_account_evidence(&value, &wire, [0x99; 32], None, policy),
            Err(EvidenceError::SelectorMismatch),
            "account substitution {maintenance}"
        );
        let asset = program_account_vectors().1;
        assert_eq!(verified.account().asset_id(), asset);
        assert!(
            verify_account_evidence(&value, &wire, proof.account_id, Some(asset), policy).is_ok(),
            "held asset {maintenance}"
        );
        assert_eq!(
            verify_account_evidence(&value, &wire, proof.account_id, Some([0x55; 32]), policy),
            Err(EvidenceError::Account(AccountProofError::AssetIdentity)),
            "asset substitution {maintenance}"
        );
        let foreign_key = AccountEvidencePolicy {
            handshake_sequencer_key: [0x7a; 32],
            ..policy
        };
        assert_eq!(
            verify_account_evidence(&value, &wire, proof.account_id, None, foreign_key),
            Err(EvidenceError::SequencerMismatch),
            "sequencer substitution {maintenance}"
        );
        let foreign_network = AccountEvidencePolicy {
            expected_network_id: policy.expected_network_id + 1,
            ..policy
        };
        assert!(
            verify_account_evidence(&value, &wire, proof.account_id, None, foreign_network)
                .is_err(),
            "network substitution {maintenance}"
        );
        let checkpoint_root = AccountEvidencePolicy {
            root_selector: RootSelector::Checkpoint([0x11; 32]),
            ..policy
        };
        assert_eq!(
            verify_account_evidence(&value, &wire, proof.account_id, None, checkpoint_root),
            Err(EvidenceError::SelectorMismatch),
            "root selector substitution {maintenance}"
        );
        let mut changed = value.clone();
        let last = changed.len() - 1;
        changed[last] ^= 1;
        assert!(
            verify_account_evidence(&changed, &wire, proof.account_id, None, policy).is_err(),
            "value mutation {maintenance}"
        );
    }
}

#[test]
fn version_two_decodes_and_verifies_production_settlement_with_signed_freshness() {
    let (proof, authorization, value) = fixture(true);
    let wire = evidence(2, &proof);
    let verified = read_evidence(&proof, authorization, &value, wire.clone())
        .unwrap_or_else(|error| panic!("maintenance read: {error:?}"));
    assert_eq!(verified.canonical_bytes(), value);
    assert_eq!(verified.proof_material(), wire);
    assert_eq!(verified.achieved(), VerificationLevel::STATE_PROVEN);
    assert_eq!(verified.freshness().global_sequence, 10);
    assert_eq!(verified.freshness().batch_number, 7);
    assert_eq!(verified.freshness().observed_head_sequence, 99);
}

#[test]
fn ordinary_version_one_still_verifies_and_both_version_swaps_fail() {
    let (proof, authorization, value) = fixture(false);
    let verified = read_evidence(&proof, authorization, &value, evidence(1, &proof))
        .unwrap_or_else(|error| panic!("ordinary read: {error:?}"));
    assert_eq!(verified.achieved(), VerificationLevel::STATE_PROVEN);
    assert_eq!(verified.freshness().global_sequence, 10);
    assert_eq!(
        read_evidence(&proof, authorization, &value, evidence(2, &proof)),
        Err(ReadError::ProductionEvidence(EvidenceError::Receipt))
    );
    let (proof, authorization, value) = fixture(true);
    assert_eq!(
        read_evidence(&proof, authorization, &value, evidence(1, &proof)),
        Err(ReadError::Account(AccountProofError::ReceiptSignature))
    );
}

#[test]
fn mutated_maintenance_commitment_and_signed_inclusion_are_rejected() {
    let (proof, authorization, value) = fixture(true);
    let mut changed = proof.clone();
    let digest_offset = changed.receipt_bytes.len() - 160;
    changed.receipt_bytes[digest_offset] ^= 1;
    assert_eq!(
        read_evidence(&changed, authorization, &value, evidence(2, &changed)),
        Err(ReadError::ProductionEvidence(EvidenceError::Receipt))
    );
    let mut changed = proof.clone();
    let ledger_root_offset = changed.receipt_bytes.len() - 128;
    changed.receipt_bytes[ledger_root_offset] ^= 1;
    assert!(matches!(
        read_evidence(&changed, authorization, &value, evidence(2, &changed)),
        Err(ReadError::Account(AccountProofError::Header(_)))
    ));
    let mut changed = proof;
    changed.header_signature[0] ^= 1;
    assert!(matches!(
        read_evidence(&changed, authorization, &value, evidence(2, &changed)),
        Err(ReadError::Account(AccountProofError::Header(_)))
    ));
}

#[test]
fn receipt_and_maintenance_length_limits_are_independent() {
    let (mut proof, authorization, value) = fixture(false);
    for length in [4096, 4097, 86_680, 86_681] {
        proof.receipt_bytes.resize(length, 0);
        let ordinary_error = if length <= 4096 {
            ReadError::Account(AccountProofError::Header(InclusionError::Merkle(
                MerkleError::RootMismatch,
            )))
        } else {
            ReadError::ProductionEvidence(EvidenceError::Malformed)
        };
        assert_eq!(
            read_evidence(&proof, authorization, &value, evidence(1, &proof)),
            Err(ordinary_error),
            "ordinary length {length}"
        );
        let maintenance_error = if length <= 86_680 {
            EvidenceError::Receipt
        } else {
            EvidenceError::Malformed
        };
        assert_eq!(
            read_evidence(&proof, authorization, &value, evidence(2, &proof)),
            Err(ReadError::ProductionEvidence(maintenance_error)),
            "maintenance length {length}"
        );
    }
}
