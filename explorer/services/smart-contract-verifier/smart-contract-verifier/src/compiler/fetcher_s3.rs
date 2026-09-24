use super::{
    fetcher::{FetchError, Fetcher, FileValidator, Version},
    fetcher_versions::{VersionsFetcher, VersionsRefresher},
};
use async_trait::async_trait;
use bytes::Bytes;
use cron::Schedule;
use primitive_types::H256;
use s3::{request_trait::ResponseData, Bucket};
use std::{collections::HashSet, marker::PhantomData, path::PathBuf, str::FromStr, sync::Arc};
use thiserror::Error;
use tokio::task::JoinHandle;
use tracing::{debug, instrument};

#[derive(Error, Debug)]
enum ListError {
    #[error("listing s3 directory failed: {0}")]
    Fetch(s3::error::S3Error),
}

struct S3VersionFetcher<Ver> {
    bucket: Arc<Bucket>,
    _phantom_data: PhantomData<Ver>,
}

impl<Ver> S3VersionFetcher<Ver> {
    fn new(bucket: Arc<Bucket>) -> S3VersionFetcher<Ver> {
        S3VersionFetcher {
            bucket,
            _phantom_data: Default::default(),
        }
    }
}

#[async_trait]
impl<Ver: Version> VersionsFetcher for S3VersionFetcher<Ver> {
    type Versions = HashSet<Ver>;
    type Error = ListError;

    fn len(vers: &Self::Versions) -> usize {
        vers.len()
    }

    #[instrument(skip(self), level = "debug")]
    async fn fetch_versions(&self) -> Result<Self::Versions, Self::Error> {
        let folders = self
            .bucket
            .list("".to_string(), Some("/".to_string()))
            .await
            .map_err(ListError::Fetch)?;

        let fetched_versions: HashSet<Ver> = folders
            .into_iter()
            .filter_map(|x| x.common_prefixes)
            .flatten()
            .filter_map(|v| Ver::from_str(v.prefix.trim_end_matches('/')).ok())
            .collect();
        debug!(
            "found version on bucket of len = {}",
            fetched_versions.len()
        );
        Ok(fetched_versions)
    }
}

pub struct S3Fetcher<Ver: Version> {
    bucket: Arc<Bucket>,
    folder: PathBuf,
    versions: VersionsRefresher<HashSet<Ver>>,
    validator: Option<Arc<dyn FileValidator<Ver>>>,
}

fn spawn_fetch_s3(
    bucket: Arc<Bucket>,
    path: PathBuf,
) -> JoinHandle<Result<ResponseData, FetchError>> {
    tokio::spawn(async move {
        bucket
            .get_object(path.to_str().unwrap())
            .await
            .map_err(anyhow::Error::msg)
            .map_err(FetchError::Fetch)
    })
}

fn status_code_error(name: &str, status_code: u16) -> FetchError {
    FetchError::Fetch(anyhow::anyhow!(
        "s3 returned non 200 status code while fetching {}: {}",
        name,
        status_code
    ))
}

impl<Ver: Version> S3Fetcher<Ver> {
    pub async fn new(
        bucket: Arc<Bucket>,
        folder: PathBuf,
        refresh_schedule: Option<Schedule>,
        validator: Option<Arc<dyn FileValidator<Ver>>>,
    ) -> anyhow::Result<S3Fetcher<Ver>> {
        let fetcher = Arc::new(S3VersionFetcher::new(bucket.clone()));
        let versions = VersionsRefresher::new(fetcher, refresh_schedule).await?;
        Ok(S3Fetcher {
            bucket,
            folder,
            versions,
            validator,
        })
    }

