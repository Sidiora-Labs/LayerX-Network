use k256::ecdsa::{Signature, SigningKey};
use layerx_client::availability::{AvailabilityRecords, AvailabilityResult};
use layerx_client::head::Head;
use layerx_crypto::secp256k1;
use layerx_explorer_index::verify::Verifier;
use layerx_explorer_index::{IndexError, Indexer, IngestOutcome, QueryError};
use layerx_proof::availability::{
    verify_chunk, AvailabilityClass, Chunk, RootCommitments, VerifiedChunk,
};
use layerx_proof::checkpoint::{
    checkpoint_id, Attestation, Certificate, Checkpoint, GuarantorKey, SettlementDomain,
};
use layerx_proof::merkle::{build_leaf_hash_proof, root};
use layerx_types::verify::VerificationLevel;
use layerx_wire::limits::PROTOCOL_VERSION;
use sha2::{Digest as _, Sha256};

const HEADER_HEX: &str = "000217010f010002020000002a0300000000000000070400000000000000080500000000000000010600000000000000040700000020070707070707070707070707070707070707070707070707070707070707070708000000200808080808080808080808080808080808080808080808080808080808080808090000002091ed12e8565698680de301805638f596971c38d675d0258fd6827008587d2ccf0a00000020616323e29dec4e7e5b8ce8e23fd9c440d41e9a4b7aed8fa1912e739e9319066c0b000000203977f389195d255de7f536f64e62e68c99ca9e4fd9cb72e66041fd6cb80de3e10c0000002012e44fb808b082f72b3f7fecf45d9fb45d5c693bd8e98599c9ca53e2a9a48f0e0d000000202a6b085ba8513ee8878a31da25d7f2a059f197ed0637afe18def06f2a7b4841f0e00000000000003e80f000000200f0f0f0f0f0f0f0f0f0f0f0f0f0f0f0f0f0f0f0f0f0f0f0f0f0f0f0f0f0f0f0f";
const AVAILABILITY_ROOT: [u8; 32] = [
    0x12, 0xe4, 0x4f, 0xb8, 0x08, 0xb0, 0x82, 0xf7, 0x2b, 0x3f, 0x7f, 0xec, 0xf4, 0x5d, 0x9f, 0xb4,
    0x5d, 0x5c, 0x69, 0x3b, 0xd8, 0xe9, 0x85, 0x99, 0xc9, 0xca, 0x53, 0xe2, 0xa9, 0xa4, 0x8f, 0x0e,
];

fn decode_hex(value: &str) -> Vec<u8> {
    assert_eq!(value.len() % 2, 0, "hex vector must have pairs");
    value
        .as_bytes()
        .chunks_exact(2)
        .map(|pair| (nibble(pair[0]) << 4) | nibble(pair[1]))
        .collect()
}

fn nibble(value: u8) -> u8 {
    match value {
        b'0'..=b'9' => value - b'0',
        b'a'..=b'f' => value - b'a' + 10,
        _ => panic!("non-lowercase-hex test vector"),
    }
}

fn framed(bytes: &[u8]) -> Vec<u8> {
    let length = u32::try_from(bytes.len())
        .unwrap_or_else(|error| panic!("fixture record too long: {error}"));
    let mut encoded = length.to_be_bytes().to_vec();
    encoded.extend_from_slice(bytes);
    encoded
}

fn tagged(kind: u8, bytes: &[u8]) -> Vec<u8> {
    let mut encoded = vec![kind];
    encoded.extend_from_slice(&framed(bytes));
    encoded
}

fn chunk_digest(batch_number: u64, index: u32, class: AvailabilityClass, bytes: &[u8]) -> [u8; 32] {
    let length = u32::try_from(bytes.len())
        .unwrap_or_else(|error| panic!("fixture chunk too long: {error}"));
    let mut hasher = Sha256::new();
    hasher.update(b"LXP/v1/da-chunk\0");
    hasher.update(batch_number.to_be_bytes());
    hasher.update(index.to_be_bytes());
    hasher.update([class as u8]);
    hasher.update(0_u64.to_be_bytes());
    hasher.update(length.to_be_bytes());
    hasher.update(bytes);
    hasher.finalize().into()
}

