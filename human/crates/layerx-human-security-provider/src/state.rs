use crate::{number, text, Error, Result};
use base64::{engine::general_purpose::STANDARD, Engine as _};
use data_encoding::BASE32_NOPAD;
use hmac::{Hmac, Mac as _};
use layerx_human_service::store::PrincipalId;
use layerx_programs::ProtocolDeploymentVerifier;
use layerx_proof::merkle::decode_proof;
use serde::{Deserialize, Serialize};
use sha1::Sha1;
use sha2::{Digest as _, Sha256};
use std::collections::BTreeMap;
use std::fs::{self, File, OpenOptions};
use std::io::{Read as _, Write as _};
use std::os::unix::fs::{DirBuilderExt as _, MetadataExt as _, OpenOptionsExt as _};
use std::path::{Component, Path, PathBuf};
use subtle::ConstantTimeEq as _;
use zeroize::Zeroizing;

const LIMIT: u64 = 16 * 1024 * 1024;
const MAX_RECORDS: u64 = 4096;
const SETUP_LIFETIME: u64 = 300;
const REMASK: u64 = 60;

#[derive(Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct RecoveryReceipt {
    pub version: u8,
    pub principal: String,
    pub evidence_id: String,
    pub canonical_receipt: String,
    pub receipt_proof: String,
    pub header: String,
    pub header_signature: String,
}
impl RecoveryReceipt {
    fn verify(&self, trust: &Path) -> Result<()> {
        if self.version != 1 {
            return Err(Error::Refused);
        }
        PrincipalId::new(self.principal.clone()).map_err(|_| Error::Refused)?;
        text(self.evidence_id.as_bytes())?;
        text(self.canonical_receipt.as_bytes())?;
        let receipt = decode64(&self.canonical_receipt)?;
        let decoded = layerx_wire::receipt::decode(&receipt).map_err(|_| Error::Refused)?;
        if layerx_wire::receipt::encode(&decoded).map_err(|_| Error::Refused)? != receipt {
            return Err(Error::Refused);
        }
        let proof = decode_proof(&decode64(&self.receipt_proof)?).map_err(|_| Error::Refused)?;
        let signature: [u8; 64] = decode64(&self.header_signature)?
            .try_into()
            .map_err(|_| Error::Refused)?;
        let _ = protected_read(trust, LIMIT)?;
        ProtocolDeploymentVerifier::from_protected_history(trust, 1)
            .map_err(|_| Error::Refused)?
            .verify_historical_protocol_head(&receipt, &proof, &decode64(&self.header)?, &signature)
            .map_err(|_| Error::Refused)?;
        Ok(())
    }
}
fn decode64(value: &str) -> Result<Vec<u8>> {
    let bytes = STANDARD.decode(value).map_err(|_| Error::Refused)?;
    if STANDARD.encode(&bytes) != value {
        return Err(Error::Refused);
    }
    Ok(bytes)
}

