use std::error::Error as StdError;
use std::fs::{self, DirBuilder};
use std::io::{Read, Write};
use std::net::TcpListener;
use std::os::unix::fs::{DirBuilderExt, MetadataExt, PermissionsExt};
use std::os::unix::net::UnixStream;
use std::path::PathBuf;
use std::sync::Arc;
use std::thread;
use std::time::Duration;

use k256::ecdsa::SigningKey;
use layerx_human_service::journeys::DepositRuntime;
use layerx_human_service::server::movement_provider::{
    MovementProviderCodec, MovementProviderConfig, MovementProviderRequest as Request,
    MovementProviderResponse as Response, NativeMovementCodec, PlanningContext, PlanningRequest,
    UnixMovementProvider,
};
use layerx_human_service::store::{AgentTenantId, PrincipalId};
use layerx_human_service::trace::TraceId;
use layerx_paxeer_client::{
    ChainSignal, CheckpointProof, DepositProofConfig, EndpointConfig, EndpointTransport,
    FinalityTracker, ProofFault, TrackerConfig, TransactionHash, WithdrawalAttestation,
    WithdrawalBoundary, WithdrawalConfig,
};
use layerx_types::intent::EvmAddress;
use sha2::{Digest, Sha256};
use sha3::Keccak256;

use crate::config::{hex_string, Config, MAX_FRAME};
use crate::journal::{read_private, Journal};
use crate::listener::{Listener, ListenerConfig};
use crate::service::EvidenceService;
use crate::Error;

type Result<T = ()> = std::result::Result<T, Box<dyn StdError>>;
fn checked<T, E: std::fmt::Debug>(value: std::result::Result<T, E>) -> Result<T> {
    value.map_err(|error| format!("{error:?}").into())
}

struct Directory(PathBuf);
impl Directory {
    fn new() -> Result<Self> {
        let mut nonce = [0; 8];
        checked(getrandom::fill(&mut nonce))?;
        let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .ancestors()
            .nth(3)
            .ok_or("repository root")?
            .join("qual-logs/hm5");
        fs::create_dir_all(&root)?;
        let path = root.join(format!(
            "test-{}-{}",
            std::process::id(),
            hex_string(&nonce)
        ));
        DirBuilder::new().mode(0o700).create(&path)?;
        Ok(Self(path))
    }
}
impl Drop for Directory {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}
fn random_seed() -> Result<[u8; 32]> {
    let mut bytes = [0; 32];
    checked(getrandom::fill(&mut bytes))?;
    Ok(bytes)
}
fn listener_config(dir: &Directory) -> ListenerConfig {
    ListenerConfig {
        socket: dir.0.join("movement.sock"),
        allowed_uid: rustix::process::geteuid().as_raw(),
        allowed_gid: rustix::process::getegid().as_raw(),
        maximum_frame_bytes: MAX_FRAME,
        deadline: Duration::from_millis(100),
        protocol: 2,
    }
}
fn config(dir: &Directory) -> Result<Config> {
    let closed = TcpListener::bind("127.0.0.1:0")?;
    let endpoint = EndpointConfig {
        url: format!("http://{}", closed.local_addr()?),
        request_timeout: Duration::from_millis(50),
        transport: EndpointTransport::LocalEmulator,
        expected_chain_id: 31337,
    };
    drop(closed);
    let evidence_root = dir.0.join("evidence");
    DirBuilder::new().mode(0o700).create(&evidence_root)?;
    Ok(Config {
        listener: listener_config(dir),
        state_root: dir.0.join("state"),
        evidence_root,
        tracker: TrackerConfig {
            endpoints: vec![endpoint.clone()],
            minimum_endpoint_agreement: 1,
            required_confirmations: 2,
            poll_cadence: Duration::from_secs(1),
            delayed_after_polls: 2,
        },
        proof: DepositProofConfig {
            endpoints: vec![endpoint],
            minimum_endpoint_agreement: 1,
            required_confirmations: 2,
            paxeer_chain_id: 31337,
            paxeer_checkpoint_authority: ed25519_dalek::SigningKey::from_bytes(&random_seed()?)
                .verifying_key()
                .to_bytes(),
            custody_reference: [9; 32],
            layerx_network_id: 77,
            layerx_protocol_version: 2,
        },
        vault: EvmAddress::new([8; 20]),
        checkpoint_registry: EvmAddress::new([12; 20]),
        claims_contract: EvmAddress::new([7; 20]),
        exit_contract: EvmAddress::new([13; 20]),
        executor: None,
        checkpoint_interval_seconds: 10,
        paxeer_block_seconds: 1,
        reminder_interval_seconds: 60,
    })
}
fn client(config: &Config) -> Result<UnixMovementProvider> {
    checked(UnixMovementProvider::new(
        MovementProviderConfig {
            socket: config.listener.socket.clone(),
            peer_uid: rustix::process::geteuid().as_raw(),
            peer_gid: rustix::process::getegid().as_raw(),
            maximum_frame_bytes: MAX_FRAME,
            deadline: Duration::from_secs(2),
        },
        Arc::new(checked(NativeMovementCodec::for_protocol(
            config.listener.protocol,
        ))?),
        checked(WithdrawalBoundary::new_for_protocol(
            WithdrawalConfig {
                endpoints: config.tracker.endpoints.clone(),
                minimum_endpoint_agreement: config.tracker.minimum_endpoint_agreement,
                claims_contract: EvmAddress::new([7; 20]),
                required_confirmations: 2,
                poll_cadence: Duration::from_secs(1),
                delayed_after_polls: 2,
            },
            config.listener.protocol,
        ))?,
    ))
}
fn plan(operation: &str) -> Result<PlanningRequest> {
    let money = serde_json::json!({"amount":"10", "currency":"LXP"});
    let body = match operation {
        "move.quote" => {
            serde_json::json!({"source":"account", "destination":"managed-agent", "money":money})
        }
        "deposit.start" => serde_json::json!({"money":money}),
        "withdraw.start" => {
            serde_json::json!({"money":money, "destination":"0x1111111111111111111111111111111111111111"})
        }
        "exit.start" => serde_json::json!({"confirmation":"GET MY MONEY OUT"}),
        _ => return Err("unsupported test operation".into()),
    };
    checked(PlanningRequest::from_wire_parts(PlanningRequest {
        principal: checked(PrincipalId::new("alice"))?,
        tenant: checked(AgentTenantId::new("tenant-alice"))?,
        context: untrusted_context()?,
        operation: operation.to_owned(),
        idempotency_key: [3; 32],
        canonical_body: serde_json::to_vec(&body)?,
        trace: checked(TraceId::parse("trc_0123456789abcdef0123456789abcdef"))?,
        now: 100,
    }))
}