fn availability_result() -> AvailabilityResult {
    let records = AvailabilityRecords {
        activities: vec![b"activity-public-1".to_vec()],
        receipts: vec![b"receipt-public-1".to_vec(), b"receipt-public-2".to_vec()],
        events: vec![b"event-public-1".to_vec(), b"event-public-2".to_vec()],
        oracle_inputs: vec![b"oracle-public-1".to_vec()],
    };
    let activities = framed(&records.activities[0]);
    let mut receipts = tagged(1, &records.receipts[0]);
    receipts.extend_from_slice(&tagged(1, &records.receipts[1]));
    receipts.extend_from_slice(&tagged(2, &records.events[0]));
    receipts.extend_from_slice(&tagged(2, &records.events[1]));
    let oracle = framed(&records.oracle_inputs[0]);
    let sections = [
        (AvailabilityClass::Activities, activities),
        (AvailabilityClass::Receipts, receipts),
        (AvailabilityClass::Oracle, oracle),
        (AvailabilityClass::StateDiff, b"state-diff-public".to_vec()),
        (AvailabilityClass::Recovery, b"recovery-public".to_vec()),
    ];
    let chunks: Vec<_> = sections
        .into_iter()
        .enumerate()
        .map(|(index, (class, bytes))| {
            let index = u32::try_from(index)
                .unwrap_or_else(|error| panic!("fixture index overflow: {error}"));
            Chunk {
                batch_number: 8,
                index,
                class,
                class_offset: 0,
                claimed_hash: chunk_digest(8, index, class, &bytes),
                bytes,
            }
        })
        .collect();
    let hashes: Vec<_> = chunks.iter().map(|chunk| chunk.claimed_hash).collect();
    let mut verified = Vec::<VerifiedChunk>::new();
    for (index, chunk) in chunks.into_iter().enumerate() {
        let (proof, availability_root) = build_leaf_hash_proof(&hashes, index)
            .unwrap_or_else(|error| panic!("fixture availability proof failed: {error:?}"));
        assert_eq!(availability_root, AVAILABILITY_ROOT);
        verified.push(
            verify_chunk(chunk, &proof, 8, &availability_root)
                .unwrap_or_else(|error| panic!("fixture chunk failed: {error:?}")),
        );
    }
    let roots = RootCommitments {
        activity: root(&[records.activities[0].as_slice()])
            .unwrap_or_else(|error| panic!("activity root failed: {error:?}")),
        receipt: root(&[
            records.receipts[0].as_slice(),
            records.receipts[1].as_slice(),
        ])
        .unwrap_or_else(|error| panic!("receipt root failed: {error:?}")),
        event: root(&[records.events[0].as_slice(), records.events[1].as_slice()])
            .unwrap_or_else(|error| panic!("event root failed: {error:?}")),
        oracle: root(&[records.oracle_inputs[0].as_slice()])
            .unwrap_or_else(|error| panic!("oracle root failed: {error:?}")),
    };
    AvailabilityResult::from_verified("node-boundary".to_owned(), verified, records, roots)
        .unwrap_or_else(|error| panic!("complete availability fixture failed: {error:?}"))
}

fn key(value: u8) -> (SigningKey, [u8; 33], [u8; 32]) {
    let mut scalar = [0_u8; 32];
    scalar[31] = value;
    let signing = SigningKey::from_bytes((&scalar).into())
        .unwrap_or_else(|error| panic!("invalid fixture signing key: {error}"));
    let encoded = signing.verifying_key().to_encoded_point(true);
    let public_key = encoded
        .as_bytes()
        .try_into()
        .unwrap_or_else(|_| panic!("invalid compressed key width"));
    let mut identifier = [0_u8; 32];
    identifier[0] = value;
    (signing, public_key, identifier)
}

