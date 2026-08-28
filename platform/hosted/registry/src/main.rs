//! Executable entry point of the hosted program registry.

use std::env;
use std::fs::{self, File};
use std::io::{self, Read, Write};
use std::net::TcpListener;
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{mpsc, Arc, Mutex, TryLockError};
use std::thread;
use std::time::{Duration, Instant};
use std::time::{SystemTime, UNIX_EPOCH};

use layerx_platform_registry::{parse_request, refusal, write_response, Config, Registrar, RegistryAuthority};
use layerx_programs::hex;
use rustls::pki_types::{CertificateDer, PrivateKeyDer};
use rustls::server::WebPkiClientVerifier;
use rustls::{RootCertStore, ServerConfig, ServerConnection, StreamOwned};
use zeroize::{Zeroize as _, Zeroizing};
use rustix::process::{kill_process, Pid};
use rustix::signal::Signal;

const DEFAULT_LISTEN: &str = "127.0.0.1:9420";
const DEFAULT_ROOT: &str = "/var/lib/layerx-program-registry";
static ACTIVE_CONNECTIONS: AtomicUsize = AtomicUsize::new(0);

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
            .ok_or_else(|| io::Error::new(io::ErrorKind::TimedOut, "absolute request deadline expired"))
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
        value.parse().map_err(|_| format!("{name} must be an integer"))
    })?;
    if value == 0 || value > maximum {
        return Err(format!("{name} is outside its bound"));
    }
    Ok(value)
}

