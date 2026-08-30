mod support;

use std::fs;

use layerx_human_service::store::{
    AgentTenantId, AuditDisposition, EvidenceRef, MigrationError, PrincipalId, PrincipalStore,
    RowKey, StoreError, Table, TenancyError,
};
use support::{
    directory, install_and_open, principal, retention_uniform, row_key, tenancy, version_file_bytes,
};

const TABLES: [Table; 4] = [
    Table::Journeys,
    Table::Notifications,
    Table::Telemetry,
    Table::Cache,
];

fn contains(haystack: &[u8], needle: &[u8]) -> bool {
    haystack
        .windows(needle.len())
        .any(|window| window == needle)
}

fn v1_length(value: usize) -> [u8; 4] {
    u32::try_from(value)
        .unwrap_or_else(|_| panic!("length overflow"))
        .to_be_bytes()
}

fn v1_store_file(entries: &[(u8, &str, u64, &[u8])]) -> Vec<u8> {
    let mut bytes = b"LXHP".to_vec();
    bytes.extend_from_slice(&1_u32.to_be_bytes());
    bytes.extend_from_slice(&v1_length(entries.len()));
    for (code, key, written_at, value) in entries {
        bytes.push(*code);
        bytes.extend_from_slice(&v1_length(key.len()));
        bytes.extend_from_slice(key.as_bytes());
        bytes.extend_from_slice(&written_at.to_be_bytes());
        bytes.extend_from_slice(&v1_length(value.len()));
        bytes.extend_from_slice(value);
    }
    bytes
}

#[test]
fn hostile_principal_identifiers_are_unrepresentable() {
    let overlong = "a".repeat(129);
    let hostile = [
        "",
        ".",
        "..",
        "../sibling",
        "a/../b",
        "a/b",
        "a\\b",
        "a\0b",
        "Alice",
        "alice ",
        " alice",
        "café",
        "alice.bak",
        "alice:0",
        overlong.as_str(),
    ];
    for candidate in hostile {
        let refused = PrincipalId::new(candidate);
        assert!(
            matches!(refused, Err(StoreError::InvalidPrincipal)),
            "accepted hostile principal {candidate:?}"
        );
    }
    assert!(PrincipalId::new("a".repeat(128)).is_ok());
    assert!(PrincipalId::new("alice-1_x").is_ok());
}

#[test]
fn hostile_row_keys_are_unrepresentable() {
    let overlong = "k".repeat(129);
    let hostile = [
        "",
        ".",
        "..",
        "../../store",
        "k/../k",
        "k/k",
        "k\\k",
        "k\0k",
        "Key",
        "k k",
        "kéy",
        "k.bin",
        overlong.as_str(),
    ];
    for candidate in hostile {
        let refused = RowKey::new(candidate);
        assert!(
            matches!(refused, Err(StoreError::InvalidKey)),
            "accepted hostile key {candidate:?}"
        );
    }
    assert!(RowKey::new("journey-2024_01").is_ok());
}

#[test]
fn agent_tenant_identifiers_enforce_agent_layer_bounds() {
    for candidate in ["", "a\0b"] {
        assert!(matches!(
            AgentTenantId::new(candidate),
            Err(StoreError::InvalidTenant)
        ));
    }
    assert!(matches!(
        AgentTenantId::new("t".repeat(256)),
        Err(StoreError::InvalidTenant)
    ));
    assert!(AgentTenantId::new("tenant-a").is_ok());
    assert!(AgentTenantId::new("t".repeat(255)).is_ok());
}

