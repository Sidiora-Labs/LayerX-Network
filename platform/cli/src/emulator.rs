use std::fmt;
use std::fs::{self, DirBuilder, File, OpenOptions};
use std::io::{ErrorKind, Write as _};
use std::net::{SocketAddr, TcpStream, ToSocketAddrs as _};
use std::path::{Path, PathBuf};
use std::thread;
use std::time::{Duration, Instant};

use ed25519_dalek::{SigningKey, VerifyingKey};
use serde_json::{json, Value};
use zeroize::Zeroizing;

use crate::encoding::{fixed_hex, hex_encode};
use crate::http::Client;
use crate::install;

pub const PROFILE_SUBDIRECTORY: &str = "emulator";
pub const SEED_FILE: &str = "sequencer.seed";
pub const ANCHOR_FILE: &str = "sequencer.anchor";
pub const IDENTITY_ROUTE: &str = "/v1/sequencer";
pub const ENDPOINT_INPUT: &str = "--endpoint";
pub const NETWORK_ID_INPUT: &str = "--network-id";
pub const ANCHOR_INPUT: &str = "--sequencer-trust-anchor";
pub const ANCHOR_FILE_INPUT: &str = "--sequencer-trust-anchor-file";
pub const STORED_ANCHOR_INPUT: &str = "configured sequencer trust anchor";

const IDENTITY_WAIT: Duration = Duration::from_secs(20);
const IDENTITY_POLL: Duration = Duration::from_millis(100);
const CONNECT_TIMEOUT: Duration = Duration::from_secs(2);
const PROVISION_LOCK_FILE: &str = ".emulator-provision.lock";

#[derive(Debug)]
pub enum BootstrapError {
    SeedExists {
        path: PathBuf,
    },
    AnchorExists {
        path: PathBuf,
    },
    ProfileUnavailable {
        path: PathBuf,
        detail: String,
    },
    SeedUnwritable {
        path: PathBuf,
        detail: String,
    },
    AnchorUnwritable {
        path: PathBuf,
        detail: String,
    },
    RandomnessUnavailable {
        detail: String,
    },
    InputMissing {
        missing: Vec<&'static str>,
    },
    AnchorConflict,
    AnchorUnreadable {
        path: PathBuf,
        detail: String,
    },
    AnchorEmpty {
        input: &'static str,
    },
    AnchorMalformed {
        input: &'static str,
        detail: String,
    },
    AnchorUnbound {
        name: String,
    },
    EnvironmentUnconfigured {
        name: String,
    },
    NetworkIdReserved,
    IdentityUnavailable {
        endpoint: String,
        detail: String,
    },
    IdentityMalformed {
        endpoint: String,
        detail: String,
    },
    NetworkIdMismatch {
        supplied: u32,
        advertised: u32,
    },
    AnchorMismatch {
        input: &'static str,
        supplied: String,
        advertised: String,
    },
}

impl BootstrapError {
    #[must_use]
    pub const fn code(&self) -> &'static str {
        match self {
            Self::SeedExists { .. } => "sequencer_seed_exists",
            Self::AnchorExists { .. } => "sequencer_trust_anchor_exists",
            Self::ProfileUnavailable { .. } => "profile_directory_unavailable",
            Self::SeedUnwritable { .. } => "sequencer_seed_unwritable",
            Self::AnchorUnwritable { .. } => "sequencer_trust_anchor_unwritable",
            Self::RandomnessUnavailable { .. } => "randomness_unavailable",
            Self::InputMissing { .. } => "environment_input_missing",
            Self::AnchorConflict => "sequencer_trust_anchor_conflict",
            Self::AnchorUnreadable { .. } => "sequencer_trust_anchor_unreadable",
            Self::AnchorEmpty { .. } => "sequencer_trust_anchor_empty",
            Self::AnchorMalformed { .. } => "sequencer_trust_anchor_malformed",
            Self::AnchorUnbound { .. } => "sequencer_trust_anchor_unbound",
            Self::EnvironmentUnconfigured { .. } => "environment_unconfigured",
            Self::NetworkIdReserved => "network_id_reserved",
            Self::IdentityUnavailable { .. } => "sequencer_identity_unavailable",
            Self::IdentityMalformed { .. } => "sequencer_identity_malformed",
            Self::NetworkIdMismatch { .. } => "network_id_mismatch",
            Self::AnchorMismatch { .. } => "sequencer_trust_anchor_mismatch",
        }
    }
}