fn untrusted_context() -> Result<PlanningContext> {
    use layerx_agent_api::identity::{AgentDid, AuthorityRef};
    use layerx_human_service::custody::KeyId;
    use layerx_types::{account::AccountId, amount::Amount, ids::AssetId};
    Ok(PlanningContext {
        account: checked(AccountId::parse("agent:did:layerx:alice:main"))?,
        reserve: checked(AccountId::parse("system:paxeer-reserve"))?,
        withdrawals_account: checked(AccountId::parse("system:paxeer-withdrawals"))?,
        route: None,
        amount: Amount::from_u128(10),
        asset: AssetId::new([1; 32]),
        currency: "LXP".to_owned(),
        actor: checked(AgentDid::new("did:layerx:alice"))?,
        authority: checked(AuthorityRef::new("primary"))?,
        custody_key: checked(KeyId::new("human-primary"))?,
        custody_provider_reference: vec![1],
        custody_binding_digest: [2; 32],
        wallet: EvmAddress::new([3; 20]),
        network: checked(layerx_types::intent::NetworkId::new(77))?,
        protocol_version: 2,
        paxeer_chain_id: 31337,
        account_sequence: 1,
        budget_grant: None,
        fee_limit: 10,
        evm_gas_limit: 100_000,
        evm_max_fee_per_gas: 100,
        evm_max_priority_fee_per_gas: 1,
        not_before: 100,
        not_after: 200,
        binding_receipt_digest: [4; 32],
        identity_authority_evidence: vec![0],
        balance_evidence: vec![0],
    })
}

