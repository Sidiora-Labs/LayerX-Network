pub use layerx_proof::inclusion::SequencerAuthorization;
use std::fs::{self, File};
use std::io::{Read as _, Write as _};
use std::net::{IpAddr, TcpStream, ToSocketAddrs as _};
use std::path::{Path, PathBuf};
use std::time::Duration;

use ed25519_dalek::{Signature as Ed25519Signature, VerifyingKey};
use layerx_crypto::{ed25519, SignatureMessage};
use layerx_proof::receipt::AuthorizedBatch;
use layerx_types::activity::{
    Authority, EnvelopeBuilder, Signature, TimestampBound, UnsignedEnvelope,
};
use layerx_types::amount::Amount;
use layerx_types::ids::{Did, IdempotencyKey};
use layerx_wire::activity::{encode_signed_envelope, encode_unsigned_envelope};
use layerx_wire::hash::{activity_id, Domain};
use native_tls::{Certificate, Identity, TlsConnector};
use serde::{Deserialize, Serialize};
use sha2::{Digest as _, Sha256};

use crate::{
    compile_operator_send, compile_payer_grant_draw, operator_send_authorization_message,
    verify_order_receipt, AuthenticatedPrincipal, RampDirection, RampError, RampOrder,
    ReceiptEvidence, VerifiedLayerxLeg, COMPLIANCE_CONTRACT_VERSION, PAXEER_CONTRACT_VERSION,
    PROVIDER_CONTRACT_VERSION,
};

const MAX_SECRET_BYTES: usize = 64 * 1024;
const MAX_CA_BYTES: usize = 1024 * 1024;
const MAX_BODY_BYTES: usize = 512 * 1024;
const MAX_HEADER_BYTES: usize = 32 * 1024;

#[derive(Clone, Debug)]
pub struct SecretFile(PathBuf);

impl SecretFile {
    /// # Errors
    /// Returns [`RampError::Configuration`] when the path is not a private file within the size bound.
    pub fn new(path: impl Into<PathBuf>) -> Result<Self, RampError> {
        let path = path.into();
        let metadata = fs::metadata(&path).map_err(|_| RampError::Configuration)?;
        if !metadata.is_file() || metadata.len() == 0 || metadata.len() > MAX_SECRET_BYTES as u64 {
            return Err(RampError::Configuration);
        }
        require_private(&metadata)?;
        Ok(Self(path))
    }

    /// # Errors
    /// Returns [`RampError::Configuration`] when the file cannot be read within the size bound.
    pub fn read(&self) -> Result<Vec<u8>, RampError> {
        let file = File::open(&self.0).map_err(|_| RampError::Configuration)?;
        let mut bytes = Vec::new();
        file.take((MAX_SECRET_BYTES + 1) as u64)
            .read_to_end(&mut bytes)
            .map_err(|_| RampError::Configuration)?;
        if bytes.is_empty() || bytes.len() > MAX_SECRET_BYTES {
            return Err(RampError::Configuration);
        }
        Ok(bytes)
    }

    #[must_use]
    pub fn path(&self) -> &Path {
        &self.0
    }
}

#[derive(Clone, Debug)]
pub struct MutualTlsFiles {
    pub ca_pem: PathBuf,
    pub identity_pkcs12: SecretFile,
    pub identity_password: SecretFile,
}

#[derive(Clone, Debug)]
pub struct Endpoint {
    host: String,
    port: u16,
    base_path: String,
}

impl Endpoint {
    /// # Errors
    /// Returns [`RampError::Configuration`] when the value is not a canonical HTTPS DNS endpoint.
    pub fn parse(value: &str) -> Result<Self, RampError> {
        let rest = value
            .strip_prefix("https://")
            .ok_or(RampError::Configuration)?;
        let (authority, path) = rest.split_once('/').map_or((rest, ""), |value| value);
        if authority.is_empty()
            || authority.contains(['@', '?', '#', '\\'])
            || path.contains(['?', '#', '\\'])
        {
            return Err(RampError::Configuration);
        }
        let (host, port) = authority.rsplit_once(':').map_or_else(
            || Ok::<_, RampError>((authority.to_owned(), 443)),
            |(host, port)| {
                Ok((
                    host.to_owned(),
                    port.parse::<u16>().map_err(|_| RampError::Configuration)?,
                ))
            },
        )?;
        if host.is_empty() || host.parse::<IpAddr>().is_ok() {
            return Err(RampError::Configuration);
        }
        Ok(Self {
            host,
            port,
            base_path: if path.is_empty() {
                String::new()
            } else {
                format!("/{}", path.trim_end_matches('/'))
            },
        })
    }

    fn authority(&self) -> String {
        if self.port == 443 {
            self.host.clone()
        } else {
            format!("{}:{}", self.host, self.port)
        }
    }
}

#[derive(Clone, Copy, Debug)]
pub struct HttpRequest<'a> {
    pub endpoint: &'a Endpoint,
    pub method: &'a str,
    pub path: &'a str,
    pub authorization: Option<&'a str>,
    pub idempotency: Option<&'a str>,
    pub contract: Option<&'a str>,
}

pub struct MutualTlsClient {
    connector: TlsConnector,
    timeout: Duration,
}

impl MutualTlsClient {
    /// # Errors
    /// Returns [`RampError::Configuration`] when the timeout, CA or client identity cannot be used.
    pub fn new(files: &MutualTlsFiles, timeout: Duration) -> Result<Self, RampError> {
        if timeout.is_zero() {
            return Err(RampError::Configuration);
        }
        let mut ca_bytes = Vec::new();
        File::open(&files.ca_pem)
            .map_err(|_| RampError::Configuration)?
            .take((MAX_CA_BYTES + 1) as u64)
            .read_to_end(&mut ca_bytes)
            .map_err(|_| RampError::Configuration)?;
        if ca_bytes.is_empty() || ca_bytes.len() > MAX_CA_BYTES {
            return Err(RampError::Configuration);
        }
        let ca = Certificate::from_pem(&ca_bytes).map_err(|_| RampError::Configuration)?;
        let identity_bytes = files.identity_pkcs12.read()?;
        let password_bytes = files.identity_password.read()?;
        let password = std::str::from_utf8(&password_bytes)
            .map_err(|_| RampError::Configuration)?
            .trim_end_matches(['\r', '\n']);
        let identity = Identity::from_pkcs12(&identity_bytes, password)
            .map_err(|_| RampError::Configuration)?;
        let connector = TlsConnector::builder()
            .disable_built_in_roots(true)
            .add_root_certificate(ca)
            .identity(identity)
            .min_protocol_version(Some(native_tls::Protocol::Tlsv12))
            .build()
            .map_err(|_| RampError::Configuration)?;
        Ok(Self { connector, timeout })
    }

    /// # Errors
    /// Returns [`RampError::Configuration`] when the request exceeds its bound and
    /// [`RampError::Provider`] when transport or the HTTP response is refused.
    pub fn json<T: Serialize>(
        &self,
        request: HttpRequest<'_>,
        body: Option<&T>,
    ) -> Result<HttpResponse, RampError> {
        let body = body
            .map(serde_json::to_vec)
            .transpose()
            .map_err(|_| RampError::Configuration)?
            .unwrap_or_default();
        self.request(request, "application/json", &body)
    }

