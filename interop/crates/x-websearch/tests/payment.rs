use std::error::Error;
use std::fmt::Write as _;
use std::io::{Read as _, Write as _};
use std::net::{SocketAddr, TcpListener, TcpStream};
use std::os::unix::fs::PermissionsExt as _;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, PoisonError};
use std::thread;
use std::time::Duration;

use base64::engine::general_purpose::STANDARD;
use base64::Engine as _;
use ed25519_dalek::{Signer as _, SigningKey};
use layerx_crypto::payments::{Grant, Payment};
use layerx_interop_gateway::adapter::{AdapterId, ConformanceSuite};
use layerx_proof::merkle::leaf_hash;
use layerx_wire::encode::Encoder;
use layerx_wire::hash::{receipt_digest, Domain};
use serde_json::{json, Value};
use sha2::{Digest as _, Sha256};
use x_websearch::assets::{AcceptedAsset, AcceptedAssets};
use x_websearch::canonical::{canonical_bytes, content_digest, digest_hex, ContentKind};
use x_websearch::config::AssetSymbol;
use x_websearch::content::ContentStore;
use x_websearch::crawl::title_of;
use x_websearch::index::WebIndex;
use x_websearch::payment::{
    account_id, hex, purpose_hash, receiver_did, sign_draw, wallet_account, DrawRequest,
    PaymentGate, EXACT, METERED, PAYER_DID, PAYMENT_REQUIRED, PAYMENT_RESPONSE, PAYMENT_SIGNATURE,
    PROTOCOL_VERSION,
};
use x_websearch::search::search_route;
use x_websearch::{
    Config, KeyFiles, Limits, Request, Response, Route, RouteTable, RunningServer, Server,
};

type Outcome<T = ()> = Result<T, Box<dyn Error>>;

const RECEIVER_SECRET: &str = "9d61b19deffd5a60ba844af492ec2cc44449c5697b326919703bac031cae7f60";
const NOW_MS: u64 = 1_000_000_000_000;
const CURRENCIES: [&str; 4] = ["SID", "PAX", "USDC", "USDL"];
const RECORD_VARIABLE: &str = "X_WEBSEARCH_RECORD_EXCHANGE";

const fn fixed_clock() -> u64 {
    NOW_MS
}

fn fixtures() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures")
}

fn read_json(path: &Path) -> Outcome<Value> {
    Ok(serde_json::from_str(&std::fs::read_to_string(path)?)?)
}

fn fail(message: impl Into<String>) -> Box<dyn Error> {
    message.into().into()
}

fn text<'a>(value: &'a Value, pointer: &str) -> Outcome<&'a str> {
    value
        .pointer(pointer)
        .and_then(Value::as_str)
        .ok_or_else(|| fail(format!("missing {pointer}")))
}

/// One recorded gateway answer.
struct Rule {
    method: String,
    params: Value,
    answer: Value,
    once: bool,
    used: bool,
}

/// Replays a recorded gateway exchange on the loopback interface. A request
/// no recorded rule matches is answered 503.
struct Gateway {
    address: SocketAddr,
    rules: Arc<Mutex<Vec<Rule>>>,
    calls: Arc<Mutex<Vec<String>>>,
}

