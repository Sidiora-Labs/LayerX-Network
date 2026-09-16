//! Verified receipt retrieval and receipt-only unknown resolution.

use std::thread;

use layerx_proof::receipt::{
    verify, verify_sequencer_signature, AuthorizedBatch, VerificationFailure, VerifiedReceipt,
};
use layerx_proof::receipt::{
    verify_native_owner_outcome, NativeOwnerOutcomeContext, NativeOwnerOutcomeFailure,
};

use crate::client::ReconnectPolicy;
use crate::lni::refusal::decode_core_refusal;
use crate::lni::schema::{decode_envelope, encode_envelope, Envelope, SchemaError, Version};
use crate::lni::transport::{FrameTransport, TransportError};
use crate::submit::Unknown;
use layerx_types::result::ResultCode;
use layerx_wire::receipt::Receipt;

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

/// Server-side wait boundary for one exact activity receipt.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ReceiptWaitMode {
    /// Return the current receipt or canonical absence immediately.
    Immediate,
    /// Wait for execution and publication through the request deadline.
    Published,
    /// Wait for a durable receipt or its completed publication.
    Durable,
}

/// Authenticated receipt lookup context derived from the accepted handshake.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct AuthenticatedLookupContext {
    pub interface_version: Version,
    pub correlation_id: u64,
    pub sequencer_public_key: [u8; 32],
    pub wait_mode: ReceiptWaitMode,
}

/// Canonical receipt bytes authenticated by the handshake-pinned sequencer.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AuthenticatedReceipt {
    canonical_bytes: Vec<u8>,
    receipt: Receipt,
    activity_id: [u8; 32],
    global_sequence: u64,
    module_id: u16,
    result_code: ResultCode,
}

impl AuthenticatedReceipt {
    /// Borrows the exact canonical signed receipt returned by core.
    #[must_use]
    pub fn canonical_bytes(&self) -> &[u8] {
        &self.canonical_bytes
    }

    /// Borrows the decoded receipt whose canonical encoding and sequencer
    /// signature were verified.
    #[must_use]
    pub const fn receipt(&self) -> &Receipt {
        &self.receipt
    }

    /// Returns the exact activity identifier bound by the receipt.
    #[must_use]
    pub const fn activity_id(&self) -> [u8; 32] {
        self.activity_id
    }

    /// Returns the receipt's committed global sequence.
    #[must_use]
    pub const fn global_sequence(&self) -> u64 {
        self.global_sequence
    }

    /// Returns the protocol module which produced the receipt.
    #[must_use]
    pub const fn module_id(&self) -> u16 {
        self.module_id
    }

    /// Returns the exact lossless protocol result code.
    #[must_use]
    pub const fn result_code(&self) -> ResultCode {
        self.result_code
    }
}

/// One authenticated activity lookup result.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum AuthenticatedLookup {
    /// No receipt existed at the instant of an immediate lookup.
    Absent,
    /// A requested wait completed without a receipt before its deadline.
    TimedOut,
    /// The exact canonical receipt was authenticated independently.
    Verified(AuthenticatedReceipt),
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
    NativeOwnerVerification(NativeOwnerOutcomeFailure),
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
    InvalidCorrelation,
    InterfaceVersion(Version),
    InvalidActivityId,
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
    lookup_verified(transport, selector, context, |bytes| {
        verify(bytes, &context.authorised_batch).map_err(ReceiptError::Verification)
    })
}

/// Retrieves an owner module outcome bound to an independently retained signing request.
///
/// # Errors
/// Refuses malformed transport, selector mismatches and every activity, owner,
/// signature, receipt, root and fee binding failure.
pub fn lookup_native_owner(
    transport: &mut dyn FrameTransport,
    selector: ReceiptSelector,
    context: LookupContext,
    expected: &NativeOwnerOutcomeContext<'_>,
) -> Result<Lookup, ReceiptError> {
    lookup_verified(transport, selector, context, |bytes| {
        verify_native_owner_outcome(bytes, &context.authorised_batch, expected)
            .map_err(ReceiptError::NativeOwnerVerification)
    })
}

