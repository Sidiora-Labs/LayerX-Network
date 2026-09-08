use crate::{Error, Result, Store};
use std::fs;
use std::io::{Read, Write};
use std::os::unix::fs::{MetadataExt as _, PermissionsExt as _};
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};
use zeroize::Zeroizing;

const MAX_FRAME: usize = 1_048_576;

pub struct Config {
    pub state_root: PathBuf,
    pub trust_history: PathBuf,
    pub socket: PathBuf,
    pub allowed_uid: u32,
    pub deadline: Duration,
}
impl Config {
    pub fn from_env() -> Result<Self> {
        let path = |name| {
            std::env::var_os(name)
                .map(PathBuf::from)
                .ok_or(Error::Configuration)
        };
        let seconds = match std::env::var("LAYERX_HUMAN_SECURITY_PROVIDER_DEADLINE_SECONDS") {
            Ok(value) => value.parse::<u64>().map_err(|_| Error::Configuration)?,
            Err(std::env::VarError::NotPresent) => 5,
            Err(_) => return Err(Error::Configuration),
        };
        let config = Self {
            state_root: path("LAYERX_HUMAN_SECURITY_PROVIDER_STATE_ROOT")?,
            trust_history: path("LAYERX_HUMAN_SECURITY_PROVIDER_TRUST_HISTORY")?,
            socket: path("LAYERX_HUMAN_SECURITY_PROVIDER_SOCKET")?,
            allowed_uid: std::env::var("LAYERX_HUMAN_SECURITY_PROVIDER_ALLOWED_UID")
                .map_err(|_| Error::Configuration)?
                .parse()
                .map_err(|_| Error::Configuration)?,
            deadline: Duration::from_secs(seconds),
        };
        config.validate()?;
        Ok(config)
    }
    fn validate(&self) -> Result<()> {
        if !self.socket.is_absolute()
            || !self.state_root.is_absolute()
            || !self.trust_history.is_absolute()
            || self.deadline < Duration::from_secs(1)
            || self.deadline > Duration::from_secs(60)
        {
            return Err(Error::Configuration);
        }
        Ok(())
    }
}
pub fn serve(config: Config, shutdown: Arc<AtomicBool>) -> Result<()> {
    config.validate()?;
    let mut store = Store::open(&config.state_root, &config.trust_history)?;
    let parent = config.socket.parent().ok_or(Error::Configuration)?;
    let metadata = fs::symlink_metadata(parent)?;
    if !metadata.is_dir()
        || metadata.file_type().is_symlink()
        || metadata.uid() != rustix::process::geteuid().as_raw()
        || metadata.mode() & 0o022 != 0
    {
        return Err(Error::Configuration);
    }
    if parent.canonicalize()? != parent {
        return Err(Error::Configuration);
    }
    let listener = UnixListener::bind(&config.socket)?;
    let socket_metadata = fs::symlink_metadata(&config.socket)?;
    let cleanup = SocketCleanup {
        path: config.socket.clone(),
        device: socket_metadata.dev(),
        inode: socket_metadata.ino(),
    };
    fs::set_permissions(&config.socket, fs::Permissions::from_mode(0o660))?;
    listener.set_nonblocking(true)?;
    while !shutdown.load(Ordering::Relaxed) {
        match listener.accept() {
            Ok((mut stream, _)) => {
                let deadline = Instant::now() + config.deadline;
                let peer =
                    rustix::net::sockopt::socket_peercred(&stream).map_err(|_| Error::Refused)?;
                if peer.uid.as_raw() != config.allowed_uid {
                    let _ = send(&mut stream, &frame(1, &[])?, deadline, &shutdown);
                    discard(&mut stream, deadline, &shutdown);
                    continue;
                }
                let response = receive(&mut stream, deadline, &shutdown)
                    .and_then(|(op, fields)| store.dispatch(op, &fields));
                let bytes = match response {
                    Ok(fields) => frame(0, &fields)?,
                    Err(_) => frame(1, &[])?,
                };
                let _ = send(&mut stream, &bytes, deadline, &shutdown);
            }
            Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                std::thread::sleep(Duration::from_millis(10))
            }
            Err(error) if error.kind() == std::io::ErrorKind::Interrupted => {}
            Err(error) => return Err(error.into()),
        }
    }
    drop(listener);
    drop(cleanup);
    Ok(())
}
struct SocketCleanup {
    path: PathBuf,
    device: u64,
    inode: u64,
}
impl Drop for SocketCleanup {
    fn drop(&mut self) {
        if fs::symlink_metadata(&self.path)
            .is_ok_and(|m| (m.dev(), m.ino()) == (self.device, self.inode))
        {
            let _ = fs::remove_file(&self.path);
        }
    }
}
fn timeout(deadline: Instant, shutdown: &AtomicBool) -> Result<Duration> {
    if shutdown.load(Ordering::Relaxed) {
        return Err(Error::Refused);
    }
    let left = deadline
        .checked_duration_since(Instant::now())
        .ok_or(Error::Refused)?;
    if left.is_zero() {
        return Err(Error::Refused);
    }
    Ok(left.min(Duration::from_millis(100)))
}
fn receive(
    stream: &mut UnixStream,
    deadline: Instant,
    shutdown: &AtomicBool,
) -> Result<(u8, Vec<Vec<u8>>)> {
    let mut prefix = [0; 4];
    read(stream, &mut prefix, deadline, shutdown)?;
    let length = u32::from_be_bytes(prefix) as usize;
    if !(10..=MAX_FRAME).contains(&length) {
        return Err(Error::Refused);
    }
    let mut payload = Zeroizing::new(vec![0; length]);
    read(stream, &mut payload, deadline, shutdown)?;
    if &payload[..5] != b"LXSP\x01" {
        return Err(Error::Refused);
    }
    let op = payload[5];
    let count = u32::from_be_bytes(payload[6..10].try_into().map_err(|_| Error::Refused)?) as usize;
    if count > 4 {
        return Err(Error::Refused);
    }
    let mut cursor = 10usize;
    let mut fields = Vec::new();
    for _ in 0..count {
        let end = cursor.checked_add(4).ok_or(Error::Refused)?;
        let size = u32::from_be_bytes(
            payload
                .get(cursor..end)
                .ok_or(Error::Refused)?
                .try_into()
                .map_err(|_| Error::Refused)?,
        ) as usize;
        cursor = end;
        let end = cursor.checked_add(size).ok_or(Error::Refused)?;
        if size > 4096 {
            return Err(Error::Refused);
        }
        fields.push(payload.get(cursor..end).ok_or(Error::Refused)?.to_vec());
        cursor = end;
    }
    if cursor != payload.len() {
        return Err(Error::Refused);
    }
    Ok((op, fields))
}
fn read(
    stream: &mut UnixStream,
    mut bytes: &mut [u8],
    deadline: Instant,
    shutdown: &AtomicBool,
) -> Result<()> {
    while !bytes.is_empty() {
        stream.set_read_timeout(Some(timeout(deadline, shutdown)?))?;
        match stream.read(bytes) {
            Ok(0) => return Err(Error::Refused),
            Ok(n) => bytes = &mut bytes[n..],
            Err(e)
                if matches!(
                    e.kind(),
                    std::io::ErrorKind::Interrupted
                        | std::io::ErrorKind::WouldBlock
                        | std::io::ErrorKind::TimedOut
                ) => {}
            Err(e) => return Err(e.into()),
        }
    }
    Ok(())
}
fn send(
    stream: &mut UnixStream,
    mut bytes: &[u8],
    deadline: Instant,
    shutdown: &AtomicBool,
) -> Result<()> {
    while !bytes.is_empty() {
        stream.set_write_timeout(Some(timeout(deadline, shutdown)?))?;
        match stream.write(bytes) {
            Ok(0) => return Err(Error::Refused),
            Ok(n) => bytes = &bytes[n..],
            Err(e)
                if matches!(
                    e.kind(),
                    std::io::ErrorKind::Interrupted
                        | std::io::ErrorKind::WouldBlock
                        | std::io::ErrorKind::TimedOut
                ) => {}
            Err(e) => return Err(e.into()),
        }
    }
    Ok(())
}
fn frame(code: u8, fields: &[Vec<u8>]) -> Result<Zeroizing<Vec<u8>>> {
    let mut bytes = Zeroizing::new(vec![0; 4]);
    bytes.extend_from_slice(b"LXSP\x01");
    bytes.push(code);
    bytes.extend_from_slice(&(fields.len() as u32).to_be_bytes());
    for field in fields {
        bytes.extend_from_slice(&(field.len() as u32).to_be_bytes());
        bytes.extend_from_slice(field);
    }
    let length = bytes.len() - 4;
    if length > MAX_FRAME {
        return Err(Error::Refused);
    }
    bytes[..4].copy_from_slice(&(length as u32).to_be_bytes());
    Ok(bytes)
}

fn discard(stream: &mut UnixStream, deadline: Instant, shutdown: &AtomicBool) {
    let mut buffer = [0; 1024];
    while let Ok(remaining) = timeout(deadline, shutdown) {
        if stream.set_read_timeout(Some(remaining)).is_err() {
            break;
        }
        match stream.read(&mut buffer) {
            Ok(0) => break,
            Ok(_) => {}
            Err(error)
                if matches!(
                    error.kind(),
                    std::io::ErrorKind::Interrupted
                        | std::io::ErrorKind::WouldBlock
                        | std::io::ErrorKind::TimedOut
                ) => {}
            Err(_) => break,
        }
    }
}
