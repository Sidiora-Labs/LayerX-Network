use ed25519_dalek::{Signer as _, SigningKey};
#[path = "../../../../../tests/support/lxgb_metadata.rs"]
mod lxgb_metadata;

use layerx_agentd::boot::{handshake_gate, Gate, GateError};
use layerx_client::lni::handshake::{perform, HandshakeConfig};
use layerx_client::lni::refusal::decode_core_refusal;
use layerx_client::lni::schema::{decode_envelope, encode_envelope, Envelope, Version};
use layerx_client::lni::transport::{ConnectionGate, FrameTransport, Limits, Uds};
use layerx_programs::hex;
use layerx_types::activity::{Authority, EnvelopeBuilder, Signature, TimestampBound};
use layerx_types::amount::Amount;
use layerx_types::ids::{Did, IdempotencyKey};
use layerx_types::intent::ProgramId;
use layerx_types::payload::{ActivityType, ModuleId, ModuleRegistration, ModuleRegistry, Payload};
use layerx_types::program_call::{NativeProgramCall, Resources};
use layerx_wire::activity::{decode_signed, encode_signed_envelope, encode_unsigned_envelope};
use layerx_wire::hash::{activity_id, Domain};
use sha2::{Digest as _, Sha256};
use std::collections::BTreeMap;
use std::fmt::Debug;
use std::fs;
use std::net::{TcpListener, TcpStream};
use std::os::unix::fs::PermissionsExt;
use std::os::unix::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::Mutex;
use std::thread;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

const LNI_FRAME_BYTES: usize = 1_212_416;
const LOG_BYTES: u64 = 64 * 1024 * 1024;
const MODULE_GOVERNANCE: u16 = 7;
const METERING_AUTHORITY_GENESIS: u8 = 1;
static FIXTURE_LOCK: Mutex<()> = Mutex::new(());
#[derive(Clone, Copy)]
struct Scope {
    protocol_version: u16,
    network_id: u32,
}
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

#[derive(Clone, Copy)]
pub(super) struct Clock {
    pub(super) wall_time: fn() -> SystemTime,
    pub(super) monotonic_time: fn() -> Instant,
}

