//! Drives the treasury socket signer client against the real
//! `platform/hosted/node/signer/signer.py` and proves the core's signing path
//! produces the bytes a seed holder produces, without the seed.

use ed25519_dalek::SigningKey;
use layerx_platform_core::{
    asset_registry, build_send_with_identity_sequence, build_send_with_signer, did_for_public_key,
    SeedSigner, SendError, SendRequest, SocketSigner, TreasurySigner as _,
};
use std::fmt::Debug;
use std::fs;
use std::io::Read;
use std::os::unix::fs::PermissionsExt;
use std::os::unix::net::UnixStream;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicU64, Ordering};
use std::thread;
use std::time::{Duration, Instant};

static NEXT_SIGNER: AtomicU64 = AtomicU64::new(0);

fn must<T, E: Debug>(result: Result<T, E>, what: &str) -> T {
    result.unwrap_or_else(|error| panic!("{what}: {error:?}"))
}

fn random32() -> [u8; 32] {
    let mut bytes = [0_u8; 32];
    must(
        fs::File::open("/dev/urandom").and_then(|mut file| file.read_exact(&mut bytes)),
        "urandom",
    );
    bytes
}

fn effective_uid() -> u32 {
    let status = must(fs::read_to_string("/proc/self/status"), "process status");
    status
        .lines()
        .find_map(|line| line.strip_prefix("Uid:"))
        .and_then(|line| line.split_whitespace().nth(1))
        .and_then(|value| value.parse::<u32>().ok())
        .unwrap_or_else(|| panic!("effective uid is not readable"))
}

fn repository_root() -> PathBuf {
    let manifest = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    manifest.ancestors().nth(3).map_or_else(
        || panic!("repository root above {}", manifest.display()),
        Path::to_path_buf,
    )
}

struct Signer {
    child: Child,
    root: PathBuf,
    socket: PathBuf,
    stderr: PathBuf,
}

impl Signer {
    fn start(seed: &[u8; 32]) -> Self {
        let root = std::env::temp_dir().join(format!(
            "layerx-core-signer-{}-{}",
            std::process::id(),
            NEXT_SIGNER.fetch_add(1, Ordering::Relaxed)
        ));
        must(fs::create_dir_all(&root), "signer root");
        must(
            fs::set_permissions(&root, fs::Permissions::from_mode(0o700)),
            "signer root mode",
        );
        let key = root.join("treasury.key");
        must(fs::write(&key, seed), "treasury seed");
        must(
            fs::set_permissions(&key, fs::Permissions::from_mode(0o600)),
            "treasury seed mode",
        );
        let socket = root.join("treasury-signer.sock");
        let stderr = root.join("signer.stderr");
        let program = repository_root().join("platform/hosted/node/signer/signer.py");
        assert!(program.is_file(), "{} is missing", program.display());
        let child = must(
            Command::new("python3")
                .arg(&program)
                .arg("--socket")
                .arg(&socket)
                .arg("--allowed-uid")
                .arg(effective_uid().to_string())
                .arg("--provider")
                .arg("file")
                .arg("--key-file")
                .arg(&key)
                .stdin(Stdio::null())
                .stdout(Stdio::null())
                .stderr(Stdio::from(must(
                    fs::File::create(&stderr),
                    "signer stderr",
                )))
                .spawn(),
            "spawn signer.py",
        );
        let mut signer = Self {
            child,
            root,
            socket,
            stderr,
        };
        let deadline = Instant::now() + Duration::from_secs(30);
        while UnixStream::connect(&signer.socket).is_err() {
            if let Ok(Some(status)) = signer.child.try_wait() {
                panic!(
                    "treasury signer exited early with {status}: {}",
                    signer.diagnostics()
                );
            }
            assert!(
                Instant::now() < deadline,
                "treasury signer socket did not appear: {}",
                signer.diagnostics()
            );
            thread::sleep(Duration::from_millis(50));
        }
        signer
    }

    fn diagnostics(&self) -> String {
        fs::read_to_string(&self.stderr).unwrap_or_default()
    }

