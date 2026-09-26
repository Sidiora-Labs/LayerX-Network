use std::collections::BTreeSet;
use std::fmt::Write as _;
use std::io::{BufRead as _, BufReader, Write as _};
use std::net::{SocketAddr, TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU32, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;

use base64::engine::general_purpose::STANDARD;
use base64::Engine as _;
use layerx_agentd::budget::{BudgetLimiter, LimitConfig, LimitId, LimitScope};
use layerx_agentd::capability::{Capability, CapabilityDimensions, CapabilityId, RateCeiling};
use layerx_agentd::identity::{
    register, CoreIdentity, IdentityError, IdentityResolver, ProtocolAuthority,
};
use layerx_agentd::policy::approval::{
    ApprovalContext, ApprovalRegistry, ApprovalState, ApproverId,
};
use layerx_agentd::prepare::PreparationLifecycle;
use layerx_agentd::session::{open, OpenRequest, SessionId, SessionRegistry};
use layerx_agentd::session_control::SessionControl;
use layerx_agentd::store::{Store, TenantId};
use layerx_mcp::approval::{approve, reject, ApprovalPolicy};
use layerx_mcp::boundary::{AgentSurface, ProgramReads};
use layerx_mcp::catalogue::{self, ArgumentError, WEB_TOOLS};
use layerx_mcp::server::{Server, WebBoundary, WebRoute};
use layerx_mcp::stdio::{Bound, Session};
use layerx_mcp::tools::web::{
    canonical_bytes, content_digest, web_tool, Currency, ExactTerms, GrantTerms, Scheme,
    WebApproval, WebCall, WebConfig, WebContent, WebFetch, WebOperation, WebPayer, WebPayerError,
    WebResult, WebSearch, WebToolError,
};
use layerx_types::ids::Did;
use layerx_types::verify::VerificationLevel;
use serde_json::{json, Value};
use sha2::{Digest as _, Sha256};
use sha3::Keccak256;

const NETWORK: &str = "layerx:1";
const RECORDED_FETCH_URL: &str = "paxeer";
const TAMPERED_PAYER: &str = "4aa4897f08c81fcd9b003dfb8f4a369a9ed828f91ab228e04b14e02f03727aaf";
const ROUTED_FETCH_URL: &str = "https://paxeer.app/index.html";
const OBSERVED_SEQUENCE: u64 = 50;
const AGENT_BEARER: &str = "agent-bearer-0123456789abcdef0123456789abcdef";

static NEXT_DIRECTORY: AtomicU64 = AtomicU64::new(1);

fn fixture(name: &str) -> Value {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../../interop/crates/x-websearch/tests/fixtures")
        .join(name);
    let text = std::fs::read_to_string(&path)
        .unwrap_or_else(|error| panic!("fixture {}: {error}", path.display()));
    serde_json::from_str(&text).unwrap_or_else(|error| panic!("fixture {name}: {error}"))
}

fn text<'a>(value: &'a Value, pointer: &str) -> &'a str {
    value
        .pointer(pointer)
        .and_then(Value::as_str)
        .unwrap_or_else(|| panic!("fixture field {pointer}"))
}

fn unhex(text: &str) -> Vec<u8> {
    assert!(text.len().is_multiple_of(2), "odd hex");
    (0..text.len())
        .step_by(2)
        .map(|index| {
            u8::from_str_radix(&text[index..index + 2], 16)
                .unwrap_or_else(|error| panic!("hex: {error}"))
        })
        .collect()
}

fn unhex32(text: &str) -> [u8; 32] {
    unhex(text)
        .try_into()
        .unwrap_or_else(|_| panic!("32-byte hex"))
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().fold(String::new(), |mut output, byte| {
        let _ = write!(output, "{byte:02x}");
        output
    })
}

/// What the replaying sidecar changes from the recording for one test.
#[derive(Clone, Default)]
struct Script {
    fetch_body: Option<Vec<u8>>,
    content: Option<(String, Vec<u8>)>,
    served: Vec<(String, Vec<u8>)>,
    tamper_payer: bool,
    pending_first: bool,
    refuse_settlement: bool,
}

/// A loopback listener that answers exactly the recorded client exchange.
struct Replay {
    address: SocketAddr,
    signatures: Arc<AtomicU32>,
}

impl Replay {
    fn start(script: Script) -> Self {
        let listener =
            TcpListener::bind("127.0.0.1:0").unwrap_or_else(|error| panic!("bind: {error}"));
        let address = listener
            .local_addr()
            .unwrap_or_else(|error| panic!("address: {error}"));
        let signatures = Arc::new(AtomicU32::new(0));
        let counter = Arc::clone(&signatures);
        let exchange = fixture("client-exchange.json")
            .get("exchange")
            .and_then(Value::as_array)
            .cloned()
            .unwrap_or_else(|| panic!("recorded exchange"));
        thread::spawn(move || {
            for stream in listener.incoming().flatten() {
                answer(stream, &exchange, &script, &counter);
            }
        });
        Self {
            address,
            signatures,
        }
    }

    fn endpoint(&self) -> String {
        format!("http://{}", self.address)
    }

    fn signatures(&self) -> u32 {
        self.signatures.load(Ordering::SeqCst)
    }
}

struct Incoming {
    method: String,
    target: String,
    headers: Vec<(String, String)>,
}

fn read_request(stream: &TcpStream) -> Option<Incoming> {
    let mut reader = BufReader::new(stream);
    let mut line = String::new();
    reader.read_line(&mut line).ok()?;
    let mut parts = line.split_whitespace();
    let method = parts.next()?.to_owned();
    let target = parts.next()?.to_owned();
    let mut headers = Vec::new();
    loop {
        let mut header = String::new();
        reader.read_line(&mut header).ok()?;
        let header = header.trim_end();
        if header.is_empty() {
            break;
        }
        let (name, value) = header.split_once(':')?;
        headers.push((name.trim().to_owned(), value.trim().to_owned()));
    }
    Some(Incoming {
        method,
        target,
        headers,
    })
}

fn header<'a>(incoming: &'a Incoming, name: &str) -> Option<&'a str> {
    incoming
        .headers
        .iter()
        .find(|(candidate, _)| candidate.eq_ignore_ascii_case(name))
        .map(|(_, value)| value.as_str())
}

