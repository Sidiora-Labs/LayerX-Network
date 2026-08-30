//! Principal-scoped append-only live event journal.

use super::backend::ApiFailure;
use crate::store::{PrincipalScope, RowKey, StoreError, Table};
use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine as _};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sha2::{Digest as _, Sha256};

const HEAD: &str = "stream-head";
const DOMAIN: &[u8] = b"layerx-human-stream-cursor/v1";
const MAX_PAGE: usize = 100;

#[derive(Serialize, Deserialize)]
struct Event {
    sequence: u64,
    source: String,
    kind: String,
    observed_at: u64,
    payload: Value,
}

pub struct StreamJournal {
    key: [u8; 32],
}
impl StreamJournal {
    pub const fn new(key: [u8; 32]) -> Self {
        Self { key }
    }

    pub fn append(
        &self,
        scope: &mut PrincipalScope<'_>,
        source: &str,
        kind: &str,
        observed_at: u64,
        payload: Value,
    ) -> Result<(), ApiFailure> {
        if source.is_empty()
            || source.len() > 128
            || !matches!(
                kind,
                "journey-progress"
                    | "approval-created"
                    | "approval-approved"
                    | "approval-rejected"
                    | "approval-expired"
                    | "notification"
            )
        {
            return Err(ApiFailure::upstream_degraded());
        }
        let source_digest: [u8; 32] = Sha256::digest(source.as_bytes()).into();
        let source_key =
            RowKey::new(format!("stream-source-{}", hex(&source_digest))).map_err(store_failure)?;
        if scope.get(Table::Stream, &source_key).is_some() {
            return Ok(());
        }
        let head_key = RowKey::new(HEAD).map_err(store_failure)?;
        let sequence = scope
            .get(Table::Stream, &head_key)
            .map(|row| decode_u64(row.bytes()))
            .transpose()?
            .unwrap_or(0)
            .checked_add(1)
            .ok_or_else(ApiFailure::unavailable)?;
        let event = Event {
            sequence,
            source: source.to_owned(),
            kind: kind.to_owned(),
            observed_at,
            payload,
        };
        let bytes = serde_json::to_vec(&event).map_err(|_| ApiFailure::upstream_degraded())?;
        let event_key =
            RowKey::new(format!("stream-event-{sequence:016x}")).map_err(store_failure)?;
        scope
            .put(Table::Stream, event_key, observed_at, bytes)
            .map_err(store_failure)?;
        scope
            .put(
                Table::Stream,
                source_key,
                observed_at,
                sequence.to_be_bytes().to_vec(),
            )
            .map_err(store_failure)?;
        scope
            .put(
                Table::Stream,
                head_key,
                observed_at,
                sequence.to_be_bytes().to_vec(),
            )
            .map_err(store_failure)
    }

    pub fn open(&self, scope: &PrincipalScope<'_>) -> Result<Value, ApiFailure> {
        let position = self.head(scope)?;
        Ok(json!({"cursor":self.cursor(scope,position)}))
    }
    pub fn next(&self, scope: &PrincipalScope<'_>, cursor: &str) -> Result<Value, ApiFailure> {
        let after = self.decode_cursor(scope, cursor)?;
        let head = self.head(scope)?;
        if after > head {
            return Err(ApiFailure::invalid_request(Some("cursor")));
        }
        let mut events = Vec::new();
        let through = head.min(after.saturating_add(MAX_PAGE as u64));
        for sequence in after.saturating_add(1)..=through {
            let key =
                RowKey::new(format!("stream-event-{sequence:016x}")).map_err(store_failure)?;
            let row = scope
                .get(Table::Stream, &key)
                .ok_or_else(ApiFailure::unavailable)?;
            let event: Event =
                serde_json::from_slice(row.bytes()).map_err(|_| ApiFailure::upstream_degraded())?;
            if event.sequence != sequence {
                return Err(ApiFailure::upstream_degraded());
            }
            let journey = if event.kind == "journey-progress" {
                event
                    .source
                    .strip_prefix("journey:")
                    .and_then(|value| value.rsplit_once(':').map(|pair| pair.0))
                    .map(|value| {
                        crate::notify::JourneyId::new(value.to_owned())
                            .map_err(|_| ApiFailure::upstream_degraded())
                    })
                    .transpose()?
                    .map(|id| {
                        crate::journeys::JourneyEngine::load(scope, &id)
                            .map_err(|_| ApiFailure::upstream_degraded())
                    })
                    .transpose()?
                    .flatten()
                    .map(|journey| super::production_reads::journey_json(&journey))
                    .transpose()?
            } else {
                None
            };
            events.push(json!({"cursor":self.cursor(scope,sequence),"kind":event.kind,"observed_at":event.observed_at,"journey":journey.or_else(||event.payload.get("journey").cloned()),"approval":event.payload.get("approval"),"notification":event.payload.get("notification")}));
        }
        Ok(json!({"events":events,"next_cursor":self.cursor(scope,through)}))
    }
    fn head(&self, scope: &PrincipalScope<'_>) -> Result<u64, ApiFailure> {
        let key = RowKey::new(HEAD).map_err(store_failure)?;
        scope
            .get(Table::Stream, &key)
            .map(|row| decode_u64(row.bytes()))
            .transpose()
            .map(|v| v.unwrap_or(0))
    }
    fn cursor(&self, scope: &PrincipalScope<'_>, position: u64) -> String {
        let principal = Sha256::digest(scope.principal().as_str().as_bytes());
        let mut body = Vec::with_capacity(72);
        body.extend_from_slice(&principal);
        body.extend_from_slice(&position.to_be_bytes());
        let mut mac = Sha256::new();
        mac.update(DOMAIN);
        mac.update(self.key);
        mac.update(&body);
        body.extend_from_slice(&mac.finalize());
        format!("cur_{}", URL_SAFE_NO_PAD.encode(body))
    }
    fn decode_cursor(&self, scope: &PrincipalScope<'_>, cursor: &str) -> Result<u64, ApiFailure> {
        let bytes = cursor
            .strip_prefix("cur_")
            .and_then(|v| URL_SAFE_NO_PAD.decode(v).ok())
            .filter(|v| v.len() == 72)
            .ok_or_else(|| ApiFailure::invalid_request(Some("cursor")))?;
        let principal = Sha256::digest(scope.principal().as_str().as_bytes());
        if bytes[..32] != principal[..] {
            return Err(ApiFailure::forbidden());
        }
        let mut mac = Sha256::new();
        mac.update(DOMAIN);
        mac.update(self.key);
        mac.update(&bytes[..40]);
        if bytes[40..] != mac.finalize()[..] {
            return Err(ApiFailure::forbidden());
        }
        Ok(u64::from_be_bytes(bytes[32..40].try_into().map_err(
            |_| ApiFailure::invalid_request(Some("cursor")),
        )?))
    }
}
fn decode_u64(bytes: &[u8]) -> Result<u64, ApiFailure> {
    Ok(u64::from_be_bytes(
        bytes
            .try_into()
            .map_err(|_| ApiFailure::upstream_degraded())?,
    ))
}
fn store_failure(error: StoreError) -> ApiFailure {
    match error {
        StoreError::Io(_) => ApiFailure::unavailable(),
        _ => ApiFailure::upstream_degraded(),
    }
}
fn hex(bytes: &[u8]) -> String {
    const H: &[u8; 16] = b"0123456789abcdef";
    bytes
        .iter()
        .flat_map(|b| {
            [
                char::from(H[usize::from(b >> 4)]),
                char::from(H[usize::from(b & 15)]),
            ]
        })
        .collect()
}
