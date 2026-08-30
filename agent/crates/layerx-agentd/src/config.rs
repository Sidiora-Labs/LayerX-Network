//! Fail-closed startup configuration.
//!
//! Configuration is loaded from one explicitly named UTF-8 file containing `key=value`
//! declarations. Exact `LAYERX_*` environment keys override the corresponding file values.
//! Duplicate file declarations, unknown `LayerX` settings, blank overrides, and incomplete
//! per-tenant maps are errors; no security-relevant value has a default.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt::{Display, Formatter};
use std::fs::{self, File, Metadata};
use std::io::Read;
use std::os::unix::fs::MetadataExt;
use std::path::{Component, Path, PathBuf};

use layerx_types::verify::VerificationLevel;
use layerx_wire::limits::PROTOCOL_VERSION;

use crate::store::TenantId;

const MAX_CONFIG_BYTES: usize = 65_536;
const MAX_LINE_BYTES: usize = 4_096;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct RequiredSetting {
    pub file_key: &'static str,
    pub environment_key: &'static str,
}

pub const SECURITY_RELEVANT_SETTINGS: [RequiredSetting; 8] = [
    RequiredSetting {
        file_key: "network_id",
        environment_key: "LAYERX_NETWORK_ID",
    },
    RequiredSetting {
        file_key: "node_endpoint",
        environment_key: "LAYERX_NODE_ENDPOINT",
    },
    RequiredSetting {
        file_key: "expected_protocol_version",
        environment_key: "LAYERX_EXPECTED_PROTOCOL_VERSION",
    },
    RequiredSetting {
        file_key: "tenants",
        environment_key: "LAYERX_TENANTS",
    },
    RequiredSetting {
        file_key: "policy_sources",
        environment_key: "LAYERX_POLICY_SOURCES",
    },
    RequiredSetting {
        file_key: "signer_configurations",
        environment_key: "LAYERX_SIGNER_CONFIGURATIONS",
    },
    RequiredSetting {
        file_key: "verification_defaults",
        environment_key: "LAYERX_VERIFICATION_DEFAULTS",
    },
    RequiredSetting {
        file_key: "sequencer_authority_source",
        environment_key: "LAYERX_SEQUENCER_AUTHORITY_SOURCE",
    },
];

pub const PRECEDENCE: &str =
    "exact LAYERX_* environment values override the explicitly named configuration file";

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct StartupConfig {
    pub network_id: u32,
    pub node_endpoint: PathBuf,
    pub expected_protocol_version: u16,
    pub tenants: BTreeSet<TenantId>,
    pub policy_sources: BTreeMap<TenantId, PathBuf>,
    pub signer_configurations: BTreeMap<TenantId, PathBuf>,
    pub verification_defaults: BTreeMap<TenantId, VerificationLevel>,
    pub sequencer_authority_source: PathBuf,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RejectionReason {
    Missing,
    Empty,
    Duplicate,
    Unknown,
    InvalidInteger,
    UnsupportedProtocol,
    InvalidTenant,
    InvalidPath,
    IncompleteTenantMap,
    InvalidVerificationLevel,
    TooLarge,
    InvalidEncoding,
    Unavailable,
    Unprotected,
}

impl Display for RejectionReason {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        let reason = match self {
            Self::Missing => "is required and was not set",
            Self::Empty => "must not be empty",
            Self::Duplicate => "was declared more than once",
            Self::Unknown => "is not a recognised startup setting",
            Self::InvalidInteger => "is not a valid non-zero integer",
            Self::UnsupportedProtocol => "is not supported by the compiled wire codec",
            Self::InvalidTenant => "contains an invalid or duplicate tenant",
            Self::InvalidPath => "must be an absolute normalised path",
            Self::IncompleteTenantMap => "must contain exactly one value for every tenant",
            Self::InvalidVerificationLevel => "contains an unsafe verification default",
            Self::TooLarge => "exceeds the configured input bound",
            Self::InvalidEncoding => "is not valid UTF-8",
            Self::Unavailable => "could not be read",
            Self::Unprotected => "is not a protected regular file owned by this process user",
        };
        formatter.write_str(reason)
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ConfigError {
    pub setting: String,
    pub reason: RejectionReason,
}

impl Display for ConfigError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        write!(
            formatter,
            "configuration setting {} {}",
            self.setting, self.reason
        )
    }
}