    fn stop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

impl Drop for Signer {
    fn drop(&mut self) {
        self.stop();
        if let Err(error) = fs::remove_dir_all(&self.root) {
            if !thread::panicking() {
                panic!("failed to remove {}: {error}", self.root.display());
            }
        }
    }
}

fn request(source_did: &str) -> SendRequest {
    SendRequest {
        network_id: 42,
        source_did: source_did.to_owned(),
        destination_did: did_for_public_key(&[7; 32]),
        asset: [2; 32],
        amount: 25,
        account_sequence: 3,
        idempotency_key: [3; 32],
        not_before_ms: 1_000,
        expires_at_ms: 2_000,
        fee_limit: 1_000,
    }
}

#[test]
fn socket_signer_signs_the_treasury_send_the_seed_holder_signs() {
    let seed = random32();
    let seed_signer = SeedSigner::new(&seed);
    let expected_key = SigningKey::from_bytes(&seed).verifying_key().to_bytes();
    assert_eq!(seed_signer.public_key(), expected_key);
    let signer_process = Signer::start(&seed);
    let socket = must(
        SocketSigner::connect(&signer_process.socket),
        "connect to the signer",
    );
    assert_eq!(socket.public_key(), expected_key);
    assert_eq!(socket.path(), signer_process.socket.as_path());

    let digest = random32();
    let signature = must(socket.sign_digest(&digest), "signature over the socket");
    must(
        layerx_crypto::ed25519::verify_digest(&expected_key, &signature, &digest),
        "socket signature verifies",
    );
    assert_eq!(
        signature,
        must(seed_signer.sign_digest(&digest), "seed signature"),
        "Ed25519 is deterministic, so both custody paths agree byte for byte"
    );

    let treasury_did = did_for_public_key(&expected_key);
    let request = request(&treasury_did);
    let over_socket = must(
        build_send_with_signer(&socket, 11, &request),
        "send signed over the socket",
    );
    let in_process = must(
        build_send_with_identity_sequence(&seed, 11, &request),
        "send signed in process",
    );
    assert_eq!(over_socket, in_process);
    assert_eq!(over_socket.signer_public_key, expected_key);
    let (registry, _) = must(asset_registry(), "asset registry");
    let activity = must(
        layerx_wire::activity::decode_signed(&over_socket.canonical, &registry),
        "decode the socket-signed send",
    );
    assert_eq!(activity.account_sequence(), 11);
    assert_eq!(activity.actor_did(), treasury_did.as_bytes());
}

#[test]
fn socket_signer_refuses_absent_or_relative_sockets_and_reports_outages() {
    let seed = random32();
    let mut signer_process = Signer::start(&seed);
    let absent = signer_process.root.join("absent.sock");
    let refused = SocketSigner::connect(&absent);
    assert!(
        refused
            .as_ref()
            .is_err_and(|error| error.contains("is not available")),
        "{refused:?}"
    );
    let relative = SocketSigner::connect(Path::new("treasury-signer.sock"));
    assert!(
        relative
            .as_ref()
            .is_err_and(|error| error.contains("is not an absolute path")),
        "{relative:?}"
    );

    let socket = must(
        SocketSigner::connect(&signer_process.socket),
        "connect to the signer",
    );
    let request = request(&did_for_public_key(&socket.public_key()));
    must(
        build_send_with_signer(&socket, 1, &request),
        "send while the signer is up",
    );
    signer_process.stop();
    let outage = build_send_with_signer(&socket, 2, &request);
    assert!(
        matches!(&outage, Err(SendError::Signer(reason)) if reason.contains("is not available")),
        "{outage:?}"
    );
    let invalid = build_send_with_signer(
        &socket,
        3,
        &SendRequest {
            amount: 0,
            ..request
        },
    );
    assert_eq!(
        invalid,
        Err(SendError::Invalid(
            "amount must be greater than zero".to_owned()
        )),
        "request validation never reaches the signer"
    );
}
