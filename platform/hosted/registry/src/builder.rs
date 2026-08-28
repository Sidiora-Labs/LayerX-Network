//! Hermetic sandbox that rebuilds published source with the pinned toolchain.
//!
//! Each attempt materialises the canonical archive into a fresh directory,
//! runs bounded declared arguments through the operator-pinned entrypoint with
//! a scrubbed environment and no network, and
//! returns only the bytes found at the declared artifact path. The published
//! plan supplies data arguments only; it cannot replace the executable, and
//! its builder image must be the image this host is pinned to.

use std::fs::{self, File};
use std::io::{self, Read as _, Seek as _, SeekFrom};
use std::os::fd::AsRawFd as _;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::Mutex;
use std::thread;
use std::time::{Duration, Instant};

use layerx_programs::{BuildAttempt, BuildRefusal, BuildRunner, SourceArchive};
use sha2::{Digest as _, Sha256};

const BUILD_LOG: &str = "layerx-build.log";
const POLL_MILLISECONDS: u64 = 100;
const LOG_TAIL: usize = 1_024;
const MAX_ARTIFACT_BYTES: u64 = 32 * 1024 * 1024;
const MAX_ENVIRONMENT_FILES: usize = 100_000;
const MAX_ENVIRONMENT_BYTES: u64 = 4 * 1024 * 1024 * 1024;
const ENVIRONMENT_DOMAIN: &[u8] = b"LayerX/hosted-builder/environment/v1\0";

struct QuotaWorkspace {
    root: PathBuf,
    _lock: File,
}

impl Drop for QuotaWorkspace {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.root);
        if let Some(slot) = self.root.parent() {
            let _ = File::open(slot).and_then(|directory| directory.sync_all());
        }
    }
}

/// Sandbox that executes exactly one declared build command per attempt.
#[derive(Clone, Debug)]
pub struct HermeticBuilder {
    workspace: PathBuf,
    builder_image_digest: [u8; 32],
    environment_root: PathBuf,
    entrypoint: String,
    isolation_runtime: PathBuf,
    isolation_runtime_digest: [u8; 32],
    job_supervisor: PathBuf,
    job_supervisor_digest: [u8; 32],
    cgroup_root: PathBuf,
    timeout_seconds: u64,
    memory_bytes: u64,
    process_limit: u32,
    file_size_bytes: u64,
    request_deadline: std::sync::Arc<Mutex<Option<Instant>>>,
}

impl HermeticBuilder {
    /// Binds the sandbox to the builder image this host is pinned to.
    ///
    /// # Errors
    ///
    /// Refuses unpinned environment or isolation bytes, invalid resource
    /// bounds, and unusable environment or workspace paths.
    pub fn new(
        workspace: PathBuf,
        builder_image_digest: [u8; 32],
        environment_root: PathBuf,
        entrypoint: String,
        isolation_runtime: PathBuf,
        isolation_runtime_digest: [u8; 32],
        job_supervisor: PathBuf,
        job_supervisor_digest: [u8; 32],
        cgroup_root: PathBuf,
        timeout_seconds: u64,
        memory_bytes: u64,
        process_limit: u32,
        file_size_bytes: u64,
    ) -> Result<Self, String> {
        if builder_image_digest == [0; 32] {
            return Err("the pinned builder image digest is required".to_owned());
        }
        if !(1..=3_600).contains(&timeout_seconds)
            || !(67_108_864..=8_589_934_592).contains(&memory_bytes)
            || !(1..=256).contains(&process_limit)
            || !(33_554_432..=134_217_728).contains(&file_size_bytes)
            || !entrypoint.starts_with('/')
            || entrypoint.contains("..")
        {
            return Err("the isolated builder bounds and absolute in-environment entrypoint are required".to_owned());
        }
        let environment_root = fs::canonicalize(&environment_root)
            .map_err(|error| format!("builder environment is unavailable: {error}"))?;
        if environment_digest(&environment_root, None)? != builder_image_digest {
            return Err("builder environment bytes do not match the configured immutable digest".to_owned());
        }
        let isolation_runtime = verified_executable(isolation_runtime, isolation_runtime_digest, "isolation runtime")?;
        let job_supervisor = verified_executable(job_supervisor, job_supervisor_digest, "cgroup job supervisor")?;
        let cgroup_root = fs::canonicalize(cgroup_root)
            .map_err(|error| format!("delegated builder cgroup is unavailable: {error}"))?;
        if !cgroup_root.is_dir() {
            return Err("delegated builder cgroup must be a directory".to_owned());
        }
        let workspace = validate_build_boundary(&workspace, &cgroup_root)?;
        if !environment_root.join(entrypoint.trim_start_matches('/')).is_file() {
            return Err("builder entrypoint is absent from the pinned environment".to_owned());
        }
        Ok(Self {
            workspace,
            builder_image_digest,
            environment_root,
            entrypoint,
            isolation_runtime,
            isolation_runtime_digest,
            job_supervisor,
            job_supervisor_digest,
            cgroup_root,
            timeout_seconds,
            memory_bytes,
            process_limit,
            file_size_bytes,
            request_deadline: std::sync::Arc::new(Mutex::new(None)),
        })
    }

