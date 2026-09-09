//! Provider-isolated, bounded data-availability retrieval and verification.

use std::time::Duration;

use layerx_proof::availability::{
    verify_chunk, verify_reassembled, AvailabilityCheck, AvailabilityClass, AvailabilityFailure,
    Chunk, ClassReport, ReassembledRecords, ReassemblyReport, RootCommitments, VerifiedChunk,
};
use layerx_proof::merkle::{Proof, MAX_DEPTH};

use crate::lni::schema::{decode_envelope, encode_envelope, Envelope, SchemaError, Version};
use crate::lni::transport::{FrameTransport, TransportError};

const AVAILABILITY_REQUEST_TAG: u16 = 18;
const AVAILABILITY_CHUNK_TAG: u16 = 19;
const AVAILABILITY_END_TAG: u16 = 20;
const ALL_CLASSES: [AvailabilityClass; 5] = [
    AvailabilityClass::Activities,
    AvailabilityClass::Receipts,
    AvailabilityClass::Oracle,
    AvailabilityClass::StateDiff,
    AvailabilityClass::Recovery,
];

/// One canonical retrieval key.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum AvailabilitySelector {
    Checkpoint([u8; 32]),
    Batch(u64),
    SequenceRange { first: u64, last: u64 },
    Activity([u8; 32]),
    SealedCandidate(u64),
}

impl AvailabilitySelector {
    fn encode(self) -> Result<Vec<u8>, FetchError> {
        let mut bytes = Vec::new();
        match self {
            Self::Checkpoint(identifier) => {
                bytes.push(1);
                bytes.extend_from_slice(&identifier);
            }
            Self::Batch(batch) => {
                bytes.push(2);
                bytes.extend_from_slice(&batch.to_be_bytes());
            }
            Self::SequenceRange { first, last } => {
                if last < first {
                    return Err(FetchError::InvalidSelector);
                }
                bytes.push(3);
                bytes.extend_from_slice(&first.to_be_bytes());
                bytes.extend_from_slice(&last.to_be_bytes());
            }
            Self::SealedCandidate(batch) => {
                bytes.push(5);
                bytes.extend_from_slice(&batch.to_be_bytes());
            }
            Self::Activity(identifier) => {
                bytes.push(4);
                bytes.extend_from_slice(&identifier);
            }
        }
        Ok(bytes)
    }
}

/// Explicit finite retrieval bounds. The transport enforces the per-receive
/// deadline; this layer enforces total bytes and chunk count.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct RetrievalLimits {
    pub maximum_bytes: usize,
    pub maximum_chunks: usize,
    pub deadline: Duration,
}

impl RetrievalLimits {
    fn validate(self) -> Result<Self, FetchError> {
        if self.maximum_bytes == 0 || self.maximum_chunks == 0 || self.deadline.is_zero() {
            Err(FetchError::InvalidLimits)
        } else {
            Ok(self)
        }
    }
}

/// Trusted commitments and request identity for a complete provider attempt.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct FetchContext {
    pub interface_version: Version,
    pub correlation_id: u64,
    pub expected_batch_number: u64,
    pub data_availability_root: [u8; 32],
    pub record_roots: RootCommitments,
    pub limits: RetrievalLimits,
}

/// One independently connected provider. No attempt can borrow two transports.
pub struct Provider<'a> {
    pub name: String,
    pub transport: &'a mut dyn FrameTransport,
}

/// Ordered provider fallback set.
pub struct ProviderSet<'a> {
    providers: Vec<Provider<'a>>,
}

impl<'a> ProviderSet<'a> {
    #[must_use]
    pub fn new(providers: Vec<Provider<'a>>) -> Self {
        Self { providers }
    }

    #[must_use]
    pub fn len(&self) -> usize {
        self.providers.len()
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.providers.is_empty()
    }
}

/// Provider-attributed progress emitted only after per-chunk verification.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Progress<'a> {
    pub provider: &'a str,
    pub chunk: &'a VerifiedChunk,
    pub verified_chunks: usize,
    pub verified_bytes: usize,
}

