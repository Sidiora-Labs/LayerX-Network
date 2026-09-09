//! Executable entry point of the hosted program registry.

use std::env;
use std::fs::{self, File};
use std::io::{self, Read, Write};
use std::net::TcpListener;
use std::os::unix::fs::MetadataExt as _;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{mpsc, Arc, Mutex, OnceLock, TryLockError};
use std::thread;
use std::time::{Duration, Instant};
use std::time::{SystemTime, UNIX_EPOCH};

use layerx_platform_registry::{
    parse_request, refusal, write_response, Config, HermeticBuilder, Registrar, RegistryAuthority,
};
use layerx_programs::hex;
use rustix::process::{kill_process, Pid, Signal};
use rustls::pki_types::{CertificateDer, PrivateKeyDer};
use rustls::server::WebPkiClientVerifier;
use rustls::{RootCertStore, ServerConfig, ServerConnection, StreamOwned};
use zeroize::{Zeroize as _, Zeroizing};

const DEFAULT_LISTEN: &str = "127.0.0.1:9420";
const DEFAULT_ROOT: &str = "/var/lib/layerx-program-registry";
static ACTIVE_CONNECTIONS: AtomicUsize = AtomicUsize::new(0);

static WORKER_ROOT: OnceLock<PathBuf> = OnceLock::new();
const CONTROLLERS: &str = "+cpu +memory +pids +io";

fn discovery_owner_allowed(metadata: &fs::Metadata) -> bool {
    metadata.uid() == 0
}

fn locate_container_cgroup(mount: &Path) -> Result<PathBuf, String> {
    locate_cgroup_matching(
        mount,
        &fs::metadata("/sys/fs/cgroup").map_err(|e| e.to_string())?,
    )
}

fn locate_cgroup_matching(mount: &Path, namespace: &fs::Metadata) -> Result<PathBuf, String> {
    if !mount.is_absolute() || fs::canonicalize(mount).map_err(|e| e.to_string())? != mount {
        return Err("host cgroup mount must be canonical and absolute".to_owned());
    }
    let mut excluded_owners = std::collections::HashSet::new();
    let mut pending = vec![(mount.to_path_buf(), 0)];
    let mut found = None;
    let mut visited = 0;
    while let Some((path, depth)) = pending.pop() {
        visited += 1;
        if visited > 65_536 {
            return Err("host cgroup walk exceeds directory bound".to_owned());
        }
        let metadata = fs::symlink_metadata(&path).map_err(|e| e.to_string())?;
        if !metadata.is_dir() || metadata.dev() != namespace.dev() {
            continue;
        }
        if !discovery_owner_allowed(&metadata) {
            if excluded_owners.insert(metadata.uid()) {
                eprintln!(
                    "DEBUG host cgroup discovery excludes directories owned by uid {}",
                    metadata.uid()
                );
            }
            continue;
        }
        if metadata.ino() == namespace.ino() {
            if found.replace(path.clone()).is_some() {
                return Err("multiple container cgroup matches".to_owned());
            }
            continue;
        }
        if depth < 8 {
            for entry in fs::read_dir(&path).map_err(|e| {
                format!(
                    "host cgroup directory {} is unreadable: {e}",
                    path.display()
                )
            })? {
                let entry = entry.map_err(|e| e.to_string())?;
                if entry.file_type().map_err(|e| e.to_string())?.is_dir() {
                    pending.push((entry.path(), depth + 1));
                }
            }
        }
    }
    let root = found.ok_or("container cgroup not found within depth 8")?;
    let members = fs::read_to_string(root.join("cgroup.procs")).map_err(|e| e.to_string())?;
    if !members
        .lines()
        .any(|pid| pid == std::process::id().to_string())
    {
        return Err("container cgroup does not contain the registry process".to_owned());
    }
    Ok(root)
}

fn delegate_container_cgroup(root: &Path) -> Result<PathBuf, String> {
    use nix::unistd::{chown, Gid, Uid};
    let controllers =
        fs::read_to_string(root.join("cgroup.controllers")).map_err(|e| e.to_string())?;
    for required in ["cpu", "memory", "pids", "io"] {
        if !controllers
            .split_whitespace()
            .any(|value| value == required)
        {
            return Err(format!(
                "container cgroup lacks required controller {required}"
            ));
        }
    }
    let main = root.join("main");
    fs::create_dir(&main).map_err(|e| e.to_string())?;
    fs::write(main.join("cgroup.procs"), std::process::id().to_string())
        .map_err(|e| e.to_string())?;
    fs::write(root.join("cgroup.subtree_control"), CONTROLLERS).map_err(|e| e.to_string())?;
    let workers = root.join("workers");
    fs::create_dir(&workers).map_err(|e| e.to_string())?;
    fs::write(workers.join("cgroup.subtree_control"), CONTROLLERS).map_err(|e| e.to_string())?;
    for path in [
        root.join("cgroup.procs"),
        workers.clone(),
        workers.join("cgroup.procs"),
        workers.join("cgroup.subtree_control"),
        workers.join("cgroup.threads"),
    ] {
        chown(&path, Some(Uid::from_raw(4030)), Some(Gid::from_raw(4030)))
            .map_err(|e| format!("container cgroup ownership failed: {e}"))?;
    }
    Ok(workers)
}

fn verify_dropped_privileges() -> Result<(), String> {
    let status = fs::read_to_string("/proc/self/status").map_err(|e| e.to_string())?;
    for field in ["CapEff:", "CapPrm:", "CapInh:"] {
        let value = status
            .lines()
            .find_map(|line| line.strip_prefix(field))
            .ok_or_else(|| format!("missing privilege field {field}"))?;
        if u64::from_str_radix(value.trim(), 16) != Ok(0) {
            return Err(format!("registry retains capabilities in {field}"));
        }
    }
    for field in ["Uid:", "Gid:"] {
        let values = status
            .lines()
            .find_map(|line| line.strip_prefix(field))
            .ok_or_else(|| format!("missing identity field {field}"))?;
        if values.split_whitespace().collect::<Vec<_>>() != ["4030"; 4] {
            return Err(format!(
                "registry identity is not irrevocably 4030 in {field}"
            ));
        }
    }
    let root = nix::unistd::Uid::from_raw(0);
    if nix::unistd::setresuid(root, root, root) != Err(nix::errno::Errno::EPERM) {
        return Err("registry root identity could be restored".to_owned());
    }
    Ok(())
}

fn drop_registry_privileges() -> Result<(), String> {
    use nix::unistd::{setgroups, setresgid, setresuid, Gid, Uid};
    nix::sys::prctl::set_keepcaps(false).map_err(|e| e.to_string())?;
    let gid = Gid::from_raw(4030);
    let uid = Uid::from_raw(4030);
    setgroups(&[gid]).map_err(|e| e.to_string())?;
    setresgid(gid, gid, gid).map_err(|e| e.to_string())?;
    setresuid(uid, uid, uid).map_err(|e| e.to_string())?;
    verify_dropped_privileges()
}

