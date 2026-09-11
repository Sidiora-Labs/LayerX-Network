use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::fs::{self, File, OpenOptions};
use std::io::{BufRead, BufReader, Write};
use std::os::unix::fs::MetadataExt;
use std::path::{Path, PathBuf};

const SNAPSHOT_FILE: &str = "snapshot.json";
const JOURNAL_FILE: &str = "journal.log";
const READY_MARKER_FILE: &str = "ready.marker";
const MAX_RECORD_BYTES: usize = 64 * 1024;
pub const MAX_SESSIONS_PER_PRINCIPAL: usize = 4096;
pub const SUBJECT_TENANT_CONFLICT: &str = "principal subject belongs to another tenant";

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Principal {
    pub tenant: String,
    pub sub: String,
    pub allowed_signer_public_keys: Vec<String>,
    pub account: Option<String>,
    pub audiences: Vec<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct StoredSession {
    pub session_id: String,
    pub tenant: String,
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
    Session(StoredSession),
    Revoke { session_id: String, revoked_at: u64 },
}

#[derive(Default, Deserialize)]
#[serde(deny_unknown_fields)]
struct Snapshot {
    principals: BTreeMap<String, BTreeMap<String, Principal>>,
    sessions: BTreeMap<String, StoredSession>,
}

#[derive(Serialize)]
struct SnapshotView<'a> {
    principals: &'a BTreeMap<String, BTreeMap<String, Principal>>,
    sessions: &'a BTreeMap<String, StoredSession>,
}

#[derive(Default)]
struct State {
    principals: BTreeMap<String, BTreeMap<String, Principal>>,
    sessions: BTreeMap<String, StoredSession>,
    subject_tenants: BTreeMap<String, String>,
}

impl State {
    fn restore(snapshot: Snapshot) -> Result<Self, String> {
        let mut state = Self::default();
        for (tenant, subjects) in snapshot.principals {
            for (sub, principal) in subjects {
                if principal.tenant != tenant || principal.sub != sub {
                    return Err(
                        "snapshot principal is not keyed by its tenant and subject".to_owned()
                    );
                }
                state.insert_principal(principal)?;
            }
        }
        for (session_id, session) in snapshot.sessions {
            if session.session_id != session_id {
                return Err("snapshot session is not keyed by its identifier".to_owned());
            }
            state.insert_session(session)?;
        }
        Ok(state)
    }

    const fn view(&self) -> SnapshotView<'_> {
        SnapshotView {
            principals: &self.principals,
            sessions: &self.sessions,
        }
    }

    fn principal(&self, tenant: &str, sub: &str) -> Option<&Principal> {
        self.principals
            .get(tenant)
            .and_then(|subjects| subjects.get(sub))
    }

    fn subject_conflict(&self, principal: &Principal) -> bool {
        self.subject_tenants
            .get(&principal.sub)
            .is_some_and(|bound| bound != &principal.tenant)
    }

    fn insert_principal(&mut self, principal: Principal) -> Result<(), String> {
        if self.subject_conflict(&principal) {
            return Err(SUBJECT_TENANT_CONFLICT.to_owned());
        }
        self.subject_tenants
            .insert(principal.sub.clone(), principal.tenant.clone());
        self.principals
            .entry(principal.tenant.clone())
            .or_default()
            .insert(principal.sub.clone(), principal);
        Ok(())
    }

    fn insert_session(&mut self, session: StoredSession) -> Result<(), String> {
        if self
            .principal(&session.tenant, &session.principal)
            .is_none()
        {
            return Err("session references no principal of its tenant".to_owned());
        }
        self.sessions.insert(session.session_id.clone(), session);
        Ok(())
    }

    fn live_sessions(&self, tenant: &str, sub: &str) -> usize {
        self.sessions
            .values()
            .filter(|existing| {
                existing.tenant == tenant
                    && existing.principal == sub
                    && existing.revoked_at.is_none()
            })
            .count()
    }
}

pub struct Store {
    directory: PathBuf,
    journal: File,
    directory_identity: (u64, u64),
    failed: bool,
    state: State,
}

