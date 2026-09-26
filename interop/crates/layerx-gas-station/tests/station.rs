use std::cell::{Cell, RefCell};
use std::collections::BTreeMap;
use std::path::PathBuf;
use std::rc::Rc;

use k256::ecdsa::SigningKey;
use k256::elliptic_curve::rand_core::OsRng;
use layerx_gas_station::config::StationConfig;
use layerx_gas_station::journal::{Completion, Entry, Journal, Key};
use layerx_gas_station::price::{PaymasterRateSource, PriceError};
use layerx_gas_station::quote::{address_word, keccak, word, Address, Word, SIDIORA};
use layerx_gas_station::rpc::{
    bytes, hex, quantity, ConfiguredRpc, Exchange, HttpsExchange, JsonRpc, RpcFault,
};
use layerx_gas_station::signer::{LocalSigner, QuoteSigner, SignerError};
use layerx_gas_station::station::{
    GasStation, Progress, QuoteOutcome, StationError, SubmitRequest,
};
use layerx_gas_station::tx::{authorization_digest, batch_digest, Authorization, Call, Fees};
use layerx_gas_station::{QuoteError, QuoteRequest};
use serde_json::{json, Value};

type TestResult = Result<(), Box<dyn std::error::Error>>;

const AMOUNT: u128 = 3_145_140;