    /// # Errors
    /// Returns [`RampError::Configuration`] when the request exceeds its bound and
    /// [`RampError::Provider`] when transport or the HTTP response is refused.
    pub fn request(
        &self,
        request: HttpRequest<'_>,
        content_type: &str,
        body: &[u8],
    ) -> Result<HttpResponse, RampError> {
        let HttpRequest {
            endpoint,
            method,
            path,
            authorization,
            idempotency,
            contract,
        } = request;
        if !matches!(method, "GET" | "POST")
            || !path.starts_with('/')
            || path.contains(['?', '#', '\\'])
            || path.split('/').any(|segment| matches!(segment, "." | ".."))
            || body.len() > MAX_BODY_BYTES
            || authorization.is_some_and(invalid_header)
            || idempotency.is_some_and(invalid_header)
            || contract.is_some_and(invalid_header)
        {
            return Err(RampError::Configuration);
        }
        let address = (endpoint.host.as_str(), endpoint.port)
            .to_socket_addrs()
            .map_err(|_| RampError::Provider)?
            .next()
            .ok_or(RampError::Provider)?;
        let stream =
            TcpStream::connect_timeout(&address, self.timeout).map_err(|_| RampError::Provider)?;
        stream
            .set_read_timeout(Some(self.timeout))
            .and_then(|()| stream.set_write_timeout(Some(self.timeout)))
            .map_err(|_| RampError::Provider)?;
        let mut tls = self
            .connector
            .connect(&endpoint.host, stream)
            .map_err(|_| RampError::Provider)?;
        let target = format!("{}{}", endpoint.base_path, path);
        let mut headers = format!(
            "{method} {target} HTTP/1.1\r\nHost: {}\r\nContent-Type: {content_type}\r\nAccept: application/json\r\nContent-Length: {}\r\nConnection: close\r\n",
            endpoint.authority(),
            body.len()
        );
        if let Some(value) = authorization {
            headers.push_str("Authorization: ");
            headers.push_str(value);
            headers.push_str("\r\n");
        }
        if let Some(value) = idempotency {
            headers.push_str("Idempotency-Key: ");
            headers.push_str(value);
            headers.push_str("\r\n");
        }
        if let Some(value) = contract {
            headers.push_str("LayerX-Ramp-Contract: ");
            headers.push_str(value);
            headers.push_str("\r\n");
        }
        headers.push_str("\r\n");
        tls.write_all(headers.as_bytes())
            .and_then(|()| tls.write_all(body))
            .and_then(|()| tls.flush())
            .map_err(|_| RampError::Provider)?;
        let mut response = Vec::new();
        tls.take((MAX_HEADER_BYTES + MAX_BODY_BYTES + 1) as u64)
            .read_to_end(&mut response)
            .map_err(|_| RampError::Provider)?;
        parse_response(&response)
    }
}

#[derive(Clone, Debug)]
pub struct HttpResponse {
    pub status: u16,
    pub body: Vec<u8>,
}

fn parse_response(bytes: &[u8]) -> Result<HttpResponse, RampError> {
    let boundary = bytes
        .windows(4)
        .position(|window| window == b"\r\n\r\n")
        .ok_or(RampError::Provider)?;
    if boundary > MAX_HEADER_BYTES {
        return Err(RampError::Provider);
    }
    let header = std::str::from_utf8(&bytes[..boundary]).map_err(|_| RampError::Provider)?;
    let status_line = header.lines().next().ok_or(RampError::Provider)?;
    let mut status_parts = status_line.split_ascii_whitespace();
    if status_parts.next() != Some("HTTP/1.1") {
        return Err(RampError::Provider);
    }
    let status = status_parts
        .next()
        .and_then(|value| value.parse::<u16>().ok())
        .filter(|value| (100..600).contains(value))
        .ok_or(RampError::Provider)?;
    let mut content_length = None;
    let mut chunked = false;
    let mut content_type = None;
    for line in header.lines().skip(1) {
        let (name, value) = line.split_once(':').ok_or(RampError::Provider)?;
        if name.eq_ignore_ascii_case("content-length") {
            let length = value
                .trim()
                .parse::<usize>()
                .map_err(|_| RampError::Provider)?;
            if content_length.replace(length).is_some() {
                return Err(RampError::Provider);
            }
        } else if name.eq_ignore_ascii_case("transfer-encoding") {
            if chunked || !value.trim().eq_ignore_ascii_case("chunked") {
                return Err(RampError::Provider);
            }
            chunked = true;
        } else if name.eq_ignore_ascii_case("content-type")
            && content_type.replace(value.trim()).is_some()
        {
            return Err(RampError::Provider);
        }
    }
    if chunked && content_length.is_some() {
        return Err(RampError::Provider);
    }
    let raw_body = bytes
        .get(boundary.saturating_add(4)..)
        .ok_or(RampError::Provider)?;
    let body = if chunked {
        decode_chunked(raw_body)?
    } else if let Some(length) = content_length {
        if length > MAX_BODY_BYTES || raw_body.len() != length {
            return Err(RampError::Provider);
        }
        raw_body.to_vec()
    } else {
        raw_body.to_vec()
    };
    if body.len() > MAX_BODY_BYTES {
        return Err(RampError::Provider);
    }
    if !body.is_empty() && content_type != Some("application/json") {
        return Err(RampError::Provider);
    }
    Ok(HttpResponse { status, body })
}

fn decode_chunked(mut input: &[u8]) -> Result<Vec<u8>, RampError> {
    let mut output = Vec::new();
    loop {
        let line_end = input
            .windows(2)
            .position(|window| window == b"\r\n")
            .ok_or(RampError::Provider)?;
        let line = std::str::from_utf8(&input[..line_end]).map_err(|_| RampError::Provider)?;
        if line.contains(';') {
            return Err(RampError::Provider);
        }
        let length = usize::from_str_radix(line, 16).map_err(|_| RampError::Provider)?;
        input = input
            .get(line_end.saturating_add(2)..)
            .ok_or(RampError::Provider)?;
        if length == 0 {
            return if input == b"\r\n" || input.is_empty() {
                Ok(output)
            } else {
                Err(RampError::Provider)
            };
        }
        if output.len().saturating_add(length) > MAX_BODY_BYTES {
            return Err(RampError::Provider);
        }
        let chunk = input.get(..length).ok_or(RampError::Provider)?;
        if input.get(length..length.saturating_add(2)) != Some(b"\r\n") {
            return Err(RampError::Provider);
        }
        output.extend_from_slice(chunk);
        input = input
            .get(length.saturating_add(2)..)
            .ok_or(RampError::Provider)?;
    }
}

fn invalid_header(value: &str) -> bool {
    value.is_empty()
        || value.len() > 4096
        || value.bytes().any(|byte| matches!(byte, b'\r' | b'\n' | 0))
}

