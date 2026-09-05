//! A durable append-only journal: every record is one JSON line, appended and
//! fsynced before the caller learns it was accepted, and replayed in order on
//! open. Readiness is answered by writing a marker into the same directory.

use serde::de::DeserializeOwned;
use serde::Serialize;
use std::fs::{self, File, OpenOptions};
use std::io::{BufRead, BufReader, Read, Write};
use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};
use std::path::{Path, PathBuf};

const JOURNAL_FILE: &str = "journal.log";
const READY_MARKER_FILE: &str = "ready.marker";
const MAX_RECORD_BYTES: usize = 64 * 1024;
const MAX_JOURNAL_RECORDS: usize = 1_000_000;

/// The open journal.
pub struct Journal {
    directory: PathBuf,
    file: File,
    records: usize,
    available: bool,
}

impl Journal {
    /// Opens or creates the journal under `directory` and replays every
    /// record into `apply` in append order.
    ///
    /// # Errors
    /// Returns a description when the directory or the journal is unusable or
    /// a line fails to decode.
    pub fn open<T: DeserializeOwned>(
        directory: &Path,
        mut apply: impl FnMut(T),
    ) -> Result<Self, String> {
        fs::create_dir_all(directory).map_err(|error| format!("state directory: {error}"))?;
        fs::set_permissions(directory, fs::Permissions::from_mode(0o700))
            .map_err(|error| error.to_string())?;
        let path = directory.join(JOURNAL_FILE);
        let file = OpenOptions::new()
            .create(true)
            .read(true)
            .append(true)
            .mode(0o600)
            .open(&path)
            .map_err(|error| error.to_string())?;
        file.try_lock()
            .map_err(|error| format!("journal lock: {error}"))?;
        sync_directory(directory)?;
        let mut records = 0_usize;
        if path.exists() {
            let mut reader = BufReader::new(
                File::open(&path).map_err(|error| format!("journal open: {error}"))?,
            );
            loop {
                let index = records;
                let mut bytes = Vec::new();
                let count = reader
                    .by_ref()
                    .take(
                        u64::try_from(MAX_RECORD_BYTES + 1)
                            .map_err(|_| "invalid journal bound".to_owned())?,
                    )
                    .read_until(b'\n', &mut bytes)
                    .map_err(|error| error.to_string())?;
                if count == 0 {
                    break;
                }
                if bytes.last() != Some(&b'\n') {
                    return Err("truncated or oversized journal record".to_owned());
                }
                let line = std::str::from_utf8(&bytes)
                    .map_err(|error| error.to_string())?
                    .trim_end_matches('\n');
                if line.is_empty() {
                    continue;
                }
                if line.len() > MAX_RECORD_BYTES {
                    return Err(format!("journal line {index} exceeds its bound"));
                }
                let record: T = serde_json::from_str(line)
                    .map_err(|error| format!("journal line {index}: {error}"))?;
                apply(record);
                records += 1;
                if records > MAX_JOURNAL_RECORDS {
                    return Err("journal exceeds its record bound".to_owned());
                }
            }
        }
        Ok(Self {
            directory: directory.to_path_buf(),
            file,
            records,
            available: true,
        })
    }

    /// Number of records replayed or appended.
    #[must_use]
    pub fn len(&self) -> usize {
        self.records
    }

