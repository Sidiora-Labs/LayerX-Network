//! Emergency exit against the native LayerX custody precompile.
//!
//! The chain answers come from an in-process JSON-RPC endpoint in the crate's
//! established harness style. Every proof the journey consumes is real: the
//! account state witness is built and encoded with `layerx_proof`, the anchor
//! is the witness's own state root, and the recipient authorization is a real
//! ed25519 signature over `exit_recipient_message`. The claim identifiers the
//! precompile reports back are derived independently with the published custody
//! helpers, so the harness never echoes the journey's own numbers.

use std::collections::BTreeMap;
use std::fs;
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::{Arc, Mutex, MutexGuard};
use std::thread;
use std::time::Duration;

use ed25519_dalek::{Signer as _, SigningKey};
use serde_json::{json, Value};
use sha2::{Digest as _, Sha256};

use layerx_human_service::audit::{
    verify_export, AuditChain, AuditEvent, JourneyKind, JourneyState,
};
use layerx_human_service::journeys::{
    ExitBoundaryError, ExitJourney, ExitJourneyError, ExitPlan, ExitStage, ExitWallet,
    ExitWalletOutcome, ExitWalletRequest, IrreversibleExitConfirmation, EXIT_CONFIRMATION_PHRASE,
    EXIT_IRREVERSIBILITY_NOTICE, EXIT_NORMAL_OPERATION_MESSAGE, EXIT_SETTINGS_SURFACE, EXIT_TITLE,
    ORDINARY_WITHDRAWAL_PATH,
};
use layerx_human_service::notify::JourneyId;
use layerx_human_service::store::{PrincipalId, PrincipalStore};
use layerx_human_service::trace::TraceId;
use layerx_paxeer_client::custody::{
    exit_claim_id, exit_recipient_message, exit_withdrawal_id, withdrawal_nullifier,
    CLAIM_QUEUED_TOPIC, EMERGENCY_EXIT_EXECUTED_TOPIC, SELECTOR_EXECUTE_FORCED_EXIT,
    SELECTOR_EXIT_ELIGIBLE, SELECTOR_GET_CLAIM, SELECTOR_NULLIFIER_STATUS,
    SELECTOR_REQUEST_FORCED_EXIT,
};
use layerx_paxeer_client::state_proof::{AccountPath, StateWitness};
use layerx_paxeer_client::{
    EmergencyExit, EndpointConfig, EndpointTransport, ExitConfig, ExitError, ExitEvidence,
    ForcedExitMaterial, TransactionHash, ANCHOR_PRECOMPILE, CUSTODY_PRECOMPILE,
};
use layerx_types::intent::EvmAddress;

use layerx_human_test_support as support;
use support::{directory, install_and_open, principal, retention_uniform, tenancy};

const CHAIN_ID: u64 = 31_337;
const NETWORK_ID: u32 = 7332;
const BALANCE: u128 = 5_000_000;
const BATCH: u64 = 21;
const REQUIRED_CONFIRMATIONS: u64 = 2;
const ACCOUNT_NAME: &[u8] = b"agent:exit-holder:main";
const ASSET: [u8; 32] = [0x24; 32];
const RECIPIENT: [u8; 20] = [0x42; 20];
const DENOM: &str = "ulxp";
const AVAILABLE_AT: u64 = 300;
const FIRST_HEAD: u64 = 10;
const WORD: usize = 32;
/// `latestFinalized()` on the anchor precompile.
const SELECTOR_LATEST_FINALIZED: [u8; 4] = [0x6c, 0xdd, 0x45, 0xae];
/// `finalizedStateRoot(uint64)` on the anchor precompile.
const SELECTOR_FINALIZED_STATE_ROOT: [u8; 4] = [0x0f, 0x60, 0x7f, 0xe4];

// ---------------------------------------------------------------------------
// Real forced-exit material
// ---------------------------------------------------------------------------

fn state_leaf(key: &[u8], value: &[u8]) -> [u8; 32] {
    let mut hasher = Sha256::new();
    hasher.update(b"LXP/v1/state-leaf\0");
    hasher.update(u32::try_from(key.len()).unwrap_or(u32::MAX).to_be_bytes());
    hasher.update(u32::try_from(value.len()).unwrap_or(u32::MAX).to_be_bytes());
    hasher.update(key);
    hasher.update(value);
    hasher.finalize().into()
}

/// The exact native account value the custody module's balance verifier reads.
fn account_value(authority: [u8; 32]) -> ([u8; 32], Vec<u8>) {
    let mut hasher = Sha256::new();
    hasher.update(b"LX:ACCOUNT:v1");
    hasher.update(
        u32::try_from(ACCOUNT_NAME.len())
            .unwrap_or(u32::MAX)
            .to_be_bytes(),
    );
    hasher.update(ACCOUNT_NAME);
    let account_id: [u8; 32] = hasher.finalize().into();
    let mut value = u16::try_from(ACCOUNT_NAME.len())
        .unwrap_or(u16::MAX)
        .to_be_bytes()
        .to_vec();
    value.extend_from_slice(ACCOUNT_NAME);
    value.push(1);
    value.extend_from_slice(&BALANCE.to_be_bytes());
    value.extend_from_slice(&ASSET);
    value.push(1);
    value.extend_from_slice(&9_u64.to_be_bytes());
    value.extend_from_slice(&2_u64.to_be_bytes());
    value.extend_from_slice(&[0, 0]);
    value.extend_from_slice(&authority);
    value.push(1);
    layerx_proof::state::decode_account_value(account_id, &value)
        .unwrap_or_else(|error| panic!("account value: {error:?}"));
    (account_id, value)
}