impl Clock {
    fn now_ms(self) -> u64 {
        let elapsed = must((self.wall_time)().duration_since(UNIX_EPOCH), "clock");
        u64::try_from(elapsed.as_millis()).unwrap_or(u64::MAX)
    }
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

fn preallocate_log(path: &Path) {
    let mode =
        std::env::var("LAYERX_TEST_BOOTSTRAP_LOG_MODE").unwrap_or_else(|_| "absent".to_owned());
    assert!(
        !path.exists(),
        "fresh log already exists: {}",
        path.display()
    );
    if mode == "absent" {
        return;
    }
    assert!(
        mode == "empty" || mode == "preallocated",
        "unsupported bootstrap log mode: {mode}"
    );
    let file = must(
        fs::File::create(path),
        &format!("create {}", path.display()),
    );
    let size = if mode == "empty" { 0 } else { LOG_BYTES };
    must(file.set_len(size), &format!("size {}", path.display()));
    assert_eq!(must(file.metadata(), "initial log metadata").len(), size);
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

fn token() -> String {
    hex::encode(&random32())
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
    receipt_state_root: [u8; 32],
}

fn genesis_request(
    clock: Clock,
    scope: Scope,
    asset: &[u8; 32],
    guarantor_key: &[u8; 33],
) -> Vec<u8> {
    let mut request = Vec::with_capacity(512);
    request.extend_from_slice(b"LXGB");
    request.push(2);
    request.extend_from_slice(&scope.protocol_version.to_be_bytes());
    request.extend_from_slice(&scope.network_id.to_be_bytes());
    request.extend_from_slice(&clock.now_ms().to_be_bytes());
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
    let issuer = SigningKey::from_bytes(&random32());
    lxgb_metadata::append(
        &mut request,
        asset,
        &issuer.verifying_key().to_bytes(),
        &random32(),
    );
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

fn build_genesis(clock: Clock, scope: Scope, root: &Path, builder: &Path) -> Genesis {
    let directory = root.join("genesis");
    make_dir(&directory, 0o755);
    let asset = random32();
    let guarantor_key = genesis_guarantor_key(&directory);
    write(
        &directory.join("request.lxgb"),
        &genesis_request(clock, scope, &asset, &guarantor_key),
        0o600,
    );
    write(&directory.join("signer.key"), &random32(), 0o600);
    let artifacts = directory.join("artifacts");
    command(
        &builder.to_string_lossy(),
        &[
            &directory.join("request.lxgb").to_string_lossy(),
            &directory.join("signer.key").to_string_lossy(),
            &artifacts.to_string_lossy(),
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
        receipt_state_root,
    }
}

fn registration(scope: Scope, receipt_state_root: &[u8; 32]) -> Vec<u8> {
    let mut encoded = Vec::with_capacity(82);
    encoded.extend_from_slice(b"LXGR");
    encoded.push(1);
    encoded.extend_from_slice(&scope.network_id.to_be_bytes());
    encoded.extend_from_slice(&0_u64.to_be_bytes());
    encoded.extend_from_slice(receipt_state_root);
    encoded.extend_from_slice(receipt_state_root);
    encoded.push(1);
    encoded
}

fn node_config(scope: Scope, role: &str) -> String {
    let network_id = scope.network_id;
    format!(
        "role={role}\nnetwork_id={network_id}\nstart_sequence=0\nverify_workers=0\nnetwork_workers=0\nprojection_workers=0\ncheckpoint_workers=0\nserial_execution=true\n"
    )
}

struct Identity {
    daemon_uid: u32,
    daemon_gid: u32,
    client_uid: u32,
    client_gid: u32,
}

fn identity() -> Identity {
    let client_uid = rustix_free_uid();
    assert_eq!(
        client_uid, 0,
        "the real-node harness must run as root so layerxd can run under a distinct uid"
    );
    Identity {
        daemon_uid: 65_534,
        daemon_gid: 0,
        client_uid,
        client_gid: 0,
    }
}

fn rustix_free_uid() -> u32 {
    let status = must(fs::read_to_string("/proc/self/status"), "process status");
    status
        .lines()
        .find_map(|line| line.strip_prefix("Uid:"))
        .and_then(|line| line.split_whitespace().nth(1))
        .and_then(|value| value.parse::<u32>().ok())
        .unwrap_or_else(|| panic!("effective uid is not readable"))
}

fn spawn_daemon(
    layerxd: &Path,
    mode: &str,
    config: &Path,
    environment: &BTreeMap<&str, String>,
    identity: &Identity,
    stderr: PathBuf,
) -> Daemon {
    let mut command = Command::new(layerxd);
    command
        .arg(mode)
        .arg(config)
        .env_clear()
        .env("PATH", "/usr/bin:/bin")
        .envs(environment.iter().map(|(key, value)| (key, value.as_str())))
        .uid(identity.daemon_uid)
        .gid(identity.daemon_gid)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::from(must(
            fs::File::create(&stderr),
            "daemon stderr file",
        )));
    let child = must(command.spawn(), &format!("spawn {mode}"));
    Daemon { child, stderr }
}

fn wait_for_port(clock: Clock, port: u16, daemon: &mut Daemon, what: &str) {
    let deadline = (clock.monotonic_time)() + Duration::from_secs(30);
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
            (clock.monotonic_time)() < deadline,
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
        maximum_queued_bytes: LNI_FRAME_BYTES,
        deadline: Duration::from_secs(10),
    }
}

fn connect_lni(scope: Scope, socket: &Path, gate: &ConnectionGate) -> Uds {
    let mut transport = must(Uds::connect(socket, gate, lni_limits()), "LNI connect");
    let handshake = must(
        perform(
            &mut transport,
            &HandshakeConfig {
                built_interface_version: Version::V1_4,
                expected_protocol_version: scope.protocol_version,
                expected_network_id: scope.network_id,
            },
            None,
        ),
        "LNI handshake",
    );
    assert!(!handshake
        .node()
        .advertised_capabilities
        .iter()
        .any(|name| name == "account_read"));
    let mut account_request = vec![0, 1, 2];
    account_request.extend_from_slice(&random32());
    account_request.extend_from_slice(&[1, 3]);
    let (tag, payload, proof) = exchange(&mut transport, 7, 9_001, &account_request);
    assert_eq!(tag, 25);
    assert!(proof.is_empty());
    let refusal =
        decode_core_refusal(&payload).unwrap_or_else(|| panic!("account refusal is not canonical"));
    assert_eq!(refusal.class, 3);
    assert_eq!(
        refusal.result.raw(),
        layerx_types::result::KnownResult::ModuleDisabled.raw()
    );
    transport
}

fn wait_for_lni(
    clock: Clock,
    scope: Scope,
    socket: &Path,
    gate: &ConnectionGate,
    daemon: &mut Daemon,
) {
    let deadline = (clock.monotonic_time)() + Duration::from_secs(60);
    let expected = HandshakeConfig {
        built_interface_version: Version::V1_4,
        expected_protocol_version: scope.protocol_version,
        expected_network_id: scope.network_id,
    };
    loop {
        if socket.exists() {
            if let Ok(mut transport) = Uds::connect(socket, gate, lni_limits()) {
                if perform(&mut transport, &expected, None).is_ok() {
                    return;
                }
            }
        }
        if let Ok(Some(status)) = daemon.child.try_wait() {
            panic!(
                "sequencer exited early with {status}: {}",
                daemon.diagnostics()
            );
        }
        assert!(
            (clock.monotonic_time)() < deadline,
            "sequencer LNI did not come up: {}",
            daemon.diagnostics()
        );
        thread::sleep(Duration::from_millis(100));
    }
}

fn exchange(
    transport: &mut dyn FrameTransport,
    tag: u16,
    correlation_id: u64,
    payload: &[u8],
) -> (u16, Vec<u8>, Vec<u8>) {
    let request = must(
        encode_envelope(Envelope {
            version: Version::V1_4,
            message_tag: tag,
            correlation_id,
            canonical_payload: payload,
            proof_material: &[],
        }),
        "LNI request encoding",
    );
    must(transport.send(&request), "LNI send");
    let response = must(transport.receive(), "LNI receive");
    let response = must(decode_envelope(&response), "LNI response decoding");
    assert_eq!(response.correlation_id, correlation_id);
    (
        response.message_tag,
        response.canonical_payload.to_vec(),
        response.proof_material.to_vec(),
    )
}

fn domain_hash(domain: Domain, bytes: &[u8]) -> [u8; 32] {
    let mut digest = Sha256::new();
    digest.update(domain.tag());
    digest.update(bytes);
    digest.finalize().into()
}

fn registry() -> ModuleRegistry {
    let activity = must(ActivityType::new(ModuleId::Programs, 3), "activity type");
    let registration = must(
        ModuleRegistration::new(ModuleId::Programs, &[activity]),
        "registration",
    );
    must(ModuleRegistry::new(&[registration]), "registry")
}

struct Actor {
    signing_key: SigningKey,
    did: String,
}

fn actor() -> Actor {
    let signing_key = SigningKey::from_bytes(&random32());
    let did = format!(
        "did:layerx:{}",
        hex::encode(&signing_key.verifying_key().to_bytes())
    );
    Actor { signing_key, did }
}

fn signed_program_call(clock: Clock, scope: Scope, actor: &Actor, sequence: u64) -> Vec<u8> {
    let idempotency = random32();
    let not_before = clock.now_ms().saturating_sub(30_000);
    let expires_at = clock.now_ms() + 120_000;
    let call = NativeProgramCall {
        program_id: ProgramId::new(random32()),
        guest_abi: 1,
        entrypoint: b"layerx_call",
        calldata: &[],
        capabilities: &[0, 0],
        access_declaration: b"LayerX/programs/access-declaration/v1\0\0",
        response_capacity: 16,
        resources: Resources([
            1_000_000, 16_777_216, 1_048_576, 1_048_576, 64, 1_048_576, 4096,
        ]),
    };
    let payload_bytes = must(call.encode(), "native call encoding");
    let activity_type = must(ActivityType::new(ModuleId::Programs, 3), "activity type");
    let payload = must(
        Payload::new(&registry(), activity_type, &payload_bytes),
        "payload",
    );
    let payload_hash = domain_hash(Domain::PayloadHash, payload.as_bytes());
    let public_key = actor.signing_key.verifying_key().to_bytes();
    let mut builder = EnvelopeBuilder::new();
    must(
        builder
            .protocol_version(scope.protocol_version)
            .and_then(|value| value.network_id(scope.network_id))
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

struct Submitted {
    activity_type: ActivityType,
    activity_id: [u8; 32],
    receipt: Vec<u8>,
}

fn submit_and_wait(
    clock: Clock,
    transport: &mut dyn FrameTransport,
    signed: &[u8],
    correlation: u64,
) -> Submitted {
    let decoded = must(decode_signed(signed, &registry()), "signed activity decode");
    let expected_id = must(activity_id(&decoded), "activity id");
    let (tag, retained, evidence) = exchange(transport, 3, correlation, signed);
    assert_eq!(
        tag,
        4,
        "submit was refused: {:?}",
        decode_core_refusal(&retained)
    );
    assert_eq!(retained, signed);
    assert_eq!(evidence.as_slice(), expected_id);
    let mut selector = Vec::with_capacity(33);
    selector.push(1);
    selector.extend_from_slice(&expected_id);
    let deadline = (clock.monotonic_time)() + Duration::from_secs(30);
    let mut attempt = 0_u64;
    loop {
        let (tag, receipt, proof) = exchange(transport, 5, correlation + 1000 + attempt, &selector);
        assert_eq!(tag, 6);
        assert!(proof.is_empty());
        if !receipt.is_empty() {
            return Submitted {
                activity_type: decoded.activity_type(),
                activity_id: expected_id,
                receipt,
            };
        }
        assert!(
            (clock.monotonic_time)() < deadline,
            "receipt never became durable"
        );
        attempt += 1;
        thread::sleep(Duration::from_millis(50));
    }
}

fn verify_replica_receipt(fixture: &Fixture, submitted: &Submitted) {
    let scope = fixture.scope;
    let decoded = must(
        layerx_wire::receipt::decode(&submitted.receipt),
        "real receipt decoding",
    );
    let protocol = decoded
        .protocol()
        .unwrap_or_else(|| panic!("real receipt protocol missing"));
    assert_eq!(protocol.activity_id(), submitted.activity_id);
    assert_eq!(protocol.protocol_version(), scope.protocol_version);
    let unsigned = must(
        layerx_wire::receipt::encode_unsigned(&decoded),
        "unsigned receipt encoding",
    );
    let digest = must(
        layerx_wire::hash::receipt_digest(&unsigned),
        "real receipt digest",
    );
    let body = fetch_replica_receipt(fixture, protocol.batch_id(), digest);
    let inclusion = verify_replica_inclusion(fixture, submitted, &body);
    assert_replica_receipt(fixture, submitted, protocol, &inclusion);
}

fn fetch_replica_receipt(
    fixture: &Fixture,
    batch_id: [u8; 32],
    digest: [u8; 32],
) -> serde_json::Value {
    let replica_port = fixture.replica_port;
    let replica_token = &fixture.replica_token;
    let port = replica_port;
    let bearer = replica_token;
    let url = format!(
        "http://127.0.0.1:{port}/v1/batches/{}/receipt-authority?receipt_digest={}",
        hex::encode(&batch_id),
        hex::encode(&digest)
    );
    let client: ureq::Agent = ureq::Agent::config_builder()
        .timeout_global(Some(Duration::from_secs(10)))
        .build()
        .into();
    let mut response = must(
        client
            .get(&url)
            .header("Authorization", &format!("Bearer {bearer}"))
            .call(),
        "real replica evidence",
    );
    must(
        response.body_mut().read_json(),
        "real replica evidence JSON",
    )
}

fn verify_replica_inclusion(
    fixture: &Fixture,
    submitted: &Submitted,
    body: &serde_json::Value,
) -> ReplicaReceiptEvidence {
    let replica_id = fixture.replica_id;
    let sequencer_id = fixture.sequencer_id;
    let sequencer_key = fixture.sequencer_key;
    let last_batch = fixture.last_batch;
    let replica = replica_id;
    let sequencer = sequencer_id;
    let key = sequencer_key;
    assert_eq!(
        body["authority_replica_id"].as_str(),
        Some(hex::encode(&replica).as_str())
    );
    assert_eq!(
        body["sequencer_public_key"].as_str(),
        Some(hex::encode(&key).as_str())
    );
    let field = |name| {
        must(
            hex::decode(
                body["batch_evidence"][name]
                    .as_str()
                    .unwrap_or_else(|| panic!("replica field missing")),
            ),
            "replica hexadecimal",
        )
    };
    let header = field("header_hex");
    let signature: [u8; 64] = must(
        field("header_signature").try_into(),
        "replica header signature",
    );
    let canonical_proof = must(
        layerx_wire::receipt::decode_merkle_proof(&field("receipt_proof_hex")),
        "replica wire receipt proof",
    );
    let proof = must(
        layerx_proof::merkle::Proof::new(
            canonical_proof.leaf_index(),
            canonical_proof.leaf_count(),
            canonical_proof.siblings().to_vec(),
        ),
        "replica receipt proof",
    );
    let authorization =
        layerx_proof::inclusion::SequencerAuthorization::new(sequencer, key, 1, last_batch);
    let inclusion = must(
        layerx_proof::inclusion::verify_receipt(
            &submitted.receipt,
            &proof,
            &header,
            &signature,
            &authorization,
        ),
        "real replica signature and inclusion",
    );
    let committed = inclusion.header().header();
    let (last_activity_sequence, activity_resulting_root) =
        match body["batch_evidence"].get("batch_identity") {
            None => (committed.last_sequence(), committed.resulting_state_root()),
            Some(identity) => verify_maintenance_attachment(
                submitted,
                identity,
                &inclusion,
                &ReplicaInclusionInputs {
                    header: &header,
                    signature: &signature,
                    proof: &proof,
                    authorization: &authorization,
                },
            ),
        };
    ReplicaReceiptEvidence {
        inclusion,
        last_activity_sequence,
        activity_resulting_root,
    }
}

struct ReplicaReceiptEvidence {
    inclusion: layerx_proof::inclusion::InclusionEvidence,
    last_activity_sequence: u64,
    activity_resulting_root: [u8; 32],
}

struct ReplicaInclusionInputs<'a> {
    header: &'a [u8],
    signature: &'a [u8; 64],
    proof: &'a layerx_proof::merkle::Proof,
    authorization: &'a layerx_proof::inclusion::SequencerAuthorization,
}

fn verify_maintenance_attachment(
    submitted: &Submitted,
    identity: &serde_json::Value,
    inclusion: &layerx_proof::inclusion::InclusionEvidence,
    inputs: &ReplicaInclusionInputs<'_>,
) -> (u64, [u8; 32]) {
    assert_eq!(identity["kind"].as_str(), Some("occupancy_maintenance_v2"));
    let field = |name| {
        must(
            hex::decode(
                identity[name]
                    .as_str()
                    .unwrap_or_else(|| panic!("maintenance field missing")),
            ),
            "maintenance hexadecimal",
        )
    };
    let maintenance_bytes = field("receipt_hex");
    let canonical_proof = must(
        layerx_wire::receipt::decode_merkle_proof(&field("receipt_proof_hex")),
        "maintenance wire proof",
    );
    let maintenance_proof = must(
        layerx_proof::merkle::Proof::new(
            canonical_proof.leaf_index(),
            canonical_proof.leaf_count(),
            canonical_proof.siblings().to_vec(),
        ),
        "maintenance proof",
    );
    let receipt = must(
        layerx_wire::receipt::decode(&submitted.receipt),
        "activity receipt",
    );
    let protocol = receipt
        .protocol()
        .unwrap_or_else(|| panic!("activity protocol"));
    let header = inclusion.header().header();
    let activity = must(
        layerx_proof::receipt::authorized_maintained_activity_batch(
            &submitted.receipt,
            &layerx_proof::receipt::AuthorizedBatch::new(
                protocol.batch_id(),
                protocol.asset(),
                header.previous_state_root(),
                header.resulting_state_root(),
                inputs.authorization.public_key(),
            ),
            &layerx_proof::receipt::MaintainedOutcomeEvidence {
                header: inputs.header,
                header_signature: inputs.signature,
                activity_proof: inputs.proof,
                maintenance: &maintenance_bytes,
                maintenance_proof: &maintenance_proof,
                authorization: inputs.authorization,
            },
        ),
        "authenticated maintenance transition",
    );
    let maintenance = must(
        layerx_wire::maintenance::decode_occupancy_maintenance(&maintenance_bytes),
        "maintenance receipt",
    );
    assert_eq!(
        header.resulting_state_root(),
        maintenance.resulting_state_root
    );
    assert_eq!(
        protocol.resulting_state_root(),
        maintenance.previous_state_root
    );
    assert_eq!(
        activity.resulting_state_root(),
        maintenance.previous_state_root
    );
    (
        header
            .last_sequence()
            .checked_sub(1)
            .unwrap_or_else(|| panic!("maintained activity endpoint")),
        activity.resulting_state_root(),
    )
}

fn assert_replica_receipt(
    fixture: &Fixture,
    submitted: &Submitted,
    protocol: &layerx_wire::receipt::ProtocolReceipt,
    evidence: &ReplicaReceiptEvidence,
) {
    let scope = fixture.scope;
    let sequencer_key = fixture.sequencer_key;
    let key = sequencer_key;
    let committed = evidence.inclusion.header().header();
    assert_eq!(committed.network_id(), scope.network_id);
    assert_eq!(committed.protocol_version(), scope.protocol_version);
    assert!(protocol.global_sequence() >= committed.first_sequence());
    assert!(protocol.global_sequence() <= committed.last_sequence());
    assert_eq!(
        submitted.activity_type,
        must(
            ActivityType::new(ModuleId::Programs, 3),
            "submitted call type"
        )
    );
    assert_eq!(
        protocol.module_id(),
        submitted.activity_type.module() as u16
    );
    assert_eq!(protocol.activity_root(), committed.activity_merkle_root());
    let expected_batch = must(
        layerx_wire::hash::program_execution_batch_id(
            committed.previous_state_root(),
            committed.activity_merkle_root(),
            committed.first_sequence(),
            evidence.last_activity_sequence,
            committed.batch_number(),
        ),
        "real execution batch identity",
    );
    assert_eq!(expected_batch, protocol.batch_id());
    assert_eq!(
        protocol.previous_state_root(),
        committed.previous_state_root()
    );
    assert_eq!(
        protocol.resulting_state_root(),
        evidence.activity_resulting_root
    );
    assert_eq!(protocol.module_version(), 4);
    assert_eq!(protocol.operation(), 0);
    assert_eq!(
        protocol.result_code(),
        layerx_types::result::KnownResult::UnknownField.raw()
    );
    assert!(protocol.effects().is_empty());
    assert_eq!(protocol.transfer_set_root(), [0; 32]);
    assert_eq!(protocol.asset(), [0; 32]);
    assert_eq!(protocol.amount(), 0);
    assert_eq!(protocol.fee_charged(), 0);
    assert_eq!(protocol.from(), [0; 32]);
    assert_eq!(protocol.to(), [0; 32]);
    assert_eq!(protocol.debit_balance_before(), 0);
    assert_eq!(protocol.debit_balance_after(), 0);
    assert_eq!(protocol.credit_balance_before(), 0);
    assert_eq!(protocol.credit_balance_after(), 0);
    must(
        layerx_proof::receipt::verify_sequencer_signature(&submitted.receipt, key),
        "real planning refusal signature",
    );
}

struct Fixture {
    clock: Clock,
    scope: Scope,
    identity: Identity,
    root: PathBuf,
    genesis: Genesis,
    layerxd: PathBuf,
    migrations: PathBuf,
    sequencer_seed: [u8; 32],
    sequencer_key: [u8; 32],
    sequencer_id: [u8; 32],
    replica_id: [u8; 32],
    replica_token: String,
    program_token: String,
    replica_port: u16,
    program_port: u16,
    last_batch: u64,
}

fn fixture(
    clock: Clock,
    policy: super::TestAuthorityPolicy<'_>,
    expected_sequencer_id: Option<[u8; 32]>,
) -> Fixture {
    let scope = Scope {
        protocol_version: policy.protocol_version,
        network_id: policy.network_id,
    };
    let identity = identity();
    let repository = repository_root();
    let binaries = std::env::var_os("LAYERX_TEST_NATIVE_BIN_DIR")
        .map_or_else(|| repository.join("build/bin"), PathBuf::from);
    let layerxd = binaries.join("layerxd");
    let builder = binaries.join("layerx-genesis-build");
    assert!(layerxd.is_file(), "{} is not built", layerxd.display());
    assert!(builder.is_file(), "{} is not built", builder.display());
    let root = std::env::temp_dir().join(format!(
        "layerx-authority-{}-{}-{}",
        std::process::id(),
        clock.now_ms(),
        hex::encode(&random32()[..8])
    ));
    make_dir(&root, 0o755);
    let genesis = build_genesis(clock, scope, &root, &builder);
    let daemon_binary = root.join("layerxd");
    must(
        fs::hard_link(&layerxd, &daemon_binary),
        "link layerxd into the harness root",
    );
    let layerxd = daemon_binary;
    let migrations = root.join("migrations");
    make_dir(&migrations, 0o755);
    for entry in must(
        fs::read_dir(repository.join("migrations")),
        "migrations listing",
    ) {
        let source = must(entry, "migration entry").path();
        if source.is_file() {
            let name = source
                .file_name()
                .unwrap_or_else(|| panic!("migration file name"));
            must(fs::copy(&source, migrations.join(name)), "copy migration");
        }
    }

    let sequencer_seed = policy.handshake_signing_seed;
    let sequencer_signing = SigningKey::from_bytes(&sequencer_seed);
    let sequencer_key = sequencer_signing.verifying_key().to_bytes();
    let sequencer_id = expected_sequencer_id.unwrap_or(sequencer_key);
    let replica_id = random32();
    let replica_token = token();
    let program_token = token();
    let replica_port = free_port();
    let program_port = free_port();
    let last_batch = policy
        .handshake_batch
        .checked_add(1)
        .unwrap_or_else(|| panic!("fixture batch range overflow"));

    Fixture {
        clock,
        scope,
        identity,
        root,
        genesis,
        layerxd,
        migrations,
        sequencer_seed,
        sequencer_key,
        sequencer_id,
        replica_id,
        replica_token,
        program_token,
        replica_port,
        program_port,
        last_batch,
    }
}

struct Replica {
    directory: PathBuf,
    environment: BTreeMap<&'static str, String>,
    daemon: Daemon,
}

fn start_replica(fixture: &Fixture) -> Replica {
    let root = &fixture.root;
    let scope = fixture.scope;
    let identity = &fixture.identity;
    let layerxd = &fixture.layerxd;
    let replica_id = fixture.replica_id;
    let sequencer_id = fixture.sequencer_id;
    let sequencer_key = fixture.sequencer_key;
    let last_batch = fixture.last_batch;
    let replica_token = &fixture.replica_token;
    let replica_port = fixture.replica_port;
    let replica_dir = root.join("replica");
    make_dir(&replica_dir, 0o700);
    write(
        &replica_dir.join("config.txt"),
        node_config(scope, "replica").as_bytes(),
        0o600,
    );
    preallocate_log(&replica_dir.join("replica.log"));
    chown_tree(&replica_dir, identity.daemon_uid, identity.daemon_gid);
    let mut replica_env = BTreeMap::new();
    replica_env.insert(
        "LAYERX_AUTHORITY_REPLICA_LOG",
        replica_dir
            .join("replica.log")
            .to_string_lossy()
            .into_owned(),
    );
    replica_env.insert("LAYERX_AUTHORITY_REPLICA_ID", hex::encode(&replica_id));
    replica_env.insert("LAYERX_AUTHORITY_SEQUENCER_ID", hex::encode(&sequencer_id));
    replica_env.insert(
        "LAYERX_AUTHORITY_SEQUENCER_PUBLIC_KEY",
        hex::encode(&sequencer_key),
    );
    replica_env.insert("LAYERX_AUTHORITY_FIRST_BATCH", 1_u64.to_string());
    replica_env.insert("LAYERX_AUTHORITY_LAST_BATCH", last_batch.to_string());
    replica_env.insert("LAYERX_AUTHORITY_BEARER_TOKEN", replica_token.clone());
    replica_env.insert("LAYERX_AUTHORITY_ADDRESS", "127.0.0.1".to_owned());
    replica_env.insert("LAYERX_AUTHORITY_PORT", replica_port.to_string());
    let mut replica = spawn_daemon(
        layerxd,
        "--authority-replica",
        &replica_dir.join("config.txt"),
        &replica_env,
        identity,
        root.join("replica.stderr"),
    );
    wait_for_port(
        fixture.clock,
        replica_port,
        &mut replica,
        "authority replica",
    );
    assert!(
        must(
            fs::metadata(replica_dir.join("replica.log")),
            "replica log metadata"
        )
        .len()
            > 0,
        "replica did not grow its fresh log"
    );

    Replica {
        directory: replica_dir,
        environment: replica_env,
        daemon: replica,
    }
}

struct NodePaths {
    directory: PathBuf,
    checkpoints: PathBuf,
    logs: PathBuf,
    socket: PathBuf,
    actor: Actor,
}

fn prepare_node(fixture: &Fixture) -> NodePaths {
    let root = &fixture.root;
    let scope = fixture.scope;
    let genesis = &fixture.genesis;
    let identity = &fixture.identity;
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
        &registration(scope, &genesis.receipt_state_root),
        0o600,
    );
    let actor = actor();
    let identities = format!(
        "{}:{}:1\n",
        hex::encode(actor.did.as_bytes()),
        hex::encode(&actor.signing_key.verifying_key().to_bytes())
    );
    write(
        &node_dir.join("identities.txt"),
        identities.as_bytes(),
        0o600,
    );
    write(
        &node_dir.join("config.txt"),
        node_config(scope, "sequencer").as_bytes(),
        0o600,
    );
    for name in [
        "feed.log",
        "canonical.log",
        "receipt-authority.log",
        "batch.log",
        "evidence.log",
    ] {
        preallocate_log(&logs.join(name));
    }
    chown_tree(&genesis.directory, identity.daemon_uid, identity.daemon_gid);
    chown_tree(&node_dir, identity.daemon_uid, identity.daemon_gid);
    chown_tree(&run_dir, identity.daemon_uid, identity.client_gid);
    let socket = run_dir.join("layerxd.sock");
    NodePaths {
        directory: node_dir,
        checkpoints,
        logs,
        socket,
        actor,
    }
}

fn node_storage_environment(
    fixture: &Fixture,
    paths: &NodePaths,
) -> BTreeMap<&'static str, String> {
    let genesis = &fixture.genesis;
    let migrations = &fixture.migrations;
    let node_dir = &paths.directory;
    let checkpoints = &paths.checkpoints;
    let logs = &paths.logs;
    let text = |path: PathBuf| path.to_string_lossy().into_owned();
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
    node_env.insert(
        "LAYERX_NODE_CHECKPOINT_DIRECTORY",
        text(checkpoints.clone()),
    );
    node_env.insert(
        "LAYERX_NODE_SNAPSHOT",
        text(genesis.directory.join("00000000000000000000.lxs")),
    );
    node_env.insert(
        "LAYERX_NODE_GENESIS_MANIFEST",
        text(genesis.directory.join("genesis.manifest")),
    );
    node_env.insert(
        "LAYERX_NODE_GENESIS_REGISTRATION",
        text(node_dir.join("registration.lxgr")),
    );
    node_env.insert(
        "LAYERX_NODE_IDENTITIES",
        text(node_dir.join("identities.txt")),
    );
    node_env.insert("LAYERX_NODE_PROGRAM_FEED_LOG", text(logs.join("feed.log")));
    node_env.insert(
        "LAYERX_NODE_CANONICAL_LOG",
        text(logs.join("canonical.log")),
    );
    node_env.insert(
        "LAYERX_NODE_RECEIPT_AUTHORITY_LOG",
        text(logs.join("receipt-authority.log")),
    );
    node_env.insert("LAYERX_NODE_BATCH_LOG", text(logs.join("batch.log")));
    node_env.insert("LAYERX_NODE_EVIDENCE_LOG", text(logs.join("evidence.log")));
    node_env.insert(
        "LAYERX_NODE_HISTORY_DATABASE",
        text(node_dir.join("history.db")),
    );
    node_env.insert(
        "LAYERX_NODE_HISTORY_MIGRATIONS",
        text(migrations.join("0007_history_index.sql")),
    );
    node_env
}

fn node_environment(fixture: &Fixture, paths: &NodePaths) -> BTreeMap<&'static str, String> {
    let identity = &fixture.identity;
    let sequencer_id = fixture.sequencer_id;
    let sequencer_key = fixture.sequencer_key;
    let sequencer_seed = fixture.sequencer_seed;
    let last_batch = fixture.last_batch;
    let replica_port = fixture.replica_port;
    let replica_id = fixture.replica_id;
    let replica_token = &fixture.replica_token;
    let program_token = &fixture.program_token;
    let program_port = fixture.program_port;
    let socket = &paths.socket;
    let text = |path: &Path| path.to_string_lossy().into_owned();
    let mut node_env = node_storage_environment(fixture, paths);
    node_env.insert("LAYERX_NODE_SEQUENCER_ID", hex::encode(&sequencer_id));
    node_env.insert(
        "LAYERX_NODE_SEQUENCER_PUBLIC_KEY",
        hex::encode(&sequencer_key),
    );
    node_env.insert(
        "LAYERX_NODE_SEQUENCER_PRIVATE_KEY",
        hex::encode(&sequencer_seed),
    );
    node_env.insert("LAYERX_NODE_FIRST_BATCH", 1_u64.to_string());
    node_env.insert("LAYERX_NODE_LAST_BATCH", last_batch.to_string());
    node_env.insert(
        "LAYERX_NODE_AUTHORITY_REPLICA_ADDRESS",
        "127.0.0.1".to_owned(),
    );
    node_env.insert(
        "LAYERX_NODE_AUTHORITY_REPLICA_PORT",
        replica_port.to_string(),
    );
    node_env.insert("LAYERX_NODE_AUTHORITY_REPLICA_ID", hex::encode(&replica_id));
    node_env.insert(
        "LAYERX_NODE_AUTHORITY_REPLICA_BEARER_TOKEN",
        replica_token.clone(),
    );
    node_env.insert("LAYERX_NODE_PROGRAM_BEARER_TOKEN", program_token.clone());
    node_env.insert("LAYERX_NODE_PROGRAM_ADDRESS", "127.0.0.1".to_owned());
    node_env.insert("LAYERX_NODE_PROGRAM_PORT", program_port.to_string());
    node_env.insert("LAYERX_NODE_LNI_SOCKET", text(socket));
    node_env.insert(
        "LAYERX_NODE_LNI_ALLOWED_UID",
        identity.client_uid.to_string(),
    );
    node_env.insert(
        "LAYERX_NODE_LNI_ALLOWED_GID",
        identity.client_gid.to_string(),
    );
    node_env.insert("LAYERX_NODE_LNI_FRAME_BYTES", LNI_FRAME_BYTES.to_string());
    node_env.insert("LAYERX_NODE_LNI_DEADLINE_MS", "10000".to_owned());
    node_env
}

