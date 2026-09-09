use ed25519_dalek::{Signer as _, SigningKey};
use layerx_client::lni::handshake::{perform, HandshakeConfig};
use layerx_client::lni::preparation::{preparation_state, PreparationStateContext};
use layerx_client::lni::schema::Version;
use layerx_client::lni::transport::{ConnectionGate, Limits, Uds};
use layerx_programs::hex::encode as hex_encode;
use layerx_types::activity::{Authority, EnvelopeBuilder, Signature, TimestampBound};
use layerx_types::amount::Amount;
use layerx_types::ids::{Did, IdempotencyKey};
use layerx_types::payload::{ActivityType, ModuleId, ModuleRegistration, ModuleRegistry, Payload};
use sha2::{Digest as _, Sha256};
use std::collections::BTreeMap;
use std::fmt::Debug;
use std::fs;
use std::io::Read as _;
use std::net::{TcpListener, TcpStream};
use std::os::unix::fs::PermissionsExt as _;
use std::os::unix::process::CommandExt as _;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicU64, Ordering};
use std::thread;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

static NEXT_CLUSTER: AtomicU64 = AtomicU64::new(0);
const NETWORK_ID: u32 = 7332;
const PROTOCOL_VERSION: u16 = 3;
const LAST_BATCH: u64 = u64::MAX;
const LNI_FRAME_BYTES: usize = 1_212_416;
const LOG_BYTES: u64 = 64 * 1024 * 1024;
const MODULE_GOVERNANCE: u16 = 7;
const DAEMON_UID: u32 = 65534;
const DAEMON_GID: u32 = 0;

fn treasury_did(seed: &[u8; 32]) -> String {
    format!(
        "did:layerx:{}",
        hex_encode(&SigningKey::from_bytes(seed).verifying_key().to_bytes())
    )
}
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

fn now_ms() -> u64 {
    u64::try_from(must(SystemTime::now().duration_since(UNIX_EPOCH), "clock").as_millis())
        .unwrap_or(u64::MAX)
}

fn repository_root() -> PathBuf {
    let manifest = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    manifest.ancestors().nth(3).map_or_else(
        || panic!("repository root above {}", manifest.display()),
        Path::to_path_buf,
    )
}

fn free_port() -> u16 {
    let listener = must(TcpListener::bind("127.0.0.1:0"), "ephemeral port");
    must(listener.local_addr(), "listener address").port()
}

fn write(path: &Path, bytes: &[u8], mode: u32) {
    must(fs::write(path, bytes), &format!("write {}", path.display()));
    must(
        fs::set_permissions(path, fs::Permissions::from_mode(mode)),
        &format!("chmod {}", path.display()),
    );
}

fn preallocate_log(path: &Path) {
    let file = must(
        fs::File::create(path),
        &format!("create {}", path.display()),
    );
    must(file.set_len(LOG_BYTES), &format!("size {}", path.display()));
    must(
        fs::set_permissions(path, fs::Permissions::from_mode(0o600)),
        &format!("chmod {}", path.display()),
    );
}

fn make_dir(path: &Path, mode: u32) {
    must(
        fs::create_dir_all(path),
        &format!("mkdir {}", path.display()),
    );
    must(
        fs::set_permissions(path, fs::Permissions::from_mode(mode)),
        &format!("chmod {}", path.display()),
    );
}

fn chown_tree(path: &Path, uid: u32, gid: u32) {
    must(
        std::os::unix::fs::chown(path, Some(uid), Some(gid)),
        &format!("chown {}", path.display()),
    );
    if path.is_dir() {
        for entry in must(fs::read_dir(path), &format!("list {}", path.display())) {
            chown_tree(&must(entry, "directory entry").path(), uid, gid);
        }
    }
}

