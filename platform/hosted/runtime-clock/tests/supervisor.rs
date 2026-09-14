use std::fmt::Write as _;
use std::fs;
use std::io::{Read, Write};
use std::net::Shutdown;
use std::os::unix::fs::PermissionsExt;
use std::os::unix::net::UnixStream;
use std::os::unix::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use layerx_client::runtime_clock::{ClockBinding, RuntimeClock};
use layerx_types::clock::{Clock, ClockError, Deadline};
use layerx_types::clock_protocol::{self, Request, RESPONSE_BYTES};

type Result<T = ()> = std::result::Result<T, Box<dyn std::error::Error>>;

struct Directory(PathBuf);

impl Directory {
    fn new() -> Result<Self> {
        let mut unique = [0; 16];
        getrandom::fill(&mut unique)?;
        let mut name = String::with_capacity(32);
        for byte in unique {
            write!(name, "{byte:02x}")?;
        }
        let root = std::env::temp_dir().join(format!("lxc-test-{name}"));
        fs::create_dir(&root)?;
        fs::set_permissions(&root, fs::Permissions::from_mode(0o700))?;
        Ok(Self(root))
    }
}

impl Drop for Directory {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

struct Process(Child);

impl Drop for Process {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

fn launch(root: &Path, mode: &str) -> Result<Process> {
    Ok(Process(
        Command::new(env!("CARGO_BIN_EXE_layerx-runtime-clock"))
            .args(["--runtime-dir"])
            .arg(root)
            .arg("--")
            .arg(std::env::current_exe()?)
            .args(["--exact", "actual_supervisor_capability", "--nocapture"])
            .env("LX_CLOCK_TEST_MODE", mode)
            .env("LX_CLOCK_TEST_ROOT", root)
            .stdout(Stdio::null())
            .spawn()?,
    ))
}

fn wait_file(root: &Path, child: &mut Child) -> Result<u32> {
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        if let Ok(text) = fs::read_to_string(root.join("child")) {
            return Ok(text.parse()?);
        }
        assert!(
            child.try_wait()?.is_none(),
            "child exited before clock binding"
        );
        assert!(Instant::now() < deadline, "clock child startup deadline");
        std::thread::sleep(Duration::from_millis(10));
    }
}

fn wait_exit(child: &mut Child) -> Result<std::process::ExitStatus> {
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        if let Some(status) = child.try_wait()? {
            return Ok(status);
        }
        assert!(Instant::now() < deadline, "clock process shutdown deadline");
        std::thread::sleep(Duration::from_millis(10));
    }
}

fn child_running(process: u32) -> bool {
    fs::read_to_string(format!("/proc/{process}/stat"))
        .is_ok_and(|state| !state.contains(") Z") && !state.contains(") X"))
}

fn stage_receipt(root: &Path) -> Result {
    let source = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../../tests/fixtures/programs/maintained-multicall");
    let target = root.join("fixture");
    fs::create_dir(&target)?;
    for name in ["header", "sequencer.public", "receipt-0"] {
        fs::copy(source.join(name), target.join(name))?;
    }
    Ok(())
}

fn nonroot_supervision(root: &Path) -> Result {
    let uid = rustix::process::geteuid().as_raw();
    let gid = rustix::process::getegid().as_raw();
    let (uid, gid) = if uid == 0 { (65534, 65534) } else { (uid, gid) };
    let clock = root.join("runtime-clock");
    let test = root.join("clock-test");
    fs::copy(env!("CARGO_BIN_EXE_layerx-runtime-clock"), &clock)?;
    fs::copy(std::env::current_exe()?, &test)?;
    fs::set_permissions(&clock, fs::Permissions::from_mode(0o555))?;
    fs::set_permissions(&test, fs::Permissions::from_mode(0o555))?;
    fs::set_permissions(root, fs::Permissions::from_mode(0o755))?;
    let runtime = root.join("nonroot");
    fs::create_dir(&runtime)?;
    fs::set_permissions(&runtime, fs::Permissions::from_mode(0o700))?;
    stage_receipt(&runtime)?;
    if rustix::process::geteuid().as_raw() == 0 {
        rustix::fs::chown(
            &runtime,
            Some(rustix::process::Uid::from_raw(uid)),
            Some(rustix::process::Gid::from_raw(gid)),
        )?;
    }
    let mut child = Process(
        Command::new(clock)
            .arg("--runtime-dir")
            .arg(&runtime)
            .arg("--")
            .arg(test)
            .args(["--exact", "actual_supervisor_capability", "--nocapture"])
            .env("LX_CLOCK_TEST_MODE", "sample")
            .env("LX_CLOCK_TEST_ROOT", &runtime)
            .env("LX_CLOCK_TEST_EXPECTED_UID", uid.to_string())
            .uid(uid)
            .gid(gid)
            .stdout(Stdio::null())
            .spawn()?,
    );
    assert!(wait_exit(&mut child.0)?.success());
    Ok(())
}

