//! Data-availability chunk, class, and committed-root verification.

use std::collections::BTreeSet;

use layerx_wire::decode::Decoder;
use layerx_wire::hash::availability_chunk_digest;
use layerx_wire::limits::MAX_MESSAGE_BYTES;

use crate::merkle::{root, root_from_leaf_hashes, verify_leaf_hash, MerkleError, Proof};

const MAX_CHUNKS: usize = 4096;
const MAX_CHUNK_BYTES: usize = 65_536;
const MAX_SECTION_BYTES: usize = 16_777_216;
const MAX_RECORDS: usize = 65_535;

/// The five and only five protocol availability classes.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
#[repr(u8)]
pub enum AvailabilityClass {
    /// Canonical accepted activities.
    Activities = 1,
    /// Canonical activity receipts.
    Receipts = 2,
    /// Canonical signed oracle inputs.
    Oracle = 3,
    /// Canonical state-diff material.
    StateDiff = 4,
    /// Canonical recovery metadata.
    Recovery = 5,
}

impl AvailabilityClass {
    const ALL: [Self; 5] = [
        Self::Activities,
        Self::Receipts,
        Self::Oracle,
        Self::StateDiff,
        Self::Recovery,
    ];
}

/// One exact chunk response and its claimed digest.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Chunk {
    /// Batch that produced the chunk.
    pub batch_number: u64,
    /// Flattened bundle index committed by the Merkle proof.
    pub index: u32,
    /// Availability section containing the bytes.
    pub class: AvailabilityClass,
    /// Byte offset inside the section.
    pub class_offset: u64,
    /// Exact served bytes.
    pub bytes: Vec<u8>,
    /// Digest claimed by the provider.
    pub claimed_hash: [u8; 32],
}

/// A chunk whose metadata digest and inclusion path both passed.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct VerifiedChunk {
    chunk: Chunk,
    data_availability_root: [u8; 32],
    leaf_count: u32,
}

impl VerifiedChunk {
    /// Borrows the exact verified provider response.
    #[must_use]
    pub const fn chunk(&self) -> &Chunk {
        &self.chunk
    }

    /// Returns the availability root against which this exact chunk passed.
    #[must_use]
    pub const fn data_availability_root(&self) -> [u8; 32] {
        self.data_availability_root
    }
}

/// Exact availability verification stage.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum AvailabilityCheck {
    /// Chunk batch differs from the requested batch.
    BatchNumber,
    /// Chunk and proof indices differ.
    ChunkIndex,
    /// Provider's claimed digest differs from recomputation.
    ChunkHash,
    /// Inclusion under the availability root failed.
    ChunkInclusion,
    /// Verified chunks were not in strict flattened-index order.
    ChunkOrder,
    /// Section offsets were not contiguous.
    ClassOffset,
    /// At least one required class was withheld.
    MissingClass,
    /// Reassembled activity bytes did not match their root.
    ActivityRoot,
    /// Reassembled receipt bytes did not match their root.
    ReceiptRoot,
    /// Reassembled event bytes did not match their root.
    EventRoot,
    /// Reassembled oracle bytes did not match their root.
    OracleRoot,
    /// The response does not cover every leaf in its committed bundle.
    BundleCompleteness,
    /// Recomputed complete-bundle root differs from the chunk proof root.
    AvailabilityRoot,
    /// A section does not use the canonical bounded record encoding.
    RecordEncoding,
    /// Caller records differ from records decoded from authenticated chunks.
    RecordBinding,
    /// Chunk metadata exceeds the native protocol bounds.
    ChunkBounds,
}

/// Availability failure evidence retained for audit and provider attribution.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AvailabilityFailure {
    /// Exact failed stage.
    pub check: AvailabilityCheck,
    /// Exact bytes served by the failing provider.
    pub served_bytes: Vec<u8>,
    /// Commitment the served bytes failed against.
    pub commitment: [u8; 32],
    /// Classes obtained before the failure was established.
    pub classes: ClassReport,
}

/// Explicit obtained and missing class sets.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ClassReport {
    /// Classes present in the verified response.
    pub obtained: Vec<AvailabilityClass>,
    /// Classes absent from the verified response.
    pub missing: Vec<AvailabilityClass>,
}

