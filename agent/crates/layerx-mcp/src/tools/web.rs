//! The web tool: search, fetch and content by digest from one configured
//! x-websearch sidecar, paid per request over 402LXP.
//!
//! Every search and fetch asks the sidecar for its offers, keeps the one offer
//! in the chosen asset and scheme, passes the spend through the approval
//! boundary, builds `PAYMENT-SIGNATURE` through the x402 [`Buyer`], and
//! releases the resource only after the `PAYMENT-RESPONSE` receipt verifies
//! under the configured sequencer key and pays exactly the offer from this
//! payer. PAX, the kernel's native coin, is paid from and into the main
//! accounts `agent:<did>:main`; SID, USDC and USDL from and into the per-asset
//! accounts `agent:<did>:asset:<id>`. Content by digest is served unpaid and
//! is never paid for. A fetch or content response whose recomputed content
//! digest differs from the one it claims or was asked for is refused.
//! Everything the sidecar returns is external content and is carried in
//! `opaque_` fields.

use std::fmt::Write as _;
use std::io::{Read as _, Write as _};
use std::net::{TcpStream, ToSocketAddrs as _};
use std::time::Duration;

use base64::engine::general_purpose::STANDARD;
use base64::Engine as _;
use layerx_agent_api::identity::{ActivityType, AgentDid, Asset, AuthorityRef, ExplicitSet};
use layerx_agent_api::prepare::{
    CanonicalBytes, DisclosedAmount, Disclosure, IdempotencyRef, PreparationRef, Prepared,
    SigningPreimage,
};
use layerx_agent_api::{Amount, TimestampSeconds};
use layerx_agentd::policy::approval::{
    ApprovalContext, ApprovalRegistry, ApprovalState, ApprovalTicket,
};
use layerx_crypto::payments::Payment;
use layerx_interop_gateway::trace::TraceId;
use layerx_proof::merkle::leaf_hash;
use layerx_proof::receipt::{verify, verify_sequencer_signature, AuthorizedBatch};
use layerx_types::payload::ModuleId;
use layerx_x402::buyer::{
    BuiltPayment, Buyer, BuyerPaymentPlane, PaymentBuildRequest, SupportedKind,
};
use layerx_x402::model::{
    account_identifiers, PaymentRequired, PaymentRequirements, SettlementResponse, X402Error,
};
use serde_json::{json, Value};
use sha2::{Digest as _, Sha256};
use sha3::Keccak256;

use crate::approval::{self, ApprovalError, ApprovalPolicy, Requirement};
use crate::catalogue::{self, ArgumentError};

pub const PAYMENT_REQUIRED: &str = "PAYMENT-REQUIRED";
pub const PAYMENT_SIGNATURE: &str = "PAYMENT-SIGNATURE";
pub const PAYMENT_RESPONSE: &str = "PAYMENT-RESPONSE";
pub const PAYER_DID: &str = "LAYERX-PAYER-DID";

/// The ASCII domain every canonical content encoding starts with.
pub const CONTENT_DOMAIN: &[u8] = b"PAXEERX_WEB_CONTENT_V1";
/// Sequences an approval hold for one web spend stays open.
pub const APPROVAL_WINDOW: u64 = 600;
/// The most results one search returns.
pub const MAX_SEARCH_RESULTS: usize = 10;
/// The largest sidecar response the tool reads.
pub const MAX_RESPONSE_BYTES: usize = 9_437_184;

const PURPOSE_DOMAIN: &[u8] = b"LayerX/x-websearch/v1/purpose\0";
const SPEND_DOMAIN: &str = "LayerX/mcp/web/spend/v1";
const TRACE_DOMAIN: &[u8] = b"LayerX/mcp/web/trace/v1\0";
const SEQUENCER_SIGNED: &str = "sequencer-signed";
const MAX_RETRY_DELAY: Duration = Duration::from_secs(5);
const MAX_TIMEOUT: Duration = Duration::from_secs(60);
const MAX_PENDING_ATTEMPTS: u8 = 10;
const MAX_DID_BYTES: usize = 255;
const ASSET_MODULE: u16 = 1;
const SEND_OPERATION: u8 = 5;
const GRANT_OPERATION: u16 = 7;
const GRANT_BYTES: usize = 346;

/// The four assets a sidecar accepts.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Currency {
    Sid,
    Pax,
    Usdc,
    Usdl,
}

impl Currency {
    #[must_use]
    pub const fn code(self) -> &'static str {
        match self {
            Self::Sid => "SID",
            Self::Pax => "PAX",
            Self::Usdc => "USDC",
            Self::Usdl => "USDL",
        }
    }

    #[must_use]
    pub fn parse(code: &str) -> Option<Self> {
        match code {
            "SID" => Some(Self::Sid),
            "PAX" => Some(Self::Pax),
            "USDC" => Some(Self::Usdc),
            "USDL" => Some(Self::Usdl),
            _ => None,
        }
    }
}

/// How one request is paid: a draw against the payer's grant, or a receipt
/// of a payment the payer already made.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Scheme {
    Metered,
    Exact,
}