    /// Binds subsequent attempts to the monotonic ingress deadline.
    pub fn set_request_deadline(&self, deadline: Instant) -> Result<(), String> {
        self.request_deadline
            .lock()
            .map(|mut current| *current = Some(deadline))
            .map_err(|_| "builder deadline lock is unavailable".to_owned())
    }

    /// Returns the builder image digest every published plan must declare.
    #[must_use]
    pub const fn builder_image_digest(&self) -> [u8; 32] {
        self.builder_image_digest
    }

    fn sandbox(&self, attempt: &BuildAttempt<'_>) -> Result<QuotaWorkspace, BuildRefusal> {
        let mut slots = fs::read_dir(&self.workspace).map_err(unavailable)?
            .collect::<Result<Vec<_>, _>>().map_err(unavailable)?;
        slots.sort_by_key(fs::DirEntry::file_name);
        let (slot, lock) = slots.into_iter().find_map(|entry| {
            let slot = entry.path();
            let lock = slot.join(".layerx-build-lock");
            let file = fs::OpenOptions::new().create(true).read(true).write(true).open(&lock).ok()?;
            rustix::fs::flock(&file, rustix::fs::FlockOperation::NonBlockingLockExclusive)
                .ok().map(|()| (slot, file))
        }).ok_or_else(|| BuildRefusal::SandboxUnavailable {
            reason: "no hard-quota build filesystem is available".to_owned(),
        })?;
        for entry in fs::read_dir(&slot).map_err(unavailable)? {
            let entry = entry.map_err(unavailable)?;
            if entry.file_name() == ".layerx-build-lock" || entry.file_name() == "lost+found" {
                continue;
            }
            let kind = entry.file_type().map_err(unavailable)?;
            if kind.is_dir() {
                fs::remove_dir_all(entry.path()).map_err(unavailable)?;
            } else {
                fs::remove_file(entry.path()).map_err(unavailable)?;
            }
        }
        File::open(&slot).and_then(|directory| directory.sync_all()).map_err(unavailable)?;
        let root = slot.join(format!("attempt-{}", attempt.attempt));
        if let Err(error) = fs::create_dir(&root) {
            return Err(unavailable(error));
        }
        let workspace = QuotaWorkspace { root, _lock: lock };
        let deadline = self.request_deadline.lock().map_err(|_| BuildRefusal::SandboxUnavailable {
            reason: "builder deadline lock is unavailable".to_owned(),
        })?.ok_or_else(|| BuildRefusal::SandboxUnavailable {
            reason: "builder request deadline is unavailable".to_owned(),
        })?;
        fs::create_dir_all(workspace.root.join("source")).map_err(unavailable)?;
        materialize(&workspace.root.join("source"), attempt.archive, deadline)?;
        copy_environment(&self.environment_root, &workspace.root.join("environment"), deadline)?;
        if environment_digest(&workspace.root.join("environment"), Some(deadline)).map_err(|reason| BuildRefusal::SandboxUnavailable { reason })?
            != self.builder_image_digest
        {
            return Err(BuildRefusal::BuilderImageMismatch);
        }
        Ok(workspace)
    }

