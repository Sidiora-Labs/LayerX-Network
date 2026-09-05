//! Drives the core boundary over TLS against a real `layerxd` sequencer and
//! authority replica started from `build/bin`.

use ed25519_dalek::SigningKey;
use layerx_client::lni::handshake::{perform, HandshakeConfig};
use layerx_client::lni::schema::Version;
use layerx_client::lni::transport::{ConnectionGate, Limits, Uds};
use layerx_platform_core::{build_send, hex_encode, treasury_did, SendRequest};
use native_tls::{Certificate, Identity, TlsConnector};
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;
use std::fmt::Debug;
use std::fs;
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::os::unix::fs::PermissionsExt;
use std::os::unix::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::thread;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

const NETWORK_ID: u32 = 7332;
const PROTOCOL_VERSION: u16 = 2;
const LAST_BATCH: u64 = u64::MAX;
const LNI_FRAME_BYTES: usize = 1_146_902;
const LOG_BYTES: u64 = 64 * 1024 * 1024;
const MODULE_GOVERNANCE: u16 = 7;
const DAEMON_UID: u32 = 65534;
const DAEMON_GID: u32 = 0;

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
    manifest
        .ancestors()
        .nth(3)
        .map(Path::to_path_buf)
        .unwrap_or_else(|| panic!("repository root above {}", manifest.display()))
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
    stderr: PathBuf,
}

impl Daemon {
    fn stop(&mut self) {
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
    let child = must(command.spawn(), &format!("spawn {}", program.display()));
    Daemon { child, stderr }
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
        built_interface_version: Version::V1_3,
        expected_protocol_version: PROTOCOL_VERSION,
        expected_network_id: NETWORK_ID,
    }
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

struct Certificates {
    directory: PathBuf,
    ca_der: Vec<u8>,
}

impl Certificates {
    fn path(&self, name: &str) -> PathBuf {
        self.directory.join(name)
    }

