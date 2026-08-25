use std::net::TcpListener;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicU16, Ordering};
use std::thread;
use std::time::Duration;

use k256::ecdsa::SigningKey;
use layerx_paxeer_client::{
    balance_leaf, raw_call, EmergencyExit, EndpointConfig, EndpointFault, EndpointTransport,
    ExecutionOutcome, ExitConfig, ExitEligibility, ExitError, ExitEvidence, ExitProgress,
    ExitRefusal, GuarantorAttestation, Json, PaxeerClient, TransactionHash, TransactionInclusion,
};
use layerx_types::intent::EvmAddress;
use sha2::{Digest as _, Sha256};

const GOVERNANCE: EvmAddress = EvmAddress::new([
    0xf3, 0x9f, 0xd6, 0xe5, 0x1a, 0xad, 0x88, 0xf6, 0xf4, 0xce, 0x6a, 0xb8, 0x82, 0x72, 0x79, 0xcf,
    0xff, 0xb9, 0x22, 0x66,
]);
const EMERGENCY_COUNCIL: EvmAddress = EvmAddress::new([
    0x70, 0x99, 0x79, 0x70, 0xc5, 0x18, 0x12, 0xdc, 0x3a, 0x01, 0x0c, 0x7d, 0x01, 0xb5, 0x0e, 0x0d,
    0x17, 0xdc, 0x79, 0xc8,
]);
const RECIPIENT: EvmAddress = EvmAddress::new([
    0x3c, 0x44, 0xcd, 0xdd, 0xb6, 0xa9, 0x00, 0xfa, 0x2b, 0x58, 0x5d, 0xd2, 0x99, 0xe0, 0x3d, 0x12,
    0xfa, 0x42, 0x93, 0xbc,
]);
const GOVERNANCE_KEY: [u8; 32] = [
    0xac, 0x09, 0x74, 0xbe, 0xc3, 0x9a, 0x17, 0xe3, 0x6b, 0xa4, 0xa6, 0xb4, 0xd2, 0x38, 0xff, 0x94,
    0x4b, 0xac, 0xb4, 0x78, 0xcb, 0xed, 0x5e, 0xfc, 0xae, 0x78, 0x4d, 0x7b, 0xf4, 0xf2, 0xff, 0x80,
];
const NETWORK_ID: u32 = 17;
const BALANCE: u128 = 300;
const CUSTODY: u128 = 500;
const ACCOUNT: [u8; 32] = [0x44; 32];
const ASSET: [u8; 32] = [0x33; 32];
const GENESIS: [u8; 32] = [0x22; 32];
const CONFIG_HASH: [u8; 32] = [0x11; 32];
const DATA_AVAILABILITY_ROOT: [u8; 32] = [0x66; 32];
const CHECKPOINT_DOMAIN: &[u8] = b"LXP/v1/checkpoint-certificate\x00";

const REGISTER_ASSET: [u8; 4] = [0xea, 0x24, 0x92, 0x88];
const MINT: [u8; 4] = [0x40, 0xc1, 0x0f, 0x19];
const APPROVE: [u8; 4] = [0x09, 0x5e, 0xa7, 0xb3];
const DEPOSIT: [u8; 4] = [0x8a, 0x9e, 0x53, 0x2c];
const SET_SETTLEMENT_MODULE: [u8; 4] = [0x0f, 0x2f, 0x5f, 0x64];
const SET_CONSUMER: [u8; 4] = [0x02, 0xc9, 0xef, 0x45];
const DEPOSIT_BOND: [u8; 4] = [0x5c, 0x37, 0x4a, 0x21];
const REGISTER_CHECKPOINT: [u8; 4] = [0xf3, 0x89, 0xd8, 0x3f];
const DECLARE_EMERGENCY: [u8; 4] = [0x50, 0xd1, 0x7f, 0xff];
const BALANCE_OF: [u8; 4] = [0x70, 0xa0, 0x82, 0x31];

static NEXT_PORT: AtomicU16 = AtomicU16::new(0);

struct Anvil {
    child: Child,
    endpoint: EndpointConfig,
    reader: PaxeerClient,
}

