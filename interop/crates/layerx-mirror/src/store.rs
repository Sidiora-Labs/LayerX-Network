//! Crash-safe append-only publication state and immutable archive spool.

use std::collections::BTreeMap;
use std::fs::{self, File, OpenOptions};
use std::io::{Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};
use std::sync::Arc;

use sha2::{Digest, Sha256};

use crate::{ArchiveCommitment, CheckpointCoordinate, MirrorCursor, NodeHead};

const JOURNAL_MAGIC: &[u8; 8] = b"LXMJNL02";
const HEAD_MAGIC: &[u8; 8] = b"LXMHED02";
const SPOOL_MAGIC: &[u8; 8] = b"LXMSPL02";
const RECORD_VERSION: u16 = 2;
const MAX_RECORD_BYTES: usize = 512 * 1024;
const MAX_SIGNED_PAYLOAD_BYTES: usize = 384 * 1024;
const MAX_ARCHIVE_BYTES: usize = 64 * 1024 * 1024;

/// Independently persisted chain lane.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub enum MirrorChain {
    Ethereum,
    Solana,
}

impl MirrorChain {
    const fn tag(self) -> u8 {
        match self {
            Self::Ethereum => 1,
            Self::Solana => 2,
        }
    }

    const fn from_tag(value: u8) -> Result<Self, StoreError> {
        match value {
            1 => Ok(Self::Ethereum),
            2 => Ok(Self::Solana),
            _ => Err(StoreError::Corrupt),
        }
    }
}

/// Idempotent on-chain stage. No archive-size transaction is required.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PublicationStage {
    Manifest,
    Chunk(u32),
    Finalize,
}

/// Complete durable publication state machine. `BroadcastUnknown` is never
/// retried with a replacement nonce/blockhash until chain history resolves it.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PublicationPhase {
    Prepared,
    PreBroadcastFailure,
    Signed,
    BroadcastUnknown,
    Pending,
    Finalized,
    RetrievedVerified,
    PermanentRefusal,
    Reorged,
    BroadcastExpired,
}

impl PublicationPhase {
    const fn tag(self) -> u8 {
        match self {
            Self::Prepared => 1,
            Self::PreBroadcastFailure => 2,
            Self::Signed => 3,
            Self::BroadcastUnknown => 4,
            Self::Pending => 5,
            Self::Finalized => 6,
            Self::RetrievedVerified => 7,
            Self::PermanentRefusal => 8,
            Self::Reorged => 9,
            Self::BroadcastExpired => 10,
        }
    }

    const fn from_tag(value: u8) -> Result<Self, StoreError> {
        match value {
            1 => Ok(Self::Prepared),
            2 => Ok(Self::PreBroadcastFailure),
            3 => Ok(Self::Signed),
            4 => Ok(Self::BroadcastUnknown),
            5 => Ok(Self::Pending),
            6 => Ok(Self::Finalized),
            7 => Ok(Self::RetrievedVerified),
            8 => Ok(Self::PermanentRefusal),
            9 => Ok(Self::Reorged),
            10 => Ok(Self::BroadcastExpired),
            _ => Err(StoreError::Corrupt),
        }
    }
}

/// Exact transaction identity retained before and after broadcast.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum TransactionIdentity {
    None,
    Ethereum([u8; 32]),
    Solana([u8; 64]),
}

/// Canonical finality position, retained so later reorg checks compare the
/// exact previously accepted block rather than only a height.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum FinalityPosition {
    None,
    Ethereum {
        block_number: u64,
        block_hash: [u8; 32],
    },
    Solana {
        slot: u64,
        blockhash: [u8; 32],
    },
}

/// One immutable journal transition. Signed bytes are present in `Signed` and
/// every post-signing phase, guaranteeing restart recovery before broadcast.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PublicationRecord {
    pub chain: MirrorChain,
    pub batch_number: u64,
    pub commitment: ArchiveCommitment,
    pub checkpoint: Option<CheckpointCoordinate>,
    pub stage: PublicationStage,
    pub phase: PublicationPhase,
    pub stage_payload_digest: [u8; 32],
    pub signed_payload: Vec<u8>,
    pub transaction: TransactionIdentity,
    pub position: FinalityPosition,
}

