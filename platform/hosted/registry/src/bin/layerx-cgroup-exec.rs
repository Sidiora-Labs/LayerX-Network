//! Trusted cgroup-v2 supervisor for one isolated reproducible-build process tree.

use std::env;
use std::fs;
use std::io::Write as _;
use std::os::fd::AsRawFd as _;
use std::os::unix::fs::MetadataExt as _;
use std::os::unix::process::CommandExt as _;
use std::path::{Path, PathBuf};
use std::process::{Command, ExitCode};
use std::thread;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use rustix::process::{getpid, kill_process, Signal};

const POLL: Duration = Duration::from_millis(10);
const ATTACH_TIMEOUT: Duration = Duration::from_secs(5);

struct Limits {
    root: PathBuf,
    memory: u64,
    cpu_usec: u64,
    pids: u32,
    io_write: u64,
    wall_ms: u64,
    workspace: PathBuf,
    command: Vec<String>,
}

struct JobCgroup {
    path: PathBuf,
}

impl JobCgroup {
    fn kill(&self) -> Result<(), String> {
        write_control(&self.path, "cgroup.kill", b"1")
    }

    fn remove(self) -> Result<(), String> {
        let mut last_error = None;
        let mut result = Ok(());
        for _ in 0..100 {
            match fs::remove_dir(&self.path) {
                Ok(()) => {
                    last_error = None;
                    break;
                }
                Err(error) => {
                    last_error = Some(error);
                    thread::sleep(POLL);
                }
            }
        }
        if let Some(error) = last_error {
            result = Err(format!("could not remove job cgroup: {error}"));
        }
        std::mem::forget(self);
        result
    }
}

impl Drop for JobCgroup {
    fn drop(&mut self) {
        let _ = self.kill();
        for _ in 0..100 {
            if fs::remove_dir(&self.path).is_ok() {
                return;
            }
            thread::sleep(POLL);
        }
    }
}

fn value(argument: &str, name: &str) -> Option<u64> {
    argument.strip_prefix(name)?.parse().ok()
}

fn parse() -> Result<Limits, String> {
    let arguments = env::args().skip(1).collect::<Vec<_>>();
    if arguments.first().map(String::as_str) == Some("--stopped-launcher") {
        let split = arguments
            .iter()
            .position(|argument| argument == "--")
            .ok_or_else(|| "stopped launcher omitted command boundary".to_owned())?;
        let command = arguments.get(split + 1..).unwrap_or_default();
        let (program, rest) = command
            .split_first()
            .ok_or_else(|| "stopped launcher omitted command".to_owned())?;
        kill_process(getpid(), Signal::STOP).map_err(|error| error.to_string())?;
        return Err(Command::new(program).args(rest).exec().to_string());
    }
    if !arguments.iter().any(|argument| argument == "--cgroup-v2")
        || !arguments
            .iter()
            .any(|argument| argument == "--attach-before-exec")
        || !arguments
            .iter()
            .any(|argument| argument == "--kill-tree-on-exit")
    {
        return Err("mandatory cgroup supervisor contract is absent".to_owned());
    }
    let split = arguments
        .iter()
        .position(|argument| argument == "--")
        .ok_or_else(|| "supervisor omitted command boundary".to_owned())?;
    let root_index = arguments
        .iter()
        .position(|argument| argument == "--cgroup-root")
        .ok_or_else(|| "supervisor omitted cgroup root".to_owned())?;
    let root = arguments
        .get(root_index + 1)
        .map(PathBuf::from)
        .ok_or_else(|| "supervisor omitted cgroup root value".to_owned())?;
    let workspace_index = arguments
        .iter()
        .position(|argument| argument == "--workspace-device-path")
        .ok_or_else(|| "supervisor omitted quota workspace".to_owned())?;
    let workspace = arguments
        .get(workspace_index + 1)
        .map(PathBuf::from)
        .ok_or_else(|| "supervisor omitted quota workspace value".to_owned())?;
    let number = |name: &str| {
        arguments
            .iter()
            .find_map(|argument| value(argument, name))
            .filter(|number| *number > 0)
            .ok_or_else(|| format!("supervisor omitted {name}"))
    };
    let command = arguments.get(split + 1..).unwrap_or_default().to_vec();
    if command.is_empty() || !root.is_absolute() {
        return Err("supervisor command or cgroup root is invalid".to_owned());
    }
    Ok(Limits {
        root,
        memory: number("--memory-max=")?,
        cpu_usec: number("--cpu-time-max-usec=")?,
        pids: u32::try_from(number("--pids-max=")?)
            .map_err(|_| "pids limit is invalid".to_owned())?,
        io_write: number("--io-write-max=")?,
        wall_ms: number("--wall-time-max-ms=")?,
        workspace,
        command,
    })
}

fn write_control(root: &Path, name: &str, value: impl AsRef<[u8]>) -> Result<(), String> {
    let path = root.join(name);
    let mut file = fs::OpenOptions::new()
        .write(true)
        .open(&path)
        .map_err(|error| format!("{} is unavailable: {error}", path.display()))?;
    file.write_all(value.as_ref())
        .map_err(|error| error.to_string())
}

