use native_tls::{Certificate, TlsConnector};
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;
use std::io::{Read, Write};
use std::net::{TcpStream, ToSocketAddrs};
use std::time::Duration;
use zeroize::Zeroizing;

const CONNECT_TIMEOUT: Duration = Duration::from_secs(3);
const IO_TIMEOUT: Duration = Duration::from_secs(8);
const MAX_RESPONSE: usize = 256 * 1024;
const AUDIT_ATTEMPTS: usize = 8;
const MAX_KEYS_PER_PRINCIPAL: u64 = 128;

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
    },
    RateLimited {
        retry_after_seconds: u64,
    },
    Revoked,
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
        principal_digest: &str,
        audit_event: &str,
    ) -> Result<Reservation, String> {
        let key = format!("gateway:key:{}", record.key_id);
        let window = now / record.quota_window_seconds;
        let usage = format!("gateway:quota:{}:{window}", record.key_id);
        let idem = format!("gateway:idem:{idempotency_scope}");
        let owner = format!("gateway:activity:{activity_id}");
        let retry = record.quota_window_seconds - now % record.quota_window_seconds;
        for _ in 0..AUDIT_ATTEMPTS {
            let (head, chain) = self.audit_values(audit_event)?;
            let response = self.command(&[
                "EVAL",
                RESERVE_SCRIPT,
                "7",
                &key,
                &usage,
                &idem,
                "gateway:audit",
                "gateway:audit:head",
                "gateway:pending",
                &owner,
                &record.epoch.to_string(),
                &record.quota_requests.to_string(),
                &retry.to_string(),
                &retention_seconds.to_string(),
                request_digest,
                audit_event,
                &now.to_string(),
                &head,
                &chain,
                principal_digest,
            ])?;
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
                    })
                }
                _ => return Err("gateway reservation state is invalid".to_owned()),
            }
        }
        Err("gateway audit head remained contended".to_owned())
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
        Ok((head, format!("{digest:x}")))
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
if existing then return {'existing', existing, redis.call('HGET', KEYS[3], 'state') or '', redis.call('HGET', KEYS[3], 'response') or '', redis.call('HGET', KEYS[3], 'receipt') or ''} end
local previous = redis.call('GET', KEYS[5]) or ''
if previous ~= ARGV[8] then return {'audit_retry'} end
local owner = redis.call('GET', KEYS[7])
if owner and owner ~= ARGV[10] then return {'conflict'} end
local used = tonumber(redis.call('GET', KEYS[2]) or '0')
if used >= tonumber(ARGV[2]) then
 redis.call('XADD', KEYS[4], '*', 'previous', ARGV[8], 'chain', ARGV[9], 'event', ARGV[6], 'outcome', 'rate_limited'); redis.call('SET', KEYS[5], ARGV[9])
 return {'rate_limited', ARGV[3]}
end
redis.call('INCR', KEYS[2]); redis.call('EXPIRE', KEYS[2], ARGV[3])
redis.call('HSET', KEYS[3], 'digest', ARGV[5], 'state', 'pending', 'started_at', ARGV[7]); redis.call('EXPIRE', KEYS[3], ARGV[4]); redis.call('SADD', KEYS[6], KEYS[3])
redis.call('SET', KEYS[7], ARGV[10])
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
 redis.call('SET', KEYS[5], ARGV[9])
end
redis.call('HSET', KEYS[1], 'state', ARGV[2], 'response', ARGV[3], 'receipt', ARGV[4]); redis.call('SREM', KEYS[2], KEYS[1])
redis.call('XADD', KEYS[3], '*', 'previous', ARGV[6], 'chain', ARGV[7], 'event', ARGV[5], 'outcome', ARGV[2]); redis.call('SET', KEYS[4], ARGV[7])
return {'completed'}
"#;

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
