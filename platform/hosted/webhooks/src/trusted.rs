//! Canonical event-source and receipt-verification adapters.

use layerx_platform_gateway::{verify_activity_operation, AuthorityFacts, VerifiedOperation};
use native_tls::{Certificate, Identity};
use serde::Deserialize;
use std::collections::BTreeMap;
use std::env;
use std::fs;
use subtle::ConstantTimeEq;
use zeroize::{Zeroize, Zeroizing};

use crate::boundary::{Client, ClientIdentity, Endpoint};
use crate::encoding::{fixed_hex, hex_decode, hex_encode};
use crate::error::WebhookError;
use crate::events::{
    settled_payment, EventDraft, EventId, EventKind, PaymentDraft, Principal, ProtocolEvent,
    ProtocolFact, SubjectId, Verification,
};

const MAX_SOURCE_FACTS: usize = 32;

pub struct TrustedEvent(ProtocolEvent);

impl TrustedEvent {
    pub(crate) fn event(&self) -> &ProtocolEvent {
        &self.0
    }
}

struct Source {
    endpoint: Endpoint,
    token: Zeroizing<String>,
}

struct ReceiptVerifier {
    client: Client,
    component: Endpoint,
    component_token: Zeroizing<String>,
    authority: Endpoint,
    authority_token: Zeroizing<String>,
    trusted_sequencer_key: [u8; 32],
    network_id: String,
    wire_version: String,
}

pub struct TrustedSources {
    client: Client,
    sources: BTreeMap<EventKind, Source>,
    verifier: ReceiptVerifier,
}

pub struct DeveloperIdentity {
    client: Client,
    endpoint: Endpoint,
    token: Zeroizing<String>,
}