fn class_report(chunks: &[VerifiedChunk]) -> ClassReport {
    let obtained_set: BTreeSet<_> = chunks.iter().map(|chunk| chunk.chunk.class).collect();
    let obtained = AvailabilityClass::ALL
        .into_iter()
        .filter(|class| obtained_set.contains(class))
        .collect();
    let missing = AvailabilityClass::ALL
        .into_iter()
        .filter(|class| !obtained_set.contains(class))
        .collect();
    ClassReport { obtained, missing }
}

/// The canonical record streams decoded from the reassembled sections.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ReassembledRecords<'a> {
    /// Canonical activity records, in global-sequence order.
    pub activities: &'a [&'a [u8]],
    /// Canonical receipt records, in global-sequence order.
    pub receipts: &'a [&'a [u8]],
    /// Canonical event records recovered from the receipt/event material.
    pub events: &'a [&'a [u8]],
    /// Canonical signed oracle inputs, in committed order.
    pub oracle_inputs: &'a [&'a [u8]],
}

/// Exact owned record streams decoded from authenticated availability sections.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AvailabilityRecords {
    /// Canonical activities in committed order.
    pub activities: Vec<Vec<u8>>,
    /// Canonical receipts in committed order.
    pub receipts: Vec<Vec<u8>>,
    /// Canonical events in committed order.
    pub events: Vec<Vec<u8>>,
    /// Canonical oracle inputs in committed order.
    pub oracle_inputs: Vec<Vec<u8>>,
}

impl AvailabilityRecords {
    /// Binds every record byte to a complete authenticated bundle and its roots.
    ///
    /// # Errors
    ///
    /// Returns retained provider evidence for malformed, incomplete or differing material.
    pub fn verify(
        &self,
        chunks: &[VerifiedChunk],
        commitments: RootCommitments,
    ) -> Result<ReassemblyReport, AvailabilityFailure> {
        let activities: Vec<_> = self.activities.iter().map(Vec::as_slice).collect();
        let receipts: Vec<_> = self.receipts.iter().map(Vec::as_slice).collect();
        let events: Vec<_> = self.events.iter().map(Vec::as_slice).collect();
        let oracle_inputs: Vec<_> = self.oracle_inputs.iter().map(Vec::as_slice).collect();
        verify_reassembled(
            chunks,
            &ReassembledRecords {
                activities: &activities,
                receipts: &receipts,
                events: &events,
                oracle_inputs: &oracle_inputs,
            },
            commitments,
        )
    }
}

/// Four signed batch commitments checked after section reassembly.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct RootCommitments {
    /// Header activity root.
    pub activity: [u8; 32],
    /// Header receipt root.
    pub receipt: [u8; 32],
    /// Header event root.
    pub event: [u8; 32],
    /// Header oracle root.
    pub oracle: [u8; 32],
}

/// Successful five-class reassembly and root-verification report.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ReassemblyReport {
    /// Explicitly complete availability-class report.
    pub classes: ClassReport,
    /// Total exact provider bytes retained across all chunks.
    pub total_bytes: usize,
}

fn no_classes() -> ClassReport {
    ClassReport {
        obtained: Vec::new(),
        missing: AvailabilityClass::ALL.to_vec(),
    }
}