impl Store {
    pub fn open(directory: &Path) -> Result<Self, String> {
        fs::create_dir_all(directory).map_err(|error| format!("state directory: {error}"))?;
        let snapshot_path = directory.join(SNAPSHOT_FILE);
        let journal_path = directory.join(JOURNAL_FILE);
        let snapshot = if snapshot_path.exists() {
            let bytes = fs::read(&snapshot_path).map_err(|error| format!("snapshot: {error}"))?;
            serde_json::from_slice::<Snapshot>(&bytes)
                .map_err(|error| format!("snapshot is not readable: {error}"))?
        } else {
            Snapshot::default()
        };
        let mut state = State::restore(snapshot)?;
        if journal_path.exists() {
            replay_journal(&journal_path, &mut state)?;
        }
        write_snapshot(directory, &snapshot_path, &state.view())?;
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
        let metadata = fs::metadata(directory).map_err(|error| error.to_string())?;
        Ok(Self {
            directory_identity: (metadata.dev(), metadata.ino()),
            failed: false,
            directory: directory.to_path_buf(),
            journal,
            state,
        })
    }

    #[must_use]
    pub fn principal(&self, tenant: &str, sub: &str) -> Option<&Principal> {
        self.state.principal(tenant, sub)
    }

    #[must_use]
    pub fn subject_tenant(&self, sub: &str) -> Option<&str> {
        self.state.subject_tenants.get(sub).map(String::as_str)
    }

    #[must_use]
    pub fn session(&self, session_id: &str) -> Option<&StoredSession> {
        self.state.sessions.get(session_id)
    }

    pub fn put_principal(&mut self, principal: Principal) -> Result<(), String> {
        if self.state.subject_conflict(&principal) {
            return Err(SUBJECT_TENANT_CONFLICT.to_owned());
        }
        self.append(&Record::Principal(principal.clone()))?;
        self.state.insert_principal(principal)
    }

    pub fn put_session(&mut self, session: StoredSession) -> Result<(), String> {
        if self
            .state
            .principal(&session.tenant, &session.principal)
            .is_none()
        {
            return Err("session principal is unknown".to_owned());
        }
        if self.state.sessions.contains_key(&session.session_id) {
            return Err("session identifier already exists".to_owned());
        }
        if self
            .state
            .live_sessions(&session.tenant, &session.principal)
            >= MAX_SESSIONS_PER_PRINCIPAL
        {
            return Err("principal session bound reached".to_owned());
        }
        self.append(&Record::Session(session.clone()))?;
        self.state.insert_session(session)
    }

    pub fn revoke_session(
        &mut self,
        session_id: &str,
        revoked_at: u64,
    ) -> Result<Option<u64>, String> {
        let Some(session) = self.state.sessions.get(session_id) else {
            return Ok(None);
        };
        if let Some(existing) = session.revoked_at {
            return Ok(Some(existing));
        }
        self.append(&Record::Revoke {
            session_id: session_id.to_owned(),
            revoked_at,
        })?;
        if let Some(session) = self.state.sessions.get_mut(session_id) {
            session.revoked_at = Some(revoked_at);
        }
        Ok(Some(revoked_at))
    }

    pub fn check_available(&self) -> Result<(), String> {
        if self.failed {
            return Err("journal write failed; restart required".to_owned());
        }
        let directory = fs::metadata(&self.directory).map_err(|error| error.to_string())?;
        let journal =
            fs::metadata(self.directory.join(JOURNAL_FILE)).map_err(|error| error.to_string())?;
        let open_journal = self.journal.metadata().map_err(|error| error.to_string())?;
        if (directory.dev(), directory.ino()) != self.directory_identity
            || (journal.dev(), journal.ino()) != (open_journal.dev(), open_journal.ino())
        {
            return Err("state directory or journal was replaced; restart required".to_owned());
        }
        Ok(())
    }

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

    fn append(&mut self, record: &Record) -> Result<(), String> {
        self.check_available()?;
        let mut line = serde_json::to_vec(record).map_err(|error| error.to_string())?;
        if line.len() >= MAX_RECORD_BYTES {
            return Err("journal record exceeds its bound".to_owned());
        }
        line.push(b'\n');
        if let Err(error) = self
            .journal
            .write_all(&line)
            .and_then(|()| self.journal.sync_all())
        {
            self.failed = true;
            return Err(format!("journal append: {error}"));
        }
        Ok(())
    }
}

