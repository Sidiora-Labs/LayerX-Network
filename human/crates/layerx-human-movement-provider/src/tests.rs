use std::error::Error as StdError;
use std::fs::{self, DirBuilder};
use std::io::{Read, Write};
use std::net::{SocketAddr, TcpListener};
use std::os::unix::fs::{DirBuilderExt, MetadataExt, PermissionsExt};
use std::os::unix::net::UnixStream;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::Arc;
use std::thread;
use std::time::Duration;

use layerx_client::lni::transport::{Limits, MutualTlsConfig};
use layerx_human_service::custody::RemoteKmsProvider;
use layerx_human_service::journeys::DepositRuntime;
use layerx_human_service::server::movement_provider::{
    MovementProviderCodec, MovementProviderConfig, MovementProviderRequest as Request,
    MovementProviderResponse as Response, NativeMovementCodec, PlanningContext, PlanningRequest,
    UnixMovementProvider,
};
use layerx_human_service::store::{AgentTenantId, PrincipalId};
use layerx_human_service::trace::TraceId;
use layerx_paxeer_client::{
    raw_call, ChainSignal, DepositProofConfig, EndpointConfig, EndpointTransport, FinalityTracker,
    ProofFault, TrackerConfig, TransactionHash, WithdrawalBoundary, WithdrawalConfig,
    WithdrawalMaterial,
};
use layerx_types::intent::EvmAddress;
use rustls::{
    pki_types::{CertificateDer, PrivateKeyDer},
    RootCertStore,
};
use sha2::{Digest, Sha256};

use crate::config::{hex_string, Config, MAX_FRAME};
use crate::journal::{read_private, Journal};
use crate::listener::{Listener, ListenerConfig};
use crate::service::EvidenceService;
use crate::Error;

type Result<T = ()> = std::result::Result<T, Box<dyn StdError>>;
fn checked<T, E: std::fmt::Debug>(value: std::result::Result<T, E>) -> Result<T> {
    value.map_err(|error| format!("{error:?}").into())
}

