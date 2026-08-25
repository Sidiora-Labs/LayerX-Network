use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicU16, Ordering};
use std::sync::{Mutex, OnceLock};
use std::thread;
use std::time::Duration;

use ed25519_dalek::SigningKey;
use layerx_paxeer_client::{
    raw_call, CancelledFundsDisposition, ChallengeKind, CheckpointProof, ClaimProgress,
    CommittedWithdrawalDebit, DebitExpectation, EndpointConfig, EndpointTransport,
    ExecutionOutcome, FinalityReport, FinalityStage, Json, PaxeerFundsDisposition, PayoutEvidence,
    ProtocolDebitDisposition, SubmittedWithdrawalClaim, TransactionHash, TransactionInclusion,
    WithdrawalAttestation, WithdrawalBoundary, WithdrawalConfig, WithdrawalError,
};
use layerx_proof::receipt::AuthorizedBatch;
use layerx_types::intent::EvmAddress;
use sha2::{Digest as _, Sha256};

const FUNDED: &str = "0xf39Fd6e51aad88F6F4ce6aB8827279cffFb92266";
const FUNDED_PRIVATE_KEY: &str =
    "0xac0974bec39a17e36ba4a6b4d238ff944bacb478cbed5efcae784d7bf4f2ff80";
const CHALLENGER: &str = "0x70997970C51812dc3A010C7d01b50e0d17dc79C8";
const RECIPIENT: &str = "0x3C44CdDdB6a900fa2b585dd299e03d12FA4293BC";

const NETWORK_ID: u32 = 17;
const ASSET: [u8; 32] = [0x42; 32];
const AMOUNT: u128 = 25;
const VAULT_BALANCE: u128 = 100;
const GENESIS: [u8; 32] = [0x10; 32];
const GUARANTOR_ID: [u8; 32] = quantity_const(1);
const WITHDRAW_RECEIPT_HEX: &str = "0x0001520100010000002031313131313131313131313131313131313131313131313131313131313131310000000000000001000000204141414141414141414141414141414141414141414141414141414141414141000000204242424242424242424242424242424242424242424242424242424242424242000000204444444444444444444444444444444444444444444444444444444444444444000000000000000000000000000000000000000000000001000000204343434343434343434343434343434343434343434343434343434343434343000800000002000000010100000020424242424242424242424242424242424242424242424242424242424242424200000000000000000000000000000019000000203333333333333333333333333333333333333333333333333333333333333333000000000000000000000000000000640000000000000000000000000000004b0000000000000001000000203434343434343434343434343434343434343434343434343434343434343434000000000000000000000000000000000000000000000000000000000000001900000020454545454545454545454545454545454545454545454545454545454545454500000020464646464646464646464646464646464646464646464646464646464646464600000020474747474747474747474747474747474747474747474747474747474747474700000000000003e80100000040a3c5df8259d413eaddaab76e7c1efd2a5ffc8a7beefc7b28ffca71e44d830c79e9763f0c8f4dfec8ecbf7e6ce61a8a5de40b1d6c0ff19c313da85982df118f06";

const REGISTER_ASSET: [u8; 4] = [0xea, 0x24, 0x92, 0x88];
const SET_SETTLEMENT_MODULE: [u8; 4] = [0x0f, 0x2f, 0x5f, 0x64];
const SET_CONSUMER: [u8; 4] = [0x02, 0xc9, 0xef, 0x45];
const SET_SLASHING_AUTHORITY: [u8; 4] = [0xef, 0x45, 0x5c, 0x4c];
const MINT: [u8; 4] = [0x40, 0xc1, 0x0f, 0x19];
const APPROVE: [u8; 4] = [0x09, 0x5e, 0xa7, 0xb3];
const DEPOSIT: [u8; 4] = [0x8a, 0x9e, 0x53, 0x2c];
const ACTIVATE_GUARANTOR: [u8; 4] = [0x23, 0x7d, 0xd0, 0x4f];
const DEPOSIT_BOND: [u8; 4] = [0xf5, 0x14, 0x8c, 0x24];
const REGISTER_CHECKPOINT: [u8; 4] = [0xc7, 0x2a, 0x88, 0x43];
const RAISE_CHALLENGE: [u8; 4] = [0x0c, 0xfc, 0xd9, 0x2c];
const RESOLVE_CHALLENGE: [u8; 4] = [0x7d, 0x89, 0x12, 0x2d];
const BALANCE_OF: [u8; 4] = [0x70, 0xa0, 0x82, 0x31];

static NEXT_PORT: AtomicU16 = AtomicU16::new(0);
static BYTECODE: OnceLock<Mutex<BTreeMap<&'static str, String>>> = OnceLock::new();

struct Anvil {
    child: Child,
    endpoint: EndpointConfig,
}

