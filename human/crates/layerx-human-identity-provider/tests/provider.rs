use std::error::Error;
use std::fs::{self, OpenOptions};
use std::io::{Read, Write};
use std::os::unix::fs::{MetadataExt, OpenOptionsExt, PermissionsExt};
use std::os::unix::net::UnixStream;
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

use layerx_human_identity_provider::{Policy, Server, State};
use layerx_human_service::auth::Device;
use layerx_human_service::onboarding::RecoveryPolicy;
use layerx_human_service::store::PrincipalId;
use layerx_types::ids::Did;
use layerx_types::intent::{ApprovalThreshold, RecoveryRoot};

type Result<T = ()> = std::result::Result<T, Box<dyn Error>>;

fn policy() -> Policy {
    Policy {
        root: [0x43; 32],
        threshold: 1,
        delay_seconds: 86_400,
    }
}

struct Running {
    shutdown: Arc<AtomicBool>,
    worker: Option<JoinHandle<std::io::Result<()>>>,
}

impl Running {
    fn start(socket: &Path, root: &Path, uid: u32) -> Result<Self> {
        let server = Server::bind(
            socket,
            State::open(root, policy())?,
            uid,
            Duration::from_millis(200),
        )?;
        let shutdown = Arc::new(AtomicBool::new(false));
        let flag = Arc::clone(&shutdown);
        Ok(Self {
            shutdown,
            worker: Some(thread::spawn(move || server.run(&flag))),
        })
    }

    fn stop(mut self) -> Result {
        self.shutdown.store(true, Ordering::Release);
        self.worker
            .take()
            .ok_or("worker missing")?
            .join()
            .map_err(|_| "worker panicked")??;
        Ok(())
    }
}

impl Drop for Running {
    fn drop(&mut self) {
        self.shutdown.store(true, Ordering::Release);
        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        }
    }
}

fn request(operation: u8, fields: &[&[u8]]) -> Result<Vec<u8>> {
    let mut bytes = b"LXIP\x01".to_vec();
    bytes.push(operation);
    bytes.extend_from_slice(&u32::try_from(fields.len())?.to_be_bytes());
    for field in fields {
        bytes.extend_from_slice(&u32::try_from(field.len())?.to_be_bytes());
        bytes.extend_from_slice(field);
    }
    Ok(bytes)
}

fn exchange(socket: &Path, bytes: &[u8]) -> Result<Vec<u8>> {
    let mut stream = UnixStream::connect(socket)?;
    stream.set_read_timeout(Some(Duration::from_secs(2)))?;
    stream.write_all(&u32::try_from(bytes.len())?.to_be_bytes())?;
    stream.write_all(bytes)?;
    let mut length = [0; 4];
    stream.read_exact(&mut length)?;
    let length = u32::from_be_bytes(length) as usize;
    assert!((10..=1_048_576).contains(&length));
    let mut response = vec![0; length];
    stream.read_exact(&mut response)?;
    Ok(response)
}

fn call(socket: &Path, operation: u8, fields: &[&[u8]]) -> Result<Vec<Vec<u8>>> {
    let response = exchange(socket, &request(operation, fields)?)?;
    assert_eq!(&response[..6], b"LXIP\x01\x00");
    let count = u32::from_be_bytes(response[6..10].try_into()?);
    let mut remaining = &response[10..];
    let mut decoded = Vec::new();
    for _ in 0..count {
        let length = u32::from_be_bytes(remaining[..4].try_into()?) as usize;
        decoded.push(remaining[4..4 + length].to_vec());
        remaining = &remaining[4 + length..];
    }
    assert!(remaining.is_empty());
    Ok(decoded)
}

