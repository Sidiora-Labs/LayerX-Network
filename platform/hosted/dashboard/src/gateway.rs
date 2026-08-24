use layerx_platform_webhooks::encoding::hex_encode;
use layerx_platform_webhooks::events::{Principal, Verification};
use native_tls::{Certificate, TlsConnector};
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;
use std::env;
use std::fs;
use std::io::{Read, Write};
use std::net::{TcpStream, ToSocketAddrs};
use std::time::Duration;
use zeroize::{Zeroize, Zeroizing};

use crate::error::DashboardError;
use crate::model::{
    per_mille, KeyView, RequestOutcome, RequestRecord, RequestSummary, UsageSummary,
};

const CONNECT_TIMEOUT: Duration = Duration::from_secs(3);
const IO_TIMEOUT: Duration = Duration::from_secs(8);
const MAX_RESPONSE: usize = 2 * 1024 * 1024;
const MAX_KEYS: usize = 128;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Snapshot {
    pub keys: Vec<KeyView>,
    pub usage: UsageSummary,
    pub requests: RequestSummary,
    pub recent_requests: Vec<RequestRecord>,
}

#[derive(Clone)]
struct Endpoint {
    host: String,
    port: u16,
}

enum Resp {
    Simple(String),
    Bulk(Option<Vec<u8>>),
    Integer(i64),
    Array(Vec<Resp>),
}

pub struct Store {
    endpoint: Endpoint,
    ca: Certificate,
    username: Zeroizing<String>,
    password: Zeroizing<String>,
}

impl Store {
    pub fn from_environment() -> Result<Self, String> {
        let endpoint = parse_endpoint(
            &env::var("LAYERX_DASHBOARD_GATEWAY_REDIS_URL")
                .map_err(|_| "dashboard gateway Redis URL is required")?,
        )?;
        let ca = Certificate::from_der(
            &fs::read(
                env::var("LAYERX_DASHBOARD_REDIS_CA_DER")
                    .map_err(|_| "dashboard Redis CA is required")?,
            )
            .map_err(|error| error.to_string())?,
        )
        .map_err(|error| error.to_string())?;
        Ok(Self {
            endpoint,
            ca,
            username: read_secret("LAYERX_DASHBOARD_GATEWAY_REDIS_USERNAME_FILE")?,
            password: read_secret("LAYERX_DASHBOARD_GATEWAY_REDIS_PASSWORD_FILE")?,
        })
    }

    pub fn ready(&self) -> bool {
        matches!(self.command(&["PING"]), Ok(Resp::Simple(value)) if value == "PONG")
    }

    pub fn keys(&self, principal: &Principal, now: u64) -> Result<Vec<KeyView>, DashboardError> {
        let digest = principal_digest(principal);
        let ids = self.key_ids(&digest)?;
        let mut keys = Vec::with_capacity(ids.len());
        for id in ids {
            let key = format!("gateway:key:{id}");
            let values = self
                .command(&[
                    "HMGET",
                    &key,
                    "principal",
                    "quota_requests",
                    "quota_window_seconds",
                    "disabled",
                ])
                .map_err(|_| DashboardError::CorruptStore)?;
            let Resp::Array(fields) = values else {
                return Err(DashboardError::CorruptStore);
            };
            if fields.len() != 4 || text(&fields[0]).as_deref() != Some(digest.as_str()) {
                return Err(DashboardError::CorruptStore);
            }
            let allowed = number(&fields[1])?;
            let window = number(&fields[2])?;
            if allowed == 0 || window == 0 {
                return Err(DashboardError::CorruptStore);
            }
            let disabled = text(&fields[3]).as_deref() == Some("1");
            let window_number = now / window;
            let window_started_at = window_number.saturating_mul(window);
            let usage_key = format!("gateway:quota:{id}:{window_number}");
            let used = match self
                .command(&["GET", &usage_key])
                .map_err(|_| DashboardError::CorruptStore)?
            {
                Resp::Bulk(None) => 0,
                value => number(&value)?,
            };
            keys.push(KeyView {
                key_id: id,
                principal: principal.as_str().to_owned(),
                disabled,
                requests_per_window: allowed,
                window_seconds: window,
                used_in_window: used,
                remaining_in_window: allowed.saturating_sub(used),
                window_started_at,
                window_resets_at: window_started_at.saturating_add(window),
                window_lapsed: false,
                utilisation_per_mille: per_mille(used, allowed),
            });
        }
        Ok(keys)
    }

    pub fn usage(&self, principal: &Principal, now: u64) -> Result<UsageSummary, DashboardError> {
        Ok(usage_summary(&self.keys(principal, now)?))
    }

