use std::error::Error;
use std::path::Path;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use ed25519_dalek::{Signer as _, SigningKey};
use layerx_agentd::read::{LayerxdProgramBalanceReader, NativeReadRoute, ProgramAuthority};
use layerx_client::client::{ClientConfig, ReconnectPolicy};
use layerx_client::lni::handshake::{perform, HandshakeConfig};
use layerx_client::lni::schema::{decode_envelope, encode_envelope, Envelope, Version};
use layerx_client::lni::transport::{ConnectionGate, FrameTransport, Limits, Uds};
use layerx_client::Client;
use layerx_programs::{hex, DeploymentProof, ProgramId, ProtocolDeploymentVerifier, Registry};
use layerx_proof::program::{verify_program_execution, ProgramExecutionExpectation};
use layerx_proof::receipt::verify_sequencer_signature;
use layerx_sdk::program_lifecycle::NativeProgramLifecycleRequest;
use layerx_types::account::AccountId;
use layerx_types::clock::Deadline;
use layerx_types::ids::Did;
use layerx_types::program_call::{NativeProgramCall, Resources};
use layerx_types::program_lifecycle::{NativeProgramDeploy, ProgramUpgradePolicy};
use layerx_wire::activity::decode_signed;
use layerx_wire::encode::Encoder;
use layerx_wire::hash::{activity_id, receipt_digest};
use layerx_wire::receipt::encode_unsigned;
use serde_json::Value;
use sha2::{Digest as _, Sha256};