/// Recomputes the chunk digest and verifies its index-aware Merkle path.
///
/// # Errors
///
/// Returns evidence retaining the exact served bytes and failed commitment.
pub fn verify_chunk(
    chunk: Chunk,
    proof: &Proof,
    expected_batch_number: u64,
    data_availability_root: &[u8; 32],
) -> Result<VerifiedChunk, AvailabilityFailure> {
    if chunk.bytes.len() > MAX_CHUNK_BYTES
        || usize::try_from(proof.leaf_count()).map_or(true, |count| count > MAX_CHUNKS)
        || u64::try_from(chunk.bytes.len())
            .ok()
            .and_then(|length| chunk.class_offset.checked_add(length))
            .is_none()
    {
        return Err(AvailabilityFailure {
            check: AvailabilityCheck::ChunkBounds,
            served_bytes: chunk.bytes,
            commitment: *data_availability_root,
            classes: no_classes(),
        });
    }
    if chunk.batch_number != expected_batch_number {
        return Err(AvailabilityFailure {
            check: AvailabilityCheck::BatchNumber,
            served_bytes: chunk.bytes,
            commitment: *data_availability_root,
            classes: no_classes(),
        });
    }
    if chunk.index != proof.leaf_index() {
        return Err(AvailabilityFailure {
            check: AvailabilityCheck::ChunkIndex,
            served_bytes: chunk.bytes,
            commitment: *data_availability_root,
            classes: no_classes(),
        });
    }
    let computed = availability_chunk_digest(
        chunk.batch_number,
        chunk.index,
        chunk.class as u8,
        chunk.class_offset,
        &chunk.bytes,
    )
    .map_err(|_| AvailabilityFailure {
        check: AvailabilityCheck::ChunkHash,
        served_bytes: chunk.bytes.clone(),
        commitment: chunk.claimed_hash,
        classes: no_classes(),
    })?;
    if computed != chunk.claimed_hash {
        return Err(AvailabilityFailure {
            check: AvailabilityCheck::ChunkHash,
            served_bytes: chunk.bytes,
            commitment: chunk.claimed_hash,
            classes: no_classes(),
        });
    }
    verify_leaf_hash(&computed, proof, data_availability_root).map_err(|_error: MerkleError| {
        AvailabilityFailure {
            check: AvailabilityCheck::ChunkInclusion,
            served_bytes: chunk.bytes.clone(),
            commitment: *data_availability_root,
            classes: no_classes(),
        }
    })?;
    Ok(VerifiedChunk {
        chunk,
        data_availability_root: *data_availability_root,
        leaf_count: proof.leaf_count(),
    })
}

fn all_served_bytes(chunks: &[VerifiedChunk]) -> Vec<u8> {
    let capacity = chunks.iter().fold(0_usize, |total, chunk| {
        total.saturating_add(chunk.chunk.bytes.len())
    });
    let mut bytes = Vec::with_capacity(capacity);
    for chunk in chunks {
        bytes.extend_from_slice(&chunk.chunk.bytes);
    }
    bytes
}

fn compare_root(
    records: &[&[u8]],
    expected: [u8; 32],
    check: AvailabilityCheck,
    chunks: &[VerifiedChunk],
    classes: &ClassReport,
) -> Result<(), AvailabilityFailure> {
    let computed = root(records).map_err(|_| AvailabilityFailure {
        check,
        served_bytes: all_served_bytes(chunks),
        commitment: expected,
        classes: classes.clone(),
    })?;
    if computed == expected {
        Ok(())
    } else {
        Err(AvailabilityFailure {
            check,
            served_bytes: all_served_bytes(chunks),
            commitment: expected,
            classes: classes.clone(),
        })
    }
}

/// Verifies ordering, section contiguity, class completeness, and all four
/// signed batch record roots after reassembly.
///
/// # Errors
///
/// Returns retained provider bytes, the mismatching commitment, and the class
/// report at the exact point of failure.
pub fn verify_reassembled(
    chunks: &[VerifiedChunk],
    records: &ReassembledRecords<'_>,
    commitments: RootCommitments,
) -> Result<ReassemblyReport, AvailabilityFailure> {
    let (decoded, report) = reassemble(chunks, commitments)?;
    if !decoded
        .activities
        .iter()
        .map(Vec::as_slice)
        .eq(records.activities.iter().copied())
        || !decoded
            .receipts
            .iter()
            .map(Vec::as_slice)
            .eq(records.receipts.iter().copied())
        || !decoded
            .events
            .iter()
            .map(Vec::as_slice)
            .eq(records.events.iter().copied())
        || !decoded
            .oracle_inputs
            .iter()
            .map(Vec::as_slice)
            .eq(records.oracle_inputs.iter().copied())
    {
        return Err(bundle_failure(chunks, AvailabilityCheck::RecordBinding));
    }
    Ok(report)
}

fn bundle_failure(chunks: &[VerifiedChunk], check: AvailabilityCheck) -> AvailabilityFailure {
    AvailabilityFailure {
        check,
        served_bytes: all_served_bytes(chunks),
        commitment: chunks
            .first()
            .map_or([0; 32], VerifiedChunk::data_availability_root),
        classes: class_report(chunks),
    }
}

