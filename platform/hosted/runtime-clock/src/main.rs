mod server;

use std::ffi::OsString;
use std::io;
use std::os::unix::process::CommandExt;
use std::path::PathBuf;
use std::process::{Child, Command, ExitCode};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

struct Supervised {
    child: Child,
    group: rustix::process::Pid,
}

impl Drop for Supervised {
    fn drop(&mut self) {
        let _ = rustix::process::kill_process_group(self.group, rustix::process::Signal::TERM);
        let deadline = Instant::now() + Duration::from_secs(5);
        while self.child.try_wait().is_ok_and(|status| status.is_none())
            && Instant::now() < deadline
        {
            std::thread::sleep(Duration::from_millis(10));
        }
        let _ = rustix::process::kill_process_group(self.group, rustix::process::Signal::KILL);
        let _ = self.child.wait();
    }
}

fn arguments() -> io::Result<(PathBuf, Vec<OsString>)> {
    let mut arguments = std::env::args_os().skip(1);
    let mut root = PathBuf::from("/tmp");
    match arguments.next().as_deref() {
        Some(value) if value == "--runtime-dir" => {
            root = arguments
                .next()
                .ok_or_else(|| io::Error::other("runtime directory required"))?
                .into();
            if arguments.next().as_deref() != Some(std::ffi::OsStr::new("--")) {
                return Err(io::Error::other("child command delimiter required"));
            }
        }
        Some(value) if value == "--" => {}
        _ => return Err(io::Error::other("expected [--runtime-dir path] -- command")),
    }
    let command: Vec<_> = arguments.collect();
    if command.is_empty() || command.len() > 256 {
        return Err(io::Error::other("one bounded child command is required"));
    }
    Ok((root, command))
}

fn run() -> io::Result<u8> {
    let (root, arguments) = arguments()?;
    let mut server = server::Server::bind(&root)?;
    let signals = Arc::new(AtomicUsize::new(0));
    signal_hook::flag::register_usize(signal_hook::consts::SIGTERM, Arc::clone(&signals), 1)?;
    signal_hook::flag::register_usize(signal_hook::consts::SIGINT, Arc::clone(&signals), 2)?;
    let child = Command::new("/usr/bin/setpriv")
        .args(["--pdeathsig", "KILL", "--"])
        .args(&arguments)
        .env("LAYERX_RUNTIME_CLOCK_SOCKET", server.socket())
        .env("LAYERX_RUNTIME_CLOCK_PID", std::process::id().to_string())
        .env(
            "LAYERX_RUNTIME_CLOCK_UID",
            rustix::process::geteuid().as_raw().to_string(),
        )
        .process_group(0)
        .spawn()?;
    let group = i32::try_from(child.id())
        .ok()
        .and_then(rustix::process::Pid::from_raw)
        .ok_or_else(|| io::Error::other("invalid child process group"))?;
    let mut supervised = Supervised { child, group };
    let mut shutdown = None;
    loop {
        match signals.swap(0, Ordering::AcqRel) {
            0 => {}
            signal => {
                let signal = if signal == 1 {
                    rustix::process::Signal::TERM
                } else {
                    rustix::process::Signal::INT
                };
                rustix::process::kill_process_group(supervised.group, signal)?;
                shutdown.get_or_insert_with(|| Instant::now() + Duration::from_secs(5));
            }
        }
        if let Some(status) = supervised.child.try_wait()? {
            return Ok(status
                .code()
                .and_then(|code| u8::try_from(code).ok())
                .unwrap_or(1));
        }
        if shutdown.is_some_and(|deadline| Instant::now() >= deadline) {
            return Err(io::Error::new(
                io::ErrorKind::TimedOut,
                "supervised process did not stop",
            ));
        }
        server.poll()?;
        std::thread::sleep(Duration::from_millis(1));
    }
}

fn main() -> ExitCode {
    match run() {
        Ok(code) => ExitCode::from(code),
        Err(error) => {
            eprintln!("runtime clock refused: {error}");
            ExitCode::FAILURE
        }
    }
}
