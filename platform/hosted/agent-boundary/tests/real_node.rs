//! Exercises the agent boundary against a real `layerxd` sequencer and a real
//! `layerxd --authority-replica` started from `build/bin`.

use ed25519_dalek::{Signer as _, SigningKey};
use layerx_proof::receipt::verify_sequencer_signature;
use layerx_types::activity::{Authority, EnvelopeBuilder, Signature, TimestampBound};
use layerx_types::amount::Amount;
use layerx_types::ids::{Did, IdempotencyKey};
use layerx_types::payload::{ActivityType, ModuleId, ModuleRegistration, ModuleRegistry, Payload};
use layerx_wire::activity::{decode_signed, encode_signed_envelope, encode_unsigned_envelope};
use layerx_wire::encode::Encoder;
use layerx_wire::hash::{activity_id, Domain};
use layerx_wire::limits::STATE_COMMITMENT_PROTOCOL_VERSION as PROTOCOL_VERSION;
use native_tls::{Certificate, Identity, TlsConnector};
use sha2::{Digest as _, Sha256};
use std::collections::BTreeMap;
use std::fmt::Debug;
use std::fs;
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::thread;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

const NETWORK_ID: u32 = 7446;
const NETWORK_NAME: &str = "layerx-boundary-test";
const FIRST_BATCH: u64 = 1;
const LAST_BATCH: u64 = 1_000_000;
const LNI_FRAME_BYTES: usize = 1_212_416;
const SEND_ACTIVITY: u16 = 5;
const MODULE_GOVERNANCE: u16 = 7;
const METERING_AUTHORITY_GENESIS: u8 = 1;
const BOUNDARY_UID: u32 = 65534;
const BOUNDARY_GID: u32 = 65534;

fn must<T, E: Debug>(result: Result<T, E>, what: &str) -> T {
    result.unwrap_or_else(|error| panic!("{what}: {error:?}"))
}

fn random32() -> [u8; 32] {
    let mut bytes = [0_u8; 32];
    must(getrandom::fill(&mut bytes), "random bytes");
    if bytes == [0; 32] {
        bytes[0] = 1;
    }
    bytes
}

fn now_ms() -> u64 {
    let elapsed = must(SystemTime::now().duration_since(UNIX_EPOCH), "clock");
    u64::try_from(elapsed.as_millis()).unwrap_or(u64::MAX)
}

fn hex(bytes: &[u8]) -> String {
    const DIGITS: &[u8; 16] = b"0123456789abcdef";
    let mut encoded = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        encoded.push(char::from(DIGITS[usize::from(byte >> 4)]));
        encoded.push(char::from(DIGITS[usize::from(byte & 15)]));
    }
    encoded
}

fn unhex(text: &str) -> Vec<u8> {
    assert!(
        text.len().is_multiple_of(2),
        "hex text {text} has odd length"
    );
    (0..text.len())
        .step_by(2)
        .map(|index| must(u8::from_str_radix(&text[index..index + 2], 16), "hex digit"))
        .collect()
}

fn repository_root() -> PathBuf {
    let manifest = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    manifest.ancestors().nth(3).map_or_else(
        || panic!("repository root above {}", manifest.display()),
        Path::to_path_buf,
    )
}

fn free_port() -> u16 {
    let listener = must(TcpListener::bind("127.0.0.1:0"), "port allocation");
    must(listener.local_addr(), "port address").port()
}

