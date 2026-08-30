//! Durable encrypted protocol-session signer registry.

use crate::sign::ProvisionedSessionKey;
use layerx_crypto::keystore::{Keystore, KeystoreEntropy};
use layerx_crypto::session::IssuedSessionKey;
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::os::unix::fs::{MetadataExt, OpenOptionsExt, PermissionsExt};
use std::path::{Path, PathBuf};
use zeroize::Zeroizing;

pub struct SessionKeyRegistry {
    root: PathBuf,
    operator_secret: Zeroizing<Vec<u8>>,
    network_id: u32,
    owner_uid: u32,
}
#[derive(Debug)]
pub enum SessionKeyRegistryError {
    Io,
    Unprotected,
    Invalid,
    Exists,
    Crypto,
}

impl SessionKeyRegistry {
    pub fn open(
        root: PathBuf,
        operator_secret: Vec<u8>,
        network_id: u32,
        owner_uid: u32,
    ) -> Result<Self, SessionKeyRegistryError> {
        if !root.is_absolute() || operator_secret.len() < 32 || network_id == 0 {
            return Err(SessionKeyRegistryError::Invalid);
        }
        fs::create_dir_all(&root).map_err(|_| SessionKeyRegistryError::Io)?;
        let meta = fs::symlink_metadata(&root).map_err(|_| SessionKeyRegistryError::Io)?;
        if !meta.file_type().is_dir()
            || meta.file_type().is_symlink()
            || meta.uid() != owner_uid
            || meta.permissions().mode() & 0o077 != 0
        {
            return Err(SessionKeyRegistryError::Unprotected);
        }
        for entry in fs::read_dir(&root).map_err(|_| SessionKeyRegistryError::Io)? {
            let path = entry.map_err(|_| SessionKeyRegistryError::Io)?.path();
            if path.extension().is_some_and(|value| value == "tmp") {
                validate_file(&path, owner_uid)?;
                fs::remove_file(path).map_err(|_| SessionKeyRegistryError::Io)?;
            }
        }
        fs::File::open(&root)
            .and_then(|file| file.sync_all())
            .map_err(|_| SessionKeyRegistryError::Io)?;
        Ok(Self {
            root,
            operator_secret: Zeroizing::new(operator_secret),
            network_id,
            owner_uid,
        })
    }
    pub fn provision(
        &self,
        grant_id: [u8; 32],
        seed: &[u8; 32],
        issued: IssuedSessionKey,
    ) -> Result<(), SessionKeyRegistryError> {
        if self.revoked_path(grant_id).exists() {
            return Err(SessionKeyRegistryError::Invalid);
        }
        let identity = identity(grant_id, &issued);
        let mut salt = [0; 16];
        let mut nonce = [0; 24];
        getrandom::fill(&mut salt).map_err(|_| SessionKeyRegistryError::Crypto)?;
        getrandom::fill(&mut nonce).map_err(|_| SessionKeyRegistryError::Crypto)?;
        let envelope = Keystore::seal(
            seed,
            &self.operator_secret,
            &identity,
            self.network_id,
            KeystoreEntropy::new(salt, nonce).map_err(|_| SessionKeyRegistryError::Crypto)?,
        )
        .map_err(|_| SessionKeyRegistryError::Crypto)?;
        let bytes = envelope
            .to_bytes()
            .map_err(|_| SessionKeyRegistryError::Crypto)?;
        let path = self.path(grant_id);
        let temp = self
            .root
            .join(format!("{}.{}.tmp", hex(grant_id), std::process::id()));
        let mut file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .open(&temp)
            .map_err(|error| {
                if error.kind() == std::io::ErrorKind::AlreadyExists {
                    SessionKeyRegistryError::Exists
                } else {
                    SessionKeyRegistryError::Io
                }
            })?;
        if file.write_all(&bytes).is_err() || file.sync_all().is_err() {
            let _ = fs::remove_file(&temp);
            return Err(SessionKeyRegistryError::Io);
        }
        drop(file);
        let published = fs::hard_link(&temp, &path);
        let _ = fs::remove_file(&temp);
        match published {
            Ok(()) => {}
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {
                return Err(SessionKeyRegistryError::Exists)
            }
            Err(_) => return Err(SessionKeyRegistryError::Io),
        }
        fs::File::open(&self.root)
            .and_then(|file| file.sync_all())
            .map_err(|_| SessionKeyRegistryError::Io)?;
        Ok(())
    }
    pub fn revoke(&self, grant_id: [u8; 32]) -> Result<(), SessionKeyRegistryError> {
        let marker = self.revoked_path(grant_id);
        if marker.exists() {
            validate_revocation_marker(&marker, self.owner_uid)?;
            return Ok(());
        }
        let temp = self.root.join(format!(
            "{}.revoked.{}.tmp",
            hex(grant_id),
            std::process::id()
        ));
        let mut file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .open(&temp)
            .map_err(|error| {
                if error.kind() == std::io::ErrorKind::AlreadyExists {
                    SessionKeyRegistryError::Exists
                } else {
                    SessionKeyRegistryError::Io
                }
            })?;
        if file.write_all(b"LXSRV1").is_err() || file.sync_all().is_err() {
            let _ = fs::remove_file(&temp);
            return Err(SessionKeyRegistryError::Io);
        }
        drop(file);
        let published = fs::hard_link(&temp, &marker);
        let _ = fs::remove_file(&temp);
        match published {
            Ok(()) => {}
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {
                validate_revocation_marker(&marker, self.owner_uid)?
            }
            Err(_) => return Err(SessionKeyRegistryError::Io),
        }
        fs::File::open(&self.root)
            .and_then(|file| file.sync_all())
            .map_err(|_| SessionKeyRegistryError::Io)?;
        Ok(())
    }
    pub fn load(
        &self,
        grant_id: [u8; 32],
        issued: IssuedSessionKey,
    ) -> Result<ProvisionedSessionKey, SessionKeyRegistryError> {
        let marker = self.revoked_path(grant_id);
        if marker.exists() {
            return Err(SessionKeyRegistryError::Invalid);
        }
        let identity = identity(grant_id, &issued);
        let bytes = crate::config::read_protected_source(&self.path(grant_id), 8192)
            .map_err(|_| SessionKeyRegistryError::Unprotected)?;
        let envelope = Keystore::from_bytes(&bytes).map_err(|_| SessionKeyRegistryError::Crypto)?;
        envelope
            .open_with(&self.operator_secret, &identity, self.network_id, |seed| {
                ProvisionedSessionKey::from_seed(seed, issued)
            })
            .map_err(|_| SessionKeyRegistryError::Crypto)?
            .map(|key| key.bind_revocation_marker(marker, self.owner_uid))
            .map_err(|_| SessionKeyRegistryError::Crypto)
    }
    pub fn probe(&self) -> Result<(), SessionKeyRegistryError> {
        let meta = fs::symlink_metadata(&self.root).map_err(|_| SessionKeyRegistryError::Io)?;
        if !meta.file_type().is_dir()
            || meta.file_type().is_symlink()
            || meta.uid() != self.owner_uid
            || meta.permissions().mode() & 0o077 != 0
        {
            return Err(SessionKeyRegistryError::Unprotected);
        }
        for entry in fs::read_dir(&self.root).map_err(|_| SessionKeyRegistryError::Io)? {
            let path = entry.map_err(|_| SessionKeyRegistryError::Io)?.path();
            validate_file(&path, self.owner_uid)?;
            let bytes = crate::config::read_protected_source(&path, 8192)
                .map_err(|_| SessionKeyRegistryError::Unprotected)?;
            if path.extension().is_some_and(|value| value == "revoked") {
                if bytes != b"LXSRV1" {
                    return Err(SessionKeyRegistryError::Invalid);
                }
                continue;
            }
            let envelope =
                Keystore::from_bytes(&bytes).map_err(|_| SessionKeyRegistryError::Crypto)?;
            envelope
                .open_with(
                    &self.operator_secret,
                    envelope.identity(),
                    self.network_id,
                    |_| (),
                )
                .map_err(|_| SessionKeyRegistryError::Crypto)?;
        }
        Ok(())
    }
    fn path(&self, id: [u8; 32]) -> PathBuf {
        self.root.join(format!("{}.lxks", hex(id)))
    }
    fn revoked_path(&self, id: [u8; 32]) -> PathBuf {
        self.root.join(format!("{}.revoked", hex(id)))
    }
}
fn validate_file(path: &Path, owner_uid: u32) -> Result<(), SessionKeyRegistryError> {
    let meta = fs::symlink_metadata(path).map_err(|_| SessionKeyRegistryError::Io)?;
    if !meta.file_type().is_file()
        || meta.file_type().is_symlink()
        || meta.uid() != owner_uid
        || meta.permissions().mode() & 0o077 != 0
    {
        Err(SessionKeyRegistryError::Unprotected)
    } else {
        Ok(())
    }
}
fn validate_revocation_marker(path: &Path, owner_uid: u32) -> Result<(), SessionKeyRegistryError> {
    validate_file(path, owner_uid)?;
    let bytes = crate::config::read_protected_source(path, 16)
        .map_err(|_| SessionKeyRegistryError::Unprotected)?;
    if bytes == b"LXSRV1" {
        Ok(())
    } else {
        Err(SessionKeyRegistryError::Invalid)
    }
}
fn identity(grant: [u8; 32], issued: &IssuedSessionKey) -> Vec<u8> {
    let mut out = b"layerx-agentd/session-key/v1\0".to_vec();
    out.extend(grant);
    out.extend(issued.session_public_key);
    out.extend(issued.revocation_sequence.to_be_bytes());
    out.extend(issued.expires_at.to_be_bytes());
    out
}
fn hex(bytes: [u8; 32]) -> String {
    const H: &[u8; 16] = b"0123456789abcdef";
    bytes
        .iter()
        .flat_map(|b| [H[(b >> 4) as usize] as char, H[(b & 15) as usize] as char])
        .collect()
}