fn initialize_cgroup_boundary() -> Result<PathBuf, String> {
    if !nix::unistd::geteuid().is_root() {
        return Err("registry startup requires UID 0 for container cgroup delegation".to_owned());
    }
    let mount = env::var("LAYERX_REGISTRY_HOST_CGROUP_MOUNT")
        .map_err(|_| "LAYERX_REGISTRY_HOST_CGROUP_MOUNT is required".to_owned())?;
    let root = locate_container_cgroup(Path::new(&mount))?;
    let workers = delegate_container_cgroup(&root)?;
    drop_registry_privileges()?;
    WORKER_ROOT
        .set(workers.clone())
        .map_err(|_| "worker root already initialized")?;
    Ok(workers)
}

struct ConnectionGuard;

impl Drop for ConnectionGuard {
    fn drop(&mut self) {
        ACTIVE_CONNECTIONS.fetch_sub(1, Ordering::AcqRel);
    }
}

struct BuildGuard<'a>(&'a AtomicUsize);

impl Drop for BuildGuard<'_> {
    fn drop(&mut self) {
        self.0.fetch_sub(1, Ordering::AcqRel);
    }
}

struct Service {
    builder: HermeticBuilder,
    builder_ready: Arc<Mutex<Option<Instant>>>,
    registrar_gate: Mutex<()>,
    request_authority: RegistryAuthority,
    publication_authority: RegistryAuthority,
    active_builds: AtomicUsize,
    max_builds: usize,
    timeout: Duration,
}

struct DeadlineStream {
    inner: StreamOwned<ServerConnection, std::net::TcpStream>,
    deadline: Instant,
}

impl DeadlineStream {
    fn remaining(&self) -> io::Result<Duration> {
        self.deadline
            .checked_duration_since(Instant::now())
            .filter(|remaining| !remaining.is_zero())
            .ok_or_else(|| {
                io::Error::new(io::ErrorKind::TimedOut, "absolute request deadline expired")
            })
    }
}

impl Read for DeadlineStream {
    fn read(&mut self, bytes: &mut [u8]) -> io::Result<usize> {
        self.inner.sock.set_read_timeout(Some(self.remaining()?))?;
        self.inner.read(bytes)
    }
}

impl Write for DeadlineStream {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        self.inner.sock.set_write_timeout(Some(self.remaining()?))?;
        self.inner.write(bytes)
    }

    fn flush(&mut self) -> io::Result<()> {
        self.inner.sock.set_write_timeout(Some(self.remaining()?))?;
        self.inner.flush()
    }
}

struct WatchdogCompletion(mpsc::Sender<()>);

impl Drop for WatchdogCompletion {
    fn drop(&mut self) {
        let _ = self.0.send(());
    }
}

fn now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .ok()
        .and_then(|value| u64::try_from(value.as_millis()).ok())
        .unwrap_or(0)
}

fn parse_u64(name: &str, default: u64) -> Result<u64, String> {
    env::var(name).map_or(Ok(default), |value| {
        value
            .parse()
            .map_err(|_| format!("{name} must be an integer"))
    })
}

fn parse_u32(name: &str, default: u32) -> Result<u32, String> {
    env::var(name).map_or(Ok(default), |value| {
        value
            .parse()
            .map_err(|_| format!("{name} must be an integer"))
    })
}

fn parse_usize(name: &str, default: usize, maximum: usize) -> Result<usize, String> {
    let value = env::var(name).map_or(Ok(default), |value| {
        value
            .parse()
            .map_err(|_| format!("{name} must be an integer"))
    })?;
    if value == 0 || value > maximum {
        return Err(format!("{name} is outside its bound"));
    }
    Ok(value)
}

fn read_secret(name: &str) -> Result<Zeroizing<String>, String> {
    let configured = PathBuf::from(env::var(name).map_err(|_| format!("{name} is required"))?);
    if !configured.is_absolute()
        || fs::canonicalize(&configured).map_err(|error| error.to_string())? != configured
    {
        return Err(format!("{name} must name a canonical absolute file"));
    }
    let metadata = fs::metadata(&configured).map_err(|error| error.to_string())?;
    if !metadata.is_file() || metadata.len() > 4_098 {
        return Err(format!("{name} must name a bounded regular file"));
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::{MetadataExt as _, PermissionsExt as _};
        if metadata.permissions().mode() & 0o007 != 0 || metadata.nlink() != 1 {
            return Err(format!(
                "{name} must be process-group private and singly linked"
            ));
        }
    }
    let mut secret = fs::read_to_string(&configured).map_err(|error| error.to_string())?;
    while matches!(secret.as_bytes().last(), Some(b'\r' | b'\n')) {
        secret.pop();
    }
    if secret.is_empty() || secret.len() > 4_096 {
        secret.zeroize();
        return Err(format!("{name} does not contain a bounded secret"));
    }
    Ok(Zeroizing::new(secret))
}

fn read_bounded_file(name: &str, maximum: u64) -> Result<Vec<u8>, String> {
    let configured = PathBuf::from(env::var(name).map_err(|_| format!("{name} is required"))?);
    if !configured.is_absolute() {
        return Err(format!("{name} must name an absolute file"));
    }
    let metadata = fs::metadata(&configured).map_err(|error| error.to_string())?;
    if !metadata.is_file() || metadata.len() == 0 || metadata.len() > maximum {
        return Err(format!("{name} must name a bounded regular file"));
    }
    fs::read(configured).map_err(|error| error.to_string())
}

fn read_private_file(name: &str, maximum: u64) -> Result<Vec<u8>, String> {
    let configured = PathBuf::from(env::var(name).map_err(|_| format!("{name} is required"))?);
    if !configured.is_absolute()
        || fs::canonicalize(&configured).map_err(|error| error.to_string())? != configured
    {
        return Err(format!("{name} must name a canonical absolute file"));
    }
    let metadata = fs::metadata(&configured).map_err(|error| error.to_string())?;
    if !metadata.is_file() || metadata.len() == 0 || metadata.len() > maximum {
        return Err(format!("{name} must name a bounded regular file"));
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::{MetadataExt as _, PermissionsExt as _};
        if metadata.permissions().mode() & 0o007 != 0 || metadata.nlink() != 1 {
            return Err(format!(
                "{name} must be process-group private and singly linked"
            ));
        }
    }
    fs::read(configured).map_err(|error| error.to_string())
}

