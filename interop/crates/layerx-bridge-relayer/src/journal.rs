//! Append-only relay journal. Every decision that must survive a restart is
//! one JSON line, written and fsynced before the action it records takes
//! effect outside the process: an item is observed before its scan cursor
//! moves, and a transaction is journaled with its exact signed bytes before it
//! is broadcast. On restart the relayer therefore rebroadcasts the same bytes
//! instead of signing a second transaction for the same bridge event.
//!
//! Items are keyed by `in:{chain}:{txHash}:{logIndex}` for Ethereum deposits
//! and `out:{chain}:{paxeerTxHash}:{nonce}` for Paxeer burns.

use std::collections::BTreeMap;
use std::fmt;
use std::fs::{File, OpenOptions};
use std::io::{BufRead as _, BufReader, Seek as _, SeekFrom, Write as _};
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::abi::LogPosition;
use crate::attestation::{InboundAttestation, OutboundAttestation};
use crate::hex;

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum JournalError {
    Io(String),
    Corrupt { line: usize },
    Conflict(String),
}

impl fmt::Display for JournalError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io(error) => write!(formatter, "journal i/o: {error}"),
            Self::Corrupt { line } => write!(formatter, "journal line {line} is corrupt"),
            Self::Conflict(detail) => write!(formatter, "journal conflict: {detail}"),
        }
    }
}

impl std::error::Error for JournalError {}

fn io(error: &std::io::Error) -> JournalError {
    JournalError::Io(error.to_string())
}

/// The journal key of an inbound deposit: `(chain, txHash, logIndex)`.
#[must_use]
pub fn inbound_key(chain_id: u64, tx_hash: &[u8; 32], log_index: u64) -> String {
    format!("in:{chain_id}:{}:{log_index}", hex::prefixed(tx_hash))
}

