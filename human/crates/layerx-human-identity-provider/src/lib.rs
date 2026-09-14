#![forbid(unsafe_code)]

mod binding;
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
    binding_reader: Option<binding::Reader>,
    clock: std::sync::Arc<dyn layerx_types::clock::Clock>,
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
        clock: std::sync::Arc<dyn layerx_types::clock::Clock>,
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
            binding_reader: None,
            clock,
        })
    }

    /// Adds a separately authenticated read-only principal binding endpoint.
    ///
    /// # Errors
    /// Refuses a changed durable tenant, unsafe socket or failed durable write.
    pub fn with_binding_reader(
        mut self,
        socket: &Path,
        tenant: &str,
        allowed_uids: &[u32],
    ) -> io::Result<Self> {
        if self.binding_reader.is_some() || socket == self.socket {
            return Err(invalid("binding reader already configured"));
        }
        let reader = binding::Reader::bind(socket, allowed_uids)?;
        self.state.bind_reader_tenant(tenant)?;
        self.binding_reader = Some(reader);
        Ok(self)
    }

    /// Serves one frame per authenticated connection until shutdown is requested.
    ///
    /// # Errors
    /// Returns listener or durable-state errors; malformed peers are isolated.
    pub fn run(mut self, shutdown: &AtomicBool) -> io::Result<()> {
        while !shutdown.load(Ordering::Acquire) {
            if let Some(reader) = &self.binding_reader {
                reader.accept(&self.state, self.deadline)?;
            }
            match self.listener.accept() {
                Ok((mut peer, _)) => {
                    let credentials = rustix::net::sockopt::socket_peercred(&peer)?;
                    if credentials.uid.as_raw() == self.allowed_uid {
                        wire::serve(
                            &mut peer,
                            &mut self.state,
                            self.deadline,
                            self.clock.as_ref(),
                        )?;
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