fn write(path: &Path, bytes: &[u8], mode: u32) {
    must(fs::write(path, bytes), &format!("write {}", path.display()));
    must(
        fs::set_permissions(path, fs::Permissions::from_mode(mode)),
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

fn chown(path: &Path, uid: u32, gid: u32) {
    must(
        std::os::unix::fs::chown(path, Some(uid), Some(gid)),
        &format!("chown {}", path.display()),
    );
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

fn token() -> String {
    hex(&random32())
}

fn text(path: &Path) -> String {
    path.to_string_lossy().into_owned()
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

struct Genesis {
    directory: PathBuf,
    asset: [u8; 32],
    receipt_state_root: [u8; 32],
}

fn genesis_request(asset: &[u8; 32], guarantor_key: &[u8; 33]) -> Vec<u8> {
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
    let mut guarantor_id = [0_u8; 32];
    guarantor_id[0] = 1;
    request.extend_from_slice(&guarantor_id);
    request.extend_from_slice(guarantor_key);
    request.extend_from_slice(&[0_u8; 16]);
    request.extend_from_slice(asset);
    request.extend_from_slice(&1_u32.to_be_bytes());
    for value in [1_u64, 1, 1, 1, 1, 8, 8, 64, 8] {
        request.extend_from_slice(&value.to_be_bytes());
    }
    request.extend_from_slice(&1_u64.to_be_bytes());
    request.push(METERING_AUTHORITY_GENESIS);
    request.extend_from_slice(&1_u32.to_be_bytes());
    for value in [1_u64, 1, 2, 4, 1, 1, 100] {
        request.extend_from_slice(&value.to_be_bytes());
    }
    for value in [100_u64, 1, 1, 10, 1, 1000] {
        request.extend_from_slice(&value.to_be_bytes());
    }
    request
}

fn genesis_guarantor_key(directory: &Path) -> [u8; 33] {
    let private = directory.join("guarantor-key.pem");
    let public = directory.join("guarantor-public.der");
    write(&private, &[], 0o600);
    command(
        "openssl",
        &[
            "genpkey",
            "-algorithm",
            "EC",
            "-pkeyopt",
            "ec_paramgen_curve:secp256k1",
            "-out",
            &private.to_string_lossy(),
        ],
    );
    command(
        "openssl",
        &[
            "ec",
            "-in",
            &private.to_string_lossy(),
            "-pubout",
            "-conv_form",
            "compressed",
            "-outform",
            "DER",
            "-out",
            &public.to_string_lossy(),
        ],
    );
    let encoded = must(fs::read(&public), "guarantor public key");
    let compressed = encoded
        .get(encoded.len().saturating_sub(33)..)
        .unwrap_or_else(|| panic!("compressed guarantor public key missing"));
    must(
        compressed.try_into(),
        "compressed guarantor public key length",
    )
}

fn build_genesis(root: &Path, builder: &Path) -> Genesis {
    let directory = root.join("genesis");
    make_dir(&directory, 0o755);
    let asset = random32();
    let guarantor_key = genesis_guarantor_key(&directory);
    write(
        &directory.join("request.lxgb"),
        &genesis_request(&asset, &guarantor_key),
        0o600,
    );
    write(&directory.join("signer.key"), &random32(), 0o600);
    let artifacts = directory.join("artifacts");
    command(
        &text(builder),
        &[
            &text(&directory.join("request.lxgb")),
            &text(&directory.join("signer.key")),
            &text(&artifacts),
        ],
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
        "role={role}\nnetwork_id={NETWORK_ID}\nstart_sequence=0\nverify_workers=0\nnetwork_workers=0\nprojection_workers=0\ncheckpoint_workers=0\nserial_execution=true\n"
    )
}

fn spawn_daemon(
    layerxd: &Path,
    mode: &str,
    config: &Path,
    environment: &BTreeMap<&str, String>,
    stderr: PathBuf,
) -> Daemon {
    let mut command = Command::new(layerxd);
    command
        .arg(mode)
        .arg(config)
        .env_clear()
        .env("PATH", "/usr/bin:/bin")
        .envs(environment.iter().map(|(key, value)| (key, value.as_str())))
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::from(must(
            fs::File::create(&stderr),
            "daemon stderr file",
        )));
    let child = must(command.spawn(), &format!("spawn {mode}"));
    Daemon { child, stderr }
}

fn wait_for_port(port: u16, daemon: &mut Daemon, what: &str) {
    let deadline = Instant::now() + Duration::from_secs(60);
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

fn wait_for_socket(socket: &Path, daemon: &mut Daemon) {
    let deadline = Instant::now() + Duration::from_secs(60);
    loop {
        if socket.exists() {
            return;
        }
        if let Ok(Some(status)) = daemon.child.try_wait() {
            panic!(
                "sequencer exited early with {status}: {}",
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

fn domain_hash(domain: Domain, bytes: &[u8]) -> [u8; 32] {
    let mut digest = Sha256::new();
    digest.update(domain.tag());
    digest.update(bytes);
    digest.finalize().into()
}

fn registry() -> ModuleRegistry {
    let activity = must(
        ActivityType::new(ModuleId::Asset, SEND_ACTIVITY),
        "activity type",
    );
    let registration = must(
        ModuleRegistration::new(ModuleId::Asset, &[activity]),
        "registration",
    );
    must(ModuleRegistry::new(&[registration]), "registry")
}

#[allow(clippy::too_many_arguments)]
fn send_payload(
    signing_key: &SigningKey,
    source: [u8; 32],
    destination: [u8; 32],
    asset: [u8; 32],
    amount: u128,
    sequence: u64,
    idempotency: [u8; 32],
    expires_at: u64,
    context: [u8; 32],
) -> Vec<u8> {
    let public_key = signing_key.verifying_key().to_bytes();
    let mut authorization = Encoder::new(512);
    must(
        authorization
            .u16(0x5301)
            .and_then(|()| authorization.fixed(&source))
            .and_then(|()| authorization.fixed(&destination))
            .and_then(|()| authorization.fixed(&asset))
            .and_then(|()| authorization.u128(amount))
            .and_then(|()| authorization.u64(sequence))
            .and_then(|()| authorization.fixed(&idempotency))
            .and_then(|()| authorization.u64(expires_at))
            .and_then(|()| authorization.fixed(&context))
            .and_then(|()| authorization.u8(0))
            .and_then(|()| authorization.u8(1))
            .and_then(|()| authorization.fixed(&source))
            .and_then(|()| authorization.fixed(&context))
            .and_then(|()| authorization.u32(NETWORK_ID))
            .and_then(|()| authorization.u16(PROTOCOL_VERSION)),
        "authorization encoding",
    );
    let signature = signing_key
        .sign(&domain_hash(
            Domain::SignaturePreimage,
            &authorization.finish(),
        ))
        .to_bytes();
    let mut payload = Encoder::new(512);
    must(
        payload
            .u16(0x5301)
            .and_then(|()| payload.u16(10))
            .and_then(|()| payload.fixed(&source))
            .and_then(|()| payload.fixed(&destination))
            .and_then(|()| payload.fixed(&asset))
            .and_then(|()| payload.u128(amount))
            .and_then(|()| payload.u64(sequence))
            .and_then(|()| payload.fixed(&idempotency))
            .and_then(|()| payload.u64(expires_at))
            .and_then(|()| payload.fixed(&context))
            .and_then(|()| payload.u8(0))
            .and_then(|()| payload.u8(1))
            .and_then(|()| payload.fixed(&source))
            .and_then(|()| payload.fixed(&public_key))
            .and_then(|()| payload.fixed(&signature))
            .and_then(|()| payload.fixed(&context))
            .and_then(|()| payload.u32(NETWORK_ID))
            .and_then(|()| payload.u16(PROTOCOL_VERSION)),
        "payload encoding",
    );
    payload.finish()
}

struct Actor {
    signing_key: SigningKey,
    did: String,
    source: [u8; 32],
}

fn actor() -> Actor {
    let signing_key = SigningKey::from_bytes(&random32());
    let did = format!(
        "did:layerx:{}",
        hex(&signing_key.verifying_key().to_bytes())
    );
    let name = format!("agent:{did}:main");
    let length = must(u32::try_from(name.len()), "account name length");
    let mut digest = Sha256::new();
    digest.update(b"LX:ACCOUNT:v1");
    digest.update(length.to_be_bytes());
    digest.update(name.as_bytes());
    Actor {
        signing_key,
        did,
        source: digest.finalize().into(),
    }
}

fn signed_send(actor: &Actor, asset: [u8; 32], sequence: u64) -> Vec<u8> {
    let destination = random32();
    let idempotency = random32();
    let amount = 1_u128;
    let not_before = now_ms().saturating_sub(30_000);
    let expires_at = now_ms() + 120_000;
    let mut context_material = Vec::with_capacity(144);
    context_material.extend_from_slice(&actor.source);
    context_material.extend_from_slice(&destination);
    context_material.extend_from_slice(&asset);
    context_material.extend_from_slice(&amount.to_be_bytes());
    context_material.extend_from_slice(&idempotency);
    let context = domain_hash(Domain::ContextHash, &context_material);
    let payload_bytes = send_payload(
        &actor.signing_key,
        actor.source,
        destination,
        asset,
        amount,
        sequence,
        idempotency,
        expires_at,
        context,
    );
    let activity_type = must(
        ActivityType::new(ModuleId::Asset, SEND_ACTIVITY),
        "activity type",
    );
    let payload = must(
        Payload::new(&registry(), activity_type, &payload_bytes),
        "payload",
    );
    let payload_hash = domain_hash(Domain::PayloadHash, payload.as_bytes());
    let public_key = actor.signing_key.verifying_key().to_bytes();
    let mut builder = EnvelopeBuilder::new();
    must(
        builder
            .protocol_version(PROTOCOL_VERSION)
            .and_then(|value| value.network_id(NETWORK_ID))
            .and_then(|value| value.activity_type(activity_type))
            .and_then(|value| value.actor_did(must(Did::new(actor.did.as_bytes()), "did")))
            .and_then(|value| value.authority(must(Authority::owner(&public_key), "authority")))
            .and_then(|value| value.account_sequence(sequence))
            .and_then(|value| {
                value.timestamp_bound(must(
                    TimestampBound::new(not_before, expires_at),
                    "timestamp bound",
                ))
            })
            .and_then(|value| value.idempotency_key(IdempotencyKey::new(idempotency)))
            .and_then(|value| value.fee_limit(Amount::from_u128(0)))
            .and_then(|value| value.payload_hash(payload_hash))
            .and_then(|value| value.payload(payload))
            .map(|_| ()),
        "envelope",
    );
    let unsigned = must(builder.build(), "unsigned envelope");
    let unsigned_bytes = must(encode_unsigned_envelope(&unsigned), "unsigned bytes");
    let signature = actor
        .signing_key
        .sign(&domain_hash(Domain::SignaturePreimage, &unsigned_bytes))
        .to_bytes();
    let signed = unsigned.attach_signature(must(Signature::new(&signature), "signature"));
    must(encode_signed_envelope(&signed), "signed bytes")
}

fn expected_activity_id(signed: &[u8]) -> String {
    let decoded = must(decode_signed(signed, &registry()), "signed activity decode");
    hex(&must(activity_id(&decoded), "activity id"))
}

struct HttpAnswer {
    status: u16,
    headers: BTreeMap<String, String>,
    body: Vec<u8>,
}

impl HttpAnswer {
    fn content_type(&self) -> &str {
        self.headers.get("content-type").map_or("", String::as_str)
    }

    fn json(&self) -> serde_json::Value {
        must(serde_json::from_slice(&self.body), "JSON body")
    }

    fn error_code(&self) -> String {
        field(&self.json()["error"], "code").to_owned()
    }

    fn text(&self) -> String {
        String::from_utf8_lossy(&self.body).into_owned()
    }
}

fn field<'a>(value: &'a serde_json::Value, name: &str) -> &'a str {
    value[name]
        .as_str()
        .unwrap_or_else(|| panic!("field {name} missing in {value}"))
}

fn parse_http(raw: &[u8]) -> HttpAnswer {
    let end = raw
        .windows(4)
        .position(|window| window == b"\r\n\r\n")
        .unwrap_or_else(|| panic!("HTTP response without header terminator"));
    let head = String::from_utf8_lossy(&raw[..end]).into_owned();
    let mut lines = head.split("\r\n");
    let start = lines
        .next()
        .unwrap_or_else(|| panic!("HTTP status line missing in {head}"));
    assert!(start.starts_with("HTTP/1.1 "), "status line {start}");
    let status = start
        .split_whitespace()
        .nth(1)
        .and_then(|code| code.parse::<u16>().ok())
        .unwrap_or_else(|| panic!("HTTP status missing in {head}"));
    let mut headers = BTreeMap::new();
    for line in lines {
        let (name, value) = line
            .split_once(':')
            .unwrap_or_else(|| panic!("malformed header {line}"));
        let previous = headers.insert(name.trim().to_ascii_lowercase(), value.trim().to_owned());
        assert!(previous.is_none(), "duplicate header {name}");
    }
    let body = raw[end + 4..].to_vec();
    assert_eq!(
        headers
            .get("content-length")
            .and_then(|value| value.parse::<usize>().ok()),
        Some(body.len()),
        "content length mismatch"
    );
    HttpAnswer {
        status,
        headers,
        body,
    }
}

struct Client {
    port: u16,
    certificate: Certificate,
}

struct Call<'a> {
    method: &'a str,
    path: &'a str,
    bearer: Option<&'a str>,
    idempotency: Option<&'a str>,
    content_type: Option<&'a str>,
    body: &'a [u8],
    identity: Option<&'a Identity>,
}

impl<'a> Call<'a> {
    fn get(path: &'a str, bearer: Option<&'a str>) -> Self {
        Self {
            method: "GET",
            path,
            bearer,
            idempotency: None,
            content_type: None,
            body: &[],
            identity: None,
        }
    }

    fn submit(path: &'a str, bearer: &'a str, key: &'a str, body: &'a [u8]) -> Self {
        Self {
            method: "POST",
            path,
            bearer: Some(bearer),
            idempotency: Some(key),
            content_type: Some("application/octet-stream"),
            body,
            identity: None,
        }
    }
}

impl Client {
    fn try_call(&self, call: &Call<'_>) -> Result<HttpAnswer, String> {
        let mut builder = TlsConnector::builder();
        builder.add_root_certificate(self.certificate.clone());
        if let Some(identity) = call.identity {
            builder.identity(identity.clone());
        }
        let connector = must(builder.build(), "TLS connector");
        let tcp =
            TcpStream::connect(("127.0.0.1", self.port)).map_err(|error| error.to_string())?;
        must(
            tcp.set_read_timeout(Some(Duration::from_secs(60))),
            "read timeout",
        );
        let mut stream = connector
            .connect("localhost", tcp)
            .map_err(|error| error.to_string())?;
        let authorization = call.bearer.map_or(String::new(), |token| {
            format!("Authorization: Bearer {token}\r\n")
        });
        let idempotency = call
            .idempotency
            .map_or(String::new(), |key| format!("Idempotency-Key: {key}\r\n"));
        let content_type = call
            .content_type
            .map_or(String::new(), |value| format!("Content-Type: {value}\r\n"));
        let request = format!(
            "{} {} HTTP/1.1\r\nHost: localhost\r\n{authorization}Accept: application/json\r\n{content_type}{idempotency}Content-Length: {}\r\nConnection: close\r\n\r\n",
            call.method,
            call.path,
            call.body.len()
        );
        stream
            .write_all(request.as_bytes())
            .and_then(|()| stream.write_all(call.body))
            .and_then(|()| stream.flush())
            .map_err(|error| error.to_string())?;
        let mut raw = Vec::new();
        let _ = stream.read_to_end(&mut raw);
        if raw.is_empty() {
            return Err("TLS peer closed without an HTTP response".to_owned());
        }
        Ok(parse_http(&raw))
    }

    fn call(&self, call: &Call<'_>) -> HttpAnswer {
        let answer = must(self.try_call(call), "boundary request");
        assert_eq!(
            answer.content_type(),
            "application/json",
            "{} {} answered {}",
            call.method,
            call.path,
            answer.text()
        );
        assert_eq!(
            answer.headers.get("cache-control").map(String::as_str),
            Some("no-store")
        );
        assert_eq!(
            answer.headers.get("connection").map(String::as_str),
            Some("close")
        );
        assert!(!answer.headers.contains_key("transfer-encoding"));
        answer
    }

    fn get(&self, path: &str, bearer: Option<&str>) -> HttpAnswer {
        self.call(&Call::get(path, bearer))
    }
}

fn http_get(port: u16, path: &str, bearer: &str) -> HttpAnswer {
    let mut stream = must(TcpStream::connect(("127.0.0.1", port)), "daemon connect");
    must(
        stream.set_read_timeout(Some(Duration::from_secs(30))),
        "read timeout",
    );
    must(
        stream.write_all(
            format!("GET {path} HTTP/1.1\r\nHost: 127.0.0.1\r\nAuthorization: Bearer {bearer}\r\nConnection: close\r\n\r\n")
                .as_bytes(),
        ),
        "request write",
    );
    let mut raw = Vec::new();
    let _ = stream.read_to_end(&mut raw);
    parse_http(&raw)
}

struct TlsMaterial {
    server_der: PathBuf,
    server_key_der: PathBuf,
    server_certificate: Certificate,
    client_ca_der: PathBuf,
    client_identity: Identity,
    rogue_identity: Identity,
}

fn issue_ca(directory: &Path, name: &str) -> (PathBuf, PathBuf) {
    let key = directory.join(format!("{name}-key.pem"));
    let certificate = directory.join(format!("{name}.pem"));
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
            &text(&key),
            "-out",
            &text(&certificate),
            "-days",
            "1",
            "-subj",
            &format!("/O=LayerX boundary test/CN={name}"),
            "-addext",
            "basicConstraints=critical,CA:TRUE",
            "-addext",
            "keyUsage=critical,keyCertSign,cRLSign",
        ],
    );
    (certificate, key)
}

fn issue_client(directory: &Path, name: &str, ca: &(PathBuf, PathBuf)) -> Identity {
    let key = directory.join(format!("{name}-key.pem"));
    let request = directory.join(format!("{name}.csr"));
    let certificate = directory.join(format!("{name}.pem"));
    let extensions = directory.join(format!("{name}.ext"));
    write(
        &extensions,
        b"basicConstraints=CA:FALSE\nkeyUsage=digitalSignature\nextendedKeyUsage=clientAuth\n",
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
            &text(&key),
            "-out",
            &text(&request),
            "-subj",
            &format!("/O=LayerX boundary test/CN={name}"),
        ],
    );
    command(
        "openssl",
        &[
            "x509",
            "-req",
            "-in",
            &text(&request),
            "-CA",
            &text(&ca.0),
            "-CAkey",
            &text(&ca.1),
            "-CAcreateserial",
            "-days",
            "1",
            "-extfile",
            &text(&extensions),
            "-out",
            &text(&certificate),
        ],
    );
    must(
        Identity::from_pkcs8(
            &must(fs::read(&certificate), "client certificate"),
            &must(fs::read(&key), "client key"),
        ),
        "client identity",
    )
}

fn tls_material(root: &Path) -> TlsMaterial {
    let directory = root.join("tls");
    make_dir(&directory, 0o755);
    let server_pem = directory.join("server.pem");
    let server_der = directory.join("server.der");
    let server_key_pem = directory.join("server-key.pem");
    let server_key_der = directory.join("server-key.der");
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
            &text(&server_key_pem),
            "-out",
            &text(&server_pem),
            "-days",
            "1",
            "-subj",
            "/CN=localhost",
            "-addext",
            "subjectAltName=DNS:localhost",
        ],
    );
    command(
        "openssl",
        &[
            "x509",
            "-in",
            &text(&server_pem),
            "-outform",
            "DER",
            "-out",
            &text(&server_der),
        ],
    );
    command(
        "openssl",
        &[
            "pkcs8",
            "-topk8",
            "-nocrypt",
            "-in",
            &text(&server_key_pem),
            "-outform",
            "DER",
            "-out",
            &text(&server_key_der),
        ],
    );
    let client_ca = issue_ca(&directory, "client-ca");
    let client_ca_der = directory.join("client-ca.der");
    command(
        "openssl",
        &[
            "x509",
            "-in",
            &text(&client_ca.0),
            "-outform",
            "DER",
            "-out",
            &text(&client_ca_der),
        ],
    );
    let rogue_ca = issue_ca(&directory, "rogue-ca");
    let client_identity = issue_client(&directory, "gateway-client", &client_ca);
    let rogue_identity = issue_client(&directory, "rogue-client", &rogue_ca);
    for path in [&server_der, &server_key_der, &client_ca_der] {
        chown(path, BOUNDARY_UID, BOUNDARY_GID);
        must(
            fs::set_permissions(path, fs::Permissions::from_mode(0o600)),
            "tls permissions",
        );
    }
    TlsMaterial {
        server_certificate: must(
            Certificate::from_der(&must(fs::read(&server_der), "certificate")),
            "certificate parse",
        ),
        server_der,
        server_key_der,
        client_ca_der,
        client_identity,
        rogue_identity,
    }
}

