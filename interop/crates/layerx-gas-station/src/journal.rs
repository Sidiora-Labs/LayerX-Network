use std::collections::BTreeMap;
use std::fs::{File, OpenOptions};
use std::io::{BufRead as _, BufReader, Write as _};
use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::quote::{keccak, quote_digest, word, Address, Quote, Word};
use crate::tx::{recover, Fees};
use crate::SignedQuote;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum JournalError {
    Io,
    Corrupt,
    Conflict,
    Locked,
}
impl std::fmt::Display for JournalError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "journal refused: {self:?}")
    }
}
impl std::error::Error for JournalError {}

#[derive(Clone, Copy, Debug, Deserialize, Serialize, Eq, PartialEq, Ord, PartialOrd)]
#[serde(deny_unknown_fields)]
pub struct Key {
    pub sponsor: Address,
    pub quote_nonce: Word,
}

#[derive(Clone, Debug, Deserialize, Serialize, Eq, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct QuoteRecord {
    pub key: Key,
    pub chain_id: u64,
    pub paymaster: Address,
    pub account: Address,
    pub token: Address,
    pub maximum: Word,
    pub amount: Word,
    pub deadline: u64,
    pub gas_cost: Word,
    pub issued_at: u64,
    pub signature: Vec<u8>,
    pub fees: Fees,
}
impl QuoteRecord {
    /// # Errors
    /// Refuses malformed or incorrectly signed records.
    pub fn signed_quote(&self) -> Result<SignedQuote, JournalError> {
        let quote = Quote {
            sponsor: self.key.sponsor,
            token: self.token,
            max_token_amount: self.maximum,
            token_amount: self.amount,
            deadline: word(u128::from(self.deadline)),
            nonce: self.key.quote_nonce,
            gas_cost: self.gas_cost,
        };
        let digest = quote_digest(word(u128::from(self.chain_id)), self.account, &quote);
        let signature = self
            .signature
            .as_slice()
            .try_into()
            .map_err(|_| JournalError::Corrupt)?;
        if recover(digest, &signature).map_err(|_| JournalError::Corrupt)? != self.key.sponsor
            || self.amount == word(0)
            || self.amount > self.maximum
            || self.deadline < self.issued_at
            || self.gas_cost != word(self.fees.gas_cost().map_err(|_| JournalError::Corrupt)?)
        {
            return Err(JournalError::Corrupt);
        }
        Ok(SignedQuote {
            quote,
            digest,
            signature,
        })
    }
}
#[derive(Clone, Debug, Deserialize, Serialize, Eq, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct Submission {
    pub nonce: u64,
    pub hash: Word,
    pub raw: Vec<u8>,
}
#[derive(Clone, Debug, Deserialize, Serialize, Eq, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct Replacement {
    pub nonce: u64,
    pub fees: Fees,
    pub hash: Word,
    pub raw: Vec<u8>,
}

