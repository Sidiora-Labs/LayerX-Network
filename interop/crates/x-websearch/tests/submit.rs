use std::error::Error;
use std::io::{Read as _, Write as _};
use std::net::{SocketAddr, TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, PoisonError};
use std::thread;

use k256::ecdsa::SigningKey;
use serde_json::{json, Value};
use x_websearch::attest::{signer_address, AttestorSet, Ready};
use x_websearch::submit::{
    attestor_set, fulfil_calldata, fulfil_gas_limit, request_status, selector, Fees, JournalState,
    Outcome, SubmitError, Submitter, Transaction, FULFIL_SIGNATURE, STATUS_FULFILLED,
};
use x_websearch::watch::{hex0x, unhex0x, EvmRpc, XWEB_PRECOMPILE};

type Checked<T = ()> = Result<T, Box<dyn Error>>;

const CHAIN_ID: u64 = 713_714;

/// The signatures of the test attestors three, one and two over the
/// `evm-fetch` digest: ascending by signer address.
const SIGNATURES: [&str; 3] = [
    "0x7ed23e8fa77c59172964767ddddf8c7d96de1f421479572a0a87e313962b52970ed8483730dabee36f7baf748532bff032f8d8b0a3f782244a93883a8a81f2c71c",
    "0x93854ca688e858b2a183944e5c6c9aa339bbbe46d792e3f46e042fd79aa641a25948de84fb0aea7c4fd5c05e14fa26659deb82928af88b3d7e9761f63a65152d1b",
    "0xd94dba30b84f2e593ffef9a4a943b93ad24556694ebb1f34b3bc2fa1cea096837d63a7f1445e3502162be14fbb93669ae53f549a86f4fd231528a3f98f4938d71c",
];

fn fail(message: impl Into<String>) -> Box<dyn Error> {
    message.into().into()
}

fn fixtures() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/evm")
}

fn read_json(path: &Path) -> Checked<Value> {
    Ok(serde_json::from_str(&std::fs::read_to_string(path)?)?)
}

fn text<'a>(value: &'a Value, pointer: &str) -> Checked<&'a str> {
    value
        .pointer(pointer)
        .and_then(Value::as_str)
        .ok_or_else(|| fail(format!("missing {pointer}")))
}

fn bytes(text: &str) -> Checked<Vec<u8>> {
    unhex0x(text).ok_or_else(|| fail(format!("{text} is not hexadecimal")))
}

fn secp(first: u8, last: u8) -> Checked<SigningKey> {
    let mut secret = [0_u8; 32];
    secret[0] = first;
    secret[31] = last;
    Ok(SigningKey::from_slice(&secret)?)
}

fn submitter_key() -> Checked<SigningKey> {
    secp(0x5a, 1)
}

/// The fulfil for the `evm-fetch` vector's request with all three
/// signatures.
fn ready() -> Checked<Ready> {
    let signers = [3, 1, 2]
        .into_iter()
        .map(|index| secp(0x3c, index).map(|key| signer_address(&key)))
        .collect::<Checked<Vec<_>>>()?;
    let signatures = SIGNATURES
        .iter()
        .map(|signature| {
            bytes(signature)?
                .try_into()
                .map_err(|_| fail("signature is not 65 bytes"))
        })
        .collect::<Checked<Vec<[u8; 65]>>>()?;
    Ok(Ready {
        request_id: 7,
        response: b"Paxeer X Network".to_vec(),
        content_digest: [0x22; 32],
        full_length: 16,
        callback_gas: 200_000,
        digest: bytes("0x21e2a70f243f13f7e2b72d14cc842c5f6b1779b532144b01c1d61d47b886a0be")?
            .try_into()
            .map_err(|_| fail("digest"))?,
        signers,
        signatures,
    })
}

struct Scratch(PathBuf);

impl Scratch {
    fn new(name: &str) -> Checked<Self> {
        let path =
            std::env::temp_dir().join(format!("x-websearch-submit-{}-{name}", std::process::id()));
        let _ = std::fs::remove_dir_all(&path);
        std::fs::create_dir_all(&path)?;
        Ok(Self(path))
    }

