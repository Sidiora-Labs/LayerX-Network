//! The published `LayerX` webhook signature scheme, version one.
//!
//! Every delivery carries four authenticated headers:
//!
//! ```text
//! layerx-webhook-id:        <event-id>
//! layerx-webhook-timestamp: <unix-seconds>
//! layerx-webhook-key-id:    <endpoint-signing-key-id>
//! layerx-webhook-signature: v1=<standard padded base64 of a 64-byte Ed25519 signature>
//! ```
//!
//! The signed message is the UTF-8 string `"<id>.<timestamp>."` followed by the
//! exact request body bytes, and the signature is Ed25519 under the endpoint's
//! own signing key. This is the scheme the shipped buyer, seller, merchant and
//! agent middleware and the Express, Next, `FastAPI` and Spring integrations
//! verify, so a hosted delivery is consumable by an unmodified integration.
//!
//! `layerx-webhook-id` carries the *event* identifier, not the attempt: every
//! attempt and every redelivery of one event repeats the same identifier over a
//! byte-identical body, which is what lets a receiver deduplicate an
//! at-least-once repeat instead of treating it as a conflicting delivery.
//!
//! Replay protection is two sided. The timestamp must fall inside the accepted
//! window, which bounds how long a captured delivery stays presentable, and the
//! identifier must be admitted only once inside that window, which
//! [`ReplayGuard`] enforces for the receiver.
//!
//! Deliveries also carry `layerx-webhook-delivery`, `layerx-webhook-attempt`,
//! `layerx-webhook-kind`, `layerx-webhook-subject`, `layerx-webhook-sequence`
//! and `layerx-webhook-endpoint`. Those are operational metadata outside the
//! signature: every value they carry is also present inside the signed body, and
//! a receiver applying side effects must read the signed body rather than trust
//! them.

use ed25519_dalek::{Signature, VerifyingKey};
use std::collections::BTreeMap;

use crate::encoding::{base64_encode, fixed_base64};
use crate::error::WebhookError;

/// Domain word carried in the delivered envelope.
pub const SCHEME_VERSION: &str = "LayerX/webhooks/v1";
/// Header carrying the event identifier receivers deduplicate on.
pub const ID_HEADER: &str = "layerx-webhook-id";
/// Header carrying the signature timestamp in whole seconds.
pub const TIMESTAMP_HEADER: &str = "layerx-webhook-timestamp";
/// Header carrying the identifier of the signing key.
pub const KEY_HEADER: &str = "layerx-webhook-key-id";
/// Header carrying the signature itself.
pub const SIGNATURE_HEADER: &str = "layerx-webhook-signature";
/// Header carrying the delivery identifier, outside the signature.
pub const DELIVERY_HEADER: &str = "layerx-webhook-delivery";
/// Header carrying the event family, outside the signature.
pub const KIND_HEADER: &str = "layerx-webhook-kind";
/// Header carrying the ordering subject, outside the signature.
pub const SUBJECT_HEADER: &str = "layerx-webhook-subject";
/// Header carrying the position inside the subject, outside the signature.
pub const SEQUENCE_HEADER: &str = "layerx-webhook-sequence";
/// Header carrying the one-based attempt counter, outside the signature.
pub const ATTEMPT_HEADER: &str = "layerx-webhook-attempt";
/// Header carrying the endpoint identifier, outside the signature.
pub const ENDPOINT_HEADER: &str = "layerx-webhook-endpoint";
/// Prefix of the signature header value.
pub const SIGNATURE_PREFIX: &str = "v1=";
/// Prefix of every issued signing key identifier.
pub const KEY_PREFIX: &str = "whk_";
/// Default accepted age of a presented signature, in whole seconds.
pub const DEFAULT_TOLERANCE_SECONDS: u64 = 300;
/// Accepted clock lead of a presented signature, in whole seconds.
pub const MAXIMUM_FUTURE_SKEW_SECONDS: u64 = 30;
/// Largest accepted identifier length, matching the shipped consumers.
pub const MAXIMUM_ID_LENGTH: usize = 255;
/// Largest accepted signing key identifier length.
pub const MAXIMUM_KEY_ID_LENGTH: usize = 64;
/// The receiver obligation this scheme places on developers.
pub const RECEIVER_OBLIGATION: &str = "Verify layerx-webhook-signature over \"<layerx-webhook-id>.<layerx-webhook-timestamp>.\" followed by the exact body bytes before reading the body, reject a timestamp outside the accepted window, and deduplicate on layerx-webhook-id before applying side effects.";

