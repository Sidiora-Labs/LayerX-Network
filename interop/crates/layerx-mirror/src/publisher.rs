use std::collections::BTreeMap;

use layerx_agent_api::read::{CheckpointValue, VerifiedRead};
use layerx_agent_api::verify::Level;
use layerx_client::availability::{AvailabilityRecords, AvailabilityResult};
use layerx_crypto::ed25519;
use layerx_proof::availability::{AvailabilityClass, RootCommitments};
use layerx_proof::checkpoint::ThresholdReport;
use layerx_proof::merkle::{root, root_from_leaf_hashes};
use layerx_wire::hash::{availability_chunk_digest, batch_header_digest, checkpoint_id};
use layerx_wire::receipt::{
    decode_batch_header, decode_checkpoint, encode_batch_header, encode_checkpoint,
};
use sha2::{Digest, Sha256};

const ARCHIVE_MAGIC: &[u8; 8] = b"LXMIRROR";
const ARCHIVE_VERSION: u16 = 2;
const ARCHIVE_DOMAIN: &[u8] = b"LXP/mirror/archive/v2\0";
const MAX_ARCHIVE_BYTES: usize = 64 * 1024 * 1024;
const MAX_ARCHIVE_CHUNKS: usize = 65_536;
const MAX_ARCHIVE_RECORDS: usize = 1_048_576;
const MAX_FIELD_BYTES: usize = 32 * 1024 * 1024;

/// A content commitment shared by both archive chains.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct ArchiveCommitment([u8; 32]);

impl ArchiveCommitment {
    pub(crate) const fn from_bytes(bytes: [u8; 32]) -> Self {
        Self(bytes)
    }

    #[must_use]
    pub const fn as_bytes(&self) -> &[u8; 32] {
        &self.0
    }
}

/// Latest checkpoint known at the `LayerX` node boundary.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct CheckpointCoordinate {
    pub batch_number: u64,
    pub checkpoint_id: [u8; 32],
}

/// Latest sealed/finalised coordinates observed through the node boundary.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct NodeHead {
    pub latest_sealed_batch: u64,
    pub latest_finalised_checkpoint: Option<CheckpointCoordinate>,
}

/// Exact core-published sequencer authorization covering one signed header.
/// The signature and range are retained in the archive so a mirror consumer
/// never has to trust a publisher assertion about batch authority.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct BatchAuthorization {
    pub sequencer_id: [u8; 32],
    pub sequencer_public_key: [u8; 32],
    pub first_batch_number: u64,
    pub last_batch_number: u64,
    pub header_signature: [u8; 64],
}

/// Proof-gated batch material paired with its exact core authorization.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct NodeBatch {
    canonical_header: Vec<u8>,
    authorization: BatchAuthorization,
}

impl NodeBatch {
    /// Admits batch material only after canonical decoding and exact sequencer
    /// authorization verification. Authenticated transports use the same
    /// constructor after binding the key to their handshake identity.
    pub fn verify(
        canonical_header: Vec<u8>,
        authorization: BatchAuthorization,
        trust: &crate::SignedHeaderTrust,
    ) -> Result<Self, SourceError> {
        let header =
            decode_batch_header(&canonical_header).map_err(|_| SourceError::BatchHeader)?;
        let reproduced = encode_batch_header(&header).map_err(|_| SourceError::BatchHeader)?;
        if reproduced != canonical_header {
            return Err(SourceError::BatchHeader);
        }
        if authorization.sequencer_id != trust.sequencer_id
            || authorization.sequencer_public_key != trust.sequencer_public_key
            || authorization.first_batch_number != trust.first_batch_number
            || authorization.last_batch_number != trust.last_batch_number
        {
            return Err(SourceError::BatchAuthorization);
        }
        verify_batch_authorization(&header, &canonical_header, authorization)
            .map_err(|_| SourceError::BatchAuthorization)?;
        Ok(Self {
            canonical_header,
            authorization,
        })
    }

    pub(crate) fn authenticated(
        canonical_header: Vec<u8>,
        authorization: BatchAuthorization,
    ) -> Self {
        Self {
            canonical_header,
            authorization,
        }
    }
}

/// Proof-gated checkpoint material paired with its exact node read.
#[derive(Clone, Copy)]
pub struct NodeCheckpoint<'a> {
    read: &'a VerifiedRead<CheckpointValue>,
    verification: &'a ThresholdReport,
}

impl<'a> NodeCheckpoint<'a> {
    #[must_use]
    pub const fn verified(
        read: &'a VerifiedRead<CheckpointValue>,
        verification: &'a ThresholdReport,
    ) -> Self {
        Self { read, verification }
    }
}

/// Exact chunk bytes retained in a mirror archive.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ArchivedChunk {
    pub index: u32,
    pub class: AvailabilityClass,
    pub class_offset: u64,
    pub claimed_hash: [u8; 32],
    pub bytes: Vec<u8>,
}

/// Exact public record streams retained in a mirror archive.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ArchivedRecords {
    pub activities: Vec<Vec<u8>>,
    pub receipts: Vec<Vec<u8>>,
    pub events: Vec<Vec<u8>>,
    pub oracle_inputs: Vec<Vec<u8>>,
}

impl From<&AvailabilityRecords> for ArchivedRecords {
    fn from(records: &AvailabilityRecords) -> Self {
        Self {
            activities: records.activities.clone(),
            receipts: records.receipts.clone(),
            events: records.events.clone(),
            oracle_inputs: records.oracle_inputs.clone(),
        }
    }
}

/// Exact finalised checkpoint certificate retained with the archive.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ArchivedCheckpoint {
    pub coordinate: CheckpointCoordinate,
    pub canonical_certificate: Vec<u8>,
}

/// Decoded, independently checkable mirror archive contents.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ArchiveData {
    pub protocol_version: u16,
    pub network_id: u32,
    pub batch_number: u64,
    pub canonical_batch_header: Vec<u8>,
    pub batch_authorization: BatchAuthorization,
    pub data_availability_root: [u8; 32],
    pub record_roots: RootCommitments,
    pub chunks: Vec<ArchivedChunk>,
    pub records: ArchivedRecords,
    pub checkpoint: Option<ArchivedCheckpoint>,
}

