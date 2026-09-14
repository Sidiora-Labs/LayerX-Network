use std::error::Error;
use std::fs;
use std::io::{Read as _, Write as _};
use std::os::unix::fs::{symlink, PermissionsExt as _};
use std::os::unix::net::UnixStream;
use std::path::Path;
use std::sync::{Arc, atomic::{AtomicBool, Ordering}};
use std::thread::{self, JoinHandle};
use std::time::Duration;
use layerx_human_identity_provider::{Policy, Server, State};
use layerx_identity_binding::{Client, Config};
use layerx_human_service::store::{AgentTenantId, PrincipalId, PrincipalStore,
    PrincipalTenancyAuthority, RetentionPeriod, RetentionPolicy, RowKey, StoreError, Table,
    TenancyDigest, TenancyMap};

type Result<T = ()> = std::result::Result<T, Box<dyn Error>>;

#[derive(Debug)]
struct Authority(Client);
impl PrincipalTenancyAuthority for Authority {
    fn tenant_for(&self, principal: &PrincipalId) -> std::result::Result<AgentTenantId, StoreError> {
        let binding = self.0.lookup(principal.as_str())?;
        AgentTenantId::new(binding.agent_tenant())
    }
}

struct Running {
    shutdown: Arc<AtomicBool>,
    worker: Option<JoinHandle<std::io::Result<()>>>,
}
impl Running {
    fn start(root: &Path) -> Result<Self> {
        let uid = rustix::process::geteuid().as_raw();
        let state = State::open(&root.join("identity"), Policy {
            root: [0x43; 32], threshold: 1, delay_seconds: 86_400,
        })?;
        let server = Server::bind(&root.join("identity.sock"), state, uid, Duration::from_secs(1), layerx_client::runtime_clock::RuntimeClock::from_environment()?)?
            .with_binding_reader(&root.join("binding.sock"), "human-provider", &[uid])?;
        let shutdown = Arc::new(AtomicBool::new(false));
        let flag = Arc::clone(&shutdown);
        Ok(Self { shutdown, worker: Some(thread::spawn(move || server.run(&flag))) })
    }
    fn stop(mut self) -> Result {
        self.shutdown.store(true, Ordering::Release);
        self.worker.take().ok_or("worker missing")?.join().map_err(|_| "provider panicked")??;
        Ok(())
    }
}
impl Drop for Running {
    fn drop(&mut self) {
        self.shutdown.store(true, Ordering::Release);
        if let Some(worker) = self.worker.take() { let _ = worker.join(); }
    }
}

fn provision(root: &Path, email: &str, idempotency: &str) -> Result<PrincipalId> {
    let mut request = b"LXIP\x01\x01".to_vec();
    let fields = [email.as_bytes(), b"Person", idempotency.as_bytes(), &1_u64.to_be_bytes()];
    request.extend_from_slice(&4_u32.to_be_bytes());
    for field in fields {
        request.extend_from_slice(&u32::try_from(field.len())?.to_be_bytes());
        request.extend_from_slice(field);
    }
    let mut connection = UnixStream::connect(root.join("identity.sock"))?;
    connection.set_read_timeout(Some(Duration::from_secs(1)))?;
    connection.set_write_timeout(Some(Duration::from_secs(1)))?;
    connection.write_all(&u32::try_from(request.len())?.to_be_bytes())?;
    connection.write_all(&request)?;
    let mut size = [0; 4];
    connection.read_exact(&mut size)?;
    let size = u32::from_be_bytes(size) as usize;
    assert!((10..=4096).contains(&size));
    let mut response = vec![0; size];
    connection.read_exact(&mut response)?;
    assert_eq!(&response[..10], b"LXIP\x01\x00\x00\x00\x00\x05");
    let length = u32::from_be_bytes(response[10..14].try_into()?) as usize;
    assert!((1..=128).contains(&length));
    Ok(PrincipalId::new(std::str::from_utf8(&response[14..14 + length])?)?)
}

fn client(root: &Path) -> Result<Client> {
    Ok(Client::new(Config { socket: root.join("binding.sock"), tenant: "human-provider".into(),
        peer_uid: rustix::process::geteuid().as_raw(), peer_gid: rustix::process::getegid().as_raw(),
        deadline: Duration::from_secs(1) })?)
}