    fn execute(&self, attempt: &BuildAttempt<'_>, root: &Path) -> Result<(), BuildRefusal> {
        let isolation_runtime = verified_executable_fd(
            &self.isolation_runtime,
            self.isolation_runtime_digest,
            "isolation runtime",
        )
        .map_err(|reason| BuildRefusal::SandboxUnavailable { reason })?;
        let job_supervisor = verified_executable_fd(
            &self.job_supervisor,
            self.job_supervisor_digest,
            "cgroup job supervisor",
        )
        .map_err(|reason| BuildRefusal::SandboxUnavailable { reason })?;
        let source = root.join("source");
        let environment = root.join("environment");
        let arguments = &attempt.plan.environment.command;
        if arguments.len() > 32 || arguments.iter().any(|argument| {
            argument.is_empty()
                || argument.len() > 512
                || argument.starts_with('/')
                || argument.contains("..")
                || argument.bytes().any(|byte| byte.is_ascii_control())
        }) {
            return Err(BuildRefusal::InvalidPlan);
        }
        let log_path = source.join(BUILD_LOG);
        let log = File::create(&log_path).map_err(unavailable)?;
        let mut log_reader = log.try_clone().map_err(unavailable)?;
        let request_deadline = self.request_deadline.lock().map_err(|_| BuildRefusal::SandboxUnavailable {
            reason: "builder deadline lock is unavailable".to_owned(),
        })?.ok_or_else(|| BuildRefusal::SandboxUnavailable {
            reason: "builder request deadline is unavailable".to_owned(),
        })?;
        let remaining = request_deadline.checked_duration_since(Instant::now()).ok_or_else(|| {
            BuildRefusal::BuilderFailed {
                reason: "request deadline expired before cgroup admission".to_owned(),
            }
        })?;
        let mut child = Command::new(format!("/proc/self/fd/{}", job_supervisor.as_raw_fd()))
            .arg("--cgroup-v2")
            .arg("--cgroup-root")
            .arg(&self.cgroup_root)
            .arg("--attach-before-exec")
            .arg("--kill-tree-on-exit")
            .arg(format!("--memory-max={}", self.memory_bytes))
            .arg(format!("--cpu-time-max-usec={}", self.timeout_seconds.saturating_mul(1_000_000)))
            .arg(format!("--pids-max={}", self.process_limit))
            .arg(format!("--io-write-max={}", self.file_size_bytes))
            .arg("--workspace-device-path")
            .arg(&source)
            .arg(format!("--wall-time-max-ms={}", remaining.as_millis()))
            .arg("--")
            .arg(format!("/proc/self/fd/{}", isolation_runtime.as_raw_fd()))
            .args([
                "--unshare-all",
                "--die-with-parent",
                "--new-session",
                "--disable-userns",
                "--cap-drop",
                "ALL",
                "--clearenv",
                "--ro-bind",
            ])
            .arg(&environment)
            .arg("/")
            .args(["--dir", "/build", "--bind"])
            .arg(&source)
            .arg("/build")
            .args(["--dir", "/tmp", "--tmpfs", "/tmp", "--proc", "/proc", "--dev", "/dev"])
            .args(["--chdir", "/build", "--setenv", "HOME", "/tmp", "--setenv", "TMPDIR", "/tmp"])
            .args(["--setenv", "CARGO_HOME", "/opt/cache/cargo", "--setenv", "RUSTUP_HOME", "/opt/cache/rustup"])
            .args(["--setenv", "CARGO_TERM_COLOR", "never", "--setenv", "CARGO_NET_OFFLINE", "true"])
            .args(["--setenv", "TZ", "UTC", "--setenv", "LC_ALL", "C"])
            .arg("--setenv")
            .arg("SOURCE_DATE_EPOCH")
            .arg(attempt.plan.environment.source_date_epoch.to_string())
            .arg("--setenv")
            .arg("RUSTFLAGS")
            .arg("--remap-path-prefix=/build=/layerx/source")
            .arg("--")
            .arg(&self.entrypoint)
            .args(arguments)
            .env_clear()
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::from(log))
            .spawn()
            .map_err(unavailable)?;
        let build_deadline = Instant::now()
            .checked_add(Duration::from_secs(self.timeout_seconds))
            .ok_or_else(|| BuildRefusal::SandboxUnavailable {
                reason: "declared build timeout is out of range".to_owned(),
            })?;
        let deadline = build_deadline.min(request_deadline);
        let status = loop {
            match child.try_wait() {
                Ok(Some(status)) => break status,
                Ok(None) => {
                    if Instant::now() >= deadline {
                        let _ = child.kill();
                        let _ = child.wait();
                        return Err(BuildRefusal::BuilderFailed {
                            reason: format!(
                                "pinned build exceeded {} seconds",
                                self.timeout_seconds
                            ),
                        });
                    }
                    thread::sleep(Duration::from_millis(POLL_MILLISECONDS));
                }
                Err(error) => return Err(unavailable(error)),
            }
        };
        if status.success() {
            Ok(())
        } else {
            Err(BuildRefusal::BuilderFailed {
                reason: format!("{status}: {}", tail_open(&mut log_reader)?),
            })
        }
    }
}

