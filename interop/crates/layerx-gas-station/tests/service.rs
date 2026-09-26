use std::collections::BTreeMap;
use std::io::{Read as _, Write as _};
use std::net::{SocketAddr, TcpListener, TcpStream};
use std::path::PathBuf;
use std::sync::mpsc;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use k256::ecdsa::SigningKey;
use k256::elliptic_curve::rand_core::OsRng;
use layerx_gas_station::config::{ServiceConfig, StationConfig};
use layerx_gas_station::journal::Journal;
use layerx_gas_station::price::PaymasterRateSource;
use layerx_gas_station::quote::{
    address_word, keccak, quote_digest, word, Address, Quote, SIDIORA,
};
use layerx_gas_station::rpc::{bytes, hex, ConfiguredRpc, Exchange, HttpsExchange, RpcFault};
use layerx_gas_station::service::{serve, Limits, Service};
use layerx_gas_station::signer::{LocalSigner, QuoteSigner};
use layerx_gas_station::station::GasStation;
use layerx_gas_station::tx::{
    authorization_digest, batch_digest, encode_sponsored, recover, Authorization, Call,
};
use layerx_gas_station::SignedQuote;
use serde_json::{json, Value};

type TestResult = Result<(), Box<dyn std::error::Error>>;

const CHAIN_ID: u64 = 1325;
const PAYMASTER: Address = [0x44; 20];
const AMOUNT: u128 = 3_145_140;
const GAS_COST: &str = "1000000000000000000";

struct Recording {
    fixture: Value,
    phase: Mutex<String>,
    bindings: Mutex<BTreeMap<String, Value>>,
    sent: Mutex<Vec<Vec<u8>>>,
}
impl Recording {
    fn phase(&self, phase: &str) {
        if let Ok(mut current) = self.phase.lock() {
            *current = phase.into();
        }
    }
    fn bind(&self, name: &str, value: Value) {
        if let Ok(mut bindings) = self.bindings.lock() {
            bindings.insert(name.into(), value);
        }
    }
    fn expect_quote_nonce(&self, sponsor: Address, nonce: u128) {
        let consumed = [
            keccak(b"usedQuoteNonces(address,uint256)")[..4].to_vec(),
            address_word(sponsor).to_vec(),
            word(nonce).to_vec(),
        ]
        .concat();
        self.bind("$consumed_call", json!(hex(&consumed)));
    }
    fn sent(&self) -> Vec<Vec<u8>> {
        self.sent
            .lock()
            .map(|sent| sent.clone())
            .unwrap_or_default()
    }
}
fn substitute(value: &Value, bindings: &BTreeMap<String, Value>) -> Value {
    match value {
        Value::String(s) if s.starts_with('$') => bindings
            .get(s)
            .cloned()
            .unwrap_or_else(|| panic!("unbound fixture field")),
        Value::Array(values) => {
            Value::Array(values.iter().map(|v| substitute(v, bindings)).collect())
        }
        Value::Object(values) => Value::Object(
            values
                .iter()
                .map(|(k, v)| (k.clone(), substitute(v, bindings)))
                .collect(),
        ),
        _ => value.clone(),
    }
}

/// The recorded node: every exchange is answered from tests/fixtures/cycle.json,
/// except in the `unreachable` phase, where the request goes to a real HTTPS
/// exchange aimed at a closed loopback port.
struct ReplayExchange(Arc<Recording>);
impl Exchange for ReplayExchange {
    fn request(&self, _endpoint: &str, method: &str, params: &Value) -> Result<Value, RpcFault> {
        let phase = self
            .0
            .phase
            .lock()
            .map_err(|_| RpcFault::Unavailable)?
            .clone();
        if phase == "unreachable" {
            return HttpsExchange.request("https://127.0.0.1:1", method, params);
        }
        if method == "eth_sendRawTransaction" {
            let raw = bytes(params[0].as_str().ok_or(RpcFault::Malformed)?)?;
            self.0.bind("$raw", json!(hex(&raw)));
            self.0.bind("$hash", json!(hex(&keccak(&raw))));
            self.0
                .sent
                .lock()
                .map_err(|_| RpcFault::Unavailable)?
                .push(raw);
        }
        let bindings = self
            .0
            .bindings
            .lock()
            .map_err(|_| RpcFault::Unavailable)?
            .clone();
        for name in [phase.as_str(), "*"] {
            let rules = self.0.fixture["phases"][name]
                .as_array()
                .ok_or(RpcFault::Malformed)?;
            for rule in rules {
                if rule["method"] == method && substitute(&rule["params"], &bindings) == *params {
                    if rule["fault"] == "unavailable" {
                        return Err(RpcFault::Unavailable);
                    }
                    if let Some(code) = rule["error"]["code"].as_i64() {
                        return Err(RpcFault::Rejected { code });
                    }
                    return Ok(substitute(&rule["result"], &bindings));
                }
            }
        }
        panic!("unmatched recorded exchange: {method}")
    }
}

