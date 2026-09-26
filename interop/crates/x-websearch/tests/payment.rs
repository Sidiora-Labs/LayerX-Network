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
use layerx_interop_gateway::adapter::{AdapterId, ConformanceSuite};
use serde_json::{json, Value};
use sha2::{Digest as _, Sha256};
use x_websearch::payment::{
    hex, PaymentGate, EXACT, METERED, PAYER_DID, PAYMENT_REQUIRED, PAYMENT_RESPONSE,
    PAYMENT_SIGNATURE,
};
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
        let mut routes = RouteTable::new();
        for route in [Route::Search, Route::Fetch, Route::Content] {
            let counter = Arc::clone(&served);
            PaymentGate::install(&gate, &mut routes, route, move |request: &Request| {
                counter.fetch_add(1, Ordering::SeqCst);
                Response::json(
                    200,
                    json!({ "path": request.path, "results": [] })
                        .to_string()
                        .into_bytes(),
                )
            })?;
        }
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
    let mut digest = Sha256::new();
    digest.update(b"x-websearch-test/receive/");
    digest.update(label.as_bytes());
    hex(&digest.finalize())
}

fn metered_payload(grant: &str, label: &str) -> Value {
    json!({ "grant": grant, "idempotencyKey": receive_key(label) })
}

struct Reply {
    status: u16,
    headers: Vec<(String, String)>,
    body: Value,
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
        let body = serde_json::from_slice(&bytes[end + 4..]).unwrap_or(Value::Null);
        Ok(Self {
            status,
            headers,
            body,
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
    }
    assert_eq!(harness.served(), 4);
    assert_eq!(harness.gateway.calls("lx_sendActivity"), 4);
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
    Ok(json!({
        "request": { "method": "GET", "target": target, "headers": request },
        "response": { "status": reply.status, "headers": response, "body": reply.body },
    }))
}

#[test]
fn the_client_exchange_matches_the_recording() -> Outcome {
    let harness = Harness::start("client", &["metered.json", "exact.json"])?;
    let payer = harness.payer()?.to_owned();
    let grant = harness.grant("/grants/SID")?;
    let exact = harness
        .buyer
        .pointer("/exact/USDC")
        .cloned()
        .ok_or_else(|| fail("payment"))?;
    let metered = harness.signature(
        "/search?q=paxeer",
        Some(&payer),
        METERED,
        "SID",
        &metered_payload(&grant, "metered-SID"),
    )?;
    let exact = harness.signature("/fetch?url=paxeer", None, EXACT, "USDC", &exact)?;
    let steps: [(&str, Vec<(&str, &str)>); 4] = [
        ("/search?q=paxeer", vec![(PAYER_DID, payer.as_str())]),
        (
            "/search?q=paxeer",
            vec![
                (PAYER_DID, payer.as_str()),
                (PAYMENT_SIGNATURE, metered.as_str()),
            ],
        ),
        ("/fetch?url=paxeer", vec![]),
        (
            "/fetch?url=paxeer",
            vec![(PAYMENT_SIGNATURE, exact.as_str())],
        ),
    ];
    let mut exchange = Vec::new();
    for (target, headers) in &steps {
        let reply = harness.get(target, headers)?;
        exchange.push(exchange_entry(target, headers, &reply)?);
    }
    assert_eq!(harness.served(), 2);
    let recorded = json!({ "exchange": exchange });
    let path = fixtures().join("client-exchange.json");
    if std::env::var_os(RECORD_VARIABLE).is_some() {
        std::fs::write(
            &path,
            format!("{}\n", serde_json::to_string_pretty(&recorded)?),
        )?;
    }
    assert_eq!(read_json(&path)?, recorded);
    Ok(())
}
