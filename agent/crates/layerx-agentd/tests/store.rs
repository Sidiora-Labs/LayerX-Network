use layerx_agentd::store::{
    migrate, MigrationError, ObjectKind, StorageClass, Store, StoreError, StoredValue, TenantId,
    TenantKey, CURRENT_SCHEMA_VERSION,
};
use std::fs;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};

static NEXT_DIRECTORY: AtomicU64 = AtomicU64::new(1);

fn test_directory(name: &str) -> PathBuf {
    let sequence = NEXT_DIRECTORY.fetch_add(1, Ordering::Relaxed);
    std::env::temp_dir().join(format!(
        "layerx-agentd-{name}-{}-{sequence}",
        std::process::id()
    ))
}

fn tenant(name: &str) -> TenantId {
    match TenantId::new(name) {
        Ok(value) => value,
        Err(error) => panic!("test tenant must be valid: {error}"),
    }
}

fn tenant_key(tenant_id: &TenantId, kind: ObjectKind, id: &[u8]) -> TenantKey {
    match TenantKey::new(tenant_id.clone(), kind, id.to_vec()) {
        Ok(value) => value,
        Err(error) => panic!("test key must be valid: {error}"),
    }
}

#[test]
fn every_access_is_tenant_scoped_and_persists_exact_bytes() {
    let root = test_directory("scope");
    let alpha = tenant("alpha");
    let beta = tenant("beta");
    let alpha_key = tenant_key(&alpha, ObjectKind::Policy, b"default");
    let beta_key = tenant_key(&beta, ObjectKind::Policy, b"default");
    let mut store = match Store::open(&root) {
        Ok(value) => value,
        Err(error) => panic!("store open failed: {error}"),
    };
    if let Err(error) = store.put_local(alpha_key.clone(), b"alpha-policy".to_vec()) {
        panic!("put failed: {error}");
    }
    assert!(store.get(&beta_key).is_none());
    drop(store);

    let reopened = match Store::open(&root) {
        Ok(value) => value,
        Err(error) => panic!("store reopen failed: {error}"),
    };
    let Some(stored) = reopened.get(&alpha_key) else {
        panic!("persisted value missing")
    };
    assert_eq!(stored.bytes(), b"alpha-policy");
    assert_eq!(stored.class(), StorageClass::LocalOnly);
    let _ = fs::remove_dir_all(root);
}

#[test]
fn tenant_enumeration_is_kind_scoped_stable_and_restored_from_disk() {
    let root = test_directory("tenant-enumeration");
    let alpha = tenant("alpha");
    let beta = tenant("beta");
    let zeta = tenant("zeta");
    let policy_only = tenant("policy-only");
    let mut store = match Store::open(&root) {
        Ok(value) => value,
        Err(error) => panic!("store open failed: {error}"),
    };
    assert!(store.tenant_ids_for_kind(ObjectKind::Session).is_empty());

    for (tenant_id, object_id) in [
        (zeta.clone(), b"session-z".as_slice()),
        (alpha.clone(), b"session-a-2".as_slice()),
        (beta.clone(), b"session-b".as_slice()),
        (alpha.clone(), b"session-a-1".as_slice()),
    ] {
        let key = tenant_key(&tenant_id, ObjectKind::Session, object_id);
        if let Err(error) = store.put_local(key, object_id.to_vec()) {
            panic!("session write failed: {error}");
        }
    }
    if let Err(error) = store.put_local(
        tenant_key(&policy_only, ObjectKind::Policy, b"policy"),
        b"policy-bytes".to_vec(),
    ) {
        panic!("policy write failed: {error}");
    }
    if let Err(error) = store.put_core_cache(
        tenant_key(&beta, ObjectKind::Receipt, b"receipt"),
        b"receipt-bytes".to_vec(),
    ) {
        panic!("receipt write failed: {error}");
    }
    drop(store);

    let reopened = match Store::open(&root) {
        Ok(value) => value,
        Err(error) => panic!("store reopen failed: {error}"),
    };
    assert_eq!(
        reopened.tenant_ids_for_kind(ObjectKind::Session),
        vec![alpha, beta.clone(), zeta]
    );
    assert_eq!(
        reopened.tenant_ids_for_kind(ObjectKind::Receipt),
        vec![beta]
    );
    assert_eq!(
        reopened.tenant_ids_for_kind(ObjectKind::Policy),
        vec![policy_only]
    );
    assert!(reopened
        .tenant_ids_for_kind(ObjectKind::Configuration)
        .is_empty());
    drop(reopened);
    let _ = fs::remove_dir_all(root);
}