#[cfg(unix)]
fn validate_build_boundary(workspace: &Path, cgroup: &Path) -> Result<PathBuf, String> {
    use std::os::unix::fs::MetadataExt as _;

    let cgroup_metadata = fs::metadata(cgroup).map_err(|error| error.to_string())?;
    if cgroup_metadata.uid() != 4030 || cgroup_metadata.gid() != 4030 {
        return Err("builder cgroup is not delegated to UID/GID 4030".to_owned());
    }
    let controllers = fs::read_to_string(cgroup.join("cgroup.subtree_control"))
        .map_err(|error| format!("builder cgroup controllers are unavailable: {error}"))?;
    if ["cpu", "memory", "pids", "io"].iter().any(|required| {
        !controllers.split_whitespace().any(|controller| controller == *required)
    }) {
        return Err("builder cgroup delegation omits a required controller".to_owned());
    }
    let root = fs::canonicalize(workspace)
        .map_err(|error| format!("build quota root is unavailable: {error}"))?;
    let root_device = fs::metadata(&root).map_err(|error| error.to_string())?.dev();
    let slots = fs::read_dir(&root).map_err(|error| error.to_string())?
        .collect::<Result<Vec<_>, _>>().map_err(|error| error.to_string())?;
    let mounted = slots.iter().filter(|entry| {
        entry.file_type().is_ok_and(|kind| kind.is_dir())
            && entry.metadata().is_ok_and(|metadata| {
                metadata.uid() == 4030 && metadata.gid() == 4030 && metadata.dev() != root_device
            })
    }).count();
    if mounted == 0 || mounted > 64 {
        return Err("no owned hard-quota build filesystem is mounted".to_owned());
    }
    for entry in slots.iter().filter(|entry| {
        entry.file_type().is_ok_and(|kind| kind.is_dir())
            && entry.metadata().is_ok_and(|metadata| metadata.dev() != root_device)
    }) {
        let slot = entry.path();
        let lock = fs::OpenOptions::new().create(true).read(true).write(true)
            .open(slot.join(".layerx-build-lock")).map_err(|error| error.to_string())?;
        if rustix::fs::flock(&lock, rustix::fs::FlockOperation::NonBlockingLockExclusive).is_err() {
            continue;
        }
        for stale in fs::read_dir(&slot).map_err(|error| error.to_string())? {
            let stale = stale.map_err(|error| error.to_string())?;
            if stale.file_name() == ".layerx-build-lock" || stale.file_name() == "lost+found" {
                continue;
            }
            if stale.file_type().map_err(|error| error.to_string())?.is_dir() {
                fs::remove_dir_all(stale.path()).map_err(|error| error.to_string())?;
            } else {
                fs::remove_file(stale.path()).map_err(|error| error.to_string())?;
            }
        }
        File::open(&slot).and_then(|directory| directory.sync_all())
            .map_err(|error| format!("stale quota slot could not be durably reclaimed: {error}"))?;
    }
    Ok(root)
}