type Result<T> = std::result::Result<T, Box<dyn Error>>;
fn checked<T, E: std::fmt::Debug>(value: std::result::Result<T, E>) -> Result<T> {
    value.map_err(|error| format!("{error:?}").into())
}
fn now() -> Result<u64> {
    Ok(u64::try_from(
        SystemTime::now().duration_since(UNIX_EPOCH)?.as_millis(),
    )?)
}
fn field<'a>(value: &'a Value, key: &str) -> Result<&'a str> {
    value[key]
        .as_str()
        .ok_or_else(|| format!("missing {key}").into())
}
fn limits() -> Limits {
    Limits {
        maximum_frame_bytes: 16_777_216,
        maximum_connections: 1,
        maximum_streams: 1,
        maximum_queued_bytes: 16_777_216,
        deadline: Duration::from_secs(8),
    }
}
fn configuration(socket: &Path) -> ClientConfig {
    ClientConfig {
        endpoint: socket.to_owned(),
        handshake: HandshakeConfig {
            built_interface_version: Version::V1_5,
            expected_protocol_version: 3,
            expected_network_id: 77,
        },
        limits: limits(),
        reconnect: ReconnectPolicy {
            maximum_attempts: 4,
            base_delay: Duration::from_millis(10),
            maximum_delay: Duration::from_millis(100),
            jitter_percent: 10,
        },
    }
}
fn domain(label: &[u8], bytes: &[u8]) -> [u8; 32] {
    let mut hash = Sha256::new();
    hash.update(b"LXP/v1/");
    hash.update(label);
    hash.update([0]);
    hash.update(bytes);
    hash.finalize().into()
}
fn signed(
    client: &mut Client,
    route: &mut NativeReadRoute,
    key: &SigningKey,
    did: &Did,
    ordinal: u16,
    payload: &[u8],
) -> Result<Vec<u8>> {
    let history = checked(route.signed_authority())?.ok_or("missing fee authority")?;
    checked(client.reconnect())?;
    let fee_asset = checked(client.native_fee_policy(398))?.value.asset.asset_id;
    let account_id = checked(layerx_wire::hash::account_id_for_protocol(
        &checked(AccountId::parse(&format!(
            "agent:{}:main",
            std::str::from_utf8(did.as_bytes())?
        )))?,
        3,
    ))?;
    let account = checked(client.account_with_history(
        account_id,
        layerx_types::verify::VerificationLevel::BATCH_INCLUDED,
        399,
        &history,
    ))?;
    let account = checked(layerx_proof::state::decode_account_value(
        account_id,
        account.canonical_bytes(),
    ))?;
    assert!(!account.frozen);
    let held = account.asset.ok_or("missing funded fee asset")?;
    assert_eq!(held.asset_id, fee_asset);
    let fee_limit = held.balance.min(1_000_000_000_000);
    let state = checked(client.preparation_state(did, 400))?;
    assert_eq!(state.kernel_epoch, 2);
    let timestamp = now()?;
    let public = key.verifying_key().to_bytes();
    let mut nonce = payload.to_vec();
    nonce.extend_from_slice(&state.account_sequence.to_be_bytes());
    let nonce: [u8; 32] = Sha256::digest(nonce).into();
    let mut fields = Encoder::new(1_048_576);
    checked(fields.tag(1, 12))?;
    checked(fields.u16(3))?;
    checked(fields.tag(2, 12))?;
    checked(fields.u32(77))?;
    checked(fields.tag(3, 12))?;
    checked(fields.u32((9 << 16) | u32::from(ordinal)))?;
    checked(fields.tag(4, 12))?;
    checked(fields.bytes(did.as_bytes(), 255))?;
    checked(fields.tag(5, 12))?;
    checked(fields.bytes(&public, 524_288))?;
    checked(fields.tag(6, 12))?;
    checked(fields.u64(state.account_sequence))?;
    checked(fields.tag(7, 12))?;
    checked(fields.u64(timestamp - 30_000))?;
    checked(fields.u64(timestamp + 120_000))?;
    checked(fields.tag(8, 12))?;
    checked(fields.bytes(&nonce, 32))?;
    checked(fields.tag(9, 12))?;
    checked(fields.u128(fee_limit))?;
    checked(fields.tag(10, 12))?;
    checked(fields.bytes(&domain(b"payload-hash", payload), 32))?;
    checked(fields.tag(11, 12))?;
    checked(fields.bytes(payload, 524_288))?;
    let fields = fields.finish();
    let mut unsigned = vec![0, 3, 0x10, 1, 11];
    unsigned.extend_from_slice(&fields);
    let signature = key.sign(&domain(b"signature-preimage", &unsigned));
    let mut signature_field = Encoder::new(70);
    checked(signature_field.tag(12, 12))?;
    checked(signature_field.bytes(&signature.to_bytes(), 128))?;
    let mut canonical = vec![0, 3, 0x10, 1, 12];
    canonical.extend_from_slice(&fields);
    canonical.extend_from_slice(&signature_field.finish());
    checked(decode_signed(&canonical, &state.module_registry))?;
    Ok(canonical)
}
fn get(endpoint: &str, token: &str, path: &str) -> Result<Option<Value>> {
    let mut response = ureq::get(format!("{endpoint}{path}"))
        .header("Authorization", &format!("Bearer {token}"))
        .config()
        .timeout_global(Some(Duration::from_secs(8)))
        .http_status_as_error(false)
        .build()
        .call()?;
    let status = response.status().as_u16();
    let value: Value = serde_json::from_str(&response.body_mut().read_to_string()?)?;
    if status == 200 {
        return Ok(Some(value));
    }
    if status == 503
        && (value["native_result"].as_i64() == Some(-106) || value["code"].as_i64() == Some(-106))
    {
        return Ok(None);
    }
    Err(format!(
        "native Programs read status {status}: {}",
        value.get("code").unwrap_or(&Value::Null)
    )
    .into())
}
fn wait_get(endpoint: &str, token: &str, path: &str) -> Result<Value> {
    let clock = layerx_client::runtime_clock::RuntimeClock::from_environment()?;
    let mut deadline = checked(Deadline::start(clock.as_ref(), Duration::from_secs(30)))?;
    loop {
        if checked(deadline.remaining(clock.as_ref()))?.is_zero() {
            return Err("Programs publication deadline".into());
        }
        if let Some(value) = get(endpoint, token, path)? {
            return Ok(value);
        }
        std::thread::sleep(Duration::from_millis(25));
    }
}
fn deployment(socket: &Path, id: [u8; 32]) -> Result<DeploymentProof> {
    let mut connection = checked(Uds::connect(socket, &ConnectionGate::new(1), limits()))?;
    checked(perform(
        &mut connection,
        &configuration(socket).handshake,
        None,
    ))?;
    let mut payload = vec![0, 1, 4];
    payload.extend_from_slice(&id);
    let request = checked(encode_envelope(Envelope {
        version: Version::V1_5,
        message_tag: 16,
        correlation_id: 900,
        canonical_payload: &payload,
        proof_material: &[],
    }))?;
    checked(connection.send(&request))?;
    let response = checked(connection.receive())?;
    let response = checked(decode_envelope(&response))?;
    assert_eq!(response.version, Version::V1_5);
    assert_eq!(response.correlation_id, 900);
    assert_eq!(
        response.message_tag,
        17,
        "deployment proof refusal: {:?}",
        layerx_client::lni::refusal::decode_core_refusal(response.canonical_payload)
    );
    assert!(response.proof_material.is_empty());
    checked(DeploymentProof::decode(response.canonical_payload))
}
fn submit(
    client: &mut Client,
    canonical: &[u8],
    did: &Did,
    public: [u8; 32],
    endpoint: &str,
    token: &str,
) -> Result<Vec<u8>> {
    checked(client.reconnect())?;
    let state = checked(client.preparation_state(did, 401))?;
    let activity = checked(decode_signed(canonical, &state.module_registry))?;
    let expected = checked(activity_id(&activity))?;
    match checked(client.submit_signed(&state.module_registry, public, 402, 1, canonical))? {
        layerx_client::submit::Submission::Acknowledged(ack) => {
            assert_eq!(ack.activity_id(), expected);
        }
        layerx_client::submit::Submission::Unknown(_) => {
            return Err("unknown Programs admission".into())
        }
    }
    let value = wait_get(
        endpoint,
        token,
        &format!(
            "/v1/programs/receipts/by-idempotency/{}",
            hex::encode(&activity.idempotency_key())
        ),
    )?;
    assert_eq!(field(&value, "activity_id")?, hex::encode(&expected));
    checked(hex::decode(field(&value, "receipt")?))
}
fn call_payload(
    program: [u8; 32],
    account: [u8; 32],
    asset: [u8; 32],
    seed: &[u8],
) -> Result<Vec<u8>> {
    let mut calldata = vec![1, 1];
    calldata.extend_from_slice(&u16::try_from(seed.len())?.to_be_bytes());
    calldata.extend_from_slice(seed);
    for field in [&account, &asset, &account, &account] {
        calldata.extend_from_slice(field);
    }
    calldata.extend_from_slice(&1_u128.to_be_bytes());
    calldata.extend_from_slice(&[0x72; 32]);
    calldata.extend_from_slice(&[0x73; 32]);
    let mut capabilities = vec![0, 4, 3, 5];
    capabilities.extend_from_slice(&asset);
    capabilities.extend_from_slice(&account);
    capabilities.extend_from_slice(&1_u128.to_be_bytes());
    capabilities.extend_from_slice(&[7, 8]);
    checked(
        NativeProgramCall {
            program_id: layerx_types::intent::ProgramId::new(program),
            guest_abi: 2,
            entrypoint: b"layerx_call",
            calldata: &calldata,
            capabilities: &capabilities,
            access_declaration: b"LayerX/programs/access-declaration/v1\0\0",
            response_capacity: 1024,
            resources: Resources([
                1_000_000, 16_777_216, 1_048_576, 1_048_576, 64, 1_048_576, 4096,
            ]),
        }
        .encode(),
    )
}

