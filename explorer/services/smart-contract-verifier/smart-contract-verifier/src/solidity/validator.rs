use crate::compiler::{FileValidator, Version};
use anyhow::{Context, Error};
use async_trait::async_trait;
use foundry_compilers::solc::Solc;
use std::path::Path;

#[derive(Default, Copy, Clone)]
pub struct SolcValidator {}

#[async_trait]
impl<Ver: Version> FileValidator<Ver> for SolcValidator {
    async fn validate(&self, ver: &Ver, path: &Path) -> Result<(), Error> {
        let solc = Solc::new(path).context("could not get compiler version")?;
        let solc_ver = solc.version;
        // ignore build and pre metadata
        let solc_ver = semver::Version::new(solc_ver.major, solc_ver.minor, solc_ver.patch);

        if &solc_ver != ver.to_semver() {
            Err(anyhow::anyhow!(
                "versions don't match: expected={}, got={}",
                ver.to_semver(),
                solc_ver
            ))
        } else {
            Ok(())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        compiler::{DetailedVersion, Fetcher, ListFetcher},
        consts::DEFAULT_SOLIDITY_COMPILER_LIST,
    };
    use std::{
        fs::OpenOptions, io::Write, os::unix::prelude::OpenOptionsExt, path::PathBuf, str::FromStr,
    };
    use tokio::sync::OnceCell;

    fn default_version() -> DetailedVersion {
        DetailedVersion::from_str("v0.8.9+commit.e5eed63a").unwrap()
    }

    async fn fetch_compiler() -> PathBuf {
        static COMPILERS: OnceCell<PathBuf> = OnceCell::const_new();
        COMPILERS
            .get_or_init(|| async {
                let tmp_dir = tempfile::tempdir().unwrap();
                let url = DEFAULT_SOLIDITY_COMPILER_LIST.try_into().unwrap();
                let fetcher = ListFetcher::new(url, tmp_dir.into_path(), None, None)
                    .await
                    .expect("Fetch releases");
                fetcher.fetch(&default_version()).await.unwrap()
            })
            .await
            .clone()
    }

    #[tokio::test]
    async fn success() {
        let compiler = fetch_compiler().await;
        let validator = SolcValidator::default();
        validator
            .validate(&default_version(), compiler.as_path())
            .await
            .unwrap();
    }

    #[tokio::test]
    async fn wrong_version() {
        let compiler = fetch_compiler().await;
        let validator = SolcValidator::default();
        let other_ver = DetailedVersion::from_str("v0.8.10+commit.e5eed63a").unwrap();
        validator
            .validate(&other_ver, compiler.as_path())
            .await
            .expect_err("expected version mismatch");
    }

    #[cfg(target_family = "unix")]
    #[tokio::test]
    async fn corrupted_binary() {
        let tmp_dir = tempfile::tempdir().unwrap();
        let compiler = tmp_dir.path().join("wrong_solc");
        let mut file = OpenOptions::new()
            .create(true)
            .write(true)
            .truncate(true)
            .mode(0o777)
            .open(compiler.clone())
            .unwrap();
        file.write_all(b"This isn't a compiler").unwrap();

        let validator = SolcValidator::default();
        validator
            .validate(&default_version(), compiler.as_path())
            .await
            .expect_err("expected failing to execute file");
    }
}
