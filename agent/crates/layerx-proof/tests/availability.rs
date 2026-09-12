use layerx_proof::availability::{
    verify_chunk, verify_reassembled, AvailabilityCheck, AvailabilityClass, Chunk,
    ReassembledRecords, RootCommitments, VerifiedChunk,
};
use layerx_proof::merkle::{build_leaf_hash_proof, root};
use layerx_wire::encode::Encoder;
use layerx_wire::hash::availability_chunk_digest;

fn sequence(record: &[u8]) -> Vec<u8> {
    let mut encoder = Encoder::new(1_048_576);
    assert_eq!(encoder.sequence_length(1, 65_535), Ok(()));
    assert_eq!(encoder.bytes(record, 1_048_576), Ok(()));
    encoder.finish()
}

fn tagged(kind: u8, record: &[u8]) -> Vec<u8> {
    let mut encoder = Encoder::new(1_048_576);
    assert_eq!(encoder.u8(kind), Ok(()));
    assert_eq!(encoder.bytes(record, 1_048_576), Ok(()));
    encoder.finish()
}

fn chunks() -> (Vec<Chunk>, Vec<layerx_proof::merkle::Proof>, [u8; 32]) {
    let mut receipts = tagged(1, b"receipt");
    receipts.extend_from_slice(&tagged(2, b"event"));
    let sections = [
        (AvailabilityClass::Activities, sequence(b"activity")),
        (AvailabilityClass::Receipts, receipts),
        (AvailabilityClass::Oracle, sequence(b"oracle")),
        (AvailabilityClass::StateDiff, b"d".to_vec()),
        (AvailabilityClass::Recovery, b"e".to_vec()),
    ];
    let chunks = sections
        .into_iter()
        .enumerate()
        .map(|(index, (class, bytes))| Chunk {
            batch_number: 7,
            index: u32::try_from(index).unwrap_or_else(|_| panic!("index")),
            class,
            class_offset: 0,
            bytes,
            claimed_hash: [0; 32],
        })
        .collect();
    authenticate(chunks)
}

fn authenticate(
    mut chunks: Vec<Chunk>,
) -> (Vec<Chunk>, Vec<layerx_proof::merkle::Proof>, [u8; 32]) {
    for chunk in &mut chunks {
        chunk.claimed_hash = availability_chunk_digest(
            chunk.batch_number,
            chunk.index,
            chunk.class as u8,
            chunk.class_offset,
            &chunk.bytes,
        )
        .unwrap_or_else(|error| panic!("digest: {error:?}"));
    }
    let hashes: Vec<_> = chunks.iter().map(|chunk| chunk.claimed_hash).collect();
    let mut proofs = Vec::new();
    let mut root_value = [0; 32];
    for index in 0..hashes.len() {
        let (proof, computed) = build_leaf_hash_proof(&hashes, index)
            .unwrap_or_else(|error| panic!("proof build failed: {error:?}"));
        proofs.push(proof);
        root_value = computed;
    }
    (chunks, proofs, root_value)
}

fn verified(chunks: Vec<Chunk>) -> Vec<VerifiedChunk> {
    let (chunks, proofs, commitment) = authenticate(chunks);
    chunks
        .into_iter()
        .zip(proofs)
        .map(|(chunk, proof)| {
            let batch = chunk.batch_number;
            verify_chunk(chunk, &proof, batch, &commitment)
                .unwrap_or_else(|error| panic!("valid inclusion: {error:?}"))
        })
        .collect()
}

fn records<'a>() -> (ReassembledRecords<'a>, RootCommitments) {
    static ACTIVITIES: [&[u8]; 1] = [b"activity"];
    static RECEIPTS: [&[u8]; 1] = [b"receipt"];
    static EVENTS: [&[u8]; 1] = [b"event"];
    static ORACLE: [&[u8]; 1] = [b"oracle"];
    let commitments = RootCommitments {
        activity: root(&ACTIVITIES)
            .unwrap_or_else(|error| panic!("activity root failed: {error:?}")),
        receipt: root(&RECEIPTS).unwrap_or_else(|error| panic!("receipt root failed: {error:?}")),
        event: root(&EVENTS).unwrap_or_else(|error| panic!("event root failed: {error:?}")),
        oracle: root(&ORACLE).unwrap_or_else(|error| panic!("oracle root failed: {error:?}")),
    };
    (
        ReassembledRecords {
            activities: &ACTIVITIES,
            receipts: &RECEIPTS,
            events: &EVENTS,
            oracle_inputs: &ORACLE,
        },
        commitments,
    )
}