impl ArchiveData {
    /// Decodes and verifies archive framing, canonical headers, chunk hashes,
    /// availability and record roots, and checkpoint certificate encoding.
    ///
    /// # Errors
    ///
    /// Returns a typed archive error for malformed, oversized, or
    /// commitment-inconsistent data.
    #[allow(clippy::too_many_lines)]
    pub fn decode(bytes: &[u8]) -> Result<Self, ArchiveError> {
        if bytes.len() > MAX_ARCHIVE_BYTES {
            return Err(ArchiveError::ArchiveTooLarge);
        }
        let mut reader = Reader::new(bytes);
        if reader.take(ARCHIVE_MAGIC.len())? != ARCHIVE_MAGIC {
            return Err(ArchiveError::Format);
        }
        if reader.u16()? != ARCHIVE_VERSION {
            return Err(ArchiveError::Version);
        }
        let protocol_version = reader.u16()?;
        let network_id = reader.u32()?;
        let batch_number = reader.u64()?;
        let canonical_batch_header = reader.bytes(MAX_FIELD_BYTES)?;
        let header =
            decode_batch_header(&canonical_batch_header).map_err(|_| ArchiveError::BatchHeader)?;
        let reproduced = encode_batch_header(&header).map_err(|_| ArchiveError::BatchHeader)?;
        if reproduced != canonical_batch_header
            || header.protocol_version() != protocol_version
            || header.network_id() != network_id
            || header.batch_number() != batch_number
        {
            return Err(ArchiveError::BatchHeader);
        }
        let batch_authorization = BatchAuthorization {
            sequencer_id: reader.array()?,
            sequencer_public_key: reader.array()?,
            first_batch_number: reader.u64()?,
            last_batch_number: reader.u64()?,
            header_signature: reader.array()?,
        };
        verify_batch_authorization(&header, &canonical_batch_header, batch_authorization)?;
        let data_availability_root = reader.array()?;
        let record_roots = RootCommitments {
            activity: reader.array()?,
            receipt: reader.array()?,
            event: reader.array()?,
            oracle: reader.array()?,
        };
        if header.data_availability_root() != data_availability_root
            || header.activity_merkle_root() != record_roots.activity
            || header.receipt_merkle_root() != record_roots.receipt
            || header.event_merkle_root() != record_roots.event
            || header.oracle_root() != record_roots.oracle
        {
            return Err(ArchiveError::RootMismatch);
        }
        let chunk_count = reader.count(MAX_ARCHIVE_CHUNKS)?;
        let mut chunks = Vec::with_capacity(chunk_count);
        for _ in 0..chunk_count {
            let index = reader.u32()?;
            let class = decode_class(reader.u8()?)?;
            let class_offset = reader.u64()?;
            let claimed_hash = reader.array()?;
            let chunk_bytes = reader.bytes(MAX_FIELD_BYTES)?;
            let computed = availability_chunk_digest(
                batch_number,
                index,
                class as u8,
                class_offset,
                &chunk_bytes,
            )
            .map_err(|_| ArchiveError::ChunkHash)?;
            if computed != claimed_hash {
                return Err(ArchiveError::ChunkHash);
            }
            chunks.push(ArchivedChunk {
                index,
                class,
                class_offset,
                claimed_hash,
                bytes: chunk_bytes,
            });
        }
        verify_chunk_set(&chunks, batch_number, data_availability_root)?;
        let records = ArchivedRecords {
            activities: reader.records()?,
            receipts: reader.records()?,
            events: reader.records()?,
            oracle_inputs: reader.records()?,
        };
        verify_record_roots(&records, record_roots)?;
        let checkpoint = match reader.u8()? {
            0 => None,
            1 => {
                let coordinate = CheckpointCoordinate {
                    batch_number: reader.u64()?,
                    checkpoint_id: reader.array()?,
                };
                let canonical_certificate = reader.bytes(MAX_FIELD_BYTES)?;
                let certificate = decode_checkpoint(&canonical_certificate)
                    .map_err(|_| ArchiveError::Checkpoint)?;
                let reproduced =
                    encode_checkpoint(&certificate).map_err(|_| ArchiveError::Checkpoint)?;
                let certificate_header = certificate.header();
                let computed_id = checkpoint_id(
                    &encode_batch_header(certificate_header)
                        .map_err(|_| ArchiveError::Checkpoint)?,
                    certificate.validity_proof(),
                )
                .map_err(|_| ArchiveError::Checkpoint)?;
                if reproduced != canonical_certificate
                    || certificate_header.protocol_version() != protocol_version
                    || certificate_header.network_id() != network_id
                    || certificate_header.batch_number() != coordinate.batch_number
                    || computed_id != coordinate.checkpoint_id
                {
                    return Err(ArchiveError::Checkpoint);
                }
                Some(ArchivedCheckpoint {
                    coordinate,
                    canonical_certificate,
                })
            }
            _ => return Err(ArchiveError::Format),
        };
        reader.finish()?;
        Ok(Self {
            protocol_version,
            network_id,
            batch_number,
            canonical_batch_header,
            batch_authorization,
            data_availability_root,
            record_roots,
            chunks,
            records,
            checkpoint,
        })
    }

    fn encode(&self) -> Result<Vec<u8>, ArchiveError> {
        let mut writer = Writer::new();
        writer.raw(ARCHIVE_MAGIC)?;
        writer.u16(ARCHIVE_VERSION)?;
        writer.u16(self.protocol_version)?;
        writer.u32(self.network_id)?;
        writer.u64(self.batch_number)?;
        writer.bytes(&self.canonical_batch_header)?;
        writer.raw(&self.batch_authorization.sequencer_id)?;
        writer.raw(&self.batch_authorization.sequencer_public_key)?;
        writer.u64(self.batch_authorization.first_batch_number)?;
        writer.u64(self.batch_authorization.last_batch_number)?;
        writer.raw(&self.batch_authorization.header_signature)?;
        writer.raw(&self.data_availability_root)?;
        writer.raw(&self.record_roots.activity)?;
        writer.raw(&self.record_roots.receipt)?;
        writer.raw(&self.record_roots.event)?;
        writer.raw(&self.record_roots.oracle)?;
        writer.count(self.chunks.len(), MAX_ARCHIVE_CHUNKS)?;
        for chunk in &self.chunks {
            writer.u32(chunk.index)?;
            writer.u8(chunk.class as u8)?;
            writer.u64(chunk.class_offset)?;
            writer.raw(&chunk.claimed_hash)?;
            writer.bytes(&chunk.bytes)?;
        }
        writer.records(&self.records.activities)?;
        writer.records(&self.records.receipts)?;
        writer.records(&self.records.events)?;
        writer.records(&self.records.oracle_inputs)?;
        if let Some(checkpoint) = &self.checkpoint {
            writer.u8(1)?;
            writer.u64(checkpoint.coordinate.batch_number)?;
            writer.raw(&checkpoint.coordinate.checkpoint_id)?;
            writer.bytes(&checkpoint.canonical_certificate)?;
        } else {
            writer.u8(0)?;
        }
        writer.finish()
    }
}

/// A fully verified node-bound archive ready for independent chain writes.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Archive {
    data: ArchiveData,
    bytes: Vec<u8>,
    commitment: ArchiveCommitment,
    node_head: NodeHead,
}

impl Archive {
    /// Restores immutable archive bytes from the crash-safe spool. All archive
    /// evidence is reverified before chain workers receive the value.
    pub(crate) fn from_spool(bytes: Vec<u8>, node_head: NodeHead) -> Result<Self, SourceError> {
        let data = ArchiveData::decode(&bytes).map_err(SourceError::Archive)?;
        if node_head.latest_sealed_batch < data.batch_number
            || node_head
                .latest_finalised_checkpoint
                .is_some_and(|checkpoint| checkpoint.batch_number > node_head.latest_sealed_batch)
        {
            return Err(SourceError::HeadBehindBatch);
        }
        let commitment = archive_commitment(&bytes);
        Ok(Self {
            data,
            bytes,
            commitment,
            node_head,
        })
    }

