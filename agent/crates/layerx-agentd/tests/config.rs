use std::collections::BTreeMap;
use std::fs;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};

use layerx_agentd::config::{
    load, validate, ConfigError, RejectionReason, SECURITY_RELEVANT_SETTINGS,
};
use layerx_types::verify::VerificationLevel;

static NEXT_FILE: AtomicU64 = AtomicU64::new(1);

fn path(label: &str) -> PathBuf {
    let sequence = NEXT_FILE.fetch_add(1, Ordering::Relaxed);
    std::env::temp_dir().join(format!(
        "layerx-agentd-config-{label}-{}-{sequence}.kv",
        std::process::id()
    ))
}

fn values() -> BTreeMap<String, String> {
    BTreeMap::from([
        ("network_id".to_owned(), "42".to_owned()),
        (
            "node_endpoint".to_owned(),
            "/run/layerx/layerxd.sock".to_owned(),
        ),
        ("expected_protocol_version".to_owned(), "1".to_owned()),
        ("tenants".to_owned(), "tenant-a,tenant-b".to_owned()),
        (
            "policy_sources".to_owned(),
            "tenant-a:/etc/layerx/policy-a.kvx,tenant-b:/etc/layerx/policy-b.kvx".to_owned(),
        ),
        (
            "signer_configurations".to_owned(),
            "tenant-a:/etc/layerx/signer-a.kvx,tenant-b:/etc/layerx/signer-b.kvx".to_owned(),
        ),
        (
            "verification_defaults".to_owned(),
            "tenant-a:state-proven,tenant-b:checkpoint-finalised".to_owned(),
        ),
        (
            "sequencer_authority_source".to_owned(),
            "/etc/layerx/sequencer-authorities.csv".to_owned(),
        ),
    ])
}

fn source(values: &BTreeMap<String, String>) -> String {
    let mut rendered = String::new();
    for (key, value) in values {
        rendered.push_str(key);
        rendered.push('=');
        rendered.push_str(value);
        rendered.push('\n');
    }
    rendered
}

#[test]
fn explicit_environment_precedence_produces_one_fully_typed_configuration() {
    let path = path("precedence");
    fs::write(&path, source(&values())).unwrap_or_else(|error| panic!("write: {error}"));
    let environment = BTreeMap::from([
        (
            "LAYERX_NODE_ENDPOINT".to_owned(),
            "/run/layerx/override.sock".to_owned(),
        ),
        ("PATH".to_owned(), "/usr/bin".to_owned()),
    ]);
    let loaded = load(&path, &environment).unwrap_or_else(|error| panic!("load: {error}"));
    assert_eq!(loaded.network_id, 42);
    assert_eq!(
        loaded.node_endpoint,
        PathBuf::from("/run/layerx/override.sock")
    );
    assert_eq!(loaded.expected_protocol_version, 1);
    assert_eq!(loaded.tenants.len(), 2);
    assert!(loaded
        .verification_defaults
        .values()
        .any(|level| *level == VerificationLevel::STATE_PROVEN));
    assert_eq!(loaded.policy_sources.len(), loaded.tenants.len());
    assert_eq!(loaded.signer_configurations.len(), loaded.tenants.len());
    assert_eq!(
        loaded.sequencer_authority_source,
        PathBuf::from("/etc/layerx/sequencer-authorities.csv")
    );
    let _ = fs::remove_file(path);
}

#[test]
fn every_security_relevant_setting_is_required_without_a_default() {
    assert_eq!(SECURITY_RELEVANT_SETTINGS.len(), 8);
    for setting in SECURITY_RELEVANT_SETTINGS {
        let mut incomplete = values();
        incomplete.remove(setting.file_key);
        assert_eq!(
            validate(&incomplete),
            Err(ConfigError {
                setting: setting.file_key.to_owned(),
                reason: RejectionReason::Missing,
            })
        );
    }
}

#[test]
fn duplicate_file_setting_and_unknown_environment_setting_are_refused() {
    let path = path("ambiguous");
    let mut duplicated = source(&values());
    duplicated.push_str("network_id=43\n");
    fs::write(&path, duplicated).unwrap_or_else(|error| panic!("write: {error}"));
    assert_eq!(
        load(&path, &BTreeMap::new()),
        Err(ConfigError {
            setting: "network_id".to_owned(),
            reason: RejectionReason::Duplicate,
        })
    );
    fs::write(&path, source(&values())).unwrap_or_else(|error| panic!("rewrite: {error}"));
    assert_eq!(
        load(
            &path,
            &BTreeMap::from([("LAYERX_NETWORK".to_owned(), "42".to_owned())])
        ),
        Err(ConfigError {
            setting: "LAYERX_NETWORK".to_owned(),
            reason: RejectionReason::Unknown,
        })
    );
    let _ = fs::remove_file(path);
}

#[test]
fn unsafe_paths_protocols_maps_and_blank_overrides_name_the_setting() {
    let cases = [
        ("network_id", "0", RejectionReason::InvalidInteger),
        (
            "node_endpoint",
            "relative.sock",
            RejectionReason::InvalidPath,
        ),
        (
            "expected_protocol_version",
            "2",
            RejectionReason::UnsupportedProtocol,
        ),
        (
            "policy_sources",
            "tenant-a:/etc/layerx/a.kvx",
            RejectionReason::IncompleteTenantMap,
        ),
        (
            "signer_configurations",
            "tenant-a:/etc/layerx/a.kvx,tenant-c:/etc/layerx/c.kvx",
            RejectionReason::IncompleteTenantMap,
        ),
        (
            "verification_defaults",
            "tenant-a:unverified,tenant-b:state-proven",
            RejectionReason::InvalidVerificationLevel,
        ),
        (
            "sequencer_authority_source",
            "relative.csv",
            RejectionReason::InvalidPath,
        ),
    ];
    for (setting, value, reason) in cases {
        let mut invalid = values();
        invalid.insert(setting.to_owned(), value.to_owned());
        assert_eq!(
            validate(&invalid),
            Err(ConfigError {
                setting: setting.to_owned(),
                reason,
            })
        );
    }

    let path = path("blank-override");
    fs::write(&path, source(&values())).unwrap_or_else(|error| panic!("write: {error}"));
    assert_eq!(
        load(
            &path,
            &BTreeMap::from([("LAYERX_NETWORK_ID".to_owned(), String::new())])
        ),
        Err(ConfigError {
            setting: "network_id".to_owned(),
            reason: RejectionReason::Empty,
        })
    );
    let _ = fs::remove_file(path);
}
