use native_tls::{Certificate, TlsConnector};
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;
use std::io::{Read, Write};
use std::net::{TcpStream, ToSocketAddrs};
use std::time::Duration;
use zeroize::Zeroizing;

const CONNECT_TIMEOUT: Duration = Duration::from_secs(3);
const IO_TIMEOUT: Duration = Duration::from_secs(8);
const MAX_RESPONSE: usize = 16 * 1024 * 1024;
const CONTINUATION_CHUNK_BYTES: usize = 128 * 1024;
const MAX_CONTINUATION_BYTES: usize = 17 * 65_536;
const MAX_CONTINUATION_CHUNKS: usize = 9;
const AUDIT_ATTEMPTS: usize = 8;
const MAX_KEYS_PER_PRINCIPAL: u64 = 128;
const MAX_TAP_REPLAY_SECONDS: u64 = 3_600;

#[derive(Clone)]
pub struct RedisEndpoint {
    host: String,
    port: u16,
}

impl RedisEndpoint {
    pub fn parse(value: &str) -> Result<Self, String> {
        let authority = value
            .strip_prefix("rediss://")
            .ok_or_else(|| "gateway Redis endpoint must use rediss".to_owned())?
            .trim_end_matches('/');
        if authority.is_empty() || authority.contains(['@', '/', '?', '#', '\\']) {
            return Err("gateway Redis endpoint is not canonical".to_owned());
        }
        let (host, port) = authority.rsplit_once(':').map_or_else(
            || Ok::<_, String>((authority.to_owned(), 6379)),
            |(host, port)| {
                Ok((
                    host.to_owned(),
                    port.parse::<u16>()
                        .map_err(|_| "gateway Redis port is invalid".to_owned())?,
                ))
            },
        )?;
        if host.is_empty() {
            return Err("gateway Redis host is missing".to_owned());
        }
        Ok(Self { host, port })
    }
}

pub struct RedisStore {
    endpoint: RedisEndpoint,
    ca: Certificate,
    username: Zeroizing<String>,
    password: Zeroizing<String>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct KeyRecord {
    pub key_id: String,
    pub principal_digest: String,
    pub salt: String,
    pub secret_digest: String,
    pub signer_public_key: String,
    pub scopes: String,
    pub quota_requests: u64,
    pub quota_window_seconds: u64,
    pub epoch: u64,
    pub disabled: bool,
}

pub enum Reservation {
    Reserved,
    Existing {
        digest: String,
        state: String,
        response: String,
        receipt: String,
        principal: String,
    },
    RateLimited {
        retry_after_seconds: u64,
    },
    Revoked,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct OperationRecord {
    pub digest: String,
    pub state: String,
    pub response: String,
    pub receipt: String,
    pub principal: String,
    pub activity_id: String,
    pub idempotency_key: String,
    pub continuation: String,
}

/// Durable evidence binding written when a cryptographically verified TAP
/// credential consumes its one-use nonce.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TapCredentialRecord {
    pub principal_digest: String,
    pub key_id: String,
    pub layerx_agent: String,
    pub trusted_agent_id: String,
    pub trusted_agent_domain: String,
    pub intent: String,
    pub evidence_digest: String,
    pub activity_id: Option<String>,
    pub signer_public_key: String,
    pub target_authority: String,
    pub target_path: String,
    pub operation_identity: String,
    pub credential_expires_at: u64,
}

/// Closed result of the atomic durable TAP nonce transition.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum TapNonceConsumption {
    Consumed { binding_digest: String },
    AlreadyConsumed { binding_digest: String },
    Replay,
}

enum Resp {
    Simple(String),
    Bulk(Option<Vec<u8>>),
    Integer(i64),
    Array(Vec<Resp>),
}

impl RedisStore {
    pub fn new(
        endpoint: RedisEndpoint,
        ca: Certificate,
        username: Zeroizing<String>,
        password: Zeroizing<String>,
    ) -> Self {
        Self {
            endpoint,
            ca,
            username,
            password,
        }
    }

    pub fn ready(&self) -> bool {
        matches!(self.command(&["PING"]), Ok(Resp::Simple(value)) if value == "PONG")
    }

    pub fn issue_key(&self, record: &KeyRecord, audit_event: &str) -> Result<(), String> {
        let key = format!("gateway:key:{}", record.key_id);
        let principal_keys = format!("gateway:principal:{}:keys", record.principal_digest);
        for _ in 0..AUDIT_ATTEMPTS {
            let (head, chain) = self.audit_values(audit_event)?;
            let response = self.command(&[
                "EVAL",
                ISSUE_SCRIPT,
                "4",
                &key,
                &principal_keys,
                "gateway:audit",
                "gateway:audit:head",
                &record.key_id,
                &record.principal_digest,
                &record.salt,
                &record.secret_digest,
                &record.signer_public_key,
                &record.scopes,
                &record.quota_requests.to_string(),
                &record.quota_window_seconds.to_string(),
                &record.epoch.to_string(),
                &MAX_KEYS_PER_PRINCIPAL.to_string(),
                audit_event,
                &head,
                &chain,
            ])?;
            let tag = array_tag(response)?;
            if tag == "audit_retry" {
                continue;
            }
            return if tag == "issued" {
                Ok(())
            } else {
                Err("gateway key issue conflicted".to_owned())
            };
        }
        Err("gateway audit head remained contended".to_owned())
    }

