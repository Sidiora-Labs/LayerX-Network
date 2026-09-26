//! Test transport and signer. `Recording` replays recorded JSON-RPC exchanges
//! from `tests/fixtures/*.json`; everything above the transport — log and
//! ABI decoding, digests, journal, transaction assembly — is the relayer's
//! real code. `SignerServer` speaks `interop/deploy/mirror/signer-protocol.md`
//! over a real Unix socket so the relayer's remote signer client is exercised
//! end to end, with per-handle policy domains.

#![allow(dead_code)]

use std::collections::BTreeMap;
use std::fs;
use std::io::{Read as _, Write as _};
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use k256::ecdsa::SigningKey;
use layerx_bridge_relayer::hex;
use layerx_bridge_relayer::rpc::{JsonRpc, RpcFault, SendOutcome};
use layerx_mirror::signer::{
    RemoteChainSigner, RemoteSignerConfig, SignerEndpoint, SigningAlgorithm,
};
use serde_json::Value;
use sha3::{Digest as _, Keccak256};

#[derive(Clone, Debug)]
struct Rule {
    method: String,
    params: Value,
    result: Option<Value>,
    error: Option<(i64, String)>,
    fault: bool,
    once: bool,
    used: bool,
}

#[derive(Default)]
struct RecordingState {
    /// endpoint -> phase -> rules
    endpoints: BTreeMap<String, BTreeMap<String, Vec<Rule>>>,
    phase: String,
    /// Every raw transaction broadcast, in order, rebroadcasts included.
    sent: Vec<(String, Vec<u8>)>,
    /// Distinct transaction hashes in first-broadcast order.
    submitted: Vec<String>,
    unmatched: Vec<String>,
    calls: Vec<(String, String, Value)>,
}

fn parse_rule(value: &Value) -> Rule {
    let method = value
        .get("method")
        .and_then(Value::as_str)
        .unwrap_or_else(|| panic!("fixture rule without method: {value}"))
        .to_owned();
    let error = value.get("error").map(|error| {
        (
            error
                .get("code")
                .and_then(Value::as_i64)
                .unwrap_or_default(),
            error
                .get("message")
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_owned(),
        )
    });
    Rule {
        method,
        params: value.get("params").cloned().unwrap_or(Value::Null),
        result: value.get("result").cloned(),
        error,
        fault: value.get("fault").and_then(Value::as_str) == Some("unavailable"),
        once: value.get("once").and_then(Value::as_bool) == Some(true),
        used: false,
    }
}

/// A recorded exchange shared by every endpoint and relayer instance of one
/// test, so a relayer "restart" replays against the same recording.
#[derive(Clone)]
pub struct Recording {
    state: Arc<Mutex<RecordingState>>,
}

