use crate::config::{protected, Config};
use crate::wire::{blob, Error, Request, Result};
use ring::aead::{Aad, LessSafeKey, Nonce, UnboundKey, AES_256_GCM};
use ring::signature::{Ed25519KeyPair, KeyPair};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::fs::{self, File, OpenOptions};
use std::io::Write;
use std::os::unix::fs::{MetadataExt, OpenOptionsExt, PermissionsExt};
use std::path::PathBuf;
use zeroize::{Zeroize, Zeroizing};

const MAX_STATE: usize = 8 * 1024 * 1024;
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Record {
    binding: [u8; 32],
    class: u8,
    handle: [u8; 32],
    public: [u8; 32],
    seed: Option<[u8; 32]>,
    previous: Option<[u8; 32]>,
    generation: u64,
    #[serde(default)]
    wallet: Option<crate::evm::Wallet>,
    #[serde(default)]
    send_actions: BTreeMap<String, (crate::evm_types::SendPlanAuthorization, Vec<u8>)>,
}
impl Drop for Record {
    fn drop(&mut self) {
        self.seed.zeroize();
    }
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct State {
    version: u8,
    provider: String,
    network: u32,
    records: BTreeMap<String, Record>,
}
pub(crate) struct Store {
    root: PathBuf,
    state: State,
    key: LessSafeKey,
    aad: Vec<u8>,
    healthy: bool,
    _lock: File,
}
pub(crate) fn hex(bytes: &[u8]) -> String {
    const DIGITS: &[u8; 16] = b"0123456789abcdef";
    bytes
        .iter()
        .flat_map(|b| {
            [
                char::from(DIGITS[usize::from(b >> 4)]),
                char::from(DIGITS[usize::from(b & 15)]),
            ]
        })
        .collect()
}
fn signing_key(seed: &[u8; 32]) -> Result<Ed25519KeyPair> {
    Ed25519KeyPair::from_seed_unchecked(seed).map_err(|_| Error::Integrity)
}
fn public(seed: &[u8; 32]) -> Result<[u8; 32]> {
    signing_key(seed)?
        .public_key()
        .as_ref()
        .try_into()
        .map_err(|_| Error::Integrity)
}
fn new_seed() -> Result<Zeroizing<[u8; 32]>> {
    let mut seed = Zeroizing::new([0; 32]);
    getrandom::fill(&mut *seed).map_err(|_| Error::Unavailable)?;
    Ok(seed)
}
fn description(record: &Record) -> Result<Vec<u8>> {
    let mut bytes = Vec::new();
    blob(&mut bytes, &record.handle)?;
    bytes.extend(record.public);
    bytes.extend(record.binding);
    bytes.push(0);
    Ok(bytes)
}
impl Store {
    pub fn open(config: &Config) -> std::result::Result<Self, String> {
        if !config.state.is_absolute() {
            return Err("state path must be absolute".into());
        }
        fs::create_dir_all(&config.state).map_err(|_| "state directory unavailable")?;
        let metadata =
            fs::symlink_metadata(&config.state).map_err(|_| "state metadata unavailable")?;
        if !metadata.is_dir()
            || metadata.file_type().is_symlink()
            || metadata.uid() != rustix::process::geteuid().as_raw()
        {
            return Err("state ownership refused".into());
        }
        fs::set_permissions(&config.state, fs::Permissions::from_mode(0o700))
            .map_err(|_| "state permissions unavailable")?;
        let lock_fd = rustix::fs::open(
            config.state.join("owner.lock"),
            rustix::fs::OFlags::CREATE
                | rustix::fs::OFlags::RDWR
                | rustix::fs::OFlags::NOFOLLOW
                | rustix::fs::OFlags::CLOEXEC,
            rustix::fs::Mode::from_bits_truncate(0o600),
        )
        .map_err(|_| "state lock unavailable")?;
        let lock = File::from(lock_fd);
        let metadata = lock
            .metadata()
            .map_err(|_| "state lock metadata unavailable")?;
        if !metadata.is_file()
            || metadata.uid() != rustix::process::geteuid().as_raw()
            || metadata.mode() & 0o077 != 0
        {
            return Err("state lock ownership or permissions refused".into());
        }
        lock.try_lock().map_err(|_| "state already owned")?;
        let key = LessSafeKey::new(
            UnboundKey::new(&AES_256_GCM, &config.seal).map_err(|_| "seal key invalid")?,
        );
        let mut aad = b"LXKP/state/v1\0".to_vec();
        aad.extend(config.provider.as_bytes());
        aad.extend(config.network.to_be_bytes());
        let path = config.state.join("state.aead");
        let state = if path.try_exists().map_err(|_| "state lookup failed")? {
            let mut encrypted = protected(&path, MAX_STATE + 28, true)?;
            if encrypted.len() < 28 {
                return Err("state truncated".into());
            }
            let nonce: [u8; 12] = encrypted[..12]
                .try_into()
                .map_err(|_| "state nonce invalid")?;
            let plain = key
                .open_in_place(
                    Nonce::assume_unique_for_key(nonce),
                    Aad::from(&aad),
                    &mut encrypted[12..],
                )
                .map_err(|_| "state authentication failed")?;
            serde_json::from_slice::<State>(plain).map_err(|_| "state encoding invalid")?
        } else {
            State {
                version: 1,
                provider: config.provider.clone(),
                network: config.network,
                records: BTreeMap::new(),
            }
        };
        validate(&state, config)?;
        let temp = config.state.join("state.next");
        if temp
            .try_exists()
            .map_err(|_| "state recovery lookup failed")?
        {
            let metadata =
                fs::symlink_metadata(&temp).map_err(|_| "state recovery metadata failed")?;
            if !metadata.is_file()
                || metadata.file_type().is_symlink()
                || metadata.uid() != rustix::process::geteuid().as_raw()
                || metadata.mode() & 0o077 != 0
            {
                return Err("state recovery file refused".into());
            }
            fs::remove_file(temp).map_err(|_| "state recovery failed")?;
        }
        let mut store = Self {
            root: config.state.clone(),
            state,
            key,
            aad,
            healthy: true,
            _lock: lock,
        };
        store
            .persist()
            .map_err(|_| "state durability unavailable")?;
        Ok(store)
    }
    fn persist(&mut self) -> Result<()> {
        let result = self.write_state();
        if result.is_err() {
            self.healthy = false;
        }
        result
    }
    fn write_state(&self) -> Result<()> {
        let mut plain =
            Zeroizing::new(serde_json::to_vec(&self.state).map_err(|_| Error::Integrity)?);
        if plain.len() > MAX_STATE {
            return Err(Error::Unavailable);
        }
        let mut nonce = [0; 12];
        getrandom::fill(&mut nonce).map_err(|_| Error::Unavailable)?;
        self.key
            .seal_in_place_append_tag(
                Nonce::assume_unique_for_key(nonce),
                Aad::from(&self.aad),
                &mut *plain,
            )
            .map_err(|_| Error::Unavailable)?;
        let temp = self.root.join("state.next");
        let mut file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .open(&temp)
            .map_err(|_| Error::Unavailable)?;
        file.write_all(&nonce)
            .and_then(|()| file.write_all(&plain))
            .and_then(|()| file.sync_all())
            .map_err(|_| Error::Unavailable)?;
        fs::rename(temp, self.root.join("state.aead")).map_err(|_| Error::Unavailable)?;
        File::open(&self.root)
            .and_then(|f| f.sync_all())
            .map_err(|_| Error::Unavailable)
    }
    pub fn dispatch(
        &mut self,
        request: &Request<'_>,
        signing_digest: Option<[u8; 32]>,
    ) -> Result<Vec<u8>> {
        if !self.healthy {
            return Err(Error::Unavailable);
        }
        if request.provider != self.state.provider
            || (request.operation != 0 && request.network != self.state.network)
        {
            return Err(Error::Refused);
        }
        if request.operation == 0 {
            self.persist()?;
            return Ok(Vec::new());
        }
        if request.operation == 1 {
            return self.create(request);
        }
        if request.operation == 3 && request.version == 1 {
            return Err(Error::Refused);
        }
        if request.operation == 11 {
            return self.authorize_send(request, signing_digest.ok_or(Error::Refused)?);
        }
        if request.operation >= 6 {
            return self.evm(request);
        }
        let key = hex(&request.binding);
        let record = self.state.records.get_mut(&key).ok_or(Error::NotFound)?;
        if record.class != request.class || request.reference != record.handle {
            return Err(Error::Integrity);
        }
        if request.operation == 4 && record.seed.is_none() {
            return Ok(Vec::new());
        }
        let seed = record.seed.as_ref().ok_or(Error::NotFound)?;
        match request.operation {
            2 => description(record),
            3 => self.rotate(&key, request.expected.ok_or(Error::Refused)?),
            4 => {
                record.seed.zeroize();
                record.seed = None;
                record.wallet = None;
                record.send_actions.clear();
                self.persist()?;
                Ok(Vec::new())
            }
            5 => Ok(signing_key(seed)?
                .sign(&signing_digest.ok_or(Error::Refused)?)
                .as_ref()
                .to_vec()),
            _ => Err(Error::Refused),
        }
    }
    fn authorize_send(&mut self, request: &Request<'_>, digest: [u8; 32]) -> Result<Vec<u8>> {
        let record = self
            .state
            .records
            .get_mut(&hex(&request.binding))
            .ok_or(Error::NotFound)?;
        if record.class != request.class || request.reference != record.handle {
            return Err(Error::Refused);
        }
        let seed = record.seed.as_ref().ok_or(Error::NotFound)?;
        let authorization: crate::evm_types::SendPlanAuthorization =
            serde_json::from_slice(request.evm).map_err(|_| Error::Refused)?;
        let key = hex(&authorization.action_key);
        if let Some((previous, signature)) = record.send_actions.get(&key) {
            return if previous == &authorization {
                Ok(signature.clone())
            } else {
                Err(Error::Conflict)
            };
        }
        if record.send_actions.len() >= 4096 {
            return Err(Error::Unavailable);
        }
        let signature = signing_key(seed)?.sign(&digest).as_ref().to_vec();
        record
            .send_actions
            .insert(key, (authorization, signature.clone()));
        self.persist()?;
        Ok(signature)
    }
    fn evm(&mut self, request: &Request<'_>) -> Result<Vec<u8>> {
        use crate::evm_types::{EvmAcknowledgement, EvmPlanAuthorization};
        let record = self
            .state
            .records
            .get_mut(&hex(&request.binding))
            .ok_or(Error::NotFound)?;
        if record.class != request.class
            || request.reference != record.handle
            || record.seed.is_none()
        {
            return Err(Error::Refused);
        }
        if request.operation == 6 {
            if record.wallet.is_none() {
                record.wallet = Some(crate::evm::Wallet::create()?);
            }
            let address = record.wallet.as_ref().ok_or(Error::Integrity)?.address;
            self.persist()?;
            return Ok(address.to_vec());
        }
        let wallet = record.wallet.as_mut().ok_or(Error::NotFound)?;
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_err(|_| Error::Unavailable)?
            .as_secs();
        let action = match request.operation {
            7 => {
                let authorization: EvmPlanAuthorization =
                    serde_json::from_slice(request.evm).map_err(|_| Error::Refused)?;
                if authorization.binding_digest != request.binding {
                    return Err(Error::Refused);
                }
                wallet.authorize(authorization, now)?
            }
            8 => {
                let key: [u8; 32] =
                    serde_json::from_slice(request.evm).map_err(|_| Error::Refused)?;
                wallet.sign(&key, now)?
            }
            9 => {
                let ack: EvmAcknowledgement =
                    serde_json::from_slice(request.evm).map_err(|_| Error::Refused)?;
                let action = wallet
                    .actions
                    .get_mut(&hex(&ack.action_key))
                    .ok_or(Error::NotFound)?;
                if action.transaction_hash != Some(ack.transaction_hash) {
                    return Err(Error::Integrity);
                }
                action.acknowledged = true;
                action.clone()
            }
            10 => {
                let key: [u8; 32] =
                    serde_json::from_slice(request.evm).map_err(|_| Error::Refused)?;
                wallet
                    .actions
                    .get(&hex(&key))
                    .ok_or(Error::NotFound)?
                    .clone()
            }
            12 => {
                let request: crate::evm_types::EvmExternalSignature =
                    serde_json::from_slice(request.evm).map_err(|_| Error::Refused)?;
                wallet.accept_external(&request.action_key, &request.signature, now)?
            }
            _ => return Err(Error::Refused),
        };
        let response = serde_json::to_vec(&action).map_err(|_| Error::Integrity)?;
        self.persist()?;
        Ok(response)
    }
    fn create(&mut self, request: &Request<'_>) -> Result<Vec<u8>> {
        let key = hex(&request.binding);
        if let Some(record) = self.state.records.get(&key) {
            if record.class != request.class {
                return Err(Error::Integrity);
            }
            if record.seed.is_none() || record.generation != 0 {
                return Err(Error::Conflict);
            }
            return description(record);
        }
        if self.state.records.len() >= 4096 {
            return Err(Error::Unavailable);
        }
        let seed = new_seed()?;
        let public = public(&seed)?;
        let mut handle = [0; 32];
        getrandom::fill(&mut handle).map_err(|_| Error::Unavailable)?;
        if handle == [0; 32] || self.state.records.values().any(|r| r.handle == handle) {
            return Err(Error::Unavailable);
        }
        let record = Record {
            binding: request.binding,
            class: request.class,
            handle,
            public,
            seed: Some(*seed),
            previous: None,
            generation: 0,
            wallet: None,
            send_actions: BTreeMap::new(),
        };
        let response = description(&record)?;
        self.state.records.insert(key, record);
        self.persist()?;
        Ok(response)
    }
    fn rotate(&mut self, key: &str, expected: [u8; 32]) -> Result<Vec<u8>> {
        let record = self.state.records.get_mut(key).ok_or(Error::NotFound)?;
        if record.previous == Some(expected) {
            return description(record);
        }
        if record.public != expected {
            return Err(Error::Conflict);
        }
        let seed = new_seed()?;
        let public = public(&seed)?;
        if public == record.public {
            return Err(Error::Unavailable);
        }
        let generation = record.generation.checked_add(1).ok_or(Error::Unavailable)?;
        record.previous = Some(record.public);
        record.public = public;
        record.seed.zeroize();
        record.seed = Some(*seed);
        record.generation = generation;
        let response = description(record)?;
        self.persist()?;
        Ok(response)
    }
}
fn validate(state: &State, config: &Config) -> std::result::Result<(), String> {
    if state.version != 1
        || state.provider != config.provider
        || state.network != config.network
        || state.records.len() > 4096
    {
        return Err("state policy mismatch".into());
    }
    let mut handles = std::collections::BTreeSet::new();
    for (key, record) in &state.records {
        if key != &hex(&record.binding)
            || record.binding == [0; 32]
            || !matches!(record.class, 1 | 2)
            || record.handle == [0; 32]
            || record.public == [0; 32]
            || !handles.insert(record.handle)
            || (record.generation == 0) != record.previous.is_none()
            || record.previous == Some(record.public)
        {
            return Err("state invariant failed".into());
        }
        if let Some(wallet) = &record.wallet {
            if record.seed.is_none() {
                return Err("destroyed wallet invariant failed".into());
            }
            wallet
                .validate(record.binding)
                .map_err(|_| "wallet state invariant failed")?;
        }
        if let Some(seed) = &record.seed {
            if public(seed).map_err(|_| "state key invalid")? != record.public {
                return Err("state key binding failed".into());
            }
        }
    }
    Ok(())
}
