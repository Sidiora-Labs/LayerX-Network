use layerx_crypto::{ed25519, SignatureMessage};
use layerx_types::activity::{Authority, EnvelopeBuilder, Signature, TimestampBound};
use layerx_types::amount::Amount;
use layerx_types::ids::{Did, IdempotencyKey};
use layerx_types::payload::{ModuleRegistry, Payload};

use crate::canonical::{self, Activity, Domain};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum OwnerActivityError {
    Encoding,
    Binding,
    Signature,
}

pub struct OwnerEnvelopeContext {
    pub actor: Did,
    pub owner_public_key: [u8; 32],
    pub network_id: u32,
    pub account_sequence: u64,
    pub not_before_ms: u64,
    pub not_after_ms: u64,
    pub action_key: [u8; 32],
    pub fee_limit: u128,
}

/// # Errors
/// Refuses invalid native owner envelope fields or undisclosable typed payloads.
pub fn unsigned_native(
    compiled: &crate::CompiledIntent,
    context: &OwnerEnvelopeContext,
    registry: &ModuleRegistry,
) -> Result<(Vec<u8>, layerx_crypto::disclosure::Disclosure), OwnerActivityError> {
    let invalid = |_| OwnerActivityError::Encoding;
    let mut builder = EnvelopeBuilder::new();
    builder.protocol_version(3).map_err(invalid)?;
    builder.network_id(context.network_id).map_err(invalid)?;
    builder
        .activity_type(compiled.activity_type())
        .map_err(invalid)?;
    builder.actor_did(context.actor.clone()).map_err(invalid)?;
    builder
        .authority(Authority::owner(&context.owner_public_key).map_err(invalid)?)
        .map_err(invalid)?;
    builder
        .account_sequence(context.account_sequence)
        .map_err(invalid)?;
    builder
        .timestamp_bound(
            TimestampBound::new(context.not_before_ms, context.not_after_ms).map_err(invalid)?,
        )
        .map_err(invalid)?;
    builder
        .idempotency_key(IdempotencyKey::new(context.action_key))
        .map_err(invalid)?;
    builder
        .fee_limit(Amount::from_u128(context.fee_limit))
        .map_err(invalid)?;
    builder
        .payload_hash(compiled.payload_hash())
        .map_err(invalid)?;
    builder
        .payload(compiled.payload().clone())
        .map_err(invalid)?;
    let envelope = builder.build().map_err(invalid)?;
    let bytes =
        canonical::unsigned_envelope_bytes(&envelope).map_err(|_| OwnerActivityError::Encoding)?;
    let disclosure = layerx_crypto::disclosure::bind(&bytes, registry)
        .map_err(|_| OwnerActivityError::Binding)?;
    Ok((bytes, disclosure))
}

/// # Errors
/// Refuses malformed canonical bytes, mismatched payload hashes or invalid owner signatures.
pub fn verify(bytes: &[u8], registry: &ModuleRegistry) -> Result<Activity, OwnerActivityError> {
    let activity = canonical::decode_signed_activity(bytes, registry)
        .map_err(|_| OwnerActivityError::Encoding)?;
    if canonical::signed_activity_bytes(&activity).map_err(|_| OwnerActivityError::Encoding)?
        != bytes
        || canonical::payload_hash(&activity).map_err(|_| OwnerActivityError::Encoding)?
            != activity.payload_hash()
    {
        return Err(OwnerActivityError::Binding);
    }
    let owner = activity
        .authority()
        .try_into()
        .map_err(|_| OwnerActivityError::Binding)?;
    let signature = activity
        .signature()
        .ok_or(OwnerActivityError::Signature)?
        .try_into()
        .map_err(|_| OwnerActivityError::Signature)?;
    let unsigned =
        canonical::unsigned_activity_bytes(&activity).map_err(|_| OwnerActivityError::Encoding)?;
    let message = SignatureMessage::new(
        Domain::SignaturePreimage,
        activity.protocol_version(),
        activity.network_id(),
        &unsigned,
    )
    .map_err(|_| OwnerActivityError::Signature)?;
    ed25519::verify(owner, signature, message).map_err(|_| OwnerActivityError::Signature)?;
    Ok(activity)
}

/// # Errors
/// Refuses a changed unsigned envelope, owner key mismatch or invalid returned signature.
pub fn attach_signature(
    unsigned: &[u8],
    signature: [u8; 64],
    owner: [u8; 32],
    registry: &ModuleRegistry,
) -> Result<Vec<u8>, OwnerActivityError> {
    let activity = canonical::decode_unsigned_activity(unsigned, registry)
        .map_err(|_| OwnerActivityError::Encoding)?;
    if activity.authority() != owner {
        return Err(OwnerActivityError::Binding);
    }
    let invalid = |_| OwnerActivityError::Encoding;
    let mut builder = EnvelopeBuilder::new();
    builder
        .protocol_version(activity.protocol_version())
        .map_err(invalid)?;
    builder.network_id(activity.network_id()).map_err(invalid)?;
    builder
        .activity_type(activity.activity_type())
        .map_err(invalid)?;
    builder
        .actor_did(Did::new(activity.actor_did()).map_err(|_| OwnerActivityError::Encoding)?)
        .map_err(invalid)?;
    builder
        .authority(Authority::owner(&owner).map_err(invalid)?)
        .map_err(invalid)?;
    builder
        .account_sequence(activity.account_sequence())
        .map_err(invalid)?;
    builder
        .timestamp_bound(
            TimestampBound::new(
                activity.timestamp_bound().not_before,
                activity.timestamp_bound().not_after,
            )
            .map_err(invalid)?,
        )
        .map_err(invalid)?;
    builder
        .idempotency_key(IdempotencyKey::new(activity.idempotency_key()))
        .map_err(invalid)?;
    builder
        .fee_limit(Amount::from_u128(activity.fee_limit()))
        .map_err(invalid)?;
    builder
        .payload_hash(activity.payload_hash())
        .map_err(invalid)?;
    builder
        .payload(
            Payload::new(registry, activity.activity_type(), activity.payload())
                .map_err(|_| OwnerActivityError::Encoding)?,
        )
        .map_err(invalid)?;
    let envelope = builder.build().map_err(invalid)?;
    if canonical::unsigned_envelope_bytes(&envelope).map_err(|_| OwnerActivityError::Encoding)?
        != unsigned
    {
        return Err(OwnerActivityError::Binding);
    }
    let signed = envelope.attach_signature(Signature::new(&signature).map_err(invalid)?);
    let bytes =
        canonical::signed_envelope_bytes(&signed).map_err(|_| OwnerActivityError::Encoding)?;
    verify(&bytes, registry)?;
    Ok(bytes)
}
