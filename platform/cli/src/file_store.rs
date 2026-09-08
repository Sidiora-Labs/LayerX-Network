use std::collections::BTreeMap;
use std::fs::{self, File, OpenOptions};
use std::io::{Read as _, Write as _};
use std::num::NonZeroU32;
use std::path::{Path, PathBuf};

use keyring_core::Error;
use ring::{aead, pbkdf2};
use zeroize::Zeroizing;

const HEADER: &[u8; 8] = b"LXVAULT1";
const LIMIT: u64 = 1024 * 1024;
type Secrets = BTreeMap<String, Zeroizing<String>>;

pub struct Entry {
    directory: PathBuf,
    identity: String,
    passphrase: Zeroizing<String>,
}

fn failure(message: impl Into<String>) -> Error {
    Error::PlatformFailure(Box::new(std::io::Error::other(message.into())))
}

fn io_error(error: impl std::fmt::Display) -> Error {
    failure(error.to_string())
}

impl Entry {
    pub fn from_environment(identity: String) -> Result<Self, Error> {
        let config = crate::config::path().map_err(failure)?;
        let parent = config
            .parent()
            .ok_or_else(|| failure("config has no parent"))?;
        let passphrase = Zeroizing::new(
            std::env::var("LAYERX_CREDENTIAL_PASSPHRASE")
                .map_err(|_| failure("LAYERX_CREDENTIAL_PASSPHRASE is required"))?,
        );
        if !(12..=16384).contains(&passphrase.len()) {
            return Err(failure("credential passphrase must contain 12-16384 bytes"));
        }
        Ok(Self {
            directory: parent.join("credentials"),
            identity,
            passphrase,
        })
    }

    fn lock(&self) -> Result<File, Error> {
        #[cfg(not(unix))]
        return Err(failure(
            "file credential storage requires Unix owner-only permissions",
        ));
        #[cfg(unix)]
        {
            use std::os::unix::fs::{DirBuilderExt as _, OpenOptionsExt as _};
            if let Some(parent) = self.directory.parent() {
                fs::create_dir_all(parent).map_err(io_error)?;
            }
            match fs::DirBuilder::new().mode(0o700).create(&self.directory) {
                Ok(()) => (),
                Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => (),
                Err(error) => return Err(io_error(error)),
            }
            check_metadata(&self.directory, true)?;
            let lock = OpenOptions::new()
                .read(true)
                .write(true)
                .create(true)
                .truncate(false)
                .mode(0o600)
                .custom_flags(
                    i32::try_from(
                        (rustix::fs::OFlags::NOFOLLOW | rustix::fs::OFlags::NONBLOCK).bits(),
                    )
                    .map_err(io_error)?,
                )
                .open(self.directory.join("vault.lock"))
                .map_err(io_error)?;
            check_file(&lock)?;
            lock.lock().map_err(io_error)?;
            Ok(lock)
        }
    }

    fn read(&self) -> Result<Secrets, Error> {
        let path = self.directory.join("vault");
        let mut options = OpenOptions::new();
        options.read(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt as _;
            options.custom_flags(
                i32::try_from((rustix::fs::OFlags::NOFOLLOW | rustix::fs::OFlags::NONBLOCK).bits())
                    .map_err(io_error)?,
            );
        }
        let file = match options.open(path) {
            Ok(file) => file,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                return Ok(BTreeMap::new())
            }
            Err(error) => return Err(io_error(error)),
        };
        check_file(&file)?;
        let mut bytes = Zeroizing::new(Vec::new());
        file.take(LIMIT + 1)
            .read_to_end(&mut bytes)
            .map_err(io_error)?;
        if bytes.len() as u64 > LIMIT || bytes.len() < 52 || &bytes[..8] != HEADER {
            return Err(failure("invalid encrypted credential vault"));
        }
        let salt: [u8; 16] = bytes[8..24].try_into().map_err(io_error)?;
        let nonce: [u8; 12] = bytes[24..36].try_into().map_err(io_error)?;
        let key = self.key(&salt)?;
        let plaintext = key
            .open_in_place(
                aead::Nonce::assume_unique_for_key(nonce),
                aead::Aad::from(HEADER),
                &mut bytes[36..],
            )
            .map_err(|_| {
                failure("credential vault authentication failed: wrong passphrase or damaged vault")
            })?;
        let decoded: BTreeMap<String, String> =
            serde_json::from_slice(plaintext).map_err(io_error)?;
        Ok(decoded
            .into_iter()
            .map(|(name, value)| (name, Zeroizing::new(value)))
            .collect())
    }

    fn key(&self, salt: &[u8; 16]) -> Result<aead::LessSafeKey, Error> {
        let mut key = Zeroizing::new([0_u8; 32]);
        let iterations =
            NonZeroU32::new(600_000).ok_or_else(|| failure("invalid KDF iterations"))?;
        pbkdf2::derive(
            pbkdf2::PBKDF2_HMAC_SHA256,
            iterations,
            salt,
            self.passphrase.as_bytes(),
            key.as_mut(),
        );
        aead::UnboundKey::new(&aead::AES_256_GCM, key.as_ref())
            .map(aead::LessSafeKey::new)
            .map_err(io_error)
    }

    fn write(&self, secrets: &Secrets) -> Result<(), Error> {
        let plain: BTreeMap<&str, &str> = secrets
            .iter()
            .map(|(name, value)| (name.as_str(), value.as_str()))
            .collect();
        let mut bytes = Zeroizing::new(serde_json::to_vec(&plain).map_err(io_error)?);
        if bytes.len() as u64 + 52 > LIMIT {
            return Err(failure("credential vault exceeds size limit"));
        }
        let mut salt = [0_u8; 16];
        let mut nonce = [0_u8; 12];
        getrandom::fill(&mut salt).map_err(io_error)?;
        getrandom::fill(&mut nonce).map_err(io_error)?;
        self.key(&salt)?
            .seal_in_place_append_tag(
                aead::Nonce::assume_unique_for_key(nonce),
                aead::Aad::from(HEADER),
                &mut *bytes,
            )
            .map_err(io_error)?;
        let mut temporary = tempfile::NamedTempFile::new_in(&self.directory).map_err(io_error)?;
        check_file(temporary.as_file())?;
        temporary
            .write_all(HEADER)
            .and_then(|()| temporary.write_all(&salt))
            .and_then(|()| temporary.write_all(&nonce))
            .and_then(|()| temporary.write_all(&bytes))
            .and_then(|()| temporary.as_file().sync_all())
            .map_err(io_error)?;
        temporary
            .persist(self.directory.join("vault"))
            .map_err(io_error)?;
        File::open(&self.directory)
            .and_then(|file| file.sync_all())
            .map_err(io_error)
    }

    pub fn get_password(&self) -> Result<String, Error> {
        let _lock = self.lock()?;
        self.read()?
            .get(&self.identity)
            .map(|secret| secret.to_string())
            .ok_or(Error::NoEntry)
    }

    pub fn set_password(&self, value: &str) -> Result<(), Error> {
        let _lock = self.lock()?;
        let mut secrets = self.read()?;
        secrets.insert(self.identity.clone(), Zeroizing::new(value.to_owned()));
        self.write(&secrets)
    }

    pub fn delete_credential(&self) -> Result<(), Error> {
        let _lock = self.lock()?;
        let mut secrets = self.read()?;
        secrets.remove(&self.identity).ok_or(Error::NoEntry)?;
        self.write(&secrets)
    }
}

