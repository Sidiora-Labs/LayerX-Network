//! x402 v2 buyer role: strict offer selection, payment construction through a
//! typed `LayerX` boundary, extension echoing, and local receipt capture.

use base64::engine::general_purpose::STANDARD;
use base64::Engine as _;
use layerx_interop_gateway::trace::{TraceId, Traced};
use layerx_proof::merkle::leaf_hash;
use layerx_proof::receipt::{verify, AuthorizedBatch};
use serde::Deserialize;
use serde_json::Value;

use crate::codec::{decode_header, encode_header};
use crate::model::{
    PaymentPayload, PaymentRequired, PaymentRequirements, SettlementResponse, X402Error,
    X402_VERSION,
};

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SupportedKind {
    pub scheme: String,
    pub network: String,
}

#[derive(Clone, Debug, PartialEq)]
pub struct PaymentBuildRequest {
    pub requirements: PaymentRequirements,
    pub idempotency_key: [u8; 32],
    pub trace: TraceId,
}

/// Boundary that constructs the scheme-specific payment through the plane's
/// typed authority. The buyer adapter never signs or invents payment bytes.
pub trait BuyerPaymentPlane {
    /// Constructs the scheme payload through the plane's authority.
    ///
    /// # Errors
    ///
    /// Returns a typed construction or policy refusal.
    fn construct(&mut self, request: PaymentBuildRequest) -> Result<Value, X402Error>;
}

#[derive(Clone, Debug, PartialEq)]
pub struct BuiltPayment {
    pub header: String,
    pub payload: PaymentPayload,
    pub idempotency_key: [u8; 32],
}

#[derive(Clone, Debug, PartialEq)]
pub struct CapturedSettlement {
    pub response: SettlementResponse,
    pub canonical_receipt: Vec<u8>,
    pub receipt_digest: [u8; 32],
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
struct LayerXEvidence {
    receipt: String,
    receipt_digest: String,
    verification_level: String,
    #[serde(default)]
    purpose_hash: Option<String>,
}

/// x402 buyer with a closed list of supported scheme/network pairs.
#[derive(Clone, Debug)]
pub struct Buyer {
    supported: Vec<SupportedKind>,
}

impl Buyer {
    /// Constructs a buyer with at least one bounded supported kind.
    ///
    /// # Errors
    ///
    /// Refuses an empty or duplicate support declaration.
    pub fn new(supported: Vec<SupportedKind>) -> Result<Self, X402Error> {
        if supported.is_empty() || supported.len() > 64 {
            return Err(X402Error::UnsupportedOffer);
        }
        for (index, kind) in supported.iter().enumerate() {
            if kind.scheme.is_empty()
                || kind.network.is_empty()
                || supported[..index].contains(kind)
            {
                return Err(X402Error::UnsupportedOffer);
            }
        }
        Ok(Self { supported })
    }

    /// Parses a `PAYMENT-REQUIRED` header, selects the first supported offer
    /// in seller order, and asks the real plane boundary to construct its
    /// scheme payload. Required extensions are echoed byte-for-value.
    ///
    /// # Errors
    ///
    /// Returns a trace-bound typed refusal for malformed or unsupported
    /// offers and plane construction failures.
    pub fn build_payment(
        &self,
        required_header: &str,
        idempotency_key: [u8; 32],
        plane: &mut impl BuyerPaymentPlane,
        trace: &TraceId,
    ) -> Result<BuiltPayment, Traced<X402Error>> {
        let fail = |error| trace.wrap(error);
        if idempotency_key == [0; 32] {
            return Err(fail(X402Error::InvalidPayload));
        }
        let required: PaymentRequired = decode_header(required_header).map_err(fail)?;
        required.validate().map_err(fail)?;
        let accepted = required
            .accepts
            .iter()
            .find(|requirements| {
                self.supported.iter().any(|kind| {
                    kind.scheme == requirements.scheme && kind.network == requirements.network
                })
            })
            .cloned()
            .ok_or_else(|| fail(X402Error::UnsupportedOffer))?;
        let scheme_payload = plane
            .construct(PaymentBuildRequest {
                requirements: accepted.clone(),
                idempotency_key,
                trace: trace.clone(),
            })
            .map_err(fail)?;
        if !scheme_payload.is_object() {
            return Err(fail(X402Error::InvalidPayload));
        }
        let payload = PaymentPayload {
            x402_version: X402_VERSION,
            resource: Some(required.resource),
            payload: scheme_payload,
            accepted,
            extensions: required.extensions,
        };
        payload.validate().map_err(fail)?;
        let header = encode_header(&payload).map_err(fail)?;
        Ok(BuiltPayment {
            header,
            payload,
            idempotency_key,
        })
    }