struct Boundary {
    binary: PathBuf,
    environment: BTreeMap<&'static str, String>,
    stderr: PathBuf,
    process: Option<Daemon>,
    port: u16,
}

impl Boundary {
    fn start(&mut self) {
        let mut command = Command::new("/usr/bin/setpriv");
        command
            .arg("--reuid")
            .arg(BOUNDARY_UID.to_string())
            .arg("--regid")
            .arg(BOUNDARY_GID.to_string())
            .arg("--groups")
            .arg(BOUNDARY_GID.to_string())
            .arg("--")
            .arg(&self.binary)
            .env_clear()
            .envs(
                self.environment
                    .iter()
                    .map(|(key, value)| (*key, value.as_str())),
            )
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::from(must(
                fs::OpenOptions::new()
                    .create(true)
                    .append(true)
                    .open(&self.stderr),
                "boundary stderr file",
            )));
        let mut daemon = Daemon {
            child: must(command.spawn(), "spawn agent boundary"),
            stderr: self.stderr.clone(),
        };
        wait_for_port(self.port, &mut daemon, "agent boundary");
        self.process = Some(daemon);
    }

    fn stop(&mut self) {
        if let Some(mut process) = self.process.take() {
            process.stop();
        }
    }

    fn diagnostics(&self) -> String {
        fs::read_to_string(&self.stderr).unwrap_or_default()
    }
}