impl fmt::Display for BootstrapError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "{}: ", self.code())?;
        match self {
            Self::SeedExists { path } => write!(
                formatter,
                "{} already holds a sequencer seed; pass --force to replace it",
                path.display()
            ),
            Self::AnchorExists { path } => write!(
                formatter,
                "{} already holds a sequencer trust anchor; pass --force to replace it",
                path.display()
            ),
            Self::ProfileUnavailable { path, detail } => {
                write!(formatter, "could not prepare {}: {detail}", path.display())
            }
            Self::SeedUnwritable { path, detail } => write!(
                formatter,
                "could not write the sequencer seed to {}: {detail}",
                path.display()
            ),
            Self::AnchorUnwritable { path, detail } => write!(
                formatter,
                "could not write the sequencer trust anchor to {}: {detail}",
                path.display()
            ),
            Self::RandomnessUnavailable { detail } => {
                write!(formatter, "operating-system randomness failed: {detail}")
            }
            Self::InputMissing { missing } => write!(
                formatter,
                "{} required; {ENDPOINT_INPUT}, {NETWORK_ID_INPUT} and a sequencer trust anchor ({ANCHOR_FILE_INPUT} or {ANCHOR_INPUT}) must be supplied together",
                describe_missing(missing)
            ),
            Self::AnchorConflict => write!(
                formatter,
                "{ANCHOR_FILE_INPUT} and {ANCHOR_INPUT} cannot both be supplied"
            ),
            Self::AnchorUnreadable { path, detail } => write!(
                formatter,
                "could not read the sequencer trust anchor from {}: {detail}",
                path.display()
            ),
            Self::AnchorEmpty { input } => {
                write!(formatter, "{input} supplied an empty sequencer trust anchor")
            }
            Self::AnchorMalformed { input, detail } => {
                write!(formatter, "{input} is not a sequencer trust anchor: {detail}")
            }
            Self::AnchorUnbound { name } => write!(
                formatter,
                "environment {name} has no bound sequencer trust anchor; supply {ENDPOINT_INPUT}, {NETWORK_ID_INPUT} and {ANCHOR_FILE_INPUT}"
            ),
            Self::EnvironmentUnconfigured { name } => write!(
                formatter,
                "environment {name} is not configured; supply {ENDPOINT_INPUT}, {NETWORK_ID_INPUT} and {ANCHOR_FILE_INPUT}"
            ),
            Self::NetworkIdReserved => write!(formatter, "{NETWORK_ID_INPUT} zero is reserved"),
            Self::IdentityUnavailable { endpoint, detail } => write!(
                formatter,
                "{endpoint} did not advertise a sequencer identity: {detail}"
            ),
            Self::IdentityMalformed { endpoint, detail } => write!(
                formatter,
                "{endpoint} advertised a malformed sequencer identity: {detail}"
            ),
            Self::NetworkIdMismatch {
                supplied,
                advertised,
            } => write!(
                formatter,
                "{NETWORK_ID_INPUT} {supplied} disagrees with the network id {advertised} the endpoint advertises"
            ),
            Self::AnchorMismatch {
                input,
                supplied,
                advertised,
            } => write!(
                formatter,
                "{input} {supplied} disagrees with the sequencer identity {advertised} the endpoint advertises"
            ),
        }
    }
}

impl From<BootstrapError> for String {
    fn from(error: BootstrapError) -> Self {
        error.to_string()
    }
}

fn describe_missing(missing: &[&str]) -> String {
    match missing {
        [] => "an input is".to_owned(),
        [only] => format!("{only} is"),
        [first, second] => format!("{first} and {second} are"),
        [head @ .., last] => format!("{} and {last} are", head.join(", ")),
    }
}