impl Recording {
    pub fn load(name: &str) -> Self {
        let path = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fixtures")
            .join(name);
        let text = fs::read_to_string(&path)
            .unwrap_or_else(|error| panic!("fixture {}: {error}", path.display()));
        let document: Value =
            serde_json::from_str(&text).unwrap_or_else(|error| panic!("fixture json: {error}"));
        let mut state = RecordingState {
            phase: "*".to_owned(),
            ..RecordingState::default()
        };
        let endpoints = document
            .get("endpoints")
            .and_then(Value::as_object)
            .unwrap_or_else(|| panic!("fixture without endpoints"));
        for (endpoint, phases) in endpoints {
            let phases = phases
                .as_object()
                .unwrap_or_else(|| panic!("endpoint {endpoint} without phases"));
            let mut parsed = BTreeMap::new();
            for (phase, rules) in phases {
                let rules = rules
                    .as_array()
                    .unwrap_or_else(|| panic!("phase {phase} is not a list"))
                    .iter()
                    .map(parse_rule)
                    .collect();
                parsed.insert(phase.clone(), rules);
            }
            state.endpoints.insert(endpoint.clone(), parsed);
        }
        Self {
            state: Arc::new(Mutex::new(state)),
        }
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, RecordingState> {
        self.state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    pub fn set_phase(&self, phase: &str) {
        phase.clone_into(&mut self.lock().phase);
    }

    pub fn endpoint(&self, name: &str) -> Box<dyn JsonRpc> {
        assert!(
            self.lock().endpoints.contains_key(name),
            "fixture has no endpoint {name}"
        );
        Box::new(FixtureRpc {
            recording: self.clone(),
            name: name.to_owned(),
        })
    }

    /// Raw transactions broadcast so far, rebroadcasts included.
    pub fn sent(&self) -> Vec<(String, Vec<u8>)> {
        self.lock().sent.clone()
    }

    pub fn unmatched(&self) -> Vec<String> {
        self.lock().unmatched.clone()
    }

    /// How many calls of `method` reached `endpoint`.
    pub fn count(&self, endpoint: &str, method: &str) -> usize {
        self.lock()
            .calls
            .iter()
            .filter(|(name, called, _)| name == endpoint && called == method)
            .count()
    }
}

fn substitute(value: &Value, submitted: &[String]) -> Value {
    match value {
        Value::String(text) => text
            .strip_prefix("$submitted:")
            .and_then(|index| index.parse::<usize>().ok())
            .map_or_else(
                || value.clone(),
                |index| {
                    Value::String(
                        submitted
                            .get(index)
                            .cloned()
                            .unwrap_or_else(|| "$unsubmitted".to_owned()),
                    )
                },
            ),
        Value::Array(values) => Value::Array(
            values
                .iter()
                .map(|value| substitute(value, submitted))
                .collect(),
        ),
        Value::Object(map) => Value::Object(
            map.iter()
                .map(|(key, value)| (key.clone(), substitute(value, submitted)))
                .collect(),
        ),
        other => other.clone(),
    }
}

fn matches(pattern: &Value, actual: &Value) -> bool {
    match (pattern, actual) {
        (Value::String(text), _) if text == "$any" => true,
        (Value::Array(left), Value::Array(right)) => {
            left.len() == right.len() && left.iter().zip(right).all(|(l, r)| matches(l, r))
        }
        (Value::Object(left), Value::Object(right)) => {
            left.len() == right.len()
                && left
                    .iter()
                    .all(|(key, value)| right.get(key).is_some_and(|other| matches(value, other)))
        }
        _ => pattern == actual,
    }
}

enum Answer {
    Result(Value),
    Error(i64, String),
    Fault,
}

pub struct FixtureRpc {
    recording: Recording,
    name: String,
}

impl FixtureRpc {
    fn answer(&self, method: &str, params: &Value) -> Option<Answer> {
        let mut state = self.recording.lock();
        state
            .calls
            .push((self.name.clone(), method.to_owned(), params.clone()));
        let submitted = state.submitted.clone();
        let phase = state.phase.clone();
        let phases = state.endpoints.get_mut(&self.name)?;
        for phase in [phase.as_str(), "*"] {
            let Some(rules) = phases.get_mut(phase) else {
                continue;
            };
            for rule in rules.iter_mut() {
                if rule.used
                    || rule.method != method
                    || !matches(&substitute(&rule.params, &submitted), params)
                {
                    continue;
                }
                if rule.once {
                    rule.used = true;
                }
                if rule.fault {
                    return Some(Answer::Fault);
                }
                if let Some((code, message)) = &rule.error {
                    return Some(Answer::Error(*code, message.clone()));
                }
                return Some(Answer::Result(substitute(
                    rule.result.as_ref().unwrap_or(&Value::Null),
                    &submitted,
                )));
            }
        }
        None
    }

    fn miss(&self, method: &str, params: &Value) -> RpcFault {
        let mut state = self.recording.lock();
        let phase = state.phase.clone();
        state
            .unmatched
            .push(format!("{} [{phase}] {method} {params}", self.name));
        RpcFault::Unavailable
    }
}

impl JsonRpc for FixtureRpc {
    fn call(&self, method: &str, params: Value) -> Result<Value, RpcFault> {
        match self.answer(method, &params) {
            Some(Answer::Result(value)) => Ok(value),
            Some(Answer::Error(code, message)) => Err(RpcFault::Rejected { code, message }),
            Some(Answer::Fault) => Err(RpcFault::Unavailable),
            None => Err(self.miss(method, &params)),
        }
    }

    fn send_raw_transaction(&self, raw: &[u8], hash: &[u8; 32]) -> Result<SendOutcome, RpcFault> {
        let computed: [u8; 32] = Keccak256::digest(raw).into();
        assert_eq!(&computed, hash, "the relayer's hash must be keccak(raw)");
        let hash = hex::prefixed(hash);
        {
            let mut state = self.recording.lock();
            state.sent.push((self.name.clone(), raw.to_vec()));
            if !state.submitted.contains(&hash) {
                state.submitted.push(hash.clone());
            }
        }
        let params = Value::Array(vec![Value::String(hex::prefixed(raw))]);
        match self.answer("eth_sendRawTransaction", &params) {
            Some(Answer::Result(Value::String(text))) if text == "$hash" => {
                Ok(SendOutcome::Accepted)
            }
            Some(Answer::Result(Value::String(text))) if text == hash => Ok(SendOutcome::Accepted),
            Some(Answer::Result(_)) => Ok(SendOutcome::Unknown),
            Some(Answer::Error(code, message)) => Err(RpcFault::Rejected { code, message }),
            Some(Answer::Fault) => Err(RpcFault::Unavailable),
            None => Err(self.miss("eth_sendRawTransaction", &params)),
        }
    }
}

/// A test key: 31 zero bytes and `last`.
pub fn key(last: u8) -> SigningKey {
    let mut secret = [0_u8; 32];
    secret[31] = last;
    SigningKey::from_slice(&secret).unwrap_or_else(|error| panic!("test key: {error}"))
}

pub fn secret(last: u8) -> [u8; 32] {
    let mut secret = [0_u8; 32];
    secret[31] = last;
    secret
}

pub fn public_key(key: &SigningKey) -> Vec<u8> {
    key.verifying_key()
        .to_encoded_point(true)
        .as_bytes()
        .to_vec()
}

struct SignerKey {
    key: SigningKey,
    domains: Vec<Vec<u8>>,
}

/// One request the test signer served: (handle, domain, digest).
pub type SignerRequest = (String, Vec<u8>, [u8; 32]);

/// A signer daemon for tests: each handle owns one key and may sign under one
/// policy domain only; anything else is refused.
pub struct SignerServer {
    pub socket: PathBuf,
    requests: Arc<Mutex<Vec<SignerRequest>>>,
}

fn read_frame(stream: &mut UnixStream) -> Option<Vec<u8>> {
    let mut length = [0_u8; 4];
    stream.read_exact(&mut length).ok()?;
    let length = usize::try_from(u32::from_be_bytes(length)).ok()?;
    if length > 8192 {
        return None;
    }
    let mut request = vec![0_u8; length];
    stream.read_exact(&mut request).ok()?;
    Some(request)
}

struct Request {
    handle: String,
    domain: Vec<u8>,
    digest: [u8; 32],
}

fn parse_request(request: &[u8]) -> Option<Request> {
    let mut cursor = request.strip_prefix(b"LXCS")?;
    let take = |cursor: &mut &[u8], count: usize| -> Option<Vec<u8>> {
        let (head, tail) = cursor.split_at_checked(count)?;
        *cursor = tail;
        Some(head.to_vec())
    };
    let version = take(&mut cursor, 2)?;
    let algorithm = take(&mut cursor, 1)?;
    if version != [0, 1] || algorithm != [1] {
        return None;
    }
    let handle_length = take(&mut cursor, 2)?;
    let handle = take(
        &mut cursor,
        usize::from(u16::from_be_bytes([handle_length[0], handle_length[1]])),
    )?;
    let domain_length = take(&mut cursor, 2)?;
    let domain = take(
        &mut cursor,
        usize::from(u16::from_be_bytes([domain_length[0], domain_length[1]])),
    )?;
    let digest: [u8; 32] = take(&mut cursor, 32)?.try_into().ok()?;
    let message_length = take(&mut cursor, 4)?;
    if message_length != [0, 0, 0, 0] || !cursor.is_empty() {
        return None;
    }
    Some(Request {
        handle: String::from_utf8(handle).ok()?,
        domain,
        digest,
    })
}

impl SignerServer {
    /// Serves `keys` (handle, key, allowed policy domains) on a fresh socket.
    pub fn start(name: &str, keys: Vec<(&str, SigningKey, Vec<&[u8]>)>) -> Self {
        let directory = work_directory(&format!("signer-{name}"));
        let socket = directory.join("s.sock");
        let listener = UnixListener::bind(&socket)
            .unwrap_or_else(|error| panic!("signer socket {}: {error}", socket.display()));
        let keys: BTreeMap<String, SignerKey> = keys
            .into_iter()
            .map(|(handle, key, domains)| {
                (
                    handle.to_owned(),
                    SignerKey {
                        key,
                        domains: domains.into_iter().map(<[u8]>::to_vec).collect(),
                    },
                )
            })
            .collect();
        let requests = Arc::new(Mutex::new(Vec::new()));
        let log = Arc::clone(&requests);
        std::thread::spawn(move || {
            for stream in listener.incoming() {
                let Ok(mut stream) = stream else { continue };
                let Some(frame) = read_frame(&mut stream) else {
                    continue;
                };
                let response = match parse_request(&frame) {
                    Some(request) => {
                        log.lock()
                            .unwrap_or_else(std::sync::PoisonError::into_inner)
                            .push((
                                request.handle.clone(),
                                request.domain.clone(),
                                request.digest,
                            ));
                        match keys.get(&request.handle) {
                            Some(entry) if entry.domains.contains(&request.domain) => {
                                match entry.key.sign_prehash_recoverable(&request.digest) {
                                    Ok((signature, recovery)) => {
                                        let mut response = vec![0_u8];
                                        response.extend_from_slice(&signature.to_bytes());
                                        response.push(recovery.to_byte());
                                        response
                                    }
                                    Err(_) => vec![1],
                                }
                            }
                            _ => vec![1],
                        }
                    }
                    None => vec![1],
                };
                let length = u32::try_from(response.len()).unwrap_or_default();
                let _ = stream
                    .write_all(&length.to_be_bytes())
                    .and_then(|()| stream.write_all(&response));
            }
        });
        Self { socket, requests }
    }

    /// Requests served so far: (handle, domain, digest).
    pub fn requests(&self) -> Vec<SignerRequest> {
        self.requests
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .clone()
    }

    pub fn remote(&self, handle: &str, key: &SigningKey) -> RemoteChainSigner {
        RemoteChainSigner::new(RemoteSignerConfig {
            endpoint: SignerEndpoint::Uds {
                socket: self.socket.clone(),
            },
            algorithm: SigningAlgorithm::Secp256k1Recoverable,
            key_handle: handle.to_owned(),
            public_key: public_key(key),
            timeout: Duration::from_secs(5),
        })
        .unwrap_or_else(|error| panic!("remote signer {handle}: {error:?}"))
    }
}

pub fn work_directory(name: &str) -> PathBuf {
    let directory = std::env::temp_dir().join(format!("lxbr-{}-{name}", std::process::id()));
    let _ = fs::remove_dir_all(&directory);
    fs::create_dir_all(&directory)
        .unwrap_or_else(|error| panic!("work directory {}: {error}", directory.display()));
    directory
}
