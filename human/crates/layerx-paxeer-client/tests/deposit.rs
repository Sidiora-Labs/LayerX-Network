use std::path::PathBuf;
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicU16, Ordering};
use std::thread;
use std::time::Duration;

use ed25519_dalek::{Signer as _, SigningKey};
use layerx_paxeer_client::{
    account_address, deposit_leaf_bytes, deposit_root_registration_message, raw_call, CreditFault,
    CreditPath, CustodyFault, DepositFailure, DepositProofConfig, DepositProofVerifier,
    DepositRootRegistration, EndpointConfig, EndpointTransport, ExecutionOutcome, FinalityReport,
    FinalityStage, FinalityTracker, Json, PaxeerClient, ProofFault, PublishedDepositProof,
    TrackerConfig, TransactionHash, TransactionInclusion,
};
use layerx_proof::merkle::{leaf_hash, node_hash, Proof};
use layerx_proof::receipt::AuthorizedBatch;
use layerx_types::account::AccountId;
use layerx_types::amount::Amount;
use layerx_types::ids::AssetId;
use layerx_types::intent::EvmAddress;
use layerx_types::payload::{ActivityType, ModuleId, ModuleRegistration, ModuleRegistry};
use sha2::{Digest as _, Sha256};

const FUNDED: &str = "0xf39Fd6e51aad88F6F4ce6aB8827279cffFb92266";
const EMERGENCY: &str = "0x70997970C51812dc3A010C7d01b50e0d17dc79C8";
const TOKEN_CREATION: &str = include_str!("contracts/IntegrationToken.hex");
const REGISTRY_CREATION: &str = include_str!("contracts/AssetRegistry.hex");
const VAULT_CREATION: &str = include_str!("contracts/LayerXVault.hex");
const ASSET: [u8; 32] = [0x42; 32];
const AMOUNT: u128 = 25;
const CORE_NETWORK: u32 = 17;
const CHECKPOINT_ID_HEX: &str = "5cd43e8c1a6a0ba5594d75846fe40bd851909368fc0a7439657180c5fb8b9572";
const CREDIT_ACTIVITY_ID_HEX: &str =
    "3ad4f279bd6297c488fbf76c0802a3fb20c5060d955a7648e6417f8653f8fa11";
const CREDIT_RECEIPT_HEX: &str = "000152010001000000203ad4f279bd6297c488fbf76c0802a3fb20c5060d955a7648e6417f8653f8fa110000000000000009000000200707070707070707070707070707070707070707070707070707070707070707000000200808080808080808080808080808080808080808080808080808080808080808000000200808080808080808080808080808080808080808080808080808080808080808000000000000000000000000000000000000000000000001000000205cd43e8c1a6a0ba5594d75846fe40bd851909368fc0a7439657180c5fb8b957200080000000100000001010000002042424242424242424242424242424242424242424242424242424242424242420000000000000000000000000000001900000020f94d2cc01cae556915267bc3d1ad7c58034009ea25cbe56906be12b9ca876de0000000000000000000000000000000640000000000000000000000000000004b00000000000000010000002042de0bc2f3c75fd9995e3ad3d57efaf06530b93679d956ddf17fa9d325e1d60d0000000000000000000000000000000a00000000000000000000000000000023000000200909090909090909090909090909090909090909090909090909090909090909000000200a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a000000200b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b00000000000003e80100000040d8bdb7a072cdbc700f7390c482c40823192d2bfc983749456a20e08dc42f54526e153909c707f05d141c20ec9a728f6358fe1a460f6bf7ca7171758511b02e0d";
const CUSTODY_REFERENCE: [u8; 32] = [0x71; 32];

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

