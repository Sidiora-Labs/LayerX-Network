use super::*;
use layerx_client::lni::transport::{FrameTransport, TransportError};
use layerx_proof::inclusion::{verify_receipt, SequencerAuthorization};
use layerx_proof::merkle::Proof;
use layerx_wire::hash::{receipt_digest, receipt_execution_batch_id};
use layerx_wire::receipt::{decode_batch_header, decode_merkle_proof, encode_unsigned};

struct AccountProofConnection {
    child: Child,
    input: std::process::ChildStdin,
    output: std::process::ChildStdout,
    corrupt_account_root: Option<[u8; 32]>,
}

impl AccountProofConnection {
    fn connect(cluster: &Cluster) -> Self {
        let mut child = must(
            Command::new("/usr/bin/setpriv")
                .args([
                    "--reuid",
                    &BOUNDARY_UID.to_string(),
                    "--regid",
                    &BOUNDARY_GID.to_string(),
                    "--groups",
                    &BOUNDARY_GID.to_string(),
                    "--",
                    "/usr/bin/python3",
                    "-c",
                ])
                .arg(include_str!("lni_relay.py"))
                .arg(cluster.root.join("run/layerxd.sock"))
                .stdin(Stdio::piped())
                .stdout(Stdio::piped())
                .stderr(Stdio::inherit())
                .spawn(),
            "real LNI account-proof connection",
        );
        let input = child
            .stdin
            .take()
            .unwrap_or_else(|| panic!("LNI relay input missing"));
        let output = child
            .stdout
            .take()
            .unwrap_or_else(|| panic!("LNI relay output missing"));
        Self {
            child,
            input,
            output,
            corrupt_account_root: None,
        }
    }
}

impl Drop for AccountProofConnection {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

impl FrameTransport for AccountProofConnection {
    fn send(&mut self, bytes: &[u8]) -> Result<(), TransportError> {
        layerx_client::lni::framing::write_frame(&mut self.input, bytes, LNI_FRAME_BYTES)
    }