fn safe_segment(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 128
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.' | b':'))
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ComplianceDecision {
    pub decision_id: String,
    pub order_digest: [u8; 32],
    pub customer_principal: String,
    pub operator_principal: String,
    pub decision: ComplianceOutcome,
    pub reason_code: String,
    pub issued_at: u64,
    pub expires_at: u64,
    pub signature: String,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ComplianceOutcome {
    Approved,
    Refused,
    ManualReview,
}

impl ComplianceDecision {
    /// # Errors
    /// Returns [`RampError::Compliance`] when the decision does not bind the order or fail signature verification.
    pub fn verify(
        &self,
        order: &RampOrder,
        public_key: &[u8; 32],
        now: u64,
    ) -> Result<(), RampError> {
        if self.order_digest != order.order_digest
            || self.customer_principal != order.customer.principal_id
            || self.operator_principal != order.operator.principal_id
            || self.issued_at > now
            || self.expires_at < now
            || self.expires_at > order.quote.expires_at
            || !safe_segment(&self.decision_id)
            || !safe_segment(&self.reason_code)
        {
            return Err(RampError::Compliance);
        }
        verify_detached(public_key, &canonical_compliance(self), &self.signature)
            .map_err(|()| RampError::Compliance)
    }
}

fn canonical_compliance(value: &ComplianceDecision) -> Vec<u8> {
    let mut bytes = Vec::new();
    bytes.extend_from_slice(COMPLIANCE_CONTRACT_VERSION.as_bytes());
    push(&mut bytes, value.decision_id.as_bytes());
    push(&mut bytes, &value.order_digest);
    push(&mut bytes, value.customer_principal.as_bytes());
    push(&mut bytes, value.operator_principal.as_bytes());
    push(
        &mut bytes,
        match value.decision {
            ComplianceOutcome::Approved => b"approved",
            ComplianceOutcome::Refused => b"refused",
            ComplianceOutcome::ManualReview => b"manual_review",
        },
    );
    push(&mut bytes, value.reason_code.as_bytes());
    push(&mut bytes, &value.issued_at.to_be_bytes());
    push(&mut bytes, &value.expires_at.to_be_bytes());
    bytes
}

pub struct ComplianceClient {
    pub http: MutualTlsClient,
    pub endpoint: Endpoint,
    pub service_token: String,
    pub verifying_key: [u8; 32],
}

pub struct IdentityClient {
    pub http: MutualTlsClient,
    pub endpoint: Endpoint,
    pub service_token: String,
    pub audience: String,
}

impl IdentityClient {
    /// # Errors
    /// Returns [`RampError::InvalidPrincipal`] when introspection refuses the presented credential.
    pub fn authenticate(
        &self,
        authorization: &str,
        now: u64,
    ) -> Result<AuthenticatedPrincipal, RampError> {
        #[derive(Serialize)]
        struct Request<'a> {
            token: &'a str,
            audience: &'a str,
        }
        #[derive(Deserialize)]
        #[serde(deny_unknown_fields)]
        struct Response {
            active: bool,
            principal_id: String,
            account: String,
            audience: String,
            expires_at: u64,
        }
        let token = authorization
            .strip_prefix("Bearer ")
            .filter(|value| !invalid_header(value))
            .ok_or(RampError::InvalidPrincipal)?;
        let authorization = format!("Bearer {}", self.service_token);
        let response = self.http.json(
            HttpRequest {
                endpoint: &self.endpoint,
                method: "POST",
                path: "/v1/introspect",
                authorization: Some(&authorization),
                idempotency: None,
                contract: Some("layerx-identity-introspection-v1"),
            },
            Some(&Request {
                token,
                audience: &self.audience,
            }),
        )?;
        if response.status != 200 {
            return Err(RampError::InvalidPrincipal);
        }
        let identity: Response =
            serde_json::from_slice(&response.body).map_err(|_| RampError::InvalidPrincipal)?;
        if !identity.active || identity.audience != self.audience || identity.expires_at <= now {
            return Err(RampError::InvalidPrincipal);
        }
        let principal = AuthenticatedPrincipal {
            principal_id: identity.principal_id,
            account: identity.account,
        };
        super::validate_principal(&principal)?;
        Ok(principal)
    }
}

impl ComplianceClient {
    /// # Errors
    /// Returns [`RampError::Compliance`] when the decision is refused or fails verification.
    pub fn evaluate(&self, order: &RampOrder, now: u64) -> Result<ComplianceDecision, RampError> {
        #[derive(Serialize)]
        struct Request<'a> {
            contract: &'static str,
            order: &'a RampOrder,
        }
        let authorization = format!("Bearer {}", self.service_token);
        let idempotency = hex(&order.order_digest);
        let response = self.http.json(
            HttpRequest {
                endpoint: &self.endpoint,
                method: "POST",
                path: "/v1/decisions",
                authorization: Some(&authorization),
                idempotency: Some(&idempotency),
                contract: Some(COMPLIANCE_CONTRACT_VERSION),
            },
            Some(&Request {
                contract: COMPLIANCE_CONTRACT_VERSION,
                order,
            }),
        )?;
        if response.status != 200 {
            return Err(RampError::Compliance);
        }
        let decision: ComplianceDecision =
            serde_json::from_slice(&response.body).map_err(|_| RampError::Compliance)?;
        decision.verify(order, &self.verifying_key, now)?;
        Ok(decision)
    }
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ProviderState {
    SubmittedUnknown,
    Pending,
    Settled,
    Refused,
    Reversed,
    ManualReview,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ProviderResult {
    pub operation_id: String,
    pub order_digest: [u8; 32],
    pub direction: RampDirection,
    pub order_id: String,
    pub quote_id: String,
    pub customer_principal: String,
    pub layerx_asset: [u8; 32],
    pub layerx_amount: u128,
    pub provider_token: String,
    pub beneficiary_token: String,
    pub amount_minor: u128,
    pub currency: String,
    pub state: ProviderState,
    pub evidence_digest: Option<[u8; 32]>,
    pub refusal_code: Option<String>,
    pub retry_at: Option<u64>,
}

impl ProviderResult {
    fn validate(&self, order: &RampOrder) -> Result<(), RampError> {
        let coded_state = matches!(
            self.state,
            ProviderState::Refused | ProviderState::Reversed | ProviderState::ManualReview
        );
        if self.order_digest != order.order_digest
            || self.direction != order.direction()
            || self.order_id != order.order_id
            || self.quote_id != order.quote.quote_id
            || self.customer_principal != order.customer.principal_id
            || self.layerx_asset != order.quote.layerx_asset
            || self.layerx_amount != order.quote.layerx_amount
            || self.provider_token != order.quote.provider_token
            || self.beneficiary_token != order.quote.payout_token
            || self.amount_minor != order.quote.external_amount_minor
            || self.currency != order.quote.external_currency
            || !safe_segment(&self.operation_id)
            || matches!(self.state, ProviderState::Settled | ProviderState::Reversed)
                && self.evidence_digest.is_none_or(|digest| digest == [0; 32])
            || coded_state
                && self
                    .refusal_code
                    .as_deref()
                    .is_none_or(|code| !safe_segment(code))
            || !coded_state && self.refusal_code.is_some()
            || matches!(
                self.state,
                ProviderState::SubmittedUnknown | ProviderState::Pending
            ) && self.retry_at.is_none_or(|retry| retry == 0)
        {
            return Err(RampError::Provider);
        }
        Ok(())
    }
}

pub struct ProviderClient {
    pub http: MutualTlsClient,
    pub endpoint: Endpoint,
    pub credential: String,
    pub settlement_path: String,
    pub status_path: String,
}

impl ProviderClient {
    /// # Errors
    /// Returns [`RampError::Provider`] when the provider refuses or returns a result that does not bind the order.
    pub fn submit(&self, order: &RampOrder) -> Result<ProviderResult, RampError> {
        #[derive(Serialize)]
        struct Request<'a> {
            contract: &'static str,
            order_digest: [u8; 32],
            direction: RampDirection,
            order_id: &'a str,
            quote_id: &'a str,
            customer_principal: &'a str,
            layerx_asset: [u8; 32],
            layerx_amount: u128,
            provider_token: &'a str,
            beneficiary_token: &'a str,
            amount_minor: u128,
            currency: &'a str,
            expires_at: u64,
        }
        let body = Request {
            contract: PROVIDER_CONTRACT_VERSION,
            order_digest: order.order_digest,
            direction: order.direction(),
            order_id: &order.order_id,
            quote_id: &order.quote.quote_id,
            customer_principal: &order.customer.principal_id,
            layerx_asset: order.quote.layerx_asset,
            layerx_amount: order.quote.layerx_amount,
            provider_token: &order.quote.provider_token,
            beneficiary_token: &order.quote.payout_token,
            amount_minor: order.quote.external_amount_minor,
            currency: &order.quote.external_currency,
            expires_at: order.quote.expires_at,
        };
        let authorization = format!("Bearer {}", self.credential);
        let idempotency = hex(&order.order_digest);
        let response = self.http.json(
            HttpRequest {
                endpoint: &self.endpoint,
                method: "POST",
                path: &self.settlement_path,
                authorization: Some(&authorization),
                idempotency: Some(&idempotency),
                contract: Some(PROVIDER_CONTRACT_VERSION),
            },
            Some(&body),
        )?;
        Self::decode(order, &response)
    }

    /// # Errors
    /// Returns [`RampError::Provider`] when the operation identifier is invalid or the provider result does not bind the order.
    pub fn reconcile(
        &self,
        order: &RampOrder,
        operation_id: &str,
    ) -> Result<ProviderResult, RampError> {
        if !safe_segment(operation_id) && !operation_id.starts_with("idempotency:") {
            return Err(RampError::Provider);
        }
        let path = operation_id.strip_prefix("idempotency:").map_or_else(
            || {
                format!(
                    "{}/{}",
                    self.status_path.trim_end_matches('/'),
                    operation_id
                )
            },
            |idempotency| {
                format!(
                    "{}/by-idempotency/{}",
                    self.status_path.trim_end_matches('/'),
                    idempotency
                )
            },
        );
        let authorization = format!("Bearer {}", self.credential);
        let response = self.http.json::<serde_json::Value>(
            HttpRequest {
                endpoint: &self.endpoint,
                method: "GET",
                path: &path,
                authorization: Some(&authorization),
                idempotency: None,
                contract: Some(PROVIDER_CONTRACT_VERSION),
            },
            None,
        )?;
        Self::decode(order, &response)
    }

    fn decode(order: &RampOrder, response: &HttpResponse) -> Result<ProviderResult, RampError> {
        if !matches!(response.status, 200 | 202 | 409 | 422) {
            return Err(RampError::Provider);
        }
        let result: ProviderResult =
            serde_json::from_slice(&response.body).map_err(|_| RampError::Provider)?;
        result.validate(order)?;
        Ok(result)
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ProviderCallback {
    pub callback_id: String,
    pub provider_sequence: u64,
    pub result: ProviderResult,
    pub signature: String,
}

impl ProviderCallback {
    /// # Errors
    /// Returns [`RampError::Provider`] when the callback identity, signature or bound result is refused.
    pub fn verify(&self, order: &RampOrder, public_key: &[u8; 32]) -> Result<(), RampError> {
        if !safe_segment(&self.callback_id) || self.provider_sequence == 0 {
            return Err(RampError::Provider);
        }
        let canonical = serde_json::to_vec(&(
            PROVIDER_CONTRACT_VERSION,
            &self.callback_id,
            self.provider_sequence,
            &self.result,
        ))
        .map_err(|_| RampError::Provider)?;
        verify_detached(public_key, &canonical, &self.signature)
            .map_err(|()| RampError::Provider)?;
        self.result.validate(order)
    }
}

#[derive(Clone, Debug)]
pub struct ActivityConfig {
    pub actor_did: Vec<u8>,
    pub protocol_version: u16,
    pub network_id: u32,
    pub fee_limit: u128,
    pub signer_public_key: [u8; 32],
}

pub struct LayerxClient {
    pub sequencer_authorization: SequencerAuthorization,
    pub http: MutualTlsClient,
    pub gateway: Endpoint,
    pub receipt_authority: Endpoint,
    pub signer: Endpoint,
    pub gateway_key: String,
    pub authority_token: String,
    pub signer_token: String,
    pub activity: ActivityConfig,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum LayerxSubmission {
    Unknown {
        activity_id: [u8; 32],
        canonical_activity: Option<Vec<u8>>,
    },
    Pending {
        activity_id: [u8; 32],
        canonical_activity: Option<Vec<u8>>,
    },
    Refused {
        activity_id: [u8; 32],
        canonical_activity: Vec<u8>,
        code: String,
    },
    Verified {
        leg: VerifiedLayerxLeg,
        canonical_activity: Option<Vec<u8>>,
    },
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PreparedLayerx {
    activity_id: [u8; 32],
    canonical_activity: Vec<u8>,
}

impl PreparedLayerx {
    #[must_use]
    pub const fn activity_id(&self) -> [u8; 32] {
        self.activity_id
    }

    #[must_use]
    pub fn canonical_activity(&self) -> &[u8] {
        &self.canonical_activity
    }
}

impl LayerxClient {
    /// # Errors
    /// Returns [`RampError::InvalidOrder`], [`RampError::Intent`] or [`RampError::Layerx`] when compilation, encoding or signing fails.
    pub fn prepare_payment(
        &self,
        order: &RampOrder,
        account_sequence: u64,
        now: u64,
        registry: &layerx_types::payload::ModuleRegistry,
    ) -> Result<PreparedLayerx, RampError> {
        let compiled = match order.direction() {
            RampDirection::OnRamp => {
                let message = operator_send_authorization_message(
                    order,
                    account_sequence,
                    self.activity.network_id,
                    self.activity.protocol_version,
                )?;
                let authorization = self.sign(order, &message)?;
                compile_operator_send(
                    order,
                    account_sequence,
                    self.activity.network_id,
                    self.activity.protocol_version,
                    self.activity.signer_public_key,
                    authorization,
                    registry,
                )?
            }
            RampDirection::OffRamp => compile_payer_grant_draw(order, account_sequence, registry)?,
        };
        let unsigned = self.unsigned(order, account_sequence, now, compiled)?;
        let canonical = encode_unsigned_envelope(&unsigned).map_err(|_| RampError::Layerx)?;
        let signature = self.sign(order, &canonical)?;
        let signed =
            unsigned.attach_signature(Signature::new(&signature).map_err(|_| RampError::Layerx)?);
        let signed_bytes = encode_signed_envelope(&signed).map_err(|_| RampError::Layerx)?;
        let decoded = layerx_wire::activity::decode_signed(&signed_bytes, registry)
            .map_err(|_| RampError::Layerx)?;
        let identifier = activity_id(&decoded).map_err(|_| RampError::Layerx)?;
        Ok(PreparedLayerx {
            activity_id: identifier,
            canonical_activity: signed_bytes,
        })
    }

    #[must_use]
    pub fn submit_prepared(&self, order: &RampOrder, prepared: PreparedLayerx) -> LayerxSubmission {
        let identifier = prepared.activity_id;
        let signed_bytes = prepared.canonical_activity;
        let authorization = format!("LayerX-Key {}", self.gateway_key);
        let idempotency = hex(&order.order_digest);
        let Ok(response) = self.http.request(
            HttpRequest {
                endpoint: &self.gateway,
                method: "POST",
                path: "/v1/activities",
                authorization: Some(&authorization),
                idempotency: Some(&idempotency),
                contract: None,
            },
            "application/octet-stream",
            &signed_bytes,
        ) else {
            return LayerxSubmission::Unknown {
                activity_id: identifier,
                canonical_activity: Some(signed_bytes),
            };
        };
        if response.status == 202 {
            return LayerxSubmission::Pending {
                activity_id: identifier,
                canonical_activity: Some(signed_bytes),
            };
        }
        if matches!(response.status, 400 | 401 | 403 | 404 | 422) {
            return LayerxSubmission::Refused {
                activity_id: identifier,
                canonical_activity: signed_bytes,
                code: format!("gateway_http_{}", response.status),
            };
        }
        if response.status != 200 {
            return LayerxSubmission::Unknown {
                activity_id: identifier,
                canonical_activity: Some(signed_bytes),
            };
        }
        match self.resolve(order, identifier) {
            Ok(LayerxSubmission::Unknown { activity_id, .. }) => LayerxSubmission::Unknown {
                activity_id,
                canonical_activity: Some(signed_bytes),
            },
            Ok(LayerxSubmission::Pending { activity_id, .. }) => LayerxSubmission::Pending {
                activity_id,
                canonical_activity: Some(signed_bytes),
            },
            Ok(LayerxSubmission::Verified { leg, .. }) => LayerxSubmission::Verified {
                leg,
                canonical_activity: Some(signed_bytes),
            },
            Ok(LayerxSubmission::Refused { .. }) | Err(_) => LayerxSubmission::Unknown {
                activity_id: identifier,
                canonical_activity: Some(signed_bytes),
            },
        }
    }

    /// # Errors
    /// Returns [`RampError::Layerx`] when receipt or authority facts cannot be verified against the order.
    pub fn resolve(
        &self,
        order: &RampOrder,
        activity: [u8; 32],
    ) -> Result<LayerxSubmission, RampError> {
        #[derive(Deserialize)]
        #[serde(deny_unknown_fields)]
        struct ResultBody {
            result: ReceiptBody,
        }
        #[derive(Deserialize)]
        #[serde(deny_unknown_fields)]
        struct ReceiptBody {
            activity_id: String,
            receipt: String,
        }
        let id = hex(&activity);
        let gateway_authorization = format!("LayerX-Key {}", self.gateway_key);
        let receipt_path = format!("/v1/receipts/{id}");
        let response = self.http.json::<serde_json::Value>(
            HttpRequest {
                endpoint: &self.gateway,
                method: "GET",
                path: &receipt_path,
                authorization: Some(&gateway_authorization),
                idempotency: None,
                contract: None,
            },
            None,
        )?;
        if response.status == 404 {
            return Ok(LayerxSubmission::Pending {
                activity_id: activity,
                canonical_activity: None,
            });
        }
        if response.status != 200 {
            return Ok(LayerxSubmission::Unknown {
                activity_id: activity,
                canonical_activity: None,
            });
        }
        let receipt: ResultBody =
            serde_json::from_slice(&response.body).map_err(|_| RampError::Layerx)?;
        if receipt.result.activity_id != id {
            return Err(RampError::Layerx);
        }
        let canonical_receipt = decode_hex(&receipt.result.receipt, 256 * 1024)?;
        let authority_authorization = format!("Bearer {}", self.authority_token);
        let authority_path = format!("/v1/authorized-batches/by-activity/{id}");
        let authority = self.http.json::<serde_json::Value>(
            HttpRequest {
                endpoint: &self.receipt_authority,
                method: "GET",
                path: &authority_path,
                authorization: Some(&authority_authorization),
                idempotency: None,
                contract: None,
            },
            None,
        )?;
        if authority.status != 200 {
            return Err(RampError::Layerx);
        }
        let facts: AuthorityBody =
            serde_json::from_slice(&authority.body).map_err(|_| RampError::Layerx)?;
        facts.validate_context(
            &id,
            self.activity.network_id,
            self.activity.protocol_version,
            self.sequencer_authorization.public_key(),
        )?;
        let mut evidence = ReceiptEvidence {
            activity_id: activity,
            canonical_receipt,
            authorized_batch: AuthorizedBatch::new(
                parse_hex32(&facts.batch_id)?,
                parse_hex32(&facts.asset)?,
                parse_hex32(&facts.previous_state_root)?,
                parse_hex32(&facts.resulting_state_root)?,
                parse_hex32(&facts.sequencer_public_key)?,
            ),
        };
        if let Some(maintained) = facts.batch_evidence {
            evidence.authorized_batch = maintained
                .authorize(
                    &evidence.canonical_receipt,
                    &evidence.authorized_batch,
                    &self.sequencer_authorization,
                )
                .map_err(|_| RampError::Layerx)?;
        }
        verify_order_receipt(order, &evidence).map(|leg| LayerxSubmission::Verified {
            leg,
            canonical_activity: None,
        })
    }

    fn unsigned(
        &self,
        order: &RampOrder,
        sequence: u64,
        now: u64,
        compiled: crate::CompiledPayment,
    ) -> Result<UnsignedEnvelope, RampError> {
        if now >= order.quote.expires_at {
            return Err(RampError::InvalidOrder);
        }
        let mut builder = EnvelopeBuilder::new();
        builder
            .protocol_version(self.activity.protocol_version)
            .and_then(|builder| builder.network_id(self.activity.network_id))
            .and_then(|builder| builder.activity_type(compiled.activity_type))
            .map_err(|_| RampError::Layerx)?;
        builder
            .actor_did(Did::new(&self.activity.actor_did).map_err(|_| RampError::Layerx)?)
            .and_then(|builder| {
                Authority::owner(&self.activity.signer_public_key)
                    .and_then(|authority| builder.authority(authority))
            })
            .and_then(|builder| builder.account_sequence(sequence))
            .and_then(|builder| {
                TimestampBound::new(now, order.quote.expires_at)
                    .and_then(|bound| builder.timestamp_bound(bound))
            })
            .and_then(|builder| builder.idempotency_key(IdempotencyKey::new(order.order_digest)))
            .and_then(|builder| builder.fee_limit(Amount::from_u128(self.activity.fee_limit)))
            .and_then(|builder| builder.payload_hash(compiled.payload_hash))
            .and_then(|builder| builder.payload(compiled.payload))
            .map_err(|_| RampError::Layerx)?;
        builder.build().map_err(|_| RampError::Layerx)
    }

    fn sign(&self, order: &RampOrder, canonical: &[u8]) -> Result<[u8; 64], RampError> {
        #[derive(Serialize)]
        struct Request<'a> {
            key_handle: &'a str,
            algorithm: &'static str,
            message: String,
        }
        #[derive(Deserialize)]
        #[serde(deny_unknown_fields)]
        struct Response {
            signature: String,
        }
        let message = SignatureMessage::new(
            Domain::SignaturePreimage,
            self.activity.protocol_version,
            self.activity.network_id,
            canonical,
        )
        .map_err(|_| RampError::Layerx)?;
        let digest = message.digest();
        let authorization = format!("Bearer {}", self.signer_token);
        let response = self.http.json(
            HttpRequest {
                endpoint: &self.signer,
                method: "POST",
                path: "/v1/signatures",
                authorization: Some(&authorization),
                idempotency: None,
                contract: None,
            },
            Some(&Request {
                key_handle: &order.operator.signer_key_handle,
                algorithm: "ed25519",
                message: base64_encode(&digest),
            }),
        )?;
        if response.status != 200 {
            return Err(RampError::Layerx);
        }
        let body: Response =
            serde_json::from_slice(&response.body).map_err(|_| RampError::Layerx)?;
        let signature = base64_decode(&body.signature)?;
        let signature: [u8; 64] = signature.try_into().map_err(|_| RampError::Layerx)?;
        ed25519::verify_digest(&self.activity.signer_public_key, &signature, &digest)
            .map_err(|_| RampError::Layerx)?;
        Ok(signature)
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct PaxeerSubmission {
    pub operation_id: String,
    pub idempotency_key: [u8; 32],
    pub operator_account: String,
    pub wallet_address: String,
    pub vault_id: String,
    pub asset: [u8; 32],
    pub amount: u128,
    pub transaction_hash: String,
}

pub struct PaxeerCustodyClient {
    pub http: MutualTlsClient,
    pub endpoint: Endpoint,
    pub credential: String,
    pub broadcast_path: String,
    pub status_path: String,
    pub operator_account: String,
    pub wallet_address: String,
    pub vault_id: String,
    pub signer_key_handle: String,
}

impl PaxeerCustodyClient {
    /// # Errors
    /// Returns [`RampError::Paxeer`] when the custody broadcast is refused or does not bind the requested transfer.
    pub fn broadcast(
        &self,
        asset: [u8; 32],
        amount: u128,
        idempotency_key: [u8; 32],
    ) -> Result<PaxeerSubmission, RampError> {
        #[derive(Serialize)]
        struct Request<'a> {
            contract: &'static str,
            operator_account: &'a str,
            wallet_address: &'a str,
            vault_id: &'a str,
            signer_key_handle: &'a str,
            asset: [u8; 32],
            amount: u128,
            idempotency_key: [u8; 32],
        }
        if asset == [0; 32] || amount == 0 || idempotency_key == [0; 32] {
            return Err(RampError::Paxeer);
        }
        let authorization = format!("Bearer {}", self.credential);
        let idempotency = hex(&idempotency_key);
        let response = self.http.json(
            HttpRequest {
                endpoint: &self.endpoint,
                method: "POST",
                path: &self.broadcast_path,
                authorization: Some(&authorization),
                idempotency: Some(&idempotency),
                contract: Some(PAXEER_CONTRACT_VERSION),
            },
            Some(&Request {
                contract: PAXEER_CONTRACT_VERSION,
                operator_account: &self.operator_account,
                wallet_address: &self.wallet_address,
                vault_id: &self.vault_id,
                signer_key_handle: &self.signer_key_handle,
                asset,
                amount,
                idempotency_key,
            }),
        )?;
        self.decode(&response, asset, amount, idempotency_key)
    }

    /// # Errors
    /// Returns [`RampError::Paxeer`] when custody status is refused or does not bind the requested transfer.
    pub fn reconcile(
        &self,
        asset: [u8; 32],
        amount: u128,
        idempotency_key: [u8; 32],
    ) -> Result<PaxeerSubmission, RampError> {
        let authorization = format!("Bearer {}", self.credential);
        let path = format!(
            "{}/by-idempotency/{}",
            self.status_path.trim_end_matches('/'),
            hex(&idempotency_key)
        );
        let response = self.http.json::<serde_json::Value>(
            HttpRequest {
                endpoint: &self.endpoint,
                method: "GET",
                path: &path,
                authorization: Some(&authorization),
                idempotency: None,
                contract: Some(PAXEER_CONTRACT_VERSION),
            },
            None,
        )?;
        self.decode(&response, asset, amount, idempotency_key)
    }

    fn decode(
        &self,
        response: &HttpResponse,
        asset: [u8; 32],
        amount: u128,
        idempotency_key: [u8; 32],
    ) -> Result<PaxeerSubmission, RampError> {
        if response.status != 200 && response.status != 202 {
            return Err(RampError::Paxeer);
        }
        let submission: PaxeerSubmission =
            serde_json::from_slice(&response.body).map_err(|_| RampError::Paxeer)?;
        if submission.idempotency_key != idempotency_key
            || submission.operator_account != self.operator_account
            || submission.wallet_address != self.wallet_address
            || submission.vault_id != self.vault_id
            || submission.asset != asset
            || submission.amount != amount
            || !safe_segment(&submission.operation_id)
        {
            return Err(RampError::Paxeer);
        }
        layerx_paxeer_client::TransactionHash::from_hex(&submission.transaction_hash)
            .map_err(|_| RampError::Paxeer)?;
        Ok(submission)
    }
}

fn verify_detached(public_key: &[u8; 32], message: &[u8], signature: &str) -> Result<(), ()> {
    let key = VerifyingKey::from_bytes(public_key).map_err(|_| ())?;
    if key.is_weak() {
        return Err(());
    }
    let bytes = decode_hex(signature, 64).map_err(|_| ())?;
    let signature = Ed25519Signature::from_slice(&bytes).map_err(|_| ())?;
    key.verify_strict(message, &signature).map_err(|_| ())
}

fn push(output: &mut Vec<u8>, value: &[u8]) {
    output.extend_from_slice(&(value.len() as u128).to_be_bytes());
    output.extend_from_slice(value);
}

#[must_use]
pub fn hex(bytes: &[u8]) -> String {
    const DIGITS: &[u8; 16] = b"0123456789abcdef";
    let mut output = String::with_capacity(bytes.len().saturating_mul(2));
    for byte in bytes {
        output.push(char::from(DIGITS[usize::from(byte >> 4)]));
        output.push(char::from(DIGITS[usize::from(byte & 0x0f)]));
    }
    output
}

/// # Errors
/// Returns [`RampError::Configuration`] when the value is not 32 hexadecimal bytes.
pub fn parse_hex32(value: &str) -> Result<[u8; 32], RampError> {
    decode_hex(value, 32)?
        .try_into()
        .map_err(|_| RampError::Configuration)
}

/// # Errors
/// Returns [`RampError::Configuration`] when the value is not even-length hexadecimal within `maximum` bytes.
pub fn decode_hex(value: &str, maximum: usize) -> Result<Vec<u8>, RampError> {
    let value = value.strip_prefix("0x").unwrap_or(value);
    if !value.len().is_multiple_of(2) || value.len() / 2 > maximum {
        return Err(RampError::Configuration);
    }
    value
        .as_bytes()
        .chunks_exact(2)
        .map(|pair| {
            let high = nibble(pair[0]).ok_or(RampError::Configuration)?;
            let low = nibble(pair[1]).ok_or(RampError::Configuration)?;
            Ok((high << 4) | low)
        })
        .collect()
}

const fn nibble(value: u8) -> Option<u8> {
    match value {
        b'0'..=b'9' => Some(value - b'0'),
        b'a'..=b'f' => Some(value - b'a' + 10),
        b'A'..=b'F' => Some(value - b'A' + 10),
        _ => None,
    }
}

const BASE64_ALPHABET: &[u8; 64] =
    b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";

fn base64_encode(bytes: &[u8]) -> String {
    let mut encoded = String::with_capacity(bytes.len().div_ceil(3).saturating_mul(4));
    for chunk in bytes.chunks(3) {
        let first = u32::from(chunk.first().copied().unwrap_or(0));
        let second = u32::from(chunk.get(1).copied().unwrap_or(0));
        let third = u32::from(chunk.get(2).copied().unwrap_or(0));
        let triple = (first << 16) | (second << 8) | third;
        encoded.push(char::from(
            BASE64_ALPHABET[usize::try_from((triple >> 18) & 63).unwrap_or(0)],
        ));
        encoded.push(char::from(
            BASE64_ALPHABET[usize::try_from((triple >> 12) & 63).unwrap_or(0)],
        ));
        encoded.push(if chunk.len() > 1 {
            char::from(BASE64_ALPHABET[usize::try_from((triple >> 6) & 63).unwrap_or(0)])
        } else {
            '='
        });
        encoded.push(if chunk.len() > 2 {
            char::from(BASE64_ALPHABET[usize::try_from(triple & 63).unwrap_or(0)])
        } else {
            '='
        });
    }
    encoded
}

fn base64_decode(encoded: &str) -> Result<Vec<u8>, RampError> {
    if encoded.is_empty() || !encoded.len().is_multiple_of(4) {
        return Err(RampError::Layerx);
    }
    let body = encoded.trim_end_matches('=');
    if encoded.len().saturating_sub(body.len()) > 2 {
        return Err(RampError::Layerx);
    }
    let mut decoded = Vec::with_capacity(body.len().saturating_mul(3) / 4);
    let mut accumulator = 0_u32;
    let mut bits = 0_u32;
    for byte in body.bytes() {
        let sextet = match byte {
            b'A'..=b'Z' => byte.wrapping_sub(b'A'),
            b'a'..=b'z' => byte.wrapping_sub(b'a').wrapping_add(26),
            b'0'..=b'9' => byte.wrapping_sub(b'0').wrapping_add(52),
            b'+' => 62,
            b'/' => 63,
            _ => return Err(RampError::Layerx),
        };
        accumulator = ((accumulator << 6) | u32::from(sextet)) & 0xffff;
        bits = bits.saturating_add(6);
        if bits >= 8 {
            bits = bits.saturating_sub(8);
            decoded.push(u8::try_from((accumulator >> bits) & 0xff).unwrap_or(0));
        }
    }
    if base64_encode(&decoded) != encoded {
        return Err(RampError::Layerx);
    }
    Ok(decoded)
}

#[cfg(unix)]
fn require_private(metadata: &fs::Metadata) -> Result<(), RampError> {
    use std::os::unix::fs::PermissionsExt as _;
    if metadata.permissions().mode() & 0o077 != 0 {
        return Err(RampError::Configuration);
    }
    Ok(())
}

#[cfg(not(unix))]
fn require_private(_metadata: &fs::Metadata) -> Result<(), RampError> {
    Ok(())
}

/// # Errors
/// Returns [`RampError::Provider`] when the callback cannot be encoded.
pub fn callback_evidence_digest(callback: &ProviderCallback) -> Result<[u8; 32], RampError> {
    let bytes = serde_json::to_vec(callback).map_err(|_| RampError::Provider)?;
    let mut hasher = Sha256::new();
    hasher.update(b"LXP/market-maker-ramp/provider-callback/v1\0");
    hasher.update(bytes);
    Ok(hasher.finalize().into())
}

/// Parses and validates independently configured sequencer trust inputs.
///
/// # Errors
/// Returns the invalid pin name for malformed keys, identities or bounds.
pub fn configured_sequencer(
    id: &str,
    key: &str,
    first: &str,
    last: &str,
) -> Result<SequencerAuthorization, &'static str> {
    let authorization = SequencerAuthorization::from_config(id, key, first, last)?;
    let bytes = authorization.public_key();
    let key =
        ed25519_dalek::VerifyingKey::from_bytes(&bytes).map_err(|_| "sequencer public key")?;
    let mut y = bytes;
    y[31] &= 0x7f;
    let mut prime = [0xff; 32];
    prime[0] = 0xed;
    prime[31] = 0x7f;
    if key.is_weak() || y.iter().rev().cmp(prime.iter().rev()) != std::cmp::Ordering::Less {
        return Err("sequencer public key");
    }
    Ok(authorization)
}

#[derive(serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MaintainedBatchDocument {
    header_hex: String,
    header_signature: String,
    receipt_proof_hex: String,
    batch_identity: MaintainedIdentityDocument,
}

#[derive(serde::Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
enum MaintainedIdentityDocument {
    OccupancyMaintenanceV2 {
        receipt_hex: String,
        receipt_proof_hex: String,
    },
}

impl MaintainedBatchDocument {
    /// Authenticates this selected maintained attachment under configured pins.
    ///
    /// # Errors
    /// Refuses malformed encodings, signatures, inclusion, identity or roots.
    pub fn authorize(
        &self,
        receipt: &[u8],
        facts: &layerx_proof::receipt::AuthorizedBatch,
        authorization: &SequencerAuthorization,
    ) -> Result<layerx_proof::receipt::AuthorizedBatch, &'static str> {
        fn bytes(text: &str) -> Result<Vec<u8>, &'static str> {
            if text.len() > 2_097_152
                || !text.len().is_multiple_of(2)
                || !text.bytes().all(|byte| byte.is_ascii_hexdigit())
            {
                return Err("maintained evidence encoding");
            }
            text.as_bytes()
                .chunks_exact(2)
                .map(|pair| {
                    let digits =
                        std::str::from_utf8(pair).map_err(|_| "maintained evidence encoding")?;
                    u8::from_str_radix(digits, 16).map_err(|_| "maintained evidence encoding")
                })
                .collect()
        }
        fn proof(text: &str) -> Result<layerx_proof::merkle::Proof, &'static str> {
            let encoded = bytes(text)?;
            let path = layerx_wire::receipt::decode_merkle_proof(&encoded)
                .map_err(|_| "maintained proof encoding")?;
            layerx_proof::merkle::Proof::new(
                path.leaf_index(),
                path.leaf_count(),
                path.siblings().to_vec(),
            )
            .map_err(|_| "maintained proof encoding")
        }
        let header = bytes(&self.header_hex)?;
        let signature: [u8; 64] = bytes(&self.header_signature)?
            .try_into()
            .map_err(|_| "maintained signature encoding")?;
        let activity_proof = proof(&self.receipt_proof_hex)?;
        let MaintainedIdentityDocument::OccupancyMaintenanceV2 {
            receipt_hex,
            receipt_proof_hex,
        } = &self.batch_identity;
        let maintenance = bytes(receipt_hex)?;
        let maintenance_proof = proof(receipt_proof_hex)?;
        layerx_proof::receipt::authorized_maintained_activity_batch(
            receipt,
            facts,
            &layerx_proof::receipt::MaintainedOutcomeEvidence {
                header: &header,
                header_signature: &signature,
                activity_proof: &activity_proof,
                maintenance: &maintenance,
                maintenance_proof: &maintenance_proof,
                authorization,
            },
        )
        .map_err(|_| "maintained evidence verification")
    }
}

