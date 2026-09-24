use crate::{handler::cache_request_builder::*, stores::AsyncCacheStore};
use bon::Builder;
use dashmap::DashMap;
use futures::{
    FutureExt,
    future::{BoxFuture, Shared},
};
use std::{cmp::Eq, fmt::Display, hash::Hash, sync::Arc, time::Duration};
use thiserror::Error;

type SharedFuture<V, E> = Shared<BoxFuture<'static, Result<V, E>>>;
type EventHandler<K, V> = Arc<dyn Fn(&K, &V) + Send + Sync>;
type InflightRequest<K, V, C> =
    SharedFuture<V, CacheRequestError<<C as AsyncCacheStore<K, V>>::Error>>;
type InflightMap<K, V, C> = DashMap<K, InflightRequest<K, V, C>>;

#[derive(Builder, Clone)]
pub struct CacheHandler<C, K, V>
where
    C: AsyncCacheStore<K, V>,
    K: Sync + Hash + Eq,
{
    #[builder(start_fn)]
    cache: Arc<C>,
    #[builder(default)]
    inflight: Arc<InflightMap<K, V, C>>,
    #[builder(default = noop_event_handler())]
    on_hit: EventHandler<K, V>,
    #[builder(default = noop_event_handler())]
    on_computed: EventHandler<K, V>,
    #[builder(default = noop_event_handler())]
    on_inflight_computed: EventHandler<K, V>,
    #[builder(default = noop_event_handler())]
    on_refresh_computed: EventHandler<K, V>,
    pub default_ttl: Duration,
    pub default_refresh_ahead: Option<Duration>,
}

#[inline]
fn noop_event_handler<K, V>() -> EventHandler<K, V> {
    Arc::new(|_: &K, _: &V| {})
}

impl<C, K, V> CacheHandler<C, K, V>
where
    C: AsyncCacheStore<K, V> + Send + Sync + 'static,
    K: Sync + Hash + Eq,
{
    pub fn request(&self) -> CacheRequestBuilder<C, K, V>
    where
        Self: Sized,
    {
        CacheRequest::builder(
            self.cache.clone(),
            self.inflight.clone(),
            self.on_hit.clone(),
            self.on_computed.clone(),
            self.on_inflight_computed.clone(),
            self.on_refresh_computed.clone(),
        )
    }

    pub fn default_request(&self) -> CacheRequestBuilder<C, K, V, SetRefreshAhead<SetTtl>>
    where
        Self: Sized,
    {
        self.request()
            .ttl(self.default_ttl)
            .maybe_refresh_ahead(self.default_refresh_ahead)
    }
}

#[derive(Debug, Clone, thiserror::Error)]
#[error("stringified error: {inner}")]
pub struct CachedError {
    inner: String,
}

impl CachedError {
    pub fn new<E: std::fmt::Display>(e: E) -> Self {
        Self {
            inner: e.to_string(),
        }
    }
}

#[derive(Builder)]
#[builder(finish_fn(vis = "", name = build_internal))]
pub struct CacheRequest<C, K, V>
where
    C: AsyncCacheStore<K, V>,
    K: Sync,
{
    #[builder(start_fn)]
    cache: Arc<C>,
    #[builder(start_fn)]
    inflight: Arc<InflightMap<K, V, C>>,
    #[builder(start_fn)]
    on_hit: EventHandler<K, V>,
    #[builder(start_fn)]
    on_computed: EventHandler<K, V>,
    #[builder(start_fn)]
    on_inflight_computed: EventHandler<K, V>,
    #[builder(start_fn)]
    on_refresh_computed: EventHandler<K, V>,
    #[builder(into)]
    key: K,
    ttl: Option<Duration>,
    refresh_ahead: Option<Duration>,
}