    fn receive(&mut self) -> Result<Vec<u8>, TransportError> {
        let bytes = layerx_client::lni::framing::read_frame(&mut self.output, LNI_FRAME_BYTES)?;
        let Some(account) = self.corrupt_account_root.take() else {
            return Ok(bytes);
        };
        let envelope = must(
            layerx_client::lni::schema::decode_envelope(&bytes),
            "real proof envelope",
        );
        assert_eq!(envelope.message_tag, 17);
        let mut proof = envelope.proof_material.to_vec();
        assert!(proof.len() >= 68);
        assert_eq!(&proof[..4], &[0, 1, 2, 1]);
        assert_eq!(&proof[4..36], &account);
        proof[36] ^= 1;
        Ok(must(
            layerx_client::lni::schema::encode_envelope(layerx_client::lni::schema::Envelope {
                proof_material: &proof,
                ..envelope
            }),
            "fault-injected real account proof",
        ))
    }
}

fn verify_funded_accounts(
    cluster: &Cluster,
    protocol: &layerx_wire::receipt::ProtocolReceipt,
    authorization: SequencerAuthorization,
) {
    use layerx_client::evidence::RootSelector;
    use layerx_client::lni::handshake::{perform, HandshakeConfig};
    use layerx_client::lni::schema::Version;
    use layerx_client::read::{account, ReadContext, Requested};
    use layerx_types::verify::VerificationLevel;

    let mut connection = AccountProofConnection::connect(cluster);
    let handshake = must(
        perform(
            &mut connection,
            &HandshakeConfig {
                built_interface_version: Version::V1_4,
                expected_protocol_version: PROTOCOL_VERSION,
                expected_network_id: NETWORK_ID,
            },
            None,
        ),
        "account proof handshake",
    );
    assert_eq!(
        handshake.node().authorised_sequencer_key,
        cluster.sequencer_key
    );
    let head = layerx_client::head::HeadTracker::new(handshake.node()).current();
    for (index, (identifier, expected_balance)) in [
        (reserve_account(), 0),
        (cluster.actor.source, FUNDING_AMOUNT),
    ]
    .into_iter()
    .enumerate()
    {
        let value = must(
            account(
                &mut connection,
                identifier,
                ReadContext {
                    interface_version: handshake.node().interface_version,
                    correlation_id: must(u64::try_from(index + 1), "account correlation"),
                    expected_protocol_version: PROTOCOL_VERSION,
                    expected_network_id: NETWORK_ID,
                    requested: Requested::new(VerificationLevel::STATE_PROVEN),
                    head,
                    sequencer_authorization: authorization,
                    handshake_sequencer_key: cluster.sequencer_key,
                    root_selector: RootSelector::Latest,
                },
            ),
            "funding account-state proof",
        );
        verify_funded_account(cluster, protocol, identifier, expected_balance, &value);
        verify_account_bundle(
            cluster,
            protocol,
            identifier,
            &value,
            &mut connection,
            layerx_client::evidence::EvidenceContext {
                interface_version: handshake.node().interface_version,
                correlation_id: must(u64::try_from(index + 10), "bundle correlation"),
                expected_protocol_version: PROTOCOL_VERSION,
                expected_network_id: NETWORK_ID,
                handshake_sequencer_key: cluster.sequencer_key,
            },
        );
    }
}

fn verify_funded_account(
    cluster: &Cluster,
    protocol: &layerx_wire::receipt::ProtocolReceipt,
    identifier: [u8; 32],
    expected_balance: u128,
    value: &layerx_client::read::ReadValue,
) {
    use layerx_proof::state::decode_account_value;
    use layerx_types::verify::VerificationLevel;
    assert_eq!(value.achieved(), VerificationLevel::STATE_PROVEN);
    let decoded = must(
        decode_account_value(identifier, value.canonical_bytes()),
        "funding canonical account",
    );
    assert_eq!(decoded.asset_id(), cluster.asset);
    assert_eq!(decoded.balance(), expected_balance);
    if identifier == reserve_account() {
        let balances = protocol.effects()[2].body();
        let after_sequence = u64::from_be_bytes(must(
            balances[104..112].try_into(),
            "reserve after sequence",
        ));
        assert_eq!(decoded.next_sequence, after_sequence);
    }
}

fn verify_account_bundle(
    cluster: &Cluster,
    protocol: &layerx_wire::receipt::ProtocolReceipt,
    identifier: [u8; 32],
    value: &layerx_client::read::ReadValue,
    connection: &mut AccountProofConnection,
    context: layerx_client::evidence::EvidenceContext,
) {
    use layerx_client::evidence::{
        proof_bundle, EvidenceError, ProofBundleSelector, VerifiedProofBundle,
    };
    let selector = ProofBundleSelector::AccountState {
        activity_id: protocol.activity_id(),
        account_id: identifier,
    };
    let bundle = must(
        proof_bundle(connection, selector, context, &bridge_registry()),
        "public nested account verifier",
    );
    let VerifiedProofBundle::Account {
        canonical_bytes,
        activity_id,
        verified,
        signed_header,
    } = bundle
    else {
        panic!("account proof bundle required");
    };
    assert_eq!(canonical_bytes, value.canonical_bytes());
    assert_eq!(activity_id, protocol.activity_id());
    assert_eq!(
        verified.header().header().resulting_state_root(),
        protocol.resulting_state_root()
    );
    assert_eq!(signed_header.public_key, cluster.sequencer_key);
    assert_eq!(verified.receipt_activity_id(), protocol.activity_id());
    connection.corrupt_account_root = Some(identifier);
    let mut retry = context;
    retry.correlation_id += 100;
    assert!(matches!(
        proof_bundle(connection, selector, retry, &bridge_registry()),
        Err(EvidenceError::Account(_))
    ));
}

const CUSTODY_CHAIN_ID: u64 = 31337;
const FUNDING_AMOUNT: u128 = 100_000_000_000_000;

pub(super) struct CustodyChain {
    root: PathBuf,
    nodes: Vec<Daemon>,
    evidence_arguments: Vec<String>,
}

impl CustodyChain {
    pub(super) fn verify_evidence(&self) {
        producer("test_evidence.py", &self.evidence_arguments);
        println!("real custody Python evidence suite passed");
    }
}

impl Drop for CustodyChain {
    fn drop(&mut self) {
        for node in &mut self.nodes {
            node.stop();
        }
        if thread::panicking() {
            for node in &self.nodes {
                eprintln!("custody Anvil stderr:\n{}", node.diagnostics());
            }
            eprintln!("custody artifacts retained at {}", self.root.display());
        } else {
            let _ = fs::remove_dir_all(&self.root);
        }
    }
}

fn producer(script: &str, arguments: &[String]) {
    let script = repository_root().join("tests/bridge").join(script);
    let output = must(
        Command::new("python3").arg(script).args(arguments).output(),
        "custody producer",
    );
    assert!(
        output.status.success(),
        "custody producer failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    eprintln!("{}", String::from_utf8_lossy(&output.stdout));
    eprintln!("{}", String::from_utf8_lossy(&output.stderr));
}

fn start_anvil(chain: &mut CustodyChain, fork: Option<(&str, u64)>) -> String {
    let port = loop {
        let candidate = free_port();
        if candidate != 18545 {
            break candidate;
        }
    };
    let stderr = chain.root.join(format!("anvil-{port}.stderr"));
    let mut invocation = Command::new("anvil");
    invocation.args([
        "--host",
        "127.0.0.1",
        "--port",
        &port.to_string(),
        "--chain-id",
        &CUSTODY_CHAIN_ID.to_string(),
        "--silent",
    ]);
    if let Some((url, block)) = fork {
        invocation.args(["--fork-url", url, "--fork-block-number", &block.to_string()]);
    }
    let child = must(
        invocation
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::from(must(fs::File::create(&stderr), "Anvil stderr")))
            .spawn(),
        "start isolated Anvil",
    );
    let mut daemon = Daemon { child, stderr };
    wait_for_port(port, &mut daemon, "custody Anvil");
    chain.nodes.push(daemon);
    format!("http://127.0.0.1:{port}")
}

fn bridge_registry() -> ModuleRegistry {
    let activity = must(
        ActivityType::new(ModuleId::Bridge, 1),
        "custody credit ordinal",
    );
    let registration = must(
        ModuleRegistration::new(ModuleId::Bridge, &[activity]),
        "custody module",
    );
    must(ModuleRegistry::new(&[registration]), "custody registry")
}

fn reserve_account() -> [u8; 32] {
    let reserve_name = b"system:paxeer-reserve";
    let mut reserve = Sha256::new();
    reserve.update(b"LX:ACCOUNT:v1");
    reserve.update(must(u32::try_from(reserve_name.len()), "reserve name length").to_be_bytes());
    reserve.update(reserve_name);
    reserve.finalize().into()
}

fn verify_funding_projection(cluster: &Cluster, protocol: &layerx_wire::receipt::ProtocolReceipt) {
    assert_eq!(
        protocol.previous_state_root(),
        cluster.genesis_receipt_state_root
    );
    assert_eq!(protocol.operation(), 0);
    assert_eq!(protocol.asset(), [0; 32]);
    assert_eq!(protocol.amount(), 0);
    assert_eq!(protocol.from(), [0; 32]);
    assert_eq!(protocol.to(), [0; 32]);
    assert_eq!(protocol.debit_sequence(), 0);
    assert_eq!(protocol.debit_balance_before(), 0);
    assert_eq!(protocol.debit_balance_after(), 0);
    assert_eq!(protocol.credit_balance_before(), 0);
    assert_eq!(protocol.credit_balance_after(), 0);
    assert_eq!(protocol.authorization_hash(), [0; 32]);
    assert_eq!(protocol.context_hash(), [0; 32]);
    assert_eq!(protocol.transfer_set_root(), [0; 32]);
}

fn verify_funding_effects(protocol: &layerx_wire::receipt::ProtocolReceipt, credit: &[u8]) {
    let effects = protocol.effects();
    assert_eq!(effects.len(), 3);
    let transfer = &effects[0];
    assert_eq!(transfer.module_id(), 8);
    assert_eq!(transfer.kind(), 2);
    assert!(transfer.monetary());
    assert_ne!(transfer.transfer_set_root(), [0; 32]);
    assert_eq!(transfer.event_type(), 0);
    assert!(transfer.body().is_empty());
    for (effect, event_type, length) in [(&effects[1], 1, 208), (&effects[2], 2, 112)] {
        assert_eq!(effect.module_id(), 8);
        assert_eq!(effect.kind(), 3);
        assert_eq!(effect.event_type(), event_type);
        assert!(!effect.monetary());
        assert_eq!(effect.transfer_set_root(), [0; 32]);
        assert_eq!(effect.body().len(), length);
    }
    let mut expected = credit[43..139].to_vec();
    expected.extend_from_slice(&FUNDING_AMOUNT.to_be_bytes());
    expected.extend_from_slice(&credit[5..37]);
    expected.extend_from_slice(&Sha256::digest(credit));
    expected.extend_from_slice(&0_u128.to_be_bytes());
    expected.extend_from_slice(&FUNDING_AMOUNT.to_be_bytes());
    assert_eq!(effects[1].body(), expected);
    let balances = effects[2].body();
    assert_eq!(&balances[..32], &reserve_account());
    assert_eq!(
        &balances[32..48],
        &0_u128.to_be_bytes(),
        "reserve before is committed genesis, not interim issuance"
    );
    assert_eq!(&balances[48..64], &0_u128.to_be_bytes());
    assert_eq!(&balances[64..80], &0_u128.to_be_bytes());
    assert_eq!(&balances[80..96], &FUNDING_AMOUNT.to_be_bytes());
    let before = u64::from_be_bytes(must(
        balances[96..104].try_into(),
        "reserve before sequence",
    ));
    let after = u64::from_be_bytes(must(
        balances[104..112].try_into(),
        "reserve after sequence",
    ));
    assert_eq!(Some(after), before.checked_add(1));
}

fn verify_funding_receipt(cluster: &Cluster, signed: &[u8], answer: &HttpAnswer) {
    assert_eq!(
        answer.status,
        200,
        "funding must succeed: {}",
        answer.text()
    );
    let activity = must(
        decode_signed(signed, &bridge_registry()),
        "C-signed credit activity",
    );
    assert_eq!(activity.activity_type().module(), ModuleId::Bridge);
    assert_eq!(activity.activity_type().ordinal(), 1);
    assert_eq!(activity.protocol_version(), PROTOCOL_VERSION);
    assert_eq!(activity.network_id(), NETWORK_ID);
    assert_eq!(activity.account_sequence(), 1);
    assert_eq!(activity.payload().len(), 427);
    assert_eq!(&activity.payload()[75..107], &cluster.asset);
    assert_eq!(&activity.payload()[107..139], &cluster.actor.source);
    assert_eq!(
        &activity.payload()[139..171],
        &cluster.actor.signing_key.verifying_key().to_bytes()
    );
    assert_eq!(&activity.payload()[191..207], &FUNDING_AMOUNT.to_be_bytes());
    let mut nullifier = Sha256::new();
    nullifier.update(b"LX:DEPOSIT:NULLIFIER:v1");
    nullifier.update(&activity.payload()[43..75]);
    let nullifier: [u8; 32] = nullifier.finalize().into();
    assert_eq!(activity.idempotency_key(), nullifier);
    let expected_id = must(activity_id(&activity), "funding activity id");
    let document = answer.json();
    let result = &document["result"];
    assert_eq!(field(result, "state"), "completed");
    assert_eq!(field(result, "activity_id"), hex(&expected_id));
    let bytes = unhex(field(result, "receipt"));
    let receipt = must(
        verify_sequencer_signature(&bytes, cluster.sequencer_key),
        "funding receipt signature",
    );
    let protocol = receipt
        .protocol()
        .unwrap_or_else(|| panic!("funding protocol receipt required"));
    assert_eq!(protocol.activity_id(), expected_id);
    assert_eq!(protocol.protocol_version(), PROTOCOL_VERSION);
    assert_eq!(protocol.module_id(), 8);
    assert_eq!(protocol.module_version(), 1);
    assert_eq!(
        protocol.result_code(),
        0,
        "refused funding cannot qualify escrow"
    );
    verify_funding_projection(cluster, protocol);
    verify_funding_effects(protocol, activity.payload());
    let authorization = verify_funding_batch(cluster, &receipt, &bytes);
    verify_funded_accounts(cluster, protocol, authorization);
}

fn verify_funding_batch(
    cluster: &Cluster,
    receipt: &layerx_wire::receipt::Receipt,
    bytes: &[u8],
) -> SequencerAuthorization {
    let protocol = receipt
        .protocol()
        .unwrap_or_else(|| panic!("funding protocol receipt required"));
    let digest = must(
        receipt_digest(&must(encode_unsigned(receipt), "unsigned funding receipt")),
        "funding receipt digest",
    );
    let evidence = http_get(
        cluster.program_port,
        &format!(
            "/v1/batches/{}/receipt-authority?receipt_digest={}",
            hex(&protocol.batch_id()),
            hex(&digest),
        ),
        &cluster.program_token,
    );
    assert_eq!(evidence.status, 200, "{}", evidence.text());
    let document = evidence.json();
    assert_eq!(
        field(&document, "sequencer_public_key"),
        hex(&cluster.sequencer_key)
    );
    let evidence = &document["batch_evidence"];
    let header_bytes = unhex(field(evidence, "header_hex"));
    let header = must(decode_batch_header(&header_bytes), "funding header");
    assert_eq!(header.protocol_version(), PROTOCOL_VERSION);
    assert_eq!(header.network_id(), NETWORK_ID);
    assert!(
        (header.first_sequence()..=header.last_sequence()).contains(&protocol.global_sequence())
    );
    let signature = must(
        <[u8; 64]>::try_from(unhex(field(evidence, "header_signature"))),
        "funding header signature",
    );
    let wire_proof = must(
        decode_merkle_proof(&unhex(field(evidence, "receipt_proof_hex"))),
        "funding proof",
    );
    let proof = must(
        Proof::new(
            wire_proof.leaf_index(),
            wire_proof.leaf_count(),
            wire_proof.siblings().to_vec(),
        ),
        "funding inclusion proof",
    );
    let authorization = SequencerAuthorization::new(
        header.sequencer_id(),
        cluster.sequencer_key,
        FIRST_BATCH,
        LAST_BATCH,
    );
    must(
        verify_receipt(bytes, &proof, &header_bytes, &signature, &authorization),
        "offline funding inclusion",
    );
    let activity = funding_activity_batch(bytes, evidence, &authorization);
    assert_eq!(protocol.batch_id(), activity.batch_id());
    assert_eq!(protocol.previous_state_root(), header.previous_state_root());
    assert_eq!(
        protocol.resulting_state_root(),
        activity.resulting_state_root()
    );
    let mut tampered = bytes.to_vec();
    let last = tampered.len() - 1;
    tampered[last] ^= 1;
    assert!(verify_receipt(&tampered, &proof, &header_bytes, &signature, &authorization).is_err());
    authorization
}

fn funding_activity_batch(
    bytes: &[u8],
    evidence: &serde_json::Value,
    authorization: &SequencerAuthorization,
) -> layerx_proof::receipt::AuthorizedBatch {
    use layerx_proof::receipt::{
        authorized_maintained_activity_batch, AuthorizedBatch, MaintainedOutcomeEvidence,
    };
    let receipt = must(layerx_wire::receipt::decode(bytes), "funding receipt");
    let protocol = receipt
        .protocol()
        .unwrap_or_else(|| panic!("funding protocol"));
    let header_bytes = unhex(field(evidence, "header_hex"));
    let header = must(decode_batch_header(&header_bytes), "funding header");
    let authority = AuthorizedBatch::new(
        protocol.batch_id(),
        protocol.asset(),
        header.previous_state_root(),
        header.resulting_state_root(),
        authorization.public_key(),
    );
    let Some(identity) = evidence.get("batch_identity") else {
        assert_eq!(
            protocol.batch_id(),
            must(
                receipt_execution_batch_id(protocol, &header),
                "funding batch id"
            )
        );
        return authority;
    };
    assert_eq!(field(identity, "kind"), "occupancy_maintenance_v2");
    let decode_proof = |value: &serde_json::Value| {
        let wire = must(
            decode_merkle_proof(&unhex(field(value, "receipt_proof_hex"))),
            "funding wire proof",
        );
        must(
            Proof::new(
                wire.leaf_index(),
                wire.leaf_count(),
                wire.siblings().to_vec(),
            ),
            "funding Merkle proof",
        )
    };
    let proof = decode_proof(evidence);
    let maintenance_proof = decode_proof(identity);
    let maintenance = unhex(field(identity, "receipt_hex"));
    let signature = must(
        <[u8; 64]>::try_from(unhex(field(evidence, "header_signature"))),
        "funding header signature",
    );
    let activity = must(
        authorized_maintained_activity_batch(
            bytes,
            &authority,
            &MaintainedOutcomeEvidence {
                header: &header_bytes,
                header_signature: &signature,
                activity_proof: &proof,
                maintenance: &maintenance,
                maintenance_proof: &maintenance_proof,
                authorization,
            },
        ),
        "authenticated funding maintenance",
    );
    let record = must(
        layerx_wire::maintenance::decode_occupancy_maintenance(&maintenance),
        "funding maintenance",
    );
    assert_eq!(header.resulting_state_root(), record.resulting_state_root);
    assert_eq!(protocol.resulting_state_root(), record.previous_state_root);
    assert_eq!(activity.resulting_state_root(), record.previous_state_root);
    activity
}

fn credit_actor(cluster: &Cluster, root: &Path, profile: &Path, credit: &Path, actor_key: &Path) {
    let sign_credit_binary = repository_root().join("build/tests/bridge/sign-credit");
    assert!(
        sign_credit_binary.is_file(),
        "parent must build {}",
        sign_credit_binary.display()
    );
    let signed_path = root.join("signed-credit.bin");
    command(
        &text(&sign_credit_binary),
        &[
            &text(profile),
            &text(credit),
            &cluster.actor.did,
            &text(actor_key),
            "1",
            &now_ms().saturating_sub(1000).to_string(),
            &text(&signed_path),
        ],
    );
    let signed = must(fs::read(&signed_path), "native signed custody credit");
    let credit_test_binary = repository_root().join("build/tests/bridge/test-credit");
    assert!(
        credit_test_binary.is_file(),
        "parent must build {}",
        credit_test_binary.display()
    );
    command(
        &text(&credit_test_binary),
        &[
            &text(&cluster.root.join("genesis/artifacts/genesis.manifest")),
            &text(&signed_path),
            &text(actor_key),
        ],
    );
    eprintln!(
        "native custody credit gates passed: {}",
        credit_test_binary.display()
    );
    let activity = must(
        decode_signed(&signed, &bridge_registry()),
        "credit idempotency key",
    );
    let key = hex(&activity.idempotency_key());
    let deadline = Instant::now() + Duration::from_secs(60);
    let answer = loop {
        let answer = cluster.client.call(&Call::submit(
            "/v1/activities",
            &cluster.gateway_token,
            &key,
            &signed,
        ));
        if answer.status != 202 || Instant::now() >= deadline {
            break answer;
        }
        thread::sleep(Duration::from_millis(100));
    };
    verify_funding_receipt(cluster, &signed, &answer);
    let replay = cluster.client.call(&Call::submit(
        "/v1/activities",
        &cluster.gateway_token,
        &key,
        &signed,
    ));
    assert_eq!(replay.status, 200, "{}", replay.text());
    assert_eq!(replay.text(), answer.text());
    assert_eq!(journal_record(cluster, &key)["attempts"], 1);
}

pub(super) fn start_funded_cluster() -> (Cluster, CustodyChain) {
    let root =
        std::env::temp_dir().join(format!("layerx-custody-{}-{}", std::process::id(), token()));
    make_dir(&root, 0o700);
    let mut chain = CustodyChain {
        root,
        nodes: Vec::new(),
        evidence_arguments: Vec::new(),
    };
    let actor = actor();
    let asset = format!("0x{}", hex(&random32()));
    let beneficiary = format!("0x{}", hex(&actor.source));
    let actor_key = chain.root.join("actor.key");
    let attestor_key = chain.root.join("attestor.key");
    write(&actor_key, &actor.signing_key.to_bytes(), 0o600);
    write(&attestor_key, &random32(), 0o600);
    let primary = start_anvil(&mut chain, None);
    let deployment_path = chain.root.join("deployment.json");
    producer(
        "deploy_local_custody.py",
        &[
            "--rpc".into(),
            primary.clone(),
            "--asset".into(),
            asset.clone(),
            "--beneficiary".into(),
            beneficiary.clone(),
            "--amount".into(),
            FUNDING_AMOUNT.to_string(),
            "--output".into(),
            text(&deployment_path),
            "--allow-local-chain".into(),
        ],
    );
    let deployment: serde_json::Value = must(
        serde_json::from_slice(&must(fs::read(&deployment_path), "real custody deployment")),
        "custody deployment JSON",
    );
    assert_eq!(deployment["chain_id"], CUSTODY_CHAIN_ID);
    assert_eq!(field(&deployment, "beneficiary"), beneficiary);
    assert_eq!(field(&deployment, "asset"), asset);
    assert_eq!(field(&deployment, "amount"), FUNDING_AMOUNT.to_string());
    let (profile, credit) = produce_custody_credit(
        &mut chain,
        &primary,
        &deployment,
        &actor,
        &asset,
        &attestor_key,
    );
    let cluster = start_cluster_with_custody(Some(CustodySetup {
        profile_path: profile.clone(),
        actor,
    }));
    check_readiness(&cluster);
    credit_actor(&cluster, &chain.root, &profile, &credit, &actor_key);
    (cluster, chain)
}

fn produce_custody_credit(
    chain: &mut CustodyChain,
    primary: &str,
    deployment: &serde_json::Value,
    actor: &Actor,
    asset: &str,
    attestor_key: &Path,
) -> (PathBuf, PathBuf) {
    let beneficiary = format!("0x{}", hex(&actor.source));
    let beneficiary_key = format!("0x{}", hex(&actor.signing_key.verifying_key().to_bytes()));
    let fork_block = deployment["fork_block"]
        .as_u64()
        .unwrap_or_else(|| panic!("custody fork block missing"));
    let secondary = start_anvil(chain, Some((primary, fork_block)));
    assert_ne!(primary, secondary);
    let profile = chain.root.join("profile.bin");
    producer(
        "custody_credit.py",
        &[
            "profile".into(),
            "--rpc".into(),
            primary.to_owned(),
            "--rpc".into(),
            secondary.clone(),
            "--network-id".into(),
            NETWORK_ID.to_string(),
            "--chain-id".into(),
            CUSTODY_CHAIN_ID.to_string(),
            "--vault".into(),
            field(deployment, "vault").into(),
            "--runtime-sha256".into(),
            field(deployment, "runtime_sha256").into(),
            "--asset".into(),
            asset.to_owned(),
            "--confirmations".into(),
            "64".into(),
            "--attestor-key".into(),
            text(attestor_key),
            "--output".into(),
            text(&profile),
        ],
    );
    assert_eq!(
        must(fs::metadata(&profile), "custody profile metadata").len(),
        must(u64::try_from(CUSTODY_PROFILE_BYTES), "profile length")
    );
    let credit = chain.root.join("credit.bin");
    let evidence_arguments = vec![
        "--rpc".into(),
        primary.to_owned(),
        "--rpc".into(),
        secondary,
        "--profile".into(),
        text(&profile),
        "--network-id".into(),
        NETWORK_ID.to_string(),
        "--transaction".into(),
        field(deployment, "transaction").into(),
        "--beneficiary".into(),
        beneficiary,
        "--beneficiary-key".into(),
        beneficiary_key,
        "--expected-amount".into(),
        FUNDING_AMOUNT.to_string(),
        "--attestor-key".into(),
        text(attestor_key),
    ];
    let mut attest_arguments = vec!["attest".into()];
    attest_arguments.extend(evidence_arguments.clone());
    attest_arguments.extend(["--output".into(), text(&credit)]);
    producer("custody_credit.py", &attest_arguments);
    chain.evidence_arguments = evidence_arguments;
    (profile, credit)
}

#[test]
fn maintained_program_journal_rejects_missing_corrupt_and_substituted_attachments() {
    let cluster = start_cluster();
    check_readiness(&cluster);
    let signed = signed_program_call(&cluster.actor, random32());
    let key = format!("maintenance-{}", token());
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
    let key_digest = format!("{:x}", Sha256::digest(key.as_bytes()));
    let journal = cluster
        .state_dir
        .join("journal")
        .join(format!("{key_digest}.json"));
    let original = must(fs::read(&journal), "maintained journal");
    let document: serde_json::Value =
        must(serde_json::from_slice(&original), "maintained journal JSON");
    assert_eq!(
        document["program_execution"]["evidence"]["batch_identity"]["kind"],
        "occupancy_maintenance_v2"
    );
    for case in 0..5 {
        let mut corrupted = document.clone();
        let evidence = &mut corrupted["program_execution"]["evidence"];
        match case {
            0 => {
                evidence
                    .as_object_mut()
                    .unwrap_or_else(|| panic!("batch evidence"))
                    .remove("batch_identity");
            }
            1 => {
                evidence["batch_identity"] = serde_json::json!({"kind": "historical"});
            }
            2 => {
                let mut bytes = unhex(field(&evidence["batch_identity"], "receipt_hex"));
                let last = bytes.len() - 1;
                bytes[last] ^= 1;
                evidence["batch_identity"]["receipt_hex"] = serde_json::json!(hex(&bytes));
            }
            3 => {
                evidence["batch_identity"]["receipt_proof_hex"] =
                    evidence["receipt_proof_hex"].clone();
            }
            4 => {
                evidence["batch_identity"]["kind"] = serde_json::json!("unknown");
            }
            _ => unreachable!(),
        }
        must(
            fs::write(
                &journal,
                must(serde_json::to_vec(&corrupted), "corrupt maintained journal"),
            ),
            "write corrupt maintained journal",
        );
        let refused = cluster.client.call(&Call::submit(
            "/v1/programs/call",
            &cluster.gateway_token,
            &key,
            &signed,
        ));
        let expected_code = if case == 4 {
            "persistence_unavailable"
        } else {
            "program_artifacts_invalid"
        };
        assert_refusal(&refused, 503, expected_code);
    }
    must(fs::write(&journal, original), "restore maintained journal");
    let replay = cluster.client.call(&Call::submit(
        "/v1/programs/call",
        &cluster.gateway_token,
        &key,
        &signed,
    ));
    assert_eq!(replay.status, 200, "{}", replay.text());
    assert_eq!(replay.text(), submitted.text());
    assert_eq!(journal_record(&cluster, &key)["attempts"], 1);
}
