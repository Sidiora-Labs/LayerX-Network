use std::path::PathBuf;
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicU16, Ordering};
use std::thread;
use std::time::Duration;

use ed25519_dalek::SigningKey as ReceiptSigningKey;
use layerx_agent_api::error::RequestId;
use layerx_agent_api::identity::{AgentDid, AuthorityRef};
use layerx_agent_api::prepare::TimestampBound as AgentTimestampBound;
use layerx_agent_api::{Amount as AgentAmount, Sequence, TimestampSeconds};
use layerx_paxeer_client::{
    account_address, raw_call, AgentCreditContext, CreditFault, CreditPath, CustodyFault,
    DepositFailure, DepositProof, EndpointConfig, EndpointTransport, ExecutionOutcome,
    FinalityReport, FinalityStage, FinalityTracker, FinalizedCheckpoint, Json, PaxeerClient,
    ProofFault, TrackerConfig,
    TransactionHash, TransactionInclusion,
};
use layerx_proof::checkpoint::{
    checkpoint_id, Attestation, Certificate, Checkpoint, CheckpointError, GuarantorKey,
};
use layerx_proof::receipt::AuthorizedBatch;
use layerx_sdk::{Client as AgentClient, Deployment, Operation};
use layerx_types::account::AccountId;
use layerx_types::amount::Amount;
use layerx_types::ids::{AssetId, CheckpointId};
use layerx_types::intent::EvmAddress;
use layerx_types::payload::{ActivityType, ModuleId, ModuleRegistration, ModuleRegistry};

const FUNDED: &str = "0xf39Fd6e51aad88F6F4ce6aB8827279cffFb92266";
const EMERGENCY: &str = "0x70997970C51812dc3A010C7d01b50e0d17dc79C8";
const TOKEN_CREATION: &str = include_str!("contracts/IntegrationToken.hex");
const REGISTRY_CREATION: &str = include_str!("contracts/AssetRegistry.hex");
const VAULT_CREATION: &str = include_str!("contracts/LayerXVault.hex");
const ASSET: [u8; 32] = [0x42; 32];
const AMOUNT: u128 = 25;
const CORE_NETWORK: u32 = 17;
const CHECKPOINT_HEADER_HEX: &str = "000117010f010001020000001103000000000000000704000000000000000805000000000000000b0600000000000000130700000020070707070707070707070707070707070707070707070707070707070707070708000000200808080808080808080808080808080808080808080808080808080808080808090000002009090909090909090909090909090909090909090909090909090909090909090a000000200a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0b000000200b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0c000000200c0c0c0c0c0c0c0c0c0c0c0c0c0c0c0c0c0c0c0c0c0c0c0c0c0c0c0c0c0c0c0c0d000000200d0d0d0d0d0d0d0d0d0d0d0d0d0d0d0d0d0d0d0d0d0d0d0d0d0d0d0d0d0d0d0d0e00000000000003e80f000000200f0f0f0f0f0f0f0f0f0f0f0f0f0f0f0f0f0f0f0f0f0f0f0f0f0f0f0f0f0f0f0f";
const CHECKPOINT_ID_HEX: &str = "5cd43e8c1a6a0ba5594d75846fe40bd851909368fc0a7439657180c5fb8b9572";
const CREDIT_ACTIVITY_ID_HEX: &str =
    "3ad4f279bd6297c488fbf76c0802a3fb20c5060d955a7648e6417f8653f8fa11";
