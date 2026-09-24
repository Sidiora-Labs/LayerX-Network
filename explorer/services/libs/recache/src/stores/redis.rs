use crate::stores::AsyncCacheStore;
use bon::bon;
use redis::{AsyncCommands, RedisError};
use serde::{Deserialize, Serialize};
use std::{sync::Arc, time::Duration};
use thiserror::Error;

#[derive(Clone)]
pub struct RedisStore {
    connection: redis::aio::ConnectionManager,
    prefix: String,
}

#[bon]
impl RedisStore {
    #[builder]
    pub async fn new(
        connection_string: impl Into<String>,
        prefix: impl Into<String>,
        reconnect_retry_factor: Option<u64>,
        reconnect_max_delay: Option<Duration>,
        response_timeout: Option<Duration>,
        connection_timeout: Option<Duration>,
    ) -> Result<Self, RedisError> {
        let client = redis::Client::open(connection_string.into())?;

        let mut config = redis::aio::ConnectionManagerConfig::new();
        if let Some(reconnect_retry_factor) = reconnect_retry_factor {
            config = config.set_factor(reconnect_retry_factor);
        }
        if let Some(reconnect_max_delay) = reconnect_max_delay {
            config = config.set_max_delay(reconnect_max_delay.as_millis() as u64);
        }
        if let Some(response_timeout) = response_timeout {
            config = config.set_response_timeout(response_timeout);
        }
        if let Some(connection_timeout) = connection_timeout {
            config = config.set_connection_timeout(connection_timeout);
        }

        let connection = redis::aio::ConnectionManager::new_with_config(client, config).await?;

        let prefix = prefix.into();

        Ok(Self { connection, prefix })
    }

    fn format_key(&self, key: &str) -> String {
        format!("{}:{}", self.prefix, key)
    }

    fn try_deserialize<V>(&self, value: &str) -> Result<V, RedisStoreError>
    where
        V: for<'de> Deserialize<'de>,
    {
        serde_json::from_str(value).map_err(|e| RedisStoreError::CacheDeserializationError {
            cached_value: value.to_string(),
            error: Arc::new(e),
        })
    }
}

#[derive(Error, Debug, Clone)]
pub enum RedisStoreError {
    #[error("redis error")]
    RedisCacheError(#[from] Arc<redis::RedisError>),
    #[error("error deserializing cached value {cached_value:?}: {error:?}")]
    CacheDeserializationError {
        cached_value: String,
        error: Arc<serde_json::Error>,
    },
    #[error("error serializing cached value: {error:?}")]
    CacheSerializationError { error: Arc<serde_json::Error> },
}

#[async_trait::async_trait]
impl<V> AsyncCacheStore<String, V> for RedisStore
where
    V: Send + Sync + Serialize + for<'de> Deserialize<'de>,
{
    type Error = RedisStoreError;

    async fn get(&self, key: &String) -> Result<Option<V>, Self::Error> {
        let mut conn = self.connection.clone();
        let val: Option<String> = conn.get(self.format_key(key)).await.map_err(Arc::new)?;

        val.map(|v| self.try_deserialize(&v)).transpose()
    }

    async fn get_with_ttl(
        &self,
        key: &String,
    ) -> Result<Option<(V, Option<Duration>)>, Self::Error> {
        let script =
            redis::Script::new(r#"return {redis.call("GET",KEYS[1]),redis.call("PTTL",KEYS[1])}"#);
        let mut conn = self.connection.clone();

        let (val, ttl): (Option<String>, i64) = script
            .key(self.format_key(key))
            .invoke_async(&mut conn)
            .await
            .map_err(Arc::new)?;

        let val = val.map(|v| self.try_deserialize(&v)).transpose()?.map(|v| {
            let ttl_duration = if ttl >= 0 {
                Some(Duration::from_millis(ttl as u64))
            } else {
                None
            };
            (v, ttl_duration)
        });

        Ok(val)
    }

    async fn set(&self, key: &String, value: &V, ttl: Option<Duration>) -> Result<(), Self::Error> {
        let mut conn = self.connection.clone();
        let json = serde_json::to_string(value)
            .map_err(|e| RedisStoreError::CacheSerializationError { error: Arc::new(e) })?;
        let redis_key = self.format_key(key);

        match ttl {
            Some(ttl) => {
                let _: () = conn
                    .set_ex(redis_key, json, ttl.as_secs())
                    .await
                    .map_err(Arc::new)?;
            }
            None => {
                let _: () = conn.set(redis_key, json).await.map_err(Arc::new)?;
            }
        }
        Ok(())
    }

    async fn delete(&self, key: &String) -> Result<(), Self::Error> {
        let mut conn = self.connection.clone();
        let _: () = conn.del(self.format_key(key)).await.map_err(Arc::new)?;
        Ok(())
    }
}