/// Why one provider attempt did not become a complete retrieval.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ProviderFailure {
    Transport(TransportError),
    Envelope(SchemaError),
    UnexpectedResponse,
    MalformedChunk,
    Chunk(AvailabilityFailure),
    Reassembly(AvailabilityFailure),
    SizeBound { attempted: usize, maximum: usize },
    ChunkBound { attempted: usize, maximum: usize },
}

/// Honest result for one failed or partial provider, never combined with
/// another provider's bytes.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ProviderReport {
    pub provider: String,
    pub verified_chunks: usize,
    pub verified_bytes: usize,
    pub classes: ClassReport,
    pub failure: ProviderFailure,
}

/// Exact verified provider chunks and complete root report.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AvailabilityResult {
    pub provider: String,
    pub chunks: Vec<VerifiedChunk>,
    pub report: ReassemblyReport,
    records: AvailabilityRecords,
    record_roots: RootCommitments,
    batch_number: u64,
    data_availability_root: [u8; 32],
}

impl AvailabilityResult {
    /// Constructs a complete retrieval only after rechecking the record roots,
    /// class completeness, chunk ordering, batch identity and availability-root
    /// identity carried by the proof-gated chunks.
    ///
    /// # Errors
    ///
    /// Returns the exact availability verification failure for incomplete,
    /// reordered, cross-batch, cross-root or record-root-mismatched material.
    pub fn from_verified(
        provider: String,
        chunks: Vec<VerifiedChunk>,
        records: AvailabilityRecords,
        record_roots: RootCommitments,
    ) -> Result<Self, AvailabilityFailure> {
        let report = records.verify(&chunks, record_roots)?;
        let first = chunks.first().ok_or_else(|| malformed(&chunks))?;
        let batch_number = first.chunk().batch_number;
        let data_availability_root = first.data_availability_root();
        if chunks.iter().any(|chunk| {
            chunk.chunk().batch_number != batch_number
                || chunk.data_availability_root() != data_availability_root
        }) {
            return Err(malformed(&chunks));
        }
        Ok(Self {
            provider,
            chunks,
            report,
            records,
            record_roots,
            batch_number,
            data_availability_root,
        })
    }

    /// Returns the exact public record streams whose committed roots passed.
    #[must_use]
    pub const fn records(&self) -> &AvailabilityRecords {
        &self.records
    }

    /// Returns the verified batch shared by every chunk.
    #[must_use]
    pub const fn batch_number(&self) -> u64 {
        self.batch_number
    }

    /// Returns the verified root shared by every chunk proof.
    #[must_use]
    pub const fn data_availability_root(&self) -> [u8; 32] {
        self.data_availability_root
    }

    /// Returns the four roots rechecked over the exposed record streams.
    #[must_use]
    pub const fn record_roots(&self) -> RootCommitments {
        self.record_roots
    }
}

/// Exact owned record streams recovered from a complete availability result.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AvailabilityRecords {
    pub activities: Vec<Vec<u8>>,
    pub receipts: Vec<Vec<u8>>,
    pub events: Vec<Vec<u8>>,
    pub oracle_inputs: Vec<Vec<u8>>,
}