    pub fn key(&self, key_id: &str) -> Result<Option<KeyRecord>, String> {
        let key = format!("gateway:key:{key_id}");
        match self.command(&["HGETALL", &key])? {
            Resp::Array(values) if values.is_empty() => Ok(None),
            Resp::Array(values) => {
                let fields = pairs(&values)?;
                Ok(Some(KeyRecord {
                    key_id: key_id.to_owned(),
                    principal_digest: required(&fields, "principal")?,
                    salt: required(&fields, "salt")?,
                    secret_digest: required(&fields, "secret_digest")?,
                    signer_public_key: required(&fields, "signer_public_key")?,
                    scopes: required(&fields, "scopes")?,
                    quota_requests: number(&fields, "quota_requests")?,
                    quota_window_seconds: number(&fields, "quota_window_seconds")?,
                    epoch: number(&fields, "epoch")?,
                    disabled: required(&fields, "disabled")? == "1",
                }))
            }
            _ => Err("gateway key response is invalid".to_owned()),
        }
    }

    pub fn list_keys(&self, principal_digest: &str) -> Result<Vec<String>, String> {
        let key = format!("gateway:principal:{principal_digest}:keys");
        let Resp::Array(values) = self.command(&["SMEMBERS", &key])? else {
            return Err("gateway key-list response is invalid".to_owned());
        };
        let mut keys = values
            .iter()
            .map(text)
            .collect::<Option<Vec<_>>>()
            .ok_or_else(|| "gateway key-list response contains an invalid identifier".to_owned())?;
        keys.sort();
        if keys.len() > MAX_KEYS_PER_PRINCIPAL as usize {
            return Err("gateway principal key-list exceeds its bound".to_owned());
        }
        Ok(keys)
    }

    pub fn rotate_key(
        &self,
        old: &KeyRecord,
        replacement: &KeyRecord,
        audit_event: &str,
    ) -> Result<(), String> {
        let old_key = format!("gateway:key:{}", old.key_id);
        let new_key = format!("gateway:key:{}", replacement.key_id);
        let principal_keys = format!("gateway:principal:{}:keys", old.principal_digest);
        for _ in 0..AUDIT_ATTEMPTS {
            let (head, chain) = self.audit_values(audit_event)?;
            let response = self.command(&[
                "EVAL",
                ROTATE_SCRIPT,
                "5",
                &old_key,
                &new_key,
                &principal_keys,
                "gateway:audit",
                "gateway:audit:head",
                &old.principal_digest,
                &old.epoch.to_string(),
                &replacement.key_id,
                &replacement.salt,
                &replacement.secret_digest,
                &replacement.signer_public_key,
                &replacement.scopes,
                &replacement.quota_requests.to_string(),
                &replacement.quota_window_seconds.to_string(),
                &replacement.epoch.to_string(),
                audit_event,
                &head,
                &chain,
                &MAX_KEYS_PER_PRINCIPAL.to_string(),
            ])?;
            let tag = array_tag(response)?;
            if tag == "audit_retry" {
                continue;
            }
            return if tag == "rotated" {
                Ok(())
            } else {
                Err("gateway key rotation conflicted".to_owned())
            };
        }
        Err("gateway audit head remained contended".to_owned())
    }

    pub fn revoke_key(
        &self,
        key_id: &str,
        principal_digest: &str,
        audit_event: &str,
    ) -> Result<bool, String> {
        let key = format!("gateway:key:{key_id}");
        for _ in 0..AUDIT_ATTEMPTS {
            let (head, chain) = self.audit_values(audit_event)?;
            let response = self.command(&[
                "EVAL",
                REVOKE_SCRIPT,
                "3",
                &key,
                "gateway:audit",
                "gateway:audit:head",
                principal_digest,
                audit_event,
                &head,
                &chain,
            ])?;
            let tag = array_tag(response)?;
            if tag == "audit_retry" {
                continue;
            }
            return Ok(tag == "revoked" || tag == "already_revoked");
        }
        Err("gateway audit head remained contended".to_owned())
    }

    pub fn reserve(
        &self,
        record: &KeyRecord,
        idempotency_scope: &str,
        request_digest: &str,
        now: u64,
        retention_seconds: u64,
        activity_id: &str,
        protocol_idempotency_key: &str,
        principal_digest: &str,
        audit_event: &str,
        continuation: &str,
    ) -> Result<Reservation, String> {
        let continuation_chunks = continuation_chunks(continuation)?;
        let key = format!("gateway:key:{}", record.key_id);
        let window = now / record.quota_window_seconds;
        let usage = format!("gateway:quota:{}:{window}", record.key_id);
        let idem = format!("gateway:idem:{idempotency_scope}");
        let owner = format!("gateway:activity:{activity_id}");
        let activity_operation = format!("gateway:activity-operation:{activity_id}");
        let retry = record.quota_window_seconds - now % record.quota_window_seconds;
        for _ in 0..AUDIT_ATTEMPTS {
            let (head, chain) = self.audit_values(audit_event)?;
            let epoch = record.epoch.to_string();
            let quota_requests = record.quota_requests.to_string();
            let retry = retry.to_string();
            let retention_seconds = retention_seconds.to_string();
            let now = now.to_string();
            let mut arguments = vec![
                "EVAL",
                RESERVE_SCRIPT,
                "8",
                key.as_str(),
                usage.as_str(),
                idem.as_str(),
                "gateway:audit",
                "gateway:audit:head",
                "gateway:pending",
                owner.as_str(),
                activity_operation.as_str(),
                epoch.as_str(),
                quota_requests.as_str(),
                retry.as_str(),
                retention_seconds.as_str(),
                request_digest,
                audit_event,
                now.as_str(),
                head.as_str(),
                chain.as_str(),
                principal_digest,
                protocol_idempotency_key,
            ];
            arguments.extend(continuation_chunks.iter().copied());
            let response = self.command(&arguments)?;
            let Resp::Array(values) = response else {
                return Err("gateway reservation response is invalid".to_owned());
            };
            match values.first().and_then(text).as_deref() {
                Some("audit_retry") => continue,
                Some("reserved") => return Ok(Reservation::Reserved),
                Some("revoked") => return Ok(Reservation::Revoked),
                Some("rate_limited") => {
                    return Ok(Reservation::RateLimited {
                        retry_after_seconds: values
                            .get(1)
                            .and_then(text)
                            .and_then(|value| value.parse::<u64>().ok())
                            .ok_or_else(|| "gateway retry value is invalid".to_owned())?,
                    })
                }
                Some("existing") => {
                    return Ok(Reservation::Existing {
                        digest: values.get(1).and_then(text).unwrap_or_default(),
                        state: values.get(2).and_then(text).unwrap_or_default(),
                        response: values.get(3).and_then(text).unwrap_or_default(),
                        receipt: values.get(4).and_then(text).unwrap_or_default(),
                        principal: values.get(5).and_then(text).unwrap_or_default(),
                    })
                }
                _ => return Err("gateway reservation state is invalid".to_owned()),
            }
        }
        Err("gateway audit head remained contended".to_owned())
    }

