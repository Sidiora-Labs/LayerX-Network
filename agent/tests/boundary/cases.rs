use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::thread;
use std::time::Duration;

use layerx_client::lni::handshake::{perform, Handshake, HandshakeConfig};
use layerx_client::lni::preparation::{preparation_state, PreparationStateContext};
use layerx_client::lni::schema::{encode_envelope, Capability, Envelope, Version};
use layerx_client::lni::transport::{ConnectionGate, FrameTransport, Limits, Uds};
use layerx_proof::merkle::leaf_hash;
use layerx_types::ids::Did;
use layerx_types::payload::{ActivityType, ModuleId, ModuleRegistration, ModuleRegistry};
use layerx_wire::activity::decode_signed;
use layerx_wire::hash::{activity_id, batch_header_digest, checkpoint_id};
use layerx_wire::receipt::{
    decode as decode_receipt, decode_batch_header, decode_checkpoint,
    encode as encode_receipt, encode_batch_header, encode_checkpoint,
};

static NEXT_DIRECTORY: AtomicUsize = AtomicUsize::new(0);

#[derive(Debug)]
struct Response {
    version: Version,
    tag: u16,
    correlation_id: u64,
    payload: Vec<u8>,
    proof: Vec<u8>,
}

struct NodeProcess {
    child: Child,
    socket: PathBuf,
}

impl NodeProcess {
    fn start(
        executable: &Path,
        socket: &Path,
        genesis: &Path,
        mode: &str,
    ) -> Result<Self, String> {
        let child = Command::new(executable)
            .arg("--serve")
            .arg(socket)
            .arg(genesis)
            .arg(mode)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::inherit())
            .spawn()
            .map_err(|error| format!("could not start real layerxd: {error}"))?;
        let mut process = Self {
            child,
            socket: socket.to_path_buf(),
        };
        for _ in 0..200 {
            if socket.exists() {
                return Ok(process);
            }
            if let Some(status) = process
                .child
                .try_wait()
                .map_err(|error| format!("could not inspect layerxd: {error}"))?
            {
                return Err(format!("layerxd exited before listen: {status}"));
            }
            thread::sleep(Duration::from_millis(5));
        }
        let _ = process.child.kill();
        let _ = process.child.wait();
        Err("real layerxd did not create its Unix socket".to_owned())
    }

    fn stop(mut self) -> Result<(), String> {
        self.child
            .kill()
            .map_err(|error| format!("could not stop layerxd: {error}"))?;
        self.child
            .wait()
            .map_err(|error| format!("could not reap layerxd: {error}"))?;
        let _ = fs::remove_file(&self.socket);
        Ok(())
    }
}

impl Drop for NodeProcess {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
        let _ = fs::remove_file(&self.socket);
    }
}

fn limits() -> Limits {
    Limits {
        maximum_frame_bytes: 1_048_630,
        maximum_connections: 4,
        maximum_streams: 32,
        maximum_queued_bytes: 4_194_304,
        deadline: Duration::from_secs(2),
    }
}

pub(crate) fn connect(socket: &Path) -> Result<Uds, String> {
    Uds::connect(socket, &ConnectionGate::new(4), limits())
        .map_err(|error| format!("LNI connect failed: {error:?}"))
}

fn config() -> HandshakeConfig {
    HandshakeConfig {
        built_interface_version: Version::V1_1,
        expected_protocol_version: 1,
        expected_network_id: 77,
    }
}

fn request(
    transport: &mut impl FrameTransport,
    tag: u16,
    correlation_id: u64,
    payload: &[u8],
) -> Result<(), String> {
    let encoded = encode_envelope(Envelope {
        version: Version::V1_1,
        message_tag: tag,
        correlation_id,
        canonical_payload: payload,
        proof_material: &[],
    })
    .map_err(|error| format!("request encoding failed: {error:?}"))?;
    transport
        .send(&encoded)
        .map_err(|error| format!("request send failed: {error:?}"))
}

fn receive(transport: &mut impl FrameTransport) -> Result<Response, String> {
    let bytes = transport
        .receive()
        .map_err(|error| format!("response receive failed: {error:?}"))?;
    decode_response(&bytes)
}