const PUBLIC_KEY_BYTES: usize = 32;
const SIGNATURE_BYTES: usize = 64;

/// Returns true when the value is a usable signing key identifier.
#[must_use]
pub fn valid_key_id(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= MAXIMUM_KEY_ID_LENGTH
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'_' | b'-'))
}

/// Returns true when the value is a usable deduplication identifier.
#[must_use]
pub fn valid_id(value: &str) -> bool {
    !value.is_empty() && value.len() <= MAXIMUM_ID_LENGTH && !value.contains('\0')
}

/// Parses one canonical unsigned integer, refusing leading zeros exactly as the
/// shipped consumers do.
///
/// # Errors
/// Returns [`WebhookError::SignatureRejected`] for a non-canonical integer.
pub fn parse_canonical_integer(value: &str) -> Result<u64, WebhookError> {
    let canonical = value == "0"
        || (!value.starts_with('0')
            && !value.is_empty()
            && value.bytes().all(|byte| byte.is_ascii_digit()));
    if !canonical {
        return Err(WebhookError::SignatureRejected);
    }
    value.parse().map_err(|_| WebhookError::SignatureRejected)
}

/// Builds the exact message this scheme signs.
#[must_use]
pub fn canonical_message(id: &str, timestamp: u64, payload: &[u8]) -> Vec<u8> {
    let prefix = format!("{id}.{timestamp}.");
    let mut message = Vec::with_capacity(prefix.len().saturating_add(payload.len()));
    message.extend_from_slice(prefix.as_bytes());
    message.extend_from_slice(payload);
    message
}

/// Encodes one signature as the header value receivers parse.
#[must_use]
pub fn signature_header(signature: &[u8; SIGNATURE_BYTES]) -> String {
    format!("{SIGNATURE_PREFIX}{}", base64_encode(signature))
}

/// Everything a receiver needs to verify one presented delivery.
#[derive(Clone, Copy, Debug)]
pub struct Presentation<'a> {
    /// The received `layerx-webhook-id` value.
    pub id: &'a str,
    /// The received `layerx-webhook-timestamp` value, still as text.
    pub timestamp: &'a str,
    /// The received `layerx-webhook-key-id` value.
    pub key_id: &'a str,
    /// The received `layerx-webhook-signature` value.
    pub signature: &'a str,
    /// The exact received body bytes.
    pub payload: &'a [u8],
    /// Receiver clock in whole seconds.
    pub now: u64,
    /// Accepted signature age in whole seconds.
    pub tolerance_seconds: u64,
}

/// The delivery identity a verified signature commits to.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Verified {
    /// The deduplication identifier the signature committed to.
    pub id: String,
    /// The signature timestamp in whole seconds.
    pub timestamp: u64,
    /// The key identifier that verified the signature.
    pub key_id: String,
}