impl Scheme {
    #[must_use]
    pub const fn code(self) -> &'static str {
        match self {
            Self::Metered => "metered",
            Self::Exact => "exact",
        }
    }

    #[must_use]
    pub fn parse(code: &str) -> Option<Self> {
        match code {
            "metered" => Some(Self::Metered),
            "exact" => Some(Self::Exact),
            _ => None,
        }
    }

    const fn activity_type(self) -> u16 {
        match self {
            Self::Metered => GRANT_OPERATION,
            Self::Exact => 5,
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct WebSearch {
    pub query: String,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct WebFetch {
    pub url: String,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct WebContent {
    pub digest: [u8; 32],
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum WebOperation {
    Search(WebSearch),
    Fetch(WebFetch),
    Content(WebContent),
}

impl WebOperation {
    #[must_use]
    pub const fn tool_name(&self) -> &'static str {
        match self {
            Self::Search(_) => "web.search",
            Self::Fetch(_) => "web.fetch",
            Self::Content(_) => "web.content",
        }
    }

    /// The request target on the sidecar.
    #[must_use]
    pub fn target(&self) -> String {
        match self {
            Self::Search(search) => format!("/search?q={}", percent_encode(&search.query)),
            Self::Fetch(fetch) => format!("/fetch?url={}", percent_encode(&fetch.url)),
            Self::Content(content) => format!("/content/{}", hex(&content.digest)),
        }
    }
}

/// One validated web tool call. The currency, scheme and idempotency key
/// bind the payment of a search or a fetch; content by digest is unpaid.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct WebCall {
    pub operation: WebOperation,
    pub currency: Currency,
    pub scheme: Scheme,
    pub idempotency_key: [u8; 32],
}

impl WebCall {
    /// Validates MCP call arguments against the catalogue schema of one web
    /// tool and decodes them.
    ///
    /// # Errors
    /// Refuses a tool outside the web tools and every argument the catalogue
    /// refuses, and a zero idempotency key.
    pub fn from_arguments(name: &str, arguments: &Value) -> Result<Self, ArgumentError> {
        if !catalogue::WEB_TOOLS.iter().any(|tool| tool.name == name) {
            return Err(ArgumentError::Unknown(name.to_owned()));
        }
        catalogue::validate(name, arguments)?;
        let text = |field: &'static str| {
            arguments
                .get(field)
                .and_then(Value::as_str)
                .ok_or(ArgumentError::Missing(field))
        };
        let operation = match name {
            "web.search" => WebOperation::Search(WebSearch {
                query: text("query")?.to_owned(),
            }),
            "web.fetch" => WebOperation::Fetch(WebFetch {
                url: text("url")?.to_owned(),
            }),
            _ => WebOperation::Content(WebContent {
                digest: unhex32(text("digest")?).ok_or(ArgumentError::Malformed("digest"))?,
            }),
        };
        let currency =
            Currency::parse(text("currency")?).ok_or(ArgumentError::Malformed("currency"))?;
        let scheme = Scheme::parse(text("scheme")?).ok_or(ArgumentError::Malformed("scheme"))?;
        let idempotency_key = unhex32(text("idempotency_key")?)
            .filter(|key| *key != [0; 32])
            .ok_or(ArgumentError::Malformed("idempotency_key"))?;
        Ok(Self {
            operation,
            currency,
            scheme,
            idempotency_key,
        })
    }
}

/// The plain HTTP endpoint of one sidecar, `http://host[:port]`.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SidecarEndpoint {
    host: String,
    port: u16,
    authority: String,
}

impl SidecarEndpoint {
    /// # Errors
    /// Refuses anything but `http://` followed by a host and an optional port
    /// with no path, query or credentials.
    pub fn parse(text: &str) -> Result<Self, WebToolError> {
        let refuse = || WebToolError::Configuration("endpoint");
        let authority = text.strip_prefix("http://").ok_or_else(refuse)?;
        let authority = authority.strip_suffix('/').unwrap_or(authority);
        let (host, port) = if let Some(rest) = authority.strip_prefix('[') {
            let (host, tail) = rest.split_once(']').ok_or_else(refuse)?;
            (host, tail.strip_prefix(':'))
        } else {
            match authority.split_once(':') {
                Some((host, port)) => (host, Some(port)),
                None => (authority, None),
            }
        };
        let port = match port {
            Some(port) if port.bytes().all(|byte| byte.is_ascii_digit()) => {
                port.parse::<u16>().map_err(|_| refuse())?
            }
            Some(_) => return Err(refuse()),
            None => 80,
        };
        if host.is_empty()
            || port == 0
            || !host
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || b".-:".contains(&byte))
        {
            return Err(refuse());
        }
        Ok(Self {
            host: host.to_owned(),
            port,
            authority: authority.to_owned(),
        })
    }
}

/// Everything the tool needs besides the call: the sidecar, the payer, the
/// network the offers must settle on and the sequencer trust.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct WebConfig {
    endpoint: SidecarEndpoint,
    payer_did: String,
    network: String,
    sequencer_public_key: [u8; 32],
    timeout: Duration,
    pending_attempts: u8,
}

impl WebConfig {
    /// # Errors
    /// Refuses a malformed endpoint, payer DID or network, a zero sequencer
    /// key, a timeout outside one to sixty seconds and a pending retry count
    /// outside one to ten, naming the field.
    pub fn new(
        endpoint: &str,
        payer_did: &str,
        network: &str,
        sequencer_public_key: [u8; 32],
        timeout: Duration,
        pending_attempts: u8,
    ) -> Result<Self, WebToolError> {
        let endpoint = SidecarEndpoint::parse(endpoint)?;
        if !valid_payer(payer_did) {
            return Err(WebToolError::Configuration("payer_did"));
        }
        if !network.strip_prefix("layerx:").is_some_and(|rest| {
            !rest.is_empty() && rest.bytes().all(|byte| byte.is_ascii_alphanumeric())
        }) {
            return Err(WebToolError::Configuration("network"));
        }
        if sequencer_public_key == [0; 32] {
            return Err(WebToolError::Configuration("sequencer_public_key"));
        }
        if timeout.is_zero() || timeout > MAX_TIMEOUT {
            return Err(WebToolError::Configuration("timeout"));
        }
        if pending_attempts == 0 || pending_attempts > MAX_PENDING_ATTEMPTS {
            return Err(WebToolError::Configuration("pending_attempts"));
        }
        Ok(Self {
            endpoint,
            payer_did: payer_did.to_owned(),
            network: network.to_owned(),
            sequencer_public_key,
            timeout,
            pending_attempts,
        })
    }

    #[must_use]
    pub fn payer_did(&self) -> &str {
        &self.payer_did
    }
}

/// The approval boundary one web spend passes through.
pub struct WebApproval<'a> {
    pub registry: &'a ApprovalRegistry,
    pub policy: ApprovalPolicy,
    /// The audit context; its request id is replaced by the call's key.
    pub context: ApprovalContext,
    pub current_sequence: u64,
}

/// What the payer's grant must bind for a metered offer.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct GrantTerms {
    pub payer_did: String,
    pub payer_account: [u8; 32],
    pub recipient: [u8; 32],
    pub asset: [u8; 32],
    pub amount: u128,
    pub purpose_hash: [u8; 32],
    pub idempotency_key: [u8; 32],
}