#[derive(Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
struct Account {
    clock: u64,
    methods: BTreeMap<String, Method>,
    setup: Option<Setup>,
    backups: Vec<[u8; 32]>,
}
#[derive(Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
struct Method {
    label: String,
    secret: [u8; 20],
    enabled_at: u64,
    counter: u64,
}
#[derive(Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
struct Setup {
    id: String,
    label: String,
    secret: [u8; 20],
    issued_at: u64,
    expires_at: u64,
    attempts: u8,
}
#[derive(Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
struct State {
    accounts: BTreeMap<String, Account>,
    receipts: BTreeMap<String, BTreeMap<String, RecoveryReceipt>>,
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Record {
    sequence: u64,
    previous: [u8; 32],
    state: State,
}

pub struct Store {
    root: PathBuf,
    trust: PathBuf,
    lock: File,
    root_identity: (u64, u64),
    state: State,
    sequence: u64,
    digest: [u8; 32],
    healthy: bool,
}
impl Store {
    pub fn open(root: &Path, trust: &Path) -> Result<Self> {
        check_components(root)?;
        if !root.exists() {
            fs::DirBuilder::new().mode(0o700).create(root)?;
            File::open(root.parent().ok_or(Error::Configuration)?)?.sync_all()?;
        }
        let metadata = fs::symlink_metadata(root)?;
        if !metadata.is_dir()
            || metadata.mode() & 0o777 != 0o700
            || metadata.uid() != rustix::process::geteuid().as_raw()
        {
            return Err(Error::Configuration);
        }
        let _ = protected_read(trust, LIMIT)?;
        ProtocolDeploymentVerifier::from_protected_history(trust, 1)
            .map_err(|_| Error::Configuration)?;
        let lock_path = root.join("writer.lock");
        let lock = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .mode(0o600)
            .custom_flags(rustix::fs::OFlags::NOFOLLOW.bits() as i32)
            .open(&lock_path)?;
        check_file(&lock.metadata()?)?;
        rustix::fs::flock(&lock, rustix::fs::FlockOperation::NonBlockingLockExclusive)
            .map_err(|_| Error::Refused)?;
        let mut store = Self {
            root: root.into(),
            trust: trust.into(),
            lock,
            root_identity: (metadata.dev(), metadata.ino()),
            state: State::default(),
            sequence: 0,
            digest: [0; 32],
            healthy: true,
        };
        let entries = fs::read_dir(root)?
            .map(|entry| entry.map(|entry| entry.file_name()))
            .collect::<std::io::Result<Vec<_>>>()?;
        if entries.iter().all(|name| name == "writer.lock") {
            let record = Record {
                sequence: 0,
                previous: [0; 32],
                state: State::default(),
            };
            let bytes = serde_json::to_vec(&record).map_err(|_| Error::Corrupt)?;
            store.atomic("snapshot.json", &bytes)?;
            store.atomic("initialized", &Sha256::digest(&bytes))?;
            store.atomic("head", &head_bytes(0, Sha256::digest(&bytes).into()))?;
        }
        let (state, sequence, digest) = store.replay(true)?;
        store.state = state;
        store.sequence = sequence;
        store.digest = digest;
        Ok(store)
    }