fn qualified_child(mode: &str, root: &Path) -> Result {
    if let Ok(expected) = std::env::var("LX_CLOCK_TEST_EXPECTED_UID") {
        assert_eq!(
            rustix::process::geteuid().as_raw(),
            expected.parse::<u32>()?
        );
        assert_ne!(rustix::process::geteuid().as_raw(), 0);
    }
    let binding = ClockBinding::from_environment()?;
    let clock = RuntimeClock::connect(binding.clone())?;
    if mode == "inherited" {
        assert_eq!(
            binding.process,
            std::env::var("LX_CLOCK_TEST_EXPECTED_PARENT")?.parse::<u32>()?
        );
        return Ok(());
    }
    fs::write(root.join("child"), std::process::id().to_string())?;
    if mode == "capacity" {
        let mut occupied = Vec::new();
        for counter in 1..=64 {
            let mut stream = UnixStream::connect(&binding.socket)?;
            stream.set_write_timeout(Some(Duration::from_secs(1)))?;
            stream.write_all(
                &Request {
                    counter,
                    wait_nanoseconds: clock_protocol::MAX_WAIT_NANOSECONDS,
                }
                .encode()?,
            )?;
            stream.shutdown(Shutdown::Write)?;
            occupied.push(stream);
        }
        std::thread::sleep(Duration::from_millis(50));
        assert!(clock.sample(Duration::from_secs(1)).is_err());
        return Ok(());
    }
    if mode == "kill" {
        loop {
            clock.wait(Duration::from_secs(300))?;
        }
    }
    if mode == "signal" {
        let stopped = Arc::new(AtomicBool::new(false));
        signal_hook::flag::register(signal_hook::consts::SIGTERM, stopped.clone())?;
        signal_hook::flag::register(signal_hook::consts::SIGINT, stopped.clone())?;
        fs::write(root.join("signals-ready"), b"ready")?;
        while !stopped.load(Ordering::Acquire) {
            clock.sample(Duration::from_secs(1))?;
            std::thread::sleep(Duration::from_millis(1));
        }
        return Ok(());
    }
    let first = clock.sample(Duration::from_secs(1))?;
    let second = clock.wait(Duration::from_millis(1_100))?;
    assert!(second.elapsed_since(first)? >= Duration::from_millis(1_100));
    assert!(second.unix_milliseconds >= first.unix_milliseconds);
    let mut deadline = Deadline::start(clock.as_ref(), Duration::from_millis(20))?;
    clock.wait(Duration::from_millis(25))?;
    assert!(deadline.remaining(clock.as_ref())?.is_zero());
    let mut wrong = binding.clone();
    wrong.process = wrong.process.checked_add(1).ok_or("process overflow")?;
    assert!(RuntimeClock::connect(wrong).is_err());
    let mut wrong = binding.clone();
    wrong.user = wrong.user.checked_add(1).ok_or("uid overflow")?;
    assert!(RuntimeClock::connect(wrong).is_err());
    assert!(clock.sample(Duration::ZERO).is_err());
    assert!(clock.wait(Duration::from_secs(301)).is_err());
    assert!(matches!(
        Deadline::start(clock.as_ref(), Duration::MAX),
        Err(ClockError::Overflow)
    ));
    actual_codec(&binding)?;
    malformed_clients(&binding)?;
    assert!(clock
        .sample(Duration::from_secs(1))?
        .follows(second)
        .is_ok());
    stalled_public_read(clock.clone(), root)?;
    let status = Command::new(std::env::current_exe()?)
        .args(["--exact", "actual_supervisor_capability", "--nocapture"])
        .env("LX_CLOCK_TEST_MODE", "inherited")
        .env("LX_CLOCK_TEST_EXPECTED_PARENT", binding.process.to_string())
        .stdout(Stdio::null())
        .status()?;
    assert!(status.success());
    Ok(())
}