    fn client_identity(&self, name: &str) -> Identity {
        let certificate = must(fs::read(self.path(&format!("{name}.pem"))), "client cert");
        let key = must(
            fs::read(self.path(&format!("{name}-key.pem"))),
            "client key",
        );
        must(Identity::from_pkcs8(&certificate, &key), "client identity")
    }
}

fn issue_ca(directory: &Path, name: &str, subject: &str) {
    let key = text(&directory.join(format!("{name}-key.pem")));
    let certificate = text(&directory.join(format!("{name}.pem")));
    command(
        "openssl",
        &[
            "req",
            "-x509",
            "-newkey",
            "ec",
            "-pkeyopt",
            "ec_paramgen_curve:P-256",
            "-nodes",
            "-keyout",
            &key,
            "-out",
            &certificate,
            "-days",
            "1",
            "-subj",
            subject,
            "-addext",
            "basicConstraints=critical,CA:TRUE,pathlen:0",
            "-addext",
            "keyUsage=critical,keyCertSign,cRLSign",
        ],
    );
    command(
        "openssl",
        &[
            "x509",
            "-in",
            &certificate,
            "-outform",
            "DER",
            "-out",
            &text(&directory.join(format!("{name}.der"))),
        ],
    );
}

fn issue_cert(directory: &Path, ca: &str, name: &str, usage: &str) {
    let key = text(&directory.join(format!("{name}-key.pem")));
    let csr = text(&directory.join(format!("{name}.csr")));
    let certificate = text(&directory.join(format!("{name}.pem")));
    let extensions = directory.join(format!("{name}.ext"));
    write(
        &extensions,
        format!(
            "basicConstraints=CA:FALSE\nkeyUsage=digitalSignature,keyEncipherment\nextendedKeyUsage={usage}\nsubjectAltName=DNS:localhost,IP:127.0.0.1\n"
        )
        .as_bytes(),
        0o600,
    );
    command(
        "openssl",
        &[
            "req",
            "-new",
            "-newkey",
            "ec",
            "-pkeyopt",
            "ec_paramgen_curve:P-256",
            "-nodes",
            "-keyout",
            &key,
            "-out",
            &csr,
            "-subj",
            &format!("/O=LayerX beta/CN={name}"),
        ],
    );
    command(
        "openssl",
        &[
            "x509",
            "-req",
            "-in",
            &csr,
            "-CA",
            &text(&directory.join(format!("{ca}.pem"))),
            "-CAkey",
            &text(&directory.join(format!("{ca}-key.pem"))),
            "-CAcreateserial",
            "-days",
            "1",
            "-extfile",
            &text(&extensions),
            "-out",
            &certificate,
        ],
    );
    command(
        "openssl",
        &[
            "x509",
            "-in",
            &certificate,
            "-outform",
            "DER",
            "-out",
            &text(&directory.join(format!("{name}.der"))),
        ],
    );
    command(
        "openssl",
        &[
            "pkcs8",
            "-topk8",
            "-nocrypt",
            "-in",
            &key,
            "-outform",
            "DER",
            "-out",
            &text(&directory.join(format!("{name}-key.der"))),
        ],
    );
}

fn certificates(root: &Path) -> Certificates {
    let directory = root.join("tls");
    make_dir(&directory, 0o700);
    issue_ca(
        &directory,
        "ca",
        "/O=LayerX beta/CN=LayerX beta internal CA",
    );
    issue_ca(&directory, "rogue-ca", "/O=Somebody else/CN=rogue CA");
    issue_cert(&directory, "ca", "core", "serverAuth");
    issue_cert(&directory, "ca", "admin", "serverAuth");
    issue_cert(&directory, "ca", "client", "clientAuth");
    issue_cert(&directory, "rogue-ca", "rogue", "clientAuth");
    let ca_der = must(fs::read(directory.join("ca.der")), "ca der");
    Certificates { directory, ca_der }
}

struct HttpAnswer {
    status: u16,
    headers: BTreeMap<String, String>,
    body: String,
}

fn parse_http(raw: &[u8]) -> HttpAnswer {
    let position = raw
        .windows(4)
        .position(|window| window == b"\r\n\r\n")
        .unwrap_or_else(|| panic!("no header terminator in {raw:?}"));
    let head = String::from_utf8_lossy(&raw[..position]).into_owned();
    let mut lines = head.split("\r\n");
    let status = lines
        .next()
        .and_then(|line| line.split_whitespace().nth(1))
        .and_then(|code| code.parse::<u16>().ok())
        .unwrap_or_else(|| panic!("status line missing in {head}"));
    let mut headers = BTreeMap::new();
    for line in lines {
        let (name, value) = line
            .split_once(':')
            .unwrap_or_else(|| panic!("malformed header {line}"));
        let previous = headers.insert(name.trim().to_ascii_lowercase(), value.trim().to_owned());
        assert!(previous.is_none(), "duplicate header {name}");
    }
    let length = headers
        .get("content-length")
        .and_then(|value| value.parse::<usize>().ok())
        .unwrap_or_else(|| panic!("content-length missing in {head}"));
    let body = &raw[position + 4..];
    assert_eq!(body.len(), length, "body length matches content-length");
    assert_eq!(
        headers.get("content-type").map(String::as_str),
        Some("application/json")
    );
    assert_eq!(
        headers.get("cache-control").map(String::as_str),
        Some("no-store")
    );
    assert_eq!(headers.get("connection").map(String::as_str), Some("close"));
    assert!(!headers.contains_key("transfer-encoding"));
    HttpAnswer {
        status,
        headers,
        body: String::from_utf8_lossy(body).into_owned(),
    }
}

struct Http {
    port: u16,
    ca: Certificate,
    identity: Option<Identity>,
}

impl Http {
    fn connector(&self) -> TlsConnector {
        let mut builder = TlsConnector::builder();
        builder.add_root_certificate(self.ca.clone());
        if let Some(identity) = &self.identity {
            builder.identity(identity.clone());
        }
        must(builder.build(), "tls connector")
    }

