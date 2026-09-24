use super::fetcher::{FetchError, Fetcher, Version};
use crate::metrics;
use std::{collections::HashMap, path::PathBuf, sync::Arc};
use tracing::Instrument;

pub struct DownloadCache<T> {
    cache: parking_lot::Mutex<HashMap<T, Arc<tokio::sync::RwLock<Option<PathBuf>>>>>,
}

impl<T> Default for DownloadCache<T> {
    fn default() -> Self {
        Self {
            cache: parking_lot::Mutex::new(HashMap::new()),
        }
    }
}

impl<Ver: Version> DownloadCache<Ver> {
    async fn try_get(&self, ver: &Ver) -> Option<PathBuf> {
        let entry = {
            let cache = self.cache.lock();
            cache.get(ver).cloned()
        };
        match entry {
            Some(lock) => {
                let file = lock.read().await;
                file.as_ref().cloned()
            }
            None => None,
        }
    }
}

impl<Ver: Version> DownloadCache<Ver> {
    pub async fn get<D: Fetcher<Version = Ver> + ?Sized>(
        &self,
        fetcher: &D,
        ver: &Ver,
    ) -> Result<PathBuf, FetchError> {
        metrics::DOWNLOAD_CACHE_TOTAL.inc();
        match self.try_get(ver).await {
            Some(file) => {
                metrics::DOWNLOAD_CACHE_HITS.inc();
                Ok(file)
            }
            None => {
                let _timer = metrics::COMPILER_FETCH_TIME.start_timer();
                let span = tracing::debug_span!("fetch compiler", ver = ver.to_string());
                self.fetch(fetcher, ver).instrument(span).await
            }
        }
    }

    async fn fetch<D: Fetcher<Version = Ver> + ?Sized>(
        &self,
        fetcher: &D,
        ver: &Ver,
    ) -> Result<PathBuf, FetchError> {
        let lock = {
            let mut cache = self.cache.lock();
            Arc::clone(cache.entry(ver.clone()).or_default())
        };
        let mut entry = lock.write().await;
        match entry.as_ref() {
            Some(file) => Ok(file.clone()),
            None => {
                tracing::info!(target: "compiler_cache", "installing file version {}", ver);
                let file = fetcher.fetch(ver).await?;
                *entry = Some(file.clone());
                Ok(file)
            }
        }
    }

    pub async fn load_from_dir(&self, dir: &PathBuf) -> std::io::Result<()> {
        let paths = read_dir_paths(dir)?;
        let versions = filter_versions(paths);
        self.add_versions(versions).await;
        Ok(())
    }

    async fn add_versions(&self, versions: HashMap<Ver, PathBuf>) {
        for (version, path) in versions {
            let solc_path = path.join("solc");
            if solc_path.exists() {
                tracing::info!("found local compiler version {}", version);
                let lock = {
                    let mut cache = self.cache.lock();
                    Arc::clone(cache.entry(version.clone()).or_default())
                };
                *lock.write().await = Some(solc_path);
            } else {
                tracing::warn!(
                    "found verions {} but file {:?} doesn't exists",
                    version,
                    solc_path
                );
            }
        }
    }
}

fn read_dir_paths(dir: &PathBuf) -> std::io::Result<impl Iterator<Item = PathBuf>> {
    let paths = std::fs::read_dir(dir)?.filter_map(|r| r.ok().map(|e| e.path()));
    Ok(paths)
}

fn filter_versions<Ver: Version>(dirs: impl Iterator<Item = PathBuf>) -> HashMap<Ver, PathBuf> {
    dirs.filter_map(|path| {
        path.file_name()
            .and_then(|n| n.to_str())
            .map(String::from)
            .and_then(|n| Ver::from_str(&n).ok())
            .map(|v| (v, path))
    })
    .collect()
}

#[cfg(test)]
mod tests {
    use super::{
        super::{fetcher_list::ListFetcher, version_detailed as evm_version},
        *,
    };
    use crate::consts::DEFAULT_SOLIDITY_COMPILER_LIST;
    use async_trait::async_trait;
    use futures::{executor::block_on, join, pin_mut};
    use pretty_assertions::assert_eq;
    use std::{collections::HashSet, env::temp_dir, str::FromStr, time::Duration};
    use tokio::{spawn, task::yield_now, time::timeout};

    fn new_version(major: u64) -> evm_version::DetailedVersion {
        evm_version::DetailedVersion::Release(evm_version::ReleaseVersion {
            version: semver::Version::new(major, 0, 0),
            commit: "00010203".to_string(),
        })
    }