#[test]
fn real_client_refuses_unavailable_plans_and_reads_actual_endpoint_failure() -> Result {
    let dir = Directory::new()?;
    let config = config(&dir)?;
    let journal = Journal::open(&config.state_root, 2)?;
    let mut service = EvidenceService::new(&config, journal)?;
    let listener = Listener::bind(listener_config(&dir))?;
    let mut client = client(&config)?;
    let server = thread::spawn(move || {
        for _ in 0..7 {
            listener.serve_next(&mut service)?;
        }
        Ok::<_, Error>(())
    });
    assert!(!client.ready());
    assert!(client.move_plan(plan("move.quote")?).is_err());
    assert!(client.deposit_plan(plan("deposit.start")?).is_err());
    assert!(client.withdrawal_plan(plan("withdraw.start")?).is_err());
    assert!(client.exit_plan(plan("exit.start")?).is_err());
    let transaction = TransactionHash::new([4; 32]);
    let report = checked(client.poll_finality(transaction))?;
    assert!(matches!(report.signal(), ChainSignal::Unreachable { .. }));
    assert_eq!(report.polls(), 1);
    assert_eq!(
        client.obtain_proof(transaction),
        Err(layerx_paxeer_client::DepositFailure::ProofUnavailable(
            ProofFault::ProducerUnavailable
        ))
    );
    server.join().map_err(|_| "server panicked")??;
    let journal = Journal::open(&config.state_root, 2)?;
    let bytes = checked(
        NativeMovementCodec::new().encode_request(&Request::PollDepositFinality(transaction)),
    )?;
    let key = hex_string(&Sha256::digest(bytes));
    let response = journal
        .record(&key)
        .ok_or("missing poll record")?
        .response
        .as_ref()
        .ok_or("missing response")?;
    let Response::DepositFinality(saved) =
        checked(NativeMovementCodec::new().decode_response(response))?
    else {
        return Err("wrong durable reply".into());
    };
    assert_eq!(saved, report);
    Ok(())
}

#[test]
fn wrong_uid_and_oversized_frames_are_refused_before_payload_allocation() -> Result {
    for wrong_uid in [true, false] {
        let dir = Directory::new()?;
        let config = config(&dir)?;
        let mut service = EvidenceService::new(&config, Journal::open(&config.state_root, 2)?)?;
        let mut policy = listener_config(&dir);
        if wrong_uid {
            policy.allowed_uid ^= 1;
        }
        let socket = policy.socket.clone();
        let listener = Listener::bind(policy)?;
        let server = thread::spawn(move || listener.serve_next(&mut service));
        let mut stream = UnixStream::connect(socket)?;
        stream.set_read_timeout(Some(Duration::from_secs(2)))?;
        let mut header = layerx_human_service::server::movement_provider::MOVEMENT_PROTOCOL_VERSION
            .to_be_bytes()
            .to_vec();
        header.extend(if wrong_uid { 2_u64 } else { u64::MAX }.to_be_bytes());
        let _ = stream.write_all(&header);
        let _ = stream.write_all(&[2, 14]);
        let mut reply = [0; 1];
        assert!(!matches!(stream.read(&mut reply), Ok(1)));
        server.join().map_err(|_| "server panicked")??;
    }
    Ok(())
}

#[test]
fn partial_header_deadline_releases_the_listener() -> Result {
    let dir = Directory::new()?;
    let config = config(&dir)?;
    let mut service = EvidenceService::new(&config, Journal::open(&config.state_root, 2)?)?;
    let listener = Listener::bind(listener_config(&dir))?;
    let client = client(&config)?;
    let server = thread::spawn(move || {
        listener.serve_next(&mut service)?;
        listener.serve_next(&mut service)
    });
    let mut stalled = UnixStream::connect(&config.listener.socket)?;
    stalled.write_all(&[0])?;
    stalled.set_read_timeout(Some(Duration::from_secs(2)))?;
    let mut byte = [0];
    assert!(!matches!(stalled.read(&mut byte), Ok(1)));
    assert!(!client.ready());
    server.join().map_err(|_| "server panicked")??;
    Ok(())
}

#[test]
fn listener_does_not_replace_live_socket_or_follow_symlinks() -> Result {
    let dir = Directory::new()?;
    let listener = Listener::bind(listener_config(&dir))?;
    assert!(matches!(
        Listener::bind(listener_config(&dir)),
        Err(Error::Conflict)
    ));
    assert_eq!(
        fs::metadata(dir.0.join("movement.sock"))?.mode() & 0o777,
        0o660
    );
    drop(listener);
    let target = dir.0.join("unrelated");
    fs::write(&target, b"preserve")?;
    std::os::unix::fs::symlink(&target, dir.0.join("movement.sock"))?;
    assert!(Listener::bind(listener_config(&dir)).is_err());
    assert_eq!(fs::read(target)?, b"preserve");
    Ok(())
}

