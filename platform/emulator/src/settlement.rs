//! The merchant settlement contract admitted by `POST /v1/settle`.
//!
//! The seller middleware posts its settlement request verbatim, so this module
//! parses exactly those fields, binds the presented evidence to the canonical
//! receipt it names, and reports the activity the settlement authority has to
//! resolve. Nothing here decides a settlement: the caller resolves the named
//! activity against its own ledger and renders the outcome with
//! [`settled`] or [`refused`].

use layerx_proof::receipt::AuthorizedBatch;
use serde::Deserialize;
use sha2::{Digest, Sha256};
use subtle::ConstantTimeEq;

const X402_VERSION: u16 = 2;
const IDEMPOTENCY_DOMAIN: &[u8] = b"LayerX/middleware/x402/idempotency\0";
const VERIFICATION_LEVEL: &str = "sequencer-signed";
const MAX_PRINCIPAL_BYTES: usize = 512;
const MAX_KEY_BYTES: usize = 256;
const MAX_RECEIPT_BYTES: usize = 256 * 1024;

/// The largest settlement request body the route accepts.
pub const MAX_REQUEST_BYTES: usize = 512 * 1024;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RequestDocument {
    principal: String,
    payload: PayloadDocument,
    requirements: serde_json::Value,
    #[serde(rename = "idempotencyKey")]
    idempotency_key: String,
    #[serde(rename = "requestDigest")]
    request_digest: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct PayloadDocument {
    #[serde(rename = "x402Version")]
    x402_version: u16,
    #[serde(default)]
    resource: Option<serde_json::Value>,
    payload: EvidenceDocument,
    accepted: serde_json::Value,
    #[serde(default)]
    extensions: Option<serde_json::Value>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct EvidenceDocument {
    receipt: String,
    #[serde(rename = "receiptDigest")]
    receipt_digest: String,
    #[serde(rename = "verificationLevel")]
    verification_level: String,
    #[serde(default, rename = "idempotencyKey")]
    idempotency_key: Option<String>,
    #[serde(default, rename = "purposeHash")]
    purpose_hash: Option<String>,
}

/// A settlement request bound to the exact canonical receipt it presents.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Claim {
    receipt: Vec<u8>,
    activity_id: [u8; 32],
    idempotency_key: String,
}

impl Claim {
    /// Borrows the canonical receipt bytes the merchant presented.
    #[must_use]
    pub fn receipt(&self) -> &[u8] {
        &self.receipt
    }

    /// Returns the activity the presented receipt names.
    #[must_use]
    pub const fn activity_id(&self) -> [u8; 32] {
        self.activity_id
    }
}

/// The exact contract check that refused a settlement request.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Refusal {
    /// The body is not the bounded settlement request the seller sends.
    Request,
    /// The evidence asks for a verification level this authority never grants.
    VerificationLevel,
    /// A digest, key or receipt field is not the bounded encoding it declares.
    Evidence,
    /// The declared receipt digest is not the digest of the presented receipt.
    ReceiptDigest,
    /// The idempotency key is not bound to this principal and request digest.
    IdempotencyBinding,
    /// The presented bytes are not one canonical protocol receipt.
    Receipt,
}

impl Refusal {
    /// Returns the machine-readable reason carried to the merchant.
    #[must_use]
    pub const fn reason(self) -> &'static str {
        match self {
            Self::Request => "invalid_settlement_request",
            Self::VerificationLevel => "unsupported_verification_level",
            Self::Evidence => "invalid_receipt_evidence",
            Self::ReceiptDigest => "receipt_digest_mismatch",
            Self::IdempotencyBinding => "idempotency_binding_mismatch",
            Self::Receipt => "invalid_canonical_receipt",
        }
    }
}

/// Parses one seller settlement request and binds it to its canonical receipt.
///
/// # Errors
/// Returns the exact contract, evidence, digest, binding or receipt check that
/// refused the request. No partially bound claim is returned.
pub fn claim(body: &[u8]) -> Result<Claim, Refusal> {
    if body.is_empty() || body.len() > MAX_REQUEST_BYTES {
        return Err(Refusal::Request);
    }
    let document: RequestDocument = serde_json::from_slice(body).map_err(|_| Refusal::Request)?;
    if document.principal.is_empty()
        || document.principal.len() > MAX_PRINCIPAL_BYTES
        || document.payload.x402_version != X402_VERSION
        || !document.requirements.is_object()
        || !document.payload.accepted.is_object()
        || !document.payload.resource.as_ref().is_none_or(is_object)
        || !document.payload.extensions.as_ref().is_none_or(is_object)
    {
        return Err(Refusal::Request);
    }
    let evidence = &document.payload.payload;
    if evidence.verification_level != VERIFICATION_LEVEL {
        return Err(Refusal::VerificationLevel);
    }
    let receipt_digest = hex32(&evidence.receipt_digest).ok_or(Refusal::Evidence)?;
    let request_digest = hex32(&document.request_digest).ok_or(Refusal::Evidence)?;
    let idempotency_key = hex32(&document.idempotency_key).ok_or(Refusal::Evidence)?;
    if evidence
        .purpose_hash
        .as_deref()
        .is_some_and(|value| hex32(value).is_none())
        || evidence
            .idempotency_key
            .as_deref()
            .is_some_and(|value| value.is_empty() || value.len() > MAX_KEY_BYTES)
    {
        return Err(Refusal::Evidence);
    }
    let receipt = base64_decode(&evidence.receipt)
        .filter(|bytes| !bytes.is_empty() && bytes.len() <= MAX_RECEIPT_BYTES)
        .ok_or(Refusal::Evidence)?;
    let presented = layerx_proof::merkle::leaf_hash(&receipt).map_err(|_| Refusal::Evidence)?;
    if presented.ct_eq(&receipt_digest).unwrap_u8() != 1 {
        return Err(Refusal::ReceiptDigest);
    }
    let mut binding = Sha256::new();
    binding.update(IDEMPOTENCY_DOMAIN);
    binding.update(document.principal.as_bytes());
    binding.update(request_digest);
    let bound: [u8; 32] = binding.finalize().into();
    if bound.ct_eq(&idempotency_key).unwrap_u8() != 1 {
        return Err(Refusal::IdempotencyBinding);
    }
    let decoded = layerx_wire::receipt::decode(&receipt).map_err(|_| Refusal::Receipt)?;
    let activity_id = decoded
        .protocol()
        .map(layerx_wire::receipt::ProtocolReceipt::activity_id)
        .ok_or(Refusal::Receipt)?;
    if activity_id == [0; 32] {
        return Err(Refusal::Receipt);
    }
    Ok(Claim {
        receipt,
        activity_id,
        idempotency_key: document.idempotency_key,
    })
}