struct Cluster {
    root: PathBuf,
    sequencer: Daemon,
    replica: Daemon,
    boundary: Boundary,
    client: Client,
    program_port: u16,
    program_token: String,
    gateway_token: String,
    registry_token: String,
    state_dir: PathBuf,
    asset: [u8; 32],
    sequencer_key: [u8; 32],
    actor: Actor,
    tls: TlsMaterial,
}

struct NodeSetup {
    sequencer_seed: [u8; 32],
    sequencer_key: [u8; 32],
    sequencer_id: [u8; 32],
    replica_id: [u8; 32],
    replica_token: String,
    program_token: String,
    replica_port: u16,
    program_port: u16,
    boundary_port: u16,
}

fn start_replica(root: &Path, layerxd: &Path, setup: &NodeSetup) -> Daemon {
    let replica_dir = root.join("replica");
    make_dir(&replica_dir, 0o700);
    write(
        &replica_dir.join("config.txt"),
        node_config("replica").as_bytes(),
        0o600,
    );
    let mut replica_env = BTreeMap::new();
    replica_env.insert(
        "LAYERX_AUTHORITY_REPLICA_LOG",
        text(&replica_dir.join("replica.log")),
    );
    replica_env.insert("LAYERX_AUTHORITY_REPLICA_ID", hex(&setup.replica_id));
    replica_env.insert("LAYERX_AUTHORITY_SEQUENCER_ID", hex(&setup.sequencer_id));
    replica_env.insert(
        "LAYERX_AUTHORITY_SEQUENCER_PUBLIC_KEY",
        hex(&setup.sequencer_key),
    );
    replica_env.insert("LAYERX_AUTHORITY_FIRST_BATCH", FIRST_BATCH.to_string());
    replica_env.insert("LAYERX_AUTHORITY_LAST_BATCH", LAST_BATCH.to_string());
    replica_env.insert("LAYERX_AUTHORITY_BEARER_TOKEN", setup.replica_token.clone());
    replica_env.insert("LAYERX_AUTHORITY_ADDRESS", "127.0.0.1".to_owned());
    replica_env.insert("LAYERX_AUTHORITY_PORT", setup.replica_port.to_string());
    let mut replica = spawn_daemon(
        layerxd,
        "--authority-replica",
        &replica_dir.join("config.txt"),
        &replica_env,
        root.join("replica.stderr"),
    );
    wait_for_port(setup.replica_port, &mut replica, "authority replica");

    replica
}

fn sequencer_environment(
    repository: &Path,
    genesis: &Genesis,
    node_dir: &Path,
    socket: &Path,
    setup: &NodeSetup,
) -> BTreeMap<&'static str, String> {
    let checkpoints = node_dir.join("checkpoints");
    let logs = node_dir.join("logs");
    let mut node_env = BTreeMap::new();
    node_env.insert("LAYERX_NODE_PAXEER_CHAIN_ID", "31337".to_owned());
    node_env.insert("LAYERX_NODE_PAXEER_RPC_ADDRESS", "127.0.0.1".to_owned());
    node_env.insert("LAYERX_NODE_PAXEER_RPC_PORT", free_port().to_string());
    node_env.insert(
        "LAYERX_NODE_SETTLEMENT_CONTRACT",
        format!("0x{}", "11".repeat(20)),
    );
    node_env.insert(
        "LAYERX_NODE_CHECKPOINT_REGISTRY",
        format!("0x{}", "22".repeat(20)),
    );
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
    node_env.insert("LAYERX_NODE_PROGRAM_FEED_LOG", text(&logs.join("feed.log")));
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
        text(&node_dir.join("history.db")),
    );
    node_env.insert(
        "LAYERX_NODE_HISTORY_MIGRATIONS",
        text(&repository.join("migrations/0007_history_index.sql")),
    );
    node_env.insert("LAYERX_NODE_SEQUENCER_ID", hex(&setup.sequencer_id));
    node_env.insert(
        "LAYERX_NODE_SEQUENCER_PUBLIC_KEY",
        hex(&setup.sequencer_key),
    );
    node_env.insert(
        "LAYERX_NODE_SEQUENCER_PRIVATE_KEY",
        hex(&setup.sequencer_seed),
    );
    node_env.insert("LAYERX_NODE_FIRST_BATCH", FIRST_BATCH.to_string());
    node_env.insert("LAYERX_NODE_LAST_BATCH", LAST_BATCH.to_string());
    node_env.insert(
        "LAYERX_NODE_AUTHORITY_REPLICA_ADDRESS",
        "127.0.0.1".to_owned(),
    );
    node_env.insert(
        "LAYERX_NODE_AUTHORITY_REPLICA_PORT",
        setup.replica_port.to_string(),
    );
    node_env.insert("LAYERX_NODE_AUTHORITY_REPLICA_ID", hex(&setup.replica_id));
    node_env.insert(
        "LAYERX_NODE_AUTHORITY_REPLICA_BEARER_TOKEN",
        setup.replica_token.clone(),
    );
    node_env.insert(
        "LAYERX_NODE_PROGRAM_BEARER_TOKEN",
        setup.program_token.clone(),
    );
    node_env.insert("LAYERX_NODE_PROGRAM_ADDRESS", "127.0.0.1".to_owned());
    node_env.insert("LAYERX_NODE_PROGRAM_PORT", setup.program_port.to_string());
    node_env.insert("LAYERX_NODE_LNI_SOCKET", text(socket));
    node_env.insert("LAYERX_NODE_LNI_ALLOWED_UID", BOUNDARY_UID.to_string());
    node_env.insert("LAYERX_NODE_LNI_ALLOWED_GID", BOUNDARY_GID.to_string());
    node_env.insert("LAYERX_NODE_LNI_FRAME_BYTES", LNI_FRAME_BYTES.to_string());
    node_env.insert("LAYERX_NODE_LNI_DEADLINE_MS", "10000".to_owned());
    node_env
}

fn start_sequencer(
    root: &Path,
    layerxd: &Path,
    repository: &Path,
    genesis: &Genesis,
    setup: &NodeSetup,
) -> (Daemon, Actor, PathBuf) {
    let node_dir = root.join("node");
    let checkpoints = node_dir.join("checkpoints");
    let logs = node_dir.join("logs");
    let run_dir = root.join("run");
    make_dir(&node_dir, 0o700);
    make_dir(&checkpoints, 0o700);
    make_dir(&logs, 0o700);
    make_dir(&run_dir, 0o750);
    chown(&run_dir, 0, BOUNDARY_GID);
    write(
        &node_dir.join("registration.lxgr"),
        &registration(&genesis.receipt_state_root),
        0o600,
    );
    let actor = actor();
    let identities = format!(
        "{}:{}:1\n",
        hex(actor.did.as_bytes()),
        hex(&actor.signing_key.verifying_key().to_bytes())
    );
    write(
        &node_dir.join("identities.txt"),
        identities.as_bytes(),
        0o600,
    );
    write(
        &node_dir.join("config.txt"),
        node_config("sequencer").as_bytes(),
        0o600,
    );
    let socket = run_dir.join("layerxd.sock");
    let node_env = sequencer_environment(repository, genesis, &node_dir, &socket, setup);
    let mut sequencer = spawn_daemon(
        layerxd,
        "--serve",
        &node_dir.join("config.txt"),
        &node_env,
        root.join("sequencer.stderr"),
    );
    wait_for_socket(&socket, &mut sequencer);
    wait_for_port(setup.program_port, &mut sequencer, "program listener");

    (sequencer, actor, socket)
}