#[test]
fn read_paths_cannot_cross_principals() {
    let root = directory("cross-read");
    let map = tenancy(&[("alice", "tenant-a"), ("bob", "tenant-b")]);
    let (mut store, _digest) = install_and_open(&root, &map, retention_uniform(1_000));
    let alice = principal("alice");
    let bob = principal("bob");
    let shared = row_key("journey-1");
    let internal_key = row_key("audit-1");
    let export_key = row_key("audit-2");
    {
        let mut scope = store
            .principal(&alice)
            .unwrap_or_else(|error| panic!("alice scope: {error}"));
        assert_eq!(scope.principal().as_str(), "alice");
        assert_eq!(scope.tenant().as_str(), "tenant-a");
        for table in TABLES {
            scope
                .put(table, shared.clone(), 10, b"alice-value".to_vec())
                .unwrap_or_else(|error| panic!("put: {error}"));
        }
        scope
            .append_audit(
                internal_key.clone(),
                11,
                b"alice-audit".to_vec(),
                AuditDisposition::Internal,
            )
            .unwrap_or_else(|error| panic!("audit: {error}"));
        scope
            .append_audit(
                export_key.clone(),
                12,
                b"alice-evidence-log".to_vec(),
                AuditDisposition::Exportable {
                    evidence: vec![EvidenceRef::new(Table::Journeys, shared.clone())],
                },
            )
            .unwrap_or_else(|error| panic!("audit: {error}"));
    }
    let scope = store
        .principal(&bob)
        .unwrap_or_else(|error| panic!("bob scope: {error}"));
    assert_eq!(scope.tenant().as_str(), "tenant-b");
    for table in TABLES {
        assert!(scope.get(table, &shared).is_none());
        assert!(scope.keys(table).is_empty());
    }
    assert!(scope.audit(&internal_key).is_none());
    assert!(scope.audit(&export_key).is_none());
    assert!(scope.audit_keys().is_empty());
}

#[test]
fn write_paths_land_only_in_the_writers_namespace() {
    let root = directory("cross-write");
    let map = tenancy(&[("alice", "tenant-a"), ("bob", "tenant-b")]);
    let (mut store, _digest) = install_and_open(&root, &map, retention_uniform(1_000));
    let alice = principal("alice");
    let bob = principal("bob");
    let shared = row_key("journey-1");
    let audit_key = row_key("audit-1");
    {
        let mut scope = store
            .principal(&alice)
            .unwrap_or_else(|error| panic!("alice scope: {error}"));
        scope
            .put(Table::Journeys, shared.clone(), 1, b"alice-owned".to_vec())
            .unwrap_or_else(|error| panic!("put: {error}"));
        scope
            .append_audit(
                audit_key.clone(),
                2,
                b"alice-audit".to_vec(),
                AuditDisposition::Internal,
            )
            .unwrap_or_else(|error| panic!("audit: {error}"));
    }
    {
        let mut scope = store
            .principal(&bob)
            .unwrap_or_else(|error| panic!("bob scope: {error}"));
        let removed = scope
            .remove(Table::Journeys, &shared)
            .unwrap_or_else(|error| panic!("remove: {error}"));
        assert!(!removed, "bob removed a row he never wrote");
        scope
            .put(Table::Journeys, shared.clone(), 3, b"bob-owned".to_vec())
            .unwrap_or_else(|error| panic!("put: {error}"));
        scope
            .append_audit(
                audit_key.clone(),
                4,
                b"bob-audit".to_vec(),
                AuditDisposition::Internal,
            )
            .unwrap_or_else(|error| panic!("audit: {error}"));
        scope
            .expire(u64::MAX)
            .unwrap_or_else(|error| panic!("expire: {error}"));
    }
    let scope = store
        .principal(&alice)
        .unwrap_or_else(|error| panic!("alice scope: {error}"));
    let row = scope
        .get(Table::Journeys, &shared)
        .unwrap_or_else(|| panic!("alice row lost"));
    assert_eq!(row.bytes(), b"alice-owned");
    assert_eq!(row.written_at(), 1);
    let audit = scope
        .audit(&audit_key)
        .unwrap_or_else(|| panic!("alice audit lost"));
    assert_eq!(audit.bytes(), b"alice-audit");
}