#[derive(Clone)]
struct SharedLog(Arc<Mutex<Vec<u8>>>);
impl std::io::Write for SharedLog {
    fn write(&mut self, data: &[u8]) -> std::io::Result<usize> {
        self.0
            .lock()
            .map_err(|_| std::io::Error::other("log poisoned"))?
            .extend_from_slice(data);
        Ok(data.len())
    }
    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

fn wire_calls() -> Value {
    json!([{"to": hex(&[0x55; 20]), "data": "0x010203", "value": "0"}])
}

struct Harness {
    address: SocketAddr,
    recorded: Arc<Recording>,
    account: LocalSigner,
    sponsor: Address,
    secrets: Vec<String>,
    log: SharedLog,
    journal: PathBuf,
}

fn station_config(name: &str) -> StationConfig {
    StationConfig {
        chain_id: CHAIN_ID,
        endpoints: vec![format!("https://{}", std::process::id())],
        paymaster: PAYMASTER,
        token: SIDIORA,
        decimals: 6,
        max_rate_age: 300,
        spread_bps: 500,
        margin_bps: 100,
        per_account_limit: 8_000_000,
        per_interval_limit: 16_000_000,
        per_quote_limit: 4_000_000,
        interval_seconds: 60,
        balance_floor: 100,
        relayer_key_env: format!("PAXEER_SERVICE_{name}_KEY"),
    }
}

fn ephemeral_signer(
    config: &StationConfig,
) -> Result<(LocalSigner, String), Box<dyn std::error::Error>> {
    let secret = hex(&SigningKey::random(&mut OsRng).to_bytes());
    std::env::set_var(&config.relayer_key_env, &secret);
    let signer = LocalSigner::from_config(config);
    std::env::remove_var(&config.relayer_key_env);
    Ok((signer?, secret))
}

/// Builds the real service over the recorded node inside its own thread and
/// serves it on `listener`, returning once it is ready or refused to start.
fn spawn_service(
    config: ServiceConfig,
    signer: LocalSigner,
    recorded: Arc<Recording>,
    journal: PathBuf,
    listener: TcpListener,
    mut log: SharedLog,
) -> TestResult {
    let (ready, started) = mpsc::channel();
    std::thread::spawn(move || {
        let built = (|| -> Result<_, Box<dyn std::error::Error>> {
            let rpc = ConfiguredRpc::new(&config.station, ReplayExchange(Arc::clone(&recorded)))?;
            let rates = PaymasterRateSource::new(
                ConfiguredRpc::new(&config.station, ReplayExchange(recorded))?,
                config.station.paymaster,
            );
            let station = GasStation::new(
                config.station.clone(),
                signer,
                rpc,
                rates,
                Journal::open(&journal)?,
            )?;
            Ok(Service::new(
                &config,
                station,
                || Some(1000),
                Limits {
                    max_body: 65_536,
                    read_time: Duration::from_millis(500),
                },
            ))
        })();
        match built {
            Ok(mut service) => {
                if ready.send(Ok(())).is_ok() {
                    let _ = serve(&listener, &mut service, &mut log);
                }
            }
            Err(error) => {
                let _ = ready.send(Err(error.to_string()));
            }
        }
    });
    started.recv_timeout(Duration::from_secs(10))??;
    Ok(())
}

fn start(name: &str) -> Result<Harness, Box<dyn std::error::Error>> {
    let station = station_config(name);
    let (signer, relayer_secret) = ephemeral_signer(&station)?;
    let (account, account_secret) = ephemeral_signer(&station)?;
    let sponsor = signer.address();
    let journal = std::env::temp_dir().join(format!(
        "paxeer-service-{}-{}.jsonl",
        name.to_ascii_lowercase(),
        std::process::id()
    ));
    if journal.exists() {
        std::fs::remove_file(&journal)?;
    }
    let bindings = [
        ("$sponsor", hex(&sponsor)),
        ("$paymaster", hex(&PAYMASTER)),
        ("$current_rate_call", hex(&keccak(b"currentRate()")[..4])),
        ("$rate_updated_call", hex(&keccak(b"rateUpdatedAt()")[..4])),
        ("$account", hex(&account.address())),
        ("$zero", hex(&word(0))),
        ("$one", hex(&word(1))),
        ("$batch_nonce_call", hex(&keccak(b"nonce()")[..4])),
    ]
    .into_iter()
    .map(|(k, v)| (k.to_owned(), json!(v)))
    .collect();
    let recorded = Arc::new(Recording {
        fixture: serde_json::from_str(include_str!("fixtures/cycle.json"))?,
        phase: Mutex::new("submit".into()),
        bindings: Mutex::new(bindings),
        sent: Mutex::new(vec![]),
    });
    recorded.expect_quote_nonce(sponsor, 1);
    let listener = TcpListener::bind("127.0.0.1:0")?;
    let address = listener.local_addr()?;
    let config = ServiceConfig {
        station,
        listen: address,
        gas_limit: 200_000,
        max_priority_fee_per_gas: 1_000_000_000,
    };
    let log = SharedLog(Arc::new(Mutex::new(Vec::new())));
    spawn_service(
        config,
        signer,
        Arc::clone(&recorded),
        journal.clone(),
        listener,
        log.clone(),
    )?;
    Ok(Harness {
        address,
        recorded,
        account,
        sponsor,
        secrets: vec![relayer_secret, account_secret],
        log,
        journal,
    })
}

impl Harness {
    fn exchange(&self, request: &[u8]) -> Result<(u16, Value), Box<dyn std::error::Error>> {
        let mut stream = TcpStream::connect(self.address)?;
        stream.set_read_timeout(Some(Duration::from_secs(10)))?;
        stream.write_all(request)?;
        let mut response = Vec::new();
        stream.read_to_end(&mut response)?;
        let split = response
            .windows(4)
            .position(|w| w == b"\r\n\r\n")
            .ok_or("response without head")?;
        let head = std::str::from_utf8(&response[..split])?;
        let status = head
            .split(' ')
            .nth(1)
            .ok_or("response without status")?
            .parse()?;
        let body = &response[split + 4..];
        assert!(head.contains(&format!("Content-Length: {}", body.len())));
        Ok((status, serde_json::from_slice(body)?))
    }