    pub fn operation(&self, idempotency_scope: &str) -> Result<Option<OperationRecord>, String> {
        let key = format!("gateway:idem:{idempotency_scope}");
        match self.command(&["HGETALL", &key])? {
            Resp::Array(values) if values.is_empty() => Ok(None),
            Resp::Array(values) => {
                let fields = pairs(&values)?;
                let continuation = durable_continuation(&fields)?;
                Ok(Some(OperationRecord {
                    digest: required(&fields, "digest")?,
                    state: required(&fields, "state")?,
                    response: fields.get("response").cloned().unwrap_or_default(),
                    receipt: fields.get("receipt").cloned().unwrap_or_default(),
                    principal: required(&fields, "principal")?,
                    activity_id: required(&fields, "activity_id")?,
                    idempotency_key: required(&fields, "idempotency_key")?,
                    continuation,
                }))
            }
            _ => Err("gateway operation response is invalid".to_owned()),
        }
    }

    pub fn consume_read(
        &self,
        record: &KeyRecord,
        now: u64,
        audit_event: &str,
    ) -> Result<Option<u64>, String> {
        let key = format!("gateway:key:{}", record.key_id);
        let window = now / record.quota_window_seconds;
        let usage = format!("gateway:quota:{}:{window}", record.key_id);
        let retry = record.quota_window_seconds - now % record.quota_window_seconds;
        for _ in 0..AUDIT_ATTEMPTS {
            let (head, chain) = self.audit_values(audit_event)?;
            let response = self.command(&[
                "EVAL",
                CONSUME_SCRIPT,
                "4",
                &key,
                &usage,
                "gateway:audit",
                "gateway:audit:head",
                &record.epoch.to_string(),
                &record.quota_requests.to_string(),
                &retry.to_string(),
                audit_event,
                &head,
                &chain,
            ])?;
            let Resp::Array(values) = response else {
                return Err("gateway quota response is invalid".to_owned());
            };
            match values.first().and_then(text).as_deref() {
                Some("audit_retry") => continue,
                Some("consumed") => return Ok(None),
                Some("rate_limited") => return Ok(Some(retry)),
                Some("revoked") => return Err("gateway key was revoked".to_owned()),
                _ => return Err("gateway quota state is invalid".to_owned()),
            }
        }
        Err("gateway audit head remained contended".to_owned())
    }

    pub fn complete(
        &self,
        idempotency_scope: &str,
        request_digest: &str,
        state: &str,
        response_hex: &str,
        receipt_hex: &str,
        activity_id: Option<&str>,
        principal_digest: &str,
        audit_event: &str,
    ) -> Result<(), String> {
        let idem = format!("gateway:idem:{idempotency_scope}");
        let owner = activity_id.map_or_else(
            || "gateway:activity:none".to_owned(),
            |activity_id| format!("gateway:activity:{activity_id}"),
        );
        let bind_owner = if activity_id.is_some() { "1" } else { "0" };
        for _ in 0..AUDIT_ATTEMPTS {
            let (head, chain) = self.audit_values(audit_event)?;
            let response = self.command(&[
                "EVAL",
                COMPLETE_SCRIPT,
                "5",
                &idem,
                "gateway:pending",
                "gateway:audit",
                "gateway:audit:head",
                &owner,
                request_digest,
                state,
                response_hex,
                receipt_hex,
                audit_event,
                &head,
                &chain,
                bind_owner,
                principal_digest,
            ])?;
            let tag = array_tag(response)?;
            if tag == "audit_retry" {
                continue;
            }
            return if tag == "completed" {
                Ok(())
            } else {
                Err("gateway completion conflicted".to_owned())
            };
        }
        Err("gateway audit head remained contended".to_owned())
    }

    pub fn activity_owner(&self, activity_id: &str) -> Result<Option<String>, String> {
        let key = format!("gateway:activity:{activity_id}");
        match self.command(&["GET", &key])? {
            Resp::Bulk(Some(value)) => String::from_utf8(value)
                .map(Some)
                .map_err(|_| "gateway activity owner is invalid".to_owned()),
            Resp::Bulk(None) => Ok(None),
            _ => Err("gateway activity owner response is invalid".to_owned()),
        }
    }

