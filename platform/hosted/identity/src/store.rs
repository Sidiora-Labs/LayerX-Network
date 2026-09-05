use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::fs::{self, File, OpenOptions};
use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};

const SNAPSHOT_FILE: &str = "snapshot.json";
const JOURNAL_FILE: &str = "journal.log";
const READY_MARKER_FILE: &str = "ready.marker";
const MAX_RECORD_BYTES: usize = 64 * 1024;
pub const MAX_SESSIONS_PER_PRINCIPAL: usize = 4096;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Principal {
    pub sub: String,
    pub allowed_signer_public_keys: Vec<String>,
    pub account: Option<String>,
    pub audiences: Vec<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Session {
    pub session_id: String,
    pub principal: String,
    pub token_digest: String,
    pub csrf_digest: String,
    pub csrf_sealed: String,
    pub issued_at: u64,
    pub expires_at: u64,
    pub revoked_at: Option<u64>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
enum Record {
    Principal(Principal),
    Session(Session),
    Revoke { session_id: String, revoked_at: u64 },
}

#[derive(Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Snapshot {
    principals: BTreeMap<String, Principal>,
    sessions: BTreeMap<String, Session>,
}

pub struct Store {
    directory: PathBuf,
    journal: File,
    principals: BTreeMap<String, Principal>,
    sessions: BTreeMap<String, Session>,
}

impl Store {
    pub fn open(directory: &Path) -> Result<Self, String> {
        fs::create_dir_all(directory).map_err(|error| format!("state directory: {error}"))?;
        let snapshot_path = directory.join(SNAPSHOT_FILE);
        let journal_path = directory.join(JOURNAL_FILE);
        let mut state = if snapshot_path.exists() {
            let bytes = fs::read(&snapshot_path).map_err(|error| format!("snapshot: {error}"))?;
            serde_json::from_slice::<Snapshot>(&bytes)
                .map_err(|error| format!("snapshot is not readable: {error}"))?
        } else {
            Snapshot::default()
        };
        if journal_path.exists() {
            replay_journal(&journal_path, &mut state)?;
        }
        write_snapshot(directory, &snapshot_path, &state)?;
        let journal = OpenOptions::new()
            .create(true)
            .write(true)
            .truncate(true)
            .open(&journal_path)
            .map_err(|error| format!("journal: {error}"))?;
        journal
            .sync_all()
            .map_err(|error| format!("journal sync: {error}"))?;
        sync_directory(directory)?;
        Ok(Self {
            directory: directory.to_path_buf(),
            journal,
            principals: state.principals,
            sessions: state.sessions,
        })
    }

    #[must_use]
    pub fn principal(&self, sub: &str) -> Option<&Principal> {
        self.principals.get(sub)
    }

    #[must_use]
    pub fn session(&self, session_id: &str) -> Option<&Session> {
        self.sessions.get(session_id)
    }

    pub fn put_principal(&mut self, principal: Principal) -> Result<(), String> {
        self.append(&Record::Principal(principal.clone()))?;
        self.principals.insert(principal.sub.clone(), principal);
        Ok(())
    }

    pub fn put_session(&mut self, session: Session) -> Result<(), String> {
        if !self.principals.contains_key(&session.principal) {
            return Err("session principal is unknown".to_owned());
        }
        if self.sessions.contains_key(&session.session_id) {
            return Err("session identifier already exists".to_owned());
        }
        let live = self
            .sessions
            .values()
            .filter(|existing| {
                existing.principal == session.principal && existing.revoked_at.is_none()
            })
            .count();
        if live >= MAX_SESSIONS_PER_PRINCIPAL {
            return Err("principal session bound reached".to_owned());
        }
        self.append(&Record::Session(session.clone()))?;
        self.sessions.insert(session.session_id.clone(), session);
        Ok(())
    }

    pub fn revoke_session(
        &mut self,
        session_id: &str,
        revoked_at: u64,
    ) -> Result<Option<u64>, String> {
        let Some(session) = self.sessions.get(session_id) else {
            return Ok(None);
        };
        if let Some(existing) = session.revoked_at {
            return Ok(Some(existing));
        }
        self.append(&Record::Revoke {
            session_id: session_id.to_owned(),
            revoked_at,
        })?;
        if let Some(session) = self.sessions.get_mut(session_id) {
            session.revoked_at = Some(revoked_at);
        }
        Ok(Some(revoked_at))
    }

    pub fn probe_writable(&self) -> Result<(), String> {
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

    fn append(&mut self, record: &Record) -> Result<(), String> {
        let mut line = serde_json::to_vec(record).map_err(|error| error.to_string())?;
        if line.len() >= MAX_RECORD_BYTES {
            return Err("journal record exceeds its bound".to_owned());
        }
        line.push(b'\n');
        self.journal
            .write_all(&line)
            .and_then(|()| self.journal.sync_all())
            .map_err(|error| format!("journal append: {error}"))
    }
}

fn apply(state: &mut Snapshot, record: Record) -> Result<(), String> {
    match record {
        Record::Principal(principal) => {
            state.principals.insert(principal.sub.clone(), principal);
        }
        Record::Session(session) => {
            if !state.principals.contains_key(&session.principal) {
                return Err("journal session references an unknown principal".to_owned());
            }
            state.sessions.insert(session.session_id.clone(), session);
        }
        Record::Revoke {
            session_id,
            revoked_at,
        } => {
            let session = state
                .sessions
                .get_mut(&session_id)
                .ok_or_else(|| "journal revocation references an unknown session".to_owned())?;
            session.revoked_at = Some(revoked_at);
        }
    }
    Ok(())
}

fn replay_journal(path: &Path, state: &mut Snapshot) -> Result<(), String> {
    let file = File::open(path).map_err(|error| format!("journal: {error}"))?;
    let mut reader = BufReader::new(file);
    let mut line = Vec::new();
    loop {
        line.clear();
        let count = reader
            .read_until(b'\n', &mut line)
            .map_err(|error| format!("journal read: {error}"))?;
        if count == 0 {
            return Ok(());
        }
        if line.last() != Some(&b'\n') {
            eprintln!("layerx-identity: discarding a torn trailing journal record");
            return Ok(());
        }
        if line.len() > MAX_RECORD_BYTES {
            return Err("journal record exceeds its bound".to_owned());
        }
        let record: Record = serde_json::from_slice(&line[..line.len() - 1])
            .map_err(|error| format!("journal record is not readable: {error}"))?;
        apply(state, record)?;
    }
}

fn write_snapshot(directory: &Path, path: &Path, state: &Snapshot) -> Result<(), String> {
    let temporary = directory.join(format!("{SNAPSHOT_FILE}.tmp"));
    let bytes = serde_json::to_vec(state).map_err(|error| error.to_string())?;
    let mut file = OpenOptions::new()
        .create(true)
        .write(true)
        .truncate(true)
        .open(&temporary)
        .map_err(|error| format!("snapshot: {error}"))?;
    file.write_all(&bytes)
        .and_then(|()| file.sync_all())
        .map_err(|error| format!("snapshot write: {error}"))?;
    fs::rename(&temporary, path).map_err(|error| format!("snapshot rename: {error}"))?;
    sync_directory(directory)
}

fn sync_directory(directory: &Path) -> Result<(), String> {
    File::open(directory)
        .and_then(|handle| handle.sync_all())
        .map_err(|error| format!("directory sync: {error}"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU64, Ordering};

    fn directory(name: &str) -> PathBuf {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let root = std::env::temp_dir().join(format!(
            "identity-store-{name}-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        let _ = fs::remove_dir_all(&root);
        root
    }

    fn principal(sub: &str) -> Principal {
        Principal {
            sub: sub.to_owned(),
            allowed_signer_public_keys: vec!["ab".repeat(32)],
            account: Some(format!("agent:{sub}:main")),
            audiences: vec!["ramp".to_owned()],
        }
    }

    fn session(id: &str, sub: &str, expires_at: u64) -> Session {
        Session {
            session_id: id.to_owned(),
            principal: sub.to_owned(),
            token_digest: "11".repeat(32),
            csrf_digest: "22".repeat(32),
            csrf_sealed: "33".repeat(48),
            issued_at: 1,
            expires_at,
            revoked_at: None,
        }
    }

    #[test]
    fn state_survives_reopen_and_compaction() {
        let root = directory("reopen");
        {
            let mut store = Store::open(&root).unwrap_or_else(|error| panic!("open: {error}"));
            store
                .put_principal(principal("did:key:alpha"))
                .unwrap_or_else(|error| panic!("principal: {error}"));
            store
                .put_session(session("s1", "did:key:alpha", 100))
                .unwrap_or_else(|error| panic!("session: {error}"));
            store
                .put_session(session("s2", "did:key:alpha", 200))
                .unwrap_or_else(|error| panic!("session: {error}"));
            assert_eq!(
                store
                    .revoke_session("s2", 50)
                    .unwrap_or_else(|error| panic!("revoke: {error}")),
                Some(50)
            );
            assert_eq!(
                store
                    .revoke_session("s2", 60)
                    .unwrap_or_else(|error| panic!("revoke: {error}")),
                Some(50)
            );
            assert_eq!(
                store
                    .revoke_session("missing", 60)
                    .unwrap_or_else(|error| panic!("revoke: {error}")),
                None
            );
        }
        let journal_before = fs::metadata(root.join(JOURNAL_FILE))
            .map(|metadata| metadata.len())
            .unwrap_or_default();
        assert!(journal_before > 0, "journal must hold the appended records");
        {
            let store = Store::open(&root).unwrap_or_else(|error| panic!("reopen: {error}"));
            assert_eq!(
                store.principal("did:key:alpha"),
                Some(&principal("did:key:alpha"))
            );
            assert_eq!(
                store.session("s1"),
                Some(&session("s1", "did:key:alpha", 100))
            );
            let revoked = store.session("s2").cloned();
            assert_eq!(revoked.and_then(|value| value.revoked_at), Some(50));
        }
        let journal_after = fs::metadata(root.join(JOURNAL_FILE))
            .map(|metadata| metadata.len())
            .unwrap_or_default();
        assert_eq!(
            journal_after, 0,
            "reopen compacts the journal into the snapshot"
        );
        assert!(root.join(SNAPSHOT_FILE).exists());
        let store = Store::open(&root).unwrap_or_else(|error| panic!("third open: {error}"));
        assert_eq!(
            store.session("s1"),
            Some(&session("s1", "did:key:alpha", 100))
        );
    }

    #[test]
    fn torn_trailing_record_is_discarded_and_malformed_records_refuse() {
        let root = directory("torn");
        {
            let mut store = Store::open(&root).unwrap_or_else(|error| panic!("open: {error}"));
            store
                .put_principal(principal("did:key:beta"))
                .unwrap_or_else(|error| panic!("principal: {error}"));
        }
        let journal = root.join(JOURNAL_FILE);
        let complete = serde_json::to_vec(&Record::Session(session("s9", "did:key:beta", 5)))
            .unwrap_or_default();
        let mut bytes = complete.clone();
        bytes.push(b'\n');
        bytes.extend_from_slice(&complete[..complete.len() / 2]);
        fs::write(&journal, &bytes).unwrap_or_else(|error| panic!("write: {error}"));
        {
            let store = Store::open(&root).unwrap_or_else(|error| panic!("reopen: {error}"));
            assert!(store.session("s9").is_some());
        }
        fs::write(
            &journal,
            b"{\"Revoke\":{\"session_id\":\"nope\",\"revoked_at\":1}}\n",
        )
        .unwrap_or_else(|error| panic!("write: {error}"));
        assert!(Store::open(&root).is_err());
        fs::write(&journal, b"not json\n").unwrap_or_else(|error| panic!("write: {error}"));
        assert!(Store::open(&root).is_err());
    }

    #[test]
    fn session_requires_a_known_principal_and_unique_identifier() {
        let root = directory("bounds");
        let mut store = Store::open(&root).unwrap_or_else(|error| panic!("open: {error}"));
        assert!(store.put_session(session("s1", "did:key:none", 1)).is_err());
        store
            .put_principal(principal("did:key:gamma"))
            .unwrap_or_else(|error| panic!("principal: {error}"));
        store
            .put_session(session("s1", "did:key:gamma", 1))
            .unwrap_or_else(|error| panic!("session: {error}"));
        assert!(store
            .put_session(session("s1", "did:key:gamma", 2))
            .is_err());
        store
            .probe_writable()
            .unwrap_or_else(|error| panic!("probe: {error}"));
        assert!(root.join(READY_MARKER_FILE).exists());
    }
}
