//! The forced-exit path of the native custody precompile.
//!
//! Every test drives the real [`EmergencyExit`] over real HTTP against an
//! in-process JSON-RPC server answering the custody precompile (`0x…1013`) and
//! the anchor precompile (`0x…1014`). The exit evidence is a real
//! [`StateWitness`] built with `layerx_proof::state_witness`, whose root is the
//! finalized state root the anchor reports, and a real ed25519 signature by the
//! account authority over [`exit_recipient_message`]; nothing is fabricated.

use std::collections::HashMap;
use std::io::{Read as _, Write as _};
use std::net::{TcpListener, TcpStream};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;

use ed25519_dalek::{Signer as _, SigningKey};
use layerx_paxeer_client::custody::{
    exit_claim_id, exit_eligible_calldata, exit_recipient_message, exit_withdrawal_id,
    get_claim_calldata, nullifier_status_calldata, withdrawal_nullifier, CustodyClaim,
    EMERGENCY_EXIT_EXECUTED_TOPIC,
};
use layerx_paxeer_client::state_proof::{AccountPath, StateWitness};
use layerx_paxeer_client::{
    parse_json, verify_exit_balance, EmergencyExit, EndpointConfig, EndpointTransport, ExitClaim,
    ExitConfig, ExitEligibility, ExitError, ExitEvidence, ExitProgress, ExitRefusal,
    ForcedExitMaterial, Json, LogRecord, TransactionHash, ANCHOR_PRECOMPILE, CUSTODY_PRECOMPILE,
};
use layerx_types::intent::EvmAddress;
use sha2::{Digest as _, Sha256};

type TestResult = Result<(), Box<dyn std::error::Error>>;

const CHAIN_ID: u64 = 31_337;
const NETWORK_ID: u32 = 7_332;
const REQUIRED_CONFIRMATIONS: u64 = 2;
const LATEST_BATCH: u64 = 118;
const EXIT_BLOCK: u64 = 61;
const BALANCE: u128 = 5_000_000;
const ACCOUNT_NAME: &[u8] = b"agent:exit-holder:main";
const ASSET: [u8; 32] = [0x24; 32];
const RECIPIENT: EvmAddress = EvmAddress::new([0x42; 20]);
const AUTHORITY_SEED: [u8; 32] = [0x61; 32];
const SELECTOR_LATEST_FINALIZED: [u8; 4] = [0x6c, 0xdd, 0x45, 0xae];
const SELECTOR_FINALIZED_STATE_ROOT: [u8; 4] = [0x0f, 0x60, 0x7f, 0xe4];

// ---------------------------------------------------------------------------
// In-process JSON-RPC server
// ---------------------------------------------------------------------------

#[derive(Clone, Debug, Default)]
struct Chain {
    head: u64,
    calls: HashMap<(String, String), String>,
    receipts: HashMap<String, Json>,
    transactions: HashMap<String, Json>,
}

impl Chain {
    fn view(&mut self, target: EvmAddress, calldata: &[u8], answer: &[u8]) {
        self.calls
            .insert((hex(&target.bytes()), hex(calldata)), hex(answer));
    }

    fn answer(&self, method: &str, params: &[Json]) -> Json {
        match method {
            "eth_chainId" => text(&format!("0x{CHAIN_ID:x}")),
            "eth_blockNumber" => text(&format!("0x{:x}", self.head)),
            "eth_getBlockByNumber" => match params.first().and_then(Json::as_text) {
                Some("latest") => block(self.head),
                Some(tag) => match quantity(tag) {
                    Some(number) if number <= self.head => block(number),
                    _ => Json::Null,
                },
                None => Json::Null,
            },
            "eth_getTransactionReceipt" => params
                .first()
                .and_then(Json::as_text)
                .and_then(|hash| self.receipts.get(hash))
                .cloned()
                .unwrap_or(Json::Null),
            "eth_getTransactionByHash" => params
                .first()
                .and_then(Json::as_text)
                .and_then(|hash| self.transactions.get(hash))
                .cloned()
                .unwrap_or(Json::Null),
            "eth_call" => {
                let request = params.first();
                let to = request
                    .and_then(|value| value.member("to"))
                    .and_then(Json::as_text)
                    .unwrap_or_default()
                    .to_owned();
                let data = request
                    .and_then(|value| value.member("data"))
                    .and_then(Json::as_text)
                    .unwrap_or_default()
                    .to_owned();
                self.calls
                    .get(&(to, data))
                    .map_or_else(|| text("0x"), |value| text(value))
            }
            _ => Json::Null,
        }
    }
}

struct Node {
    endpoint: EndpointConfig,
    chain: Arc<Mutex<Chain>>,
}