#[test]
fn principal_subtrees_are_disjoint_on_disk() {
    let root = directory("disk-disjoint");
    let map = tenancy(&[("alice", "tenant-a"), ("bob", "tenant-b")]);
    let (mut store, _digest) = install_and_open(&root, &map, retention_uniform(1_000));
    let sentinel_key = row_key("sentinel");
    for (name, sentinel) in [
        ("alice", b"alice-sentinel-3f9a".as_slice()),
        ("bob", b"bob-sentinel-77c2".as_slice()),
    ] {
        let mut scope = store
            .principal(&principal(name))
            .unwrap_or_else(|error| panic!("scope: {error}"));
        scope
            .put(Table::Journeys, sentinel_key.clone(), 1, sentinel.to_vec())
            .unwrap_or_else(|error| panic!("put: {error}"));
    }
    let principals_dir = root.join("principals");
    let mut names = Vec::new();
    for entry in fs::read_dir(&principals_dir).unwrap_or_else(|error| panic!("read dir: {error}")) {
        let entry = entry.unwrap_or_else(|error| panic!("entry: {error}"));
        names.push(entry.file_name().to_string_lossy().into_owned());
        assert!(entry.path().is_dir());
    }
    names.sort();
    assert_eq!(names, ["alice", "bob"]);
    let alice_bytes = fs::read(principals_dir.join("alice").join("store.bin"))
        .unwrap_or_else(|error| panic!("alice file: {error}"));
    let bob_bytes = fs::read(principals_dir.join("bob").join("store.bin"))
        .unwrap_or_else(|error| panic!("bob file: {error}"));
    assert!(contains(&alice_bytes, b"alice-sentinel-3f9a"));
    assert!(!contains(&alice_bytes, b"bob-sentinel-77c2"));
    assert!(contains(&bob_bytes, b"bob-sentinel-77c2"));
    assert!(!contains(&bob_bytes, b"alice-sentinel-3f9a"));
}

#[test]
fn unmapped_principals_are_refused() {
    let root = directory("unmapped");
    let map = tenancy(&[("alice", "tenant-a")]);
    let (mut store, _digest) = install_and_open(&root, &map, retention_uniform(1_000));
    let mallory = principal("mallory");
    let refused = store.principal(&mallory);
    assert!(matches!(
        refused,
        Err(StoreError::Tenancy(TenancyError::Unmapped))
    ));
}

#[test]
fn tenancy_configuration_tampering_is_refused() {
    let root = directory("tenancy-tamper");
    let map = tenancy(&[("alice", "tenant-a")]);
    let retention = retention_uniform(1_000);
    let expected = map
        .digest()
        .unwrap_or_else(|error| panic!("digest: {error}"));
    let missing = PrincipalStore::open(&root, retention, expected);
    assert!(matches!(
        missing,
        Err(StoreError::Tenancy(TenancyError::Missing))
    ));
    let digest = map
        .install(&root)
        .unwrap_or_else(|error| panic!("install: {error}"));
    let path = root.join("tenancy.map");
    let mut bytes = fs::read(&path).unwrap_or_else(|error| panic!("read: {error}"));
    bytes[5] ^= 0x01;
    fs::write(&path, &bytes).unwrap_or_else(|error| panic!("write: {error}"));
    let tampered = PrincipalStore::open(&root, retention, digest);
    assert!(matches!(
        tampered,
        Err(StoreError::Tenancy(TenancyError::Tampered))
    ));
    let forged = tenancy(&[("mallory", "tenant-a")]);
    forged
        .install(&root)
        .unwrap_or_else(|error| panic!("forged install: {error}"));
    let resealed = PrincipalStore::open(&root, retention, digest);
    assert!(matches!(
        resealed,
        Err(StoreError::Tenancy(TenancyError::DigestMismatch))
    ));
}

#[test]
fn newer_store_versions_are_refused_at_startup() {
    let root = directory("newer-version");
    let map = tenancy(&[("alice", "tenant-a")]);
    let (store, digest) = install_and_open(&root, &map, retention_uniform(1_000));
    drop(store);
    fs::write(root.join("store.version"), version_file_bytes(9))
        .unwrap_or_else(|error| panic!("write version: {error}"));
    let refused = PrincipalStore::open(&root, retention_uniform(1_000), digest);
    assert!(matches!(
        refused,
        Err(StoreError::Migration(MigrationError::NewerSchema {
            found: 9,
            supported: 2
        }))
    ));
    fs::write(root.join("store.version"), version_file_bytes(2))
        .unwrap_or_else(|error| panic!("restore version: {error}"));
    let alice_dir = root.join("principals").join("alice");
    fs::create_dir_all(&alice_dir).unwrap_or_else(|error| panic!("mkdir: {error}"));
    let mut newer_file = b"LXHP".to_vec();
    newer_file.extend_from_slice(&9_u32.to_be_bytes());
    fs::write(alice_dir.join("store.bin"), newer_file)
        .unwrap_or_else(|error| panic!("write file: {error}"));
    let mut store = PrincipalStore::open(&root, retention_uniform(1_000), digest)
        .unwrap_or_else(|error| panic!("open: {error}"));
    let refused = store.principal(&principal("alice"));
    assert!(matches!(
        refused,
        Err(StoreError::Migration(MigrationError::NewerSchema {
            found: 9,
            supported: 2
        }))
    ));
}