    fn raw(&self, request: &str, body: &[u8]) -> Result<HttpAnswer, String> {
        let tcp = must(TcpStream::connect(("127.0.0.1", self.port)), "connect");
        must(
            tcp.set_read_timeout(Some(Duration::from_secs(60))),
            "read timeout",
        );
        let mut stream = self
            .connector()
            .connect("localhost", tcp)
            .map_err(|error| error.to_string())?;
        must(stream.write_all(request.as_bytes()), "write request");
        must(stream.write_all(body), "write body");
        let mut raw = Vec::new();
        let _ = stream.read_to_end(&mut raw);
        Ok(parse_http(&raw))
    }

    fn request(
        &self,
        method: &str,
        target: &str,
        headers: &[(&str, &str)],
        body: &[u8],
    ) -> HttpAnswer {
        let mut request = format!(
            "{method} {target} HTTP/1.1\r\nHost: localhost\r\nContent-Length: {}\r\n",
            body.len()
        );
        for (name, value) in headers {
            request.push_str(&format!("{name}: {value}\r\n"));
        }
        request.push_str("\r\n");
        must(self.raw(&request, body), "tls request")
    }

    fn get(&self, target: &str) -> HttpAnswer {
        self.request("GET", target, &[], &[])
    }
}

fn json(answer: &HttpAnswer) -> serde_json::Value {
    must(
        serde_json::from_str(&answer.body),
        &format!("json body {}", answer.body),
    )
}

fn error_code(answer: &HttpAnswer) -> String {
    json(answer)["error"]["code"]
        .as_str()
        .unwrap_or_else(|| panic!("no error code in {}", answer.body))
        .to_owned()
}

fn assert_refusal(answer: &HttpAnswer, status: u16, code: &str) {
    assert_eq!(answer.status, status, "status for {code}: {}", answer.body);
    assert_eq!(error_code(answer), code, "code: {}", answer.body);
    let value = json(answer);
    let retry = value["error"]["retry"].as_str().unwrap_or_default();
    match retry {
        "after" => {
            assert!(
                value["error"]["retry_after_seconds"].is_u64(),
                "{}",
                answer.body
            );
            assert!(
                answer.headers.contains_key("retry-after"),
                "Retry-After for {code}"
            );
        }
        "never" => assert!(value["error"].get("retry_after_seconds").is_none()),
        other => panic!("unexpected retry class {other} in {}", answer.body),
    }
}

struct Cluster {
    root: PathBuf,
    replica: Daemon,
    sequencer: Option<Daemon>,
    lni_socket: PathBuf,
    program_port: u16,
    program_token: String,
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
    let repository = repository_root();
    let layerxd_source = repository.join("build/bin/layerxd");
    let builder = repository.join("build/bin/layerx-genesis-build");
    assert!(
        layerxd_source.is_file(),
        "{} is not built",
        layerxd_source.display()
    );
    assert!(builder.is_file(), "{} is not built", builder.display());
    let root =
        std::env::temp_dir().join(format!("layerx-core-{}-{}", std::process::id(), now_ms()));
    make_dir(&root, 0o755);
    let layerxd = root.join("layerxd");
    must(fs::copy(&layerxd_source, &layerxd), "copy layerxd");
    must(
        fs::set_permissions(&layerxd, fs::Permissions::from_mode(0o755)),
        "chmod layerxd",
    );
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
    replica_env.insert("LAYERX_AUTHORITY_REPLICA_ID", hex_encode(&replica_id));
    replica_env.insert("LAYERX_AUTHORITY_SEQUENCER_ID", hex_encode(&sequencer_id));
    replica_env.insert(
        "LAYERX_AUTHORITY_SEQUENCER_PUBLIC_KEY",
        hex_encode(&sequencer_key),
    );
    replica_env.insert("LAYERX_AUTHORITY_FIRST_BATCH", "1".to_owned());
    replica_env.insert("LAYERX_AUTHORITY_LAST_BATCH", LAST_BATCH.to_string());
    replica_env.insert("LAYERX_AUTHORITY_BEARER_TOKEN", replica_token.clone());
    replica_env.insert("LAYERX_AUTHORITY_ADDRESS", "127.0.0.1".to_owned());
    replica_env.insert("LAYERX_AUTHORITY_PORT", replica_port.to_string());
    let mut replica = spawn(
        &layerxd,
        &[
            "--authority-replica",
            &text(&replica_dir.join("config.txt")),
        ],
        &replica_env,
        true,
        root.join("replica.stderr"),
    );
    wait_for_port(replica_port, &mut replica, "authority replica");

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
            hex_encode(&treasury_key)
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
        preallocate_log(&logs.join(name));
    }
    chown_tree(&genesis.directory, DAEMON_UID, DAEMON_GID);
    chown_tree(&node_dir, DAEMON_UID, DAEMON_GID);
    chown_tree(&run_dir, DAEMON_UID, 0);
    let lni_socket = run_dir.join("layerxd.lni.sock");
    let mut node_env = BTreeMap::new();
    node_env.insert("LAYERX_NODE_CHECKPOINT_DIRECTORY", text(&checkpoints));
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
    node_env.insert("LAYERX_NODE_HISTORY_MIGRATIONS", text(&migrations));
    node_env.insert("LAYERX_NODE_SEQUENCER_ID", hex_encode(&sequencer_id));
    node_env.insert(
        "LAYERX_NODE_SEQUENCER_PUBLIC_KEY",
        hex_encode(&sequencer_key),
    );
    node_env.insert(
        "LAYERX_NODE_SEQUENCER_PRIVATE_KEY",
        hex_encode(&sequencer_seed),
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
    node_env.insert("LAYERX_NODE_AUTHORITY_REPLICA_ID", hex_encode(&replica_id));
    node_env.insert("LAYERX_NODE_AUTHORITY_REPLICA_BEARER_TOKEN", replica_token);
    node_env.insert("LAYERX_NODE_PROGRAM_ADDRESS", "127.0.0.1".to_owned());
    node_env.insert("LAYERX_NODE_PROGRAM_PORT", program_port.to_string());
    node_env.insert("LAYERX_NODE_PROGRAM_BEARER_TOKEN", program_token.clone());
    node_env.insert("LAYERX_NODE_LNI_SOCKET", text(&lni_socket));
    node_env.insert("LAYERX_NODE_LNI_ALLOWED_UID", "0".to_owned());
    node_env.insert("LAYERX_NODE_LNI_ALLOWED_GID", "0".to_owned());
    node_env.insert("LAYERX_NODE_LNI_FRAME_BYTES", LNI_FRAME_BYTES.to_string());
    node_env.insert("LAYERX_NODE_LNI_DEADLINE_MS", "2000".to_owned());
    for name in [
        "LAYERX_NODE_PAXEER_CHAIN_ID",
        "LAYERX_NODE_SETTLEMENT_CONTRACT",
        "LAYERX_NODE_PAXEER_RPC_ADDRESS",
        "LAYERX_NODE_PAXEER_RPC_PORT",
    ] {
        if let Ok(value) = std::env::var(name) {
            node_env.insert(name, value);
        }
    }
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
        sequencer_id,
        sequencer_key,
        treasury_seed,
        treasury_did,
        asset: genesis.asset,
    }
}