fn tls_config() -> Result<Arc<ServerConfig>, String> {
    rustls::crypto::ring::default_provider()
        .install_default()
        .map_err(|_| "failed to install the registry TLS provider".to_owned())?;
    let certificate = CertificateDer::from(read_bounded_file(
        "LAYERX_REGISTRY_TLS_CERT_DER",
        64 * 1024,
    )?);
    let private_key =
        PrivateKeyDer::try_from(read_private_file("LAYERX_REGISTRY_TLS_KEY_DER", 64 * 1024)?)
            .map_err(|_| "registry TLS private key is invalid".to_owned())?;
    let client_ca = CertificateDer::from(read_bounded_file(
        "LAYERX_REGISTRY_CLIENT_CA_DER",
        64 * 1024,
    )?);
    let mut roots = RootCertStore::empty();
    roots
        .add(client_ca)
        .map_err(|_| "registry client CA is invalid".to_owned())?;
    let verifier = WebPkiClientVerifier::builder(roots.into())
        .build()
        .map_err(|_| "registry client certificate verifier is invalid".to_owned())?;
    ServerConfig::builder()
        .with_client_cert_verifier(verifier)
        .with_single_cert(vec![certificate], private_key)
        .map(Arc::new)
        .map_err(|_| "registry TLS identity is invalid".to_owned())
}

fn parse_path(name: &str, default: PathBuf) -> PathBuf {
    env::var(name).map_or(default, PathBuf::from)
}

fn config(builder_cgroup_root: &Path) -> Result<Config, String> {
    let root = parse_path("LAYERX_REGISTRY_STATE", PathBuf::from(DEFAULT_ROOT));
    let digest = env::var("LAYERX_REGISTRY_BUILDER_IMAGE_DIGEST")
        .map_err(|_| "LAYERX_REGISTRY_BUILDER_IMAGE_DIGEST is required".to_owned())?;
    let request_authority =
        RegistryAuthority::new(read_secret("LAYERX_REGISTRY_REQUEST_TOKEN_FILE")?)?;
    let publication_authority =
        RegistryAuthority::new(read_secret("LAYERX_REGISTRY_PUBLICATION_TOKEN_FILE")?)?;
    if request_authority.same_as(&publication_authority) {
        return Err("request and publication authorities must be distinct".to_owned());
    }
    let request_timeout_seconds = parse_u64("LAYERX_REGISTRY_REQUEST_TIMEOUT_SECONDS", 1_800)?;
    if !(1..=3_600).contains(&request_timeout_seconds) {
        return Err("LAYERX_REGISTRY_REQUEST_TIMEOUT_SECONDS is outside its bound".to_owned());
    }
    Ok(Config {
        listen: env::var("LAYERX_REGISTRY_LISTEN").unwrap_or_else(|_| DEFAULT_LISTEN.to_owned()),
        journal: parse_path("LAYERX_REGISTRY_JOURNAL", root.join("journal")),
        mirror: parse_path("LAYERX_REGISTRY_SOURCE_MIRROR", root.join("sources")),
        verified: parse_path("LAYERX_REGISTRY_VERIFIED", root.join("verified")),
        workspace: parse_path("LAYERX_REGISTRY_BUILD_ROOT", root.join("builds")),
        builder_image_digest: hex::decode_digest(&digest)
            .map_err(|error| format!("LAYERX_REGISTRY_BUILDER_IMAGE_DIGEST is invalid: {error}"))?,
        builder_environment_root: PathBuf::from(
            env::var("LAYERX_REGISTRY_BUILDER_ENVIRONMENT_ROOT")
                .map_err(|_| "LAYERX_REGISTRY_BUILDER_ENVIRONMENT_ROOT is required".to_owned())?,
        ),
        builder_entrypoint: env::var("LAYERX_REGISTRY_BUILDER_ENTRYPOINT")
            .map_err(|_| "LAYERX_REGISTRY_BUILDER_ENTRYPOINT is required".to_owned())?,
        builder_isolation_runtime: PathBuf::from(
            env::var("LAYERX_REGISTRY_BUILDER_ISOLATION_RUNTIME")
                .map_err(|_| "LAYERX_REGISTRY_BUILDER_ISOLATION_RUNTIME is required".to_owned())?,
        ),
        builder_isolation_runtime_digest: hex::decode_digest(
            &env::var("LAYERX_REGISTRY_BUILDER_ISOLATION_RUNTIME_DIGEST").map_err(|_| {
                "LAYERX_REGISTRY_BUILDER_ISOLATION_RUNTIME_DIGEST is required".to_owned()
            })?,
        )
        .map_err(|error| format!("builder isolation runtime digest is invalid: {error}"))?,
        builder_job_supervisor: PathBuf::from(
            env::var("LAYERX_REGISTRY_BUILDER_JOB_SUPERVISOR")
                .map_err(|_| "LAYERX_REGISTRY_BUILDER_JOB_SUPERVISOR is required".to_owned())?,
        ),
        builder_job_supervisor_digest: hex::decode_digest(
            &env::var("LAYERX_REGISTRY_BUILDER_JOB_SUPERVISOR_DIGEST").map_err(|_| {
                "LAYERX_REGISTRY_BUILDER_JOB_SUPERVISOR_DIGEST is required".to_owned()
            })?,
        )
        .map_err(|error| format!("builder job supervisor digest is invalid: {error}"))?,
        builder_cgroup_root: builder_cgroup_root.to_path_buf(),
        build_timeout_seconds: parse_u64("LAYERX_REGISTRY_BUILD_TIMEOUT_SECONDS", 1_800)?,
        build_memory_bytes: parse_u64("LAYERX_REGISTRY_BUILD_MEMORY_BYTES", 2_147_483_648)?,
        build_process_limit: parse_u32("LAYERX_REGISTRY_BUILD_PROCESS_LIMIT", 64)?,
        build_file_size_bytes: parse_u64("LAYERX_REGISTRY_BUILD_FILE_SIZE_BYTES", 67_108_864)?,
        attempts: parse_u32("LAYERX_REGISTRY_ATTEMPTS", 2)?,
        staleness_ms: parse_u64("LAYERX_REGISTRY_MAX_STALENESS_SECONDS", 300)?
            .checked_mul(1_000)
            .ok_or_else(|| "LAYERX_REGISTRY_MAX_STALENESS_SECONDS is too large".to_owned())?,
        node_endpoint: env::var("LAYERX_REGISTRY_NODE_ENDPOINT")
            .map_err(|_| "LAYERX_REGISTRY_NODE_ENDPOINT is required".to_owned())?,
        node_authorization: env::var("LAYERX_REGISTRY_NODE_AUTHORIZATION")
            .map_err(|_| "LAYERX_REGISTRY_NODE_AUTHORIZATION is required".to_owned())?,
        outbound_ca_der: fs::read(
            env::var("LAYERX_REGISTRY_OUTBOUND_CA_DER")
                .map_err(|_| "LAYERX_REGISTRY_OUTBOUND_CA_DER is required".to_owned())?,
        )
        .map_err(|error| format!("LAYERX_REGISTRY_OUTBOUND_CA_DER is unreadable: {error}"))?,
        receipt_authority_endpoint: env::var("LAYERX_REGISTRY_RECEIPT_AUTHORITY_ENDPOINT")
            .map_err(|_| "LAYERX_REGISTRY_RECEIPT_AUTHORITY_ENDPOINT is required".to_owned())?,
        receipt_authority_authorization: env::var(
            "LAYERX_REGISTRY_RECEIPT_AUTHORITY_AUTHORIZATION",
        )
        .map_err(|_| "LAYERX_REGISTRY_RECEIPT_AUTHORITY_AUTHORIZATION is required".to_owned())?,
        receipt_authority_replica_id: hex::decode_digest(
            &env::var("LAYERX_REGISTRY_RECEIPT_AUTHORITY_REPLICA_ID").map_err(|_| {
                "LAYERX_REGISTRY_RECEIPT_AUTHORITY_REPLICA_ID is required".to_owned()
            })?,
        )
        .map_err(|error| {
            format!("LAYERX_REGISTRY_RECEIPT_AUTHORITY_REPLICA_ID is invalid: {error}")
        })?,
        sequencer_trust_history: PathBuf::from(
            env::var("LAYERX_REGISTRY_SEQUENCER_TRUST_HISTORY")
                .map_err(|_| "LAYERX_REGISTRY_SEQUENCER_TRUST_HISTORY is required".to_owned())?,
        ),
        request_authority,
        publication_authority,
        request_timeout_seconds,
        max_connections: parse_usize("LAYERX_REGISTRY_MAX_CONNECTIONS", 128, 1_024)?,
        max_builds: parse_usize("LAYERX_REGISTRY_MAX_BUILDS", 4, 64)?,
        tls: tls_config()?,
    })
}

