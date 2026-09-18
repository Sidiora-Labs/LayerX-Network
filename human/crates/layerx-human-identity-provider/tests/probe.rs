use layerx_client::runtime_clock::RuntimeClock;
use std::error::Error;
use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::Path;
use std::process::{Command, Output};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::thread::{self, JoinHandle};
use std::time::Duration;

use layerx_human_identity_provider::{Policy, Server, State};

type Result<T = ()> = std::result::Result<T, Box<dyn Error>>;

fn policy() -> Policy {
    Policy {
        root: [0x51; 32],
        threshold: 1,
        delay_seconds: 86_400,
    }
}

fn probe(socket: &Path) -> Result<Output> {
    Ok(
        Command::new(env!("CARGO_BIN_EXE_layerx-human-identity-provider"))
            .arg("probe")
            .env("LAYERX_HUMAN_IDENTITY_PROVIDER_SOCKET", socket)
            .env("LAYERX_HUMAN_IDENTITY_PROVIDER_DEADLINE_SECONDS", "5")
            .output()?,
    )
}

#[test]
fn probe_reports_ready_only_while_the_identity_provider_serves() -> Result {
    let directory = tempfile::Builder::new()
        .permissions(fs::Permissions::from_mode(0o700))
        .tempdir()?;
    let root = directory.path().join("state");
    let socket = directory.path().join("identity.sock");

    let absent = probe(&socket)?;
    assert_eq!(absent.status.code(), Some(1));
    assert!(absent.stdout.is_empty());

    let server = Server::bind(
        &socket,
        State::open(&root, policy())?,
        rustix::process::geteuid().as_raw(),
        Duration::from_secs(5),
        RuntimeClock::from_environment()?,
    )?;
    let shutdown = Arc::new(AtomicBool::new(false));
    let flag = Arc::clone(&shutdown);
    let worker: JoinHandle<std::io::Result<()>> = thread::spawn(move || server.run(&flag));

    let ready = probe(&socket)?;
    assert_eq!(ready.status.code(), Some(0));
    assert!(ready.stdout.is_empty());

    shutdown.store(true, Ordering::Release);
    worker.join().map_err(|_| "identity provider panicked")??;

    let stopped = probe(&socket)?;
    assert_eq!(stopped.status.code(), Some(1));
    Ok(())
}

#[test]
fn probe_refuses_an_incomplete_socket_configuration() -> Result {
    let missing = Command::new(env!("CARGO_BIN_EXE_layerx-human-identity-provider"))
        .arg("probe")
        .env_remove("LAYERX_HUMAN_IDENTITY_PROVIDER_SOCKET")
        .output()?;
    assert_eq!(missing.status.code(), Some(1));
    let relative = Command::new(env!("CARGO_BIN_EXE_layerx-human-identity-provider"))
        .arg("probe")
        .env("LAYERX_HUMAN_IDENTITY_PROVIDER_SOCKET", "identity.sock")
        .output()?;
    assert_eq!(relative.status.code(), Some(1));
    Ok(())
}
