use std::error::Error;
use std::io::{Read as _, Write as _};
use std::net::{SocketAddr, TcpListener, TcpStream};
use std::os::unix::fs::PermissionsExt as _;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, PoisonError};
use std::thread;
use std::time::Duration;

use k256::ecdsa::SigningKey;
use serde_json::{json, Value};
use x_websearch::api::ApiPayload;
use x_websearch::attest::{
    self, evm_requester, network_word, recover_signer, sign_digest, signer_address,
    stored_response, Answer, Attestation, Attestor, AttestorSet, Discard, Level, SignatureExchange,
    DOMAIN, MAX_RESPONSE_BYTES, ORIGIN_EVM, PREIMAGE_LENGTH,
};
use x_websearch::config::FetchLimits;
use x_websearch::content::ContentStore;
use x_websearch::fetch::Fetcher;
use x_websearch::index::WebIndex;
use x_websearch::keys::{KeyFiles, KeyRefusal, KeyRole};
use x_websearch::search::{self, SearchResult};
use x_websearch::watch::{
    decode_requested, hex0x, keccak, requested_topic, unhex0x, EvmError, EvmRpc, RequestWatcher,
    WebRequest, XWEB_PRECOMPILE,
};
use x_websearch::{Limits, RouteTable, RunningServer, Server};

type Outcome<T = ()> = Result<T, Box<dyn Error>>;

const CHAIN_ID: u64 = 713_714;
const RECEIVER_SECRET: &str = "9d61b19deffd5a60ba844af492ec2cc44449c5697b326919703bac031cae7f60";

/// The signatures the three test attestors make over the `evm-fetch` digest,
/// as an independent signer produced them.
const SIGNATURE_ONE: &str = "0x93854ca688e858b2a183944e5c6c9aa339bbbe46d792e3f46e042fd79aa641a25948de84fb0aea7c4fd5c05e14fa26659deb82928af88b3d7e9761f63a65152d1b";
const SIGNATURE_TWO: &str = "0xd94dba30b84f2e593ffef9a4a943b93ad24556694ebb1f34b3bc2fa1cea096837d63a7f1445e3502162be14fbb93669ae53f549a86f4fd231528a3f98f4938d71c";
const SIGNATURE_THREE: &str = "0x7ed23e8fa77c59172964767ddddf8c7d96de1f421479572a0a87e313962b52970ed8483730dabee36f7baf748532bff032f8d8b0a3f782244a93883a8a81f2c71c";

fn fail(message: impl Into<String>) -> Box<dyn Error> {
    message.into().into()
}

fn fixtures() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures")
}

fn read_json(path: &Path) -> Outcome<Value> {
    Ok(serde_json::from_str(&std::fs::read_to_string(path)?)?)
}

fn text<'a>(value: &'a Value, pointer: &str) -> Outcome<&'a str> {
    value
        .pointer(pointer)
        .and_then(Value::as_str)
        .ok_or_else(|| fail(format!("missing {pointer}")))
}

fn fixed<const N: usize>(value: &Value, pointer: &str) -> Outcome<[u8; N]> {
    unhex0x(text(value, pointer)?)
        .and_then(|bytes| bytes.try_into().ok())
        .ok_or_else(|| fail(format!("{pointer} is not {N} bytes")))
}

/// The test attestor key `i`: 0x3c, zeros, then `i`.
fn attestor_key(index: u8) -> Outcome<SigningKey> {
    let mut secret = [0_u8; 32];
    secret[0] = 0x3c;
    secret[31] = index;
    Ok(SigningKey::from_slice(&secret)?)
}

fn address(text: &str) -> Outcome<[u8; 20]> {
    unhex0x(text)
        .and_then(|bytes| bytes.try_into().ok())
        .ok_or_else(|| fail(format!("{text} is not an address")))
}

fn signature(text: &str) -> Outcome<[u8; 65]> {
    unhex0x(text)
        .and_then(|bytes| bytes.try_into().ok())
        .ok_or_else(|| fail(format!("{text} is not a signature")))
}

struct Scratch(PathBuf);