pub fn provision(force: bool) -> Result<Value, String> {
    let profile = install::layerx_directory()?;
    validate_profile_path(&profile)?;
    let directory = profile.join(PROFILE_SUBDIRECTORY);
    let seed_file = directory.join(SEED_FILE);
    let anchor_file = directory.join(ANCHOR_FILE);
    if !atomic_directory_exchange_supported() {
        return Err(BootstrapError::ProfileUnavailable {
            path: directory,
            detail: "atomic owner-only identity publication is unsupported on this platform".into(),
        }
        .into());
    }
    install::validate_existing_ancestors(&seed_file, false).map_err(|detail| {
        BootstrapError::ProfileUnavailable {
            path: directory.clone(),
            detail,
        }
    })?;
    ensure_profile_directory(&profile)?;
    let _provision_lock = lock_profile(&profile)?;
    cleanup_stale_identity_directories(&profile)?;
    let target_exists = validate_identity_directory(&directory)?;
    if !force {
        if exists(&seed_file)? {
            return Err(BootstrapError::SeedExists { path: seed_file }.into());
        }
        if exists(&anchor_file)? {
            return Err(BootstrapError::AnchorExists { path: anchor_file }.into());
        }
    }
    let mut seed = Zeroizing::new([0_u8; 32]);
    getrandom::fill(seed.as_mut()).map_err(|error| BootstrapError::RandomnessUnavailable {
        detail: error.to_string(),
    })?;
    if *seed == [0; 32] {
        return Err(BootstrapError::RandomnessUnavailable {
            detail: "returned an all-zero seed".into(),
        }
        .into());
    }
    let anchor = {
        let signing = SigningKey::from_bytes(&seed);
        hex_encode(&signing.verifying_key().to_bytes())
    };
    let encoded = Zeroizing::new(hex_encode(&seed[..]));
    let staging = create_staging_directory(&profile)?;
    let staging_seed = staging.join(SEED_FILE);
    let staging_anchor = staging.join(ANCHOR_FILE);
    let publication = (|| {
        write_private(&staging_seed, encoded.as_bytes()).map_err(|detail| {
            BootstrapError::SeedUnwritable {
                path: seed_file.clone(),
                detail,
            }
        })?;
        write_private(&staging_anchor, anchor.as_bytes()).map_err(|detail| {
            BootstrapError::AnchorUnwritable {
                path: anchor_file.clone(),
                detail,
            }
        })?;
        File::open(&staging)
            .and_then(|staging| staging.sync_all())
            .map_err(|error| BootstrapError::ProfileUnavailable {
                path: staging.clone(),
                detail: format!("could not synchronize staged identity: {error}"),
            })?;
        publish_identity_directory(&profile, &staging, &directory, target_exists)
    })();
    if publication.is_err() && staging.exists() {
        let _ = remove_identity_directory(&staging);
    }
    let warnings = publication?;
    Ok(json!({
        "directory": directory.display().to_string(),
        "sequencer_seed_file": seed_file.display().to_string(),
        "sequencer_trust_anchor_file": anchor_file.display().to_string(),
        "sequencer_trust_anchor": anchor,
        "seed_storage": "profile-directory-owner-only",
        "warnings": warnings,
    }))
}

fn exists(path: &Path) -> Result<bool, BootstrapError> {
    match fs::symlink_metadata(path) {
        Ok(_) => Ok(true),
        Err(error) if error.kind() == ErrorKind::NotFound => Ok(false),
        Err(error) => Err(BootstrapError::ProfileUnavailable {
            path: path.to_path_buf(),
            detail: error.to_string(),
        }),
    }
}

fn validate_profile_path(profile: &Path) -> Result<(), BootstrapError> {
    if profile.parent().is_none() || profile.file_name().is_none() {
        return Err(BootstrapError::ProfileUnavailable {
            path: profile.to_path_buf(),
            detail: "the filesystem root cannot be used as a LayerX profile directory".into(),
        });
    }
    Ok(())
}