#[derive(Clone, Debug)]
struct Exit {
    evidence: ExitEvidence,
    account: [u8; 32],
    state_root: [u8; 32],
    withdrawal_id: [u8; 32],
    nullifier: [u8; 32],
    claim_id: [u8; 32],
}

fn forced_exit() -> Exit {
    let authority = SigningKey::from_bytes(&[0x61; 32]);
    let (account, value) = account_value(authority.verifying_key().to_bytes());
    let mut key = vec![4_u8];
    key.extend_from_slice(&account);
    let witness = StateWitness {
        module_id: 0,
        key,
        value,
        account_path: Some(AccountPath {
            index: 0,
            count: 2,
            siblings: vec![state_leaf(&[4; 33], b"neighbour")],
        }),
        leaf_index_a: 0,
        leaf_count_a: 2,
        siblings_a: vec![state_leaf(b"sequence", &BATCH.to_be_bytes())],
        leaf_count_b: 10,
        siblings_b: vec![[0xd1; 32], [0xd2; 32], [0xd3; 32], [0xd4; 32]],
    };
    let state_root = witness
        .root()
        .unwrap_or_else(|error| panic!("witness root: {error:?}"));
    let encoded = witness
        .encode()
        .unwrap_or_else(|error| panic!("witness encode: {error:?}"));
    let recipient = EvmAddress::new(RECIPIENT);
    let message = exit_recipient_message(NETWORK_ID, &account, &ASSET, recipient, &state_root);
    let recipient_signature = authority.sign(&message).to_bytes();
    let evidence = ExitEvidence {
        material: ForcedExitMaterial {
            witness: encoded,
            batch_number: BATCH,
            account,
            asset_id: ASSET,
            recipient,
            recipient_signature,
        },
        finalised_balance: BALANCE,
    };
    layerx_paxeer_client::verify_exit_balance(&evidence, NETWORK_ID, state_root)
        .unwrap_or_else(|error| panic!("exit balance: {error:?}"));

    let withdrawal_id = exit_withdrawal_id(NETWORK_ID, &account, &ASSET, &state_root);
    let nullifier = withdrawal_nullifier(
        NETWORK_ID,
        &withdrawal_id,
        &account,
        &ASSET,
        BALANCE,
        &state_root,
    );
    Exit {
        evidence,
        account,
        state_root,
        withdrawal_id,
        nullifier,
        claim_id: exit_claim_id(CHAIN_ID, nullifier),
    }
}

// ---------------------------------------------------------------------------
// In-process chain
// ---------------------------------------------------------------------------

/// Which exit event the execute transaction emits.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum ExitEvent {
    /// The precompile's real `EmergencyExitExecuted` log.
    Bound,
    /// A succeeding execute transaction that emitted no exit event at all.
    Absent,
    /// An exit event that pays some other recipient.
    Foreign,
}

#[derive(Clone, Debug)]
struct Log {
    address: [u8; 20],
    topics: Vec<[u8; 32]>,
    data: Vec<u8>,
}

#[derive(Clone, Debug)]
struct Receipt {
    block: u64,
    index: u64,
    status: u64,
    logs: Vec<Log>,
}

struct Chain {
    exit: Exit,
    head: u64,
    block_time: u64,
    hashes: BTreeMap<u64, [u8; 32]>,
    receipts: BTreeMap<[u8; 32], Receipt>,
    eligible: bool,
    nullifier_status: u8,
    claim: Option<(u8, u64)>,
    event: ExitEvent,
    sequence: u64,
}

impl Chain {
    fn new(exit: Exit, event: ExitEvent) -> Arc<Mutex<Self>> {
        let mut hashes = BTreeMap::new();
        for number in 0..=FIRST_HEAD {
            hashes.insert(number, block_hash(number));
        }
        Arc::new(Mutex::new(Self {
            exit,
            head: FIRST_HEAD,
            block_time: 0,
            hashes,
            receipts: BTreeMap::new(),
            eligible: false,
            nullifier_status: 0,
            claim: None,
            event,
            sequence: 0,
        }))
    }

    fn mine(&mut self) {
        self.head = self.head.saturating_add(1);
        self.hashes.insert(self.head, block_hash(self.head));
    }