fn start_sequencer(
    fixture: &Fixture,
    paths: &NodePaths,
    node_env: &BTreeMap<&str, String>,
) -> (ConnectionGate, Daemon, Uds) {
    let scope = fixture.scope;
    let layerxd = &fixture.layerxd;
    let identity = &fixture.identity;
    let root = &fixture.root;
    let node_dir = &paths.directory;
    let socket = &paths.socket;
    let gate = ConnectionGate::new(8);
    let mut sequencer = spawn_daemon(
        layerxd,
        "--serve",
        &node_dir.join("config.txt"),
        node_env,
        identity,
        root.join("sequencer.stderr"),
    );
    wait_for_lni(fixture.clock, scope, socket, &gate, &mut sequencer);
    let transport = connect_lni(scope, socket, &gate);
    (gate, sequencer, transport)
}

fn submit_calls(
    fixture: &Fixture,
    paths: &NodePaths,
    transport: &mut Uds,
    policy: super::TestAuthorityPolicy<'_>,
) -> Vec<(Vec<u8>, Submitted)> {
    let scope = fixture.scope;
    let mut retained = Vec::new();
    for sequence in 1..=policy.handshake_batch {
        let signed = signed_program_call(fixture.clock, scope, &paths.actor, sequence);
        let submitted = submit_and_wait(fixture.clock, transport, &signed, sequence * 10_000);
        verify_replica_receipt(fixture, &submitted);
        retained.push((signed, submitted));
    }
    retained
}