fn ensure_profile_directory(directory: &Path) -> Result<(), BootstrapError> {
    let unavailable = |detail: String| BootstrapError::ProfileUnavailable {
        path: directory.to_path_buf(),
        detail,
    };
    let mut builder = DirBuilder::new();
    builder.recursive(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::DirBuilderExt as _;
        builder.mode(0o700);
    }
    builder
        .create(directory)
        .map_err(|error| unavailable(error.to_string()))?;
    let metadata =
        fs::symlink_metadata(directory).map_err(|error| unavailable(error.to_string()))?;
    if metadata.file_type().is_symlink() || !metadata.is_dir() {
        return Err(unavailable("path is not a directory".into()));
    }
    if !owned_by_current_user(&metadata) {
        return Err(unavailable(
            "directory is not owned by the current user".into(),
        ));
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt as _;
        if metadata.mode() & 0o022 != 0 {
            return Err(unavailable(
                "profile directory is writable by another user".into(),
            ));
        }
    }
    Ok(())
}

fn lock_profile(profile: &Path) -> Result<File, BootstrapError> {
    let path = profile.join(PROVISION_LOCK_FILE);
    #[cfg(unix)]
    let file = {
        use rustix::fs::{open, Mode, OFlags};
        File::from(
            open(
                &path,
                OFlags::CREATE | OFlags::RDWR | OFlags::CLOEXEC | OFlags::NOFOLLOW,
                Mode::from_raw_mode(0o600),
            )
            .map_err(|error| BootstrapError::ProfileUnavailable {
                path: path.clone(),
                detail: format!("could not open the provision lock: {error}"),
            })?,
        )
    };
    #[cfg(not(unix))]
    let file = OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .open(&path)
        .map_err(|error| BootstrapError::ProfileUnavailable {
            path: path.clone(),
            detail: format!("could not open the provision lock: {error}"),
        })?;
    let metadata = file
        .metadata()
        .map_err(|error| BootstrapError::ProfileUnavailable {
            path: path.clone(),
            detail: format!("could not inspect the provision lock: {error}"),
        })?;
    if !metadata.is_file() || !owner_only(&metadata) || !owned_by_current_user(&metadata) {
        return Err(BootstrapError::ProfileUnavailable {
            path,
            detail: "provision lock must be an owner-only regular file owned by the current user"
                .into(),
        });
    }
    #[cfg(unix)]
    rustix::fs::flock(&file, rustix::fs::FlockOperation::LockExclusive).map_err(|error| {
        BootstrapError::ProfileUnavailable {
            path: profile.join(PROVISION_LOCK_FILE),
            detail: format!("could not lock emulator provisioning: {error}"),
        }
    })?;
    Ok(file)
}

fn create_staging_directory(profile: &Path) -> Result<PathBuf, BootstrapError> {
    let mut random = [0_u8; 16];
    getrandom::fill(&mut random).map_err(|error| BootstrapError::RandomnessUnavailable {
        detail: error.to_string(),
    })?;
    let staging = profile.join(format!(".emulator-stage-{}", hex_encode(&random)));
    let mut builder = DirBuilder::new();
    #[cfg(unix)]
    {
        use std::os::unix::fs::DirBuilderExt as _;
        builder.mode(0o700);
    }
    builder
        .create(&staging)
        .map_err(|error| BootstrapError::ProfileUnavailable {
            path: staging.clone(),
            detail: format!("could not create the staged identity: {error}"),
        })?;
    let metadata =
        fs::symlink_metadata(&staging).map_err(|error| BootstrapError::ProfileUnavailable {
            path: staging.clone(),
            detail: format!("could not inspect the staged identity: {error}"),
        })?;
    if !metadata.is_dir() || !owner_only(&metadata) || !owned_by_current_user(&metadata) {
        let _ = fs::remove_dir(&staging);
        return Err(BootstrapError::ProfileUnavailable {
            path: staging,
            detail: "staged identity directory is not owner-only".into(),
        });
    }
    Ok(staging)
}

fn cleanup_stale_identity_directories(profile: &Path) -> Result<(), BootstrapError> {
    for entry in fs::read_dir(profile).map_err(|error| BootstrapError::ProfileUnavailable {
        path: profile.to_path_buf(),
        detail: format!("could not inspect stale identity stages: {error}"),
    })? {
        let entry = entry.map_err(|error| BootstrapError::ProfileUnavailable {
            path: profile.to_path_buf(),
            detail: format!("could not inspect a stale identity stage: {error}"),
        })?;
        if !entry
            .file_name()
            .to_string_lossy()
            .starts_with(".emulator-stage-")
        {
            continue;
        }
        remove_identity_directory(&entry.path()).map_err(|detail| {
            BootstrapError::ProfileUnavailable {
                path: entry.path(),
                detail: format!("could not clean a stale identity stage: {detail}"),
            }
        })?;
    }
    Ok(())
}