    /// Applies one user transaction exactly as the custody precompile would:
    /// `requestForcedExit` only queues the claim and starts its delay, and
    /// `executeForcedExit` pays a queued claim once that delay elapsed.
    fn submit(&mut self, calldata: &[u8]) -> TransactionHash {
        self.sequence = self.sequence.saturating_add(1);
        let mut bytes = [0_u8; 32];
        bytes[..8].copy_from_slice(&self.sequence.to_be_bytes());
        bytes[31] = 0xa7;
        let transaction = TransactionHash::new(bytes);
        let mut logs = Vec::new();
        let mut status = 1;
        if calldata.starts_with(&SELECTOR_REQUEST_FORCED_EXIT) {
            if self.eligible && self.claim.is_none() {
                self.claim = Some((1, AVAILABLE_AT));
                self.nullifier_status = 1;
                logs.push(self.queued_log());
            } else {
                status = 0;
            }
        } else if calldata.starts_with(&SELECTOR_EXECUTE_FORCED_EXIT) {
            match self.claim {
                Some((1, available_at)) if self.block_time >= available_at => {
                    self.claim = Some((2, available_at));
                    self.nullifier_status = 2;
                    logs.extend(self.executed_log());
                }
                _ => status = 0,
            }
        } else {
            status = 0;
        }
        self.receipts.insert(
            bytes,
            Receipt {
                block: self.head,
                index: 0,
                status,
                logs,
            },
        );
        transaction
    }

    fn queued_log(&self) -> Log {
        let mut data = ASSET.to_vec();
        data.extend_from_slice(&address_word(RECIPIENT));
        data.extend_from_slice(&u128_word(BALANCE));
        data.extend_from_slice(&u64_word(AVAILABLE_AT));
        Log {
            address: CUSTODY_PRECOMPILE.bytes(),
            topics: vec![
                CLAIM_QUEUED_TOPIC,
                self.exit.claim_id,
                self.exit.nullifier,
                self.exit.state_root,
            ],
            data,
        }
    }

    fn executed_log(&self) -> Option<Log> {
        let recipient = match self.event {
            ExitEvent::Bound => RECIPIENT,
            ExitEvent::Absent => return None,
            ExitEvent::Foreign => [0x43; 20],
        };
        let mut data = self.exit.account.to_vec();
        data.extend_from_slice(&ASSET);
        data.extend_from_slice(&address_word(recipient));
        data.extend_from_slice(&u128_word(BALANCE));
        Some(Log {
            address: CUSTODY_PRECOMPILE.bytes(),
            topics: vec![
                EMERGENCY_EXIT_EXECUTED_TOPIC,
                self.exit.claim_id,
                self.exit.nullifier,
                self.exit.state_root,
            ],
            data,
        })
    }

    /// `ILayerXCustody.Claim` exactly as `getClaim(bytes32)` returns it.
    fn claim_tuple(&self) -> Vec<u8> {
        let mut out = u64_word(u64::try_from(WORD).unwrap_or_default()).to_vec();
        let Some((status, available_at)) = self.claim else {
            out.extend_from_slice(&[0_u8; 14 * WORD]);
            return out;
        };
        let body = [
            self.exit.claim_id,
            u64_word(2),
            u64_word(u64::from(status)),
            self.exit.nullifier,
            self.exit.withdrawal_id,
            self.exit.account,
            ASSET,
            u64_word(u64::try_from(13 * WORD).unwrap_or_default()),
            address_word(RECIPIENT),
            u128_word(BALANCE),
            u64_word(BATCH),
            self.exit.state_root,
            u64_word(available_at),
        ];
        for word in body {
            out.extend_from_slice(&word);
        }
        out.extend_from_slice(&u64_word(u64::try_from(DENOM.len()).unwrap_or_default()));
        let mut tail = DENOM.as_bytes().to_vec();
        tail.resize(WORD, 0);
        out.extend_from_slice(&tail);
        out
    }

    fn view(&self, to: &[u8], data: &[u8]) -> Vec<u8> {
        let selector = data.get(..4).unwrap_or_default();
        if to == CUSTODY_PRECOMPILE.bytes().as_slice() {
            if selector == SELECTOR_EXIT_ELIGIBLE {
                return u64_word(u64::from(self.eligible)).to_vec();
            }
            if selector == SELECTOR_NULLIFIER_STATUS {
                return u64_word(u64::from(self.nullifier_status)).to_vec();
            }
            if selector == SELECTOR_GET_CLAIM {
                return self.claim_tuple();
            }
        } else if to == ANCHOR_PRECOMPILE.bytes().as_slice() {
            if selector == SELECTOR_LATEST_FINALIZED {
                let mut out = u64_word(BATCH).to_vec();
                out.extend_from_slice(&u64_word(1));
                return out;
            }
            if selector == SELECTOR_FINALIZED_STATE_ROOT {
                let mut requested = [0_u8; 8];
                requested.copy_from_slice(data.get(4 + 24..4 + WORD).unwrap_or(&[0; 8]));
                if u64::from_be_bytes(requested) == BATCH {
                    let mut out = self.exit.state_root.to_vec();
                    out.extend_from_slice(&u64_word(1));
                    return out;
                }
                let mut out = vec![0_u8; WORD];
                out.extend_from_slice(&u64_word(0));
                return out;
            }
        }
        Vec::new()
    }
}

fn block_hash(number: u64) -> [u8; 32] {
    let mut hasher = Sha256::new();
    hasher.update(b"layerx-human-exit-block\0");
    hasher.update(number.to_be_bytes());
    hasher.finalize().into()
}