impl Node {
    fn launch(chain: Chain) -> Result<Self, Box<dyn std::error::Error>> {
        let shared = Arc::new(Mutex::new(chain));
        let listener = TcpListener::bind("127.0.0.1:0")?;
        let address = listener.local_addr()?;
        let served = Arc::clone(&shared);
        thread::spawn(move || {
            for stream in listener.incoming() {
                let Ok(mut stream) = stream else { continue };
                let Some(body) = read_request(&mut stream) else {
                    continue;
                };
                let Ok(request) = parse_json(&body) else {
                    continue;
                };
                let method = request
                    .member("method")
                    .and_then(Json::as_text)
                    .unwrap_or_default()
                    .to_owned();
                let params = match request.member("params") {
                    Some(Json::Array(items)) => items.clone(),
                    _ => Vec::new(),
                };
                let result = match served.lock() {
                    Ok(chain) => chain.answer(&method, &params),
                    Err(_) => Json::Null,
                };
                let payload = Json::Object(vec![
                    member("jsonrpc", text("2.0")),
                    member("id", Json::Number("1".to_owned())),
                    member("result", result),
                ])
                .render();
                let response = format!(
                    "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{payload}",
                    payload.len()
                );
                let _ = stream.write_all(response.as_bytes());
                let _ = stream.flush();
            }
        });
        Ok(Self {
            endpoint: endpoint(&format!("http://{address}")),
            chain: shared,
        })
    }

    fn exit(&self) -> Result<EmergencyExit, Box<dyn std::error::Error>> {
        EmergencyExit::new(configuration(self.endpoint.clone()))
            .map_err(|error| format!("{error:?}").into())
    }

    fn edit(&self, change: impl FnOnce(&mut Chain)) -> TestResult {
        let mut chain = self.chain.lock().map_err(|_| "chain lock poisoned")?;
        change(&mut chain);
        Ok(())
    }
}

fn endpoint(url: &str) -> EndpointConfig {
    EndpointConfig {
        url: url.to_owned(),
        request_timeout: Duration::from_secs(5),
        transport: EndpointTransport::LocalEmulator,
        expected_chain_id: CHAIN_ID,
    }
}

fn configuration(endpoint: EndpointConfig) -> ExitConfig {
    ExitConfig {
        endpoints: vec![endpoint],
        minimum_endpoint_agreement: 1,
        network_id: NETWORK_ID,
        required_confirmations: REQUIRED_CONFIRMATIONS,
        poll_cadence: Duration::from_millis(10),
        delayed_after_polls: 8,
    }
}

fn read_request(stream: &mut TcpStream) -> Option<String> {
    let mut buffer = Vec::new();
    let mut chunk = [0_u8; 4_096];
    loop {
        let read = stream.read(&mut chunk).ok()?;
        if read == 0 {
            return None;
        }
        buffer.extend_from_slice(chunk.get(..read)?);
        let Some(position) = buffer
            .windows(4)
            .position(|window| window == b"\r\n\r\n")
            .map(|position| position.saturating_add(4))
        else {
            continue;
        };
        let headers = String::from_utf8_lossy(buffer.get(..position)?).to_ascii_lowercase();
        let length: usize = headers
            .split("content-length:")
            .nth(1)
            .and_then(|rest| rest.split("\r\n").next())
            .and_then(|value| value.trim().parse().ok())?;
        let end = position.checked_add(length)?;
        if buffer.len() >= end {
            return String::from_utf8(buffer.get(position..end)?.to_vec()).ok();
        }
    }
}

// ---------------------------------------------------------------------------
// Encoding helpers
// ---------------------------------------------------------------------------

fn hex(bytes: &[u8]) -> String {
    let mut text = String::from("0x");
    for byte in bytes {
        text.push_str(&format!("{byte:02x}"));
    }
    text
}

fn quantity(value: &str) -> Option<u64> {
    u64::from_str_radix(value.strip_prefix("0x")?, 16).ok()
}

fn text(value: &str) -> Json {
    Json::Text(value.to_owned())
}

fn member(name: &str, value: Json) -> (String, Json) {
    (name.to_owned(), value)
}

fn block_hash(number: u64) -> [u8; 32] {
    let mut hasher = Sha256::new();
    hasher.update(b"paxeer-test-block\0");
    hasher.update(number.to_be_bytes());
    hasher.finalize().into()
}

fn block(number: u64) -> Json {
    Json::Object(vec![
        member("number", text(&format!("0x{number:x}"))),
        member("hash", text(&hex(&block_hash(number)))),
        member("timestamp", text("0x1")),
    ])
}

fn number_word(value: &[u8]) -> [u8; 32] {
    let mut word = [0_u8; 32];
    let start = 32_usize.saturating_sub(value.len());
    word[start..].copy_from_slice(value);
    word
}

fn address_word(address: EvmAddress) -> [u8; 32] {
    let mut word = [0_u8; 32];
    word[12..].copy_from_slice(&address.bytes());
    word
}

fn boolean_word(value: bool) -> [u8; 32] {
    number_word(&[u8::from(value)])
}

fn two_words(value: [u8; 32], present: bool) -> Vec<u8> {
    let mut out = value.to_vec();
    out.extend_from_slice(&boolean_word(present));
    out
}

fn anchor_call(selector: [u8; 4], batch_number: u64) -> Vec<u8> {
    let mut data = selector.to_vec();
    data.extend_from_slice(&number_word(&batch_number.to_be_bytes()));
    data
}