fn load_rules(names: &[&str]) -> Outcome<Vec<Rule>> {
    let mut rules = Vec::new();
    for name in names {
        let recording = read_json(&fixtures().join("gateway").join(name))?;
        let list = recording
            .pointer("/endpoints/gateway/*")
            .and_then(Value::as_array)
            .ok_or_else(|| fail(format!("{name} has no gateway rules")))?;
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

fn answer(rules: &Mutex<Vec<Rule>>, calls: &Mutex<Vec<String>>, body: &[u8]) -> Option<Value> {
    let request: Value = serde_json::from_slice(body).ok()?;
    let method = request.get("method")?.as_str()?.to_owned();
    let params = request.get("params").cloned().unwrap_or(Value::Null);
    calls
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .push(method.clone());
    let mut rules = rules.lock().unwrap_or_else(PoisonError::into_inner);
    let rule = rules.iter_mut().find(|rule| {
        !rule.used
            && rule.method == method
            && (rule.params == json!("$any") || rule.params == params)
    })?;
    rule.used = rule.once;
    let mut reply =
        json!({ "jsonrpc": "2.0", "id": request.get("id").cloned().unwrap_or(json!(1)) });
    if let (Some(reply), Some(answer)) = (reply.as_object_mut(), rule.answer.as_object()) {
        reply.extend(answer.clone());
    }
    Some(reply)
}

fn serve(stream: &mut TcpStream, rules: &Mutex<Vec<Rule>>, calls: &Mutex<Vec<String>>) -> Outcome {
    let (_, body) = read_http(stream)?;
    let response = answer(rules, calls, &body).map_or_else(
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

impl Gateway {
    fn start(names: &[&str]) -> Outcome<Self> {
        let listener = TcpListener::bind("127.0.0.1:0")?;
        let address = listener.local_addr()?;
        let rules = Arc::new(Mutex::new(load_rules(names)?));
        let calls = Arc::new(Mutex::new(Vec::new()));
        let (served_rules, served_calls) = (Arc::clone(&rules), Arc::clone(&calls));
        thread::spawn(move || {
            for mut stream in listener.incoming().flatten() {
                let _ = serve(&mut stream, &served_rules, &served_calls);
            }
        });
        Ok(Self {
            address,
            rules,
            calls,
        })
    }

    fn calls(&self, method: &str) -> usize {
        self.calls
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .iter()
            .filter(|call| *call == method)
            .count()
    }

    fn rule_count(&self) -> usize {
        self.rules
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .len()
    }
}

/// The pinned conformance suite: the recorded gateway exchange the seller is
/// qualified against, by content digest and rule count.
fn conformance(gateway: &Gateway) -> Outcome<ConformanceSuite> {
    let mut names: Vec<PathBuf> = std::fs::read_dir(fixtures().join("gateway"))?
        .map(|entry| entry.map(|entry| entry.path()))
        .collect::<Result<_, _>>()?;
    names.sort();
    let mut digest = Sha256::new();
    for name in names {
        digest.update(std::fs::read(name)?);
    }
    let count = u64::try_from(gateway.rule_count())?;
    Ok(ConformanceSuite::new(
        AdapterId::new("x402-v2")?,
        count,
        digest.finalize().into(),
    )?)
}

struct Harness {
    server: Option<RunningServer>,
    gateway: Gateway,
    served: Arc<AtomicUsize>,
    buyer: Value,
    scratch: PathBuf,
}

impl Drop for Harness {
    fn drop(&mut self) {
        if let Some(server) = self.server.take() {
            let _ = server.shutdown();
        }
        let _ = std::fs::remove_dir_all(&self.scratch);
    }
}

fn configure(scratch: &Path, gateway: &Gateway, buyer: &Value) -> Outcome<Config> {
    let mut config = read_json(&fixtures().join("config/valid.json"))?;
    config["data_dir"] = json!(scratch.join("data"));
    config["gateway"]["endpoint"] = json!(format!("http://{}/rpc", gateway.address));
    config["gateway"]["sequencer_public_key"] = json!(text(buyer, "/sequencerPublicKey")?);
    Ok(Config::parse(&config.to_string())?)
}

fn receiver_key(scratch: &Path) -> Outcome<ed25519_dalek::SigningKey> {
    let path = scratch.join("receiver.key");
    std::fs::write(&path, RECEIVER_SECRET)?;
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600))?;
    let files = KeyFiles {
        attestor: None,
        submitter: None,
        receiver: path,
    };
    Ok(files.load()?.receiver().clone())
}

impl Harness {
    fn start(name: &str, recordings: &[&str]) -> Outcome<Self> {
        let scratch =
            std::env::temp_dir().join(format!("x-websearch-payment-{}-{name}", std::process::id()));
        let _ = std::fs::remove_dir_all(&scratch);
        std::fs::create_dir_all(&scratch)?;
        let buyer = read_json(&fixtures().join("gateway/buyer.json"))?;
        let gateway = Gateway::start(recordings)?;
        let config = configure(&scratch, &gateway, &buyer)?;
        let receiver = receiver_key(&scratch)?;
        let gate = Arc::new(PaymentGate::new(
            &config,
            &receiver,
            conformance(&gateway)?,
            fixed_clock,
        )?);
        let served = Arc::new(AtomicUsize::new(0));
        let data = scratch.join("data");
        let store = Arc::new(ContentStore::open(&data, &[])?);
        let index = Arc::new(WebIndex::open(&data)?);
        let pages = vectors()?;
        for page in &pages {
            index.put(&page.payload, &page.title(), &page.text)?;
        }
        index.commit()?;
        let mut routes = RouteTable::new();
        let (counter, search_store) = (Arc::clone(&served), Arc::clone(&store));
        PaymentGate::install(
            &gate,
            &mut routes,
            Route::Search,
            move |request: &Request| {
                counter.fetch_add(1, Ordering::SeqCst);
                search_route(&index, &search_store, request)
            },
        )?;
        let (counter, fetch_store) = (Arc::clone(&served), Arc::clone(&store));
        PaymentGate::install(
            &gate,
            &mut routes,
            Route::Fetch,
            move |request: &Request| {
                counter.fetch_add(1, Ordering::SeqCst);
                fetch_page(&pages, &fetch_store, request)
            },
        )?;
        routes.set(Route::Content, move |request: &Request| {
            store.handle(request)
        })?;
        let running = Server::bind("127.0.0.1:0".parse()?, Limits::default(), routes)?.spawn()?;
        Ok(Self {
            server: Some(running),
            gateway,
            served,
            buyer,
            scratch,
        })
    }

    fn address(&self) -> Outcome<SocketAddr> {
        self.server
            .as_ref()
            .map(RunningServer::local_addr)
            .ok_or_else(|| fail("server stopped"))
    }

    fn served(&self) -> usize {
        self.served.load(Ordering::SeqCst)
    }

    fn payer(&self) -> Outcome<&str> {
        text(&self.buyer, "/payerDid")
    }

    fn get(&self, target: &str, headers: &[(&str, &str)]) -> Outcome<Reply> {
        let mut stream = TcpStream::connect(self.address()?)?;
        stream.set_read_timeout(Some(Duration::from_secs(30)))?;
        let mut request = format!("GET {target} HTTP/1.1\r\nHost: 127.0.0.1\r\n");
        for (name, value) in headers {
            write!(request, "{name}: {value}\r\n")?;
        }
        request.push_str("\r\n");
        stream.write_all(request.as_bytes())?;
        let mut bytes = Vec::new();
        stream.read_to_end(&mut bytes)?;
        Reply::parse(&bytes)
    }

    /// Asks for the offers, picks the one for `scheme` and `currency`, and
    /// returns the `PAYMENT-SIGNATURE` paying it with `payload`.
    fn signature(
        &self,
        target: &str,
        payer: Option<&str>,
        scheme: &str,
        currency: &str,
        payload: &Value,
    ) -> Outcome<String> {
        let headers: Vec<(&str, &str)> =
            payer.map(|payer| (PAYER_DID, payer)).into_iter().collect();
        let challenge = self.get(target, &headers)?;
        if challenge.status != 402 {
            return Err(fail(format!(
                "expected a challenge, got {}",
                challenge.status
            )));
        }
        let required = challenge.decoded(PAYMENT_REQUIRED)?;
        let offer = required
            .get("accepts")
            .and_then(Value::as_array)
            .and_then(|offers| {
                offers.iter().find(|offer| {
                    offer.get("scheme").and_then(Value::as_str) == Some(scheme)
                        && offer
                            .pointer("/extra/layerx/currency")
                            .and_then(Value::as_str)
                            == Some(currency)
                })
            })
            .ok_or_else(|| fail(format!("no {scheme} offer in {currency}")))?;
        Ok(encode(
            &json!({ "x402Version": 2, "accepted": offer, "payload": payload }),
        ))
    }

    fn pay(&self, target: &str, payer: Option<&str>, signature: &str) -> Outcome<Reply> {
        let mut headers = vec![(PAYMENT_SIGNATURE, signature)];
        if let Some(payer) = payer {
            headers.push((PAYER_DID, payer));
        }
        self.get(target, &headers)
    }

    fn metered(&self, target: &str, currency: &str, grant: &str, label: &str) -> Outcome<Reply> {
        let payer = self.payer()?;
        let signature = self.signature(
            target,
            Some(payer),
            METERED,
            currency,
            &metered_payload(grant, label),
        )?;
        self.pay(target, Some(payer), &signature)
    }

    fn exact(&self, target: &str, currency: &str, payment: &Value) -> Outcome<Reply> {
        let signature = self.signature(target, None, EXACT, currency, payment)?;
        self.pay(target, None, &signature)
    }

    fn grant(&self, pointer: &str) -> Outcome<String> {
        Ok(text(&self.buyer, pointer)?.to_owned())
    }
}

fn encode(value: &Value) -> String {
    STANDARD.encode(value.to_string())
}

fn receive_key(label: &str) -> String {
    hex(&receive_key_bytes(label))
}

fn receive_key_bytes(label: &str) -> [u8; 32] {
    labelled("x-websearch-test/receive/", label)
}

fn labelled(domain: &str, label: &str) -> [u8; 32] {
    let mut digest = Sha256::new();
    digest.update(domain.as_bytes());
    digest.update(label.as_bytes());
    digest.finalize().into()
}

/// One page of `content-vectors.json`.
#[derive(Clone)]
struct Page {
    payload: String,
    media_type: String,
    text: String,
    digest: String,
}

impl Page {
    /// The title the crawler indexes: the first line of an HTML page's text.
    fn title(&self) -> String {
        if self.media_type == "text/html" {
            title_of(&self.text)
        } else {
            String::new()
        }
    }
}

fn vectors() -> Outcome<Vec<Page>> {
    let recorded = read_json(&fixtures().join("content-vectors.json"))?;
    recorded
        .get("vectors")
        .and_then(Value::as_array)
        .ok_or_else(|| fail("content vectors"))?
        .iter()
        .map(|vector| {
            Ok(Page {
                payload: text(vector, "/payload")?.to_owned(),
                media_type: text(vector, "/media_type")?.to_owned(),
                text: text(vector, "/text")?.to_owned(),
                digest: text(vector, "/digest")?.to_owned(),
            })
        })
        .collect()
}

/// The `/fetch` resource over the committed pages: the page recorded at the
/// requested URL, or the first page's text served at it, answered in the
/// fetch route's shape after its canonical bytes are written to the store.
fn fetch_page(pages: &[Page], store: &ContentStore, request: &Request) -> Response {
    let Ok(Some(url)) = request.query_param("url") else {
        return Response::error(400, "missing_url");
    };
    let Some(page) = pages
        .iter()
        .find(|page| page.payload == url)
        .or_else(|| pages.first())
    else {
        return Response::error(500, "no_pages");
    };
    let Ok(canonical) = canonical_bytes(
        ContentKind::Fetch,
        url.as_bytes(),
        &page.media_type,
        &page.text,
    ) else {
        return Response::error(500, "canonical");
    };
    let Ok(digest) = store.put(&canonical) else {
        return Response::error(500, "content_store_error");
    };
    Response::json(
        200,
        json!({
            "url": url,
            "final_url": url,
            "media_type": page.media_type,
            "digest": digest_hex(&digest),
            "length": page.text.len(),
            "text": page.text,
        })
        .to_string()
        .into_bytes(),
    )
}

/// A query value as `encodeURIComponent` and `quote(safe='')` write it.
fn percent(text: &str) -> String {
    let mut encoded = String::with_capacity(text.len());
    for byte in text.bytes() {
        if byte.is_ascii_alphanumeric() || b"-_.~".contains(&byte) {
            encoded.push(char::from(byte));
        } else {
            let _ = write!(encoded, "%{byte:02X}");
        }
    }
    encoded
}

fn metered_payload(grant: &str, label: &str) -> Value {
    json!({ "grant": grant, "idempotencyKey": receive_key(label) })
}

struct Reply {
    status: u16,
    headers: Vec<(String, String)>,
    body: Value,
    raw: Vec<u8>,
}

impl Reply {
    fn parse(bytes: &[u8]) -> Outcome<Self> {
        let end = bytes
            .windows(4)
            .position(|window| window == b"\r\n\r\n")
            .ok_or_else(|| fail("response without a head"))?;
        let head = std::str::from_utf8(&bytes[..end])?;
        let mut lines = head.split("\r\n");
        let status = lines
            .next()
            .and_then(|line| line.split(' ').nth(1))
            .ok_or_else(|| fail("response without a status"))?
            .parse()?;
        let headers = lines
            .filter_map(|line| line.split_once(": "))
            .map(|(name, value)| (name.to_owned(), value.to_owned()))
            .collect();
        let raw = bytes[end + 4..].to_vec();
        let body = serde_json::from_slice(&raw).unwrap_or(Value::Null);
        Ok(Self {
            status,
            headers,
            body,
            raw,
        })
    }

    fn header(&self, name: &str) -> Option<&str> {
        self.headers
            .iter()
            .find(|(candidate, _)| candidate.eq_ignore_ascii_case(name))
            .map(|(_, value)| value.as_str())
    }

    fn decoded(&self, name: &str) -> Outcome<Value> {
        let header = self
            .header(name)
            .ok_or_else(|| fail(format!("no {name} header")))?;
        Ok(serde_json::from_slice(&STANDARD.decode(header)?)?)
    }

    fn error(&self) -> Option<&str> {
        self.body.get("error").and_then(Value::as_str)
    }
}

fn assert_settled(reply: &Reply) -> Outcome {
    assert_eq!(reply.status, 200, "{:?}", reply.body);
    let response = reply.decoded(PAYMENT_RESPONSE)?;
    assert_eq!(response.get("success"), Some(&json!(true)));
    Ok(())
}

fn assert_refused(reply: &Reply, reason: &str) {
    assert_eq!(
        (reply.status, reply.error()),
        (402, Some(reason)),
        "{:?}",
        reply.body
    );
    assert!(reply.header(PAYMENT_REQUIRED).is_some());
}

#[test]
fn metered_draws_settle_in_each_asset() -> Outcome {
    let harness = Harness::start("metered", &["metered.json"])?;
    for currency in CURRENCIES {
        let grant = harness.grant(&format!("/grants/{currency}"))?;
        let reply = harness.metered(
            &format!("/search?q={currency}"),
            currency,
            &grant,
            &format!("metered-{currency}"),
        )?;
        assert_settled(&reply)?;
        let response = reply.decoded(PAYMENT_RESPONSE)?;
        assert_eq!(response.get("network"), Some(&json!("layerx:1")));
        assert_eq!(
            response.pointer("/extensions/layerx/purposeHash"),
            Some(&json!(hex(&purpose_hash(&receiver_public_key()?))))
        );
        let asset = accepted(currency)?;
        let payer = account_id(&wallet_account(harness.payer()?, &asset))
            .ok_or_else(|| fail("payer account"))?;
        assert_eq!(response.get("payer"), Some(&json!(hex(&payer))));
        assert_eq!(reply.body.get("query"), Some(&json!(currency)));
    }
    assert_eq!(harness.served(), 4);
    assert_eq!(harness.gateway.calls("lx_sendActivity"), 4);
    Ok(())
}

#[test]
fn pax_is_paid_into_the_main_accounts_and_every_other_asset_into_its_own() -> Outcome {
    let harness = Harness::start("accounts", &["refusals.json"])?;
    let payer = harness.payer()?;
    let receiver = receiver_did(&receiver_public_key()?);
    let offers = harness
        .get("/search?q=accounts", &[(PAYER_DID, payer)])?
        .decoded(PAYMENT_REQUIRED)?;
    let offers = offers
        .get("accepts")
        .and_then(Value::as_array)
        .ok_or_else(|| fail("offers"))?;
    for offer in offers {
        let currency = text(offer, "/extra/layerx/currency")?;
        let asset = accepted(currency)?;
        let (receiver_account, payer_account) = if currency == "PAX" {
            (
                format!("agent:{receiver}:main"),
                format!("agent:{payer}:main"),
            )
        } else {
            (
                format!("agent:{receiver}:asset:{}", asset.id_hex()),
                format!("agent:{payer}:asset:{}", asset.id_hex()),
            )
        };
        assert_eq!(text(offer, "/extra/layerx/account")?, receiver_account);
        let pay_to = account_id(&receiver_account).ok_or_else(|| fail("payee"))?;
        assert_eq!(text(offer, "/payTo")?, hex(&pay_to));
        if text(offer, "/scheme")? == METERED {
            let drawn = account_id(&payer_account).ok_or_else(|| fail("payer"))?;
            assert_eq!(text(offer, "/extra/layerx/payer")?, hex(&drawn));
        }
    }
    assert_eq!(offers.len(), 8);
    Ok(())
}

#[test]
fn exact_receipts_settle_in_each_asset() -> Outcome {
    let harness = Harness::start("exact", &["exact.json"])?;
    for currency in CURRENCIES {
        let payment = harness
            .buyer
            .pointer(&format!("/exact/{currency}"))
            .cloned()
            .ok_or_else(|| fail("payment"))?;
        let reply = harness.exact(&format!("/fetch?url={currency}"), currency, &payment)?;
        assert_settled(&reply)?;
        let response = reply.decoded(PAYMENT_RESPONSE)?;
        assert_eq!(response.pointer("/extensions/layerx/purposeHash"), None);
        assert_eq!(reply.body.get("url"), Some(&json!(currency)));
    }
    assert_eq!(harness.served(), 4);
    assert_eq!(harness.gateway.calls("lx_getReceipt"), 4);
    Ok(())
}

#[test]
fn a_pending_draw_recovers_the_same_activity_and_releases_once() -> Outcome {
    let harness = Harness::start("pending", &["pending.json"])?;
    let payer = harness.payer()?;
    let grant = harness.grant("/grants/SID")?;
    let target = "/search?q=pending";
    let signature = harness.signature(
        target,
        Some(payer),
        METERED,
        "SID",
        &metered_payload(&grant, "pending-SID"),
    )?;
    for _ in 0..2 {
        let reply = harness.pay(target, Some(payer), &signature)?;
        assert_eq!(
            (reply.status, reply.error()),
            (503, Some("payment_pending"))
        );
        assert_eq!(reply.header("Retry-After"), Some("1"));
        assert_eq!(harness.served(), 0);
    }
    assert_settled(&harness.pay(target, Some(payer), &signature)?)?;
    assert_eq!(harness.served(), 1);
    assert_eq!(harness.gateway.calls("lx_sendActivity"), 1);
    assert_eq!(harness.gateway.calls("lx_getActivityStatus"), 2);
    assert_eq!(harness.gateway.calls("lx_getReceipt"), 1);
    assert_eq!(harness.gateway.calls("lx_getSequence"), 2);
    assert_refused(
        &harness.pay(target, Some(payer), &signature)?,
        "receipt_consumed",
    );
    assert_eq!(harness.served(), 1);
    assert_eq!(harness.gateway.calls("lx_sendActivity"), 1);
    Ok(())
}

#[test]
fn malformed_and_unmatched_payments_are_challenged() -> Outcome {
    let harness = Harness::start("challenge", &["refusals.json"])?;
    let payer = harness.payer()?;
    let target = "/search?q=challenge";
    let challenge = harness.get(target, &[(PAYER_DID, payer)])?;
    assert_refused(&challenge, "payment_required");
    let offers = challenge.decoded(PAYMENT_REQUIRED)?;
    assert_eq!(
        offers
            .pointer("/accepts")
            .and_then(Value::as_array)
            .map(Vec::len),
        Some(8)
    );
    assert_eq!(
        harness
            .get(target, &[])?
            .decoded(PAYMENT_REQUIRED)?
            .pointer("/accepts")
            .and_then(Value::as_array)
            .map(Vec::len),
        Some(4)
    );
    assert_refused(
        &harness.pay(target, Some(payer), "not a payment")?,
        "payment_invalid",
    );
    let grant = harness.grant("/grants/SID")?;
    let signature = harness.signature(
        target,
        Some(payer),
        METERED,
        "SID",
        &metered_payload(&grant, "altered"),
    )?;
    let mut altered: Value = serde_json::from_slice(&STANDARD.decode(&signature)?)?;
    altered["accepted"]["amount"] = json!("1");
    assert_refused(
        &harness.pay(target, Some(payer), &encode(&altered))?,
        "payment_invalid",
    );
    let mut redirected: Value = serde_json::from_slice(&STANDARD.decode(&signature)?)?;
    redirected["accepted"]["payTo"] = json!("11".repeat(32));
    assert_refused(
        &harness.pay(target, Some(payer), &encode(&redirected))?,
        "payment_invalid",
    );
    assert_eq!(
        harness
            .get("/search?q=x", &[(PAYER_DID, "did:Upper")])?
            .status,
        400
    );
    assert_eq!(harness.served(), 0);
    assert_eq!(harness.gateway.calls("lx_sendActivity"), 0);
    Ok(())
}

#[test]
fn grants_that_do_not_bind_this_draw_are_refused() -> Outcome {
    let harness = Harness::start("grants", &["refusals.json"])?;
    for (grant, reason) in [
        ("recurring", "grant_not_metered"),
        ("expired", "grant_expired"),
        ("otherPurpose", "grant_purpose_mismatch"),
        ("lowLimit", "grant_limit"),
        ("otherRecipient", "grant_recipient_mismatch"),
    ] {
        let grant = harness.grant(&format!("/refusedGrants/{grant}"))?;
        let reply = harness.metered(&format!("/search?q={reason}"), "SID", &grant, reason)?;
        assert_refused(&reply, reason);
        assert_eq!(
            reply.decoded(PAYMENT_RESPONSE)?.get("success"),
            Some(&json!(false))
        );
    }
    let grant = harness.grant("/grants/SID")?;
    let bogus = format!("{}00", &grant[..grant.len() - 2]);
    assert_refused(
        &harness.metered("/search?q=bogus", "SID", &bogus, "bogus")?,
        "invalid_grant",
    );
    assert_refused(
        &harness.metered("/search?q=asset", "PAX", &grant, "asset")?,
        "grant_payer_mismatch",
    );
    let other = format!("did:layerx:{}", "11".repeat(32));
    let target = "/search?q=other-payer";
    let signature = harness.signature(
        target,
        Some(&other),
        METERED,
        "SID",
        &metered_payload(&grant, "other-payer"),
    )?;
    assert_refused(
        &harness.pay(target, Some(&other), &signature)?,
        "grant_payer_mismatch",
    );
    assert_eq!(harness.served(), 0);
    assert_eq!(harness.gateway.calls("lx_sendActivity"), 0);
    Ok(())
}

#[test]
fn draws_the_gateway_does_not_settle_release_nothing() -> Outcome {
    let harness = Harness::start("draws", &["refusals.json"])?;
    let grant = harness.grant("/grants/SID")?;
    for (label, reason) in [
        ("refused-other-receipt", "receipt_mismatch"),
        ("refused-untrusted", "receipt_unverified"),
        ("refused-failed", "payment_failed"),
        ("refused-other-activity", "receipt_mismatch"),
        ("refused-amount", "receipt_mismatch"),
    ] {
        assert_refused(
            &harness.metered(&format!("/search?q={label}"), "SID", &grant, label)?,
            reason,
        );
    }
    let pax = harness.grant("/grants/PAX")?;
    assert_refused(
        &harness.metered("/search?q=conflict", "PAX", &pax, "refused-failed")?,
        "idempotency_conflict",
    );
    let payer = harness.payer()?;
    let target = "/search?q=unknown";
    let signature = harness.signature(
        target,
        Some(payer),
        METERED,
        "SID",
        &metered_payload(&grant, "refused-unknown"),
    )?;
    let reply = harness.pay(target, Some(payer), &signature)?;
    assert_eq!(
        (reply.status, reply.error()),
        (503, Some("payment_pending"))
    );
    assert_refused(
        &harness.pay("/search?q=elsewhere", Some(payer), &signature)?,
        "request_mismatch",
    );
    assert_eq!(harness.served(), 0);
    assert_eq!(harness.gateway.calls("lx_sendActivity"), 6);
    Ok(())
}

#[test]
fn exact_receipts_that_do_not_pay_this_request_are_refused() -> Outcome {
    let harness = Harness::start("exact-refusals", &["exact.json"])?;
    for (payment, reason) in [
        ("/refusedExact/unknown", "receipt_unknown"),
        ("/refusedExact/otherPayee", "receipt_mismatch"),
        ("/refusedExact/untrusted", "receipt_unverified"),
    ] {
        let payment = harness
            .buyer
            .pointer(payment)
            .cloned()
            .ok_or_else(|| fail("payment"))?;
        assert_refused(
            &harness.exact(&format!("/fetch?url={reason}"), "SID", &payment)?,
            reason,
        );
    }
    let sid = harness
        .buyer
        .pointer("/exact/SID")
        .cloned()
        .ok_or_else(|| fail("payment"))?;
    let mut mismatched = sid.clone();
    mismatched["receiptDigest"] = harness
        .buyer
        .pointer("/exact/PAX/receiptDigest")
        .cloned()
        .unwrap_or(Value::Null);
    assert_refused(
        &harness.exact("/fetch?url=digest", "SID", &mismatched)?,
        "receipt_digest_mismatch",
    );
    assert_refused(
        &harness.exact("/fetch?url=asset", "PAX", &sid)?,
        "receipt_mismatch",
    );
    let mut unleveled = sid.clone();
    unleveled["verificationLevel"] = json!("self-reported");
    assert_refused(
        &harness.exact("/fetch?url=level", "SID", &unleveled)?,
        "invalid_payment_payload",
    );
    assert_settled(&harness.exact("/fetch?url=first", "SID", &sid)?)?;
    let mut again = sid;
    again["idempotencyKey"] = json!(receive_key("again"));
    assert_refused(
        &harness.exact("/fetch?url=second", "SID", &again)?,
        "receipt_consumed",
    );
    assert_eq!(harness.served(), 1);
    Ok(())
}

fn receiver_secret() -> Outcome<SigningKey> {
    let secret: [u8; 32] = x_websearch::payment::unhex(RECEIVER_SECRET)
        .and_then(|bytes| bytes.try_into().ok())
        .ok_or_else(|| fail("receiver secret"))?;
    Ok(SigningKey::from_bytes(&secret))
}

fn receiver_public_key() -> Outcome<[u8; 32]> {
    Ok(receiver_secret()?.verifying_key().to_bytes())
}

fn accepted(currency: &str) -> Outcome<AcceptedAsset> {
    let config = Config::parse(&read_json(&fixtures().join("config/valid.json"))?.to_string())?;
    let assets = AcceptedAssets::new(&config.assets).map_err(|error| fail(format!("{error:?}")))?;
    assets
        .all()
        .iter()
        .find(|asset| asset.symbol.code() == currency)
        .copied()
        .ok_or_else(|| fail(format!("no {currency} asset")))
}

fn wire<T, E: std::fmt::Debug>(result: Result<T, E>) -> Outcome<T> {
    result.map_err(|error| fail(format!("{error:?}")))
}

/// The deterministic test keys the gateway recording is signed with. Only
/// their public halves and signatures reach the fixtures.
fn test_key(label: &str) -> SigningKey {
    SigningKey::from_bytes(&labelled("x-websearch-test/key/", label))
}

/// One asset movement a recorded receipt attests.
struct Movement<'a> {
    label: &'a str,
    operation: u8,
    activity_id: [u8; 32],
    asset: &'a AcceptedAsset,
    amount: u128,
    from: [u8; 32],
    to: [u8; 32],
}

fn encode_receipt(movement: &Movement<'_>, signature: Option<[u8; 64]>) -> Outcome<Vec<u8>> {
    let root = |field: &str| {
        labelled(
            "x-websearch-test/receipt/",
            &format!("{}/{field}", movement.label),
        )
    };
    let amount = movement.amount;
    let mut encoder = Encoder::new(4_096);
    wire(encoder.structure_header_version(0x5201, PROTOCOL_VERSION))?;
    wire(encoder.u16(PROTOCOL_VERSION))?;
    wire(encoder.bytes(&movement.activity_id, 32))?;
    wire(encoder.u64(9))?;
    wire(encoder.bytes(&root("previous-state"), 32))?;
    wire(encoder.bytes(&root("resulting-state"), 32))?;
    wire(encoder.bytes(&root("activity"), 32))?;
    wire(encoder.i32(0))?;
    wire(encoder.sequence_length(0, 512))?;
    wire(encoder.u128(1))?;
    wire(encoder.bytes(&root("batch"), 32))?;
    wire(encoder.u16(1))?;
    wire(encoder.u32(1))?;
    wire(encoder.u32(1))?;
    wire(encoder.u8(movement.operation))?;
    wire(encoder.bytes(&movement.asset.asset_id, 32))?;
    wire(encoder.u128(amount))?;
    wire(encoder.bytes(&movement.from, 32))?;
    wire(encoder.u128(amount * 100))?;
    wire(encoder.u128(amount * 99))?;
    wire(encoder.u64(1))?;
    wire(encoder.bytes(&movement.to, 32))?;
    wire(encoder.u128(amount))?;
    wire(encoder.u128(amount * 2))?;
    wire(encoder.bytes(&root("transfer-set"), 32))?;
    wire(encoder.bytes(&root("authorization"), 32))?;
    wire(encoder.bytes(&root("context"), 32))?;
    wire(encoder.u64(1_000))?;
    wire(encoder.u8(u8::from(signature.is_some())))?;
    if let Some(signature) = signature {
        wire(encoder.bytes(&signature, 64))?;
    }
    Ok(encoder.finish())
}

/// The canonical receipt `sequencer` signs for `movement`.
fn signed_receipt(movement: &Movement<'_>, sequencer: &SigningKey) -> Outcome<Vec<u8>> {
    let digest = wire(receipt_digest(&encode_receipt(movement, None)?))?;
    encode_receipt(movement, Some(sequencer.sign(&digest).to_bytes()))
}

fn exact_payment(receipt: &[u8]) -> Outcome<Value> {
    Ok(json!({
        "receipt": STANDARD.encode(receipt),
        "receiptDigest": hex(&wire(leaf_hash(receipt))?),
        "verificationLevel": "sequencer-signed",
    }))
}

/// What a payer-signed grant binds besides its signer.
struct GrantTerms {
    from: [u8; 32],
    recipient: [u8; 32],
    asset: [u8; 32],
    per_draw_maximum: u128,
    allowance: u128,
    window_length: u64,
    expiration: u64,
    purpose: [u8; 32],
}

fn signed_grant(payer: &SigningKey, terms: &GrantTerms, actor: &str) -> Outcome<(Grant, String)> {
    let recurring = terms.window_length != 0;
    let mut body = Vec::with_capacity(250);
    body.extend_from_slice(&terms.from);
    body.extend_from_slice(&terms.recipient);
    body.extend_from_slice(&terms.asset);
    body.extend_from_slice(&terms.per_draw_maximum.to_be_bytes());
    body.extend_from_slice(&terms.allowance.to_be_bytes());
    body.push(u8::from(recurring));
    body.extend_from_slice(&terms.window_length.to_be_bytes());
    body.extend_from_slice(&terms.expiration.to_be_bytes());
    body.extend_from_slice(&terms.purpose);
    body.push(0);
    body.extend_from_slice(&[0; 32]);
    body.extend_from_slice(&0_u64.to_be_bytes());
    body.extend_from_slice(&payer.verifying_key().to_bytes());
    let mut id = Sha256::new();
    id.update(Domain::AuthorityHash.tag());
    id.update(b"LXP:GRANT:v1");
    id.update(&body);
    let id: [u8; 32] = id.finalize().into();
    let grant = Grant {
        id,
        from: terms.from,
        recipient: terms.recipient,
        asset: terms.asset,
        per_draw_maximum: terms.per_draw_maximum,
        allowance: terms.allowance,
        recurring,
        window_length: terms.window_length,
        expiration: terms.expiration,
        purpose_hash: terms.purpose,
        has_reference: false,
        reference_hash: [0; 32],
        revocation_sequence: 0,
        public_key: payer.verifying_key().to_bytes(),
        signature: payer.sign(&id).to_bytes(),
    };
    let encoded = wire(Payment::IssueGrant(grant.clone()).encode(actor.as_bytes()))?;
    Ok((grant, hex(&encoded)))
}

fn rules(list: &[Value]) -> Value {
    json!({ "endpoints": { "gateway": { "*": list } } })
}

fn identity_rule(did: &str, sequence: u64, once: bool) -> Value {
    let mut rule = json!({
        "method": "lx_getSequence",
        "params": [did, "identity"],
        "result": {
            "did": did,
            "next_sequence": sequence.to_string(),
            "verification": "authenticated_node_snapshot",
        },
    });
    if once {
        rule["once"] = json!(true);
    }
    rule
}

fn account_rule(account: &[u8; 32]) -> Value {
    json!({
        "method": "lx_getSequence",
        "params": [hex(account)],
        "result": { "id": hex(account), "next_sequence": RECEIVER_SEQUENCE.to_string() },
    })
}

fn send_rule(activity: &[u8], answer: (&str, Value)) -> Value {
    let (kind, value) = answer;
    let mut rule = json!({ "method": "lx_sendActivity", "params": [hex(activity), "executed"] });
    rule[kind] = value;
    rule
}

fn completed(activity_id: &[u8; 32], receipt: &[u8]) -> Value {
    json!({ "activity_id": hex(activity_id), "receipt": hex(receipt), "state": "completed" })
}

const RECEIVER_SEQUENCE: u64 = 3;
const IDENTITY_SEQUENCE: u64 = 7;
const GRANT_EXPIRATION: u64 = NOW_MS / 1_000 + 86_400;

/// The gateway recording, rebuilt from the real draw signer, grant codec and
/// receipt encoding under the deterministic test keys.
#[allow(clippy::too_many_lines)]
fn gateway_recording() -> Outcome<Vec<(&'static str, Value)>> {
    let receiver = receiver_secret()?;
    let receiver_did = receiver_did(&receiver.verifying_key().to_bytes());
    let purpose = purpose_hash(&receiver.verifying_key().to_bytes());
    let sequencer = test_key("sequencer");
    let untrusted = test_key("untrusted-sequencer");
    let payer = test_key("payer");
    let payer_did = x_websearch::payment::receiver_did(&payer.verifying_key().to_bytes());
    let account = |did: &str, asset: &AcceptedAsset| {
        account_id(&wallet_account(did, asset)).ok_or_else(|| fail("account"))
    };
    let other_did = format!("did:layerx:{}", "22".repeat(32));
    let draw = |grant: &Grant, asset: &AcceptedAsset, identity: u64, label: &str| {
        wire(sign_draw(
            &receiver,
            &DrawRequest {
                grant,
                amount: asset.price,
                receiver_sequence: RECEIVER_SEQUENCE,
                identity_sequence: identity,
                idempotency_key: receive_key_bytes(label),
                network_id: 1,
                now_ms: NOW_MS,
            },
        ))
    };
    let terms = |asset: &AcceptedAsset| -> Outcome<GrantTerms> {
        Ok(GrantTerms {
            from: account(&payer_did, asset)?,
            recipient: account(&receiver_did, asset)?,
            asset: asset.asset_id,
            per_draw_maximum: asset.price * 10,
            allowance: asset.price * 10_000,
            window_length: 0,
            expiration: GRANT_EXPIRATION,
            purpose,
        })
    };

    let mut grants = serde_json::Map::new();
    let mut exact = serde_json::Map::new();
    let mut metered_identity = Vec::new();
    let mut metered_accounts = Vec::new();
    let mut metered_sends = Vec::new();
    let mut exact_rules = Vec::new();
    for (position, currency) in (0_u64..).zip(CURRENCIES) {
        let asset = accepted(currency)?;
        let (grant, encoded) = signed_grant(&payer, &terms(&asset)?, &receiver_did)?;
        grants.insert(currency.to_owned(), json!(encoded));
        let label = format!("metered-{currency}");
        let signed = draw(&grant, &asset, IDENTITY_SEQUENCE + position, &label)?;
        let pay_to = account(&receiver_did, &asset)?;
        let receipt = signed_receipt(
            &Movement {
                label: &label,
                operation: 6,
                activity_id: signed.activity_id,
                asset: &asset,
                amount: asset.price,
                from: account(&payer_did, &asset)?,
                to: pay_to,
            },
            &sequencer,
        )?;
        let mut result = completed(&signed.activity_id, &receipt);
        result["commitment"] = json!("executed");
        metered_identity.push(identity_rule(
            &receiver_did,
            IDENTITY_SEQUENCE + position,
            true,
        ));
        metered_accounts.push(account_rule(&pay_to));
        metered_sends.push(send_rule(&signed.canonical, ("result", result)));

        let label = format!("exact-{currency}");
        let activity_id = labelled("x-websearch-test/activity/", &label);
        let receipt = signed_receipt(
            &Movement {
                label: &label,
                operation: 5,
                activity_id,
                asset: &asset,
                amount: asset.price,
                from: account(&payer_did, &asset)?,
                to: pay_to,
            },
            &sequencer,
        )?;
        exact.insert(currency.to_owned(), exact_payment(&receipt)?);
        exact_rules.push(json!({
            "method": "lx_getReceipt",
            "params": [hex(&activity_id)],
            "result": completed(&activity_id, &receipt),
        }));
    }
    let mut metered_rules = Vec::new();
    for (identity, account) in metered_identity.into_iter().zip(metered_accounts) {
        metered_rules.push(identity);
        metered_rules.push(account);
    }
    metered_rules.extend(metered_sends);

    let sid = accepted("SID")?;
    let sid_receiver = account(&receiver_did, &sid)?;
    let sid_payer = account(&payer_did, &sid)?;
    let sid_exact =
        |label: &str, to: [u8; 32], signer: &SigningKey| -> Outcome<(Value, [u8; 32])> {
            let activity_id = labelled("x-websearch-test/activity/", label);
            let receipt = signed_receipt(
                &Movement {
                    label,
                    operation: 5,
                    activity_id,
                    asset: &sid,
                    amount: sid.price,
                    from: sid_payer,
                    to,
                },
                signer,
            )?;
            Ok((exact_payment(&receipt)?, activity_id))
        };
    let other_payee = account(&other_did, &sid)?;
    let (other_payment, _) = sid_exact("exact-other-payee", other_payee, &sequencer)?;
    let (unknown_payment, unknown_activity) = sid_exact("exact-unknown", sid_receiver, &sequencer)?;
    let (untrusted_payment, _) = sid_exact("exact-untrusted", sid_receiver, &untrusted)?;
    exact_rules.push(json!({
        "method": "lx_getReceipt",
        "params": [hex(&unknown_activity)],
        "error": { "code": -32004, "message": "unknown activity" },
    }));

    let refused = |edit: &dyn Fn(&mut GrantTerms)| -> Outcome<String> {
        let mut refused = terms(&sid)?;
        edit(&mut refused);
        Ok(signed_grant(&payer, &refused, &receiver_did)?.1)
    };
    let refused_grants = json!({
        "expired": refused(&|terms| terms.expiration = NOW_MS / 1_000 - 1)?,
        "lowLimit": refused(&|terms| {
            terms.per_draw_maximum = sid.price - 1;
            terms.allowance = (sid.price - 1) * 1_000;
        })?,
        "otherPurpose": refused(&|terms| {
            terms.purpose = labelled("x-websearch-test/purpose/", "other");
        })?,
        "otherRecipient": refused(&|terms| terms.recipient = other_payee)?,
        "recurring": refused(&|terms| terms.window_length = 3_600)?,
    });

    let (sid_grant, _) = signed_grant(&payer, &terms(&sid)?, &receiver_did)?;
    let sid_movement = |label: &'static str, activity_id: [u8; 32], amount: u128| Movement {
        label,
        operation: 6,
        activity_id,
        asset: &sid,
        amount,
        from: sid_payer,
        to: sid_receiver,
    };
    let pending = draw(&sid_grant, &sid, IDENTITY_SEQUENCE, "pending-SID")?;
    let pending_receipt = signed_receipt(
        &sid_movement("pending-SID", pending.activity_id, sid.price),
        &sequencer,
    )?;
    let pending_id = hex(&pending.activity_id);
    let pending_rules = vec![
        identity_rule(&receiver_did, IDENTITY_SEQUENCE, false),
        account_rule(&sid_receiver),
        send_rule(
            &pending.canonical,
            (
                "error",
                json!({
                    "code": -32001,
                    "data": { "activity_id": pending_id, "state": "pending" },
                    "message": "activity pending",
                }),
            ),
        ),
        json!({
            "method": "lx_getActivityStatus",
            "once": true,
            "params": [pending_id],
            "result": { "activity_id": pending_id, "state": "pending" },
        }),
        json!({
            "method": "lx_getActivityStatus",
            "params": [pending_id],
            "result": { "activity_id": pending_id, "state": "completed" },
        }),
        json!({
            "method": "lx_getReceipt",
            "params": [pending_id],
            "result": completed(&pending.activity_id, &pending_receipt),
        }),
    ];

    let refusal = |label: &str| draw(&sid_grant, &sid, IDENTITY_SEQUENCE, label);
    let other_receipt = refusal("refused-other-receipt")?;
    let untrusted_draw = refusal("refused-untrusted")?;
    let failed = refusal("refused-failed")?;
    let other_activity = refusal("refused-other-activity")?;
    let amount = refusal("refused-amount")?;
    let elsewhere = labelled("x-websearch-test/activity/", "elsewhere");
    let refusal_rules = vec![
        identity_rule(&receiver_did, IDENTITY_SEQUENCE, false),
        account_rule(&sid_receiver),
        send_rule(
            &other_receipt.canonical,
            (
                "result",
                completed(
                    &other_receipt.activity_id,
                    &signed_receipt(
                        &sid_movement(
                            "refused-other-receipt",
                            other_activity.activity_id,
                            sid.price,
                        ),
                        &sequencer,
                    )?,
                ),
            ),
        ),
        send_rule(
            &untrusted_draw.canonical,
            (
                "result",
                completed(
                    &untrusted_draw.activity_id,
                    &signed_receipt(
                        &sid_movement("refused-untrusted", untrusted_draw.activity_id, sid.price),
                        &untrusted,
                    )?,
                ),
            ),
        ),
        send_rule(
            &failed.canonical,
            (
                "result",
                json!({ "activity_id": hex(&failed.activity_id), "state": "failed" }),
            ),
        ),
        send_rule(
            &other_activity.canonical,
            (
                "result",
                completed(
                    &elsewhere,
                    &signed_receipt(
                        &sid_movement("refused-other-activity", elsewhere, sid.price),
                        &sequencer,
                    )?,
                ),
            ),
        ),
        send_rule(
            &amount.canonical,
            (
                "result",
                completed(
                    &amount.activity_id,
                    &signed_receipt(
                        &sid_movement("refused-amount", amount.activity_id, sid.price + 1),
                        &sequencer,
                    )?,
                ),
            ),
        ),
    ];

    let buyer = json!({
        "exact": exact,
        "grants": grants,
        "payerDid": payer_did,
        "refusedExact": {
            "otherPayee": other_payment,
            "unknown": unknown_payment,
            "untrusted": untrusted_payment,
        },
        "refusedGrants": refused_grants,
        "sequencerPublicKey": hex(&sequencer.verifying_key().to_bytes()),
    });
    Ok(vec![
        ("buyer.json", buyer),
        ("exact.json", rules(&exact_rules)),
        ("metered.json", rules(&metered_rules)),
        ("pending.json", rules(&pending_rules)),
        ("refusals.json", rules(&refusal_rules)),
    ])
}