fn command(program: &str, arguments: &[&str]) {
    let output = must(
        Command::new(program).args(arguments).output(),
        &format!("run {program}"),
    );
    assert!(
        output.status.success(),
        "{program} {arguments:?} failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
}

fn text(path: &Path) -> String {
    path.to_string_lossy().into_owned()
}

fn token() -> String {
    hex_encode(&random32())
}

fn sha256(parts: &[&[u8]]) -> [u8; 32] {
    let mut digest = Sha256::new();
    for part in parts {
        digest.update(part);
    }
    digest.finalize().into()
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

struct Daemon {
    child: Child,
    supervised: bool,
    stderr: PathBuf,
}

impl Daemon {
    fn stop(&mut self) {
        if self.supervised && matches!(self.child.try_wait(), Ok(None)) {
            let group = format!("-{}", self.child.id());
            let _ = Command::new("kill").args(["-TERM", "--", &group]).status();
            let deadline = Instant::now() + Duration::from_secs(15);
            while matches!(self.child.try_wait(), Ok(None)) && Instant::now() < deadline {
                thread::sleep(Duration::from_millis(50));
            }
            let _ = Command::new("kill")
                .args(["-KILL", "--", &group])
                .stderr(Stdio::null())
                .status();
        }
        let _ = self.child.kill();
        let _ = self.child.wait();
    }

    fn diagnostics(&self) -> String {
        fs::read_to_string(&self.stderr).unwrap_or_default()
    }
}

impl Drop for Daemon {
    fn drop(&mut self) {
        self.stop();
    }
}

fn spawn(
    program: &Path,
    arguments: &[&str],
    environment: &BTreeMap<&str, String>,
    daemon_identity: bool,
    stderr: PathBuf,
) -> Daemon {
    let mut command = Command::new(program);
    command
        .args(arguments)
        .env_clear()
        .env("PATH", "/usr/bin:/bin")
        .envs(environment.iter().map(|(key, value)| (key, value.as_str())))
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::from(must(fs::File::create(&stderr), "stderr file")));
    if daemon_identity {
        command.uid(DAEMON_UID).gid(DAEMON_GID);
    }
    let mut dump = String::new();
    for (key, value) in environment {
        dump.push_str(key);
        dump.push('=');
        dump.push_str(value);
        dump.push('\n');
    }
    write(&stderr.with_extension("env"), dump.as_bytes(), 0o600);
    let supervised = program
        .file_name()
        .is_some_and(|name| name == "supervisor.sh");
    if supervised {
        command.process_group(0);
    }
    let child = must(command.spawn(), &format!("spawn {}", program.display()));
    Daemon {
        child,
        supervised,
        stderr,
    }
}

fn wait_for_port(port: u16, daemon: &mut Daemon, what: &str) {
    let deadline = Instant::now() + Duration::from_secs(30);
    loop {
        if TcpStream::connect(("127.0.0.1", port)).is_ok() {
            return;
        }
        if let Ok(Some(status)) = daemon.child.try_wait() {
            panic!(
                "{what} exited early with {status}: {}",
                daemon.diagnostics()
            );
        }
        assert!(
            Instant::now() < deadline,
            "{what} did not open port {port}: {}",
            daemon.diagnostics()
        );
        thread::sleep(Duration::from_millis(50));
    }
}

fn lni_limits() -> Limits {
    Limits {
        maximum_frame_bytes: LNI_FRAME_BYTES,
        maximum_connections: 4,
        maximum_streams: 1,
        maximum_queued_bytes: 4 * 1024 * 1024,
        deadline: Duration::from_secs(5),
    }
}

fn handshake_config() -> HandshakeConfig {
    HandshakeConfig {
        built_interface_version: Version::V1_4,
        expected_protocol_version: PROTOCOL_VERSION,
        expected_network_id: NETWORK_ID,
    }
}

fn account_sequence(socket: &Path, did: &str) -> u64 {
    let gate = ConnectionGate::new(1);
    let mut transport = must(Uds::connect(socket, &gate, lni_limits()), "LNI connect");
    let handshake = must(
        perform(&mut transport, &handshake_config(), None),
        "LNI handshake",
    );
    let actor = must(Did::new(did.as_bytes()), "treasury DID");
    let snapshot = must(
        preparation_state(
            &mut transport,
            &actor,
            PreparationStateContext {
                interface_version: handshake.node().interface_version,
                expected_network_id: NETWORK_ID,
                minimum_observed_head: 0,
                correlation_id: 3,
            },
        ),
        "treasury preparation state",
    );
    snapshot.account_sequence
}

fn signed_program_activity(
    seed: &[u8; 32],
    did: &str,
    sequence: u64,
    ordinal: u16,
    bytes: &[u8],
) -> Vec<u8> {
    let signing_key = SigningKey::from_bytes(seed);
    let public_key = signing_key.verifying_key().to_bytes();
    let activity_type = must(
        ActivityType::new(ModuleId::Programs, ordinal),
        "Programs type",
    );
    let registration = must(
        ModuleRegistration::new(ModuleId::Programs, &[activity_type]),
        "Programs registration",
    );
    let registry = must(ModuleRegistry::new(&[registration]), "Programs registry");
    let payload = must(
        Payload::new(&registry, activity_type, bytes),
        "call payload",
    );
    let payload_hash = must(
        layerx_wire::hash::payload_hash_for(&payload),
        "payload hash",
    );
    let mut builder = EnvelopeBuilder::new();
    must(
        builder
            .protocol_version(PROTOCOL_VERSION)
            .and_then(|value| value.network_id(NETWORK_ID))
            .and_then(|value| value.activity_type(activity_type))
            .and_then(|value| value.actor_did(must(Did::new(did.as_bytes()), "actor DID")))
            .and_then(|value| value.authority(must(Authority::owner(&public_key), "owner")))
            .and_then(|value| value.account_sequence(sequence))
            .and_then(|value| {
                value.timestamp_bound(must(
                    TimestampBound::new(now_ms() - 30_000, now_ms() + 120_000),
                    "validity",
                ))
            })
            .and_then(|value| value.idempotency_key(IdempotencyKey::new(random32())))
            .and_then(|value| value.fee_limit(Amount::from_u128(0)))
            .and_then(|value| value.payload_hash(payload_hash))
            .and_then(|value| value.payload(payload))
            .map(|_| ()),
        "program envelope",
    );
    let unsigned = must(builder.build(), "program envelope build");
    let digest = must(
        layerx_wire::sign::preimage_unsigned(&unsigned),
        "signing preimage",
    );
    let signature = signing_key.sign(digest.as_bytes()).to_bytes();
    must(
        layerx_wire::activity::encode_signed_envelope(
            &unsigned.attach_signature(must(Signature::new(&signature), "signature")),
        ),
        "signed ProgramCall",
    )
}

fn chain_head(socket: &Path) -> Option<(u64, [u8; 32])> {
    let gate = ConnectionGate::new(1);
    let mut transport = Uds::connect(socket, &gate, lni_limits()).ok()?;
    let handshake = perform(&mut transport, &handshake_config(), None).ok()?;
    Some((
        handshake.node().chain_head_sequence,
        handshake.node().authorised_sequencer_key,
    ))
}

fn wait_for_lni(socket: &Path, daemon: &mut Daemon) {
    let deadline = Instant::now() + Duration::from_secs(60);
    loop {
        if socket.exists() && chain_head(socket).is_some() {
            return;
        }
        if let Ok(Some(status)) = daemon.child.try_wait() {
            panic!(
                "layerxd --serve exited early with {status}: {}",
                daemon.diagnostics()
            );
        }
        assert!(
            Instant::now() < deadline,
            "sequencer LNI did not come up: {}",
            daemon.diagnostics()
        );
        thread::sleep(Duration::from_millis(100));
    }
}

struct Genesis {
    directory: PathBuf,
    asset: [u8; 32],
    receipt_state_root: [u8; 32],
}

fn genesis_request(asset: &[u8; 32], sequencer_key: &[u8; 32]) -> Vec<u8> {
    let mut request = Vec::with_capacity(512);
    request.extend_from_slice(b"LXGB");
    request.push(1);
    request.extend_from_slice(&PROTOCOL_VERSION.to_be_bytes());
    request.extend_from_slice(&NETWORK_ID.to_be_bytes());
    request.extend_from_slice(&now_ms().to_be_bytes());
    request.extend_from_slice(&1_u16.to_be_bytes());
    request.extend_from_slice(&MODULE_GOVERNANCE.to_be_bytes());
    let mut parameter_key = [0_u8; 32];
    parameter_key[..17].copy_from_slice(b"parameter-version");
    request.extend_from_slice(&parameter_key);
    let mut parameter_value = [0_u8; 32];
    parameter_value[31] = 1;
    request.extend_from_slice(&parameter_value);
    request.extend_from_slice(&1_u16.to_be_bytes());
    request.extend_from_slice(&sha256(&[
        b"layerx-beta-guarantor:",
        hex_encode(sequencer_key).as_bytes(),
    ]));
    request.push(2);
    request.extend_from_slice(sequencer_key);
    request.extend_from_slice(&[0_u8; 16]);
    request.extend_from_slice(asset);
    request.extend_from_slice(&1_u32.to_be_bytes());
    for value in [1_u64, 1, 1, 1, 1, 8, 8, 64, 8] {
        request.extend_from_slice(&value.to_be_bytes());
    }
    request.extend_from_slice(&1_u64.to_be_bytes());
    request.push(1);
    request.extend_from_slice(&1_u32.to_be_bytes());
    for value in [1_u64, 1, 2, 4, 1, 1, 100] {
        request.extend_from_slice(&value.to_be_bytes());
    }
    for value in [100_u64, 1, 1, 10, 1, 1000] {
        request.extend_from_slice(&value.to_be_bytes());
    }
    assert_eq!(request.len(), 395, "LXGB request length");
    request
}

fn build_genesis(root: &Path, builder: &Path, sequencer_seed: &[u8; 32]) -> Genesis {
    let directory = root.join("genesis");
    make_dir(&directory, 0o755);
    let asset = random32();
    let sequencer_key = SigningKey::from_bytes(sequencer_seed)
        .verifying_key()
        .to_bytes();
    write(
        &directory.join("request.lxgb"),
        &genesis_request(&asset, &sequencer_key),
        0o600,
    );
    write(&directory.join("signer.key"), sequencer_seed, 0o600);
    let artifacts = directory.join("artifacts");
    command(
        &text(builder),
        &[
            &text(&directory.join("request.lxgb")),
            &text(&directory.join("signer.key")),
            &text(&artifacts),
        ],
    );
    must(
        fs::remove_file(directory.join("signer.key")),
        "discard signer",
    );
    let request = must(
        fs::read(artifacts.join("paxeer-registration-request.lxrr")),
        "registration request",
    );
    assert_eq!(request.len(), 73, "LXRR artifact length");
    assert_eq!(&request[..4], b"LXRR");
    let mut receipt_state_root = [0_u8; 32];
    receipt_state_root.copy_from_slice(&request[41..73]);
    Genesis {
        directory: artifacts,
        asset,
        receipt_state_root,
    }
}

fn registration(receipt_state_root: &[u8; 32]) -> Vec<u8> {
    let mut encoded = Vec::with_capacity(82);
    encoded.extend_from_slice(b"LXGR");
    encoded.push(1);
    encoded.extend_from_slice(&NETWORK_ID.to_be_bytes());
    encoded.extend_from_slice(&0_u64.to_be_bytes());
    encoded.extend_from_slice(receipt_state_root);
    encoded.extend_from_slice(receipt_state_root);
    encoded.push(1);
    encoded
}

fn node_config(role: &str) -> String {
    format!(
        "role={role}\nnetwork_id={NETWORK_ID}\nstart_sequence=0\nverify_workers=2\nnetwork_workers=2\nprojection_workers=2\ncheckpoint_workers=1\nserial_execution=false\n"
    )
}

struct Cluster {
    root: PathBuf,
    replica: Daemon,
    sequencer: Option<Daemon>,
    lni_socket: PathBuf,
    program_port: u16,
    program_token: String,
    replica_port: u16,
    replica_token: String,
    sequencer_id: [u8; 32],
    sequencer_key: [u8; 32],
    treasury_seed: [u8; 32],
    treasury_did: String,
    asset: [u8; 32],
}

fn start_cluster(with_sequencer: bool) -> Cluster {
    assert_eq!(
        effective_uid(),
        0,
        "the real-node harness must run as root so layerxd can run under a distinct uid"
    );
    let (root, layerxd, builder, migrations) = cluster_artifacts();
    let sequencer_seed = random32();
    let sequencer_key = SigningKey::from_bytes(&sequencer_seed)
        .verifying_key()
        .to_bytes();
    let sequencer_id = sha256(&[b"layerx-sequencer:", hex_encode(&sequencer_key).as_bytes()]);
    let replica_id = sha256(&[
        b"layerx-authority-replica:",
        hex_encode(&sequencer_key).as_bytes(),
    ]);
    let treasury_seed = random32();
    let treasury_did = treasury_did(&treasury_seed);
    let treasury_key = SigningKey::from_bytes(&treasury_seed)
        .verifying_key()
        .to_bytes();
    let genesis = build_genesis(&root, &builder, &sequencer_seed);
    let replica_token = token();
    let program_token = token();
    let replica_port = free_port();
    let program_port = free_port();

    let replica = start_replica(
        &root,
        &layerxd,
        [&sequencer_key, &sequencer_id, &replica_id],
        &replica_token,
        replica_port,
    );
    let (node_dir, checkpoints, logs, run_dir) =
        node_storage(&root, &genesis, &treasury_did, &treasury_key);
    let lni_socket = run_dir.join("layerxd.lni.sock");
    let node_env = node_environment(
        [&node_dir, &checkpoints, &logs, &migrations, &lni_socket],
        &genesis,
        [&sequencer_id, &sequencer_key, &sequencer_seed, &replica_id],
        [replica_port, program_port],
        [&replica_token, &program_token],
    );
    let sequencer = with_sequencer.then(|| {
        let mut sequencer = spawn(
            &layerxd,
            &["--serve", &text(&node_dir.join("config.txt"))],
            &node_env,
            true,
            root.join("sequencer.stderr"),
        );
        wait_for_lni(&lni_socket, &mut sequencer);
        sequencer
    });
    Cluster {
        root,
        replica,
        sequencer,
        lni_socket,
        program_port,
        program_token,
        replica_port,
        replica_token,
        sequencer_id,
        sequencer_key,
        treasury_seed,
        treasury_did,
        asset: genesis.asset,
    }
}

fn cluster_artifacts() -> (PathBuf, PathBuf, PathBuf, PathBuf) {
    let repository = repository_root();
    let binaries = std::env::var_os("LAYERX_TEST_NATIVE_BIN_DIR")
        .map_or_else(|| repository.join("build/bin"), PathBuf::from);
    let layerxd_source = binaries.join("layerxd");
    let builder = binaries.join("layerx-genesis-build");
    assert!(
        layerxd_source.is_file(),
        "{} is not built",
        layerxd_source.display()
    );
    assert!(builder.is_file(), "{} is not built", builder.display());
    let root = std::env::temp_dir().join(format!(
        "layerx-registry-native-{}-{}-{}",
        std::process::id(),
        now_ms(),
        NEXT_CLUSTER.fetch_add(1, Ordering::Relaxed)
    ));
    make_dir(&root, 0o755);
    let layerxd = root.join("layerxd");
    must(fs::hard_link(&layerxd_source, &layerxd), "link layerxd");
    let migrations = root.join("0007_history_index.sql");
    must(
        fs::copy(
            repository.join("migrations/0007_history_index.sql"),
            &migrations,
        ),
        "copy migrations",
    );
    must(
        fs::set_permissions(&migrations, fs::Permissions::from_mode(0o644)),
        "chmod migrations",
    );

    (root, layerxd, builder, migrations)
}

fn start_replica(
    root: &Path,
    layerxd: &Path,
    keys: [&[u8; 32]; 3],
    replica_token: &str,
    replica_port: u16,
) -> Daemon {
    let [sequencer_key, sequencer_id, replica_id] = keys;
    let replica_dir = root.join("replica");
    make_dir(&replica_dir, 0o700);
    write(
        &replica_dir.join("config.txt"),
        node_config("replica").as_bytes(),
        0o600,
    );
    preallocate_log(&replica_dir.join("receipt-authority.log"));
    chown_tree(&replica_dir, DAEMON_UID, DAEMON_GID);
    let mut replica_env = BTreeMap::new();
    replica_env.insert(
        "LAYERX_AUTHORITY_REPLICA_LOG",
        text(&replica_dir.join("receipt-authority.log")),
    );
    replica_env.insert("LAYERX_AUTHORITY_REPLICA_ID", hex_encode(replica_id));
    replica_env.insert("LAYERX_AUTHORITY_SEQUENCER_ID", hex_encode(sequencer_id));
    replica_env.insert(
        "LAYERX_AUTHORITY_SEQUENCER_PUBLIC_KEY",
        hex_encode(sequencer_key),
    );
    replica_env.insert("LAYERX_AUTHORITY_FIRST_BATCH", "1".to_owned());
    replica_env.insert("LAYERX_AUTHORITY_LAST_BATCH", LAST_BATCH.to_string());
    replica_env.insert("LAYERX_AUTHORITY_BEARER_TOKEN", replica_token.to_owned());
    replica_env.insert("LAYERX_AUTHORITY_ADDRESS", "127.0.0.1".to_owned());
    replica_env.insert("LAYERX_AUTHORITY_PORT", replica_port.to_string());
    let mut replica = spawn(
        layerxd,
        &[
            "--authority-replica",
            &text(&replica_dir.join("config.txt")),
        ],
        &replica_env,
        true,
        root.join("replica.stderr"),
    );
    wait_for_port(replica_port, &mut replica, "authority replica");

    replica
}

fn node_storage(
    root: &Path,
    genesis: &Genesis,
    treasury_did: &str,
    treasury_key: &[u8; 32],
) -> (PathBuf, PathBuf, PathBuf, PathBuf) {
    let node_dir = root.join("node");
    let checkpoints = node_dir.join("checkpoints");
    let logs = node_dir.join("logs");
    let run_dir = root.join("run");
    make_dir(&node_dir, 0o700);
    make_dir(&checkpoints, 0o700);
    make_dir(&logs, 0o700);
    make_dir(&run_dir, 0o750);
    write(
        &node_dir.join("registration.lxgr"),
        &registration(&genesis.receipt_state_root),
        0o600,
    );
    write(
        &node_dir.join("identities.txt"),
        format!(
            "{}:{}:0\n",
            hex_encode(treasury_did.as_bytes()),
            hex_encode(treasury_key)
        )
        .as_bytes(),
        0o600,
    );
    write(
        &node_dir.join("config.txt"),
        node_config("sequencer").as_bytes(),
        0o600,
    );
    for name in [
        "program-feed.log",
        "canonical.log",
        "receipt-authority.log",
        "batch.log",
        "evidence.log",
    ] {
        write(&logs.join(name), &[], 0o600);
    }
    chown_tree(&genesis.directory, DAEMON_UID, DAEMON_GID);
    chown_tree(&node_dir, DAEMON_UID, DAEMON_GID);
    chown_tree(&run_dir, DAEMON_UID, 0);
    (node_dir, checkpoints, logs, run_dir)
}

fn node_environment(
    paths: [&Path; 5],
    genesis: &Genesis,
    keys: [&[u8; 32]; 4],
    ports: [u16; 2],
    tokens: [&str; 2],
) -> BTreeMap<&'static str, String> {
    let [node_dir, checkpoints, logs, migrations, lni_socket] = paths;
    let [sequencer_id, sequencer_key, sequencer_seed, replica_id] = keys;
    let [replica_port, program_port] = ports;
    let [replica_token, program_token] = tokens;
    let mut node_env = BTreeMap::new();
    node_env.insert("LAYERX_NODE_CHECKPOINT_DIRECTORY", text(checkpoints));
    node_env.insert(
        "LAYERX_NODE_SNAPSHOT",
        text(&genesis.directory.join("00000000000000000000.lxs")),
    );
    node_env.insert(
        "LAYERX_NODE_GENESIS_MANIFEST",
        text(&genesis.directory.join("genesis.manifest")),
    );
    node_env.insert(
        "LAYERX_NODE_GENESIS_REGISTRATION",
        text(&node_dir.join("registration.lxgr")),
    );
    node_env.insert(
        "LAYERX_NODE_IDENTITIES",
        text(&node_dir.join("identities.txt")),
    );
    node_env.insert(
        "LAYERX_NODE_PROGRAM_FEED_LOG",
        text(&logs.join("program-feed.log")),
    );
    node_env.insert(
        "LAYERX_NODE_CANONICAL_LOG",
        text(&logs.join("canonical.log")),
    );
    node_env.insert(
        "LAYERX_NODE_RECEIPT_AUTHORITY_LOG",
        text(&logs.join("receipt-authority.log")),
    );
    node_env.insert("LAYERX_NODE_BATCH_LOG", text(&logs.join("batch.log")));
    node_env.insert("LAYERX_NODE_EVIDENCE_LOG", text(&logs.join("evidence.log")));
    node_env.insert(
        "LAYERX_NODE_HISTORY_DATABASE",
        text(&node_dir.join("history.sqlite")),
    );
    node_env.insert("LAYERX_NODE_HISTORY_MIGRATIONS", text(migrations));
    node_env.insert("LAYERX_NODE_SEQUENCER_ID", hex_encode(sequencer_id));
    node_env.insert(
        "LAYERX_NODE_SEQUENCER_PUBLIC_KEY",
        hex_encode(sequencer_key),
    );
    node_env.insert(
        "LAYERX_NODE_SEQUENCER_PRIVATE_KEY",
        hex_encode(sequencer_seed),
    );
    node_env.insert("LAYERX_NODE_FIRST_BATCH", "1".to_owned());
    node_env.insert("LAYERX_NODE_LAST_BATCH", LAST_BATCH.to_string());
    node_env.insert(
        "LAYERX_NODE_AUTHORITY_REPLICA_ADDRESS",
        "127.0.0.1".to_owned(),
    );
    node_env.insert(
        "LAYERX_NODE_AUTHORITY_REPLICA_PORT",
        replica_port.to_string(),
    );
    node_env.insert("LAYERX_NODE_AUTHORITY_REPLICA_ID", hex_encode(replica_id));
    node_env.insert(
        "LAYERX_NODE_AUTHORITY_REPLICA_BEARER_TOKEN",
        replica_token.to_owned(),
    );
    node_env.insert("LAYERX_NODE_PROGRAM_ADDRESS", "127.0.0.1".to_owned());
    node_env.insert("LAYERX_NODE_PROGRAM_PORT", program_port.to_string());
    node_env.insert("LAYERX_NODE_PROGRAM_BEARER_TOKEN", program_token.to_owned());
    node_env.insert("LAYERX_NODE_LNI_SOCKET", text(lni_socket));
    node_env.insert("LAYERX_NODE_LNI_ALLOWED_UID", "0".to_owned());
    node_env.insert("LAYERX_NODE_LNI_ALLOWED_GID", "0".to_owned());
    node_env.insert("LAYERX_NODE_LNI_FRAME_BYTES", LNI_FRAME_BYTES.to_string());
    node_env.insert("LAYERX_NODE_LNI_DEADLINE_MS", "2000".to_owned());
    finality_environment(&mut node_env);
    node_env
}