fn tuple(head: &[[u8; 32]], text_index: usize, value: &str) -> Vec<u8> {
    let mut out = number_word(&32_u64.to_be_bytes()).to_vec();
    let offset = head.len().saturating_mul(32);
    for (index, word) in head.iter().enumerate() {
        if index == text_index {
            out.extend_from_slice(&number_word(&offset.to_be_bytes()));
        } else {
            out.extend_from_slice(word);
        }
    }
    out.extend_from_slice(&number_word(&value.len().to_be_bytes()));
    let mut data = value.as_bytes().to_vec();
    while data.len() % 32 != 0 {
        data.push(0);
    }
    out.extend_from_slice(&data);
    out
}

fn encode_claim(claim: &CustodyClaim) -> Vec<u8> {
    tuple(
        &[
            claim.claim_id,
            number_word(&[claim.kind]),
            number_word(&[claim.status]),
            claim.nullifier,
            claim.withdrawal_id,
            claim.account,
            claim.asset_id,
            [0; 32],
            address_word(claim.recipient),
            number_word(&claim.amount.to_be_bytes()),
            number_word(&claim.batch_number.to_be_bytes()),
            claim.anchor,
            number_word(&claim.available_at.to_be_bytes()),
        ],
        7,
        &claim.denom,
    )
}

fn receipt_json(transaction: &str, number: u64, status: u64, logs: Vec<Json>) -> Json {
    Json::Object(vec![
        member("transactionHash", text(transaction)),
        member("blockNumber", text(&format!("0x{number:x}"))),
        member("blockHash", text(&hex(&block_hash(number)))),
        member("transactionIndex", text("0x0")),
        member("status", text(&format!("0x{status:x}"))),
        member("contractAddress", Json::Null),
        member("logs", Json::Array(logs)),
    ])
}

fn transaction_json(transaction: &str, input: &[u8]) -> Json {
    Json::Object(vec![
        member("hash", text(transaction)),
        member("to", text(&hex(&CUSTODY_PRECOMPILE.bytes()))),
        member("input", text(&hex(input))),
        member("value", text("0x0")),
    ])
}

fn hash(value: &str) -> Result<TransactionHash, Box<dyn std::error::Error>> {
    TransactionHash::from_hex(value).map_err(|error| format!("{error:?}").into())
}

// ---------------------------------------------------------------------------
// A real finalized account witness
// ---------------------------------------------------------------------------

fn state_leaf(key: &[u8], value: &[u8]) -> Result<[u8; 32], Box<dyn std::error::Error>> {
    let mut hasher = Sha256::new();
    hasher.update(b"LXP/v1/state-leaf\0");
    hasher.update(u32::try_from(key.len())?.to_be_bytes());
    hasher.update(u32::try_from(value.len())?.to_be_bytes());
    hasher.update(key);
    hasher.update(value);
    Ok(hasher.finalize().into())
}

/// The canonical account record `layerx_proof::state::decode_account_value`
/// accepts, carrying the authority key the exit recipient signature is checked
/// against.
fn account_value(
    name: &[u8],
    asset: [u8; 32],
    balance: u128,
    authority: [u8; 32],
) -> Result<([u8; 32], Vec<u8>), Box<dyn std::error::Error>> {
    let mut hasher = Sha256::new();
    hasher.update(b"LX:ACCOUNT:v1");
    hasher.update(u32::try_from(name.len())?.to_be_bytes());
    hasher.update(name);
    let account_id: [u8; 32] = hasher.finalize().into();
    let mut value = u16::try_from(name.len())?.to_be_bytes().to_vec();
    value.extend_from_slice(name);
    value.push(1);
    value.extend_from_slice(&balance.to_be_bytes());
    value.extend_from_slice(&asset);
    value.push(1);
    value.extend_from_slice(&9_u64.to_be_bytes());
    value.extend_from_slice(&2_u64.to_be_bytes());
    value.extend_from_slice(&[0, 0]);
    value.extend_from_slice(&authority);
    value.push(1);
    // The real canonical decoder is the authority on this record's shape.
    layerx_proof::state::decode_account_value(account_id, &value)
        .map_err(|error| format!("{error:?}"))?;
    Ok((account_id, value))
}

struct Fixture {
    evidence: ExitEvidence,
    account: [u8; 32],
    state_root: [u8; 32],
    withdrawal_id: [u8; 32],
    nullifier: [u8; 32],
    claim_id: [u8; 32],
    authority: SigningKey,
    message: Vec<u8>,
    transaction: String,
}

fn witness(account: [u8; 32], value: Vec<u8>) -> Result<StateWitness, Box<dyn std::error::Error>> {
    let mut key = vec![4_u8];
    key.extend_from_slice(&account);
    Ok(StateWitness {
        module_id: 0,
        key,
        value,
        account_path: Some(AccountPath {
            index: 0,
            count: 2,
            siblings: vec![state_leaf(&[4; 33], b"neighbour")?],
        }),
        leaf_index_a: 0,
        leaf_count_a: 2,
        siblings_a: vec![state_leaf(b"sequence", &21_u64.to_be_bytes())?],
        leaf_count_b: 10,
        siblings_b: vec![[0xd1; 32], [0xd2; 32], [0xd3; 32], [0xd4; 32]],
    })
}