#[cfg(not(unix))]
fn validate_build_boundary(_workspace: &Path, _cgroup: &Path) -> Result<PathBuf, String> {
    Err("build boundary delegation requires Linux".to_owned())
}

fn verified_executable(path: PathBuf, expected: [u8; 32], label: &str) -> Result<PathBuf, String> {
    let canonical = fs::canonicalize(path).map_err(|error| format!("{label} is unavailable: {error}"))?;
    let metadata = fs::metadata(&canonical).map_err(|error| format!("{label} is unavailable: {error}"))?;
    if !metadata.is_file() || metadata.len() == 0 || metadata.len() > 64 * 1024 * 1024 {
        return Err(format!("{label} is not a bounded regular file"));
    }
    let bytes = fs::read(&canonical).map_err(|error| format!("{label} is unreadable: {error}"))?;
    let digest: [u8; 32] = Sha256::digest(&bytes).into();
    if digest != expected {
        return Err(format!("{label} bytes do not match the configured digest"));
    }
    Ok(canonical)
}

fn verified_executable_fd(path: &Path, expected: [u8; 32], label: &str) -> Result<File, String> {
    use rustix::fs::{fcntl_setfd, FdFlags};

    let mut file = File::open(path).map_err(|error| format!("{label} is unavailable: {error}"))?;
    let metadata = file.metadata().map_err(|error| format!("{label} is unavailable: {error}"))?;
    if !metadata.is_file() || metadata.len() == 0 || metadata.len() > 64 * 1024 * 1024 {
        return Err(format!("{label} is not a bounded regular file"));
    }
    let mut bytes = Vec::with_capacity(usize::try_from(metadata.len()).unwrap_or(0));
    file.read_to_end(&mut bytes).map_err(|error| format!("{label} is unreadable: {error}"))?;
    if <[u8; 32]>::from(Sha256::digest(&bytes)) != expected {
        return Err(format!("{label} bytes do not match the configured digest"));
    }
    file.seek(SeekFrom::Start(0)).map_err(|error| format!("{label} is unseekable: {error}"))?;
    fcntl_setfd(&file, FdFlags::empty()).map_err(|error| format!("{label} fd cannot be inherited: {error}"))?;
    Ok(file)
}

fn environment_digest(root: &Path, deadline: Option<Instant>) -> Result<[u8; 32], String> {
    let mut pending = vec![root.to_path_buf()];
    let mut entries = Vec::new();
    while let Some(directory) = pending.pop() {
        if deadline.is_some_and(|deadline| Instant::now() >= deadline) {
            return Err("request deadline expired while hashing the build environment".to_owned());
        }
        for entry in fs::read_dir(&directory).map_err(|error| error.to_string())? {
            let entry = entry.map_err(|error| error.to_string())?;
            let path = entry.path();
            let metadata = fs::symlink_metadata(&path).map_err(|error| error.to_string())?;
            if metadata.file_type().is_symlink() {
                return Err("builder environment contains a symbolic link".to_owned());
            }
            if metadata.is_dir() {
                entries.push((path.clone(), true, 0));
                pending.push(path);
            } else if metadata.is_file() {
                entries.push((path, false, file_mode(&metadata)));
            } else {
                return Err("builder environment contains a non-regular object".to_owned());
            }
            if entries.len().saturating_add(pending.len()) > MAX_ENVIRONMENT_FILES {
                return Err("builder environment exceeds its file bound".to_owned());
            }
        }
    }
    entries.sort_by(|left, right| left.0.cmp(&right.0));
    let mut total = 0_u64;
    let mut digest = Sha256::new();
    digest.update(ENVIRONMENT_DOMAIN);
    for (path, directory, mode) in entries {
        if deadline.is_some_and(|deadline| Instant::now() >= deadline) {
            return Err("request deadline expired while hashing the build environment".to_owned());
        }
        let relative = path.strip_prefix(root).map_err(|_| "builder environment path escaped its root".to_owned())?;
        let name = relative.to_str().ok_or_else(|| "builder environment path is not UTF-8".to_owned())?;
        let bytes = if directory {
            Vec::new()
        } else {
            fs::read(&path).map_err(|error| error.to_string())?
        };
        total = total.checked_add(bytes.len() as u64).ok_or_else(|| "builder environment size overflowed".to_owned())?;
        if total > MAX_ENVIRONMENT_BYTES {
            return Err("builder environment exceeds its byte bound".to_owned());
        }
        digest.update((name.len() as u64).to_be_bytes());
        digest.update(name.as_bytes());
        digest.update([u8::from(directory)]);
        digest.update(mode.to_be_bytes());
        digest.update((bytes.len() as u64).to_be_bytes());
        digest.update(bytes);
    }
    Ok(digest.finalize().into())
}

