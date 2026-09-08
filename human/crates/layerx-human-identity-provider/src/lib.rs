#![forbid(unsafe_code)]

mod provision;
mod state;
mod wire;

pub use state::{Policy, State};

use std::fs;
use std::io;
use std::os::unix::fs::{FileTypeExt, MetadataExt, PermissionsExt};
use std::os::unix::net::UnixListener;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

/// One bounded, sequential Unix listener. Queued peers do not allocate workers.
pub struct Server {
    listener: UnixListener,
    socket: PathBuf,
    socket_identity: (u64, u64),
    state: State,
    allowed_uid: u32,
    deadline: Duration,
}

impl Server {
    /// Opens consistent protected state before publishing a socket.
    ///
    /// # Errors
    /// Rejects unprotected paths, live sockets, invalid policies and corrupt state.
    pub fn bind(
        socket: &Path,
        state: State,
        allowed_uid: u32,
        deadline: Duration,
    ) -> io::Result<Self> {
        if !socket.is_absolute() || deadline.is_zero() || deadline > Duration::from_secs(60) {
            return Err(invalid("invalid socket path or deadline"));
        }
        state.ready()?;
        let parent = socket
            .parent()
            .ok_or_else(|| invalid("socket parent missing"))?;
        state::check_directory(parent, false)?;
        remove_stale_socket(socket)?;
        let listener = UnixListener::bind(socket)?;
        fs::set_permissions(socket, fs::Permissions::from_mode(0o660))?;
        let metadata = fs::symlink_metadata(socket)?;
        rustix::net::listen(&listener, 16)?;
        listener.set_nonblocking(true)?;
        Ok(Self {
            listener,
            socket: socket.to_owned(),
            socket_identity: (metadata.dev(), metadata.ino()),
            state,
            allowed_uid,
            deadline,
        })
    }

    /// Serves one frame per authenticated connection until shutdown is requested.
    ///
    /// # Errors
    /// Returns listener or durable-state errors; malformed peers are isolated.
    pub fn run(mut self, shutdown: &AtomicBool) -> io::Result<()> {
        while !shutdown.load(Ordering::Acquire) {
            match self.listener.accept() {
                Ok((mut peer, _)) => {
                    let credentials = rustix::net::sockopt::socket_peercred(&peer)?;
                    if credentials.uid.as_raw() == self.allowed_uid {
                        wire::serve(&mut peer, &mut self.state, self.deadline)?;
                    }
                }
                Err(error) if error.kind() == io::ErrorKind::WouldBlock => {
                    std::thread::sleep(Duration::from_millis(10));
                }
                Err(error) if error.kind() == io::ErrorKind::Interrupted => {}
                Err(error) => return Err(error),
            }
        }
        Ok(())
    }
}

impl Drop for Server {
    fn drop(&mut self) {
        if let Ok(metadata) = fs::symlink_metadata(&self.socket) {
            if (metadata.dev(), metadata.ino()) == self.socket_identity {
                let _ = fs::remove_file(&self.socket);
            }
        }
    }
}

fn remove_stale_socket(socket: &Path) -> io::Result<()> {
    let metadata = match fs::symlink_metadata(socket) {
        Ok(value) => value,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(()),
        Err(error) => return Err(error),
    };
    if !metadata.file_type().is_socket()
        || metadata.uid() != rustix::process::geteuid().as_raw()
        || metadata.mode() & 0o007 != 0
    {
        return Err(invalid("untrusted existing socket"));
    }
    let probe = rustix::net::socket_with(
        rustix::net::AddressFamily::UNIX,
        rustix::net::SocketType::STREAM,
        rustix::net::SocketFlags::NONBLOCK | rustix::net::SocketFlags::CLOEXEC,
        None,
    )?;
    let address = rustix::net::SocketAddrUnix::new(socket)?;
    match rustix::net::connect(&probe, &address) {
        Err(rustix::io::Errno::CONNREFUSED) => {
            let current = fs::symlink_metadata(socket)?;
            if (current.dev(), current.ino()) != (metadata.dev(), metadata.ino()) {
                return Err(invalid("socket changed"));
            }
            fs::remove_file(socket)
        }
        _ => Err(io::Error::new(io::ErrorKind::AddrInUse, "socket is in use")),
    }
}

fn invalid(message: &'static str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, message)
}
