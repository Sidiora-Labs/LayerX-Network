//! The Paxeer withdrawal boundary against the native custody precompile.
//!
//! Every test drives the real [`WithdrawalBoundary`] over real HTTP against an
//! in-process JSON-RPC server that answers the custody precompile (`0x…1013`)
//! and anchor precompile (`0x…1014`) views the boundary reads. The withdrawal
//! evidence is the real `bound-native-withdrawal` fixture, so the receipt
//! signature, the Merkle path under the sequencer-signed header and the
//! withdrawal effect body all verify locally for real.

use std::collections::HashMap;
use std::io::{Read as _, Write as _};
use std::net::{TcpListener, TcpStream};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;

use layerx_paxeer_client::custody::{
    exit_eligible_calldata, get_asset_calldata, get_claim_calldata, native_asset_id_calldata,
    nullifier_status_calldata, withdrawal_claim_id, withdrawal_nullifier, CustodyAsset,
    CustodyClaim, CLAIM_FINALISED_TOPIC, CLAIM_QUEUED_TOPIC, CUSTODY_RELEASE_TOPIC,
};
use layerx_paxeer_client::{
    parse_json, CancelledFundsDisposition, ClaimProgress, ClaimRefusal, CommittedWithdrawalDebit,
    DebitExpectation, EndpointConfig, EndpointTransport, FinalityReport, FinalityStage,
    FinalityTracker, Json, PaxeerFundsDisposition, ProtocolDebitDisposition, TransactionHash,
    WithdrawalBoundary, WithdrawalClaim, WithdrawalConfig, WithdrawalError, WithdrawalMaterial,
    ANCHOR_PRECOMPILE, CUSTODY_PRECOMPILE, WEI_PER_BASE_UNIT,
};
use layerx_proof::receipt::AuthorizedBatch;
use layerx_types::intent::EvmAddress;
use sha2::{Digest as _, Sha256};

type TestResult = Result<(), Box<dyn std::error::Error>>;

const RECEIPT: &[u8] =
    include_bytes!("../../../../tests/fixtures/asset/bound-native-withdrawal/receipt");
const PROOF: &[u8] =
    include_bytes!("../../../../tests/fixtures/asset/bound-native-withdrawal/receipt.proof");
const HEADER: &[u8] =
    include_bytes!("../../../../tests/fixtures/asset/bound-native-withdrawal/header");
const HEADER_SIGNATURE: &[u8; 64] =
    include_bytes!("../../../../tests/fixtures/asset/bound-native-withdrawal/header.signature");
const SEQUENCER_PUBLIC: [u8; 32] =
    *include_bytes!("../../../../tests/fixtures/asset/bound-native-withdrawal/sequencer.public");
const MAINTENANCE_RECEIPT: &[u8] =
    include_bytes!("../../../../tests/fixtures/asset/bound-native-withdrawal/maintenance.receipt");

const CHAIN_ID: u64 = 31_337;
const REQUIRED_CONFIRMATIONS: u64 = 2;
const SUBMISSION_BLOCK: u64 = 41;
const PAYOUT_BLOCK: u64 = 44;
const AVAILABLE_AT: u64 = 1_893_456_000;
const FINALIZED_STATE_ROOT: [u8; 4] = [0x0f, 0x60, 0x7f, 0xe4];
const FINALIZED_RECEIPT_ROOT: [u8; 4] = [0xe0, 0xa3, 0xcc, 0xaa];

// ---------------------------------------------------------------------------
// In-process JSON-RPC server
// ---------------------------------------------------------------------------