impl Anvil {
    fn launch() -> Self {
        for _ in 0..8 {
            let offset = NEXT_PORT.fetch_add(1, Ordering::Relaxed);
            let lane = u16::try_from(std::process::id() % 7_000).unwrap_or(0);
            let port = 24_000_u16
                .saturating_add(lane)
                .saturating_add(offset.saturating_mul(11));
            let endpoint = EndpointConfig {
                url: format!("http://127.0.0.1:{port}"),
                request_timeout: Duration::from_secs(10),
                transport: EndpointTransport::LocalEmulator,
                expected_chain_id: 31_337,
            };
            let child = Command::new(anvil_binary())
                .arg("--port")
                .arg(port.to_string())
                .arg("--chain-id")
                .arg("31337")
                .arg("--gas-limit")
                .arg("100000000")
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
        for _ in 0..200 {
            if raw_call(&self.endpoint, "eth_blockNumber", &[]).is_ok() {
                return true;
            }
            thread::sleep(Duration::from_millis(25));
        }
        false
    }

    fn call(&self, method: &str, params: &[Json]) -> Json {
        raw_call(&self.endpoint, method, params)
            .unwrap_or_else(|failure| panic!("{method}: {failure:?}"))
    }

    fn send(
        &self,
        from: &str,
        to: Option<EvmAddress>,
        data: &[u8],
        value: u128,
    ) -> TransactionHash {
        let mut fields = vec![
            text_member("from", from),
            text_member("data", &bytes_hex(data)),
            text_member("gas", "0x3938700"),
        ];
        if let Some(address) = to {
            fields.push(text_member("to", &address_hex(address)));
        }
        if value != 0 {
            fields.push(text_member("value", &format!("0x{value:x}")));
        }
        let result = self.call("eth_sendTransaction", &[Json::Object(fields)]);
        let hash = result
            .as_text()
            .unwrap_or_else(|| panic!("eth_sendTransaction: expected hash"));
        TransactionHash::from_hex(hash)
            .unwrap_or_else(|error| panic!("transaction hash: {error:?}"))
    }

    fn deploy(&self, contract: &'static str, arguments: &[[u8; 32]]) -> EvmAddress {
        let mut creation = hex_bytes(&contract_bytecode(contract));
        for argument in arguments {
            creation.extend_from_slice(argument);
        }
        let transaction = self.send(FUNDED, None, &creation, 0);
        let receipt = wait_receipt(self, transaction);
        assert_eq!(receipt.execution, ExecutionOutcome::Succeeded);
        receipt
            .deployed_contract
            .unwrap_or_else(|| panic!("{contract}: no deployed address"))
    }

    fn send_checked(
        &self,
        from: &str,
        to: EvmAddress,
        data: &[u8],
        value: u128,
    ) -> TransactionHash {
        let transaction = self.send(from, Some(to), data, value);
        let receipt = wait_receipt(self, transaction);
        assert_eq!(receipt.execution, ExecutionOutcome::Succeeded);
        transaction
    }

    fn mine(&self) {
        self.call("evm_mine", &[]);
    }

    fn advance(&self, seconds: u64) {
        self.call("evm_increaseTime", &[Json::Number(seconds.to_string())]);
        self.mine();
    }

    fn latest_timestamp(&self) -> u64 {
        let block = self.call(
            "eth_getBlockByNumber",
            &[Json::Text("latest".to_owned()), Json::Bool(false)],
        );
        let text = block
            .member("timestamp")
            .and_then(Json::as_text)
            .unwrap_or_else(|| panic!("latest timestamp absent"));
        u64::from_str_radix(text.trim_start_matches("0x"), 16)
            .unwrap_or_else(|error| panic!("latest timestamp: {error}"))
    }

    fn token_balance(&self, token: EvmAddress, owner: EvmAddress) -> u128 {
        let result = self.call(
            "eth_call",
            &[
                Json::Object(vec![
                    text_member("to", &address_hex(token)),
                    text_member(
                        "data",
                        &bytes_hex(&call_data(BALANCE_OF, &[address_word(owner)])),
                    ),
                ]),
                Json::Text("latest".to_owned()),
            ],
        );
        let bytes = hex_bytes(
            result
                .as_text()
                .unwrap_or_else(|| panic!("balanceOf result is not hex")),
        );
        let word: [u8; 32] = bytes
            .try_into()
            .unwrap_or_else(|bytes: Vec<u8>| panic!("balance word length {}", bytes.len()));
        word_u128(word)
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

struct Fixture {
    anvil: Anvil,
    boundary: WithdrawalBoundary,
    challenge_manager: EvmAddress,
    token: EvmAddress,
    vault: EvmAddress,
    recipient: EvmAddress,
    checkpoint_hash: [u8; 32],
    submitted: SubmittedWithdrawalClaim,
}

#[derive(Clone)]
struct Header {
    protocol_version: u16,
    network_id: u32,
    epoch: u64,
    batch_number: u64,
    first_sequence: u64,
    last_sequence: u64,
    previous_state_root: [u8; 32],
    resulting_state_root: [u8; 32],
    activity_merkle_root: [u8; 32],
    receipt_merkle_root: [u8; 32],
    event_merkle_root: [u8; 32],
    data_availability_root: [u8; 32],
    oracle_root: [u8; 32],
    timestamp: u64,
    sequencer_id: [u8; 32],
}

fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .ancestors()
        .nth(3)
        .unwrap_or_else(|| panic!("repository root absent"))
        .to_path_buf()
}

fn foundry_binary(name: &str) -> PathBuf {
    let binary = PathBuf::from(format!("/root/.foundry/bin/{name}"));
    if binary.exists() {
        binary
    } else {
        PathBuf::from(name)
    }
}

fn anvil_binary() -> PathBuf {
    foundry_binary("anvil")
}

fn contract_bytecode(contract: &'static str) -> String {
    let cache = BYTECODE.get_or_init(|| Mutex::new(BTreeMap::new()));
    let mut cache = cache
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    if let Some(bytecode) = cache.get(contract) {
        return bytecode.clone();
    }
    let output = Command::new(foundry_binary("forge"))
        .arg("inspect")
        .arg(contract)
        .arg("bytecode")
        .current_dir(repo_root())
        .output()
        .unwrap_or_else(|error| panic!("forge inspect {contract}: {error}"));
    assert!(
        output.status.success(),
        "forge inspect {contract}: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let bytecode = String::from_utf8(output.stdout)
        .unwrap_or_else(|error| panic!("forge bytecode utf8: {error}"))
        .trim()
        .to_owned();
    assert!(bytecode.starts_with("0x"));
    cache.insert(contract, bytecode.clone());
    bytecode
}

fn text_member(name: &str, value: &str) -> (String, Json) {
    (name.to_owned(), Json::Text(value.to_owned()))
}

fn parse_address(text: &str) -> EvmAddress {
    let bytes = hex_bytes(text);
    EvmAddress::new(
        bytes
            .try_into()
            .unwrap_or_else(|bytes: Vec<u8>| panic!("address length {}", bytes.len())),
    )
}

fn address_hex(address: EvmAddress) -> String {
    bytes_hex(&address.bytes())
}

fn hex_bytes(text: &str) -> Vec<u8> {
    let digits = text
        .trim()
        .strip_prefix("0x")
        .unwrap_or_else(|| panic!("hex prefix absent"));
    assert_eq!(digits.len() % 2, 0);
    digits
        .as_bytes()
        .chunks_exact(2)
        .map(|pair| (hex_nibble(pair[0]) << 4) | hex_nibble(pair[1]))
        .collect()
}

fn hex_nibble(byte: u8) -> u8 {
    match byte {
        b'0'..=b'9' => byte - b'0',
        b'a'..=b'f' => byte - b'a' + 10,
        b'A'..=b'F' => byte - b'A' + 10,
        _ => panic!("non-hex digit"),
    }
}

fn bytes_hex(bytes: &[u8]) -> String {
    const DIGITS: &[u8; 16] = b"0123456789abcdef";
    let mut text = String::from("0x");
    for byte in bytes {
        text.push(char::from(DIGITS[usize::from(byte >> 4)]));
        text.push(char::from(DIGITS[usize::from(byte & 0x0f)]));
    }
    text
}

const fn quantity_const(value: u8) -> [u8; 32] {
    let mut word = [0_u8; 32];
    word[31] = value;
    word
}

fn quantity_word(bytes: &[u8]) -> [u8; 32] {
    let mut word = [0_u8; 32];
    word[32_usize.saturating_sub(bytes.len())..].copy_from_slice(bytes);
    word
}

fn usize_word(value: usize) -> [u8; 32] {
    quantity_word(&value.to_be_bytes())
}

fn address_word(address: EvmAddress) -> [u8; 32] {
    let mut word = [0_u8; 32];
    word[12..].copy_from_slice(&address.bytes());
    word
}

fn bool_word(value: bool) -> [u8; 32] {
    quantity_word(&[u8::from(value)])
}

fn call_data(selector: [u8; 4], words: &[[u8; 32]]) -> Vec<u8> {
    let mut data = Vec::with_capacity(4_usize.saturating_add(words.len().saturating_mul(32)));
    data.extend_from_slice(&selector);
    for word in words {
        data.extend_from_slice(word);
    }
    data
}

fn word_u128(word: [u8; 32]) -> u128 {
    assert_eq!(word[..16], [0; 16]);
    let mut bytes = [0_u8; 16];
    bytes.copy_from_slice(&word[16..]);
    u128::from_be_bytes(bytes)
}

fn wait_receipt(anvil: &Anvil, transaction: TransactionHash) -> TransactionInclusion {
    let client = layerx_paxeer_client::PaxeerClient::new(vec![anvil.endpoint.clone()])
        .unwrap_or_else(|error| panic!("client: {error:?}"));
    for _ in 0..300 {
        if let Some(receipt) = client
            .transaction_receipt(transaction)
            .unwrap_or_else(|error| panic!("receipt: {error:?}"))
        {
            return receipt;
        }
        thread::sleep(Duration::from_millis(20));
    }
    panic!("transaction was not included");
}

fn final_report(
    anvil: &Anvil,
    boundary: &WithdrawalBoundary,
    transaction: TransactionHash,
) -> FinalityReport {
    let mut tracker = boundary
        .track(transaction)
        .unwrap_or_else(|error| panic!("tracker: {error:?}"));
    for _ in 0..300 {
        let report = tracker.poll();
        if matches!(report.stage(), FinalityStage::Final { .. }) {
            return report;
        }
        if matches!(report.stage(), FinalityStage::Confirming { .. }) {
            anvil.mine();
        }
        thread::sleep(Duration::from_millis(20));
    }
    panic!("transaction did not reach finality");
}

#[allow(clippy::too_many_lines)]
fn deploy_suite(
    anvil: &Anvil,
) -> (
    EvmAddress,
    EvmAddress,
    EvmAddress,
    EvmAddress,
    EvmAddress,
    EvmAddress,
) {
    let owner = parse_address(FUNDED);
    let challenger = parse_address(CHALLENGER);
    let token = anvil.deploy("IntegrationToken", &[address_word(owner)]);
    let asset_registry = anvil.deploy(
        "AssetRegistry",
        &[
            address_word(owner),
            address_word(challenger),
            [0x21; 32],
            quantity_word(&1_u128.to_be_bytes()),
        ],
    );
    let vault = anvil.deploy(
        "LayerXVault",
        &[
            address_word(asset_registry),
            address_word(owner),
            address_word(challenger),
            [0x22; 32],
            quantity_word(&1_u128.to_be_bytes()),
        ],
    );
    let bond = anvil.deploy(
        "GuarantorBond",
        &[
            address_word(owner),
            address_word(owner),
            quantity_word(&1_u16.to_be_bytes()),
            quantity_word(&NETWORK_ID.to_be_bytes()),
            quantity_word(&100_u32.to_be_bytes()),
            [0; 32],
            quantity_word(&86_400_u64.to_be_bytes()),
            [0x23; 32],
            quantity_word(&1_u128.to_be_bytes()),
        ],
    );
    let checkpoint_registry = anvil.deploy(
        "CheckpointRegistry",
        &[
            address_word(bond),
            quantity_word(&1_u16.to_be_bytes()),
            quantity_word(&NETWORK_ID.to_be_bytes()),
            quantity_word(&1_u16.to_be_bytes()),
            quantity_word(&1_u16.to_be_bytes()),
            quantity_word(&3_600_u64.to_be_bytes()),
            quantity_word(&300_u64.to_be_bytes()),
            GENESIS,
            [0x24; 32],
            quantity_word(&1_u128.to_be_bytes()),
        ],
    );
    let challenge_manager = anvil.deploy(
        "CheckpointChallengeManager",
        &[
            address_word(checkpoint_registry),
            address_word(bond),
            address_word(owner),
            address_word(challenger),
            quantity_word(&3_600_u64.to_be_bytes()),
            quantity_word(&1_u128.to_be_bytes()),
            [0x25; 32],
            quantity_word(&1_u128.to_be_bytes()),
        ],
    );
    let nullifier_registry = anvil.deploy(
        "WithdrawalNullifierRegistry",
        &[
            address_word(owner),
            address_word(challenger),
            [0x26; 32],
            quantity_word(&1_u128.to_be_bytes()),
        ],
    );
    let claims = anvil.deploy(
        "WithdrawalClaims",
        &[
            address_word(checkpoint_registry),
            address_word(challenge_manager),
            address_word(nullifier_registry),
            address_word(vault),
            [0x27; 32],
            quantity_word(&1_u128.to_be_bytes()),
        ],
    );

    for (target, data) in [
        (
            asset_registry,
            call_data(
                REGISTER_ASSET,
                &[
                    ASSET,
                    address_word(token),
                    quantity_word(&6_u8.to_be_bytes()),
                    quantity_word(&1_u128.to_be_bytes()),
                    quantity_word(&1_000_u128.to_be_bytes()),
                ],
            ),
        ),
        (
            vault,
            call_data(
                SET_SETTLEMENT_MODULE,
                &[address_word(claims), bool_word(true)],
            ),
        ),
        (
            nullifier_registry,
            call_data(SET_CONSUMER, &[address_word(claims), bool_word(true)]),
        ),
        (
            bond,
            call_data(SET_SLASHING_AUTHORITY, &[address_word(challenge_manager)]),
        ),
        (
            token,
            call_data(
                MINT,
                &[
                    address_word(owner),
                    quantity_word(&VAULT_BALANCE.to_be_bytes()),
                ],
            ),
        ),
        (
            token,
            call_data(
                APPROVE,
                &[
                    address_word(vault),
                    quantity_word(&VAULT_BALANCE.to_be_bytes()),
                ],
            ),
        ),
        (
            vault,
            call_data(
                DEPOSIT,
                &[
                    ASSET,
                    quantity_word(&VAULT_BALANCE.to_be_bytes()),
                    [0x28; 32],
                ],
            ),
        ),
    ] {
        anvil.send_checked(FUNDED, target, &data, 0);
    }
    anvil.send_checked(
        FUNDED,
        bond,
        &call_data(
            ACTIVATE_GUARANTOR,
            &[
                GUARANTOR_ID,
                address_word(owner),
                address_word(owner),
                quantity_word(&1_u64.to_be_bytes()),
                quantity_word(&1_u64.to_be_bytes()),
            ],
        ),
        0,
    );
    anvil.send_checked(
        FUNDED,
        bond,
        &call_data(DEPOSIT_BOND, &[GUARANTOR_ID]),
        1,
    );
    (
        token,
        vault,
        bond,
        checkpoint_registry,
        challenge_manager,
        claims,
    )
}

fn debit_expectation(recipient: EvmAddress) -> DebitExpectation {
    DebitExpectation {
        activity_id: [0x31; 32],
        network_id: NETWORK_ID,
        withdrawal_id: [0x32; 32],
        account: [0x33; 32],
        withdrawals_account: [0x34; 32],
        asset_id: ASSET,
        amount: AMOUNT,
        recipient,
    }
}

fn committed_debit(expectation: DebitExpectation) -> CommittedWithdrawalDebit {
    assert_eq!(expectation.activity_id, [0x31; 32]);
    assert_eq!(expectation.asset_id, ASSET);
    assert_eq!(expectation.amount, AMOUNT);
    assert_eq!(expectation.account, [0x33; 32]);
    assert_eq!(expectation.withdrawals_account, [0x34; 32]);
    let signer = SigningKey::from_bytes(&[0x51; 32]);
    let batch = AuthorizedBatch::new(
        [0x43; 32],
        ASSET,
        [0x41; 32],
        [0x42; 32],
        signer.verifying_key().to_bytes(),
    );
    CommittedWithdrawalDebit::verify(&hex_bytes(WITHDRAW_RECEIPT_HEX), &batch, expectation)
        .unwrap_or_else(|error| panic!("committed debit: {error:?}"))
}

fn withdrawal_leaf(expectation: DebitExpectation) -> [u8; 32] {
    let mut hasher = Sha256::new();
    hasher.update(b"LXP/v1/merkle-leaf\0");
    hasher.update(expectation.withdrawal_id);
    hasher.update(expectation.account);
    hasher.update(expectation.asset_id);
    hasher.update(expectation.amount.to_be_bytes());
    hasher.update(address_word(expectation.recipient));
    hasher.finalize().into()
}

fn checkpoint_header(state_root: [u8; 32], timestamp: u64) -> Header {
    Header {
        protocol_version: 1,
        network_id: NETWORK_ID,
        epoch: 1,
        batch_number: 1,
        first_sequence: 1,
        last_sequence: 1,
        previous_state_root: GENESIS,
        resulting_state_root: state_root,
        activity_merkle_root: [0x61; 32],
        receipt_merkle_root: [0x62; 32],
        event_merkle_root: [0x63; 32],
        data_availability_root: [0x64; 32],
        oracle_root: [0x65; 32],
        timestamp,
        sequencer_id: [0x66; 32],
    }
}

fn encoded_header(header: &Header) -> Vec<u8> {
    let mut bytes = vec![0x00, 0x01, 0x17, 0x01, 0x0f];
    bytes.push(1);
    bytes.extend_from_slice(&header.protocol_version.to_be_bytes());
    bytes.push(2);
    bytes.extend_from_slice(&header.network_id.to_be_bytes());
    for (tag, value) in [
        (3, header.epoch),
        (4, header.batch_number),
        (5, header.first_sequence),
        (6, header.last_sequence),
    ] {
        bytes.push(tag);
        bytes.extend_from_slice(&value.to_be_bytes());
    }
    for (tag, value) in [
        (7, header.previous_state_root),
        (8, header.resulting_state_root),
        (9, header.activity_merkle_root),
        (10, header.receipt_merkle_root),
        (11, header.event_merkle_root),
        (12, header.data_availability_root),
        (13, header.oracle_root),
    ] {
        bytes.push(tag);
        bytes.extend_from_slice(&32_u32.to_be_bytes());
        bytes.extend_from_slice(&value);
    }
    bytes.push(14);
    bytes.extend_from_slice(&header.timestamp.to_be_bytes());
    bytes.push(15);
    bytes.extend_from_slice(&32_u32.to_be_bytes());
    bytes.extend_from_slice(&header.sequencer_id);
    assert_eq!(bytes.len(), 354);
    bytes
}

fn checkpoint_hash(header: &Header) -> [u8; 32] {
    let mut hasher = Sha256::new();
    hasher.update(b"LXP/v1/checkpoint-certificate\0");
    hasher.update(encoded_header(header));
    hasher.update(0_u32.to_be_bytes());
    hasher.finalize().into()
}

fn signed_attestation(
    header: &Header,
    checkpoint: [u8; 32],
    settlement_contract: EvmAddress,
) -> WithdrawalAttestation {
    let attested_at = header
        .timestamp
        .checked_add(1)
        .unwrap_or_else(|| panic!("checkpoint timestamp cannot advance attestation milliseconds"));
    let mut message = Vec::with_capacity(189);
    message.extend_from_slice(&header.protocol_version.to_be_bytes());
    message.extend_from_slice(&header.network_id.to_be_bytes());
    message.extend_from_slice(&31_337_u64.to_be_bytes());
    message.extend_from_slice(&settlement_contract.bytes());
    message.extend_from_slice(&header.epoch.to_be_bytes());
    message.extend_from_slice(&checkpoint);
    message.extend_from_slice(&checkpoint);
    message.extend_from_slice(&GUARANTOR_ID);
    message.extend_from_slice(&header.batch_number.to_be_bytes());
    message.extend_from_slice(&header.data_availability_root);
    message.extend_from_slice(&[1, 1, 0x1f]);
    message.extend_from_slice(&attested_at.to_be_bytes());
    assert_eq!(message.len(), 189);
    let mut hasher = Sha256::new();
    hasher.update(b"LXP/v1/guarantor-attestation\0");
    hasher.update(message);
    let digest: [u8; 32] = hasher.finalize().into();
    let signature = sign_digest(digest);
    let mut r = [0_u8; 32];
    let mut s = [0_u8; 32];
    r.copy_from_slice(&signature[..32]);
    s.copy_from_slice(&signature[32..64]);
    WithdrawalAttestation {
        protocol_version: header.protocol_version,
        network_id: header.network_id,
        paxeer_chain_id: 31_337,
        settlement_contract,
        epoch: header.epoch,
        checkpoint_id: checkpoint,
        checkpoint_hash: checkpoint,
        guarantor_id: GUARANTOR_ID,
        batch_number: header.batch_number,
        data_availability_root: header.data_availability_root,
        replayed: true,
        data_available: true,
        availability_class_mask: 0x1f,
        attested_at,
        signer: parse_address(FUNDED),
        signature_r: r,
        signature_s: s,
        signature_v: signature[64],
    }
}

fn sign_digest(digest: [u8; 32]) -> Vec<u8> {
    let output = Command::new(foundry_binary("cast"))
        .args([
            "wallet",
            "sign",
            "--no-hash",
            "--private-key",
            FUNDED_PRIVATE_KEY,
            &bytes_hex(&digest),
        ])
        .output()
        .unwrap_or_else(|error| panic!("cast wallet sign: {error}"));
    assert!(
        output.status.success(),
        "cast wallet sign: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let signature = hex_bytes(
        String::from_utf8(output.stdout)
            .unwrap_or_else(|error| panic!("signature utf8: {error}"))
            .trim(),
    );
    assert_eq!(signature.len(), 65);
    assert!(matches!(signature[64], 27 | 28));
    signature
}

fn header_words(header: &Header) -> [[u8; 32]; 15] {
    [
        quantity_word(&header.protocol_version.to_be_bytes()),
        quantity_word(&header.network_id.to_be_bytes()),
        quantity_word(&header.epoch.to_be_bytes()),
        quantity_word(&header.batch_number.to_be_bytes()),
        quantity_word(&header.first_sequence.to_be_bytes()),
        quantity_word(&header.last_sequence.to_be_bytes()),
        header.previous_state_root,
        header.resulting_state_root,
        header.activity_merkle_root,
        header.receipt_merkle_root,
        header.event_merkle_root,
        header.data_availability_root,
        header.oracle_root,
        quantity_word(&header.timestamp.to_be_bytes()),
        header.sequencer_id,
    ]
}

fn attestation_words(attestation: &WithdrawalAttestation) -> [[u8; 32]; 18] {
    [
        quantity_word(&attestation.protocol_version.to_be_bytes()),
        quantity_word(&attestation.network_id.to_be_bytes()),
        quantity_word(&attestation.paxeer_chain_id.to_be_bytes()),
        address_word(attestation.settlement_contract),
        quantity_word(&attestation.epoch.to_be_bytes()),
        attestation.checkpoint_id,
        attestation.checkpoint_hash,
        attestation.guarantor_id,
        quantity_word(&attestation.batch_number.to_be_bytes()),
        attestation.data_availability_root,
        bool_word(attestation.replayed),
        bool_word(attestation.data_available),
        quantity_word(&[attestation.availability_class_mask]),
        quantity_word(&attestation.attested_at.to_be_bytes()),
        address_word(attestation.signer),
        attestation.signature_r,
        attestation.signature_s,
        quantity_word(&[attestation.signature_v]),
    ]
}

fn register_checkpoint_calldata(header: &Header, attestation: &WithdrawalAttestation) -> Vec<u8> {
    let mut words = Vec::new();
    words.extend_from_slice(&header_words(header));
    words.push(usize_word(17 * 32));
    words.push(usize_word(18 * 32));
    words.push([0; 32]);
    words.push(usize_word(1));
    words.extend_from_slice(&attestation_words(attestation));
    call_data(REGISTER_CHECKPOINT, &words)
}

fn fixture() -> Fixture {
    let anvil = Anvil::launch();
    let (token, vault, bond, checkpoint_registry, challenge_manager, claims) =
        deploy_suite(&anvil);
    let recipient = parse_address(RECIPIENT);
    let expectation = debit_expectation(recipient);
    let debit = committed_debit(expectation);
    let leaf = withdrawal_leaf(expectation);
    let timestamp_ms = anvil
        .latest_timestamp()
        .checked_mul(1_000)
        .unwrap_or_else(|| panic!("latest block timestamp exceeds canonical milliseconds"));
    let header = checkpoint_header(leaf, timestamp_ms);
    let checkpoint_hash = checkpoint_hash(&header);
    let attestation = signed_attestation(&header, checkpoint_hash, bond);
    anvil.send_checked(
        FUNDED,
        checkpoint_registry,
        &register_checkpoint_calldata(&header, &attestation),
        0,
    );
    let boundary = WithdrawalBoundary::new(WithdrawalConfig {
        endpoints: vec![anvil.endpoint.clone()],
        minimum_endpoint_agreement: 1,
        claims_contract: claims,
        required_confirmations: 2,
        poll_cadence: Duration::from_millis(20),
        delayed_after_polls: 100,
    })
    .unwrap_or_else(|error| panic!("withdrawal boundary: {error:?}"));
    let proof = CheckpointProof {
        checkpoint_hash,
        state_root: leaf,
        epoch: header.epoch,
        batch_number: header.batch_number,
        data_availability_root: header.data_availability_root,
        leaf_index: 0,
        siblings: Vec::new(),
        attestations: vec![attestation],
    };
    let mut wrong = proof.clone();
    wrong.state_root[0] ^= 0xff;
    assert!(matches!(
        boundary.construct_claim(debit.clone(), wrong),
        Err(WithdrawalError::Refused(
            layerx_paxeer_client::ClaimRefusal::RootMismatch { .. }
        ))
    ));
    let claim = boundary
        .construct_claim(debit, proof)
        .unwrap_or_else(|error| panic!("construct claim: {error:?}"));
    assert_eq!(claim.leaf(), leaf);
    assert_ne!(claim.nullifier(), [0; 32]);
    let transaction = anvil.send(FUNDED, Some(claims), claim.calldata(), 0);
    let report = final_report(&anvil, &boundary, transaction);
    let submitted = boundary
        .accept_submission(claim, &report)
        .unwrap_or_else(|error| panic!("accept submission: {error:?}"));
    assert_eq!(submitted.claim().proof().checkpoint_hash, checkpoint_hash);
    Fixture {
        anvil,
        boundary,
        challenge_manager,
        token,
        vault,
        recipient,
        checkpoint_hash,
        submitted,
    }
}

#[test]
fn real_claim_waits_then_verifies_the_actual_vault_and_token_payout() {
    let fixture = fixture();
    assert_eq!(
        fixture
            .anvil
            .token_balance(fixture.token, fixture.recipient),
        0
    );
    let progress = fixture
        .boundary
        .progress(&fixture.submitted)
        .unwrap_or_else(|error| panic!("initial progress: {error:?}"));
    assert!(matches!(
        progress,
        ClaimProgress::WaitingForChallengeWindow { remaining, .. } if !remaining.is_zero()
    ));

    let premature = fixture.anvil.send(
        FUNDED,
        Some(fixture.boundary.claims_contract()),
        &fixture.submitted.finalise_calldata(),
        0,
    );
    let premature_report = final_report(&fixture.anvil, &fixture.boundary, premature);
    assert!(matches!(
        fixture
            .boundary
            .verify_payout(&fixture.submitted, &premature_report),
        Err(WithdrawalError::Reverted { .. })
    ));
    assert_eq!(
        fixture
            .anvil
            .token_balance(fixture.token, fixture.recipient),
        0
    );

    fixture.anvil.advance(3_601);
    assert!(matches!(
        fixture
            .boundary
            .progress(&fixture.submitted)
            .unwrap_or_else(|error| panic!("ready progress: {error:?}")),
        ClaimProgress::ReadyToFinalise { .. }
    ));
    let payout_transaction = fixture.anvil.send(
        FUNDED,
        Some(fixture.boundary.claims_contract()),
        &fixture.submitted.finalise_calldata(),
        0,
    );
    let report = final_report(&fixture.anvil, &fixture.boundary, payout_transaction);
    assert_eq!(
        fixture
            .boundary
            .progress(&fixture.submitted)
            .unwrap_or_else(|error| panic!("paid contract state: {error:?}")),
        ClaimProgress::PaidAwaitingPayoutVerification
    );
    let payout = fixture
        .boundary
        .verify_payout(&fixture.submitted, &report)
        .unwrap_or_else(|error| panic!("payout evidence: {error:?}"));
    assert_eq!(
        payout,
        PayoutEvidence {
            debit_receipt_reference: fixture.submitted.claim().debit().receipt_reference(),
            checkpoint_hash: fixture.checkpoint_hash,
            claim_id: fixture.submitted.claim_id(),
            payout_transaction,
            payout_inclusion: match report.stage() {
                FinalityStage::Final { inclusion, .. } => inclusion,
                stage => panic!("expected final payout, got {stage:?}"),
            },
            vault: fixture.vault,
            token: fixture.token,
            asset_id: ASSET,
            recipient: fixture.recipient,
            amount: AMOUNT,
        }
    );
    assert_eq!(
        fixture
            .anvil
            .token_balance(fixture.token, fixture.recipient),
        AMOUNT
    );
    assert_eq!(
        fixture.anvil.token_balance(fixture.token, fixture.vault),
        VAULT_BALANCE - AMOUNT
    );
}

#[test]
fn real_pending_challenge_reports_timing_and_upheld_challenge_cancels_without_payout() {
    let fixture = fixture();
    let evidence_hash = [0x91; 32];
    fixture.anvil.send_checked(
        CHALLENGER,
        fixture.challenge_manager,
        &call_data(
            RAISE_CHALLENGE,
            &[
                fixture.checkpoint_hash,
                quantity_word(&1_u8.to_be_bytes()),
                evidence_hash,
            ],
        ),
        1,
    );
    let progress = fixture
        .boundary
        .progress(&fixture.submitted)
        .unwrap_or_else(|error| panic!("challenge hold: {error:?}"));
    let ClaimProgress::ChallengeHeld(hold) = progress else {
        panic!("expected challenge hold, got {progress:?}");
    };
    assert_eq!(hold.kind, ChallengeKind::DataAvailability);
    assert_eq!(hold.evidence_hash, evidence_hash);
    assert_eq!(hold.window_closes_at, fixture.submitted.available_at());
    assert!(hold.raised_at <= hold.observed_at);
    assert!(hold.resolution_has_no_on_chain_deadline);

    fixture.anvil.send_checked(
        FUNDED,
        fixture.challenge_manager,
        &call_data(
            RESOLVE_CHALLENGE,
            &[fixture.checkpoint_hash, bool_word(true)],
        ),
        0,
    );
    let expected_disposition = CancelledFundsDisposition {
        paxeer: PaxeerFundsDisposition::RetainedInVault {
            vault: fixture.vault,
            asset_id: ASSET,
            amount: AMOUNT,
        },
        layerx: ProtocolDebitDisposition::RemainsCommittedPendingProtocolRecovery {
            debit_receipt_reference: fixture.submitted.claim().debit().receipt_reference(),
        },
    };
    assert_eq!(
        fixture
            .boundary
            .progress(&fixture.submitted)
            .unwrap_or_else(|error| panic!("upheld progress: {error:?}")),
        ClaimProgress::ChallengeUpheldAwaitingCancellation {
            disposition: expected_disposition,
        }
    );

    let cancellation_transaction = fixture.anvil.send(
        FUNDED,
        Some(fixture.boundary.claims_contract()),
        &fixture.submitted.cancellation_calldata(),
        0,
    );
    let report = final_report(&fixture.anvil, &fixture.boundary, cancellation_transaction);
    let cancellation = fixture
        .boundary
        .verify_cancellation(&fixture.submitted, &report)
        .unwrap_or_else(|error| panic!("cancellation evidence: {error:?}"));
    assert_eq!(cancellation.disposition, expected_disposition);
    assert_eq!(
        cancellation.cancellation_transaction,
        cancellation_transaction
    );
    assert_eq!(
        fixture
            .boundary
            .progress(&fixture.submitted)
            .unwrap_or_else(|error| panic!("cancelled progress: {error:?}")),
        ClaimProgress::Cancelled {
            disposition: expected_disposition,
        }
    );
    assert_eq!(
        fixture
            .anvil
            .token_balance(fixture.token, fixture.recipient),
        0
    );
    assert_eq!(
        fixture.anvil.token_balance(fixture.token, fixture.vault),
        VAULT_BALANCE
    );
}
