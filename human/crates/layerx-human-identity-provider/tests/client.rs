use std::error::Error;
use std::fs;
use std::io::Write;
use std::os::unix::fs::PermissionsExt;
use std::os::unix::net::UnixStream;
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::thread::{self, JoinHandle};
use std::time::Duration;

use layerx_human_identity_provider::{Policy, Server, State};
pub use layerx_human_service::{auth, onboarding, security, store};
use store::PrincipalId;

#[path = "../../layerx-human-service/src/server/identity_dispatch.rs"]
mod source_client;
use source_client::{IdentityProviderConfig, RemoteIdentityProvider};

type Result<T = ()> = std::result::Result<T, Box<dyn Error>>;

trait DirectoryRead {
    fn resolve_email(&self, email: &str) -> Result<PrincipalId>;
    fn device_for_assertion(
        &self,
        principal: &PrincipalId,
        assertion: &str,
    ) -> Result<auth::Device>;
}

impl DirectoryRead for RemoteIdentityProvider {
    fn resolve_email(&self, email: &str) -> Result<PrincipalId> {
        let fields = self.call(2, &[email.as_bytes()])?;
        let [principal] = fields.as_slice() else {
            return Err("invalid resolve response".into());
        };
        Ok(PrincipalId::new(std::str::from_utf8(principal)?)?)
    }

    fn device_for_assertion(
        &self,
        principal: &PrincipalId,
        assertion: &str,
    ) -> Result<auth::Device> {
        let fields = self.call(3, &[principal.as_str().as_bytes(), assertion.as_bytes()])?;
        let [id, label, platform] = fields.as_slice() else {
            return Err("invalid device response".into());
        };
        Ok(auth::Device::new(
            std::str::from_utf8(id)?,
            std::str::from_utf8(label)?,
            std::str::from_utf8(platform)?,
        )?)
    }
}

fn policy() -> Policy {
    Policy {
        root: [0x43; 32],
        threshold: 1,
        delay_seconds: 86_400,
    }
}

struct Running {
    shutdown: Arc<AtomicBool>,
    worker: Option<JoinHandle<std::io::Result<()>>>,
}
impl Running {
    fn start(socket: &Path, root: &Path, allowed_uid: u32) -> Result<Self> {
        let server = Server::bind(
            socket,
            State::open(root, policy())?,
            allowed_uid,
            Duration::from_millis(200),
        )?;
        let shutdown = Arc::new(AtomicBool::new(false));
        let flag = Arc::clone(&shutdown);
        Ok(Self {
            shutdown,
            worker: Some(thread::spawn(move || server.run(&flag))),
        })
    }
    fn stop(mut self) -> Result {
        self.shutdown.store(true, Ordering::Release);
        self.worker
            .take()
            .ok_or("worker missing")?
            .join()
            .map_err(|_| "worker panicked")??;
        Ok(())
    }
}
impl Drop for Running {
    fn drop(&mut self) {
        self.shutdown.store(true, Ordering::Release);
        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        }
    }
}

fn client(socket: &Path) -> Result<RemoteIdentityProvider> {
    RemoteIdentityProvider::new(IdentityProviderConfig {
        socket: socket.to_owned(),
        deadline: Duration::from_secs(2),
        maximum_frame_bytes: 1_048_576,
        peer_uid: rustix::process::geteuid().as_raw(),
        peer_gid: rustix::process::getegid().as_raw(),
    })
    .map_err(|error| format!("{error:?}").into())
}

#[test]
fn original_client_provisions_resolves_and_replays_assertion_devices() -> Result {
    let directory = tempfile::Builder::new()
        .permissions(fs::Permissions::from_mode(0o700))
        .tempdir()?;
    let socket = directory.path().join("identity.sock");
    let root = directory.path().join("state");
    let uid = rustix::process::geteuid().as_raw();
    let running = Running::start(&socket, &root, uid)?;
    let client = client(&socket)?;
    client
        .probe()
        .map_err(|error| format!("initial probe: {error:?}"))?;
    let account = client
        .provision("person@example.com", "Person", "key", 1)
        .map_err(|error| format!("first provision: {error:?}"))?;
    let retry = client
        .provision("person@example.com", "Person", "key", 2)
        .map_err(|error| format!("idempotent provision: {error:?}"))?;
    assert_eq!(account.principal, retry.principal);
    assert_eq!(account.onboarding, retry.onboarding);
    assert_eq!(
        client
            .resolve_email("person@example.com")
            .map_err(|error| format!("resolve provisioned email: {error:?}"))?,
        account.principal
    );
    assert!(client
        .device_for_assertion(&account.principal, "assertion-1")
        .is_err());
    assert!(client.resolve_email("unknown@example.com").is_err());
    assert!(client
        .provision("different@example.com", "Person", "key", 3)
        .is_err());
    running.stop()?;
    let mut state = State::open(&root, policy())?;
    let device = auth::Device::mint("Personal laptop", "linux")?;
    state.bind_device(&account.principal, "assertion-1", device.clone())?;
    drop(state);
    let running = Running::start(&socket, &root, uid)?;
    client
        .probe()
        .map_err(|error| format!("restarted probe: {error:?}"))?;
    assert_eq!(
        client
            .device_for_assertion(&account.principal, "assertion-1")
            .map_err(|error| format!("resolve assertion after restart: {error:?}"))?,
        device
    );
    let other = client
        .provision("other@example.com", "Other", "other-key", 4)
        .map_err(|error| format!("second provision: {error:?}"))?;
    assert!(client
        .device_for_assertion(&other.principal, "assertion-1")
        .is_err());
    assert_eq!(
        client
            .provision("person@example.com", "Person", "key", 5)
            .map_err(|error| format!("replay first provision: {error:?}"))?
            .onboarding,
        account.onboarding
    );
    running.stop()?;
    Ok(())
}

#[test]
fn original_client_observes_wrong_uid_refusal_and_survives_malformed_peer() -> Result {
    let directory = tempfile::Builder::new()
        .permissions(fs::Permissions::from_mode(0o700))
        .tempdir()?;
    let socket = directory.path().join("identity.sock");
    let root = directory.path().join("state");
    let uid = rustix::process::geteuid().as_raw();
    let running = Running::start(&socket, &root, uid ^ 1)?;
    assert!(client(&socket)?.probe().is_err());
    running.stop()?;
    let running = Running::start(&socket, &root, uid)?;
    {
        let mut malformed = UnixStream::connect(&socket)?;
        malformed.write_all(&u32::MAX.to_be_bytes())?;
    }
    client(&socket)?.probe().map_err(|e| format!("{e:?}"))?;
    running.stop()?;
    fs::write(root.join("state.json"), b"corrupt")?;
    assert!(State::open(&root, policy()).is_err());
    assert!(client(&socket)?.probe().is_err());
    Ok(())
}

#[test]
fn original_source_auxiliary_items_remain_type_checked() -> Result {
    let _ = source_client::security_digest;
    let _ = source_client::profile;
    let _ = source_client::update_profile;
    let _ = source_client::step_up_challenge;
    let _ = source_client::step_up_evidence;
    let _ = source_client::onboarding_status;
    let store_error = PrincipalId::new("invalid principal")
        .err()
        .ok_or("invalid principal accepted")?;
    let error = source_client::IdentityDispatchError::from(store_error);
    match error {
        source_client::IdentityDispatchError::Store(inner) => {
            assert!(matches!(inner, store::StoreError::InvalidPrincipal));
        }
        _ => return Err("store error mapping changed".into()),
    }
    Ok(())
}