fn verify_call_artifacts(
    receipt: &[u8],
    activity: &layerx_wire::activity::Activity,
    key: [u8; 32],
    service: (&str, &str),
    program: [u8; 32],
) -> Result<()> {
    let (endpoint, token) = service;
    let signed_receipt = checked(verify_sequencer_signature(receipt, key))?;
    let protocol = signed_receipt.protocol().ok_or("CALL receipt")?;
    assert_eq!(protocol.result_code(), 0);
    assert_eq!(protocol.activity_id(), checked(activity_id(activity))?);
    let digest = checked(receipt_digest(&checked(encode_unsigned(&signed_receipt))?))?;
    let artifacts = wait_get(
        endpoint,
        token,
        &format!(
            "/v1/programs/activities/{}/artifacts?receipt_digest={}",
            hex::encode(&protocol.activity_id()),
            hex::encode(&digest)
        ),
    )?;
    checked(verify_program_execution(
        receipt,
        &checked(hex::decode(field(&artifacts, "terminal_payload")?))?,
        &checked(hex::decode(field(&artifacts, "call_graph")?))?,
        ProgramExecutionExpectation {
            sequencer_public_key: key,
            previous_state_root: protocol.previous_state_root(),
            activity_id: checked(activity_id(activity))?,
            payload_hash: activity.payload_hash(),
            program_id: program,
            guest_abi_version: 2,
        },
    ))?;
    Ok(())
}