    pub fn activity_operation(&self, activity_id: &str) -> Result<Option<OperationRecord>, String> {
        let key = format!("gateway:activity-operation:{activity_id}");
        let operation_key = match self.command(&["GET", &key])? {
            Resp::Bulk(Some(value)) => String::from_utf8(value)
                .map_err(|_| "gateway activity operation key is invalid".to_owned())?,
            Resp::Bulk(None) => return Ok(None),
            _ => return Err("gateway activity operation response is invalid".to_owned()),
        };
        if !operation_key.starts_with("gateway:idem:") {
            return Err("gateway activity operation key is outside the idempotency namespace".to_owned());
        }
        match self.command(&["HGETALL", &operation_key])? {
            Resp::Array(values) if values.is_empty() => Ok(None),
            Resp::Array(values) => {
                let fields = pairs(&values)?;
                let continuation = durable_continuation(&fields)?;
                Ok(Some(OperationRecord {
                    digest: required(&fields, "digest")?,
                    state: required(&fields, "state")?,
                    response: fields.get("response").cloned().unwrap_or_default(),
                    receipt: fields.get("receipt").cloned().unwrap_or_default(),
                    principal: required(&fields, "principal")?,
                    activity_id: required(&fields, "activity_id")?,
                    idempotency_key: required(&fields, "idempotency_key")?,
                    continuation,
                }))
            }
            _ => Err("gateway activity operation record is invalid".to_owned()),
        }
    }

    /// Atomically consumes a TAP registry-key/nonce pair and persists the
    /// credential, exact LayerX activity, and signer binding through the
    /// effective credential expiry.
    pub fn consume_tap_nonce(
        &self,
        registry_key: &str,
        nonce: &str,
        record: &TapCredentialRecord,
        now: u64,
        replay_until: u64,
        audit_event: &str,
    ) -> Result<TapNonceConsumption, String> {
        validate_tap_record(registry_key, nonce, record, now, replay_until)?;
        let nonce_scope = digest(&[
            b"gateway-tap-nonce-v1",
            registry_key.as_bytes(),
            nonce.as_bytes(),
        ]);
        let activity_id = record.activity_id.as_deref().unwrap_or("");
        let binding_digest = tap_binding_digest(record);
        let nonce_key = format!("gateway:tap:nonce:{nonce_scope}");
        let binding_key = format!("gateway:tap:binding:{binding_digest}");
        let ttl = replay_until - now;
        for _ in 0..AUDIT_ATTEMPTS {
            let (head, chain) = self.audit_values(audit_event)?;
            let response = self.command(&[
                "EVAL",
                TAP_CONSUME_SCRIPT,
                "4",
                &nonce_key,
                &binding_key,
                "gateway:audit",
                "gateway:audit:head",
                &ttl.to_string(),
                &binding_digest,
                &record.principal_digest,
                &record.key_id,
                &record.layerx_agent,
                &record.trusted_agent_id,
                &record.trusted_agent_domain,
                &record.intent,
                &record.evidence_digest,
                activity_id,
                &record.signer_public_key,
                &record.target_authority,
                &record.target_path,
                &record.operation_identity,
                &record.credential_expires_at.to_string(),
                &now.to_string(),
                audit_event,
                &head,
                &chain,
            ])?;
            let tag = array_tag(response)?;
            if tag == "audit_retry" {
                continue;
            }
            return match tag.as_str() {
                "consumed" => Ok(TapNonceConsumption::Consumed {
                    binding_digest,
                }),
                "existing" => Ok(TapNonceConsumption::AlreadyConsumed {
                    binding_digest,
                }),
                "replay" => Ok(TapNonceConsumption::Replay),
                _ => Err("gateway TAP nonce transition conflicted".to_owned()),
            };
        }
        Err("gateway audit head remained contended".to_owned())
    }

    /// Reads one credential-to-activity binding previously committed by the
    /// atomic TAP nonce transition.
    pub fn tap_binding(&self, binding_digest: &str) -> Result<Option<TapCredentialRecord>, String> {
        if !valid_hex_digest(binding_digest) {
            return Err("gateway TAP binding digest is invalid".to_owned());
        }
        let key = format!("gateway:tap:binding:{binding_digest}");
        match self.command(&["HGETALL", &key])? {
            Resp::Array(values) if values.is_empty() => Ok(None),
            Resp::Array(values) => {
                let fields = pairs(&values)?;
                let activity_id = fields
                    .get("activity_id")
                    .filter(|value| !value.is_empty())
                    .cloned();
                let record = TapCredentialRecord {
                    principal_digest: required(&fields, "principal")?,
                    key_id: required(&fields, "key_id")?,
                    layerx_agent: required(&fields, "layerx_agent")?,
                    trusted_agent_id: required(&fields, "trusted_agent_id")?,
                    trusted_agent_domain: required(&fields, "trusted_agent_domain")?,
                    intent: required(&fields, "intent")?,
                    evidence_digest: required(&fields, "evidence_digest")?,
                    activity_id,
                    signer_public_key: required(&fields, "signer_public_key")?,
                    target_authority: required(&fields, "target_authority")?,
                    target_path: required(&fields, "target_path")?,
                    operation_identity: required(&fields, "operation_identity")?,
                    credential_expires_at: number(&fields, "credential_expires_at")?,
                };
                validate_tap_record_fields(&record)?;
                if tap_binding_digest(&record) != binding_digest {
                    return Err("gateway TAP binding digest does not match its record".to_owned());
                }
                Ok(Some(record))
            }
            _ => Err("gateway TAP binding response is invalid".to_owned()),
        }
    }

    fn audit_values(&self, event: &str) -> Result<(String, String), String> {
        let head = match self.command(&["GET", "gateway:audit:head"])? {
            Resp::Bulk(Some(value)) => {
                String::from_utf8(value).map_err(|_| "gateway audit head is invalid".to_owned())?
            }
            Resp::Bulk(None) => String::new(),
            _ => return Err("gateway audit head response is invalid".to_owned()),
        };
        let mut digest = Sha256::new();
        digest.update((head.len() as u64).to_be_bytes());
        digest.update(head.as_bytes());
        digest.update((event.len() as u64).to_be_bytes());
        digest.update(event.as_bytes());
        Ok((head, format!("{:x}", digest.finalize())))
    }