fn fixture() -> Result<Fixture, Box<dyn std::error::Error>> {
    let authority = SigningKey::from_bytes(&AUTHORITY_SEED);
    let (account, value) = account_value(
        ACCOUNT_NAME,
        ASSET,
        BALANCE,
        authority.verifying_key().to_bytes(),
    )?;
    let witness = witness(account, value)?;
    let state_root = witness.root().map_err(|error| format!("{error:?}"))?;
    witness
        .verify(state_root)
        .map_err(|error| format!("{error:?}"))?;
    let encoded = witness.encode().map_err(|error| format!("{error:?}"))?;
    let message = exit_recipient_message(NETWORK_ID, &account, &ASSET, RECIPIENT, &state_root);
    let material = ForcedExitMaterial {
        witness: encoded,
        batch_number: LATEST_BATCH,
        account,
        asset_id: ASSET,
        recipient: RECIPIENT,
        recipient_signature: authority.sign(&message).to_bytes(),
    }
    .validated()
    .map_err(|error| format!("{error:?}"))?;
    let withdrawal_id = exit_withdrawal_id(NETWORK_ID, &account, &ASSET, &state_root);
    let nullifier = withdrawal_nullifier(
        NETWORK_ID,
        &withdrawal_id,
        &account,
        &ASSET,
        BALANCE,
        &state_root,
    );
    Ok(Fixture {
        evidence: ExitEvidence {
            material,
            finalised_balance: BALANCE,
        },
        account,
        state_root,
        withdrawal_id,
        nullifier,
        claim_id: exit_claim_id(CHAIN_ID, nullifier),
        authority,
        message,
        transaction: hex(&[0xc3; 32]),
    })
}

impl Fixture {
    /// The chain exactly as a halted Paxeer network serves an eligible exit.
    fn chain(&self) -> Chain {
        let mut chain = Chain {
            head: EXIT_BLOCK
                .saturating_add(REQUIRED_CONFIRMATIONS)
                .saturating_sub(1),
            ..Chain::default()
        };
        chain.view(
            ANCHOR_PRECOMPILE,
            &SELECTOR_LATEST_FINALIZED,
            &two_words(number_word(&LATEST_BATCH.to_be_bytes()), true),
        );
        chain.view(
            ANCHOR_PRECOMPILE,
            &anchor_call(SELECTOR_FINALIZED_STATE_ROOT, LATEST_BATCH),
            &two_words(self.state_root, true),
        );
        chain.view(
            CUSTODY_PRECOMPILE,
            &exit_eligible_calldata(),
            &boolean_word(true),
        );
        chain.view(
            CUSTODY_PRECOMPILE,
            &nullifier_status_calldata(self.nullifier),
            &number_word(&[0]),
        );
        chain.view(
            CUSTODY_PRECOMPILE,
            &get_claim_calldata(self.claim_id),
            &encode_claim(&self.stored_claim(0)),
        );
        chain
    }

    fn stored_claim(&self, status: u8) -> CustodyClaim {
        CustodyClaim {
            claim_id: self.claim_id,
            kind: 2,
            status,
            nullifier: self.nullifier,
            withdrawal_id: self.withdrawal_id,
            account: self.account,
            asset_id: ASSET,
            denom: "ulxp".to_owned(),
            recipient: RECIPIENT,
            amount: BALANCE,
            batch_number: LATEST_BATCH,
            anchor: self.state_root,
            available_at: 0,
        }
    }

    fn executed_log(&self, amount: u128) -> LogRecord {
        let mut data = self.account.to_vec();
        data.extend_from_slice(&ASSET);
        data.extend_from_slice(&address_word(RECIPIENT));
        data.extend_from_slice(&number_word(&amount.to_be_bytes()));
        LogRecord {
            address: CUSTODY_PRECOMPILE,
            topics: vec![
                EMERGENCY_EXIT_EXECUTED_TOPIC,
                self.claim_id,
                self.nullifier,
                self.state_root,
            ],
            data,
        }
    }

    fn executed_log_json(&self, number: u64) -> Json {
        let record = self.executed_log(BALANCE);
        Json::Object(vec![
            member("address", text(&hex(&record.address.bytes()))),
            member(
                "topics",
                Json::Array(
                    record
                        .topics
                        .iter()
                        .map(|topic| text(&hex(topic)))
                        .collect(),
                ),
            ),
            member("data", text(&hex(&record.data))),
            member("transactionHash", text(&self.transaction)),
            member("blockHash", text(&hex(&block_hash(number)))),
            member("blockNumber", text(&format!("0x{number:x}"))),
            member("transactionIndex", text("0x0")),
            member("removed", Json::Bool(false)),
        ])
    }

