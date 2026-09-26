use std::error::Error;
use std::io::{Read as _, Write as _};
use std::net::{SocketAddr, TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, PoisonError};
use std::thread;

use ed25519_dalek::SigningKey as SubmitterKey;
use k256::ecdsa::SigningKey;
use serde_json::{json, Value};
use x_websearch::attest::{
    network_word, recover_signer, sign_digest, signer_address, stored_response, AttestorSet, Ready,
    SignatureExchange, ORIGIN_PROGRAM,
};
use x_websearch::config::FetchLimits;
use x_websearch::content::ContentStore;
use x_websearch::fetch::Fetcher;
use x_websearch::index::WebIndex;
use x_websearch::kernel::{
    decode_request_record, encode_activity, observation_bytes, ActivityOptions, KernelAttestor,
    KernelError, KernelRelay, KernelWatcher, ObservationSubmitter, ProgramRequest, Step,
    EVENTS_METHOD, OBSERVATION_HEADER_BYTES, REQUEST_TOPIC,
};
use x_websearch::payment::{hex, unhex};
use x_websearch::search;
use x_websearch::watch::{keccak, unhex0x};

type Checked<T = ()> = Result<T, Box<dyn Error>>;

const NETWORK: u32 = 9;
const SUBMITTER_DID: &str = "did:key:web-attestor-one";

/// The three signatures over the origin-2 digest of the shared adapter
/// observation, ascending by signer.
const OBSERVATION_SIGNATURES: [&str; 3] = [
    "5aa67dfdbcd13d3348b83646f6c940a4ab46bd53bd1e569949a77eff57e946e44b9cc5c576b7b15a7e80fa701d945af66a4a14d671a25785270c57f4a0c3d6d21b",
    "ca5e20f8607e3dd9cf1cf34e2eb1bf9a608e399ea5da7044e8a0eb75c5b87fba5ac71a449071ba4afde29614adfb21b135367f0bbb847f9b2689ecceae9c39ef1b",
    "4a8f5d5c2d772ad6fee3a946b594b91605c952e8cd522961368c4e3e6616a9d13c961187d6e87a4b3c727d3970871bc83a7b800752a2394bb702c56eb5a76db01b",
];

const CONTENT_DIGEST: &str = "2d823e82313101707a7081be2efb28edce966e3e6d1eaec9f7c1e48088f90781";

fn fail(message: impl Into<String>) -> Box<dyn Error> {
    message.into().into()
}

fn manifest() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

fn fixtures() -> PathBuf {
    manifest().join("tests/fixtures/kernel")
}

fn repository() -> PathBuf {
    manifest().join("../../..")
}

fn read_json(path: &Path) -> Checked<Value> {
    Ok(serde_json::from_str(&std::fs::read_to_string(path)?)?)
}

fn bytes(text: &str) -> Checked<Vec<u8>> {
    unhex(text).ok_or_else(|| fail(format!("{text} is not hexadecimal")))
}

fn fixed<const N: usize>(text: &str) -> Checked<[u8; N]> {
    bytes(text)?
        .try_into()
        .map_err(|_| fail(format!("{text} is not {N} bytes")))
}

fn attestor_key(index: u8) -> Checked<SigningKey> {
    let mut secret = [0_u8; 32];
    secret[0] = 0x3c;
    secret[31] = index;
    Ok(SigningKey::from_slice(&secret)?)
}

fn submitter_key() -> SubmitterKey {
    let mut seed = [0_u8; 32];
    seed[0] = 21;
    SubmitterKey::from_bytes(&seed)
}

fn program(first: u8) -> [u8; 32] {
    std::array::from_fn(|i| first.wrapping_add(u8::try_from(i).unwrap_or(0)))
}

struct Scratch(PathBuf);