struct BoundarySetup {
    boundary: Boundary,
    client: Client,
    gateway_token: String,
    registry_token: String,
    state_dir: PathBuf,
    tls: TlsMaterial,
}

fn start_boundary(root: &Path, socket: &Path, setup: &NodeSetup) -> BoundarySetup {
    let tls = tls_material(root);
    let tokens_dir = root.join("tokens");
    make_dir(&tokens_dir, 0o755);
    let gateway_token = token();
    let registry_token = token();
    for (name, value) in [
        ("gateway.token", format!("{gateway_token}\n")),
        ("registry.token", registry_token.clone()),
        ("node.token", format!("{}\r\n", setup.program_token)),
    ] {
        let path = tokens_dir.join(name);
        write(&path, value.as_bytes(), 0o600);
        chown(&path, BOUNDARY_UID, BOUNDARY_GID);
    }
    let state_dir = root.join("state");
    make_dir(&state_dir, 0o700);
    chown(&state_dir, BOUNDARY_UID, BOUNDARY_GID);
    let bin_dir = root.join("bin");
    make_dir(&bin_dir, 0o755);
    let binary = bin_dir.join("layerx-agent-boundary");
    must(
        fs::copy(env!("CARGO_BIN_EXE_layerx-agent-boundary"), &binary),
        "copy boundary binary",
    );
    must(
        fs::set_permissions(&binary, fs::Permissions::from_mode(0o755)),
        "boundary binary permissions",
    );
    let mut environment = BTreeMap::new();
    environment.insert(
        "LAYERX_AGENT_BOUNDARY_LISTEN",
        format!("127.0.0.1:{}", setup.boundary_port),
    );
    environment.insert("LAYERX_AGENT_BOUNDARY_TLS_CERT_DER", text(&tls.server_der));
    environment.insert(
        "LAYERX_AGENT_BOUNDARY_TLS_KEY_DER",
        text(&tls.server_key_der),
    );
    environment.insert(
        "LAYERX_AGENT_BOUNDARY_CLIENT_CA_DER",
        text(&tls.client_ca_der),
    );
    environment.insert(
        "LAYERX_AGENT_BOUNDARY_GATEWAY_TOKEN_FILE",
        text(&tokens_dir.join("gateway.token")),
    );
    environment.insert(
        "LAYERX_AGENT_BOUNDARY_REGISTRY_TOKEN_FILE",
        text(&tokens_dir.join("registry.token")),
    );
    environment.insert("LAYERX_AGENT_BOUNDARY_LNI_SOCKET", text(socket));
    environment.insert(
        "LAYERX_AGENT_BOUNDARY_NODE_URL",
        format!("http://127.0.0.1:{}", setup.program_port),
    );
    environment.insert(
        "LAYERX_AGENT_BOUNDARY_NODE_BEARER_TOKEN_FILE",
        text(&tokens_dir.join("node.token")),
    );
    environment.insert("LAYERX_AGENT_BOUNDARY_STATE_DIR", text(&state_dir));
    environment.insert(
        "LAYERX_AGENT_BOUNDARY_PROTOCOL_NETWORK_ID",
        NETWORK_ID.to_string(),
    );
    environment.insert("LAYERX_AGENT_BOUNDARY_NETWORK_ID", NETWORK_NAME.to_owned());
    environment.insert("LAYERX_AGENT_BOUNDARY_RECEIPT_WAIT_MS", "15000".to_owned());
    let mut boundary = Boundary {
        binary,
        environment,
        stderr: root.join("boundary.stderr"),
        process: None,
        port: setup.boundary_port,
    };
    boundary.start();
    let client = Client {
        port: setup.boundary_port,
        certificate: tls.server_certificate.clone(),
    };
    BoundarySetup {
        boundary,
        client,
        gateway_token,
        registry_token,
        state_dir,
        tls,
    }
}

fn start_cluster() -> Cluster {
    assert_eq!(
        effective_uid(),
        0,
        "the real-node harness must run as root so the boundary can run under uid {BOUNDARY_UID}"
    );
    let repository = repository_root();
    let layerxd = repository.join("build/bin/layerxd");
    let builder = repository.join("build/bin/layerx-genesis-build");
    assert!(layerxd.is_file(), "{} is not built", layerxd.display());
    assert!(builder.is_file(), "{} is not built", builder.display());
    let root = std::env::temp_dir().join(format!(
        "layerx-agent-boundary-{}-{}-{}",
        std::process::id(),
        now_ms(),
        hex(&random32()[..8])
    ));
    make_dir(&root, 0o755);
    let genesis = build_genesis(&root, &builder);

    let sequencer_seed = random32();
    let sequencer_signing = SigningKey::from_bytes(&sequencer_seed);
    let sequencer_key = sequencer_signing.verifying_key().to_bytes();
    let setup = NodeSetup {
        sequencer_seed,
        sequencer_key,
        sequencer_id: random32(),
        replica_id: random32(),
        replica_token: token(),
        program_token: token(),
        replica_port: free_port(),
        program_port: free_port(),
        boundary_port: free_port(),
    };
    let replica = start_replica(&root, &layerxd, &setup);
    let (sequencer, actor, socket) =
        start_sequencer(&root, &layerxd, &repository, &genesis, &setup);
    let boundary = start_boundary(&root, &socket, &setup);
    Cluster {
        root,
        sequencer,
        replica,
        boundary: boundary.boundary,
        client: boundary.client,
        program_port: setup.program_port,
        program_token: setup.program_token,
        gateway_token: boundary.gateway_token,
        registry_token: boundary.registry_token,
        state_dir: boundary.state_dir,
        asset: genesis.asset,
        sequencer_key: setup.sequencer_key,
        actor,
        tls: boundary.tls,
    }
}

impl Drop for Cluster {
    fn drop(&mut self) {
        self.boundary.stop();
        self.sequencer.stop();
        self.replica.stop();
        if std::thread::panicking() {
            eprintln!("sequencer stderr:\n{}", self.sequencer.diagnostics());
            eprintln!("replica stderr:\n{}", self.replica.diagnostics());
            eprintln!("boundary stderr:\n{}", self.boundary.diagnostics());
        } else {
            let _ = fs::remove_dir_all(&self.root);
        }
    }
}

fn journal_record(cluster: &Cluster, key: &str) -> serde_json::Value {
    let digest = format!("{:x}", Sha256::digest(key.as_bytes()));
    let path = cluster
        .state_dir
        .join("journal")
        .join(format!("{digest}.json"));
    must(
        serde_json::from_slice(&must(fs::read(&path), "journal record")),
        "journal JSON",
    )
}

fn assert_refusal(answer: &HttpAnswer, status: u16, code: &str) {
    assert_eq!(answer.status, status, "{}", answer.text());
    assert_eq!(answer.error_code(), code, "{}", answer.text());
    let error = &answer.json()["error"];
    let retry = field(error, "retry");
    assert!(retry == "never" || retry == "after");
    assert_eq!(
        retry == "after",
        error.get("retry_after_seconds").is_some(),
        "{}",
        answer.text()
    );
}

fn wait_ready(cluster: &Cluster) -> HttpAnswer {
    let deadline = Instant::now() + Duration::from_secs(60);
    loop {
        let ready = cluster.client.get("/readyz", None);
        if ready.status == 200 || Instant::now() >= deadline {
            return ready;
        }
        thread::sleep(Duration::from_millis(200));
    }
}