#[cfg(unix)]
fn file_mode(metadata: &fs::Metadata) -> u32 {
    use std::os::unix::fs::PermissionsExt as _;

    metadata.permissions().mode()
}

#[cfg(not(unix))]
fn file_mode(_metadata: &fs::Metadata) -> u32 {
    0
}

impl BuildRunner for HermeticBuilder {
    fn run(&self, attempt: &BuildAttempt<'_>) -> Result<Vec<u8>, BuildRefusal> {
        if attempt.plan.environment.builder_image_digest != self.builder_image_digest {
            return Err(BuildRefusal::BuilderImageMismatch);
        }
        let workspace = self.sandbox(attempt)?;
        let produced = self.execute(attempt, &workspace.root).and_then(|()| {
            read_beneath(
                &workspace.root.join("source"),
                Path::new(&attempt.plan.artifact_path),
                MAX_ARTIFACT_BYTES,
            )
            .map_err(|error| BuildRefusal::SandboxUnavailable {
                reason: format!("sandbox artifact could not be opened safely: {error}"),
            })
        });
        produced
    }
}

fn copy_environment(source: &Path, target: &Path, deadline: Instant) -> Result<(), BuildRefusal> {
    fs::create_dir_all(target).map_err(unavailable)?;
    let mut pending = vec![(source.to_path_buf(), target.to_path_buf())];
    let mut seen = 0_usize;
    let mut copied = 0_u64;
    while let Some((from, to)) = pending.pop() {
        if Instant::now() >= deadline {
            return Err(BuildRefusal::BuilderFailed {
                reason: "request deadline expired while snapshotting the build environment".to_owned(),
            });
        }
        for entry in fs::read_dir(&from).map_err(unavailable)? {
            let entry = entry.map_err(unavailable)?;
            let source_path = entry.path();
            let target_path = to.join(entry.file_name());
            let metadata = fs::symlink_metadata(&source_path).map_err(unavailable)?;
            seen = seen.saturating_add(1);
            if seen > MAX_ENVIRONMENT_FILES || metadata.file_type().is_symlink() {
                return Err(BuildRefusal::SandboxUnavailable {
                    reason: "builder environment is not a bounded regular tree".to_owned(),
                });
            }
            if metadata.is_dir() {
                fs::create_dir(&target_path).map_err(unavailable)?;
                pending.push((source_path, target_path));
            } else if metadata.is_file() {
                copied = copied.checked_add(metadata.len()).ok_or_else(|| BuildRefusal::SandboxUnavailable {
                    reason: "builder environment size overflowed".to_owned(),
                })?;
                if copied > MAX_ENVIRONMENT_BYTES {
                    return Err(BuildRefusal::SandboxUnavailable {
                        reason: "builder environment exceeds its byte bound".to_owned(),
                    });
                }
                fs::copy(&source_path, &target_path).map_err(unavailable)?;
                fs::set_permissions(&target_path, metadata.permissions()).map_err(unavailable)?;
            } else {
                return Err(BuildRefusal::SandboxUnavailable {
                    reason: "builder environment contains a non-regular object".to_owned(),
                });
            }
        }
    }
    Ok(())
}