#[test]
fn provision_resolve_device_and_restart_replay() -> Result {
    let directory = tempfile::Builder::new()
        .permissions(fs::Permissions::from_mode(0o700))
        .tempdir()?;
    let root = directory.path().join("state");
    let socket = directory.path().join("identity.sock");
    let uid = rustix::process::geteuid().as_raw();
    let running = Running::start(&socket, &root, uid)?;
    let credentials = rustix::net::sockopt::socket_peercred(&UnixStream::connect(&socket)?)?;
    assert_eq!(credentials.uid.as_raw(), uid);
    assert_eq!(fs::metadata(&socket)?.mode() & 0o777, 0o660);
    assert!(call(&socket, 0, &[])?.is_empty());
    let fields = [
        b"person@example.com".as_slice(),
        b"Person",
        b"request-1",
        &123_u64.to_be_bytes(),
    ];
    let account = call(&socket, 1, &fields)?;
    assert_eq!(account.len(), 5);
    let principal = PrincipalId::new(std::str::from_utf8(&account[0])?)?;
    Did::new(&account[1]).map_err(|error| format!("{error:?}"))?;
    RecoveryPolicy::new(
        RecoveryRoot::new(account[2].as_slice().try_into()?),
        ApprovalThreshold::new(u16::from_be_bytes(account[3].as_slice().try_into()?))
            .map_err(|error| format!("{error:?}"))?,
        u64::from_be_bytes(account[4].as_slice().try_into()?),
    )?;
    assert_eq!(call(&socket, 1, &fields)?, account);
    assert_eq!(call(&socket, 2, &[fields[0]])?, vec![account[0].clone()]);
    assert_eq!(
        exchange(
            &socket,
            &request(3, &[principal.as_str().as_bytes(), b"assertion-1"])?
        )?[5],
        1
    );
    assert!(State::open(&root, policy()).is_err());
    running.stop()?;
    assert!(!socket.exists());
    let device = Device::mint("Personal laptop", "linux")?;
    let mut state = State::open(&root, policy())?;
    state.bind_device(&principal, "assertion-1", device.clone())?;
    state.bind_device(&principal, "assertion-1", device.clone())?;
    assert!(state
        .bind_device(&principal, "assertion-1", Device::mint("Other", "linux")?)
        .is_err());
    drop(state);
    OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(root.join("state.pending"))?
        .write_all(b"torn uncommitted write")?;
    let running = Running::start(&socket, &root, uid)?;
    assert!(!root.join("state.pending").exists());
    assert_eq!(call(&socket, 1, &fields)?, account);
    let encoded_device = call(&socket, 3, &[principal.as_str().as_bytes(), b"assertion-1"])?;
    assert_eq!(
        encoded_device,
        vec![
            device.device_id().as_bytes(),
            device.label().as_bytes(),
            device.platform().as_bytes()
        ]
    );
    let other = call(
        &socket,
        1,
        &[
            b"other@example.com",
            b"Other",
            b"request-2",
            &124_u64.to_be_bytes(),
        ],
    )?;
    assert_ne!(other[0], account[0]);
    assert_eq!(
        exchange(&socket, &request(3, &[&other[0], b"assertion-1"])?)?[5],
        1
    );
    running.stop()?;
    for name in ["state.json", "writer.lock"] {
        let metadata = fs::metadata(root.join(name))?;
        assert_eq!(metadata.mode() & 0o777, 0o600);
        assert_eq!(metadata.uid(), uid);
    }
    Ok(())
}

#[test]
fn refuses_conflicts_unknowns_and_malformed_frames() -> Result {
    let directory = tempfile::Builder::new()
        .permissions(fs::Permissions::from_mode(0o700))
        .tempdir()?;
    let socket = directory.path().join("identity.sock");
    let running = Running::start(
        &socket,
        &directory.path().join("state"),
        rustix::process::geteuid().as_raw(),
    )?;
    call(
        &socket,
        1,
        &[b"one@example.com", b"One", b"key", &1_u64.to_be_bytes()],
    )?;
    let invalid = vec![
        request(
            1,
            &[b"two@example.com", b"One", b"key", &1_u64.to_be_bytes()],
        )?,
        request(
            1,
            &[
                b"one@example.com",
                b"One",
                b"other-key",
                &1_u64.to_be_bytes(),
            ],
        )?,
        request(
            1,
            &[b"UPPER@example.com", b"One", b"new", &1_u64.to_be_bytes()],
        )?,
        request(1, &[b"bad", b"One", b"new", &1_u64.to_be_bytes()])?,
        request(1, &[b"ok@example.com", b"One", b"new", b"1"])?,
        request(2, &[b"unknown@example.com"])?,
        request(9, &[])?,
        request(0, &[b"extra"])?,
        b"LXIP\x02\0\0\0\0\0".to_vec(),
        b"NOPE\x01\0\0\0\0\0".to_vec(),
        b"LXIP\x01\x02\xff\xff\xff\xff".to_vec(),
        b"LXIP\x01\x02\0\0\0\x01\xff\xff\xff\xff".to_vec(),
        b"LXIP\x01\0\0\0\0\0trailing".to_vec(),
    ];
    for bytes in invalid {
        assert_eq!(exchange(&socket, &bytes)?, b"LXIP\x01\x01\0\0\0\0");
        assert!(call(&socket, 0, &[])?.is_empty());
    }
    for length in [0_u32, 9, 1_048_577, u32::MAX] {
        let mut stream = UnixStream::connect(&socket)?;
        stream.set_read_timeout(Some(Duration::from_secs(2)))?;
        stream.write_all(&length.to_be_bytes())?;
        let mut response = [0; 14];
        stream.read_exact(&mut response)?;
        assert_eq!(&response[4..], b"LXIP\x01\x01\0\0\0\0");
    }
    let mut slow = UnixStream::connect(&socket)?;
    slow.write_all(&100_u32.to_be_bytes())?;
    slow.write_all(b"L")?;
    let start = Instant::now();
    assert!(call(&socket, 0, &[])?.is_empty());
    assert!(start.elapsed() < Duration::from_secs(2));
    running.stop()
}