fn check_readiness(cluster: &Cluster) {
    let live = cluster.client.get("/livez", None);
    assert_eq!(live.status, 200);
    assert_eq!(
        live.text(),
        "{\"status\":\"live\",\"service\":\"agent-boundary\"}"
    );
    let ready = wait_ready(cluster);
    assert_eq!(ready.status, 200, "{}", ready.text());
    assert_eq!(
        ready.text(),
        format!(
            "{{\"ready\":true,\"network_id\":\"{NETWORK_NAME}\",\"wire_version\":\"{PROTOCOL_VERSION}\",\"synchronous_receipts\":true,\"state_snapshot\":true}}"
        )
    );
    let with_bearer = cluster.client.get("/readyz", Some(&cluster.gateway_token));
    assert_eq!(with_bearer.text(), ready.text());
}

fn check_entitlements(cluster: &Cluster, signed: &[u8]) {
    let client = &cluster.client;
    let anonymous = client.call(&Call {
        method: "POST",
        path: "/v1/activities",
        bearer: None,
        idempotency: Some("anonymous-key"),
        content_type: Some("application/octet-stream"),
        body: signed,
        identity: None,
    });
    assert_refusal(&anonymous, 401, "identity_required");
    let stranger = client.call(&Call::submit(
        "/v1/activities",
        &token(),
        "stranger-key",
        signed,
    ));
    assert_refusal(&stranger, 401, "identity_required");
    let registry_submit = client.call(&Call::submit(
        "/v1/activities",
        &cluster.registry_token,
        "registry-key",
        signed,
    ));
    assert_refusal(&registry_submit, 403, "entitlement_denied");
    let registry_receipt = client.get(
        &format!("/v1/receipts/{}", "0".repeat(64)),
        Some(&cluster.registry_token),
    );
    assert_refusal(&registry_receipt, 403, "entitlement_denied");
    let gateway_relay = client.get(
        "/v1/protocol/account-state/head",
        Some(&cluster.gateway_token),
    );
    assert_refusal(&gateway_relay, 403, "entitlement_denied");
    let gateway_internal = client.get(
        &format!("/internal/v1/receipts/{}", "0".repeat(64)),
        Some(&cluster.gateway_token),
    );
    assert_refusal(&gateway_internal, 403, "entitlement_denied");
    let anonymous_relay = client.get("/v1/protocol/account-state/head", None);
    assert_refusal(&anonymous_relay, 401, "identity_required");
    let unknown_route = client.get("/v1/unknown", Some(&cluster.gateway_token));
    assert_refusal(&unknown_route, 404, "not_found");
}

struct Submitted {
    key: String,
    activity_id: String,
    receipt: Vec<u8>,
    body: String,
}

fn submit_send(cluster: &Cluster, signed: &[u8], key: &str) -> Submitted {
    let expected = expected_activity_id(signed);
    let answer = cluster.client.call(&Call::submit(
        "/v1/activities",
        &cluster.gateway_token,
        key,
        signed,
    ));
    assert_eq!(answer.status, 200, "{}", answer.text());
    let document = answer.json();
    let result = &document["result"];
    let keys: Vec<&String> = result
        .as_object()
        .unwrap_or_else(|| panic!("result must be an object: {document}"))
        .keys()
        .collect();
    assert_eq!(
        keys,
        [
            "activity_id",
            "call_graph",
            "receipt",
            "state",
            "terminal_payload"
        ]
        .iter()
        .map(|key| (*key).to_owned())
        .collect::<Vec<String>>()
        .iter()
        .collect::<Vec<&String>>()
    );
    assert_eq!(field(result, "activity_id"), expected);
    let receipt = unhex(field(result, "receipt"));
    let verified = must(
        verify_sequencer_signature(&receipt, cluster.sequencer_key),
        "receipt sequencer signature",
    );
    let protocol = verified
        .protocol()
        .unwrap_or_else(|| panic!("protocol receipt"));
    assert_eq!(hex(&protocol.activity_id()), expected);
    let state = field(result, "state");
    if protocol.result_code() == 0 {
        assert_eq!(state, "completed");
    } else {
        assert_eq!(state, "refused");
    }
    assert_eq!(field(result, "terminal_payload"), "");
    assert_eq!(field(result, "call_graph"), "");
    Submitted {
        key: key.to_owned(),
        activity_id: expected,
        receipt,
        body: answer.text(),
    }
}

fn check_receipt_routes(cluster: &Cluster, submitted: &Submitted) {
    let client = &cluster.client;
    let receipt = client.get(
        &format!("/v1/receipts/{}", submitted.activity_id),
        Some(&cluster.gateway_token),
    );
    assert_eq!(receipt.status, 200, "{}", receipt.text());
    assert_eq!(
        receipt.text(),
        format!(
            "{{\"activity_id\":\"{}\",\"receipt\":\"{}\"}}",
            submitted.activity_id,
            hex(&submitted.receipt)
        )
    );
    let uppercase = client.get(
        &format!(
            "/v1/receipts/{}",
            submitted.activity_id.to_ascii_uppercase()
        ),
        Some(&cluster.gateway_token),
    );
    assert_eq!(uppercase.text(), receipt.text());
    let internal = client.get(
        &format!("/internal/v1/receipts/{}", submitted.activity_id),
        Some(&cluster.registry_token),
    );
    assert_eq!(internal.status, 200, "{}", internal.text());
    assert_eq!(internal.text(), receipt.text());
    let unknown = client.get(
        &format!("/v1/receipts/{}", hex(&random32())),
        Some(&cluster.gateway_token),
    );
    assert_refusal(&unknown, 404, "receipt_not_found");
    let internal_unknown = client.get(
        &format!("/internal/v1/receipts/{}", hex(&random32())),
        Some(&cluster.registry_token),
    );
    assert_refusal(&internal_unknown, 404, "receipt_not_found");
    let malformed = client.get("/v1/receipts/not-hex", Some(&cluster.gateway_token));
    assert_refusal(&malformed, 400, "invalid_activity_id");
}

fn check_program_routes(cluster: &Cluster, submitted: &Submitted, signed: &[u8]) {
    let client = &cluster.client;
    let not_program = client.get(
        &format!("/v1/programs/activities/{}", submitted.activity_id),
        Some(&cluster.gateway_token),
    );
    assert_refusal(&not_program, 400, "not_program_call");
    let unknown = client.get(
        &format!("/v1/programs/activities/{}", hex(&random32())),
        Some(&cluster.gateway_token),
    );
    assert_refusal(&unknown, 404, "activity_not_journaled");
    let call = client.call(&Call::submit(
        "/v1/programs/call",
        &cluster.gateway_token,
        "program-call-key",
        signed,
    ));
    assert_refusal(&call, 400, "not_program_call");
    let simulate = client.call(&Call {
        method: "POST",
        path: "/v1/programs/simulate",
        bearer: Some(&cluster.gateway_token),
        idempotency: None,
        content_type: Some("application/json"),
        body: b"{}",
        identity: None,
    });
    assert_refusal(&simulate, 503, "capability_unavailable");
}

fn check_refusals(cluster: &Cluster, valid: &[u8]) -> (String, String) {
    let client = &cluster.client;
    let stranger = actor();
    let unknown_did = signed_send(&stranger, cluster.asset, 1);
    let key = format!("unknown-did-{}", token());
    let refused = client.call(&Call::submit(
        "/v1/activities",
        &cluster.gateway_token,
        &key,
        &unknown_did,
    ));
    assert_refusal(&refused, 422, "unknown_did");
    let replay = client.call(&Call::submit(
        "/v1/activities",
        &cluster.gateway_token,
        &key,
        &unknown_did,
    ));
    assert_eq!(replay.status, refused.status);
    assert_eq!(replay.text(), refused.text());
    let record = journal_record(cluster, &key);
    assert_eq!(record["state"], "refused");
    assert_eq!(record["attempts"], 1);
    let mut tampered = valid.to_vec();
    let last = tampered.len() - 1;
    tampered[last] ^= 0x01;
    let bad_signature = client.call(&Call::submit(
        "/v1/activities",
        &cluster.gateway_token,
        &format!("tampered-{}", token()),
        &tampered,
    ));
    assert_refusal(&bad_signature, 422, "bad_signature");
    let garbage = client.call(&Call::submit(
        "/v1/activities",
        &cluster.gateway_token,
        &format!("garbage-{}", token()),
        b"not an activity",
    ));
    assert_refusal(&garbage, 400, "malformed_activity");
    let wrong_type = client.call(&Call {
        method: "POST",
        path: "/v1/activities",
        bearer: Some(&cluster.gateway_token),
        idempotency: Some("wrong-type"),
        content_type: Some("application/json"),
        body: valid,
        identity: None,
    });
    assert_refusal(&wrong_type, 400, "content_type_required");
    let no_key = client.call(&Call {
        method: "POST",
        path: "/v1/activities",
        bearer: Some(&cluster.gateway_token),
        idempotency: None,
        content_type: Some("application/octet-stream"),
        body: valid,
        identity: None,
    });
    assert_refusal(&no_key, 400, "idempotency_key_required");
    let bad_key = client.call(&Call::submit(
        "/v1/activities",
        &cluster.gateway_token,
        "bad key!",
        valid,
    ));
    assert_refusal(&bad_key, 400, "invalid_idempotency_key");
    (key, refused.text())
}