fn present_maintained<'de, D: serde::Deserializer<'de>>(
    deserializer: D,
) -> Result<Option<MaintainedBatchDocument>, D::Error> {
    <MaintainedBatchDocument as serde::Deserialize>::deserialize(deserializer).map(Some)
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct AuthorityBody {
    activity_id: String,
    batch_id: String,
    asset: String,
    previous_state_root: String,
    resulting_state_root: String,
    sequencer_public_key: String,
    network_id: String,
    wire_version: String,
    #[serde(default, deserialize_with = "present_maintained")]
    batch_evidence: Option<MaintainedBatchDocument>,
}

impl AuthorityBody {
    fn validate_context(
        &self,
        activity_id: &str,
        network_id: u32,
        protocol_version: u16,
        sequencer_public_key: [u8; 32],
    ) -> Result<(), RampError> {
        if self.activity_id != activity_id
            || self.network_id != network_id.to_string()
            || self.wire_version != protocol_version.to_string()
            || parse_hex32(&self.sequencer_public_key)? != sequencer_public_key
        {
            return Err(RampError::Layerx);
        }
        Ok(())
    }
}

#[cfg(test)]
mod maintained_consumer_tests {
    use super::*;
    use layerx_proof::receipt::{verify_outcome, verify_program_state, AuthorizedBatch};
    use std::path::PathBuf;

    fn required<T, E: std::fmt::Debug>(value: Result<T, E>) -> T {
        value.unwrap_or_else(|error| panic!("{error:?}"))
    }
    fn bytes(value: &str) -> Vec<u8> {
        value
            .as_bytes()
            .chunks_exact(2)
            .map(|pair| required(u8::from_str_radix(required(std::str::from_utf8(pair)), 16)))
            .collect()
    }
    fn field(value: &serde_json::Value, name: &str) -> String {
        value[name]
            .as_str()
            .unwrap_or_else(|| panic!("missing {name}"))
            .to_owned()
    }
    fn pins(value: &serde_json::Value) -> SequencerAuthorization {
        required(SequencerAuthorization::from_config(
            &field(value, "sequencer_id"),
            &field(value, "sequencer_public_key"),
            &field(value, "first_batch"),
            &field(value, "last_batch"),
        ))
    }
    fn facts(value: &serde_json::Value) -> AuthorizedBatch {
        let fixed = |name| required(bytes(&field(value, name)).try_into());
        AuthorizedBatch::new(
            fixed("batch_id"),
            fixed("asset"),
            fixed("previous_state_root"),
            fixed("resulting_state_root"),
            fixed("sequencer_public_key"),
        )
    }
    fn captured() -> serde_json::Value {
        let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
        let path = root.join("../../hosted/gateway/tests/fixtures/maintained-authority.json");
        required(serde_json::from_slice(&required(std::fs::read(path))))
    }
    #[test]
    fn real_maintained_response_requires_independent_pins_and_exact_variant() {
        let capture = captured();
        let authority = &capture["authority"];
        let receipt = bytes(&field(&capture, "receipt_hex"));
        let document: MaintainedBatchDocument =
            required(serde_json::from_value(authority["batch_evidence"].clone()));
        let original = facts(authority);
        let authorization = pins(&capture);
        let selected = required(document.authorize(&receipt, &original, &authorization));
        assert!(verify_outcome(&receipt, &selected).is_ok());
        assert!(
            verify_outcome(&receipt, &original).is_err(),
            "maintained response must fail historical verification"
        );
        for name in [
            "sequencer_id",
            "sequencer_public_key",
            "first_batch",
            "last_batch",
        ] {
            let mut changed = capture.clone();
            changed[name] = match name {
                "first_batch" => serde_json::json!(u64::MAX.to_string()),
                "last_batch" => serde_json::json!("0"),
                _ => serde_json::json!("aa".repeat(32)),
            };
            let key = required(bytes(&field(&changed, "sequencer_public_key")).try_into());
            let id = required(bytes(&field(&changed, "sequencer_id")).try_into());
            let first = required(field(&changed, "first_batch").parse());
            let last = required(field(&changed, "last_batch").parse());
            assert!(
                document
                    .authorize(
                        &receipt,
                        &original,
                        &SequencerAuthorization::new(id, key, first, last)
                    )
                    .is_err(),
                "{name}"
            );
        }
        for name in [
            "batch_id",
            "asset",
            "previous_state_root",
            "resulting_state_root",
            "sequencer_public_key",
        ] {
            let mut changed = authority.clone();
            changed[name] = serde_json::json!("aa".repeat(32));
            let result = document.authorize(&receipt, &facts(&changed), &authorization);
            assert!(
                result.is_err()
                    || result.is_ok_and(|selected| verify_outcome(&receipt, &selected).is_err()),
                "{name}"
            );
        }
        for change in ["kind", "unknown", "null"] {
            let mut changed = authority["batch_evidence"].clone();
            match change {
                "kind" => changed["batch_identity"] = serde_json::json!({"kind": "historical"}),
                "unknown" => changed["unexpected"] = serde_json::json!(true),
                _ => changed = serde_json::Value::Null,
            }
            assert!(serde_json::from_value::<MaintainedBatchDocument>(changed).is_err());
        }
    }
    #[test]
    fn historical_document_cannot_enter_maintained_verification() {
        let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
        let historical: serde_json::Value =
            required(serde_json::from_slice(&required(std::fs::read(root.join(
                "../../hosted/authority/tests/fixtures/real-program-deploy-receipt.json",
            )))));
        let receipt = bytes(&field(&historical, "receipt_hex"));
        let header = required(layerx_wire::receipt::decode_batch_header(&bytes(&field(
            &historical,
            "header_hex",
        ))));
        let decoded = required(layerx_wire::receipt::decode(&receipt));
        let protocol = decoded.protocol().unwrap_or_else(|| panic!("protocol"));
        let historical_facts = AuthorizedBatch::new(
            protocol.batch_id(),
            protocol.asset(),
            header.previous_state_root(),
            header.resulting_state_root(),
            required(bytes(&field(&historical, "sequencer_public_key_hex")).try_into()),
        );
        assert!(verify_program_state(&receipt, &historical_facts).is_ok());
        let capture = captured();
        let document: MaintainedBatchDocument = required(serde_json::from_value(
            capture["authority"]["batch_evidence"].clone(),
        ));
        assert!(document
            .authorize(&receipt, &historical_facts, &pins(&capture))
            .is_err());
    }
}

#[cfg(test)]
mod authorization_config_tests {
    use super::configured_sequencer;
    #[test]
    fn mandatory_authorization_fields_refuse_missing_malformed_and_reversed_inputs() {
        let id = "11".repeat(32);
        let key = "d75a980182b10ab7d54bfed3c964073a0ee172f3daa62325af021a68f707511a";
        assert!(configured_sequencer(&id, key, "1", "1").is_ok());
        for fields in [
            ["", key, "1", "2"],
            [&id, "", "1", "2"],
            [&id, key, "", "2"],
            [&id, key, "1", ""],
            ["xyz", key, "1", "2"],
            [&id, "ff", "1", "2"],
            [&id, key, "2", "1"],
            [&id, key, "+1", "2"],
            [&id, key, "1", "18446744073709551616"],
        ] {
            assert!(configured_sequencer(fields[0], fields[1], fields[2], fields[3]).is_err());
        }
        assert!(configured_sequencer(&id, &"00".repeat(32), "1", "2").is_err());
        assert!(configured_sequencer(&id, &"ff".repeat(32), "1", "2").is_err());
    }
}

#[cfg(test)]
mod authority_shape_tests {
    use super::*;
    #[test]
    fn real_authority_shape_selects_attachment_without_null_or_unknown_fallback() {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../hosted/gateway/tests/fixtures/maintained-authority.json");
        let bytes = std::fs::read(path).unwrap_or_else(|error| panic!("{error}"));
        let capture: serde_json::Value =
            serde_json::from_slice(&bytes).unwrap_or_else(|error| panic!("{error}"));
        let document = capture["authority"].clone();
        let mut facts: AuthorityBody = serde_json::from_value(document.clone())
            .unwrap_or_else(|error| panic!("canonical authority: {error}"));
        let activity = facts.activity_id.clone();
        let network = facts
            .network_id
            .parse::<u32>()
            .unwrap_or_else(|error| panic!("network: {error}"));
        let protocol = facts
            .wire_version
            .parse::<u16>()
            .unwrap_or_else(|error| panic!("protocol: {error}"));
        let key =
            parse_hex32(&facts.sequencer_public_key).unwrap_or_else(|error| panic!("key: {error}"));
        assert!(facts
            .validate_context(&activity, network, protocol, key)
            .is_ok());
        facts.network_id = network
            .checked_add(1)
            .unwrap_or_else(|| panic!("network exhausted"))
            .to_string();
        assert!(facts
            .validate_context(&activity, network, protocol, key)
            .is_err());
        facts.network_id = network.to_string();
        assert!(facts
            .validate_context(&"00".repeat(32), network, protocol, key)
            .is_err());
        assert!(facts.validate_context(&activity, network, 0, key).is_err());
        assert!(facts
            .validate_context(&activity, network, protocol, [0; 32])
            .is_err());
        assert!(serde_json::from_value::<AuthorityBody>(document.clone()).is_ok());
        let mut historical = document.clone();
        historical
            .as_object_mut()
            .unwrap_or_else(|| panic!("object"))
            .remove("batch_evidence");
        assert!(serde_json::from_value::<AuthorityBody>(historical).is_ok());
        let mut null = document.clone();
        null["batch_evidence"] = serde_json::Value::Null;
        assert!(serde_json::from_value::<AuthorityBody>(null).is_err());
        let mut unknown = document;
        unknown["unexpected"] = serde_json::json!(true);
        assert!(serde_json::from_value::<AuthorityBody>(unknown).is_err());
    }
}
