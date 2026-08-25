use std::collections::BTreeMap;
use std::fs::{self, File, OpenOptions};
use std::io::{ErrorKind, Write};
use std::path::PathBuf;
use std::os::unix::fs::{DirBuilderExt as _, OpenOptionsExt as _, PermissionsExt as _};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sha2::{Digest as _, Sha256};
use subtle::ConstantTimeEq;
use zeroize::{Zeroize, Zeroizing};

use crate::rpc::{RpcCluster, RpcQuorumConfig};
use crate::source_codec::{decode_fixed_hex, hex};
use crate::MigrationError;

const JOURNAL_DOMAIN: &[u8] = b"LayerX/interop/migration/journal/v1\0";
const HEAD_DOMAIN: &[u8] = b"LayerX/interop/migration/journal-head/v1\0";
const CURSOR_DOMAIN: &[u8] = b"LayerX/interop/migration/cursor/v1\0";
const MAX_RECORD_BYTES: usize = 128 * 1024;
const MAX_RECORDS: usize = 2_000_000;
static TEMP_SEQUENCE: AtomicU64 = AtomicU64::new(1);

/// Authenticated append-only state location. The authentication key is read
/// from the named file and is never serialized with journal records.
#[derive(Clone, Debug, Eq, PartialEq, Deserialize)]
pub struct JournalConfig {
    pub directory: PathBuf,
    pub authentication_key_file: PathBuf,
    pub namespace: String,
    pub rollback_anchor_id: String,
    pub rollback_anchor_rpc: RpcQuorumConfig,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub(crate) struct ChainCheckpoint {
    pub(crate) height: u64,
    pub(crate) hash: [u8; 32],
    pub(crate) parent_hash: [u8; 32],
    pub(crate) previous_height: u64,
    pub(crate) previous_hash: [u8; 32],
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
struct ClaimCheckpoint {
    height: u64,
    block_hash: [u8; 32],
    evidence_digest: [u8; 32],
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
struct HistoryCheckpoint {
    previous_cursor: Option<[u8; 32]>,
    previous_anchor_hash: Option<[u8; 32]>,
    from: u64,
    to: u64,
    anchor_hash: [u8; 32],
    evidence_digest: [u8; 32],
    next_cursor: [u8; 32],
    committed: bool,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "kebab-case")]
enum Update {
    Chain {
        network: String,
        checkpoint: ChainCheckpoint,
    },
    Claim {
        key: String,
        checkpoint: ClaimCheckpoint,
    },
    HistoryPrepared {
        stream: String,
        checkpoint: HistoryCheckpoint,
    },
    HistoryCommitted {
        stream: String,
        evidence_digest: [u8; 32],
        next_cursor: [u8; 32],
    },
    Ownership {
        key: String,
        evidence_digest: [u8; 32],
    },
    CustodyReference {
        key: String,
        claim_digest: [u8; 32],
    },
}

#[derive(Serialize, Deserialize)]
struct RecordBody {
    sequence: u64,
    previous: [u8; 32],
    update: Update,
}

#[derive(Serialize, Deserialize)]
struct Record {
    body: RecordBody,
    mac: [u8; 32],
}

#[derive(Serialize, Deserialize)]
struct HeadBody {
    sequence: u64,
    digest: [u8; 32],
}

#[derive(Serialize, Deserialize)]
struct HeadSeal {
    body: HeadBody,
    mac: [u8; 32],
}

#[derive(Default)]
struct State {
    sequence: u64,
    digest: [u8; 32],
    chains: BTreeMap<String, ChainCheckpoint>,
    claims: BTreeMap<String, ClaimCheckpoint>,
    histories: BTreeMap<String, HistoryCheckpoint>,
    ownership: BTreeMap<String, [u8; 32]>,
    custody_references: BTreeMap<String, [u8; 32]>,
}

pub(crate) struct Journal {
    directory: PathBuf,
    key: Zeroizing<Vec<u8>>,
    rollback_anchor_id: String,
    rollback_anchor: RpcCluster,
}

impl Journal {
    pub(crate) fn new(config: &JournalConfig) -> Result<Self, MigrationError> {
        if !valid_name(&config.namespace)
            || !valid_key(&config.rollback_anchor_id)
            || !config.directory.is_absolute()
            || !config.authentication_key_file.is_absolute()
        {
            return Err(MigrationError::Configuration);
        }
        let key_metadata = fs::symlink_metadata(&config.authentication_key_file)
            .map_err(|_| MigrationError::Configuration)?;
        if !key_metadata.file_type().is_file()
            || key_metadata.permissions().mode() & 0o077 != 0
        {
            return Err(MigrationError::Configuration);
        }
        let mut key =
            fs::read(&config.authentication_key_file).map_err(|_| MigrationError::Configuration)?;
        while matches!(key.last(), Some(b'\r' | b'\n')) {
            key.pop();
        }
        if !(32..=64).contains(&key.len()) {
            key.zeroize();
            return Err(MigrationError::Configuration);
        }
        let directory = config.directory.join(&config.namespace);
        let mut builder = fs::DirBuilder::new();
        builder.recursive(true).mode(0o700);
        builder
            .create(&directory)
            .map_err(|_| MigrationError::Configuration)?;
        let directory_metadata = fs::symlink_metadata(&directory)
            .map_err(|_| MigrationError::Configuration)?;
        if !directory_metadata.file_type().is_dir()
            || directory_metadata.permissions().mode() & 0o077 != 0
        {
            return Err(MigrationError::Configuration);
        }
        let journal = Self {
            directory,
            key: Zeroizing::new(key),
            rollback_anchor_id: config.rollback_anchor_id.clone(),
            rollback_anchor: RpcCluster::new(&config.rollback_anchor_rpc)?,
        };
        journal.load()?;
        Ok(journal)
    }

    pub(crate) fn checkpoint(
        &self,
        network: &str,
    ) -> Result<Option<ChainCheckpoint>, MigrationError> {
        Ok(self.load()?.chains.get(network).cloned())
    }

    pub(crate) fn record_chain(
        &self,
        network: &str,
        checkpoint: ChainCheckpoint,
    ) -> Result<(), MigrationError> {
        if !valid_key(network)
            || checkpoint.height == 0
            || checkpoint.hash == [0; 32]
            || checkpoint.parent_hash == [0; 32]
            || (checkpoint.previous_height == 0) != (checkpoint.previous_hash == [0; 32])
        {
            return Err(MigrationError::CheckpointConflict);
        }
        self.append(Update::Chain {
            network: network.to_owned(),
            checkpoint,
        })
    }

    pub(crate) fn record_claim(
        &self,
        key: &str,
        height: u64,
        block_hash: [u8; 32],
        evidence_digest: [u8; 32],
    ) -> Result<(), MigrationError> {
        if !valid_key(key) || height == 0 || block_hash == [0; 32] || evidence_digest == [0; 32] {
            return Err(MigrationError::CheckpointConflict);
        }
        self.append(Update::Claim {
            key: key.to_owned(),
            checkpoint: ClaimCheckpoint {
                height,
                block_hash,
                evidence_digest,
            },
        })
    }

    #[allow(clippy::too_many_arguments)]
    pub(crate) fn prepare_history(
        &self,
        stream: &str,
        previous_cursor: Option<[u8; 32]>,
        from: u64,
        to: u64,
        anchor_hash: [u8; 32],
        evidence_digest: [u8; 32],
        next_cursor: [u8; 32],
    ) -> Result<(), MigrationError> {
        if !valid_key(stream)
            || from == 0
            || to < from
            || anchor_hash == [0; 32]
            || evidence_digest == [0; 32]
            || next_cursor == [0; 32]
        {
            return Err(MigrationError::CheckpointConflict);
        }
        let state = self.load()?;
        let previous_anchor_hash = match state.histories.get(stream) {
            Some(current) if current.previous_cursor == previous_cursor => {
                current.previous_anchor_hash
            }
            Some(current) if current.committed && previous_cursor == Some(current.next_cursor) => {
                Some(current.anchor_hash)
            }
            None if previous_cursor.is_none() => None,
            _ => return Err(MigrationError::CheckpointConflict),
        };
        self.append(Update::HistoryPrepared {
            stream: stream.to_owned(),
            checkpoint: HistoryCheckpoint {
                previous_cursor,
                previous_anchor_hash,
                from,
                to,
                anchor_hash,
                evidence_digest,
                next_cursor,
                committed: false,
            },
        })
    }

    pub(crate) fn commit_history(
        &self,
        stream: &str,
        evidence_digest: [u8; 32],
        next_cursor: [u8; 32],
    ) -> Result<(), MigrationError> {
        if !valid_key(stream) || evidence_digest == [0; 32] || next_cursor == [0; 32] {
            return Err(MigrationError::CheckpointConflict);
        }
        self.append(Update::HistoryCommitted {
            stream: stream.to_owned(),
            evidence_digest,
            next_cursor,
        })
    }

    pub(crate) fn validate_history(
        &self,
        stream: &str,
        previous_cursor: Option<[u8; 32]>,
        from: u64,
        to: u64,
        evidence_digest: [u8; 32],
    ) -> Result<(), MigrationError> {
        if !valid_key(stream) || from == 0 || to < from || evidence_digest == [0; 32] {
            return Err(MigrationError::CheckpointConflict);
        }
        let state = self.load()?;
        match state.histories.get(stream) {
            Some(current)
                if current.previous_cursor == previous_cursor
                    && current.from == from
                    && current.to == to
                    && current.evidence_digest == evidence_digest =>
            {
                Ok(())
            }
            Some(current)
                if current.committed
                    && previous_cursor == Some(current.next_cursor)
                    && from == current.to.saturating_add(1) =>
            {
                Ok(())
            }
            None if previous_cursor.is_none() => Ok(()),
            _ => Err(MigrationError::CheckpointConflict),
        }
    }

    pub(crate) fn history_parent_anchor(
        &self,
        stream: &str,
        previous_cursor: Option<[u8; 32]>,
    ) -> Result<Option<[u8; 32]>, MigrationError> {
        if !valid_key(stream) {
            return Err(MigrationError::CheckpointConflict);
        }
        let state = self.load()?;
        match state.histories.get(stream) {
            Some(current) if current.previous_cursor == previous_cursor => {
                Ok(current.previous_anchor_hash)
            }
            Some(current) if current.committed && previous_cursor == Some(current.next_cursor) => {
                Ok(Some(current.anchor_hash))
            }
            None if previous_cursor.is_none() => Ok(None),
            _ => Err(MigrationError::CheckpointConflict),
        }
    }

    pub(crate) fn record_ownership(
        &self,
        key: &str,
        evidence_digest: [u8; 32],
    ) -> Result<(), MigrationError> {
        if !valid_key(key) || evidence_digest == [0; 32] {
            return Err(MigrationError::CheckpointConflict);
        }
        self.append(Update::Ownership {
            key: key.to_owned(),
            evidence_digest,
        })
    }

    pub(crate) fn record_custody_reference(
        &self,
        key: &str,
        claim_digest: [u8; 32],
    ) -> Result<(), MigrationError> {
        if !valid_key(key) || claim_digest == [0; 32] {
            return Err(MigrationError::CheckpointConflict);
        }
        self.append(Update::CustodyReference {
            key: key.to_owned(),
            claim_digest,
        })
    }

    pub(crate) fn cursor(&self, context: &[u8]) -> [u8; 32] {
        hmac(self.key.as_slice(), CURSOR_DOMAIN, context)
    }

    fn append(&self, update: Update) -> Result<(), MigrationError> {
        for _ in 0..64 {
            let mut state = self.load()?;
            if apply(&mut state, &update)? == Apply::Already {
                return Ok(());
            }
            let body = RecordBody {
                sequence: state.sequence.saturating_add(1),
                previous: state.digest,
                update: update.clone(),
            };
            let body_bytes =
                serde_json::to_vec(&body).map_err(|_| MigrationError::CheckpointIntegrity)?;
            let record = Record {
                mac: hmac(self.key.as_slice(), JOURNAL_DOMAIN, &body_bytes),
                body,
            };
            let bytes =
                serde_json::to_vec(&record).map_err(|_| MigrationError::CheckpointIntegrity)?;
            if bytes.len() > MAX_RECORD_BYTES {
                return Err(MigrationError::CheckpointIntegrity);
            }
            let temporary = self.temporary_path();
            let mut file = match OpenOptions::new()
                .create_new(true)
                .write(true)
                .mode(0o600)
                .open(&temporary)
            {
                Ok(file) => file,
                Err(error) if error.kind() == ErrorKind::AlreadyExists => continue,
                Err(_) => return Err(MigrationError::CheckpointIntegrity),
            };
            if file
                .write_all(&bytes)
                .and_then(|()| file.sync_all())
                .is_err()
            {
                let _ = fs::remove_file(&temporary);
                return Err(MigrationError::CheckpointIntegrity);
            }
            let final_path = self.record_path(record.body.sequence);
            match fs::hard_link(&temporary, &final_path) {
                Ok(()) => {
                    let digest = Sha256::digest(&bytes).into();
                    self.write_head_seal(record.body.sequence, digest)?;
                    let _ = fs::remove_file(&temporary);
                    File::open(&self.directory)
                        .and_then(|directory| directory.sync_all())
                        .map_err(|_| MigrationError::CheckpointIntegrity)?;
                    let state = self.load()?;
                    if state.sequence < record.body.sequence {
                        return Err(MigrationError::CheckpointIntegrity);
                    }
                    return Ok(());
                }
                Err(error) if error.kind() == ErrorKind::AlreadyExists => {
                    let _ = fs::remove_file(&temporary);
                }
                Err(_) => {
                    let _ = fs::remove_file(&temporary);
                    return Err(MigrationError::CheckpointIntegrity);
                }
            }
        }
        Err(MigrationError::CheckpointConflict)
    }

    fn load(&self) -> Result<State, MigrationError> {
        let mut records = Vec::new();
        let mut seals = Vec::new();
        for entry in
            fs::read_dir(&self.directory).map_err(|_| MigrationError::CheckpointIntegrity)?
        {
            let entry = entry.map_err(|_| MigrationError::CheckpointIntegrity)?;
            let name = entry
                .file_name()
                .into_string()
                .map_err(|_| MigrationError::CheckpointIntegrity)?;
            if let Some(sequence) = seal_sequence(&name) {
                if !entry
                    .file_type()
                    .map_err(|_| MigrationError::CheckpointIntegrity)?
                    .is_file()
                {
                    return Err(MigrationError::CheckpointIntegrity);
                }
                seals.push((sequence, entry.path()));
                continue;
            }
            let Some(sequence) = record_sequence(&name) else {
                if name.starts_with(".tmp-") {
                    continue;
                }
                return Err(MigrationError::CheckpointIntegrity);
            };
            if !entry
                .file_type()
                .map_err(|_| MigrationError::CheckpointIntegrity)?
                .is_file()
            {
                return Err(MigrationError::CheckpointIntegrity);
            }
            records.push((sequence, entry.path()));
        }
        records.sort_by_key(|(sequence, _)| *sequence);
        if records.len() > MAX_RECORDS {
            return Err(MigrationError::CheckpointIntegrity);
        }
        let mut state = State::default();
        let mut digests = BTreeMap::new();
        for (sequence, path) in records {
            if sequence != state.sequence.saturating_add(1) {
                return Err(MigrationError::CheckpointIntegrity);
            }
            let bytes = fs::read(path).map_err(|_| MigrationError::CheckpointIntegrity)?;
            if bytes.is_empty() || bytes.len() > MAX_RECORD_BYTES {
                return Err(MigrationError::CheckpointIntegrity);
            }
            let record: Record =
                serde_json::from_slice(&bytes).map_err(|_| MigrationError::CheckpointIntegrity)?;
            if record.body.sequence != sequence || record.body.previous != state.digest {
                return Err(MigrationError::CheckpointIntegrity);
            }
            let body_bytes = serde_json::to_vec(&record.body)
                .map_err(|_| MigrationError::CheckpointIntegrity)?;
            let expected = hmac(self.key.as_slice(), JOURNAL_DOMAIN, &body_bytes);
            if expected.ct_eq(&record.mac).unwrap_u8() != 1 {
                return Err(MigrationError::CheckpointIntegrity);
            }
            if apply(&mut state, &record.body.update)? != Apply::Applied {
                return Err(MigrationError::CheckpointIntegrity);
            }
            state.sequence = sequence;
            state.digest = Sha256::digest(&bytes).into();
            digests.insert(sequence, state.digest);
        }
        seals.sort_by_key(|(sequence, _)| *sequence);
        let mut highest_seal = 0_u64;
        for (sequence, path) in seals {
            let bytes = fs::read(path).map_err(|_| MigrationError::CheckpointIntegrity)?;
            if bytes.is_empty() || bytes.len() > MAX_RECORD_BYTES {
                return Err(MigrationError::CheckpointIntegrity);
            }
            let seal: HeadSeal =
                serde_json::from_slice(&bytes).map_err(|_| MigrationError::CheckpointIntegrity)?;
            if seal.body.sequence != sequence {
                return Err(MigrationError::CheckpointIntegrity);
            }
            let body =
                serde_json::to_vec(&seal.body).map_err(|_| MigrationError::CheckpointIntegrity)?;
            let expected = hmac(self.key.as_slice(), HEAD_DOMAIN, &body);
            if expected.ct_eq(&seal.mac).unwrap_u8() != 1
                || digests.get(&sequence) != Some(&seal.body.digest)
            {
                return Err(MigrationError::CheckpointIntegrity);
            }
            highest_seal = highest_seal.max(sequence);
        }
        if highest_seal > state.sequence {
            return Err(MigrationError::CheckpointIntegrity);
        }
        if state.sequence > highest_seal {
            self.write_head_seal(state.sequence, state.digest)?;
        }
        self.synchronize_rollback_anchor(&state, &digests)?;
        Ok(state)
    }

    fn synchronize_rollback_anchor(
        &self,
        state: &State,
        digests: &BTreeMap<u64, [u8; 32]>,
    ) -> Result<(), MigrationError> {
        let current = self.rollback_anchor.call(
            "layerx_getMigrationJournalHead",
            json!([self.rollback_anchor_id.as_str()]),
        )?;
        let (mut sequence, mut digest) = parse_anchor(&current)?;
        if sequence > state.sequence {
            return Err(MigrationError::CheckpointIntegrity);
        }
        let local = if sequence == 0 {
            [0; 32]
        } else {
            *digests
                .get(&sequence)
                .ok_or(MigrationError::CheckpointIntegrity)?
        };
        if local != digest {
            return Err(MigrationError::CheckpointIntegrity);
        }
        while sequence < state.sequence {
            let next_sequence = sequence.saturating_add(1);
            let next_digest = *digests
                .get(&next_sequence)
                .ok_or(MigrationError::CheckpointIntegrity)?;
            let advanced = self.rollback_anchor.call(
                "layerx_advanceMigrationJournalHead",
                json!([{
                    "anchor_id": self.rollback_anchor_id.as_str(),
                    "expected_sequence": sequence,
                    "expected_digest": hex(&digest),
                    "sequence": next_sequence,
                    "digest": hex(&next_digest)
                }]),
            )?;
            let (observed_sequence, observed_digest) = parse_anchor(&advanced)?;
            if observed_sequence != next_sequence || observed_digest != next_digest {
                return Err(MigrationError::CheckpointConflict);
            }
            sequence = observed_sequence;
            digest = observed_digest;
        }
        Ok(())
    }

    fn write_head_seal(&self, sequence: u64, digest: [u8; 32]) -> Result<(), MigrationError> {
        let body = HeadBody { sequence, digest };
        let body_bytes =
            serde_json::to_vec(&body).map_err(|_| MigrationError::CheckpointIntegrity)?;
        let seal = HeadSeal {
            mac: hmac(self.key.as_slice(), HEAD_DOMAIN, &body_bytes),
            body,
        };
        let bytes = serde_json::to_vec(&seal).map_err(|_| MigrationError::CheckpointIntegrity)?;
        let temporary = self.temporary_path();
        let mut file = OpenOptions::new()
            .create_new(true)
            .write(true)
            .mode(0o600)
            .open(&temporary)
            .map_err(|_| MigrationError::CheckpointIntegrity)?;
        file.write_all(&bytes)
            .and_then(|()| file.sync_all())
            .map_err(|_| MigrationError::CheckpointIntegrity)?;
        match fs::hard_link(&temporary, self.seal_path(sequence)) {
            Ok(()) => {}
            Err(error) if error.kind() == ErrorKind::AlreadyExists => {}
            Err(_) => {
                let _ = fs::remove_file(&temporary);
                return Err(MigrationError::CheckpointIntegrity);
            }
        }
        let _ = fs::remove_file(&temporary);
        File::open(&self.directory)
            .and_then(|directory| directory.sync_all())
            .map_err(|_| MigrationError::CheckpointIntegrity)
    }

    fn temporary_path(&self) -> PathBuf {
        let clock = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_or(0, |duration| duration.as_nanos());
        let local = TEMP_SEQUENCE.fetch_add(1, Ordering::Relaxed);
        self.directory
            .join(format!(".tmp-{}-{clock}-{local}", std::process::id()))
    }

    fn record_path(&self, sequence: u64) -> PathBuf {
        self.directory.join(format!("{sequence:020}.record"))
    }

    fn seal_path(&self, sequence: u64) -> PathBuf {
        self.directory.join(format!("{sequence:020}.seal"))
    }
}

#[derive(Clone, Copy, Eq, PartialEq)]
enum Apply {
    Applied,
    Already,
}

fn apply(state: &mut State, update: &Update) -> Result<Apply, MigrationError> {
    match update {
        Update::Chain {
            network,
            checkpoint,
        } => match state.chains.get(network) {
            Some(current) if current == checkpoint => Ok(Apply::Already),
            Some(current) if checkpoint.height < current.height => Ok(Apply::Already),
            Some(current) if checkpoint.height == current.height => {
                Err(MigrationError::CheckpointConflict)
            }
            Some(current)
                if checkpoint.previous_height != current.height
                    || checkpoint.previous_hash != current.hash =>
            {
                Err(MigrationError::CheckpointConflict)
            }
            None if checkpoint.previous_height != 0 || checkpoint.previous_hash != [0; 32] => {
                Err(MigrationError::CheckpointConflict)
            }
            _ => {
                state.chains.insert(network.clone(), checkpoint.clone());
                Ok(Apply::Applied)
            }
        },
        Update::Claim { key, checkpoint } => match state.claims.get(key) {
            Some(current) if current == checkpoint => Ok(Apply::Already),
            Some(_) => Err(MigrationError::CheckpointConflict),
            None => {
                state.claims.insert(key.clone(), checkpoint.clone());
                Ok(Apply::Applied)
            }
        },
        Update::HistoryPrepared { stream, checkpoint } => match state.histories.get(stream) {
            Some(current) if same_history_page(current, checkpoint) => Ok(Apply::Already),
            Some(current)
                if current.committed
                    && checkpoint.previous_cursor == Some(current.next_cursor)
                    && checkpoint.previous_anchor_hash == Some(current.anchor_hash)
                    && checkpoint.from == current.to.saturating_add(1) =>
            {
                state.histories.insert(stream.clone(), checkpoint.clone());
                Ok(Apply::Applied)
            }
            Some(_) => Err(MigrationError::CheckpointConflict),
            None if checkpoint.previous_cursor.is_none()
                && checkpoint.previous_anchor_hash.is_none() =>
            {
                state.histories.insert(stream.clone(), checkpoint.clone());
                Ok(Apply::Applied)
            }
            None => Err(MigrationError::CheckpointConflict),
        },
        Update::HistoryCommitted {
            stream,
            evidence_digest,
            next_cursor,
        } => match state.histories.get_mut(stream) {
            Some(current)
                if current.evidence_digest == *evidence_digest
                    && current.next_cursor == *next_cursor
                    && current.committed =>
            {
                Ok(Apply::Already)
            }
            Some(current)
                if current.evidence_digest == *evidence_digest
                    && current.next_cursor == *next_cursor
                    && !current.committed =>
            {
                current.committed = true;
                Ok(Apply::Applied)
            }
            _ => Err(MigrationError::CheckpointConflict),
        },
        Update::Ownership {
            key,
            evidence_digest,
        } => match state.ownership.get(key) {
            Some(current) if current == evidence_digest => Ok(Apply::Already),
            Some(_) => Err(MigrationError::CheckpointConflict),
            None => {
                state.ownership.insert(key.clone(), *evidence_digest);
                Ok(Apply::Applied)
            }
        },
        Update::CustodyReference { key, claim_digest } => match state.custody_references.get(key) {
            Some(current) if current == claim_digest => Ok(Apply::Already),
            Some(_) => Err(MigrationError::CheckpointConflict),
            None => {
                state.custody_references.insert(key.clone(), *claim_digest);
                Ok(Apply::Applied)
            }
        },
    }
}

fn same_history_page(left: &HistoryCheckpoint, right: &HistoryCheckpoint) -> bool {
    left.previous_cursor == right.previous_cursor
        && left.previous_anchor_hash == right.previous_anchor_hash
        && left.from == right.from
        && left.to == right.to
        && left.anchor_hash == right.anchor_hash
        && left.evidence_digest == right.evidence_digest
        && left.next_cursor == right.next_cursor
}

fn hmac(key: &[u8], domain: &[u8], context: &[u8]) -> [u8; 32] {
    let mut normalized = [0_u8; 64];
    if key.len() > normalized.len() {
        normalized[..32].copy_from_slice(&Sha256::digest(key));
    } else {
        normalized[..key.len()].copy_from_slice(key);
    }
    let mut inner_pad = [0x36_u8; 64];
    let mut outer_pad = [0x5c_u8; 64];
    for index in 0..64 {
        inner_pad[index] ^= normalized[index];
        outer_pad[index] ^= normalized[index];
    }
    let mut inner = Sha256::new();
    inner.update(inner_pad);
    inner.update((domain.len() as u64).to_be_bytes());
    inner.update(domain);
    inner.update((context.len() as u64).to_be_bytes());
    inner.update(context);
    let inner_digest = inner.finalize();
    let mut outer = Sha256::new();
    outer.update(outer_pad);
    outer.update(inner_digest);
    normalized.zeroize();
    inner_pad.zeroize();
    outer_pad.zeroize();
    outer.finalize().into()
}

fn record_sequence(name: &str) -> Option<u64> {
    let stem = name.strip_suffix(".record")?;
    if stem.len() != 20 || !stem.bytes().all(|byte| byte.is_ascii_digit()) {
        return None;
    }
    stem.parse().ok()
}

fn seal_sequence(name: &str) -> Option<u64> {
    let stem = name.strip_suffix(".seal")?;
    if stem.len() != 20 || !stem.bytes().all(|byte| byte.is_ascii_digit()) {
        return None;
    }
    stem.parse().ok()
}

fn parse_anchor(value: &Value) -> Result<(u64, [u8; 32]), MigrationError> {
    let sequence = value
        .get("sequence")
        .and_then(Value::as_u64)
        .ok_or(MigrationError::RpcResponseMismatch)?;
    let digest = value
        .get("digest")
        .and_then(Value::as_str)
        .ok_or(MigrationError::RpcResponseMismatch)?;
    if sequence == 0 {
        if digest != hex(&[0; 32]) {
            return Err(MigrationError::CheckpointIntegrity);
        }
        Ok((0, [0; 32]))
    } else {
        Ok((sequence, decode_fixed_hex(&format!("0x{digest}"))?))
    }
}

fn valid_name(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 64
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_'))
}

fn valid_key(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 512
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b':' | b'.'))
}