const MAX_WORKER_IPC_BYTES: u64 = 32 * 1024 * 1024 + 65_536;

struct WorkerCgroup {
    path: PathBuf,
    worker_leaf: PathBuf,
    build_root: PathBuf,
    kill_file: File,
}

struct CgroupCreation {
    path: PathBuf,
    committed: bool,
}

impl Drop for CgroupCreation {
    fn drop(&mut self) {
        if !self.committed {
            remove_cgroup_tree(&self.path);
        }
    }
}

impl WorkerCgroup {
    fn create() -> Result<Self, String> {
        let root = WORKER_ROOT
            .get()
            .ok_or("worker cgroup root is unavailable")?;
        let path = root.join(format!("request-{}-{}", std::process::id(), now()));
        fs::create_dir(&path).map_err(|error| format!("worker cgroup creation failed: {error}"))?;
        let mut creation = CgroupCreation {
            path: path.clone(),
            committed: false,
        };
        let worker_leaf = path.join("worker");
        let build_root = path.join("builds");
        fs::write(
            path.join("cgroup.subtree_control"),
            b"+cpu +memory +pids +io",
        )
        .map_err(|error| format!("worker cgroup delegation failed: {error}"))?;
        fs::create_dir(&worker_leaf)
            .map_err(|error| format!("worker leaf creation failed: {error}"))?;
        fs::create_dir(&build_root)
            .map_err(|error| format!("build cgroup root creation failed: {error}"))?;
        fs::write(
            build_root.join("cgroup.subtree_control"),
            b"+cpu +memory +pids +io",
        )
        .map_err(|error| format!("build cgroup delegation failed: {error}"))?;
        let kill_file = fs::OpenOptions::new()
            .write(true)
            .open(path.join("cgroup.kill"))
            .map_err(|error| format!("worker cgroup kill boundary failed: {error}"))?;
        creation.committed = true;
        Ok(Self {
            path,
            worker_leaf,
            build_root,
            kill_file,
        })
    }

    fn attach(&self, pid: u32) -> Result<(), String> {
        fs::write(self.worker_leaf.join("cgroup.procs"), pid.to_string())
            .map_err(|error| format!("worker cgroup attachment failed: {error}"))
    }

    fn kill(&self) {
        let mut kill_file = &self.kill_file;
        let _ = kill_file.write_all(b"1");
    }
}

impl Drop for WorkerCgroup {
    fn drop(&mut self) {
        self.kill();
        for _ in 0..100 {
            let empty = fs::read_to_string(self.path.join("cgroup.events"))
                .ok()
                .is_some_and(|events| events.lines().any(|line| line == "populated 0"));
            if !empty {
                thread::sleep(Duration::from_millis(10));
                continue;
            }
            remove_cgroup_tree(&self.path);
            if !self.path.exists() {
                return;
            }
            thread::sleep(Duration::from_millis(10));
        }
    }
}

fn remove_cgroup_tree(root: &std::path::Path) {
    if let Ok(entries) = fs::read_dir(root) {
        for entry in entries.flatten() {
            if entry.file_type().is_ok_and(|kind| kind.is_dir()) {
                remove_cgroup_tree(&entry.path());
            }
        }
    }
    let _ = fs::remove_dir(root);
}

fn process_stopped(pid: u32) -> bool {
    fs::read_to_string(format!("/proc/{pid}/status"))
        .ok()
        .is_some_and(|status| {
            status
                .lines()
                .find(|line| line.starts_with("State:"))
                .is_some_and(|state| state.contains('T'))
        })
}

fn reclaim_worker_cgroups() -> Result<(), String> {
    let root = WORKER_ROOT
        .get()
        .ok_or("worker cgroup root is unavailable")?;
    for entry in fs::read_dir(root).map_err(|error| error.to_string())? {
        let entry = entry.map_err(|error| error.to_string())?;
        if entry
            .file_type()
            .map_err(|error| error.to_string())?
            .is_dir()
            && (entry.file_name().to_string_lossy().starts_with("request-")
                || entry.file_name().to_string_lossy().starts_with("job-"))
        {
            fs::write(entry.path().join("cgroup.kill"), b"1")
                .map_err(|error| format!("stale worker cgroup cannot be killed: {error}"))?;
            for _ in 0..100 {
                remove_cgroup_tree(&entry.path());
                if !entry.path().exists() {
                    break;
                }
                thread::sleep(Duration::from_millis(10));
            }
            if entry.path().exists() {
                return Err("stale worker cgroup could not be reclaimed".to_owned());
            }
        }
    }
    Ok(())
}