#[test]
fn v1_stores_migrate_forward_to_the_current_version() {
    let root = directory("migrate-forward");
    let map = tenancy(&[("alice", "tenant-a")]);
    let alice_dir = root.join("principals").join("alice");
    fs::create_dir_all(&alice_dir).unwrap_or_else(|error| panic!("mkdir: {error}"));
    let v1_file = v1_store_file(&[
        (3, "audit-1", 9, b"v1-audit".as_slice()),
        (1, "journey-1", 7, b"v1-journey".as_slice()),
        (2, "notice-1", 8, b"v1-notice".as_slice()),
        (4, "sample-1", 6, b"v1-sample".as_slice()),
    ]);
    fs::write(alice_dir.join("store.bin"), v1_file)
        .unwrap_or_else(|error| panic!("write file: {error}"));
    fs::write(root.join("store.version"), version_file_bytes(1))
        .unwrap_or_else(|error| panic!("write version: {error}"));
    let digest = map
        .install(&root)
        .unwrap_or_else(|error| panic!("install: {error}"));
    let mut store = PrincipalStore::open(&root, retention_uniform(1_000), digest)
        .unwrap_or_else(|error| panic!("open: {error}"));
    assert_eq!(store.version(), 2);
    let version_bytes = fs::read(root.join("store.version"))
        .unwrap_or_else(|error| panic!("read version: {error}"));
    assert_eq!(version_bytes, version_file_bytes(2));
    let migrated =
        fs::read(alice_dir.join("store.bin")).unwrap_or_else(|error| panic!("read file: {error}"));
    assert_eq!(migrated.get(4..8), Some(2_u32.to_be_bytes().as_slice()));
    {
        let scope = store
            .principal(&principal("alice"))
            .unwrap_or_else(|error| panic!("scope: {error}"));
        let journey = scope
            .get(Table::Journeys, &row_key("journey-1"))
            .unwrap_or_else(|| panic!("journey lost"));
        assert_eq!(journey.bytes(), b"v1-journey");
        assert_eq!(journey.written_at(), 7);
        let notice = scope
            .get(Table::Notifications, &row_key("notice-1"))
            .unwrap_or_else(|| panic!("notice lost"));
        assert_eq!(notice.bytes(), b"v1-notice");
        let sample = scope
            .get(Table::Telemetry, &row_key("sample-1"))
            .unwrap_or_else(|| panic!("sample lost"));
        assert_eq!(sample.bytes(), b"v1-sample");
        assert!(scope.keys(Table::Cache).is_empty());
        let audit = scope
            .audit(&row_key("audit-1"))
            .unwrap_or_else(|| panic!("audit lost"));
        assert_eq!(audit.bytes(), b"v1-audit");
        assert_eq!(audit.written_at(), 9);
        assert!(!audit.is_exportable());
    }
    drop(store);
    fs::write(root.join("store.version"), version_file_bytes(1))
        .unwrap_or_else(|error| panic!("rewind version: {error}"));
    let mut store = PrincipalStore::open(&root, retention_uniform(1_000), digest)
        .unwrap_or_else(|error| panic!("reopen: {error}"));
    let scope = store
        .principal(&principal("alice"))
        .unwrap_or_else(|error| panic!("scope: {error}"));
    assert!(scope.get(Table::Journeys, &row_key("journey-1")).is_some());
    let version_bytes = fs::read(root.join("store.version"))
        .unwrap_or_else(|error| panic!("read version: {error}"));
    assert_eq!(version_bytes, version_file_bytes(2));
}

