use std::fs::{self, File};
use std::net::Shutdown;
use std::os::unix::fs::{FileTypeExt, MetadataExt, PermissionsExt};
use std::os::unix::net::UnixListener;
use std::path::{Path, PathBuf};
use std::sync::mpsc;
use std::thread;
use std::time::Duration;

use layerx_human_service::server::movement_provider::{
    serve_connection, MovementProviderService, NativeMovementCodec,
};

use crate::config::MAX_FRAME;
use crate::Error;

pub(crate) struct ListenerConfig {
    pub socket: PathBuf,
    pub allowed_uid: u32,
    pub allowed_gid: u32,
    pub maximum_frame_bytes: usize,
    pub deadline: Duration,
    pub protocol: u16,
}

pub(crate) struct Listener {
    socket: UnixListener,
    config: ListenerConfig,
    codec: NativeMovementCodec,
    identity: (u64, u64),
    _lock: File,
}

impl Listener {
    pub fn bind(config: ListenerConfig) -> Result<Self, Error> {
        if !config.socket.is_absolute()
            || !(2..=MAX_FRAME).contains(&config.maximum_frame_bytes)
            || config.deadline.is_zero()
            || config.deadline > Duration::from_secs(60)
        {
            return Err(Error::Configuration);
        }
        let codec =
            NativeMovementCodec::for_protocol(config.protocol).map_err(|_| Error::Configuration)?;
        let parent = config.socket.parent().ok_or(Error::Configuration)?;
        let meta = fs::symlink_metadata(parent)?;
        if !meta.is_dir()
            || fs::canonicalize(parent)? != parent
            || meta.uid() != rustix::process::geteuid().as_raw()
            || meta.gid() != rustix::process::getegid().as_raw()
            || meta.mode() & 0o027 != 0
        {
            return Err(Error::Integrity);
        }
        let lock_path = config.socket.with_extension("sock.lock");
        let lock = crate::journal::socket_lock(&lock_path)?;
        match fs::symlink_metadata(&config.socket) {
            Ok(meta) => {
                if !meta.file_type().is_socket()
                    || meta.uid() != rustix::process::geteuid().as_raw()
                {
                    return Err(Error::Integrity);
                }
                match probe_socket(&config.socket) {
                    Ok(()) => return Err(Error::Conflict),
                    Err(error) if error.kind() == std::io::ErrorKind::ConnectionRefused => {
                        fs::remove_file(&config.socket)?;
                    }
                    Err(error) => return Err(error.into()),
                }
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => (),
            Err(error) => return Err(error.into()),
        }
        let socket = UnixListener::bind(&config.socket)?;
        rustix::net::listen(&socket, 8).map_err(std::io::Error::from)?;
        fs::set_permissions(&config.socket, fs::Permissions::from_mode(0o660))?;
        let meta = fs::symlink_metadata(&config.socket)?;
        File::open(parent)?.sync_all()?;
        Ok(Self {
            socket,
            config,
            codec,
            identity: (meta.dev(), meta.ino()),
            _lock: lock,
        })
    }

    pub fn serve_next(&self, service: &mut dyn MovementProviderService) -> Result<(), Error> {
        let (stream, _) = self.socket.accept()?;
        let deadline_stream = stream.try_clone()?;
        let (finished, pending) = mpsc::sync_channel(1);
        let deadline = self.config.deadline;
        let watchdog = thread::Builder::new().spawn(move || {
            if matches!(
                pending.recv_timeout(deadline),
                Err(mpsc::RecvTimeoutError::Timeout)
            ) {
                let _ = deadline_stream.shutdown(Shutdown::Both);
            }
        })?;
        let _ = serve_connection(
            stream,
            self.config.allowed_uid,
            self.config.allowed_gid,
            self.config.maximum_frame_bytes,
            self.config.deadline,
            &self.codec,
            service,
        );
        let _ = finished.send(());
        watchdog.join().map_err(|_| Error::Integrity)?;
        Ok(())
    }
}

impl Drop for Listener {
    fn drop(&mut self) {
        if let Ok(meta) = fs::symlink_metadata(&self.config.socket) {
            if meta.file_type().is_socket() && (meta.dev(), meta.ino()) == self.identity {
                let _ = fs::remove_file(&self.config.socket);
            }
        }
    }
}

fn probe_socket(path: &Path) -> std::io::Result<()> {
    use rustix::net::{socket_with, AddressFamily, SocketAddrUnix, SocketFlags, SocketType};
    let probe = socket_with(
        AddressFamily::UNIX,
        SocketType::STREAM,
        SocketFlags::CLOEXEC | SocketFlags::NONBLOCK,
        None,
    )?;
    let address = SocketAddrUnix::new(path)?;
    rustix::net::connect(&probe, &address).map_err(std::io::Error::from)
}
