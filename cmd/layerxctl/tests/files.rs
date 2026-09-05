use std::error::Error;
use std::fs;
use std::os::unix::fs::DirBuilderExt;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

#[test]
fn rejects_fifo_without_waiting_for_a_writer() -> Result<(), Box<dyn Error>> {
    let directory = std::env::temp_dir().join(format!(
        "layerxctl-fifo-{}-{}",
        std::process::id(),
        SystemTime::now().duration_since(UNIX_EPOCH)?.as_nanos()
    ));
    fs::DirBuilder::new().mode(0o700).create(&directory)?;
    let fifo = directory.join("activity");
    assert!(Command::new("mkfifo").arg(&fifo).status()?.success());
    let mut child = Command::new(env!("CARGO_BIN_EXE_layerxctl"))
        .args([
            "submit",
            "--socket",
            "/nonexistent/layerxctl-test.sock",
            "--network-id",
            "42",
            "--protocol-version",
            "3",
            "--actor",
            "did:key:operator-test",
            "--public-key",
            &"01".repeat(32),
            "--activity",
        ])
        .arg(&fifo)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()?;
    let deadline = Instant::now() + Duration::from_secs(3);
    let timed_out = loop {
        if child.try_wait()?.is_some() {
            break false;
        }
        if Instant::now() >= deadline {
            child.kill()?;
            break true;
        }
        std::thread::sleep(Duration::from_millis(10));
    };
    let output = child.wait_with_output()?;
    fs::remove_dir_all(&directory)?;
    assert!(
        !timed_out,
        "FIFO input blocked before its file type was checked"
    );
    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("regular file"));
    Ok(())
}