#[derive(Clone, Copy, Debug, Deserialize, Serialize, Eq, PartialEq)]
#[serde(tag = "outcome", rename_all = "snake_case", deny_unknown_fields)]
pub enum Completion {
    Consumed,
    Included {
        hash: Word,
        block_number: u64,
        sid_collected: Word,
        pax_spent: Word,
    },
    Reverted {
        hash: Word,
    },
    Cancelled {
        hash: Word,
        block_number: u64,
    },
}
#[derive(Clone, Debug, Deserialize, Serialize, Eq, PartialEq)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum Entry {
    Quoted {
        quote: Box<QuoteRecord>,
    },
    Prepared {
        key: Key,
        submission: Submission,
    },
    Completed {
        key: Key,
        account: Address,
        completion: Completion,
    },
    Released {
        key: Key,
        nonce: u64,
    },
    Replaced {
        key: Key,
        replacement: Replacement,
    },
    Cancelled {
        key: Key,
        hash: Word,
        block_number: u64,
    },
}
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Item {
    pub account: Address,
    pub quote: Option<QuoteRecord>,
    pub submission: Option<Submission>,
    pub replacement: Option<Replacement>,
    pub completion: Option<Completion>,
}
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct State {
    pub items: BTreeMap<Key, Item>,
}
impl State {
    /// Whether a live submission or replacement of `sponsor` holds `nonce`.
    #[must_use]
    pub fn holds(&self, sponsor: Address, nonce: u64) -> bool {
        self.items.iter().any(|(key, item)| {
            key.sponsor == sponsor
                && (item.submission.as_ref().is_some_and(|s| s.nonce == nonce)
                    || item.replacement.as_ref().is_some_and(|r| r.nonce == nonce))
        })
    }
    fn apply(&mut self, entry: &Entry) -> Result<(), JournalError> {
        match entry {
            Entry::Quoted { quote } => {
                quote.signed_quote()?;
                if self.items.contains_key(&quote.key) {
                    return Err(JournalError::Conflict);
                }
                self.items.insert(
                    quote.key,
                    Item {
                        account: quote.account,
                        quote: Some(*quote.clone()),
                        submission: None,
                        replacement: None,
                        completion: None,
                    },
                );
            }
            Entry::Prepared { key, submission } => {
                if submission.raw.first() != Some(&4) || keccak(&submission.raw) != submission.hash
                {
                    return Err(JournalError::Corrupt);
                }
                if self.holds(key.sponsor, submission.nonce) {
                    return Err(JournalError::Conflict);
                }
                let item = self.items.get_mut(key).ok_or(JournalError::Conflict)?;
                if item.quote.is_none() || item.submission.is_some() || item.completion.is_some() {
                    return Err(JournalError::Conflict);
                }
                item.submission = Some(submission.clone());
            }
            Entry::Completed {
                key,
                account,
                completion,
            } => {
                let item = self.items.entry(*key).or_insert(Item {
                    account: *account,
                    quote: None,
                    submission: None,
                    replacement: None,
                    completion: None,
                });
                if item.account != *account || item.completion.is_some() {
                    return Err(JournalError::Conflict);
                }
                match completion {
                    Completion::Consumed => (),
                    Completion::Included {
                        hash,
                        sid_collected,
                        ..
                    } => {
                        if item.submission.as_ref().is_none_or(|s| s.hash != *hash)
                            || item
                                .quote
                                .as_ref()
                                .is_none_or(|q| q.amount != *sid_collected)
                        {
                            return Err(JournalError::Conflict);
                        }
                    }
                    Completion::Reverted { hash } => {
                        if item.submission.as_ref().is_none_or(|s| s.hash != *hash) {
                            return Err(JournalError::Conflict);
                        }
                    }
                    Completion::Cancelled { .. } => return Err(JournalError::Conflict),
                }
                item.completion = Some(*completion);
            }
            Entry::Released { key, nonce } => {
                let item = self.items.get_mut(key).ok_or(JournalError::Conflict)?;
                if item.completion.is_some()
                    || item.replacement.is_some()
                    || item.submission.as_ref().is_none_or(|s| s.nonce != *nonce)
                {
                    return Err(JournalError::Conflict);
                }
                item.submission = None;
            }
            Entry::Replaced { key, replacement } => {
                if replacement.raw.first() != Some(&2)
                    || keccak(&replacement.raw) != replacement.hash
                {
                    return Err(JournalError::Corrupt);
                }
                let item = self.items.get_mut(key).ok_or(JournalError::Conflict)?;
                if item.completion.is_some()
                    || item.replacement.is_some()
                    || item
                        .submission
                        .as_ref()
                        .is_none_or(|s| s.nonce != replacement.nonce)
                {
                    return Err(JournalError::Conflict);
                }
                let original = item.quote.as_ref().ok_or(JournalError::Conflict)?.fees;
                if !replacement
                    .fees
                    .replaces(original)
                    .map_err(|_| JournalError::Corrupt)?
                {
                    return Err(JournalError::Corrupt);
                }
                item.replacement = Some(replacement.clone());
            }
            Entry::Cancelled {
                key,
                hash,
                block_number,
            } => {
                let item = self.items.get_mut(key).ok_or(JournalError::Conflict)?;
                if item.completion.is_some()
                    || item.replacement.as_ref().is_none_or(|r| r.hash != *hash)
                {
                    return Err(JournalError::Conflict);
                }
                item.completion = Some(Completion::Cancelled {
                    hash: *hash,
                    block_number: *block_number,
                });
            }
        }
        Ok(())
    }
}
pub struct Journal {
    file: File,
    state: State,
    poisoned: bool,
}
impl Journal {
    /// # Errors
    /// Refuses concurrent writers, malformed lines, torn tails and contradictory records.
    pub fn open(path: &Path) -> Result<Self, JournalError> {
        let file = OpenOptions::new()
            .read(true)
            .append(true)
            .create(true)
            .open(path)
            .map_err(|_| JournalError::Io)?;
        file.try_lock().map_err(|_| JournalError::Locked)?;
        let mut reader = BufReader::new(&file);
        let mut state = State::default();
        loop {
            let mut line = Vec::new();
            let count = reader
                .read_until(b'\n', &mut line)
                .map_err(|_| JournalError::Io)?;
            if count == 0 {
                break;
            }
            if line.last() != Some(&b'\n') || count > 4_194_304 {
                return Err(JournalError::Corrupt);
            }
            let entry = serde_json::from_slice(&line).map_err(|_| JournalError::Corrupt)?;
            state.apply(&entry)?;
        }
        file.sync_all().map_err(|_| JournalError::Io)?;
        let parent = path
            .parent()
            .filter(|p| !p.as_os_str().is_empty())
            .unwrap_or_else(|| Path::new("."));
        File::open(parent)
            .and_then(|f| f.sync_all())
            .map_err(|_| JournalError::Io)?;
        Ok(Self {
            file,
            state,
            poisoned: false,
        })
    }
    #[must_use]
    pub const fn state(&self) -> &State {
        &self.state
    }
    /// # Errors
    /// Refuses conflicting entries and permanently stops this writer after uncertain disk writes.
    pub fn append(&mut self, entry: &Entry) -> Result<(), JournalError> {
        if self.poisoned {
            return Err(JournalError::Io);
        }
        let mut next = self.state.clone();
        next.apply(entry)?;
        let mut line = serde_json::to_vec(entry).map_err(|_| JournalError::Corrupt)?;
        line.push(b'\n');
        if line.len() > 4_194_304 {
            return Err(JournalError::Corrupt);
        }
        self.poisoned = true;
        self.file
            .write_all(&line)
            .and_then(|()| self.file.flush())
            .and_then(|()| self.file.sync_all())
            .map_err(|_| JournalError::Io)?;
        self.state = next;
        self.poisoned = false;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::signer::QuoteSigner;
    #[test]
    fn append_replay_lock_conflict_and_corruption() -> Result<(), Box<dyn std::error::Error>> {
        let path =
            std::env::temp_dir().join(format!("paxeer-journal-{}.jsonl", std::process::id()));
        if path.exists() {
            std::fs::remove_file(&path)?;
        }
        let mut journal = Journal::open(&path)?;
        assert!(matches!(Journal::open(&path), Err(JournalError::Locked)));
        let entry = Entry::Completed {
            key: Key {
                sponsor: [1; 20],
                quote_nonce: word(2),
            },
            account: [3; 20],
            completion: Completion::Consumed,
        };
        journal.append(&entry)?;
        assert_eq!(journal.append(&entry), Err(JournalError::Conflict));
        let state = journal.state().clone();
        drop(journal);
        let journal = Journal::open(&path)?;
        assert_eq!(journal.state(), &state);
        drop(journal);
        let mut file = OpenOptions::new().append(true).open(&path)?;
        file.write_all(b"{")?;
        drop(file);
        assert!(matches!(Journal::open(&path), Err(JournalError::Corrupt)));
        std::fs::write(&path, b"bad\n")?;
        assert!(matches!(Journal::open(&path), Err(JournalError::Corrupt)));
        std::fs::remove_file(path)?;
        Ok(())
    }
    fn quoted(
        signer: &impl QuoteSigner,
        quote_nonce: u128,
    ) -> Result<Entry, Box<dyn std::error::Error>> {
        let fees = Fees {
            gas_limit: 200_000,
            max_fee_per_gas: 5_000_000_000_000,
            max_priority_fee_per_gas: 1_000_000_000,
        };
        let mut record = QuoteRecord {
            key: Key {
                sponsor: signer.address(),
                quote_nonce: word(quote_nonce),
            },
            chain_id: 1325,
            paymaster: [0x44; 20],
            account: [0x11; 20],
            token: [0x22; 20],
            maximum: word(3_200_000),
            amount: word(3_145_140),
            deadline: 1019,
            gas_cost: word(fees.gas_cost()?),
            issued_at: 1000,
            signature: vec![],
            fees,
        };
        let quote = Quote {
            sponsor: record.key.sponsor,
            token: record.token,
            max_token_amount: record.maximum,
            token_amount: record.amount,
            deadline: word(1019),
            nonce: record.key.quote_nonce,
            gas_cost: record.gas_cost,
        };
        record.signature = signer
            .sign_digest(quote_digest(word(1325), record.account, &quote))?
            .to_vec();
        Ok(Entry::Quoted {
            quote: Box::new(record),
        })
    }
    fn prepared(key: Key, nonce: u64, marker: u8) -> Entry {
        let raw = vec![4, marker];
        Entry::Prepared {
            key,
            submission: Submission {
                nonce,
                hash: keccak(&raw),
                raw,
            },
        }
    }
    #[test]
    fn release_replacement_and_cancellation_replay_and_refuse_conflicts(
    ) -> Result<(), Box<dyn std::error::Error>> {
        let signer = crate::signer::tests::signer()?;
        let path = std::env::temp_dir().join(format!(
            "paxeer-journal-lifecycle-{}.jsonl",
            std::process::id()
        ));
        if path.exists() {
            std::fs::remove_file(&path)?;
        }
        let key = |n: u128| Key {
            sponsor: signer.address(),
            quote_nonce: word(n),
        };
        let mut journal = Journal::open(&path)?;
        for n in [1, 2, 3] {
            journal.append(&quoted(&signer, n)?)?;
        }
        journal.append(&prepared(key(1), 5, 1))?;
        assert_eq!(
            journal.append(&prepared(key(2), 5, 2)),
            Err(JournalError::Conflict)
        );
        for refused in [
            Entry::Released {
                key: key(1),
                nonce: 6,
            },
            Entry::Released {
                key: key(2),
                nonce: 5,
            },
        ] {
            assert_eq!(journal.append(&refused), Err(JournalError::Conflict));
        }
        let release = Entry::Released {
            key: key(1),
            nonce: 5,
        };
        journal.append(&release)?;
        assert!(!journal.state().holds(signer.address(), 5));
        assert_eq!(journal.append(&release), Err(JournalError::Conflict));
        journal.append(&prepared(key(2), 5, 2))?;
        let Entry::Quoted { quote } = quoted(&signer, 2)? else {
            return Err("expected a quote".into());
        };
        let fees = quote.fees.replacement()?;
        let cancellation = crate::tx::sign_cancellation(1325, 5, fees, &signer)?;
        let replaced = |nonce: u64, fees: Fees| Entry::Replaced {
            key: key(2),
            replacement: Replacement {
                nonce,
                fees,
                hash: cancellation.hash,
                raw: cancellation.raw.clone(),
            },
        };
        let mut underpriced = fees;
        underpriced.max_fee_per_gas -= 1;
        assert_eq!(
            journal.append(&replaced(5, underpriced)),
            Err(JournalError::Corrupt)
        );
        assert_eq!(
            journal.append(&replaced(6, fees)),
            Err(JournalError::Conflict)
        );
        journal.append(&replaced(5, fees))?;
        assert_eq!(
            journal.append(&replaced(5, fees)),
            Err(JournalError::Conflict)
        );
        assert_eq!(
            journal.append(&Entry::Released {
                key: key(2),
                nonce: 5
            }),
            Err(JournalError::Conflict)
        );
        assert_eq!(
            journal.append(&prepared(key(3), 5, 3)),
            Err(JournalError::Conflict)
        );
        let cancelled = Completion::Cancelled {
            hash: cancellation.hash,
            block_number: 17,
        };
        assert_eq!(
            journal.append(&Entry::Completed {
                key: key(2),
                account: [0x11; 20],
                completion: cancelled,
            }),
            Err(JournalError::Conflict)
        );
        assert_eq!(
            journal.append(&Entry::Cancelled {
                key: key(2),
                hash: word(9),
                block_number: 17,
            }),
            Err(JournalError::Conflict)
        );
        journal.append(&Entry::Cancelled {
            key: key(2),
            hash: cancellation.hash,
            block_number: 17,
        })?;
        let state = journal.state().clone();
        drop(journal);
        let journal = Journal::open(&path)?;
        assert_eq!(journal.state(), &state);
        assert_eq!(journal.state().items[&key(1)].submission, None);
        assert_eq!(journal.state().items[&key(2)].completion, Some(cancelled));
        drop(journal);
        std::fs::remove_file(path)?;
        Ok(())
    }
}