fn write_private(path: &Path, contents: &[u8]) -> Result<(), String> {
    let mut options = OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt as _;
        options.mode(0o600);
    }
    let mut file = match options.open(path) {
        Ok(file) => file,
        Err(error) => return Err(format!("could not create {}: {error}", path.display())),
    };
    let written = file
        .write_all(contents)
        .and_then(|()| file.write_all(b"\n"))
        .and_then(|()| file.sync_all())
        .map_err(|error| format!("could not write {}: {error}", path.display()));
    if let Err(failure) = written {
        let _ = fs::remove_file(path);
        return Err(failure);
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt as _;
        if let Err(error) = fs::set_permissions(path, fs::Permissions::from_mode(0o600)) {
            let _ = fs::remove_file(path);
            return Err(format!("could not protect {}: {error}", path.display()));
        }
    }
    let metadata = file
        .metadata()
        .map_err(|error| format!("could not inspect {}: {error}", path.display()))?;
    if !metadata.is_file() || !owner_only(&metadata) || !owned_by_current_user(&metadata) {
        return Err(format!(
            "{} is not an owner-only file owned by the current user",
            path.display()
        ));
    }
    Ok(())
}

fn validate_identity_directory(directory: &Path) -> Result<bool, BootstrapError> {
    let metadata = match fs::symlink_metadata(directory) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == ErrorKind::NotFound => return Ok(false),
        Err(error) => {
            return Err(BootstrapError::ProfileUnavailable {
                path: directory.to_path_buf(),
                detail: error.to_string(),
            })
        }
    };
    if metadata.file_type().is_symlink()
        || !metadata.is_dir()
        || !owner_only(&metadata)
        || !owned_by_current_user(&metadata)
    {
        return Err(BootstrapError::ProfileUnavailable {
            path: directory.to_path_buf(),
            detail:
                "identity directory must be owner-only, non-symlink and owned by the current user"
                    .into(),
        });
    }
    for entry in fs::read_dir(directory).map_err(|error| BootstrapError::ProfileUnavailable {
        path: directory.to_path_buf(),
        detail: error.to_string(),
    })? {
        let entry = entry.map_err(|error| BootstrapError::ProfileUnavailable {
            path: directory.to_path_buf(),
            detail: error.to_string(),
        })?;
        let name = entry.file_name();
        if name != SEED_FILE && name != ANCHOR_FILE {
            return Err(BootstrapError::ProfileUnavailable {
                path: entry.path(),
                detail: "unexpected entry in the emulator identity directory".into(),
            });
        }
        let metadata = fs::symlink_metadata(entry.path()).map_err(|error| {
            BootstrapError::ProfileUnavailable {
                path: entry.path(),
                detail: error.to_string(),
            }
        })?;
        if metadata.file_type().is_symlink()
            || !metadata.is_file()
            || !owner_only(&metadata)
            || !owned_by_current_user(&metadata)
        {
            return Err(BootstrapError::ProfileUnavailable {
                path: entry.path(),
                detail:
                    "identity entry must be an owner-only regular file owned by the current user"
                        .into(),
            });
        }
    }
    Ok(true)
}