    fn command(&self, arguments: &[&str]) -> Result<Resp, String> {
        let connector = TlsConnector::builder()
            .add_root_certificate(self.ca.clone())
            .min_protocol_version(Some(native_tls::Protocol::Tlsv12))
            .build()
            .map_err(|error| error.to_string())?;
        let mut last = None;
        for address in (self.endpoint.host.as_str(), self.endpoint.port)
            .to_socket_addrs()
            .map_err(|error| error.to_string())?
            .take(8)
        {
            match TcpStream::connect_timeout(&address, CONNECT_TIMEOUT) {
                Ok(tcp) => {
                    tcp.set_read_timeout(Some(IO_TIMEOUT))
                        .map_err(|error| error.to_string())?;
                    tcp.set_write_timeout(Some(IO_TIMEOUT))
                        .map_err(|error| error.to_string())?;
                    let mut stream = connector
                        .connect(&self.endpoint.host, tcp)
                        .map_err(|error| error.to_string())?;
                    write_command(
                        &mut stream,
                        &["AUTH", self.username.as_str(), self.password.as_str()],
                    )?;
                    if !matches!(read_resp(&mut stream, 0)?, Resp::Simple(value) if value == "OK") {
                        return Err("gateway Redis authentication failed".to_owned());
                    }
                    write_command(&mut stream, arguments)?;
                    return read_resp(&mut stream, 0);
                }
                Err(error) => last = Some(error),
            }
        }
        Err(last.map_or_else(
            || "gateway Redis did not resolve".to_owned(),
            |error| error.to_string(),
        ))
    }
}

const ISSUE_SCRIPT: &str = r#"
local current = redis.call('GET', KEYS[4]) or ''
if current ~= ARGV[12] then return {'audit_retry'} end
if redis.call('EXISTS', KEYS[1]) == 1 then return {'conflict'} end
if redis.call('SCARD', KEYS[2]) >= tonumber(ARGV[10]) then return {'limit'} end
redis.call('HSET', KEYS[1], 'principal', ARGV[2], 'salt', ARGV[3], 'secret_digest', ARGV[4], 'signer_public_key', ARGV[5], 'scopes', ARGV[6], 'quota_requests', ARGV[7], 'quota_window_seconds', ARGV[8], 'epoch', ARGV[9], 'disabled', '0')
redis.call('SADD', KEYS[2], ARGV[1])
redis.call('XADD', KEYS[3], '*', 'previous', ARGV[12], 'chain', ARGV[13], 'event', ARGV[11])
redis.call('SET', KEYS[4], ARGV[13])
return {'issued'}
"#;

const ROTATE_SCRIPT: &str = r#"
local current = redis.call('GET', KEYS[5]) or ''
if current ~= ARGV[12] then return {'audit_retry'} end
if redis.call('HGET', KEYS[1], 'principal') ~= ARGV[1] or redis.call('HGET', KEYS[1], 'epoch') ~= ARGV[2] or redis.call('HGET', KEYS[1], 'disabled') ~= '0' then return {'conflict'} end
if redis.call('EXISTS', KEYS[2]) == 1 then return {'conflict'} end
if redis.call('SCARD', KEYS[3]) >= tonumber(ARGV[14]) then return {'limit'} end
redis.call('HSET', KEYS[1], 'disabled', '1', 'epoch', tostring(tonumber(ARGV[2]) + 1))
redis.call('HSET', KEYS[2], 'principal', ARGV[1], 'salt', ARGV[4], 'secret_digest', ARGV[5], 'signer_public_key', ARGV[6], 'scopes', ARGV[7], 'quota_requests', ARGV[8], 'quota_window_seconds', ARGV[9], 'epoch', ARGV[10], 'disabled', '0')
redis.call('SADD', KEYS[3], ARGV[3])
redis.call('XADD', KEYS[4], '*', 'previous', ARGV[12], 'chain', ARGV[13], 'event', ARGV[11]); redis.call('SET', KEYS[5], ARGV[13])
return {'rotated'}
"#;

const REVOKE_SCRIPT: &str = r#"
if redis.call('HGET', KEYS[1], 'principal') ~= ARGV[1] then return {'forbidden'} end
if redis.call('HGET', KEYS[1], 'disabled') == '1' then return {'already_revoked'} end
local current = redis.call('GET', KEYS[3]) or ''
if current ~= ARGV[3] then return {'audit_retry'} end
redis.call('HSET', KEYS[1], 'disabled', '1'); redis.call('HINCRBY', KEYS[1], 'epoch', 1)
redis.call('XADD', KEYS[2], '*', 'previous', ARGV[3], 'chain', ARGV[4], 'event', ARGV[2]); redis.call('SET', KEYS[3], ARGV[4])
return {'revoked'}
"#;

const RESERVE_SCRIPT: &str = r#"
if redis.call('HGET', KEYS[1], 'disabled') ~= '0' or redis.call('HGET', KEYS[1], 'epoch') ~= ARGV[1] then return {'revoked'} end
local existing = redis.call('HGET', KEYS[3], 'digest')
if existing then return {'existing', existing, redis.call('HGET', KEYS[3], 'state') or '', redis.call('HGET', KEYS[3], 'response') or '', redis.call('HGET', KEYS[3], 'receipt') or '', redis.call('HGET', KEYS[3], 'principal') or ''} end
local previous = redis.call('GET', KEYS[5]) or ''
if previous ~= ARGV[8] then return {'audit_retry'} end
local owner = redis.call('GET', KEYS[7])
if owner and owner ~= ARGV[10] then return {'conflict'} end
local operation = redis.call('GET', KEYS[8])
if operation and operation ~= KEYS[3] then return {'conflict'} end
local used = tonumber(redis.call('GET', KEYS[2]) or '0')
if used >= tonumber(ARGV[2]) then
 redis.call('XADD', KEYS[4], '*', 'previous', ARGV[8], 'chain', ARGV[9], 'event', ARGV[6], 'outcome', 'rate_limited'); redis.call('SET', KEYS[5], ARGV[9])
 return {'rate_limited', ARGV[3]}