fn request_worker(remaining_ms: u64, build_root: &Path) -> Result<(), String> {
    if remaining_ms == 0 {
        return Err("worker deadline is empty".to_owned());
    }
    let mut encoded = Vec::new();
    io::stdin()
        .take(MAX_WORKER_IPC_BYTES)
        .read_to_end(&mut encoded)
        .map_err(|error| error.to_string())?;
    if u64::try_from(encoded.len()).map_or(true, |length| length >= MAX_WORKER_IPC_BYTES) {
        return Err("worker request exceeds bounded IPC".to_owned());
    }
    let (request, builder): (layerx_platform_registry::Request, HermeticBuilder) =
        serde_json::from_slice(&encoded).map_err(|error| error.to_string())?;
    let config = config(build_root)?;
    let deadline = Instant::now()
        .checked_add(Duration::from_millis(remaining_ms))
        .ok_or_else(|| "worker deadline is invalid".to_owned())?;
    let builder = builder.bind_worker(&config)?;
    let response =
        Registrar::open_with_builder(&config, now(), builder, request.path == "/healthz")?.route(
            &request,
            now(),
            deadline,
        );
    serde_json::to_writer(io::stdout().lock(), &response).map_err(|error| error.to_string())
}

fn isolated_route(
    builder: &HermeticBuilder,
    request: &layerx_platform_registry::Request,
    deadline: Instant,
) -> layerx_platform_registry::Response {
    let Some(remaining) = deadline.checked_duration_since(Instant::now()) else {
        return refusal(
            503,
            "request_deadline_exceeded",
            "the registry request deadline expired",
        );
    };
    let encoded = match serde_json::to_vec(&(request, builder)) {
        Ok(encoded)
            if u64::try_from(encoded.len()).is_ok_and(|length| length < MAX_WORKER_IPC_BYTES) =>
        {
            encoded
        }
        _ => {
            return refusal(
                503,
                "worker_unavailable",
                "the bounded request worker IPC refused the request",
            )
        }
    };
    let Ok(executable) = env::current_exe() else {
        return refusal(
            503,
            "worker_unavailable",
            "the request worker executable is unavailable",
        );
    };
    let Ok(worker_group) = WorkerCgroup::create() else {
        return refusal(
            503,
            "worker_unavailable",
            "the request worker cgroup is unavailable",
        );
    };
    let Ok(mut child) = Command::new(executable)
        .arg("--stopped-request-worker")
        .arg(remaining.as_millis().to_string())
        .arg(&worker_group.build_root)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
    else {
        return refusal(
            503,
            "worker_unavailable",
            "the request worker could not start",
        );
    };
    let pid = child.id();
    while !process_stopped(pid) {
        if Instant::now() >= deadline {
            let _ = child.kill();
            let _ = child.wait();
            return refusal(
                503,
                "request_deadline_exceeded",
                "the request worker expired before attachment",
            );
        }
        thread::sleep(Duration::from_millis(1));
    }
    if worker_group.attach(pid).is_err() {
        let _ = child.kill();
        let _ = child.wait();
        return refusal(
            503,
            "worker_unavailable",
            "the request worker could not be attached",
        );
    }
    let Some(raw_pid) = i32::try_from(pid).ok().and_then(Pid::from_raw) else {
        return refusal(
            503,
            "worker_unavailable",
            "the request worker pid is invalid",
        );
    };
    if kill_process(raw_pid, Signal::CONT).is_err() {
        let _ = child.kill();
        let _ = child.wait();
        return refusal(
            503,
            "worker_unavailable",
            "the request worker could not continue",
        );
    }
    complete_worker(&mut child, &worker_group, encoded, deadline)
}

fn serve(config: &Config) -> Result<(), String> {
    reclaim_worker_cgroups()?;
    let registrar = Registrar::open(config, now())?;
    for unit in registrar.quarantined_units() {
        eprintln!("layerx-program-registry: {unit}");
    }
    let builder = registrar.verified_builder();
    drop(registrar);
    let builder_ready = Arc::new(Mutex::new(Some(Instant::now())));
    let monitor_ready = Arc::clone(&builder_ready);
    let mut monitored_builder = builder.clone();
    thread::spawn(move || loop {
        let checked = Instant::now();
        let valid = if monitored_builder.check_environment_metadata().is_ok() {
            true
        } else {
            if let Ok(mut ready) = monitor_ready.lock() {
                *ready = None;
            }
            monitored_builder.reverify_environment().is_ok()
        };
        if let Ok(mut ready) = monitor_ready.lock() {
            *ready = valid.then_some(checked);
        }
        thread::sleep(Duration::from_millis(250));
    });
    let service = Arc::new(Service {
        builder,
        builder_ready,
        registrar_gate: Mutex::new(()),
        request_authority: config.request_authority.clone(),
        publication_authority: config.publication_authority.clone(),
        active_builds: AtomicUsize::new(0),
        max_builds: config.max_builds,
        timeout: Duration::from_secs(config.request_timeout_seconds),
    });
    let listener = TcpListener::bind(&config.listen).map_err(|error| error.to_string())?;
    eprintln!(
        "LayerX program registry ready on {} with journal {} and source mirror {}",
        config.listen,
        config.journal.display(),
        config.mirror.display()
    );
    for connection in listener.incoming() {
        match connection {
            Ok(stream) => {
                if ACTIVE_CONNECTIONS.fetch_add(1, Ordering::AcqRel) >= config.max_connections {
                    ACTIVE_CONNECTIONS.fetch_sub(1, Ordering::AcqRel);
                    let _ = stream.shutdown(std::net::Shutdown::Both);
                    continue;
                }
                let service = Arc::clone(&service);
                let tls = Arc::clone(&config.tls);
                let Some(deadline) = Instant::now().checked_add(service.timeout) else {
                    let _ = stream.shutdown(std::net::Shutdown::Both);
                    continue;
                };
                thread::spawn(move || {
                    serve_connection(stream, tls, &service, deadline);
                });
            }
            Err(error) => eprintln!("program registry accept error: {error}"),
        }
    }
    Ok(())
}

fn main() {
    let mut arguments = env::args().skip(1);
    if arguments.next().as_deref() == Some("--stopped-request-worker") {
        let remaining = arguments
            .next()
            .and_then(|value| value.parse().ok())
            .unwrap_or(0);
        let Some(build_root) = arguments.next().map(PathBuf::from) else {
            std::process::exit(3);
        };
        if verify_dropped_privileges().is_err()
            || kill_process(rustix::process::getpid(), Signal::STOP).is_err()
        {
            std::process::exit(3);
        }
        if let Err(error) = request_worker(remaining, &build_root) {
            eprintln!("layerx-program-registry worker: {error}");
            std::process::exit(3);
        }
        return;
    }
    if let Err(error) = initialize_cgroup_boundary()
        .and_then(|root| config(&root))
        .and_then(|config| serve(&config))
    {
        eprintln!("layerx-program-registry: {error}");
        std::process::exit(2);
    }
}