    /// Builds one archive exclusively from an authoritative batch read, a
    /// complete verified availability result, and optional verified checkpoint.
    ///
    /// # Errors
    ///
    /// Rejects insufficient proof levels, cross-batch/root data, non-canonical
    /// node bytes, inconsistent checkpoint evidence, or dishonest head values.
    pub fn from_node(
        batch: &NodeBatch,
        availability: &AvailabilityResult,
        checkpoint: Option<NodeCheckpoint<'_>>,
        node_head: NodeHead,
    ) -> Result<Self, SourceError> {
        let canonical_batch_header = batch.canonical_header.clone();
        let header =
            decode_batch_header(&canonical_batch_header).map_err(|_| SourceError::BatchHeader)?;
        let reproduced = encode_batch_header(&header).map_err(|_| SourceError::BatchHeader)?;
        if reproduced != canonical_batch_header {
            return Err(SourceError::BatchHeader);
        }
        verify_batch_authorization(&header, &canonical_batch_header, batch.authorization)
            .map_err(|_| SourceError::BatchAuthorization)?;
        if header.batch_number() != availability.batch_number() {
            return Err(SourceError::BatchMismatch);
        }
        let record_roots = availability.record_roots();
        if header.data_availability_root() != availability.data_availability_root()
            || header.activity_merkle_root() != record_roots.activity
            || header.receipt_merkle_root() != record_roots.receipt
            || header.event_merkle_root() != record_roots.event
            || header.oracle_root() != record_roots.oracle
        {
            return Err(SourceError::RootMismatch);
        }
        if node_head.latest_sealed_batch < header.batch_number() {
            return Err(SourceError::HeadBehindBatch);
        }
        if node_head
            .latest_finalised_checkpoint
            .is_some_and(|coordinate| coordinate.batch_number > node_head.latest_sealed_batch)
        {
            return Err(SourceError::HeadBehindCheckpoint);
        }
        let chunks = availability
            .chunks
            .iter()
            .map(|verified| {
                let chunk = verified.chunk();
                ArchivedChunk {
                    index: chunk.index,
                    class: chunk.class,
                    class_offset: chunk.class_offset,
                    claimed_hash: chunk.claimed_hash,
                    bytes: chunk.bytes.clone(),
                }
            })
            .collect::<Vec<_>>();
        let archived_checkpoint = checkpoint
            .map(|material| {
                verify_node_checkpoint(
                    material,
                    header.network_id(),
                    node_head.latest_finalised_checkpoint,
                )
            })
            .transpose()?;
        let data = ArchiveData {
            protocol_version: header.protocol_version(),
            network_id: header.network_id(),
            batch_number: header.batch_number(),
            canonical_batch_header,
            batch_authorization: batch.authorization,
            data_availability_root: availability.data_availability_root(),
            record_roots,
            chunks,
            records: availability.records().into(),
            checkpoint: archived_checkpoint,
        };
        let bytes = data.encode().map_err(SourceError::Archive)?;
        let decoded = ArchiveData::decode(&bytes).map_err(SourceError::Archive)?;
        if decoded != data {
            return Err(SourceError::Archive(ArchiveError::Format));
        }
        let commitment = archive_commitment(&bytes);
        Ok(Self {
            data,
            bytes,
            commitment,
            node_head,
        })
    }

    #[must_use]
    pub const fn data(&self) -> &ArchiveData {
        &self.data
    }

    #[must_use]
    pub fn bytes(&self) -> &[u8] {
        &self.bytes
    }

    #[must_use]
    pub const fn commitment(&self) -> ArchiveCommitment {
        self.commitment
    }

    #[must_use]
    pub const fn node_head(&self) -> NodeHead {
        self.node_head
    }
}

fn verify_batch_authorization(
    header: &layerx_wire::receipt::BatchHeader,
    canonical_header: &[u8],
    authorization: BatchAuthorization,
) -> Result<(), ArchiveError> {
    if authorization.sequencer_id == [0; 32]
        || authorization.sequencer_public_key == [0; 32]
        || authorization.first_batch_number > authorization.last_batch_number
        || header.sequencer_id() != authorization.sequencer_id
        || header.batch_number() < authorization.first_batch_number
        || header.batch_number() > authorization.last_batch_number
    {
        return Err(ArchiveError::BatchAuthorization);
    }
    let digest =
        batch_header_digest(canonical_header).map_err(|_| ArchiveError::BatchAuthorization)?;
    ed25519::verify_digest(
        &authorization.sequencer_public_key,
        &authorization.header_signature,
        &digest,
    )
    .map_err(|_| ArchiveError::BatchAuthorization)
}

fn verify_node_checkpoint(
    material: NodeCheckpoint<'_>,
    network_id: u32,
    head: Option<CheckpointCoordinate>,
) -> Result<ArchivedCheckpoint, SourceError> {
    if material.read.achieved_verification_level < Level::CheckpointFinalised {
        return Err(SourceError::CheckpointVerificationLevel);
    }
    let certificate_bytes = material.read.value.0.as_bytes().to_vec();
    let certificate =
        decode_checkpoint(&certificate_bytes).map_err(|_| SourceError::CheckpointCertificate)?;
    let reproduced =
        encode_checkpoint(&certificate).map_err(|_| SourceError::CheckpointCertificate)?;
    let certificate_header = certificate.header();
    let canonical_header =
        encode_batch_header(certificate_header).map_err(|_| SourceError::CheckpointCertificate)?;
    let canonical_checkpoint_id = checkpoint_id(&canonical_header, certificate.validity_proof())
        .map_err(|_| SourceError::CheckpointCertificate)?;
    let report_roots = material.verification.record_roots();
    let certificate_roots = RootCommitments {
        activity: certificate_header.activity_merkle_root(),
        receipt: certificate_header.receipt_merkle_root(),
        event: certificate_header.event_merkle_root(),
        oracle: certificate_header.oracle_root(),
    };
    if reproduced != certificate_bytes
        || certificate_header.network_id() != network_id
        || material.verification.protocol_version() != certificate_header.protocol_version()
        || material.verification.network_id() != certificate_header.network_id()
        || material.verification.batch_number() != certificate_header.batch_number()
        || material.verification.first_sequence() != certificate_header.first_sequence()
        || material.verification.last_sequence() != certificate_header.last_sequence()
        || material.verification.data_availability_root()
            != certificate_header.data_availability_root()
        || material.verification.resulting_state_root() != certificate_header.resulting_state_root()
        || report_roots != certificate_roots
        || material.verification.required
            != usize::try_from(certificate.threshold())
                .map_err(|_| SourceError::CheckpointMismatch)?
        || material.verification.achieved < material.verification.required
        || material.verification.achieved != certificate.guarantor_signatures().len()
        || material.verification.evidence().settlement_reference()
            != (!certificate.settlement_reference().is_empty())
                .then_some(certificate.settlement_reference())
    {
        return Err(SourceError::CheckpointMismatch);
    }
    let checkpoint_id = material
        .verification
        .evidence()
        .checkpoint_id()
        .ok_or(SourceError::CheckpointMismatch)?;
    if checkpoint_id != canonical_checkpoint_id {
        return Err(SourceError::CheckpointMismatch);
    }
    let coordinate = CheckpointCoordinate {
        batch_number: material.verification.batch_number(),
        checkpoint_id,
    };
    let head = head.ok_or(SourceError::HeadBehindCheckpoint)?;
    if coordinate.batch_number > head.batch_number
        || (coordinate.batch_number == head.batch_number
            && coordinate.checkpoint_id != head.checkpoint_id)
    {
        return Err(SourceError::CheckpointMismatch);
    }
    Ok(ArchivedCheckpoint {
        coordinate,
        canonical_certificate: certificate_bytes,
    })
}

