use async_trait::async_trait;
use bytes::Bytes;
use mismatch::Mismatch;
use primitive_types::H256;
use sha2::{Digest, Sha256};
use std::{
    fmt::{Debug, Display},
    fs::{File, OpenOptions},
    hash::Hash,
    io::ErrorKind,
    os::unix::prelude::OpenOptionsExt,
    path::{Path, PathBuf},
    str::FromStr,
};
use thiserror::Error;
use tracing::instrument;

#[derive(Error, Debug)]
pub enum FetchError {
    #[error("version {0} not found")]
    NotFound(String),
    #[error("couldn't fetch the file: {0}")]
    Fetch(anyhow::Error),
    #[error("hashsum of fetched file mismatch: {0}")]
    HashMismatch(#[from] Mismatch<H256>),
    #[error("can't parse hashsum: {0}")]
    HashParse(anyhow::Error),
    #[error("couldn't create file: {0}")]
    File(#[from] std::io::Error),
    #[error("tokio sheduling error: {0}")]
    Schedule(#[from] tokio::task::JoinError),
    #[error("validation failed: {0}")]
    Validation(anyhow::Error),
}

#[async_trait]
pub trait FileValidator<Ver>: Send + Sync {
    async fn validate(&self, ver: &Ver, path: &Path) -> Result<(), anyhow::Error>;
}

#[async_trait]
pub trait Fetcher: Send + Sync {
    type Version;
    async fn fetch(&self, ver: &Self::Version) -> Result<PathBuf, FetchError>;
    fn all_versions(&self) -> Vec<Self::Version>;
}

pub trait Version:
    Clone + Debug + Display + FromStr + PartialEq + Eq + Hash + Send + Sync + 'static
{
    fn to_semver(&self) -> &semver::Version;
}

impl Version for semver::Version {
    fn to_semver(&self) -> &semver::Version {
        self
    }
}

#[cfg(target_family = "unix")]
fn create_executable(path: &Path) -> Result<File, std::io::Error> {
    OpenOptions::new()
        .create(true)
        .write(true)
        .truncate(true)
        .mode(0o777)
        .open(path)
}

#[instrument(skip(bytes), level = "debug")]
pub fn validate_checksum(bytes: &Bytes, expected: H256) -> Result<(), Mismatch<H256>> {
    let found = Sha256::digest(bytes);
    let found = H256::from_slice(&found);
    if expected != found {
        Err(Mismatch::new(expected, found))
    } else {
        Ok(())
    }
}

pub async fn write_executable<Ver: Version>(
    data: Bytes,
    sha: H256,
    path: &Path,
    ver: &Ver,
    validator: Option<&dyn FileValidator<Ver>>,
) -> Result<PathBuf, FetchError> {
    let folder = path.join(ver.to_string());
    let file = folder.join("solc");
    let mut file_tmp = file.clone();
    file_tmp.set_extension("tmp");

    let save_result = {
        let file_tmp = file_tmp.clone();
        let data = data.clone();
        let span = tracing::debug_span!("save executable");
        tokio::task::spawn_blocking(move || {
            let _guard = span.enter();
            std::fs::create_dir_all(&folder)?;
            std::fs::remove_file(file_tmp.as_path()).or_else(|e| {
                if e.kind() == ErrorKind::NotFound {
                    Ok(())
                } else {
                    Err(e)
                }
            })?;
            let mut file_tmp = create_executable(file_tmp.as_path())?;
            std::io::copy(&mut data.as_ref(), &mut file_tmp)
        })
    };
    let check_result = {
        let span = tracing::debug_span!("check hash result");
        tokio::task::spawn_blocking(move || {
            let _guard = span.enter();
            validate_checksum(&data, sha)
        })
    };

    let (check_result, save_result) = futures::join!(check_result, save_result);
    check_result??;
    save_result??;

    if let Some(validator) = validator {
        validator
            .validate(ver, file_tmp.as_path())
            .await
            .map_err(FetchError::Validation)?;
    }

    tokio::fs::rename(&file_tmp, &file).await?;

    Ok(file)
}

#[cfg(test)]
mod tests {
    use super::{super::version_detailed as evm_version, *};
    use std::str::FromStr;

    #[tokio::test]
    async fn write_text_executable() {
        let tmp_dir = tempfile::tempdir().unwrap();

        let data = "this is a compiler binary";
        let bytes = Bytes::from_static(data.as_bytes());
        let sha = Sha256::digest(data.as_bytes());

        let version = evm_version::DetailedVersion::from_str("v0.4.10+commit.f0d539ae").unwrap();
        let file = write_executable(
            bytes,
            H256::from_slice(&sha),
            tmp_dir.path(),
            &version,
            None,
        )
        .await
        .unwrap();

        let content = tokio::fs::read_to_string(file).await.unwrap();
        assert_eq!(data, content);
    }

    #[tokio::test]
    async fn wrong_file_checksum() {
        let tmp_dir = tempfile::tempdir().unwrap();

        let data = "this is a compiler binary";
        let bytes = Bytes::from_static(data.as_bytes());
        let sha = H256::default();
        let version = evm_version::DetailedVersion::from_str("v0.4.10+commit.f0d539ae").unwrap();

        let err = write_executable(bytes, sha, tmp_dir.path(), &version, None)
            .await
            .expect_err("expected to fail with wrong checksum");
        assert!(matches!(err, FetchError::HashMismatch(_)));

        let dir = tmp_dir.path().join(version.to_string());
        let file = dir.join("solc");
        let mut tmp_file = file.clone();
        tmp_file.set_extension("tmp");
        assert!(!file.exists());
        assert!(tmp_file.exists());
    }
}