fn u64_word(value: u64) -> [u8; 32] {
    let mut word = [0_u8; 32];
    word[24..].copy_from_slice(&value.to_be_bytes());
    word
}

fn u128_word(value: u128) -> [u8; 32] {
    let mut word = [0_u8; 32];
    word[16..].copy_from_slice(&value.to_be_bytes());
    word
}

fn address_word(value: [u8; 20]) -> [u8; 32] {
    let mut word = [0_u8; 32];
    word[12..].copy_from_slice(&value);
    word
}

fn lock(chain: &Arc<Mutex<Chain>>) -> MutexGuard<'_, Chain> {
    chain
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

// ---------------------------------------------------------------------------
// In-process JSON-RPC endpoint
// ---------------------------------------------------------------------------

fn read_request_body(stream: &mut TcpStream) -> Option<Vec<u8>> {
    let mut buffer = Vec::new();
    let mut byte = [0_u8; 1];
    let mut expected = None;
    loop {
        match stream.read(&mut byte) {
            Ok(1) => buffer.push(byte[0]),
            _ => return None,
        }
        if expected.is_none() && buffer.ends_with(b"\r\n\r\n") {
            let head = String::from_utf8_lossy(&buffer).to_ascii_lowercase();
            let length: usize = head
                .split("content-length:")
                .nth(1)
                .and_then(|rest| rest.split("\r\n").next())
                .and_then(|value| value.trim().parse().ok())?;
            expected = Some(buffer.len().saturating_add(length));
        }
        if expected.is_some_and(|total| buffer.len() >= total) {
            break;
        }
    }
    let body = buffer
        .windows(4)
        .position(|window| window == b"\r\n\r\n")?
        .saturating_add(4);
    buffer.get(body..).map(<[u8]>::to_vec)
}

fn serve(listener: TcpListener, chain: Arc<Mutex<Chain>>) {
    thread::spawn(move || loop {
        let Ok((mut stream, _)) = listener.accept() else {
            return;
        };
        let Some(body) = read_request_body(&mut stream) else {
            continue;
        };
        let Ok(request) = serde_json::from_slice::<Value>(&body) else {
            continue;
        };
        let payload = json!({
            "jsonrpc": "2.0",
            "id": 1,
            "result": answer(&chain, &request),
        })
        .to_string();
        let head = format!(
            "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
            payload.len()
        );
        let _ = stream
            .write_all(head.as_bytes())
            .and_then(|()| stream.write_all(payload.as_bytes()))
            .and_then(|()| stream.flush());
    });
}

fn answer(chain: &Arc<Mutex<Chain>>, request: &Value) -> Value {
    let method = request["method"].as_str().unwrap_or_default();
    let params = &request["params"];
    let chain = lock(chain);
    match method {
        "eth_chainId" => json!(quantity(CHAIN_ID)),
        "eth_blockNumber" => json!(quantity(chain.head)),
        "eth_getBlockByNumber" => {
            let number = hex_quantity(&params[0]);
            chain.hashes.get(&number).map_or(
                Value::Null,
                |hash| json!({ "number": quantity(number), "hash": hex(hash) }),
            )
        }
        "eth_getTransactionReceipt" => {
            let mut requested = [0_u8; 32];
            let bytes = hex_bytes(&params[0]);
            if bytes.len() != 32 {
                return Value::Null;
            }
            requested.copy_from_slice(&bytes);
            chain
                .receipts
                .get(&requested)
                .map_or(Value::Null, |receipt| {
                    let block_hash = chain
                        .hashes
                        .get(&receipt.block)
                        .copied()
                        .unwrap_or_default();
                    receipt_json(requested, receipt, block_hash)
                })
        }
        "eth_getTransactionByHash" => Value::Null,
        "eth_call" => json!(hex(
            &chain.view(&hex_bytes(&params[0]["to"]), &hex_bytes(&params[0]["data"]))
        )),
        _ => Value::Null,
    }
}

fn receipt_json(transaction: [u8; 32], receipt: &Receipt, block_hash: [u8; 32]) -> Value {
    let logs = receipt
        .logs
        .iter()
        .map(|log| {
            json!({
                "address": hex(&log.address),
                "topics": log.topics.iter().map(|topic| hex(topic)).collect::<Vec<_>>(),
                "data": hex(&log.data),
                "transactionHash": hex(&transaction),
                "blockHash": hex(&block_hash),
                "blockNumber": quantity(receipt.block),
                "transactionIndex": quantity(receipt.index),
                "removed": false,
            })
        })
        .collect::<Vec<_>>();
    json!({
        "transactionHash": hex(&transaction),
        "blockNumber": quantity(receipt.block),
        "blockHash": hex(&block_hash),
        "transactionIndex": quantity(receipt.index),
        "status": quantity(receipt.status),
        "contractAddress": Value::Null,
        "logs": logs,
    })
}

fn quantity(value: u64) -> String {
    format!("0x{value:x}")
}

fn hex(bytes: &[u8]) -> String {
    let mut text = String::from("0x");
    for byte in bytes {
        text.push_str(&format!("{byte:02x}"));
    }
    text
}