impl Anvil {
    fn launch() -> Self {
        for _ in 0..8 {
            let port = next_port();
            let endpoint = EndpointConfig {
                url: format!("http://127.0.0.1:{port}"),
                request_timeout: Duration::from_secs(10),
                transport: EndpointTransport::LocalEmulator,
                expected_chain_id: 31_337,
            };
            let child = Command::new(anvil_binary())
                .arg("--port")
                .arg(port.to_string())
                .arg("--silent")
                .arg("--code-size-limit")
                .arg("50000")
                .stdin(Stdio::null())
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .spawn()
                .unwrap_or_else(|error| panic!("spawn anvil: {error}"));
            let reader = PaxeerClient::new(vec![endpoint.clone()])
                .unwrap_or_else(|error| panic!("Paxeer client: {error:?}"));
            let mut anvil = Self {
                child,
                endpoint,
                reader,
            };
            if anvil.ready() {
                return anvil;
            }
            anvil.halt();
        }
        panic!("no free port for anvil")
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

    fn text_result(&self, method: &str, params: &[Json]) -> String {
        self.call(method, params)
            .as_text()
            .unwrap_or_else(|| panic!("{method}: expected text result"))
            .to_owned()
    }

    fn transact(
        &self,
        sender: EvmAddress,
        recipient: EvmAddress,
        calldata: &[u8],
        value: u128,
    ) -> TransactionInclusion {
        let transaction = self.send(sender, Some(recipient), calldata, value);
        let inclusion = self.wait_receipt(transaction);
        assert_eq!(inclusion.execution, ExecutionOutcome::Succeeded);
        inclusion
    }

    fn send(
        &self,
        sender: EvmAddress,
        recipient: Option<EvmAddress>,
        calldata: &[u8],
        value: u128,
    ) -> TransactionHash {
        let mut fields = vec![
            text_member("from", &address_hex(sender)),
            text_member("data", &bytes_hex(calldata)),
            text_member("gas", "0x17d7840"),
            text_member("value", &format!("0x{value:x}")),
        ];
        if let Some(to) = recipient {
            fields.push(text_member("to", &address_hex(to)));
        }
        let hash = self.text_result("eth_sendTransaction", &[Json::Object(fields)]);
        TransactionHash::from_hex(&hash)
            .unwrap_or_else(|error| panic!("transaction hash: {error:?}"))
    }

    fn wait_receipt(&self, transaction: TransactionHash) -> TransactionInclusion {
        for _ in 0..300 {
            let receipt = self
                .reader
                .transaction_receipt(transaction)
                .unwrap_or_else(|error| panic!("receipt: {error:?}"));
            if let Some(inclusion) = receipt {
                return inclusion;
            }
            thread::sleep(Duration::from_millis(20));
        }
        panic!("transaction was not included in time")
    }

    fn deploy(&self, name: &str, arguments: &[[u8; 32]]) -> EvmAddress {
        let mut creation = forge_bytecode(name);
        for argument in arguments {
            creation.extend_from_slice(argument);
        }
        let transaction = self.send(GOVERNANCE, None, &creation, 0);
        let inclusion = self.wait_receipt(transaction);
        assert_eq!(inclusion.execution, ExecutionOutcome::Succeeded);
        inclusion
            .deployed_contract
            .unwrap_or_else(|| panic!("{name}: no deployed contract in receipt"))
    }

    fn mine(&self) {
        self.call("evm_mine", &[]);
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

struct Topology {
    token: EvmAddress,
    registry: EvmAddress,
    vault: EvmAddress,
    bond: EvmAddress,
    checkpoint: EvmAddress,
    nullifiers: EvmAddress,
    exit: EvmAddress,
}

impl Topology {
    fn deploy(anvil: &Anvil) -> Self {
        let token = anvil.deploy("IntegrationToken", &[address_word(GOVERNANCE)]);
        let registry = anvil.deploy(
            "AssetRegistry",
            &[
                address_word(GOVERNANCE),
                address_word(EMERGENCY_COUNCIL),
                CONFIG_HASH,
                quantity_word(1),
            ],
        );
        let vault = anvil.deploy(
            "LayerXVault",
            &[
                address_word(registry),
                address_word(GOVERNANCE),
                address_word(EMERGENCY_COUNCIL),
                CONFIG_HASH,
                quantity_word(1),
            ],
        );
        let bond = anvil.deploy(
            "GuarantorBond",
            &[
                address_word(GOVERNANCE),
                quantity_word(100),
                quantity_word(0),
                quantity_word(86_400),
                CONFIG_HASH,
                quantity_word(1),
            ],
        );
        let checkpoint = anvil.deploy(
            "CheckpointRegistry",
            &[
                address_word(bond),
                quantity_word(1),
                quantity_word(u128::from(NETWORK_ID)),
                quantity_word(1),
                quantity_word(1),
                quantity_word(3_600),
                quantity_word(3_600),
                GENESIS,
                CONFIG_HASH,
                quantity_word(1),
            ],
        );
        let challenge = anvil.deploy(
            "CheckpointChallengeManager",
            &[
                address_word(checkpoint),
                address_word(bond),
                address_word(GOVERNANCE),
                address_word(EMERGENCY_COUNCIL),
                quantity_word(3_600),
                quantity_word(1),
                CONFIG_HASH,
                quantity_word(1),
            ],
        );
        let nullifiers = anvil.deploy(
            "WithdrawalNullifierRegistry",
            &[
                address_word(GOVERNANCE),
                address_word(EMERGENCY_COUNCIL),
                CONFIG_HASH,
                quantity_word(1),
            ],
        );
        let exit = anvil.deploy(
            "EmergencyExit",
            &[
                address_word(checkpoint),
                address_word(challenge),
                address_word(nullifiers),
                address_word(vault),
                address_word(GOVERNANCE),
                address_word(EMERGENCY_COUNCIL),
                quantity_word(3_600),
                CONFIG_HASH,
                quantity_word(1),
            ],
        );
        Self {
            token,
            registry,
            vault,
            bond,
            checkpoint,
            nullifiers,
            exit,
        }
    }

    fn prepare(&self, anvil: &Anvil) -> ExitEvidence {
        self.prepare_custody(anvil);
        anvil.transact(
            GOVERNANCE,
            self.bond,
            &static_call(DEPOSIT_BOND, &[quantity_word(1), quantity_word(1)]),
            1,
        );
        let state_root = balance_leaf(&ACCOUNT, &ASSET, BALANCE, RECIPIENT);
        let (digest, attestation) = signed_checkpoint(state_root);
        anvil.transact(
            GOVERNANCE,
            self.checkpoint,
            &register_checkpoint_calldata(state_root, digest, &attestation),
            0,
        );
        ExitEvidence {
            account: ACCOUNT,
            asset_id: ASSET,
            finalised_balance: BALANCE,
            recipient: RECIPIENT,
            leaf_index: 0,
            siblings: Vec::new(),
            attestations: vec![attestation],
        }
    }

    fn prepare_custody(&self, anvil: &Anvil) {
        anvil.transact(
            GOVERNANCE,
            self.registry,
            &static_call(
                REGISTER_ASSET,
                &[
                    ASSET,
                    address_word(self.token),
                    quantity_word(6),
                    quantity_word(1),
                    quantity_word(1_000),
                ],
            ),
            0,
        );
        anvil.transact(
            GOVERNANCE,
            self.token,
            &static_call(MINT, &[address_word(GOVERNANCE), quantity_word(CUSTODY)]),
            0,
        );
        anvil.transact(
            GOVERNANCE,
            self.token,
            &static_call(APPROVE, &[address_word(self.vault), quantity_word(CUSTODY)]),
            0,
        );
        anvil.transact(
            GOVERNANCE,
            self.vault,
            &static_call(DEPOSIT, &[ASSET, quantity_word(CUSTODY), [0x77; 32]]),
            0,
        );
        anvil.transact(
            GOVERNANCE,
            self.vault,
            &static_call(
                SET_SETTLEMENT_MODULE,
                &[address_word(self.exit), quantity_word(1)],
            ),
            0,
        );
        anvil.transact(
            GOVERNANCE,
            self.nullifiers,
            &static_call(SET_CONSUMER, &[address_word(self.exit), quantity_word(1)]),
            0,
        );
    }
}

#[test]
fn exits_against_published_paxeer_evidence_while_core_reads_are_unavailable() {
    prove_core_endpoint_is_unavailable();
    let anvil = Anvil::launch();
    let topology = Topology::deploy(&anvil);
    let evidence = topology.prepare(&anvil);
    let exit = exit_client(&anvil, topology.exit);

    let normal = exit
        .eligibility()
        .unwrap_or_else(|error| panic!("normal eligibility: {error:?}"));
    assert!(matches!(
        normal,
        ExitEligibility::NetworkOperatingNormally { .. }
    ));
    assert_eq!(
        exit.construct_claim(&evidence),
        Err(ExitError::Refused(ExitRefusal::NotEligible {
            eligibility: normal,
        }))
    );

    anvil.transact(
        EMERGENCY_COUNCIL,
        topology.exit,
        &static_call(DECLARE_EMERGENCY, &[]),
        0,
    );
    assert!(matches!(
        exit.eligibility(),
        Ok(ExitEligibility::Eligible { .. })
    ));
    let mut unproven = evidence.clone();
    unproven.finalised_balance = unproven.finalised_balance.saturating_add(1);
    assert!(matches!(
        exit.construct_claim(&unproven),
        Err(ExitError::Refused(ExitRefusal::BalanceNotProven { .. }))
    ));
    let mut uncertified = evidence.clone();
    uncertified.attestations.clear();
    assert!(matches!(
        exit.construct_claim(&uncertified),
        Err(ExitError::Refused(
            ExitRefusal::CertificateNotRecorded { .. }
        ))
    ));
    let claim = exit
        .construct_claim(&evidence)
        .unwrap_or_else(|error| panic!("verified exit claim: {error:?}"));
    let transaction = anvil.send(GOVERNANCE, Some(claim.contract), &claim.calldata, 0);
    assert!(matches!(
        two_confirmation_progress(&anvil, &exit, transaction, ExecutionOutcome::Succeeded),
        ExitProgress::Settled {
            confirmations: 2,
            ..
        }
    ));
    assert_eq!(token_balance(&anvil, topology.token, RECIPIENT), BALANCE);

    assert!(matches!(
        exit.construct_claim(&evidence),
        Err(ExitError::Refused(ExitRefusal::AlreadyExited { .. }))
    ));
    let replay = anvil.send(GOVERNANCE, Some(claim.contract), &claim.calldata, 0);
    assert!(matches!(
        two_confirmation_progress(&anvil, &exit, replay, ExecutionOutcome::Reverted),
        ExitProgress::Refused {
            confirmations: 2,
            ..
        }
    ));
}

fn two_confirmation_progress(
    anvil: &Anvil,
    exit: &EmergencyExit,
    transaction: TransactionHash,
    execution: ExecutionOutcome,
) -> ExitProgress {
    assert_eq!(anvil.wait_receipt(transaction).execution, execution);
    let mut tracker = exit
        .track(transaction)
        .unwrap_or_else(|error| panic!("exit tracker: {error:?}"));
    assert_eq!(
        ExitProgress::of(&tracker.poll()),
        ExitProgress::Confirming {
            execution,
            confirmations: 1,
            required: 2,
        }
    );
    anvil.mine();
    ExitProgress::of(&tracker.poll())
}

fn prove_core_endpoint_is_unavailable() {
    let listener = TcpListener::bind("127.0.0.1:0")
        .unwrap_or_else(|error| panic!("reserve unavailable core port: {error}"));
    let port = listener
        .local_addr()
        .unwrap_or_else(|error| panic!("unavailable core address: {error}"))
        .port();
    drop(listener);
    let endpoint = EndpointConfig {
        url: format!("http://127.0.0.1:{port}"),
        request_timeout: Duration::from_millis(100),
        transport: EndpointTransport::LocalEmulator,
        expected_chain_id: 31_337,
    };
    let failure = match raw_call(&endpoint, "layerx_getFinalisedCheckpoint", &[]) {
        Err(failure) => failure,
        Ok(value) => panic!("ordinary LayerX core unexpectedly answered: {value:?}"),
    };
    assert!(
        matches!(failure.fault, EndpointFault::Connect { .. }),
        "unavailable core endpoint must fail to connect, got {:?}",
        failure.fault
    );
}

fn exit_client(anvil: &Anvil, contract: EvmAddress) -> EmergencyExit {
    EmergencyExit::new(ExitConfig {
        endpoints: vec![anvil.endpoint.clone()],
        minimum_endpoint_agreement: 1,
        exit_contract: contract,
        required_confirmations: 2,
        poll_cadence: Duration::from_millis(20),
        delayed_after_polls: 50,
    })
    .unwrap_or_else(|error| panic!("emergency exit client: {error:?}"))
}

fn signed_checkpoint(state_root: [u8; 32]) -> ([u8; 32], GuarantorAttestation) {
    let digest = checkpoint_digest(state_root);
    let mut attestation = GuarantorAttestation {
        checkpoint_id: digest,
        checkpoint_hash: digest,
        guarantor_id: quantity_word(1),
        batch_number: 1,
        data_availability_root: DATA_AVAILABILITY_ROOT,
        replayed: true,
        data_available: true,
        availability_class_mask: 0x1f,
        attested_at: 2,
        signer: GOVERNANCE,
        signature_r: [0; 32],
        signature_s: [0; 32],
        signature_v: 0,
    };
    let signing_key = SigningKey::from_bytes((&GOVERNANCE_KEY).into())
        .unwrap_or_else(|error| panic!("checkpoint signing key: {error}"));
    let message = attestation_digest(&attestation);
    let (signature, recovery_id) = signing_key
        .sign_prehash_recoverable(&message)
        .unwrap_or_else(|error| panic!("checkpoint signature: {error}"));
    attestation
        .signature_r
        .copy_from_slice(&signature.r().to_bytes());
    attestation
        .signature_s
        .copy_from_slice(&signature.s().to_bytes());
    attestation.signature_v = recovery_id.to_byte().saturating_add(27);
    (digest, attestation)
}

fn checkpoint_digest(state_root: [u8; 32]) -> [u8; 32] {
    let mut header = vec![0x00, 0x01, 0x17, 0x01, 0x0f];
    header.extend_from_slice(&[1]);
    header.extend_from_slice(&1_u16.to_be_bytes());
    header.extend_from_slice(&[2]);
    header.extend_from_slice(&NETWORK_ID.to_be_bytes());
    for (field, value) in [(3_u8, 1_u64), (4, 1), (5, 1), (6, 100)] {
        header.push(field);
        header.extend_from_slice(&value.to_be_bytes());
    }
    for (field, value) in [
        (7_u8, GENESIS),
        (8, state_root),
        (9, [0x55; 32]),
        (10, [0x56; 32]),
        (11, [0x57; 32]),
        (12, DATA_AVAILABILITY_ROOT),
        (13, [0x58; 32]),
    ] {
        header.push(field);
        header.extend_from_slice(&32_u32.to_be_bytes());
        header.extend_from_slice(&value);
    }
    header.push(14);
    header.extend_from_slice(&1_u64.to_be_bytes());
    header.push(15);
    header.extend_from_slice(&32_u32.to_be_bytes());
    header.extend_from_slice(&[0x59; 32]);
    assert_eq!(header.len(), 354);
    sha256(&[CHECKPOINT_DOMAIN, &header, &0_u32.to_be_bytes()])
}

fn attestation_digest(attestation: &GuarantorAttestation) -> [u8; 32] {
    sha256(&[
        CHECKPOINT_DOMAIN,
        &attestation.checkpoint_id,
        &attestation.checkpoint_hash,
        &attestation.guarantor_id,
        &attestation.batch_number.to_be_bytes(),
        &attestation.data_availability_root,
        &[u8::from(attestation.replayed)],
        &[u8::from(attestation.data_available)],
        &[attestation.availability_class_mask],
        &attestation.attested_at.to_be_bytes(),
    ])
}

fn register_checkpoint_calldata(
    state_root: [u8; 32],
    digest: [u8; 32],
    attestation: &GuarantorAttestation,
) -> Vec<u8> {
    let mut words = vec![
        quantity_word(1),
        quantity_word(u128::from(NETWORK_ID)),
        quantity_word(1),
        quantity_word(1),
        quantity_word(1),
        quantity_word(100),
        GENESIS,
        state_root,
        [0x55; 32],
        [0x56; 32],
        [0x57; 32],
        DATA_AVAILABILITY_ROOT,
        [0x58; 32],
        quantity_word(1),
        [0x59; 32],
        quantity_word(17 * 32),
        quantity_word(18 * 32),
        quantity_word(0),
        quantity_word(1),
    ];
    words.extend_from_slice(&attestation_words(attestation));
    assert_eq!(digest, attestation.checkpoint_id);
    static_call(REGISTER_CHECKPOINT, &words)
}

fn attestation_words(attestation: &GuarantorAttestation) -> [[u8; 32]; 13] {
    [
        attestation.checkpoint_id,
        attestation.checkpoint_hash,
        attestation.guarantor_id,
        quantity_word(u128::from(attestation.batch_number)),
        attestation.data_availability_root,
        quantity_word(u128::from(attestation.replayed)),
        quantity_word(u128::from(attestation.data_available)),
        quantity_word(u128::from(attestation.availability_class_mask)),
        quantity_word(u128::from(attestation.attested_at)),
        address_word(attestation.signer),
        attestation.signature_r,
        attestation.signature_s,
        quantity_word(u128::from(attestation.signature_v)),
    ]
}

fn token_balance(anvil: &Anvil, token: EvmAddress, account: EvmAddress) -> u128 {
    let bytes = anvil
        .reader
        .call_contract(token, &static_call(BALANCE_OF, &[address_word(account)]))
        .unwrap_or_else(|error| panic!("token balance: {error:?}"));
    assert_eq!(bytes.len(), 32, "token balance: expected one ABI word");
    bytes
        .iter()
        .skip(16)
        .fold(0_u128, |value, byte| (value << 8) | u128::from(*byte))
}

fn sha256(parts: &[&[u8]]) -> [u8; 32] {
    let mut hasher = Sha256::new();
    for part in parts {
        hasher.update(part);
    }
    hasher.finalize().into()
}

fn forge_bytecode(name: &str) -> Vec<u8> {
    let output = Command::new(forge_binary())
        .arg("inspect")
        .arg(name)
        .arg("bytecode")
        .current_dir(repository_root())
        .stderr(Stdio::inherit())
        .output()
        .unwrap_or_else(|error| panic!("forge inspect {name}: {error}"));
    assert!(output.status.success(), "forge inspect {name} failed");
    let text = std::str::from_utf8(&output.stdout)
        .unwrap_or_else(|error| panic!("forge inspect {name} utf-8: {error}"));
    decode_hex(text.trim()).unwrap_or_else(|error| panic!("forge inspect {name}: {error}"))
}

fn repository_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../..")
        .canonicalize()
        .unwrap_or_else(|error| panic!("repository root: {error}"))
}

fn forge_binary() -> PathBuf {
    let foundry = PathBuf::from("/root/.foundry/bin/forge");
    if foundry.exists() {
        foundry
    } else {
        PathBuf::from("forge")
    }
}

fn anvil_binary() -> PathBuf {
    let foundry = PathBuf::from("/root/.foundry/bin/anvil");
    if foundry.exists() {
        foundry
    } else {
        PathBuf::from("anvil")
    }
}

fn next_port() -> u16 {
    let offset = NEXT_PORT.fetch_add(1, Ordering::Relaxed);
    let pid_lane = u16::try_from(std::process::id() % 9_000).unwrap_or(0);
    20_000_u16
        .saturating_add(pid_lane)
        .saturating_add(offset.saturating_mul(7))
}

fn text_member(name: &str, value: &str) -> (String, Json) {
    (name.to_owned(), Json::Text(value.to_owned()))
}

fn static_call(selector: [u8; 4], words: &[[u8; 32]]) -> Vec<u8> {
    let mut data = Vec::with_capacity(4 + words.len().saturating_mul(32));
    data.extend_from_slice(&selector);
    for word in words {
        data.extend_from_slice(word);
    }
    data
}

fn quantity_word(value: u128) -> [u8; 32] {
    let mut word = [0_u8; 32];
    word[16..].copy_from_slice(&value.to_be_bytes());
    word
}

fn address_word(address: EvmAddress) -> [u8; 32] {
    let mut word = [0_u8; 32];
    word[12..].copy_from_slice(&address.bytes());
    word
}

fn address_hex(address: EvmAddress) -> String {
    bytes_hex(&address.bytes())
}

fn bytes_hex(bytes: &[u8]) -> String {
    let mut text = String::from("0x");
    for byte in bytes {
        text.push(hex_char(byte >> 4));
        text.push(hex_char(byte & 0x0f));
    }
    text
}

fn decode_hex(text: &str) -> Result<Vec<u8>, &'static str> {
    let digits = text.strip_prefix("0x").ok_or("missing 0x prefix")?;
    if digits.len() % 2 != 0 {
        return Err("odd hex length");
    }
    let mut bytes = Vec::with_capacity(digits.len() / 2);
    for pair in digits.as_bytes().chunks_exact(2) {
        let high = hex_nibble(pair[0]).ok_or("non-hex digit")?;
        let low = hex_nibble(pair[1]).ok_or("non-hex digit")?;
        bytes.push((high << 4) | low);
    }
    Ok(bytes)
}

const fn hex_nibble(byte: u8) -> Option<u8> {
    match byte {
        b'0'..=b'9' => Some(byte - b'0'),
        b'a'..=b'f' => Some(byte - b'a' + 10),
        b'A'..=b'F' => Some(byte - b'A' + 10),
        _ => None,
    }
}

fn hex_char(nibble: u8) -> char {
    char::from(b"0123456789abcdef"[usize::from(nibble & 0x0f)])
}