impl std::error::Error for ConfigError {}

/// Loads, resolves, and validates startup configuration without consulting ambient defaults.
///
/// `environment` is an explicit snapshot supplied by the process entry point. Non-LayerX
/// variables are ignored, while an unknown `LAYERX_*` variable is rejected as a likely typo.
///
/// # Errors
///
/// Returns the exact setting and rejection reason for file I/O, syntax, precedence, or
/// semantic validation failures.
pub fn load(
    file: impl AsRef<Path>,
    environment: &BTreeMap<String, String>,
) -> Result<StartupConfig, ConfigError> {
    let bytes = read_protected_source(file.as_ref(), MAX_CONFIG_BYTES).map_err(|failure| {
        error(
            "configuration_file",
            match failure {
                ProtectedSourceError::Unavailable => RejectionReason::Unavailable,
                ProtectedSourceError::TooLarge => RejectionReason::TooLarge,
                ProtectedSourceError::Unprotected | ProtectedSourceError::Changed => {
                    RejectionReason::Unprotected
                }
            },
        )
    })?;
    let source = std::str::from_utf8(&bytes)
        .map_err(|_| error("configuration_file", RejectionReason::InvalidEncoding))?;
    let mut values = parse_file(source)?;
    for (name, value) in environment {
        if let Some(setting) = SECURITY_RELEVANT_SETTINGS
            .iter()
            .find(|setting| setting.environment_key == name)
        {
            values.insert(setting.file_key.to_owned(), value.clone());
        } else if name.starts_with("LAYERX_") {
            return Err(error(name, RejectionReason::Unknown));
        }
    }
    validate(&values)
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ProtectedSourceError {
    Unavailable,
    Unprotected,
    Changed,
    TooLarge,
}

/// Reads one operator-trusted source without accepting indirection or mutable
/// file metadata at the read boundary.
pub fn read_protected_source(
    path: &Path,
    maximum_bytes: usize,
) -> Result<Vec<u8>, ProtectedSourceError> {
    if !path.is_absolute() {
        return Err(ProtectedSourceError::Unprotected);
    }
    let canonical = fs::canonicalize(path).map_err(|_| ProtectedSourceError::Unavailable)?;
    if canonical != path {
        return Err(ProtectedSourceError::Unprotected);
    }
    let process_uid = fs::metadata("/proc/self")
        .map_err(|_| ProtectedSourceError::Unavailable)?
        .uid();
    let path_before = fs::symlink_metadata(path).map_err(|_| ProtectedSourceError::Unavailable)?;
    validate_protected_metadata(&path_before, process_uid)?;

    let mut file = File::open(path).map_err(|_| ProtectedSourceError::Unavailable)?;
    let file_before = file
        .metadata()
        .map_err(|_| ProtectedSourceError::Unavailable)?;
    validate_protected_metadata(&file_before, process_uid)?;
    if !same_file_snapshot(&path_before, &file_before) {
        return Err(ProtectedSourceError::Changed);
    }

    let limit = u64::try_from(maximum_bytes)
        .ok()
        .and_then(|value| value.checked_add(1))
        .ok_or(ProtectedSourceError::TooLarge)?;
    let mut bytes = Vec::new();
    file.by_ref()
        .take(limit)
        .read_to_end(&mut bytes)
        .map_err(|_| ProtectedSourceError::Unavailable)?;

    let file_after = file
        .metadata()
        .map_err(|_| ProtectedSourceError::Unavailable)?;
    let path_after = fs::symlink_metadata(path).map_err(|_| ProtectedSourceError::Unavailable)?;
    if fs::canonicalize(path).map_err(|_| ProtectedSourceError::Changed)? != path
        || !same_file_snapshot(&file_before, &file_after)
        || !same_file_snapshot(&file_after, &path_after)
    {
        return Err(ProtectedSourceError::Changed);
    }
    if bytes.len() > maximum_bytes {
        return Err(ProtectedSourceError::TooLarge);
    }
    Ok(bytes)
}

fn validate_protected_metadata(
    metadata: &Metadata,
    process_uid: u32,
) -> Result<(), ProtectedSourceError> {
    if !metadata.file_type().is_file()
        || metadata.uid() != process_uid
        || metadata.mode() & 0o077 != 0
    {
        return Err(ProtectedSourceError::Unprotected);
    }
    Ok(())
}

fn same_file_snapshot(left: &Metadata, right: &Metadata) -> bool {
    left.dev() == right.dev()
        && left.ino() == right.ino()
        && left.uid() == right.uid()
        && left.gid() == right.gid()
        && left.mode() == right.mode()
        && left.nlink() == right.nlink()
        && left.size() == right.size()
        && left.mtime() == right.mtime()
        && left.mtime_nsec() == right.mtime_nsec()
        && left.ctime() == right.ctime()
        && left.ctime_nsec() == right.ctime_nsec()
}

/// Validates a fully resolved map of file-key names into a typed startup configuration.
///
/// # Errors
///
/// Returns the exact missing, invalid, ambiguous, or unsafe setting without substituting a
/// default.
pub fn validate(values: &BTreeMap<String, String>) -> Result<StartupConfig, ConfigError> {
    for key in values.keys() {
        if !SECURITY_RELEVANT_SETTINGS
            .iter()
            .any(|setting| setting.file_key == key)
        {
            return Err(error(key, RejectionReason::Unknown));
        }
    }
    for setting in SECURITY_RELEVANT_SETTINGS {
        let value = values
            .get(setting.file_key)
            .ok_or_else(|| error(setting.file_key, RejectionReason::Missing))?;
        if value.trim().is_empty() {
            return Err(error(setting.file_key, RejectionReason::Empty));
        }
    }

    let network_id = parse_nonzero::<u32>(values, "network_id")?;
    let expected_protocol_version = parse_nonzero::<u16>(values, "expected_protocol_version")?;
    if expected_protocol_version != PROTOCOL_VERSION {
        return Err(error(
            "expected_protocol_version",
            RejectionReason::UnsupportedProtocol,
        ));
    }
    let node_endpoint = parse_path(required(values, "node_endpoint")?, "node_endpoint")?;
    let tenants = parse_tenants(required(values, "tenants")?)?;
    let policy_sources = parse_tenant_paths(
        required(values, "policy_sources")?,
        "policy_sources",
        &tenants,
    )?;
    let signer_configurations = parse_tenant_paths(
        required(values, "signer_configurations")?,
        "signer_configurations",
        &tenants,
    )?;
    let verification_defaults =
        parse_verification_defaults(required(values, "verification_defaults")?, &tenants)?;
    let sequencer_authority_source = parse_path(
        required(values, "sequencer_authority_source")?,
        "sequencer_authority_source",
    )?;
    Ok(StartupConfig {
        network_id,
        node_endpoint,
        expected_protocol_version,
        tenants,
        policy_sources,
        signer_configurations,
        verification_defaults,
        sequencer_authority_source,
    })
}

fn parse_file(source: &str) -> Result<BTreeMap<String, String>, ConfigError> {
    let mut values = BTreeMap::new();
    for line in source.lines() {
        if line.len() > MAX_LINE_BYTES {
            return Err(error("configuration_file", RejectionReason::TooLarge));
        }
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let (key, value) = line
            .split_once('=')
            .ok_or_else(|| error("configuration_file", RejectionReason::Unknown))?;
        let key = key.trim();
        if !SECURITY_RELEVANT_SETTINGS
            .iter()
            .any(|setting| setting.file_key == key)
        {
            return Err(error(key, RejectionReason::Unknown));
        }
        if values
            .insert(key.to_owned(), value.trim().to_owned())
            .is_some()
        {
            return Err(error(key, RejectionReason::Duplicate));
        }
    }
    Ok(values)
}

fn required<'a>(
    values: &'a BTreeMap<String, String>,
    setting: &'static str,
) -> Result<&'a str, ConfigError> {
    values
        .get(setting)
        .map(String::as_str)
        .ok_or_else(|| error(setting, RejectionReason::Missing))
}

