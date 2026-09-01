//! Canonical signed and unsigned activity-envelope wire forms.

use layerx_types::activity::{Envelope, UnsignedEnvelope};
use layerx_types::payload::{ActivityType, ModuleRegistry};
use layerx_types::result::KnownResult;

use crate::decode::Decoder;
use crate::encode::Encoder;
use crate::limits::MAX_MESSAGE_BYTES;
use crate::WireError;

const STRUCTURE_TAG: u16 = 0x1001;
const SIGNED_FIELD_COUNT: u8 = 12;
const UNSIGNED_FIELD_COUNT: u8 = 11;
const MAX_DID_BYTES: usize = 255;
const MAX_PAYLOAD_BYTES: usize = 524_288;
const MAX_SIGNATURE_BYTES: usize = 128;

/// Timestamp interval carried by a canonical activity.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct TimestampBound {
    /// Inclusive lower bound.
    pub not_before: u64,
    /// Inclusive upper bound.
    pub not_after: u64,
}

/// An activity value obtainable only by decoding a canonical signed or unsigned
/// envelope.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Activity {
    protocol_version: u16,
    network_id: u32,
    kind: ActivityType,
    actor_did: Vec<u8>,
    authority: Vec<u8>,
    account_sequence: u64,
    timestamp_bound: TimestampBound,
    idempotency_key: [u8; 32],
    fee_limit: u128,
    payload_hash: [u8; 32],
    payload: Vec<u8>,
    signature: Option<Vec<u8>>,
}

impl Activity {
    /// Returns the protocol version decoded from the envelope field.
    #[must_use]
    pub const fn protocol_version(&self) -> u16 {
        self.protocol_version
    }

    /// Returns the protocol network decoded from the envelope.
    #[must_use]
    pub const fn network_id(&self) -> u32 {
        self.network_id
    }

    /// Returns the activity type decoded from the envelope.
    #[must_use]
    pub const fn activity_type(&self) -> ActivityType {
        self.kind
    }

    /// Borrows the actor DID decoded from the envelope.
    #[must_use]
    pub fn actor_did(&self) -> &[u8] {
        &self.actor_did
    }

    /// Borrows the exact protocol authority representation.
    #[must_use]
    pub fn authority(&self) -> &[u8] {
        &self.authority
    }

    /// Returns the core-derived account sequence.
    #[must_use]
    pub const fn account_sequence(&self) -> u64 {
        self.account_sequence
    }

    /// Returns the inclusive validity interval decoded from the envelope.
    #[must_use]
    pub const fn timestamp_bound(&self) -> TimestampBound {
        self.timestamp_bound
    }

    /// Returns the exact idempotency key decoded from the envelope.
    #[must_use]
    pub const fn idempotency_key(&self) -> [u8; 32] {
        self.idempotency_key
    }

    /// Returns the maximum protocol fee authorised by the envelope.
    #[must_use]
    pub const fn fee_limit(&self) -> u128 {
        self.fee_limit
    }

    /// Borrows canonical module payload bytes.
    #[must_use]
    pub fn payload(&self) -> &[u8] {
        &self.payload
    }

    /// Returns the payload digest carried by the envelope.
    #[must_use]
    pub const fn payload_hash(&self) -> [u8; 32] {
        self.payload_hash
    }

    /// Borrows a signature only when this is the signed form.
    #[must_use]
    pub fn signature(&self) -> Option<&[u8]> {
        self.signature.as_deref()
    }
}

/// Exact canonical bytes a signer is permitted to receive.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SigningBytes(Vec<u8>);

impl SigningBytes {
    /// Borrows the canonical unsigned envelope.
    #[must_use]
    pub fn as_bytes(&self) -> &[u8] {
        &self.0
    }
}

fn field(decoder: &mut Decoder<'_>, expected: u8) -> Result<(), WireError> {
    let offset = decoder.offset();
    let actual = decoder
        .tag(SIGNED_FIELD_COUNT)
        .map_err(|_| WireError::known(KnownResult::UnknownField, offset))?;
    if actual != expected {
        return Err(WireError::known(KnownResult::UnknownField, offset));
    }
    Ok(())
}

fn fixed_32(decoder: &mut Decoder<'_>) -> Result<[u8; 32], WireError> {
    let offset = decoder.offset();
    let bytes = decoder.bytes(32)?;
    bytes
        .try_into()
        .map_err(|_| WireError::known(KnownResult::NonCanonical, offset))
}