#[test]
fn rejects_wrong_uid_and_unsafe_or_corrupt_state() -> Result {
    let directory = tempfile::Builder::new()
        .permissions(fs::Permissions::from_mode(0o700))
        .tempdir()?;
    let socket = directory.path().join("identity.sock");
    let root = directory.path().join("state");
    let uid = rustix::process::geteuid().as_raw();
    let running = Running::start(&socket, &root, uid ^ 1)?;
    assert!(exchange(&socket, &request(0, &[])?).is_err());
    running.stop()?;
    fs::set_permissions(root.join("state.json"), fs::Permissions::from_mode(0o644))?;
    assert!(State::open(&root, policy()).is_err());
    fs::set_permissions(root.join("state.json"), fs::Permissions::from_mode(0o600))?;
    let bytes = fs::read(root.join("state.json"))?;
    fs::write(root.join("state.json"), b"torn committed write")?;
    assert!(State::open(&root, policy()).is_err());
    fs::write(root.join("state.json"), &bytes)?;
    let saved = root.join("saved.json");
    fs::rename(root.join("state.json"), &saved)?;
    std::os::unix::fs::symlink(&saved, root.join("state.json"))?;
    assert!(State::open(&root, policy()).is_err());
    fs::remove_file(root.join("state.json"))?;
    fs::hard_link(&saved, root.join("state.json"))?;
    assert!(State::open(&root, policy()).is_err());
    Ok(())
}

#[test]
fn binary_sigterm_and_crash_restart() -> Result {
    let directory = tempfile::Builder::new()
        .permissions(fs::Permissions::from_mode(0o700))
        .tempdir()?;
    let socket = directory.path().join("identity.sock");
    let root = directory.path().join("state");
    let policy_file = directory.path().join("policy.json");
    OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(&policy_file)?
        .write_all(&serde_json::to_vec(&policy())?)?;
    let command = || {
        let mut command =
            std::process::Command::new(env!("CARGO_BIN_EXE_layerx-human-identity-provider"));
        command
            .env("LAYERX_HUMAN_IDENTITY_PROVIDER_STATE_ROOT", &root)
            .env("LAYERX_HUMAN_IDENTITY_PROVIDER_SOCKET", &socket)
            .env(
                "LAYERX_HUMAN_IDENTITY_PROVIDER_RECOVERY_POLICY_FILE",
                &policy_file,
            )
            .env(
                "LAYERX_HUMAN_IDENTITY_PROVIDER_ALLOWED_UID",
                rustix::process::geteuid().as_raw().to_string(),
            );
        command
    };
    let mut child = OwnedChild(command().spawn()?);
    wait_ready(&socket, &mut child.0)?;
    let fields = [
        b"restart@example.com".as_slice(),
        b"Restart",
        b"restart-key",
        &1_u64.to_be_bytes(),
    ];
    let account = call(&socket, 1, &fields)?;
    child.0.kill()?;
    child.0.wait()?;
    assert!(socket.exists());
    let device = Device::mint("Restart laptop", "linux")?;
    let mut enrollment = OwnedChild(
        command()
            .arg("bind-device")
            .stdin(std::process::Stdio::piped())
            .spawn()?,
    );
    let body = serde_json::json!({"principal": std::str::from_utf8(&account[0])?,
        "assertion_id": "restart-assertion", "device": device});
    enrollment
        .0
        .stdin
        .take()
        .ok_or("stdin missing")?
        .write_all(&serde_json::to_vec(&body)?)?;
    assert!(enrollment.0.wait()?.success());
    let mut child = OwnedChild(command().spawn()?);
    wait_ready(&socket, &mut child.0)?;
    assert_eq!(call(&socket, 1, &fields)?, account);
    assert_eq!(
        call(&socket, 3, &[&account[0], b"restart-assertion"])?[0],
        device.device_id().as_bytes()
    );
    let pid = rustix::process::Pid::from_raw(i32::try_from(child.0.id())?).ok_or("invalid pid")?;
    rustix::process::kill_process(pid, rustix::process::Signal::TERM)?;
    let expires = Instant::now() + Duration::from_secs(3);
    loop {
        if let Some(status) = child.0.try_wait()? {
            assert!(status.success());
            break;
        }
        assert!(Instant::now() < expires, "SIGTERM shutdown deadline");
        thread::sleep(Duration::from_millis(10));
    }
    assert!(!socket.exists());
    Ok(())
}