fn parse_nonzero<T>(
    values: &BTreeMap<String, String>,
    setting: &'static str,
) -> Result<T, ConfigError>
where
    T: std::str::FromStr + PartialEq + Default,
{
    let value = required(values, setting)?
        .parse::<T>()
        .map_err(|_| error(setting, RejectionReason::InvalidInteger))?;
    if value == T::default() {
        Err(error(setting, RejectionReason::InvalidInteger))
    } else {
        Ok(value)
    }
}

fn parse_tenants(value: &str) -> Result<BTreeSet<TenantId>, ConfigError> {
    let mut tenants = BTreeSet::new();
    for raw in value.split(',') {
        let canonical = raw.trim();
        if !valid_tenant(canonical) {
            return Err(error("tenants", RejectionReason::InvalidTenant));
        }
        let tenant = TenantId::new(canonical)
            .map_err(|_| error("tenants", RejectionReason::InvalidTenant))?;
        if !tenants.insert(tenant) {
            return Err(error("tenants", RejectionReason::InvalidTenant));
        }
    }
    if tenants.is_empty() {
        Err(error("tenants", RejectionReason::InvalidTenant))
    } else {
        Ok(tenants)
    }
}

fn parse_tenant_paths(
    value: &str,
    setting: &'static str,
    tenants: &BTreeSet<TenantId>,
) -> Result<BTreeMap<TenantId, PathBuf>, ConfigError> {
    let mut paths = BTreeMap::new();
    for declaration in value.split(',') {
        let (tenant_text, path_text) = declaration
            .split_once(':')
            .ok_or_else(|| error(setting, RejectionReason::IncompleteTenantMap))?;
        let tenant_text = tenant_text.trim();
        if !valid_tenant(tenant_text) {
            return Err(error(setting, RejectionReason::InvalidTenant));
        }
        let tenant = TenantId::new(tenant_text)
            .map_err(|_| error(setting, RejectionReason::InvalidTenant))?;
        if !tenants.contains(&tenant) || paths.contains_key(&tenant) {
            return Err(error(setting, RejectionReason::IncompleteTenantMap));
        }
        paths.insert(tenant, parse_path(path_text.trim(), setting)?);
    }
    if paths.keys().eq(tenants.iter()) {
        Ok(paths)
    } else {
        Err(error(setting, RejectionReason::IncompleteTenantMap))
    }
}