fn decode_response(bytes: &[u8]) -> Result<Response, String> {
    if bytes.len() < 22 {
        return Err("truncated response envelope".to_owned());
    }
    let version = Version {
        major: u16::from_be_bytes([bytes[0], bytes[1]]),
        minor: u16::from_be_bytes([bytes[2], bytes[3]]),
    };
    let tag = u16::from_be_bytes([bytes[4], bytes[5]]);
    let correlation_id = u64::from_be_bytes(
        bytes[6..14]
            .try_into()
            .map_err(|_| "invalid correlation identifier".to_owned())?,
    );
    let payload_length = usize::try_from(u32::from_be_bytes(
        bytes[14..18]
            .try_into()
            .map_err(|_| "invalid payload length".to_owned())?,
    ))
    .map_err(|_| "unrepresentable payload length".to_owned())?;
    let payload_end = 18_usize
        .checked_add(payload_length)
        .ok_or_else(|| "payload length overflow".to_owned())?;
    let proof_length_end = payload_end
        .checked_add(4)
        .ok_or_else(|| "proof prefix overflow".to_owned())?;
    let proof_length = usize::try_from(u32::from_be_bytes(
        bytes
            .get(payload_end..proof_length_end)
            .ok_or_else(|| "truncated proof prefix".to_owned())?
            .try_into()
            .map_err(|_| "invalid proof length".to_owned())?,
    ))
    .map_err(|_| "unrepresentable proof length".to_owned())?;
    let proof_end = proof_length_end
        .checked_add(proof_length)
        .ok_or_else(|| "proof length overflow".to_owned())?;
    if proof_end != bytes.len() {
        return Err("response envelope has trailing or truncated bytes".to_owned());
    }
    Ok(Response {
        version,
        tag,
        correlation_id,
        payload: bytes[18..payload_end].to_vec(),
        proof: bytes[proof_length_end..proof_end].to_vec(),
    })
}

fn expect(response: &Response, tag: u16, correlation_id: u64) -> Result<(), String> {
    if response.version != Version::V1_1
        || response.tag != tag
        || response.correlation_id != correlation_id
    {
        return Err(format!(
            "unexpected response: version={:?} tag={} correlation={}",
            response.version, response.tag, response.correlation_id
        ));
    }
    Ok(())
}

fn assert_leaf(response: &Response) -> Result<(), String> {
    let root = leaf_hash(&response.payload)
        .map_err(|error| format!("independent payload hash failed: {error:?}"))?;
    if response.proof.as_slice() != root {
        return Err(format!("response tag {} claimed a mismatched root", response.tag));
    }
    Ok(())
}

fn registry() -> Result<ModuleRegistry, String> {
    let activity = ActivityType::new(ModuleId::Asset, 1)
        .map_err(|error| format!("activity type failed: {error:?}"))?;
    let registration = ModuleRegistration::new(ModuleId::Asset, &[activity])
        .map_err(|error| format!("module registration failed: {error:?}"))?;
    ModuleRegistry::new(&[registration])
        .map_err(|error| format!("module registry failed: {error:?}"))
}

