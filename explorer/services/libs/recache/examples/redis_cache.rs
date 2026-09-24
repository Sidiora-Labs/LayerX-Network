use recache::{handler::CacheHandler, stores::redis::RedisStore};
use serde::{Deserialize, Serialize};
use std::{
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    },
    time::Duration,
};

#[derive(Debug, Serialize, Deserialize, Clone)]
struct A {
    a: i32,
    b: String,
}

impl A {
    pub fn new(a: i32, b: String) -> Self {
        Self { a, b }
    }
}

type RedisCacheHandler<V> = CacheHandler<RedisStore, String, V>;
static REFRESH_HOOK_COUNTER: AtomicUsize = AtomicUsize::new(0);

async fn call(cache: &RedisCacheHandler<A>) {
    let res = cache
        .default_request()
        .key("test")
        .execute(|| async {
            tokio::time::sleep(Duration::from_secs(10)).await;
            Ok::<A, String>(A::new(1, "2".to_string()))
        })
        .await
        .unwrap();
    println!("{res:?}");
}

#[tokio::main]
pub async fn main() {
    let cache = RedisStore::builder()
        .connection_string("redis://127.0.0.1:6379")
        .prefix("test")
        .build()
        .await
        .unwrap();

    let cache = CacheHandler::builder(Arc::new(cache))
        .on_refresh_computed(Arc::new(move |k, v| {
            REFRESH_HOOK_COUNTER.fetch_add(1, Ordering::Relaxed);
            println!("refreshed: {k:?} {v:?}")
        }))
        .on_inflight_computed(Arc::new(move |k, v| {
            println!("got result from inflight request: {k:?} {v:?}")
        }))
        .default_ttl(Duration::from_secs(10))
        .maybe_default_refresh_ahead(Some(Duration::from_secs(5)))
        .build();

    let now = std::time::Instant::now();

    // Initializes the cache
    call(&cache).await;
    println!("time: {:?}", now.elapsed());
    tokio::time::sleep(Duration::from_secs(6)).await;
    println!("time: {:?}", now.elapsed());
    // Triggers background refresh-ahead
    call(&cache).await;
    println!("time: {:?}", now.elapsed());
    tokio::time::sleep(Duration::from_secs(6)).await;
    println!("time: {:?}", now.elapsed());
    // Awaits already running background refresh-ahead
    call(&cache).await;
    println!("time: {:?}", now.elapsed());
}