fn verify_bundle(chunks: &[VerifiedChunk]) -> Result<ReassemblyReport, AvailabilityFailure> {
    let classes = class_report(chunks);
    if !classes.missing.is_empty() {
        return Err(bundle_failure(chunks, AvailabilityCheck::MissingClass));
    }
    for pair in chunks.windows(2) {
        if pair[0].chunk.index >= pair[1].chunk.index
            || pair[0].chunk.batch_number != pair[1].chunk.batch_number
            || pair[0].data_availability_root != pair[1].data_availability_root
            || pair[0].chunk.class > pair[1].chunk.class
        {
            return Err(bundle_failure(chunks, AvailabilityCheck::ChunkOrder));
        }
    }
    if chunks.len() > MAX_CHUNKS
        || chunks.iter().enumerate().any(|(index, chunk)| {
            usize::try_from(chunk.chunk.index) != Ok(index)
                || usize::try_from(chunk.leaf_count) != Ok(chunks.len())
        })
    {
        return Err(bundle_failure(
            chunks,
            AvailabilityCheck::BundleCompleteness,
        ));
    }
    let mut total_bytes = 0;
    for class in AvailabilityClass::ALL {
        let mut expected_offset = 0_u64;
        let mut count = 0;
        let mut empty = false;
        for chunk in chunks.iter().filter(|chunk| chunk.chunk.class == class) {
            if chunk.chunk.class_offset != expected_offset
                || empty
                || (count != 0 && chunk.chunk.bytes.is_empty())
            {
                return Err(bundle_failure(chunks, AvailabilityCheck::ClassOffset));
            }
            let length = u64::try_from(chunk.chunk.bytes.len())
                .map_err(|_| bundle_failure(chunks, AvailabilityCheck::ChunkBounds))?;
            expected_offset = expected_offset
                .checked_add(length)
                .filter(|value| *value <= MAX_SECTION_BYTES as u64)
                .ok_or_else(|| bundle_failure(chunks, AvailabilityCheck::ChunkBounds))?;
            total_bytes += chunk.chunk.bytes.len();
            count += 1;
            empty = chunk.chunk.bytes.is_empty();
        }
    }
    let hashes: Vec<_> = chunks
        .iter()
        .map(|chunk| chunk.chunk.claimed_hash)
        .collect();
    let computed = root_from_leaf_hashes(&hashes)
        .map_err(|_| bundle_failure(chunks, AvailabilityCheck::AvailabilityRoot))?;
    if chunks
        .first()
        .is_none_or(|chunk| computed != chunk.data_availability_root)
    {
        return Err(bundle_failure(chunks, AvailabilityCheck::AvailabilityRoot));
    }
    Ok(ReassemblyReport {
        classes,
        total_bytes,
    })
}

/// Reconstructs canonical records solely from a complete authenticated bundle.
///
/// # Errors
///
/// Returns retained provider evidence if bundle completeness, encoding or any root fails.
pub fn reassemble(
    chunks: &[VerifiedChunk],
    commitments: RootCommitments,
) -> Result<(AvailabilityRecords, ReassemblyReport), AvailabilityFailure> {
    let report = verify_bundle(chunks)?;
    let records = Sections::from_chunks(chunks)?.into_records();
    for (stream, commitment, check) in [
        (
            &records.activities,
            commitments.activity,
            AvailabilityCheck::ActivityRoot,
        ),
        (
            &records.receipts,
            commitments.receipt,
            AvailabilityCheck::ReceiptRoot,
        ),
        (
            &records.events,
            commitments.event,
            AvailabilityCheck::EventRoot,
        ),
        (
            &records.oracle_inputs,
            commitments.oracle,
            AvailabilityCheck::OracleRoot,
        ),
    ] {
        let bytes: Vec<_> = stream.iter().map(Vec::as_slice).collect();
        compare_root(&bytes, commitment, check, chunks, &report.classes)?;
    }
    Ok((records, report))
}
struct Sections {
    activities: Vec<Vec<u8>>,
    receipts: Vec<Vec<u8>>,
    events: Vec<Vec<u8>>,
    oracle: Vec<Vec<u8>>,
}