fn read_secret(name: &str) -> Result<Zeroizing<String>, String> {
    let configured = PathBuf::from(env::var(name).map_err(|_| format!("{name} is required"))?);
    if !configured.is_absolute() || fs::canonicalize(&configured).map_err(|error| error.to_string())? != configured {
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
            return Err(format!("{name} must be process-group private and singly linked"));
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
            return Err(format!("{name} must be process-group private and singly linked"));
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
    let private_key = PrivateKeyDer::try_from(read_private_file(
        "LAYERX_REGISTRY_TLS_KEY_DER",
        64 * 1024,
    )?)
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

fn config() -> Result<Config, String> {
    let root = parse_path("LAYERX_REGISTRY_STATE", PathBuf::from(DEFAULT_ROOT));
    let digest = env::var("LAYERX_REGISTRY_BUILDER_IMAGE_DIGEST")
        .map_err(|_| "LAYERX_REGISTRY_BUILDER_IMAGE_DIGEST is required".to_owned())?;
    let request_authority = RegistryAuthority::new(read_secret("LAYERX_REGISTRY_REQUEST_TOKEN_FILE")?)?;
    let publication_authority = RegistryAuthority::new(read_secret("LAYERX_REGISTRY_PUBLICATION_TOKEN_FILE")?)?;
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
        builder_environment_root: PathBuf::from(env::var("LAYERX_REGISTRY_BUILDER_ENVIRONMENT_ROOT")
            .map_err(|_| "LAYERX_REGISTRY_BUILDER_ENVIRONMENT_ROOT is required".to_owned())?),
        builder_entrypoint: env::var("LAYERX_REGISTRY_BUILDER_ENTRYPOINT")
            .map_err(|_| "LAYERX_REGISTRY_BUILDER_ENTRYPOINT is required".to_owned())?,
        builder_isolation_runtime: PathBuf::from(env::var("LAYERX_REGISTRY_BUILDER_ISOLATION_RUNTIME")
            .map_err(|_| "LAYERX_REGISTRY_BUILDER_ISOLATION_RUNTIME is required".to_owned())?),
        builder_isolation_runtime_digest: hex::decode_digest(
            &env::var("LAYERX_REGISTRY_BUILDER_ISOLATION_RUNTIME_DIGEST")
                .map_err(|_| "LAYERX_REGISTRY_BUILDER_ISOLATION_RUNTIME_DIGEST is required".to_owned())?,
        ).map_err(|error| format!("builder isolation runtime digest is invalid: {error}"))?,
        builder_job_supervisor: PathBuf::from(env::var("LAYERX_REGISTRY_BUILDER_JOB_SUPERVISOR")
            .map_err(|_| "LAYERX_REGISTRY_BUILDER_JOB_SUPERVISOR is required".to_owned())?),
        builder_job_supervisor_digest: hex::decode_digest(
            &env::var("LAYERX_REGISTRY_BUILDER_JOB_SUPERVISOR_DIGEST")
                .map_err(|_| "LAYERX_REGISTRY_BUILDER_JOB_SUPERVISOR_DIGEST is required".to_owned())?,
        ).map_err(|error| format!("builder job supervisor digest is invalid: {error}"))?,
        builder_cgroup_root: PathBuf::from(env::var("LAYERX_REGISTRY_BUILDER_CGROUP_ROOT")
            .map_err(|_| "LAYERX_REGISTRY_BUILDER_CGROUP_ROOT is required".to_owned())?),
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
        let root = PathBuf::from(env::var("LAYERX_REGISTRY_BUILDER_CGROUP_ROOT")
            .map_err(|_| "worker cgroup root is unavailable".to_owned())?);
        let path = root.join(format!("request-{}-{}", std::process::id(), now()));
        fs::create_dir(&path).map_err(|error| format!("worker cgroup creation failed: {error}"))?;
        let mut creation = CgroupCreation { path: path.clone(), committed: false };
        let worker_leaf = path.join("worker");
        let build_root = path.join("builds");
        fs::write(path.join("cgroup.subtree_control"), b"+cpu +memory +pids +io")
            .map_err(|error| format!("worker cgroup delegation failed: {error}"))?;
        fs::create_dir(&worker_leaf).map_err(|error| format!("worker leaf creation failed: {error}"))?;
        fs::create_dir(&build_root).map_err(|error| format!("build cgroup root creation failed: {error}"))?;
        fs::write(build_root.join("cgroup.subtree_control"), b"+cpu +memory +pids +io")
            .map_err(|error| format!("build cgroup delegation failed: {error}"))?;
        let kill_file = fs::OpenOptions::new().write(true).open(path.join("cgroup.kill"))
            .map_err(|error| format!("worker cgroup kill boundary failed: {error}"))?;
        creation.committed = true;
        Ok(Self { path, worker_leaf, build_root, kill_file })
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
            let empty = fs::read_to_string(self.path.join("cgroup.events")).ok().is_some_and(|events| {
                events.lines().any(|line| line == "populated 0")
            });
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
    fs::read_to_string(format!("/proc/{pid}/status")).ok().is_some_and(|status| {
        status.lines().find(|line| line.starts_with("State:"))
            .is_some_and(|state| state.contains('T'))
    })
}

fn reclaim_worker_cgroups() -> Result<(), String> {
    let root = PathBuf::from(env::var("LAYERX_REGISTRY_BUILDER_CGROUP_ROOT")
        .map_err(|_| "worker cgroup root is unavailable".to_owned())?);
    for entry in fs::read_dir(&root).map_err(|error| error.to_string())? {
        let entry = entry.map_err(|error| error.to_string())?;
        if entry.file_type().map_err(|error| error.to_string())?.is_dir()
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

fn request_worker(remaining_ms: u64) -> Result<(), String> {
    if remaining_ms == 0 {
        return Err("worker deadline is empty".to_owned());
    }
    let mut encoded = Vec::new();
    io::stdin().take(MAX_WORKER_IPC_BYTES).read_to_end(&mut encoded)
        .map_err(|error| error.to_string())?;
    if u64::try_from(encoded.len()).map_or(true, |length| length >= MAX_WORKER_IPC_BYTES) {
        return Err("worker request exceeds bounded IPC".to_owned());
    }
    let request = serde_json::from_slice(&encoded).map_err(|error| error.to_string())?;
    let config = config()?;
    let deadline = Instant::now().checked_add(Duration::from_millis(remaining_ms))
        .ok_or_else(|| "worker deadline is invalid".to_owned())?;
    let response = Registrar::open(&config, now())?.route(&request, now(), deadline);
    serde_json::to_writer(io::stdout().lock(), &response).map_err(|error| error.to_string())
}

fn isolated_route(request: &layerx_platform_registry::Request, deadline: Instant) -> layerx_platform_registry::Response {
    let Some(remaining) = deadline.checked_duration_since(Instant::now()) else {
        return refusal(503, "request_deadline_exceeded", "the registry request deadline expired");
    };
    let encoded = match serde_json::to_vec(request) {
        Ok(encoded) if u64::try_from(encoded.len()).is_ok_and(|length| length < MAX_WORKER_IPC_BYTES) => encoded,
        _ => return refusal(503, "worker_unavailable", "the bounded request worker IPC refused the request"),
    };
    let executable = match env::current_exe() {
        Ok(executable) => executable,
        Err(_) => return refusal(503, "worker_unavailable", "the request worker executable is unavailable"),
    };
    let worker_group = match WorkerCgroup::create() {
        Ok(group) => group,
        Err(_) => return refusal(503, "worker_unavailable", "the request worker cgroup is unavailable"),
    };
    let mut child = match Command::new(executable)
        .arg("--stopped-request-worker")
        .arg(remaining.as_millis().to_string())
        .env("LAYERX_REGISTRY_BUILDER_CGROUP_ROOT", &worker_group.build_root)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
    {
        Ok(child) => child,
        Err(_) => return refusal(503, "worker_unavailable", "the request worker could not start"),
    };
    let pid = child.id();
    while !process_stopped(pid) {
        if Instant::now() >= deadline {
            let _ = child.kill();
            let _ = child.wait();
            return refusal(503, "request_deadline_exceeded", "the request worker expired before attachment");
        }
        thread::sleep(Duration::from_millis(1));
    }
    if worker_group.attach(pid).is_err() {
        let _ = child.kill();
        let _ = child.wait();
        return refusal(503, "worker_unavailable", "the request worker could not be attached");
    }
    let raw_pid = match i32::try_from(pid).ok().and_then(Pid::from_raw) {
        Some(pid) => pid,
        None => return refusal(503, "worker_unavailable", "the request worker pid is invalid"),
    };
    if kill_process(raw_pid, Signal::Cont).is_err() {
        let _ = child.kill();
        let _ = child.wait();
        return refusal(503, "worker_unavailable", "the request worker could not continue");
    }
    let mut input = match child.stdin.take() {
        Some(input) => input,
        None => return refusal(503, "worker_unavailable", "the request worker input is unavailable"),
    };
    let mut output = match child.stdout.take() {
        Some(output) => output,
        None => return refusal(503, "worker_unavailable", "the request worker output is unavailable"),
    };
    let writer = thread::spawn(move || input.write_all(&encoded));
    let reader = thread::spawn(move || {
        let mut bytes = Vec::new();
        output.take(MAX_WORKER_IPC_BYTES).read_to_end(&mut bytes).map(|_| bytes)
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
        return refusal(503, "request_deadline_exceeded", "the isolated request worker was cancelled at its deadline");
    }
    if !status.is_some_and(|status| status.success())
        || u64::try_from(bytes.len()).map_or(true, |length| length >= MAX_WORKER_IPC_BYTES)
    {
        return refusal(503, "worker_unavailable", "the bounded request worker refused completion");
    }
    serde_json::from_slice(&bytes)
        .unwrap_or_else(|_| refusal(503, "worker_unavailable", "the request worker returned an invalid response"))
}

fn serve(config: &Config) -> Result<(), String> {
    reclaim_worker_cgroups()?;
    Registrar::open(config, now())?;
    let service = Arc::new(Service {
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
            Ok(mut stream) => {
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
                    let _connection = ConnectionGuard;
                    let watchdog_socket = match stream.try_clone() {
                        Ok(socket) => socket,
                        Err(_) => return,
                    };
                    let (completed, completion) = mpsc::channel();
                    let watchdog_wait = deadline.saturating_duration_since(Instant::now());
                    thread::spawn(move || {
                        if completion.recv_timeout(watchdog_wait).is_err() {
                            let _ = watchdog_socket.shutdown(std::net::Shutdown::Both);
                        }
                    });
                    let _watchdog = WatchdogCompletion(completed);
                    let connection = match ServerConnection::new(tls) {
                        Ok(connection) => connection,
                        Err(_) => return,
                    };
                    let mut stream = DeadlineStream {
                        inner: StreamOwned::new(connection, stream),
                        deadline,
                    };
                    let response = parse_request(&mut stream).map_or_else(
                        |_| refusal(400, "invalid_request", "request could not be parsed"),
                        |request| {
                            let header = request.headers.get("authorization").map(String::as_str);
                            let authenticated = if request.path == "/healthz" {
                                true
                            } else if request.path == "/__registry/sources" {
                                service.publication_authority.verifies(header)
                            } else {
                                service.request_authority.verifies(header)
                            };
                            if !authenticated {
                                return refusal(401, "authentication_required", "a valid registry authority is required");
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
                                        return refusal(503, "registry_unavailable", "registry state lock is unavailable");
                                    }
                                    Err(TryLockError::WouldBlock) => {
                                        if Instant::now() >= deadline {
                                            return refusal(503, "request_deadline_exceeded", "the registry request deadline expired in the bounded queue");
                                        }
                                        thread::sleep(Duration::from_millis(1));
                                    }
                                }
                            };
                            isolated_route(&request, deadline)
                        },
                    );
                    let response = if Instant::now() >= deadline {
                        refusal(503, "request_deadline_exceeded", "the registry request deadline expired")
                    } else {
                        response
                    };
                    let Some(remaining) = deadline.checked_duration_since(Instant::now()) else {
                        return;
                    };
                    if stream.inner.sock.set_write_timeout(Some(remaining)).is_err() {
                        return;
                    }
                    let _ = write_response(&mut stream, &response);
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
        let remaining = arguments.next().and_then(|value| value.parse().ok()).unwrap_or(0);
        if kill_process(rustix::process::getpid(), Signal::Stop).is_err() {
            std::process::exit(3);
        }
        if let Err(error) = request_worker(remaining) {
            eprintln!("layerx-program-registry worker: {error}");
            std::process::exit(3);
        }
        return;
    }
    if let Err(error) = config().and_then(|config| serve(&config)) {
        eprintln!("layerx-program-registry: {error}");
        std::process::exit(2);
    }
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

    #[test]
    fn deployment_contract_keeps_https_mtls_and_bearer_roles_aligned() {
        assert!(GATEWAY_DEPLOYMENT.contains(
            "https://layerx-program-registry.layerx-testnet.svc.cluster.local:9420"
        ));
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
            "LAYERX_REGISTRY_BUILDER_CGROUP_ROOT",
            "LAYERX_REGISTRY_BUILD_MEMORY_BYTES",
            "LAYERX_REGISTRY_BUILD_PROCESS_LIMIT",
        ] {
            assert!(REGISTRY_DEPLOYMENT.contains(required));
        }
        assert!(!REGISTRY_DEPLOYMENT.contains("httpGet: {path: /healthz"));
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
            "environment_digest(&root.join(\"environment\"), Some(deadline))",
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
            "Signal::Stop",
            "Signal::Cont",
            "cpu.stat",
            "io.stat",
            "io.max",
        ] {
            assert!(CGROUP_SUPERVISOR_SOURCE.contains(aggregate_boundary));
        }
        for quota_boundary in ["cgroup.subtree_control", "mkfs.ext4", "-N", "--autoclear", "mountpoint -q", "e2fsck -p"] {
            assert!(PROVISIONER_SOURCE.contains(quota_boundary));
        }
    }

    #[test]
    fn request_deadline_cancels_only_the_bounded_worker_and_preserves_listener_liveness() {
        for boundary in ["registrar_gate: Mutex<()>", "--stopped-request-worker", "worker_group.kill()", "cgroup.kill", "child.wait()"] {
            assert!(MAIN_SOURCE.contains(boundary));
        }
        assert!(!MAIN_SOURCE.contains(concat!("std::process::", "abort()")));
    }

    #[test]
    fn delegation_quota_and_open_inode_execution_fail_closed() {
        for boundary in ["cgroup.subtree_control", "mountpoint -q", "losetup -j", "e2fsck -p", "mkfs.ext4", "-N", "--autoclear", "stat -c %u:%g"] {
            assert!(PROVISIONER_SOURCE.contains(boundary));
        }
        for boundary in ["CapabilityBoundingSet=CAP_SYS_ADMIN CAP_CHOWN CAP_DAC_OVERRIDE", "ProtectSystem=strict", "ReadWritePaths=/sys/fs/cgroup /var/lib/layerx-program-registry-builds"] {
            assert!(NODE_UNIT.contains(boundary));
        }
        assert!(REGISTRY_DEPLOYMENT.contains("layerx.io/program-registry-boundary: \"v1\""));
        for boundary in ["NonBlockingLockExclusive", "sync_all", "metadata.dev() != root_device", "fcntl_setfd", "/proc/self/fd/"] {
            assert!(BUILDER_SOURCE.contains(boundary));
        }
    }
}
