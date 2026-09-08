mod support;

use data_encoding::BASE32_NOPAD;
use hmac::{Hmac, Mac as _};
use layerx_human_security_provider::{serve, Config, Store};
use layerx_human_service::security::{
    AuthenticatorProvider, RecoveryEvidenceProvider, SecurityBoundaryError,
};
use layerx_human_service::server::production_auth::{
    RemoteSecurityProvider, SecurityProviderConfig,
};
use layerx_human_service::store::PrincipalId;
use sha1::Sha1;
use std::fs::{self, OpenOptions};
use std::io::{Read as _, Write as _};
use std::os::unix::fs::{
    DirBuilderExt as _, MetadataExt as _, OpenOptionsExt as _, PermissionsExt as _,
};
use std::os::unix::net::UnixStream;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

static NEXT: AtomicU64 = AtomicU64::new(0);
struct Fixture {
    root: PathBuf,
}
impl Fixture {
    fn new() -> Self {
        let root = std::env::temp_dir().join(format!(
            "lxsp-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::DirBuilder::new().mode(0o700).create(&root).unwrap();
        let (history, receipt) = support::evidence();
        private(&root.join("trust"), &history);
        private(
            &root.join("receipt"),
            &serde_json::to_vec(&receipt).unwrap(),
        );
        Self { root }
    }
    fn state(&self) -> PathBuf {
        self.root.join("state")
    }
    fn trust(&self) -> PathBuf {
        self.root.join("trust")
    }
    fn socket(&self) -> PathBuf {
        self.root.join("socket")
    }
    fn client(&self) -> RemoteSecurityProvider {
        RemoteSecurityProvider::new(SecurityProviderConfig {
            socket: self.socket(),
            deadline: Duration::from_secs(3),
            maximum_frame_bytes: 1_048_576,
        })
        .unwrap()
    }
    fn start(&self, uid: u32) -> Running {
        let stop = Arc::new(AtomicBool::new(false));
        let config = Config {
            state_root: self.state(),
            trust_history: self.trust(),
            socket: self.socket(),
            allowed_uid: uid,
            deadline: Duration::from_secs(1),
        };
        let shutdown = stop.clone();
        let thread = std::thread::spawn(move || serve(config, shutdown));
        let deadline = Instant::now() + Duration::from_secs(5);
        while !self.socket().exists() {
            assert!(!thread.is_finished(), "server exited before binding");
            assert!(Instant::now() < deadline);
            std::thread::sleep(Duration::from_millis(10));
        }
        Running {
            stop,
            thread: Some(thread),
        }
    }
    fn ingest(&self) -> std::process::Output {
        Command::new(env!("CARGO_BIN_EXE_layerx-human-security-provider"))
            .arg("ingest-recovery-receipt")
            .arg(self.root.join("receipt"))
            .env("LAYERX_HUMAN_SECURITY_PROVIDER_STATE_ROOT", self.state())
            .env("LAYERX_HUMAN_SECURITY_PROVIDER_TRUST_HISTORY", self.trust())
            .output()
            .unwrap()
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        fs::remove_dir_all(&self.root).unwrap();
    }
}
struct Running {
    stop: Arc<AtomicBool>,
    thread: Option<std::thread::JoinHandle<layerx_human_security_provider::Result<()>>>,
}
impl Drop for Running {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        self.thread.take().unwrap().join().unwrap().unwrap();
    }
}
fn private(path: &Path, bytes: &[u8]) {
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(path)
        .unwrap();
    file.write_all(bytes).unwrap();
    file.sync_all().unwrap();
}
fn principal() -> PrincipalId {
    PrincipalId::new("alice").unwrap()
}
fn uid() -> u32 {
    rustix::process::geteuid().as_raw()
}
fn code(secret: &str, now: u64) -> String {
    let secret = BASE32_NOPAD.decode(secret.as_bytes()).unwrap();
    let mut mac = Hmac::<Sha1>::new_from_slice(&secret).unwrap();
    mac.update(&(now / 30).to_be_bytes());
    let hash = mac.finalize().into_bytes();
    let offset = usize::from(hash[19] & 15);
    let number = u32::from_be_bytes(hash[offset..offset + 4].try_into().unwrap()) & 0x7fff_ffff;
    format!("{:06}", number % 1_000_000)
}

#[test]
fn all_authenticator_operations_expiry_equality_and_restart() {
    let fixture = Fixture::new();
    let server = fixture.start(uid());
    let mut client = fixture.client();
    let p = principal();
    client.probe().unwrap();
    assert!(client.status(&p).unwrap().methods.is_empty());
    assert!(client.rotate_backup_codes(&p, 1000).is_err());
    let setup = client.begin_setup(&p, "Phone", 1000).unwrap();
    assert_eq!(setup.expires_at, 1300);
    assert_eq!(setup.secret.remask_at(), 1060);
    assert!(setup.secret.copyable());
    assert!(!setup.otpauth_uri.copyable());
    assert!(setup
        .otpauth_uri
        .expose()
        .contains("algorithm=SHA1&digits=6&period=30"));
    let setup_code = code(setup.secret.expose(), 1300);
    drop(server);
    let server = fixture.start(uid());
    let enabled = client
        .finish_setup(&p, &setup.setup_id, &setup_code, 1300)
        .unwrap();
    assert_eq!(enabled.method.label, "Phone");
    assert_eq!(enabled.method.enabled_at, 1300);
    assert_eq!(enabled.method.last_used_at, None);
    assert_eq!(enabled.backup_codes.expose().len(), 10);
    assert_eq!(enabled.backup_codes.remask_at(), 1360);
    assert_eq!(
        client
            .finish_setup(&p, &setup.setup_id, &setup_code, 1300)
            .unwrap_err(),
        SecurityBoundaryError::Refused
    );
    let rotated = client.rotate_backup_codes(&p, 1300).unwrap();
    assert_ne!(rotated.expose(), enabled.backup_codes.expose());
    drop(server);
    let _server = fixture.start(uid());
    let status = client.status(&p).unwrap();
    assert_eq!(status.methods, vec![enabled.method.clone()]);
    assert_eq!(status.backup_codes_remaining, 10);
    assert!(client.disable(&p, &enabled.method.id, 1299).is_err());
    let status = client.disable(&p, &enabled.method.id, 1300).unwrap();
    assert_eq!(status.backup_codes_remaining, 0);
    assert!(status.methods.is_empty());
    let expired = client.begin_setup(&p, "Tablet", 2000).unwrap();
    assert_eq!(
        client
            .finish_setup(
                &p,
                &expired.setup_id,
                &code(expired.secret.expose(), 2301),
                2301
            )
            .unwrap_err(),
        SecurityBoundaryError::Refused
    );
    assert!(client.begin_setup(&p, "Overflow", u64::MAX).is_err());
    assert_eq!(fs::metadata(fixture.state()).unwrap().mode() & 0o777, 0o700);
    assert_eq!(
        fs::metadata(fixture.socket()).unwrap().mode() & 0o777,
        0o660
    );
    for entry in fs::read_dir(fixture.state()).unwrap() {
        assert_eq!(entry.unwrap().metadata().unwrap().mode() & 0o777, 0o600);
    }
}

#[test]
fn attempts_and_principal_binding_survive_restart() {
    let fixture = Fixture::new();
    let server = fixture.start(uid());
    let mut client = fixture.client();
    let setup = client.begin_setup(&principal(), "Phone", 1000).unwrap();
    assert!(client
        .finish_setup(
            &PrincipalId::new("bob").unwrap(),
            &setup.setup_id,
            &code(setup.secret.expose(), 1000),
            1000
        )
        .is_err());
    for _ in 0..4 {
        assert!(client
            .finish_setup(&principal(), &setup.setup_id, "invalid", 1000)
            .is_err());
    }
    drop(server);
    let server = fixture.start(uid());
    assert!(client
        .finish_setup(&principal(), &setup.setup_id, "invalid", 1000)
        .is_err());
    drop(server);
    let _server = fixture.start(uid());
    assert!(client
        .finish_setup(
            &principal(),
            &setup.setup_id,
            &code(setup.secret.expose(), 1000),
            1000
        )
        .is_err());
    let replacement = client.begin_setup(&principal(), "New", 1001).unwrap();
    assert!(client
        .finish_setup(
            &principal(),
            &setup.setup_id,
            &code(setup.secret.expose(), 1001),
            1001
        )
        .is_err());
    client
        .finish_setup(
            &principal(),
            &replacement.setup_id,
            &code(replacement.secret.expose(), 1001),
            1001,
        )
        .unwrap();
}

#[test]
fn verified_admin_ingest_reveal_and_tamper_refusal() {
    let fixture = Fixture::new();
    assert!(fixture.ingest().status.success());
    assert!(fixture.ingest().status.success());
    let server = fixture.start(uid());
    let client = fixture.client();
    let receipt = client
        .reveal_verified_receipt(&principal(), "recovery-1", 100)
        .unwrap();
    assert_eq!(receipt.expose(), support::evidence().1.canonical_receipt);
    assert_eq!(receipt.remask_at(), 160);
    assert!(client
        .reveal_verified_receipt(&PrincipalId::new("bob").unwrap(), "recovery-1", 100)
        .is_err());
    assert!(client
        .reveal_verified_receipt(&principal(), "absent", 100)
        .is_err());
    assert!(!fixture.ingest().status.success());
    drop(server);
    let server = fixture.start(uid());
    assert_eq!(
        client
            .reveal_verified_receipt(&principal(), "recovery-1", 101)
            .unwrap()
            .expose(),
        receipt.expose()
    );
    drop(server);
    let mut bad = support::evidence().1;
    let replacement = if bad.header_signature.starts_with('A') {
        "B"
    } else {
        "A"
    };
    bad.header_signature.replace_range(..1, replacement);
    fs::write(
        fixture.root.join("receipt"),
        serde_json::to_vec(&bad).unwrap(),
    )
    .unwrap();
    assert!(!fixture.ingest().status.success());
    fs::set_permissions(
        fixture.root.join("receipt"),
        fs::Permissions::from_mode(0o644),
    )
    .unwrap();
    assert!(!fixture.ingest().status.success());
}

#[test]
fn wrong_uid_and_malformed_frames_are_refused() {
    let fixture = Fixture::new();
    let server = fixture.start(uid().checked_add(1).unwrap());
    assert_eq!(
        fixture.client().probe(),
        Err(SecurityBoundaryError::Refused)
    );
    drop(server);
    let _server = fixture.start(uid());
    let payloads: Vec<Vec<u8>> = vec![
        b"WRNG\x01\x00\x00\x00\x00\x00".to_vec(),
        b"LXSP\x02\x00\x00\x00\x00\x00".to_vec(),
        b"LXSP\x01\xff\x00\x00\x00\x00".to_vec(),
        b"LXSP\x01\x00\xff\xff\xff\xff".to_vec(),
        b"LXSP\x01\x00\x00\x00\x00\x00x".to_vec(),
        b"LXSP\x01\x01\x00\x00\x00\x01\xff\xff\xff\xff".to_vec(),
        b"LXSP\x01\x01\x00\x00\x00\x01\x00\x00\x00\x01\xff".to_vec(),
    ];
    for payload in payloads {
        let mut stream = UnixStream::connect(fixture.socket()).unwrap();
        stream
            .set_read_timeout(Some(Duration::from_secs(2)))
            .unwrap();
        stream
            .write_all(&(payload.len() as u32).to_be_bytes())
            .unwrap();
        stream.write_all(&payload).unwrap();
        let mut response = [0; 14];
        stream.read_exact(&mut response).unwrap();
        assert_eq!(&response, b"\x00\x00\x00\x0aLXSP\x01\x01\x00\x00\x00\x00");
    }
    for length in [0u32, 1_048_577, u32::MAX] {
        let mut stream = UnixStream::connect(fixture.socket()).unwrap();
        stream.write_all(&length.to_be_bytes()).unwrap();
        let mut response = [0; 14];
        stream.read_exact(&mut response).unwrap();
        assert_eq!(response[9], 1);
    }
    fixture.client().probe().unwrap();
}

#[test]
fn slow_partial_frame_has_total_deadline() {
    let fixture = Fixture::new();
    let _server = fixture.start(uid());
    let mut stream = UnixStream::connect(fixture.socket()).unwrap();
    stream
        .set_read_timeout(Some(Duration::from_secs(2)))
        .unwrap();
    stream.write_all(&10u32.to_be_bytes()).unwrap();
    let start = Instant::now();
    for byte in b"LXSP" {
        if stream.write_all(&[*byte]).is_err() {
            break;
        }
        std::thread::sleep(Duration::from_millis(350));
    }
    let mut response = [0; 1];
    assert!(matches!(stream.read(&mut response), Ok(0) | Err(_)));
    assert!(start.elapsed() < Duration::from_secs(2));
    fixture.client().probe().unwrap();
}

#[test]
fn corruption_withdraws_readiness_and_refuses_replay() {
    for target in [
        "initialized",
        "snapshot.json",
        "head",
        "00000000000000000001.json",
    ] {
        let fixture = Fixture::new();
        let server = fixture.start(uid());
        fixture
            .client()
            .begin_setup(&principal(), "Phone", 100)
            .unwrap();
        let path = fixture.state().join(target);
        let original = fs::read(&path).unwrap();
        fs::write(&path, b"corrupt").unwrap();
        assert!(fixture.client().probe().is_err());
        fs::write(&path, &original).unwrap();
        assert!(fixture.client().probe().is_err());
        drop(server);
        fs::write(&path, b"corrupt").unwrap();
        assert!(Store::open(&fixture.state(), &fixture.trust()).is_err());
    }
}

#[test]
fn missing_tail_incomplete_initialization_symlinks_and_writer_lock_refuse() {
    let fixture = Fixture::new();
    let server = fixture.start(uid());
    fixture
        .client()
        .begin_setup(&principal(), "Phone", 100)
        .unwrap();
    assert!(Store::open(&fixture.state(), &fixture.trust()).is_err());
    drop(server);
    fs::remove_file(fixture.state().join("00000000000000000001.json")).unwrap();
    assert!(Store::open(&fixture.state(), &fixture.trust()).is_err());
    let partial = Fixture::new();
    fs::DirBuilder::new()
        .mode(0o700)
        .create(partial.state())
        .unwrap();
    private(&partial.state().join("transaction.tmp"), b"partial");
    assert!(Store::open(&partial.state(), &partial.trust()).is_err());
    let linked = Fixture::new();
    std::os::unix::fs::symlink(fixture.state(), linked.state()).unwrap();
    assert!(Store::open(&linked.state(), &linked.trust()).is_err());
    let bad_mode = Fixture::new();
    fs::DirBuilder::new()
        .mode(0o755)
        .create(bad_mode.state())
        .unwrap();
    assert!(Store::open(&bad_mode.state(), &bad_mode.trust()).is_err());
}

#[test]
fn binary_configuration_and_signal_shutdown() {
    let fixture = Fixture::new();
    let binary = env!("CARGO_BIN_EXE_layerx-human-security-provider");
    let configured = || {
        let mut command = Command::new(binary);
        command
            .env("LAYERX_HUMAN_SECURITY_PROVIDER_STATE_ROOT", fixture.state())
            .env(
                "LAYERX_HUMAN_SECURITY_PROVIDER_TRUST_HISTORY",
                fixture.trust(),
            )
            .env("LAYERX_HUMAN_SECURITY_PROVIDER_SOCKET", fixture.socket())
            .env(
                "LAYERX_HUMAN_SECURITY_PROVIDER_ALLOWED_UID",
                uid().to_string(),
            );
        command
    };
    assert!(!configured()
        .env_remove("LAYERX_HUMAN_SECURITY_PROVIDER_ALLOWED_UID")
        .output()
        .unwrap()
        .status
        .success());
    for seconds in ["0", "61", "invalid"] {
        assert!(!configured()
            .env("LAYERX_HUMAN_SECURITY_PROVIDER_DEADLINE_SECONDS", seconds)
            .output()
            .unwrap()
            .status
            .success());
    }
    for signal in [rustix::process::Signal::TERM, rustix::process::Signal::INT] {
        let mut child = configured().spawn().unwrap();
        let deadline = Instant::now() + Duration::from_secs(5);
        while fixture.client().probe().is_err() {
            assert!(child.try_wait().unwrap().is_none());
            assert!(Instant::now() < deadline);
            std::thread::sleep(Duration::from_millis(10));
        }
        rustix::process::kill_process(
            rustix::process::Pid::from_raw(child.id() as i32).unwrap(),
            signal,
        )
        .unwrap();
        while child.try_wait().unwrap().is_none() {
            assert!(Instant::now() < deadline);
            std::thread::sleep(Duration::from_millis(10));
        }
        assert!(child.wait().unwrap().success());
        assert!(!fixture.socket().exists());
    }
}

#[test]
fn stored_recovery_is_reverified_against_protected_trust() {
    let fixture = Fixture::new();
    assert!(fixture.ingest().status.success());
    let server = fixture.start(uid());
    fixture.client().probe().unwrap();
    let mut history = fs::read(fixture.trust()).unwrap();
    let public_offset = b"LayerX/sequencer-trust-history/v1\0".len() + 4 + 2 + 4 + 8 + 32;
    history[public_offset] ^= 1;
    fs::write(fixture.trust(), history).unwrap();
    assert!(fixture
        .client()
        .reveal_verified_receipt(&principal(), "recovery-1", 100)
        .is_err());
    assert!(fixture.client().probe().is_err());
    drop(server);
    assert!(Store::open(&fixture.state(), &fixture.trust()).is_err());
}

#[test]
fn protected_state_files_reject_symlinks_hardlinks_and_public_modes() {
    for mode in 0..3 {
        let fixture = Fixture::new();
        drop(Store::open(&fixture.state(), &fixture.trust()).unwrap());
        let snapshot = fixture.state().join("snapshot.json");
        match mode {
            0 => {
                let original = fixture.root.join("original");
                fs::rename(&snapshot, &original).unwrap();
                std::os::unix::fs::symlink(original, &snapshot).unwrap();
            }
            1 => fs::hard_link(&snapshot, fixture.root.join("alias")).unwrap(),
            _ => fs::set_permissions(&snapshot, fs::Permissions::from_mode(0o644)).unwrap(),
        }
        assert!(Store::open(&fixture.state(), &fixture.trust()).is_err());
    }
}