pub(crate) struct Directory(PathBuf);
impl Directory {
    pub(crate) fn new() -> Result<Self> {
        let mut nonce = [0; 8];
        checked(getrandom::fill(&mut nonce))?;
        let root = std::env::temp_dir().join("hm5");
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
fn unreachable_endpoint() -> Result<EndpointConfig> {
    let closed = TcpListener::bind("127.0.0.1:0")?;
    let endpoint = EndpointConfig {
        url: format!("http://{}", closed.local_addr()?),
        request_timeout: Duration::from_millis(50),
        transport: EndpointTransport::LocalEmulator,
        expected_chain_id: 31337,
    };
    drop(closed);
    Ok(endpoint)
}
pub(crate) fn config(dir: &Directory) -> Result<Config> {
    config_for(dir, unreachable_endpoint()?, None)
}
fn config_for(
    dir: &Directory,
    endpoint: EndpointConfig,
    executor: Option<Arc<RemoteKmsProvider>>,
) -> Result<Config> {
    let evidence_root = dir.0.join("evidence");
    DirBuilder::new().mode(0o700).create(&evidence_root)?;
    Ok(Config {
        listener: listener_config(dir),
        state_root: dir.0.join("state"),
        evidence_root,
        custody_profile: None,
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
        checkpoint_registry: EvmAddress::new([12; 20]),
        executor,
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
        Arc::new(NativeMovementCodec::new()),
        // The boundary's protocol version is the withdrawal receipt's, not the
        // provider socket's: the custody precompile only honours
        // state-commitment receipts, so the boundary adopts that version alone.
        checked(WithdrawalBoundary::new(WithdrawalConfig {
            endpoints: config.tracker.endpoints.clone(),
            minimum_endpoint_agreement: config.tracker.minimum_endpoint_agreement,
            required_confirmations: 2,
            poll_cadence: Duration::from_secs(1),
            delayed_after_polls: 2,
        }))?,
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
        request_anchor: [18; 32],
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
fn signed_withdrawal_material_replays_without_converting_it_to_authority() -> Result {
    for protocol in [2, 3] {
        let dir = Directory::new()?;
        let codec = NativeMovementCodec::new();
        let material = withdrawal_material()?;
        let response =
            checked(codec.encode_response(&Response::WithdrawalMaterial(Some(material.clone()))))?;
        let mut journal = Journal::open(&dir.0.join("state"), protocol)?;
        let key = "b".repeat(64);
        journal.begin(
            &key,
            &checked(codec.encode_request(&Request::WithdrawalMaterial(
                layerx_paxeer_client::DebitExpectation {
                    activity_id: [5; 32],
                    network_id: 77,
                    withdrawal_id: [5; 32],
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
        let mut invalid = material;
        invalid.header_signature[0] ^= 1;
        let invalid_response = Response::WithdrawalMaterial(Some(invalid));
        let invalid_bytes = checked(codec.encode_response(&invalid_response))?;
        let decoded = checked(codec.decode_response(&invalid_bytes))?;
        assert_eq!(checked(codec.encode_response(&decoded))?, invalid_bytes);
    }
    Ok(())
}

/// The real bound native withdrawal a `LayerX` node serves: its canonical
/// receipt, the wire Merkle path under the header's receipt root, the
/// canonical batch header and the sequencer signature over that header. Every
/// byte comes from the recorded fixture; none of it is synthesised here.
pub(crate) fn withdrawal_material() -> Result<WithdrawalMaterial> {
    const RECEIPT: &[u8] =
        include_bytes!("../../../../tests/fixtures/asset/bound-native-withdrawal/receipt");
    const PROOF: &[u8] =
        include_bytes!("../../../../tests/fixtures/asset/bound-native-withdrawal/receipt.proof");
    const HEADER: &[u8] =
        include_bytes!("../../../../tests/fixtures/asset/bound-native-withdrawal/header");
    const SIGNATURE: &[u8] =
        include_bytes!("../../../../tests/fixtures/asset/bound-native-withdrawal/header.signature");
    checked(
        WithdrawalMaterial {
            receipt: RECEIPT.to_vec(),
            proof: PROOF.to_vec(),
            header: HEADER.to_vec(),
            header_signature: <[u8; 64]>::try_from(SIGNATURE)?,
        }
        .validated(),
    )
}

/// The registry-publication check the Solidity checkpoint registry used to
/// answer is now the anchor precompile's finalized roots: stored withdrawal
/// material is never served while no origin anchors the batch its signed
/// header names, and material for a debit this provider never bound is refused
/// outright.
#[test]
fn stored_withdrawal_material_is_served_only_once_the_anchor_finalises_its_batch() -> Result {
    use layerx_human_service::server::movement_provider::MovementProviderService;
    let dir = Directory::new()?;
    let config = config(&dir)?;
    let codec = NativeMovementCodec::new();
    let planning = plan("withdraw.start")?;
    let identity = layerx_human_service::journeys::MovementExecutionIdentity {
        principal: planning.principal.clone(),
        tenant: planning.tenant.clone(),
        account: checked(layerx_paxeer_client::account_address_for_protocol(
            &planning.context.account,
            2,
        ))?,
        wallet: planning.context.wallet,
        plan_id: planning.idempotency_key,
    };
    let debit = layerx_paxeer_client::DebitExpectation {
        activity_id: [42; 32],
        withdrawal_id: [42; 32],
        network_id: planning.context.network.value(),
        account: identity.account,
        withdrawals_account: checked(layerx_paxeer_client::account_address_for_protocol(
            &planning.context.withdrawals_account,
            2,
        ))?,
        asset_id: planning.context.asset.bytes(),
        amount: planning.context.amount.value(),
        recipient: identity.wallet,
    };
    let mut journal = Journal::open(&config.state_root, 2)?;
    let key = hex_string(&[43; 32]);
    journal.begin(
        &key,
        &checked(codec.encode_request(&Request::BindWithdrawalDebit {
            identity,
            debit,
            receipt_reference: [44; 32],
        }))?,
    )?;
    journal.complete(&key, &checked(codec.encode_response(&Response::Ready))?)?;
    drop(journal);
    let path = config.evidence_root.join(format!(
        "withdrawal-{}.bin",
        hex_string(&debit.withdrawal_id)
    ));
    let published = checked(
        codec.encode_response(&Response::WithdrawalMaterial(Some(withdrawal_material()?))),
    )?;
    assert!(crate::journal::publish_private(&path, &published)?);
    let mut service = EvidenceService::new(&config, Journal::open(&config.state_root, 2)?)?;
    assert_eq!(
        service.dispatch(Request::WithdrawalMaterial(debit)),
        Response::Unavailable
    );
    let mut unbound = debit;
    unbound.activity_id = [45; 32];
    unbound.withdrawal_id = unbound.activity_id;
    assert_eq!(
        service.dispatch(Request::WithdrawalMaterial(unbound)),
        Response::ContractViolation
    );
    Ok(())
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
fn published_evidence_is_private_complete_and_never_overwrites_an_existing_path() -> Result {
    let dir = Directory::new()?;
    let codec = NativeMovementCodec::new();
    let original = checked(codec.encode_response(&Response::Unavailable))?;
    let changed = checked(codec.encode_response(&Response::ContractViolation))?;
    let path = dir.0.join("evidence.bin");
    assert!(crate::journal::publish_private(&path, &original)?);
    assert_eq!(read_private(&path, MAX_FRAME)?, original);
    let metadata = fs::symlink_metadata(&path)?;
    assert_eq!(metadata.mode() & 0o777, 0o600);
    assert_eq!(metadata.nlink(), 1);
    assert!(!crate::journal::publish_private(&path, &changed)?);
    assert_eq!(read_private(&path, MAX_FRAME)?, original);
    assert!(crate::journal::publish_private(&dir.0.join("empty.bin"), &[]).is_err());
    assert!(
        crate::journal::publish_private(&dir.0.join("large.bin"), &vec![0; MAX_FRAME + 1]).is_err()
    );
    let linked = dir.0.join("linked.bin");
    std::os::unix::fs::symlink(&path, &linked)?;
    assert!(!crate::journal::publish_private(&linked, &changed)?);
    assert!(read_private(&linked, MAX_FRAME).is_err());
    assert_eq!(read_private(&path, MAX_FRAME)?, original);
    assert!(fs::read_dir(&dir.0)?.all(|entry| entry.is_ok_and(|entry| {
        !entry
            .file_name()
            .to_string_lossy()
            .starts_with("proof-pending-")
    })));
    Ok(())
}

#[test]
fn deposit_export_arguments_require_exact_nonzero_public_identity() -> Result {
    use crate::evidence_export::Request;
    assert!(Request::arguments(std::iter::empty()).is_ok_and(|value| value.is_none()));
    let valid = vec![
        "--publish-deposit-proof".to_owned(),
        format!("0x{}", "11".repeat(32)),
        format!("0x{}", "22".repeat(32)),
        "agent:did:layerx:deposit-recipient:main".to_owned(),
    ];
    assert!(Request::arguments(valid.clone().into_iter())?.is_some());
    for index in 0..valid.len() {
        let mut invalid = valid.clone();
        invalid[index] = "invalid".to_owned();
        assert!(Request::arguments(invalid.into_iter()).is_err());
    }
    for index in [1, 2] {
        let mut invalid = valid.clone();
        invalid[index] = format!("0x{}", "00".repeat(32));
        assert!(Request::arguments(invalid.into_iter()).is_err());
    }
    for length in 1..valid.len() {
        assert!(Request::arguments(valid[..length].iter().cloned()).is_err());
    }
    let mut extra = valid;
    extra.push("extra".to_owned());
    assert!(Request::arguments(extra.into_iter()).is_err());
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

#[test]
fn withdrawal_mapping_preserves_anchor_and_refuses_old_plan_shape() -> Result {
    let request = plan("withdraw.start")?;
    let plan = checked(crate::planning::withdrawal_plan(
        &request,
        layerx_human_service::journeys::SettlementConfig {
            checkpoint_interval_seconds: 10,
            paxeer_block_seconds: 2,
            required_confirmations: 2,
        },
        60,
    ))?;
    assert_eq!(plan.request_anchor.bytes(), request.context.request_anchor);
    assert_eq!(plan.payout_address, request.context.wallet);
    assert_eq!(plan.agent.fee_limit, request.context.fee_limit);
    let codec = NativeMovementCodec::new();
    let encoded = checked(codec.encode_response(&Response::WithdrawalPlan(plan.clone())))?;
    assert_eq!(
        checked(codec.decode_response(&encoded))?,
        Response::WithdrawalPlan(plan)
    );
    let mut old = encoded;
    let tag = old
        .windows(2)
        .position(|bytes| bytes == [1, 6])
        .ok_or("plan tag absent")?;
    old[tag + 1] = 5;
    assert!(codec.decode_response(&old).is_err());
    Ok(())
}

#[test]
fn receipt_identity_mapping_survives_restart_and_refuses_reassignment() -> Result {
    let dir = Directory::new()?;
    let codec = NativeMovementCodec::new();
    let planning = plan("withdraw.start")?;
    let identity = layerx_human_service::journeys::MovementExecutionIdentity {
        principal: planning.principal.clone(),
        tenant: planning.tenant.clone(),
        account: checked(layerx_paxeer_client::account_address_for_protocol(
            &planning.context.account,
            2,
        ))?,
        wallet: planning.context.wallet,
        plan_id: planning.idempotency_key,
    };
    let debit = layerx_paxeer_client::DebitExpectation {
        activity_id: [31; 32],
        withdrawal_id: [31; 32],
        network_id: planning.context.network.value(),
        account: identity.account,
        withdrawals_account: checked(layerx_paxeer_client::account_address_for_protocol(
            &planning.context.withdrawals_account,
            2,
        ))?,
        asset_id: planning.context.asset.bytes(),
        amount: planning.context.amount.value(),
        recipient: identity.wallet,
    };
    let request = Request::BindWithdrawalDebit {
        identity: identity.clone(),
        debit,
        receipt_reference: [32; 32],
    };
    let bytes = checked(codec.encode_request(&request))?;
    assert_eq!(checked(codec.decode_request(&bytes))?, request);
    let key = hex_string(&[30; 32]);
    let root = dir.0.join("journal");
    let mut journal = Journal::open(&root, 2)?;
    journal.begin(&key, &bytes)?;
    assert!(!journal.has_withdrawal_debit(&debit));
    journal.complete(&key, &checked(codec.encode_response(&Response::Ready))?)?;
    drop(journal);
    let mut journal = Journal::open(&root, 2)?;
    assert!(journal.has_withdrawal_debit(&debit));
    journal.begin(&key, &bytes)?;
    let mut other = debit;
    other.activity_id = [33; 32];
    other.withdrawal_id = other.activity_id;
    let changed = checked(codec.encode_request(&Request::BindWithdrawalDebit {
        identity: identity.clone(),
        debit: other,
        receipt_reference: [34; 32],
    }))?;
    assert!(journal.begin(&key, &changed).is_err());
    assert!(!journal.has_withdrawal_debit(&other));
    other.withdrawal_id = [35; 32];
    assert!(codec
        .encode_request(&Request::BindWithdrawalDebit {
            identity,
            debit: other,
            receipt_reference: [34; 32]
        })
        .is_err());
    Ok(())
}

fn probe_settings(config: &Config) -> crate::probe::Settings {
    crate::probe::Settings {
        socket: config.listener.socket.clone(),
        deadline: Duration::from_secs(2),
        maximum_frame_bytes: MAX_FRAME,
        protocol: config.listener.protocol,
    }
}

#[test]
fn probe_reaches_the_real_listener_and_reports_the_provider_answer() -> Result {
    let dir = Directory::new()?;
    let config = config(&dir)?;
    let settings = probe_settings(&config);
    assert!(crate::probe::probe(&settings).is_err());
    let mut service = EvidenceService::new(&config, Journal::open(&config.state_root, 2)?)?;
    let listener = Listener::bind(listener_config(&dir))?;
    let server = thread::spawn(move || listener.serve_next(&mut service));
    assert_eq!(
        checked(crate::probe::probe(&settings))?,
        crate::probe::Outcome::NotReady
    );
    server.join().map_err(|_| "server panicked")??;
    assert!(crate::probe::probe(&settings).is_err());
    Ok(())
}

#[test]
fn probe_accepts_only_the_provider_ready_answer() -> Result {
    let codec = NativeMovementCodec::new();
    let ready = checked(codec.encode_response(&Response::Ready))?;
    assert_eq!(
        checked(crate::probe::interpret(2, &ready))?,
        crate::probe::Outcome::Ready
    );
    let unavailable = checked(codec.encode_response(&Response::Unavailable))?;
    assert_eq!(
        checked(crate::probe::interpret(2, &unavailable))?,
        crate::probe::Outcome::NotReady
    );
    assert!(crate::probe::interpret(2, b"not a movement frame").is_err());
    Ok(())
}

fn anvil_binary() -> PathBuf {
    let foundry = PathBuf::from("/root/.foundry/bin/anvil");
    if foundry.exists() {
        foundry
    } else {
        PathBuf::from("anvil")
    }
}

/// A real local Paxeer-compatible JSON-RPC origin, started for the readiness
/// tests and stopped again so readiness can be observed on both sides of the
/// origin going away.
struct Anvil {
    child: Child,
    endpoint: EndpointConfig,
}

impl Anvil {
    fn launch() -> Result<Self> {
        for _ in 0..8 {
            let reserved = TcpListener::bind("127.0.0.1:0")?;
            let port = reserved.local_addr()?.port();
            drop(reserved);
            let child = Command::new(anvil_binary())
                .arg("--port")
                .arg(port.to_string())
                .arg("--silent")
                .stdin(Stdio::null())
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .spawn()?;
            let mut anvil = Self {
                child,
                endpoint: EndpointConfig {
                    url: format!("http://127.0.0.1:{port}"),
                    request_timeout: Duration::from_secs(2),
                    transport: EndpointTransport::LocalEmulator,
                    expected_chain_id: 31337,
                },
            };
            if anvil.answers() {
                return Ok(anvil);
            }
            anvil.halt();
        }
        Err("no local paxeer emulator became reachable".into())
    }

    fn answers(&self) -> bool {
        for _ in 0..100 {
            if raw_call(&self.endpoint, "eth_chainId", &[]).is_ok() {
                return true;
            }
            thread::sleep(Duration::from_millis(50));
        }
        false
    }

    fn halt(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
        for _ in 0..100 {
            if raw_call(&self.endpoint, "eth_chainId", &[]).is_err() {
                return;
            }
            thread::sleep(Duration::from_millis(50));
        }
    }
}

impl Drop for Anvil {
    fn drop(&mut self) {
        self.halt();
    }
}

fn openssl(root: &Path, arguments: &[&str]) -> Result<()> {
    let result = Command::new("openssl")
        .args(arguments)
        .current_dir(root)
        .output()?;
    if !result.status.success() {
        return Err(format!(
            "openssl failed: {}",
            String::from_utf8_lossy(&result.stderr)
        )
        .into());
    }
    Ok(())
}

/// Builds the same `RemoteKmsProvider` the movement mode builds from its
/// environment, from freshly minted mutual-TLS material and a real local
/// endpoint address.
fn execution_authority(dir: &Directory) -> Result<Arc<RemoteKmsProvider>> {
    let _ = rustls::crypto::ring::default_provider().install_default();
    let root = dir.0.join("kms");
    DirBuilder::new().mode(0o700).create(&root)?;
    openssl(
        &root,
        &[
            "req",
            "-x509",
            "-newkey",
            "rsa:2048",
            "-nodes",
            "-keyout",
            "ca.key",
            "-out",
            "ca.pem",
            "-days",
            "1",
            "-subj",
            "/CN=movement provider test CA",
            "-addext",
            "basicConstraints=critical,CA:TRUE",
        ],
    )?;
    openssl(
        &root,
        &[
            "req",
            "-newkey",
            "rsa:2048",
            "-nodes",
            "-keyout",
            "client.key",
            "-out",
            "client.csr",
            "-subj",
            "/CN=movement-provider",
        ],
    )?;
    fs::write(root.join("extensions"), "extendedKeyUsage=clientAuth\n")?;
    openssl(
        &root,
        &[
            "x509",
            "-req",
            "-in",
            "client.csr",
            "-CA",
            "ca.pem",
            "-CAkey",
            "ca.key",
            "-CAcreateserial",
            "-out",
            "client.pem",
            "-days",
            "1",
            "-extfile",
            "extensions",
        ],
    )?;
    openssl(
        &root,
        &[
            "x509",
            "-in",
            "client.pem",
            "-outform",
            "DER",
            "-out",
            "client.der",
        ],
    )?;
    openssl(
        &root,
        &[
            "pkcs8",
            "-topk8",
            "-nocrypt",
            "-in",
            "client.key",
            "-outform",
            "DER",
            "-out",
            "client-key.der",
        ],
    )?;
    openssl(
        &root,
        &["x509", "-in", "ca.pem", "-outform", "DER", "-out", "ca.der"],
    )?;
    let mut roots = RootCertStore::empty();
    checked(roots.add(CertificateDer::from(fs::read(root.join("ca.der"))?)))?;
    let certificate = CertificateDer::from(fs::read(root.join("client.der"))?);
    let key = checked(PrivateKeyDer::try_from(fs::read(
        root.join("client-key.der"),
    )?))?;
    let tls = checked(MutualTlsConfig::new(roots, vec![certificate], key))?;
    let reserved = TcpListener::bind("127.0.0.1:0")?;
    let address: SocketAddr = reserved.local_addr()?;
    drop(reserved);
    Ok(Arc::new(checked(RemoteKmsProvider::new(
        "movement-provider-test-kms",
        address,
        "localhost",
        tls,
        Limits {
            maximum_frame_bytes: 2_097_152,
            maximum_connections: 1,
            maximum_streams: 1,
            maximum_queued_bytes: 2_097_152,
            deadline: Duration::from_secs(2),
        },
    ))?))
}

fn readiness_listener(dir: &Directory) -> ListenerConfig {
    let mut policy = listener_config(dir);
    policy.deadline = Duration::from_secs(10);
    policy
}

#[test]
fn readiness_is_ready_while_the_real_paxeer_origin_and_execution_authority_answer() -> Result {
    let dir = Directory::new()?;
    let paxeer = Anvil::launch()?;
    let config = config_for(
        &dir,
        paxeer.endpoint.clone(),
        Some(execution_authority(&dir)?),
    )?;
    let mut service = EvidenceService::new(&config, Journal::open(&config.state_root, 2)?)?;
    let listener = Listener::bind(readiness_listener(&dir))?;
    let client = client(&config)?;
    let server = thread::spawn(move || listener.serve_next(&mut service));
    assert!(client.ready());
    server.join().map_err(|_| "server panicked")??;
    let journal = Journal::open(&config.state_root, 2)?;
    let key = hex_string(&Sha256::digest(checked(
        NativeMovementCodec::new().encode_request(&Request::Readiness),
    )?));
    let recorded = journal
        .record(&key)
        .ok_or("missing readiness record")?
        .response
        .as_ref()
        .ok_or("missing readiness response")?;
    assert_eq!(
        checked(NativeMovementCodec::new().decode_response(recorded))?,
        Response::Ready
    );
    Ok(())
}

#[test]
fn readiness_stops_being_ready_once_the_paxeer_origin_is_down() -> Result {
    let dir = Directory::new()?;
    let mut paxeer = Anvil::launch()?;
    let config = config_for(
        &dir,
        paxeer.endpoint.clone(),
        Some(execution_authority(&dir)?),
    )?;
    let mut service = EvidenceService::new(&config, Journal::open(&config.state_root, 2)?)?;
    let listener = Listener::bind(readiness_listener(&dir))?;
    let client = client(&config)?;
    let server = thread::spawn(move || {
        listener.serve_next(&mut service)?;
        listener.serve_next(&mut service)
    });
    assert!(client.ready());
    paxeer.halt();
    assert!(!client.ready());
    server.join().map_err(|_| "server panicked")??;
    Ok(())
}