fn decode_internal(
    bytes: &[u8],
    registry: &ModuleRegistry,
    signed: bool,
) -> Result<Activity, WireError> {
    let mut decoder = Decoder::new(bytes, MAX_MESSAGE_BYTES);
    let envelope_version = decoder.structure_header_version(STRUCTURE_TAG)?;
    let expected_count = if signed {
        SIGNED_FIELD_COUNT
    } else {
        UNSIGNED_FIELD_COUNT
    };
    let count = decoder.u8()?;
    if count != expected_count {
        return Err(WireError::known(
            KnownResult::MalformedEnvelope,
            decoder.offset(),
        ));
    }
    field(&mut decoder, 1)?;
    let protocol_version = decoder.u16()?;
    if protocol_version != envelope_version {
        return Err(WireError::known(
            KnownResult::VersionUnsupported,
            decoder.offset(),
        ));
    }
    field(&mut decoder, 2)?;
    let network_id = decoder.u32()?;
    field(&mut decoder, 3)?;
    let raw_activity_type = decoder.u32()?;
    let activity_type = ActivityType::from_u32(raw_activity_type)
        .map_err(|_| WireError::known(KnownResult::UnknownActivity, decoder.offset()))?;
    if !registry.declares(activity_type) {
        return Err(WireError::known(
            KnownResult::UnknownActivity,
            decoder.offset(),
        ));
    }
    field(&mut decoder, 4)?;
    let actor_did = decoder.bytes_owned(MAX_DID_BYTES)?;
    field(&mut decoder, 5)?;
    let authority = decoder.bytes_owned(MAX_PAYLOAD_BYTES)?;
    field(&mut decoder, 6)?;
    let account_sequence = decoder.u64()?;
    field(&mut decoder, 7)?;
    let timestamp_bound = TimestampBound {
        not_before: decoder.u64()?,
        not_after: decoder.u64()?,
    };
    if timestamp_bound.not_after < timestamp_bound.not_before {
        return Err(WireError::known(
            KnownResult::MalformedEnvelope,
            decoder.offset(),
        ));
    }
    field(&mut decoder, 8)?;
    let idempotency_key = fixed_32(&mut decoder)?;
    field(&mut decoder, 9)?;
    let fee_limit = decoder.u128()?;
    field(&mut decoder, 10)?;
    let payload_hash = fixed_32(&mut decoder)?;
    field(&mut decoder, 11)?;
    let payload = decoder.bytes_owned(MAX_PAYLOAD_BYTES)?;
    let signature = if signed {
        field(&mut decoder, 12)?;
        Some(decoder.bytes_owned(MAX_SIGNATURE_BYTES)?)
    } else {
        None
    };
    decoder.finish()?;
    Ok(Activity {
        protocol_version,
        network_id,
        kind: activity_type,
        actor_did,
        authority,
        account_sequence,
        timestamp_bound,
        idempotency_key,
        fee_limit,
        payload_hash,
        payload,
        signature,
    })
}

/// Decodes a canonical signed activity and rejects every unknown field, version,
/// module, and activity type.
///
/// # Errors
///
/// Returns the exact typed protocol rejection and never unwinds on input bytes.
pub fn decode_signed(bytes: &[u8], registry: &ModuleRegistry) -> Result<Activity, WireError> {
    decode_internal(bytes, registry, true)
}

/// Decodes a canonical unsigned activity.
///
/// # Errors
///
/// Returns the exact typed protocol rejection and never accepts a signature.
pub fn decode_unsigned(bytes: &[u8], registry: &ModuleRegistry) -> Result<Activity, WireError> {
    decode_internal(bytes, registry, false)
}

