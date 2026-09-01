use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::io::{BufRead, BufReader, Write};
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, ExitCode, Stdio};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Barrier};
use std::thread;
use std::time::Duration;

use layerx_agent_api::identity::TenantId as SdkTenantId;
use layerx_agent_api::read::{
    AccountRef, BalanceValue, BatchRef, CheckpointRef, Freshness, RelativeTo, VerifiedRead,
};
use layerx_agent_api::track::{SubmissionRef, TrackRequest};
use layerx_agent_api::verify::Level;
use layerx_agent_api::{Amount, ContractVersion, Sequence, SubmissionState};
use layerx_agentd::idempotency::{
    EconomicResult, IdempotencyError, Outcome, RetentionPolicy, Store as IdempotencyStore,
};
use layerx_agentd::store::TenantId;
use layerx_client::lni::handshake::{perform, HandshakeConfig};
use layerx_client::lni::schema::{encode_envelope, Capability, Envelope, Version};
use layerx_client::lni::transport::{ConnectionGate, FrameTransport, Limits, Uds};
use layerx_proof::merkle::leaf_hash;
use layerx_sdk::approval::{
    ApprovalApproveRequest, ApprovalDecisionOutcome, ApprovalEventKind, ApprovalGetRequest,
    ApprovalId, ApprovalListRequest, ApprovalRejectRequest, DecisionKey,
};
use layerx_sdk::{Client as RustSdkClient, Deployment, Operation};
use layerx_types::result::ResultCode;

#[derive(Clone, Debug, Eq, PartialEq)]
struct Observation {
    state: String,
    error: String,
    verification: String,
    idempotency: String,
    result_code: String,
    receipt_count: usize,
    economic_effects: usize,
}

impl Observation {
    fn encode(&self) -> String {
        format!(
            "state={};error={};verification={};idempotency={};result_code={};receipt_count={};economic_effects={}",
            self.state,
            self.error,
            self.verification,
            self.idempotency,
            self.result_code,
            self.receipt_count,
            self.economic_effects
        )
    }

    fn decode(encoded: &str) -> Result<Self, String> {
        let fields = encoded
            .split(';')
            .map(|field| {
                field
                    .split_once('=')
                    .map(|(key, value)| (key.to_owned(), value.to_owned()))
                    .ok_or_else(|| format!("malformed observation field {field}"))
            })
            .collect::<Result<BTreeMap<_, _>, _>>()?;
        let required = |key: &str| {
            fields
                .get(key)
                .cloned()
                .ok_or_else(|| format!("observation missing {key}"))
        };
        Ok(Self {
            state: required("state")?,
            error: required("error")?,
            verification: required("verification")?,
            idempotency: required("idempotency")?,
            result_code: required("result_code")?,
            receipt_count: required("receipt_count")?
                .parse()
                .map_err(|_| "invalid receipt_count".to_owned())?,
            economic_effects: required("economic_effects")?
                .parse()
                .map_err(|_| "invalid economic_effects".to_owned())?,
        })
    }
}

fn unquote(value: &str) -> Result<String, String> {
    value
        .strip_prefix('"')
        .and_then(|value| value.strip_suffix('"'))
        .map(str::to_owned)
        .ok_or_else(|| format!("expected quoted value, got {value}"))
}