#[test]
fn verifies_chunks_all_classes_and_reassembled_roots() {
    let (chunks, proofs, root_value) = chunks();
    let verified: Vec<_> = chunks
        .into_iter()
        .zip(&proofs)
        .map(|(chunk, proof)| {
            verify_chunk(chunk, proof, 7, &root_value)
                .unwrap_or_else(|error| panic!("valid chunk failed: {error:?}"))
        })
        .collect();
    let (records, commitments) = records();
    let report = verify_reassembled(&verified, &records, commitments)
        .unwrap_or_else(|error| panic!("valid reassembly failed: {error:?}"));
    assert_eq!(report.classes.obtained.len(), 5);
    assert!(report.classes.missing.is_empty());
    assert_eq!(report.total_bytes, 54);
}

#[test]
fn retains_evidence_for_altered_and_cross_batch_chunks() {
    let (mut chunks, proofs, root_value) = chunks();
    chunks[0].bytes[0] ^= 1;
    let altered = verify_chunk(chunks[0].clone(), &proofs[0], 7, &root_value)
        .err()
        .unwrap_or_else(|| panic!("altered chunk verified"));
    assert_eq!(altered.check, AvailabilityCheck::ChunkHash);
    assert_eq!(altered.served_bytes, chunks[0].bytes);
    assert_eq!(altered.commitment, chunks[0].claimed_hash);

    chunks[1].batch_number = 8;
    let wrong_batch = verify_chunk(chunks[1].clone(), &proofs[1], 7, &root_value)
        .err()
        .unwrap_or_else(|| panic!("cross-batch chunk verified"));
    assert_eq!(wrong_batch.check, AvailabilityCheck::BatchNumber);
    assert_eq!(wrong_batch.served_bytes, chunks[1].bytes);
}

#[test]
fn rejects_reordered_chunks_withheld_classes_and_root_mismatch() {
    let (chunks, proofs, root_value) = chunks();
    let mut verified: Vec<_> = chunks
        .into_iter()
        .zip(&proofs)
        .map(|(chunk, proof)| {
            verify_chunk(chunk, proof, 7, &root_value)
                .unwrap_or_else(|error| panic!("valid chunk failed: {error:?}"))
        })
        .collect();
    let (records, commitments) = records();

    verified.swap(0, 1);
    let reordered = verify_reassembled(&verified, &records, commitments)
        .err()
        .unwrap_or_else(|| panic!("reordered chunks verified"));
    assert_eq!(reordered.check, AvailabilityCheck::ChunkOrder);
    verified.swap(0, 1);

    let withheld = verify_reassembled(&verified[..4], &records, commitments)
        .err()
        .unwrap_or_else(|| panic!("withheld class verified"));
    assert_eq!(withheld.check, AvailabilityCheck::MissingClass);
    assert_eq!(withheld.classes.missing, vec![AvailabilityClass::Recovery]);

    let mut wrong = commitments;
    wrong.event[0] ^= 1;
    let mismatch = verify_reassembled(&verified, &records, wrong)
        .err()
        .unwrap_or_else(|| panic!("mismatching event root verified"));
    assert_eq!(mismatch.check, AvailabilityCheck::EventRoot);
    assert_eq!(mismatch.commitment, wrong.event);
    assert_eq!(mismatch.served_bytes.len(), 54);
}