end
redis.call('INCR', KEYS[2]); redis.call('EXPIRE', KEYS[2], ARGV[3])
local continuation_count = #ARGV - 11
redis.call('HSET', KEYS[3], 'digest', ARGV[5], 'state', 'pending', 'started_at', ARGV[7], 'principal', ARGV[10], 'activity_id', string.sub(KEYS[7], 18), 'idempotency_key', ARGV[11], 'continuation_count', continuation_count)
for index = 1, continuation_count do redis.call('HSET', KEYS[3], 'continuation_' .. tostring(index - 1), ARGV[11 + index]) end
redis.call('EXPIRE', KEYS[3], ARGV[4]); redis.call('SADD', KEYS[6], KEYS[3])
redis.call('SET', KEYS[7], ARGV[10], 'EX', ARGV[4])
redis.call('SET', KEYS[8], KEYS[3], 'EX', ARGV[4])
redis.call('XADD', KEYS[4], '*', 'previous', ARGV[8], 'chain', ARGV[9], 'event', ARGV[6], 'outcome', 'pending'); redis.call('SET', KEYS[5], ARGV[9])
return {'reserved'}
"#;

const CONSUME_SCRIPT: &str = r#"
if redis.call('HGET', KEYS[1], 'disabled') ~= '0' or redis.call('HGET', KEYS[1], 'epoch') ~= ARGV[1] then return {'revoked'} end
local previous = redis.call('GET', KEYS[4]) or ''
if previous ~= ARGV[5] then return {'audit_retry'} end
local used = tonumber(redis.call('GET', KEYS[2]) or '0')
local outcome = 'consumed'
if used >= tonumber(ARGV[2]) then outcome = 'rate_limited' else redis.call('INCR', KEYS[2]); redis.call('EXPIRE', KEYS[2], ARGV[3]) end
redis.call('XADD', KEYS[3], '*', 'previous', ARGV[5], 'chain', ARGV[6], 'event', ARGV[4], 'outcome', outcome); redis.call('SET', KEYS[4], ARGV[6])
return {outcome}
"#;

const COMPLETE_SCRIPT: &str = r#"
if redis.call('HGET', KEYS[1], 'digest') ~= ARGV[1] then return {'conflict'} end
if redis.call('HGET', KEYS[1], 'state') ~= 'pending' then return {'completed'} end
local previous = redis.call('GET', KEYS[4]) or ''
if previous ~= ARGV[6] then return {'audit_retry'} end
if ARGV[8] == '1' then
 local owner = redis.call('GET', KEYS[5])
 if owner and owner ~= ARGV[9] then return {'conflict'} end
 redis.call('SET', KEYS[5], ARGV[9], 'KEEPTTL')
end
redis.call('HSET', KEYS[1], 'state', ARGV[2], 'response', ARGV[3], 'receipt', ARGV[4]); if ARGV[2] ~= 'pending' then redis.call('SREM', KEYS[2], KEYS[1]) end
redis.call('XADD', KEYS[3], '*', 'previous', ARGV[6], 'chain', ARGV[7], 'event', ARGV[5], 'outcome', ARGV[2]); redis.call('SET', KEYS[4], ARGV[7])
return {'completed'}
"#;

const TAP_CONSUME_SCRIPT: &str = r#"
local previous = redis.call('GET', KEYS[4]) or ''
if previous ~= ARGV[18] then return {'audit_retry'} end
local existing = redis.call('GET', KEYS[1])
if existing then
 local outcome = 'replay'
 if existing == ARGV[2] then
  if redis.call('HGET', KEYS[2], 'principal') == ARGV[3] and redis.call('HGET', KEYS[2], 'key_id') == ARGV[4] and redis.call('HGET', KEYS[2], 'layerx_agent') == ARGV[5] and redis.call('HGET', KEYS[2], 'trusted_agent_id') == ARGV[6] and redis.call('HGET', KEYS[2], 'trusted_agent_domain') == ARGV[7] and redis.call('HGET', KEYS[2], 'intent') == ARGV[8] and redis.call('HGET', KEYS[2], 'evidence_digest') == ARGV[9] and redis.call('HGET', KEYS[2], 'activity_id') == ARGV[10] and redis.call('HGET', KEYS[2], 'signer_public_key') == ARGV[11] and redis.call('HGET', KEYS[2], 'target_authority') == ARGV[12] and redis.call('HGET', KEYS[2], 'target_path') == ARGV[13] and redis.call('HGET', KEYS[2], 'operation_identity') == ARGV[14] and redis.call('HGET', KEYS[2], 'credential_expires_at') == ARGV[15] then outcome = 'existing' else outcome = 'corrupt' end
 end
 redis.call('XADD', KEYS[3], '*', 'previous', ARGV[18], 'chain', ARGV[19], 'event', ARGV[17], 'outcome', outcome)
 redis.call('SET', KEYS[4], ARGV[19])
 if outcome == 'existing' then return {'existing'} end
 if outcome == 'corrupt' then return {'conflict'} end
 return {'replay'}