struct RestartContext<'a> {
    paths: &'a NodePaths,
    environment: &'a BTreeMap<&'static str, String>,
    gate: &'a ConnectionGate,
    retained: &'a [(Vec<u8>, Submitted)],
    policy: super::TestAuthorityPolicy<'a>,
}

fn restart_authority(
    fixture: &Fixture,
    authority_gate: &mut Gate,
    config: &layerx_agentd::config::StartupConfig,
    replica: &mut Replica,
    sequencer: &mut Daemon,
    context: &RestartContext<'_>,
) -> Result<(), GateError> {
    let layerxd = &fixture.layerxd;
    let identity = &fixture.identity;
    let root = &fixture.root;
    let scope = fixture.scope;
    let replica_port = fixture.replica_port;
    let sequencer_key = fixture.sequencer_key;
    let paths = context.paths;
    let node_dir = &paths.directory;
    let socket = &paths.socket;
    let node_env = context.environment;
    let gate = context.gate;
    let retained = context.retained;
    let policy = context.policy;
    authority_gate.disconnected();
    assert!(authority_gate.evidence_authority().is_err());
    sequencer.stop();
    replica.daemon.stop();
    *authority_gate = Gate::new(config)?;
    assert!(authority_gate.evidence_authority().is_err());
    replica.daemon = spawn_daemon(
        layerxd,
        "--authority-replica",
        &replica.directory.join("config.txt"),
        &replica.environment,
        identity,
        root.join("replica-restart.stderr"),
    );
    wait_for_port(
        fixture.clock,
        replica_port,
        &mut replica.daemon,
        "restarted authority replica",
    );
    *sequencer = spawn_daemon(
        layerxd,
        "--serve",
        &node_dir.join("config.txt"),
        node_env,
        identity,
        root.join("sequencer-restart.stderr"),
    );
    wait_for_lni(fixture.clock, scope, socket, gate, sequencer);
    let mut transport = connect_lni(scope, socket, gate);
    for (index, (signed, original)) in retained.iter().enumerate() {
        let correlation = u64::try_from(index).unwrap_or_else(|_| panic!("correlation overflow"))
            * 10_000
            + 100_000;
        let recovered = submit_and_wait(fixture.clock, &mut transport, signed, correlation);
        assert_eq!(recovered.activity_id, original.activity_id);
        assert_eq!(recovered.receipt, original.receipt);
        verify_replica_receipt(fixture, &recovered);
    }
    drop(transport);
    let mut transport = must(
        Uds::connect(socket, gate, lni_limits()),
        "restart authority transport",
    );
    let status = handshake_gate(authority_gate, &mut transport)?;
    assert_eq!(status.protocol_version, scope.protocol_version);
    assert_eq!(status.network_id, scope.network_id);
    assert_eq!(status.latest_sealed_batch, policy.handshake_batch);
    assert_eq!(status.authorised_sequencer_key, sequencer_key);
    assert!(status.writes_ready);
    assert!(authority_gate.evidence_authority().is_ok());
    Ok(())
}

