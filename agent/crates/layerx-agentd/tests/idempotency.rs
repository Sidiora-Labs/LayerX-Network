use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};
use std::sync::{Arc, Barrier};
use std::thread;

use layerx_agentd::idempotency::{
    EconomicResult, IdempotencyError, Outcome, RetentionPolicy, Store,
};
use layerx_agentd::store::TenantId;

static NEXT_DIRECTORY: AtomicU64 = AtomicU64::new(1);

fn directory(label: &str) -> std::path::PathBuf {
    let sequence = NEXT_DIRECTORY.fetch_add(1, Ordering::Relaxed);
    std::env::temp_dir().join(format!(
        "layerx-idempotency-{label}-{}-{sequence}",
        std::process::id()
    ))
}

fn tenant() -> TenantId {
    TenantId::new("tenant-a").unwrap_or_else(|error| panic!("tenant: {error}"))
}

fn retention() -> RetentionPolicy {
    RetentionPolicy::new(100, 50).unwrap_or_else(|error| panic!("retention: {error:?}"))
}

fn result() -> EconomicResult {
    EconomicResult {
        response_bytes: b"executed-once".to_vec(),
        receipt_ref: Some([0x77; 32]),
    }
}

#[test]
fn repeated_key_returns_original_and_changed_body_conflicts() {
    let root = directory("repeat");
    let store =
        Store::open(&root, tenant(), retention()).unwrap_or_else(|error| panic!("open: {error:?}"));
    let key = [1; 32];
    let first = store
        .execute(key, b"canonical-request", 100, |attempt| {
            assert_eq!(attempt.idempotency_key, key);
            assert_eq!(attempt.exact_request_bytes, b"canonical-request");
            assert!(!attempt.retry);
            Ok(result())
        })
        .unwrap_or_else(|error| panic!("first: {error:?}"));
    assert_eq!(first, Outcome::First(result()));
    let repeated = store
        .execute(key, b"canonical-request", 101, |_| {
            panic!("settled duplicate executed again")
        })
        .unwrap_or_else(|error| panic!("repeat: {error:?}"));
    assert_eq!(repeated, Outcome::RepeatedOriginal(result()));
    assert!(matches!(
        store.execute(key, b"different-request", 102, |_| Ok(result())),
        Err(IdempotencyError::Conflict(conflict))
            if conflict.key == key
                && conflict.original_request_digest != conflict.repeated_request_digest
    ));
    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn concurrent_duplicates_produce_exactly_one_economic_effect() {
    let root = directory("concurrent");
    let store = Arc::new(
        Store::open(&root, tenant(), retention()).unwrap_or_else(|error| panic!("open: {error:?}")),
    );
    let barrier = Arc::new(Barrier::new(32));
    let effects = Arc::new(AtomicUsize::new(0));
    let mut workers = Vec::new();
    for _ in 0..32 {
        let store = Arc::clone(&store);
        let barrier = Arc::clone(&barrier);
        let effects = Arc::clone(&effects);
        workers.push(thread::spawn(move || {
            barrier.wait();
            store.execute([2; 32], b"same-economic-intent", 100, |attempt| {
                assert_eq!(attempt.idempotency_key, [2; 32]);
                effects.fetch_add(1, Ordering::SeqCst);
                Ok(result())
            })
        }));
    }
    let outcomes: Vec<_> = workers
        .into_iter()
        .map(|worker| {
            worker
                .join()
                .unwrap_or_else(|_| panic!("worker panicked"))
                .unwrap_or_else(|error| panic!("execute: {error:?}"))
        })
        .collect();
    assert_eq!(effects.load(Ordering::SeqCst), 1);
    assert_eq!(
        outcomes
            .iter()
            .filter(|outcome| matches!(outcome, Outcome::First(_)))
            .count(),
        1
    );
    assert_eq!(
        outcomes
            .iter()
            .filter(|outcome| matches!(outcome, Outcome::RepeatedOriginal(_)))
            .count(),
        31
    );
    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn post_restart_retry_reuses_original_result_and_pending_bytes() {
    let root = directory("restart");
    let settled_key = [3; 32];
    {
        let store = Store::open(&root, tenant(), retention())
            .unwrap_or_else(|error| panic!("open: {error:?}"));
        store
            .execute(settled_key, b"settled-body", 100, |_| Ok(result()))
            .unwrap_or_else(|error| panic!("settle: {error:?}"));
        assert!(matches!(
            store.execute([4; 32], b"pending-body", 100, |_| Err(
                "transport lost".to_owned()
            )),
            Err(IdempotencyError::Operation(_))
        ));
    }
    let store = Store::open(&root, tenant(), retention())
        .unwrap_or_else(|error| panic!("reopen: {error:?}"));
    assert_eq!(
        store
            .restore(&[settled_key, [4; 32]])
            .unwrap_or_else(|error| panic!("restore: {error:?}")),
        2
    );
    let settled = store
        .execute(settled_key, b"settled-body", 101, |_| {
            panic!("restored settled operation executed")
        })
        .unwrap_or_else(|error| panic!("restored settled: {error:?}"));
    assert_eq!(settled, Outcome::RepeatedOriginal(result()));
    assert_eq!(
        store
            .sweep(1_000)
            .unwrap_or_else(|error| panic!("sweep: {error:?}")),
        1
    );
    let retried = store
        .execute([4; 32], b"pending-body", 1_001, |attempt| {
            assert!(attempt.retry);
            assert_eq!(attempt.idempotency_key, [4; 32]);
            assert_eq!(attempt.exact_request_bytes, b"pending-body");
            Ok(result())
        })
        .unwrap_or_else(|error| panic!("retry: {error:?}"));
    assert_eq!(retried, Outcome::RepeatedOriginal(result()));
    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn retention_never_expires_inside_the_protocol_window() {
    assert!(matches!(
        RetentionPolicy::new(49, 50),
        Err(IdempotencyError::InvalidRetention)
    ));
    let root = directory("retention");
    let store =
        Store::open(&root, tenant(), retention()).unwrap_or_else(|error| panic!("open: {error:?}"));
    store
        .execute([5; 32], b"retained-intent", 100, |_| Ok(result()))
        .unwrap_or_else(|error| panic!("execute: {error:?}"));
    assert_eq!(
        store
            .sweep(149)
            .unwrap_or_else(|error| panic!("sweep: {error:?}")),
        0
    );
    assert_eq!(
        store
            .sweep(199)
            .unwrap_or_else(|error| panic!("sweep: {error:?}")),
        0
    );
    assert_eq!(
        store
            .sweep(200)
            .unwrap_or_else(|error| panic!("sweep: {error:?}")),
        1
    );
    let _ = std::fs::remove_dir_all(root);
}
