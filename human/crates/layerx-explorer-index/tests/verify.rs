use std::path::PathBuf;
use std::process::Command;

use layerx_client::availability::{AvailabilityRecords, AvailabilityResult};
use layerx_client::head::Head;
use layerx_explorer_index::verify::{EvidenceKind, PastedInclusion, Verifier, VerifyError};
use layerx_explorer_index::{IndexError, Indexer, IngestOutcome, QueryError};
use layerx_proof::availability::{
    verify_chunk, AvailabilityClass, Chunk, RootCommitments, VerifiedChunk,
};
use layerx_proof::inclusion::{InclusionError, SequencerAuthorization};
use layerx_proof::merkle::{build_leaf_hash_proof, root, MerkleError};
use layerx_proof::receipt::{AuthorizedBatch, ReceiptCheck};
use layerx_types::verify::VerificationLevel;
use sha2::{Digest as _, Sha256};

const MAXIMUM_FIXTURE_BYTES: usize = 1_048_576;

struct CoreFixture {
    receipt: Vec<u8>,
    receipt_authority: AuthorizedBatch,
    activity: Vec<u8>,
    proof: Vec<u8>,
    header: Vec<u8>,
    header_signature: [u8; 64],
    sequencer_authority: SequencerAuthorization,
    activity_root: [u8; 32],
}

impl CoreFixture {
    fn load() -> Self {
        let binary = std::env::var_os("LAYERX_EXPLORER_CORE_FIXTURE")
            .map_or_else(default_fixture_binary, PathBuf::from);
        let output = Command::new(&binary)
            .output()
            .unwrap_or_else(|error| panic!("failed to run {}: {error}", binary.display()));
        assert!(
            output.status.success(),
            "core fixture failed with {:?}: {}",
            output.status.code(),
            String::from_utf8_lossy(&output.stderr)
        );
        assert!(output.stdout.len() <= MAXIMUM_FIXTURE_BYTES);
        let mut reader = Reader::new(&output.stdout);
        assert_eq!(reader.array::<4>(), *b"LXEF");
        assert_eq!(reader.byte(), 1);
        let receipt = reader.sized();
        let sequencer_public_key = reader.array();
        let batch_id = reader.array();
        let asset = reader.array();
        let previous_state_root = reader.array();
        let resulting_state_root = reader.array();
        let activity = reader.sized();
        let proof = reader.sized();
        let header = reader.sized();
        let header_signature = reader.array();
        let sequencer_id = reader.array();
        let first_batch = reader.u64();
        let last_batch = reader.u64();
        let activity_root = reader.array();
        assert!(reader.is_empty(), "core fixture has trailing bytes");
        Self {
            receipt,
            receipt_authority: AuthorizedBatch::new(
                batch_id,
                asset,
                previous_state_root,
                resulting_state_root,
                sequencer_public_key,
            ),
            activity,
            proof,
            header,
            header_signature,
            sequencer_authority: SequencerAuthorization::new(
                sequencer_id,
                sequencer_public_key,
                first_batch,
                last_batch,
            ),
            activity_root,
        }
    }

    fn verifier(&self) -> Verifier {
        Verifier::new(vec![self.receipt_authority], vec![self.sequencer_authority])
    }

    fn inclusion(&self) -> PastedInclusion<'_> {
        PastedInclusion {
            kind: EvidenceKind::ActivityInclusion,
            proof_bytes: &self.proof,
            canonical_leaf_bytes: &self.activity,
            named_root: self.activity_root,
            canonical_header_bytes: &self.header,
            header_signature: self.header_signature,
        }
    }
}

struct Reader<'a> {
    bytes: &'a [u8],
    offset: usize,
}

impl<'a> Reader<'a> {
    const fn new(bytes: &'a [u8]) -> Self {
        Self { bytes, offset: 0 }
    }

    fn byte(&mut self) -> u8 {
        self.take(1)[0]
    }

    fn array<const N: usize>(&mut self) -> [u8; N] {
        self.take(N)
            .try_into()
            .unwrap_or_else(|_| panic!("fixture field width mismatch"))
    }

    fn u32(&mut self) -> u32 {
        u32::from_be_bytes(self.array())
    }

    fn u64(&mut self) -> u64 {
        u64::from_be_bytes(self.array())
    }

