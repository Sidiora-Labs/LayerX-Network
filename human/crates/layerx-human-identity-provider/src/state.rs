use std::collections::BTreeSet;
use std::fs::{self, File, OpenOptions};
use std::io::{self, Read, Write};
use std::os::unix::fs::{DirBuilderExt, MetadataExt, OpenOptionsExt};
use std::path::{Path, PathBuf};

use layerx_human_service::auth::{AccountIdentity, Device};
use layerx_human_service::onboarding::RecoveryPolicy;
use layerx_human_service::store::PrincipalId;
use layerx_types::ids::Did;
use layerx_types::intent::{ApprovalThreshold, RecoveryRoot};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::invalid;

const MAX_STATE_BYTES: u64 = 16 * 1024 * 1024;
const MAX_ACCOUNTS: usize = 10_000;
const MAX_BINDINGS: usize = 20_000;

/// An existing recovery authority's commitment; this service never invents one.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Policy {
    pub root: [u8; 32],
    pub threshold: u16,
    pub delay_seconds: u64,
}

impl Policy {
    /// Loads an owner-only regular JSON file containing a real recovery policy.
    ///
    /// # Errors
    /// Refuses unsafe files, malformed JSON and invalid protocol values.
    pub fn read(path: &Path) -> io::Result<Self> {
        let bytes = read_protected(path, 4096)?;
        let policy: Self = serde_json::from_slice(&bytes).map_err(|_| invalid("invalid policy"))?;
        policy.validate()?;
        Ok(policy)
    }