/// The journal key of an outbound burn: `(chain, paxeerTxHash, nonce)`.
#[must_use]
pub fn outbound_key(chain_id: u64, paxeer_tx_hash: &[u8; 32], nonce: u64) -> String {
    format!("out:{chain_id}:{}:{nonce}", hex::prefixed(paxeer_tx_hash))
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Position {
    pub block_number: u64,
    #[serde(with = "hex::fixed_serde")]
    pub block_hash: [u8; 32],
}

impl From<LogPosition> for Position {
    fn from(value: LogPosition) -> Self {
        Self {
            block_number: value.block_number,
            block_hash: value.block_hash,
        }
    }
}

/// The attested content of an observed bridge event.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(tag = "direction", rename_all = "snake_case", deny_unknown_fields)]
pub enum Observation {
    Inbound {
        chain_id: u64,
        #[serde(with = "hex::fixed_serde")]
        vault: [u8; 20],
        #[serde(with = "hex::fixed_serde")]
        tx_hash: [u8; 32],
        log_index: u64,
        #[serde(with = "hex::fixed_serde")]
        recipient: [u8; 32],
        #[serde(with = "hex::fixed_serde")]
        asset: [u8; 20],
        #[serde(with = "hex::fixed_serde")]
        amount: [u8; 32],
        position: Position,
    },
    Outbound {
        chain_id: u64,
        #[serde(with = "hex::fixed_serde")]
        vault: [u8; 20],
        #[serde(with = "hex::fixed_serde")]
        paxeer_tx_hash: [u8; 32],
        paxeer_nonce: u64,
        #[serde(with = "hex::fixed_serde")]
        recipient: [u8; 20],
        #[serde(with = "hex::fixed_serde")]
        asset: [u8; 20],
        #[serde(with = "hex::fixed_serde")]
        amount: [u8; 32],
        position: Position,
    },
}

impl Observation {
    #[must_use]
    pub fn inbound(attestation: &InboundAttestation, position: Position) -> Self {
        Self::Inbound {
            chain_id: attestation.chain_id,
            vault: attestation.vault,
            tx_hash: attestation.tx_hash,
            log_index: attestation.log_index,
            recipient: attestation.recipient,
            asset: attestation.asset,
            amount: attestation.amount,
            position,
        }
    }

    #[must_use]
    pub fn outbound(attestation: &OutboundAttestation, position: Position) -> Self {
        Self::Outbound {
            chain_id: attestation.chain_id,
            vault: attestation.vault,
            paxeer_tx_hash: attestation.paxeer_tx_hash,
            paxeer_nonce: attestation.paxeer_nonce,
            recipient: attestation.recipient,
            asset: attestation.asset,
            amount: attestation.amount,
            position,
        }
    }

    /// The journal key this observation is stored under.
    #[must_use]
    pub fn key(&self) -> String {
        match self {
            Self::Inbound {
                chain_id,
                tx_hash,
                log_index,
                ..
            } => inbound_key(*chain_id, tx_hash, *log_index),
            Self::Outbound {
                chain_id,
                paxeer_tx_hash,
                paxeer_nonce,
                ..
            } => outbound_key(*chain_id, paxeer_tx_hash, *paxeer_nonce),
        }
    }

    #[must_use]
    pub const fn inbound_attestation(&self) -> Option<InboundAttestation> {
        match *self {
            Self::Inbound {
                chain_id,
                vault,
                tx_hash,
                log_index,
                recipient,
                asset,
                amount,
                ..
            } => Some(InboundAttestation {
                chain_id,
                vault,
                tx_hash,
                log_index,
                recipient,
                asset,
                amount,
            }),
            Self::Outbound { .. } => None,
        }
    }

    #[must_use]
    pub const fn outbound_attestation(&self) -> Option<OutboundAttestation> {
        match *self {
            Self::Outbound {
                chain_id,
                vault,
                paxeer_tx_hash,
                paxeer_nonce,
                recipient,
                asset,
                amount,
                ..
            } => Some(OutboundAttestation {
                chain_id,
                vault,
                paxeer_tx_hash,
                paxeer_nonce,
                recipient,
                asset,
                amount,
            }),
            Self::Inbound { .. } => None,
        }
    }
}

/// How an item left the relay.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(tag = "outcome", rename_all = "snake_case", deny_unknown_fields)]
pub enum Completion {
    /// This relayer's transaction executed successfully and is final.
    Included {
        #[serde(with = "hex::fixed_serde")]
        tx_hash: [u8; 32],
        block_number: u64,
    },
    /// The destination already consumed the event's nullifier, through this
    /// or another relayer's transaction.
    AlreadyBridged,
}

/// One journal line.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum Entry {
    /// Every block below `next_block` of `stream` has been scanned.
    Cursor { stream: String, next_block: u64 },
    Observed {
        item: String,
        observation: Observation,
    },
    /// This relayer's attestor signature, `r || s || v`.
    Signed {
        item: String,
        #[serde(with = "hex::fixed_serde")]
        signature: [u8; 65],
    },
    /// A signed transaction, journaled before its first broadcast.
    Submitted {
        item: String,
        #[serde(with = "hex::fixed_serde")]
        submitter: [u8; 20],
        nonce: u64,
        #[serde(with = "hex::fixed_serde")]
        tx_hash: [u8; 32],
        #[serde(with = "hex::bytes_serde")]
        raw: Vec<u8>,
    },
    /// The transaction executed and reverted without consuming the nullifier.
    Reverted {
        item: String,
        #[serde(with = "hex::fixed_serde")]
        tx_hash: [u8; 32],
    },
    /// The transaction can no longer execute (refused, or its nonce was
    /// consumed by another transaction) and the nullifier is still open.
    Dropped {
        item: String,
        #[serde(with = "hex::fixed_serde")]
        tx_hash: [u8; 32],
    },
    Completed {
        item: String,
        completion: Completion,
    },
    /// The event can never be bridged as observed; it is not signed.
    Refused { item: String, reason: String },
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SubmissionStatus {
    Pending,
    Reverted,
    Dropped,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Submission {
    pub submitter: [u8; 20],
    pub nonce: u64,
    pub tx_hash: [u8; 32],
    pub raw: Vec<u8>,
    pub status: SubmissionStatus,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Item {
    pub observation: Observation,
    pub signature: Option<[u8; 65]>,
    pub submissions: Vec<Submission>,
    pub completion: Option<Completion>,
    pub refusal: Option<String>,
}

impl Item {
    /// Whether the relayer still has work to do for this item.
    #[must_use]
    pub const fn is_open(&self) -> bool {
        self.completion.is_none() && self.refusal.is_none()
    }

    /// The transaction still awaiting an outcome, if any.
    #[must_use]
    pub fn pending(&self) -> Option<&Submission> {
        self.submissions
            .iter()
            .find(|submission| submission.status == SubmissionStatus::Pending)
    }
}

/// The state every journal line replays into.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct State {
    pub cursors: BTreeMap<String, u64>,
    pub items: BTreeMap<String, Item>,
}

impl State {
    fn item_mut(&mut self, item: &str) -> Result<&mut Item, JournalError> {
        self.items
            .get_mut(item)
            .ok_or_else(|| JournalError::Conflict(format!("{item} was never observed")))
    }

    fn apply(&mut self, entry: &Entry) -> Result<(), JournalError> {
        match entry {
            Entry::Cursor { stream, next_block } => {
                let current = self.cursors.entry(stream.clone()).or_insert(*next_block);
                if *next_block < *current {
                    return Err(JournalError::Conflict(format!(
                        "cursor {stream} moved back from {current} to {next_block}"
                    )));
                }
                *current = *next_block;
            }
            Entry::Observed { item, observation } => {
                if observation.key() != *item {
                    return Err(JournalError::Conflict(format!(
                        "{item} does not match its observation"
                    )));
                }
                if let Some(existing) = self.items.get(item) {
                    if existing.observation != *observation {
                        return Err(JournalError::Conflict(format!(
                            "{item} observed with different content"
                        )));
                    }
                } else {
                    self.items.insert(
                        item.clone(),
                        Item {
                            observation: *observation,
                            signature: None,
                            submissions: Vec::new(),
                            completion: None,
                            refusal: None,
                        },
                    );
                }
            }
            Entry::Signed { item, signature } => {
                let record = self.item_mut(item)?;
                if record
                    .signature
                    .is_some_and(|existing| existing != *signature)
                {
                    return Err(JournalError::Conflict(format!("{item} signed twice")));
                }
                record.signature = Some(*signature);
            }
            Entry::Submitted {
                item,
                submitter,
                nonce,
                tx_hash,
                raw,
            } => {
                let record = self.item_mut(item)?;
                if record.pending().is_some() {
                    return Err(JournalError::Conflict(format!(
                        "{item} already has a pending transaction"
                    )));
                }
                record.submissions.push(Submission {
                    submitter: *submitter,
                    nonce: *nonce,
                    tx_hash: *tx_hash,
                    raw: raw.clone(),
                    status: SubmissionStatus::Pending,
                });
            }
            Entry::Reverted { item, tx_hash } | Entry::Dropped { item, tx_hash } => {
                let status = if matches!(entry, Entry::Reverted { .. }) {
                    SubmissionStatus::Reverted
                } else {
                    SubmissionStatus::Dropped
                };
                let record = self.item_mut(item)?;
                let submission = record
                    .submissions
                    .iter_mut()
                    .find(|submission| {
                        submission.tx_hash == *tx_hash
                            && submission.status == SubmissionStatus::Pending
                    })
                    .ok_or_else(|| {
                        JournalError::Conflict(format!("{item} has no pending {tx_hash:02x?}"))
                    })?;
                submission.status = status;
            }
            Entry::Completed { item, completion } => {
                let record = self.item_mut(item)?;
                if record.completion.is_some() {
                    return Err(JournalError::Conflict(format!("{item} completed twice")));
                }
                record.completion = Some(*completion);
            }
            Entry::Refused { item, reason } => {
                self.item_mut(item)?.refusal = Some(reason.clone());
            }
        }
        Ok(())
    }
}

/// The journal file and the state it replays into.
pub struct Journal {
    path: PathBuf,
    file: File,
    state: State,
}

impl Journal {
    /// Opens or creates the journal and replays it. A final line without its
    /// terminating newline is a write torn by a crash: it never took effect
    /// outside the process and is truncated away. Any other malformed or
    /// conflicting line is fatal.
    ///
    /// # Errors
    ///
    /// Refuses unreadable, corrupt or self-contradictory journals.
    pub fn open(path: &Path) -> Result<Self, JournalError> {
        let mut file = OpenOptions::new()
            .read(true)
            .append(true)
            .create(true)
            .open(path)
            .map_err(|error| io(&error))?;
        let mut state = State::default();
        let mut reader = BufReader::new(&file);
        let mut valid_length = 0_u64;
        let mut line_number = 0_usize;
        loop {
            let mut line = String::new();
            let read = reader.read_line(&mut line).map_err(|error| io(&error))?;
            if read == 0 {
                break;
            }
            line_number += 1;
            if !line.ends_with('\n') {
                break;
            }
            let entry: Entry = serde_json::from_str(line.trim_end())
                .map_err(|_| JournalError::Corrupt { line: line_number })?;
            state.apply(&entry)?;
            valid_length +=
                u64::try_from(read).map_err(|_| JournalError::Corrupt { line: line_number })?;
        }
        drop(reader);
        let length = file.metadata().map_err(|error| io(&error))?.len();
        if length != valid_length {
            file.set_len(valid_length).map_err(|error| io(&error))?;
            file.sync_all().map_err(|error| io(&error))?;
        }
        file.seek(SeekFrom::End(0)).map_err(|error| io(&error))?;
        if let Some(directory) = path
            .parent()
            .filter(|parent| !parent.as_os_str().is_empty())
        {
            File::open(directory)
                .and_then(|directory| directory.sync_all())
                .map_err(|error| io(&error))?;
        }
        Ok(Self {
            path: path.to_path_buf(),
            file,
            state,
        })
    }

    #[must_use]
    pub fn path(&self) -> &Path {
        &self.path
    }

    #[must_use]
    pub const fn state(&self) -> &State {
        &self.state
    }

    /// Validates, writes, fsyncs and applies one entry, in that order.
    ///
    /// # Errors
    ///
    /// Refuses an entry that contradicts the journal, and any i/o failure.
    pub fn append(&mut self, entry: &Entry) -> Result<(), JournalError> {
        let mut next = self.state.clone();
        next.apply(entry)?;
        let mut line =
            serde_json::to_vec(entry).map_err(|error| JournalError::Io(error.to_string()))?;
        line.push(b'\n');
        self.file.write_all(&line).map_err(|error| io(&error))?;
        self.file.sync_data().map_err(|error| io(&error))?;
        self.state = next;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use std::fs;

    use super::*;

    fn directory(name: &str) -> PathBuf {
        let path = std::env::temp_dir().join(format!(
            "layerx-bridge-relayer-journal-{}-{name}",
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&path);
        fs::create_dir_all(&path).unwrap_or_else(|error| panic!("temp dir: {error}"));
        path
    }

    fn observation() -> Observation {
        Observation::inbound(
            &InboundAttestation {
                chain_id: 1,
                vault: [0x11; 20],
                tx_hash: [0xaa; 32],
                log_index: 3,
                recipient: [0x55; 32],
                asset: [0x44; 20],
                amount: [0x01; 32],
            },
            Position {
                block_number: 100,
                block_hash: [0xbb; 32],
            },
        )
    }

    fn open(path: &Path) -> Journal {
        Journal::open(path).unwrap_or_else(|error| panic!("journal: {error}"))
    }

    fn append(journal: &mut Journal, entry: &Entry) {
        journal
            .append(entry)
            .unwrap_or_else(|error| panic!("append: {error}"));
    }

    #[test]
    fn a_reopened_journal_replays_to_the_same_state() {
        let path = directory("replay").join("journal.jsonl");
        let key = observation().key();
        assert_eq!(key, format!("in:1:0x{}:3", "aa".repeat(32)));
        let before = {
            let mut journal = open(&path);
            append(
                &mut journal,
                &Entry::Observed {
                    item: key.clone(),
                    observation: observation(),
                },
            );
            append(
                &mut journal,
                &Entry::Cursor {
                    stream: "in:1".to_owned(),
                    next_block: 109,
                },
            );
            append(
                &mut journal,
                &Entry::Signed {
                    item: key.clone(),
                    signature: [0x1b; 65],
                },
            );
            append(
                &mut journal,
                &Entry::Submitted {
                    item: key.clone(),
                    submitter: [0xb1; 20],
                    nonce: 5,
                    tx_hash: [0xcc; 32],
                    raw: vec![2, 3, 4],
                },
            );
            journal.state().clone()
        };
        let reopened = open(&path);
        assert_eq!(reopened.state(), &before);
        let item = &reopened.state().items[&key];
        assert_eq!(item.pending().map(|submission| submission.nonce), Some(5));
        assert_eq!(
            item.pending().map(|submission| submission.raw.clone()),
            Some(vec![2, 3, 4])
        );
        assert_eq!(reopened.state().cursors["in:1"], 109);
    }

    #[test]
    fn a_second_pending_transaction_and_a_backward_cursor_are_refused() {
        let path = directory("conflicts").join("journal.jsonl");
        let key = observation().key();
        let mut journal = open(&path);
        append(
            &mut journal,
            &Entry::Observed {
                item: key.clone(),
                observation: observation(),
            },
        );
        let submitted = Entry::Submitted {
            item: key.clone(),
            submitter: [0xb1; 20],
            nonce: 5,
            tx_hash: [0xcc; 32],
            raw: vec![1],
        };
        append(&mut journal, &submitted);
        assert!(matches!(
            journal.append(&submitted),
            Err(JournalError::Conflict(_))
        ));
        append(
            &mut journal,
            &Entry::Cursor {
                stream: "in:1".to_owned(),
                next_block: 10,
            },
        );
        assert!(matches!(
            journal.append(&Entry::Cursor {
                stream: "in:1".to_owned(),
                next_block: 9,
            }),
            Err(JournalError::Conflict(_))
        ));
        let mut different = observation();
        if let Observation::Inbound { amount, .. } = &mut different {
            *amount = [0x02; 32];
        }
        assert!(matches!(
            journal.append(&Entry::Observed {
                item: key,
                observation: different,
            }),
            Err(JournalError::Conflict(_))
        ));
        let lines = fs::read_to_string(&path)
            .unwrap_or_default()
            .lines()
            .count();
        assert_eq!(lines, 3, "refused entries are never written");
    }

    #[test]
    fn a_torn_final_line_is_truncated_and_a_corrupt_middle_line_is_fatal() {
        let path = directory("torn").join("journal.jsonl");
        let key = observation().key();
        {
            let mut journal = open(&path);
            append(
                &mut journal,
                &Entry::Observed {
                    item: key.clone(),
                    observation: observation(),
                },
            );
        }
        let intact = fs::read(&path).unwrap_or_default();
        let mut torn = intact.clone();
        torn.extend_from_slice(b"{\"kind\":\"signed\",\"item\":\"in:1");
        fs::write(&path, &torn).unwrap_or_else(|error| panic!("write: {error}"));
        let mut journal = open(&path);
        assert_eq!(fs::read(&path).unwrap_or_default(), intact);
        append(
            &mut journal,
            &Entry::Refused {
                item: key,
                reason: "test".to_owned(),
            },
        );
        drop(journal);
        let mut corrupt = b"not json\n".to_vec();
        corrupt.extend_from_slice(&fs::read(&path).unwrap_or_default());
        fs::write(&path, &corrupt).unwrap_or_else(|error| panic!("write: {error}"));
        assert!(matches!(
            Journal::open(&path),
            Err(JournalError::Corrupt { line: 1 })
        ));
    }
}
