use super::*;
use layerx_intents::{compile, Intent, IntentKind, LxpSend};
use layerx_platform_core::{
    asset_registry, domain_hash, main_account, send_context_hash, SignedSend,
};
use layerx_types::account::AccountId;
use layerx_types::ids::AssetId;
use layerx_types::intent::{
    AuthorizationSignature, ContextHash, NetworkId, ProtocolVersion, PublicKey, SendAuthorization,
    SendAuthorizationKind, Sequence, TimestampSeconds,
};
use layerx_wire::encode::Encoder;
use layerx_wire::hash::Domain;

pub(super) struct Funding {
    nodes: Vec<Daemon>,
    root: PathBuf,
    pub(super) recipient_did: String,
}

impl Drop for Funding {
    fn drop(&mut self) {
        for node in &mut self.nodes {
            node.stop();
        }
        if !thread::panicking() {
            let _ = fs::remove_dir_all(&self.root);
        }
    }
}

impl Funding {
    fn anvil(&mut self, fork: Option<(&str, u64)>) -> String {
        let port = loop {
            let port = free_port();
            if port != 18545 {
                break port;
            }
        };
        let stderr = self.root.join(format!("anvil-{port}.log"));
        let mut command = Command::new("anvil");
        command.args([
            "--host",
            "127.0.0.1",
            "--port",
            &port.to_string(),
            "--chain-id",
            "31337",
            "--silent",
        ]);
        if let Some((url, block)) = fork {
            command.args(["--fork-url", url, "--fork-block-number", &block.to_string()]);
        }
        let child = must(
            command
                .stdin(Stdio::null())
                .stdout(Stdio::null())
                .stderr(Stdio::from(must(fs::File::create(&stderr), "Anvil log")))
                .spawn(),
            "Anvil",
        );
        let mut daemon = Daemon {
            child,
            supervised: false,
            stderr,
        };
        wait_for_port(port, &mut daemon, "disposable custody chain");
        self.nodes.push(daemon);
        format!("http://127.0.0.1:{port}")
    }
}

fn producer(script: &str, args: &[&str]) {
    let output = must(
        Command::new("python3")
            .arg(repository_root().join("tests/bridge").join(script))
            .args(args)
            .output(),
        "custody producer",
    );
    assert!(
        output.status.success(),
        "custody producer: {}",
        String::from_utf8_lossy(&output.stderr)
    );
}

pub(super) fn start() -> (Cluster, Funding) {
    let root =
        std::env::temp_dir().join(format!("pay4-funding-{}-{}", std::process::id(), now_ms()));
    make_dir(&root, 0o700);
    let recipient_seed = random32();
    let recipient_did = treasury_did(&recipient_seed);
    let mut funding = Funding {
        nodes: Vec::new(),
        root,
        recipient_did: recipient_did.clone(),
    };
    let seed = random32();
    let did = treasury_did(&seed);
    let account = must(main_account(&did), "funding account");
    let primary = funding.anvil(None);
    let asset = format!("0x{}", hex_encode(&random32()));
    let beneficiary = format!("0x{}", hex_encode(&account));
    let deployment = funding.root.join("deployment.json");
    let actor_key = funding.root.join("actor.key");
    let attestor_key = funding.root.join("attestor.key");
    write(&actor_key, &seed, 0o600);
    write(&attestor_key, &random32(), 0o600);
    producer(
        "deploy_local_custody.py",
        &[
            "--rpc",
            &primary,
            "--asset",
            &asset,
            "--beneficiary",
            &beneficiary,
            "--amount",
            "100000000000000",
            "--output",
            &text(&deployment),
            "--allow-local-chain",
        ],
    );
    let deployment: serde_json::Value = must(
        serde_json::from_slice(&must(fs::read(&deployment), "deployment")),
        "deployment JSON",
    );
    assert_eq!(deployment["chain_id"], 31337);
    let recipient_deployment = deposit_recipient(&funding, &primary, &recipient_did);
    let secondary = funding.anvil(Some((
        &primary,
        recipient_deployment["fork_block"]
            .as_u64()
            .required("fork block"),
    )));
    let profile = funding.root.join("profile.bin");
    custody_profile(
        [&primary, &secondary],
        [&profile, &attestor_key],
        &deployment,
        &asset,
    );
    let credit = funding.root.join("credit.bin");
    attest_credit(
        [&primary, &secondary],
        [&profile, &attestor_key, &credit],
        &seed,
        &did,
        deployment["transaction"].as_str().required("transaction"),
        "100000000000000",
    );
    let cluster = credit_node(
        &funding,
        &profile,
        &credit,
        &actor_key,
        seed,
        &did,
        &recipient_seed,
    );
    let recipient_credit = funding.root.join("recipient-credit.bin");
    attest_credit(
        [&primary, &secondary],
        [&profile, &attestor_key, &recipient_credit],
        &recipient_seed,
        &recipient_did,
        recipient_deployment["transaction"]
            .as_str()
            .required("recipient transaction"),
        "1",
    );
    credit_recipient(
        &funding,
        &cluster,
        &profile,
        &recipient_credit,
        &recipient_seed,
        &recipient_did,
    );
    (cluster, funding)
}