    /// Tests, that caching works, meaning that cache downloads each version only once
    #[test]
    fn value_is_cached() {
        #[derive(Default)]
        struct MockFetcher {
            counter: parking_lot::Mutex<HashMap<evm_version::DetailedVersion, u32>>,
        }

        #[async_trait]
        impl Fetcher for MockFetcher {
            type Version = evm_version::DetailedVersion;

            async fn fetch(&self, ver: &Self::Version) -> Result<PathBuf, FetchError> {
                *self.counter.lock().entry(ver.clone()).or_default() += 1;
                Ok(PathBuf::from(ver.to_string()))
            }

            fn all_versions(&self) -> Vec<Self::Version> {
                vec![]
            }
        }

        let fetcher = MockFetcher::default();
        let cache = DownloadCache::default();

        let vers: Vec<_> = (0..3).map(new_version).collect();

        let get_and_check = |ver: &evm_version::DetailedVersion| {
            let value = block_on(cache.get(&fetcher, ver)).unwrap();
            assert_eq!(value, PathBuf::from(ver.to_string()));
        };

        get_and_check(&vers[0]);
        get_and_check(&vers[1]);
        get_and_check(&vers[0]);
        get_and_check(&vers[0]);
        get_and_check(&vers[1]);
        get_and_check(&vers[1]);
        get_and_check(&vers[2]);
        get_and_check(&vers[2]);
        get_and_check(&vers[1]);
        get_and_check(&vers[0]);

        let counter = fetcher.counter.lock();
        assert_eq!(counter.len(), 3);
        assert!(counter.values().all(|&count| count == 1));
    }

    /// Tests, that cache will not block requests for already downloaded values,
    /// while it downloads others
    #[tokio::test]
    async fn downloading_not_blocks() {
        const TIMEOUT: Duration = Duration::from_secs(10);

        #[derive(Clone)]
        struct MockBlockingFetcher {
            sync: Arc<tokio::sync::Mutex<()>>,
        }

        #[async_trait]
        impl Fetcher for MockBlockingFetcher {
            type Version = evm_version::DetailedVersion;

            async fn fetch(&self, ver: &Self::Version) -> Result<PathBuf, FetchError> {
                let _guard = self.sync.lock().await;
                Ok(PathBuf::from(ver.to_string()))
            }

            fn all_versions(&self) -> Vec<Self::Version> {
                vec![]
            }
        }

        let sync = Arc::<tokio::sync::Mutex<()>>::default();
        let fetcher = MockBlockingFetcher { sync: sync.clone() };
        let cache = Arc::new(DownloadCache::default());

        let vers: Vec<_> = (0..3).map(new_version).collect();

        // fill the cache
        cache.get(&fetcher, &vers[1]).await.unwrap();

        // lock the fetcher
        let guard = sync.lock().await;

        // try to download (it will block on mutex)
        let handle = {
            let cache = cache.clone();
            let vers = vers.clone();
            let fetcher = fetcher.clone();
            spawn(
                async move { join!(cache.get(&fetcher, &vers[0]), cache.get(&fetcher, &vers[2])) },
            )
        };
        // so we could rerun future after timeout
        pin_mut!(handle);
        // give the thread to the scheduler so it could run "handle" task
        yield_now().await;

        // check, that while we're downloading we don't block the cache
        timeout(TIMEOUT, cache.get(&fetcher, &vers[1]))
            .await
            .expect("should not block")
            .expect("expected value not error");

        // check, that we're blocked on downloading
        timeout(Duration::from_millis(100), &mut handle)
            .await
            .expect_err("should block");

        // release the lock
        std::mem::drop(guard);

        // now we can finish downloading
        let vals = timeout(TIMEOUT, handle)
            .await
            .expect("should not block")
            .unwrap();
        vals.0.expect("expected value got error");
        vals.1.expect("expected value got error");
    }

    #[tokio::test]
    async fn filter_versions() {
        let versions: HashSet<evm_version::DetailedVersion> =
            vec![1, 2, 3, 4, 5].into_iter().map(new_version).collect();

        let paths = versions.iter().map(|v| v.to_string().into()).chain(vec![
            "some_random_dir".into(),
            ".".into(),
            "..".into(),
            "�0.7.0+commit.9e61f92b".into(),
        ]);

        let versions_map = super::filter_versions(paths);
        let filtered_versions = HashSet::from_iter(versions_map.into_keys());
        assert_eq!(versions, filtered_versions,);
    }

    #[tokio::test]
    async fn load_downloaded_compiler() {
        let ver = evm_version::DetailedVersion::from_str("0.7.0+commit.9e61f92b").unwrap();
        let dir = temp_dir();

        let url = DEFAULT_SOLIDITY_COMPILER_LIST
            .try_into()
            .expect("Getting url");
        let fetcher = ListFetcher::new(url, temp_dir(), None, None)
            .await
            .expect("Fetch releases");
        fetcher.fetch(&ver).await.expect("download should complete");

        let cache = DownloadCache::default();
        cache
            .load_from_dir(&dir)
            .await
            .expect("cannot load compilers");

        let path = cache
            .try_get(&ver)
            .await
            .expect("version should appear in cache");
        assert!(path.exists(), "solc compiler file should exists");
    }
}
