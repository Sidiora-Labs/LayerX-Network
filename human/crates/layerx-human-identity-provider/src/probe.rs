use std::io::{self, Read, Write};
use std::os::unix::fs::{FileTypeExt, MetadataExt};
use std::os::unix::net::UnixStream;
use std::path::{Path, PathBuf};
use std::sync::mpsc;
use std::thread;
use std::time::Duration;

const MAGIC: &[u8; 5] = b"LXIP\x01";
const READINESS: u8 = 0;
const FRAME: usize = 10;

pub(crate) struct Settings {
    pub socket: PathBuf,
    pub deadline: Duration,
}

impl Settings {
    pub fn from_environment() -> io::Result<Self> {
        let socket = PathBuf::from(
            std::env::var_os("LAYERX_HUMAN_IDENTITY_PROVIDER_SOCKET")
                .ok_or_else(|| refused("LAYERX_HUMAN_IDENTITY_PROVIDER_SOCKET is required"))?,
        );
        if !socket.is_absolute() {
            return Err(refused("LAYERX_HUMAN_IDENTITY_PROVIDER_SOCKET is required"));
        }
        let seconds = match std::env::var("LAYERX_HUMAN_IDENTITY_PROVIDER_DEADLINE_SECONDS") {
            Ok(value) => value
                .parse::<u64>()
                .map_err(|_| refused("invalid LAYERX_HUMAN_IDENTITY_PROVIDER_DEADLINE_SECONDS"))?,
            Err(std::env::VarError::NotPresent) => 5,
            Err(error) => return Err(io::Error::new(io::ErrorKind::InvalidInput, error)),
        };
        if !(1..=60).contains(&seconds) {
            return Err(refused(
                "invalid LAYERX_HUMAN_IDENTITY_PROVIDER_DEADLINE_SECONDS",
            ));
        }
        Ok(Self {
            socket,
            deadline: Duration::from_secs(seconds),
        })
    }
}

pub(crate) fn run() -> io::Result<()> {
    probe(&Settings::from_environment()?)
}

/// Answers the readiness of the live provider within `settings.deadline`.
pub(crate) fn probe(settings: &Settings) -> io::Result<()> {
    let socket = settings.socket.clone();
    let deadline = settings.deadline;
    let (finished, answer) = mpsc::sync_channel(1);
    thread::Builder::new().spawn(move || {
        let _ = finished.send(exchange(&socket, deadline));
    })?;
    match answer.recv_timeout(deadline) {
        Ok(result) => result,
        Err(_) => Err(io::Error::new(
            io::ErrorKind::TimedOut,
            "identity provider did not answer readiness",
        )),
    }
}

fn exchange(socket: &Path, deadline: Duration) -> io::Result<()> {
    let metadata = std::fs::symlink_metadata(socket)?;
    if !metadata.file_type().is_socket()
        || metadata.uid() != rustix::process::geteuid().as_raw()
        || metadata.mode() & 0o007 != 0
    {
        return Err(refused("untrusted identity provider socket"));
    }
    let mut stream = UnixStream::connect(socket)?;
    stream.set_read_timeout(Some(deadline))?;
    stream.set_write_timeout(Some(deadline))?;
    let mut request = Vec::with_capacity(4 + FRAME);
    request.extend_from_slice(
        &u32::try_from(FRAME)
            .map_err(|_| refused("readiness frame length"))?
            .to_be_bytes(),
    );
    request.extend_from_slice(MAGIC);
    request.push(READINESS);
    request.extend_from_slice(&0_u32.to_be_bytes());
    stream.write_all(&request)?;
    stream.flush()?;
    let mut length = [0; 4];
    stream.read_exact(&mut length)?;
    if u32::from_be_bytes(length) as usize != FRAME {
        return Err(refused("identity provider is not ready"));
    }
    let mut response = [0; FRAME];
    stream.read_exact(&mut response)?;
    if &response[..5] != MAGIC || response[5] != 0 || response[6..] != [0; 4] {
        return Err(refused("identity provider is not ready"));
    }
    Ok(())
}

fn refused(message: &'static str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidInput, message)
}