fn finality_environment(node_env: &mut BTreeMap<&'static str, String>) {
    node_env.insert("LAYERX_NODE_PAXEER_CHAIN_ID", "31337".to_owned());
    node_env.insert(
        "LAYERX_NODE_SETTLEMENT_CONTRACT",
        format!("0x{}", "11".repeat(20)),
    );
    node_env.insert(
        "LAYERX_NODE_CHECKPOINT_REGISTRY",
        format!("0x{}", "22".repeat(20)),
    );
    node_env.insert("LAYERX_NODE_PAXEER_RPC_ADDRESS", "127.0.0.1".to_owned());
    node_env.insert("LAYERX_NODE_PAXEER_RPC_PORT", "1".to_owned());
}

fn executed_deployment() -> (Cluster, layerx_programs::DeploymentProof) {
    use layerx_types::intent::ProgramId;
    use layerx_types::program_lifecycle::{NativeProgramDeploy, ProgramUpgradePolicy};
    let cluster = start_cluster(true);
    assert!(cluster.replica.child.id() > 0);
    assert!(cluster
        .sequencer
        .as_ref()
        .is_some_and(|daemon| daemon.child.id() > 0));
    assert_ne!(cluster.program_port, cluster.replica_port);
    assert!(!cluster.program_token.is_empty() && !cluster.replica_token.is_empty());
    assert_ne!(cluster.asset, [0; 32]);
    let fixture: serde_json::Value = must(
        serde_json::from_slice(&must(
            fs::read(
                repository_root()
                    .join("platform/sdk/conformance/fixtures/native-program-deploy-v3.json"),
            ),
            "native deployment payload",
        )),
        "payload JSON",
    );
    let bytes = must(
        layerx_programs::hex::decode(
            fixture["payload_hex"]
                .as_str()
                .unwrap_or_else(|| panic!("payload missing")),
        ),
        "payload hex",
    );
    let original = must(NativeProgramDeploy::decode(&bytes), "native payload");
    let account = must(
        layerx_types::account::AccountId::parse(&format!("agent:{}:main", cluster.treasury_did)),
        "account",
    );
    let deployment = NativeProgramDeploy {
        program_id: ProgramId::new(random32()),
        policy: ProgramUpgradePolicy::Authority(must(
            layerx_wire::hash::account_id_for_protocol(&account, 3),
            "owner",
        )),
        ..original
    };
    let signed = signed_program_activity(
        &cluster.treasury_seed,
        &cluster.treasury_did,
        account_sequence(&cluster.lni_socket, &cluster.treasury_did),
        1,
        &must(deployment.encode(), "deployment encoding"),
    );
    let mut invalid = signed.clone();
    let last = invalid.len() - 1;
    invalid[last] ^= 1;
    let head = chain_head(&cluster.lni_socket);
    assert!(layerx_platform_registry::deployment::deploy(
        &cluster.lni_socket,
        &invalid,
        Instant::now() + Duration::from_secs(5)
    )
    .is_err());
    assert_eq!(chain_head(&cluster.lni_socket), head);
    let result = layerx_platform_registry::deployment::deploy(
        &cluster.lni_socket,
        &signed,
        Instant::now() + Duration::from_secs(15),
    );
    if result.is_err() {
        diagnose_signed_deployment(&cluster, &signed);
    }
    let proof = must(result, "real native deployment proof");
    assert_eq!(proof.activity, signed);
    (cluster, proof)
}