fn encode_internal(activity: &Activity, signed: bool) -> Result<Vec<u8>, WireError> {
    let mut encoder = Encoder::new(MAX_MESSAGE_BYTES);
    encoder.structure_header_version(STRUCTURE_TAG, activity.protocol_version)?;
    encoder.u8(if signed {
        SIGNED_FIELD_COUNT
    } else {
        UNSIGNED_FIELD_COUNT
    })?;
    encoder.tag(1, SIGNED_FIELD_COUNT)?;
    encoder.u16(activity.protocol_version)?;
    encoder.tag(2, SIGNED_FIELD_COUNT)?;
    encoder.u32(activity.network_id)?;
    encoder.tag(3, SIGNED_FIELD_COUNT)?;
    encoder.u32(activity.kind.value())?;
    encoder.tag(4, SIGNED_FIELD_COUNT)?;
    encoder.bytes(&activity.actor_did, MAX_DID_BYTES)?;
    encoder.tag(5, SIGNED_FIELD_COUNT)?;
    encoder.bytes(&activity.authority, MAX_PAYLOAD_BYTES)?;
    encoder.tag(6, SIGNED_FIELD_COUNT)?;
    encoder.u64(activity.account_sequence)?;
    encoder.tag(7, SIGNED_FIELD_COUNT)?;
    encoder.u64(activity.timestamp_bound.not_before)?;
    encoder.u64(activity.timestamp_bound.not_after)?;
    encoder.tag(8, SIGNED_FIELD_COUNT)?;
    encoder.bytes(&activity.idempotency_key, 32)?;
    encoder.tag(9, SIGNED_FIELD_COUNT)?;
    encoder.u128(activity.fee_limit)?;
    encoder.tag(10, SIGNED_FIELD_COUNT)?;
    encoder.bytes(&activity.payload_hash, 32)?;
    encoder.tag(11, SIGNED_FIELD_COUNT)?;
    encoder.bytes(&activity.payload, MAX_PAYLOAD_BYTES)?;
    if signed {
        let Some(signature) = &activity.signature else {
            return Err(WireError::known(
                KnownResult::MalformedEnvelope,
                encoder.as_bytes().len(),
            ));
        };
        encoder.tag(12, SIGNED_FIELD_COUNT)?;
        encoder.bytes(signature, MAX_SIGNATURE_BYTES)?;
    }
    Ok(encoder.finish())
}

/// Encodes the exact eleven-field unsigned form used to derive signing input.
///
/// # Errors
///
/// Returns a typed limit error if a previously decoded value cannot fit the
/// protocol envelope budget.
pub fn encode_unsigned(activity: &Activity) -> Result<Vec<u8>, WireError> {
    encode_internal(activity, false)
}

/// Encodes a newly constructed typed unsigned envelope through the sole wire path.
///
/// # Errors
///
/// Returns a typed limit error when the envelope cannot fit the protocol
/// envelope budget.
pub fn encode_unsigned_envelope(envelope: &UnsignedEnvelope) -> Result<Vec<u8>, WireError> {
    encode_internal(&activity_from_unsigned(envelope), false)
}

/// Encodes the exact twelve-field submitted signed form.
///
/// # Errors
///
/// Refuses an unsigned value and returns typed bound failures.
pub fn encode_signed(activity: &Activity) -> Result<Vec<u8>, WireError> {
    encode_internal(activity, true)
}

/// Encodes a newly signed typed envelope through the sole wire path.
///
/// # Errors
///
/// Returns typed bound failures when the signed envelope violates a limit.
pub fn encode_signed_envelope(envelope: &Envelope) -> Result<Vec<u8>, WireError> {
    encode_internal(&activity_from_signed(envelope), true)
}

/// Derives the sole signer input from the canonical unsigned form.
///
/// # Errors
///
/// Returns a typed encoding error if the decoded value violates a bound.
pub fn signing_bytes(activity: &Activity) -> Result<SigningBytes, WireError> {
    encode_unsigned(activity).map(SigningBytes)
}

fn activity_from_unsigned(envelope: &UnsignedEnvelope) -> Activity {
    Activity {
        protocol_version: envelope.protocol_version(),
        network_id: envelope.network_id(),
        kind: envelope.activity_type(),
        actor_did: envelope.actor_did().as_bytes().to_vec(),
        authority: envelope.authority().as_bytes().to_vec(),
        account_sequence: envelope.account_sequence(),
        timestamp_bound: TimestampBound {
            not_before: envelope.timestamp_bound().not_before(),
            not_after: envelope.timestamp_bound().not_after(),
        },
        idempotency_key: envelope.idempotency_key().bytes(),
        fee_limit: envelope.fee_limit().value(),
        payload_hash: envelope.payload_hash(),
        payload: envelope.payload().as_bytes().to_vec(),
        signature: None,
    }
}

fn activity_from_signed(envelope: &Envelope) -> Activity {
    Activity {
        protocol_version: envelope.protocol_version(),
        network_id: envelope.network_id(),
        kind: envelope.activity_type(),
        actor_did: envelope.actor_did().as_bytes().to_vec(),
        authority: envelope.authority().as_bytes().to_vec(),
        account_sequence: envelope.account_sequence(),
        timestamp_bound: TimestampBound {
            not_before: envelope.timestamp_bound().not_before(),
            not_after: envelope.timestamp_bound().not_after(),
        },
        idempotency_key: envelope.idempotency_key().bytes(),
        fee_limit: envelope.fee_limit().value(),
        payload_hash: envelope.payload_hash(),
        payload: envelope.payload().as_bytes().to_vec(),
        signature: Some(envelope.signature().as_bytes().to_vec()),
    }
}