fn verify_chunk_set(
    chunks: &[ArchivedChunk],
    batch_number: u64,
    expected_root: [u8; 32],
) -> Result<(), ArchiveError> {
    if chunks.is_empty() {
        return Err(ArchiveError::ChunkOrder);
    }
    let mut offsets = [0_u64; 5];
    let mut present = [false; 5];
    for (position, chunk) in chunks.iter().enumerate() {
        let expected_index = u32::try_from(position).map_err(|_| ArchiveError::ChunkOrder)?;
        if chunk.index != expected_index {
            return Err(ArchiveError::ChunkOrder);
        }
        let class_index = usize::from(chunk.class as u8 - 1);
        if chunk.class_offset != offsets[class_index] {
            return Err(ArchiveError::ChunkOrder);
        }
        let length = u64::try_from(chunk.bytes.len()).map_err(|_| ArchiveError::Length)?;
        offsets[class_index] = offsets[class_index]
            .checked_add(length)
            .ok_or(ArchiveError::Length)?;
        present[class_index] = true;
        let computed = availability_chunk_digest(
            batch_number,
            chunk.index,
            chunk.class as u8,
            chunk.class_offset,
            &chunk.bytes,
        )
        .map_err(|_| ArchiveError::ChunkHash)?;
        if computed != chunk.claimed_hash {
            return Err(ArchiveError::ChunkHash);
        }
    }
    if present.iter().any(|value| !value) {
        return Err(ArchiveError::MissingClass);
    }
    let hashes = chunks
        .iter()
        .map(|chunk| chunk.claimed_hash)
        .collect::<Vec<_>>();
    let computed = root_from_leaf_hashes(&hashes).map_err(|_| ArchiveError::RootMismatch)?;
    if computed != expected_root {
        return Err(ArchiveError::RootMismatch);
    }
    Ok(())
}

fn verify_record_roots(
    records: &ArchivedRecords,
    expected: RootCommitments,
) -> Result<(), ArchiveError> {
    let activities = record_root(&records.activities)?;
    let receipts = record_root(&records.receipts)?;
    let events = record_root(&records.events)?;
    let oracle = record_root(&records.oracle_inputs)?;
    if activities != expected.activity
        || receipts != expected.receipt
        || events != expected.event
        || oracle != expected.oracle
    {
        return Err(ArchiveError::RecordRoot);
    }
    Ok(())
}

fn record_root(records: &[Vec<u8>]) -> Result<[u8; 32], ArchiveError> {
    let leaves = records.iter().map(Vec::as_slice).collect::<Vec<_>>();
    root(&leaves).map_err(|_| ArchiveError::RecordRoot)
}

pub(crate) fn archive_commitment(bytes: &[u8]) -> ArchiveCommitment {
    let mut hasher = Sha256::new();
    hasher.update(ARCHIVE_DOMAIN);
    hasher.update(bytes);
    ArchiveCommitment(hasher.finalize().into())
}

fn decode_class(value: u8) -> Result<AvailabilityClass, ArchiveError> {
    match value {
        1 => Ok(AvailabilityClass::Activities),
        2 => Ok(AvailabilityClass::Receipts),
        3 => Ok(AvailabilityClass::Oracle),
        4 => Ok(AvailabilityClass::StateDiff),
        5 => Ok(AvailabilityClass::Recovery),
        _ => Err(ArchiveError::Format),
    }
}

/// Failure while admitting node data to an archive.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum SourceError {
    BatchVerificationLevel,
    BatchHeader,
    BatchAuthorization,
    BatchMismatch,
    RootMismatch,
    CheckpointVerificationLevel,
    CheckpointCertificate,
    CheckpointMismatch,
    HeadBehindBatch,
    HeadBehindCheckpoint,
    Archive(ArchiveError),
}

/// Failure while encoding or validating an untrusted retrieved archive.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ArchiveError {
    ArchiveTooLarge,
    Length,
    Format,
    Version,
    BatchHeader,
    BatchAuthorization,
    RootMismatch,
    ChunkHash,
    ChunkOrder,
    MissingClass,
    RecordRoot,
    Checkpoint,
    TrailingBytes,
}

struct Writer {
    bytes: Vec<u8>,
}

impl Writer {
    fn new() -> Self {
        Self { bytes: Vec::new() }
    }

    fn ensure(&self, additional: usize) -> Result<(), ArchiveError> {
        if self.bytes.len().saturating_add(additional) > MAX_ARCHIVE_BYTES {
            Err(ArchiveError::ArchiveTooLarge)
        } else {
            Ok(())
        }
    }

    fn raw(&mut self, value: &[u8]) -> Result<(), ArchiveError> {
        self.ensure(value.len())?;
        self.bytes.extend_from_slice(value);
        Ok(())
    }

    fn u8(&mut self, value: u8) -> Result<(), ArchiveError> {
        self.raw(&[value])
    }

    fn u16(&mut self, value: u16) -> Result<(), ArchiveError> {
        self.raw(&value.to_be_bytes())
    }

    fn u32(&mut self, value: u32) -> Result<(), ArchiveError> {
        self.raw(&value.to_be_bytes())
    }

    fn u64(&mut self, value: u64) -> Result<(), ArchiveError> {
        self.raw(&value.to_be_bytes())
    }

    fn count(&mut self, value: usize, maximum: usize) -> Result<(), ArchiveError> {
        if value > maximum {
            return Err(ArchiveError::Length);
        }
        let value = u32::try_from(value).map_err(|_| ArchiveError::Length)?;
        self.u32(value)
    }

    fn bytes(&mut self, value: &[u8]) -> Result<(), ArchiveError> {
        if value.len() > MAX_FIELD_BYTES {
            return Err(ArchiveError::Length);
        }
        self.count(value.len(), MAX_FIELD_BYTES)?;
        self.raw(value)
    }

    fn records(&mut self, records: &[Vec<u8>]) -> Result<(), ArchiveError> {
        self.count(records.len(), MAX_ARCHIVE_RECORDS)?;
        for record in records {
            self.bytes(record)?;
        }
        Ok(())
    }

    fn finish(self) -> Result<Vec<u8>, ArchiveError> {
        if self.bytes.len() > MAX_ARCHIVE_BYTES {
            Err(ArchiveError::ArchiveTooLarge)
        } else {
            Ok(self.bytes)
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

    fn take(&mut self, length: usize) -> Result<&'a [u8], ArchiveError> {
        let end = self
            .offset
            .checked_add(length)
            .ok_or(ArchiveError::Length)?;
        let value = self
            .bytes
            .get(self.offset..end)
            .ok_or(ArchiveError::Format)?;
        self.offset = end;
        Ok(value)
    }

    fn u8(&mut self) -> Result<u8, ArchiveError> {
        self.take(1)?.first().copied().ok_or(ArchiveError::Format)
    }

    fn u16(&mut self) -> Result<u16, ArchiveError> {
        Ok(u16::from_be_bytes(self.array()?))
    }

    fn u32(&mut self) -> Result<u32, ArchiveError> {
        Ok(u32::from_be_bytes(self.array()?))
    }

    fn u64(&mut self) -> Result<u64, ArchiveError> {
        Ok(u64::from_be_bytes(self.array()?))
    }

    fn array<const N: usize>(&mut self) -> Result<[u8; N], ArchiveError> {
        self.take(N)?.try_into().map_err(|_| ArchiveError::Format)
    }

    fn count(&mut self, maximum: usize) -> Result<usize, ArchiveError> {
        let count = usize::try_from(self.u32()?).map_err(|_| ArchiveError::Length)?;
        if count > maximum {
            Err(ArchiveError::Length)
        } else {
            Ok(count)
        }
    }

    fn bytes(&mut self, maximum: usize) -> Result<Vec<u8>, ArchiveError> {
        let length = self.count(maximum)?;
        Ok(self.take(length)?.to_vec())
    }

    fn records(&mut self) -> Result<Vec<Vec<u8>>, ArchiveError> {
        let count = self.count(MAX_ARCHIVE_RECORDS)?;
        let mut records = Vec::with_capacity(count);
        for _ in 0..count {
            records.push(self.bytes(MAX_FIELD_BYTES)?);
        }
        Ok(records)
    }

    fn finish(self) -> Result<(), ArchiveError> {
        if self.offset == self.bytes.len() {
            Ok(())
        } else {
            Err(ArchiveError::TrailingBytes)
        }
    }
}

/// Ethereum archive target and finality policy.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct EthereumConfig {
    pub chain_id: u64,
    pub archive_contract: [u8; 20],
    pub required_confirmations: u64,
}