fn pretty(value: &Value) -> Outcome<String> {
    Ok(format!("{}\n", serde_json::to_string_pretty(value)?))
}

#[test]
fn the_gateway_recording_is_reproduced_by_the_real_signers() -> Outcome {
    let recording = gateway_recording()?;
    let recorded = std::env::var_os(RECORD_VARIABLE).is_some();
    for (name, value) in &recording {
        let path = fixtures().join("gateway").join(name);
        if recorded {
            std::fs::write(&path, pretty(value)?)?;
        }
        assert_eq!(std::fs::read_to_string(&path)?, pretty(value)?, "{name}");
    }
    let text =
        serde_json::to_string(&recording.iter().map(|(_, value)| value).collect::<Vec<_>>())?;
    for secret in [
        hex(&test_key("sequencer").to_bytes()),
        hex(&test_key("untrusted-sequencer").to_bytes()),
        hex(&test_key("payer").to_bytes()),
        RECEIVER_SECRET.to_owned(),
    ] {
        assert!(!text.contains(&secret));
    }
    Ok(())
}

fn exchange_entry(target: &str, headers: &[(&str, &str)], reply: &Reply) -> Outcome<Value> {
    let mut request = serde_json::Map::new();
    for (name, value) in headers {
        let value = if *name == PAYMENT_SIGNATURE {
            serde_json::from_slice(&STANDARD.decode(value)?)?
        } else {
            json!(value)
        };
        request.insert((*name).to_owned(), value);
    }
    let mut response = serde_json::Map::new();
    for name in [PAYMENT_REQUIRED, PAYMENT_RESPONSE] {
        if reply.header(name).is_some() {
            response.insert(name.to_owned(), reply.decoded(name)?);
        }
    }
    if let Some(retry) = reply.header("Retry-After") {
        response.insert("Retry-After".to_owned(), json!(retry));
    }
    let mut answer = json!({ "status": reply.status, "headers": response });
    if reply.body.is_null() {
        answer["bodyBase64"] = json!(STANDARD.encode(&reply.raw));
    } else {
        answer["body"] = reply.body.clone();
    }
    Ok(json!({
        "request": { "method": "GET", "target": target, "headers": request },
        "response": answer,
    }))
}