fn custody_profile(
    rpcs: [&str; 2],
    paths: [&Path; 2],
    deployment: &serde_json::Value,
    asset: &str,
) {
    let [primary, secondary] = rpcs;
    let [profile, attestor_key] = paths;
    producer(
        "custody_credit.py",
        &[
            "profile",
            "--rpc",
            primary,
            "--rpc",
            secondary,
            "--network-id",
            &NETWORK_ID.to_string(),
            "--chain-id",
            "31337",
            "--vault",
            deployment["vault"].as_str().required("vault"),
            "--runtime-sha256",
            deployment["runtime_sha256"].as_str().required("runtime"),
            "--asset",
            asset,
            "--confirmations",
            "64",
            "--attestor-key",
            &text(attestor_key),
            "--output",
            &text(profile),
        ],
    );
}

fn deposit_recipient(funding: &Funding, primary: &str, recipient_did: &str) -> serde_json::Value {
    let recipient_deployment = funding.root.join("recipient-deployment.json");
    let output = Command::new("python3")
        .arg(repository_root().join("platform/hosted/gateway/tests/local/deposit_recipient.py"))
        .args([
            primary,
            &text(&funding.root.join("deployment.json")),
            &format!(
                "0x{}",
                hex_encode(&main_account(recipient_did).required("recipient account"))
            ),
            &text(&recipient_deployment),
        ])
        .output()
        .required("recipient custody deposit");
    assert!(
        output.status.success(),
        "recipient deposit: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let recipient_deployment: serde_json::Value =
        serde_json::from_slice(&fs::read(&recipient_deployment).required("recipient deployment"))
            .required("recipient JSON");
    recipient_deployment
}

fn attest_credit(
    rpcs: [&str; 2],
    paths: [&Path; 3],
    seed: &[u8; 32],
    did: &str,
    transaction: &str,
    amount: &str,
) {
    let [primary, secondary] = rpcs;
    let [profile, attestor_key, output] = paths;
    producer(
        "custody_credit.py",
        &[
            "attest",
            "--rpc",
            primary,
            "--rpc",
            secondary,
            "--profile",
            &text(profile),
            "--network-id",
            &NETWORK_ID.to_string(),
            "--transaction",
            transaction,
            "--beneficiary",
            &format!(
                "0x{}",
                hex_encode(&main_account(did).required("beneficiary"))
            ),
            "--beneficiary-key",
            &format!(
                "0x{}",
                hex_encode(&SigningKey::from_bytes(seed).verifying_key().to_bytes())
            ),
            "--expected-amount",
            amount,
            "--attestor-key",
            &text(attestor_key),
            "--output",
            &text(output),
        ],
    );
}

fn credit_recipient(
    funding: &Funding,
    cluster: &Cluster,
    profile: &Path,
    recipient_credit: &Path,
    recipient_seed: &[u8; 32],
    recipient_did: &str,
) {
    let recipient_key = funding.root.join("recipient.key");
    write(&recipient_key, recipient_seed, 0o600);
    let signed = funding.root.join("signed-recipient-credit.bin");
    command(
        &text(&repository_root().join("build/tests/bridge/sign-credit")),
        &[
            &text(profile),
            &text(recipient_credit),
            recipient_did,
            &text(&recipient_key),
            &account_sequence(&cluster.lni_socket, recipient_did).to_string(),
            &now_ms().saturating_sub(1000).to_string(),
            &text(&signed),
        ],
    );
    submit_credit(
        cluster,
        &fs::read(&signed).required("recipient credit"),
        recipient_seed,
    );
}

fn credit_node(
    funding: &Funding,
    profile: &Path,
    credit: &Path,
    actor_key: &Path,
    seed: [u8; 32],
    did: &str,
    recipient_seed: &[u8; 32],
) -> Cluster {
    let cluster = start_node(profile, seed, recipient_seed);
    let signed = funding.root.join("signed-credit.bin");
    command(
        &text(&repository_root().join("build/tests/bridge/sign-credit")),
        &[
            &text(profile),
            &text(credit),
            did,
            &text(actor_key),
            &account_sequence(&cluster.lni_socket, did).to_string(),
            &now_ms().saturating_sub(1000).to_string(),
            &text(&signed),
        ],
    );
    submit_credit(
        &cluster,
        &must(fs::read(&signed), "signed custody credit"),
        &seed,
    );
    cluster
}

fn submit_credit(cluster: &Cluster, signed: &[u8], seed: &[u8; 32]) {
    use layerx_client::submit::{submit_signed, Submission, SubmissionContext};
    let gate = ConnectionGate::new(1);
    let mut transport = must(
        Uds::connect(&cluster.lni_socket, &gate, lni_limits()),
        "credit LNI",
    );
    let handshake = must(
        perform(&mut transport, &handshake_config(), None),
        "credit handshake",
    );
    let kind = must(ActivityType::new(ModuleId::Bridge, 1), "bridge kind");
    let registry = must(
        ModuleRegistry::new(&[must(
            ModuleRegistration::new(ModuleId::Bridge, &[kind]),
            "bridge",
        )]),
        "registry",
    );
    let submitted = must(
        submit_signed(
            &mut transport,
            &registry,
            SubmissionContext {
                interface_version: handshake.node().interface_version,
                protocol_version: PROTOCOL_VERSION,
                network_id: NETWORK_ID,
                correlation_id: 1,
                signer_public_key: SigningKey::from_bytes(seed).verifying_key().to_bytes(),
                attempt: 1,
            },
            signed,
        ),
        "credit admission",
    );
    let Submission::Acknowledged(ack) = submitted else {
        panic!("credit admission unknown")
    };
    let mut selector = vec![1];
    selector.extend_from_slice(&ack.activity_id());
    selector.push(1);
    drop(transport);
    let deadline = Instant::now() + Duration::from_secs(30);
    loop {
        let (tag, bytes) = receipt_wait_request(&cluster.lni_socket, &selector);
        assert_eq!(tag, 6);
        if !bytes.is_empty() {
            let receipt = must(
                layerx_proof::receipt::verify_sequencer_signature(&bytes, cluster.sequencer_key),
                "credit signature",
            );
            let receipt = receipt.protocol().required("credit protocol receipt");
            assert_eq!(receipt.activity_id(), ack.activity_id());
            assert_eq!(receipt.result_code(), 0);
            return;
        }
        assert!(Instant::now() < deadline, "credit receipt deadline");
    }
}

fn funded_genesis(
    root: &Path,
    builder: &Path,
    sequencer_seed: &[u8; 32],
    profile: &Path,
) -> Genesis {
    let directory = root.join("genesis");
    make_dir(&directory, 0o755);
    let profile_bytes = must(fs::read(profile), "custody profile");
    assert_eq!(profile_bytes.len(), 207);
    let asset = must(profile_bytes[97..129].try_into(), "custody asset");
    let sequencer_key = SigningKey::from_bytes(sequencer_seed)
        .verifying_key()
        .to_bytes();
    let mut request = genesis_request(&asset, &sequencer_key);
    let schedule = request.len() - 215;
    request[schedule + 34..schedule + 50].copy_from_slice(&1_u128.to_be_bytes());
    request[schedule + 119..schedule + 135].copy_from_slice(&4_u128.to_be_bytes());
    write(&directory.join("request.lxgb"), &request, 0o600);
    write(&directory.join("signer.key"), sequencer_seed, 0o600);
    let artifacts = directory.join("artifacts");
    command(
        &text(builder),
        &[
            &text(&directory.join("request.lxgb")),
            &text(&directory.join("signer.key")),
            &text(&artifacts),
            "--custody-profile",
            &text(profile),
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

fn start_node(profile: &Path, treasury_seed: [u8; 32], recipient_seed: &[u8; 32]) -> Cluster {
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
    let treasury_did = treasury_did(&treasury_seed);
    let treasury_key = SigningKey::from_bytes(&treasury_seed)
        .verifying_key()
        .to_bytes();
    let genesis = funded_genesis(&root, &builder, &sequencer_seed, profile);
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
    let identities = node_dir.join("identities.txt");
    let mut configured = fs::read(&identities).required("bootstrap identities");
    configured.extend_from_slice(
        format!(
            "{}:{}:0\n",
            hex_encode(layerx_platform_core::treasury_did(recipient_seed).as_bytes()),
            hex_encode(
                &SigningKey::from_bytes(recipient_seed)
                    .verifying_key()
                    .to_bytes()
            )
        )
        .as_bytes(),
    );
    write(&identities, &configured, 0o600);
    chown_tree(&node_dir, DAEMON_UID, DAEMON_GID);
    let lni_socket = run_dir.join("layerxd.lni.sock");
    let node_env = node_environment(
        [&node_dir, &checkpoints, &logs, &migrations, &lni_socket],
        &genesis,
        [&sequencer_id, &sequencer_key, &sequencer_seed, &replica_id],
        [replica_port, program_port],
        [&replica_token, &program_token],
    );
    let sequencer = Some({
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

pub(super) fn send(
    seed: &[u8; 32],
    identity_sequence: u64,
    request: &SendRequest,
) -> Result<SignedSend, String> {
    if request.amount == 0 {
        return Err("amount must be greater than zero".into());
    }
    if request.expires_at_ms <= request.not_before_ms {
        return Err("expiry must follow the validity start".into());
    }
    let signing_key = SigningKey::from_bytes(seed);
    let public_key = signing_key.verifying_key().to_bytes();
    let source = main_account(&request.source_did)?;
    let target = AccountId::parse(&format!("agent:{}:main", request.destination_did))
        .map_err(|e| format!("account: {e:?}"))?;
    let destination = layerx_wire::hash::account_id_for_protocol(&target, PROTOCOL_VERSION)
        .map_err(|e| format!("account id: {e:?}"))?;
    let context = send_context_hash(
        &source,
        &destination,
        &request.asset,
        request.amount,
        &request.idempotency_key,
    );
    let authorization = send_authorization(&signing_key, &source, &destination, request, &context)?;
    let from = AccountId::parse(&format!("agent:{}:main", request.source_did))
        .map_err(|error| format!("source account is invalid: {error:?}"))?;
    let to = target;
    let intent = LxpSend::new(
        from,
        to,
        AssetId::new(request.asset),
        Amount::from_u128(request.amount),
        Sequence::from_u64(request.account_sequence),
        IdempotencyKey::new(request.idempotency_key),
        TimestampSeconds::from_u64(request.expires_at_ms),
        ContextHash::new(context),
        SendAuthorization::new(
            SendAuthorizationKind::Owner,
            PublicKey::new(public_key),
            AuthorizationSignature::new(authorization),
        ),
        NetworkId::new(request.network_id)
            .map_err(|error| format!("network id is invalid: {error:?}"))?,
        ProtocolVersion::new(layerx_wire::limits::STATE_COMMITMENT_PROTOCOL_VERSION)
            .map_err(|error| format!("protocol version is invalid: {error:?}"))?,
    )
    .map_err(|error| format!("send intent is invalid: {error:?}"))?;
    let (registry, activity_type) = asset_registry()?;
    let compiled = compile(&Intent::v1(IntentKind::LxpSend(intent)), &registry)
        .map_err(|error| format!("send intent does not compile: {error:?}"))?;
    if compiled.activity_type() != activity_type {
        return Err("compiled intent is not an asset send".into());
    }
    let actor = Did::new(request.source_did.as_bytes())
        .map_err(|error| format!("source DID is invalid: {error:?}"))?;
    let authority = Authority::owner(&public_key)
        .map_err(|error| format!("owner authority is invalid: {error:?}"))?;
    let timestamp = TimestampBound::new(request.not_before_ms, request.expires_at_ms)
        .map_err(|error| format!("timestamp bound is invalid: {error:?}"))?;
    let mut builder = EnvelopeBuilder::new();
    builder
        .protocol_version(layerx_wire::limits::STATE_COMMITMENT_PROTOCOL_VERSION)
        .and_then(|value| value.network_id(request.network_id))
        .and_then(|value| value.activity_type(activity_type))
        .and_then(|value| value.actor_did(actor))
        .and_then(|value| value.authority(authority))
        .and_then(|value| value.account_sequence(identity_sequence))
        .and_then(|value| value.timestamp_bound(timestamp))
        .and_then(|value| value.idempotency_key(IdempotencyKey::new(request.idempotency_key)))
        .and_then(|value| value.fee_limit(Amount::from_u128(request.fee_limit)))
        .and_then(|value| value.payload_hash(compiled.payload_hash()))
        .and_then(|value| value.payload(compiled.payload().clone()))
        .map_err(|error| format!("send envelope is invalid: {error:?}"))?;
    let unsigned = builder
        .build()
        .map_err(|error| format!("send envelope is incomplete: {error:?}"))?;
    let unsigned_bytes = layerx_wire::activity::encode_unsigned_envelope(&unsigned)
        .map_err(|error| format!("send signing bytes are invalid: {error:?}"))?;
    let digest = domain_hash(Domain::SignaturePreimage, &unsigned_bytes);
    let signature = disclosed_signature(seed, &unsigned_bytes, &registry)?;
    layerx_crypto::ed25519::verify_digest(&public_key, &signature, &digest)
        .map_err(|error| format!("send signature does not verify: {error:?}"))?;
    let signed = unsigned.attach_signature(
        Signature::new(&signature)
            .map_err(|error| format!("send signature is invalid: {error:?}"))?,
    );
    let canonical = layerx_wire::activity::encode_signed_envelope(&signed)
        .map_err(|error| format!("signed send is invalid: {error:?}"))?;
    let decoded = layerx_wire::activity::decode_signed(&canonical, &registry)
        .map_err(|error| format!("signed send does not decode: {error:?}"))?;
    let activity_id = layerx_wire::hash::activity_id(&decoded)
        .map_err(|error| format!("send activity id is invalid: {error:?}"))?;
    Ok(SignedSend {
        canonical,
        activity_id,
        source_account: source,
        destination_account: destination,
        signer_public_key: public_key,
        idempotency_key: request.idempotency_key,
    })
}

fn disclosed_signature(
    seed: &[u8; 32],
    canonical: &[u8],
    registry: &ModuleRegistry,
) -> Result<[u8; 64], String> {
    let disclosure = layerx_crypto::disclosure::bind(canonical, registry)
        .map_err(|error| format!("send disclosure is invalid: {error:?}"))?;
    let local_key = layerx_crypto::signer::LocalSigner::new(*seed);
    let mut future =
        layerx_crypto::signer::sign_disclosed(&local_key, canonical, &disclosure, registry);
    let mut context = std::task::Context::from_waker(std::task::Waker::noop());
    let signature = match std::future::Future::poll(future.as_mut(), &mut context) {
        std::task::Poll::Ready(result) => *result
            .map_err(|error| format!("send signer refused: {error:?}"))?
            .as_bytes(),
        std::task::Poll::Pending => return Err("local signer unexpectedly pending".into()),
    };
    Ok(signature)
}

fn send_authorization(
    signing_key: &SigningKey,
    source: &[u8; 32],
    destination: &[u8; 32],
    request: &SendRequest,
    context: &[u8; 32],
) -> Result<[u8; 64], String> {
    let mut authorization = Encoder::new(512);
    authorization
        .u16(0x5301)
        .and_then(|()| authorization.fixed(source))
        .and_then(|()| authorization.fixed(destination))
        .and_then(|()| authorization.fixed(&request.asset))
        .and_then(|()| authorization.u128(request.amount))
        .and_then(|()| authorization.u64(request.account_sequence))
        .and_then(|()| authorization.fixed(&request.idempotency_key))
        .and_then(|()| authorization.u64(request.expires_at_ms))
        .and_then(|()| authorization.fixed(context))
        .and_then(|()| authorization.u8(0))
        .and_then(|()| authorization.u8(SendAuthorizationKind::Owner as u8))
        .and_then(|()| authorization.fixed(source))
        .and_then(|()| authorization.fixed(context))
        .and_then(|()| authorization.u32(request.network_id))
        .and_then(|()| authorization.u16(layerx_wire::limits::STATE_COMMITMENT_PROTOCOL_VERSION))
        .map_err(|error| format!("send authorization is too large: {error:?}"))?;
    let digest = domain_hash(Domain::SignaturePreimage, &authorization.finish());
    Ok(signing_key.sign(&digest).to_bytes())
}