impl EthereumConfig {
    /// Rejects an unspecified chain, address, or finality policy.
    ///
    /// # Errors
    ///
    /// Returns [`ConfigError::Ethereum`] when any required value is zero.
    pub fn validate(self) -> Result<Self, ConfigError> {
        if self.chain_id == 0
            || self.archive_contract == [0; 20]
            || self.required_confirmations == 0
        {
            Err(ConfigError::Ethereum)
        } else {
            Ok(self)
        }
    }
}

/// Solana archive target and finality policy.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct SolanaConfig {
    pub genesis_hash: [u8; 32],
    pub archive_program: [u8; 32],
    pub archive_account: [u8; 32],
    pub required_rooted_slots: u64,
}

impl SolanaConfig {
    /// Rejects an unspecified cluster, program, archive account, or finality
    /// policy.
    ///
    /// # Errors
    ///
    /// Returns [`ConfigError::Solana`] when any required value is zero.
    pub fn validate(self) -> Result<Self, ConfigError> {
        if self.genesis_hash == [0; 32]
            || self.archive_program == [0; 32]
            || self.archive_account == [0; 32]
            || self.required_rooted_slots == 0
        {
            Err(ConfigError::Solana)
        } else {
            Ok(self)
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ConfigError {
    Ethereum,
    Solana,
}

/// Exact append request delivered to an Ethereum signer/RPC implementation.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct EthereumArchiveWrite<'a> {
    pub chain_id: u64,
    pub archive_contract: [u8; 20],
    pub commitment: ArchiveCommitment,
    pub network_id: u32,
    pub batch_number: u64,
    pub checkpoint: Option<CheckpointCoordinate>,
    pub archive: &'a [u8],
}

/// Exact append request delivered to a Solana signer/RPC implementation.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct SolanaArchiveWrite<'a> {
    pub genesis_hash: [u8; 32],
    pub archive_program: [u8; 32],
    pub archive_account: [u8; 32],
    pub commitment: ArchiveCommitment,
    pub network_id: u32,
    pub batch_number: u64,
    pub checkpoint: Option<CheckpointCoordinate>,
    pub archive: &'a [u8],
}

/// Successful Ethereum transaction submission.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct EthereumSubmission {
    pub transaction_hash: [u8; 32],
}

/// Successful Solana transaction submission.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct SolanaSubmission {
    pub signature: [u8; 64],
}

/// Canonicality/finality observation for an Ethereum archive transaction.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum EthereumObservation {
    Pending,
    Canonical {
        block_number: u64,
        block_hash: [u8; 32],
        confirmations: u64,
    },
    Reorged {
        former_block_number: u64,
        former_block_hash: [u8; 32],
    },
    Rejected,
}

/// Canonicality/finality observation for a Solana archive transaction.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SolanaObservation {
    Pending,
    Canonical {
        slot: u64,
        blockhash: [u8; 32],
        rooted_slots: u64,
    },
    Reorged {
        former_slot: u64,
        former_blockhash: [u8; 32],
    },
    Rejected,
}

/// Production Ethereum append/read/finality boundary. Implementations must
/// write the exact request bytes and retrieve them by the same commitment.
pub trait EthereumArchiveClient {
    /// Appends the exact archive to the configured Ethereum archive contract.
    ///
    /// # Errors
    ///
    /// Returns a typed chain failure when submission cannot be accepted.
    fn append(
        &mut self,
        request: EthereumArchiveWrite<'_>,
    ) -> Result<EthereumSubmission, ChainFailure>;

    /// Observes canonicality and confirmation depth for a submitted write.
    ///
    /// # Errors
    ///
    /// Returns a typed chain failure when the observation is unavailable.
    fn observe(&mut self, transaction_hash: [u8; 32]) -> Result<EthereumObservation, ChainFailure>;

    /// Retrieves exact archive bytes by commitment.
    ///
    /// # Errors
    ///
    /// Returns a typed chain failure when retrieval cannot be completed.
    fn retrieve(
        &mut self,
        archive_contract: [u8; 20],
        commitment: ArchiveCommitment,
    ) -> Result<Option<Vec<u8>>, ChainFailure>;
}

/// Production Solana append/read/finality boundary. Implementations must write
/// the exact request bytes and retrieve them by the same commitment.
pub trait SolanaArchiveClient {
    /// Appends the exact archive to the configured Solana archive program.
    ///
    /// # Errors
    ///
    /// Returns a typed chain failure when submission cannot be accepted.
    fn append(&mut self, request: SolanaArchiveWrite<'_>)
        -> Result<SolanaSubmission, ChainFailure>;

    /// Observes canonicality and rooted-slot depth for a submitted write.
    ///
    /// # Errors
    ///
    /// Returns a typed chain failure when the observation is unavailable.
    fn observe(&mut self, signature: [u8; 64]) -> Result<SolanaObservation, ChainFailure>;

    /// Retrieves exact archive bytes by commitment.
    ///
    /// # Errors
    ///
    /// Returns a typed chain failure when retrieval cannot be completed.
    fn retrieve(
        &mut self,
        archive_program: [u8; 32],
        archive_account: [u8; 32],
        commitment: ArchiveCommitment,
    ) -> Result<Option<Vec<u8>>, ChainFailure>;
}

/// Transport/submission failure exposed without converting `LayerX` operation
/// into an error.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ChainFailure {
    Unavailable(String),
    RateLimited(String),
    SubmissionRejected(String),
    InvalidResponse(String),
}

/// Latest independently confirmed archive coordinates for one chain.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct MirrorCursor {
    pub latest_batch: Option<u64>,
    pub latest_checkpoint: Option<CheckpointCoordinate>,
}

/// Explicit checkpoint freshness relative to the latest node observation.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CheckpointFreshness {
    NodeHasNoCheckpoint,
    Current,
    NotYetMirrored { target_batch: u64 },
    Behind { batch_lag: u64 },
    DifferentAtBatch { batch_number: u64 },
}

/// Per-chain mirror freshness, never inferred from the other mirror.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct MirrorFreshness {
    pub node_latest_sealed_batch: u64,
    pub latest_batch_mirrored: Option<u64>,
    pub batch_lag: u64,
    pub node_latest_finalised_checkpoint: Option<CheckpointCoordinate>,
    pub latest_checkpoint_mirrored: Option<CheckpointCoordinate>,
    pub checkpoint: CheckpointFreshness,
}