    pub fn requests(
        &self,
        principal: &Principal,
        limit: usize,
    ) -> Result<Vec<RequestRecord>, DashboardError> {
        let digest = principal_digest(principal);
        let count = limit.clamp(1, 200).saturating_mul(8).min(1600);
        let response = self
            .command(&[
                "XREVRANGE",
                "gateway:audit",
                "+",
                "-",
                "COUNT",
                &count.to_string(),
            ])
            .map_err(|_| DashboardError::CorruptStore)?;
        let Resp::Array(entries) = response else {
            return Err(DashboardError::CorruptStore);
        };
        let mut records = Vec::new();
        for entry in entries {
            let Resp::Array(parts) = entry else {
                return Err(DashboardError::CorruptStore);
            };
            if parts.len() != 2 {
                return Err(DashboardError::CorruptStore);
            }
            let id = text(&parts[0]).ok_or(DashboardError::CorruptStore)?;
            let fields = pairs(&parts[1])?;
            let event = fields.get("event").ok_or(DashboardError::CorruptStore)?;
            let Some(operation_digest) = event.strip_prefix(&format!("{digest}:")) else {
                continue;
            };
            let outcome = match fields.get("outcome").map(String::as_str) {
                Some("rate_limited") => RequestOutcome::RateLimited,
                Some("receipt_verified" | "completed") => RequestOutcome::Completed,
                Some("pending") | None => RequestOutcome::Pending,
                Some(_) => RequestOutcome::Refused,
            };
            let at = id
                .split_once('-')
                .and_then(|(milliseconds, _)| milliseconds.parse::<u64>().ok())
                .map(|milliseconds| milliseconds / 1_000)
                .ok_or(DashboardError::CorruptStore)?;
            records.push(RequestRecord {
                at,
                operation: None,
                operation_digest: operation_digest.to_owned(),
                outcome,
                verification: Verification::Unverified,
            });
            if records.len() >= limit.clamp(1, 200) {
                break;
            }
        }
        Ok(records)
    }

    pub fn snapshot(
        &self,
        principal: &Principal,
        now: u64,
        limit: usize,
    ) -> Result<Snapshot, DashboardError> {
        let keys = self.keys(principal, now)?;
        let recent_requests = self.requests(principal, limit)?;
        Ok(Snapshot {
            usage: usage_summary(&keys),
            keys,
            requests: request_summary(&recent_requests),
            recent_requests,
        })
    }

    fn key_ids(&self, principal_digest: &str) -> Result<Vec<String>, DashboardError> {
        let key = format!("gateway:principal:{principal_digest}:keys");
        let Resp::Array(values) = self
            .command(&["SMEMBERS", &key])
            .map_err(|_| DashboardError::CorruptStore)?
        else {
            return Err(DashboardError::CorruptStore);
        };
        if values.len() > MAX_KEYS {
            return Err(DashboardError::CorruptStore);
        }
        let mut ids = values
            .iter()
            .map(text)
            .collect::<Option<Vec<_>>>()
            .ok_or(DashboardError::CorruptStore)?;
        ids.sort();
        Ok(ids)
    }

    fn command(&self, arguments: &[&str]) -> Result<Resp, String> {
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
                    if !matches!(read_resp(&mut stream, 0)?, Resp::Simple(value) if value == "OK") {
                        return Err("dashboard Redis authentication failed".to_owned());
                    }
                    write_resp(&mut stream, arguments)?;
                    return read_resp(&mut stream, 0);
                }
                Err(error) => last = Some(error),
            }
        }
        Err(last.map_or_else(
            || "dashboard Redis did not resolve".to_owned(),
            |error| error.to_string(),
        ))
    }
}

fn usage_summary(keys: &[KeyView]) -> UsageSummary {
    let mut summary = UsageSummary::default();
    for key in keys {
        summary.keys = summary.keys.saturating_add(1);
        if key.disabled {
            summary.disabled_keys = summary.disabled_keys.saturating_add(1);
        } else {
            summary.live_keys = summary.live_keys.saturating_add(1);
            summary.requests_allowed = summary
                .requests_allowed
                .saturating_add(key.requests_per_window);
            summary.requests_used = summary.requests_used.saturating_add(key.used_in_window);
            summary.requests_remaining = summary
                .requests_remaining
                .saturating_add(key.remaining_in_window);
        }
    }
    summary.utilisation_per_mille = per_mille(summary.requests_used, summary.requests_allowed);
    summary
}

fn request_summary(records: &[RequestRecord]) -> RequestSummary {
    let mut summary = RequestSummary::default();
    for record in records {
        summary.records = summary.records.saturating_add(1);
        match record.outcome {
            RequestOutcome::Pending => summary.pending = summary.pending.saturating_add(1),
            RequestOutcome::Completed => summary.completed = summary.completed.saturating_add(1),
            RequestOutcome::RateLimited => {
                summary.rate_limited = summary.rate_limited.saturating_add(1);
            }
            RequestOutcome::Refused => summary.refused = summary.refused.saturating_add(1),
        }
        summary.first_at = Some(summary.first_at.map_or(record.at, |at| at.min(record.at)));
        summary.last_at = Some(summary.last_at.map_or(record.at, |at| at.max(record.at)));
    }
    summary
}

fn principal_digest(principal: &Principal) -> String {
    hex_encode(&Sha256::digest(principal.as_str().as_bytes()))
}