    fn journal(&self) -> PathBuf {
        self.0.join("submit")
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

/// One recorded EVM answer.
struct Rule {
    method: String,
    params: Value,
    answer: Value,
    once: bool,
    used: bool,
}

fn load_rules(names: &[&str]) -> Checked<Vec<Rule>> {
    let mut rules = Vec::new();
    for name in names {
        let recording = read_json(&fixtures().join(name))?;
        let list = recording
            .pointer("/endpoints/evm/*")
            .and_then(Value::as_array)
            .ok_or_else(|| fail(format!("{name} has no evm rules")))?;
        for rule in list {
            let answer = rule
                .get("result")
                .map(|result| json!({ "result": result }))
                .or_else(|| rule.get("error").map(|error| json!({ "error": error })))
                .ok_or_else(|| fail("rule without an answer"))?;
            rules.push(Rule {
                method: text(rule, "/method")?.to_owned(),
                params: rule.get("params").cloned().unwrap_or(Value::Null),
                answer,
                once: rule.get("once").and_then(Value::as_bool).unwrap_or(false),
                used: false,
            });
        }
    }
    Ok(rules)
}

fn read_http(stream: &mut TcpStream) -> Checked<Vec<u8>> {
    let mut bytes = Vec::new();
    let mut chunk = [0; 4_096];
    loop {
        if let Some(end) = bytes.windows(4).position(|window| window == b"\r\n\r\n") {
            let head = String::from_utf8(bytes[..end].to_vec())?;
            let length = head
                .split("\r\n")
                .find_map(|line| {
                    let (name, value) = line.split_once(':')?;
                    name.eq_ignore_ascii_case("content-length")
                        .then(|| value.trim().parse::<usize>().ok())?
                })
                .unwrap_or(0);
            if bytes.len() >= end + 4 + length {
                return Ok(bytes[end + 4..end + 4 + length].to_vec());
            }
        }
        let read = stream.read(&mut chunk)?;
        if read == 0 {
            return Err(fail("connection closed early"));
        }
        bytes.extend_from_slice(&chunk[..read]);
    }
}

type Calls = Arc<Mutex<Vec<(String, Value)>>>;

/// Replays a recorded EVM exchange on the loopback interface. A request no
/// recorded rule matches is answered 503. At every broadcast it reads the
/// journal file of request 7 as it is on disk at that moment.
struct Node {
    address: SocketAddr,
    calls: Calls,
    journalled: Arc<Mutex<Vec<Option<String>>>>,
}

struct Replay {
    rules: Mutex<Vec<Rule>>,
    calls: Calls,
    journal: PathBuf,
    journalled: Arc<Mutex<Vec<Option<String>>>>,
}

impl Replay {
    fn reply(&self, request: &Value) -> Option<Value> {
        let method = request.get("method")?.as_str()?;
        let params = request.get("params").cloned().unwrap_or(Value::Null);
        if method == "eth_sendRawTransaction" {
            self.journalled
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .push(std::fs::read_to_string(self.journal.join("7.json")).ok());
        }
        let mut rules = self.rules.lock().unwrap_or_else(PoisonError::into_inner);
        let rule = rules
            .iter_mut()
            .find(|rule| !rule.used && rule.method == method && rule.params == params)?;
        rule.used = rule.once;
        let mut reply =
            json!({ "jsonrpc": "2.0", "id": request.get("id").cloned().unwrap_or(json!(1)) });
        if let (Some(reply), Some(answer)) = (reply.as_object_mut(), rule.answer.as_object()) {
            reply.extend(answer.clone());
        }
        Some(reply)
    }

    fn serve(&self, stream: &mut TcpStream) -> Checked {
        let request: Value = serde_json::from_slice(&read_http(stream)?)?;
        self.calls
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .push((
                text(&request, "/method")?.to_owned(),
                request.get("params").cloned().unwrap_or(Value::Null),
            ));
        let response = self.reply(&request).map_or_else(
            || "HTTP/1.1 503 Service Unavailable\r\nContent-Length: 0\r\nConnection: close\r\n\r\n".to_owned(),
            |reply| {
                let body = reply.to_string();
                format!(
                    "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                    body.len()
                )
            },
        );
        stream.write_all(response.as_bytes())?;
        Ok(())
    }
}

impl Node {
    fn start(names: &[&str], journal: &Path) -> Checked<Self> {
        let listener = TcpListener::bind("127.0.0.1:0")?;
        let address = listener.local_addr()?;
        let calls: Calls = Arc::new(Mutex::new(Vec::new()));
        let journalled = Arc::new(Mutex::new(Vec::new()));
        let replay = Replay {
            rules: Mutex::new(load_rules(names)?),
            calls: Arc::clone(&calls),
            journal: journal.to_path_buf(),
            journalled: Arc::clone(&journalled),
        };
        thread::spawn(move || {
            for mut stream in listener.incoming().flatten() {
                let _ = replay.serve(&mut stream);
            }
        });
        Ok(Self {
            address,
            calls,
            journalled,
        })
    }

    fn rpc(&self) -> Checked<EvmRpc> {
        Ok(EvmRpc::new(&format!("http://{}/", self.address))?)
    }

    fn params(&self, method: &str) -> Vec<Value> {
        self.calls
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .iter()
            .filter(|(name, _)| name == method)
            .map(|(_, params)| params.clone())
            .collect()
    }

    fn calls(&self, method: &str) -> usize {
        self.params(method).len()
    }

    fn journalled(&self) -> Vec<Option<String>> {
        self.journalled
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone()
    }
}

fn recorded() -> Checked<Value> {
    read_json(&fixtures().join("fulfil.json"))
}

fn recorded_hash() -> Checked<[u8; 32]> {
    bytes(text(&recorded()?, "/hash")?)?
        .try_into()
        .map_err(|_| fail("hash is not 32 bytes"))
}

#[test]
fn fulfil_calldata_and_the_signed_transaction_match_an_independent_encoder() -> Checked {
    let recorded = recorded()?;
    let ready = ready()?;
    let calldata = fulfil_calldata(&ready);
    assert_eq!(selector(FULFIL_SIGNATURE), [0xae, 0xb8, 0xa7, 0xec]);
    assert_eq!(hex0x(&calldata), text(&recorded, "/calldata")?);
    let gas_limit = fulfil_gas_limit(&calldata, ready.signatures.len(), ready.callback_gas);
    assert_eq!(
        Some(gas_limit),
        recorded.get("gas_limit").and_then(Value::as_u64)
    );
    let transaction = Transaction {
        chain_id: CHAIN_ID,
        nonce: 3,
        fees: Fees {
            max_fee_per_gas: text(&recorded, "/max_fee_per_gas")?.parse()?,
            max_priority_fee_per_gas: text(&recorded, "/max_priority_fee_per_gas")?.parse()?,
        },
        gas_limit,
        to: XWEB_PRECOMPILE,
        data: calldata,
    };
    let signed = transaction.sign(&submitter_key()?)?;
    assert_eq!(hex0x(&signed.raw), text(&recorded, "/raw")?);
    assert_eq!(signed.hash, recorded_hash()?);

    let inverted = Transaction {
        fees: Fees {
            max_fee_per_gas: 1,
            max_priority_fee_per_gas: 2,
        },
        ..transaction.clone()
    };
    assert_eq!(inverted.sign(&submitter_key()?), Err(SubmitError::Encode));
    let unchained = Transaction {
        chain_id: 0,
        ..transaction
    };
    assert_eq!(unchained.sign(&submitter_key()?), Err(SubmitError::Encode));
    Ok(())
}

#[test]
fn the_attestor_set_and_request_status_are_read_from_the_precompile() -> Checked {
    let scratch = Scratch::new("views")?;
    let node = Node::start(
        &["already-fulfilled.json", "chain.json"],
        &scratch.journal(),
    )?;
    let set = attestor_set(&node.rpc()?)?;
    let expected = [1, 2, 3]
        .into_iter()
        .map(|index| secp(0x3c, index).map(|key| signer_address(&key)))
        .collect::<Checked<Vec<_>>>()?;
    assert_eq!(
        set,
        AttestorSet {
            signers: expected,
            threshold: 2
        }
    );
    assert_eq!(request_status(&node.rpc()?, 7)?, STATUS_FULFILLED);
    Ok(())
}

#[test]
fn a_fulfil_is_journalled_before_broadcast_and_confirmed_from_its_receipt() -> Checked {
    let scratch = Scratch::new("submit")?;
    let node = Node::start(&["submit.json", "chain.json"], &scratch.journal())?;
    let submitter = Submitter::open(node.rpc()?, submitter_key()?, CHAIN_ID, &scratch.journal())?;
    assert_eq!(
        hex0x(&submitter.address()),
        "0xa3e570d1d3ad527d6c518c53be6c6293cddeee01"
    );
    let hash = recorded_hash()?;
    assert_eq!(submitter.submit(&ready()?)?, Outcome::Sent { hash });

    let raw = text(&recorded()?, "/raw")?.to_owned();
    assert_eq!(node.params("eth_sendRawTransaction"), vec![json!([&raw])]);
    let journalled = node.journalled();
    assert_eq!(journalled.len(), 1);
    let on_disk: Value = serde_json::from_str(
        journalled[0]
            .as_deref()
            .ok_or_else(|| fail("no journal entry on disk at broadcast"))?,
    )?;
    assert_eq!(text(&on_disk, "/state")?, "signed");
    assert_eq!(text(&on_disk, "/raw")?, raw);
    assert_eq!(on_disk.get("nonce"), Some(&json!(3)));

    let entry = submitter
        .journal()
        .load(7)?
        .ok_or_else(|| fail("no journal entry"))?;
    assert_eq!(entry.state, JournalState::Signed);

    assert_eq!(submitter.confirm(7)?, Some(Outcome::Fulfilled));
    let entry = submitter
        .journal()
        .load(7)?
        .ok_or_else(|| fail("no journal entry"))?;
    assert_eq!(entry.state, JournalState::Fulfilled);
    assert!(entry.state.completed());

    // A settled request is answered from the journal alone.
    assert_eq!(submitter.submit(&ready()?)?, Outcome::Fulfilled);
    assert_eq!(node.calls("eth_sendRawTransaction"), 1);
    assert_eq!(node.calls("eth_getTransactionCount"), 1);
    assert_eq!(submitter.confirm(8)?, None);
    Ok(())
}

#[test]
fn a_restart_rebroadcasts_the_journalled_bytes_without_signing_again() -> Checked {
    let scratch = Scratch::new("restart")?;
    let node = Node::start(&["submit.json", "chain.json"], &scratch.journal())?;
    let hash = recorded_hash()?;
    {
        let submitter =
            Submitter::open(node.rpc()?, submitter_key()?, CHAIN_ID, &scratch.journal())?;
        assert_eq!(submitter.submit(&ready()?)?, Outcome::Sent { hash });
    }
    let restarted = Submitter::open(node.rpc()?, submitter_key()?, CHAIN_ID, &scratch.journal())?;
    let resumed = restarted.resume()?;
    assert_eq!(resumed, vec![(7, Ok(Outcome::Sent { hash }))]);
    let broadcasts = node.params("eth_sendRawTransaction");
    assert_eq!(broadcasts.len(), 2);
    assert_eq!(broadcasts[0], broadcasts[1]);
    assert_eq!(node.calls("eth_getTransactionCount"), 1);
    assert_eq!(node.calls("eth_maxPriorityFeePerGas"), 1);

    // A submit after the restart rebroadcasts too, and the receipt settles it.
    assert_eq!(restarted.submit(&ready()?)?, Outcome::Sent { hash });
    assert_eq!(node.calls("eth_sendRawTransaction"), 3);
    assert_eq!(restarted.confirm(7)?, Some(Outcome::Fulfilled));
    assert!(restarted.resume()?.is_empty());
    assert_eq!(node.calls("eth_getTransactionCount"), 1);
    Ok(())
}

#[test]
fn an_already_fulfilled_request_is_recorded_as_completed_without_a_transaction() -> Checked {
    let scratch = Scratch::new("already")?;
    let node = Node::start(
        &["already-fulfilled.json", "chain.json"],
        &scratch.journal(),
    )?;
    let submitter = Submitter::open(node.rpc()?, submitter_key()?, CHAIN_ID, &scratch.journal())?;
    let outcome = submitter.submit(&ready()?)?;
    assert_eq!(outcome, Outcome::AlreadyFulfilled);
    assert!(outcome.completed());
    let on_disk = read_json(&scratch.journal().join("7.json"))?;
    assert_eq!(
        on_disk,
        json!({ "request_id": 7, "state": "already_fulfilled" })
    );
    assert_eq!(node.calls("eth_getTransactionCount"), 0);
    assert_eq!(node.calls("eth_sendRawTransaction"), 0);
    assert_eq!(submitter.confirm(7)?, Some(Outcome::AlreadyFulfilled));
    assert_eq!(submitter.submit(&ready()?)?, Outcome::AlreadyFulfilled);
    assert_eq!(node.calls("eth_call"), 1);
    Ok(())
}

#[test]
fn a_refused_broadcast_for_a_request_fulfilled_meanwhile_is_completed() -> Checked {
    let scratch = Scratch::new("lost")?;
    let node = Node::start(&["lost-race.json", "chain.json"], &scratch.journal())?;
    let submitter = Submitter::open(node.rpc()?, submitter_key()?, CHAIN_ID, &scratch.journal())?;
    assert_eq!(submitter.submit(&ready()?)?, Outcome::AlreadyFulfilled);
    assert_eq!(node.calls("eth_sendRawTransaction"), 1);
    let entry = submitter
        .journal()
        .load(7)?
        .ok_or_else(|| fail("no journal entry"))?;
    assert_eq!(entry.state, JournalState::AlreadyFulfilled);
    assert_eq!(entry.transaction, None);
    Ok(())
}

#[test]
fn signatures_out_of_ascending_signer_order_are_refused_before_any_call() -> Checked {
    let scratch = Scratch::new("order")?;
    let node = Node::start(&["submit.json", "chain.json"], &scratch.journal())?;
    let submitter = Submitter::open(node.rpc()?, submitter_key()?, CHAIN_ID, &scratch.journal())?;
    let mut unordered = ready()?;
    unordered.signers.swap(0, 1);
    unordered.signatures.swap(0, 1);
    assert_eq!(submitter.submit(&unordered), Err(SubmitError::Encode));
    let mut repeated = ready()?;
    repeated.signers[1] = repeated.signers[0];
    assert_eq!(submitter.submit(&repeated), Err(SubmitError::Encode));
    assert!(node.params("eth_call").is_empty());
    assert!(!scratch.journal().join("7.json").exists());
    Ok(())
}