    fn post(&self, path: &str, body: &Value) -> Result<(u16, Value), Box<dyn std::error::Error>> {
        let body = body.to_string();
        self.exchange(
            format!(
                "POST {path} HTTP/1.1\r\nHost: station\r\nContent-Type: application/json\r\nContent-Length: {}\r\n\r\n{body}",
                body.len()
            )
            .as_bytes(),
        )
    }

    /// The body `requestGasQuote` in the TypeScript SDK sends.
    fn quote_body(&self) -> Value {
        json!({
            "account": hex(&self.account.address()),
            "nonce": "0",
            "calls": wire_calls(),
            "maxTokenAmount": "3200000",
            "gasCost": GAS_COST,
            "chainId": "1325",
            "token": "0x21f7b20a555199fa73A238B1a91FD0f549068fEe",
            "decimals": 6,
        })
    }

    fn quoted(&self, response: &Value) -> Result<SignedQuote, Box<dyn std::error::Error>> {
        let quote = &response["quote"];
        let number = |name: &str| -> Result<u128, Box<dyn std::error::Error>> {
            Ok(quote[name]
                .as_str()
                .ok_or("numeric field not a string")?
                .parse()?)
        };
        let address = |name: &str| -> Result<Address, Box<dyn std::error::Error>> {
            Ok(bytes(quote[name].as_str().ok_or("address not a string")?)?
                .try_into()
                .map_err(|_| "address length")?)
        };
        let quote = Quote {
            sponsor: address("sponsor")?,
            token: address("token")?,
            max_token_amount: word(number("maxTokenAmount")?),
            token_amount: word(number("tokenAmount")?),
            deadline: word(number("deadline")?),
            nonce: word(number("quoteNonce")?),
            gas_cost: word(number("gasCost")?),
        };
        let signature: [u8; 65] = bytes(
            response["relayerSignature"]
                .as_str()
                .ok_or("signature not a string")?,
        )?
        .try_into()
        .map_err(|_| "signature length")?;
        Ok(SignedQuote {
            digest: quote_digest(word(u128::from(CHAIN_ID)), self.account.address(), &quote),
            quote,
            signature,
        })
    }