#[test]
fn signed_bytes_outbox_and_idempotency_are_one_durable_write() {
    let root = test_directory("transaction");
    let tenant_id = tenant("tenant-a");
    let mut store = match Store::open(&root) {
        Ok(value) => value,
        Err(error) => panic!("store open failed: {error}"),
    };
    if let Err(error) = store.record_submission(
        tenant_id.clone(),
        b"intent-1".to_vec(),
        b"signed-canonical-activity".to_vec(),
        b"queued".to_vec(),
    ) {
        panic!("submission transaction failed: {error}");
    }
    drop(store);

    let reopened = match Store::open(&root) {
        Ok(value) => value,
        Err(error) => panic!("store reopen failed: {error}"),
    };
    for kind in [
        ObjectKind::PreparedActivity,
        ObjectKind::Outbox,
        ObjectKind::Idempotency,
    ] {
        assert!(reopened
            .get(&tenant_key(&tenant_id, kind, b"intent-1"))
            .is_some());
    }
    let _ = fs::remove_dir_all(root);
}

#[test]
fn deletion_loses_only_declared_local_artifacts_and_core_cache_rebuilds() {
    let root = test_directory("rebuild");
    let tenant_id = tenant("tenant-a");
    let receipt_key = tenant_key(&tenant_id, ObjectKind::Receipt, b"activity-7");
    let policy_key = tenant_key(&tenant_id, ObjectKind::Policy, b"policy-1");
    let core_receipt = b"core-produced-receipt".to_vec();

    let mut store = match Store::open(&root) {
        Ok(value) => value,
        Err(error) => panic!("store open failed: {error}"),
    };
    if let Err(error) = store.put_core_cache(receipt_key.clone(), core_receipt.clone()) {
        panic!("cache write failed: {error}");
    }
    if let Err(error) = store.put_local(policy_key.clone(), b"local-policy".to_vec()) {
        panic!("local write failed: {error}");
    }
    drop(store);
    if let Err(error) = fs::remove_dir_all(&root) {
        panic!("store deletion failed: {error}");
    }

    let mut restarted = match Store::open(&root) {
        Ok(value) => value,
        Err(error) => panic!("store restart failed: {error}"),
    };
    assert!(restarted.get(&receipt_key).is_none());
    assert!(restarted.get(&policy_key).is_none());
    if let Err(error) = restarted.restore_core_cache([(receipt_key.clone(), core_receipt)]) {
        panic!("core reconstruction failed: {error}");
    }
    let Some(rebuilt) = restarted.get(&receipt_key) else {
        panic!("core-produced receipt did not reconstruct")
    };
    assert_eq!(rebuilt.class(), StorageClass::CoreProducedCache);
    assert!(restarted.get(&policy_key).is_none());
    let _ = fs::remove_dir_all(root);
}

#[test]
fn migrates_every_supported_prior_version_and_refuses_newer() {
    let root = test_directory("migration");
    if let Err(error) = fs::create_dir_all(&root) {
        panic!("test directory failed: {error}");
    }
    let mut v1 = Vec::new();
    v1.extend_from_slice(b"LXAS");
    v1.extend_from_slice(&1_u32.to_be_bytes());
    v1.extend_from_slice(&1_u32.to_be_bytes());
    let tenant_bytes = b"tenant-a".as_slice();
    v1.extend_from_slice(&u32::try_from(tenant_bytes.len()).unwrap_or(0).to_be_bytes());
    v1.extend_from_slice(tenant_bytes);
    v1.push(ObjectKind::Receipt as u8);
    for bytes in [b"receipt-1".as_slice(), b"receipt-bytes".as_slice()] {
        v1.extend_from_slice(&u32::try_from(bytes.len()).unwrap_or(0).to_be_bytes());
        v1.extend_from_slice(bytes);
    }
    if let Err(error) = fs::write(root.join("store.bin"), v1) {
        panic!("v1 store write failed: {error}");
    }
    if let Err(error) = migrate(&root) {
        panic!("migration failed: {error}");
    }
    let migrated = match fs::read(root.join("store.bin")) {
        Ok(value) => value,
        Err(error) => panic!("migrated read failed: {error}"),
    };
    assert_eq!(&migrated[4..8], &CURRENT_SCHEMA_VERSION.to_be_bytes());
    let opened = match Store::open(&root) {
        Ok(value) => value,
        Err(error) => panic!("migrated store did not open: {error}"),
    };
    let receipt = tenant_key(&tenant("tenant-a"), ObjectKind::Receipt, b"receipt-1");
    assert_eq!(
        opened.get(&receipt).map(StoredValue::class),
        Some(StorageClass::CoreProducedCache)
    );

    let mut newer = b"LXAS".to_vec();
    newer.extend_from_slice(&(CURRENT_SCHEMA_VERSION + 1).to_be_bytes());
    if let Err(error) = fs::write(root.join("store.bin"), newer) {
        panic!("newer store write failed: {error}");
    }
    let Err(error) = Store::open(&root) else {
        panic!("newer schema was accepted")
    };
    assert!(matches!(
        error,
        StoreError::Migration(MigrationError::NewerSchema { .. })
    ));
    let _ = fs::remove_dir_all(root);
}
