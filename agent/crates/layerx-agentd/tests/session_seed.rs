use layerx_agentd::session_keys::SessionKeyRegistry;
use std::fs;
use std::os::unix::fs::{MetadataExt as _, PermissionsExt as _};
use std::sync::atomic::{AtomicU64, Ordering};

static NEXT: AtomicU64 = AtomicU64::new(1);

#[test]
fn prepared_session_seeds_are_encrypted_bound_and_survive_real_registry_restart() {
    let root = std::env::temp_dir().join(format!(
        "layerx-session-seed-{}-{}",
        std::process::id(),
        NEXT.fetch_add(1, Ordering::Relaxed)
    ));
    fs::create_dir(&root).unwrap_or_else(|error| panic!("private test directory: {error:?}"));
    fs::set_permissions(&root, fs::Permissions::from_mode(0o700))
        .unwrap_or_else(|error| panic!("private permissions: {error:?}"));
    let uid = fs::metadata(&root)
        .unwrap_or_else(|error| panic!("directory metadata: {error:?}"))
        .uid();
    let registry = SessionKeyRegistry::open(root.clone(), vec![0x61; 32], 77, uid)
        .unwrap_or_else(|error| panic!("real encrypted registry: {error:?}"));
    let namespace = b"tenant-a/principal-a/agent-a/action-a";
    let seed = registry
        .prepare_seed(namespace, [7; 32])
        .unwrap_or_else(|error| panic!("initial preparation: {error:?}"));
    assert_ne!(*seed, [0; 32]);
    assert_eq!(
        *registry
            .prepare_seed(namespace, [7; 32])
            .unwrap_or_else(|error| panic!("same request: {error:?}")),
        *seed
    );
    assert!(registry.prepare_seed(namespace, [8; 32]).is_err());
    assert_ne!(
        *registry
            .prepare_seed(b"tenant-b/principal-a/agent-a/action-a", [7; 32])
            .unwrap_or_else(|error| panic!("separate tenant: {error:?}")),
        *seed
    );
    for entry in fs::read_dir(&root).unwrap_or_else(|error| panic!("registry entries: {error:?}")) {
        let bytes = fs::read(
            entry
                .unwrap_or_else(|error| panic!("entry: {error:?}"))
                .path(),
        )
        .unwrap_or_else(|error| panic!("encrypted record: {error:?}"));
        assert!(!bytes.windows(32).any(|part| part == seed.as_ref()));
    }
    registry
        .probe()
        .unwrap_or_else(|error| panic!("all envelopes authenticate: {error:?}"));
    drop(registry);
    let restarted = SessionKeyRegistry::open(root.clone(), vec![0x61; 32], 77, uid)
        .unwrap_or_else(|error| panic!("restart: {error:?}"));
    assert_eq!(
        *restarted
            .prepare_seed(namespace, [7; 32])
            .unwrap_or_else(|error| panic!("durable seed: {error:?}")),
        *seed
    );
    drop(restarted);
    let wrong_network = SessionKeyRegistry::open(root.clone(), vec![0x61; 32], 78, uid)
        .unwrap_or_else(|error| panic!("network-bound reader: {error:?}"));
    assert!(wrong_network.prepare_seed(namespace, [7; 32]).is_err());
    let wrong_key = SessionKeyRegistry::open(root, vec![0x62; 32], 77, uid)
        .unwrap_or_else(|error| panic!("secret-bound reader: {error:?}"));
    assert!(wrong_key.prepare_seed(namespace, [7; 32]).is_err());
}
