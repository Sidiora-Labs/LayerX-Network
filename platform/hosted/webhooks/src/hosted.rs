//! Multi-instance hosted webhook state and delivery engine.

use ed25519_dalek::{Signature, Verifier as _, VerifyingKey};
use native_tls::{Certificate, Identity, TlsConnector};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, VecDeque};
use std::env;
use std::fs;
use std::io::{Read, Write};
use std::net::{TcpStream, ToSocketAddrs};
use std::time::{Duration, Instant};
use subtle::ConstantTimeEq;
use zeroize::{Zeroize, Zeroizing};

use crate::boundary::{Client, ClientIdentity, Endpoint};
use crate::deliveries::{AttemptRecord, DeliveryRecord, DeliveryState, FailureKind};
use crate::encoding::{base64_decode, base64_encode, digest, hex_encode};
use crate::endpoints::{EndpointHealth, RetryPolicy};
use crate::error::WebhookError;
use crate::events::{DeliveryId, EndpointId, EventKind, Principal, ProtocolEvent, Verification};
use crate::scheme;
use crate::trusted::TrustedEvent;

const MAX_ENDPOINTS: usize = 32;
const MAX_EVENTS: usize = 20_000;
const MAX_ATTEMPTS: usize = 32;
const MAX_SHARD_BYTES: usize = 16 * 1024 * 1024;
const CAS_ATTEMPTS: usize = 16;
const REDIS_CONNECT_TIMEOUT: Duration = Duration::from_secs(3);
const REDIS_IO_TIMEOUT: Duration = Duration::from_secs(8);
const MAX_REDIS_RESPONSE: usize = 20 * 1024 * 1024;
const CURSOR_PREFIX: &str = "lxwc2_";

#[derive(Clone, Debug, Serialize, Deserialize)]
struct SigningKeyRef {
    id: String,
    handle: String,
    public_key: [u8; 32],
}

#[derive(Clone, Debug, Serialize, Deserialize)]
struct StoredEndpoint {
    id: EndpointId,
    url: String,
    kinds: Vec<EventKind>,
    minimum_verification: Verification,
    key: SigningKeyRef,
    pending_key: Option<SigningKeyRef>,
    pending_key_scope: Option<String>,
    pending_key_activates_at: u64,
    last_key_activates_at: u64,
    created_at: u64,
    registration_scope: String,
    key_rotated_at: u64,
    suspended: bool,
    suspended_reason: Option<String>,
    consecutive_dead_letters: u32,
    delivered_total: u64,
    dead_lettered_total: u64,
    last_delivery_at: Option<u64>,
    last_failure: Option<String>,
    last_failure_at: Option<u64>,
}

impl StoredEndpoint {
    fn accepts(&self, event: &ProtocolEvent) -> bool {
        (self.kinds.is_empty() || self.kinds.contains(&event.kind()))
            && event.verification().at_least(self.minimum_verification)
    }

    fn promote(&mut self, now: u64) {
        if now >= self.pending_key_activates_at {
            if let Some(pending) = self.pending_key.take() {
                self.key = pending;
                self.pending_key_activates_at = 0;
            }
        }
    }

    fn signing_key(&self, now: u64) -> &SigningKeyRef {
        self.pending_key
            .as_ref()
            .filter(|_| now >= self.pending_key_activates_at)
            .unwrap_or(&self.key)
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "state", rename_all = "kebab-case")]
enum StoredDeliveryState {
    Pending,
    InFlight {
        attempt: u32,
        started_at: u64,
        lease: String,
    },
    Retrying {
        attempt: u32,
        next_attempt_at: u64,
        failure: FailureKind,
        status: Option<u16>,
    },
    Delivered {
        attempt: u32,
        at: u64,
        status: u16,
    },
    DeadLettered {
        attempts: u32,
        at: u64,
        failure: FailureKind,
        status: Option<u16>,
    },
}

impl StoredDeliveryState {
    fn attempts(&self) -> u32 {
        match self {
            Self::Pending => 0,
            Self::InFlight { attempt, .. }
            | Self::Retrying { attempt, .. }
            | Self::Delivered { attempt, .. } => *attempt,
            Self::DeadLettered { attempts, .. } => *attempts,
        }
    }

    fn due(&self, created_at: u64, timeout: u64) -> Option<u64> {
        match self {
            Self::Pending => Some(created_at),
            Self::InFlight { started_at, .. } => Some(started_at.saturating_add(timeout)),
            Self::Retrying {
                next_attempt_at, ..
            } => Some(*next_attempt_at),
            Self::Delivered { .. } | Self::DeadLettered { .. } => None,
        }
    }