fn hex_quantity(value: &Value) -> u64 {
    value
        .as_str()
        .and_then(|text| text.strip_prefix("0x"))
        .and_then(|digits| u64::from_str_radix(digits, 16).ok())
        .unwrap_or_default()
}

fn hex_bytes(value: &Value) -> Vec<u8> {
    let Some(digits) = value.as_str().and_then(|text| text.strip_prefix("0x")) else {
        return Vec::new();
    };
    digits
        .as_bytes()
        .chunks_exact(2)
        .filter_map(|pair| {
            std::str::from_utf8(pair)
                .ok()
                .and_then(|text| u8::from_str_radix(text, 16).ok())
        })
        .collect()
}

// ---------------------------------------------------------------------------
// Wallet boundary
// ---------------------------------------------------------------------------

struct StubWallet {
    chain: Arc<Mutex<Chain>>,
    opened: BTreeMap<[u8; 32], (ExitWalletRequest, TransactionHash)>,
    drop_first_acknowledgement: bool,
    submissions: u64,
}

impl StubWallet {
    fn new(chain: &Arc<Mutex<Chain>>, drop_first_acknowledgement: bool) -> Self {
        Self {
            chain: Arc::clone(chain),
            opened: BTreeMap::new(),
            drop_first_acknowledgement,
            submissions: 0,
        }
    }

    const fn submissions(&self) -> u64 {
        self.submissions
    }
}

impl ExitWallet for StubWallet {
    fn submit_or_resolve(
        &mut self,
        request: &ExitWalletRequest,
    ) -> Result<ExitWalletOutcome, ExitBoundaryError> {
        if let Some((original, transaction)) = self.opened.get(&request.action_key) {
            if original != request {
                return Err(ExitBoundaryError::ContractViolation);
            }
            return Ok(ExitWalletOutcome::Submitted(*transaction));
        }
        if request.contract != CUSTODY_PRECOMPILE || request.calldata.is_empty() {
            return Err(ExitBoundaryError::ContractViolation);
        }
        let transaction = lock(&self.chain).submit(&request.calldata);
        self.opened
            .insert(request.action_key, (request.clone(), transaction));
        self.submissions = self.submissions.saturating_add(1);
        if self.drop_first_acknowledgement {
            self.drop_first_acknowledgement = false;
            Err(ExitBoundaryError::Unavailable)
        } else {
            Ok(ExitWalletOutcome::Submitted(transaction))
        }
    }
}

// ---------------------------------------------------------------------------
// Harness
// ---------------------------------------------------------------------------

fn launch(event: ExitEvent) -> (Arc<Mutex<Chain>>, EmergencyExit, Exit) {
    let exit = forced_exit();
    let chain = Chain::new(exit.clone(), event);
    let listener = TcpListener::bind("127.0.0.1:0").unwrap_or_else(|error| panic!("bind: {error}"));
    let address = listener
        .local_addr()
        .unwrap_or_else(|error| panic!("address: {error}"));
    serve(listener, Arc::clone(&chain));
    let client = EmergencyExit::new(ExitConfig {
        endpoints: vec![EndpointConfig {
            url: format!("http://{address}"),
            request_timeout: Duration::from_secs(5),
            transport: EndpointTransport::LocalEmulator,
            expected_chain_id: CHAIN_ID,
        }],
        minimum_endpoint_agreement: 1,
        network_id: NETWORK_ID,
        required_confirmations: REQUIRED_CONFIRMATIONS,
        poll_cadence: Duration::from_millis(1),
        delayed_after_polls: 8,
    })
    .unwrap_or_else(|error| panic!("exit client: {error:?}"));
    (chain, client, exit)
}

fn plan(journey: &str, idempotency_key: [u8; 32], evidence: ExitEvidence) -> ExitPlan {
    ExitPlan {
        journey_id: JourneyId::new(journey).unwrap_or_else(|error| panic!("journey id: {error}")),
        idempotency_key,
        evidence,
    }
}