fn attestation(
    identifier: [u8; 32],
    guarantor_id: [u8; 32],
    signing_key: &SigningKey,
) -> Attestation {
    let settlement_contract = [0x55; 20];
    let mut message = [0_u8; 189];
    message[..2].copy_from_slice(&PROTOCOL_VERSION.to_be_bytes());
    message[2..6].copy_from_slice(&42_u32.to_be_bytes());
    message[6..14].copy_from_slice(&31_337_u64.to_be_bytes());
    message[14..34].copy_from_slice(&settlement_contract);
    message[34..42].copy_from_slice(&7_u64.to_be_bytes());
    message[42..74].copy_from_slice(&identifier);
    message[74..106].copy_from_slice(&identifier);
    message[106..138].copy_from_slice(&guarantor_id);
    message[138..146].copy_from_slice(&8_u64.to_be_bytes());
    message[146..178].copy_from_slice(&AVAILABILITY_ROOT);
    message[178] = 1;
    message[179] = 1;
    message[180] = 0x1f;
    message[181..].copy_from_slice(&(1_000 + u64::from(guarantor_id[0])).to_be_bytes());
    let mut hasher = Sha256::new();
    hasher.update(b"LXP/v1/guarantor-attestation\0");
    hasher.update(message);
    let digest: [u8; 32] = hasher.finalize().into();
    let (signature, recovery_id): (Signature, _) = signing_key
        .sign_prehash_recoverable(&digest)
        .unwrap_or_else(|error| panic!("fixture attestation signing failed: {error}"));
    let signer = secp256k1::evm_address(
        signing_key
            .verifying_key()
            .to_encoded_point(true)
            .as_bytes(),
    )
    .unwrap_or_else(|error| panic!("fixture attestation signer failed: {error:?}"));
    Attestation::new(
        PROTOCOL_VERSION,
        42,
        31_337,
        settlement_contract,
        7,
        identifier,
        identifier,
        guarantor_id,
        8,
        AVAILABILITY_ROOT,
        true,
        true,
        0x1f,
        1_000 + u64::from(guarantor_id[0]),
        signer,
        signature.to_bytes().into(),
        27 + u8::from(recovery_id),
    )
}

fn checkpoint_fixture() -> (Certificate, Vec<GuarantorKey>, [u8; 32]) {
    let checkpoint = Checkpoint::new(decode_hex(HEADER_HEX), b"EXPLORER-PROOF".to_vec());
    let identifier = checkpoint_id(&checkpoint)
        .unwrap_or_else(|error| panic!("checkpoint identifier failed: {error:?}"));
    let mut attestations = Vec::new();
    let mut keys = Vec::new();
    for value in 1..=3 {
        let (signing, public_key, guarantor_id) = key(value);
        attestations.push(attestation(identifier, guarantor_id, &signing));
        keys.push(GuarantorKey::new(guarantor_id, public_key, true));
    }
    (
        Certificate::new(checkpoint, attestations, 2, None),
        keys,
        identifier,
    )
}

#[test]
fn deleting_and_rebuilding_from_verified_boundary_evidence_is_identical() {
    let availability = availability_result();
    let (certificate, keys, checkpoint_id) = checkpoint_fixture();
    let head = Head {
        chain_sequence: 4,
        sealed_batch: 8,
        finalised_checkpoint: checkpoint_id,
    };
    let mut original = Indexer::new(head);
    assert_eq!(
        original.ingest_availability(&availability),
        Ok(IngestOutcome::Inserted)
    );
    assert_eq!(
        original.ingest_checkpoint(
            &certificate,
            &keys,
            checkpoint_id,
            SettlementDomain::new(31_337, [0x55; 20]),
            None,
        ),
        Ok(IngestOutcome::Inserted)
    );
    assert!(original.freshness().is_current());
    assert_eq!(
        original.ingest_availability(&availability),
        Ok(IngestOutcome::AlreadyPresent)
    );
    assert_eq!(
        original.ingest_checkpoint(
            &certificate,
            &keys,
            checkpoint_id,
            SettlementDomain::new(31_337, [0x55; 20]),
            None,
        ),
        Ok(IngestOutcome::AlreadyPresent)
    );
    let expected = original.snapshot();
    assert_eq!(expected.receipts.len(), 2);
    assert_eq!(expected.events.len(), 2);
    assert!(expected
        .receipts
        .iter()
        .all(|record| record.verification_level == VerificationLevel::CHECKPOINT_FINALISED));

    drop(original);
    let mut rebuilt = Indexer::new(head);
    assert_eq!(
        rebuilt.ingest_checkpoint(
            &certificate,
            &keys,
            checkpoint_id,
            SettlementDomain::new(31_337, [0x55; 20]),
            None,
        ),
        Ok(IngestOutcome::Inserted)
    );
    assert_eq!(
        rebuilt.ingest_availability(&availability),
        Ok(IngestOutcome::Inserted)
    );
    assert_eq!(rebuilt.snapshot(), expected);
    let batch = rebuilt
        .batch(8)
        .value
        .unwrap_or_else(|| panic!("rebuilt batch missing"));
    let receipt = rebuilt
        .receipt(batch.receipt_ids[0])
        .value
        .unwrap_or_else(|| panic!("rebuilt receipt missing"));
    assert_eq!(receipt.canonical_bytes, b"receipt-public-1");
    assert!(rebuilt.batch(8).freshness.is_current());

    let verifier = Verifier::new(Vec::new(), Vec::new());
    let public = rebuilt.public(&verifier);
    let checkpoints = public
        .checkpoints(None, 10)
        .unwrap_or_else(|failure| panic!("checkpoint browse failed: {failure:?}"));
    assert_eq!(checkpoints.value.items.len(), 1);
    assert_eq!(checkpoints.value.items[0].checkpoint_id, checkpoint_id);
    assert_eq!(
        checkpoints.value.items[0].verification_level,
        VerificationLevel::CHECKPOINT_FINALISED
    );
    assert!(checkpoints.freshness.is_current());
    let batches = public
        .batches(None, 10)
        .unwrap_or_else(|failure| panic!("batch browse failed: {failure:?}"));
    assert_eq!(batches.value.items.len(), 1);
    assert_eq!(batches.value.items[0].batch_number, 8);
    assert_eq!(
        batches.value.items[0].verification_level,
        VerificationLevel::CHECKPOINT_FINALISED
    );
    assert!(batches.freshness.is_current());
    let Err(invalid) = public.batches(None, 0) else {
        panic!("zero-sized public page unexpectedly succeeded");
    };
    assert_eq!(invalid.error, QueryError::InvalidPageSize);
    assert!(invalid.freshness.is_current());
    let Err(incomplete) = public.account_activity([5; 32], None, 10) else {
        panic!("unverified account index unexpectedly answered");
    };
    assert_eq!(
        incomplete.error,
        QueryError::AccountIndexIncomplete { batch: 8 }
    );
    assert!(incomplete.freshness.is_current());
}