#[test]
fn unknown_versions_and_corrupt_v1_rows_refuse_migration() {
    let root = directory("migrate-refuse");
    let map = tenancy(&[("alice", "tenant-a")]);
    let digest = map
        .install(&root)
        .unwrap_or_else(|error| panic!("install: {error}"));
    fs::write(root.join("store.version"), version_file_bytes(0))
        .unwrap_or_else(|error| panic!("write version: {error}"));
    let unknown = PrincipalStore::open(&root, retention_uniform(1_000), digest);
    assert!(matches!(
        unknown,
        Err(StoreError::Migration(MigrationError::UnknownVersion(0)))
    ));
    fs::write(root.join("store.version"), version_file_bytes(1))
        .unwrap_or_else(|error| panic!("write version: {error}"));
    let alice_dir = root.join("principals").join("alice");
    fs::create_dir_all(&alice_dir).unwrap_or_else(|error| panic!("mkdir: {error}"));
    fs::write(
        alice_dir.join("store.bin"),
        v1_store_file(&[(9, "row-1", 1, b"stray".as_slice())]),
    )
    .unwrap_or_else(|error| panic!("write file: {error}"));
    let corrupt = PrincipalStore::open(&root, retention_uniform(1_000), digest);
    assert!(matches!(
        corrupt,
        Err(StoreError::Migration(MigrationError::CorruptStore(_)))
    ));
}

#[test]
fn foreign_entries_in_the_principals_tree_are_refused() {
    let root = directory("foreign-tree");
    let map = tenancy(&[("alice", "tenant-a")]);
    let (store, digest) = install_and_open(&root, &map, retention_uniform(1_000));
    drop(store);
    let evil_dir = root.join("principals").join("EVIL");
    fs::create_dir_all(&evil_dir).unwrap_or_else(|error| panic!("mkdir: {error}"));
    let refused = PrincipalStore::open(&root, retention_uniform(1_000), digest);
    assert!(matches!(refused, Err(StoreError::Corrupt(_))));
    fs::remove_dir(&evil_dir).unwrap_or_else(|error| panic!("rmdir: {error}"));
    fs::write(root.join("principals").join("stray"), b"x")
        .unwrap_or_else(|error| panic!("write: {error}"));
    let refused = PrincipalStore::open(&root, retention_uniform(1_000), digest);
    assert!(matches!(refused, Err(StoreError::Corrupt(_))));
}

#[test]
fn expiry_never_deletes_evidence_referenced_by_exportable_audit() {
    let root = directory("evidence-pins");
    let map = tenancy(&[("alice", "tenant-a")]);
    let (mut store, digest) = install_and_open(&root, &map, retention_uniform(100));
    let alice = principal("alice");
    let pinned_journey = row_key("journey-pinned");
    let doomed_journey = row_key("journey-doomed");
    let pinned_notice = row_key("notice-pinned");
    {
        let mut scope = store
            .principal(&alice)
            .unwrap_or_else(|error| panic!("scope: {error}"));
        for (table, key) in [
            (Table::Journeys, &pinned_journey),
            (Table::Journeys, &doomed_journey),
            (Table::Notifications, &pinned_notice),
            (Table::Telemetry, &row_key("sample-1")),
            (Table::Cache, &row_key("view-1")),
        ] {
            scope
                .put(table, key.clone(), 0, b"row".to_vec())
                .unwrap_or_else(|error| panic!("put: {error}"));
        }
        scope
            .append_audit(
                row_key("audit-int"),
                0,
                b"int".to_vec(),
                AuditDisposition::Internal,
            )
            .unwrap_or_else(|error| panic!("audit: {error}"));
        scope
            .append_audit(
                row_key("audit-exp"),
                0,
                b"exp".to_vec(),
                AuditDisposition::Exportable {
                    evidence: vec![
                        EvidenceRef::new(Table::Journeys, pinned_journey.clone()),
                        EvidenceRef::new(Table::Notifications, pinned_notice.clone()),
                    ],
                },
            )
            .unwrap_or_else(|error| panic!("audit: {error}"));
        let report = scope
            .expire(1_000)
            .unwrap_or_else(|error| panic!("expire: {error}"));
        assert_eq!(report.expired_rows, 3);
        assert_eq!(report.expired_audit, 1);
        assert_eq!(report.pinned_evidence_retained, 2);
        assert_eq!(report.exportable_audit_retained, 1);
        assert!(scope.get(Table::Journeys, &pinned_journey).is_some());
        assert!(scope.get(Table::Notifications, &pinned_notice).is_some());
        assert!(scope.get(Table::Journeys, &doomed_journey).is_none());
        assert!(scope.audit(&row_key("audit-int")).is_none());
        let exportable = scope
            .audit(&row_key("audit-exp"))
            .unwrap_or_else(|| panic!("exportable audit lost"));
        assert!(exportable.is_exportable());
        let pinned_removal = scope.remove(Table::Journeys, &pinned_journey);
        assert!(matches!(pinned_removal, Err(StoreError::EvidencePinned)));
        let pinned_overwrite =
            scope.put(Table::Journeys, pinned_journey.clone(), 5, b"new".to_vec());
        assert!(matches!(pinned_overwrite, Err(StoreError::EvidencePinned)));
    }
    drop(store);
    let mut store = PrincipalStore::open(&root, retention_uniform(100), digest)
        .unwrap_or_else(|error| panic!("reopen: {error}"));
    let scope = store
        .principal(&alice)
        .unwrap_or_else(|error| panic!("scope: {error}"));
    assert!(scope.get(Table::Journeys, &pinned_journey).is_some());
    assert!(scope.get(Table::Journeys, &doomed_journey).is_none());
}

