//! Verifies public account and balance reads client-side against the exact
//! nested state evidence a node exports alongside them.

use std::collections::BTreeMap;
use std::io::{Read as _, Write as _};
use std::net::{TcpListener, TcpStream};
use std::sync::mpsc::{channel, Sender};
use std::thread::{self, JoinHandle};

use ed25519_dalek::{Signer as _, SigningKey};
use layerx_client::evidence::verification_label;
use layerx_proof::merkle::{build_proof, Proof};
use layerx_proof::state::{decode_account_value, NestedAccountProof};
use layerx_sdk::rpc::{RpcClient, RpcError};
use layerx_sdk::rpc_verification::{AccountPolicy, VerifiedRpcAccount, VerifiedRpcBalances};
use layerx_wire::encode::Encoder;
use layerx_wire::hash::{batch_header_digest, receipt_digest};
use layerx_wire::limits::PROTOCOL_VERSION;
use serde_json::{json, Value};
use sha2::{Digest as _, Sha256};

const PROGRAM_ACCOUNT_VECTORS: &str =
    include_str!("../../../../tests/vectors/program_account_state_v2.vec");
const NETWORK_ID: u32 = 42;
const BATCH_NUMBER: u64 = 7;
const RECEIPT_SEQUENCE: u64 = 10;
const HEAD_SEQUENCE: u64 = 99;

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

fn encode_hex(bytes: &[u8]) -> String {
    bytes.iter().fold(String::new(), |mut text, byte| {
        text.push(char::from_digit(u32::from(byte >> 4), 16).unwrap_or_else(|| panic!("nibble")));
        text.push(char::from_digit(u32::from(byte & 0x0f), 16).unwrap_or_else(|| panic!("nibble")));
        text
    })
}

fn account_vector() -> ([u8; 32], Vec<u8>) {
    let entries: BTreeMap<&str, &str> = PROGRAM_ACCOUNT_VECTORS
        .lines()
        .filter_map(|line| line.split_once('='))
        .collect();
    let account_id = hex(entries
        .get("account_id")
        .unwrap_or_else(|| panic!("missing account id vector")))
    .try_into()
    .unwrap_or_else(|_| panic!("account id is not 32 bytes"));
    let value = hex(entries
        .get("account_value")
        .unwrap_or_else(|| panic!("missing account value vector")));
    (account_id, value)
}