/// The page each asset's exact fetch is recorded for.
const FETCHED: [(&str, usize); 4] = [("SID", 0), ("PAX", 1), ("USDC", 2), ("USDL", 3)];

#[test]
fn the_client_exchange_matches_the_recording() -> Outcome {
    let harness = Harness::start("client", &["metered.json", "exact.json"])?;
    let pages = vectors()?;
    let payer = harness.payer()?.to_owned();
    let grant = harness.grant("/grants/SID")?;
    let search = "/search?q=paxeer".to_owned();
    let metered = harness.signature(
        &search,
        Some(&payer),
        METERED,
        "SID",
        &metered_payload(&grant, "metered-SID"),
    )?;
    let mut steps: Vec<(String, Vec<(&str, String)>)> = vec![
        (search.clone(), vec![(PAYER_DID, payer.clone())]),
        (
            search,
            vec![(PAYER_DID, payer.clone()), (PAYMENT_SIGNATURE, metered)],
        ),
    ];
    for (currency, page) in FETCHED {
        let target = format!("/fetch?url={}", percent(&pages[page].payload));
        let payment = harness
            .buyer
            .pointer(&format!("/exact/{currency}"))
            .cloned()
            .ok_or_else(|| fail("payment"))?;
        let signature = harness.signature(&target, None, EXACT, currency, &payment)?;
        steps.push((target.clone(), vec![]));
        steps.push((target, vec![(PAYMENT_SIGNATURE, signature)]));
    }
    steps.push((format!("/content/{}", pages[0].digest), vec![]));
    let mut exchange = Vec::new();
    for (target, headers) in &steps {
        let headers: Vec<(&str, &str)> = headers
            .iter()
            .map(|(name, value)| (*name, value.as_str()))
            .collect();
        let reply = harness.get(target, &headers)?;
        exchange.push(exchange_entry(target, &headers, &reply)?);
    }
    assert_eq!(harness.served(), 5);

    let statuses: Vec<u64> = exchange
        .iter()
        .filter_map(|entry| entry.pointer("/response/status").and_then(Value::as_u64))
        .collect();
    assert_eq!(
        statuses,
        [402, 200, 402, 200, 402, 200, 402, 200, 402, 200, 200]
    );
    let settled = &exchange[1]["response"]["headers"][PAYMENT_RESPONSE];
    assert_eq!(
        settled.pointer("/extensions/layerx/purposeHash"),
        Some(&json!(hex(&purpose_hash(&receiver_public_key()?))))
    );
    for (step, (currency, page)) in FETCHED.iter().enumerate() {
        let paid = &exchange[3 + 2 * step]["response"];
        let asset = accepted(currency)?;
        assert_eq!(paid["headers"][PAYMENT_RESPONSE]["success"], true);
        assert_eq!(
            paid["headers"][PAYMENT_RESPONSE]["amount"],
            asset.price.to_string()
        );
        assert_eq!(paid["body"]["url"], pages[*page].payload);
        assert_eq!(paid["body"]["digest"], pages[*page].digest);
    }
    let stored = STANDARD.decode(text(&exchange[10], "/response/bodyBase64")?)?;
    assert_eq!(digest_hex(&content_digest(&stored)), pages[0].digest);
    let pax = accepted("PAX")?;
    assert_eq!(pax.symbol, AssetSymbol::Pax);
    let recording_text = serde_json::to_string(&exchange)?;
    assert!(!recording_text.contains(&harness.address()?.to_string()));
    assert!(!recording_text.contains(&harness.gateway.address.to_string()));

    let recorded = json!({ "exchange": exchange });
    let path = fixtures().join("client-exchange.json");
    if std::env::var_os(RECORD_VARIABLE).is_some() {
        std::fs::write(&path, pretty(&recorded)?)?;
    }
    assert_eq!(read_json(&path)?, recorded);
    Ok(())
}