#[test]
fn native_deployment_proof_binds_real_activity_receipt_and_state() {
    use layerx_programs::verify_state_membership;
    use layerx_proof::inclusion::{verify_activity, verify_receipt, SequencerAuthorization};
    let (cluster, proof) = executed_deployment();
    let authorization =
        SequencerAuthorization::new(cluster.sequencer_id, cluster.sequencer_key, 1, LAST_BATCH);
    must(
        verify_activity(
            &proof.activity,
            &proof.activity_proof,
            &proof.state.header,
            &proof.state.header_signature,
            &authorization,
        ),
        "native activity inclusion",
    );
    must(
        verify_receipt(
            &proof.state.receipt,
            &proof.state.receipt_proof,
            &proof.state.header,
            &proof.state.header_signature,
            &authorization,
        ),
        "native receipt inclusion",
    );
    let receipt = must(
        layerx_wire::receipt::decode(&proof.state.receipt),
        "native receipt",
    );
    let protocol = receipt
        .protocol()
        .unwrap_or_else(|| panic!("protocol receipt"));
    assert_maintained_root(&proof, protocol, &authorization);
    let record = &proof.state.program_record;
    must(
        verify_state_membership(
            &record.key,
            &record.value,
            &record.proof,
            proof.state.programs_root,
        ),
        "program record",
    );
    assert_lifecycle_neighbors(&proof);
    let mut changed = proof.clone();
    changed.state.header_signature[0] ^= 1;
    assert!(verify_activity(
        &changed.activity,
        &changed.activity_proof,
        &changed.state.header,
        &changed.state.header_signature,
        &authorization
    )
    .is_err());
    changed = proof.clone();
    changed.state.program_record.value[0] ^= 1;
    assert!(verify_state_membership(
        &changed.state.program_record.key,
        &changed.state.program_record.value,
        &changed.state.program_record.proof,
        changed.state.programs_root
    )
    .is_err());
    assert_eq!(
        must(
            layerx_programs::DeploymentProof::decode(&proof.canonical_encoding()),
            "canonical round trip"
        ),
        proof
    );
}