    fn validate(&self) -> io::Result<()> {
        let threshold = ApprovalThreshold::new(self.threshold)
            .map_err(|_| invalid("invalid recovery threshold"))?;
        RecoveryPolicy::new(RecoveryRoot::new(self.root), threshold, self.delay_seconds)
            .map_err(|_| invalid("invalid recovery policy"))?;
        Ok(())
    }
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Account {
    principal: String,
    did: Vec<u8>,
    email: String,
    display_name: String,
    idempotency_key: String,
    created_at: u64,
    policy: Policy,
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Binding {
    principal: String,
    assertion_id: String,
    device: Device,
}

#[derive(Clone, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Snapshot {
    accounts: Vec<Account>,
    bindings: Vec<Binding>,
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Envelope {
    version: u8,
    checksum: [u8; 32],
    snapshot: Snapshot,
}

/// Exclusive, replay-validated durable account and assertion directory.
pub struct State {
    root: PathBuf,
    lock: File,
    durable_digest: [u8; 32],
    poisoned: bool,
    directory: File,
    snapshot: Snapshot,
    policy: Policy,
}

impl State {
    /// Acquires exclusive ownership, replays the last atomic snapshot and cleans
    /// an uncommitted temporary file only after the committed state validates.
    ///
    /// # Errors
    /// Refuses corrupt state, unsafe permissions, competing writers or bad policy.
    pub fn open(root: &Path, policy: Policy) -> io::Result<Self> {
        policy.validate()?;
        if !root.is_absolute() {
            return Err(invalid("state root must be absolute"));
        }
        if !root.try_exists()? {
            let parent = root
                .parent()
                .ok_or_else(|| invalid("state parent missing"))?;
            check_directory(parent, false)?;
            fs::DirBuilder::new().mode(0o700).create(root)?;
            File::open(parent)?.sync_all()?;
        }
        check_directory(root, true)?;
        let directory = File::open(root)?;
        let lock = open_protected(&root.join("writer.lock"), true)?;
        lock.try_lock()
            .map_err(|_| invalid("state already locked"))?;
        let established = match read_protected(&root.join("initialized"), 16) {
            Ok(bytes) if bytes == b"LXIP-state-v1" => true,
            Ok(_) => return Err(invalid("invalid initialization marker")),
            Err(error) if error.kind() == io::ErrorKind::NotFound => false,
            Err(error) => return Err(error),
        };
        let snapshot_path = root.join("state.json");
        let (snapshot, initialized, durable_digest) =
            match read_protected(&snapshot_path, MAX_STATE_BYTES) {
                Ok(bytes) => {
                    let envelope: Envelope = serde_json::from_slice(&bytes)
                        .map_err(|_| invalid("corrupt state envelope"))?;
                    let encoded = serde_json::to_vec(&envelope.snapshot)?;
                    let checksum: [u8; 32] = Sha256::digest(&encoded).into();
                    if envelope.version != 1 || checksum != envelope.checksum {
                        return Err(invalid("state checksum or version mismatch"));
                    }
                    validate_snapshot(&envelope.snapshot)?;
                    (envelope.snapshot, true, Sha256::digest(&bytes).into())
                }
                Err(error) if error.kind() == io::ErrorKind::NotFound && !established => {
                    (Snapshot::default(), false, [0; 32])
                }
                Err(error) => return Err(error),
            };
        let mut state = Self {
            root: root.to_owned(),
            lock,
            durable_digest,
            poisoned: false,
            directory,
            snapshot,
            policy,
        };
        let pending = state.root.join("state.pending");
        match fs::symlink_metadata(&pending) {
            Ok(_) => {
                open_protected(&pending, false)?;
                fs::remove_file(pending)?;
                state.directory.sync_all()?;
            }
            Err(error) if error.kind() == io::ErrorKind::NotFound => {}
            Err(error) => return Err(error),
        }
        if !initialized {
            state.commit(Snapshot::default())?;
        }
        if !established {
            let mut marker = OpenOptions::new()
                .write(true)
                .create_new(true)
                .mode(0o600)
                .open(root.join("initialized"))?;
            marker.write_all(b"LXIP-state-v1")?;
            marker.sync_all()?;
            state.directory.sync_all()?;
        }
        Ok(state)
    }

    /// Imports metadata from a trusted enrollment authority while the server is
    /// stopped. Assertion IDs are globally unique and cannot be rebound.
    ///
    /// # Errors
    /// Refuses unknown principals, conflicts, invalid metadata or persistence errors.
    pub fn bind_device(
        &mut self,
        principal: &PrincipalId,
        assertion_id: &str,
        device: Device,
    ) -> io::Result<()> {
        self.ready()?;
        validate_text(assertion_id, 4096)?;
        Device::new(device.device_id(), device.label(), device.platform())
            .map_err(|_| invalid("invalid device"))?;
        if !self
            .snapshot
            .accounts
            .iter()
            .any(|item| item.principal == principal.as_str())
        {
            return Err(invalid("unknown principal"));
        }
        if let Some(binding) = self
            .snapshot
            .bindings
            .iter()
            .find(|item| item.assertion_id == assertion_id)
        {
            return if binding.principal == principal.as_str() && binding.device == device {
                Ok(())
            } else {
                Err(invalid("assertion binding conflict"))
            };
        }
        if self.snapshot.bindings.len() >= MAX_BINDINGS {
            return Err(invalid("binding capacity exhausted"));
        }
        let mut next = self.snapshot.clone();
        next.bindings.push(Binding {
            principal: principal.as_str().to_owned(),
            assertion_id: assertion_id.to_owned(),
            device,
        });
        self.commit(next)
    }

    pub(crate) fn provision(&mut self, fields: &[Vec<u8>]) -> io::Result<Vec<Vec<u8>>> {
        let email = text(&fields[0])?;
        let display_name = text(&fields[1])?;
        let key = text(&fields[2])?;
        let now = u64::from_be_bytes(
            fields[3]
                .as_slice()
                .try_into()
                .map_err(|_| invalid("invalid time"))?,
        );
        validate_account_input(email, display_name, key)?;
        if let Some(account) = self
            .snapshot
            .accounts
            .iter()
            .find(|item| item.idempotency_key == key)
        {
            if account.email != email || account.display_name != display_name {
                return Err(invalid("idempotency conflict"));
            }
            return Ok(account_fields(account));
        }
        if self
            .snapshot
            .accounts
            .iter()
            .any(|item| item.email == email)
        {
            return Err(invalid("email already bound"));
        }
        if self.snapshot.accounts.len() >= MAX_ACCOUNTS {
            return Err(invalid("account capacity exhausted"));
        }
        let mut entropy = [0u8; 32];
        getrandom::fill(&mut entropy).map_err(|_| io::Error::other("entropy unavailable"))?;
        let identifier = format!("act_{}", hex(&entropy));
        let principal = PrincipalId::new(identifier).map_err(|_| invalid("invalid principal"))?;
        if self
            .snapshot
            .accounts
            .iter()
            .any(|item| item.principal == principal.as_str())
        {
            return Err(io::Error::other("principal collision"));
        }
        let did = Did::new(format!("did:layerx:{}", principal.as_str()).as_bytes())
            .map_err(|_| invalid("invalid DID"))?;
        let account = Account {
            principal: principal.as_str().to_owned(),
            did: did.as_bytes().to_vec(),
            email: email.to_owned(),
            display_name: display_name.to_owned(),
            idempotency_key: key.to_owned(),
            created_at: now,
            policy: self.policy.clone(),
        };
        let response = account_fields(&account);
        let mut next = self.snapshot.clone();
        next.accounts.push(account);
        self.commit(next)?;
        Ok(response)
    }

    pub(crate) fn resolve(&self, fields: &[Vec<u8>]) -> io::Result<Vec<Vec<u8>>> {
        let email = text(&fields[0])?;
        self.snapshot
            .accounts
            .iter()
            .find(|item| item.email == email)
            .map(|item| vec![item.principal.as_bytes().to_vec()])
            .ok_or_else(|| invalid("unknown email"))
    }

    pub(crate) fn device(&self, fields: &[Vec<u8>]) -> io::Result<Vec<Vec<u8>>> {
        let principal =
            PrincipalId::new(text(&fields[0])?).map_err(|_| invalid("invalid principal"))?;
        let assertion = text(&fields[1])?;
        let binding = self
            .snapshot
            .bindings
            .iter()
            .find(|item| item.principal == principal.as_str() && item.assertion_id == assertion)
            .ok_or_else(|| invalid("unknown assertion binding"))?;
        Ok(vec![
            binding.device.device_id().as_bytes().to_vec(),
            binding.device.label().as_bytes().to_vec(),
            binding.device.platform().as_bytes().to_vec(),
        ])
    }

    pub(crate) fn ready(&self) -> io::Result<()> {
        if self.poisoned {
            return Err(io::Error::other("durability uncertain"));
        }
        check_directory(&self.root, true)?;
        for (path, opened) in [
            (&self.root, &self.directory),
            (&self.root.join("writer.lock"), &self.lock),
        ] {
            let actual = fs::symlink_metadata(path)?;
            let held = opened.metadata()?;
            if (actual.dev(), actual.ino()) != (held.dev(), held.ino()) {
                return Err(io::Error::other("state ownership changed"));
            }
        }
        let bytes = read_protected(&self.root.join("state.json"), MAX_STATE_BYTES)?;
        let digest: [u8; 32] = Sha256::digest(bytes).into();
        if digest != self.durable_digest {
            return Err(io::Error::other("committed state changed"));
        }
        Ok(())
    }

    fn commit(&mut self, next: Snapshot) -> io::Result<()> {
        let checksum: [u8; 32] = Sha256::digest(serde_json::to_vec(&next)?).into();
        let bytes = serde_json::to_vec(&Envelope {
            version: 1,
            checksum,
            snapshot: next.clone(),
        })?;
        if bytes.len() as u64 > MAX_STATE_BYTES {
            return Err(invalid("state capacity exhausted"));
        }
        self.poisoned = true;
        let pending = self.root.join("state.pending");
        let mut file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .custom_flags(
                i32::try_from(rustix::fs::OFlags::NOFOLLOW.bits())
                    .map_err(|_| invalid("unsupported open flags"))?,
            )
            .open(&pending)?;
        file.write_all(&bytes)?;
        file.sync_all()?;
        fs::rename(&pending, self.root.join("state.json"))?;
        self.directory.sync_all()?;
        self.durable_digest = Sha256::digest(&bytes).into();
        self.snapshot = next;
        self.poisoned = false;
        Ok(())
    }
}

fn validate_snapshot(snapshot: &Snapshot) -> io::Result<()> {
    if snapshot.accounts.len() > MAX_ACCOUNTS || snapshot.bindings.len() > MAX_BINDINGS {
        return Err(invalid("state capacity exceeded"));
    }
    let mut principals = BTreeSet::new();
    let mut emails = BTreeSet::new();
    let mut keys = BTreeSet::new();
    let mut dids = BTreeSet::new();
    for account in &snapshot.accounts {
        PrincipalId::new(account.principal.clone()).map_err(|_| invalid("corrupt principal"))?;
        Did::new(&account.did).map_err(|_| invalid("corrupt DID"))?;
        validate_account_input(
            &account.email,
            &account.display_name,
            &account.idempotency_key,
        )?;
        account.policy.validate()?;
        if !principals.insert(account.principal.as_str())
            || !emails.insert(account.email.as_str())
            || !keys.insert(account.idempotency_key.as_str())
            || !dids.insert(&account.did)
        {
            return Err(invalid("duplicate account binding"));
        }
    }
    let mut assertions = BTreeSet::new();
    for binding in &snapshot.bindings {
        validate_text(&binding.assertion_id, 4096)?;
        Device::new(
            binding.device.device_id(),
            binding.device.label(),
            binding.device.platform(),
        )
        .map_err(|_| invalid("corrupt device"))?;
        if !principals.contains(binding.principal.as_str())
            || !assertions.insert(&binding.assertion_id)
        {
            return Err(invalid("orphaned or duplicate assertion binding"));
        }
    }
    Ok(())
}

fn account_fields(account: &Account) -> Vec<Vec<u8>> {
    vec![
        account.principal.as_bytes().to_vec(),
        account.did.clone(),
        account.policy.root.to_vec(),
        account.policy.threshold.to_be_bytes().to_vec(),
        account.policy.delay_seconds.to_be_bytes().to_vec(),
    ]
}

fn validate_account_input(email: &str, display_name: &str, key: &str) -> io::Result<()> {
    validate_text(email, 256)?;
    validate_text(display_name, 256)?;
    validate_text(key, 4096)?;
    let (local, domain) = email
        .split_once('@')
        .ok_or_else(|| invalid("invalid email"))?;
    if !email.is_ascii()
        || email
            .bytes()
            .any(|b| b.is_ascii_uppercase() || b.is_ascii_whitespace())
        || local.is_empty()
        || domain.contains('@')
        || !domain.contains('.')
        || domain.starts_with('.')
        || domain.ends_with('.')
    {
        return Err(invalid("noncanonical email"));
    }
    AccountIdentity::new(email, display_name).map_err(|_| invalid("invalid identity"))?;
    Ok(())
}

fn validate_text(value: &str, maximum: usize) -> io::Result<()> {
    if value.is_empty() || value.len() > maximum || value.chars().any(char::is_control) {
        return Err(invalid("invalid text"));
    }
    Ok(())
}

fn text(bytes: &[u8]) -> io::Result<&str> {
    let value = std::str::from_utf8(bytes).map_err(|_| invalid("invalid UTF-8"))?;
    validate_text(value, 4096)?;
    Ok(value)
}

fn hex(bytes: &[u8]) -> String {
    const DIGITS: &[u8; 16] = b"0123456789abcdef";
    bytes
        .iter()
        .flat_map(|byte| {
            [
                char::from(DIGITS[usize::from(byte >> 4)]),
                char::from(DIGITS[usize::from(byte & 15)]),
            ]
        })
        .collect()
}

pub(crate) fn check_directory(path: &Path, private: bool) -> io::Result<()> {
    if !path.is_absolute() || fs::canonicalize(path)? != path {
        return Err(invalid("noncanonical directory"));
    }
    let metadata = fs::symlink_metadata(path)?;
    let mask = if private { 0o077 } else { 0o022 };
    if !metadata.is_dir()
        || metadata.uid() != rustix::process::geteuid().as_raw()
        || metadata.mode() & mask != 0
    {
        return Err(invalid("unprotected directory"));
    }
    for parent in path.ancestors().skip(1) {
        let ancestor = fs::symlink_metadata(parent)?;
        let trusted_owner =
            ancestor.uid() == 0 || ancestor.uid() == rustix::process::geteuid().as_raw();
        let sticky = ancestor.mode() & 0o1000 != 0;
        if !trusted_owner || (ancestor.mode() & 0o022 != 0 && !sticky) {
            return Err(invalid("unprotected directory ancestor"));
        }
    }
    Ok(())
}

fn open_protected(path: &Path, create: bool) -> io::Result<File> {
    let file = OpenOptions::new()
        .read(true)
        .write(create)
        .create(create)
        .mode(0o600)
        .custom_flags(
            i32::try_from((rustix::fs::OFlags::NOFOLLOW | rustix::fs::OFlags::NONBLOCK).bits())
                .map_err(|_| invalid("unsupported open flags"))?,
        )
        .open(path)?;
    let metadata = file.metadata()?;
    if !metadata.is_file()
        || metadata.uid() != rustix::process::geteuid().as_raw()
        || metadata.mode() & 0o777 != 0o600
        || metadata.nlink() != 1
    {
        return Err(invalid("unprotected state file"));
    }
    Ok(file)
}

fn read_protected(path: &Path, maximum: u64) -> io::Result<Vec<u8>> {
    let parent = path
        .parent()
        .ok_or_else(|| invalid("file parent missing"))?;
    check_directory(parent, false)?;
    let file = open_protected(path, false)?;
    if file.metadata()?.len() > maximum {
        return Err(invalid("file too large"));
    }
    let mut bytes = Vec::new();
    file.take(maximum + 1).read_to_end(&mut bytes)?;
    if bytes.len() as u64 > maximum {
        return Err(invalid("file grew beyond bound"));
    }
    Ok(bytes)
}