pub(super) fn authorize(
    clock: Clock,
    authority_gate: &mut Gate,
    policy: super::TestAuthorityPolicy<'_>,
    expected_sequencer_id: Option<[u8; 32]>,
    restart_config: Option<&layerx_agentd::config::StartupConfig>,
) -> Result<(), GateError> {
    let _guard = FIXTURE_LOCK
        .lock()
        .unwrap_or_else(|_| panic!("real authority fixture lock poisoned"));
    let fixture = fixture(clock, policy, expected_sequencer_id);
    let mut replica = start_replica(&fixture);
    let paths = prepare_node(&fixture);
    let node_env = node_environment(&fixture, &paths);
    let (gate, mut sequencer, mut transport) = start_sequencer(&fixture, &paths, &node_env);
    let retained = submit_calls(&fixture, &paths, &mut transport, policy);
    drop(transport);
    let mut transport = must(
        Uds::connect(&paths.socket, &gate, lni_limits()),
        "fresh authority handshake transport",
    );
    let result = handshake_gate(authority_gate, &mut transport).map(|status| {
        assert_eq!(status.protocol_version, fixture.scope.protocol_version);
        assert_eq!(status.network_id, fixture.scope.network_id);
        assert_eq!(status.latest_sealed_batch, policy.handshake_batch);
        assert_eq!(status.authorised_sequencer_key, fixture.sequencer_key);
        assert!(status.writes_ready);
    });
    drop(transport);
    if let Some(config) = restart_config {
        result?;
        restart_authority(
            &fixture,
            authority_gate,
            config,
            &mut replica,
            &mut sequencer,
            &RestartContext {
                paths: &paths,
                environment: &node_env,
                gate: &gate,
                retained: &retained,
                policy,
            },
        )?;
    }
    sequencer.stop();
    replica.daemon.stop();
    let _ = fs::remove_dir_all(&fixture.root);
    result
}