#[test]
fn real_deployment_produces_verified_canonical_journal_pair() {
    use layerx_platform_registry::FileDeploymentJournal;
    use layerx_programs::ProtocolDeploymentVerifier;
    let (cluster, proof) = executed_deployment();
    let header = must(
        layerx_wire::receipt::decode_batch_header(&proof.state.header),
        "header",
    );
    let mut history = b"LayerX/sequencer-trust-history/v1\0".to_vec();
    history.extend_from_slice(&1_u16.to_be_bytes());
    history.extend_from_slice(&0_u16.to_be_bytes());
    history.extend_from_slice(&PROTOCOL_VERSION.to_be_bytes());
    history.extend_from_slice(&NETWORK_ID.to_be_bytes());
    history.extend_from_slice(&header.epoch().to_be_bytes());
    history.extend_from_slice(&cluster.sequencer_id);
    history.extend_from_slice(&cluster.sequencer_key);
    history.extend_from_slice(&1_u64.to_be_bytes());
    history.extend_from_slice(&LAST_BATCH.to_be_bytes());
    history.extend_from_slice(&[0; 9]);
    let path = cluster.root.join("trust-history");
    write(&path, &history, 0o600);
    let verifier = must(
        ProtocolDeploymentVerifier::from_protected_history(&path, 60_000),
        "protocol-3 trust history",
    );
    let evidence = must(
        verifier.verify_deployment(&proof, now_ms()),
        "real deployment verification",
    );
    assert_eq!(
        must(
            verifier.verify_historical_deployment(&proof),
            "historical deployment"
        ),
        evidence
    );
    assert_deployment_refusals(&cluster, &proof, &history, &verifier);
    let journal = must(
        FileDeploymentJournal::open(cluster.root.join("journal")),
        "journal",
    );
    must(journal.append(&evidence), "append");
    must(journal.export_pair(&evidence), "pair");
    for (suffix, expected) in [
        ("admission", proof.canonical_encoding()),
        ("deployment", evidence.record().canonical_encoding()),
    ] {
        let path = cluster.root.join("journal/pairs").join(format!(
            "{}.{suffix}",
            hex_encode(&evidence.receipt_digest())
        ));
        assert_eq!(must(fs::read(&path), "pair bytes"), expected);
        assert_eq!(
            must(fs::metadata(&path), "pair mode").permissions().mode() & 0o777,
            0o600
        );
    }
    assert_human_materialization(&cluster);
}