fn apply(state: &mut State, record: Record) -> Result<(), String> {
    match record {
        Record::Principal(principal) => state.insert_principal(principal).map_err(|error| {
            if error == SUBJECT_TENANT_CONFLICT {
                "journal principal binds one subject to two tenants".to_owned()
            } else {
                error
            }
        }),
        Record::Session(session) => state
            .insert_session(session)
            .map_err(|_| "journal session references an unknown principal".to_owned()),
        Record::Revoke {
            session_id,
            revoked_at,
        } => {
            let session = state
                .sessions
                .get_mut(&session_id)
                .ok_or_else(|| "journal revocation references an unknown session".to_owned())?;
            session.revoked_at = Some(revoked_at);
            Ok(())
        }
    }
}

fn replay_journal(path: &Path, state: &mut State) -> Result<(), String> {
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

fn write_snapshot(directory: &Path, path: &Path, state: &SnapshotView<'_>) -> Result<(), String> {
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

    fn principal(tenant: &str, sub: &str) -> Principal {
        Principal {
            tenant: tenant.to_owned(),
            sub: sub.to_owned(),
            allowed_signer_public_keys: vec!["ab".repeat(32)],
            account: Some(format!("agent:{sub}:main")),
            audiences: vec!["ramp".to_owned()],
        }
    }

    fn session(id: &str, tenant: &str, sub: &str, expires_at: u64) -> StoredSession {
        StoredSession {
            session_id: id.to_owned(),
            tenant: tenant.to_owned(),
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
                .put_principal(principal("beta", "did:key:alpha"))
                .unwrap_or_else(|error| panic!("principal: {error}"));
            store
                .put_session(session("s1", "beta", "did:key:alpha", 100))
                .unwrap_or_else(|error| panic!("session: {error}"));
            store
                .put_session(session("s2", "beta", "did:key:alpha", 200))
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
                store.principal("beta", "did:key:alpha"),
                Some(&principal("beta", "did:key:alpha"))
            );
            assert_eq!(
                store.session("s1"),
                Some(&session("s1", "beta", "did:key:alpha", 100))
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
            Some(&session("s1", "beta", "did:key:alpha", 100))
        );
    }

    #[test]
    fn principals_are_keyed_by_tenant_and_subject() {
        let root = directory("tenant-key");
        let mut store = Store::open(&root).unwrap_or_else(|error| panic!("open: {error}"));
        store
            .put_principal(principal("alpha", "did:key:one"))
            .unwrap_or_else(|error| panic!("alpha principal: {error}"));
        store
            .put_principal(principal("bravo", "did:key:two"))
            .unwrap_or_else(|error| panic!("bravo principal: {error}"));
        assert_eq!(
            store.principal("alpha", "did:key:one"),
            Some(&principal("alpha", "did:key:one"))
        );
        assert_eq!(store.principal("bravo", "did:key:one"), None);
        assert_eq!(store.principal("alpha", "did:key:two"), None);
        assert_eq!(store.subject_tenant("did:key:one"), Some("alpha"));
        assert_eq!(store.subject_tenant("did:key:two"), Some("bravo"));
        assert_eq!(store.subject_tenant("did:key:none"), None);
        let mut replacement = principal("bravo", "did:key:one");
        replacement.allowed_signer_public_keys = vec!["cd".repeat(32)];
        assert_eq!(
            store.put_principal(replacement),
            Err(SUBJECT_TENANT_CONFLICT.to_owned())
        );
        assert_eq!(
            store.principal("alpha", "did:key:one"),
            Some(&principal("alpha", "did:key:one"))
        );
        assert_eq!(store.principal("bravo", "did:key:one"), None);
        let mut updated = principal("alpha", "did:key:one");
        updated.allowed_signer_public_keys = vec!["cd".repeat(32)];
        store
            .put_principal(updated.clone())
            .unwrap_or_else(|error| panic!("same tenant update: {error}"));
        assert_eq!(store.principal("alpha", "did:key:one"), Some(&updated));
        drop(store);
        let store = Store::open(&root).unwrap_or_else(|error| panic!("reopen: {error}"));
        assert_eq!(store.principal("alpha", "did:key:one"), Some(&updated));
        assert_eq!(store.principal("bravo", "did:key:one"), None);
        let snapshot = fs::read_to_string(root.join(SNAPSHOT_FILE)).unwrap_or_default();
        assert!(
            snapshot.contains("\"alpha\":{\"did:key:one\"")
                && snapshot.contains("\"tenant\":\"alpha\""),
            "the snapshot keys principals by tenant then subject: {snapshot}"
        );
    }

    #[test]
    fn sessions_are_scoped_to_the_tenant_of_their_principal() {
        let root = directory("tenant-session");
        let mut store = Store::open(&root).unwrap_or_else(|error| panic!("open: {error}"));
        store
            .put_principal(principal("alpha", "did:key:one"))
            .unwrap_or_else(|error| panic!("alpha principal: {error}"));
        store
            .put_principal(principal("bravo", "did:key:two"))
            .unwrap_or_else(|error| panic!("bravo principal: {error}"));
        assert!(store
            .put_session(session("s1", "bravo", "did:key:one", 100))
            .is_err());
        assert!(store
            .put_session(session("s1", "alpha", "did:key:two", 100))
            .is_err());
        store
            .put_session(session("s1", "alpha", "did:key:one", 100))
            .unwrap_or_else(|error| panic!("alpha session: {error}"));
        store
            .put_session(session("s2", "bravo", "did:key:two", 100))
            .unwrap_or_else(|error| panic!("bravo session: {error}"));
        assert_eq!(
            store.session("s1").map(|stored| stored.tenant.as_str()),
            Some("alpha")
        );
        assert_eq!(
            store.session("s2").map(|stored| stored.tenant.as_str()),
            Some("bravo")
        );
        drop(store);
        let store = Store::open(&root).unwrap_or_else(|error| panic!("reopen: {error}"));
        assert_eq!(
            store.session("s1"),
            Some(&session("s1", "alpha", "did:key:one", 100))
        );
        assert_eq!(
            store.session("s2"),
            Some(&session("s2", "bravo", "did:key:two", 100))
        );
    }

    #[test]
    fn cross_tenant_state_on_disk_is_refused() {
        let root = directory("tenant-disk");
        {
            let mut store = Store::open(&root).unwrap_or_else(|error| panic!("open: {error}"));
            store
                .put_principal(principal("alpha", "did:key:one"))
                .unwrap_or_else(|error| panic!("principal: {error}"));
        }
        let journal = root.join(JOURNAL_FILE);
        let alpha = fs::read(&journal).unwrap_or_else(|error| panic!("read journal: {error}"));
        let mut both = alpha.clone();
        both.extend_from_slice(
            &serde_json::to_vec(&Record::Principal(principal("bravo", "did:key:one")))
                .unwrap_or_default(),
        );
        both.push(b'\n');
        fs::write(&journal, &both).unwrap_or_else(|error| panic!("write journal: {error}"));
        assert!(
            Store::open(&root).is_err(),
            "a journal must not bind one subject to two tenants"
        );
        fs::write(&journal, &alpha).unwrap_or_else(|error| panic!("restore journal: {error}"));
        {
            let store = Store::open(&root).unwrap_or_else(|error| panic!("compaction: {error}"));
            assert_eq!(
                store.principal("alpha", "did:key:one"),
                Some(&principal("alpha", "did:key:one"))
            );
        }
        let mut bytes = serde_json::to_vec(&Record::Principal(principal("bravo", "did:key:one")))
            .unwrap_or_default();
        bytes.push(b'\n');
        fs::write(&journal, &bytes).unwrap_or_else(|error| panic!("write journal: {error}"));
        assert!(
            Store::open(&root).is_err(),
            "a journal must not claim a subject the snapshot bound to another tenant"
        );
        let mut bytes =
            serde_json::to_vec(&Record::Session(session("s3", "bravo", "did:key:one", 5)))
                .unwrap_or_default();
        bytes.push(b'\n');
        fs::write(&journal, &bytes).unwrap_or_else(|error| panic!("write journal: {error}"));
        assert!(
            Store::open(&root).is_err(),
            "a journal session must name a principal of its own tenant"
        );
        fs::write(&journal, b"").unwrap_or_else(|error| panic!("truncate journal: {error}"));
        let miskeyed = serde_json::json!({
            "principals": {"bravo": {"did:key:one": principal("alpha", "did:key:one")}},
            "sessions": {}
        });
        fs::write(root.join(SNAPSHOT_FILE), miskeyed.to_string())
            .unwrap_or_else(|error| panic!("write snapshot: {error}"));
        assert!(
            Store::open(&root).is_err(),
            "a snapshot principal must be keyed by its own tenant"
        );
        let duplicated = serde_json::json!({
            "principals": {
                "alpha": {"did:key:one": principal("alpha", "did:key:one")},
                "bravo": {"did:key:one": principal("bravo", "did:key:one")}
            },
            "sessions": {}
        });
        fs::write(root.join(SNAPSHOT_FILE), duplicated.to_string())
            .unwrap_or_else(|error| panic!("write snapshot: {error}"));
        assert!(
            Store::open(&root).is_err(),
            "a snapshot must not bind one subject to two tenants"
        );
        let untenanted = serde_json::json!({
            "principals": {"alpha": {"did:key:one": {
                "sub": "did:key:one",
                "allowed_signer_public_keys": ["ab".repeat(32)],
                "account": null,
                "audiences": []
            }}},
            "sessions": {}
        });
        fs::write(root.join(SNAPSHOT_FILE), untenanted.to_string())
            .unwrap_or_else(|error| panic!("write snapshot: {error}"));
        assert!(
            Store::open(&root).is_err(),
            "a principal without a tenant is not readable"
        );
    }

    #[test]
    fn torn_trailing_record_is_discarded_and_malformed_records_refuse() {
        let root = directory("torn");
        {
            let mut store = Store::open(&root).unwrap_or_else(|error| panic!("open: {error}"));
            store
                .put_principal(principal("beta", "did:key:beta"))
                .unwrap_or_else(|error| panic!("principal: {error}"));
        }
        let journal = root.join(JOURNAL_FILE);
        let complete =
            serde_json::to_vec(&Record::Session(session("s9", "beta", "did:key:beta", 5)))
                .unwrap_or_default();
        let mut bytes = fs::read(&journal).unwrap_or_else(|error| panic!("read journal: {error}"));
        bytes.extend_from_slice(&complete);
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
    fn replaced_journal_refuses_readiness_and_writes() {
        let root = directory("replaced");
        let mut store = Store::open(&root).unwrap_or_else(|error| panic!("open: {error}"));
        fs::remove_file(root.join(JOURNAL_FILE)).unwrap_or_else(|error| panic!("unlink: {error}"));
        fs::write(root.join(JOURNAL_FILE), b"").unwrap_or_else(|error| panic!("replace: {error}"));
        assert!(store.probe_writable().is_err());
        assert!(store
            .put_principal(principal("beta", "did:key:alpha"))
            .is_err());
        assert!(store.principal("beta", "did:key:alpha").is_none());
    }

    #[test]
    fn failed_journal_write_requires_restart() {
        let root = directory("write-failure");
        let mut store = Store::open(&root).unwrap_or_else(|error| panic!("open: {error}"));
        store.journal = File::open(root.join(JOURNAL_FILE))
            .unwrap_or_else(|error| panic!("read-only journal: {error}"));
        assert!(store
            .put_principal(principal("beta", "did:key:alpha"))
            .is_err());
        store.journal = OpenOptions::new()
            .append(true)
            .open(root.join(JOURNAL_FILE))
            .unwrap_or_else(|error| panic!("writable journal: {error}"));
        assert!(store.probe_writable().is_err());
        assert!(store
            .put_principal(principal("beta", "did:key:alpha"))
            .is_err());
        drop(store);
        let mut store = Store::open(&root).unwrap_or_else(|error| panic!("restart: {error}"));
        assert!(store
            .put_principal(principal("beta", "did:key:alpha"))
            .is_ok());
    }

    #[test]
    fn session_requires_a_known_principal_and_unique_identifier() {
        let root = directory("bounds");
        let mut store = Store::open(&root).unwrap_or_else(|error| panic!("open: {error}"));
        assert!(store
            .put_session(session("s1", "beta", "did:key:none", 1))
            .is_err());
        store
            .put_principal(principal("beta", "did:key:gamma"))
            .unwrap_or_else(|error| panic!("principal: {error}"));
        store
            .put_session(session("s1", "beta", "did:key:gamma", 1))
            .unwrap_or_else(|error| panic!("session: {error}"));
        assert!(store
            .put_session(session("s1", "beta", "did:key:gamma", 2))
            .is_err());
        store
            .probe_writable()
            .unwrap_or_else(|error| panic!("probe: {error}"));
        assert!(root.join(READY_MARKER_FILE).exists());
    }
}