fn counter(root: &Path, file: &str, field: &str, missing_is_zero: bool) -> Result<u64, String> {
    let text = fs::read_to_string(root.join(file)).map_err(|error| error.to_string())?;
    let words = text.split_whitespace().collect::<Vec<_>>();
    let mut found = false;
    let mut total = 0_u64;
    for (index, word) in words.iter().enumerate() {
        let encoded = word
            .strip_prefix(field)
            .and_then(|value| value.strip_prefix('='));
        let adjacent = (*word == field)
            .then(|| words.get(index + 1).copied())
            .flatten();
        if let Some(raw) = encoded.or(adjacent) {
            total = total
                .checked_add(
                    raw.parse::<u64>()
                        .map_err(|_| format!("{file} contains an invalid {field} counter"))?,
                )
                .ok_or_else(|| format!("{file} {field} counter overflowed"))?;
            found = true;
        }
    }
    if found || missing_is_zero {
        Ok(total)
    } else {
        Err(format!("{file} omitted {field}"))
    }
}

fn stopped(pid: u32) -> bool {
    fs::read_to_string(format!("/proc/{pid}/status"))
        .ok()
        .and_then(|text| {
            text.lines()
                .find(|line| line.starts_with("State:"))
                .map(str::to_owned)
        })
        .is_some_and(|state| state.contains('T'))
}

fn linux_device(device: u64) -> (u64, u64) {
    let major = (device >> 8 & 0xfff) | (device >> 32 & 0xffff_f000);
    let minor = (device & 0xff) | (device >> 12 & 0xffff_ff00);
    (major, minor)
}

fn supervise(limits: Limits) -> Result<ExitCode, String> {
    let nonce = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|_| "system clock is unavailable".to_owned())?
        .as_nanos();
    let job_path = limits
        .root
        .join(format!("job-{}-{nonce}", std::process::id()));
    fs::create_dir(&job_path).map_err(|error| format!("could not create job cgroup: {error}"))?;
    let job = JobCgroup { path: job_path };
    write_control(&job.path, "memory.max", limits.memory.to_string())?;
    write_control(&job.path, "memory.swap.max", b"0")?;
    write_control(&job.path, "memory.oom.group", b"1")?;
    write_control(&job.path, "pids.max", limits.pids.to_string())?;
    if job.path.join("io.max").exists() {
        let device = fs::metadata(&limits.workspace)
            .map_err(|error| format!("quota workspace device is unavailable: {error}"))?
            .dev();
        let (major, minor) = linux_device(device);
        write_control(
            &job.path,
            "io.max",
            format!("{major}:{minor} wbps={}", limits.io_write),
        )?;
    }
    fs::OpenOptions::new()
        .write(true)
        .open(job.path.join("cgroup.kill"))
        .map_err(|error| format!("cgroup.kill is unavailable: {error}"))?;
    let executable = fs::File::open("/proc/self/exe").map_err(|error| error.to_string())?;
    rustix::io::fcntl_setfd(&executable, rustix::io::FdFlags::empty())
        .map_err(|error| format!("supervisor executable fd cannot be inherited: {error}"))?;
    let mut child = Command::new(format!("/proc/self/fd/{}", executable.as_raw_fd()))
        .arg("--stopped-launcher")
        .arg("--")
        .args(&limits.command)
        .spawn()
        .map_err(|error| error.to_string())?;
    let pid = child.id();
    let attach_deadline = Instant::now() + ATTACH_TIMEOUT;
    while !stopped(pid) {
        if Instant::now() >= attach_deadline {
            let _ = child.kill();
            job.kill()?;
            let _ = child.wait();
            return Err("child did not stop before cgroup attachment".to_owned());
        }
        thread::sleep(POLL);
    }
    if let Err(error) = write_control(&job.path, "cgroup.procs", pid.to_string()) {
        let _ = child.kill();
        let _ = child.wait();
        return Err(error);
    }
    let raw_pid = i32::try_from(pid).map_err(|_| "child pid is invalid".to_owned())?;
    kill_process(
        rustix::process::Pid::from_raw(raw_pid).ok_or_else(|| "child pid is invalid".to_owned())?,
        Signal::CONT,
    )
    .map_err(|error| error.to_string())?;
    let deadline = Instant::now() + Duration::from_millis(limits.wall_ms);
    let result = loop {
        if let Some(status) = child.try_wait().map_err(|error| error.to_string())? {
            break status
                .code()
                .and_then(|code| u8::try_from(code).ok())
                .map_or(ExitCode::FAILURE, ExitCode::from);
        }
        let cpu = counter(&job.path, "cpu.stat", "usage_usec", false)?;
        let written = counter(&job.path, "io.stat", "wbytes", true)?;
        if Instant::now() >= deadline || cpu > limits.cpu_usec || written > limits.io_write {
            job.kill()?;
            let _ = child.wait();
            break ExitCode::FAILURE;
        }
        thread::sleep(POLL);
    };
    job.kill()?;
    job.remove()?;
    Ok(result)
}

fn main() -> ExitCode {
    match parse().and_then(supervise) {
        Ok(code) => code,
        Err(error) => {
            eprintln!("layerx-cgroup-exec: {error}");
            ExitCode::FAILURE
        }
    }
}