impl MirrorFreshness {
    fn new(cursor: MirrorCursor, node: NodeHead) -> Self {
        let batch_lag = node
            .latest_sealed_batch
            .saturating_sub(cursor.latest_batch.unwrap_or(0));
        let checkpoint = match (node.latest_finalised_checkpoint, cursor.latest_checkpoint) {
            (None, _) => CheckpointFreshness::NodeHasNoCheckpoint,
            (Some(target), None) => CheckpointFreshness::NotYetMirrored {
                target_batch: target.batch_number,
            },
            (Some(target), Some(current)) if target == current => CheckpointFreshness::Current,
            (Some(target), Some(current)) if current.batch_number < target.batch_number => {
                CheckpointFreshness::Behind {
                    batch_lag: target.batch_number - current.batch_number,
                }
            }
            (Some(target), Some(current)) => CheckpointFreshness::DifferentAtBatch {
                batch_number: current.batch_number.max(target.batch_number),
            },
        };
        Self {
            node_latest_sealed_batch: node.latest_sealed_batch,
            latest_batch_mirrored: cursor.latest_batch,
            batch_lag,
            node_latest_finalised_checkpoint: node.latest_finalised_checkpoint,
            latest_checkpoint_mirrored: cursor.latest_checkpoint,
            checkpoint,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PublicationId {
    Ethereum([u8; 32]),
    Solana([u8; 64]),
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ChainPosition {
    Ethereum {
        block_number: u64,
        block_hash: [u8; 32],
        confirmations: u64,
    },
    Solana {
        slot: u64,
        blockhash: [u8; 32],
        rooted_slots: u64,
    },
}

/// Typed reason a mirror is behind while `LayerX` continues operating.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum MirrorDegradation {
    Chain(ChainFailure),
    TransactionRejected,
    ArchiveNotRetrievable,
    RetrievedCommitmentMismatch,
    RetrievedArchive(ArchiveError),
}

/// Independent outcome for one archive chain.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum MirrorState {
    Confirmed {
        commitment: ArchiveCommitment,
        publication: PublicationId,
        position: ChainPosition,
        freshness: MirrorFreshness,
    },
    Pending {
        commitment: ArchiveCommitment,
        publication: PublicationId,
        freshness: MirrorFreshness,
    },
    Reorged {
        commitment: ArchiveCommitment,
        publication: PublicationId,
        former_position: ChainPosition,
        freshness: MirrorFreshness,
    },
    Degraded {
        commitment: ArchiveCommitment,
        degradation: MirrorDegradation,
        freshness: MirrorFreshness,
    },
}

impl MirrorState {
    #[must_use]
    pub const fn freshness(&self) -> MirrorFreshness {
        match self {
            Self::Confirmed { freshness, .. }
            | Self::Pending { freshness, .. }
            | Self::Reorged { freshness, .. }
            | Self::Degraded { freshness, .. } => *freshness,
        }
    }
}

/// Both chain outcomes. Neither outcome can suppress or replace the other.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PublicationReport {
    pub commitment: ArchiveCommitment,
    pub ethereum: MirrorState,
    pub solana: MirrorState,
}

/// Result of retrieving and validating one archive from a mirror.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum RetrievalState {
    Retrieved(Box<ArchiveData>),
    Missing,
    Degraded(MirrorDegradation),
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RetrievalReport {
    pub ethereum: RetrievalState,
    pub solana: RetrievalState,
}

struct EthereumPublisher<C> {
    config: EthereumConfig,
    client: C,
    cursor: MirrorCursor,
    publications: BTreeMap<ArchiveCommitment, EthereumSubmission>,
    confirmed: BTreeMap<ArchiveCommitment, MirrorCoordinate>,
}

impl<C: EthereumArchiveClient> EthereumPublisher<C> {
    fn publish(&mut self, archive: &Archive) -> MirrorState {
        let commitment = archive.commitment();
        let submission = if let Some(submission) = self.publications.get(&commitment).copied() {
            submission
        } else {
            let request = EthereumArchiveWrite {
                chain_id: self.config.chain_id,
                archive_contract: self.config.archive_contract,
                commitment,
                network_id: archive.data.network_id,
                batch_number: archive.data.batch_number,
                checkpoint: archive
                    .data
                    .checkpoint
                    .as_ref()
                    .map(|value| value.coordinate),
                archive: archive.bytes(),
            };
            match self.client.append(request) {
                Ok(submission) => {
                    self.publications.insert(commitment, submission);
                    submission
                }
                Err(failure) => return self.degraded(archive, MirrorDegradation::Chain(failure)),
            }
        };
        let publication = PublicationId::Ethereum(submission.transaction_hash);
        match self.client.observe(submission.transaction_hash) {
            Err(failure) => self.degraded(archive, MirrorDegradation::Chain(failure)),
            Ok(EthereumObservation::Pending) => MirrorState::Pending {
                commitment,
                publication,
                freshness: MirrorFreshness::new(self.cursor, archive.node_head),
            },
            Ok(EthereumObservation::Rejected) => {
                self.publications.remove(&commitment);
                self.confirmed.remove(&commitment);
                self.cursor = cursor_from_confirmed(&self.confirmed);
                self.degraded(archive, MirrorDegradation::TransactionRejected)
            }
            Ok(EthereumObservation::Reorged {
                former_block_number,
                former_block_hash,
            }) => {
                self.publications.remove(&commitment);
                self.confirmed.remove(&commitment);
                self.cursor = cursor_from_confirmed(&self.confirmed);
                MirrorState::Reorged {
                    commitment,
                    publication,
                    former_position: ChainPosition::Ethereum {
                        block_number: former_block_number,
                        block_hash: former_block_hash,
                        confirmations: 0,
                    },
                    freshness: MirrorFreshness::new(self.cursor, archive.node_head),
                }
            }
            Ok(EthereumObservation::Canonical { confirmations, .. })
                if confirmations < self.config.required_confirmations =>
            {
                MirrorState::Pending {
                    commitment,
                    publication,
                    freshness: MirrorFreshness::new(self.cursor, archive.node_head),
                }
            }
            Ok(EthereumObservation::Canonical {
                block_number,
                block_hash,
                confirmations,
            }) => {
                let position = ChainPosition::Ethereum {
                    block_number,
                    block_hash,
                    confirmations,
                };
                self.confirm_retrieval(archive, submission, position)
            }
        }
    }

    fn confirm_retrieval(
        &mut self,
        archive: &Archive,
        submission: EthereumSubmission,
        position: ChainPosition,
    ) -> MirrorState {
        let commitment = archive.commitment();
        let retrieved = match self
            .client
            .retrieve(self.config.archive_contract, commitment)
        {
            Ok(Some(bytes)) => bytes,
            Ok(None) => {
                self.invalidate(commitment);
                return self.degraded(archive, MirrorDegradation::ArchiveNotRetrievable);
            }
            Err(failure) => return self.degraded(archive, MirrorDegradation::Chain(failure)),
        };
        if archive_commitment(&retrieved) != commitment {
            self.invalidate(commitment);
            return self.degraded(archive, MirrorDegradation::RetrievedCommitmentMismatch);
        }
        if retrieved != archive.bytes {
            self.invalidate(commitment);
            return self.degraded(archive, MirrorDegradation::RetrievedCommitmentMismatch);
        }
        if let Err(error) = ArchiveData::decode(&retrieved) {
            self.invalidate(commitment);
            return self.degraded(archive, MirrorDegradation::RetrievedArchive(error));
        }
        self.confirmed
            .insert(commitment, MirrorCoordinate::from_archive(&archive.data));
        self.cursor = cursor_from_confirmed(&self.confirmed);
        MirrorState::Confirmed {
            commitment,
            publication: PublicationId::Ethereum(submission.transaction_hash),
            position,
            freshness: MirrorFreshness::new(self.cursor, archive.node_head),
        }
    }