    fn sized(&mut self) -> Vec<u8> {
        let length = usize::try_from(self.u32())
            .unwrap_or_else(|error| panic!("fixture length overflow: {error}"));
        assert!(length <= MAXIMUM_FIXTURE_BYTES);
        self.take(length).to_vec()
    }

    fn take(&mut self, length: usize) -> &'a [u8] {
        let end = self
            .offset
            .checked_add(length)
            .unwrap_or_else(|| panic!("fixture offset overflow"));
        let bytes = self
            .bytes
            .get(self.offset..end)
            .unwrap_or_else(|| panic!("truncated core fixture"));
        self.offset = end;
        bytes
    }

    fn is_empty(&self) -> bool {
        self.offset == self.bytes.len()
    }
}

#[test]
fn core_receipt_and_inclusion_verify_independently_and_tampering_is_refused() {
    let fixture = CoreFixture::load();
    let verifier = fixture.verifier();
    let receipt = verifier
        .receipt(&fixture.receipt)
        .unwrap_or_else(|error| panic!("core receipt did not verify: {error:?}"));
    assert_eq!(receipt.kind, EvidenceKind::Receipt);
    assert_eq!(receipt.achieved_level, VerificationLevel::SEQUENCER_SIGNED);
    assert!(receipt.receipt_digest.is_some());

    let inclusion = verifier
        .inclusion(&fixture.inclusion())
        .unwrap_or_else(|error| panic!("core inclusion did not verify: {error:?}"));
    assert_eq!(inclusion.kind, EvidenceKind::ActivityInclusion);
    assert_eq!(inclusion.achieved_level, VerificationLevel::BATCH_INCLUDED);
    assert_eq!(inclusion.proof_root, Some(fixture.activity_root));

    let mut altered_receipt = fixture.receipt.clone();
    let last = altered_receipt
        .last_mut()
        .unwrap_or_else(|| panic!("core receipt was empty"));
    *last ^= 1;
    assert_eq!(
        verifier.receipt(&altered_receipt),
        Err(VerifyError::Receipt(ReceiptCheck::SequencerSignature))
    );

    let mut altered_proof = fixture.proof.clone();
    let last = altered_proof
        .last_mut()
        .unwrap_or_else(|| panic!("core proof was empty"));
    *last ^= 1;
    assert_eq!(
        verifier.inclusion(&PastedInclusion {
            proof_bytes: &altered_proof,
            ..fixture.inclusion()
        }),
        Err(VerifyError::Inclusion(InclusionError::Merkle(
            MerkleError::RootMismatch
        )))
    );

    let truncated_proof = fixture
        .proof
        .get(..fixture.proof.len().saturating_sub(1))
        .unwrap_or_else(|| panic!("core proof truncation failed"));
    assert_eq!(
        verifier.inclusion(&PastedInclusion {
            proof_bytes: truncated_proof,
            ..fixture.inclusion()
        }),
        Err(VerifyError::Proof(MerkleError::Encoding))
    );

    let mut altered_signature = fixture.header_signature;
    altered_signature[0] ^= 1;
    assert_eq!(
        verifier.inclusion(&PastedInclusion {
            header_signature: altered_signature,
            ..fixture.inclusion()
        }),
        Err(VerifyError::Inclusion(InclusionError::HeaderSignature))
    );

    let mut altered_root = fixture.activity_root;
    altered_root[0] ^= 1;
    assert_eq!(
        verifier.inclusion(&PastedInclusion {
            named_root: altered_root,
            ..fixture.inclusion()
        }),
        Err(VerifyError::NamedRoot)
    );

    let no_trust = Verifier::new(Vec::new(), Vec::new());
    assert_eq!(
        no_trust.receipt(&fixture.receipt),
        Err(VerifyError::MissingTrust(EvidenceKind::Receipt))
    );
    assert_eq!(
        no_trust.inclusion(&fixture.inclusion()),
        Err(VerifyError::MissingTrust(EvidenceKind::ActivityInclusion))
    );
}