fn publish_identity_directory(
    profile: &Path,
    staging: &Path,
    directory: &Path,
    target_exists: bool,
) -> Result<Vec<String>, BootstrapError> {
    install::validate_existing_ancestors(directory, false).map_err(|detail| {
        BootstrapError::ProfileUnavailable {
            path: directory.to_path_buf(),
            detail,
        }
    })?;
    validate_identity_directory(staging)?;
    let profile_directory =
        File::open(profile).map_err(|error| BootstrapError::ProfileUnavailable {
            path: profile.to_path_buf(),
            detail: format!("could not open the profile directory: {error}"),
        })?;
    if target_exists {
        validate_identity_directory(directory)?;
        atomic_exchange(staging, directory).map_err(|detail| {
            BootstrapError::ProfileUnavailable {
                path: directory.to_path_buf(),
                detail,
            }
        })?;
    } else {
        atomic_publish(staging, directory).map_err(|detail| {
            BootstrapError::ProfileUnavailable {
                path: directory.to_path_buf(),
                detail,
            }
        })?;
    }
    let mut warnings = Vec::new();
    if let Err(error) = profile_directory.sync_all() {
        let rollback = if target_exists {
            atomic_exchange(staging, directory)
        } else {
            atomic_publish(directory, staging)
        };
        if rollback.is_ok() {
            let _ = profile_directory.sync_all();
            return Err(BootstrapError::ProfileUnavailable {
                path: directory.to_path_buf(),
                detail: format!(
                    "identity publication could not be synchronized and was rolled back: {error}"
                ),
            });
        }
        warnings.push(format!(
            "identity publication committed, but profile synchronization failed: {error}"
        ));
    }
    if target_exists {
        match remove_identity_directory(staging) {
            Ok(()) => {
                if let Err(error) = profile_directory.sync_all() {
                    warnings.push(format!(
                        "retired identity cleanup completed, but profile synchronization failed: {error}"
                    ));
                }
            }
            Err(error) => warnings.push(format!(
                "retired identity cleanup is pending at {}: {error}",
                staging.display()
            )),
        }
    }
    Ok(warnings)
}

#[cfg(any(target_os = "linux", target_os = "android", target_vendor = "apple"))]
fn atomic_exchange(left: &Path, right: &Path) -> Result<(), String> {
    rustix::fs::renameat_with(
        rustix::fs::CWD,
        left,
        rustix::fs::CWD,
        right,
        rustix::fs::RenameFlags::EXCHANGE,
    )
    .map_err(|error| format!("could not atomically replace {}: {error}", right.display()))
}

#[cfg(not(any(target_os = "linux", target_os = "android", target_vendor = "apple")))]
fn atomic_exchange(_left: &Path, right: &Path) -> Result<(), String> {
    Err(format!(
        "atomic directory exchange is unsupported for {}",
        right.display()
    ))
}

#[cfg(any(target_os = "linux", target_os = "android", target_vendor = "apple"))]
fn atomic_publish(staging: &Path, directory: &Path) -> Result<(), String> {
    rustix::fs::renameat_with(
        rustix::fs::CWD,
        staging,
        rustix::fs::CWD,
        directory,
        rustix::fs::RenameFlags::NOREPLACE,
    )
    .map_err(|error| format!("could not publish {}: {error}", directory.display()))
}

#[cfg(not(any(target_os = "linux", target_os = "android", target_vendor = "apple")))]
fn atomic_publish(_staging: &Path, directory: &Path) -> Result<(), String> {
    Err(format!(
        "atomic directory publication is unsupported for {}",
        directory.display()
    ))
}

const fn atomic_directory_exchange_supported() -> bool {
    cfg!(any(
        target_os = "linux",
        target_os = "android",
        target_vendor = "apple"
    ))
}

fn remove_identity_directory(directory: &Path) -> Result<(), String> {
    validate_identity_directory(directory).map_err(|error| error.to_string())?;
    for name in [SEED_FILE, ANCHOR_FILE] {
        let path = directory.join(name);
        match fs::remove_file(&path) {
            Ok(()) => {}
            Err(error) if error.kind() == ErrorKind::NotFound => {}
            Err(error) => return Err(format!("could not remove {}: {error}", path.display())),
        }
    }
    fs::remove_dir(directory)
        .map_err(|error| format!("could not remove {}: {error}", directory.display()))
}

#[cfg(unix)]
#[allow(clippy::verbose_bit_mask)]
fn owner_only(metadata: &fs::Metadata) -> bool {
    use std::os::unix::fs::MetadataExt as _;
    metadata.mode() & 0o077 == 0
}

#[cfg(not(unix))]
const fn owner_only(_metadata: &fs::Metadata) -> bool {
    false
}