impl AvailabilityRecords {
    fn verify(
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

/// Complete single-provider result or all provider-attributed partials.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum FetchOutcome {
    Complete(Box<AvailabilityResult>),
    Partial(Vec<ProviderReport>),
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum FetchError {
    NoProviders,
    InvalidLimits,
    InvalidSelector,
    CorrelationOverflow,
    UnavailableCapability,
    DisconnectedClient,
}

/// Tries providers serially and returns only a fully verified single-provider
/// result. Verified chunks are streamed to `on_chunk` as progress and retain
/// their provider identity.
///
/// # Errors
///
/// Refuses an empty provider set, invalid selector/bounds, or correlation-id
/// overflow before starting the affected provider.
pub fn fetch<F>(
    providers: &mut ProviderSet<'_>,
    selector: AvailabilitySelector,
    context: FetchContext,
    mut on_chunk: F,
) -> Result<FetchOutcome, FetchError>
where
    F: FnMut(Progress<'_>),
{
    if providers.is_empty() {
        return Err(FetchError::NoProviders);
    }
    let limits = context.limits.validate()?;
    let selector_bytes = selector.encode()?;
    let mut partials = Vec::new();
    for (index, provider) in providers.providers.iter_mut().enumerate() {
        let correlation_id = context
            .correlation_id
            .checked_add(u64::try_from(index).map_err(|_| FetchError::CorrelationOverflow)?)
            .ok_or(FetchError::CorrelationOverflow)?;
        match fetch_provider(
            provider.transport,
            &provider.name,
            &selector_bytes,
            FetchContext {
                correlation_id,
                limits,
                ..context
            },
            &mut on_chunk,
        ) {
            Ok(result) => return Ok(FetchOutcome::Complete(Box::new(result))),
            Err(report) => partials.push(*report),
        }
    }
    Ok(FetchOutcome::Partial(partials))
}

/// Fetches a sealed candidate against the caller's verified signed-header commitments.
/// This does not attest, register a checkpoint, or establish finality.
///
/// # Errors
///
/// Returns the same provider, bounds and correlation errors as [`fetch`].
pub fn fetch_sealed_candidate<F>(
    providers: &mut ProviderSet<'_>,
    context: FetchContext,
    on_chunk: F,
) -> Result<FetchOutcome, FetchError>
where
    F: FnMut(Progress<'_>),
{
    fetch(
        providers,
        AvailabilitySelector::SealedCandidate(context.expected_batch_number),
        context,
        on_chunk,
    )
}

fn fetch_provider<F>(
    transport: &mut dyn FrameTransport,
    provider: &str,
    selector: &[u8],
    context: FetchContext,
    on_chunk: &mut F,
) -> Result<AvailabilityResult, Box<ProviderReport>>
where
    F: FnMut(Progress<'_>),
{
    let request = encode_envelope(Envelope {
        version: context.interface_version,
        message_tag: AVAILABILITY_REQUEST_TAG,
        correlation_id: context.correlation_id,
        canonical_payload: selector,
        proof_material: &[],
    })
    .map_err(|error| report(provider, &[], ProviderFailure::Envelope(error)))?;
    transport
        .send(&request)
        .map_err(|error| report(provider, &[], ProviderFailure::Transport(error)))?;
    let mut chunks = Vec::new();
    let mut total_bytes = 0_usize;
    loop {
        let response_bytes = transport
            .receive()
            .map_err(|error| report(provider, &chunks, ProviderFailure::Transport(error)))?;
        let response = decode_envelope(&response_bytes)
            .map_err(|error| report(provider, &chunks, ProviderFailure::Envelope(error)))?;
        if response.version.major != context.interface_version.major
            || response.correlation_id != context.correlation_id
        {
            return Err(report(
                provider,
                &chunks,
                ProviderFailure::UnexpectedResponse,
            ));
        }
        match response.message_tag {
            AVAILABILITY_CHUNK_TAG => {
                receive_chunk(
                    provider,
                    &mut chunks,
                    &mut total_bytes,
                    response.canonical_payload,
                    response.proof_material,
                    context,
                    on_chunk,
                )?;
            }
            AVAILABILITY_END_TAG => {
                if !response.canonical_payload.is_empty() || !response.proof_material.is_empty() {
                    return Err(report(
                        provider,
                        &chunks,
                        ProviderFailure::UnexpectedResponse,
                    ));
                }
                let records = Sections::from_chunks(&chunks).map_err(|failure| {
                    report(provider, &chunks, ProviderFailure::Reassembly(failure))
                });
                let records = records?.into_records();
                let verified =
                    records
                        .verify(&chunks, context.record_roots)
                        .map_err(|failure| {
                            report(provider, &chunks, ProviderFailure::Reassembly(failure))
                        })?;
                return Ok(AvailabilityResult {
                    provider: provider.to_owned(),
                    chunks,
                    report: verified,
                    records,
                    record_roots: context.record_roots,
                    batch_number: context.expected_batch_number,
                    data_availability_root: context.data_availability_root,
                });
            }
            _ => {
                return Err(report(
                    provider,
                    &chunks,
                    ProviderFailure::UnexpectedResponse,
                ));
            }
        }
    }
}

fn receive_chunk<F>(
    provider: &str,
    chunks: &mut Vec<VerifiedChunk>,
    total_bytes: &mut usize,
    bytes: &[u8],
    metadata: &[u8],
    context: FetchContext,
    on_chunk: &mut F,
) -> Result<(), Box<ProviderReport>>
where
    F: FnMut(Progress<'_>),
{
    let attempted_chunks = chunks.len().saturating_add(1);
    if attempted_chunks > context.limits.maximum_chunks {
        return Err(report(
            provider,
            chunks,
            ProviderFailure::ChunkBound {
                attempted: attempted_chunks,
                maximum: context.limits.maximum_chunks,
            },
        ));
    }
    let attempted_bytes = total_bytes.saturating_add(bytes.len());
    if attempted_bytes > context.limits.maximum_bytes {
        return Err(report(
            provider,
            chunks,
            ProviderFailure::SizeBound {
                attempted: attempted_bytes,
                maximum: context.limits.maximum_bytes,
            },
        ));
    }
    let (chunk, proof) =
        decode_chunk(bytes, metadata).map_err(|failure| report(provider, chunks, failure))?;
    let verified = verify_chunk(
        chunk,
        &proof,
        context.expected_batch_number,
        &context.data_availability_root,
    )
    .map_err(|failure| report(provider, chunks, ProviderFailure::Chunk(failure)))?;
    *total_bytes = attempted_bytes;
    chunks.push(verified);
    let latest = chunks
        .last()
        .ok_or_else(|| report(provider, chunks, ProviderFailure::MalformedChunk))?;
    on_chunk(Progress {
        provider,
        chunk: latest,
        verified_chunks: chunks.len(),
        verified_bytes: *total_bytes,
    });
    Ok(())
}

fn report(
    provider: &str,
    chunks: &[VerifiedChunk],
    failure: ProviderFailure,
) -> Box<ProviderReport> {
    let classes = class_report(chunks);
    Box::new(ProviderReport {
        provider: provider.to_owned(),
        verified_chunks: chunks.len(),
        verified_bytes: chunks.iter().fold(0_usize, |total, chunk| {
            total.saturating_add(chunk.chunk().bytes.len())
        }),
        classes,
        failure,
    })
}

fn class_report(chunks: &[VerifiedChunk]) -> ClassReport {
    let obtained: Vec<_> = ALL_CLASSES
        .into_iter()
        .filter(|class| chunks.iter().any(|chunk| chunk.chunk().class == *class))
        .collect();
    let missing = ALL_CLASSES
        .into_iter()
        .filter(|class| !obtained.contains(class))
        .collect();
    ClassReport { obtained, missing }
}

fn decode_chunk(bytes: &[u8], metadata: &[u8]) -> Result<(Chunk, Proof), ProviderFailure> {
    let mut reader = Reader::new(metadata);
    let batch_number = reader.u64()?;
    let index = reader.u32()?;
    let class = match reader.u8()? {
        1 => AvailabilityClass::Activities,
        2 => AvailabilityClass::Receipts,
        3 => AvailabilityClass::Oracle,
        4 => AvailabilityClass::StateDiff,
        5 => AvailabilityClass::Recovery,
        _ => return Err(ProviderFailure::MalformedChunk),
    };
    let class_offset = reader.u64()?;
    let claimed_hash = reader.array()?;
    let leaf_index = reader.u32()?;
    let leaf_count = reader.u32()?;
    let sibling_count = usize::from(reader.u8()?);
    if sibling_count > MAX_DEPTH {
        return Err(ProviderFailure::MalformedChunk);
    }
    let mut siblings = Vec::with_capacity(sibling_count);
    for _ in 0..sibling_count {
        siblings.push(reader.array()?);
    }
    reader.finish()?;
    let proof = Proof::new(leaf_index, leaf_count, siblings)
        .map_err(|_| ProviderFailure::MalformedChunk)?;
    Ok((
        Chunk {
            batch_number,
            index,
            class,
            class_offset,
            bytes: bytes.to_vec(),
            claimed_hash,
        },
        proof,
    ))
}

struct Sections {
    activities: Vec<Vec<u8>>,
    receipts: Vec<Vec<u8>>,
    events: Vec<Vec<u8>>,
    oracle: Vec<Vec<u8>>,
}

impl Sections {
    fn from_chunks(chunks: &[VerifiedChunk]) -> Result<Self, AvailabilityFailure> {
        let activities = section_bytes(chunks, AvailabilityClass::Activities);
        let receipts = section_bytes(chunks, AvailabilityClass::Receipts);
        let oracle = section_bytes(chunks, AvailabilityClass::Oracle);
        let activities = decode_records(&activities, false).map_err(|()| malformed(chunks))?;
        let receipt_records = decode_records(&receipts, true).map_err(|()| malformed(chunks))?;
        let mut verified_receipts = Vec::new();
        let mut events = Vec::new();
        for (kind, bytes) in receipt_records {
            match kind {
                1 => verified_receipts.push(bytes),
                2 => events.push(bytes),
                _ => return Err(malformed(chunks)),
            }
        }
        let oracle = decode_records(&oracle, false)
            .map_err(|()| malformed(chunks))?
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
    let mut reader = RecordReader::new(bytes);
    let count = if tagged { None } else { Some(reader.u32()?) };
    let mut records = Vec::new();
    let mut previous_kind = 1;
    while !reader.finished() {
        let kind = if tagged { reader.u8()? } else { 0 };
        if tagged && (kind < previous_kind || kind > 2) {
            return Err(());
        }
        if tagged {
            previous_kind = kind;
        }
        let length = usize::try_from(reader.u32()?).map_err(|_| ())?;
        records.push((kind, reader.bytes(length)?.to_vec()));
    }
    if let Some(count) = count {
        if usize::try_from(count).map_err(|_| ())? != records.len() {
            return Err(());
        }
    }
    Ok(records)
}

fn malformed(chunks: &[VerifiedChunk]) -> AvailabilityFailure {
    AvailabilityFailure {
        check: AvailabilityCheck::ChunkOrder,
        served_bytes: chunks
            .iter()
            .flat_map(|chunk| chunk.chunk().bytes.iter().copied())
            .collect(),
        commitment: [0; 32],
        classes: class_report(chunks),
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

    fn take<const LENGTH: usize>(&mut self) -> Result<[u8; LENGTH], ProviderFailure> {
        let end = self
            .offset
            .checked_add(LENGTH)
            .ok_or(ProviderFailure::MalformedChunk)?;
        let value = self
            .bytes
            .get(self.offset..end)
            .ok_or(ProviderFailure::MalformedChunk)?;
        self.offset = end;
        value
            .try_into()
            .map_err(|_| ProviderFailure::MalformedChunk)
    }

    fn u8(&mut self) -> Result<u8, ProviderFailure> {
        self.take().map(u8::from_be_bytes)
    }

    fn u32(&mut self) -> Result<u32, ProviderFailure> {
        self.take().map(u32::from_be_bytes)
    }

    fn u64(&mut self) -> Result<u64, ProviderFailure> {
        self.take().map(u64::from_be_bytes)
    }

    fn array<const LENGTH: usize>(&mut self) -> Result<[u8; LENGTH], ProviderFailure> {
        self.take()
    }

    fn finish(self) -> Result<(), ProviderFailure> {
        if self.offset == self.bytes.len() {
            Ok(())
        } else {
            Err(ProviderFailure::MalformedChunk)
        }
    }
}

struct RecordReader<'a> {
    bytes: &'a [u8],
    offset: usize,
}

impl<'a> RecordReader<'a> {
    const fn new(bytes: &'a [u8]) -> Self {
        Self { bytes, offset: 0 }
    }

    const fn finished(&self) -> bool {
        self.offset == self.bytes.len()
    }

    fn bytes(&mut self, length: usize) -> Result<&'a [u8], ()> {
        let end = self.offset.checked_add(length).ok_or(())?;
        let value = self.bytes.get(self.offset..end).ok_or(())?;
        self.offset = end;
        Ok(value)
    }

    fn u8(&mut self) -> Result<u8, ()> {
        let bytes: [u8; 1] = self.bytes(1)?.try_into().map_err(|_| ())?;
        Ok(u8::from_be_bytes(bytes))
    }

    fn u32(&mut self) -> Result<u32, ()> {
        let bytes: [u8; 4] = self.bytes(4)?.try_into().map_err(|_| ())?;
        Ok(u32::from_be_bytes(bytes))
    }
}
