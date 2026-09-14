use std::fmt::Write as _;
use std::fs::{self, DirBuilder};
use std::io::{self, Read, Write};
use std::net::Shutdown;
use std::os::unix::fs::{DirBuilderExt, MetadataExt, PermissionsExt};
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use layerx_types::clock::{ClockError, ClockReading};
use layerx_types::clock_protocol::{self, Request, REQUEST_BYTES};

struct TimeAuthority {
    generation: [u8; 16],
    origin: Instant,
    previous: Option<ClockReading>,
    failed: bool,
}

impl TimeAuthority {
    fn sample(&mut self) -> Result<ClockReading, ClockError> {
        if self.failed {
            return Err(ClockError::Unavailable);
        }
        let result = self.read();
        if result.is_err() {
            self.failed = true;
        }
        result
    }

    fn read(&mut self) -> Result<ClockReading, ClockError> {
        let wall = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_err(|_| ClockError::Regression)?;
        let monotonic = Instant::now()
            .checked_duration_since(self.origin)
            .ok_or(ClockError::Regression)?;
        let reading = ClockReading {
            generation: self.generation,
            unix_milliseconds: u64::try_from(wall.as_millis()).map_err(|_| ClockError::Overflow)?,
            monotonic_nanoseconds: u64::try_from(monotonic.as_nanos())
                .map_err(|_| ClockError::Overflow)?,
        };
        if let Some(previous) = self.previous {
            reading.follows(previous)?;
        }
        self.previous = Some(reading);
        Ok(reading)
    }
}

pub(crate) struct Server {
    listener: UnixListener,
    directory: PathBuf,
    socket: PathBuf,
    identity: (u64, u64),
    authority: Arc<Mutex<TimeAuthority>>,
    shutdown: Arc<AtomicBool>,
    workers: Vec<JoinHandle<()>>,
}

impl Server {
    pub(crate) fn bind(root: &Path) -> io::Result<Self> {
        let metadata = fs::symlink_metadata(root)?;
        if !root.is_absolute() || !metadata.is_dir() {
            return Err(io::Error::other(
                "clock runtime root is not an absolute directory",
            ));
        }
        let mut generation = [0; 16];
        getrandom::fill(&mut generation)
            .map_err(|_| io::Error::other("clock entropy unavailable"))?;
        if generation == [0; 16] {
            return Err(io::Error::other("invalid clock generation"));
        }
        let mut name = String::with_capacity(32);
        for byte in generation {
            write!(name, "{byte:02x}").map_err(io::Error::other)?;
        }
        let directory = root.join(format!("lxc-{name}"));
        DirBuilder::new().mode(0o700).create(&directory)?;
        let socket = directory.join("clock.sock");
        let listener = match UnixListener::bind(&socket) {
            Ok(listener) => listener,
            Err(error) => {
                let _ = fs::remove_dir(&directory);
                return Err(error);
            }
        };
        let metadata = fs::symlink_metadata(&directory)?;
        let server = Self {
            listener,
            directory,
            socket,
            identity: (metadata.dev(), metadata.ino()),
            authority: Arc::new(Mutex::new(TimeAuthority {
                generation,
                origin: Instant::now(),
                previous: None,
                failed: false,
            })),
            shutdown: Arc::new(AtomicBool::new(false)),
            workers: Vec::new(),
        };
        fs::set_permissions(&server.socket, fs::Permissions::from_mode(0o600))?;
        rustix::net::listen(&server.listener, 16)?;
        server.listener.set_nonblocking(true)?;
        server
            .authority
            .lock()
            .map_err(|_| io::Error::other("clock unavailable"))?
            .sample()
            .map_err(io::Error::other)?;
        Ok(server)
    }

    pub(crate) fn socket(&self) -> &Path {
        &self.socket
    }