impl Sections {
    fn from_chunks(chunks: &[VerifiedChunk]) -> Result<Self, AvailabilityFailure> {
        let classes = class_report(chunks);
        if !classes.missing.is_empty() {
            return Err(AvailabilityFailure {
                check: AvailabilityCheck::MissingClass,
                classes,
                ..bundle_failure(chunks, AvailabilityCheck::RecordEncoding)
            });
        }
        let activities = section_bytes(chunks, AvailabilityClass::Activities);
        let receipts = section_bytes(chunks, AvailabilityClass::Receipts);
        let oracle = section_bytes(chunks, AvailabilityClass::Oracle);
        let activities = decode_records(&activities, false)
            .map_err(|()| bundle_failure(chunks, AvailabilityCheck::RecordEncoding))?;
        let receipt_records = decode_records(&receipts, true)
            .map_err(|()| bundle_failure(chunks, AvailabilityCheck::RecordEncoding))?;
        let mut verified_receipts = Vec::new();
        let mut events = Vec::new();
        for (kind, bytes) in receipt_records {
            match kind {
                1 => verified_receipts.push(bytes),
                2 => events.push(bytes),
                _ => return Err(bundle_failure(chunks, AvailabilityCheck::RecordEncoding)),
            }
        }
        let oracle = decode_records(&oracle, false)
            .map_err(|()| bundle_failure(chunks, AvailabilityCheck::RecordEncoding))?
            .into_iter()
            .map(|(_, bytes)| bytes)
            .collect();
        Ok(Self {
            activities: activities.into_iter().map(|(_, bytes)| bytes).collect(),
            receipts: verified_receipts,
            events,
            oracle,
        })
    }

    fn into_records(self) -> AvailabilityRecords {
        AvailabilityRecords {
            activities: self.activities,
            receipts: self.receipts,
            events: self.events,
            oracle_inputs: self.oracle,
        }
    }
}

fn section_bytes(chunks: &[VerifiedChunk], class: AvailabilityClass) -> Vec<u8> {
    let mut bytes = Vec::new();
    for chunk in chunks.iter().filter(|chunk| chunk.chunk().class == class) {
        bytes.extend_from_slice(&chunk.chunk().bytes);
    }
    bytes
}

fn decode_records(bytes: &[u8], tagged: bool) -> Result<Vec<(u8, Vec<u8>)>, ()> {
    let mut reader = Decoder::new(bytes, MAX_SECTION_BYTES);
    let count = if tagged {
        None
    } else {
        Some(reader.sequence_length(MAX_RECORDS).map_err(|_| ())?)
    };
    let mut records = Vec::new();
    let mut previous_kind = 1;
    let mut counts = [0_usize; 3];
    while reader.remaining() != 0 {
        let kind = if tagged {
            reader.u8().map_err(|_| ())?
        } else {
            0
        };
        if tagged && (kind < previous_kind || kind > 2) {
            return Err(());
        }
        if tagged {
            previous_kind = kind;
        }
        let count_for_kind = &mut counts[usize::from(kind)];
        if *count_for_kind == MAX_RECORDS {
            return Err(());
        }
        *count_for_kind += 1;
        let maximum = if tagged {
            MAX_SECTION_BYTES
        } else {
            MAX_MESSAGE_BYTES
        };
        records.push((kind, reader.bytes_owned(maximum).map_err(|_| ())?));
    }
    if count.is_some_and(|count| count != records.len()) {
        return Err(());
    }
    Ok(records)
}

#[cfg(test)]
mod tests {
    use super::decode_records;
    use super::Sections;
    use layerx_wire::encode::Encoder;

    #[test]
    fn counted_sequences_and_tagged_receipts_remain_canonical() {
        let mut sequence = Encoder::new(1024);
        assert_eq!(sequence.sequence_length(1, 1), Ok(()));
        assert_eq!(sequence.bytes(b"activity", 1024), Ok(()));
        let bytes = sequence.finish();
        assert_eq!(
            decode_records(&bytes, false),
            Ok(vec![(0, b"activity".to_vec())])
        );
        assert_eq!(decode_records(&bytes[4..], false), Err(()));
        assert_eq!(decode_records(&[], false), Err(()));
        let mut wrong_count = bytes.clone();
        wrong_count[..4].copy_from_slice(&2_u32.to_be_bytes());
        assert_eq!(decode_records(&wrong_count, false), Err(()));
        let mut receipts = Encoder::new(1024);
        for (kind, record) in [(2, b"event".as_slice()), (1, b"receipt".as_slice())] {
            assert_eq!(receipts.u8(kind), Ok(()));
            assert_eq!(receipts.bytes(record, 1024), Ok(()));
        }
        assert_eq!(decode_records(&receipts.finish(), true), Err(()));
        assert!(Sections::from_chunks(&[]).is_err());
    }
}
