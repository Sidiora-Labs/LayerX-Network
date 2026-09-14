use super::{checked, fixture::Fixture, Result};
use ed25519_dalek::SigningKey;
use serde_json::{json, Value};
use sha2::{Digest as _, Sha256};
use std::io::Write as _;
use std::os::unix::fs::PermissionsExt as _;
use std::path::PathBuf;
use std::process::{Child, Command, Stdio};
use std::time::Duration;

pub struct Identity {
    pub principal: String,
    pub did: layerx_types::ids::Did,
    pub recovery_root: [u8; 32],
    pub binding: PathBuf,
    pub root: PathBuf,
    process: Child,
}
impl Drop for Identity {
    fn drop(&mut self) {
        if let Some(pid) =
            rustix::process::Pid::from_raw(self.process.id().try_into().unwrap_or_default())
        {
            let _ = rustix::process::kill_process(pid, rustix::process::Signal::TERM);
        }
        let _ = self.process.wait();
    }
}
fn protected(path: &std::path::Path, bytes: &[u8]) -> Result<()> {
    use std::os::unix::fs::OpenOptionsExt as _;
    let mut file = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(path)?;
    file.write_all(bytes)?;
    file.sync_all()?;
    Ok(())
}
fn recovery_root() -> [u8; 32] {
    let mut commitment = Sha256::new();
    commitment.update(b"LX:HUMAN:RECOVERY:v1\0");
    commitment.update(1_u16.to_be_bytes());
    commitment.update(1_u16.to_be_bytes());
    commitment.update(
        SigningKey::from_bytes(&[0x55; 32])
            .verifying_key()
            .as_bytes(),
    );
    commitment.finalize().into()
}

pub fn start(fixture: &Fixture) -> Result<Identity> {
    use layerx_types::clock::Clock as _;
    let clock = layerx_client::runtime_clock::RuntimeClock::from_environment()?;
    let root = std::env::temp_dir().join(format!("lx-managed-identity-{}", std::process::id()));
    protected(
        &fixture.directory.join("identity-provider-path"),
        root.as_os_str().as_encoded_bytes(),
    )?;
    std::fs::create_dir(&root)?;
    std::fs::set_permissions(&root, std::fs::Permissions::from_mode(0o700))?;
    let recovery_root = recovery_root();
    let policy = root.join("recovery.json");
    protected(
        &policy,
        &serde_json::to_vec(&json!({"root":recovery_root,"threshold":1,"delay_seconds":60}))?,
    )?;
    let binary = std::env::var("LAYERX_TEST_MANAGED_IDENTITY_BIN")?;
    let mut command = Command::new(&binary);
    command
        .arg("provision-owner")
        .env(
            "LAYERX_HUMAN_IDENTITY_PROVIDER_STATE_ROOT",
            root.join("state"),
        )
        .env(
            "LAYERX_HUMAN_IDENTITY_PROVIDER_RECOVERY_POLICY_FILE",
            &policy,
        )
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::inherit());
    let mut child = command.spawn()?;
    let now = clock.sample(Duration::from_secs(1))?.unix_milliseconds / 1000;
    child.stdin.take().ok_or("provider input missing")?.write_all(&serde_json::to_vec(&json!({
        "email":"managed-limit@example.test","display_name":"Managed limit fixture","idempotency_key":"managed-limit-real-owner","now":now}))?)?;
    let output = child.wait_with_output()?;
    assert!(
        output.status.success(),
        "actual Identity provisioning refused"
    );
    let value: Value = serde_json::from_slice(&output.stdout)?;
    let principal = value["principal"]
        .as_str()
        .ok_or("provider principal")?
        .to_owned();
    let did = checked(layerx_types::ids::Did::new(
        value["did"].as_str().ok_or("provider DID")?.as_bytes(),
    ))?;
    assert_eq!(value["recovery_root"], json!(recovery_root));
    let binding = root.join("binding.sock");
    let log = std::fs::File::create(root.join("service.log"))?;
    let process = Command::new(binary)
        .arg("serve")
        .env(
            "LAYERX_HUMAN_IDENTITY_PROVIDER_STATE_ROOT",
            root.join("state"),
        )
        .env(
            "LAYERX_HUMAN_IDENTITY_PROVIDER_RECOVERY_POLICY_FILE",
            policy,
        )
        .env(
            "LAYERX_HUMAN_IDENTITY_PROVIDER_SOCKET",
            root.join("identity.sock"),
        )
        .env("LAYERX_HUMAN_IDENTITY_PROVIDER_ALLOWED_UID", "4021")
        .env("LAYERX_HUMAN_IDENTITY_PROVIDER_BINDING_SOCKET", &binding)
        .env(
            "LAYERX_HUMAN_IDENTITY_PROVIDER_BINDING_TENANT",
            "native-managed-limit",
        )
        .env(
            "LAYERX_HUMAN_IDENTITY_PROVIDER_BINDING_ALLOWED_UIDS",
            "4021",
        )
        .stdout(log.try_clone()?)
        .stderr(log)
        .spawn()?;
    let mut identity = Identity {
        principal,
        did,
        recovery_root,
        binding,
        root,
        process,
    };
    ready(&mut identity, clock)?;
    Ok(identity)
}

fn ready(
    identity: &mut Identity,
    clock: std::sync::Arc<layerx_client::runtime_clock::RuntimeClock>,
) -> Result<()> {
    let mut deadline =
        layerx_types::clock::Deadline::start(clock.as_ref(), Duration::from_secs(90))?;
    while !identity.binding.exists() {
        assert!(
            identity.process.try_wait()?.is_none(),
            "actual Identity provider exited"
        );
        if deadline.remaining(clock.as_ref())?.is_zero() {
            return Err("Identity binding readiness deadline".into());
        }
        clock.wait(Duration::from_millis(25))?;
    }
    let client = layerx_identity_binding::Client::new(
        layerx_identity_binding::Config {
            socket: identity.binding.clone(),
            tenant: "native-managed-limit".to_owned(),
            peer_uid: 4021,
            peer_gid: 4021,
            deadline: Duration::from_secs(5),
        },
        clock,
    )?;
    let actual = client.lookup(&identity.principal)?;
    assert_eq!(actual.did(), &identity.did);
    Ok(())
}