impl Scratch {
    fn new(name: &str) -> Outcome<Self> {
        let path =
            std::env::temp_dir().join(format!("x-websearch-attest-{}-{name}", std::process::id()));
        let _ = std::fs::remove_dir_all(&path);
        std::fs::create_dir_all(&path)?;
        Ok(Self(path))
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

fn load_rules(names: &[&str]) -> Outcome<Vec<Rule>> {
    let mut rules = Vec::new();
    for name in names {
        let recording = read_json(&fixtures().join("evm").join(name))?;
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

fn read_http(stream: &mut TcpStream) -> Outcome<(String, Vec<u8>)> {
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
                return Ok((head, bytes[end + 4..end + 4 + length].to_vec()));
            }
        }
        let read = stream.read(&mut chunk)?;
        if read == 0 {
            return Err(fail("connection closed early"));
        }
        bytes.extend_from_slice(&chunk[..read]);
    }
}

/// Replays a recorded EVM exchange on the loopback interface. A request no
/// recorded rule matches is answered 503.
struct Node {
    address: SocketAddr,
    calls: Arc<Mutex<Vec<(String, Value)>>>,
}

fn reply(rules: &Mutex<Vec<Rule>>, request: &Value) -> Option<Value> {
    let method = request.get("method")?.as_str()?;
    let params = request.get("params").cloned().unwrap_or(Value::Null);
    let mut rules = rules.lock().unwrap_or_else(PoisonError::into_inner);
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

fn serve_rpc(
    stream: &mut TcpStream,
    rules: &Mutex<Vec<Rule>>,
    calls: &Mutex<Vec<(String, Value)>>,
) -> Outcome {
    let (_, body) = read_http(stream)?;
    let request: Value = serde_json::from_slice(&body)?;
    calls.lock().unwrap_or_else(PoisonError::into_inner).push((
        text(&request, "/method")?.to_owned(),
        request.get("params").cloned().unwrap_or(Value::Null),
    ));
    let response = reply(rules, &request).map_or_else(
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

impl Node {
    fn start(names: &[&str]) -> Outcome<Self> {
        let listener = TcpListener::bind("127.0.0.1:0")?;
        let address = listener.local_addr()?;
        let rules = Arc::new(Mutex::new(load_rules(names)?));
        let calls = Arc::new(Mutex::new(Vec::new()));
        let served = Arc::clone(&calls);
        thread::spawn(move || {
            for mut stream in listener.incoming().flatten() {
                let _ = serve_rpc(&mut stream, &rules, &served);
            }
        });
        Ok(Self { address, calls })
    }

    fn rpc(&self) -> Outcome<EvmRpc> {
        Ok(EvmRpc::new(&format!("http://{}/", self.address))?)
    }

    fn calls(&self, method: &str) -> usize {
        self.calls
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .iter()
            .filter(|(name, _)| name == method)
            .count()
    }
}

#[test]
fn the_watcher_follows_requested_logs_to_the_confirmation_depth() -> Outcome {
    let scratch = Scratch::new("watch")?;
    let node = Node::start(&["watch.json"])?;
    let state = scratch.0.join("watch");
    let mut watcher = RequestWatcher::open(node.rpc()?, 2, &state, Some(0x19))?;
    let requests = watcher.poll()?;
    assert_eq!(watcher.last_head(), 0x20);
    assert_eq!(watcher.next_block(), Some(0x1f));
    assert_eq!(std::fs::read_to_string(state.join("cursor"))?, "31");
    assert_eq!(requests.len(), 2);
    let mut paid = [0_u8; 32];
    paid[24..].copy_from_slice(&1_000_000_000_000_000_u64.to_be_bytes());
    let mut requester = [0_u8; 20];
    requester[17..].copy_from_slice(&[0x0a, 0x11, 0xce]);
    assert_eq!(
        requests[0],
        WebRequest {
            request_id: 7,
            requester,
            kind: 1,
            payload: b"https://paxeer.app/".to_vec(),
            callback_gas: 200_000,
            paid,
            timeout_height: 528,
            block_number: 0x1c,
        }
    );
    assert_eq!(requests[1].request_id, 8);
    assert_eq!(requests[1].kind, 2);
    assert_eq!(requests[1].payload, b"paxeer x network");
    assert_eq!(requests[1].block_number, 0x1d);

    // The head has not moved past the confirmed range: no second query.
    assert!(watcher.poll()?.is_empty());
    assert_eq!(node.calls("eth_getLogs"), 1);

    // A restart resumes from the stored cursor, not from the start given.
    let mut reopened = RequestWatcher::open(node.rpc()?, 2, &state, Some(0))?;
    assert_eq!(reopened.next_block(), Some(0x1f));
    assert!(reopened.poll()?.is_empty());
    assert_eq!(node.calls("eth_getLogs"), 1);
    assert_eq!(node.calls("eth_blockNumber"), 3);
    Ok(())
}

#[test]
fn only_exact_requested_logs_of_the_precompile_decode() -> Outcome {
    let recording = read_json(&fixtures().join("evm/watch.json"))?;
    let log = recording
        .pointer("/endpoints/evm/*/1/result/0")
        .cloned()
        .ok_or_else(|| fail("no recorded log"))?;
    assert_eq!(decode_requested(&log)?.request_id, 7);
    assert_eq!(
        text(&log, "/topics/0")?,
        hex0x(&requested_topic()),
        "the recorded topic is the event's"
    );

    let mut foreign = log.clone();
    foreign["address"] = json!(hex0x(&[0x11; 20]));
    assert_eq!(decode_requested(&foreign), Err(EvmError::ForeignLog));
    let mut other_event = log.clone();
    other_event["topics"][0] = json!(hex0x(&keccak(b"Other()")));
    assert_eq!(decode_requested(&other_event), Err(EvmError::ForeignLog));
    let mut removed = log.clone();
    removed["removed"] = json!(true);
    assert_eq!(decode_requested(&removed), Err(EvmError::ForeignLog));

    let data = text(&log, "/data")?.to_owned();
    let mut trailing = log.clone();
    trailing["data"] = json!(format!("{data}{}", "00".repeat(32)));
    assert_eq!(decode_requested(&trailing), Err(EvmError::Malformed));
    let mut dirty_padding = log.clone();
    dirty_padding["data"] = json!(format!("{}01", &data[..data.len() - 2]));
    assert_eq!(decode_requested(&dirty_padding), Err(EvmError::Malformed));
    let mut wide_id = log.clone();
    wide_id["topics"][1] = json!(hex0x(&[0xff; 32]));
    assert_eq!(decode_requested(&wide_id), Err(EvmError::Malformed));
    let mut short_topics = log;
    short_topics["topics"] = json!([hex0x(&requested_topic())]);
    assert_eq!(decode_requested(&short_topics), Err(EvmError::Malformed));
    assert_eq!(XWEB_PRECOMPILE[18..], [0x10, 0x19]);
    Ok(())
}

fn vectors() -> Outcome<Value> {
    read_json(
        &Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../../modules/xweb/types/testdata/preimage-vectors.json"),
    )
}

fn number_at(value: &Value, pointer: &str) -> Outcome<u64> {
    value
        .pointer(pointer)
        .and_then(Value::as_u64)
        .ok_or_else(|| fail(format!("missing {pointer}")))
}

/// Checks the api preimage vector's payload against the codec vector it
/// names in the api vector file.
fn api_vector_payload(name: &str, payload: &[u8]) -> Outcome {
    let codec = read_json(
        &Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../../modules/xweb/types/testdata/api-vectors.json"),
    )?;
    let vector = codec
        .get("vectors")
        .and_then(Value::as_array)
        .and_then(|list| {
            list.iter()
                .find(|vector| vector.get("name") == Some(&json!(name)))
        })
        .ok_or_else(|| fail(format!("no api vector {name}")))?;
    assert_eq!(hex0x(payload), text(vector, "/payload")?);
    assert_eq!(hex0x(&keccak(payload)), text(vector, "/payload_hash")?);
    Ok(())
}

#[test]
fn the_preimage_matches_every_pinned_vector() -> Outcome {
    let pinned = vectors()?;
    assert_eq!(text(&pinned, "/domain")?.as_bytes(), DOMAIN);
    assert_eq!(
        pinned.get("preimage_length").and_then(Value::as_u64),
        Some(u64::try_from(PREIMAGE_LENGTH)?)
    );
    let list = pinned
        .get("vectors")
        .and_then(Value::as_array)
        .ok_or_else(|| fail("no vectors"))?;
    assert_eq!(list.len(), 3);
    let names = ["evm-fetch", "program-search", "evm-api-single"];
    for (vector, name) in list.iter().zip(names) {
        assert_eq!(text(vector, "/name")?, name);
        let number = |key: &str| {
            vector
                .get(key)
                .and_then(Value::as_u64)
                .ok_or_else(|| fail(format!("missing {key}")))
        };
        let payload_hex =
            unhex0x(text(vector, "/payload_hex")?).ok_or_else(|| fail("payload_hex is not hex"))?;
        let payload = if name == "evm-api-single" {
            api_vector_payload(text(vector, "/payload_vector")?, &payload_hex)?;
            payload_hex.as_slice()
        } else {
            let payload = text(vector, "/payload")?.as_bytes();
            assert_eq!(payload, payload_hex.as_slice());
            payload
        };
        let response = text(vector, "/response")?.as_bytes();
        assert_eq!(
            unhex0x(text(vector, "/response_hex")?).as_deref(),
            Some(response)
        );
        assert_eq!(hex0x(&keccak(payload)), text(vector, "/payload_hash")?);
        assert_eq!(hex0x(&keccak(response)), text(vector, "/response_hash")?);
        let attestation = Attestation {
            origin: u8::try_from(number("origin")?)?,
            network_id: network_word(number("network_id")?),
            requester: fixed(vector, "/requester")?,
            request_id: number("request_id")?,
            kind: u8::try_from(number("kind")?)?,
            payload_hash: keccak(payload),
            content_digest: fixed(vector, "/content_digest")?,
            response_hash: keccak(response),
            full_length: u32::try_from(number("full_length")?)?,
        };
        assert_eq!(hex0x(&attestation.preimage()), text(vector, "/preimage")?);
        let digest = attestation.digest();
        assert_eq!(hex0x(&digest), text(vector, "/digest")?);
        let signer = recover_signer(&digest, &signature(text(vector, "/signature")?)?)?;
        assert_eq!(hex0x(&signer), text(vector, "/signer")?);
    }

    // The api vector is a single-level request whose one signature comes
    // from the attestor its payload names.
    let api = &list[2];
    assert_eq!(number_at(api, "/kind")?, 3);
    let payload = ApiPayload::decode(
        &unhex0x(text(api, "/payload_hex")?).ok_or_else(|| fail("payload_hex is not hex"))?,
    )?;
    assert_eq!(payload.level, 1);
    assert_eq!(hex0x(&payload.attestor), text(api, "/signer")?);

    // The origin-1 attestation of the decoded request is the evm-fetch vector.
    let evm = &list[0];
    assert_eq!(text(evm, "/name")?, "evm-fetch");
    let mut requester = [0_u8; 20];
    requester[17..].copy_from_slice(&[0x0a, 0x11, 0xce]);
    let request = WebRequest {
        request_id: 7,
        requester,
        kind: 1,
        payload: b"https://paxeer.app/".to_vec(),
        callback_gas: 200_000,
        paid: [0; 32],
        timeout_height: 528,
        block_number: 0x1c,
    };
    let attestation = Attestation::evm(CHAIN_ID, &request, [0x22; 32], b"Paxeer X Network", 16);
    assert_eq!(attestation.origin, ORIGIN_EVM);
    assert_eq!(attestation.requester, evm_requester(requester));
    assert_eq!(hex0x(&attestation.digest()), text(evm, "/digest")?);
    Ok(())
}

#[test]
fn attestor_signatures_are_raw_low_s_and_match_an_independent_signer() -> Outcome {
    let digest: [u8; 32] = fixed(&vectors()?, "/vectors/0/digest")?;
    for (index, expected, signer) in [
        (
            1,
            SIGNATURE_ONE,
            "0x9c4ca69112e7f6ee6b77a8c39dc6039556e1ec37",
        ),
        (
            2,
            SIGNATURE_TWO,
            "0xdffad6f7777ff7d6c00d3ef0ebcec7a4a7bad2d5",
        ),
        (
            3,
            SIGNATURE_THREE,
            "0x7908498435429fb92cabb64821e1592cf086cf2e",
        ),
    ] {
        let key = attestor_key(index)?;
        let produced = sign_digest(&key, &digest)?;
        assert_eq!(hex0x(&produced), expected);
        assert_eq!(hex0x(&signer_address(&key)), signer);
        assert_eq!(recover_signer(&digest, &produced)?, address(signer)?);
    }

    let original = signature(SIGNATURE_ONE)?;
    let mut wrong_v = original;
    wrong_v[64] = 1;
    assert_eq!(
        recover_signer(&digest, &wrong_v),
        Err(attest::SignatureError::Recovery)
    );
    // s' = n - s recovers with the other parity but is refused as high.
    let order = unhex0x("0xfffffffffffffffffffffffffffffffebaaedce6af48a03bbfd25e8cd0364141")
        .ok_or_else(|| fail("order"))?;
    let mut negated = [0_u8; 32];
    let mut borrow = 0_u16;
    for ((out, n), s) in negated
        .iter_mut()
        .rev()
        .zip(order.iter().rev())
        .zip(original[32..64].iter().rev())
    {
        let difference = u16::from(*n) + 256 - u16::from(*s) - borrow;
        *out = u8::try_from(difference & 0xff)?;
        borrow = u16::from(difference < 256);
    }
    let mut high = original;
    high[32..64].copy_from_slice(&negated);
    high[64] = if original[64] == 27 { 28 } else { 27 };
    assert_eq!(
        recover_signer(&digest, &high),
        Err(attest::SignatureError::HighS)
    );
    Ok(())
}

#[test]
fn the_stored_response_is_cut_at_a_character_boundary() -> Outcome {
    let short = "Paxeer X Network";
    assert_eq!(stored_response(short)?, (short.as_bytes().to_vec(), 16));
    let long = format!("a{}", "é".repeat(3_000));
    let (response, full_length) = stored_response(&long)?;
    assert_eq!(full_length, 6_001);
    assert_eq!(response.len(), MAX_RESPONSE_BYTES - 1);
    assert!(std::str::from_utf8(&response).is_ok());
    let exact = "b".repeat(MAX_RESPONSE_BYTES + 10);
    let (response, full_length) = stored_response(&exact)?;
    assert_eq!(response.len(), MAX_RESPONSE_BYTES);
    assert_eq!(full_length, 4_106);
    Ok(())
}

/// A loopback web site with one plain-text page and no robots.txt.
struct Site {
    address: SocketAddr,
}

fn serve_page(stream: &mut TcpStream, page: &[u8]) -> Outcome {
    let (head, _) = read_http(stream)?;
    let target = head
        .split(' ')
        .nth(1)
        .ok_or_else(|| fail("no request target"))?;
    let response = if target == "/plain.txt" {
        let mut bytes = format!(
            "HTTP/1.1 200 OK\r\nContent-Type: text/plain; charset=utf-8\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
            page.len()
        )
        .into_bytes();
        bytes.extend_from_slice(page);
        bytes
    } else {
        b"HTTP/1.1 404 Not Found\r\nContent-Length: 0\r\nConnection: close\r\n\r\n".to_vec()
    };
    stream.write_all(&response)?;
    Ok(())
}

impl Site {
    fn start() -> Outcome<Self> {
        let page = std::fs::read(fixtures().join("site/plain.txt"))?;
        let listener = TcpListener::bind("127.0.0.1:0")?;
        let address = listener.local_addr()?;
        thread::spawn(move || {
            for mut stream in listener.incoming().flatten() {
                let _ = serve_page(&mut stream, &page);
            }
        });
        Ok(Self { address })
    }
}

fn loopback_fetcher() -> Outcome<Fetcher> {
    Ok(Fetcher::new(FetchLimits {
        connect_timeout_ms: 3_000,
        total_timeout_ms: 10_000,
        max_body_bytes: 2_097_152,
        max_redirects: 3,
        allow_loopback: true,
    })?)
}

#[test]
fn an_attestor_fetches_and_searches_independently_and_signs_the_digest() -> Outcome {
    let scratch = Scratch::new("attestor")?;
    let site = Site::start()?;
    let data = scratch.0.join("data");
    let store = Arc::new(ContentStore::open(&data, &[])?);
    let index = Arc::new(WebIndex::open(&data)?);
    index.put(
        "https://paxeer.app/",
        "Paxeer",
        "Paxeer X Network web search for contracts",
    )?;
    index.commit()?;
    let key = attestor_key(1)?;
    let attestor = Attestor::new(
        key.clone(),
        CHAIN_ID,
        Arc::new(loopback_fetcher()?),
        Arc::clone(&index),
        Arc::clone(&store),
    );
    assert_eq!(attestor.signer(), signer_address(&key));

    let url = format!("http://{}/plain.txt", site.address);
    let mut requester = [0_u8; 20];
    requester[19] = 0x42;
    let request = WebRequest {
        request_id: 11,
        requester,
        kind: 1,
        payload: url.clone().into_bytes(),
        callback_gas: 50_000,
        paid: [0; 32],
        timeout_height: 900,
        block_number: 30,
    };
    let answer = attestor.attest(&request)?;
    let page = loopback_fetcher()?.fetch(&url)?;
    let (response, full_length) = stored_response(&page.text)?;
    assert_eq!(answer.response, response);
    assert_eq!(answer.attestation.full_length, full_length);
    assert_eq!(answer.attestation.content_digest, page.digest);
    assert_eq!(store.get(&page.digest)?, Some(page.canonical));
    assert_eq!(answer.attestation.network_id, network_word(CHAIN_ID));
    assert_eq!(answer.attestation.payload_hash, keccak(url.as_bytes()));
    assert_eq!(answer.digest, answer.attestation.digest());
    assert_eq!(answer.callback_gas, 50_000);
    assert_eq!(answer.timeout_height, 900);
    assert_eq!(answer.level, Level::Majority);
    assert_eq!(
        recover_signer(&answer.digest, &answer.signature)?,
        signer_address(&key)
    );

    let query = "paxeer network";
    let search_request = WebRequest {
        request_id: 12,
        kind: 2,
        payload: query.as_bytes().to_vec(),
        ..request.clone()
    };
    let answer = attestor.attest(&search_request)?;
    let results: Vec<SearchResult> = search::search(&index, query)?
        .into_iter()
        .map(|scored| scored.result)
        .collect();
    assert_eq!(results.len(), 1);
    let canonical = search::search_canonical_bytes(query, &results)?;
    let digest = store
        .put(&canonical)
        .map_err(|error| fail(error.to_string()))?;
    assert_eq!(answer.attestation.content_digest, digest);
    assert_eq!(answer.response, search::search_text(&results)?.into_bytes());
    assert_eq!(answer.level, Level::Majority);

    let unknown = WebRequest {
        kind: 9,
        ..request.clone()
    };
    assert_eq!(
        attestor.attest(&unknown),
        Err(attest::AttestError::UnknownKind(9))
    );
    let binary = WebRequest {
        payload: vec![0xff, 0xfe],
        ..request
    };
    assert_eq!(attestor.attest(&binary), Err(attest::AttestError::Payload));
    Ok(())
}

/// This sidecar's answer to the evm-fetch vector's request, signed by the
/// test attestor `index`, with `response` in place of the vector's.
fn vector_answer(index: u8, response: &[u8]) -> Outcome<Answer> {
    let mut requester = [0_u8; 20];
    requester[17..].copy_from_slice(&[0x0a, 0x11, 0xce]);
    let request = WebRequest {
        request_id: 7,
        requester,
        kind: 1,
        payload: b"https://paxeer.app/".to_vec(),
        callback_gas: 200_000,
        paid: [0; 32],
        timeout_height: 528,
        block_number: 0x1c,
    };
    let attestation = Attestation::evm(
        CHAIN_ID,
        &request,
        [0x22; 32],
        response,
        u32::try_from(response.len())?,
    );
    let key = attestor_key(index)?;
    let digest = attestation.digest();
    Ok(Answer {
        attestation,
        level: Level::Majority,
        response: response.to_vec(),
        callback_gas: 200_000,
        timeout_height: 528,
        digest,
        signer: signer_address(&key),
        signature: sign_digest(&key, &digest)?,
    })
}

fn three_attestors() -> Outcome<AttestorSet> {
    Ok(AttestorSet {
        signers: vec![
            signer_address(&attestor_key(1)?),
            signer_address(&attestor_key(2)?),
            signer_address(&attestor_key(3)?),
        ],
        threshold: 2,
    })
}

struct Sidecar {
    exchange: Arc<SignatureExchange>,
    server: Option<RunningServer>,
    url: String,
}

impl Drop for Sidecar {
    fn drop(&mut self) {
        if let Some(server) = self.server.take() {
            let _ = server.shutdown();
        }
    }
}

fn sidecar(state: &Path, peers: &[String], answer: Answer) -> Outcome<Sidecar> {
    let exchange = Arc::new(SignatureExchange::open(state, peers)?);
    exchange.record(answer);
    let mut routes = RouteTable::new();
    attest::register(&mut routes, &exchange)?;
    assert!(attest::register(&mut routes, &exchange).is_err());
    let limits = Limits {
        workers: 2,
        ..Limits::default()
    };
    let server = Server::bind("127.0.0.1:0".parse()?, limits, routes)?.spawn()?;
    let url = format!("http://{}", server.local_addr());
    Ok(Sidecar {
        exchange,
        server: Some(server),
        url,
    })
}

fn get(address: &str, target: &str) -> Outcome<(u16, Value)> {
    let host = address.trim_start_matches("http://");
    let mut stream = TcpStream::connect(host)?;
    stream.set_read_timeout(Some(Duration::from_secs(5)))?;
    stream.write_all(format!("GET {target} HTTP/1.1\r\nHost: {host}\r\n\r\n").as_bytes())?;
    let (head, body) = read_http(&mut stream)?;
    let status = head
        .split(' ')
        .nth(1)
        .ok_or_else(|| fail("no status"))?
        .parse()?;
    Ok((status, serde_json::from_slice(&body)?))
}

#[test]
fn signatures_are_exchanged_over_the_route_and_a_different_digest_is_discarded() -> Outcome {
    let scratch = Scratch::new("exchange")?;
    let vector = b"Paxeer X Network";
    let two = sidecar(&scratch.0.join("two"), &[], vector_answer(2, vector)?)?;
    let three = sidecar(
        &scratch.0.join("three"),
        &[],
        vector_answer(3, b"Paxeer X Networks")?,
    )?;
    let peers = vec![two.url.clone(), three.url.clone()];
    let one = sidecar(&scratch.0.join("one"), &peers, vector_answer(1, vector)?)?;
    let set = three_attestors()?;

    let (status, record) = get(&one.url, "/attestations/7")?;
    assert_eq!(status, 200);
    assert_eq!(
        text(&record, "/digest")?,
        "0x21e2a70f243f13f7e2b72d14cc842c5f6b1779b532144b01c1d61d47b886a0be"
    );
    assert_eq!(text(&record, "/signature")?, SIGNATURE_ONE);
    assert_eq!(get(&one.url, "/attestations/99")?.0, 404);
    assert_eq!(get(&one.url, "/attestations/007")?.0, 400);
    assert_eq!(get(&one.url, "/attestations/x")?.0, 400);
    assert_eq!(get(&one.url, "/attestations/")?.0, 400);

    // One signature alone does not reach the threshold of two.
    assert_eq!(one.exchange.ready(7, &set), None);
    assert_eq!(one.exchange.collect(7, &set), 2);
    let ready = one
        .exchange
        .ready(7, &set)
        .ok_or_else(|| fail("threshold not reached"))?;
    assert_eq!(ready.request_id, 7);
    assert_eq!(ready.response, vector);
    assert_eq!(ready.content_digest, [0x22; 32]);
    assert_eq!(ready.full_length, 16);
    assert_eq!(ready.callback_gas, 200_000);
    assert_eq!(
        ready.signers,
        vec![
            signer_address(&attestor_key(1)?),
            signer_address(&attestor_key(2)?)
        ]
    );
    assert!(ready.signers.windows(2).all(|pair| pair[0] < pair[1]));
    assert_eq!(
        ready.signatures,
        vec![signature(SIGNATURE_ONE)?, signature(SIGNATURE_TWO)?]
    );

    let discarded = one.exchange.discarded();
    assert_eq!(discarded.len(), 1);
    assert_eq!(discarded[0].peer, three.url);
    assert_eq!(discarded[0].reason, Discard::DifferentDigest);
    assert_eq!(
        discarded[0].claimed_digest,
        Some(
            three
                .exchange
                .answer(7)
                .ok_or_else(|| fail("no answer"))?
                .digest
        )
    );
    let log = std::fs::read_to_string(one.exchange.log_path())?;
    let lines: Vec<Value> = log
        .lines()
        .map(serde_json::from_str)
        .collect::<Result<_, _>>()?;
    assert_eq!(lines.len(), 1);
    assert_eq!(text(&lines[0], "/reason")?, "different_digest");
    assert_eq!(text(&lines[0], "/peer")?, three.url);
    assert_eq!(lines[0].get("request_id"), Some(&json!(7)));

    // A request this sidecar has no answer for collects nothing.
    assert_eq!(one.exchange.collect(8, &set), 0);
    assert_eq!(one.exchange.pending(), vec![7]);
    one.exchange.expire(528);
    assert_eq!(one.exchange.pending(), vec![7]);
    one.exchange.expire(529);
    assert!(one.exchange.pending().is_empty());
    Ok(())
}

#[test]
fn a_peer_record_is_taken_only_for_a_registered_signer_it_recovers_to() -> Outcome {
    let scratch = Scratch::new("accept")?;
    let exchange = SignatureExchange::open(&scratch.0, &[])?;
    exchange.record(vector_answer(1, b"Paxeer X Network")?);
    let set = three_attestors()?;
    let honest = vector_answer(3, b"Paxeer X Network")?.record();

    let mut forged = honest.clone();
    forged["signer"] = json!(hex0x(&signer_address(&attestor_key(2)?)));
    assert_eq!(
        exchange.accept("peer", 7, &forged, &set),
        Err(Discard::BadSignature)
    );
    let mut garbled = honest.clone();
    garbled["signature"] = json!(format!("0x{}", "00".repeat(65)));
    assert_eq!(
        exchange.accept("peer", 7, &garbled, &set),
        Err(Discard::BadSignature)
    );
    let unregistered = AttestorSet {
        signers: set.signers[..2].to_vec(),
        threshold: 2,
    };
    assert_eq!(
        exchange.accept("peer", 7, &honest, &unregistered),
        Err(Discard::UnknownSigner)
    );
    let mut extra = honest.clone();
    extra["note"] = json!("x");
    assert_eq!(
        exchange.accept("peer", 7, &extra, &set),
        Err(Discard::Malformed)
    );
    let mut other = honest.clone();
    other["request_id"] = json!(8);
    assert_eq!(
        exchange.accept("peer", 7, &other, &set),
        Err(Discard::WrongRequest)
    );
    assert_eq!(exchange.accept("peer", 9, &honest, &set), Ok(None));
    assert_eq!(exchange.discarded().len(), 5);
    assert_eq!(
        std::fs::read_to_string(exchange.log_path())?
            .lines()
            .count(),
        5
    );
    assert_eq!(exchange.ready(7, &set), None);

    assert_eq!(
        exchange.accept("peer", 7, &honest, &set),
        Ok(Some(signer_address(&attestor_key(3)?)))
    );
    let ready = exchange
        .ready(7, &set)
        .ok_or_else(|| fail("threshold not reached"))?;
    assert_eq!(
        ready.signatures,
        vec![signature(SIGNATURE_THREE)?, signature(SIGNATURE_ONE)?]
    );
    // A signer later removed from the set no longer counts.
    assert_eq!(exchange.ready(7, &unregistered), None);
    exchange.forget(7);
    assert_eq!(exchange.ready(7, &set), None);
    Ok(())
}

#[test]
fn the_attestor_key_must_differ_from_the_submitter_and_receiver_keys() -> Outcome {
    let scratch = Scratch::new("keys")?;
    let write = |name: &str, secret: &str| -> Outcome<PathBuf> {
        let path = scratch.0.join(name);
        std::fs::write(&path, secret)?;
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600))?;
        Ok(path)
    };
    let attestor = write(
        "attestor.key",
        "3c00000000000000000000000000000000000000000000000000000000000001",
    )?;
    let same = write(
        "same.key",
        "3c00000000000000000000000000000000000000000000000000000000000001",
    )?;
    let submitter = write(
        "submitter.key",
        "5a00000000000000000000000000000000000000000000000000000000000001",
    )?;
    let receiver = write("receiver.key", RECEIVER_SECRET)?;
    let receiver_as_attestor = write("receiver-copy.key", RECEIVER_SECRET)?;

    let refused = KeyFiles {
        attestor: Some(attestor.clone()),
        submitter: Some(same),
        receiver: receiver.clone(),
    }
    .load()
    .err()
    .ok_or_else(|| fail("identical attestor and submitter keys loaded"))?;
    assert_eq!(refused.role, KeyRole::Submitter);
    assert_eq!(refused.refusal, KeyRefusal::SameKeyAs(KeyRole::Attestor));

    let refused = KeyFiles {
        attestor: Some(receiver_as_attestor),
        submitter: Some(submitter.clone()),
        receiver: receiver.clone(),
    }
    .load()
    .err()
    .ok_or_else(|| fail("identical attestor and receiver keys loaded"))?;
    assert_eq!(refused.role, KeyRole::Receiver);
    assert_eq!(refused.refusal, KeyRefusal::SameKeyAs(KeyRole::Attestor));

    let keys = KeyFiles {
        attestor: Some(attestor),
        submitter: Some(submitter),
        receiver,
    }
    .load()?;
    let attestor = keys.attestor().ok_or_else(|| fail("no attestor key"))?;
    assert_eq!(signer_address(attestor), signer_address(&attestor_key(1)?));
    Ok(())
}