fn state_leaf(key: &[u8], value: &[u8]) -> [u8; 32] {
    let mut bytes = Vec::with_capacity(26 + key.len() + value.len());
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

fn receipt_bytes(resulting_state_root: [u8; 32], sequencer: &SigningKey) -> Vec<u8> {
    let encode = |signature: Option<[u8; 64]>| {
        let mut encoder = Encoder::new(4096);
        assert_eq!(
            encoder.structure_header_version(0x5201, PROTOCOL_VERSION),
            Ok(())
        );
        assert_eq!(encoder.u16(PROTOCOL_VERSION), Ok(()));
        assert_eq!(encoder.bytes(&[0x71; 32], 32), Ok(()));
        assert_eq!(encoder.u64(RECEIPT_SEQUENCE), Ok(()));
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
    let digest = receipt_digest(&unsigned)
        .unwrap_or_else(|error| panic!("receipt digest failed: {error:?}"));
    encode(Some(sequencer.sign(&digest).to_bytes()))
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
    let digests: [(u8, [u8; 32]); 8] = [
        (7, [0x31; 32]),
        (8, resulting_state_root),
        (9, [0x32; 32]),
        (10, receipt_root),
        (11, [0x33; 32]),
        (12, [0x34; 32]),
        (13, [0x35; 32]),
        (15, sequencer_id),
    ];
    let counters: [(u8, u64); 5] = [(3, 2), (4, BATCH_NUMBER), (5, 9), (6, 10), (14, 1_000)];
    for field in 1..=15_u8 {
        assert_eq!(encoder.tag(field, 15), Ok(()));
        match field {
            1 => assert_eq!(encoder.u16(PROTOCOL_VERSION), Ok(())),
            2 => assert_eq!(encoder.u32(NETWORK_ID), Ok(())),
            _ => {
                if let Some((_, counter)) = counters.iter().find(|(tag, _)| *tag == field) {
                    assert_eq!(encoder.u64(*counter), Ok(()));
                } else {
                    let (_, digest) = digests
                        .iter()
                        .find(|(tag, _)| *tag == field)
                        .unwrap_or_else(|| panic!("header field {field} is undefined"));
                    assert_eq!(encoder.bytes(digest, 32), Ok(()));
                }
            }
        }
    }
    let bytes = encoder.finish();
    assert_eq!(bytes.len(), 354);
    bytes
}

fn nested_fixture(
    account_id: [u8; 32],
    account_value: &[u8],
    sequencer: &SigningKey,
) -> NestedAccountProof {
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

    let receipt = receipt_bytes(resulting_state_root, sequencer);
    let (receipt_proof, receipt_root) = build_proof(&[receipt.as_slice()], 0)
        .unwrap_or_else(|error| panic!("receipt proof: {error:?}"));
    let header = header_bytes(resulting_state_root, receipt_root, sequencer_id);
    let header_digest =
        batch_header_digest(&header).unwrap_or_else(|error| panic!("header digest: {error:?}"));
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
        header_signature: sequencer.sign(&header_digest).to_bytes(),
    }
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

/// Encodes exactly the account evidence a node exports with a latest-root read.
fn exported_evidence(proof: &NestedAccountProof, sequencer_id: [u8; 32]) -> Vec<u8> {
    let mut bytes = 1_u16.to_be_bytes().to_vec();
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
    bytes.extend_from_slice(&sequencer_id);
    bytes.extend_from_slice(&sequencer_id);
    bytes.extend_from_slice(&BATCH_NUMBER.to_be_bytes());
    bytes.extend_from_slice(&BATCH_NUMBER.to_be_bytes());
    append_length(&mut bytes, &proof.header_bytes);
    bytes.extend_from_slice(&proof.header_signature);
    bytes.push(0);
    bytes
}

struct Fixture {
    account_id: [u8; 32],
    canonical_value: Vec<u8>,
    proof_material: Vec<u8>,
    sequencer_key: [u8; 32],
}

fn fixture() -> Fixture {
    let (account_id, canonical_value) = account_vector();
    let sequencer = SigningKey::from_bytes(&[0x51; 32]);
    let sequencer_key = sequencer.verifying_key().to_bytes();
    let proof = nested_fixture(account_id, &canonical_value, &sequencer);
    Fixture {
        account_id,
        canonical_value,
        proof_material: exported_evidence(&proof, sequencer_key),
        sequencer_key,
    }
}

/// Builds the exact result object the core public account read serves.
fn served(fixture: &Fixture) -> Value {
    let account = decode_account_value(fixture.account_id, &fixture.canonical_value)
        .unwrap_or_else(|error| panic!("canonical account: {error:?}"));
    let name =
        std::str::from_utf8(&account.name).unwrap_or_else(|error| panic!("account name: {error}"));
    json!({
        "account_id": encode_hex(&fixture.account_id),
        "name": name,
        "asset_id": encode_hex(&account.asset_id()),
        "balance": account.balance().to_string(),
        "next_sequence": account.next_sequence.to_string(),
        "frozen": account.frozen,
        "canonical_value": encode_hex(&fixture.canonical_value),
        "proof_material": encode_hex(&fixture.proof_material),
        "observed_head_sequence": HEAD_SEQUENCE.to_string(),
        "batch_number": BATCH_NUMBER.to_string(),
        "verification": "state_proven"
    })
}

fn policy(sequencer_key: [u8; 32]) -> AccountPolicy {
    AccountPolicy {
        protocol_version: PROTOCOL_VERSION,
        network_id: NETWORK_ID,
        sequencer_key,
    }
}

#[test]
fn served_account_read_is_reproduced_from_its_own_proof_material() {
    let fixture = fixture();
    let result = served(&fixture);
    let verified = VerifiedRpcAccount::from_rpc_result(
        &result,
        fixture.account_id,
        &policy(fixture.sequencer_key),
    )
    .unwrap_or_else(|error| panic!("served read refused: {error:?}"));
    let evidence = verified.evidence();
    assert_eq!(evidence.account().account_id, fixture.account_id);
    assert_eq!(
        Some(evidence.account().balance().to_string().as_str()),
        result["balance"].as_str()
    );
    assert_eq!(
        Some(encode_hex(&evidence.account().asset_id()).as_str()),
        result["asset_id"].as_str()
    );
    assert!(evidence.account().has_asset());
    assert_eq!(evidence.batch_number(), BATCH_NUMBER);
    assert_eq!(evidence.observed_sequence(), RECEIPT_SEQUENCE);
    assert_eq!(verification_label(evidence.level()), Some("state_proven"));
    assert_eq!(evidence.signed_header().public_key, fixture.sequencer_key);
    assert_eq!(verified.canonical_bytes(), fixture.canonical_value);
    assert_eq!(verified.proof_material(), fixture.proof_material);
}

#[test]
fn served_fields_the_proof_does_not_reproduce_are_refused() {
    let fixture = fixture();
    let policy = policy(fixture.sequencer_key);
    for (field, substitute) in [
        ("balance", json!("1")),
        ("next_sequence", json!("9")),
        ("frozen", json!(true)),
        ("name", json!("module:programs:value:2222222222222222")),
        ("asset_id", json!("11".repeat(32))),
        ("account_id", json!("22".repeat(32))),
        ("batch_number", json!("8")),
        ("verification", json!("unverified")),
        ("verification", json!("checkpoint_finalised")),
        ("verification", json!("state_proven ")),
    ] {
        let mut result = served(&fixture);
        result[field] = substitute.clone();
        let refusal = VerifiedRpcAccount::from_rpc_result(&result, fixture.account_id, &policy);
        assert!(
            matches!(refusal, Err(RpcError::Verification)),
            "substituting {field} with {substitute} was accepted"
        );
    }
}

#[test]
fn absent_or_malformed_evidence_is_refused() {
    let fixture = fixture();
    let policy = policy(fixture.sequencer_key);
    let mut without_proof = served(&fixture);
    without_proof["proof_material"] = json!("");
    assert!(matches!(
        VerifiedRpcAccount::from_rpc_result(&without_proof, fixture.account_id, &policy),
        Err(RpcError::InvalidResponse)
    ));

    let mut without_label = served(&fixture);
    assert!(without_label
        .as_object_mut()
        .unwrap_or_else(|| panic!("served result is an object"))
        .remove("verification")
        .is_some());
    assert!(matches!(
        VerifiedRpcAccount::from_rpc_result(&without_label, fixture.account_id, &policy),
        Err(RpcError::Verification)
    ));

    let mut padded_decimal = served(&fixture);
    padded_decimal["next_sequence"] = json!("007");
    assert!(matches!(
        VerifiedRpcAccount::from_rpc_result(&padded_decimal, fixture.account_id, &policy),
        Err(RpcError::InvalidResponse)
    ));

    let mut forged_signature = fixture.proof_material.clone();
    let last_signature_byte = forged_signature.len() - 2;
    forged_signature[last_signature_byte] ^= 1;
    let mut forged = served(&fixture);
    forged["proof_material"] = json!(encode_hex(&forged_signature));
    assert!(matches!(
        VerifiedRpcAccount::from_rpc_result(&forged, fixture.account_id, &policy),
        Err(RpcError::Verification)
    ));

    let mut substituted_value = fixture.canonical_value.clone();
    let balance_byte = substituted_value.len() - 1;
    substituted_value[balance_byte] ^= 1;
    let mut substituted = served(&fixture);
    substituted["canonical_value"] = json!(encode_hex(&substituted_value));
    assert!(matches!(
        VerifiedRpcAccount::from_rpc_result(&substituted, fixture.account_id, &policy),
        Err(RpcError::Verification)
    ));
}

#[test]
fn evidence_outside_the_pinned_trust_is_refused() {
    let fixture = fixture();
    let result = served(&fixture);
    let foreign = [
        AccountPolicy {
            network_id: NETWORK_ID + 1,
            ..policy(fixture.sequencer_key)
        },
        AccountPolicy {
            protocol_version: PROTOCOL_VERSION + 1,
            ..policy(fixture.sequencer_key)
        },
        policy([0x7a; 32]),
    ];
    for policy in foreign {
        assert!(matches!(
            VerifiedRpcAccount::from_rpc_result(&result, fixture.account_id, &policy),
            Err(RpcError::Verification)
        ));
    }
    assert!(matches!(
        VerifiedRpcAccount::from_rpc_result(&result, [0x99; 32], &policy(fixture.sequencer_key)),
        Err(RpcError::Verification)
    ));
}

fn read_request_body(stream: &mut TcpStream) -> Vec<u8> {
    let mut buffer = Vec::new();
    let mut byte = [0_u8; 1];
    let mut expected = None;
    loop {
        let read = stream
            .read(&mut byte)
            .unwrap_or_else(|error| panic!("read request: {error}"));
        assert_eq!(read, 1, "the request ended before its body");
        buffer.push(byte[0]);
        if expected.is_none() && buffer.ends_with(b"\r\n\r\n") {
            let head = String::from_utf8_lossy(&buffer).to_ascii_lowercase();
            let length: usize = head
                .split("content-length:")
                .nth(1)
                .and_then(|rest| rest.split("\r\n").next())
                .and_then(|value| value.trim().parse().ok())
                .unwrap_or_else(|| panic!("request has no content length"));
            expected = Some(buffer.len() + length);
        }
        if expected.is_some_and(|total| buffer.len() >= total) {
            break;
        }
    }
    let body = buffer
        .windows(4)
        .position(|window| window == b"\r\n\r\n")
        .unwrap_or_else(|| panic!("request head is unterminated"))
        + 4;
    buffer[body..].to_vec()
}

fn serve(
    listener: TcpListener,
    results: Vec<Value>,
    calls: Sender<(String, Value)>,
) -> JoinHandle<()> {
    thread::spawn(move || {
        for result in results {
            let (mut stream, _) = listener
                .accept()
                .unwrap_or_else(|error| panic!("accept: {error}"));
            let request: Value = serde_json::from_slice(&read_request_body(&mut stream))
                .unwrap_or_else(|error| panic!("request body: {error}"));
            calls
                .send((
                    request["method"].as_str().unwrap_or_default().to_owned(),
                    request["params"].clone(),
                ))
                .unwrap_or_else(|error| panic!("record call: {error}"));
            let body = serde_json::to_vec(&json!({
                "jsonrpc": "2.0",
                "id": request["id"].as_str().unwrap_or_default(),
                "result": result
            }))
            .unwrap_or_else(|error| panic!("encode response: {error}"));
            let head = format!(
                "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                body.len()
            );
            stream
                .write_all(head.as_bytes())
                .and_then(|()| stream.write_all(&body))
                .and_then(|()| stream.flush())
                .unwrap_or_else(|error| panic!("write response: {error}"));
        }
    })
}

#[test]
fn rpc_account_and_balance_reads_verify_before_returning() {
    let fixture = fixture();
    let listener = TcpListener::bind("127.0.0.1:0").unwrap_or_else(|error| panic!("bind: {error}"));
    let port = listener
        .local_addr()
        .unwrap_or_else(|error| panic!("local address: {error}"))
        .port();
    let mut understated = served(&fixture);
    understated["balance"] = json!("1");
    let (calls, observed) = channel();
    let server = serve(
        listener,
        vec![served(&fixture), served(&fixture), understated],
        calls,
    );

    let client = RpcClient::connect(&format!("http://127.0.0.1:{port}/rpc"), None)
        .unwrap_or_else(|error| panic!("connect: {error:?}"));
    let policy = policy(fixture.sequencer_key);
    let account = client
        .verified_account(fixture.account_id, &policy)
        .unwrap_or_else(|error| panic!("verified account: {error:?}"));
    assert_eq!(account.canonical_bytes(), fixture.canonical_value);
    let balance = client
        .verified_balance(fixture.account_id, &policy)
        .unwrap_or_else(|error| panic!("verified balance: {error:?}"));
    assert_eq!(
        balance.evidence().account().balance(),
        account.evidence().account().balance()
    );
    assert!(matches!(
        client.verified_account(fixture.account_id, &policy),
        Err(RpcError::Verification)
    ));

    assert!(server.join().is_ok(), "the server thread panicked");
    let selector = json!([encode_hex(&fixture.account_id)]);
    let recorded: Vec<(String, Value)> = observed.iter().collect();
    assert_eq!(
        recorded,
        vec![
            ("lx_getAccount".to_owned(), selector.clone()),
            ("lx_getBalance".to_owned(), selector.clone()),
            ("lx_getAccount".to_owned(), selector),
        ]
    );
}

const LISTED_DID: &str = "did:layerx:alice";

/// Encodes the exact canonical value the account registry commits for an
/// agent-owned account holding one asset.
fn agent_fixture(name: &[u8], kind: u8, balance: u128, next_sequence: u64) -> Fixture {
    let length = u32::try_from(name.len()).unwrap_or_else(|_| panic!("account name length"));
    let mut identity = b"LX:ACCOUNT:v1".to_vec();
    identity.extend_from_slice(&length.to_be_bytes());
    identity.extend_from_slice(name);
    let account_id: [u8; 32] = Sha256::digest(identity).into();
    let mut canonical_value = Vec::with_capacity(103 + name.len());
    canonical_value.extend_from_slice(
        &u16::try_from(name.len())
            .unwrap_or_else(|_| panic!("account name length"))
            .to_be_bytes(),
    );
    canonical_value.extend_from_slice(name);
    canonical_value.push(kind);
    canonical_value.extend_from_slice(&balance.to_be_bytes());
    canonical_value.extend_from_slice(&[0x22; 32]);
    canonical_value.push(1);
    canonical_value.extend_from_slice(&next_sequence.to_be_bytes());
    canonical_value.extend_from_slice(&3_u64.to_be_bytes());
    canonical_value.push(0);
    canonical_value.push(0);
    canonical_value.extend_from_slice(&[0; 32]);
    canonical_value.push(0);
    let sequencer = SigningKey::from_bytes(&[0x51; 32]);
    let sequencer_key = sequencer.verifying_key().to_bytes();
    let proof = nested_fixture(account_id, &canonical_value, &sequencer);
    Fixture {
        account_id,
        canonical_value,
        proof_material: exported_evidence(&proof, sequencer_key),
        sequencer_key,
    }
}

/// Builds the two accounts a DID listing serves, in ascending identifier order.
fn listed_fixtures(did: &str) -> Vec<Fixture> {
    let mut fixtures = vec![
        agent_fixture(format!("agent:{did}:main").as_bytes(), 1, 250, 4),
        agent_fixture(format!("agent:{did}:budget:ops").as_bytes(), 2, 75, 1),
    ];
    fixtures.sort_by(|left, right| left.account_id.cmp(&right.account_id));
    fixtures
}

/// Builds the exact result object the core DID account listing serves.
fn served_listing(did: &str, fixtures: &[Fixture]) -> Value {
    json!({
        "did": did,
        "accounts": fixtures.iter().map(served).collect::<Vec<_>>(),
        "verification": "state_proven"
    })
}

#[test]
fn served_did_listing_is_reproduced_entry_by_entry() {
    let fixtures = listed_fixtures(LISTED_DID);
    let sequencer_key = fixtures[0].sequencer_key;
    let listing = served_listing(LISTED_DID, &fixtures);
    let verified =
        VerifiedRpcBalances::from_rpc_result(&listing, LISTED_DID, &policy(sequencer_key))
            .unwrap_or_else(|error| panic!("served listing refused: {error:?}"));
    assert_eq!(verified.did(), LISTED_DID);
    assert_eq!(verified.accounts().len(), fixtures.len());
    assert_eq!(verification_label(verified.level()), Some("state_proven"));
    for (account, fixture) in verified.accounts().iter().zip(&fixtures) {
        assert_eq!(account.evidence().account().account_id, fixture.account_id);
        assert_eq!(account.canonical_bytes(), fixture.canonical_value);
        assert_eq!(account.proof_material(), fixture.proof_material);
        assert!(account
            .evidence()
            .account()
            .name
            .starts_with(b"agent:did:layerx:alice:"));
        assert_eq!(account.evidence().batch_number(), BATCH_NUMBER);
    }
    let balances: Vec<u128> = verified
        .accounts()
        .iter()
        .map(|account| account.evidence().account().balance())
        .collect();
    let expected: Vec<u128> = fixtures
        .iter()
        .map(|fixture| {
            decode_account_value(fixture.account_id, &fixture.canonical_value)
                .unwrap_or_else(|error| panic!("canonical account: {error:?}"))
                .balance()
        })
        .collect();
    assert_eq!(balances, expected);
    assert_eq!(balances.iter().sum::<u128>(), 325);
}

#[test]
fn did_listings_the_proofs_do_not_establish_are_refused() {
    let fixtures = listed_fixtures(LISTED_DID);
    let pinned = policy(fixtures[0].sequencer_key);
    let listing = served_listing(LISTED_DID, &fixtures);

    assert!(matches!(
        VerifiedRpcBalances::from_rpc_result(&listing, "did:layerx:bob", &pinned),
        Err(RpcError::Verification)
    ));

    let mut without_did = listing.clone();
    assert!(without_did
        .as_object_mut()
        .unwrap_or_else(|| panic!("served listing is an object"))
        .remove("did")
        .is_some());
    assert!(matches!(
        VerifiedRpcBalances::from_rpc_result(&without_did, LISTED_DID, &pinned),
        Err(RpcError::Verification)
    ));

    let mut empty = listing.clone();
    empty["accounts"] = json!([]);
    assert!(matches!(
        VerifiedRpcBalances::from_rpc_result(&empty, LISTED_DID, &pinned),
        Err(RpcError::Verification)
    ));

    let mut not_a_list = listing.clone();
    not_a_list["accounts"] = json!({});
    assert!(matches!(
        VerifiedRpcBalances::from_rpc_result(&not_a_list, LISTED_DID, &pinned),
        Err(RpcError::InvalidResponse)
    ));

    let mut reversed = listing.clone();
    reversed["accounts"] = json!(fixtures.iter().rev().map(served).collect::<Vec<_>>());
    assert!(matches!(
        VerifiedRpcBalances::from_rpc_result(&reversed, LISTED_DID, &pinned),
        Err(RpcError::Verification)
    ));

    let mut duplicated = listing.clone();
    duplicated["accounts"] = json!([served(&fixtures[0]), served(&fixtures[0])]);
    assert!(matches!(
        VerifiedRpcBalances::from_rpc_result(&duplicated, LISTED_DID, &pinned),
        Err(RpcError::Verification)
    ));

    for label in [
        json!("authenticated_node_snapshot"),
        json!("checkpoint_finalised"),
        json!("unverified"),
        Value::Null,
    ] {
        let mut relabelled = listing.clone();
        relabelled["verification"] = label.clone();
        assert!(
            matches!(
                VerifiedRpcBalances::from_rpc_result(&relabelled, LISTED_DID, &pinned),
                Err(RpcError::Verification)
            ),
            "listing label {label} was accepted"
        );
    }

    let mut understated = listing.clone();
    understated["accounts"][0]["balance"] = json!("1");
    assert!(matches!(
        VerifiedRpcBalances::from_rpc_result(&understated, LISTED_DID, &pinned),
        Err(RpcError::Verification)
    ));

    let mut relabelled_entry = listing.clone();
    relabelled_entry["accounts"][1]["verification"] = json!("checkpoint_finalised");
    assert!(matches!(
        VerifiedRpcBalances::from_rpc_result(&relabelled_entry, LISTED_DID, &pinned),
        Err(RpcError::Verification)
    ));

    let foreign = agent_fixture(b"agent:did:layerx:bob:main", 1, 250, 4);
    let mut owners = fixtures
        .iter()
        .chain(std::iter::once(&foreign))
        .collect::<Vec<_>>();
    owners.sort_by(|left, right| left.account_id.cmp(&right.account_id));
    let mut mixed = listing.clone();
    mixed["accounts"] = json!(owners
        .iter()
        .map(|fixture| served(fixture))
        .collect::<Vec<_>>());
    assert!(matches!(
        VerifiedRpcBalances::from_rpc_result(&mixed, LISTED_DID, &pinned),
        Err(RpcError::Verification)
    ));

    let program = fixture();
    let mut foreign_kind = listing.clone();
    foreign_kind["accounts"] = json!([served(&program)]);
    assert!(matches!(
        VerifiedRpcBalances::from_rpc_result(&foreign_kind, LISTED_DID, &pinned),
        Err(RpcError::Verification)
    ));

    assert!(matches!(
        VerifiedRpcBalances::from_rpc_result(&listing, LISTED_DID, &policy([0x7a; 32])),
        Err(RpcError::Verification)
    ));
}

#[test]
fn rpc_balances_read_verifies_before_returning() {
    let fixtures = listed_fixtures(LISTED_DID);
    let listener = TcpListener::bind("127.0.0.1:0").unwrap_or_else(|error| panic!("bind: {error}"));
    let port = listener
        .local_addr()
        .unwrap_or_else(|error| panic!("local address: {error}"))
        .port();
    let mut relabelled = served_listing(LISTED_DID, &fixtures);
    relabelled["verification"] = json!("authenticated_node_snapshot");
    let (calls, observed) = channel();
    let server = serve(
        listener,
        vec![served_listing(LISTED_DID, &fixtures), relabelled],
        calls,
    );

    let client = RpcClient::connect(&format!("http://127.0.0.1:{port}/rpc"), None)
        .unwrap_or_else(|error| panic!("connect: {error:?}"));
    let policy = policy(fixtures[0].sequencer_key);
    let balances = client
        .verified_balances(LISTED_DID, &policy)
        .unwrap_or_else(|error| panic!("verified balances: {error:?}"));
    assert_eq!(balances.did(), LISTED_DID);
    assert_eq!(balances.accounts().len(), fixtures.len());
    assert!(matches!(
        client.verified_balances(LISTED_DID, &policy),
        Err(RpcError::Verification)
    ));
    assert!(matches!(
        client.verified_balances("", &policy),
        Err(RpcError::InvalidRequest)
    ));

    assert!(server.join().is_ok(), "the server thread panicked");
    let recorded: Vec<(String, Value)> = observed.iter().collect();
    assert_eq!(
        recorded,
        vec![
            ("lx_getBalances".to_owned(), json!([LISTED_DID])),
            ("lx_getBalances".to_owned(), json!([LISTED_DID])),
        ]
    );
}