struct OwnedChild(std::process::Child);
impl Drop for OwnedChild {
    fn drop(&mut self) {
        if matches!(self.0.try_wait(), Ok(None)) {
            let _ = self.0.kill();
            let _ = self.0.wait();
        }
    }
}

fn wait_ready(socket: &Path, child: &mut std::process::Child) -> Result {
    let expires = Instant::now() + Duration::from_secs(5);
    while Instant::now() < expires {
        if exchange(socket, &request(0, &[])?).is_ok() {
            return Ok(());
        }
        assert!(
            child.try_wait()?.is_none(),
            "provider exited during startup"
        );
        thread::sleep(Duration::from_millis(10));
    }
    Err("provider did not become ready".into())
}

#[test]
fn loses_readiness_when_committed_state_is_changed() -> Result {
    let directory = tempfile::Builder::new()
        .permissions(fs::Permissions::from_mode(0o700))
        .tempdir()?;
    let root = directory.path().join("state");
    let socket = directory.path().join("identity.sock");
    let mut running = Running::start(&socket, &root, rustix::process::geteuid().as_raw())?;
    assert!(call(&socket, 0, &[])?.is_empty());
    fs::write(root.join("state.json"), b"corrupt")?;
    assert!(exchange(&socket, &request(0, &[])?).is_err());
    let result = running
        .worker
        .take()
        .ok_or("worker missing")?
        .join()
        .map_err(|_| "worker panic")?;
    assert!(result.is_err());
    assert!(!socket.exists());
    Ok(())
}

#[test]
fn refuses_live_sockets_regular_files_and_socket_symlinks() -> Result {
    let directory = tempfile::Builder::new()
        .permissions(fs::Permissions::from_mode(0o700))
        .tempdir()?;
    let socket = directory.path().join("identity.sock");
    let uid = rustix::process::geteuid().as_raw();
    let running = Running::start(&socket, &directory.path().join("first"), uid)?;
    let state = State::open(&directory.path().join("second"), policy())?;
    assert!(Server::bind(&socket, state, uid, Duration::from_secs(1)).is_err());
    running.stop()?;
    fs::write(&socket, b"keep this file")?;
    let state = State::open(&directory.path().join("second"), policy())?;
    assert!(Server::bind(&socket, state, uid, Duration::from_secs(1)).is_err());
    assert_eq!(fs::read(&socket)?, b"keep this file");
    fs::remove_file(&socket)?;
    std::os::unix::fs::symlink(directory.path().join("missing"), &socket)?;
    let state = State::open(&directory.path().join("second"), policy())?;
    assert!(Server::bind(&socket, state, uid, Duration::from_secs(1)).is_err());
    assert!(fs::symlink_metadata(&socket)?.file_type().is_symlink());
    Ok(())
}

#[test]
fn protects_policy_and_state_directory() -> Result {
    let directory = tempfile::Builder::new()
        .permissions(fs::Permissions::from_mode(0o700))
        .tempdir()?;
    let policy_file = directory.path().join("policy.json");
    OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(&policy_file)?
        .write_all(&serde_json::to_vec(&policy())?)?;
    assert_eq!(Policy::read(&policy_file)?, policy());
    fs::set_permissions(&policy_file, fs::Permissions::from_mode(0o640))?;
    assert!(Policy::read(&policy_file).is_err());
    fs::set_permissions(&policy_file, fs::Permissions::from_mode(0o600))?;
    let link = directory.path().join("policy-link");
    std::os::unix::fs::symlink(&policy_file, &link)?;
    assert!(Policy::read(&link).is_err());
    let root = directory.path().join("state");
    drop(State::open(&root, policy())?);
    fs::set_permissions(&root, fs::Permissions::from_mode(0o750))?;
    assert!(State::open(&root, policy()).is_err());
    fs::set_permissions(&root, fs::Permissions::from_mode(0o700))?;
    let root_link = directory.path().join("state-link");
    std::os::unix::fs::symlink(&root, &root_link)?;
    assert!(State::open(&root_link, policy()).is_err());
    let mut invalid_policy = policy();
    invalid_policy.root = [0; 32];
    assert!(State::open(&root, invalid_policy).is_err());
    Ok(())
}

#[test]
fn missing_committed_snapshot_does_not_reinitialize_an_established_directory() -> Result {
    let directory = tempfile::Builder::new()
        .permissions(fs::Permissions::from_mode(0o700))
        .tempdir()?;
    let root = directory.path().join("state");
    drop(State::open(&root, policy())?);
    assert_eq!(fs::read(root.join("initialized"))?, b"LXIP-state-v1");
    assert_eq!(
        fs::metadata(root.join("initialized"))?.mode() & 0o777,
        0o600
    );
    fs::remove_file(root.join("state.json"))?;
    assert!(State::open(&root, policy()).is_err());
    assert!(!root.join("state.json").exists());
    Ok(())
}