struct Boundary {
    process: Daemon,
    core: Http,
    admin: Http,
    admin_token: String,
    supervisor_socket: PathBuf,
}

fn start_boundary(cluster: &Cluster, certificates: &Certificates) -> Boundary {
    let secrets = cluster.root.join("secrets");
    make_dir(&secrets, 0o700);
    let admin_token = token();
    write(
        &secrets.join("admin-token"),
        format!("{admin_token}\n").as_bytes(),
        0o600,
    );
    write(
        &secrets.join("program-token"),
        cluster.program_token.as_bytes(),
        0o600,
    );
    write(
        &secrets.join("treasury-key.hex"),
        hex_encode(&cluster.treasury_seed).as_bytes(),
        0o600,
    );
    let state = cluster.root.join("state");
    make_dir(&state, 0o700);
    let supervisor_socket = cluster.root.join("run").join("supervisor.sock");
    let core_port = free_port();
    let admin_port = free_port();
    let mut env = BTreeMap::new();
    env.insert("LAYERX_CORE_LISTEN", format!("127.0.0.1:{core_port}"));
    env.insert(
        "LAYERX_CORE_ADMIN_LISTEN",
        format!("127.0.0.1:{admin_port}"),
    );
    env.insert(
        "LAYERX_CORE_TLS_CERT_DER",
        text(&certificates.path("core.der")),
    );
    env.insert(
        "LAYERX_CORE_TLS_KEY_DER",
        text(&certificates.path("core-key.der")),
    );
    env.insert(
        "LAYERX_CORE_ADMIN_TLS_CERT_DER",
        text(&certificates.path("admin.der")),
    );
    env.insert(
        "LAYERX_CORE_ADMIN_TLS_KEY_DER",
        text(&certificates.path("admin-key.der")),
    );
    env.insert(
        "LAYERX_CORE_CLIENT_CA_DER",
        text(&certificates.path("ca.der")),
    );
    env.insert("LAYERX_CORE_LNI_SOCKET", text(&cluster.lni_socket));
    env.insert("LAYERX_CORE_NETWORK_ID", NETWORK_ID.to_string());
    env.insert(
        "LAYERX_CORE_NODE_URL",
        format!("http://127.0.0.1:{}", cluster.program_port),
    );
    env.insert(
        "LAYERX_CORE_NODE_BEARER_TOKEN_FILE",
        text(&secrets.join("program-token")),
    );
    env.insert(
        "LAYERX_CORE_ADMIN_TOKEN_FILE",
        text(&secrets.join("admin-token")),
    );
    env.insert(
        "LAYERX_CORE_TREASURY_KEY_FILE",
        text(&secrets.join("treasury-key.hex")),
    );
    env.insert("LAYERX_CORE_TREASURY_ASSET", hex_encode(&cluster.asset));
    env.insert(
        "LAYERX_CORE_SEQUENCER_ID",
        hex_encode(&cluster.sequencer_id),
    );
    env.insert("LAYERX_CORE_SUPERVISOR_SOCKET", text(&supervisor_socket));
    env.insert("LAYERX_CORE_STATE_DIR", text(&state));
    env.insert("LAYERX_CORE_RECEIPT_DEADLINE_MS", "20000".to_owned());
    let mut process = spawn(
        Path::new(env!("CARGO_BIN_EXE_layerx-core-boundary")),
        &[],
        &env,
        false,
        cluster.root.join("boundary.stderr"),
    );
    wait_for_port(core_port, &mut process, "core boundary");
    wait_for_port(admin_port, &mut process, "core admin");
    let ca = must(
        Certificate::from_der(&certificates.ca_der),
        "ca certificate",
    );
    Boundary {
        process,
        core: Http {
            port: core_port,
            ca: ca.clone(),
            identity: None,
        },
        admin: Http {
            port: admin_port,
            ca,
            identity: None,
        },
        admin_token,
        supervisor_socket,
    }
}