    #[instrument(skip(self), level = "debug")]
    async fn fetch_file(&self, ver: &Ver) -> Result<(Bytes, H256), FetchError> {
        {
            let versions = self.versions.read();
            if !versions.contains(ver) {
                return Err(FetchError::NotFound(ver.clone().to_string()));
            }
        }

        let folder = PathBuf::from(ver.to_string());
        let data = spawn_fetch_s3(self.bucket.clone(), folder.join("solc"));
        let hash = spawn_fetch_s3(self.bucket.clone(), folder.join("sha256.hash"));
        let (data, hash) = futures::join!(data, hash);
        let (data, hash) = (data??, hash??);
        let (status_code, hash) = (hash.status_code(), hash.bytes());
        if status_code != 200 {
            return Err(status_code_error("hash data", status_code));
        }
        let (status_code, data) = (data.status_code(), data.bytes().to_vec());
        if status_code != 200 {
            return Err(status_code_error("executable file", status_code));
        }
        let hash = std::str::from_utf8(hash)
            .map_err(anyhow::Error::msg)
            .map_err(FetchError::HashParse)?;
        let hash = H256::from_str(hash)
            .map_err(anyhow::Error::msg)
            .map_err(FetchError::HashParse)?;
        Ok((data.into(), hash))
    }
}

#[async_trait]
impl<Ver: Version> Fetcher for S3Fetcher<Ver> {
    type Version = Ver;

    async fn fetch(&self, ver: &Self::Version) -> Result<PathBuf, FetchError> {
        let (data, hash) = self.fetch_file(ver).await?;
        super::fetcher::write_executable(data, hash, &self.folder, ver, self.validator.as_deref())
            .await
    }

    fn all_versions(&self) -> Vec<Self::Version> {
        let versions = self.versions.read();
        versions.iter().cloned().collect()
    }
}

#[cfg(test)]
mod tests {
    use super::{super::version_detailed as evm_version, *};
    use pretty_assertions::assert_eq;
    use s3::{creds::Credentials, Region};
    use serde::Serialize;
    use sha2::{Digest, Sha256};
    use std::time::Duration;
    use wiremock::{
        matchers::{method, path},
        Mock, MockServer, ResponseTemplate,
    };

    fn mock_get_object(p: &str, obj: &[u8]) -> Mock {
        Mock::given(method("GET"))
            .and(path(p))
            .respond_with(ResponseTemplate::new(200).set_body_bytes(obj))
    }

    #[derive(Serialize)]
    struct Prefix {
        #[serde(rename = "Prefix")]
        prefix: String,
    }

    #[derive(Serialize)]
    struct ListBucketResult {
        #[serde(rename = "Name")]
        name: String,
        #[serde(rename = "Prefix")]
        prefix: String,
        #[serde(rename = "IsTruncated")]
        is_truncated: bool,
        #[serde(rename = "CommonPrefixes", default)]
        common_prefixes: Vec<Prefix>,
    }

    fn mock_list_objects(p: &str, prefixes: impl Iterator<Item = String>) -> Mock {
        let value = ListBucketResult {
            name: p.into(),
            prefix: p.into(),
            is_truncated: false,
            common_prefixes: prefixes
                .map(|p| p + "/")
                .map(|prefix| Prefix { prefix })
                .collect(),
        };
        let data = quick_xml::se::to_string(&value).unwrap();
        Mock::given(method("GET"))
            .and(path(p))
            .respond_with(ResponseTemplate::new(200).set_body_string(data))
    }

    fn test_bucket(endpoint: String) -> Arc<Bucket> {
        let region = Region::Custom {
            region: "".into(),
            endpoint,
        };
        Arc::new(
            Bucket::new(
                "solc-releases",
                region,
                Credentials::new(Some(""), Some(""), None, None, None).unwrap(),
            )
            .unwrap()
            .with_path_style(), // for local testing
        )
    }