    fn claim(&self, exit: &EmergencyExit) -> Result<ExitClaim, Box<dyn std::error::Error>> {
        exit.construct_claim(&self.evidence)
            .map_err(|error| format!("{error:?}").into())
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[test]
fn eligibility_reports_exactly_what_the_precompiles_declare() -> TestResult {
    let fixture = fixture()?;
    let node = Node::launch(fixture.chain())?;
    let exit = node.exit()?;

    assert_eq!(exit.contract(), CUSTODY_PRECOMPILE);
    assert_eq!(exit.network_id(), NETWORK_ID);
    assert_eq!(exit.required_confirmations(), REQUIRED_CONFIRMATIONS);
    assert_eq!(
        exit.eligibility().map_err(|error| format!("{error:?}"))?,
        ExitEligibility::Eligible {
            batch_number: LATEST_BATCH,
            state_root: fixture.state_root,
        }
    );

    node.edit(|chain| {
        chain.view(
            CUSTODY_PRECOMPILE,
            &exit_eligible_calldata(),
            &boolean_word(false),
        );
    })?;
    assert_eq!(
        exit.eligibility().map_err(|error| format!("{error:?}"))?,
        ExitEligibility::NetworkOperatingNormally {
            batch_number: LATEST_BATCH,
        }
    );

    node.edit(|chain| {
        chain.view(
            ANCHOR_PRECOMPILE,
            &SELECTOR_LATEST_FINALIZED,
            &two_words([0; 32], false),
        );
    })?;
    assert_eq!(
        exit.eligibility().map_err(|error| format!("{error:?}"))?,
        ExitEligibility::NoFinalisedCheckpoint
    );

    // A malformed precompile answer is never read as eligibility.
    node.edit(|chain| {
        chain.view(
            ANCHOR_PRECOMPILE,
            &SELECTOR_LATEST_FINALIZED,
            &number_word(&LATEST_BATCH.to_be_bytes()),
        );
    })?;
    assert!(matches!(
        exit.eligibility(),
        Err(ExitError::Contract { .. })
    ));
    Ok(())
}

#[test]
fn a_constructed_exit_binds_the_finalised_witness_and_signed_recipient() -> TestResult {
    let fixture = fixture()?;
    let node = Node::launch(fixture.chain())?;
    let exit = node.exit()?;
    let claim = fixture.claim(&exit)?;

    assert_eq!(claim.contract, CUSTODY_PRECOMPILE);
    assert_eq!(claim.calldata, fixture.evidence.material.request_calldata());
    assert_eq!(
        claim.execute_calldata,
        fixture.evidence.material.execute_calldata()
    );
    assert_ne!(claim.calldata, claim.execute_calldata);
    assert_eq!(claim.batch_number, LATEST_BATCH);
    assert_eq!(claim.state_root, fixture.state_root);
    assert_eq!(claim.withdrawal_id, fixture.withdrawal_id);
    assert_eq!(claim.nullifier, fixture.nullifier);
    assert_eq!(claim.claim_id, fixture.claim_id);
    assert_eq!(claim.account, fixture.account);
    assert_eq!(claim.asset_id, ASSET);
    assert_eq!(claim.finalised_balance, BALANCE);
    assert_eq!(claim.recipient, RECIPIENT);

    // The identifiers are the declared formulas over the anchor state root.
    assert_eq!(
        claim.withdrawal_id,
        exit_withdrawal_id(NETWORK_ID, &fixture.account, &ASSET, &fixture.state_root)
    );
    assert_eq!(
        claim.nullifier,
        withdrawal_nullifier(
            NETWORK_ID,
            &claim.withdrawal_id,
            &fixture.account,
            &ASSET,
            BALANCE,
            &fixture.state_root,
        )
    );
    assert_eq!(claim.claim_id, exit_claim_id(CHAIN_ID, claim.nullifier));
    Ok(())
}

#[test]
fn construction_refuses_an_ineligible_network_or_a_stale_batch() -> TestResult {
    let fixture = fixture()?;
    let node = Node::launch(fixture.chain())?;
    let exit = node.exit()?;

    let mut stale = fixture.evidence.clone();
    stale.material.batch_number = LATEST_BATCH.saturating_sub(1);
    assert_eq!(
        exit.construct_claim(&stale),
        Err(ExitError::Refused(ExitRefusal::StaleBatch {
            latest: LATEST_BATCH,
            supplied: LATEST_BATCH - 1,
        }))
    );

    node.edit(|chain| {
        chain.view(
            CUSTODY_PRECOMPILE,
            &exit_eligible_calldata(),
            &boolean_word(false),
        );
    })?;
    assert_eq!(
        exit.construct_claim(&fixture.evidence),
        Err(ExitError::Refused(ExitRefusal::NotEligible {
            eligibility: ExitEligibility::NetworkOperatingNormally {
                batch_number: LATEST_BATCH,
            },
        }))
    );

    node.edit(|chain| {
        chain.view(
            ANCHOR_PRECOMPILE,
            &SELECTOR_LATEST_FINALIZED,
            &two_words([0; 32], false),
        );
    })?;
    assert_eq!(
        exit.construct_claim(&fixture.evidence),
        Err(ExitError::Refused(ExitRefusal::NotEligible {
            eligibility: ExitEligibility::NoFinalisedCheckpoint,
        }))
    );
    Ok(())
}

#[test]
fn construction_refuses_a_state_root_the_witness_does_not_prove() -> TestResult {
    let fixture = fixture()?;
    let node = Node::launch(fixture.chain())?;
    let exit = node.exit()?;
    let mut wrong = fixture.state_root;
    wrong[0] ^= 1;
    node.edit(move |chain| {
        chain.view(
            ANCHOR_PRECOMPILE,
            &anchor_call(SELECTOR_FINALIZED_STATE_ROOT, LATEST_BATCH),
            &two_words(wrong, true),
        );
    })?;
    assert_eq!(
        exit.construct_claim(&fixture.evidence),
        Err(ExitError::Refused(ExitRefusal::NativeBalanceNotProven))
    );
    Ok(())
}

#[test]
fn construction_refuses_every_used_nullifier_state() -> TestResult {
    let fixture = fixture()?;
    let node = Node::launch(fixture.chain())?;
    let exit = node.exit()?;
    let nullifier = fixture.nullifier;
    for (status, expected) in [
        (1_u8, ExitRefusal::Held { nullifier }),
        (2, ExitRefusal::AlreadyExited { nullifier }),
        (3, ExitRefusal::ClaimCancelled { nullifier }),
    ] {
        node.edit(move |chain| {
            chain.view(
                CUSTODY_PRECOMPILE,
                &nullifier_status_calldata(nullifier),
                &number_word(&[status]),
            );
        })?;
        assert_eq!(
            exit.construct_claim(&fixture.evidence),
            Err(ExitError::Refused(expected))
        );
    }

    node.edit(move |chain| {
        chain.view(
            CUSTODY_PRECOMPILE,
            &nullifier_status_calldata(nullifier),
            &number_word(&[4]),
        );
    })?;
    assert!(matches!(
        exit.construct_claim(&fixture.evidence),
        Err(ExitError::Contract { .. })
    ));
    Ok(())
}

#[test]
fn exit_balance_verification_refuses_every_unproven_binding() -> TestResult {
    let fixture = fixture()?;
    let evidence = &fixture.evidence;
    let root = fixture.state_root;
    verify_exit_balance(evidence, NETWORK_ID, root).map_err(|error| format!("{error:?}"))?;

    // Empty consensus bindings are refused before any proof work.
    for field in 0..4 {
        let mut changed = evidence.clone();
        let expected = match field {
            0 => {
                changed.material.account = [0; 32];
                ExitRefusal::EmptyAccount
            }
            1 => {
                changed.material.asset_id = [0; 32];
                ExitRefusal::EmptyAsset
            }
            2 => {
                changed.finalised_balance = 0;
                ExitRefusal::ZeroBalance
            }
            _ => {
                changed.material.recipient = EvmAddress::new([0; 20]);
                ExitRefusal::ZeroRecipient
            }
        };
        assert_eq!(
            verify_exit_balance(&changed, NETWORK_ID, root),
            Err(ExitError::Refused(expected))
        );
    }

    // Bytes that are not a canonical witness are refused as material.
    let mut not_a_witness = evidence.clone();
    not_a_witness.material.witness = vec![0xff; 64];
    assert_eq!(
        verify_exit_balance(&not_a_witness, NETWORK_ID, root),
        Err(ExitError::Refused(ExitRefusal::Material("witness")))
    );

    // A witness that does not open under the anchor root proves nothing.
    let mut wrong_root = root;
    wrong_root[31] ^= 1;
    assert_eq!(
        verify_exit_balance(evidence, NETWORK_ID, wrong_root),
        Err(ExitError::Refused(ExitRefusal::NativeBalanceNotProven))
    );

    // The witness is the authority on the account, asset and whole balance.
    for field in 0..3 {
        let mut changed = evidence.clone();
        match field {
            0 => changed.material.account[0] ^= 1,
            1 => changed.material.asset_id[31] ^= 1,
            _ => changed.finalised_balance += 1,
        }
        assert_eq!(
            verify_exit_balance(&changed, NETWORK_ID, root),
            Err(ExitError::Refused(ExitRefusal::NativeBalanceNotProven))
        );
    }

    // Only the account authority can name the recipient, for this network,
    // this recipient and this anchor.
    for position in [0_usize, 5, 63] {
        let mut changed = evidence.clone();
        changed.material.recipient_signature[position] ^= 1;
        assert_eq!(
            verify_exit_balance(&changed, NETWORK_ID, root),
            Err(ExitError::Refused(ExitRefusal::RecipientNotAuthorized))
        );
    }
    let mut other_recipient = evidence.clone();
    other_recipient.material.recipient = EvmAddress::new([0x51; 20]);
    assert_eq!(
        verify_exit_balance(&other_recipient, NETWORK_ID, root),
        Err(ExitError::Refused(ExitRefusal::RecipientNotAuthorized))
    );
    assert_eq!(
        verify_exit_balance(evidence, NETWORK_ID + 1, root),
        Err(ExitError::Refused(ExitRefusal::RecipientNotAuthorized))
    );

    let stranger = SigningKey::from_bytes(&[0x62; 32]);
    let mut foreign_signer = evidence.clone();
    foreign_signer.material.recipient_signature = stranger.sign(&fixture.message).to_bytes();
    assert_eq!(
        verify_exit_balance(&foreign_signer, NETWORK_ID, root),
        Err(ExitError::Refused(ExitRefusal::RecipientNotAuthorized))
    );

    // The signed message's domain separation is load bearing.
    let mut undomained = fixture.message.clone();
    undomained.remove(b"LX:SETTLE:RECIPIENT:v1".len());
    let mut wrong_domain = evidence.clone();
    wrong_domain.material.recipient_signature = fixture.authority.sign(&undomained).to_bytes();
    assert_eq!(
        verify_exit_balance(&wrong_domain, NETWORK_ID, root),
        Err(ExitError::Refused(ExitRefusal::RecipientNotAuthorized))
    );

    // An account record without an authority key can never authorise an exit.
    let mut unauthorised = fixture.evidence.clone();
    let (account, mut value) = account_value(
        ACCOUNT_NAME,
        ASSET,
        BALANCE,
        fixture.authority.verifying_key().to_bytes(),
    )?;
    let last = value.len().saturating_sub(1);
    value[last] = 0;
    let at = value.len().saturating_sub(33);
    value[at..last].copy_from_slice(&[0; 32]);
    let stripped = witness(account, value)?;
    let stripped_root = stripped.root().map_err(|error| format!("{error:?}"))?;
    unauthorised.material.witness = stripped.encode().map_err(|error| format!("{error:?}"))?;
    assert_eq!(
        verify_exit_balance(&unauthorised, NETWORK_ID, stripped_root),
        Err(ExitError::Refused(ExitRefusal::RecipientNotAuthorized))
    );
    Ok(())
}

#[test]
fn the_stored_claim_record_only_binds_the_constructed_exit() -> TestResult {
    let fixture = fixture()?;
    let node = Node::launch(fixture.chain())?;
    let exit = node.exit()?;
    let claim = fixture.claim(&exit)?;

    // Status 0 is "not queued", never an error.
    assert_eq!(
        exit.claim_record(&claim)
            .map_err(|error| format!("{error:?}"))?,
        None
    );

    let claim_id = fixture.claim_id;
    let queued = encode_claim(&fixture.stored_claim(1));
    node.edit(move |chain| {
        chain.view(CUSTODY_PRECOMPILE, &get_claim_calldata(claim_id), &queued);
    })?;
    assert_eq!(
        exit.claim_record(&claim)
            .map_err(|error| format!("{error:?}"))?,
        Some(fixture.stored_claim(1))
    );

    // A withdrawal claim under the same identifier never binds an exit.
    let mut wrong_kind = fixture.stored_claim(1);
    wrong_kind.kind = 1;
    let encoded = encode_claim(&wrong_kind);
    node.edit(move |chain| {
        chain.view(CUSTODY_PRECOMPILE, &get_claim_calldata(claim_id), &encoded);
    })?;
    assert!(matches!(
        exit.claim_record(&claim),
        Err(ExitError::Contract { .. })
    ));

    let mut wrong_amount = fixture.stored_claim(2);
    wrong_amount.amount = BALANCE.saturating_add(1);
    let encoded = encode_claim(&wrong_amount);
    node.edit(move |chain| {
        chain.view(CUSTODY_PRECOMPILE, &get_claim_calldata(claim_id), &encoded);
    })?;
    assert!(matches!(
        exit.claim_record(&claim),
        Err(ExitError::Contract { .. })
    ));
    Ok(())
}

#[test]
fn execution_evidence_must_be_the_precompiles_own_single_event() -> TestResult {
    let fixture = fixture()?;
    let node = Node::launch(fixture.chain())?;
    let exit = node.exit()?;
    let claim = fixture.claim(&exit)?;
    let executed = fixture.executed_log(BALANCE);

    let event = EmergencyExit::verify_executed(&claim, std::slice::from_ref(&executed))
        .map_err(|error| format!("{error:?}"))?;
    assert_eq!(event.claim_id, claim.claim_id);
    assert_eq!(event.nullifier, claim.nullifier);
    assert_eq!(event.anchor, claim.state_root);
    assert_eq!(event.account, claim.account);
    assert_eq!(event.asset_id, claim.asset_id);
    assert_eq!(event.recipient, claim.recipient);
    assert_eq!(event.amount, claim.finalised_balance);

    assert_eq!(
        EmergencyExit::verify_executed(&claim, &[]),
        Err(ExitError::MissingEvent)
    );
    assert_eq!(
        EmergencyExit::verify_executed(&claim, &[executed.clone(), executed.clone()]),
        Err(ExitError::DuplicateEvent)
    );

    let mut elsewhere = executed.clone();
    elsewhere.address = EvmAddress::new([0x0d; 20]);
    assert_eq!(
        EmergencyExit::verify_executed(&claim, &[elsewhere]),
        Err(ExitError::MissingEvent)
    );

    assert_eq!(
        EmergencyExit::verify_executed(&claim, &[fixture.executed_log(BALANCE + 1)]),
        Err(ExitError::EventMismatch)
    );

    let mut foreign_anchor = executed.clone();
    foreign_anchor.topics[3] = [0x77; 32];
    assert_eq!(
        EmergencyExit::verify_executed(&claim, &[foreign_anchor]),
        Err(ExitError::EventMismatch)
    );

    let mut truncated = executed;
    truncated.data.truncate(96);
    assert!(matches!(
        EmergencyExit::verify_executed(&claim, &[truncated]),
        Err(ExitError::Contract { .. })
    ));
    Ok(())
}

#[test]
fn exit_progress_reports_only_verified_paxeer_finality() -> TestResult {
    let fixture = fixture()?;
    let node = Node::launch(fixture.chain())?;
    let exit = node.exit()?;
    let claim = fixture.claim(&exit)?;
    let transaction = fixture.transaction.clone();

    // An unknown transaction is never more than pending.
    let mut tracker = exit
        .track(hash(&transaction)?)
        .map_err(|error| format!("{error:?}"))?;
    assert_eq!(ExitProgress::of(&tracker.poll()), ExitProgress::Pending);

    let executed = fixture.executed_log_json(EXIT_BLOCK);
    let input = claim.execute_calldata.clone();
    let tx = transaction.clone();
    node.edit(move |chain| {
        chain.head = EXIT_BLOCK;
        chain
            .transactions
            .insert(tx.clone(), transaction_json(&tx, &input));
        chain.receipts.insert(
            tx.clone(),
            receipt_json(&tx, EXIT_BLOCK, 1, vec![executed.clone()]),
        );
    })?;
    assert_eq!(
        ExitProgress::of(&tracker.poll()),
        ExitProgress::Confirming {
            execution: layerx_paxeer_client::ExecutionOutcome::Succeeded,
            confirmations: 1,
            required: REQUIRED_CONFIRMATIONS,
        }
    );

    node.edit(|chain| {
        chain.head = EXIT_BLOCK
            .saturating_add(REQUIRED_CONFIRMATIONS)
            .saturating_sub(1);
    })?;
    let report = tracker.poll();
    let ExitProgress::Settled {
        inclusion,
        confirmations,
    } = ExitProgress::of(&report)
    else {
        return Err(format!(
            "expected settlement, observed {:?}",
            ExitProgress::of(&report)
        )
        .into());
    };
    assert_eq!(inclusion.block.number, EXIT_BLOCK);
    assert_eq!(confirmations, REQUIRED_CONFIRMATIONS);

    // The executed event the endpoint served binds the constructed claim.
    let logs = report.receipt_logs().ok_or("receipt logs")?;
    EmergencyExit::verify_executed(&claim, logs).map_err(|error| format!("{error:?}"))?;

    // A transaction that disappears after inclusion is reported as displaced.
    let tx = transaction.clone();
    node.edit(move |chain| {
        chain.receipts.remove(&tx);
        chain.transactions.remove(&tx);
    })?;
    assert_eq!(
        ExitProgress::of(&tracker.poll()),
        ExitProgress::Displaced { requeued: false }
    );

    // A reverted execute call is refused, never settled.
    let executed = fixture.executed_log_json(EXIT_BLOCK);
    let input = claim.execute_calldata.clone();
    let tx = transaction.clone();
    node.edit(move |chain| {
        chain
            .transactions
            .insert(tx.clone(), transaction_json(&tx, &input));
        chain.receipts.insert(
            tx.clone(),
            receipt_json(&tx, EXIT_BLOCK, 0, vec![executed.clone()]),
        );
    })?;
    let mut tracker = exit
        .track(hash(&transaction)?)
        .map_err(|error| format!("{error:?}"))?;
    assert_eq!(
        ExitProgress::of(&tracker.poll()),
        ExitProgress::Refused {
            inclusion: layerx_paxeer_client::TransactionInclusion {
                block: layerx_paxeer_client::BlockRef {
                    number: EXIT_BLOCK,
                    hash: block_hash(EXIT_BLOCK),
                },
                transaction_index: 0,
                execution: layerx_paxeer_client::ExecutionOutcome::Reverted,
                deployed_contract: None,
            },
            confirmations: REQUIRED_CONFIRMATIONS,
        }
    );
    Ok(())
}

#[test]
fn exit_configuration_refuses_weak_policies() -> TestResult {
    let base = configuration(endpoint("http://127.0.0.1:24998"));

    let mut zero_network = base.clone();
    zero_network.network_id = 0;
    assert_eq!(
        EmergencyExit::new(zero_network).err(),
        Some(layerx_paxeer_client::ExitConfigError::ZeroNetworkId)
    );

    let mut zero_confirmations = base.clone();
    zero_confirmations.required_confirmations = 0;
    assert_eq!(
        EmergencyExit::new(zero_confirmations).err(),
        Some(layerx_paxeer_client::ExitConfigError::ZeroRequiredConfirmations)
    );

    let mut zero_cadence = base.clone();
    zero_cadence.poll_cadence = Duration::ZERO;
    assert_eq!(
        EmergencyExit::new(zero_cadence).err(),
        Some(layerx_paxeer_client::ExitConfigError::ZeroPollCadence)
    );

    let mut zero_stall = base.clone();
    zero_stall.delayed_after_polls = 0;
    assert_eq!(
        EmergencyExit::new(zero_stall).err(),
        Some(layerx_paxeer_client::ExitConfigError::ZeroDelayedAfterPolls)
    );

    let mut unreachable_agreement = base.clone();
    unreachable_agreement.minimum_endpoint_agreement = 2;
    assert!(EmergencyExit::new(unreachable_agreement).is_err());

    let mut no_endpoints = base.clone();
    no_endpoints.endpoints.clear();
    assert!(EmergencyExit::new(no_endpoints).is_err());

    assert!(EmergencyExit::new(base).is_ok());
    Ok(())
}