    fn public(&self) -> DeliveryState {
        match self {
            Self::Pending => DeliveryState::Pending,
            Self::InFlight {
                attempt,
                started_at,
                ..
            } => DeliveryState::InFlight {
                attempt: *attempt,
                started_at: *started_at,
            },
            Self::Retrying {
                attempt,
                next_attempt_at,
                failure,
                status,
            } => DeliveryState::Retrying {
                attempt: *attempt,
                next_attempt_at: *next_attempt_at,
                failure: *failure,
                status: *status,
            },
            Self::Delivered {
                attempt,
                at,
                status,
            } => DeliveryState::Delivered {
                attempt: *attempt,
                at: *at,
                status: *status,
            },
            Self::DeadLettered {
                attempts,
                at,
                failure,
                status,
            } => DeliveryState::DeadLettered {
                attempts: *attempts,
                at: *at,
                failure: *failure,
                status: *status,
            },
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
struct StoredDelivery {
    id: DeliveryId,
    endpoint: EndpointId,
    event: String,
    subject: String,
    subject_sequence: u64,
    log_position: u64,
    created_at: u64,
    state: StoredDeliveryState,
    attempts: Vec<AttemptRecord>,
    replay_of: Option<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
struct PrincipalShard {
    principal: Principal,
    cursor_floor: u64,
    next_position: u64,
    endpoints: BTreeMap<String, StoredEndpoint>,
    events: BTreeMap<u64, ProtocolEvent>,
    positions: BTreeMap<String, u64>,
    deliveries: BTreeMap<String, StoredDelivery>,
    queues: BTreeMap<String, VecDeque<String>>,
    dead_letters: Vec<String>,
    high_water: BTreeMap<String, u64>,
}

impl PrincipalShard {
    fn empty(principal: Principal) -> Self {
        Self {
            principal,
            cursor_floor: 1,
            next_position: 1,
            endpoints: BTreeMap::new(),
            events: BTreeMap::new(),
            positions: BTreeMap::new(),
            deliveries: BTreeMap::new(),
            queues: BTreeMap::new(),
            dead_letters: Vec::new(),
            high_water: BTreeMap::new(),
        }
    }
}

#[derive(Clone, Debug, Serialize)]
pub struct HostedRegistration {
    pub endpoint: String,
    pub key_id: String,
    pub public_key: String,
    pub public_keys_json: String,
    pub activates_at: Option<u64>,
    pub receiver_obligation: String,
}

impl HostedRegistration {
    fn from_key(endpoint: &EndpointId, key: &SigningKeyRef, activates_at: Option<u64>) -> Self {
        let public_key = base64_encode(&key.public_key);
        Self {
            endpoint: endpoint.as_str().to_owned(),
            key_id: key.id.clone(),
            public_keys_json: format!("{{\"{}\":\"{}\"}}", key.id, public_key),
            public_key,
            activates_at,
            receiver_obligation: scheme::RECEIVER_OBLIGATION.to_owned(),
        }
    }
}

#[derive(Clone, Debug, Serialize)]
pub struct HostedPublishOutcome {
    pub position: u64,
    pub duplicate: bool,
    pub queued: Vec<String>,
}

#[derive(Clone, Debug, Default, Serialize)]
pub struct HostedDispatchReport {
    pub attempted: u32,
    pub delivered: u32,
    pub retrying: u32,
    pub dead_lettered: u32,
    pub blocked: u32,
}

#[derive(Clone, Debug, Serialize)]
pub struct HostedSnapshot {
    pub endpoints: Vec<EndpointHealth>,
    pub events: Vec<ProtocolEvent>,
    pub deliveries: Vec<DeliveryRecord>,
    pub dead_letters: Vec<DeliveryRecord>,
    pub cursor_floor: u64,
    pub last_position: u64,
}

#[derive(Clone, Debug, Serialize)]
pub struct HostedEventPage {
    pub events: Vec<ProtocolEvent>,
    pub next_cursor: String,
    pub has_more: bool,
}

#[derive(Clone, Debug, Serialize)]
pub struct HostedRedeliveryOutcome {
    pub queued: Vec<String>,
    pub next_cursor: String,
    pub has_more: bool,
}

#[derive(Clone)]
struct RedisEndpoint {
    host: String,
    port: u16,
}

impl RedisEndpoint {
    fn parse(value: &str) -> Result<Self, String> {
        let authority = value
            .strip_prefix("rediss://")
            .ok_or_else(|| "webhook Redis endpoint must use rediss".to_owned())?
            .trim_end_matches('/');
        if authority.is_empty() || authority.contains(['@', '/', '?', '#', '\\']) {
            return Err("webhook Redis endpoint is not canonical".to_owned());
        }
        let (host, port) = authority.rsplit_once(':').map_or_else(
            || Ok::<_, String>((authority.to_owned(), 6379)),
            |(host, port)| {
                Ok((
                    host.to_owned(),
                    port.parse::<u16>()
                        .map_err(|_| "webhook Redis port is invalid".to_owned())?,
                ))
            },
        )?;
        if host.is_empty() {
            return Err("webhook Redis host is missing".to_owned());
        }
        Ok(Self { host, port })
    }
}

enum Resp {
    Simple(String),
    Bulk(Option<Vec<u8>>),
    Integer(i64),
    Array(Vec<Resp>),
}

struct RedisRepository {
    endpoint: RedisEndpoint,
    ca: Certificate,
    username: Zeroizing<String>,
    password: Zeroizing<String>,
}

impl RedisRepository {
    fn ready(&self) -> bool {
        matches!(self.command(&["PING"]), Ok(Resp::Simple(value)) if value == "PONG")
    }

    fn load(&self, principal: &Principal) -> Result<(u64, PrincipalShard), WebhookError> {
        let key = shard_key(principal);
        let response = self
            .command(&["HMGET", &key, "revision", "json"])
            .map_err(|_| WebhookError::Unavailable)?;
        let Resp::Array(values) = response else {
            return Err(WebhookError::CorruptStore);
        };
        if values.len() != 2 {
            return Err(WebhookError::CorruptStore);
        }
        let revision = resp_text(&values[0])
            .map(|value| value.parse::<u64>())
            .transpose()
            .map_err(|_| WebhookError::CorruptStore)?
            .unwrap_or(0);
        let shard = match resp_bytes(&values[1]) {
            None => PrincipalShard::empty(principal.clone()),
            Some(bytes) => serde_json::from_slice::<PrincipalShard>(bytes)
                .map_err(|_| WebhookError::CorruptStore)?,
        };
        if shard.principal != *principal {
            return Err(WebhookError::CorruptStore);
        }
        Ok((revision, shard))
    }

    fn compare_and_set(
        &self,
        principal: &Principal,
        expected: u64,
        shard: &PrincipalShard,
    ) -> Result<bool, WebhookError> {
        let bytes = serde_json::to_vec(shard).map_err(|_| WebhookError::CorruptStore)?;
        if bytes.len() > MAX_SHARD_BYTES {
            return Err(WebhookError::Unavailable);
        }
        let json = std::str::from_utf8(&bytes).map_err(|_| WebhookError::CorruptStore)?;
        let digest = principal_digest(principal);
        let response = self
            .command(&[
                "EVAL",
                CAS_SCRIPT,
                "2",
                &shard_key(principal),
                "webhooks:principals",
                &expected.to_string(),
                json,
                &digest,
            ])
            .map_err(|_| WebhookError::Unavailable)?;
        Ok(matches!(response, Resp::Integer(value) if value == 1))
    }

    fn principals(&self) -> Result<Vec<Principal>, WebhookError> {
        let response = self
            .command(&["SMEMBERS", "webhooks:principals"])
            .map_err(|_| WebhookError::Unavailable)?;
        let Resp::Array(values) = response else {
            return Err(WebhookError::CorruptStore);
        };
        if values.len() > 100_000 {
            return Err(WebhookError::CorruptStore);
        }
        let mut principals = Vec::with_capacity(values.len());
        for digest in values.iter().filter_map(resp_text) {
            let state_key = format!("webhooks:principal:{digest}");
            let response = self
                .command(&["HGET", &state_key, "json"])
                .map_err(|_| WebhookError::Unavailable)?;
            let bytes = resp_bytes(&response).ok_or(WebhookError::CorruptStore)?;
            let shard: PrincipalShard =
                serde_json::from_slice(bytes).map_err(|_| WebhookError::CorruptStore)?;
            if principal_digest(&shard.principal) != digest {
                return Err(WebhookError::CorruptStore);
            }
            principals.push(shard.principal);
        }
        Ok(principals)
    }

    fn command(&self, arguments: &[&str]) -> Result<Resp, String> {
        let mut last = None;
        for address in (self.endpoint.host.as_str(), self.endpoint.port)
            .to_socket_addrs()
            .map_err(|error| error.to_string())?
            .take(8)
        {
            match TcpStream::connect_timeout(&address, REDIS_CONNECT_TIMEOUT) {
                Ok(tcp) => {
                    tcp.set_read_timeout(Some(REDIS_IO_TIMEOUT))
                        .map_err(|error| error.to_string())?;
                    tcp.set_write_timeout(Some(REDIS_IO_TIMEOUT))
                        .map_err(|error| error.to_string())?;
                    let connector = TlsConnector::builder()
                        .add_root_certificate(self.ca.clone())
                        .min_protocol_version(Some(native_tls::Protocol::Tlsv12))
                        .build()
                        .map_err(|error| error.to_string())?;
                    let mut stream = connector
                        .connect(&self.endpoint.host, tcp)
                        .map_err(|error| error.to_string())?;
                    write_resp(
                        &mut stream,
                        &["AUTH", self.username.as_str(), self.password.as_str()],
                    )?;
                    match read_resp(&mut stream, 0)? {
                        Resp::Simple(value) if value == "OK" => {}
                        _ => return Err("webhook Redis authentication failed".to_owned()),
                    }
                    write_resp(&mut stream, arguments)?;
                    return read_resp(&mut stream, 0);
                }
                Err(error) => last = Some(error),
            }
        }
        Err(last.map_or_else(
            || "webhook Redis did not resolve".to_owned(),
            |error| error.to_string(),
        ))
    }
}

struct KmsClient {
    client: Client,
    endpoint: Endpoint,
    token: Zeroizing<String>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct KmsKeyResponse {
    key_id: String,
    handle: String,
    public_key: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct KmsSignatureResponse {
    signature: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct KmsReadiness {
    ready: bool,
    ed25519_non_exportable: bool,
}

impl KmsClient {
    fn create_key(&self, idempotency: &str) -> Result<SigningKeyRef, WebhookError> {
        let body = serde_json::to_vec(&serde_json::json!({
            "algorithm": "ed25519",
            "purpose": "layerx-webhook-v1"
        }))
        .map_err(|_| WebhookError::Unavailable)?;
        let response = self
            .client
            .request(
                &self.endpoint,
                "POST",
                "/v1/signing-keys",
                Some(self.token.as_str()),
                Some(idempotency),
                &[],
                &body,
            )
            .map_err(|_| WebhookError::Unavailable)?;
        if !matches!(response.status, 200 | 201)
            || !response.content_type.starts_with("application/json")
        {
            return Err(WebhookError::Unavailable);
        }
        let key: KmsKeyResponse =
            serde_json::from_slice(&response.body).map_err(|_| WebhookError::Unavailable)?;
        let public = base64_decode(&key.public_key)?;
        let public_key: [u8; 32] = public.try_into().map_err(|_| WebhookError::Unavailable)?;
        if !scheme::valid_key_id(&key.key_id)
            || !key.key_id.starts_with(scheme::KEY_PREFIX)
            || key.handle.is_empty()
            || key.handle.len() > 512
            || key.handle.contains(['\0', '\r', '\n'])
        {
            return Err(WebhookError::Unavailable);
        }
        Ok(SigningKeyRef {
            id: key.key_id,
            handle: key.handle,
            public_key,
        })
    }

    fn sign(&self, key: &SigningKeyRef, message: &[u8]) -> Result<[u8; 64], WebhookError> {
        let body = serde_json::to_vec(&serde_json::json!({
            "key_handle": key.handle,
            "algorithm": "ed25519",
            "message": base64_encode(message)
        }))
        .map_err(|_| WebhookError::Unavailable)?;
        let response = self
            .client
            .request(
                &self.endpoint,
                "POST",
                "/v1/signatures",
                Some(self.token.as_str()),
                None,
                &[],
                &body,
            )
            .map_err(|_| WebhookError::Unavailable)?;
        if response.status != 200 || !response.content_type.starts_with("application/json") {
            return Err(WebhookError::Unavailable);
        }
        let signed: KmsSignatureResponse =
            serde_json::from_slice(&response.body).map_err(|_| WebhookError::Unavailable)?;
        let signature: [u8; 64] = base64_decode(&signed.signature)?
            .try_into()
            .map_err(|_| WebhookError::Unavailable)?;
        let verifier =
            VerifyingKey::from_bytes(&key.public_key).map_err(|_| WebhookError::Unavailable)?;
        verifier
            .verify(message, &Signature::from_bytes(&signature))
            .map_err(|_| WebhookError::Unavailable)?;
        Ok(signature)
    }

    fn ready(&self) -> bool {
        self.client
            .request(
                &self.endpoint,
                "GET",
                "/readyz",
                Some(self.token.as_str()),
                None,
                &[],
                &[],
            )
            .ok()
            .filter(|response| {
                response.status == 200 && response.content_type.starts_with("application/json")
            })
            .and_then(|response| serde_json::from_slice::<KmsReadiness>(&response.body).ok())
            .is_some_and(|status| status.ready && status.ed25519_non_exportable)
    }
}

struct Prepared {
    principal: Principal,
    delivery: String,
    endpoint: String,
    url: String,
    event: String,
    kind: EventKind,
    subject: String,
    sequence: u64,
    attempt: u32,
    lease: String,
    key: SigningKeyRef,
    payload: Vec<u8>,
    timestamp: u64,
}

#[derive(Serialize)]
struct Envelope<'a> {
    scheme: &'a str,
    endpoint_id: &'a str,
    receiver_obligation: &'a str,
    event: &'a ProtocolEvent,
}

pub struct HostedService {
    repository: RedisRepository,
    kms: KmsClient,
    outbound: Client,
    outbound_ca: ClientIdentity,
    policy: RetryPolicy,
    instance: String,
    cursor_key: Zeroizing<[u8; 32]>,
    key_overlap_seconds: u64,
}

pub struct HostedReader {
    repository: RedisRepository,
    policy: RetryPolicy,
}

impl HostedReader {
    pub fn from_environment() -> Result<Self, String> {
        let ca = Certificate::from_der(&read_required("LAYERX_WEBHOOKS_INTERNAL_CA_DER")?)
            .map_err(|error| error.to_string())?;
        let policy = RetryPolicy {
            base_delay_seconds: number("LAYERX_WEBHOOKS_BASE_DELAY_SECONDS", 10)?,
            maximum_delay_seconds: number("LAYERX_WEBHOOKS_MAXIMUM_DELAY_SECONDS", 3_600)?,
            maximum_attempts: u32::try_from(number("LAYERX_WEBHOOKS_MAXIMUM_ATTEMPTS", 8)?)
                .map_err(|_| "maximum webhook attempts is invalid".to_owned())?,
            spread_percent: u8::try_from(number("LAYERX_WEBHOOKS_SPREAD_PERCENT", 20)?)
                .map_err(|_| "webhook spread is invalid".to_owned())?,
            suspend_after_dead_letters: u32::try_from(number(
                "LAYERX_WEBHOOKS_SUSPEND_AFTER_DEAD_LETTERS",
                20,
            )?)
            .map_err(|_| "webhook suspension threshold is invalid".to_owned())?,
            in_flight_timeout_seconds: number("LAYERX_WEBHOOKS_LEASE_SECONDS", 120)?,
        }
        .validate()
        .map_err(|_| "webhook retry policy is invalid".to_owned())?;
        Ok(Self {
            repository: RedisRepository {
                endpoint: RedisEndpoint::parse(
                    &env::var("LAYERX_WEBHOOKS_REDIS_URL")
                        .map_err(|_| "LAYERX_WEBHOOKS_REDIS_URL is required")?,
                )?,
                ca,
                username: read_secret("LAYERX_WEBHOOKS_REDIS_USERNAME_FILE")?,
                password: read_secret("LAYERX_WEBHOOKS_REDIS_PASSWORD_FILE")?,
            },
            policy,
        })
    }

    pub fn ready(&self) -> bool {
        self.repository.ready()
    }

    pub fn snapshot(
        &self,
        principal: &Principal,
        now: u64,
        limit: usize,
    ) -> Result<HostedSnapshot, WebhookError> {
        let (_, shard) = self.repository.load(principal)?;
        Ok(snapshot_of(&shard, self.policy, now, limit.clamp(1, 200)))
    }
}

impl HostedService {
    pub fn from_environment() -> Result<Self, String> {
        let shared_ca = Certificate::from_der(&read_required("LAYERX_WEBHOOKS_INTERNAL_CA_DER")?)
            .map_err(|error| error.to_string())?;
        let identity_password = read_secret("LAYERX_WEBHOOKS_CLIENT_IDENTITY_PASSWORD_FILE")?;
        let identity = Identity::from_pkcs12(
            &read_required("LAYERX_WEBHOOKS_CLIENT_IDENTITY_PKCS12")?,
            identity_password.as_str(),
        )
        .map_err(|error| error.to_string())?;
        let internal_identity = ClientIdentity::new(shared_ca.clone(), Some(identity));
        let public_ca = Certificate::from_der(&read_required("LAYERX_WEBHOOKS_PUBLIC_CA_DER")?)
            .map_err(|error| error.to_string())?;
        let cursor = read_secret("LAYERX_WEBHOOKS_CURSOR_KEY_FILE")?;
        let cursor_key = Zeroizing::new(parse_hex32(cursor.as_str())?);
        let policy = RetryPolicy {
            base_delay_seconds: number("LAYERX_WEBHOOKS_BASE_DELAY_SECONDS", 10)?,
            maximum_delay_seconds: number("LAYERX_WEBHOOKS_MAXIMUM_DELAY_SECONDS", 3_600)?,
            maximum_attempts: u32::try_from(number("LAYERX_WEBHOOKS_MAXIMUM_ATTEMPTS", 8)?)
                .map_err(|_| "maximum webhook attempts is invalid".to_owned())?,
            spread_percent: u8::try_from(number("LAYERX_WEBHOOKS_SPREAD_PERCENT", 20)?)
                .map_err(|_| "webhook spread is invalid".to_owned())?,
            suspend_after_dead_letters: u32::try_from(number(
                "LAYERX_WEBHOOKS_SUSPEND_AFTER_DEAD_LETTERS",
                20,
            )?)
            .map_err(|_| "webhook suspension threshold is invalid".to_owned())?,
            in_flight_timeout_seconds: number("LAYERX_WEBHOOKS_LEASE_SECONDS", 120)?,
        }
        .validate()
        .map_err(|_| "webhook retry policy is invalid".to_owned())?;
        let kms = KmsClient {
            client: Client::trusted(internal_identity.clone()),
            endpoint: Endpoint::parse(
                &env::var("LAYERX_WEBHOOKS_KMS_URL")
                    .map_err(|_| "LAYERX_WEBHOOKS_KMS_URL is required")?,
            )?,
            token: read_secret("LAYERX_WEBHOOKS_KMS_TOKEN_FILE")?,
        };
        let instance = env::var("LAYERX_WEBHOOKS_INSTANCE_ID")
            .map_err(|_| "LAYERX_WEBHOOKS_INSTANCE_ID is required".to_owned())?;
        if !valid_token(&instance, 128) {
            return Err("webhook instance identifier is invalid".to_owned());
        }
        Ok(Self {
            repository: RedisRepository {
                endpoint: RedisEndpoint::parse(
                    &env::var("LAYERX_WEBHOOKS_REDIS_URL")
                        .map_err(|_| "LAYERX_WEBHOOKS_REDIS_URL is required")?,
                )?,
                ca: shared_ca,
                username: read_secret("LAYERX_WEBHOOKS_REDIS_USERNAME_FILE")?,
                password: read_secret("LAYERX_WEBHOOKS_REDIS_PASSWORD_FILE")?,
            },
            kms,
            outbound: Client::public(ClientIdentity::new(public_ca.clone(), None)),
            outbound_ca: ClientIdentity::new(public_ca, None),
            policy,
            instance,
            cursor_key,
            key_overlap_seconds: number("LAYERX_WEBHOOKS_KEY_OVERLAP_SECONDS", 86_400)?,
        })
    }

    pub fn ready(&self) -> bool {
        self.repository.ready() && self.kms.ready()
    }

    pub fn policy(&self) -> RetryPolicy {
        self.policy
    }

    fn transact<R>(
        &self,
        principal: &Principal,
        mut change: impl FnMut(&mut PrincipalShard) -> Result<R, WebhookError>,
    ) -> Result<R, WebhookError> {
        for _ in 0..CAS_ATTEMPTS {
            let (revision, mut shard) = self.repository.load(principal)?;
            let result = change(&mut shard)?;
            if self
                .repository
                .compare_and_set(principal, revision, &shard)?
            {
                return Ok(result);
            }
        }
        Err(WebhookError::Unavailable)
    }

    pub fn register(
        &self,
        principal: &Principal,
        url: &str,
        kinds: &[EventKind],
        minimum_verification: Verification,
        idempotency: &str,
        now: u64,
    ) -> Result<HostedRegistration, WebhookError> {
        if !valid_token(idempotency, 128) {
            return Err(WebhookError::InvalidRequest);
        }
        let destination = Endpoint::parse(url).map_err(|_| WebhookError::InvalidRequest)?;
        self.outbound
            .validate_destination(&destination)
            .map_err(|_| WebhookError::InvalidRequest)?;
        let endpoint = EndpointId::generate()?;
        let key_scope = format!("register:{}:{idempotency}", principal_digest(principal));
        let key = self.kms.create_key(&key_scope)?;
        let mut kinds = kinds.to_vec();
        kinds.sort_unstable();
        kinds.dedup();
        self.transact(principal, |shard| {
            if let Some(existing) = shard
                .endpoints
                .values()
                .find(|value| value.registration_scope == key_scope)
            {
                return Ok(HostedRegistration::from_key(
                    &existing.id,
                    &existing.key,
                    None,
                ));
            }
            if shard.endpoints.len() >= MAX_ENDPOINTS {
                return Err(WebhookError::InvalidRequest);
            }
            if shard.endpoints.values().any(|value| value.url == url) {
                return Err(WebhookError::EventConflict);
            }
            shard.endpoints.insert(
                endpoint.as_str().to_owned(),
                StoredEndpoint {
                    id: endpoint.clone(),
                    url: url.to_owned(),
                    kinds: kinds.clone(),
                    minimum_verification,
                    key: key.clone(),
                    pending_key: None,
                    pending_key_scope: None,
                    pending_key_activates_at: 0,
                    last_key_activates_at: 0,
                    created_at: now,
                    registration_scope: key_scope.clone(),
                    key_rotated_at: now,
                    suspended: false,
                    suspended_reason: None,
                    consecutive_dead_letters: 0,
                    delivered_total: 0,
                    dead_lettered_total: 0,
                    last_delivery_at: None,
                    last_failure: None,
                    last_failure_at: None,
                },
            );
            let inserted = shard
                .endpoints
                .get(endpoint.as_str())
                .ok_or(WebhookError::CorruptStore)?;
            Ok(HostedRegistration::from_key(
                &inserted.id,
                &inserted.key,
                None,
            ))
        })
    }

    pub fn rotate_key(
        &self,
        principal: &Principal,
        endpoint: &EndpointId,
        idempotency: &str,
        now: u64,
    ) -> Result<HostedRegistration, WebhookError> {
        if !valid_token(idempotency, 128) {
            return Err(WebhookError::InvalidRequest);
        }
        self.endpoint(principal, endpoint, now)?;
        let scope = format!(
            "rotate:{}:{}:{idempotency}",
            principal_digest(principal),
            endpoint.as_str()
        );
        let key = self.kms.create_key(&scope)?;
        let activates = now.saturating_add(self.key_overlap_seconds);
        self.transact(principal, |shard| {
            let record = shard
                .endpoints
                .get_mut(endpoint.as_str())
                .ok_or(WebhookError::UnknownEndpoint)?;
            record.promote(now);
            if record.pending_key_scope.as_deref() == Some(scope.as_str()) {
                let pending = record.pending_key.as_ref().unwrap_or(&record.key);
                return Ok(HostedRegistration::from_key(
                    endpoint,
                    pending,
                    Some(record.last_key_activates_at),
                ));
            }
            if record.pending_key.is_some() {
                return Err(WebhookError::EventConflict);
            }
            record.pending_key = Some(key.clone());
            record.pending_key_scope = Some(scope.clone());
            record.pending_key_activates_at = activates;
            record.last_key_activates_at = activates;
            record.key_rotated_at = now;
            Ok(HostedRegistration::from_key(
                endpoint,
                &key,
                Some(activates),
            ))
        })
    }

    pub fn signing_keys(
        &self,
        principal: &Principal,
        endpoint: &EndpointId,
        now: u64,
    ) -> Result<Vec<HostedRegistration>, WebhookError> {
        let record = self.endpoint(principal, endpoint, now)?;
        let mut keys = vec![HostedRegistration::from_key(
            endpoint,
            record.signing_key(now),
            None,
        )];
        if let Some(pending) = &record.pending_key {
            if pending.id != record.signing_key(now).id {
                keys.push(HostedRegistration::from_key(
                    endpoint,
                    pending,
                    Some(record.pending_key_activates_at),
                ));
            }
        }
        Ok(keys)
    }

    fn endpoint(
        &self,
        principal: &Principal,
        endpoint: &EndpointId,
        _now: u64,
    ) -> Result<StoredEndpoint, WebhookError> {
        let (_, shard) = self.repository.load(principal)?;
        shard
            .endpoints
            .get(endpoint.as_str())
            .cloned()
            .ok_or(WebhookError::UnknownEndpoint)
    }

    pub fn suspend(
        &self,
        principal: &Principal,
        endpoint: &EndpointId,
        reason: &str,
        now: u64,
    ) -> Result<(), WebhookError> {
        if reason.is_empty() || reason.len() > 256 || reason.contains(['\0', '\r', '\n']) {
            return Err(WebhookError::InvalidRequest);
        }
        self.transact(principal, |shard| {
            let record = shard
                .endpoints
                .get_mut(endpoint.as_str())
                .ok_or(WebhookError::UnknownEndpoint)?;
            record.suspended = true;
            record.suspended_reason = Some(reason.to_owned());
            record.last_failure_at = Some(now);
            Ok(())
        })
    }

    pub fn resume(&self, principal: &Principal, endpoint: &EndpointId) -> Result<(), WebhookError> {
        self.transact(principal, |shard| {
            let record = shard
                .endpoints
                .get_mut(endpoint.as_str())
                .ok_or(WebhookError::UnknownEndpoint)?;
            record.suspended = false;
            record.suspended_reason = None;
            record.consecutive_dead_letters = 0;
            Ok(())
        })
    }

    pub fn publish(
        &self,
        trusted: &TrustedEvent,
        now: u64,
    ) -> Result<HostedPublishOutcome, WebhookError> {
        let event = trusted.event();
        event.validate()?;
        let mut stable_queued: Option<Vec<String>> = None;
        self.transact(event.principal(), |shard| {
            if let Some(position) = shard.positions.get(event.id().as_str()).copied() {
                let existing = shard
                    .events
                    .get(&position)
                    .ok_or(WebhookError::CorruptStore)?;
                if existing != event {
                    return Err(WebhookError::EventConflict);
                }
                let queued = shard
                    .deliveries
                    .values()
                    .filter(|delivery| delivery.event == event.id().as_str())
                    .map(|delivery| delivery.id.as_str().to_owned())
                    .collect();
                return Ok(HostedPublishOutcome {
                    position,
                    duplicate: true,
                    queued,
                });
            }
            if shard.events.len() >= MAX_EVENTS {
                return Err(WebhookError::Unavailable);
            }
            let reached = shard
                .high_water
                .get(event.subject().as_str())
                .copied()
                .unwrap_or(0);
            if event.subject_sequence() <= reached {
                return Err(WebhookError::OrderViolation);
            }
            let position = shard.next_position;
            shard.next_position = position.saturating_add(1);
            shard.events.insert(position, event.clone());
            shard
                .positions
                .insert(event.id().as_str().to_owned(), position);
            shard.high_water.insert(
                event.subject().as_str().to_owned(),
                event.subject_sequence(),
            );
            let targets: Vec<EndpointId> = shard
                .endpoints
                .values()
                .filter(|endpoint| endpoint.accepts(event))
                .map(|endpoint| endpoint.id.clone())
                .collect();
            let mut queued = stable_queued.take().unwrap_or_default();
            if queued.len() != targets.len() {
                queued.clear();
                for _ in &targets {
                    queued.push(DeliveryId::generate()?.as_str().to_owned());
                }
                stable_queued = Some(queued.clone());
            }
            for (target, id) in targets.into_iter().zip(&queued) {
                enqueue(shard, event, position, now, target, id.clone(), None)?;
            }
            Ok(HostedPublishOutcome {
                position,
                duplicate: false,
                queued,
            })
        })
    }

    pub fn dispatch(&self, now: u64, budget: u32) -> Result<HostedDispatchReport, WebhookError> {
        let mut report = HostedDispatchReport::default();
        for principal in self.repository.principals()? {
            while report.attempted < budget {
                let Some(prepared) = self.prepare(&principal, now)? else {
                    break;
                };
                report.attempted = report.attempted.saturating_add(1);
                let started = Instant::now();
                let outcome = self.send(&prepared);
                let latency = u64::try_from(started.elapsed().as_millis()).unwrap_or(u64::MAX);
                self.settle(&prepared, outcome, latency, now, &mut report)?;
            }
        }
        Ok(report)
    }

    fn prepare(&self, principal: &Principal, now: u64) -> Result<Option<Prepared>, WebhookError> {
        let lease = random_lease(&self.instance)?;
        self.transact(principal, |shard| {
            let keys: Vec<String> = shard.queues.keys().cloned().collect();
            for queue in keys {
                while let Some(head) = shard.queues.get(&queue).and_then(VecDeque::front).cloned() {
                    let terminal = shard.deliveries.get(&head).is_none_or(|delivery| {
                        matches!(
                            delivery.state,
                            StoredDeliveryState::Delivered { .. }
                                | StoredDeliveryState::DeadLettered { .. }
                        )
                    });
                    if !terminal {
                        break;
                    }
                    if let Some(values) = shard.queues.get_mut(&queue) {
                        values.pop_front();
                    }
                }
                let Some(id) = shard.queues.get(&queue).and_then(VecDeque::front).cloned() else {
                    shard.queues.remove(&queue);
                    continue;
                };
                let Some(delivery) = shard.deliveries.get(&id).cloned() else {
                    continue;
                };
                let Some(due) = delivery
                    .state
                    .due(delivery.created_at, self.policy.in_flight_timeout_seconds)
                else {
                    continue;
                };
                if due > now {
                    continue;
                }
                let endpoint = shard
                    .endpoints
                    .get_mut(delivery.endpoint.as_str())
                    .ok_or(WebhookError::CorruptStore)?;
                if endpoint.suspended {
                    continue;
                }
                endpoint.promote(now);
                let key = endpoint.signing_key(now).clone();
                let event = shard
                    .events
                    .get(&delivery.log_position)
                    .ok_or(WebhookError::CorruptStore)?;
                let payload = serde_json::to_vec(&Envelope {
                    scheme: scheme::SCHEME_VERSION,
                    endpoint_id: delivery.endpoint.as_str(),
                    receiver_obligation: scheme::RECEIVER_OBLIGATION,
                    event,
                })
                .map_err(|_| WebhookError::CorruptStore)?;
                let attempt = delivery.state.attempts().saturating_add(1);
                let record = shard
                    .deliveries
                    .get_mut(&id)
                    .ok_or(WebhookError::CorruptStore)?;
                record.state = StoredDeliveryState::InFlight {
                    attempt,
                    started_at: now,
                    lease: lease.clone(),
                };
                return Ok(Some(Prepared {
                    principal: principal.clone(),
                    delivery: id,
                    endpoint: delivery.endpoint.as_str().to_owned(),
                    url: endpoint.url.clone(),
                    event: delivery.event,
                    kind: event.kind(),
                    subject: delivery.subject,
                    sequence: delivery.subject_sequence,
                    attempt,
                    lease: lease.clone(),
                    key,
                    payload,
                    timestamp: now,
                }));
            }
            Ok(None)
        })
    }

    fn send(&self, prepared: &Prepared) -> Result<u16, FailureKind> {
        let message =
            scheme::canonical_message(&prepared.event, prepared.timestamp, &prepared.payload);
        let signature = self
            .kms
            .sign(&prepared.key, &message)
            .map_err(|_| FailureKind::Unreachable)?;
        let endpoint = Endpoint::parse(&prepared.url).map_err(|_| FailureKind::Protocol)?;
        let client = Client::public(self.outbound_ca.clone());
        let headers = vec![
            (scheme::ID_HEADER.to_owned(), prepared.event.clone()),
            (
                scheme::TIMESTAMP_HEADER.to_owned(),
                prepared.timestamp.to_string(),
            ),
            (scheme::KEY_HEADER.to_owned(), prepared.key.id.clone()),
            (
                scheme::SIGNATURE_HEADER.to_owned(),
                scheme::signature_header(&signature),
            ),
            (
                scheme::DELIVERY_HEADER.to_owned(),
                prepared.delivery.clone(),
            ),
            (
                scheme::KIND_HEADER.to_owned(),
                prepared.kind.as_str().to_owned(),
            ),
            (scheme::SUBJECT_HEADER.to_owned(), prepared.subject.clone()),
            (
                scheme::SEQUENCE_HEADER.to_owned(),
                prepared.sequence.to_string(),
            ),
            (
                scheme::ATTEMPT_HEADER.to_owned(),
                prepared.attempt.to_string(),
            ),
            (
                scheme::ENDPOINT_HEADER.to_owned(),
                prepared.endpoint.clone(),
            ),
        ];
        let response = client
            .request(
                &endpoint,
                "POST",
                "",
                None,
                None,
                &headers,
                &prepared.payload,
            )
            .map_err(|error| {
                if error.contains("timed out") {
                    FailureKind::Timeout
                } else {
                    FailureKind::Unreachable
                }
            })?;
        if (300..400).contains(&response.status) {
            return Err(FailureKind::Refused);
        }
        Ok(response.status)
    }

    fn settle(
        &self,
        prepared: &Prepared,
        outcome: Result<u16, FailureKind>,
        latency_ms: u64,
        now: u64,
        report: &mut HostedDispatchReport,
    ) -> Result<(), WebhookError> {
        let policy = self.policy;
        self.transact(&prepared.principal, |shard| {
            let delivery = shard
                .deliveries
                .get_mut(&prepared.delivery)
                .ok_or(WebhookError::UnknownDelivery)?;
            let owns_lease = matches!(
                &delivery.state,
                StoredDeliveryState::InFlight { attempt, lease, .. }
                    if *attempt == prepared.attempt && lease.as_bytes().ct_eq(prepared.lease.as_bytes()).unwrap_u8() == 1
            );
            if !owns_lease {
                return Ok(());
            }
            let (status, failure) = match outcome {
                Ok(value) if (200..300).contains(&value) => (Some(value), None),
                Ok(410) => (Some(410), Some(FailureKind::Gone)),
                Ok(value) => (Some(value), Some(FailureKind::Refused)),
                Err(value) => (None, Some(value)),
            };
            if delivery.attempts.len() >= MAX_ATTEMPTS {
                return Err(WebhookError::CorruptStore);
            }
            delivery.attempts.push(AttemptRecord {
                attempt: prepared.attempt,
                at: now,
                status,
                failure,
                latency_ms,
            });
            let endpoint = shard
                .endpoints
                .get_mut(&prepared.endpoint)
                .ok_or(WebhookError::CorruptStore)?;
            if failure.is_none() {
                delivery.state = StoredDeliveryState::Delivered {
                    attempt: prepared.attempt,
                    at: now,
                    status: status.unwrap_or(204),
                };
                endpoint.delivered_total = endpoint.delivered_total.saturating_add(1);
                endpoint.consecutive_dead_letters = 0;
                endpoint.last_delivery_at = Some(now);
                report.delivered = report.delivered.saturating_add(1);
                return Ok(());
            }
            let failure = failure.unwrap_or(FailureKind::Protocol);
            endpoint.last_failure = Some(failure.as_str().to_owned());
            endpoint.last_failure_at = Some(now);
            if failure.permanent() || prepared.attempt >= policy.maximum_attempts {
                delivery.state = StoredDeliveryState::DeadLettered {
                    attempts: prepared.attempt,
                    at: now,
                    failure,
                    status,
                };
                shard.dead_letters.push(prepared.delivery.clone());
                endpoint.dead_lettered_total = endpoint.dead_lettered_total.saturating_add(1);
                endpoint.consecutive_dead_letters = endpoint.consecutive_dead_letters.saturating_add(1);
                if endpoint.consecutive_dead_letters >= policy.suspend_after_dead_letters {
                    endpoint.suspended = true;
                    endpoint.suspended_reason = Some(
                        "consecutive dead letters reached the suspension bound".to_owned(),
                    );
                }
                report.dead_lettered = report.dead_lettered.saturating_add(1);
            } else {
                let delay = policy.backoff_seconds(
                    prepared.attempt,
                    &digest(prepared.delivery.as_bytes()),
                );
                delivery.state = StoredDeliveryState::Retrying {
                    attempt: prepared.attempt,
                    next_attempt_at: now.saturating_add(delay),
                    failure,
                    status,
                };
                report.retrying = report.retrying.saturating_add(1);
            }
            Ok(())
        })
    }

    pub fn snapshot(
        &self,
        principal: &Principal,
        now: u64,
        limit: usize,
    ) -> Result<HostedSnapshot, WebhookError> {
        let (_, shard) = self.repository.load(principal)?;
        Ok(snapshot_of(&shard, self.policy, now, limit.clamp(1, 200)))
    }

    pub fn events_since(
        &self,
        principal: &Principal,
        endpoint: &EndpointId,
        cursor: Option<&str>,
        limit: usize,
    ) -> Result<HostedEventPage, WebhookError> {
        let (_, shard) = self.repository.load(principal)?;
        let record = shard
            .endpoints
            .get(endpoint.as_str())
            .ok_or(WebhookError::UnknownEndpoint)?;
        let from = match cursor {
            Some(value) => decode_cursor(&self.cursor_key, principal, endpoint, value)?,
            None => shard.cursor_floor.saturating_sub(1),
        };
        if from.saturating_add(1) < shard.cursor_floor {
            return Err(WebhookError::CursorExpired);
        }
        let selected: Vec<(u64, ProtocolEvent)> = shard
            .events
            .range(from.saturating_add(1)..)
            .filter(|(_, event)| record.accepts(event))
            .take(limit.clamp(1, 200).saturating_add(1))
            .map(|(position, event)| (*position, event.clone()))
            .collect();
        let has_more = selected.len() > limit.clamp(1, 200);
        let visible = selected.len().min(limit.clamp(1, 200));
        let position = selected
            .get(visible.saturating_sub(1))
            .map_or(from, |(position, _)| *position);
        Ok(HostedEventPage {
            events: selected
                .into_iter()
                .take(visible)
                .map(|(_, event)| event)
                .collect(),
            next_cursor: encode_cursor(&self.cursor_key, principal, endpoint, position),
            has_more,
        })
    }

    pub fn redeliver(
        &self,
        principal: &Principal,
        endpoint: &EndpointId,
        cursor: Option<&str>,
        limit: usize,
        idempotency: &str,
        now: u64,
    ) -> Result<HostedRedeliveryOutcome, WebhookError> {
        if !valid_token(idempotency, 128) {
            return Err(WebhookError::InvalidRequest);
        }
        let page = self.events_since(principal, endpoint, cursor, limit)?;
        let events = page.events.clone();
        let delivery_ids = events
            .iter()
            .map(|event| {
                scoped_delivery_id(
                    b"redelivery",
                    principal,
                    endpoint,
                    idempotency,
                    event.id().as_str(),
                )
            })
            .collect::<Result<Vec<_>, _>>()?;
        let queued = self.transact(principal, |shard| {
            for (event, id) in events.iter().zip(&delivery_ids) {
                let position = shard
                    .positions
                    .get(event.id().as_str())
                    .copied()
                    .ok_or(WebhookError::CursorExpired)?;
                enqueue(
                    shard,
                    event,
                    position,
                    now,
                    endpoint.clone(),
                    id.clone(),
                    None,
                )?;
            }
            Ok(delivery_ids.clone())
        })?;
        Ok(HostedRedeliveryOutcome {
            queued,
            next_cursor: page.next_cursor,
            has_more: page.has_more,
        })
    }

    pub fn replay_dead_letter(
        &self,
        principal: &Principal,
        delivery: &DeliveryId,
        idempotency: &str,
        now: u64,
    ) -> Result<String, WebhookError> {
        if !valid_token(idempotency, 128) {
            return Err(WebhookError::InvalidRequest);
        }
        let (_, current) = self.repository.load(principal)?;
        let old = current
            .deliveries
            .get(delivery.as_str())
            .ok_or(WebhookError::UnknownDelivery)?;
        let replacement = scoped_delivery_id(
            b"dead-letter-replay",
            principal,
            &old.endpoint,
            idempotency,
            old.event.as_str(),
        )?;
        self.transact(principal, |shard| {
            if shard.deliveries.contains_key(&replacement) {
                return Ok(replacement.clone());
            }
            let old = shard
                .deliveries
                .get(delivery.as_str())
                .cloned()
                .ok_or(WebhookError::UnknownDelivery)?;
            if !matches!(old.state, StoredDeliveryState::DeadLettered { .. }) {
                return Err(WebhookError::NotDeadLettered);
            }
            let event = shard
                .events
                .get(&old.log_position)
                .cloned()
                .ok_or(WebhookError::CursorExpired)?;
            enqueue(
                shard,
                &event,
                old.log_position,
                now,
                old.endpoint,
                replacement.clone(),
                Some(delivery.as_str().to_owned()),
            )?;
            Ok(replacement.clone())
        })
    }

    pub fn prune(&self, principal: &Principal, retain: usize) -> Result<usize, WebhookError> {
        self.transact(principal, |shard| {
            let mut released = 0_usize;
            while shard.events.len() > retain.max(1) {
                let Some((&position, event)) = shard.events.first_key_value() else {
                    break;
                };
                let event_id = event.id().as_str().to_owned();
                let terminal = shard
                    .deliveries
                    .values()
                    .filter(|delivery| delivery.event == event_id)
                    .all(|delivery| {
                        matches!(
                            delivery.state,
                            StoredDeliveryState::Delivered { .. }
                                | StoredDeliveryState::DeadLettered { .. }
                        )
                    });
                if !terminal {
                    break;
                }
                shard.events.remove(&position);
                shard.positions.remove(&event_id);
                let retired: Vec<String> = shard
                    .deliveries
                    .iter()
                    .filter(|(_, delivery)| delivery.event == event_id)
                    .map(|(id, _)| id.clone())
                    .collect();
                for id in &retired {
                    shard.deliveries.remove(id);
                }
                shard
                    .dead_letters
                    .retain(|id| !retired.iter().any(|retired| retired == id));
                for queue in shard.queues.values_mut() {
                    queue.retain(|id| !retired.iter().any(|retired| retired == id));
                }
                shard.queues.retain(|_, queue| !queue.is_empty());
                shard.cursor_floor = position.saturating_add(1);
                released = released.saturating_add(1);
            }
            Ok(released)
        })
    }

    pub fn prune_all(&self, retain: usize) -> Result<usize, WebhookError> {
        let mut released = 0_usize;
        for principal in self.repository.principals()? {
            released = released.saturating_add(self.prune(&principal, retain)?);
        }
        Ok(released)
    }
}

fn enqueue(
    shard: &mut PrincipalShard,
    event: &ProtocolEvent,
    position: u64,
    now: u64,
    endpoint: EndpointId,
    id: String,
    replay_of: Option<String>,
) -> Result<(), WebhookError> {
    if shard.deliveries.contains_key(&id) {
        return Ok(());
    }
    let delivery_id = DeliveryId::new(id.clone())?;
    let queue = format!("{}|{}", endpoint.as_str(), event.subject().as_str());
    shard.deliveries.insert(
        id.clone(),
        StoredDelivery {
            id: delivery_id,
            endpoint,
            event: event.id().as_str().to_owned(),
            subject: event.subject().as_str().to_owned(),
            subject_sequence: event.subject_sequence(),
            log_position: position,
            created_at: now,
            state: StoredDeliveryState::Pending,
            attempts: Vec::new(),
            replay_of,
        },
    );
    shard.queues.entry(queue).or_default().push_back(id);
    Ok(())
}

fn snapshot_of(
    shard: &PrincipalShard,
    policy: RetryPolicy,
    now: u64,
    limit: usize,
) -> HostedSnapshot {
    let mut deliveries: Vec<DeliveryRecord> = shard
        .deliveries
        .values()
        .filter_map(|delivery| delivery_record(shard, delivery))
        .collect();
    deliveries.sort_by(|left, right| right.log_position.cmp(&left.log_position));
    deliveries.truncate(limit);
    let dead_letters = shard
        .dead_letters
        .iter()
        .rev()
        .filter_map(|id| shard.deliveries.get(id))
        .filter_map(|delivery| delivery_record(shard, delivery))
        .take(limit)
        .collect();
    let endpoints = shard
        .endpoints
        .values()
        .map(|endpoint| endpoint_health(shard, endpoint, policy, now))
        .collect();
    let events = shard.events.values().rev().take(limit).cloned().collect();
    HostedSnapshot {
        endpoints,
        events,
        deliveries,
        dead_letters,
        cursor_floor: shard.cursor_floor,
        last_position: shard.next_position.saturating_sub(1),
    }
}

fn delivery_record(shard: &PrincipalShard, delivery: &StoredDelivery) -> Option<DeliveryRecord> {
    let event = shard.events.get(&delivery.log_position)?;
    Some(DeliveryRecord {
        delivery: delivery.id.as_str().to_owned(),
        endpoint: delivery.endpoint.as_str().to_owned(),
        event: delivery.event.clone(),
        kind: event.kind(),
        subject: delivery.subject.clone(),
        subject_sequence: delivery.subject_sequence,
        log_position: delivery.log_position,
        created_at: delivery.created_at,
        state: delivery.state.public(),
        attempts: delivery.attempts.clone(),
        verification: event.verification(),
        receipt_digest: event.receipt_digest().map(str::to_owned),
        replay_of: delivery.replay_of.clone(),
    })
}

fn endpoint_health(
    shard: &PrincipalShard,
    endpoint: &StoredEndpoint,
    policy: RetryPolicy,
    now: u64,
) -> EndpointHealth {
    let selected: Vec<&StoredDelivery> = shard
        .deliveries
        .values()
        .filter(|delivery| delivery.endpoint == endpoint.id)
        .collect();
    let pending = selected
        .iter()
        .filter(|delivery| matches!(delivery.state, StoredDeliveryState::Pending))
        .count() as u64;
    let in_flight = selected
        .iter()
        .filter(|delivery| matches!(delivery.state, StoredDeliveryState::InFlight { .. }))
        .count() as u64;
    let retrying = selected
        .iter()
        .filter(|delivery| matches!(delivery.state, StoredDeliveryState::Retrying { .. }))
        .count() as u64;
    let oldest_undelivered = selected
        .iter()
        .filter(|delivery| {
            !matches!(
                delivery.state,
                StoredDeliveryState::Delivered { .. } | StoredDeliveryState::DeadLettered { .. }
            )
        })
        .map(|delivery| delivery.created_at)
        .min();
    let next_attempt_at = selected
        .iter()
        .filter_map(|delivery| {
            delivery
                .state
                .due(delivery.created_at, policy.in_flight_timeout_seconds)
        })
        .min();
    EndpointHealth {
        endpoint: endpoint.id.as_str().to_owned(),
        url: endpoint.url.clone(),
        kinds: endpoint.kinds.clone(),
        minimum_verification: endpoint.minimum_verification,
        suspended: endpoint.suspended,
        suspended_reason: endpoint.suspended_reason.clone(),
        pending,
        in_flight,
        retrying,
        delivered_total: endpoint.delivered_total,
        dead_lettered_total: endpoint.dead_lettered_total,
        consecutive_dead_letters: endpoint.consecutive_dead_letters,
        oldest_undelivered_seconds: oldest_undelivered.map_or(0, |at| now.saturating_sub(at)),
        next_attempt_at,
        last_delivery_at: endpoint.last_delivery_at,
        last_failure: endpoint.last_failure.clone(),
        last_failure_at: endpoint.last_failure_at,
        key_id: endpoint.signing_key(now).id.clone(),
        public_key: base64_encode(&endpoint.signing_key(now).public_key),
        key_rotated_at: endpoint.key_rotated_at,
        pending_key_id: endpoint.pending_key.as_ref().map(|key| key.id.clone()),
        pending_public_key: endpoint
            .pending_key
            .as_ref()
            .map(|key| base64_encode(&key.public_key)),
        pending_key_activates_at: endpoint
            .pending_key
            .as_ref()
            .map(|_| endpoint.pending_key_activates_at),
    }
}

fn encode_cursor(
    key: &[u8; 32],
    principal: &Principal,
    endpoint: &EndpointId,
    position: u64,
) -> String {
    let message = cursor_message(principal, endpoint, position);
    let tag = hmac(key, b"LayerX/webhooks/cursor/v2", &message);
    format!("{CURSOR_PREFIX}{position:016x}{}", hex_encode(&tag[..16]))
}

fn decode_cursor(
    key: &[u8; 32],
    principal: &Principal,
    endpoint: &EndpointId,
    cursor: &str,
) -> Result<u64, WebhookError> {
    let value = cursor
        .strip_prefix(CURSOR_PREFIX)
        .ok_or(WebhookError::InvalidCursor)?;
    if value.len() != 48 {
        return Err(WebhookError::InvalidCursor);
    }
    let position =
        u64::from_str_radix(&value[..16], 16).map_err(|_| WebhookError::InvalidCursor)?;
    let expected = encode_cursor(key, principal, endpoint, position);
    if expected.as_bytes().ct_eq(cursor.as_bytes()).unwrap_u8() != 1 {
        return Err(WebhookError::InvalidCursor);
    }
    Ok(position)
}

fn cursor_message(principal: &Principal, endpoint: &EndpointId, position: u64) -> Vec<u8> {
    let mut message = Vec::new();
    message.extend_from_slice(principal.as_str().as_bytes());
    message.push(0);
    message.extend_from_slice(endpoint.as_str().as_bytes());
    message.push(0);
    message.extend_from_slice(&position.to_be_bytes());
    message
}

fn hmac(key: &[u8; 32], domain: &[u8], context: &[u8]) -> [u8; 32] {
    let mut inner_pad = [0x36_u8; 64];
    let mut outer_pad = [0x5c_u8; 64];
    for (index, byte) in key.iter().enumerate() {
        inner_pad[index] ^= byte;
        outer_pad[index] ^= byte;
    }
    let mut inner = Sha256::new();
    inner.update(inner_pad);
    inner.update((domain.len() as u64).to_be_bytes());
    inner.update(domain);
    inner.update((context.len() as u64).to_be_bytes());
    inner.update(context);
    let inner_hash = inner.finalize();
    let mut outer = Sha256::new();
    outer.update(outer_pad);
    outer.update(inner_hash);
    inner_pad.zeroize();
    outer_pad.zeroize();
    outer.finalize().into()
}

fn principal_digest(principal: &Principal) -> String {
    hex_encode(&Sha256::digest(principal.as_str().as_bytes()))
}

fn shard_key(principal: &Principal) -> String {
    format!("webhooks:principal:{}", principal_digest(principal))
}

fn random_lease(instance: &str) -> Result<String, WebhookError> {
    let mut random = [0_u8; 24];
    getrandom::fill(&mut random).map_err(|_| WebhookError::Entropy)?;
    Ok(format!("{}:{}", instance, hex_encode(&random)))
}

fn scoped_delivery_id(
    domain: &[u8],
    principal: &Principal,
    endpoint: &EndpointId,
    idempotency: &str,
    event: &str,
) -> Result<String, WebhookError> {
    let mut hash = Sha256::new();
    for part in [
        domain,
        principal.as_str().as_bytes(),
        endpoint.as_str().as_bytes(),
        idempotency.as_bytes(),
        event.as_bytes(),
    ] {
        hash.update((part.len() as u64).to_be_bytes());
        hash.update(part);
    }
    let id = format!("whdl_{}", hex_encode(&hash.finalize()[..16]));
    DeliveryId::new(id.clone())?;
    Ok(id)
}

fn read_required(name: &str) -> Result<Vec<u8>, String> {
    fs::read(env::var(name).map_err(|_| format!("{name} is required"))?)
        .map_err(|error| error.to_string())
}

fn read_secret(name: &str) -> Result<Zeroizing<String>, String> {
    let mut value =
        String::from_utf8(read_required(name)?).map_err(|_| format!("{name} is not UTF-8"))?;
    while matches!(value.as_bytes().last(), Some(b'\r' | b'\n')) {
        value.pop();
    }
    if value.is_empty() || value.len() > 4096 {
        value.zeroize();
        return Err(format!("{name} is empty or oversized"));
    }
    Ok(Zeroizing::new(value))
}

fn number(name: &str, default: u64) -> Result<u64, String> {
    env::var(name).map_or(Ok(default), |value| {
        value
            .parse::<u64>()
            .map_err(|_| format!("{name} is not an integer"))
    })
}

fn parse_hex32(value: &str) -> Result<[u8; 32], String> {
    if value.len() != 64 {
        return Err("secret must be 32 hexadecimal bytes".to_owned());
    }
    let mut bytes = [0_u8; 32];
    for (index, pair) in value.as_bytes().chunks_exact(2).enumerate() {
        let text = std::str::from_utf8(pair).map_err(|_| "secret is not hexadecimal")?;
        bytes[index] =
            u8::from_str_radix(text, 16).map_err(|_| "secret is not hexadecimal".to_owned())?;
    }
    Ok(bytes)
}

fn valid_token(value: &str, maximum: usize) -> bool {
    !value.is_empty()
        && value.len() <= maximum
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.'))
}

fn write_resp(stream: &mut impl Write, arguments: &[&str]) -> Result<(), String> {
    write!(stream, "*{}\r\n", arguments.len()).map_err(|error| error.to_string())?;
    for argument in arguments {
        write!(stream, "${}\r\n", argument.len()).map_err(|error| error.to_string())?;
        stream
            .write_all(argument.as_bytes())
            .map_err(|error| error.to_string())?;
        stream
            .write_all(b"\r\n")
            .map_err(|error| error.to_string())?;
    }
    stream.flush().map_err(|error| error.to_string())
}

fn read_resp(stream: &mut impl Read, depth: usize) -> Result<Resp, String> {
    if depth > 8 {
        return Err("webhook Redis response nesting is excessive".to_owned());
    }
    let mut marker = [0_u8; 1];
    stream
        .read_exact(&mut marker)
        .map_err(|error| error.to_string())?;
    match marker[0] {
        b'+' => Ok(Resp::Simple(read_line(stream)?)),
        b'-' => Err(format!(
            "webhook Redis refused command: {}",
            read_line(stream)?
        )),
        b':' => read_line(stream)?
            .parse::<i64>()
            .map(Resp::Integer)
            .map_err(|_| "webhook Redis integer is invalid".to_owned()),
        b'$' => {
            let length = read_line(stream)?
                .parse::<i64>()
                .map_err(|_| "webhook Redis bulk length is invalid".to_owned())?;
            if length == -1 {
                return Ok(Resp::Bulk(None));
            }
            let length = usize::try_from(length)
                .map_err(|_| "webhook Redis bulk length is invalid".to_owned())?;
            if length > MAX_REDIS_RESPONSE {
                return Err("webhook Redis response exceeds its bound".to_owned());
            }
            let mut bytes = vec![0_u8; length];
            stream
                .read_exact(&mut bytes)
                .map_err(|error| error.to_string())?;
            let mut ending = [0_u8; 2];
            stream
                .read_exact(&mut ending)
                .map_err(|error| error.to_string())?;
            if ending != *b"\r\n" {
                return Err("webhook Redis response is malformed".to_owned());
            }
            Ok(Resp::Bulk(Some(bytes)))
        }
        b'*' => {
            let length = read_line(stream)?
                .parse::<usize>()
                .map_err(|_| "webhook Redis array length is invalid".to_owned())?;
            if length > 200_000 {
                return Err("webhook Redis array exceeds its bound".to_owned());
            }
            let mut values = Vec::with_capacity(length);
            for _ in 0..length {
                values.push(read_resp(stream, depth + 1)?);
            }
            Ok(Resp::Array(values))
        }
        _ => Err("webhook Redis response marker is invalid".to_owned()),
    }
}

fn read_line(stream: &mut impl Read) -> Result<String, String> {
    let mut bytes = Vec::new();
    loop {
        let mut byte = [0_u8; 1];
        stream
            .read_exact(&mut byte)
            .map_err(|error| error.to_string())?;
        bytes.push(byte[0]);
        if bytes.len() > 8192 {
            return Err("webhook Redis response line is excessive".to_owned());
        }
        if bytes.ends_with(b"\r\n") {
            bytes.truncate(bytes.len().saturating_sub(2));
            return String::from_utf8(bytes)
                .map_err(|_| "webhook Redis response is not UTF-8".to_owned());
        }
    }
}

fn resp_text(value: &Resp) -> Option<String> {
    match value {
        Resp::Simple(value) => Some(value.clone()),
        Resp::Bulk(Some(value)) => String::from_utf8(value.clone()).ok(),
        Resp::Bulk(None) | Resp::Integer(_) | Resp::Array(_) => None,
    }
}

fn resp_bytes(value: &Resp) -> Option<&[u8]> {
    match value {
        Resp::Bulk(Some(value)) => Some(value),
        _ => None,
    }
}

const CAS_SCRIPT: &str = r#"
local current = redis.call('HGET', KEYS[1], 'revision')
if current == false then current = '0' end
if current ~= ARGV[1] then return 0 end
redis.call('HSET', KEYS[1], 'json', ARGV[2])
redis.call('HINCRBY', KEYS[1], 'revision', 1)
redis.call('SADD', KEYS[2], ARGV[3])
return 1
"#;