    #[tokio::test]
    async fn fetch_file() {
        let expected_file = "this is 100% a valid compiler trust me";
        let expected_hash = Sha256::digest(expected_file);

        let mock_server = MockServer::start().await;

        // Without "0x" prefix at checksum
        mock_get_object(
            "/solc-releases/v0.4.10%2Bcommit.f0d539ae/solc",
            expected_file.as_bytes(),
        )
        .mount(&mock_server)
        .await;

        mock_get_object(
            "/solc-releases/v0.4.10%2Bcommit.f0d539ae/sha256.hash",
            hex::encode(expected_hash).as_bytes(),
        )
        .mount(&mock_server)
        .await;

        // With "0x" prefix at checksum
        mock_get_object(
            "/solc-releases/v0.4.11%2Bcommit.68ef5810/solc",
            expected_file.as_bytes(),
        )
        .mount(&mock_server)
        .await;

        mock_get_object(
            "/solc-releases/v0.4.11%2Bcommit.68ef5810/sha256.hash",
            format!("0x{}", hex::encode(expected_hash)).as_bytes(),
        )
        .mount(&mock_server)
        .await;

        let versions = vec![
            evm_version::DetailedVersion::from_str("v0.4.10+commit.f0d539ae").unwrap(),
            evm_version::DetailedVersion::from_str("v0.4.11+commit.68ef5810").unwrap(),
        ];

        // create type directly to avoid extra work in constructor
        let fetcher = S3Fetcher {
            bucket: test_bucket(mock_server.uri()),
            folder: Default::default(),
            versions: VersionsRefresher::new_static(HashSet::from_iter(
                versions.clone().into_iter(),
            )),
            validator: None,
        };

        for version in versions {
            let (compiler, hash) = fetcher.fetch_file(&version).await.unwrap();
            assert_eq!(
                expected_file, compiler,
                "Invalid file for version: {}",
                version
            );
            assert_eq!(
                expected_hash.as_slice(),
                hash.as_ref(),
                "Invalid hash for version: {}",
                version
            );
        }
    }

    #[tokio::test]
    async fn list() {
        let expected_versions: Vec<_> = [
            "v0.4.10+commit.f0d539ae",
            "v0.8.13+commit.abaa5c0e",
            "v0.5.1+commit.c8a2cb62",
        ]
        .into_iter()
        .map(evm_version::DetailedVersion::from_str)
        .map(|x| x.unwrap())
        .collect();

        let mock_server = MockServer::start().await;
        mock_list_objects(
            "/solc-releases/",
            expected_versions.iter().map(|x| x.to_string()),
        )
        .mount(&mock_server)
        .await;

        let fetcher = S3VersionFetcher::new(test_bucket(mock_server.uri()));
        let versions = fetcher.fetch_versions().await.unwrap();
        let expected_versions = HashSet::from_iter(expected_versions.into_iter());
        assert_eq!(expected_versions, versions);
    }

    #[tokio::test]
    async fn refresh_list() {
        let all_versions: Vec<_> = [
            "v0.4.10+commit.f0d539ae",
            "v0.8.13+commit.abaa5c0e",
            "v0.5.1+commit.c8a2cb62",
        ]
        .into_iter()
        .map(evm_version::DetailedVersion::from_str)
        .map(|x| x.unwrap())
        .collect();

        let mock_server = MockServer::start().await;
        mock_list_objects("/solc-releases/", std::iter::empty())
            .mount(&mock_server)
            .await;

        let fetcher = S3Fetcher::new(
            test_bucket(mock_server.uri()),
            Default::default(),
            Some(Schedule::from_str("* * * * * * *").unwrap()),
            None,
        )
        .await
        .unwrap();

        {
            let versions = fetcher.versions.read();
            assert!(versions.is_empty());
        }

        {
            let expected_versions = &all_versions[0..2];
            mock_server.reset().await;
            mock_list_objects(
                "/solc-releases/",
                expected_versions.iter().map(|x| x.to_string()),
            )
            .mount(&mock_server)
            .await;

            tokio::time::sleep(Duration::from_secs(2)).await;

            let expected_versions = HashSet::from_iter(expected_versions.iter().cloned());
            let versions = fetcher.versions.read();
            assert_eq!(expected_versions, *versions);
        }

        {
            let expected_versions = &all_versions[1..3];
            mock_server.reset().await;
            mock_list_objects(
                "/solc-releases/",
                expected_versions
                    .iter()
                    .map(|x| x.to_string())
                    .chain(std::iter::once("some_garbage".into())),
            )
            .mount(&mock_server)
            .await;

            tokio::time::sleep(Duration::from_secs(2)).await;

            let expected_versions = HashSet::from_iter(expected_versions.iter().cloned());
            let versions = fetcher.versions.read();
            assert_eq!(expected_versions, *versions);
        }
    }
}