const CREDIT_RECEIPT_HEX: &str = "000152010001000000203ad4f279bd6297c488fbf76c0802a3fb20c5060d955a7648e6417f8653f8fa110000000000000009000000200707070707070707070707070707070707070707070707070707070707070707000000200808080808080808080808080808080808080808080808080808080808080808000000200808080808080808080808080808080808080808080808080808080808080808000000000000000000000000000000000000000000000001000000205cd43e8c1a6a0ba5594d75846fe40bd851909368fc0a7439657180c5fb8b957200080000000100000001010000002042424242424242424242424242424242424242424242424242424242424242420000000000000000000000000000001900000020f94d2cc01cae556915267bc3d1ad7c58034009ea25cbe56906be12b9ca876de0000000000000000000000000000000640000000000000000000000000000004b00000000000000010000002042de0bc2f3c75fd9995e3ad3d57efaf06530b93679d956ddf17fa9d325e1d60d0000000000000000000000000000000a00000000000000000000000000000023000000200909090909090909090909090909090909090909090909090909090909090909000000200a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a000000200b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b00000000000003e80100000040d8bdb7a072cdbc700f7390c482c40823192d2bfc983749456a20e08dc42f54526e153909c707f05d141c20ec9a728f6358fe1a460f6bf7ca7171758511b02e0d";
const GUARANTOR_PUBLIC_KEYS: [&str; 3] = [
    "0279be667ef9dcbbac55a06295ce870b07029bfcdb2dce28d959f2815b16f81798",
    "02c6047f9441ed7d6d3045406e95c07cd85c778e4b8cef3ca7abac09b95c709ee5",
    "02f9308a019258c31049344f85f89d5229b531c845836f99b08601f113bce036f9",
];
const GUARANTOR_SIGNATURES: [&str; 3] = [
    "f9ea8b1dc2d6f6db15ac68bf2ac645081310822839fe31e40fca5f37665d8e2224a16055357219970908e922b785542548f2406ac2bfae1a8d420b613d3c701e",
    "ee20fdb15279bda8a90b66b5b3817ed294ec8362948665bc066c8b90a95366cb34902aa1845f66e66c97889b0039a7115463760ce0eb7a7620d8d3d857342aad",
    "cdaac951fb38f3e4d713fcb46a19d41caa60811be4532acdab7b7dc99d2d1de47afbd9349d93eaf98cf464a16b3a0b08229200f403048230d164c3abed716389",
];

static NEXT_PORT: AtomicU16 = AtomicU16::new(0);

fn next_port() -> u16 {
    let offset = NEXT_PORT.fetch_add(1, Ordering::Relaxed);
    let pid_lane = u16::try_from(std::process::id() % 8_000).unwrap_or(0);
    21_000_u16
        .saturating_add(pid_lane)
        .saturating_add(offset.saturating_mul(7))
}

fn anvil_binary() -> PathBuf {
    let foundry = PathBuf::from("/root/.foundry/bin/anvil");
    if foundry.exists() {
        foundry
    } else {
        PathBuf::from("anvil")
    }
}

struct Anvil {
    child: Child,
    endpoint: EndpointConfig,
}

impl Anvil {
    fn launch() -> Self {
        for _ in 0..8 {
            let port = next_port();
            let endpoint = EndpointConfig {
                url: format!("http://127.0.0.1:{port}"),
                request_timeout: Duration::from_secs(5),
                transport: EndpointTransport::LocalEmulator,
                expected_chain_id: 31_337,
            };
            let child = Command::new(anvil_binary())
                .arg("--port")
                .arg(port.to_string())
                .arg("--silent")
                .stdin(Stdio::null())
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .spawn()
                .unwrap_or_else(|error| panic!("spawn anvil: {error}"));
            let mut anvil = Self { child, endpoint };
            if anvil.ready() {
                return anvil;
            }
            anvil.halt();
        }
        panic!("no free port for anvil");
    }

    fn ready(&self) -> bool {
        for _ in 0..100 {
            if raw_call(&self.endpoint, "eth_blockNumber", &[]).is_ok() {
                return true;
            }
            thread::sleep(Duration::from_millis(50));
        }
        false
    }

    fn call(&self, method: &str, params: &[Json]) -> Json {
        raw_call(&self.endpoint, method, params)
            .unwrap_or_else(|failure| panic!("{method}: {failure:?}"))
    }

    fn send(&self, to: Option<EvmAddress>, data: &str) -> TransactionHash {
        let mut fields = vec![text_member("from", FUNDED), text_member("data", data)];
        if let Some(address) = to {
            fields.push(text_member("to", &address_hex(address)));
        }
        fields.push(text_member("gas", "0x989680"));
        let result = self.call("eth_sendTransaction", &[Json::Object(fields)]);
        let hash = result
            .as_text()
            .unwrap_or_else(|| panic!("eth_sendTransaction: expected hash"));
        TransactionHash::from_hex(hash)
            .unwrap_or_else(|error| panic!("transaction hash: {error:?}"))
    }

    fn deploy(&self, creation: &str, arguments: &[[u8; 32]]) -> EvmAddress {
        let transaction = self.send(None, &calldata(creation.trim(), arguments));
        let receipt = wait_receipt(&client(self), transaction);
        assert_eq!(receipt.execution, ExecutionOutcome::Succeeded);
        receipt
            .deployed_contract
            .unwrap_or_else(|| panic!("deployment receipt omitted contract address"))
    }