fn complete_worker(
    child: &mut std::process::Child,
    worker_group: &WorkerCgroup,
    encoded: Vec<u8>,
    deadline: Instant,
) -> layerx_platform_registry::Response {
    let Some(mut input) = child.stdin.take() else {
        return refusal(
            503,
            "worker_unavailable",
            "the request worker input is unavailable",
        );
    };
    let Some(output) = child.stdout.take() else {
        return refusal(
            503,
            "worker_unavailable",
            "the request worker output is unavailable",
        );
    };
    let writer = thread::spawn(move || input.write_all(&encoded));
    let reader = thread::spawn(move || {
        let mut bytes = Vec::new();
        output
            .take(MAX_WORKER_IPC_BYTES)
            .read_to_end(&mut bytes)
            .map(|_| bytes)
    });
    let status = loop {
        match child.try_wait() {
            Ok(Some(status)) => break Some(status),
            Ok(None) if Instant::now() < deadline => thread::sleep(Duration::from_millis(1)),
            _ => {
                worker_group.kill();
                let _ = child.wait();
                break None;
            }
        }
    };
    let _ = writer.join();
    let bytes = reader.join().ok().and_then(Result::ok).unwrap_or_default();
    if status.is_none() {
        return refusal(
            503,
            "request_deadline_exceeded",
            "the isolated request worker was cancelled at its deadline",
        );
    }
    if !status.is_some_and(|status| status.success())
        || u64::try_from(bytes.len()).map_or(true, |length| length >= MAX_WORKER_IPC_BYTES)
    {
        return refusal(
            503,
            "worker_unavailable",
            "the bounded request worker refused completion",
        );
    }
    serde_json::from_slice(&bytes).unwrap_or_else(|_| {
        refusal(
            503,
            "worker_unavailable",
            "the request worker returned an invalid response",
        )
    })
}

fn route_request(
    service: &Service,
    request: &layerx_platform_registry::Request,
    deadline: Instant,
) -> layerx_platform_registry::Response {
    let header = request.headers.get("authorization").map(String::as_str);
    let authenticated = if request.path == "/healthz" {
        true
    } else if request.path == "/__registry/sources" {
        service.publication_authority.verifies(header)
    } else {
        service.request_authority.verifies(header)
    };
    if !authenticated {
        return refusal(
            401,
            "authentication_required",
            "a valid registry authority is required",
        );
    }
    if service
        .builder_ready
        .lock()
        .ok()
        .and_then(|ready| *ready)
        .is_none_or(|checked| checked.elapsed() >= Duration::from_secs(2))
    {
        return refusal(
            503,
            "builder_unavailable",
            "startup-verified builder state is unavailable or changed",
        );
    }
    if request.path == "/healthz" {
        return isolated_route(&service.builder, request, deadline);
    }
    let is_build = request.method == "POST"
        && request.path.starts_with("/v1/programs/registry/")
        && request.path.ends_with("/source");
    let _build = if is_build {
        if service.active_builds.fetch_add(1, Ordering::AcqRel) >= service.max_builds {
            service.active_builds.fetch_sub(1, Ordering::AcqRel);
            return refusal(503, "build_queue_full", "the bounded build queue is full");
        }
        Some(BuildGuard(&service.active_builds))
    } else {
        None
    };
    let _registrar_gate = loop {
        match service.registrar_gate.try_lock() {
            Ok(gate) => break gate,
            Err(TryLockError::Poisoned(_)) => {
                return refusal(
                    503,
                    "registry_unavailable",
                    "registry state lock is unavailable",
                );
            }
            Err(TryLockError::WouldBlock) => {
                if Instant::now() >= deadline {
                    return refusal(
                        503,
                        "request_deadline_exceeded",
                        "the registry request deadline expired in the bounded queue",
                    );
                }
                thread::sleep(Duration::from_millis(1));
            }
        }
    };
    isolated_route(&service.builder, request, deadline)
}

fn serve_connection(
    stream: std::net::TcpStream,
    tls: Arc<ServerConfig>,
    service: &Service,
    deadline: Instant,
) {
    let _connection = ConnectionGuard;
    let Ok(watchdog_socket) = stream.try_clone() else {
        return;
    };
    let (completed, completion) = mpsc::channel();
    let watchdog_wait = deadline.saturating_duration_since(Instant::now());
    thread::spawn(move || {
        if completion.recv_timeout(watchdog_wait).is_err() {
            let _ = watchdog_socket.shutdown(std::net::Shutdown::Both);
        }
    });
    let _watchdog = WatchdogCompletion(completed);
    let Ok(connection) = ServerConnection::new(tls) else {
        return;
    };
    let mut stream = DeadlineStream {
        inner: StreamOwned::new(connection, stream),
        deadline,
    };
    let response = parse_request(&mut stream).map_or_else(
        |_| refusal(400, "invalid_request", "request could not be parsed"),
        |request| route_request(service, &request, deadline),
    );
    let response = if Instant::now() >= deadline {
        refusal(
            503,
            "request_deadline_exceeded",
            "the registry request deadline expired",
        )
    } else {
        response
    };
    let Some(remaining) = deadline.checked_duration_since(Instant::now()) else {
        return;
    };
    if stream
        .inner
        .sock
        .set_write_timeout(Some(remaining))
        .is_err()
    {
        return;
    }
    let _ = write_response(&mut stream, &response);
}

#[cfg(test)]
mod tests {
    const REGISTRY_DEPLOYMENT: &str = include_str!("../deployment.yaml");
    const GATEWAY_DEPLOYMENT: &str = include_str!("../../gateway/deployment.yaml");
    const BUILDER_SOURCE: &str = include_str!("builder.rs");
    const CGROUP_SUPERVISOR_SOURCE: &str = include_str!("bin/layerx-cgroup-exec.rs");
    const MAIN_SOURCE: &str = include_str!("main.rs");
    const PROVISIONER_SOURCE: &str = include_str!("../node-provision-build-boundary.sh");
    const NODE_UNIT: &str = include_str!("../layerx-program-registry-boundary.service");