struct Journey {
    client: Client,
    route: NativeReadRoute,
    reader: LayerxdProgramBalanceReader,
    program: [u8; 32],
    key: SigningKey,
    did: Did,
    endpoint: String,
    token: String,
    sequencer: [u8; 32],
}

fn setup(socket: &Path, directory: &Path, config: &Value) -> Result<Journey> {
    let endpoint = field(config, "endpoint")?;
    let token = std::fs::read_to_string(field(config, "token_file")?)?;
    let replica_token = std::fs::read_to_string(field(config, "replica_token_file")?)?;
    let ca = std::fs::read(field(config, "ca_file")?)?;
    let key = SigningKey::from_bytes(&[0x11; 32]);
    let public = key.verifying_key().to_bytes();
    let did = checked(Did::new(
        format!("did:layerx:{}", hex::encode(&public)).as_bytes(),
    ))?;
    let mut client = checked(Client::connect(configuration(socket)))?;
    assert_eq!(client.head().sealed_batch, 15);
    let mut route = checked(NativeReadRoute::new(
        checked(Client::connect(configuration(socket)))?,
        did.clone(),
        "native-programs-handover-history-cursor-bound".into(),
        layerx_client::runtime_clock::RuntimeClock::from_environment()?,
    ))?;
    route = checked(route.with_protected_finality(&directory.join("handover-finality.conf")))?;
    route = checked(route.with_protected_genesis(&directory.join("handover-genesis.bin")))?;
    let preceding_history = checked(route.signed_authority())?.ok_or("missing history")?;
    let protected = checked(ProtocolDeploymentVerifier::from_protected_history(
        Path::new(field(config, "trust_file")?),
        300_000,
    ))?;
    let stale_verifier = checked(protected.with_signed_history(&preceding_history))?;
    let wasm = std::fs::read(field(config, "wasm_file")?)?;
    let program = [0x71; 32];
    let deploy = NativeProgramDeploy {
        program_id: layerx_types::intent::ProgramId::new(program),
        guest_abi: 2,
        policy: ProgramUpgradePolicy::Immutable,
        new_hash: Sha256::digest(&wasm).into(),
        interface: None,
        wasm: &wasm,
    };
    let canonical = signed(
        &mut client,
        &mut route,
        &key,
        &did,
        1,
        &checked(deploy.encode())?,
    )?;
    let state = checked(client.preparation_state(&did, 403))?;
    let request = checked(NativeProgramLifecycleRequest::deploy(
        &state.module_registry,
        deploy,
        &canonical,
    ))?;
    let receipt = submit(&mut client, &canonical, &did, public, endpoint, &token)?;
    let deploy_receipt = checked(verify_sequencer_signature(
        &receipt,
        preceding_history
            .intervals()
            .last()
            .ok_or("missing current authority")?
            .public_key(),
    ))?;
    assert_eq!(
        deploy_receipt
            .protocol()
            .ok_or("Deploy receipt")?
            .result_code(),
        0
    );
    let proof = deployment(socket, request.bound_activity_id())?;
    assert_eq!(proof.activity, canonical);
    assert_eq!(proof.state.receipt, receipt);
    let history = checked(route.signed_authority())?.ok_or("missing deployed history")?;
    assert!(stale_verifier.verify_historical_deployment(&proof).is_err());
    let verifier = checked(protected.with_signed_history(&history))?;
    let evidence = checked(verifier.verify_historical_deployment(&proof))?;
    assert_eq!(evidence.program().bytes(), program);
    let mut registry = Registry::new();
    checked(registry.record_verified_deployment(&evidence))?;
    let replica_id = checked(hex::decode_digest(field(config, "replica_id")?))?;
    let mut reader = checked(LayerxdProgramBalanceReader::connect(
        endpoint,
        token.clone(),
        ProgramAuthority {
            endpoint: field(config, "replica_endpoint")?,
            authorization: replica_token,
            replica_id,
            ca_der: &ca,
        },
        protected,
        registry,
    ))?;
    assert!(reader
        .read(checked(ProgramId::new(program))?, now()?)
        .is_err());
    checked(reader.refresh_authority(&history))?;
    checked(reader.read(checked(ProgramId::new(program))?, now()?))?;
    let sequencer = history
        .intervals()
        .last()
        .ok_or("missing current authority")?
        .public_key();
    Ok(Journey {
        client,
        route,
        reader,
        program,
        key,
        did,
        endpoint: endpoint.to_owned(),
        token,
        sequencer,
    })
}