    /// The body `sendSponsoredBatch` in the web application sends.
    fn submission_body(&self, quote_response: &Value) -> Result<Value, Box<dyn std::error::Error>> {
        let signed = self.quoted(quote_response)?;
        let calls = vec![Call {
            to: [0x55; 20],
            value: word(0),
            data: vec![1, 2, 3],
        }];
        let account_signature = self.account.sign_digest(batch_digest(
            CHAIN_ID,
            self.account.address(),
            word(0),
            &calls,
            &signed.quote,
        )?)?;
        let mut authorization = Authorization {
            chain_id: CHAIN_ID,
            delegate: PAYMASTER,
            nonce: 0,
            signature: [0; 65],
        };
        let auth_signature = self
            .account
            .sign_digest(authorization_digest(&authorization)?)?;
        authorization.signature = auth_signature;
        let data = encode_sponsored(&calls, &signed, &account_signature)?;
        Ok(json!({
            "call": {"to": hex(&self.account.address()), "value": "0", "data": hex(&data)},
            "authorization": {
                "chainId": "1325",
                "address": hex(&PAYMASTER),
                "nonce": "0",
                "yParity": auth_signature[64] - 27,
                "r": hex(&auth_signature[..32]),
                "s": hex(&auth_signature[32..64]),
            },
            "batch": {
                "chainId": "1325",
                "account": hex(&self.account.address()),
                "nonce": "0",
                "calls": wire_calls(),
                "quote": quote_response["quote"],
            },
            "accountSignature": hex(&account_signature),
            "relayerSignature": quote_response["relayerSignature"],
        }))
    }

    fn log(&self) -> String {
        self.log
            .0
            .lock()
            .map(|log| String::from_utf8_lossy(&log).into_owned())
            .unwrap_or_default()
    }

    fn assert_log_clean(&self, lines: &[&str]) {
        let log = self.log();
        assert_eq!(log.lines().collect::<Vec<_>>(), lines);
        for secret in &self.secrets {
            assert!(!log.contains(secret.trim_start_matches("0x")));
        }
        for raw in self.recorded.sent() {
            assert!(!log.contains(hex(&raw).trim_start_matches("0x")));
        }
    }

    fn journal_is_empty(&self) -> Result<bool, Box<dyn std::error::Error>> {
        Ok(std::fs::read_to_string(&self.journal)?.is_empty())
    }