fn materialize(root: &Path, archive: &SourceArchive, deadline: Instant) -> Result<(), BuildRefusal> {
    for file in archive.files() {
        if Instant::now() >= deadline {
            return Err(BuildRefusal::BuilderFailed {
                reason: "request deadline expired while materializing source".to_owned(),
            });
        }
        let target = root.join(&file.path);
        if let Some(parent) = target.parent() {
            fs::create_dir_all(parent).map_err(unavailable)?;
        }
        fs::write(&target, &file.content).map_err(unavailable)?;
        make_non_executable(&target)?;
    }
    Ok(())
}

#[cfg(unix)]
fn make_non_executable(path: &Path) -> Result<(), BuildRefusal> {
    use std::os::unix::fs::PermissionsExt as _;

    fs::set_permissions(path, fs::Permissions::from_mode(0o600)).map_err(unavailable)
}

#[cfg(not(unix))]
fn make_non_executable(_path: &Path) -> Result<(), BuildRefusal> {
    Ok(())
}

fn tail_open(file: &mut File) -> Result<String, BuildRefusal> {
    let metadata = file.metadata().map_err(unavailable)?;
    if !metadata.is_file() || metadata.len() > 134_217_728 {
        return Err(BuildRefusal::SandboxUnavailable {
            reason: "builder log is not a bounded regular file".to_owned(),
        });
    }
    let length = metadata.len();
    let start = length.saturating_sub(LOG_TAIL as u64);
    file.seek(SeekFrom::Start(start)).map_err(unavailable)?;
    let mut bytes = Vec::new();
    file.take(LOG_TAIL as u64).read_to_end(&mut bytes).map_err(unavailable)?;
    Ok(String::from_utf8_lossy(&bytes)
        .replace(['\n', '\r', '"'], " ")
        .trim()
        .to_owned())
}

#[cfg(target_os = "linux")]
fn read_beneath(root: &Path, relative: &Path, maximum: u64) -> Result<Vec<u8>, io::Error> {
    use rustix::fs::{fstat, open, openat2, FileType, Mode, OFlags, ResolveFlags};

    let directory = open(
        root,
        OFlags::RDONLY | OFlags::DIRECTORY | OFlags::NOFOLLOW | OFlags::CLOEXEC,
        Mode::empty(),
    )?;
    let descriptor = openat2(
        &directory,
        relative,
        OFlags::RDONLY | OFlags::NOFOLLOW | OFlags::CLOEXEC,
        Mode::empty(),
        ResolveFlags::BENEATH | ResolveFlags::NO_SYMLINKS | ResolveFlags::NO_MAGICLINKS,
    )?;
    let stat = fstat(&descriptor)?;
    if FileType::from_raw_mode(stat.st_mode) != FileType::RegularFile
        || stat.st_size < 0
        || u64::try_from(stat.st_size).map_or(true, |size| size > maximum)
    {
        return Err(io::Error::other("sandbox output is not a bounded regular file"));
    }
    let mut file = File::from(descriptor);
    let mut bytes = Vec::with_capacity(usize::try_from(stat.st_size).unwrap_or(0));
    file.take(maximum.saturating_add(1)).read_to_end(&mut bytes)?;
    if bytes.len() as u64 > maximum {
        return Err(io::Error::other("sandbox output exceeds its byte bound"));
    }
    Ok(bytes)
}

#[cfg(not(target_os = "linux"))]
fn read_beneath(_root: &Path, _relative: &Path, _maximum: u64) -> Result<Vec<u8>, io::Error> {
    Err(io::Error::other("descriptor-safe sandbox output requires Linux openat2"))
}

#[allow(clippy::needless_pass_by_value)]
fn unavailable(error: io::Error) -> BuildRefusal {
    BuildRefusal::SandboxUnavailable {
        reason: error.to_string(),
    }
}