#[cfg(unix)]
fn owned_by_current_user(metadata: &fs::Metadata) -> bool {
    use std::os::unix::fs::MetadataExt as _;
    metadata.uid() == rustix::process::geteuid().as_raw()
}

#[cfg(not(unix))]
const fn owned_by_current_user(_metadata: &fs::Metadata) -> bool {
    false
}

#[cfg(test)]
mod profile_path_tests {
    use std::path::Path;

    use super::{validate_profile_path, BootstrapError};

    #[test]
    fn filesystem_root_is_not_a_profile_directory() {
        let error = match validate_profile_path(Path::new("/")) {
            Ok(()) => panic!("root must be refused"),
            Err(error) => error,
        };
        assert!(matches!(error, BootstrapError::ProfileUnavailable { .. }));
    }
}

pub struct EnvironmentInputs {
    pub endpoint: Option<String>,
    pub network_id: Option<u32>,
    pub sequencer_trust_anchor: Option<String>,
    pub sequencer_trust_anchor_file: Option<PathBuf>,
}

pub struct BoundEnvironment {
    pub endpoint: String,
    pub network_id: u32,
    pub sequencer_trust_anchor: String,
    pub anchor_input: &'static str,
}

enum AnchorSource {
    Literal(String),
    File(PathBuf),
}

pub fn resolve_inputs(inputs: EnvironmentInputs) -> Result<Option<BoundEnvironment>, String> {
    let EnvironmentInputs {
        endpoint,
        network_id,
        sequencer_trust_anchor,
        sequencer_trust_anchor_file,
    } = inputs;
    let anchor = match (sequencer_trust_anchor, sequencer_trust_anchor_file) {
        (Some(_), Some(_)) => return Err(BootstrapError::AnchorConflict.into()),
        (Some(value), None) => Some(AnchorSource::Literal(value)),
        (None, Some(path)) => Some(AnchorSource::File(path)),
        (None, None) => None,
    };
    match (endpoint, network_id, anchor) {
        (None, None, None) => Ok(None),
        (Some(endpoint), Some(network_id), Some(anchor)) => {
            validate_endpoint(&endpoint)?;
            if network_id == 0 {
                return Err(BootstrapError::NetworkIdReserved.into());
            }
            let (anchor_input, sequencer_trust_anchor) = load_anchor(anchor)?;
            Ok(Some(BoundEnvironment {
                endpoint,
                network_id,
                sequencer_trust_anchor,
                anchor_input,
            }))
        }
        (endpoint, network_id, anchor) => {
            let mut missing = Vec::new();
            if endpoint.is_none() {
                missing.push(ENDPOINT_INPUT);
            }
            if network_id.is_none() {
                missing.push(NETWORK_ID_INPUT);
            }
            if anchor.is_none() {
                missing.push(ANCHOR_FILE_INPUT);
            }
            Err(BootstrapError::InputMissing { missing }.into())
        }
    }
}

fn validate_endpoint(endpoint: &str) -> Result<(), String> {
    Client::new(endpoint, None).map(|_| ())
}

fn load_anchor(source: AnchorSource) -> Result<(&'static str, String), BootstrapError> {
    let (input, raw) = match source {
        AnchorSource::Literal(value) => (ANCHOR_INPUT, value),
        AnchorSource::File(path) => {
            let contents =
                fs::read_to_string(&path).map_err(|error| BootstrapError::AnchorUnreadable {
                    path: path.clone(),
                    detail: error.to_string(),
                })?;
            (ANCHOR_FILE_INPUT, contents)
        }
    };
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return Err(BootstrapError::AnchorEmpty { input });
    }
    let bytes = decode_public_key(trimmed)
        .map_err(|detail| BootstrapError::AnchorMalformed { input, detail })?;
    Ok((input, hex_encode(&bytes)))
}

fn decode_public_key(encoded: &str) -> Result<[u8; 32], String> {
    let bytes = fixed_hex::<32>("sequencer trust anchor", encoded)?;
    VerifyingKey::from_bytes(&bytes)
        .map_err(|_| "sequencer trust anchor is not a decodable Ed25519 public key".to_owned())?;
    Ok(bytes)
}