    fn degraded(&self, archive: &Archive, degradation: MirrorDegradation) -> MirrorState {
        MirrorState::Degraded {
            commitment: archive.commitment(),
            degradation,
            freshness: MirrorFreshness::new(self.cursor, archive.node_head),
        }
    }

    fn retrieve(&mut self, commitment: ArchiveCommitment) -> RetrievalState {
        let state = match self
            .client
            .retrieve(self.config.archive_contract, commitment)
        {
            Ok(Some(bytes)) => validate_retrieved(commitment, &bytes),
            Ok(None) => RetrievalState::Missing,
            Err(failure) => RetrievalState::Degraded(MirrorDegradation::Chain(failure)),
        };
        if matches!(&state, RetrievalState::Missing)
            || matches!(
                &state,
                RetrievalState::Degraded(
                    MirrorDegradation::RetrievedCommitmentMismatch
                        | MirrorDegradation::RetrievedArchive(_)
                )
            )
        {
            self.invalidate(commitment);
        }
        state
    }

    fn invalidate(&mut self, commitment: ArchiveCommitment) {
        self.confirmed.remove(&commitment);
        self.cursor = cursor_from_confirmed(&self.confirmed);
    }
}

struct SolanaPublisher<C> {
    config: SolanaConfig,
    client: C,
    cursor: MirrorCursor,
    publications: BTreeMap<ArchiveCommitment, SolanaSubmission>,
    confirmed: BTreeMap<ArchiveCommitment, MirrorCoordinate>,
}

impl<C: SolanaArchiveClient> SolanaPublisher<C> {
    fn publish(&mut self, archive: &Archive) -> MirrorState {
        let commitment = archive.commitment();
        let submission = if let Some(submission) = self.publications.get(&commitment).copied() {
            submission
        } else {
            let request = SolanaArchiveWrite {
                genesis_hash: self.config.genesis_hash,
                archive_program: self.config.archive_program,
                archive_account: self.config.archive_account,
                commitment,
                network_id: archive.data.network_id,
                batch_number: archive.data.batch_number,
                checkpoint: archive
                    .data
                    .checkpoint
                    .as_ref()
                    .map(|value| value.coordinate),
                archive: archive.bytes(),
            };
            match self.client.append(request) {
                Ok(submission) => {
                    self.publications.insert(commitment, submission);
                    submission
                }
                Err(failure) => return self.degraded(archive, MirrorDegradation::Chain(failure)),
            }
        };
        let publication = PublicationId::Solana(submission.signature);
        match self.client.observe(submission.signature) {
            Err(failure) => self.degraded(archive, MirrorDegradation::Chain(failure)),
            Ok(SolanaObservation::Pending) => MirrorState::Pending {
                commitment,
                publication,
                freshness: MirrorFreshness::new(self.cursor, archive.node_head),
            },
            Ok(SolanaObservation::Rejected) => {
                self.publications.remove(&commitment);
                self.confirmed.remove(&commitment);
                self.cursor = cursor_from_confirmed(&self.confirmed);
                self.degraded(archive, MirrorDegradation::TransactionRejected)
            }
            Ok(SolanaObservation::Reorged {
                former_slot,
                former_blockhash,
            }) => {
                self.publications.remove(&commitment);
                self.confirmed.remove(&commitment);
                self.cursor = cursor_from_confirmed(&self.confirmed);
                MirrorState::Reorged {
                    commitment,
                    publication,
                    former_position: ChainPosition::Solana {
                        slot: former_slot,
                        blockhash: former_blockhash,
                        rooted_slots: 0,
                    },
                    freshness: MirrorFreshness::new(self.cursor, archive.node_head),
                }
            }
            Ok(SolanaObservation::Canonical { rooted_slots, .. })
                if rooted_slots < self.config.required_rooted_slots =>
            {
                MirrorState::Pending {
                    commitment,
                    publication,
                    freshness: MirrorFreshness::new(self.cursor, archive.node_head),
                }
            }
            Ok(SolanaObservation::Canonical {
                slot,
                blockhash,
                rooted_slots,
            }) => {
                let position = ChainPosition::Solana {
                    slot,
                    blockhash,
                    rooted_slots,
                };
                self.confirm_retrieval(archive, submission, position)
            }
        }
    }

    fn confirm_retrieval(
        &mut self,
        archive: &Archive,
        submission: SolanaSubmission,
        position: ChainPosition,
    ) -> MirrorState {
        let commitment = archive.commitment();
        let retrieved = match self.client.retrieve(
            self.config.archive_program,
            self.config.archive_account,
            commitment,
        ) {
            Ok(Some(bytes)) => bytes,
            Ok(None) => {
                self.invalidate(commitment);
                return self.degraded(archive, MirrorDegradation::ArchiveNotRetrievable);
            }
            Err(failure) => return self.degraded(archive, MirrorDegradation::Chain(failure)),
        };
        if archive_commitment(&retrieved) != commitment {
            self.invalidate(commitment);
            return self.degraded(archive, MirrorDegradation::RetrievedCommitmentMismatch);
        }
        if retrieved != archive.bytes {
            self.invalidate(commitment);
            return self.degraded(archive, MirrorDegradation::RetrievedCommitmentMismatch);
        }
        if let Err(error) = ArchiveData::decode(&retrieved) {
            self.invalidate(commitment);
            return self.degraded(archive, MirrorDegradation::RetrievedArchive(error));
        }
        self.confirmed
            .insert(commitment, MirrorCoordinate::from_archive(&archive.data));
        self.cursor = cursor_from_confirmed(&self.confirmed);
        MirrorState::Confirmed {
            commitment,
            publication: PublicationId::Solana(submission.signature),
            position,
            freshness: MirrorFreshness::new(self.cursor, archive.node_head),
        }
    }

    fn degraded(&self, archive: &Archive, degradation: MirrorDegradation) -> MirrorState {
        MirrorState::Degraded {
            commitment: archive.commitment(),
            degradation,
            freshness: MirrorFreshness::new(self.cursor, archive.node_head),
        }
    }

    fn retrieve(&mut self, commitment: ArchiveCommitment) -> RetrievalState {
        let state = match self.client.retrieve(
            self.config.archive_program,
            self.config.archive_account,
            commitment,
        ) {
            Ok(Some(bytes)) => validate_retrieved(commitment, &bytes),
            Ok(None) => RetrievalState::Missing,
            Err(failure) => RetrievalState::Degraded(MirrorDegradation::Chain(failure)),
        };
        if matches!(&state, RetrievalState::Missing)
            || matches!(
                &state,
                RetrievalState::Degraded(
                    MirrorDegradation::RetrievedCommitmentMismatch
                        | MirrorDegradation::RetrievedArchive(_)
                )
            )
        {
            self.invalidate(commitment);
        }
        state
    }