pub struct SourceTrigger {
    token: Zeroizing<String>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct SessionResponse {
    active: bool,
    sub: String,
    #[serde(default)]
    csrf_token: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct SourceRecord {
    id: String,
    principal: String,
    subject: String,
    subject_sequence: u64,
    occurred_at: u64,
    facts: Vec<SourceFact>,
    #[serde(default)]
    activity_id: Option<String>,
    #[serde(default)]
    amount: Option<String>,
    #[serde(default)]
    asset: Option<String>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct SourceFact {
    name: String,
    value: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ComponentReceipt {
    activity_id: String,
    receipt: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct AuthorityResponse {
    activity_id: String,
    batch_id: String,
    asset: String,
    previous_state_root: String,
    resulting_state_root: String,
    sequencer_public_key: String,
    network_id: String,
    wire_version: String,
}

impl TrustedSources {
    pub fn from_environment() -> Result<Self, String> {
        let ca = Certificate::from_der(
            &fs::read(
                env::var("LAYERX_WEBHOOKS_INTERNAL_CA_DER")
                    .map_err(|_| "LAYERX_WEBHOOKS_INTERNAL_CA_DER is required")?,
            )
            .map_err(|error| error.to_string())?,
        )
        .map_err(|error| error.to_string())?;
        let password = read_secret("LAYERX_WEBHOOKS_CLIENT_IDENTITY_PASSWORD_FILE")?;
        let identity = Identity::from_pkcs12(
            &fs::read(
                env::var("LAYERX_WEBHOOKS_CLIENT_IDENTITY_PKCS12")
                    .map_err(|_| "LAYERX_WEBHOOKS_CLIENT_IDENTITY_PKCS12 is required")?,
            )
            .map_err(|error| error.to_string())?,
            password.as_str(),
        )
        .map_err(|error| error.to_string())?;
        let client_identity = ClientIdentity::new(ca, Some(identity));
        let mut sources = BTreeMap::new();
        for (kind, stem) in [
            (EventKind::Journey, "JOURNEY"),
            (EventKind::Payment, "PAYMENT"),
            (EventKind::Approval, "APPROVAL"),
            (EventKind::Program, "PROGRAM"),
        ] {
            sources.insert(
                kind,
                Source {
                    endpoint: Endpoint::parse(
                        &env::var(format!("LAYERX_WEBHOOKS_{stem}_SOURCE_URL"))
                            .map_err(|_| format!("{stem} event source URL is required"))?,
                    )?,
                    token: read_secret(&format!("LAYERX_WEBHOOKS_{stem}_SOURCE_TOKEN_FILE"))?,
                },
            );
        }
        let trusted = read_secret("LAYERX_WEBHOOKS_SEQUENCER_PUBLIC_KEY_FILE")?;
        let wire_version = bounded_env("LAYERX_WEBHOOKS_LXP_WIRE_VERSION", 32)?;
        if wire_version.parse::<u16>().ok() != Some(layerx_wire::limits::STATE_COMMITMENT_PROTOCOL_VERSION) {
            return Err("webhook LXP wire version is not the current beta protocol".to_owned());
        }
        let verifier = ReceiptVerifier {
            client: Client::trusted(client_identity.clone()),
            component: Endpoint::parse(
                &env::var("LAYERX_WEBHOOKS_COMPONENT_URL")
                    .map_err(|_| "LAYERX_WEBHOOKS_COMPONENT_URL is required")?,
            )?,
            component_token: read_secret("LAYERX_WEBHOOKS_COMPONENT_TOKEN_FILE")?,
            authority: Endpoint::parse(
                &env::var("LAYERX_WEBHOOKS_AUTHORITY_URL")
                    .map_err(|_| "LAYERX_WEBHOOKS_AUTHORITY_URL is required")?,
            )?,
            authority_token: read_secret("LAYERX_WEBHOOKS_AUTHORITY_TOKEN_FILE")?,
            trusted_sequencer_key: fixed_hex::<32>(trusted.as_str())
                .map_err(|_| "trusted sequencer key is invalid".to_owned())?,
            network_id: bounded_env("LAYERX_WEBHOOKS_NETWORK_ID", 64)?,
            wire_version,
        };
        Ok(Self {
            client: Client::trusted(client_identity),
            sources,
            verifier,
        })
    }

    pub fn ready(&self) -> bool {
        self.verifier.ready()
            && self.sources.values().all(|source| {
                self.client
                    .request(
                        &source.endpoint,
                        "GET",
                        "/readyz",
                        Some(source.token.as_str()),
                        None,
                        &[],
                        &[],
                    )
                    .is_ok_and(|response| response.status == 200)
            })
    }

    pub fn fetch(
        &self,
        kind: EventKind,
        source_event_id: &str,
    ) -> Result<TrustedEvent, WebhookError> {
        if !valid_identifier(source_event_id, 128) {
            return Err(WebhookError::InvalidRequest);
        }
        let source = self
            .sources
            .get(&kind)
            .ok_or(WebhookError::InvalidRequest)?;
        let path = format!("/internal/v1/events/{source_event_id}");
        let response = self
            .client
            .request(
                &source.endpoint,
                "GET",
                &path,
                Some(source.token.as_str()),
                None,
                &[],
                &[],
            )
            .map_err(|_| WebhookError::Unavailable)?;
        if response.status != 200 || !response.content_type.starts_with("application/json") {
            return Err(WebhookError::Unavailable);
        }
        let record: SourceRecord =
            serde_json::from_slice(&response.body).map_err(|_| WebhookError::Unavailable)?;
        if record.id != source_event_id || record.facts.len() > MAX_SOURCE_FACTS {
            return Err(WebhookError::InvalidRequest);
        }
        self.event(kind, record).map(TrustedEvent)
    }

    fn event(&self, kind: EventKind, record: SourceRecord) -> Result<ProtocolEvent, WebhookError> {
        let id = EventId::new(record.id)?;
        let principal = Principal::new(record.principal)?;
        let subject = SubjectId::new(record.subject)?;
        let operation = record
            .activity_id
            .as_deref()
            .map(|activity| self.verifier.verify(activity))
            .transpose()?;
        if kind == EventKind::Payment {
            let operation = operation
                .as_ref()
                .ok_or(WebhookError::VerificationRequired)?;
            let amount = record.amount.ok_or(WebhookError::InvalidRequest)?;
            let asset = record.asset.ok_or(WebhookError::InvalidRequest)?;
            return settled_payment(PaymentDraft {
                id,
                principal,
                subject,
                subject_sequence: record.subject_sequence,
                occurred_at: record.occurred_at,
                operation,
                amount,
                asset,
            });
        }
        let mut facts = Vec::with_capacity(record.facts.len().saturating_add(2));
        for fact in record.facts {
            facts.push(ProtocolFact::unverified(fact.name, fact.value)?);
        }
        if let Some(operation) = operation.as_ref() {
            let verification = Verification::parse(operation.verification_level())?;
            let receipt = hex_encode(&operation.receipt_digest());
            facts.push(ProtocolFact::verified(
                "activity_id",
                hex_encode(&operation.activity_id()),
                verification,
                receipt.as_str(),
            )?);
            facts.push(ProtocolFact::verified(
                "result_code",
                operation.result_code().to_string(),
                verification,
                receipt,
            )?);
        }
        ProtocolEvent::new(EventDraft {
            id,
            kind,
            principal,
            subject,
            subject_sequence: record.subject_sequence,
            occurred_at: record.occurred_at,
            facts,
        })
    }
}

impl DeveloperIdentity {
    pub fn from_environment() -> Result<Self, String> {
        let ca = Certificate::from_der(
            &fs::read(
                env::var("LAYERX_WEBHOOKS_INTERNAL_CA_DER")
                    .map_err(|_| "LAYERX_WEBHOOKS_INTERNAL_CA_DER is required")?,
            )
            .map_err(|error| error.to_string())?,
        )
        .map_err(|error| error.to_string())?;
        let password = read_secret("LAYERX_WEBHOOKS_CLIENT_IDENTITY_PASSWORD_FILE")?;
        let identity = Identity::from_pkcs12(
            &fs::read(
                env::var("LAYERX_WEBHOOKS_CLIENT_IDENTITY_PKCS12")
                    .map_err(|_| "LAYERX_WEBHOOKS_CLIENT_IDENTITY_PKCS12 is required")?,
            )
            .map_err(|error| error.to_string())?,
            password.as_str(),
        )
        .map_err(|error| error.to_string())?;
        Ok(Self {
            client: Client::trusted(ClientIdentity::new(ca, Some(identity))),
            endpoint: Endpoint::parse(
                &env::var("LAYERX_WEBHOOKS_IDENTITY_URL")
                    .map_err(|_| "LAYERX_WEBHOOKS_IDENTITY_URL is required")?,
            )?,
            token: read_secret("LAYERX_WEBHOOKS_IDENTITY_TOKEN_FILE")?,
        })
    }

    pub fn from_dashboard_environment() -> Result<Self, String> {
        let ca = Certificate::from_der(
            &fs::read(
                env::var("LAYERX_DASHBOARD_INTERNAL_CA_DER")
                    .map_err(|_| "LAYERX_DASHBOARD_INTERNAL_CA_DER is required")?,
            )
            .map_err(|error| error.to_string())?,
        )
        .map_err(|error| error.to_string())?;
        let password = read_secret("LAYERX_DASHBOARD_CLIENT_IDENTITY_PASSWORD_FILE")?;
        let identity = Identity::from_pkcs12(
            &fs::read(
                env::var("LAYERX_DASHBOARD_CLIENT_IDENTITY_PKCS12")
                    .map_err(|_| "LAYERX_DASHBOARD_CLIENT_IDENTITY_PKCS12 is required")?,
            )
            .map_err(|error| error.to_string())?,
            password.as_str(),
        )
        .map_err(|error| error.to_string())?;
        Ok(Self {
            client: Client::trusted(ClientIdentity::new(ca, Some(identity))),
            endpoint: Endpoint::parse(
                &env::var("LAYERX_DASHBOARD_IDENTITY_URL")
                    .map_err(|_| "LAYERX_DASHBOARD_IDENTITY_URL is required")?,
            )?,
            token: read_secret("LAYERX_DASHBOARD_IDENTITY_TOKEN_FILE")?,
        })
    }

    pub fn authenticate(
        &self,
        authorization: Option<&str>,
        cookie: Option<&str>,
        anti_forgery: Option<&str>,
        mutation: bool,
    ) -> Result<Principal, WebhookError> {
        let (session, cookie_auth) = match authorization {
            Some(value) => (
                value
                    .strip_prefix("Bearer ")
                    .filter(|token| !token.is_empty() && token.len() <= 4096)
                    .ok_or(WebhookError::InvalidRequest)?,
                false,
            ),
            None => (
                session_cookie(cookie.ok_or(WebhookError::InvalidRequest)?)?,
                true,
            ),
        };
        let body = Zeroizing::new(
            serde_json::to_vec(&serde_json::json!({ "token": session }))
                .map_err(|_| WebhookError::Unavailable)?,
        );
        let response = self
            .client
            .request(
                &self.endpoint,
                "POST",
                "/v1/sessions/introspect",
                Some(self.token.as_str()),
                None,
                &[],
                &body,
            )
            .map_err(|_| WebhookError::Unavailable)?;
        if response.status != 200 || !response.content_type.starts_with("application/json") {
            return Err(WebhookError::InvalidRequest);
        }
        let session: SessionResponse =
            serde_json::from_slice(&response.body).map_err(|_| WebhookError::Unavailable)?;
        if !session.active {
            return Err(WebhookError::InvalidRequest);
        }
        if cookie_auth && mutation {
            let presented = anti_forgery.ok_or(WebhookError::InvalidRequest)?;
            if session.csrf_token.is_empty()
                || session.csrf_token.len() > 256
                || session
                    .csrf_token
                    .as_bytes()
                    .ct_eq(presented.as_bytes())
                    .unwrap_u8()
                    != 1
            {
                return Err(WebhookError::InvalidRequest);
            }
        }
        Principal::new(session.sub)
    }
}

impl SourceTrigger {
    pub fn from_environment() -> Result<Self, String> {
        Ok(Self {
            token: read_secret("LAYERX_WEBHOOKS_SOURCE_TRIGGER_TOKEN_FILE")?,
        })
    }

    pub fn operator_from_environment() -> Result<Self, String> {
        Ok(Self {
            token: read_secret("LAYERX_WEBHOOKS_OPERATOR_TOKEN_FILE")?,
        })
    }

    pub fn authorizes(&self, authorization: Option<&str>) -> bool {
        authorization
            .and_then(|value| value.strip_prefix("Bearer "))
            .is_some_and(|value| self.token.as_bytes().ct_eq(value.as_bytes()).unwrap_u8() == 1)
    }
}

impl ReceiptVerifier {
    fn ready(&self) -> bool {
        [
            (&self.component, self.component_token.as_str()),
            (&self.authority, self.authority_token.as_str()),
        ]
        .into_iter()
        .all(|(endpoint, token)| {
            self.client
                .request(endpoint, "GET", "/readyz", Some(token), None, &[], &[])
                .is_ok_and(|response| response.status == 200)
        })
    }

    fn verify(&self, activity: &str) -> Result<VerifiedOperation, WebhookError> {
        let expected = fixed_hex::<32>(activity)?;
        let receipt_response = self
            .client
            .request(
                &self.component,
                "GET",
                &format!("/internal/v1/receipts/{activity}"),
                Some(self.component_token.as_str()),
                None,
                &[],
                &[],
            )
            .map_err(|_| WebhookError::Unavailable)?;
        let authority_response = self
            .client
            .request(
                &self.authority,
                "GET",
                &format!("/internal/v1/activities/{activity}/authority"),
                Some(self.authority_token.as_str()),
                None,
                &[],
                &[],
            )
            .map_err(|_| WebhookError::Unavailable)?;
        if receipt_response.status != 200
            || authority_response.status != 200
            || !receipt_response
                .content_type
                .starts_with("application/json")
            || !authority_response
                .content_type
                .starts_with("application/json")
        {
            return Err(WebhookError::VerificationRequired);
        }
        let receipt: ComponentReceipt = serde_json::from_slice(&receipt_response.body)
            .map_err(|_| WebhookError::VerificationRequired)?;
        let authority: AuthorityResponse = serde_json::from_slice(&authority_response.body)
            .map_err(|_| WebhookError::VerificationRequired)?;
        if receipt.activity_id != activity
            || authority.activity_id != activity
            || authority.network_id != self.network_id
            || authority.wire_version != self.wire_version
        {
            return Err(WebhookError::VerificationRequired);
        }
        let sequencer = fixed_hex::<32>(&authority.sequencer_public_key)?;
        if sequencer != self.trusted_sequencer_key {
            return Err(WebhookError::VerificationRequired);
        }
        verify_activity_operation(
            &hex_decode(&receipt.receipt)?,
            AuthorityFacts::new(
                fixed_hex(&authority.batch_id)?,
                fixed_hex(&authority.asset)?,
                fixed_hex(&authority.previous_state_root)?,
                fixed_hex(&authority.resulting_state_root)?,
                sequencer,
            ),
            &self.trusted_sequencer_key,
            Some(expected),
        )
        .map_err(WebhookError::from)
    }
}

fn read_secret(name: &str) -> Result<Zeroizing<String>, String> {
    let path = env::var(name).map_err(|_| format!("{name} is required"))?;
    let mut value = fs::read_to_string(path).map_err(|error| error.to_string())?;
    while matches!(value.as_bytes().last(), Some(b'\r' | b'\n')) {
        value.pop();
    }
    if value.is_empty() || value.len() > 4096 {
        value.zeroize();
        return Err(format!("{name} does not contain a bounded secret"));
    }
    Ok(Zeroizing::new(value))
}

fn bounded_env(name: &str, maximum: usize) -> Result<String, String> {
    let value = env::var(name).map_err(|_| format!("{name} is required"))?;
    if !valid_identifier(&value, maximum) {
        return Err(format!("{name} is invalid"));
    }
    Ok(value)
}

fn valid_identifier(value: &str, maximum: usize) -> bool {
    !value.is_empty()
        && value.len() <= maximum
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.'))
}

fn session_cookie(header: &str) -> Result<&str, WebhookError> {
    let mut selected = None;
    for part in header.split(';') {
        let (name, value) = part
            .trim()
            .split_once('=')
            .ok_or(WebhookError::InvalidRequest)?;
        if name == "__Host-layerx-session" {
            if selected.is_some() || value.is_empty() || value.len() > 4096 {
                return Err(WebhookError::InvalidRequest);
            }
            selected = Some(value);
        }
    }
    selected.ok_or(WebhookError::InvalidRequest)
}