    fn assert_container_delegation_contract() {
        assert!(REGISTRY_DEPLOYMENT.contains("readinessProbe: {tcpSocket: {port: registry}"));
        assert!(!REGISTRY_DEPLOYMENT.contains("LAYERX_REGISTRY_BUILDER_CGROUP_ROOT"));
        assert!(REGISTRY_DEPLOYMENT
            .contains("{name: host-cgroup, mountPath: /run/layerx/host-cgroup, readOnly: false}"));
        assert!(REGISTRY_DEPLOYMENT.contains(
            "{name: host-cgroup, hostPath: {path: /sys/fs/cgroup/kubelet.slice, type: Directory}}"
        ));
        let (pod, registry) = REGISTRY_DEPLOYMENT
            .split_once("      containers:")
            .ok_or("missing containers")
            .unwrap_or_else(|e| panic!("{e}"));
        assert!(!pod.contains("runAsUser: 0"));
        assert!(
            pod.contains("runAsNonRoot: true, runAsUser: 4030, runAsGroup: 4030, fsGroup: 4030")
        );
        assert!(registry.contains("securityContext: {runAsUser: 0, runAsGroup: 0, runAsNonRoot: false, allowPrivilegeEscalation: false, readOnlyRootFilesystem: true, capabilities: {drop: [ALL], add: [CHOWN, SETUID, SETGID]}, seccompProfile: {type: RuntimeDefault}}"));
        assert_eq!(registry.matches("securityContext:").count(), 1);
        assert_eq!(registry.matches("capabilities:").count(), 1);
        let source = MAIN_SOURCE.split("#[cfg(test)]").next().unwrap_or_default();
        for required in [
            "geteuid().is_root()",
            "metadata.dev() != namespace.dev()",
            "metadata.ino() == namespace.ino()",
            "depth < 8",
            "found.replace(path.clone()).is_some()",
            "cgroup.procs",
            "cgroup.subtree_control",
            "set_keepcaps(false)",
            "setgroups(&[gid])",
            "setresgid(gid, gid, gid)",
            "setresuid(uid, uid, uid)",
            "CapEff:",
            "CapPrm:",
            "CapInh:",
            "[\"4030\"; 4]",
            "Err(nix::errno::Errno::EPERM)",
        ] {
            assert!(source.contains(required), "missing {required}");
        }
        let drop = source
            .split("fn drop_registry_privileges()")
            .nth(1)
            .unwrap_or_default();
        let mut previous = 0;
        for step in [
            "set_keepcaps(false)",
            "setgroups(&[gid])",
            "setresgid(gid, gid, gid)",
            "setresuid(uid, uid, uid)",
            "verify_dropped_privileges()",
        ] {
            let position = drop.find(step).unwrap_or_else(|| panic!("missing {step}"));
            assert!(position > previous);
            previous = position;
        }
        assert!(source
            .contains("initialize_cgroup_boundary()\n        .and_then(|root| config(&root))"));
    }

    #[test]
    fn deployment_contract_keeps_https_mtls_and_bearer_roles_aligned() {
        assert!(GATEWAY_DEPLOYMENT
            .contains("https://layerx-program-registry.layerx-testnet.svc.cluster.local:9420"));
        for required in [
            "LAYERX_REGISTRY_TLS_CERT_DER",
            "LAYERX_REGISTRY_TLS_KEY_DER",
            "LAYERX_REGISTRY_CLIENT_CA_DER",
            "LAYERX_REGISTRY_REQUEST_TOKEN_FILE",
            "LAYERX_REGISTRY_PUBLICATION_TOKEN_FILE",
            "layerx-program-registry-server-tls",
            "layerx-internal-ca",
            "LAYERX_REGISTRY_BUILDER_ENVIRONMENT_ROOT",
            "LAYERX_REGISTRY_BUILDER_ISOLATION_RUNTIME_DIGEST",
            "LAYERX_REGISTRY_BUILDER_JOB_SUPERVISOR_DIGEST",
            "LAYERX_REGISTRY_HOST_CGROUP_MOUNT",
            "LAYERX_REGISTRY_BUILD_MEMORY_BYTES",
            "LAYERX_REGISTRY_BUILD_PROCESS_LIMIT",
        ] {
            assert!(REGISTRY_DEPLOYMENT.contains(required));
        }
        assert!(!REGISTRY_DEPLOYMENT.contains("httpGet: {path: /healthz"));
        assert_container_delegation_contract();
        assert!(GATEWAY_DEPLOYMENT.contains(
            "{app: layerx-program-registry}}}]\n      ports: [{protocol: TCP, port: 9420}]"
        ));
        for enforced in [
            "--unshare-all",
            "--disable-userns",
            "--cap-drop",
            "--ro-bind",
            "--attach-before-exec",
            "--cpu-time-max-usec=",
            "environment_digest(&workspace.root.join(\"environment\"), Some(deadline))",
            "openat2(",
            "ResolveFlags::BENEATH",
        ] {
            assert!(BUILDER_SOURCE.contains(enforced));
        }
        for deadline_boundary in [
            "checked_duration_since(Instant::now())",
            "completion.recv_timeout(remaining)",
            "--stopped-request-worker",
            "worker_group.kill()",
            "MAX_WORKER_IPC_BYTES",
        ] {
            assert!(MAIN_SOURCE.contains(deadline_boundary));
        }
        assert!(!MAIN_SOURCE.contains(concat!("std::process::", "abort()")));
        for aggregate_boundary in [
            "memory.max",
            "memory.oom.group",
            "pids.max",
            "cgroup.procs",
            "cgroup.kill",
            "Signal::STOP",
            "Signal::CONT",
            "cpu.stat",
            "io.stat",
            "io.max",
        ] {
            assert!(CGROUP_SUPERVISOR_SOURCE.contains(aggregate_boundary));
        }
        for quota_boundary in [
            "mkfs.ext4",
            "-N",
            "-O AUTOCLEAR",
            "mountpoint -q",
            "e2fsck -p",
        ] {
            assert!(PROVISIONER_SOURCE.contains(quota_boundary));
        }
    }

    #[test]
    fn request_deadline_cancels_only_the_bounded_worker_and_preserves_listener_liveness() {
        for boundary in [
            "registrar_gate: Mutex<()>",
            "--stopped-request-worker",
            "worker_group.kill()",
            "cgroup.kill",
            "child.wait()",
        ] {
            assert!(MAIN_SOURCE.contains(boundary));
        }
        assert!(!MAIN_SOURCE.contains(concat!("std::process::", "abort()")));
    }