#[test]
fn journal_replays_pending_actions_and_refuses_conflicts_corruption_and_second_writer() -> Result {
    let dir = Directory::new()?;
    let root = dir.0.join("state");
    let key = "a".repeat(64);
    let codec = NativeMovementCodec::new();
    let request = checked(
        codec.encode_request(&Request::PollDepositFinality(TransactionHash::new([3; 32]))),
    )?;
    let other = checked(
        codec.encode_request(&Request::PollDepositFinality(TransactionHash::new([4; 32]))),
    )?;
    let mut journal = Journal::open(&root, 2)?;
    journal.begin(&key, &request)?;
    assert!(matches!(Journal::open(&root, 2), Err(Error::Conflict)));
    assert!(matches!(journal.begin(&key, &other), Err(Error::Conflict)));
    drop(journal);
    let mut journal = Journal::open(&root, 2)?;
    assert_eq!(journal.record(&key).ok_or("record")?.request, request);
    assert!(journal.record(&key).ok_or("record")?.response.is_none());
    journal.complete(
        &key,
        &checked(codec.encode_response(&Response::Unavailable))?,
    )?;
    drop(journal);
    assert!(Journal::open(&root, 3).is_err());
    let journal = Journal::open(&root, 2)?;
    assert!(journal.record(&key).ok_or("record")?.response.is_some());
    assert_eq!(
        fs::metadata(root.join("journal.bin"))?.mode() & 0o777,
        0o600
    );
    drop(journal);
    let mut bytes = fs::read(root.join("journal.bin"))?;
    let last = bytes.last_mut().ok_or("journal empty")?;
    *last ^= 1;
    fs::write(root.join("journal.bin"), bytes)?;
    assert!(Journal::open(&root, 2).is_err());
    Ok(())
}

#[test]
fn private_files_refuse_symlinks_hardlinks_and_public_permissions() -> Result {
    let dir = Directory::new()?;
    let path = dir.0.join("proof.bin");
    fs::write(&path, b"not proof material")?;
    fs::set_permissions(&path, fs::Permissions::from_mode(0o600))?;
    assert!(read_private(&path, 1).is_err());
    let link = dir.0.join("link.bin");
    std::os::unix::fs::symlink(&path, &link)?;
    assert!(read_private(&link, MAX_FRAME).is_err());
    fs::remove_file(&link)?;
    fs::hard_link(&path, &link)?;
    assert!(read_private(&path, MAX_FRAME).is_err());
    fs::remove_file(link)?;
    fs::set_permissions(&path, fs::Permissions::from_mode(0o644))?;
    assert!(read_private(&path, MAX_FRAME).is_err());
    Ok(())
}

#[test]
fn signed_native_checkpoint_proofs_replay_without_converting_them_to_authority() -> Result {
    for protocol in [2, 3] {
        let dir = Directory::new()?;
        let codec = checked(NativeMovementCodec::for_protocol(protocol))?;
        let proof = signed_checkpoint(protocol)?;
        let response =
            checked(codec.encode_response(&Response::CheckpointProof(Some(proof.clone()))))?;
        let mut journal = Journal::open(&dir.0.join("state"), protocol)?;
        let key = "b".repeat(64);
        journal.begin(
            &key,
            &checked(codec.encode_request(&Request::CheckpointProof(
                layerx_paxeer_client::DebitExpectation {
                    activity_id: [5; 32],
                    network_id: 77,
                    withdrawal_id: [6; 32],
                    account: [7; 32],
                    withdrawals_account: [8; 32],
                    asset_id: [9; 32],
                    amount: 10,
                    recipient: EvmAddress::new([10; 20]),
                },
            )))?,
        )?;
        journal.complete(&key, &response)?;
        drop(journal);
        let journal = Journal::open(&dir.0.join("state"), protocol)?;
        assert_eq!(
            journal.record(&key).ok_or("record")?.response.as_ref(),
            Some(&response)
        );
        let mut invalid = proof;
        invalid.attestations[0].signature_r[0] ^= 1;
        let invalid_response = Response::CheckpointProof(Some(invalid));
        let invalid_bytes = checked(codec.encode_response(&invalid_response))?;
        let decoded = checked(codec.decode_response(&invalid_bytes))?;
        assert_eq!(checked(codec.encode_response(&decoded))?, invalid_bytes);
    }
    Ok(())
}