fn deposit_verifier(
    anvil: &Anvil,
    required_confirmations: u64,
    authority: &SigningKey,
) -> DepositProofVerifier {
    DepositProofVerifier::new(DepositProofConfig {
        endpoints: vec![anvil.endpoint.clone()],
        minimum_endpoint_agreement: 1,
        required_confirmations,
        paxeer_chain_id: 31_337,
        paxeer_checkpoint_authority: authority.verifying_key().to_bytes(),
        custody_reference: CUSTODY_REFERENCE,
        layerx_network_id: CORE_NETWORK,
        layerx_protocol_version: 1,
    })
    .unwrap_or_else(|error| panic!("deposit proof verifier: {error:?}"))
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

fn deposit_id_from_receipt(
    anvil: &Anvil,
    transaction: TransactionHash,
    vault: EvmAddress,
) -> [u8; 32] {
    let receipt = anvil.call(
        "eth_getTransactionReceipt",
        &[Json::Text(transaction.to_hex())],
    );
    let logs = match receipt.member("logs") {
        Some(Json::Array(logs)) => logs,
        _ => panic!("receipt logs are missing"),
    };
    for log in logs {
        let Some(address) = log.member("address").and_then(Json::as_text) else {
            continue;
        };
        if parse_address(address) != vault {
            continue;
        }
        let topics = match log.member("topics") {
            Some(Json::Array(topics)) => topics,
            _ => panic!("custody log topics are missing"),
        };
        let topic = topics
            .get(1)
            .and_then(Json::as_text)
            .and_then(|value| value.strip_prefix("0x"))
            .unwrap_or_else(|| panic!("custody deposit id is missing"));
        return hex_array(topic);
    }
    panic!("custody event is missing")
}

fn published_proof(
    anvil: &Anvil,
    transaction: TransactionHash,
    vault: EvmAddress,
    authority: &SigningKey,
) -> PublishedDepositProof {
    let deposit_id = deposit_id_from_receipt(anvil, transaction, vault);
    let checkpoint_id = hex_array(CHECKPOINT_ID_HEX);
    let leaf_bytes = deposit_leaf_bytes(
        deposit_id,
        CUSTODY_REFERENCE,
        AssetId::new(ASSET),
        Amount::from_u128(AMOUNT),
        checkpoint_id,
        CORE_NETWORK,
        1,
    )
    .unwrap_or_else(|error| panic!("deposit leaf: {error:?}"));
    let deposit_root = leaf_hash(&leaf_bytes)
        .unwrap_or_else(|error| panic!("deposit leaf hash: {error:?}"));
    let mut registration = DepositRootRegistration {
        checkpoint_id,
        checkpoint_state_root: [8; 32],
        deposit_root,
        custody_reference: CUSTODY_REFERENCE,
        network_id: CORE_NETWORK,
        protocol_version: 1,
        signature: [0; 64],
    };
    let message = deposit_root_registration_message(&registration)
        .unwrap_or_else(|error| panic!("deposit root message: {error:?}"));
    registration.signature = authority.sign(&message).to_bytes();
    PublishedDepositProof {
        registration,
        inclusion_proof: Proof::new(0, 1, Vec::new())
            .unwrap_or_else(|error| panic!("deposit proof path: {error:?}")),
    }
}

fn dummy_published_proof(authority: &SigningKey) -> PublishedDepositProof {
    let mut registration = DepositRootRegistration {
        checkpoint_id: hex_array(CHECKPOINT_ID_HEX),
        checkpoint_state_root: [8; 32],
        deposit_root: [9; 32],
        custody_reference: CUSTODY_REFERENCE,
        network_id: CORE_NETWORK,
        protocol_version: 1,
        signature: [0; 64],
    };
    let message = deposit_root_registration_message(&registration)
        .unwrap_or_else(|error| panic!("deposit root message: {error:?}"));
    registration.signature = authority.sign(&message).to_bytes();
    PublishedDepositProof {
        registration,
        inclusion_proof: Proof::new(0, 1, Vec::new())
            .unwrap_or_else(|error| panic!("deposit proof path: {error:?}")),
    }
}

fn bridge_registry() -> ModuleRegistry {
    let credit = ActivityType::new(ModuleId::Bridge, 1)
        .unwrap_or_else(|error| panic!("credit activity: {error:?}"));
    let registration = ModuleRegistration::new(ModuleId::Bridge, &[credit])
        .unwrap_or_else(|error| panic!("bridge registration: {error:?}"));
    ModuleRegistry::new(&[registration])
        .unwrap_or_else(|error| panic!("module registry: {error:?}"))
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
    signing: &SigningKey,
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
fn signed_custody_root_mints_an_opaque_proof_but_credit_ingress_fails_closed() {
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
    let authority = SigningKey::from_bytes(&[11; 32]);
    let published = published_proof(&anvil, deposit_transaction, vault, &authority);
    let signed_message = deposit_root_registration_message(&published.registration)
        .unwrap_or_else(|error| panic!("signed root message: {error:?}"));
    assert!(signed_message.starts_with(b"LX:PAXEER:DEPOSIT:ROOT:v1"));
    assert!(signed_message.ends_with(&[0, 0, 0, 17, 0, 1]));
    let proof_verifier = deposit_verifier(&anvil, 1, &authority);
    let proof = proof_verifier
        .obtain(&report, vault, published.clone())
        .unwrap_or_else(|error| panic!("deposit proof: {error:?}"));
    assert_eq!(proof.transaction(), deposit_transaction);
    assert_eq!(proof.custody_reference(), CUSTODY_REFERENCE);
    assert_eq!(proof.custody().asset, AssetId::new(ASSET));
    assert_eq!(proof.custody().amount, Amount::from_u128(AMOUNT));
    assert_eq!(proof.custody().beneficiary, account_address(&recipient));
    assert_eq!(proof.checkpoint_id().bytes(), hex_array(CHECKPOINT_ID_HEX));
    assert_eq!(proof.checkpoint_state_root(), [8; 32]);
    assert_eq!(proof.deposit_root(), proof.leaf_hash());
    assert_eq!(proof.inclusion_proof().leaf_index(), 0);
    assert_eq!(proof.inclusion_proof().leaf_count(), 1);
    assert_eq!(proof.network_id(), CORE_NETWORK);
    assert_eq!(proof.protocol_version(), 1);
    let mut nullifier = Sha256::new();
    nullifier.update(b"LX:DEPOSIT:NULLIFIER:v1");
    nullifier.update(proof.custody().deposit_id);
    assert_eq!(proof.nullifier(), <[u8; 32]>::from(nullifier.finalize()));
    assert_eq!(proof.idempotency_key().bytes(), proof.nullifier());

    let decoy = leaf_hash(b"another canonical deposit leaf")
        .unwrap_or_else(|error| panic!("decoy leaf: {error:?}"));
    let mut indexed = published.clone();
    indexed.inclusion_proof = Proof::new(1, 2, vec![decoy])
        .unwrap_or_else(|error| panic!("index-aware path: {error:?}"));
    indexed.registration.deposit_root = node_hash(&decoy, &proof.leaf_hash())
        .unwrap_or_else(|error| panic!("index-aware root: {error:?}"));
    let message = deposit_root_registration_message(&indexed.registration)
        .unwrap_or_else(|error| panic!("indexed root message: {error:?}"));
    indexed.registration.signature = authority.sign(&message).to_bytes();
    let indexed_proof = proof_verifier
        .obtain(&report, vault, indexed.clone())
        .unwrap_or_else(|error| panic!("index-aware deposit proof: {error:?}"));
    assert_eq!(indexed_proof.inclusion_proof().leaf_index(), 1);
    assert_eq!(indexed_proof.inclusion_proof().leaf_count(), 2);

    let mut wrong_index = indexed;
    wrong_index.inclusion_proof = Proof::new(0, 2, vec![decoy])
        .unwrap_or_else(|error| panic!("wrong index path: {error:?}"));
    assert!(matches!(
        proof_verifier.obtain(&report, vault, wrong_index),
        Err(DepositFailure::ProofUnavailable(
            ProofFault::DepositInclusion(_)
        ))
    ));

    let mut bad_signature = published.clone();
    bad_signature.registration.signature[0] ^= 1;
    assert!(matches!(
        proof_verifier.obtain(&report, vault, bad_signature),
        Err(DepositFailure::ProofUnavailable(
            ProofFault::InvalidDepositRootSignature
        ))
    ));

    let mut wrong_network = published.clone();
    wrong_network.registration.network_id = CORE_NETWORK.saturating_add(1);
    assert_eq!(
        proof_verifier.obtain(&report, vault, wrong_network),
        Err(DepositFailure::ProofUnavailable(
            ProofFault::RegistrationNetworkMismatch {
                expected: CORE_NETWORK,
                found: CORE_NETWORK.saturating_add(1),
            }
        ))
    );

    let mut wrong_custody_reference = published.clone();
    wrong_custody_reference.registration.custody_reference = [0x72; 32];
    assert_eq!(
        proof_verifier.obtain(&report, vault, wrong_custody_reference),
        Err(DepositFailure::ProofUnavailable(
            ProofFault::CustodyReferenceMismatch {
                expected: CUSTODY_REFERENCE,
                found: [0x72; 32],
            }
        ))
    );

    let mut wrong_root = published.clone();
    wrong_root.registration.deposit_root = [0x55; 32];
    let message = deposit_root_registration_message(&wrong_root.registration)
        .unwrap_or_else(|error| panic!("wrong root message: {error:?}"));
    wrong_root.registration.signature = authority.sign(&message).to_bytes();
    assert!(matches!(
        proof_verifier.obtain(&report, vault, wrong_root),
        Err(DepositFailure::ProofUnavailable(
            ProofFault::DepositInclusion(_)
        ))
    ));

    let registry = bridge_registry();
    assert!(matches!(
        CreditPath::prepare(&proof, &reserve, &recipient, &registry),
        Err(DepositFailure::CreditRefused(
            CreditFault::BridgeProofIngressUnavailable
        ))
    ));

    let signing = SigningKey::from_bytes(&[3; 32]);
    let activity_id = hex_array(CREDIT_ACTIVITY_ID_HEX);
    let fields = ReceiptFields {
        activity_id,
        previous_state_root: [7; 32],
        resulting_state_root: proof.checkpoint_state_root(),
        batch_id: proof.checkpoint_id().bytes(),
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
            activity_id,
            &reserve,
            &recipient,
        ),
        Err(DepositFailure::CreditRefused(
            CreditFault::BridgeProofIngressUnavailable
        ))
    ));
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
    let authority = SigningKey::from_bytes(&[11; 32]);
    let published = dummy_published_proof(&authority);
    let proof_verifier = deposit_verifier(&anvil, 1, &authority);
    assert!(matches!(
        proof_verifier.obtain(&report, vault, published.clone()),
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
    let deeper_verifier =
        deposit_verifier(&anvil, required.saturating_add(1), &authority);
    assert!(matches!(
        deeper_verifier.obtain(&unavailable, vault, published),
        Err(DepositFailure::ProofUnavailable(
            ProofFault::NotFinal { .. }
        ))
    ));
}
