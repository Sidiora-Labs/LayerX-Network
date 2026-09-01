use std::collections::BTreeMap;

use ed25519_dalek::{Signer as _, SigningKey};
use layerx_proof::inclusion::{InclusionError, SequencerAuthorization};
use layerx_proof::merkle::{build_proof, MerkleError, Proof};
use layerx_proof::state::{
    decode_account_value, verify_nested_account, AccountProofError, NestedAccountProof,
};
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

fn generic_node(left: [u8; 32], right: [u8; 32]) -> [u8; 32] {
    let mut bytes = Vec::with_capacity(88);
    bytes.extend_from_slice(b"LXP/v1/merkle-internal\0");
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

#[test]
fn decodes_exact_c_account_leaf_and_refuses_identity_asset_and_value_swaps() {
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
    let value = hex(entries
        .get("account_value")
        .unwrap_or_else(|| panic!("missing account value vector")));
    let account = decode_account_value(account_id, &value)
        .unwrap_or_else(|error| panic!("C account value refused: {error:?}"));
    assert_eq!(account.account_id, account_id);
    assert_eq!(account.asset_id, asset_id);
    assert_eq!(account.balance, 0x12_34_56);
    assert!(account.has_asset);
    assert_eq!(account.next_sequence, 7);
    assert_eq!(account.created_at_sequence, 3);

    let mut swapped_account = account_id;
    swapped_account[31] ^= 1;
    assert_eq!(
        decode_account_value(swapped_account, &value),
        Err(AccountProofError::AccountIdentity)
    );

    let mut swapped_asset = value.clone();
    let asset_offset = 2 + account.name.len() + 1 + 16;
    swapped_asset[asset_offset] ^= 1;
    let decoded = decode_account_value(account_id, &swapped_asset)
        .unwrap_or_else(|error| panic!("canonical swapped asset refused too early: {error:?}"));
    assert_ne!(decoded.asset_id, asset_id);

    let mut swapped_balance = value.clone();
    let balance_offset = 2 + account.name.len() + 1;
    swapped_balance[balance_offset + 15] ^= 1;
    let decoded = decode_account_value(account_id, &swapped_balance)
        .unwrap_or_else(|error| panic!("canonical swapped balance refused too early: {error:?}"));
    assert_ne!(decoded.balance, account.balance);

    let mut bad_boolean = value.clone();
    let has_asset_offset = asset_offset + 32;
    bad_boolean[has_asset_offset] = 2;
    assert_eq!(
        decode_account_value(account_id, &bad_boolean),
        Err(AccountProofError::AccountEncoding)
    );

    let mut trailing = value.clone();
    trailing.push(0);
    assert_eq!(
        decode_account_value(account_id, &trailing),
        Err(AccountProofError::AccountEncoding)
    );
}

#[test]
fn verifies_the_exact_nested_state_domains_and_receipt_authority() {
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
    let sequencer = SigningKey::from_bytes(&[0x51; 32]);
    let (proof, authorization, resulting_state_root) =
        nested_fixture(account_id, &account_value, Some(&sequencer));
    let verified = verify_nested_account(
        &account_value,
        account_id,
        Some(asset_id),
        &proof,
        &authorization,
    )
    .unwrap_or_else(|error| panic!("exact nested proof failed: {error:?}"));
    assert_eq!(verified.account().account_id, account_id);
    assert_eq!(verified.account().asset_id, asset_id);
    assert_eq!(
        verified.header().header().resulting_state_root(),
        resulting_state_root
    );
    assert_eq!(verified.observed_sequence(), 10);
    assert_eq!(verified.observed_at_ms(), 1_000);

    let mut other_account = account_id;
    other_account[0] ^= 1;
    assert_eq!(
        verify_nested_account(
            &account_value,
            other_account,
            Some(asset_id),
            &proof,
            &authorization,
        ),
        Err(AccountProofError::AccountIdentity)
    );

    let mut other_asset = asset_id;
    other_asset[0] ^= 1;
    assert_eq!(
        verify_nested_account(
            &account_value,
            account_id,
            Some(other_asset),
            &proof,
            &authorization,
        ),
        Err(AccountProofError::AssetIdentity)
    );

    let mut other_balance = account_value.clone();
    let balance_offset = 2 + verified.account().name.len() + 1;
    other_balance[balance_offset + 15] ^= 1;
    assert_eq!(
        verify_nested_account(
            &other_balance,
            account_id,
            Some(asset_id),
            &proof,
            &authorization,
        ),
        Err(AccountProofError::AccountProof(MerkleError::RootMismatch))
    );

    let mut changed = proof.clone();
    changed.account_root[0] ^= 1;
    assert_eq!(
        verify_nested_account(
            &account_value,
            account_id,
            Some(asset_id),
            &changed,
            &authorization,
        ),
        Err(AccountProofError::AccountProof(MerkleError::RootMismatch))
    );

    changed = proof.clone();
    changed.universal_root[0] ^= 1;
    assert_eq!(
        verify_nested_account(
            &account_value,
            account_id,
            Some(asset_id),
            &changed,
            &authorization,
        ),
        Err(AccountProofError::UniversalRoot)
    );

    changed = proof.clone();
    changed.resulting_state_root[0] ^= 1;
    assert_eq!(
        verify_nested_account(
            &account_value,
            account_id,
            Some(asset_id),
            &changed,
            &authorization,
        ),
        Err(AccountProofError::StateRoot)
    );

    changed = proof.clone();
    let (_, receipt_root) = build_proof(&[changed.receipt_bytes.as_slice()], 0)
        .unwrap_or_else(|error| panic!("receipt root: {error:?}"));
    changed.header_bytes = header_bytes(
        [0x91; 32],
        receipt_root,
        sequencer.verifying_key().to_bytes(),
    );
    changed.header_signature = sequencer
        .sign(
            &batch_header_digest(&changed.header_bytes)
                .unwrap_or_else(|error| panic!("swapped header digest: {error:?}")),
        )
        .to_bytes();
    assert_eq!(
        verify_nested_account(
            &account_value,
            account_id,
            Some(asset_id),
            &changed,
            &authorization,
        ),
        Err(AccountProofError::StateRoot)
    );

    let wrong_authorization =
        SequencerAuthorization::new([0x81; 32], sequencer.verifying_key().to_bytes(), 7, 7);
    assert_eq!(
        verify_nested_account(
            &account_value,
            account_id,
            Some(asset_id),
            &proof,
            &wrong_authorization,
        ),
        Err(AccountProofError::Header(InclusionError::SequencerIdentity))
    );

    changed = proof.clone();
    changed.receipt_bytes = receipt_bytes([0x71; 32], [0x92; 32], Some(&sequencer));
    let (receipt_proof, receipt_root) = build_proof(&[changed.receipt_bytes.as_slice()], 0)
        .unwrap_or_else(|error| panic!("mismatched receipt proof: {error:?}"));
    changed.receipt_proof = receipt_proof;
    changed.header_bytes = header_bytes(
        changed.resulting_state_root,
        receipt_root,
        sequencer.verifying_key().to_bytes(),
    );
    changed.header_signature = sequencer
        .sign(
            &batch_header_digest(&changed.header_bytes)
                .unwrap_or_else(|error| panic!("receipt header digest: {error:?}")),
        )
        .to_bytes();
    assert_eq!(
        verify_nested_account(
            &account_value,
            account_id,
            Some(asset_id),
            &changed,
            &authorization,
        ),
        Err(AccountProofError::ReceiptBinding)
    );

    let mut account_key = [0_u8; 33];
    account_key[0] = 4;
    account_key[1..].copy_from_slice(&account_id);
    changed = proof.clone();
    changed.account_root = generic_node(
        state_leaf(&account_key, &account_value),
        proof.account_proof.siblings()[0],
    );
    assert_eq!(
        verify_nested_account(
            &account_value,
            account_id,
            Some(asset_id),
            &changed,
            &authorization,
        ),
        Err(AccountProofError::AccountProof(MerkleError::RootMismatch))
    );

    let (missing_signature, authorization, _) = nested_fixture(account_id, &account_value, None);
    assert_eq!(
        verify_nested_account(
            &account_value,
            account_id,
            Some(asset_id),
            &missing_signature,
            &authorization,
        ),
        Err(AccountProofError::ReceiptSignature)
    );

    let wrong_receipt_key = SigningKey::from_bytes(&[0x52; 32]);
    let (wrong_signature, authorization, _) =
        nested_fixture(account_id, &account_value, Some(&wrong_receipt_key));
    assert_eq!(
        verify_nested_account(
            &account_value,
            account_id,
            Some(asset_id),
            &wrong_signature,
            &authorization,
        ),
        Err(AccountProofError::ReceiptSignature)
    );
}

#[test]
fn malformed_state_path_geometry_never_reaches_nested_verification() {
    assert_eq!(Proof::new(0, 0, Vec::new()), Err(MerkleError::EmptyTree));
    assert_eq!(
        Proof::new(2, 2, vec![[0; 32]]),
        Err(MerkleError::LeafIndex { index: 2, count: 2 })
    );
    assert_eq!(
        Proof::new(0, 3, vec![[0; 32]]),
        Err(MerkleError::PathLength {
            expected: 2,
            actual: 1,
        })
    );
    assert_eq!(
        Proof::new(0, 3, vec![[0; 32]; 33]),
        Err(MerkleError::PathLength {
            expected: 2,
            actual: 33,
        })
    );
}