#[allow(clippy::too_many_lines)]
fn exercise_live_messages(
    transport: &mut impl FrameTransport,
    handshake: &Handshake,
) -> Result<BTreeSet<u16>, String> {
    for capability in [
        Capability::NodeInfo,
        Capability::Submit,
        Capability::ReceiptLookup,
        Capability::AccountRead,
        Capability::HistoryRange,
        Capability::BatchHeader,
        Capability::Checkpoint,
        Capability::ProofBundle,
        Capability::AvailabilityFetch,
        Capability::EventSubscribe,
        Capability::PreparationState,
    ] {
        handshake
            .capabilities()
            .require(capability)
            .map_err(|error| format!("real node capability gap: {error:?}"))?;
    }

    let mut covered = BTreeSet::from([1_u16, 2_u16]);
    request(transport, 9, 9, &[0, 0, 0, 1])?;
    covered.insert(9);
    let history = receive(transport)?;
    expect(&history, 10, 9)?;
    assert_leaf(&history)?;
    covered.insert(10);
    let history_end = receive(transport)?;
    expect(&history_end, 11, 9)?;
    covered.insert(11);

    request(transport, 3, 3, &history.payload)?;
    covered.insert(3);
    let submitted = receive(transport)?;
    expect(&submitted, 4, 3)?;
    let decoded = decode_signed(&submitted.payload, &registry()?)
        .map_err(|error| format!("node activity is not canonical: {error:?}"))?;
    let identifier = activity_id(&decoded)
        .map_err(|error| format!("activity identifier failed: {error:?}"))?;
    if submitted.proof.as_slice() != identifier {
        return Err("submitted activity identifier mismatch".to_owned());
    }
    covered.insert(4);

    request(transport, 5, 5, &identifier)?;
    covered.insert(5);
    let receipt = receive(transport)?;
    expect(&receipt, 6, 5)?;
    let decoded_receipt = decode_receipt(&receipt.payload)
        .map_err(|error| format!("node receipt is not canonical: {error:?}"))?;
    if encode_receipt(&decoded_receipt)
        .map_err(|error| format!("receipt re-encode failed: {error:?}"))?
        != receipt.payload
    {
        return Err("receipt canonical re-encoding changed bytes".to_owned());
    }
    assert_leaf(&receipt)?;
    covered.insert(6);

    request(transport, 7, 7, &[1])?;
    covered.insert(7);
    let account = receive(transport)?;
    expect(&account, 8, 7)?;
    assert_leaf(&account)?;
    covered.insert(8);

    request(transport, 12, 12, &[22])?;
    covered.insert(12);
    let header = receive(transport)?;
    expect(&header, 13, 12)?;
    let decoded_header = decode_batch_header(&header.payload)
        .map_err(|error| format!("node header is not canonical: {error:?}"))?;
    if encode_batch_header(&decoded_header)
        .map_err(|error| format!("header re-encode failed: {error:?}"))?
        != header.payload
    {
        return Err("batch header canonical re-encoding changed bytes".to_owned());
    }
    let header_identifier = batch_header_digest(&header.payload)
        .map_err(|error| format!("batch header hash failed: {error:?}"))?;
    if header.proof.as_slice() != header_identifier {
        return Err("batch header identifier mismatch".to_owned());
    }
    covered.insert(13);

    request(transport, 14, 14, &[22])?;
    covered.insert(14);
    let checkpoint = receive(transport)?;
    expect(&checkpoint, 15, 14)?;
    let decoded_checkpoint = decode_checkpoint(&checkpoint.payload)
        .map_err(|error| format!("checkpoint is not canonical: {error:?}"))?;
    if encode_checkpoint(&decoded_checkpoint)
        .map_err(|error| format!("checkpoint re-encode failed: {error:?}"))?
        != checkpoint.payload
    {
        return Err("checkpoint canonical re-encoding changed bytes".to_owned());
    }
    let proof_length = usize::try_from(u32::from_be_bytes(
        checkpoint.payload[354..358]
            .try_into()
            .map_err(|_| "checkpoint proof length malformed".to_owned())?,
    ))
    .map_err(|_| "checkpoint proof length is unrepresentable".to_owned())?;
    let proof_end = 358_usize
        .checked_add(proof_length)
        .ok_or_else(|| "checkpoint proof overflow".to_owned())?;
    let checkpoint_identifier = checkpoint_id(
        &checkpoint.payload[..354],
        checkpoint
            .payload
            .get(358..proof_end)
            .ok_or_else(|| "checkpoint proof truncated".to_owned())?,
    )
    .map_err(|error| format!("checkpoint identifier failed: {error:?}"))?;
    if checkpoint.proof.as_slice() != checkpoint_identifier {
        return Err("checkpoint identifier mismatch".to_owned());
    }
    covered.insert(15);

    request(transport, 16, 16, &[1])?;
    covered.insert(16);
    let proof = receive(transport)?;
    expect(&proof, 17, 16)?;
    assert_leaf(&proof)?;
    covered.insert(17);

    request(transport, 18, 18, &[22])?;
    covered.insert(18);
    let chunk = receive(transport)?;
    expect(&chunk, 19, 18)?;
    assert_leaf(&chunk)?;
    covered.insert(19);
    let availability_end = receive(transport)?;
    expect(&availability_end, 20, 18)?;
    if availability_end.payload.first() != Some(&0x1f)
        || availability_end.proof != availability_end.payload[1..]
    {
        return Err("availability class report or commitment mismatch".to_owned());
    }
    covered.insert(20);

    request(transport, 21, 21, &[0])?;
    covered.insert(21);
    let event = receive(transport)?;
    expect(&event, 22, 21)?;
    assert_leaf(&event)?;
    covered.insert(22);
    let gap = receive(transport)?;
    expect(&gap, 23, 21)?;
    covered.insert(23);
    let heartbeat = receive(transport)?;
    expect(&heartbeat, 24, 21)?;
    covered.insert(24);

    let actor = Did::new(b"did:layerx:production-boundary")
        .map_err(|error| format!("preparation actor failed: {error:?}"))?;
    let preparation = preparation_state(
        transport,
        &actor,
        PreparationStateContext {
            interface_version: Version::V1_1,
            expected_network_id: 77,
            minimum_observed_head: handshake.node().chain_head_sequence,
            correlation_id: 27,
        },
    )
    .map_err(|error| format!("production preparation snapshot failed: {error:?}"))?;
    if preparation.actor != actor
        || preparation.account_sequence != 5
        || preparation.protocol_timestamp != 1_700_000_001_000
        || preparation.observed_head_sequence != 10
        || preparation.kernel_epoch != 3
        || !preparation.module_registry.declares(
            ActivityType::new(ModuleId::Programs, 1)
                .map_err(|error| format!("program activity failed: {error:?}"))?,
        )
    {
        return Err("production preparation snapshot changed facts".to_owned());
    }
    covered.insert(26);
    covered.insert(27);

    let incompatible = encode_envelope(Envelope {
        version: Version { major: 2, minor: 0 },
        message_tag: 1,
        correlation_id: 25,
        canonical_payload: &[],
        proof_material: &[],
    })
    .map_err(|error| format!("incompatible request encoding failed: {error:?}"))?;
    transport
        .send(&incompatible)
        .map_err(|error| format!("incompatible request send failed: {error:?}"))?;
    let error = receive(transport)?;
    expect(&error, 25, 25)?;
    covered.insert(25);

    request(transport, 9, 26, &history_end.payload)?;
    let page_two = receive(transport)?;
    expect(&page_two, 10, 26)?;
    assert_leaf(&page_two)?;
    let page_two_end = receive(transport)?;
    expect(&page_two_end, 11, 26)?;
    Ok(covered)
}