#[test]
fn public_queries_use_protocol_ids_and_attach_freshness_and_levels() {
    let fixture = CoreFixture::load();
    let availability = availability_result(&fixture);
    let mut index = Indexer::new(Head {
        chain_sequence: 9,
        sealed_batch: 3,
        finalised_checkpoint: [19; 32],
    });
    assert_eq!(
        index.ingest_availability(&availability),
        Ok(IngestOutcome::Inserted)
    );
    let verifier = fixture.verifier();
    {
        let public = index.public(&verifier);
        let Err(incomplete) = public.account_activity(account(5), None, 10) else {
            panic!("incomplete account index unexpectedly answered");
        };
        assert_eq!(
            incomplete.error,
            QueryError::AccountIndexIncomplete { batch: 3 }
        );
        assert_eq!(incomplete.freshness.batches_behind(), 0);
    }

    assert_eq!(
        index.ingest_receipt_authority(3, &fixture.receipt_authority),
        Ok(IngestOutcome::Inserted)
    );
    assert_eq!(
        index.ingest_receipt_authority(3, &fixture.receipt_authority),
        Ok(IngestOutcome::AlreadyPresent)
    );
    let public = index.public(&verifier);
    let batches = public
        .batches(None, 10)
        .unwrap_or_else(|failure| panic!("batch browse refused: {failure:?}"));
    assert_eq!(batches.value.items.len(), 1);
    assert_eq!(batches.value.items[0].batch_number, 3);
    assert_eq!(
        batches.value.items[0].verification_level,
        VerificationLevel::UNVERIFIED
    );
    assert_eq!(batches.freshness.batches_behind(), 0);
    assert!(!batches.freshness.is_current());

    let activities = public
        .account_activity(account(5), None, 10)
        .unwrap_or_else(|failure| panic!("account query refused: {failure:?}"));
    assert_eq!(activities.value.items.len(), 1);
    let activity = &activities.value.items[0];
    assert_eq!(activity.global_sequence, 9);
    assert_eq!(activity.amount, 25);
    assert_eq!(activity.from, account(5));
    assert_eq!(activity.to, account(6));
    assert_eq!(
        activity.verification_level,
        VerificationLevel::SEQUENCER_SIGNED
    );
    assert_eq!(activities.freshness, batches.freshness);

    let protocol_receipt = public.receipt_by_id(activity.activity_id);
    assert_eq!(
        protocol_receipt
            .value
            .as_ref()
            .map(|record| record.canonical_bytes.as_slice()),
        Some(fixture.receipt.as_slice())
    );
    assert_eq!(protocol_receipt.freshness, batches.freshness);
    let digest_receipt = public.receipt_by_id(activity.receipt_digest);
    assert_eq!(digest_receipt.value, protocol_receipt.value);
    assert_eq!(digest_receipt.freshness, batches.freshness);
    let missing = public.receipt_by_id([0xff; 32]);
    assert!(missing.value.is_none());
    assert_eq!(missing.freshness, batches.freshness);

    let receipt_report = public
        .verify_receipt(&fixture.receipt)
        .unwrap_or_else(|failure| panic!("public receipt verify refused: {failure:?}"));
    assert_eq!(
        receipt_report.value.achieved_level,
        VerificationLevel::SEQUENCER_SIGNED
    );
    assert_eq!(
        receipt_report.value.receipt_digest,
        Some(activity.receipt_digest)
    );
    assert_eq!(receipt_report.freshness, batches.freshness);
    let inclusion_report = public
        .verify_inclusion(&fixture.inclusion())
        .unwrap_or_else(|failure| panic!("public inclusion refused: {failure:?}"));
    assert_eq!(
        inclusion_report.value.achieved_level,
        VerificationLevel::BATCH_INCLUDED
    );
    assert_eq!(inclusion_report.freshness, batches.freshness);

    let mut altered = fixture.receipt.clone();
    altered[0] ^= 1;
    let Err(refused) = public.verify_receipt(&altered) else {
        panic!("altered receipt unexpectedly verified");
    };
    assert_eq!(refused.freshness, batches.freshness);
    assert!(matches!(*refused.error, VerifyError::Receipt(_)));
}

fn default_fixture_binary() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .ancestors()
        .nth(3)
        .unwrap_or_else(|| panic!("explorer crate is outside repository"))
        .join("build/tests/explorer_fixture")
}