impl PublicationRecord {
    /// Refuses phase/identity combinations that would erase an ambiguous
    /// broadcast or claim finality without exact chain position.
    pub fn validate(&self) -> Result<(), StoreError> {
        if self.batch_number == 0 || self.signed_payload.len() > MAX_SIGNED_PAYLOAD_BYTES {
            return Err(StoreError::InvalidTransition);
        }
        if matches!(self.stage, PublicationStage::Chunk(index) if index >= 524_288) {
            return Err(StoreError::InvalidTransition);
        }
        let post_signing = matches!(
            self.phase,
            PublicationPhase::Signed
                | PublicationPhase::BroadcastUnknown
                | PublicationPhase::Pending
                | PublicationPhase::Finalized
                | PublicationPhase::RetrievedVerified
                | PublicationPhase::Reorged
                | PublicationPhase::BroadcastExpired
        );
        if post_signing
            && (self.signed_payload.is_empty()
                || matches!(self.transaction, TransactionIdentity::None))
        {
            return Err(StoreError::InvalidTransition);
        }
        if matches!(
            self.phase,
            PublicationPhase::Finalized
                | PublicationPhase::RetrievedVerified
                | PublicationPhase::Reorged
        ) && matches!(self.position, FinalityPosition::None)
        {
            return Err(StoreError::InvalidTransition);
        }
        match (self.chain, &self.transaction, self.position) {
            (MirrorChain::Ethereum, TransactionIdentity::Solana(_), _)
            | (MirrorChain::Solana, TransactionIdentity::Ethereum(_), _)
            | (MirrorChain::Ethereum, _, FinalityPosition::Solana { .. })
            | (MirrorChain::Solana, _, FinalityPosition::Ethereum { .. }) => {
                Err(StoreError::InvalidTransition)
            }
            _ => Ok(()),
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct JournalHead {
    sequence: u64,
    offset: u64,
    digest: [u8; 32],
}

/// Append-only per-chain state store with an atomically replaced sealed head.
pub struct PublicationJournal {
    directory: PathBuf,
    chain: MirrorChain,
    first_batch_number: u64,
    log: File,
    _lock: File,
    head: JournalHead,
    records: BTreeMap<(ArchiveCommitment, PublicationStageKey), PublicationRecord>,
}

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
struct PublicationStageKey(u32);

impl From<PublicationStage> for PublicationStageKey {
    fn from(value: PublicationStage) -> Self {
        match value {
            PublicationStage::Manifest => Self(0),
            PublicationStage::Chunk(index) => Self(index.saturating_add(1)),
            PublicationStage::Finalize => Self(u32::MAX),
        }
    }
}

impl PublicationJournal {
    /// Opens and fully replays a lane after validating the atomically sealed
    /// high-water offset and hash chain. A truncated but internally valid
    /// prefix is rejected against the independent head file.
    pub fn open(
        directory: impl Into<PathBuf>,
        chain: MirrorChain,
        first_batch_number: u64,
    ) -> Result<Self, StoreError> {
        if first_batch_number == 0 {
            return Err(StoreError::InvalidTransition);
        }
        let directory = directory.into();
        fs::create_dir_all(&directory).map_err(StoreError::Io)?;
        let log_path = directory.join(format!("{}.journal", lane_name(chain)));
        let head_path = directory.join(format!("{}.head", lane_name(chain)));
        let lock_path = directory.join(format!("{}.lock", lane_name(chain)));
        let lock = OpenOptions::new()
            .create(true)
            .read(true)
            .write(true)
            .open(lock_path)
            .map_err(StoreError::Io)?;
        lock.try_lock().map_err(|_| StoreError::Conflict)?;
        let mut log = OpenOptions::new()
            .create(true)
            .read(true)
            .append(true)
            .open(&log_path)
            .map_err(StoreError::Io)?;
        let on_disk_head = read_head(&head_path)?;
        let (head, records) = replay(&mut log, chain, on_disk_head)?;
        if on_disk_head != Some(head) {
            write_head(&directory, &head_path, head)?;
        }
        Ok(Self {
            directory,
            chain,
            first_batch_number,
            log,
            _lock: lock,
            head,
            records,
        })
    }

    /// Durably appends one state transition and seals its new high-water head
    /// before updating the in-memory view.
    pub fn append(&mut self, record: PublicationRecord) -> Result<(), StoreError> {
        record.validate()?;
        if record.chain != self.chain {
            return Err(StoreError::InvalidTransition);
        }
        if self.records.values().any(|existing| {
            existing.batch_number == record.batch_number && existing.commitment != record.commitment
        }) {
            return Err(StoreError::Conflict);
        }
        if let Some(previous) = self.records.get(&(record.commitment, record.stage.into())) {
            validate_transition(previous, &record)?;
            if previous == &record {
                return Ok(());
            }
        }
        let body = encode_record(&record)?;
        let sequence = self
            .head
            .sequence
            .checked_add(1)
            .ok_or(StoreError::Length)?;
        let digest = record_digest(self.head.digest, sequence, &body);
        let length = u32::try_from(body.len()).map_err(|_| StoreError::Length)?;
        let mut framed = Vec::with_capacity(8 + body.len() + 32);
        framed.extend_from_slice(&sequence.to_be_bytes());
        framed.extend_from_slice(&length.to_be_bytes());
        framed.extend_from_slice(&body);
        framed.extend_from_slice(&digest);
        self.log.write_all(&framed).map_err(StoreError::Io)?;
        self.log.sync_data().map_err(StoreError::Io)?;
        let offset = self
            .head
            .offset
            .checked_add(u64::try_from(framed.len()).map_err(|_| StoreError::Length)?)
            .ok_or(StoreError::Length)?;
        let head = JournalHead {
            sequence,
            offset,
            digest,
        };
        let head_path = self
            .directory
            .join(format!("{}.head", lane_name(self.chain)));
        write_head(&self.directory, &head_path, head)?;
        self.head = head;
        self.records
            .insert((record.commitment, record.stage.into()), record);
        Ok(())
    }

    #[must_use]
    pub fn record(
        &self,
        commitment: ArchiveCommitment,
        stage: PublicationStage,
    ) -> Option<&PublicationRecord> {
        self.records.get(&(commitment, stage.into()))
    }

    /// Returns only the contiguous retrieved-and-verified prefix. A later
    /// confirmed batch never jumps over an absent or reorged predecessor.
    #[must_use]
    pub fn cursor(&self) -> MirrorCursor {
        let mut next = self.first_batch_number;
        let mut latest_checkpoint = None;
        loop {
            let confirmed = self.records.values().find(|record| {
                record.batch_number == next
                    && record.stage == PublicationStage::Finalize
                    && record.phase == PublicationPhase::RetrievedVerified
            });
            let Some(record) = confirmed else {
                break;
            };
            if let Some(checkpoint) = record.checkpoint {
                latest_checkpoint = Some(checkpoint);
            }
            let Some(incremented) = next.checked_add(1) else {
                break;
            };
            next = incremented;
        }
        let latest_batch = (next > self.first_batch_number).then_some(next - 1);
        MirrorCursor {
            latest_batch,
            latest_checkpoint,
        }
    }

    #[must_use]
    pub fn records(&self) -> impl Iterator<Item = &PublicationRecord> {
        self.records.values()
    }
}

/// Exact archive bytes plus the node head observed while they were acquired.
pub struct SpooledArchive {
    pub bytes: Vec<u8>,
    pub node_head: NodeHead,
}

/// Immutable content-addressed spool shared by independent chain workers.
#[derive(Clone, Debug)]
pub struct ArchiveSpool {
    directory: PathBuf,
    _lock: Arc<File>,
}

impl ArchiveSpool {
    pub fn open(directory: impl Into<PathBuf>) -> Result<Self, StoreError> {
        let directory = directory.into();
        fs::create_dir_all(&directory).map_err(StoreError::Io)?;
        let lock = OpenOptions::new()
            .create(true)
            .read(true)
            .write(true)
            .open(directory.join(".spool.lock"))
            .map_err(StoreError::Io)?;
        lock.try_lock().map_err(|_| StoreError::Conflict)?;
        Ok(Self {
            directory,
            _lock: Arc::new(lock),
        })
    }

    /// Writes once by commitment using temp-file sync and atomic rename. An
    /// existing object must be byte-identical.
    pub fn put(
        &self,
        commitment: ArchiveCommitment,
        bytes: &[u8],
        node_head: NodeHead,
    ) -> Result<(), StoreError> {
        if bytes.is_empty() || bytes.len() > MAX_ARCHIVE_BYTES {
            return Err(StoreError::Length);
        }
        let path = self.path(commitment);
        if path.exists() {
            let existing = self.get(commitment)?;
            return if existing.bytes == bytes && existing.node_head == node_head {
                Ok(())
            } else {
                Err(StoreError::Conflict)
            };
        }
        let payload = encode_spool(bytes, node_head)?;
        let temporary = self
            .directory
            .join(format!(".{}.tmp", hex(commitment.as_bytes())));
        let mut file = OpenOptions::new()
            .create(true)
            .truncate(true)
            .write(true)
            .open(&temporary)
            .map_err(StoreError::Io)?;
        file.write_all(&payload).map_err(StoreError::Io)?;
        file.sync_all().map_err(StoreError::Io)?;
        fs::rename(&temporary, &path).map_err(StoreError::Io)?;
        sync_directory(&self.directory)?;
        Ok(())
    }

    pub fn get(&self, commitment: ArchiveCommitment) -> Result<SpooledArchive, StoreError> {
        let mut file = File::open(self.path(commitment)).map_err(StoreError::Io)?;
        let mut bytes = Vec::new();
        file.read_to_end(&mut bytes).map_err(StoreError::Io)?;
        decode_spool(&bytes, commitment)
    }

    /// Lists only canonical content-addressed objects. Temporary or malformed
    /// names are ignored; every returned object is still verified by `get`.
    pub fn commitments(&self) -> Result<Vec<ArchiveCommitment>, StoreError> {
        let mut output = Vec::new();
        for entry in fs::read_dir(&self.directory).map_err(StoreError::Io)? {
            let entry = entry.map_err(StoreError::Io)?;
            if !entry.file_type().map_err(StoreError::Io)?.is_file() {
                continue;
            }
            let Some(name) = entry.file_name().to_str().map(str::to_owned) else {
                continue;
            };
            let Some(stem) = name.strip_suffix(".archive") else {
                continue;
            };
            let Some(bytes) = decode_hex_fixed(stem) else {
                continue;
            };
            output.push(ArchiveCommitment::from_bytes(bytes));
        }
        output.sort();
        output.dedup();
        Ok(output)
    }

    fn path(&self, commitment: ArchiveCommitment) -> PathBuf {
        self.directory
            .join(format!("{}.archive", hex(commitment.as_bytes())))
    }
}

#[derive(Debug)]
pub enum StoreError {
    Io(std::io::Error),
    Corrupt,
    Length,
    Conflict,
    InvalidTransition,
    Rollback,
}

impl Clone for StoreError {
    fn clone(&self) -> Self {
        match self {
            Self::Io(error) => Self::Io(std::io::Error::from(error.kind())),
            Self::Corrupt => Self::Corrupt,
            Self::Length => Self::Length,
            Self::Conflict => Self::Conflict,
            Self::InvalidTransition => Self::InvalidTransition,
            Self::Rollback => Self::Rollback,
        }
    }
}

impl PartialEq for StoreError {
    fn eq(&self, other: &Self) -> bool {
        match (self, other) {
            (Self::Io(left), Self::Io(right)) => left.kind() == right.kind(),
            (Self::Corrupt, Self::Corrupt)
            | (Self::Length, Self::Length)
            | (Self::Conflict, Self::Conflict)
            | (Self::InvalidTransition, Self::InvalidTransition)
            | (Self::Rollback, Self::Rollback) => true,
            _ => false,
        }
    }
}

impl Eq for StoreError {}

fn validate_transition(
    previous: &PublicationRecord,
    next: &PublicationRecord,
) -> Result<(), StoreError> {
    if previous.chain != next.chain
        || previous.batch_number != next.batch_number
        || previous.commitment != next.commitment
        || previous.checkpoint != next.checkpoint
        || previous.stage != next.stage
        || previous.stage_payload_digest != next.stage_payload_digest
    {
        return Err(StoreError::Conflict);
    }
    let solana_replacement = previous.chain == MirrorChain::Solana
        && matches!(
            previous.phase,
            PublicationPhase::Reorged | PublicationPhase::BroadcastExpired
        )
        && next.phase == PublicationPhase::Signed;
    if !solana_replacement
        && !previous.signed_payload.is_empty()
        && (previous.signed_payload != next.signed_payload
            || previous.transaction != next.transaction)
    {
        return Err(StoreError::Conflict);
    }
    let allowed = match previous.phase {
        PublicationPhase::Prepared => matches!(
            next.phase,
            PublicationPhase::PreBroadcastFailure
                | PublicationPhase::Signed
                | PublicationPhase::PermanentRefusal
        ),
        PublicationPhase::PreBroadcastFailure => matches!(
            next.phase,
            PublicationPhase::PreBroadcastFailure
                | PublicationPhase::Signed
                | PublicationPhase::PermanentRefusal
        ),
        PublicationPhase::Signed => matches!(
            next.phase,
            PublicationPhase::BroadcastUnknown
                | PublicationPhase::Pending
                | PublicationPhase::PermanentRefusal
        ),
        PublicationPhase::BroadcastUnknown => matches!(
            next.phase,
            PublicationPhase::BroadcastUnknown
                | PublicationPhase::BroadcastExpired
                | PublicationPhase::Pending
                | PublicationPhase::Finalized
                | PublicationPhase::PermanentRefusal
        ),
        PublicationPhase::Pending => matches!(
            next.phase,
            PublicationPhase::Pending
                | PublicationPhase::Finalized
                | PublicationPhase::PermanentRefusal
        ),
        PublicationPhase::Finalized => matches!(
            next.phase,
            PublicationPhase::Finalized
                | PublicationPhase::RetrievedVerified
                | PublicationPhase::Reorged
        ),
        PublicationPhase::RetrievedVerified => matches!(
            next.phase,
            PublicationPhase::RetrievedVerified | PublicationPhase::Reorged
        ),
        PublicationPhase::Reorged => matches!(
            next.phase,
            PublicationPhase::Reorged | PublicationPhase::Signed
        ),
        PublicationPhase::BroadcastExpired => matches!(
            next.phase,
            PublicationPhase::BroadcastExpired
                | PublicationPhase::Signed
                | PublicationPhase::PermanentRefusal
        ),
        PublicationPhase::PermanentRefusal => next.phase == PublicationPhase::PermanentRefusal,
    };
    allowed.then_some(()).ok_or(StoreError::InvalidTransition)
}

fn replay(
    log: &mut File,
    chain: MirrorChain,
    expected_head: Option<JournalHead>,
) -> Result<
    (
        JournalHead,
        BTreeMap<(ArchiveCommitment, PublicationStageKey), PublicationRecord>,
    ),
    StoreError,
> {
    log.seek(SeekFrom::Start(0)).map_err(StoreError::Io)?;
    let mut records: BTreeMap<(ArchiveCommitment, PublicationStageKey), PublicationRecord> =
        BTreeMap::new();
    let mut head = JournalHead {
        sequence: 0,
        offset: 0,
        digest: [0; 32],
    };
    let mut sealed_prefix_seen = expected_head.is_none() || expected_head == Some(head);
    loop {
        let mut sequence_bytes = [0_u8; 8];
        match log.read_exact(&mut sequence_bytes) {
            Ok(()) => {}
            Err(error) if error.kind() == std::io::ErrorKind::UnexpectedEof => break,
            Err(error) => return Err(StoreError::Io(error)),
        }
        let sequence = u64::from_be_bytes(sequence_bytes);
        let mut length_bytes = [0_u8; 4];
        log.read_exact(&mut length_bytes)
            .map_err(|_| StoreError::Corrupt)?;
        let length =
            usize::try_from(u32::from_be_bytes(length_bytes)).map_err(|_| StoreError::Length)?;
        if length == 0 || length > MAX_RECORD_BYTES {
            return Err(StoreError::Length);
        }
        let mut body = vec![0_u8; length];
        log.read_exact(&mut body).map_err(|_| StoreError::Corrupt)?;
        let mut digest = [0_u8; 32];
        log.read_exact(&mut digest)
            .map_err(|_| StoreError::Corrupt)?;
        if sequence != head.sequence.checked_add(1).ok_or(StoreError::Length)?
            || digest != record_digest(head.digest, sequence, &body)
        {
            return Err(StoreError::Corrupt);
        }
        let record = decode_record(&body)?;
        if record.chain != chain {
            return Err(StoreError::Corrupt);
        }
        if records.values().any(|existing| {
            existing.batch_number == record.batch_number && existing.commitment != record.commitment
        }) {
            return Err(StoreError::Conflict);
        }
        if let Some(previous) = records.get(&(record.commitment, record.stage.into())) {
            validate_transition(previous, &record)?;
        }
        records.insert((record.commitment, record.stage.into()), record);
        head = JournalHead {
            sequence,
            offset: head
                .offset
                .checked_add(8 + 4 + u64::try_from(length).map_err(|_| StoreError::Length)? + 32)
                .ok_or(StoreError::Length)?,
            digest,
        };
        if expected_head == Some(head) {
            sealed_prefix_seen = true;
        }
    }
    let actual_length = log.metadata().map_err(StoreError::Io)?.len();
    if actual_length != head.offset {
        return Err(StoreError::Corrupt);
    }
    if !sealed_prefix_seen {
        return Err(StoreError::Rollback);
    }
    log.seek(SeekFrom::End(0)).map_err(StoreError::Io)?;
    Ok((head, records))
}

fn record_digest(previous: [u8; 32], sequence: u64, body: &[u8]) -> [u8; 32] {
    let mut hasher = Sha256::new();
    hasher.update(b"LayerX/mirror/journal-record/v2\0");
    hasher.update(previous);
    hasher.update(sequence.to_be_bytes());
    hasher.update(u32::try_from(body.len()).unwrap_or(u32::MAX).to_be_bytes());
    hasher.update(body);
    hasher.finalize().into()
}

fn encode_record(record: &PublicationRecord) -> Result<Vec<u8>, StoreError> {
    record.validate()?;
    let mut bytes = Vec::new();
    bytes.extend_from_slice(JOURNAL_MAGIC);
    bytes.extend_from_slice(&RECORD_VERSION.to_be_bytes());
    bytes.push(record.chain.tag());
    bytes.extend_from_slice(&record.batch_number.to_be_bytes());
    bytes.extend_from_slice(record.commitment.as_bytes());
    encode_checkpoint(&mut bytes, record.checkpoint);
    match record.stage {
        PublicationStage::Manifest => bytes.push(1),
        PublicationStage::Chunk(index) => {
            bytes.push(2);
            bytes.extend_from_slice(&index.to_be_bytes());
        }
        PublicationStage::Finalize => bytes.push(3),
    }
    bytes.push(record.phase.tag());
    bytes.extend_from_slice(&record.stage_payload_digest);
    push_bytes(&mut bytes, &record.signed_payload, MAX_SIGNED_PAYLOAD_BYTES)?;
    encode_transaction(&mut bytes, &record.transaction);
    encode_position(&mut bytes, record.position);
    if bytes.len() > MAX_RECORD_BYTES {
        return Err(StoreError::Length);
    }
    Ok(bytes)
}

fn decode_record(bytes: &[u8]) -> Result<PublicationRecord, StoreError> {
    let mut reader = Reader::new(bytes);
    if reader.take(8)? != JOURNAL_MAGIC || reader.u16()? != RECORD_VERSION {
        return Err(StoreError::Corrupt);
    }
    let chain = MirrorChain::from_tag(reader.u8()?)?;
    let batch_number = reader.u64()?;
    let commitment = ArchiveCommitment::from_bytes(reader.array()?);
    let checkpoint = decode_checkpoint(&mut reader)?;
    let stage = match reader.u8()? {
        1 => PublicationStage::Manifest,
        2 => PublicationStage::Chunk(reader.u32()?),
        3 => PublicationStage::Finalize,
        _ => return Err(StoreError::Corrupt),
    };
    let phase = PublicationPhase::from_tag(reader.u8()?)?;
    let stage_payload_digest = reader.array()?;
    let signed_payload = reader.bytes(MAX_SIGNED_PAYLOAD_BYTES)?.to_vec();
    let transaction = decode_transaction(&mut reader)?;
    let position = decode_position(&mut reader)?;
    reader.finish()?;
    let record = PublicationRecord {
        chain,
        batch_number,
        commitment,
        checkpoint,
        stage,
        phase,
        stage_payload_digest,
        signed_payload,
        transaction,
        position,
    };
    record.validate()?;
    Ok(record)
}

fn encode_checkpoint(output: &mut Vec<u8>, value: Option<CheckpointCoordinate>) {
    if let Some(value) = value {
        output.push(1);
        output.extend_from_slice(&value.batch_number.to_be_bytes());
        output.extend_from_slice(&value.checkpoint_id);
    } else {
        output.push(0);
    }
}

fn decode_checkpoint(reader: &mut Reader<'_>) -> Result<Option<CheckpointCoordinate>, StoreError> {
    match reader.u8()? {
        0 => Ok(None),
        1 => Ok(Some(CheckpointCoordinate {
            batch_number: reader.u64()?,
            checkpoint_id: reader.array()?,
        })),
        _ => Err(StoreError::Corrupt),
    }
}

fn encode_transaction(output: &mut Vec<u8>, value: &TransactionIdentity) {
    match value {
        TransactionIdentity::None => output.push(0),
        TransactionIdentity::Ethereum(hash) => {
            output.push(1);
            output.extend_from_slice(hash);
        }
        TransactionIdentity::Solana(signature) => {
            output.push(2);
            output.extend_from_slice(signature);
        }
    }
}

fn decode_transaction(reader: &mut Reader<'_>) -> Result<TransactionIdentity, StoreError> {
    match reader.u8()? {
        0 => Ok(TransactionIdentity::None),
        1 => Ok(TransactionIdentity::Ethereum(reader.array()?)),
        2 => Ok(TransactionIdentity::Solana(reader.array()?)),
        _ => Err(StoreError::Corrupt),
    }
}

fn encode_position(output: &mut Vec<u8>, value: FinalityPosition) {
    match value {
        FinalityPosition::None => output.push(0),
        FinalityPosition::Ethereum {
            block_number,
            block_hash,
        } => {
            output.push(1);
            output.extend_from_slice(&block_number.to_be_bytes());
            output.extend_from_slice(&block_hash);
        }
        FinalityPosition::Solana { slot, blockhash } => {
            output.push(2);
            output.extend_from_slice(&slot.to_be_bytes());
            output.extend_from_slice(&blockhash);
        }
    }
}

fn decode_position(reader: &mut Reader<'_>) -> Result<FinalityPosition, StoreError> {
    match reader.u8()? {
        0 => Ok(FinalityPosition::None),
        1 => Ok(FinalityPosition::Ethereum {
            block_number: reader.u64()?,
            block_hash: reader.array()?,
        }),
        2 => Ok(FinalityPosition::Solana {
            slot: reader.u64()?,
            blockhash: reader.array()?,
        }),
        _ => Err(StoreError::Corrupt),
    }
}

fn read_head(path: &Path) -> Result<Option<JournalHead>, StoreError> {
    let mut file = match File::open(path) {
        Ok(file) => file,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(StoreError::Io(error)),
    };
    let mut bytes = Vec::new();
    file.read_to_end(&mut bytes).map_err(StoreError::Io)?;
    if bytes.len() != 8 + 8 + 8 + 32 || bytes.get(..8) != Some(HEAD_MAGIC) {
        return Err(StoreError::Corrupt);
    }
    Ok(Some(JournalHead {
        sequence: u64::from_be_bytes(bytes[8..16].try_into().map_err(|_| StoreError::Corrupt)?),
        offset: u64::from_be_bytes(bytes[16..24].try_into().map_err(|_| StoreError::Corrupt)?),
        digest: bytes[24..56].try_into().map_err(|_| StoreError::Corrupt)?,
    }))
}

fn write_head(directory: &Path, path: &Path, head: JournalHead) -> Result<(), StoreError> {
    let temporary = directory.join(format!(
        ".{}.tmp",
        path.file_name()
            .and_then(|n| n.to_str())
            .ok_or(StoreError::Corrupt)?
    ));
    let mut file = OpenOptions::new()
        .create(true)
        .truncate(true)
        .write(true)
        .open(&temporary)
        .map_err(StoreError::Io)?;
    file.write_all(HEAD_MAGIC).map_err(StoreError::Io)?;
    file.write_all(&head.sequence.to_be_bytes())
        .map_err(StoreError::Io)?;
    file.write_all(&head.offset.to_be_bytes())
        .map_err(StoreError::Io)?;
    file.write_all(&head.digest).map_err(StoreError::Io)?;
    file.sync_all().map_err(StoreError::Io)?;
    fs::rename(&temporary, path).map_err(StoreError::Io)?;
    sync_directory(directory)
}

fn encode_spool(bytes: &[u8], head: NodeHead) -> Result<Vec<u8>, StoreError> {
    let mut output = Vec::with_capacity(96 + bytes.len());
    output.extend_from_slice(SPOOL_MAGIC);
    output.extend_from_slice(&head.latest_sealed_batch.to_be_bytes());
    encode_checkpoint(&mut output, head.latest_finalised_checkpoint);
    push_bytes(&mut output, bytes, MAX_ARCHIVE_BYTES)?;
    let digest: [u8; 32] = Sha256::digest(&output).into();
    output.extend_from_slice(&digest);
    Ok(output)
}

fn decode_spool(bytes: &[u8], commitment: ArchiveCommitment) -> Result<SpooledArchive, StoreError> {
    if bytes.len() < 8 + 8 + 1 + 4 + 32 {
        return Err(StoreError::Corrupt);
    }
    let (payload, digest_bytes) = bytes.split_at(bytes.len() - 32);
    let digest: [u8; 32] = Sha256::digest(payload).into();
    if digest_bytes != digest {
        return Err(StoreError::Corrupt);
    }
    let mut reader = Reader::new(payload);
    if reader.take(8)? != SPOOL_MAGIC {
        return Err(StoreError::Corrupt);
    }
    let latest_sealed_batch = reader.u64()?;
    let latest_finalised_checkpoint = decode_checkpoint(&mut reader)?;
    let archive = reader.bytes(MAX_ARCHIVE_BYTES)?.to_vec();
    reader.finish()?;
    if crate::archive_commitment(&archive) != commitment {
        return Err(StoreError::Conflict);
    }
    Ok(SpooledArchive {
        bytes: archive,
        node_head: NodeHead {
            latest_sealed_batch,
            latest_finalised_checkpoint,
        },
    })
}

fn push_bytes(output: &mut Vec<u8>, value: &[u8], maximum: usize) -> Result<(), StoreError> {
    if value.len() > maximum {
        return Err(StoreError::Length);
    }
    output.extend_from_slice(
        &u32::try_from(value.len())
            .map_err(|_| StoreError::Length)?
            .to_be_bytes(),
    );
    output.extend_from_slice(value);
    Ok(())
}

struct Reader<'a> {
    bytes: &'a [u8],
    offset: usize,
}

impl<'a> Reader<'a> {
    const fn new(bytes: &'a [u8]) -> Self {
        Self { bytes, offset: 0 }
    }

    fn take(&mut self, length: usize) -> Result<&'a [u8], StoreError> {
        let end = self.offset.checked_add(length).ok_or(StoreError::Length)?;
        let value = self
            .bytes
            .get(self.offset..end)
            .ok_or(StoreError::Corrupt)?;
        self.offset = end;
        Ok(value)
    }

    fn u8(&mut self) -> Result<u8, StoreError> {
        self.take(1)?.first().copied().ok_or(StoreError::Corrupt)
    }

    fn u16(&mut self) -> Result<u16, StoreError> {
        self.take(2)?
            .try_into()
            .map(u16::from_be_bytes)
            .map_err(|_| StoreError::Corrupt)
    }

    fn u32(&mut self) -> Result<u32, StoreError> {
        self.take(4)?
            .try_into()
            .map(u32::from_be_bytes)
            .map_err(|_| StoreError::Corrupt)
    }

    fn u64(&mut self) -> Result<u64, StoreError> {
        self.take(8)?
            .try_into()
            .map(u64::from_be_bytes)
            .map_err(|_| StoreError::Corrupt)
    }

    fn array<const N: usize>(&mut self) -> Result<[u8; N], StoreError> {
        self.take(N)?.try_into().map_err(|_| StoreError::Corrupt)
    }

    fn bytes(&mut self, maximum: usize) -> Result<&'a [u8], StoreError> {
        let length = usize::try_from(self.u32()?).map_err(|_| StoreError::Length)?;
        if length > maximum {
            return Err(StoreError::Length);
        }
        self.take(length)
    }