fn parse_verification_defaults(
    value: &str,
    tenants: &BTreeSet<TenantId>,
) -> Result<BTreeMap<TenantId, VerificationLevel>, ConfigError> {
    let setting = "verification_defaults";
    let mut levels = BTreeMap::new();
    for declaration in value.split(',') {
        let (tenant_text, level_text) = declaration
            .split_once(':')
            .ok_or_else(|| error(setting, RejectionReason::IncompleteTenantMap))?;
        let tenant_text = tenant_text.trim();
        if !valid_tenant(tenant_text) {
            return Err(error(setting, RejectionReason::InvalidTenant));
        }
        let tenant = TenantId::new(tenant_text)
            .map_err(|_| error(setting, RejectionReason::InvalidTenant))?;
        if !tenants.contains(&tenant) || levels.contains_key(&tenant) {
            return Err(error(setting, RejectionReason::IncompleteTenantMap));
        }
        let level = match level_text.trim() {
            "sequencer-signed" => VerificationLevel::SEQUENCER_SIGNED,
            "batch-included" => VerificationLevel::BATCH_INCLUDED,
            "state-proven" => VerificationLevel::STATE_PROVEN,
            "checkpoint-finalised" => VerificationLevel::CHECKPOINT_FINALISED,
            "settlement-anchored" => VerificationLevel::SETTLEMENT_ANCHORED,
            _ => return Err(error(setting, RejectionReason::InvalidVerificationLevel)),
        };
        levels.insert(tenant, level);
    }
    if levels.keys().eq(tenants.iter()) {
        Ok(levels)
    } else {
        Err(error(setting, RejectionReason::IncompleteTenantMap))
    }
}

fn parse_path(value: &str, setting: &'static str) -> Result<PathBuf, ConfigError> {
    let path = PathBuf::from(value);
    if value
        .bytes()
        .any(|byte| byte == 0 || byte.is_ascii_control())
        || !path.is_absolute()
        || path == Path::new("/")
        || path
            .components()
            .any(|component| matches!(component, Component::ParentDir | Component::CurDir))
    {
        Err(error(setting, RejectionReason::InvalidPath))
    } else {
        Ok(path)
    }
}

fn valid_tenant(value: &str) -> bool {
    !value.is_empty()
        && value.bytes().all(|byte| {
            byte.is_ascii_lowercase() || byte.is_ascii_digit() || matches!(byte, b'-' | b'_' | b'.')
        })
}

fn error(setting: impl Into<String>, reason: RejectionReason) -> ConfigError {
    ConfigError {
        setting: setting.into(),
        reason,
    }
}