impl Boundary {
    fn admin_post(&self, path: &str, key: &str, body: &str) -> HttpAnswer {
        let bearer = format!("Bearer {}", self.admin_token);
        self.admin.request(
            "POST",
            path,
            &[
                ("Authorization", &bearer),
                ("Content-Type", "application/json"),
                ("Idempotency-Key", key),
            ],
            body.as_bytes(),
        )
    }
}

fn funding_body(did: &str, public_key: &str, amount: u64) -> String {
    serde_json::json!({
        "funding_id": format!("fund-{}", now_ms()),
        "did": did,
        "public_key": public_key,
        "amount": amount,
    })
    .to_string()
}

fn recipient() -> (String, String) {
    let key = SigningKey::from_bytes(&random32())
        .verifying_key()
        .to_bytes();
    let hex = hex_encode(&key);
    (format!("did:layerx:{hex}"), hex)
}

fn assert_readiness_shape(answer: &HttpAnswer) {
    let value = json(answer);
    assert_eq!(
        value["ready"],
        serde_json::Value::Bool(true),
        "{}",
        answer.body
    );
    assert_eq!(
        value["network_id"],
        serde_json::json!(NETWORK_ID.to_string())
    );
    assert_eq!(value["wire_version"], serde_json::json!("1.3"));
    assert_eq!(value["synchronous_receipts"], serde_json::Value::Bool(true));
    assert_eq!(value["state_snapshot"], serde_json::Value::Bool(true));
    let object = value.as_object().unwrap_or_else(|| panic!("object"));
    assert_eq!(
        object.len(),
        5,
        "gateway ReadinessResponse denies unknown fields"
    );
}