/// What the payer's payment must settle for an exact offer.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ExactTerms {
    pub payer_did: String,
    pub recipient: [u8; 32],
    pub recipient_account: String,
    pub asset: [u8; 32],
    pub amount: u128,
    pub idempotency_key: [u8; 32],
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum WebPayerError {
    Refused,
    Unavailable,
}

/// The payer's signing boundary. The tool never holds key material: it asks
/// the payer for a signed canonical grant or for the canonical receipt of a
/// payment, and checks what it receives against the approved offer.
pub trait WebPayer {
    /// The payer-signed 346-byte canonical grant for a metered offer.
    ///
    /// # Errors
    /// Returns the payer's refusal or unavailability.
    fn grant(&mut self, terms: &GrantTerms) -> Result<Vec<u8>, WebPayerError>;

    /// The sequencer-signed canonical receipt of the payer's send for an
    /// exact offer.
    ///
    /// # Errors
    /// Returns the payer's refusal or unavailability.
    fn pay(&mut self, terms: &ExactTerms) -> Result<Vec<u8>, WebPayerError>;
}

#[derive(Debug)]
pub enum WebToolError {
    Configuration(&'static str),
    Transport,
    Protocol(&'static str),
    Status(u16),
    OfferUnavailable,
    OfferMismatch(&'static str),
    ApprovalRequired(Box<ApprovalTicket>),
    ApprovalPending(Box<ApprovalTicket>),
    ApprovalRefused(ApprovalState),
    ApprovalChanged,
    Approval(ApprovalError),
    Payer(WebPayerError),
    PaymentArtifact(&'static str),
    Payment(X402Error),
    PaymentRejected(String),
    SettlementRefused(Option<String>),
    SettlementPending,
    SettlementUnverified,
    SettlementMismatch,
    DigestMismatch,
    ContentMalformed(&'static str),
}

/// The verified settlement of one request.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct WebSettlement {
    pub scheme: Scheme,
    pub currency: Currency,
    pub network: String,
    pub asset: [u8; 32],
    pub pay_to: [u8; 32],
    pub amount: u128,
    pub payer: [u8; 32],
    pub receipt_digest: [u8; 32],
    pub transaction: String,
}

/// One search result; every field is external content.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct UntrustedHit {
    pub url: String,
    pub title: String,
    pub snippet: String,
}

/// The released resource. Every text in it is external content.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum WebResult {
    Search {
        opaque_results: Vec<UntrustedHit>,
    },
    Fetch {
        url: String,
        opaque_final_url: String,
        media_type: String,
        digest: [u8; 32],
        length: u64,
        opaque_text: String,
    },
    Content {
        digest: [u8; 32],
        kind: u8,
        opaque_payload: Vec<u8>,
        media_type: String,
        opaque_text: String,
    },
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct WebOutcome {
    pub tool: &'static str,
    /// The verified settlement of a paid search or fetch; `None` for the
    /// unpaid content by digest.
    pub settlement: Option<WebSettlement>,
    pub result: WebResult,
}

impl WebOutcome {
    /// The tool result: the settlement evidence of a paid call and the
    /// external content, marked untrusted.
    #[must_use]
    pub fn evidence(&self) -> Value {
        let content = match &self.result {
            WebResult::Search { opaque_results } => json!({
                "results": opaque_results.iter().map(|hit| json!({
                    "url": hit.url,
                    "title": hit.title,
                    "snippet": hit.snippet,
                })).collect::<Vec<_>>(),
            }),
            WebResult::Fetch {
                url,
                opaque_final_url,
                media_type,
                digest,
                length,
                opaque_text,
            } => json!({
                "url": url,
                "final_url": opaque_final_url,
                "media_type": media_type,
                "digest": hex(digest),
                "length": length,
                "text": opaque_text,
            }),
            WebResult::Content {
                digest,
                kind,
                opaque_payload,
                media_type,
                opaque_text,
            } => json!({
                "digest": hex(digest),
                "kind": kind,
                "payload": String::from_utf8_lossy(opaque_payload),
                "media_type": media_type,
                "text": opaque_text,
            }),
        };
        let mut evidence = json!({
            "tool": self.tool,
            "untrusted": true,
            "content": content,
        });
        if let (Some(settlement), Some(fields)) = (&self.settlement, evidence.as_object_mut()) {
            fields.insert(
                "settlement".to_owned(),
                json!({
                    "scheme": settlement.scheme.code(),
                    "currency": settlement.currency.code(),
                    "network": settlement.network,
                    "asset": hex(&settlement.asset),
                    "payTo": hex(&settlement.pay_to),
                    "amount": settlement.amount.to_string(),
                    "payer": hex(&settlement.payer),
                    "receiptDigest": hex(&settlement.receipt_digest),
                    "transaction": settlement.transaction,
                    "verificationLevel": SEQUENCER_SIGNED,
                }),
            );
        }
        evidence
    }
}

/// Performs one web call against the configured sidecar: a paid search or
/// fetch, or the unpaid content by digest.
///
/// # Errors
/// Refuses an unavailable or inconsistent offer, a spend the approval
/// boundary holds, rejects or has not approved, a payer artifact that does
/// not bind the offer, a settlement that is refused, pending past the retry
/// bound, unverified under the sequencer key or not paying the offer from
/// this payer, a payment challenge on the content path, and content whose
/// digest does not match.
pub fn web_tool(
    config: &WebConfig,
    call: &WebCall,
    approval: &WebApproval<'_>,
    payer: &mut dyn WebPayer,
) -> Result<WebOutcome, WebToolError> {
    if let WebOperation::Content(content) = &call.operation {
        return stored_content(config, content);
    }
    let target = call.operation.target();
    let mut headers = Vec::new();
    if call.scheme == Scheme::Metered {
        headers.push((PAYER_DID, config.payer_did.clone()));
    }
    let challenge = request(config, &target, &headers)?;
    if challenge.status != 402 {
        return Err(WebToolError::Status(challenge.status));
    }
    let offered = challenge
        .header(PAYMENT_REQUIRED)
        .ok_or(WebToolError::Protocol("payment_required_missing"))?;
    let (required_header, offer) = select_offer(config, call, offered)?;
    let trace = TraceId::mint(trace_entropy(&call.idempotency_key, &target));
    let buyer = Buyer::new(vec![SupportedKind {
        scheme: call.scheme.code().to_owned(),
        network: config.network.clone(),
    }])
    .map_err(WebToolError::Payment)?;
    let mut plane = WebPlane {
        config,
        call,
        approval,
        payer,
        target: &target,
        refusal: None,
    };
    let built = buyer
        .build_payment(&required_header, call.idempotency_key, &mut plane, &trace)
        .map_err(|traced| {
            plane
                .refusal
                .take()
                .unwrap_or_else(|| WebToolError::Payment(traced.into_error()))
        })?;
    headers.push((PAYMENT_SIGNATURE, built.header.clone()));
    let reply = settle(config, &target, &headers)?;
    let response = reply
        .header(PAYMENT_RESPONSE)
        .ok_or(WebToolError::SettlementUnverified)?;
    let settlement = verify_settlement(config, call, &offer, &built, response, &trace)?;
    let result = verify_resource(&call.operation, &reply.body)?;
    Ok(WebOutcome {
        tool: call.operation.tool_name(),
        settlement: Some(settlement),
        result,
    })
}

/// Reads stored content by digest. The sidecar serves it unpaid, so a
/// payment challenge on this path is a protocol mismatch and is never paid,
/// and the body is released only when its digest is the one asked for.
fn stored_content(config: &WebConfig, content: &WebContent) -> Result<WebOutcome, WebToolError> {
    let operation = WebOperation::Content(*content);
    let reply = request(config, &operation.target(), &[])?;
    match reply.status {
        200 => Ok(WebOutcome {
            tool: operation.tool_name(),
            settlement: None,
            result: verify_content(&content.digest, &reply.body)?,
        }),
        402 => Err(WebToolError::Protocol("content_payment_required")),
        status => Err(WebToolError::Status(status)),
    }
}

fn settle(
    config: &WebConfig,
    target: &str,
    headers: &[(&str, String)],
) -> Result<HttpReply, WebToolError> {
    let mut attempts = 0_u8;
    loop {
        let reply = request(config, target, headers)?;
        match reply.status {
            200 => return Ok(reply),
            503 if reply.error_code().as_deref() == Some("payment_pending") => {
                attempts = attempts.saturating_add(1);
                if attempts >= config.pending_attempts {
                    return Err(WebToolError::SettlementPending);
                }
                std::thread::sleep(retry_delay(&reply));
            }
            402 => {
                if let Some(header) = reply.header(PAYMENT_RESPONSE) {
                    let response: SettlementResponse =
                        decode_header(header).ok_or(WebToolError::SettlementUnverified)?;
                    return Err(WebToolError::SettlementRefused(response.error_reason));
                }
                return Err(WebToolError::PaymentRejected(
                    reply.error_code().unwrap_or_default(),
                ));
            }
            status => return Err(WebToolError::Status(status)),
        }
    }
}

/// The facts of the one offer a call pays.
#[derive(Clone, Debug, Eq, PartialEq)]
struct OfferFacts {
    receiver_did: String,
    account: String,
    asset: [u8; 32],
    pay_to: [u8; 32],
    amount: u128,
    payer_accounts: [[u8; 32]; 2],
    metered_payer: Option<[u8; 32]>,
    purpose_hash: Option<[u8; 32]>,
}

fn select_offer(
    config: &WebConfig,
    call: &WebCall,
    header: &str,
) -> Result<(String, OfferFacts), WebToolError> {
    let mut required: PaymentRequired =
        decode_header(header).ok_or(WebToolError::Protocol("payment_required_malformed"))?;
    required.validate().map_err(WebToolError::Payment)?;
    required.accepts.retain(|offer| {
        offer.scheme == call.scheme.code()
            && offer.network == config.network
            && offer
                .extra
                .as_ref()
                .and_then(|extra| extra.pointer("/layerx/currency"))
                .and_then(Value::as_str)
                == Some(call.currency.code())
    });
    let [offer] = required.accepts.as_slice() else {
        return Err(WebToolError::OfferUnavailable);
    };
    let facts = offer_facts(config, call, offer)?;
    let encoded = serde_json::to_vec(&required)
        .map_err(|_| WebToolError::Protocol("payment_required_encoding"))?;
    Ok((STANDARD.encode(encoded), facts))
}

fn offer_facts(
    config: &WebConfig,
    call: &WebCall,
    offer: &PaymentRequirements,
) -> Result<OfferFacts, WebToolError> {
    let terms = offer.layerx_terms().map_err(WebToolError::Payment)?;
    let (asset, pay_to) = offer.layerx_facts().map_err(WebToolError::Payment)?;
    if terms.currency != call.currency.code() {
        return Err(WebToolError::OfferMismatch("currency"));
    }
    let receiver_did = terms
        .account
        .strip_prefix("agent:")
        .and_then(|rest| rest.strip_suffix(&account_suffix(call.currency, &asset)))
        .filter(|did| valid_payer(did))
        .ok_or(WebToolError::OfferMismatch("account"))?
        .to_owned();
    let payer_accounts =
        account_identifiers(&wallet_account(&config.payer_did, call.currency, &asset))
            .map_err(WebToolError::Payment)?;
    let layerx = offer.extra.as_ref().and_then(|extra| extra.get("layerx"));
    let (metered_payer, purpose_hash) = match call.scheme {
        Scheme::Metered => {
            let payer = layerx
                .and_then(|terms| terms.get("payer"))
                .and_then(Value::as_str)
                .and_then(unhex32)
                .filter(|payer| payer_accounts.contains(payer))
                .ok_or(WebToolError::OfferMismatch("payer"))?;
            let purpose = layerx
                .and_then(|terms| terms.get("purposeHash"))
                .and_then(Value::as_str)
                .and_then(unhex32)
                .filter(|purpose| purpose_of(&receiver_did) == Some(*purpose))
                .ok_or(WebToolError::OfferMismatch("purpose"))?;
            (Some(payer), Some(purpose))
        }
        Scheme::Exact => (None, None),
    };
    Ok(OfferFacts {
        receiver_did,
        account: terms.account,
        asset,
        pay_to,
        amount: offer.amount.value(),
        payer_accounts,
        metered_payer,
        purpose_hash,
    })
}

/// The buyer's plane: approval first, then the payer's artifact, checked
/// against the offer before it becomes the scheme payload.
struct WebPlane<'a, 'p> {
    config: &'a WebConfig,
    call: &'a WebCall,
    approval: &'a WebApproval<'a>,
    payer: &'p mut dyn WebPayer,
    target: &'a str,
    refusal: Option<WebToolError>,
}

impl BuyerPaymentPlane for WebPlane<'_, '_> {
    fn construct(&mut self, request: PaymentBuildRequest) -> Result<Value, X402Error> {
        self.authorise(&request.requirements, request.idempotency_key)
            .map_err(|refusal| {
                self.refusal = Some(refusal);
                X402Error::InvalidPayload
            })
    }
}

impl WebPlane<'_, '_> {
    fn authorise(
        &mut self,
        offer: &PaymentRequirements,
        key: [u8; 32],
    ) -> Result<Value, WebToolError> {
        let facts = offer_facts(self.config, self.call, offer)?;
        self.approve(offer, &facts, key)?;
        match self.call.scheme {
            Scheme::Metered => {
                let terms = GrantTerms {
                    payer_did: self.config.payer_did.clone(),
                    payer_account: facts
                        .metered_payer
                        .ok_or(WebToolError::OfferMismatch("payer"))?,
                    recipient: facts.pay_to,
                    asset: facts.asset,
                    amount: facts.amount,
                    purpose_hash: facts
                        .purpose_hash
                        .ok_or(WebToolError::OfferMismatch("purpose"))?,
                    idempotency_key: key,
                };
                let grant = self.payer.grant(&terms).map_err(WebToolError::Payer)?;
                check_grant(&grant, &terms)?;
                Ok(json!({ "grant": hex(&grant), "idempotencyKey": hex(&key) }))
            }
            Scheme::Exact => {
                let terms = ExactTerms {
                    payer_did: self.config.payer_did.clone(),
                    recipient: facts.pay_to,
                    recipient_account: facts.account.clone(),
                    asset: facts.asset,
                    amount: facts.amount,
                    idempotency_key: key,
                };
                let receipt = self.payer.pay(&terms).map_err(WebToolError::Payer)?;
                let digest = check_receipt(self.config, &receipt, &facts)?;
                Ok(json!({
                    "receipt": STANDARD.encode(&receipt),
                    "receiptDigest": hex(&digest),
                    "verificationLevel": SEQUENCER_SIGNED,
                }))
            }
        }
    }

    fn approve(
        &self,
        offer: &PaymentRequirements,
        facts: &OfferFacts,
        key: [u8; 32],
    ) -> Result<(), WebToolError> {
        let boundary = self.approval;
        let mut context = boundary.context.clone();
        context.request_id = key;
        if context.agent.as_bytes() != self.config.payer_did.as_bytes() {
            return Err(WebToolError::Configuration("approval_agent"));
        }
        let prepared = spend_preparation(
            self.config,
            self.call,
            self.target,
            offer,
            facts,
            key,
            boundary.current_sequence,
        )?;
        let ticket = boundary
            .registry
            .ticket(key)
            .map_err(|error| WebToolError::Approval(ApprovalError::Daemon(error)))?;
        if let Some(ticket) = ticket {
            if ticket.disclosure.canonical_digest != prepared.disclosure.canonical_digest {
                return Err(WebToolError::ApprovalChanged);
            }
            return match ticket.state {
                ApprovalState::Approved
                    if ticket.expires_at_sequence > boundary.current_sequence =>
                {
                    Ok(())
                }
                ApprovalState::Approved => {
                    Err(WebToolError::ApprovalRefused(ApprovalState::Expired))
                }
                ApprovalState::AwaitingApproval => {
                    Err(WebToolError::ApprovalPending(Box::new(ticket)))
                }
                state => Err(WebToolError::ApprovalRefused(state)),
            };
        }
        match approval::require(
            boundary.registry,
            boundary.policy,
            context,
            prepared,
            boundary.current_sequence,
        )
        .map_err(WebToolError::Approval)?
        {
            Requirement::NotRequired { .. } => Ok(()),
            Requirement::Required(ticket) => Err(WebToolError::ApprovalRequired(ticket)),
        }
    }
}

/// The disclosure an approver sees for one web spend. Its canonical bytes
/// bind the tool, the resource, the whole offer and the idempotency key.
fn spend_preparation(
    config: &WebConfig,
    call: &WebCall,
    target: &str,
    offer: &PaymentRequirements,
    facts: &OfferFacts,
    key: [u8; 32],
    current_sequence: u64,
) -> Result<Prepared, WebToolError> {
    let invalid = |_| WebToolError::Configuration("approval_disclosure");
    let canonical = serde_json::to_vec(&json!({
        "domain": SPEND_DOMAIN,
        "tool": call.operation.tool_name(),
        "target": target,
        "offer": offer,
        "idempotencyKey": hex(&key),
    }))
    .map_err(|_| WebToolError::Configuration("approval_disclosure"))?;
    let digest: [u8; 32] = Sha256::digest(&canonical).into();
    let expiry = current_sequence
        .checked_add(APPROVAL_WINDOW)
        .ok_or(WebToolError::Configuration("approval_window"))?;
    let receiver = AgentDid::new(facts.receiver_did.clone()).map_err(invalid)?;
    Ok(Prepared {
        preparation_ref: PreparationRef::new(format!("web-{}", hex(&digest[..8])))
            .map_err(invalid)?,
        unsigned_canonical_bytes: CanonicalBytes::new(canonical).map_err(invalid)?,
        signing_preimage: SigningPreimage::new(digest.to_vec()).map_err(invalid)?,
        disclosure: Disclosure {
            canonical_digest: digest,
            activity_type: ActivityType(call.scheme.activity_type()),
            actor: AgentDid::new(config.payer_did.clone()).map_err(invalid)?,
            authority: AuthorityRef::new(format!("x402:{}:{}", call.scheme.code(), config.network))
                .map_err(invalid)?,
            counterparties: ExplicitSet::allow(vec![receiver.clone()]),
            amounts: ExplicitSet::allow(vec![DisclosedAmount {
                counterparty: receiver,
                amount: Amount(facts.amount),
            }]),
            asset: Asset::new(call.currency.code()).map_err(invalid)?,
            fee_limit: Amount(0),
            expiry: TimestampSeconds(expiry),
            idempotency_key: IdempotencyRef::new(hex(&key)).map_err(invalid)?,
        },
        expiry: TimestampSeconds(expiry),
    })
}

fn check_grant(bytes: &[u8], terms: &GrantTerms) -> Result<(), WebToolError> {
    let refuse = WebToolError::PaymentArtifact("grant");
    if bytes.len() != GRANT_BYTES {
        return Err(refuse);
    }
    let Ok(Payment::IssueGrant(grant)) = Payment::decode(
        ModuleId::Asset,
        GRANT_OPERATION,
        bytes,
        terms.payer_did.as_bytes(),
    ) else {
        return Err(refuse);
    };
    if grant.from != terms.payer_account
        || grant.recipient != terms.recipient
        || grant.asset != terms.asset
        || grant.purpose_hash != terms.purpose_hash
        || grant.recurring
        || grant.window_length != 0
        || grant.has_reference
        || grant.per_draw_maximum < terms.amount
        || grant.allowance < terms.amount
    {
        return Err(refuse);
    }
    Ok(())
}

/// Verifies the payer's exact receipt: sequencer-signed under the configured
/// key, a successful asset send from this payer to the offer's payee of the
/// offer's asset and amount. Returns its receipt digest.
fn check_receipt(
    config: &WebConfig,
    receipt: &[u8],
    facts: &OfferFacts,
) -> Result<[u8; 32], WebToolError> {
    let refuse = || WebToolError::PaymentArtifact("receipt");
    let decoded =
        verify_sequencer_signature(receipt, config.sequencer_public_key).map_err(|_| refuse())?;
    let protocol = decoded.protocol().ok_or_else(refuse)?;
    if protocol.module_id() != ASSET_MODULE
        || protocol.operation() != SEND_OPERATION
        || protocol.result_code() != 0
        || !facts.payer_accounts.contains(&protocol.from())
        || protocol.to() != facts.pay_to
        || protocol.asset() != facts.asset
        || protocol.amount() != facts.amount
    {
        return Err(refuse());
    }
    let batch = authorised_batch(config, receipt).ok_or_else(refuse)?;
    let verified = verify(receipt, &batch).map_err(|_| refuse())?;
    leaf_hash(verified.canonical_bytes()).map_err(|_| refuse())
}

/// The batch a receipt names, bound to the configured sequencer key, once
/// the receipt's sequencer signature verifies under that key.
fn authorised_batch(config: &WebConfig, receipt: &[u8]) -> Option<AuthorizedBatch> {
    let decoded = verify_sequencer_signature(receipt, config.sequencer_public_key).ok()?;
    let protocol = decoded.protocol()?;
    Some(AuthorizedBatch::new(
        protocol.batch_id(),
        protocol.asset(),
        protocol.previous_state_root(),
        protocol.resulting_state_root(),
        config.sequencer_public_key,
    ))
}

fn verify_settlement(
    config: &WebConfig,
    call: &WebCall,
    offer: &OfferFacts,
    built: &BuiltPayment,
    header: &str,
    trace: &TraceId,
) -> Result<WebSettlement, WebToolError> {
    let response: SettlementResponse =
        decode_header(header).ok_or(WebToolError::SettlementUnverified)?;
    if !response.success {
        return Err(WebToolError::SettlementRefused(response.error_reason));
    }
    let receipt = response
        .extensions
        .get("layerx")
        .and_then(|layerx| layerx.get("receipt"))
        .and_then(Value::as_str)
        .and_then(|receipt| STANDARD.decode(receipt).ok())
        .ok_or(WebToolError::SettlementUnverified)?;
    let batch = authorised_batch(config, &receipt).ok_or(WebToolError::SettlementUnverified)?;
    let captured = Buyer::capture_settlement(header, built, &batch, trace)
        .map_err(|_| WebToolError::SettlementMismatch)?;
    let payer = captured
        .response
        .payer
        .as_deref()
        .and_then(unhex32)
        .ok_or(WebToolError::SettlementMismatch)?;
    if !offer.payer_accounts.contains(&payer)
        || offer.metered_payer.is_some_and(|metered| metered != payer)
    {
        return Err(WebToolError::SettlementMismatch);
    }
    Ok(WebSettlement {
        scheme: call.scheme,
        currency: call.currency,
        network: captured.response.network.clone(),
        asset: offer.asset,
        pay_to: offer.pay_to,
        amount: offer.amount,
        payer,
        receipt_digest: captured.receipt_digest,
        transaction: captured.response.transaction,
    })
}

fn verify_resource(operation: &WebOperation, body: &[u8]) -> Result<WebResult, WebToolError> {
    match operation {
        WebOperation::Search(_) => verify_search(body),
        WebOperation::Fetch(fetch) => verify_fetch(&fetch.url, body),
        WebOperation::Content(content) => verify_content(&content.digest, body),
    }
}

/// Reads a search answer: at most ten results of url, title and snippet.
///
/// # Errors
/// Refuses a body that is not that shape.
pub fn verify_search(body: &[u8]) -> Result<WebResult, WebToolError> {
    let refuse = || WebToolError::ContentMalformed("search");
    let value: Value = serde_json::from_slice(body).map_err(|_| refuse())?;
    let results = value
        .get("results")
        .and_then(Value::as_array)
        .filter(|results| results.len() <= MAX_SEARCH_RESULTS)
        .ok_or_else(refuse)?;
    let mut hits = Vec::with_capacity(results.len());
    for result in results {
        let field = |name: &str| result.get(name).and_then(Value::as_str).map(str::to_owned);
        let (Some(url), Some(title), Some(snippet)) =
            (field("url"), field("title"), field("snippet"))
        else {
            return Err(refuse());
        };
        hits.push(UntrustedHit {
            url,
            title,
            snippet,
        });
    }
    Ok(WebResult::Search {
        opaque_results: hits,
    })
}

/// Reads a fetch answer and recomputes its content digest from the
/// requested URL, the media type and the text.
///
/// # Errors
/// Refuses a malformed answer, an answer for another URL, a length that is
/// not the text's, and a digest that does not match the recomputed one.
pub fn verify_fetch(url: &str, body: &[u8]) -> Result<WebResult, WebToolError> {
    let refuse = || WebToolError::ContentMalformed("fetch");
    let value: Value = serde_json::from_slice(body).map_err(|_| refuse())?;
    let text = |name: &str| value.get(name).and_then(Value::as_str);
    let (Some(answered), Some(final_url), Some(media_type), Some(digest), Some(opaque_text)) = (
        text("url"),
        text("final_url"),
        text("media_type"),
        text("digest").and_then(unhex32),
        text("text"),
    ) else {
        return Err(refuse());
    };
    if answered != url {
        return Err(WebToolError::ContentMalformed("fetch_url"));
    }
    let length = u64::try_from(opaque_text.len()).map_err(|_| refuse())?;
    if value.get("length").and_then(Value::as_u64) != Some(length) {
        return Err(WebToolError::ContentMalformed("fetch_length"));
    }
    let canonical = canonical_bytes(1, url.as_bytes(), media_type, opaque_text)?;
    if content_digest(&canonical) != digest {
        return Err(WebToolError::DigestMismatch);
    }
    Ok(WebResult::Fetch {
        url: url.to_owned(),
        opaque_final_url: final_url.to_owned(),
        media_type: media_essence(media_type).ok_or_else(refuse)?,
        digest,
        length,
        opaque_text: opaque_text.to_owned(),
    })
}

/// Checks stored content against the digest it was requested by and
/// decodes its canonical fields.
///
/// # Errors
/// Refuses bytes whose keccak256 is not the digest and bytes that are not
/// exactly one canonical encoding.
pub fn verify_content(digest: &[u8; 32], bytes: &[u8]) -> Result<WebResult, WebToolError> {
    if content_digest(bytes) != *digest {
        return Err(WebToolError::DigestMismatch);
    }
    let refuse = || WebToolError::ContentMalformed("content");
    let rest = bytes.strip_prefix(CONTENT_DOMAIN).ok_or_else(refuse)?;
    let (&kind, rest) = rest.split_first().ok_or_else(refuse)?;
    if !matches!(kind, 1 | 2) {
        return Err(refuse());
    }
    let (payload, rest) = take_prefixed::<4>(rest).ok_or_else(refuse)?;
    let (media_type, rest) = take_prefixed::<4>(rest).ok_or_else(refuse)?;
    let (text, rest) = take_prefixed::<8>(rest).ok_or_else(refuse)?;
    let media_type = std::str::from_utf8(media_type).map_err(|_| refuse())?;
    if !rest.is_empty() || media_essence(media_type).as_deref() != Some(media_type) {
        return Err(refuse());
    }
    Ok(WebResult::Content {
        digest: *digest,
        kind,
        opaque_payload: payload.to_vec(),
        media_type: media_type.to_owned(),
        opaque_text: String::from_utf8(text.to_vec()).map_err(|_| refuse())?,
    })
}

fn take_prefixed<const N: usize>(bytes: &[u8]) -> Option<(&[u8], &[u8])> {
    let (length, rest) = bytes.split_first_chunk::<N>()?;
    let mut wide = [0_u8; 8];
    wide[8 - N..].copy_from_slice(length);
    let length = usize::try_from(u64::from_be_bytes(wide)).ok()?;
    (rest.len() >= length).then(|| rest.split_at(length))
}

/// The canonical content bytes: the domain, the kind, the payload with a
/// big-endian `u32` length, the media type without parameters in lower case
/// with a `u32` length, and the text with a `u64` length.
///
/// # Errors
/// Refuses a malformed media type and a field longer than its length prefix.
pub fn canonical_bytes(
    kind: u8,
    payload: &[u8],
    media_type: &str,
    text: &str,
) -> Result<Vec<u8>, WebToolError> {
    let refuse = || WebToolError::ContentMalformed("canonical");
    let media_type = media_essence(media_type).ok_or_else(refuse)?;
    let payload_length = u32::try_from(payload.len()).map_err(|_| refuse())?;
    let media_length = u32::try_from(media_type.len()).map_err(|_| refuse())?;
    let text_length = u64::try_from(text.len()).map_err(|_| refuse())?;
    let mut bytes = Vec::with_capacity(
        CONTENT_DOMAIN.len() + 17 + payload.len() + media_type.len() + text.len(),
    );
    bytes.extend_from_slice(CONTENT_DOMAIN);
    bytes.push(kind);
    bytes.extend_from_slice(&payload_length.to_be_bytes());
    bytes.extend_from_slice(payload);
    bytes.extend_from_slice(&media_length.to_be_bytes());
    bytes.extend_from_slice(media_type.as_bytes());
    bytes.extend_from_slice(&text_length.to_be_bytes());
    bytes.extend_from_slice(text.as_bytes());
    Ok(bytes)
}

/// keccak256 of canonical content bytes.
#[must_use]
pub fn content_digest(canonical: &[u8]) -> [u8; 32] {
    Keccak256::digest(canonical).into()
}

fn media_essence(media_type: &str) -> Option<String> {
    let essence = media_type.split(';').next()?.trim().to_ascii_lowercase();
    let (kind, subtype) = essence.split_once('/')?;
    let token = |part: &str| {
        !part.is_empty()
            && part
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || b"!#$&-^_.+".contains(&byte))
    };
    (token(kind) && token(subtype)).then_some(essence)
}

fn purpose_of(receiver_did: &str) -> Option<[u8; 32]> {
    let key = receiver_did.strip_prefix("did:layerx:").and_then(unhex32)?;
    let mut hasher = Sha256::new();
    hasher.update(PURPOSE_DOMAIN);
    hasher.update(key);
    Some(hasher.finalize().into())
}

/// The account `did` pays or is paid in `currency`: the main account
/// `agent:<did>:main` for PAX, the kernel's native coin, and the per-asset
/// account `agent:<did>:asset:<id>` for SID, USDC and USDL.
fn wallet_account(did: &str, currency: Currency, asset: &[u8; 32]) -> String {
    format!("agent:{did}{}", account_suffix(currency, asset))
}

fn account_suffix(currency: Currency, asset: &[u8; 32]) -> String {
    match currency {
        Currency::Pax => ":main".to_owned(),
        Currency::Sid | Currency::Usdc | Currency::Usdl => format!(":asset:{}", hex(asset)),
    }
}

fn valid_payer(did: &str) -> bool {
    did.len() <= MAX_DID_BYTES
        && did.starts_with("did:")
        && did.split(':').count() >= 3
        && !did.ends_with(':')
        && !did.contains("::")
        && !did.contains(":asset:")
        && did
            .bytes()
            .all(|byte| matches!(byte, b'a'..=b'z' | b'0'..=b'9' | b'.' | b'_' | b':' | b'-'))
}

fn trace_entropy(key: &[u8; 32], target: &str) -> [u8; 16] {
    let mut hasher = Sha256::new();
    hasher.update(TRACE_DOMAIN);
    hasher.update(key);
    hasher.update(target.as_bytes());
    let digest: [u8; 32] = hasher.finalize().into();
    let mut entropy = [0; 16];
    entropy.copy_from_slice(&digest[..16]);
    entropy
}

fn decode_header<T: serde::de::DeserializeOwned>(header: &str) -> Option<T> {
    let bytes = STANDARD.decode(header.trim()).ok()?;
    serde_json::from_slice(&bytes).ok()
}

struct HttpReply {
    status: u16,
    headers: Vec<(String, String)>,
    body: Vec<u8>,
}

impl HttpReply {
    fn header(&self, name: &str) -> Option<&str> {
        self.headers
            .iter()
            .find(|(candidate, _)| candidate.eq_ignore_ascii_case(name))
            .map(|(_, value)| value.as_str())
    }

    fn error_code(&self) -> Option<String> {
        serde_json::from_slice::<Value>(&self.body)
            .ok()?
            .get("error")?
            .as_str()
            .map(str::to_owned)
    }
}

fn retry_delay(reply: &HttpReply) -> Duration {
    reply
        .header("Retry-After")
        .and_then(|value| value.trim().parse::<u64>().ok())
        .map_or(Duration::from_secs(1), Duration::from_secs)
        .min(MAX_RETRY_DELAY)
}

fn request(
    config: &WebConfig,
    target: &str,
    headers: &[(&str, String)],
) -> Result<HttpReply, WebToolError> {
    let endpoint = &config.endpoint;
    let address = (endpoint.host.as_str(), endpoint.port)
        .to_socket_addrs()
        .map_err(|_| WebToolError::Transport)?
        .next()
        .ok_or(WebToolError::Transport)?;
    let mut stream = TcpStream::connect_timeout(&address, config.timeout)
        .map_err(|_| WebToolError::Transport)?;
    stream
        .set_read_timeout(Some(config.timeout))
        .and_then(|()| stream.set_write_timeout(Some(config.timeout)))
        .map_err(|_| WebToolError::Transport)?;
    let mut head = format!(
        "GET {target} HTTP/1.1\r\nHost: {}\r\nConnection: close\r\n",
        endpoint.authority
    );
    for (name, value) in headers {
        if value.bytes().any(|byte| byte.is_ascii_control()) {
            return Err(WebToolError::Protocol("header_value"));
        }
        write!(head, "{name}: {value}\r\n").map_err(|_| WebToolError::Protocol("header_value"))?;
    }
    head.push_str("\r\n");
    stream
        .write_all(head.as_bytes())
        .map_err(|_| WebToolError::Transport)?;
    let mut raw = Vec::new();
    let limit = u64::try_from(MAX_RESPONSE_BYTES)
        .map_err(|_| WebToolError::Protocol("response_limit"))?
        .saturating_add(1);
    stream
        .take(limit)
        .read_to_end(&mut raw)
        .map_err(|_| WebToolError::Transport)?;
    if raw.len() > MAX_RESPONSE_BYTES {
        return Err(WebToolError::Protocol("response_too_large"));
    }
    parse_reply(&raw)
}

fn parse_reply(raw: &[u8]) -> Result<HttpReply, WebToolError> {
    let malformed = || WebToolError::Protocol("malformed_response");
    let end = raw
        .windows(4)
        .position(|window| window == b"\r\n\r\n")
        .ok_or_else(malformed)?;
    let head = std::str::from_utf8(&raw[..end]).map_err(|_| malformed())?;
    let mut lines = head.split("\r\n");
    let status = lines
        .next()
        .and_then(|line| {
            line.strip_prefix("HTTP/1.1 ")
                .or_else(|| line.strip_prefix("HTTP/1.0 "))
        })
        .and_then(|rest| rest.get(..3))
        .and_then(|code| code.parse::<u16>().ok())
        .ok_or_else(malformed)?;
    let mut headers = Vec::new();
    for line in lines {
        let (name, value) = line.split_once(':').ok_or_else(malformed)?;
        headers.push((name.trim().to_owned(), value.trim().to_owned()));
    }
    let mut body = raw[end + 4..].to_vec();
    let declared = headers
        .iter()
        .find(|(name, _)| name.eq_ignore_ascii_case("content-length"))
        .map(|(_, value)| value.parse::<usize>().map_err(|_| malformed()))
        .transpose()?;
    if let Some(length) = declared {
        if body.len() < length {
            return Err(malformed());
        }
        body.truncate(length);
    }
    Ok(HttpReply {
        status,
        headers,
        body,
    })
}

fn percent_encode(text: &str) -> String {
    let mut encoded = String::with_capacity(text.len());
    for byte in text.bytes() {
        if byte.is_ascii_alphanumeric() || b"-._~".contains(&byte) {
            encoded.push(char::from(byte));
        } else {
            let _ = write!(encoded, "%{byte:02X}");
        }
    }
    encoded
}

fn hex(bytes: &[u8]) -> String {
    const DIGITS: &[u8; 16] = b"0123456789abcdef";
    let mut output = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        output.push(char::from(DIGITS[usize::from(byte >> 4)]));
        output.push(char::from(DIGITS[usize::from(byte & 0x0f)]));
    }
    output
}

fn unhex32(text: &str) -> Option<[u8; 32]> {
    if text.len() != 64 {
        return None;
    }
    let mut output = [0_u8; 32];
    for (index, pair) in text.as_bytes().chunks_exact(2).enumerate() {
        let pair = std::str::from_utf8(pair).ok()?;
        output[index] = u8::from_str_radix(pair, 16).ok()?;
    }
    Some(output)
}