fn create_genesis(executable: &Path, path: &Path) -> Result<(), String> {
    let status = Command::new(executable)
        .arg("--write-genesis")
        .arg(path)
        .status()
        .map_err(|error| format!("could not invoke genesis writer: {error}"))?;
    if !status.success() {
        return Err(format!("real layerxd genesis writer failed: {status}"));
    }
    let metadata = fs::metadata(path)
        .map_err(|error| format!("signed genesis manifest missing: {error}"))?;
    if metadata.len() == 0 {
        return Err("signed genesis manifest is empty".to_owned());
    }
    Ok(())
}

/// Runs every boundary case against an external core-linked `layerxd` process.
///
/// # Errors
///
/// Fails on process startup, any missing message/capability, non-canonical
/// protocol bytes, dishonest health behavior, or incomplete coverage.
pub fn agent_boundary_conformance_suite(
    executable: &Path,
    repository: &Path,
) -> Result<String, String> {
    if !executable.is_file() || !repository.join("include/layerx/lxp_daemon.h").is_file() {
        return Err("boundary suite requires the repository's real layerxd binary".to_owned());
    }
    let sequence = NEXT_DIRECTORY.fetch_add(1, Ordering::Relaxed);
    let directory = std::env::temp_dir().join(format!(
        "layerx-boundary-{}-{sequence}",
        std::process::id()
    ));
    fs::create_dir(&directory)
        .map_err(|error| format!("could not create boundary directory: {error}"))?;
    let genesis = directory.join("genesis.lxp");
    let socket = directory.join("layerxd.sock");
    create_genesis(executable, &genesis)?;

    if connect(&socket).is_ok() {
        return Err("unreachable-node case unexpectedly connected".to_owned());
    }

    let normal = NodeProcess::start(executable, &socket, &genesis, "normal")?;
    let mut connection = connect(&socket)?;
    let accepted = perform(&mut connection, &config(), None)
        .map_err(|error| format!("normal handshake failed: {error:?}"))?;
    let capability_qualification = crate::gaps::verify_and_render(&accepted)?;
    let covered = exercise_live_messages(&mut connection, &accepted)?;
    if covered != (1_u16..=27).collect() {
        return Err(format!("incomplete live message coverage: {covered:?}"));
    }
    drop(connection);
    normal.stop()?;

    let restarted = NodeProcess::start(executable, &socket, &genesis, "normal")?;
    let mut reconnect = connect(&socket)?;
    let after_restart = perform(&mut reconnect, &config(), Some(&accepted))
        .map_err(|error| format!("restart handshake failed: {error:?}"))?;
    if after_restart.node().chain_head_sequence != 10 {
        return Err("restart changed the durable advertised head".to_owned());
    }
    drop(reconnect);
    restarted.stop()?;

    let behind = NodeProcess::start(executable, &socket, &genesis, "behind")?;
    let mut behind_connection = connect(&socket)?;
    let behind_handshake = perform(&mut behind_connection, &config(), None)
        .map_err(|error| format!("behind-node handshake failed: {error:?}"))?;
    if behind_handshake.node().chain_head_sequence != 5 {
        return Err("behind node did not disclose its stale head".to_owned());
    }
    drop(behind_connection);
    behind.stop()?;

    let degraded = NodeProcess::start(executable, &socket, &genesis, "degraded")?;
    let mut degraded_connection = connect(&socket)?;
    let degraded_handshake = perform(&mut degraded_connection, &config(), None)
        .map_err(|error| format!("degraded-node handshake failed: {error:?}"))?;
    if degraded_handshake
        .capabilities()
        .contains(Capability::Submit)
    {
        return Err("degraded node advertised unavailable submission".to_owned());
    }
    request(&mut degraded_connection, 3, 30, &[1])?;
    let refusal = receive(&mut degraded_connection)?;
    expect(&refusal, 25, 30)?;
    drop(degraded_connection);
    degraded.stop()?;

    fs::remove_dir_all(&directory)
        .map_err(|error| format!("could not clean boundary directory: {error}"))?;
    Ok(format!(
        "agent boundary conformance suite passed: real node, 27 messages, restart, behind, unreachable, degraded\n{capability_qualification}"
    ))
}