#[test]
fn unrelated_caller_records_cannot_authenticate_unrelated_chunk_bytes() {
    let (mut chunks, _, _) = chunks();
    for (index, chunk) in chunks.iter_mut().enumerate() {
        chunk.bytes = vec![b'a' + u8::try_from(index).unwrap_or(0)];
    }
    let material = verified(chunks);
    let (records, commitments) = records();
    let failure = verify_reassembled(&material, &records, commitments)
        .err()
        .unwrap_or_else(|| panic!("the former five-byte positive must fail canonical decoding"));
    assert_eq!(failure.check, AvailabilityCheck::RecordEncoding);
    assert_eq!(failure.served_bytes, b"abcde");
    assert_eq!(failure.served_bytes.len(), 5);
    assert_eq!(failure.commitment, material[0].data_availability_root());

    let (chunks, _, _) = self::chunks();
    let material = verified(chunks);
    let substituted = [b"substituted".as_slice()];
    let supplied = ReassembledRecords {
        activities: &substituted,
        ..records.clone()
    };
    let mut substituted_roots = commitments;
    substituted_roots.activity =
        root(&substituted).unwrap_or_else(|error| panic!("record root: {error:?}"));
    let failure = verify_reassembled(&material, &supplied, substituted_roots)
        .err()
        .unwrap_or_else(|| {
            panic!("self-consistent caller records and roots are unrelated to authenticated bytes")
        });
    assert_eq!(failure.check, AvailabilityCheck::ActivityRoot);
    assert_eq!(failure.commitment, substituted_roots.activity);
    let failure = verify_reassembled(&material, &supplied, commitments)
        .err()
        .unwrap_or_else(|| {
            panic!("supplied records must equal decoded records even with genuine roots")
        });
    assert_eq!(failure.check, AvailabilityCheck::RecordBinding);
}

#[test]
fn all_classes_do_not_prove_withheld_state_or_recovery_suffixes() {
    for class in [AvailabilityClass::StateDiff, AvailabilityClass::Recovery] {
        let (mut chunks, _, _) = chunks();
        let first = chunks
            .iter()
            .position(|chunk| chunk.class == class)
            .unwrap_or_else(|| panic!("class absent"));
        let mut tail = chunks[first].clone();
        tail.class_offset = 1;
        tail.bytes = b"unavailable-tail".to_vec();
        chunks.insert(first + 1, tail);
        for (index, chunk) in chunks.iter_mut().enumerate() {
            chunk.index = u32::try_from(index).unwrap_or_else(|error| panic!("index: {error:?}"));
        }
        let mut material = verified(chunks);
        let (records, commitments) = records();
        assert!(verify_reassembled(&material, &records, commitments).is_ok());
        material.remove(first + 1);
        let failure = verify_reassembled(&material, &records, commitments)
            .err()
            .unwrap_or_else(|| panic!("all five prefixes cannot prove full availability"));
        assert_eq!(failure.check, AvailabilityCheck::BundleCompleteness);
        assert!(failure.classes.missing.is_empty());
        assert_eq!(failure.commitment, material[0].data_availability_root());
    }
}

#[test]
fn duplicate_reordered_cross_root_and_cross_batch_material_is_refused() {
    let (chunks, _, _) = chunks();
    let material = verified(chunks.clone());
    let (records, commitments) = records();
    let mut duplicate = material.clone();
    duplicate.insert(1, material[0].clone());
    assert_eq!(
        verify_reassembled(&duplicate, &records, commitments)
            .err()
            .unwrap_or_else(|| panic!("duplicate"))
            .check,
        AvailabilityCheck::ChunkOrder
    );
    let mut cross_root_chunks = chunks.clone();
    cross_root_chunks[4].bytes.push(0);
    let different_root = verified(cross_root_chunks);
    let mut mixed = material.clone();
    mixed[4] = different_root[4].clone();
    assert_eq!(
        verify_reassembled(&mixed, &records, commitments)
            .err()
            .unwrap_or_else(|| panic!("cross root"))
            .check,
        AvailabilityCheck::ChunkOrder
    );
    let mut cross_batch_chunks = chunks;
    for chunk in &mut cross_batch_chunks {
        chunk.batch_number = 8;
    }
    let different_batch = verified(cross_batch_chunks);
    mixed[4] = different_batch[4].clone();
    assert_eq!(
        verify_reassembled(&mixed, &records, commitments)
            .err()
            .unwrap_or_else(|| panic!("cross batch"))
            .check,
        AvailabilityCheck::ChunkOrder
    );
}

#[test]
fn authenticated_noncanonical_class_order_offsets_and_empty_chunks_are_refused() {
    let (chunks, _, _) = chunks();
    let (records, commitments) = records();
    let mut wrong_order = chunks.clone();
    wrong_order.swap(0, 1);
    for (index, chunk) in wrong_order.iter_mut().enumerate() {
        chunk.index = u32::try_from(index).unwrap_or_else(|error| panic!("index: {error:?}"));
    }
    assert_eq!(
        verify_reassembled(&verified(wrong_order), &records, commitments)
            .err()
            .unwrap_or_else(|| panic!("class order"))
            .check,
        AvailabilityCheck::ChunkOrder
    );
    let mut wrong_offset = chunks.clone();
    wrong_offset[3].class_offset = 1;
    assert_eq!(
        verify_reassembled(&verified(wrong_offset), &records, commitments)
            .err()
            .unwrap_or_else(|| panic!("class offset"))
            .check,
        AvailabilityCheck::ClassOffset
    );
    let mut empty = chunks;
    let mut extra = empty[4].clone();
    extra.index = 5;
    extra.class_offset = 1;
    extra.bytes.clear();
    empty.push(extra);
    assert_eq!(
        verify_reassembled(&verified(empty), &records, commitments)
            .err()
            .unwrap_or_else(|| panic!("empty tail"))
            .check,
        AvailabilityCheck::ClassOffset
    );
}