fn stalled_public_read(clock: Arc<RuntimeClock>, root: &Path) -> Result {
    use layerx_sdk::rpc::{Commitment, RpcClient, RpcError};
    use layerx_sdk::rpc_verification::ReceiptPolicy;
    let fixture = root.join("fixture");
    let header = layerx_wire::receipt::decode_batch_header(&fs::read(fixture.join("header"))?)
        .map_err(|error| format!("native batch fixture: {error:?}"))?;
    let key: [u8; 32] = fs::read(fixture.join("sequencer.public"))?
        .try_into()
        .map_err(|_| "fixture key length")?;
    let receipt = layerx_proof::receipt::verify_sequencer_signature(
        &fs::read(fixture.join("receipt-0"))?,
        key,
    )
    .map_err(|error| format!("native receipt fixture: {error:?}"))?;
    let activity = receipt
        .protocol()
        .ok_or("native protocol receipt")?
        .activity_id();
    let policy = ReceiptPolicy {
        protocol_version: header.protocol_version(),
        network_id: header.network_id(),
        sequencer: layerx_proof::inclusion::SequencerAuthorization::new(
            header.sequencer_id(),
            key,
            0,
            u64::MAX,
        ),
        trusted_checkpoint_context_digest: None,
    };
    let listener = std::net::TcpListener::bind("127.0.0.1:0")?;
    let address = listener.local_addr()?;
    let peer = std::thread::spawn(move || -> std::io::Result<()> {
        let (stream, _) = listener.accept()?;
        std::thread::sleep(Duration::from_millis(300));
        drop(stream);
        Ok(())
    });
    let client = RpcClient::connect(&format!("http://{address}"), None)
        .map_err(|error| format!("RPC: {error:?}"))?
        .with_clock(clock);
    let start = Instant::now();
    assert!(
        matches!(client.wait_for(activity, Commitment::Executed, &policy, Duration::from_millis(20)), Err(RpcError::Pending { activity_id }) if activity_id == activity)
    );
    assert!(start.elapsed() < Duration::from_millis(200));
    peer.join().map_err(|_| "stalled peer panicked")??;
    Ok(())
}

fn actual_codec(binding: &ClockBinding) -> Result {
    let mut stream = UnixStream::connect(&binding.socket)?;
    stream.set_read_timeout(Some(Duration::from_secs(1)))?;
    stream.write_all(
        &Request {
            counter: 1,
            wait_nanoseconds: 0,
        }
        .encode()?,
    )?;
    stream.shutdown(Shutdown::Write)?;
    let mut response = [0; RESPONSE_BYTES];
    stream.read_exact(&mut response)?;
    let reading = clock_protocol::decode_response(&response, 1)?;
    for index in [0, 4, 6, 7, 15] {
        let mut changed = response;
        changed[index] ^= 1;
        assert!(clock_protocol::decode_response(&changed, 1).is_err());
    }
    let mut changed = response;
    changed[16..32].fill(0);
    assert!(clock_protocol::decode_response(&changed, 1).is_err());
    let mut changed = reading;
    changed.generation[0] ^= 1;
    assert_eq!(changed.follows(reading), Err(ClockError::Regression));
    let mut changed = reading;
    changed.monotonic_nanoseconds = reading
        .monotonic_nanoseconds
        .checked_sub(1)
        .ok_or("monotonic start")?;
    assert_eq!(changed.follows(reading), Err(ClockError::Regression));
    let mut changed = reading;
    changed.unix_milliseconds = reading
        .unix_milliseconds
        .checked_sub(1)
        .ok_or("wall start")?;
    assert_eq!(changed.follows(reading), Err(ClockError::Regression));
    assert!(Request {
        counter: 0,
        wait_nanoseconds: 0
    }
    .encode()
    .is_err());
    assert!(Request {
        counter: 1,
        wait_nanoseconds: u64::MAX
    }
    .encode()
    .is_err());
    Ok(())
}