#[derive(Error, Debug, Clone)]
pub enum CacheRequestError<CacheError> {
    #[error("compute error: {0}")]
    ComputeError(String),
    #[error("cache error: {0}")]
    CacheError(#[from] CacheError),
}

impl<C, K, V> CacheRequest<C, K, V>
where
    C: AsyncCacheStore<K, V> + Send + Sync + 'static,
    C::Error: Send + Sync + Clone,
    K: Send + Sync + Clone + 'static + Hash + Eq,
    V: Send + Sync + Clone + 'static,
{
    pub async fn execute<F, Fut, E>(self, compute: F) -> Result<V, CacheRequestError<C::Error>>
    where
        F: FnOnce() -> Fut,
        Fut: Future<Output = Result<V, E>> + Send + 'static,
        E: Display,
    {
        if let Some((val, maybe_ttl)) = self.cache.get_with_ttl(&self.key).await? {
            (self.on_hit)(&self.key, &val);

            if let Some(value_ttl) = maybe_ttl {
                self.handle_refresh(compute, value_ttl);
            }
            return Ok(val);
        }

        let fut = match self.inflight.entry(self.key.clone()) {
            dashmap::Entry::Occupied(entry) => {
                let fut_shared = entry.get().clone();
                async move {
                    let val = fut_shared.await?;
                    (self.on_inflight_computed)(&self.key, &val);
                    Ok(val)
                }
                .boxed()
            }
            dashmap::Entry::Vacant(entry) => {
                let fut_shared = Self::build_shared_future(compute());

                entry.insert(fut_shared.clone());

                let inflight = Arc::clone(&self.inflight);
                async move {
                    let res = fut_shared.await;
                    if let Ok(val) = res.as_ref() {
                        let _ = self.cache.set(&self.key, val, self.ttl).await;
                        (self.on_computed)(&self.key, val);
                    }
                    inflight.remove(&self.key);
                    res
                }
                .boxed()
            }
        };

        fut.await
    }

    pub fn handle_refresh<F, Fut, E>(self, compute: F, value_ttl: Duration)
    where
        F: FnOnce() -> Fut,
        Fut: Future<Output = Result<V, E>> + Send + 'static,
        E: Display,
    {
        if self.ttl.is_none() {
            return;
        }

        let refresh_threshold = match self.refresh_ahead {
            Some(refresh_threshold) => refresh_threshold,
            None => return,
        };

        if value_ttl >= refresh_threshold {
            return;
        }

        let entry = self.inflight.entry(self.key.clone());
        if let dashmap::Entry::Vacant(entry) = entry {
            let fut_shared = Self::build_shared_future(compute());

            entry.insert(fut_shared.clone());

            let inflight = Arc::clone(&self.inflight);
            tokio::spawn(async move {
                let res = fut_shared.await;
                if let Ok(val) = res.as_ref() {
                    let _ = self.cache.set(&self.key, val, self.ttl).await;
                    (self.on_refresh_computed)(&self.key, val);
                }
                inflight.remove(&self.key);
            });
        }
    }

    fn build_shared_future<Fut, E>(fut: Fut) -> SharedFuture<V, CacheRequestError<C::Error>>
    where
        Fut: Future<Output = Result<V, E>> + Send + 'static,
        E: Display,
    {
        async move {
            fut.await
                .map_err(|e| CacheRequestError::ComputeError(e.to_string()))
        }
        .boxed()
        .shared()
    }
}

impl<C, K, V, S: cache_request_builder::IsComplete> CacheRequestBuilder<C, K, V, S>
where
    C: AsyncCacheStore<K, V> + Send + Sync + 'static,
    C::Error: Send + Sync + Clone,
    K: Send + Sync + Clone + 'static + Hash + Eq,
    V: Send + Sync + Clone + 'static,
{
    pub async fn execute<F, Fut, E>(self, compute: F) -> Result<V, CacheRequestError<C::Error>>
    where
        F: FnOnce() -> Fut,
        Fut: Future<Output = Result<V, E>> + Send + 'static,
        E: Display,
    {
        let req = self.build_internal();
        req.execute(compute).await
    }
}