fn decode(header: &str) -> Option<Value> {
    serde_json::from_slice(&STANDARD.decode(header).ok()?).ok()
}

fn encode(value: &Value) -> String {
    STANDARD.encode(serde_json::to_vec(value).unwrap_or_else(|error| panic!("encode: {error}")))
}

/// The recorded entry this request replays: same method, target and header
/// names, and for a paid request the same version, offer and scheme payload.
fn recorded<'a>(exchange: &'a [Value], incoming: &Incoming, target: &str) -> Option<&'a Value> {
    let names: BTreeSet<String> = incoming
        .headers
        .iter()
        .map(|(name, _)| name.to_ascii_uppercase())
        .filter(|name| name.starts_with("PAYMENT-") || name.starts_with("LAYERX-"))
        .collect();
    exchange.iter().find(|entry| {
        let request = &entry["request"];
        let recorded_names: BTreeSet<String> = request["headers"]
            .as_object()
            .map(|headers| headers.keys().cloned().collect())
            .unwrap_or_default();
        if request["method"] != incoming.method.as_str()
            || request["target"] != target
            || recorded_names != names
        {
            return false;
        }
        let payer_matches = request["headers"]
            .get("LAYERX-PAYER-DID")
            .is_none_or(|payer| header(incoming, "LAYERX-PAYER-DID") == payer.as_str());
        let signature_matches =
            request["headers"]
                .get("PAYMENT-SIGNATURE")
                .is_none_or(|expected| {
                    header(incoming, "PAYMENT-SIGNATURE")
                        .and_then(decode)
                        .is_some_and(|sent| {
                            sent["x402Version"] == expected["x402Version"]
                                && sent["accepted"] == expected["accepted"]
                                && sent["payload"] == expected["payload"]
                        })
                });
        payer_matches && signature_matches
    })
}

