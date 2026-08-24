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
    pub fn new(path: impl Into<PathBuf>) -> Result<Self, RampError> {
        let path = path.into();
        let metadata = fs::metadata(&path).map_err(|_| RampError::Configuration)?;
        if !metadata.is_file() || metadata.len() == 0 || metadata.len() > MAX_SECRET_BYTES as u64 {
            return Err(RampError::Configuration);
        }
        require_private(&metadata)?;
        Ok(Self(path))
    }

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

pub struct MutualTlsClient {
    connector: TlsConnector,
    timeout: Duration,
}

impl MutualTlsClient {
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

    pub fn json<T: Serialize>(
        &self,
        endpoint: &Endpoint,
        method: &str,
        path: &str,
        authorization: Option<&str>,
        idempotency: Option<&str>,
        contract: Option<&str>,
        body: Option<&T>,
    ) -> Result<HttpResponse, RampError> {
        let body = body
            .map(serde_json::to_vec)
            .transpose()
            .map_err(|_| RampError::Configuration)?
            .unwrap_or_default();
        self.request(
            endpoint,
            method,
            path,
            authorization,
            idempotency,
            contract,
            "application/json",
            &body,
        )
    }

    #[allow(clippy::too_many_arguments)]
    pub fn request(
        &self,
        endpoint: &Endpoint,
        method: &str,
        path: &str,
        authorization: Option<&str>,
        idempotency: Option<&str>,
        contract: Option<&str>,
        content_type: &str,
        body: &[u8],
    ) -> Result<HttpResponse, RampError> {
        if !matches!(method, "GET" | "POST")
            || !path.starts_with('/')
            || path.contains(['?', '#', '\\'])
            || path
                .split('/')
                .any(|segment| matches!(segment, "." | ".."))
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
        let stream = TcpStream::connect_timeout(&address, self.timeout)
            .map_err(|_| RampError::Provider)?;
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
            headers.push_str(&format!("Authorization: {value}\r\n"));
        }
        if let Some(value) = idempotency {
            headers.push_str(&format!("Idempotency-Key: {value}\r\n"));
        }
        if let Some(value) = contract {
            headers.push_str(&format!("LayerX-Ramp-Contract: {value}\r\n"));
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
        } else if name.eq_ignore_ascii_case("content-type") {
            if content_type.replace(value.trim()).is_some() {
                return Err(RampError::Provider);
            }
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
    Ok(HttpResponse {
        status,
        body,
    })
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
        && value.bytes().all(|byte| {
            byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.' | b':')
        })
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
        verify_detached(
            public_key,
            &canonical_compliance(self),
            &self.signature,
        )
        .map_err(|_| RampError::Compliance)
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
        let response = self.http.json(
            &self.endpoint,
            "POST",
            "/v1/introspect",
            Some(&format!("Bearer {}", self.service_token)),
            None,
            Some("layerx-identity-introspection-v1"),
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
    pub fn evaluate(&self, order: &RampOrder, now: u64) -> Result<ComplianceDecision, RampError> {
        #[derive(Serialize)]
        struct Request<'a> {
            contract: &'static str,
            order: &'a RampOrder,
        }
        let response = self.http.json(
            &self.endpoint,
            "POST",
            "/v1/decisions",
            Some(&format!("Bearer {}", self.service_token)),
            Some(&hex(&order.order_digest)),
            Some(COMPLIANCE_CONTRACT_VERSION),
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
                && self
                    .evidence_digest
                    .is_none_or(|digest| digest == [0; 32])
            || coded_state
                && self
                    .refusal_code
                    .as_deref()
                    .is_none_or(|code| !safe_segment(code))
            || !coded_state && self.refusal_code.is_some()
            || matches!(self.state, ProviderState::SubmittedUnknown | ProviderState::Pending)
                && self.retry_at.is_none_or(|retry| retry == 0)
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
        let response = self.http.json(
            &self.endpoint,
            "POST",
            &self.settlement_path,
            Some(&format!("Bearer {}", self.credential)),
            Some(&hex(&order.order_digest)),
            Some(PROVIDER_CONTRACT_VERSION),
            Some(&body),
        )?;
        self.decode(order, response)
    }

    pub fn reconcile(
        &self,
        order: &RampOrder,
        operation_id: &str,
    ) -> Result<ProviderResult, RampError> {
        if !safe_segment(operation_id) && !operation_id.starts_with("idempotency:") {
            return Err(RampError::Provider);
        }
        let path = operation_id.strip_prefix("idempotency:").map_or_else(
            || format!("{}/{}", self.status_path.trim_end_matches('/'), operation_id),
            |idempotency| {
                format!(
                    "{}/by-idempotency/{}",
                    self.status_path.trim_end_matches('/'),
                    idempotency
                )
            },
        );
        let response = self.http.json::<serde_json::Value>(
            &self.endpoint,
            "GET",
            &path,
            Some(&format!("Bearer {}", self.credential)),
            None,
            Some(PROVIDER_CONTRACT_VERSION),
            None,
        )?;
        self.decode(order, response)
    }

    fn decode(&self, order: &RampOrder, response: HttpResponse) -> Result<ProviderResult, RampError> {
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
            .map_err(|_| RampError::Provider)?;
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
            RampDirection::OffRamp => {
                compile_payer_grant_draw(order, account_sequence, registry)?
            }
        };
        let unsigned = self.unsigned(order, account_sequence, now, compiled)?;
        let canonical = encode_unsigned_envelope(&unsigned).map_err(|_| RampError::Layerx)?;
        let signature = self.sign(order, &canonical)?;
        let signed = unsigned.attach_signature(
            Signature::new(&signature).map_err(|_| RampError::Layerx)?,
        );
        let signed_bytes = encode_signed_envelope(&signed).map_err(|_| RampError::Layerx)?;
        let decoded = layerx_wire::activity::decode_signed(&signed_bytes, registry)
            .map_err(|_| RampError::Layerx)?;
        let identifier = activity_id(&decoded).map_err(|_| RampError::Layerx)?;
        Ok(PreparedLayerx {
            activity_id: identifier,
            canonical_activity: signed_bytes,
        })
    }

    pub fn submit_prepared(
        &self,
        order: &RampOrder,
        prepared: PreparedLayerx,
    ) -> Result<LayerxSubmission, RampError> {
        let identifier = prepared.activity_id;
        let signed_bytes = prepared.canonical_activity;
        let response = match self.http.request(
            &self.gateway,
            "POST",
            "/v1/activities",
            Some(&format!("LayerX-Key {}", self.gateway_key)),
            Some(&hex(&order.order_digest)),
            None,
            "application/octet-stream",
            &signed_bytes,
        ) {
            Ok(response) => response,
            Err(_) => {
                return Ok(LayerxSubmission::Unknown {
                    activity_id: identifier,
                    canonical_activity: Some(signed_bytes),
                });
            }
        };
        if response.status == 202 {
            return Ok(LayerxSubmission::Pending {
                activity_id: identifier,
                canonical_activity: Some(signed_bytes),
            });
        }
        if matches!(response.status, 400 | 401 | 403 | 404 | 422) {
            return Ok(LayerxSubmission::Refused {
                activity_id: identifier,
                canonical_activity: signed_bytes,
                code: format!("gateway_http_{}", response.status),
            });
        }
        if response.status != 200 {
            return Ok(LayerxSubmission::Unknown {
                activity_id: identifier,
                canonical_activity: Some(signed_bytes),
            });
        }
        match self.resolve(order, identifier) {
            Ok(LayerxSubmission::Unknown { activity_id, .. }) => Ok(LayerxSubmission::Unknown {
                activity_id,
                canonical_activity: Some(signed_bytes),
            }),
            Ok(LayerxSubmission::Pending { activity_id, .. }) => Ok(LayerxSubmission::Pending {
                activity_id,
                canonical_activity: Some(signed_bytes),
            }),
            Ok(LayerxSubmission::Verified { leg, .. }) => Ok(LayerxSubmission::Verified {
                leg,
                canonical_activity: Some(signed_bytes),
            }),
            Ok(LayerxSubmission::Refused { .. }) | Err(_) => Ok(LayerxSubmission::Unknown {
                activity_id: identifier,
                canonical_activity: Some(signed_bytes),
            }),
        }
    }

    pub fn resolve(
        &self,
        order: &RampOrder,
        activity: [u8; 32],
    ) -> Result<LayerxSubmission, RampError> {
        let id = hex(&activity);
        let response = self.http.json::<serde_json::Value>(
            &self.gateway,
            "GET",
            &format!("/v1/receipts/{id}"),
            Some(&format!("LayerX-Key {}", self.gateway_key)),
            None,
            None,
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
        let receipt: ResultBody =
            serde_json::from_slice(&response.body).map_err(|_| RampError::Layerx)?;
        if receipt.result.activity_id != id {
            return Err(RampError::Layerx);
        }
        let canonical_receipt = decode_hex(&receipt.result.receipt, 256 * 1024)?;
        let authority = self.http.json::<serde_json::Value>(
            &self.receipt_authority,
            "GET",
            &format!("/v1/authorized-batches/by-activity/{id}"),
            Some(&format!("Bearer {}", self.authority_token)),
            None,
            None,
            None,
        )?;
        if authority.status != 200 {
            return Err(RampError::Layerx);
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
        }
        let facts: AuthorityBody =
            serde_json::from_slice(&authority.body).map_err(|_| RampError::Layerx)?;
        if facts.activity_id != id {
            return Err(RampError::Layerx);
        }
        let evidence = ReceiptEvidence {
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
        let response = self.http.json(
            &self.signer,
            "POST",
            "/v1/signatures",
            Some(&format!("Bearer {}", self.signer_token)),
            None,
            None,
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
        let response = self.http.json(
            &self.endpoint,
            "POST",
            &self.broadcast_path,
            Some(&format!("Bearer {}", self.credential)),
            Some(&hex(&idempotency_key)),
            Some(PAXEER_CONTRACT_VERSION),
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
        self.decode(response, asset, amount, idempotency_key)
    }

    pub fn reconcile(
        &self,
        asset: [u8; 32],
        amount: u128,
        idempotency_key: [u8; 32],
    ) -> Result<PaxeerSubmission, RampError> {
        let response = self.http.json::<serde_json::Value>(
            &self.endpoint,
            "GET",
            &format!(
                "{}/by-idempotency/{}",
                self.status_path.trim_end_matches('/'),
                hex(&idempotency_key)
            ),
            Some(&format!("Bearer {}", self.credential)),
            None,
            Some(PAXEER_CONTRACT_VERSION),
            None,
        )?;
        self.decode(response, asset, amount, idempotency_key)
    }

    fn decode(
        &self,
        response: HttpResponse,
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
    output.extend_from_slice(&u128::from(value.len()).to_be_bytes());
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

pub fn parse_hex32(value: &str) -> Result<[u8; 32], RampError> {
    decode_hex(value, 32)?
        .try_into()
        .map_err(|_| RampError::Configuration)
}

pub fn decode_hex(value: &str, maximum: usize) -> Result<Vec<u8>, RampError> {
    let value = value.strip_prefix("0x").unwrap_or(value);
    if value.len() % 2 != 0 || value.len() / 2 > maximum {
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

#[must_use]
pub fn callback_evidence_digest(callback: &ProviderCallback) -> Result<[u8; 32], RampError> {
    let bytes = serde_json::to_vec(callback).map_err(|_| RampError::Provider)?;
    let mut hasher = Sha256::new();
    hasher.update(b"LXP/market-maker-ramp/provider-callback/v1\0");
    hasher.update(bytes);
    Ok(hasher.finalize().into())
}