/// Drives one exit from the emergency declaration to the point where the
/// `executeForcedExit` transaction is one confirmation short of final.
fn drive_to_execute_confirming(
    store: &mut PrincipalStore,
    owner: &PrincipalId,
    trace: &TraceId,
    client: &EmergencyExit,
    wallet: &mut StubWallet,
    chain: &Arc<Mutex<Chain>>,
    plan: &ExitPlan,
    confirmation: IrreversibleExitConfirmation,
) {
    lock(chain).eligible = true;
    let mut scope = store
        .principal(owner)
        .unwrap_or_else(|error| panic!("scope: {error}"));
    let mut audit = AuditChain::open(&scope).unwrap_or_else(|error| panic!("audit: {error}"));
    let mut journey = ExitJourney::start(&mut scope, &mut audit, trace, plan, confirmation, 200)
        .unwrap_or_else(|error| panic!("start: {error}"));
    let mut step = |journey: &mut ExitJourney, now: u64, what: &str| -> ExitStage {
        lock(chain).block_time = now;
        journey
            .advance(&mut scope, &mut audit, trace, client, wallet, now)
            .unwrap_or_else(|error| panic!("{what}: {error}"))
            .stage()
            .clone()
    };
    assert_eq!(
        step(&mut journey, 201, "construct"),
        ExitStage::WaitingForWallet
    );
    assert!(matches!(
        step(&mut journey, 202, "request submission"),
        ExitStage::ConfirmingPaxeer { .. }
    ));
    assert!(matches!(
        step(&mut journey, 203, "request confirmation"),
        ExitStage::ConfirmingPaxeer {
            confirmations: 1,
            required: REQUIRED_CONFIRMATIONS,
            ..
        }
    ));
    lock(chain).mine();
    assert!(matches!(
        step(&mut journey, 204, "queued exit"),
        ExitStage::WaitingForForcedExitDelay {
            available_at: AVAILABLE_AT,
            ..
        }
    ));
    assert!(matches!(
        step(&mut journey, 205, "waiting for the forced-exit delay"),
        ExitStage::WaitingForForcedExitDelay { .. }
    ));
    assert_eq!(
        step(&mut journey, AVAILABLE_AT, "delay elapsed"),
        ExitStage::WaitingForWallet
    );
    assert!(matches!(
        step(&mut journey, 301, "execute submission"),
        ExitStage::ConfirmingPaxeer { .. }
    ));
    lock(chain).mine();
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[test]
fn typed_confirmation_and_guidance_are_exact() {
    assert!(IrreversibleExitConfirmation::parse("get my money out").is_err());
    assert!(IrreversibleExitConfirmation::parse("GET MY MONEY OUT ").is_err());
    let confirmation = IrreversibleExitConfirmation::parse(EXIT_CONFIRMATION_PHRASE)
        .unwrap_or_else(|error| panic!("confirmation: {error:?}"));
    assert_ne!(confirmation.digest(), [0; 32]);
    assert_eq!(
        layerx_human_service::journeys::ExitStatus::settings_surface(),
        EXIT_SETTINGS_SURFACE
    );
    assert_eq!(
        layerx_human_service::journeys::ExitStatus::title(),
        EXIT_TITLE
    );
    assert_eq!(
        layerx_human_service::journeys::ExitStatus::irreversibility_notice(),
        EXIT_IRREVERSIBILITY_NOTICE
    );
    assert_eq!(EXIT_NORMAL_OPERATION_MESSAGE, "Emergency exit is unavailable because the network is operating normally. Use ordinary withdrawal instead.");
    assert_eq!(ORDINARY_WITHDRAWAL_PATH, "/app/withdraw");
}

#[test]
#[allow(clippy::too_many_lines)]
fn degraded_core_ack_gap_and_restarts_converge_on_one_finalised_exit() {
    let (chain, client, exit) = launch(ExitEvent::Bound);
    let root = directory("exit-restart");
    let map = tenancy(&[("alice", "tenant-alpha"), ("bob", "tenant-beta")]);
    let retention = retention_uniform(50_000);
    let (mut store, digest) = install_and_open(&root, &map, retention);
    let owner = principal("alice");
    let trace = TraceId::mint([0x61; 16]);
    let confirmation = IrreversibleExitConfirmation::parse(EXIT_CONFIRMATION_PHRASE)
        .unwrap_or_else(|error| panic!("confirmation: {error:?}"));

    // While the network is operating normally the precompile refuses the exit
    // and the journey sends the owner to ordinary withdrawal instead.
    let normal_plan = plan("jrn_exitnormal0001", [0x31; 32], exit.evidence.clone());
    {
        let mut wallet = StubWallet::new(&chain, false);
        let mut scope = store
            .principal(&owner)
            .unwrap_or_else(|error| panic!("normal scope: {error}"));
        let mut audit =
            AuditChain::open(&scope).unwrap_or_else(|error| panic!("normal audit: {error}"));
        let mut journey = ExitJourney::start(
            &mut scope,
            &mut audit,
            &trace,
            &normal_plan,
            confirmation,
            100,
        )
        .unwrap_or_else(|error| panic!("normal start: {error}"));
        let status = journey
            .advance(&mut scope, &mut audit, &trace, &client, &mut wallet, 101)
            .unwrap_or_else(|error| panic!("normal advance: {error}"));
        assert_eq!(
            status.normal_operation_message(),
            Some(EXIT_NORMAL_OPERATION_MESSAGE)
        );
        assert_eq!(
            status.stage(),
            &ExitStage::UnavailableWhileNetworkOperatingNormally {
                ordinary_withdrawal_path: ORDINARY_WITHDRAWAL_PATH,
            }
        );
        assert_eq!(wallet.submissions(), 0);
        assert!(lock(&chain).claim.is_none());
    }

    lock(&chain).eligible = true;
    let mut wallet = StubWallet::new(&chain, true);
    let plan = plan("jrn_exitrestart0001", [0x41; 32], exit.evidence.clone());

    // The wallet acknowledgement for the request transaction is lost.
    {
        let mut scope = store
            .principal(&owner)
            .unwrap_or_else(|error| panic!("scope: {error}"));
        let mut audit = AuditChain::open(&scope).unwrap_or_else(|error| panic!("audit: {error}"));
        let mut journey =
            ExitJourney::start(&mut scope, &mut audit, &trace, &plan, confirmation, 200)
                .unwrap_or_else(|error| panic!("start: {error}"));
        assert_eq!(
            journey
                .advance(&mut scope, &mut audit, &trace, &client, &mut wallet, 201)
                .unwrap_or_else(|error| panic!("construct: {error}"))
                .stage(),
            &ExitStage::WaitingForWallet
        );
        assert!(matches!(
            journey.advance(&mut scope, &mut audit, &trace, &client, &mut wallet, 202),
            Err(ExitJourneyError::Boundary(ExitBoundaryError::Unavailable))
        ));
        assert_eq!(
            journey
                .status()
                .unwrap_or_else(|error| panic!("status: {error}"))
                .stage(),
            &ExitStage::WaitingForWallet
        );
        assert_eq!(wallet.submissions(), 1);
    }

    // After a restart the same wallet action resolves to the same transaction.
    drop(store);
    let mut store = PrincipalStore::open(&root, retention, digest)
        .unwrap_or_else(|error| panic!("restart store: {error}"));
    {
        let mut scope = store
            .principal(&owner)
            .unwrap_or_else(|error| panic!("restart scope: {error}"));
        let mut audit =
            AuditChain::open(&scope).unwrap_or_else(|error| panic!("restart audit: {error}"));
        let mut journey = ExitJourney::load(&scope, &plan.journey_id)
            .unwrap_or_else(|error| panic!("load: {error}"))
            .unwrap_or_else(|| panic!("exit missing after restart"));
        assert!(matches!(
            journey
                .advance(&mut scope, &mut audit, &trace, &client, &mut wallet, 203)
                .unwrap_or_else(|error| panic!("resolve wallet action: {error}"))
                .stage(),
            ExitStage::ConfirmingPaxeer { .. }
        ));
        assert_eq!(wallet.submissions(), 1);
        assert!(matches!(
            journey
                .advance(&mut scope, &mut audit, &trace, &client, &mut wallet, 204)
                .unwrap_or_else(|error| panic!("first finality poll: {error}"))
                .stage(),
            ExitStage::ConfirmingPaxeer {
                confirmations: 1,
                required: REQUIRED_CONFIRMATIONS,
                ..
            }
        ));
    }

    // The request transaction becomes final: the precompile has queued the
    // exit, and only the forced-exit delay stands between it and payment.
    drop(store);
    lock(&chain).mine();
    let mut store = PrincipalStore::open(&root, retention, digest)
        .unwrap_or_else(|error| panic!("second restart store: {error}"));
    let mut scope = store
        .principal(&owner)
        .unwrap_or_else(|error| panic!("second restart scope: {error}"));
    let mut audit =
        AuditChain::open(&scope).unwrap_or_else(|error| panic!("second audit: {error}"));
    let mut journey = ExitJourney::load(&scope, &plan.journey_id)
        .unwrap_or_else(|error| panic!("second load: {error}"))
        .unwrap_or_else(|| panic!("exit missing after second restart"));
    assert_eq!(
        journey
            .advance(&mut scope, &mut audit, &trace, &client, &mut wallet, 205)
            .unwrap_or_else(|error| panic!("queued exit: {error}"))
            .stage(),
        &ExitStage::WaitingForForcedExitDelay {
            claim_id: exit.claim_id,
            available_at: AVAILABLE_AT,
        }
    );
    assert_eq!(lock(&chain).claim, Some((1, AVAILABLE_AT)));
    assert_eq!(wallet.submissions(), 1);

    // Executing before the delay elapsed is never attempted.
    assert!(matches!(
        journey
            .advance(&mut scope, &mut audit, &trace, &client, &mut wallet, 206)
            .unwrap_or_else(|error| panic!("delay still running: {error}"))
            .stage(),
        ExitStage::WaitingForForcedExitDelay { .. }
    ));
    assert_eq!(wallet.submissions(), 1);
    assert_eq!(lock(&chain).claim, Some((1, AVAILABLE_AT)));

    lock(&chain).block_time = AVAILABLE_AT;
    assert_eq!(
        journey
            .advance(
                &mut scope,
                &mut audit,
                &trace,
                &client,
                &mut wallet,
                AVAILABLE_AT
            )
            .unwrap_or_else(|error| panic!("delay elapsed: {error}"))
            .stage(),
        &ExitStage::WaitingForWallet
    );
    lock(&chain).block_time = 301;
    assert!(matches!(
        journey
            .advance(&mut scope, &mut audit, &trace, &client, &mut wallet, 301)
            .unwrap_or_else(|error| panic!("execute submission: {error}"))
            .stage(),
        ExitStage::ConfirmingPaxeer { .. }
    ));
    assert_eq!(wallet.submissions(), 2);
    assert_eq!(lock(&chain).claim, Some((2, AVAILABLE_AT)));

    assert!(matches!(
        journey
            .advance(&mut scope, &mut audit, &trace, &client, &mut wallet, 302)
            .unwrap_or_else(|error| panic!("execute confirmation: {error}"))
            .stage(),
        ExitStage::ConfirmingPaxeer {
            confirmations: 1,
            required: REQUIRED_CONFIRMATIONS,
            ..
        }
    ));

    lock(&chain).mine();
    let status = journey
        .advance(&mut scope, &mut audit, &trace, &client, &mut wallet, 303)
        .unwrap_or_else(|error| panic!("finality: {error}"));
    let ExitStage::Done(finality) = status.stage() else {
        panic!("exit was not final: {:?}", status.stage());
    };
    assert_eq!(finality.confirmations, REQUIRED_CONFIRMATIONS);
    assert_ne!(finality.block_hash, [0; 32]);
    assert_eq!(wallet.submissions(), 2);

    // The terminal state replays without moving money a second time.
    let terminal = journey
        .advance(&mut scope, &mut audit, &trace, &client, &mut wallet, 304)
        .unwrap_or_else(|error| panic!("terminal replay: {error}"));
    assert_eq!(terminal, status);
    assert_eq!(wallet.submissions(), 2);
    assert_eq!(lock(&chain).claim, Some((2, AVAILABLE_AT)));

    let entries = audit
        .entries(&scope)
        .unwrap_or_else(|error| panic!("audit entries: {error}"));
    assert!(entries.iter().any(|entry| matches!(
        entry.event(),
        AuditEvent::JourneyTransition {
            kind: JourneyKind::Exit,
            to: JourneyState::DoneFinalised,
            ..
        }
    ) && !entry.evidence().is_empty()));
    assert!(entries.iter().any(|entry| matches!(
        entry.event(),
        AuditEvent::JourneyTransition {
            kind: JourneyKind::Exit,
            to: JourneyState::StillChecking,
            ..
        }
    )));
    let bundle = audit
        .export(&scope)
        .unwrap_or_else(|error| panic!("audit export: {error}"));
    let report = verify_export(&bundle).unwrap_or_else(|error| panic!("verify export: {error}"));
    assert!(report.entries() >= 6);
    assert!(report.evidence_rows() >= 4);

    drop(scope);
    let bob = principal("bob");
    let bob_scope = store
        .principal(&bob)
        .unwrap_or_else(|error| panic!("bob scope: {error}"));
    assert!(
        ExitJourney::load(&bob_scope, &plan.journey_id)
            .unwrap_or_else(|error| panic!("bob load: {error}"))
            .is_none(),
        "the exit journey must remain owned by its principal"
    );
    drop(bob_scope);
    drop(store);
    fs::remove_dir_all(&root).unwrap_or_else(|error| panic!("cleanup: {error}"));
}

/// The precompile's `EmergencyExitExecuted` event is what binds a final
/// execute transaction to this exit. Without it — or with one that pays
/// someone else — the journey never reports the money as out.
#[test]
fn settlement_requires_the_precompile_exit_event() {
    for (label, event, expected) in [
        ("absent", ExitEvent::Absent, ExitError::MissingEvent),
        ("foreign", ExitEvent::Foreign, ExitError::EventMismatch),
    ] {
        let (chain, client, exit) = launch(event);
        let root = directory(&format!("exit-event-{label}"));
        let map = tenancy(&[("alice", "tenant-alpha")]);
        let retention = retention_uniform(50_000);
        let (mut store, _) = install_and_open(&root, &map, retention);
        let owner = principal("alice");
        let trace = TraceId::mint([0x62; 16]);
        let confirmation = IrreversibleExitConfirmation::parse(EXIT_CONFIRMATION_PHRASE)
            .unwrap_or_else(|error| panic!("confirmation: {error:?}"));
        let plan = plan("jrn_exitevent00001", [0x51; 32], exit.evidence.clone());
        let mut wallet = StubWallet::new(&chain, false);
        drive_to_execute_confirming(
            &mut store,
            &owner,
            &trace,
            &client,
            &mut wallet,
            &chain,
            &plan,
            confirmation,
        );

        let mut scope = store
            .principal(&owner)
            .unwrap_or_else(|error| panic!("{label} scope: {error}"));
        let mut audit =
            AuditChain::open(&scope).unwrap_or_else(|error| panic!("{label} audit: {error}"));
        let mut journey = ExitJourney::load(&scope, &plan.journey_id)
            .unwrap_or_else(|error| panic!("{label} load: {error}"))
            .unwrap_or_else(|| panic!("{label}: exit missing"));
        lock(&chain).block_time = 302;
        let outcome = journey.advance(&mut scope, &mut audit, &trace, &client, &mut wallet, 302);
        match outcome {
            Err(ExitJourneyError::Paxeer(error)) => assert_eq!(error, expected),
            other => panic!("{label}: unbound exit event became {other:?}"),
        }
        assert!(
            matches!(
                journey
                    .status()
                    .unwrap_or_else(|error| panic!("{label} status: {error}"))
                    .stage(),
                ExitStage::ConfirmingPaxeer { .. }
            ),
            "{label}: the exit must not settle without its precompile event"
        );

        drop(scope);
        drop(store);
        fs::remove_dir_all(&root).unwrap_or_else(|error| panic!("{label} cleanup: {error}"));
    }
}