fn parse_endpoint(value: &str) -> Result<Endpoint, String> {
    let authority = value
        .strip_prefix("rediss://")
        .ok_or_else(|| "dashboard Redis endpoint must use rediss".to_owned())?
        .trim_end_matches('/');
    if authority.is_empty() || authority.contains(['@', '/', '?', '#', '\\']) {
        return Err("dashboard Redis endpoint is not canonical".to_owned());
    }
    let (host, port) = authority.rsplit_once(':').map_or_else(
        || Ok::<_, String>((authority.to_owned(), 6379)),
        |(host, port)| {
            Ok((
                host.to_owned(),
                port.parse::<u16>()
                    .map_err(|_| "dashboard Redis port is invalid".to_owned())?,
            ))
        },
    )?;
    if host.is_empty() {
        return Err("dashboard Redis host is missing".to_owned());
    }
    Ok(Endpoint { host, port })
}

fn read_secret(name: &str) -> Result<Zeroizing<String>, String> {
    let mut value = fs::read_to_string(env::var(name).map_err(|_| format!("{name} is required"))?)
        .map_err(|error| error.to_string())?;
    while matches!(value.as_bytes().last(), Some(b'\r' | b'\n')) {
        value.pop();
    }
    if value.is_empty() || value.len() > 4096 {
        value.zeroize();
        return Err(format!("{name} is empty or oversized"));
    }
    Ok(Zeroizing::new(value))
}

fn number(value: &Resp) -> Result<u64, DashboardError> {
    text(value)
        .ok_or(DashboardError::CorruptStore)?
        .parse::<u64>()
        .map_err(|_| DashboardError::CorruptStore)
}

fn pairs(value: &Resp) -> Result<BTreeMap<String, String>, DashboardError> {
    let Resp::Array(values) = value else {
        return Err(DashboardError::CorruptStore);
    };
    if !values.len().is_multiple_of(2) {
        return Err(DashboardError::CorruptStore);
    }
    values
        .chunks_exact(2)
        .map(|pair| {
            Ok((
                text(&pair[0]).ok_or(DashboardError::CorruptStore)?,
                text(&pair[1]).ok_or(DashboardError::CorruptStore)?,
            ))
        })
        .collect()
}

fn text(value: &Resp) -> Option<String> {
    match value {
        Resp::Simple(value) => Some(value.clone()),
        Resp::Bulk(Some(value)) => String::from_utf8(value.clone()).ok(),
        Resp::Integer(value) => Some(value.to_string()),
        Resp::Bulk(None) | Resp::Array(_) => None,
    }
}

fn write_resp(stream: &mut impl Write, arguments: &[&str]) -> Result<(), String> {
    write!(stream, "*{}\r\n", arguments.len()).map_err(|error| error.to_string())?;
    for argument in arguments {
        write!(stream, "${}\r\n{}\r\n", argument.len(), argument)
            .map_err(|error| error.to_string())?;
    }
    stream.flush().map_err(|error| error.to_string())
}

fn read_resp(stream: &mut impl Read, depth: usize) -> Result<Resp, String> {
    if depth > 8 {
        return Err("dashboard Redis nesting is excessive".to_owned());
    }
    let mut marker = [0_u8; 1];
    stream
        .read_exact(&mut marker)
        .map_err(|error| error.to_string())?;
    match marker[0] {
        b'+' => Ok(Resp::Simple(read_line(stream)?)),
        b'-' => Err(format!(
            "dashboard Redis refused command: {}",
            read_line(stream)?
        )),
        b':' => read_line(stream)?
            .parse::<i64>()
            .map(Resp::Integer)
            .map_err(|_| "dashboard Redis integer is invalid".to_owned()),
        b'$' => {
            let length = read_line(stream)?
                .parse::<i64>()
                .map_err(|_| "dashboard Redis bulk length is invalid".to_owned())?;
            if length == -1 {
                return Ok(Resp::Bulk(None));
            }
            let length = usize::try_from(length)
                .map_err(|_| "dashboard Redis bulk length is invalid".to_owned())?;
            if length > MAX_RESPONSE {
                return Err("dashboard Redis response exceeds its bound".to_owned());
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
                return Err("dashboard Redis response is malformed".to_owned());
            }
            Ok(Resp::Bulk(Some(bytes)))
        }
        b'*' => {
            let length = read_line(stream)?
                .parse::<usize>()
                .map_err(|_| "dashboard Redis array length is invalid".to_owned())?;
            if length > 10_000 {
                return Err("dashboard Redis array exceeds its bound".to_owned());
            }
            let mut values = Vec::with_capacity(length);
            for _ in 0..length {
                values.push(read_resp(stream, depth + 1)?);
            }
            Ok(Resp::Array(values))
        }
        _ => Err("dashboard Redis response marker is invalid".to_owned()),
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
            return Err("dashboard Redis response line is excessive".to_owned());
        }
        if bytes.ends_with(b"\r\n") {
            bytes.truncate(bytes.len().saturating_sub(2));
            return String::from_utf8(bytes)
                .map_err(|_| "dashboard Redis response is not UTF-8".to_owned());
        }
    }
}