fn check_metadata(path: &Path, directory: bool) -> Result<(), Error> {
    let metadata = fs::symlink_metadata(path).map_err(io_error)?;
    if metadata.file_type().is_symlink() || metadata.is_dir() != directory {
        return Err(failure("credential path has an unsafe file type"));
    }
    check_permissions(&metadata)
}

fn check_file(file: &File) -> Result<(), Error> {
    let metadata = file.metadata().map_err(io_error)?;
    if !metadata.is_file() {
        return Err(failure("credential path must be a regular file"));
    }
    check_permissions(&metadata)
}

fn check_permissions(metadata: &fs::Metadata) -> Result<(), Error> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt as _;
        if metadata.mode() & 0o077 != 0 || metadata.uid() != rustix::process::geteuid().as_raw() {
            return Err(failure("credential files require owner-only permissions (0600 files, 0700 directory) and current-user ownership"));
        }
        Ok(())
    }
    #[cfg(not(unix))]
    Err(failure(
        "file credential storage requires Unix owner-only permissions",
    ))
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;
    use std::os::unix::fs::PermissionsExt as _;

    #[test]
    fn encrypted_entry_lifecycle_and_refusals() -> Result<(), Box<dyn std::error::Error>> {
        let temp = tempfile::tempdir()?;
        let mut entry = Entry {
            directory: temp.path().join("credentials"),
            identity: "key:test".into(),
            passphrase: Zeroizing::new("a long test-only passphrase".into()),
        };
        assert!(matches!(entry.get_password(), Err(Error::NoEntry)));
        entry.set_password("first credential secret")?;
        assert_eq!(entry.get_password()?, "first credential secret");
        let path = entry.directory.join("vault");
        let first = fs::read(&path)?;
        assert!(!first
            .windows(23)
            .any(|bytes| bytes == b"first credential secret"));
        assert_eq!(fs::metadata(&path)?.permissions().mode() & 0o777, 0o600);
        fs::set_permissions(&entry.directory, fs::Permissions::from_mode(0o755))?;
        assert!(entry.get_password().is_err());
        fs::set_permissions(&entry.directory, fs::Permissions::from_mode(0o700))?;
        let lock_path = entry.directory.join("vault.lock");
        fs::set_permissions(&lock_path, fs::Permissions::from_mode(0o644))?;
        assert!(entry.set_password("must not use public lock").is_err());
        fs::set_permissions(&lock_path, fs::Permissions::from_mode(0o600))?;
        entry.set_password("rotated credential secret")?;
        assert_eq!(entry.get_password()?, "rotated credential secret");
        assert_ne!(first, fs::read(&path)?);
        entry.passphrase = Zeroizing::new("a wrong test-only passphrase".into());
        assert!(entry.get_password().is_err());
        assert!(entry.set_password("must not overwrite").is_err());
        assert!(entry.delete_credential().is_err());
        entry.passphrase = Zeroizing::new("a long test-only passphrase".into());
        fs::set_permissions(&path, fs::Permissions::from_mode(0o640))?;
        assert!(entry.get_password().is_err());
        assert!(entry.set_password("must not overwrite").is_err());
        assert!(entry.delete_credential().is_err());
        fs::set_permissions(&path, fs::Permissions::from_mode(0o600))?;
        entry.delete_credential()?;
        assert!(matches!(entry.get_password(), Err(Error::NoEntry)));
        let mut damaged = fs::read(&path)?;
        damaged[40] ^= 1;
        fs::write(&path, damaged)?;
        assert!(entry.get_password().is_err());
        fs::remove_file(&path)?;
        std::os::unix::fs::symlink(temp.path().join("absent"), &path)?;
        assert!(entry.set_password("must not follow symlink").is_err());
        Ok(())
    }
}