#[test]
fn boundary_refuses_typed_and_journals_while_the_daemon_is_down() {
    let cluster = start_cluster(false);
    let certificates = certificates(&cluster.root);
    let boundary = start_boundary(&cluster, &certificates);
    let core = &boundary.core;
    let admin = &boundary.admin;

    assert_eq!(core.get("/livez").status, 200);
    assert_eq!(admin.get("/livez").status, 200);
    assert_refusal(&core.get("/readyz"), 503, "node_unavailable");
    assert_refusal(&admin.get("/readyz"), 503, "node_unavailable");
    assert_refusal(&core.get("/v1/sequencer"), 503, "node_unavailable");
    assert_refusal(&core.get("/v1/state"), 503, "node_unavailable");
    assert_refusal(
        &core.get("/v1/protocol/account-state/head"),
        503,
        "node_unavailable",
    );
    assert_refusal(
        &core.get(&format!("/v1/receipts/{}", hex_encode(&[7_u8; 32]))),
        503,
        "node_unavailable",
    );
    assert_refusal(&core.get("/v1/receipts/not-hex"), 400, "invalid_argument");
    assert_refusal(
        &core.request(
            "POST",
            "/v1/programs/simulate",
            &[("Content-Type", "application/json")],
            b"{}",
        ),
        503,
        "capability_unavailable",
    );
    assert_refusal(
        &core.get("/v1/programs/registry"),
        503,
        "capability_unavailable",
    );
    assert_refusal(&core.get("/nope"), 404, "not_found");
    assert_refusal(
        &core.request("DELETE", "/v1/activities", &[], &[]),
        405,
        "method_not_allowed",
    );
    assert_refusal(&core.get("/livez?x=<script>"), 400, "invalid_request");
    assert_refusal(
        &core.request(
            "POST",
            "/v1/activities",
            &[("Content-Type", "text/plain")],
            b"zz",
        ),
        400,
        "content_type_required",
    );
    assert_refusal(
        &core.request(
            "POST",
            "/v1/activities",
            &[("Content-Type", "application/json")],
            b"{\"activity\":\"zz\"}",
        ),
        400,
        "invalid_argument",
    );
    let signed = must(
        build_send(
            &cluster.treasury_seed,
            &SendRequest {
                network_id: NETWORK_ID,
                source_did: cluster.treasury_did.clone(),
                destination_did: recipient().0,
                asset: cluster.asset,
                amount: 5,
                account_sequence: 0,
                idempotency_key: random32(),
                not_before_ms: now_ms() - 1_000,
                expires_at_ms: now_ms() + 60_000,
                fee_limit: 1_000,
            },
        ),
        "treasury send",
    );
    let body = serde_json::json!({ "activity": hex_encode(&signed.canonical) }).to_string();
    assert_refusal(
        &core.request(
            "POST",
            "/v1/activities",
            &[("Content-Type", "application/json")],
            body.as_bytes(),
        ),
        503,
        "node_unavailable",
    );

    let with_client = Http {
        port: core.port,
        ca: core.ca.clone(),
        identity: Some(certificates.client_identity("client")),
    };
    assert_eq!(
        with_client.get("/livez").status,
        200,
        "client certificate chained to the CA is accepted"
    );
    let rogue = Http {
        port: core.port,
        ca: core.ca.clone(),
        identity: Some(certificates.client_identity("rogue")),
    };
    let rogue_request = "GET /livez HTTP/1.1\r\nHost: localhost\r\nContent-Length: 0\r\n\r\n";
    assert!(
        rogue.raw(rogue_request, &[]).is_err(),
        "client certificate from another CA must fail the handshake"
    );

    let (did, public_key) = recipient();
    let valid = funding_body(&did, &public_key, 25);
    assert_refusal(
        &admin.request(
            "POST",
            "/admin/v1/testnet/fund",
            &[("Content-Type", "application/json")],
            valid.as_bytes(),
        ),
        401,
        "unauthorized",
    );
    let bearer = format!("Bearer {}", boundary.admin_token);
    assert_refusal(
        &admin.request(
            "POST",
            "/admin/v1/testnet/fund",
            &[("Authorization", &bearer)],
            valid.as_bytes(),
        ),
        400,
        "content_type_required",
    );
    assert_refusal(
        &admin.request(
            "POST",
            "/admin/v1/testnet/fund",
            &[
                ("Authorization", &bearer),
                ("Content-Type", "application/json"),
            ],
            valid.as_bytes(),
        ),
        400,
        "idempotency_key_required",
    );
    assert_refusal(
        &boundary.admin_post("/admin/v1/testnet/fund", "bad key!", &valid),
        400,
        "invalid_idempotency_key",
    );
    assert_refusal(&admin.get("/admin/v1/testnet/fund"), 401, "unauthorized");
    let get = admin.request(
        "GET",
        "/admin/v1/testnet/fund",
        &[("Authorization", &bearer)],
        &[],
    );
    assert_refusal(&get, 405, "method_not_allowed");

    let unavailable = boundary.admin_post("/admin/v1/testnet/fund", "fund-down-1", &valid);
    assert_refusal(&unavailable, 503, "node_unavailable");
    let again = boundary.admin_post("/admin/v1/testnet/fund", "fund-down-1", &valid);
    assert_refusal(&again, 503, "node_unavailable");

    let invalid = funding_body(&did, &public_key, 0);
    let refused = boundary.admin_post("/admin/v1/testnet/fund", "fund-invalid-1", &invalid);
    assert_refusal(&refused, 400, "invalid_argument");
    let replayed = boundary.admin_post("/admin/v1/testnet/fund", "fund-invalid-1", &invalid);
    assert_eq!(replayed.status, 400);
    assert_eq!(
        replayed.body, refused.body,
        "journaled refusal replays byte for byte"
    );
    let conflict = boundary.admin_post("/admin/v1/testnet/fund", "fund-invalid-1", &valid);
    assert_refusal(&conflict, 409, "idempotency_conflict");
    let unknown_field = serde_json::json!({
        "funding_id": "f", "did": did, "public_key": public_key, "amount": 1, "extra": 1
    })
    .to_string();
    assert_refusal(
        &boundary.admin_post("/admin/v1/testnet/fund", "fund-invalid-2", &unknown_field),
        400,
        "invalid_argument",
    );

    assert!(!boundary.supervisor_socket.exists());
    assert_refusal(
        &boundary.admin_post("/admin/v1/testnet/reset", "reset-1", "{}"),
        503,
        "supervisor_unavailable",
    );
    assert_refusal(
        &boundary.admin_post("/admin/v1/testnet/reset", "reset-2", "{\"a\":1}"),
        400,
        "invalid_argument",
    );
    assert_refusal(
        &boundary.admin_post("/admin/v1/testnet/other", "x-1", "{}"),
        404,
        "not_found",
    );

    drop(boundary);
    drop(cluster);
}