#[test]
fn evidence_references_resolve_only_inside_their_own_scope() {
    let root = directory("evidence-scope");
    let map = tenancy(&[("alice", "tenant-a"), ("bob", "tenant-b")]);
    let (mut store, _digest) = install_and_open(&root, &map, retention_uniform(10));
    let exhibit = row_key("exhibit-1");
    {
        let mut scope = store
            .principal(&principal("alice"))
            .unwrap_or_else(|error| panic!("scope: {error}"));
        scope
            .put(
                Table::Journeys,
                exhibit.clone(),
                0,
                b"alice-exhibit".to_vec(),
            )
            .unwrap_or_else(|error| panic!("put: {error}"));
    }
    {
        let mut scope = store
            .principal(&principal("bob"))
            .unwrap_or_else(|error| panic!("scope: {error}"));
        let dangling = scope.append_audit(
            row_key("audit-1"),
            0,
            b"claim".to_vec(),
            AuditDisposition::Exportable {
                evidence: vec![EvidenceRef::new(Table::Journeys, exhibit.clone())],
            },
        );
        assert!(matches!(dangling, Err(StoreError::MissingEvidence)));
        scope
            .put(Table::Journeys, exhibit.clone(), 0, b"bob-exhibit".to_vec())
            .unwrap_or_else(|error| panic!("put: {error}"));
        scope
            .append_audit(
                row_key("audit-1"),
                0,
                b"claim".to_vec(),
                AuditDisposition::Exportable {
                    evidence: vec![EvidenceRef::new(Table::Journeys, exhibit.clone())],
                },
            )
            .unwrap_or_else(|error| panic!("audit: {error}"));
    }
    {
        let mut scope = store
            .principal(&principal("alice"))
            .unwrap_or_else(|error| panic!("scope: {error}"));
        let report = scope
            .expire(1_000)
            .unwrap_or_else(|error| panic!("expire: {error}"));
        assert_eq!(report.expired_rows, 1);
        assert!(scope.get(Table::Journeys, &exhibit).is_none());
    }
    let mut scope = store
        .principal(&principal("bob"))
        .unwrap_or_else(|error| panic!("scope: {error}"));
    let report = scope
        .expire(1_000)
        .unwrap_or_else(|error| panic!("expire: {error}"));
    assert_eq!(report.pinned_evidence_retained, 1);
    assert!(scope.get(Table::Journeys, &exhibit).is_some());
}

#[test]
fn audit_entries_are_append_only() {
    let root = directory("audit-append");
    let map = tenancy(&[("alice", "tenant-a")]);
    let (mut store, _digest) = install_and_open(&root, &map, retention_uniform(1_000));
    let mut scope = store
        .principal(&principal("alice"))
        .unwrap_or_else(|error| panic!("scope: {error}"));
    scope
        .append_audit(
            row_key("audit-1"),
            1,
            b"one".to_vec(),
            AuditDisposition::Internal,
        )
        .unwrap_or_else(|error| panic!("audit: {error}"));
    let duplicate = scope.append_audit(
        row_key("audit-1"),
        2,
        b"two".to_vec(),
        AuditDisposition::Internal,
    );
    assert!(matches!(duplicate, Err(StoreError::DuplicateAuditEntry)));
    let entry = scope
        .audit(&row_key("audit-1"))
        .unwrap_or_else(|| panic!("entry lost"));
    assert_eq!(entry.bytes(), b"one");
    assert_eq!(entry.written_at(), 1);
}