fn check_relays(cluster: &Cluster, submitted: &Submitted) {
    let client = &cluster.client;
    let paths = [
        "/v1/protocol/account-state/head".to_owned(),
        format!("/v1/receipts/{}/account-state", submitted.activity_id),
        "/v1/programs/account-state/changes?after_sequence=0".to_owned(),
        format!("/v1/programs/{}/account-state?at=1", hex(&random32())),
        format!(
            "/v1/batches/{}/receipt-authority?receipt_digest={}",
            hex(&random32()),
            hex(&random32())
        ),
    ];
    for path in &paths {
        let relayed = client.get(path, Some(&cluster.registry_token));
        let direct = http_get(cluster.program_port, path, &cluster.program_token);
        assert_eq!(relayed.status, direct.status, "{path}: {}", relayed.text());
        assert_eq!(
            relayed.body, direct.body,
            "{path}: relay must not alter the daemon document"
        );
    }
    let head = client.get(&paths[0], Some(&cluster.registry_token));
    assert_eq!(head.status, 200, "{}", head.text());
    for path in [
        "/v1/programs/account-state/changes",
        "/v1/programs/account-state/changes?after_sequence=x",
        "/v1/receipts/abc/account-state",
        "/v1/batches/abc/receipt-authority?receipt_digest=abc",
        "/v1/protocol/account-state/head?x=1",
    ] {
        let refused = client.get(path, Some(&cluster.registry_token));
        assert_refusal(&refused, 404, "not_found");
    }
    let wrong_token = client.get(&paths[0], Some(&token()));
    assert_refusal(&wrong_token, 401, "identity_required");
}

fn check_client_certificates(cluster: &Cluster) {
    let mut trusted = Call::get("/livez", None);
    trusted.identity = Some(&cluster.tls.client_identity);
    let answer = cluster.client.call(&trusted);
    assert_eq!(answer.status, 200);
    let mut rogue = Call::get("/livez", None);
    rogue.identity = Some(&cluster.tls.rogue_identity);
    let outcome = cluster.client.try_call(&rogue);
    assert!(
        outcome.is_err() || outcome.as_ref().is_ok_and(|answer| answer.body.is_empty()),
        "a client certificate outside the configured CA must not be served"
    );
}

#[test]
fn real_node_boundary_serves_the_component_contract() {
    let mut cluster = start_cluster();
    check_readiness(&cluster);
    let first = signed_send(&cluster.actor, cluster.asset, 1);
    check_entitlements(&cluster, &first);
    let key = format!("first-{}", token());
    let submitted = submit_send(&cluster, &first, &key);
    let replay = cluster.client.call(&Call::submit(
        "/v1/activities",
        &cluster.gateway_token,
        &submitted.key,
        &first,
    ));
    assert_eq!(replay.status, 200);
    assert_eq!(replay.text(), submitted.body);
    let second = signed_send(&cluster.actor, cluster.asset, 2);
    let conflict = cluster.client.call(&Call::submit(
        "/v1/activities",
        &cluster.gateway_token,
        &submitted.key,
        &second,
    ));
    assert_refusal(&conflict, 409, "idempotency_conflict");
    let record = journal_record(&cluster, &submitted.key);
    assert_eq!(record["state"], "completed");
    assert_eq!(record["attempts"], 1);
    assert_eq!(record["activity_id"], submitted.activity_id);
    assert_eq!(record["receipt"], hex(&submitted.receipt));
    check_receipt_routes(&cluster, &submitted);
    check_program_routes(&cluster, &submitted, &first);
    let (refused_key, refused_body) = check_refusals(&cluster, &first);
    check_relays(&cluster, &submitted);
    check_client_certificates(&cluster);

    cluster.boundary.stop();
    cluster.boundary.start();
    check_readiness(&cluster);
    let replay_after_restart = cluster.client.call(&Call::submit(
        "/v1/activities",
        &cluster.gateway_token,
        &submitted.key,
        &first,
    ));
    assert_eq!(replay_after_restart.status, 200);
    assert_eq!(replay_after_restart.text(), submitted.body);
    let stranger = actor();
    let refused_replay = cluster.client.call(&Call::submit(
        "/v1/activities",
        &cluster.gateway_token,
        &refused_key,
        &signed_send(&stranger, cluster.asset, 1),
    ));
    assert_refusal(&refused_replay, 409, "idempotency_conflict");
    assert_eq!(journal_record(&cluster, &submitted.key)["attempts"], 1);
    assert_eq!(journal_record(&cluster, &refused_key)["attempts"], 1);
    assert_eq!(
        journal_record(&cluster, &refused_key)["refusal"]["code"],
        "unknown_did"
    );
    assert!(refused_body.contains("unknown_did"));
    let second_key = format!("second-{}", token());
    let second_submitted = submit_send(&cluster, &second, &second_key);
    assert_ne!(second_submitted.activity_id, submitted.activity_id);
    check_receipt_routes(&cluster, &second_submitted);

    check_daemon_loss(&mut cluster, &submitted, &first);
}

#[test]
fn persisted_submission_attempt_is_not_repeated_after_connectivity_returns() {
    let mut cluster = start_cluster();
    cluster.boundary.stop();
    let run_directory = cluster.root.join("run");
    must(
        fs::set_permissions(&run_directory, fs::Permissions::from_mode(0o700)),
        "deny boundary access to the real node socket",
    );
    cluster.boundary.start();
    let signed = signed_send(&cluster.actor, cluster.asset, 1);
    let key = format!("uncertain-{}", token());
    let unavailable = cluster.client.call(&Call::submit(
        "/v1/activities",
        &cluster.gateway_token,
        &key,
        &signed,
    ));
    assert_refusal(&unavailable, 503, "node_unavailable");
    let pending = journal_record(&cluster, &key);
    assert_eq!(pending["attempts"], 1);
    assert_eq!(pending["state"], "submitting");
    cluster.boundary.stop();
    must(
        fs::set_permissions(&run_directory, fs::Permissions::from_mode(0o750)),
        "restore boundary access to the real node socket",
    );
    cluster.boundary.start();
    check_readiness(&cluster);
    let retry = cluster.client.call(&Call::submit(
        "/v1/activities",
        &cluster.gateway_token,
        &key,
        &signed,
    ));
    assert_eq!(retry.status, 202, "{}", retry.text());
    assert_eq!(retry.json()["state"], "unknown");
    assert_eq!(journal_record(&cluster, &key)["attempts"], 1);
    assert_eq!(journal_record(&cluster, &key)["state"], "submitting");
    let activity = field(&pending, "activity_id");
    let lookup = cluster.client.get(
        &format!("/v1/receipts/{activity}"),
        Some(&cluster.gateway_token),
    );
    assert_eq!(lookup.status, 404, "{}", lookup.text());
}