    fn invalidate(&mut self, commitment: ArchiveCommitment) {
        self.confirmed.remove(&commitment);
        self.cursor = cursor_from_confirmed(&self.confirmed);
    }
}

#[derive(Clone, Copy)]
struct MirrorCoordinate {
    batch_number: u64,
    checkpoint: Option<CheckpointCoordinate>,
}

impl MirrorCoordinate {
    fn from_archive(archive: &ArchiveData) -> Self {
        Self {
            batch_number: archive.batch_number,
            checkpoint: archive.checkpoint.as_ref().map(|value| value.coordinate),
        }
    }
}

fn cursor_from_confirmed(
    confirmed: &BTreeMap<ArchiveCommitment, MirrorCoordinate>,
) -> MirrorCursor {
    let latest_batch = confirmed.values().map(|value| value.batch_number).max();
    let latest_checkpoint = confirmed
        .values()
        .filter_map(|value| value.checkpoint)
        .max_by_key(|coordinate| coordinate.batch_number);
    MirrorCursor {
        latest_batch,
        latest_checkpoint,
    }
}

fn validate_retrieved(commitment: ArchiveCommitment, bytes: &[u8]) -> RetrievalState {
    if archive_commitment(bytes) != commitment {
        return RetrievalState::Degraded(MirrorDegradation::RetrievedCommitmentMismatch);
    }
    match ArchiveData::decode(bytes) {
        Ok(archive) => RetrievalState::Retrieved(Box::new(archive)),
        Err(error) => RetrievalState::Degraded(MirrorDegradation::RetrievedArchive(error)),
    }
}

/// Coordinates independent Ethereum and Solana archival writes. Chain
/// degradation is returned in-band and never blocks `LayerX` operation or the
/// other mirror attempt.
pub struct GenericPublisher<E, S> {
    ethereum: EthereumPublisher<E>,
    solana: SolanaPublisher<S>,
}

impl<E: EthereumArchiveClient, S: SolanaArchiveClient> GenericPublisher<E, S> {
    /// Creates a dual-chain publisher with validated archive destinations.
    ///
    /// # Errors
    ///
    /// Returns the exact invalid chain configuration before any write.
    pub fn new(
        ethereum_config: EthereumConfig,
        ethereum_client: E,
        solana_config: SolanaConfig,
        solana_client: S,
    ) -> Result<Self, ConfigError> {
        let ethereum_config = ethereum_config.validate()?;
        let solana_config = solana_config.validate()?;
        Ok(Self {
            ethereum: EthereumPublisher {
                config: ethereum_config,
                client: ethereum_client,
                cursor: MirrorCursor::default(),
                publications: BTreeMap::new(),
                confirmed: BTreeMap::new(),
            },
            solana: SolanaPublisher {
                config: solana_config,
                client: solana_client,
                cursor: MirrorCursor::default(),
                publications: BTreeMap::new(),
                confirmed: BTreeMap::new(),
            },
        })
    }

    /// Submits or advances one node-derived archive on both mirrors. Calling
    /// this again rechecks the original publication instead of duplicating it,
    /// including after confirmation so a later reorg remains observable.
    #[must_use]
    pub fn publish(&mut self, archive: &Archive) -> PublicationReport {
        let ethereum = self.ethereum.publish(archive);
        let solana = self.solana.publish(archive);
        PublicationReport {
            commitment: archive.commitment(),
            ethereum,
            solana,
        }
    }

    /// Retrieves and independently validates the same archive from both
    /// mirrors. Each result is reported separately.
    #[must_use]
    pub fn retrieve(&mut self, commitment: ArchiveCommitment) -> RetrievalReport {
        RetrievalReport {
            ethereum: self.ethereum.retrieve(commitment),
            solana: self.solana.retrieve(commitment),
        }
    }

    #[must_use]
    pub const fn ethereum_cursor(&self) -> MirrorCursor {
        self.ethereum.cursor
    }

    #[must_use]
    pub const fn solana_cursor(&self) -> MirrorCursor {
        self.solana.cursor
    }

    /// Returns both clients after all tracked publication state has been
    /// intentionally discarded by the caller.
    #[must_use]
    pub fn into_clients(self) -> (E, S) {
        (self.ethereum.client, self.solana.client)
    }
}

/// Exact durable status for the Ethereum lane.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum EthereumLaneState {
    Progress {
        stage: crate::store::PublicationStage,
        phase: crate::store::PublicationPhase,
        transaction_hash: Option<[u8; 32]>,
        position: crate::store::FinalityPosition,
        freshness: MirrorFreshness,
    },
    Degraded {
        error: crate::ethereum::EthereumError,
        freshness: MirrorFreshness,
    },
}

/// Exact durable status for the Solana lane.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum SolanaLaneState {
    Progress {
        stage: crate::store::PublicationStage,
        phase: crate::store::PublicationPhase,
        signature: Option<[u8; 64]>,
        position: crate::store::FinalityPosition,
        freshness: MirrorFreshness,
    },
    Degraded {
        error: crate::solana::SolanaError,
        freshness: MirrorFreshness,
    },
}

/// Independent dual-chain durable publication report.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DurablePublicationReport {
    pub commitment: ArchiveCommitment,
    pub ethereum: EthereumLaneState,
    pub solana: SolanaLaneState,
}

/// Production publisher over concrete durable Ethereum and Solana clients.
/// The executable runs each lane on its own worker; this type keeps their state
/// and errors independent even when called by an embedded operator.
pub struct Publisher {
    ethereum: crate::ethereum::EthereumArchiveClient,
    solana: crate::solana::SolanaArchiveClient,
}

impl Publisher {
    #[must_use]
    pub const fn new(
        ethereum: crate::ethereum::EthereumArchiveClient,
        solana: crate::solana::SolanaArchiveClient,
    ) -> Self {
        Self { ethereum, solana }
    }

    /// Advances both durable lanes without converting either failure into the
    /// other lane's state or into a LayerX node failure.
    #[must_use]
    pub fn publish(&mut self, archive: &Archive) -> DurablePublicationReport {
        let ethereum_client = &mut self.ethereum;
        let solana_client = &mut self.solana;
        let (ethereum_result, solana_result) = std::thread::scope(|scope| {
            let ethereum = scope.spawn(|| ethereum_client.advance(archive));
            let solana = scope.spawn(|| solana_client.advance(archive));
            (
                ethereum
                    .join()
                    .unwrap_or(Err(crate::ethereum::EthereumError::WorkerTerminated)),
                solana
                    .join()
                    .unwrap_or(Err(crate::solana::SolanaError::WorkerTerminated)),
            )
        });
        let ethereum = match ethereum_result {
            Ok(progress) => EthereumLaneState::Progress {
                stage: progress.stage,
                phase: progress.phase,
                transaction_hash: progress.transaction_hash,
                position: progress.position,
                freshness: MirrorFreshness::new(progress.cursor, archive.node_head()),
            },
            Err(error) => EthereumLaneState::Degraded {
                error,
                freshness: MirrorFreshness::new(self.ethereum.cursor(), archive.node_head()),
            },
        };
        let solana = match solana_result {
            Ok(progress) => SolanaLaneState::Progress {
                stage: progress.stage,
                phase: progress.phase,
                signature: progress.signature,
                position: progress.position,
                freshness: MirrorFreshness::new(progress.cursor, archive.node_head()),
            },
            Err(error) => SolanaLaneState::Degraded {
                error,
                freshness: MirrorFreshness::new(self.solana.cursor(), archive.node_head()),
            },
        };
        DurablePublicationReport {
            commitment: archive.commitment(),
            ethereum,
            solana,
        }
    }

    #[must_use]
    pub fn ethereum_cursor(&self) -> MirrorCursor {
        self.ethereum.cursor()
    }

    #[must_use]
    pub fn solana_cursor(&self) -> MirrorCursor {
        self.solana.cursor()
    }

    pub fn retrieve(
        &self,
        commitment: ArchiveCommitment,
    ) -> (
        Result<Option<Vec<u8>>, crate::ethereum::EthereumError>,
        Result<Option<Vec<u8>>, crate::solana::SolanaError>,
    ) {
        (
            self.ethereum.retrieve(commitment),
            self.solana.retrieve(commitment),
        )
    }
}