    #[test]
    fn delegation_quota_and_open_inode_execution_fail_closed() {
        assert_container_delegation_contract();
        for boundary in [
            "mountpoint -q",
            "losetup -j",
            "e2fsck -p",
            "mkfs.ext4",
            "-N",
            "-O AUTOCLEAR",
            "stat -c %u:%g",
        ] {
            assert!(PROVISIONER_SOURCE.contains(boundary));
        }
        for boundary in [
            "CapabilityBoundingSet=CAP_CHOWN CAP_DAC_OVERRIDE",
            "AmbientCapabilities=CAP_CHOWN CAP_DAC_OVERRIDE",
            "PrivateTmp=yes",
            "ProtectHome=yes",
            "NoNewPrivileges=yes",
            "Before=kubelet.service",
            "ProtectSystem=strict",
            "ReadWritePaths=/var/lib/layerx-program-registry-builds /run/lock",
        ] {
            assert!(NODE_UNIT.contains(boundary));
        }
        for boundary in [
            "systemd-mount --no-ask-password --collect --automount=no",
            "--type=ext4 --options=loop,nosuid,nodev,noatime",
            "--property=Before=kubelet.service",
            "for option in rw nosuid nodev noatime",
            "stat -c %d",
        ] {
            assert!(PROVISIONER_SOURCE.contains(boundary));
        }
        assert!(!NODE_UNIT.contains("CAP_SYS_ADMIN"));
        assert!(!NODE_UNIT.contains("cgroup"));
        assert!(!PROVISIONER_SOURCE.to_lowercase().contains("cgroup"));
        assert!(MAIN_SOURCE
            .split("#[cfg(test)]")
            .next()
            .unwrap_or_default()
            .contains("cgroup.subtree_control"));
        assert!(!PROVISIONER_SOURCE.lines().any(|line| {
            let command = line.trim_start();
            command.starts_with("mount ") || command.contains("losetup --find")
        }));
        assert!(REGISTRY_DEPLOYMENT.contains("layerx.io/program-registry-boundary: \"v2\""));
        for boundary in [
            "NonBlockingLockExclusive",
            "sync_all",
            "metadata.dev() != root_device",
            "fcntl_setfd",
            "/proc/self/fd/",
        ] {
            assert!(BUILDER_SOURCE.contains(boundary));
        }
    }
    #[test]
    fn discovery_excludes_non_root_owned_subtrees() -> Result<(), Box<dyn std::error::Error>> {
        use super::*;
        let root = env::temp_dir().join(format!(
            "registry-discovery-{}-{}",
            std::process::id(),
            now()
        ));
        fs::create_dir(&root)?;
        let result = (|| -> Result<(), Box<dyn std::error::Error>> {
            let excluded = root.join("excluded");
            fs::create_dir(&excluded)?;
            let metadata = fs::metadata(&excluded)?;
            assert_eq!(discovery_owner_allowed(&metadata), metadata.uid() == 0);
            if !nix::unistd::geteuid().is_root() {
                assert!(!discovery_owner_allowed(&metadata));
                return Ok(());
            }
            let hidden = excluded.join("hidden");
            fs::create_dir(&hidden)?;
            nix::unistd::chown(
                &excluded,
                Some(nix::unistd::Uid::from_raw(4030)),
                Some(nix::unistd::Gid::from_raw(4030)),
            )?;
            assert!(!discovery_owner_allowed(&fs::metadata(&excluded)?));
            assert_eq!(
                locate_cgroup_matching(&root, &fs::metadata(&hidden)?),
                Err("container cgroup not found within depth 8".to_owned())
            );
            let matched = root.join("matched");
            fs::create_dir(&matched)?;
            fs::write(matched.join("cgroup.procs"), std::process::id().to_string())?;
            assert_eq!(
                locate_cgroup_matching(&root, &fs::metadata(&matched)?)?,
                matched
            );
            Ok(())
        })();
        fs::remove_dir_all(root)?;
        result
    }

    fn continue_stopped_child(pid: u32) -> Result<(), Box<dyn std::error::Error>> {
        use super::*;
        kill_process(
            Pid::from_raw(i32::try_from(pid)?).ok_or("invalid child pid")?,
            Signal::CONT,
        )?;
        let deadline = Instant::now() + Duration::from_secs(5);
        while process_stopped(pid) && Instant::now() < deadline {
            thread::sleep(Duration::from_millis(1));
        }
        assert!(!process_stopped(pid));
        Ok(())
    }

    #[test]
    fn live_container_delegation_attaches_and_kills_stopped_child(
    ) -> Result<(), Box<dyn std::error::Error>> {
        use super::*;
        use nix::sys::statfs::{statfs, CGROUP2_SUPER_MAGIC};
        use nix::sys::statvfs::FsFlags;
        use std::os::unix::process::ExitStatusExt as _;
        if let Some(root) = env::var_os("LAYERX_REGISTRY_TEST_DELEGATION_CHILD") {
            let root = PathBuf::from(root);
            fs::write(root.join("cgroup.procs"), std::process::id().to_string())?;
            let workers = delegate_container_cgroup(&root)?;
            drop_registry_privileges()?;
            for path in [
                root.join("cgroup.procs"),
                workers.clone(),
                workers.join("cgroup.procs"),
                workers.join("cgroup.subtree_control"),
                workers.join("cgroup.threads"),
            ] {
                let metadata = fs::metadata(path)?;
                assert_eq!((metadata.uid(), metadata.gid()), (4030, 4030));
            }
            for path in [&root, &workers] {
                let controllers = fs::read_to_string(path.join("cgroup.subtree_control"))?;
                for controller in ["cpu", "memory", "pids", "io"] {
                    assert!(controllers
                        .split_whitespace()
                        .any(|value| value == controller));
                }
            }
            WORKER_ROOT
                .set(workers)
                .map_err(|_| "worker root already set")?;
            let group = WorkerCgroup::create()?;
            let mut child = Command::new("/bin/sh")
                .args(["-c", "kill -STOP $$; exec sleep 30"])
                .spawn()?;
            let deadline = Instant::now() + Duration::from_secs(5);
            while !process_stopped(child.id()) && Instant::now() < deadline {
                thread::sleep(Duration::from_millis(1));
            }
            if !process_stopped(child.id()) {
                child.kill()?;
                child.wait()?;
                return Err("child did not stop".into());
            }
            if let Err(error) = group.attach(child.id()) {
                child.kill()?;
                child.wait()?;
                return Err(error.into());
            }
            assert!(fs::read_to_string(group.worker_leaf.join("cgroup.procs"))?
                .lines()
                .any(|pid| pid == child.id().to_string()));
            continue_stopped_child(child.id())?;
            group.kill();
            assert_eq!(child.wait()?.signal(), Some(9));
            let path = group.path.clone();
            drop(group);
            assert!(!path.exists());
            return Ok(());
        }
        if !nix::unistd::geteuid().is_root() {
            eprintln!("SKIP live cgroup delegation: requires root");
            return Ok(());
        }
        let filesystem = statfs("/sys/fs/cgroup")?;
        if filesystem.filesystem_type() != CGROUP2_SUPER_MAGIC
            || filesystem.flags().contains(FsFlags::ST_RDONLY)
        {
            eprintln!("SKIP live cgroup delegation: /sys/fs/cgroup is not writable cgroup2");
            return Ok(());
        }
        let root = PathBuf::from(format!(
            "/sys/fs/cgroup/layerx-registry-test-{}-{}",
            std::process::id(),
            now()
        ));
        fs::create_dir(&root)?;
        let cleanup = CgroupCreation {
            path: root.clone(),
            committed: false,
        };
        let result = Command::new(env::current_exe()?)
            .args([
                "--exact",
                "tests::live_container_delegation_attaches_and_kills_stopped_child",
                "--nocapture",
                "--test-threads=1",
            ])
            .env("LAYERX_REGISTRY_TEST_DELEGATION_CHILD", &root)
            .status();
        fs::write(root.join("cgroup.kill"), "1")?;
        drop(cleanup);
        assert!(result?.success());
        assert!(!root.exists());
        Ok(())
    }
}