    fn halt(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

impl Drop for Anvil {
    fn drop(&mut self) {
        self.halt();
    }
}

fn text_member(name: &str, value: &str) -> (String, Json) {
    (name.to_owned(), Json::Text(value.to_owned()))
}

fn client(anvil: &Anvil) -> PaxeerClient {
    PaxeerClient::new(vec![anvil.endpoint.clone()])
        .unwrap_or_else(|error| panic!("client: {error:?}"))
}

fn wait_receipt(reader: &PaxeerClient, transaction: TransactionHash) -> TransactionInclusion {
    for _ in 0..200 {
        if let Some(receipt) = reader
            .transaction_receipt(transaction)
            .unwrap_or_else(|error| panic!("receipt: {error:?}"))
        {
            return receipt;
        }
        thread::sleep(Duration::from_millis(20));
    }
    panic!("transaction was not included in time");
}

fn final_report(anvil: &Anvil, transaction: TransactionHash) -> FinalityReport {
    let mut tracker = FinalityTracker::new(
        TrackerConfig {
            endpoints: vec![anvil.endpoint.clone()],
            minimum_endpoint_agreement: 1,
            required_confirmations: 1,
            poll_cadence: Duration::from_millis(20),
            delayed_after_polls: 100,
        },
        transaction,
    )
    .unwrap_or_else(|error| panic!("tracker: {error:?}"));
    for _ in 0..200 {
        let report = tracker.poll();
        if matches!(report.stage(), FinalityStage::Final { .. }) {
            return report;
        }
        thread::sleep(Duration::from_millis(20));
    }
    panic!("transaction did not finalize in time");
}

fn hex_nibble(byte: u8) -> u8 {
    match byte {
        b'0'..=b'9' => byte - b'0',
        b'a'..=b'f' => byte - b'a' + 10,
        b'A'..=b'F' => byte - b'A' + 10,
        _ => panic!("non-hex digit"),
    }
}

fn hex_bytes(value: &str) -> Vec<u8> {
    assert_eq!(value.len() % 2, 0);
    value
        .as_bytes()
        .chunks_exact(2)
        .map(|pair| (hex_nibble(pair[0]) << 4) | hex_nibble(pair[1]))
        .collect()
}

fn hex_array<const N: usize>(value: &str) -> [u8; N] {
    hex_bytes(value)
        .try_into()
        .unwrap_or_else(|bytes: Vec<u8>| panic!("expected {N} bytes, got {}", bytes.len()))
}

fn parse_address(value: &str) -> EvmAddress {
    let digits = value
        .strip_prefix("0x")
        .unwrap_or_else(|| panic!("address has no prefix"));
    assert_eq!(digits.len(), 40);
    let mut address = [0_u8; 20];
    for (slot, pair) in address.iter_mut().zip(digits.as_bytes().chunks_exact(2)) {
        *slot = (hex_nibble(pair[0]) << 4) | hex_nibble(pair[1]);
    }
    EvmAddress::new(address)
}

fn address_word(address: EvmAddress) -> [u8; 32] {
    let mut word = [0_u8; 32];
    word[12..].copy_from_slice(&address.bytes());
    word
}

fn u128_word(value: u128) -> [u8; 32] {
    let mut word = [0_u8; 32];
    word[16..].copy_from_slice(&value.to_be_bytes());
    word
}

fn byte_word(value: u8) -> [u8; 32] {
    let mut word = [0_u8; 32];
    word[31] = value;
    word
}

fn calldata(prefix: &str, words: &[[u8; 32]]) -> String {
    const DIGITS: &[u8; 16] = b"0123456789abcdef";
    let mut encoded = prefix.to_owned();
    for word in words {
        for byte in word {
            encoded.push(char::from(DIGITS[usize::from(byte >> 4)]));
            encoded.push(char::from(DIGITS[usize::from(byte & 0x0f)]));
        }
    }
    encoded
}

fn address_hex(address: EvmAddress) -> String {
    format!("0x{}", &calldata("0x", &[address_word(address)])[26..])
}

fn deploy_custody(anvil: &Anvil) -> (EvmAddress, EvmAddress) {
    let owner = parse_address(FUNDED);
    let emergency = parse_address(EMERGENCY);
    let token = anvil.deploy(TOKEN_CREATION, &[address_word(owner)]);
    let registry = anvil.deploy(
        REGISTRY_CREATION,
        &[
            address_word(owner),
            address_word(emergency),
            [7; 32],
            u128_word(1),
        ],
    );
    let vault = anvil.deploy(
        VAULT_CREATION,
        &[
            address_word(registry),
            address_word(owner),
            address_word(emergency),
            [8; 32],
            u128_word(1),
        ],
    );
    assert_eq!(
        wait_receipt(
            &client(anvil),
            anvil.send(
                Some(token),
                &calldata("0x40c10f19", &[address_word(owner), u128_word(100)]),
            ),
        )
        .execution,
        ExecutionOutcome::Succeeded
    );
    assert_eq!(
        wait_receipt(
            &client(anvil),
            anvil.send(
                Some(registry),
                &calldata(
                    "0xea249288",
                    &[
                        ASSET,
                        address_word(token),
                        byte_word(6),
                        u128_word(1),
                        u128_word(1_000),
                    ],
                ),
            ),
        )
        .execution,
        ExecutionOutcome::Succeeded
    );
    assert_eq!(
        wait_receipt(
            &client(anvil),
            anvil.send(
                Some(token),
                &calldata("0x095ea7b3", &[address_word(vault), u128_word(AMOUNT)],),
            ),
        )
        .execution,
        ExecutionOutcome::Succeeded
    );
    (token, vault)
}

fn checkpoint_header() -> Vec<u8> {
    hex_bytes(CHECKPOINT_HEADER_HEX)
}

fn guarantor(value: u8) -> (Attestation, GuarantorKey) {
    let index = usize::from(value.saturating_sub(1));
    let identifier = hex_array(CHECKPOINT_ID_HEX);
    let mut id = [0_u8; 32];
    id[0] = value;
    let public = hex_array(GUARANTOR_PUBLIC_KEYS[index]);
    (
        Attestation::new(
            identifier,
            identifier,
            id,
            8,
            [12; 32],
            true,
            true,
            0x1f,
            1_000 + u64::from(value),
            hex_array(GUARANTOR_SIGNATURES[index]),
        ),
        GuarantorKey::new(id, public, true),
    )
}

fn finalized_checkpoint() -> (FinalizedCheckpoint, Certificate, Vec<GuarantorKey>) {
    let checkpoint = Checkpoint::new(checkpoint_header(), b"REAL-CORE-PROOF".to_vec());
    let identifier = checkpoint_id(&checkpoint)
        .unwrap_or_else(|error| panic!("checkpoint identifier: {error:?}"));
    assert_eq!(identifier, hex_array(CHECKPOINT_ID_HEX));
    let mut attestations = Vec::new();
    let mut bonded = Vec::new();
    for value in 1..=3 {
        let (attestation, key) = guarantor(value);
        attestations.push(attestation);
        bonded.push(key);
    }
    let certificate = Certificate::new(checkpoint, attestations, 2, None);
    let finalized =
        FinalizedCheckpoint::verify(&certificate, &bonded, CheckpointId::new(identifier), None)
            .unwrap_or_else(|error| panic!("finalized checkpoint: {error:?}"));
    (finalized, certificate, bonded)
}

fn bridge_registry() -> ModuleRegistry {
    let credit = ActivityType::new(ModuleId::Bridge, 1)
        .unwrap_or_else(|error| panic!("credit activity: {error:?}"));
    let registration = ModuleRegistration::new(ModuleId::Bridge, &[credit])
        .unwrap_or_else(|error| panic!("bridge registration: {error:?}"));
    ModuleRegistry::new(&[registration])
        .unwrap_or_else(|error| panic!("module registry: {error:?}"))
}

fn agent_context() -> AgentCreditContext {
    AgentCreditContext {
        request_id: RequestId(41),
        actor: AgentDid::new("did:layerx:deposit-recipient")
            .unwrap_or_else(|error| panic!("actor DID: {error:?}")),
        authority: AuthorityRef::new("owner:deposit-owner")
            .unwrap_or_else(|error| panic!("owner authority: {error:?}")),
        account_sequence: Sequence(5),
        timestamp_bound: AgentTimestampBound {
            not_before: TimestampSeconds(995),
            not_after: TimestampSeconds(1_010),
        },
        fee_limit: AgentAmount(7),
    }
}

#[derive(Clone)]
struct ReceiptFields {
    activity_id: [u8; 32],
    previous_state_root: [u8; 32],
    resulting_state_root: [u8; 32],
    batch_id: [u8; 32],
    asset: [u8; 32],
    amount: u128,
    from: [u8; 32],
    to: [u8; 32],
}

fn signed_receipt(
    fields: &ReceiptFields,
    signing: &ReceiptSigningKey,
) -> (Vec<u8>, AuthorizedBatch) {
    assert_eq!(fields.activity_id, hex_array(CREDIT_ACTIVITY_ID_HEX));
    assert_eq!(fields.previous_state_root, [7; 32]);
    assert_eq!(fields.resulting_state_root, [8; 32]);
    assert_eq!(fields.batch_id, hex_array(CHECKPOINT_ID_HEX));
    assert_eq!(fields.asset, ASSET);
    assert_eq!(fields.amount, AMOUNT);
    assert_ne!(fields.from, [0; 32]);
    assert_ne!(fields.to, [0; 32]);
    assert_ne!(fields.from, fields.to);
    let batch = AuthorizedBatch::new(
        fields.batch_id,
        fields.asset,
        fields.previous_state_root,
        fields.resulting_state_root,
        signing.verifying_key().to_bytes(),
    );
    (hex_bytes(CREDIT_RECEIPT_HEX), batch)
}

#[test]
#[allow(clippy::too_many_lines)]
fn finalized_custody_is_credited_once_through_the_agent_contract() {
    let anvil = Anvil::launch();
    let (_token, vault) = deploy_custody(&anvil);
    let reserve = AccountId::parse("system:paxeer-reserve")
        .unwrap_or_else(|error| panic!("reserve: {error:?}"));
    let recipient = AccountId::parse("agent:did:layerx:deposit-recipient:main")
        .unwrap_or_else(|error| panic!("recipient: {error:?}"));

    let deposit_transaction = anvil.send(
        Some(vault),
        &calldata(
            "0x8a9e532c",
            &[ASSET, u128_word(AMOUNT), account_address(&recipient)],
        ),
    );
    let report = final_report(&anvil, deposit_transaction);
    let (checkpoint, certificate, bonded) = finalized_checkpoint();
    assert_eq!(checkpoint.network_id(), CORE_NETWORK);
    assert_eq!(checkpoint.state_root(), [8; 32]);
    let proof = DepositProof::obtain_from_certificate(
        &client(&anvil),
        &report,
        vault,
        &certificate,
        &bonded,
        checkpoint.id(),
        None,
    )
    .unwrap_or_else(|error| panic!("deposit proof: {error:?}"));
    assert_eq!(proof.transaction(), deposit_transaction);
    assert_eq!(proof.custody_reference(), deposit_transaction.bytes());
    assert_eq!(proof.custody().asset, AssetId::new(ASSET));
    assert_eq!(proof.custody().amount, Amount::from_u128(AMOUNT));
    assert_eq!(proof.custody().beneficiary, account_address(&recipient));
    assert_eq!(proof.checkpoint(), &checkpoint);
    assert!(proof.finalized());
    assert_ne!(proof.commitment(), [0; 32]);

    let mut insufficient = certificate.clone();
    insufficient = Certificate::new(
        insufficient.checkpoint().clone(),
        insufficient.attestations()[..1].to_vec(),
        2,
        None,
    );
    assert_eq!(
        FinalizedCheckpoint::verify(&insufficient, &bonded, checkpoint.id(), None),
        Err(CheckpointError::Threshold {
            achieved: 1,
            required: 2,
        })
    );
    assert!(matches!(
        DepositProof::obtain_from_certificate(
            &client(&anvil),
            &report,
            vault,
            &insufficient,
            &bonded,
            checkpoint.id(),
            None,
        ),
        Err(DepositFailure::ProofUnavailable(ProofFault::Checkpoint(
            CheckpointError::Threshold {
                achieved: 1,
                required: 2,
            }
        )))
    ));

    let registry = bridge_registry();
    let path = CreditPath::prepare(&proof, &reserve, &recipient, &registry)
        .unwrap_or_else(|error| panic!("credit path: {error:?}"));
    assert_eq!(path.deposit(), proof.deposit_id());
    assert_eq!(path.compiled().activity_type().module(), ModuleId::Bridge);
    assert_eq!(path.compiled().activity_type().ordinal(), 1);
    let agent = AgentClient::daemon(
        "/run/layerx/agent.sock",
        layerx_agent_api::agent_api_schema_v1().version,
    )
    .unwrap_or_else(|error| panic!("agent client: {error:?}"));
    let call = path
        .agent_call(&agent, agent_context())
        .unwrap_or_else(|error| panic!("agent credit call: {error:?}"));
    assert_eq!(call.deployment(), Deployment::Daemon);
    assert_eq!(call.operation(), Operation::Prepare);
    let mutation = call.request();
    assert_eq!(mutation.request_id, RequestId(41));
    assert_eq!(mutation.key.bytes(), path.idempotency_key().bytes());
    assert_ne!(mutation.body_digest.0, [0; 32]);
    assert_eq!(
        hex_bytes(mutation.operation.idempotency_key.as_str()),
        path.idempotency_key().bytes()
    );
    assert_eq!(
        mutation.operation.payload.as_bytes(),
        path.compiled().payload().as_bytes()
    );
    assert_eq!(
        mutation.operation.payload_hash,
        path.compiled().payload_hash()
    );
    let repeated = path
        .agent_call(&agent, agent_context())
        .unwrap_or_else(|error| panic!("repeated agent credit call: {error:?}"));
    assert_eq!(repeated.request(), mutation);

    let signing = ReceiptSigningKey::from_bytes(&[3; 32]);
    let activity_id = hex_array(CREDIT_ACTIVITY_ID_HEX);
    let fields = ReceiptFields {
        activity_id,
        previous_state_root: [7; 32],
        resulting_state_root: checkpoint.state_root(),
        batch_id: checkpoint.id().bytes(),
        asset: ASSET,
        amount: AMOUNT,
        from: account_address(&reserve),
        to: account_address(&recipient),
    };
    let (receipt_bytes, authorized) = signed_receipt(&fields, &signing);
    assert!(matches!(
        proof.accept_credit(
            &receipt_bytes,
            &authorized,
            [0x91; 32],
            &reserve,
            &recipient,
        ),
        Err(DepositFailure::CreditRefused(
            CreditFault::WrongActivity { .. }
        ))
    ));

    let credit = proof
        .accept_credit(
            &receipt_bytes,
            &authorized,
            activity_id,
            &reserve,
            &recipient,
        )
        .unwrap_or_else(|error| panic!("verified credit receipt: {error:?}"));
    assert_eq!(credit.activity_id(), activity_id);
    assert_eq!(credit.amount(), AMOUNT);
}

#[test]
fn deposit_failures_remain_typed_at_each_boundary() {
    let anvil = Anvil::launch();
    let (_token, vault) = deploy_custody(&anvil);
    let recipient = AccountId::parse("agent:did:layerx:deposit-recipient:main")
        .unwrap_or_else(|error| panic!("recipient: {error:?}"));
    let reverted_transaction = anvil.send(
        Some(vault),
        &calldata(
            "0x8a9e532c",
            &[ASSET, u128_word(0), account_address(&recipient)],
        ),
    );
    let report = final_report(&anvil, reverted_transaction);
    let (checkpoint, _, _) = finalized_checkpoint();
    assert!(matches!(
        DepositProof::obtain(&client(&anvil), &report, vault, checkpoint.clone()),
        Err(DepositFailure::CustodyFailed(CustodyFault::Reverted { .. }))
    ));

    let FinalityStage::Final { required, .. } = report.stage()
    else {
        panic!("expected final report");
    };
    let mut confirming = FinalityTracker::new(
        TrackerConfig {
            endpoints: vec![anvil.endpoint.clone()],
            minimum_endpoint_agreement: 1,
            required_confirmations: required.saturating_add(1),
            poll_cadence: Duration::from_millis(20),
            delayed_after_polls: 100,
        },
        reverted_transaction,
    )
    .unwrap_or_else(|error| panic!("confirming tracker: {error:?}"));
    let unavailable = confirming.poll();
    assert!(matches!(
        DepositProof::obtain(&client(&anvil), &unavailable, vault, checkpoint),
        Err(DepositFailure::ProofUnavailable(
            ProofFault::NotFinal { .. }
        ))
    ));
}