fn assert_human_materialization(cluster: &Cluster) {
    let consumer = Command::new("python3")
        .args([
            "-c",
            r"
import importlib.util, os, pathlib, shutil, sys, tempfile
spec = importlib.util.spec_from_file_location('provision', sys.argv[1])
module = importlib.util.module_from_spec(spec)
spec.loader.exec_module(module)
root = pathlib.Path(sys.argv[2])
records = module.journal_records(root)
assert len(records) == 2
assert all(data == (root / name).read_bytes() for name, data in records.items())
with tempfile.TemporaryDirectory(dir=root.parent) as directory:
    work = pathlib.Path(directory)
    module.materialize_journal(work, root)
    destination = work / 'registry-journal'
    assert module.journal_records(destination) == records
    assert destination.stat().st_mode & 0o777 == 0o700
    try:
        module.materialize_journal(work, root)
    except module.Refused:
        pass
    else:
        raise AssertionError('existing export accepted')
    assert module.journal_records(destination) == records
for mutation in ('missing', 'mode', 'symlink', 'hardlink', 'empty'):
    with tempfile.TemporaryDirectory(dir=root.parent) as directory:
        work = pathlib.Path(directory)
        source = work / 'source'
        shutil.copytree(root, source)
        record = next(source.glob('*.deployment'))
        if mutation == 'missing':
            record.unlink()
        elif mutation == 'mode':
            record.chmod(0o644)
        elif mutation == 'symlink':
            record.unlink()
            record.symlink_to(root / record.name)
        elif mutation == 'hardlink':
            record.unlink()
            os.link(root / record.name, record)
        elif mutation == 'empty':
            record.write_bytes(b'')
        try:
            module.materialize_journal(work, source)
        except module.Refused:
            pass
        else:
            raise AssertionError(mutation + ' accepted')
        assert not (work / 'registry-journal').exists()
        assert not (work / '.registry-journal-publish').exists()
",
        ])
        .arg(repository_root().join("platform/hosted/human/provision.py"))
        .arg(cluster.root.join("journal/pairs"))
        .output();
    let output = must(consumer, "Human journal consumer");
    assert!(
        output.status.success(),
        "Human consumer refused native pair: {}",
        String::from_utf8_lossy(&output.stderr)
    );
}