end
if redis.call('EXISTS', KEYS[2]) == 1 then return {'conflict'} end
local consumed = redis.call('SET', KEYS[1], ARGV[2], 'EX', ARGV[1], 'NX')
if not consumed then return {'conflict'} end
redis.call('HSET', KEYS[2], 'principal', ARGV[3], 'key_id', ARGV[4], 'layerx_agent', ARGV[5], 'trusted_agent_id', ARGV[6], 'trusted_agent_domain', ARGV[7], 'intent', ARGV[8], 'evidence_digest', ARGV[9], 'activity_id', ARGV[10], 'signer_public_key', ARGV[11], 'target_authority', ARGV[12], 'target_path', ARGV[13], 'operation_identity', ARGV[14], 'credential_expires_at', ARGV[15], 'consumed_at', ARGV[16])
redis.call('EXPIRE', KEYS[2], ARGV[1])
redis.call('XADD', KEYS[3], '*', 'previous', ARGV[18], 'chain', ARGV[19], 'event', ARGV[17], 'outcome', 'consumed')
redis.call('SET', KEYS[4], ARGV[19])
return {'consumed'}
"#;

fn validate_tap_record(
    registry_key: &str,
    nonce: &str,
    record: &TapCredentialRecord,
    now: u64,
    replay_until: u64,
) -> Result<(), String> {
    validate_tap_record_fields(record)?;
    if registry_key != record.key_id
        || !bounded_text(registry_key, 512)
        || !bounded_text(nonce, 512)
        || replay_until <= now
        || replay_until - now > MAX_TAP_REPLAY_SECONDS
        || replay_until < record.credential_expires_at
    {
        return Err("gateway TAP credential binding is invalid".to_owned());
    }
    Ok(())
}

fn validate_tap_record_fields(record: &TapCredentialRecord) -> Result<(), String> {
    if !valid_hex_digest(&record.principal_digest)
        || !valid_hex_digest(&record.layerx_agent)
        || !bounded_text(&record.key_id, 512)
        || !bounded_text(&record.trusted_agent_id, 512)
        || !bounded_text(&record.trusted_agent_domain, 2048)
        || !record.trusted_agent_domain.starts_with("https://")
        || !matches!(record.intent.as_str(), "browse" | "pay")
        || !valid_hex_digest(&record.evidence_digest)
        || record
            .activity_id
            .as_ref()
            .is_some_and(|value| !valid_hex_digest(value))
        || !valid_hex_digest(&record.signer_public_key)
        || !canonical_target_authority(&record.target_authority)
        || !canonical_target_path(&record.target_path)
        || !valid_hex_digest(&record.operation_identity)
        || record.credential_expires_at == 0
    {
        return Err("gateway TAP credential binding is invalid".to_owned());
    }
    Ok(())
}

fn bounded_text(value: &str, maximum: usize) -> bool {
    !value.is_empty()
        && value.len() <= maximum
        && !value.bytes().any(|byte| byte.is_ascii_control())
}

fn canonical_target_authority(value: &str) -> bool {
    if !bounded_text(value, 512)
        || value != value.to_ascii_lowercase()
        || value.contains(['/', '@', '?', '#', '\\'])
    {
        return false;
    }
    let (host, port) = value.rsplit_once(':').map_or((value, None), |(host, port)| {
        (host, Some(port))
    });
    if host.is_empty()
        || host.contains(':')
        || host.ends_with('.')
        || port.is_some_and(|port| {
            port.parse::<u16>().map_or(true, |number| {
                number == 0 || number.to_string() != port
            })
        })
    {
        return false;
    }
    !host.split('.').any(|label| {
        label.is_empty()
            || label.len() > 63
            || !label
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-')
            || !label
                .as_bytes()
                .first()
                .is_some_and(u8::is_ascii_alphanumeric)
            || !label
                .as_bytes()
                .last()
                .is_some_and(u8::is_ascii_alphanumeric)
    })
}

fn canonical_target_path(value: &str) -> bool {
    value == "/"
        || (bounded_text(value, 512)
            && value.starts_with('/')
            && !value.ends_with('/')
            && !value.split('/').skip(1).any(|segment| {
                segment.is_empty()
                    || matches!(segment, "." | "..")
                    || !segment.bytes().all(|byte| {
                        byte.is_ascii_alphanumeric()
                            || matches!(byte, b'-' | b'_' | b'.' | b'~')
                    })
            }))
}

fn valid_hex_digest(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || matches!(byte, b'a'..=b'f'))
}

fn tap_binding_digest(record: &TapCredentialRecord) -> String {
    digest(&[
        b"gateway-tap-binding-v1",
        record.principal_digest.as_bytes(),
        record.key_id.as_bytes(),
        record.layerx_agent.as_bytes(),
        record.trusted_agent_id.as_bytes(),
        record.trusted_agent_domain.as_bytes(),
        record.intent.as_bytes(),
        record.evidence_digest.as_bytes(),
        record.activity_id.as_deref().unwrap_or("").as_bytes(),
        record.signer_public_key.as_bytes(),
        record.target_authority.as_bytes(),
        record.target_path.as_bytes(),
        record.operation_identity.as_bytes(),
        &record.credential_expires_at.to_be_bytes(),
    ])
}

fn digest(parts: &[&[u8]]) -> String {
    let mut digest = Sha256::new();
    for part in parts {
        digest.update((part.len() as u64).to_be_bytes());
        digest.update(part);
    }
    format!("{:x}", digest.finalize())
}

fn write_command(stream: &mut impl Write, arguments: &[&str]) -> Result<(), String> {
    write!(stream, "*{}\r\n", arguments.len()).map_err(|error| error.to_string())?;
    for argument in arguments {
        write!(stream, "${}\r\n", argument.len()).map_err(|error| error.to_string())?;
        stream
            .write_all(argument.as_bytes())
            .map_err(|e| e.to_string())?;
        stream.write_all(b"\r\n").map_err(|e| e.to_string())?;
    }
    stream.flush().map_err(|error| error.to_string())
}