struct CountedSigner {
    inner: LocalSigner,
    count: Rc<Cell<usize>>,
}
impl QuoteSigner for CountedSigner {
    fn address(&self) -> Address {
        self.inner.address()
    }
    fn sign_digest(&self, digest: Word) -> Result<[u8; 65], SignerError> {
        self.count.set(self.count.get() + 1);
        self.inner.sign_digest(digest)
    }
}
struct Recording {
    fixture: Value,
    phase: RefCell<String>,
    bindings: RefCell<BTreeMap<String, Value>>,
    sent: RefCell<Vec<Vec<u8>>>,
    journal: PathBuf,
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
struct ReplayExchange(Rc<Recording>);
impl Exchange for ReplayExchange {
    fn request(&self, _endpoint: &str, method: &str, params: &Value) -> Result<Value, RpcFault> {
        if method == "eth_sendRawTransaction" {
            let raw = bytes(params[0].as_str().ok_or(RpcFault::Malformed)?)?;
            let hash = keccak(&raw);
            let journal =
                std::fs::read_to_string(&self.0.journal).map_err(|_| RpcFault::Unavailable)?;
            let entries = journal
                .lines()
                .map(serde_json::from_str::<Entry>)
                .collect::<Result<Vec<_>, _>>()
                .map_err(|_| RpcFault::Malformed)?;
            assert!(entries.iter().any(|entry| matches!(entry, Entry::Prepared { submission, .. } if submission.raw == raw && submission.hash == hash)));
            assert!(journal.ends_with('\n'));
            self.0
                .bindings
                .borrow_mut()
                .insert("$raw".into(), json!(hex(&raw)));
            self.0
                .bindings
                .borrow_mut()
                .insert("$hash".into(), json!(hex(&hash)));
            self.0.sent.borrow_mut().push(raw);
        }
        for phase in [self.0.phase.borrow().as_str(), "*"] {
            let rules = self.0.fixture["phases"][phase]
                .as_array()
                .ok_or(RpcFault::Malformed)?;
            for rule in rules {
                if rule["method"] == method
                    && substitute(&rule["params"], &self.0.bindings.borrow()) == *params
                {
                    if rule["fault"] == "unavailable" {
                        return Err(RpcFault::Unavailable);
                    }
                    if let Some(code) = rule["error"]["code"].as_i64() {
                        return Err(RpcFault::Rejected { code });
                    }
                    return Ok(substitute(&rule["result"], &self.0.bindings.borrow()));
                }
            }
        }
        panic!("unmatched recorded exchange: {method}")
    }
}
fn config() -> StationConfig {
    StationConfig {
        chain_id: 1325,
        endpoints: vec![format!("https://{}", std::process::id())],
        paymaster: [0x44; 20],
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
        relayer_key_env: "PAXEER_STATION_TEST_KEY".into(),
    }
}
fn install_ephemeral_signer(config: &StationConfig) -> Result<LocalSigner, SignerError> {
    let key = SigningKey::random(&mut OsRng);
    std::env::set_var(&config.relayer_key_env, hex(&key.to_bytes()));
    let signer = LocalSigner::from_config(config);
    std::env::remove_var(&config.relayer_key_env);
    signer
}
fn recording(
    path: PathBuf,
    account: Address,
    sponsor: Address,
) -> Result<Rc<Recording>, Box<dyn std::error::Error>> {
    let fixture: Value = serde_json::from_str(include_str!("fixtures/cycle.json"))?;
    let consumed = [
        keccak(b"usedQuoteNonces(address,uint256)")[..4].to_vec(),
        address_word(sponsor).to_vec(),
        word(7).to_vec(),
    ]
    .concat();
    let bindings = BTreeMap::from([
        ("$sponsor", hex(&sponsor)),
        ("$paymaster", hex(&[0x44; 20])),
        ("$current_rate_call", hex(&keccak(b"currentRate()")[..4])),
        ("$rate_updated_call", hex(&keccak(b"rateUpdatedAt()")[..4])),
        ("$account", hex(&account)),
        ("$token", hex(&SIDIORA)),
        ("$zero", hex(&word(0))),
        ("$one", hex(&word(1))),
        ("$batch_nonce_call", hex(&keccak(b"nonce()")[..4])),
        ("$consumed_call", hex(&consumed)),
        ("$block", hex(&[0x10; 32])),
        ("$finalized", hex(&[0x12; 32])),
        ("$account_word", hex(&address_word(account))),
        ("$sponsor_word", hex(&address_word(sponsor))),
        ("$token_word", hex(&address_word(SIDIORA))),
        ("$amount", hex(&word(AMOUNT))),
        (
            "$transfer_topic",
            hex(&keccak(b"Transfer(address,address,uint256)")),
        ),
        (
            "$sponsored_topic",
            hex(&keccak(b"Sponsored(address,address,uint256,uint256)")),
        ),
        ("$sponsored_data", hex(&[word(AMOUNT), word(7)].concat())),
        (
            "$balance_call",
            hex(&[
                keccak(b"balanceOf(address)")[..4].to_vec(),
                address_word(sponsor).to_vec(),
            ]
            .concat()),
        ),
    ])
    .into_iter()
    .map(|(k, v)| (k.into(), json!(v)))
    .collect();
    Ok(Rc::new(Recording {
        fixture,
        phase: RefCell::new("submit".into()),
        bindings: RefCell::new(bindings),
        sent: RefCell::new(vec![]),
        journal: path,
    }))
}
type TestStation = GasStation<
    SharedSigner,
    ConfiguredRpc<ReplayExchange>,
    PaymasterRateSource<ConfiguredRpc<ReplayExchange>>,
>;
fn prepare_submission(
    station: &mut TestStation,
    config: &StationConfig,
    account_signer: &LocalSigner,
    signer: &CountedSigner,
    recorded: &Recording,
) -> Result<SubmitRequest, Box<dyn std::error::Error>> {
    let account = account_signer.address();
    let sponsor = signer.address();
    let fees = Fees {
        gas_limit: 200_000,
        max_fee_per_gas: 5_000_000_000_000,
        max_priority_fee_per_gas: 1_000_000_000,
    };
    let request = QuoteRequest {
        account,
        max_token_amount: 3_200_000,
        gas_cost: fees.gas_cost()?,
        deadline: 1019,
        quote_nonce: word(7),
    };
    let QuoteOutcome::Signed(quoted) = station.quote(&request, fees, 1000)? else {
        panic!("expected quote")
    };
    assert_eq!(quoted.quote.token_amount, word(AMOUNT));
    assert_eq!(signer.count.get(), 1);
    assert!(matches!(
        station.quote(&request, fees, 1000)?,
        QuoteOutcome::Signed(_)
    ));
    assert_eq!(signer.count.get(), 1);
    let calls = vec![Call {
        to: [0x55; 20],
        value: word(0),
        data: vec![1, 2, 3],
    }];
    let mut auth = Authorization {
        chain_id: config.chain_id,
        delegate: config.paymaster,
        nonce: 0,
        signature: [0; 65],
    };
    auth.signature = account_signer.sign_digest(authorization_digest(&auth)?)?;
    let signature = account_signer.sign_digest(batch_digest(
        config.chain_id,
        account,
        word(0),
        &calls,
        &quoted.quote,
    )?)?;
    let key = Key {
        sponsor,
        quote_nonce: word(7),
    };
    let submit = SubmitRequest {
        key,
        account,
        calls,
        authorizations: vec![auth],
        account_signature: signature,
    };
    assert!(recorded.sent.borrow().is_empty());
    Ok(submit)
}
fn assert_balances(
    config: &StationConfig,
    recorded: &Rc<Recording>,
    sponsor: Address,
) -> TestResult {
    let rpc = ConfiguredRpc::new(config, ReplayExchange(Rc::clone(recorded)))?;
    let before = quantity("0x3782dace9d900000")?;
    let after = rpc.call("eth_getBalance", json!([hex(&sponsor), "latest"]))?;
    let after = quantity(after.as_str().ok_or(RpcFault::Malformed)?)?;
    assert_eq!(before - after, 500_000_000_000_000_000);
    let data = recorded.bindings.borrow()["$balance_call"].clone();
    let balance = rpc.call(
        "eth_call",
        json!([{"to":hex(&SIDIORA),"data":data},"latest"]),
    )?;
    assert_eq!(balance, hex(&word(AMOUNT)));
    Ok(())
}
fn open_station(
    config: &StationConfig,
    signer: &Rc<CountedSigner>,
    recorded: &Rc<Recording>,
    path: &std::path::Path,
) -> Result<TestStation, StationError> {
    GasStation::new(
        config.clone(),
        SharedSigner(Rc::clone(signer)),
        ConfiguredRpc::new(config, ReplayExchange(Rc::clone(recorded)))?,
        PaymasterRateSource::new(
            ConfiguredRpc::new(config, ReplayExchange(Rc::clone(recorded)))?,
            config.paymaster,
        ),
        Journal::open(path)?,
    )
}
fn full_cycle(initial_phase: &str) -> TestResult {
    let config = config();
    let inner = install_ephemeral_signer(&config)?;
    let account_signer = install_ephemeral_signer(&config)?;
    let account = account_signer.address();
    let sponsor = inner.address();
    let count = Rc::new(Cell::new(0));
    let signer = Rc::new(CountedSigner {
        inner,
        count: Rc::clone(&count),
    });
    let path = std::env::temp_dir().join(format!("paxeer-cycle-{}.jsonl", std::process::id()));
    if path.exists() {
        std::fs::remove_file(&path)?;
    }
    let recorded = recording(path.clone(), account, sponsor)?;
    *recorded.phase.borrow_mut() = initial_phase.into();
    let start = || open_station(&config, &signer, &recorded, &path);
    let mut station = start()?;
    let mut submit =
        prepare_submission(&mut station, &config, &account_signer, &signer, &recorded)?;
    let key = submit.key;
    submit.account_signature[0] ^= 1;
    assert!(station.submit(&submit, 1000).is_err());
    assert_eq!(count.get(), 1);
    assert!(recorded.sent.borrow().is_empty());
    submit.account_signature[0] ^= 1;
    if initial_phase == "refused" {
        assert!(matches!(
            station.submit(&submit, 1000),
            Err(StationError::Rpc(RpcFault::Rejected { code: -32000 }))
        ));
        assert!(station.journal().state().items[&key].submission.is_some());
        assert!(station.journal().state().items[&key].completion.is_none());
    } else {
        assert_eq!(station.submit(&submit, 1000)?, Progress::Pending);
    }
    assert_eq!(count.get(), 2);
    assert_eq!(recorded.sent.borrow().len(), 1);
    assert_envelope(&recorded.sent.borrow()[0], sponsor, account)?;
    drop(station);
    *recorded.phase.borrow_mut() = "restart".into();
    let mut station = start()?;
    assert_eq!(station.resume(key)?, Progress::Pending);
    assert_eq!(count.get(), 2);
    assert_eq!(recorded.sent.borrow()[0], recorded.sent.borrow()[1]);
    drop(station);
    for phase in ["already_known", "nonce_too_low"] {
        *recorded.phase.borrow_mut() = phase.into();
        let mut station = start()?;
        assert_eq!(station.resume(key)?, Progress::Pending);
        assert!(station.journal().state().items[&key].completion.is_none());
        assert_eq!(count.get(), 2);
        assert!(recorded
            .sent
            .borrow()
            .iter()
            .all(|raw| raw == &recorded.sent.borrow()[0]));
    }
    *recorded.phase.borrow_mut() = "included".into();
    let mut station = start()?;
    let expected = Completion::Included {
        hash: keccak(&recorded.sent.borrow()[0]),
        block_number: 16,
        sid_collected: word(AMOUNT),
        pax_spent: word(500_000_000_000_000_000),
    };
    assert_eq!(station.resume(key)?, Progress::Completed(expected));
    assert_eq!(station.resume(key)?, Progress::Completed(expected));
    assert_eq!(count.get(), 2);
    assert_eq!(recorded.sent.borrow().len(), 4);
    assert_balances(&config, &recorded, sponsor)?;
    drop(station);
    let recovered = Journal::open(&path)?;
    assert_eq!(recovered.state().items[&key].completion, Some(expected));
    drop(recovered);
    std::fs::remove_file(&path)?;
    *recorded.phase.borrow_mut() = "consumed".into();
    let mut station = start()?;
    assert_eq!(
        station.submit(&submit, 1000)?,
        Progress::Completed(Completion::Consumed)
    );
    assert_eq!(
        station.journal().state().items[&key].completion,
        Some(Completion::Consumed)
    );
    assert_eq!(count.get(), 2);
    assert_eq!(recorded.sent.borrow().len(), 4);
    drop(station);
    std::fs::remove_file(&path)?;
    Ok(())
}
struct SharedSigner(Rc<CountedSigner>);
impl QuoteSigner for SharedSigner {
    fn address(&self) -> Address {
        self.0.address()
    }
    fn sign_digest(&self, digest: Word) -> Result<[u8; 65], SignerError> {
        self.0.sign_digest(digest)
    }
}
#[test]
fn quote_submit_collect_restart_and_consumed_nonce() -> TestResult {
    full_cycle("submit")?;
    full_cycle("refused")
}

#[test]
fn quote_refused_when_governed_rate_is_stale_or_missing() -> TestResult {
    let mut config = config();
    config.relayer_key_env = "PAXEER_STATION_RATE_TEST_KEY".into();
    let inner = install_ephemeral_signer(&config)?;
    let account = install_ephemeral_signer(&config)?.address();
    let sponsor = inner.address();
    let count = Rc::new(Cell::new(0));
    let signer = Rc::new(CountedSigner {
        inner,
        count: Rc::clone(&count),
    });
    let path = std::env::temp_dir().join(format!("paxeer-rate-{}.jsonl", std::process::id()));
    if path.exists() {
        std::fs::remove_file(&path)?;
    }
    let recorded = recording(path.clone(), account, sponsor)?;
    let mut station = open_station(&config, &signer, &recorded, &path)?;
    let fees = Fees {
        gas_limit: 200_000,
        max_fee_per_gas: 5_000_000_000_000,
        max_priority_fee_per_gas: 1_000_000_000,
    };
    let request = QuoteRequest {
        account,
        max_token_amount: 3_200_000,
        gas_cost: fees.gas_cost()?,
        deadline: 1019,
        quote_nonce: word(7),
    };
    for (phase, source_refusal, pricing_refusal) in [
        ("stale_rate", Some(PriceError::StaleRate), None),
        ("aged_rate", None, Some(PriceError::StaleRate)),
        ("missing_rate", Some(PriceError::MissingRate), None),
    ] {
        *recorded.phase.borrow_mut() = phase.into();
        let refusal = match station.quote(&request, fees, 1000) {
            Err(StationError::Price(refusal)) => (Some(refusal), None),
            Err(StationError::Quote(QuoteError::Price(refusal))) => (None, Some(refusal)),
            _ => (None, None),
        };
        assert_eq!(refusal, (source_refusal, pricing_refusal));
        assert!(station.journal().state().items.is_empty());
        assert_eq!(count.get(), 0);
    }
    drop(station);
    std::fs::remove_file(&path)?;
    Ok(())
}

#[test]
fn response_reads_bound_total_time_and_size() -> TestResult {
    use std::io::{Cursor, Write as _};
    use std::os::unix::net::UnixStream;
    use std::time::{Duration, Instant};

    let budget = Duration::from_millis(300);
    let (mut reader, mut writer) = UnixStream::pair()?;
    reader.set_nonblocking(true)?;
    let sending = std::thread::spawn(move || {
        let mut sent = 0;
        while writer.write_all(b"x").is_ok() {
            sent += 1;
            std::thread::sleep(Duration::from_millis(20));
        }
        sent
    });
    let started = Instant::now();
    assert_eq!(
        HttpsExchange::read_response(&mut reader, budget),
        Err(RpcFault::Unavailable)
    );
    assert!(started.elapsed() >= budget);
    assert!(started.elapsed() < Duration::from_secs(2));
    drop(reader);
    assert!(sending.join().map_err(|_| "response writer panicked")? > 1);

    let body = json!({"jsonrpc":"2.0","id":1,"result":"0x5"}).to_string();
    let response = format!(
        "HTTP/1.1 200 OK\r\nContent-Length: {}\r\n\r\n{body}",
        body.len()
    );
    assert_eq!(
        HttpsExchange::read_response(&mut Cursor::new(response.as_bytes()), budget)?,
        json!("0x5")
    );
    assert_eq!(
        HttpsExchange::read_response(&mut Cursor::new(response.as_bytes()), Duration::ZERO),
        Err(RpcFault::Unavailable)
    );
    assert_eq!(
        HttpsExchange::read_response(&mut Cursor::new(vec![b'x'; 1_048_577]), budget),
        Err(RpcFault::Malformed)
    );
    let (mut reader, _writer) = UnixStream::pair()?;
    reader.set_nonblocking(true)?;
    assert_eq!(
        HttpsExchange::read_response(&mut reader, budget),
        Err(RpcFault::Unavailable)
    );
    Ok(())
}

type RlpParts<'a> = (&'a [u8], &'a [u8]);
fn rlp_item(input: &[u8]) -> Result<RlpParts<'_>, Box<dyn std::error::Error>> {
    let first = *input.first().ok_or("empty RLP")?;
    let (offset, length) = match first {
        0..=127 => (0, 1),
        128..=183 => (1, usize::from(first - 128)),
        184..=191 | 248..=255 => {
            let count = usize::from(if first < 192 {
                first - 183
            } else {
                first - 247
            });
            let mut length = 0_usize;
            for byte in input.get(1..=count).ok_or("short length")? {
                length = length
                    .checked_mul(256)
                    .and_then(|n| n.checked_add(usize::from(*byte)))
                    .ok_or("length overflow")?;
            }
            (1 + count, length)
        }
        192..=247 => (1, usize::from(first - 192)),
    };
    let end = offset + length;
    Ok((
        input.get(offset..end).ok_or("short payload")?,
        input.get(end..).ok_or("short tail")?,
    ))
}
fn assert_envelope(raw: &[u8], sponsor: Address, account: Address) -> TestResult {
    assert_eq!(raw[0], 4);
    let (mut payload, rest) = rlp_item(&raw[1..])?;
    assert!(rest.is_empty());
    let mut fields = Vec::new();
    let mut signing_length = 0;
    while !payload.is_empty() {
        let before = payload.len();
        let (field, tail) = rlp_item(payload)?;
        if fields.len() < 10 {
            signing_length += before - tail.len();
        }
        fields.push(field);
        payload = tail;
    }
    assert_eq!(fields.len(), 13);
    assert_eq!(fields[0], [5, 45]);
    assert_eq!(fields[1], [5]);
    assert_eq!(fields[5], account);
    assert!(fields[6].is_empty());
    assert!(fields[8].is_empty());
    assert_eq!(&fields[7][..4], &keccak(b"executeSponsored((address,uint256,bytes)[],(address,address,uint256,uint256,uint256,uint256,uint256),bytes,bytes)")[..4]);
    let (authorization, rest) = rlp_item(fields[9])?;
    assert!(rest.is_empty());
    let (chain, tail) = rlp_item(authorization)?;
    assert_eq!(chain, [5, 45]);
    let (delegate, _) = rlp_item(tail)?;
    assert_eq!(delegate, [0x44; 20]);
    let (payload, _) = rlp_item(&raw[1..])?;
    let length_bytes = signing_length.to_be_bytes();
    let count = length_bytes.iter().skip_while(|b| **b == 0).count();
    let mut unsigned = vec![4, 247 + u8::try_from(count)?];
    unsigned.extend_from_slice(&length_bytes[length_bytes.len() - count..]);
    unsigned.extend_from_slice(&payload[..signing_length]);
    let mut signature = [0_u8; 65];
    assert!(fields[11].len() <= 32 && fields[12].len() <= 32);
    signature[32 - fields[11].len()..32].copy_from_slice(fields[11]);
    signature[64 - fields[12].len()..64].copy_from_slice(fields[12]);
    signature[64] = 27 + fields[10].first().copied().unwrap_or(0);
    assert_eq!(
        layerx_gas_station::tx::recover(keccak(&unsigned), &signature)?,
        sponsor
    );
    Ok(())
}