/// Renders the settled result exactly as the seller middleware parses it.
#[must_use]
pub fn settled(claim: &Claim, canonical: &[u8], batch: &AuthorizedBatch) -> serde_json::Value {
    serde_json::json!({
        "state": "settled",
        "activity_id": hex(&claim.activity_id),
        "idempotency_key": claim.idempotency_key,
        "receipt_base64": base64_encode(canonical),
        "authorized_batch": {
            "batch_id": hex(&batch.batch_id()),
            "asset": hex(&batch.asset()),
            "previous_state_root": hex(&batch.previous_state_root()),
            "resulting_state_root": hex(&batch.resulting_state_root()),
            "sequencer_public_key": hex(&batch.sequencer_public_key()),
        },
    })
}

/// Renders a settlement this authority refuses, naming the exact reason.
#[must_use]
pub fn refused(reason: &str) -> serde_json::Value {
    serde_json::json!({ "state": "refused", "reason": reason })
}

fn is_object(value: &serde_json::Value) -> bool {
    value.is_object()
}

fn base64_decode(value: &str) -> Option<Vec<u8>> {
    let bytes = value.as_bytes();
    if !bytes.len().is_multiple_of(4) {
        return None;
    }
    let mut decoded = Vec::with_capacity(bytes.len() / 4 * 3);
    for (index, chunk) in bytes.chunks(4).enumerate() {
        let padding = chunk.iter().rev().take_while(|byte| **byte == b'=').count();
        if padding > 2
            || (padding != 0 && index + 1 != bytes.len() / 4)
            || chunk[..4 - padding].contains(&b'=')
        {
            return None;
        }
        let mut accumulator = 0_u32;
        for byte in &chunk[..4 - padding] {
            accumulator = (accumulator << 6) | u32::from(sextet(*byte)?);
        }
        accumulator <<= match padding {
            0 => 0,
            1 => 6,
            _ => 12,
        };
        let group = accumulator.to_be_bytes();
        match padding {
            0 => decoded.extend_from_slice(&group[1..4]),
            1 => {
                if group[3] != 0 {
                    return None;
                }
                decoded.extend_from_slice(&group[1..3]);
            }
            _ => {
                if group[2] != 0 || group[3] != 0 {
                    return None;
                }
                decoded.push(group[1]);
            }
        }
    }
    Some(decoded)
}

fn sextet(byte: u8) -> Option<u8> {
    match byte {
        b'A'..=b'Z' => Some(byte - b'A'),
        b'a'..=b'z' => Some(byte - b'a' + 26),
        b'0'..=b'9' => Some(byte - b'0' + 52),
        b'+' => Some(62),
        b'/' => Some(63),
        _ => None,
    }
}

fn base64_encode(bytes: &[u8]) -> String {
    const ALPHABET: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut encoded = String::with_capacity(bytes.len().div_ceil(3) * 4);
    for chunk in bytes.chunks(3) {
        let first = chunk[0];
        let second = chunk.get(1).copied().unwrap_or(0);
        let third = chunk.get(2).copied().unwrap_or(0);
        encoded.push(char::from(ALPHABET[usize::from(first >> 2)]));
        encoded.push(char::from(
            ALPHABET[usize::from(((first & 0x03) << 4) | (second >> 4))],
        ));
        if chunk.len() > 1 {
            encoded.push(char::from(
                ALPHABET[usize::from(((second & 0x0f) << 2) | (third >> 6))],
            ));
        } else {
            encoded.push('=');
        }
        if chunk.len() > 2 {
            encoded.push(char::from(ALPHABET[usize::from(third & 0x3f)]));
        } else {
            encoded.push('=');
        }
    }
    encoded
}

fn hex32(value: &str) -> Option<[u8; 32]> {
    if value.len() != 64 || !value.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return None;
    }
    let mut bytes = [0_u8; 32];
    for (index, pair) in value.as_bytes().chunks_exact(2).enumerate() {
        let text = std::str::from_utf8(pair).ok()?;
        bytes[index] = u8::from_str_radix(text, 16).ok()?;
    }
    Some(bytes)
}

fn hex(bytes: &[u8]) -> String {
    const DIGITS: &[u8; 16] = b"0123456789abcdef";
    let mut value = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        value.push(char::from(DIGITS[usize::from(byte >> 4)]));
        value.push(char::from(DIGITS[usize::from(byte & 0x0f)]));
    }
    value
}