/// Verifies one presented delivery against the public keys a receiver accepts.
///
/// # Errors
/// Returns [`WebhookError::StaleTimestamp`] when the timestamp is outside the
/// accepted window and [`WebhookError::SignatureRejected`] when the identifiers
/// are malformed, no accepted key carries the presented key identifier, or the
/// signature does not verify.
pub fn verify(
    presentation: &Presentation<'_>,
    keys: &BTreeMap<String, [u8; PUBLIC_KEY_BYTES]>,
) -> Result<Verified, WebhookError> {
    if !valid_id(presentation.id) || !valid_key_id(presentation.key_id) {
        return Err(WebhookError::SignatureRejected);
    }
    let timestamp = parse_canonical_integer(presentation.timestamp)?;
    if timestamp.saturating_sub(presentation.now) > MAXIMUM_FUTURE_SKEW_SECONDS
        || presentation.now.saturating_sub(timestamp) > presentation.tolerance_seconds
    {
        return Err(WebhookError::StaleTimestamp);
    }
    let encoded = presentation
        .signature
        .strip_prefix(SIGNATURE_PREFIX)
        .ok_or(WebhookError::SignatureRejected)?;
    let signature =
        fixed_base64::<SIGNATURE_BYTES>(encoded).map_err(|_| WebhookError::SignatureRejected)?;
    let public_key = keys
        .get(presentation.key_id)
        .ok_or(WebhookError::SignatureRejected)?;
    let verifying =
        VerifyingKey::from_bytes(public_key).map_err(|_| WebhookError::SignatureRejected)?;
    let message = canonical_message(presentation.id, timestamp, presentation.payload);
    verifying
        .verify_strict(&message, &Signature::from_bytes(&signature))
        .map_err(|_| WebhookError::SignatureRejected)?;
    Ok(Verified {
        id: presentation.id.to_owned(),
        timestamp,
        key_id: presentation.key_id.to_owned(),
    })
}

/// Bounded receiver-side replay protection over delivery identifiers.
///
/// A repeat inside the window is reported as [`WebhookError::ReplayRejected`],
/// which a receiver answers with an accepting status and no side effect: an
/// at-least-once repeat is a duplicate to swallow, never a delivery to refuse,
/// because refusing it would keep the sender retrying.
#[derive(Clone, Debug)]
pub struct ReplayGuard {
    tolerance_seconds: u64,
    capacity: usize,
    seen: BTreeMap<String, u64>,
}

impl ReplayGuard {
    /// Creates a guard that remembers identifiers for the tolerance window and
    /// refuses to admit once the bound is reached.
    ///
    /// # Errors
    /// Returns [`WebhookError::InvalidRequest`] when either bound is zero.
    pub fn new(tolerance_seconds: u64, capacity: usize) -> Result<Self, WebhookError> {
        if tolerance_seconds == 0 || capacity == 0 {
            return Err(WebhookError::InvalidRequest);
        }
        Ok(Self {
            tolerance_seconds,
            capacity,
            seen: BTreeMap::new(),
        })
    }

    /// Admits one identifier exactly once inside the replay window.
    ///
    /// # Errors
    /// Returns [`WebhookError::StaleTimestamp`] outside the window,
    /// [`WebhookError::ReplayRejected`] for an identifier already admitted, and
    /// [`WebhookError::ReplayCapacity`] when the guard cannot deduplicate.
    pub fn admit(&mut self, id: &str, timestamp: u64, now: u64) -> Result<(), WebhookError> {
        if timestamp.saturating_sub(now) > MAXIMUM_FUTURE_SKEW_SECONDS
            || now.saturating_sub(timestamp) > self.tolerance_seconds
        {
            return Err(WebhookError::StaleTimestamp);
        }
        let horizon = now.saturating_sub(self.tolerance_seconds);
        self.seen.retain(|_, admitted| *admitted >= horizon);
        if self.seen.contains_key(id) {
            return Err(WebhookError::ReplayRejected);
        }
        if self.seen.len() >= self.capacity {
            return Err(WebhookError::ReplayCapacity);
        }
        self.seen.insert(id.to_owned(), timestamp);
        Ok(())
    }

    /// Returns how many identifiers the guard is currently remembering.
    #[must_use]
    pub fn remembered(&self) -> usize {
        self.seen.len()
    }
}