fn account(first: u8) -> [u8; 32] {
    let mut value = [0_u8; 32];
    value[0] = first;
    value
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

fn counted(bytes: &[u8]) -> Vec<u8> {
    let mut encoder = layerx_wire::encode::Encoder::new(1_048_576);
    assert_eq!(encoder.sequence_length(1, 65_535), Ok(()));
    assert_eq!(encoder.bytes(bytes, 1_048_576), Ok(()));
    encoder.finish()
}

fn availability_material(fixture: &CoreFixture, canonical: bool) -> (Vec<VerifiedChunk>, AvailabilityRecords, RootCommitments) {
    let records = AvailabilityRecords {
        activities: vec![fixture.activity.clone()],
        receipts: vec![fixture.receipt.clone()],
        events: vec![b"core-receipt-observed".to_vec()],
        oracle_inputs: vec![b"core-fixture-oracle-input".to_vec()],
    };
    let sections = [
        (
            AvailabilityClass::Activities,
            if canonical { counted(&records.activities[0]) } else { framed(&records.activities[0]) },
        ),
        (AvailabilityClass::Receipts, {
            let mut section = tagged(1, &records.receipts[0]);
            section.extend_from_slice(&tagged(2, &records.events[0]));
            section
        }),
        (AvailabilityClass::Oracle, if canonical { counted(&records.oracle_inputs[0]) } else { framed(&records.oracle_inputs[0]) }),
        (AvailabilityClass::StateDiff, b"core-state-diff".to_vec()),
        (AvailabilityClass::Recovery, b"core-recovery".to_vec()),
    ];
    let chunks = sections
        .into_iter()
        .enumerate()
        .map(|(index, (class, bytes))| {
            let index = u32::try_from(index)
                .unwrap_or_else(|error| panic!("chunk index overflow: {error}"));
            Chunk {
                batch_number: 3,
                index,
                class,
                class_offset: 0,
                claimed_hash: chunk_digest(3, index, class, &bytes),
                bytes,
            }
        })
        .collect::<Vec<_>>();
    let hashes = chunks
        .iter()
        .map(|chunk| chunk.claimed_hash)
        .collect::<Vec<_>>();
    let mut verified = Vec::<VerifiedChunk>::new();
    for (index, chunk) in chunks.into_iter().enumerate() {
        let (proof, availability_root) = build_leaf_hash_proof(&hashes, index)
            .unwrap_or_else(|error| panic!("availability proof failed: {error:?}"));
        verified.push(
            verify_chunk(chunk, &proof, 3, &availability_root)
                .unwrap_or_else(|error| panic!("chunk verification failed: {error:?}")),
        );
    }
    let roots = RootCommitments {
        activity: root(&[records.activities[0].as_slice()])
            .unwrap_or_else(|error| panic!("activity root failed: {error:?}")),
        receipt: root(&[records.receipts[0].as_slice()])
            .unwrap_or_else(|error| panic!("receipt root failed: {error:?}")),
        event: root(&[records.events[0].as_slice()])
            .unwrap_or_else(|error| panic!("event root failed: {error:?}")),
        oracle: root(&[records.oracle_inputs[0].as_slice()])
            .unwrap_or_else(|error| panic!("oracle root failed: {error:?}")),
    };
    (verified, records, roots)
}

fn availability_result(fixture: &CoreFixture) -> AvailabilityResult {
    let (verified, records, roots) = availability_material(fixture, true);
    AvailabilityResult::from_verified("core-boundary".to_owned(), verified, records, roots)
        .unwrap_or_else(|error| panic!("availability assembly failed: {error:?}"))
}

#[test]
fn core_records_cannot_bypass_canonical_availability_encoding() {
    let fixture = CoreFixture::load();
    let (verified, records, roots) = availability_material(&fixture, false);
    let failure = AvailabilityResult::from_verified("core-boundary".to_owned(), verified, records, roots)
        .err().unwrap_or_else(|| panic!("uncounted activity and oracle sections must remain unindexable"));
    assert_eq!(failure.check, layerx_proof::availability::AvailabilityCheck::RecordEncoding);
}

#[test]
fn wrong_receipt_authority_cannot_partially_materialise_account_rows() {
    let fixture = CoreFixture::load();
    let availability = availability_result(&fixture);
    let mut index = Indexer::new(Head {
        chain_sequence: 9,
        sealed_batch: 3,
        finalised_checkpoint: [19; 32],
    });
    assert_eq!(
        index.ingest_availability(&availability),
        Ok(IngestOutcome::Inserted)
    );
    let wrong = AuthorizedBatch::new([0xff; 32], [0; 32], [0; 32], [0; 32], [0; 32]);
    assert_eq!(
        index.ingest_receipt_authority(3, &wrong),
        Err(IndexError::ReceiptVerification {
            id: index.snapshot().batches[0].receipt_ids[0],
            check: ReceiptCheck::BatchId,
        })
    );
    assert!(index.snapshot().account_activities.is_empty());
}