#[test]
fn boundary_serves_the_real_sequencer_over_the_lni() {
    let mut cluster = start_cluster(true);
    let certificates = certificates(&cluster.root);
    let boundary = start_boundary(&cluster, &certificates);
    let core = &boundary.core;

    let ready = core.get("/readyz");
    assert_eq!(ready.status, 200, "{}", ready.body);
    assert_readiness_shape(&ready);
    assert_readiness_shape(&boundary.admin.get("/readyz"));

    let sequencer = core.get("/v1/sequencer");
    assert_eq!(sequencer.status, 200, "{}", sequencer.body);
    let value = json(&sequencer);
    assert_eq!(value["ok"], serde_json::Value::Bool(true));
    assert_eq!(value["result"]["network_id"], serde_json::json!(NETWORK_ID));
    assert_eq!(
        value["result"]["sequencer_public_key"],
        serde_json::json!(hex_encode(&cluster.sequencer_key))
    );
    assert!(value["trace"]
        .as_str()
        .is_some_and(|trace| trace.starts_with("core-")));

    let state = core.get("/v1/state");
    assert_eq!(state.status, 200, "{}", state.body);
    assert_eq!(json(&state)["ok"], serde_json::Value::Bool(true));
    let head = core.get("/v1/protocol/account-state/head");
    assert_eq!(head.status, 200, "{}", head.body);
    let missing = core.get(&format!(
        "/v1/receipts/{}/account-state",
        hex_encode(&[9_u8; 32])
    ));
    assert_eq!(missing.status, 404, "{}", missing.body);
    assert_refusal(
        &core.get(&format!("/v1/receipts/{}", hex_encode(&[9_u8; 32]))),
        404,
        "not_found",
    );

    let (before, key) = chain_head(&cluster.lni_socket).unwrap_or_else(|| panic!("LNI head"));
    assert_eq!(key, cluster.sequencer_key);
    let (did, public_key) = recipient();
    let body = funding_body(&did, &public_key, 25);
    let first = boundary.admin_post("/admin/v1/testnet/fund", "fund-real-1", &body);
    assert!(
        first.status == 422,
        "a fresh genesis carries no funded treasury account, so funding must be refused with a typed 4xx: {} {}",
        first.status,
        first.body
    );
    let code = error_code(&first);
    assert!(
        code == "treasury_account_unavailable" || code == "insufficient_treasury_balance",
        "{}",
        first.body
    );
    let second = boundary.admin_post("/admin/v1/testnet/fund", "fund-real-1", &body);
    assert_eq!(second.status, first.status);
    assert_eq!(
        second.body, first.body,
        "the journal replays the funding outcome"
    );
    let (after, _) = chain_head(&cluster.lni_socket).unwrap_or_else(|| panic!("LNI head"));
    assert_eq!(
        before, after,
        "a repeated Idempotency-Key does not move value twice"
    );

    let signed = must(
        build_send(
            &cluster.treasury_seed,
            &SendRequest {
                network_id: NETWORK_ID,
                source_did: cluster.treasury_did.clone(),
                destination_did: did,
                asset: cluster.asset,
                amount: 5,
                account_sequence: 0,
                idempotency_key: random32(),
                not_before_ms: now_ms() - 1_000,
                expires_at_ms: now_ms() + 60_000,
                fee_limit: 1_000,
            },
        ),
        "treasury send",
    );
    let activity = serde_json::json!({ "activity": hex_encode(&signed.canonical) }).to_string();
    let submitted = core.request(
        "POST",
        "/v1/activities",
        &[
            ("Content-Type", "application/json"),
            ("Idempotency-Key", "act-1"),
        ],
        activity.as_bytes(),
    );
    let outcome = json(&submitted);
    let refused_receipt =
        submitted.status == 200 && outcome["result"]["state"] == serde_json::json!("refused");
    let refused_submission = submitted.status == 422
        && outcome["error"]["code"] == serde_json::json!("submission_refused");
    assert!(
        refused_receipt || refused_submission,
        "an unfunded treasury send must be refused by the core, not completed: {} {}",
        submitted.status,
        submitted.body
    );
    if refused_receipt {
        assert_eq!(
            outcome["result"]["activity_id"],
            serde_json::json!(hex_encode(&signed.activity_id))
        );
    }

    if let Some(mut sequencer) = cluster.sequencer.take() {
        sequencer.stop();
    }
    assert_refusal(&core.get("/readyz"), 503, "node_unavailable");
    drop(boundary);
    drop(cluster.replica);
}