struct AdvertisedIdentity {
    network_id: u32,
    sequencer_public_key: String,
}

fn advertised_identity(
    endpoint: &str,
    value: &Value,
) -> Result<AdvertisedIdentity, BootstrapError> {
    let malformed = |detail: &str| BootstrapError::IdentityMalformed {
        endpoint: endpoint.to_owned(),
        detail: detail.to_owned(),
    };
    let result = value
        .get("result")
        .filter(|result| result.is_object())
        .ok_or_else(|| malformed("response carries no result object"))?;
    let network_id = result
        .get("network_id")
        .and_then(Value::as_u64)
        .and_then(|value| u32::try_from(value).ok())
        .ok_or_else(|| malformed("network_id is not a 32-bit integer"))?;
    let encoded = result
        .get("sequencer_public_key")
        .and_then(Value::as_str)
        .ok_or_else(|| malformed("sequencer_public_key is absent"))?;
    let bytes = decode_public_key(encoded).map_err(|detail| malformed(&detail))?;
    Ok(AdvertisedIdentity {
        network_id,
        sequencer_public_key: hex_encode(&bytes),
    })
}

pub fn verify_sequencer_identity(bound: &BoundEnvironment) -> Result<Value, String> {
    wait_for_listener(&bound.endpoint)?;
    let client = Client::new(&bound.endpoint, None)?;
    let value =
        client
            .get(IDENTITY_ROUTE)
            .map_err(|detail| BootstrapError::IdentityUnavailable {
                endpoint: bound.endpoint.clone(),
                detail,
            })?;
    let advertised = advertised_identity(&bound.endpoint, &value)?;
    if advertised.network_id != bound.network_id {
        return Err(BootstrapError::NetworkIdMismatch {
            supplied: bound.network_id,
            advertised: advertised.network_id,
        }
        .into());
    }
    if advertised.sequencer_public_key != bound.sequencer_trust_anchor {
        return Err(BootstrapError::AnchorMismatch {
            input: bound.anchor_input,
            supplied: bound.sequencer_trust_anchor.clone(),
            advertised: advertised.sequencer_public_key,
        }
        .into());
    }
    Ok(json!({
        "endpoint": bound.endpoint,
        "route": IDENTITY_ROUTE,
        "network_id": advertised.network_id,
        "sequencer_public_key": advertised.sequencer_public_key,
    }))
}

fn wait_for_listener(endpoint: &str) -> Result<(), BootstrapError> {
    let address = endpoint_socket_address(endpoint)?;
    let deadline = Instant::now() + IDENTITY_WAIT;
    loop {
        match TcpStream::connect_timeout(&address, CONNECT_TIMEOUT) {
            Ok(stream) => {
                drop(stream);
                return Ok(());
            }
            Err(error) => {
                if Instant::now() >= deadline {
                    return Err(BootstrapError::IdentityUnavailable {
                        endpoint: endpoint.to_owned(),
                        detail: format!(
                            "no listener accepted a connection at {address} within {}s: {error}",
                            IDENTITY_WAIT.as_secs()
                        ),
                    });
                }
                thread::sleep(IDENTITY_POLL);
            }
        }
    }
}

fn endpoint_socket_address(endpoint: &str) -> Result<SocketAddr, BootstrapError> {
    let unavailable = |detail: &str| BootstrapError::IdentityUnavailable {
        endpoint: endpoint.to_owned(),
        detail: detail.to_owned(),
    };
    let (scheme, remainder) = endpoint
        .split_once("://")
        .ok_or_else(|| unavailable("endpoint has no scheme"))?;
    let authority = remainder.split('/').next().unwrap_or_default();
    if authority.is_empty() {
        return Err(unavailable("endpoint has no authority"));
    }
    let default_port: u16 = if scheme == "https" { 443 } else { 80 };
    let resolved = authority
        .to_socket_addrs()
        .or_else(|_| format!("{authority}:{default_port}").to_socket_addrs())
        .map_err(|error| unavailable(&format!("endpoint authority did not resolve: {error}")))?
        .next();
    resolved.ok_or_else(|| unavailable("endpoint authority resolved to no address"))
}