    fn replay(&self, recover: bool) -> Result<(State, u64, [u8; 32])> {
        let metadata = fs::symlink_metadata(&self.root)?;
        if !metadata.is_dir()
            || metadata.file_type().is_symlink()
            || metadata.mode() & 0o777 != 0o700
            || metadata.uid() != rustix::process::geteuid().as_raw()
            || (metadata.dev(), metadata.ino()) != self.root_identity
        {
            return Err(Error::Corrupt);
        }
        let lock_metadata = fs::symlink_metadata(self.root.join("writer.lock"))?;
        check_file(&lock_metadata)?;
        let held = self.lock.metadata()?;
        if (held.dev(), held.ino()) != (lock_metadata.dev(), lock_metadata.ino()) {
            return Err(Error::Corrupt);
        }
        let marker = protected_read(&self.root.join("initialized"), 32)?;
        let initial = protected_read(&self.root.join("snapshot.json"), LIMIT)?;
        let mut digest: [u8; 32] = Sha256::digest(&initial).into();
        if marker.as_slice() != digest {
            return Err(Error::Corrupt);
        }
        let record = parse_record(&initial)?;
        if record.sequence != 0 || record.previous != [0; 32] || record.state != State::default() {
            return Err(Error::Corrupt);
        }
        let mut state = record.state;
        let mut journals = Vec::new();
        let mut pending = false;
        for entry in fs::read_dir(&self.root)? {
            let name = entry?
                .file_name()
                .into_string()
                .map_err(|_| Error::Corrupt)?;
            match name.as_str() {
                "writer.lock" | "initialized" | "snapshot.json" | "head" => {}
                "transaction.tmp" if recover => {
                    let path = self.root.join("transaction.tmp");
                    check_file(&fs::symlink_metadata(&path)?)?;
                    let _ = protected_read(&path, LIMIT)?;
                    pending = true;
                }
                _ if name.len() == 25 && name.ends_with(".json") => {
                    let sequence = name[..20].parse::<u64>().map_err(|_| Error::Corrupt)?;
                    if name != format!("{sequence:020}.json") {
                        return Err(Error::Corrupt);
                    }
                    journals.push(sequence);
                }
                _ => return Err(Error::Corrupt),
            }
        }
        journals.sort_unstable();
        if journals.len() as u64 > MAX_RECORDS {
            return Err(Error::Corrupt);
        }
        let head = protected_read(&self.root.join("head"), 40)?;
        if head.len() != 40 {
            return Err(Error::Corrupt);
        }
        let mut previous_head = head_bytes(0, digest);
        let mut sequence = 0;
        for next in journals {
            if next != sequence + 1 {
                return Err(Error::Corrupt);
            }
            let bytes = protected_read(&self.root.join(format!("{next:020}.json")), LIMIT)?;
            let record = parse_record(&bytes)?;
            if record.sequence != next || record.previous != digest {
                return Err(Error::Corrupt);
            }
            validate_state(&record.state, &self.trust)?;
            for (principal, old) in &state.accounts {
                if record
                    .state
                    .accounts
                    .get(principal)
                    .is_none_or(|new| new.clock < old.clock)
                {
                    return Err(Error::Corrupt);
                }
            }
            previous_head = head_bytes(sequence, digest);
            state = record.state;
            sequence = next;
            digest = Sha256::digest(&bytes).into();
        }
        let committed_head = head_bytes(sequence, digest);
        let repair_head = head.as_slice() != committed_head;
        if repair_head && (!recover || sequence == 0 || head.as_slice() != previous_head) {
            return Err(Error::Corrupt);
        }
        if pending {
            fs::remove_file(self.root.join("transaction.tmp"))?;
            File::open(&self.root)?.sync_all()?;
        }
        if repair_head {
            self.atomic("head", &committed_head)?;
        }
        Ok((state, sequence, digest))
    }
    fn consistent(&mut self) -> Result<()> {
        if !self.healthy {
            return Err(Error::Corrupt);
        }
        match self.replay(false) {
            Ok((state, sequence, digest))
                if sequence == self.sequence && digest == self.digest && state == self.state =>
            {
                Ok(())
            }
            _ => {
                self.healthy = false;
                Err(Error::Corrupt)
            }
        }
    }
    fn atomic(&self, name: &str, bytes: &[u8]) -> Result<()> {
        let temp = self.root.join("transaction.tmp");
        let mut file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .open(&temp)?;
        file.write_all(bytes)?;
        file.sync_all()?;
        if name == "head" && self.root.join(name).symlink_metadata().is_ok() {
            let _ = protected_read(&self.root.join(name), 40)?;
        } else if self.root.join(name).symlink_metadata().is_ok() {
            return Err(Error::Corrupt);
        }
        fs::rename(temp, self.root.join(name))?;
        File::open(&self.root)?.sync_all()?;
        Ok(())
    }
    fn commit(&mut self, next: State) -> Result<()> {
        if self.sequence >= MAX_RECORDS {
            return Err(Error::Refused);
        }
        validate_state(&next, &self.trust)?;
        let sequence = self.sequence + 1;
        let bytes = Zeroizing::new(
            serde_json::to_vec(&Record {
                sequence,
                previous: self.digest,
                state: next.clone(),
            })
            .map_err(|_| Error::Corrupt)?,
        );
        if bytes.len() as u64 > LIMIT {
            return Err(Error::Refused);
        }
        if let Err(error) = self.atomic(&format!("{sequence:020}.json"), &bytes) {
            self.healthy = false;
            return Err(error);
        }
        let digest = Sha256::digest(&bytes).into();
        if let Err(error) = self.atomic("head", &head_bytes(sequence, digest)) {
            self.healthy = false;
            return Err(error);
        }
        self.digest = digest;
        self.sequence = sequence;
        self.state = next;
        Ok(())
    }
    pub(crate) fn dispatch(&mut self, op: u8, fields: &[Vec<u8>]) -> Result<Vec<Vec<u8>>> {
        self.consistent()?;
        let counts = [0, 1, 3, 4, 3, 2, 3];
        if counts.get(usize::from(op)) != Some(&fields.len()) {
            return Err(Error::Refused);
        }
        if op == 0 {
            return Ok(Vec::new());
        }
        let principal =
            PrincipalId::new(text(&fields[0])?.to_owned()).map_err(|_| Error::Refused)?;
        let p = principal.as_str();
        if op == 1 {
            return Ok(status(self.state.accounts.get(p)));
        }
        let now = number(fields.last().ok_or(Error::Refused)?)?;
        let remask = now.checked_add(REMASK).ok_or(Error::Refused)?;
        if op == 6 {
            let id = text(&fields[1])?;
            let receipt = self
                .state
                .receipts
                .get(p)
                .and_then(|receipts| receipts.get(id))
                .ok_or(Error::Refused)?;
            receipt.verify(&self.trust)?;
            return Ok(vec![
                receipt.canonical_receipt.as_bytes().to_vec(),
                remask.to_be_bytes().to_vec(),
                vec![1],
            ]);
        }
        let mut next = self.state.clone();
        if next.accounts.len() >= 1024 && !next.accounts.contains_key(p) {
            return Err(Error::Refused);
        }
        let account = next.accounts.entry(p.to_owned()).or_default();
        if now < account.clock {
            return Err(Error::Refused);
        }
        account.clock = now;
        let result = match op {
            2 => {
                let label = text(&fields[1])?.to_owned();
                if label.len() > 256 || account.methods.len() >= 16 {
                    return Err(Error::Refused);
                }
                let mut secret = [0; 20];
                getrandom::fill(&mut secret).map_err(|_| Error::Refused)?;
                let setup = Setup {
                    id: random_id()?,
                    label,
                    secret,
                    issued_at: now,
                    expires_at: now.checked_add(SETUP_LIFETIME).ok_or(Error::Refused)?,
                    attempts: 0,
                };
                let encoded = BASE32_NOPAD.encode(&secret);
                let uri = format!("otpauth://totp/LayerX:{p}?secret={encoded}&issuer=LayerX&algorithm=SHA1&digits=6&period=30");
                let result = vec![
                    setup.id.as_bytes().to_vec(),
                    encoded.into_bytes(),
                    remask.to_be_bytes().to_vec(),
                    uri.into_bytes(),
                    remask.to_be_bytes().to_vec(),
                    setup.expires_at.to_be_bytes().to_vec(),
                ];
                account.setup = Some(setup);
                Ok(result)
            }
            3 => {
                let id = text(&fields[1])?;
                let code = text(&fields[2])?;
                let setup = account.setup.as_mut().ok_or(Error::Refused)?;
                if setup.id != id
                    || now > setup.expires_at
                    || now < setup.issued_at
                    || setup.attempts >= 5
                {
                    return Err(Error::Refused);
                }
                setup.attempts += 1;
                let counter = (now / 30).saturating_sub(1)..=(now / 30).saturating_add(1);
                let mut matched = None;
                for counter in counter {
                    if bool::from(
                        totp(&setup.secret, counter)?
                            .as_bytes()
                            .ct_eq(code.as_bytes()),
                    ) {
                        matched = Some(counter);
                    }
                }
                if let Some(counter) = matched {
                    let method = Method {
                        label: setup.label.clone(),
                        secret: setup.secret,
                        enabled_at: now,
                        counter,
                    };
                    let method_id = random_id()?;
                    let mut result = vec![
                        method_id.as_bytes().to_vec(),
                        method.label.as_bytes().to_vec(),
                        now.to_be_bytes().to_vec(),
                        Vec::new(),
                        remask.to_be_bytes().to_vec(),
                    ];
                    if account.methods.contains_key(&method_id) {
                        return Err(Error::Refused);
                    }
                    account.methods.insert(method_id, method);
                    account.setup = None;
                    result.extend(backups(account)?);
                    Ok(result)
                } else {
                    Err(Error::Refused)
                }
            }
            4 => {
                let id = text(&fields[1])?;
                if account.methods.remove(id).is_none() {
                    return Err(Error::Refused);
                }
                if account.methods.is_empty() {
                    account.backups.clear();
                }
                Ok(status(Some(account)))
            }
            5 => {
                if account.methods.is_empty() {
                    return Err(Error::Refused);
                }
                let mut result = vec![remask.to_be_bytes().to_vec()];
                result.extend(backups(account)?);
                Ok(result)
            }
            _ => Err(Error::Refused),
        };
        self.commit(next)?;
        result
    }
}
fn parse_record(bytes: &[u8]) -> Result<Record> {
    let record: Record = serde_json::from_slice(bytes).map_err(|_| Error::Corrupt)?;
    if serde_json::to_vec(&record).map_err(|_| Error::Corrupt)? != bytes {
        return Err(Error::Corrupt);
    }
    Ok(record)
}
fn validate_state(state: &State, trust: &Path) -> Result<()> {
    if state.accounts.len() > 1024 || state.receipts.len() > 1024 {
        return Err(Error::Corrupt);
    }
    for (p, account) in &state.accounts {
        PrincipalId::new(p.clone()).map_err(|_| Error::Corrupt)?;
        if account.methods.len() > 16
            || !matches!(account.backups.len(), 0 | 10)
            || (account.methods.is_empty() && !account.backups.is_empty())
        {
            return Err(Error::Corrupt);
        }
        for (id, method) in &account.methods {
            text(id.as_bytes())?;
            text(method.label.as_bytes())?;
            if method.label.len() > 256
                || method.enabled_at > account.clock
                || method.counter > account.clock / 30 + 1
            {
                return Err(Error::Corrupt);
            }
        }
        if let Some(setup) = &account.setup {
            text(setup.id.as_bytes())?;
            text(setup.label.as_bytes())?;
            if setup.label.len() > 256
                || setup.issued_at.checked_add(SETUP_LIFETIME) != Some(setup.expires_at)
                || setup.issued_at > account.clock
                || setup.attempts > 5
            {
                return Err(Error::Corrupt);
            }
        }
    }
    for (p, receipts) in &state.receipts {
        if receipts.len() > 64 {
            return Err(Error::Corrupt);
        }
        for (id, receipt) in receipts {
            if &receipt.principal != p || &receipt.evidence_id != id {
                return Err(Error::Corrupt);
            }
            receipt.verify(trust)?;
        }
    }
    Ok(())
}
fn status(account: Option<&Account>) -> Vec<Vec<u8>> {
    let mut fields = vec![
        (account.map_or(0, |a| a.backups.len()) as u32)
            .to_be_bytes()
            .to_vec(),
        (account.map_or(0, |a| a.methods.len()) as u32)
            .to_be_bytes()
            .to_vec(),
    ];
    if let Some(account) = account {
        for (id, method) in &account.methods {
            fields.extend([
                id.as_bytes().to_vec(),
                method.label.as_bytes().to_vec(),
                method.enabled_at.to_be_bytes().to_vec(),
                Vec::new(),
            ]);
        }
    }
    fields
}
fn random_id() -> Result<String> {
    let mut random = [0; 20];
    getrandom::fill(&mut random).map_err(|_| Error::Refused)?;
    Ok(BASE32_NOPAD.encode(&random).to_ascii_lowercase())
}
fn backups(account: &mut Account) -> Result<Vec<Vec<u8>>> {
    let mut codes = Vec::new();
    account.backups.clear();
    for _ in 0..10 {
        let code = random_id()?;
        let digest: [u8; 32] = Sha256::digest(code.as_bytes()).into();
        if account.backups.contains(&digest) {
            return Err(Error::Refused);
        }
        account.backups.push(digest);
        codes.push(code.into_bytes());
    }
    Ok(codes)
}
fn totp(secret: &[u8; 20], counter: u64) -> Result<String> {
    let mut mac = Hmac::<Sha1>::new_from_slice(secret).map_err(|_| Error::Refused)?;
    mac.update(&counter.to_be_bytes());
    let digest = mac.finalize().into_bytes();
    let offset = usize::from(digest[19] & 15);
    let value = (u32::from(digest[offset]) << 24
        | u32::from(digest[offset + 1]) << 16
        | u32::from(digest[offset + 2]) << 8
        | u32::from(digest[offset + 3]))
        & 0x7fff_ffff;
    Ok(format!("{:06}", value % 1_000_000))
}
pub fn ingest_recovery_receipt(root: &Path, trust: &Path, path: &Path) -> Result<()> {
    let bytes = protected_read(path, 32768)?;
    let receipt: RecoveryReceipt = serde_json::from_slice(&bytes).map_err(|_| Error::Refused)?;
    if serde_json::to_vec(&receipt).map_err(|_| Error::Refused)? != bytes.as_slice() {
        return Err(Error::Refused);
    }
    receipt.verify(trust)?;
    let mut store = Store::open(root, trust)?;
    store.consistent()?;
    let mut next = store.state.clone();
    let receipts = next.receipts.entry(receipt.principal.clone()).or_default();
    if let Some(old) = receipts.get(&receipt.evidence_id) {
        return if old == &receipt {
            Ok(())
        } else {
            Err(Error::Refused)
        };
    }
    receipts.insert(receipt.evidence_id.clone(), receipt);
    store.commit(next)
}
fn check_components(path: &Path) -> Result<()> {
    if !path.is_absolute() {
        return Err(Error::Configuration);
    }
    let mut current = PathBuf::new();
    for part in path.components() {
        if !matches!(part, Component::RootDir | Component::Normal(_)) {
            return Err(Error::Configuration);
        }
        current.push(part);
        match fs::symlink_metadata(&current) {
            Ok(metadata) if metadata.file_type().is_symlink() => return Err(Error::Configuration),
            Ok(_) => {}
            Err(error) if error.kind() == std::io::ErrorKind::NotFound && current == path => {}
            Err(error) => return Err(error.into()),
        }
    }
    Ok(())
}
fn check_file(metadata: &fs::Metadata) -> Result<()> {
    if !metadata.is_file()
        || metadata.mode() & 0o777 != 0o600
        || metadata.uid() != rustix::process::geteuid().as_raw()
        || metadata.nlink() != 1
    {
        return Err(Error::Configuration);
    }
    Ok(())
}
fn protected_read(path: &Path, limit: u64) -> Result<Zeroizing<Vec<u8>>> {
    check_components(path)?;
    let before = fs::symlink_metadata(path)?;
    check_file(&before)?;
    let file = OpenOptions::new()
        .read(true)
        .custom_flags(rustix::fs::OFlags::NOFOLLOW.bits() as i32)
        .open(path)?;
    let after = file.metadata()?;
    check_file(&after)?;
    if (before.dev(), before.ino()) != (after.dev(), after.ino()) || after.len() > limit {
        return Err(Error::Corrupt);
    }
    let mut bytes = Zeroizing::new(Vec::new());
    file.take(limit + 1).read_to_end(&mut bytes)?;
    if bytes.len() as u64 != after.len() {
        return Err(Error::Corrupt);
    }
    Ok(bytes)
}

fn head_bytes(sequence: u64, digest: [u8; 32]) -> Vec<u8> {
    let mut bytes = sequence.to_be_bytes().to_vec();
    bytes.extend_from_slice(&digest);
    bytes
}