fn parse_scenarios(path: &Path) -> Result<BTreeMap<String, Observation>, String> {
    let source = fs::read_to_string(path)
        .map_err(|error| format!("could not read {}: {error}", path.display()))?;
    let mut current = None;
    let mut fields: BTreeMap<String, BTreeMap<String, String>> = BTreeMap::new();
    for raw in source.lines() {
        let line = raw.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        if let Some(section) = line
            .strip_prefix("[scenario.")
            .and_then(|value| value.strip_suffix(']'))
        {
            current = Some(section.to_owned());
            fields.entry(section.to_owned()).or_default();
            continue;
        }
        if line.starts_with('[') {
            current = None;
            continue;
        }
        let Some(scenario) = current.as_ref() else {
            continue;
        };
        let (key, value) = line
            .split_once('=')
            .ok_or_else(|| format!("malformed scenario declaration {line}"))?;
        fields
            .get_mut(scenario)
            .ok_or_else(|| "scenario parser lost section".to_owned())?
            .insert(key.trim().to_owned(), value.trim().to_owned());
    }
    fields
        .into_iter()
        .map(|(name, values)| {
            let string = |key: &str| {
                values
                    .get(key)
                    .ok_or_else(|| format!("scenario {name} missing {key}"))
                    .and_then(|value| unquote(value))
            };
            let number = |key: &str| {
                values
                    .get(key)
                    .ok_or_else(|| format!("scenario {name} missing {key}"))?
                    .parse::<usize>()
                    .map_err(|_| format!("scenario {name} has invalid {key}"))
            };
            let observation = Observation {
                state: string("state")?,
                error: string("error")?,
                verification: string("verification")?,
                idempotency: string("idempotency")?,
                result_code: string("result_code")?,
                receipt_count: number("receipt_count")?,
                economic_effects: number("economic_effects")?,
            };
            Ok((name, observation))
        })
        .collect()
}

struct NodeProcess {
    child: Child,
    socket: PathBuf,
}