fn open(root: &Path, digest: TenancyDigest, client: Client) -> Result<PrincipalStore> {
    let period = RetentionPeriod::new(1_000);
    Ok(PrincipalStore::open_with_authority(root,
        RetentionPolicy { journeys: period, notifications: period, audit: period, telemetry: period, cache: period },
        digest, Arc::new(Authority(client)))?)
}

#[test]
fn actual_provider_binding_preserves_dynamic_store_isolation_and_restart() -> Result {
    let directory = tempfile::Builder::new().permissions(fs::Permissions::from_mode(0o700)).tempdir()?;
    let root = directory.path();
    let running = Running::start(root)?;
    let alice = provision(root, "alice@example.com", "alice")?;
    let bob = provision(root, "bob@example.com", "bob")?;
    assert_eq!(provision(root, "alice@example.com", "alice")?, alice);
    let client = client(root)?;
    let store_root = root.join("store");
    let digest = TenancyMap::new([])?.install(&store_root)?;
    let mut store = open(&store_root, digest, client.clone())?;
    assert!(open(&store_root, digest, client.clone()).is_err());
    let key = RowKey::new("record")?;
    {
        let mut scope = store.principal(&alice)?;
        assert_eq!(scope.tenant().as_str(), client.lookup(alice.as_str())?.agent_tenant());
        scope.put(Table::Journeys, key.clone(), 1, b"alice".to_vec())?;
    }
    {
        let mut scope = store.principal(&bob)?;
        assert!(scope.get(Table::Journeys, &key).is_none());
        scope.put(Table::Journeys, key.clone(), 1, b"bob".to_vec())?;
    }
    let mut expected = vec![alice.clone(), bob.clone()];
    expected.sort();
    assert_eq!(store.known_principals()?, expected);
    assert!(store.principal(&PrincipalId::new("unprovisioned")?).is_err());
    drop(store);
    running.stop()?;
    assert!(open(&store_root, digest, client.clone()).is_err());
    let running = Running::start(root)?;
    let mut store = open(&store_root, digest, client)?;
    assert_eq!(store.principal(&alice)?.get(Table::Journeys, &key).ok_or("alice row")?.bytes(), b"alice");
    assert_eq!(store.principal(&bob)?.get(Table::Journeys, &key).ok_or("bob row")?.bytes(), b"bob");
    running.stop()?;
    assert!(store.principal(&alice).is_err());
    Ok(())
}

#[test]
fn actual_provider_refuses_static_conflicts_and_durable_binding_replacement() -> Result {
    let directory = tempfile::Builder::new().permissions(fs::Permissions::from_mode(0o700)).tempdir()?;
    let root = directory.path();
    let running = Running::start(root)?;
    let principal = provision(root, "owner@example.com", "owner")?;
    let client = client(root)?;
    let static_root = root.join("static-store");
    let digest = TenancyMap::new([(principal.clone(), AgentTenantId::new("wrong-tenant")?)])?.install(&static_root)?;
    assert!(open(&static_root, digest, client.clone()).is_err());
    let store_root = root.join("store");
    let digest = TenancyMap::new([])?.install(&store_root)?;
    let mut store = open(&store_root, digest, client.clone())?;
    store.principal(&principal)?.put(Table::Journeys, RowKey::new("record")?, 1, b"durable".to_vec())?;
    drop(store);
    let binding = store_root.join("principals").join(principal.as_str()).join("provider-binding");
    let original = fs::read(&binding)?;
    fs::write(&binding, b"changed-tenant")?;
    assert!(open(&store_root, digest, client.clone()).is_err());
    fs::write(&binding, original)?;
    let mut store = open(&store_root, digest, client.clone())?;
    assert!(store.principal(&principal)?.get(Table::Journeys, &RowKey::new("record")?).is_some());
    drop(store);
    let missing = root.join("missing");
    fs::remove_file(&binding)?;
    symlink(&missing, &binding)?;
    assert!(open(&store_root, digest, client).is_err());
    assert!(!missing.exists());
    assert!(binding.is_symlink());
    running.stop()?;
    Ok(())
}