    fn finish(self) -> Result<(), StoreError> {
        (self.offset == self.bytes.len())
            .then_some(())
            .ok_or(StoreError::Corrupt)
    }
}

fn sync_directory(path: &Path) -> Result<(), StoreError> {
    File::open(path)
        .and_then(|file| file.sync_all())
        .map_err(StoreError::Io)
}

fn lane_name(chain: MirrorChain) -> &'static str {
    match chain {
        MirrorChain::Ethereum => "ethereum",
        MirrorChain::Solana => "solana",
    }
}

fn hex(bytes: &[u8]) -> String {
    const DIGITS: &[u8; 16] = b"0123456789abcdef";
    let mut output = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        output.push(char::from(DIGITS[usize::from(byte >> 4)]));
        output.push(char::from(DIGITS[usize::from(byte & 0x0f)]));
    }
    output
}

fn decode_hex_fixed(value: &str) -> Option<[u8; 32]> {
    if value.len() != 64 {
        return None;
    }
    let mut output = [0_u8; 32];
    for (index, pair) in value.as_bytes().chunks_exact(2).enumerate() {
        let high = hex_value(pair[0])?;
        let low = hex_value(pair[1])?;
        output[index] = high << 4 | low;
    }
    Some(output)
}

fn hex_value(value: u8) -> Option<u8> {
    match value {
        b'0'..=b'9' => Some(value - b'0'),
        b'a'..=b'f' => Some(value - b'a' + 10),
        _ => None,
    }
}