impl NodeProcess {
    fn start(executable: &Path, socket: &Path, genesis: &Path, mode: &str) -> Result<Self, String> {
        let child = Command::new(executable)
            .arg("--serve")
            .arg(socket)
            .arg(genesis)
            .arg(mode)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::inherit())
            .spawn()
            .map_err(|error| format!("could not start layerxd: {error}"))?;
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
        Err("layerxd did not create its socket".to_owned())
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

#[derive(Debug)]
struct NodeResponse {
    tag: u16,
    correlation: u64,
    payload: Vec<u8>,
    proof: Vec<u8>,
}

fn limits() -> Limits {
    Limits {
        maximum_frame_bytes: 1_048_576,
        maximum_connections: 4,
        maximum_streams: 32,
        maximum_queued_bytes: 4_194_304,
        deadline: Duration::from_secs(2),
    }
}

fn connect_node(socket: &Path) -> Result<Uds, String> {
    Uds::connect(socket, &ConnectionGate::new(4), limits())
        .map_err(|error| format!("node connection failed: {error:?}"))
}

fn node_config() -> HandshakeConfig {
    HandshakeConfig {
        built_interface_version: Version::V1_0,
        expected_protocol_version: layerx_wire::limits::PROTOCOL_VERSION,
        expected_network_id: 77,
    }
}

fn node_request(
    transport: &mut impl FrameTransport,
    tag: u16,
    correlation: u64,
    payload: &[u8],
) -> Result<(), String> {
    let bytes = encode_envelope(Envelope {
        version: Version::V1_0,
        message_tag: tag,
        correlation_id: correlation,
        canonical_payload: payload,
        proof_material: &[],
    })
    .map_err(|error| format!("node request encoding failed: {error:?}"))?;
    transport
        .send(&bytes)
        .map_err(|error| format!("node request failed: {error:?}"))
}

fn node_response(transport: &mut impl FrameTransport) -> Result<NodeResponse, String> {
    let bytes = transport
        .receive()
        .map_err(|error| format!("node response failed: {error:?}"))?;
    if bytes.len() < 22 {
        return Err("truncated node response".to_owned());
    }
    let tag = u16::from_be_bytes([bytes[4], bytes[5]]);
    let correlation = u64::from_be_bytes(
        bytes[6..14]
            .try_into()
            .map_err(|_| "invalid node correlation".to_owned())?,
    );
    let payload_length = u32::from_be_bytes(
        bytes[14..18]
            .try_into()
            .map_err(|_| "invalid node payload length".to_owned())?,
    ) as usize;
    let payload_end = 18_usize
        .checked_add(payload_length)
        .ok_or_else(|| "node payload overflow".to_owned())?;
    let proof_prefix_end = payload_end
        .checked_add(4)
        .ok_or_else(|| "node proof prefix overflow".to_owned())?;
    let proof_length = u32::from_be_bytes(
        bytes
            .get(payload_end..proof_prefix_end)
            .ok_or_else(|| "truncated node proof prefix".to_owned())?
            .try_into()
            .map_err(|_| "invalid node proof length".to_owned())?,
    ) as usize;
    let proof_end = proof_prefix_end
        .checked_add(proof_length)
        .ok_or_else(|| "node proof overflow".to_owned())?;
    if proof_end != bytes.len() {
        return Err("node response length mismatch".to_owned());
    }
    Ok(NodeResponse {
        tag,
        correlation,
        payload: bytes[18..payload_end].to_vec(),
        proof: bytes[proof_prefix_end..].to_vec(),
    })
}

fn expect_node(response: &NodeResponse, tag: u16, correlation: u64) -> Result<(), String> {
    if response.tag != tag || response.correlation != correlation {
        return Err(format!("unexpected node response {response:?}"));
    }
    Ok(())
}

fn create_genesis(executable: &Path, path: &Path) -> Result<(), String> {
    let status = Command::new(executable)
        .arg("--write-genesis")
        .arg(path)
        .status()
        .map_err(|error| format!("could not create genesis: {error}"))?;
    if status.success() {
        Ok(())
    } else {
        Err(format!("genesis writer failed: {status}"))
    }
}

fn qualify_node(
    executable: &Path,
    directory: &Path,
    expected: &BTreeMap<String, Observation>,
) -> Result<NodeProcess, String> {
    let genesis = directory.join("genesis.lxp");
    let socket = directory.join("layerxd.sock");
    create_genesis(executable, &genesis)?;
    let normal = NodeProcess::start(executable, &socket, &genesis, "normal")?;
    let mut transport = connect_node(&socket)?;
    let handshake = perform(&mut transport, &node_config(), None)
        .map_err(|error| format!("node handshake failed: {error:?}"))?;
    handshake
        .capabilities()
        .require(Capability::AccountRead)
        .map_err(|error| format!("account capability missing: {error:?}"))?;
    node_request(&mut transport, 7, 7, &[1])?;
    let account = node_response(&mut transport)?;
    expect_node(&account, 8, 7)?;
    if account.proof
        != leaf_hash(&account.payload).map_err(|error| format!("read proof: {error:?}"))?
    {
        return Err("proven read root mismatch".to_owned());
    }
    assert_scenario(
        expected,
        "proven_read",
        &Observation {
            state: "Value".to_owned(),
            error: "None".to_owned(),
            verification: "StateProven".to_owned(),
            idempotency: "None".to_owned(),
            result_code: "none".to_owned(),
            receipt_count: 0,
            economic_effects: 0,
        },
    )?;
    node_request(&mut transport, 21, 21, &[0])?;
    expect_node(&node_response(&mut transport)?, 22, 21)?;
    expect_node(&node_response(&mut transport)?, 23, 21)?;
    let heartbeat = node_response(&mut transport)?;
    expect_node(&heartbeat, 24, 21)?;
    if heartbeat.payload != 10_u64.to_be_bytes() {
        return Err("subscription heartbeat changed".to_owned());
    }
    assert_scenario(
        expected,
        "subscription_gap",
        &Observation {
            state: "Gap".to_owned(),
            error: "None".to_owned(),
            verification: "SequencerSigned".to_owned(),
            idempotency: "None".to_owned(),
            result_code: "none".to_owned(),
            receipt_count: 0,
            economic_effects: 0,
        },
    )?;
    drop(transport);
    normal.stop()?;

    let degraded = NodeProcess::start(executable, &socket, &genesis, "degraded")?;
    let mut transport = connect_node(&socket)?;
    let handshake = perform(&mut transport, &node_config(), None)
        .map_err(|error| format!("degraded handshake failed: {error:?}"))?;
    if handshake
        .capabilities()
        .contains(Capability::AvailabilityFetch)
    {
        return Err("degraded node advertised availability".to_owned());
    }
    node_request(&mut transport, 18, 18, &[22])?;
    let unavailable = node_response(&mut transport)?;
    expect_node(&unavailable, 25, 18)?;
    if unavailable.payload.first() != Some(&3) {
        return Err("availability refusal class changed".to_owned());
    }
    assert_scenario(
        expected,
        "availability_failure",
        &Observation {
            state: "Refused".to_owned(),
            error: "UnavailableCapability".to_owned(),
            verification: "Unverified".to_owned(),
            idempotency: "None".to_owned(),
            result_code: "none".to_owned(),
            receipt_count: 0,
            economic_effects: 0,
        },
    )?;
    drop(transport);
    degraded.stop()?;
    NodeProcess::start(executable, &socket, &genesis, "normal")
}

fn tenant() -> Result<TenantId, String> {
    TenantId::new("parity-tenant").map_err(|error| format!("tenant: {error}"))
}

fn retention() -> Result<RetentionPolicy, String> {
    RetentionPolicy::new(100, 50).map_err(|error| format!("retention: {error:?}"))
}

fn economic_result() -> EconomicResult {
    EconomicResult {
        response_bytes: b"executed-once".to_vec(),
        receipt_ref: Some([0x77; 32]),
    }
}

fn qualify_idempotency(
    directory: &Path,
    expected: &BTreeMap<String, Observation>,
) -> Result<(), String> {
    let repeated_root = directory.join("idempotency-repeat");
    let effects = AtomicUsize::new(0);
    let store = IdempotencyStore::open(&repeated_root, tenant()?, retention()?)
        .map_err(|error| format!("repeat store: {error:?}"))?;
    let first = store
        .execute([1; 32], b"same-intent", 10, |_| {
            effects.fetch_add(1, Ordering::SeqCst);
            Ok(economic_result())
        })
        .map_err(|error| format!("first retry scenario: {error:?}"))?;
    let repeated = store
        .execute([1; 32], b"same-intent", 11, |_| {
            Err("duplicate executed".to_owned())
        })
        .map_err(|error| format!("repeated retry scenario: {error:?}"))?;
    if !matches!(first, Outcome::First(_))
        || !matches!(repeated, Outcome::RepeatedOriginal(_))
        || effects.load(Ordering::SeqCst) != 1
    {
        return Err("repeated idempotency produced more than one effect".to_owned());
    }
    assert_idempotency(expected, "idempotency_repeat", "RepeatedOriginal")?;

    let concurrent_root = directory.join("idempotency-concurrent");
    let store = Arc::new(
        IdempotencyStore::open(&concurrent_root, tenant()?, retention()?)
            .map_err(|error| format!("concurrent store: {error:?}"))?,
    );
    let barrier = Arc::new(Barrier::new(16));
    let effects = Arc::new(AtomicUsize::new(0));
    let workers = (0..16)
        .map(|_| {
            let store = Arc::clone(&store);
            let barrier = Arc::clone(&barrier);
            let effects = Arc::clone(&effects);
            thread::spawn(move || {
                barrier.wait();
                store.execute([2; 32], b"concurrent-intent", 20, |_| {
                    effects.fetch_add(1, Ordering::SeqCst);
                    Ok(economic_result())
                })
            })
        })
        .collect::<Vec<_>>();
    let outcomes = workers
        .into_iter()
        .map(|worker| {
            worker
                .join()
                .map_err(|_| "concurrent idempotency worker panicked".to_owned())?
                .map_err(|error| format!("concurrent idempotency: {error:?}"))
        })
        .collect::<Result<Vec<_>, _>>()?;
    if effects.load(Ordering::SeqCst) != 1
        || outcomes
            .iter()
            .filter(|outcome| matches!(outcome, Outcome::First(_)))
            .count()
            != 1
    {
        return Err("concurrent idempotency produced more than one effect".to_owned());
    }
    assert_idempotency(expected, "idempotency_concurrent", "Deduplicated")?;

    let restart_root = directory.join("idempotency-restart");
    {
        let store = IdempotencyStore::open(&restart_root, tenant()?, retention()?)
            .map_err(|error| format!("restart store: {error:?}"))?;
        if !matches!(
            store.execute([3; 32], b"restart-intent", 30, |_| Err(
                "transport lost".to_owned()
            )),
            Err(IdempotencyError::Operation(_))
        ) {
            return Err("pre-restart submission did not remain unresolved".to_owned());
        }
    }
    let effects = AtomicUsize::new(0);
    let store = IdempotencyStore::open(&restart_root, tenant()?, retention()?)
        .map_err(|error| format!("reopened store: {error:?}"))?;
    if store
        .restore(&[[3; 32]])
        .map_err(|error| format!("restore idempotency: {error:?}"))?
        != 1
    {
        return Err("post-restart idempotency record was not restored".to_owned());
    }
    let restored = store
        .execute([3; 32], b"restart-intent", 31, |attempt| {
            if !attempt.retry || attempt.exact_request_bytes != b"restart-intent" {
                return Err("restart changed exact request bytes".to_owned());
            }
            effects.fetch_add(1, Ordering::SeqCst);
            Ok(economic_result())
        })
        .map_err(|error| format!("post-restart retry: {error:?}"))?;
    if !matches!(restored, Outcome::RepeatedOriginal(_)) || effects.load(Ordering::SeqCst) != 1 {
        return Err("post-restart retry duplicated the effect".to_owned());
    }
    assert_idempotency(expected, "idempotency_restart", "RestoredOriginal")
}

fn assert_idempotency(
    expected: &BTreeMap<String, Observation>,
    scenario: &str,
    idempotency: &str,
) -> Result<(), String> {
    assert_scenario(
        expected,
        scenario,
        &Observation {
            state: "Executed".to_owned(),
            error: "None".to_owned(),
            verification: "SequencerSigned".to_owned(),
            idempotency: idempotency.to_owned(),
            result_code: "0".to_owned(),
            receipt_count: 1,
            economic_effects: 1,
        },
    )
}

fn qualify_state_semantics(expected: &BTreeMap<String, Observation>) -> Result<(), String> {
    if SubmissionState::Unknown.name() != "unknown" {
        return Err("Rust contract collapsed unknown state".to_owned());
    }
    assert_scenario(
        expected,
        "unknown_submission",
        &Observation {
            state: "Unknown".to_owned(),
            error: "None".to_owned(),
            verification: "Unverified".to_owned(),
            idempotency: "Original".to_owned(),
            result_code: "none".to_owned(),
            receipt_count: 0,
            economic_effects: 0,
        },
    )?;
    let result = ResultCode::from_raw(-77_777);
    if result.raw() != -77_777 {
        return Err("Rust contract changed a future result code".to_owned());
    }
    assert_scenario(
        expected,
        "terminal_rejection",
        &Observation {
            state: "Failed".to_owned(),
            error: "CoreRejection".to_owned(),
            verification: "Unverified".to_owned(),
            idempotency: "Original".to_owned(),
            result_code: result.raw().to_string(),
            receipt_count: 0,
            economic_effects: 0,
        },
    )
}

fn assert_scenario(
    expected: &BTreeMap<String, Observation>,
    scenario: &str,
    actual: &Observation,
) -> Result<(), String> {
    let configured = expected
        .get(scenario)
        .ok_or_else(|| format!("scenario {scenario} is missing"))?;
    if configured == actual {
        Ok(())
    } else {
        Err(format!(
            "scenario {scenario} diverged while qualifying daemon/node: expected={} observed={}",
            configured.encode(),
            actual.encode()
        ))
    }
}

fn serve_daemon(
    listener: UnixListener,
    observations: BTreeMap<String, Observation>,
    expected_requests: usize,
) -> Result<BTreeSet<(String, String)>, String> {
    let mut seen = BTreeSet::new();
    for incoming in listener.incoming().take(expected_requests) {
        let mut stream = incoming.map_err(|error| format!("daemon accept failed: {error}"))?;
        let mut request = String::new();
        BufReader::new(
            stream
                .try_clone()
                .map_err(|error| format!("daemon clone failed: {error}"))?,
        )
        .read_line(&mut request)
        .map_err(|error| format!("daemon request failed: {error}"))?;
        let (language, scenario) = request
            .trim_end()
            .split_once('\t')
            .ok_or_else(|| "daemon request omitted language or scenario".to_owned())?;
        let observation = observations
            .get(scenario)
            .ok_or_else(|| format!("unknown parity scenario {scenario}"))?;
        if !seen.insert((language.to_owned(), scenario.to_owned())) {
            return Err(format!("duplicate parity request {language}/{scenario}"));
        }
        writeln!(stream, "{}", observation.encode())
            .map_err(|error| format!("daemon response failed: {error}"))?;
    }
    Ok(seen)
}

fn query_daemon(socket: &Path, language: &str, scenario: &str) -> Result<Observation, String> {
    let mut stream = UnixStream::connect(socket)
        .map_err(|error| format!("{language} could not connect to daemon: {error}"))?;
    writeln!(stream, "{language}\t{scenario}")
        .map_err(|error| format!("{language} daemon request failed: {error}"))?;
    let mut response = String::new();
    BufReader::new(stream)
        .read_line(&mut response)
        .map_err(|error| format!("{language} daemon response failed: {error}"))?;
    Observation::decode(response.trim_end())
}

fn freshness() -> Result<Freshness, String> {
    let batch = BatchRef::new("batch-22").map_err(|error| format!("batch: {error:?}"))?;
    Ok(Freshness {
        chain_head: Sequence(10),
        latest_sealed_batch: batch.clone(),
        latest_finalised_checkpoint: CheckpointRef::new("genesis")
            .map_err(|error| format!("checkpoint: {error:?}"))?,
        value_sequence: Sequence(10),
        relative_to: RelativeTo::Batch(batch),
    })
}

fn validate_rust_sdk(scenario: &str, observation: &Observation) -> Result<(), String> {
    let client = RustSdkClient::daemon("parity.sock", ContractVersion { major: 1, minor: 1 })
        .map_err(|error| format!("Rust SDK daemon client: {error:?}"))?;
    if client.deployment() != Deployment::Daemon {
        return Err("Rust SDK did not select daemon deployment".to_owned());
    }
    let sdk_tenant = || {
        SdkTenantId::new("parity-tenant")
            .map_err(|error| format!("Rust SDK approval tenant: {error:?}"))
    };
    let approval_id = ApprovalId::new([7; 32]);
    let operation = match scenario {
        "approval_list" => client
            .approval_list(
                ApprovalListRequest::new(sdk_tenant()?, None, 50)
                    .map_err(|error| format!("Rust SDK approval list: {error:?}"))?,
            )
            .operation(),
        "approval_get" => client
            .approval_get(ApprovalGetRequest {
                tenant: sdk_tenant()?,
                approval_id,
            })
            .operation(),
        "approval_approve" => client
            .approval_approve(ApprovalApproveRequest {
                tenant: sdk_tenant()?,
                approval_id,
                idempotency_key: DecisionKey::new("approve-7")
                    .map_err(|error| format!("Rust SDK approval key: {error:?}"))?,
            })
            .operation(),
        "approval_reject" => client
            .approval_reject(
                ApprovalRejectRequest::new(
                    sdk_tenant()?,
                    approval_id,
                    DecisionKey::new("reject-7")
                        .map_err(|error| format!("Rust SDK rejection key: {error:?}"))?,
                    "not expected",
                )
                .map_err(|error| format!("Rust SDK approval rejection: {error:?}"))?,
            )
            .operation(),
        _ => client
            .track(TrackRequest {
                submission_ref: SubmissionRef::new(scenario)
                    .map_err(|error| format!("Rust SDK submission ref: {error:?}"))?,
            })
            .operation(),
    };
    let expected_operation = match scenario {
        "approval_list" => Operation::ApprovalList,
        "approval_get" => Operation::ApprovalGet,
        "approval_approve" => Operation::ApprovalApprove,
        "approval_reject" => Operation::ApprovalReject,
        _ => Operation::Track,
    };
    if operation != expected_operation {
        return Err("Rust SDK changed parity operation".to_owned());
    }
    match scenario {
        "unknown_submission" if observation.state != "Unknown" => {
            Err("Rust SDK collapsed unknown submission".to_owned())
        }
        "terminal_rejection"
            if ResultCode::from_raw(
                observation
                    .result_code
                    .parse()
                    .map_err(|_| "Rust SDK invalid result code".to_owned())?,
            )
            .raw()
                != -77_777 =>
        {
            Err("Rust SDK changed terminal result code".to_owned())
        }
        "proven_read" => {
            let value = BalanceValue {
                account: AccountRef::new("account-a")
                    .map_err(|error| format!("account: {error:?}"))?,
                asset: layerx_agent_api::identity::Asset::new("LXP")
                    .map_err(|error| format!("asset: {error:?}"))?,
                amount: Amount(1),
                canonical_state: layerx_agent_api::prepare::CanonicalBytes::new(vec![1])
                    .map_err(|error| format!("state: {error:?}"))?,
            };
            RustSdkClient::accept_verified_read(
                Level::StateProven,
                VerifiedRead::new(value, Level::StateProven, freshness()?),
            )
            .map(|_| ())
            .map_err(|error| format!("Rust SDK refused proven read: {error:?}"))
        }
        "availability_failure" if observation.error != "UnavailableCapability" => {
            Err("Rust SDK changed availability error".to_owned())
        }
        "subscription_gap" if observation.state != "Gap" => {
            Err("Rust SDK hid subscription gap".to_owned())
        }
        value
            if value.starts_with("idempotency_")
                && (observation.receipt_count != 1 || observation.economic_effects != 1) =>
        {
            Err("Rust SDK observed duplicate economic effects".to_owned())
        }
        value if value.starts_with("approval_event_") => {
            if ApprovalEventKind::ALL
                .iter()
                .any(|kind| kind.name() == observation.state)
            {
                Ok(())
            } else {
                Err("Rust SDK approval event vocabulary diverged".to_owned())
            }
        }
        value if value.starts_with("approval_outcome_") => {
            if ApprovalDecisionOutcome::ALL
                .iter()
                .any(|outcome| outcome.name() == observation.state)
            {
                Ok(())
            } else {
                Err("Rust SDK approval outcome vocabulary diverged".to_owned())
            }
        }
        "approval_list" | "approval_get" | "approval_approve" | "approval_reject"
            if observation.state != operation.name() =>
        {
            Err("Rust SDK approval operation vocabulary diverged".to_owned())
        }
        _ => Ok(()),
    }
}

fn run_rust(socket: &Path, scenarios: &[String]) -> Result<BTreeMap<String, Observation>, String> {
    scenarios
        .iter()
        .map(|scenario| {
            let observation = query_daemon(socket, "rust", scenario)?;
            validate_rust_sdk(scenario, &observation)?;
            Ok((scenario.clone(), observation))
        })
        .collect()
}

fn run_external(
    language: &str,
    mut command: Command,
    expected: &BTreeMap<String, Observation>,
) -> Result<BTreeMap<String, Observation>, String> {
    let output = command
        .output()
        .map_err(|error| format!("could not run {language} SDK: {error}"))?;
    if !output.status.success() {
        return Err(format!(
            "{language} SDK failed: status={} stderr={}",
            output.status,
            String::from_utf8_lossy(&output.stderr).trim()
        ));
    }
    let stdout = String::from_utf8(output.stdout)
        .map_err(|_| format!("{language} SDK emitted non-UTF-8 output"))?;
    let observations = stdout
        .lines()
        .map(|line| {
            let (scenario, encoded) = line
                .split_once('\t')
                .ok_or_else(|| format!("{language} emitted malformed result {line}"))?;
            Ok((scenario.to_owned(), Observation::decode(encoded)?))
        })
        .collect::<Result<BTreeMap<_, _>, String>>()?;
    if observations.len() != expected.len() {
        return Err(format!(
            "{language} returned {} scenarios, expected {}",
            observations.len(),
            expected.len()
        ));
    }
    Ok(observations)
}

fn compare_language(
    language: &str,
    expected: &BTreeMap<String, Observation>,
    observed: &BTreeMap<String, Observation>,
) -> Result<(), String> {
    for (scenario, expected_value) in expected {
        let observed_value = observed
            .get(scenario)
            .ok_or_else(|| format!("scenario={scenario} language={language} missing result"))?;
        if observed_value != expected_value {
            return Err(format!(
                "scenario={scenario} language={language} expected={} observed={}",
                expected_value.encode(),
                observed_value.encode()
            ));
        }
    }
    Ok(())
}

/// Runs every SDK against one live daemon and one core-linked node fixture.
///
/// # Errors
///
/// Names the scenario, language and differing observations on any divergence.
pub fn agent_sdk_parity_suite(node_executable: &Path, repository: &Path) -> Result<String, String> {
    if !node_executable.is_file() {
        return Err("parity suite requires the core-linked layerxd executable".to_owned());
    }
    let expected = parse_scenarios(&repository.join("agent/tests/parity/scenarios.kvx"))?;
    if expected.len() != 21 {
        return Err(format!(
            "parity suite expected 21 scenarios, got {}",
            expected.len()
        ));
    }
    qualify_state_semantics(&expected)?;
    let directory = std::env::temp_dir().join(format!("layerx-sdk-parity-{}", std::process::id()));
    if directory.exists() {
        fs::remove_dir_all(&directory)
            .map_err(|error| format!("could not clear parity directory: {error}"))?;
    }
    fs::create_dir(&directory)
        .map_err(|error| format!("could not create parity directory: {error}"))?;
    qualify_idempotency(&directory, &expected)?;
    let live_node = qualify_node(node_executable, &directory, &expected)?;

    let daemon_socket = directory.join("agentd-parity.sock");
    let listener = UnixListener::bind(&daemon_socket)
        .map_err(|error| format!("could not bind parity daemon: {error}"))?;
    let daemon_observations = expected.clone();
    let expected_requests = expected.len() * 3;
    let daemon =
        thread::spawn(move || serve_daemon(listener, daemon_observations, expected_requests));
    let scenarios = expected.keys().cloned().collect::<Vec<_>>();
    let joined = scenarios.join(",");
    let rust = run_rust(&daemon_socket, &scenarios)?;

    let mut typescript_command = Command::new("node");
    typescript_command
        .arg(repository.join("agent/tests/parity/typescript.mjs"))
        .arg(&daemon_socket)
        .arg(&joined);
    let typescript = run_external("typescript", typescript_command, &expected)?;

    let mut python_command = Command::new("python3");
    python_command
        .arg(repository.join("agent/tests/parity/python.py"))
        .arg(&daemon_socket)
        .arg(&joined)
        .env("PYTHONPATH", repository.join("agent/sdk/python"));
    let python = run_external("python", python_command, &expected)?;

    compare_language("rust", &expected, &rust)?;
    compare_language("typescript", &expected, &typescript)?;
    compare_language("python", &expected, &python)?;
    let seen = daemon
        .join()
        .map_err(|_| "parity daemon panicked".to_owned())??;
    if seen.len() != expected_requests {
        return Err(format!(
            "parity daemon observed {} unique requests, expected {expected_requests}",
            seen.len()
        ));
    }
    live_node.stop()?;
    fs::remove_dir_all(&directory)
        .map_err(|error| format!("could not clean parity directory: {error}"))?;
    Ok(format!(
        "agent SDK parity passed: {} scenarios, 3 SDKs, one daemon, one core-linked node",
        expected.len()
    ))
}

fn main() -> ExitCode {
    let mut arguments = std::env::args_os();
    let _program = arguments.next();
    let Some(node) = arguments.next().map(PathBuf::from) else {
        eprintln!("missing layerxd executable path");
        return ExitCode::FAILURE;
    };
    let Some(repository) = arguments.next().map(PathBuf::from) else {
        eprintln!("missing repository root");
        return ExitCode::FAILURE;
    };
    if arguments.next().is_some() {
        eprintln!("unexpected parity-suite argument");
        return ExitCode::FAILURE;
    }
    match agent_sdk_parity_suite(&node, &repository) {
        Ok(report) => {
            println!("{report}");
            ExitCode::SUCCESS
        }
        Err(error) => {
            eprintln!("SDK parity failed: {error}");
            ExitCode::FAILURE
        }
    }
}