fn assert_lifecycle_neighbors(proof: &layerx_programs::DeploymentProof) {
    use layerx_programs::{verify_state_membership, ProgramLifecycleProof};
    let record = &proof.state.program_record;
    let ProgramLifecycleProof::Active { lower, upper } = &proof.state.lifecycle else {
        panic!("fresh deployment must be active");
    };
    let mut target = b"wind-down\0s".to_vec();
    target.extend_from_slice(&record.key[8..]);
    for witness in [lower, upper].into_iter().flatten() {
        must(
            verify_state_membership(
                &witness.key,
                &witness.value,
                &witness.proof,
                proof.state.programs_root,
            ),
            "lifecycle neighbor",
        );
    }
    assert!(lower.as_ref().is_some_and(|leaf| leaf.key < target));
    assert!(upper.as_ref().is_none_or(|leaf| leaf.key > target));
    let lower = lower.as_ref().unwrap_or_else(|| panic!("lower neighbor"));
    if let Some(upper) = upper {
        assert_eq!(lower.proof.leaf_index + 1, upper.proof.leaf_index);
        assert_eq!(lower.proof.leaf_count, upper.proof.leaf_count);
    } else {
        assert_eq!(lower.proof.leaf_index + 1, lower.proof.leaf_count);
    }
}

