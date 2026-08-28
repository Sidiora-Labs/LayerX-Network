//! Verified receipt retrieval and receipt-only unknown resolution.

use std::thread;

use layerx_proof::receipt::{verify, AuthorizedBatch, VerificationFailure, VerifiedReceipt};

use crate::client::ReconnectPolicy;
use crate::lni::schema::{decode_envelope, encode_envelope, Envelope, SchemaError, Version};
use crate::lni::refusal::decode_core_refusal;
use crate::lni::transport::{FrameTransport, TransportError};
use crate::submit::Unknown;

const RECEIPT_LOOKUP_REQUEST_TAG: u16 = 5;
const RECEIPT_LOOKUP_RESPONSE_TAG: u16 = 6;
const ERROR_RESPONSE_TAG: u16 = 25;

/// One and only one canonical receipt lookup key.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ReceiptSelector {
    ActivityId([u8; 32]),
    IdempotencyKey {
        idempotency_key: [u8; 32],
        expected_activity_id: [u8; 32],
    },
    GlobalSequence(u64),
}

impl ReceiptSelector {
    fn encode(self) -> Vec<u8> {
        match self {
            Self::ActivityId(identifier) => {
                let mut bytes = Vec::with_capacity(33);
                bytes.push(1);
                bytes.extend_from_slice(&identifier);
                bytes
            }
            Self::IdempotencyKey {
                idempotency_key, ..
            } => {
                let mut bytes = Vec::with_capacity(33);
                bytes.push(2);
                bytes.extend_from_slice(&idempotency_key);
                bytes
            }
            Self::GlobalSequence(sequence) => {
                let mut bytes = Vec::with_capacity(9);
                bytes.push(3);
                bytes.extend_from_slice(&sequence.to_be_bytes());
                bytes
            }
        }
    }
}

/// Fixed verification and correlation context for one receipt lookup.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct LookupContext {
    pub interface_version: Version,
    pub correlation_id: u64,
    pub authorised_batch: AuthorizedBatch,
}

/// A verified exact receipt or a canonical absence response.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum Lookup {
    Absent,
    Verified(Box<VerifiedReceipt>),
}

/// Receipt lookup or verification refusal.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ReceiptError {
    Transport(TransportError),
    Envelope(SchemaError),
    UnexpectedResponse,
    CoreRefusal {
        class: u8,
        result: layerx_types::result::ResultCode,
    },
    Verification(VerificationFailure),
    ActivityMismatch {
        expected: [u8; 32],
        actual: [u8; 32],
    },
    SequenceMismatch {
        expected: u64,
        actual: u64,
    },
    UnavailableCapability,
    Disconnected,
}

impl From<TransportError> for ReceiptError {
    fn from(value: TransportError) -> Self {
        Self::Transport(value)
    }
}

impl From<SchemaError> for ReceiptError {
    fn from(value: SchemaError) -> Self {
        Self::Envelope(value)
    }
}

/// The result of bounded receipt-only resolution.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum Resolution {
    Resolved(Box<VerifiedReceipt>),
    Unknown(Unknown),
}

/// Looks up and verifies one exact core-produced receipt.
///
/// # Errors
///
/// Refuses transport/schema failures, mismatched response identity, proof
/// verification failure, or a receipt that does not match the requested
/// activity/sequence.
pub fn lookup(
    transport: &mut dyn FrameTransport,
    selector: ReceiptSelector,
    context: LookupContext,
) -> Result<Lookup, ReceiptError> {
    let selector_bytes = selector.encode();
    let request = encode_envelope(Envelope {
        version: context.interface_version,
        message_tag: RECEIPT_LOOKUP_REQUEST_TAG,
        correlation_id: context.correlation_id,
        canonical_payload: &selector_bytes,
        proof_material: &[],
    })?;
    transport.send(&request)?;
    let response_bytes = transport.receive()?;
    let response = decode_envelope(&response_bytes)?;
    if response.version.major == context.interface_version.major
        && response.message_tag == ERROR_RESPONSE_TAG
        && response.correlation_id == context.correlation_id
        && response.proof_material.is_empty()
    {
        let refusal = decode_core_refusal(response.canonical_payload)
            .ok_or(ReceiptError::UnexpectedResponse)?;
        return Err(ReceiptError::CoreRefusal {
            class: refusal.class,
            result: refusal.result,
        });
    }
    if response.version.major != context.interface_version.major
        || response.message_tag != RECEIPT_LOOKUP_RESPONSE_TAG
        || response.correlation_id != context.correlation_id
    {
        return Err(ReceiptError::UnexpectedResponse);
    }
    if response.canonical_payload.is_empty() {
        return Ok(Lookup::Absent);
    }
    let verified = verify(response.canonical_payload, &context.authorised_batch)
        .map_err(ReceiptError::Verification)?;
    let Some(receipt) = verified.receipt().protocol() else {
        return Err(ReceiptError::Verification(VerificationFailure {
            check: layerx_proof::receipt::ReceiptCheck::ReceiptShape,
        }));
    };
    match selector {
        ReceiptSelector::ActivityId(expected)
        | ReceiptSelector::IdempotencyKey {
            expected_activity_id: expected,
            ..
        } if receipt.activity_id() != expected => {
            return Err(ReceiptError::ActivityMismatch {
                expected,
                actual: receipt.activity_id(),
            });
        }
        ReceiptSelector::GlobalSequence(expected) if receipt.global_sequence() != expected => {
            return Err(ReceiptError::SequenceMismatch {
                expected,
                actual: receipt.global_sequence(),
            });
        }
        ReceiptSelector::ActivityId(_)
        | ReceiptSelector::IdempotencyKey { .. }
        | ReceiptSelector::GlobalSequence(_) => {}
    }
    Ok(Lookup::Verified(Box::new(verified)))
}

/// Resolves an unknown submission only through bounded receipt lookups.
///
/// This routine never calls submission and therefore cannot create a new
/// activity, idempotency key, or account sequence.
///
/// # Errors
///
/// Refuses malformed, mismatched, or unverifiable receipt evidence. Transport
/// loss retains the first-class unknown state for a later connection.
pub fn resolve_unknown(
    transport: &mut dyn FrameTransport,
    unknown: &Unknown,
    mut context: LookupContext,
    policy: ReconnectPolicy,
) -> Result<Resolution, ReceiptError> {
    let selector = ReceiptSelector::IdempotencyKey {
        idempotency_key: unknown.idempotency_key(),
        expected_activity_id: unknown.activity_id(),
    };
    let base_correlation_id = context.correlation_id;
    let mut attempts = 0_u32;
    for attempt in 0..policy.maximum_attempts {
        if attempt != 0 {
            thread::sleep(policy.delay(attempt, "receipt-resolution"));
        }
        context.correlation_id = base_correlation_id
            .checked_add(u64::from(attempt))
            .ok_or(ReceiptError::UnexpectedResponse)?;
        attempts = attempts.saturating_add(1);
        match lookup(transport, selector, context) {
            Ok(Lookup::Verified(receipt)) => return Ok(Resolution::Resolved(receipt)),
            Ok(Lookup::Absent) => {}
            Err(ReceiptError::Transport(_)) => {
                return Ok(Resolution::Unknown(
                    unknown.after_resolution_attempts(attempts),
                ));
            }
            Err(error) => return Err(error),
        }
    }
    Ok(Resolution::Unknown(
        unknown.after_resolution_attempts(attempts),
    ))
}