pub(crate) fn signed_checkpoint(protocol: u16) -> Result<CheckpointProof> {
    let key = checked(SigningKey::from_slice(&random_seed()?))?;
    let point = key.verifying_key().to_encoded_point(false);
    let hash = Keccak256::digest(&point.as_bytes()[1..]);
    let mut attestation = WithdrawalAttestation {
        protocol_version: protocol,
        network_id: 77,
        paxeer_chain_id: 31337,
        settlement_contract: EvmAddress::new([8; 20]),
        epoch: 1,
        checkpoint_id: [1; 32],
        checkpoint_hash: [1; 32],
        guarantor_id: [2; 32],
        batch_number: 1,
        data_availability_root: [3; 32],
        replayed: true,
        data_available: true,
        availability_class_mask: 31,
        attested_at: 1,
        signer: EvmAddress::new(checked(hash[12..].try_into())?),
        signature_r: [0; 32],
        signature_s: [0; 32],
        signature_v: 27,
    };
    let mut message = Vec::new();
    message.extend(protocol.to_be_bytes());
    message.extend(attestation.network_id.to_be_bytes());
    message.extend(attestation.paxeer_chain_id.to_be_bytes());
    message.extend(attestation.settlement_contract.bytes());
    message.extend(attestation.epoch.to_be_bytes());
    message.extend(attestation.checkpoint_id);
    message.extend(attestation.checkpoint_hash);
    message.extend(attestation.guarantor_id);
    message.extend(attestation.batch_number.to_be_bytes());
    message.extend(attestation.data_availability_root);
    message.extend([1, 1, 31]);
    message.extend(attestation.attested_at.to_be_bytes());
    let mut digest = Sha256::new();
    digest.update(b"LXP/v2/guarantor-attestation\0");
    digest.update(message);
    let (signature, recovery) = checked(key.sign_prehash_recoverable(&digest.finalize()))?;
    attestation
        .signature_r
        .copy_from_slice(&signature.to_bytes()[..32]);
    attestation
        .signature_s
        .copy_from_slice(&signature.to_bytes()[32..]);
    attestation.signature_v = recovery.to_byte() + 27;
    checked(CheckpointProof::validated_for_protocol(
        protocol,
        [1; 32],
        [4; 32],
        1,
        1,
        [3; 32],
        0,
        Vec::new(),
        vec![attestation],
    ))
}

#[test]
fn malformed_proof_candidates_are_refused_over_the_real_socket() -> Result {
    let dir = Directory::new()?;
    let config = config(&dir)?;
    let transaction = TransactionHash::new([5; 32]);
    let path = config
        .evidence_root
        .join(format!("deposit-{}.bin", hex_string(&transaction.bytes())));
    fs::write(&path, b"unverifiable")?;
    fs::set_permissions(path, fs::Permissions::from_mode(0o600))?;
    let mut service = EvidenceService::new(&config, Journal::open(&config.state_root, 2)?)?;
    let listener = Listener::bind(listener_config(&dir))?;
    let mut client = client(&config)?;
    let server = thread::spawn(move || listener.serve_next(&mut service));
    assert_eq!(
        client.obtain_proof(transaction),
        Err(layerx_paxeer_client::DepositFailure::ProofUnavailable(
            ProofFault::EvidenceSourceMismatch
        ))
    );
    server.join().map_err(|_| "server panicked")??;
    Ok(())
}