/// Looks up one exact activity receipt with handshake-pinned sequencer
/// authentication and an explicit server-side wait policy.
///
/// This operation sends exactly one request and receives exactly one response.
/// It never polls, retries, or re-submits an activity.
///
/// # Errors
///
/// Refuses unsupported wait modes, malformed correlation, transport/schema
/// failures, typed core refusals, invalid sequencer signatures, and receipts
/// for any activity other than the exact requested identifier.
pub fn lookup_authenticated(
    transport: &mut dyn FrameTransport,
    activity_id: [u8; 32],
    context: AuthenticatedLookupContext,
) -> Result<AuthenticatedLookup, ReceiptError> {
    if context.correlation_id == 0 {
        return Err(ReceiptError::InvalidCorrelation);
    }
    if activity_id == [0; 32] {
        return Err(ReceiptError::InvalidActivityId);
    }
    let mut selector = Vec::with_capacity(34);
    selector.push(1);
    selector.extend_from_slice(&activity_id);
    match (context.interface_version.minor, context.wait_mode) {
        (minor, ReceiptWaitMode::Immediate) if minor >= Version::V1_6.minor => selector.push(0),
        (minor, ReceiptWaitMode::Published) if minor >= Version::V1_5.minor => selector.push(1),
        (minor, ReceiptWaitMode::Durable) if minor >= Version::V1_6.minor => selector.push(2),
        (_, ReceiptWaitMode::Immediate) => {}
        _ => return Err(ReceiptError::InterfaceVersion(context.interface_version)),
    }
    if context.interface_version.major != Version::V1_0.major {
        return Err(ReceiptError::InterfaceVersion(context.interface_version));
    }
    let request = encode_envelope(Envelope {
        version: context.interface_version,
        message_tag: RECEIPT_LOOKUP_REQUEST_TAG,
        correlation_id: context.correlation_id,
        canonical_payload: &selector,
        proof_material: &[],
    })?;
    transport.send(&request)?;
    let response_bytes = transport.receive()?;
    let response = decode_envelope(&response_bytes)?;
    if response.version != context.interface_version
        || response.correlation_id != context.correlation_id
    {
        return Err(ReceiptError::UnexpectedResponse);
    }
    if response.message_tag == ERROR_RESPONSE_TAG {
        if !response.proof_material.is_empty() {
            return Err(ReceiptError::UnexpectedResponse);
        }
        let refusal = decode_core_refusal(response.canonical_payload)
            .ok_or(ReceiptError::UnexpectedResponse)?;
        return Err(ReceiptError::CoreRefusal {
            class: refusal.class,
            result: refusal.result,
        });
    }
    if response.message_tag != RECEIPT_LOOKUP_RESPONSE_TAG {
        return Err(ReceiptError::UnexpectedResponse);
    }
    if response.canonical_payload.is_empty() {
        return Ok(match context.wait_mode {
            ReceiptWaitMode::Immediate => AuthenticatedLookup::Absent,
            ReceiptWaitMode::Published | ReceiptWaitMode::Durable => AuthenticatedLookup::TimedOut,
        });
    }
    let receipt =
        verify_sequencer_signature(response.canonical_payload, context.sequencer_public_key)
            .map_err(ReceiptError::Verification)?;
    let protocol = receipt
        .protocol()
        .ok_or(ReceiptError::Verification(VerificationFailure {
            check: layerx_proof::receipt::ReceiptCheck::ReceiptShape,
        }))?;
    if protocol.activity_id() != activity_id {
        return Err(ReceiptError::ActivityMismatch {
            expected: activity_id,
            actual: protocol.activity_id(),
        });
    }
    let global_sequence = protocol.global_sequence();
    let module_id = protocol.module_id();
    let result_code = ResultCode::from_raw(protocol.result_code());
    Ok(AuthenticatedLookup::Verified(AuthenticatedReceipt {
        canonical_bytes: response.canonical_payload.to_vec(),
        receipt,
        activity_id,
        global_sequence,
        module_id,
        result_code,
    }))
}

fn lookup_verified(
    transport: &mut dyn FrameTransport,
    selector: ReceiptSelector,
    context: LookupContext,
    authenticate: impl FnOnce(&[u8]) -> Result<VerifiedReceipt, ReceiptError>,
) -> Result<Lookup, ReceiptError> {
    let mut selector_bytes = selector.encode();
    if context.interface_version.minor >= 5 {
        selector_bytes.push(1);
    }
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
    let verified = authenticate(response.canonical_payload)?;
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
    if context.interface_version.minor >= 5 {
        if policy.maximum_attempts == 0 {
            return Ok(Resolution::Unknown(unknown.after_resolution_attempts(0)));
        }
        return match lookup(transport, selector, context) {
            Ok(Lookup::Verified(receipt)) => Ok(Resolution::Resolved(receipt)),
            Ok(Lookup::Absent) | Err(ReceiptError::Transport(_)) => {
                Ok(Resolution::Unknown(unknown.after_resolution_attempts(1)))
            }
            Err(error) => Err(error),
        };
    }
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