fn main() -> Result<()> {
    let args: Vec<_> = std::env::args().collect();
    if args.len() != 4 {
        return Err("usage: native_handover_programs SOCKET DIRECTORY CONFIG".into());
    }
    let socket = Path::new(&args[1]);
    let directory = Path::new(&args[2]);
    let config: Value = serde_json::from_slice(&std::fs::read(&args[3])?)?;
    let Journey {
        mut client,
        mut route,
        mut reader,
        program,
        key,
        did,
        endpoint,
        token,
        sequencer,
    } = setup(socket, directory, &config)?;
    let public = key.verifying_key().to_bytes();
    let asset = checked(hex::decode_digest(
        "b5a32b12029f8ddfb905f90f280f664b46390de0fc62770fc197dd87b18cd898",
    ))?;
    let seed = b"handover-escrow";
    let mut account_input = b"LayerX/programs/program-account/v1\0".to_vec();
    account_input.extend_from_slice(&program);
    account_input.extend_from_slice(&u32::try_from(seed.len())?.to_be_bytes());
    account_input.extend_from_slice(seed);
    let account: [u8; 32] = Sha256::digest(account_input).into();
    let mut registration = program.to_vec();
    registration.extend_from_slice(b"LXPA1");
    registration.extend_from_slice(&asset);
    registration.extend_from_slice(&u32::try_from(seed.len())?.to_be_bytes());
    registration.extend_from_slice(seed);
    let registration = signed(&mut client, &mut route, &key, &did, 6, &registration)?;
    let registered = submit(&mut client, &registration, &did, public, &endpoint, &token)?;
    let key = sequencer;
    assert_eq!(
        checked(verify_sequencer_signature(&registered, key))?
            .protocol()
            .ok_or("registration receipt")?
            .result_code(),
        0
    );
    let call = call_payload(program, account, asset, seed)?;
    let canonical = signed(
        &mut client,
        &mut route,
        &SigningKey::from_bytes(&[0x11; 32]),
        &did,
        3,
        &call,
    )?;
    let receipt = submit(&mut client, &canonical, &did, public, &endpoint, &token)?;
    checked(client.reconnect())?;
    let state = checked(client.preparation_state(&did, 403))?;
    let activity = checked(decode_signed(&canonical, &state.module_registry))?;
    verify_call_artifacts(&receipt, &activity, key, (&endpoint, &token), program)?;
    let included = checked(route.read(&format!(
        "/v1/reads/receipt/{}",
        hex::encode(&checked(activity_id(&activity))?)
    )))?;
    assert_eq!(field(&included, "canonical_hex")?, hex::encode(&receipt));
    assert_eq!(field(&included, "sequencer_public_key")?, hex::encode(&key));
    assert_eq!(
        included["verification_level"],
        layerx_types::verify::VerificationLevel::BATCH_INCLUDED.wire_rank()
    );
    let final_history = checked(route.signed_authority())?.ok_or("missing CALL history")?;
    assert!(reader
        .read(checked(ProgramId::new(program))?, now()?)
        .is_err());
    checked(reader.refresh_authority(&final_history))?;
    let before = checked(reader.read(checked(ProgramId::new(program))?, now()?))?;
    assert_eq!(
        before
            .accounts
            .iter()
            .find(|value| value.account == account)
            .ok_or("missing Program account")?
            .amount,
        1
    );
    let replay = submit(&mut client, &canonical, &did, public, &endpoint, &token)?;
    assert_eq!(replay, receipt);
    let after = checked(reader.read(checked(ProgramId::new(program))?, now()?))?;
    assert_eq!(before, after);
    println!("post-handover native Deploy, account registration, escrow CALL and idempotent replay verified through current signed Programs authority; stale and protected initial-key reads refused");
    Ok(())
}