    pub(crate) fn poll(&mut self) -> io::Result<()> {
        let mut index = 0;
        while index < self.workers.len() {
            if self.workers[index].is_finished() {
                self.workers
                    .swap_remove(index)
                    .join()
                    .map_err(|_| io::Error::other("clock worker failed"))?;
            } else {
                index += 1;
            }
        }
        for _ in 0..64 {
            let stream = match self.listener.accept() {
                Ok((stream, _)) => stream,
                Err(error) if error.kind() == io::ErrorKind::WouldBlock => break,
                Err(error) if error.kind() == io::ErrorKind::Interrupted => continue,
                Err(error) => return Err(error),
            };
            if self.workers.len() >= 64 {
                continue;
            }
            let peer = rustix::net::sockopt::socket_peercred(&stream)?;
            if peer.uid != rustix::process::geteuid() {
                continue;
            }
            let authority = Arc::clone(&self.authority);
            let shutdown = Arc::clone(&self.shutdown);
            self.workers.push(
                std::thread::Builder::new()
                    .name("runtime-clock-peer".into())
                    .spawn(move || {
                        let _ = serve(stream, &authority, &shutdown);
                    })?,
            );
        }
        Ok(())
    }
}

impl Drop for Server {
    fn drop(&mut self) {
        self.shutdown.store(true, Ordering::Release);
        for worker in self.workers.drain(..) {
            let _ = worker.join();
        }
        if fs::symlink_metadata(&self.directory)
            .is_ok_and(|metadata| (metadata.dev(), metadata.ino()) == self.identity)
        {
            let _ = fs::remove_file(&self.socket);
            let _ = fs::remove_dir(&self.directory);
        }
    }
}

fn serve(
    mut stream: UnixStream,
    authority: &Mutex<TimeAuthority>,
    shutdown: &AtomicBool,
) -> io::Result<()> {
    let deadline = Instant::now() + Duration::from_secs(1);
    let mut bytes = [0; REQUEST_BYTES];
    read_before(&mut stream, &mut bytes, deadline)?;
    stream.set_read_timeout(Some(remaining(deadline)?))?;
    let mut trailing = [0];
    if stream.read(&mut trailing)? != 0 {
        return Err(io::Error::other("clock request has trailing bytes"));
    }
    let request = Request::decode(&bytes).map_err(io::Error::other)?;
    let until = Instant::now()
        .checked_add(Duration::from_nanos(request.wait_nanoseconds))
        .ok_or_else(|| io::Error::other("clock wait overflow"))?;
    while Instant::now() < until {
        if shutdown.load(Ordering::Acquire) {
            return Err(io::Error::other("clock stopped"));
        }
        std::thread::sleep(
            until
                .saturating_duration_since(Instant::now())
                .min(Duration::from_millis(10)),
        );
    }
    let reading = authority
        .lock()
        .map_err(|_| io::Error::other("clock unavailable"))?
        .sample();
    let response = clock_protocol::response(request.counter, reading);
    write_before(
        &mut stream,
        &response,
        Instant::now() + Duration::from_secs(1),
    )?;
    stream.shutdown(Shutdown::Write)
}

fn remaining(deadline: Instant) -> io::Result<Duration> {
    deadline
        .checked_duration_since(Instant::now())
        .filter(|value| !value.is_zero())
        .ok_or_else(|| io::Error::new(io::ErrorKind::TimedOut, "clock frame expired"))
}

fn read_before(stream: &mut UnixStream, mut bytes: &mut [u8], deadline: Instant) -> io::Result<()> {
    while !bytes.is_empty() {
        stream.set_read_timeout(Some(remaining(deadline)?))?;
        match stream.read(bytes)? {
            0 => {
                return Err(io::Error::new(
                    io::ErrorKind::UnexpectedEof,
                    "incomplete clock request",
                ))
            }
            length => bytes = &mut bytes[length..],
        }
    }
    Ok(())
}

fn write_before(stream: &mut UnixStream, mut bytes: &[u8], deadline: Instant) -> io::Result<()> {
    while !bytes.is_empty() {
        stream.set_write_timeout(Some(remaining(deadline)?))?;
        match stream.write(bytes)? {
            0 => {
                return Err(io::Error::new(
                    io::ErrorKind::WriteZero,
                    "incomplete clock response",
                ))
            }
            length => bytes = &bytes[length..],
        }
    }
    Ok(())
}