    fn finish(self) -> TestResult {
        std::fs::remove_file(&self.journal)?;
        Ok(())
    }
}

#[test]
fn quote_and_sponsored_submission_over_a_socket() -> TestResult {
    let harness = start("CYCLE")?;
    let (status, quote) = harness.post("/quote", &harness.quote_body())?;
    assert_eq!(status, 200);
    let expected = json!({
        "sponsor": hex(&harness.sponsor),
        "token": hex(&SIDIORA),
        "maxTokenAmount": "3200000",
        "tokenAmount": AMOUNT.to_string(),
        "deadline": "1019",
        "quoteNonce": "1",
        "gasCost": GAS_COST,
        "decimals": 6,
    });
    assert_eq!(quote["quote"], expected);
    assert_eq!(quote.as_object().map(serde_json::Map::len), Some(2));
    let signed = harness.quoted(&quote)?;
    assert_eq!(recover(signed.digest, &signed.signature)?, harness.sponsor);

    let (status, submitted) = harness.post("/submit", &harness.submission_body(&quote)?)?;
    assert_eq!(status, 200);
    let sent = harness.recorded.sent();
    assert_eq!(sent.len(), 1);
    assert_eq!(sent[0][0], 4);
    assert_eq!(
        submitted,
        json!({"transactionHash": hex(&keccak(&sent[0]))})
    );
    let submitted = submitted.to_string();
    assert!(!submitted.contains(hex(&sent[0]).trim_start_matches("0x")));
    for secret in &harness.secrets {
        assert!(!submitted.contains(secret.trim_start_matches("0x")));
        assert!(!quote.to_string().contains(secret.trim_start_matches("0x")));
    }
    harness.assert_log_clean(&["/quote 200", "/submit 200"]);
    harness.finish()
}

#[test]
fn quote_refusals_are_4xx_and_unavailability_is_5xx() -> TestResult {
    let harness = start("SPLIT")?;
    let mut lines = Vec::new();
    for (field, value) in [
        ("chainId", json!("1")),
        ("token", json!(hex(&[0x66; 20]))),
        ("decimals", json!(18)),
        ("gasCost", json!("1000000000000000001")),
        ("maxTokenAmount", json!("1")),
    ] {
        let mut body = harness.quote_body();
        body[field] = value;
        let (status, answer) = harness.post("/quote", &body)?;
        assert_eq!((status, answer), (422, json!({"error": "refused"})));
        lines.push("/quote 422");
    }
    let mut body = harness.quote_body();
    body["extra"] = json!("field");
    assert_eq!(harness.post("/quote", &body)?.0, 400);
    lines.push("/quote 400");
    assert_eq!(
        harness
            .exchange(b"POST /quote HTTP/1.1\r\nContent-Length: 1\r\n\r\n{")?
            .0,
        400
    );
    lines.push("/quote 400");
    assert_eq!(harness.exchange(b"GET /quote HTTP/1.1\r\n\r\n")?.0, 405);
    lines.push("/quote 405");
    assert_eq!(
        harness.exchange(b"POST /elsewhere HTTP/1.1\r\nContent-Length: 0\r\n\r\n")?,
        (404, json!({"error": "not_found"}))
    );
    lines.push("- 404");
    assert_eq!(
        harness.exchange(b"POST /submit HTTP/1.1\r\nHost: station\r\n\r\n")?,
        (411, json!({"error": "length_required"}))
    );
    lines.push("/submit 411");
    assert_eq!(
        harness.exchange(b"POST /quote HTTP/1.1\r\nContent-Length: 65537\r\n\r\n")?,
        (413, json!({"error": "too_large"}))
    );
    lines.push("/quote 413");
    assert_eq!(
        harness.exchange(b"POST /quote HTTP/1.1\r\nContent-Len")?,
        (408, json!({"error": "timeout"}))
    );
    lines.push("- 408");
    assert!(harness.journal_is_empty()?);

    harness.recorded.phase("stale_rate");
    assert_eq!(
        harness.post("/quote", &harness.quote_body())?,
        (503, json!({"error": "unavailable"}))
    );
    lines.push("/quote 503");
    harness.recorded.phase("unreachable");
    assert_eq!(harness.post("/quote", &harness.quote_body())?.0, 503);
    lines.push("/quote 503");
    assert!(harness.journal_is_empty()?);
    harness.assert_log_clean(&lines);
    harness.finish()
}

#[test]
fn submission_refusals_are_4xx_and_an_unreachable_node_is_5xx() -> TestResult {
    let harness = start("SUBMIT")?;
    let mut lines = Vec::new();
    harness.recorded.phase("refused");
    let (status, quote) = harness.post("/quote", &harness.quote_body())?;
    assert_eq!(status, 200);
    lines.push("/quote 200");
    let submission = harness.submission_body(&quote)?;
    let mut unknown = submission.clone();
    unknown["batch"]["quote"]["quoteNonce"] = json!("99");
    assert_eq!(harness.post("/submit", &unknown)?.0, 422);
    lines.push("/submit 422");
    let mut tampered = submission.clone();
    tampered["call"]["data"] = json!("0x00");
    assert_eq!(harness.post("/submit", &tampered)?.0, 422);
    lines.push("/submit 422");
    let mut raised = submission.clone();
    raised["batch"]["quote"]["tokenAmount"] = json!("3145141");
    assert_eq!(harness.post("/submit", &raised)?.0, 422);
    lines.push("/submit 422");
    assert!(harness.recorded.sent().is_empty());

    assert_eq!(
        harness.post("/submit", &submission)?,
        (422, json!({"error": "refused"}))
    );
    lines.push("/submit 422");
    assert_eq!(harness.recorded.sent().len(), 1);
    harness.recorded.phase("unreachable");
    assert_eq!(
        harness.post("/submit", &submission)?,
        (503, json!({"error": "unavailable"}))
    );
    lines.push("/submit 503");
    assert_eq!(harness.recorded.sent().len(), 1);
    harness.assert_log_clean(&lines);
    harness.finish()
}