fn answer(mut stream: TcpStream, exchange: &[Value], script: &Script, signatures: &AtomicU32) {
    let Some(incoming) = read_request(&stream) else {
        return;
    };
    let content = script
        .content
        .iter()
        .chain(script.served.iter())
        .find(|(target, _)| *target == incoming.target);
    let target = if content.is_some() {
        format!("/fetch?url={RECORDED_FETCH_URL}")
    } else {
        incoming.target.clone()
    };
    let Some(entry) = recorded(exchange, &incoming, &target) else {
        respond(&mut stream, 400, &[], br#"{"error":"unrecorded_request"}"#);
        return;
    };
    if header(&incoming, "PAYMENT-SIGNATURE").is_some() {
        let count = signatures.fetch_add(1, Ordering::SeqCst) + 1;
        if script.pending_first && count == 1 {
            let retry = [("Retry-After".to_owned(), "1".to_owned())];
            respond(&mut stream, 503, &retry, br#"{"error":"payment_pending"}"#);
            return;
        }
        if script.refuse_settlement {
            refuse(&mut stream, exchange, &target);
            return;
        }
    }
    let response = &entry["response"];
    let mut headers = Vec::new();
    for (name, value) in response["headers"].as_object().into_iter().flatten() {
        let mut value = value.clone();
        if name == "PAYMENT-RESPONSE" && script.tamper_payer {
            value["payer"] = json!(TAMPERED_PAYER);
        }
        headers.push((name.clone(), encode(&value)));
    }
    let status = u16::try_from(response["status"].as_u64().unwrap_or(500)).unwrap_or(500);
    let paid = status == 200;
    let body = match (content, paid, target.starts_with("/fetch")) {
        (Some((_, bytes)), true, _) => bytes.clone(),
        (None, true, true) if script.fetch_body.is_some() => {
            script.fetch_body.clone().unwrap_or_default()
        }
        _ => serde_json::to_vec(&response["body"]).unwrap_or_default(),
    };
    respond(&mut stream, status, &headers, &body);
}

/// The sidecar's refusal: a fresh challenge carrying the refused settlement.
fn refuse(stream: &mut TcpStream, exchange: &[Value], target: &str) {
    let challenge = exchange
        .iter()
        .find(|entry| entry["request"]["target"] == target && entry["response"]["status"] == 402)
        .unwrap_or_else(|| panic!("recorded challenge"));
    let required = &challenge["response"]["headers"]["PAYMENT-REQUIRED"];
    let refusal = json!({
        "success": false,
        "errorReason": "insufficient_funds",
        "transaction": "",
        "network": NETWORK,
    });
    let headers = [
        ("PAYMENT-REQUIRED".to_owned(), encode(required)),
        ("PAYMENT-RESPONSE".to_owned(), encode(&refusal)),
    ];
    let body = json!({ "error": "insufficient_funds", "paymentRequired": required });
    respond(stream, 402, &headers, body.to_string().as_bytes());
}

fn respond(stream: &mut TcpStream, status: u16, headers: &[(String, String)], body: &[u8]) {
    let mut head = format!(
        "HTTP/1.1 {status} Replay\r\nContent-Length: {}\r\nConnection: close\r\n",
        body.len()
    );
    for (name, value) in headers {
        let _ = write!(head, "{name}: {value}\r\n");
    }
    head.push_str("\r\n");
    let _ = stream
        .write_all(head.as_bytes())
        .and_then(|()| stream.write_all(body));
}

/// The payer's recorded artifacts: the payer-signed grant and the
/// sequencer-signed receipt the recorded requests carry.
struct RecordedPayer {
    grant: Vec<u8>,
    receipt: Vec<u8>,
    grants: Vec<GrantTerms>,
    payments: Vec<ExactTerms>,
}

impl RecordedPayer {
    fn new() -> Self {
        let buyer = fixture("gateway/buyer.json");
        Self {
            grant: unhex(text(&buyer, "/grants/SID")),
            receipt: STANDARD
                .decode(text(&buyer, "/exact/USDC/receipt"))
                .unwrap_or_else(|error| panic!("receipt: {error}")),
            grants: Vec::new(),
            payments: Vec::new(),
        }
    }

    fn with_grant(grant: Vec<u8>) -> Self {
        Self {
            grant,
            ..Self::new()
        }
    }

    fn with_receipt(receipt: Vec<u8>) -> Self {
        Self {
            receipt,
            ..Self::new()
        }
    }
}

impl WebPayer for RecordedPayer {
    fn grant(&mut self, terms: &GrantTerms) -> Result<Vec<u8>, WebPayerError> {
        self.grants.push(terms.clone());
        Ok(self.grant.clone())
    }

    fn pay(&mut self, terms: &ExactTerms) -> Result<Vec<u8>, WebPayerError> {
        self.payments.push(terms.clone());
        Ok(self.receipt.clone())
    }
}

fn payer_did() -> String {
    text(&fixture("gateway/buyer.json"), "/payerDid").to_owned()
}

fn sequencer_key() -> [u8; 32] {
    unhex32(text(&fixture("gateway/buyer.json"), "/sequencerPublicKey"))
}

fn config(replay: &Replay, key: [u8; 32]) -> WebConfig {
    WebConfig::new(
        &replay.endpoint(),
        &payer_did(),
        NETWORK,
        key,
        Duration::from_secs(5),
        3,
    )
    .unwrap_or_else(|error| panic!("config: {error:?}"))
}

fn context() -> ApprovalContext {
    ApprovalContext {
        tenant: TenantId::new("tenant-a").unwrap_or_else(|error| panic!("tenant: {error}")),
        agent: Did::new(payer_did().as_bytes()).unwrap_or_else(|error| panic!("DID: {error:?}")),
        session: SessionId([7; 32]),
        capability: CapabilityId([9; 32]),
        policy_version: "policy-v1".to_owned(),
        request_id: [0; 32],
    }
}

fn approval(registry: &ApprovalRegistry, threshold: u128) -> WebApproval<'_> {
    WebApproval {
        registry,
        policy: ApprovalPolicy {
            amount_threshold: threshold,
        },
        context: context(),
        current_sequence: 10,
    }
}

fn metered_key() -> [u8; 32] {
    Sha256::digest(b"x-websearch-test/receive/metered-SID").into()
}

fn search_call() -> WebCall {
    WebCall {
        operation: WebOperation::Search(WebSearch {
            query: "paxeer".to_owned(),
        }),
        currency: Currency::Sid,
        scheme: Scheme::Metered,
        idempotency_key: metered_key(),
    }
}

fn fetch_call() -> WebCall {
    WebCall {
        operation: WebOperation::Fetch(WebFetch {
            url: RECORDED_FETCH_URL.to_owned(),
        }),
        currency: Currency::Usdc,
        scheme: Scheme::Exact,
        idempotency_key: [0x5a; 32],
    }
}

/// An encoder written from the canonical content definition, independent of
/// the tool's own.
fn reference_canonical(kind: u8, payload: &[u8], media_type: &str, text: &str) -> Vec<u8> {
    let mut bytes = b"PAXEERX_WEB_CONTENT_V1".to_vec();
    bytes.push(kind);
    bytes.extend_from_slice(
        &u32::try_from(payload.len())
            .unwrap_or(u32::MAX)
            .to_be_bytes(),
    );
    bytes.extend_from_slice(payload);
    bytes.extend_from_slice(
        &u32::try_from(media_type.len())
            .unwrap_or(u32::MAX)
            .to_be_bytes(),
    );
    bytes.extend_from_slice(media_type.as_bytes());
    bytes.extend_from_slice(&u64::try_from(text.len()).unwrap_or(u64::MAX).to_be_bytes());
    bytes.extend_from_slice(text.as_bytes());
    bytes
}

fn vectors() -> Vec<Value> {
    fixture("content-vectors.json")["vectors"]
        .as_array()
        .cloned()
        .unwrap_or_else(|| panic!("content vectors"))
}

fn fetch_body(tamper_text: bool) -> (Vec<u8>, [u8; 32]) {
    fetch_answer(RECORDED_FETCH_URL, tamper_text)
}

fn fetch_answer(url: &str, tamper_text: bool) -> (Vec<u8>, [u8; 32]) {
    let vector = &vectors()[1];
    let media_type = text(vector, "/media_type");
    let page = text(vector, "/text");
    let digest: [u8; 32] =
        Keccak256::digest(reference_canonical(1, url.as_bytes(), media_type, page)).into();
    let served = if tamper_text {
        format!("{page} altered")
    } else {
        page.to_owned()
    };
    let body = json!({
        "url": url,
        "final_url": url,
        "media_type": media_type,
        "digest": hex(&digest),
        "length": served.len(),
        "text": served,
    });
    (body.to_string().into_bytes(), digest)
}

#[test]
fn canonical_content_matches_the_sidecar_vectors() {
    for vector in vectors() {
        let expected = unhex32(text(&vector, "/digest"));
        let payload = text(&vector, "/payload").as_bytes();
        let media_type = text(&vector, "/media_type");
        let page = text(&vector, "/text");
        let reference: [u8; 32] =
            Keccak256::digest(reference_canonical(1, payload, media_type, page)).into();
        assert_eq!(reference, expected);
        let canonical = canonical_bytes(1, payload, media_type, page)
            .unwrap_or_else(|error| panic!("canonical: {error:?}"));
        assert_eq!(content_digest(&canonical), expected);
    }
}

#[test]
fn paid_metered_search_settles_and_releases_untrusted_results() {
    let replay = Replay::start(Script::default());
    let registry = ApprovalRegistry::default();
    let mut payer = RecordedPayer::new();
    let outcome = web_tool(
        &config(&replay, sequencer_key()),
        &search_call(),
        &approval(&registry, u128::MAX),
        &mut payer,
    )
    .unwrap_or_else(|error| panic!("search: {error:?}"));
    assert_eq!(replay.signatures(), 1);
    assert_eq!(payer.grants.len(), 1);
    assert!(payer.payments.is_empty());
    assert_eq!(payer.grants[0].amount, 3114);
    assert_eq!(payer.grants[0].idempotency_key, metered_key());
    let settlement = &outcome.settlement;
    assert_eq!(
        hex(&settlement.receipt_digest),
        "54c4a7b999f9e107c0dd700f98dd39de2987a060a5e4b5c6e1bc324fd71e0fa5"
    );
    assert_eq!(
        hex(&settlement.payer),
        "f750fa06be3076a21a9b5ac018da732ee0ee18e42b9ffcb15b5dc3528ea65949"
    );
    assert_eq!(settlement.amount, 3114);
    assert_eq!(
        outcome.result,
        WebResult::Search {
            opaque_results: Vec::new()
        }
    );
    let evidence = outcome.evidence();
    assert_eq!(evidence["untrusted"], true);
    assert_eq!(evidence["tool"], "web.search");
    assert_eq!(
        evidence["settlement"]["verificationLevel"],
        "sequencer-signed"
    );
    assert_eq!(
        evidence["settlement"]["transaction"],
        "lxp:54c4a7b999f9e107c0dd700f98dd39de2987a060a5e4b5c6e1bc324fd71e0fa5"
    );
    assert!(registry
        .ticket(metered_key())
        .unwrap_or_else(|error| panic!("ticket: {error:?}"))
        .is_none());
}

#[test]
fn paid_exact_fetch_settles_and_checks_the_content_digest() {
    let (body, digest) = fetch_body(false);
    let replay = Replay::start(Script {
        fetch_body: Some(body),
        ..Script::default()
    });
    let registry = ApprovalRegistry::default();
    let mut payer = RecordedPayer::new();
    let outcome = web_tool(
        &config(&replay, sequencer_key()),
        &fetch_call(),
        &approval(&registry, u128::MAX),
        &mut payer,
    )
    .unwrap_or_else(|error| panic!("fetch: {error:?}"));
    assert_eq!(replay.signatures(), 1);
    assert_eq!(payer.payments.len(), 1);
    assert_eq!(payer.payments[0].amount, 1000);
    assert_eq!(
        hex(&outcome.settlement.receipt_digest),
        "5088d0659995e37111051d23861b813c29d221021f5b6043831e9f191ca0fc36"
    );
    let WebResult::Fetch {
        digest: released,
        opaque_text,
        media_type,
        ..
    } = &outcome.result
    else {
        panic!("fetch result");
    };
    assert_eq!(*released, digest);
    assert_eq!(opaque_text, text(&vectors()[1], "/text"));
    assert_eq!(media_type, "text/plain");
    assert_eq!(outcome.evidence()["content"]["digest"], hex(&digest));
}

#[test]
fn paid_content_by_digest_releases_only_matching_bytes() {
    let vector = &vectors()[2];
    let digest = unhex32(text(vector, "/digest"));
    let canonical = reference_canonical(
        1,
        text(vector, "/payload").as_bytes(),
        text(vector, "/media_type"),
        text(vector, "/text"),
    );
    let call = WebCall {
        operation: WebOperation::Content(WebContent { digest }),
        ..fetch_call()
    };
    let target = format!("/content/{}", hex(&digest));
    let replay = Replay::start(Script {
        content: Some((target.clone(), canonical)),
        ..Script::default()
    });
    let registry = ApprovalRegistry::default();
    let outcome = web_tool(
        &config(&replay, sequencer_key()),
        &call,
        &approval(&registry, u128::MAX),
        &mut RecordedPayer::new(),
    )
    .unwrap_or_else(|error| panic!("content: {error:?}"));
    let WebResult::Content {
        kind,
        opaque_payload,
        media_type,
        opaque_text,
        ..
    } = &outcome.result
    else {
        panic!("content result");
    };
    assert_eq!(*kind, 1);
    assert_eq!(opaque_payload, text(vector, "/payload").as_bytes());
    assert_eq!(media_type, "application/json");
    assert_eq!(opaque_text, text(vector, "/text"));

    let mut altered = reference_canonical(
        1,
        text(vector, "/payload").as_bytes(),
        text(vector, "/media_type"),
        text(vector, "/text"),
    );
    if let Some(last) = altered.last_mut() {
        *last ^= 1;
    }
    let tampered = Replay::start(Script {
        content: Some((target, altered)),
        ..Script::default()
    });
    let refused = web_tool(
        &config(&tampered, sequencer_key()),
        &call,
        &approval(&registry, u128::MAX),
        &mut RecordedPayer::new(),
    );
    assert!(
        matches!(refused, Err(WebToolError::DigestMismatch)),
        "{refused:?}"
    );
}

#[test]
fn fetch_with_a_mismatched_digest_is_refused() {
    let (body, _) = fetch_body(true);
    let replay = Replay::start(Script {
        fetch_body: Some(body),
        ..Script::default()
    });
    let registry = ApprovalRegistry::default();
    let refused = web_tool(
        &config(&replay, sequencer_key()),
        &fetch_call(),
        &approval(&registry, u128::MAX),
        &mut RecordedPayer::new(),
    );
    assert!(
        matches!(refused, Err(WebToolError::DigestMismatch)),
        "{refused:?}"
    );
    assert_eq!(replay.signatures(), 1);
}

#[test]
fn an_unapproved_spend_is_held_and_never_paid() {
    let replay = Replay::start(Script::default());
    let registry = ApprovalRegistry::default();
    let config = config(&replay, sequencer_key());
    let held = approval(&registry, 0);
    let mut payer = RecordedPayer::new();
    let refused = web_tool(&config, &search_call(), &held, &mut payer);
    let Err(WebToolError::ApprovalRequired(ticket)) = refused else {
        panic!("expected an approval hold, got {refused:?}");
    };
    assert_eq!(ticket.hold_id, metered_key());
    assert_eq!(ticket.state, ApprovalState::AwaitingApproval);
    assert_eq!(ticket.disclosure.amounts.values()[0].amount.0, 3114);
    assert_eq!(replay.signatures(), 0);
    assert!(payer.grants.is_empty());

    let pending = web_tool(&config, &search_call(), &held, &mut payer);
    assert!(
        matches!(pending, Err(WebToolError::ApprovalPending(_))),
        "{pending:?}"
    );
    assert_eq!(replay.signatures(), 0);
    assert!(payer.grants.is_empty());

    let approver = ApproverId::new("operator").unwrap_or_else(|error| panic!("{error:?}"));
    approve(&registry, ticket.hold_id, approver, &ticket.disclosure, 11)
        .unwrap_or_else(|error| panic!("approve: {error:?}"));
    let outcome = web_tool(&config, &search_call(), &held, &mut payer)
        .unwrap_or_else(|error| panic!("approved search: {error:?}"));
    assert_eq!(outcome.settlement.amount, 3114);
    assert_eq!(replay.signatures(), 1);
    assert_eq!(payer.grants.len(), 1);
}

#[test]
fn a_rejected_spend_is_refused_without_payment() {
    let replay = Replay::start(Script::default());
    let registry = ApprovalRegistry::default();
    let config = config(&replay, sequencer_key());
    let held = approval(&registry, 0);
    let mut payer = RecordedPayer::new();
    let Err(WebToolError::ApprovalRequired(ticket)) =
        web_tool(&config, &search_call(), &held, &mut payer)
    else {
        panic!("expected an approval hold");
    };
    let approver = ApproverId::new("operator").unwrap_or_else(|error| panic!("{error:?}"));
    reject(&registry, ticket.hold_id, approver, &ticket.disclosure, 11)
        .unwrap_or_else(|error| panic!("reject: {error:?}"));
    let refused = web_tool(&config, &search_call(), &held, &mut payer);
    assert!(
        matches!(
            refused,
            Err(WebToolError::ApprovalRefused(ApprovalState::Rejected))
        ),
        "{refused:?}"
    );
    assert_eq!(replay.signatures(), 0);
    assert!(payer.grants.is_empty());
}

#[test]
fn a_refused_settlement_releases_nothing() {
    let replay = Replay::start(Script {
        refuse_settlement: true,
        ..Script::default()
    });
    let registry = ApprovalRegistry::default();
    let refused = web_tool(
        &config(&replay, sequencer_key()),
        &search_call(),
        &approval(&registry, u128::MAX),
        &mut RecordedPayer::new(),
    );
    assert!(
        matches!(&refused, Err(WebToolError::SettlementRefused(Some(reason))) if reason == "insufficient_funds"),
        "{refused:?}"
    );
    assert_eq!(replay.signatures(), 1);
}

#[test]
fn a_settlement_under_another_sequencer_key_is_refused() {
    let replay = Replay::start(Script::default());
    let registry = ApprovalRegistry::default();
    let refused = web_tool(
        &config(&replay, [7; 32]),
        &search_call(),
        &approval(&registry, u128::MAX),
        &mut RecordedPayer::new(),
    );
    assert!(
        matches!(refused, Err(WebToolError::SettlementUnverified)),
        "{refused:?}"
    );
    assert_eq!(replay.signatures(), 1);
}

#[test]
fn a_settlement_naming_another_payer_is_refused() {
    let replay = Replay::start(Script {
        tamper_payer: true,
        ..Script::default()
    });
    let registry = ApprovalRegistry::default();
    let refused = web_tool(
        &config(&replay, sequencer_key()),
        &search_call(),
        &approval(&registry, u128::MAX),
        &mut RecordedPayer::new(),
    );
    assert!(
        matches!(refused, Err(WebToolError::SettlementMismatch)),
        "{refused:?}"
    );
}

#[test]
fn a_pending_settlement_is_retried_with_the_same_signature() {
    let replay = Replay::start(Script {
        pending_first: true,
        ..Script::default()
    });
    let registry = ApprovalRegistry::default();
    let mut payer = RecordedPayer::new();
    let outcome = web_tool(
        &config(&replay, sequencer_key()),
        &search_call(),
        &approval(&registry, u128::MAX),
        &mut payer,
    )
    .unwrap_or_else(|error| panic!("pending search: {error:?}"));
    assert_eq!(outcome.settlement.amount, 3114);
    assert_eq!(replay.signatures(), 2);
    assert_eq!(payer.grants.len(), 1);
}

#[test]
fn payer_artifacts_that_do_not_bind_the_offer_are_never_sent() {
    let buyer = fixture("gateway/buyer.json");
    let replay = Replay::start(Script::default());
    let registry = ApprovalRegistry::default();
    let config = config(&replay, sequencer_key());
    let other = unhex(text(&buyer, "/refusedGrants/otherRecipient"));
    let refused = web_tool(
        &config,
        &search_call(),
        &approval(&registry, u128::MAX),
        &mut RecordedPayer::with_grant(other),
    );
    assert!(
        matches!(refused, Err(WebToolError::PaymentArtifact("grant"))),
        "{refused:?}"
    );
    let receipt = STANDARD
        .decode(text(&buyer, "/refusedExact/otherPayee/receipt"))
        .unwrap_or_else(|error| panic!("receipt: {error}"));
    let refused = web_tool(
        &config,
        &fetch_call(),
        &approval(&registry, u128::MAX),
        &mut RecordedPayer::with_receipt(receipt),
    );
    assert!(
        matches!(refused, Err(WebToolError::PaymentArtifact("receipt"))),
        "{refused:?}"
    );
    assert_eq!(replay.signatures(), 0);
}

#[test]
fn web_arguments_are_validated_by_the_catalogue() {
    let key = "11".repeat(32);
    let call = WebCall::from_arguments(
        "web.fetch",
        &json!({
            "url": "https://paxeer.app/index.html",
            "currency": "USDC",
            "scheme": "exact",
            "idempotency_key": key,
        }),
    )
    .unwrap_or_else(|error| panic!("fetch arguments: {error:?}"));
    assert_eq!(call.currency, Currency::Usdc);
    assert_eq!(call.scheme, Scheme::Exact);
    assert_eq!(
        call.operation.target(),
        "/fetch?url=https%3A%2F%2Fpaxeer.app%2Findex.html"
    );
    let search = WebCall::from_arguments(
        "web.search",
        &json!({"query": "paxeer", "currency": "SID", "scheme": "metered", "idempotency_key": key}),
    )
    .unwrap_or_else(|error| panic!("search arguments: {error:?}"));
    assert_eq!(search.operation.target(), "/search?q=paxeer");
    let digest = "ab".repeat(32);
    let content = WebCall::from_arguments(
        "web.content",
        &json!({"digest": digest, "currency": "PAX", "scheme": "exact", "idempotency_key": key}),
    )
    .unwrap_or_else(|error| panic!("content arguments: {error:?}"));
    assert_eq!(content.operation.target(), format!("/content/{digest}"));

    let base =
        json!({"query": "q", "currency": "SID", "scheme": "metered", "idempotency_key": key});
    let mut unknown = base.clone();
    unknown["extra"] = json!("x");
    assert_eq!(
        WebCall::from_arguments("web.search", &unknown),
        Err(ArgumentError::Unknown("extra".to_owned()))
    );
    let mut currency = base.clone();
    currency["currency"] = json!("ETH");
    assert_eq!(
        WebCall::from_arguments("web.search", &currency),
        Err(ArgumentError::Malformed("currency"))
    );
    let mut scheme = base.clone();
    scheme["scheme"] = json!("upto");
    assert_eq!(
        WebCall::from_arguments("web.search", &scheme),
        Err(ArgumentError::Malformed("scheme"))
    );
    let mut control = base.clone();
    control["query"] = json!("a\u{0}b");
    assert_eq!(
        WebCall::from_arguments("web.search", &control),
        Err(ArgumentError::Malformed("query"))
    );
    let mut zero = base.clone();
    zero["idempotency_key"] = json!("0".repeat(64));
    assert_eq!(
        WebCall::from_arguments("web.search", &zero),
        Err(ArgumentError::Malformed("idempotency_key"))
    );
    let local = json!({"url": "file:///etc/passwd", "currency": "SID", "scheme": "exact", "idempotency_key": key});
    assert_eq!(
        WebCall::from_arguments("web.fetch", &local),
        Err(ArgumentError::Malformed("url"))
    );
    assert_eq!(
        WebCall::from_arguments("wallet.send", &base),
        Err(ArgumentError::Unknown("wallet.send".to_owned()))
    );
}

#[test]
fn web_tools_are_listed_as_untrusted_output() {
    assert_eq!(catalogue::web_surface().len(), 3);
    for tool in WEB_TOOLS {
        let listing = catalogue::listing(tool).unwrap_or_else(|| panic!("listing {}", tool.name));
        assert_eq!(listing["_meta"]["layerx/output"], "untrusted");
        assert_eq!(listing["annotations"]["readOnlyHint"], false);
        assert!(catalogue::untrusted_output(tool.name));
        let schema = catalogue::input_schema(tool.name).unwrap_or_else(|| panic!("schema"));
        assert_eq!(schema["additionalProperties"], false);
        assert_eq!(schema["required"].as_array().map(Vec::len), Some(4));
    }
    let wallet = catalogue::surface(layerx_mcp::server::DeploymentMode::Full)
        .into_iter()
        .find(|tool| tool.name == "wallet.send")
        .unwrap_or_else(|| panic!("wallet.send"));
    let listing = catalogue::listing(wallet).unwrap_or_else(|| panic!("wallet listing"));
    assert!(listing["_meta"].get("layerx/output").is_none());
    assert!(!catalogue::untrusted_output("wallet.send"));
}

#[test]
fn configuration_fails_closed_naming_the_field() {
    let key = sequencer_key();
    let did = payer_did();
    let second = Duration::from_secs(1);
    let refused = |result: Result<WebConfig, WebToolError>| match result {
        Err(WebToolError::Configuration(field)) => field,
        other => panic!("expected a configuration refusal, got {other:?}"),
    };
    assert_eq!(
        refused(WebConfig::new(
            "https://paxeer.app",
            &did,
            NETWORK,
            key,
            second,
            1
        )),
        "endpoint"
    );
    assert_eq!(
        refused(WebConfig::new(
            "http://127.0.0.1:1/path",
            &did,
            NETWORK,
            key,
            second,
            1
        )),
        "endpoint"
    );
    assert_eq!(
        refused(WebConfig::new(
            "http://127.0.0.1:1",
            "payer",
            NETWORK,
            key,
            second,
            1
        )),
        "payer_did"
    );
    assert_eq!(
        refused(WebConfig::new(
            "http://127.0.0.1:1",
            &did,
            "eip155:1",
            key,
            second,
            1
        )),
        "network"
    );
    assert_eq!(
        refused(WebConfig::new(
            "http://127.0.0.1:1",
            &did,
            NETWORK,
            [0; 32],
            second,
            1
        )),
        "sequencer_public_key"
    );
    assert_eq!(
        refused(WebConfig::new(
            "http://127.0.0.1:1",
            &did,
            NETWORK,
            key,
            Duration::ZERO,
            1
        )),
        "timeout"
    );
    assert_eq!(
        refused(WebConfig::new(
            "http://127.0.0.1:1",
            &did,
            NETWORK,
            key,
            second,
            0
        )),
        "pending_attempts"
    );
}

struct BoundaryIdentity(CoreIdentity);

impl IdentityResolver for BoundaryIdentity {
    fn resolve(&mut self, _did: &Did) -> Result<Option<CoreIdentity>, IdentityError> {
        Ok(Some(self.0.clone()))
    }
}

fn directory(label: &str) -> PathBuf {
    let sequence = NEXT_DIRECTORY.fetch_add(1, Ordering::Relaxed);
    std::env::temp_dir().join(format!(
        "layerx-mcp-web-{label}-{}-{sequence}",
        std::process::id()
    ))
}

/// Replays the verified program balance route of the agent daemon on
/// loopback, so the bound server reads its core sequence the ordinary way.
fn agent_daemon() -> String {
    let listener =
        TcpListener::bind("127.0.0.1:0").unwrap_or_else(|error| panic!("agent listener: {error}"));
    let endpoint = listener
        .local_addr()
        .unwrap_or_else(|error| panic!("agent address: {error}"))
        .to_string();
    thread::spawn(move || {
        for mut stream in listener.incoming().flatten() {
            let Some(incoming) = read_request(&stream) else {
                continue;
            };
            let bearer = format!("Bearer {AGENT_BEARER}");
            let authorized = header(&incoming, "Authorization") == Some(bearer.as_str());
            let program = incoming
                .target
                .strip_prefix("/v1/programs/")
                .and_then(|rest| rest.strip_suffix("/balances"));
            let (status, body) = match program {
                Some(program) if authorized => (
                    200,
                    json!({
                        "program": program,
                        "lifecycle": "active",
                        "accounts": [],
                        "freshness": {
                            "observed_sequence": OBSERVED_SEQUENCE,
                            "observed_at": 1,
                            "receipt_digest": "33".repeat(32),
                            "state_root": "44".repeat(32),
                            "valid_through": 400,
                        },
                    }),
                ),
                Some(_) => (401, json!({"error": "unauthorized"})),
                None => (404, json!({"error": "not_found"})),
            };
            respond(&mut stream, status, &[], body.to_string().as_bytes());
        }
    });
    endpoint
}

/// A server bound to a daemon session whose agent is the recorded payer and
/// whose scopes serve exactly the three web tools.
fn bound_server(root: &Path) -> Server {
    let tenant = TenantId::new("tenant-a").unwrap_or_else(|error| panic!("tenant: {error}"));
    let capability = Capability::new(
        CapabilityId([9; 32]),
        tenant.clone(),
        CapabilityDimensions {
            activity_types: BTreeSet::from([7]),
            counterparties: BTreeSet::from([[2; 32]]),
            assets: BTreeSet::from([[3; 32]]),
            amount_ceiling: 100,
            rate_ceiling: RateCeiling {
                maximum_uses: 2,
                window_sequences: 10,
            },
            purposes: BTreeSet::from(["service-payment".to_owned()]),
            expiry_sequence: 200,
        },
    )
    .unwrap_or_else(|error| panic!("capability: {error:?}"));
    let mut store =
        Store::open(root.join("store")).unwrap_or_else(|error| panic!("store: {error}"));
    capability
        .persist(&mut store)
        .unwrap_or_else(|error| panic!("capability persist: {error:?}"));
    let agent = Did::new(payer_did().as_bytes()).unwrap_or_else(|error| panic!("DID: {error:?}"));
    let mut resolver = BoundaryIdentity(CoreIdentity {
        canonical_bytes: b"payer-identity".to_vec(),
        head_sequence: 10,
        revocation_sequence: 1,
        verification_level: VerificationLevel::STATE_PROVEN,
        frozen: false,
        authorities: vec![ProtocolAuthority::CapabilityGrant(capability.id.0)],
    });
    let identity = register(&mut store, tenant.clone(), agent.clone(), &mut resolver)
        .unwrap_or_else(|error| panic!("identity: {error:?}"));
    let mut sessions = SessionRegistry::default();
    let token = open(
        &mut store,
        &mut sessions,
        &identity,
        OpenRequest {
            session_id: SessionId([7; 32]),
            token_id: [8; 32],
            tenant,
            agent,
            authority: ProtocolAuthority::CapabilityGrant(capability.id.0),
            permitted_activity_types: BTreeSet::from([7]),
            scopes: BTreeSet::from([
                "write".to_owned(),
                "write:web:search".to_owned(),
                "write:web:fetch".to_owned(),
                "write:web:content".to_owned(),
            ]),
            expiry_sequence: 150,
            opening_client: "mcp".to_owned(),
            policy_version: "policy-v1".to_owned(),
        },
        OBSERVED_SEQUENCE,
    )
    .unwrap_or_else(|error| panic!("session: {error:?}"));
    let budgets = BudgetLimiter::new(vec![LimitConfig {
        id: LimitId([9; 16]),
        name: "mcp-limit".to_owned(),
        scope: LimitScope::Tenant([1; 32]),
        ceiling: 1_000,
        consumed: 0,
    }])
    .unwrap_or_else(|error| panic!("limiter: {error:?}"));
    let control = SessionControl::new(
        Arc::new(Mutex::new(store)),
        sessions,
        Arc::new(PreparationLifecycle::default()),
        Arc::new(budgets),
    );
    Server::bind(
        control,
        token.credential(),
        capability.id,
        OBSERVED_SEQUENCE,
        root,
    )
    .unwrap_or_else(|error| panic!("bind: {error:?}"))
}

fn web_session(
    replay: &Replay,
    approvals: &Arc<ApprovalRegistry>,
    threshold: u128,
    root: &Path,
) -> Session<WebBoundary<ProgramReads>> {
    let server = bound_server(root);
    let surface = AgentSurface::new(
        &agent_daemon(),
        AGENT_BEARER.to_owned(),
        &"55".repeat(32),
        Duration::from_secs(5),
    )
    .unwrap_or_else(|error| panic!("agent surface: {error:?}"));
    let route = WebRoute::new(
        config(replay, sequencer_key()),
        Arc::clone(approvals),
        ApprovalPolicy {
            amount_threshold: threshold,
        },
        Box::new(RecordedPayer::new()),
    );
    let boundary = server
        .route_web(ProgramReads::new(surface), route)
        .unwrap_or_else(|error| panic!("route: {error:?}"));
    Session::new(Bound::Full(Box::new(server)), boundary)
}

fn rpc(session: &mut Session<WebBoundary<ProgramReads>>, request: &Value) -> Value {
    session
        .handle(&request.to_string())
        .unwrap_or_else(|| panic!("no answer to {request}"))
}

fn call(session: &mut Session<WebBoundary<ProgramReads>>, name: &str, arguments: &Value) -> Value {
    let answer = rpc(
        session,
        &json!({
            "jsonrpc": "2.0",
            "id": 2,
            "method": "tools/call",
            "params": {"name": name, "arguments": arguments},
        }),
    );
    answer
        .get("result")
        .cloned()
        .unwrap_or_else(|| panic!("tools/call {name}: {answer}"))
}

fn search_arguments() -> Value {
    json!({
        "query": "paxeer",
        "currency": "SID",
        "scheme": "metered",
        "idempotency_key": hex(&metered_key()),
    })
}

fn paid(result: &Value, tool: &str) -> Value {
    assert_eq!(result["isError"], false, "{tool}: {result}");
    let structured = &result["structuredContent"];
    assert_eq!(structured["tool"], tool);
    let evidence = structured["result"].clone();
    assert_eq!(evidence["_meta"]["layerx/output"], "untrusted", "{tool}");
    assert_eq!(evidence["untrusted"], true);
    assert_eq!(evidence["tool"], tool);
    assert_eq!(
        evidence["settlement"]["verificationLevel"],
        "sequencer-signed"
    );
    let rendered = result["content"][0]["text"]
        .as_str()
        .unwrap_or_else(|| panic!("{tool} text content"));
    assert!(
        rendered.contains("\"layerx/output\": \"untrusted\""),
        "{tool}"
    );
    evidence
}

fn refusal(result: &Value) -> String {
    assert_eq!(result["isError"], true, "{result}");
    result["structuredContent"]["refusal"]
        .as_str()
        .unwrap_or_else(|| panic!("refusal text: {result}"))
        .to_owned()
}

fn assert_web_listing(session: &mut Session<WebBoundary<ProgramReads>>) {
    let listed = rpc(
        session,
        &json!({"jsonrpc": "2.0", "id": 1, "method": "tools/list"}),
    );
    let tools = listed["result"]["tools"]
        .as_array()
        .unwrap_or_else(|| panic!("tools/list: {listed}"));
    let names: Vec<&str> = tools
        .iter()
        .filter_map(|tool| tool["name"].as_str())
        .collect();
    assert_eq!(names, ["web.search", "web.fetch", "web.content"]);
    for tool in tools {
        assert_eq!(tool["_meta"]["layerx/output"], "untrusted");
        assert_eq!(tool["annotations"]["readOnlyHint"], false);
        assert_eq!(tool["inputSchema"]["additionalProperties"], false);
    }
}

fn assert_unpaid_refusals(session: &mut Session<WebBoundary<ProgramReads>>, replay: &Replay) {
    let unserved = rpc(
        session,
        &json!({
            "jsonrpc": "2.0",
            "id": 3,
            "method": "tools/call",
            "params": {"name": "wallet.send", "arguments": {}},
        }),
    );
    assert_eq!(unserved["error"]["code"], -32602);
    let local = call(
        session,
        "web.fetch",
        &json!({
            "url": "file:///etc/passwd",
            "currency": "USDC",
            "scheme": "exact",
            "idempotency_key": "5a".repeat(32),
        }),
    );
    assert!(refusal(&local).contains("url"));
    assert_eq!(local["structuredContent"]["stage"], "arguments");
    assert_eq!(replay.signatures(), 0);
}

#[test]
fn a_bound_server_lists_and_pays_the_web_tools_through_tools_call() {
    let (fetch, fetch_digest) = fetch_answer(ROUTED_FETCH_URL, false);
    let vector = &vectors()[2];
    let content_digest_hex = text(vector, "/digest").to_owned();
    let canonical = reference_canonical(
        1,
        text(vector, "/payload").as_bytes(),
        text(vector, "/media_type"),
        text(vector, "/text"),
    );
    let replay = Replay::start(Script {
        served: vec![
            (
                "/fetch?url=https%3A%2F%2Fpaxeer.app%2Findex.html".to_owned(),
                fetch,
            ),
            (format!("/content/{content_digest_hex}"), canonical),
        ],
        ..Script::default()
    });
    let approvals = Arc::new(ApprovalRegistry::default());
    let root = directory("routed");
    let mut session = web_session(&replay, &approvals, u128::MAX, &root);

    assert_web_listing(&mut session);
    assert_unpaid_refusals(&mut session, &replay);

    let search = paid(
        &call(&mut session, "web.search", &search_arguments()),
        "web.search",
    );
    assert_eq!(
        search["settlement"]["receiptDigest"],
        "54c4a7b999f9e107c0dd700f98dd39de2987a060a5e4b5c6e1bc324fd71e0fa5"
    );
    assert_eq!(search["settlement"]["amount"], "3114");
    assert_eq!(search["content"]["results"], json!([]));
    assert_eq!(replay.signatures(), 1);

    let fetched = paid(
        &call(
            &mut session,
            "web.fetch",
            &json!({
                "url": ROUTED_FETCH_URL,
                "currency": "USDC",
                "scheme": "exact",
                "idempotency_key": "5a".repeat(32),
            }),
        ),
        "web.fetch",
    );
    assert_eq!(
        fetched["settlement"]["receiptDigest"],
        "5088d0659995e37111051d23861b813c29d221021f5b6043831e9f191ca0fc36"
    );
    assert_eq!(fetched["content"]["url"], ROUTED_FETCH_URL);
    assert_eq!(fetched["content"]["digest"], hex(&fetch_digest));
    assert_eq!(fetched["content"]["text"], text(&vectors()[1], "/text"));
    assert_eq!(replay.signatures(), 2);

    let stored = paid(
        &call(
            &mut session,
            "web.content",
            &json!({
                "digest": content_digest_hex,
                "currency": "USDC",
                "scheme": "exact",
                "idempotency_key": "5b".repeat(32),
            }),
        ),
        "web.content",
    );
    assert_eq!(stored["content"]["digest"], content_digest_hex);
    assert_eq!(stored["content"]["media_type"], "application/json");
    assert_eq!(stored["content"]["text"], text(vector, "/text"));
    assert_eq!(replay.signatures(), 3);

    for key in [metered_key(), [0x5a; 32], [0x5b; 32]] {
        assert!(approvals
            .ticket(key)
            .unwrap_or_else(|error| panic!("ticket: {error:?}"))
            .is_none());
    }
    assert_eq!(session.bound().audit_entries(), 6);
    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn a_held_web_spend_through_tools_call_is_never_paid_until_approved() {
    let replay = Replay::start(Script::default());
    let approvals = Arc::new(ApprovalRegistry::default());
    let root = directory("held");
    let mut session = web_session(&replay, &approvals, 0, &root);

    let held = call(&mut session, "web.search", &search_arguments());
    let detail = refusal(&held);
    assert!(
        detail.contains(&format!(
            "held for approval under hold {}",
            hex(&metered_key())
        )),
        "{detail}"
    );
    assert_eq!(held["structuredContent"]["stage"], "daemon");
    assert_eq!(held["structuredContent"]["state"], "refused");
    assert_eq!(replay.signatures(), 0);
    let ticket = approvals
        .ticket(metered_key())
        .unwrap_or_else(|error| panic!("ticket: {error:?}"))
        .unwrap_or_else(|| panic!("the spend was not held"));
    assert_eq!(ticket.state, ApprovalState::AwaitingApproval);
    assert_eq!(ticket.disclosure.amounts.values()[0].amount.0, 3114);
    assert_eq!(ticket.disclosure.actor.as_str(), payer_did());

    let pending = call(&mut session, "web.search", &search_arguments());
    assert!(refusal(&pending).contains("still awaiting approval"));
    assert_eq!(replay.signatures(), 0);

    let approver = ApproverId::new("operator").unwrap_or_else(|error| panic!("{error:?}"));
    approve(
        &approvals,
        ticket.hold_id,
        approver,
        &ticket.disclosure,
        OBSERVED_SEQUENCE + 1,
    )
    .unwrap_or_else(|error| panic!("approve: {error:?}"));
    let released = paid(
        &call(&mut session, "web.search", &search_arguments()),
        "web.search",
    );
    assert_eq!(released["settlement"]["amount"], "3114");
    assert_eq!(replay.signatures(), 1);
    let _ = std::fs::remove_dir_all(root);
}