#[test]
fn canonical_record_count_lengths_tags_and_trailing_bytes_are_enforced() {
    let (chunks, _, _) = chunks();
    let (records, commitments) = records();
    let mut wrong_count = chunks[0].bytes.clone();
    wrong_count[..4].copy_from_slice(&2_u32.to_be_bytes());
    let mut trailing = chunks[0].bytes.clone();
    trailing.push(0);
    let mut excessive = chunks[0].bytes.clone();
    excessive[..4].copy_from_slice(&65_536_u32.to_be_bytes());
    for bytes in [
        chunks[0].bytes[4..].to_vec(),
        wrong_count,
        trailing,
        excessive,
        chunks[0].bytes[..chunks[0].bytes.len() - 1].to_vec(),
    ] {
        let mut altered = chunks.clone();
        altered[0].bytes = bytes;
        assert_eq!(
            verify_reassembled(&verified(altered), &records, commitments)
                .err()
                .unwrap_or_else(|| panic!("canonical record encoding"))
                .check,
            AvailabilityCheck::RecordEncoding
        );
    }
    for bytes in [
        tagged(0, b"receipt"),
        tagged(3, b"receipt"),
        [tagged(2, b"event"), tagged(1, b"receipt")].concat(),
    ] {
        let mut altered = chunks.clone();
        altered[1].bytes = bytes;
        assert_eq!(
            verify_reassembled(&verified(altered), &records, commitments)
                .err()
                .unwrap_or_else(|| panic!("canonical receipt tags"))
                .check,
            AvailabilityCheck::RecordEncoding
        );
    }
}

#[test]
fn chunk_size_offset_and_leaf_count_bounds_match_native_verification() {
    let (chunks, proofs, commitment) = chunks();
    let mut oversized = chunks[0].clone();
    oversized.bytes = vec![0; 65_537];
    assert_eq!(
        verify_chunk(oversized, &proofs[0], 7, &commitment)
            .err()
            .unwrap_or_else(|| panic!("chunk size"))
            .check,
        AvailabilityCheck::ChunkBounds
    );
    let mut overflow = chunks[0].clone();
    overflow.class_offset = u64::MAX;
    assert_eq!(
        verify_chunk(overflow, &proofs[0], 7, &commitment)
            .err()
            .unwrap_or_else(|| panic!("offset overflow"))
            .check,
        AvailabilityCheck::ChunkBounds
    );
    let proof = layerx_proof::merkle::Proof::new(0, 4097, vec![[0; 32]; 13])
        .unwrap_or_else(|error| panic!("proof shape: {error:?}"));
    assert_eq!(
        verify_chunk(chunks[0].clone(), &proof, 7, &commitment)
            .err()
            .unwrap_or_else(|| panic!("chunk count"))
            .check,
        AvailabilityCheck::ChunkBounds
    );
}

#[test]
fn individually_valid_proofs_cannot_disagree_about_bundle_leaf_count() {
    let (chunks, proofs, commitment) = chunks();
    let mut material = verified(chunks.clone());
    let alternate = layerx_proof::merkle::Proof::new(0, 6, proofs[0].siblings().to_vec())
        .unwrap_or_else(|error| panic!("same-depth alternate count: {error:?}"));
    material[0] =
        verify_chunk(chunks[0].clone(), &alternate, 7, &commitment).unwrap_or_else(|error| {
            panic!("this interior leaf path alone cannot establish total leaf count: {error:?}")
        });
    let (records, commitments) = records();
    assert_eq!(
        verify_reassembled(&material, &records, commitments)
            .err()
            .unwrap_or_else(|| panic!("mixed proof counts"))
            .check,
        AvailabilityCheck::BundleCompleteness
    );
}