fn malformed_clients(binding: &ClockBinding) -> Result {
    let original = Request {
        counter: 1,
        wait_nanoseconds: 0,
    }
    .encode()?;
    let mut malformed = original.to_vec();
    malformed[4] ^= 1;
    let mut trailing = original.to_vec();
    trailing.push(0);
    for bytes in [original[..10].to_vec(), malformed, trailing] {
        let mut stream = UnixStream::connect(&binding.socket)?;
        stream.set_read_timeout(Some(Duration::from_secs(2)))?;
        stream.write_all(&bytes)?;
        stream.shutdown(Shutdown::Write)?;
        let mut response = [0; 1];
        assert!(matches!(stream.read(&mut response), Ok(0) | Err(_)));
    }
    let start = Instant::now();
    let mut stream = UnixStream::connect(&binding.socket)?;
    stream.set_read_timeout(Some(Duration::from_secs(2)))?;
    for byte in original {
        if stream.write_all(&[byte]).is_err() {
            break;
        }
        std::thread::sleep(Duration::from_millis(100));
    }
    let mut response = [0; 1];
    assert!(matches!(stream.read(&mut response), Ok(0) | Err(_)));
    assert!(start.elapsed() < Duration::from_secs(2));
    Ok(())
}

#[test]
fn actual_supervisor_capability() -> Result {
    if let Ok(mode) = std::env::var("LX_CLOCK_TEST_MODE") {
        let root = PathBuf::from(std::env::var_os("LX_CLOCK_TEST_ROOT").ok_or("test root")?);
        return qualified_child(&mode, &root);
    }
    let directory = Directory::new()?;
    stage_receipt(&directory.0)?;
    let mut process = launch(&directory.0, "sample")?;
    assert!(wait_exit(&mut process.0)?.success());
    assert!(fs::read_dir(&directory.0)?.all(|entry| entry
        .is_ok_and(|entry| entry.file_name() == "child" || entry.file_name() == "fixture")));
    let mut capacity = launch(&directory.0, "capacity")?;
    assert!(wait_exit(&mut capacity.0)?.success());
    for signal in [
        rustix::process::Signal::TERM,
        rustix::process::Signal::INT,
        rustix::process::Signal::KILL,
    ] {
        fs::remove_file(directory.0.join("child"))?;
        let mode = if signal == rustix::process::Signal::KILL {
            "kill"
        } else {
            "signal"
        };
        let mut process = launch(&directory.0, mode)?;
        let child = wait_file(&directory.0, &mut process.0)?;
        if mode == "signal" {
            let deadline = Instant::now() + Duration::from_secs(2);
            while !directory.0.join("signals-ready").exists() {
                assert!(Instant::now() < deadline);
                std::thread::sleep(Duration::from_millis(1));
            }
        }
        let supervisor = rustix::process::Pid::from_raw(i32::try_from(process.0.id())?)
            .ok_or("supervisor pid")?;
        rustix::process::kill_process(supervisor, signal)?;
        let status = wait_exit(&mut process.0)?;
        assert_eq!(status.success(), mode == "signal");
        let deadline = Instant::now() + Duration::from_secs(2);
        while child_running(child) {
            assert!(
                Instant::now() < deadline,
                "supervised child survived parent shutdown"
            );
            std::thread::sleep(Duration::from_millis(1));
        }
        if mode == "signal" {
            fs::remove_file(directory.0.join("signals-ready"))?;
        }
    }
    fs::remove_file(directory.0.join("child"))?;
    let mut late = Process(
        Command::new(std::env::current_exe()?)
            .args(["--exact", "actual_supervisor_capability", "--nocapture"])
            .env("LX_CLOCK_TEST_MODE", "sample")
            .env("LX_CLOCK_TEST_ROOT", &directory.0)
            .env("LAYERX_RUNTIME_CLOCK_SOCKET", directory.0.join("dead.sock"))
            .env("LAYERX_RUNTIME_CLOCK_PID", "1")
            .env(
                "LAYERX_RUNTIME_CLOCK_UID",
                rustix::process::geteuid().as_raw().to_string(),
            )
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()?,
    );
    assert!(!wait_exit(&mut late.0)?.success());
    assert!(!directory.0.join("child").exists());
    nonroot_supervision(&directory.0)?;
    Ok(())
}