#[derive(Clone, Debug, Default)]
struct Chain {
    head: u64,
    timestamp: u64,
    calls: HashMap<(String, String), String>,
    receipts: HashMap<String, Json>,
    transactions: HashMap<String, Json>,
    balances: HashMap<String, String>,
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
                Some("latest") => block(self.head, self.timestamp),
                Some(tag) => match quantity(tag) {
                    Some(number) if number <= self.head => block(number, self.timestamp),
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
            "eth_getBalance" => params
                .first()
                .and_then(Json::as_text)
                .and_then(|address| self.balances.get(address))
                .map_or_else(|| text("0x0"), |value| text(value)),
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
    chain: Arc<Mutex<Chain>>,
    endpoint: EndpointConfig,
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
            chain: shared,
            endpoint: endpoint(&format!("http://{address}")),
        })
    }

    fn boundary(&self) -> Result<WithdrawalBoundary, Box<dyn std::error::Error>> {
        WithdrawalBoundary::new(configuration(self.endpoint.clone()))
            .map_err(|error| format!("{error:?}").into())
    }

    fn edit(&self, change: impl FnOnce(&mut Chain)) -> Result<(), Box<dyn std::error::Error>> {
        let mut chain = self.chain.lock().map_err(|_| "chain lock poisoned")?;
        change(&mut chain);
        Ok(())
    }

    fn report(
        &self,
        boundary: &WithdrawalBoundary,
        transaction: TransactionHash,
    ) -> Result<FinalityReport, Box<dyn std::error::Error>> {
        let mut tracker: FinalityTracker = boundary
            .track(transaction)
            .map_err(|error| format!("{error:?}"))?;
        let report = tracker.poll();
        match report.stage() {
            FinalityStage::Final { .. } => Ok(report),
            stage => Err(format!("expected a final stage, observed {stage:?}").into()),
        }
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

fn configuration(endpoint: EndpointConfig) -> WithdrawalConfig {
    WithdrawalConfig {
        endpoints: vec![endpoint],
        minimum_endpoint_agreement: 1,
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

fn block(number: u64, timestamp: u64) -> Json {
    Json::Object(vec![
        member("number", text(&format!("0x{number:x}"))),
        member("hash", text(&hex(&block_hash(number)))),
        member("timestamp", text(&format!("0x{timestamp:x}"))),
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

/// Encodes one dynamic Solidity tuple whose single `string` member sits at
/// `text_index`, exactly as the precompile returns `getClaim` and `getAsset`.
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

fn encode_asset(asset: &CustodyAsset) -> Vec<u8> {
    tuple(
        &[
            asset.asset_id,
            [0; 32],
            address_word(asset.pointer),
            boolean_word(asset.enabled),
            boolean_word(asset.paused),
            number_word(&asset.minimum_deposit.to_be_bytes()),
            number_word(&asset.custody_cap.to_be_bytes()),
            number_word(&asset.custodied.to_be_bytes()),
            number_word(&asset.released.to_be_bytes()),
            number_word(&asset.pending.to_be_bytes()),
        ],
        1,
        &asset.denom,
    )
}

/// `(value, present)` exactly as the anchor precompile returns its roots.
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

fn log(
    transaction: &str,
    number: u64,
    address: EvmAddress,
    topics: &[[u8; 32]],
    data: &[u8],
) -> Json {
    Json::Object(vec![
        member("address", text(&hex(&address.bytes()))),
        member(
            "topics",
            Json::Array(topics.iter().map(|topic| text(&hex(topic))).collect()),
        ),
        member("data", text(&hex(data))),
        member("transactionHash", text(transaction)),
        member("blockHash", text(&hex(&block_hash(number)))),
        member("blockNumber", text(&format!("0x{number:x}"))),
        member("transactionIndex", text("0x0")),
        member("removed", Json::Bool(false)),
    ])
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

fn transaction_json(transaction: &str, to: Option<EvmAddress>, input: &[u8], value: &str) -> Json {
    Json::Object(vec![
        member("hash", text(transaction)),
        member(
            "to",
            to.map_or(Json::Null, |address| text(&hex(&address.bytes()))),
        ),
        member("input", text(&hex(input))),
        member("value", text(value)),
    ])
}

fn hash(value: &str) -> Result<TransactionHash, Box<dyn std::error::Error>> {
    TransactionHash::from_hex(value).map_err(|error| format!("{error:?}").into())
}

// ---------------------------------------------------------------------------
// The real withdrawal evidence
// ---------------------------------------------------------------------------

struct Fixture {
    debit: CommittedWithdrawalDebit,
    material: WithdrawalMaterial,
    expectation: DebitExpectation,
    batch_number: u64,
    state_root: [u8; 32],
    receipt_root: [u8; 32],
    anchor: [u8; 32],
    nullifier: [u8; 32],
    claim_id: [u8; 32],
    transaction: String,
    payout_transaction: String,
}

fn fixture_proof() -> Result<layerx_proof::merkle::Proof, Box<dyn std::error::Error>> {
    let wire =
        layerx_wire::receipt::decode_merkle_proof(PROOF).map_err(|error| format!("{error:?}"))?;
    layerx_proof::merkle::Proof::new(
        wire.leaf_index(),
        wire.leaf_count(),
        wire.siblings().to_vec(),
    )
    .map_err(|error| format!("{error:?}").into())
}

fn withdrawal_effect() -> Result<Vec<u8>, Box<dyn std::error::Error>> {
    let decoded = layerx_wire::receipt::decode(RECEIPT).map_err(|error| format!("{error:?}"))?;
    let protocol = decoded
        .protocol()
        .ok_or("fixture is not a protocol receipt")?;
    let body = protocol
        .effects()
        .get(1)
        .map(layerx_wire::receipt::Effect::body)
        .ok_or("fixture has no withdrawal effect")?;
    if body.len() != 254 {
        return Err("fixture withdrawal effect has an unexpected length".into());
    }
    Ok(body.to_vec())
}

fn expectation() -> Result<DebitExpectation, Box<dyn std::error::Error>> {
    let decoded = layerx_wire::receipt::decode(RECEIPT).map_err(|error| format!("{error:?}"))?;
    let protocol = decoded
        .protocol()
        .ok_or("fixture is not a protocol receipt")?;
    let body = withdrawal_effect()?;
    Ok(DebitExpectation {
        activity_id: protocol.activity_id(),
        network_id: u32::from_be_bytes(body.get(2..6).ok_or("network")?.try_into()?),
        withdrawal_id: body.get(6..38).ok_or("withdrawal_id")?.try_into()?,
        account: body.get(38..70).ok_or("account")?.try_into()?,
        withdrawals_account: protocol.to(),
        asset_id: body.get(70..102).ok_or("asset_id")?.try_into()?,
        amount: u128::from_be_bytes(body.get(102..118).ok_or("amount")?.try_into()?),
        recipient: EvmAddress::new(body.get(130..150).ok_or("recipient")?.try_into()?),
    })
}

fn authorized_batch() -> Result<AuthorizedBatch, Box<dyn std::error::Error>> {
    let decoded = layerx_wire::receipt::decode(RECEIPT).map_err(|error| format!("{error:?}"))?;
    let protocol = decoded
        .protocol()
        .ok_or("fixture is not a protocol receipt")?;
    Ok(AuthorizedBatch::new(
        protocol.batch_id(),
        protocol.asset(),
        protocol.previous_state_root(),
        protocol.resulting_state_root(),
        SEQUENCER_PUBLIC,
    ))
}

fn fixture() -> Result<Fixture, Box<dyn std::error::Error>> {
    let decoded = layerx_wire::receipt::decode(RECEIPT).map_err(|error| format!("{error:?}"))?;
    let protocol = decoded
        .protocol()
        .ok_or("fixture is not a protocol receipt")?;
    let header =
        layerx_wire::receipt::decode_batch_header(HEADER).map_err(|error| format!("{error:?}"))?;
    let expectation = expectation()?;
    let debit = CommittedWithdrawalDebit::verify(RECEIPT, &authorized_batch()?, expectation)
        .map_err(|error| format!("{error:?}"))?;
    let material = WithdrawalMaterial::from_inclusion(
        RECEIPT.to_vec(),
        &fixture_proof()?,
        HEADER.to_vec(),
        *HEADER_SIGNATURE,
    )
    .map_err(|error| format!("{error:?}"))?;
    let body = withdrawal_effect()?;
    let anchor: [u8; 32] = body.get(150..182).ok_or("anchor")?.try_into()?;
    let nullifier = withdrawal_nullifier(
        expectation.network_id,
        &expectation.withdrawal_id,
        &expectation.account,
        &expectation.asset_id,
        expectation.amount,
        &anchor,
    );
    if nullifier != protocol.context_hash() {
        return Err("fixture nullifier does not match the receipt context hash".into());
    }
    Ok(Fixture {
        debit,
        material,
        expectation,
        batch_number: header.batch_number(),
        state_root: header.resulting_state_root(),
        receipt_root: header.receipt_merkle_root(),
        anchor,
        nullifier,
        claim_id: withdrawal_claim_id(CHAIN_ID, nullifier, expectation.recipient),
        transaction: hex(&[0xa1; 32]),
        payout_transaction: hex(&[0xb2; 32]),
    })
}

impl Fixture {
    fn stored_claim(&self, status: u8) -> CustodyClaim {
        CustodyClaim {
            claim_id: self.claim_id,
            kind: 1,
            status,
            nullifier: self.nullifier,
            withdrawal_id: self.expectation.withdrawal_id,
            account: self.expectation.account,
            asset_id: self.expectation.asset_id,
            denom: "ulxp".to_owned(),
            recipient: self.expectation.recipient,
            amount: self.expectation.amount,
            batch_number: self.batch_number,
            anchor: self.anchor,
            available_at: AVAILABLE_AT,
        }
    }

    fn asset(&self, enabled: bool, paused: bool) -> CustodyAsset {
        CustodyAsset {
            asset_id: self.expectation.asset_id,
            denom: "ulxp".to_owned(),
            pointer: EvmAddress::new([0; 20]),
            enabled,
            paused,
            minimum_deposit: 1,
            custody_cap: 1_000_000,
            custodied: 1_000,
            released: 0,
            pending: 1,
        }
    }

    fn queued_log(&self, amount: u128) -> Json {
        let mut data = self.expectation.asset_id.to_vec();
        data.extend_from_slice(&address_word(self.expectation.recipient));
        data.extend_from_slice(&number_word(&amount.to_be_bytes()));
        data.extend_from_slice(&number_word(&AVAILABLE_AT.to_be_bytes()));
        log(
            &self.transaction,
            SUBMISSION_BLOCK,
            CUSTODY_PRECOMPILE,
            &[
                CLAIM_QUEUED_TOPIC,
                self.claim_id,
                self.nullifier,
                self.anchor,
            ],
            &data,
        )
    }

    fn finalised_logs(&self) -> Vec<Json> {
        let mut release = number_word(&self.expectation.amount.to_be_bytes()).to_vec();
        release.extend_from_slice(&address_word(CUSTODY_PRECOMPILE));
        vec![
            log(
                &self.payout_transaction,
                PAYOUT_BLOCK,
                CUSTODY_PRECOMPILE,
                &[CLAIM_FINALISED_TOPIC, self.claim_id, self.nullifier],
                &[],
            ),
            log(
                &self.payout_transaction,
                PAYOUT_BLOCK,
                CUSTODY_PRECOMPILE,
                &[
                    CUSTODY_RELEASE_TOPIC,
                    self.claim_id,
                    self.expectation.asset_id,
                    address_word(self.expectation.recipient),
                ],
                &release,
            ),
        ]
    }

    /// The chain exactly as a healthy Paxeer node serves a constructible claim.
    fn chain(&self) -> Chain {
        let mut chain = Chain {
            head: SUBMISSION_BLOCK
                .saturating_add(REQUIRED_CONFIRMATIONS)
                .saturating_sub(1),
            timestamp: AVAILABLE_AT.saturating_sub(600),
            ..Chain::default()
        };
        chain.view(
            ANCHOR_PRECOMPILE,
            &anchor_call(FINALIZED_STATE_ROOT, self.batch_number),
            &two_words(self.state_root, true),
        );
        chain.view(
            ANCHOR_PRECOMPILE,
            &anchor_call(FINALIZED_RECEIPT_ROOT, self.batch_number),
            &two_words(self.receipt_root, true),
        );
        chain.view(
            CUSTODY_PRECOMPILE,
            &nullifier_status_calldata(self.nullifier),
            &number_word(&[0]),
        );
        chain.view(
            CUSTODY_PRECOMPILE,
            &get_asset_calldata(self.expectation.asset_id),
            &encode_asset(&self.asset(true, false)),
        );
        chain.view(
            CUSTODY_PRECOMPILE,
            &native_asset_id_calldata(),
            &self.expectation.asset_id,
        );
        chain.view(
            CUSTODY_PRECOMPILE,
            &get_claim_calldata(self.claim_id),
            &encode_claim(&self.stored_claim(0)),
        );
        chain
    }

    /// Registers the queue transaction, its `ClaimQueued` log and the pending
    /// stored claim, exactly as the precompile leaves them after a request.
    fn queue(&self, node: &Node, claim: &WithdrawalClaim) -> TestResult {
        let calldata = claim.calldata().to_vec();
        let queued = self.queued_log(self.expectation.amount);
        let pending = encode_claim(&self.stored_claim(1));
        let claim_id = self.claim_id;
        let transaction = self.transaction.clone();
        node.edit(move |chain| {
            chain.transactions.insert(
                transaction.clone(),
                transaction_json(&transaction, Some(CUSTODY_PRECOMPILE), &calldata, "0x0"),
            );
            chain.receipts.insert(
                transaction.clone(),
                receipt_json(&transaction, SUBMISSION_BLOCK, 1, vec![queued]),
            );
            chain.view(CUSTODY_PRECOMPILE, &get_claim_calldata(claim_id), &pending);
        })
    }

    fn submitted(
        &self,
        node: &Node,
        boundary: &WithdrawalBoundary,
    ) -> Result<
        (
            WithdrawalClaim,
            layerx_paxeer_client::SubmittedWithdrawalClaim,
        ),
        Box<dyn std::error::Error>,
    > {
        let claim = boundary
            .construct_claim(self.debit.clone(), self.material.clone())
            .map_err(|error| format!("{error:?}"))?;
        self.queue(node, &claim)?;
        let report = node.report(boundary, hash(&self.transaction)?)?;
        let submitted = boundary
            .accept_submission(claim.clone(), &report)
            .map_err(|error| format!("{error:?}"))?;
        Ok((claim, submitted))
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[test]
fn a_constructed_claim_binds_the_real_inclusion_and_precompile_state() -> TestResult {
    let fixture = fixture()?;
    let node = Node::launch(fixture.chain())?;
    let boundary = node.boundary()?;

    assert_eq!(boundary.custody_precompile(), CUSTODY_PRECOMPILE);
    assert_eq!(
        boundary.protocol_version(),
        layerx_intents::canonical::STATE_COMMITMENT_PROTOCOL_VERSION
    );

    let claim = boundary
        .construct_claim(fixture.debit.clone(), fixture.material.clone())
        .map_err(|error| format!("{error:?}"))?;
    assert_eq!(claim.contract(), CUSTODY_PRECOMPILE);
    assert_eq!(claim.batch_number(), fixture.batch_number);
    assert_eq!(claim.anchor(), fixture.anchor);
    assert_eq!(claim.nullifier(), fixture.nullifier);
    assert_eq!(claim.claim_id(), fixture.claim_id);
    assert_eq!(claim.calldata(), fixture.material.request_calldata());
    assert_eq!(claim.material(), &fixture.material);
    assert_eq!(claim.debit().expectation(), fixture.expectation);
    assert_ne!(fixture.anchor, [0; 32]);
    Ok(())
}

#[test]
fn an_accepted_submission_progresses_and_pays_out_against_verified_evidence() -> TestResult {
    let fixture = fixture()?;
    let node = Node::launch(fixture.chain())?;
    let boundary = node.boundary()?;
    let (claim, submitted) = fixture.submitted(&node, &boundary)?;

    assert_eq!(submitted.claim(), &claim);
    assert_eq!(submitted.claim_id(), fixture.claim_id);
    assert_eq!(submitted.available_at(), AVAILABLE_AT);
    assert_eq!(
        submitted.submission_transaction(),
        hash(&fixture.transaction)?
    );
    assert_eq!(
        submitted.submission_inclusion().block.number,
        SUBMISSION_BLOCK
    );
    assert_eq!(
        submitted.finalise_calldata(),
        fixture.material.finalise_calldata()
    );
    assert_ne!(submitted.finalise_calldata(), claim.calldata());

    match boundary
        .progress(&submitted)
        .map_err(|error| format!("{error:?}"))?
    {
        ClaimProgress::WaitingForChallengeWindow {
            available_at,
            observed_at,
            remaining,
        } => {
            assert_eq!(available_at, AVAILABLE_AT);
            assert_eq!(observed_at, AVAILABLE_AT - 600);
            assert_eq!(remaining, Duration::from_secs(600));
        }
        other => return Err(format!("expected a challenge window, observed {other:?}").into()),
    }

    node.edit(|chain| chain.timestamp = AVAILABLE_AT)?;
    assert_eq!(
        boundary
            .progress(&submitted)
            .map_err(|error| format!("{error:?}"))?,
        ClaimProgress::ReadyToFinalise {
            available_at: AVAILABLE_AT,
            observed_at: AVAILABLE_AT,
        }
    );

    // The permissionless finalise call pays the recipient in wei.
    let finalise = submitted.finalise_calldata();
    let payout = fixture.payout_transaction.clone();
    let logs = fixture.finalised_logs();
    let paid = encode_claim(&fixture.stored_claim(2));
    let recipient = hex(&fixture.expectation.recipient.bytes());
    let wei = fixture
        .expectation
        .amount
        .checked_mul(WEI_PER_BASE_UNIT)
        .ok_or("payout wei")?;
    let claim_id = fixture.claim_id;
    let nullifier = fixture.nullifier;
    node.edit(move |chain| {
        chain.head = PAYOUT_BLOCK
            .saturating_add(REQUIRED_CONFIRMATIONS)
            .saturating_sub(1);
        chain.transactions.insert(
            payout.clone(),
            transaction_json(&payout, Some(CUSTODY_PRECOMPILE), &finalise, "0x0"),
        );
        chain
            .receipts
            .insert(payout.clone(), receipt_json(&payout, PAYOUT_BLOCK, 1, logs));
        chain.view(CUSTODY_PRECOMPILE, &get_claim_calldata(claim_id), &paid);
        chain.view(
            CUSTODY_PRECOMPILE,
            &nullifier_status_calldata(nullifier),
            &number_word(&[2]),
        );
        chain.balances.insert(recipient, format!("0x{wei:x}"));
    })?;

    assert_eq!(
        boundary
            .progress(&submitted)
            .map_err(|error| format!("{error:?}"))?,
        ClaimProgress::PaidAwaitingPayoutVerification
    );

    let payout_report = node.report(&boundary, hash(&fixture.payout_transaction)?)?;
    let evidence = boundary
        .verify_payout(&submitted, &payout_report)
        .map_err(|error| format!("{error:?}"))?;
    assert_eq!(evidence.claim_id, fixture.claim_id);
    assert_eq!(evidence.checkpoint_hash, fixture.anchor);
    assert_eq!(
        evidence.debit_receipt_reference,
        fixture.debit.receipt_reference()
    );
    assert_eq!(evidence.vault, CUSTODY_PRECOMPILE);
    assert_eq!(evidence.token, EvmAddress::new([0; 20]));
    assert_eq!(evidence.asset_id, fixture.expectation.asset_id);
    assert_eq!(evidence.recipient, fixture.expectation.recipient);
    assert_eq!(evidence.amount, fixture.expectation.amount);
    assert_eq!(
        evidence.payout_transaction,
        hash(&fixture.payout_transaction)?
    );
    assert_eq!(evidence.payout_inclusion.block.number, PAYOUT_BLOCK);

    // A recipient balance below the released amount is never a payout.
    let recipient = hex(&fixture.expectation.recipient.bytes());
    node.edit(move |chain| {
        chain
            .balances
            .insert(recipient, format!("0x{:x}", WEI_PER_BASE_UNIT - 1));
    })?;
    assert!(matches!(
        boundary.verify_payout(&submitted, &payout_report),
        Err(WithdrawalError::PayoutNotVerified { .. })
    ));
    Ok(())
}

#[test]
fn a_payout_is_refused_for_a_wrong_target_input_or_value() -> TestResult {
    let fixture = fixture()?;
    let node = Node::launch(fixture.chain())?;
    let boundary = node.boundary()?;
    let claim = boundary
        .construct_claim(fixture.debit.clone(), fixture.material.clone())
        .map_err(|error| format!("{error:?}"))?;
    fixture.queue(&node, &claim)?;
    let report = node.report(&boundary, hash(&fixture.transaction)?)?;
    let calldata = claim.calldata().to_vec();

    let elsewhere = EvmAddress::new([0x0d; 20]);
    let transaction = fixture.transaction.clone();
    let wrong_target = transaction_json(&transaction, Some(elsewhere), &calldata, "0x0");
    node.edit(move |chain| {
        chain.transactions.insert(transaction, wrong_target);
    })?;
    assert_eq!(
        boundary.accept_submission(claim.clone(), &report),
        Err(WithdrawalError::TransactionTarget {
            expected: CUSTODY_PRECOMPILE,
            found: Some(elsewhere),
        })
    );

    let transaction = fixture.transaction.clone();
    let wrong_input = transaction_json(
        &transaction,
        Some(CUSTODY_PRECOMPILE),
        &fixture.material.finalise_calldata(),
        "0x0",
    );
    node.edit(move |chain| {
        chain.transactions.insert(transaction, wrong_input);
    })?;
    assert_eq!(
        boundary.accept_submission(claim.clone(), &report),
        Err(WithdrawalError::TransactionInput)
    );

    let transaction = fixture.transaction.clone();
    let wrong_value = transaction_json(&transaction, Some(CUSTODY_PRECOMPILE), &calldata, "0x1");
    node.edit(move |chain| {
        chain.transactions.insert(transaction, wrong_value);
    })?;
    assert_eq!(
        boundary.accept_submission(claim, &report),
        Err(WithdrawalError::TransactionValue)
    );
    Ok(())
}

#[test]
fn construction_refuses_unfinalised_batches_and_mismatched_roots() -> TestResult {
    let fixture = fixture()?;
    let mut chain = fixture.chain();
    chain.view(
        ANCHOR_PRECOMPILE,
        &anchor_call(FINALIZED_STATE_ROOT, fixture.batch_number),
        &two_words([0; 32], false),
    );
    let node = Node::launch(chain)?;
    let boundary = node.boundary()?;
    let batch_number = fixture.batch_number;
    assert_eq!(
        boundary.construct_claim(fixture.debit.clone(), fixture.material.clone()),
        Err(WithdrawalError::Refused(ClaimRefusal::BatchNotFinalised {
            batch_number
        }))
    );

    let mut wrong_state_root = fixture.state_root;
    wrong_state_root[0] ^= 1;
    node.edit(move |chain| {
        chain.view(
            ANCHOR_PRECOMPILE,
            &anchor_call(FINALIZED_STATE_ROOT, batch_number),
            &two_words(wrong_state_root, true),
        );
    })?;
    assert_eq!(
        boundary.construct_claim(fixture.debit.clone(), fixture.material.clone()),
        Err(WithdrawalError::Refused(
            ClaimRefusal::FinalisedRootMismatch { batch_number }
        ))
    );

    let mut wrong_receipt_root = fixture.receipt_root;
    wrong_receipt_root[31] ^= 1;
    let state_root = fixture.state_root;
    node.edit(move |chain| {
        chain.view(
            ANCHOR_PRECOMPILE,
            &anchor_call(FINALIZED_STATE_ROOT, batch_number),
            &two_words(state_root, true),
        );
        chain.view(
            ANCHOR_PRECOMPILE,
            &anchor_call(FINALIZED_RECEIPT_ROOT, batch_number),
            &two_words(wrong_receipt_root, true),
        );
    })?;
    assert_eq!(
        boundary.construct_claim(fixture.debit.clone(), fixture.material.clone()),
        Err(WithdrawalError::Refused(
            ClaimRefusal::FinalisedRootMismatch { batch_number }
        ))
    );
    Ok(())
}

#[test]
fn construction_refuses_used_nullifiers_and_unavailable_assets() -> TestResult {
    let fixture = fixture()?;
    let node = Node::launch(fixture.chain())?;
    let boundary = node.boundary()?;
    let nullifier = fixture.nullifier;

    for status in [1_u8, 2, 3] {
        node.edit(move |chain| {
            chain.view(
                CUSTODY_PRECOMPILE,
                &nullifier_status_calldata(nullifier),
                &number_word(&[status]),
            );
        })?;
        assert_eq!(
            boundary.construct_claim(fixture.debit.clone(), fixture.material.clone()),
            Err(WithdrawalError::Refused(ClaimRefusal::NullifierUsed {
                nullifier,
                status,
            }))
        );
    }
    node.edit(move |chain| {
        chain.view(
            CUSTODY_PRECOMPILE,
            &nullifier_status_calldata(nullifier),
            &number_word(&[0]),
        );
    })?;

    for (enabled, paused) in [(false, false), (true, true)] {
        let asset = encode_asset(&fixture.asset(enabled, paused));
        let asset_id = fixture.expectation.asset_id;
        node.edit(move |chain| {
            chain.view(CUSTODY_PRECOMPILE, &get_asset_calldata(asset_id), &asset);
        })?;
        assert_eq!(
            boundary.construct_claim(fixture.debit.clone(), fixture.material.clone()),
            Err(WithdrawalError::Refused(ClaimRefusal::AssetUnavailable {
                asset_id
            }))
        );
    }
    Ok(())
}

#[test]
fn construction_refuses_material_that_is_not_the_verified_debit() -> TestResult {
    let fixture = fixture()?;
    let node = Node::launch(fixture.chain())?;
    let boundary = node.boundary()?;

    let mut foreign = fixture.material.clone();
    foreign.receipt = MAINTENANCE_RECEIPT.to_vec();
    assert_eq!(
        boundary.construct_claim(fixture.debit.clone(), foreign),
        Err(WithdrawalError::Refused(ClaimRefusal::ReceiptMismatch {
            debit: fixture.debit.receipt_reference(),
            material: <[u8; 32]>::from(Sha256::digest(MAINTENANCE_RECEIPT)),
        }))
    );

    let mut tampered_proof = fixture.material.clone();
    let last = tampered_proof.proof.len().saturating_sub(1);
    tampered_proof.proof[last] ^= 1;
    assert_eq!(
        boundary.construct_claim(fixture.debit.clone(), tampered_proof),
        Err(WithdrawalError::Refused(ClaimRefusal::Inclusion(
            "receipt_path"
        )))
    );

    let mut tampered_signature = fixture.material.clone();
    tampered_signature.header_signature[0] ^= 1;
    assert_eq!(
        boundary.construct_claim(fixture.debit.clone(), tampered_signature),
        Err(WithdrawalError::Refused(ClaimRefusal::Inclusion(
            "header_signature"
        )))
    );

    let mut empty_header = fixture.material.clone();
    empty_header.header.clear();
    assert_eq!(
        boundary.construct_claim(fixture.debit.clone(), empty_header),
        Err(WithdrawalError::Refused(ClaimRefusal::Material("header")))
    );

    let mut blank_signature = fixture.material.clone();
    blank_signature.header_signature = [0; 64];
    assert_eq!(
        boundary.construct_claim(fixture.debit.clone(), blank_signature),
        Err(WithdrawalError::Refused(ClaimRefusal::Material(
            "header_signature"
        )))
    );
    Ok(())
}

#[test]
fn construction_refuses_a_debit_that_names_another_network_or_withdrawal() -> TestResult {
    let fixture = fixture()?;
    let node = Node::launch(fixture.chain())?;
    let boundary = node.boundary()?;
    let batch = authorized_batch()?;

    let mut other_network = fixture.expectation;
    other_network.network_id ^= 1;
    let debit = CommittedWithdrawalDebit::verify(RECEIPT, &batch, other_network)
        .map_err(|error| format!("{error:?}"))?;
    assert_eq!(
        boundary.construct_claim(debit, fixture.material.clone()),
        Err(WithdrawalError::Refused(ClaimRefusal::NetworkMismatch {
            debit: other_network.network_id,
            header: fixture.expectation.network_id,
        }))
    );

    let mut other_withdrawal = fixture.expectation;
    other_withdrawal.withdrawal_id[0] ^= 1;
    let debit = CommittedWithdrawalDebit::verify(RECEIPT, &batch, other_withdrawal)
        .map_err(|error| format!("{error:?}"))?;
    assert_eq!(
        boundary.construct_claim(debit, fixture.material.clone()),
        Err(WithdrawalError::Refused(ClaimRefusal::Effect("withdrawal")))
    );
    Ok(())
}

#[test]
fn a_cancelled_claim_reports_both_sides_of_the_funds_boundary() -> TestResult {
    let fixture = fixture()?;
    let node = Node::launch(fixture.chain())?;
    let boundary = node.boundary()?;
    let (_, submitted) = fixture.submitted(&node, &boundary)?;

    let cancelled = encode_claim(&fixture.stored_claim(3));
    let claim_id = fixture.claim_id;
    let nullifier = fixture.nullifier;
    node.edit(move |chain| {
        chain.view(
            CUSTODY_PRECOMPILE,
            &get_claim_calldata(claim_id),
            &cancelled,
        );
        chain.view(
            CUSTODY_PRECOMPILE,
            &nullifier_status_calldata(nullifier),
            &number_word(&[3]),
        );
    })?;

    let expected = CancelledFundsDisposition {
        paxeer: PaxeerFundsDisposition::RetainedInVault {
            vault: CUSTODY_PRECOMPILE,
            asset_id: fixture.expectation.asset_id,
            amount: fixture.expectation.amount,
        },
        layerx: ProtocolDebitDisposition::RemainsCommittedPendingProtocolRecovery {
            debit_receipt_reference: fixture.debit.receipt_reference(),
        },
    };
    assert_eq!(
        boundary
            .progress(&submitted)
            .map_err(|error| format!("{error:?}"))?,
        ClaimProgress::Cancelled {
            disposition: expected
        }
    );

    // Cancellation carries no transaction: the evidence is the agreed state.
    let evidence = boundary
        .verify_cancellation(&submitted)
        .map_err(|error| format!("{error:?}"))?;
    assert_eq!(evidence.claim_id, fixture.claim_id);
    assert_eq!(evidence.checkpoint_hash, fixture.anchor);
    assert_eq!(evidence.disposition, expected);
    assert_eq!(
        evidence.observed_head,
        SUBMISSION_BLOCK
            .saturating_add(REQUIRED_CONFIRMATIONS)
            .saturating_sub(1)
    );
    assert_eq!(
        evidence.debit_receipt_reference,
        fixture.debit.receipt_reference()
    );

    // A cancelled claim can never be turned into a payout.
    let report = node.report(&boundary, hash(&fixture.transaction)?)?;
    assert!(boundary.verify_payout(&submitted, &report).is_err());

    // Nor is cancellation accepted while the nullifier is not terminal.
    node.edit(move |chain| {
        chain.view(
            CUSTODY_PRECOMPILE,
            &nullifier_status_calldata(nullifier),
            &number_word(&[1]),
        );
    })?;
    assert!(matches!(
        boundary.verify_cancellation(&submitted),
        Err(WithdrawalError::ClaimState { .. })
    ));
    Ok(())
}

#[test]
fn restore_submission_readmits_a_claim_in_any_stored_state() -> TestResult {
    let fixture = fixture()?;
    let node = Node::launch(fixture.chain())?;
    let boundary = node.boundary()?;
    let claim = boundary
        .construct_claim(fixture.debit.clone(), fixture.material.clone())
        .map_err(|error| format!("{error:?}"))?;
    fixture.queue(&node, &claim)?;
    let report = node.report(&boundary, hash(&fixture.transaction)?)?;
    let claim_id = fixture.claim_id;

    for status in [1_u8, 2, 3] {
        let stored = encode_claim(&fixture.stored_claim(status));
        node.edit(move |chain| {
            chain.view(CUSTODY_PRECOMPILE, &get_claim_calldata(claim_id), &stored);
        })?;
        let submitted = boundary
            .restore_submission(claim.clone(), &report)
            .map_err(|error| format!("restore status {status}: {error:?}"))?;
        assert_eq!(submitted.claim_id(), fixture.claim_id);
        assert_eq!(submitted.available_at(), AVAILABLE_AT);
        if status != 1 {
            // Only `accept_submission` insists the claim is still pending.
            assert!(matches!(
                boundary.accept_submission(claim.clone(), &report),
                Err(WithdrawalError::ClaimState { .. })
            ));
        }
    }

    let absent = encode_claim(&fixture.stored_claim(0));
    node.edit(move |chain| {
        chain.view(CUSTODY_PRECOMPILE, &get_claim_calldata(claim_id), &absent);
    })?;
    assert!(matches!(
        boundary.restore_submission(claim.clone(), &report),
        Err(WithdrawalError::ClaimState { .. })
    ));

    // A stored claim that names another recipient never binds.
    let mut foreign = fixture.stored_claim(1);
    foreign.recipient = EvmAddress::new([0x0e; 20]);
    let encoded = encode_claim(&foreign);
    node.edit(move |chain| {
        chain.view(CUSTODY_PRECOMPILE, &get_claim_calldata(claim_id), &encoded);
    })?;
    assert!(matches!(
        boundary.restore_submission(claim, &report),
        Err(WithdrawalError::ClaimState { .. })
    ));
    Ok(())
}

#[test]
fn a_queue_event_that_does_not_bind_the_claim_is_refused() -> TestResult {
    let fixture = fixture()?;
    let node = Node::launch(fixture.chain())?;
    let boundary = node.boundary()?;
    let claim = boundary
        .construct_claim(fixture.debit.clone(), fixture.material.clone())
        .map_err(|error| format!("{error:?}"))?;
    fixture.queue(&node, &claim)?;
    let transaction = fixture.transaction.clone();

    let tx = transaction.clone();
    node.edit(move |chain| {
        chain.receipts.insert(
            tx.clone(),
            receipt_json(&tx, SUBMISSION_BLOCK, 1, Vec::new()),
        );
    })?;
    let report = node.report(&boundary, hash(&fixture.transaction)?)?;
    assert_eq!(
        boundary.accept_submission(claim.clone(), &report),
        Err(WithdrawalError::MissingEvent("ClaimQueued"))
    );

    let tx = transaction.clone();
    let queued = fixture.queued_log(fixture.expectation.amount);
    let repeated = vec![queued.clone(), queued];
    node.edit(move |chain| {
        chain
            .receipts
            .insert(tx.clone(), receipt_json(&tx, SUBMISSION_BLOCK, 1, repeated));
    })?;
    let report = node.report(&boundary, hash(&fixture.transaction)?)?;
    assert_eq!(
        boundary.accept_submission(claim.clone(), &report),
        Err(WithdrawalError::DuplicateEvent("ClaimQueued"))
    );

    let tx = transaction;
    let foreign = fixture.queued_log(fixture.expectation.amount.saturating_add(1));
    node.edit(move |chain| {
        chain.receipts.insert(
            tx.clone(),
            receipt_json(&tx, SUBMISSION_BLOCK, 1, vec![foreign]),
        );
    })?;
    let report = node.report(&boundary, hash(&fixture.transaction)?)?;
    assert!(matches!(
        boundary.accept_submission(claim, &report),
        Err(WithdrawalError::MalformedEvent { .. })
    ));
    Ok(())
}

#[test]
fn a_reverted_or_unconfirmed_submission_is_never_accepted() -> TestResult {
    let fixture = fixture()?;
    let node = Node::launch(fixture.chain())?;
    let boundary = node.boundary()?;
    let claim = boundary
        .construct_claim(fixture.debit.clone(), fixture.material.clone())
        .map_err(|error| format!("{error:?}"))?;
    fixture.queue(&node, &claim)?;

    // One confirmation short of the declared depth.
    node.edit(|chain| chain.head = SUBMISSION_BLOCK)?;
    let mut tracker = boundary
        .track(hash(&fixture.transaction)?)
        .map_err(|error| format!("{error:?}"))?;
    let report = tracker.poll();
    assert!(matches!(report.stage(), FinalityStage::Confirming { .. }));
    assert!(matches!(
        boundary.accept_submission(claim.clone(), &report),
        Err(WithdrawalError::NotFinal { .. })
    ));

    // A reverted submission is final but never accepted.
    let transaction = fixture.transaction.clone();
    let queued = fixture.queued_log(fixture.expectation.amount);
    node.edit(move |chain| {
        chain.head = SUBMISSION_BLOCK
            .saturating_add(REQUIRED_CONFIRMATIONS)
            .saturating_sub(1);
        chain.receipts.insert(
            transaction.clone(),
            receipt_json(&transaction, SUBMISSION_BLOCK, 0, vec![queued]),
        );
    })?;
    let report = node.report(&boundary, hash(&fixture.transaction)?)?;
    assert!(matches!(
        boundary.accept_submission(claim, &report),
        Err(WithdrawalError::Reverted { .. })
    ));
    Ok(())
}

#[test]
fn boundary_configuration_refuses_weak_policies() -> TestResult {
    let base = configuration(endpoint("http://127.0.0.1:24999"));

    let mut zero_confirmations = base.clone();
    zero_confirmations.required_confirmations = 0;
    assert!(WithdrawalBoundary::new(zero_confirmations).is_err());

    let mut zero_cadence = base.clone();
    zero_cadence.poll_cadence = Duration::ZERO;
    assert!(WithdrawalBoundary::new(zero_cadence).is_err());

    let mut zero_stall = base.clone();
    zero_stall.delayed_after_polls = 0;
    assert!(WithdrawalBoundary::new(zero_stall).is_err());

    let mut no_endpoints = base.clone();
    no_endpoints.endpoints.clear();
    assert!(WithdrawalBoundary::new(no_endpoints).is_err());

    let mut unreachable_agreement = base.clone();
    unreachable_agreement.minimum_endpoint_agreement = 2;
    assert!(WithdrawalBoundary::new(unreachable_agreement).is_err());

    // The custody precompile only accepts state-commitment withdrawal receipts.
    for version in [0_u16, 1, 2, 4] {
        assert!(WithdrawalBoundary::new_for_protocol(base.clone(), version).is_err());
    }
    assert!(WithdrawalBoundary::new_for_protocol(
        base,
        layerx_intents::canonical::STATE_COMMITMENT_PROTOCOL_VERSION,
    )
    .is_ok());
    Ok(())
}

#[test]
fn the_boundary_only_ever_reads_the_two_declared_precompiles() -> TestResult {
    let fixture = fixture()?;
    let node = Node::launch(fixture.chain())?;
    let boundary = node.boundary()?;
    let (_, submitted) = fixture.submitted(&node, &boundary)?;
    boundary
        .progress(&submitted)
        .map_err(|error| format!("{error:?}"))?;

    // Every declared view is a fixed-width read-only call.
    assert_eq!(get_claim_calldata(fixture.claim_id).len(), 36);
    assert_eq!(nullifier_status_calldata(fixture.nullifier).len(), 36);
    assert_eq!(get_asset_calldata(fixture.expectation.asset_id).len(), 36);
    assert_eq!(native_asset_id_calldata().len(), 4);
    assert_eq!(exit_eligible_calldata().len(), 4);

    let chain = node.chain.lock().map_err(|_| "chain lock poisoned")?;
    let custody = hex(&CUSTODY_PRECOMPILE.bytes());
    let anchor = hex(&ANCHOR_PRECOMPILE.bytes());
    for (target, _) in chain.calls.keys() {
        assert!(
            target == &custody || target == &anchor,
            "unexpected call target {target}"
        );
    }
    Ok(())
}