impl Scratch {
    fn new(name: &str) -> Checked<Self> {
        let path =
            std::env::temp_dir().join(format!("x-websearch-kernel-{}-{name}", std::process::id()));
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

/// One recorded gateway answer. A rule without params answers any params.
struct Rule {
    method: String,
    params: Option<Value>,
    answer: Value,
}

fn load_rules(name: &str) -> Checked<Vec<Rule>> {
    let recording = read_json(&fixtures().join(name))?;
    let list = recording
        .pointer("/endpoints/gateway/*")
        .and_then(Value::as_array)
        .ok_or_else(|| fail(format!("{name} has no gateway rules")))?;
    list.iter()
        .map(|rule| {
            let answer = rule
                .get("result")
                .map(|result| json!({ "result": result }))
                .or_else(|| rule.get("error").map(|error| json!({ "error": error })))
                .ok_or_else(|| fail("rule without an answer"))?;
            Ok(Rule {
                method: rule
                    .get("method")
                    .and_then(Value::as_str)
                    .ok_or_else(|| fail("rule without a method"))?
                    .to_owned(),
                params: rule.get("params").cloned(),
                answer,
            })
        })
        .collect()
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

/// Replays a recorded gateway exchange on the loopback interface. A request
/// no recorded rule matches is answered 503.
struct Gateway {
    address: SocketAddr,
    calls: Calls,
}

fn serve(rules: &[Rule], calls: &Calls, stream: &mut TcpStream) -> Checked {
    let request: Value = serde_json::from_slice(&read_http(stream)?)?;
    let method = request
        .get("method")
        .and_then(Value::as_str)
        .ok_or_else(|| fail("request without a method"))?
        .to_owned();
    let params = request.get("params").cloned().unwrap_or(Value::Null);
    calls
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .push((method.clone(), params.clone()));
    let rule = rules.iter().find(|rule| {
        rule.method == method && rule.params.as_ref().is_none_or(|wanted| *wanted == params)
    });
    let response = rule.map_or_else(
        || "HTTP/1.1 503 Service Unavailable\r\nContent-Length: 0\r\nConnection: close\r\n\r\n".to_owned(),
        |rule| {
            let mut reply = json!({ "jsonrpc": "2.0", "id": request.get("id").cloned().unwrap_or(json!(1)) });
            if let (Some(reply), Some(answer)) = (reply.as_object_mut(), rule.answer.as_object()) {
                reply.extend(answer.clone());
            }
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

impl Gateway {
    fn start(name: &str) -> Checked<Self> {
        let rules = load_rules(name)?;
        let listener = TcpListener::bind("127.0.0.1:0")?;
        let address = listener.local_addr()?;
        let calls: Calls = Arc::new(Mutex::new(Vec::new()));
        let recorded = Arc::clone(&calls);
        thread::spawn(move || {
            for mut stream in listener.incoming().flatten() {
                let _ = serve(&rules, &recorded, &mut stream);
            }
        });
        Ok(Self { address, calls })
    }

    fn endpoint(&self) -> String {
        format!("http://{}/", self.address)
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
}

fn record(request_id: u64, kind: u8, payload: &[u8]) -> Checked<Vec<u8>> {
    let mut out = request_id.to_be_bytes().to_vec();
    out.push(kind);
    out.extend_from_slice(&u32::try_from(payload.len())?.to_be_bytes());
    out.extend_from_slice(payload);
    Ok(out)
}

#[test]
fn a_request_record_decodes_only_in_its_canonical_form() -> Checked {
    let id = program(0xa0);
    let data = record(0x0102_0304_0506_0708, 2, b"paxeer network")?;
    assert_eq!(
        decode_request_record(id, 41, &data)?,
        ProgramRequest {
            program_id: id,
            request_id: 0x0102_0304_0506_0708,
            kind: 2,
            payload: b"paxeer network".to_vec(),
            sequence: 41,
        }
    );
    assert_eq!(
        decode_request_record(id, 41, &data[..12]),
        Err(KernelError::Record)
    );
    assert_eq!(
        decode_request_record(id, 41, &data[..data.len() - 1]),
        Err(KernelError::Record)
    );
    let mut longer = data.clone();
    longer.push(0);
    assert_eq!(
        decode_request_record(id, 41, &longer),
        Err(KernelError::Record)
    );
    assert_eq!(
        decode_request_record(id, 41, &record(1, 3, b"paxeer network")?),
        Err(KernelError::Record)
    );
    assert_eq!(
        decode_request_record(id, 41, &record(1, 1, b"")?),
        Err(KernelError::Record)
    );
    Ok(())
}

#[test]
fn a_program_attestation_matches_the_shared_origin_two_vector() -> Checked {
    let vectors =
        read_json(&repository().join("modules/xweb/types/testdata/preimage-vectors.json"))?;
    let vector = vectors
        .pointer("/vectors")
        .and_then(Value::as_array)
        .and_then(|list| {
            list.iter()
                .find(|vector| vector.get("name") == Some(&json!("program-search")))
        })
        .ok_or_else(|| fail("no program-search vector"))?;
    let text = |name: &str| -> Checked<String> {
        vector
            .get(name)
            .and_then(Value::as_str)
            .map(str::to_owned)
            .ok_or_else(|| fail(format!("vector has no {name}")))
    };
    let hex32 = |name: &str| -> Checked<[u8; 32]> {
        unhex0x(&text(name)?)
            .and_then(|bytes| bytes.try_into().ok())
            .ok_or_else(|| fail(format!("{name} is not 32 bytes")))
    };
    let request = ProgramRequest {
        program_id: hex32("requester")?,
        request_id: 42,
        kind: 2,
        payload: text("payload")?.into_bytes(),
        sequence: 0,
    };
    let attestation = request.attestation(
        1,
        hex32("content_digest")?,
        text("response")?.as_bytes(),
        5_000,
    );
    assert_eq!(attestation.origin, ORIGIN_PROGRAM);
    assert_eq!(attestation.network_id, network_word(1));
    assert_eq!(attestation.payload_hash, hex32("payload_hash")?);
    assert_eq!(attestation.response_hash, hex32("response_hash")?);
    assert_eq!(
        unhex0x(&text("preimage")?).ok_or_else(|| fail("preimage"))?,
        attestation.preimage().to_vec()
    );
    assert_eq!(attestation.digest(), hex32("digest")?);
    Ok(())
}

/// The shared adapter observation: program request 0x0102030405060708 of
/// program a0..bf on network 9, fetch of `https://paxeer.app/`.
fn adapter_request() -> ProgramRequest {
    ProgramRequest {
        program_id: program(0xa0),
        request_id: 0x0102_0304_0506_0708,
        kind: 1,
        payload: b"https://paxeer.app/".to_vec(),
        sequence: 0,
    }
}

fn adapter_ready(request: &ProgramRequest) -> Checked<Ready> {
    let response = b"Paxeer X Network".to_vec();
    let content_digest = fixed::<32>(CONTENT_DIGEST)?;
    let digest = request
        .attestation(NETWORK, content_digest, &response, 16)
        .digest();
    let signatures = OBSERVATION_SIGNATURES
        .iter()
        .map(|signature| fixed::<65>(signature))
        .collect::<Checked<Vec<_>>>()?;
    let signers = signatures
        .iter()
        .map(|signature| recover_signer(&digest, signature))
        .collect::<Result<Vec<_>, _>>()?;
    Ok(Ready {
        request_id: request.request_id,
        response,
        content_digest,
        full_length: 16,
        callback_gas: 0,
        digest,
        signers,
        signatures,
    })
}

#[test]
fn the_observation_activity_equals_the_kernel_adapter_fixture() -> Checked {
    let request = adapter_request();
    assert_eq!(
        request.attestation(NETWORK, [0; 32], b"", 0).payload_hash,
        fixed::<32>("62da65e513a2dc07e8c56bb6e148a96d6d259e496ec215d58c89522fa9649ce1")?
    );
    let ready = adapter_ready(&request)?;
    let registered = (1..=3)
        .map(|index| attestor_key(index).map(|key| signer_address(&key)))
        .collect::<Checked<Vec<_>>>()?;
    assert!(ready.signers.windows(2).all(|pair| pair[0] < pair[1]));
    assert!(ready
        .signers
        .iter()
        .all(|signer| registered.contains(signer)));
    assert_eq!(ready.signers.len(), 3);

    let observation = observation_bytes(NETWORK, &request, &ready)?;
    assert_eq!(
        observation.len(),
        OBSERVATION_HEADER_BYTES + 16 + 1 + 3 * 65
    );
    assert_eq!(observation[0], ORIGIN_PROGRAM);
    assert_eq!(&observation[29..33], &NETWORK.to_be_bytes());
    assert_eq!(&observation[33..65], &request.program_id);
    assert_eq!(&observation[65..73], &request.request_id.to_be_bytes());

    let options = ActivityOptions {
        network_id: NETWORK,
        actor_did: SUBMITTER_DID,
        account_sequence: 7,
        fee_limit: 100,
        not_before: 1_000,
        not_after: 61_000,
    };
    let activity = encode_activity(&observation, &submitter_key(), &options)?;
    let fixture =
        std::fs::read_to_string(repository().join("tests/fixtures/web/observation-activity.hex"))?;
    assert_eq!(hex(&activity), fixture.trim());

    for refused in [
        ActivityOptions {
            network_id: 0,
            ..options
        },
        ActivityOptions {
            fee_limit: 0,
            ..options
        },
        ActivityOptions {
            actor_did: "",
            ..options
        },
        ActivityOptions {
            not_before: 61_001,
            ..options
        },
    ] {
        assert_eq!(
            encode_activity(&observation, &submitter_key(), &refused),
            Err(KernelError::Encode)
        );
    }
    let mut unsigned = ready.clone();
    unsigned.signatures.clear();
    assert_eq!(
        observation_bytes(NETWORK, &request, &unsigned),
        Err(KernelError::Encode)
    );
    let mut short = ready.clone();
    short.full_length = 15;
    assert_eq!(
        observation_bytes(NETWORK, &request, &short),
        Err(KernelError::Encode)
    );
    let mut other = ready;
    other.request_id += 1;
    assert_eq!(
        observation_bytes(NETWORK, &request, &other),
        Err(KernelError::Encode)
    );
    Ok(())
}

fn events_params(from: u64) -> Value {
    json!([{ "topic": hex(REQUEST_TOPIC), "from_sequence": from, "limit": 256 }])
}

#[test]
fn the_watcher_follows_request_records_through_the_recorded_gateway() -> Checked {
    let scratch = Scratch::new("watch")?;
    let gateway = Gateway::start("watch.json")?;
    let state = scratch.0.join("state");
    let mut watcher = KernelWatcher::open(&gateway.endpoint(), &state, 40)?;
    assert_eq!(watcher.next_sequence(), 40);
    let requests = watcher.poll()?;
    assert_eq!(
        requests,
        vec![
            ProgramRequest {
                program_id: program(0xa0),
                request_id: 0x0102_0304_0506_0708,
                kind: 2,
                payload: b"paxeer network".to_vec(),
                sequence: 41,
            },
            ProgramRequest {
                program_id: program(0xb0),
                request_id: 5,
                kind: 1,
                payload: b"https://paxeer.app/".to_vec(),
                sequence: 43,
            },
        ]
    );
    assert_eq!(watcher.next_sequence(), 44);
    assert_eq!(watcher.poll()?, Vec::new());
    assert_eq!(watcher.next_sequence(), 44);
    assert_eq!(
        gateway.params(EVENTS_METHOD),
        vec![events_params(40), events_params(44)]
    );
    let reopened = KernelWatcher::open(&gateway.endpoint(), &state, 0)?;
    assert_eq!(reopened.next_sequence(), 44);

    let mut foreign = KernelWatcher::open(&gateway.endpoint(), &scratch.0.join("foreign"), 60)?;
    assert_eq!(foreign.poll(), Err(KernelError::Malformed));
    assert_eq!(foreign.next_sequence(), 60);
    let mut unknown = KernelWatcher::open(&gateway.endpoint(), &scratch.0.join("unknown"), 70)?;
    assert_eq!(unknown.poll(), Err(KernelError::Record));
    assert_eq!(unknown.next_sequence(), 70);
    let mut rejected = KernelWatcher::open(&gateway.endpoint(), &scratch.0.join("rejected"), 80)?;
    assert_eq!(
        rejected.poll(),
        Err(KernelError::Rejected { code: -32_601 })
    );
    let mut absent = KernelWatcher::open(&gateway.endpoint(), &scratch.0.join("absent"), 90)?;
    assert_eq!(absent.poll(), Err(KernelError::Unavailable));
    assert!(KernelWatcher::open("ftp://gateway/", &scratch.0.join("bad"), 0).is_err());
    Ok(())
}

#[test]
fn the_relay_answers_a_program_request_and_posts_its_observation() -> Checked {
    let scratch = Scratch::new("relay")?;
    let gateway = Gateway::start("relay.json")?;
    let data = scratch.0.join("data");
    let store = Arc::new(ContentStore::open(&data, &[])?);
    let index = Arc::new(WebIndex::open(&data)?);
    index.put(
        "https://paxeer.app/",
        "Paxeer",
        "Paxeer X Network web search for programs",
    )?;
    index.commit()?;
    let fetcher = Arc::new(Fetcher::new(FetchLimits {
        connect_timeout_ms: 3_000,
        total_timeout_ms: 10_000,
        max_body_bytes: 2_097_152,
        max_redirects: 3,
        allow_loopback: true,
    })?);
    let key = attestor_key(1)?;
    let attestor = KernelAttestor::new(
        key.clone(),
        NETWORK,
        fetcher,
        Arc::clone(&index),
        Arc::clone(&store),
    );
    assert_eq!(attestor.signer(), signer_address(&key));
    let set = AttestorSet {
        signers: vec![signer_address(&key)],
        threshold: 1,
    };
    let mut relay = KernelRelay::new(
        KernelWatcher::open(&gateway.endpoint(), &scratch.0.join("state"), 0)?,
        attestor,
        SignatureExchange::open(&scratch.0.join("exchange"), &[])?,
        set,
        ObservationSubmitter::new(
            &gateway.endpoint(),
            submitter_key(),
            SUBMITTER_DID.to_owned(),
            NETWORK,
            100,
        )?,
    );
    let steps = relay.step(2_000)?;
    assert_eq!(
        steps,
        vec![Step::Posted {
            program_id: program(0xa0),
            request_id: 0x0102_0304_0506_0708,
            result: json!({ "state": "executed" }),
        }]
    );
    assert!(relay.queued().is_empty());
    assert!(relay.exchange.pending().is_empty());

    let query = "paxeer network";
    let results: Vec<search::SearchResult> = search::search(&index, query)?
        .into_iter()
        .map(|scored| scored.result)
        .collect();
    assert_eq!(results.len(), 1);
    let content_digest = store
        .put(&search::search_canonical_bytes(query, &results)?)
        .map_err(|error| fail(error.to_string()))?;
    let (response, full_length) = stored_response(&search::search_text(&results)?)?;
    let request = ProgramRequest {
        program_id: program(0xa0),
        request_id: 0x0102_0304_0506_0708,
        kind: 2,
        payload: query.as_bytes().to_vec(),
        sequence: 3,
    };
    let digest = request
        .attestation(NETWORK, content_digest, &response, full_length)
        .digest();
    let ready = Ready {
        request_id: request.request_id,
        response,
        content_digest,
        full_length,
        callback_gas: 0,
        digest,
        signers: vec![signer_address(&key)],
        signatures: vec![sign_digest(&key, &digest)?],
    };
    let observation = observation_bytes(NETWORK, &request, &ready)?;
    assert_eq!(&observation[74..106], &keccak(query.as_bytes()));
    let activity = encode_activity(
        &observation,
        &submitter_key(),
        &ActivityOptions {
            network_id: NETWORK,
            actor_did: SUBMITTER_DID,
            account_sequence: 7,
            fee_limit: 100,
            not_before: 1_000,
            not_after: 62_000,
        },
    )?;
    assert_eq!(
        gateway.params("lx_getSequence"),
        vec![json!([SUBMITTER_DID, "identity"])]
    );
    assert_eq!(
        gateway.params("lx_sendActivity"),
        vec![json!([hex(&activity), "executed"])]
    );
    Ok(())
}