fn diagnose_signed_deployment(cluster: &Cluster, signed: &[u8]) {
    use layerx_client::lni::schema::{decode_envelope, encode_envelope, Envelope};
    use layerx_client::lni::transport::FrameTransport as _;
    use layerx_proof::inclusion::{verify_receipt, SequencerAuthorization};
    let kind = must(ActivityType::new(ModuleId::Programs, 1), "type");
    let registry = must(
        ModuleRegistry::new(&[must(
            ModuleRegistration::new(ModuleId::Programs, &[kind]),
            "registration",
        )]),
        "registry",
    );
    let activity = must(
        layerx_wire::activity::decode_signed(signed, &registry),
        "activity",
    );
    let id = must(layerx_wire::hash::activity_id(&activity), "id");
    let mut transport = must(
        Uds::connect(&cluster.lni_socket, &ConnectionGate::new(1), lni_limits()),
        "diagnostic LNI",
    );
    let handshake = must(
        perform(&mut transport, &handshake_config(), None),
        "diagnostic handshake",
    );
    let mut payload = vec![0, 1, 3];
    payload.extend_from_slice(&id);
    must(
        transport.send(&must(
            encode_envelope(Envelope {
                version: handshake.node().interface_version,
                message_tag: 16,
                correlation_id: 3,
                canonical_payload: &payload,
                proof_material: &[],
            }),
            "diagnostic request",
        )),
        "diagnostic send",
    );
    let bytes = must(transport.receive(), "diagnostic receive");
    let response = must(decode_envelope(&bytes), "diagnostic response");
    assert_eq!(response.message_tag, 17);
    assert_eq!(response.correlation_id, 3);
    let material = response.proof_material;
    assert!(material.len() >= 44);
    assert_eq!(&material[..3], &[0, 1, 3]);
    assert_eq!(&material[3..35], &id);
    let merkle_end = 44 + usize::from(material[43]) * 32;
    let mut inclusion = vec![1];
    inclusion.extend_from_slice(&material[35..merkle_end]);
    let inclusion = must(
        layerx_proof::merkle::decode_proof(&inclusion),
        "receipt path",
    );
    let header_start = merkle_end + 86;
    let length = u32::from_be_bytes(must(
        material[header_start - 4..header_start].try_into(),
        "header length",
    )) as usize;
    let header_bytes = &material[header_start..header_start + length];
    let signature = must(
        material[header_start + length..].try_into(),
        "header signature",
    );
    must(
        verify_receipt(
            response.canonical_payload,
            &inclusion,
            header_bytes,
            &signature,
            &SequencerAuthorization::new(
                cluster.sequencer_id,
                cluster.sequencer_key,
                1,
                LAST_BATCH,
            ),
        ),
        "retained signed receipt inclusion",
    );
    let receipt = must(
        layerx_wire::receipt::decode(response.canonical_payload),
        "retained receipt",
    );
    let receipt = receipt
        .protocol()
        .unwrap_or_else(|| panic!("protocol receipt"));
    let header = must(
        layerx_wire::receipt::decode_batch_header(header_bytes),
        "retained header",
    );
    eprintln!("native deployment evidence: receipt sequence {}, batch last sequence {}, receipt count {}, receipt root equals signed header root: {}",
        receipt.global_sequence(), header.last_sequence(), inclusion.leaf_count(),
        receipt.resulting_state_root() == header.resulting_state_root());
}

fn assert_deployment_refusals(
    cluster: &Cluster,
    proof: &layerx_programs::DeploymentProof,
    history: &[u8],
    verifier: &layerx_programs::ProtocolDeploymentVerifier,
) {
    use layerx_programs::ProtocolDeploymentVerifier;
    for version in [0_u16, 1, 2, 4] {
        let mut other = history.to_vec();
        let offset = b"LayerX/sequencer-trust-history/v1\0".len() + 4;
        other[offset..offset + 2].copy_from_slice(&version.to_be_bytes());
        let other_path = cluster.root.join(format!("trust-{version}"));
        write(&other_path, &other, 0o600);
        let result = ProtocolDeploymentVerifier::from_protected_history(&other_path, 60_000);
        if matches!(version, 1 | 2) {
            let legacy = must(result, "legacy history remains supported");
            assert!(legacy.verify_deployment(proof, now_ms()).is_err());
        } else {
            assert!(result.is_err());
        }
    }
    let mut mutations = Vec::new();
    let mut changed = proof.clone();
    changed.maintenance = None;
    mutations.push(changed);
    let mut changed = proof.clone();
    changed.state.receipt[40] ^= 1;
    mutations.push(changed);
    let mut changed = proof.clone();
    changed.state.header_signature[0] ^= 1;
    mutations.push(changed);
    let mut changed = proof.clone();
    changed.state.programs_root[0] ^= 1;
    mutations.push(changed);
    let mut changed = proof.clone();
    changed.state.receipt_proof = proof
        .maintenance
        .as_ref()
        .unwrap_or_else(|| panic!("maintenance"))
        .receipt_proof
        .clone();
    mutations.push(changed);
    let mut changed = proof.clone();
    changed
        .maintenance
        .as_mut()
        .unwrap_or_else(|| panic!("maintenance"))
        .receipt[10] ^= 1;
    mutations.push(changed);
    let mut changed = proof.clone();
    changed
        .maintenance
        .as_mut()
        .unwrap_or_else(|| panic!("maintenance"))
        .receipt_proof = proof.state.receipt_proof.clone();
    mutations.push(changed);
    for changed in mutations {
        assert!(verifier.verify_deployment(&changed, now_ms()).is_err());
        assert!(verifier.verify_historical_deployment(&changed).is_err());
    }
}

fn assert_maintained_root(
    proof: &layerx_programs::DeploymentProof,
    protocol: &layerx_wire::receipt::ProtocolReceipt,
    authorization: &layerx_proof::inclusion::SequencerAuthorization,
) {
    use layerx_programs::verify_state_membership;
    use layerx_proof::inclusion::verify_receipt;
    let header = must(
        layerx_wire::receipt::decode_batch_header(&proof.state.header),
        "header",
    );
    assert_ne!(
        protocol.resulting_state_root(),
        header.resulting_state_root()
    );
    let maintenance = proof
        .maintenance
        .as_ref()
        .unwrap_or_else(|| panic!("maintenance missing"));
    must(
        verify_receipt(
            &maintenance.receipt,
            &maintenance.receipt_proof,
            &proof.state.header,
            &proof.state.header_signature,
            authorization,
        ),
        "maintenance inclusion",
    );
    assert_eq!(proof.state.receipt_proof.leaf_index(), 0);
    assert_eq!(proof.state.receipt_proof.leaf_count(), 2);
    assert_eq!(maintenance.receipt_proof.leaf_index(), 1);
    must(
        verify_state_membership(
            &9_u16.to_be_bytes(),
            &proof.state.programs_root,
            &proof.state.programs_root_proof,
            header.resulting_state_root(),
        ),
        "Programs root",
    );
}