#[test]
fn freshness_and_evidence_mismatches_are_refused_without_partial_rows() {
    let availability = availability_result();
    let (certificate, keys, checkpoint_id) = checkpoint_fixture();
    let behind_head = Head {
        chain_sequence: 3,
        sealed_batch: 7,
        finalised_checkpoint: [7; 32],
    };
    let mut behind = Indexer::new(behind_head);
    assert_eq!(
        behind.ingest_availability(&availability),
        Err(IndexError::BatchAheadOfHead { batch: 8, head: 7 })
    );
    assert_eq!(behind.snapshot().batches.len(), 0);

    let wrong_checkpoint_head = Head {
        chain_sequence: 4,
        sealed_batch: 8,
        finalised_checkpoint: [9; 32],
    };
    let mut mismatched = Indexer::new(wrong_checkpoint_head);
    assert_eq!(
        mismatched.ingest_checkpoint(
            &certificate,
            &keys,
            checkpoint_id,
            SettlementDomain::new(31_337, [0x55; 20]),
            None,
        ),
        Err(IndexError::CheckpointHeadMismatch {
            expected: [9; 32],
            actual: checkpoint_id,
        })
    );
    assert_eq!(mismatched.snapshot().checkpoints.len(), 0);

    let mut current = Indexer::new(Head {
        chain_sequence: 4,
        sealed_batch: 8,
        finalised_checkpoint: checkpoint_id,
    });
    assert_eq!(
        current.ingest_checkpoint(
            &certificate,
            &keys,
            checkpoint_id,
            SettlementDomain::new(31_337, [0x55; 20]),
            None,
        ),
        Ok(IngestOutcome::Inserted)
    );
    assert!(!current.freshness().is_current());
    assert_eq!(current.freshness().batches_behind(), 9);
    assert_eq!(
        current.ingest_availability(&availability),
        Ok(IngestOutcome::Inserted)
    );
    assert!(current.freshness().is_current());
    current
        .refresh_head(Head {
            chain_sequence: 5,
            sealed_batch: 9,
            finalised_checkpoint: [10; 32],
        })
        .unwrap_or_else(|error| panic!("head advance failed: {error:?}"));
    assert!(!current.freshness().is_current());
    assert_eq!(current.freshness().batches_behind(), 1);
    assert_eq!(
        current.refresh_head(Head {
            chain_sequence: 4,
            sealed_batch: 8,
            finalised_checkpoint: checkpoint_id,
        }),
        Err(IndexError::HeadRegression)
    );
}

#[test]
fn availability_result_exposes_only_record_sets_that_reverify() {
    let result = availability_result();
    let mut records = result.records().clone();
    records.receipts[0][0] ^= 1;
    let refused = AvailabilityResult::from_verified(
        "node-boundary".to_owned(),
        result.chunks.clone(),
        records,
        result.record_roots(),
    );
    assert!(
        refused.is_err(),
        "altered receipt set must not become indexable"
    );
}