    /// Captures a `PAYMENT-RESPONSE` only after locally verifying the attached
    /// canonical `LayerX` receipt under the caller's authorised batch.
    ///
    /// # Errors
    ///
    /// Refuses pending/failed responses as success, missing or mismatched
    /// evidence, malformed receipts and unauthorised sequencer signatures,
    /// a grant settlement whose `purposeHash` is absent or is not the one the
    /// accepted offer carries, and an exact settlement carrying one.
    pub fn capture_settlement(
        response_header: &str,
        expected: &BuiltPayment,
        authorised_batch: &AuthorizedBatch,
        trace: &TraceId,
    ) -> Result<CapturedSettlement, Traced<X402Error>> {
        let fail = |error| trace.wrap(error);
        let response: SettlementResponse = decode_header(response_header).map_err(fail)?;
        response.validate_wire().map_err(fail)?;
        if !response.success {
            return Err(fail(X402Error::PaymentRefused));
        }
        let evidence_value = response
            .extensions
            .get("layerx")
            .cloned()
            .ok_or_else(|| fail(X402Error::EvidenceMissing))?;
        let evidence: LayerXEvidence =
            serde_json::from_value(evidence_value).map_err(|_| fail(X402Error::EvidenceMissing))?;
        if evidence.verification_level != "sequencer-signed"
            || evidence.purpose_hash
                != expected_purpose(&expected.payload.accepted).map_err(fail)?
        {
            return Err(fail(X402Error::EvidenceMismatch));
        }
        let canonical_receipt = STANDARD
            .decode(evidence.receipt.as_bytes())
            .map_err(|_| fail(X402Error::EvidenceMismatch))?;
        let verified = verify(&canonical_receipt, authorised_batch)
            .map_err(|_| fail(X402Error::EvidenceMismatch))?;
        let protocol = verified
            .receipt()
            .protocol()
            .ok_or_else(|| fail(X402Error::EvidenceMismatch))?;
        let (asset, recipient) = expected.payload.accepted.layerx_facts().map_err(fail)?;
        let payer = hex(&protocol.from());
        if response.network != expected.payload.accepted.network
            || response.amount != Some(expected.payload.accepted.amount)
            || protocol.asset() != asset
            || protocol.to() != recipient
            || protocol.amount() != expected.payload.accepted.amount.value()
            || response.payer.as_deref() != Some(payer.as_str())
        {
            return Err(fail(X402Error::EvidenceMismatch));
        }
        let receipt_digest =
            leaf_hash(verified.canonical_bytes()).map_err(|_| fail(X402Error::EvidenceMismatch))?;
        let digest_text = hex(&receipt_digest);
        if evidence.receipt_digest != digest_text
            || response.transaction != format!("lxp:{digest_text}")
        {
            return Err(fail(X402Error::EvidenceMismatch));
        }
        Ok(CapturedSettlement {
            response,
            canonical_receipt,
            receipt_digest,
        })
    }
}

/// The `purposeHash` a settlement of `accepted` must repeat: none for an
/// exact offer, and for a metered or subscription grant offer the 64 lowercase
/// hexadecimal characters of its challenge, which such an offer must carry.
fn expected_purpose(accepted: &PaymentRequirements) -> Result<Option<String>, X402Error> {
    if !matches!(accepted.scheme.as_str(), "metered" | "subscription") {
        return Ok(None);
    }
    accepted
        .extra
        .as_ref()
        .and_then(|extra| extra.pointer("/layerx/purposeHash"))
        .and_then(Value::as_str)
        .filter(|purpose| {
            purpose.len() == 64
                && purpose
                    .bytes()
                    .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
        })
        .map(|purpose| Some(purpose.to_owned()))
        .ok_or(X402Error::EvidenceMismatch)
}

fn hex(bytes: &[u8; 32]) -> String {
    const DIGITS: &[u8; 16] = b"0123456789abcdef";
    let mut output = String::with_capacity(64);
    for byte in bytes {
        output.push(char::from(DIGITS[usize::from(byte >> 4)]));
        output.push(char::from(DIGITS[usize::from(byte & 0x0f)]));
    }
    output
}