    /// True when no record has been journaled.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.records == 0
    }

    /// Appends one record and fsyncs it.
    ///
    /// # Errors
    /// Returns a description when the record exceeds its bound or the write
    /// or sync fails; the journal is then marked unavailable.
    pub fn append<T: Serialize>(&mut self, record: &T) -> Result<(), String> {
        self.check_available()?;
        if self.records >= MAX_JOURNAL_RECORDS {
            return Err("journal is full".to_owned());
        }
        let mut line = serde_json::to_vec(record).map_err(|error| error.to_string())?;
        if line.len() >= MAX_RECORD_BYTES || line.contains(&b'\n') {
            return Err("journal record exceeds its bound".to_owned());
        }
        line.push(b'\n');
        if let Err(error) = self
            .file
            .write_all(&line)
            .and_then(|()| self.file.sync_all())
        {
            self.available = false;
            return Err(format!("journal append: {error}"));
        }
        self.records += 1;
        Ok(())
    }

    /// Proves the directory accepts durable writes by replacing the readiness
    /// marker with a synced file.
    ///
    /// # Errors
    /// Returns a description when the marker cannot be written and synced.
    pub fn probe_writable(&self) -> Result<(), String> {
        self.check_available()?;
        let temporary = self.directory.join(format!("{READY_MARKER_FILE}.tmp"));
        let marker = self.directory.join(READY_MARKER_FILE);
        let mut file = OpenOptions::new()
            .create(true)
            .write(true)
            .truncate(true)
            .open(&temporary)
            .map_err(|error| format!("ready marker: {error}"))?;
        file.write_all(b"ready\n")
            .and_then(|()| file.sync_all())
            .map_err(|error| format!("ready marker sync: {error}"))?;
        fs::rename(&temporary, &marker).map_err(|error| format!("ready marker rename: {error}"))?;
        sync_directory(&self.directory)
    }

    fn check_available(&self) -> Result<(), String> {
        if self.available {
            Ok(())
        } else {
            Err("journal is unavailable after a failed append".to_owned())
        }
    }
}

fn sync_directory(directory: &Path) -> Result<(), String> {
    File::open(directory)
        .and_then(|handle| handle.sync_all())
        .map_err(|error| format!("directory sync: {error}"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde::Deserialize;

    #[derive(Serialize, Deserialize, PartialEq, Eq, Debug)]
    struct Entry {
        id: String,
        sequence: u64,
    }

    fn temporary_directory(name: &str) -> PathBuf {
        let directory = std::env::temp_dir()
            .join("layerx-internal-tests")
            .join("journal")
            .join(format!("{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&directory);
        directory
    }

    #[test]
    fn appends_are_replayed_in_order_after_reopen() {
        let directory = temporary_directory("replay");
        let mut journal = Journal::open::<Entry>(&directory, |_| {})
            .unwrap_or_else(|error| panic!("open: {error}"));
        assert!(journal.is_empty());
        journal
            .probe_writable()
            .unwrap_or_else(|error| panic!("{error}"));
        for sequence in 1..=3 {
            journal
                .append(&Entry {
                    id: format!("e{sequence}"),
                    sequence,
                })
                .unwrap_or_else(|error| panic!("append: {error}"));
        }
        assert_eq!(journal.len(), 3);
        drop(journal);
        let mut replayed = Vec::new();
        let journal = Journal::open::<Entry>(&directory, |entry| replayed.push(entry))
            .unwrap_or_else(|error| panic!("reopen: {error}"));
        assert_eq!(journal.len(), 3);
        assert_eq!(
            replayed,
            vec![
                Entry {
                    id: "e1".to_owned(),
                    sequence: 1
                },
                Entry {
                    id: "e2".to_owned(),
                    sequence: 2
                },
                Entry {
                    id: "e3".to_owned(),
                    sequence: 3
                },
            ]
        );
        assert!(directory.join(READY_MARKER_FILE).exists());
        let _ = fs::remove_dir_all(&directory);
    }

    #[test]
    fn corrupt_lines_refuse_to_open() {
        let directory = temporary_directory("corrupt");
        fs::create_dir_all(&directory).unwrap_or_else(|error| panic!("{error}"));
        fs::write(
            directory.join(JOURNAL_FILE),
            b"{\"id\":\"e1\",\"sequence\":1}\nnot json\n",
        )
        .unwrap_or_else(|error| panic!("{error}"));
        assert!(Journal::open::<Entry>(&directory, |_| {}).is_err());
        let _ = fs::remove_dir_all(&directory);
    }
}