#[test]
#[ignore = "requires LAYERX_HUMAN_MOVEMENT_TEST_PAXEER_RPC and a real signed deposit proof producer"]
fn live_paxeer_deposit_proof_is_reverified_after_restart() -> Result {
    let endpoint = std::env::var("LAYERX_HUMAN_MOVEMENT_TEST_PAXEER_RPC")?;
    let transaction = checked(TransactionHash::from_hex(&std::env::var(
        "LAYERX_HUMAN_MOVEMENT_TEST_TRANSACTION",
    )?))?;
    let dir = Directory::new()?;
    let mut config = Config::from_environment()?;
    if config.proof.paxeer_chain_id == 125
        || !config
            .proof
            .endpoints
            .iter()
            .any(|value| value.url == endpoint)
    {
        return Err(
            "test requires an explicitly designated disposable chain other than 125".into(),
        );
    }
    config.state_root = dir.0.join("state");
    config.listener.socket = dir.0.join("movement.sock");
    config.listener.allowed_uid = rustix::process::geteuid().as_raw();
    config.listener.allowed_gid = rustix::process::getegid().as_raw();
    let mut prior = None;
    for _ in 0..2 {
        let mut service = EvidenceService::new(
            &config,
            Journal::open(&config.state_root, config.listener.protocol)?,
        )?;
        let listener = Listener::bind(ListenerConfig {
            socket: config.listener.socket.clone(),
            allowed_uid: config.listener.allowed_uid,
            allowed_gid: config.listener.allowed_gid,
            maximum_frame_bytes: MAX_FRAME,
            deadline: config.listener.deadline,
            protocol: config.listener.protocol,
        })?;
        let mut client = client(&config)?;
        let server = thread::spawn(move || listener.serve_next(&mut service));
        let result = client.obtain_proof(transaction);
        server.join().map_err(|_| "server panicked")??;
        let proof = checked(result)?;
        assert_eq!(proof.transaction(), transaction);
        if let Some(prior) = prior {
            assert_eq!(proof.nullifier(), prior);
        }
        prior = Some(proof.nullifier());
    }
    Ok(())
}

#[test]
fn tracker_configuration_refuses_insecure_production_and_duplicate_endpoints() -> Result {
    let dir = Directory::new()?;
    let mut config = config(&dir)?;
    config
        .tracker
        .endpoints
        .push(config.tracker.endpoints[0].clone());
    config.tracker.minimum_endpoint_agreement = 2;
    assert!(FinalityTracker::new(config.tracker.clone(), TransactionHash::new([1; 32])).is_err());
    config.tracker.endpoints.truncate(1);
    config.tracker.endpoints[0].url = "http://paxeer.example.invalid".to_owned();
    assert!(FinalityTracker::new(config.tracker, TransactionHash::new([1; 32])).is_err());
    Ok(())
}

#[test]
fn stale_owned_socket_recovers_but_unmanaged_live_socket_is_preserved() -> Result {
    let dir = Directory::new()?;
    let socket = dir.0.join("movement.sock");
    let live = std::os::unix::net::UnixListener::bind(&socket)?;
    assert!(Listener::bind(listener_config(&dir)).is_err());
    assert!(socket.exists());
    drop(live);
    let recovered = Listener::bind(listener_config(&dir))?;
    assert!(socket.exists());
    drop(recovered);
    assert!(!socket.exists());
    Ok(())
}

#[test]
fn drip_fed_payload_cannot_extend_absolute_connection_deadline() -> Result {
    let dir = Directory::new()?;
    let config = config(&dir)?;
    let mut service = EvidenceService::new(&config, Journal::open(&config.state_root, 2)?)?;
    let listener = Listener::bind(listener_config(&dir))?;
    let server = thread::spawn(move || listener.serve_next(&mut service));
    let mut stream = UnixStream::connect(&config.listener.socket)?;
    stream.write_all(&1_u16.to_be_bytes())?;
    stream.write_all(&(MAX_FRAME as u64).to_be_bytes())?;
    let mut closed = false;
    for _ in 0..40 {
        if stream.write_all(&[1]).is_err() {
            closed = true;
            break;
        }
        thread::sleep(Duration::from_millis(20));
    }
    drop(stream);
    server.join().map_err(|_| "server panicked")??;
    assert!(
        closed,
        "payload trickle extended connection beyond its absolute deadline"
    );
    Ok(())
}

#[test]
fn restarting_server_reobserves_instead_of_serving_archived_finality() -> Result {
    let dir = Directory::new()?;
    let config = config(&dir)?;
    let transaction = TransactionHash::new([6; 32]);
    for requests in [2, 1] {
        let mut service = EvidenceService::new(&config, Journal::open(&config.state_root, 2)?)?;
        let listener = Listener::bind(listener_config(&dir))?;
        let mut client = client(&config)?;
        let server = thread::spawn(move || {
            for _ in 0..requests {
                listener.serve_next(&mut service)?;
            }
            Ok::<_, Error>(())
        });
        for expected_polls in 1..=requests {
            let report = checked(client.poll_finality(transaction))?;
            assert_eq!(report.polls(), expected_polls);
            assert!(matches!(report.signal(), ChainSignal::Unreachable { .. }));
        }
        server.join().map_err(|_| "server panicked")??;
    }
    Ok(())
}
