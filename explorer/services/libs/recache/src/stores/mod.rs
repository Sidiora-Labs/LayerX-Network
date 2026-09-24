pub mod redis;

use std::time::Duration;

#[async_trait::async_trait]
pub trait AsyncCacheStore<K, V>
where
    K: Sync,
{
    type Error;

    async fn get(&self, key: &K) -> Result<Option<V>, Self::Error>;
    async fn get_with_ttl(&self, key: &K) -> Result<Option<(V, Option<Duration>)>, Self::Error> {
        self.get(key).await.map(|val| val.map(|val| (val, None)))
    }
    async fn set(&self, key: &K, value: &V, ttl: Option<Duration>) -> Result<(), Self::Error>;
    async fn delete(&self, key: &K) -> Result<(), Self::Error>;
}
