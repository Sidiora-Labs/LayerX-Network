use crate::scheduler;
use async_trait::async_trait;
use cron::Schedule;
use std::sync::Arc;
use tracing::instrument;

#[async_trait]
pub trait VersionsFetcher {
    type Versions;
    type Error: std::fmt::Display;

    fn len(vers: &Self::Versions) -> usize;
    async fn fetch_versions(&self) -> Result<Self::Versions, Self::Error>;
}

pub struct VersionsRefresher<T>(Arc<parking_lot::RwLock<T>>);

impl<T> Clone for VersionsRefresher<T> {
    fn clone(&self) -> Self {
        Self(self.0.clone())
    }
}

impl<T> VersionsRefresher<T> {
    pub fn new_static(val: T) -> Self {
        Self(Arc::new(parking_lot::RwLock::new(val)))
    }

    pub fn read(&self) -> parking_lot::RwLockReadGuard<'_, T> {
        self.0.read()
    }
}

impl<T: PartialEq> VersionsRefresher<T> {
    #[instrument(skip(self, fetcher), level = "debug")]
    pub async fn refresh<F: VersionsFetcher<Versions = T>>(&self, fetcher: &F) {
        tracing::info!("looking for new compilers versions");
        let refresh_result = fetcher.fetch_versions().await;
        match refresh_result {
            Ok(fetched_versions) => {
                let need_to_update = {
                    let old = self.0.read();
                    fetched_versions != *old
                };
                if need_to_update {
                    let (old_len, new_len) = {
                        // we don't need to check condition again,
                        // we can just override the value
                        let mut old = self.0.write();
                        let old_len = F::len(&old);
                        let new_len = F::len(&fetched_versions);
                        *old = fetched_versions;
                        (old_len, new_len)
                    };
                    tracing::info!(
                        "found new compiler versions. old length: {}, new length: {}",
                        old_len,
                        new_len,
                    );
                } else {
                    tracing::info!("no new versions found")
                }
            }
            Err(err) => {
                tracing::error!("error during version refresh: {}", err);
            }
        }
    }
}

impl<T: PartialEq + Send + Sync + 'static> VersionsRefresher<T> {
    pub async fn new<F: VersionsFetcher<Versions = T> + Send + Sync + 'static>(
        fetcher: Arc<F>,
        refresh_schedule: Option<Schedule>,
    ) -> Result<Self, F::Error> {
        let values = fetcher.fetch_versions().await?;
        let refresher = Self::new_static(values);
        if let Some(cron_schedule) = refresh_schedule {
            refresher.clone().spawn_refresh_job(fetcher, cron_schedule);
        }
        Ok(refresher)
    }

    fn spawn_refresh_job<F: VersionsFetcher<Versions = T> + Send + Sync + 'static>(
        self,
        fetcher: Arc<F>,
        cron_schedule: Schedule,
    ) {
        tracing::info!("spawn version refresh job");
        scheduler::spawn_job(cron_schedule, "refresh compiler versions", move || {
            let fetcher = fetcher.clone();
            let versions = self.clone();
            async move { versions.refresh(fetcher.as_ref()).await }
        });
    }
}