fn check_daemon_loss(cluster: &mut Cluster, submitted: &Submitted, first: &[u8]) {
    cluster.sequencer.stop();
    let deadline = Instant::now() + Duration::from_secs(30);
    let lost = loop {
        let ready = cluster.client.get("/readyz", None);
        if ready.status != 200 || Instant::now() >= deadline {
            break ready;
        }
        thread::sleep(Duration::from_millis(100));
    };
    assert_refusal(&lost, 503, "node_unavailable");
    let third = signed_send(&cluster.actor, cluster.asset, 3);
    let third_key = format!("third-{}", token());
    let unavailable = cluster.client.call(&Call::submit(
        "/v1/activities",
        &cluster.gateway_token,
        &third_key,
        &third,
    ));
    assert_eq!(unavailable.status, 503, "{}", unavailable.text());
    assert!(
        matches!(
            unavailable.error_code().as_str(),
            "node_unavailable" | "node_transport_lost"
        ),
        "{}",
        unavailable.text()
    );
    let journaled = cluster.client.call(&Call::submit(
        "/v1/activities",
        &cluster.gateway_token,
        &submitted.key,
        first,
    ));
    assert_eq!(journaled.status, 200);
    assert_eq!(journaled.text(), submitted.body);
    let relay_lost = cluster.client.get(
        "/v1/protocol/account-state/head",
        Some(&cluster.registry_token),
    );
    assert_refusal(&relay_lost, 503, "node_unavailable");
    let live = cluster.client.get("/livez", None);
    assert_eq!(live.status, 200);
}

fn signed_program_call(actor: &Actor, program_id: [u8; 32]) -> Vec<u8> {
    use layerx_types::intent::ProgramId;
    use layerx_types::intent::{CallBudget, Calldata, ProgramCall, RequestedCapabilities};
    let activity_type = must(
        ActivityType::new(ModuleId::Programs, 3),
        "program activity type",
    );
    let registration = must(
        ModuleRegistration::new(ModuleId::Programs, &[activity_type]),
        "program registration",
    );
    let registry = must(ModuleRegistry::new(&[registration]), "program registry");
    let call = ProgramCall::new(
        ProgramId::new(program_id),
        must(Calldata::new(&[]), "calldata"),
        must(CallBudget::new(1000, Amount::from_u128(0)), "call budget"),
        must(RequestedCapabilities::new(&[]), "capabilities"),
    );
    let payload = must(
        Payload::new(&registry, activity_type, &call.canonical_payload()),
        "call payload",
    );
    let payload_hash = domain_hash(Domain::PayloadHash, payload.as_bytes());
    let key = actor.signing_key.verifying_key().to_bytes();
    let mut builder = EnvelopeBuilder::new();
    must(
        builder
            .protocol_version(PROTOCOL_VERSION)
            .and_then(|value| value.network_id(NETWORK_ID))
            .and_then(|value| value.activity_type(activity_type))
            .and_then(|value| value.actor_did(must(Did::new(actor.did.as_bytes()), "actor DID")))
            .and_then(|value| value.authority(must(Authority::owner(&key), "owner")))
            .and_then(|value| value.account_sequence(1))
            .and_then(|value| {
                value.timestamp_bound(must(
                    TimestampBound::new(now_ms().saturating_sub(30_000), now_ms() + 120_000),
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
    let digest = domain_hash(
        Domain::SignaturePreimage,
        &must(encode_unsigned_envelope(&unsigned), "signing bytes"),
    );
    let signature = actor.signing_key.sign(&digest).to_bytes();
    must(
        encode_signed_envelope(
            &unsigned.attach_signature(must(Signature::new(&signature), "signature")),
        ),
        "signed ProgramCall",
    )
}

#[test]
fn real_program_call_refusal_artifacts_are_bound_and_replay_after_restart() {
    let mut cluster = start_cluster();
    check_readiness(&cluster);
    let program_id = random32();
    let signed = signed_program_call(&cluster.actor, program_id);
    let key = format!("program-refusal-{}", token());
    let deadline = Instant::now() + Duration::from_secs(30);
    let submitted = loop {
        let answer = cluster.client.call(&Call::submit(
            "/v1/programs/call",
            &cluster.gateway_token,
            &key,
            &signed,
        ));
        if answer.status == 200 || Instant::now() >= deadline {
            break answer;
        }
        assert!(
            answer.status == 202 || answer.status == 503,
            "{}",
            answer.text()
        );
        thread::sleep(Duration::from_millis(100));
    };
    assert_eq!(submitted.status, 200, "{}", submitted.text());
    let document = submitted.json();
    let result = &document["result"];
    assert_eq!(result["state"], "refused");
    assert_eq!(result["terminal_payload"], "");
    assert_eq!(result["call_graph"], "");
    check_program_refusal_artifact_endpoint(&cluster, result);
    let lookup_path = format!("/v1/programs/activities/{}", field(result, "activity_id"));
    let lookup = cluster
        .client
        .get(&lookup_path, Some(&cluster.gateway_token));
    assert_eq!(lookup.status, 200, "{}", lookup.text());
    assert_eq!(lookup.json()["result"]["program_id"], hex(&program_id));
    assert_eq!(journal_record(&cluster, &key)["attempts"], 1);
    let key_digest = format!("{:x}", Sha256::digest(key.as_bytes()));
    let journal = cluster
        .state_dir
        .join("journal")
        .join(format!("{key_digest}.json"));
    let original = must(fs::read(&journal), "completed journal");
    let mut corrupted: serde_json::Value = must(serde_json::from_slice(&original), "journal JSON");
    corrupted["program_execution"]["call_graph"] = serde_json::json!("00");
    must(
        fs::write(
            &journal,
            must(serde_json::to_vec(&corrupted), "corrupt journal encoding"),
        ),
        "corrupt artifact",
    );
    let refused = cluster.client.call(&Call::submit(
        "/v1/programs/call",
        &cluster.gateway_token,
        &key,
        &signed,
    ));
    assert_refusal(&refused, 503, "program_artifacts_invalid");
    must(fs::write(&journal, &original), "restore verified artifacts");
    cluster.boundary.stop();
    cluster.sequencer.stop();
    cluster.replica.stop();
    cluster.boundary.start();
    let replay = cluster.client.call(&Call::submit(
        "/v1/programs/call",
        &cluster.gateway_token,
        &key,
        &signed,
    ));
    assert_eq!(replay.status, 200, "{}", replay.text());
    assert_eq!(replay.text(), submitted.text());
    let replay_lookup = cluster
        .client
        .get(&lookup_path, Some(&cluster.gateway_token));
    assert_eq!(replay_lookup.status, 200, "{}", replay_lookup.text());
    assert_eq!(replay_lookup.text(), lookup.text());
    assert_eq!(journal_record(&cluster, &key)["attempts"], 1);
}

fn check_program_refusal_artifact_endpoint(cluster: &Cluster, result: &serde_json::Value) {
    let receipt_bytes = unhex(field(result, "receipt"));
    let receipt = must(
        verify_sequencer_signature(&receipt_bytes, cluster.sequencer_key),
        "real ProgramCall receipt",
    );
    let protocol = receipt
        .protocol()
        .unwrap_or_else(|| panic!("protocol receipt"));
    assert_eq!(protocol.module_id(), 9);
    assert!(protocol.result_code() < 0);
    assert_eq!(hex(&protocol.activity_id()), field(result, "activity_id"));
    let digest = must(
        layerx_wire::hash::receipt_digest(&must(
            layerx_wire::receipt::encode_unsigned(&receipt),
            "unsigned receipt",
        )),
        "receipt digest",
    );
    let artifact_path = format!(
        "/v1/programs/activities/{}/artifacts?receipt_digest={}",
        field(result, "activity_id"),
        hex(&digest)
    );
    let artifacts = http_get(cluster.program_port, &artifact_path, &cluster.program_token);
    if protocol.program_outcome().is_some() {
        assert_eq!(artifacts.status, 200, "{}", artifacts.text());
        let artifacts = artifacts.json();
        assert_eq!(artifacts["activity_id"], result["activity_id"]);
        assert_eq!(artifacts["receipt_digest"], hex(&digest));
        assert_eq!(artifacts["terminal_payload"], "");
        assert_eq!(artifacts["call_graph"], "");
    } else {
        assert_ne!(artifacts.status, 200);
    }
    assert_ne!(
        http_get(cluster.program_port, &artifact_path, &token()).status,
        200
    );
    let wrong_activity = format!(
        "/v1/programs/activities/{}/artifacts?receipt_digest={}",
        hex(&random32()),
        hex(&digest)
    );
    assert_ne!(
        http_get(
            cluster.program_port,
            &wrong_activity,
            &cluster.program_token
        )
        .status,
        200
    );
    let wrong_digest = format!(
        "/v1/programs/activities/{}/artifacts?receipt_digest={}",
        field(result, "activity_id"),
        hex(&random32())
    );
    assert_ne!(
        http_get(cluster.program_port, &wrong_digest, &cluster.program_token).status,
        200
    );
}
