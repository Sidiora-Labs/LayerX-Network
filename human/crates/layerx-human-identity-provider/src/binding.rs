use std::collections::BTreeSet;
use std::fs;
use std::io;
use std::os::unix::fs::{MetadataExt, PermissionsExt};
use std::os::unix::net::UnixListener;
use std::path::{Path, PathBuf};
use std::time::Duration;

use crate::{invalid, remove_stale_socket, state, wire, State};

pub(crate) struct Reader {
    listener: UnixListener,
    socket: PathBuf,
    identity: (u64, u64),
    allowed_uids: BTreeSet<u32>,
}

impl Reader {
    pub(crate) fn bind(socket: &Path, allowed_uids: &[u32]) -> io::Result<Self> {
        let readers: BTreeSet<_> = allowed_uids.iter().copied().collect();
        if !socket.is_absolute()
            || readers.is_empty()
            || readers.len() > 16
            || readers.len() != allowed_uids.len()
        {
            return Err(invalid("binding socket must be absolute"));
        }
        state::check_directory(
            socket.parent().ok_or_else(|| invalid("socket parent"))?,
            false,
        )?;
        remove_stale_socket(socket)?;
        let listener = UnixListener::bind(socket)?;
        fs::set_permissions(socket, fs::Permissions::from_mode(0o660))?;
        let metadata = fs::symlink_metadata(socket)?;
        rustix::net::listen(&listener, 16)?;
        listener.set_nonblocking(true)?;
        Ok(Self {
            listener,
            socket: socket.to_owned(),
            identity: (metadata.dev(), metadata.ino()),
            allowed_uids: readers,
        })
    }

    pub(crate) fn accept(
        &self,
        state: &State,
        deadline: Duration,
        clock: &dyn layerx_types::clock::Clock,
    ) -> io::Result<bool> {
        match self.listener.accept() {
            Ok((mut peer, _)) => {
                let credentials = rustix::net::sockopt::socket_peercred(&peer)?;
                if self.allowed_uids.contains(&credentials.uid.as_raw()) {
                    wire::serve_binding(&mut peer, state, deadline, clock)?;
                }
                Ok(true)
            }
            Err(error)
                if matches!(
                    error.kind(),
                    io::ErrorKind::WouldBlock | io::ErrorKind::Interrupted
                ) =>
            {
                Ok(false)
            }
            Err(error) => Err(error),
        }
    }
}

impl Drop for Reader {
    fn drop(&mut self) {
        if let Ok(metadata) = fs::symlink_metadata(&self.socket) {
            if (metadata.dev(), metadata.ino()) == self.identity {
                let _ = fs::remove_file(&self.socket);
            }
        }
    }
}