fn read_resp(stream: &mut impl Read, depth: usize) -> Result<Resp, String> {
    if depth > 4 {
        return Err("gateway Redis nesting exceeds its bound".to_owned());
    }
    let mut prefix = [0_u8; 1];
    stream.read_exact(&mut prefix).map_err(|e| e.to_string())?;
    let line = read_line(stream)?;
    match prefix[0] {
        b'+' => Ok(Resp::Simple(line)),
        b'-' => Err("gateway Redis returned an error".to_owned()),
        b':' => line
            .parse::<i64>()
            .map(Resp::Integer)
            .map_err(|_| "gateway Redis integer is invalid".to_owned()),
        b'$' => {
            let length = line
                .parse::<i64>()
                .map_err(|_| "gateway Redis bulk length is invalid".to_owned())?;
            if length == -1 {
                return Ok(Resp::Bulk(None));
            }
            let length = usize::try_from(length)
                .map_err(|_| "gateway Redis bulk length is invalid".to_owned())?;
            if length > MAX_RESPONSE {
                return Err("gateway Redis bulk exceeds its bound".to_owned());
            }
            let mut value = vec![0_u8; length];
            stream.read_exact(&mut value).map_err(|e| e.to_string())?;
            let mut end = [0_u8; 2];
            stream.read_exact(&mut end).map_err(|e| e.to_string())?;
            if end != *b"\r\n" {
                return Err("gateway Redis bulk is malformed".to_owned());
            }
            Ok(Resp::Bulk(Some(value)))
        }
        b'*' => {
            let length = line
                .parse::<usize>()
                .map_err(|_| "gateway Redis array length is invalid".to_owned())?;
            if length > 128 {
                return Err("gateway Redis array exceeds its bound".to_owned());
            }
            let mut values = Vec::with_capacity(length);
            for _ in 0..length {
                values.push(read_resp(stream, depth + 1)?);
            }
            Ok(Resp::Array(values))
        }
        _ => Err("gateway Redis response prefix is invalid".to_owned()),
    }
}

fn read_line(stream: &mut impl Read) -> Result<String, String> {
    let mut value = Vec::new();
    loop {
        let mut byte = [0_u8; 1];
        stream.read_exact(&mut byte).map_err(|e| e.to_string())?;
        if byte[0] == b'\r' {
            stream.read_exact(&mut byte).map_err(|e| e.to_string())?;
            if byte[0] != b'\n' {
                return Err("gateway Redis line is malformed".to_owned());
            }
            break;
        }
        if value.len() >= 4096 {
            return Err("gateway Redis line exceeds its bound".to_owned());
        }
        value.push(byte[0]);
    }
    String::from_utf8(value).map_err(|_| "gateway Redis line is not UTF-8".to_owned())
}

fn text(value: &Resp) -> Option<String> {
    match value {
        Resp::Simple(value) => Some(value.clone()),
        Resp::Bulk(Some(value)) => String::from_utf8(value.clone()).ok(),
        Resp::Integer(value) => Some(value.to_string()),
        Resp::Bulk(None) | Resp::Array(_) => None,
    }
}

fn pairs(values: &[Resp]) -> Result<BTreeMap<String, String>, String> {
    if values.len() % 2 != 0 {
        return Err("gateway Redis hash is malformed".to_owned());
    }
    let mut fields = BTreeMap::new();
    for pair in values.chunks_exact(2) {
        let name = text(&pair[0]).ok_or_else(|| "gateway Redis hash name is invalid".to_owned())?;
        let value =
            text(&pair[1]).ok_or_else(|| "gateway Redis hash value is invalid".to_owned())?;
        fields.insert(name, value);
    }
    Ok(fields)
}

fn required(fields: &BTreeMap<String, String>, name: &str) -> Result<String, String> {
    fields
        .get(name)
        .cloned()
        .ok_or_else(|| format!("gateway key omitted {name}"))
}

fn continuation_chunks(value: &str) -> Result<Vec<&str>, String> {
    if value.len() > MAX_CONTINUATION_BYTES {
        return Err("gateway continuation exceeds its bound".to_owned());
    }
    let mut chunks = Vec::new();
    let mut start = 0;
    while start < value.len() {
        let mut end = (start + CONTINUATION_CHUNK_BYTES).min(value.len());
        while end > start && !value.is_char_boundary(end) {
            end -= 1;
        }
        if end == start || chunks.len() == MAX_CONTINUATION_CHUNKS {
            return Err("gateway continuation chunking failed".to_owned());
        }
        chunks.push(&value[start..end]);
        start = end;
    }
    Ok(chunks)
}

fn durable_continuation(fields: &BTreeMap<String, String>) -> Result<String, String> {
    if let Some(value) = fields.get("continuation") {
        if value.len() > MAX_CONTINUATION_BYTES {
            return Err("gateway continuation exceeds its bound".to_owned());
        }
        return Ok(value.clone());
    }
    let count = fields
        .get("continuation_count")
        .map_or(Ok(0), |value| value.parse::<usize>())
        .map_err(|_| "gateway continuation count is invalid".to_owned())?;
    if count > MAX_CONTINUATION_CHUNKS {
        return Err("gateway continuation count exceeds its bound".to_owned());
    }
    let mut continuation = String::new();
    for index in 0..count {
        continuation.push_str(required(fields, &format!("continuation_{index}"))?.as_str());
        if continuation.len() > MAX_CONTINUATION_BYTES {
            return Err("gateway continuation exceeds its bound".to_owned());
        }
    }
    Ok(continuation)
}

fn number(fields: &BTreeMap<String, String>, name: &str) -> Result<u64, String> {
    required(fields, name)?
        .parse::<u64>()
        .map_err(|_| format!("gateway key {name} is invalid"))
}

fn array_tag(response: Resp) -> Result<String, String> {
    let Resp::Array(values) = response else {
        return Err("gateway Redis script response is invalid".to_owned());
    };
    values
        .first()
        .and_then(text)
        .ok_or_else(|| "gateway Redis script tag is invalid".to_owned())
}
